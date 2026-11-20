# The browser target

The browser target draws the map on WebGPU from a WebAssembly module generated from the
same frozen UniFFI contract the native targets use. The page is a plain ES-module
harness with no bundler. `platform/web/` holds the playground, fixture server,
Playwright suite and performance harnesses; `crates/rustic-maps-native-core/` owns the
generated module; and `crates/rustic-maps-web-bindings/` supplies the canvas and
provider slots the contract cannot express.

## What lives where

- `crates/rustic-maps-native-core/`: the generated `wasm2` module and the re-exported
  `browser_canvas` crate; `src/rustic_maps_wasm.udl` is the committed WebAssembly contract.
- `crates/rustic-maps-web-bindings/`: the hand-written canvas; `src/canvas.rs` holds
  `BrowserMapView`, `src/exports.rs` `MapCamera`, and `src/live_providers.rs` the slots, fallback
and browser seams.
- `platform/web/generated/`: the generated ESM modules, declarations, `rustic_maps.wasm` and
wasm-bindgen glue, gitignored; `types/browser_canvas.d.ts` declares the untyped glue.
- `platform/web/`: `serve.mjs` (static server on `127.0.0.1:8642`), the playground (`index.html`,
  `main.mjs`, `playground.mjs`), the golden page, the Playwright suite and gates (`tests/`), the
performance harnesses (`perf/`) and the npm manifest (`npm/`).

## The generated surface

### Generation from the contract

`scripts/generate-wasm-bindings.sh` builds `rustic-maps-native-core` for
`wasm32-unknown-unknown`, then runs
`ubrn build wasm2 --config ubrn.config.yaml --release` from `platform/rn`: it compiles,
reads the `UNIFFI_META_*` records, writes the TypeScript, runs `wasm-bindgen`, and
stages the module. The scaffolding and bindgen consume the committed
`crates/rustic-maps-native-core/src/rustic_maps_wasm.udl`, the frozen contract minus the
native `MapView` interface and its doc block, so the 349 signature-table entries and 122
checksums align with the module, zero missing.

`wasm2` generates dictionaries, unit enums, the tagged `SdkError` union (detail in
`.inner.detail`), handle-backed interfaces, `Promise<T>` async members with
`{ signal }`, foreign-trait `XImpl` classes, `Array<T>` /`ArrayBuffer`, `bigint` for
`u64` in 71 fields or arguments, and static `fitBounds` from `Name=fit_bounds`.

The entrypoint exports `uniffiInitAsync(source)`: it opens the module once, registers
the signature table, calls the namespace `initialize()` checksums, and is idempotent.
When the module imports `__wbindgen_placeholder__`, staging runs `wasm-bindgen`, keeps
`<stem>_bg.js`, and hands it to the player as `resolveModule`, which calls
`__wbg_set_wasm` and `__wbindgen_start`.

### One module, two ABIs

One module serves the generated contract and the canvas. It carries 5,351 exports: 306
UniFFI exports and metadata globals, plus wasm-bindgen classes (`mapview_new`,
`mapview_renderFrame`, `mapssdk_create`, and the rest), and imports
`./rustic_maps_bg.js` 640 times.

