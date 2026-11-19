# Release

A release is a tag. Pushing `vX.Y.Z` runs `.github/workflows/release.yml`, which
verifies the gate set on the tagged tree, builds one package per compiled target,
assembles checksums and Sigstore signatures, and opens a draft GitHub Release.
Publishing the draft is manual. A published release is immutable: a bad release is
superseded by a new tag, never patched in place. Distribution is GitHub Releases only:
no Maven Central, npm registry or Swift Package Manager registry entry. Each platform
installs from a downloaded archive, and every package carries its licenses and
third-party notices. The downloads page states that no versioned release is published
yet and every package is built from source.

## Versioning and identities

**One version line.** The workspace version in `Cargo.toml` (`version = 0.1.0`) is the
single source of truth. It is stamped into the Maven publication version, both npm
versions, the Swift Package Manager manifest and tag, and the wasm package.
`scripts/check-workspace-version.sh` refuses a build when any file disagrees; the
release gates run it first. **Semantic versioning, 0.x until 1.0.** The UniFFI contract
is addition-only, so contract breaks do not exist. Packaging breaks can: an artifact-set
change or a renamed target. While the version stays 0.x, a breaking packaging change
bumps the minor. **Tag discipline.** `vX.Y.Z` tags trigger a release; `vX.Y.Z-rc.N` runs
the same workflow with a pre-release marker. Tags are never moved or re-pushed. A broken
release is superseded by a new tag, and the failed asset stays so its checksums keep the
history honest. The tag version must match the workspace version. **MSRV.** The
workspace declares `rust-version = "1.89"` and edition 2024. The gate toolchain is
pinned in `rust-toolchain.toml` at 1.99.0. **Changelog.** Every meaningful change lands
in `CHANGELOG.md`. Release notes come from the section whose heading matches the tag;
the curated public changelog chapter is refreshed in the same documentation commit.
Unreleased work stays under `[Unreleased]`.

### Package identities

| Surface | Identity |
|---|---|
| Maven | `com.angoratek:rustic-maps-sdk`, namespace `com.rusticmaps.sdk` |
| npm, React Native | `@angoratek/rustic-maps-sdk` |
| npm, WebAssembly | `@angoratek/rustic-maps-sdk-web` |
| Swift Package Manager product | `RusticMapsSDK`, generated module `rustic_mapsFFI` |
| Swift wrapper target | `MapNativeCoreUI` |
| Generated UniFFI namespace | `rustic_maps` |

**Rustic Maps SDK** is the product name; **Rustic Maps SDK Ref App** is the artifact
name on the reference applications' installer, launcher, `CFBundleDisplayName` and
Appetize listing. Neither touches a published identity or an Appetize build id.

## The artifacts

A release carries one package per compiled target plus the notices, the install README
and the dual license files. `SHA256SUMS.txt`, a `.sha256` sidecar per file and a
`.sigstore.json` bundle per signed file complete the set. The workflow asserts the exact
set in both directions.

| Platform | Release asset | Contents | Measured size |
|---|---|---|---|
| Android | `rustic-maps-sdk-android-vX.Y.Z.aar` | Kotlin bindings, per-ABI Rust libraries, `MapView`/Compose wrapper | about 16 MiB; stripped libraries 12 MiB arm64-v8a, 8.7 MiB armeabi-v7a, 13 MiB x86_64 |
| iOS | `rustic-maps-sdk-ios-vX.Y.Z.xcframework.zip` | static library, headers, module maps, licenses, notices | 217 MiB per slice, 207 MiB after `strip -x` |
| React Native | `rustic-maps-sdk-rn-vX.Y.Z.tgz` | TurboModule sources, Android static archives, iOS framework | 249 MiB download, 786 MiB unpacked |
| WebAssembly | `rustic-maps-sdk-web-vX.Y.Z.tgz` | ESM set, wasm module, declarations, licenses, notices | module held to an 8 MiB budget |
| All platforms | `rustic-maps-sdk-notices-vX.Y.Z.md` | third-party notices for every package | — |
| All platforms | `rustic-maps-sdk-vX.Y.Z-README.md` | install and verification snippets per platform | — |
| All platforms | `LICENSE-MIT`, `LICENSE-APACHE` | the dual license | — |

**Self-containment.** Each compiled package carries everything needed to link it: the RN
tarball vendors the Android archives and iOS framework, the AAR embeds its consumer R8
rules, and the XCFramework and wasm tarballs carry their headers, declarations, licenses
and notices. Consumers need no Rust. The Rust crates stay source-only;
`scripts/assemble-release-artifacts.sh` also assembles
`rustic-maps-sdk-rust-vX.Y.Z.tar.gz` and checksums.

### Android — the AAR

`scripts/build-android.sh` cross-builds the `rustic-maps-native-core` cdylib for
`arm64-v8a`, `armeabi-v7a` and `x86_64` at the API-24 NDK linker level, regenerates the
Kotlin bindings from the frozen UDL metadata, stages sources and per-ABI
`librustic_maps.so` files into `platform/android/map-sdk-native/src/main/jniLibs`, and
runs `gradle :maps-sdk:assembleRelease` (AGP 9.0.0, Gradle wrapper 9.5.1, compileSdk 36,
minSdk 24). `:maps-sdk` compiles those sources and the Compose wrapper into the one AAR;
`:map-sdk-native` publishes nothing. The published dependencies are
`net.java.dev.jna:jna:5.18.1@aar` and
`org.jetbrains.kotlinx:kotlinx-coroutines-core:1.10.2`. The Compose wrapper is
`compileOnly` and does not reach the POM; a Compose host supplies the BOM,
`androidx.compose.ui:ui` and `androidx.lifecycle:lifecycle-runtime-compose:2.9.4`. The
`maven-publish` block declares group `com.angoratek`, artifact `rustic-maps-sdk`, and
the single version line. `consumer-rules.pro` keeps the JNI bridge, JNA dispatch and
generated `uniffi` names intact for minified applications.

