# Changelog — Rustic Maps SDK

All meaningful changes to the SDK are recorded here. Dates are ISO-8601.

## [Unreleased]

### Changed (2026-11-19 — CI runs only what a push can change)

- A pull-request branch is no longer linted twice: `actionlint` keeps its `pull_request` trigger and runs on `push` for `main` only. The wasm, React Native, benchmark, Android and iOS workflows now also run when `rust-toolchain.toml` changes, so a toolchain edit that affects a build is tested by that build; the pinned-toolchain target that broke the wasm jobs had been invisible to them. `rn-app-shell` also watches `scripts/generate-rn-bindings.sh` and `wasm-e2e` watches `scripts/build-wasm2-package.sh`, the scripts those jobs run: a change to the binding generator used to leave both jobs unstarted. Dependabot's pull requests are exempt from the sign-off check, because the bot cannot sign a commit and the dependency review is their gate.

### Changed (2026-11-18 — the gate stamps the tree it checked)

- `scripts/run-gate.sh` runs the format, workspace clippy and workspace test gates once and records a stamp keyed on the tree it checked. The pre-push hook accepts that stamp and returns at once, so a push no longer holds the transport open for the gate's duration; a tree without a stamp still runs the whole gate.
- `rust-toolchain.toml` declares the `wasm32-unknown-unknown` target, so the pinned toolchain installs it on a fresh runner. The wasm jobs had failed with "can't find crate for `core`" because the directory override selected the pinned toolchain, which lacked the target.

### Changed (2026-11-18 — the working plan retires into the issue tracker)

- `PLAN.md` is gone. Every open item is one issue with a milestone and a label, each milestone description carries its done-when line, and the eight open question groups moved to issues #124 to #131. `PRD.md` fell from 306 to 190 lines with all 70 decisions intact, `CLAUDE.md` from 302 to 144 lines, and the changelog's stale identifiers, paths and ordering were corrected. The DCO check applies to outside contributions only.

### Changed (2026-11-18 — the public repository is the working repository)

- `angoratek/rustic-maps-sdk` carries the curated opening history plus every commit made after it, and it owns the issues, branches and pull requests from now on. The earlier private repository is archived read-only after the later commits were replayed onto the public tip with identical trees. No SDK code or published documentation changed.

### Changed (2026-11-18 — one camera geographic-bounds helper)

- `camera_geographic_bounds` and the `METERS_PER_DEGREE_LATITUDE` and `MINIMUM_UNPROJECTED_BOUNDS_CORNER_COUNT` constants now live in `rustic-maps-rendering::camera`. The browser binding delegates to it and the native surface view calls it through the wrapped renderer camera, so the two copies of the corner unprojection, the steep-pitch fallback and its constants are gone. Two tests in the rendering crate pin the top-down corners and the 70° fallback; the existing browser and native bounds tests still pass.

### Fixed (2026-11-18 — the host build allows the wasm-only field names)

- Removing the `#[allow(dead_code)]` from `generated_contract_field_names` made every constant that only the wasm-only canvas and vector-layer modules read dead on the host test build, and the workspace clippy gate failed on 53 of them. The allow is scoped to non-wasm32 builds; under wasm32 an unused constant still fails.

### Changed (2026-11-18 — one bounding-box aggregation)

- `BoundingBox::from_coordinates` builds the smallest box around an iterator of coordinates and returns `None` for an empty one. The everymap bridge, offline routing and traffic-flow cache now call it instead of four hand-rolled min/max loops. The two camera bounds copies are folded into the shared camera helper tracked separately.

### Changed (2026-11-18 — cache lifetimes are named)

- The default cache lifetimes in `response_category.rs` were inline arithmetic (`24 * 60 * 60`, `10 * 60`, `30 * 24 * 60 * 60`). They are now named constants: one per category plus `SECONDS_PER_MINUTE`, `SECONDS_PER_HOUR` and `SECONDS_PER_DAY`. The values are unchanged and the existing expiry test still pins them.

### Removed (2026-11-18 — dead contract field names)

- Twelve `generated_contract_field_names` constants had no reader outside their own module: the two bounding-box fields and the ten route fields. They are deleted, and the module's blanket `#[allow(dead_code)]` is gone, so the next unused name fails the wasm32 build instead of drifting. The remaining constants are imported by the canvas, vector-layer and vector-source readers; the host test build keeps a scoped allow because those wasm-only readers do not exist there.

### Changed (2026-11-18 — documentation cut by content, not line wrapping)

- The six contributor guides fell from 50,342 to 40,262 words: `Architecture` 9,851 → 7,331, `Browser` 6,355 → 4,246, `Providers` 6,764 → 4,792, `StyleSupport` 9,348 → 6,605, `Release` 5,873 → 5,626 and `ReferenceAppUX` 12,151 → 11,659. Process narrative, the everymap release diary, the style implementation order and residual backlog, and the spent history-rewrite runbook are gone; the substantive style backlog and open questions moved to the issue tracker. Every constant, path, command, contract name, capability row and failure mode stayed.
- The tutorial chapters lost 44–46% of their words: `first-map` 2,383 → 1,330 and `navigation-simulation` 1,866 → 1,017. Shared rules now sit above the four platform tabs, and per-tab prose that restated them, along with duplicated listener and gesture code, is gone.
- The style guides were the largest prose wins; `ReferenceAppUX` keeps 96% of its words because the consumer-copy table, token tables and component inventory are the design contract itself. The remaining book volume is fenced code (1,630 lines), provider rows and reference tables, not narrative.

### Fixed (2026-11-18 — two documentation gates that were not checking)

- `check-provider-docs.mjs` accepted only five-column provider rows. The merged capability tables carry attribution and terms as two trailing cells, so every availability and capability check was silently skipped while the gate reported success. It now reads rows with trailing cells and fails a page with no readable provider row.
- `generate-doc-snippets.mjs --check` failed at `HEAD`: `search-and-routing/frame-the-route` had no source region in any reference application. The two blocks are tutorial-only, so they are no longer marked as generated; the generator now reports 32 chapters, 0 drifted.

### Added (2026-11-18 — the WebAssembly API reference covers the contract classes)

- The generated reference page now parses the class declarations in the
  generated `platform/web/generated/rustic_maps.d.ts` and renders `MapSdk`,
  `Navigator`, `RouteSimulator` and `PositionController` with every member,
  parameter names, return type and summary. The page previously named the four
  classes without their members, so a reader had to open the declaration file
  to find a signature.

### Fixed (2026-11-18 — the hosted demo's budget matches what streamed)

- The claim endpoint reserved a full three-minute session per claim, so claims
  that never pressed Appetize's own Start button still consumed the month while
  the Appetize dashboard counted only streamed time. Before each decision the
  endpoint now reconciles its monthly counter with Appetize's usage report
  (`GET /v1/usageSummary`, read-only key in `APPETIZE_API_KEY`): the counter
  drops to what actually streamed plus one session of headroom for in-flight
  claims, never below the report and never above the reservation. Without the
  key, or when the report is unreachable, the reservation stands alone. The
  status readout reports the reconciled `streamedSeconds`, and the endpoint's
  suites (`npm test`, 18 cases) cover the reconciliation arithmetic and the
  report parsing.

### Changed (2026-11-17 — documentation consolidated to eight guides)

- The 25-file `docs/` set became eight guides plus the generated notices:
  `Architecture.md` (contract, crate graph, `MapView`, provider layer, offline
  routing), `Browser.md` (generated surface, loop, fallbacks, tile throughput,
  console, measured performance), `Providers.md` (registry, credentials,
  coverage matrix, provider-backed sources, caching), `StyleSupport.md`,
  `ReferenceAppUX.md`, `Release.md` (artifacts, workflow, signing, site,
  screenshots, hosted demo, public-repository runbook), `DocsStyleGuide.md` and
  `THIRD_PARTY_NOTICES.md`. `docs/README.md` indexes them.
