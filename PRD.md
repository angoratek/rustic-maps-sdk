# PRD — Rustic Maps SDK

**Version:** 0.2  
**Status:** Approved and in force; decisions recorded through 2026-11-18  
**Product name:** Rustic Maps SDK

## 1. Vision

A high-performance, open-source, cross-platform native map SDK in Rust: MapQuest's
web-service APIs through vendor-neutral contracts, turn-by-turn navigation, offline
tiles and routing, generated Android/iOS/React Native bindings from one UniFFI UDL,
a raster-first Rust renderer with vector second, and first-class WebAssembly.

**References and non-goals.** Navara, Ferrostar, MapLibre Native, everymap-rs and
the Mapbox Vector Tile specification are the design references (`README.md` Credits
carries the long form). V1 excludes a static-map image API, 3D rendering,
vehicle-specific routing, voice synthesis, and a navigation app.

## 2. Users and personas

Mobile developers add maps, search, routing and navigation in Kotlin or Swift
without Rust; React Native developers get a typed Turbo Module; Rust integrators
implement 3–8 traits without touching core; maintainers test in the browser and ship.

## 3. Architecture

One Rust core behind one frozen UniFFI contract, with `MapView` as the entry point
on every target. Dependencies point upward only, from bindings to feature crates to
`geospatial-traits` and `geospatial-types`, and no trait or type crate names a
provider. `rustic-maps-native-core` is the only UniFFI binding crate, mirrored by
the generated wasm2 module; the provider layer is the published everymap crates,
which own vendor URLs, credentials, status mapping and terms. The workspace has 17
crates (`docs/Architecture.md` §2 canonical); async methods use UniFFI async through
`runtime.rs`; native uses `tokio` and wasm has none; the offline compiler never ships.

## 4. Functional requirements

### 4.1 Provider abstraction (FR-A)

The core speaks only to traits; every MapQuest API maps to at least one, and a new
vendor implements traits with no core change. Ten contracts cover geocoding,
routing, distance matrix, place search, traffic incidents, traffic flow, elevation,
raster tiles, offline routing and vector tiles; each maps to the matching MapQuest
API where one exists, elevation answers `ProviderNotConfigured`, and raster and
vector accept any `{z}/{x}/{y}` template. Records are fully named, including
`GeographicCoordinate`, `BoundingBox`, `GeocodingResult` with `geocode_quality`,
`RouteOptions`, `RouteData` with legs and steps, `Place`, `TrafficIncident`,
`TrafficFlowData` and `ElevationPoint`; the FFI mirror uses the v2 vocabulary for
some names, and `docs/Architecture.md` holds the table. Routing, geocoding and the
navigation plus rendering loop are the spine; matrix, search, traffic and elevation
follow.

**FR-B MapQuest provider.** An async client over the provider transport (`reqwest`
native, `fetch` on wasm) injects the key at runtime, paces per-plan requests with a
bucket returning `RateLimitExhausted`, and replays recorded fixtures; traffic
overlays through the renderer.

**FR-C Caching.** Cacheable GETs hash to a SHA-256 fingerprint in SQLite with a
per-category time-to-live and per-call override; the quota is an optional byte
budget or free-disk share, `CacheQuota::default()` is unbounded, and LRU eviction
follows a configured budget. Tiles cache up to 30 days by default (decision 7).

**FR-D Rendering.** `wgpu` targets Metal, Vulkan, GLES3 and WebGPU; without WebGPU a
browser runs a 2D tile grid and reports `degraded`. The camera, quadtree scheduler
and bounded texture pool serve shipped `MapView` widgets that own surface, frame
pump, gestures and camera; overlays cover markers, pins, polylines, polygons and
traffic. An MVT v2.1 decoder accepts any `{z}/{x}/{y}` source, and the MapLibre
style specification (decision 45) drives fill, line, circle and symbol layers, with
place names, road names and route shields. Seven ids ship: `carto-light`,
`carto-dark`, `openmaptiles-light`, `openmaptiles-dark`, `bright-light`,
`fiord-dark`, `positron-light`.