```kotlin
repositories { flatDir { dirs("libs") } }
dependencies {
    implementation(files("libs/rustic-maps-sdk-android-0.1.0.aar"))
    implementation("net.java.dev.jna:jna:5.18.1@aar")   // UniFFI runtime
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.10.2")
}
```

### iOS — the XCFramework

`scripts/build-ios.sh` builds the `rustic-maps-native-core` `staticlib` for
`aarch64-apple-ios` and `aarch64-apple-ios-sim`, then joins the slices with their
headers and modulemaps into `platform/ios/artifacts/rustic_mapsFFI.xcframework`. A
static archive is consumed by Swift Package Manager binary targets with no run-time
framework embedding. `platform/ios/Package.swift` exposes the binary target from a local
path, pins `swift-tools-version: 5.10` and `.iOS(.v17)`, and adds the `MapNativeCoreUI`
wrapper with the shared registry as a `.copy` resource. For distribution, the
XCFramework is zipped with the `.xcframework` directory at the zip root, and the
checksum is `swift package compute-checksum <zip>` (the same SHA-256 the workflow
computes with `sha256sum`). `scripts/generate-swift-package-manifest.sh` emits a
`Package.swift` on the `swift-package` branch declaring
`binaryTarget(name:url:checksum:)`. After the draft uploads its assets, the workflow
force-pushes one commit with the manifest, the generated bindings
(`Sources/RusticMapsSDK/rustic_maps.swift`), the wrapper sources and the
licenses/notices; `workflow_dispatch` skips the push. Consumers add `.package(url:
"https://github.com/angoratek/rustic-maps-sdk.git", branch: "swift-package")`; the
branch is rebuilt per release, so pin `revision:` when needed. The archive stays
unsigned (ad-hoc): a static library is not run-time code-signable, and integrity is
pinned by the Swift Package Manager checksum and the release-side signature. The
licenses and notices are copied **inside** the `.xcframework` directory before zipping,
because Swift Package Manager requires exactly one top-level item. The privacy manifest
`platform/ios/PrivacyInfo.xcprivacy` declares the disk-space required-reason API
(`NSPrivacyAccessedAPICategoryDiskSpace`, reason `E174.1`) that the Rust tile cache
uses; `scripts/build-ios.sh` installs it into the generated target and copies it into
the XCFramework. Each static slice is 217 MiB, 207 MiB after `strip -x`; a `staticlib`
carries every compiled object, so the 10 MiB per-architecture payload budget cannot be
read off its file size. The slices cover arm64 only.

### React Native — the TurboModule package

`scripts/build-rn.sh` runs `ubrn generate jsi bindings` over the frozen UDL and `ubrn
generate jsi turbo-module` (the glue: `src/NativeMapsSdkRn.ts`, `src/index.ts`, `cpp/`),
builds the Rust static archives for the three ABIs into `android/src/main/jniLibs`, and
typechecks with `tsc --noEmit`. The generated `MapsSdkRn.podspec` declares
`uniffi-bindgen-react-native` 0.31.0-6; `scripts/check-rn-pod-swift.sh` type-checks the
pod's Swift sources. The release package renames `package.json` to
`@angoratek/rustic-maps-sdk`, drops `"private": true`, and adds the `files` allow-list,
the `react` and `react-native` peer dependencies, and the license expression. The
allow-list names exactly the surface the podspec and CMake consume:

```json
"files": [
  "src", "cpp", "android", "android/src/main/jniLibs", "ios",
  "AngoratekRusticMapsSdkFramework.xcframework", "MapsSdkRn.podspec",
  "tsconfig.json", "LICENSE-MIT", "LICENSE-APACHE",
  "THIRD_PARTY_NOTICES.md"
]
```

`npm pack` adds `package.json`, `README` and `LICENSE`, and excludes `node_modules`,
lock files, `.git` and log files. The pack step regenerates the glue from the frozen UDL
and asserts the expected file set. The tarball ships the three Android ABIs as
`android/src/main/jniLibs/<abi>/librustic_maps.a`, plus the arm64 device/simulator
`AngoratekRusticMapsSdkFramework.xcframework` that ubrn builds.
`scripts/package-rn-npm.sh --with-native-payload` packs the payload, and
`scripts/assemble-release-artifacts.sh` stages it from the platform builds (the Android
static archives and the renamed `rustic_mapsFFI.xcframework`). The archives use
`SUPPORTED_ANDROID_ABIS`, the same table `scripts/build-android.sh` cross-builds.
Staging happens in a fresh temporary directory, so a leftover payload cannot leak in;
`--dry-run` prints the exact member list. `scripts/verify-rn-package.mjs` unpacks the
tarball and reads its packaged manifests: the podspec's `vendored_frameworks` (through
each slice's `Info.plist`), the CMake `IMPORTED_LOCATION` archive for every ABI in the
Gradle `ndk { abiFilters }` list, the CMake sources, and every file `build.gradle`
names. It checks each exists, the version matches the workspace, and the `files`
allow-list covers every artifact. Install with `npm install
./rustic-maps-sdk-rn-vX.Y.Z.tgz`.

### WebAssembly — the wasm package

