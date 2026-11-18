# The browser target

The browser target draws the map on WebGPU from a WebAssembly module generated
from the same frozen UniFFI contract the native targets use. The page is a
plain ES-module harness with no bundler: `platform/web/` holds the playground,
fixture server, Playwright suite and performance harnesses;
`crates/rustic-maps-native-core/` owns the generated module; and
`crates/rustic-maps-web-bindings/` supplies the canvas and provider slots the
contract cannot express.

## What lives where

| Path | Role |
| --- | --- |
| `crates/rustic-maps-native-core/` | The generated `wasm2` module: contract surface plus the re-exported `browser_canvas` crate; `src/rustic_maps_wasm.udl` is the committed WebAssembly contract |
| `crates/rustic-maps-web-bindings/` | Hand-written canvas: `src/canvas.rs` (`BrowserMapView`), `src/exports.rs` (`MapCamera`), `src/live_providers.rs` (slots, fallback, browser seams), `src/vector_layer.rs`, `src/vector_source_fields.rs`, `src/refapp.rs`, `src/refapp_math.rs`, `src/shared.rs`, `src/web_camera.rs`, `src/generated_contract_field_names.rs` |
| `platform/web/generated/` | The generated package: ESM modules, declarations, `rustic_maps.wasm`, wasm-bindgen glue; gitignored build output |
| `platform/web/types/browser_canvas.d.ts` | Declarations for the untyped wasm-bindgen `rustic_maps_bg.js` glue |
| `platform/web/serve.mjs` | Static server on `127.0.0.1:8642`: harness, fixtures, registry, generated runtime |
| `platform/web/index.html` | The playground: `#map-canvas`, `#map-overlay`, provider console, settings, diagnostics |
| `platform/web/main.mjs`, `playground.mjs` | Opens the module with `uniffiInitAsync`, exposes `window.mapTest` behind `window.mapTestReady`, and drives gestures, layers, navigation, console and frame loop |
| `platform/web/golden.html`, `golden.mjs` | A fixed 512×512 fixture page that renders the committed golden scenes on demand |
| `platform/web/tests/`, `perf/`, `npm/` | The Playwright suite and file gates, the performance harnesses, and the npm manifest for `@angoratek/rustic-maps-sdk-web` |

## The generated surface

### Generation from the contract

`scripts/generate-wasm-bindings.sh` builds `rustic-maps-native-core` for
`wasm32-unknown-unknown`, then runs `ubrn build wasm2 --config ubrn.config.yaml
--release` from `platform/rn`: it compiles, reads the `UNIFFI_META_*` records,
writes the TypeScript, runs `wasm-bindgen`, and stages the module. The
scaffolding and bindgen consume the committed
`crates/rustic-maps-native-core/src/rustic_maps_wasm.udl`, the frozen contract
minus the native `MapView` interface and its doc block, so the 349
signature-table entries and 122 checksums align with the module, zero missing.
Release output:

| Artifact | Size / lines |
| --- | --- |
| `rustic_maps.ts` (the API) | 679,923 bytes, 16,426 lines |
| `rustic_maps-ffi.ts` (signature table) | 139,437 bytes, 2,843 lines |
| `index.ts` (entrypoint) | 2,177 bytes, 55 lines |
| `rustic_maps.wasm` (staged) | 6,575,205 bytes; 5,887,591 after `wasm-opt -Oz` |
| `rustic_maps_bg.js` (wasm-bindgen glue) | 242,699 bytes |
| debug module, for comparison | 38,373,193 bytes |

`wasm2` covers every construct here: 61 dictionaries become `export type` plus
factory; 27 unit enums become numeric `export enum`; the 11 `SdkError` variants
become a tagged class union with detail in `.inner.detail`; the 22 interfaces
(minus `MapView`) become handle-backed classes; 42 async members become
`Promise<T>` with optional `{ signal }`; `[Throws=SdkError]` rejects with one
class per variant; the 12 `[Trait, WithForeign]` traits become `XImpl` classes
and vtable registration exports; `sequence<T>` and `bytes` become `Array<T>`
and `ArrayBuffer`; `u64` becomes `bigint` in 71 fields or arguments;
`Name=fit_bounds` becomes static `fitBounds`; single-threaded wasm is checked by
`ubrn`.

The entrypoint exports `uniffiInitAsync(source)`: it opens the module once,
registers the signature table, calls the namespace `initialize()` checksums,
and is idempotent. When the module imports `__wbindgen_placeholder__`, staging
runs `wasm-bindgen`, keeps `<stem>_bg.js`, and hands it to the player as
`resolveModule`, which calls `__wbg_set_wasm` and `__wbindgen_start`.

### One module, two ABIs

One module serves both the generated contract and the canvas. It carries 5,351
exports: 306 UniFFI exports and metadata globals, plus wasm-bindgen classes
(`mapview_new`, `mapview_renderFrame`, `mapssdk_create`, and the rest). It
imports `./rustic_maps_bg.js` 640 times; the player wires the glue, and a
canvas-side class answers calls after `uniffiInitAsync`.

Objects do not cross the ABIs. A generated `MapSdk` is a JavaScript proxy over
an integer UniFFI handle. The canvas glue's `MapView` constructor passes
`maps.__wbg_ptr` into `wasm.mapview_new`; a generated object has no
`__wbg_ptr`, and wasm-bindgen cannot accept a UniFFI handle, so only primitives
and plain values cross. The registry stays shared by mirroring
`MapSdk::register_*` into the canvas slots on wasm (`provider.rs`'s
`set_shared_*`). The contract's `MapView` never reaches the page: the canvas
class is `BrowserMapView`, and the signature table is asserted to name no
`MapView`.

