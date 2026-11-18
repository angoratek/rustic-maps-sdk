# Rustic Maps SDK contract

The SDK has one API surface. A single UniFFI UDL contract defines every type
and call, and the Android, iOS, React Native and WebAssembly bindings are built
against that contract. This page explains what the contract contains, how each
target binds it, and how to read the UDL. For member-by-member signatures and
return records, use the [API reference](api-reference.md); the contract file
itself is also published as a [plain-text download](../udl/rustic_maps.udl).

## What the contract is

The contract is one file written in UniFFI's interface definition language. It
declares the namespace, the records and enums that cross the language boundary,
the objects that own state, the interfaces your own code can implement, and the
error type every call can raise. It carries the doc comments the generated
bindings show, so the contract is both the implementation source and the
reference for the surface.

The contract is pre-1.0 and **addition-only**: an existing member keeps its
signature and its meaning, and a release adds members rather than removing or
renaming them. An upgrade therefore does not rewrite your calls. The
[changelog](changelog.md) records what each area adds, and the
[versions and support](versions-and-support.md) page explains how releases are
numbered.

Every target answers `getSdkCompiledVersion`, which reports the version of the
loaded core. Compare it with the artifact you shipped; a mismatch means your
dependency declaration and the loaded library disagree.

## How each target binds it

The native targets are generated from the contract. The browser target is a
hand-maintained TypeScript façade over the same Rust core, kept aligned by a
parity check. Android and iOS call generated types; React Native calls a
generated Turbo Module; WebAssembly calls the façade. The contract namespace is
`rustic_maps`, and the generated Kotlin package and Swift `RusticMapsSDK`
module carry the generated surface. The UDL writes members and fields in
`snake_case`; the bindings camelCase them. Kotlin enums are `UPPER_SNAKE`
constants, Swift cases are lowerCamel, and the TypeScript surfaces use their
own declaration's spelling. One concept keeps one name across platforms:
`MapView`, `MapCamera`, `MapSdk`, `Navigator`, `search`, `calculateRoute`,
`startNavigation`.

## The namespace and core types

The namespace `rustic_maps` holds the free functions and the object types. The
objects group the surface by capability.

**Free functions**

- `getSdkCompiledVersion` — the compiled SDK version.
- `coordinateToNormalizedWorld`, `tileKeyForCoordinate`, `tilesCovering` —
  projection and web-mercator tile addressing.
- `offlineRouteFromArchiveBytes` — a route over an offline graph archive held in
  memory, with no provider registered.
- `defaultStepAdvancePolicy`, `defaultDeviationPolicy`, `defaultReroutePolicy` —
  the SDK defaults a navigation session starts from.

**Value records**

Coordinates and geometry cross as `GeographicCoordinate`, `BoundingBox`,
`ScreenPoint`, `NormalizedWorldCoordinate`, `TileKey`, `ViewportSize` and
`EdgeInsets`. Provider results cross as `Place`, `Route`, `RouteLeg`,
`RouteStep`, `DistanceMatrix`, `TrafficIncident`, `TrafficFlowData` and
`RasterTileData`. Configuration crosses as `RouteOptions`, `AvoidOptions`,
`UrlTemplateRasterTileSource`, `UrlTemplateVectorTileSource`, `MapOverlay`,
`OverlayColor` and `TileCacheTerms`. Navigation crosses as
`NavigationLocationUpdate`, `TripNavigationState`, `NavigationInstruction`,
`DeviationStatusChange`, `SimulatedTripPace`, `SimulatedTripFix` and the
policy records.

**Objects**

- `MapSdk` — the provider registry and the provider-backed calls: geocoding,
  search, routing, distance matrix, traffic, elevation and raster tiles, plus
  `configure_provider`, `provider_registrations` and the
  `register_*_from_foreign` calls.
- `MapView` — the native map surface: attach, resize, detach, per-frame draw,
  overlays, the location indicator, a vector base map and frame statistics.
- `Navigator` — the navigation session: start, location updates, reroutes,
  topic subscriptions and read-only state.
- `RouteSimulator` — deterministic interpolation along a planned route at a
  pace you choose.
- `SearchEngine`, `RoutingEngine`, `OfflineManager`, `Diagnostics` — thin
  facades over the matching `MapSdk` members, so a capability has one object.
- `MapCamera` — an immutable camera pose with projection, fit-bounds and
  gesture math.
- `Subscription` — the handle that cancels one listener subscription.

**Interfaces your code implements**

