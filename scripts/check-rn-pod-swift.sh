#!/usr/bin/env bash
# Type-checks the React Native pod's Swift sources against the generated
# UniFFI bindings and the vendored FFI module headers.
#
# The pod target cannot yet be compiled by `xcodebuild` on Xcode 26: the
# `React-Core-prebuilt` explicit-module scan fails before the pod's own
# sources are compiled. This gate type-checks those sources directly, so a
# Swift error in the package's iOS bridge (a missing enum case, an
# Objective-C-representability mistake, a drifted member signature) is caught
# on every React Native build instead of waiting for the toolchain fix.
#
# Usage: scripts/check-rn-pod-swift.sh
#
# Prerequisites: Xcode with the iOS Simulator SDK, the generated
# `platform/rn/ios/generated/rustic_maps.swift` and the vendored
# `platform/rn/AngoratekRusticMapsSdkFramework.xcframework` (both produced by
# `scripts/build-rn.sh`).
set -euo pipefail

readonly REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly RN_ROOT="${REPO_ROOT}/platform/rn"
readonly GENERATED_BINDINGS="${RN_ROOT}/ios/generated/rustic_maps.swift"
readonly VENDORED_FRAMEWORK_HEADERS="${RN_ROOT}/AngoratekRusticMapsSdkFramework.xcframework/ios-arm64-simulator/Headers"
readonly SWIFT_POD_SOURCES=(
    "${RN_ROOT}/ios/MapsSdkRusticMapView.swift"
    "${RN_ROOT}/ios/MapsSdkNativeJson.swift"
    "${RN_ROOT}/ios/MapsSdkMapHostView.swift"
    "${RN_ROOT}/ios/MapsSdkNativeRuntime.swift"
    "${RN_ROOT}/ios/MapsSdkNativeEventChannel.swift"
)
readonly IOS_DEPLOYMENT_TARGET="17.0"
readonly SIMULATOR_ARCHITECTURE="arm64"

if ! command -v xcrun >/dev/null 2>&1; then
    printf 'check-rn-pod-swift: xcrun is required (Xcode command-line tools).\n' >&2
    exit 1
fi
if [ ! -f "${GENERATED_BINDINGS}" ]; then
    printf 'check-rn-pod-swift: %s is missing; run scripts/build-rn.sh first.\n' \
        "${GENERATED_BINDINGS}" >&2
    exit 1
fi
if [ ! -d "${VENDORED_FRAMEWORK_HEADERS}" ]; then
    printf 'check-rn-pod-swift: %s is missing; run scripts/build-rn.sh first.\n' \
        "${VENDORED_FRAMEWORK_HEADERS}" >&2
    exit 1
fi

readonly SIMULATOR_SDK_PATH="$(xcrun --sdk iphonesimulator --show-sdk-path)"
xcrun swiftc -typecheck \
    -swift-version 5 \
    -sdk "${SIMULATOR_SDK_PATH}" \
    -target "${SIMULATOR_ARCHITECTURE}-apple-ios${IOS_DEPLOYMENT_TARGET}-simulator" \
    -I "${VENDORED_FRAMEWORK_HEADERS}" \
    "${GENERATED_BINDINGS}" \
    "${SWIFT_POD_SOURCES[@]}"

printf 'ok: the React Native pod Swift sources type-check against the generated bindings\n'
