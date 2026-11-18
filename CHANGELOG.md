# Changelog — Rustic Maps SDK

All meaningful changes to the SDK are recorded here. Dates are ISO-8601.

## [Unreleased]

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

### Fixed (2026-11-16 — platform hardening and the round's audit repairs)

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

### Added (2026-11-06 — the demo's provider console)

- The web demo opens a provider console that lists one row per provider request, prefixed with the
  provider's display name: contract, method, redacted path, status class and duration. Rows filter by
  provider and outcome, copy as text and export as JSON, and never carry a credential. The published
  **See provider requests in the demo** chapter explains the fields, the outcome words and how to report a
  provider failure. Browser cases cover geocoding and routing rows, filtering and a credential-free export.

### Added (2026-11-06 — the browser diagnostics event carries typed provider facts)

- Each flushed browser diagnostic event now carries a `providerRequest` object when it is a provider
  request: provider key, contract, HTTP method, redacted path, status class, outcome, duration and response
  bytes. The browser console can read fields instead of parsing the message line. Integer fields that
  `tracing` delivers through `record_debug` are read there too, so duration and byte counts are no longer
  dropped from the structured record.

### Changed (2026-11-06 — attribution names only the providers in use)

- The map's attribution line now carries the base-map source, the traffic provider while the overlay is on
  and the routing provider while a route is shown, instead of every provider in the registry. The settings
  panel keeps the full configured set and its terms, so the compliance text is unchanged.

### Changed (2026-11-06 — one vendor construction path)

- The five credentialed vendor builders and their credential parameters moved out of the native catalog and
  the browser dispatch into `rustic-maps-providers`; both bindings construct every vendor through the same
  function, including the test-only base URL override. The two copies of the request-observer wiring and the
  credential parameter names are gone.

### Changed (2026-11-06 — one provider table)

- The browser dispatch derives each vendor's display name and served contracts from `rustic-maps-providers`,
  the new shared catalog table; the native catalog is pinned to the same table by a parity test. A provider can
  no longer be advertised on one target and refused on another.

### Changed (2026-11-06 — a provider failure names its provider)

- A provider-caused refusal in the web demo now carries the provider's display name as a prefix when the
  detail does not already name it, in the credential rows and the capability picker, so a notification says
  which provider answered.

### Added (2026-11-06 — a route carries its full geometry)

- The `Route` record gains an additive `geometry` sequence: the provider's full route polyline in traversal order. HERE, Radar, Mapbox and TomTom report the shape as one polyline and no per-step geometry, so before this change their routes reached the demo with no line and a `(0,0)` box. The bridge now carries the polyline, derives the bounding box from it when the provider reports none, the wasm surface emits it, and the demo draws and frames from it, falling back to step shapes for providers that only report those.

### Changed (2026-11-06 — the SDK moves to everymap 0.4.1)

- The provider-layer dependencies move to 0.4.1. Mapbox, Radar and Google routes now decode their encoded shapes with one bounded decoder and report `INVALID_POLYLINE` instead of an empty geometry; the browser transport drops Fetch-forbidden headers, so MapQuest tiles and URL-template sources stop preflighting and failing with a 404 `OPTIONS`; and Radar's live step payloads deserialize again after a duplicate `maneuver` alias was removed.
- The recorded MapQuest session id is gone from the history and the release tags of the provider layer, and its crates.io tarballs are recorded as the remaining residual.

### Fixed (2026-11-05 — provider selection and evidence)

- The recorded MapQuest route fixture no longer carries the session id from the original capture; it holds a synthetic placeholder, and the same value was removed from the provider layer's history and release tags.

- The Android disk-cache owner is keyed off the descriptor's declared caching ceiling, so applying a TomTom key no longer attempts a cache registration the terms do not support; the wasm traffic-cache statistics report the serving vendor's counters, and a vendor raster base fetches past the recorded-fixture zoom.
- The provider-name guard scans `mapbox` again and strips only the registered MVT media type and the format's identifier; two shell comments that named the vendor are reworded.
- The browser dispatch carries the same display names and credential trimming as the native catalog, the unreachable install-all layer is deleted, and the catalog test now exercises `configure_provider` and the registration readback rather than calling the installer directly.

### Added (2026-11-05 — HERE, Google, TomTom, Mapbox and Radar are selectable)

- Five vendor providers join the build on native and in the browser, beside MapQuest, Photon, OSRM and Valhalla. Each takes its issued credential at runtime: HERE and TomTom a query-parameter key, Google a query-parameter key, Mapbox an access token, Radar the `Authorization` header. Their served capabilities are the ones the SDK's bridge carries: HERE and TomTom also serve traffic, and TomTom serves raster tiles.
- The native catalog carries nine callable keys and the browser dispatch a table of vendor rows; adding a provider is one row plus its registry descriptor. `platform/providers.json` carries twelve descriptors with attribution, terms, availability and the `wasm_registration` alias, and the provider-name guard understands that some display names are ordinary words.
- A descriptor that declares a raster-tile caching ceiling installs its tiles through the disk cache; every other provider registers one capability per call. The iOS, React Native and web credential rows now register through the catalog instead of a fixed full-service export.


### Changed (2026-11-05 — the SDK pins everymap-rs 0.4.0 and enforces the tile-source ceiling)

- The seven provider-layer dependencies move from 0.3 to 0.4. The 0.4 line publishes eleven provider crates and adds `AutocompleteResult::quality`, which the bridge maps onto the SDK's granularity scale.
- A registered tile source's ten-second ceiling is enforced by the provider layer: a request whose budget cannot fund it inside the ceiling is refused with `RateLimitExhausted` instead of waiting for the next refill. The native integration test asserts the refusal, and the stale note that the ceiling could not be expressed is gone.

### Docs (2026-11-05 — provider pages tell the provider-layer story)

- The five provider-capability pages, the Rust page and the add-a-provider guide now describe the published provider layer and the four keys this build registers: MapQuest, Photon, OSRM and Valhalla. Photon, OSRM and Valhalla rows, attribution and keyless registration are named wherever they were missing.
- Deleted names are gone: `provider-transport`, `mapquest-provider`, `photon-provider`, `osrm-provider`, `valhalla-provider`, `url-template-raster-provider`, `url-template-vector-provider` and `openstreetmap-us-vector-provider`, together with the type names that moved to the provider layer (`MapQuestGeocoder`, `MapQuestRouter`, `MapQuestDistanceMatrix`, `OsrmRouter`, the URL-template constructors).
- The landing, quickstart, how-it-works, first-map, live-demo, UDL-contract and provider-mixing chapters name the same provider set, and the workspace documents carry the corrected registration calls, attribution rendering and mirror notes.

### Fixed (2026-11-05 — suggestions keep their provider's line and quality)

- A type-ahead suggestion's display text is the line its provider published, so choosing a row puts in the field the text the settled search answers for. The bridge had appended the candidate's city, which turned "Capitol Mall" into "Capitol Mall, Sacramento" and broke the recorded-flow expectations.
- A suggestion reports the provider's match granularity instead of assuming the most precise class: everymap-rs's `AutocompleteResult` gained an additive `quality` field, Photon fills it from the feature class, and the bridge maps it onto the SDK's scale with the point default only when a provider classifies nothing.

### Fixed (2026-11-05 — provider request observations reach the browser diagnostics again)

- The web and native diagnostics rings record events by module-path prefix, and the prefix list still named the deleted `provider_transport` crate. Provider observations are emitted from `rustic_maps_core` now, so every request went unrecorded; the list names the core crate and the dead entry is gone.

### Changed (2026-11-05 — the debug centre mark is gone and the puck's tokens are mirrored)

- The web playground no longer paints a red mark at the canvas centre. The renderer's location indicator is the only position cue on every target, and the design-language notes that named the mark are updated.
- The three location-animation tokens — `location-animation-duration-milliseconds`, `location-animation-minimum-milliseconds` and `location-animation-dead-reckoning` — are mirrored in the Android, iOS, React Native and web shells. The UX parity gate reads the string-valued choice as well as the numeric rows, so the design artifact and the four consumer surfaces cannot drift.

### Changed (2026-11-05 — Photon and OpenStreetMap US join the provider layer)

- Geocoding, place search and type-ahead move to everymap-rs's Photon provider, and the OpenStreetMap US tileset is served by the provider layer's named vector source with its identifying user agent and usage policy carried in the declaration. The SDK's `photon-provider`, `openstreetmap-us-vector-provider` and `provider-transport` crates are deleted, the second `reqwest` major leaves the tree, and the diagnostics vocabulary (`ProviderRequestObservation`, its redaction, the tracing sink and the provider-layer adapter) now lives in `rustic-maps-core`'s `provider_observation` module. The provider set is entirely on everymap-rs; the registry, cache wrappers and UDL callbacks still speak `geospatial-traits`, which is the remaining phase-5 item.

### Changed (2026-11-05 — north-up holds during navigation)

- The compass is a mode toggle while a guided trip runs: choosing north-up pins the camera at bearing 0 and keeps it there across every following fix, and choosing course-up again resumes the eased course chase. `map-refapp-core`'s `FollowCamera` carries the mode (`CourseUp` default, `NorthUp`), the UniFFI and wasm surfaces expose it, and the compass stays visible while north-up holds so the visitor can leave the mode. A new trip starts course-up.

### Changed (2026-11-05 — the location puck glides between fixes)

- The position disc and its accuracy ring are drawn from an interpolated pose instead of the raw fix: `map-refapp-core` gains a deterministic `LocationPoseAnimator` (motion over the fix interval clamped to a documented band, retarget from the drawn pose when a fix lands mid-flight, shortest-arc course, eased accuracy), the wasm bundle exposes it, and the demo's frame pump draws it. The follow camera keeps its own chase; the puck moves at the trip's own speed rather than in fix-sized hops.

### Added (2026-11-05 — provider-layer requests reach the diagnostics ring)

- MapQuest requests are reported through the SDK's provider-request observation again: the provider layer's own observer is translated into the diagnostics vocabulary (`provider_transport::EveryMapRequestObserver`), so the advanced pane, the typed `ProviderRequestTrace` and the browser event stream keep the provider key, contract, method, redacted path, status class, duration and outcome. A response without a recorded request is dropped rather than reported with invented fields; the response byte count is zero because the provider layer does not report a body.

### Fixed (2026-11-05 — the browser builds provider calls again)

- The SDK depends on everymap-rs 0.3.1, whose browser target reads time through `web-time`; the provider layer had measured time with `std::time::Instant`, which is unimplemented on `wasm32-unknown-unknown`, so every provider call in a browser trapped the module. Raster and vector base maps, MapQuest requests and OSRM/Valhalla routes work in the browser again.
- A registered tile source carries its documented ten-second ceiling into the provider layer's pacing declaration (`TILE_SOURCE_MAXIMUM_WAIT_SECONDS`), which 0.3.1 can express. An over-budget tile request is refused with `RateLimitExhausted` instead of waiting for the next refill.

### Changed (2026-11-04 — MapQuest runs on the provider layer)

- Geocoding, search with type-ahead suggestions, routing, the distance matrix, traffic incidents and flow, and raster tiles are served by everymap-rs's MapQuest providers on the native targets and in the browser; the SDK's `mapquest-provider` crate is removed. One credential still installs every capability, and the contract records, the provider keys, the attribution and the settings surface are unchanged.
- Three conversions are deliberately conservative: a type-ahead candidate without a position is left to the settled search, an incident without a recognisable class or start position is not shown, and an origin-destination pair the service could not measure is omitted rather than reported as zero distance.
- The demo-route recorder is now `cargo run -p rustic-maps-native-core --example record_demo_routes` and records through the same provider adapter the SDK registers; the rendering fixture recorders fetch through the same provider. The wasm clippy closure in CI no longer names the removed provider crates.
- **Known issue, browser only:** provider calls in a WebAssembly build trap the module, because the provider layer reads time through `std::time::Instant`, which is unimplemented on `wasm32-unknown-unknown`. Native builds are unaffected. The fix is prepared upstream and ships in the provider layer's next patch release; until then the browser base maps that register a tile source or call a live provider fail.

### Changed (2026-11-04 — OSRM and Valhalla routing run on the provider layer)

- Routes from OSRM and Valhalla, on the native targets and in the browser, are calculated by everymap-rs's providers; the SDK's `osrm-provider` and `valhalla-provider` crates are removed. A route keeps its legs, maneuver classification, instruction and speech text, lane guidance and geometry, and its bounds are derived from that geometry when the provider reports none. The provider's capability answer decides which transport modes and strategies a settings surface offers.

### Changed (2026-11-04 — URL-template tile sources run on the provider layer)

- A registered tile source's request budget is enforced by the provider layer's token bucket. Its declared capacity and refill rate apply, while the ten-second refusal ceiling does not: the provider layer's pacing declaration has no maximum wait yet, so an over-budget request waits for its refill instead of failing fast. The contract and the settings surface are unchanged; the ceiling returns with the provider layer's next patch release.
- The raster and vector base maps that any `{z}/{x}/{y}` tileset can serve, on the native targets and in the browser, now fetch through everymap-rs's URL-template providers. The SDK's own `url-template-raster-provider` and `url-template-vector-provider` crates are removed; the contract is unchanged, so a registered source keeps its template, name, attribution, zoom span, image format, resolution, pacing and user agent, and the adapter converts the returned tile and any failure at one seam.

### Changed (2026-11-04 — the SDK depends on the published provider layer)

- `rustic-maps-core` depends on everymap-rs 0.3.0 and gains tile adapters that present an everymap raster or vector tile provider through the SDK's own contracts, converting the request echo, format, resolution and wire encoding, and mapping provider failures onto the unified error categories. The native and wasm32 builds compile against the published crate.

### Changed (2026-11-04 — the provider layer ships as everymap-rs 0.3.0)

- everymap-rs 0.3.0 is published to crates.io for all twelve of its crates, tagged `v0.3.0`, covering the transport seam, the wasm target, navigation-grade routing, the typed tile contracts and the MapQuest, OSRM, Valhalla and URL-template providers this SDK consumes. Its publish script now derives the crate list from the workspace graph, so a future crate cannot be dropped from a release. The SDK's dependency on it switches in the next round.

### Changed (2026-11-04 — the type-ahead's empty and failure copy is shared)

- The no-match row and the inline failure sentence now come from the shared `ui_labels` (`plan.no_matches`, `plan.autocomplete_unavailable`) instead of per-shell literals. Android, iOS and React Native use the canonical wording; Android's failure line remains the contract's own detail text and the web keeps its hidden list, both declared as omissions in the parity gate.

### Added (2026-11-04 — provider requests are readable as typed records)

- `LogEvent` gains an optional `provider_request_trace` carrying the serving provider key and contract, the HTTP method, the redacted request path, the status class, the derived outcome, the attempt duration and the response size. The transport's existing redaction is preserved, the diagnostics ring replays the typed record, and the message line stays for human-readable logs. The change is addition-only; Kotlin, Swift, React Native and the wasm mirror regenerate from the same UDL.

### Added (2026-11-04 — road names keep a share of the frame's label budget)

- A frame now reserves eight of its thirty-two label slots for line-placed names (`RESERVED_LINE_LABEL_SLOTS`), so a dense set of ranked places can no longer spend the whole budget and leave the streets nameless. The reservation is capped by the number of line-placed text candidates the frame offers, so unused slots return to places instead of under-filling the frame. Two unit tests pin the reserved case and the returned-slots case; the committed Bright z14 golden is unchanged.

### Changed (2026-11-04 — six owner rulings unblock the queued decisions)

- The provider layer moves to everymap-rs 0.3.0: its transport and navigation PRs are approved to merge, the upstream release build-out completes, and this SDK then consumes it directly.
- The label budget keeps a fixed number of slots for line-placed road names, and route shields become a dedicated overlay kind drawing shaped badges from `routes.ref`.
- Diagnostics gain the typed `ProviderRequestTrace` on `LogEvent`, the contract `MapView` gains camera and fit members with `MapCamera.interpolate`, and the React Native declaration and ordinal layer are generated from the UDL.

### Changed (2026-11-04 — shipped packages carry only SDK code)

- The reference application's demo scenes loader, demo data and UX theme are out of the published Android AAR and iOS SwiftPM package, living only in the reference-application targets. The build scripts, release assembly and release workflow now assert that the AAR (including its nested `classes.jar`), the XCFramework and the web tarball contain no demo identifier or `reference-scenes` resource, and fail if one appears.


### Added (2026-11-04 — capture-free frame snapshots on the web surface)

- `MapView.snapshot()` returns the last rendered frame's RGBA bytes by replaying its recorded draw calls into an offscreen target and reading them back, with no surface acquire, present or `drawImage` re-present. It shares one encoder with the presenting pass, and a browser case proves repeated renders at one pose are byte-identical and match the golden within tolerance.


### Changed (2026-11-04 — release re-runs skip unchanged platforms)

- The release workflow decides per platform whether to rebuild: an input-hash plan job reuses a prior run's artifact only when the revision, the platform's inputs, the recorded SHA-256 and every companion artifact still match, and a restore job re-verifies the bytes before assembly re-checks them. `scripts/release-plan.mjs` reproduces the decision locally, with 14 node tests and an actionlint-checked workflow; the rule is documented in the packaging guide.


### Changed (2026-11-04 — the WebAssembly contract mirror is generated from the UDL)

- `scripts/generate-web-facade.mjs` now generates the web façade's contract mirror (every mirrored dictionary with its documentation and the enum unions) and the Rust bindings' JavaScript field-name constants from the UDL, with a `--check` mode wired into contract sync and a test that fails on one-line drift. The wasm-only projections and the canvas class surface remain hand-written in a separate file; no runtime behaviour changed.


### Fixed (2026-11-04 — the RN iOS overlay decoder accepts every contract kind)

- The React Native iOS decoder now accepts the contract's `PointPins` overlay kind (ordinal 4) instead of rejecting it, matching the generated TypeScript enum and Android; a binding-surface audit records the finding and eight others with ranked proposals.


### Changed (2026-11-04 — every published screenshot comes from one revision)

- The screenshot set is recaptured uniformly from a single revision on Android, iOS, React Native and the web: the plan sheet, the provider/settings section, the navigation frame at the shared marker, the vector base and the layers panel closed and open, 24 frames in all. The capture log, build prerequisites and iOS attachment export are recorded, and the credential check is clean.


### Changed (2026-11-04 — the React Native plan flow matches the iOS pattern)

- The RN My location chip now shows disabled-with-reason and filled states whose marks follow swaps and clear on edits; focusing a field keeps the shared result list; editing text clears the assigned-row marker and held place; a selected keyless routing provider plans through it; and the live route card drops the destination title to match iOS. The behavior note's RN paragraphs were refreshed, with six new Jest cases (201 tests total).


### Added (2026-11-04 — ferry crossings and the mode/strategy contract)

- A step whose mode is `ferry` now classifies as a ferry maneuver and carries its display and speech instruction, with a documented hand-authored fixture and a provider-crate regression test; a browser reference-flow case plans the same crossing for Fastest and Shortest and asserts identical requests. The directions design note and the published routing chapter document which modes and strategies each provider honours, the fallbacks, and the exact sentences each surface shows.


### Added (2026-11-04 — every provider request reaches the diagnostics ring)

- Every provider client, including explicit-transport constructions for OSRM, Valhalla and Photon, executes through the observed transport, and a bridge emits one structured, redacted observation per attempt (provider, contract, method, path, status class, outcome, duration, bytes) into the existing diagnostics ring and live subscription on native and the web. Tests assert exactly one event per request and that no credential ever appears.


### Removed (2026-11-04 — the unused wasm label descriptor seam)

- The wasm-only label descriptor free helpers, their JS builder, the orphaned field constants, the `/glyphs` fixture route and the allow-list entries are gone; the vector overlay decoder now returns overlays only. The committed glyph fixture stays for the native golden.


### Changed (2026-11-04 — one label pipeline on the web surface)

- The browser text-coverage probes now drive labels through an installed vector source and a committed measurement style instead of a page-supplied glyph range and label list, and the wasm-only `setLabels`, `clearLabels` and `setGlyphRange` members are gone from the canvas, the hand-written types and every call site.


### Added (2026-11-04 — the plan-flow audit and its evidence series)

- `docs/design/PlanFlowAudit.md` compares every step of the plan flow against the iOS reference pattern, with the SDK and shell versions in the header and a verdict per step; nine areas match and six smaller divergences are recorded with proposed fixes. The React Native peek now names the selected demo plan like the other targets, pinned by a Jest case.
- A dated evidence series records the commit, versions, device and test-report paths for Android, iOS, React Native and the web for the same flow.


### Changed (2026-11-04 — the device location source is remembered like the other map state)

- The reference applications now remember whether the device location source was running: web, Android and iOS write `deviceLocationEnabled` to the versioned preferences record, restore the source after a relaunch with center-on-first-fix when permission already holds, and never prompt at launch. A proof case on each target shows the style switch, the traffic overlay and a centered device fix all surviving a relaunch.


### Added (2026-11-04 — POI labels and a style-layer coverage audit)

- A committed OpenMapTiles `poi` fixture, a symbol style that paints POI names with sprite icons, and a headless golden placing six names and three icons over recorded imagery.
- `scripts/check-style-layer-coverage.mjs` audits every committed vector fixture for name/ref-bearing source layers that no style paints: 19 layers found, 14 painted, 5 intentionally unpainted with documented reasons, and stale allow-list entries fail the gate. It runs with the browser suite and in CI. Route shields remain drawn as symbol text pending the owner's decision on a shield font versus a dedicated overlay kind.


### Changed (2026-11-04 — gestures draw from cache and settle exactly)

- While the camera moves, the vector base now freezes its style-zoom bucket, suppresses transitions and skips per-tile label extraction; newly revealed tiles still style once. The first still frame resolves the style and labels at its pose and is byte-identical to a direct frame, proven by a unit test and a browser case with zero differing pixels. The native surface infers the gesture from the camera each draw, the canvas takes a canvas-only gesture flag, and the web demo reports pointer, wheel, double-tap and keyboard motion. Measured on the browser harness, the median frame time while panning fell from 8.0 ms to 1.4–1.8 ms with no frame over budget.


### Changed (2026-11-04 — style sort keys, translate anchors and cross-fade)

- `fill-sort-key` and `circle-sort-key` now order a layer's features stably, `fill-translate-anchor` distinguishes map-anchored (bearing-rotated) from viewport-anchored translation, and fill, line and circle colour/opacity cross-fade over their declared transition durations on the live surfaces, with the untimed path unchanged. A mid-fade golden and unit tests cover the new behaviour.


### Verified (2026-11-04 — the browser and native renderers agree on the regional scene)

- A browser regional golden now sits beside the native Carto golden over the same committed z9 scene and style: the live browser frame is byte-identical to its own golden, and the two renders differ by a mean channel error of 0.28 with a 0.67% outlier fraction, all of it label glyphs. The style-refinement note records the measured road-layer counts, hidden classes and per-class widths.
- The comparison surfaced a real defect rather than a renderer difference: the persistent label history could make the live surfaces alternate between two valid label sets at regional zoom because a remembered label overrode a higher-ranked competitor. Label placement now orders every candidate by class, rank and style tier, with the history only breaking exact ties, so identical frames place the same labels; the regional browser golden was repinned on the deterministic set and the cross-renderer error halved.


### Changed (2026-11-04 — labels collide per glyph and rank by class)

- Collision detection now uses every drawn glyph's own padded box, measured from the glyph atlas' bearings and advances and oriented for line labels, falling back to one padded block box only when a block carries no bitmaps.
- Placement orders symbols by class first (shields before place names), then feature rank, then the style layer's zoom tier; regional thinning drops lower-ranked labels above each frame-zoom band before ordering, with unranked road names and shields exempt. Dense regional goldens at zoom 13 and zoom 8 pin the thinning, the affected style goldens were regenerated and reviewed, the Bright typography audit is documented, and an iOS capture records the device rendering.


### Added (2026-11-04 — the layers control pictured on every platform)

- The *Map layers and traffic* chapter now shows the layers panel closed and open on Android, iOS, React Native and the web, captured from one revision with the session credential and a shared camera pose: the open frames show the base maps, traffic, the four overlay scenes, Clear overlays and device location.
- React Native gained the shared Clear overlays action its panel was missing, with a Jest render/act test.


### Verified (2026-11-04 — a remembered routing choice survives a relaunch)

- The browser records per-capability provider selections with the other preferences, and a case proves the keyless routing choice survives a reload and plans the recorded route from the same-origin fixture; Android recreates the activity and re-plans through its fixture provider; iOS relaunches and re-plans against the committed OSRM payload through its loopback server. A keyless iOS session now plans through a registered keyless routing provider, matching Android, and an automation-only routing deployment override was added.


### Added (2026-11-04 — icon symbols from a sprite atlas)

- Vector styles can draw `icon-image` symbols: the `sprite` document and sheet load through an injected source (committed fixtures in tests, no network), `icon-size`, `icon-anchor`, `icon-offset`, `icon-rotate`, `icon-rotation-alignment`, `icon-padding`, `icon-allow-overlap` and `icon-ignore-placement` compile with the specification's defaults, and icons draw through the same textured pipeline as tiles in the headless, native and browser renderers. Text and icon collision boxes are separate, and a missing sprite, index or icon draws nothing instead of failing.
- A committed style, sprite index and sheet with four in-repo geometric icons (no third-party assets) back an end-to-end golden over the recorded Sacramento imagery.


### Added (2026-11-04 — a keyed four-frame screenshot set on every platform)

- The published set is now four frames per platform, captured with a session-injected MapQuest credential from one revision and synchronized to the same marker: the plan sheet with Capitol Park and Tower Bridge, the provider section with every visible capability row `MapQuest · Ready`, the navigation frame with the first instruction, polyline, ETA and speed at 1×, and a vector-base frame showing the OpenFreeMap Bright style with its own drawn labels.
- The vector section replaces the obsolete browser-only `vector-base-with-place-labels` capture, and each vector capture asserts the styled base is actually serving (SDK vector status, a `Serving` row, or the web tile mode and provider key).
- Android captures run through the instrumented screenshot test, iOS through an XCUITest with result-bundle export, React Native through a new runner, and the web through its Playwright capture spec; `docs/ScreenshotCapture.md` records every command and marker, and `screenshots.md`, the style guide, `CLAUDE.md` and the flow gate's frame list are updated together. The credential is injected for the session only and an in-process grep over every published image and text file finds no trace of it.


### Added (2026-11-04 — layers, provider mixing and the source archive documented)

- A new *Map layers and traffic* chapter documents the floating layers control (base maps, vector styles, traffic, demo overlays, Clear overlays, device location), the attribution control with its terms panel and live-region announcement, per-platform placement, and the boundary with the settings surface; screenshot placeholders wait on the published keyed capture set.
- The *Mix providers per capability* page now covers the capability readback and its order, the keyed and keyless paths with each provider's default address, the public demo budgets, and the exact failure and next-step sentences.
- The Rust page gains an install-from-source-archive note (contents, what Cargo builds, what is deliberately excluded), linked from the downloads page, and the reference-app guide's component inventory gains the layers control.


### Verified (2026-11-04 — the raster tile path is not the frame cost)

- The instanced-tile cleanup was measured before implementation and closed without code: the raster tile stage is at the timer floor (renderFrame p50 0.1–0.2 ms, p95 0.3 ms; wasm tile pipeline p95 0.1 ms; no frame over budget in 120 at 1920×1080, unchanged at 2560×1440 with 24 fragments). Removing the per-fragment bind-group sets would save roughly 0.02–0.06 ms, below the harness resolution, against a texture-array cache in three GPU paths and a shader/vertex-layout rewrite. The vector layer is where the frame time goes (renderFrame p50 8.2 ms, GPU pass 0.52 ms).


### Added (2026-11-04 — one member reloads the raster tile source)

- `MapView.reloadTileSource()` / `reload_tile_source()` is the one way to make the map re-resolve its raster provider after a credential or source change: it replaces the tile scheduler (fresh pool and warm registry), drops in-flight warm fetches and any retained zoom level, keeps the camera, overlays, location indicator and vector base, and leaves the view unsettled so the pump redraws. A call during a frame applies when that frame restores; a detached view is a no-op; an attached view with no provider throws.
- The member is additive across the contract, Kotlin, Swift, the React Native binding and the wasm canvas, and the Android, iOS and React Native wrappers and reference apps now call it instead of detaching and re-attaching the surface. Native unit tests, a browser case that proves a changed same-origin source serves new bytes after the reload, and the regenerated bindings ship together.


### Changed (2026-11-03 — the location indicator becomes a two-mode puck)

- The position marker is now a drawn puck. In tracking mode it is a soft dot: a dark halo, a white ring and the accent core. In navigation mode it is a white-outlined up-course dart with the same halo and ring, rotated by the course relative to the camera and degrading to the dot when the course is unknown. The accuracy ring, screen anchoring, world-copy behaviour and top-layer draw order are unchanged.
- The style record gains nullable ring and halo colours as an additive change across the contract, the native mirror, the wasm validation and the React Native decoders; the accent colour and radius remain overridable, and every shell already switches visual kind from its trip state.
- Goldens cover tracking, tracking with a heading (byte-identical to plain tracking) and navigation; device captures on Android, iOS and the web show both forms. The React Native device capture remains blocked by the recorded Xcode/React-Core build residual.


### Added (2026-11-03 — type-ahead on Android, iOS and React Native)

- All four targets now suggest places while the visitor types, under the token-driven rule: three characters, a 250 ms quiet window, at most five rows, one cancellable request per field, dropped superseded answers, and a chosen row that fills the field, closes the list and holds the place for Plan route.
- Android renders suggestions in both plan fields from a per-field controller with a cancellable coroutine and generation guard; iOS adds `SearchTypeAheadController` with a cancelled task, the same guard and a loopback fixture server for its UI tests; React Native adds `planSearchAutocomplete.ts` with an abortable request per query. Each shell chooses the provider from the SDK's capability readback and the credential state, installing the registry's keyless search descriptor only when nothing serves the slot.
- MapQuest's autocomplete request now asks for the same five candidates the shells render, and `docs/ReferenceAppUX.md` §5.5 documents the behavior.

### Added (2026-11-03 — providers declare their tile pacing)

- Both URL-template source records carry optional `token_bucket_capacity_tokens` and `refill_tokens_per_second` fields, so a provider with a stricter published budget lowers its bucket from its own declaration. `provider_transport::TileSourcePacing` validates the values (refusing non-finite, zero or negative numbers with an error naming the source) and carries the 8/8 defaults; both provider crates keep their existing constructors and add pacing-aware ones.
- The fields are nullable with null defaults, so generated Kotlin and Swift constructors stay source-compatible; the native registration, the wasm façade and the React Native surface pass them through. Tests prove a declared bucket throttles where the default admits, that an unusable declaration is refused, and that the browser pass-through works.


### Verified (2026-11-03 — one reference-app surface on every target)

- The React Native settings sheet's per-capability provider rows are pinned to the shared registry: a credentialed descriptor in a keyless session renders `MapQuest · Needs API key`, a keyless descriptor renders `Ready`, and the text composes from the descriptor's display name and the SDK's credential state.
- Android now asserts the converged surface from the composed semantics on the standing Pixel 9: the settings sheet carries provider credentials and capabilities with no base-map, vector-style or overlay-scene section; the floating layers control owns the base-map rows, the scene switches and Clear overlays; zoom, my-location, 2D/3D, layers and attribution are all 40 dp; and the drawer lists no demo-plan or demo-scene picker. The connected suite ran 43 tests with 0 failures (10 skipped), and the opt-in capture produced the plan sheet, provider section and navigation frames.
- The React Native suite (183 tests), the Android AAR build, Kotlin compilation for both source sets, 57 JVM tests, and the parity, provider-name, platform-parity, contract-sync and snippet gates are green.


### Added (2026-11-03 — type-ahead with one shared rule)

- Three behavior tokens join the reference-app artifact: `autocomplete-minimum-query-characters` (3), `autocomplete-debounce-milliseconds` (250) and `autocomplete-maximum-results` (5), mirrored on Android, iOS, React Native and the web and enforced by the parity gate. `docs/design/Autocomplete.md` states the rule every target follows, including the keyless/keyed provider choice and the empty and error states.
- The web demo implements the rule on both trip fields: the timer restarts on every keystroke, a query shorter than three characters never reaches a provider, a superseded answer is dropped, at most five rows render, and choosing a row fills the field and closes the list. The SDK's capability readback decides whether the registered provider or the demo places answer.
- Three browser cases pin the threshold, the restarted debounce (exactly one provider request for a typed query) and selection; the provider is the registry's keyless place provider aimed at the same-origin fixtures.


### Added (2026-11-03 — a keyless Photon search provider)

- `photon-provider` serves place search, autocomplete and forward/reverse geocoding against any Photon deployment, over the shared transport seam with bounded retries; autocomplete asks for five candidates, a settled search for ten, and a search boundary becomes Photon's `bbox` filter. Display lines come from the feature's name or its composed address components, and Photon's feature classes map onto the SDK's geocode-quality scale.
- The provider registry declares Photon keyless for geocoding and place search, with its attribution, terms, public-instance note and same-origin fixture routes; the native provider catalog and the browser dispatch register it for both capabilities, so the reference applications' settings surfaces offer it without code changes and without a vendor name in any shell.
- Committed GeoJSON fixtures pin the search and reverse responses, and a browser case registers the provider against the fixture server and asserts the mapped rows, including an empty-query refusal before any request.


### Changed (2026-11-03 — zoom changes cross-fade through the retired tile level)

- Crossing a tile zoom level no longer swaps the whole tile set in one frame. A `ZoomTransition` keeps the retired level's pooled tiles on screen — projected with the current camera, so they scale into place — and dissolves them into the new level once at least 85% of its visible tiles are present, or after a one-second cap. The retired level draws from the pool alone and never fetches.
- The tile vertex format carries a per-quad opacity and all three tile pipelines blend with standard alpha, so the retired level draws over the new one as an exact dissolve. The native surface, the browser canvas and the headless capture path share the retained-level draw; a running fade keeps the native frame unsettled and the browser pump on the animation cadence, and canvas frame statistics report `zoomTransitionRunning` and `retainedTiles`.
- A committed golden captures a half-faded z15-over-z14 frame, and a browser case asserts the hold, the dissolve and a zoom-out that never clears to the background.

### Changed (2026-11-03 — tile fetches run through a bounded parallel pool)

- An uncached viewport now fills through a pool of up to eight provider fetches instead of one at a time: the tile scheduler's background warm task keeps that many in flight and refills as each completes, and the blocking frame path collects its budgeted misses through the same pool. `TILE_FETCH_CONCURRENCY` names the bound; warming marks, delivery order, cancellation and the per-frame budget keep their semantics.
- The URL-template raster and vector providers admit an 8-token burst and refill at 8 requests per second, four times the previous 4-token / 2-per-second bucket, so a normal viewport's tiles are admitted together rather than queuing behind pacing.
- The browser canvas already starts its network fetches detached (up to 32 in flight) and awaits only the memory move of staged payloads, so it keeps that structure; its measured gain comes from pacing and the pool. The same-origin fixture probe stays as the network-free test seam.
- The frame-latency suite asserts the pooled window and the wave floor instead of the serial floor, and a barrier provider proves the warm pass fills the window.