`scripts/generate-wasm-bindings.sh` builds `rustic-maps-native-core` release with
`wasm-bindgen-cli` locked at `0.2.129` (the CLI must match the crate), generates the
contract TypeScript, and stages the glue and module. `scripts/build-wasm2-package.sh`
compiles the ESM set and `.d.ts` declarations and emits `platform/web/generated`:
`index.js`, `rustic_maps.js`, `rustic_maps-ffi.js`, `rustic_maps_bg.js`,
`rustic_maps.wasm` and the declarations. The packaging script runs the module through
`wasm-opt -Oz`; the release job calls it with `--require-optimize` and installs binaryen
first. `scripts/check-wasm2-package-freshness.mjs` records a SHA-256 of the git-tracked
generation inputs beside the emit, recomputes it before the Playwright suite and `npm
pack`, and refuses a package whose sources changed, whose module exceeds the 8 MiB
budget, or whose npm version differs from the workspace version.
`scripts/package-web-npm.sh` produces `@angoratek/rustic-maps-sdk-web` from the
`generated/` emit, `types/browser_canvas.d.ts` published as `rustic_maps_bg.d.ts`, a
README, licenses and notices. `@ubjs/core` and `@ubjs/wasm` are runtime dependencies. A
consumer serves the package directory or points at the demo; `npm pack` writes
deterministic bytes and the script asserts the exact member set. This is the **only**
WebAssembly asset: a non-npm consumer extracts it and serves `package/`.

> WebGPU is required. Chromium-based browsers have supported it flag-free since 113
> (2023); Safari ships it from version 26; Firefox ships it since 141, with desktop
> first. It is a secure-context API, so the page must be served over HTTPS (or
> localhost) and `navigator.gpu` is the feature test to use — no user-agent sniffing.

### Notices, licenses and checksums

The notices are regenerated at release time by `scripts/generate-third-party-notices`,
byte-compared against `docs/THIRD_PARTY_NOTICES.md` as a fail-closed check, and shipped
as `rustic-maps-sdk-notices-vX.Y.Z.md`, one file for every package. The generator's
`cargo metadata --offline` resolves against the registry index; the docs-publication
workflow's priming step (`cargo metadata --format-version 1` first) is reused verbatim.
The dual license ships as `LICENSE-MIT` and `LICENSE-APACHE` verbatim in every package
and as release assets: under `assets/licenses/` in the AAR (staged by
`scripts/build-android.sh`), inside the `.xcframework` directory, in both npm tarballs,
and at the wasm package root. Every asset carries a `.sha256` sidecar, and
`SHA256SUMS.txt` aggregates them.

## The release workflow

`.github/workflows/release.yml` has the same publish-fidelity habits as the
documentation pipeline: gates on the exact tree, a provenance stamp, and a separate
deliberate publish.

```
push tag v0.4.0
  ├─ gates      the standard gate set on this exact ref
  ├─ plan       input hash per platform → run or reuse
  ├─ android    rustic-maps-sdk-android-v0.4.0.aar
  ├─ ios        rustic-maps-sdk-ios-v0.4.0.xcframework.zip
  ├─ rn         rustic-maps-sdk-rn-v0.4.0.tgz
  ├─ wasm       rustic-maps-sdk-web-v0.4.0.tgz
  ├─ restore    republishes reusable artifacts, bytes re-hashed
  └─ assemble   verify the asset set, regenerate notices, generate the
                README, hash and sign, draft the release, push swift-package
```

**Gates on the tagged commit.** The standard set (`cargo fmt --check`, `cargo clippy
--workspace --all-targets -- -D warnings`, the wasm-closure lib clippy, `cargo test
--workspace`, the Playwright suite) runs first and every packaging job depends on it. A
prior green main-branch run is not a proxy. **Dry run.** `workflow_dispatch` runs the
jobs in full, including the README, checksums and signatures, but skips the release API
call and the `swift-package` push, uploading `dist/` as a workflow artifact. **Draft
publication.** Publishing the draft (`--draft=false` or Publish on the release page) is
manual. A pre-release tag adds `--prerelease`. Release notes come from the
`CHANGELOG.md` section whose heading matches the tag. **Install README.**
`scripts/generate-release-readme.sh` assembles `rustic-maps-sdk-vX.Y.Z-README.md` from
the version and repository only, with no timestamp, host or environment value, so its
bytes are deterministic. It names every asset and carries the per-platform install
snippet (Android `flatDir` AAR, iOS `swift-package` branch, React Native and WebAssembly
npm tarballs), checksum and Sigstore commands, and license/notices pointers;
`SHA256SUMS.txt`, its `.sha256` sidecar and its `.sigstore.json` bundle cover it.

### Input-gated rebuilds

A repeated run after an assembly or publish failure should not rebuild unchanged
packages. The `plan` job computes one SHA-256 input hash per platform from the tagged
commit's tracked files: its sources, the Rust workspace closure (`Cargo.toml`,
`Cargo.lock`, `crates/`), the build and packaging scripts, the release workflow, the
version-bearing manifests, and the license and notice files. The React Native hash
includes the Android and iOS hashes; the iOS hash includes `platform/providers.json` and
`platform/reference-scenes.json`. Each build job writes
`release-artifact-<platform>.json` beside its artifact:

```json
{
  "schemaVersion": 1,
  "platform": "android",
  "version": "0.4.0",
  "commit": "…",
  "inputHash": "sha256:…",
  "artifacts": [
    { "name": "rustic-maps-sdk-android-v0.4.0.aar", "sha256": "…" }
  ]
}
```

