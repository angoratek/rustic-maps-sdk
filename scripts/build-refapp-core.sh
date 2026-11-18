#!/usr/bin/env bash
# Generate the reference applications' refapp-only UniFFI bindings.
#
# Pipeline: the pinned `uniffi-bindgen` (from `rustic-maps-native-core`, the
# same 0.31.2 release as the SDK contract) generates Kotlin and Swift from
# `crates/map-refapp-native/src/map_refapp_core.udl` -> the refapp-only Rust
# `cdylib`/`staticlib` builds of `map-refapp-native` are staged beside the
# generated sources -> Swift's two-slice XCFramework is joined for the iOS
# reference application.
#
# Outputs (all gitignored; never part of the published AAR or XCFramework):
#   - platform/android/reference-application/src/refapp-core/generated/
#       kotlin/   the generated Kotlin package `uniffi.map_refapp_core`
#       jniLibs/  `libmap_refapp_core.so` per Android ABI
#       host/     the host dylib the JVM unit suite loads
#   - platform/ios/reference-application/Generated/
#       RefappCore/                the generated Swift sources
#       refapp-core/...xcframework  the refapp-only static library
#
# Usage:
#   scripts/build-refapp-core.sh [all|android|ios]
#
# The default is `all`. `scripts/build-android.sh` and `scripts/build-ios.sh`
# remain the SDK packaging builds; this script never writes into their outputs.
#
# Toolchain prerequisites:
#   - Rust 1.99 with the Android/iOS `rust-std` targets (installed on demand).
#   - Android: `ANDROID_HOME` with the pinned NDK (any NDK 27+ works for the
#     API-24 linker binaries).
#   - iOS: Xcode (`xcodebuild`).
set -euo pipefail

readonly REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly BINDING_CRATE_NAME="map-refapp-native"
readonly BINDGEN_CRATE_NAME="rustic-maps-native-core"
readonly UDL_SOURCE="${REPO_ROOT}/crates/map-refapp-native/src/map_refapp_core.udl"
readonly UNIFFI_CONFIG="${REPO_ROOT}/crates/map-refapp-native/uniffi.toml"
# The contract-bound foreign library name: the UDL namespace, the Kotlin
# loader's name and the Swift FFI module all carry this spelling.
readonly FFI_LIBRARY_NAME="map_refapp_core"
readonly FFI_MODULE_NAME="map_refapp_coreFFI"
# The Rust library target's own name (`[lib] name` in the crate manifest).
readonly RUST_LIBRARY_NAME="map_refapp_native"

readonly ANDROID_APP_DIRECTORY="${REPO_ROOT}/platform/android/reference-application"
readonly ANDROID_GENERATED_ROOT="${ANDROID_APP_DIRECTORY}/src/refapp-core/generated"
readonly KOTLIN_GENERATED_SOURCES_DIRECTORY="${ANDROID_GENERATED_ROOT}/kotlin"
readonly ANDROID_JNILIBS_DIRECTORY="${ANDROID_GENERATED_ROOT}/jniLibs"
readonly ANDROID_HOST_LIBRARY_DIRECTORY="${ANDROID_GENERATED_ROOT}/host"
readonly ANDROID_NDK_VERSION="28.2.13676358"
readonly MIN_ANDROID_API_LEVEL="24"
readonly SUPPORTED_ANDROID_ABIS=(
    "arm64-v8a aarch64-linux-android aarch64-linux-android"
    "armeabi-v7a armv7-linux-androideabi armv7a-linux-androideabi"
    "x86_64 x86_64-linux-android x86_64-linux-android"
)

