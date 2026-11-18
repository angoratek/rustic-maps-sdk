# Architecture

What this covers: the four targets behind one contract, the crate graph, the
`MapView` surface and ownership model, the provider layer, and offline routing.

## 1. What the SDK is

Rustic Maps SDK renders maps on the GPU, resolves places, plans routes, follows a
turn-by-turn trip from simulated location updates, and routes offline from an
OpenStreetMap graph. One Rust core serves Android (Kotlin), iOS (Swift/SwiftUI),
React Native and WebAssembly. All four sit on one frozen UniFFI contract,
`crates/rustic-maps-native-core/src/rustic_maps.udl`; Kotlin, Swift and the React
Native Turbo Module are generated from it, the browser module is generated from
the same contract by the `wasm2` bindgen with a hand-written canvas class beside
it, and `platform/web/tests/contract_sync.mjs` keeps the contract split honest.
`MapView` is the entry point
on every target. The renderer is an own minimal `wgpu` renderer, raster first,
with vector styling drawn onto the same overlay pipeline.

## 2. Crate graph

The workspace has 17 crates. The provider layer is a further set of published
everymap-rs crates, consumed as versioned dependencies rather than path
dependencies, so the SDK builds against exactly what a third party would.

| Crate | Owns | Depends on |
|---|---|---|
| `geospatial-types` | Records, haversine math, typed errors | `thiserror`, `serde` |
| `geospatial-traits` | The SDK's ten provider contracts and compile-time mocks | `geospatial-types` |
| `everymap-core` (published) | Provider contracts, transport seam, pacing, retry, observation, credentials | Its transport stack |
| `everymap-providers-*` (published, 11 crates) | Vendor knowledge: HERE, Google, TomTom, Mapbox, Radar, MapQuest, Photon, OSRM, Valhalla, URL-template raster, URL-template vector | `everymap-core` |
| `rustic-maps-providers` | Shared provider catalog and credentialed vendor construction for HERE, Google, TomTom, Mapbox, Radar; opt-in `style-smoke` | `everymap-core`, vendor crates, `geospatial-types`, `rustic-maps-core` |
| `rustic-maps-core` | Provider registry, cache-then-network orchestration, everymap bridge, offline routing consumer and fallback, style resources | `geospatial-types`, `geospatial-traits`, `everymap-core`, `rustic-maps-offline-graph-archive` |
| `rustic-maps-cache` | SQLite request/response cache with TTL, LRU and quota | `geospatial-types`, `geospatial-traits`, `rustic-maps-core` |
| `rustic-maps-rendering` | Projection, camera, tile scheduler, texture pool, `wgpu` renderer, vector styling, label pass | `rustic-maps-vector-tiles`, `rustic-maps-style`, `rustic-maps-glyphs`, `geospatial-types`, `geospatial-traits`, `rustic-maps-core` |
| `rustic-maps-vector-tiles` | Mapbox Vector Tile v2.1 decoder | `thiserror` |
| `rustic-maps-style` | MapLibre style-specification version-8 model, expressions, filters, CSS colours | `thiserror`, `serde_json` |
| `rustic-maps-glyphs` | Glyph PBF reader, byte-bounded range cache, R8Unorm atlas packer | `thiserror`, `rustic-maps-vector-tiles` |
| `rustic-maps-navigation` | Turn-by-turn navigation state machine | `geospatial-types`, `web-time` |
| `rustic-maps-positioning` | Device-position validation, course smoothing, staleness | `geospatial-types`, `rustic-maps-navigation` |
| `rustic-maps-offline-graph-archive` | RMOG reader and deterministic writer, wasm-safe; native mmap seam | `geospatial-types`; `memmap2` natively |
| `rustic-maps-offline-graph-compiler` | OpenStreetMap PBF to RMOG compiler; native-only maintainer and CI tool | `geospatial-types`, `osmpbf`, `rustic-maps-offline-graph-archive` |
| `rustic-maps-native-core` | The only UniFFI binding crate: UDL, native surface view, engines, provider catalog, runtime, JNI shim | `rustic-maps-core`, `rustic-maps-navigation`, `rustic-maps-positioning`, `rustic-maps-rendering`, `rustic-maps-providers`, everymap providers, `rustic-maps-cache` natively, `rustic-maps-web-bindings` on wasm |
| `rustic-maps-web-bindings` | wasm-bindgen canvas `MapView`, browser provider dispatch, vector base, refapp math exports | `rustic-maps-core`, `rustic-maps-rendering`, `rustic-maps-navigation`, `rustic-maps-positioning`, `rustic-maps-providers`, everymap providers, `rustic-maps-vector-tiles`, `rustic-maps-style`, `rustic-maps-glyphs`, `map-refapp-core` |
| `map-refapp-core` | Pure behavior math and flow-state reducer shared by the reference applications | None |
| `map-refapp-native` | Reference-app-only UniFFI bindings over that math | `map-refapp-core`, `uniffi` |

Dependency direction is upward from records to orchestration to consumers.
`geospatial-types` has no workspace dependency and is the record and math layer.
`geospatial-traits` adds the SDK's ten provider contracts and depends only on
records. `rustic-maps-core` is orchestration; it names no provider crate and
depends on `everymap-core` and the archive. `rustic-maps-providers` is the shared
construction seam both bindings read. The two binding crates are the only crates
that construct vendors and export a contract; on wasm,
`rustic-maps-native-core` consumes `rustic-maps-web-bindings` so one generated
module carries the contract surface and the hand-written canvas classes with one
initialization and one provider registry. The offline compiler is a test-only
fixture producer in `rustic-maps-core` and never enters the production dependency
graph; the wasm-viable archive crate is consumed directly by the core.

## 3. The contract

### One UDL, four consumers

`crates/rustic-maps-native-core/src/rustic_maps.udl` is the single source of the
public API, in the `rustic_maps` namespace and pinned to `uniffi 0.31.2`.
`crates/rustic-maps-native-core/src/rustic_maps_wasm.udl` is exactly that contract
minus the native surface `MapView` interface and its documentation block; both the
wasm scaffolding and the bindgen consume that one file.

| Target | Generated from | Shipped as |
|---|---|---|
| Kotlin | `rustic_maps.udl` through UniFFI | Bindings AAR plus the Kotlin `MapView` wrapper AAR |
| Swift | `rustic_maps.udl` through UniFFI | `RusticMapsSDK` and `MapNativeCoreUI` in the XCFramework and Swift package |
| React Native | `rustic_maps.udl` through `uniffi-bindgen-react-native` | TurboModule package and typed `<MapView>` |
| WebAssembly | `rustic_maps_wasm.udl` through `wasm2`, plus the hand-written canvas adapter | The browser npm package |

The browser cannot get its view class from the UDL: a DOM `HTMLCanvasElement`
and a `GPUDevice` are not expressible in UniFFI types. A thin wasm-bindgen canvas
adapter therefore stays hand-written, and it is the only hand-maintained piece of
the wasm surface. Everything else — record shapes, enum spellings, method and
class names, error variants — is generated from the one contract, so the browser
writes the same camelCase names as Kotlin, Swift and React Native. The canvas
class is exported as `BrowserMapView`, named distinctly so the contract
`MapView`, which the WebAssembly contract strips, can never reach the page.
`MapCamera` and the gesture pose algebra stay beside it in the same wasm-bindgen
glue; the contract's records never cross into the canvas.

The wasm entry keeps the product name with an async factory
(`MapsSdk.create()`), while the contract interface is the direct constructor
`MapSdk()` that Kotlin and Swift bind. The shapes differ on purpose — a browser
page awaits its module and then a factory; native hosts construct the
interface — and `contract_sync.mjs` records that one-name tolerance rather than
mirroring an async factory the UDL does not carry.

### Addition-only changes

The contract is addition-only by default. A breaking change before 1.0 requires
an explicit, recorded product decision and ships with regenerated bindings,
reference applications and documentation in the same commit; the revision that
renamed types and members and introduced the `rustic_maps` namespace is the
standing example. `contract_sync.mjs` is addition-tolerant by design: it asserts
the wasm TypeScript mirror against the UDL rather than pinning a checksum.
Generated bindings are build artifacts, not source. After a UDL change, run
`scripts/build-android.sh` and `scripts/build-ios.sh`, and `scripts/build-rn.sh`
when its surface matters, before trusting a platform build.

### UniFFI constraints

- Per-member `[Async, Throws=SdkError, Name=...]` only; interface-level `[Async]`
  is rejected by the UDL parser.
- `[Trait, WithForeign]`: the UDL placeholder traits are discarded by the macro
  path, so the binding crate defines the real Rust traits itself, with matching
  signatures, `Send + Sync` supertraits and a `#[doc(hidden)]
  uniffi_foreign_handle` default. `async-trait` is a direct binding-crate
  dependency.
- Orphan rule: FFI records are crate-local mirrors with explicit converters;
  E0210 forbids implementing FFI traits for foreign-crate types.
- ubrn renders module identifiers verbatim from the namespace, so the namespace
  must be `rustic_maps`; hyphens produce invalid TypeScript. It resolves the
  crate through `cargo metadata` walking up from the current directory.
- On the 0.31.2 line the generated TypeScript's 108 checksum items agree with
  the Rust scaffolding; that alignment retires the guarded-deep-import
  workaround an earlier release required.

