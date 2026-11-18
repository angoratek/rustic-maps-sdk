#!/usr/bin/env bash
# Package the generated React Native TurboModule as `@angoratek/rustic-maps-sdk`
# and emit the release tarball `rustic-maps-sdk-rn-v<version>.tgz`
# (docs/Release.md (The artifacts)).
#
# The generated sources are gitignored ubrn output, so this script regenerates
# them from the frozen UDL before packing: the JSI bindings
# (src/generated/, cpp/generated/) and the TurboModule glue (src/index.ts,
# src/NativeMapsSdkRn.ts, cpp/, android/, ios/, MapsSdkRn.podspec). Node
# dependencies are installed from the committed lockfile when missing.
#
# Two package shapes exist:
#
#   - default: the *source* package the generated podspec/CMake consume, plus
#     the dual license files and the third-party notices. Useful for a
#     packaging dry run: without the payload, `npm install` alone cannot build
#     the module.
#   - `--with-native-payload`: additionally embed the compiled native payload —
#     `android/src/main/jniLibs/<abi>/librustic_maps.a` for the AAR's three
#     ABIs and the `AngoratekRusticMapsSdkFramework.xcframework`. This is the
#     shape every release ships: `scripts/assemble-release-artifacts.sh` and
#     the release workflow stage the payload from the Android and iOS release
#     builds, so packaging needs neither the Android NDK nor Xcode.
#
# Native-payload architecture decision (docs/Release.md (The artifacts)): the
# payload mirrors the AAR's three Android ABIs (`arm64-v8a`, `armeabi-v7a`,
# `x86_64`) and carries the arm64 device/simulator iOS framework ubrn builds.
# That is the owner ruling of 2026-10-29 ("the RN package ships the AAR's three
# ABIs"); the RN app shell itself still pins arm64-v8a.
#
# The package is staged into a fresh temporary directory before `npm pack`, so
# a payload left in `platform/rn` by an earlier staging can only enter a
# `--with-native-payload` tarball, and the exact-member assertion stays
# meaningful for both shapes.
#
# Usage:
#   scripts/package-rn-npm.sh [--output-dir <directory>]
#                             [--with-native-payload] [--dry-run]
#
#   --dry-run prints the `npm pack` member list and writes no tarball.
#
# Idempotent: generation rewrites its outputs; the staging tree is fresh on
# every run and the tarball is overwritten.
set -euo pipefail

