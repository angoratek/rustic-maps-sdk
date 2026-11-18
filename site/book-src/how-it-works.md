# How it works

The SDK is one Rust core behind one generated interface. This page describes
that interface's architecture: the data model, the camera and tile pipeline, the
provider registry, the renderer, navigation, and offline routing. Every public
type and call named here is part of the [FFI contract](udl-contract.md).

## Coordinates and the camera

A position is a `GeographicCoordinate`: latitude and longitude in decimal
degrees on the WGS 84 ellipsoid. A region is a `BoundingBox` between a
south-west and a north-east corner, and a box never spans the antimeridian.

The camera is a `MapCamera` value: an anchored pose of center, zoom, bearing,
and pitch. The ground point under the center sits at the viewport center, zoom
and pitch are clamped, and bearing wraps into the range from 0 up to but not
including 360 degrees.

| Camera attribute | What it means |
|---|---|
| `center` | The coordinate anchored at the viewport center |
| `zoom_level` | Scale; a continuous level, not necessarily a whole number |
| `bearing_degrees` | Rotation clockwise from north |
| `pitch_degrees` | Tilt from top-down (0) toward the horizon |

A camera carries no setters. A new pose is a new constructor call, and the
attribute getters read the state back as it was built. The second constructor,
`fitBounds`, frames a bounding box inside the inset region you give it:
top-down, north-up, zoomed to the largest level that keeps the whole box
visible, and anchored on the inset region's center. Insets are the pixels your
own UI keeps clear, so route cards and attribution never hide the destination.

The camera answers screen mappings without drawing. `projectToScreen` and
`screenToGroundCoordinate` convert between a coordinate and viewport pixels,
`visibleTileKeys` lists the tiles the view covers, and
`groundMetersPerScreenPixel` sizes a scale bar.

## Zoom levels and tiles

A tile is one payload at a fixed zoom, addressed by a `TileKey`: a column, a row
counted from the north, and the zoom level. The addressing is the web-mercator
XYZ scheme. At zoom 0 the world is one tile, and each level doubles the columns
and rows.

The camera's zoom is continuous, so it answers two different questions.
`tileZoomLevel` gives the nearest whole level a tile fetch should use.
`visibleTileKeys` gives every tile at a chosen level that intersects the visible
ground, sorted row-major from the north.

A raster tile is a pre-rendered image. The contract carries it as
`RasterTileData` with its encoded bytes, an image format (PNG or JPEG), and a
resolution: `Standard`, or `Retina` for the doubled-texel `@2x` form.

A vector tile carries geometry and style separately from the pixels. The
renderer decodes it, styles it with a named style, and draws it over the raster
tiles and beneath the overlay list. Installing a vector base map on the map view
with `setVectorTileSource` replaces any earlier vector base. The scheduler
fetches and decodes each tile once and restyles it from cached geometry when the
style changes or the camera crosses a style zoom stop, so it never refetches a
tile for either. Bright, Fiord, and Positron style OpenMapTiles-schema sources;
Carto Light and Carto Dark style the Sourdough source. Place names and road names
are drawn from a glyph atlas. A style that declares a shield block on its
route-reference layer draws each road or transit reference as a shaped badge;
Bright, Fiord, and Positron carry no route-reference layer, so those bases draw
no shields.

## The architecture at a glance

```text
your application
      │  Kotlin / Swift / TypeScript / JavaScript
      ▼
generated bindings  ── the UniFFI UDL contract (one surface, four platforms)
      │
      ▼
Rust core
  ├── provider registry   provider lookup and capability routing
  ├── provider adapters   vendor URLs, credentials, status mapping, attribution
  ├── camera + renderer   camera math, tile scheduling, GPU raster and vector paint
  ├── navigation          turn-by-turn reducer (pure, event-driven)
  ├── tile cache          disk response cache (SQLite, LRU byte budget)
  └── offline routing     archive reader + A* routing + tile pack manager
```

The browser build compiles the same core modules to WebAssembly and adds one
layer the native targets do not need: a canvas binding that owns the WebGPU
device and a fetch transport that replaces the native HTTP stack. Everything
above that layer — camera math, tile scheduling, routing, navigation — is the
same code the phone builds run.

## Threading and lifecycle

The renderer draws only when asked. The native `MapView` owns a frame pump: it
draws on camera, overlay, and tile changes, and skips a frame when
`isFrameSettled` reports that nothing would change. The WebAssembly surface has
no pump; your page calls `renderFrame` on each change. A still map therefore
costs no frames.