Exported `[Async]` methods hand reactor-dependent work (reqwest, `tokio::time`)
through the crate-owned runtime in `crates/rustic-maps-native-core/src/runtime.rs`
using the `drive()` handback; foreign event loops otherwise panic with "there is
no reactor running". Locks stay short and never span an await, so every exported
future stays `Send`.

## 4. The `MapView` surface and ownership model

The intent is one object a developer holds per screen; engines hold the SDK root;
the wasm harness gets the same names. Spelling in the table is as a developer
types it in Kotlin, Swift or TypeScript. UniFFI camelCases UDL `snake_case`
automatically, so the UDL keeps snake_case; the column is the consumer-facing
name.

### Master naming table

| Concept | HERE | Mapbox v11 | Google | OSM / MapLibre | Before v2 | Adopted in v2 |
|---|---|---|---|---|---|---|
| SDK root object | `SDKNativeEngine` (shared instance) | `MapboxMapsOptions` + `MapView` | `MapsInitializer` | `MapLibre.getInstance` | `MapSdk` | **`MapsSdk`** (Rust type stays `MapSdk`) |
| Embeddable map surface | `MapView` (`FrameLayout`) | `MapView` | `MapView` / `GMSMapView` | `MapView` / `FlutterMap` | `MapSurfaceView` (UDL), `GpuMapCanvas` (wasm) | **`MapView`** |
| Platform surface handle | — (internal) | `MapSurface` (`TextureView`/`SurfaceView`) | — | `NativeMapView` | `MapSurfaceTargetHandle` | **`SurfaceHandle`** (internal, hidden) |
| Lifecycle | `onCreate/onResume/onPause/onDestroy` | `onStart/onStop/onLowMemory/onDestroy` | `onCreate…onDestroy` + `onSaveInstanceState` | `onCreate…onDestroy` + `onSaveInstanceState` | `attach` / `resize` / `detach` / `draw` | **`onStart/onStop/onDestroy`**; `attach/detach` stay internal |
| Readiness | `MapView.OnReadyListener.onMapViewReady()` | `MapLoadedCallback` | `OnMapReadyCallback.onMapReady` | `OnMapReadyCallback.onMapReady` | none (only `frame_settled`) | **`MapView.OnReadyListener.onMapReady(mapView)`** |
| Camera value | `MapCamera` | `CameraOptions` / `CameraState` | `CameraPosition` | `CameraPosition` | `MapCamera` | **`MapCamera`** (keep) |
| Apply camera | `mapView.camera.lookAt(...)` | `mapboxMap.setCamera(CameraOptions)` | `moveCamera(CameraUpdate)` | `map.animateCamera(CameraUpdate)` | `new MapCamera(...)` then `draw(camera)` | **`mapView.setCamera(camera)` / `animateCamera(camera, durationMs)`** |
| Fit bounds | `MapCameraUpdateFactory.newLookAt(GeoBoundingBox)` | `cameraForCoordinateBounds(...)` | `CameraUpdateFactory.newLatLngBounds` | `getCameraForLatLngBounds` | `MapCamera(bounds, viewport, insets, maxZoom)` constructor | **`mapView.fitBounds(bounds, edgeInsets, maximumZoom)`** |
| Camera getters | `mapView.camera` | `mapboxMap.cameraState` | `map.getCameraPosition()` | `map.getCameraPosition()` | `MapCamera.center()/zoomLevel()/bearingDegrees()/pitchDegrees()` | **`mapView.camera` (value) with those getters** |
| Marker | `MapMarker` | `PointAnnotationManager` | `Marker` (`GMSMarker`) | `PointAnnotationManager` | `MapOverlayDescriptor(kind = PointMarkers)` | **`Marker`** |
| Polyline | `MapPolyline` | `PolylineAnnotationManager` | `Polyline` | `LineAnnotationManager` | `MapOverlayDescriptor(kind = RoutePolyline)` | **`Polyline`** |
| Polygon | `MapPolygon` | `PolygonAnnotationManager` | `Polygon` | `FillAnnotationManager` | `MapOverlayDescriptor(kind = FilledPolygon)` | **`Polygon`** |
| Overlay collection | `mapView.mapScene.addMapPolyline(...)` | `mapboxMap.annotations` | `googleMap.addMarker(...)` | `map.addMarkers(...)` | `setOverlays(list)` / `clearOverlays()` | **`mapView.addOverlay(...)`, `removeOverlay(handle)`, `clearOverlays()`** |
| Location puck | `LocationIndicator` | `LocationPuck2D/3D` | `MyLocation` layer | `LocationComponent` | `set_location_indicator` / `clear_location_indicator` | **`mapView.locationIndicator.show(...)` / `.hide()`** |
| Search | `SearchEngine` | `SearchEngine` (`mapbox-search-android`) | Places SDK (`PlacesClient`) | Places plugin | `MapSdk.forwardGeocode` / `searchPlaces` / `searchAutocomplete` | **`SearchEngine.search(query, boundary)` / `suggest(query)` / `reverseGeocode(coordinate)`** |
| Routing | `RoutingEngine.calculateRoute(with:options:)` | `MapboxDirections` | Directions API | — (none on-device) | `MapSdk.calculateRoute(waypoints, options)` | **`RoutingEngine.calculateRoute(waypoints, options)`** |
| Distance matrix | `RoutingEngine.calculateRoute` overload | Matrix API | Distance Matrix API | — | `MapSdk.calculateDistanceMatrix` | **`RoutingEngine.calculateDistanceMatrix(origins, destinations, mode)`** |
| Traffic | `TrafficEngine` | — | — | — | `MapSdk.fetchTrafficIncidents` / `fetchTrafficFlow` | **`TrafficEngine.incidents(bounds)` / `.flow(bounds)`** |
| Navigation | `Navigator` / `VisualNavigator` | `MapboxNavigation` / `NavigationMapView` | Navigation SDK (`NavigationView`) | — | `MapNavigationController` | **`Navigator`** |
| Start navigation | `navigator.startRoute(...)` | `navigation.startTrip(...)` | `NavigationLauncher` | — | `start_trip(route, ...)` | **`navigator.startNavigation(route, options)`** |
| Stop navigation | `navigator.stop()` | `navigation.stop()` | — | — | (none) | **`navigator.stopNavigation()`** |
| Location feed | `LocationEngine` → updates | `location` provider | — | `LocationComponent` | `provide_location_update(update)` | **`navigator.updateLocation(update)`** |
| Reroute | `RoutingEngine` + `Navigator` callbacks | `NavigationRoute` refresh | — | — | `provide_reroute` / `begin_reroute_calculation` | **`navigator.applyReroute(route)`**; gate stays internal |
| Navigation events | `NavigatorListener` | `NavigationListener` | `NavigationListener` | — | `NavigationStateEventListener` + 4 `subscribe_to_*` | **`NavigationListener` + `navigator.addNavigationListener { }`** |
| Subscription handle | `NavigatorListener` remove | `Cancelable` | `ListenerRegistration` | `Cancelable` | `NavigationTopicSubscription.cancel()` | **`Subscription.cancel()`** (keep shape) |
| Offline downloads | `MapDownloader` / `MapUpdater` / `OfflineSearchEngine` | `OfflineManager` / `OfflineRegion` | — | `OfflineManager.createOfflineRegion` | `register_mapquest_providers_with_tile_cache` + `set_raster_tile_cache_time_to_live` | **`OfflineManager.tileCacheTerms()` / `.setTileTimeToLive(seconds)`**, **`.regionTileCount(region)` / `.downloadRegion(region, listener)` / `.cancelRegionDownload(id)` / `.regionRecords()` / `.regionRecord(id)` / `.deleteRegion(id)`** |
| Offline progress | `MapDownloader` progress | `OfflineRegionObserver.onStatus` | — | `OfflineRegionObserver` | `TileFrameStatistics` only | **`RegionDownloadProgressListener.onRegionDownloadProgress(progress)`** (`completedTileCount` / `totalTileCount` / `networkFetchedTileCount` / `diskServedTileCount`) |
| Provider credential | `SDKOptions(AuthenticationMode.withKeySecret)` | `MapboxOptions.accessToken` | manifest API key | style URL | `register_mapquest_providers(key)` | **`maps.configureProviderSet(providerKey, credential)`** (every capability one provider serves) or **`maps.configureProvider(contract, providerKey, credential)`** (one capability) |
| Provider instance | engine constructor injection | `MapboxOptions` | `MapsInitializer` | `MapLibre.setHttpClient` | `register_*_provider_from_foreign(...)` | **`maps.configureProvider(contract, providerKey, credential)`** for a built-in key; a foreign object uses the matching `register_*_from_foreign` call. The `maps.providers.register(provider)` façade is not landed |
| Base-map / style | `mapScene.loadScene(MapScheme)` | `loadStyle(StyleURI)` | `GoogleMap.setMapType` | `setStyle(url)` | (raster/vector tile source on the canvas) | **`mapView.setStyle(MapStyle)` + `maps.tileProviders.setRasterSource(source)`** |
| Diagnostics | — | `MapboxMaps` logging | — | — | `subscribe_to_diagnostic_events` / `flush_recent_diagnostic_events` | **`maps.diagnostics.addListener(listener, minimumSeverity)` / `.flushRecent(maximumCount)`** |
| Errors | typed `SDKError` | `MapboxMap` throws | logs + callbacks | throws | `SdkError` | **`SdkException`** (keep the UDL type name `SdkError`) |