### Changed (2026-11-03 — one map surface across the reference apps)

- Base map, style and overlay-scene selection now lives only in the floating layers control on every target; the settings sheets keep credentials, per-capability providers, offline maps, diagnostics and theme.
- The demo-plan and demo-scene pickers are gone from the drawers, returning that space to search, planning and navigation. The shared demo data, remembered choices and launch automation stay: iOS selects a recorded plan with `-DemoPlanScenario` (and keeps its §5.1 fit-contract UI test), the web exposes `window.selectDemoPlanScenario`, and the layers panel owns the scene switches, the readiness reason and the clear action.
- Every circular map control — zoom, my-location, 2D/3D, layers and attribution — is 40 units on every target, from one shared `map-control-diameter` token that `scripts/check_ux_parity.mjs` enforces.
- The iOS drawer sits on the bottom safe area; the extra 16 pt gutter band below it is gone, and the attribution glyph grows to the shared control size.

### Changed (2026-11-03 — labels hold their place across frames)

- The label pass carries a persistent collision index: a name placed by the previous frame is tried before every newcomer, so panning half a street name off the edge no longer lets a district name take its space for one frame and lose it the next. A name that leaves the candidate set is forgotten, and the index is dropped when the vector base is installed or cleared. The native surfaces carry it in their draw session and the browser canvas in the view; a page's own `setLabels` list, which every call replaces wholesale, still draws without it.

### Added (2026-11-03 — the traffic cache's counters cross the contract)

- `MapSdk.traffic_flow_cache_statistics()` returns the serving traffic-flow cache's hits, misses, held cells, evictions and held segments beside the grid zoom, entry budget and time-to-live it enforces, or `null` when no traffic-flow provider is registered or the provider owns its own freshness policy and answers uncached. The registry now keeps the cache handle beside the provider it wraps, so a diagnostics panel or developer console can show what the cache did; the reference app's layer panel publishes the counters after each settled refresh.

### Verified (2026-11-03 — the base-layer switch on every suite)

- The raster → vector → raster and vector → vector sequences are asserted on all four surfaces. The browser case reads the canvas' vector-source status in the same turn as a style change (the new style with zero drawn tiles) and proves the next frame never carries the previous style's geometry; the Android and iOS suites drive the same sequence through their own layer controls on the standing devices; the React Native suite drives the settings rows and asserts the stored base map and the moving Serving mark.

### Verified (2026-11-03 — the offline pack scenario on both standing devices)

- The Android Pixel 9 and the iOS iPhone 17 Pro record the same offline pack scenario from a credential injected for the session only: the cache registers, the demo region downloads with live progress, a cancelled download keeps a partial pack that a later call resumes to every tile, a relaunch lists the kept pack from the store, and Delete removes it. The Android run also force-quits with airplane mode on and renders the stored region offline; the simulator shares its host network, so its XCUITest asserts the resumed download serving the kept tiles from disk in place of cutting the network. Evidence is under `tmp/offline-pack-proof/`; the iOS case is `testTheOfflineRegionDownloadSurvivesARelaunchAndDeletesItsPack`, key-gated like the other live-data UI cases.

### Changed (2026-11-03 — the shared reference-app math ships once)

- The browser reference app now runs the same `map-refapp-core` math as the native apps: `rustic-maps-web-bindings` exports the follow camera, course median, smoothstep and exponential easing, scale-bar ladder and trace cursor, and `map_controls.mjs` and `playground.mjs` read them instead of mirrored JavaScript. The contract-sync gate knows the new wasm-only helpers, and a browser case drives the classes and functions inside the page.
- The native reference apps consume `map-refapp-native`, a refapp-only UniFFI binding over the same crate, generated by `scripts/build-refapp-core.sh`. Android takes the whole surface; iOS takes the scale ladder and circular course median and keeps its own follow-camera zoom policy, which deliberately differs. The AAR and XCFramework builds assert the refapp-only crate never enters a published package.
- The web Map layers panel names the vector provider on every style row (`OpenFreeMap · Bright`), matching the settings pickers and the native base rows, and the browser plan fields carry the A/B endpoint markers.
- The redundant inline "assigns" badge is gone from the Android and iOS plan fields; the assignment sentence above the Plan route action states the target once.
- React Native renders the shared `no '{provider}' provider is configured` sentence when a capability read finds no provider instead of the binding's representation, and states the reason once rather than under both Directions groups.
- The Android, iOS and web build scripts run the compiled `uniffi-bindgen` directly; on some hosts `cargo run` stalls at process start and the build waited indefinitely.
- The published screenshot set is three frames per platform from one revision — the plan sheet with both endpoints, the provider section with composed rows, and the simulated trip — captured at phone width. `site/book-src/screenshots.md`, `docs/DocsStyleGuide.md` §5 and `CLAUDE.md` record the set, and the Android and web capture seams are committed and opt-in.

### Changed (2026-11-03 — one reference-application behavior model)

- The four reference applications now follow one written behavior model: `docs/design/ReferenceApplicationBehavior.md`, the machine-readable `platform/reference-flows.json` (four named flows and 24 steps) and `scripts/check-reference-flows.mjs`, which fails when a step, copy ID, stable hook, screenshot step, registry endpoint or build input goes missing. The pure `map-refapp-core` crate holds the shared follow-camera, course-median, easing, scale-bar and trace-cursor math plus the total `advance_flow` reducer, with no SDK handle, clock or I/O.
- Android reached the iOS search/plan/drive flow: visible Origin/Destination labels with A/B markers, per-field Search, a My location chip for each field, a swap action, typed-text resolution on Plan route (assigned place → exact demo place → provider result), the route card with distance, duration and ETA, the stated trip source, and the top-center turn banner with speed controls. The same field, provider and diagnostics changes landed on iOS, React Native and the web demo.
- Diagnostics are off by default on every target: no diagnostics subscription and no ad-hoc rendering console line until the visitor opens the pane, then one bounded stream (100 events, 100-event backlog, 500 ms coalescing, Clear) at Info with an explicit Show debug events switch for Debug, per session.
- Provider surfaces are descriptor-driven everywhere: rows and picker entries read `{display_name} · {credential state}` (for example `MapQuest · Needs API key`), and keyless OSRM and Valhalla pass the registry's public demo endpoints (`https://router.project-osrm.org`, `https://valhalla1.openstreetmap.de`) as their deployment address. An unreachable server reports its verbatim failure with a retry/own-deployment next step and never falls back silently.
- The WebAssembly surface now compiles OSRM and Valhalla routing providers, so the browser demo can switch providers like the native apps. The diagnostics subscriber keeps every tracing callsite interested so a later Debug opt-in can see events first hit at Info, while the per-event gate still keeps the render path at near-zero logging cost.

### Changed (2026-11-03 — the tile-cache registration is provider-keyed)

- The contract's tile-cache registration is now `set_provider_credential_with_tile_cache(provider_key, credential, tile_cache_database_path, tile_cache_absolute_maximum_bytes)`. The key selects a credentialed provider built into the SDK that serves raster tiles and declares caching terms; the registration installs every capability that provider serves and wraps its raster-tile provider in the disk cache at the provider's declared ceiling. Unknown keys, providers with no raster-tile service or no declared terms, and blank credentials are refused with typed errors. The previous MapQuest-named parameter and the vendor wording in the UDL are gone; Kotlin, Swift, React Native and the reference applications call the generic member.
- A traffic-flow implementation registered from application code is served as-is: the SDK's grid cache is its own freshness policy for the providers it installs, so a host-provided implementation keeps control of freshness and failure reporting. The catalog-installed provider path keeps the cache. This restores the reference-app behaviour where a camera-pose refresh that fails keeps the previous overlay and reports the failure.

### Changed (2026-11-03 — the wasm module generates the contract without the native surface)

- The WebAssembly scaffolding is generated from the frozen contract with exactly one interface removed: the native surface `MapView`, which the browser serves through the hand-written canvas class. Everything else — `MapSdk`, `MapCamera`, `Navigator`, the provider interfaces and every record — is still generated and compiled for wasm. The refusing stub file is deleted, and a test pins that the split removes exactly that interface and keeps the rest.

### Added (2026-11-03 — offline tile packs end to end)

- `docs/design/TilePackManager.md` specifies the offline pack feature: the manager built from the installed tile cache, the addition-only `OfflineManager` contract surface, progress/cancellation/resume semantics, platform exposure, the reference-app flow and the device proof. `PLAN.md` carries it as M63 with ordered checkboxes and eight owner questions.
- `TilePackManager` now takes a blocking-work executor, resumes a partial pack after the prepare future is dropped, and reports an incomplete pack as a typed failure while keeping the tiles that did arrive. The catalog carries each provider's raster maximum zoom, and the installed-cache snapshot builds the manager with that ceiling and the provider's declared terms.
- The contract adds the region records (`RegionDefinition`, `RegionRecord`, `RegionDownloadProgress`, `RegionDownloadPhase`, `RegionRemovalReport`), the `RegionDownloadProgressListener` foreign trait and the `OfflineManager` members `region_tile_count`, `download_region`, `cancel_region_download`, `region_records`, `region_record` and `delete_region`. A same-region download is refused, cancellation returns the partial record as a successful `Cancelled` result, replacing the tile-cache installation cancels in-flight downloads, and the WebAssembly surface refuses the disk pack store honestly.
- The Android, iOS and React Native reference applications gain the offline-region card: the demo region's tile count, download with live completed/total and network-versus-disk progress, cancel and resume, the stored regions with tile and byte counts, delete, and the SDK's `TileCache` refusal when no cache is installed. The shared artifact gains the `downtown-sacramento` demo region and the ten offline-card labels; the published offline-maps chapter documents the native flow and states the browser limitation.

### Fixed (2026-11-03 — runtime, cache and rendering hardening)

- A cancelled `draw` no longer strands the map surface: the frame's ownership guard lives inside the spawned draw work and restores the surface and vector-base slots when the work finishes, when it panics, and when the caller drops the returned future. A racing `detach` still wins and leaves the slot honestly detached. Drop-the-future and panic-unwind tests cover both paths.
- Blocking work left the async reactor: `rustic-maps-core` exposes a blocking-work executor seam that native builds back with `spawn_blocking` and the browser runs inline. Cache lookups and writes, the offline graph search and tile-pack persistence go through it; the offline graph is cloned out of its read lock before the search rather than held across it, and pack freshness writes batch at a bounded chunk size.
- SQLite eviction computed its deficit once and now deletes the least-recently-used prefix in one statement, skipping the pass entirely while the store is under budget. Pack-pinned rows and the oversize-entry tolerance keep their previous semantics.
- `set_provider_credential` trims its input, refuses a blank or whitespace-only credential with a typed error, and installs the provider through the shared catalog (one client across every capability) instead of duplicating the construction wiring. The tile-pack zoom ceiling is declared by the provider crate and passed into the cache as configuration; the cache crate no longer names a provider.
- Expression `get` borrows the feature's property map instead of cloning the whole map per lookup, and clones only the addressed value.
- Overlay payloads are shared per frame instead of deep-cloned twice: the overlay store hands the frame one `Arc` snapshot, the settle check compares a mutation revision, and scaling happens during emission. Label glyph lookup binary-searches the packed atlas groups.
- The WebAssembly contract mirror is checked field by field: `contract_sync` now parses UDL dictionaries and enums, compares every dictionary the TypeScript mirror declares (field set, field type and `null` versus optional), and reports a nullable-sequence parser gap that had hidden drift. The mirror's real `null`/optional mismatches are fixed, and a package-manifest case asserts the packed tarball carries the curated and generated declarations. The wasm `MapView` placeholder is documented as the honest surface refusal and covered by behavioural parity tests, because the generated scaffolding cannot drop it without a contract split.
- The React Native package builds its tile-cache path with `java.io.File` (available from API 1), so Android 24 and 25 no longer crash on `java.nio.file.Paths`; a source gate enforces the floor and the package gate asserts the packed runtime and Java 8 compile options.
- Both TurboModule build fixes (the Android CMake `libandroid` link and the iOS `ReactCodegen` include) now run inside `scripts/generate-rn-bindings.sh`, and `verify-rn-package.mjs` fails when either marker is missing. Autolinking resolves one hand-written package exposing the ubrn module, `MapsSdkNativeModule` and `MapsSdkMapViewManager`; the app-shell manual registrations are gone and a tarball-install smoke exercises the packaged `react-native.config.js`. `uniffi-bindgen-react-native` is a production dependency, and the Android CMake fails fast with a remedy when the generated bindings are absent.
- `setTileCacheTimeToLive` validates its input in TypeScript and in both native runtimes: a finite, positive, whole second count within the provider's declared ceiling is accepted; anything else fails with a stable typed error instead of trapping on iOS or wrapping on Android.
- Android now delivers `onFrameStatistics` with the same nested `statistics` payload as iOS, so the JavaScript `onFrameStatistics` callback fires on both platforms; a parity test checks the six field names in both emitters.
- The React Native package root exports the curated `MapView`, `MapsSdk` and generated namespace from the generated entry (applied by the generator and marker-asserted by the package gate), and a Jest smoke imports the real root with the native installer mocked.
- The Android `MapsSdkFacade` takes the shared registry's provider descriptors and registers every capability a descriptor declares through the generic `configureProvider` call; the hardcoded `mapquest` dispatch and vendor state are gone. The provider-name guard now scans `maps-sdk/src`.
- iOS lifecycle: the map view observes background, foreground and memory-warning notifications, releases the GPU surface on background and memory pressure, re-attaches on foreground, and `setRenderingPaused(true)` releases the surface. Background location is enabled only when the app declares the `location` background mode and the visitor grants Always authorization.
- iOS Core Location rejects fixes with negative horizontal accuracy or an age beyond the documented staleness bound, selects accuracy per consumer mode (idle versus navigation) and treats `.locationUnknown` as transient instead of a source failure.
- The iOS draw delegate compares the new statistics and camera readback before publishing, throttles changed frames to a named cadence and skips the vector-source readback when the tile counts did not change.
- iOS diagnostics arm on the first sheet open, stage deliveries without creating a task per event, drain one batch per merge tick, and invalidate the timer and stop accepting deliveries when the sheet closes.

### Fixed (2026-11-03 — React Native, iOS packaging and browser lifecycle hardening)

- The React Native pod's map widget is now generated from the SwiftPM wrapper by `scripts/sync-ios-pod-view.mjs`; the script is a gate in the Playwright global setup and the React Native build, so one copy can no longer drift from the other. The sync closed the pod's missing `baseMap` failure phase and the latent Swift errors around its observer protocol, view-identifier registration, `fitBounds` call and boxed ground-meters readback. `scripts/check-rn-pod-swift.sh` type-checks the pod's Swift sources against the generated bindings and the vendored FFI module, because the pod target itself still cannot compile on Xcode 26 (the `React-Core-prebuilt` explicit-module scan fails first).
- The React Native Android module tears down through `invalidate()` (releasing the event sink, the lifecycle listener, the coroutine scope and the device-location source) instead of the removed `onCatalystInstanceDestroy()`. A malformed JavaScript payload no longer throws on the main looper and kills the process: a host-view command failure is caught and reaches the view's `error` event with its phase.
- The React Native iOS runtime applies the launch-environment credential once per process (retrying until it succeeds) instead of re-registering it and reinstalling the tile cache on every `initialize()`.
- The iOS SDK ships a `PrivacyInfo.xcprivacy` declaring the disk-space required-reason API (`NSPrivacyAccessedAPICategoryDiskSpace`, `E174.1`) used by the tile cache's eviction; it travels in the `RusticMapsSDK` resource bundle and in the published XCFramework directory. The reference application carries its own manifest for its `UserDefaults` record (`CA92.1`).
- The Android AAR's consumer R8 rules keep the JNA dispatch and the generated `uniffi` bindings, and a clean application outside the repository depends on the one AAR, builds with R8 minification on, and renders live MapQuest tiles on the Pixel_9 emulator (`tmp/m61-external-app/`).
- The browser page follows its layout box with a `ResizeObserver`, re-sizes the backing store when the display density changes, keeps a hidden tab from drawing (the queued frame draws when it becomes visible) and frees the WebGPU canvas on `pagehide` after any frame in flight; two browser cases cover resize/release and the hidden-tab queue.
- `stage_tile_parent_fallback` borrows the draw state only outside its parent fetch, so the fetch no longer holds a `RefCell` across an await; a busy state is a coded refusal like the re-entrant frame.

### Changed (2026-11-02 — the site and demo republished from the current tree)

- The documentation site, API references, downloads and live demo were republished from `e2546a5` (published as `gh-pages` commit `ad13581a`) with a full locally assembled artifact set: one Android AAR, an iOS XCFramework zip, one WebAssembly npm tarball, the React Native package, the Rust source archive, notices and checksums. The demo carries the current wasm build, including the 1.5× retina threshold.

### Fixed (2026-11-02 — platform hardening, first batch from the five surface audits)

- The token bucket reserves tokens as a signed balance, so concurrent waiters wake at distinct instants instead of under-pacing; a NaN or infinite capacity, refill or timestamp is refused at construction and in the signed schedule.
- A cache *read* failure degrades to a miss like the vector path instead of failing the provider call; the tile-pack reader reports a corrupted zoom range, TTL or format as `RecordCorrupted` instead of clamping it; `tile_pack` validation rejects a minimum zoom above its maximum.
- An expression whose arithmetic produces NaN or infinity evaluates to `Null` rather than leaking a non-finite value into a style; tests cover each operator family.
- The decode failure map and the empty-font fallback are bounded and panic-free, a lost surface returns `SurfaceConfigurationFailed` instead of panicking, an empty label font builds without an error path, and a tile fetch failure is traced instead of swallowed.
- `ReqwestTransport::default()` keeps the transport's connect and request timeouts, so a fallback client can never inherit a request that hangs forever.

### Fixed (2026-11-02 — the hosted Android demo draws on Appetize again)

- The Appetize Android build was three days and 311 commits behind: it predates `a7c6185`, which relaxed the device request from `wgpu::Limits::downlevel_defaults()` to `downlevel_webgl2_defaults()`. Appetize's emulator exposes a software GLES adapter that reports zero compute-workgroup limits, so the old build refused the device and showed `RenderingSurfaceUnavailable` with no tiles. Reproduced on the Pixel_9 emulator with the old limits and fixed by refreshing the hosted build in place from the current tree (build `5a2b37wuwdn4oehc5yfatdkxoa`, version code 6, source commit `620f18e`); the refreshed APK installed on the Pixel_9 shows live MapQuest tiles and no surface error.
- After the one-AAR consolidation the React Native host still declared both the retired `map-sdk-native` AAR and `maps-sdk`, so its release build failed on duplicate `uniffi.rustic_maps` classes and `librustic_maps.so`; it now consumes the single AAR. The hosted React Native Android build is refreshed in place (build `usxc23e5mgukljuzapocwrycki`, version code 2, source commit `ea6ea6c`) and renders live tiles on the Pixel_9 emulator. The hosted iOS simulator build is refreshed in place as well (version code 6, source commit `4217bc7`) and draws tiles on the standing iPhone 17 Pro simulator.
- `WGPU_BACKEND` now actually selects the instance's backends: wgpu 30's `InstanceDescriptor::new_without_display_handle()` ignores the variable, so the `gl` pin both Android shells set was inert and the emulator kept its Vulkan driver mapped. `SurfaceRenderTarget::from_unsafe_target` sets `wgpu::Backends::from_env()`, verified on the emulator (the Vulkan driver no longer maps; tiles render over GLES).

### Fixed (2026-11-02 — browser surface hardening)

- The WebGPU canvas installs a device-lost callback and an uncaptured-error handler: both record an error in the diagnostics ring and a later frame reports the coded `RenderingSurfaceUnavailable` error, which the page turns into its documented 2D degradation instead of a frozen map. `initialize()` claims its device only on success, so a failed first attempt can be retried.
- A rejected `renderFrame` is caught by the page: it names the failure in the error report, degrades to the 2D tile grid and keeps the render loop alive; a browser case forces a rejecting frame and asserts the error text, the degraded status and that the loop still paints.
- A browser `fetch` now carries a 30-second deadline enforced with an `AbortController`, so a stalled connection cannot pin an in-flight request slot; a browser case stalls a provider route and asserts the `request timed out` failure.
- The surface extent is clamped to the adapter's maximum texture dimension (an 8K backing store is presented scaled instead of failing every configure), with a unit test; a frame re-entered while another holds the canvas state is refused with a coded error rather than a `RefCell` panic.
- The demo's tile-resolution threshold counted only a full 2× backing store, so a 2× laptop at reduced browser zoom (about 1.6) fell back to standard tiles and looked soft; the threshold is 1.5 now, and a browser case at device scale 1.6 proves the page requests the `@2x` fixtures.
- Tile fetches distinguish a definitive "no such tile" answer from a transient failure: a 404/410 (or an unserved address with no live source) marks the tile unavailable, while a transport error is retried after a 2-4-8-16-30 second back-off. The unavailable set, staged payload set and retry schedules are bounded. A browser case injects a 500 and asserts the same address is requested again after the back-off; a gzip-bomb case bounds the decompressed vector tile at 16 MiB.
- `clearProviderRegistrations()` also releases the traffic-flow cache and the live-raster zoom ceiling it owned, with a browser case.
- The Playwright suite now imports one guarded `test` fixture: any request that leaves the harness origin aborts unless the test intercepts it itself, so a missing fixture fails loudly instead of reaching a service. In CI, a preflight launches the same Chromium and fails the job when no WebGPU adapter exists instead of letting the GPU cases skip. A shared `waitForMapTestReady` helper waits for the page module to install its readiness promise before awaiting it, so a slow module load under CPU pressure cannot race a test's first assertion, and the suite runs one worker: parallel browser processes sharing one GPU made adapter creation intermittently exceed the readiness waits (the same file passes 15/15 serially and failed under three concurrent workers).
- The WebAssembly package records a fingerprint of the sources it was emitted from; the Playwright setup and `npm pack` refuse a stale or misversioned package, the module has an 8 MiB budget, and the release job installs binaryen and builds with `--require-optimize`. The curated `rustic_maps.d.ts` contract record declarations now ship in the tarball under the `./rustic_maps` type subpath. `trafficFlowCacheStatistics()` returns JavaScript `null` (not `undefined`) while no traffic provider is registered, matching its documentation.

### Added (2026-11-02 — the MapLibre demotiles style renders end to end)

- Sixteen z2 vector tiles recorded from `demotiles.maplibre.org` are committed as fixtures (384 KB) with their OpenStreetMap/ODbL and Natural Earth attribution, and a headless golden renders the published MapLibre demotiles style over them: country fills, dashed geolines with line-placed labels, country names and the style's water background, with the label pass asserted non-empty. It is the end-to-end proof that a style published for MapLibre renders recognisably here.

### Added (2026-11-02 — variable label anchors, radial offsets and line max-angle)

- `text-variable-anchor` gives a point-placed label an ordered list of anchors; the frame keeps the first one that does not collide (one label, not one per anchor), so a name can move above, below or beside its symbol to find room. `text-radial-offset` pushes such a label outward from the symbol by the given ems.
- `text-max-angle` sets how sharply a line label's straight run may bend (degrees); a layer that omits it keeps the renderer's documented 20-degree default, so existing styles and goldens are unchanged.
- Unit tests pin the first-fitting anchor fallback, the radial displacement, the wider-angle longer run, and the resolved style values.

### Added (2026-11-02 — text anchors and rotation alignment for labels)

- `text-anchor` places the text block against its symbol anchor in all nine positions (`center`, four edges, four corners); a line-placed name keeps its run placement.
- `text-rotation-alignment` lets a point-placed label follow the map's bearing (`map`) or stay horizontal (`viewport`, and the `auto` default for point placement); a line-placed name keeps its segment rotation unless the style asks for `viewport`. `text-keep-upright` flips a label past a quarter turn, the specification's default.
- Unit tests pin the anchor offset, the map-aligned point rotation, the viewport alignment and the upright flip; defaults reproduce the previous centred, screen-facing labels, so every label golden and the browser vector-style goldens are unchanged.

### Added (2026-11-02 — wrapped label text with line height, letter spacing and justification)

- Symbol layers now wrap point-placed labels at `text-max-width` (in ems) on word boundaries; a line-placed name stays on one line, the specification's rule. The text block reports its widest line and its total height, and the collision box uses both.
- `text-line-height` (default 1.2 em) stacks the lines around the anchor, `text-letter-spacing` (default 0) widens each line's advance, and `text-justify` aligns lines within the block — `auto`/`center` center the short lines, `left`/`right` align every line to the block edge. Rotated line-placed names apply the same layout in their own frame.
- Existing single-line labels render identically: the defaults reproduce the previous centering, so every label golden and the browser vector-style goldens are unchanged.

### Changed (2026-11-02 — one Android artifact and one WebAssembly package)

- The Android release is one AAR: `:maps-sdk` compiles the generated Kotlin bindings, the per-ABI Rust libraries and the `MapView`/Compose wrapper into `rustic-maps-sdk-android-v<version>.aar`; `:map-sdk-native` is an internal source holder with no publication. The release assembly, the release workflow, the release README, the downloads renderer and the first-map, requirements, downloads and packaging pages name the single file, and the assembly and workflow reject an AAR missing the bindings, the wrapper or an ABI.
- The WebAssembly release is one tarball: the npm-shaped `rustic-maps-sdk-web-v<version>.tgz` already carries the module, its JavaScript shim, both type files, the manifest, licences and notices, so the plain `.tar.gz` is retired; a static host extracts the same tarball and serves its `package/` directory. The assembly, workflow, downloads renderer, release README, WebAssembly tutorial and requirements section follow.

### Fixed (2026-11-02 — a configured provider outranks the keyless raster fallback)

- A page-registered URL-template raster source is now a fallback: `registerUrlTemplateRasterTileSource` no longer writes the `RasterTiles` registry slot, the canvas and `fetchRasterTile` resolve the configured provider first and the fallback second, `clearProviderRegistrations` keeps the fallback, and the new `clearUrlTemplateRasterTileSource` removes it. `providerRegistrations()` reports the raster contract as unconfigured while only the fallback serves, so a keyed session can never be silently redirected to the keyless source.
- The browser transport drops the Fetch specification's forbidden request headers (`user-agent`, `host`, `content-length`, `connection`, `cookie`, `origin`, `referer`, `accept-encoding` and the `proxy-*`/`sec-*` families) before `fetch`; the native transport still sends the provider's declared user agent. Forwarding one made a cross-origin tile request advertise it in a CORS preflight that the tile host refused, which is why no live tile loaded.
- The browser demo stops passing a user agent.
- Browser cases pin the precedence in both directions — a keyless page fetches through the fallback, a configured provider leaves the fallback untouched (request-origin counting with an intercepted tile), the readback does not call the fallback configured — and prove that a declared user agent never reaches the request.

### Added (2026-11-02 — symbol text and placement semantics)

- Symbol layers now resolve their final text the way the specification defines: the `text-field` expression (including a `format` expression flattened to its text), legacy `{property}` tokens substituted from the feature, and `text-transform` applied.
- The label pass honours `text-offset` (a displacement from the anchor, in ems), `symbol-avoid-edges` (a label whose padded box would cross the viewport edge is dropped), `text-ignore-placement` (collision rejection skipped), `symbol-placement: line-center` (one placement at the run midpoint) and a declared `symbol-spacing`; a layer that omits `symbol-spacing` keeps the renderer's 90-pixel repeat distance, so existing styles and goldens are unchanged.
- Unit tests pin token substitution with a whole-number property, the case transform, the offset shift, line-center's single placement, the spacing-driven repeat count, edge avoidance and ignore-placement.

### Added (2026-11-02 — fill and circle paint: outlines, translations, strokes, blur and pitch)

- Filled polygons honour `fill-outline-color` (a one-pixel ring over the outer ring and every hole), `fill-antialias` (a disabled edge paints no outline) and `fill-translate` (a screen-pixel displacement); opacity folds into the fill's alpha.
- Point markers honour `circle-stroke-color`/`-width`/`-opacity` (a stroke drawn outside the radius), `circle-blur` (a fainter fringe under the marker), `circle-translate`, and `circle-pitch-scale`/`circle-pitch-alignment`. An explicit `map` pitch behaviour projects ground offsets, so a pitched camera sees the ellipse; a layer that declares neither property keeps this renderer's screen-facing default, documented on `DEFAULT_POINT_MARKER_PITCH_SCALE`.
- A translated fill or a ground-pitched circle joins the styled line stroke in requiring the camera-space overlay assembly (`overlays_require_camera_space_strokes`); every other overlay keeps the cached world-space path.
- A new headless golden (`fill_and_circle_paint.png`) pins an outlined fill, a translated fill, and plain, stroked and blurred markers; unit tests pin outline gating, the translation shift, the stroke annulus, the blur extent and the pitched ellipse.

### Added (2026-11-02 — line caps, joins, dashes, offsets and casings)

- Line layers now honour the style specification's line presentation: `line-cap` (butt, round, square), `line-join` (miter with the specification's limit of 2, bevel, round), `line-dasharray` in line-width units with the phase carried across a polyline's segments, `line-offset`, `line-gap-width` (two casing bands outside the gap), `line-blur` (a documented fringe painted under the core stroke) and `line-sort-key` (stable feature order within a layer). Paint — colour, width and opacity — already resolves per feature from expressions.
- The renderer carries the presentation as `PolylineStroke` on `RoutePolylineOverlay`; `None` keeps the existing default route stroke, so routes, traffic, the seven committed styles and every existing golden are unchanged. A style layer that declares any line presentation property gets the styled stroke.
- The WebAssembly canvas detects a styled overlay (`overlays_require_camera_space_strokes`) and assembles that list at the frame's camera pose, because a dash phase and a cap are screen-space; every other overlay keeps the cached world-space path.
- A new headless golden (`line_semantics_strokes.png`) pins miter, round and bevel joins, round and square caps, dashes, offsets and casing gaps; unit tests pin each behaviour's geometry.

### Added (2026-11-02 — style sources bind to providers, GeoJSON tiles deterministically, raster layers route)

- A style document may now declare several sources and bind layers to them by id: vector sources carry TileJSON `url`/`tiles`, `bounds`, `scheme`, zoom spans, `attribution` and `vector_layers`; raster sources carry their own; inline GeoJSON is a third kind. `minzoom`/`maxzoom`, a TMS `scheme` and `bounds` now change what is fetched, and a style's `attribution` wins over the provider descriptor's. A layer naming an unknown source is reported as a diagnostic and the rest of the style still draws.
- Sources bind to providers by registry key at the host seam (`rustic-maps-rendering::source_binding`): a style names a source abstractly and the host supplies a vector, raster or GeoJSON binding. No URL, credential or transport lives in the style model or the renderer — a provider crate owns them — so adding a provider still needs no style change.
- GeoJSON tiling is deterministic: web-mercator projection into tile-local units, ring and segment clipping, integer quantisation with duplicate collapse, positively wound exteriors and negatively wound holes, features in input order with sorted attributes and stable generated ids. The same input and viewport produce byte-identical tiles.
- A `raster` style layer routes to the existing raster scheduler through its binding rather than the vector overlay pipeline; a frame with both kinds fetches each from its own provider.

### Changed (2026-11-02 — the renderer paints compiled style plans, in layer order across every tile)

- The vector renderer no longer reads the retired per-tile style artifact shape: `rustic-maps-style` compiles a version-8 document into per-layer render plans (resolved zoom spans, per-feature paint and layout expressions, filters ready to evaluate), and `vector_style.rs` is now its consumer. The seven committed styles are generated into version-8 documents by `scripts/convert-vector-style-artifacts.mjs`, whose `--check` keeps them byte-stable.
- Painting is ordered by layer across the whole visible tile set rather than per tile, so a later tile's fill can no longer paint over an earlier tile's road line. A cross-tile test proves it with two adjacent addresses and a fill-then-line style.
- The retired parser survives only as a `cfg(test)` module that the parity test imports: seven styles compared against it at five zoom levels over three committed tiles, with exact layer order, resolved paint, overlay primitives and label candidates, and zero unsupported properties in every converted document. The five vector goldens and three base goldens pass without regeneration.

### Added (2026-11-02 — MapLibre style documents, expressions and filters)

- New `rustic-maps-style` crate: a renderer-free, wasm-safe parse of the MapLibre version-8 style document (sources, ordered layers, typed layout/paint, zoom spans, filters, root properties) with CSS colours, a typed expression tree, `step` and linear/exponential/cubic-bezier `interpolate`, `case`/`match`/`coalesce`, comparisons and boolean logic, arithmetic, string and colour operators, and normalization of the deprecated `{stops, type, base, property, default}` function form. Both the legacy and expression filter spellings normalize into the same boolean tree. Unknown and unimplemented properties are reported with their layer or source id rather than silently dropped; rendering behaviour, `vector_style.rs` and every existing golden are untouched.
- The committed MapLibre demotiles style parses with zero unsupported properties on all eight of its layers and its legacy functions and `match` expression evaluate as designed. The fixture is test data only, accompanied by its BSD-3-Clause licence text and the MapTiler, ODbL/OSM US and Natural Earth attribution.

### Added (round 129 — the browser target serves the whole provider contract, and the React Native package ships its libraries)

- The WebAssembly surface now serves the contract's provider calls through the same per-capability registry the native targets use: `configureProvider(contract, providerKey, credential)` chooses the provider for one capability, `providerRegistrations()` reports which provider serves each, `routeCapabilities()` answers the modes and strategy the serving router honours, and `reverseGeocode` and `fetchElevationPoints` complete the geocoding and elevation calls. The live calls resolve through the registry, so a call with no provider installed answers the contract's own "no provider configured for <Contract>" text instead of a browser-specific sentence.
- `registerUrlTemplateRasterTileSource(source)` installs any `{z}/{x}/{y}` raster tileset as the serving raster-tile provider, validated at registration; the canvas' live raster fallback fetches through it, and the canvas no longer carries its own template machinery. The five legacy browser calls (`registerProvider`, `clearProvider`, `setProviderCredential(apiKey, baseUrl)`, `clearMapquestProviders`, `MapView.setRasterTileSource`) are retired.
- The React Native npm tarball is self-contained: it carries the three Android static archives (`arm64-v8a`, `armeabi-v7a`, `x86_64`) and `AngoratekRusticMapsSdkFramework.xcframework`, staged from the Android and iOS release builds with no second Rust build. Installing it needs no Rust toolchain. A fresh checkout can package it because `proguard-rules.pro` and `AndroidManifestNew.xml` — both named unconditionally by the generated Gradle file — are versioned sources again.
- `scripts/verify-rn-package.mjs` reads the packaged podspec, Gradle file, `CMakeLists.txt` and `package.json` out of the tarball and fails on a named-but-absent or empty artifact, on a per-ABI directory carrying an archive no manifest names, on a `files` allow-list that would drop one, and on a version that disagrees with the artifact set. The release workflow's `rn` job now waits for the `android` and `ios` jobs and runs the same gate.

