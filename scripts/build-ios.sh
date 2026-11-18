#!/usr/bin/env bash
# Build the iOS XCFramework for the Maps SDK native bindings.
#
# Pipeline: release `staticlib` builds of `rustic-maps-native-core` for
# `aarch64-apple-ios` (device) and `aarch64-apple-ios-sim` (simulator) ->
# uniffi Swift bindings generated from the metadata embedded in each static
# library -> the generated Swift sources land in platform/ios/Sources/
# RusticMapsSDK and the two static libraries plus their modulemaps are joined
# by `xcodebuild -create-xcframework` into
# platform/ios/artifacts/rustic_mapsFFI.xcframework (two slices) ->
# verification: `xcodebuild -scheme RusticMapsSDK -destination
# 'generic/platform=iOS Simulator' build`.
#
# The package, product and Swift module are named `RusticMapsSDK` (packaging
# identity, docs/Release.md (Versioning and identities)). The XCFramework module
# name `rustic_mapsFFI` is contract-bound: uniffi derives it from the UDL
# namespace, and the generated bindings import it through `canImport`.
#
# staticlib over cdylib: a static archive is consumed straight out of the
# XCFramework by Swift Package Manager binary targets with no run-time
# framework embedding; cdylib would require a `.framework`/`.dynlib` slice
# and a signing step for distribution. The generated Swift bindings link the
# FFI symbols through the provided modulemap.
#
# Toolchain prerequisites (validated):
#   - Rust 1.99.0 / cargo 1.99.0; script installs `rust-std` for
#     `aarch64-apple-ios` and `aarch64-apple-ios-sim` on demand.
#   - Xcode 26.6 (xcodebuild + swift), validated with Swift 6.3.3 but the
#     package is pinned to Swift 5 language mode (tools-version 5.10).
#   - Gradle-independent: nothing else.
#
# Idempotent: every step is a no-op or self-overwriting on re-run.
set -euo pipefail

readonly REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly BINDING_CRATE_NAME="rustic-maps-native-core"
readonly FFI_ARCHIVE_NAME="librustic_maps.a"
readonly FFI_MODULE_NAME="rustic_mapsFFI"
readonly UDL_LIBRARY_CRATE_NAME="rustic_maps"
readonly IOS_PACKAGE_ROOT="${REPO_ROOT}/platform/ios"
readonly XCFRAMEWORK_ROOT="${IOS_PACKAGE_ROOT}/artifacts"
readonly GENERATED_SOURCES_DIR="${IOS_PACKAGE_ROOT}/Sources/RusticMapsSDK"
readonly IOS_DEVICE_TARGET="aarch64-apple-ios"
readonly IOS_SIMULATOR_TARGET="aarch64-apple-ios-sim"
# The deployment floor the Swift package declares (`platform/ios/Package.swift`).
# Rust's iOS targets default to 10.0, which is older than the C dependencies'
# prebuilt objects (aws-lc and SQLite are compiled against the installed SDK)
# and makes the link fail on symbols those objects require.
readonly IOS_MINIMUM_DEPLOYMENT_TARGET="17.0"
readonly SUPPORTED_IOS_TARGETS=("${IOS_DEVICE_TARGET}" "${IOS_SIMULATOR_TARGET}")
readonly XCFRAMEWORK_PATH="${XCFRAMEWORK_ROOT}/${FFI_MODULE_NAME}.xcframework"
# Package-level notices (docs/Release.md (The artifacts)); copied into the
# XCFramework directory itself so the SPM binaryTarget archive keeps exactly
# one top-level item (the `.xcframework` directory).
readonly MIT_LICENSE_PATH="${REPO_ROOT}/LICENSE-MIT"
readonly APACHE_LICENSE_PATH="${REPO_ROOT}/LICENSE-APACHE"
readonly NOTICES_PATH="${REPO_ROOT}/docs/THIRD_PARTY_NOTICES.md"
# The disk-space required-reason declaration the SDK target ships; the
# generated sources directory is replaced on every run, so the committed
# manifest is installed into it (and into the XCFramework zip) here.
readonly PRIVACY_MANIFEST_PATH="${IOS_PACKAGE_ROOT}/PrivacyInfo.xcprivacy"
# The refapp-only binding crate's file-name marker. It must never appear in
# the published XCFramework or the generated SDK Swift sources: the demo
# policy stays in the reference application.
readonly REFAPP_ONLY_PACKAGE_MARKER="map_refapp"
# Refapp-only demo content the published package must not carry: the demo
# scenes loader/model, the UX theme, and the shared demo artifact resource all
# live in the reference application's own source set (audit C10).
readonly REFAPP_ONLY_DEMO_MARKERS=(
    "ReferenceScenesCatalog"
    "ReferenceAppUXTheme"
    "reference-scenes"
)

require_tool() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "error: required tool '$1' was not found on PATH" >&2
        exit 1
    fi
}

