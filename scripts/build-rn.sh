#!/usr/bin/env bash
# Build the React Native bindings for the Maps SDK, both platforms.
#
# Pipeline: npm install of the platform/rn workspace (ubrn, react, react-
# native, typescript, prettier for formatting) -> `ubrn generate jsi
# bindings` over the frozen UDL contract (TypeScript + JSI C++) -> `ubrn
# generate jsi turbo-module` (TurboModule glue: src/NativeMapsSdkRn.ts,
# src/index.ts, cpp/<package-name>.{h,cpp}) -> the two app-build inputs the
# ubrn TurboModule's Android build needs (the regenerated CMakeLists' missing
# `libandroid.so` link line, re-applied idempotently, plus the Rust static
# archives for the AAR's three ABIs) -> the iOS app-build inputs (the Rust
# xcframework the pod vendors via `ubrn build ios`, the codegen spec include
# re-pointed past the own-name header collision, the app shell's Pods and
# xcodeproj wiring, and the xcodebuild app build) -> typecheck with
# `tsc --noEmit`.
#
# Toolchain prerequisites (validated matrix):
#   - Node v25.8.2, npm 11.11.1.
#   - uniffi-bindgen-react-native 0.31.0-6 (devDependency; pinned in
#     package.json).
#   - react 19.3.1 / react-native 0.88.1 / typescript 5.9.3 act as the
#     type-level environment for the generated code; no Metro bundle, app
#     shell, or native runtime is produced or run by this script.
#   - Rust workspace present so the frozen UDL (crates/rustic-maps-native-core/
#     src/rustic_maps.udl) can be read and the static archives the two
#     native app builds link can be produced. The Android step requires
#     ANDROID_HOME with the NDK the Compose AAR pipeline pins
#     (scripts/build-android.sh's ANDROID_NDK_VERSION constants are kept in
#     sync by hand). The iOS steps require Xcode plus CocoaPods
#     (brew install cocoapods — no bundler Gemfile is pinned for the shell).
#
# Residual per-platform-manual steps (honest gaps, not forced):
#   - the app shell's JavaScript bundle is served by the developer's own
#     Metro (rnc-cli start --port 8081 from platform/rn/app-shell) — no
#     release bundling is produced here;
#   - the simulator install/launch drives (xcrun simctl) stay manual: the
#     booted standing simulator is a shared resource across rounds, the
#     script builds the app but never installs or launches it;
#   - the app builds keyless: a provider credential is entered at runtime in
#     the app's settings surface (no key enters the build or the Info.plist).
#
# Verification level: generation completing plus the TypeScript compiling is
# what is claimed; running the bindings requires the Rust cdylib loaded
# through the TurboModule in a React Native app shell, which is out of scope
# for this slice.
#
# Idempotent: every step is a no-op or self-overwriting on re-run.
set -euo pipefail

readonly REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly RN_PROJECT_ROOT="${REPO_ROOT}/platform/rn"
readonly BINDING_CRATE_DIR="${REPO_ROOT}/crates/rustic-maps-native-core"
readonly FROZEN_UDL_PATH="${BINDING_CRATE_DIR}/src/rustic_maps.udl"
readonly BINDING_CRATE_NAME="rustic-maps-native-core"
readonly UNIFFI_NAMESPACE_NAME="rustic_maps"
readonly GENERATED_TS_DIR="src/generated"
# `cpp/generated` for the JSI bindings (the TurboModule glue stays flat in
# `cpp/`): the generated `android/CMakeLists.txt` compiles
# `../cpp/generated/rustic_maps.cpp` next to the flat
# `../cpp/maps-sdk-rn.cpp`, so the uniffi-bindgen-react-native layout the
# CMake expects is `cpp/generated` for this step.
readonly GENERATED_CPP_DIR="cpp/generated"
readonly UBRN_COMMAND="${RN_PROJECT_ROOT}/node_modules/.bin/ubrn"
readonly TYPESCRIPT_COMMAND="${RN_PROJECT_ROOT}/node_modules/.bin/tsc"

