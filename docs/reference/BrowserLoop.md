# The browser loop (`platform/web/`)

This is the maintainer guide to the WebAssembly browser harness. It shows
you how to build the `pkg`, serve it, test it in Playwright, and what the
WebGPU canvas is allowed to assume. Every fact below is grounded in the
named files.

## What lives where

| File | Role |
| --- | --- |
| `crates/rustic-maps-web-bindings/` | `wasm-bindgen` cdylib: the JS surface (`exports.rs`) and the WebGPU canvas (`src/canvas.rs`, wasm32-only) |
| `platform/web/pkg/` | Generated `wasm-bindgen` ES module (`--target web` shape); generated locally and gitignored, never committed |
| `platform/web/serve.mjs` | Dependency-free static server on `127.0.0.1:8642` with the same-origin `/tiles/{zoom}_{x}_{y}.png` fixture route into `crates/rustic-maps-rendering/fixtures/render/tiles/` |
| `platform/web/index.html` | The playground: drag-pan, wheel zoom, bearing/pitch sliders, WebGPU canvas (`#map-canvas`) with the 2D tile-grid overlay (`#map-overlay`) above it |
| `platform/web/main.mjs` | Loads the wasm module, creates the `MapsSdk` root (`await MapsSdk.create()`), and exposes the bound surface plus that root on `window.mapTest`; the `window.mapTestReady` promise gates every test |
| `platform/web/tests/sdk_core.spec.mjs` | The core Playwright tests (version, projection, round trip, antimeridian listing, WebGPU smoke, drag readout, overlay and theme controls) |
| `platform/web/types/rustic_maps.d.ts` | Generated TypeScript mirror of the frozen UniFFI contract's records, enums and error unions; `scripts/generate-web-facade.mjs` writes it from the UDL (snake-case contract fields become lower camel case to match the runtime objects) and appends the hand-written projections in `rustic_maps_surface.d.ts` |
| `platform/web/tests/contract_sync.mjs` | Contract-sync test (no npm dependency): runs the generator's `--check` mode and parses the wasm export sources to assert they serve the shared camera/projection surface |
| `platform/web/playwright.config.mjs` | Chromium channel, viewport 640×480, `deviceScaleFactor: 1`, `globalSetup: ./global_setup.mjs` (the contract-sync assertions), `webServer: node serve.mjs` with `reuseExistingServer: false` |

## Building the pkg

The harness has no bundler and no `wasm-pack`; `wasm-bindgen-cli` runs
directly. Use the same commands as the README's WebAssembly section:

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129   # must match the crate version
cargo build -p rustic-maps-web-bindings --release --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir platform/web/pkg \
  target/wasm32-unknown-unknown/release/rustic_maps_web_bindings.wasm
```

`scripts/build-wasm.sh` runs those two steps and, when binaryen's
`wasm-opt` is installed, optimizes the module (`-Oz`, about 91% of the
unoptimized size). Pass `--no-optimize` to skip it.

**Version matching is load-bearing.** The CLI refuses mismatches, or
corrupts the output, when its version differs from the `wasm-bindgen`
crate version the wasm binary was compiled with. The workspace pins
`wasm-bindgen 0.2.129` (the `crates/rustic-maps-web-bindings` dependency tree),
and the installed CLI is 0.2.129. After you bump the crate version, you
must reinstall the CLI at the same version and regenerate
`platform/web/pkg`. Check both ends with `wasm-bindgen --version` and
`cargo tree -p rustic-maps-web-bindings | grep wasm-bindgen`.

Clippy runs for both hosts on this crate: `cargo clippy -p
rustic-maps-web-bindings --target wasm32-unknown-unknown`. The GPU canvas slice
is `cfg`-gated to wasm32.

## The contract-sync test (UDL ↔ wasm surface)

Run the sync test with `node --test platform/web/tests/contract_sync.mjs`.
It asserts that the wasm surface mirrors the frozen UniFFI contract
`crates/rustic-maps-native-core/src/rustic_maps.udl`, and it needs neither npm
nor a browser: it first runs `scripts/generate-web-facade.mjs --check` against
the committed TypeScript mirror (`platform/web/types/rustic_maps.d.ts`) and
the generated Rust field-name constants, then reads the hand-reviewed wasm
exports (`crates/rustic-maps-web-bindings/src/exports.rs`) and compares the
shared camera/projection surface.

Contract fields and enum values reach the mirror only through the generator:
`tile_x` becomes `tileX`, for example, and the UniFFI types' TypeScript
spellings
(`double`/`u32` ↔ `number`). It then requires the frozen `MapCamera`
methods and free helpers in the UDL and in both wasm sources, and it
rejects a wasm member the UDL lacks (the wasm-only lifecycle helpers
`initializeSdk` and `devicePixelRatioThreshold` are the documented
exceptions; `MapsSdk.create()` is the SDK root's constructor, not a
contract member). The comparison is addition-tolerant: contract-only members
(the `MapSdk` provider surface, the `fit_bounds` constructor) are never
required on wasm, so the UDL can grow without breaking the test. The
generated `pkg/rustic_maps_web_bindings.d.ts` is checked the same way whenever
the build artifact is present. You need no server or browser session to
run it, and the Playwright config's `globalSetup` also runs it before
every `npx playwright test`.

## Serving the harness and testing by hand

```bash
node platform/web/serve.mjs    # http://127.0.0.1:8642/
```

The server answers `/` and static paths from `platform/web/`, and
`/tiles/{zoom}_{x}_{y}.png` from the recorded platform tile fixtures. The
browser never fetches from an external network, and unknown tiles answer
with a 404. The playground camera defaults to the Sacramento anchor
(38.5766, -121.4944) at zoom 14, which is the recorded fixture neighborhood,
so the page shows real imagery immediately.

The browser needs no flags for the manual loop: plain Chrome over
`localhost`, which is a secure context, resolves WebGPU.

## Running the Playwright e2e suite

```bash
cd platform/web
npx playwright install chromium   # first time only
npx playwright test               # chromium channel, headless
```

Three config details matter. `deviceScaleFactor: 1` makes the canvas
backing pixels equal the viewport pixels. The viewport is 640×480. And
the config's own webServer spins up `serve.mjs` with
`reuseExistingServer: false`; because port 8642 is hardcoded, a manually
started server or another running harness will conflict. Kill it first,
or the run fails to bind (see "Known gaps").

Tests gate on `window.mapTestReady`, which resolves only after the
Rust-side `MapsSdk.create()` completed (`initializeSdk()` remains the
legacy alias). GPU-path assertions additionally gate on
`window.mapRendererMode()`, and the smoke test skips with a documented
reason when the mode degrades to `overlay`.

## WebGPU facts

- **A secure context is required, and localhost counts.** WebGPU exists
  only in secure contexts, and `http://127.0.0.1:8642` qualifies.