Coordinates, tiles, camera value and progress records are already better named
than most vendors' (`GeographicCoordinate` beats `LatLng`, `Point`,
`CLLocationCoordinate2D`). Everything above the record layer — the root object,
the view, the navigation controller and the wasm names — is where the v2 names
apply.

### Naming rules

- **R1 — Noun-first types.** `MapView`, `MapCamera`, `MapStyle`, `SearchEngine`,
  `RoutingEngine`, `Navigator`, `OfflineManager`. Never `MapViewHost`,
  `MapSurfaceHost`, `CanvasManager`.
- **R2 — One concept, one word, everywhere.** Pick *view*, *camera*, *marker*,
  *polyline*, *polygon*, *route*, *navigation*, *navigator*, *region*,
  *provider*, *credential*, *listener*, and never alternate with *surface*,
  *scene*, *puck*, *trip*, *subscription* (as a noun for the thing you listen
  to).
- **R3 — Engine = noun + verb methods.** `SearchEngine.search`,
  `RoutingEngine.calculateRoute`, `Navigator.startNavigation`,
  `OfflineManager.downloadRegion`.
- **R4 — Approved verbs.** `search`, `suggest`, `reverseGeocode`,
  `calculateRoute`, `calculateDistanceMatrix`, `startNavigation`,
  `stopNavigation`, `updateLocation`, `setCamera`, `animateCamera`, `fitBounds`,
  `addOverlay`, `removeOverlay`, `clearOverlays`, `configureProviderSet`,
  `configureProvider`, `providerRegistrations`, `register`, `downloadRegion`,
  `deleteRegion`, `addListener`, `removeListener`, `cancel`.
- **R5 — Booleans read as questions.** `isFrameSettled`, `isNavigationActive`,
  `isOffline`. Never `frame_settled` on a foreign surface.
- **R6 — Events are `on<PastParticiple>` on a listener interface.**
  `onMapReady`, `onCameraChanged`, `onInstruction`, `onSpeechInstruction`,
  `onDeviationChanged`, `onPhaseChanged`, `onArrival`,
  `onError`.
- **R7 — Units live in the name, spelled out.** `_meters`, `_seconds`,
  `_pixels`, `_degrees`, `_bytes`. Keep `distanceRemainingMeters`,
  `durationRemainingSeconds`; never `ms`, `px`, `deg` in identifiers (prose may
  abbreviate).
- **R8 — No vendor in a public name.** `registerMapquestProviders` bakes
  MapQuest into every integrator's call site and contradicts provider
  genericity. Vendor names belong in provider crates and the registry descriptor
  only.
- **R9 — No platform/internal leakage.** `Surface`, `Gpu`, `Canvas`,
  `NativeWindow`, `Pointer`, `Descriptor` never appear in a name a developer
  types. `MapSurfaceTargetHandle` carries `ANativeWindow*` / `UIView*` and is
  internal to the wrapper.
- **R10 — No transport-mode suffixes.** `_live` (`forwardGeocodeLive`,
  `calculateRouteLive`, `fetchRasterTileLive`) describes the harness's transport
  choice, not the SDK operation. Live/recorded is a provider registration, never
  a method name.
- **R11 — snake_case only inside Rust/UDL.** UniFFI camelCases UDL members for
  Kotlin, Swift and RN. The wasm surface does not: wasm-bindgen preserves Rust
  names unless each export carries `#[wasm_bindgen(js_name = ...)]`. Every wasm
  export must set `js_name`.
- **R12 — Abbreviations.** `SDK`, `URL`, `HTTP`, `GPS`, `MVT` allowed in prose,
  never as identifier shorthand (`docs/DocsStyleGuide.md` rules this). `Gpu` is
  an implementation word, not an abbreviation the API needs — drop it.
- **R13 — Values are immutable, verbs live on the owning object.** `MapCamera`
  is a value (matches Mapbox `CameraOptions`); change the camera by telling the
  view (`setCamera`), not by mutating the camera.
- **R14 — Prefix scope by plane, not by vendor.** `Map*` for things drawn on the
  map (`MapView`, `MapCamera`, `MapStyle`, `Marker`, `Polyline`, `Polygon`),
  `Navigation*` for trip/session data (`NavigationState`, `NavigationListener`,
  `NavigationPhase`). Do not mix (`MapNavigationController` is both).

Never rename: `GeographicCoordinate`, `BoundingBox`, `RouteStep`, `RouteLeg`,
`RouteOptions`, `AvoidOptions`, `NavigationPhase`, `SdkError`, `MapCamera`.

### Create and lifecycle

Each target constructs the same surface differently. Kotlin builds
`MapView(context, maps)` as a `FrameLayout` and forwards `onStart` / `onStop` /
`onDestroy`; Swift builds `MapView(frame: .zero, maps:)` and has no lifecycle
forwarding, because readiness is the only gate; React Native constructs
`new MapView(maps)` and binds the native host view to it; wasm constructs
`new MapView(canvas, maps)` with async init inside and awaits `mapView.ready`.
Readiness is `MapView.OnReadyListener.onMapReady(mapView)` everywhere.
`configureProviderSet("mapquest", credential)` throws `SdkException` verbatim.
The whole-set call registers every capability the named provider serves from the
one credential it issued; `configureProvider(contract, providerKey, credential)`
chooses one capability independently.

### Choosing a provider

`configureProvider(contract, providerKey, credential)` is provider-neutral by
design: the contract name selects the capability slot, the key names a provider
the registry knows, and the credential is whatever that provider issued.
`configureProviderSet(providerKey, credential)` fills every capability the named
provider serves from a single credential.

| `providerKey` | Serves | Credential |
|---|---|---|
| `mapquest` | geocoding, routing, distance matrix, place search, traffic incidents, traffic flow, raster tiles | a MapQuest API key |
| `here` | geocoding, routing, traffic incidents, traffic flow, vector tiles | a HERE API key |
| `google` | geocoding, routing | a Google Maps API key |
| `tomtom` | geocoding, routing, traffic incidents, traffic flow, raster tiles | a TomTom API key |
| `mapbox` | geocoding, routing, vector tiles | a Mapbox access token |
| `radar` | geocoding, routing | a Radar API key |
| `osrm` | routing | the deployment's base URL (empty for a local server) |
| `valhalla` | routing | the deployment's base URL (empty for a local server) |
| `photon` | geocoding, place search, type-ahead | the deployment's base URL (empty for the public instance) |
| `openstreetmap` | raster tiles | none (some sources want a User-Agent) |
| `osmus`, `openfreemap` | a styled vector base with labels | none |

The final three rows are base-map source declarations in the registry rather than
callable provider keys. A keyless base map draws, and the keyless providers
answer their contracts without a credential. Adding a provider is a provider
crate, a registry descriptor and a dispatch row; the calls above do not change.
`providerRegistrations()` reports, in a stable order, which key serves each
capability, and every reference app's settings surface renders its rows from the
same registry. The wasm seam serves the construction half: `await
MapsSdk.create()` resolves to the SDK root (`maps.compiledVersion`), `new
MapView(canvas, maps)` binds the canvas to it (`mapView.sdkCompiledVersion`), and
the optional third argument (`new MapView(canvas, maps,
TileResolution.Retina)`) pins tile resolution for capture harnesses; omitted, it
follows the `devicePixelRatioThreshold()` rule. `initializeSdk()` stays as the
readiness alias for existing harness code.

### Camera

The camera verbs are `mapView.camera`, `setCamera(camera)`,
`animateCamera(camera, durationMs)`, `fitBounds(bounds, edgeInsets,
maximumZoom)` and `cameraGeographicBounds()`. A gesture cancel stops an
animation.

The UDL carries four `MapView` camera members and the `MapCamera` constructors.
They are additive: no existing camera, gesture or draw member changes, and the
generated Kotlin, Swift and React Native bindings carry them. The Android, iOS
and React Native views hold the contract `MapCamera` as their only camera state:
a set, a gesture, a fit and every animation step hand the contract value to the
native view and report its readback, and an animation frame is one
`MapCamera.interpolate` call. The wrapper's camera value type is a projection of
that readback, not a second state store. The hand-maintained WebAssembly
TypeScript façade carries the same members. The camera-change notification
remains a callback (`onCameraChanged` on Android and React Native,
`mapView(_:didChangeCamera:isGestureDriven:)` on iOS) rather than a subscription.