Objects do not cross the ABIs. A generated `MapSdk` is a JavaScript proxy over an
integer UniFFI handle; the canvas glue's `MapView` constructor passes `maps.__wbg_ptr`
into `wasm.mapview_new`, and wasm-bindgen cannot accept a UniFFI handle, so only
primitives and plain values cross. The registry stays shared by mirroring
`MapSdk::register_*` into the canvas slots on wasm (`provider.rs`'s `set_shared_*`). The
contract's `MapView` never reaches the page: the canvas class is `BrowserMapView`, and
the signature table is asserted to name no `MapView`.

### Package layout and freshness

`scripts/build-wasm2-package.sh` compiles the TypeScript and clears the output directory
first, so a removed module cannot leave a stale file:

```
index.js / index.d.ts · rustic_maps.js / rustic_maps.d.ts
rustic_maps-ffi.js / rustic_maps-ffi.d.ts · rustic_maps_bg.js / rustic_maps_bg.d.ts
rustic_maps.wasm · SOURCE_FINGERPRINT
```

`wasm-opt -Oz` runs when binaryen is installed; pass `--no-optimize` to skip it,
`--no-generate` to repackage existing bindings, or `--require-optimize` to fail when it
is absent. The npm package is `@angoratek/rustic-maps-sdk-web`, versioned with the
workspace, depending on `@ubjs/core` and `@ubjs/wasm` at `0.31.0-6`.

`scripts/check-wasm2-package-freshness.mjs` hashes the UDLs, the build, generation and
packaging scripts and the relative-specifier rewrite, compares the fingerprint with
`SOURCE_FINGERPRINT`, asserts the emitted module set, and enforces the size budget.
`wasm-bindgen` must match the crate (`0.2.129`); after a bump reinstall the CLI and
regenerate `platform/web/generated`, checking with `wasm-bindgen --version` and
`cargo tree -p rustic-maps-native-core | grep wasm-bindgen`.

### Browser constraints

- Async init is mandatory: nothing touches the surface before `uniffiInitAsync` resolves. wasm is
single-threaded and per-instance; workers are the only isolation, and long Rust calls
block the main thread.
- Errors carry detail in `SdkError.inner.detail`; `String(error)` yields the variant, and the page
façade adapts both to `{ code, message }`. `u64` is `bigint` in 71 places.
- `setProviderBaseUrl` and `clearProviderRegistrations` have no contract member and remain
wasm-only seams.

## Building, serving and testing

### Build and serve

```bash
npm ci                        # platform/web: tsc and the @ubjs runtime
npm ci --prefix platform/rn   # platform/rn: ubrn
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129   # must match the crate
scripts/build-wasm2-package.sh
cargo clippy -p rustic-maps-web-bindings --target wasm32-unknown-unknown
```

```bash
node platform/web/serve.mjs    # http://127.0.0.1:8642/
```

| Route | Serves |
| --- | --- |
| `/` and static paths | `platform/web/` |
| `/tiles/{zoom}_{x}_{y}.png`, `…@2x.png` | raster tile fixtures in `crates/rustic-maps-rendering/fixtures/render/tiles/` |
| `/providers/{provider_key}/…`, `/providers.json` | the fixture routes and the provider registry |
| `/golden/{file}.png` | the allow-listed golden fixtures |
| `/golden-vector/{z}/{x}/{y}.pbf` | the committed vector tiles the golden scenes use |
| `/generated-runtime/@ubjs/…` | the installed `@ubjs` runtime under `platform/web/node_modules` |

The browser never fetches externally, and an unknown tile answers 404. The playground
camera defaults to the Sacramento anchor (38.5766, -121.4944) at zoom 14. The host is
`127.0.0.1` and the port `8642` unless `PORT` overrides it. A manual server and a test
run share one port; plain Chrome over `localhost` resolves WebGPU with no flags.

### Playwright

```bash
cd platform/web
npx playwright install chromium   # first time only
npx playwright test               # chromium channel, headless
```

The configuration uses the system Chromium channel, a 640×480 viewport,
`deviceScaleFactor: 1`, and one worker, because every page owns a WebGPU device. CI gets
one retry; local runs stay strict. `globalSetup: ./global_setup.mjs` builds a current
package and runs the file-parsing gates (contract sync, the provider-name guard, the
platform-parity floor, style-layer coverage, reference flows, the iOS pod/SwiftPM
mirror, the package freshness gate and npm packaging assertions), then asserts a WebGPU
adapter in CI. `webServer` runs `node serve.mjs` with `reuseExistingServer: false`.
Tests gate on `window.mapTestReady` and `window.mapRendererMode()`; the smoke test skips
with a documented reason when the mode degrades to `overlay`.

### Contract sync and import maps

```bash
node --test platform/web/tests/contract_sync.mjs
```

The test needs no server or browser, but the package must be built first. It asserts
that `crates/rustic-maps-native-core/src/rustic_maps_wasm.udl` is exactly
`crates/rustic-maps-native-core/src/rustic_maps.udl` minus the native `MapView`
interface and its doc block, keeping `MapSdk`, `MapCamera` and `Navigator`; and that
`platform/web/generated` carries `uniffiInitAsync`, the contract classes, the glue's
distinct `BrowserMapView`, a signature table that never names `MapView`, and the
workspace version, with the npm manifest identity (`@angoratek/rustic-maps-sdk-web`)
matching the workspace.

No bundler resolves the bare specifiers: `index.html`, `golden.html` and
`generated_wasm_smoke.html` carry an import map for `@ubjs/core`, `@ubjs/wasm` and
`@ubjs/wasm/core` at `/generated-runtime/@ubjs/…`; `serve.mjs` maps that prefix onto the
installed packages.

## What the browser does differently

### WebGPU facts

- **A secure context is required, and localhost counts.** WebGPU exists only in secure contexts,
and `http://127.0.0.1:8642` qualifies.
- **The Chromium channel works headless with no flags.** The adapter resolves with no launch flags
at all. The suite still passes
  `--enable-unsafe-webgpu --enable-gpu --enable-features=Vulkan --use-vulkan=swiftshader
  --use-angle=swiftshader --no-sandbox` through `platform/web/browser_launch_options.mjs` for
GPU-process stability; Dawn on Linux runners wants Vulkan, or the loop stalls.
- **`drawImage` of a WebGPU canvas reads only a freshly presented frame.** Pixel probes send a
zero-delta wheel event to re-schedule a GPU frame, then sample `getImageData` across the
following frames.
- **Playwright's WebKit also resolves WebGPU today.** WebKit 27.2 (`webkit-2370`, headless on
macOS) exposes `navigator.gpu`, initializes in `gpu` mode, and reads requested 4 / drawn
4 / missing 0 at zoom 14. A WebKit run therefore does not exercise the 2D fallback; it
fires only when `MapView` fails to acquire an adapter or device.

