#!/usr/bin/env bash
# Builds the release artifact set locally and writes checksums, so a release
# can be assembled on a workstation when the hosted CI is unavailable and the
# docs build can publish real downloads.
#
# Output: target/release-artifacts/v<version>/ containing
#   - rustic-maps-sdk-android-v<version>.aar           (bindings + Rust + MapView)
#   - rustic-maps-sdk-ios-v<version>.xcframework.zip   (device + simulator)
#   - rustic-maps-sdk-web-v<version>.tgz               (the one WebAssembly
#                                                       package: npm-installable
#                                                       and statically servable)
#   - rustic-maps-sdk-rn-v<version>.tgz                (npm package with the
#                                                       prebuilt Android static
#                                                       archives and iOS framework)
#   - rustic-maps-sdk-rust-v<version>.tar.gz           (Rust workspace sources)
#   - rustic-maps-sdk-notices-v<version>.md            (third-party notices)
#   - one `.sha256` sidecar per file and SHA256SUMS.txt
#
# Every file is built from the current checkout by the same scripts the
# release workflow runs. Sigstore bundles are not produced here (they need
# the release workflow's OIDC identity); the downloads page states that.
#
# The Android package is one artifact: the generated Kotlin bindings, the
# per-ABI Rust libraries and the `MapView`/Compose wrapper. A gate below
# inspects the AAR and fails when any of the three is missing, so a wrapper
# without its bindings or a bindings archive without an ABI cannot ship.
#
# The React Native package is self-contained: its podspec vendors
# `AngoratekRusticMapsSdkFramework.xcframework` and its Gradle/CMake build
# imports `android/src/main/jniLibs/<abi>/librustic_maps.a`, so the assembly
# must carry both in the tarball. It reuses the platform release builds rather
# than rebuilding: the static archive `cargo build --release` already emits
# beside each Android ABI's cdylib, and the `rustic_mapsFFI.xcframework`
# `scripts/build-ios.sh` produces (renamed to the framework name the pod
# declares). A gate below reads the packaged podspec, CMakeLists.txt,
# build.gradle and package.json and fails when any artifact they name is
# absent from the tarball, or when the `files` allow-list would exclude one.
#
# Usage:
#   scripts/assemble-release-artifacts.sh
#   scripts/assemble-release-artifacts.sh --skip-android --skip-ios
#   scripts/assemble-release-artifacts.sh --skip-rust
#
# Prerequisites: the toolchains each build script checks (Rust targets, Xcode,
# Android SDK/NDK, Node/npm). The React Native package reuses the Android and
# iOS release builds, so its step needs those build outputs; missing tools or
# outputs abort the relevant step.
set -euo pipefail

readonly REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly WORKSPACE_VERSION="$(
    sed -n 's/^version = "\(.*\)"/\1/p' "${REPO_ROOT}/Cargo.toml" | head -n 1
)"
readonly OUTPUT_DIR="${REPO_ROOT}/target/release-artifacts/v${WORKSPACE_VERSION}"
# The React Native package's native payload is the platform release builds'
# output: the staticlib halves of the Android cdylib builds (cargo emits both
# crate types in one command) and the iOS XCFramework build-ios.sh packages.
# Android ABI -> Rust target, matching build-android.sh's SUPPORTED_ANDROID_ABIS.
readonly RN_ANDROID_STATIC_ARCHIVE_ABIS=(
    "arm64-v8a aarch64-linux-android"
    "armeabi-v7a armv7-linux-androideabi"
    "x86_64 x86_64-linux-android"
)
readonly RN_ANDROID_STATIC_ARCHIVE_NAME="librustic_maps.a"
readonly RN_IOS_VENDORED_FRAMEWORK_NAME="AngoratekRusticMapsSdkFramework.xcframework"
readonly RN_IOS_SOURCE_FRAMEWORK_PATH="${REPO_ROOT}/platform/ios/artifacts/rustic_mapsFFI.xcframework"
# The refapp-only binding crate's file-name marker (already asserted absent by
# the platform build scripts) plus the demo content that belongs to the
# reference applications, not the SDK: the demo scenes loader and its artifact
# records, the UX theme, the UX dimensions and the shared demo artifact
# resource (audit C10). The release assembly reads the artifact a consumer
# receives, so a regression at copy time still fails here.
readonly REFAPP_ONLY_ARTIFACT_MARKERS=(
    "map_refapp"
    "ReferenceScenesLoader"
    "ReferenceScenesArtifact"
    "ReferenceScenesCatalog"
    "ReferenceAppUXTheme"
    "ReferenceAppUXDimensions"
    "reference-scenes"
)