| Member | Returns | Behavior and failure modes |
| --- | --- | --- |
| `MapView.camera()` | `MapCamera` | The pose the view holds. Starts at latitude 0, longitude 0, zoom 2, north-up and top-down; every `set_camera`, `fit_bounds` or accepted gesture transform updates it. The view stores its own pose so a wrapper reports the same value it hands to `draw`; drawing still receives the pose as an argument. Cannot fail. |
| `MapView.set_camera(camera)` | nothing | Adopts a validated pose and cannot fail. The constructor clamps zoom and pitch and wraps the bearing, so adopting never changes an overlay. |
| `MapView.fit_bounds(bounding_box, edge_insets, maximum_zoom_level)` | `MapCamera?` | Frames the box inside the attached viewport's inset region, top-down and north-up, at the largest zoom keeping the whole box inside the insets and clamped to the maximum; the center anchors on the inset region's center. Answers the adopted camera, or null when no surface viewport is attached (nothing changes). Throws `InvalidCoordinateInput` when the box is not canonical, the insets are negative or leave no inset area, the maximum zoom is not finite, or the box spans no positive world area. A refused fit keeps the previous camera. Uses the attached viewport instead of taking one. |
| `MapView.camera_geographic_bounds()` | `BoundingBox?` | Unprojects the four viewport corners through the current camera. Answers null when no surface viewport is attached or the projection cannot answer. At a steep pitch, when fewer than three corners reach the ground, the camera's own ground scale bounds a box around its center instead. |
| `MapCamera.interpolate(start_camera, target_camera, fraction)` | `MapCamera` | The one camera-interpolation curve. Clamps `fraction` into `[0, 1]` and eases with smoothstep, moves the center along the shortest longitude path in Mercator space, interpolates zoom and pitch linearly and takes the shortest bearing arc. At or below zero it answers `start_camera` verbatim; at or above one, `target_camera` verbatim, so animation endpoints are exact. Throws `InvalidCoordinateInput` when `fraction` is not finite. |

The UDL spells these in snake_case; UniFFI generates `camera`, `setCamera`,
`fitBounds` and `cameraGeographicBounds` on the native surfaces, and
`MapCamera.interpolate` keeps its name.

The browser façade maps the same members onto the canvas view. The browser has no
attached platform surface, so the viewport and the vertical field of view are
explicit arguments, exactly as in `MapCamera.groundMetersPerScreenPixel` and the
projection methods. `mapView.camera()` answers the one stored pose;
`mapView.setCamera(camera)` adopts a validated pose and borrows the camera, so
the caller keeps its instance;
`mapView.fitBounds(southWestLatitude, southWestLongitude, northEastLatitude,
northEastLongitude, widthPixels, heightPixels, insetTopPixels, insetRightPixels,
insetBottomPixels, insetLeftPixels, maximumZoomLevel)` answers the adopted
camera or null before the view initializes, and a refused fit keeps the previous
camera and throws `InvalidCoordinateInput`;
`mapView.cameraGeographicBounds(widthPixels, heightPixels, fieldOfViewDegrees)`
reads the stored pose's geographic box with the ground-scale fallback at steep
pitch; and `MapCamera.interpolate(startCamera, targetCamera, fraction)` is the
static projection of the same curve, with exact endpoints and the shortest
bearing arc. `renderFrame(camera, fieldOfViewDegrees)` keeps its per-frame camera
argument. A rendered frame is authoritative: it updates the stored pose
`camera()` reports.

### Overlays, gestures and location

`Marker` / `Polyline` / `Polygon` are binding-layer types converting to the
existing `MapOverlayDescriptor` list in one `setOverlays` call — the
single-descriptor-with-nullable-payload record stays inside the FFI, never in a
developer's hands. The binding calls are `addMarker`, `addPolyline`,
`addPolygon`, `removeOverlay` and `clearOverlays`.

The id-addressed members are on the UDL as `MapView.add_overlay` /
`update_overlay` / `remove_overlay` (Kotlin `addOverlay` / `updateOverlay` /
`removeOverlay`). `add_overlay(overlay)` answers a generated opaque identifier
(`overlay-1`, a monotonic per-view sequence never reused, even across
`clear_overlays`/`set_overlays`); validation is `set_overlays`' all-or-nothing
rule for the single descriptor, so a refused descriptor throws and adds nothing.
`update_overlay(id, overlay)` replaces the addressed overlay in its draw
position; an invalid descriptor throws and keeps the previous overlay, and an
unknown id throws a typed `SdkError` with a precise detail. `remove_overlay(id)`
answers whether the id was present (unknown is not an error), and
`clear_overlays()` clears both the bulk list and every addressed overlay.
`set_overlays` replaces the whole drawn list — including addressed overlays —
and drops the addressed bookkeeping, so an earlier id no longer addresses
anything.
Overlay state, the addressed set and the identifier sequence outlive
attach/detach.

The location layer follows the same ownership: `MapView.set_location_indicator`
takes a state and a style, `clear_location_indicator` removes it, and a null
style field resolves to its documented default. Validation is all-or-nothing, and
the state outlives attach/detach. Gestures switch through
`mapView.gestures.isPanEnabled`, `isZoomEnabled`, `isRotateEnabled` and
`isTiltEnabled`, and `MapView.InteractionListener` reports `onPanBegin`,
`onPan` and `onPanEnd`.

The three `MapCamera` gesture members (`pan_by_screen_pixels`,
`zoom_by_levels_around_screen_point`, `rotate_by_degrees_around_screen_point`)
carry this algebra in the contract, so all four shells compute identical results;
the reference apps route their pan/pinch/rotate math through them.

### Capability facades: search, routing, navigation

`SearchEngine(maps)` exposes `search(query, boundary)`, `suggest(query)` for
type-ahead and `reverseGeocode(coordinate)`. `RoutingEngine(maps)` exposes
`calculateRoute(waypoints, RouteOptions(...))` and
`calculateDistanceMatrix(origins, destinations, TransportationMode.DRIVING)`.
`Navigator(maps)` exposes `startNavigation(route, NavigationOptions(...))`,
`updateLocation(locationUpdate)`, `applyReroute(replacementRoute)`,
`stopNavigation()` and `addNavigationListener(listener)`, which returns a
`Subscription` whose `cancel()` is idempotent. The listener has four methods:
`onInstruction`, `onSpeechInstruction`, `onDeviationChanged` and
`onPhaseChanged`.

One listener with four methods replaces the four `subscribe_to_*` calls (and
matches HERE's `NavigatorListener` / Mapbox's `NavigationListener`). The four
topics remain inside the implementation: a listener that implements only
`onInstruction` still must not receive speech on that method.

`Navigator.add_navigation_listener` and `Navigator.stop_navigation` are on the
UDL. The listener is subscribed to all four topics in one call, and the returned
composite `Subscription`'s `cancel()` removes all four registrations
(idempotent). `stop_navigation()` ends the active trip and clears trip state —
`current_trip_state`/`current_instructions` answer null, and later fixes,
reroutes and reroute-request signals are dropped until `start_navigation` begins
a new trip. It resets the speech-trigger band memory and the
reroute-recalculation gate with the trip (utterance ids stay monotonic; the
configured reroute policy persists) and leaves listener subscriptions untouched.
No stop event is dispatched: the native reducer and the frozen phase channel have
no stop transition, so inventing one would be semantics the reducer does not own.

Traffic is served today by `MapSdk.fetch_traffic_incidents` and
`MapSdk.fetch_traffic_flow`, both taking a `BoundingBox`; the `TrafficEngine`
form in the naming table is the naming target, not a separate contract interface.

### Offline downloads

`OfflineManager(maps)` exposes `tileCacheTerms()` and
`setTileTimeToLive(timeToLiveSeconds)`; the provider's declared terms ceiling
still clamps what the cache stores, and the readback reports the effective value.

| Member | Shape |
|---|---|
| `OfflineManager.region_tile_count(region)` | synchronous count of covered tiles, so a settings surface can warn before a large download |
| `OfflineManager.download_region(region, progress_listener)` | async; returns the final `RegionRecord` |
| `OfflineManager.cancel_region_download(region_id)` | signals an in-flight download; idempotent and a no-op for an unknown or finished region |
| `OfflineManager.region_records()` | async; every region stored on disk |
| `OfflineManager.region_record(region_id)` | async; one region, or `null` when the id is unknown |
| `OfflineManager.delete_region(region_id)` | async; removes the region and returns a `RegionRemovalReport` |

`set_tile_pack_byte_budget(byte_budget)` reserves payload bytes for tile packs
beside the general LRU cache, and `tile_pack_byte_budget()` reads the effective
reservation. A download that would take its pack past the budget stops, keeps
the stored tiles and their membership, and reports `CacheOperationFailure`; a
later call under a raised budget resumes the remaining tiles. The budget is its
own reserve, so ordinary cache eviction never frees a present pack tile, and the
structural 65,536-tile cap remains the only tile-count limit.

The records are `RegionDefinition` (region id, display name, bounding box,
minimum and maximum zoom level, image format, tile resolution, optional requested
retention), `RegionRecord` (the same shape plus effective retention, member tile
count, present tile count and present payload bytes), `RegionDownloadProgress`
(region id, total and completed tile counts, network-fetched and disk-served
split, and a `RegionDownloadPhase`), `RegionDownloadPhase` (`Downloading`,
`Completed`, `Cancelled`, `Failed`) and `RegionRemovalReport` (removed entry count
and removed payload bytes). `RegionDownloadProgressListener` follows
`[Trait, WithForeign]`; its `on_region_download_progress` runs on an SDK worker
thread and must marshal to the platform UI thread. A cancelled download is a
successful partial result: the terminal phase is `Cancelled` and the returned
record's present count is below its member count, so a later download resumes
from the tiles already on disk. A region whose maximum zoom level is deeper than
the installed provider's declared raster ceiling is refused with
`InvalidCoordinateInput`; the contract adds no cancellation token and no new
`SdkError` variant. The WebAssembly surface has no disk pack store: the browser
binds no `TileCacheInstallation`, so the TypeScript façade neither declares nor
serves these members.

