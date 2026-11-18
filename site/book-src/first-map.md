# Your first map

Install the SDK, register a map provider, and draw your first map. Choose your
platform and follow the steps in order. Every tab uses the same demo camera:
Capitol Park, Sacramento at 38.5766, -121.4944, zoom 12, north-up and
top-down. The SDK calls compile against the [FFI contract](udl-contract.md).
If you are new to map SDKs, read [Map concepts](map-concepts.md) first.

## Choosing a provider

Register one provider, then apply its credential at **runtime** from a settings
screen, secure storage, or a launch environment for automated tests; never bake
it into the build. A keyless base map still draws; search, routing and traffic
respond once a provider that serves those contracts has a credential. The
[provider capability pages](provider-capabilities-android.md) list each provider,
what it serves and its credential.

## Camera and overlay rules

`animateCamera` eases to a pose over a duration, and `fitBounds` frames a box
inside a viewport with edge insets. `zoomByLevelsAroundScreenPoint` and
`rotateByDegreesAroundScreenPoint` apply pinch and twist deltas with the same
focal-point correction on every platform. `setOverlays` replaces the drawn list
in one go; `addOverlay`, `updateOverlay` and `removeOverlay` address one entry
at a time with an identifier that is monotonic per view. The browser descriptor
spells the color field `color`; the native tabs spell it `rgbaColor`.

<div class="platform-tabs">
<div class="platform-tab-titles" role="tablist">
<button type="button" data-platform-tab="android">Android</button>
<button type="button" data-platform-tab="ios">iOS</button>
<button type="button" data-platform-tab="react-native">React Native</button>
<button type="button" data-platform-tab="wasm">WebAssembly</button>
</div>

<div class="platform-tab-panel" data-platform-panel="android">

### Before you start

- Android Studio with the SDK 36 toolchain.
- A project with `minSdk = 24`.
- The AAR built by the SDK build script, copied into `app/libs/`.
- A provider API key for the provider you register.

### Install the artifact

In your application module's `build.gradle.kts`, add the local AAR and the two
runtime dependencies the Kotlin bindings need:

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

Keep the version in each file name exact.

The Compose entry point is optional; the AAR declares its Compose
dependencies `compileOnly`. A Compose host adds the Compose BOM,
`androidx.compose.ui:ui` and
`androidx.lifecycle:lifecycle-runtime-compose:2.9.4` from its own module.

### Build the SDK and register the provider

Construct the SDK and install the map-platform provider set in one call:

```kotlin
import uniffi.rustic_maps.MapSdk

val mapSdk = MapSdk()
// The key belongs to the provider you registered; see "Choosing a provider".
mapSdk.configureProviderSet("mapquest", providerApiKey)
```

The call throws the provider's message when the key is refused. Read the key
from a settings screen or secure configuration; the SDK has no build field for
a credential.

### Show the map in your layout

The package ships a lifecycle-aware `MapView` that owns the surface, frame
pump, resizing, gestures and camera animation. Drop it into your composition
and give it the SDK:

```kotlin
import com.rusticmaps.sdk.compose.MapView

MapView(
    sdk = mapSdk,
    listener = mapViewListener,
    modifier = Modifier.fillMaxSize(),
)
```

Place the plain view in your layout when you are not using Compose, or host it
with `AndroidView`; the widget is a plain `SurfaceView` subclass. It owns
attach, frames and detach.

### Open the map

Set the camera the view draws with:

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

Use `fitBounds` to frame a bounded region instead of hand-tuning zoom.

### Pan the camera from a gesture

Forward the drag delta to the camera member and keep the pose it returns; the
ground point the pointer grabbed becomes the new center:

```kotlin
val pannedCamera = camera.panByScreenPixels(
    deltaXPixels = dragDeltaXPixels,
    deltaYPixels = dragDeltaYPixels,
    viewport = ViewportSize(
        widthPixels = viewportWidthPixels,
        heightPixels = viewportHeightPixels,
    ),
    fieldOfViewDegrees = 60.0,
)
center = pannedCamera.center()
```

`MapView` applies pan, pinch and rotation from its own gesture handlers; call
this member when you drive the camera from your own control or a test.

### Add your first overlay

Set the overlay list the surface draws every frame. This route polyline uses
the demo's two waypoints (Capitol Park to Tower Bridge):