A build job is skipped only when a completed run for the same revision recorded an input
hash for that platform, that hash equals the current tree's, and the record names the
artifact and its SHA-256 while the prior run still holds every artifact the job uploads,
including the React Native payload companions. Anything else runs the build. The
`restore` job downloads a reusable artifact, re-checks its bytes, and republishes it;
`scripts/restore-release-artifact.sh` exits non-zero on a mismatch. The assemble job
repeats the check and still asserts the exact asset set.

```bash
node scripts/release-plan.mjs --base "$(git describe --tags --abbrev=0)"
node scripts/release-plan.mjs --base v0.3.0 --head HEAD --json
```

The table prints each platform's input hash, changed input count, and run-or-reuse
decision. Both revisions are commits; run it from a clean checkout of the release
commit. `--prior-directory` points at a prior run's records. A missing record never
skips a build.

## Signing and provenance

Every asset and `SHA256SUMS.txt` is signed with keyless Sigstore cosign. No long-lived
secret is needed: the runner's GitHub Actions OIDC identity is the signing material. The
workflow requests `contents: write` and `id-token: write`. `--bundle` writes the
signature and certificate together as `<asset>.sigstore.json`, and `--yes` is required
in non-interactive CI. The OIDC token binds each bundle to the workflow repository and
tag ref:

```bash
cosign verify-blob <asset> \
  --bundle <asset>.sigstore.json \
  --certificate-identity-regexp "https://github.com/angoratek/rustic-maps-sdk/.github/workflows/release.yml@refs/tags/vX.Y.Z" \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com"
```

Keyless signing is preferred over a GPG secret, which would need rotation and expiry
management. The verification command ships in the release notes; verify by checksum
first, then signature. **Reproducibility, stated honestly.** The jobs are deterministic
per commit for the notices, checksums and bindings codegen. Compiler caches and CI
runners are not, so byte-level reproducibility of the native binaries is not claimed or
verified. The artifact manifest is what consumers verify.

## The documentation site

The site lives at `angoratek.github.io/rustic-maps-sdk-docs`. It carries the demo, the
rustdoc API reference at `site/api/` and the guides through one pipeline:
`scripts/build-docs-site.sh` stages, renders, scrubs, stamps `provenance.txt` (source
commit, dirty-tree flag, generator, content license, map attribution), and publishes
from a clean tree to `gh-pages`. **Build gates.** A chapter change runs `node
scripts/check-docs-references.mjs` (no orphaned or dangling `docs/` reference), `node
scripts/check-doc-snippets.mjs` (every fenced block compiles against the generated
surface) and `node scripts/check-reference-flows.mjs` (the reference flows agree with
the shared copy, provider registry and platform sources). A site change runs `bash
scripts/build-docs-site.sh`. A publish requires `--publish --gates-green=<commit>`: the
flag names the commit whose gate set passed, `PAGES_REPO_DIR` points at a
pages-repository checkout, and the asserted commit must equal `HEAD` or the publish is
refused. The CI `docs-publication` workflow builds and verifies the staging (including a
byte-for-byte notices check) and uploads it; publication is a deliberate local or
dispatched action. **Pre-publish scrub.** A fail-closed scrub runs over the staged site.
It denies the paths `PLAN.md`, `PRD.md`, `CLAUDE.md`, `.env`, `local.properties` and
`Local.xcconfig`; it denies the content `dev.mapssdk`, the loopback address and
`com.angoratek`, with a narrow allowlist for published package-name forms in the
installation and demo chapters. The scrub self-checks first. **Downloads page.** The
page states that no versioned release is published yet and every package is built from
source. A published release uses `/downloads/vX.Y.Z/` as an immutable record: each file
has a `.sha256` sidecar and `.sigstore.json` bundle, `SHA256SUMS.txt` covers the set,
and no file is rewritten. Files under the host's limits (100 MB per file, 1 GB site) are
mirrored; larger ones ship only with the release. The version page prints the
verification identity.

### The live wasm demo at `/demo`

The demo is the browser harness served statically, keyless and gated. It ships a
directory whose layout answers the same URLs the dev server maps.

| Served path | Source |
|---|---|
| `/demo/` | `platform/web/index.html` plus `main.mjs`, `map_controls.mjs`, `overlay_scenes.mjs`, copied 1:1 |
| `/demo/generated/*` | `platform/web/generated/*`: the ESM set, declarations and `rustic_maps.wasm` |
| `/demo/generated-runtime/@ubjs/*` | the page's player runtime from `platform/web/node_modules/@ubjs` |
| `/demo/tiles/*` | the recorded tile fixtures in `crates/rustic-maps-rendering/fixtures/render/tiles` |
| `/demo/reference-scenes.json` | `platform/reference-scenes.json` |
| `/demo/offline-graph/routing_grid.rmog` | `crates/rustic-maps-offline-graph-compiler/fixtures/rmog/routing_grid.rmog` |

The `/golden/` route family is test-only and the demo bundles none of it. The demo
`index.html` is a curated copy; its only edit is the footer, with the version stamp from
the wasm `get_sdk_compiled_version` probe and a link to the release that shipped it.
Everything else stays byte-equal to the harness page. The demo deploys statically,
because GitHub Pages cannot run Node; it is keyless and renders recorded fixture tiles
and the recorded offline graph. Attribution comes from the shared registry on the page.
A missing `navigator.gpu` shows the documented degradation message with the MDN link.
The directory carries the same `provenance.txt` fields and the `gates-green=<commit>`
assertion. The full Playwright suite must have passed against this exact bundle, and
copied files are byte-equality-asserted against `platform/web`. The scrub covers
`/demo`, and the staged tree stays under 25 MB, enforced by `LIVE_DEMO_MAXIMUM_BYTES`.

## Screenshots

