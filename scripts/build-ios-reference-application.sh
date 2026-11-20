#!/usr/bin/env bash
# Builds the iOS reference application for the simulator (M11 / M24).
#
# `scripts/build-ios.sh` produces the XCFramework and verifies the Swift
# package; the *application* is a separate Xcode project that is generated
# from `project.yml` by xcodegen and never committed (`.gitignore` names
# `*.xcodeproj`). A checkout therefore has no project until this script runs
# xcodegen — a fresh CI runner included.
#
# The destination is a concrete simulator: the XCFramework carries device and
# simulator arm64 slices but no x86_64 slice, so a generic simulator
# destination asks for an architecture the framework does not have and the
# link fails.
#
# Every build is keyless: a credential is entered at runtime through the
# settings surface or injected for one automation session, never compiled in.
#
# Usage:
#   scripts/build-ios-reference-application.sh
set -euo pipefail

readonly REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly APPLICATION_DIRECTORY="${REPO_ROOT}/platform/ios/reference-application"
readonly SIMULATOR_DESTINATION="platform=iOS Simulator,name=iPhone 17 Pro"
# The refapp-only bindings `scripts/build-refapp-core.sh ios` generates. The
# project references both paths, so a missing generation is reported here with
# the command that produces it rather than as an xcodegen path error.
readonly REFAPP_CORE_GENERATED_SOURCES="${APPLICATION_DIRECTORY}/Generated/RefappCore"
readonly REFAPP_CORE_XCFRAMEWORK="${APPLICATION_DIRECTORY}/Generated/refapp-core/map_refapp_coreFFI.xcframework"

if ! command -v xcodegen >/dev/null 2>&1; then
    printf '%s\n' 'build-ios-reference-application: xcodegen is required (brew install xcodegen).' >&2
    exit 1
fi

if [ ! -d "${REFAPP_CORE_GENERATED_SOURCES}" ] || [ ! -d "${REFAPP_CORE_XCFRAMEWORK}" ]; then
    printf '%s\n' 'build-ios-reference-application: the refapp-only bindings are missing; run scripts/build-refapp-core.sh ios first.' >&2
    exit 1
fi

(
    cd "${APPLICATION_DIRECTORY}"
    xcodegen generate
)

xcodebuild \
    -project "${APPLICATION_DIRECTORY}/ReferenceApplication.xcodeproj" \
    -scheme ReferenceApplication \
    -configuration Debug \
    -destination "${SIMULATOR_DESTINATION}" \
    -derivedDataPath "${APPLICATION_DIRECTORY}/build" \
    ONLY_ACTIVE_ARCH=YES \
    build
