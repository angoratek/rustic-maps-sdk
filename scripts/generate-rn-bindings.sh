#!/usr/bin/env bash
# Generates the React Native binding package's sources from the frozen UDL
# with `ubrn`: the JSI TypeScript + C++ bindings, then the TurboModule glue
# (`src/NativeMapsSdkRn.ts`, `src/index.ts`, `cpp/<package>.{h,cpp}`).
#
# The generated trees are gitignored build artifacts, so every consumer that
# needs them (the app build, the jest/TypeScript gate) generates them here
# rather than assuming a local run.
#
# Usage:
#   scripts/generate-rn-bindings.sh                  # both steps
#   scripts/generate-rn-bindings.sh --bindings       # JSI bindings only
#   scripts/generate-rn-bindings.sh --turbo-module   # TurboModule glue only
#   scripts/generate-rn-bindings.sh --check          # fail on drift
#
# `--check` requires a generation in the working tree, regenerates the
# selected steps with the same post-generation patches into a temporary
# directory outside the repository, and compares every generated file
# byte-for-byte. It names each differing file and exits non-zero on any
# difference. Combine it with `--bindings` or `--turbo-module` to check one
# step only.
#
# Prerequisites: `npm ci` in `platform/rn` (the `ubrn` binary), a Rust
# toolchain (ubrn resolves the crate through `cargo metadata`).
set -euo pipefail

readonly REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly RN_PROJECT_ROOT="${REPO_ROOT}/platform/rn"
readonly BINDING_CRATE_DIR="${REPO_ROOT}/crates/rustic-maps-native-core"
readonly BINDING_CRATE_NAME="rustic-maps-native-core"
readonly FROZEN_UDL_PATH="${BINDING_CRATE_DIR}/src/rustic_maps.udl"
readonly UNIFFI_NAMESPACE_NAME="rustic_maps"
readonly GENERATED_TS_DIR="src/generated"
readonly GENERATED_CPP_DIR="cpp/generated"
readonly UBRN_COMMAND="${RN_PROJECT_ROOT}/node_modules/.bin/ubrn"

readonly REAPPPLIED_CMAKE_LIBANDROID_MARKER="find_library(ANDROID_LIB android)"

ensure_libandroid_link_line() {
    # The generated `android/CMakeLists.txt` links the Rust static archive
    # but never `libandroid.so`, whose `ANativeWindow_*` symbols the
    # statically linked Rust library's wgpu GLES backend references: a clean
    # TurboModule build fails to link without it, and a warm `.cxx` build
    # cache can mask the gap for exactly one regeneration. The fix is
    # applied idempotently after every generation run so the tree is
    # reproducible without a hand edit.
    local project_root="${1:-$RN_PROJECT_ROOT}"
    local generated_cmake="${project_root}/android/CMakeLists.txt"
    if [ ! -f "${generated_cmake}" ]; then
        printf 'generate-rn-bindings: the generated TurboModule CMakeLists.txt is missing: %s\n' \
            "${generated_cmake}" >&2
        exit 1
    fi
    if ! grep -qF "${REAPPPLIED_CMAKE_LIBANDROID_MARKER}" "${generated_cmake}"; then
        awk '
            /find_library\(LOGCAT log\)/ {
                print
                print "find_library(ANDROID_LIB android)"
                next
            }
            /\$\{LOGCAT\}/ {
                print
                print "  ${ANDROID_LIB}"
                next
            }
            { print }
        ' "${generated_cmake}" > "${generated_cmake}.patched"
        mv "${generated_cmake}.patched" "${generated_cmake}"
        printf '==> applied the libandroid.so link line in the generated CMakeLists.txt\n'
    fi
}

# The ubrn-generated module header names the RN codegen spec umbrella by the
# same basename the module's own header carries (the TurboModule config's
# `name: MapsSdkRn` equals the codegenConfig name): the quoted
# `#import "MapsSdkRn.h"` resolves to the header itself (quoted-include
# resolution prefers the including file's directory), so the
# `@protocol NativeMapsSdkRnSpec` is never seen and the pod's `MapsSdkRn.mm`
# fails with `no type or protocol named 'NativeMapsSdkRnSpec'`. The
# ReactCodegen pod exports the spec umbrella on the public headers path, so
# the include re-points there. Idempotent, re-applied after every generation
# run (the generation overwrites the header); the Android side needs no such
# guard because its codegen spec is discovered by Gradle, not a quoted
# include.
readonly CODEGEN_SPEC_INCLUDE_SELF_NAME='#import "MapsSdkRn.h"'
readonly CODEGEN_SPEC_INCLUDE_REPOINTED='#import <ReactCodegen/MapsSdkRn/MapsSdkRn.h>'

