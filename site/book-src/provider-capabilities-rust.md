# Provider capabilities in Rust

The Rust crates are the SDK's implementation layer. Providers come from two directions:
the published provider layer carries the vendor implementations, and the SDK adapts each
one to its own records at a single seam. The four application targets bind the same core
through the contract.

The repository publishes the Rust API reference but no Rust application package or
public `MapsSdk`/`MapView` object; the downloadable artifacts are the Android, iOS,
React Native and WebAssembly targets. Building inside the repository gives you the
contracts, registry, provider crates, renderer, navigation reducer and offline archive
APIs directly.

## What you need

- Rust 1.89 or newer, with the repository's Cargo workspace.
- A provider credential only for the live provider APIs, read at runtime and never
  stored in the crate, a sample response or a log.
- A deployment base URL instead of a credential for the keyless providers OSRM, Valhalla
  and Photon, when you do not use their public endpoints.

## Providers available in Rust

The provider layer publishes eleven vendor crates. This build catalogs nine keys and
serves the three tile sources as base maps.

| Provider | Key | Credential | Form in Rust | Serves | Attribution line | Terms |
|---|---|---|---|---|---|---|
| MapQuest | `mapquest` | API key | `everymap-providers-mapquest` | Geocoding, search and suggestions, routing, distance matrix, traffic incidents and flow, raster tiles | Map data © OpenStreetMap contributors (ODbL) — tile imagery © MapQuest | MapQuest terms apply: rate limits and a 30-day raster-tile caching ceiling. |
| HERE | `here` | API key | `everymap-providers-here` | Geocoding, routing, traffic incidents and flow, vector tiles with HERE Base | © 20XX HERE, XXX | HERE terms apply: per-plan transaction limits and a caching policy that bounds stored results. |
| Google Maps | `google` | API key | `everymap-providers-google` | Geocoding, routing | Map data © Google | Google Maps Platform terms apply: billing per SKU, caching restrictions, and no permanent storage of most content. |
| TomTom | `tomtom` | API key | `everymap-providers-tomtom` | Geocoding, routing, traffic incidents and flow, raster tiles | © TomTom | TomTom terms apply: per-plan request quotas and a caching policy for stored tiles and results. |
| Mapbox | `mapbox` | Access token | `everymap-providers-mapbox` | Geocoding, routing, vector tiles with Mapbox Streets | © Mapbox © OpenStreetMap contributors | Mapbox terms apply: usage-based billing and attribution of Mapbox and OpenStreetMap wherever the data is shown. |
| Radar | `radar` | API key | `everymap-providers-radar` | Geocoding, routing | © Radar | Radar terms apply: per-plan request quotas and no permanent storage of geocoding results. |
| Photon | `photon` | None, or a deployment URL | `everymap-providers-photon` | Geocoding, search and suggestions | Map data © OpenStreetMap contributors (ODbL) — search by Photon | The public Photon instance is offered for reasonable, non-commercial use; a self-hosted deployment follows its operator's terms. |
| OpenStreetMap Standard | `openstreetmap` | None | `everymap-providers-url-template-raster` with the registry template | Raster tiles | Map data © OpenStreetMap contributors (ODbL) — raster tiles © OpenStreetMap contributors | OpenStreetMap tile usage policy applies: low-volume use only, with attribution. |
| OpenStreetMap US | `osmus` | None | `everymap-providers-url-template-vector` with `openstreetmap_us_vector_source()` | Vector tiles with Carto Light and Carto Dark | Map data © OpenStreetMap contributors (ODbL) — vector tiles © OpenStreetMap US | OpenStreetMap US tileservice terms apply: free for low-volume, non-commercial use with attribution. |
| OpenFreeMap | `openfreemap` | None | `everymap-providers-url-template-vector` with `openfreemap_vector_source()` | Vector tiles with Bright, Fiord, and Positron | Map data © OpenStreetMap contributors (ODbL) — vector tiles © OpenFreeMap and OpenMapTiles | OpenFreeMap terms apply: free public use with attribution; bulk downloads are limited to 3,000 tiles or 50 MB per download. |
| OSRM | `osrm` | None, or a deployment URL | `everymap-providers-osrm` | Routing | Map data © OpenStreetMap contributors (ODbL) — routing by OSRM | Project OSRM's public demo server is capped at one request per second; a self-hosted deployment follows its operator's terms. |
| Valhalla | `valhalla` | None, or a deployment URL | `everymap-providers-valhalla` | Routing | Map data © OpenStreetMap contributors (ODbL) — routing by Valhalla | The FOSSGIS public demo server allows one request per second with an identifying User-Agent; a self-hosted deployment follows its operator's terms. |