### Package layout and freshness

`scripts/build-wasm2-package.sh` compiles the TypeScript and clears the output
directory first, so a removed module cannot leave a stale file:

```
index.js / index.d.ts · rustic_maps.js / rustic_maps.d.ts
rustic_maps-ffi.js / rustic_maps-ffi.d.ts · rustic_maps_bg.js / rustic_maps_bg.d.ts
rustic_maps.wasm · SOURCE_FINGERPRINT
```

`wasm-opt -Oz` runs when binaryen is installed, with Rust's wasm feature set;
pass `--no-optimize` to skip it, `--no-generate` to repackage existing bindings,
or `--require-optimize` to fail when it is absent. The npm package is
`@angoratek/rustic-maps-sdk-web`, versioned with the workspace, depending on
`@ubjs/core` and `@ubjs/wasm` at `0.31.0-6`.

`scripts/check-wasm2-package-freshness.mjs` hashes every input a contract
change can travel through: the frozen and stripped UDLs, the build, generation
and packaging scripts, and the relative-specifier rewrite. It compares the
fingerprint with `SOURCE_FINGERPRINT`, asserts the emitted module set, and
enforces the size budget. A package emitted from different sources cannot be
tested.

**Version matching is load-bearing.** The `wasm-bindgen` CLI refuses
mismatches, or corrupts output, when its version differs from the crate's. The
workspace pins `wasm-bindgen 0.2.129`; after a bump, reinstall the CLI at that
version and regenerate `platform/web/generated`. Check both ends with
`wasm-bindgen --version` and `cargo tree -p rustic-maps-native-core | grep
wasm-bindgen`.

### Browser constraints

- Async init is mandatory: nothing touches the surface before
  `uniffiInitAsync` resolves. wasm is single-threaded and per-instance; workers
  are the only isolation, and long Rust calls block the main thread.
- Errors carry detail in `SdkError.inner.detail`; `String(error)` yields the
  variant, and the page façade adapts both to its `{ code, message }` shape.
  `u64` is `bigint` in 71 places, so page and test arithmetic convert
  deliberately.
- `setProviderBaseUrl` and `clearProviderRegistrations` have no contract
  member and remain wasm-only seams.

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

The browser never fetches externally, and an unknown tile answers 404. The
playground camera defaults to the Sacramento anchor (38.5766, -121.4944) at
zoom 14, the recorded neighborhood, so imagery appears immediately. The host is
`127.0.0.1` and the port `8642` unless `PORT` overrides it; the Playwright
configuration reads both from `serve.mjs`. A manual server and a test run share
one port, so stop one first. Plain Chrome over `localhost` resolves WebGPU with
no flags.

### Playwright

```bash
cd platform/web
npx playwright install chromium   # first time only
npx playwright test               # chromium channel, headless
```

The configuration uses the system Chromium channel, a 640×480 viewport,
`deviceScaleFactor: 1`, and one worker. One worker is deliberate: every page
owns a WebGPU device, and parallel processes contend for adapter and device
creation until a readiness wait expires. CI gets one retry because some cases
await an asynchronous tile or label settle; local runs stay strict.

`globalSetup: ./global_setup.mjs` builds a current package (reusing
`target/wasm-bindings` when present) and runs the file-parsing gates before any
browser test: contract sync, the provider-name guard, the platform-parity
floor, style-layer coverage, reference flows, the iOS pod/SwiftPM mirror, the
package freshness gate and npm packaging assertions. It then asserts a WebGPU
adapter in CI, so a GPU-dependent test cannot pass by skipping. `webServer`
runs `node serve.mjs` with `reuseExistingServer: false`.

Tests gate on `window.mapTestReady`, which resolves only after
`uniffiInitAsync` opened the module. GPU-path assertions also gate on
`window.mapRendererMode()`, and the smoke test skips with a documented reason
when the mode degrades to `overlay`.

### Contract sync and import maps

```bash
node --test platform/web/tests/contract_sync.mjs
```

The test needs no server or browser, but the package must be built first. It
asserts two facts:

1. `crates/rustic-maps-native-core/src/rustic_maps_wasm.udl` is exactly
   `crates/rustic-maps-native-core/src/rustic_maps.udl` minus the native
   `MapView` interface and its doc block, keeping `MapSdk`, `MapCamera` and
   `Navigator`.
2. `platform/web/generated` carries `uniffiInitAsync`, the contract classes,
   the glue's distinct `BrowserMapView`, a signature table that never names
   `MapView`, and the workspace version. The npm manifest identity
   (`@angoratek/rustic-maps-sdk-web`) and version match the workspace.

`globalSetup` builds the package and runs this test before every browser run.

No bundler resolves the bare specifiers. `index.html`, `golden.html` and
`generated_wasm_smoke.html` carry an import map for `@ubjs/core`, `@ubjs/wasm`
and `@ubjs/wasm/core` at `/generated-runtime/@ubjs/…`; `serve.mjs` maps that
prefix onto the installed packages.

## What the browser does differently

### WebGPU facts

- **A secure context is required, and localhost counts.** WebGPU exists only
  in secure contexts, and `http://127.0.0.1:8642` qualifies.
