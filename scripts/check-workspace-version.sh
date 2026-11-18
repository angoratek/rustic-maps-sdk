#!/usr/bin/env bash
# Assert the single-version policy of docs/PackagingAndRelease.md §5: the
# workspace version (`Cargo.toml` `[workspace.package].version`) is the one
# version line, and every release package manifest that declares a version
# must agree with it. The release workflow runs this on the tagged tree and
# also asserts that the tag itself names the same version.
#
# What is checked today (the release package surface of §2):
#   - platform/rn/package.json           (the @angoratek/rustic-maps-sdk
#                                        React Native package version)
#   - platform/rn/ubrn.config.yaml      (the ubrn package config version)
#   - platform/android/maps-sdk/build.gradle.kts   (the one published AAR's
#                                                  Maven version)
#                                       (the Maven publication's
#                                        `version = "..."`)
#   - platform/web/npm/package.json     (the @angoratek/rustic-maps-sdk-web
#                                        WebAssembly package version)
#
# What is deliberately not checked: the reference applications (the Android,
# iOS and RN app shells). They are demo applications distributed through the
# docs site, not release packages; §5 stamps the Android, iOS, React Native
# and WebAssembly packages, and each app shell carries its own app version.
# platform/web/package.json is the private Playwright harness manifest, not a
# release package. platform/ios/Package.swift has no version field by design:
# Swift Package Manager package versions are the release tags themselves.
#
# Usage:
#   scripts/check-workspace-version.sh
#   scripts/check-workspace-version.sh --tag v0.1.0     # assert a release tag
#   scripts/check-workspace-version.sh --print-version # print the version only
#
# Exit codes: 0 success, 1 a version mismatch or a malformed version/tag,
# 2 a usage error.
set -euo pipefail

readonly REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly WORKSPACE_MANIFEST="${REPO_ROOT}/Cargo.toml"
# SemVer with an optional pre-release suffix (`0.1.0-rc.1`); build metadata
# is deliberately not accepted, matching the tag discipline of §5.
readonly VERSION_PATTERN='^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z][0-9A-Za-z.-]*)?$'
readonly TAG_PREFIX="v"

declare -a failures=()
declare -a checked_manifest_paths=()
declare -a notes=()
workspace_version=""
release_tag=""
print_version_only=0

usage() {
    cat >&2 <<'USAGE'
usage: check-workspace-version.sh [--tag <vX.Y.Z>] [--print-version]

  --tag <tag>        also assert that the release tag (for example v0.1.0)
                     names the workspace version
  --print-version    print only the workspace version on success
USAGE
}

fail() {
    printf 'error: %s\n' "$1" >&2
    exit 1
}

