# Four-Platform Capability and UX Parity Audit

Date: 2026-10-31
Owner direction audited against: every key SDK feature must originate in Rust, be
bound through the frozen UniFFI contract, and reach **Android (Kotlin), iOS
(SwiftUI), React Native (TypeScript/JSI) and WebAssembly** with the same
features, UX, wording, labels, UI flow and docs.

Scope: read-only analysis of the tree on branch `docs/four-platform-parity`.
No source, test, `PLAN.md` or `CHANGELOG.md` file was modified. The two offline
gates were run and both pass:

```
node scripts/check_ux_parity.mjs                     -> PASS (tokens 51 rows, copy 49 ids)
node --test platform/web/tests/contract_sync.mjs     -> PASS (1/1)
```

**Updates after the audit:** round 47 added the wasm search-place surface
(`suggestPlaces`/`searchPlaces` and their boundary forms, with the registry's
search fixture route), round 50 added the distance matrix
(`calculateDistanceMatrix` over the committed three-location fixture), and
round 51 added traffic incidents/flow over the committed Denver and New York
payloads, round 52 added the full route-options object, round 55 added the
distance matrix and traffic to React Native's curated API, round 61 added
the wasm navigation event bridge (the contract's listener topics), and round 62
taught `contract_sync.mjs` to compare the wasm canvas `MapView`, closing most
of §3.2's first gap, the wasm listener gap, §5 item 6 and part of §4's RN row;
`PLAN.md` M31 tracks the remaining slices. The matrix below records the
audited state as of 2026-10-31 and is not rewritten in place.

**Round 75 closed the plan-and-drive UX register.** A fresh four-app audit of
origin/destination search, route planning and the polyline, simulated
navigation, raster/vector switching and offline maps found ten divergences;
the round fixed the consumer-facing ones:

- Android and iOS now carry the same **Search an origin** entry on the home
  peek the React Native shell already had, so both plan fields are one tap
  from the home sheet (`ReferenceApplicationScreen.kt` peek action row,
  `HomeSheetView.swift` peek row).
- The iOS search view lists **origin first, destination second**, the
  documented §4.1 order the other three apps already used, and its dismiss
  action reads **Done**; Android's search view gained the **Plan a route**
  heading, so all three native search views render the same heading and
  dismiss word. Android's peek lost its stray trailing "Search" label and its
  trip-running label variant.
- Route options now agree: the React Native host sessions plan pedestrian,
  **fastest**, metric (they had planned shortest), and the browser's live
  planner sends the same options object (it had used the contract defaults,
  automobile). Recorded, live and offline routes therefore describe one
  route.
- The browser's live guidance now sits where §1 puts it: the turn banner,
  then line and deviation alert moved out of the Advanced panel to a
  top-center map overlay, the drawer's one ETA-distance line replaced the
  web-only arrival clock and progress list, and durations format with the
  same whole-minutes-plus-seconds rule as the native shells. The browser's
  offline route is now the planner's route (startable, framed, named in the
  route summary), and the base-map flow moved from Advanced into the settings
  sheet, whose section order now matches the native settings.
- The React Native off-route phase reads the canonical **Off route**; the
  allow-listed `OffRoute` override retired with the wording-parity ruling
  (PRD §6 item 23).

Recorded exemptions (visible, not silent): the browser has no disk tile
cache, so its offline card carries the route and provenance but not the
native cache/tile accounting lines; iOS's extra "Sample journey" and
"Simulate off route" controls are manual demo affordances whose recorded
scenarios the shared demo plans already cover on every platform; and no
platform sets a route endpoint by tapping the map (a shared opportunity, not
a divergence).

