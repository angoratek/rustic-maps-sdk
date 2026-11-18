# Packaging and release

This document records the packaging rulings, the artifact matrix, the
release-workflow design, and the docs-site product requirements for the demo,
the API reference, the guides and the license surfaces. Every claim about the
repository is checked against the script, workflow or manifest it describes.

## 1. Decisions locked (owner, 2026-10-29)

Four rulings are locked. The rest of this document designs to exactly these.

### Ruling 1 — distribution through GitHub Releases only

Releases carry signed, tagged archives: the Android AAR, the iOS
XCFramework zip, the React Native npm tarball, the WebAssembly npm package,
plus a unified checksum manifest and the third-party notices. No Maven
Central, no npm registry, no Swift Package Manager registry.

Consequences:

- Each platform needs an install path that works without a registry. Swift
  Package Manager can consume a release asset directly through a
  `binaryTarget(url:checksum:)` manifest. Android installs a downloaded AAR
  from a local directory. React Native and WebAssembly install from the
  checked-into-a-project tarball rather than from `npm install <name>`.
- The SDK never publishes credentials, tokens, or registry accounts as part
  of the release pipeline. The only secrets a release workflow holds are the
  signing identity (with Sigstore keyless signing, none at all — the token
  is ambient in GitHub Actions).
- The release is immutable once published. A bad release is not patched in
  place; a new tag ships it. This matches how MapLibre runs its per-platform
  releases (draft, build, upload, publish) — see section 7.

### Ruling 2 — identity: the angoratek namespace

- Maven group `com.angoratek` (the published artifact is
  `com.angoratek:rustic-maps-sdk`).
- npm scope `@angoratek`, with the packages `@angoratek/rustic-maps-sdk`
  (React Native) and `@angoratek/rustic-maps-sdk-web` (WebAssembly).
- Swift product name `RusticMapsSDK`.

#### Product name vs. artifact name (recorded 2026-10-31, M26 slice E)

One product, two kinds of name, and they never trade places:

- **Product — "Rustic Maps SDK"**: the docs-site title, the README heading,
  both npm package descriptions, the Maven POM `name`/`description`, the
  release-note titles and the Rust crates' descriptions.
- **Artifact — "Rustic Maps SDK Ref App"**: the three reference
  applications' installer label, launcher name, `CFBundleDisplayName` and
  Appetize listing. They demonstrate the SDK; they are not the SDK.

Neither name touches a published package identity
(`com.angoratek:rustic-maps-sdk`, `@angoratek/rustic-maps-sdk`,
`@angoratek/rustic-maps-sdk-web`, SwiftPM `RusticMapsSDK` /
`MapNativeCoreUI`) or an Appetize build id.

Consequences in the current tree — all packaging identities, so none touches
the frozen UniFFI contract: the Swift package product is `RusticMapsSDK` (the
generated `rustic_mapsFFI` module keeps its name), the React Native package
carries the scoped npm identity, and the one Android artifact publishes under
group `com.angoratek`, artifact `rustic-maps-sdk`, namespace
`com.rusticmaps.sdk`. The pre-publish scrub in `scripts/build-docs-site.sh`
denies `com.angoratek` by default and allows the published package-name forms in
the installation and demo chapters; the scrub itself stays fail-closed.

### The hosted demo build is not a production artifact

The React Native reference application's release APK is signed with the
repository's debug keystore (`platform/rn/app-shell/android/app/debug.keystore`,
the standard AOSP debug key). That build exists only for the Appetize-hosted
demo and for local UI automation; it is never published as a store artifact or
offered as something an application ships. An application that distributes the
SDK signs its own build with its own keystore, as the release instructions
above describe. The keystore holds no production secret, and no credential is
present in the build beyond the dedicated hosted-demo key described in
`ProviderCredentialsAndBrowserProviders.md` §6.1.

### Ruling 3 — the docs site hosts the live wasm demo at `/demo`

The demo reuses the established publish pipeline (`--publish --gates-green=<commit>`, where the flag names the commit whose gates passed;
provenance stamp, byte-equality render gates, fail-closed scrub) of the
public docs repository `angoratek/rustic-maps-sdk-docs`. Section 4
specifies the bundle, its gates, and the requirements list.

### Ruling 4 — cadence: tag-driven release workflow

A `vX.Y.Z` tag push builds the full package matrix and drafts a GitHub
Release containing the binaries, the checksums, the third-party notices, and
the API-reference links. Publishing the draft is the owner's deliberate
step — the same publish-gating model `scripts/build-docs-site.sh` already
enforces with `--gates-green=<commit>`.

## 2. Artifact matrix

Sizes below were measured on the working tree of 2026-10-29 from the
artifacts the build scripts currently produce; the React Native tarball was
measured on 2026-11-01. Binary size is a PRD §5 gate
("Rust core (raster-only) ≤ 10 MiB per architecture after strip/thin"), so
the table calls out where the gate is and is not met.