- **The Chromium channel works headless with no flags.** The adapter
  resolves with no launch flags at all, verified during the canvas
  slice. The Playwright config still passes `--enable-unsafe-webgpu
  --enable-gpu --enable-features=Vulkan --use-vulkan=swiftshader
  --use-angle=swiftshader --no-sandbox`: belt-and-braces flags for GPU
  process and software-vulkan stability in headless, not requirements in the
  current build.
- **`drawImage` of a WebGPU canvas reads only a freshly presented
  frame.** Once the compositor has consumed a presented frame, later
  `drawImage` calls stop yielding those bytes. The smoke test works
  around this: it sends a zero-delta wheel event, which re-schedules a
  GPU frame without moving the camera, then samples `getImageData`
  across the following animation frames. Reuse this pattern for any
  future pixel probe of `#map-canvas`.
- **Playwright's WebKit also resolves WebGPU today.** (WebKit 27.2,
  `webkit-2370`, verified headless on macOS.) `navigator.gpu` exists, the
  page initializes in `gpu` mode, and the frame statistics read requested
  4 / drawn 4 / missing 0 at fetch zoom 14 with real imagery on the
  surface. A WebKit run therefore does not exercise the 2D overlay
  fallback today; it fires only when `MapView` fails to acquire an
  adapter or device.

## The 2D-overlay fallback story

`MapView::initialize` fails cleanly when no WebGPU adapter supports
the canvas, or when the device request fails: it returns a string error
and never panics. The Playwright page catches that, abandons the GPU
canvas, and reports `mode overlay (WebGPU unavailable: …)` in the status
line and the readout. In overlay mode the separate `#map-overlay` canvas
*is* the scene: a background grid plus the projected tile grid from
`visible_tile_keys`. In GPU mode the overlay carries only the opt-in
debug tile grid, so the WebGPU-locked canvas never
needs a 2D context of its own (claiming one would poison the surface).

Two notes qualify how broad that fallback is:

- `wgpu` is compiled with both the `webgpu` and `webgl` workspace
  features, so the wasm adapter request could in principle also reach a
  WebGL2 context on canvas. The canvas slice deliberately does not
  implement or test that path; the documented fallback is the 2D overlay.
- The plan once said "WebGL2 fallback checked in Safari/WebKit". The
  WebKit finding above softens it: current WebKit ships WebGPU, so the
  thing to check there is *graceful degradation*, not WebGL.

## Parity with the native renderer

`crates/rustic-maps-web-bindings/src/canvas.rs` mirrors the native headless
pipeline in `crates/rustic-maps-rendering`. The native crate cannot be reused
directly on wasm because it is `cfg`-gated out there, so the copy follows
one rule: **same WGSL text, same vertex layout, same index pattern**.

- The WGSL (`TILE_QUAD_SHADER_SOURCE`) is copied verbatim from the native
  renderer: clip-space positions in, texture sample out.
- Vertices are `ClipSpaceTileVertex` chunks (`Float32x4` clip position at
  offset 0, `Float32x2` texture coordinates at the `offset_of!` offset),
  the same memory layout the native `rustic-maps-rendering` encoder emits.