readonly IOS_APP_DIRECTORY="${REPO_ROOT}/platform/ios/reference-application"
readonly IOS_GENERATED_ROOT="${IOS_APP_DIRECTORY}/Generated"
readonly IOS_GENERATED_SOURCES_DIRECTORY="${IOS_GENERATED_ROOT}/RefappCore"
readonly IOS_XCFRAMEWORK_ROOT="${IOS_GENERATED_ROOT}/refapp-core"
readonly IOS_XCFRAMEWORK_PATH="${IOS_XCFRAMEWORK_ROOT}/${FFI_MODULE_NAME}.xcframework"
readonly IOS_DEVICE_TARGET="aarch64-apple-ios"
readonly IOS_SIMULATOR_TARGET="aarch64-apple-ios-sim"
readonly SUPPORTED_IOS_TARGETS=("${IOS_DEVICE_TARGET}" "${IOS_SIMULATOR_TARGET}")
readonly IOS_MINIMUM_OS_VERSION="17.0"

readonly HOST_CARGO_TARGET_TRIPLE="$(rustc -vV | awk '/^host:/ {print $2}')"
if [ "$(uname)" = "Darwin" ]; then
    readonly HOST_CDYLIB_SUFFIX="dylib"
else
    readonly HOST_CDYLIB_SUFFIX="so"
fi

readonly GENERATED_SWIFT_STAGING_DIRECTORY="$(mktemp -d)"
cleanup() {
    rm -rf "${GENERATED_SWIFT_STAGING_DIRECTORY}"
}
trap cleanup EXIT

usage() {
    echo "usage: $(basename "$0") [all|android|ios]" >&2
}

require_tool() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "error: required tool '$1' was not found on PATH" >&2
        exit 1
    fi
}

install_rust_target_if_missing() {
    local rust_target="$1"
    if ! rustup target list --installed | grep -qx "${rust_target}"; then
        rustup target add "${rust_target}"
    fi
}

run_bindgen() {
    local language="$1"
    local out_directory="$2"
    # Run the built generator directly. `cargo run` and, on some hosts, the
    # `cargo build` that precedes it can stall at process start with no CPU
    # use (the dynamic loader never hands control to the binary). The platform
    # build scripts produce `target/debug/uniffi-bindgen` from this same
    # pinned crate, so reuse that binary when it is present and build it only
    # when it is missing, as on a fresh CI runner.
    local bindgen_binary="${REPO_ROOT}/target/debug/uniffi-bindgen"
    if [ ! -x "${bindgen_binary}" ]; then
        (
            cd "${REPO_ROOT}"
            cargo build --quiet -p "${BINDGEN_CRATE_NAME}" --bin uniffi-bindgen
        )
    fi
    "${bindgen_binary}" generate \
        "${UDL_SOURCE}" \
        --config "${UNIFFI_CONFIG}" \
        --language "${language}" \
        --out-dir "${out_directory}" \
        --no-format
}

cargo_target_environment_name() {
    # `aarch64-linux-android` -> `CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER`.
    echo "CARGO_TARGET_$(echo "$1" | tr '[:lower:]-' '[:upper:]_')_LINKER"
}

cc_environment_name() {
    # `aarch64-linux-android` -> `CC_aarch64_linux_android`.
    echo "CC_$(echo "$1" | tr '-' '_')"
}

ar_environment_name() {
    # `aarch64-linux-android` -> `AR_aarch64_linux_android`.
    echo "AR_$(echo "$1" | tr '-' '_')"
}

build_android_cdylib() {
    local rust_target="$1"
    local ndk_triple="$2"
    local ndk_toolchain_bin_directory
    ndk_toolchain_bin_directory="$(ls -d "${ANDROID_HOME}/ndk/${ANDROID_NDK_VERSION}/toolchains/llvm/prebuilt/"*-x86_64"/bin" | head -n 1)"
    local linker_binary
    linker_binary="$(ls "${ndk_toolchain_bin_directory}/${ndk_triple}${MIN_ANDROID_API_LEVEL}-clang" | head -n 1)"
    if [ ! -x "${linker_binary}" ]; then
        echo "error: NDK linker binary not found: ${linker_binary}" >&2
        exit 1
    fi
    env \
        "$(cargo_target_environment_name "${rust_target}")=${linker_binary}" \
        "$(cc_environment_name "${rust_target}")=${linker_binary}" \
        "$(ar_environment_name "${rust_target}")=${ndk_toolchain_bin_directory}/llvm-ar" \
        cargo build --release -p "${BINDING_CRATE_NAME}" --target "${rust_target}"
}