| Platform | Artifact | Measured size | Per-architecture size against the 10 MiB gate |
|---|---|---|---|
| Android | `rustic-maps-sdk-android-vX.Y.Z.aar` (one artifact) | 16 MiB measured 2026-10-29 before the wrapper merge | arm64-v8a 12 MiB, armeabi-v7a 8.7 MiB, x86_64 13 MiB (`.so` after the release profile's `strip = true`) |
| iOS | `rustic_mapsFFI.xcframework` (2 slices) | 434 MiB total | 217 MiB per slice; 207 MiB after `strip -x` |
| React Native | `rustic-maps-sdk-rn-v0.1.0.tgz` | 249 MiB download, 786 MiB unpacked | three Android static archives (153–173 MiB each) and two arm64 iOS static archives (143 MiB each); the final link discards unused code |
| Wasm | `platform/web/generated` | 6.3 MiB | the generated ESM set plus `rustic_maps.wasm`, 6.2 MiB |

### 2.1 Android — the AAR

What exists today. `scripts/build-android.sh` cross-builds the `rustic-maps-native-core`
cdylib for three ABIs (`arm64-v8a`, `armeabi-v7a`, `x86_64`) at the API-24 NDK
linker level, regenerates the Kotlin bindings from the frozen UDL metadata
embedded in the host cdylib, stages the sources and per-ABI `librustic_maps.so`
files into `platform/android/map-sdk-native/src/main/jniLibs`, and runs
`gradle :maps-sdk:assembleRelease` (AGP 9.0.0, Gradle wrapper 9.5.1,
compileSdk 36, minSdk 24). `:maps-sdk` compiles those sources and the Compose
wrapper into the one AAR; `:map-sdk-native` is the internal source holder and
publishes nothing. The AAR's published dependencies are
`net.java.dev.jna:jna:5.18.1@aar` (the UniFFI Kotlin runtime) and
`org.jetbrains.kotlinx:kotlinx-coroutines-core:1.10.2` (the generated
`suspend fun` bindings). The Compose wrapper is optional, so its artifacts are
`compileOnly` inputs and do not reach the POM: a Compose host supplies the
Compose BOM, `androidx.compose.ui:ui` (which carries the Compose runtime) and
`androidx.lifecycle:lifecycle-runtime-compose:2.9.4` (the `LocalLifecycleOwner`
the wrapper reads) from its own application module. A host that places the
widget with `AndroidView` needs neither Compose artifact.

What the release adds.

- The `maven-publish` block on `:maps-sdk`: group `com.angoratek`, artifact
  `rustic-maps-sdk`, version declared in the module's build file (section 5's
  single-version policy). The POM carries the runtime dependencies, so a
  consumer's transitive resolution is exact.
- The SDK ships **one** Android AAR, `rustic-maps-sdk-android-vX.Y.Z.aar`, containing the generated Kotlin bindings, the per-ABI Rust libraries and the `MapView`/Compose wrapper. The `:map-sdk-native` Gradle module remains as the internal source holder `scripts/build-android.sh` generates into; it publishes nothing. A consumer adds one file.
- The AAR embeds the consumer R8 rules (`consumer-rules.pro`): the JNI surface
  bridge, JNA's reflective dispatch and the generated `uniffi` bindings keep
  their exact names, so a minified consumer application still links the Rust
  library at run time.
- Release asset name `rustic-maps-sdk-android-vX.Y.Z.aar`, a `.sha256`
  sidecar, a row in `SHA256SUMS.txt`, and a `.sigstore.json` signature
  bundle (section 3).
- README content shipped beside the asset (or in the release notes): the
  copy-paste installation snippet.

Installation snippet for integrators (the Gradle-documented `flatDir`
repository type; no registry involved):

```kotlin
repositories { flatDir { dirs("libs") } }
dependencies {
    implementation(files("libs/rustic-maps-sdk-android-0.1.0.aar"))
    implementation("net.java.dev.jna:jna:5.18.1@aar")   // UniFFI runtime
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.10.2")
}
```

A checksum manifest for the AAR needs at minimum the archive bytes, so a
checksum mismatch can be told apart from a corrupt download. The release
adds one `.sha256` sidecar per asset plus the aggregated `SHA256SUMS.txt`
covering every asset, which is what a mirror-verify script consumes.

### 2.2 iOS — the XCFramework

What exists today. `scripts/build-ios.sh` builds the `rustic-maps-native-core`
`staticlib` for `aarch64-apple-ios` (device) and `aarch64-apple-ios-sim`
(simulator) and joins the two slices with their headers and modulemaps into
`platform/ios/artifacts/rustic_mapsFFI.xcframework`. The script's header
records the design decision: a static archive is consumed straight out of
the XCFramework by Swift Package Manager binary targets with no run-time
framework embedding, while a dynamic library would require a `.framework`
slice and a signing step. `platform/ios/Package.swift` exposes the
`rustic_mapsFFI` binary target from a local path, pins
`swift-tools-version: 5.10` (Swift 5 language mode) and `.iOS(.v17)`, and
adds the `MapNativeCoreUI` wrapper target with the shared provider registry as
a `.copy` resource. The shared demo artifact is not a package resource: the
reference applications bundle `platform/reference-scenes.json` from its single
source, and the demo scenes loader and UX theme live in their source sets.

What the release adds.