- **The Chromium channel works headless with no flags.** The adapter resolves
  with no launch flags at all. The suite still passes
  `--enable-unsafe-webgpu --enable-gpu --enable-features=Vulkan
  --use-vulkan=swiftshader --use-angle=swiftshader --no-sandbox` through
  `platform/web/browser_launch_options.mjs` for GPU-process stability. Dawn on
  Linux runners wants Vulkan, or the adapter initializes and the loop stalls.
- **`drawImage` of a WebGPU canvas reads only a freshly presented frame.**
  Pixel probes send a zero-delta wheel event to re-schedule a GPU frame without
  moving the camera, then sample `getImageData` across the following frames.
- **Playwright's WebKit also resolves WebGPU today.** WebKit 27.2
  (`webkit-2370`, headless on macOS) exposes `navigator.gpu`, initializes in
  `gpu` mode, and reads requested 4 / drawn 4 / missing 0 at zoom 14. A WebKit
  run therefore does not exercise the 2D fallback; it fires only when
  `MapView` fails to acquire an adapter or device.

### The 2D overlay fallback

`BrowserMapView::initialize` fails cleanly when no adapter supports the canvas
or the device request fails: it returns a string error and never panics. The
page catches that, abandons the GPU canvas, and reports `mode overlay (WebGPU
unavailable: …)` in the status line and readout.

In overlay mode `#map-overlay` is the scene: a background grid plus the
projected tile grid from `visible_tile_keys`. In GPU mode it carries only the
opt-in debug tile grid, so the WebGPU-locked canvas never needs a 2D context
(claiming one would poison the surface). `wgpu` has both the `webgpu` and
`webgl` features, but a WebGL2 path is neither implemented nor tested; the
documented fallback is the 2D overlay.

### The keyless raster fallback

A live `{z}/{x}/{y}` source registered by a page is a **fallback**. It serves
only while nothing else serves the raster-tile contract. A configured provider
always outranks it, whatever order the two registrations happen in.

The rule exists because two faults once let a keyless source displace a
credential: the page registered the OpenStreetMap source during renderer
start-up and a remembered credential installed MapQuest into the same
`RasterTiles` slot, so the asynchronous fallback could land last; and the
transport forwarded a provider-declared `User-Agent` to `fetch`, failing the
cross-origin preflight. The provider was never at fault.

**Registry.** The fallback lives beside the per-capability registry, not inside
its `RasterTiles` slot. `registerUrlTemplateRasterTileSource` validates and
stores the fallback only, never replacing a configured raster provider;
registering again replaces the previous fallback. The canvas resolves the
raster provider in this order: the shared slot the contract registry mirrors,
the configured `RasterTiles` provider, the page fallback, then the
recorded-fixture provider for the demo area. `clearProviderRegistrations`
clears configured providers but leaves the fallback, so a cleared page keeps
drawing live tiles; `clearSharedProviderSlots` does the same for the mirrors;
`clearUrlTemplateRasterTileSource` removes the fallback. The wasm façade
(`platform/web/main.mjs`) carries the clear call; `contract_sync.mjs` and the
platform parity gate see it as an addition. The frozen UDL is unchanged.

**Transport and failures.** The browser transport drops the Fetch-forbidden
headers it declares: `user-agent`, `cookie`, `host`, `origin`, `referer` and
`connection`. The native transport still sends the declared user agent, which
OpenStreetMap's tile policy expects from a server. A 404 or 410 is definitive,
so the canvas stops asking; a transport or server failure retries after 2
seconds (`TILE_FETCH_RETRY_BASE_DELAY_MILLISECONDS`), doubling to a 30-second
cap (`TILE_FETCH_RETRY_MAXIMUM_DELAY_MILLISECONDS`). The unavailable-address,
staged-payload and retry-schedule memories are bounded:
`MAXIMUM_TRACKED_TILE_RETRY_SCHEDULES` is 1,024 and the unavailable set clears
at its own maximum, so a long pan neither grows them without limit nor
blacklists a tile. Every request also carries a 30-second deadline from
`DEFAULT_REQUEST_TIMEOUT`.

**Demo and verification.** The page stops passing `userAgent`; the browser
supplies its own. The fallback is registered once, unconditionally, and cannot
displace a configured provider; key restore and renderer start-up may run in
either order. A keyed session makes zero requests to the fallback host; a
keyless session outside the recorded fixtures still draws live tiles; clearing
registrations returns to the fallback without a reload; a cross-origin request
carries no forbidden header; and native tests still observe the declared agent.

### Parity with the native renderer

`crates/rustic-maps-web-bindings/src/canvas.rs` mirrors the native headless
pipeline in `crates/rustic-maps-rendering`. The native crate cannot be reused
directly on wasm because it is `cfg`-gated out there, so the copy follows one
rule: **same WGSL text, same vertex layout, same index pattern**.

- The WGSL (`TILE_QUAD_SHADER_SOURCE`) is copied verbatim: clip-space
  positions in, texture sample out. Vertices are `ClipSpaceTileVertex` chunks
  (`Float32x4` clip position at offset 0, `Float32x2` texture coordinates at
  the `offset_of!` offset), the native encoder's layout.
- Indices are `u16` in the pattern `0, 1, 2, 2, 1, 3` (`INDICES_PER_QUAD` =
  6, `CORNERS_PER_QUAD` = 4), bounded under `MAXIMUM_QUADS_PER_FRAME`.