The published set is six captures per platform from one revision: four flow frames and a
layers-control pair. Publish them together; never update one frame or one platform
alone.

1. **Plan sheet.** Origin and Destination holding Capitol Park and Tower Bridge, each
   Search action, the My location chips, swap and Plan route.
2. **Provider section.** Every capability row reading its composed `{display name} ·
   {credential state}` line, the applied credential's row, and the serving provider on
   each picker.
3. **Navigation.** The Capitol Park ⇔ Tower Bridge trip at its **first instruction**,
   with turn banner, then line, route polyline, ETA-distance line and playback speed at
   **1×**.
4. **Vector base.** The styled OpenFreeMap Bright base with street and place labels,
   asserting it serves: the Serving mark on iOS and React Native, the Bright row and
   vector tile mode on the web, and the vector source status (style id, provider, drawn
   tiles) on Android.

The navigation marker is the same everywhere; captures are phone width only. The layers
pair, from one keyed session: **closed** is the map on the credential provider's raster
base with the layers and attribution controls on the end edge; **open** is the same
session and camera with the panel scrolled to the last base-map rows, traffic, overlays
with **Clear overlays**, and device location.

| Requirement | Value |
| --- | --- |
| Android device | the standing Pixel_9 emulator, booted |
| Android SDK and NDK | `ANDROID_HOME` set; the NDK version `scripts/build-android.sh` pins |
| iOS simulator | the standing iPhone 17 Pro, booted |
| Xcode and xcodegen | the pinned Xcode; `xcodegen` on `PATH` |
| React Native | Metro on port 8081 from `platform/rn/app-shell`, and the debug app installed |
| Web | `platform/web` dependencies installed and a WebGPU-capable Chromium |
| Credential | a MapQuest key in the repository root `.env` as `MAPQUEST_CONSUMER_KEY` |

No capture run compiles the credential into a build; every run injects it into the
session through the runtime seam the reference applications document.

### Credential handling

Load the key into the capture shell without printing it:

```bash
set -a
. /path/to/repository/.env
set +a
```

Verify it once with a live tile request; the command uses the environment variable,
never a literal key, and prints only the response status:

```bash
curl -sS -o /tmp/mapquest-probe.png \
  -w '%{http_code} %{content_type}\n' \
  "https://www.mapquestapi.com/tiles/v1/render/latest/vivid/13/1873/3374/256/png?key=${MAPQUEST_CONSUMER_KEY}"
file /tmp/mapquest-probe.png
```

Expected: `200 image/png` and a `PNG image data` file. Stop and report anything else; do
not capture with a refused key.

### Per-platform capture

**Android.** The opt-in instrumentation case is
`platform/android/reference-application/src/androidTest/kotlin/com/rusticmaps/sdk/referenceapplication/ReferenceApplicationScreenshotCaptureTest.kt`,
on the Pixel_9 emulator at 1080×2200.

```bash
# A fresh checkout has no generated bindings, reference-application bindings
# or AAR; produce them once.
./scripts/build-android.sh
./scripts/build-refapp-core.sh android

cd platform/android
./gradlew :reference-application:assembleDebugAndroidTest

adb install -r reference-application/build/outputs/apk/debug/reference-application-debug.apk
adb install -r reference-application/build/outputs/apk/androidTest/debug/reference-application-debug-androidTest.apk

adb shell am instrument -w \
  -e class 'com.rusticmaps.sdk.referenceapplication.ReferenceApplicationScreenshotCaptureTest#capturesTheKeyedPlanSheetProviderSectionNavigationAndVectorFrames' \
  -e captureReferenceScreenshots true \
  -e providerApiKey "$MAPQUEST_CONSUMER_KEY" \
  -e providerKey mapquest \
  com.rusticmaps.sdk.referenceapplication.test/androidx.test.runner.AndroidJUnitRunner

adb pull /sdcard/Android/data/com.rusticmaps.sdk.referenceapplication/files/reference-screenshots/. \
  tmp/reference-screenshots/android/
```

The four-frame method asserts the style id, provider key and drawn tile count before the
vector frame. The layers pair replaces the class selector with
`#capturesTheKeyedLayersPanelClosedAndOpenFrames` and runs the same command. Use `adb
shell am instrument`, not the Gradle connected task, which uninstalls the app and its
capture directory before the host can pull it. **iOS.** The opt-in XCUITest is
`platform/ios/reference-application/Tests/ReferenceApplicationUITests/ReferenceApplicationScreenshotCaptureTest.swift`,
on the iPhone 17 Pro simulator. A class-level run carries six attachments.

```bash
./scripts/build-ios.sh
./scripts/build-refapp-core.sh ios

cd platform/ios/reference-application
xcodegen generate
REPOSITORY_ROOT="$(git rev-parse --show-toplevel)"

TEST_RUNNER_CAPTURE_REFERENCE_SCREENSHOTS=1 \
TEST_RUNNER_PROVIDER_API_KEY="$MAPQUEST_CONSUMER_KEY" \
TEST_RUNNER_PROVIDER_KEY=mapquest \
xcodebuild test -project ReferenceApplication.xcodeproj \
  -scheme ReferenceApplication -sdk iphonesimulator \
  -destination 'platform=iOS Simulator,name=iPhone 17 Pro' \
  -only-testing:ReferenceApplicationUITests/ReferenceApplicationScreenshotCaptureTest \
  -resultBundlePath \
  "$REPOSITORY_ROOT/tmp/reference-screenshots/ios/ReferenceApplicationScreenshots.xcresult"

xcrun xcresulttool export attachments \
  --path "$REPOSITORY_ROOT/tmp/reference-screenshots/ios/ReferenceApplicationScreenshots.xcresult" \
  --output-path "$REPOSITORY_ROOT/tmp/reference-screenshots/ios/attachments"
```