### The 2D overlay fallback

`BrowserMapView::initialize` fails cleanly when no adapter supports the canvas or the
device request fails: it returns a string error and never panics. The page catches that,
abandons the GPU canvas, and reports `mode overlay (WebGPU unavailable: …)` in the
status line and readout.

In overlay mode `#map-overlay` is the scene: a background grid plus the projected tile
grid from `visible_tile_keys`. In GPU mode it carries only the opt-in debug tile grid,
so the WebGPU-locked canvas never needs a 2D context (claiming one would poison the
surface). `wgpu` has both the `webgpu` and `webgl` features, but a WebGL2 path is
neither implemented nor tested; the documented fallback is the 2D overlay.

### The keyless raster fallback

A live `{z}/{x}/{y}` source registered by a page is a **fallback**: it serves only while
nothing else serves the raster-tile contract, and a configured provider always outranks
it, whatever order the two registrations happen in.

The fallback lives beside the per-capability registry, not inside its `RasterTiles`
slot. `registerUrlTemplateRasterTileSource` validates and stores the fallback only,
never replacing a configured raster provider; registering again replaces the previous
fallback. The canvas resolves the raster provider in this order: the shared slot the
contract registry mirrors, the configured `RasterTiles` provider, the page fallback,
then the recorded-fixture provider for the demo area when the page serves it.
`clearProviderRegistrations` clears configured providers but leaves the fallback, so a
cleared page keeps drawing live tiles; `clearSharedProviderSlots` does the same for the
mirrors; `clearUrlTemplateRasterTileSource` removes the fallback. The wasm façade
(`platform/web/main.mjs`) carries the clear call; `contract_sync.mjs` and the platform
parity gate see it as an addition. The frozen UDL is unchanged.

The recorded-fixture probe is a canvas construction capability.
`new BrowserMapView(canvas, tileResolution, recordedTileFixturesAreServed)` defaults
`recordedTileFixturesAreServed` to `true`, which is the local dev server and test
harness behaviour. A page that stages no fixtures passes `false`: the raster fetch then
asks the registered provider directly and records no fixture attempt, so the deployed
page issues no `/tiles` 404 for an explicitly selected raster base. The playground
derives the value from its host, and the harness drives the deployed behaviour from a
local origin with `?noRecordedTileFixtures`.

The registry's OpenStreetMap Standard raster base is live-only and commits no tile, so
its descriptor records the served contract as fixtureless. The fallback case answers the
intercepted live host with a generated placeholder image rather than another provider's
recorded imagery, keeping the test network-free without misstating tile provenance.