- **Async calls** (`search`, `forwardGeocode`, `calculateRoute`, tile fetches)
  suspend and resume on the platform's own async runtime: Kotlin coroutines,
  Swift concurrency, or JavaScript promises. They are safe to call from the main
  thread.
- **Navigation callbacks** are delivered in registration order on the runtime
  that produced the location update. Do no blocking work in them; hand results
  to your UI layer.
- **Drawing** is single-flight: one `draw` may be in flight per surface, and a
  second call while it runs reports `RenderingSurfaceUnavailable`. A render pump
  should skip a frame when `isFrameSettled` reports nothing changed.
- **Surface lifecycle** is attach, resize, draw, detach. Attaching without a
  usable platform surface throws; detach before the platform view goes away.
  Re-attach after you apply a credential, because a keyless start cannot draw
  provider imagery.

## The provider registry

Provider-backed capability reaches you through `MapSdk`, an object with no
providers at construction. You register implementations, then call the contract
methods: forward and reverse geocoding, routing, the distance matrix, place
search, traffic incidents, traffic flow, and raster tiles. Every call resolves
through the registry. A call into a contract with no registration throws
`ProviderNotConfigured` naming the contract.

Registration paths:

- `setProviderCredential` installs the map-platform provider set from one API
  key, with the SDK defaults for rate limiting and retries.
- `setProviderCredentialWithTileCache` installs a named credentialed provider's
  set and wraps its raster tiles in the disk response cache.
- `registerUrlTemplateRasterTileSource` installs any web-mercator XYZ raster
  tileset from a `{z}/{x}/{y}` URL template, with no provider key.
- The `register*FromForeign` calls install an implementation your own Kotlin
  or Swift code supplies through the matching interface: `GeocodingProvider`,
  `RoutingProvider`, `DistanceMatrixProvider`, `SearchPlaceProvider`,
  `TrafficIncidentProvider`, `TrafficFlowProvider`, `ElevationProvider` and
  `RasterTileProvider`.

Registration is per object. Re-registering the provider set replaces every slot;
re-registering a URL-template raster source replaces the raster-tile slot, keeps
every other slot serving, and drops any installed tile cache. A second `MapSdk`
object needs its own registration.

The ten provider contracts include elevation. The binding exposes the
elevation call, and no shipped provider fills it, so it refuses until you
register one.

## Supported providers

Capability comes from whichever provider you register. These ship today:

| Provider | What it answers | Credential |
|---|---|---|
| MapQuest | geocoding, reverse geocoding, routing, distance matrix, place search, traffic incidents, traffic flow, raster tiles | API key |
| HERE | geocoding, reverse geocoding, routing, traffic incidents, traffic flow | API key |
| Google Maps | geocoding, reverse geocoding, routing | API key |
| TomTom | geocoding, reverse geocoding, routing, traffic incidents, traffic flow, raster tiles | API key |
| Mapbox | geocoding, reverse geocoding, routing | access token |
| Radar | geocoding, reverse geocoding, routing | API key |
| Photon | geocoding, reverse geocoding, place search, type-ahead | none, or a deployment URL |
| OSRM | routing | none, or a deployment URL |
| Valhalla | routing | none, or a deployment URL |
| OpenStreetMap US (Sourdough) | vector tiles | none |
| OpenFreeMap | vector tiles | none |
| OpenStreetMap Standard | raster tiles | none |

The keyless vector and raster sources back the live demo's base map; they carry
their own attribution and usage terms, which the SDK surfaces alongside the
imagery.

Adding a provider is a provider implementation plus a registration. Your
application's calls stay the same.

## The cache path

`setProviderCredentialWithTileCache` installs the provider set and wraps the
raster-tile contract in a disk response cache: a SQLite database at a path you
own, under an absolute byte budget. A cache hit answers from disk, and a cache
miss goes to the network and back-fills.

Cached tile responses are retained under the tiles category's time-to-live, 30
calendar days by default. That default is also the imagery provider's terms
ceiling. `setRasterTileCacheTimeToLive` tunes the value, an override above the
ceiling is accepted and clamped, and `rasterTileCacheTerms` reads back three
values: the effective time-to-live, the provider's declared ceiling, and the
category default. The byte budget is fixed when the store opens, so changing it
means registering again, and a fresh registration starts at the category
default.

Raster imagery is the layer the cache holds. Offline routing is a separate layer
with its own archive.

## One contract, four platforms

One UniFFI UDL file defines the surface, and it is the single source of truth:
the Kotlin, Swift, React Native, and WebAssembly bindings are all generated from
it. The contract changes addition-only: an existing member keeps its signature
and meaning, and a release adds members and records. The
[changelog](changelog.md) records what each release adds.

