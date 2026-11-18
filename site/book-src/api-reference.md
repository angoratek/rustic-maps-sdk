# API reference

The SDK exposes one provider-neutral map platform to five surfaces: Rust
libraries, Android/Kotlin, iOS/Swift, React Native and WebAssembly. Every
surface carries the same object model, the same records and the same error
variants; only the spelling changes with the language.

Use this page to find the right reference page and to learn the rules every
surface shares. Each platform page covers every public type that surface
exposes: what it does, how it is constructed, every member with its
parameters and return value, the failures it can raise, its async and
threading rules, and a minimal example.

## Choose your surface

| Surface | Reference | You are writing |
| --- | --- | --- |
| Rust | [Rust API reference](api-reference-rust.md) | a Rust service, tool or custom target |
| Android | [Android API reference](api-reference-android.md) | Kotlin or Java on Android |
| iOS | [iOS API reference](api-reference-ios.md) | Swift or SwiftUI on iOS |
| React Native | [React Native API reference](api-reference-react-native.md) | TypeScript with React Native |
| WebAssembly | [WebAssembly API reference](api-reference-wasm.md) | TypeScript or JavaScript in a browser |

## The object model

One root object owns provider registration and the tile cache. One surface
view draws it. Engines are thin facades over the same root, so a call made
through an engine has exactly the same validation, failures and detail
messages as the matching root member.

| Type | Purpose | Android / iOS | React Native | WebAssembly | Rust |
| --- | --- | --- | --- | --- | --- |
| `MapSdk` | Provider registry root and async provider calls | `MapSdk` | `MapsSdk` package handle; generated `MapSdk` | `MapsSdk` | `ProviderRegistry` plus the provider traits |
| `MapView` | Surface lifecycle, overlays, location layer, vector base, frame statistics | shipped widget plus generated `MapView` | `MapView` component | `MapView` canvas class | `SurfaceTileRenderer`, `HeadlessTileRenderer`, `WorldSpaceOverlayGeometry` |
| `MapCamera` | Anchored camera: center, zoom, bearing, pitch | `MapCamera` plus `MapCameraPose` | generated `MapCamera`; `MapCamera` pose in the component | `MapCamera` | `MapCamera` |
| `Navigator` | Turn-by-turn trip controller | `Navigator` | generated `Navigator` | `Navigator` | `NavigationSession` and `NavigationStateBroadcaster` |
| `RouteSimulator` | Distance- or time-parameterized trip playback | `RouteSimulator` | generated `RouteSimulator` | `RouteSimulator` | `RouteWalk` |
| `SearchEngine` | Search, suggestions and reverse geocoding | `SearchEngine` | generated `SearchEngine` | free functions | `SearchPlaceProvider`, `GeocodingProvider` |
| `RoutingEngine` | Route calculation and distance matrix | `RoutingEngine` | generated `RoutingEngine` | free functions | `RoutingProvider`, `DistanceMatrixProvider` |
| `OfflineManager` | Tile-cache terms and time-to-live | `OfflineManager` | generated `OfflineManager` | not exposed | `rustic-maps-core` cache policy |
| `Diagnostics` | Structured log events | `Diagnostics` | generated `Diagnostics` | free functions | `tracing` events; `rustic-maps-navigation` recorder |

`MapSdk` is the only object that holds provider state. Construct it once,
register a credential or a base-map source, and pass it to the view and to
every engine. Constructing a second root opens a second provider registry and
a second tile cache.

## Naming across surfaces

| Concept | Kotlin | Swift | TypeScript | Rust |
| --- | --- | --- | --- | --- |
| Type | `PascalCase` | `PascalCase` | `PascalCase` | `PascalCase` |
| Member | `camelCase` | `camelCase` | `camelCase` | `snake_case` |
| Enum variant | `UPPER_SNAKE_CASE` | `lowerCamelCase` | `PascalCase` | `PascalCase` |
| Record field | `camelCase` | `camelCase` | `camelCase` | `snake_case` |
| Error type | `SdkException` sealed class | `SdkError` enum | `SdkError` class with `tag` | `SDKError` enum |