The browser transport drops the Fetch-forbidden headers it declares: `user-agent`,
`cookie`, `host`, `origin`, `referer` and `connection`. The native transport still sends
the declared user agent, which OpenStreetMap's tile policy expects from a server. A 404
or 410 is definitive, so the canvas stops asking; a transport or server failure retries
after 2 seconds (`TILE_FETCH_RETRY_BASE_DELAY_MILLISECONDS`), doubling to a 30-second
cap (`TILE_FETCH_RETRY_MAXIMUM_DELAY_MILLISECONDS`).
`MAXIMUM_TRACKED_TILE_RETRY_SCHEDULES` is 1,024, so a long pan neither grows the
memories without limit nor blacklists a tile. Every request carries a 30-second deadline
from `DEFAULT_REQUEST_TIMEOUT`.

### Parity with the native renderer

`crates/rustic-maps-web-bindings/src/canvas.rs` mirrors the native headless pipeline in
`crates/rustic-maps-rendering`. The native crate is `cfg` -gated out on wasm, so the
copy follows one rule: **same WGSL text, same vertex layout, same index pattern**.

- The WGSL (`TILE_QUAD_SHADER_SOURCE`) is copied verbatim. Vertices are `ClipSpaceTileVertex`
chunks (`Float32x4` clip position at offset 0, `Float32x2` texture coordinates at the
  `offset_of!` offset).
- Indices are `u16` in the pattern `0, 1, 2, 2, 1, 3` (`INDICES_PER_QUAD` = 6, `CORNERS_PER_QUAD`
= 4), bounded under `MAXIMUM_QUADS_PER_FRAME`.
- Tile textures are `Rgba8Unorm` with a linear, clamp-to-edge sampler. Both preferred surface
formats (`Bgra8Unorm`, `Rgba8Unorm`) are non-sRGB, so texel values reach the screen
unchanged, making a browser-versus-golden pixel compare meaningful. An undecodable
pooled payload skips its quads and is evicted for refetch; only structural failures
surface as errors.

The page serializes frames: the binding borrows draw state across the `renderFrame`
future, so the playground keeps a busy/queued loop.

### Canvas versus golden

`platform/web/tests/golden_compare.spec.mjs` renders the committed golden scenes in
`crates/rustic-maps-rendering/fixtures/render/golden/` with the playground camera:
center 38.5766/-121.4944, zoom 14, bearing 0, pitch 0 (pitched variant 45°), field of
view 60°, equal to `DEFAULT_FIELD_OF_VIEW_DEGREES`. Only the **512×512 viewport** of
`golden_image.rs` differs from the harness's 640×480 window.

- **Probe.** Send a zero-delta wheel nudge to re-present, `drawImage` the GPU canvas into a
same-size 2D probe, and sample `getImageData` across a dozen frames, keeping the
minimum-error sample; a stale read is a clear-color frame.
- **Geometry.** Use 512×512 with `deviceScaleFactor: 1`, so pixels are 1:1 with backing pixels. The
clear color `[0, 0, 0, 1]` and `CompositeAlphaMode::Opaque` make the 2D round trip
opacity-neutral.
- **Tolerance.** Mirror `golden_image.rs`: mean absolute per-channel error ≤ 2.0, and at most a
0.02 fraction of pixels with a maximum channel error exceeding 24. The golden must
decode to 512×512; settled statistics must match the scene's tile counts and
`tileZoomLevel === 14`. Both cameras list the trailing tile `14/2663/6286`, which the
page resolves through its zoom-13 parent via `BrowserMapView::stageParentTile`. The
pitched camera also lists `14/2661/6286`, wholly outside the viewport at pitch 45; the
quad assembler culls it, but it has pooled imagery, so
  `drawnTiles` is 8 and `missingTiles` is 0 (top-down draws 4, missing 0). `missingTiles` reports
tiles the scheduler could not resolve, never tiles the frame culled.
- **Overlay scenes.** The page also renders four Sacramento overlay scenes — route line, polygon
fill, markers (24 triangles) and traffic flow — against their goldens.
- **Hazards.** Probe only after a forced re-present, and control the window or use the fixture page.
  `--use-angle=swiftshader` renders software ANGLE, whose sampler behavior is the difference the