ensure_codegen_spec_include_on_ios() {
    local project_root="${1:-$RN_PROJECT_ROOT}"
    local module_header="${project_root}/ios/MapsSdkRn.h"
    if [ ! -f "${module_header}" ]; then
        printf 'generate-rn-bindings: the generated TurboModule iOS module header is missing: %s\n' \
            "${module_header}" >&2
        exit 1
    fi
    if grep -qF "${CODEGEN_SPEC_INCLUDE_SELF_NAME}" "${module_header}"; then
        # A temporary file and a rename edit in place on both BSD and GNU sed:
        # `sed -i ''` is BSD syntax that GNU sed reads as a script argument.
        sed "s|${CODEGEN_SPEC_INCLUDE_SELF_NAME}|${CODEGEN_SPEC_INCLUDE_REPOINTED}|" \
            "${module_header}" > "${module_header}.repointed"
        mv "${module_header}.repointed" "${module_header}"
        printf '==> re-pointed the codegen spec include past the own-name header collision\n'
    fi
}

readonly CMAKE_GENERATED_BINDINGS_GUARD_MARKER="# rustic-maps: generated bindings guard"

ensure_generated_bindings_guard_in_cmake() {
    # The TurboModule library compiles the generator's C++ output. A
    # consumer that installs the package without the generator dependency
    # (or without running it) would otherwise fail deep in the C++ compile
    # with a missing-header error that names no cause. The guard fails at
    # configure time with the file and the remedy. `uniffi-bindgen-react-native`
    # is a runtime dependency of the package, so a normal `npm install`
    # provides it; the generated sources still have to be produced by a
    # generation step. Idempotent, re-applied after every generation run.
    local project_root="${1:-$RN_PROJECT_ROOT}"
    local generated_cmake="${project_root}/android/CMakeLists.txt"
    if [ ! -f "${generated_cmake}" ]; then
        printf 'generate-rn-bindings: the generated TurboModule CMakeLists.txt is missing: %s\n' \
            "${generated_cmake}" >&2
        exit 1
    fi
    if grep -qF "${CMAKE_GENERATED_BINDINGS_GUARD_MARKER}" "${generated_cmake}"; then
        return
    fi
    local guard_file="${generated_cmake}.guard"
    local patched_file="${generated_cmake}.patched"
    cat > "${guard_file}" <<'CMAKE_GENERATED_BINDINGS_GUARD'
# rustic-maps: generated bindings guard
# The C++ below is uniffi-bindgen-react-native output, not package source.
# Fail at configure time with an actionable message instead of a missing
# header deep inside the compile.
execute_process(
    COMMAND node -p "require.resolve('uniffi-bindgen-react-native/package.json')"
    RESULT_VARIABLE UNIFFI_GENERATOR_RESOLVE_RESULT
    OUTPUT_QUIET
    ERROR_QUIET
)
if(NOT UNIFFI_GENERATOR_RESOLVE_RESULT EQUAL 0)
    message(FATAL_ERROR
        "uniffi-bindgen-react-native is not installed. It is a runtime dependency "
        "of @angoratek/rustic-maps-sdk; reinstall the package with `npm install` "
        "so the generator is available, then regenerate the bindings.")
endif()
if(NOT EXISTS "${CMAKE_CURRENT_SOURCE_DIR}/../cpp/generated/rustic_maps.cpp" OR
   NOT EXISTS "${CMAKE_CURRENT_SOURCE_DIR}/../cpp/generated/rustic_maps.hpp")
    message(FATAL_ERROR
        "the generated UniFFI C++ bindings are missing under cpp/generated. Run "
        "`node_modules/.bin/ubrn generate jsi bindings` (or the package's "
        "generate script) before building the TurboModule.")
endif()
CMAKE_GENERATED_BINDINGS_GUARD
    # Insert after `project(...)`, so the generated file's own
    # `cmake_minimum_required` stays the first command.
    awk -v guard_file="${guard_file}" '
        { print }
        /^project\(/ && !inserted {
            while ((getline guard_line < guard_file) > 0) { print guard_line }
            close(guard_file)
            inserted = 1
        }
    ' "${generated_cmake}" > "${patched_file}"
    mv "${patched_file}" "${generated_cmake}"
    rm -f "${guard_file}"
    if ! grep -qF "${CMAKE_GENERATED_BINDINGS_GUARD_MARKER}" "${generated_cmake}"; then
        printf 'generate-rn-bindings: could not insert the generated-bindings guard (no project() line)\n' >&2
        exit 1
    fi
    printf '==> added the generated-bindings guard to the CMakeLists.txt\n'
}

readonly CURATED_ROOT_EXPORTS_MARKER="// rustic-maps: curated root exports"

ensure_curated_root_exports() {
    # `src/index.ts` is ubrn output, so the package's curated surface is
    # appended after every generation instead of hand-edited: a bare
    # generation would otherwise ship a root entry that exports only the
    # generated contract, and a consumer could not import the package's own
    # `MapView` and `MapsSdk` from it. The generated namespace stays
    # exported beside them. Idempotent via the marker.
    local project_root="${1:-$RN_PROJECT_ROOT}"
    local generated_index="${project_root}/src/index.ts"
    if [ ! -f "${generated_index}" ]; then
        printf 'generate-rn-bindings: the generated TurboModule TypeScript entry is missing: %s\n' \
            "${generated_index}" >&2
        exit 1
    fi
    if grep -qF "${CURATED_ROOT_EXPORTS_MARKER}" "${generated_index}"; then
        # Replace an existing curated block with the current one: the export
        # list changes with the package's public surface, and a stale block in
        # a developer's generated tree must not keep the old names.
        local marker_line_number
        marker_line_number="$(grep -nF "${CURATED_ROOT_EXPORTS_MARKER}" "${generated_index}" | head -n 1 | cut -d: -f1)"
        head -n "$((marker_line_number - 1))" "${generated_index}" > "${generated_index}.tmp"
        mv "${generated_index}.tmp" "${generated_index}"
    fi
    cat >> "${generated_index}" <<'CURATED_ROOT_EXPORTS'

// rustic-maps: curated root exports
// The generated module exports the whole contract surface; the package adds
// its own components and the generated namespace, so a consumer imports
// `MapView`, `MapsSdk` and the bindings from the package root. Explicit
// exports take precedence over the generated star export, so `MapView` here
// is the component and the generated one stays reachable through
// `rustic_maps`.
export { MapView } from './MapView';
export { MapsSdk, mapsSdk } from './MapsSdk';
export {
  InvalidTileCacheTimeToLiveError,
  MapsSdkError,
  MapsSdkErrorCode,
  TILE_CACHE_TIME_TO_LIVE_ERROR_CODE,
  TILE_CACHE_TIME_TO_LIVE_INVALID_MESSAGE,
} from './MapsSdk';
export type { MapsSdkErrorCodeValue } from './MapsSdk';
export type {
  MapCamera,
  MapViewFailure,
  MapViewFailurePhase,
  MapViewHandle,
  MapViewProps,
} from './MapView';
export type {
  DevicePositionFix,
  PositionSourceFailureReason,
  ProviderCredentialResult,
  SdkRegistrationResult,
  UrlTemplateRasterImageFormat,
  UrlTemplateRasterTileSourceDefinition,
  UrlTemplateTileResolution,
  UrlTemplateVectorTileSource,
  VectorTileSourceStatus,
} from './MapsSdk';
export { rustic_maps };
CURATED_ROOT_EXPORTS
    printf '==> appended the curated root exports to src/index.ts\n'
}

readonly BINDING_OUTPUT_PATHS=(
    "${GENERATED_TS_DIR}"
    "${GENERATED_CPP_DIR}"
)

# Everything `generate jsi turbo-module` writes into the package root. The
# list names each output so a drift report points at the file, not a
# directory.
readonly TURBO_MODULE_OUTPUT_PATHS=(
    "MapsSdkRn.podspec"
    "android/CMakeLists.txt"
    "android/build.gradle"
    "android/cpp-adapter.cpp"
    "android/src/main/AndroidManifest.xml"
    "android/src/main/java/com/mapssdkrn/MapsSdkRnModule.kt"
    "android/src/main/java/com/mapssdkrn/MapsSdkRnPackage.kt"
    "cpp/maps-sdk-rn.cpp"
    "cpp/maps-sdk-rn.h"
    "ios/MapsSdkRn.h"
    "ios/MapsSdkRn.mm"
    "src/NativeMapsSdkRn.ts"
    "src/index.ts"
)

run_bindings=false
run_turbo_module=false
run_check=false
for argument in "$@"; do
    case "$argument" in
        --bindings) run_bindings=true ;;
        --turbo-module) run_turbo_module=true ;;
        --check) run_check=true ;;
        *) printf 'generate-rn-bindings: unknown argument %s\n' "$argument" >&2; exit 1 ;;
    esac