generate_jsi_bindings() {
    # Both output directories hold nothing but generated sources, and their
    # file names derive from the contract namespace; the shared generator
    # wipes and rewrites them, and the jest/TypeScript CI gate calls the same
    # script, so a local run and CI cannot diverge.
    "${REPO_ROOT}/scripts/generate-rn-bindings.sh" --bindings
}

generate_turbo_module() {
    "${REPO_ROOT}/scripts/generate-rn-bindings.sh" --turbo-module
}

# ABI -> Rust target -> NDK triple prefix, matching scripts/build-android.sh's
# SUPPORTED_ANDROID_ABIS table. The owner ruling of 2026-10-29 is that the RN
# package ships the AAR's three ABIs, so the TurboModule's static archives
# cover the same set; the app shell still builds arm64-v8a only.
readonly RUST_STATIC_ARCHIVE_ANDROID_ARCHITECTURES=(
    "arm64-v8a aarch64-linux-android aarch64-linux-android"
    "armeabi-v7a armv7-linux-androideabi armv7a-linux-androideabi"
    "x86_64 x86_64-linux-android x86_64-linux-android"
)
readonly RUST_STATIC_ARCHIVE_NDK_VERSION="28.2.13676358"
# Kept in sync with the NDK toolchain API level scripts/build-android.sh pins
# (its MIN_ANDROID_API_LEVEL constant).
readonly RUST_STATIC_ARCHIVE_MIN_ANDROID_API_LEVEL="24"
readonly RUST_STATIC_ARCHIVE_JNI_LIBS_DIR="${RN_PROJECT_ROOT}/android/src/main/jniLibs"
readonly RUST_STATIC_ARCHIVE_NAME="librustic_maps.a"

cargo_target_linker_environment_name() {
    # `aarch64-linux-android` -> `CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER`.
    echo "CARGO_TARGET_$(echo "$1" | tr '[:lower:]-' '[:upper:]_')_LINKER"
}

cc_environment_name() {
    # `aarch64-linux-android` -> `CC_aarch64_linux_android`, the variable the
    # `cc` crate reads for C dependency builds.
    echo "CC_$(echo "$1" | tr '-' '_')"
}

ar_environment_name() {
    # `aarch64-linux-android` -> `AR_aarch64_linux_android`.
    echo "AR_$(echo "$1" | tr '-' '_')"
}

install_rust_target_if_missing() {
    local rust_target="$1"
    if command -v rustup >/dev/null 2>&1 \
        && ! rustup target list --installed | grep -qx "${rust_target}"; then
        rustup target add "${rust_target}"
    fi
}

