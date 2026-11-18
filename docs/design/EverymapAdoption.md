# everymap-rs adoption

**Status:** design, owner-directed 2026-11-03 (PRD §6 item 49). Work: `PLAN.md` M66.
**Question it answers:** how does this SDK move its provider layer onto `angoratek/everymap-rs` without changing its own contract, and what must everymap-rs gain first?

## Outcome

One provider abstraction serves both projects. `everymap-rs` keeps its own domain API for applications that never render a map; `rustic-maps-sdk` consumes the same crates for routing, geocoding, search, traffic, raster and vector tiles, so provider knowledge is written once. The SDK's UniFFI UDL, reference applications, package identities and provider-name guard are unchanged by the switch.

## Where the two projects stand

The SDK pins `everymap-rs` **0.4.7** (`Cargo.lock`), which publishes `everymap-core`, eleven provider crates and a CLI. The 0.2.2 release (MIT, MSRV 1.86) was the adoption baseline, with five vendor provider crates (HERE, Google, TomTom, Mapbox, Radar) and ten domain traits. At that tag it:

- cannot build for `wasm32`: the client is `reqwest 0.13` over `tokio` with `features = ["full"]` and no target gating, and there is no browser transport;
- binds auth and the HTTP client to `reqwest` types, so an alternative transport cannot be plugged in;
- has no rate limiting, retry, timeout or request observer, and no way to redact credential-bearing query values;
- models routing as start/end only, with a flat step list and no legs, maneuver type, speech, lane guidance or shape polyline;
- has an untyped tile response (string format, resolution smuggled through provider extras) and no vector-tile trait or source descriptor;
- has no autocomplete, distance-matrix, elevation, offline-routing or route-capabilities trait, and models traffic as a centre-plus-radius call without flow geometry;
- ships none of the providers this SDK uses: MapQuest, OSRM, Valhalla, OSM US or URL-template sources.

This SDK therefore kept its traits and crates until everymap-rs carried the equivalent, and the switch was the last phase, not the first. The provider switch now consumes 0.4.7 on native and wasm.

## Target shape

1. `everymap-core` owns the contracts and the plumbing: transport seam, pacing, retry, observation, credentials and the domain traits.
2. `everymap-providers-*` owns vendor knowledge: MapQuest, OSRM, Valhalla, the URL-template raster and vector sources, and the OSM US/OpenFreeMap descriptors, beside the existing HERE/Google/TomTom/Mapbox/Radar crates.
3. `rustic-maps-sdk` depends on those crates directly: the registry and catalog answer with everymap provider instances, the cache wrappers wrap everymap traits, and the wasm bindings dispatch to the same providers through the browser transport. `geospatial-traits` and the duplicate provider crates leave the tree.
4. The SDK keeps what is renderer- and product-specific: the UDL contract, the offline `RMOG` graph engine, the tile schedulers, the reference applications and the registry descriptors.

## Trait mapping

| This SDK (today) | everymap-rs (target) | Work upstream |
|---|---|---|
| `GeocodingProvider` forward/reverse | `Geocoder` | Result shape widened to the SDK's quality classes; options mapped |
| `SearchPlaceProvider` settled search | `Geocoder` with a boundary | Keep |
| `SearchPlaceProvider.search_autocomplete` | new autocomplete trait | Add |
| `RoutingProvider` | `Router` | Ordered waypoints, legs, maneuver/speech/lane, shape, `route_capabilities` |
| `DistanceMatrixProvider` | new trait | Add |
| `ElevationProvider` | new trait | Add |
| `TrafficIncidentProvider`, `TrafficFlowProvider` | `TrafficProvider` | Bounding-box queries, flow geometry and congestion level |
| `RasterTileProvider` | `TileProvider` | Typed format/resolution echoed in the response |
| `VectorTileProvider` | new vector-tile trait | Source descriptor (encoding, attribution, zoom span, style hints) |
| `OfflineRoutingProvider` | new trait | Add; the `RMOG` engine stays in this repository |
| Registry, cache-then-network, tile cache | above the traits | Unchanged shape; wraps everymap traits |

## Phases

