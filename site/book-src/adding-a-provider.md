# Add a provider

A provider is a set of implementations of the vendor-neutral contracts plus one
descriptor in the shared provider registry. The descriptor carries the labels,
availability, attribution and tile sources; the implementations carry the
vendor's requests and status mapping. No credential is part of either.

There are two implementation routes and one no-code route:

| Route | Use it when | What changes |
|---|---|---|
| Provider crate in the provider layer | The provider serves routing, search, distance matrix, traffic, elevation, or vector tiles, or needs its own HTTP policy | A crate implementing the `everymap-core` contracts, a descriptor, a catalog row, a dispatch arm, and a registration call on each target |
| Host implementation | The provider serves geocoding or raster tiles from code that already lives in the native app | A Kotlin or Swift class implementing the interface, a registration call, and a descriptor |
| URL-template source | The provider publishes ordinary `{z}/{x}/{y}` raster or vector tiles | A descriptor only; the SDK's generic source does the work |

## What every provider needs

1. **Implementations.** One struct or class per contract the provider serves.
2. **A descriptor.** The shared registry entry that every application reads.
3. **A dispatch arm.** The one place that maps the provider key to its
   registration call on a target.
4. **Committed sample responses.** The tests and the demo read provider payloads
   from committed sample data, never from the network.

## Route 1: a provider crate in the provider layer

### Implement the contract

Implement the matching contract from `everymap-core`. The methods are async
and return the provider layer's error type on refusal or failure. The SDK
adapts the implementation to its own records at
`rustic_maps_core::everymap_bridge`, so the vendor crate never names an SDK
type.

```rust
use async_trait::async_trait;
use everymap_core::domains::routing::{RouteOptions, RouteResponse, Router};
use everymap_core::types::Coordinate;
use everymap_core::EveryMapResult;

struct MyRoutingProvider;

#[async_trait]
impl Router for MyRoutingProvider {
    async fn calculate_route(
        &self,
        _start: &Coordinate,
        _end: &Coordinate,
        _options: &RouteOptions,
    ) -> EveryMapResult<RouteResponse> {
        todo!("implement the two-endpoint request and parse")
    }

    async fn calculate_route_with_waypoints(
        &self,
        _waypoints: &[Coordinate],
        _options: &RouteOptions,
    ) -> EveryMapResult<RouteResponse> {
        todo!("implement the ordered-waypoint request and parse")
    }
}
```

The contracts are `Geocoder`, `PlaceSearchProvider`, `AutocompleteProvider`,
`Router`, `DistanceMatrixProvider`, `TrafficProvider`, `RasterTileProvider`,
`VectorTileProvider`, `ElevationProvider`, `IsolineProvider`, `RouteMatcher`,
`TourPlanner`, `NetworkPositioner`, `AttributeProvider`, `MapImageProvider`
and `OfflineRoutingProvider`. A provider implements only the ones it serves.
The bridge adapts geocoding, search, type-ahead, routing, distance matrix,
traffic, raster tiles and vector tiles, so a vendor provider needs no adapter
of its own for those.

### Register it in the core

`ProviderRegistry` holds one slot per contract. Registering replaces the
slot; a call into an empty slot returns `ProviderNotConfigured`.

```rust
use std::sync::Arc;
use rustic_maps_core::everymap_bridge::EveryMapRouter;
use rustic_maps_core::ProviderRegistry;

fn registry_with(provider: Arc<dyn everymap_core::domains::routing::Router>) -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();
    registry.register_routing_provider(Arc::new(EveryMapRouter::new(provider)));
    registry
}
```

Use the provider layer's shared client for HTTP. It gives the native and
browser builds one request, retry and rate-limit path, so a provider behaves
the same on every target.

### Expose it to an application target

1. **Add the descriptor.** One entry in the shared provider registry, with the
   capabilities the provider serves in `served_contracts`.
2. **Add the catalog row.** One entry maps the provider key to the crate's
   construction call and the capabilities it serves. This row makes the key
   selectable; compiling the crate into a binding alone does not.
