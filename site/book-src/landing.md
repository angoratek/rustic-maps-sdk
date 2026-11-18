# Rustic Maps SDK

<div class="overview-header">

**Rustic Maps SDK** draws maps, resolves places, plans routes, follows a
turn-by-turn trip, and routes offline. One Rust core serves **Android
(Kotlin)**, **iOS (Swift)**, **React Native (TypeScript)**, and **WebAssembly
(TypeScript)** through one UniFFI contract. Map platforms connect through
vendor-neutral provider contracts: the provider layer carries MapQuest, Photon,
OSRM, Valhalla, HERE, Google Maps, TomTom, Mapbox and Radar, and any `{z}/{x}/{y}`
raster or vector source connects through the URL-template providers.

<p class="landing-cta">
<a href="demo/">Open the live wasm demo</a>
<a href="demo/mobile/">Run the apps on a hosted device</a>
<a href="first-map.html">Start building</a>
<a href="downloads.html">Downloads</a>
</p>

</div>

## What you get

<div class="feature-grid">

<div class="feature-card">

### One contract, four platforms

You call the same members on every target: Kotlin suspend functions, Swift async
throwing methods, React Native promises, and the WebAssembly TypeScript surface.
One UniFFI definition generates the native bindings, the browser mirror is
checked against it, and the contract changes addition-only.

</div>

<div class="feature-card">

### Search, routing, and traffic

Ten provider contracts cover geocoding, routing, the distance matrix, place
search, traffic incidents, traffic flow, elevation, raster tiles, vector tiles,
and offline routing. MapQuest implements the platform API set; Photon serves
keyless geocoding and search, OSRM and Valhalla serve keyless routing, and the
URL-template providers serve any raster or vector base map.

</div>

<div class="feature-card">

### A rendering core that draws anywhere

The renderer spans web-mercator projection, an anchored camera with animations
and fit-bounds framing, exact frustum tile listing, a frame-driven tile scheduler
with per-frame budgets and parent fallback, and a byte-budget tile texture pool
feeding a `wgpu` pipeline. Raster and vector scenes draw on Android, iOS, React
Native and in the browser through WebGPU.

</div>

<div class="feature-card">

### Turn-by-turn navigation

The navigation state machine follows the Ferrostar model. It composes banner and
spoken instructions from the provider's text, publishes speech triggers at
documented distance bands, and leaves deviation handling and reroute policy at
clear seams.

</div>

<div class="feature-card">

### Offline maps

An OpenStreetMap PBF extract compiles into a compact, self-describing routing
archive. An offline provider answers routes over the memory-mapped graph with
recorded turn restrictions, and a fallback decorator wraps your online provider
when you choose to fall back.

</div>

<div class="feature-card">

### Structured diagnostics

A bounded, per-subsystem diagnostics ring streams structured log events to a
listener with coordinate scrubbing at the sink. Readbacks expose frame tile
statistics, cache terms, and provider-failure records.

</div>

</div>

## Architecture at one glance

| Layer | What it does |
|---|---|
| Records and types | Coordinates, bounding boxes, routes, places, traffic, tiles, and the typed error model |
| Provider contracts | The ten capabilities a provider implementation fulfills: geocoding, routing, distance matrix, search, traffic incidents, traffic flow, elevation, raster tiles, vector tiles, and offline routing |
| Resolution and cache | Provider lookup with cache-then-network behavior, backed by a disk response cache with per-category time-to-live and byte-budget eviction |
| Rendering | Projection, camera, tile scheduling, texture pooling, and the GPU renderer |
| Navigation | The turn-by-turn trip state machine and its instruction composition |
| Offline routing | OpenStreetMap extracts compiled into a routing archive, plus its memory-mapped reader |
| Bindings | The generated Kotlin, Swift, React Native, and WebAssembly surfaces over one contract |

The full call surface is the [FFI contract](udl-contract.md); each member's
documentation lives in the generated [API reference](api-reference.md).

## Requirements at a glance

- **Android**: API level 24 or higher. One AAR carries the bindings, the
  `MapView` and three ABIs; it measures about 16 MiB.
- **iOS**: iOS 17 or higher. The XCFramework ships two arm64 architectures.
- **React Native**: React Native 0.88 or newer and React 19 or newer.
- **WebAssembly**: a browser with WebGPU. The wasm package measures 3.6 MiB.

Artifacts are distributed from the [Downloads](downloads.md) page rather than a
package registry. Per-platform tables, artifact sizes, and the version policy
are in [Requirements and distributions](requirements.md).

## Where to go next

1. Install the artifacts and build a rendered map: [Your first
   map](first-map.md).
2. Resolve places and plan a route: [Search and
   routing](search-and-routing.md).
3. Simulate a guided turn-by-turn drive: [Turn-by-turn
   navigation](navigation-simulation.md).
4. Keep rendering and routing working without a network: [Offline
   maps](offline-maps.md).
5. Switch the base map, traffic and device location from the map: [Map
   layers and traffic](map-layers-and-traffic.md).
6. Serve each capability from a different provider: [Mix providers per
   capability](mix-providers-per-capability.md).

## License and attribution

The SDK is dual-licensed under **MIT OR Apache-2.0**. Map imagery © MapQuest;
map data © OpenStreetMap contributors (ODbL). See [third-party
notices](third-party-notices.md) for the full attribution and license table.