ensure_rust_static_archive() {
    # The ubrn TurboModule's Android build links the Rust lib target as a
    # STATIC archive (its generated `CMakeLists.txt` imports
    # `${ANDROID_ABI}/librustic_maps.a`); the Compose AAR pipeline
    # (`scripts/build-android.sh`) produces only the cdylib, so this script
    # builds the archives itself with the very same NDK toolchain environment
    # pattern, for the same three ABIs the AAR ships.
    if ! command -v cargo >/dev/null 2>&1; then
        echo "error: required tool 'cargo' was not found on PATH" >&2
        exit 1
    fi
    : "${ANDROID_HOME:?error: ANDROID_HOME must point at the Android SDK for the TurboModule static archives}"
    if [ ! -d "${ANDROID_HOME}/ndk/${RUST_STATIC_ARCHIVE_NDK_VERSION}" ]; then
        echo "error: the Android NDK ${RUST_STATIC_ARCHIVE_NDK_VERSION} is missing under ANDROID_HOME" >&2
        exit 1
    fi
    local ndk_toolchain_bin_dir
    ndk_toolchain_bin_dir="$(ls -d "${ANDROID_HOME}/ndk/${RUST_STATIC_ARCHIVE_NDK_VERSION}/toolchains/llvm/prebuilt/"*-x86_64"/bin" | head -n 1)"
    local abi_and_target android_abi rust_target ndk_triple
    local linker_binary crate_static_archive archive_path
    for abi_and_target in "${RUST_STATIC_ARCHIVE_ANDROID_ARCHITECTURES[@]}"; do
        android_abi="$(echo "${abi_and_target}" | cut -d' ' -f1)"
        rust_target="$(echo "${abi_and_target}" | cut -d' ' -f2)"
        ndk_triple="$(echo "${abi_and_target}" | cut -d' ' -f3)"
        install_rust_target_if_missing "${rust_target}"
        linker_binary="${ndk_toolchain_bin_dir}/${ndk_triple}${RUST_STATIC_ARCHIVE_MIN_ANDROID_API_LEVEL}-clang"
        if [ ! -x "${linker_binary}" ]; then
            echo "error: NDK linker binary not found: ${linker_binary}" >&2
            exit 1
        fi
        echo "==> building the Rust static archive for ${android_abi}"
        env \
            "$(cargo_target_linker_environment_name "${rust_target}")=${linker_binary}" \
            "$(cc_environment_name "${rust_target}")=${linker_binary}" \
            "$(ar_environment_name "${rust_target}")=${ndk_toolchain_bin_dir}/llvm-ar" \
            cargo build --quiet --release -p "${BINDING_CRATE_NAME}" --target "${rust_target}"
        crate_static_archive="${REPO_ROOT}/target/${rust_target}/release/${RUST_STATIC_ARCHIVE_NAME}"
        if [ ! -f "${crate_static_archive}" ]; then
            echo "error: the static archive was not produced at ${crate_static_archive}" >&2
            exit 1
        fi
        archive_path="${RUST_STATIC_ARCHIVE_JNI_LIBS_DIR}/${android_abi}/${RUST_STATIC_ARCHIVE_NAME}"
        mkdir -p "$(dirname "${archive_path}")"
        cp -f "${crate_static_archive}" "${archive_path}"
    done
}

typecheck_generated_typescript() {
    (
        cd "${RN_PROJECT_ROOT}"
        "${TYPESCRIPT_COMMAND}" --noEmit
    )
}

# ---------------------------------------------------------------------------
# The iOS app-build inputs (mirroring the Android stage's shape)
# ---------------------------------------------------------------------------

readonly IOS_SHELL_DIR="${RN_PROJECT_ROOT}/app-shell/ios"
# `ubrn build ios` names the framework from the npm package identity
# (@angoratek/rustic-maps-sdk -> AngoratekRusticMapsSdkFramework); the
# generated podspec's `vendored_frameworks` points at the same name.
readonly RUST_XCFRAMEWORK_NAME="AngoratekRusticMapsSdkFramework.xcframework"
# The deployment floor the Swift package declares (`platform/ios/Package.swift`).
# Rust's iOS targets default to 10.0, older than the prebuilt C objects in the
# dependency graph (aws-lc), so the link fails without it.
readonly IOS_MINIMUM_DEPLOYMENT_TARGET="17.0"
# The build script's wire step (platform/rn/app-shell/ios/wire-ios-shell.rb)
# needs the `xcodeproj` Ruby gem; it ships inside CocoaPods' libexec bundle,
# where the Homebrew formula stages its gems.
readonly XCODEPROJ_GEM_STAGING_GLOB="${HOME}/Cellar/cocoapods/*/libexec/gems/*/lib"
readonly XCODEPROJ_GEM_STAGING_GLOB_DARWIN="${HOMEBREW_PREFIX:-/opt/homebrew}/Cellar/cocoapods/*/libexec/gems/*/lib"

