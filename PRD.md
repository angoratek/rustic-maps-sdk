# PRD — Rustic Maps SDK

**Version:** 0.2 (supersedes `tmp/map-sdk-rs-research/PRD.md`)  
**Date:** 2026-10-27, approved at the review gate (`PLAN.md` §0); §6 items 7–17 added 2026-10-28 → 2026-10-30  
**Status:** Approved and in force  
**Product name:** Rustic Maps SDK (adopted with the 2026-10-29 rebrand; the `map-*` crate/directory rename to `rustic-maps-*` is tracked in `PLAN.md` M17)

---

## 1. Vision

A high-performance, open-source, cross-platform **native map SDK in Rust** exposing:

1. **All MapQuest web-service APIs** (Directions, Geocoding, Search Ahead, Traffic — the Elevation API is retired by MapQuest and stays unimplemented) through a vendor-neutral abstraction layer,
2. **Turn-by-turn navigation** (online and offline),
3. **Offline maps** (raster tile packs + offline routing graph),
4. **Native bindings** for Android (Kotlin), iOS (Swift) and React Native (Turbo Module), generated from a single UniFFI UDL contract,
5. **A Rust rendering engine** — raster tiles first (option 1), Mapbox Vector Tile source via any compatible URL second (option 2),
6. **WebAssembly** as a first-class target so the team can exercise renderer, navigation and core APIs in the browser without waiting for device builds.

### 1.1 Reference projects (what we take from each)

| Project | Reuse / inspiration |
|---|---|
| `maplibre/navara` | Headless-engine topology (core decoupled from platform surfaces, `wgpu` backend); a wasm-friendly core |
| `stadiamaps/ferrostar` | Pure-functional navigation state machine; structured journey/progress model; recording-based testing |
| `maplibre/maplibre-native` | Map view lifecycle, tile management semantics, label/glyph handling reference for the vector phase |
| `angoratek/everymap-rs` | Domain-driven provider traits to onboard MapQuest, HERE, TomTom, Mapbox, Radar, Google behind the same contracts |
| `mapbox/vector-tile-spec` | Option-2 vector tile decoding target |

### 1.2 Explicit non-goals (v1)

- **No static-map image API in the trait layer** — the SDK renders; a `StaticMapProvider` would be a leaky convenience.
- **No 3D/terrain rendering** in option-1 raster mode (raster tiles are already textured imagery).
- **No vehicle-specific routing rules** at launch; standard `TransportationMode` set only (automobile, bicycle, pedestrian).
- **No voice audio synthesis** — the SDK emits text-to-speech strings; platforms own TTS engines.
- **Not a navigation app** — the refapps demo; the SDK ships libraries.

---

## 2. Users & personas

| Persona | Need |
|---|---|
| Mobile app developer (Kotlin) | Add maps, search, routing, navigation with a few lines; no Rust knowledge |
| Mobile app developer (Swift) | Same, via SPM package + XCFramework |
| React Native developer | Same, via Turbo Module with typed TypeScript surface and JSI performance |
| Provider integrator (Rust) | Implement 3–8 traits to plug in HERE/TomTom/Mapbox/Radar/Google without touching core |
| SDK maintainer | Test changes fast (browser + unit), ship per-platform artifacts from CI |

---

## 3. Architecture overview

```
                    ┌─────────────────────────────────────────────┐
                    │              platform layer                 │
                    │  Kotlin (AAR)   Swift (XCFramework+SPM)     │
                    │  RN Turbo Module (JSI/C++)   Wasm TS (web)  │
                    └──────────────────────┬──────────────────────┘
                                           │ UniFFI UDL (single contract)
                    ┌──────────────────────┴──────────────────────┐
                    │          rustic-maps-native-core            │
                    │      (uniffi objects, callbacks, errors)    │
                    └──────────────────────┬──────────────────────┘
                                           │
        ┌──────────────────────────────────┼──────────────────────────────────┐
        │                                  │                                  │
 rustic-maps-navigation        rustic-maps-rendering              everymap-rs
   TBT state machine              wgpu engine                       (provider layer:
   offline routing                raster + MVT                       contracts,
                                                                     transport,
                                                                     vendor crates)
        │                                  │
        └───────────────┬──────────────────┘
                        │
               rustic-maps-core   (provider registry, cache-then-network,
                        │          rate limiting)
            geospatial-traits / geospatial-types

  Beside this spine: rustic-maps-cache (disk cache), rustic-maps-vector-tiles,
  rustic-maps-glyphs, rustic-maps-offline-graph-archive, and the maintainer-only
  rustic-maps-offline-graph-compiler.
```

**Crate layout.** The diagram above is illustrative (it predates the vector, transport and URL-template crates). The canonical inventory — 16 crates, one line each — is the table in `README.md` §Repository layout; this section states only the rules that outlive it:

- Dependencies point upward only: `rustic-maps-native-core` → feature crates (`rustic-maps-rendering`, `rustic-maps-navigation`, provider crates) → `geospatial-traits`/`geospatial-types`. No trait or type crate depends on any provider.
- `rustic-maps-native-core` is the only UniFFI binding crate: its UDL is the single contract for Kotlin, Swift and React Native; `rustic-maps-web-bindings` is the wasm-bindgen surface that mirrors it.
- `rustic-maps-offline-graph-compiler` is a maintainer/CI tool and never ships in an artifact.
- The provider layer is the published `everymap-rs` crates (`everymap-core` and its `everymap-providers-*` vendor crates): they own the vendor URLs, credentials, status mapping, attribution and terms, and this repository keeps only the contracts its rendering, navigation and binding surfaces speak.
- Every async boundary uses `tokio` internally; async public methods cross UniFFI's async support so Kotlin/Swift/JS get suspend/async-await/Promise automatically (reactor-dependent work goes through the crate-owned runtime in `rustic-maps-native-core/src/runtime.rs`).

---

## 4. Functional requirements

### 4.1 Provider abstraction (FR-A)

The core speaks only to traits. Every implemented MapQuest API maps to at least one trait; onboarding another vendor = implementing the traits, no core changes.

