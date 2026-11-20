#!/usr/bin/env bash
# Writes the hosted demo's credential into a build, and nowhere else
# (the hosted-demo credential exception, 2026-10-30; see the issue tracker).
#
# The hosted demo runs on Appetize, where a visitor sees a live map instead
# of a mock scene only if the build can reach a real provider. That calls for
# a credential inside the artifact, which every other build must not have, so
# the credential is:
#
#   - dedicated: issued for the demo alone, revoked and re-issued on rotation,
#     never the key the fixture tools and the live-smoke example use;
#   - injected on the build machine from a secret, never stored in this
#     repository and never written into a file git tracks;
#   - read at startup through the same runtime path as an automation
#     injection (applied through the public registration path, never
#     persisted to the Keystore/Keychain, never logged);
#   - absent from a normal build, which therefore stays keyless.
#
# Android: writes `assets/hosted-demo-credential.json` into the reference
# application (gitignored). The file is deleted by `--clean`, so a local build
# left behind cannot silently become a keyed build.
#
# iOS: writes nothing. The application reads the credential from Info.plist,
# whose `HostedDemoProviderAPIKey` entry expands from the build setting
# `HOSTED_DEMO_PROVIDER_API_KEY`; pass it on the xcodebuild command line:
#
#   xcodebuild … HOSTED_DEMO_PROVIDER_API_KEY="$HOSTED_DEMO_PROVIDER_API_KEY" \
#                HOSTED_DEMO_PROVIDER_KEY="$HOSTED_DEMO_PROVIDER_KEY"
#
# Usage:
#   HOSTED_DEMO_PROVIDER_API_KEY=… scripts/inject-hosted-demo-credential.sh
#   scripts/inject-hosted-demo-credential.sh --clean
set -euo pipefail

readonly REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly CREDENTIAL_PATHS=(
    "${REPO_ROOT}/platform/android/reference-application/src/main/assets/hosted-demo-credential.json"
    "${REPO_ROOT}/platform/rn/app-shell/android/app/src/main/assets/hosted-demo-credential.json"
)
readonly DEFAULT_PROVIDER_KEY="mapquest"

clean() {
    local credential_path
    for credential_path in "${CREDENTIAL_PATHS[@]}"; do
        rm -f "${credential_path}"
        printf 'removed %s\n' "${credential_path#"${REPO_ROOT}/"}"
    done
}

if [ "${1:-}" = "--clean" ]; then
    clean
    exit 0
fi

credential="${HOSTED_DEMO_PROVIDER_API_KEY:-}"
provider_key="${HOSTED_DEMO_PROVIDER_KEY:-${DEFAULT_PROVIDER_KEY}}"

if [ -z "${credential}" ]; then
    printf '%s\n' 'inject-hosted-demo-credential: HOSTED_DEMO_PROVIDER_API_KEY is not set.' >&2
    printf '%s\n' 'Set the dedicated demo credential in the environment, or skip the hosted build:' >&2
    printf '%s\n' 'a build made without it is keyless by design.' >&2
    exit 1
fi

umask 077
for credential_path in "${CREDENTIAL_PATHS[@]}"; do
    mkdir -p "$(dirname "${credential_path}")"
    printf '{\n  "provider_key": "%s",\n  "api_key": "%s"\n}\n' \
        "${provider_key}" "${credential}" > "${credential_path}"
    printf 'wrote %s (provider %s; the credential itself is not printed)\n' \
        "${credential_path#"${REPO_ROOT}/"}" "${provider_key}"
done
