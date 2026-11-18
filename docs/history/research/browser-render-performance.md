# WebAssembly map-renderer performance: evidence and a prioritized plan

Date: 2026-10-30. Scope: the browser path (`map-web-bindings` → wgpu/WebGPU).

## 0. What the code does today (measurement targets, not assumptions)

- Every `renderFrame` re-tessellates **all** overlays on the calling thread: `overlay_triangle_vertices` at `crates/map-rendering/src/surface.rs:788` (wasm path `crates/map-web-bindings/src/canvas.rs:1069`). The tessellator (`overlay.rs`)
  ear-clips polygons with holes in O(n²) (`overlay.rs:1107`), builds screen-space joins per interior vertex (`overlay.rs:649`), and clips every
  triangle against three world-copy windows (`overlay.rs:974`), into a fresh `Vec` per frame.
- The vector base is delivered through `setOverlays` as one JS descriptor **per styled feature per tile**: `platform/web/tile_switch.mjs:344`, built from `{latitude, longitude}`
  objects in `crates/map-web-bindings/src/vector_layer.rs:235` and parsed back field-by-field via `Reflect` in `canvas.rs:1347`. Nine tiles (`playground.mjs:1045`), potentially 10⁴–10⁵ coordinate
  objects per reload.
- Tile quads and overlays create **new GPU buffers every frame**: `create_filled_buffer` (`surface.rs:766-821`), `create_buffer_init` (`canvas.rs:1117-1151`). Tile textures are cached by
  payload revision (good).
- Raster tiles are decoded on the render path **twice**: once to validate in `preload_tile_payload` (`canvas.rs:650-662`) and again in `tile_texture_and_bind_group` (`canvas.rs:499-505`), plus
  copies at `canvas.rs:731` and `surface.rs:939`.
- Missing tiles are fetched **sequentially inside the frame** with `await` (`canvas.rs:1663-1681`), up to 32 round trips per `renderFrame`, while the
  draw-state `RefCell` borrow is held.
- Per frame the code also rebuilds `Renderer::new` (`canvas.rs:1025`), serializes vertices per component with `to_le_bytes` (`surface.rs:1084`), and issues one `set_bind_group`
  + `draw_indexed` per tile fragment (`canvas.rs:1191-1205`).

## 0.1 Do this first (highest payoff per unit risk)

1. **Tessellate overlays once per overlay revision in world space; project on the GPU.** Replace clip-space vertices
   with world-space positions plus per-vertex extrude/normal attributes; add a uniform view-projection matrix and
   viewport size, and do the screen-space width offset in the vertex shader. This removes ear clipping, joins,
   world-copy clipping and per-frame `Vec` growth from the camera-motion path. Caching clip-space output by camera pose
   only helps while the camera is static and is a stopgap, not the fix. Risk: visual parity (width, joins, holes,
   antimeridian); gate on the golden scenes and the browser golden-compare page.
2. **One persistent tile draw.** Allocate vertex/index buffers at a high-water mark and `write_buffer` into them; draw all tile
   quads with one instanced call from a unit quad plus per-instance rect/UV/layer, with tiles in a `texture_2d_array` and one cached
   bind group. Overlay drawing is already one draw. Risk: low; shaders change, the scene contract does not.
3. **Get decode and fetch off the frame path.** Decode each raster payload once (or let `createImageBitmap` decode off-thread and use
   `copyExternalImageToTexture`), prefetch on camera change instead of inside `renderFrame`, fan fetches out in parallel, and let the existing `TileScheduler` defer
   whatever is not ready. Risk: low; the scheduler already models deferral.

**Target frame budget (1080p, 60 Hz):** 16.7 ms total; main thread (JS glue + wasm geometry + encoding) ≤ 6 ms; GPU ≤ 8
ms; compositor/other ≤ 1 ms; ≥ 2 ms headroom. p95 ≤ 16.7 ms and p99 ≤ 33 ms while panning; no frame > 50 ms. No network
or image decode inside a presenting frame.

## 1. How the fast web map renderers stay fast

### 1.1 MapLibre GL JS / Mapbox GL JS

