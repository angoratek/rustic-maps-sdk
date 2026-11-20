#!/usr/bin/env bash
# Build the one Android AAR: the generated Kotlin bindings, the per-ABI Rust
# libraries and the `MapView`/Compose wrapper together.
#
# Pipeline: per-ABI Rust release builds of `rustic-maps-native-core` (cdylib)
# with the Android NDK clang linkers -> uniffi Kotlin bindings generated from
# the frozen UDL metadata embedded in the built cdylib -> the Kotlin sources
# and the per-ABI `librustic_maps.so` files land in the internal source-holder
# module (platform/android/maps-sdk-native) -> `gradle :maps-sdk:assembleRelease`
# compiles them into the one published AAR, `gradle :smoke-jvm:smokeRun`
# exercises the generated Kotlin against the host dylib, and
# `gradle :maps-sdk:testDebugUnitTest` runs the wrapper's unit suite over the
# same host dylib.
#
# Toolchain prerequisites (validated matrix):
#   - Rust 1.99.0 / cargo 1.99.0 with the `rust-std` components for the three
#     Android targets below (installed on demand by this script).
#   - ANDROID_HOME pointing at an SDK with `cmdline-tools/latest/bin`
#     (sdkmanager), `platforms/android-36` and an NDK
#     (`ndk;28.2.13676358` was used; any NDK 27+ works for the API-24
#     linker binaries).
#   - Java 21 (e.g. Temurin 21) for the Android Gradle Plugin; resolved via
#     /usr/libexec/java_home when present.
#   - Gradle 9.5.1 on PATH; the wrapper below is regenerated at the same
#     version.
# Validated plugin matrix: AGP 9.0.0, Kotlin Android plugin 2.2.21.
#
# Idempotent: every step is a no-op or self-overwriting on re-run.
set -euo pipefail

readonly REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly BINDING_CRATE_NAME="rustic-maps-native-core"
readonly BINDING_CRATE_FFI_LIBRARY_NAME="librustic_maps.so"
readonly GRADLE_ROOT="${REPO_ROOT}/platform/android"
readonly GRADLE_MODULE_PATH="${GRADLE_ROOT}/maps-sdk-native"
readonly KOTLIN_GENERATED_SOURCES_DIR="${GRADLE_MODULE_PATH}/src/main/kotlin"
readonly JNILIBS_DIR="${GRADLE_MODULE_PATH}/src/main/jniLibs"
# The published artifact is selected by its exact release name: a debug AAR
# left beside it must never be inspected or reported instead.
readonly RELEASE_AAR_PATH="${GRADLE_ROOT}/maps-sdk/build/outputs/aar/maps-sdk-release.aar"
# Package-level notices (docs/Release.md (The artifacts)). The module's
# build.gradle.kts adds this directory as an assets source, so the AAR carries
# `assets/licenses/{LICENSE-MIT,LICENSE-APACHE,THIRD_PARTY_NOTICES.md}`.
readonly LICENSE_NOTICES_ASSETS_DIR="${GRADLE_MODULE_PATH}/build/generated/sdk-licenses/licenses"
readonly MIT_LICENSE_PATH="${REPO_ROOT}/LICENSE-MIT"
readonly APACHE_LICENSE_PATH="${REPO_ROOT}/LICENSE-APACHE"
readonly NOTICES_PATH="${REPO_ROOT}/docs/THIRD_PARTY_NOTICES.md"
# The refapp-only binding crate's file-name marker. It must never appear in
# the published AAR: the demo policy stays in the reference application.
readonly REFAPP_ONLY_PACKAGE_MARKER="map_refapp"
# Refapp-only demo content the published AAR must not carry: the demo scenes
# loader and its artifact records, the UX theme, the UX dimensions and the
# shared demo artifact resource all live in the reference application module
# (audit C10).
readonly REFAPP_ONLY_DEMO_MARKERS=(
    "ReferenceScenesLoader"
    "ReferenceScenesArtifact"
    "ReferenceAppUXTheme"
    "ReferenceAppUXDimensions"
    "reference-scenes"
    # The demo/navigation classes now live in the reference application: a
    # registry decoder, the collecting navigation listener and their packages
    # must not ship in the published AAR.
    "ProviderRegistryLoader"
    "CollectingNavigationStateListener"
    "com/rusticmaps/sdk/demo/"
    "com/rusticmaps/sdk/navigation/"
)
readonly ANDROID_NDK_VERSION="28.2.13676358"
# Android 16 KB page-size devices map native libraries in 16 KB pages; every
# ELF LOAD segment must be aligned to at least this boundary. The NDK's LLVM
# linker emits it for API 24+ targets, and the assertion below keeps a
# toolchain change from silently regressing the published AAR.
readonly SIXTEEN_KILOBYTE_PAGE_ALIGNMENT=0x4000
# 16 KB page-size Android devices run 64-bit binaries only; the 32-bit
# armeabi-v7a slice is never loaded there, so its historical 4 KB alignment is
# not a regression. The symbol assertion below still covers every ABI.
readonly SIXTEEN_KILOBYTE_REQUIRED_ABIS=("arm64-v8a" "x86_64")
# The hand-written JNI shim in `librustic_maps.so` is bound to the exact Kotlin
# class name; a missing symbol is a runtime crash in the consumer, so the
# build asserts both exports exist per ABI.
readonly REQUIRED_JNI_SYMBOLS=(
    "Java_com_rusticmaps_sdk_NativeWindowBridge_nativeWindowPointerFromSurface"
    "Java_com_rusticmaps_sdk_NativeWindowBridge_releaseNativeWindow"
)
# Lowest Android API level the NDK linker binaries below target; kept in sync
# with `minSdk` in the module's build.gradle.kts.
readonly MIN_ANDROID_API_LEVEL="24"
# ABI -> Rust target -> NDK triple prefix (linker binary base name). Note the
# NDK spells the armv7 triple `armv7a-linux-androideabi` while the Rust
# target is `armv7-linux-androideabi`.
readonly SUPPORTED_ANDROID_ABIS=(
    "arm64-v8a aarch64-linux-android aarch64-linux-android"
    "armeabi-v7a armv7-linux-androideabi armv7a-linux-androideabi"
    "x86_64 x86_64-linux-android x86_64-linux-android"
)