- Tile textures are `Rgba8Unorm` with a linear, clamp-to-edge sampler. Both
  preferred surface formats (`Bgra8Unorm`, `Rgba8Unorm`) are non-sRGB, so texel
  values reach the screen unchanged, making a browser-versus-golden pixel
  compare meaningful. Degradation matches: an undecodable pooled payload skips
  its quads and is evicted for refetch, while only structural failures surface
  as errors.

The page serializes frames: the binding borrows draw state across the
`renderFrame` future, so the playground keeps a busy/queued loop.

### Canvas versus golden

`platform/web/tests/golden_compare.spec.mjs` renders the committed golden
scenes in `crates/rustic-maps-rendering/fixtures/render/golden/` with the
playground camera: center 38.5766/-121.4944, zoom 14, bearing 0, pitch 0
(pitched variant 45°), field of view 60°, equal to
`DEFAULT_FIELD_OF_VIEW_DEGREES`. Only the **512×512 viewport** of
`golden_image.rs` differs from the harness's 640×480 window.

- **Probe.** Send a zero-delta wheel nudge to re-present, `drawImage` the GPU
  canvas into a same-size 2D probe, and sample `getImageData` across a dozen
  frames, keeping the minimum-error sample; a stale read is a clear-color frame.
- **Geometry.** Use 512×512 with `deviceScaleFactor: 1`, so pixels are 1:1 with
  canvas backing pixels and the sampler never rescales. The clear color
  `[0, 0, 0, 1]` and `CompositeAlphaMode::Opaque` make the 2D round trip
  opacity-neutral (premultiplication is a no-op at alpha 255).
- **Golden bytes.** The allow-listed `/golden/{file}.png` route in `serve.mjs`
  names only the committed goldens; the page `createImageBitmap`s them and
  diffs in-page, same-origin, avoiding a 512×512×4 payload over the Playwright
  protocol. The fixed 512×512 `golden.html` renders the camera on demand.
- **Tolerance.** Mirror `golden_image.rs`: mean absolute per-channel error
  ≤ 2.0, and at most a 0.02 fraction of pixels with a maximum channel error
  exceeding 24. The golden must decode to 512×512; settled statistics must
  match the scene's tile counts and `tileZoomLevel === 14`. Both cameras list
  the trailing tile `14/2663/6286`, which the page resolves through its zoom-13
  parent via `BrowserMapView::stageParentTile`, so it draws. The pitched camera
  also lists `14/2661/6286`, wholly outside the viewport at pitch 45; the quad
  assembler culls it, but it has pooled imagery, so `drawnTiles` is 8 and
  `missingTiles` is 0 (top-down draws 4, missing 0). `missingTiles` reports
  tiles the scheduler could not resolve, never tiles the frame culled. The
  compare is a pixel loop, not a screenshot.
- **Overlay scenes.** The page also renders four Sacramento overlay scenes —
  route line, polygon fill, markers (24 triangles) and traffic flow — against
  their goldens at the top-down camera's counts.
- **Hazards.** Probe only after a forced re-present, and control the window or
  use the fixture page. `--use-angle=swiftshader` renders software ANGLE, whose
  sampler behavior is the difference the tolerance absorbs; if its mean error
  exceeds the budget, drop the argument rather than widening the tolerance. The
  fetch zoom clamps to 14, so higher cameras are not apples-to-apples.

## Tile throughput and zoom

### A bounded parallel pool

`TILE_FETCH_CONCURRENCY` (default 8) bounds every tile fetch pass:

- `TileWarmTask::run` keeps at most that many provider futures in flight
  through a `FuturesUnordered`, refilling as each completes. A key leaves the
  warming set when its outcome is delivered, and a cancelled run unmarks
  everything it did not complete.
- `TileScheduler::prepare_frame` collects budgeted fetches through
  `buffer_unordered(TILE_FETCH_CONCURRENCY)` and inserts results into the
  texture pool serially, so the pool is never mutated concurrently.
- The browser canvas starts network fetches detached, up to
  `MAXIMUM_CONCURRENT_TILE_FETCHES` (32); the warm pass it awaits only moves
  staged payloads into the pool, so it stays on the presenting frame. The gain
  comes from pacing and the pool, not that move.
- The per-tile fixture probe stays: it is the network-free test seam, runs once
  per address, and removing it would make every fixture-backed case depend on a
  live provider. Its extra round trip is bounded by the staging cache.

### Provider pacing and the zoom cross-fade

A URL-template provider creates its token bucket from the source's
`TileSourcePacing` declaration: a capacity of 8 immediate requests and a refill
of 8 per second, matching the pool, so a normal viewport is a burst and is paced
only under sustained demand. A stricter source budget overrides it.

A `ZoomTransition` owned by the surface session (native) and the draw state
(browser) records the tile-z change and drives the frame:

1. **Hold.** After a tile-z change, the frame resolves the previous level's
   visible keys from the pool only, never fetching, and draws them over the new
   level. Projection with the current camera scales them automatically; no
   shader uniform is needed.
2. **Fade.** When at least 85% of the new level's keys are present
   (`ZOOM_TRANSITION_READY_TILE_FRACTION`), or the hold deadline expires, the
   previous level's per-quad opacity ramps from 1 to 0 over 250 ms, keeping slow
   stragglers from delaying the new detail. Old over new with standard alpha
   blending is an exact dissolve.
3. **Settled.** A running transition keeps the frame unsettled, so both pumps
   keep drawing; frame statistics expose the transition state and the
   present/pending counts.