- The SPM consumption path: the XCFramework is zipped with the `.xcframework`
  directory at the zip root (the Apple packaging requirement), and the
  checksum is computed with `swift package compute-checksum <zip>` (the same
  SHA-256 the workflow computes with `sha256sum`). A generated `Package.swift`
  on the `swift-package` branch of this repository declares
  `binaryTarget(name:url:checksum:)` with the release-asset URL and that
  checksum (the release-URL manifest is what MapLibre's iOS distribution
  packages do, and Ferrostar's release notes record the same SPM shape).
  `scripts/generate-swift-package-manifest.sh` emits the manifest from the
  version, URL and checksum; the release workflow runs it after the draft
  release uploads its assets, then force-pushes a single commit carrying the
  manifest, the generated Swift binding sources
  (`Sources/RusticMapsSDK/rustic_maps.swift`, captured by the ios job),
  the `MapNativeCoreUI` wrapper sources and the license/notices files. A
  `workflow_dispatch` rehearsal skips the push. Consumers add the package as
  `.package(url: "https://github.com/angoratek/rustic-maps-sdk.git",
  branch: "swift-package")`; the branch is rebuilt per release, so a
  deployment that must not follow a moving branch pins the branch commit with
  SPM's `revision:` requirement.
  The rename to `RusticMapsSDK` applies to this manifest's `products` block.
- Signing decision. The archive stays unsigned (ad-hoc): a static library is
  not run-time code-signable and its integrity is pinned by the SPM
  checksum and the release-side signature instead. If the framework later
  moves to a dynamic slice (the size option below), a codesigning pass with
  the team identity enters the release job — that is the only point where
  signing identity matters.
- The dual license files and the third-party notices are copied **inside**
  the `.xcframework` directory before zipping (`scripts/build-ios.sh`), not
  beside it: SPM requires the binaryTarget archive to hold exactly one
  top-level item, and it reads the slices through the XCFramework's
  `Info.plist` while ignoring the extra files.
- The privacy manifest `platform/ios/PrivacyInfo.xcprivacy` declares the
  disk-space required-reason API (`NSPrivacyAccessedAPICategoryDiskSpace`,
  reason `E174.1`) that the Rust tile cache uses to decide when to evict.
  `scripts/build-ios.sh` installs it into the generated `RusticMapsSDK`
  target, whose `Package.swift` resource bundle ships it, and copies it into
  the XCFramework directory so the published archive carries the declaration
  as well. The reference application carries its own manifest for the
  `UserDefaults` record it writes.
- Release asset name `rustic-maps-sdk-ios-vX.Y.Z.xcframework.zip`, the
  `.sha256` sidecar row, and the aggregated manifest.

Size reality (measured 2026-10-29). Each static slice is 217 MiB, and
`strip -x` on a copy brings a slice to 207 MiB, so debug information is
roughly 10 MiB of it and the rest is the static archive's full object set —
a `staticlib` carries every compiled object of the whole dependency tree;
unused code is discarded later, at the consumer's final link (dead-code
stripping). The 10 MiB PRD gate is written for per-architecture payload and
cannot be read off a static archive's file size. The dynamic-library option
(discussed in the script's own header and by MapLibre, which publishes
dynamic XCFrameworks) is what makes the gate directly measurable; it costs
the framework-embedding and codesigning step. This is a genuine decision and
sits in section 6.

An additional gap worth recording: the two current slices cover only arm64.
Consumers who still run Intel Macs cannot use the simulator slice. Whether
an `x86_64-apple-ios-sim` slice joins the release is part of the same
question.

### 2.3 React Native — the TurboModule package

What exists today. `scripts/build-rn.sh` runs `ubrn generate jsi bindings`
over the frozen UDL (TypeScript + JSI C++) and `ubrn generate jsi
turbo-module` (the TurboModule glue: `src/NativeMapsSdkRn.ts`, `src/index.ts`,
`cpp/`), re-applies the `libandroid.so` CMake link line idempotently, builds
the Rust static archives for the AAR's three ABIs into the TurboModule's
`android/src/main/jniLibs`, and typechecks with `tsc --noEmit`. The
generated `MapsSdkRn.podspec` publishes `ios/` and `cpp/` source files and
declares the `uniffi-bindgen-react-native` version dependency (0.31.0-6).
The pod's own Swift sources are type-checked by
`scripts/check-rn-pod-swift.sh` against the generated bindings and the
vendored FFI module: the pod target itself cannot be compiled on Xcode 26
because the `React-Core-prebuilt` explicit-module scan fails before those
sources are reached, so the build gate catches a Swift error there instead.
The TurboModule surface itself is generated source (compiled by the app
against its own React Native version); the Rust side is the prebuilt static
archive the generated CMake links and the XCFramework the pod vendors, which
`build-rn.sh` produces and the release tarball carries (below).

What the release adds: the npm identity and the packed tarball.

- `package.json` renames to `@angoratek/rustic-maps-sdk`, drops
  `"private": true`, and adds the release fields: a `files` allow-list, the
  `react` / `react-native` peer dependencies, and the license expression.
- The `files` allow-list is the point of this section today.
  `platform/rn/.gitignore` history shows how easy it is to over-ignore or
  over-include generated output: an un-anchored `android/` / `src/` pattern
  silently un-versioned the app shell until commit `e67143d` restored
  anchored patterns with an explanatory comment. `files` is an allow-list,
  so the failure mode inverts and an over-inclusive default cannot recur;
  it names exactly the TurboModule surface the podspec and CMake consume:

  ```json
  "files": [
    "src", "cpp", "android", "android/src/main/jniLibs", "ios",
    "AngoratekRusticMapsSdkFramework.xcframework", "MapsSdkRn.podspec",
    "tsconfig.json", "LICENSE-MIT", "LICENSE-APACHE",
    "THIRD_PARTY_NOTICES.md"
  ]
  ```

  `npm pack` always adds `package.json`, `README`, and `LICENSE`
  regardless of the field, and always excludes `node_modules`, lock files,
  `.git`, and log files. The pack step regenerates the glue from the frozen
  UDL, so it is present at pack time (it is gitignored output, per the tree's
  own comment); the pack step then asserts the expected file set
  before archiving — a list equality test, the same fidelity habit the
  fixture guards use. The native-payload entries name the compiled libraries
  every release carries; the list equality test includes them by discovery.