3. **Add committed sample responses.** Each served API gets a route pattern and
   a committed response under the provider's key; the test server and the demo
   use them.

An application selects the provider by key. The call replaces one capability and
leaves every other capability serving.

**Kotlin**

```kotlin
val mapSdk = MapSdk()
mapSdk.configureProvider(
    contract = ProviderContract.Routing,
    providerKey = "my-provider",
    credential = "",
)
val routing = mapSdk.providerRegistrations()
    .first { it.contract == ProviderContract.Routing }
check(routing.registered && routing.providerKey == "my-provider")
```

**Swift**

```swift
let mapSdk = MapSdk()
try mapSdk.configureProvider(
    contract: .routing,
    providerKey: "my-provider",
    credential: ""
)
let routing = mapSdk.providerRegistrations()
    .first { $0.contract == .routing }
precondition(routing?.registered == true && routing?.providerKey == "my-provider")
```

`providerRegistrations()` answers every capability in the same order every
time — geocoding, routing, distance matrix, place search, traffic incidents,
traffic flow, elevation, raster tiles, vector tiles, offline routing — so a
settings surface can render one row per capability without knowing the list.
A capability with no provider reports `registered` false and an empty key.

A provider that needs no credential accepts an empty string; a provider with
a credential refuses a blank one and leaves the previously serving provider
in place. That is the whole integration: the application stores its own
choice (a key and, when needed, a credential) and calls `configureProvider`
at startup.

### Sample: a routing provider key in an app

The OSRM provider ships with the SDK, so its catalog row already maps `osrm`
to `OsrmRouter::new`. OSRM is keyless: the credential field carries the
deployment's base URL, and an empty string targets a local server. Choose it
for routing at startup.

**Kotlin**

```kotlin
mapSdk.configureProvider(
    contract = ProviderContract.Routing,
    providerKey = "osrm",
    credential = "http://10.0.2.2:5000",
)
```

**Swift**

```swift
try mapSdk.configureProvider(
    contract: .routing,
    providerKey: "osrm",
    credential: "http://localhost:5000"
)
```

`RoutingFacade.calculateRoute` resolves the routing slot, so the result comes
from the chosen provider and no other capability changes. To let the reader
switch providers, render one row per capability from
`providerRegistrations()`, list the descriptors whose `served_contracts`
contain that capability, and call `configureProvider` with the picked key.

### Constraints

- A provider type is `Send + Sync`; trait methods are async.
- The crate owns its vendor URL, credential handling, status mapping, and
  error detail. It does not name another vendor.
- Map transport failures to `NetworkRequestFailed`, unparseable payloads to
  `ResponseParsingFailure`, documented refusals to `ProviderRequestRejected`,
  and an exhausted limiter to `RateLimitExhausted`. Keep the detail verbatim.
- A vector source's `source_name` is its cache namespace; two tilesets must
  never share one name.
- The provider name appears only in its crate, its sample-data directory, the
  shared descriptor and the dispatch arms; the provider-name guard enforces
  that boundary.

## Route 2: a host implementation

Every provider contract can be implemented directly in application code:
geocoding, routing, distance matrix, place search, traffic incidents, traffic
flow, elevation, and raster tiles. Register the implementation on the
`MapSdk` object that the map view and every provider call use; it replaces
that one capability and leaves the others serving.

### Geocoding in Kotlin

**Kotlin**

```kotlin
import uniffi.rustic_maps.GeocodingProvider
import uniffi.rustic_maps.GeographicCoordinate
import uniffi.rustic_maps.MapSdk
import uniffi.rustic_maps.Place

class AppGeocodingProvider : GeocodingProvider {
    override suspend fun forwardGeocode(addressText: String): List<Place> {
        return emptyList()
    }

    override suspend fun reverseGeocode(
        coordinate: GeographicCoordinate,
    ): List<Place> {
        return emptyList()
    }
}

val mapSdk = MapSdk()
mapSdk.registerGeocodingProviderFromForeign(AppGeocodingProvider())
```