done
if [ "$run_bindings" = false ] && [ "$run_turbo_module" = false ]; then
    run_bindings=true
    run_turbo_module=true
fi

if [ ! -x "$UBRN_COMMAND" ]; then
    printf 'generate-rn-bindings: %s is missing; run npm ci in platform/rn first\n' "$UBRN_COMMAND" >&2
    exit 1
fi

generate_bindings_into() {
    local project_root="$1"
    # Both output directories hold nothing but generated sources, and their
    # file names derive from the contract namespace: wipe them so a namespace
    # from an earlier contract state cannot linger beside the current one.
    rm -rf "${project_root}/${GENERATED_TS_DIR}" "${project_root}/${GENERATED_CPP_DIR}"
    (
        cd "$BINDING_CRATE_DIR"
        "$UBRN_COMMAND" generate jsi bindings "$FROZEN_UDL_PATH" \
            --crate "$BINDING_CRATE_NAME" \
            --ts-dir "${project_root}/${GENERATED_TS_DIR}" \
            --cpp-dir "${project_root}/${GENERATED_CPP_DIR}"
    )
}

generate_turbo_module_into() {
    local project_root="$1"
    (
        cd "$project_root"
        "$UBRN_COMMAND" generate jsi turbo-module "$UNIFFI_NAMESPACE_NAME" \
            --config ubrn.config.yaml
    )
    # The ubrn TurboModule output is a starting point, not an app-build
    # input: the known gaps are re-applied here so every generation — the
    # build script, the packaging script, a bare run and the drift check —
    # produces the same buildable tree.
    ensure_libandroid_link_line "$project_root"
    ensure_codegen_spec_include_on_ios "$project_root"
    ensure_generated_bindings_guard_in_cmake "$project_root"
    ensure_curated_root_exports "$project_root"
}