Routing from a compiled archive is a contract function on every native surface —
archive bytes in, a `Route` out — sharing one `rustic-maps-core` implementation
with the wasm `offlineRouteFromArchiveBytes` export. The contract member is
`offlineRouteFromArchiveBytes(archiveBytes, start, end)`; the request uses the
archive profile's default options (automobile, fastest, metric), and a corrupt
archive throws `OfflineRoutingFailure`. The engine-shaped façade
(`OfflineRoutingEngine.fromArchiveBytes`) remains the v2 target, not a
requirement for the capability.

### Tile and base-map registration

A raster source is declared with `registerUrlTemplateRasterTileSource(source)` or
selected per capability with `configureProvider(ProviderContract.RasterTiles,
"openstreetmap", "")`; a vector base is installed with
`setVectorTileSource(source)`. A raster provider or credential change reaches the
map through `reloadTileSource()`, which re-resolves the provider and drops the
scheduler's pooled tiles without an attach/detach cycle; the camera, overlays and
location layer keep their state. The browser target serves the same
per-capability registry through `configureProvider`, `providerRegistrations`,
`configureProviderSet(providerKey, credential)` and
`clearProviderRegistrations`; the older `registerProvider` / `clearProvider`
names are gone.

A vector base can also be served by application code. Implement
`VectorTileProvider` — `sourceDescriptor()` describes the tileset and
`fetchVectorTile(tileX, tileY, zoomLevel)` returns its bytes — and register it
with `registerVectorTileProviderFromForeign(provider)`. The SDK selects it under
the application key `application`, so a provider-backed base names it in
`ProviderVectorTileSource(providerKey = "application", styleId = …)`.
`vectorTileProviderIsRegistered()` reports whether the slot is filled. The
URL-template source record carries the tileset's terms text, `{s}` subdomains,
per-request headers and style hints beside the template; null or empty fields
declare none.

`setProviderVectorTileSource` installs a base from a provider configured for the
vector-tiles capability; `setProviderVectorTileSourceFromDocument` fetches that
provider's published style document through the provider's own authenticated
client, so the credential never leaves the provider layer and the request carries
its transport, pacing and retry policy. A compiled document is kept per provider
and address, so installing the same family again costs no request. A document
that fails to fetch, decode or compile throws and leaves the previous base in
force.

`vectorTileSourceStatus()` reports what the base is drawing with and what its
last resolved frame achieved. The same record crosses every target; the browser
spells each field in lower camel case.

| Record field | WebAssembly field | Meaning |
|---|---|---|
| `is_configured` | `isConfigured` | Whether a vector base is installed. |
| `style_id` | `styleId` | The installed style id. |
| `source_name` | `sourceName` | The served tileset's name. |
| `attribution_text` | `attributionText` | The attribution line the terms require. |
| `maximum_zoom_level` | `maximumZoomLevel` | The highest zoom the tileset serves. |
| `drawn_tile_count` | `drawnTileCount` | Tile addresses the last frame drew. |
| `failed_tile_count` | `failedTileCount` | Tile addresses whose fetch or decode failed. |
| `last_failure_detail` | `lastFailureDetail` | The last tile failure's detail; null when none. |
| `unsupported_property_count` | `unsupportedPropertyCount` | Every style property or layer kind the renderer could not apply. |
| `unsupported_property_names` | `unsupportedPropertyNames` | The distinct unsupported properties, scope-qualified and sorted; at most 32 names. |
| `degraded_asset_names` | `degradedAssetNames` | Assets that fell back (a declared sprite, glyph endpoint or undrawable source), sorted; at most 16 names. |
| `source_layer_counts` | `sourceLayerCounts` | Per-source `matched_layer_count` and `drawn_layer_count` for the last frame, sorted by source id. |

The two name lists are bounded so a large vendor document cannot grow the status
without bound; the count still reports every diagnostic. A document whose
declared vector source names a tileset the provider does not serve is refused at
install, naming the declared source and the served tileset, rather than painting
fragments.

### Readiness and errors

`mapView.setOnReadyListener(...)` reports that the map can draw and project.
`maps.diagnostics.addListener(listener, SdkLogSeverity.WARNING)` replaces
`subscribe_to_diagnostic_events`; `flushRecent(listener, severity,
maximumCount)` replays buffered events. Keep the typed-exception model (Mapbox
does the same) and do not invent string-based error callbacks (Google's model);
`SdkException` carries the typed, verbatim detail. Readiness is the only
callback.

### Ownership split and the frame pump

The shipped SDK wrapper owns surface acquisition and lifetime, the frame pump,
resize, the camera value store and animation, gesture-to-camera deltas, overlay
bookkeeping by id, readiness and error derivation, and pointer-to-coordinate for
taps. The platform application owns widgets, sheets, navigation UI, TTS and the
provider credentials UI.

The frame pump lives in the platform wrapper (Kotlin/Swift), never in Rust. The
contract is explicit that there is no SDK render loop (`rustic_maps.udl` states
"never an SDK-internal loop"); moving it into Rust would require a platform
callback into FFI, i.e. a new foreign-callback interface and a threading story
for UIKit/Choreographer — the opposite of a simplification. The wrapper is also
the only place that can observe window attachment and pause.

**Android** uses `Choreographer.postFrameCallback` + a `Channel<Long>(CONFLATED)`
+ one consumer coroutine. Copy the proven RN shape
(`ReferenceMapController.kt:744-778`) rather than the Compose `withFrameNanos`
one, so the same code serves XML and Compose hosts. Post from
`onAttachedToWindow`; stop on `onDetachedFromWindow`,
`onWindowVisibilityChanged != VISIBLE`, or `setRenderingPaused(true)`. **iOS**
uses `CADisplayLink` on `.main`/`.common`, started in `didMoveToWindow` when
`window != nil`, invalidated when `nil`, plus `setRenderingPaused`.

Per tick, in order (all already validated in-tree):

1. extent check → `resize` when the capped pixel extent changed (never every
   frame);
2. `frame_settled(camera)` — pure query; throws only when detached
   (`udl:1468-1477`);
3. if settled → pause the pump; else `draw` with at most one in flight
   (`SdkException.RenderingSurfaceUnavailable` → retry next tick);
4. report statistics / ready / error.

Because `draw` performs its fetch phase inside the call (the await spans
network), `frame_settled == true` means nothing is pending, so the pump may stop
entirely rather than keep ticking. On settle:
`Choreographer.removeFrameCallback` / `displayLink.isPaused = true`. Resume on:
camera set/animate, gesture delta, overlay mutation, location-indicator change,
surface created/changed, window attach / visibility /
`setRenderingPaused(false)`, `retryAttach`, and a ~1 s watchdog tick. Keep
ticking while `frame_settled == false`; if statistics report `deferred == 0` and
`unavailable > 0` with an unchanged pose for N consecutive frames, drop to a 1 s
retry cadence — otherwise a permanently failing provider becomes a 60 fps spin
against the network (the scheduler retries `Unavailable` tiles every frame:
`rustic-maps-rendering/src/tile_scheduler.rs:11-12, :34-35`). The app must still
do nothing; optionally call `setRenderingPaused(true)` in
`onStop`/`scenePhase == .background` to release the GPU surface early, or
`retryAttach()` after `onError`.

### Device pixel ratio

The committed styles declare point/CSS sizes while the surface draws device
pixels. The wrapper reports the drawing buffer's device pixels per point with
`MapView.set_device_pixel_ratio` whenever it applies the drawable scale — at
attach, after a resize and on a rebuilt native view. The SDK multiplies placed
glyph sizes, overlay line widths and marker/pin radii by that ratio exactly once,
so labels and overlays keep their declared point size on a 1×, 2× or 3×
surface.
Android reports `resources.displayMetrics.density` (1.0 until the display
resolves); iOS reports its capped drawable scale (`drawable pixels / point`). The
wasm canvas derives its own ratio from `canvas.width / clientWidth` and carries
no setter, so the member stays native-only.

### Native view construction

**Android: the surface pointer shim ships inside the AAR.** Android exposes no
public Kotlin API for a `Surface`'s `ANativeWindow*`, so every integrator would
otherwise hand-write a three-line C shim and a JNI declaration. The SDK ships it:
the crate builds the Android `cdylib` (`librustic_maps.so`), so a
`cfg(target_os = "android")` module with `jni` and `ndk-sys` dependencies exports
`Java_com_angoratek_rusticmaps_map_NativeWindowShim_acquireNativeWindow`, and
the AAR's Kotlin object loads it with `System.loadLibrary("rustic_maps")` (the
same library the UniFFI JNA bindings load). A fixed class/package name makes the
mangled symbol a compile-time constant; `RegisterNatives`/`JNI_OnLoad` buys
nothing here, and a consumer ProGuard keep rule covers obfuscation. Release the
window on every failure path: both current app-side hosts leak one
`ANativeWindow*` reference per failed attach.