readonly REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly RN_ROOT="${REPO_ROOT}/platform/rn"
readonly NPM_PACKAGE_NAME="angoratek-rustic-maps-sdk"
readonly RELEASE_ASSET_PREFIX="rustic-maps-sdk-rn-v"
readonly VERSION_GUARD="${REPO_ROOT}/scripts/check-workspace-version.sh"
readonly MIT_LICENSE_PATH="${REPO_ROOT}/LICENSE-MIT"
readonly APACHE_LICENSE_PATH="${REPO_ROOT}/LICENSE-APACHE"
readonly NOTICES_PATH="${REPO_ROOT}/docs/THIRD_PARTY_NOTICES.md"
readonly NOTICES_PACKAGE_NAME="THIRD_PARTY_NOTICES.md"
# The generated TurboModule surface packed in every shape. `platform/rn`'s
# `files` allow-list names the same set; the committed manifest is
# authoritative for a manual `npm pack`, this list is what the release script
# stages.
readonly PACKAGE_SOURCE_ENTRIES=(
    "src"
    "cpp"
    "android"
    "ios"
    "MapsSdkRn.podspec"
    "react-native.config.js"
    "tsconfig.json"
    "package.json"
)
readonly NATIVE_PAYLOAD_ANDROID_ARCHIVES=(
    "android/src/main/jniLibs/arm64-v8a/librustic_maps.a"
    "android/src/main/jniLibs/armeabi-v7a/librustic_maps.a"
    "android/src/main/jniLibs/x86_64/librustic_maps.a"
)
readonly NATIVE_PAYLOAD_IOS_FRAMEWORK="AngoratekRusticMapsSdkFramework.xcframework"
# Transient build outputs a previous native build may leave inside the copied
# Gradle/CMake trees; they are never part of the package.
readonly STAGING_TRANSIENT_PATHS=(
    "android/build"
    "android/.cxx"
    "android/.gradle"
)
# The exact npm tarball member set for the pinned ubrn generation plus the
# license and notices files staged below, sorted. A ubrn upgrade that changes
# the generated file set updates this list in the same change (a deliberate
# packaging change, like the contract gates). The native payload files are
# appended by discovery in `--with-native-payload` mode.
readonly EXPECTED_TARBALL_MEMBERS=(
    "package/LICENSE-APACHE"
    "package/LICENSE-MIT"
    "package/MapsSdkRn.podspec"
    "package/THIRD_PARTY_NOTICES.md"
    "package/android/CMakeLists.txt"
    "package/android/build.gradle"
    "package/android/cpp-adapter.cpp"
    "package/android/proguard-rules.pro"
    "package/android/src/main/AndroidManifest.xml"
    "package/android/src/main/AndroidManifestNew.xml"
    "package/android/src/main/java/com/mapssdkrn/MapsSdkMapHost.kt"
    "package/android/src/main/java/com/mapssdkrn/MapsSdkMapHostView.kt"
    "package/android/src/main/java/com/mapssdkrn/MapsSdkMapViewManager.kt"
    "package/android/src/main/java/com/mapssdkrn/MapsSdkNativeEventChannel.kt"
    "package/android/src/main/java/com/mapssdkrn/MapsSdkNativeJson.kt"
    "package/android/src/main/java/com/mapssdkrn/MapsSdkNativeModule.kt"
    "package/android/src/main/java/com/mapssdkrn/MapsSdkNativeRuntime.kt"
    "package/android/src/main/java/com/mapssdkrn/MapsSdkRnModule.kt"
    "package/android/src/main/java/com/mapssdkrn/MapsSdkRnPackage.kt"
    "package/cpp/generated/rustic_maps.cpp"
    "package/cpp/generated/rustic_maps.hpp"
    "package/cpp/maps-sdk-rn.cpp"
    "package/cpp/maps-sdk-rn.h"
    "package/ios/MapsSdkMapHostView.swift"
    "package/ios/MapsSdkMapViewManager.mm"
    "package/ios/MapsSdkNativeEventChannel.swift"
    "package/ios/MapsSdkNativeJson.swift"
    "package/ios/MapsSdkNativeModule.h"
    "package/ios/MapsSdkNativeModule.mm"
    "package/ios/MapsSdkNativeRuntime.swift"
    "package/ios/MapsSdkRn.h"
    "package/ios/MapsSdkRn.mm"
    "package/ios/MapsSdkRusticMapView.swift"
    "package/ios/generated/rustic_maps.swift"
    "package/package.json"
    "package/react-native.config.js"
    "package/src/MapView.tsx"
    "package/src/MapsSdk.ts"
    "package/src/MapsSdkError.ts"
    "package/src/NativeMapsSdkRn.ts"
    "package/src/generated/rustic_maps-ffi.ts"
    "package/src/generated/rustic_maps.ts"
    "package/src/index.ts"
    "package/tsconfig.json"
)
# Fail-closed name guards; the exact-list assertion already rejects these,
# but naming them yields an actionable message when one appears. The payload
# guards apply to the source shape only: `jniLibs` and `.xcframework` are the
# `--with-native-payload` payload, and never anything else.
readonly BASE_DENIED_MEMBER_PARTS=(
    "app-shell"
    "fixtures"
    "tests"
    ".keystore"
    "node_modules"
    "/build/"
)
readonly NATIVE_PAYLOAD_DENIED_MEMBER_PARTS=(
    "jniLibs"
    ".xcframework"
)

output_dir="${REPO_ROOT}/dist"
with_native_payload=false
dry_run=false
staging_dir=""

usage() {
    cat >&2 <<'USAGE'
usage: package-rn-npm.sh [--output-dir <directory>]
                         [--with-native-payload] [--dry-run]

  --output-dir <directory>  where the release tarball is written
                            (default: <repo>/dist)
  --with-native-payload     also pack the three jniLibs archives and the
                            arm64 iOS XCFramework, staged from the Android and
                            iOS release builds
  --dry-run                 print the npm member list and write no tarball
USAGE
}

fail() {
    printf 'error: %s\n' "$1" >&2
    exit 1
}

require_tool() {
    command -v "$1" >/dev/null 2>&1 || fail "required tool '$1' was not found on PATH"
}

cleanup() {
    if [ -n "${staging_dir}" ] && [ -d "${staging_dir}" ]; then
        rm -rf "${staging_dir}"
    fi
}
trap cleanup EXIT