skip_android=false
skip_ios=false
skip_wasm=false
skip_rn=false
skip_rust=false
for argument in "$@"; do
    case "$argument" in
        --skip-android) skip_android=true ;;
        --skip-ios) skip_ios=true ;;
        --skip-wasm) skip_wasm=true ;;
        --skip-rn) skip_rn=true ;;
        --skip-rust) skip_rust=true ;;
        *) printf 'assemble-release-artifacts: unknown argument %s\n' "$argument" >&2; exit 1 ;;
    esac
done

log_step() {
    printf '\n==> %s\n' "$1"
}

fail() {
    printf 'assemble-release-artifacts: %s\n' "$1" >&2
    exit 1
}

# The Android artifact must carry all three halves: the generated Kotlin
# bindings, the wrapper classes, and one Rust library per supported ABI. The
# check reads the AAR a consumer receives, not the build directory it came
# from.
verify_android_package() {
    local aar_path="$1" inspection_directory
    inspection_directory="$(mktemp -d)"
    if ! unzip -q "${aar_path}" -d "${inspection_directory}"; then
        rm -rf "${inspection_directory}"
        fail "the Android AAR ${aar_path} cannot be unpacked"
    fi
    if [ ! -s "${inspection_directory}/classes.jar" ]; then
        rm -rf "${inspection_directory}"
        fail "the Android AAR carries no classes.jar"
    fi
    local class_listing
    class_listing="$(unzip -l "${inspection_directory}/classes.jar")"
    if ! printf '%s' "${class_listing}" | grep -q 'uniffi/rustic_maps/'; then
        rm -rf "${inspection_directory}"
        fail "the Android AAR carries no generated Kotlin bindings"
    fi
    if ! printf '%s' "${class_listing}" | grep -q 'com/rusticmaps/sdk/MapView.class'; then
        rm -rf "${inspection_directory}"
        fail "the Android AAR carries no MapView wrapper"
    fi
    # The published AAR carries SDK code only: no refapp-only binding crate,
    # demo scenes loader/model, UX theme, UX dimensions or demo artifact.
    local aar_listing marker
    aar_listing="$(unzip -l "${aar_path}")"
    for marker in "${REFAPP_ONLY_ARTIFACT_MARKERS[@]}"; do
        if printf '%s' "${aar_listing}" | grep -qi -- "${marker}" \
            || printf '%s' "${class_listing}" | grep -qi -- "${marker}"; then
            rm -rf "${inspection_directory}"
            fail "the Android AAR carries refapp-only content (${marker})"
        fi
    done
    local android_abi
    for android_abi in arm64-v8a armeabi-v7a x86_64; do
        if [ ! -s "${inspection_directory}/jni/${android_abi}/librustic_maps.so" ]; then
            rm -rf "${inspection_directory}"
            fail "the Android AAR is missing the ${android_abi} Rust library"
        fi
    done
    rm -rf "${inspection_directory}"
}