| Trait | Methods | MapQuest mapping |
|---|---|---|
| `GeocodingProvider` | `forward_geocode(address)`, `reverse_geocode(coordinate)` | Geocoding API v1 (batch geocode included) |
| `RoutingProvider` | `calculate_route(waypoints, RouteOptions)` | Directions API v2 (`POST /v2/directions`), incl. leg shape geometry, maneuver list, turn restrictions |
| `DistanceMatrixProvider` | `calculate_distance_matrix(origins, destinations, transportation_mode)` | Directions API v2 matrix + one-to-many |
| `SearchPlaceProvider` | `search_autocomplete(query_text, search_boundary)`, `search_places(query_text, search_boundary)` | Search Ahead API v3 |
| `TrafficIncidentProvider` | `fetch_traffic_incidents(bounding_box)` | Traffic API v2 (`/v2/incidents`) |
| `TrafficFlowProvider` | `fetch_traffic_flow(bounding_box)` | Traffic API v2 (`/v2/links` speeds/flow) |
| `ElevationProvider` | `fetch_elevation_points(points)` | none — MapQuest retired its Elevation API; the provider answers `ProviderNotConfigured` until another source is chosen |
| `RasterTileProvider` | `fetch_raster_tile(tile_x, tile_y, zoom_level, image_format, tile_resolution)` | MapQuest static raster tiles (option-1 imagery); any `{z}/{x}/{y}` source through `everymap-providers-url-template-raster` |
| `OfflineRoutingProvider` | `load_offline_graph(graph_source)`, `calculate_offline_route(start, end, route_options)` | n/a (OSM-backed `RMOG` graph) |
| `VectorTileProvider` | `source_descriptor()`, `fetch_vector_tile(tile_x, tile_y, zoom_level)` | n/a (any `{z}/{x}/{y}` template: OpenStreetMap US Sourdough, OpenFreeMap, `everymap-providers-url-template-vector`) |

Data records (all fully-named, no abbreviations): `GeographicCoordinate`, `BoundingBox`, `GeocodingResult` (with `geocode_quality` enum, not a raw string), `RouteOptions` (`TransportationMode`, `AvoidFlags`, `UnitSystem`, `departure_time`), `RouteData` (distance, duration, `RouteLeg` list, each leg with `RouteStep` list carrying maneuver type, instruction text, lane guidance, shape polyline), `Place`, `TrafficIncident`, `TrafficFlowData`, `ElevationPoint`. These are the Rust-side names; the FFI mirror renames some of them in the v2 vocabulary (`Route`, `Place`, `ViewportSize`, `EdgeInsets`, `FrameStatistics`, …) — `docs/PublicApiV2.md` holds the rename table.

**Priority note:** FR-A + `RoutingProvider` + `GeocodingProvider` and the TBT + rendering loop are the spine; matrix/search/traffic/elevation follow immediately after.

### 4.2 MapQuest provider (FR-B)

- An asynchronous client over the provider layer's transport seam (`reqwest` natively, the browser `fetch` in wasm), with explicit API-key injection; header/parameter serialization stable and unit-tested.
- **Token-bucket rate limiter** (per MapQuest plan limits) with configurable capacity/refill; exhausted buckets surface a typed `RateLimitExhausted` error instead of hammering the API.
- Parsers are separated from the HTTP layer and tested against **recorded payload fixtures** for every endpoint (success, partial, and documented error codes).
- Traffic incidents/flow can be overlaid as data layers on the rendered map (FR-D) without touching engine internals.

### 4.3 Caching & network policy (FR-C)