require_tool() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "error: required tool '$1' was not found on PATH" >&2
        exit 1
    fi
}

install_android_target_if_missing() {
    local rust_target="$1"
    if ! rustup target list --installed | grep -qx "${rust_target}"; then
        rustup target add "${rust_target}"
    fi
}

cargo_target_environment_name() {
    # `aarch64-linux-android` -> `CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER`.
    echo "CARGO_TARGET_$(echo "$1" | tr '[:lower:]-' '[:upper:]_')_LINKER"
}

cc_environment_name() {
    # `aarch64-linux-android` -> `CC_aarch64_linux_android` (lowercase, dashes
    # underscored), the variable the `cc` crate reads for the C dependency
    # builds (e.g. `ring`) on this target; the NDK ships only the API-suffixed
    # compiler wrappers, so the plain `<triple>-clang` binary `cc` probes by
    # default does not exist.
    echo "CC_$(echo "$1" | tr '-' '_')"
}

ar_environment_name() {
    # `aarch64-linux-android` -> `AR_aarch64_linux_android` (same convention
    # as the `cc` crate's compiler variable above).
    echo "AR_$(echo "$1" | tr '-' '_')"
}

build_android_cdylib() {
    local rust_target="$1"
    local ndk_triple="$2"
    local ndk_toolchain_bin_dir
    # The NDK prebuilt directory name carries the host OS and its
    # architecture ("darwin-x86_64", "linux-x86_64"); the unquoted x86_64
    # glob covers both hosts this script runs on (local macOS, CI Linux).
    ndk_toolchain_bin_dir="$(ls -d "${ANDROID_HOME}/ndk/${ANDROID_NDK_VERSION}/toolchains/llvm/prebuilt/"*-x86_64"/bin" | head -n 1)"
    local linker_binary
    linker_binary="$(ls "${ndk_toolchain_bin_dir}/${ndk_triple}${MIN_ANDROID_API_LEVEL}-clang" | head -n 1)"
    if [ ! -x "${linker_binary}" ]; then
        echo "error: NDK linker binary not found: ${linker_binary}" >&2
        exit 1
    fi
    env \
        "$(cargo_target_environment_name "${rust_target}")=${linker_binary}" \
        "$(cc_environment_name "${rust_target}")=${linker_binary}" \
        "$(ar_environment_name "${rust_target}")=${ndk_toolchain_bin_dir}/llvm-ar" \
        cargo build --release -p "${BINDING_CRATE_NAME}" --target "${rust_target}"
}