install_ios_target_if_missing() {
    local rust_target="$1"
    if ! rustup target list --installed | grep -qx "${rust_target}"; then
        rustup target add "${rust_target}"
    fi
}

build_ios_static_library() {
    local rust_target="$1"
    IPHONEOS_DEPLOYMENT_TARGET="${IOS_MINIMUM_DEPLOYMENT_TARGET}" \
        cargo build --release -p "${BINDING_CRATE_NAME}" --target "${rust_target}"
}

# Runs the pinned `uniffi-bindgen` from the workspace build. `cargo run` can
# stall at process start on some hosts with no CPU use; the binary built from
# the same pinned crate is equivalent, and a missing binary is built once.
run_pinned_uniffi_bindgen() {
    local bindgen_binary="${REPO_ROOT}/target/debug/uniffi-bindgen"
    if [ ! -x "${bindgen_binary}" ]; then
        cargo build --quiet --bin uniffi-bindgen
    fi
    "${bindgen_binary}" "$@"
}

generate_swift_bindings() {
    # One generation is enough: the metadata embedded in either static
    # library describes the same frozen UDL contract; the second target's
    # archive only differs by machine code. Generate from the device archive
    # and reuse for both slices.
    local device_archive="${REPO_ROOT}/target/${IOS_DEVICE_TARGET}/release/${FFI_ARCHIVE_NAME}"
    local generated_dir="${GEN_SWIFT_DIR}"
    # Run the built generator directly: on some hosts `cargo run` stalls at
    # process start with no CPU use, and the binary is produced by the same
    # workspace build that produced the archive above.
    run_pinned_uniffi_bindgen generate \
        --library "${device_archive}" \
        --language swift \
        --out-dir "${generated_dir}"
}

stage_headers_and_modulemaps() {
    # The XCFramework per-slice `Headers/` directory needs the FFI header and
    # a modulemap named `module.modulemap` (renamed from the generated
    # `<crate>FFI.modulemap`); the generated module name
    # (`rustic_mapsFFI`) is what the generated Swift imports via
    # `canImport`.
    local generated_dir="$1"
    local stage_dir="$2"
    mkdir -p "${stage_dir}/Headers"
    cp "${generated_dir}/${FFI_MODULE_NAME}.h" "${stage_dir}/Headers/"
    cp "${generated_dir}/${FFI_MODULE_NAME}.modulemap" "${stage_dir}/Headers/module.modulemap"
}