- Every cacheable GET (geocode, search, static tiles) is hashed to a fingerprint (normalized URL + params, SHA-256) → SQLite store.
- TTL per response category; user-facing override per call.
- Quota: configurable as **percentage of free disk space** (default 5%) or absolute byte budget; background LRU eviction when the budget is crossed.
- Cache never bypasses the provider trait: `rustic-maps-core` applies cache-then-network around any provider.
- **Tile imagery caches heavily by default (owner ruling, 2026-10-28):** raster tiles and MVT-vector tiles keep responses up to **30 calendar days** (the common maximum under imagery-provider terms, e.g. MapQuest's) unless the provider's terms say otherwise; the tile TTL and cache budget are developer-configurable in SDK settings, and the provider contract carries its own caching terms so an integrator sets the values knowingly per provider. Disk tile packs (FR-F) are the same store's long-lived form, subject to the same per-provider ceiling.

### 4.4 Rendering engine (FR-D)

**Option 1 — raster tiles (first deliverable):**
- `wgpu` pipeline targeting Metal (iOS), Vulkan (Android), GLES3 fallback (older Android) and WebGPU on the web. There is **no WebGL2 backend**: a browser without WebGPU runs the page's own 2D tile-grid overlay and reports itself as `degraded` on the readiness ladder.
- Camera model: center coordinate, zoom, bearing, pitch, padding — animation-capable.
- Quadtree-tiled scheduler: prioritized fetch queue around the camera frustum; parent-tile fallback while children load; texture pool to avoid GPU thrash.
- Native surface wiring: the shipped `MapView` widgets — Kotlin over an Android `SurfaceView`, Swift `UIView` with a `CAMetalLayer`, an RN component over both, and a canvas-owning wasm class — each owning the surface lifecycle, frame pump, gestures and camera, following maplibre-native lifecycle patterns (attach/detach, pause/resume, memory pressure).
- Data overlays: markers (including teardrop point pins), polylines/polygons (route shape), traffic layers.
- Render correctness verified by **golden-image tests** (headless `wgpu` → PNG compare) plus interactive smoke on each platform.

**Option 2 — vector tiles (phase 2, after raster is stable):**
- Mapbox Vector Tile v2.1 decoder (protobuf → geometry) working on **any URL/template** the integrator supplies (`VectorTileSource`), independent of MapQuest.
- Render pipelines: fill, line, circle drawn on the existing overlay pipeline from three committed named styles (Bright light, Fiord dark, Positron quiet light); the symbol pipeline draws point-placed Latin labels from an SDF glyph atlas on every surface, while **road names and shields are not rendered yet**.
- Styling: a small schema (layer/type/filter/paint, zoom ranges and width stops), kept small; no full Mapbox-GL style compliance in phase 2.

### 4.5 Turn-by-turn navigation (FR-E)

- **State machine** (ferrostar-inspired, pure functions): `Idle → Navigating → Rerouting → Arrived`, plus `OffRoute`.
- Inputs: raw location samples (coordinate, horizontal accuracy, heading, speed). Processing: snap to route geometry via route-progress matching (distance-along-route with outlier rejection); a **Hidden-Markov-Model map matcher** upgrades snapping in a later iteration (per the hardening notes) — the interfaces are built for it from day one.
- Deviation: configurable off-route threshold (default 25 m, accuracy-aware) → `Rerouting` → remote (MapQuest) or local (offline graph) re-route, whichever is available first.
- Outputs via callback interface `NavigationListener`: snapped location, remaining distance/duration, **maneuver instructions with text-to-speech strings**, lane guidance, arrival.
- Online routing from MapQuest; offline routing from the embedded graph (FR-F); graceful online→offline degradation.

### 4.6 Offline maps & routing (FR-F)

- **Tile packs:** integrator/first-launch downloads a region's raster tiles (bounding box + zoom range) into the same SQLite/flat-file store used by the cache; `TilePackManager` knows what is on disk, evicts per-region, and the renderer reads tiles through the same scheduler (network tile and offline tile are indistinguishable, per FR-D).
- **Offline routing graph:** custom self-describing **RMOG** archive (format version 1) compiled from OpenStreetMap extracts by `rustic-maps-offline-graph-compiler` (nodes, directed edges with distance/duration/road class, turn restrictions, lane counts/geometry):
  ```
  archive OfflineRoutingNetworkGraph { nodes, edges, lane_guidances, instruction_strings, instruction_lookup_indices }
  ```
- Engine: **Dijkstra upgraded to A\*** (haversine heuristic) over the memory-mapped archive on native; on wasm the archive loads from an in-memory `ArrayBuffer` (no `mmap` in the browser path — a compile-time target switch, not a runtime one).
- Route→turns: the compiler pre-computes instruction strings per restricted turn; runtime generates maneuver boundaries from graph geometry. Richness improves incrementally (this is the known trade-off vs. Valhalla, accepted in the decisions log).
- Safety: mmap alignment bounds validated at load; truncated/corrupt archives fail with typed `OfflineGraphInvalid` errors, never UB.

### 4.7 WebAssembly target (FR-G)

- `wasm32-unknown-unknown` builds for: `geospatial-types`, `geospatial-traits` trait consumers, `everymap-core` and the `everymap-providers-*` vendor crates (HTTP through everymap's browser fetch transport), `rustic-maps-rendering` (WebGPU; a browser without it gets the page's 2D tile-grid overlay), `rustic-maps-navigation` (state machine + in-memory graph loading), `rustic-maps-offline-graph-archive` and `rustic-maps-vector-tiles`. There is **no wasm cache today** — `rustic-maps-cache` is native-only (bundled SQLite) and an IndexedDB store is a backlog item.
- Ships a hand-reviewed TypeScript/JS API (no UniFFI on wasm) mirrored from the UDL contract; a **contract test suite asserts the TS surface stays in sync with the UDL** (`platform/web/tests/contract_sync.mjs`, addition-tolerant by design).
- Browser test harness: **Playwright** specs over a locally served page, plus `node --test` for the contract and provider-name guards, enabling the "test often in the web browser" loop the project explicitly wants. The separate GPU frame-budget harness (`platform/web/perf/`) runs outside the suite because the suite renders under SwiftShader.

### 4.8 Native bindings & packaging (FR-H)

- One UDL contract under `rustic-maps-native-core`, generated with `uniffi` **pinned to 0.31.2** (aligned with the React Native and wasm generation lines, §6 item 20).
- Android: JNI bridging (including the `ANativeWindow` surface shim, compiled into the same library) + Kotlin sources → Gradle library → AAR (`com.angoratek:rustic-maps-sdk`), with the idiomatic Kotlin wrapper module and `MapView` built beside it (suspend functions for async, lifecycle-aware `MapView`).
- iOS: static libs → XCFramework → Swift package (`platform/ios/Package.swift` builds the products `RusticMapsSDK` and `MapNativeCoreUI` locally; a release consumes a generated `Package.swift` with a `binaryTarget(url:checksum:)` from the `swift-package` branch). The Swift wrapper provides async/await and the shipped `MapView`.
- React Native: **`uniffi-bindgen-react-native` (ubrn)** produces Turbo Module JSI bindings + typed TS (npm `@angoratek/rustic-maps-sdk`); long-running calls return Promises off the JS thread.
- Web: npm `@angoratek/rustic-maps-sdk-web` over the wasm-bindgen web target.
- Version embedding: `get_sdk_compiled_version()` surfaces crate version + git hash in every binding surface.

### 4.9 Reference applications (FR-I)

One refapp per platform demonstrating: map display, search, two-field route planning, simulated navigation with live UI, offline maps, the settings surface and provider-echoing error states. **Uniform UX language across the apps, idiomatic implementation per platform** (Kotlin app uses Compose; Swift app uses SwiftUI; RN app uses React Native primitives; the browser demo is the web refapp of record). Apps live under `platform/` and double as UI-test hosts. Android and iOS are also published to a hosted demo (`/demo/mobile`).

---

## 5. Non-functional requirements

| Area | Requirement |
|---|---|
| Performance | Cold map attach → first rendered tile < 500 ms on mid-range devices; navigation input→listener callback < 30 ms; offline route calculation < 500 ms for metro-graph queries on flagship hardware. Browser frames are measured (not asserted) by the committed harness: 60 Hz p95 at 1080p while panning is the target (§6 item 12) |
| Binary size | Rust core (raster-only) ≤ 10 MiB per architecture after strip/thin; the gate reads on the dynamic framework slice when it lands (owner ruling 2026-10-29, `PLAN.md` M10) |
| Memory | Tile texture pool bounded (configurable, default budget per platform); graph mmap pages read-only |
| Reliability | All error paths typed; no panics across the FFI boundary (catch_unwind guards at crate edges) |
| Security | Provider credentials entered at runtime, never logged and never embedded in a build (§6 item 17); cache files excluded from device backups via platform conventions |
| Compliance | Attribution and terms rendered by the default map view, read from the active provider's registry descriptor (MapQuest's line, or the ODbL/OSM notice for an OpenStreetMap source) |
| Code quality | `cargo fmt --check`, `cargo clippy -D warnings`, the workspace tests on every change (see `CLAUDE.md` and `PLAN.md`); full-name public identifiers |
| Licensing | SDK dual-licensed MIT OR Apache-2.0; dependencies audited for GPL contamination before adoption |

---