readonly HOST_CARGO_TARGET_TRIPLE="$(rustc -vV | awk '/^host:/ {print $2}')"
readonly METADATA_CARGO_TARGET_TRIPLE="${HOST_CARGO_TARGET_TRIPLE}"
# Rust's cdylib suffix follows the host OS: `.dylib` on macOS, `.so` on
# Linux (the CI runner). The host-artifact paths below stay host-independent
# through this constant.
if [ "$(uname)" = "Darwin" ]; then
    readonly HOST_CDYLIB_SUFFIX="dylib"
else
    readonly HOST_CDYLIB_SUFFIX="so"
fi

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

generate_kotlin_bindings() {
    # Generate from the metadata embedded in a built artifact of this same
    # workspace (the host cdylib is enough; the metadata is target-
    # independent, and the build of the Android arm64 cdylib above already
    # proved the Rust side compiles). The build is unconditional: a host
    # artifact left behind by an earlier contract state must never feed the
    # generation.
    # The library file name on disk is the crate's declared `[lib] name`
    # (`rustic_maps`), which the contract-bound Kotlin loader also loads.
    local host_cdylib="${REPO_ROOT}/target/${METADATA_CARGO_TARGET_TRIPLE}/release/librustic_maps.${HOST_CDYLIB_SUFFIX}"
    # The release profile strips symbols, and uniffi reads its metadata from an
    # exported symbol of the library it is pointed at; a stripped library left
    # the generator with nothing to read, exiting successfully without writing
    # a file. This build exists only to feed the generator, so it stays
    # unstripped.
    CARGO_PROFILE_RELEASE_STRIP=false \
        cargo build --quiet --release -p "${BINDING_CRATE_NAME}" --target "${METADATA_CARGO_TARGET_TRIPLE}"
    if [ ! -f "${host_cdylib}" ]; then
        echo "error: host cdylib was not produced at ${host_cdylib}" >&2
        exit 1
    fi
    # The generation directory holds nothing but generated sources, and the
    # package path is derived from the contract namespace: wipe it so a
    # namespace from an earlier contract state cannot linger beside the
    # current one.
    rm -rf "${KOTLIN_GENERATED_SOURCES_DIR}/uniffi"
    # Run the built generator directly: on some hosts `cargo run` stalls at
    # process start with no CPU use, and the binary is produced by the same
    # workspace build that produced the host cdylib above. Its output is kept
    # beside the build so a silent run can still be explained.
    local generation_log="${REPO_ROOT}/target/uniffi-bindgen-kotlin.log"
    mkdir -p "$(dirname "${generation_log}")"
    run_pinned_uniffi_bindgen generate \
        --library "${host_cdylib}" \
        --language kotlin \
        --out-dir "${KOTLIN_GENERATED_SOURCES_DIR}" \
        --no-format 2>&1 | tee "${generation_log}"
    # The generator can exit successfully without writing a file (an unusable
    # library, for example); without this check the failure surfaces minutes
    # later as Gradle's "generated bindings are missing" with no cause named.
    local generated_kotlin_file_count
    generated_kotlin_file_count=$(find "${KOTLIN_GENERATED_SOURCES_DIR}" -name '*.kt' | wc -l | tr -d ' ')
    if [ "${generated_kotlin_file_count}" -eq 0 ]; then
        printf 'error: the generator produced no Kotlin sources under %s\n' \
            "${KOTLIN_GENERATED_SOURCES_DIR}" >&2
        printf 'generator: %s\nlibrary: %s (%s bytes, %s metadata symbol(s))\noutput:\n' \
            "${REPO_ROOT}/target/debug/uniffi-bindgen" \
            "${host_cdylib}" "$(wc -c < "${host_cdylib}" | tr -d ' ')" \
            "$(grep -a -o 'UNIFFI_META[A-Za-z0-9_]*' "${host_cdylib}" | wc -l | tr -d ' ')" >&2
        find "${KOTLIN_GENERATED_SOURCES_DIR}" -maxdepth 3 -print >&2 || true
        cat "${generation_log}" >&2 || true
        exit 1
    fi
    printf '==> generated %s Kotlin file(s)\n' "${generated_kotlin_file_count}"
}

