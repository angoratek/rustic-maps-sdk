# Your first map

Install the SDK, register a map provider, and draw your first map. Choose
your platform and follow the steps in order. The SDK calls in these steps
compile against the [FFI contract](udl-contract.md). If you are new to map
SDKs, read [Map concepts](map-concepts.md) first: it explains tiles, zoom
levels, the world coordinate system, and the camera these steps build on.

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

In your application module's `build.gradle.kts`, add the local AAR and
the two runtime dependencies the Kotlin bindings need:

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

The Compose entry point is optional, and the AAR declares its Compose
dependencies `compileOnly`. A Compose host adds the Compose BOM,
`androidx.compose.ui:ui` (which carries the Compose runtime) and
`androidx.lifecycle:lifecycle-runtime-compose:2.9.4` (the wrapper reads
`LocalLifecycleOwner`) from its own application module. A host that places the
plain map view with `AndroidView` already supplies `androidx.compose.ui:ui`,
because `AndroidView` comes from it, but does not need
`lifecycle-runtime-compose`.

### Build the SDK and register the provider

Construct the SDK object and install the map-platform provider set in one
call:

```kotlin
import uniffi.rustic_maps.MapSdk

val mapSdk = MapSdk()
// The key belongs to the provider you registered; see "Choosing a provider".
mapSdk.configureProviderSet("mapquest", providerApiKey)
```

The call throws the provider's message when the key is refused. Surface it
in your error state instead of continuing.

Read the key at **runtime**: from your own secure configuration or a
settings screen, never from source and never baked into the build. The SDK
has no build field for a credential.


### Show the map in your layout

The package ships a lifecycle-aware `MapView` that owns the surface, the
frame pump, resizing, gestures and the camera animation. Drop it into your
composition and give it the SDK:

```kotlin
import com.rusticmaps.sdk.compose.MapView

MapView(
    sdk = mapSdk,
    listener = mapViewListener,
    modifier = Modifier.fillMaxSize(),
)
```

Place the plain view in your layout when you are not using Compose, or host it
with `AndroidView` from a Compose screen; the widget is a plain `SurfaceView`
subclass. Either way you never touch a native window pointer, never attach, and
never pump frames — the view does all of it, and pauses itself when the host
stops or the window detaches.

### Open the map

Set the camera the view draws with. The shared demo camera on every
platform is Capitol Park, Sacramento at zoom 12, north-up and top-down:

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

`animateCamera` eases to a pose over a duration. `fitBounds` frames a box
inside a viewport with edge insets. Use them to keep search results and the
route preview in view.

### Pan the camera from a gesture

Gesture recognition stays in your platform view; the camera algebra is the
SDK's. Forward the drag delta to the camera member and keep the pose it
returns (the ground point the pointer grabbed becomes the new center):

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

`MapView` applies pan, pinch, and rotation from its own gesture handlers.
Call this member when you drive the camera from your own control or from a
test.

`zoomByLevelsAroundScreenPoint` and `rotateByDegreesAroundScreenPoint` apply
the pinch and twist deltas with the same focal-point correction, so every
platform computes identical results.

### Add your first overlay

Draw more than tiles by setting the overlay list the surface draws every
frame. A route polyline over the demo's two waypoints (Capitol Park to
Tower Bridge):

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

`setOverlays` is the bulk call for a list you own whole: it replaces the
drawn list in one go. For overlays you keep across frames, address them
individually: `addOverlay` returns an opaque identifier, `updateOverlay`
replaces that entry in its draw position, and `removeOverlay` returns
whether it was present:

```kotlin
val routeOverlayId = mapView.addOverlay(routePolylineOverlay)
// ...later, when the route changes:
mapView.updateOverlay(routeOverlayId, replacementPolylineOverlay)
// ...and when it is dismissed:
mapView.removeOverlay(routeOverlayId)
```

Identifiers are monotonic per view and never reused; `clearOverlays()`
clears both the bulk list and the addressed overlays.

</div>

<div class="platform-tab-panel" data-platform-panel="ios">

### Before you start

- Xcode with Swift 5.10 (`swift-tools-version: 5.10`).
- A project with an iOS 17 deployment target.
- The iOS package built by the SDK build script.
- A provider API key for the provider you register.