# The iOS archive is the XCFramework the SwiftPM binaryTarget consumes. It
# must carry the binary slices and none of the reference application's demo
# content; the Swift sources published beside it on the `swift-package` branch
# are asserted by scripts/build-ios.sh.
verify_ios_package() {
    local zip_path="$1" listing marker
    if ! listing="$(unzip -l "${zip_path}")"; then
        fail "the iOS XCFramework archive cannot be read: ${zip_path}"
    fi
    if ! printf '%s' "${listing}" | grep -q 'rustic_mapsFFI.xcframework/Info.plist'; then
        fail "the iOS archive carries no XCFramework"
    fi
    for marker in "${REFAPP_ONLY_ARTIFACT_MARKERS[@]}"; do
        if printf '%s' "${listing}" | grep -qi -- "${marker}"; then
            fail "the iOS XCFramework archive carries refapp-only content (${marker})"
        fi
    done
}

# Place the React Native payload where the generated podspec and CMake import
# it, from the artifacts the Android and iOS release builds already produce:
# no second Rust build, and the same binaries the AAR and XCFramework zip ship.
stage_rn_native_payload() {
    local abi_and_target android_abi rust_target source_archive destination_archive
    # The staging directory is rebuilt from the platform builds every time: a
    # leftover archive from an earlier build-rn.sh run must never ride into
    # the tarball, and the package's size is part of what it ships.
    rm -rf "${REPO_ROOT}/platform/rn/android/src/main/jniLibs"
    for abi_and_target in "${RN_ANDROID_STATIC_ARCHIVE_ABIS[@]}"; do
        android_abi="${abi_and_target%% *}"
        rust_target="${abi_and_target#* }"
        source_archive="${REPO_ROOT}/target/${rust_target}/release/${RN_ANDROID_STATIC_ARCHIVE_NAME}"
        destination_archive="${REPO_ROOT}/platform/rn/android/src/main/jniLibs/${android_abi}/${RN_ANDROID_STATIC_ARCHIVE_NAME}"
        if [ ! -s "${source_archive}" ]; then
            fail "the React Native Android payload is missing ${source_archive}; run scripts/build-android.sh (its release build emits the static archive beside the cdylib) and do not pass --skip-android"
        fi
        mkdir -p "$(dirname "${destination_archive}")"
        cp -f "${source_archive}" "${destination_archive}"
    done
    local destination_framework="${REPO_ROOT}/platform/rn/${RN_IOS_VENDORED_FRAMEWORK_NAME}"
    if [ ! -f "${RN_IOS_SOURCE_FRAMEWORK_PATH}/Info.plist" ]; then
        fail "the React Native iOS payload is missing ${RN_IOS_SOURCE_FRAMEWORK_PATH}; run scripts/build-ios.sh and do not pass --skip-ios"
    fi
    rm -rf "${destination_framework}"
    cp -R "${RN_IOS_SOURCE_FRAMEWORK_PATH}" "${destination_framework}"
}

# The RN tarball is self-contained only when every artifact its own manifests
# name is inside it. `scripts/verify-rn-package.mjs` unpacks the tarball and
# reads the packaged podspec, Gradle file, CMakeLists.txt and package.json, so
# the gate checks exactly what a consumer receives; the release workflow runs
# the same script after packaging.
verify_rn_package() {
    local tarball="$1"
    node "${REPO_ROOT}/scripts/verify-rn-package.mjs" "${tarball}" "${WORKSPACE_VERSION}"
}

mkdir -p "$OUTPUT_DIR"

if [ "$skip_android" = false ]; then
    log_step "building the one Android AAR"
    "${REPO_ROOT}/scripts/build-android.sh"
    # The retired wrapper AAR must never ride along from an earlier assembly
    # into this run's checksums and downloads manifest.
    rm -f "${OUTPUT_DIR}"/rustic-maps-sdk-android-ui-*.aar \
        "${OUTPUT_DIR}"/rustic-maps-sdk-android-ui-*.aar.sha256 \
        "${OUTPUT_DIR}"/rustic-maps-sdk-android-ui-*.aar.sigstore.json
    cp "${REPO_ROOT}/platform/android/maps-sdk/build/outputs/aar/maps-sdk-release.aar" \
        "${OUTPUT_DIR}/rustic-maps-sdk-android-v${WORKSPACE_VERSION}.aar"
    verify_android_package "${OUTPUT_DIR}/rustic-maps-sdk-android-v${WORKSPACE_VERSION}.aar"