- Merging reconciled the sources against the tree: the aggregate required check
  is `format-and-lint`, the console's current element identifiers and filters,
  the six forbidden-header names the browser transport actually drops, the
  region-download members that are on the contract, and `style_schema` as
  planned rather than shipped. Duplicated process narrative and migration
  history are removed; every table, constant, path and test name survives.
- Dated entries keep the document names in use at the time; `docs/README.md`
  and this section name the current set.
- Gate on the merged tree: formatting, workspace clippy, the workspace tests,
  the wasm closure, generation drift, the rebuilt browser package and its
  freshness gate, contract sync, the provider guard and parity floor, snippets,
  docs references, provider docs, style coverage, DCO, release plan, actionlint,
  the full browser suite (241 passed, 2 skipped), the Android AAR and
  reference-app bindings, the iOS XCFramework and unit tests, React Native
  typecheck and Jest, and the documentation-site build with a clean scrub.

### Changed (2026-11-17 — the public record is revision-independent)

- Commit ids recorded in `PLAN.md` and `docs/` became dated or descriptive
  references (52 in `PLAN.md`, four in `docs/`), because the published history
  is a rewritten mirror whose commit ids differ; no backticked commit id
  remains in any Markdown file, and the tree carries no absolute local path
  outside the docs-site scrubber and its test.
- Decision vocabulary replaces process vocabulary, recording the decision with dates and `PRD.md` §6 item numbers where they exist, across fourteen repository documents. The published site pages were
  already clean; `docs/DocsStyleGuide.md` keeps the banned-word list because it
  defines the rule, and vendor billing sentences stay as terms text.
- The history rewrite and publication runbook is `docs/Release.md`.

### Changed (2026-11-17 — repository documentation trimmed to the live guides)

- The implemented design notes are retired: `Autocomplete.md`,
  `DirectionsModesAndStrategy.md`, `LocationInterpolation.md`,
  `MapControlsAndTrafficLayer.md`, `PositioningAndRouteUx.md`,
  `ReferenceAppSettingsAndPersistence.md`, `ReferenceApplicationBehavior.md`,
  `RouteShields.md`, `TilePackManager.md`, `BindingSurfaceConsolidation.md`,
  `PlanFlowAudit.md`, `ProviderGenericity.md`, the three `history/` records and
  the three merged pointer stubs are deleted (4,261 lines); `docs/` holds 25
  documents and `docs/README.md` indexes every one. The decisions they recorded
  nowhere else are `PRD.md` §6 items 66–69 (puck interpolation with dead
  reckoning opt-in, the versioned preferences record, the per-surface
  directions reason text, and the five platform-owned surface concerns); the
  Rust-desktop positioning question is tracked in the issue tracker.
- Every reference from source comments, tests, scripts and the root documents
  now points at a surviving home: `PRD.md` §6 items, `docs/ReferenceAppUX.md`
  §2.5/§4.1/§4.2/§5.5, `platform/providers.json`, `platform/reference-flows.json`,
  `docs/Browser.md`,
  `docs/Providers.md`, `docs/Architecture.md` and the
  code that implements the design. No behaviour, assertion, workflow or site
  page changed.

### Changed (2026-11-17 — browser surface generated from the contract)

- The browser API is generated from the frozen UniFFI contract by the
  `wasm2` bindgen; the hand-written contract mirrors are retired outright (no
  deprecation): `navigation.rs`, `positioning.rs`, `diagnostics.rs`,
  `diagnostics_filter.rs`, `camera_interaction.rs`, the contract half of
  `exports.rs` and `live_providers.rs`, and the unused contract field-name
  constants. The crate now carries only the GPU canvas (`BrowserMapView`,
  `MapCamera` and their pose algebra), the recorded-tile and vector-layer
  helpers, the reference-application math, and the provider slots plus the
  fixture-endpoint/clear seams the canvas reads.
- The canvas class is exported as `BrowserMapView`, so the stripped contract
  `MapView` never reaches the page; the canvas constructor takes only its
  element and an optional tile resolution, and the obsolete compiled-version
  hand-off is gone.
- The WebAssembly package is `platform/web/generated`: `scripts/build-wasm2-package.sh`
  compiles the ESM set and its `.d.ts` declarations from the bindgen output and
  runs `wasm-opt`. `scripts/build-wasm.sh`, `scripts/check-wasm-package-freshness.mjs`,
  `platform/web/pkg`, `platform/web/types/rustic_maps.d.ts`,
  `rustic_maps_surface.d.ts`, and `scripts/generate-web-facade.mjs` and its
  test are deleted.
- `platform/web/types/browser_canvas.d.ts` declares the wasm-bindgen glue the
  page imports. `contract_sync.mjs` is reduced to the stripped-UDL split and
  the generated package identity; the npm package ships the generated ESM set,
  its declarations, the canvas declarations, and the notices, with `@ubjs/core`
  and `@ubjs/wasm` as runtime dependencies.
- The docs site stages `/demo` from `platform/web/generated` and the player
  runtime beside it; the reference generator, the snippet checker and the
  browser guides read the generated declarations.

### Added (2026-11-17 — WebAssembly camera members)

- The WebAssembly façade carries the camera contract. `MapCamera.interpolate`
  is a static member over the SDK's one interpolation curve: exact endpoints,
  the shortest longitude path and bearing arc, smoothstep easing, and an
  `InvalidCoordinateInput` failure for a non-finite fraction. The canvas
  `MapView` holds one camera pose: `camera()` reads it back, `setCamera`
  adopts it, `fitBounds` frames a box in the caller's viewport and adopts the
  fitted camera (or answers null before the view initializes), and
  `cameraGeographicBounds(widthPixels, heightPixels, fieldOfViewDegrees)`
  answers the stored pose's flattened box with the ground-scale fallback at a
  steep pitch. `renderFrame` keeps its per-frame camera argument and is
  authoritative for the stored pose.
- The browser reference page keeps one `MapCamera` and routes every gesture,
  slider, keyboard move, follow-camera update and preset through it. Camera
  presets ease through one `MapCamera.interpolate` move over the shared
  duration instead of five per-component eases, so a click never changes the
  pose synchronously and the settle equals the preset exactly. Trip framing
  and the traffic bounds readback use the new view members, and the page's
  meters-per-degree and corner-unprojection copies are gone.

### Added (2026-11-17 — tile payload cache hit/miss readback)

- `MapSdk.tile_cache_statistics()` reports the installed tile payload cache's
  exact `hit_count` and `miss_count` beside its `stored_byte_count`, or null
  while no tile cache is installed. Both the raster base and the vector base
  fetch through the same store, so one counter pair covers every payload
  lookup; a store read failure counts as a miss, a time-to-live override keeps
  the counters, and a reinstalled cache starts at zero. Available in Kotlin,
  Swift and React Native through the regenerated bindings and the RN JSON
  encoders.
- The Android, iOS and React Native developer console shows the tile cache's
  exact hits and misses beside the traffic cache's, with the last frame's
  requested, drawn, fallback and missing counts on a separate labelled row.
  The browser console already read the vector payload cache counters from the
  canvas status.

### Added (2026-11-17 — vector source binding and status fidelity)

- A provider-backed vector base verifies every vector source its style document
  references against the tileset the provider serves. A source that declares a
  `url` or `tiles` naming a different tileset refuses the install with a message
  that names the declared source(s), their addresses and the served tileset,
  instead of painting fragments. A source with no address is bound to the
  provider as before, and a source of a kind the vector base cannot draw is
  reported by name as a degraded asset.