# `ios/generated/rustic_maps.swift` is a committed symlink to the shared
# SwiftPM binding file, so the pod and the Swift package compile one generated
# copy. The target is a gitignored build artifact: the ubrn iOS build stages it
# under `ios/swift`, and `install_shared_swift_bindings` copies it to the
# symlink's target before the staging directory is removed, so a clean checkout
# produces the file the pod compiles.
readonly SHARED_SWIFT_BINDINGS_DIR="${REPO_ROOT}/platform/ios/Sources/RusticMapsSDK"
readonly GENERATED_SWIFT_BINDING_FILE_NAME="rustic_maps.swift"
readonly UBRN_IOS_NATIVE_BINDINGS_DIR="${RN_PROJECT_ROOT}/ios/swift"

install_shared_swift_bindings() {
    local staged_binding="${UBRN_IOS_NATIVE_BINDINGS_DIR}/${GENERATED_SWIFT_BINDING_FILE_NAME}"
    if [ ! -f "${staged_binding}" ]; then
        echo "error: the ubrn iOS build did not stage ${staged_binding}" >&2
        exit 1
    fi
    mkdir -p "${SHARED_SWIFT_BINDINGS_DIR}"
    cp -f "${staged_binding}" \
        "${SHARED_SWIFT_BINDINGS_DIR}/${GENERATED_SWIFT_BINDING_FILE_NAME}"
}

build_ios_rust_framework() {
    # The same `rustic-maps-native-core` release build the Android step archives is
    # packaged as the xcframework the pod vendors (`vendored_frameworks`
    # in the generated podspec). The app target must link exactly this ONE
    # archive: `scripts/build-ios.sh` packages the identical archive at
    # platform/ios/artifacts for the SwiftPM-based Compose reference app,
    # and the RN shell's Swift code compiles `rustic_maps.swift` (the
    # generated contract binding) against that xcframework's Headers
    # modulemap — modulemap without a link directive — so the symbols land
    # from the pod's vendored archive at link time. Never link both
    # archives into one app.
    #
    # `--native-bindings` stages the FFI header and modulemap into the
    # xcframework's per-slice `Headers/` directory. Without it the framework
    # is a bare archive and `check-rn-pod-swift.sh` (and a consumer's pod
    # build) has no module to import. The staging directory's Swift binding is
    # copied to the committed pod symlink's target and the staging directory
    # is then removed, so the pod compiles one generated binding from
    # `ios/generated/rustic_maps.swift`.
    (
        cd "${RN_PROJECT_ROOT}"
        IPHONEOS_DEPLOYMENT_TARGET="${IOS_MINIMUM_DEPLOYMENT_TARGET}" \
            "${UBRN_COMMAND}" build ios --config ubrn.config.yaml --release --native-bindings
        install_shared_swift_bindings
        rm -rf "${RN_PROJECT_ROOT}/ios/swift"
    )
}

# The ubrn-generated module header's codegen-spec include is re-pointed by
# the shared generator (`scripts/generate-rn-bindings.sh`), so every
# generation applies it — including a standalone package run.

find_pod_command() {
    if command -v pod >/dev/null 2>&1; then
        echo "pod"
        return 0
    fi
    if [ -x "/opt/homebrew/bin/pod" ]; then
        echo "/opt/homebrew/bin/pod"
        return 0
    fi
    echo "error: CocoaPods is missing (brew install cocoapods); the iOS app" \
        "build needs `pod install` over the app shell's Podfile" >&2
    return 1
}

install_ios_pods() {
    # The workspace resolves the MapsSdkRn pod through `use_native_modules!`
    # (the ubrn-packaged copy under app-shell/node_modules/maps-sdk-rn, a
    # symlink to platform/rn): unlike the Android Gradle path, which
    # consumes the repo workspace package through `project()`, the iOS pod
    # consumes the packaged copy, so this script's earlier re-pack of the
    # generated TypeScript and the vendored xcframework must land before
    # `pod install`.
    (cd "${IOS_SHELL_DIR}" && "$(find_pod_command)" install)
}

