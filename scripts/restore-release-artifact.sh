#!/usr/bin/env bash
# Restores a platform artifact that the release plan marked reusable and
# proves the bytes still match what the prior run recorded.
#
# The release workflow calls this once per reusable platform. It downloads the
# artifact and its `release-artifact-<platform>.json` record from the prior
# workflow run, then checks three things before the assemble job may use it:
#
#   1. the plan's decision for the platform is `reuse`;
#   2. the record's input hash equals the plan's current input hash;
#   3. every file the record names is present with its recorded SHA-256.
#
# Any mismatch exits non-zero. The assemble job still asserts the exact
# published asset set, so a restored artifact can never stand in for a missing
# package. Nothing here reaches the network except the GitHub CLI download of
# the prior run's artifacts.
#
# Usage:
#   scripts/restore-release-artifact.sh <platform> <prior-run-id> <plan-json>
#
# Environment: GH_TOKEN (the workflow token with actions: read).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly REPO_ROOT
readonly SUPPORTED_PLATFORMS=("android" "ios" "wasm" "rn")

usage() {
    cat >&2 <<'USAGE'
usage: restore-release-artifact.sh <platform> <prior-run-id> <plan-json>

  platform       android | ios | wasm | rn
  prior-run-id   the workflow run whose artifacts are restored
  plan-json      release-plan.json produced for this run
USAGE
}

fail() {
    printf 'restore-release-artifact: %s\n' "$1" >&2
    exit 1
}

platform="${1:-}"
prior_run_id="${2:-}"
plan_json="${3:-}"
if [ -z "${platform}" ] || [ -z "${prior_run_id}" ] || [ -z "${plan_json}" ]; then
    usage
    exit 2
fi

supported=false
for candidate in "${SUPPORTED_PLATFORMS[@]}"; do
    if [ "${candidate}" = "${platform}" ]; then
        supported=true
    fi
done
if [ "${supported}" = false ]; then
    fail "unsupported platform ${platform}"
fi
if [ ! -s "${plan_json}" ]; then
    fail "the release plan ${plan_json} is missing"
fi

readonly destination="${REPO_ROOT}/target/restored-release/${platform}"
readonly package_directory="${destination}/package"
readonly record_directory="${destination}/record"
readonly record_path="${record_directory}/release-artifact-${platform}.json"

decision="$(jq -r ".platforms.${platform}.decision" "${plan_json}")"
if [ "${decision}" != "reuse" ]; then
    fail "the plan does not mark ${platform} reusable (decision ${decision})"
fi
input_hash="$(jq -r ".platforms.${platform}.inputHash" "${plan_json}")"
version="$(jq -r '.version' "${plan_json}")"
if [ -z "${input_hash}" ] || [ "${input_hash}" = "null" ]; then
    fail "the plan has no input hash for ${platform}"
fi

rm -rf "${destination}"
mkdir -p "${package_directory}" "${record_directory}"
gh run download "${prior_run_id}" \
    --name "release-${platform}" \
    --dir "${package_directory}"
gh run download "${prior_run_id}" \
    --name "release-artifact-${platform}" \
    --dir "${record_directory}"

if [ ! -s "${record_path}" ]; then
    fail "the prior run has no ${platform} artifact record"
fi
recorded_input_hash="$(jq -r '.inputHash' "${record_path}")"
recorded_version="$(jq -r '.version' "${record_path}")"
recorded_platform="$(jq -r '.platform' "${record_path}")"
if [ "${recorded_platform}" != "${platform}" ]; then
    fail "the prior record names ${recorded_platform}, not ${platform}"
fi
if [ "${recorded_version}" != "${version}" ]; then
    fail "the prior record is for version ${recorded_version}, not ${version}"
fi
if [ "${recorded_input_hash}" != "${input_hash}" ]; then
    fail "the prior record's input hash does not match the current plan"
fi

artifact_count="$(jq -r '.artifacts | length' "${record_path}")"
if [ "${artifact_count}" -lt 1 ]; then
    fail "the prior record names no artifact bytes"
fi

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

while IFS=$'\t' read -r expected_sha256 artifact_name; do
    artifact_path="${package_directory}/${artifact_name}"
    if [ ! -f "${artifact_path}" ]; then
        fail "the restored ${platform} artifact ${artifact_name} is missing"
    fi
    actual_sha256="$(sha256_of "${artifact_path}")"
    if [ "${actual_sha256}" != "${expected_sha256}" ]; then
        fail "the restored ${platform} artifact ${artifact_name} does not match its recorded SHA-256"
    fi
done < <(jq -r '.artifacts[] | [.sha256, .name] | @tsv' "${record_path}")

printf 'restored %s from run %s: %s\n' \
    "${platform}" "${prior_run_id}" "${package_directory}"