stage_ffi_libraries_into_jni_libs() {
    rm -rf "${JNILIBS_DIR}"
    for abi_and_target in "${SUPPORTED_ANDROID_ABIS[@]}"; do
        local android_abi="$(echo "${abi_and_target}" | cut -d' ' -f1)"
        local rust_target="$(echo "${abi_and_target}" | cut -d' ' -f2)"
        local source_library="${REPO_ROOT}/target/${rust_target}/release/${BINDING_CRATE_FFI_LIBRARY_NAME}"
        if [ ! -f "${source_library}" ]; then
            echo "error: missing cdylib ${source_library}" >&2
            exit 1
        fi
        mkdir -p "${JNILIBS_DIR}/${android_abi}"
        cp "${source_library}" "${JNILIBS_DIR}/${android_abi}/"
    done
}

stage_license_notices_assets() {
    # Deterministic copies from their single sources; the AAR's assets tree is
    # generated under build/ on every run, so a stale notice cannot linger.
    rm -rf "${LICENSE_NOTICES_ASSETS_DIR}"
    mkdir -p "${LICENSE_NOTICES_ASSETS_DIR}"
    cp "${MIT_LICENSE_PATH}" "${APACHE_LICENSE_PATH}" "${NOTICES_PATH}" "${LICENSE_NOTICES_ASSETS_DIR}/"
}

stage_host_library_for_smoke() {
    # The `:smoke-jvm` module loads the Apple-host cdylib through JNA to run
    # the generated Kotlin bindings on the JVM; the Android `.so` libraries
    # run only on Android.
    local smoke_jni_dir="${REPO_ROOT}/platform/android/smoke-jvm/jni"
    mkdir -p "${smoke_jni_dir}"
    cp "${REPO_ROOT}/target/${METADATA_CARGO_TARGET_TRIPLE}/release/librustic_maps.${HOST_CDYLIB_SUFFIX}" "${smoke_jni_dir}/"
}