- **Worker-side parse and tessellation.** "Vector tiles are fetched and parsed on WebWorker threads"; `WorkerTile#parse` builds a `Bucket`
  per layer family, and "Bucket is the single point of knowledge about turning vector tiles into WebGL buffers" ([ARCHITECTURE.md](https://github.com/maplibre/maplibre-gl-js/blob/main/ARCHITECTURE.md)).
  `ArrayGroup` on the worker becomes `BufferGroup` on the main thread, "tuned for consumption by WebGL" ([bucket.ts](https://cdn.jsdelivr.net/npm/maplibre-gl@6.7.0/src/data/bucket.ts)).
- **Upload once per tile, keep the buffers.** `Tile#upload` → `Bucket#upload` runs when a tile becomes ready; `Bucket#destroy` releases resources
  ([life-of-a-tile.md](https://github.com/maplibre/maplibre-gl-js/blob/main/developer-guides/life-of-a-tile.md)). Frames bind cached buffers and `drawElements` per layer/tile; they do not rebuild geometry.
- **Tile cache with view-dependent sizing and LRU eviction.** `maxTileCacheSize` / `minTileCacheSize` default to `approxTilesInView * 5`; off-screen tiles go to an LRU
  cache ([source_cache.ts](https://github.com/mapbox/mapbox-gl-js/blob/main/src/source/source_cache.ts), [Map API](https://docs.mapbox.com/mapbox-gl-js/api/map)).
- **Geometry independent of frame state.** `LineBucket` computes extrude vectors and emits layout vertices/indices once, packing
  normals into a byte with `EXTRUDE_SCALE = 63` ([line_bucket.ts](https://github.com/maplibre/maplibre-gl-js/blob/main/src/data/bucket/line_bucket.ts)); [line.vertex.glsl](https://github.com/mapbox/mapbox-gl-js/blob/master/src/shaders/line.vertex.glsl) applies width, gap width, DPR and antialiasing per frame. Width changes do
  not re-tessellate.
- **Symbols:** layout and collision boxes are computed in the worker; per-frame placement is a known hotspot with many
  colliding symbols ([issue #7699](https://github.com/maplibre/maplibre-gl-js/issues/7699)).
- **Render-to-texture is real but scoped.** GL JS's terrain render cache renders batches of draped layers into offscreen
  framebuffers and reuses them across interactive frames; cache efficiency was 31.8% for `streets-v11` and 100% for `light-v10`/`dark-v10`,
  with 60 FPS panning versus 17 FPS static on the test machine ([PR #10258](https://github.com/mapbox/mapbox-gl-js/pull/10258)). Mapbox's native v10 render cache stores draw-call
  output as textures and reported +7–13 FPS and 25–30% core-rendering speedups ([v10 RC](https://www.mapbox.com/blog/maps-sdk-v10-release-candidate)). Correction to the brief: GL JS
  does **not** rasterize the ordinary 2D vector map to textures; static imagery is the server-side Static Images API.
  The transferable pattern is cached per-tile geometry and cached expensive layers.
- **Practical tuning:** combine layers/sources, prefer vector tiles over GeoJSON, use style-optimized tiles, avoid
  needless expressions ([Mapbox performance guide](https://docs.mapbox.com/help/troubleshooting/mapbox-gl-js-performance/)).

### 1.2 deck.gl

- Layers are matched across cycles by `id`; only changed props trigger `updateState`, and `finalizeState` releases GPU resources ([layer lifecycle](https://deck.gl/docs/developer-guide/custom-layers/layer-lifecycle)).
- `AttributeManager` supports **per-attribute invalidation** and rebuilds only what was invalidated; it "intentionally does not do
  advanced change detection", and `updateTriggers` use shallow comparison ([attribute management](https://deck.gl/docs/developer-guide/custom-layers/attribute-management)). Binary attributes or partial `_dataDiff` ranges bypass
  CPU attribute generation ([Layer API](https://deck.gl/docs/api-reference/core/layer)).
- Instanced attributes exist to "minimize draw calls"; the performance guide says layer update is the expensive
  operation and recommends chunked loading, `updateTriggers` over new data, and `visible` over add/remove ([performance](https://deck.gl/docs/developer-guide/performance)).

### 1.3 Tangram, MapKit JS, Google Maps JS

- **Tangram:** `TileManager`/`tileCache`, a worker `JobQueue`, cached GL render state (`cacheGlState`, batched `flushResourceDeletion`) and an offscreen selection buffer
  ([map.cpp](https://github.com/styluslabs/tangram-ng/blob/6e836ea2/core/src/map.cpp)). 512 px tiles fetch one zoom level lower and cut request counts; tile LOD by screen area and proxy tiles on
  failure were added in the fork ([v0.13.0](https://github.com/tangrams/tangram/releases/tag/v0.13.0), [tangram-ng](https://github.com/styluslabs/tangram-ng)).
- **MapKit JS:** adaptive rendering modes per device — full client-side WebGL, labels-only client rendering, and fully
  server-rendered tiles ([WWDC 2018](https://nonstrict.eu/wwdcindex/wwdc2018/212/)). Startup is prioritized with `loadPriority`, libraries load on demand, and the loader is
  non-blocking ([Tech Talk](https://developer.apple.com/videos/play/tech-talks/110353/)).
- **Google Maps JS:** vector maps are vector tiles "rendered client-side using WebGL", with raster fallback when
  hardware acceleration is unavailable ([rendering type](https://developers.google.com/maps/documentation/javascript/map-rendering-type), [WebGL support](https://developers.google.com/maps/documentation/javascript/webgl/support)); fractional zoom, tilt/heading and WebGL Overlay View sit on the
  same renderer ([WebGL overview](https://developers.google.com/maps/documentation/javascript/webgl), [I/O 2021](https://mapsplatform.google.com/resources/blog/google-maps-platform-announcements-google-io-2021/)).
- **Common pattern:** decode, style and tessellate once per tile/revision; move typed arrays rather than object trees;
  keep GPU buffers for the tile's lifetime; per frame update uniforms, bind cached resources and draw. Our renderer
  violates the last sentence in all three parts.

## 2. WebAssembly-specific overheads and how to avoid them

### 2.1 Crossing is cheap; marshalling is not

- An empty wasm call is ~2 ns; a 100k-element `f64` slice costs ~71 µs versus 74 ns for 8 elements — cost scales with
  bytes ([measurements](https://www.webassembly-wasm.com/compilation-pipelines-toolchain-setup/wasm-performance-benchmarking/measuring-js-to-wasm-call-overhead/)).
- Exported `&[T]` parameters copy in **and** out; returning `Vec<T>` copies out. Import views are zero-copy only during the
  call ([wasm-bindgen guide](https://wasm-bindgen.github.io/wasm-bindgen/reference/attributes/on-js-imports/slice.html)). A view still allocates a JS TypedArray object, so per-frame views add GC pressure ([issue #1615](https://github.com/rustwasm/wasm-bindgen/issues/1615)).
- Chrome, 1M `f32`: copy-in ≈ 0.6 ms, copy-out ≈ 0.7 ms, real work ≈ 1.9 ms; a reused persistent buffer removes both
  copies ([passing arrays](https://www.webassembly-wasm.com/js-wasm-interop-memory-management/passing-complex-types-across-the-boundary/passing-arrays-between-javascript-and-wasm/)).
- `JsValue` trees and `Reflect::get` are the worst case: a boundary crossing plus property lookup and often an allocation **per
  field**. A `Vec<JsValue>` parameter also clones each handle into wasm.

### 2.2 What this means here

- The vector base makes a full round trip: MVT → Rust structs → JS objects (one per coordinate) → wasm → Rust structs
  (`vector_layer.rs:235`, `canvas.rs:1347`). Keep styled geometry in Rust keyed by tile and style revision. If JS must see it, expose flat typed
  arrays (`Float32Array` positions, `Uint32Array` indices, a small descriptor table).
- Tile bytes are copied at least three times before upload (fetch view → `Vec` → pool `Vec` → RGBA buffer). Use one
  owned buffer per tile and decode in place or in a worker.
- The `RefCell` is not the problem: `borrow_mut` costs nanoseconds. The problem is that the borrow spans `await`s, forcing full frame
  serialization (the page's busy flag already serializes too). Fix the awaits, not the `RefCell`.
- Reuse `Vec`s across frames (`clear()` + `with_capacity`) and cast with `bytemuck::cast_slice` instead of per-component `to_le_bytes` (`surface.rs:1048-1092` does one `extend_from_slice`
  per float).

### 2.3 Memory and allocation

- `WebAssembly.Memory.grow()` can detach existing views; a zero-copy view taken before growth becomes invalid ([MDN](https://developer.mozilla.org/en-US/docs/WebAssembly/JavaScript_interface/Memory/grow)). Pre-size memory for the
  steady state and avoid per-frame growth; keep tile payload, texture and descriptor caches byte-capped (the raster path
  already does).

### 2.4 Build flags, in order of real effect

- `lto = true`, `codegen-units = 1`, `opt-level = 3` for speed; `opt-level = "s"` only if size dominates. `wasm-opt -O3` for speed (`-Os`/`-Oz` for size) can add another
  15–20% size reduction and sometimes speed ([code size guide](https://rustwasm.github.io/docs/book/reference/code-size.html)).
- `panic = "abort"`: panics already abort on `wasm32-unknown-unknown` because unwinding is unavailable; setting it is fine but is not a speed win. Keep
  panic paths off hot code ([debugging](https://github.com/rustwasm/book/blob/master/src/reference/debugging.md)).
- `-C target-feature=+simd128`: fixed-width SIMD intrinsics are stable since Rust 1.54 ([stabilization](https://github.com/rust-lang/rust/pull/86204), [core::arch::wasm32](https://doc.rust-lang.org/stable/core/arch/wasm32/index.html)). Expect 2–3× on compute-bound contiguous
  `f32`, little on memory-bound or pointer-chasing code. SIMD is validated at module load, so ship baseline and SIMD
  artifacts and select with `wasm-feature-detect`.
- wasm threads need `SharedArrayBuffer`, cross-origin isolation (`COOP: same-origin`, `COEP: require-corp`) and shared memory; `wasm-bindgen-rayon` currently needs nightly
  ([web.dev](https://web.dev/articles/webassembly-threads), [wasm-bindgen-rayon](https://crates.io/crates/wasm-bindgen-rayon)). A separate worker wasm instance has a much better cost/benefit for decode/styling.

## 3. WebGPU practice for a map

### 3.1 Buffers and draws

- **Default to `writeBuffer`.** It is "often fastest", low-complexity, and preferred for WASM (a mapped buffer would add a copy
  out of the wasm heap). Use `mappedAtCreation` for static data and a 2–3-buffer staging ring only when buffer writes are proven hot
  ([Toji, buffer uploads](https://toji.dev/webgpu-best-practices/buffer-uploads.html)).
- **Never create buffers per frame.** Allocate at a high-water mark, grow by re-creating only when capacity is exceeded,
  and `write_buffer` the used range. The current `create_filled_buffer` / `create_buffer_init` allocates a `GPUBuffer`, a write and a JS object each frame.
- **Per-call cost is ~5 ns native but ~200–500 ns desktop WebGPU and ~500–1500 ns mobile** through wasm→JS→Mojo; a
  20k-call renderer is IPC-bound. Shadow batching took 20,000 draws to 1 and 76 ms to 57 ms; color batching took 32,190
  draws to 14 and gained 34.6% FPS ([Godot WebGPU measurements](https://github.com/dwalter/godotwebgpu/blob/webgpu-4.6.2/webgpu_site/PERFORMANCE_AND_OPTIMIZATION.md)). Our 9–30 tile draws plus two overlay draws mean batching saves tens of
  microseconds, not milliseconds: do it because it is cheap and makes the draw path camera-independent, not because it
  explains the current slowness.
- Cache bind groups with the texture/view they reference (already done per tile); share `BindGroupLayout`s; create pipelines once
  (already done).
- Instanced tile quads: one unit quad and per-instance `{rect, uv_rect, layer}`. With a `texture_2d_array` that is one bind group and one `draw_indexed(..., 0..tile_count)`. Prefer
  arrays to an atlas: no UV bleeding and simpler mips; the default `maxTextureArrayLayers` is 256, ample for 9–30 tiles.
- Render bundles shipped with WebGPU in Chrome 113 ([announcement](https://developer.chrome.com/blog/webgpu-release), [MDN](https://developer.mozilla.org/en-US/docs/Web/API/GPURenderBundleEncoder)); with two draws per frame they are not yet worth it.
- Avoid per-frame pipeline/state churn; pass frame state in one small uniform buffer.

### 3.2 Textures

- Upload only on payload revision change (already true). For raster imagery, `fetch → blob → createImageBitmap` decodes off the main thread and `copyExternalImageToTexture`
  moves it to the GPU; prefer `ImageBitmap` to `HTMLImageElement`, which can force a synchronous decode ([createImageBitmap](https://developer.chrome.com/blog/createimagebitmap-in-chrome-50), [Toji, img textures](https://toji.dev/webgpu-best-practices/img-textures.html), [MDN](https://developer.mozilla.org/en-US/docs/Web/API/GPUQueue/copyExternalImageToTexture)).
- Generate mipmaps for minified raster tiles (render-pass downsample); the current sampler has no mip chain and aliases
  when zoomed out. A layered upload also collapses several arrivals into one `write_texture`.

### 3.3 Reading GPU timings in Chrome today

| Capability | Status | Use |
|---|---|---|
| `timestamp-query` | Chrome 121+; quantized to 100 µs (disable via `chrome://flags/#enable-webgpu-developer-features`) | Per-pass time via `createQuerySet`, `timestampWrites`, resolve + map ([Chrome 121](https://developer.chrome.com/blog/new-in-webgpu-121)) |
| `GPUAdapter.info` | Chrome 127+; `requestAdapterInfo()` removed in 131 | Log vendor/architecture/device per run ([Chrome 127](https://developer.chrome.com/blog/new-in-webgpu-127), [removal](https://groups.google.com/a/chromium.org/g/blink-dev/c/HxOgGf4NzQ4)) |
| WebGPU Inspector | Available | CPU-submit vs GPU-time plots, captures with auto-injected timestamps ([repo](https://github.com/brendan-duncan/webgpu_inspector)) |
| `chrome://gpu` | Always | Confirm hardware acceleration/backend/SwiftShader fallback |
| DevTools Performance GPU track | Available | Correlate GPU work with main-thread frames |
| `chrome://tracing` | Legacy | Prefer DevTools/Perfetto |

Measurement trap: MapLibre's benchmark under-reported by 6× on hardware (0.413 → 2.57 ms) and ~390× on SwiftShader
(0.440 → 171 ms) until a `readPixels` forced GPU synchronization ([PR #7285](https://github.com/maplibre/maplibre-gl-js/pull/7285)). In WebGPU, map a timestamp buffer or copy one pixel
before declaring a frame done.

## 4. Profiling recipe for Rust→wasm in a browser

- **Build:** `wasm-pack build --profiling` gives an optimized release build with debug info ([wasm-pack build](https://wasm-bindgen.github.io/wasm-pack/book/commands/build.html)). Its default `wasm-opt` is `-O` and DWARF is
  off; add `--debuginfo` and configure `[package.metadata.wasm-pack.profile.profiling]` ([config](https://wasm-bindgen.github.io/wasm-pack/book/cargo-toml-configuration.html)). Keep the **name section** (`wasm-bindgen --keep-debug` or `wasm-opt --debuginfo`) or Chrome shows `wasm-function[123]` ([profiling wasm](https://www.webassembly-wasm.com/webassembly-core-concepts-browser-runtime/debugging-and-profiling-wasm-modules/profiling-wasm-with-the-chrome-performance-panel/)).
  With DevTools open, Chrome tiers wasm down to unoptimized code, so in-page `console.time`/`performance.now` are unreliable for absolute
  comparisons; use the Performance panel and keep A/B runs in the same configuration ([DevTools wasm](https://developer.chrome.com/docs/devtools/wasm)).
- **One timeline:** wasm frames appear as `wasm://wasm/<hash>` with names from the name section; use Bottom-Up self time and watch
  `v8.wasm.compile*`/`CompileLazy` and GC ([guide](https://www.webassembly-wasm.com/webassembly-core-concepts-browser-runtime/debugging-and-profiling-wasm-modules/profiling-wasm-with-the-chrome-performance-panel/), [reference](https://developer.chrome.com/docs/devtools/performance/reference)). Rust `tracing` spans become User Timing marks via [tracing-web](https://crates.io/crates/tracing-web) ([source](https://docs.rs/tracing-web/latest/src/tracing_web/performance_layer.rs.html)); [tracing-wasm](https://docs.rs/tracing-wasm/latest/tracing_wasm/) is the older
  alternative. `performance.mark/measure` through `web-sys` is fine for coarse spans. Firefox Profiler demangles Rust wasm symbols and can
  symbolicate stripped bundles with `--symbolicate-wasm` ([PR #6008](https://github.com/firefox-devtools/profiler/pull/6008), [demangler](https://github.com/mstange/gecko-profiler-demangle)).
- **Before/after recipe:** (1) freeze a scenario — vector base, 9 tiles, 1920×1080, DPR 1, scripted 10 s
  constant-velocity pan then 2 s idle, warm caches, fresh browser, record `GPUAdapter.info` and hardware acceleration. (2)
  Instrument four clocks: `performance.measure` around `renderFrame`; wasm spans for `frame_quads`, `overlay_triangle_vertices`, texture upload and submit; a timestamp query
  around the render pass; `requestAnimationFrame` deltas. (3) Capture a DevTools Performance trace and a WebGPU Inspector frame. (4)
  Attribute: high wasm self time → geometry; high JS glue → crossings and marshalling; GPU > 8 ms → textures/fill rate;
  long `renderFrame` with idle CPU/GPU → an `await` on the frame path. (5) Report p50/p95/p99 of rAF interval, wasm time, GPU pass
  time and the frame counters (`overlayTriangles`, `drawnFragments`, `missingTiles`); keep the harness committed. Do not trust FPS alone — vsync
  saturates it at 60.

## 5. Ranked changes for this renderer

Estimates are engineering judgment; measure each with the recipe above. "Payoff" assumes a vector base and a moving
camera unless stated.

| Rank | Change | Expected payoff | Risk | Measurement |
|---|---|---|---|---|
| 1 | (e) World-space overlay geometry tessellated once per revision; GPU projection; width in the vertex shader | Removes the dominant per-frame CPU cost while panning (likely tens of ms across 9 vector tiles); also removes world-copy clipping and per-frame `Vec` growth | Medium: joins, width, holes, antimeridian parity | wasm self time of `overlay_triangle_vertices` → ~0 on camera-only frames |
| 2 | (b)+(d) Persistent vertex/index buffers + one instanced tile draw + texture array | Removes per-frame buffer allocation and 9–30 bind-group/draw calls; less GC and driver overhead; helps mobile most | Low | `renderFrame` wasm/JS time; command count; p99 |
| 3 | Extra (j): decode once, off the frame path; parallel prefetch; no `await` in `renderFrame` | Removes the double PNG decode, the serial 32-fetch loop and multi-task frame stalls; large cold-start and pan-into-new-area win | Low | `renderFrame` duration distribution; `missingTiles`; tasks per frame |
| 4 | (g) Typed-array seam for vector data, or keep geometry in Rust entirely | Eliminates 10⁴–10⁵ JS objects and the Reflect parse per vector reload | Medium-low | reload wall time, JS heap, wasm↔JS call count |
| 5 | (c) Worker wasm instance for MVT decode/style/tessellation | Moves remaining reload work off the main thread | High: second instance, transferables, provider plumbing | long tasks during reload; time-to-first-paint of a new tile |
| 6 | (h) Raster decode via `createImageBitmap` + `copyExternalImageToTexture` | Removes PNG decode from the main thread and avoids RGBA copies into wasm | Low-medium: wgpu external-texture path, color space, premultiply | main-thread decode time; upload stall |
| 7 | (a) Cache clip-space tessellation keyed by revision + camera pose | Only helps a static camera; a no-op during pan; stopgap for settled frames | Low | hit rate; idle frame time |
| 8 | (i) Avoid per-frame texture upload | Already handled by the revision cache; verify, do not rewrite | Low | uploads per frame (expect 0 steady state) |
| 9 | (f) SIMD / wasm threads | Only after CPU geometry remains; SIMD 2–3× on suitable kernels; threads need COOP/COEP and nightly | High for threads | wasm self time per kernel; thread startup |
| 10 | Extra: reuse `Vec`s, `bytemuck` serialization, drop per-frame `Renderer::new` | Small but trivial; less allocator/GC noise | Low | allocation profile; encode time |

- (a) is **subsumed** by (e). A camera-keyed clip-space cache looks good on an idle screenshot and does nothing while
  dragging.
- (b) alone is modest (hundreds of microseconds) because the draw list is small; it is ranked second because it is
  nearly free and is a prerequisite for a clean instanced tile path.
- (c) is the right long-term shape (MapLibre's), but the highest effort. Shrink the main-thread work with (e), (g) and
  the decode/fetch fix first; then move what remains.
- (f) is last because the hot loops are triangulation, hashing and per-feature dispatch — poor SIMD candidates — and
  threads add deployment constraints.

## 6. What "on par with other web renderers" plausibly means numerically

- **60 Hz means 16.7 ms; 120 Hz means 8.3 ms.** Mainstream web maps target 60 Hz; a 1080p map that holds 60 Hz during
  pan is "on par".
- **Mapbox GL JS's own harness** reports frame duration in the low single-digit milliseconds on desktop for moderate
  styles (historical PR tables show ~4–6 ms and 60 fps with 0% frames > 16 ms; one representative table reports 6.0 and 6.1 ms, e.g. [PR #5190](https://github.com/mapbox/mapbox-gl-js/pull/5190), [bench suite](https://github.com/mapbox/mapbox-gl-js/tree/main/bench)); treat these synthetic
  scenes as a floor for CPU+GPU work, not a promise.
- **Real-world MapLibre with a heavy 3D overlay:** 50 fps / p95 15.6 ms on an M5 Pro, 40 fps / p95 26.8 ms on an M1
  mini, 38 fps / p95 28 ms on an iPhone 16 Pro, while four other engines held 60 fps with p95 18–21 ms. The benchmark
  notes vsync saturates FPS and post-stop drawing took up to 3.3 s ([ortho-earth](https://www.ortho-earth.com/docs/story-benchmark)).
- **A healthy simple MapLibre scene** measured 2.57 ms once GPU synchronization was forced ([PR #7285](https://github.com/maplibre/maplibre-gl-js/pull/7285)). A `setFeatureState` regression
  took frame time from 16.7 ms (60 fps) to 800–1200 ms, showing how far a main-thread geometry update can fall ([issue #13624](https://github.com/mapbox/mapbox-gl-js/issues/13624)).
- **Peer-reviewed 1080p comparison:** Balla & Gede (2025) measured Leaflet, Mapbox GL JS 3.7, MapLibre GL JS 4.7.1 and
  OpenLayers 10.2 at 1920×1080 on an RTX 3070 with GeoJSON feature loads; Mapbox was fastest at ≥50k points, MapLibre
  slowest for lines/polygons, with a threshold slowdown past 50k features ([paper](https://www.mdpi.com/2220-9964/14/9/336), [DOI](https://doi.org/10.3390/ijgi14090336), [data](https://github.com/balladaniel/webmaplibs_performance)). It measures GeoJSON
  layers, not vector-tile base maps: use the curve shape, not a tile-frame budget.
- **Our target:** 1080p, vector base, 9 visible tiles, hold 60 Hz (p95 ≤ 16.7 ms, p99 ≤ 33 ms) during a scripted pan,
  with wasm ≤ 6 ms, GPU ≤ 8 ms, zero frames > 50 ms, and no fetch or decode inside a presenting frame. First imagery for
  a newly visible tile should arrive within ~100–200 ms of the data being available, consistent with MapLibre's 0.3–0.4
  s network-dominated first-tile observation in the ortho-earth benchmark. Reaching GL JS's 4–6 ms synthetic frame
  duration is not required for parity; a stable 60 Hz with headroom is.