The contract spells record fields in `snake_case`. Every binding converts them
to the language's own case; the WebAssembly seam does the same for its plain
JavaScript objects, so `south_west` arrives as `southWest` and `maximum_zoom_level` as
`maximumZoomLevel`.

## Async and threading

- The provider calls, `offlineRouteFromArchiveBytes` and `MapView.draw` are
  asynchronous. Kotlin uses `suspend` functions, Swift uses `async throws`,
  React Native returns a `Promise`, WebAssembly returns a `Promise`. Rust
  provider traits are `async` functions through `async_trait`.
- Rendering is frame-driven. There is no SDK render thread and no continuous
  redraw loop. The platform view calls `draw` once per wanted frame from its
  own display-link or frame callback, and calls it again only when
  `isFrameSettled` answers false.
- `draw` holds the view's render state for its duration. A concurrent `draw`
  or `resize` reports `RenderingSurfaceUnavailable`; retry the next frame.
  The shipped Android and iOS views do this automatically.
- Navigation listener callbacks run synchronously on the thread that drives
  the controller call. A listener must not block. Calling back into the
  controller during an in-flight dispatch has its broadcast skipped; the
  state change behind it is already applied.
- A listener registered through the diagnostics subscription is called
  fire-and-forget on the thread that produced the event. A slow listener
  never slows the event stream.

## Failures

Every failing call reports one of these contract variants. Kotlin throws a
`SdkException` subclass, Swift throws the `SdkError` case, React Native
rejects with an `SdkError` instance and WebAssembly throws the verbatim
reason string. The detail string is the verbatim reason and is safe to show
to a developer.

| Variant | Meaning |
| --- | --- |
| `InvalidCoordinateInput` | A coordinate, viewport extent, box corner or insets value failed validation. |
| `NetworkRequestFailed` | The transport failed: connection, timeout or DNS. |
| `ResponseParsingFailure` | The provider answered with a payload the parser cannot use. |
| `ProviderRequestRejected` | The provider returned a documented application-level failure. |
| `RateLimitExhausted` | The token-bucket limiter is exhausted; retry after the next refill. |
| `ProviderNotConfigured` | No provider is registered for the requested contract; the contract name is carried. |
| `InvalidNavigationInput` | A route record or navigation policy failed validation. |
| `InvalidRenderInput` | An overlay, location layer, surface handle or viewport extent is unusable. |
| `RenderingSurfaceUnavailable` | The GPU surface is unusable, or the view is busy drawing or resizing. |
| `CacheOperationFailure` | A cache read, write or eviction failed. |
| `OfflineRoutingFailure` | An offline archive is truncated, foreign, unsupported or malformed. |

An SDK call never swallows a failure. The shipped Android and iOS map views
deliver every failure to their listener or delegate with the wrapper phase it
occurred in, keeping the original error as the cause.

## Provider registration

Provider credentials never enter source or a build configuration. Read one at
runtime and register it on the root; the shipped registration path maps a
provider key to that provider's full contract set. Registering again replaces
every slot.

```kotlin
import uniffi.rustic_maps.MapSdk

val mapSdk = MapSdk()
mapSdk.configureProviderSet("mapquest", providerApiKey)
```

A base map can also be a provider-neutral `{z}/{x}/{y}` tileset registered
directly on the root, with no provider crate involved. A vector base map is
installed on the view with `setVectorTileSource` and paints one of the SDK's
committed styles; a registered vector-tile provider can instead paint the style
document it publishes through `setProviderVectorTileSourceFromDocument`.

## Where to start

- [Your first map](first-map.md) builds a rendered map on every platform.
- [Search and routing](search-and-routing.md) drives the engines.
- [Turn-by-turn navigation](navigation-simulation.md) drives a trip.
- [Offline maps](offline-maps.md) installs a tile cache and routes from an
  offline graph.

## Related reference

- [Rust API reference](api-reference-rust.md)
- [Android API reference](api-reference-android.md)
- [iOS API reference](api-reference-ios.md)
- [React Native API reference](api-reference-react-native.md)
- [WebAssembly API reference](api-reference-wasm.md)