- `vectorTileSourceStatus` reports style fidelity: the unsupported-property
  count with a sorted list of at most 32 scope-qualified names, the assets that
  fell back (a declared sprite, a declared glyph endpoint, or an undrawable
  source) in a sorted list of at most 16 names, and each referenced source's
  `matchedLayerCount` and `drawnLayerCount` for the last resolved frame. The
  per-source counts use a new `VectorTileSourceLayerCount` record. The fields
  are mirrored in the WebAssembly status object and the React Native JSON
  encoders.

### Added (2026-11-17 — vector zoom cross-fade and the label candidate cache)

- A camera crossing a tile level no longer flashes the background on the styled vector base.
  The previous level's geometry stays drawn while the new level fills, then dissolves over 250 ms
  once at least 85% of the new level is present; the hold caps at one second, a second level
  change retires the level then requested, and no retired-level tile is ever fetched. The retired
  geometry is rebuilt from the per-tile styled cache, and a tile the new level already paints as
  an ancestor stand-in is skipped, so a zoom-in never doubles translucent fills and a zoom-out
  stays covered. The native surface and the browser canvas both drive it and keep their frame
  pump running through the fade; canvas statistics report `vectorZoomTransitionRunning` and
  `vectorRetainedTiles` beside the raster cross-fade's fields.
- Each styled vector tile caches its label candidates against the tile address, style revision and
  zoom bucket, so an unchanged settle frame performs no symbol-layer walk. Frame statistics report
  the extraction and cache-hit counts, and a motion-graded frame still defers extraction to the
  settle. Measured on the browser fit harness: an unchanged z8 settle frame performs zero
  extractions and renders at 2.1 ms p50 / 5.8 ms p95 against the 6 ms budget.

### Added (2026-11-17 — developer console, camera contract and pack budget)

- A developer console on every target: a floating control beside the layers control with a
  Warning/Error dot, the scrubbed event stream newest-first filtered at Error/Warning/Info,
  provider rows naming the display name, contract, method and redacted path, status class,
  duration and bounded response excerpt, a detail view with the typed provider-request trace and
  live tile and traffic cache rows, and a credential-free export through a JSON download on the
  web and the platform share sheet on Android, iOS and React Native. The console is read-only
  over the SDK's own stream.
- The additive camera contract: `MapView.camera()`, `set_camera`, `fit_bounds`,
  `camera_geographic_bounds` and `MapCamera.interpolate`, with the fit and bounds behavior the
  platform wrappers previously mirrored.
- The Android, iOS and React Native `MapView` wrappers hold the contract `MapCamera` as their
  only camera state. Set and every gesture transform hand the contract value to the native view,
  an animation frame is one `MapCamera.interpolate` call, `fitBounds` frames through the
  attached-viewport contract member and `cameraGeographicBounds` reads the contract answer. The
  wrapper pose values are readback projections, and the duplicated normalization, smoothstep,
  shortest-arc bearing, fit and geographic-bounds math is gone. Set/readback, the interpolation
  midpoint, fit without an attached viewport, bounds delegation and the gesture cancel are
  covered on Android (JVM smoke), iOS (simulator unit tests) and React Native (Jest). The
  WebAssembly façade adoption remains. The RN Android build and strict TypeScript check also
  repaired stale declarations they exposed: the Android bridge's listener failure phase and its
  TypeScript union member, the required route geometry in both native trace decoders, and four
  test-only typing drifts.
- A separate pack byte budget: `OfflineManager.set_tile_pack_byte_budget` and
  `tile_pack_byte_budget` reserve a pack-specific budget beside the general cache while the
  65,536-tile structural cap stays the only size limit; a download that crosses the budget keeps
  the partial pack, reports the cache failure and resumes once the budget is raised.
- React Native carries the documented TypeScript mirror of the location-pose animator and the
  follow-bearing mode, with the app shell driving the puck from its frame clock and the compass
  toggling north-up during a trip.

### Fixed (2026-11-16 → 2026-11-17 — provider bases, switch recovery and vector performance)

- The web demo selects raster bases by provider: each row installs the descriptor it names, the
  Serving mark, attribution and remembered choice follow it, a credentialed row reads
  `Needs <credential label>` until its key is applied, applying a key no longer moves the
  serving base silently, and the keyless live fallback is a named row.
- A raster selection made during a hosted-style download now wins: superseded vector installs
  cannot write the scheduler, summary, status or error, and page-side callbacks from an older
  selection are ignored.
- A raster frame that lists tiles and draws none paints a neutral retry state instead of black,
  and changing or clearing the raster provider drops pooled textures, decoded payloads, retry
  schedules and in-flight fetches.
- Official vendor styles render faithfully: `mapbox://` sprite and glyph addresses resolve through
  the provider's authenticated client, the vendor property set (`text-padding`,
  `text-pitch-alignment`, `icon-text-fit` and padding, `icon-opacity`, `line-translate` and anchor)
  is applied, every unsupported property and layer kind reports a named diagnostic, a style whose
  tile schema does not match names the mismatch, and both vendor style fixtures are neutral,
  schema-matched documents and tiles.
- Vector performance: a z7 fit now paints its first tile in about 80 ms and its first full frame
  in about 1.06 s with 372k triangles (previously 3.4–4.5 s and 1.77M), frames are shared behind an
  `Arc` with a content revision, arrivals coalesce into one repaint per 100 ms, low-zoom geometry
  simplifies and culls at the style bucket, and settled frames land at p95 3.7 ms.

### Added (2026-11-16 — route shields, foreign vector providers and traffic-cache console rows)

- Route references draw as shaped shield badges. A style symbol layer declares a `shield` block
  (`shape`, fill, border, text colour and size, padding, minimum size), the reference comes from
  the `routes` layer's `ref` rather than `text-field`, and the badge tessellates in screen space
  beneath the measured glyphs, joins the frame's collision handling with its padded, rotated box,
  and draws on the native, headless and wasm canvas paths. A layer without a shield block paints
  no route reference instead of a misleading road name. A byte-exact regional vertex-stream golden
  and a tolerant image golden cover a road reference and a transit reference.
- A foreign application-supplied vector-tile provider registers through one additive contract
  path — `VectorTileEncoding`, `VectorTileSourceDescriptor`, `VectorTileData` and a
  `[Trait, WithForeign] VectorTileProvider` — with
  `MapSdk.register_vector_tile_provider_from_foreign` and
  `vector_tile_provider_is_registered`, under the provider key `application`.
- A URL-template vector source declaration keeps its terms text, subdomains, request headers and
  style hints; a keyless named tileset inherits the recorded values when its template matches, on
  the native surface. A browser cannot set request headers and does not consult those
  declarations, so its extras are caller-declared only.
- Frame statistics and the provider console report the live traffic-flow cache counters beside
  the tile cache's and keep them honest across a provider switch; the provider-console chapter
  explains every field.
- The browser canvas draws a shield badge and reports it through `labelBadgeTriangles`.

### Fixed (2026-11-16 — platform hardening and the audit repairs)

- Android: every listener and frame failure is isolated (a throwing callback or a JNI
  `LinkageError` no longer stops the pump or reaches the main thread), refused overlay and queued
  vector-source pushes restore their bookkeeping, the tile cache opens off the main thread after
  a credential is applied, the camera pose survives activity recreation, the listener is held for
  the view's life, and the build asserts the exact release AAR, its bindings, all three ABIs,
  16 KB page alignment and JNI exports.
- iOS: stale draw generations are dropped, a superseded search is cancelled and awaited, a
  refused queued vector source restores the served source, gesture-driven settings persist once
  and flush when the scene backgrounds, and a mid-session authorization change stops updates or
  reconfigures background location.
