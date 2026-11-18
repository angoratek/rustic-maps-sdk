# Your first map

Install the SDK, register a map provider, and draw your first map. Every tab
uses the same demo camera: Capitol Park, Sacramento at 38.5766, -121.4944,
zoom 12, north-up and top-down. Every call compiles against the [FFI
contract](udl-contract.md); [Map concepts](map-concepts.md) covers the model.

## Before you start

| Platform | What you need |
|---|---|
| Android | Android Studio with the SDK 36 toolchain, a project with `minSdk = 24`, and the AAR from the SDK build script in `app/libs/`. |
| iOS | Xcode with Swift 5.10 (`swift-tools-version: 5.10`), an iOS 17 deployment target, and the package from the SDK build script. |
| React Native | Node.js and npm, a bare-workflow React Native 0.88 or later application on React 19 or later, Android Studio with the SDK and NDK or Xcode and CocoaPods, and the tarball from the SDK build script. |
| WebAssembly | A Chromium-based browser with WebGPU, flag-free since Chromium 113, Safari 26 and Firefox 141 on desktop, and the package from the SDK build script. |

## Choose a provider

Register one provider and apply its credential at **runtime** from a settings
screen, secure storage, or a launch environment for automated tests. No Gradle
field, `Info.plist` or `.xcconfig` entry carries one. Registration throws the
provider's message when the key is refused. A keyless base map still draws;
search, routing and traffic answer once a provider serving those contracts has
a credential. [Provider capabilities](provider-capabilities-android.md) lists
each provider, what it serves and its credential.

## Camera, gesture and overlay rules

- `setCamera` takes a pose: latitude, longitude, zoom, bearing and pitch.
  `animateCamera` eases to a pose; `fitBounds` frames a box inside a viewport
  with edge insets.
- `panByScreenPixels` keeps the grabbed ground point as the center;
  `zoomByLevelsAroundScreenPoint` and `rotateByDegreesAroundScreenPoint` apply
  pinch and twist deltas with the same focal-point correction. `MapView` runs
  its own gestures; call these members from your own control or a test.
- `setOverlays` replaces the drawn list; `addOverlay`, `updateOverlay` and
  `removeOverlay` address one entry by the identifier `addOverlay` returns. An
  overlay is a kind, coordinates, a color and a pixel width; kinds are
  `RoutePolyline`, `FilledPolygon`, `PointMarkers`, `PointPins` and
  `TrafficFlow`. Native tabs spell the color `rgbaColor`, the browser `color`.

<div class="platform-tabs">
<div class="platform-tab-titles" role="tablist">
<button type="button" data-platform-tab="android">Android</button>
<button type="button" data-platform-tab="ios">iOS</button>
<button type="button" data-platform-tab="react-native">React Native</button>
<button type="button" data-platform-tab="wasm">WebAssembly</button>
</div>

<div class="platform-tab-panel" data-platform-panel="android">

### Install the artifact

In your application module's `build.gradle.kts`:

```kotlin
repositories {
    flatDir { dirs("libs") }
}

dependencies {
    implementation(files("libs/rustic-maps-sdk-android-v0.1.0.aar"))
    implementation("net.java.dev.jna:jna:5.18.1@aar")   // the UniFFI runtime
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.10.2")
}
```

Keep each file name's version exact. The Compose entry point is optional; the
AAR declares its Compose dependencies `compileOnly`, and a Compose host adds the
Compose BOM, `androidx.compose.ui:ui` and
`androidx.lifecycle:lifecycle-runtime-compose:2.9.4` from its own module.

### Create the SDK and show the map

```kotlin
import uniffi.rustic_maps.MapSdk

val mapSdk = MapSdk()
// The key belongs to the provider you registered; see "Choosing a provider".
mapSdk.configureProviderSet("mapquest", providerApiKey)
```

The package ships a lifecycle-aware `MapView` that owns the surface, frame
pump, resizing, gestures and camera animation:

```kotlin
import com.rusticmaps.sdk.compose.MapView

MapView(
    sdk = mapSdk,
    listener = mapViewListener,
    modifier = Modifier.fillMaxSize(),
)
```

Without Compose, place the plain `SurfaceView` subclass in your layout or host
it with `AndroidView`.

### Open the map

<!-- docs-snippet: begin first-map/open-the-map android -->
```kotlin
val mapCamera = MapCameraPose(
    latitudeDegrees = 38.5766,
    longitudeDegrees = -121.4944,
    zoomLevel = 12.0,
    bearingDegrees = 0.0,
    pitchDegrees = 0.0,
)
mapView.setCamera(mapCamera)
```
<!-- docs-snippet: end first-map/open-the-map android -->

</div>

<div class="platform-tab-panel" data-platform-panel="ios">

### Install the artifact

Add the built package's `RusticMapsSDK` and `MapNativeCoreUI` products; the
manifest pins the checksummed XCFramework as its binary target:

```swift
// Package.swift
// swift-tools-version: 5.10
import PackageDescription

let package = Package(
    name: "MyMapApp",
    dependencies: [
        .package(
            url: "https://github.com/angoratek/rustic-maps-sdk.git",
            branch: "swift-package"
        ),
    ],
    targets: [
        .executableTarget(
            name: "MyMapApp",
            dependencies: [
                .product(name: "RusticMapsSDK", package: "rustic-maps-sdk"),
                .product(name: "MapNativeCoreUI", package: "rustic-maps-sdk"),
            ]
        )
    ]
)
```

### Create the SDK and show the map

```swift
import RusticMapsSDK

let mapSdk = MapSdk()
// The key belongs to the provider you registered; see "Choosing a provider".
try mapSdk.configureProviderSet(providerKey: "mapquest", credential: providerApiKey)
```

The `MapNativeCoreUI` view owns the Metal surface, resizing and gestures; set
`mapView.sdk = mapSdk`. `SwiftUIMapView` wraps it for SwiftUI hosts.

### Open the map

<!-- docs-snippet: begin first-map/open-the-map ios -->
```swift
let mapCamera = MapCameraPose(
    latitudeDegrees: 38.5766,
    longitudeDegrees: -121.4944,
    zoomLevel: 12.0,
    bearingDegrees: 0.0,
    pitchDegrees: 0.0
)
mapView.setCamera(mapCamera)
```
<!-- docs-snippet: end first-map/open-the-map ios -->

</div>

<div class="platform-tab-panel" data-platform-panel="react-native">

### Install the artifact

Install the tarball with `npm install ./rustic-maps-sdk-rn-v0.1.0.tgz`; no
registry is involved. It carries the TurboModule glue and the compiled Rust
libraries for Android (arm64-v8a, armeabi-v7a, x86_64) and iOS (arm64 device
and simulator), so no Rust toolchain is needed. Link it with normal
autolinking. On iOS, run `pod install`. On Android, set
`reactNativeArchitectures=armeabi-v7a,arm64-v8a,x86_64` in
`android/gradle.properties`; the template default adds `x86`, which the
package does not ship.

### Create the SDK and show the map

The package owns one native SDK per process. Create it and apply the
credential; a refused credential resolves with `isApplied: false` and the
verbatim `detailText`, so surface it rather than continuing.

```js
import { MapsSdk } from '@angoratek/rustic-maps-sdk/MapsSdk';

// MapQuest, with a key the user entered in your settings screen. A keyless
// URL-template base map needs no credential at all.
const mapSdk = await MapsSdk.create();
await mapSdk.setProviderCredential('mapquest', providerApiKey);
```