Add the built Swift package and both of its products: `RusticMapsSDK`
(the generated bindings) and `MapNativeCoreUI` (the map view). The package
manifest pins the checksummed XCFramework as its binary target:

```swift
// Package.swift
// swift-tools-version: 5.10
import PackageDescription

let package = Package(
    name: "MyMapApp",
    dependencies: [
        // The generated branch manifest: its binaryTarget pins the XCFramework
        // URL and checksum.
        .package(
            url: "https://github.com/angoratek/rustic-maps-sdk.git",
            branch: "swift-package"
        ),
    ],
    targets: [
        .executableTarget(
            name: "MyMapApp",
            // Both products ship in the same package: the bindings and the
            // map view.
            dependencies: [
                .product(name: "RusticMapsSDK", package: "rustic-maps-sdk"),
                .product(name: "MapNativeCoreUI", package: "rustic-maps-sdk"),
            ]
        )
    ]
)
```

### Build the SDK and register the provider

Construct the SDK and register the provider:

```swift
import RusticMapsSDK

let mapSdk = MapSdk()
// The key belongs to the provider you registered; see "Choosing a provider".
try mapSdk.configureProviderSet(providerKey: "mapquest", credential: providerApiKey)
```

The call throws the provider's message when the key is refused.

Read the key at **runtime**: from app settings, a secure configuration
store, or a launch environment for automated tests. Do not add it to
`Info.plist` or to an `.xcconfig` that ships in the app.


### Show the map in your layout

The `MapNativeCoreUI` package ships a `MapView` that owns the Metal
surface, the frame pump, resizing, gestures and the camera animation. Put
it in your view hierarchy and give it the SDK:

```swift
// `mapView` is the `MapNativeCoreUI.MapView` the package ships: it owns the
// Metal surface, the frame pump, resizing and gestures. Hand it the SDK and
// put it in your hierarchy.
mapView.sdk = mapSdk
```

`SwiftUIMapView` wraps the same view for SwiftUI hosts. You never create a
surface handle, never attach, and never drive a display link — the view
does all of it and tears itself down when it leaves the hierarchy.

### Open the map

Set the camera the view draws with. The shared demo camera on every
platform is Capitol Park, Sacramento at zoom 12, north-up and top-down:

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

`animateCamera` eases to a pose over a duration. `fitBounds` frames a box
inside a viewport with edge insets. Use them to keep search results and the
route preview in view.

### Pan the camera from a gesture

Keep the gesture recognizers in your view and forward their deltas to the
camera member (the ground point the finger grabbed becomes the new center):

```swift
let pannedCamera = try camera.panByScreenPixels(
    deltaXPixels: dragDeltaPixels.width,
    deltaYPixels: dragDeltaPixels.height,
    viewport: viewport,
    fieldOfViewDegrees: fieldOfViewDegrees
)
let pannedCenter = pannedCamera.center()
```

`zoomByLevelsAroundScreenPoint` and `rotateByDegreesAroundScreenPoint` apply
pinch and twist deltas with the same focal-point correction.

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
- An initialized React Native (bare workflow) application, using React
  Native 0.88 or later and React 19 or later.
- Android: Android Studio with the Android SDK and NDK. Gradle compiles the
  module's C++ and links the packaged Rust archives.
- iOS: Xcode and CocoaPods, to install the package's pod.
- The tarball built by the SDK build script. It carries the TurboModule glue and
  the compiled Rust libraries for Android (arm64-v8a, armeabi-v7a, x86_64)
  and iOS (arm64 device and simulator). No Rust toolchain is needed.
- A provider API key for the provider you register.

### Install the artifact

Install the tarball you built; no registry is involved:

```bash
npm install ./rustic-maps-sdk-rn-v0.1.0.tgz
```

The package carries the TurboModule glue and the compiled Rust libraries, so
your build does not compile Rust. Link it with your React Native version's
normal autolinking. On iOS, run `pod install`. On Android, set
`reactNativeArchitectures=armeabi-v7a,arm64-v8a,x86_64` in
`android/gradle.properties` (the React Native template default adds `x86`,
which the package does not ship); the next Gradle build compiles the module's
C++ and links the packaged archives.

