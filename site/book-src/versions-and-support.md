# Versions and support

This page explains how versions are numbered, what a released version line
receives, and how to report a problem. **No versioned release is published
yet.** [Requirements and distributions](requirements.md) lists the per-platform
requirements, and [Downloads](downloads.md) explains how to build from source.

## Version numbering

Versions follow semantic versioning as `MAJOR.MINOR.PATCH`. One workspace
version is the single source of truth: every platform package is stamped from
it at release time, so one version string identifies one core. The SDK is
pre-1.0, and the [FFI contract](udl-contract.md) is addition-only, so existing
members keep their signature and meaning across an upgrade. Packaging can still
change while the release is 0.x: an artifact-set change or a renamed target
bumps the minor version, and a pre-1.0 minor can break a build even when the
contract did not change. Test a minor upgrade.

## Pin exact versions

A published release archive is immutable and carries the version in the file
name, for example `rustic-maps-sdk-android-vX.Y.Z.aar`. Keep the exact version
you validated against, and upgrade deliberately. The version you pin also names
the notices file that accompanies it.

## Two release tracks

| Track | What it is | What it means for you |
|---|---|---|
| Stable | A `vX.Y.Z` tag and its release assets | The version to ship; the support statements on this page apply to it |
| Preview | A pre-release tag (`vX.Y.Z-rc.N`) from the same workflow, marked as a pre-release | Evaluation only; a preview is superseded by the stable release of the same version |

Tags are never moved, and a broken release is superseded by a new tag. The
failed release's assets and checksums stay. Fixes target the newest minor line.

## Deprecation

The addition-only rule means a released contract member keeps its signature and
meaning. A deprecation can still mark a member or a packaging detail as
superseded, and release notes and the changelog announce it before the release
that retires it.

## Checking the loaded version

`getSdkCompiledVersion` on every surface reports the version of the loaded
core. Compare it with the artifact you shipped; a mismatch means the loaded
core and your dependency declaration disagree.

## Getting support

Report a problem through the project's issue tracker with the
`getSdkCompiledVersion` output, your platform and operating system version, the
exact SDK version you pinned, and a minimal reproduction. For a rendering
problem, include the `FrameStatistics` of an affected frame. For a provider
problem, include the `SdkError` variant and its detail.