- **Native payload decision (recorded 2026-10-30, implementing the
  2026-10-29 owner ruling).** The tarball ships the AAR's three Android ABIs
  (`arm64-v8a`, `armeabi-v7a`, `x86_64`) as
  `android/src/main/jniLibs/<abi>/librustic_maps.a`, plus the arm64
  device/simulator `AngoratekRusticMapsSdkFramework.xcframework` ubrn
  builds. The static archives use the same ABI table
  `scripts/build-android.sh` cross-builds (`SUPPORTED_ANDROID_ABIS`), so the
  RN package and the AAR agree on the Android architecture set; the RN app
  shell still builds arm64-v8a only.
  `scripts/package-rn-npm.sh --with-native-payload` packs the payload, and
  `scripts/assemble-release-artifacts.sh` stages it from the platform builds
  rather than rebuilding: the static archives the Android release build emits
  beside each ABI's cdylib, and the `rustic_mapsFFI.xcframework` the iOS build
  packages (renamed to the framework the pod declares, headers included). The
  package therefore carries the same binaries the AAR and XCFramework zip
  ship. Producing the payload needs the Android NDK and Xcode; the consumer
  does not need Rust. The
  package is staged into a fresh temporary directory before `npm pack`, so a
  leftover `jniLibs`/`.xcframework` from an earlier local build can never leak
  in; `--dry-run` prints the exact member list without writing a release
  asset.
  Two Android files the generated `build.gradle` names unconditionally
  (`proguard-rules.pro` and `src/main/AndroidManifestNew.xml`) are versioned
  package sources, so a fresh checkout can assemble the Android library
  without a hand-created file.
- **Release-assembly gate.** After packing,
  `scripts/assemble-release-artifacts.sh` unpacks the tarball and reads the
  manifests a consumer gets:
  the podspec's `vendored_frameworks` (expanded to each XCFramework slice
  binary through its `Info.plist`), the CMake `IMPORTED_LOCATION` archive for
  every ABI in the Gradle `ndk { abiFilters }` list, the CMake-compiled
  sources and include directories, and every file `build.gradle` names. It
  checks each one exists and is non-empty, asserts the tarball's
  `package.json` version equals the workspace version, and asserts the
  `files` allow-list covers every named artifact. A miss fails the assembly
  with the missing path and the manifest that named it.
- The dual license files and the third-party notices are staged into every
  RN tarball (`LICENSE-MIT`, `LICENSE-APACHE`,
  `THIRD_PARTY_NOTICES.md`) from their single sources, never copied by hand.
- Asset name `rustic-maps-sdk-rn-vX.Y.Z.tgz` (the checked-in copy of the
  packed tarball under its released name; the tarball's internal name is
  the npm identity), a `.sha256` sidecar, the manifest row, and the
  signature bundle. The current package is 249 MiB and unpacks to 786 MiB
  across 55 files; the vendored XCFramework carries its FFI headers and
  module map, so the generated Swift bindings resolve.

Install command for an application project, no registry involved:

```bash
npm install ./rustic-maps-sdk-rn-vX.Y.Z.tgz
```

### 2.4 WebAssembly — the wasm package and the served module

What exists today. `scripts/generate-wasm-bindings.sh` builds
`rustic-maps-native-core` release with `wasm-bindgen-cli` locked at `0.2.129`
(the version-locked pair is a recorded requirement: the CLI must match the
crate), generates the contract TypeScript and stages the wasm-bindgen glue and
the module; `scripts/build-wasm2-package.sh` then compiles the ESM set and its
`.d.ts` declarations and emits `platform/web/generated` — `index.js`,
`rustic_maps.js`, `rustic_maps-ffi.js`, `rustic_maps_bg.js`, `rustic_maps.wasm`
(6.2 MiB) and the declarations. The packaging script passes the module through
`wasm-opt -Oz`; the release job calls it with `--require-optimize` and installs
binaryen first, so a published module is never the unoptimized emit.
The dev server `platform/web/serve.mjs` maps four same-origin route
families onto repo files with allow-listed names and traversal guards:
`/tiles/{zoom}_{tileX}_{tileY}[@2x].png` (the recorded tile fixtures),
`/golden/{name}.png` (exactly six committed goldens),
`/reference-scenes.json` (the shared demo artifact), and
`/offline-graph/{name}.rmog` (the compiled offline-routing fixture). It also
serves the generated package's player runtime under
`/generated-runtime/@ubjs/...`, which the page's import map names.
`index.html` renders the attribution line the serving provider's descriptor
declares, and carries a documented degradation path when browser WebGPU is
unavailable.

What the release adds.

`scripts/check-wasm2-package-freshness.mjs` guards every consumer of the
package: it records a SHA-256 of the git-tracked generation inputs beside the
emit, recomputes it before the Playwright suite and before `npm pack`, and
refuses a package whose sources changed, whose module exceeds the 8 MiB budget,
or whose npm manifest version differs from the workspace version. A stale
`generated/` is therefore never tested or published.