Rename the attachments from `manifest.json` to `ios-plan-sheet.png`,
`ios-provider-section.png`, `ios-navigation.png`, `ios-vector-base.png`,
`ios-layers-closed.png` and `ios-layers-open.png`. The `TEST_RUNNER_` prefix is
required: `xcodebuild` does not forward the host environment. The layers pair recaptures
with
`-only-testing:ReferenceApplicationUITests/ReferenceApplicationScreenshotCaptureTest/testCapturesTheLayersPanelSet`
and `-resultBundlePath .../LayersPanelScreenshots.xcresult`. **React Native.** The
runner is `platform/rn/app-shell/scripts/capture_reference_screenshots.mjs`, on the same
Pixel_9 emulator with Metro on port 8081. It needs `scripts/build-android.sh`'s AAR,
`scripts/generate-rn-bindings.sh`'s JSI and TurboModule sources and static archives,
`:app:assembleDebug`, and `adb reverse tcp:8081 tcp:8081`; it clears app data before
each launch.

```bash
adb reverse tcp:8081 tcp:8081
PROVIDER_API_KEY="$MAPQUEST_CONSUMER_KEY" PROVIDER_KEY=mapquest \
  node platform/rn/app-shell/scripts/capture_reference_screenshots.mjs
```

Output lands under `tmp/reference-screenshots/react-native/`; the pair is captured on
the credential provider's raster base before a styled base is selected. **WebAssembly.**
The opt-in Playwright spec is
`platform/web/tests/capture_reference_screenshots.spec.mjs`. It applies the credential
through the page's own settings surface and keeps provider traffic same-origin with
`?useProviderFixtures`.

```bash
cd platform/web
CAPTURE_REFERENCE_SCREENSHOTS=1 PROVIDER_API_KEY="$MAPQUEST_CONSUMER_KEY" \
  npx playwright test tests/capture_reference_screenshots.spec.mjs --workers=1
```

Output lands under `tmp/reference-screenshots/web/`. Every frame is 430 px wide; the
provider frame uses 430×1600 and the other three 430×932. The pair is a second opt-in
test with the plan sheet aside, keeping the same camera and serving base.

```bash
cd platform/web
CAPTURE_REFERENCE_SCREENSHOTS=1 PROVIDER_API_KEY="$MAPQUEST_CONSUMER_KEY" \
  npx playwright test tests/capture_reference_screenshots.spec.mjs \
  --workers=1 -g "layers panel"
```

The pair lands as `layers-closed.png` and `layers-open.png`.

### Publish the reviewed set

Copy the reviewed files from one revision into `site/book-src/img/`: four flow frames
and two layers frames per platform, 24 PNGs (`android-*`, `ios-*`, `react-native-*`,
`wasm-*` × `plan-sheet`, `provider-section`, `navigation`, `vector-base`,
`layers-closed`, `layers-open`). Then run the gates:

```bash
node scripts/check-docs-references.mjs
node scripts/check-doc-snippets.mjs
node scripts/check-reference-flows.mjs
bash scripts/build-docs-site.sh
```

### Capture log

| Field | Value |
| --- | --- |
| Revision | the 2026-11-04 capture revision |
| Date | 2026-11-04 |
| Navigation marker | first instruction shown, 1× playback speed |
| Four-frame base | the registry's keyless styled base, `openfreemap` `bright-light` |
| Layers-pair base | the applied credential provider's raster base, `mapquest` |
| Android evidence | `tmp/reference-screenshots/android/` |
| iOS evidence | `tmp/reference-screenshots/ios/` |
| React Native evidence | `tmp/reference-screenshots/react-native/` |
| Web evidence | `tmp/reference-screenshots/web/` |

Recapture the whole six-capture set on every platform from one revision. The settings
frames must carry the nine-key catalog and its twelve descriptors.

### What must never appear

A credential must never appear in a published image, page, generated text file or
capture log. Run this after the set is copied and before publishing; it reads the key
from the environment, never prints it, and exits non-zero on a hit:

```bash
python3 - <<'PY'
import os
from pathlib import Path

needle = os.environ["PROVIDER_API_KEY"].encode()
roots = [
    Path("site/book-src"),
    Path("tmp/reference-screenshots"),
    Path("platform/web/test-results"),
]
text_suffixes = {".md", ".json", ".mjs", ".js", ".txt", ".log", ".xml", ".html"}
hits = []
for root in roots:
    if not root.exists():
        continue
    for path in root.rglob("*"):
        if not path.is_file():
            continue
        if path.suffix == ".png" or path.suffix in text_suffixes:
            data = path.read_bytes()
        else:
            continue
        if needle in data:
            hits.append(str(path))
if hits:
    raise SystemExit("credential found in: " + ", ".join(hits))
print("credential check: clean")
PY
```

## The hosted mobile demo