**iOS: the view owns its own layer pointer.** `MapView: UIView` with `layerClass
== CAMetalLayer` hands Rust `Unmanaged.passUnretained(self).toOpaque()` from
inside the view, so user code never does pointer math. It must also implement
`init?(coder:)` (the current app-side view `fatalError`s, so it cannot be used
from a Storyboard or XIB); cap the drawable at the platform's texture limit
(`max_texture_dimension_2d` is 2048 on downlevel adapters — an unguarded
full-screen buffer on a tall phone is rejected); and detach in `dismantleUIView`
/ `didMoveToWindow(nil)`, never in `deinit` (the Rust side must release its
surface while the layer still exists).

**React Native: one SDK object, owned by the package.** The generated JSI
bindings construct their own `MapSdk`, and there is no way to import a foreign
handle into them. A native `<MapView>` therefore uses the SDK object the package
owns and proxies map calls through its module — otherwise two registries (and
two SQLite tile caches) exist in one process. Navigation does not need that
handle: the generated `Navigator` creates its own native navigation object and
exposes the contract's `NavigationListener` (`onNavigationInstruction`,
`onNavigationPhaseChange` and the other topics), so JavaScript drives a trip
directly. The curated `MapsSdk` proxy deliberately does not duplicate this
surface; its provider-scoped members (distance matrix, traffic, credentials)
stay on the proxy because they need the shared provider registry.

### Device positioning

The contract carries one positioning controller. `PositionFix` holds a
`GeographicCoordinate coordinate`, an optional `horizontal_accuracy_meters`, an
optional `course_heading_degrees` and an optional `timestamp_seconds`.
`PositionSourceError` has four values: `PermissionDenied`, `ServiceDisabled`,
`Unavailable` and `Timeout`. `PositionController` exposes `feed_fix(fix)`
(throwing `SdkError`), `source_failed(error)`, `latest_fix()`,
`smoothed_course_degrees()`, `is_stale(now_seconds)` and `source_status()`.

The host owns the operating system's location API and its permission prompt, and
pushes fixes and failures into the controller. The controller validates each fix,
ignores stationary repeats when averaging course, smooths the course around the
circle, tracks staleness and records source failures, so the renderer's location
indicator, a follow camera and the `Navigator` read one consistent stream. A
controller built before any fix answers `latest_fix` as null; an invalid fix
throws `SdkError::InvalidCoordinateInput` with the validator's detail and leaves
the previous state in force. The wasm seam exposes the same class (`feedFix`,
`sourceFailed`, `latestFix`, `smoothedCourseDegrees`, `isStale`, `sourceStatus`);
the browser demo's Location settings use it with `navigator.geolocation`.

## 5. The provider layer

### What everymap-rs owns

One provider abstraction serves both projects. `everymap-rs` keeps its own domain
API for applications that never render a map; `rustic-maps-sdk` consumes the same
crates for routing, geocoding, search, traffic, raster and vector tiles, so
provider knowledge is written once. The SDK's UniFFI UDL, reference applications,
package identities and provider-name guard are unchanged by the switch.

The SDK pins `everymap-rs` **0.4.7** (`Cargo.lock`), which publishes
`everymap-core`, eleven provider crates and a CLI: HERE, Google, TomTom, Mapbox,
Radar, MapQuest, Photon, OSRM, Valhalla, URL-template raster and URL-template
vector.

1. `everymap-core` owns the contracts and the plumbing: transport seam, pacing,
   retry, observation, credentials and the domain traits.
2. `everymap-providers-*` owns vendor knowledge: MapQuest, OSRM, Valhalla, the
   URL-template raster and vector sources, and the OSM US/OpenFreeMap
   descriptors, beside the HERE/Google/TomTom/Mapbox/Radar crates.
3. `rustic-maps-sdk` depends on those crates directly: the registry and catalog
   answer with everymap provider instances, the cache wrappers wrap everymap
   traits, and the wasm bindings dispatch to the same providers through the
   browser transport.
4. The SDK keeps what is renderer- and product-specific: the UDL contract, the
   offline `RMOG` graph engine, the tile schedulers, the reference applications
   and the registry descriptors.

The switch retired the SDK's own credential, keyless-routing, tile-source and
transport crates. Photon and the OpenStreetMap US source live in the provider
layer, so one `reqwest` major and one crate family remain.

The provider layer was brought to parity in a fixed order: transport seam and
wasm build first; then navigation-grade routing and the missing domains; then the
vendors; then the release; then the SDK switch; then documentation and gates. The
0.2.2 release (MIT, MSRV 1.86) was the adoption baseline, with five vendor
provider crates (HERE, Google, TomTom, Mapbox, Radar) and ten domain traits. That
baseline lacked a `wasm32` build (the client was `reqwest 0.13` over `tokio` with
`features = ["full"]` and no target gating), a transport seam not bound to
`reqwest` types, pacing, retry, timeouts, request observation and credential
redaction, navigation-grade routing (legs, maneuvers, speech, lane guidance,
shape), typed raster tiles, a vector-tile trait with a source descriptor,
autocomplete, matrix, elevation, offline routing, route capabilities,
bounding-box traffic with flow geometry, and the MapQuest, OSRM, Valhalla, OSM US
and URL-template sources.

### Trait mapping

| This SDK's contract (`geospatial-traits`) | everymap-rs contract | Required upstream shape |
|---|---|---|
| `GeocodingProvider` forward/reverse | `Geocoder` | Result shape widened to the SDK's quality classes; options mapped |
| `SearchPlaceProvider` settled search | `Geocoder` with a boundary | Keep |
| `SearchPlaceProvider.search_autocomplete` | autocomplete trait | Present |
| `RoutingProvider` | `Router` | Ordered waypoints, legs, maneuver/speech/lane, shape, `route_capabilities` |
| `DistanceMatrixProvider` | distance-matrix trait | Present |
| `ElevationProvider` | elevation trait | Present |
| `TrafficIncidentProvider`, `TrafficFlowProvider` | `TrafficProvider` | Bounding-box queries, flow geometry and congestion level |
| `RasterTileProvider` | `TileProvider` | Typed format/resolution echoed in the response |
| `VectorTileProvider` | vector-tile trait | Source descriptor (encoding, attribution, zoom span, style hints) |
| `OfflineRoutingProvider` | offline-routing trait | Present; the `RMOG` engine stays in this repository |
| Registry, cache-then-network, tile cache | Above the traits | Unchanged shape; wraps everymap traits |

### Registry and catalog

The registry is one optional slot per provider contract. Each capability has an
independent slot, so a map may draw raster tiles from one provider, plan routes
with another and search with a third. A capability with no installed provider
answers calls with `ProviderNotConfigured`. There are ten contracts:
`Geocoding`, `Routing`, `DistanceMatrix`, `PlaceSearch`, `TrafficIncidents`,
`TrafficFlow`, `Elevation`, `RasterTiles`, `VectorTiles` and `OfflineRouting`.

`MapSdk.configure_provider_set(provider_key, credential)` installs every
capability a named provider serves. `MapSdk.configure_provider(contract,
provider_key, credential)` replaces exactly one slot.
`MapSdk.provider_registrations()` reports, in a stable order, which key and
display name serve each contract and whether it is registered; capabilities with
no provider carry an empty key and name and `registered` false.

`rustic-maps-providers` carries the one catalog table. The native catalog and the
browser dispatch both read it, and their tests compare their rows against it; the
registry test compares the descriptors in `platform/providers.json` against it.
The table carries no vendor URL, credential value or transport. Adding a provider
is one row there, one construction function in the binding's shared seam, a
provider crate and a registry descriptor. Nine keys are callable on native and
wasm: `mapquest`, `here`, `google`, `tomtom`, `mapbox`, `radar`, `osrm`,
`photon`, `valhalla`. The registry also carries `openstreetmap`, `osmus` and
`openfreemap`, which are URL-template base-map source declarations rather than
callable keys. Each descriptor records key, display name, credential label,
per-platform availability, attribution, terms, fixture routes, wasm registration,
raster/vector templates and style families.

### The bridge and the consolidation order

`rustic-maps-core` depends on everymap provider instances through the bridge in
`crates/rustic-maps-core/src/everymap_bridge.rs`. The adapters convert records
and errors only; they hold no vendor knowledge and name no provider. The
registry, cache wrappers and UDL provider callbacks still speak the SDK's own
contracts, with the bridge converting at the seam. A host provider supplied over
the UDL keeps its own foreign-trait seam.

Anything that restates a provider contract here is a temporary seam with a
deletion order, not a second abstraction:

| Duplication | Where | Verdict |
|---|---|---|
| Ten provider traits alongside everymap's sixteen contracts | `geospatial-traits` | **Delete.** The registry and consumers move to the everymap contracts; record conversion stays a function, not a trait. |
| Five vendor builders and credential parameters, written once per binding | native catalog, wasm dispatch | **Merge** into one crate both bindings consume (`rustic-maps-providers`); the bindings keep only their registration call. |
| Served-contract and display-name tables | native catalog, wasm dispatch, registry descriptor | **One table** in the shared crate, checked against the descriptor by a parity test. |
| Record adapters (`EveryMapGeocoder`, …) | `rustic-maps-core/src/everymap_bridge.rs` | **Keep only as free conversion functions** once the registry speaks everymap contracts. A host provider over the UDL keeps its own foreign-trait seam. |
| Provider tile knowledge | URL-template sources, vendor crates | Already single-sourced. |