All twelve rows above are catalogued by this build. HERE and Mapbox also implement
vector tiles in the provider layer, but the SDK's vector base is a URL-template source
rather than a registry slot, so those implementations are not selectable through
`configure_provider`.

Each vector style also carries its own attribution line. The style ids are `carto-light`
and `carto-dark` for OpenStreetMap US, `bright-light`, `fiord-dark`, and
`positron-light` for OpenFreeMap, `omv-light` for HERE, and `streets-light` for Mapbox.
The vector source's status readback reports the active style's line.

HERE and Mapbox paint their vector tiles with the style document the provider publishes.
The SDK fetches that document with the registered credential and keeps it in the
installed tile cache for up to thirty days, or for the session when no tile cache is
installed, so no style file ships with your application.

Whether a provider is reachable from a shipped target depends on that target's dispatch
table. The browser registers all nine keys: MapQuest, HERE, Google Maps, TomTom, Mapbox,
Radar, Photon, OSRM and Valhalla; the vector base reaches every target through the
generic URL-template vector provider.

## What you can call

| Capability | Rust surface |
|---|---|
| Forward and reverse geocoding | `everymap_core::domains::search::Geocoder`; `MapQuestGeocoder`, `HereGeocoder`, `GoogleGeocoder`, `TomTomGeocoder`, `MapBoxGeocoder`, `RadarGeocoder`, `PhotonGeocoder` |
| Suggestions and search | `PlaceSearchProvider`; `MapQuestPlaceSearchProvider`, `PhotonPlaceSearchProvider` |
| Type-ahead | `AutocompleteProvider`; `MapQuestAutocompleteProvider`, `PhotonAutocompleteProvider` |
| Routing with options | `Router`; `MapQuestRouter`, `HereRouter`, `GoogleRouter`, `TomTomRouter`, `MapBoxRouter`, `RadarRouter`, `OsrmRouter`, `ValhallaRouter` |
| Route capabilities | `Router::route_capabilities()` |
| Distance matrix | `DistanceMatrixProvider`; `MapQuestDistanceMatrix` |
| Traffic incidents and flow | `TrafficProvider`; `MapQuestTrafficProvider`, `HereTraffic`, `TomTomTraffic` |
| Elevation | `ElevationProvider`; no shipped implementation |
| Raster tiles | `RasterTileProvider`; `MapQuestRasterTileProvider`, `TomTomTileProvider`, `UrlTemplateRasterTileProvider` |
| Vector tiles | `VectorTileProvider`; `UrlTemplateVectorTileProvider` |
| Offline routing | `OfflineRoutingProvider`, `ArchiveOfflineRoutingProvider`, `calculate_offline_route_from_archive_bytes` |
| Navigation | `rustic-maps-navigation`: reducer, policies, `RouteWalk`, `SimulatedTripPace` |
| Rendering | `rustic-maps-rendering`: `MapCamera`, headless renderer, surface renderer, committed vector styles |

The workspace crates behind these surfaces: `everymap-core` holds the provider
contracts, the portable transport seam, pacing, retry and request observation;
`geospatial-types` holds coordinates, bounding boxes, route records, tile data and
`SDKError`; `geospatial-traits` holds the SDK's own provider traits, which a host
implements when it supplies a provider inside its application. `rustic-maps-core` holds
`ProviderRegistry`, the provider bridge, cache-then-network policy, offline routing and
the fallback; `rustic-maps-cache` holds the disk response cache and byte budget;
`rustic-maps-rendering` holds the `wgpu` renderer, camera and vector styles;
`rustic-maps-vector-tiles` is the MVT decoder; `rustic-maps-glyphs` is the glyph reader,
range cache and atlas packer; `rustic-maps-navigation` is the navigation reducer,
policies, speech pacing and route walk; `rustic-maps-offline-graph-archive` reads
compiled routing archives; `rustic-maps-offline-graph-compiler` compiles OpenStreetMap
into an archive. `rustic-maps-native-core` is the UniFFI binding crate behind the native
targets; `rustic-maps-web-bindings` is the WebAssembly binding crate.

## Register a provider

A vendor provider is written against the provider layer's contracts in `everymap-core`
and presented to the SDK through the bridge in `rustic_maps_core::everymap_bridge`. A
host that implements a provider inside its own application implements the SDK's own
traits from `geospatial-traits` and registers the implementation directly.