parse_arguments() {
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --output-dir)
                [ "$#" -ge 2 ] || { usage; exit 2; }
                output_dir="$2"
                shift 2
                ;;
            --with-native-payload)
                with_native_payload=true
                shift
                ;;
            --dry-run)
                dry_run=true
                shift
                ;;
            -h|--help)
                usage
                exit 0
                ;;
            *)
                printf 'error: unknown argument: %s\n' "$1" >&2
                usage
                exit 2
                ;;
        esac
    done
}

install_dependencies_if_missing() {
    if [ ! -x "${RN_ROOT}/node_modules/.bin/ubrn" ]; then
        echo "==> installing the React Native package dependencies (npm ci)"
        (cd "${RN_ROOT}" && npm ci --no-audit --no-fund)
    fi
}

regenerate_bindings() {
    # The ubrn CLI is pinned by package-lock.json; generation is a pure
    # function of the frozen UDL and ubrn.config.yaml. The shared generator
    # owns the post-generation build fixes (`libandroid.so`, the ReactCodegen
    # spec include) and the curated root exports, so a packaged tree is the
    # same tree the app build consumes — never a bare ubrn output.
    echo "==> generating the JSI bindings and the TurboModule glue"
    "${REPO_ROOT}/scripts/generate-rn-bindings.sh"
}

read_manifest_version() {
    node -e 'process.stdout.write(require(process.argv[1]).version)' "$1"
}

assert_native_payload_present() {
    local android_archive
    for android_archive in "${NATIVE_PAYLOAD_ANDROID_ARCHIVES[@]}"; do
        [ -f "${RN_ROOT}/${android_archive}" ] \
            || fail "missing the Android native payload ${android_archive#"${REPO_ROOT}/"}; run scripts/build-rn.sh first"
    done
    [ -f "${RN_ROOT}/${NATIVE_PAYLOAD_IOS_FRAMEWORK}/Info.plist" ] \
        || fail "missing the iOS native payload ${NATIVE_PAYLOAD_IOS_FRAMEWORK}; run scripts/build-rn.sh first"
    if [ -z "$(find "${RN_ROOT}/${NATIVE_PAYLOAD_IOS_FRAMEWORK}" -mindepth 1 -maxdepth 1 \
        ! -name "Info.plist" -print -quit)" ]; then
        fail "the iOS native payload ${NATIVE_PAYLOAD_IOS_FRAMEWORK} contains no slices"
    fi
}

stage_package() {
    staging_dir="$(mktemp -d)"
    local entry
    for entry in "${PACKAGE_SOURCE_ENTRIES[@]}"; do
        [ -e "${RN_ROOT}/${entry}" ] \
            || fail "missing generated package entry ${entry}; run scripts/build-rn.sh first"
        cp -R "${RN_ROOT}/${entry}" "${staging_dir}/${entry}"
    done
    for entry in "${STAGING_TRANSIENT_PATHS[@]}"; do
        rm -rf "${staging_dir:?}/${entry}"
    done
    find "${staging_dir}" -name ".DS_Store" -delete
    # The pod references the generated Swift bindings through a repository
    # symlink (`platform/rn/ios/generated/rustic_maps.swift` -> the generated
    # shared binding output). A packed tarball must carry the file itself, never
    # a link into the repository, so materialize it — and fail loudly when the
    # generated bindings are absent instead of shipping a broken link.
    local generated_swift_relative="ios/generated/rustic_maps.swift"
    local generated_swift_source="${RN_ROOT}/${generated_swift_relative}"
    local generated_swift_staged="${staging_dir}/${generated_swift_relative}"
    if [ -L "${generated_swift_source}" ]; then
        # In the staged tree the relative link points outside the temp tree,
        # so test (and read) the source tree's link, which resolves locally.
        if [ ! -f "${generated_swift_source}" ]; then
            fail "the generated Swift bindings are missing (${generated_swift_relative} -> $(readlink "${generated_swift_source}")); run scripts/build-rn.sh first"
        fi
        rm "${generated_swift_staged}"
        # `cp` follows the source link, so the staged tree holds the file.
        cp "${generated_swift_source}" "${generated_swift_staged}"
    fi
    # The dual license text and the third-party notices are package-level
    # inputs: copied into every shipped shape, from their single sources.
    cp "${MIT_LICENSE_PATH}" "${APACHE_LICENSE_PATH}" "${staging_dir}/"
    cp "${NOTICES_PATH}" "${staging_dir}/${NOTICES_PACKAGE_NAME}"
    if [ "${with_native_payload}" = true ]; then
        assert_native_payload_present
        cp -R "${RN_ROOT}/${NATIVE_PAYLOAD_IOS_FRAMEWORK}" "${staging_dir}/"
        # The Android payload rides inside the copied `android/` tree; the
        # required archive is asserted above, so a stale/empty jniLibs cannot
        # silently satisfy the flag.
    else
        # The copied `android/` tree may carry a payload from an earlier
        # staging or build-rn.sh run; the source shape carries none.
        rm -rf "${staging_dir}/android/src/main/jniLibs"
    fi
}