### Geocoding in Swift

**Swift**

```swift
import RusticMapsSDK

final class AppGeocodingProvider: GeocodingProvider {
    func forwardGeocode(addressText: String) async throws -> [Place] {
        return []
    }

    func reverseGeocode(coordinate: GeographicCoordinate) async throws -> [Place] {
        return []
    }
}

let mapSdk = MapSdk()
try mapSdk.registerGeocodingProviderFromForeign(
    geocodingProvider: AppGeocodingProvider()
)
```

### Routing in Kotlin

**Kotlin**

```kotlin
import uniffi.rustic_maps.GeographicCoordinate
import uniffi.rustic_maps.MapSdk
import uniffi.rustic_maps.Route
import uniffi.rustic_maps.RouteOptions
import uniffi.rustic_maps.RoutingProvider

class AppRoutingProvider : RoutingProvider {
    override suspend fun calculateRoute(
        waypoints: List<GeographicCoordinate>,
        routeOptions: RouteOptions,
    ): Route {
        TODO("call your routing service and build the route")
    }
}

val mapSdk = MapSdk()
mapSdk.registerRoutingProviderFromForeign(AppRoutingProvider())
```

### Routing in Swift

**Swift**

```swift
import RusticMapsSDK

final class AppRoutingProvider: RoutingProvider {
    func calculateRoute(
        waypoints: [GeographicCoordinate],
        routeOptions: RouteOptions
    ) async throws -> Route {
        fatalError("call your routing service and build the route")
    }
}

let mapSdk = MapSdk()
try mapSdk.registerRoutingProviderFromForeign(routingProvider: AppRoutingProvider())
```

### Raster tiles in Kotlin

**Kotlin**

```kotlin
import uniffi.rustic_maps.MapSdk
import uniffi.rustic_maps.RasterImageFormat
import uniffi.rustic_maps.RasterTileData
import uniffi.rustic_maps.RasterTileProvider
import uniffi.rustic_maps.RasterTileResolution

class AppRasterTileProvider : RasterTileProvider {
    override suspend fun fetchRasterTile(
        tileX: UInt,
        tileY: UInt,
        zoomLevel: UByte,
        imageFormat: RasterImageFormat,
        tileResolution: RasterTileResolution,
    ): RasterTileData {
        TODO("fetch the encoded tile from your service")
    }
}

val mapSdk = MapSdk()
mapSdk.registerRasterTileProviderFromForeign(AppRasterTileProvider())
```

### Raster tiles in Swift

**Swift**

```swift
import RusticMapsSDK

final class AppRasterTileProvider: RasterTileProvider {
    func fetchRasterTile(
        tileX: UInt32,
        tileY: UInt32,
        zoomLevel: UInt8,
        imageFormat: RasterImageFormat,
        tileResolution: RasterTileResolution
    ) async throws -> RasterTileData {
        fatalError("fetch the encoded tile from your service")
    }
}

let mapSdk = MapSdk()
try mapSdk.registerRasterTileProviderFromForeign(
    rasterTileProvider: AppRasterTileProvider()
)
```

### Constraints

- Every contract has a host-implementable interface, so an app may supply any
  provider kind. The registration call names the contract:
  `registerGeocodingProviderFromForeign`,
  `registerRoutingProviderFromForeign`,
  `registerDistanceMatrixProviderFromForeign`,
  `registerSearchPlaceProviderFromForeign`,
  `registerTrafficIncidentProviderFromForeign`,
  `registerTrafficFlowProviderFromForeign`,
  `registerElevationProviderFromForeign` and
  `registerRasterTileProviderFromForeign`. Swift spells each with its
  trailing argument label.
- The implementation runs on the foreign runtime and calls back into the SDK.
  Do not block; hand long work to your own queue or coroutine scope.
- A failure must be one of the contract's `SdkError` variants. An
  unclassified failure still reaches the typed refusal at the call, and the
  SDK records it for `latestProviderFailureDiagnostics()`.