- React Native: one typed `MapsSdkError` with stable codes and native detail, the URL-template
  declaration's four extra fields cross the bridge with nullable semantics, the curated type
  names are unambiguous, and the package declares generated `types` and `react-native` entries.
- Rendering: shield parsing is bounded to named pixel limits and applies only to symbol layers;
  a frame places one label per distinct reference text, and badges follow that rule; the legacy
  style-parity harness compares shields; a failed traffic-cache install clears the previous
  readback.
- Documentation: the published chapters no longer say shields are not drawn, the Compose
  dependency instructions match the `compileOnly` POM, and the binding-surface record names the
  current React Native types and the Android package moves.

### Removed (2026-11-16 — the documentation hostname)

- The SDK no longer advertises a custom documentation hostname. The retired
  hostname stopped serving the published documentation and its Vercel proxy
  project is deleted; the canonical address is
  <https://angoratek.github.io/rustic-maps-sdk-docs/>, and every reference now
  uses it: the README links, the reference-app user-agent contact URL in
  `platform/providers.json`, the hosted-demo claim endpoint's CORS allowlist,
  the Appetize referrer list and the smoke tool's default remote URL.
- The `site/hostname` proxy project and its setup record are removed.

### Added (2026-11-15 — the vector base guide)

- *Draw a provider's own map style* documents installing a provider-published style document as the base
  map: the outcome, the prerequisites, four numbered steps, the four resources the SDK fetches, the host
  scope the credential obeys, the degradations that keep a base usable, and the member each target calls.


### Changed (2026-11-14 — a credentialed vector family stays listed until its key is applied)

- The Android, iOS and React Native map-layers controls list a provider-published vector family before its
  credential is applied, disabled with `Needs <credential label>`, instead of hiding the family. The rows
  match the web demo and the reference UX, and a keyless or credentialed family is distinguishable from an
  absent one.
- React Native's map failure renders the shared `no '<provider>' provider is configured` sentence when the
  SDK refused because nothing serves the capability, instead of the generated binding's representation.

### Fixed (2026-11-14 — stale diagnostic-feed test helpers and an ambiguous view type)

- The Android and iOS diagnostics-feed tests construct the contract's `LogEvent` without its
  `providerRequestTrace` member and no longer compiled; both pass it as absent.
- The iOS reference application failed to build on `MapView` ambiguity between the generated class and the
  shipped `MapNativeCoreUI` wrapper; the one bare annotation is qualified.

### Changed (2026-11-14 — the base reports the style's own source policy)

- `VectorBaseScheduler` binds its style's `base` source through the existing `VectorSourceBinding` policy,
  so a style's declared `minzoom`, `maxzoom` and `attribution` reach the base and the tileset descriptor
  fills whatever the style leaves out. Replacing the style rebinds, and the native and browser status
  readbacks report the style's line rather than the descriptor's.
- The reference application still passes the registry's style attribution into its URL-template source,
  because the committed styles declare no source attribution of their own.

### Added (2026-11-13 — the style's own glyphs reach the label pass)

- A provider-published style document now draws its own place names: the install fetches the style's
  `glyphs` ranges, packs them into a `LabelFont` and hands it to the vector base, so a styled base paints
  its labels in the family's own glyphs instead of the SDK's embedded range. The font is cached per
  provider, document address and font stack, so a repeated install costs no request.
- `VectorBaseScheduler` gains `with_label_font` and `label_font`; a style replaced on a live scheduler
  drops the previous style's font. A style with no glyph endpoint, no symbol font stack, or ranges the
  install cannot fetch, decode or pack keeps the embedded Noto Latin range and still installs. So does a
  glyph address the SDK cannot turn into a fetchable `http`/`https` template, such as a vendor URI scheme
  (`mapbox://fonts/…`) or a relative path, so a vendor family never loses its base over an address the
  SDK cannot fetch; only an address resolving to an origin other than the document's refuses the install,
  before any request.
- The glyph protocol's Latin blocks are 256-codepoint ranges: the second block is `256-511`
  (`U+0100`–`U+01FF`), so `GlyphRange::LATIN_EXTENDED` replaces `LATIN_EXTENDED_A`, whose `256-383`
  segment no glyph server serves. Its served payload is committed at
  `fixtures/glyphs/noto_sans_regular_256-511.pbf`.

### Added (2026-11-13 — the style's own sprite reaches the vector base)