build_expected_member_file() {
    local expected_file="$1"
    printf '%s\n' "${EXPECTED_TARBALL_MEMBERS[@]}" > "${expected_file}"
    if [ "${with_native_payload}" = true ]; then
        (
            cd "${staging_dir}"
            # `npm pack` prefixes every member with the package root.
            find "${NATIVE_PAYLOAD_IOS_FRAMEWORK}" -type f -print \
                | sed 's|^|package/|'
            find "android/src/main/jniLibs" -type f -print \
                | sed 's|^|package/|'
        ) >> "${expected_file}"
    fi
    sort -o "${expected_file}" "${expected_file}"
}

assert_tarball_members() {
    local tarball="$1"
    local expected_file actual_file denied_part
    expected_file="$(mktemp)"
    actual_file="$(mktemp)"
    # shellcheck disable=SC2064
    trap "rm -f '${expected_file}' '${actual_file}'; cleanup" EXIT
    build_expected_member_file "${expected_file}"
    tar -tzf "${tarball}" | grep -v '/$' | sort > "${actual_file}"
    if ! diff -u "${expected_file}" "${actual_file}"; then
        fail "the npm tarball does not hold exactly the expected generated files and payload"
    fi
    local -a denied_member_parts=("${BASE_DENIED_MEMBER_PARTS[@]}")
    if [ "${with_native_payload}" = false ]; then
        denied_member_parts+=("${NATIVE_PAYLOAD_DENIED_MEMBER_PARTS[@]}")
    fi
    for denied_part in "${denied_member_parts[@]}"; do
        if grep -qF "${denied_part}" "${actual_file}"; then
            fail "the npm tarball contains a denied path part: ${denied_part}"
        fi
    done
}

print_dry_run_members() {
    echo "==> npm pack --dry-run (staged package members)"
    (
        cd "${staging_dir}"
        npm pack --dry-run --json
    ) | node -e '
        const chunks = [];
        process.stdin.on("data", (chunk) => chunks.push(chunk)).on("end", () => {
            const report = JSON.parse(chunks.join(""));
            for (const file of report[0].files.sort((left, right) => left.path.localeCompare(right.path))) {
                console.log(file.path);
            }
        });
    '
}

main() {
    parse_arguments "$@"
    require_tool node
    require_tool npm
    require_tool tar
    require_tool cargo

    install_dependencies_if_missing
    regenerate_bindings

    local workspace_version manifest_version
    workspace_version="$("${VERSION_GUARD}" --print-version)"
    manifest_version="$(read_manifest_version "${RN_ROOT}/package.json")"
    [ "${manifest_version}" = "${workspace_version}" ] \
        || fail "platform/rn/package.json declares ${manifest_version}, but the workspace version is ${workspace_version}"

    stage_package

    if [ "${dry_run}" = true ]; then
        print_dry_run_members
        exit 0
    fi

    mkdir -p "${output_dir}"
    output_dir="$(cd "${output_dir}" && pwd)"
    local packed_tarball release_tarball
    packed_tarball="${output_dir}/${NPM_PACKAGE_NAME}-${workspace_version}.tgz"
    release_tarball="${output_dir}/${RELEASE_ASSET_PREFIX}${workspace_version}.tgz"
    rm -f "${packed_tarball}" "${release_tarball}"
    (cd "${staging_dir}" && npm pack --pack-destination "${output_dir}" >/dev/null)
    [ -f "${packed_tarball}" ] || fail "npm pack did not produce ${packed_tarball}"
    mv "${packed_tarball}" "${release_tarball}"

    assert_tarball_members "${release_tarball}"

    printf 'ok: %s (%s bytes, %s members)\n' \
        "${release_tarball}" "$(wc -c < "${release_tarball}" | tr -d ' ')" \
        "$(tar -tzf "${release_tarball}" | grep -vc '/$')"
}

main "$@"