The tile vertex format gains an opacity attribute, the three pipeline copies
switch to alpha blending, and the retained level counts against the frame's
quad budget.

The constants are `TILE_FETCH_CONCURRENCY` (8), which bounds provider futures
in one pass; `DEFAULT_TOKEN_BUCKET_CAPACITY_TOKENS` (8) and
`DEFAULT_REFILL_TOKENS_PER_SECOND` (8), the immediate and sustained tile rates
a URL-template provider admits; `ZOOM_TRANSITION_FADE_MILLISECONDS` (250); and
`ZOOM_TRANSITION_MAXIMUM_HOLD_MILLISECONDS` (1,000), the longest the previous
level waits for a late tile.

A recording provider asserts the pool's observed maximum concurrency is exactly
the constant and the serial floor is gone; `frame_latency` asserts the pooled
ceiling. A browser case crosses a tile level by wheel and by double-tap and
asserts: the previous level draws during the hold, the new level draws when
present, the fade ends, and zoom-out never clears to the background. The raster
and styled-base cases ship. The measured browser numbers (requests in flight,
time to a filled viewport, frame p95 while panning) ship in the fit harness.

## The developer console

Every reference app and the browser demo carry a developer console over the
SDK's structured log stream, so key events are visible without a debugger.

| Surface | Shape |
| --- | --- |
| `MapSdk.subscribe_to_diagnostic_events(listener, minimum_severity)` | Fire-and-forget delivery to a `SdkDiagnosticEventListener` |
| `MapSdk.flush_recent_diagnostic_events(listener, minimum_severity, maximum_count)` | Reads back the recent buffer |
| `MapSdk.set_response_excerpt_capture(enabled)` | Opt-in response-excerpt capture |
| `Diagnostics` façade | `add_listener`, `flush_recent` over the same objects |
| `LogEvent` | timestamp, `severity`, `subsystem_target`, `message_line`, `provider_request_trace` |
| `SdkLogSeverity` | `Trace`, `Debug`, `Info`, `Warn`, `Error` |

Events are already scrubbed: coordinates are rounded and API keys never
appear. The console keeps a level filter (Error, Warning and Info; Info shows
all three), renders one event per line newest first, and opens a detail view
with the full message and, for a provider trace, the fields below. A cache
summary shows the tile-cache and traffic-cache counters.

The observation seam is `crates/rustic-maps-core/src/provider_observation.rs`:
a redacted `ProviderRequestObservation` per attempt (query values removed,
status class, duration, byte count, a bounded character-boundary snippet), with
`ObservedTransport` observing retries per attempt. MapQuest and the five vendor
clients report through it; OSRM, Valhalla, Photon and the URL-template sources
do not. The binding bridges it into the diagnostics ring, and
`LogEvent.provider_request_trace` carries it to every console.

### Provider request trace and contract

`ProviderRequestTrace` carries `provider_key` (the registry key, or
`application` for a foreign implementation), `provider_contract`
(`RoutingProvider`, `GeocodingProvider`, …), `http_method`, `request_path`
(the path only; query values dropped except documented non-secret parameters),
`status_class` (`2xx`, `3xx`, `4xx`, `5xx` or the transport-failure marker),
`outcome` (`success`, `client-error`, …), `duration_milliseconds`,
`response_bytes`, and `response_excerpt` (the first 512 bytes when capture is
on, truncated at a character boundary; null otherwise).

Never included: the credential, its label's value, authorization headers, and
query parameters the transport classifies as secret; a transport test feeds a
credential through a request and asserts neither value nor header appears.
Cache events join the stream as `Info`: tile-cache and routing-cache hits and
misses, each with cache kind and entry key, aggregated per session.

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

`set_response_excerpt_capture(boolean enabled)` turns excerpt capture on for
the session. Emission is gated by the subscription's minimum severity: traces
are `Info`, failures stay `Warn`/`Error`.

The transport sees a URL and a method, not the contract or key, so
`HttpRequest` gains `provider_contract` and `provider_key`, filled in at each
provider's one request-building site per API. The observer is one process-wide
sink installed by the native binding and the wasm façade; with no sink, the
transport skips the observation with no allocation. Redaction happens before
the observation is built: query values are dropped except the allow-list
`format`, `overview`, `geometries` and `units`, the path keeps its structure,
and the excerpt excludes secret-classified content. A credential in a query
parameter never appears because the value is removed while the URL is split.
The excerpt reuses the token vocabulary `key`, `apiKey`, `access_token`,
`session` and `signature`; a credential in a header never exposes the header.

### The console surface, retention and tests

A floating button sits above the attribution notice, opposite the map layer
control, and opens a full-height sheet on a phone or a side panel on a wide
viewport, carrying a dot when a Warning or Error arrived since it was last
opened. The console is read-only over the SDK stream, keeps no state beyond the
filter, never issues a provider call, and sits behind the readiness/diagnostics
area so it is one tap from the map. The browser demo implements it as the
provider console, with identifiers
`provider-console-control`, `provider-console-panel`, `provider-console-rows`,
`provider-console-detail` and `provider-console-alert-dot`. It filters by
level, provider and outcome, shows cache counters, and offers Copy and Export.
Android, iOS and React Native carry the same console.