```rust
use std::sync::Arc;
use everymap_providers_osrm::{OsrmProviderConfiguration, OsrmRouter};
use rustic_maps_core::everymap_bridge::EveryMapRouter;
use rustic_maps_core::ProviderRegistry;

fn registry_with_osrm() -> ProviderRegistry {
    let router = OsrmRouter::new(OsrmProviderConfiguration {
        base_url: "https://router.project-osrm.org".to_string(),
        ..Default::default()
    });
    let mut registry = ProviderRegistry::new();
    registry.register_routing_provider(Arc::new(EveryMapRouter::new(Arc::new(router))));
    registry
}
```

`ProviderRegistry` holds one optional slot per contract. Registering replaces that slot;
`require_*` answers `ProviderNotConfigured` with the contract name when a slot is empty.
A keyed provider is also reachable through the SDK's `MapSdk::configure_provider`, which
resolves the key against the compiled catalog and registers exactly the requested
capability.

## Use a URL-template source

A URL-template raster source takes the registry's template, the attribution and terms
its policy requires, the zoom span, the image format, the resolution and an optional
request budget.

```rust
use everymap_core::domains::tiling::{RasterImageFormat, RasterTileResolution};
use everymap_providers_url_template_raster::{
    UrlTemplateRasterTileProvider, UrlTemplateRasterTileSource,
};

fn open_street_map_raster_source() -> Result<UrlTemplateRasterTileProvider, everymap_core::EveryMapError> {
    UrlTemplateRasterTileProvider::new(UrlTemplateRasterTileSource {
        url_template: "https://tile.openstreetmap.org/{z}/{x}/{y}.png".to_string(),
        source_name: "openstreetmap".to_string(),
        attribution_text: "Map data © OpenStreetMap contributors (ODbL) — raster tiles © OpenStreetMap contributors".to_string(),
        terms_text: "OpenStreetMap tile usage policy: low-volume use with attribution.".to_string(),
        minimum_zoom_level: 0,
        maximum_zoom_level: 19,
        image_format: RasterImageFormat::Png,
        tile_resolution: RasterTileResolution::Standard,
        subdomains: Vec::new(),
        headers: Vec::new(),
        pacing: None,
    })
}
```

The constructor validates the declaration and the template before any request is sent,
so a malformed source is refused at construction rather than on the first tile.
`with_transport` takes the same declaration plus an injected transport.

## Route without the network

The offline routing archive is the self-describing container compiled from an
OpenStreetMap extract. The helper takes the archive bytes, so the same call serves every
target.

```rust
use geospatial_types::{GeographicCoordinate, SDKError};
use rustic_maps_core::calculate_offline_route_from_archive_bytes;

async fn route_over_archive(archive_bytes: &[u8]) -> Result<(), SDKError> {
    let start = GeographicCoordinate {
        latitude: 0.001,
        longitude: 0.0,
    };
    let end = GeographicCoordinate {
        latitude: 0.001,
        longitude: 0.002,
    };
    let route =
        calculate_offline_route_from_archive_bytes(archive_bytes, &start, &end).await?;
    let _ = route.distance_meters;
    Ok(())
}
```

The request uses the archive profile's default options: automobile, fastest, metric. A
corrupt archive returns `OfflineRoutingFailure`; a coordinate outside the graph's snap
tolerance or a pair the graph cannot connect is refused, never guessed.

## What you should see

After registering a provider, `require_*` answers the implementation and the contract
calls run through it. An empty slot returns `ProviderNotConfigured` with the contract
name. A keyed registration answers the provider's display name and key through the
registration readback. The offline helper returns a `RouteData` on a connected pair, and
a typed `SDKError` on a corrupt archive or an unconnectable pair.

## Constraints

- A provider crate implements the contracts it serves and returns the contract's error
  types. It owns its vendor URL, credential handling, status mapping, attribution and
  terms.
- Provider types must be `Send + Sync`; trait methods are async.
- Use the provider layer's shared client for HTTP so the native and browser builds share
  one request, retry and rate-limit path.
- A new provider key becomes reachable from a shipped target through that target's
  dispatch table and a registry descriptor. A contract with no registration call on the
  FFI surface needs an addition-only contract change before a native or browser
  application can select it.
- Tests never reach the network. Provider responses in tests come from committed sample
  data served under the provider's key.

## Where to go next

- [Add a provider](adding-a-provider.md) for the descriptor and dispatch work around a
  provider crate.
- [The FFI contract](udl-contract.md) for the surface the application targets bind.
- [Offline maps](offline-maps.md) for the archive and tile-cache story.