The demo at `/demo/mobile/` streams the Android, iOS and React Native reference
applications through Appetize, one tab per platform. **The free plan.** 30 streamed
minutes per month, shared by every visitor; 3-minute sessions, so at most ten full
sessions a month. Sessions open in their own tab; a link with embed parameters may show
Appetize's "app may not be embedded" notice. Nothing streams until **Start demo** is
pressed. **The claim service.** `platform/web/mobile-demo/mobile-demo.mjs` asks a claim
endpoint for a session; the default is
`https://rustic-maps-demo-claim.vercel.app/api/claim`, overridable through the page's
`demo-claim-endpoint` meta element. It enforces the monthly streamed-minutes budget, a
per-visitor daily session count, and an optional operator bypass through
`?operatorToken=`. The visitor is a randomly generated local token, not an IP address,
so shared addresses do not lock each other out. The page never holds the operator
secret; the token travels only in the URL the operator opens. When the daily count is
reached, the page rests with "You have used today's demo sessions. Try again tomorrow."
and the month's remaining minutes. A grant reserves the whole 3-minute session before
the stream starts, so an abandoned claim can never overrun the free plan. That
reservation is pessimistic: a claim where the visitor never presses Appetize's own Start
button costs the budget but streams nothing. Before every decision the endpoint
therefore reconciles its monthly counter with Appetize's usage report (`GET
/v1/usageSummary`, the account key in the endpoint's `APPETIZE_API_KEY` environment
variable): the counter drops to what actually streamed plus one session of headroom for
claims in flight, and never below the report or above the reservation. Without the key,
or when the report is unreachable, the reservation stands alone, which can only refuse a
session early. The page's `status=true` readout reports the reconciled `streamedSeconds`
beside the counter. **Reviewing the demo.** Append the operator token:

```
https://<demo-host>/demo/mobile/?operatorToken=<operator-token>
```

The claim endpoint grants an operator session without counting against the daily
allowance; streamed minutes still count against the month. When the month runs out, the
page rests until it rolls over; raising the budget is an Appetize plan change.
**Re-applying session settings.** The uploader updates settings only when it uploads. To
re-apply them without a new artifact, for example after an account-level change dropped
the referrer allow-list:

```bash
set -a; source .env; set +a
node scripts/upload-demo-builds.mjs --reapply-settings
```

It reads each build id from `platform/demo-builds.json` and sends the timeout,
three-minute limit, one-session concurrency, referrer allow-list and button text as a
JSON body, then verifies the allow-list reads back. Builds and ids are unchanged.
**Rebuilding.** `scripts/upload-demo-builds.mjs` updates the three published Appetize
builds in place: Android, iOS and React Native Android. A React Native iOS entry exists
but is unpublished, so it has no build id. Build ids never change. Re-upload after any
change that should reach visitors, then confirm the app starts in a session. **The
dedicated credential policy.** The Android and iOS hosted builds may carry a credential
no other build carries. It is:

- **dedicated**: issued for the demo alone, revoked and re-issued on rotation, never the
  key the fixture tools and the live-smoke example use;
- **injected on the build machine** from the repository secret
  `HOSTED_DEMO_PROVIDER_API_KEY`, never stored in the repository and never written into
  a file git tracks;
- **read at startup** through the same public registration path as an automation
  injection, never persisted to the Keystore or Keychain, never logged;
- **absent from a normal build**, which therefore stays keyless.

`scripts/inject-hosted-demo-credential.sh` writes the Android
`assets/hosted-demo-credential.json` (gitignored) and deletes it with `--clean`, so a
local build cannot silently become keyed. For iOS it writes nothing: the application
reads `HostedDemoProviderAPIKey` from `Info.plist`, expanded from
`HOSTED_DEMO_PROVIDER_API_KEY` (optionally paired with `HOSTED_DEMO_PROVIDER_KEY`) on
the `xcodebuild` command line. The `.github/workflows/hosted-demo.yml` workflow builds
the three applications and uploads them only when the Appetize API token is present.

## Documentation snippets

Two tutorial chapters carry snippet markers and their marked code blocks are **generated from
compiled source** in the reference applications: `site/book-src/first-map.md` and
`site/book-src/search-and-routing.md`. Other blocks and chapters are hand-authored and
compile-checked. `scripts/check-doc-snippets.mjs` compiles every block against the
generated surfaces; `scripts/generate-doc-snippets.mjs` regenerates blocks from marked
regions. Both chapters walk one example: create the SDK, show a map, search two places,
plan and draw a route, start navigation, and make an offline region available. Each
platform tab uses the same order and identifiers (`mapSdk`, `apiKey`, `mapView`,
`mapCamera`, `searchQuery`, `originPlace`, `destinationPlace`, `plannedRoute`,
`routeOverlay`, `navigationController`, `listener`). **Marker format in source.** A
marked region begins and ends with a comment carrying the snippet identifier:

```kotlin
// docs-snippet: begin search-and-routing/plan-route
val plannedRoute = mapSdk.planRoute(originPlace, destinationPlace)
// docs-snippet: end search-and-routing/plan-route
```

The identifier is `<chapter>/<step>`. Markers are ordinary comments in the platform's
syntax (`//` for Kotlin, Swift, TypeScript and JavaScript). A region is a contiguous run
of complete statements at one indentation depth; the generator strips the common leading
whitespace. Regions may nest only if the outer region is itself a snippet. **Marker
format in a chapter.** A block names its identifier and platform:

```markdown
<!-- docs-snippet: begin search-and-routing/plan-route android -->
```kotlin
val plannedRoute = mapSdk.planRoute(originPlace, destinationPlace)
```
<!-- docs-snippet: end search-and-routing/plan-route android -->
```

The generator rewrites everything between the comments with the region it found, fenced
with the matching language. Prose is authored and reviewed; only the fenced content is
generated.

```sh
node scripts/generate-doc-snippets.mjs           # rewrite chapters in place
node scripts/generate-doc-snippets.mjs --check   # fail on any drift
```