```kotlin
import uniffi.rustic_maps.GeographicCoordinate
import uniffi.rustic_maps.MapOverlay
import uniffi.rustic_maps.MapOverlayKind
import uniffi.rustic_maps.OverlayColor

mapView.setOverlays(
    listOf(
        MapOverlay(
            kind = MapOverlayKind.ROUTE_POLYLINE,
            coordinates = listOf(
                GeographicCoordinate(38.5766, -121.4944),
                GeographicCoordinate(38.5802, -121.5083),
            ),
            rgbaColor = OverlayColor(red = 0.2, green = 0.4, blue = 0.9, alpha = 0.9),
            widthPixels = 3.0,
            // The other payload fields stay null for this kind.
            ringCoordinates = null,
            pointMarkerPositions = null,
            pointMarkerRadiusPixels = null,
            trafficFlowSegments = null,
        ),
    ),
)
```

For overlays you keep across frames, address them individually:

```kotlin
val routeOverlayId = mapView.addOverlay(routePolylineOverlay)
// ...later, when the route changes:
mapView.updateOverlay(routeOverlayId, replacementPolylineOverlay)
// ...and when it is dismissed:
mapView.removeOverlay(routeOverlayId)
```

</div>

<div class="platform-tab-panel" data-platform-panel="ios">

### Before you start

- Xcode with Swift 5.10 (`swift-tools-version: 5.10`).
- A project with an iOS 17 deployment target.
- The iOS package built by the SDK build script.
- A provider API key for the provider you register.

Add the built Swift package and both products: `RusticMapsSDK`, the generated
bindings, and `MapNativeCoreUI`, the map view. The manifest pins the checksummed
XCFramework as its binary target:

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

### Build the SDK and register the provider

```swift
import RusticMapsSDK

let mapSdk = MapSdk()
// The key belongs to the provider you registered; see "Choosing a provider".
try mapSdk.configureProviderSet(providerKey: "mapquest", credential: providerApiKey)
```

The call throws the provider's message when the key is refused. Read the key
from app settings, a secure configuration store, or a launch environment for
automated tests; do not add it to `Info.plist` or to a shipped `.xcconfig`.

### Show the map in your layout

The `MapNativeCoreUI` package ships a `MapView` that owns the Metal surface,
frame pump, resizing, gestures and camera animation:

```swift
// `mapView` is the `MapNativeCoreUI.MapView` the package ships: it owns the
// Metal surface, the frame pump, resizing and gestures.
mapView.sdk = mapSdk
```

`SwiftUIMapView` wraps the same view for SwiftUI hosts. The view owns attach,
frames and teardown; you never create a surface handle or drive a display link.

### Open the map

Set the camera the view draws with:

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

### Pan the camera from a gesture

Keep the gesture recognizers in your view and forward their deltas to the
camera member:

```swift
let pannedCamera = try camera.panByScreenPixels(
    deltaXPixels: dragDeltaPixels.width,
    deltaYPixels: dragDeltaPixels.height,
    viewport: viewport,
    fieldOfViewDegrees: fieldOfViewDegrees
)
let pannedCenter = pannedCamera.center()
```

### Fit the map to what you show

For a bounded region, let the camera fit the box instead of hand-tuning
zoom:

```swift
let fittedCamera = try MapCamera.fitBounds(
    boundingBox: regionBox,
    viewport: viewport,
    edgeInsets: EdgeInsets(
        left: 224, top: 48, right: 48, bottom: 96
    ),
    maximumZoomLevel: 22
)
```

</div>

<div class="platform-tab-panel" data-platform-panel="react-native">

### Before you start

- Node.js and npm.
- An initialized React Native bare-workflow application, using React Native
  0.88 or later and React 19 or later.
- Android: Android Studio with the Android SDK and NDK. iOS: Xcode and
  CocoaPods.
- The tarball built by the SDK build script. It carries the TurboModule glue
  and the compiled Rust libraries for Android (arm64-v8a, armeabi-v7a,
  x86_64) and iOS (arm64 device and simulator); no Rust toolchain is needed.
- A provider API key for the provider you register.

### Install the artifact

Install the tarball you built; no registry is involved:

```bash
npm install ./rustic-maps-sdk-rn-v0.1.0.tgz
```