| Platform | How you call the contract |
|---|---|
| Android (Kotlin) | Suspend functions and typed records; failures are the contract's `SdkError` variants |
| iOS (Swift) | Async throwing methods and records |
| React Native | Promise-returning TurboModule methods with lower camel case field names |
| WebAssembly | TypeScript classes and records over the wasm module, with the same names and field spellings as the native bindings |

Plain data crosses as records and enums. The error model crosses as the
`SdkError` variants the failing call can produce, with verbatim detail text.
`getSdkCompiledVersion` on every surface answers the compiled SDK version, so a
diagnostics screen can compare the loaded core with the artifacts your
application shipped.

## The error model

Every failure crosses the contract as one `SdkError` variant carrying a verbatim
detail string. The retry column separates transient failures from input and
configuration failures.

| Variant | Meaning | Retry? |
|---|---|---|
| `NetworkRequestFailed` | The transport failed | Yes, with backoff |
| `RateLimitExhausted` | Your own token bucket is empty | Yes, after the next refill |
| `ProviderRequestRejected` | The provider refused the request (quota, bad input) | No; fix the request or the plan |
| `ResponseParsingFailure` | The provider answered something unusable | No; report it verbatim |
| `ProviderNotConfigured` | No implementation is registered for that capability | No; register one |
| `InvalidCoordinateInput` | A coordinate, viewport, or box failed validation | No; fix the input |
| `InvalidRenderInput` | An overlay or surface input is unusable | No; fix the input |
| `RenderingSurfaceUnavailable` | The surface could not draw this frame | Yes, on the next frame |
| `InvalidNavigationInput` | A route or navigation policy failed validation | No; fix the input |
| `CacheOperationFailure` | A cache read, write, or eviction failed | No; the detail names the cause |
| `OfflineRoutingFailure` | An offline archive failed validation or could not be read | No; fix the archive |

Surface the detail string as it arrives: it names the provider's own reason and
is the text support asks for. Never retry a non-retryable variant unchanged.

## Navigation state

Routing returns a `Route`: legs between consecutive waypoints, and steps inside
each leg, each carrying its distance, duration, maneuver label, and instruction
texts. Navigation consumes that value rather than extending it.

`Navigator` holds a trip. You start one with `startNavigation(route,
unitSystem, stepAdvancePolicy, deviationPolicy)` and deliver one position fix at
a time with `updateLocation`. The controller publishes four topics: banner
instructions, speech triggers, deviation status, and phase changes. A
subscription is cancelled explicitly through its handle.

A trip has three phases: `Navigating`, `OffRoute`, and `Arrived`. Rerouting is
not a phase; a trip moves between navigating and off route until it arrives.
Deviation has two grades:

- `FullyOffRoute` freezes progress until a graded fix recovers or a reroute
  replaces the route.
- `OffStepOnRoute` means the fix is off the current step but still on a later
  part of the route, and the trip keeps navigating.

Guidance uses two policies you choose at trip start. `StepAdvancePolicy` decides
when the trip moves to the next step, and `DeviationPolicy` decides when a fix
counts as off route. A fix without a reported accuracy never advances a step and
is never judged for deviation, so a simulated source must carry accuracy like a
real one.

Instruction text has two channels. The banner channel carries the display text,
and the speech channel carries the text-to-speech text through
`SpeechInstruction`. The SDK decides when a line is newsworthy and supplies the
text verbatim. Your application owns playback and synthesis, and `utteranceId`
is a stable identity for deduplication.

A deviation asks for a replacement route through a reroute calculation gate.
`beginRerouteCalculation` answers true when the request passes the in-flight,
cooldown, and movement checks, `applyReroute` applies the replacement, and
`cancelRerouteCalculation` abandons it. The trip state's
`isCalculatingNewRoute` reports the in-flight fact.

## Offline routing

Offline routing answers from a compiled archive rather than a network round trip.
The archive is built from an OpenStreetMap extract and is self-describing. It
answers routes with an A* search over the archive's graph and honors recorded
turn restrictions. Maneuver text derives from the geometry where the extract
carries no street names.

The direct route call is a contract function on every surface:
`offlineRouteFromArchiveBytes` takes the archive bytes (a path is not needed, so
the browser can call it) plus a start and an end coordinate. The offline routing
decorator wraps your online provider when you choose to fall back; one compiled
archive serves every target.

## Where to go next

The guides apply these concepts in order: [Your first map](first-map.md),
[Search and routing](search-and-routing.md), [Turn-by-turn
navigation](navigation-simulation.md), and [Offline maps](offline-maps.md).
