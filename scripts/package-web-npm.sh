#!/usr/bin/env bash
# Package the generated WebAssembly bindings as the npm package
# `@angoratek/rustic-maps-sdk-web` and emit the release tarball
# `rustic-maps-sdk-web-v<version>.tgz` (docs/Release.md (The artifacts)).
#
# Inputs:
#   - platform/web/generated/  the generated `wasm2` ESM set with declarations
#   - platform/web/npm/       the committed npm manifest and README
#   - LICENSE-MIT, LICENSE-APACHE, docs/THIRD_PARTY_NOTICES.md
#
# The manifest is copied from platform/web/npm/package.json rather than
# rewritten here, so `scripts/check-workspace-version.sh` can assert its
# version against the workspace version. The staged tree is a temporary
# directory; `npm pack` writes the tarball to the output directory.
#
# The tarball's file list is asserted exactly (fail-closed): an unexpected
# extra file and a missing file both abort the packaging run. The app shell,
# test fixtures, and keystores can therefore never ride along.
#
# Usage:
#   scripts/package-web-npm.sh [--output-dir <directory>]
#
# The generated package must already be built:
#   scripts/build-wasm2-package.sh
#
# Idempotent: the staging tree is fresh on every run and the tarball is
# overwritten.
set -euo pipefail

readonly REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly WEB_ROOT="${REPO_ROOT}/platform/web"
readonly GENERATED_PACKAGE_DIR="${WEB_ROOT}/generated"
readonly NPM_PACKAGE_SOURCE_DIR="${WEB_ROOT}/npm"
readonly CANVAS_TYPES_PATH="${WEB_ROOT}/types/browser_canvas.d.ts"
readonly NPM_PACKAGE_NAME="angoratek-rustic-maps-sdk-web"
readonly RELEASE_ASSET_PREFIX="rustic-maps-sdk-web-v"
readonly VERSION_GUARD="${REPO_ROOT}/scripts/check-workspace-version.sh"
readonly NOTICES_PATH="${REPO_ROOT}/docs/THIRD_PARTY_NOTICES.md"
readonly MIT_LICENSE_PATH="${REPO_ROOT}/LICENSE-MIT"
readonly APACHE_LICENSE_PATH="${REPO_ROOT}/LICENSE-APACHE"
readonly EMITTED_PACKAGE_FILES=(
    "index.js"
    "index.d.ts"
    "rustic_maps.js"
    "rustic_maps.d.ts"
    "rustic_maps-ffi.js"
    "rustic_maps-ffi.d.ts"
    "rustic_maps_bg.js"
    "rustic_maps.wasm"
)
# The glue's hand-written declarations publish beside it under its own name.
readonly CANVAS_TYPES_FILE_NAME="rustic_maps_bg.d.ts"
# The exact npm tarball member set, sorted (tar order is not relied upon).
readonly EXPECTED_TARBALL_MEMBERS=(
    "package/LICENSE-APACHE"
    "package/LICENSE-MIT"
    "package/README.md"
    "package/THIRD_PARTY_NOTICES.md"
    "package/index.d.ts"
    "package/index.js"
    "package/package.json"
    "package/rustic_maps-ffi.d.ts"
    "package/rustic_maps-ffi.js"
    "package/rustic_maps.d.ts"
    "package/rustic_maps.js"
    "package/rustic_maps.wasm"
    "package/rustic_maps_bg.d.ts"
    "package/rustic_maps_bg.js"
)
# Fail-closed name guards; the exact-list assertion above already rejects
# these, but naming them yields an actionable message when one appears.
readonly DENIED_MEMBER_PARTS=("app-shell" "fixtures" "tests" ".keystore" "node_modules")

output_dir="${REPO_ROOT}/dist"
staging_dir=""

