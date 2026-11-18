# Web façade retirement (M83 slice 4)

**Status:** design decision, 2026-11-17. The page loads the generated `wasm2` package (`0bde0dd4`); `rustic-maps-native-core` owns the ABI and mirrors raster/vector registrations into the canvas slots on wasm (`provider.rs`'s `set_shared_*`); `rustic-maps-web-bindings` is linked into that module as `browser_canvas`.

## Outcome

Delete every hand-written **contract mirror**; keep the canvas and recorded-tile helpers hand-written. Two browser-only seams remain: the fixture endpoint override and the registration clear. Close the endpoint gap with (b), the smallest change; (a) is the contract-clean upgrade.

## Retirement matrix

The package exposes 62 top-level names (47 functions, 12 classes, 3 enums).

| File (lines) | Use | Generated equivalent | Verdict |
|---|---|---|---|
| `exports.rs` (1,145) | `MapCamera`, `MapsSdk.create`, version; `initializeSdk` only in sync; `RouteSimulator` unused; projection already generated | all in the UDL | **DELETE ~715**; keep `MapCamera` + error/object helpers (~430) |
| `camera_interaction.rs` (83) | page aliases | `MapCamera` pan/zoom/rotate methods | **DELETE** |
| `navigation.rs` (1,187), `positioning.rs` (187) | playground, walkthrough, navigation/device specs | UDL `Navigator`/`PositionController` + typed listeners | **DELETE** |
| `diagnostics.rs` (673), `diagnostics_filter.rs` (75) | console, provider diagnostics | `MapSdk` subscribe/flush/excerpt; `new MapSdk()` installs the subscriber | **DELETE** |
| `live_providers.rs` (2,048) | provider calls/config/readbacks | `MapSdk` members | **DELETE ~1,500**; keep ~550 (slots, revision, override, clear) |
| `generated_contract_field_names.rs` (291) | canvas + vector layer | none (ubrn emits no Rust constants) | **KEEP-MOVE-TO-CANVAS** |
| `shared.rs` (166) | canvas tile URL/zoom, validation | partial | **KEEP** |
| `refapp.rs` (600), `refapp_math.rs` (213) | map controls, cluster/indicator | none (not in the UDL) | **KEEP-WASM-ONLY** |
| `canvas.rs` (6,702); `vector_layer.rs` (357), `vector_source_fields.rs` (468), `web_camera.rs` (366) | canvas, goldens, overlays, vector base, camera store | not expressible | **KEEP-MOVE-TO-CANVAS**; drop the `MapsSdk` arg/field |

Provider calls/config/readbacks, diagnostics, navigation, positioning and pure helpers move; `refapp` math and every canvas class do not.

## Generated replacements and differences

| `_bg.js` today | Generated | Difference |
|---|---|---|
| `MapsSdk.create()` (async) | `new MapSdk()` (sync) | constructor; drop the compiled-version hand-off |
| `configureProvider(string, …)` | `MapSdk.configureProvider(ProviderContract, …)` | string → numeric enum |
| `configureProviderSet`, `providerRegistrations`, `routeCapabilities` | same names on `MapSdk` | contract/modes are enums |
| geocoding/search, route, matrix, elevation calls | `MapSdk` members | `Place` nests `coordinate`; options/coordinates are records; `PlaceRow` flattens today |
| `fetchTrafficIncidents/Flow`, `fetchRasterTile(zoom,x,y)` | records; `fetchRasterTile(x,y,zoom,format,resolution)` | adapters build the record; extract `imageBytes` |
| `trafficFlowCacheStatistics` | `MapSdk.trafficFlowCacheStatistics()` | `u64` → **bigint** (8 fields) |
| `subscribe/flushRecentDiagnosticEvents` | `MapSdk` + `SdkDiagnosticEventListener` | flush delivers via listener, returns count; adapter collects an array |
| `Navigator` single callback, `startNavigation(route, "metric")` | typed topics, `UnitSystem`, three policies | largest adapter: listener classes + `default_*_policy()` |
| `PositionController` string status; errors `{code,message}` | record/enum; tagged `SdkError` `.inner.detail` | adapters; existing `adapted_failure` covers errors |

71 generated places use `u64`; the façade converts only displayed/compared values.

## The endpoint gap

`configure_provider`/`configure_provider_set` pass a **credential only**. Keyless providers (osrm, valhalla, photon) take the deployment address in that slot (`deployment_base_url_or_default`); MapQuest and shared vendors (here, google, tomtom, mapbox, radar) hardcode the public endpoint. `setProviderBaseUrl` (web-bindings) stores an override `install_provider` applies: `build_vendor(..., Some(url))` for shared vendors, `MapQuestProviderConfiguration.base_url` for MapQuest.

Fixture specs calling it: `live_provider`, `navigation_listeners`, `provider_contract_surface` (MapQuest / registry `providers[0]`, `test-key`); `provider_console_provider_matrix`, `provider_request_diagnostics`, `provider_console_panel`, `provider_console_response_excerpt`, `place_suggestion_golden`, `developer_console` (every credentialed geocoding provider); `keyless_place_provider`; `autocomplete`; `reference_flows`.

**Answer:** not every case is expressible as `configure_provider(capability, key, endpoint)`. Keyless specs are; every credentialed spec needs a synthetic credential **and** an endpoint, and no contract member carries both.

- **(a) Addition-only member.**
  ```
  [Throws=SdkError]
  void configure_provider_endpoint(
      ProviderContract contract, string provider_key,
      string credential, string base_url);
  [Throws=SdkError]
  void configure_provider_set_endpoint(
      string provider_key, string credential, string base_url);
  ```
  `ProviderCatalogEntry.install` becomes `fn(&MapSdk, ProviderContract, &str, Option<&str>)`; `configure_provider` passes `None`; the endpoint form validates http(s)+host and passes `Some`, which MapQuest sets on its configuration and shared vendors pass to `build_vendor`. Checklist: both UDL files (split test `wasm_contract_split.rs`) → `records.rs` (no new record) → `provider.rs` → regenerate Kotlin/Swift (`build-android.sh`, `build-ios.sh`) and RN (`build-rn.sh`) → generated wasm TS + generation gate → `PublicApiV2.md`, `ProviderCredentialsAndBrowserProviders.md`, API reference. Addition-only; real behaviour for self-hosted/enterprise HERE, TomTom, Mapbox Atlas and proxies, not a test-only seam.
- **(b) Keep the browser override (recommended, smallest).** Keep `setProviderBaseUrl`; expose its table as `#[cfg(wasm32)] pub(crate) fn base_url_override(provider_key)` and read it from native-core's two credential-only install paths on wasm. ~30–50 lines, no UDL/regeneration churn; every fixture spec closes unchanged. It stays browser-only and test-oriented, exactly as M83 records.
- **(c) Stateful `set_provider_endpoint`.** One addition, but endpoint state leaks between registrations; rejected.

Companion seam: `clearProviderRegistrations` cannot clear the generated registry; the façade keeps it for the canvas slots and replaces its `MapSdk` instance, which drops the native registry.

## Deletion count

- Rust: ≈4,750 lines leave the crate (14,672 → ≈9,900): the exports contract half, navigation, positioning, diagnostics + filter, camera interaction, the provider-call half of `live_providers`, unused field-name constants (≈200 move into `canvas.rs`).
- Web: `types/rustic_maps.d.ts` (1,157) and `rustic_maps_surface.d.ts` (183) deleted, replaced by generated package TS plus one ~200-line `browser_canvas.d.ts`; `generate-web-facade.mjs` (1,202) + test (209) deleted; `contract_sync.mjs` 1,037 → ~120 (UDL split + package identity).
- `build-wasm.sh` (79) retires into `build-wasm2-package.sh` (generate + package + `wasm-opt` + freshness); `platform/web/pkg` (6.3 MB), `check-wasm-package-freshness.mjs` (198) and `global_setup.mjs`'s legacy branch retire; `package-web-npm.sh`, `build-docs-site.sh`, `check-doc-snippets.mjs`, `generate-api-reference.mjs` retarget to `platform/web/generated`.

## Migration order and gates

1. **Endpoint bridge (b)**: override accessor + native-core install reads it. Gates: fmt, workspace clippy, wasm-closure clippy, native-core/web-bindings tests, `build-wasm2-package.sh`, `live_provider`, `provider_contract_surface`.
2. **Page façade to the generated surface**: `MapSdk`, provider calls, diagnostics listener, navigation/positioning adapters, error/bigint shapes. Gates: full Playwright, `contract_sync`, `wasm_readiness`, `generated_wasm_package`, `golden_compare`.
3. **Delete the mirrors and retire the generator/package**: exports contract half, navigation, positioning, diagnostics, camera interaction, `live_providers` provider calls, types, `generate-web-facade`, `pkg`, `build-wasm.sh`. Gates: full pre-commit gate, wasm32 closure clippy, `generated_wasm_package`, `wasm_readiness`, Android/iOS/RN builds.
4. **Docs**: PLAN/CHANGELOG and browser guides; `build-docs-site.sh`, `check-doc-snippets.mjs`.

Proving specs: providers — `live_provider`, `provider_contract_surface`, `provider_console_provider_matrix`, `provider_console_panel`, `provider_request_diagnostics`, `provider_console_response_excerpt`, `autocomplete`, `keyless_place_provider`, `place_suggestion_golden`, `developer_console`, `second_provider`, `reference_flows`, `live_raster_fallback`; diagnostics — both provider diagnostics specs and `page_lifecycle`; navigation — `navigation_listeners`, `navigation_demo`, `navigation_guidance_refresh`, `voice_guidance`, `north_up_during_navigation`, `follow_trip_tile_coverage`; positioning — `device_location`, `location_puck_smoothness`; pure helpers — `sdk_core`, `offline_routing`, `offline_maps_card`; canvas/refapp — `golden_compare`, `overlay_addressing`, `vector_layer`, `vector_labels`, `tile_switch`, `vector_source_status`, `wasm_camera_contract`, `map_cluster_and_indicator`.