generate_android_bindings() {
    echo "==> checking the Android toolchain"
    require_tool cargo
    require_tool rustup
    : "${ANDROID_HOME:?error: ANDROID_HOME must point at the Android SDK}"
    if [ ! -d "${ANDROID_HOME}/ndk/${ANDROID_NDK_VERSION}" ]; then
        echo "error: the pinned NDK ${ANDROID_NDK_VERSION} is not installed; run scripts/build-android.sh first" >&2
        exit 1
    fi

    echo "==> generating the refapp-only Kotlin bindings"
    rm -rf "${KOTLIN_GENERATED_SOURCES_DIRECTORY}"
    mkdir -p "${KOTLIN_GENERATED_SOURCES_DIRECTORY}"
    run_bindgen kotlin "${KOTLIN_GENERATED_SOURCES_DIRECTORY}"
    if [ ! -f "${KOTLIN_GENERATED_SOURCES_DIRECTORY}/uniffi/${FFI_LIBRARY_NAME}/${FFI_LIBRARY_NAME}.kt" ]; then
        echo "error: Kotlin generation did not produce the expected package" >&2
        exit 1
    fi

    echo "==> building the refapp-only Android cdylibs"
    rm -rf "${ANDROID_JNILIBS_DIRECTORY}"
    for abi_and_target in "${SUPPORTED_ANDROID_ABIS[@]}"; do
        local android_abi="$(echo "${abi_and_target}" | cut -d' ' -f1)"
        local rust_target="$(echo "${abi_and_target}" | cut -d' ' -f2)"
        local ndk_triple="$(echo "${abi_and_target}" | cut -d' ' -f3)"
        install_rust_target_if_missing "${rust_target}"
        build_android_cdylib "${rust_target}" "${ndk_triple}"
        local built_library="${REPO_ROOT}/target/${rust_target}/release/lib${RUST_LIBRARY_NAME}.so"
        if [ ! -f "${built_library}" ]; then
            echo "error: missing cdylib ${built_library}" >&2
            exit 1
        fi
        mkdir -p "${ANDROID_JNILIBS_DIRECTORY}/${android_abi}"
        cp "${built_library}" "${ANDROID_JNILIBS_DIRECTORY}/${android_abi}/lib${FFI_LIBRARY_NAME}.so"
    done

    echo "==> staging the host library for the JVM unit suite"
    rm -rf "${ANDROID_HOST_LIBRARY_DIRECTORY}"
    mkdir -p "${ANDROID_HOST_LIBRARY_DIRECTORY}"
    cargo build --release --quiet -p "${BINDING_CRATE_NAME}" --target "${HOST_CARGO_TARGET_TRIPLE}"
    local host_library="${REPO_ROOT}/target/${HOST_CARGO_TARGET_TRIPLE}/release/lib${RUST_LIBRARY_NAME}.${HOST_CDYLIB_SUFFIX}"
    if [ ! -f "${host_library}" ]; then
        echo "error: missing host cdylib ${host_library}" >&2
        exit 1
    fi
    cp "${host_library}" "${ANDROID_HOST_LIBRARY_DIRECTORY}/lib${FFI_LIBRARY_NAME}.${HOST_CDYLIB_SUFFIX}"
}