- Indices are `u16` in the two-triangle pattern `0, 1, 2, 2, 1, 3`
  (`INDICES_PER_QUAD` = 6, `CORNERS_PER_QUAD` = 4), with the caller
  bounding fragments under `MAXIMUM_QUADS_PER_FRAME` before encoding.
- Tile textures are `Rgba8Unorm` with a linear, clamp-to-edge sampler.
  Both preferred surface formats (`Bgra8Unorm`, `Rgba8Unorm`) are
  non-sRGB, so tile texel values reach the screen unchanged. This parity
  is what makes a browser-vs-golden pixel compare meaningful.
- Degradation rules match too: an undecodable pooled payload skips its
  quads for the frame and is evicted from the pool for refetch, while
  only structural failures surface as errors.

The page must also serialize frames. The wasm binding borrows its draw
state across the `renderFrame` future, so overlapping calls are a borrow
violation; the playground keeps a busy/queued render loop.

## Canvas-vs-golden compare (`tests/golden_compare.spec.mjs`)

The committed golden scenes (`crates/rustic-maps-rendering/fixtures/render/golden/`)
are rendered with the exact camera the playground defaults to: center
38.5766/-121.4944, zoom 14, bearing 0, pitch 0 (pitched variant 45°), field
of view 60° == `rustic-maps-rendering`'s `DEFAULT_FIELD_OF_VIEW_DEGREES`. The
only parameter to bridge is the **512×512 viewport** of `golden_image.rs`
versus the harness's 640×480 window.

- **Probe mechanics** (reuse the smoke test's proven pattern): send a
  zero-delta wheel nudge to re-present, then `drawImage` the GPU canvas
  into a same-size 2D probe and sample `getImageData` across a dozen
  animation frames, keeping the best (minimum-error) sample. A stale
  pre-presentation read is a clear-color frame and maximizes the error
  rather than matching.
- **Pixel geometry**: set the viewport to 512×512 (or size the canvas
  explicitly) with `deviceScaleFactor: 1`, so composited pixels are 1:1
  with canvas backing pixels and the sampler never rescales. The clear
  color `[0, 0, 0, 1]` and `CompositeAlphaMode::Opaque` make the 2D round
  trip opacity-neutral (premultiplication is a no-op at alpha 255).
- **Golden bytes reach the page** the way tile fixtures do: an added
  allow-listed route in `serve.mjs` (for example `/golden/{file}.png`,
  naming only the two committed goldens) keeps the compare same-origin
  and lets the page `createImageBitmap` the golden and diff in-page. This
  avoids a 512×512×4 payload over the Playwright protocol. A dedicated
  minimal fixture page (a fixed 512×512 canvas rendering the golden
  camera on demand) beats driving the playground UI for this test.
- **Tolerance strategy**: mirror `golden_image.rs` verbatim. The mean
  absolute per-channel error stays ≤ 2.0, and at most a 0.02 fraction of
  pixels may have a maximum channel error exceeding 24. Add preconditions
  as well: the golden decodes to 512×512; settled frame statistics match
  the scene's tile counts and `tileZoomLevel === 14`; and the pitched
  camera's fully culled 14/2661/6286 leaves one requested-but-undrawn
  tile, so its expected `missingTiles` is 1, not 0. The comparison there
  is pixel-loop based, not screenshot-based, which is the right
  granularity for the browser probe too. The page also reproduces the
  golden harness's deliberate parent-fallback tile (the last visible
  scene tile drawn from its pooled zoom-13 parent via
  `MapView::stageParentTile`); without it the browser
  would draw sharper imagery than the reference.
- **Hazards**:
  1. The freshness rule above: probe only right after a forced
     re-present, and never assume that `renderFrame`'s resolution
     implies readable bytes.
  2. Canvas size mismatch: the playground resizes its canvases to the
     window, so the test must control the window or use a dedicated page.
  3. `--use-angle=swiftshader` renders the software ANGLE path, whose
     sampler behavior is exactly the GPU-vs-GPU difference the tolerance
     absorbs. If the SwiftShader mean error exceeds the budget, drop the
     swiftshader argument for this test project rather than widening the
     shared tolerance.
  4. The fetch zoom clamps to the fixtures' maximum (14), so cameras
     above zoom 14 are not apples-to-apples against these goldens.

## Known gaps

- `serve.mjs` hardcodes the host and port (`127.0.0.1:8642`), and the
  Playwright config runs with `reuseExistingServer: false`. Two harness
  sessions, or a manual server plus a test run, therefore cannot share
  the machine's single port. A test-only copy with an environment-variable
  port override is the current workaround; upstreaming that override
  would be a one-line change.
- `scripts/build-wasm.sh` automates the pkg step (the release build, the
  `wasm-bindgen` emit and, when binaryen is installed, `wasm-opt -Oz`).
  `pkg/` is gitignored, so the output is never committed.
- Playwright's WebKit browser binary is not installed by the harness (the
  config uses the system Chromium channel, `channel: 'chromium'`, in
  `playwright.config.mjs`). Enable the WebKit check with
  `npx playwright install webkit` before running it.
- The project's chromium tests use `--use-angle=swiftshader` (software
  GPU); a hardware-GPU compare run is untested.