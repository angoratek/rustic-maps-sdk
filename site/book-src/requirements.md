# Requirements and distributions

The SDK builds from source into the archives you install into your project.
This page lists the minimum platform versions, the artifact each platform
installs, and what each artifact weighs. [Downloads](downloads.md) explains how
to build each artifact.

## Android

| Requirement | Value |
|---|---|
| Minimum API level | 24 |
| Compile SDK | 36 |
| Android Gradle Plugin | 9.0.0 (Gradle wrapper 9.5.1) |
| Kotlin binding runtime | `net.java.dev.jna:jna:5.18.1@aar` (UniFFI runtime) |
| Coroutines | `org.jetbrains.kotlinx:kotlinx-coroutines-core:1.10.2` |
| Compose host | `androidx.compose.ui:ui` from the application's Compose BOM, plus `androidx.lifecycle:lifecycle-runtime-compose:2.9.4`; not required for the `AndroidView` path |

The Android distribution is one AAR:

- `rustic-maps-sdk-android-v0.1.0.aar` — the Kotlin bindings, the Rust
  library for arm64-v8a, armeabi-v7a, and x86_64, and the `MapView` with its
  Compose wrapper in one file. It measures about 16 MiB; its stripped
  libraries measure 12 MiB for arm64-v8a, 8.7 MiB for armeabi-v7a, and
  13 MiB for x86_64.

## iOS

| Requirement | Value |
|---|---|
| Deployment target | iOS 17 or higher |
| Swift tooling | `swift-tools-version: 5.10` (Swift 5 language mode) |
| Architectures | arm64 (device) and arm64 (simulator) |

The iOS package is
`rustic-maps-sdk-ios-v0.1.0.xcframework.zip`: the XCFramework containing the
static library, its headers, and module maps, packaged for a Swift Package
Manager binary target. Each architecture measures 217 MiB, and 207 MiB after
`strip -x`. A static archive carries every compiled object, and your final
link discards unused code, so these archive sizes are not the size your
application adds.

## React Native

The React Native build produces `rustic-maps-sdk-rn-v0.1.0.tgz`, the
TurboModule package (`@angoratek/rustic-maps-sdk`). It carries the generated
TypeScript surface, the JSI C++ glue, the podspec and CMake wiring, and the
compiled Rust libraries:

- `android/src/main/jniLibs/<abi>/librustic_maps.a` for arm64-v8a,
  armeabi-v7a, and x86_64, linked by the module's CMake build;
- `AngoratekRusticMapsSdkFramework.xcframework` for arm64 device and arm64
  simulator, vendored by the pod.

The tarball downloads 249 MiB and unpacks to 786 MiB; the static archives
account for almost all of it, and your final link discards unused code.

| Requirement | Value |
|---|---|
| React Native | 0.88 or later |
| React | 19 or later |
| Android build | Android Studio with the Android SDK and NDK; set `reactNativeArchitectures` to arm64-v8a, armeabi-v7a and x86_64 |
| iOS build | Xcode and CocoaPods; run `pod install` after installing the package |
| Rust toolchain | not needed; the package carries the compiled libraries |

For a different React Native or React version, regenerate the glue with a
matching generator version.

## WebAssembly

The WebAssembly build produces one package:

- `rustic-maps-sdk-web-v0.1.0.tgz` — the wasm module, its JavaScript shim,
  both type files, the npm manifest, the licenses and the notices. Install it
  with npm or extract it and serve its `package/` directory.

The package downloads 3.6 MiB, of which the wasm module is 3.4 MiB.

The browser surface renders through WebGPU. Chromium-based browsers support it
flag-free since 113 (2023), Safari from version 26, and Firefox since 141 with
desktop first. WebGPU is a secure-context API, so serve your page over HTTPS
or from localhost, and test for `navigator.gpu` rather than sniffing the
user agent. MDN's WebGPU page carries the live compatibility table.

## Notices

`rustic-maps-sdk-notices-v0.1.0.md` carries the third-party dependency notices
that accompany your own distribution.

## Versions policy

Pin exact versions. Release archives are immutable and carry a version in the
file name, for example `rustic-maps-sdk-android-v0.1.0.aar`; keep the version
you validated against and upgrade deliberately. One Cargo workspace version is
the single source of truth, and `getSdkCompiledVersion` returns the loaded
core's version across the FFI. The FFI contract changes addition-only, so a
minor upgrade keeps your compiled bindings valid. Every release carries a
per-asset SHA-256 checksum, an aggregated manifest, and Sigstore bundles; the
[versions and support](versions-and-support.md) page covers the numbering,
release tracks and verification.

The sizes above are measured from locally built artifacts.