The staged migration is: (1) the shared construction crate; (2) the registry on
everymap contracts; (3) the `register_*_from_foreign` callbacks keep an SDK-side
trait adapted beside the registry; (4) delete `geospatial-traits` and move its
mocks into the tests that use them (`geospatial-types` stays the record and math
layer); (5) a duplicate-abstraction gate fails a new `*Provider` trait outside
the UDL seam, and the provider-name guard continues to hold vendor knowledge to
the crates and descriptors. The UDL has nine `register_*_from_foreign` methods:
geocoding, raster tile, vector tile, routing, distance matrix, search place,
traffic incident, traffic flow and elevation.

### Provider-layer facts and caveats

- The build registers all nine callable keys on native and wasm; the two
  URL-template crates remain base-map sources.
- The wasm runtime clock was a real gap. The provider layer builds for `wasm32`
  but measured time with `std::time::Instant`, whose `now()` panics on that
  target; every browser provider path trapped the module. `everymap-core` now
  reads `web-time::Instant`, the same clock the SDK's transport uses.
- `TrafficFlow.speed` in the provider layer does not declare a unit, while the
  SDK field is kilometres per hour. The only provider the SDK ships does not
  report speeds, so no wrong value is published today; a second traffic provider
  reached through the bridge must report kilometres per hour until the upstream
  field gains a unit.
- The MapQuest port closed five parity gaps: pacing and retry configuration on
  the credential provider clients (`set_rate_limiter`, `set_retry_policy`,
  `set_request_timeout`, `set_observer`); an imperial unit choice
  (`RouteOptions::unit_system`); the nine geocoding quality levels the contract
  exposes (`SearchQuality` with the MapQuest mapping); a settled place-search
  trait beside `Geocoder` and `AutocompleteProvider` (`PlaceSearchProvider`);
  and incident coordinates on the core traffic record.
- The process-wide request observer (`install_global_request_observer`) lets the
  typed `ProviderRequestTrace` keep its facts without each provider crate
  changing. `TokenBucket::with_maximum_wait` restores the refusal after a
  ceiling, reported as `RateLimited`.
- 0.4.2 carried the bounded response excerpt and keyless request observers; 0.4.3
  the provider-correctness batch; 0.4.7 the redaction and release-integrity
  batch: redacted `Debug` for requests and the MapQuest configuration, a
  classified transport error that no longer prints a URL, a lock-free OAuth 2.0
  token fetch with a timeout, one shared Radar transport-mode table, TomTom
  traffic bounding boxes, and the documented Mapbox permanent and batch geocoding
  endpoints. The Google Routes field mask was corrected in the same line, which
  made live routing work once billing was enabled.

### Design constraints and open questions

- **Capability parity.** The everymap routing and traffic shapes were narrower;
  the SDK does not adopt a provider until its capability has a parity test, and
  the provider layer must land and be contract-tested before the switch.
- **Wasm first.** The browser target is the hard requirement; the transport seam
  and the wasm CI job come first, so every later piece is proven on wasm.
- **One abstraction generation.** During the transition both projects carry
  provider code. The SDK's crates are removed only after the everymap provider
  passes the same fixture tests.
- **Dependency alignment.** everymap pinned `reqwest 0.13`/`thiserror 1.0` while
  the SDK uses `0.12`/`2.0`; the upstream crates align their versions so one
  binary carries one of each.
- **Upstream pace.** The SDK pins a published version and moves only when that
  version carries the parity the switch needs.
- **What must not change.** The UDL stays addition-only; the registry answers one
  provider per capability with independent slots; the provider layer stays
  usable without rendering and never names this SDK.