**FR-E Navigation.** A pure state machine moves through **Navigating**, **OffRoute**
and **Arrived**; rerouting is an event, not a phase. Fixes snap to the route with
outlier rejection, a 25 m accuracy-aware threshold requests a remote or offline
route, and `NavigationListener` carries `on_navigation_instruction`,
`on_deviation_status_change`, `on_navigation_phase_change` and
`on_speech_instruction`.

**FR-F Offline maps.** Tile packs download a region into the cache store, and
`TilePackManager` accounts for disk, so offline and network tiles are
indistinguishable. The `RMOG` archive (version 1) carries nodes, edges, turn
restrictions, lane guidance and instructions; routing is A* over mmap or wasm memory.

**FR-G WebAssembly.** `wasm32-unknown-unknown` builds cover the core, provider layer,
rendering, navigation, positioning, the offline archive and vector tiles; the cache
is native-only. One generated wasm2 module from `rustic-maps-native-core` plus the
hand-written canvas adapter form the browser surface, with
`platform/web/tests/contract_sync.mjs` asserting the stripped-UDL split and generated
package identity; Playwright and `node --test` cover it.

**FR-H Native bindings.** One UDL in `rustic-maps-native-core` uses `uniffi`
**0.31.2** (decision 27). Android ships one AAR (`com.angoratek:rustic-maps-sdk`)
with JNI, Kotlin and the `MapView`/Compose wrapper; iOS an XCFramework through
`RusticMapsSDK` and `MapNativeCoreUI`; React Native and Web are
`@angoratek/rustic-maps-sdk` and `@angoratek/rustic-maps-sdk-web` npm packages.
`get_sdk_compiled_version()` reports the version and git hash.

**FR-I Reference applications.** One app per platform demonstrates map, search,
two-field planning, simulated navigation, offline maps, settings and
provider-echoing errors, with uniform UX and idiomatic Compose, SwiftUI or React
Native code; the browser demo is the web reference app of record. All apps are
UI-test hosts, and Android, iOS and React Native run in the hosted demo.

## 5. Non-functional requirements

Performance: attach to first tile under 500 ms mid-range, input to callback under
30 ms, a metro offline route under 500 ms, and browser frames measured at 60 Hz
p95 at 1080p while panning (decision 12). Binary size: a 10 MiB per-architecture
core budget, gated in `docs/Release.md`, and an 8 MiB wasm module. Memory and
reliability: a bounded texture pool, read-only pages, typed errors and no panic
across FFI. Security and compliance: runtime-only credentials (decision 17), cache
files outside backups, descriptor attribution and terms. Quality and licensing:
fmt, clippy `-D warnings` and workspace tests gate every change; MIT OR Apache-2.0.

## 6. Decisions log (locked with the product owner; dates appear where recorded)

Implementation state and open work live in the GitHub issue tracker; the milestone
pointers in the code and docs name the feature area, not a schedule.

