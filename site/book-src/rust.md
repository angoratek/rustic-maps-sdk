# Use the SDK from Rust

The map SDK is a Rust workspace. The same crates that back the Android, iOS,
React Native and WebAssembly bindings can be used directly by a Rust
application: the renderer, the camera and projection math, the provider
traits, the navigation controller and the offline routing graph all live in
Rust and have no platform dependency.

## Crates

| Crate | What it provides |
|---|---|
| `rustic-maps-rendering` | The camera and projection model, the raster tile scheduler, overlays, labels and the headless `wgpu` renderer |
| `rustic-maps-core` | The provider registry, the provider bridge, offline routing and the shared SDK operations |
| `geospatial-types` | Coordinates, tiles, records and the shared error type |
| `geospatial-traits` | The SDK's own provider traits, the seam the published provider layer is adapted to |
| `everymap-core` | The provider contracts, the portable transport seam, pacing, retry and request observation |
| `everymap-providers-*` | The vendor implementations: MapQuest, Photon, OSRM, Valhalla and the URL-template raster and vector sources, beside HERE, Google, TomTom, Mapbox and Radar |
| `rustic-maps-navigation` | Route walking, instruction generation, deviation and reroute policy |
| `rustic-maps-positioning` | Position validation, course smoothing and the simulated position source |
| `rustic-maps-vector-tiles` / `rustic-maps-glyphs` | Mapbox Vector Tile decoding and glyph-atlas packing |
| `rustic-maps-offline-graph-archive` | Reading compiled offline routing graphs |

The crates are consumed from the SDK source today; a registry package follows
the first Rust release.

## Install from the source tree

The Rust package is the workspace source: the workspace manifest and lock file,
every workspace crate, the platform package sources and build scripts, the
documentation sources, and both license files. The provider implementations
come from the published `everymap-*` crates the lock file pins.

1. Get the repository source tree.
2. Point a path dependency at the crate you use, as the
   [Downloads](downloads.md#rust) page shows.

Cargo builds the crates your application uses from source, including their
tests, and downloads pinned dependencies from their own sources. Rust 1.89 or
newer is required, and the workspace uses the 2024 edition. The source tree
contains no compiled libraries, generated bindings, provider credential,
Sigstore bundles or Git history.

## Camera and projection without a GPU

Camera math and projection are pure Rust and run anywhere:

```rust
use geospatial_types::GeographicCoordinate;
use rustic_maps_rendering::{DEFAULT_FIELD_OF_VIEW_DEGREES, MapCamera, ViewportPixels};

let center = GeographicCoordinate::from_degrees(38.5766, -121.4944)?;
let camera = MapCamera::new(center, 12.0, 0.0, 0.0);
let viewport = ViewportPixels::new(1280.0, 720.0)
    .map_err(|detail| format!("invalid viewport: {detail}"))?;

let screen_point = camera.project_to_screen(
    &center,
    &viewport,
    DEFAULT_FIELD_OF_VIEW_DEGREES,
);
```

## Rendering a frame

`rustic-maps-rendering`'s headless renderer draws into an offscreen `wgpu`
target: create a `HeadlessTileRenderer` over a `GraphicsContext`, resolve the
camera's visible tiles through a `TileScheduler`, and read the frame back as
RGBA bytes. It is the same renderer the native surfaces use, and the browser
binds the same modules through a WebAssembly canvas adapter.

## Providers

Vendor providers implement the contracts in `everymap-core` — geocoding,
search, type-ahead, routing, a distance matrix, traffic, raster and vector
tiles. The SDK adapts them at `rustic_maps_core::everymap_bridge` and registers
them in the `rustic-maps-core` registry. A host implementing a provider inside
its own application implements the `geospatial-traits` traits directly. See
[Add a provider](adding-a-provider.md) for the descriptor fields and the
registration path.

## Positioning

`rustic-maps-positioning::PositionController` validates the fixes your OS source
reports, smooths their course and tracks staleness, so a Rust application can
feed the same stream to the renderer's location layer, a follow camera and
the navigation controller. `SimulatedPositionSource` walks a route for tests
and demos:

```bash
cargo run -p rustic-maps-positioning --example simulated_walk
```