- A provider-published style document now draws its own icons: `fetch_hosted_sprite_atlas` resolves the
  style's `sprite` base to the protocol's `<base>.json` index and `<base>.png` sheet, fetches both through
  the provider's authenticated style-resource client and decodes the atlas. The install prefetches it
  (the renderer's sprite seam is synchronous) and hands it over with
  `VectorBaseScheduler::with_prefetched_sprite_atlas`, cached per provider, document and base address, so
  a repeated install costs no request.
- The sprite addresses are held to the style document's origin: an off-origin base refuses the install
  before any request, and a base the SDK cannot fetch as `http`/`https` degrades to drawing text without
  icons. `SpriteSource` host injection is unchanged for tests and applications.

### Added (2026-11-13 — a provider's style resources survive a restart)

- The style document, glyph ranges and sprite sheet a provider publishes are kept in the same
  namespaced cache file as its tiles, under the terms' thirty-day window, so a reopened application
  paints its base without re-requesting them. `ResponseCategory::StyleResources` carries that window
  and `CachedStyleResourceClient` checks the store before the provider; the install wraps its registered
  fetch whenever a tile cache is installed, and goes straight to the provider when none is.
- The entry key names the provider and the resource address only: no credential enters it, so rotating a
  key keeps the cached bytes valid and two credentials on one provider share them. A failing lookup or
  write degrades to a miss rather than failing the install.

### Added (2026-11-13 — one credential scope, and the browser's own glyphs and icons)

- The provider credential now travels only to hosts the provider declares. `VendorImplementations` carries
  `style_resource_hosts`, the registry mirrors the same list, and the registration hands it to
  `ProviderRegistry` beside the style-resource client, so both the native surface and the browser canvas
  apply one rule: a style's glyph template or sprite base is fetched when it resolves to the document's own
  origin or to a declared host, and any other host is refused with the host named before a request is made.
- A style may name its resources relatively, as the style specification allows. The fetchers resolve a
  relative reference against the document's own directory, or against its origin for a root-relative one,
  and refuse a reference that climbs above that directory rather than guessing at a wider scope.
- The browser install now draws a provider document's own labels and icons. The renderer's PNG decoder
  builds on wasm, so `fetch_hosted_sprite_atlas` decodes the sheet in the page as the native surface does;
  the canvas packs the document's glyph ranges into a `LabelFont` and prefetches the sprite atlas, cached
  per provider, document address and resource. A document address the page names relatively is made
  absolute against the page's own origin once, so the document's own resource references resolve against a
  single address.
- The committed synthetic vendor documents name their `glyphs` endpoint and `sprite` base relatively, and
  the registry serves the Latin ranges and the sprite index and sheet, so the browser's provider-backed
  base case asserts the install reaches every declared resource route through the provider's client.

### Fixed (2026-11-13 — a recorded route without the geometry field)

- `RouteData.geometry` decodes a recorded document that carries no `geometry` member to an empty list,
  the shape the field already documents for a provider that reports per-step shapes only. The shared
  reference scenes and the committed navigation route and trace fixtures, recorded before the field
  existed, load again instead of failing the deserialization that every navigation fixture test hit.

### Added (2026-11-13 — hosted glyph ranges for a provider-published style)

- `HostedGlyphRanges` fetches a hosted style's glyph ranges through the provider's authenticated style
  resource client and retains them per provider, font stack and range, so two providers serving the same
  font-stack name never share a range and a repeated frame costs no request. The style document's
  `glyphs` template must carry the font-stack and range placeholders and must stay on the origin the
  document was loaded from; an address on any other origin is refused before a request is made, so a
  style document cannot aim the provider credential at another host.
- `CompiledStyle` carries the document's `glyphs` endpoint and reports the distinct font stacks its
  symbol layers name, so a host can fetch exactly the ranges the labels need; `HostedGlyphRanges`
  requests both Latin ranges and combines them into the single stack the label atlas packs.

### Fixed (2026-11-13 — the web demo installs only after the SDK root exists)

- The web demo no longer races its own wasm load. A page whose module scripts ran while the wasm binary
  was still arriving could throw `wasm is undefined` from an eagerly constructed location-puck animator
  and leave the page half-installed with readiness still reported. The animator now defers its Rust
  object to first use, a frame requested before the bound surface exists is dropped, and the page's
  whole installation runs from one entry point invoked by the resolved readiness promise — controls and
  seams first, the surface and its first frame last. A browser case holds the wasm response back and
  asserts the complete install with no page error.

### Added (2026-11-12 — the credentialed vector families on Android, iOS and React Native)

- The Kotlin and Swift `MapView` wrappers expose `setProviderVectorTileSourceFromDocument(source,
  styleDocumentUrl)`, the asynchronous member that installs a provider-backed vector base painted with the
  style document the provider publishes; the React Native package bridges it on both native halves and
  exposes it on the typed `MapViewHandle`.
- The Android, iOS and React Native layers controls offer a credentialed provider's vector family once
  its credential is applied and install it through that member, so the application never handles the
  credential and ships no vendor style file.
- Both reference applications pass the empty route `geometry` the contract requires for a route whose
  steps carry the shapes, which the regenerated bindings surfaced as a latent build break.

### Added (2026-11-12 — the credentialed vector families in the web layers control)

- The web layers control lists a credentialed provider's vector family beside the keyless ones. The row
  names the provider and style, states the credential it needs, and stays disabled until the key is
  applied; selecting it installs the family through `setProviderVectorTileSourceFromDocument`, so the
  style document is fetched by the provider's authenticated client. The keyless families keep their rows
  first, and a browser case covers the refusal, the install and the drawn tiles against committed
  fixtures.

### Added (2026-11-12 — the registry rows for the credentialed vector families)

- HERE and Mapbox declare the `vector_tiles` served contract in the shared registry, with a served zoom
  span of 17 and 22 respectively and one vector style each (`omv-light`, `streets-light`). A style entry
  now names either a committed style or the provider's published style document through `document_url`;
  the address is credential-free, and the provider client appends the credential at fetch time. The
  registry test and the platform parity floor refuse a document address that is not `https` or that
  carries a query, so a credential cannot enter the shared artifact. The five provider-capability pages
  name both families, their style ids and the fetch-and-cache behaviour.

### Added (2026-11-12 — the provider's published style document loads at selection)

- `MapView.set_provider_vector_tile_source_from_document(source, style_document_url)` is the additive
  asynchronous sibling of `set_provider_vector_tile_source`. It fetches the style document the configured
  provider publishes through the provider's authenticated style-resource client, so the credential stays in
  the provider layer, compiles the document in the renderer, and installs it on the vector base. A document
  that compiled is kept per provider key and address, so installing the same family again costs no request;
  a malformed document is never cached; and every refusal (a missing provider, a provider without a style
  fetch, a failed request, an uncompilable document) throws and leaves the installed base in force.
- `ProviderRegistry` and `MapSdk` gain the vector-tile style-resource slot, filled beside the provider by
  both catalog installers and mirrored to the browser canvas. The wasm canvas serves
  `setProviderVectorTileSourceFromDocument` with the same failure codes as the native member.
- Each credentialed vector provider's descriptor declares a `style_document` fixture route over its
  committed synthetic document, mirrored to the iOS registry, so the suites never reach the network and the
  vendor's own style file is never committed. Four native tests and one browser case cover install, the
  cached reinstall, the malformed-document refusal and the missing-fetch refusal.

### Added (2026-11-12 — the provider-authenticated style-resource fetch)

- `rustic_maps_core::StyleResourceClient` names the fetch of one rendering style resource (a style
  document, a hosted glyph range or a sprite sheet) and `VendorImplementations::style_resources` carries
  the implementation for a vendor that publishes one. HERE and Mapbox build it beside the same client that
  serves their contract calls, so the request carries the registration credential through the client's own
  authentication, transport, pacing, retry policy and redacted observation. The SDK names only the resource
  address: it retains no credential and holds no vendor URL shape, and the provider layer's
  `VectorTileProvider` contract is unchanged.
- `vendor_style_resources.rs` fetches a committed synthetic document per provider through a loopback
  server: the registered credential reaches the request through the client, the bytes are the committed
  document, and the fetched document compiles into a vector style. Vendors without a hosted vector style
  declare no style resources. No test reaches the network.

### Added (2026-11-12 — the tile-serving vendors' recorded payloads)

- TomTom's raster tile route pins one recorded Map Display PNG from the rendering tile set, and HERE's
  and Mapbox's vector tile routes share one recorded vector tile from the committed set rather than
  copying it into each provider's directory. The new `vendor_provider_tiles.rs` drives the three
  fetches through the provider layer and the SDK bridge on a loopback server, asserting the recorded
  bytes and the tile address survive the bridge and that each fetch is one request. No test reaches
  the network.
- The vector-layer and tile-switch browser suites now select their descriptor by its URL-template
  source rather than by the first vector-tile fixture route, because the credentialed vector
  providers declare fixture routes too; the suites test the URL-template path they are about.

### Changed (2026-11-12 — the provider documentation gate checks the inverse claim)

- `scripts/check-provider-docs.mjs` now parses every capability page's provider rows and refuses a row that
  offers a registrable provider the registry does not mark available on that target, claims a capability its
  descriptor does not serve, or names a capability phrase the gate has not mapped. A base-map row with no
  registration alias is outside the availability check, because the layers control installs it rather than
  the capability picker. Both refusals are negative-tested, and the gate still passes on the current pages
  (5 pages against 12 descriptors).

### Added (2026-11-12 — the suggestion line and class pinned for every place-search provider)

- Every place-search fixture route in `platform/providers.json` now pins the first suggestion its recorded
  payload must produce (`expectations.suggestion_text` and `suggestion_quality`), mirrored to the iOS
  registry. The new `place_suggestion_golden.spec.mjs` asserts the line, the class and the text the chosen
  row puts in the field for every provider that serves type-ahead: the full-service's published airport line,
  whose provider classifies nothing so the SDK's documented point default shows, and Photon's Capitol Mall,
  whose feature class maps to Street. The earlier append-the-city and always-point mappings each fail both
  cases.
- `provider_request_diagnostics.spec.mjs` now proves one redacted observation per credentialed geocoding
  provider the browser can register, with the typed fields and without the credential in the line, the
  object or the request path, so a diagnostics module-path regression fails for every provider.

### Added (2026-11-12 — the remaining vendors' recorded routing and traffic payloads)

- Google, TomTom, Mapbox and Radar routing payloads and TomTom's traffic flow and incident payloads join
  the committed fixtures, their registry descriptor routes and the iOS registry mirror. `vendor_provider_routing.rs`
  now drives the full-service, HERE, Google, TomTom, Mapbox and Radar routes through the provider layer and the
  SDK bridge on a loopback server, asserting each route's distance, decoded geometry and legs; the new
  `vendor_provider_traffic.rs` does the same for the full-service, HERE and TomTom traffic calls. MapQuest's
  five placed incidents convert; HERE's and TomTom's incidents convert to none because the provider layer
  reports no incident coordinate for either and TomTom's type arrives capitalised, which is recorded as an
  upstream fix and pinned by the assertions. No test reaches the network.

### Changed (2026-11-11 — the SDK moves to everymap 0.4.7)

- The provider layer's 0.4.7 line is adopted. It carries the redaction and release-integrity batch: a
  redacted `Debug` for HTTP requests and the MapQuest configuration, a classified transport error that
  never prints the request URL, a lock-free OAuth 2.0 token fetch bounded by a timeout, one shared Radar
  transport-mode table, TomTom traffic bounding boxes in the documented order, and the documented Mapbox
  permanent and batch geocoding endpoints. The corrected Google Routes field mask in the same line is
  what makes live Google routing work now that its billing is enabled.

### Added (2026-11-10 — HERE's recorded routing and traffic payloads)

- HERE's recorded routing and traffic payloads join the committed fixtures and its registry descriptor
  routes (`^routes$` for POST, `^incidents$` and `^flow$` for GET), with the iOS registry mirror in step.
  `vendor_provider_routing.rs` drives the recorded route through the provider and the SDK bridge on a
  loopback server, asserting its distance, decoded geometry and legs; no test reaches the network.

### Added (2026-11-10 — a vector base served by a configured provider)

- `ProviderVectorTileSource` and `MapView.set_provider_vector_tile_source(source)` install a styled
  vector base through the provider registered for the vector-tiles capability. The provider supplies
  the tileset name, attribution line and zoom span its descriptor declares; the application chooses
  only the provider key and the committed style. The browser canvas serves the same member as
  `setProviderVectorTileSource({providerKey, styleId})`. A source naming a provider other than the
  installed one, or an unknown style, is refused with a named reason and leaves any installed base in
  force; the layers control and per-schema vendor styles follow on the recorded terms research.

### Changed (2026-11-10 — the SDK moves to everymap 0.4.3)

- The provider layer's 0.4.3 release is adopted: the Radar geocoder takes one service root, and the
  Google and Radar fixture routes now name the documented paths their clients request
  (`maps/api/geocode/json` and `v1/geocode/forward`), so the browser provider matrix and the native
  integration fixtures stay same-origin while every provider answers through the new service-root
  endpoints.
- The iOS build pins the deployment target to 17.0, the floor the Swift package declares. Rust's iOS
  targets default to 10.0, which is older than the prebuilt C objects in the dependency graph and made
  the link fail on symbols those objects require.

### Changed (breaking, 2026-11-10 — the whole-set registration names its provider)

- `MapSdk.set_provider_credential(credential)` is removed. The catalog carries six credentialed
  providers, and a bare credential cannot say which set it means. `MapSdk.configure_provider_set(provider_key,
  credential)` registers every capability the named provider serves from one credential; the
  per-capability `configure_provider` and the keyed `set_provider_credential_with_tile_cache` keep their
  shapes. The browser's `setProviderCredential(credential)` export becomes
  `configureProviderSet(providerKey, credential)`, and the React Native native runtimes call the keyed
  member. Regenerated bindings ship with the change.

### Added (2026-11-10 — an opt-in response excerpt in the provider console)

- `ProviderRequestTrace` gains an additive `response_excerpt`: while the new
  `MapSdk.set_response_excerpt_capture(true)` switch is on, each provider request observation carries
  the first 512 bytes of the response body, truncated on a character boundary, with credential-bearing
  JSON members and `name=value` assignments replaced. The switch is off by default and is process-wide.
- The web demo's provider console gains a **Capture response excerpts** switch: rows show the redacted
  prefix beneath the request line, and the text copy and JSON export carry it as `responseExcerpt`.
  A browser case proves nothing is captured before the switch, the captured prefix is bounded and
  credential-free afterwards, and the export carries the typed field. The console chapter and the design
  note describe the switch and its privacy boundary.

### Changed (2026-11-10 — keyless providers report their requests)

- OSRM, Valhalla and Photon clients are built with the same request observer the credentialed vendors use,
  on native and in the browser, so their requests appear in the diagnostics stream and the console. The
  provider matrix case now exercises every registrable provider, keyless included.

### Changed (2026-11-10 — provider vector tiles register through the catalog)

- `VendorImplementations` carries a vector-tile slot, and HERE and Mapbox build their typed vector
  providers from the same vendor construction path. Both catalog tables declare `vector_tiles` for
  them, and the native catalog and the browser dispatch install the supplied provider on the
  registry's vector-tile slot, so `configure_provider(VectorTiles, …)` is served instead of refused.

### Fixed (2026-11-10 — React Native view lifecycle, preferences and readiness)

- The iOS host remembers the JavaScript render-pause request so a later window attach cannot resume a
  paused view, tears the native objects down from the view manager's dismantle hook and on final removal
  from the hierarchy, and the build script stages the generated binding the pod's symlink names.
- A stored reference-app preferences record written by an older version is migrated through an explicit
  ladder, keeping every recognizable choice; a newer record is still ignored as a whole.
- A late JavaScript subscriber learns the map is ready through a native `isMapReady` readback and a
  one-shot delivery guard shared with the `mapReady` event.

### Changed (2026-11-10 — one namespaced tile cache)

- Raster and vector tile cache fingerprints carry a `tile_source_namespace`: the catalog provider
  key for a catalog provider, or a stable caller-supplied key for an application provider. One cache
  file can no longer serve one provider's tiles for another, and the namespace never carries a
  credential or a raw URL.
- A tile pack is identified by its namespace and its caller-chosen identifier together, and its
  record reports the namespace, so two providers' packs with the same identifier coexist and are
  reported and removed independently. A schema migration purges the pre-namespace raster and vector
  entries and pack rows once; unrelated entries survive.
- The vector-tile payload key lives once in `rustic-maps-core::tile_cache_keys`, so the disk wrapper
  and the browser's in-memory payload cache store a tile under the same key. The cached raster and
  vector wrappers share one cache handle (store, namespace, retention, executor), and a cached tile
  wrapper cannot be constructed without a namespace.

### Changed (2026-11-06 — the provider console and everymap 0.4.1)

- The demo opens a provider console: one row per request with the provider's display name, contract, method, redacted path, status class and duration, provider and outcome filters, copy as text and a credential-free JSON export. The browser diagnostics event and `LogEvent` carry the typed provider facts, attribution names only the providers in use, one vendor construction path and one provider table replace the per-binding copies, a provider failure names its provider, routes carry their full geometry, and the SDK moves to everymap-rs 0.4.1.

### Fixed (2026-11-05 — providers selectable, observable and on the provider layer)

- HERE, Google, TomTom, Mapbox and Radar become selectable on all four targets; the SDK pins everymap-rs 0.4.0 and enforces the tile-source ceiling; keyless OSRM, Valhalla and Photon report through the shared observer; suggestions keep their provider's line and quality; the Photon and OpenStreetMap US crates move to the provider layer; north-up holds during navigation and the puck glides between fixes; the debug centre mark is gone.
- The recorded MapQuest session id is gone from the history and the release tags of the provider layer, and its crates.io tarballs are recorded as the remaining residual.
- The recorded MapQuest route fixture no longer carries the session id from the original capture; it holds a synthetic placeholder, and the same value was removed from the provider layer's history and release tags.

### Changed (2026-11-04 — the provider layer takes over, the renderer completes and six decisions land)

- MapQuest, OSRM, Valhalla and the URL-template sources run on everymap-rs 0.3.0 and the SDK consumes the published crates; the type-ahead's empty and failure copy is shared; every provider request reaches the diagnostics ring; the wasm contract mirror is generated from the UDL; `MapView.reload_tile_source()` replaces four per-platform reloads; the unused wasm label seam and the second label pipeline are gone; shipped packages carry only SDK code; release re-runs skip unchanged platforms; capture-free `snapshot()` lands.
- Cartography and labels: road names keep eight of the frame's label slots, POI labels and the style-layer coverage audit land, labels collide per glyph and rank by class, style sort keys and transitions land, gestures draw from cache and settle exactly, the browser and native renderers agree on the regional scene, ferry crossings honour the mode/strategy contract, and icons draw from a sprite atlas.
- Reference apps and screenshots: the RN plan flow matches iOS, the plan-flow audit records its per-step verdicts, the device location source is remembered, all 24 published frames and the keyed four-frame set come from one revision, the layers control is pictured on every platform, and the raster tile path is measured as not the frame cost.
- The provider layer moves to everymap-rs 0.3.0: its transport and navigation PRs are approved to merge, the upstream release build-out completes, and this SDK then consumes it directly.
- The label budget keeps a fixed number of slots for line-placed road names, and route shields become a dedicated overlay kind drawing shaped badges from `routes.ref`.
- Diagnostics gain the typed `ProviderRequestTrace` on `LogEvent`, the contract `MapView` gains camera and fit members with `MapCamera.interpolate`, and the React Native declaration and ordinal layer are generated from the UDL.
- Every provider client, including explicit-transport constructions for OSRM, Valhalla and Photon, executes through the observed transport, and a bridge emits one structured, redacted observation per attempt (provider, contract, method, path, status class, outcome, duration, bytes) into the existing diagnostics ring and live subscription on native and the web. Tests assert exactly one event per request and that no credential ever appears.

### Changed (2026-11-03 — reference-app convergence, type-ahead, zoom cross-fade and offline packs)

- The reference-app surface converges on one 40-unit control token, one layers control and no demo pickers; Photon and the shared type-ahead rule reach all four targets; descriptors declare tile pacing and fetches run through an 8-permit pool; a zoom change cross-fades through the retired tile level; the renderer draws the two-mode location puck; the traffic cache's counters cross the contract; offline tile packs land end to end; `map-refapp-core` ships once to wasm and native; and the platform-hardening batches close their audit findings, including the iOS `PrivacyInfo.xcprivacy` manifest and the generic tile-cache contract member.

### Changed (2026-11-02 — the MapLibre style adoption, one artifact per platform and the first hardening batch)

- The MapLibre style adoption lands in four slices (documents and expressions, compiled render plans, provider-bound sources, line/fill/circle paint, symbol text and placement); label anchors, rotation, wrapping and radial offsets follow; the Android release becomes one AAR and the WebAssembly release one npm tarball; the keyless raster fallback no longer outranks a configured provider; the first platform-hardening batch and the browser surface hardening land; the demotiles style renders end to end; and the site and demo are republished from the current tree.

### Earlier releases

- **Cartography and provider convergence** — Regional road hierarchy and longer-name drawing, remembered choices, a provider per capability and the on-map layer control, the browser contract-parity gate, one truth about provider availability, the navigable internal docs set, and the bounded traffic cache.
- **Platform and product** — Device-driven navigation on every app, GPU upload buffers, browser goldens per vector style, generated API references and downloads, the product identity rename, the UniFFI 0.31.2 alignment, the browser canvas folded into one wasm module, wasm-safe clocks, and the Rust crates renamed `rustic-maps-*`.
- **The wasm seam and the documentation set** — Live guidance, offline routing, search and route options on the wasm seam; device positioning on the browser and native targets; the tutorial worked example; the documentation overhaul and its snippet gate; downloads rendered from the release manifest; the wasm optimization pass; and the vector base's concurrent fetch work.
- **Parity and presentation** — Provider-neutral base maps, trip-planning parity and canvas interaction, vector cartography and the shared native base, the three-way theme, the accessible web demo and the four-platform capability audit, the hosted mobile demo and frame-budget harness, token-styled control sizes, and the tutorial/snippet pipeline.
- **Bindings and providers** — Runtime credentials, the React Native reference application, live browser providers, the MVT decoder and vector provider seam, provider genericity on web/wasm and on native, the tag-driven release pipeline, the API v2 naming slices (the breaking renames are recorded verbatim below), `MapView` in the Android AAR and the Swift package, and the vendor-neutral URL-template vector source.
- **Foundations** — The MVT decoder, the React Native iOS shell, the product docs site, packaging and release design, the vector provider seam, simulated trips and speech, the offline-routing slices, and the renderer and binding slices.
- The Sacramento re-anchor moved every demo and test anchor to Capitol Park; its 216 live-recorded raster tiles, goldens, screenshots and three MapQuest payload fixtures were re-recorded with the rotated key and carry `mapUrl` credentials redacted as `key=REDACTED`.

### Recorded decisions and security notes

- The Android and iOS reference applications ship as `com.rusticmaps.sdk.referenceapplication`, the React Native shell as `com.rusticmaps.sdk.refapprn`, and the shipped Kotlin wrapper package is `com.rusticmaps.sdk` (breaking for Android integrators of the wrapper, with the JNI symbols, R8 rule and tutorial imports moved atomically). The frozen package identities, the credential storage key and the Appetize build ids are unchanged; the display name stays *Rustic Maps SDK Ref App*.

- A four-platform capability and UX audit inventoried the frozen contract (15 interfaces, 4 foreign traits, 47 dictionaries, 23 enums, 82 interface members, 12 errors) and scored every capability area per platform with file:line evidence. Android and iOS were at binding parity; React Native had generated-binding parity but a narrower curated API; the browser surface was narrower at the time. The decisions that followed: parity closes through one synced surface, the web reference app reaches the v2 flow, labels stay renderer-internal, and React Native JavaScript gets the curated provider/navigation surface (PRD §6 items 19–22).

- The Android, iOS and React Native reference apps now open keylessly on **OpenFreeMap Bright**, the registry's declared styled vector base, instead of the plain OpenStreetMap raster (PRD §6 item 18). `platform/providers.json` gained a top-level `keyless_base_map` (`provider_key` plus committed `style_id`); each shell resolves it against the descriptors and installs the style over the credential-less raster source, which stays registered so the shipped surface can attach (the surface contract still requires a raster provider). A registry that omits the field keeps the raster default, an explicit base-map choice or the demo launch argument always wins, and an applied credential is never hidden under the default. The React Native shell gained its missing keyless start registration as part of the same change.

- A hosted-demo build can carry its own provider credential. `scripts/inject-hosted-demo-credential.sh` writes it into the Android app's assets (and the React Native shell's) on the build machine from a repository secret; the iOS app reads it from `Info.plist` expanded from the build setting. It is applied through the same runtime registration path as the settings surface — never persisted, never logged — and every build made without it stays keyless. `docs/Providers.md` (the hosted-demo exception) records the bounded exception to the "no key in a build" rule.

