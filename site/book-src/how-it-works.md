# How it works

The SDK is one Rust core behind one generated interface. This page summarizes
the architecture and the provider pipeline. Every public type and call named
here is part of the [FFI contract](udl-contract.md); [Map concepts](map-concepts.md)
carries the coordinate, tile and camera model, and the guides apply it.

## The architecture at a glance

Your application calls generated bindings over the UniFFI contract. The Rust
core beneath them holds the provider registry and adapters, the camera and GPU
renderer, the navigation reducer, the SQLite tile cache, and offline routing.
The browser build compiles the same core modules to WebAssembly and adds a
canvas binding that owns the WebGPU device and a fetch transport that replaces
the native HTTP stack. Camera math, tile scheduling, routing and navigation are
the same code the phone builds run.

One UniFFI UDL file defines the surface, and the Kotlin, Swift, React Native and
WebAssembly bindings are all generated from it. The contract changes
addition-only: an existing member keeps its signature and meaning, and a release
adds members and records. Plain data crosses as records and enums, and failures
cross as `SdkError` variants. `getSdkCompiledVersion` answers the loaded core's
version on every surface.

## Threading and lifecycle

The renderer draws only when asked. The native `MapView` owns a frame pump and
skips a frame when `isFrameSettled` reports nothing would change. The
WebAssembly page calls `renderFrame` on each change, so a still map costs no
frames. Async calls suspend on the platform's own runtime and are safe from the
main thread. Navigation callbacks arrive in registration order and must not
block. Drawing is single-flight: a concurrent `draw` reports
`RenderingSurfaceUnavailable`. The surface lifecycle is attach, resize, draw,
detach; re-attach after you apply a credential.

## The provider registry

Provider-backed capability reaches you through `MapSdk`, an object with no
providers at construction. You register implementations, then call the contract
methods: geocoding, routing, the distance matrix, place search, traffic
incidents, traffic flow and raster tiles. A call into a contract with no
registration throws `ProviderNotConfigured` naming the contract.

Registration paths:

- `setProviderCredential` installs the map-platform provider set from one API
  key, with the SDK defaults for rate limiting and retries.
- `setProviderCredentialWithTileCache` installs a named credentialed provider's
  set and wraps its raster tiles in the disk response cache.
- `registerUrlTemplateRasterTileSource` installs any web-mercator XYZ raster
  tileset from a `{z}/{x}/{y}` URL template, with no provider key.
- The `register*FromForeign` calls install an implementation your own Kotlin or
  Swift code supplies through the matching interface.

Registration is per object. Re-registering the provider set replaces every slot;
re-registering a URL-template raster source replaces the raster-tile slot, keeps
every other slot serving, and drops any installed tile cache. The binding
exposes an elevation call, and no shipped provider fills it, so it refuses
until you register one.

The cache behind `setProviderCredentialWithTileCache` is SQLite at a path you
own, under an absolute byte budget. Cached tiles are retained under the tiles
category's time-to-live, 30 calendar days by default, which is also the imagery
provider's terms ceiling; `rasterTileCacheTerms` reads back the effective value,
the ceiling and the default. Offline routing is a separate layer with its own
compiled archive.

## The error model

Every failure crosses the contract as one `SdkError` variant carrying a verbatim
detail string. `NetworkRequestFailed` retries with backoff,
`RateLimitExhausted` after the next refill, and `RenderingSurfaceUnavailable` on
the next frame. The other variants report a condition you must change. Surface
the detail as it arrives; it names the provider's own reason. The
[API reference](api-reference.md) lists every variant.

## Where to go next

The guides apply these concepts in order: [Your first map](first-map.md),
[Search and routing](search-and-routing.md), [Turn-by-turn
navigation](navigation-simulation.md), and [Offline maps](offline-maps.md).