# The temporary project needs a `package.json` for ubrn to resolve a project
# root, and the config's Rust paths are relative to `platform/rn`, which is
# not the temporary directory's parent, so they are rewritten absolute. The
# TurboModule outputs themselves stay relative to the project root. ubrn
# finds prettier by walking up from the output directory, so the package's
# installed `node_modules` is linked in to keep TypeScript formatting
# identical to an in-place generation.
prepare_temporary_project() {
    local project_root="$1"
    cp "${RN_PROJECT_ROOT}/package.json" "${project_root}/package.json"
    ln -s "${RN_PROJECT_ROOT}/node_modules" "${project_root}/node_modules"
    local temporary_config="${project_root}/ubrn.config.yaml"
    sed \
        -e "s|^  src: .*crates/rustic-maps-native-core$|  src: ${BINDING_CRATE_DIR}|" \
        -e "s|^  manifestPath: .*crates/rustic-maps-native-core/Cargo.toml$|  manifestPath: ${BINDING_CRATE_DIR}/Cargo.toml|" \
        "${RN_PROJECT_ROOT}/ubrn.config.yaml" > "$temporary_config"
    if ! grep -qF "src: ${BINDING_CRATE_DIR}" "$temporary_config"; then
        printf 'generate-rn-bindings: could not point the temporary ubrn config at %s\n' \
            "$BINDING_CRATE_DIR" >&2
        exit 1
    fi
}