0. **Design and RFC** in everymap-rs, and this note (this phase).
1. **Core plumbing upstream:** transport seam with a reqwest implementation and a browser-fetch wasm implementation; feature-gated tokio; a wasm32 build and CI job; token-bucket pacing, retry for idempotent GETs, timeouts and a redacting request observer.
2. **Core domains upstream:** navigation-grade routing, typed raster and vector tiles, bounding-box traffic with geometry, autocomplete, matrix, elevation and offline-routing traits.
3. **Providers upstream:** MapQuest (geocoding, directions and matrix, Search Ahead, traffic, raster), OSRM, Valhalla, URL-template raster and vector, and the OSM US/OpenFreeMap descriptors, each with recorded fixtures and contract tests.
4. **Release 0.3.0** with the wasm/native CI matrix and the changelog.
5. **Switch this repository:** registry, catalog, cache wrappers, native registration and wasm dispatch use the everymap traits and providers; the duplicate crates are deleted; the UDL, reference apps and package identities do not change.
6. **Docs and gates:** provider-genericity guide, crate layout, name guard, the full platform gates and the release assembly.

Each phase is a PR against `everymap-rs` `main` and lands green on its own; the SDK keeps building against its own traits until phase 5 starts.

### Progress

- Phases 0–2 landed (transport seam, wasm build, reliability, navigation-grade routing, typed tiles, bounding-box traffic, autocomplete, matrix, elevation and offline-routing traits).
- Phase 3: OSRM, Valhalla, the URL-template raster/vector crates, MapQuest and the named OSM US/OpenFreeMap sources landed with the SDK's fixtures.
- Phase 4 complete: **0.3.0** is published to crates.io for all twelve crates and tagged `v0.3.0`. Its release run exposed a defect worth recording here: the publish script's hardcoded crate list predated the new providers, so the first tagged run published seven and skipped five. The list now derives from the workspace graph, and a manual re-run is the recovery path for a partial publish.
- Five parity gaps found in the MapQuest port, all closed:
  - pacing and retry configuration on the credential provider clients (closed: `set_rate_limiter`, `set_retry_policy`, `set_request_timeout` and `set_observer` on the generated and MapQuest clients);
  - an imperial unit choice for requests (closed: `RouteOptions::unit_system`);
  - the nine geocoding quality levels the SDK's contract exposes (closed: `SearchQuality` with the MapQuest mapping);
  - a settled place-search trait beside `Geocoder` and `AutocompleteProvider` (closed: `PlaceSearchProvider`);
  - incident coordinates on the core traffic record (closed).
- Phase 5 is in progress: `rustic-maps-core` depends on the published 0.3.0 crates, and the URL-template raster and vector sources, OSRM and Valhalla routing, and every MapQuest capability on every native and browser path are served by everymap providers through the bridge in `rustic-maps-core::everymap_bridge`. The URL-template, OSRM, Valhalla and MapQuest crates are deleted.
- Phase 5 is complete on the provider side: the SDK's `mapquest-provider`, `photon-provider`, `openstreetmap-us-vector-provider`, the URL-template crates, `osrm-provider`, `valhalla-provider` and `provider-transport` are all deleted. Photon and the OpenStreetMap US source moved to the provider layer (0.3.2), so one `reqwest` major remains and one crate family does. What is left is `geospatial-traits`: the registry, the cache wrappers and the UDL provider callbacks still speak the SDK's own contracts, with the bridge converting at the seam.
- The SDK now pins **0.4.7**, which publishes eleven provider crates: HERE, Google, TomTom, MapBox, Radar, MapQuest, Photon, OSRM, Valhalla and the URL-template raster and vector sources. The build registers all nine callable keys on native and wasm; the two URL-template crates remain base-map sources. 0.4.2 carried the bounded response excerpt and the keyless request observers; 0.4.3 carried the provider-correctness batch; 0.4.7 adds the redaction and release-integrity batch — redacted `Debug` for requests and the MapQuest configuration, a classified transport error that no longer prints a URL, a lock-free OAuth 2.0 token fetch with a timeout, one shared Radar transport-mode table, TomTom traffic bounding boxes, and the documented Mapbox permanent and batch geocoding endpoints. The Google Routes field mask was corrected in the same line, which is what made live routing work once billing was enabled.
- **The wasm runtime clock was a real gap, not a theoretical one.** The provider layer builds for `wasm32` but measured time with `std::time::Instant`, whose `now()` panics on that target. Every provider path in the browser trapped the module: the vector base, the URL-template raster sources and every MapQuest, OSRM and Valhalla request. The browser suite found it as 53 failures from one cause. `everymap-core` now reads `web-time::Instant`, the same clock the SDK's own transport uses, and the fix ships with the provider layer's next patch release.
- One conversion caveat is recorded rather than hidden: the provider layer's `TrafficFlow.speed` does not declare a unit, while the SDK field is kilometres per hour. The only provider the SDK ships does not report speeds, so no wrong value is published today; a second traffic provider reached through the bridge must report kilometres per hour until the upstream field gains a unit.
- Two cross-cutting gaps found earlier are now closed:
  - The process-wide request observer (`install_global_request_observer`) lets the SDK's typed `ProviderRequestTrace` keep its facts without each provider crate changing.
  - `TokenBucket::with_maximum_wait` restores the SDK's refusal after a ceiling, reported as `RateLimited`.