The package carries the TurboModule glue and the compiled Rust libraries, so
your build does not compile Rust. Link it with normal autolinking. On iOS, run
`pod install`. On Android, set
`reactNativeArchitectures=armeabi-v7a,arm64-v8a,x86_64` in
`android/gradle.properties`; the template default adds `x86`, which the
package does not ship.

### Build the SDK and register the provider

The package owns one native SDK per process. Create it and register the
provider's credential:

```js
import { MapsSdk } from '@angoratek/rustic-maps-sdk/MapsSdk';

export async function createMapSdk(providerKey, providerApiKey) {
  const mapSdk = await MapsSdk.create();
  const registration = await mapSdk.setProviderCredential(
    providerKey,
    providerApiKey,
  );
  if (!registration.isApplied) {
    throw new Error(registration.detailText);
  }
  return mapSdk;
}
```

A refused credential resolves with `isApplied: false` and the verbatim
`detailText`; surface it rather than continuing. Register one provider and
apply its credential:

```js
import { MapsSdk } from '@angoratek/rustic-maps-sdk/MapsSdk';

// MapQuest, with a key the user entered in your settings screen. A keyless
// URL-template base map needs no credential at all.
const mapSdk = await MapsSdk.create();
await mapSdk.setProviderCredential('mapquest', providerApiKey);
```

Take the key from a settings screen, secure storage, or a launch environment
for automated tests. Do not bake it into the JS bundle or a native build file.

### Draw a map surface

The package ships a typed `<MapView>` component over the native widget
(Android `SurfaceView`, iOS `CAMetalLayer`). It owns the surface lifecycle,
frame pump, gestures, camera calls, id-addressed overlays and callbacks. Create
the one native SDK instance before you mount the view.

<!-- docs-snippet: skip — a JSX fragment outside the docs checks. -->
```tsx
import { useRef } from 'react';
import { MapView, type MapViewHandle } from '@angoratek/rustic-maps-sdk/MapView';

// Hold the handle to drive the camera, overlays and location layer.
const mapViewRef = useRef<MapViewHandle | null>(null);

<MapView
  ref={mapViewRef}
  style={{ flex: 1 }}
  topBarInsetPixels={topBarInsetPixels}
  bottomBarInsetPixels={bottomBarInsetPixels}
  onMapReady={() => { /* the first frame presented */ }}
  onCameraChanged={(camera, isGestureDriven) => { /* reflect the pose */ }}
  onError={(failure) => { /* failure.phase, failure.message */ }}
/>;
```

Use the component as the documented path; it keeps one SDK instance and one
frame pump. The generated contract classes remain available for advanced use
such as a custom host view.

Check the installation with the compiled-version probe:

```js
import { getSdkCompiledVersion } from '@angoratek/rustic-maps-sdk';

getSdkCompiledVersion();
// → "0.1.0": the Rust core loaded and answered through the TurboModule.
```

### Open the map

The component's handle takes the same camera values the other tabs use:

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

### Pan the camera from a gesture

The generated `MapCamera` carries the same gesture members on the JavaScript
side. The native views call them for their gestures; use them when you drive
the camera from a control of your own or a test:

```js
const pannedCamera = camera.panByScreenPixels(
  dragDeltaXPixels,
  dragDeltaYPixels,
  viewport,
  fieldOfViewDegrees,
);
const pannedCenter = pannedCamera.center();
```

</div>

<div class="platform-tab-panel" data-platform-panel="wasm">

### Before you start

- A Chromium-based browser with WebGPU: flag-free in Chromium since 113,
  Safari from 26, and Firefox from 141 on desktop.
- The package built by the SDK build script for WebAssembly.

A page needs a provider key only for live search, routing, and imagery;
keyless, it renders recorded tiles and recorded places.

### Install the artifact

Install the WebAssembly package with npm, or extract the same tarball and serve
its `package/` directory over HTTPS or localhost:

```bash
npm install ./rustic-maps-sdk-web-v0.1.0.tgz
cp -r node_modules/@angoratek/rustic-maps-sdk-web ./web
python3 -m http.server 8000   # any static server; localhost is fine
```

```bash
tar -xzf rustic-maps-sdk-web-v0.1.0.tgz
cp -r package ./web
```

### Start the SDK and show the map on a canvas

One page creates one SDK root. Provider registration is a separate free call
whose first argument is the provider key from the shared registry. Read the
credential at **runtime** from a settings form, a server-issued token, or a
test's environment, never from the bundle.