usage() {
    cat >&2 <<'USAGE'
usage: package-web-npm.sh [--output-dir <directory>]

  --output-dir <directory>  where the release tarball is written
                            (default: <repo>/dist)
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

assert_generated_package_present() {
    [ -d "${GENERATED_PACKAGE_DIR}" ] \
        || fail "missing ${GENERATED_PACKAGE_DIR#"${REPO_ROOT}/"}; build the wasm package first (see the script header)"
    local emitted_file
    for emitted_file in "${EMITTED_PACKAGE_FILES[@]}"; do
        [ -f "${GENERATED_PACKAGE_DIR}/${emitted_file}" ] \
            || fail "missing generated ${emitted_file} in ${GENERATED_PACKAGE_DIR#"${REPO_ROOT}/"}"
    done
    [ -f "${NPM_PACKAGE_SOURCE_DIR}/package.json" ] \
        || fail "missing npm manifest ${NPM_PACKAGE_SOURCE_DIR#"${REPO_ROOT}/"}/package.json"
    [ -f "${NPM_PACKAGE_SOURCE_DIR}/README.md" ] \
        || fail "missing npm README ${NPM_PACKAGE_SOURCE_DIR#"${REPO_ROOT}/"}/README.md"
}

read_manifest_version() {
    node -e 'process.stdout.write(require(process.argv[1]).version)' "$1"
}

stage_package() {
    staging_dir="$(mktemp -d)"
    local emitted_file
    for emitted_file in "${EMITTED_PACKAGE_FILES[@]}"; do
        cp "${GENERATED_PACKAGE_DIR}/${emitted_file}" "${staging_dir}/"
    done
    # The committed manifest is authoritative, so a package.json emitted by a
    # future generator can never overwrite the published identity.
    rm -f "${staging_dir}/package.json"
    cp "${NPM_PACKAGE_SOURCE_DIR}/package.json" "${staging_dir}/package.json"
    cp "${NPM_PACKAGE_SOURCE_DIR}/README.md" "${staging_dir}/README.md"
    # The hand-written canvas declarations type the wasm-bindgen glue beside it.
    cp "${CANVAS_TYPES_PATH}" "${staging_dir}/${CANVAS_TYPES_FILE_NAME}"
    cp "${MIT_LICENSE_PATH}" "${APACHE_LICENSE_PATH}" "${staging_dir}/"
    cp "${NOTICES_PATH}" "${staging_dir}/THIRD_PARTY_NOTICES.md"
}

assert_tarball_members() {
    local tarball="$1"
    local expected_file actual_file
    expected_file="$(mktemp)"
    actual_file="$(mktemp)"
    # shellcheck disable=SC2064
    trap "rm -f '${expected_file}' '${actual_file}'; cleanup" EXIT
    printf '%s\n' "${EXPECTED_TARBALL_MEMBERS[@]}" | sort > "${expected_file}"
    tar -tzf "${tarball}" | grep -v '/$' | sort > "${actual_file}"
    if ! diff -u "${expected_file}" "${actual_file}"; then
        fail "the npm tarball does not hold exactly the expected files"
    fi
    local denied_part
    for denied_part in "${DENIED_MEMBER_PARTS[@]}"; do
        if grep -qF "${denied_part}" "${actual_file}"; then
            fail "the npm tarball contains a denied path part: ${denied_part}"
        fi
    done
}

main() {
    parse_arguments "$@"
    require_tool node
    require_tool npm
    require_tool tar
    assert_generated_package_present
    # The emitted module must have come from the current sources and carry the
    # npm manifest's version; a stale package is never packed.
    node "${REPO_ROOT}/scripts/check-wasm2-package-freshness.mjs" \
        || fail "the generated WebAssembly package is stale or misversioned; run scripts/build-wasm2-package.sh"

    local workspace_version manifest_version
    workspace_version="$("${VERSION_GUARD}" --print-version)"
    manifest_version="$(read_manifest_version "${NPM_PACKAGE_SOURCE_DIR}/package.json")"
    [ "${manifest_version}" = "${workspace_version}" ] \
        || fail "platform/web/npm/package.json declares ${manifest_version}, but the workspace version is ${workspace_version}"

    mkdir -p "${output_dir}"
    output_dir="$(cd "${output_dir}" && pwd)"
    stage_package

    local packed_tarball release_tarball
    packed_tarball="${output_dir}/${NPM_PACKAGE_NAME}-${workspace_version}.tgz"
    release_tarball="${output_dir}/${RELEASE_ASSET_PREFIX}${workspace_version}.tgz"
    rm -f "${packed_tarball}" "${release_tarball}"
    # `npm pack` normalizes the archive's mtimes, so the tarball is
    # deterministic for a given tree and npm version.
    (cd "${staging_dir}" && npm pack --pack-destination "${output_dir}" >/dev/null)
    [ -f "${packed_tarball}" ] || fail "npm pack did not produce ${packed_tarball}"
    mv "${packed_tarball}" "${release_tarball}"

    assert_tarball_members "${release_tarball}"

    printf 'ok: %s (%s bytes, %s members)\n' \
        "${release_tarball}" "$(wc -c < "${release_tarball}" | tr -d ' ')" \
        "$(tar -tzf "${release_tarball}" | grep -vc '/$')"
}

main "$@"