fi

if [ "$skip_ios" = false ]; then
    log_step "building the iOS XCFramework"
    "${REPO_ROOT}/scripts/build-ios.sh"
    (
        cd "${REPO_ROOT}/platform/ios/artifacts"
        ditto -c -k --sequesterRsrc --keepParent rustic_mapsFFI.xcframework \
            "${OUTPUT_DIR}/rustic-maps-sdk-ios-v${WORKSPACE_VERSION}.xcframework.zip"
    )
    verify_ios_package "${OUTPUT_DIR}/rustic-maps-sdk-ios-v${WORKSPACE_VERSION}.xcframework.zip"
fi

if [ "$skip_wasm" = false ]; then
    log_step "building and packaging the one WebAssembly package"
    "${REPO_ROOT}/scripts/build-wasm.sh"
    "${REPO_ROOT}/scripts/package-web-npm.sh" --output-dir "$OUTPUT_DIR"
fi

if [ "$skip_rn" = false ]; then
    # The package must carry the prebuilt Android static archives and iOS
    # framework its own podspec and CMake import. Both come from the platform
    # release builds above, so the RN step adds no second Rust build.
    log_step "staging the React Native native payload from the Android and iOS builds"
    stage_rn_native_payload
    log_step "packaging the React Native npm package with its native payload"
    "${REPO_ROOT}/scripts/package-rn-npm.sh" \
        --output-dir "$OUTPUT_DIR" --with-native-payload
    verify_rn_package "${OUTPUT_DIR}/rustic-maps-sdk-rn-v${WORKSPACE_VERSION}.tgz"
fi

if [ "$skip_rust" = false ]; then
    log_step "archiving the Rust workspace sources"
    # The crates are path-linked, so a registry package cannot resolve them;
    # the archive is the release-commit source tree, unpacked and consumed by
    # path dependency (the Rust guide's install note says how).
    git -C "$REPO_ROOT" archive --format=tar.gz \
        --prefix="rustic-maps-sdk-rust-v${WORKSPACE_VERSION}/" \
        -o "${OUTPUT_DIR}/rustic-maps-sdk-rust-v${WORKSPACE_VERSION}.tar.gz" HEAD
fi

log_step "copying notices and writing checksums"
cp "${REPO_ROOT}/docs/THIRD_PARTY_NOTICES.md" \
    "${OUTPUT_DIR}/rustic-maps-sdk-notices-v${WORKSPACE_VERSION}.md"
(
    cd "$OUTPUT_DIR"
    shasum -a 256 ./* | grep -v '\.sha256' | grep -v 'SHA256SUMS.txt' \
        | sed 's|  \./|  |' | sort -k2 > SHA256SUMS.txt
    while read -r checksum file_name; do
        printf '%s  %s\n' "$checksum" "$file_name" > "${file_name}.sha256"
    done < SHA256SUMS.txt
)

# The manifest the docs build renders the downloads table from, so a new
# version never edits prose (name, size and checksum per artifact).
node - "$OUTPUT_DIR" "$WORKSPACE_VERSION" <<'NODE'
const { readFileSync, writeFileSync, readdirSync } = require('node:fs');
const { join } = require('node:path');
const [outputDirectory, version] = process.argv.slice(2);
const files = readdirSync(outputDirectory)
  .filter(name => !name.endsWith('.sha256') && name !== 'SHA256SUMS.txt' && name !== 'downloads.json')
  .sort()
  .map(name => ({
    name,
    bytes: Number(readFileSync(join(outputDirectory, name)).byteLength),
    sha256: readFileSync(join(outputDirectory, `${name}.sha256`), 'utf8').trim().split(/\s+/)[0],
  }));
writeFileSync(
  join(outputDirectory, 'downloads.json'),
  `${JSON.stringify({ version, files }, null, 2)}\n`,
);
NODE

printf '\nartifacts ready: %s\n' "$OUTPUT_DIR"
ls -1 "$OUTPUT_DIR"