- The Android SDK uses the `com.rusticmaps.sdk` Kotlin package. The reference
  application ships as `com.rusticmaps.sdk.referenceapplication`, the React
  Native shell as `com.rusticmaps.sdk.refapprn`, and the native window bridge
  binds `Java_com_rusticmaps_sdk_NativeWindowBridge_*` in `map-native-core`;
  the Gradle namespaces, CI path filters, the RN shell's iOS bundle id and the
  tutorial's Kotlin import follow the same package.

- **Stale key seam removed**: a gitignored `Local.xcconfig` and the
  `MAPQUEST_API_KEY` line in `platform/android/local.properties` from the old
  build-time path were deleted. **The removed key should be rotated.**

- **Runtime provider credentials design** (`docs/Providers.md`): the model for
  removing build-time provider keys from every refapp — a descriptor-driven
  settings surface, masked entry, explicit apply/clear, opt-in secure
  persistence (Keychain / `EncryptedSharedPreferences`), honest keyless
  states, and future HERE/Google/TomTom/Mapbox/Radar rows.

- **Runtime credential injection for automation**: local and CI UI automation
  can supply a key at launch without a build-time seam — the Android/RN
  launch-intent extra `providerApiKey` and the iOS/RN `PROVIDER_API_KEY` launch
  environment are applied through the public registration path, never persisted
  by the injection and never logged. The Compose and XCUITest suites are
  keyless-aware: they assert the neutral prompt and skip provider-dependent
  cases when no key is supplied, and run keyed when the variable is present.