The web panel also carries the provider request console: one row per request,
naming provider, capability, method, redacted path, status, duration and a
bounded excerpt. Rows filter by provider, capability and outcome, copy as text
and export as JSON. It reads the same stream as a second subscriber and adds
nothing to the request path; the provider list comes from
`platform/providers.json`, never a hardcoded list. Excerpts are opt-in per
session through a switch stating they may contain provider content and are
never uploaded; the excerpt uses `RESPONSE_SNIPPET_MAXIMUM_BYTES` (512),
truncated on a character boundary and credential-free. The empty state names
the action that issues a request.

Retention: the SDK keeps 512 events per subsystem (`DIAGNOSTIC_RING_CAPACITY`),
and the console reads through `flush_recent` without a second copy. The
browser bounds visible rows to `PROVIDER_CONSOLE_MAXIMUM_ROWS` (200) and reads
up to `PROVIDER_CONSOLE_FLUSH_LIMIT` (1,000). A line renders at most three
visual lines; the excerpt is truncated at 512 bytes at emission, so a large
raster payload never enters the buffer. Native exports stream the filtered list
to a temporary file capped at 256 KB and hand it to the share sheet, deleting
it on next launch; the web control copies rows as text or downloads JSON, with
a defensive credential scrub.

Tests: a Rust request through a fake server emits one trace with path, status
class, duration and a 512-byte excerpt; a credential-bearing request emits
neither value nor header; a non-provider event carries no trace; each ring caps
at 512 events. Browser coverage is `developer_console.spec.mjs` (open, filter,
detail, export name, credential omission), `provider_console_panel.spec.mjs`,
`provider_console_provider_matrix.spec.mjs`,
`provider_console_response_excerpt.spec.mjs` and
`provider_request_diagnostics.spec.mjs` (rows, registry-derived list, excerpt
switch, credential-free export). Native targets open the console, render Info
events from an injected listener, and export a file matching the filtered list.

## Performance