install_generated_swift_sources() {
    # The generation directory must already be exported in `GEN_SWIFT_DIR`.
    mkdir -p "${GENERATED_SOURCES_DIR}"
    rm -rf "${GENERATED_SOURCES_DIR}"/*
    find "${GEN_SWIFT_DIR}" -maxdepth 1 -name '*.swift' -exec cp {} "${GENERATED_SOURCES_DIR}/" \;
    cp "${PRIVACY_MANIFEST_PATH}" "${GENERATED_SOURCES_DIR}/PrivacyInfo.xcprivacy"
}

create_xcframework() {
    local device_stage="${XCFRAMEWORK_ROOT}/stage/${IOS_DEVICE_TARGET}"
    local simulator_stage="${XCFRAMEWORK_ROOT}/stage/${IOS_SIMULATOR_TARGET}"
    rm -rf "${XCFRAMEWORK_ROOT}/rustic_mapsFFI.xcframework" "${XCFRAMEWORK_ROOT}/stage"
    stage_headers_and_modulemaps "${GEN_SWIFT_DIR}" "${device_stage}"
    stage_headers_and_modulemaps "${GEN_SWIFT_DIR}" "${simulator_stage}"
    xcodebuild -create-xcframework \
        -library "${REPO_ROOT}/target/${IOS_DEVICE_TARGET}/release/${FFI_ARCHIVE_NAME}" \
        -headers "${device_stage}/Headers" \
        -library "${REPO_ROOT}/target/${IOS_SIMULATOR_TARGET}/release/${FFI_ARCHIVE_NAME}" \
        -headers "${simulator_stage}/Headers" \
        -output "${XCFRAMEWORK_ROOT}/rustic_mapsFFI.xcframework"
    rm -rf "${XCFRAMEWORK_ROOT}/stage"
}

embed_license_notices() {
    # The dual license files and the generated third-party notices ship in
    # every package. SPM requires the binaryTarget archive to hold exactly one
    # top-level item, so they live inside the `.xcframework` directory rather
    # than beside it; SPM reads the slices through Info.plist and ignores the
    # additional files.
    cp "${MIT_LICENSE_PATH}" "${APACHE_LICENSE_PATH}" "${NOTICES_PATH}" "${XCFRAMEWORK_PATH}/"
    # The privacy manifest travels beside the licences too, so the published
    # XCFramework archive carries the declaration even when it is consumed
    # outside the Swift package.
    cp "${PRIVACY_MANIFEST_PATH}" "${XCFRAMEWORK_PATH}/PrivacyInfo.xcprivacy"
}

sync_shared_provider_registry() {
    # The Swift package's MapNativeCoreUI resource bundle carries the shared
    # provider registry (`platform/providers.json`).
    # The copy is refreshed from its single source on every packaging run so
    # the wrapper's bundle can never drift from the file the other refapps
    # load. Demo content is not synced: the reference applications bundle
    # `platform/reference-scenes.json` from its single source themselves.
    local source_provider_registry="${REPO_ROOT}/platform/providers.json"
    local resource_dir="${IOS_PACKAGE_ROOT}/Sources/MapNativeCoreUI/Resources"
    mkdir -p "${resource_dir}"
    cp "${source_provider_registry}" "${resource_dir}/providers.json"
}

assert_refapp_core_absent_from_package() {
    # The XCFramework is the published binary and the generated Swift sources
    # are the published module: the refapp-only binding crate lives in the
    # reference application's own generated directory instead. Assert the
    # file sets before the package build consumes them.
    if find "${XCFRAMEWORK_PATH}" -iname "*${REFAPP_ONLY_PACKAGE_MARKER}*" -print -quit | grep -q .; then
        echo "error: the published XCFramework carries the refapp-only binding crate (${REFAPP_ONLY_PACKAGE_MARKER})" >&2
        exit 1
    fi
    if find "${GENERATED_SOURCES_DIR}" -maxdepth 1 -iname "*${REFAPP_ONLY_PACKAGE_MARKER}*" -print -quit | grep -q .; then
        echo "error: the published Swift sources carry the refapp-only binding crate (${REFAPP_ONLY_PACKAGE_MARKER})" >&2
        exit 1
    fi
}

assert_demo_content_absent_from_package() {
    # The demo scenes loader/model, the UX theme and the demo artifact are
    # reference-application code, not SDK code: assert the published file sets
    # (the XCFramework, the generated SDK Swift sources and the MapNativeCoreUI
    # wrapper sources the `swift-package` branch publishes) carry none of them.
    local inspected_path inspected_description marker inspected_path_and_description
    for inspected_path_and_description in \
        "${XCFRAMEWORK_PATH}|XCFramework" \
        "${GENERATED_SOURCES_DIR}|generated Swift sources" \
        "${IOS_PACKAGE_ROOT}/Sources/MapNativeCoreUI|MapNativeCoreUI wrapper sources"; do
        inspected_path="${inspected_path_and_description%%|*}"
        inspected_description="${inspected_path_and_description#*|}"
        for marker in "${REFAPP_ONLY_DEMO_MARKERS[@]}"; do
            if find "${inspected_path}" -iname "*${marker}*" -print -quit | grep -q .; then
                echo "error: the published ${inspected_description} carries refapp-only demo content (${marker})" >&2
                exit 1
            fi
            if grep -rIq -- "${marker}" "${inspected_path}"; then
                echo "error: the published ${inspected_description} carries refapp-only demo content (${marker})" >&2
                exit 1
            fi
        done
    done
}

prepare_package_source() {
    # Replace any previous binary target path with the freshly built one and
    # drop stray build caches, then build the package for the simulator.
    (cd "${IOS_PACKAGE_ROOT}" && rm -rf .build)
}

verify_package_build() {
    (cd "${IOS_PACKAGE_ROOT}" && xcodebuild -scheme RusticMapsSDK \
        -destination 'generic/platform=iOS Simulator' build 2>&1 | tail -5)
}

readonly GEN_SWIFT_DIR="$(mktemp -d)"

main() {
    echo "==> checking toolchain"
    require_tool cargo
    require_tool xcodebuild
    require_tool swift

    echo "==> installing Rust iOS targets (no-op when present)"
    for rust_target in "${SUPPORTED_IOS_TARGETS[@]}"; do
        install_ios_target_if_missing "${rust_target}"
    done

    echo "==> building release static libraries"
    for rust_target in "${SUPPORTED_IOS_TARGETS[@]}"; do
        build_ios_static_library "${rust_target}"
    done

    echo "==> generating Swift bindings from the frozen UDL"
    generate_swift_bindings
    install_generated_swift_sources

    echo "==> syncing the shared provider registry into the package resources"
    sync_shared_provider_registry

    echo "==> creating the XCFramework"
    create_xcframework

    echo "==> embedding the licenses and third-party notices in the XCFramework"
    embed_license_notices

    echo "==> asserting the refapp-only binding crate is absent from the package"
    assert_refapp_core_absent_from_package

    echo "==> asserting refapp-only demo content is absent from the package"
    assert_demo_content_absent_from_package

    echo "==> verifying the package compiles for the iOS Simulator"
    prepare_package_source
    verify_package_build

    echo "done: ${XCFRAMEWORK_ROOT}/rustic_mapsFFI.xcframework"
    find "${XCFRAMEWORK_ROOT}/rustic_mapsFFI.xcframework" -maxdepth 3 -print
}

main "$@"