tolerance absorbs; if its mean error exceeds the budget, drop the argument rather than
widening the tolerance. The fetch zoom clamps to 14, so higher cameras are not
apples-to-apples.

## Tile throughput and zoom

### A bounded parallel pool

`TILE_FETCH_CONCURRENCY` (default 8) bounds every tile fetch pass:

- `TileWarmTask::run` keeps at most that many provider futures in flight through a
  `FuturesUnordered`; a key leaves the warming set when its outcome is delivered, and a cancelled
run unmarks everything it did not complete.
- `TileScheduler::prepare_frame` collects budgeted fetches through
  `buffer_unordered(TILE_FETCH_CONCURRENCY)` and inserts results into the texture pool serially.
- The browser canvas starts network fetches detached, up to `MAXIMUM_CONCURRENT_TILE_FETCHES` (32);
the warm pass it awaits only moves staged payloads into the pool.
- The per-tile fixture probe stays: it is the network-free test seam, runs once per
address when the page's `recordedTileFixturesAreServed` capability is enabled, and
removing it would make every fixture-backed case depend on a live provider. With the
capability disabled the probe is skipped and the provider answers directly.

### Provider pacing and the zoom cross-fade

A URL-template provider creates its token bucket from the source's `TileSourcePacing`
declaration: 8 immediate requests and 8 per second, matching the pool. A
`ZoomTransition` owned by the surface session (native) and the draw state (browser)
records the tile-z change and drives the frame:

1. **Hold.** After a tile-z change, the frame resolves the previous level's visible
keys from the pool only, never fetching, and draws them over the new level; projection
with the current camera scales them automatically.
2. **Fade.** When at least 85% of the new level's keys are present
(`ZOOM_TRANSITION_READY_TILE_FRACTION`), or the hold deadline expires, the previous
level's per-quad opacity ramps from 1 to 0 over 250 ms.
3. **Settled.** A running transition keeps the frame unsettled, so both pumps keep
drawing; frame statistics expose the transition state and the present/pending counts.

The tile vertex format gains an opacity attribute, the three pipeline copies switch to
alpha blending, and the retained level counts against the frame's quad budget. The
constants are `TILE_FETCH_CONCURRENCY` (8); `DEFAULT_TOKEN_BUCKET_CAPACITY_TOKENS` (8)
and `DEFAULT_REFILL_TOKENS_PER_SECOND` (8); `ZOOM_TRANSITION_FADE_MILLISECONDS` (250);
and `ZOOM_TRANSITION_MAXIMUM_HOLD_MILLISECONDS` (1,000), the longest the previous level
waits for a late tile.

A recording provider asserts the pool's observed maximum concurrency is exactly the
constant and the serial floor is gone; `frame_latency` asserts the pooled ceiling. A
browser case crosses a tile level by wheel and by double-tap and asserts: the previous
level draws during the hold, the new level draws when present, the fade ends, and
zoom-out never clears to the background.

## The developer console

Every reference app and the browser demo carry a developer console over the SDK's
structured log stream, so key events are visible without a debugger.

| Surface | Shape |
| --- | --- |
| `MapSdk.subscribe_to_diagnostic_events(listener, minimum_severity)` | Fire-and-forget delivery to a `SdkDiagnosticEventListener` |
| `MapSdk.flush_recent_diagnostic_events(listener, minimum_severity, maximum_count)` | Reads back the recent buffer |
| `MapSdk.set_response_excerpt_capture(enabled)` | Opt-in response-excerpt capture |
| `Diagnostics` façade | `add_listener`, `flush_recent` over the same objects |
| `LogEvent` | timestamp, `severity`, `subsystem_target`, `message_line`, `provider_request_trace` |
| `SdkLogSeverity` | `Trace`, `Debug`, `Info`, `Warn`, `Error` |

Events are already scrubbed: coordinates are rounded and API keys never appear. The
console keeps a level filter (Error, Warning and Info; Info shows all three), renders
one event per line newest first, and opens a detail view with the full message and the
provider trace fields below.