The typed `<MapView>` component owns the surface lifecycle, frame pump,
gestures, camera calls, id-addressed overlays and callbacks. Pass a
`MapViewHandle` ref and the `topBarInsetPixels`, `bottomBarInsetPixels`,
`onMapReady`, `onCameraChanged(camera, isGestureDriven)` and `onError(failure)`
props. The generated contract classes remain available for a custom host view.
`getSdkCompiledVersion()` returns `"0.1.0"` once the Rust core answers through
the TurboModule.

### Open the map

<!-- docs-snippet: begin first-map/open-the-map react-native -->
```ts
const mapCamera = {
  latitudeDegrees: 38.5766,
  longitudeDegrees: -121.4944,
  zoomLevel: 12.0,
  bearingDegrees: 0.0,
  pitchDegrees: 0.0,
};
mapView.setCamera(mapCamera);
```
<!-- docs-snippet: end first-map/open-the-map react-native -->

</div>

<div class="platform-tab-panel" data-platform-panel="wasm">

### Install the artifact

Install with `npm install ./rustic-maps-sdk-web-v0.1.0.tgz` and copy
`node_modules/@angoratek/rustic-maps-sdk-web` to `./web`, or extract the
tarball and serve its `package/` directory over HTTPS or localhost, such as
with `python3 -m http.server 8000`. A page needs a provider key only for live
search, routing and imagery; keyless, it renders recorded tiles and places.

### Start the SDK and show the map on a canvas

One page creates one SDK root, and provider registration is a method on that
root. Supply a `<canvas>` element, here `#map-canvas`; the view claims it,
acquires the WebGPU device and renders same-origin tiles, while the page keeps
pointer handling.

```js
import { uniffiInitAsync, MapSdk, getSdkCompiledVersion } from './web/index.js';
import { BrowserMapView, MapCamera, TileResolution } from './web/rustic_maps_bg.js';

await uniffiInitAsync(new URL('./web/rustic_maps.wasm', import.meta.url));
const mapSdk = new MapSdk();
mapSdk.configureProviderSet('mapquest', providerCredential);

const canvas = /** @type {HTMLCanvasElement} */ (
  document.querySelector('#map-canvas')
);
const mapView = new BrowserMapView(canvas);
const adapterDescription = await mapView.initialize();
console.log(`Maps SDK ready (version ${getSdkCompiledVersion()}) on ${adapterDescription}`);
```

The demo skips registration until a visitor supplies a key; recorded tiles and
places fill the map. Register one capability at a time with
`mapSdk.configureProvider` when two providers serve different capabilities;
[Provider capabilities on WebAssembly](provider-capabilities-wasm.md) shows the
readback. Tile resolution follows the device pixel ratio;
`new BrowserMapView(canvas, TileResolution.Retina)` pins it, and raster tiles
come from the same-origin `/tiles/{zoom}_{tileX}_{tileY}.png` path. If
`initialize()` fails without WebGPU, show a degradation message naming
`navigator.gpu` and the MDN compatibility table; never sniff the user agent.

### Open the map

```js
const mapCamera = new MapCamera(38.5766, -121.4944, 12.0, 0.0, 0.0);
const fieldOfViewDegrees = 60.0;
const frameStatistics = await mapView.renderFrame(mapCamera, fieldOfViewDegrees);
```

`renderFrame` takes the camera by value, so build a fresh `MapCamera` for each
changed pose; native views pump their own frames. The camera also projects and
unprojects without rendering: `projectToScreen`, `screenToGroundCoordinate` and
`visibleTileKeys` return plain objects with lower-camel-case fields. The free
functions `panCameraByScreenPixels`, `zoomCameraByLevelsAroundScreenPoint` and
`rotateCameraByDegreesAroundScreenPoint` are the gesture equivalents.

</div>

</div>

## What you should see

A keyless session draws the provider's base map at the demo camera; with a
credential applied, search, routing and traffic answer. Continue with [Search
and routing](search-and-routing.md), [Turn-by-turn navigation](navigation-simulation.md),
and [Offline maps](offline-maps.md); the [FFI contract](udl-contract.md)
carries every member you called.