require_working_tree_generation() {
    local missing=false
    if [ "$run_bindings" = true ]; then
        if [ ! -d "${RN_PROJECT_ROOT}/${GENERATED_TS_DIR}" ] \
            || [ ! -d "${RN_PROJECT_ROOT}/${GENERATED_CPP_DIR}" ]; then
            printf 'generate-rn-bindings: no generated JSI bindings at %s; run the generation first\n' \
                "$RN_PROJECT_ROOT" >&2
            missing=true
        fi
    fi
    if [ "$run_turbo_module" = true ] && [ ! -f "${RN_PROJECT_ROOT}/src/index.ts" ]; then
        printf 'generate-rn-bindings: no generated TurboModule glue at %s; run the generation first\n' \
            "$RN_PROJECT_ROOT" >&2
        missing=true
    fi
    if [ "$missing" = true ]; then
        exit 1
    fi
}

drift_found=false

compare_generated_file() {
    local relative_path="$1"
    local working_path="${RN_PROJECT_ROOT}/${relative_path}"
    local comparison_path="${comparison_project_root}/${relative_path}"
    if [ ! -e "$working_path" ] && [ ! -e "$comparison_path" ]; then
        return
    fi
    if [ ! -e "$working_path" ]; then
        printf 'generate-rn-bindings: stale: %s is absent from the working tree\n' \
            "$relative_path" >&2
        drift_found=true
        return
    fi
    if [ ! -e "$comparison_path" ]; then
        printf 'generate-rn-bindings: stale: %s is absent from a fresh generation\n' \
            "$relative_path" >&2
        drift_found=true
        return
    fi
    if ! cmp -s "$working_path" "$comparison_path"; then
        printf 'generate-rn-bindings: stale: %s differs from a fresh generation\n' \
            "$relative_path" >&2
        drift_found=true
    fi
}

compare_generated_path() {
    local relative_path="$1"
    local working_path="${RN_PROJECT_ROOT}/${relative_path}"
    local comparison_path="${comparison_project_root}/${relative_path}"
    if [ ! -d "$working_path" ] && [ ! -d "$comparison_path" ]; then
        compare_generated_file "$relative_path"
        return
    fi
    local child_relative_path
    while IFS= read -r child_relative_path; do
        compare_generated_file "${relative_path}/${child_relative_path}"
    done < <(
        {
            if [ -d "$working_path" ]; then
                (cd "$working_path" && find . -type f | sed 's|^\./||')
            fi
            if [ -d "$comparison_path" ]; then
                (cd "$comparison_path" && find . -type f | sed 's|^\./||')
            fi
        } | sort -u
    )
}

if [ "$run_check" = true ]; then
    require_working_tree_generation
    comparison_root="$(mktemp -d "${TMPDIR:-/tmp}/rustic-maps-rn-bindings-check.XXXXXX")"
    trap 'rm -rf "$comparison_root"' EXIT
    comparison_project_root="${comparison_root}/project"
    mkdir -p "$comparison_project_root"
    prepare_temporary_project "$comparison_project_root"
    if [ "$run_bindings" = true ]; then
        generate_bindings_into "$comparison_project_root"
    fi
    if [ "$run_turbo_module" = true ]; then
        generate_turbo_module_into "$comparison_project_root"
    fi

    if [ "$run_bindings" = true ]; then
        for relative_path in "${BINDING_OUTPUT_PATHS[@]}"; do
            compare_generated_path "$relative_path"
        done
    fi
    if [ "$run_turbo_module" = true ]; then
        for relative_path in "${TURBO_MODULE_OUTPUT_PATHS[@]}"; do
            compare_generated_path "$relative_path"
        done
    fi
    if [ "$drift_found" = true ]; then
        printf 'generate-rn-bindings: the generated React Native bindings are stale; rerun scripts/generate-rn-bindings.sh and review the diff\n' >&2
        exit 1
    fi
    printf 'react native bindings are current: every generated file matches a fresh generation\n'
    exit 0
fi

if [ "$run_bindings" = true ]; then
    generate_bindings_into "$RN_PROJECT_ROOT"
fi

if [ "$run_turbo_module" = true ]; then
    generate_turbo_module_into "$RN_PROJECT_ROOT"
fi

printf 'react native bindings generated under %s\n' "$RN_PROJECT_ROOT"