The observation seam is `crates/rustic-maps-core/src/provider_observation.rs`: a
redacted `ProviderRequestObservation` per attempt (query values removed, status class,
duration, byte count, a bounded character-boundary snippet), with `ObservedTransport`
observing retries per attempt. MapQuest and the five vendor clients report through it;
OSRM, Valhalla, Photon and the URL-template sources do not.

### Provider request trace and contract

`ProviderRequestTrace` carries `provider_key` (the registry key, or `application` for a
foreign implementation), `provider_contract` (`RoutingProvider`, `GeocodingProvider`,
…), `http_method`, `request_path` (the path only; query values dropped except documented
non-secret parameters), `status_class` (`2xx`, `3xx`, `4xx`, `5xx` or the
transport-failure marker), `outcome` (`success`, `client-error`, …),
`duration_milliseconds`, `response_bytes`, and `response_excerpt` (the first 512 bytes
when capture is on, truncated at a character boundary; null otherwise).

Never included: the credential, its label's value, authorization headers, and query
parameters the transport classifies as secret; a transport test feeds a credential
through a request and asserts neither value nor header appears. Cache events join the
stream as `Info`: tile-cache and routing-cache hits and misses, each with cache kind and
entry key, aggregated per session.

The addition is append-only:

```
dictionary ProviderRequestTrace {
    string provider_key;
    string provider_contract;
    string http_method;
    string request_path;
    string status_class;
    string outcome;
    u64 duration_milliseconds;
    u64 response_bytes;
    string? response_excerpt = null;
};
ProviderRequestTrace? provider_request_trace; // on LogEvent; null otherwise
```

`set_response_excerpt_capture(boolean enabled)` turns excerpt capture on for the
session. Traces are `Info`; failures stay `Warn` /`Error`.

`HttpRequest` gains `provider_contract` and `provider_key`, filled at each provider's
one request-building site per API. One process-wide sink installed by the native binding
and the wasm façade observes it; with no sink the transport skips the observation with
no allocation. Query values are dropped except the allow-list `format`, `overview`,
`geometries` and `units`, the path keeps its structure, and the excerpt excludes
secret-classified content. The excerpt reuses the token vocabulary `key`, `apiKey`,
`access_token`, `session` and `signature`.

### The console surface, retention and tests

A floating button sits above the attribution notice, opposite the map layer control, and
opens a full-height sheet on a phone or a side panel on a wide viewport, carrying a dot
when a Warning or Error arrived since it was last opened. The console is read-only over
the SDK stream, keeps no state beyond the filter, and never issues a provider call. The
browser demo implements it as the provider console, with identifiers
`provider-console-control`, `provider-console-panel`, `provider-console-rows`,
`provider-console-detail` and `provider-console-alert-dot`. It filters by level,
provider and outcome, shows cache counters, and offers Copy and Export. Android, iOS and
React Native carry the same console.

The web panel also carries the provider request console: one row per request, naming
provider, capability, method, redacted path, status, duration and a bounded excerpt.
Rows filter by provider, capability and outcome, copy as text and export as JSON. It
reads the same stream as a second subscriber and adds nothing to the request path; the
provider list comes from `platform/providers.json`, never a hardcoded list. The excerpt
uses `RESPONSE_SNIPPET_MAXIMUM_BYTES` (512), truncated on a character boundary and
credential-free; a switch states it may contain provider content and is never uploaded.

Retention: the SDK keeps 512 events per subsystem (`DIAGNOSTIC_RING_CAPACITY`), and the
console reads through `flush_recent` without a second copy. The browser bounds visible
rows to `PROVIDER_CONSOLE_MAXIMUM_ROWS` (200) and reads up to
`PROVIDER_CONSOLE_FLUSH_LIMIT` (1,000). Native exports stream the filtered list to a
temporary file capped at 256 KB and hand it to the share sheet; the web control copies
rows as text or downloads JSON.

Tests: a Rust request through a fake server emits one trace with path, status class,
duration and a 512-byte excerpt; a credential-bearing request emits neither value nor
header; a non-provider event carries no trace; each ring caps at 512 events. Browser
coverage is `developer_console.spec.mjs`, `provider_console_panel.spec.mjs`,
`provider_console_provider_matrix.spec.mjs`,
`provider_console_response_excerpt.spec.mjs` and
`provider_request_diagnostics.spec.mjs`. Native targets open the console, render Info
events from an injected listener, and export a file matching the filtered list.