- `GeocodingProvider`, `RoutingProvider`, `DistanceMatrixProvider`,
  `SearchPlaceProvider`, `TrafficIncidentProvider`, `TrafficFlowProvider`,
  `ElevationProvider` and `RasterTileProvider` — register your own provider
  implementation on `MapSdk` with the matching `register_*_from_foreign` call.
- `NavigationListener` — receive the instruction, deviation, phase and speech
  topics.
- `SdkDiagnosticEventListener` — receive the SDK's structured diagnostic events.

## The error model

Every failure the contract declares is one variant of `SdkError`. A variant that
carries a `detail` field carries the provider's or the validator's reason
verbatim: surface that text as it arrives. `ProviderNotConfigured` carries the
contract name instead of a detail string, and `RateLimitExhausted` carries no
payload. Retry `NetworkRequestFailed` with backoff, `RateLimitExhausted` after
the next refill, and `RenderingSurfaceUnavailable` on the next frame. The other
variants report a condition you must change: fix the input, register a provider,
repair the archive or the cache, or report the verbatim detail. The generated
Kotlin, Swift and React Native bindings expose the typed variant; the
WebAssembly façade throws the verbatim message string. The [API
reference](api-reference.md) lists every variant.

## Async rules

Members that wait on the network or other asynchronous work carry `[Async]` in
the contract. The binding presents them as a Kotlin suspend function, a Swift
async throwing method, or a promise in React Native and WebAssembly. The SDK
drives the I/O on its own runtime, so a call from the main thread does not block
your event loop.

The interfaces your code implements follow the same rule: `GeocodingProvider`
and `RasterTileProvider` methods are async, and listener callbacks are
synchronous. A callback runs on the thread that produced the event and must not
block. `MapView.draw` is async and single-flight: a concurrent draw or resize
reports `RenderingSurfaceUnavailable`, and the next frame retries. Call
`isFrameSettled` first and skip the draw when nothing has changed. A
subscription is cancelled explicitly with `Subscription.cancel`; the call is
idempotent, and dropping the handle does not unsubscribe.

## How to read the UDL

The contract uses a small set of UniFFI constructs. Read the file with this
table in hand.

| You see | It declares |
|---|---|
| `namespace rustic_maps { ... }` | The free functions of the contract. |
| `dictionary Name { ... }` | A record: a plain value with the listed fields. |
| `enum Name { "Variant", ... }` | A closed set of values. UDL writes enum variants as quoted names. |
| `interface Name { ... }` | An object with methods; some declare constructors. |
| `[Error] interface SdkError { ... }` | The error type and its variants; each variant lists its payload fields. |
| `[Trait, WithForeign] interface Name { ... }` | An interface your foreign code implements and registers. |
| `[Throws=SdkError]` | The member can raise an `SdkError` variant. |
| `[Async]` | The member returns a future. Per-member attributes are the only async form the UDL parser accepts. |
| `[Name=fit_bounds]` | The constructor or method is exposed under the given name. |

Field types read as follows: `string`, `double`, `boolean`, `u8`, `u32` and
`u64` are the scalar types; `bytes` is a byte sequence; `sequence<T>` is a
list; a trailing `?` marks an optional value. A `dictionary` field may name
another dictionary or an enum. The `///` comments in the file are the doc
comments the generated bindings carry.

This excerpt shows the four shapes together: a free function, a record, an
enum, and a provider interface your code implements.

```udl
namespace rustic_maps {
    string get_sdk_compiled_version();

    [Throws=SdkError]
    TileKey tile_key_for_coordinate(GeographicCoordinate coordinate, u8 zoom_level);
};

dictionary TileKey {
    u32 tile_x;
    u32 tile_y;
    u8 zoom_level;
};

enum UnitSystem {
    "Metric",
    "Imperial",
};

[Trait, WithForeign]
interface GeocodingProvider {
    [Async, Throws=SdkError]
    sequence<Place> reverse_geocode(GeographicCoordinate coordinate);
};
```

Read a member's name twice: the UDL spelling is what the Rust implementation
uses, and the camelCase spelling is what you type in Kotlin, Swift, React
Native or the browser. A member without `[Async]` returns its value directly.

## Where to go next

- [API reference](api-reference.md) — the generated documentation for every
  module in the current release.
- [Your first map](first-map.md) — construct a `MapView` and draw a frame.
- [How it works](how-it-works.md) — coordinates, tiles, the camera and the
  provider registry in context.