- Registration is per `MapSdk` object and replaces that slot; the other slots
  keep serving. There is no unregister call.
- The React Native package handle does not expose these interfaces, and the
  browser has no host-provided provider path. Use a Rust provider crate there.
- The provider's attribution and terms belong in the descriptor, not in the
  class.

## Route 3: a URL-template source

A plain XYZ tileset needs no provider code. Add a descriptor with the
tileset's template, zoom span and terms, then register the source once at
startup.

| Platform | Raster source | Styled vector base |
|---|---|---|
| Android | `MapSdk.registerUrlTemplateRasterTileSource(source)` | `MapView.setVectorTileSource(source)` |
| iOS | `MapSdk.registerUrlTemplateRasterTileSource(source:)` | `MapView.setVectorTileSource(_:)` |
| React Native | `MapsSdk.registerUrlTemplateRasterTileSource(source)` | `MapViewHandle.setVectorTileSource(source)` |
| WebAssembly | `registerUrlTemplateRasterTileSource(source)` | `MapView.setVectorTileSource(source)` |
| Rust | `UrlTemplateRasterTileProvider::new(source)` or `with_transport(source, transport)` | `UrlTemplateVectorTileProvider::new(source)` or `with_transport(source, transport)` |

A raster source declares its image format, pixel resolution and the
`User-Agent` its requests carry. A vector source declares the committed style
id it paints with. Templates must be absolute `http` or `https` URLs carrying
`{z}`, `{x}` and `{y}` and no other placeholders; the zoom span must not be
inverted; the style id must name a committed style. A refused source keeps
the previously serving base.

## The descriptor

One descriptor per provider lives in the shared registry. Every application, the
test server and the demo read it by name; none of them hardcode a provider.

| Field | Meaning |
|---|---|
| `provider_key` | Stable key used by every dispatch arm and every provider-keyed route |
| `display_name` | Name the settings surface shows |
| `credential_label` | Label for the credential field; blank for a keyless provider |
| `registration_available` | Whether each target registers the provider: `android`, `ios`, `react_native`, `wasm` |
| `attribution_text` | The line every shell displays wherever the imagery appears |
| `terms_note` | The provider's rate limits, caching ceiling, or usage terms |
| `settings_hint` | Where the reader creates a credential |
| `wasm_registration` | The browser dispatch key; null falls back to `provider_key` |
| `fixture_routes` | The request patterns and committed sample responses the provider's tests and the demo serve |
| `raster_url_template` | The keyless raster source, when the provider publishes one |
| `maximum_zoom_level` | The highest zoom the tileset serves |
| `url_template` | The vector tileset, when the provider publishes one |
| `glyphs_url_template` | The glyph endpoint when the tileset publishes one |
| `vector_style` | The tile schema the vector tileset carries |
| `vector_styles` | The style id, display name, theme, and attribution of each offered style |

A descriptor never carries a credential value.

## What the reader sees

The settings surface renders one credential row per descriptor, with the
provider's display name, credential label, hint, attribution and terms. The
base-map section renders the keyless raster sources and one row per vector
style. A keyless start is a first-class state: the app renders the recorded demo
until a credential is applied, and a provider-dependent call returns
`ProviderNotConfigured` verbatim. Clearing a credential removes it from memory
and the secure store, but the running SDK keeps its registered providers; a
restart returns the session to the keyless state.

## Verify the addition

1. The descriptor renders on every target's settings surface.
2. The provider's sample responses serve through its provider key; no test
   reaches the network.
3. The provider-name guard passes: the name appears only in the provider's
   crate, its sample-data directory, the descriptor and the dispatch arms.
4. The platform builds and the provider's own tests pass.

## Where to go next

- The [provider capability pages](provider-capabilities-android.md) list the
  calls each target exposes.
- [Your first map](first-map.md) shows the runtime credential path in a
  complete app.
- [Offline maps](offline-maps.md) covers the tile cache and archive routing a
  provider-backed app can add.