Supply a `<canvas>` element and hand it, plus the SDK root, to `MapView`. The
view claims the element, acquires the WebGPU device, and renders tiles fetched
from a same-origin path you host; the page keeps pointer handling.

```html
<canvas id="map-canvas"></canvas>
```

```js
import init, { MapsSdk, MapView, configureProviderSet } from './web/rustic_maps_web_bindings.js';

await init();
const mapSdk = await MapsSdk.create();
configureProviderSet('mapquest', providerCredential);

const canvas = /** @type {HTMLCanvasElement} */ (
  document.querySelector('#map-canvas')
);
const mapView = new MapView(canvas, mapSdk);
const adapterDescription = await mapView.initialize();
console.log(`Maps SDK ready (version ${mapSdk.compiledVersion}) on ${adapterDescription}`);
```

The demo skips the registration call until a visitor supplies a key; recorded
tiles and recorded places fill the map meanwhile. To serve one capability from
one provider and another from a second, register them one at a time with
`configureProvider`; [Provider capabilities on
WebAssembly](provider-capabilities-wasm.md) shows the readback.

The status probe returns the native surfaces' version string. Tile resolution
follows the device pixel ratio; the optional third constructor argument pins it
(`new MapView(canvas, mapSdk, TileResolution.Retina)`). The canvas fetches
raster tiles from the same-origin `/tiles/{zoom}_{tileX}_{tileY}.png` path.

If `initialize()` fails without WebGPU, give the page a degradation message
that names `navigator.gpu` and links the MDN compatibility table. Never sniff
the user agent.

### Open the map

Set the camera the view draws with, then present a frame:

```js
const mapCamera = new MapCamera(38.5766, -121.4944, 12.0, 0.0, 0.0);
const fieldOfViewDegrees = 60.0;
const frameStatistics = await mapView.renderFrame(mapCamera, fieldOfViewDegrees);
```

`renderFrame` takes the camera by value, so build a fresh `MapCamera` for each
changed pose. The native tabs let their view pump frames; the browser page
calls `renderFrame` itself on every change.

### Project and unproject

The same camera projects and unprojects without rendering:

```js
const screenPoint = camera.projectToScreen(
  latitudeDegrees,
  longitudeDegrees,
  canvas.width,
  canvas.height,
  fieldOfViewDegrees,
);
const groundCoordinate = camera.screenToGroundCoordinate(
  screenPoint.x,
  screenPoint.y,
  canvas.width,
  canvas.height,
  fieldOfViewDegrees,
);
const tileKeys = camera.visibleTileKeys(
  canvas.width,
  canvas.height,
  fieldOfViewDegrees,
  tileZoom,
);
```

These methods return plain objects with lower-camel-case fields. Equivalent
free functions (`panCameraByScreenPixels`, `zoomCameraByLevelsAroundScreenPoint`,
`rotateCameraByDegreesAroundScreenPoint`) are also available.

### Pan the camera from a gesture

Keep pointer capture, wheel and touch handling in your page and forward the
travel to the camera member:

```js
const pannedCamera = camera.panByScreenPixels(
  deltaXPixels,
  deltaYPixels,
  canvas.width,
  canvas.height,
  fieldOfViewDegrees,
);
const pannedCenter = pannedCamera.center();
```

Equivalent free functions (`panCameraByScreenPixels`,
`zoomCameraByLevelsAroundScreenPoint`,
`rotateCameraByDegreesAroundScreenPoint`) are also available.

### Add your first overlay

Set the overlay list the view draws every frame:

```js
const routeOverlay = {
  kind: 'routePolyline',
  coordinates: [
    { latitude: 38.5766, longitude: -121.4944 },
    { latitude: 38.5802, longitude: -121.5083 },
  ],
  color: { red: 0.2, green: 0.4, blue: 0.9, alpha: 0.9 },
  widthPixels: 3.0,
};
mapView.setOverlays([routeOverlay]);
```

</div>

</div>

## Where to continue

These guides are one worked example. The same `mapCamera`, `mapSdk`, and
`mapView` continue through [Search and routing](search-and-routing.md),
[Turn-by-turn navigation](navigation-simulation.md), and [Offline
maps](offline-maps.md). The [FFI contract](udl-contract.md) carries every
member you called, and the generated [API reference](api-reference.md)
documents them.