**Rounds 82–85 closed the plan-and-drive follow-ups and the documentation
register.** The iOS follow camera now rotates with the course (the reference
app's course helpers had swapped degree/radian constants), the React Native
trip draws its route from the first frame, and the trip wording, duration
formatting and N compass mark agree on every surface. The documentation set
was rewritten to the `docs/DocsStyleGuide.md` standard: one developer
quickstart, per-platform provider guides, generated API references (the
hand-written pages are gone), a Rust guide and Rust-first navigation, real
versioned downloads with checksums, a measured performance page and a
consumer-facing contract. Device positioning is the remaining capability gap;
its research and contract shape are in `docs/history/PositioningResearch.md` (M33),
with the Rust `rustic-maps-positioning` controller and simulated source already in the
workspace.

**Round 76 closed §3.2's vector-base gap.** The wasm canvas now serves the
contract's `setVectorTileSource`/`clearVectorTileSource`/
`vectorTileSourceStatus` over the shared `VectorBaseScheduler`: it fetches
the registry's `{z}/{x}/{y}` tileset through the browser transport, decodes
and styles each tile once, hides the raster tier, paints the style's
background, composes the base beneath the page's overlays and places the
base's symbol-layer text through the SDK's embedded default glyph font. The
browser page installs that source instead of composing overlays and labels
in JavaScript, so §4's "Vector tile source & styles" row is now **Full** on
all four targets and the label surface is renderer-internal everywhere; the
transitional `setLabels`/`clearLabels`/`setGlyphRange` exports remain only
for the isolated SDF coverage probe (a recorded follow-up).

The headline verdict:

- **Android and iOS are at binding parity** with the UDL: both consume
  `uniffi-bindgen` output generated from the same contract, so every interface,
  record and error is reachable. Their reference apps surface nearly the same v2
  UX, except that neither uses distance matrix, traffic, foreign provider traits,
  or the contract's `suggest`/`search`.
- **React Native is at generated-binding parity but not at curated-API parity.**
  `ubrn` generates the full UDL surface (`src/generated`, `src/index.ts`, both
  gitignored), but the package's hand-written `MapsSdk`/`MapView` proxies expose
  a subset, and the reference app drives navigation through *native app-shell*
  code, not JavaScript.
- **WebAssembly is the real gap.** The hand-maintained `rustic-maps-web-bindings` façade
  reaches only the camera/projection slice, the simulated-trip records, the
  `Navigator` driving core, the canvas `MapView`, and a handful of wasm-only
  free helpers. The entire `MapSdk` provider surface (search/suggest/reverse,
  distance matrix, traffic, cache/terms, provider-failure diagnostics, foreign
  provider registration), the `MapView` vector-source members, the navigation
  listener/speech/reroute-policy members and structured `SdkError` variants are
  unreachable from JavaScript.
- **Theme parity is broken on two platforms.** PRD §6 item 9 and
  `docs/ReferenceAppUX.md` §3 require Light/Dark/**Auto** with Auto the default.
  iOS and web implement it; **Android and React Native still implement a
  two-way Light/Dark toggle with a dark default**.
- **The two parity gates do not guard capability parity.** `contract_sync.mjs`
  deliberately compares only a frozen subset and never asserts the wasm
  `MapView` surface; `check_ux_parity.mjs` compares token values and extracted
  label strings, not flows, controls or capability reachability.

---

## 1. Contract inventory

Source of truth at the audit date: `crates/rustic-maps-native-core/src/rustic_maps.udl` (2127 lines).

**The counts below are frozen at the 2026-10-31 audit** and are not refreshed
in place; the contract has grown since (the per-capability provider wave, the
foreign-implementation traits and the directions capability readback all
landed afterwards). Read them as a dated record, not as the current contract
size.

### 1.1 Declaration counts

| Kind | Count | Notes |
|---|---|---|
| `interface` | 15 | 14 ordinary + `[Error] SdkError` |
| `[Trait, WithForeign]` interfaces | 4 | foreign-callback contracts |
| `dictionary` | 47 | records |
| `enum` | 23 | |
| `callback` | 0 | none; callbacks are modeled as foreign traits |
| `namespace rustic_maps` | 1 | 8 free functions |
| Interface members (excluding `SdkError` variants) | 82 | |
| `SdkError` variants | 12 | `SdkError` at UDL:181 |
| Callable UDL members (interfaces + namespace) | 90 | 82 + 8 |

The UDL declares no `callback` blocks; the four `[Trait, WithForeign]` contracts
are the callback seam:

- `GeocodingProvider` (UDL:330) — `forward_geocode`, `reverse_geocode`
- `RasterTileProvider` (UDL:343) — `fetch_raster_tile`
- `NavigationListener` (UDL:1006) — instruction, deviation, phase, speech callbacks
- `SdkDiagnosticEventListener` (UDL:1950) — `on_diagnostic_event`

### 1.2 Interfaces by capability area

| Interface | UDL line | Members | Capability area |
|---|---|---|---|
| `MapCamera` | 225 | 12 | camera/projection & gestures |
| `GeocodingProvider` (foreign) | 330 | 2 | provider registration / geocoding |
| `RasterTileProvider` (foreign) | 343 | 1 | provider registration / raster tiles |
| `MapSdk` | 751 | 19 | SDK root: providers, geocode, route, matrix, traffic, cache, diagnostics |
| `NavigationListener` (foreign) | 1006 | 4 | navigation subscriptions |
| `Subscription` | 1032 | 1 | navigation subscription handle |
| `Navigator` | 1049 | 14 | turn-by-turn controller |
| `RouteSimulator` | 1160 | 4 | simulated route walk |
| `MapView` | 1669 | 15 | rendering surface, overlays, location, vector source |
| `SdkDiagnosticEventListener` (foreign) | 1950 | 1 | diagnostics |
| `SearchEngine` | 2033 | 3 | search/suggest/reverse facade |
| `RoutingEngine` | 2059 | 2 | route + distance matrix facade |
| `OfflineManager` | 2088 | 2 | cache terms/TTL facade |
| `Diagnostics` | 2111 | 2 | diagnostics facade |
| `SdkError` (`[Error]`) | 181 | 12 variants | errors |

### 1.3 Namespace free functions (UDL:60)

`get_sdk_compiled_version`, `coordinate_to_normalized_world`,
`tile_key_for_coordinate`, `tiles_covering`,
`offline_route_from_archive_bytes`, `default_step_advance_policy`,
`default_deviation_policy`, `default_reroute_policy` — 8 total.

### 1.4 Records and enums by area

- **Geometry / camera:** `GeographicCoordinate`, `BoundingBox`, `ScreenPoint`,
  `NormalizedWorldCoordinate`, `TileKey`, `ViewportSize`, `EdgeInsets`.
- **Geocoding:** `Place`; enums `GeocodeQuality`.
- **Routing:** `AvoidOptions`, `RouteOptions`, `RouteStep`, `RouteLeg`, `Route`;
  enums `TransportationMode`, `UnitSystem`, `RouteStrategy`.
- **Distance matrix:** `DistanceMatrixEntry`, `DistanceMatrix`.
- **Traffic:** `TrafficIncident`, `TrafficFlowData`; enums
  `TrafficIncidentType`, `TrafficIncidentSeverity`, `TrafficFlowCongestionLevel`.
- **Tiles / styles:** `RasterTileData`, `UrlTemplateRasterTileSource`,
  `UrlTemplateVectorTileSource`, `VectorTileSourceStatus`; enums
  `RasterImageFormat`, `RasterTileResolution`.
- **Simulation:** `SimulatedTripPace`, `SimulatedTripFix`.
- **Navigation:** `DeviationInfo`, `RouteStepPosition`, `TripProgress`,
  `TripNavigationState`, `DistancePhrase`, `ManeuverInstructionText`,
  `ActiveManeuverInstruction`, `UpcomingManeuverInstruction`,
  `NavigationInstruction`, `DeviationStatusChange`, `NavigationLocationUpdate`,
  `StepAdvancePolicy`, `DeviationPolicy`, `ReroutePolicy`, `SpeechInstruction`;
  enums `NavigationPhase`, `DeviationGradeKind`, `DistanceUnit`,
  `DistancePhraseKind`, `ManeuverInstructionTextKind`,
  `DeviationStatusChangeKind`, `NavigationPhaseChange`,
  `StepAdvancePolicyKind`, `DeviationPolicyKind`, `SpeechInstructionKind`.
- **Rendering / overlays / location:** `OverlayColor`, `MapOverlay`,
  `SurfaceHandle`, `FrameStatistics`, `LocationIndicatorStyle`,
  `LocationIndicatorState`; enums `MapOverlayKind`, `LocationIndicatorVisualKind`.
- **Diagnostics / cache:** `TileCacheTerms`, `LogEvent`,
  `ProviderFailureDiagnostic`; enums `SdkLogSeverity`,
  `ProviderFailureClassification`.

Notably, **labels/symbols are not a contract capability at all**. There is no
label record, label interface or `MapView` label member. Native vector labels
are drawn inside `rustic-maps-rendering` (`crates/rustic-maps-rendering/src/vector_base.rs:39`,
`crates/rustic-maps-rendering/src/lib.rs:17`); the wasm seam alone exposes a label API
(`setLabels`, `clearLabels`, `setGlyphRange`, `canvas.rs:2237,2256,2266`) that is
ahead of the frozen contract.

---

## 2. Per-platform exposure matrix

Legend: **Full** = the platform can reach the capability through the contract
binding; **Partial** = reachable only in reduced form (stated); **Absent** = not
reachable. "Refapp" notes whether the reference application surfaces it.

Binding generation paths:

- **Android:** `cargo run --bin uniffi-bindgen -- generate --language kotlin`
  into `map-sdk-native/src/main/kotlin` (`scripts/build-android.sh:33,121,141`;
  output is gitignored, `platform/android/.gitignore:7`). Consumers import
  `uniffi.rustic_maps.*`; the refapp does so directly
  (`ReferenceApplicationState.kt:45,58,62-63,69`).
- **iOS:** `uniffi-bindgen generate --language swift` into
  `platform/ios/Sources/RusticMapsSDK` (`scripts/build-ios.sh:73,80,99`; output
  gitignored, `platform/ios/.gitignore:5-7`). The refapp imports `RusticMapsSDK`
  directly (`ReferenceApplicationModel.swift:17`).
- **RN:** `ubrn generate jsi bindings` into `src/generated` and
  `ubrn generate jsi turbo-module` producing `src/index.ts`
  (`scripts/build-rn.sh:56,66,83`; gitignored `platform/rn/.gitignore:27`). The
  package `main` is the generated `src/index.ts` (`platform/rn/package.json:16`),
  so the full generated UDL namespace is importable at the package root; the
  hand-written `MapsSdk`/`MapView` are separate entry points
  (`platform/rn/package.json:17-22`).
- **Wasm:** no UniFFI code generation. Hand-written `#[wasm_bindgen]` exports in
  `crates/rustic-maps-web-bindings/src/*`, mirrored by hand in
  `platform/web/types/rustic_maps.d.ts` and exposed to the page by
  `platform/web/main.mjs:5-29`.

### 2.1 Matrix

| Capability area | Android | iOS | React Native | WebAssembly |
|---|---|---|---|---|
| **Camera, projection, gesture algebra** | **Full** — generated `MapCamera`; wrapper `MapView.kt:377,462,478` | **Full** — generated; `MapView.swift:381,454,469,481` | **Full** via generated TS; curated handle `MapView.tsx:144-204` (no programmatic rotate) | **Full** — `MapCamera` class `exports.rs:95-395`; free gesture helpers `camera_interaction.rs:22,43,66` |
| **Provider registration & credentials** | **Full** — `MapSdk.setProviderCredential(+WithTileCache)`, URL-template raster source; `MapsSdkFacade.kt:46,73,109` | **Full** — generated; refapp `ReferenceApplicationModel.swift:1057` | **Partial** — curated credential/cache/raster-template proxy `MapsSdk.ts:223-282`; foreign provider traits only in generated TS, unexercised | **Partial** — `registerProvider`/`setProviderCredential`/`clearProvider` `live_providers.rs:108-144`; no tile cache, no foreign provider traits |
| **Geocoding / search** | **Full** — generated `forward/reverse/suggest/search`; refapp uses `forwardGeocode` (`ReferenceApplicationState.kt:1677,1842`) | **Full** — generated; refapp `ReferenceApplicationModel.swift:1851,2045` | **Full** — curated `MapsSdk.ts:285-364`; runtime `MapsSdkNativeRuntime.kt:294-325`; refapp native `searchPlaces` uses `forwardGeocode` (`RefappDemoSession.kt:817-834`) | **Partial** — `forwardGeocode` only (`live_providers.rs:172`); no `suggest`, `search` or `reverse_geocode` |
| **Routing (options, strategies, avoid)** | **Full** — generated `calculateRoute(waypoints, RouteOptions)`; refapp `ReferenceApplicationState.kt:1946` | **Full** — generated; `ReferenceApplicationModel.swift:2212` | **Full** — curated `MapsSdk.ts:336`; runtime `MapsSdkNativeRuntime.kt:312` | **Partial** — `calculateRoute` is a fixed two-waypoint, `RouteOptions::default()` call (`live_providers.rs:217-243`); no transport mode, strategy, avoid, units, departure time |
| **Distance matrix** | **Full** — generated `MapSdk.calculateDistanceMatrix` / `RoutingEngine`; **refapp does not use it** | **Full** — generated; refapp does not use it | **Full** at generated TS; curated API and refapp do not use it | **Absent** |
| **Traffic incidents / flow** | **Full** — generated `fetchTrafficIncidents` / `fetchTrafficFlow`; **refapp does not use them** (scene artifact flow only, `ReferenceApplicationState.kt:1430-1446`) | **Full** — generated; refapp does not use them | **Full** at generated TS; curated API and refapp do not use them | **Absent** — only the `TrafficFlowData` overlay payload shape exists (`types/rustic_maps.d.ts:407`) |
| **Raster tile source & cache** | **Full** — `UrlTemplateRasterTileSource`, tile cache + terms (`MapsSdkFacade.kt:73,109`) | **Full** — generated; refapp `ReferenceApplicationModel.swift:1057,1407` | **Full** — `MapsSdk.ts:257,367-392`; `baseMap.ts` | **Partial** — `MapView.setRasterTileSource(urlTemplate, maximumZoomLevel)` (`canvas.rs:2379`) and live provider fallback; no full source record, no cache/terms/TTL |
| **Vector tile source & styles** | **Full** — contract `MapView.setVectorTileSource` / `clearVectorTileSource` / `vectorTileSourceStatus` (`MapView.kt:586,614,642`) | **Full** — `MapView.swift:581,613,653` | **Full** — `MapView.tsx:184-188` | **Partial** — no `MapView.setVectorTileSource`/status; page composes wasm-only vector overlays and labels (`vector_layer.rs:72,94,121,139,265`; `tile_switch.mjs:444-446`) |
| **MapView rendering / surface lifecycle** | **Full** — attach/resize/draw/pump (`MapView.kt:1418,1463,1306`) | **Full** — attach/resize/draw/pump (`MapView.swift:932,964,805`) | **Full** — native host view owns surface and frame pump (`MapsSdkMapHostView.kt:51-55`); imperative bridge `MapView.tsx:220-276` | **Partial** — canvas `MapView` with `renderFrame` (`canvas.rs:1298,1545`); no `attach`/`resize`/`detach`/`is_frame_settled` (canvas lifetime is managed by the page) |
| **Overlays** | **Full** — `MapView.kt:495-531` | **Full** — `MapView.swift:494-538` | **Full** — `MapView.tsx:162-170` | **Full** — plain-object `setOverlays`/add/update/remove/clear (`canvas.rs:2111-2221`) |
| **Labels / symbols** | **Absent as a contract API** — rendered internally by `rustic-maps-rendering` (`vector_base.rs:39`) | **Absent as a contract API** — same | **Absent as a contract API** — same | **Wasm-only** — `setLabels`/`clearLabels`/`setGlyphRange` (`canvas.rs:2237,2256,2266`) |
| **Location indicator** | **Full** — `MapView.kt:541,557` | **Full** — `MapView.swift:547,563` | **Full** — `MapView.tsx:171-177` | **Full** — `canvas.rs:2312,2347` |
| **Navigation: `Navigator`** | **Full** — generated; refapp `ReferenceApplicationState.kt:290` | **Full** — generated; refapp `ReferenceApplicationModel.swift:412` | **Partial** — generated TS + native app-shell listeners (`RefappTraceReplay.kt:10-27`); curated JS API has no navigation | **Partial** — driving/readback subset: `startNavigation`, `updateLocation`, `applyReroute`, `stopNavigation`, `currentTripState` (`navigation.rs:624-704`); 5 of 14 members |
| **Navigation listeners / subscriptions / speech / reroute policy** | **Full** — generated `NavigationListener`, `Subscription`, speech, policies (`CollectingNavigationStateListener.kt:26`) | **Full** — generated (`NavigationStateCardStore.swift:90`) | **Partial** — reachable only from native app-shell code; no JS listener path | **Absent** — listeners, speech, `configure_reroute_policy`, `begin/cancel_reroute_calculation`, `current_instructions`, `subscribe_to_speech_instructions`, `add_navigation_listener` |
| **`RouteSimulator`** | **Full** — refapp `SimulatedTripDriver.kt:25,300` | **Full** — generated | **Full** — native trace replay `RefappTraceReplay.kt:19` | **Full** — `exports.rs:985-1030` |
| **Diagnostics / logging** | **Full** — `subscribeToDiagnosticEvents`/`flushRecentDiagnosticEvents` (`ReferenceApplicationState.kt:1304-1308`); provider-failure readback reachable | **Full** — `ReferenceApplicationModel.swift:3352,3367` | **Partial** — curated `diagnosticsLines` only (`MapsSdk.ts:419`); generated subscribe/flush in TS; refapp native feeds | **Partial** — free `subscribeToDiagnosticEvents`/`flushRecentDiagnosticEvents` (`diagnostics.rs:282,292`); no listener, no `latest_provider_failure_diagnostics` |
| **Offline routing graph** | **Full** — `offlineRouteFromArchiveBytes` (`ReferenceApplicationState.kt:69,2389`) | **Full** — `ReferenceApplicationModel.swift:2135` | **Full** — native `planOfflineRouteFromArchive` (`RefappNative.ts`; `App.tsx:1027`) | **Full** — `offlineRouteFromArchiveBytes` (`exports.rs:571`) |
| **Cache terms / TTL** | **Full** — `MapsSdkFacade.kt:109` | **Full** — `ReferenceApplicationModel.swift:1057` | **Full** — `MapsSdk.ts:367-392` | **Absent** |
| **Frame statistics** | **Full** — `onFrameStatistics` (`MapView.kt:125`) | **Full** — `MapView.swift:140` | **Full** — `MapView.tsx:134` | **Full** — object returned by `renderFrame` (`canvas.rs:1545`) |
| **Errors** | **Full** — typed `SdkException` variants mapped to canonical sentences (`ReferenceApplicationState.kt:353-379`) | **Full** — typed; canonical sentences (`ReferenceApplicationModel.swift:738-765`) | **Partial** — generated typed errors; curated proxy returns plain `detailText` and does not re-word (parity omission `scripts/check_ux_parity.mjs:1802-1805`) | **Partial** — throws plain strings (`exports.rs:51-52`); structured `SdkError` union documented but unreached (`types/rustic_maps.d.ts:312-331`) |
| **Foreign provider traits (`GeocodingProvider`, `RasterTileProvider`)** | **Full** — generated | **Full** — generated | **Full** at generated TS/JSI; no in-repo exercise | **Absent** |

### 2.2 What "Full" means for native

On Android, iOS and React Native (generated surface) "Full" is a property of
`uniffi-bindgen` covering the same UDL, not of bespoke code. The three
reference apps exercise a narrower set: all use `forwardGeocode`, provider
credentials, base-map switching, `Navigator`, `RouteSimulator`, offline routing
and diagnostics; **none use distance matrix, traffic fetch, foreign provider
registration, or the contract's `suggest`/`search`** (grep of `platform/android`,
`platform/ios`, `platform/rn` returns no call sites outside the generated/curated
wrappers). So the binding layer says "Full", the product surface says "not
demonstrated".

**Status (2026-11-01): the WebAssembly column of this matrix is historical.**
The façade now serves the contract's provider surface through the same
per-capability registry the native targets use: `configureProvider`,
`setProviderCredential`, `providerRegistrations`, `reverseGeocode`,
`routeCapabilities` (with the transport mode, strategy, avoid and departure
options on `calculateRouteWithOptions`), `calculateDistanceMatrix`,
`fetchTrafficIncidents`, `fetchTrafficFlow`, `fetchElevationPoints` and
`registerUrlTemplateRasterTileSource`. The legacy `registerProvider`,
`clearProvider`, `setProviderCredential(apiKey, baseUrl)`,
`clearMapquestProviders` and `MapView.setRasterTileSource` are gone, and
`platform/web/tests/contract_sync.mjs` now asserts that every `MapSdk`
contract member has a wasm export or a listed exception. What stays absent on
the browser is what cannot exist there: a disk tile cache and foreign
(callback) provider registration.

---

## 3. WebAssembly gap analysis

### 3.1 What the wasm façade actually reaches

**This section records the 2026-10-31 state.** The living answer is the gate
`platform/web/tests/contract_sync.mjs` (run directly or through Playwright's
`globalSetup`), which asserts what the wasm surface mirrors and what it
invents. The hand-written legacy calls named below are being retired for the
contract-shaped surface (PLAN.md M45).

The wasm surface is hand-written and split across:

- `exports.rs` — `MapCamera` (`:95`), free projection helpers (`:399-455`),
  `MapsSdk` (`:482`, only `create`/`compiledVersion`), offline route (`:571`),
  `RouteSimulator` (`:985`).
- `canvas.rs` — `MapView` (`:1298`): `initialize`, `renderFrame`,
  `stageParentTile`, overlay CRUD, `setLabels`/`clearLabels`/`setGlyphRange`,
  location indicator, `setBaseMapVisible`, `setRasterTileSource`,
  `setBackgroundColor`.
- `navigation.rs` — `Navigator` (`:624`) with 5 contract members + wasm-only
  `hasActiveTrip`.
- `live_providers.rs` — `registerProvider`, `clearProvider`,
  `setProviderCredential`, `clearMapquestProviders`, `forwardGeocode`,
  `calculateRoute`, `fetchRasterTile`.
- `diagnostics.rs` — `subscribeToDiagnosticEvents`, `flushRecentDiagnosticEvents`.
- `vector_layer.rs` — wasm-only vector overlay/label descriptor builders.

Of the 90 callable contract members, roughly **34 are reachable by name**:
12 `MapCamera`, 4 `RouteSimulator`, 5 `Navigator`, 8 `MapView` and 5 namespace
functions. Adding the wasm-only free-function aliases that stand in for `MapSdk`
members (`forwardGeocode`, `calculateRoute`, `fetchRasterTile`,
`setProviderCredential`, `subscribe/flush diagnostics`) reaches a little more of
the `MapSdk` intent, but not its contract shape.

### 3.2 Precisely unreachable from JavaScript at the audit date

**Superseded in part.** Several members listed below have since gained a wasm
form or a contract-shaped replacement; the current coverage question is
answered by `platform/web/tests/contract_sync.mjs` and the M45 migration, not
by this list. It is kept as the dated audit record.

`MapSdk` (UDL:751):
- `set_provider_credential_with_tile_cache`
- `raster_tile_cache_terms`, `set_raster_tile_cache_time_to_live`
- `register_geocoding_provider_from_foreign`,
  `register_raster_tile_provider_from_foreign`
- `suggest`, `search`, `reverse_geocode` (forward geocode exists as a free
  function only)
- `calculate_distance_matrix`
- `fetch_traffic_incidents`, `fetch_traffic_flow`
- `register_url_template_raster_tile_source` (only the reduced
  `MapView.setRasterTileSource` string form exists)
- `subscribe_to_diagnostic_events(listener, …)`,
  `flush_recent_diagnostic_events(listener, …)` (free-function rings exist, no
  listener)
- `latest_provider_failure_diagnostics`

`MapView` (UDL:1669): `attach`, `resize`, `detach`, `is_frame_settled`,
`set_vector_tile_source`, `clear_vector_tile_source`, `vector_tile_source_status`.

`Navigator` (UDL:1049): all three `subscribe_to_*` members, `add_navigation_listener`,
`begin_reroute_calculation`, `cancel_reroute_calculation`,
`configure_reroute_policy`, `current_instructions`.

Interfaces and traits with no wasm form: `Subscription`, `SearchEngine`,
`RoutingEngine`, `OfflineManager`, `Diagnostics`, `GeocodingProvider`,
`RasterTileProvider`, `NavigationListener`, `SdkDiagnosticEventListener`.

Namespace: the three policy defaults (`default_step_advance_policy`,
`default_deviation_policy`, `default_reroute_policy`) are not exported.

Types/payloads: `RouteOptions`/`AvoidOptions` are not parameters anywhere;
`TransportationMode`, `UnitSystem`, `RouteStrategy`, `DistanceMatrix*`,
`TrafficIncident`, `TileCacheTerms`, `ProviderFailureDiagnostic` are not
reachable as values. `MapOverlay` is a mirror only (`d.ts:377`, "the wasm seam
does not consume this type yet"). Errors are plain strings, not the 12
`SdkError` variants.

Two asymmetries beyond simple absence:

1. **Vector base rendering is contract-native on Android/iOS/RN but page-side on
   web.** Native `MapView.setVectorTileSource` draws styled tiles *and labels*
   inside Rust (`rustic-maps-rendering/src/vector_base.rs`), while the web page fetches
   MVT bytes itself and converts them to overlay/label descriptors in JS
   (`vector_layer.mjs`, `tile_switch.mjs:444-446`). The same product feature is
   produced by different code on web, and labels are not contract at all.
2. **Navigation guidance is snapshot-polled on web, event-driven on native.**
   `playground.mjs:2007` starts a `Navigator` and reads `currentTripState`; it
   cannot subscribe to `NavigationInstruction`, deviation or phase events.

### 3.3 What `contract_sync.mjs` asserts and does not assert

Asserts (`platform/web/tests/contract_sync.mjs`):

- 12 frozen dictionaries agree field-for-field with the TS mirror
  (`:47-60`, `:722-757`).
- `SdkError` variant names agree one-to-one (`:763-778`).
- The 12 `MapCamera` methods exist in the UDL and the wasm sources, and the wasm
  camera invents nothing (`:69-87`, `:798-831`).
- The 4 `RouteSimulator` methods (`:93-98`, `:837-869`).
- The 5 `Navigator` methods, plus the documented wasm-only `hasActiveTrip`
  (`:115-121`, `:127-129`, `:875-908`).
- The bounds-fitting constructor/`fittingBounds` pair (`:919-930`).
- The 5 frozen free helpers (`:132-142`, `:936-954`).
- The generated `pkg` declarations, when present (`:1061-1073`).
- Reverse direction: the wasm surface must not invent members except the
  hardcoded `WASM_ONLY_*` allowlists (`:160-205`).

Does **not** assert:

- The wasm `MapView` class against UDL `MapView` at all — no comparison exists
  for `attach`, `resize`, `detach`, `is_frame_settled`, vector-source members.
  The wasm `MapView` can drift silently.
- `MapSdk` or any provider record/enum.
- `SearchEngine`, `RoutingEngine`, `OfflineManager`, `Diagnostics`,
  `Subscription`, or the four foreign traits.
- `RouteOptions`, `AvoidOptions`, `DistanceMatrix`, `TrafficIncident`,
  `TrafficFlowData`, `TileCacheTerms`, `ProviderFailureDiagnostic`.
- Capability *coverage*: unbound contract members are explicitly excluded
  (`:25-28`), so the test passes while the whole `MapSdk` surface is absent.
- Error *shape*: only variant names are compared; the wasm seam throwing plain
  strings is tolerated.

### 3.4 `ubrn build wasm2` as an option (re-opened 2026-11-01: alignment landed, generation planned)

**Status.** The owner directed two changes on 2026-11-01 (PRD §6 items 27 and
30): the toolchain is aligned on **UniFFI 0.31.2**, and the WebAssembly and
React Native bindings are to be **generated** from the UDL instead of
hand-maintained, shipped as **one generated wasm module** with the canvas
renderer folded into its crate graph and a single provider registry.

The two proof points have landed:

- The alignment is mechanical (the binding crate compiles unchanged, every
  generated-text marker passes, and the `ubrn` TypeScript now agrees with the
  Rust scaffolding on all 108 checksum items — 37 disagreed under 0.32.2).
- `ubrn build wasm2` **works**: the checker accepts the crate's manifest with
  the direct `uniffi_core` wasm feature (no `?Send` cascade), and a generated
  package proved in Chromium and Node the init handshake, sync and async
  exports, an async `[Trait, WithForeign]` callback invoked from Rust, a
  same-origin provider tile fetch, native provider registration and the
  placeholder `MapView` refusal. The spike found and `web-time` fixed the one
  blocker — `SystemTime`/`Instant` panicked on wasm and aborted the module
  through a poisoned mutex.

The migration is `PLAN.md` M35: the canvas adapter seam (owner ruling: one
module), then the page and suite migration and the retirement of the hand
mirror and `contract_sync.mjs`.

**What the migration still requires:**

1. **A wasm-buildable contract surface** (dominant cost): `rustic-maps-native-core`'s
   tokio runtime, `reqwest`-backed transport composition and native provider
   registration do not build for `wasm32-unknown-unknown`. Either gate them
   behind `cfg(not(target_arch = "wasm32"))` or split a wasm-safe contract
   crate.
2. **A proven foreign-trait callback path.** The guide is silent on
   `[Trait, WithForeign]`; whether the single-threaded runtime player can call
   back into JavaScript for `NavigationListener`, `GeocodingProvider`,
   `RasterTileProvider` and `SdkDiagnosticEventListener` needs a spike.
3. **The canvas `MapView` stays hand-written.** It owns the surface, tile
   scheduler, overlays and label pipeline; the generated contract surface would
   sit beside it, with a naming split to document.
4. **Async and transport.** `[Async]` members become promises; the browser's
   provider path already uses `provider-transport`'s fetch implementation, which
   is the one a generated `MapSdk` would need on wasm.
5. **Page and suite migration** off the hand-written package: the generated
   bindings name types differently and throw typed errors, so `playground.mjs`,
   `main.mjs`, the golden harness and the suites migrate, and
   `platform/web/types/rustic_maps.d.ts` with `contract_sync.mjs` retire or
   re-scope.

**Recorded correction (2026-11-01).** The earlier §3.4.1 cost analysis
overstated the downgrade's risk: the hand-written foreign-trait plumbing needed
no re-derivation (the callback-interface macro is byte-identical between the
two releases), `uniffi::generate`/`GenerateOptions` did not move, and no
generated-text marker changed. The audit's version-independent point stands:
the downgrade does not by itself make the browser surface generated; the
wasm-buildable contract surface does, and that is M35's first slice.

### 3.4.2 What the migration itself requires upstream