read_workspace_version() {
    # The version key inside the `[workspace.package]` section only; the
    # workspace dependencies below carry their own `version = …` keys.
    awk '
        /^\[workspace\.package\][[:space:]]*$/ { in_section = 1; next }
        /^\[/ { in_section = 0 }
        in_section && /^version[[:space:]]*=/ {
            line = $0
            sub(/^version[[:space:]]*=[[:space:]]*/, "", line)
            sub(/[[:space:]]*#.*$/, "", line)
            gsub(/"/, "", line)
            gsub(/[[:space:]]/, "", line)
            print line
            exit
        }
    ' "${WORKSPACE_MANIFEST}"
}

read_json_version() {
    local manifest="$1"
    grep -m1 -oE '"version"[[:space:]]*:[[:space:]]*"[^"]*"' "${manifest}" \
        | sed -E 's/.*"([^"]*)"$/\1/'
}

read_yaml_version() {
    local manifest="$1"
    grep -m1 -E '^version:[[:space:]]*' "${manifest}" \
        | sed -E 's/^version:[[:space:]]*"?([^"#[:space:]]+)"?.*/\1/'
}

read_android_version_name() {
    local manifest="$1"
    grep -m1 -E '^[[:space:]]*versionName[[:space:]]*=' "${manifest}" \
        | sed -E 's/.*=[[:space:]]*"([^"]*)".*/\1/'
}

read_android_module_version() {
    # `versionName` is the Android module's declared version; the Maven
    # publication block the M10 rename adds may instead carry a bare
    # `version = "..."`. Either one is the version being packaged.
    local manifest="$1"
    local declared_version=""

    declared_version="$(read_android_version_name "${manifest}" || true)"
    if [ -n "${declared_version}" ]; then
        printf '%s' "${declared_version}"
        return 0
    fi
    grep -m1 -E '^[[:space:]]*version[[:space:]]*=' "${manifest}" \
        | sed -E 's/.*=[[:space:]]*"([^"]*)".*/\1/'
}

check_manifest() {
    local description="$1"
    local manifest_relative_path="$2"
    local declared_version="$3"

    if [ -z "${declared_version}" ]; then
        failures+=("${description}: no version key could be read from ${manifest_relative_path}")
        return
    fi
    checked_manifest_paths+=("${manifest_relative_path}")
    if [ "${declared_version}" != "${workspace_version}" ]; then
        failures+=("${description}: ${manifest_relative_path} declares \"${declared_version}\" but ${WORKSPACE_MANIFEST#"${REPO_ROOT}/"} [workspace.package].version is \"${workspace_version}\"")
    fi
}

check_android_aar_module() {
    local manifest_relative_path="platform/android/maps-sdk/build.gradle.kts"
    local manifest="${REPO_ROOT}/${manifest_relative_path}"
    local declared_version=""

    if [ -f "${manifest}" ]; then
        declared_version="$(read_android_module_version "${manifest}" || true)"
    fi
    if [ -n "${declared_version}" ]; then
        check_manifest "Android AAR module" "${manifest_relative_path}" "${declared_version}"
    else
        notes+=("${manifest_relative_path}: no versionName or Maven version declared (the AAR would be versioned by the release tag only)")
    fi
}

check_web_package() {
    local manifest_relative_path="platform/web/npm/package.json"
    local manifest="${REPO_ROOT}/${manifest_relative_path}"
    local declared_version=""

    if [ -f "${manifest}" ]; then
        declared_version="$(read_json_version "${manifest}" || true)"
    fi
    if [ -n "${declared_version}" ]; then
        check_manifest "WebAssembly package" "${manifest_relative_path}" "${declared_version}"
    else
        failures+=("WebAssembly package: no version key could be read from ${manifest_relative_path}")
    fi
    notes+=("platform/web/package.json: the private Playwright harness manifest, not a release package; its version is not stamped")
}

check_release_tag() {
    local tag_version

    if [[ "${release_tag}" != "${TAG_PREFIX}"* ]]; then
        failures+=("release tag \"${release_tag}\" does not carry the required \"${TAG_PREFIX}\" prefix (expected ${TAG_PREFIX}${workspace_version})")
        return
    fi
    tag_version="${release_tag#"${TAG_PREFIX}"}"
    if ! [[ "${tag_version}" =~ ${VERSION_PATTERN} ]]; then
        failures+=("release tag \"${release_tag}\" does not name a semantic version")
        return
    fi
    if [ "${tag_version}" != "${workspace_version}" ]; then
        failures+=("release tag \"${release_tag}\" names version \"${tag_version}\" but ${WORKSPACE_MANIFEST#"${REPO_ROOT}/"} [workspace.package].version is \"${workspace_version}\"")
    fi
}

parse_arguments() {
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --tag)
                if [ "$#" -lt 2 ]; then
                    usage
                    exit 2
                fi
                release_tag="$2"
                shift 2
                ;;
            --print-version)
                print_version_only=1
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

main() {
    parse_arguments "$@"

    [ -f "${WORKSPACE_MANIFEST}" ] || fail "missing workspace manifest ${WORKSPACE_MANIFEST}"
    workspace_version="$(read_workspace_version)"
    [ -n "${workspace_version}" ] || fail "could not read the version from [workspace.package] in ${WORKSPACE_MANIFEST}"
    if ! [[ "${workspace_version}" =~ ${VERSION_PATTERN} ]]; then
        fail "workspace version \"${workspace_version}\" is not a semantic version (expected MAJOR.MINOR.PATCH with an optional pre-release suffix)"
    fi

    local rn_package_version rn_ubrn_config_version
    rn_package_version="$(read_json_version "${REPO_ROOT}/platform/rn/package.json" || true)"
    rn_ubrn_config_version="$(read_yaml_version "${REPO_ROOT}/platform/rn/ubrn.config.yaml" || true)"
    check_manifest "React Native package" "platform/rn/package.json" "${rn_package_version}"
    check_manifest "React Native ubrn config" "platform/rn/ubrn.config.yaml" "${rn_ubrn_config_version}"
    check_android_aar_module
    check_web_package
    notes+=("platform/ios/Package.swift: SwiftPM manifests carry no version field; the package version is the release tag by design")
    notes+=("reference applications (platform/android/reference-application, platform/ios/reference-application, platform/rn/app-shell) are demo apps with their own app versions, not release packages")

    if [ -n "${release_tag}" ]; then
        check_release_tag
    fi

    if [ "${#failures[@]}" -gt 0 ]; then
        for failure in "${failures[@]}"; do
            printf 'error: %s\n' "${failure}" >&2
        done
        exit 1
    fi

    if [ "${print_version_only}" -eq 1 ]; then
        printf '%s\n' "${workspace_version}"
        return 0
    fi

    if [ -n "${release_tag}" ]; then
        printf 'ok: workspace version %s matches %d release manifest(s) and release tag %s\n' \
            "${workspace_version}" "${#checked_manifest_paths[@]}" "${release_tag}"
    else
        printf 'ok: workspace version %s matches %d release manifest(s)\n' \
            "${workspace_version}" "${#checked_manifest_paths[@]}"
    fi
    for note in "${notes[@]}"; do
        printf 'note: %s\n' "${note}"
    done
}

main "$@"