`--check` exits non-zero and names every drifted, missing or orphaned region. It runs in
`.github/workflows/docs-snippets.yml` next to the compile check, so a reference
application that changes a region without regenerating the chapters fails CI, and a
chapter that keeps a block no source produces fails too. Sources come from
`docs/doc-snippets.json`, which names the platform roots to scan; no chapter hard-codes
a file path. **What belongs in a region.** It is code the application really runs.
Prefer SDK-facing statements over view plumbing. A step that needs a platform container
(a Compose `AndroidView`, a SwiftUI `UIViewRepresentable`) marks only the closure the
container calls into. Never mark a tutorial-only region.

## Public repository opening

Opening the project changes what the public sees, how contributions arrive and how the
release pipeline is trusted, but not the product surface.

### Decisions

1. **The public history is a curated sequence in a new repository.** Four commits built
   from real trees tell the opening story: the initial public snapshot, the browser API
   generated from the frozen contract, the documentation consolidated to eight guides,
   and the hosted-demo and reference fixes. Each commit is authored under the project's
   noreply identity, every object is rewritten to mask the credential, the session
   identifier, local paths and the maintainer's name, and the tip is byte-identical to
   the private tip. The full development history stays in the private repository, and
   the repository was created fresh, so no pre-rewrite object is reachable by SHA.
2. **Contributions carry a Developer Certificate of Origin sign-off.** No CLA. CI checks
   it with `scripts/check-dco-signoff.mjs`; commits stay one line with no trailers.
3. **The opening covers the repository, docs site, browser and mobile demos;** packages
   stay build-from-source until the first versioned release.
4. **Vulnerability reports arrive through GitHub private vulnerability reporting only.**
   No personal address is published.
5. **Internal planning documents are published sanitized.** Process vocabulary, billing
   state, credential operations and absolute local paths are removed; decisions and open
   questions survive.
6. **The browser demo keeps its opt-in remembered key, the one browser exception.** The
   rule names `localStorage` behind a visible warning, and the show-key toggle is
   removed so every platform masks the field.
7. **The Rust crates stay source-only until the first versioned release.**
8. **AI assistance is disclosed, never banned.** `CONTRIBUTING.md` states the
   responsibility rule and the pull-request template carries a checkbox.

### Working in the public repository

The public repository is the working repository. `main` there is the only
integration branch: changes arrive through `feat/…`, `fix/…` or `docs/…`
branches in pull requests that pass the aggregate gate and carry a DCO
sign-off. The pre-opening development history stays in the archived private
repository, and the public history is the curated sequence plus the work that
followed it, so no public commit depends on a private object.

Every open work item is one issue in the public repository, grouped under a
milestone (`M60`, `M82`, `Backlog`) whose description states when it is done. An
issue carries its acceptance criteria, and a pull request closes it. Labels
separate ordinary work (`plan`, `enhancement`, `documentation`, `bug`) from
`owner-action` items that need the maintainer's account, credentials or
deployment.

The documentation site and both demos build from the public history. No
workflow reads a secret that exists only in the private repository.

### Before opening

**Credential history.** An early recorded route fixture carried a provider consumer key.
It is revoked and rotated before publication, and the mirror replaces it, so no public
object carries it. The tip tree carries the redacted value, and no local environment
file is archived. Workflow hardening: every action pinned by commit SHA with a version
comment, least-privilege `permissions`, read-only jobs without persisted credentials,
secrets scoped to the consuming steps, Gradle wrapper digests, and dependency review,
CodeQL, Scorecard and actionlint jobs. Security fixes: rotate local credentials, address
the SQLite and memory-map advisories, cap HTTP response and image sizes, stop persisting
the browser credential beyond the session, remove the show-key affordance, scrub the
browser console export, and settle the demo signing keystore. Add description, homepage,
topics, documentation URL and Discussions. The community-health surface is in place:
license, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md` (Contributor Covenant 2.1),
`SECURITY.md`, `SUPPORT.md`, `GOVERNANCE.md`, `MAINTAINERS.md`, `ROADMAP.md`, issue
forms, a pull-request template, `CODEOWNERS`, Dependabot and a labeler. The public
reader must see product documentation, not process: retire implemented design notes
after moving every decision into `PRD.md` §6 or the changelog; fix contradictions,
including the screenshot rules and the stale downloads-page release claim; retire the
working plan once every open item lived in the issue tracker, and keep `CHANGELOG.md`
free of stale identifiers while preserving every decision;
rewrite public pages in product vocabulary, with no process, billing or
credential-operation language and no absolute paths; and replace pinned commit
references.

### Repository settings at opening

Apply these host-side settings in one pass so the repository never exists unprotected:

- Description naming the product and its four targets; homepage the docs site; topics
  `rust`, `maps`, `android`, `ios`, `react-native`, `webassembly`, `uniffi`,
  `openstreetmap`.
- Default branch `main`; squash-merge only; automatic branch deletion after merge;
  auto-merge off until rulesets are in place.
- Ruleset on `main`: require a pull request, the aggregate `format-and-lint` check and
  conversation resolution, and block force-pushes and deletions. Tag ruleset `v*`
  restricted to maintainers.
- Private vulnerability reporting, secret scanning, push protection, Dependabot alerts
  and automated security fixes all on.
- Actions: GitHub-authored and the pinned third-party set only, read-only permissions by
  default, no `pull_request_target`, no secrets to fork PRs.
- Environments `release` and `hosted-demo` with required reviewers and deployment
  branches limited to `main` and version tags.
- Discussions enabled; issue forms and the pull-request template are in the tree. The
  docs repository keeps its own deploy path; its deploy key is an account-level action,
  never a repository secret in the SDK.

## Related documentation

[Architecture](Architecture.md) covers the core, renderer and offline graph;
[Providers](Providers.md) the provider contracts and credential handling;
[Browser](Browser.md) the WebAssembly target and browser harness.