- **Open:** whether the published provider layer keeps its current trait names or
  renames to the SDK's vocabulary (the plan assumes additive traits with older
  names kept where they make sense), and whether the SDK publishes its cache
  wrappers upstream or keeps them (they stay here because they are part of the
  SDK's caching policy).

## 6. Offline routing

The offline routing graph is the road network the SDK routes over when no network
connection is available. `rustic-maps-offline-graph-compiler` compiles one
OpenStreetMap extract into a single `RMOG` archive.
`rustic-maps-offline-graph-archive` reads that archive back with full validation
on every target, and `rustic-maps-core` answers offline routes over it with A\*.

The compiler is a maintainer and CI tool. It never ships inside the SDK, and it
stays native-only because its OpenStreetMap PBF reader does not build for wasm.
The archive format lives in the wasm-viable
`rustic-maps-offline-graph-archive` crate, which the compiler re-exports under
its own `archive` path.

### Inputs and output

The compiler reads one OpenStreetMap `.osm.pbf` extract containing the nodes,
ways and turn-restriction relations of the region you want to route in. Anything
outside the extract does not exist for the graph.

```console
rustic-maps-offline-graph-compiler region.osm.pbf region.rmog
```

After writing the file, the tool re-reads it through the validating reader and
prints the node, edge, turn-restriction and lane-guidance counts
(`crates/rustic-maps-offline-graph-compiler/src/main.rs:41`). A compile whose
bytes the reader rejects exits with a failure instead of leaving an unusable
archive behind.

### Container layout

An archive is one little-endian file. Format version 1 opens with a fixed header,
then a table of contents that names every resource, then the resource payloads in
canonical order:

```text
offset  0   magic "RMOG"                                4 bytes
offset  4   format version                              4 bytes
offset  8   table-of-contents entry count               4 bytes
offset 12   table of contents, 48 bytes per entry:
              resource name                            32 ASCII bytes, NUL-padded
              resource offset                          u64, a multiple of 8
              resource byte length                     u64
            resource payloads start on 8-byte boundaries and are separated
            by up to 7 zero bytes; the file length is padded to the same
            8-byte alignment
```

The format constants live in
`crates/rustic-maps-offline-graph-archive/src/archive.rs`: `ARCHIVE_MAGIC`
(`"RMOG"`), `ARCHIVE_FORMAT_VERSION` (`1`), `RESOURCE_NAME_BYTES` (`32`),
`ARCHIVE_RESOURCE_ALIGNMENT` (`8`), `FILE_HEADER_BYTES` (`12`) and
`TABLE_OF_CONTENTS_ENTRY_BYTES` (`48`). The table of contents is self-describing
in the narrow sense the reader enforces: each entry stores the resource name and
its byte span, and the reader requires the names in the fixed canonical order
rather than searching for them.

| Resource | Record width | Payload |
|---|---|---|
| `nodes` | 8 | quantized coordinate records |
| `edges` | 20 | directed edges |
| `lane_guidances` | 8 | lane guidance records |
| `instruction_strings` | blob | concatenated UTF-8 instruction text |
| `instruction_lookup_indices` | 4 | byte offsets into `instruction_strings`, one per turn restriction |
| `turn_restrictions` | 16 | restricted turns |

The list is `CANONICAL_RESOURCE_NAMES` in `archive.rs`. The entry count is fixed
at `RESOURCE_COUNT`, so a new resource can only arrive through a format version
bump; readers stay strict instead of silently ignoring unknown sections.

A node record stores latitude and longitude as fixed-point `u32` values at 1e-7
degree resolution, offset by 90 and 180 degrees so the whole globe stays
non-negative (`COORDINATE_QUANTUM_PER_DEGREE`, `LATITUDE_OFFSET_DEGREES`,
`LONGITUDE_OFFSET_DEGREES`, and `NodeRecord::from_geographic_degrees` in
`archive.rs`).

An edge record stores the source and target node index, the distance in meters,
the duration in milliseconds, and one packed `encoded_value` column:

```text
bits  0..3    road class (0 motorway, 1 trunk, 2 primary, 3 secondary,
              4 tertiary, 5 unclassified, 6 residential, 7 service)
bits  3..8    speed in 5 km/h cells (SPEED_QUANTUM_KILOMETERS_PER_HOUR)
bit   8       forward access (along the OpenStreetMap way node order)
bit   9       backward access (against that order)
bits 10..32   reserved, must be zero
```

The bit offsets are named constants in `archive.rs`, from
`ROAD_CLASS_BIT_OFFSET` through `ACCESS_BACKWARD_BIT_OFFSET`, and
`SPEED_CELL_LIMIT` caps the speed column at 155 km/h. Packing the three columns
keeps the routing hot path branch-free.

`instruction_strings` holds one phrase per recorded turn restriction, in
restriction order. `instruction_lookup_indices` holds one entry per restriction:
a byte offset into that blob, or `INSTRUCTION_ABSENT_INDEX` (`u32::MAX`) when no
text exists. `instruction_text` resolves an entry to the text between its offset
and the next non-absent offset. Lane guidance records carry an edge index and a
lane count; the resource has its slot in the canonical order, but the compiler
emits no records.

### Why the format is not flatdata

The `flatdata` crate was evaluated first and rejected because its Rust generation
is not hermetic. A `build.rs` must call `flatdata::generate`, which shells out to
`python3` and `pip` and installs a pinned generator from PyPI before it runs, so
every workspace build would depend on the network and a third-party Python
toolchain. This project instead pins its own minimal self-describing container
with the same resource names and validation semantics. `ARCHIVE_FORMAT_VERSION`
is the seam for a byte-compatible migration should a hermetic Rust generator
appear.

### Compile pipeline

1. **Collect.** One sequential pass over the PBF records node positions, drivable
   ways (OSM id, ordered node references, tags) and turn-restriction relation
   candidates
   (`crates/rustic-maps-offline-graph-compiler/src/compiler.rs`).
2. **Profile.** The tag rules in `profile.rs` decide which ways become edges.
   `highway` selects the road class and default free-flow speed; the drivable
   values are `motorway`, `trunk`, `primary`, `secondary`, `tertiary`,
   `unclassified`, `residential` and `service`, while `living_street` borrows
   residential and every `*_link` inherits its parent class. A plain integer
   `maxspeed` between 5 and 155 km/h overrides the class default; richer values
   fall back to it. `access=no` and `access=private` drop the way. `oneway=yes`,
   `1` and `true` keep only the node-order direction, `-1` and `reverse` keep
   only the reverse direction, and any other value keeps both.
3. **Build topology.** Only nodes referenced by a surviving way enter the graph,
   in ascending OpenStreetMap node id order. Each way segment emits one directed
   edge per allowed direction. The distance is the haversine distance between the
   two quantized node records, and the duration divides that distance by the
   stored, quantized speed, so the archive re-derives its own durations exactly.
4. **Resolve turn restrictions.** A `type=restriction` relation with a `from` way,
   a `via` node and a `to` way maps onto one of five kinds: `no_left_turn`,
   `no_right_turn`, `no_u_turn`, `no_straight_on` and `only_straight_on`. The
   compiler resolves each candidate to edge and node indices and skips
   candidates it cannot resolve, instead of failing the compile.
5. **Encode.** The archive builder validates every cross-reference and writes the
   bytes (`encode_deterministic` in `archive.rs`).

### Lane guidance, determinism and fixtures

The compiler emits no lane guidance records deliberately. OpenStreetMap
`turn:lanes` tags describe which maneuvers each lane permits (for example
`through|through|right`), while the archive's `LaneGuidanceRecord` stores a lane
count per edge. A counts-only record can represent neither the per-lane maneuvers
nor a recommended lane, so producing records from today's tags would drop
information or invent it. The resource keeps its slot so the layout stays
byte-stable for a future lane-geometry extension.

Two compiles of the same extract produce byte-identical archives. Surviving nodes
keep ascending OpenStreetMap node id order; edges sort by (source node index,
target node index, originating way id); turn restrictions sort by (from edge
index, via node index, to edge index), and instruction strings follow that same
restriction order; the writer emits resources in canonical order and writes
every alignment gap as zero bytes.

Two fixtures are committed beside the compiler, and always-on tests guard both
against drift:

- `crates/rustic-maps-offline-graph-compiler/fixtures/osm/routing_grid.osm.pbf` is the small routing-grid extract in which
  three ways survive, one way is dropped by `access=private`, one restriction
  applies, and eight nodes become six. Its test compares the committed bytes with
  the canonical generator's output.
- `crates/rustic-maps-offline-graph-compiler/fixtures/rmog/routing_grid.rmog` is that extract compiled once by the compiler
  binary. Its test regenerates the archive in process, compares the bytes, and
  then re-validates them through `from_bytes`, the same loader the browser uses.

Set `MAP_OFFLINE_GRAPH_COMPILER_REGENERATE_FIXTURE` or
`MAP_OFFLINE_GRAPH_COMPILER_REGENERATE_ARCHIVE_FIXTURE` to rewrite a fixture from
the canonical source; both switches live in the compiler's `tests/common/mod.rs`.
The round-trip test pins the archive's observable content: node order and
coordinates, edge endpoints and canonical order, one-way direction, road classes,
speeds, durations, the restriction record, and the instruction string with its
lookup index. A separate test proves two compiles agree byte for byte.

### Reading and validation

`OfflineRoutingNetworkGraph::from_bytes` parses and fully validates an archive
before returning it. Corruption never reaches the routing engine and never causes
undefined behavior. Every failure is one of three typed reasons, carried by
`SDKError::OfflineRoutingFailure` and defined in
`crates/geospatial-types/src/error.rs`:

| Reason | Raised when |
|---|---|
| `ArchiveMalformed` | the input is shorter than the header, the magic is wrong, the entry count is not the canonical six, a name is wrong or out of order, an offset is misaligned or below the previous resource's end, a span exceeds the file, a payload length is not a whole number of records, packed enum values are unknown, reserved bits are set, or the instruction blob is not UTF-8 |
| `VersionUnsupported` | the magic matches but the format version is not this crate's |
| `IndexOutOfRange` | an edge, restriction or lane-guidance record references a node or edge outside the archive, a lookup offset lands past the blob or off a UTF-8 character boundary, or the lookup count differs from the restriction count |

The builder enforces the same cross-reference checks in `encode_deterministic`,
so an out-of-range graph fails at compile time with the same typed vocabulary
instead of producing bytes the reader would reject.

### Native memory-mapped path

On native targets, `MemoryMappedArchive::open` maps the file and validates the
file-level alignment invariants before handing out bytes: the mapped region
covers at least the 12-byte header; its length is a multiple of
`ARCHIVE_RESOURCE_ALIGNMENT`, because the writer always pads to that boundary;
and the mapped base address honors the same alignment, which keeps the
table-of-contents `u64` reads naturally aligned. Page-granular mapping provides
this, and the check validates it rather than assuming it. The mapped bytes then
go through the same `from_bytes` validation as in-memory bytes. Both load paths
share one implementation, so a route from mapped file bytes and a route from
in-memory bytes of the same archive are identical. Missing, unreadable or
unmappable files surface as `ArchiveMalformed`: the typed vocabulary has no I/O
cause, and an archive that cannot be mapped is unusable in the same way a
malformed one is. `offline_routing_network_graph_from_mapped_file` combines the
mapping and validation in one call.

### Wasm path

The browser has no `mmap`, so the wasm target loads the graph from in-memory
bytes only. `OfflineGraphSource::Memory` takes the archive as a byte vector and
is the only source on wasm; an `OfflineGraphSource::FilePath` load is rejected
with a typed `ProviderRequestRejected` message that names the memory source to
use instead (`crates/rustic-maps-core/src/offline_routing.rs`).

The committed compiled fixture serves same-origin from the playground dev server,
and the wasm bindings expose `offlineRouteFromArchiveBytes`, which takes the
archive bytes plus a start and end coordinate, validates through the same reader
and answers a route. It is the browser half of the contract's
`offline_route_from_archive_bytes` free helper: both surfaces delegate to the
same `rustic-maps-core` implementation, so native and wasm answers cannot drift.

### Routing consumer

`ArchiveOfflineRoutingProvider` in `rustic-maps-core` implements the
`OfflineRoutingProvider` trait:

- **Load.** It accepts either a file path (native, memory-mapped) or bytes (every
  target), validates the archive, and builds a half-edge adjacency plus the
  turn-restriction rules once. A failed load leaves any previously loaded graph
  in place.
- **Snap.** It snaps each request point to the nearest graph node within a named
  50 m tolerance (`NODE_SNAP_TOLERANCE_METERS`) and rejects coordinates beyond
  it.
- **Search.** It runs A\* with a haversine heuristic that stays admissible for
  both weights: straight-line distance for `Shortest`, and that distance divided
  by the fastest storable edge speed for `Fastest`. `dijkstra_reference_search`
  is the same search with a zero heuristic; it is a benchmark baseline only and
  nothing routes through it.
- **Restrictions.** A restriction is evaluated against the settled path's
  incoming edge, which is exact unless a node can be reached on several best-cost
  paths through differently restricted incoming edges, in which case it
  over-restricts rather than understating.
- **Maneuvers.** Step boundaries come from the geometry: a junction whose signed
  bearing change leaves the straight band opens a turn step, with
  `STRAIGHT_ON_BAND_DEGREES`, `SLIGHT_TURN_BAND_DEGREES`,
  `STANDARD_TURN_BAND_DEGREES` and `SHARP_TURN_BAND_DEGREES` classifying slight,
  standard, sharp and U-turn maneuvers. A road-class or speed change without a
  turn opens a `Continue` step. The archive carries no street names, so step
  texts stay name-free (`"Turn right"`, `"U-turn"`) and are end-anchored on the
  banner while the maneuver type stays anchored at the step's opening junction.

`OfflineFallbackRoutingProvider` wraps an online routing provider with this one.
A successful online answer passes through untouched. When the online request
fails for a request with at least two waypoints, it calculates each consecutive
waypoint pair offline, joins the legs, sums distance and duration, and unions the
bounding boxes.

### Verification gates

The offline graph suite runs without network access: the committed extract and
compiled archive match their canonical generators byte for byte; the compiler
round trip asserts every archived field; two compiles of one extract are
byte-identical; truncation, bad magic, a corrupted version, set reserved bits and
out-of-range references each fail with the expected typed reason; mapped-file and
in-memory loads of the same archive answer identical routes; lane-tagged ways
compile to edges without fabricating lane guidance; and the fixture's right-angle
streets materialize the expected derived turns.

## 7. Related documents

- `Providers.md` — the provider registry, credentials, attribution and terms.
- `Browser.md` — the WebAssembly package, the canvas view and the browser demo.
- `StyleSupport.md` — style documents, vector tiles, glyphs and label rendering.
- `ReferenceAppUX.md` — the reference applications' behavior and design
  language.
- `Release.md` — how each platform package is produced and released.