1. **UDL-first UniFFI** — one UDL source of truth for Kotlin, Swift and React Native; the generated wasm module mirrors it, maximizing surface parity and ubrn compatibility.
2. **Own minimal `wgpu` renderer, raster first** — not a Navara fork nor maplibre-rs reuse; vector drawing lands second on the same overlay pipeline.
3. **Custom self-describing `RMOG` graph from OpenStreetMap** — not Valhalla or GraphHopper; richness such as lane guidance grows over time.
4. **WebAssembly is a first-class target** for the browser test loop.
5. **Research crate names resolve to the layout in `docs/Architecture.md` §2**; the handwritten JNI bridge in `tmp/` is discarded because UniFFI generates it.
6. **The offline engine stands (2026-10-27)** — GraphHopper lacks Rust bindings, OSRM FFI wraps a heavy C++ backend, and Valhalla's bindings have no mobile or wasm story and exceed the size budget; a `ValhallaOfflineRoutingProvider` remains a future option.
7. **Imagery tile caching honors provider terms with a heavy default (2026-10-28)** — tiles cache up to 30 days by default, the common provider ceiling; retention and budget are developer-configurable and the descriptor declares each provider's terms.
8. **Seven named vector styles ship (2026-10-30)** — `carto-light`, `carto-dark`, `openmaptiles-light`, `openmaptiles-dark`, `bright-light`, `fiord-dark` and `positron-light`; Bright, Fiord and Positron are transcribed from `hyperknot/openfreemap-styles` with MIT / BSD-3-Clause / CC BY 4.0 attribution and a change statement, and Dark Matter is deferred as nearly blank. Styles are chosen from the map's layers control, not Settings.
9. **Themes are Auto-first (2026-10-30)** — Light/Dark/Auto, Auto by default and following the OS live; explicit choices are remembered on the device. This overturns the earlier dark-default rule.
10. **Trip planning is two-field and explicit on every platform (2026-10-30)** — one Plan route action enabled when both fields are filled, green A and red B teardrop pins, and recorded keyless routes for all 20 ordered pairs of five demo places.
11. **Native vector base parity (2026-10-30)** — one shared Rust pipeline behind additive `set_vector_tile_source`, `clear_vector_tile_source` and `vector_tile_source_status` members; place names, road names and route shields render.
12. **Browser render performance is measured, then fixed in the pipeline (2026-10-30)** — a committed harness first, then GPU-projected overlays, instanced tile buffers and off-frame fetch/decode; target 60 Hz p95 at 1080p while panning.
13. **The mobile demo runs on Appetize's Free plan behind a strict guard (2026-10-30)** — click-to-start, 3-minute sessions, a daily claim budget, a resting state and a remote kill switch; the token never reaches the page, and a session opens in its own tab with the countdown on the page because the plan cannot embed.
14. **Tutorials are generated from tested source (2026-10-30)** — one worked example with identical names and logic across four platforms, extracted from the reference apps and drift-gated, plus a terse GIS-developer path.
15. **The contract may break before 1.0 under an owner ruling (2026-10-29)** — addition-only by default; a break ships with regenerated bindings, the wasm mirror, the four reference apps and docs in one commit. The `rustic_maps` namespace and its renames are the standing example.
16. **One identity for the reference applications (2026-10-30)** — bundle-id prefix `com.rusticmaps.sdk` and display name `Rustic Maps SDK Ref App`; published package identities, generated identifiers and Appetize build ids stay unchanged. Low priority.
17. **No provider credential in git or a distributed build; one bounded hosted-demo exception (2026-10-29, extended 2026-10-30)** — credentials enter at runtime with opt-in secure storage, and automation may inject one for a session. The Appetize build alone may carry a dedicated, separately issued key, never the fixture-recording key, injected from a repository secret, read through the public registration path and never persisted; every other build stays keyless, and releases, packages, docs downloads, the repository, fixtures and screenshots never carry one (`docs/Providers.md`).
18. **A keyless start opens on the registry's styled vector base (2026-10-31)** — native apps start on a declared `keyless_base_map` (today OpenFreeMap · Bright), with the keyless raster source registered underneath; raster is the fallback, an explicit choice wins, and the browser keeps its raster start and one-tap vector switch.
19. **Four-platform parity grew through the synced wasm façade (2026-10-31)** — each missing capability became a wasm export mirrored in TypeScript and asserted by `contract_sync`; a wasm2 spike was allowed as investigation. New work follows item 27.
20. **The browser reference app reaches the v2 native flow (2026-10-31)** — the web demo gains the offline-maps card, demo plans, route edit, ETA prefix, voice guidance and camera presets where the seam supports them, so all four apps present one flow. New seam members are additive.
21. **Labels are renderer-internal (2026-10-31)** — the renderer draws styled labels and no label or symbol surface enters the UDL; the old canvas label calls retire with the hand-written façade (item 70).
22. **React Native JavaScript reaches the curated provider and navigation surface (2026-10-31)** — distance matrix and live traffic join the curated proxies with jest coverage; the generated package-root `Navigator` owns its native object and exposes `NavigationListener`, so no curated duplicate is required, while the generated ubrn bindings remain the full contract.
23. **One plan-and-drive flow and one wording on all four surfaces (2026-10-31)** — both plan fields on every shell, origin before destination, headed Plan a route with Done to dismiss, and one route option set everywhere: Car, Fastest, metric. The turn banner sits top-center with the drawer's one ETA-distance line, base-map choice lives in the layers control, React Native reads Off route, and the browser can start an offline route. Remaining divergences are exemptions in `docs/ReferenceAppUX.md` §11.
24. **Rust ships as a first-class library target (2026-10-31)** — the crates are distributable libraries with no binary product required; Rust is first in every capability matrix, download table and tutorial tab set, releases carry crate packages with checksums, and crate names are recorded at the first Rust release. Package identities are unchanged.
25. **Positioning is a Rust-owned pipeline fed by host sources (2026-10-31)** — one Rust controller validates, de-duplicates and smooths fixes for the indicator, follow camera and navigation; each host owns the OS source, permission prompt and lifecycle, and pushes into `PositionController`. Permissions never enter the rendering core.
26. **One documentation standard and one screenshot scenario (2026-10-31)** — outcome-first pages with direct voice, no insider or milestone vocabulary, generated API references and compile-checked snippets (`docs/DocsStyleGuide.md`). Screenshots are one set from one revision: the plan sheet with both endpoints, the provider section with every row's name and credential state, navigation at the first instruction at 1× playback, and the styled vector base with place labels.
27. **UniFFI aligns on 0.31.2 (2026-11-01)** — the binding crate, `uniffi-bindgen-react-native 0.31.0-6` and the wasm runtime agree, with every platform gate re-run, and WebAssembly plus React Native bindings are generated from the UDL rather than hand-maintained. The wasm2 module carries the contract surface and the canvas adapter stays hand-written; the contract stays addition-only.
28. **The reference-app rename takes the breaking Kotlin package (2026-11-01)** — `com.angoratek.rusticmaps` becomes `com.rusticmaps.sdk`, with JNI symbols, the R8 rule, RN imports, the parity script and tutorial imports moving atomically plus the 0.x minor bump. Published identities and Appetize ids stay unchanged.
29. **Rust packages ship on the docs downloads (2026-11-01)** — `cargo package` archives beside the other artifacts, with checksums and an install note; crates.io publication remains a separate decision.
30. **The browser surface is one generated wasm module (2026-11-01)** — the canvas renderer ships inside the generated wasm2 crate graph with one initialization and one provider registry, not two modules with duplicated credential state. The canvas `MapView` stays a wasm-bindgen export, named distinctly from the contract `MapView`.
31. **Releases carry checksums, and Sigstore signatures when CI runs (2026-11-01)** — per-file `.sha256` sidecars and `SHA256SUMS.txt`; Sigstore bundles come from the release workflow's OIDC identity, never a maintainer identity, and artifacts are never held back for signatures.
32. **Reference-app shared logic follows a narrow plan (2026-11-01)** — a pure `map-refapp-core` crate holds follow-camera, median, easing and scale-bar math, used by the browser and bound for Android and iOS through a refapp-only UDL; React Native waits for generated bindings (item 27). A shared app state machine stays rejected.
33. **CI runs on GitHub Actions with local gates as the fallback (2026-11-01)** — pushes are batched, every workflow cancels superseded runs, and the scripted local gate remains authoritative on its own.
34. **A provider is chosen per capability, not per application (2026-11-01)** — `configure_provider(contract, provider_key, credential)` replaces one capability's slot and `provider_registrations()` reports the serving key; `configure_provider_set(provider_key, credential)` is the whole-vendor convenience over the same catalog. Any mix is legal and the apps show the serving provider per capability; OSRM and Valhalla ship as `RoutingProvider` crates.
35. **The map carries the layer and location controls (2026-11-01)** — base map, traffic and demo scenes switch from one floating control above the attribution notice on every target, not from Settings, which keeps only credentials and capabilities the control does not cover. Traffic draws congestion-colored `TrafficFlowProvider` polylines. The my-location control requests permission and centers on the first fix; the layers panel reports state and can stop the source; each route field has a My location chip (`docs/ReferenceAppUX.md` §4.2).
36. **Directions choose mode and strategy at every level (2026-11-01)** — mode and strategy travel from the application to the provider; every app exposes Car and Walk (Car default) and Fastest and Shortest (Fastest default), with `route_capabilities()` gating unsupported choices. Truck and transit need an owner ruling because new `TransportationMode` variants break exhaustive generated switches; bicycle is expressible and offered later. Item 68 owns the per-surface text.
37. **Every reference app carries a developer console (2026-11-01)** — a read-only console over the SDK's scrubbed, levelled event stream, with a detail sheet, provider traces truncated at 512 bytes, tile and routing cache hits and misses, and a 256 KB share-sheet export; credentials and secret query values never appear (`docs/Browser.md`).
38. **Positioning feedback and route planning are audited, then aligned (2026-11-01)** — the my-location control reports every state without a Settings visit. Two labelled fields assign results to the searched field, the disabled plan action names the missing half, each field has a chip, and simulation is secondary. iOS is the pattern; every finding has a UI test and evidence records the installed version (`docs/ReferenceAppUX.md` §4.1).
39. **The browser target speaks the contract, not a private façade (2026-11-01)** — every UDL member has a wasm export, private `registerProvider` and `setRasterTileSource` calls are retired, and the capability pages state one truth: the full MapQuest set is available on every target. The generated wasm2 module implements it (item 70).
40. **API references are generated, never hand-kept (2026-11-01)** — the site build produces the five platform pages from generated bindings and the Rust rustdoc tree and fails on a missing or stale page.
41. **The screenshots page documents the product, not one screen (2026-11-01)** — each platform carries the four frames of item 26 from one revision and one scenario, with the developer console joining once it ships.
42. **Every published artifact is self-contained (2026-11-01)** — one download needs neither the repository nor an undocumented Rust toolchain; the React Native package carries its native payload, release assembly fails on a missing reference, and guides state real prerequisites.
43. **Switching a base layer leaves nothing behind (2026-11-01)** — no geometry, label or background from the previous base survives a raster/vector or vector/vector switch on any target; the browser invalidates its cached tessellation on install, clear and mid-frame clear, and native targets defer a switch during a draw.
44. **Traffic layer data is cached, bounded and released (2026-11-01)** — one fetch per settled area, a bounded least-recently-used cache keyed by the answer's tile grid and following the traffic time-to-live, no segments outside the view, and reported usage.
45. **Vector styles follow the MapLibre style specification (2026-11-01)** — the full source/layer model with expressions, filters and interpolation, so MapLibre-published styles render recognisably. MapLibre Native is the semantic reference; adapted material carries its licence in `docs/THIRD_PARTY_NOTICES.md` and `docs/THIRD_PARTY_LICENSES/`, and `docs/StyleSupport.md` holds the design.
46. **Internal docs stay concise and navigable (2026-11-01)** — live policy and current designs only, duplicates merged, superseded studies deleted, and a `docs/README.md` index whose references all resolve.
47. **A configured provider outranks the keyless raster fallback (2026-11-02)** — a page-registered `{z}/{x}/{y}` source serves only a keyless session; clearing registrations returns to it. Browser requests carry no `User-Agent` or other forbidden header, so cross-origin tile hosts answer without a rejected preflight.
48. **Android ships as one artifact (2026-11-02)** — one package carries generated bindings, per-ABI libraries and the `MapView`/Compose wrapper, so an app adds one dependency; the separate wrapper AAR is retired and every page names the single file.
49. **The provider layer is everymap-rs (2026-11-03)** — this SDK consumes the published layer directly, not through a permanent adapter. It owns the transport seam, pacing, retry, observation, navigation-grade routing, typed raster and vector-tile, traffic, search, matrix, elevation and offline contracts, and the MapQuest, OSRM, Valhalla, HERE, Google, TomTom, Mapbox, Radar, Photon and URL-template crates. This repository keeps its UDL, apps, identities and guard and has deleted its duplicate layer.
50. **Tiles fetch in parallel and zoom changes cross-fade (2026-11-03)** — a bounded pool (default 8, target 4–8) and descriptor-driven buckets keep a viewport moving; the browser drops its serial warm pass and per-tile fixture probe; the previous zoom level stays projected and cross-fades to the new one, with no blank frame.
51. **Screenshots come from credential-enabled MapQuest builds (2026-11-03)** — one revision, all four surfaces, a session-only credential synchronized to one navigation moment and never present in an image, page, fixture or repository; the per-platform vector set is part of items 26 and 41.
52. **The map surface owns the chrome; the drawer is content-first (2026-11-03)** — base map, style and overlay scenes live only in the floating layers control; Settings keeps credentials, per-capability providers, offline maps, diagnostics and theme; controls including attribution are 40 units on every target; the iOS drawer hugs the bottom safe area; demo content leaves the drawers. Search gains type-ahead everywhere (3 characters, 250 ms, 5 results, cancellation), with MapQuest Search Ahead when keyed and Photon when keyless.
53. **The location indicator is a Rust-drawn two-mode puck (2026-11-03)** — a dot with accuracy ring and heading wedge when tracking, a chevron with heading cone in navigation, with icons authored here so no third-party asset licence applies.
54. **New work advances on parallel tracks (2026-11-03)** — rendering, app simplification and autocomplete, screenshots and the provider migration each ran in a worktree and integrated as they went green; upstream provider changes land as small green pull requests.
55. **everymap-rs is the released provider layer (2026-11-04)** — the upstream transport/wasm and navigation-grade routing work merged and the release publishes the layer; this repository consumes it with its UDL, apps, identities and guard unchanged (item 49). The line is at 0.4.7.
56. **The label budget reserves slots for road names (2026-11-04)** — the z14 frame keeps a named fixed count of line-placed road-name slots after class-then-rank place ordering, so streets label without reopening placement oscillation; no per-class quota is adopted, and the documented slot count is the whole mechanism.
57. **Route shields are a renderer overlay kind sourced from `routes.ref` (2026-11-04)** — a shaped badge with the number centred, driven by shield-specific style properties; refs come from the `routes` layer for road and transit routes, not `highways.ref`, and `ref` stops painting as a road name.
58. **Provider request facts become a typed contract record (2026-11-04)** — `ProviderRequestTrace` and an optional `provider_request_trace` on `LogEvent` join the UDL with regenerated surfaces; `message_line` stays for humans and shells read typed fields.
59. **The camera is contract state, not shell state (2026-11-04)** — `camera()`, `set_camera`, `fit_bounds`, `camera_geographic_bounds` and `MapCamera.interpolate` move camera state, fit and animation into the contract, additively.
60. **React Native declarations are generated from the UDL (2026-11-04)** — declarations and ordinals generate with a round-trip gate, retiring hand-mirrored records and two JSON codecs whose ordinal drift rejected a valid overlay on iOS (items 27 and 30).
61. **Every provider in the provider layer is selectable, documented and tested (2026-11-05)** — HERE, Google, TomTom, Mapbox and Radar join MapQuest, Photon, OSRM and Valhalla on every target with descriptors, fixtures and app rows; URL-template sources stay base maps. Offline payloads come from upstream's recorded tests; vendor behaviour is tested upstream and the SDK tests the integration seam, with published pages naming every provider and no deleted type (item 49).
62. **Whole-set registration names its provider (2026-11-10)** — `set_provider_credential` is removed for `configure_provider_set(provider_key, credential)`, which registers every capability one provider serves; the tile-cache variant keeps its keyed shape, and the change shipped with regenerated bindings and migrated shells (item 49).
63. **Keyless sessions never depend on a public provider instance (2026-11-17)** — a credential-less session plans the recorded routes and resolves the demo places; OSRM, Valhalla and Photon stay explicit choices with stated attribution and terms, never silent substitutes, and an explicitly chosen keyless search or routing provider may resolve arbitrary field text (item 49).
64. **The project opens as a rewritten mirror in a new public repository (2026-11-17)** — `git filter-repo` removes retired identifiers, credentials and absolute paths before the push, so no old object, private issue, pull request or workflow log is reachable. The opening covers the repository, docs site and demos; packages stay build-from-source until the first versioned release, and planning documents publish sanitized (`docs/Release.md`).
65. **Contributions carry a DCO and security reports stay private (2026-11-17)** — contributed commits carry a CI-checked Developer Certificate of Origin sign-off and no CLA; vulnerabilities arrive through GitHub private reporting and no personal address is published; contributors disclose material AI assistance. The browser demo alone may remember a key in `localStorage` behind a warning, while platforms keep masked fields; this storage exception is separate from item 17's hosted-build exception.
66. **The location puck interpolates between fixes and does not dead-reckon by default (2026-11-05)** — one pure `map-refapp-core` animator moves the disc and ring from the drawn pose to each fix, retargets mid-motion, eases bearing along the shorter arc and clamps duration to `location-animation-duration-milliseconds` above a floor of `location-animation-minimum-milliseconds`; the first fix draws immediately, an idle pose reports `is_animating` false, and dead reckoning stays behind the reserved opt-in token. Shells drive the animator from their frame pump, and the follow camera consumes fixes, not interpolated poses.
67. **One versioned preferences record per reference application (2026-11-01)** — one JSON record namespaced by app version ignores unknown keys and drops an incompatible record on a bump. It remembers theme, base map, plan, scene, camera preset, speed, last inputs, traffic and device-location state, never transient or cache state or the credential unless remember-on-device is chosen; preferences apply before the first frame and never auto-plan, and a remembered device-location choice re-centers on the first fix next launch while permission holds. Reset clears the record and the credential store stays separate. Stores are web `localStorage`, Android `SharedPreferences`, iOS `UserDefaults` and a curated React Native call; Settings is one scrollable column in a capped panel with title and Close pinned.
68. **Directions choices carry per-surface disabled-with-reason text; the browser does not gate (2026-11-01)** — an unsupported mode or strategy is disabled with a reason on the control and never sent, and only the provider refuses a plan. Android renders `{mode} directions are not available from this provider.` and `Shortest route strategy is not available from this provider.`; iOS renders `{mode} is not offered by the serving routing provider.` and `Shortest is not offered by the serving routing provider.`; React Native renders `The serving provider does not support walking routes.` with car and shortest equivalents; the web demo shows no reason line, exposes `routeCapabilities()` but does not gate, and sends the chosen pair. Remembered unsupported values fall back, Car before Walk and Shortest to Fastest; a failed capability read disables both modes and Shortest with the provider failure verbatim.
69. **Five surface concerns stay platform-owned (2026-11-04)** — the frame pump, the platform view and surface lifetime, the OS location adapters that push into `PositionController`, one vendor-named registration dispatch arm per platform, and the vendor-neutral transport plumbing.
70. **The browser API is generated from the frozen contract (2026-11-17)** — one `wasm2` module owned by `rustic-maps-native-core` carries the generated contract and the hand-written canvas, with a compatibility façade; one module is required because objects cannot cross two ABIs. It lands before the public opening, retires the hand-written mirrors and most of the contract-sync parser, and deletes the experimental surfaces outright once green (`docs/Browser.md`).