### Build the SDK and register the provider

The package owns one native SDK per process. Create it with
`MapsSdk.create()` and register the provider's credential:

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

A refused credential resolves with `isApplied: false` and the provider's
verbatim `detailText`, so surface it in your error state rather than
continuing.

### Choosing a provider

The credential call is the same on every platform; what changes is **which
provider** you register and where its key comes from.

| Provider | What it serves | Credential |
|---|---|---|
| **MapQuest** | geocoding, search, routing, the distance matrix, traffic, and raster tiles | an API key from the MapQuest developer console |
| **HERE** | geocoding, routing, traffic | a key from the HERE developer console |
| **Google Maps** | geocoding and routing | a key from the Google Cloud console |
| **TomTom** | geocoding, routing, traffic, and raster tiles | a key from the TomTom developer portal |
| **Mapbox** | geocoding and routing | an access token from the Mapbox account page |
| **Radar** | geocoding and routing | a key from the Radar dashboard |
| **Photon** | geocoding and search | none for the public instance, or the base URL of your deployment |
| **OSRM** | routing | the base URL of your deployment; the public demo server must be named explicitly |
| **Valhalla** | routing | the base URL of your deployment; the public demo server must be named explicitly |
| **A URL-template raster source** (OpenStreetMap, a self-hosted tile server) | base-map tiles only | usually none — some ask for a User-Agent instead |
| **A URL-template vector source** (OpenFreeMap, OpenStreetMap US) | a styled vector base with labels | none |
| **Your own provider** | whichever contracts you implement | whatever your implementation needs |

Register the one you intend to use, then apply its credential:

```js
import { MapsSdk } from '@angoratek/rustic-maps-sdk/MapsSdk';

// MapQuest, with a key the user entered in your settings screen. A keyless
// URL-template base map needs no credential at all: register that source and
// its tiles flow.
const mapSdk = await MapsSdk.create();
await mapSdk.setProviderCredential('mapquest', providerApiKey);
```

A keyless base map still draws, and search, routing, and traffic respond only
once a provider that serves those contracts has a credential. Nothing here
ties your app to MapQuest: the providers your app registers decide what is
available, and the calls above stay the same.

Take the key at **runtime**: a settings screen, secure storage, or a launch
environment for automated tests. Do not bake it into the JS bundle or a
native build file.


### Draw a map surface

The package ships a typed `<MapView>` component over the native widget
(Android `SurfaceView`, iOS `CAMetalLayer`). It owns the surface lifecycle,
frame pump, gestures, camera calls, id-addressed overlays, and the callback
set, so the host application only composes it. The package
also owns the one native SDK instance; create it with `MapsSdk.create()`
before you mount the view, which attaches to it as soon as it is available.

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

Use the component as the documented path: it keeps one SDK instance and one
frame pump. The generated contract classes from the package root remain
available for advanced use such as a custom host view.

Check the installation with the compiled-version probe:

```js
import { getSdkCompiledVersion } from '@angoratek/rustic-maps-sdk';

getSdkCompiledVersion();
// → "0.1.0": the Rust core loaded and answered through the TurboModule.
```

### Open the map

The component's handle takes the same camera values the other tabs use. The
shared demo camera on every platform is Capitol Park, Sacramento at zoom 12,
north-up and top-down:

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

`animateCamera` and `fitBounds` are on the same handle, with the same
signatures the other tabs show.

### Pan the camera from a gesture

The generated `MapCamera` carries the same gesture members on the JavaScript
side. The package's native views call them for their own gestures; reach for
them when you drive the camera from a control of your own or from a test:

```js
const pannedCamera = camera.panByScreenPixels(
  dragDeltaXPixels,
  dragDeltaYPixels,
  viewport,
  fieldOfViewDegrees,
);
const pannedCenter = pannedCamera.center();
```

`zoomByLevelsAroundScreenPoint` and `rotateByDegreesAroundScreenPoint` apply
pinch and twist deltas with the same focal-point correction.

### Where to continue

The camera block is the first step of the worked example that runs through
these guides. The same `mapCamera`, `mapSdk`, and `mapView` continue through
[Search and routing](search-and-routing.md), [Turn-by-turn
navigation](navigation-simulation.md), and [Offline maps](offline-maps.md)
with the same names. Continue there once this map draws.