assert_sixteen_kilobyte_page_and_jni_symbols() {
    # Two release-integrity properties of every staged ABI library:
    # 1. each ELF LOAD segment is aligned to at least 16 KB (Android's 16 KB
    #    page-size devices), and
    # 2. both hand-written JNI entry points are exported, so the Kotlin bridge
    #    cannot link to an object that lost them.
    local llvm_bin_dir
    llvm_bin_dir="$(ls -d "${ANDROID_HOME}/ndk/${ANDROID_NDK_VERSION}/toolchains/llvm/prebuilt/"*-x86_64"/bin" | head -n 1)"
    local readelf_binary="${llvm_bin_dir}/llvm-readelf"
    local nm_binary="${llvm_bin_dir}/llvm-nm"
    if [ ! -x "${readelf_binary}" ] || [ ! -x "${nm_binary}" ]; then
        echo "error: NDK LLVM inspection tools not found under ${llvm_bin_dir}" >&2
        exit 1
    fi
    local library
    for library in "${JNILIBS_DIR}"/*/*.so; do
        if [ ! -f "${library}" ]; then
            continue
        fi
        local android_abi
        android_abi="$(basename "$(dirname "${library}")")"
        local requires_sixteen_kilobyte_alignment=false
        local required_abi
        for required_abi in "${SIXTEEN_KILOBYTE_REQUIRED_ABIS[@]}"; do
            if [ "${android_abi}" = "${required_abi}" ]; then
                requires_sixteen_kilobyte_alignment=true
            fi
        done
        if [ "${requires_sixteen_kilobyte_alignment}" = true ]; then
            local load_alignments
            load_alignments="$("${readelf_binary}" -lW "${library}" | awk '$1 == "LOAD" {print $NF}')"
            if [ -z "${load_alignments}" ]; then
                echo "error: ${library} has no ELF LOAD segments to inspect" >&2
                exit 1
            fi
            local alignment
            for alignment in ${load_alignments}; do
                if (( alignment < SIXTEEN_KILOBYTE_PAGE_ALIGNMENT )); then
                    echo "error: ${library} has a LOAD segment aligned at ${alignment}; " \
                        "every segment must be at least ${SIXTEEN_KILOBYTE_PAGE_ALIGNMENT} for 16 KB page-size devices" >&2
                    exit 1
                fi
            done
        fi
        local exported_symbols
        exported_symbols="$("${nm_binary}" -D --defined-only "${library}")"
        local required_symbol
        for required_symbol in "${REQUIRED_JNI_SYMBOLS[@]}"; do
            if ! grep -q -- " ${required_symbol}$" <<<"${exported_symbols}"; then
                echo "error: ${library} does not export ${required_symbol}" >&2
                exit 1
            fi
        done
    done
}

assert_release_aar_carries_the_expected_artifacts() {
    # The one published AAR must carry the generated Kotlin bindings, the
    # hand-written wrapper classes and all three ABI libraries. The exact
    # release name is asserted first so a debug AAR can never stand in for the
    # published artifact.
    local aar_path="${RELEASE_AAR_PATH}"
    if [ ! -f "${aar_path}" ]; then
        echo "error: the release AAR was not produced at ${aar_path}" >&2
        exit 1
    fi
    local inspection_directory
    inspection_directory="$(mktemp -d)"
    unzip -p "${aar_path}" classes.jar > "${inspection_directory}/classes.jar"
    local classes_jar_listing
    classes_jar_listing="$(unzip -l "${inspection_directory}/classes.jar")"
    # One class from the generated contract bindings and the wrapper classes an
    # integrator calls. The generated `MapView` and the wrapper `MapView` live
    # in different packages, so both paths are asserted.
    local required_classes=(
        "uniffi/rustic_maps/MapSdk.class"
        "uniffi/rustic_maps/MapView.class"
        "com/rusticmaps/sdk/MapView.class"
        "com/rusticmaps/sdk/MapViewListener.class"
        "com/rusticmaps/sdk/MapViewFailure.class"
        "com/rusticmaps/sdk/MapViewFailurePhase.class"
    )
    local required_class
    for required_class in "${required_classes[@]}"; do
        if ! grep -q -F -- "${required_class}" <<<"${classes_jar_listing}"; then
            echo "error: the release AAR classes do not contain ${required_class}" >&2
            rm -rf "${inspection_directory}"
            exit 1
        fi
    done
    local aar_file_listing
    aar_file_listing="$(unzip -l "${aar_path}")"
    local abi_and_target
    for abi_and_target in "${SUPPORTED_ANDROID_ABIS[@]}"; do
        local android_abi
        android_abi="$(echo "${abi_and_target}" | cut -d' ' -f1)"
        if ! grep -q -F -- "jni/${android_abi}/librustic_maps.so" <<<"${aar_file_listing}"; then
            echo "error: the release AAR does not contain the ${android_abi} library" >&2
            rm -rf "${inspection_directory}"
            exit 1
        fi
    done
    rm -rf "${inspection_directory}"
}

assert_refapp_only_content_absent_from_aar() {
    # The AAR is the published package: the refapp-only binding crate, the
    # demo scenes loader/model, the UX theme and the demo artifact live in the
    # reference application module instead. The jniLibs entries and assets are
    # visible in the AAR file list, while the Kotlin classes sit inside the
    # nested `classes.jar`; inspect both. The markers cover the refapp-only
    # binding crate and the demo content (audit C10).
    local aar_path="${RELEASE_AAR_PATH}"
    if [ ! -f "${aar_path}" ]; then
        echo "error: the release AAR was not produced at ${aar_path}" >&2
        exit 1
    fi
    local inspection_directory
    inspection_directory="$(mktemp -d)"
    local marker
    for marker in "${REFAPP_ONLY_PACKAGE_MARKER}" "${REFAPP_ONLY_DEMO_MARKERS[@]}"; do
        if unzip -l "${aar_path}" | grep -qi -- "${marker}"; then
            echo "error: the published AAR carries refapp-only content (${marker})" >&2
            rm -rf "${inspection_directory}"
            exit 1
        fi
    done
    unzip -p "${aar_path}" classes.jar > "${inspection_directory}/classes.jar"
    for marker in "${REFAPP_ONLY_PACKAGE_MARKER}" "${REFAPP_ONLY_DEMO_MARKERS[@]}"; do
        if unzip -l "${inspection_directory}/classes.jar" | grep -qi -- "${marker}"; then
            echo "error: the published AAR classes carry refapp-only content (${marker})" >&2
            rm -rf "${inspection_directory}"
            exit 1
        fi
    done
    rm -rf "${inspection_directory}"
}

main() {
    echo "==> checking toolchain"
    require_tool cargo
    require_tool gradle
    : "${ANDROID_HOME:?error: ANDROID_HOME must point at the Android SDK}"
    if [ ! -d "${ANDROID_HOME}/ndk/${ANDROID_NDK_VERSION}" ]; then
        yes | "${ANDROID_HOME}/cmdline-tools/latest/bin/sdkmanager" --install "ndk;${ANDROID_NDK_VERSION}" > /dev/null
    fi
    if [ "$(uname)" = "Darwin" ] && /usr/libexec/java_home -v 21 > /dev/null 2>&1; then
        export JAVA_HOME="$(/usr/libexec/java_home -v 21)"
    fi

    echo "==> installing Rust Android targets (no-op when present)"
    for abi_and_target in "${SUPPORTED_ANDROID_ABIS[@]}"; do
        install_android_target_if_missing "$(echo "${abi_and_target}" | cut -d' ' -f2)"
    done

    echo "==> building release cdylibs per ABI"
    for abi_and_target in "${SUPPORTED_ANDROID_ABIS[@]}"; do
        build_android_cdylib "$(echo "${abi_and_target}" | cut -d' ' -f2)" "$(echo "${abi_and_target}" | cut -d' ' -f3)"
    done

    echo "==> generating Kotlin bindings from the frozen UDL"
    generate_kotlin_bindings

    echo "==> staging jniLibs into the Gradle module"
    stage_ffi_libraries_into_jni_libs
    stage_host_library_for_smoke

    echo "==> asserting 16 KB page alignment and JNI exports"
    assert_sixteen_kilobyte_page_and_jni_symbols

    echo "==> staging the licenses and third-party notices for the AAR"
    stage_license_notices_assets

    echo "==> regenerating the Gradle wrapper at 9.5.1"
    (cd "${GRADLE_ROOT}" && gradle --no-daemon -q wrapper --gradle-version 9.5.1)

    echo "==> assembling the one AAR"
    # `:maps-sdk` compiles the generated bindings, the ABI libraries and the
    # wrapper into a single artifact; `:maps-sdk-native` is a source holder.
    (cd "${GRADLE_ROOT}" && ./gradlew --no-daemon :maps-sdk:assembleRelease)

    echo "==> asserting the AAR carries the bindings, wrapper classes and ABIs"
    assert_release_aar_carries_the_expected_artifacts

    echo "==> running the JVM smoke against the Apple-host cdylib"
    (cd "${GRADLE_ROOT}" && ./gradlew --no-daemon :smoke-jvm:smokeRun)

    # The published wrapper's unit suite resolves the contract-owned provider
    # registry-name mapping through the generated bindings, whose JNA calls
    # load the host cdylib staged above for the smoke runner.
    echo "==> running the Android wrapper unit suite"
    (cd "${GRADLE_ROOT}" && ./gradlew --no-daemon :maps-sdk:testDebugUnitTest)

    echo "==> asserting refapp-only content is absent from the AAR"
    require_tool unzip
    assert_refapp_only_content_absent_from_aar

    echo "done: ${RELEASE_AAR_PATH}"
}

main "$@"