wire_ios_shell_xcode_project() {
    # The template app target's wiring (Swift sources, the uniffi-generated
    # rustic_maps.swift, the ObjC++ bridge glue, the demo artifact and
    # trace fixture resources, the per-sdk module search, the bundle id) —
    # idempotent. The wire script needs the `xcodeproj` gem; the brew
    # CocoaPods staging carries it (a plain `gem list xcodeproj` user
    # install also satisfies it — the load path is built from whichever is
    # found).
    local wire_script="${IOS_SHELL_DIR}/wire-ios-shell.rb"
    if [ ! -f "${wire_script}" ]; then
        echo "error: the iOS shell wiring script is missing: ${wire_script}" >&2
        exit 1
    fi
    local ruby_command="$(command -v ruby || echo /usr/bin/ruby)"
    local gem_load_paths=""
    if ! "${ruby_command}" -e 'require "xcodeproj"' >/dev/null 2>&1; then
        gem_load_paths="$(ls -d ${XCODEPROJ_GEM_STAGING_GLOB} 2>/dev/null | tr '\n' ':')" || true
        gem_load_paths="$(ls -d ${XCODEPROJ_GEM_STAGING_GLOB_DARWIN} 2>/dev/null | tr '\n' ':')" || true
    fi
    (cd "${IOS_SHELL_DIR}" && \
        "${ruby_command}" ${gem_load_paths:+-I"${gem_load_paths%:}"} \
        "${wire_script}" "MapsSdkRefappRn.xcodeproj")
}

# The Rust xcframework the iOS pod vendors is arm64-only (both slices: ubrn's
# release build produces a single-arch static archive per slice), so the app
# build pins the same arch — the exact mirror of the Android stage's
# arm64-v8a pin. A `generic/platform=iOS Simulator` destination otherwise
# builds x86_64 too, and the x86_64 link fails with undefined uniffi symbols
# (no x86_64 Rust archive exists to satisfy them).
readonly IOS_RUST_XCFRAMEWORK_ARCH="arm64"

build_ios_app_shell() {
    # The app build over the wired workspace. The app is keyless by design
    # (a provider credential is entered at runtime in the settings surface),
    # so no build setting carries one. A `generic/platform=iOS Simulator`
    # destination builds the app without needing a booted device; the
    # install/launch stays manual (see the header's residuals).
    (
        cd "${IOS_SHELL_DIR}"
        xcodebuild \
            -workspace MapsSdkRefappRn.xcworkspace \
            -scheme MapsSdkRefappRn \
            -configuration Debug \
            -destination 'generic/platform=iOS Simulator' \
            -derivedDataPath build/DerivedData \
            "ARCHS=${IOS_RUST_XCFRAMEWORK_ARCH}" \
            build
    )
}

main() {
    echo "==> installing the RN workspace dependencies (no-op when present)"
    npm install --prefix "${RN_PROJECT_ROOT}" --no-audit --no-fund

    echo "==> generating JSI bindings from the frozen UDL"
    generate_jsi_bindings

    echo "==> generating the TurboModule glue"
    generate_turbo_module

    echo "==> ensuring the Rust static archives the TurboModule Android build links"
    ensure_rust_static_archive

    echo "==> building the Rust xcframework the iOS pod vendors and its shared Swift binding"
    build_ios_rust_framework

    echo "==> type-checking the pod's Swift sources against the generated bindings"
    "${REPO_ROOT}/scripts/check-rn-pod-swift.sh"

    echo "==> installing the app shell's Pods"
    install_ios_pods

    echo "==> wiring the app shell's xcode project (idempotent)"
    wire_ios_shell_xcode_project

    echo "==> building the app shell for the iOS simulator"
    build_ios_app_shell

    echo "==> typechecking the generated TypeScript (no emit)"
    typecheck_generated_typescript

    echo "done:"
    find "${RN_PROJECT_ROOT}/src" "${RN_PROJECT_ROOT}/cpp" -type f -print | sort
}

main "$@"