</div>

<div class="platform-tab-panel" data-platform-panel="wasm">

### Before you start

- A Chromium-based browser with WebGPU: flag-free in Chromium since 113,
  Safari from 26, and Firefox from 141 on desktop.
- The package built by the SDK build script for WebAssembly.

A page needs a provider key only for live search, routing, and imagery.
Keyless, it renders recorded tiles and recorded places.

### Install the artifact

Install the WebAssembly package with npm, or extract the same tarball and
serve its `package/` directory from any static file server over HTTPS or
localhost:

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

One page creates one SDK root. Provider registration is a separate free
call whose first argument is the provider key from the shared registry;
read the credential at **runtime** — a settings form, a server-issued
token, or a test's environment — never from the bundle.

The browser has no native view to drop into a layout. You supply a
`<canvas>` element and hand it, plus the SDK root, to `MapView`. The view
claims the element, acquires the WebGPU device, and renders tiles fetched
from a same-origin path you host. The page keeps pointer handling; the
view owns the surface and the frame.

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

The demo skips the registration call until a visitor supplies a key;
recorded tiles and recorded places fill the map meanwhile. To serve one
capability from one provider and another from a second, register them one at
a time with `configureProvider`; [Provider capabilities on
WebAssembly](provider-capabilities-wasm.md) shows the readback.

The status probe returns the same version string the native surfaces do. The
view's tile resolution follows the device pixel ratio; the optional third
constructor argument pins it (`new MapView(canvas, mapSdk,
TileResolution.Retina)`). The canvas fetches raster tiles from the
same-origin `/tiles/{zoom}_{tileX}_{tileY}.png` path, so serve your imagery
there.

If `initialize()` fails in a browser without WebGPU, give the page the
documented degradation message (name `navigator.gpu` and the missing
capability; link the MDN compatibility table) — never a user-agent
sniff.

### Open the map

Set the camera the view draws with, then present a frame. The shared demo
camera on every platform is Capitol Park, Sacramento at zoom 12, north-up
and top-down:

```js
const mapCamera = new MapCamera(38.5766, -121.4944, 12.0, 0.0, 0.0);
const fieldOfViewDegrees = 60.0;
const frameStatistics = await mapView.renderFrame(mapCamera, fieldOfViewDegrees);
```

`renderFrame` takes the camera by value, so build a fresh `MapCamera` for
each changed pose. The other tabs hand their view a camera and let it pump
frames; the browser page calls `renderFrame` itself on every change.

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

These methods return plain objects with lower-camel-case fields; the
generated declarations describe each shape.

### Pan the camera from a gesture

There is no gesture layer on this surface: keep pointer capture, wheel and
touch handling in your page and forward the travel to the camera member. It
returns the resulting pose as a plain object (the ground point the pointer
grabbed becomes the new center):

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

`zoomByLevelsAroundScreenPoint` and `rotateByDegreesAroundScreenPoint` apply
pinch and twist deltas with the same focal-point correction. Equivalent free
functions (`panCameraByScreenPixels`,
`zoomCameraByLevelsAroundScreenPoint`,
`rotateCameraByDegreesAroundScreenPoint`) are also available.

### Add your first overlay

Draw more than tiles by setting the overlay list the view draws every
frame. The descriptor uses the browser's field spelling: `color`, not the
native tabs' `rgbaColor`.

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

`setOverlays` replaces the drawn list in one go. For an overlay you keep
across frames, `addOverlay` returns an opaque identifier, `updateOverlay`
replaces that entry in its draw position, and `removeOverlay` returns
whether it was present.

### Where to continue

The camera block is the first step of the worked example that runs through
these guides. The same `mapCamera`, `mapSdk`, and `mapView` continue through
[Search and routing](search-and-routing.md), [Turn-by-turn
navigation](navigation-simulation.md), and [Offline maps](offline-maps.md)
with the same names. Continue there once this map draws.

</div>

</div>

## What you verified

You built the SDK entry object, registered a map provider with your own key,
and drew map imagery through the SDK renderer. The [FFI
contract](udl-contract.md) carries every member you called, and the generated
[API reference](api-reference.md) documents them.