## Performance

Three workloads run outside `npm test`: a steady pan, a cold fit and a simulated trip.
The suite renders under SwiftShader, whose absolute frame times say nothing about a GPU
(MapLibre's benchmark under-reported roughly 390× there).

### The frame-budget harness

`platform/web/perf/run-frame-budget.mjs` drives one frozen scenario and reports
percentiles of `renderFrame`; the animation-frame interval; `stageMilliseconds` for the
tile pipeline, overlay geometry, encode and submit; and `gpuPassMilliseconds` from
timestamp queries, read a frame later. The readback ring never cancels or remaps a
pending buffer, since Firefox rejects that ("still mapped", "already mapped", "map is
pending") and can stop the loop presenting; `frame_loop_cleanliness.spec.mjs` asserts no
such error, and `gpuBufferAllocations` must stop growing on a settled golden scene.

The scenario is a vector base over a 1920 × 1080 canvas at device scale 1, or the raster
base with `--layer raster`; after a settle and warm-up, a 600 device px/s pan runs for 4
s, out and back. The 60 Hz budget is p95 ≤ 16.7 ms, p99 ≤ 33 ms, no frame > 50 ms.

```bash
cd platform/web
npm run perf                                        # the vector base
node perf/run-frame-budget.mjs                      # with flags
node perf/run-frame-budget.mjs --layer raster       # the tile path
node perf/run-frame-budget.mjs --json report.json   # keep the report
node perf/run-frame-budget.mjs --headed             # watch the pan
```

Measured on an Apple Silicon host (`apple · metal-3`), headless Chromium with hardware
WebGPU, 1920 × 1080, vector base, 120 frames: `renderFrame` p50 1.4 ms / p95 1.5 / p99
8.2 / max 9.8; animation frame p50 16.7 / p95 16.8; `overlayGeometry` p50 0.0 / p95 0.1
/ p99 7.2 / max 8.5; `encode` 0.0; `submit` p95 0.1 / max 0.2; `gpuPassMilliseconds` p50
0.8 / p95 0.82 / p99 0.89; 0 of 120 frames over 16.7 ms. The raster base draws 15 tiles
at `renderFrame` p50 0.2 ms and `gpuPassMilliseconds` p50 0.15 / p95 0.21 / p99 0.21.

### The cold fit: z14 to z7

`perf/run-fit-budget.mjs` measures on a bare surface with no page pump over committed
z7/z8 fixtures: one frame at z14, then z7 until every visible tile has drawn. It fails
when the first full frame exceeds 1.5 s, the frame draws more than 400,000 overlay
triangles at z7, the z8 unchanged-settle p95 exceeds 6 ms, or those frames extract a
label candidate. `tests/vector_fit_budget.spec.mjs` gates the hardware-independent part
and enforces time only with `MAP_FIT_BUDGET_STRICT=1`.

```bash
cd platform/web
node perf/run-fit-budget.mjs                  # readable report
node perf/run-fit-budget.mjs --json fit.json  # keep the report
node perf/run-fit-budget.mjs --headed         # watch the fit
```

Measured on the same host: first base tile 80 ms, first full frame (15 tiles) 1,059 ms,
371,745 overlay triangles, `overlayGeometry` p95 41 / max 51 ms, settled `renderFrame`
p50 2.1 / p95 5.0 ms. The z8 settle measures p50 2.1 / p95 5.8 ms (budget 6 ms), 371,965
triangles and 0 label candidate extractions / 15 cache hits; one sample was a 54.8 ms
tail.

Four choices keep a fit cheap: visible-first delivery
(`with_visible_prefetch_burst_tile_count`, `prefetch_missing_tiles_with_delivery`, with
no ring ahead below z8); per-tile styled geometry cached by tile address, style revision
and style-zoom bucket, leaving `VectorBaseFrameDiagnostics::tessellated_tile_count` zero
on a warm refit; low-zoom level of detail that simplifies rings and lines to half a
pixel at or below z8 while city zoom stays exact (at z7 in Carto light one tile drops
from 34,788 to 8,643 triangles, so the fifteen-tile frame draws 129,645 instead of
521,820); and shared frames, where an unchanged frame is the previous
`Arc<VectorBaseFrame>` and `content_revision` names its tiles, style revision, zoom
bucket and sprite atlas (over a 46,000-overlay z7 frame: 24 ns shared-frame clone, 1.02
ms owned-list clone, 0.29 ms deep compare, 4 ns revision compare).

`tests/vector_zoom_cross_fade.spec.mjs` asserts a zoom-out keeps the retired level drawn
without clearing to the background, and a cached level draws when the camera returns.
Frame statistics report `vectorZoomTransitionRunning`, `vectorRetainedTiles`,
`vectorBaseLabelCandidateExtractionCount` and `vectorBaseLabelCandidateCacheHitCount`.

### Motion grading and guided trips

During a pan, zoom or rotate the vector base paints decoded tiles and the last settled
style-zoom bucket's geometry, skipping label assembly; the first still frame resolves
the bucket and labels at the final pose. The browser reports gestures through
`setCameraGestureActive`. A graded pan measured `renderFrame` p50 1.4–1.8 ms against
8.0–8.1 ms ungraded; p99 was 8.1–10.4 against 12.6–13.0.

- **Cadence.** The demo dispatches fixes every 250 ms at 1× (the artifact's 5 s sample over the 20×
  playback factor, floored at 1 ms by `trace_dispatch_interval_milliseconds`). The puck's motion per
  fix is clamped to 120–1000 ms (`LOCATION_ANIMATION_MINIMUM_MILLISECONDS`,
  `LOCATION_ANIMATION_MAXIMUM_MILLISECONDS`).
- **Sampling.** `window.mapNavigationSnapshot()`, `window.mapLocationPose()` and
  `window.mapLastFrameStats()` expose guidance, drawn pose (`isAnimating`) and `drawnTiles` /
  `missingTiles`. A healthy run changes the distance phrase, keeps ground speed near 28 m/s at 1×,
  and reports no missing tiles on settled frames.
- **Follow trip.** `perf/run-follow-trip-budget.mjs` reports requested, drawn and missing tiles per
  second, missed frames and provider requests; `--live` with `PROVIDER_API_KEY` uses one keyed
  session. `follow_trip_tile_coverage.spec.mjs` fails when a post-grace frame misses a tile or
  guidance stops. A missing tile is one the scheduler could not resolve from its own or an
  ancestor's pooled imagery; `missingTileAddresses` names up to sixteen lagging tiles.

On the fixture-backed trip at 640 × 480: about 65 frames per second, 16–24 tiles
requested per frame at zoom 17, and every requested tile covered after the start grace.
At 8× the route leaves the recorded z17 set at `17/21295/*`, so the fixture run reports
gaps for the last seconds; the live source covers them. Remaining distance fell 1,403 m
→ 963 m over 12 s at 1×, 1,396 → 746 m at 2×, and 1,396 → 309 m at 4×.

## Known gaps

- The golden compare and the suite run under `--use-angle=swiftshader`; a hardware-GPU compare run
is untested. Playwright's WebKit binary is not installed; enable it with
  `npx playwright install webkit`.
- OSRM, Valhalla, Photon and the URL-template sources have no SDK-attached observer, so their
requests produce no `ProviderRequestTrace`.
- `setProviderBaseUrl` and `clearProviderRegistrations` remain browser-only: no contract member
carries a credential and a deployment endpoint together, so a credentialed provider
pointed at a fixture endpoint needs the override.
- Each tile binds its own texture and draws a quad; one instanced draw over a `texture_2d_array`
would be cleaner, but the raster path already measures 0.2 ms.
- The overlay-revision tessellation still runs on the presenting frame, and the browser canvas still
calls the owned-frame entry point; comparing revisions on the shared frame would bring
the settled z8 p95 under 4 ms.
- The zoom cross-fade's device-seam assertion on Android and iOS is open.

## Related guides

See [Architecture](Architecture.md) for the four-target contract, core crates and
registry, [Providers](Providers.md) for provider setup and attribution, and
[Release](Release.md) for package production.