## 6. Decisions log (locked with the product owner; each item carries its ruling date)

1. **UDL-first UniFFI** — one UDL source of truth shared by Kotlin/Swift/RN; maximizes surface parity and ubrn compatibility.
2. **Own minimal `wgpu` renderer, raster first** — not a Navara fork nor maplibre-rs reuse; vector MVT lands later.
3. **Custom self-describing offline graph from OSM** — not Valhalla tiles; richness (manual-quality lane guidance) grows over time.
4. **WebAssembly added as a first-class target** for the browser test loop (this PRD's FR-G).
5. Research crate-name conflicts resolved in favor of the crate layout in §3; the handwritten JNI bridge in `tmp/` is **discarded** (uniffi generates the JNI layer).
6. **Offline engine re-affirmed (2026-10-27)** after evaluating Rust alternatives: GraphHopper has no Rust bindings (Java engine; HTTP-only via Ferrostar adapter); OSRM FFI crates (`osrm-binding`, `osrm_interface`) wrap the Boost/Lua/TBB-heavy C++ backend; only Valhalla has usable bindings (`valhalla-rs`, cxx-embedded C++ engine) but with no Android NDK/iOS/wasm build story and a binary-size cost that breaks the ≤10 MiB budget. The pure-Rust self-describing graph keeps wasm/browser test-loop parity; a `ValhallaOfflineRoutingProvider` remains a future option behind `OfflineRoutingProvider`.
7. **Imagery tile caching honors provider terms with a heavy default (2026-10-28)** — raster (and later MVT-vector) tile responses are aggressively cached by default up to a **30-day** maximum age, reflecting typical imagery-provider terms (e.g. MapQuest's); the retention/budget live in SDK settings (developer-configurable per integration), and the provider layer declares each provider's caching terms so defaults are chosen knowingly per provider (rule for `map-cache` now and `map-rendering`'s vector-tile store at M9).
8. **Vector base maps ship three named styles (2026-10-30)** — Bright (light), Fiord (dark) and Positron (quiet light), transcribed from `hyperknot/openfreemap-styles` with their MIT / BSD-3-Clause / CC BY 4.0 attribution and an explicit change statement; Dark Matter is deferred because its near-black-on-black palette measured almost blank on the demo tiles. Style selection is a dropdown in the Base map section.
9. **Themes become Auto-first (2026-10-30)** — a three-way Light / Dark / Auto choice, Auto by default and following the OS preference live, with Light and Dark overrides remembered on the device. This overturns the earlier explicit-owned, dark-default, never-read-the-OS rule in `docs/ReferenceAppUX.md` §3.
10. **Trip planning is two-field and explicit on every platform (2026-10-30)** — origin and destination fields, one Plan route action enabled only when both are populated (native assignment-triggered planning is retired), origin green A / destination red B teardrop pins as one additive overlay kind, and keyless demo routing recorded for all 20 ordered pairs of the five demo places.
11. **Native vector base parity (2026-10-30)** — Android, iOS and React Native render the same three vector styles through one shared Rust pipeline, behind additive contract members (`set_vector_tile_source`, `clear_vector_tile_source`, `vector_tile_source_status`), with point-placed Latin place labels since the symbol pipeline's first slice (round 41) and road names still to come.
12. **Browser render performance is measured and then fixed at the pipeline (2026-10-30)** — a committed measurement harness first, then world-space overlay geometry projected on the GPU, persistent/instanced tile buffers, and fetch/decode off the presenting frame; target 60 Hz p95 at 1080p while panning.
13. **The mobile demo is hosted on Appetize's Free plan behind a strict guard (2026-10-30)** — 30 streamed minutes/month, 3-minute sessions; click-to-start, a server-side daily claim budget, a resting state and a remote kill switch, with the API token never reaching the page. The Free plan also **cannot embed** a build (`isEmbeddable:false` at account level, and the API has no per-app switch), so a granted session opens in its own browser tab with the countdown on the page.
14. **Tutorials are generated from tested source (2026-10-30)** — one continuous worked example with identical names and logic across the four platforms, blocks extracted from the reference apps and drift-gated, and both a map-concepts track and a terse GIS-developer path; all tutorial chapters are rewritten.
15. **The contract may break before 1.0, under an owner ruling (2026-10-29)** — the default stays addition-only, but a breaking rename or removal is allowed before 1.0 when the owner approves it, and then it ships with regenerated Kotlin/Swift/ubrn bindings, the wasm TypeScript mirror, the four reference apps and the docs in the same commit. The standing example is M17: the type, member and record renames and the `rustic_maps` namespace (`docs/PublicApiV2.md`).
16. **One identity for the reference applications (2026-10-30)** — the Android, iOS and React Native reference apps converge on the `com.rusticmaps.sdk` bundle-id prefix and the display name `Rustic Maps SDK Ref App`, while every published package identity (listed in `CLAUDE.md`, north star), the generated binding identifiers and the Appetize build ids stay exactly as they are. Low priority; tracked as `PLAN.md` M26.
17. **No provider credential in git or in a distributed build; one bounded exception for the hosted demo (2026-10-29, extended 2026-10-30)** — reference apps take credentials at runtime through the settings surface, with opt-in platform secure storage, and automation may inject one for a session only. The owner ruled on 2026-10-30 (option b) that the Appetize-hosted build is the one artifact that may carry a credential, because a visitor comparing SDKs should see a real map and a build living inside Appetize's service is not a downloadable distribution. The exception is bounded: the key is **dedicated and separately issued** (never the fixture-recording key), injected on the build machine from a repository secret by `scripts/inject-hosted-demo-credential.sh`, read at startup through the public registration path, never persisted, and absent from every normal build, which stays keyless. It remains forbidden in a GitHub Release, an npm/Maven/SwiftPM package, a docs download, the repository, fixtures and screenshots (`docs/ProviderCredentialsAndBrowserProviders.md` §6.1).
18. **A keyless start opens on the registry's styled vector base (2026-10-31)** — overturning the round-39 raster default: when the shared registry declares a `keyless_base_map` (a provider key plus a committed style id; today OpenFreeMap · Bright), Android, iOS and React Native select that vector style at startup. The credential-less raster source is still registered underneath, because the shipped surface attaches through the raster provider contract and hides that layer while the styled base serves; the vector base is installed once the map view exists. The raster default remains the fallback for a registry that declares no `keyless_base_map`, and an explicit base-map choice (a settings selection or the demo launch argument) always wins. The browser demo keeps its raster start and its one-tap vector switch.
19. **Four-platform capability parity closes through the synced wasm façade (2026-10-31)** — the owner chose incremental growth of the hand-maintained wasm surface over a `ubrn build wasm2` migration: each missing `MapSdk`/`MapView`/`Navigator` capability is added as a wasm export, mirrored in the TypeScript façade and asserted by `contract_sync`, so the frozen UDL, the browser test loop and the hand-written canvas adapter all survive. A time-boxed wasm2 spike remains allowed as an investigation, but no migration is planned; every new wasm capability must reach parity through the same provider traits and records the native targets use. The audit is `docs/history/FourPlatformParityAudit.md`.
20. **The browser reference app reaches the v2 native flow (2026-10-31)** — the web demo is not exempt: it gains the offline-maps card, demo plans, route edit, ETA prefix, and voice guidance and animated camera presets where the seam supports them, so the four reference apps present the same flow and wording. Any capability that needs a new seam member is added additively, not worked around in the page.
21. **Labels are renderer-internal (2026-10-31)** — styled labels are drawn by the renderer (as native already does inside `map-rendering`), not exposed as a contract surface. The wasm canvas's `setLabels`/`clearLabels`/`setGlyphRange` API is a transitional seam to be retired as the wasm vector-base path moves label generation into Rust; no label/symbol surface is added to the UDL.
22. **React Native JavaScript reaches the curated provider and navigation surface (2026-10-31; navigation clarification 2026-10-31)** — distance matrix and live traffic join the curated `MapsSdk`/`MapView` proxies (with jest coverage). Navigation listeners are satisfied by the generated package-root surface: the generated `Navigator` creates its own native navigation object (it needs no shared provider handle) and exposes the contract's `NavigationListener` callbacks, so `import { Navigator } from '@angoratek/rustic-maps-sdk'` is the supported JavaScript path; the owner ruled that a curated duplicate on the proxy is not required. The generated ubrn bindings remain the full contract; the curated API is held to the same capability story as the other three targets.
23. **One plan-and-drive flow and one wording on all four surfaces (2026-10-31)** — the owner's parity directive: the peek offers both plan fields on every shell (the destination field plus a **Search an origin** entry that opens the search view armed for the origin); the search view's fields are origin first and destination second, with the heading **Plan a route** and the dismiss action **Done**; every plan — recorded, live or offline — uses the same route options (pedestrian, fastest, metric); the live guidance sits where §1 puts it (the turn banner at the map's top-center with the drawer's one ETA-distance line), and the base-map flow lives in Settings on every surface. The React Native off-route phase now reads the canonical **Off route** (the allow-listed `OffRoute` override retired); the browser's offline route is startable like the native ones. Divergences that remain are recorded exemptions in `docs/ReferenceAppUX.md` §11 (the browser has no tile cache, so no cache/tile accounting line) rather than unstated differences.
24. **Rust ships as a first-class library target (2026-10-31)** — the map renderer and core crates are usable by other Rust applications, so they are distributed as library crates (no binary product is required) and Rust is listed first in every capability matrix, download table and tutorial tab set. The public Rust surface is documented like the other platforms, and the release record carries crate packages with checksums. The existing package identities (`com.angoratek:rustic-maps-sdk`, SwiftPM `RusticMapsSDK`/`MapNativeCoreUI`, npm `@angoratek/rustic-maps-sdk`/`-web`, the `rustic_maps` namespace) do not change; the crate names are recorded when the first Rust release is cut.
25. **Positioning is a Rust-owned pipeline fed by host sources (2026-10-31)** — device positioning is a first-class capability on every target. The SDK validates, de-duplicates and smooths fixes in one Rust controller that feeds the renderer's location indicator, the follow camera and the navigation controller; each host owns the OS source, the permission prompt and the lifecycle (Android `LocationManager`/optional fused engine, iOS `CLLocationManager`, browser Geolocation API, React Native bridges, and a Rust OS/simulated source). Permissions never move into the rendering core. Research and the contract shape are in `docs/history/PositioningResearch.md`; the work is M33.
26. **One documentation standard and one screenshot scenario (2026-10-31)** — every published page follows `docs/DocsStyleGuide.md` (the Stripe/HERE bar): outcome-first structure, direct voice, no insider asides, no milestone or plan vocabulary, generated API references, and compile-checked snippets. Screenshots are recaptured as a complete set across all platforms from one revision in the same scenario (simulated navigation with banner, polyline, ETA and speed controls); a platform is never updated alone.

27. **UniFFI aligns on 0.31.2 (2026-11-01)** — overturning the 0.32.2 pin so the binding crate, `uniffi-bindgen-react-native 0.31.0-6` and the `uniffi-runtime-wasm` line agree. The contract stays addition-only; the downgrade ships with regenerated Kotlin, Swift and React Native bindings, their generation tests reconciled, and every platform gate re-run. The alignment is the prerequisite for the owner's 2026-11-01 automation ruling: the WebAssembly and React Native bindings are to be **generated** from the UDL (the `ubrn` React Native generation and its `wasm2` flavor) rather than hand-maintained, removing the hand-written wasm TypeScript façade, its mirror and the hand-curated React Native layer. This supersedes item 19's incremental-façade strategy for new work: the migration is phased — a wasm-buildable contract surface, a proven foreign-trait callback path under the single-threaded player, the canvas `MapView` kept as the hand-written browser adapter, then the page and suite migration and retirement of `contract_sync.mjs` (or its re-scoping to generated types). The frozen contract stays addition-only throughout; the native Kotlin/Swift bindings continue to be generated from the same UDL by the aligned toolchain.
28. **The reference-app identity rename takes the breaking Kotlin package (2026-11-01)** — extends item 16's M26 slices to Slice D: the shipped wrapper package `com.angoratek.rusticmaps` becomes `com.rusticmaps.sdk`, with the JNI symbol names, R8 rule, RN imports, parity script and tutorial import lines moving atomically and the 0.x minor bump the packaging policy requires. The frozen package identities listed in `CLAUDE.md` (Maven coordinate, SwiftPM names, npm names, the generated `rustic_maps` namespace) and the Appetize build ids are unchanged.
29. **Rust packages ship on the docs downloads (2026-11-01)** — the Rust crates are distributed as `cargo package` archives beside the AAR, XCFramework and wasm packages, with checksums and an install note, instead of opening a crates.io account now. A crates.io publication remains a separate decision (names and account).

30. **The browser surface is one generated wasm module (2026-11-01)** — extending item 27: the hand-written canvas renderer ships inside the generated `wasm2` module's crate graph beside the generated contract surface, with one initialization and one provider registry, rather than two wasm modules with duplicated credential state. The canvas `MapView` stays a hand-written wasm-bindgen export (UniFFI cannot express an `HtmlCanvasElement`), and its naming is kept distinct from the contract `MapView`; the page uses one module for both rendering and the contract surface.
31. **Releases carry checksums now and Sigstore signatures when CI runs (2026-11-01)** — a published set always carries per-file `.sha256` sidecars and `SHA256SUMS.txt`; Sigstore bundles attach when the release workflow (with its OIDC identity) produces them, and no maintainer-identity signing is used as a substitute. Artifacts are never held back waiting for signatures.
32. **The reference-app shared logic follows the narrow M12 plan (2026-11-01)** — a pure `map-refapp-core` crate for the follow-camera/median/easing/scale-bar math, adopted by the browser, then bound for Android/iOS through a refapp-only UDL; the React Native adoption waits for the aligned `ubrn` generation (item 20), and a shared cross-platform app state machine stays rejected. Phase 0 (single-source tokens/strings/demo plans/error text plus the parity floor) is already shipped.
33. **CI runs on GitHub Actions with free minutes and local gates as the fallback (2026-11-01)** — pushes are batched round-by-round rather than per commit, every workflow keeps concurrency cancellation so superseded runs stop, and the scripted local gate remains authoritative and must stay green on its own because the free minutes can run out again.
34. **A provider is chosen per capability, not per application (2026-11-01)** — the registry's one-slot-per-contract model becomes reachable from every target: one addition-only contract call `configure_provider(contract, provider_key, credential)` replaces the slot for a single capability, and `provider_registrations()` reports which key serves each. `set_provider_credential` stays as the one-call convenience that installs a whole vendor set, but it is a wrapper over the same catalog rather than the only path. Any mix is therefore legal — one vendor for geocoding, a URL-template source for tiles, another for routing — and the reference apps show the serving provider per capability in their settings surface. OSRM and Valhalla ship next as `RoutingProvider` crates so a demo can route through a second provider without a commercial key.
35. **The map itself carries the layer and location controls (owner-directed 2026-11-01)** — the base map, the traffic layer and the demo overlay scenes are switched from one floating control docked above the attribution notice on the map's end edge, in the same place on every target, rather than from the settings sheet; the settings sheet keeps credentials and the capabilities the control does not cover. The traffic layer draws congestion-colored flow polylines from `TrafficFlowProvider` data — the contract and the overlay pipeline already carry it — so any provider that declares traffic joins the toggle without a renderer change and without a vendor name in a shell. Device positioning starts from the round my-location control in the map control cluster, whose first tap requests permission and centers on the first fix and whose later taps re-center; the layers panel's Device location row reports the state and stops the source. The route's origin and destination fields each carry a "My location" chip that fills that field from the latest fix. Design: `docs/design/MapControlsAndTrafficLayer.md`; work: `PLAN.md` M41.
36. **Directions choose their mode and strategy at every level (owner-directed 2026-11-01)** — a route request carries its transport mode and its strategy from the application through the contract to the provider, and every reference app exposes both next to the plan action: Car and Walk on day one (Car the default), Fastest and Shortest with Fastest the default. A provider reports what it can honour through an addition-only `route_capabilities()` readback — OSRM has no shortest-path route-service equivalent, so Shortest is disabled with its reason there rather than silently ignored. Truck and public transit wait for an owner ruling because a new `TransportationMode` variant is source-breaking for exhaustive generated switches; bicycle is already expressible and offered later. Design: `docs/design/DirectionsModesAndStrategy.md`; work: `PLAN.md` M42.
37. **Every reference app carries a developer console (owner-directed 2026-11-01)** — one floating control opens a console over the SDK's own scrubbed, levelled event stream (Error, Warning, Info), with a line per event, a detail sheet showing the provider request and response trace truncated at 512 bytes, cache rows (tile and routing cache hits and misses) and an export through the platform share sheet capped at 256 KB. Trace events are emitted by the shared provider transport with the credential and secret query values never included, and the console is read-only over the SDK's stream. Design: `docs/design/DeveloperConsole.md`; work: `PLAN.md` M43.
38. **Positioning feedback and the route-planning flow are audited, then aligned (owner-directed 2026-11-01)** — the round my-location control starts the device source and reports every state without a settings visit on every target, and the origin and destination fields follow one pattern on every target: two labelled fields, results that assign to the field that searched, a plan action that names the missing half while disabled, a my-location chip per field, and simulation as a secondary action. iOS is the reference pattern. Every finding carries a UI test, and evidence records the installed build's version string so a stale install is never mistaken for a regression. Design: `docs/design/PositioningAndRouteUx.md`; work: `PLAN.md` M44.
39. **The browser target speaks the contract, not a private façade (owner-directed 2026-11-01)** — the wasm surface is the one hand-maintained binding and had drifted: it grew its own `registerProvider` and `MapView.setRasterTileSource` calls and never received `reverseGeocode`, `configureProvider`, `providerRegistrations`, `routeCapabilities` or `registerUrlTemplateRasterTileSource`. The ruling is to close the gap now and retire the legacy calls before 1.0, with the parity gate asserting that every UDL member has a wasm export. The published capability pages then state one truth: the full MapQuest capability set is available on every target. Design and work: `PLAN.md` M45.
40. **API references are generated, never hand-kept (owner-directed 2026-11-01)** — the five platform API reference pages are produced by the site build from the generated binding surfaces and the Rust rustdoc tree; the build fails rather than publishing a missing or stale page, and the site is republished at every round that changes those sources. Work: `PLAN.md` M46.
41. **The screenshots page documents the product, not one screen (owner-directed 2026-11-01)** — each platform's series carries the navigation frame plus a settings sheet, the map layers panel and the plan/directions sheet, all from one revision in the same scenario, with the developer console joining once it ships. Work: `PLAN.md` M47.
42. **Every published artifact is self-contained (owner-directed 2026-11-01)** — a developer who downloads one artifact never needs the rest of the repository and never needs a Rust toolchain the artifact does not document. The React Native package carries the prebuilt iOS framework and Android libraries its podspec and Gradle file name; the release assembly fails when a package references an artifact it does not contain; and each first-map guide states the artifact's real prerequisites. Work: `PLAN.md` M48.

43. **Switching a base layer leaves nothing behind (owner-reported 2026-11-01)** — the base map switches between raster and a vector style, and between two vector styles, without leaving any geometry, label or background from the previous base in a later frame, on every target. The browser defect was a cached tessellation keyed only on the page's own overlay list; the fix invalidates it on install, on clear and when a clear lands mid-frame, and a test asserts a post-switch frame carries no vector-base triangles. Native targets defer a switch that lands during a draw instead of refusing it. Design and work: `PLAN.md` M49.
44. **Traffic layer data is cached, bounded and released (owner-reported 2026-11-01)** — a traffic layer fetches once per settled area, answers a revisit from a bounded cache rather than the network, stops drawing segments outside the visible area, and reports its cache usage. The cache is keyed by the tile grid the provider's answer covers, evicts by least-recent use under a fixed entry budget, and follows the traffic category's time-to-live, so memory stays bounded and a revisit is faster than a first fetch. Work: `PLAN.md` M50.
45. **Vector styles follow the MapLibre style specification (owner-directed 2026-11-01)** — the renderer adopts the full source/layer model: layers with `paint`/`layout` properties, filters, the expression language and the interpolation rules the specification defines, so zoom-dependent widths, joins, dashes, text and icon sizing, placement, collision and layer order behave as a MapLibre style expects and styles published for MapLibre (the demotiles set included) render recognisably. MapLibre Native's renderer is the semantic reference; anything copied or adapted carries its licence in `docs/THIRD_PARTY_NOTICES.md` and `docs/THIRD_PARTY_LICENSES/`. Design: `docs/design/VectorStyleSpecification.md`; work: `PLAN.md` M51.
46. **The internal docs stay concise and navigable (owner-directed 2026-11-01)** — `docs/` holds live policy, current designs and current subsystem references; duplicates are merged into one home, landed designs and dated audits leave the way of a reader, and superseded studies are deleted outright because git history retains them. A `docs/README.md` index makes the folder's purpose readable in seconds and every reference from `CLAUDE.md`, `PLAN.md`, the scripts and the code resolves. Work: `PLAN.md` M52.

47. **A configured provider always outranks the keyless raster fallback (owner-reported 2026-11-02)** — a live `{z}/{x}/{y}` raster source registered by a page is a fallback for a keyless session, never the serving provider once a credential configures raster tiles; the canvas resolves the configured provider first and clearing registrations returns to the fallback. Browser requests never carry a `User-Agent` or any other forbidden header, so a cross-origin tile host answers without a preflight it would reject. Work: `PLAN.md` M53.

48. **Android ships as one artifact (owner-directed 2026-11-02)** — the published Android package carries the generated Kotlin bindings, the per-ABI Rust libraries and the `MapView`/Compose wrapper together, so an application adds one dependency; the separate wrapper AAR is retired and every download, tutorial, release note and version-support page names the single file. Work: `PLAN.md` M61.

49. **The provider layer is everymap-rs (owner-directed 2026-11-03)** — `angoratek/everymap-rs` becomes the one provider abstraction, usable outside map rendering, and this SDK consumes it **directly** rather than through a permanent adapter. Upstream gains what the SDK needs before the switch: a transport seam with a browser-fetch wasm implementation, a wasm32 build and CI, rate limiting/retry/observation, navigation-grade routing (ordered waypoints, legs, maneuvers, speech, lane guidance, shape and route capabilities), typed raster and vector-tile traits with source descriptors, bounding-box traffic with flow geometry, autocomplete, distance matrix, elevation and offline-routing traits, and the MapQuest, OSRM, Valhalla and URL-template/OSM provider crates. Work lands as PRs against everymap-rs `main` and releases as **0.3.0**; this repository keeps its UDL, reference apps, package identities and provider-name guard unchanged and deletes its duplicate provider layer once the switch lands. Work: `PLAN.md` M66.

50. **Tiles fetch in parallel and zoom changes cross-fade (owner-directed 2026-11-03)** — tile fetching runs a bounded parallel pool (named constant, default 8, target 4–8) on every surface; provider token buckets are descriptor-driven and sized so they never serialize a normal viewport; the browser frame no longer awaits a serial warm pass nor probes a same-origin fixture before every live tile. A zoom change keeps the previous level visible, projected with the current camera so it is scaled automatically, and cross-fades to the new level once its tiles are present, with no blank frame on zoom-out. Work: `PLAN.md` M65.

51. **Screenshots come from credential-enabled MapQuest builds (owner-directed 2026-11-03)** — the published set is captured from one revision on all four surfaces with a MapQuest credential injected for the session only and synchronized to the same navigation moment; the credential never appears in a published image, page, fixture or repository. The obsolete single-browser vector section becomes a per-platform vector capture set. Work: `PLAN.md` M70.

52. **The map surface owns the chrome and the drawer is content-first (owner-directed 2026-11-03)** — base map, style and overlay scenes live only in the floating layers control; Settings keeps credentials, per-capability providers, offline maps, diagnostics and theme; every map control button including attribution is 40 units on all targets; the iOS drawer hugs the bottom safe area; demo plans and scenes leave the drawers (the shared demo data stays for tests, automation and tutorials); and search gains type-ahead on every target (3 characters, 250 ms debounce, 5 results, in-flight cancellation) with MapQuest Search Ahead when keyed and Photon when keyless. Work: `PLAN.md` M67 and M68.

53. **The location indicator is a Rust-drawn two-mode puck (owner-directed 2026-11-03)** — tracking mode is a dot with an accuracy ring and heading wedge; navigation mode is a chevron with a heading cone; both are drawn by the Rust renderer so the four targets match, with the icons authored in this repository so no third-party asset licence applies. Work: `PLAN.md` M69.

54. **The new work advances on parallel tracks (owner-directed 2026-11-03)** — rendering responsiveness, reference-app simplification and autocomplete, the screenshot set and the everymap-rs migration each run in their own worktree/branch and integrate as they go green; everymap-rs changes land as small green PRs against its `main`.

55. **everymap-rs lands and releases 0.3.0 (owner ruling 2026-11-04)** — the two draft PRs (transport seam and wasm support; navigation-grade routing and typed raster/vector contracts) are merged to `angoratek/everymap-rs` `main`, the release build-out completes there, and **0.3.0** is tagged and published. This repository then consumes everymap-rs directly and deletes its duplicate provider layer, keeping the UDL, reference apps, package identities and provider-name guard unchanged (PRD §6 item 49; `PLAN.md` M66).

56. **The label budget reserves slots for road names (owner ruling 2026-11-04)** — the z14 label frame keeps a named, fixed number of slots for line-placed road names after the strict class-then-rank place ordering, so streets label at z14 without reopening the placement-history oscillation. A per-class quota is not adopted; the reserved slot count is the whole mechanism and is a documented constant (PRD §6 item 45; `PLAN.md` M39 residual).

57. **Route shields are a renderer overlay kind sourced from `routes.ref` (owner ruling 2026-11-04)** — the renderer gains a dedicated shield overlay that draws a shaped badge with the number centred inside, driven by shield-specific style properties; shield references come from the schema's `routes` layer (road and transit route refs), not from `highways.ref`. The existing symbol-text path stops painting `ref` as a road name. This closes the coverage audit's unpainted `routes` layer (PRD §6 item 45; `PLAN.md` M39/M55 residual).

58. **Provider request facts become a typed contract record (owner ruling 2026-11-04)** — an addition-only `ProviderRequestTrace` record and an optional `provider_request_trace` field on `LogEvent` join the UDL, with regenerated surfaces in the same change; the structured `message_line` remains for human-readable logs. The shells read typed fields instead of parsing the line (PRD §6 item 37; `PLAN.md` M58).

59. **The camera is contract state, not shell state (owner ruling 2026-11-04)** — `MapView` gains `camera()`, `set_camera`, `fit_bounds` and `camera_geographic_bounds`, and `MapCamera` gains `interpolate`, so the four shells stop re-implementing camera state, fit and animation. The change is additive and ships with regenerated bindings and migrated shells (PRD §6 item 15; `PLAN.md` M56).

60. **React Native declarations are generated from the UDL (owner ruling 2026-11-04)** — the React Native declaration and ordinal layer are generated with a round-trip gate, retiring the hand-mirrored records and the two JSON codecs whose ordinal drift already rejected a valid overlay on iOS. The generated surface ships with the RN build in the same change (PRD §6 items 27 and 30; `PLAN.md` M57).

61. **Every provider in the provider layer is selectable, documented and tested (owner ruling 2026-11-05)** — the five vendor providers the SDK does not yet register (HERE, Google, TomTom, MapBox and Radar) join MapQuest, Photon, OSRM and Valhalla on every target, with registry descriptors, committed fixtures and reference-app rows. The URL-template raster and vector sources stay base-map sources rather than provider rows. Offline payloads are exported from the provider layer's own recorded tests, and the SDK covers the integration seam — bridge conversion, catalog dispatch, registry mirrors and reference-app selection — while vendor request and parse behaviour stays covered upstream. Published pages name every provider truthfully and no deleted crate, trait or type (PRD §6 item 49; `PLAN.md` M75).





62. **The whole-set registration names its provider (owner ruling 2026-11-10)** — the credential-only `set_provider_credential(credential)` is removed, not deprecated, because the catalog now carries several credentialed providers and a bare credential cannot say which set it means. `configure_provider_set(provider_key, credential)` registers every capability the named provider serves from one credential and is the whole-set convenience beside the per-capability `configure_provider`; the tile-cache variant keeps its keyed shape. Few consumers exist, so the breaking change ships with regenerated bindings and migrated shells in the same change (PRD §6 item 49; `PLAN.md` M75). The provider layer's 0.4.3 release carries the provider-correctness batch that the adapted SDK consumes.

63. **Keyless sessions never depend on a public provider instance (owner ruling 2026-11-17)** — a session without a credential plans the recorded demo routes and resolves the demo places; OSRM, Valhalla and Photon remain explicit provider choices, and the reference apps never substitute one of them silently for the keyless default. Choosing one explicitly states its attribution and terms through the registry descriptor, and an explicitly chosen keyless search or routing provider may resolve arbitrary field text. This closes the queued public-instance budget questions without a rate or volume agreement (PRD §6 item 49; `PLAN.md` M64/M68, §5 items 19 and 23).

64. **The project opens as a rewritten mirror in a new public repository (owner ruling 2026-11-17)** — the development history is rewritten with `git filter-repo` to remove every retired identifier, credential and absolute local path, then pushed to a newly created public repository so no pre-rewrite object stays reachable by SHA and no private issue, pull request or workflow log becomes public. The opening covers the repository, the published documentation site and the browser and mobile demos; platform packages and the Rust crates stay build-from-source until the first versioned release, and the downloads page states that truth. Internal planning documents are published sanitized: process vocabulary, billing state, credential operations and absolute paths leave, while decisions and open questions remain in project language. Work: `PLAN.md` M82 and `docs/OpenSourceReadiness.md`.

65. **Contributions carry a DCO and security reports stay private (owner ruling 2026-11-17)** — every contributed commit carries a Developer Certificate of Origin sign-off checked in CI; no CLA applies. Vulnerability reports arrive only through GitHub private vulnerability reporting, and no personal address is published. Contributors disclose material AI assistance; the policy never bans it. The browser demo is the single documented credential exception: it may remember a key in `localStorage` behind an explicit warning, while every platform keeps a masked field and no show-key affordance. Work: `PLAN.md` M82.

66. **The browser API is generated from the frozen contract (owner ruling 2026-11-17)** — the WebAssembly target adopts one `uniffi-bindgen-react-native` `wasm2` module owned by `rustic-maps-native-core`, carrying the generated contract surface and the hand-written canvas, and the page keeps a compatibility façade over the generated API. The prototype proved the whole contract generates (dictionaries, enums, the error union, async members, foreign traits) and that wasm-bindgen canvas exports survive in the same staged module; objects cannot cross the two ABIs, so one module is required. The migration lands **before** the public opening, retires the hand-written contract mirrors, the type mirror, the façade generator and most of the contract-sync parser, and is gated by the full browser and platform suites. Design: `docs/design/WebFacadeGeneration.md`; work: `PLAN.md` M83.