stage_static_framework() {
    # A static framework bundle keeps its Clang module in `Modules/`, so the
    # reference application links it beside the SDK's own XCFramework without
    # both modulemaps landing on the same flattened `include/module.modulemap`.
    local stage_directory="$1"
    local rust_target="$2"
    local supported_platform="$3"
    local framework="${stage_directory}/${FFI_MODULE_NAME}.framework"
    mkdir -p "${framework}/Headers" "${framework}/Modules"
    cp "${REPO_ROOT}/target/${rust_target}/release/lib${RUST_LIBRARY_NAME}.a" "${framework}/${FFI_MODULE_NAME}"
    cp "${GENERATED_SWIFT_STAGING_DIRECTORY}/${FFI_MODULE_NAME}.h" "${framework}/Headers/"
    cat > "${framework}/Modules/module.modulemap" <<MODULEMAP
framework module ${FFI_MODULE_NAME} {
    umbrella header "${FFI_MODULE_NAME}.h"
    export *
    module * { export * }
}
MODULEMAP
    cat > "${framework}/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleExecutable</key><string>${FFI_MODULE_NAME}</string>
  <key>CFBundleIdentifier</key><string>com.rusticmaps.sdk.referenceapplication.${FFI_MODULE_NAME}</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>${FFI_MODULE_NAME}</string>
  <key>CFBundlePackageType</key><string>FMWK</string>
  <key>CFBundleShortVersionString</key><string>1.0</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>MinimumOSVersion</key><string>${IOS_MINIMUM_OS_VERSION}</string>
  <key>CFBundleSupportedPlatforms</key><array><string>${supported_platform}</string></array>
</dict>
</plist>
PLIST
}

generate_ios_bindings() {
    echo "==> checking the iOS toolchain"
    require_tool cargo
    require_tool rustup
    require_tool xcodebuild

    echo "==> installing Rust iOS targets (no-op when present)"
    for rust_target in "${SUPPORTED_IOS_TARGETS[@]}"; do
        install_rust_target_if_missing "${rust_target}"
    done

    echo "==> building the refapp-only iOS static libraries"
    for rust_target in "${SUPPORTED_IOS_TARGETS[@]}"; do
        cargo build --release -p "${BINDING_CRATE_NAME}" --target "${rust_target}"
    done

    echo "==> generating the refapp-only Swift bindings"
    run_bindgen swift "${GENERATED_SWIFT_STAGING_DIRECTORY}"
    if [ ! -f "${GENERATED_SWIFT_STAGING_DIRECTORY}/${FFI_LIBRARY_NAME}.swift" ]; then
        echo "error: Swift generation did not produce the expected source" >&2
        exit 1
    fi
    rm -rf "${IOS_GENERATED_SOURCES_DIRECTORY}"
    mkdir -p "${IOS_GENERATED_SOURCES_DIRECTORY}"
    find "${GENERATED_SWIFT_STAGING_DIRECTORY}" -maxdepth 1 -name '*.swift' -exec cp {} "${IOS_GENERATED_SOURCES_DIRECTORY}/" \;

    echo "==> creating the refapp-only XCFramework"
    local device_stage="${IOS_XCFRAMEWORK_ROOT}/stage/${IOS_DEVICE_TARGET}"
    local simulator_stage="${IOS_XCFRAMEWORK_ROOT}/stage/${IOS_SIMULATOR_TARGET}"
    rm -rf "${IOS_XCFRAMEWORK_ROOT}"
    mkdir -p "${IOS_XCFRAMEWORK_ROOT}"
    stage_static_framework "${device_stage}" "${IOS_DEVICE_TARGET}" "iPhoneOS"
    stage_static_framework "${simulator_stage}" "${IOS_SIMULATOR_TARGET}" "iPhoneSimulator"
    xcodebuild -create-xcframework \
        -framework "${device_stage}/${FFI_MODULE_NAME}.framework" \
        -framework "${simulator_stage}/${FFI_MODULE_NAME}.framework" \
        -output "${IOS_XCFRAMEWORK_PATH}"
    rm -rf "${IOS_XCFRAMEWORK_ROOT}/stage"
}

main() {
    local mode="${1:-all}"
    case "${mode}" in
        all)
            generate_android_bindings
            generate_ios_bindings
            ;;
        android)
            generate_android_bindings
            ;;
        ios)
            generate_ios_bindings
            ;;
        *)
            usage
            exit 2
            ;;
    esac
    echo "done: refapp-only bindings generated"
}

main "$@"