Three workloads are measured: a steady pan, a cold fit and a simulated trip.
All drive the page's own frame path and run outside `npm test`, because the
suite renders under SwiftShader, whose absolute frame times say nothing about a
GPU (MapLibre's benchmark under-reported roughly 390× there).

### The frame-budget harness

`platform/web/perf/run-frame-budget.mjs` drives one frozen scenario and reports
percentiles of `renderFrame` (the whole wasm frame call, the work added to the
main thread); the animation-frame interval (the cadence a visitor sees, pinned
at the display refresh even when frames drop); `stageMilliseconds.tilePipeline`
(listing, fetch start, warm pass, quad assembly, texture resolution);
`stageMilliseconds.overlayGeometry` (overlay and indicator tessellation through
the camera pose); `stageMilliseconds.encode` and `.submit` (buffer encoding;
surface acquisition, pass and submission); and `gpuPassMilliseconds` (the
pass's own GPU time from timestamp queries, read a frame later, absent without
support).

The stage spans are `performance.now()` reads inside `renderFrame`, reported on
the statistics object; Chrome tiers wasm down when DevTools is open, so in-page
timing is the honest instrument. The readback ring never cancels or remaps a
pending buffer, so only completed maps are read and a frame with no free slot
carries no sample: Firefox rejects a cancelled-then-remapped buffer ("still
mapped", "already mapped", "map is pending") and can stop the loop presenting.
`frame_loop_cleanliness.spec.mjs` asserts no such error, and
`gpuBufferAllocations` must stop growing once every layer has been seen at its
largest payload, which the suite asserts on a settled golden scene.

**GPU pass time.** Timestamp queries are requested only when the adapter
advertises them and is neither the noop backend nor a CPU fallback. Each pass
writes begin and end timestamps; the query set resolves into one of two
`MAP_READ` buffers, and the next frame reads the previous map, reporting
`gpuPassMilliseconds`, while a pending map is dropped rather than delaying the
present. The value describes the **previous** frame; timestamps are quantized
(Chromium's step is commonly 100 µs), so read it beside `renderFrame`.

**The frozen scenario.** A vector base (the recorded fixtures) over a
1920 × 1080 canvas at device scale 1, or the raster base with `--layer raster`.
After a settle and a warm-up (shader compilation, first uploads, initial vector
reload), a 600 device px/s pan runs for 4 s, out and back, staying on recorded
fixtures. The 60 Hz budget is p95 ≤ 16.7 ms, p99 ≤ 33 ms, no frame > 50 ms.

```bash
cd platform/web
npm run perf                                        # the vector base
node perf/run-frame-budget.mjs                      # with flags
node perf/run-frame-budget.mjs --layer raster       # the tile path
node perf/run-frame-budget.mjs --json report.json   # keep the report
node perf/run-frame-budget.mjs --headed             # watch the pan
```

The runner starts or reuses the static server, launches hardware-WebGPU
Chromium, and reports the browser build and `GPUAdapter.info`.

Apple Silicon host (`apple · metal-3`), headless Chromium with hardware WebGPU,
1920 × 1080 at device scale 1, vector base over the recorded fixtures, 120
frames. Measured 2026-11-01 with the canvas-owned vector base, the world-space
overlay cache warm, and upload buffers reused from their high-water mark:

| Metric | p50 | p95 | p99 | max |
| --- | --- | --- | --- | --- |
| `renderFrame` | 1.4 ms | 1.5 ms | 8.2 ms | 9.8 ms |
| `animation frame` | 16.7 ms | 16.8 ms | 16.8 ms | 16.8 ms |
| `overlayGeometry` | 0.0 ms | 0.1 ms | 7.2 ms | 8.5 ms |
| `encode` | 0.0 ms | 0.0 ms | 0.0 ms | 0.0 ms |
| `submit` | 0.0 ms | 0.1 ms | 0.1 ms | 0.2 ms |
| `gpuPassMilliseconds` | 0.8 ms | 0.82 ms | 0.89 ms | 0.89 ms |

Every frame stayed inside the budget (0 of 120 over 16.7 ms). The p99 tail is
the overlay-revision pass: tessellation runs on the frame that sees a new
overlay list, while the vertex upload writes into the high-water-mark
allocation. Spread is about ±0.2 ms at the median; p95 and p99 move more
because one tail frame sets them. Compare runs on the same machine and power
state.

The raster base is not the bottleneck: `renderFrame` p50 is 0.2 ms with 15 tiles
drawn, because payloads are pooled, textures cached and buffers reused. Its GPU
pass is smaller: `gpuPassMilliseconds` p50 0.15 ms, p95 0.21 ms, p99 0.21 ms
over the same 120 frames. The steady vector frame is inside the budget by a
wide margin; the remaining work is moving the overlay-revision tessellation off
the presenting frame.

### The cold fit: z14 to z7

A fit reveals a whole screen at once, so a cold fit pays every fetch, decode
and first style pass. `perf/run-fit-budget.mjs` measures on a bare surface with
no page pump, over committed z7 and z8 fixtures: one frame at z14, then z7 at
the default cadence until every visible tile has drawn, reporting first-tile
and first-full-frame times, stage times, requested/drawn counts and triangles.

```bash
cd platform/web
node perf/run-fit-budget.mjs                  # readable report
node perf/run-fit-budget.mjs --json fit.json  # keep the report
node perf/run-fit-budget.mjs --headed         # watch the fit
```

The runner fails when the first full frame exceeds 1.5 s, the frame draws more
than 400,000 overlay triangles at z7, the z8 unchanged-settle p95 exceeds
6 ms, or those settle frames extract a label candidate. It launches the host's
Chromium with hardware WebGPU and stays outside `npm test`;
`tests/vector_fit_budget.spec.mjs` gates the hardware-independent part (the
visible set completes and z7 geometry stays inside the triangle budget) and
enforces the time budget only with `MAP_FIT_BUDGET_STRICT=1`.

Four choices keep a fit cheap:

- **Visible-first delivery.** The prefetch bursts visible misses
  (`with_visible_prefetch_burst_tile_count`, defaulting to provider capacity)
  and writes each as it lands; `prefetch_missing_tiles_with_delivery` reports it
  immediately, so the surface repaints on first arrival. The ring ahead waits
  until visible fetching stops, is not planned below z8, and drops stale
  deliveries.
- **Per-tile styled geometry.** Overlays are cached by tile address, style
  revision and style-zoom bucket; a tile styles only itself, and
  `VectorBaseFrameDiagnostics::tessellated_tile_count` is zero on a warm refit.
- **Low-zoom level of detail.** At or below z8 rings and lines are simplified
  to half a pixel and sub-pixel features culled; city zoom stays exact, so the
  z9 and z14 goldens do not change. At z7 in Carto light, one tile drops from
  34,788 to 8,643 triangles, so the fifteen-tile frame draws 129,645 instead of
  521,820.
- **Shared frames.** An unchanged frame is the previous
  `Arc<VectorBaseFrame>`; `content_revision` names its tiles, style revision,
  zoom bucket and sprite atlas, so a surface compares one integer instead of
  the `Arc`-shared list. Over a 46,000-overlay z7 frame, the bench measures a
  24 ns shared-frame clone, a 1.02 ms owned-list clone, a 0.29 ms deep compare
  and a 4 ns revision compare. The browser canvas still uses the owned-frame
  entry point; comparing revisions removes that compare.

**Measured fit.** Apple Silicon host (`apple · metal-3`), headless Chromium
with hardware WebGPU, 1920 × 1080 at device scale 1, the committed z7/z8
fixtures, one render per animation frame:

| Metric | Value |
| --- | --- |
| first drawn base tile | 80 ms |
| first full frame (15 tiles) | 1,059 ms |
| overlay triangles, full frame | 371,745 |
| `overlayGeometry` p95 / max | 41 ms / 51 ms |
| settled `renderFrame` p50 / p95 | 2.1 ms / 5.0 ms |

The triangle count is measured after the world-copy split, roughly three times
the camera-side geometry count the budget test reports. The full frame lands
inside both budgets; the settled p95 is above the 4 ms target until the browser
canvas compares `content_revision` and stops re-placing unchanged labels every
frame.

**z8 settle and the label candidate cache.** After the fit, the harness moves
to z8, drains the tile-level cross-fade, and records 30 unchanged settle
frames. Each tile caches its label candidates against the tile address, style
revision and zoom bucket, so those frames read them instead of walking the
symbol layers again.

| Metric | Value |
| --- | --- |
| z8 settle `renderFrame` p50 / p95 | 2.1 ms / 5.8 ms (budget 6 ms) |
| z8 overlay triangles | 371,965 |
| label candidate extractions / cache hits | 0 / 15 |

The p95 is the budget line; one sample was a 54.8 ms tail, reported because the
distribution is the claim. The fit table is an earlier run: the first base tile
varies between 80 and 88 ms and the first full frame between 1.06 and 1.16 s.
`tests/vector_zoom_cross_fade.spec.mjs` asserts that a zoom-out keeps the
retired level drawn at the settled frame's triangle count without clearing to
the background, and that a cached level draws when the camera returns. Frame
statistics report `vectorZoomTransitionRunning`, `vectorRetainedTiles`,
`vectorBaseLabelCandidateExtractionCount` and
`vectorBaseLabelCandidateCacheHitCount` beside the raster fields.

### Motion grading and guided trips

The vector base grades per-frame work by camera motion: during a pan, zoom or
rotate it paints decoded tiles and the last settled style-zoom bucket's
geometry, skipping label assembly; the first still frame resolves the bucket
and labels at the final pose. The browser reports gestures through
`setCameraGestureActive`, and the frame-budget scenario drives a scripted pan.

Same host and scenario as the baseline (1920 × 1080, vector base over the
recorded fixtures, 600 px/s pan for 4 s, 120 frames), measured 2026-11-04,
three graded runs and two control runs:

| `renderFrame` | p50 | p95 | p99 | max |
| --- | --- | --- | --- | --- |
| graded (118 of 120 frames; range over 3 runs) | 1.4–1.8 ms | 1.6–2.1 ms | 8.1–10.4 ms | 8.5–10.6 ms |
| ungraded (control; range over 2 runs) | 8.0–8.1 ms | 8.3–8.4 ms | 12.6–13.0 ms | 14.9–15.3 ms |

The graded median is roughly 4.5–5.7× lower. The graded p99 and max are the
two ungraded frames the window still contains — the measured pass ends with
the settle frame — not a graded frame. To reproduce, run `npm run perf` with
the grading seam and once without; the report's `motion:` line states how many
samples ran graded. Compare runs on the same host and power state.

- **Cadence.** A simulated trip adds the guidance card and location puck. The
  demo dispatches fixes every 250 ms at 1× (the artifact's 5 s sample interval
  over the 20× playback factor, floored at 1 ms by
  `trace_dispatch_interval_milliseconds`). The puck's motion per fix is the
  fix interval clamped to 120–1000 ms
  (`LOCATION_ANIMATION_MINIMUM_MILLISECONDS`,
  `LOCATION_ANIMATION_MAXIMUM_MILLISECONDS`), so the drawn pose advances every
  animation frame rather than every fix.
- **Sampling.** `window.mapNavigationSnapshot()` gives guidance values,
  `window.mapLocationPose()` the drawn pose (its `isAnimating` flag says a
  motion is in flight), and `window.mapLastFrameStats()` `drawnTiles` /
  `missingTiles`; the Advanced pane shows the same numbers. A run is healthy
  when the distance phrase changes across the window, the pose's ground speed
  stays near the trip's pace (28 m/s at 1×) instead of hopping, and
  `missingTiles` is zero on settled frames. A miss after a camera move is the
  scheduler fetching; a miss on a settled frame is a provider or pacing
  problem, named by the diagnostics stream.
- **The follow-trip budget.** `platform/web/perf/run-follow-trip-budget.mjs`
  drives the same user path and reports per second: requested, drawn and
  missing tiles, frames that missed, and provider requests from the diagnostics
  ring. It runs outside `npm test` because live mode reaches the network; add
  `--live` and set `PROVIDER_API_KEY` for one keyed session, where each tile is
  a round trip paced by the declared budget (eight immediate, eight per second
  by default). The committed regression, `follow_trip_tile_coverage.spec.mjs`,
  fails when a frame after the grace period misses a tile or guidance stops.
- **`missingTiles`.** A tile is missing only when the scheduler could not
  resolve it from its own or an ancestor's pooled imagery. A culled off-screen
  tile with imagery is a draw-list property, not a gap; `missingTileAddresses`
  names the tiles that lagged, bounded to sixteen.
- **Measured.** On the fixture-backed trip at 640 × 480: about 65 frames per
  second, 16–24 tiles requested per frame at zoom 17, and every requested tile
  covered after the start grace. The same holds at 2× (66 m/s) and 4×; at 8×
  the route leaves the recorded z17 set at its western edge (address
  `17/21295/*` has no fixture), so the fixture run reports gaps for the last
  seconds. The live source answers those addresses with no missing tiles but
  advances more slowly, so at-speed 8× coverage is verified on the published
  demo. Remaining distance fell from 1,403 m to 963 m over 12 s at 1×, 1,396 m
  to 746 m at 2×, and 1,396 m to 309 m at 4×.

## Known gaps

- The golden compare and the suite run under `--use-angle=swiftshader`; a
  hardware-GPU compare run is untested. Playwright's WebKit binary is not
  installed; enable it with `npx playwright install webkit`.
- OSRM, Valhalla, Photon and the URL-template sources have no SDK-attached
  observer, so their requests produce no `ProviderRequestTrace`.
- `setProviderBaseUrl` and `clearProviderRegistrations` remain browser-only:
  no contract member carries a credential and a deployment endpoint together,
  so a credentialed provider pointed at a fixture endpoint needs the override.
- Each tile binds its own texture and draws a quad; one instanced draw over a
  `texture_2d_array` would be cleaner, but the raster path already measures
  0.2 ms, so this is a cleanliness change.
- The overlay-revision tessellation still runs on the presenting frame, and the
  browser canvas still calls the owned-frame entry point; comparing revisions
  on the shared frame would bring the settled z8 p95 under 4 ms.
- The zoom cross-fade's device-seam assertion on Android and iOS is open.

## Related guides

See [Architecture](Architecture.md) for the four-target contract, core crates
and registry, [Providers](Providers.md) for provider setup and attribution, and
[Release](Release.md) for package production.