- `@angoratek/rustic-maps-sdk-web`: an npm package whose content is the
  `generated/` emit (the ESM set, the wasm module, the generated `.d.ts`
  declarations), the hand-written `types/browser_canvas.d.ts` published as
  `rustic_maps_bg.d.ts` beside the glue, a README with the browser-support
  statement below, the dual license files, and the notices file. The
  `@ubjs/core` and `@ubjs/wasm` player runtime packages are its runtime
  dependencies. The consumer serves the package's directory
  as a static path root (the same file layout the playground serves), or
  simply points at the demo. Asset name
  `rustic-maps-sdk-web-vX.Y.Z.tgz`, with the checksum and signature
  treatment of the other tarballs. It is the **only** WebAssembly asset: the
  same tarball serves a non-npm consumer, who extracts it and serves the
  `package/` directory. `npm pack` writes deterministic bytes, and the
  packaging script asserts the tarball's exact member set.
- The browser-support statement to publish everywhere the package appears:

  > WebGPU is required. Chromium-based browsers have supported it flag-free
  > since 113 (2023); Safari ships it from version 26; Firefox ships it
  > since 141, with desktop first. It is a secure-context API, so the page
  > must be served over HTTPS (or localhost) and `navigator.gpu` is the
  > feature test to use — no user-agent sniffing.

  This statement is the honest form of the degradation path the harness
  already implements; MDN's WebGPU page carries the live compatibility
  table, and the site links to it rather than restating versions.

## 3. Release workflow design

The workflow is one file, `.github/workflows/release.yml`, with the same
publish-fidelity habits the docs pipeline has: gates run on the exact tree
being packaged, provenance is stamped, and the publish action is a separate,
deliberate step.

### 3.1 Flow

```
push tag v0.4.0
  │
  ├─ job: gates            (the standard gate set on this exact ref)
  ├─ job: plan             (input hash per platform → run or reuse)
  ├─ job: android   →  rustic-maps-sdk-android-v0.4.0.aar   (skipped when reusable)
  ├─ job: ios       →  rustic-maps-sdk-ios-v0.4.0.xcframework.zip
  ├─ job: rn        →  rustic-maps-sdk-rn-v0.4.0.tgz
  ├─ job: wasm      →  rustic-maps-sdk-web-v0.4.0.tgz   (+ /demo bundle)
  ├─ job: restore   (republishes reusable artifacts, bytes re-hashed)
  │
  └─ job: assemble
        verify each build job's artifact + expected file set
        regenerate docs/THIRD_PARTY_NOTICES.md from the tagged Cargo.lock
        generate the install-snippet README asset (version + repository only)
        sha256sum -b every asset  →  per-asset .sha256 + SHA256SUMS.txt
        cosign sign-blob each asset  →  .sigstore.json bundles
        cosign sign-blob SHA256SUMS.txt
        gh release create --draft --title --notes  (assets + notices + README)
        generate Package.swift from the uploaded zip URL + its SHA-256 and
          force-push swift-package (manifest + generated Sources + licenses)
```

