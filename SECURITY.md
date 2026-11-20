# Security policy

## Supported versions

The SDK is pre-1.0. Security fixes land on the latest commit of `main`. Older
commits and tags receive no backports until the first stable release. Build from
the latest `main` when you need a security fix.

## Report a vulnerability

Report privately through GitHub private vulnerability reporting:

<https://github.com/angoratek/rustic-maps-sdk/security/advisories/new>

Do not open a public issue and do not send a report by email.

Include:

- the affected component, platform, and version, release tag, or commit
- the steps that reproduce the issue, with a minimal example when possible
- the impact you observed or expect
- the environment: operating system, Rust version, Node.js version, device or
  simulator, and browser
- any mitigation you already know

Use the private advisory thread for all follow-up. Do not publish details until
a fix ships and the advisory is published.

## Response targets

- Acknowledge a complete report within 3 business days.
- Triage it, with a severity assessment and a next step, within 10 business
  days.

## Scope

In scope:

- the Rust core crates, the renderer, navigation, positioning, and the offline
  routing graph
- the UniFFI contract and the generated Kotlin, Swift, TypeScript, and
  WebAssembly bindings
- the Android, iOS, React Native, and WebAssembly wrappers
- the build and release pipeline in `scripts/` and `.github/workflows/`
- the documentation site and the browser demo

Out of scope:

- vulnerabilities in third-party dependencies that are already public; report
  those upstream and open a dependency update here
- the content of the committed recordings. Synthetic payloads and style
  documents created for tests contain no live personal data. Recorded provider
  payloads and imagery, including the raster tiles and the OpenStreetMap US
  vector tiles, are third-party data; the terms recorded for each provider are
  in the Terms and vendored assets section of `docs/Providers.md` and in each
  provider's `terms_note` in `platform/providers.json`
- findings that require a credential the reporter does not own

## Accepted risks

- **`activesupport` below 8.0.5.2 in the React Native build toolchain (GHSA-8474-xrp6-wh69), accepted 2026-11-19, review 2027-02-19.** Every `cocoapods-core` release declares `activesupport >= 5.0, < 8`, so the Gemfile cannot resolve a fixed release while CocoaPods remains the iOS dependency manager; the Gemfile already holds the newest release below 8. The advisory affects a build-time tool, never a shipped artefact, and the Dependabot alert is dismissed with this reason. It is revisited when a CocoaPods release accepts activesupport 8 or the React Native build moves off CocoaPods.

## Credentials

Provider credentials are runtime-only. The SDK never commits, embeds, logs, or
distributes a provider credential. A test or a build that needs one receives it
at runtime or from an environment variable on the build machine.

Never include a credential in a report, a proof of concept, a log, or a
screenshot. If you believe a credential is present in the repository, say where
without reproducing the value.