### Added (round 129 — the layers panel reaches Android and React Native)

- The Android reference application gains the on-map layers panel: Base map (registry raster sources and vector styles, serving marked), Traffic (congestion-coloured flow polylines through the SDK's flow capability, one request per settled camera, cleanly off), Overlays (the shared scenes) and Device location (state plus Stop), docked above the attribution notice, with remembered traffic restored only while a provider serves it. `MapView` gains `cameraGeographicBounds()` for the request. 4 JVM tests and 7 Espresso cases; the platform build and the connected suite are green.


- The React Native reference shell gains the on-map layers panel the browser and iOS already carry: Base map (raster sources and vector styles from the shared registry), Traffic (congestion-coloured flow polylines, refreshed on a settled camera, switched off cleanly), Overlays (the shared scenes) and Device location (state plus Stop), with serving marks, the `Needs <credential>` and `Not configured` traffic states, remembered choices, an active dot and a modal that returns focus to its control. 14 new jest cases; 145 in the shell's suite.
- A traffic request wider than the cache's cell budget is now answered straight from the provider instead of being refused: the cache bounds its own memory, never what a caller may ask for.

### Fixed (round 129 — switching a base map leaves nothing behind)

- After switching the base map from a vector style back to raster tiles, the vector style's fills and lines kept painting over the tiles until the page happened to change its own overlays: the canvas cached its world-space overlay tessellation by the page's overlay revision, and installing, replacing or clearing the vector base changed the composed list without changing that revision. A frame now invalidates the tessellation on install, on clear, and when a clear lands mid-draw. A browser case reproduces the defect (29,042 stale triangles after the switch back) and now asserts the post-switch frame carries only the page's own geometry.

### Added (round 129 — a bounded traffic cache)

- Traffic-flow answers are cached by web-mercator grid cell (zoom 12, about 10 km) rather than by the exact requested bounding box: a small pan and a revisit of an earlier area are cache hits, and the missing cells are fetched once as their union (a miss returns the provider's own rows unchanged; a hit returns what the cells hold). The cache keeps 64 cells with least-recently-used eviction and a five-minute freshness window, both registration paths wrap their traffic provider with it, and a request larger than the budget is refused with the budget named instead of being truncated. The browser export `trafficFlowCacheStatistics()` reports hits, misses, entries, evictions and segments for the developer console.

### Changed (round 129 — the internal docs are one navigable set)

- `docs/` was consolidated: duplicate style guides merged into `DocsStyleGuide.md`, landed designs and dated audits moved under `docs/history/` (or deleted where git history is the only remaining value), superseded research removed, and a `docs/README.md` index added. The drift the review proved — retired browser calls still described as current, a wrong rate-limiter path, a stale platform order, an inventory frozen at an earlier round — is corrected.

### Added (round 129 — the browser contract-parity gate)

- `platform/web/tests/contract_sync.mjs` asserts that every `MapSdk` member the frozen contract declares has a wasm export or a listed exception with its reason, and that no exception entry is stale. The browser's ten documented exceptions are the eight host-implemented (foreign) registration members, the three disk-tile-cache members and the foreign failure-diagnostics readback.
- New browser suite `provider_contract_surface.spec.mjs` (8 cases) covers the registration readback before and after a credential, per-capability configuration, the refused-configuration guarantee, the routing capability answer, reverse geocoding over a committed fixture, the elevation refusal and the URL-template source's validation.

### Changed (round 129 — one truth about provider availability)

- Every capability page's call table now names provider registration, route capabilities and elevation, and states that no shipped provider serves elevation. The WebAssembly page, the first-map and search-and-routing wasm tabs and the add-a-provider matrix describe the contract-shaped calls.

### Added (round 128 — directions you can choose, a layer panel on iOS, and provider traces)

- Routes carry their transport mode and strategy chosen by the reader: `routeCapabilities()` (addition-only, on `MapSdk` and `RoutingEngine`) reports which modes and whether the serving provider can honour Shortest; MapQuest and Valhalla support every choice, OSRM has no shortest-path switch and says so; the cache and offline-fallback wrappers forward the answer. Browser, Android, iOS and React Native reference apps now offer Car/Walk and Fastest/Shortest beside the plan action, apply the choice to every plan they perform, remember it across launches, and disable an unsupported choice with its reason. Both native apps had hardcoded the pedestrian profile, which is why an Oakland plan through OSRM returned a ferry route where the vendor provider took the Bay Bridge.
- The iOS reference app gains the map layer panel (Base map, Traffic, Overlays, Device location), and the round location control now starts the device source instead of only re-centering: the missing authorization callback was added so a grant starts the source without a second tap, the first-fix re-centre path was repaired, and failures render in the panel with their reason.
- React Native exposes `configureProvider`, `providerRegistrations`, `routeCapabilities` and the directions selection through both native runtimes, the modules and the curated TypeScript SDK, with per-capability settings rows and remembered choices.
- Provider request observation seam: `provider-transport` builds a redacted `ProviderRequestObservation` per attempt (path with query values removed except documented shape parameters, status class, duration, byte count, a 512-byte snippet on a character boundary) behind a process-wide observer; the OSRM, Valhalla, MapQuest and URL-template clients now report through `ObservedTransport`. The developer console that reads this stream is designed in `docs/design/DeveloperConsole.md`.
- Android's plan flow was audited against the iOS reference and fixed: labelled Origin and Destination fields, a per-field Search action, an assignment hint naming the field a result will fill, and a Plan route action enabled only when both fields hold resolved places — typing alone no longer geocodes on the press.
### Fixed (round 128 — the iOS dock no longer pushes results off the screen)

- Adding the Directions row exposed an overflow in the iOS plan sheet: the full sheet plus the map-control stack was taller than the phone screen. The dock is now capped by the measured map-control row and the plan surface scrolls as one view with an explicit keyboard-dismiss control.
### Changed (round 128 — the browser façade's gap is recorded and scheduled)

- Research established that the browser target's `registerProvider` and `MapView.setRasterTileSource` calls predate the contract-defined provider wave and that `reverseGeocode`, `configureProvider`, `providerRegistrations`, `routeCapabilities` and `registerUrlTemplateRasterTileSource` are missing there. The ruling retires the legacy calls and closes the gap (M45), and the docs will state one truth: every MapQuest capability is available on every target.
- Distribution audit: the Android AARs, iOS XCFramework and wasm package are self-contained; the React Native tarball is not, because its podspec names a framework the package does not contain. The ruling bundles the prebuilt binaries into the RN package and adds an assembly gate (M48).

### Added (round 127 — any provider from application code, per-capability settings, and an on-map layer control)

- Every provider contract can now be implemented in application code: `RoutingProvider`, `DistanceMatrixProvider`, `SearchPlaceProvider`, `TrafficIncidentProvider`, `TrafficFlowProvider` and `ElevationProvider` join geocoding and raster tiles as `[Trait, WithForeign]` interfaces with their `register_*_from_foreign` calls, and `fetch_elevation_points` plus the `ElevationPoint` record give the elevation slot a call. Each registration reports the `application` key in `providerRegistrations()`, and a mapped or unclassified failure still reaches the typed refusal and the provider-failure diagnostics.
- The Android and iOS reference applications gain one settings row per capability, rendered in the SDK's readback order from the shared descriptors, with a picker limited to providers that declare the capability for that platform and that app's own credential rules; refusals show verbatim and the choices are remembered across launches. Raster and vector tile rows stay read-only and point at the map's layer control.
- The browser demo gains the on-map layer control: a floating button above the attribution notice opens Base map (raster sources and vector styles), Traffic (congestion-colored flow polylines from the serving traffic-flow provider, refreshed on camera settle) and Overlays (the demo scenes), plus a Device location row. The round my-location control now starts the device source and centres on the first fix, and the origin and destination fields each carry a "My location" chip.
- New documentation: *Mix providers per capability* is published, and *Add a provider* now covers host implementations of every contract with Kotlin and Swift routing samples.
### Fixed (round 127 — the style goldens follow the repeated road names)

- The three named-style browser goldens (Bright light, Fiord dark, Positron light) had been left stale by the repeated-road-name change and failed the outlier compare at 0.026 against the 0.02 limit. They are regenerated from the current renderer; the only difference is the added repeated names and the label placements they displaced.
### Added (round 126 — a provider per capability, and road names that repeat)

- `MapSdk.configureProvider(contract, providerKey, credential)` chooses the provider for one capability and leaves every other capability serving, and `providerRegistrations()` reports which provider serves each capability in a stable order. An application can now search with one provider, route with a self-hosted engine and draw its base map from a public tileset. `setProviderCredential` still installs a whole vendor set and now records the serving provider per capability; a refused key or a blank credential leaves the previous provider in place.
- Two keyless routing crates ship in the SDK: `osrm-provider` (OSRM route service) and `valhalla-provider` (Valhalla route service, with a polyline6 decoder and a documented maneuver mapping). Both run on committed fixtures in tests and take the deployment's base URL through the credential field.
- Every provider descriptor now declares `served_contracts`, and the registry carries descriptors and fixture routes for both routing providers.
- New documentation: the second half of *Add a provider* — the descriptor and catalog steps plus Kotlin and Swift `configureProvider` samples and a worked routing-provider walkthrough — and a *Mix providers per capability* page.
### Changed (round 126 — long road names are drawn along their curves)

- A road name now repeats along a long run instead of painting once at its midpoint: names sit at least 90 px apart and each follows the bearing of the segment under it. The Bright and OpenMapTiles city goldens were regenerated for the added repeats.
### Added (round 125 — remembered choices, a mobile-demo link, and a vector path that caches and draws progressively)

- The browser demo remembers explicit choices across reloads in a versioned local record — theme, base map source or vector style, demo plan, scenes, camera preset, speed, origin/destination and last query — with a reset row that leaves credentials untouched, and its header links to the hosted mobile demo. Restoring endpoints fills the fields without planning a route. The React Native, Android and iOS reference apps use the same record through their native stores, with the same reset behaviour; every settings surface (browser dialog, native sheets) now caps at the panel height and scrolls internally with a pinned header, verified on the standing devices.
- The vector base caches payloads (the native SQLite store at the 30-day tile policy, an in-memory TTL/LRU on wasm), prefetches a viewport ring and coarse parents on a budget, draws from cache with parent stand-ins while children are in flight, and reports cache hits, in-flight fetches, fallback tiles and time-to-first-drawn-tile. The frame never blocks on a fetch.
### Fixed (round 124 — the Android emulator attaches to its GPU)

- The native surface and headless device requests now use `wgpu::Limits::downlevel_webgl2_defaults()`. The Android emulator's ANGLE/SwiftShader device reports a zero compute-workgroup limit, which the previous downlevel request (65535) exceeded, so the reference app failed with `RenderingSurfaceUnavailable`; the renderer does no compute work, and the Metal inter-stage cap that motivated the downlevel request stays satisfied.
### Changed (round 123 — every vector style thins its roads below the city zoom)

- Bright, Fiord, Positron and both OpenMapTiles styles now gate their road classes by zoom and scale widths with it, anchored on the width each layer already painted at the city zoom so the committed z14 goldens are byte-identical while regional frames lose the dense mesh. Minor, residential, service, track, path and pedestrian classes no longer paint at zoom 9, and every visible road there is at most 60% of its city weight.
### Changed (round 122 — the Carto styles draw a road hierarchy)

- The OpenStreetMap US Carto light and dark styles now gate each road class by zoom and scale their widths with it, instead of painting every class at one fixed pixel weight: motorway and trunk from z4 with a thin casing, primary from z7, secondary from z9, tertiary from z11, residential streets from z12, pedestrian ways from z14 and service roads from z15, with links at 0.8 of their parent. A regional view (about z9) now reads as a hierarchy — a ~2.5 px motorway, hairline primary and secondary, no street grid — instead of a dense orange mesh. The in-code fallback carries the same table and a unit test pins the widths and visibility per class.
### Docs (round 121 — regional-zoom road hierarchy research and plan)

- `docs/design/VectorStyleRefinement.md` traces the reported "thick, overlapping roads" at a 3 km scale to the style data — every Carto road layer paints a constant width with no per-class zoom gate — compares what MapLibre/Mapbox, Google and HERE do, and sets out a per-class width/min-zoom table. `PLAN.md` M36 tracks the implementation: the Carto styles first, then the other schemas, route shields and place-label thinning, and a recorded z9 tile with a golden that pins the reported frame.
### Changed (round 120 — the reference applications and the Kotlin package use the product identity)

- The Android and iOS reference applications ship as `com.rusticmaps.sdk.referenceapplication`, the React Native shell as `com.rusticmaps.sdk.refapprn`, and the shipped Kotlin wrapper package is `com.rusticmaps.sdk` (breaking for Android integrators of the wrapper, with the JNI symbols, R8 rule and tutorial imports moved atomically). The frozen package identities, the credential storage key and the Appetize build ids are unchanged; the display name stays *Rustic Maps SDK Ref App*.
### Added (round 119 — the browser canvas folds into the generated wasm module)

- The binding crate now depends on the canvas crate for `wasm32` only and re-exports it, so one `ubrn build wasm2` module carries both the generated contract surface and the hand-written canvas classes under a single initialization. A provider registered once through the generated `MapSdk` serves both the contract's fetches and the canvas's own tile requests, so a page no longer keeps two registries. Proven end to end in Chromium against the committed fixtures; the page migration and the canvas-class naming split follow.
### Changed (rename — the SDK crates carry the product name)

- The Rust SDK crates and their directories are now `rustic-maps-cache`, `rustic-maps-core`, `rustic-maps-glyphs`, `rustic-maps-navigation`, `rustic-maps-native-core`, `rustic-maps-offline-graph-archive`, `rustic-maps-offline-graph-compiler`, `rustic-maps-positioning`, `rustic-maps-rendering`, `rustic-maps-vector-tiles` and `rustic-maps-web-bindings`. Workspace dependencies, `use` paths, `cargo -p` invocations, fixture and documentation paths all follow. The UniFFI contract file is `rustic_maps.udl`, the native library is `librustic_maps.so`/`.a`, and the WebAssembly package emits `rustic_maps_web_bindings.js` and its declarations. Published package identities, the `rustic_maps` contract namespace, the `rustic_mapsFFI` Swift module and the reference-app build ids are unchanged; no contract semantics changed.

### Fixed (round 118 — wasm-safe clocks in diagnostics and navigation)

- The SDK uses `web-time` for its wall clock and monotonic timer, so a failed foreign provider call, a recorded diagnostic event, speech dispatch, the reroute gate and the trip recorder no longer panic on `wasm32-unknown-unknown` and abort the module through a poisoned mutex. Native behaviour is unchanged (the crate re-exports the standard library's types there); the generated-browser harness confirms the failure path now rejects normally and keeps the module alive.
### Added (round 117 — the contract surface builds for wasm and the generated browser bindings are proven)

- The binding crate now compiles for `wasm32-unknown-unknown`: the native runtime, disk cache, surface and JNI are target-gated, exports await the browser task queue on wasm, and a placeholder contract `MapView` stands in until the canvas adapter bridges the surface-owning members. A spike with `ubrn build wasm2` generated the full TypeScript surface and proved the init handshake, synchronous and asynchronous exports, an async foreign-trait callback invoked from Rust, and a same-origin provider tile fetch. The spike also found that `SystemTime`/`Instant` panic on wasm and abort the module through a poisoned mutex; replacing them with a wasm-safe clock is the next step before the browser surface can migrate.
### Fixed (round 116 — the React Native package resolves its own runtime and CI builds its sources)

- The React Native package now declares `@babel/runtime`, which its transformed sources require; without it a consuming test environment failed to resolve `@babel/runtime/helpers/interopRequireDefault`. The shell's jest config also resolves the package (and its deep entry points) straight to its sources, so the suite no longer depends on how npm materialises the `file:` link, and CI generates the gitignored `src/index.ts`/`src/generated/` trees with the new shared `scripts/generate-rn-bindings.sh` before running jest and the typecheck. `build-rn.sh` delegates its two codegen steps to the same script.
### Added (round 115 — road names on the OpenStreetMap US vector styles)

- The Carto light and dark vector styles now label named roads along their lines — motorway, trunk, primary, secondary, tertiary, residential and unclassified, each appearing at its own zoom with size stops and a halo — and the dark style gained the place-name layer it was missing. The in-code fallback carries the same label table, and a synthetic Carto-schema tile test pins both styles to a line-placed road candidate and a place candidate.
### Changed (round 113 — UniFFI aligned on 0.31.2)

- The workspace pins `uniffi 0.31.2`, matching the `uniffi-bindgen-react-native 0.31.0-6` line used for the React Native bindings. The binding crate compiles unchanged — the hand-written foreign-trait contracts, the `[Async, Throws, Name]` members and `uniffi::generate` behave identically — and every generated Kotlin/Swift/React Native binding regenerates with no marker changes. The React Native generator's TypeScript now agrees with the Rust scaffolding on all 108 checksum items (37 disagreed under 0.32.2), retiring the guarded-import workaround. Only the lockfile and the third-party notices change.

### Fixed (round 114 — the browser demo keeps presenting under strict WebGPU validation)

- The GPU pass timestamp ring no longer cancels or remaps a pending readback buffer. Firefox rejects that lifecycle ("Buffer … is still mapped", "already mapped", "map is pending"), which flooded the console and stopped the map; the ring now leaves a pending slot alone, reads only completed maps, and skips the sample when no slot is free. A browser case drives a trip and a pan and asserts the frame loop emits no WebGPU validation error.
### Added (round 112 — Rust sources in the release set)

- The local release assembly now produces `rustic-maps-sdk-rust-v<version>.tar.gz`, the workspace source tree at the release commit, since the crates are path-linked and cannot be packaged for a registry; the downloads page labels it and the chapter shows the path-dependency install. It ships with the next version rather than being added to the immutable 0.1.0 record.
### Changed (round 111 — typed failures on the WebAssembly surface)

- Every failure the WebAssembly surface throws is now a JavaScript `Error` with a stable `code` (the contract's `SdkError` variant where one exists, a documented seam-level code otherwise), so callers can branch on the code instead of matching message text. The message is unchanged and the error's `name` is empty, so `String(error)` yields exactly the previous text; code that type-checks the thrown value as a string should switch to `error.code`/`error.message`. The TypeScript declarations document the shape and the complete code list.
### Added (round 109 — GPU pass time in the browser frame report)

- The browser canvas now reports `gpuPassMilliseconds` from timestamp queries around its render pass, read one frame later through two alternating map-readable buffers so a frame never waits on its own timestamps. The statistic is absent on adapters without timestamp support. The frame-budget harness prints a `gpu pass (ms)` percentile row (quantization and previous-frame lag stated), and the performance docs carry the method and measured numbers: 0.8 ms median on the vector base, 0.15 ms on the raster base.
### Fixed (round 107 — the React Native keyless plan reads the typed fields)

- The reference shell's keyless **Plan route** now reads the origin and destination texts at the moment of the press; the action had captured them without depending on them, so valid demo-place names typed into the fields were reported as `DemoPlaceUnresolved`. The navigation drawer also carries **Stop device navigation** while a device-driven trip runs, matching the native reference apps. Verified on the Pixel_9 emulator end to end (keyless vector base, plan, device-driven guidance with banner and ETA, drawer stop), with jest at 14 suites / 89 tests and `tsc` clean.
### Added (round 106 — device-driven navigation on every reference app)

- Android, iOS and React Native join the browser demo: a **Navigate from my location** / **Stop device navigation** control in the Location group drives the planned route's guidance from the device's own fixes, with the banner, ETA line and follow camera reading the same navigator stream the simulated trip reads. Each app starts the location source when needed, ends a simulated trip first, states the plan-first sentence when no route exists, and stops only the source the mode started. The native shells also carry the control in the navigation drawer while the trip replaces the sheet. The Android Espresso case, the iOS live-fix XCUITest and the React Native jest suite cover the flow.
### Added (round 105 — device-driven navigation in the browser demo)

- The browser demo's Location section gained **Navigate from my location**: guidance follows the planned route from the device's own fixes instead of the recorded trace, with the turn banner, ETA line and follow camera reading the same navigator stream. The control starts the location source when needed, states "Plan a route first; device navigation follows the route you planned." when no route exists, and stops the trip and only the source it started. A Playwright case covers the whole path with a stubbed device position.
### Added (round 104 — the schema vector styles paint text)

- The OpenMapTiles-schema style artifacts now carry the same seven symbol layers as the named styles: five road classes, points of interest and place names. A descriptor that offers only `openmaptiles-light` or `openmaptiles-dark` now draws labels instead of placing unnamed circles. A scheduler test pins both artifacts against the committed z14 fixture, and a new headless golden renders the schema style with its text; the golden harness fails any styled scene whose label layer is empty.
### Docs (round 103 — public documentation comments in product voice)

- Every public crate's `///` and `//!` comments now describe what the item does, its parameters, returns and failure modes instead of the project's process: the binding crate, the WebAssembly binding crate, the renderer, glyph, cache, vector-tile, offline-graph and provider crates were rewritten without milestone, round, plan or decision-record references while keeping their technical rationale. The hand-maintained WebAssembly TypeScript contract mirror was rewritten the same way, so the generated API reference pages carry no internal wording. Code lines are unchanged.

### Fixed (round 103 — the mobile demo stays usable while it rests)

- A refused session no longer removes the page's **Start demo** button, so a transient failure can be retried without reloading; the resting state and the footer now link the browser demo, which needs no session or monthly budget.
### Changed (round 102 — reusable GPU upload buffers)

- The browser canvas no longer creates vertex or index buffers per frame: the tile quads, the world-space overlay (across overlay revisions), the label halo and text passes and the location indicator each write into a persistent slot that allocates once and grows only when a payload outgrows its high-water mark. The frame statistics expose `gpuBufferAllocations`, and the browser golden suite asserts the count does not move across settled frames. Rendering is unchanged (browser suite 122 tests); the committed harness now measures `renderFrame` p50 1.4 ms, p95 1.5 ms and p99 8.2 ms on the vector base, and 0.2 ms on the raster base.
### Fixed (round 101 — the React Native launch credential is not dropped before an activity exists)

- The React Native Android runtime records a successful launch-injected credential application and the module retries it on the first host resume (`LifecycleEventListener`), so a launch intent read while `currentActivity` is still null no longer lands keyless. A failed application is retried; a successful one is not re-registered.

### Tested (round 100 — the native styled-base golden)

- A new `map-rendering` golden renders the Bright style over the committed z14 OpenMapTiles fixture through the shared vector-base scheduler and the headless renderer, composing overlays and labels. Android, iOS and React Native draw the same Rust pipeline, so the native family now pins its styled base alongside the browser's three per-style goldens.

### Tested (round 99 — a browser golden per named vector style)

- The golden harness now renders Bright light, Fiord dark and Positron light through the canvas-owned vector source over the committed z14 OpenMapTiles fixture at a fixed tile-centered camera, diffing each frame against a new committed golden. A style-artifact edit that changes what a style id paints fails the compare; the fixture route derives its path from the provider registry and answers only the committed tile. Browser suite 121 tests.

### Changed (round 98 — downloads render from the release manifest)

- `scripts/assemble-release-artifacts.sh` writes `downloads.json` (name, size and SHA-256 per artifact), and the documentation build renders the downloads version heading and table from it — links, sizes, checksum sidecars and platform descriptions — so publishing a new version never edits prose. A build without a manifest keeps the page's explicit unpublished state.

### Tested (round 96 — plan-flow gating and a GPU-timing investigation)

- iOS and React Native now cover the explicit two-field plan flow (disabled-with-reason until both fields hold text, then enabled); the XCUITest and jest cases pass with their full suites. A first GPU-timestamp attempt regressed headless rendering (later frames black) and was reverted; the required one-frame-lagged readback ring is recorded under M23.

### Tested (round 95 — the native plan-flow gating)

- iOS and React Native now test the explicit two-field plan flow: `Plan route` stays disabled with a reason naming the missing field and enables only once both fields hold text. The XCUITest and jest cases pass with their full suites (iOS `** TEST SUCCEEDED **`, RN 86 tests, `tsc` clean).

### Changed (round 94 — the wasm package gets its optimization pass)

- `scripts/build-wasm.sh` centralizes the release wasm build, the wasm-bindgen `web` emit and an optional binaryen `wasm-opt -Oz` pass; with binaryen installed the module goes from 4,079,676 to 3,718,794 bytes (91.2% of the unoptimized size), and the optimized package passes the full 118-test browser suite. The release assembly uses the script, and the browser-loop guide and performance record document the step.

### Fixed (round 93 — the iOS device source reports its fixes)

- The iOS reference app now states every accepted device fix (`Position …`) beside the Location control, and a new XCUITest proves a live fix from a simulated Core Location scenario: the source starts, the controller smooths, the tracking dot and camera follow, and the full iOS suite passes.

### Added (round 91 — React Native device positioning)

- The React Native curated `MapsSdk` exposes `startDeviceLocation`, `stopDeviceLocation`, `latestDeviceFix`, `deviceLocationSmoothedCourseDegrees` and `deviceLocationSourceStatus`, with typed fix and source-error records; Android's runtime feeds the SDK's `DeviceLocationSource` and the iOS pod carries the same controller with a pod-local Core Location source. The reference shell's half sheet gained the **Location** control — runtime permission, the tracking dot with smoothed course and accuracy ring, camera centring, and denied/unavailable status lines. Jest 85 tests, `tsc` clean, Android Kotlin compiled; the RN iOS app build remains blocked by React-Core-prebuilt's missing `cassert`.

### Added (round 90 — the Rust desktop positioning example)

- `cargo run -p map-positioning --example simulated_walk` walks a synthetic route through the contract controller and simulated source, printing the last fix and the smoothed course. The example is the documented Rust desktop path; OS-backed Rust sources remain an opt-in choice.

### Added (round 88 — the device-positioning contract surface)

- The contract gained `PositionFix`, `PositionSourceError` and the `PositionController` interface as addition-only members: hosts feed the fixes and failures their OS location source reports, and the controller validates them, smooths course around the circle, tracks staleness and records source failures for the renderer's location indicator, a follow camera and the `Navigator`. The Rust implementation delegates to the `map-positioning` crate; Kotlin, Swift and React Native bindings regenerate from the UDL, the wasm seam already serves the same class, and a Rust integration test covers the behaviour.
- Android's SDK ships `DeviceLocationSource` (LocationManager over the enabled providers, runtime-permission check, fixes and failures into the contract controller), and iOS ships the Core Location equivalent in `MapNativeCoreUI`; both compile against the regenerated bindings.
- Both native reference apps now expose a **Location** group: the runtime permission (Android) or authorization prompt (iOS), a Show/Hide control, the renderer's tracking dot with its accuracy ring, camera centring on each fix, and honest denied/unavailable status lines. Android's manifest declares the location permissions and its instrumentation suite covers the control; iOS carries the usage string and an XCUITest for the control.

### Added (round 87 — browser device positioning through the Rust controller)

- The WebAssembly seam exposes the `map-positioning` `PositionController` to the page, and the demo's settings sheet gains a **Location** section: pressing Show my location starts `navigator.geolocation`, feeds every fix through the controller (validation, stationary de-duplication, circular course smoothing, staleness), draws the renderer-owned tracking dot with its accuracy ring and centres the camera. Denied permission, an unavailable source and a timeout are reported both to the controller and in the sheet; stopping the watch clears the controller and the layer. Two Playwright cases cover the granted fix and the denied permission, and the parity floor carries the new export.

### Docs (round 85 — the full documentation set in the product voice)

- Every remaining chapter was rewritten against the documentation standard: one developer quickstart voice throughout, outcome-first structure, no insider asides, and corrected capability notes for the browser surface (suggest/search, route options, navigation listeners). The snippet gate checks 84 snippets with zero failures; the site build and fail-closed scrub pass.
- The public rustdoc for the record, trait, core, rendering, navigation and positioning crates was rewritten as consumer documentation, and the hosted-demo uploader gained `--reapply-settings` (documented in `docs/MobileDemoOperations.md`) so the live Appetize builds' referrer allow-list can be restored without re-uploading.
- The rewritten documentation set (including the generated API references, provider guides, Rust guide, downloads and screenshots) is published at the live site.

### Added (round 84 — generated API references and the positioning core)

- The platform API references are now generated at documentation-build time by `scripts/generate-api-reference.mjs` from the SDK contract, the generated Kotlin/Swift/TypeScript bindings and the wasm export sources; `--check` fails when a page is stale, and every local or CI site build regenerates them. Hand-maintained copies are gone, with generator tests for determinism, drift detection and contract coverage.
- A new `map-positioning` crate starts the device-positioning pipeline: `PositionController` validates host fixes, ignores stationary repeats for the course window, smooths course around the circle, tracks staleness and source failures, and `SimulatedPositionSource` walks a route for tests and desktop demos. The crate is wasm-safe, joins the wasm clippy closure, and ships five unit tests.

### Added (round 83 — real downloads and a measured performance page)

- `scripts/assemble-release-artifacts.sh` builds the versioned release set locally (Android bindings and `MapView` AARs, iOS XCFramework zip, WebAssembly module archive and npm package, React Native npm package, third-party notices, per-file `.sha256` sidecars and `SHA256SUMS.txt`) when hosted CI is unavailable. The docs build stages it under `/downloads/v0.1.0/` and `downloads.md` links the real files with checksums; Sigstore bundles remain with the release workflow.
- The performance chapter and `docs/BrowserRenderPerformance.md` carry a fresh measurement of the current browser frame (Apple Silicon, hardware WebGPU, 1920×1080 vector base): `renderFrame` p50 1.9 ms, p95 2.2 ms, p99 11.6 ms, with 0 of 120 measured frames over the 16.7 ms budget; the p99 tail is identified as the overlay-revision tessellation pass.

### Docs (round 83 — consumer-facing contract comments)

- Every doc comment in `map_native_core.udl` was rewritten for outside developers: no owner names, dates, plan/round/milestone references, internal paths or implementation commentary. Declarations are byte-identical, and the Kotlin, Swift and TypeScript bindings inherit the wording on regeneration. `docs/MobileDemoOperations.md` records the hosted demo's free-plan budget, operator bypass and owner actions.

### Fixed (round 82 — route visibility and trip wording on the React Native and iOS apps)

- Starting a simulated trip in the React Native reference app now draws the active trace's route polyline and endpoint pins before the first fix, so the trip never begins on a bare map; the Android and iOS hosts push the overlays at start. The iOS reference app formats remaining durations the way Android, React Native and the browser do (whole minutes plus seconds), and its compass control uses the same **N** letter as the other platforms instead of a platform-specific symbol. The browser's trip card now carries the drawer-style ETA-distance line beside the playback controls while a trip runs.
- The iOS follow camera now rotates with the course like every other surface: the recorded-trace course helpers had their degree/radian constants swapped, collapsing every computed bearing toward north. Corrected and covered by a new XCUITest that asserts the N compass appears during the simulated trip; the screenshot set recaptured from the fixed build.

### Docs (round 82 — the documentation overhaul lands its first tranche)

- New documentation standard `docs/DocsStyleGuide.md` (outcome-first structure, direct voice, banned internal vocabulary, screenshot rules, API-reference conventions), recorded in `CLAUDE.md`, `PRD.md` §6 items 24–26 and M32/M33 in `PLAN.md`.
- The site now carries one developer quickstart (no separate GIS track), rewritten concept and architecture pages, per-platform provider capability guides with an add-a-provider guide, exhaustive API references for Rust, Android, iOS, React Native and WebAssembly, and a rewritten Rustic Maps SDK contract page that no longer republishes the internal UDL comments. The site build carries authored API and contract pages, scrubs visible prose rather than HTML attributes, and the navigation lists every new page.
- The screenshot gallery is recaptured as one set: every platform in the same simulated trip with the maneuver banner, route polyline, ETA line, speed controls and exit; the iOS duration wording and compass mark and the React Native route visibility are aligned with it.

### Changed (round 81 — the vector base no longer blocks the presenting frame)

- `VectorBaseScheduler` now shares its decoded-tile cache with a detached prefetch (`Arc<Mutex<DecodedTileCache>>`, with in-flight and last-failure bookkeeping). `cached_frame` styles from the cache alone and counts pending tiles honestly; `prefetch_missing_tiles` claims the misses and returns the concurrent fetch future; `resolve_frame` stays as the awaited form for the wasm canvas and tests. The native surface spawns the prefetch and presents the cached frame, which already carries the style background — so a first frame over unvisited ground paints paper immediately and fills in as tiles land, instead of blocking for the sum of every tile's latency. A fresh Pixel_9 install paints the styled base with labels two seconds after launch. Verified by `map-rendering`'s 235 tests and the 891-test workspace.

### Changed (round 80 — the vector base fetches one frame's tiles concurrently)

- `VectorBaseScheduler::resolve_frame` now splits the visible tile set into cached and missing tiles, fetches every miss concurrently through `futures::future::join_all` (the provider's rate limiter still spaces the requests), inserts the decoded tiles, and styles the frame from the cache. A first frame over unvisited ground waits for the slowest tile instead of the sum of every tile's latency — the diagnosed cause of Android's roughly ten-second black keyless start. A fresh Pixel_9 install now paints the styled base with its place names within seconds; the remaining work (taking the fetch off the presenting frame entirely) stays recorded in `PLAN.md` §2. Verified by `map-rendering`'s 233 tests, the 889-test workspace, the wasm closure clippy and the 116-test browser suite.

### Tested (round 79 — the vector-source contract members)

- A dedicated `platform/web/tests/vector_source_status.spec.mjs` pins the wasm canvas' `vectorTileSourceStatus` to the UDL `VectorTileSourceStatus` shape (all eight lower-camel fields) in its three states — not configured, configured, cleared — and proves malformed descriptors (unknown style, empty source name, inverted zoom range, a template without `{z}/{x}/{y}`, a non-numeric zoom) are refused verbatim while the previously installed source stays in force. The browser suite is 116 tests.

### Added (round 77 — the worked example's navigation and offline steps on every platform)

- The Android, iOS and React Native scenario modules now close the shared worked example the way the browser module already does: they start the contract's `Navigator` on the planned route with the default step-advance and deviation policies, feed the route's own first vertex as the first fix, read the trip state, and — when the caller supplies the bundled `routing_grid.rmog` bytes — plan the routing-grid demo request through `offlineRouteFromArchiveBytes`. The reference apps pass their own asset/bundle bytes, so the steps are live code rather than documentation-only paths; a missing archive skips the offline step honestly. RN's jest cases assert the Navigator arguments, the first fix and the offline request; the Android Kotlin compile and the iOS simulator build are green.

### Added (round 76 — the browser's vector base moves into the canvas)

- The wasm `MapView` now serves the contract's `setVectorTileSource`, `clearVectorTileSource` and `vectorTileSourceStatus` over the shared `VectorBaseScheduler`: the descriptor is one plain object (`urlTemplate`, `sourceName`, `attributionText`, `styleId`, `minimumZoomLevel`, `maximumZoomLevel`), and the canvas fetches the registry's tileset through the browser transport, decodes and styles each visible tile once, hides the raster tier, paints the style's own background, composes the base's geometry beneath the page's overlays and places its symbol-layer place names through the SDK's embedded default glyph font. Install/clear during a frame is refused or deferred to the frame's restore, so a fast raster⇄vector switch can never paint both bases. `contract_sync` requires the three members, the parity floor carries them, and `platform/web/types/map_native_core.d.ts` mirrors the two records.
- The browser page installs that source instead of fetching tiles and composing overlays/labels in JavaScript: `loadVectorBaseOverlays` and the glyph-range fetch leave the page path, the base-map status reports the canvas' drawn/failed tile counts and style, and the scene toggles plus planned routes stay in the page's own overlay list. The vector base's labels are now renderer-internal on all four targets (PRD §6 item 21); the transitional `setLabels`/`setGlyphRange` exports remain only for the isolated text-coverage probe.
- Verified: the full 114-test browser suite (including the vector-base, style-picker, scene-overlay and in-flight-switch cases and the Bright label case, which now reads the embedded font), contract sync, the platform-parity floor and UX parity.

### Fixed (round 76 — the Android peek's action row)

- The Android peek's new two-control row gave the subtle **Search an origin** button its default `fillMaxWidth`, which measured it at the row's full width and squeezed the weighted Start control to zero width (the label then wrapped one character per line, inflating the sheet). The subtle control now wraps to its label, so the row reads exactly like the iOS and React Native peeks; verified on the Pixel_9 (`tmp/refapp-evidence/android-peek-origin-2.png`).

### Changed (round 75 — one plan-and-drive flow across the four surfaces)

- Android and iOS gained the **Search an origin** entry on the home peek that React Native already had, so both plan fields open from the home sheet; the iOS search view now lists origin before destination and its dismiss reads **Done**, and Android's search view gained the **Plan a route** heading and lost its trailing "Search" and "Trip running…" variants. All three native search views now render the same heading, field order and dismiss word.
- Route options agree across every plan: the React Native host sessions now plan pedestrian/**fastest**/metric (they had planned shortest) and the browser's live planner sends the same options object (it had used the contract defaults, automobile), so recorded, live and offline routes describe one route.
- The browser's live guidance moved to §1's placement: the turn banner, then line and deviation alert now sit in a top-center map overlay (out of the Advanced panel), the drawer's one **ETA: Distance remaining … · Duration remaining …** line replaced the web-only arrival clock and progress list, and durations format in whole minutes plus seconds like the native shells. The browser's offline route is now the planner's route — startable, framed, and named in the route summary — and the base-map flow moved from Advanced into the settings sheet, whose sections now follow the native order (Provider credentials, Base map, Theme).
- React Native's off-route phase reads the canonical **Off route**; the allow-listed `OffRoute` override was retired from `platform/reference-tokens.json`, and the UX-parity gate now asserts the canonical word. The iOS-only "Sample journey"/"Simulate off route" controls and the browser's missing tile-cache accounting line are recorded exemptions, not silent differences.
- Verified: Android `:reference-application:compileDebugKotlin`, the iOS reference-app simulator build (`** BUILD SUCCEEDED **`), both native UI suites keyless (Android `connectedDebugAndroidTest` 13 tests/0 failures/4 skipped; iOS `xcodebuild test` `** TEST SUCCEEDED **`), the full 114-test browser suite, and the `check_ux_parity`/`contract_sync`/platform-parity gates. PRD §6 item 23 records the ruling; `docs/history/FourPlatformParityAudit.md` records the audit and the remaining exemptions.

### Added (round 64 — the tutorial's WebAssembly worked example)

- `platform/web/walkthroughScenario.mjs` runs the tutorial's one worked example end to end on the wasm target with the shared identifiers other platforms use: the keyless search resolves the shared artifact's recorded demo places, the artifact's recorded route becomes the planned route and polyline overlay, the wasm `Navigator` consumes the first fix, and `offlineRouteFromArchiveBytes` plans the committed routing-graph request. A Playwright case imports the module in the page and asserts the route distance, overlay, navigating phase with speech text, and the 222 m offline route; the browser suite is 114/114. The parity floor includes the module in its worked-example identifier gate.

### Added (round 63 — a name/shape gate for the one worked example)

- The platform-parity floor now asserts the tutorial worked example's shared identifier set across the Android, iOS and React Native scenario modules (`mapSdk`, `mapView`, `mapCamera`, `originSearchQuery`, `destinationSearchQuery`, `originPlace`, `destinationPlace`, `routeOptions`, `plannedRoute`, `routeOverlay`), so a rename on one platform fails the gate, and it scans every tutorial chapter for the retired surface vocabulary (`mapSurface`, `MapSurfaceView`, `ANativeWindow`, `JNI shim`, `attach/resize/detach`). The chapters are clean and all gates plus the 113-test browser suite pass.

### Added (round 62 — the wasm canvas surface joins the contract-sync gate)

- `contract_sync.mjs` now compares the wasm canvas `MapView` against the UDL: every member it serves is either a contract `MapView` member (camel↔snake) or a documented canvas-only helper, the overlay and location-indicator members must all be present, and the absent lifecycle/vector-source members are recorded as the audit's known gap rather than drift. The wasm attribute parser now reads `js_name` through `getter`/`setter` modifiers, so the canvas' `sdkCompiledVersion` getter is compared under its JavaScript name. Contract sync, the provider-name guard, the parity floor and UX parity pass; the browser suite is 113/113.

### Added (round 61 — event-driven navigation on the wasm seam)

- The wasm `Navigator` now models the contract's foreign `NavigationListener` topics over wasm-bindgen: `addNavigationListener(callback)` receives `instruction` events for every dispatched fix, `phase` events naming the native transition (`offRouteEntered`, `offRouteCleared`, `arrived`, `rerouteApplied`), `deviation` entry/update/clear events, `speech` events carrying the contract's speech text, and the wasm-only `stop` event; `removeNavigationListener(id)` unregisters. Every event carries the same plain trip snapshot `currentTripState` answers, listener exceptions never disturb the trip, and the class diffs against the last emitted snapshot so a consumer can drive the card from events instead of polling. `contract_sync` documents the two wasm-only members as the event bridge. A Playwright case drives a fixture route through on-route and off-route fixes and asserts the topic sequence, the transition name, the deviation payload and the speech text; the browser suite is 113/113.

### Added (round 58 — voice guidance on the web reference app)

- The wasm `Navigator` snapshot now carries `speechText` on the active and upcoming maneuvers, taken from the contract's composed speech variant, and the web navigation card gained the canonical Voice guidance control. While it is on, the page speaks each new speech text through the browser's speech engine, once per instruction change, cancelling on disable and on idle, and it disables itself with an honest title when no engine exists — it never reuses the banner sentence as speech. A Playwright case stubs the speech engine and proves the spoken text equals the snapshot's speech channel and that disabling cancels. `navigation.voice_toggle` left the web copy-omission list (web mirrors 40 of 49). The only remaining canonical web role, `route.edit_action`, is recorded as a v1 layout exemption (both endpoints are edited in place), closing the owner's web v2-flow ruling within the existing layout.

### Added (round 57 — demo plans replay their recorded traces in the browser)

- The web demo's Advanced panel gains a **Demo plans** section fed by the shared artifact's `demo_plans` rows, with the canonical `plans.heading` label (which leaves the web copy-omission list, so the browser now mirrors 39 of 49 rows). Selecting a plan fetches its committed trace from a new same-origin allow-listed `/navigation-traces/{scenario}.json` route (derived from the artifact, never a hardcoded path; the docs-site staging copies the same files and fails the build when one is missing), draws the recorded route and A/B endpoint pins through the existing overlay composer, frames it with the same fitted camera the planner uses, marks the chip and states the plan's own description.
- Starting the simulated trip replays the selected trace's own fixes through the wasm `Navigator` at the recorded cadence scaled by the existing speed choices — per-sample accuracy, course from consecutive fixes — mirroring the native `SimulatedTripDriver`. The navigation card follows each snapshot: the clean walk reaches Arrived, the deviation-and-recovery trace raises the off-route alert and recovers, and the deviation-and-reroute trace applies its recorded replacement route at the first off-route judgment. Recorded traces replay verbatim, so the route-position seek row is hidden for their duration (the native `isSeekable` rule); pause, resume and speed stay. Planning a route supersedes the plan and Clear plan returns to idle. A new `demo_plans.spec.mjs` covers the picker, route framing, arrival, the off-route alert and the applied reroute; the browser suite is 109/109 and the docs-site staging gate copies the three traces byte-identically.

### Added (round 56 — a headless coverage proof for the SDF text edge)

- The golden harness now measures the SDF edge instead of trusting the golden comparison alone. A test renders one white `I` on the clear colour with no tiles and no halo at 36 and 48 device pixels, computes per-pixel coverage from the frame's own background and text luminance, and asserts a stroke interior reaches ≥0.98 coverage while the transition beside the widest interior run spans at most 2 sampled pixels (the 0.84-device-pixel half-band). Measured: 50 full pixels with 0/0 edge runs at 36 px, 123 full pixels with 2/1 edge runs at 48 px. It skips on a host without a graphics adapter, like the other goldens; all five golden tests pass on Metal. The browser half renders the same isolated `I` through the real wasm canvas at 1× and 2×, reads the presented frame back through a 2D canvas and applies the same math: both ratios keep the stroke interior at ≥0.98 coverage and each edge at ≤2 device pixels while the declared point size stays 48. M30 is complete.

### Added (round 55 — distance matrix and traffic in the React Native curated API)

- The curated `MapsSdk` proxy now exposes `calculateDistanceMatrix`, `fetchTrafficIncidents` and `fetchTrafficFlow`, answering the generated `DistanceMatrix`, `TrafficIncident` and `TrafficFlowData` records. The Android runtime/module and the iOS runtime/ObjC module implement the bridge calls through the contract's `MapSdk` members, with the JSON layer encoding enum ordinals and optional fields to the TypeScript shapes. A jest case drives the real proxy against a fake native module and pins the arguments and decoded records (78/78 app-shell tests); tsc is clean and the RN Android module compiles. The RN iOS workspace build remains blocked before this code by the environment's React-Core-prebuilt `cassert` failure; both changed Swift files parse cleanly.

### Added (round 54 — the ETA prefix on the web navigation card)

- The web guidance card now carries the canonical `ETA` row the native drawers show, rendered as a local 24-hour arrival clock from the live snapshot's remaining duration (`—` while idle). The `navigation.eta_prefix` string moved out of the web copy-omission list, so the browser mirrors 38 of 49 canonical rows. The navigation demo asserts both the idle placeholder and a real arrival during guidance; the browser suite is 105/105.

### Added (round 53 — camera presets animate on the web like the native shells)

- The web demo's camera preset chips now ease every pose component — center, zoom, bearing and pitch — over the shared move duration instead of jumping, matching the native `animateCamera` behaviour. The bearing takes the shortest angular path and re-wraps into `[0, 360)`; reduced-motion preferences collapse the move to the next frame, and the chip selection stays immediate. A new Playwright case proves the click does not assign the pose synchronously and that it settles on the shared artifact's preset pose; the existing preset case now waits for the settled pose. The browser suite is 105/105.

### Added (round 52 — the full route-options object on the wasm seam)

- `calculateRouteWithOptions` brings the contract's `RouteOptions` to the browser: transportation mode, strategy, unit system, the four avoid flags and an optional departure instant are read from one plain object with the contract defaults, and unknown enum names, non-objects and invalid timestamps are refused before the provider call. Both route exports share one answer path, so the shape cannot drift. The TypeScript mirror gains `RouteOptionsObject` and `contract_sync` documents the wasm-only alias. A new live-provider case proves the options reach the request: the imperial choice converts the fixture's 1.7 miles to 2735.8848 m while the metric case stays 1700 m; the spec is 13/13.

### Added (round 51 — traffic incidents and flow on the wasm seam)

- The browser seam now reaches the contract's traffic surface. `fetchTrafficIncidents` and `fetchTrafficFlow` validate the bounding box, call the active provider's traffic traits (the same implementations the native targets register), and answer plain rows: incidents carry identifier, type and severity names, description and start/end coordinates; flow segments carry their shape, nullable speeds and congestion-level name. The shared registry gained `traffic_incidents` and `traffic_flow` fixture routes to the committed Denver and New York payloads (root and iOS copies), `contract_sync` documents both wasm-only aliases, and the TypeScript mirror gains `TrafficIncidentRow`/`TrafficFlowRow`. A new live-provider case pins the exact fixture facts and the boundary refusal; the spec is 12/12.

### Added (round 50 — distance matrix on the wasm seam)

- The browser seam now reaches the contract's distance-matrix surface. `calculateDistanceMatrix` takes flat `[latitude, longitude, …]` origin and destination arrays plus the transportation mode, validates every pair and the mode before asking the provider, and answers the contract's `{originIndex, destinationIndex, distanceMeters, durationSeconds}` rows through the active provider's `DistanceMatrixProvider` — the same implementation the native targets register. The shared registry gained a POST `distance_matrix` route to the committed three-location unit-`k` fixture (both the root and iOS copies), `contract_sync` documents the wasm-only alias, and the TypeScript mirror gains `DistanceMatrixEntry`. A new live-provider case asserts the fixture's exact cells and the mode refusal; the spec is 11/11.

### Added (round 49 — the offline-maps card reaches the browser reference app)

- The web demo's Advanced panel now carries the v2 offline-maps card. Its action fetches the committed compiled routing-grid archive from the same-origin allow-listed route, plans the recorded demo request through the contract's `offlineRouteFromArchiveBytes` free function, draws the returned polyline over any base layer, frames it through the same fitted camera the live planner uses, and states the canonical title and honesty note plus the route's distance and duration. A missing or refused archive is reported verbatim in the card and the shared error strip; nothing is invented. The four `offline.*` canonical strings moved out of the web parity omission list, so the browser now mirrors 37 of 49 consumer-copy rows. Two Playwright cases cover the planned route and the refusal; the browser suite is 101/101.

### Added (round 48 — a platform-parity floor in the gate set)

- `platform/web/tests/platform_parity_gate.mjs` makes the audit's findings fail-closed. It runs in Playwright's `globalSetup` beside contract-sync and the provider-name guard, and standalone under `node --test`. It fails when: a native target loses its UniFFI binding-generation invocation (Kotlin and Swift through `uniffi-bindgen`, React Native through `ubrn` JSI + Turbo Module); the UDL loses an interface the reference apps consume; the wasm façade drops an export below the capability floor it has reached (camera, SDK root, providers, geocoding, search, routing, tiles, navigation, diagnostics, vector geometry); Android, iOS, React Native or web loses the Light/Dark/Auto model with Auto as the default; the web map loses its keyboard/screen-reader markup or the mobile page its ARIA tabs pattern; the iOS registry mirror drifts from `platform/providers.json`; or a fixture path the registry names is not committed. It is a source-text floor, not a behaviour test — the platform suites still prove behaviour.

### Added (round 47 — live search on the wasm seam)

- The browser seam now reaches the contract's search-place surface: `suggestPlaces`, `searchPlaces` and their boundary-taking forms answer through the active provider's `SearchPlaceProvider` (the same implementation the native targets register), mapped to the shared place-row shape that `forwardGeocode` already returned. Boundary corners are validated before the provider is called, and inverted boxes are refused verbatim. The shared registry gained a `search` fixture route pointing at the committed Denver prediction, so the tests stay offline; `contract_sync` documents the four wasm-only aliases and the TypeScript mirror gains `PlaceRow`. Two new Playwright cases cover the rows and the boundary refusal; the live-provider spec is 10/10.

### Added (round 46 — four-platform parity audit, a three-way theme everywhere and an accessible web demo)

- A four-platform capability and UX audit (`docs/history/FourPlatformParityAudit.md`) inventories the frozen contract (15 interfaces, 4 foreign traits, 47 dictionaries, 23 enums, 82 interface members, 12 errors) and scores every capability area per platform with file:line evidence. Android and iOS are at binding parity; React Native has generated-binding parity but a narrower curated API; the hand-written wasm façade reaches about 34 of 90 callable members and none of the `MapSdk` provider surface. It also records the owner rulings that followed: parity closes through the synced wasm façade (no wasm2 migration), the web reference app reaches the v2 flow, labels stay renderer-internal, and React Native JavaScript gets the curated provider/navigation surface (PRD §6 items 19–22).
- Android and React Native now implement the three-way Light/Dark/Auto theme with Auto the default, matching iOS and the web (PRD §6 item 9). Android adds `ThemeChoice`/`ThemeChoiceStore` (device-remembered, resolved against the live system scheme) and a cycling appearance button labelled with the next choice; React Native adds the same pure model, `useColorScheme()` resolution and a `loadThemeChoice`/`storeThemeChoice` bridge pair over SharedPreferences and `NSUserDefaults`. Jest pins the cycle, labels and resolution; the Android instrumentation case pins the Auto default after clearing the remembered choice.
- The web demo is usable without a pointer. The map surface is a labelled `role="application"` focus stop: arrow keys pan 100 CSS pixels, `+`/`-` zoom one level, Shift with the arrows rotates and tilts, and Home resets north and flat — every move crossing the existing wasm camera seam and announcing its result in a polite live region. A skip link reaches the control sheet, focus rings are visible, reduced-motion preferences disable eased moves, the 2D/3D control exposes `aria-pressed`, and the hosted mobile demo follows the ARIA tabs pattern (roving tabindex, arrow keys, tabpanel labelling, new-tab hint, focus handoff and Stop-returns-focus). New Playwright suites cover 11 cases; the full browser suite is 97/97.
- `docs/DocumentationPlatformParity.md` and `docs/ReferenceAppUX.md` no longer claim the wasm target has no turn-by-turn controller or provider surface; the live `Navigator` subset, forward geocoding, two-waypoint routing and offline routing are named, and the still-missing provider capabilities are listed precisely.

### Fixed (round 46 — native labels keep their point size)

- Android and iOS now draw vector labels and overlay screen sizes at the point size their styles declare. The committed styles use point/CSS sizes while the surface draws device pixels, so a 2× or 3× screen used to render text and line work smaller than designed. The contract gains the addition-only `MapView.set_device_pixel_ratio` (default 1.0; non-finite or non-positive values throw `InvalidRenderInput` and keep the previous ratio), and the surface draw multiplies every placed glyph size and halo width, and every route/traffic line width and marker/pin radius, by that ratio exactly once — including the vector base's own style-derived line work. Collision padding follows the scaled text metrics, and an idle-settled pump redraws when the ratio changes. Android reports `resources.displayMetrics.density`; iOS reports its capped drawable scale (drawable pixels per point) at attach, resize and after an SDK swap; the RN iOS widget copy carries the same call. The wasm canvas keeps its own `canvas.width / clientWidth` ratio and deliberately has no setter.
- Tests: the stored ratio scales the forwarded label layer's placed text size and halo, an invalid ratio is refused while the previous one stays in force, and `MapOverlay::scaled_to_device_pixel_ratio` multiplies each screen-space size. Device evidence: the focused iOS XCUITest and Android instrumentation case for the styled vector base pass, and the native-scale screenshots (`tmp/ios-native-density.png`, `tmp/android-native-density-1.png`) show crisp labels at their declared size.

### Fixed (round 45 — labels are crisp type, not fog)

- Vector labels now render the way MapLibre Native's `symbol_sdf` shader does. The glyph protocol's zero-distance contour is **0.75**, not 0.5: the old threshold dilated every glyph by two atlas pixels per side, closing counters and fattening strokes. The halo was a fixed lower threshold (0.32) that covered the whole 3-pixel field border — a white bloom around every label; the halo now moves the edge by its true width (`0.75 − halo_atlas_px / 8`, clamped to the border) and keeps the text quad's geometry. The anti-aliasing band is no longer a fixed ±0.05: it is `0.105 / devicePixelRatio / (textSize/24)`, i.e. 0.84 device pixels at every size and pixel ratio, as MapLibre computes it.
- The browser demo no longer has the compositor upscale its frame. On a Retina display the map canvases were 1× (CSS pixels) and every drawn pixel — text included — was blurred by the browser; they are now backed by device pixels while the layout stays in CSS pixels, and the wasm canvas scales label sizes, halos, overlay widths, marker radii, the location indicator and pan deltas by its actual `canvas.width / clientWidth` ratio. The scale bar asks the camera in CSS pixels so its length is unchanged. A new Playwright case renders at `deviceScaleFactor: 2` and pins the backing and CSS sizes; the label goldens were regenerated and reviewed.
- The full reasoning, quoted shader sources and fontnik/Navara comparisons live in `docs/research/text-rendering-quality.md`.

### Fixed (round 44 — the hosted demo can start its own session again)

- `/demo/mobile` no longer sends visitors into "This app may not be embedded on this website". The Appetize app page's own session request is same-origin, so its referer is `appetize.io` — a host the account's embed allow-list did not contain — and our link added `autoplay=true`, which made that request the first thing the page did. The uploader now keeps `appetize.io` in the allow-list beside the two documentation hosts, the session link opens the app page without autoplay (the page's own Start demo button is the click-to-start gate, so opening the tab alone never streams), and the panel copy says to press it. Reproducing the refusal (with and without our referrer, with and without autoplay) and the websocket `userError` frame behind it is recorded in `docs/research/hosted-mobile-demo-appetize.md`.

### Added (round 44 — a keyless start opens on the styled vector base)

- The Android, iOS and React Native reference apps now open keylessly on **OpenFreeMap Bright**, the registry's declared styled vector base, instead of the plain OpenStreetMap raster (owner ruling 2026-10-31, PRD §6 item 18). `platform/providers.json` gained a top-level `keyless_base_map` (`provider_key` plus committed `style_id`); each shell resolves it against the descriptors and installs the style over the credential-less raster source, which stays registered so the shipped surface can attach (the surface contract still requires a raster provider). A registry that omits the field keeps the raster default, an explicit base-map choice or the demo launch argument always wins, and an applied credential is never hidden under the default. The React Native shell gained its missing keyless start registration as part of the same change.
- The registry check now fails the build when `keyless_base_map` names a provider the registry does not declare, a provider without a vector template, a style that descriptor does not offer, or a style artifact the renderer cannot load.
- Device/browser evidence: the Android keyless `theKeylessStartOpensOnTheRegistrysDeclaredStyledBase` and the iOS `testTheKeylessStartOpensOnTheRegistrysDeclaredStyledBase` pass on the standing devices; the RN base-map unit suite pins the resolution (an incomplete or mismatched declaration resolves to nothing).

### Added (round 44 — a style filter can accept several values)

- A vector style layer's attribute filter now accepts several text spellings (`matches_any`) and a numeric equality (`number_equals`, matched through every numeric wire variant — `int`, `uint`, `sint`, `float`, `double`), not only one exact string. A filter with no accepted value matches nothing rather than everything, and a text value never satisfies a numeric filter. The three named styles keep their curated boundary layers: their upstream numeric `admin_level` filters paint nothing on the demo tile, which the research note had already measured. This closes M21's last schema item.

### Added (round 44 — the native tap contract is under test)

- The diagnostics sheet on Android, iOS and React Native now shows the session's confirmed map-tap count, and the suites pin §5.2's tap contract with real gestures: Espresso injects pointer events (`aSingleTapReportsOnceAndADoubleTapZoomsExactlyOneLevel` — one tap reports exactly one click and moves nothing; a double tap zooms exactly one level and reports no click of its own), the XCUITest mirrors it with `tap()`/`doubleTap()` against the same rendered count, and the RN jest case proves one forwarded native `mapClick` reaches one consumer call. This closes M20's last open item.

### Fixed (round 43 — the live demo draws its labels again)

- The published browser demo now paints vector place names and road names. Two production-only holes had hidden them: the page asked the site root for its glyph range (right on the dev server, a 404 under `/demo/`), and the docs staging never copied the range. The range is now resolved relative to the demo, staged at its served path with a byte-equality check, and a missing fixture fails the build instead of silently stripping every label. Verified on a staged tree served locally with live tiles: 32 place labels and 17 road names at the demo camera.
- Switching the base map from vector back to raster can no longer leave vector geometry painted over the imagery. A vector-base load that was still fetching when the visitor switched applied its result to the canvas after the switch had cleared it; the switch now cancels the load, and a browser case holds a gated load open to prove nothing is applied.

### Fixed (round 43 — a keyed start kept its provider and its tile cache)

- Applying a provider key no longer ends with the keyless OpenStreetMap raster base quietly replacing that provider's registration. The startup's keyless-base selection only checked "no base chosen yet", so after a credential installed the provider's tiles and disk cache it replaced them with the credential-less source — the map showed OSM raster and the offline card said "runs uncached". Both Android and iOS now leave a credentialed registration alone; the iOS keyed cache-accounting test and a new Android keyed Espresso case cover it.
- The keyed iOS UI-test command is corrected: `xcodebuild` does not forward the shell environment to the UI-test runner, so the documented bare `PROVIDER_API_KEY=…` skipped every keyed case without an error. The guide now uses the working `TEST_RUNNER_PROVIDER_API_KEY` form, verified on the simulator.

### Fixed (round 43 — every listed provider is offered where it can serve)

- The OpenStreetMap US provider was missing from the Android, iOS and React Native base-map menus: it publishes a vector tileset but declared no named style, and the native menus offer only descriptors with a style. It now offers **Carto Light** and **Carto Dark**, so the vector OSM base is selectable on every platform, and its raster sibling is named **OpenStreetMap Standard** so the classic keyless raster base cannot be mistaken for the vector one. The demo's default vector base prefers the Bright/Fiord/Positron family.
- A registry-consistency test now fails the build when a listed style id is not a committed style artifact, a vector descriptor offers no style, a tile template is not `{z}/{x}/{y}`, or a provider lacks a name, attribution or serving surface.

### Changed (round 43 — the hosted demo counts sessions per visitor)

- The hosted demo no longer locks a visitor out because of their network address. The page sends a random token it keeps in this browser, and the claim endpoint counts per token (two sessions a day) with the address as a backstop (four), instead of one session per address — which a VPN exit or a carrier NAT shares with everyone behind it. Four sessions a day are granted across all visitors, every resting answer states the month's remaining minutes, and `DEMO_OPERATOR_TOKEN` lets the owner watch while minutes remain without ever lifting the month's budget. The decision is a pure module with `node --test` coverage.
- The mobile demo's "← Docs" link now shares the masthead row with the page title, so it can never be crowded by the platform tabs; measured clean at phone, tablet and desktop widths.

### Added (round 43 — live guidance on the wasm seam)

- The browser seam now carries the `Navigator`: `startNavigation`, `updateLocation`, `applyReroute`, `stopNavigation` and `currentTripState` drive the same `map-navigation` reducer the native controllers use, with the same default policies, and answer a presentation-ready snapshot (phase, distance phrase, provider and composed banner text, upcoming maneuver, progress, deviation, snapped coordinate). The frozen contract is untouched; the listener, speech and reroute-recalculation members remain native until the seam has a foreign-listener contract.
- The demo's "Simulated navigation" card is now live: the fix that moves the follow camera is delivered to the navigator, and every row of the card is the returned snapshot. The fixed sample values and the "layout preview, not live guidance" notice are gone; before a trip the card says what to do, and a fix placed off the route raises the canonical deviation sentence. The banner keeps the native two-line shape (distance phrase plus the provider's own sentence).
- Seeking replays the route into the navigator on release, so the guidance card catches up with the camera and reports Arrived at the destination instead of freezing on the first step (the reducer is forward-only; a single teleported fix cannot advance the steps between).
- The WebAssembly tutorial tab now walks the live example (start the navigator, drive it with the route walker, read the snapshot), and browser cases cover the live panel, the deviation path and the seek-to-arrival (suite 85/85).

### Added (round 42 — the offline card plans a route in the apps)

- Every reference application's offline card now plans a route from the bundled test graph: one control, a committed archive, and the result drawn as the trip's route with its distance and duration, labelled as a recorded test graph rather than live data. Android, iOS and React Native all do it keyless, and a refused archive reports through the existing error strip.

### Added (round 42 — offline routing reaches every platform)

- `offline_route_from_archive_bytes` is now part of the contract: an archive's bytes and two coordinates in, a route out, on Android, iOS, React Native and the browser alike. Before this the capability existed only as a wasm free function, so the README's offline claim was true only in the browser. One implementation in `map-core` answers both the contract and the wasm export, so they cannot drift; a corrupt archive reports `OfflineRoutingFailure` rather than a coordinate error.

### Changed (round 42 — failures say what actually failed)

- Two new contract error variants, `CacheOperationFailure` and `OfflineRoutingFailure`, replace the old folding of those failures into `InvalidCoordinateInput` — a cache store that cannot be opened or a truncated offline graph is no longer reported to a Kotlin, Swift, React Native or browser consumer as "bad coordinates". The mapping is now exhaustive at the binding boundary, so a future core failure has to choose its surface deliberately.

### Added (round 42 — scrub the simulated trip)

- A simulated trip can now be **scrubbed**: a seek control in the trip drawer moves the position along the route on every platform, through the same walker that drives the trip, so the camera, the location indicator, the remaining distance and the guidance state all follow. Seeking to the end arrives; seeking back out of an arrival resumes navigation from there; a paused trip stays paused.

### Added (round 41 — one vocabulary across the reference apps)

- The reference applications now draw their user-visible copy from one shared source, `ui_labels` in `platform/reference-tokens.json`, instead of restating it in five places. `docs/ReferenceAppUX.md` §2.5 states the rule — a consumer string exists if and only if it is in the artifact — and `scripts/check_ux_parity.mjs` now reads each platform's mirror, including both React Native native hosts and the browser page's own markup, and reports what diverges or is missing.
- The React Native and web surfaces are migrated: the browser demo takes every label from `platform/web/ui_strings.mjs` (with the markup carrying a no-script default), and the React Native shell lost nine dead strings and gained the canonical wording.
- The peek's action row is now the route's Start on every surface, so "Plan a route" and "Start demo navigation" no longer describe the same button on two platforms.

### Added (round 41 — maps that say where you are)

- Vector maps now draw **place labels**. The style's symbol layers name the text, the decoded tiles supply the features, and a glyph atlas supplies the shapes; a collision pass keeps labels from overlapping and repeated names are drawn once. It runs in all three renderer backends and uploads the atlas once per change, not per frame. The browser demo shows Sacramento, Downtown, Midtown and its neighbours on the OpenStreetMap vector base; a headless golden pins the frame.
- Vector maps now draw **road names** as well: each named street's text follows its line, rotated to the road's bearing, skipping a block too short to hold it, with the style's three named styles gaining a layer per road class. The place-name collision pass covers both kinds.
- `map-glyphs` reads Mapbox glyph protobufs, caches ranges under a byte ceiling and packs an `R8Unorm` shelf atlas. Vector providers declare a `glyphs_url_template` in the registry (both committed descriptors verified live).

### Changed (round 41 — the demo trips drive real rivers of streets, not a zigzag)

- The simulated trips the reference applications replay now follow **recorded routing geometry** instead of a synthetic street grid. The three committed traces walk the provider-recorded pedestrian routes between the demo places — 43 shape entries over 1 436 m for the clean walk, where the old fixture had six points over 800 m of made-up grid — with the fixes still generated deterministically along them. A drift gate fails if a trace stops matching the recorded route it claims.

### Added (round 41 — simulated trips walk the planned route)

- `RouteSimulator` is the contract's simulation surface: build one from a route and a pace, then ask for the fix at a travelled distance or at an elapsed time. It walks the route's own polyline with the correct segment bearing, collapses duplicate geometry, handles the antimeridian and clamps at both ends, so a trip can never leave its route. Kotlin, Swift, the React Native Turbo Module and the WebAssembly package all get it from the one contract.
- Vector styles can now declare **symbol layers**: text-field fallbacks, font stack, size zoom stops, text and halo colours, placement, priority and allow-overlap. The named styles carry `place`/`poi` layers ready for the glyph pipeline; they paint no labels yet.

### Added (round 41 — a concepts primer for the guide)

- A new **Map concepts** chapter explains coordinates, the world square, tiles, continuous zoom, the camera and overlays using the SDK's own vocabulary, so a developer who has never used a map SDK can read the task guides without gaps.

### Performance (round 41 — overlays are tessellated once, not every frame)

- Overlay geometry (routes, polygons, markers, traffic) is now tessellated in world space and cached by content revision; a frame only rewrites a 128-byte uniform and issues one instanced draw. The screen-space width expansion, joins, markers and the antimeridian world-copy behaviour moved into the vertex shader and the fragment stage's window clip. Measured on the committed browser harness at 1920×1080 with 47,149 overlay triangles: `renderFrame` 6.0 → 0.1 ms p50 (p95 6.2 → 0.2), overlay geometry 5.1 → 0 ms. Native goldens are unchanged and the browser golden compare passes with identical triangle counts.

### Fixed (round 41 — simulated trips follow the plan)

- A simulated trip now walks the route the visitor planned, distance-parameterised along its own polyline, instead of a fixed demo line; the final fix lands on the destination and arrival keeps it on screen. The web demo's base-map section now states that vector styles draw land, water and roads without labels yet, matching the native shells.

### Added (round 41 — hosted builds carry a dedicated demo credential)

- A hosted-demo build can carry its own provider credential. `scripts/inject-hosted-demo-credential.sh` writes it into the Android app's assets (and the React Native shell's) on the build machine from a repository secret; the iOS app reads it from `Info.plist` expanded from the build setting. It is applied through the same runtime registration path as the settings surface — never persisted, never logged — and every build made without it stays keyless. `docs/ProviderCredentialsAndBrowserProviders.md` §6.1 records the bounded exception to the "no key in a build" rule.
- The React Native reference application is published to the hosted demo for the first time, as a release build with its JS bundle embedded.

### Fixed (round 41)

- Published package metadata points at the project's real repository again: the npm `homepage`/`repository` fields, the Android POM's url, license and scm block, and the generated release README all named a repository that does not exist.

- The hosted demo's claim endpoint answers CORS for every documented hostname the site is served from, so the canonical address could claim a session before.
- The Appetize referrer restriction is applied as a JSON body, because the multipart upload silently drops the array field, and the uploader now reads the app back and fails when the list did not stick.
- Session opens in its own tab: the Appetize Free plan reports `isEmbeddable: false` and offers no per-app switch, so `/demo/mobile` links out after a granted claim instead of framing the app.
- The React Native shell builds with the renamed package again; stale autolinking output under `android/build/generated/` was surviving `:app:clean` and pinning the old application id.
- The landing page no longer mints an `#android` fragment on first load.

### Changed (round 41 — the published site is written for SDK users)

- The contributor-conventions chapter is gone, and the landing, performance, screenshots, mobile-demo and generated API-reference/contract intros no longer describe the build pipeline, gates or provenance.
- The published changelog describes product behaviour rather than the release pipeline and CI.
- The tutorials compile against the shipped surfaces again: `MapsSdk.create()` on React Native, the default-export wasm init, `setCamera` with a five-value pose where the guide called `draw`. The snippet checker now type-checks the React Native fragments against the package's own `MapView` handle, which is what hid both defects.

### Changed (round 40 — the hosted demo runs on a free Upstash store)

- The claim endpoint is connected to the account's **Free** Upstash for Redis
  store (`upstash-kv-citrine-elephant`, `iad1`) rather than a paid one. Vercel's
  store-creation API offers no free plan (`No free billing plans are available
  for product 'upstash-kv'`), so the first attempt created a Pay-As-You-Go store;
  it was disconnected and deleted, leaving no subscription and no payment method
  behind the demo. The store is shared with `mapquest-demo-studio` — supported,
  and safe because every key the endpoint writes carries the `demo:` prefix.

### Changed (round 40 — the Kotlin package is `com.angoratek.rusticmaps`)

- The Android SDK, both Android applications, the JVM smoke module and the RN
  Android module moved from `com.angoratek.mapsdk` to
  `com.angoratek.rusticmaps` (owner ruling, finishing the product rename on
  the platform), together with the Gradle namespaces and application ids, the
  JNI binding the native window bridge is bound by
  (`Java_com_angoratek_rusticmaps_NativeWindowBridge_*` in
  `map-native-core`), the CI path filters, the RN shell's iOS bundle id and
  the tutorial's Kotlin import.
- The publish scrub now allows the released Kotlin package, which is what had
  been keeping the documentation site from publishing since round 38: the
  tutorial's own import was the only denied identifier in the tree.

### Added (round 40 — a committed browser frame-budget harness)

- **`platform/web/perf/run-frame-budget.mjs`** plus
  `docs/BrowserRenderPerformance.md`: a frozen scenario (1920 × 1080 at device
  scale 1, vector or raster base, a settle, a warm-up and a 4 s out-and-back
  600 px/s pan that never leaves the recorded fixtures) driven through the
  page's own frame path, reporting p50/p95/p99 of the render frame and the
  animation-frame interval, with the browser build and `GPUAdapter.info` in
  the report. It runs the host's Chromium with hardware WebGPU and stays out
  of `npm test`, because the Playwright suite renders under SwiftShader and
  its absolute frame times say nothing about a GPU.
- **Frame stage spans on the statistics object.** `renderFrame` now reports
  `stageMilliseconds` (`tilePipeline`, `overlayGeometry`, `encode`, `submit`),
  read from `performance.now()` inside the frame, so a slow frame is
  attributable from the page instead of guessed at from outside. The demo's
  diagnostics show them as frame facts.

### Fixed (round 40 — the iOS view defers a mid-frame base-map change too)

- `MapNativeCoreUI.MapView` queues a `setVectorTileSource`/`clearVectorTileSource`
  that arrives while a frame draw is in flight and applies it on the next pump
  tick, matching the Android view's round-39 fix. The SDK still refuses to move
  the scheduler under a running frame; the refusal no longer reaches the caller
  as a failed tap.

### Changed (round 40 — no fetch or duplicate decode inside a presenting frame)

- A browser render frame **starts** its missing tiles' fetches and returns
  instead of awaiting up to 32 sequential round trips while the draw state is
  borrowed; a landed payload dispatches `map-tile-arrived` on the canvas and
  the page draws once more. A page rendering on demand should listen for that
  event (the reference playground does).
- A tile payload is **decoded once**. The staging pass used to decode it to
  validate it and throw the result away, and the upload decoded it again; the
  staging decode is now kept — fingerprinted against the encoded bytes, so a
  re-fetched payload never serves a stale decode — and the upload consumes it.

### Changed (round 40 — overlay geometry costs 0.7 ms less per frame)

- `filled_polygon_triangles` no longer clips every triangle against all three
  world-copy windows: a triangle whose x-extent misses a window is rejected by
  two comparisons (exact — it would clip to fewer than three corners), the
  surviving clips run into fixed arrays instead of allocating two `Vec`s per
  triangle per window, and a triangle wholly inside one window skips both clip
  passes and projects its own corners. Drawn vertices are unchanged; the
  committed golden fixtures and the browser golden compare prove it. Measured
  on the harness scenario: `renderFrame` 6.6 → 5.9 ms p50, overlay tessellation
  5.6 → 5.0 ms.

### Fixed (round 39 — a base-map selection no longer fails mid-frame)

- The Android `MapView` queues a vector base-map install or clear that arrives
  while a frame draw is in flight and applies it on the next pump tick. The SDK
  still refuses the swap under a running frame — the scheduler must not move
  while it resolves — but the refusal no longer reaches the user as a failed
  tap. The live keyless base map made this reachable on every launch; the
  Espresso vector-base case caught it.

### Changed (round 39 — the reference apps open on a keyless base map)

- Both reference applications select the registry's credential-less raster
  source at startup when no base map is already chosen
  (`selectKeylessBaseMapWhenUncredentialed`, provider-neutral and
  registry-driven), and accept a `demoBaseMap` intent extra on Android and a
  `-DemoBaseMap <key>` launch argument on iOS. A keyless run — the hosted demo,
  a first launch — now opens on imagery instead of an empty surface.
- The first hosted builds are published to Appetize (version 1); the public
  ids live in `platform/demo-builds.json`.

### Added (round 39 — route planning on the React Native surface)

- **`MapsSdk.calculateRoute(waypoints, routeOptions)`** on the React Native
  package: the JavaScript surface exposed geocoding and search but no routing,
  so a JS consumer could not plan a route at all. Both native halves gained the
  method and a `Route` serializer matching the generated TypeScript type, and
  the package re-exports `Route`/`RouteOptions`.
- **`platform/rn/app-shell/src/walkthroughScenario.ts`**: the React Native half
  of the tutorial's worked example, with the same identifiers as the Android
  and iOS modules, covered by three jest cases (camera first, both searches,
  ordered waypoints, overlay geometry, and a missing place returning null
  without planning).

### Added (round 39 — the hosted mobile demo)

- **`platform/web/mobile-demo/`**: the `/demo/mobile` page. Nothing streams
  until Start; a granted claim injects the Appetize frame, a countdown runs
  from the granted session length, Stop and page-hide end the session, and the
  resting state prints the endpoint's own sentence rather than a promise.
- **`platform/demo-claim-endpoint/`**: the Vercel function over Vercel KV that
  enforces the Free plan's thirty streamed minutes a month — three-minute
  sessions, two grants a day across visitors, one per client address, a
  `DEMO_KILL_SWITCH`, and every grant reserving the whole session before it
  answers.
- **`scripts/upload-demo-builds.mjs`** and **`platform/demo-builds.json`**:
  in-place Appetize uploads that keep public build ids stable, with the
  session limits and the docs-site referrer restriction applied at upload time,
  and a public manifest holding no secret.
- **`.github/workflows/hosted-demo.yml`**: builds the Android APK and the iOS
  simulator `.app` on a release publication or on demand, uploads them when
  `APPETIZE_TOKEN` is present and reports the skip when it is not.
- **`site/book-src/mobile-demo.md`**: the honest budget statement, what runs in
  a session, and the maintainer path.

### Added (round 38 — tutorial code generated from tested source)

- **`scripts/generate-doc-snippets.mjs`**: tutorial chapters are now generated
  from marked regions of the reference applications. A source region is
  delimited by `// docs-snippet: begin <chapter>/<step>` comments; a chapter
  block by `<!-- docs-snippet: begin <id> <platform> -->` comments. `--check`
  fails on drift, on a chapter naming a region no source marks, and on a
  marked region no chapter publishes; `--self-test` proves all three.
  `docs-snippets.yml` runs both beside the compile check.
- **`docs/reference/DocumentationSnippets.md`** and **`docs/doc-snippets.json`**: the
  contract (marker format, the worked example's fixed identifiers) and the
  platform roots the generator scans.
- **`WalkthroughScenario` on Android and iOS**: the worked example —
  open the map, search the two plan fields, plan the route, draw it — runs in
  both reference applications, driven by the `demoWalkthrough` intent extra
  and the `-DemoWalkthrough YES` launch argument. The chapters' code blocks
  are generated from it, so the guides describe code that ships.

### Changed (round 38 — the guides leave the retired surface)

- **`first-map.md`**: the Android tab no longer teaches a JNI shim, an
  `ANativeWindow*` handoff, `mapSurface.attach/resize/detach` or a draw loop;
  the iOS tab no longer teaches a surface handle and a display-link pump. Both
  show the shipped `MapView` (the package creates it, owns the surface and the
  frame pump) and a generated open-the-map block.
- **`search-and-routing.md`**: the two native tabs are generated from the
  worked example and plan with the pedestrian option set the reference
  applications actually use, not an automobile request the demo never sends.
- **`navigation-simulation.md`**: the follow-camera and overlay steps use the
  shipped view's `setCamera`, `setOverlays` and `setLocationIndicator` instead
  of the deleted `mapSurface.draw`.
- **`check-doc-snippets.mjs`** carries the worked example's ambient
  identifiers and a mirrored stub for the shipped `MapNativeCoreUI` view, so
  the Swift type-check covers the new blocks (59 snippets PASS).

### Added (round 37 — the shared vector base pipeline and the native styled base)

- **`map-rendering::vector_base::VectorBaseScheduler`** — one Rust vector base
  pipeline for every target: fetches through the `VectorTileProvider` trait,
  decodes each tile once (gzip unwrapped), caches decoded tiles and styled
  geometry keyed by tile address, style revision and a quantized style-zoom
  bucket. A style change restyles cached tiles; crossing a width stop
  restyles at the new bucket — both without refetching. One tile's failure is
  counted, never fatal to the frame.
- **`vector_style` evaluates paint at an explicit zoom**:
  `overlay_primitives_for_vector_tile_at_zoom` and
  `vector_tile_overlays_at_zoom` style a cached tile at the camera's zoom
  (the existing functions delegate with the tile's own zoom, unchanged).
- **Contract (additive)**: `UrlTemplateVectorTileSource` and
  `VectorTileSourceStatus` dictionaries plus `MapView.set_vector_tile_source`,
  `clear_vector_tile_source` and `vector_tile_source_status`. The native
  `MapView` resolves the styled base inside the draw (scheduler taken out of
  its slot, never a lock across an await), draws it over the raster tiles and
  beneath the app's overlays, and `is_frame_settled` now accounts for the
  vector source and its failed tiles.
- **The URL-template vector provider refuses an empty source name and an
  inverted zoom range** at construction, so `MapView`'s zoom clamping can
  never see a malformed span.
- **Android, iOS and RN offer the styles**: the registry decoders carry the
  vector template and `vector_styles`; the Base map section names the serving
  base and opens a menu with a **Raster sources** and a **Vector styles**
  group (Bright, Fiord, Positron). Attribution follows the active style, the
  sheet states *labels are not rendered yet*, a raster selection clears the
  vector base, and the shipped `MapView` re-applies a stored source across a
  native-view rebuild. RN's `MapView` handle mirrors all three calls through
  both native modules.
- **Tests**: seven `map-rendering` scheduler tests, seven native contract
  tests (`vector_tile_source.rs`), two provider-construction tests, RN jest
  cases for the vector choices/source record and the handle contract, and an
  Espresso case.

### Added (round 36 — trip-planning parity, canvas interaction, vector cartography research)

- **Trip planning on the web is the two-field flow every route planner offers**
  (§4.1): origin and destination fields over one result list keyed by the
  active field, demo suggestion chips filling the focused field, and an
  explicit **Plan route** action that stays disabled with a reason naming the
  missing field. Planning builds the polyline from the route's own step
  shapes, draws green-A/red-B teardrop pins at the endpoints, and frames the
  route's bounding box with insets that clear the control sheet. Keyless
  planning answers any ordered pair of the demo places from the artifact's new
  `demo_routes`; a keyed session plans live through the same view.
- **`MapOverlayKind.PointPins`** (contract, additive): a teardrop pin anchored
  at its tip with a hollow head center, reusing the point-marker payload
  fields. `map-rendering` emits the pin as the annulus between the head hole
  and a star-shaped teardrop outline (a plain triangle list — the overlay
  pipeline still has no erase pass); the wasm canvas accepts
  `{kind: "pointPins"}` and the vector-layer serializer round-trips it; the
  Kotlin and Swift bindings were regenerated.
- **`MapCamera.fittingBounds`** and the contract's four camera attribute
  getters (`center`, `zoomLevel`, `bearingDegrees`, `pitchDegrees`) now exist
  on the wasm camera, so the browser can frame a planned route through the
  same math the native shells use and read the fitted pose back.
- **`demo_routes`** in `platform/reference-scenes.json`: all 20 ordered pairs
  of the five demo places, recorded live through the MapQuest provider with
  the shared options (pedestrian, fastest) by
  `crates/mapquest-provider/examples/record_demo_routes.rs`, keyless, with the
  iOS mirror byte-identical.
- **The native shells plan explicitly too**: Android `planRouteFromPlanFields`
  + a Compose Plan route button and reason strip, iOS `planRouteFromFields`
  with the `plan-route-button` control, and the RN bridge's `planRoute` on
  both app-shell platforms. Assignment never plans by itself, the default
  origin is gone, and each shell draws the same A/B pins from the route's
  endpoints. Route options unify on pedestrian + fastest.
- **Canvas interaction** (M20): the vector base and the page's own overlays now
  share one drawn list (base first), so a vector base no longer clears or
  disables the scene toggles; a double click (web, 350 ms / 24 px bounds) and a
  double tap (Android, iOS, RN) zoom one level around the pointer through the
  contract's focal zoom member, with the single tap reported exactly once.
- **Three named vector styles and an Auto theme**: the vector base offers
  **Bright** (light), **Fiord** (dark) and **Positron** (quiet light), three
  curated transcriptions of the `hyperknot/openfreemap-styles` family carried
  as committed artifacts with their upstream backgrounds (the canvas clears to
  the active style's own color), offered per descriptor in the registry and
  picked from a dropdown in the Base map section. Layers carry MapLibre-style
  `min_zoom`/`max_zoom` and `width_zoom_stops` evaluated at the tile's zoom
  (linear interpolation), so zooming changes road widths and visibility rather
  than only scaling the same paint. The theme control is now a
  three-way **Light / Dark / Auto** choice with **Auto the default**: it
  follows `prefers-color-scheme` live and remembers an explicit choice on the
  device, overturning the earlier dark-default / never-read-the-OS rule.
- **Research** (`docs/research/`): the OpenFreeMap style measurements (Bright /
  Fiord / Positron chosen; Dark Matter measured near-black-on-black), the
  browser render-performance diagnosis (per-frame re-tessellation, buffer
  recreation, double PNG decode, serial in-frame fetches), and the Appetize
  hosting economics for the Free plan.

### Fixed (round 36)

- **A route planned while the renderer was still starting was lost**: the
  overlay list now composes again the moment the GPU surface comes up, instead
  of waiting for the next unrelated overlay change.
- **The recorded (snake-case JSON) and live (camel-case wasm) route records
  read the same summary fields**, so a keyless route's distance and duration
  no longer render as `NaN`.

### Fixed (round 35)

- **The Android map band keeps the map's inline gutter on every width**, so
  the scale bar and the attribution control align with the zoom cluster
  instead of touching the viewport edge on phones (the desktop dock already
  applied it).
- **The Android attribution stays at the end edge when the scale bar renders
  nothing** (it emits no view before the first ground answer): the band row's
  `SpaceBetween` collapsed to the start edge with a single child, so a
  weighted spacer now pins the attribution and the scale bar keeps the start
  edge.

### Added (round 35 — provider-neutral base maps)

- **`url-template-raster-provider`**: a provider-neutral
  `RasterTileProvider` over any web-mercator XYZ raster tileset that
  publishes a `{z}/{x}/{y}` URL template, with the source's declared image
  format, resolution, zoom span and `User-Agent`, paced and retried through
  the shared provider plumbing. The template validator and the tile-source
  status vocabulary stay single-sourced in `url-template-vector-provider`,
  which now phrases them contract-neutrally.
- **`MapSdk.register_url_template_raster_tile_source`** (contract, additive):
  registers such a source from an application's shared provider registry
  descriptor. It replaces the raster-tile slot only, drops any installed
  tile-cache installation (the terms belonged to the provider that installed
  it), and validates the whole declaration before the slot moves.
- **A base-map switch in the three native reference apps**: a settings-sheet
  section listing every registry descriptor that either carries a credential
  row (offered from the start, marked `Needs <credential label>` until that
  key is applied — the row points at the key field rather than vanishing) or
  publishes a `raster_url_template`. The `openstreetmap` descriptor is the
  first credential-less entry; attribution now follows the serving base map
  instead of joining every registrable provider.
- **The offline card tells the truth after a credential-less switch**:
  registering a URL-template source drops the SDK's tile-cache installation,
  so the card says the serving base map runs uncached until a keyed base map
  is selected again.
- **`MapView.reloadRasterTileSource()`** on the shipped map views (Android,
  iOS, and the RN handle): the surface resolves its raster provider when it
  attaches and pools tiles by coordinate, so a provider re-registration
  alone leaves the previous imagery on screen. The call re-attaches the
  surface and restores camera, overlays, location indicator and listeners.

### Fixed (round 34)

- **The provider-name guard follows the round-33 shell rename.** Its allow-list
  still named `ReferenceMapController.kt`/`.swift`, so the Playwright global
  setup rejected the renamed `RefappDemoSession` dispatch arms and the whole
  browser suite failed before any spec ran. The allow-list now names the
  current files.

### Changed (round 34 — the contract namespace is `rustic_maps`)

- **One namespace across the four surfaces.** The UDL namespace moved from
  `map_native_core` to `rustic_maps` in one break: Kotlin imports read
  `uniffi.rustic_maps.*`, the generated Swift file is `rustic_maps.swift` over
  the `rustic_mapsFFI` module (the XCFramework name follows), and ubrn emits
  `src/generated/rustic_maps.ts` and `cpp/generated/rustic_maps.*`.
- **Everything derived from the namespace moved in the same change**: the
  build scripts (`build-android.sh` now clears the generated package directory
  so a stale namespace cannot linger; `build-ios.sh`, `build-rn.sh`,
  `package-rn-npm.sh`, `generate-swift-package-manifest.sh`), the Swift
  package manifest, the RN iOS wiring (pod symlink, Xcode project, shell wire
  script), the CI workflows, `contract_sync.mjs`'s namespace parser, the docs
  snippet harness and the tutorial Kotlin imports.
- **Deliberately unchanged**: the `map-native-core` crate name, the
  `libmap_native_core` artifacts, the JNA library name and the UDL file name;
  the crate rename is a separate approved item and no published package
  identity moves.
- **Guarded**: the Kotlin and Swift bindings-generation tests now assert the
  generated package and the FFI import, so a future namespace drift fails a
  test instead of a platform build.

### Added (round 33 — wasm MapView bound to the SDK root)

- **The wasm seam creates the SDK root once.** `await MapsSdk.create()`
  resolves to the root (`compiledVersion`), `new MapView(canvas, maps)` binds
  the canvas to it (`sdkCompiledVersion`), and the optional third argument
  pins the tile resolution for capture tools — omitted, it follows the same
  device-pixel-ratio rule as `devicePixelRatioThreshold()`. `initializeSdk()`
  stays as the legacy readiness alias. The demo, the golden harness, the
  first-map tutorial and the constructor tests were migrated, and a new
  `maps_sdk_root` Playwright spec pins the shape.

### Added (round 33 — React Native typed MapView and package-owned SDK)

- **The RN package owns the native SDK.** `MapsSdk` proxies one native
  `MapSdk` per process (credentials with and without tile cache, search,
  suggest, geocode, cache terms/TTL, provider diagnostics), and the typed
  `<MapView>` wraps the shipped widgets (Android AAR `MapView`, iOS
  `MapsSdkRusticMapView`) with insets, frame pump, gestures, camera verbs,
  id-addressed overlays, the location layer and the ready/camera/click/error/
  statistics callbacks.
- **The RN reference shell migrated** and deleted its third controller copy
  and JNI shim (`ReferenceMapController` is now the demo-only
  `RefappDemoSession`; `RefappMapView`, `RefappMapHostView`,
  `RefappMapManager`, `NativeWindowShim` and the app CMake are gone). The
  launch-intent and `PROVIDER_API_KEY` credential seams moved into the
  package runtime. `scripts/build-android.sh` now assembles the widget AAR
  alongside the native one, and `scripts/package-rn-npm.sh` materializes the
  pod's generated Swift bindings into the tarball instead of shipping the
  repository symlink (failing loudly when they are absent).
- **Tests**: both TypeScript gates, jest 53/53 (including two typed-parity
  tests) and `:app:compileDebugKotlin`. The iOS pod build is not yet run; the
  pod's generated-Swift symlink and the release payload are tracked as the
  next packaging step.

### Added (round 32 — machine-checked documentation snippets)

- **The tutorial snippets are now compiled by CI.**
  `scripts/check-doc-snippets.mjs` extracts every fenced block from
  `site/book-src/`, wraps illustrative fragments in a documented ambient
  context, and checks them against the generated surface: strict
  `tsc --noEmit` for the wasm and React Native TypeScript,
  `swiftc -typecheck` for the generated UniFFI Swift (`swift package
  dump-package` for `Package.swift` snippets), and a labelled Kotlin
  symbol/API assertion because the toolchain has no Kotlin compiler.
  `--strict` fails when a surface is unavailable and `--self-test` proves
  each checker rejects an injected type/symbol error.
- **`docs-snippets.yml`** builds the wasm package and generates the React
  Native, Swift and Kotlin bindings on docs/contract changes, then runs both
  checks. 58 snippets pass (22 JS/TS, 18 Swift + 1 manifest, 17 Kotlin API
  assertions, 3 Kotlin fragments with no SDK symbol).
- **Snippet fixes the check exposed**: a stray closing fence and a C shim
  mistagged as Kotlin, missing Swift argument labels,
  `FileManager.cachesDirectory`, React Native `null` vs `undefined`
  boundaries, the u64 tile-cache budget as a `bigint`, and an un-narrowed
  `querySelector` result.

### Added (round 32 — shared demo plans)

- **The demo-plan picker has one source.** `platform/reference-scenes.json`
  gained a `demo_plans` section (scenario, fixture path, consumer label,
  info-affordance description). Android's `ReferenceScenesLoader` and
  `MainActivity`, iOS's `ReferenceScenesCatalog` and
  `ReferenceDemoPlanLoader(catalog:)`, and RN's `referenceScenes.ts` build
  their rows from it. The app-side label/description/scenario constants are
  deleted on all three; iOS additionally fails loudly when a fixture's own
  scenario name disagrees with the artifact, and its home-sheet trace is now
  addressed from the artifact's `fixture_path` instead of a literal.
  `docs/ReferenceAppUX.md` §9 documents the section.

### Added (round 32 — design-token and error-text parity gate)

- **`platform/reference-tokens.json` is the single source of the reference
  design language**: the §2.1 semantic color roles (dark and light), the §2.3
  type scale, the §2.4 spacing/radii/gutter/behavior constants, and the two
  unified SDK error-detail sentences (`rate_limit_exhausted_detail`,
  `provider_not_configured_detail_template`).
- **`scripts/check_ux_parity.mjs` gates every platform mirror** — Compose,
  SwiftUI, React Native, and the web playground's CSS and §2.2 canvas
  constants — and is addition-tolerant: platform-derived tints are ignored,
  while a changed or missing named value fails. Platforms that genuinely have
  no mirror are printed as omissions, never silently passed. `--self-test`
  injects artifact, mirror, error-sentence and canvas drift into a temporary
  copy and asserts each fails. It runs as a dedicated `format-and-lint` step.
- **The web playground's locked §2.1 values were restored** (the round-31
  visual pass had drifted `surface`, `surface-border` and `text-secondary` in
  both themes); the doc now names the artifact and gate, and marks
  interaction-state tints as platform-derived.

### Added (round 32 — packaging residuals)

- **React Native native payload**: `scripts/package-rn-npm.sh` stages the
  package into a fresh tree, always embeds the dual licenses and third-party
  notices, and packs the three-ABI native payload
  (`jniLibs/<abi>/libmap_native_core.a` plus the arm64
  `AngoratekRusticMapsSdkFramework.xcframework`) with
  `--with-native-payload`; `scripts/build-rn.sh` now builds the same
  arm64-v8a/armeabi-v7a/x86_64 table as the AAR. The lean default package
  stays deterministic for the Linux release job; `--dry-run` prints the
  exact member list.
- **`swift-package` branch manifest**: `scripts/generate-swift-package-manifest.sh`
  emits a `Package.swift` with `binaryTarget(name:url:checksum:)` from the
  release zip URL and SHA-256; the release workflow generates it after the
  draft release uploads its assets and force-pushes one commit (manifest,
  generated Swift sources, licenses/notices).
- **Install-snippet release README**: `scripts/generate-release-readme.sh`
  writes `rustic-maps-sdk-vX.Y.Z-README.md` with per-platform install
  snippets, asset names, version, checksum/Sigstore verification and license
  pointers; the release ships it covered by the checksums and signatures.
- **Licenses and notices in every package**: the AAR (`assets/licenses/`),
  the XCFramework zip, both npm tarballs and the plain wasm archive carry the
  dual MIT/Apache-2.0 texts and `THIRD_PARTY_NOTICES.md`, and the assemble job
  asserts it fail-closed.

### Added (round 31 — iOS `MapView` in the Swift package)

- **A `MapView` ships with `MapNativeCoreUI`.** A `UIView` with a
  `CAMetalLayer`, storyboard-capable `init?(coder:)`, the surface pointer
  computed inside the view, the 2048-pixel drawable cap applied as a uniform
  scale (point size preserved), a `CADisplayLink` frame pump with the same
  settle-skip/back-off rules as Android, gestures routed through the contract
  camera members, camera `setCamera`/`animateCamera`/`fitBounds`, id-addressed
  overlays plus the bulk list, the location indicator and a
  `MapViewDelegate`. A SwiftUI `SwiftUIMapView` representable ships beside it.
- **The SwiftUI reference app migrated** and `MapSurfaceDisplayView.swift` is
  deleted; keyless sessions construct the view with no SDK, so no attach error
  is reported before a credential is applied. Every demo feature is preserved.
- **Verified** with the package build, `scripts/build-ios.sh`, the simulator
  build, and XCUITest 6/6 keyed plus the keyless ladder.

### Changed (round 31 — live-demo UX and key onboarding)

- **The browser demo is a live demo, not a "playground".** The title, panel
  heading, aria labels and first-run copy use the product vocabulary; internal
  terms ("reference-scene artifact", the renderer adapter sentence) are gone
  from first-run surfaces.
- **Setting a provider key is the obvious next step.** When keyless, the trip
  card shows a full-width *Add a provider key* action that opens Settings with
  the credential field focused; once registered it becomes a green
  *Key applied — Manage* state. Settings leads with Provider credentials (a
  visible label, masked field with show/hide, Apply/Clear, registry hint and
  terms, an applied chip) and offers an opt-in remember-on-device toggle
  (localStorage only when ticked, automatic re-registration on reload).
- **Calmer chrome.** The renderer readout is opt-in behind Advanced → Debug;
  the bottom line reads "Starting the map…", "Ready", "Fallback: 2D grid" or
  "Failed: …", with adapter/version detail in diagnostics.
- **A full visual pass** on the §2 tokens: panel surface/border/shadow/sticky
  header, button hierarchy, control heights, focus rings, contrast-measured
  accent colors, dock polish, and a phone-width bottom-sheet layout with no
  horizontal scroll.
- **Tests**: Playwright 71/71, with new coverage for the key call-to-action
  and focus return, credentials-before-appearance ordering, the readout
  opt-in, show/hide masking, and a remembered key surviving a reload.

### Added (round 31 — Android `MapView` in the AAR)

- **A lifecycle-aware `MapView` ships with the Android SDK.** The
  `ANativeWindow_fromSurface` shim is now compiled into
  `libmap_native_core.so` (target-gated `jni`/`ndk-sys`, a fixed
  `NativeWindowBridge` class, a consumer ProGuard keep rule), so integrators
  no longer write a C shim or a JNI declaration. `com.angoratek.mapsdk.MapView`
  owns the surface, `onStart`/`onStop`/`onDestroy`, the Choreographer frame
  pump (capped-extent resize, idle-settled skip, one in-flight draw,
  unavailable-only back-off, a 1 s watchdog), gestures through the contract
  camera members, camera get/set/animate/fitBounds, id-addressed overlays, the
  location indicator and one listener (`onMapReady`, `onCameraChanged`,
  `onMapClick`, `onError`, `onFrameStatistics`). A Compose `MapView` wrapper
  drives the lifecycle from `LocalLifecycleOwner`.
- **The reference app migrated**: `MapSurfaceHost.kt`,
  `NativeWindowSurfaceShim.kt` and the app-side C shim/`Android.mk` are
  deleted. A failed attach no longer leaks its `ANativeWindow*` reference.
- **Verified** with the AAR build and JVM smoke, the keyless and keyed
  Espresso runs, and an on-device smoke on the Pixel_9 (raster render, pan,
  zoom, 3D follow, arrival, diagnostics).

### Added (round 31 — API v2 engines, overlay addressing and Navigator verbs)

- **Engine interfaces on the contract.** `SearchEngine` (`search`, `suggest`,
  `reverseGeocode`), `RoutingEngine` (`calculateRoute`,
  `calculateDistanceMatrix`), `OfflineManager` (`tileCacheTerms`,
  `setTileTimeToLive`) and `Diagnostics` (`addListener`, `flushRecent`) each
  wrap an existing `MapSdk` and delegate verbatim — same validation, typed
  failures and detail messages. The matching `MapSdk` members remain.
- **Id-addressed overlays.** `MapView.addOverlay(overlay)` answers a generated
  identifier (`overlay-1`, monotonic per view, never reused),
  `updateOverlay(id, overlay)` replaces the entry in its draw position, and
  `removeOverlay(id)` answers whether it was present. Validation is the same
  all-or-nothing rule `setOverlays` applies; `setOverlays` stays the bulk call
  and replaces the whole drawn list (dropping the addressed bookkeeping), and
  `clearOverlays()` clears both. Overlay state, identifiers and the identifier
  sequence survive detach/attach. The wasm `MapView` publishes the same members
  (`addOverlay`/`updateOverlay`/`removeOverlay`) with the same validation and
  identifier semantics.
- **Navigator verbs.** `addNavigationListener(listener)` subscribes one
  listener to all four topics and returns a composite `Subscription`;
  `stopNavigation()` ends the trip and clears its state, speech-band memory and
  reroute gate while leaving subscriptions and the configured reroute policy in
  place. The four per-topic subscriptions remain.
- **Bindings and docs**: Kotlin, Swift and the RN JSI bindings regenerate from
  the same UDL; `docs/PublicApiV2.md` records the landed contract status, and
  the tutorials adopt the engine, one-listener and id-addressed overlay
  shapes.

### Added (round 31 — a vendor-neutral URL-template vector source)

- **`url-template-vector-provider`**: a wasm-safe crate that implements the
  `VectorTileProvider` contract over any `{z}/{x}/{y}` URL template, with the
  integrator supplying the source descriptor (name, attribution, zoom span)
  and the HTTP transport. It names no vendor, validates the template at
  construction (missing placeholders, unsupported `{s}`, unbalanced braces,
  non-http(s) schemes and relative templates are all refused with precise
  typed errors), refuses out-of-bounds zoom/tile requests before any network
  call, and hands wire bytes to `VectorTileData` so gzip sniffing stays in one
  place. Covered by 18 unit tests and three integration tests against a real
  local HTTP server (fetch, 404 mapping, 503→200 retry).

### Changed (round 31 — one shared provider plumbing)

- **The provider HTTP plumbing lives once.** The token-bucket limiter, the
  bounded-retry loop for idempotent GETs, the transient-error predicate, the
  transport-error mapping and the native/browser transport selector moved into
  `provider-transport`; `mapquest-provider` and
  `openstreetmap-us-vector-provider` were refactored onto them and their
  copies deleted. Defaults, backoff, typed errors and verbatim detail strings
  are unchanged, and the provider-specific status→`SDKError` maps stay in each
  provider crate.
- **No new dependencies.** `provider-transport` now depends on
  `geospatial-types` for the shared error vocabulary.

### Changed (round 31 — documentation parity completion)

- **One scenario across four platforms.** The first-map tutorial uses the same
  camera (Capitol Park, zoom 12, zero bearing/pitch) on Android, iOS, React
  Native and WebAssembly; search-and-routing uses one query, two waypoints and
  one routing mode, with one fit-bounds inset story on the native tabs.
- **Navigation reaches the map on every tab.** React Native gained the route
  polyline, the listener-driven next-maneuver card and the course-up follow
  camera; the WebAssembly tab states honestly that no navigation controller
  rides the browser surface and shows what the canvas can draw instead.
- **Shareable platform URLs fixed.** The tab component now writes
  `pathname + search + '#' + platform`, so a query string can no longer be
  swallowed by the fragment; deep links selecting a heading inside a panel and
  the persisted cross-chapter choice are unchanged.
- **Internal vocabulary is refused.** The publish scrub now rejects
  `crate(s)`, `fixture(s)`, `harness(es)`, `playground(s)`, `milestone(s)`,
  numbered development rounds and development-sense "slice" phrases, plus the
  private repository's HTTPS and SSH forms, while allowing architecture senses
  (an arm64 slice). Its self-check covers six cases.

### Fixed (round 31 — simulated-trip drawer staleness)

- **A new simulated trip no longer inherits the previous trip's arrival
  state.** Starting or ending a replay drops the last update, so the drawer
  cannot show a finished trip's "Arrived" phase and ETA beside the new trip's
  counting countdown. Pinned by a jest test.

### Changed (round 31 — CI and budget policy)

- **One compile per gate set.** `format-and-lint` now runs formatting, clippy,
  the nextest suite, the rustdoc tests and the license/advisory check in one
  job, sharing one warm target directory; the separate `test-rust-core`
  workflow is removed.
- **Documented CI budget** (`docs/PackagingAndRelease.md` §5.1): local gates
  are the primary path, workflows cancel superseded runs and carry path
  filters, the expensive platform suites are pull-request/nightly scoped, and
  publication is never automatic. The account-level spending alert remains an
  owner billing setting.

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

### Added (API v2, fourth slice — camera gesture members)

- **The contract `MapCamera` now carries the gesture algebra**, so every shell
  computes identical pan, pinch and rotate results instead of hand-rolling
  projection math. Three additive members (no renames), each answering a new
  camera object and throwing `SdkError` with the verbatim viewport detail:

  ```text
  MapCamera pan_by_screen_pixels(double delta_x_pixels, double delta_y_pixels, ViewportSize viewport, double field_of_view_degrees)
  MapCamera zoom_by_levels_around_screen_point(double delta_zoom_levels, ScreenPoint focal_point, ViewportSize viewport, double field_of_view_degrees)
  MapCamera rotate_by_degrees_around_screen_point(double delta_bearing_degrees, ScreenPoint focal_point, ViewportSize viewport, double field_of_view_degrees)
  ```

  They wrap the `map-rendering` `CameraInteraction` seam the wasm playground
  already used: the grabbed ground point becomes the new center on a pan, the
  focal world point stays under its pixel through zoom and rotation, and the
  camera's own clamps and bearing wrap always hold.
- **Kotlin/Swift/RN** receive them as `panByScreenPixels`, `zoomByLevelsAroundScreenPoint`
  and `rotateByDegreesAroundScreenPoint`; the generated TypeScript exposes the
  same members. The Android, iOS and RN reference applications route their
  drag/pinch/rotate deltas through them (gesture recognizers and the
  follow-camera break rule unchanged).
- **WebAssembly**: the `MapCamera` class publishes the three members; the M3
  free helpers (`panCameraByScreenPixels`, `zoomCameraByLevelsAroundScreenPoint`,
  `rotateCameraByDegreesAroundScreenPoint`) remain as aliases, so the demo's
  call sites are unchanged. `contract_sync.mjs` now asserts the members.
- **Tests**: `crates/map-native-core/tests/camera_gestures.rs` round-trips a
  pan, holds the focal coordinate through a zoom, checks the rotate delta and
  wrap, and pins the verbatim invalid-viewport message. The first-map tutorial
  gains a per-platform gesture snippet.

### Added (round 27 — demo refinement, vector styling, keyless providers, worldwide tiles)

- **Trip card and settings sheet (web demo)**: the sidebar leads with an
  inline place search over the shared demo places (chips plus Enter to search),
  tone-coded status lines, a stacked *Plan route* / *Start simulated trip*
  pair, and a footnote naming the recorded trip; the expert controls fold
  under a closed **Advanced** disclosure, and provider credentials plus the
  live probes moved into a settings sheet opened by the header gear.
- **Four theme-aware vector styles**: `carto-light`, `carto-dark`,
  `openmaptiles-light` and `openmaptiles-dark` artifacts (landuse, natural,
  leisure, water, boundaries, railways, tiered roads with one motorway casing,
  muted place dots) selected by style id, so a registry descriptor names its
  schema and the page joins it with the active theme.
- **Square polyline joins**: every interior vertex of a thick line fills its
  bend with a screen-space square, removing the dashed look multi-segment
  roads had under butt caps.
- **OpenFreeMap and OpenStreetMap providers**: two keyless registry
  descriptors — OpenFreeMap vector tiles (`planet/latest`, max zoom 14,
  OpenMapTiles style, committed z14 fixture) and OpenStreetMap raster tiles
  (`tile.openstreetmap.org`, max zoom 19).
- **Worldwide raster tiles in the browser demo**: the wasm canvas takes a
  runtime tile-source template and fetches recorded fixtures first, then the
  registered provider, then the registry template, so panning anywhere on
  Earth draws imagery instead of blank tiles.

### Fixed (round 27)

- **Control panel clipping**: the sidebar no longer squeezes its children, so
  an opened **Advanced** disclosure keeps its full height and every control in
  it is reachable; the vector base-map rows are disabled until the GPU
  renderer answers instead of silently ignoring a click.
- **Provider-name guard**: the guard skips gitignored scratch directories and
  the tile-switch spec reads its fixture path from the registry, so the guard
  tests source hygiene rather than test scratch; attribution assertions are
  provider-neutral.

### Changed (round 27 — documentation)

- **Shareable platform links (docs site)**: the platform tab component now
  keeps one page-wide choice, persists it across chapters, writes it into the
  address bar (`first-map.html#android`), opens the platform named by a deep
  link or by a heading anchor, and relabels the on-page contents entries per
  platform so the list stops repeating one heading name four times.
- **Search and routing on the web**: the WebAssembly tab tells the same
  search → route story as the native tabs (register a provider, geocode,
  calculate a route), with the offline-archive route kept as its own section;
  it no longer claims the provider surface has no browser exposure.
- **Downloads page**: rewritten around this site's versioned `/downloads/`
  path — no development-repository URL anywhere, no contradictory statuses,
  and a verification block that points at the identity string the version page
  prints; the publish scrub now refuses that repository URL.
- **Screenshots page**: ordered Android, iOS, React Native, WebAssembly with
  one caption vocabulary and a claim the captures support.
- **Contract wording**: the book no longer calls the interface "frozen" and
  states the pre-1.0 policy instead; `demo-loop.md` is removed from the site.

### Added (round 26 — consumer demo flow, attribution control, follow camera, CI efficiency)

- **Consumer trip flow in the web demo**: the sidebar now opens with a
  *Plan a trip* card — search the shared demo places (keyless) or live
  geocoding (keyed), select a destination to center the map, plan the route
  (the recorded Capitol Park ⇔ Tower Bridge trip when keyless, live routing
  when a provider key is applied), then start the simulated trip. Camera,
  appearance, scene overlays, the live probes and the diagnostics fold under
  a closed **Advanced** panel, so a first-time visitor sees the same
  search-first surface as the native reference apps.
- **Raster ⇄ vector base map**: a *Base map* switch driven by the provider
  registry toggles the raster layer off and paints vector tiles (OSM US
  Sourdough) decoded in wasm through the overlay layer, with a light clear
  color behind the vector fills and a status line naming the tile/feature
  counts and any failed tiles.
- **Frame-budget monitor**: the demo measures its GPU frames, reports the warm
  steady-state median and frame rate in the diagnostics readout, and shows a
  warning strip naming the cause (missing tiles, no WebGPU, GPU-bound frame,
  throttled tab).
- **Attribution info control (all four refapps)**: the always-visible
  copyright line becomes a compact ⓘ control that expands the provider
  attribution and terms, with selectable text and Escape/outside-tap closing.
- **Course-up follow and re-center (all four refapps)**: the simulated trip's
  follow camera points along the smoothed course; a pan/zoom gesture breaks
  follow and it stays broken until the my-location control re-engages it,
  which now renders an accent re-center affordance.
- **Shared demo places**: `platform/reference-scenes.json` (and its SwiftPM
  mirror) gains `demo_places` so every shell can offer a keyless search
  surface from one artifact.
- **Vector style**: the shared style artifact now paints natural, landuse,
  leisure, water, boundaries, railways, highways, routes and places, giving
  a legible light vector base instead of a black background with outlines.

### Fixed (round 26)

- **React Native compliance text**: the attribution panel renders the terms
  notes as one selectable text block instead of a mapped child list, removing
  the dev-time "Text strings must be rendered within a `<Text>`" warning; the
  control itself docks at the map's end edge with the other shells.

### Changed (round 26)

- **Attribution placement and size**: the ⓘ compliance control now sits at the
  map's bottom-end corner on Android, iOS, React Native and the web demo
  (scale bar stays bottom-start), and both the button and its panel are
  smaller than before (34–36 pt / 280–300 px panels).


- **GitHub Actions efficiency (M16)**: every workflow now has a concurrency
  group that cancels superseded runs, and the previously unfiltered suites
  (`test-rust-core`, `wasm-e2e`, `format-and-lint`) only run when their own
  inputs change. With the monthly Actions quota exhausted, the local pre-push
  gate and the local docs publish script are the complete verification path.

### Added (round 25 — hostname live, vector residuals, second provider, screenshots)

- **The documentation hostname serves the docs in place** (M14): the Vercel
  project `rustic-maps-sdk-docs-redirect` used an external `rewrite` instead of
  a redirect, so the browser kept the hostname on every path (`/`,
  `/first-map.html`, `/demo/`); the apex still 308s to `www` and `www` was
  unchanged. The short link was recorded in the README and the site landing.
  The hostname was retired on 2026-11-16.
- **Vector hole fills, style batching and a shared style artifact** (M9):
  `map-rendering` fills polygon interior rings with a CPU bridge-splice +
  ear-clipping tessellator (the flat alpha-blended triangle pipeline has no
  stencil), groups overlay primitives per style layer through
  `overlay_primitives_for_vector_tile`, and reads its demo style from the
  provider-neutral `fixtures/render/vector-style.json` with a documented
  in-code fallback. 163 map-rendering tests.
- **Second-provider proof** (M15): a keyless `osmus` descriptor, tolerant
  credential-less decoding in all four shells, availability-filtered
  attribution and an exact-path vector fixture route prove the generic path
  end to end; `second_provider.spec.mjs` asserts the fixture bytes, media type
  and 404s through `/providers/osmus/…`.
- **Screenshots gallery** (M11): a new site page carries fresh keyed captures
  from the Android, iOS, React Native and wasm reference apps at one revision,
  with captions that state the runtime-credential model.

### Fixed (round 25)

- **React Native readiness and bottom rows**: the chip renders §1's one short
  line per state ("Ready · gpu", "Loading…", "Degraded: …", "Failed: …")
  instead of the long diagnostics status, and the scale bar, reason strip and
  attribution share one bottom-anchored column so the attribution wraps inside
  the viewport rather than running off-screen under the scale bar.

### Added (round 24 — M15 native genericity, M10 packaging identity and npm distribution)

- **Native refapp provider genericity** (M15): Android, iOS and React Native now
  bundle and consume `platform/providers.json` for display names, credential
  labels, per-platform availability, hints and attribution; the registration
  dispatch arms remain the only provider-named code. Generic automation keys
  (`providerApiKey`/`providerKey` on Android/RN, `PROVIDER_API_KEY`/`PROVIDER_KEY`
  on iOS/RN, defaulting to the only registered provider) replace the
  vendor-named launch seams, and the provider-name guard now scans all three
  native shell roots with a file-level dispatch-arm allowlist.
- **angoratek packaging identity** (M10): the SwiftPM package/product/module is
  `RusticMapsSDK` (refapp imports, XcodeGen manifest, RN iOS wire script and
  tracked project follow; the `map_native_coreFFI` module stays contract-bound);
  the RN npm package is `@angoratek/rustic-maps-sdk` with a `files` allow-list
  and React/RN peers; the wasm manifest is `@angoratek/rustic-maps-sdk-web`;
  and the AAR gains a `maven-publish` publication for
  `com.angoratek:rustic-maps-sdk` whose POM carries JNA and
  kotlinx-coroutines. The version guard checks all four release manifests.
- **npm tarball packaging** (`scripts/package-web-npm.sh`,
  `scripts/package-rn-npm.sh`): the wasm package packs the generated `pkg/`
  tree with the committed manifest, README, licenses and notices
  (`rustic-maps-sdk-web-v0.1.0.tgz`, 9 members, 1,410,765 B, deterministic), and
  the RN package regenerates the ubrn bindings and packs
  `rustic-maps-sdk-rn-v0.1.0.tgz` (19 members, 83,439 B, deterministic); both
  assert the exact member set fail-closed so app shells, fixtures and keystores
  cannot ride along. The release workflow ships and keyless-signs both,
  retaining the raw wasm archive and every existing asset. The RN tarball is
  source-only until its native payload joins the pipeline.

### Changed (round 24)

- **One provider descriptor source**: `provider_credentials` left
  `platform/reference-scenes.json` (and its committed iOS mirror); every app,
  the dev server and the wasm seam read `platform/providers.json`, which gained
  an additive `settings_hint`.
- **Tutorial imports** follow the Swift module rename (`import RusticMapsSDK`),
  and the distribution pages list both npm tarballs alongside the raw archives.

### Added (round 23 — M15 provider genericity (web/wasm) and the live docs demo)

- **Shared provider registry** (`platform/providers.json`, schema 1): one
  descriptor per provider — key, display name, generic credential label,
  per-platform availability, attribution, terms note, `wasm_registration`
  dispatch key and fixture routes (file or tile-directory kind) — with no
  credential values; served same-origin at `/providers.json`.
- **Generic wasm provider registration**: `register_provider(provider_key,
  credential, base_url?)` / `clear_provider()` dispatch through one arm over
  vendor-neutral provider traits; `register_mapquest_providers` and
  `clear_mapquest_providers` remain addition-only aliases.
- **Provider-name allowlist guard**: `platform/web/tests/provider_name_guard.mjs`
  derives provider names from the registry and fails when one appears outside
  the registry, provider crate/fixtures or the wasm dispatch arm; it runs in
  the Playwright `globalSetup` and as a `node --test` suite.
- **Live wasm demo on the docs site**: `scripts/build-docs-site.sh` now stages
  `/demo` from the browser harness with byte-equality gates, the pkg tree,
  tile fixtures, the provider registry, the offline-graph fixture and
  provenance, excludes the test-only `/golden/` route and enforces a 25 MB
  ceiling; the page's tile and registry URLs are relative, so the same build
  serves at the site root and under `/demo/`. The live-demo chapter and the
  landing page link to it.

### Changed (round 23 — M15 provider genericity, web/wasm)

- **Descriptor-driven fixture router**: `platform/web/serve.mjs` resolves
  `/providers/{provider_key}/{provider-relative path}` from each descriptor's
  `fixture_routes` (compiled patterns, file fixtures, tile directories with
  the zoom/traversal guards); the server names no provider.
- **Descriptor-driven playground**: the provider credentials panel renders one
  masked row per registry descriptor (label, terms note, availability), takes
  the attribution line from `attribution_text`, and calls the generic
  registration surface; a documented `?useProviderFixtures` seam points the
  panel at the registry's same-origin fixture routes for the browser suite.
- **Web provider tests migrated** to registry-derived keys/labels/paths and the
  generic registration surface; Playwright 45/45.

### Added (M10 — tag-driven release pipeline)

- **Tag-driven release workflow** (`.github/workflows/release.yml`): a `v*.*.*`
  tag push runs the gate set on the tagged tree (fmt, workspace clippy, the
  wasm-closure lib clippy, workspace tests, Playwright, RN jest/tsc and the
  version-sync check), builds the Android AAR, the two-slice iOS XCFramework
  zip and the wasm `pkg/` archive, then assembles a draft release: an exact
  flat asset-set assertion (fail-closed in both directions), per-file
  `.sha256` sidecars and `SHA256SUMS.txt`, keyless Sigstore `cosign sign-blob`
  bundles on every asset, fail-closed third-party-notices regeneration, and
  changelog-derived notes. `workflow_dispatch` is the dry run — the same jobs
  and signing, with the assembled bundle uploaded as a workflow artifact
  instead of a release API call. The RN npm tarball is skipped while the
  package remains private/unscoped without a `files` allow-list; the wasm
  asset is a plain archive of `pkg/`; the `swift-package` branch manifest is
  still a separate item.
- **Workspace-version guard** (`scripts/check-workspace-version.sh`): asserts
  the single version line from `Cargo.toml [workspace.package].version`
  against the RN package manifest and ubrn config, and — with `--tag` — that
  the release tag names the same version; wired into the release gates and
  `format-and-lint`.

### Added (round 22 — provider genericity north star)

- **Provider genericity as a standing rule** (`CLAUDE.md`, `docs/ProviderGenericity.md`,
  M15): the audit found vendor leakage in the web dev server's fixture routes,
  the playground labels/attribution, the wasm registration export, refapp
  attribution strings and the automation key name. The plan moves all of them
  behind a shared provider registry (key, labels, availability, attribution,
  terms, fixture routes, wasm registration), a descriptor-driven
  `/providers/{provider_key}/…` fixture router, a generic wasm
  `register_provider` surface (vendor alias retained), descriptor-driven refapp
  chrome and a provider-name allowlist guard test. Adding HERE, Google,
  TomTom, Mapbox, Radar or any URL-template source becomes a provider crate +
  descriptor + dispatch arm, never a harness/refapp rewrite.

### Changed (RN refapp honesty gaps, parity audit D3–D10)

- **RN design-token mirror aligned with `docs/ReferenceAppUX.md`** (D3): the
  type scale now uses the §2.3 row names (`typeCaption`, `typeBody`,
  `typeBodyStrong`, `typeTitle`, `typeManeuver`, `typeManeuverDistance`); the
  missing §2.4 rows are mirrored (`gutter-block`, `elevation-surface`, the
  framing/navigation-follow values, `map-control-zoom-step`,
  `compass-renorth-animation`, the home-sheet fractions and peek row count) in
  `theme.ts`, `mapControls.ts` and the new `sheetLayout.ts`; the accent tint,
  scrim and screen backdrop are documented as platform-derived values rather
  than §2.1 rows.
- **RN refapp honesty states** (D4–D10): the readiness chip stays neutral while
  no state is known and shows the keyless prompt without a provider; the
  diagnostics sheet gains the recent-events block and states the RN bridge's
  event-stream unavailability; the attribution notice stays visible above the
  trip's measured navigation drawer; the scene group is disabled-with-reason
  until the map is ready and its Info describes only selected scenes; the
  guided-walk action is disabled-with-reason when no native module or plan can
  run it; the theme row is the single toggle naming the other theme; and the
  peek dock height is derived from its two rendered rows instead of the
  invented 176 px literal.

### Added (round 21 — live route and raster tiles on the wasm provider seam)

- **Live route and raster-tile calls on the wasm seam** (M13): `map-web-bindings`
  exports `calculate_route_live` and `fetch_raster_tile_live` on the registered
  MapQuest provider set. The route call sends the demo default
  (automobile/fastest/metric, `RouteOptions::default()`) and answers the same
  plain `RouteData`-shaped object the offline routing seam returns (legs,
  steps, snake-case maneuver labels, end-anchored instruction texts); the tile
  call answers standard-resolution PNG bytes as a `Uint8Array`. The dev server
  serves the recorded Directions response at the provider's own
  `directions/v2/route` path and a committed recorded tile at the Tiles API
  path, so the harness stays network-free. The playground's **Provider
  credentials** panel gains a demo-route probe (distance, duration, first
  maneuver) and a one-tile fetch probe (byte length plus an optional canvas
  preview); both exports are whitelisted as ahead-of-contract in
  `contract_sync.mjs`. Playwright 44/44 (registered route parse, registered
  PNG tile fetch, keyless refusals for all live calls, panel probes).
  Live-tile consumption by the GPU canvas remains open.
- **Vector-tile styling into overlays** (M9): `map-rendering::vector_style`
  converts a decoded vector tile into the renderer's existing fill, polyline
  and point-marker overlays under a data-only style (source layer, geometry
  kind, one-attribute text filter, RGBA/width/radius), projecting tile-local
  extent coordinates through the tile's web-mercator address. Exterior rings
  are selected by winding (holes documented as skipped); tests cover the
  projection, an attribute filter and the committed Sourdough fixture's
  fill/line/circle layers.

### Added (round 20 — live browser providers, credential injection for automation)

- **Live providers on the wasm seam** (M13): `map-web-bindings` now depends on
  the portable MapQuest provider crate and exports
  `register_mapquest_providers`, `clear_mapquest_providers` and
  `forward_geocode_live`. The playground gains a **Provider credentials**
  panel (masked key, Apply/Clear, live-search probe) that registers the key at
  runtime and fetches through the browser `fetch` transport; the dev server
  serves the recorded geocoding fixture at the provider's own path, and three
  Playwright tests cover the registered fetch/parse path, the keyless refusal
  and the panel flow (40/40 total).
- **Runtime credential injection for UI automation**: the Android/RN launch
  intent extra `mapquestApiKey` and the iOS/RN `MAPQUEST_API_KEY` launch
  environment apply a key at start through the public registration path. The
  Espresso and XCUITest suites assert the neutral prompt and skip
  provider-dependent cases without a key; the Android suite adds a key-gated
  live search → route test. The keyed CI commands are documented in
  `docs/ProviderCredentialsAndBrowserProviders.md` §5.6.

### Added (round 19 — runtime provider credentials everywhere, portable provider transport)

- **Runtime provider credentials on every native refapp** (M13): Android
  Compose, iOS SwiftUI and React Native (both shells) now start keyless and
  take provider credentials from a settings surface opened by a gear beside the
  readiness chip. The rows come from the shared `provider_credentials`
  descriptors; the field is masked, Apply registers the provider through the
  SDK, Clear removes the stored copy and states the restart requirement, and
  “Remember on this device” defaults off. Opt-in persistence uses the iOS
  Keychain and Android Keystore AES/GCM private preferences. Every build-time
  key seam is deleted (`BuildConfig`, Info.plist, `local.properties`,
  `Local.xcconfig`), so no refapp artifact can embed a credential; the keyless
  start is a neutral prompt, and an expected `ProviderNotConfigured` failure no
  longer paints a red readiness failure. Applying a credential retries the
  surface attach. Verified on the Pixel_9 (keyless prompt, masked entry, apply),
  with builds green on Android, iOS and RN; jest 33/33.
- **Portable provider HTTP transport** (M13, groundwork for live browser
  providers): the new `provider-transport` crate defines request/response
  records and the `HttpTransport` contract, with a native `reqwest`
  implementation and a browser `gloo-net` fetch implementation bridged through
  `spawn_local`/oneshot (the browser futures are not `Send`). The rate limiter
  and retry back-off use a portable clock/timer, and `mapquest-provider` is
  refactored onto the seam with no `reqwest`/`tokio` in its non-test build.
  Native tests stay green and the crate now compiles and clippies on
  `wasm32-unknown-unknown`; the wasm-closure CI job includes the new crates.
- **Runtime credential injection for automation** (round 20 amendment): local
  and CI UI automation can supply a key at launch without a build-time seam —
  the Android/RN launch-intent extra `mapquestApiKey` and the iOS/RN
  `MAPQUEST_API_KEY` launch environment are applied through the public
  registration path, never persisted by the injection and never logged. The
  Compose and XCUITest suites are keyless-aware: they assert the neutral prompt
  and skip provider-dependent cases when no key is supplied, and run keyed when
  the variable is present.
- **Docs hostname plan** (M14): `docs/DocsHostname.md` and `site/hostname/`
  recorded the verified Vercel/DNS facts and the isolated 308-redirect project
  onto the GitHub Pages docs; deployment awaited owner approval.

### Fixed (round 19)

- **Keyless refapp state**: the Compose readiness chip shows the neutral
  “Set a provider key…” prompt instead of an endless “Loading…” or a red
  provider failure, the duplicate credential line is hidden until a key is
  applied, and a successful apply retries the surface attach.
- **Stale key seam removed**: a gitignored `Local.xcconfig` and the
  `MAPQUEST_API_KEY` line in `platform/android/local.properties` from the old
  build-time path were deleted. **The removed key should be rotated.**

### Added (round 18 — runtime provider credentials design, iOS parity fixes)

- **Runtime provider credentials design** (`docs/ProviderCredentialsAndBrowserProviders.md`,
  M13): the owner-directed model for removing build-time provider keys from
  every refapp — a descriptor-driven settings surface, masked entry, explicit
  apply/clear, opt-in secure persistence (Keychain /
  `EncryptedSharedPreferences`), honest keyless states, and future
  HERE/Google/TomTom/Mapbox/Radar rows. The study records the measured CORS
  facts (MapQuest `*`, OSMUS origin), the wasm transport blocker
  (`reqwest`/`tokio` do not build for `wasm32-unknown-unknown`), and the ubrn
  web verdict (wasm-bindgen glue, no networking). The shared artifact gains
  the `provider_credentials` descriptors (never values), and
  `docs/ReferenceAppUX.md` §6.5 fixes the settings component.
- **iOS refapp parity fixes**: the scale bar now uses the same −2…9-decade
  ladder, 50–120 px width rule and whole-kilometre labels as Android, RN and
  the web; the course median is circular (a window straddling north no longer
  resolves the wrong way); and the provider error details share one wording
  (`no '<contract>' provider is configured`, the rate-limit sentence) across
  iOS, Android and RN.
- **Demo artifact text**: the navigation preview strings now match the
  regenerated Sacramento traces.

### Added (round 17 — RN control parity, CI honesty, documentation depth)

- **React Native map controls and gestures** (M5 parity audit D1/D2): the map
  control cluster is complete on both RN platforms — zoom −/+, my-location, the
  2D/3D plane toggle and a compass, plus the §2.4 scale bar, with the five
  bridge methods on Android and iOS, a camera readback carrying zoom, bearing,
  pitch, plane state, ground scale and fix availability, and single-pointer
  drag-pan wired through `RefappMapView` (`onTouchEvent`) and the iOS host
  view (`UIPanGestureRecognizer`) with the §5.3 follow-camera break. The zoom
  controls repeat at the token's 250 ms cadence, the compass appears only off
  north and eases north up, and the scale bar uses the 1-2-3-5-10 ladder. The
  bridge's Android unsubscribe no longer calls `removeAllListeners` (the D11
  trap). A pure `mapControls.ts` carries the shared math with jest coverage;
  jest 22/22, tsc clean, on-device evidence on both platforms.
- **CI gates that cannot miss the contract** (critic item 1): the Android and
  iOS platform workflows now trigger on the crate closure, `Cargo.toml` and
  `Cargo.lock`, so a UDL or SDK-crate change rebuilds the wrappers instead of
  merging silently. A new RN workflow runs jest and strict TypeScript on the
  shell and the UDL; the wasm workflow gains the wasm-closure clippy job and
  derives the wasm-bindgen CLI version from the locked graph (no more
  hand-copied constant).
- **Demo traces with real street names**: the committed navigation trace
  fixtures now read “Head east on N Street”, “Turn left onto 15th Street”,
  “Turn right onto O Street” and “Arrive at Capitol Park” instead of the
  generator's placeholder prose, so the reference apps' guided demos display
  plausible guidance. Regenerated through the guarded fixture seam; navigation
  tests green.
- **Documentation depth**: the README reflects all 13 crates, the ten provider
  contracts, the 30-day tile TTL and the current roadmap; `docs/reference/BrowserLoop.md`
  states `pkg/` is generated and gitignored; `docs/OfflineGraphCompilation.md`
  documents the shipped `RMOG` container (PRD vocabulary aligned); the site adds
  **How it works** and **Versions and support** pages; and
  `docs/RefappCoreCentralization.md` studies centralizing the reference-app core
  in Rust, with a proposed M12 (parity floor, a pure crate, a refapp-only UDL,
  RN deferred, a shared app state machine rejected).

### Added (round 16 — MVT decoder, RN iOS shell, product docs site, Sacramento re-anchor)

- **Mapbox Vector Tile v2.1 decoder** (`5bb8ad2`, M9 slice 2): the new
  `map-vector-tiles` crate turns provider-fetched vector tiles into
  renderer-consumable structures — a dependency-free protobuf wire reader,
  the layer/feature/attribute model (string, float, double, int, uint, sint,
  bool), and the geometry command-stream decoder resolving MoveTo/LineTo/
  ClosePath into tile-local extent coordinates with interior rings. No
  bytes are trusted: every malformed path surfaces as a typed
  `VectorTileError` (arbitrary-byte proptest over 512 cases), the code is
  panic-free outside tests and unwrap-free in non-test builds. Covered by
  the Mapbox specification examples, a malformed-input suite and the
  committed Sourdough fixture; wasm-safe with only `thiserror` as a
  dependency.
- **RN refapp iOS shell** (`0980a99`, M5): the React Native reference
  application now runs on iOS as well as Android — a Swift port of the
  map controller, the host view/manager/module bridge glue, a
  platform-branched event emitter (iOS `NativeEventEmitter` listener-count
  gate vs Android `DeviceEventEmitter`), the guarded-deep-import workaround
  for the ubrn checksum mismatch, and a full two-platform `build-rn.sh`
  pipeline (xcframework build → spec-include guard → pod install → project
  wiring → xcodebuild → tsc). jest 15/15; parity verified on the simulator.
  Residual: the per-slice xcframework is arm64-only, so the app pins
  `ARCHS=arm64` until ubrn ships multi-arch slices.
- **Product documentation site** (`7689a2b`, M11): the docs site now reads
  as SDK product documentation rather than a maintainer notebook — an
  Overview landing page, requirements and distributions, performance
  guidance, a live-demo entry, four tabbed per-platform tutorials (first
  map, search and routing, turn-by-turn navigation, offline maps), a
  downloads skeleton, an FAQ, and the changelog/notices, with the
  maintainer chapters demoted to a compact Project section. mdBook is
  upgraded inside the book (copied `product-docs.css`, a
  `platform-tabs.js` tab component, dark `coal` default) so the scrub,
  byte-equality and provenance publish gates stay untouched.
- **Downtown Sacramento re-anchor** (`5719d16`, `40bc43b`, `c1bc2db`): every
  demo and test anchor moved from Zurich/Berlin to Capitol Park in
  Sacramento — the shared scenes artifact and its iOS copy, all trace
  fixtures and their README, every Rust/Android/iOS/RN anchor constant,
  216 live-recorded raster tiles, the eight zoom-14 goldens, the three
  MapQuest payload fixtures (recorded with the rotated key; `mapUrl`
  credentials redacted as `key=REDACTED`) and the README screenshots.
  Honest note: the playground capture is zoom 14 / pitch 30° because no
  zoom-12 tiles were ever recorded; the caption says so.

### Added (round 15 — vector provider seam, RN speech consumer, packaging/release design)

- **Vector-tile provider seam + OpenStreetMap US implementation** (`ddc3b26`,
  M9 slice 1): the `VectorTileProvider` contract in `geospatial-traits`
  (source descriptor carrying the attribution line and zoom bounds — any
  URL-template vector source implements it, so `VectorTileSource`/FR-D
  arrives as another implementation), the `VectorTileData`
  record + gzip-sniffing wire encoding in `geospatial-types`, and the new
  `openstreetmap-us-vector-provider` crate serving osmus's Sourdough
  tileset: TileJSON-verified `https://tiles.openstreetmap.us/vector/sourdough/{z}/{x}/{y}.mvt`
  template as documented constants, the usage-policy posture encoded
  (conservative anonymous-tier rate limiting, honest User-Agent, bounded
  retries, back-off on 429), the attribution line surfaced on the
  descriptor. map-core gains the registry slot; map-cache gains the
  `VectorTiles` response category with the 30-day default TTL and the
  provider-terms ceiling lockstep test, per-tileset fingerprint
  namespacing. One real fixture tile committed with provenance; tests stay
  network-free; the frozen UDL contract is byte-identical (FFI exposure of
  vector providers is a later slice). The MVT decoder is the next slice and
  deliberately not built here.
- **RN refapp speech consumer** (`16e69de`, M5): replaces the documented
  no-op. A jest-covered decision consumer (contract `utterance_id`
  keep-last-seen dedup, toggle/no-text/arrival honesty — arrival deliveries
  are noted, never voiced; identical consecutive text legitimately
  re-speaks when the id differs) and an app-owned Android TTS speaker
  mirroring the Compose engine's semantics: queue-flush pacing, `AudioFocusRequest`
  navigation-guidance ducking with the legacy fallback, focus-loss
  honesty, drain-aware focus abandon. The drawer gains the §4.2 speech row
  and the §6.2 disabled-with-reason line. jest 15/15; verified on the
  emulator with logcat evidence; ids cross the bridge as decimal strings.
- **Packaging & release design** (`docs/PackagingAndRelease.md`, M10
  opening): the four owner rulings (GitHub Releases only; angoratek
  namespace; docs-site `/demo` hosting; tag-driven release workflow) turned
  into a concrete artifact matrix from measured tree sizes, a release.yml
  design (gates-first on the tagged ref, keyless cosign, SHA-256 sidecars,
  changelog-derived release notes), the SPM `binaryTarget` release
  distribution, npm files allow-lists, the docs-site `/demo` requirements
  and copy plan, and the single-workspace-version policy; cited public
  reference: MapLibre and Ferrostar release practices.

### Added (round 14 — M7 complete, documentation style guide, M9 opens)

- **Wasm in-memory graph path** (`1a7f8d5`, M7's last open item): the
  browser loads the compiled `RMOG` archive directly from bytes — no
  `mmap` on wasm. The compiled fixture is committed and guarded by an
  always-on byte-equality test that also re-validates it through the same
  `from_bytes` loader the wasm seam uses; the map-web-bindings export
  `offline_route_from_archive_bytes(archiveBytes, …coords)` answers
  contract-shaped route objects (legs/steps/maneuver labels, end-anchored
  texts) and typed refusals (disconnected pairs, corrupted bytes) verbatim;
  the fixture serves same-origin from the playground dev server, and a new
  4-test Playwright spec (38/38 total) pins turn-maneuver and oneway routes
  plus both refusal paths. The frozen UDL contract is untouched: the helper
  is documented as ahead-of-contract in `contract_sync.mjs`'s
  `WASM_ONLY_FREE_HELPER_NAMES`, pending a wasm offline-routing surface.

- **Tile-pack manager** (`ad8eae9`, M7 slice 4, FR-F): region prepare with a
  named tile-count budget (bbox + zoom range, capped at zoom 20 / 65 536
  tiles), per-pack progress and removal reports, and eviction coexistence —
  pack tiles live in the same SQLite store (schema v2 adds the `tile_packs`
  and `tile_pack_members` tables) as ordinary cache entries pinned by their
  pack record and excluded from LRU eviction while it exists; membership is
  written before download so a mid-prepare pack can never be evicted. A
  scheduler integration test proves pack tiles served after the pack call
  trigger no further provider requests.
- **Archive crate split** (`6650eed`, M7 slice 5): the self-describing
  archive format moved verbatim to a new `map-offline-graph-archive`
  (wasm-viable, pure byte parsing; `memmap2` target-gated), leaving
  `map-offline-graph-compiler` as the native-only PBF producer and keeping
  map-core's production dependencies wasm-clean.
- **Benchmark gate** (`472a8f5`, M7 slice 6): `dijkstra_reference_search`
  shares A\*'s exact frontier/relaxation/restriction path (a zero heuristic),
  so equality tests measure the heuristic alone; `ComputedRoute` records
  settled-node count and total cost. Deterministic, network-free criterion
  benches over weighted-grid and random-spanning synthetic families record
  the honest profile — A\* converges up to ~2× faster on anisotropic
  road-like graphs and loses ~20–35% on self-similar uniform-speed lattices.
  CI gains an informational `benchmark.yml` job (criterion artifacts only);
  the enforced regression alarm is a generous 1000 ms solved-solve budget
  test (~10³ above measured solves; re-run before bisecting on a firing).

### Fixed (round 14 — the owner's field findings, plus the docs tone)

- **Pan/rotate direction** (`9afb70e`): the iOS reference model carried the
  only gesture inversion — drag-pan set the camera center from the ground
  under `center + delta` (content moved opposite the pointer) and two-finger
  rotation turned content counterclockwise under a clockwise gesture. Both
  now follow the content-follows-pointer convention; web, Compose, and RN
  paths were proven correct. The convention is pinned per layer: three
  Playwright mouse probes, an Espresso drag test, two XCUITests over new
  `DemoGesturePanContent`/`DemoGestureRotateContent` launch demos, and a
  written contract in `docs/ReferenceAppUX.md` §5.1.
- **Maneuver-text anchoring** (`15608f5`): banner and speech texts are now
  end-anchored at provider ingestion — a step's text names the maneuver at
  the step's end, the junction its countdown points at (the Google/OSRM
  "one maneuver ahead" banner), instead of naming the turn just performed.
  Applied to `mapquest-provider` (with a pin test over the recorded Berlin
  fixture), the trace-fixture generator and the committed fixtures, and the
  shared `reference-scenes.json` preview (the iOS resource copy re-synced).
  The offline routing fallback's geometry-derived step texts follow the same
  end-anchored convention (`08dc2f3`): interior steps name the junction the
  countdown points at, the final traversal span keeps its own turn/continue
  text, and the arrival step keeps its arrival line. The audit first
  reconciled every displayed distance, phrase, step flip and speech
  utterance from every anchor state (all correct — the numbers were never
  wrong; four permanent invariant tests pin them) and established the
  stale-text pairing as the whole finding; the composition layer and the
  frozen contract are untouched, and structured `ManeuverType` stays
  head-anchored (its roundabout residual folds into the parked
  maneuver-label-normalization ask).

### Changed (round 14 — benchmark job, PLAN shape)

- The informational benchmark job's first CI run failed: plain
  `cargo bench -p map-core` also runs the libtest unit-test binary, which
  rejects criterion's `--save-baseline` (555f616). The job now targets the
  criterion bench only. PLAN.md was homogenized to the milestone + checkbox
  shape with its maintenance rules documented in CLAUDE.md, and M9 (vector
  tiles) opened with the owner's direction that the OpenStreetMap US
  tileservice back the first provider slice.

### Added (round 13 — M7 offline slices 1–3, gesture seam, diagnostics, XCUITest)

- **M7 offline opens** (owner priority 2026-10-29). Slice 1 (`7b2c04e`):
  `map-offline-graph-compiler` — the `flatdata` crate was evaluated and
  rejected (its build script shells out to pip/PyPI — non-hermetic), so the
  crate pins its own minimal self-describing flatbuffer archive (format v1,
  `RMOG` magic + table of contents) implementing PRD §4.6's schema: nodes
  (quantized coordinates), edges (packed per-edge columns: road class,
  5 km/h speed cells, access bits, reserved-must-zero), lane guidance,
  instruction strings + lookup indices, turn restrictions. The compiler
  binary reads OpenStreetMap PBF extracts deterministically (byte-stable
  across runs), the committed 449-byte `routing_grid.osm.pbf` fixture is
  guarded by an always-on byte-equality test, and the fully validating
  in-memory reader answers typed corruption errors
  (`ArchiveMalformed`/`VersionUnsupported`/`IndexOutOfRange`).
- Slice 2 (`4cba046`): `MemoryMappedArchive` (the zero-copy native reader,
  mapping length/base-address alignment validated against the archive's
  record widths) and `ArchiveOfflineRoutingProvider` in `map-core` — the
  `geospatial-traits` `OfflineRoutingProvider` implementation routing with
  A\* over the graph's half-edge adjacency, an admissible haversine
  heuristic for both route strategies, a named 50 m snap tolerance, honors
  turn restrictions, and answers honest typed rejections (`ProviderRequestRejected`
  before load / for non-automobile modes / for disconnected pairs,
  `InvalidCoordinateInput` beyond snap tolerance). Wired into the map-core
  provider registry.
- Slice 3 (`7c5d872`): bearing-derived turn maneuvers — offline route steps
  now derive real `ManeuverType::Turn` kinds from the geometry's bearing
  change at each junction (named angle bands, straight/slight/standard/
  sharp/U-turn; texts stay name-free because the archive carries no street
  names; nothing fabricated), a route step is materialized per maneuver or
  class/speed change; two-way ways route correctly in both stored
  directions after fixing the adjacency build to the compiler's
  stored-pair contract; and `OfflineFallbackRoutingProvider`
  (`map-core/src/routing_fallback.rs`) — a `RoutingProvider` decorator that
  retries over the offline graph on primary failure, joins legs for
  multi-waypoint requests, and logs honest diagnostics; no UDL change was
  needed. Lane guidance stays unproduced by explicit refusal: the schema's
  counts-only record cannot carry per-lane permitted maneuvers, and
  fabricating them would lie (pinned by a test).
- The round-12 ruling's **provider-failure diagnostic seam** (`473fa66`):
  the frozen UDL grows addition-only
  `MapSdk::latest_provider_failure_diagnostics()` plus the
  `ProviderFailureDiagnostic` dictionary and
  `ProviderFailureClassification` enum; every failed foreign-callback
  provider call is recorded onto a bounded per-`MapSdk` ring (oldest-first
  non-destructive readback; capacity 32) with the contract and method it
  occurred on, the classification (classified through the round-11
  conversion's verbatim detail prefix — output text byte-identical) and the
  failure text. `latestProviderFailureDiagnostics` is verified in the
  regenerated Kotlin/Swift generation markers.
- The M3 open item **`CameraInteraction`** (`2213dcd`): the gesture-input
  camera seam in `map-rendering` — `pan_by_screen_pixels`,
  `zoom_by_levels_around_screen_point`,
  `rotate_by_degrees_around_screen_point` — reuses the camera's own
  clamping and carries an exact focal-hold correction (a pixel's ground-ray
  direction is independent of zoom/position; a zero delta answers the input
  coordinate verbatim). The web playground's pointer/wheel loop forwards
  its deltas through three new wasm pose helpers (documented in
  `contract_sync.mjs` as ahead-of-contract; they retire onto the UDL
  methods when M5 wires the native recognizers).
- Reference-app parity: the **RN demo-plan picker** plus eased
  follow-camera pitch (`2268fab` — Choreographer-driven smoothstep mirroring
  the Compose refapp; jest 6/6) and the **first XCUITest suite** (`25a6120`
  — 4 deterministic tests mirroring the Espresso coverage; the
  container-level `.accessibilityIdentifier` clobbering fact recorded with
  five `.accessibilityElement(children: .contain)` fixes).
- CI: the **ios platform-build workflow** (`cdce344`; macOS, XCFramework →
  SwiftPM package verify, no key) and the android platform-build workflow
  (`dec716f`; ubuntu, Linux-portable NDK host handling) — both green.

### Added (rounds 10 → 11 — reference-app parity slices, imagery-caching ruling, docs site)

- Tile imagery caches heavily by default per the owner ruling (2026-10-28,
  PRD §6.7/FR-C): `map-cache`'s tiles category default retention moves
  7 → **30 calendar days** (the common maximum under imagery-provider
  terms), `mapquest-provider` declares its own terms as the documented
  `MAPQUEST_RASTER_TILE_CACHE_CEILING_SECONDS` ceiling, and a lockstep test
  keeps the cache default exactly equal to the MapQuest terms ceiling. The
  per-category override (`CachedRasterTileProvider::with_time_to_live_seconds`
  and its siblings) stays the developer seam until the SDK-settings surface
  for per-provider TTL/budget lands as an addition-only contract slice.
- Round-10 reference-application slices land: the SwiftUI (`88f6892`) and
  Compose (`4819309`) apps gain the §5.3 course-follow 3D camera (5-fix
  median bearing smoothing, gesture break / fix re-engage, pre-trip restore),
  the five-control cluster (zoom ± with 250 ms hold-repeat, scale bar,
  my-location, 2D/3D, tap-to-renorth compass with the letter rotated against
  the bearing), the renderer-owned location indicator, the speech-instruction
  topic wired to the platform TTS with flush pacing, the idle-settled frame
  pump (skip serialized draws when `frameSettled`, bounded tile-detail
  resume), and the readiness-chip diagnostics readout backed by the SDK's
  recent-events ring. The Compose slice fixed three defects in its own diff
  en route (arrival-restore redraw, second-trip start stall, a `UInt` format
  crash in the progress readback); `:reference-application:testDebugUnitTest`
  runs 12 green against the shared `map-logics` helpers. The RN round-9
  shell slice closed as already-landed (`f84c833`), with its owner-decision
  gaps recorded for the ledger (ubrn 0.31.0-6 vs uniffi 0.32.2 checksum
  disagreement, wgpu's 2048 max-texture cap, APK dual Rust copies, flatDir
  prebuilt AAR without transitive resolution, no JS `BackHandler`).
- Round-11 web parity slice (`193a036`): the wasm playground reaches the
  same reference-app surface — the full control cluster and scale bar
  (fresh per frame from the new `MapCamera.ground_meters_per_screen_pixel`
  readback, a frozen-contract member), the location indicator over the
  native `set_location_indicator`/`clear_location_indicator` seam with the
  native validation rules, the follow-camera demo machine (smoothstep
  eases, exponential chase, bounded settled pump), and the diagnostics
  readout over a wasm ring subscriber mirroring the native sink's contract
  rules (512-event bounded rings, coordinate scrub at the sink, gate that
  never raises). The Playwright suite grows 20 → 31 (the shared
  `map_controls.mjs` glue module keeps page and tests on one constant
  source) and the five page-lifecycle bugs the slice surfaced mid-verification
  are fixed (module-scoped import crash, `hidden` vs `display:flex`,
  short-viewport dock occlusion, an invisible accent-on-accent primary
  toggle, and the indicator state hook's missing `visualKind`). Honest
  not-feasible list stays on the ledger: navigation/TBT and live speech
  await the wasm seam's `MapNavigationController` (a §12 open ask), and the
  compass docks in the end-edge column (§4.2's top-end slot is the readout
  dock at this breakpoint).
- Tile-cache settings surface (M2's remaining sub-item, addition-only):
  `MapSdk.setRasterTileCacheTimeToLive(seconds)` tunes the installed cache's
  retention live, and `MapSdk.rasterTileCacheTerms()` reads the effective
  posture (`TileCacheTerms`: effective TTL, the provider's declared ceiling
  or null, the category default). A value above the provider's ceiling is
  accepted with the ceiling clamping what the cache stores (a compliance
  bound on retention — PRD §6.7 — not a preference); zero is refused; the
  byte budget stays registration-time (the store's quota is fixed at open);
  re-registration resets the tuning. Five unit tests plus Kotlin/Swift
  generation markers.
- Ferrostar batch 1 (ideas-and-mechanism adoption over BSD-3 Ferrostar,
  Stadia Maps; the credit already in the notices): the step-advance reducer
  advances through many steps per fix in a bounded loop-until-break
  (stack-safety proven at 800 advances on one fix over a 1000-step route on
  a 256 KiB stack), so 20×+ trace replays and fast real fixes can no longer
  mis-skip the step ladder; the deviation detector now distinguishes
  `FullyOffRoute` (the unchanged freeze + reroute semantics) from
  `OffStepOnRoute` — off the current step but projecting onto a future step
  keeps navigation unfrozen, keeps the countdown, and reconciles to the
  projected step within one fix (the accuracy-aware additive widening gates
  both grades); while fully off route the composed instruction distance
  phrase is honestly suppressed (no phantom-snap countdown off a frozen
  projection; provider text stays verbatim). Deviation grades and the
  suppressed phrase are addition-only contract extensions with the
  round-9 pattern's generation markers.
- Ferrostar batch 2 (`c55e3a0`): speech emission now carries an
  **utterance identity** — every `SpeechInstruction` gains `utterance_id`,
  a monotonic controller-ordered id (Ferrostar B4's mechanism with an
  integer instead of a UUID; strictly increasing for the controller's
  lifetime, never reused across trips or reroutes, identical text
  re-emitted is a new id) so platform speakers can dedup and keep
  audio-focus decisions off the SDK; and the reroute-recalculation
  **hysteresis gates** (Ferrostar B3) land at the controller seam:
  `begin_reroute_calculation()` applies the request-in-flight gate, a 5 s
  cooldown, and a 50 m raw-fix movement threshold before a recalculation
  may start (`ReroutePolicy` tunable via `configure_reroute_policy`,
  defaults via `default_reroute_policy()`, state readable as
  `TripNavigationState.is_calculating_new_route`, settled by
  `provide_reroute` applied-or-ignored or `cancel_reroute_calculation`),
  so a jittery off-route bounce cannot spam the integrator's routing
  engine. The pure reducer stays clock-free; the round-9 "no reroute
  timestamp on the phase channel" gap is honestly closed with this seam
  (a timestamp on the payloadless phase enum is not addition-only).
  19 new tests; contract additions carry the round-9 generation markers.
- Product rebrand committed (`2cf3323`, `19eea68`): the product title settles
  on **Rustic Maps SDK** everywhere — display strings renamed platform-wide
  (`61b939b`), git remotes renamed to `angoratek/rustic-maps-sdk`, the public
  docs home created as `angoratek/rustic-maps-sdk-docs` (seeded `main`
  `6ee693f` with the site sources, both licenses, and a public-safe README),
  the publish script hardened to pin render-commit git identities from the
  source repo, and a `docs-publication` CI workflow (`13df22a`) that builds,
  curates, scrubs fail-closed, and stages the site on every push to the
  site sources. The first `gh-pages` render publishes behind the
  publish-gates pipeline from a clean gated tree.

### Added (round 12 — Ferrostar batch 3, the first Espresso suite, android platform CI)

- Ferrostar batch 3 (`c54c852`) closes the navigation engine's batch list
  (M6 done): **arrival-aware advance** (B5) — `NavigationEvent::StartTrip`
  carries an optional dedicated arrival step-advance condition that governs
  advance once the remaining steps (current included) drop to ≤ 2, matching
  Ferrostar's separate arrival arm; `None` exactly preserves prior behavior,
  all existing call sites unchanged; **`CourseFiltering::SnapToRoute`** (B7)
  — a new pure course-filtering module where snap-to-route reports the
  winning route segment's bearing (measured in the same equirectangular
  local plane as the projection; `PolylineSnap` gained `bearing_degrees`),
  falling back to the reported course and then the trip's last effective
  heading, with `Raw` as the verbatim default escape hatch
  (`TripNavigationState.course_heading_degrees` is stamped natively under
  the trip's policy and intentionally invisible to FFI this round); and the
  **`NavigationRecorder`** (B8, internal-only) — a bounded 1024-entry ring
  recording topic state changes plus an explicit raw-fix channel under one
  monotonic sequence, with a draining read for debugging. No UDL or
  platform-binding changes, so no regeneration runs were needed.
- The first UI-automation suite lands (`2b2dcaf`): a 4-test Espresso
  `androidTest` class over the Compose reference app's demo machinery — the
  readiness ladder settling at peek, the demo-plan chip driving the §5.1
  fit contract and overlay push counts, the trace replay rendering the
  recorded banner instructions and exiting with a verbatim pre-trip camera
  restore, and the offline card reflecting registration and live tile
  accounting. Deterministic by design (Compose snapshot-state polling, all
  timeout values as named constants, network-free — committed fixtures
  only), each test exercising the same user-path methods the UI drives. A
  small automation readback seam on `MainActivity` mirrors the iOS
  refapp's launch-automation pattern, and the connected-test teardown's
  silent uninstall is transparently undone so the device stays in its
  mandated state.
- The android platform-build CI workflow (`dec716f`): an ubuntu job with no
  emulator cross-builds the Rust cdylibs, regenerates the Kotlin bindings,
  assembles the AAR, runs the JVM smoke, and assembles the reference app's
  debug APK — no API key involved, by design (the empty key is the honest
  failed readiness). Landing it surfaced two macOS-only assumptions in
  `scripts/build-android.sh`, now fixed portably: the NDK prebuilt host
  glob matches both `darwin-*` and `linux-x86_64`, and the host cdylib
  suffix follows the host OS (`.dylib` / `.so`).

### Added (round 11 — the React Native reference application)

- The RN app shell reaches the reference apps' v2 surface on device
  (platform/rn only; two RN agent slices verified on the Pixel_9 emulator):
  the round-9 shell already had the map render through the TurboModule + the
  native surface seam (the minimal JNI shim, the serialized per-frame draw
  pump, the readiness chip + diagnostics with the compiled-version probe and
  per-draw tile/frame statistics, camera-preset chips, the guided-walk demo
  over the real `MapNavigationController`); the wave-3 increment adds the
  §4.1 search-first plan flow (assignment-targeted origin/destination over
  the real geocoding, fitted route preview, summary card), the §7.1 turn
  banner + §7.2 drawer with simulated trips at the demo acceleration, the
  §3 explicit-owned theme toggle row, the offline tile-cache card over
  `registerMapquestProvidersWithTileCache`, the §9 demo-scene chips over the
  shared artifact, and the §6.2 JS `BackHandler` closing the sheet state
  machine and exiting only at peek. Fixes found by driving the emulator: the
  attach race (a stale retry flag kept the frame pump from a real
  `tryAttach`), the plan-state emission timing (a single emit in `finally`),
  and the back-handler desync. Gates: `tsc --noEmit` clean, jest 4/4,
  `:app:assembleDebug` verified by a full CMake reconfigure of the
  TurboModule link from a wiped cache.
- The RN package ignores were anchored so the app shell's own sources are
  versioned: the unanchored patterns silently un-versioned the shell (a
  fresh clone got an `App.tsx` whose `./src/*` imports do not exist). The
  TurboModule library's generated output keeps its documented ignore, now
  anchored to the package root with the intent spelled out.
- `scripts/build-rn.sh` owns the TurboModule's app-build inputs
  reproducibly now: the regenerated `android/CMakeLists.txt` re-derives its
  `libandroid.so` link line idempotently (the `ANativeWindow_*` symbols the
  statically linked Rust library's wgpu GLES backend needs — a clean CMake
  build fails to link without it, and a warm `.cxx` cache masks the gap for
  exactly one generation), and the arm64-v8a Rust static archive is built
  with the AAR pipeline's NDK toolchain environment instead of depending on
  a hand-copied artifact.
- Honest notes: speech stays a documented no-op in this consumer (the
  platform speech slices are the Compose/iOS seams), the follow camera has
  no pitch easing, and a Metro dev-only double-registration invariant on hot
  updates was diagnosed (a Fabric hot-update artifact, not app code).

### Hardened (round 11 — FFI panic-boundary verification, no-unwrap deny)

- The cross-cutting FFI-hardening task (PLAN §2) is done as a verification
  plus honest-edges task: with `uniffi 0.32.2` pinned, the generated
  scaffolding was read and **already contains every Rust-panic FFI
  crossing** — sync exported frames run lift, dispatch and result lowering
  inside `rust_call`, whose caught panic resolves as the
  `UnexpectedError` call status carrying the panic message (a nested
  defensive catch covers the message lowering); async exports wrap the
  future so a poll panic resolves the exported future instead of tearing
  the handback; object clone/free wrappers are contained the same way. No
  redundant `catch_unwind` layer was added on top; the verified containment
  map is documented in `map-native-core/src/panic_boundary.rs`, which also
  holds the crate's honest handlers for the remaining deliberate panics
  (poisoned state locks naming the lock, internal-invariant asserts for
  constructions validation makes unreachable). All non-test
  `unwrap`/`expect` uses in FFI-reachable code were replaced with these
  handlers across `map-native-core` (navigation, provider registry,
  surface view, diagnostics, runtime, platform surface) and the policy is
  enforced by `clippy::unwrap_used`/`clippy::expect_used` deny at both
  `map-native-core` and `map-navigation` (the broadcaster's four poisoning
  asserts and the reducer's current-step invariant keep inline justified
  allows). An addition-only Rust-side conversion
  (`From<uniffi::UnexpectedUniFFICallbackError> for SdkError` →
  `ProviderRequestRejected`) keeps a foreign provider implementation
  failing outside its typed contract a typed failure instead of the
  untyped panic route; the frozen UDL is untouched. Four FFI-boundary pin
  tests (`map-native-core/tests/panic_boundary.rs`) drive the generated
  symbols directly: the sync pin, a `Success`-status control, the async
  resurfacing through the documented runtime handback, and the typed
  mapping.

### Added (round 9 — contract additions, camera readback, tile statistics, offline cache, unified labels, v2 SwiftUI refapp), v2 Compose refapp

- `map-native-core`'s `MapCamera` attribute getters (`center`, `zoom_level`,
  `bearing_degrees`, `pitch_degrees`) reading the constructed state back; bearing
  wraps (400° → 40°) and the fit-bounds readback is verified (max zoom 12.0,
  pitch/bearing zero, center inside the framed box). The UDL grammar has no
  `[Attribute]` member (verified against `uniffi_udl-0.32.2` sources), so the getters
  are plain methods.
- Per-draw tile statistics: `map-rendering` grows `SurfaceFrameStatistics`
  (every `TileReadiness` value classified, overlay triangles counted from the
  vertex stream) returned by `render_frame`, and the UDL record `TileFrameStatistics`
  is returned by `MapSurfaceView.draw` (now `[Async, Throws=SdkError]`). Native-only
  this round: the TS wasm mirror is deliberately unchanged and `contract_sync.mjs`
  stays green (its frozen-name checks are addition-tolerant, wasm-must-not-invent
  preserved).
- `MapSdk.register_mapquest_providers_with_tile_cache(path, bytes)` installing
  `CachedRasterTileProvider` over `SqliteResponseCache` with a
  `CacheQuota { absolute_maximum_bytes: Some(budget) }` — the offline-maps demo
  substrate on the native seams (unusable input → `SdkError::InvalidCoordinateInput`;
  re-registration replaces the installed slot).
- Maneuver labels **unified** (owner decision, 2026-10-28): `ManeuverType::stable_label()`
  / `from_stable_label()` — one stable label set (`departure`, `turn-left`,
  `roundabout-2`, …) now drives both the serde `Serialize`/`Deserialize` impls and the
  FFI text conversion (`maneuver_type_text` / `native_maneuver_type_from_text`; unknown
  label → `SdkError::InvalidNavigationInput` naming the label). The three committed
  trace fixtures regenerate to the flat label arrays, and the platform refapp loaders
  pass labels through verbatim (their translation shims are deleted).
  Internationalization/multiple languages is a later, relevant-only backlog item
  (MapQuest `locale` routing parameter as the provider seam; other providers have
  equivalent support).
- The Compose reference application reaches the v2 consumer revision: the
  home sheet with grabber state machine, the search → route-summary plan flow
  over real geocoding, the navigation drawer with the turn-banner stack, the
  demo-plan picker over the three committed traces (clean walk, deviation
  recovery, deviation reroute — the reroute fixture's replacement route
  applies through `provideReroute`), the readiness chip → diagnostics sheet
  (camera-getter readback + live `TileFrameStatistics`), the offline maps card
  (`registerMapquestProvidersWithTileCache`, 16 MiB), verbatim-rule-compliant
  speech through a stop+flush `TextToSpeech` engine, and a navigation
  follow-zoom constant. Verified on the Pixel_9 emulator (relaunched windowed);
  evidence under `tmp/refapp-android-round9/`.
- Generation-marker tests pin the new Kotlin and Swift spellings of the
  tile-cache registration, the camera getters, and the statistics-returning draw.
- The SwiftUI reference application reaches the v2 consumer revision: the home
  sheet (peek/half/full, grabber fold), the search → route-summary plan flow,
  the navigation drawer (ETA line, phase, voice toggle, exit; arrival waits for
  Done), the readiness chip → diagnostics sheet with live camera-getter
  readback (the "no zoom readback" note is obsolete), demo-scene chips, the
  offline maps card (`registerMapquestProvidersWithTileCache`, 16 MiB) with a
  live `TileFrameStatistics` line, verbatim rule-compliant speech through a
  flush-before-speak `AVSpeechSynthesizer` engine, a named navigation
  follow-zoom constant, and extended launch-argument automation. Verified on
  the iPhone 17 Pro simulator; evidence under `tmp/refapp-ios-round9/`.

### Fixed (CI)

- Headless WebGPU on Linux CI runners: the Playwright launch flags now request
  Dawn's Vulkan backend (`--enable-features=Vulkan --use-vulkan=swiftshader`) —
  without them the adapter initialized (`renderer_mode` reported `gpu`) but the
  frame loop stalled: golden probes never settled, pixel checks read zero painted
  pixels, frame-waits expired (9/20 wasm-e2e failures on the round-8 pushes).
  The flags are inert on macOS hosts (MoltenVK default); local suite 20/20 with
  them applied.

### Added (CI: git hooks)

- Git hooks in the tracked `scripts/git-hooks/` directory, installed repo-locally via
  `scripts/install-git-hooks` (`git config core.hooksPath scripts/git-hooks`): a
  pre-commit `cargo fmt --all --check` and a pre-push gate running the three CI checks
  (`cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`), fast-failing with per-gate headers. Either hook is bypassed
  for one command with `MAPS_SDK_SKIP_HOOKS=1`.

### Fixed (CI)

- `map-native-core`'s platform surface lowering compiled only under a platform `cfg`:
  on the Linux CI runners (no macOS/iOS/Android branch) the ungated
  `RawDisplayHandle`/`RawWindowHandle` import and the `this_platform_pointer` /
  `validated_platform_pointer` helpers were dead (three `-D warnings` clippy failures),
  and the lowering tests assumed a macOS-style host (the foreign-pointer test failed on
  Linux). The import and helpers now carry the platform branches' exact cfg, and the
  tests are target-aware: a branch target lowers its own pointer and refuses a foreign
  one with `InvalidRenderInput`, a no-branch target refuses any handle with
  `RenderingSurfaceUnavailable` (previously untested on any target).

### Added (M3 — rendering core, GPU-free slice)

- Root `README.md`: feature overview, crate map, build/test/wasm-e2e instructions, deployment notes, contribution guide.

- `map-rendering`: web-mercator projection module (normalized world space, XYZ tile addressing with boundary-epsilon edges, `tiles_covering`, zoom scale).
- Anchored `MapCamera` (center/zoom/bearing/pitch) projecting ground magnification exactly to the zoom scale; screen↔ground unprojection with horizon/far-clip rejection; longitude wraps to the nearest world copy across the antimeridian. Camera math on `glam` f64 matrices.
- `CameraAnimation` / `interpolate_camera`: shortest mercator path for the center, shortest bearing arc, exponential-zoom-compatible lerp, smoothstep easing; endpoints return the input verbatim.
- `TileTexturePool`: byte-budget LRU pool of encoded tile payloads.
- `TileScheduler`: per-frame tile resolution against a `RasterTileProvider` — duplicate requests cost one fetch, most-detailed-first fetch order, per-frame fetch budget (`Deferred`), parent fallback to the nearest pooled ancestor (`FallsBackTo`), per-frame `Unavailable` with later-frame retry.
- 27 tests including projection round-trip properties and camera screen↔ground lossless round-trip properties (zoom 3–18, bearing, pitch ≤ 55°).
- Camera-frustum tile listing: `MapCamera::visible_tile_keys` (view frustum clipped against the ground plane by half-space clipping, antimeridian world-copy wrap, row-major) and `tile_zoom_level` for fractional-zoom fetch selection.
- Fixes to test stability: per-test SQLite fixture paths in `map-cache` cached-provider tests (parallel runs previously raced on one temp database); a camera property test no longer unwraps `screen_to_ground_coordinate` for ground points legitimately past the far clip plane.
- Coding convention: no magic numbers — all static values used in logic live in named, documented constants; applied across `map-rendering`'s camera and projection code.

### Added (M8 — wasm browser loop)

- `map-web-bindings` (new crate, cdylib): hand-reviewed `wasm_bindgen` JavaScript surface mirroring the native camera/projection API (`MapCamera` with `project_to_screen` / `screen_to_ground_coordinate` / `visible_tile_keys` / `tile_zoom_level`, plus `coordinate_to_normalized_world`, `tile_key_for_coordinate`, `tiles_covering`, `get_sdk_compiled_version`, and a `start_sdk` readiness promise). Validation plumbing shared by wasm exports and host unit tests; no UniFFI on wasm — the UDL contract stays the single source of truth (PRD §4.7).
- `platform/web`: dependency-free static server (127.0.0.1:8642), readiness-gated ES-module loader, and 4 Playwright chromium e2e tests (compiled-version check, anchor-centered projection, project↔unproject round-trip tolerances, antimeridian tile wrap at longitude 179.95).
- Interactive playground page (`platform/web/index.html`): drag-pan via ground-coordinate deltas in normalized world space, wheel zoom, bearing/pitch sliders, and a live frustum tile-grid overlay driven by `visible_tile_keys` at the camera's fetch zoom.
- WebGPU map canvas (`GpuMapCanvas` in `map-web-bindings`, wasm32-only): renders the recorded raster tile fixtures into the browser canvas through wgpu — camera-driven tile listing, same-origin `/tiles` fixture route in `platform/web/serve.mjs` (no external network fetches; fetch zoom clamped to the fixtures' maximum), `map-rendering` scheduler feeding quads assembled by `Renderer::frame_quads`/`quad_fragment_vertices`, per-(tile, revision) texture uploads with an LRU texture cache, and the headless pipeline's WGSL/vertex/index layout mirrored; the playground degrades to the 2D tile-grid overlay on a separate canvas without browser WebGPU, with the readout surfacing renderer mode, frame tile statistics, and fetch zoom. Two new Playwright tests (WebGPU smoke with pixel probe; camera-drag readout sync) bring the suite to 6.
- Canvas-vs-golden browser compare (M8 slice 3): `platform/web/golden.html` + `golden.mjs` render the two committed golden cameras on a fixed 512×512 wasm WebGPU canvas, decode the golden PNGs same-origin over a new allow-listed `/golden/{file}.png` route in `serve.mjs`, and diff rendered against golden bytes fully in-page; `tests/golden_compare.spec.mjs` asserts `golden_image.rs`'s mirrored tolerances (mean absolute per-channel error ≤ 2.0, outlier fraction ≤ 0.02 above channel error 24) on the minimum-error sample across 12 animation frames after a forced re-present. Measured parity on the headless SwiftShader chromium run: top-down mean 0.0739 / outlier 0.00000, pitched 45° mean 0.0431 / outlier 0.00000. New test-support export `GpuMapCanvas::stage_tile_parent_fallback` (withdraws a requested tile's own fixture and pools its coarser parent, mirroring the native golden harness's `fill_pool_and_prepare`) so the browser reproduces the reference imagery byte-for-byte; `platform/web/pkg` regenerated with pinned `wasm-bindgen-cli 0.2.129`. Suite green at 9 Playwright e2e tests.

### Added (M3 — headless wgpu renderer, raster slice 1)

- `map-rendering` headless renderer (native-only module): surfaceless `GraphicsContext` acquisition behind the object-safe `GraphicsContextSource` seam (`WgpuGraphicsContextSource`, pollster-blocked; wgpu 30 API), a textured-quad `RenderPipeline` taking pre-computed clip-space vertices, and `capture_frame_bytes` readback (256-byte row copy alignment stripped, blocking Wait-poll map flush).
- `Renderer::frame_quads` / `quad_fragment_vertices`: GPU-free assembly — present tiles draw their full texture, fallback draws sample the pooled ancestor's covering window over the requested child's span; view-projection built once per pass; conservative viewport culling that never drops near-plane-straddling fragments; antimeridian spans wrap as one unit and split into at most two world-copy fragments with proportionally split texture windows.
- Graceful degradation: an undecodable pooled payload skips its quad, is evicted (`TileTexturePool::evict`) for scheduler refetch, and never fails the frame; `HeadlessRenderError::GraphicsContextUnavailable` is distinguishable from per-frame capture failures.
- Golden-image tests over recorded platform imagery: `examples/record_golden_tiles.rs` records the two Zurich scenes' tiles (zoom 13–14, 256-px platform texels upscaled to the 512-px viewport quad) into `fixtures/render/tiles/`; tests load from disk only, missing tiles are hard errors naming the record command, and both goldens are tolerance-compared (verified 0.0000 mean channel error on Metal).

### Added (M4 slice 3 — async provider surface)

- `map-native-core`: the frozen `MapSdk` UDL object exposing the implemented `geospatial-traits` contracts (geocoding forward/reverse, routing, distance matrix, place search, traffic incidents, traffic flow, raster tiles) as per-member `[Async, Throws=SdkError]` methods resolved through the `map-core` `ProviderRegistry`; 9 mirrored enums + 11 mirrored records of `geospatial-types` entered the frozen contract; `SdkError` grew the five provider failure variants (`NetworkRequestFailed`, `ResponseParsingFailure`, `ProviderRequestRejected`, `RateLimitExhausted`, `ProviderNotConfigured`). Provider registration stays native (`MapSdk::register_*` — MapQuest-from-key wiring stays with the integrator; foreign-callback provider injection follows in the callback-interfaces slice); the async surface needs no Rust executor — the FFI Rust-future machinery polls it from the foreign event loop, and the registry lock never spans an await so every exported future stays `Send`. Elevation/offline-routing methods enter with their first concrete provider implementations (M6/M7). 13 end-to-end mock tests + 7 new Kotlin generation markers (`suspend fun` lowering verified).

### Added (M4 — UniFFI binding layer, slice 1)

- `map-native-core`: frozen UDL contract (`src/map_native_core.udl`) for the camera/projection surface — `MapCamera` object (`project_to_screen`, `screen_to_ground_coordinate`, `visible_tile_keys`, `tile_zoom_level`), the free projection helpers and `get_sdk_compiled_version` mirroring the wasm surface one-to-one, six records, and the fielded `SdkError` carrying only the real `InvalidCoordinateInput` variant with verbatim detail. Build-time `generate_scaffolding` and the `uniffi-bindgen` CLI pinned to `uniffi 0.32.2`; Rust implementation validates every input and delegates to `map-rendering` via explicit record converters (FFI traits cannot be implemented for foreign-crate types); 23 unit tests plus an in-process Kotlin bindings-generation test green.

### Added (M4 slice 4 — foreign-callback provider surface)

- `map-native-core`: the frozen contract's first `[Trait, WithForeign]` interfaces — `GeocodingProvider` (returns `sequence<PlaceResult>`) and `RasterTileProvider` (returns the byte-payload `RasterTileData`) — foreign (Kotlin/Swift) code implements to serve provider calls, registered on `MapSdk` through the new sync `register_geocoding_provider_from_foreign` / `register_raster_tile_provider_from_foreign` members into the same registry slots as native providers (plain contract names: no collision in the namespace, and the names a Kotlin implementor reads first). Uniffi 0.32.2 verdicts recorded in the frozen header: per-member `[Async, Throws]` lowers to foreign-vtable entries (interface-level `[Async]` stays rejected); the UDL placeholder traits are discarded by the macro path, so the binding crate defines the real traits — matching signatures, `Send + Sync` supertraits, and the `#[doc(hidden)] uniffi_foreign_handle` default required by the UDL lowering; `async-trait` becomes a direct binding-crate dependency. `foreign_providers.rs` holds the exported callback traits and the `Foreign*Provider` adapters that convert binding records ↔ native types and re-raise the callback's `SdkError` as the core error via an exact, round-trip-tested `From<SdkError> for SDKError`. 5 foreign-callback e2e tests (forward geocode, reverse echo, raster payload, both error paths) + 4 Kotlin-generation markers (`suspend fun` callback shape verified); 52 crate tests green.

### Added (M4 — wasm TS surface + contract-sync test)

- `platform/web`: hand-reviewed TypeScript mirror `types/map_native_core.d.ts` of the frozen contract's camera/projection dictionaries (`GeographicCoordinate`, `BoundingBox`, `ScreenPoint`, `NormalizedWorldCoordinate`, `TileKeyRecord`, `ViewportPixels`, `EdgeInsetsPixels`) and the `SdkError` discriminated union, with the UDL named as the single source of truth (lower-camel field names matching the wasm runtime objects; the async provider slice deliberately not mirrored — no wasm exposure yet).
- `platform/web/tests/contract_sync.mjs`: a dependency-free `node --test` contract-sync test that parses the frozen UDL (small line reader, no npm additions) and asserts the wasm surface mirrors it: frozen dictionaries agree field-and-type-per-field with the TypeScript mirror, `SdkError` variants agree one-to-one, the frozen `MapCamera` methods and free helpers exist in both the UDL and the wasm export sources (`exports.rs`, plus the generated `pkg` declarations whenever present), and the wasm surface invents nothing the contract lacks — addition-tolerant so the contract can grow without breaking the test (documented wasm-only lifecycle helpers excepted). Negative drift verified with four simulated drifts; wired as a Playwright `globalSetup` so every suite run re-asserts parity (microsecond cost, no server involvement); documented in `docs/reference/BrowserLoop.md`.

### Added (M6 slice 2 — deviation detection + reroute events)

- `map-navigation`: `DeviationDetector` seam (Ferrostar's `NavState` pattern — the live `Arc<dyn DeviationDetector>` sits beside the plain-data `TripNavigationState` in `NavigationSession`) with `DistanceDeviationDetector` — raw-fix-to-remaining-route-line distance judged against `DEFAULT_DEVIATION_DISTANCE_THRESHOLD_METERS` (25 m), widened additively by the fix's `horizontal_accuracy_meters` (accuracy-aware instead of Ferrostar's hard accuracy gate, because a 25 m excursion plus a 50 m envelope should not read 75 m off route); no accuracy report means a fix can neither raise nor clear a deviation (one threshold cannot conservatively decide both directions). `NavigationPhase::OffRoute` freezes progress Ferrostar-style: a deviating fix never snaps, recomputes progress, or consults the advance condition; a graded fix back inside the allowance recovers and clears the recorded `DeviationInfo · deviation_from_route_meters`. `NavigationEvent::RerouteProvided { route, step_advance_condition }` rebuilds the session for a route the integrator anchors at the current snapped coordinate (remote-first / offline-fallback policy stays at the integrator layer, keeping the reducer policy-free); arrival-phase reroutes are ignored; the handed-in condition and the session's detector are defensively `fresh_instance`-rebuilt (configuration persists, tracking state restarts). Or/And `StepAdvanceCondition` combinators added (both inner conditions consulted on every update; `fresh_instance` rebuilds both children fresh). Hysteresis (recovery threshold / minimum off-route dwell) deliberately deferred: Ferrostar's ported reference carries none, and this slice's detector is deliberately stateless. Observers and UDL wiring remain later slices. 56 tests green (+26: detector threshold/accuracy-margin cases, freeze/recovery/reroute reducer cases, combinator fresh-instance and child-state contracts, plus a property asserting each fix's off-route flag tracks that fix's own verdict along a manual trip).

### Added (M6 slice 1 — navigation ground floor)

- `map-navigation` (previously only a crate-root doc): pure event reducer `reduce_navigation_event` over `NavigationEvent::{StartTrip, LocationUpdated}` producing `NavigationSession` — the plain-data, FFI-safe `TripNavigationState` (phase `Navigating`/`Arrived`, traversal-ordered flattened step positions per leg, `TripProgress`, optional snapped coordinate) paired with the live `Arc<dyn StepAdvanceCondition>` outside the record (Ferrostar's NavState pairing). Polyline snapping in `geometry.rs` (per-segment equirectangular projection with documented error characteristics, endpoint clamping, exact haversine distance). Conditions: `ManualStepAdvanceCondition`, `DistanceToEndOfStepCondition` (eager, 20 m / 15 m accuracy defaults), `DistanceFromStepCondition` (delayed, 15 m), and the `fresh_instance` reset contract that makes stateful advance logic double-advance-proof. 30 tests green (conditions, snapping incl. idempotency property, reducer walker with monotonic-progress property). Deviation/rerouting, observers and UDL wiring are later slices.

### Added (M3 follow-ups: GPU texture cache, bounds fitting, route strategy)

- `map-rendering` `GpuTileTextureCache`: byte-bounded LRU cache of tiles already uploaded to GPU memory, keyed by tile and stamped with the pool's new `payload_revision` so replaced, reinserted, or evicted payloads can never draw stale pixels; every frame prunes invalidated uploads. Default budget 64 MiB (one 16 × 16 grid of 256-px RGBA8 tiles); GPU tests prove repeat frames re-upload nothing, replaced payloads upload once more, and a shrunken budget evicts least-recently-used entries immediately.
- `map-rendering` `fit_bounds` + `EdgeInsetsPixels`: top-down, north-up camera fitting a `BoundingBox` into the viewport's inset region — largest zoom keeping the whole box inside the insets (clamped to a caller maximum), center anchored on the inset center under asymmetric insets so UI cards never cover fitted content; rejects non-finite inputs and boxes without positive world area instead of producing silent all-NaN cameras.
- `geospatial-types` `RouteStrategy` (`fastest`/`shortest`) on `RouteOptions`; MapQuest automobile routing maps it onto `routeType` (other modes keep their cost model), while route-distance matrices keep the fastest default; cached fingerprints distinguish strategies so routing results never share a cache entry across strategies.

### Fixed (M3 — renderer review round)

- Cache-hit draws now refresh the pool's payload recency: previously a steady camera's pool hits let lookups lapse, so the pool's LRU evicted exactly the tiles whose textures were already on the GPU, churned refetches, and could punch holes in the map (regression test with decoy scheduler traffic).
- An upload whose bytes the pool evicted (not replaced) is pruned from the GPU cache at the next frame without a phantom upload; a corrupt refreshed payload is evicted from the pool and never served from the cache; size reductions of the GPU budget evict immediately instead of on the next insert.
- A frame whose every payload is missing renders as the clear color instead of panicking inside `wgpu` on empty vertex/index buffers.
- Research cross-survey of modern map/navigation SDKs distilled into a per-crate prioritized backlog (`docs/research/modern-sdk-inspiration.md`), feeding M5–M7 planning.

## [0.1.0] — 2026-10-27

### Added

- PRD 0.2 and milestone plan (PLAN.md) locking the architecture: UniFFI UDL-first bindings, own minimal `wgpu` raster renderer, custom flatdata offline routing graph, WebAssembly as a first-class target.
- M0 — Foundations: Cargo workspace with all nine crates from PRD §3.
- `geospatial-types`: coordinate/bounding-box records with haversine math (proptest suites), route/leg/step/lane-guidance/distance-matrix records, place, traffic, elevation, raster-tile records, and the typed `SDKError` model.
- `geospatial-traits`: nine vendor-neutral provider contracts (geocoding, routing, distance matrix, search, traffic incidents, traffic flow, elevation, raster tiles, offline routing) with compile-time mocks used by downstream tests.
- `map-core`: typed provider registry and the cache-then-network orchestration seam (typed serialization; cache storage degrades gracefully on write failure).
- CI: format-and-lint (rustfmt + clippy `-D warnings`) and test-rust-core (nextest) workflows; cargo-deny license gate configured.

## [0.1.2] — 2026-10-27

### Added (M2 — disk cache)

- `map-cache`: SQLite-backed response cache — order-independent SHA-256 `RequestDescriptor` fingerprints (category-tagged `"<category>:<hash>"`), per-category TTL defaults (tiles 7 d … flow 30 s) with per-call override, lazy TTL expiry, and least-recently-used eviction on write.
- Pooled WAL-mode connections with `BEGIN IMMEDIATE` + `SAVEPOINT`-guarded writes; LRU ordering via a monotonic `recency_sequence` (second-granularity wall-clock timestamps tie under burst traffic).
- `CacheQuota`: absolute byte cap and/or free-disk share resolved through an injectable `DiskSpaceProbe` (real fs2-backed probe plus a synthetic fixed probe for tests).
- Cache-then-network wrappers (`Cached*Provider`) for all seven implemented provider contracts around `map-core::execute_cache_then_network`; tests prove one network call per distinct fingerprint.

## [0.1.1] — 2026-10-27

### Added (M1 — MapQuest provider)

- `mapquest-provider`: shared rate-limited HTTP client (token-bucket limiter, API-key injection, bounded idempotent-GET retries, `info.statuscode` → `SDKError` mapping; HTTP 401 key rejection and 429 handling verified against the live platform).
- Implementations for seven documented MapQuest APIs: Directions v2 route, Directions v2 route matrix (`options.allToAll`, verified live), Geocoding v1 address + reverse, Search Ahead v3 prediction, Traffic v2 incidents, Traffic v2 JSON flow, Tiles API raster XYZ tiles.
- Fixtures recorded from the live platform under `fixtures/mapquest/`; wiremock end-to-end tests replay them for success, application-level statuscode, and transport-failure paths.
- `examples/live_smoke`: manual live-platform smoke run; early UDL contract draft under `docs/udl/contracts-draft.udl`.

### Notes

- The Elevation API is retired (absent from current MapQuest documentation); `ElevationProvider` therefore has no MapQuest implementation and its registry slot stays unconfigured.
- The Traffic API serves flow as an image by default, but `traffic/v2/flow?outFormat=json` — the call the mapquest-js `trafficLayer` bundle makes — returns a GeoJSON collection of congestion polylines. `TrafficFlowData` consequently carries the segment shape and congestion level with speeds optional (`current_speed_kilometers_per_hour` / `free_flow_speed_kilometers_per_hour` are `None` for MapQuest's color-bucketed flow).
- MapQuest payload facts anchored from live data: `info.statuscode` errors arrive as HTTP 200; turnType table (1 slight right … 9 left u-turn, `-1` arrival), arrivals also marked by the final maneuver; route shape split by `shape.maneuverIndexes`; route-matrix distances in the `options.unit`; incidents `type 3` is a flow condition, not a disruption; flow `properties.traffic` buckets `green`/`yellow`/`red`/`black` map to free-flow/slow/congested/blocked; the flow service caps covered area at one degree in either direction (`mapWidth`/`mapHeight`/`mapScale` jointly).
### Added (retina tile resolution)

- `geospatial-types` `RasterTileResolution` (`standard`/`retina`) on `RasterTileData` (serde-defaulted for pre-existing cache records); `RasterTileProvider::fetch_raster_tile` carries the choice, the Tiles API provider maps it onto the documented `256`/`256@2x` size segment (accepted, verified live), and cached tile fingerprints distinguish resolutions.
- `map-rendering` `TileScheduler::new` gains `requested_tile_resolution` (renderer stays resolution-agnostic): a retina texture simply carries doubled texels per logical tile span.
- Wider recorded Zurich playground footprint (`record_playground_tiles` example): 21 standard tiles (6 × 5 at zoom 14) + 12 retina `…@2x` fixtures (~2.2 MB total); the browser canvas picks between them by the device-pixel ratio against the wasm-reported `retina_device_pixel_ratio_threshold()` (2.0), the frame stats expose `tileResolution`, `serve.mjs` serves the `@2x` marker, and a Playwright test asserts the retina fetch flow on a doubled-ratio context — 7/7 green.

### Added (M3/M4 follow-ups: fly-to arc, fit-bounds binding surface)

- `map-rendering` `fly_to_animation` (`FlyToOptions`): camera flights following the leading GL JS implementation's hyperbolic fly-to arc ported onto the anchored mercator camera — long travels zoom out below the endpoints' levels and zoom back in on arrival, zoom-only flights are monotonic, identical cameras resolve instantly; the duration derives from the arc length over `travel_speed` (default 1.2, curve ρ = 1.42, and ρ self-limits so the arc never dips past the minimum zoom). `CameraAnimation` now carries its pose curve (`MercatorSmoothstep` preserves today's behavior, `FlyToArc` the new arc) and exposes `duration_seconds`.
- `map-native-core` slice-2 contract extension: `EdgeInsetsPixels` dictionary and a named throwing `[Throws=SdkError, Name=fit_bounds]` `MapCamera` constructor in `map_native_core.udl` (Kotlin `MapCamera.fitBounds` companion, Swift factory; verbatim error detail preserved), delegating to the native `fit_bounds` with the slice-1 validation path.
- `docs/udl/contracts-draft.udl`: `EdgeInsetsPixels` added; interface-level `[Async]` removed — uniffi 0.32.2's UDL parser rejects it, provider `[Async]` belongs on members only.
- `docs/reference/BrowserLoop.md`: maintainer guide to the wasm workflow (pkg build with version-matched `wasm-bindgen-cli`, serving and manual playground testing, Playwright e2e, WebGPU facts and the `drawImage`-after-present quirk, 2D-overlay fallback, the native-parity rule mirroring WGSL/vertex/index layout).
- Browser evaluation: current Playwright WebKit (27.2) ships WebGPU — the harness renders the GPU imagery directly on it; the 2D-overlay fallback fires only when adapter/device acquisition fails.

### Added (M6 slice 3 — instruction emission + navigation observers)

- `map-navigation` `compose_navigation_instructions(&TripNavigationState, UnitSystem) -> NavigationInstruction`: active and upcoming maneuver instructions with a per-unit-system distance phrase (metric/imperial bands, band boundaries unit-tested with named constants), provider-authored display/speech text never rewritten — composition only prepends a distance phrase and falls back to the provider text verbatim when nothing remains — roundabout exit index surfaced as `Option<u8>`, active-step lane guidance promoted as-is, and an explicit `Unavailable` variant for steps without provider-authored text.
- `map-navigation` `NavigationStateBroadcaster`: Mapbox-style per-topic observers (`NavigationPhaseChange`, `DeviationStatusChange`) with explicit `TopicSubscription::cancel()` (snapshot cancel: events already in flight are still delivered), single-in-flight dispatch with `SuppressedReentrant` reported on re-entrant subscribe/cancel from inside a listener, and no lock held across a callback.

### Added (M3 — overlay rendering)

- `map-rendering` `overlay.rs`: `MapOverlay` layers (route polyline, filled polygon, point markers, traffic-flow congestion coloring) rendered into a clip-space vertex stream (`ClipSpaceOverlayVertex`) — screen-space line thickening with butt caps, ear-clip polygon triangulation, octagon-fan marker geometry, four congestion-level colors, zoom-consistent pixel widths, and antimeridian world-copy windows so spans stay visible at zoom 0.
- Second `wgpu` pipeline in the headless renderer (`render_captured_frame_with_overlays`; plain `render_captured_frame` delegates with an empty overlay slice) with alpha blending over the tile pass; 16 unit tests plus 7 golden-image integration tests (5 new committed goldens at 0.0000 mean error: route line, polygon fill, markers, traffic flow, antimeridian route at zoom 0).

### Added (M4 — platform packaging pipelines)

- `scripts/build-android.sh`: release cdylibs for arm64-v8a / armeabi-v7a / x86_64 (NDK 28.2, API-24 clang linkers), Kotlin bindings generation from the frozen UDL, and a `platform/android` Gradle build (AGP 9.0.0 — which embeds Kotlin support; the separate Kotlin Android plugin is rejected — compileSdk 36 / minSdk 24) producing a release AAR with `classes.jar` and per-ABI `jni/` libraries; a `smoke-jvm` module loads the Apple-host dylib through JNA and exercises the generated Kotlin classes (compiled version, normalized-coordinate error path, tile covering, camera roundtrip).
- `scripts/build-ios.sh`: `staticlib` builds for `aarch64-apple-ios` and `aarch64-apple-ios-sim`, one Swift generation for both slices, and `xcodebuild -create-xcframework` into `platform/ios/artifacts/map_native_coreFFI.xcframework` (device + simulator slices with headers/modulemaps); `Package.swift` at tools-version 5.10 (Swift 5 language mode) builds the generated sources against both simulator architectures (`xcodebuild` simulator build green).
- `scripts/build-rn.sh`: `ubrn generate jsi bindings` (TypeScript + JSI C++) and `ubrn generate jsi turbo-module` (`NativeMapsSdkRn` spec, `src/index.ts` entrypoint with `uniffiInitAsync`, podspec/CMake/Kotlin/ObjC glue) from the frozen UDL; `tsc --noEmit` strict green (TS 5.9.3, react 19.3.1 / react-native 0.88.1 types). Recorded generator facts: ubrn renders module identifiers verbatim from the namespace (pass `map_native_core`, not the hyphenated crate name) and resolves the crate via `cargo metadata` from the CWD. Not yet exercised: running the bindings inside a Metro app shell and `ubrn build ios/android`.

### Fixed (verification hardening on the packaging slice)

- `map-native-core`: removed the speculative `map_native_core_link_anchor()` helper — no generated binding references it (the scaffolding objects are pulled through the generated code's direct scaffolding-symbol references); the XCFramework re-assembles and the Swift simulator scheme links green without it.
- `platform/rn/.gitignore`: every ubrn generator output (`src/`, `cpp/`, TurboModule glue, podspec) is now ignored like the Android and iOS generated trees already were (Kotlin bindings + jniLibs, Swift sources + XCFramework), keeping only the package/lockfile, `tsconfig.json`, and `ubrn.config.yaml` inputs in git.

### Added (M6 — navigation UDL wiring)

- `map-native-core` (round 7, slice B): the frozen `map_native_core.udl` contract extended by addition only with the navigation state surface — the `[Trait, WithForeign]` `NavigationStateEventListener` (three sync callbacks: `on_navigation_instruction`, `on_deviation_status_change`, `on_navigation_phase_change`, no error channel), the `MapNavigationController` object (constructor; three `subscribe_to_*` methods returning the cancellable `NavigationTopicSubscription`; throwing `start_trip(RouteData, UnitSystem, StepAdvancePolicy, DeviationPolicy)` and `provide_reroute(RouteData, StepAdvancePolicy)`; fire-and-forget `provide_location_update`; read-only `TripNavigationState? current_trip_state()` and `NavigationInstruction? current_instructions()` composing `compose_navigation_instructions` in the trip's unit system), and `default_step_advance_policy`/`default_deviation_policy` namespace functions. All mirror records/enums live in `records.rs` and are re-exported from the crate root: `TripNavigationState`, `NavigationPhase`, `RouteStepPosition`, `TripProgress`, `DeviationInfo`, `NavigationInstruction`/`ActiveManeuverInstruction`/`UpcomingManeuverInstruction`/`DistancePhrase`/`ManeuverInstructionText` (+ kind enums), `DeviationStatusChange`, `NavigationPhaseChange`, `NavigationLocationUpdate`, and the policy records `StepAdvancePolicy`/`DeviationPolicy` standing in for the native `Arc<dyn StepAdvanceCondition>`/`Arc<dyn DeviationDetector>` (payload-carrying native enums mirror as kind enums + nullable fields; lane guidance stays off the instruction payload, consistent with the frozen `RouteStep`).
- New `SdkError::InvalidNavigationInput { detail }`: the controller boundary refuses non-finite or negative policy thresholds and route steps whose maneuver label the inbound `native_maneuver_type_from_text` parser cannot map onto the native maneuver classification (labels are outbound-stable snake case, e.g. `departure`, `roundabout-2`, one-based). The reverse `From<SdkError> for SDKError` conversion carries the detail under `InvalidCoordinateInput` (navigation-only variant, keeps the conversion total). `MapNavigationController` re-entrancy across the seam mirrors the broadcaster's rules: the active-trip lock is released before every dispatch, and a foreign listener's re-entrant controller call meets the same single-in-flight suppression (`SuppressedReentrant`), with uniffi sync callbacks running on the calling thread.
- `tests/navigation_callbacks.rs`: ten contract-surface tests recording the listener payloads through the real controller — start-trip dispatch, default policies matching the native constants, walker-driven advance to arrival (one `Arrived` phase), deviation excursion flag/clear across topics, anchored-reroute recovery (`OffRouteEntered → RerouteApplied → OffRouteCleared`), subscription cancellation (idempotent), refused thresholds/unknown maneuver labels leaving no trip, and re-entrant listener drive suppressed while the state stays applied. Nothing committed (git rules).

### Added (M8 — overlay exposure on the wasm canvas)

- `map-web-bindings` (round 7, slice A): `GpuMapCanvas` gains the native overlay layer — a second render pipeline mirroring the headless overlay WGSL byte-for-byte (verified by direct comparison of both shader sources), same `ClipSpaceOverlayVertex` layout, alpha blending, drawn after the tile pass in the same render pass; an empty list draws nothing. JS surface: `set_overlays(list)` (plain objects selected by `kind`: `routePolyline` / `filledPolygon` / `pointMarkers` / `trafficFlow`, with color/width/radius overrides exactly where the native structs have fields and defaults from the `map-rendering` constants) + `clear_overlays()`; every fallible path (unknown kind, missing/mistyped fields, non-finite or non-positive sizes, out-of-range coordinates or color channels) rejects with a string and keeps the previous list — nothing panics. New `CanvasTrafficFlowCongestionLevel` enum and an honest `overlayTriangles` statistics field (0 without overlays).
- `platform/web`: `overlay_scenes.mjs` (shared Zurich demo scenes, mirrored coordinate-for-coordinate from `crates/map-rendering/tests/overlay_golden.rs` so the playground and the compare page cannot drift); playground scene-overlay fieldset (route/polygon/markers/traffic checkboxes + clear, enabled only on the GPU path with explicit disable reasons); the golden-compare page extends to the four Zurich overlay fixtures (`serve.mjs` allow-list grows accordingly) with exact pinned triangle counts (route 4 / polygon 2 / markers 24 / traffic 8) and a cleared-frame re-probe against the tiles-only golden. The antimeridian world-copy overlay golden stays native-only (documented: its base frame draws synthetic in-memory gradient tiles the same-origin-fixture browser canvas cannot reproduce). Playwright: 4 new compare tests + the playground-toggle spec; 14/14 green, both suites also pin `overlayTriangles = 0` for the tile-only scenes (the browser mirror of the native empty-list degradation test).

### Added (M6 — recorded GPS-trace fixtures driving end-to-end navigation)

- `map-navigation` (round 7, slice C; test-only): three committed deterministic synthetic traces under `fixtures/navigation/traces/` — clean walk to arrival, 60 m deviation excursion freezing then recovering, deviation then reroute onto a replacement route anchored by construction at the reducer's own frozen snapped coordinate — each recording seed, thresholds, provenance (honestly synthetic, not live MapQuest captures), serialized route geometry, and 79–80 accuracy-carrying samples at the 5 s replay cadence; plus `README.md` documenting format, provenance, and regeneration (`cargo test -p map-navigation regenerate_trace_fixtures -- --ignored`). A validating loader (`load_trace_fixture`) fails hard with actionable messages (8 documented malformed-document mutations covered), and an always-on test proves the regenerated bytes equal the committed files byte-for-byte. `trace_replay_tests.rs` drives the full pipeline end to end with replayed timing: clean walk (steps in order, progress monotonicity, per-fix snap envelope, arrival), instruction composition along the replay (provider text verbatim under the distance prefix, band quantities within noise + rounding, upcoming maneuver, arrival payload), freeze→recover→re-anchor semantics, reroute rebuild (phases exactly `OffRouteEntered → RerouteApplied → OffRouteCleared → Arrived`, deviation stream exactly `[OffRoute(≈60 m), OnRoute]`), broadcaster delivery order + mid-replay topic cancellation, and a proptest proving raw fixes snap inside the accuracy envelope off the clean route line. 105 map-navigation tests green (94 pre-existing + 12 trace + 1 fixture-suite count); root `Cargo.toml` gains the workspace `map-navigation` path dependency and the crate gains dev-only `serde`/`serde_json`/`proptest`.

### Fixed (round 8 — native-device parity, found by exercising the surface on real simulators)

- `map-rendering`: the GPU-device request uses `wgpu::Limits::downlevel_defaults()` in both the headless and the surface graphics-context sources. The iOS simulators' Metal adapters report `max_inter_stage_shader_variables = 15` while `Limits::default()` requests 16, so every device request failed (`Limit 'max_inter_stage_shader_variables' value 16 is better than allowed 15`); the renderer's pipelines need only a handful of varying locations, and downlevel limits are what the golden-image tests already run under.
- `map-rendering` (`surface.rs`): the surface configuration now **negotiates** the composite alpha mode — `PREFERRED_ALPHA_MODE` (`Opaque`) is requested when the adapter reports it, otherwise the first supported mode — mirroring `preferred_surface_format`'s prefer-then-negotiate pattern. Android emulator ANGLE surfaces report only `[Inherit]`, so the previous hard-coded `Opaque` made every `Surface::configure` a validation error there.
- `map-native-core` (new `runtime.rs` executor seam): a crate-owned multi-thread tokio runtime (IO + time drivers enabled, lazily initialised) with a `drive()` handback — every exported `[Async]` method (the nine `MapSdk` provider calls and `MapSurfaceView::draw`) resolves its providers synchronously, then hands the reactor-dependent work to the crate runtime and awaits only a completion handback. Before this, the foreign executor (Swift main actor, Kotlin main dispatcher) polled the exported futures directly and the first `reqwest`/`tokio::time` poll panicked "there is no reactor running" on every real device; draw frames panicked mid-prepare and wedged the surface slot `Busy` at 60 Hz. A panicking worker task resurfaces verbatim on the poller (uniffi's panic boundary reports it honestly). Two tests pin the semantics: a runtime-dependent task driven from a deliberately reactor-less current-thread executor, and panic surfacing across a thread join. The `tokio` dependency moves from dev-only to `rt-multi-thread`/`sync`/`time`.

### Added (M5 wave 2 — Swift UI wrapper + SwiftUI reference application)

- `platform/ios/Sources/MapNativeCoreUI/` (new `MapNativeCoreUI` SwiftPM library product): the design language's UI layer for Swift — `docs/ReferenceAppUX.md` §2's locked token table under exact token names (`ReferenceAppUXColor` semantic per-theme colors incl. an in-slice fix: hex-row channels now divide by 255 like the rgba rows always did — un-divided, the `Color` constructor clamps every channel at 1.0 and all hex tokens render pure white, which only the light theme exposes), `ReferenceAppUXFont` type scale, `AppLayout` spacing/radius/elevation + §1 responsive constants (560-px phone breakpoint, capped panel fractions), an explicit `AppTheme` environment (dark default, never the system preference), the main-actor `NavigationStateCardStore` + `NavigationInstructionPresentation` (§7 card set from the real listener payloads), and token-styled control primitives (`ReferenceValueSlider`, `ReferenceSwitchControl`) replacing native iOS slider-toggle chrome that ignores SwiftUI's color scheme; the shared demo artifact ships as a SwiftPM `.copy` resource built from the single source `platform/reference-scenes.json` (explicit snake-case decoding keys — `convertFromSnakeCase` collides with custom snake literals).
- `platform/ios/reference-application/`: the SwiftUI reference application (xcodegen project, bundle `dev.mapssdk.reference.ReferenceApplication`) — the map fills the viewport via a `CAMetalLayer`-layered view driven by an app-owned `CADisplayLink` performing the attach/resize lifecycle at the physical pixel extent (drawable capped at the 2048 max-texture dimension; the view keeps point size and scales the drawable) and issuing exactly one serialized `MapSurfaceView.draw(camera:)` per frame; search via `forwardGeocode`, pedestrian route planning via `calculateRoute` with route-polyline + destination-marker overlays and `MapCamera.fitBounds` framing (honest note: the contract exposes no exact zoom readback); simulated TBT replaying the committed `zurich_clean_walk.json` through the real `MapNavigationController` — three topic subscriptions, per-tick `provideLocationUpdate`, off-route displacement simulation, kept-cards arrival so the green "Arrived" banner persists after the journey concludes; readiness ladder + §6.3 provider-echoing error reports on every `SdkError` path; dark/light themes; drag-pan through `screenToGroundCoordinate` + pinch-zoom forwarding; API key injected at build time from the gitignored `Local.xcconfig` (`$(MAPQUEST_API_KEY)` in the generated Info.plist; tracked example + `scripts/prepare-local-configuration.sh` that never echoes the key). Launch-argument demo automation (`-DemoPlanCannedRoute`, `-DemoStartSimulatedJourney`, `-DemoRunOffRoute`, `-DemoSelectLightTheme`) drives exactly the same user-path methods, purely because `simctl` has no touch injection. Verified live on the iPhone 17 Pro simulator: rendered tiles, geocode→route→fit-bounds over rendered tiles, the full simulated journey to the kept-cards arrival, deviation alert with frozen guidance, both themes with pixel-verified token values (screenshots `tmp/refapp-ios/`, series kept honest including the pre-fix failure captures).

- `platform/android/maps-sdk` (round 8, wave 2, slice K): thin idiomatic Kotlin wrapper over the uniffi-generated `map-native-core` bindings, consumed unmodified (no generated-code edits) — `MapsSdkFacade`, a collecting `NavigationStateEventListener` funneling the three navigation topics into Compose `StateFlow`s on the main thread, the single `ReferenceAppUXTheme`/`ReferenceAppUXDimensions` composed from the design language's §8 tokens (dark default + light toggle; themed secondary colors before any Material default), and a snake-case `ReferenceScenesLoader` for the bundled demo artifact.
- `platform/android/reference-application` (slice K): the Jetpack Compose reference application — the map hosted in a `SurfaceView` whose pointer crosses into the SDK through a minimal documented JNI shim (`ANativeWindow_fromSurface`/release; deliberately not a UniFFI replacement — a pointer cannot cross UniFFI's value system — and zero-pointer lowers to null so the contract's exact-one-platform-pointer validation reports verbatim), an explicit frame pump issuing exactly one serialized `MapSurfaceView.draw` per composed frame via `withFrameNanos` with documented concurrent-draw/resize backpressure (`RenderingSurfaceUnavailable` → next-frame retry), geocode search + two-waypoint route planning with inset-aware `MapCamera.fitBounds`, the four artifact overlay scenes as toggles, camera preset chips + token-styled sliders, phone-width stack + wide edge docking, the readiness ladder with §6.3 verbatim error reporting (uniffi `InternalException` widened; `CancellationException` re-throw so composition teardown is never reported as failure), and simulated turn-by-turn replaying the committed `zurich_clean_walk` trace through the real `MapNavigationController` at the named 20× demo acceleration (instruction banner countdown, progress card, `Navigating → Arrived`; start and mid-trip delivery failures stop the replay visibly). Gradle build-time `Copy` tasks bundle `platform/reference-scenes.json` and the trace from their single committed sources so the bundled demo data cannot drift; the MapQuest API key resolves at build time from gitignored `platform/android/local.properties` or the `-PmapquestApiKey` Gradle property, with the `MAPQUEST_API_KEY` environment variable as the CI fallback (host-local sources win; an absent key presents the honest `failed` readiness state with configuration guidance). Verified live on the Pixel_9 emulator (SwiftShader): rendered tiles, all four overlays + the planned-route line drawn, geocode → route-planned flow over the network, and the full journey start → live banner countdown → green Arrived with logcat confirming zero reported failures (screenshots `tmp/refapp-android/`, earlier failure series kept honest).

### Added (round 8 — native surface renderer contract, render-state signal)

- `map-rendering/src/surface.rs` (slice 0, wasm32-gated): the surface renderer presenting the headless pipeline's exact scene (textured tile quads + overlay layer, byte-for-byte mirrored shaders and vertex encoding) into a live `wgpu::Surface` — platform-agnostic `SurfaceRenderTarget::from_unsafe_target` lowering (raw handles provided per platform outside the crate), surface-format and alpha-mode negotiation, configure/resize with the stale-surface retry, and the `SurfaceRenderError` taxonomy with the busy-slot draw contract.
- `map-native-core` (slice 0): the frozen `map_native_core.udl` extended addition-only with the native surface wiring — the `MapSurfaceView` object (`attach`/`resize`/`detach`/`set_overlays`/`clear_overlays` and the per-frame async `draw(camera:)`; the platform's own render cadence drives the view, the SDK keeps no render thread), `platform_surface.rs` lowering the iOS/Android platform pointers (zero or ambiguous pointers refused, `SdkError` verbatim), the `MapOverlayKind` + `MapOverlayDescriptor` mirror records (kind tag + nullable payload fields, the wasm canvas' overlay plain-object names), `register_mapquest_providers(mapquest_api_key)` installing all seven native MapQuest provider implementations from one key (re-registration replaces slots; elevation stays honest `ProviderNotConfigured`), and the new `SdkError::InvalidRenderInput` / `RenderingSurfaceUnavailable` variants. `platform/web/types/map_native_core.d.ts` mirrors the additions; the contract-sync suite stays green.
- `map-rendering` tile scheduler (slice C): the GL-JS-`idle`-equivalent render-state queries — `TileScheduler::is_idle` and `prepared_frame_is_idle`, documented over the fetch-state pending taxonomy (`Deferred` / `Unavailable` / `FallsBackTo` are exactly the states whose pixels a later frame may still change; idle when every requested tile is `Present`; a *query*, not an event — it can flip back under pool eviction) — plus `tests/idle_snapshot.rs` as the deterministic golden-test seam. `map-native-core/tests/swift_generation.rs` locks the Swift binding generation beside the existing Kotlin markers.

### Added (round 8 — reference-UX language + shared demo artifact)

- `docs/ReferenceAppUX.md` (slice D): the single design-language source of truth for all reference applications — the map-fills-the-viewport layout archetype with the phone-width rule, the locked semantic color/type/spacing token tables (dark + light), the explicit-theme model, the component inventory and disabled-with-reason / readiness-ladder / verbatim-error patterns, the SDK-vs-platform ownership boundary for gestures, the payload-named turn-by-turn card set, the per-platform token-consumption rules, and the Playwright-locked UX-host contract table.
- `platform/reference-scenes.json` + the web refapp wiring (slice D): the language-neutral demo artifact every refapp loads (camera presets, overlay scenes mirrored coordinate-for-coordinate from the native goldens, the navigation demo section); `overlay_scenes.mjs` + `serve.mjs` consume it by name, and `tests/sdk_core.spec.mjs` grew the UX-behavior lock (theme flip, readiness ladder, scene-overlay enable/disable-with-reason after readiness, preset-picker camera application, navigation preview notice, phone-width scroll axis) — the Playwright suite stands at 20/20.