- The hosted-demo upload script and `platform/demo-builds.json` keep public
  build ids stable, with the session limits and the docs-site referrer
  restriction applied at upload time, and a public manifest holding no secret.

- **Packaging & release design** (`docs/Release.md`): the four decisions
  (GitHub Releases only; angoratek namespace; docs-site `/demo` hosting;
  tag-driven release workflow) turned
  into a concrete artifact matrix from measured tree sizes, a release.yml
  design (gates-first on the tagged ref, keyless cosign, SHA-256 sidecars,
  changelog-derived release notes), the SPM `binaryTarget` release
  distribution, npm files allow-lists, the docs-site `/demo` requirements
  and copy plan, and the single-workspace-version policy; cited public
  reference: MapLibre and Ferrostar release practices.

- Tile imagery caches heavily by default (2026-10-28,
  PRD §6.7/FR-C): `rustic-maps-cache`'s tiles category default retention moves
  7 → **30 calendar days** (the common maximum under imagery-provider
  terms), the MapQuest catalog declares its own terms as the documented
  `MAPQUEST_RASTER_TILE_CACHE_CEILING_SECONDS` ceiling, and a lockstep test
  keeps the cache default exactly equal to the MapQuest terms ceiling.

### Changed (API v2, first slice — WebAssembly naming)