Publishing the draft (flipping `--draft=false` or "Publish" on the release
page) is the owner's step, performed after eyeballing the drafted assets —
the same two-step the docs pipeline uses (`stage … / publish with
`--gates-green=<commit>`).

### 3.2 Design points

- **Gates run inside the release workflow, on the tagged commit.** The
  pre-commit verification gate of CLAUDE.md (`cargo fmt --check`, `cargo
  clippy --workspace --all-targets -- -D warnings`, the wasm-closure lib
  clippy, `cargo test --workspace`, the Playwright suite) runs as the first
  job; every packaging job `needs:` it. A prior green main-branch run is not
  trusted as a proxy — the tree being packaged is exactly the tagged tree,
  which is the same discipline provenance.txt commits the docs pipeline to.
- **Dry-run mode.** The workflow also accepts `workflow_dispatch`: the jobs
  run in full (including the install-snippet README, the checksums and the
  signatures), but the release API call and the `swift-package` branch push
  are skipped and the `dist/` tree is uploaded as a workflow artifact
  instead. The first tag-driven release is rehearsed this way.
- **Permissions.** `contents: write` (draft release, artifacts,
  `swift-package` branch) and `permissions: id-token: write` for keyless
  cosign signing. The GitHub Actions OIDC token binds every signature bundle
  to the workflow repository and tag ref, so verification can pin
  `--certificate-identity-regexp
  "https://github.com/angoratek/rustic-maps-sdk/.github/workflows/release.yml@refs/tags/vX.Y.Z"`
  and the GitHub issuer URL; the `--bundle` output gives the verifier both
  the signature and the certificate in one file, and `--yes` is required in
  non-interactive CI.
- **Why keyless cosign over GPG-in-a-secret.** Keyless signing needs no
  long-lived secret in the repository: the runner's OIDC identity is the
  signing material, and the verification command pins the releasing
  workflow and tag. A GPG secret key would need rotation handling and
  expiry management for a benefit keyless does not have. This is the shape
  Kubernetes and others verify with; the verification command appears in
  the release notes, so a consumer can check every asset.
- **Notices are regenerated at release time** by the deterministic
  generator (`scripts/generate-third-party-notices`),
  byte-compared against the committed `docs/THIRD_PARTY_NOTICES.md` as a
  fail-closed check (a mismatch aborts the release), and shipped as
  `rustic-maps-sdk-notices-vX.Y.Z.md` — one notice file for every package.
  On the workflow runner, the generator's `cargo metadata --offline`
  resolves against the registry index; the docs-publication workflow's
  priming step (`cargo metadata --format-version 1` first, as a registry
  warm-up) is reused verbatim.
- **API-reference links in the release notes** point at the rustdoc tree
  (section 4) and the verbatim UDL page — both live on the site and both
  describe the same source commit the tag names; the release notes state
  that coupling.
- **Retention.** Release assets are permanent in GitHub Releases (limits:
  up to 1 000 assets per release, 2 GiB each — this project is far under
  both). Workflow-run artifacts (dry-run `dist/` trees) keep the default
  90-day retention; a dry run's `dist/` is never promoted to a release.
- **The release notes are drafted from `CHANGELOG.md`** — the section whose
  heading matches the tag, as MapLibre's release process does with its
  per-platform changelog headings, and as the curated public changelog
  chapter already does. The draft is opened with `--generate-notes` as the
  base and the changelog section replaces the body at draft time; the owner
  edits the draft before publishing.

### 3.3 Install-snippet README asset

`scripts/generate-release-readme.sh` assembles
`rustic-maps-sdk-vX.Y.Z-README.md` from the version and repository only — no
timestamp, host or environment value — so its bytes are deterministic for a
given release. It names every asset and carries the copy-paste install
snippet per platform (Android `flatDir` AAR, iOS Swift Package Manager
`swift-package` branch, React Native and WebAssembly npm tarballs), the
checksum/Sigstore verification commands, and the license/notices pointers.
The workflow generates it before the checksum and signing pass, so it is
covered by `SHA256SUMS.txt`, by its `.sha256` sidecar and by its
`.sigstore.json` bundle; the changelog-derived release notes list it beside
the packages.

### 3.4 Input-gated rebuilds

A release run can be repeated: the first attempt may fail in the assembly or
publish step after the platform packages are already built. Rebuilding
unchanged packages wastes runner minutes, so the workflow plans every
platform before it builds any.

The `plan` job computes one SHA-256 input hash per platform from the tracked
files at the tagged commit. A platform's inputs are its own sources, the Rust
workspace closure it compiles (`Cargo.toml`, `Cargo.lock`, `crates/`), the
build and packaging scripts it runs, the release workflow that pins its
toolchain, the version-bearing manifests, and the license and notice files its
package carries. The React Native hash also includes the Android and iOS
hashes because its tarball ships their compiled payload, and the iOS hash
includes the shared demo-scene and provider-registry JSON that
`scripts/build-ios.sh` copies into the Swift package. Because the version
lines live inside those manifests, a version bump always changes the hash and
forces a rebuild.

Each platform build job writes `release-artifact-<platform>.json` beside its
artifact:

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

A platform build job is skipped only when all three conditions hold:

1. a completed run for the same revision recorded an input hash for that
   platform;
2. that hash equals the hash of the current tree;
3. the record names the artifact and its SHA-256, and the prior run still
   holds every artifact the platform job uploads, including the React Native
   payload companions.

Anything else runs the build. The `restore` job downloads a reusable artifact
from the prior run, re-checks its bytes against the record, and republishes it
to the assemble job. The assemble job repeats the input-hash and byte-hash
check and still asserts the exact release asset set, so a skipped job can
never leave the release incomplete or carry bytes from a different tree.

The same decision is available before a release:

```bash
node scripts/release-plan.mjs --base "$(git describe --tags --abbrev=0)"
node scripts/release-plan.mjs --base v0.3.0 --head HEAD --json
```

The table prints each platform's input hash, the number of changed input
files, and whether the job would run or reuse the base release's artifact.
Both revisions are commits: the plan reads tracked files at a revision, so run
it from a clean checkout of the release commit. `--prior-directory` points the
command at a prior run's records, which is the form the workflow uses. A
missing record never skips a build.

## 4. Docs-site product requirements

The docs site (angoratek.github.io/rustic-maps-sdk-docs) grows three
sections: the demo, the API reference surface, and the guides. All of them
ride the existing pipeline: `scripts/build-docs-site.sh` stages, renders,
scrubs, stamps `provenance.txt`, and publishes with `--gates-green=<commit>` from a clean tree to gh-pages; the CI `docs-publication` job keeps verifying the
staging itself.

### 4.1 The live wasm demo at `/demo`

The demo page is the browser harness (`platform/web`) served from the docs
site, static, keyless, and gated.

Requirements:

1. **Static-only deploy.** The GitHub Pages origin serves files; it cannot
   run Node. The demo therefore ships a directory whose layout answers the
   same URLs `serve.mjs` maps today — the URL routes are kept verbatim and
   the path mapping becomes a copy plan. `main.mjs`, `overlay_scenes.mjs`,
   and `map_controls.mjs` need no changes.
2. **Keyless.** No API key of any kind. The demo renders recorded fixture
   tiles (already the behavior: the harness fetches same-origin `png`
   tiles and the `.rmog` archive, and there is no key usage anywhere in
   `index.html`, `main.mjs`, or `overlay_scenes.mjs`). The standing rule:
   the published demo ships keyless.
3. **Attribution rendered on the demo page itself** — the line the serving
   provider's descriptor declares stays visible, and the vector base's
   style attribution joins it; both come from the shared registry rather than
   a literal in `index.html`.
4. **WebGPU banner.** The page states its requirement in one place: if
   `navigator.gpu` is absent, the documented degradation message names the
   missing capability and links the MDN compatibility table. The harness
   text needs no change; the demo ships the same message.
5. **Provenance.** The demo directory carries the same `provenance.txt`
   stamp (source commit, dirty-tree flag, generator, content license, map
   attribution) and the publish appends the `gates-green=<commit>` assertion — the
   demo describes exactly one commit.
6. **Gates before publish.** The demo's producing pipeline is the wasm e2e
   suite: `--gates-green=<commit>` here means the full Playwright suite (45/45
   today) passed against this exact bundle. The bundle-step output is
   additionally byte-equality-asserted against `platform/web`'s own files
   where they are copied 1:1 (`pkg/`, the three `.mjs` files), an
   anti-drift guard in the same family as the UDL render gates.
7. **Scrub-clean.** The fail-closed scrub runs over the whole staged site
   including `/demo` (item: denied keys, denied paths — `PLAN.md`,
   `PRD.md`, `CLAUDE.md`, `.env`, `local.properties`, `Local.xcconfig`,
   denied content: `dev.mapssdk`, the loopback address, and
   `com.angoratek` which the ruling-2 amendment allowlists in its
   package-name form). If the demo pages name the Maven group or the npm
   scope, the amended allowlist covers exactly those identifiers.
8. **Size.** The staged `/demo` tree stays under ~25 MB before Pages'
   serving (measure at gate time; the copy plan below measures 19 MB with
   the full tile set, and a tile-subset variant is the lever if it grows).

Bundle plan (URL path → source → measured size, 2026-10-29):

| Served path | Source | Size |
|---|---|---|
| `/demo/` | `platform/web/index.html` + `main.mjs`, `map_controls.mjs`, `overlay_scenes.mjs` (copied 1:1) | small |
| `/demo/generated/*` | `platform/web/generated/*` (the generated ESM set, declarations and `rustic_maps.wasm`) | 6.3 MB |
| `/demo/generated-runtime/@ubjs/*` | the page's player runtime from `platform/web/node_modules/@ubjs` | 0.9 MB |
| `/demo/tiles/*` | `crates/rustic-maps-rendering/fixtures/render/tiles/*` (219 files) | 11 MB |
| `/demo/reference-scenes.json` | `platform/reference-scenes.json` | 8 KB |
| `/demo/offline-graph/routing_grid.rmog` | `crates/rustic-maps-offline-graph-compiler/fixtures/rmog/routing_grid.rmog` | 552 B |
| `/demo/golden/*` | — not shipped: the golden images feed visual tests, not the demo | (2 MB skipped) |

The `/golden/` route is a test-only route family; the demo bundles none of
it. The demo needs one extra input beyond the copy: the demo
`index.html` is a curated copy whose edits are the footer (version stamp
from the wasm `get_sdk_compiled_version` probe and a link to the release
that shipped it) — and everything else stays byte-equal to the harness
page, asserted by the bundle gate.

### 4.2 API reference

Decision: **rustdoc stays the published reference; Kotlin and Swift
reference generation is deferred**, with a revisiting point recorded.

- Rustdoc is already published at `site/api/` with the byte-fidelity
  assertion that the entry crate exists (the `api-reference` chapter).
  It stays, unchanged.
- Kotlin (Dokka) and Swift (DocC) would document generated bindings. The
  UDL carries 236 `///` doc-comment lines, and UniFFI propagates them into
  the generated Kotlin and Swift sources — so the generated code is
  documented. But the generated reference adds a third copy of the same
  information the rustdoc and the verbatim UDL chapter already carry, at
  the cost of two new toolchains in the release pipeline (Dokka output into
  the release; DocC archive into the site) and per-language curation of
  the generated-module pages. Effort note: roughly a small isolated CI job
  per language once the contract stops growing quickly.
- The revisit trigger: when the contract's post-M5 era gives the binding
  sources their own examples (the quickstart guides of 4.3), a Dokka HTML
  directory joins the release assets and a DocC archive joins the site.
  Until then the platform pages of the guides link to the verbatim UDL
  chapter, which is the actual generated surface.
- Honest statement in the release notes: the API reference is the rustdoc
  of the workspace plus the contract chapter; the Kotlin and Swift mirrors
  are documented through the contract.

### 4.3 The guides

The site's Guides group walks the product end to end: the Rust surface,
the first map, search and routing, turn-by-turn navigation, offline maps
and device positioning — each chapter tabbed for the platforms that ship
the feature and sourced from what the reference applications actually do
(the Espresso / XCUITest / Playwright walkthroughs of
`docs/ReferenceAppUX.md`).

- Where it lives: `site/book-src/first-map.md`,
  `search-and-routing.md`, `navigation-simulation.md`, `offline-maps.md`
  and `positioning.md`, with `rust.md` for the Rust surface; the
  `SUMMARY.md` "Guides" group lists them, and `build-docs-site.sh`
  composes the book.
- Source of truth: the snippets compile against the artifact names of
  section 2 (the install command, the SPM manifest, the npm command) and
  the method names of the frozen contract. The publication-hygiene rule of
  the style guide applies: versions and file paths are copied from source
  files, not from memory.

### 4.4 Licenses, notices, and attribution

- The dual license ships as `LICENSE-MIT` and `LICENSE-APACHE` verbatim in
  every package **and** as release assets:
  - the AAR carries them under `assets/licenses/` (staged by
    `scripts/build-android.sh` into an assets source the module's
    `build.gradle.kts` declares, so `assembleRelease` packages them);
  - the XCFramework zip carries them inside the `.xcframework` directory
    (the single-top-level-item constraint of section 2.2);
  - both npm tarballs carry them (the wasm script stages them explicitly;
    the RN script stages them together with the notices);
  - the plain wasm archive carries them at its root.
  The release assets `LICENSE-MIT` and `LICENSE-APACHE` remain the
  standalone copies.
- The third-party notices ship in every package (the
  `rustic-maps-sdk-notices-vX.Y.Z.md` asset, the section-3 regeneration
  step, plus the identical `THIRD_PARTY_NOTICES.md` inside the AAR, both
  npm tarballs, the plain wasm archive and the XCFramework) and remain a
  site chapter.
- The notices generator's `prior art & inspiration` section, the
  cargo-deny license gate, and the map-data attribution section are
  package-relevant and ride along unchanged.
- The demo page's attribution requirement is item 3 of section 4.1.

## 5. Versioning and release policy

- **One version line.** The workspace version in `Cargo.toml`
  (`version = 0.1.0`) is the single source of truth. The release stamps it
  into: the Maven publication version, the npm package versions, the SPM
  manifest and tag, and the wasm package. A tiny check step in the release
  job refuses the build when any of those files disagrees with the
  workspace version (the same "one source, asserted" shape as the UDL
  byte-equality gates).
- **Semantic versioning, 0.x until 1.0.** Breaking contract changes do not
  exist (the contract is addition-only), but packaging breaks can (an
  artifact set change, a renamed target). While the version stays 0.x, a
  breaking packaging change bumps the minor.
- **Tag discipline.** `vX.Y.Z` tags are the release trigger; a pre-release
  tag (`vX.Y.Z-rc.N`) runs the same workflow with a pre-release marker.
  Tags are never moved or re-pushed; a broken release is superseded by a
  new tag, and the failed release asset stays (its checksums keep the
  history honest). The tag version must match the workspace version, or
  the gates fail — the same rule Ferrostar records for SPM.
- **CHANGELOG discipline.** Every meaningful change lands in
  `CHANGELOG.md` (already the tree's practice). The release job uses the
  section whose heading matches the tag; release-notes curation happens at
  draft time (section 3), and the site's curated public changelog chapter
  is refreshed in the same docs commit. Unreleased sections stay under
  `[Unreleased]`.
- **Reproducibility expectation, stated honestly.** The release jobs are
  deterministic per commit for the notices, the checksums, and the
  bindings codegen; the compiler caches and CI runners are not — byte-level
  build reproducibility of the native binaries is not claimed by this
  milestone and is not verified. The artifact manifest is what consumers
  verify.

### 5.1 CI minutes budget

GitHub Actions minutes are a metered resource for this repository, so CI is
treated as a verification mirror, never the primary loop:

- **Local gates are the complete path.** The pre-push hook
  (`scripts/git-hooks/pre-push`) runs fmt, workspace clippy and the workspace
  tests, and `scripts/build-docs-site.sh --gates-green=<commit>` requires that
  exact commit's gate set before publishing. A push that skipped the hook
  still has to pass the same commands before a release.
- **Every workflow cancels a superseded run** on the same ref
  (`concurrency: … cancel-in-progress: true`) and carries a path filter, so a
  docs-only or platform-only change never compiles the workspace three times.
- **The expensive suites are event-scoped.** The Android and iOS platform
  builds keep the pull-request gate and otherwise run on the nightly schedule
  and on demand; the wasm e2e suite caches the Playwright browser bundle and
  the pinned wasm-bindgen CLI.
- **One compile per gate set.** `format-and-lint` runs formatting, clippy, the
  nextest suite, the rustdoc tests and the license/advisory check in one job
  so clippy's and nextest's compilations share one warm target directory
  (the former separate `test-rust-core` workflow paid for a cold compile).
- **A release re-run reuses unchanged packages.** The plan job compares each
  platform's input hash with the record beside the prior run's artifact and
  skips that build when they match. The restored bytes are re-hashed, and the
  assemble job still asserts the exact asset set (section 3.5).
- **Publication is never automatic.** CI is verification-only; the docs
  publish and the release draft are deliberate local or dispatched actions.

The owner configures GitHub's own spending limit and notification alert for
the account; this document records the policy the workflows implement, not a
billing setting the repository can enforce.

## 6. Open owner questions

Kept to the ones that genuinely change what gets built and shipped:

1. **iOS static slice vs the 10 MiB gate.** Keep the static archive (the
   current two-slice XCFramework; consumers dead-strip at their link) or
   switch the iOS artifact to a dynamic `.xcframework` slice (per MapLibre)
   with the framework-embedding and codesigning step it costs. The measured
   basis is section 2.2 (217 MiB per slice; 207 MiB stripped).
2. **Wider simulators.** Ship an `x86_64-apple-ios-sim` slice for Intel Mac
   consumers? Today's release serves arm64 only.
3. ~~**Android architectures.**~~ **Settled in implementation (2026-11-01, owner ruling 2026-10-29):**
   the RN native payload ships the AAR's three ABIs (`arm64-v8a`,
   `armeabi-v7a`, `x86_64`) plus the arm64 iOS framework (section 2.3).
   The Android release build emits that three-ABI static-archive table, and
   `scripts/assemble-release-artifacts.sh` packs it into every RN tarball and
   fails when a manifest-named artifact is absent. Producing it needs the NDK
   and Xcode.
4. **Demo tile set.** Bundle the full 11 MB recorded tile set (the bundle
   plan above) or a demo-trimmed subset (smaller Pages payload, less
   variety)? A subset keeps the demo at ~8 MB.