## Risks

- **Capability regression.** The everymap routing and traffic shapes are narrower today; phase 2 must land and be contract-tested before the switch, and the SDK does not adopt a provider until its capability has a parity test.
- **Wasm.** The browser target is the hard requirement. Phase 1 adds the transport seam and the wasm CI job first, so every later phase is proven on wasm from the start.
- **Two abstraction generations.** During phases 1–4 both projects carry provider code. The SDK's crates are removed only after the everymap provider passes the same fixture tests.
- **Dependency drift.** everymap pins `reqwest 0.13`/`thiserror 1.0` while the SDK uses `0.12`/`2.0`; the upstream crates align their versions as part of phase 1 so one binary carries one of each.
- **Upstream pace.** The upstream repository releases on its own schedule. The SDK pins a published version and moves only when that version carries the parity the switch needs.

## Open questions

- Whether the published provider layer keeps the current trait names or renames to the SDK's vocabulary; the plan assumes additive traits with the older names kept where they still make sense.
- Whether the SDK publishes its cache wrappers upstream or keeps them; the plan keeps them here because they are part of the SDK's caching policy.

## Provider-layer consolidation after the switch

The provider layer owns contracts, transports and vendor implementations. This
repository owns records, rendering, navigation, the offline engine and the
contract surface. Anything that restates a provider contract here is a
temporary seam with a deletion order, not a second abstraction.

| Duplication | Where | Verdict |
|---|---|---|
| Ten provider traits alongside everymap's sixteen contracts | `geospatial-traits` | **Delete.** The registry and consumers move to the everymap contracts; record conversion stays a function, not a trait. |
| Five vendor builders and credential parameters, written once per binding | native catalog, wasm dispatch | **Merge** into one crate both bindings consume (`rustic-maps-providers`); the bindings keep only their registration call. |
| Served-contract and display-name tables | native catalog, wasm dispatch, registry descriptor | **One table** in the shared crate, checked against the descriptor by a parity test (landed). |
| Record adapters (`EveryMapGeocoder`, …) | `rustic-maps-core/src/everymap_bridge.rs` | **Keep only as free conversion functions** once the registry speaks everymap contracts. A host provider over the UDL keeps its own foreign-trait seam. |
| Provider tile knowledge | URL-template sources, vendor crates | Already single-sourced. |

Staged migration: (1) the shared construction crate, landed; (2) the registry
on everymap contracts; (3) the eight `register_*_from_foreign` callbacks keep
an SDK-side trait adapted beside the registry; (4) delete `geospatial-traits`
and move its mocks into the tests that use them (`geospatial-types` stays the
record and math layer); (5) a duplicate-abstraction gate fails a new
`*Provider` trait outside the UDL seam, and the provider-name guard continues
to hold vendor knowledge to the crates and descriptors.

What must not change: the UDL stays addition-only; the registry still answers
one provider per capability with independent slots; the provider layer stays
usable without rendering and never names this SDK.