- **The WebAssembly surface speaks the contract's names.** Every wasm export
  now publishes a camelCase JavaScript name through `#[wasm_bindgen(js_name)]`,
  so a browser developer writes `mapView.renderFrame(camera, fieldOfView)`,
  `mapView.setOverlays(overlays)`, `forwardGeocode(query)`,
  `calculateRoute(...)`, `fetchRasterTile(...)`, `registerProvider(...)` and
  `initializeSdk()` instead of the Rust spellings. The canvas class is
  `MapView` (it was `GpuMapCanvas`), and the enums are `TileResolution`,
  `TrafficFlowCongestionLevel` and `SdkLogSeverity`.
- **The contract-sync gate reads `js_name`.** It compares the wasm surface and
  the generated TypeScript through an explicit camel↔snake mapping, so the
  browser names and the native contract cannot drift apart; wasm-bindgen's own
  `init`/`initSync` exports are excluded by name.

### Changed (API v2, second slice — public type names)

- **The contract's type names now use the v2 vocabulary.** One breaking change
  renames every developer-visible type, shipping its regenerated bindings, all
  four reference applications and the documentation in the same commit:
  `MapSurfaceView` → `MapView`, `MapSurfaceTargetHandle` → `SurfaceHandle`,
  `MapNavigationController` → `Navigator`,
  `NavigationStateEventListener` → `NavigationListener`,
  `NavigationTopicSubscription` → `Subscription`,
  `MapOverlayDescriptor` → `MapOverlay` (`MapOverlayKind` is unchanged),
  `PlaceResult` → `Place`, and `TileFrameStatistics` → `FrameStatistics`.
- **Migration**: replace the old type names at every call site; members and
  methods keep their current spellings until the next API v2 slice. Swift call
  sites that import Combine should qualify the subscription handle as
  `RusticMapsSDK.Subscription` to disambiguate it from `Combine.Subscription`.
  The internal core `PlaceResult` (the `geospatial-types` record the provider
  crates return) is not the FFI record and keeps its name.

### Changed (API v2, third slice — member/verb and record names)

- **The contract's methods and records now use the v2 vocabulary.** One
  breaking change renames the remaining developer-visible members and records,
  shipping its regenerated bindings, all four reference applications and the
  documentation together. `forward_geocode` deliberately keeps its name: it is
  the geocoding verb, not a search verb.
- **Migration table** (`old` → `new`, native snake_case / Kotlin+Swift+RN
  camelCase / wasm camelCase):

  | Area | Old | New |
  |---|---|---|
  | `Navigator` | `start_trip` | `start_navigation` (`startNavigation`) |
  | `Navigator` | `provide_location_update` | `update_location` (`updateLocation`) |
  | `Navigator` | `provide_reroute` | `apply_reroute` (`applyReroute`) |
  | `MapView` | `frame_settled` | `is_frame_settled` (`isFrameSettled`) |
  | `MapSdk` | `search_places` | `search` (`searchPlaces` → `search`) |
  | `MapSdk` | `search_autocomplete` | `suggest` (`searchAutocomplete` → `suggest`) |
  | `MapSdk` | `register_mapquest_providers(string key)` | `set_provider_credential(string credential)` (`registerMapquestProviders` → `setProviderCredential`) |
  | `MapSdk` | `register_mapquest_providers_with_tile_cache(...)` | `set_provider_credential_with_tile_cache(...)` |
  | record | `RouteData` | `Route` |
  | record | `ViewportPixels` | `ViewportSize` |
  | record | `EdgeInsetsPixels` | `EdgeInsets` |
  | record | `TileKeyRecord` | `TileKey` |
  | record | `DiagnosticLogEvent` | `LogEvent` |

- **Swift note**: `EdgeInsets` shadows `SwiftUI.EdgeInsets`. Views that import
  both should qualify the SDK record as `RusticMapsSDK.EdgeInsets`; the
  reference app compiles unqualified because its model file does not import
  SwiftUI.
- **Not renamed**: the UDL namespace, `set_overlays`/`clear_overlays` (the
  add/update/remove overlay API is a later slice), the engine interfaces, the
  `MapCamera` gesture members and `forward_geocode`. The internal
  `geospatial-types` `RouteData`, the `map-rendering`
  `ViewportPixels`/`EdgeInsetsPixels`/`TileKey` types and the provider-trait
  `search_places`/`search_autocomplete` methods are not the FFI records and
  keep their names. `RouteLeg` and `RouteStep` are unchanged.
- **Wasm surface**: the vendor-named `registerMapquestProviders` export is now
  `setProviderCredential`; the TypeScript mirror uses the renamed record
  interfaces.

## [0.1.2] — 2026-10-27

- The disk cache: SQLite with pooled WAL connections, order-independent SHA-256 request fingerprints, per-category TTLs and overrides, an absolute/free-disk byte quota, and least-recently-used eviction behind the cache-then-network wrappers for all seven implemented contracts.

## [0.1.1] — 2026-10-27

- The MapQuest provider: seven documented APIs behind a shared rate-limited client, fixtures recorded from the live platform with wiremock end-to-end tests, the retired-elevation and flow-shape notes, retina tile resolution, the fly-to arc and fit-bounds contract surface, overlay rendering with goldens, the Android, iOS and React Native packaging pipelines, navigation instruction composition and per-topic observers, the navigation UDL wiring, the wasm overlay canvas, recorded GPS traces driving end-to-end navigation, the native-device parity fixes, the Swift UI wrapper and SwiftUI reference app, the Kotlin and Compose reference apps, the native surface renderer contract, and the reference-UX language with its shared demo artifact.

## [0.1.0] — 2026-10-27

- The founding records and the first workspace: PRD 0.2 and the execution plan locking UniFFI UDL-first bindings, the own `wgpu` raster renderer, the flatdata offline graph and WebAssembly as a first-class target; the nine-crate Cargo workspace; the typed provider registry and cache-then-network seam; nine vendor-neutral contracts with mocks; and the format, clippy, nextest and cargo-deny gates.
