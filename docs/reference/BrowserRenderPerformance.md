# Browser render performance: the harness and the recipe

Date: 2026-11-01 (M23). Scope: the browser path (`rustic-maps-web-bindings` → wgpu over
WebGPU). The research that ranked the work is
`docs/history/research/browser-render-performance.md`; this document is the
measurement it asks for, and the numbers the performance chapter quotes.

## What the harness measures

`platform/web/perf/run-frame-budget.mjs` drives one frozen scenario through the
page's own frame path and reports percentiles, not a single screenshot.

| Metric | What it is | Why it is the one to watch |
|---|---|---|
| `renderFrame` | the whole wasm frame call, as the page awaits it | the work the integration adds to the main thread |
| `animation frame` | the interval between animation frames during the pan | the cadence a visitor sees; it stays pinned at the display's refresh even when frames are being dropped, so it is a floor, never a promise |
| `stageMilliseconds.tilePipeline` | camera listing, tile fetch start, scheduler warm pass, quad assembly, texture resolution | the raster path's cost |
| `stageMilliseconds.overlayGeometry` | overlay and location-indicator tessellation through the camera pose | the vector base's cost; the overlay path is what a vector map spends its frame on |
| `stageMilliseconds.encode` | vertex/index encoding into GPU buffers | vertex traffic |
| `stageMilliseconds.submit` | surface acquisition, render pass, queue submission | driver and IPC cost |
| `gpuPassMilliseconds` | the render pass's own GPU time from timestamp queries, read one frame after it was written | the part of a frame the CPU cannot attribute; absent when the adapter does not support timestamp queries |

The stage spans are `performance.now()` reads inside `renderFrame` itself,
reported on the frame's statistics object, so they need no profiler attached
(and Chrome tiers wasm down when DevTools is open, which makes in-page timing
the honest instrument for absolute comparisons).

The ring never cancels or remaps a buffer whose map is still pending: a
pending slot is left alone, only completed maps are read, and a frame with no
free slot carries no sample. That is not an optimisation but a portability
rule — Firefox's stricter WebGPU validation rejects a cancelled-then-remapped
buffer ("still mapped", "already mapped", "map is pending"), which floods the
console and can stop the frame loop presenting. The browser suite's
`frame_loop_cleanliness.spec.mjs` drives a trip and a pan and asserts that no
such error is emitted.

The frame statistics also carry `gpuBufferAllocations`, the canvas' cumulative
count of vertex and index buffer allocations. A steady frame reuses the
buffers it already owns, so the count must stop growing once every layer has
been seen at its largest payload; the browser suite asserts exactly that on a
settled golden scene, which turns the zero-allocation rule from a code-review
claim into a regression test.

### How the GPU pass time is measured

The canvas requests WebGPU's timestamp-query feature only when the adapter
advertises it and is neither the noop backend nor a CPU fallback adapter;
otherwise the frame simply omits `gpuPassMilliseconds` and everything else
behaves as before. When the feature is present, each frame's render pass
writes a begin and an end timestamp. After the pass, the query set resolves into a small buffer and the
result copies into one of two `MAP_READ` readback buffers; the frame then maps
that buffer and presents without waiting. At the start of the next frame the
previous map is read, unmapped and reported as `gpuPassMilliseconds`, while
the map that is still pending is dropped rather than allowed to delay the
frame. The two slots alternate so a frame always reads the other slot's
completed result.

The value therefore describes the **previous** frame's pass, which is the
price of never blocking the present. WebGPU implementations quantize
timestamps; Chromium's step is commonly 100 µs and is platform-dependent, so
the distribution is a quantized sample of a sub-millisecond pass, not a
cycle-exact figure. Read the row next to `renderFrame`, never instead of it.

## The frozen scenario

- A **vector base** (the recorded fixtures) over a 1920 × 1080 canvas at device
  scale 1, or the **raster base** for the tile path — `--layer` selects.
- A settle, then a warm-up pass (shader compilation, first uploads and the
  initial vector reload are one-time costs), then the measured pass.
- A constant **600 device px/s pan for 4 s**, out and back, so the camera
  returns to where it started and the scenario never leaves the recorded tile
  fixtures — an unmapped edge would report a smaller draw list and flatter
  numbers.
- 60 Hz budget: p95 ≤ 16.7 ms, p99 ≤ 33 ms, no frame > 50 ms.

## Running it

```sh
cd platform/web
npm run perf                                        # the vector base
node perf/run-frame-budget.mjs                      # the same, with flags
node perf/run-frame-budget.mjs --layer raster       # the tile path
node perf/run-frame-budget.mjs --json report.json   # keep the report
node perf/run-frame-budget.mjs --headed             # watch the pan
```

The runner starts the harness's static server itself (or reuses one already on
the port) and launches the host's Chromium with hardware WebGPU. It is
deliberately **not** part of `npm test`: the Playwright suite runs under
SwiftShader so CI is deterministic, and SwiftShader's absolute frame times say
nothing about a GPU (the research's §3.3 measurement trap: MapLibre's benchmark
under-reported by ~390× there). A number without the machine is not a
measurement, so the report carries the browser build and the `GPUAdapter.info`
descriptor.

## Measured baseline

Apple Silicon host (`GPUAdapter.info`: `apple · metal-3`), headless Chromium
(system channel) with hardware WebGPU,
1920 × 1080 at device scale 1, vector base over the recorded fixtures — the
scenario above, 120 measured frames:

Measurement of 2026-11-01 (the canvas-owned vector base, the world-space
overlay cache warm, every upload buffer reused from its high-water mark):

| Metric | p50 | p95 | p99 | max |
|---|---|---|---|---|
| `renderFrame` | 1.4 ms | 1.5 ms | 8.2 ms | 9.8 ms |
| `animation frame` | 16.7 ms | 16.8 ms | 16.8 ms | 16.8 ms |
| `overlayGeometry` | 0.0 ms | 0.1 ms | 7.2 ms | 8.5 ms |
| `encode` | 0.0 ms | 0.0 ms | 0.0 ms | 0.0 ms |
| `submit` | 0.0 ms | 0.1 ms | 0.1 ms | 0.2 ms |
| `gpuPassMilliseconds` | 0.8 ms | 0.82 ms | 0.89 ms | 0.89 ms |

Every measured frame stayed inside the 60 Hz budget (0 of 120 over 16.7 ms).
The p99 tail is the overlay-revision pass: the world-space tessellation runs
on the frame that sees a new overlay list, which is what the pan does at its
start, while the vertex upload now writes into the existing high-water-mark
allocation. The next performance change should move the tessellation off the
presenting frame rather than shave the steady state.

Run-to-run spread on this host is about ±0.2 ms at the median; the p95 and p99
samples move more because a single tail frame sets them. Compare runs on the
same machine and in the same power state, never across machines.

The raster base over the same ground is not the bottleneck: `renderFrame`
p50 is 0.2 ms with 15 tiles drawn, because its payloads are pooled, its
textures cached and its vertex and index buffers reused — the frame only
projects quads and writes them into buffers it already owns. Its GPU pass is
correspondingly smaller: `gpuPassMilliseconds` p50 0.15 ms, p95 0.21 ms, p99
0.21 ms over the same 120 frames.

Two conclusions drive the remaining M23 work:

1. **The steady vector frame is inside the budget by a wide margin.** With
   the world-space overlay cache warm, projection, encoding and submission
   are each under a millisecond at the median, and every upload reuses its
   buffer.
2. **The tail is the overlay-revision pass.** The p99 sample is the frame
   that re-tessellates the overlay layer; moving that tessellation off the
   presenting frame is the remaining work, not general micro-optimization.
   The raster path is not a bottleneck (p50 0.2 ms with 15 tiles drawn).

## The cold fit: z14 → z7

A fit is a different workload from a pan: one camera change reveals a whole
screen of tiles at once, so a cold fit pays every fetch, decode and first
style pass before the map is complete. `perf/run-fit-budget.mjs` measures that
workload on a bare surface with no page pump, over committed z7 and z8
performance fixtures installed with a same-origin template. It renders one
frame at z14, changes the camera to z7, then renders at the page's default
cadence until every visible tile has drawn. It reports the time to the first
drawn base tile, the time to the first full frame, per-frame `renderFrame` and
stage times, the requested/drawn tile counts and the overlay triangle count.

```sh
cd platform/web
node perf/run-fit-budget.mjs                  # readable report
node perf/run-fit-budget.mjs --json fit.json  # keep the report
node perf/run-fit-budget.mjs --headed         # watch the fit
```

The runner fails when the first full frame exceeds 1.5 s, the frame draws
more than 400 000 overlay triangles at z7, the z8 unchanged-settle p95 exceeds
6 ms, or those settle frames extract a label candidate. Like the pan harness it
launches the host's Chromium with hardware WebGPU and stays outside
`npm test`; `tests/vector_fit_budget.spec.mjs` gates the hardware-independent
part (the visible set completes and the z7 geometry stays inside the triangle
budget) and enforces the time budget only with `MAP_FIT_BUDGET_STRICT=1`.

The vector path keeps a fit cheap in four ways:

- **Visible-first delivery.** The visible prefetch queues every visible miss
  but starts them in a burst (`with_visible_prefetch_burst_tile_count`,
  defaulting to the provider's declared capacity) and writes each tile into
  the cache as it lands. `prefetch_missing_tiles_with_delivery` reports each
  tile immediately, so a presenting surface repaints on the first arrival
  instead of waiting for the slowest request. The ring ahead waits until the
  visible set is no longer fetching, is not planned below z8 (where a screen
  spans most of the world), and drops a delivery the latest camera no longer
  touches.
- **Per-tile styled geometry.** One tile's styled overlays are cached by tile
  address, style revision and style-zoom bucket; a tile arrival styles only
  itself. `VectorBaseFrameDiagnostics::tessellated_tile_count` reports the
  work: zero on a warm refit, one when one new tile arrives.
- **Low-zoom level of detail.** At or below z8 every polygon ring and styled
  line is simplified to half a pixel of screen space and features thinner
  than one pixel are culled before the geometry enters the cache. At city zoom
  the style's exact geometry is untouched, so the committed z9 and z14 goldens
  do not change. Over the committed performance tile in the Carto light style
  at z7, one tile drops from 34 788 to 8 643 tessellated triangles, so the
  fifteen-tile frame draws 129 645 instead of 521 820 — the low-zoom triangle
  budget test holds that line.
- **Shared frames.** The scheduler answers an `Arc<VectorBaseFrame>` and an
  unchanged frame is the previous allocation: `content_revision` names its
  tiles, style revision, zoom bucket and sprite atlas, so a surface compares
  one integer instead of deep-comparing the overlay list. The overlay list
  itself is shared behind an `Arc`, so even a frame clone copies no overlays.
  Over a 46 000-overlay z7 frame on an Apple Silicon host, the recorded bench
  measures a 24 ns shared-frame clone, a 1.02 ms owned-list clone (the cost the
  shared list replaced), a 0.29 ms deep overlay compare and a 4 ns revision
  compare. The browser canvas still calls the owned-frame entry point and
  compares overlays; switching it to the shared frame and comparing revisions
  is what removes its remaining per-frame compare.

### Measured fit

Apple Silicon host (`GPUAdapter.info`: `apple · metal-3`), headless Chromium
with hardware WebGPU, 1920 × 1080 at device scale 1, the committed z7/z8
performance fixtures, one render per animation frame:

| Metric | Value |
|---|---|
| first drawn base tile | 80 ms |
| first full frame (15 tiles) | 1 059 ms |
| overlay triangles, full frame | 371 745 |
| `overlayGeometry` p95 / max | 41 ms / 51 ms |
| settled `renderFrame` p50 / p95 | 2.1 ms / 5.0 ms |

The triangle count is measured after the world-copy split, so it is roughly
three times the camera-side geometry count the budget test reports. The full
frame lands inside both budgets; the settled p95 is above the 4 ms target until
the browser canvas compares `content_revision` and stops re-placing unchanged
labels every frame.

### z8 settle and the label candidate cache

After the fit, the same harness changes the camera once more to z8, drains the
tile-level cross-fade, and records 30 unchanged settle frames. Each tile
caches its label candidates against the tile address, style revision and zoom
bucket, so those frames read them instead of walking the symbol layers again.

| Metric | Value |
|---|---|
| z8 settle `renderFrame` p50 / p95 | 2.1 ms / 5.8 ms (budget 6 ms) |
| z8 overlay triangles | 371 965 |
| label candidate extractions / cache hits | 0 / 15 |

The p95 is the budget line; one sample in the measured run was a 54.8 ms tail,
reported rather than hidden because the distribution, not the single sample,
is the claim. The fit table above is an earlier run of the same harness; the
first drawn base tile varies between 80 and 88 ms and the first full frame
between 1.06 and 1.16 s across runs on this host. The cross-fade itself is
asserted by `tests/vector_zoom_cross_fade.spec.mjs`: a zoom-out keeps the
retired level drawn at the settled frame's triangle count and never clears to
the background, and the level that is already cached draws on the first frame
when the camera returns. The canvas frame statistics report
`vectorZoomTransitionRunning`, `vectorRetainedTiles`,
`vectorBaseLabelCandidateExtractionCount` and
`vectorBaseLabelCandidateCacheHitCount` beside the raster cross-fade fields.

## Motion grading

The vector base can grade its per-frame work by camera motion: while a pan,
zoom or rotate gesture is active, the frame paints the decoded tiles and the
last settled style-zoom bucket's geometry and skips label assembly; the first
still frame resolves the style bucket and the labels at the final pose. The
browser reports a gesture through the canvas' internal
`setCameraGestureActive` seam, and the frame-budget scenario drives one by
construction (it is a scripted pan), so the harness reports how many of its
samples ran graded.

Same host and scenario as the baseline above (1920 × 1080, vector base over
the recorded fixtures, 600 px/s pan for 4 s, 120 frames), measured 2026-11-04,
three graded runs and two control runs:

| `renderFrame` | p50 | p95 | p99 | max |
|---|---|---|---|---|
| graded (118 of 120 frames; range over 3 runs) | 1.4–1.8 ms | 1.6–2.1 ms | 8.1–10.4 ms | 8.5–10.6 ms |
| ungraded (control; range over 2 runs) | 8.0–8.1 ms | 8.3–8.4 ms | 12.6–13.0 ms | 14.9–15.3 ms |

The graded median is roughly 4.5–5.7× lower. The graded p99 and max are the
two ungraded frames the window still contains — the measured pass ends with
the settle frame — not a graded frame.

To reproduce, run `npm run perf` on a tree with the grading seam and once on a
tree without it; the report's `motion:` line states how many samples ran
graded, so a run that silently measured the ungraded path is visible rather
than inferred. Compare runs on the same host and power state.

## Measuring a guided trip

The frame budget above is a pan. A simulated trip adds two moving parts the
pan does not have: the guidance card and the location puck.

- **Cadence.** The demo dispatches fixes every 250 ms at 1× (the artifact's
  5 s sample interval over the 20× playback factor, floored at 1 ms by
  `trace_dispatch_interval_milliseconds`). The puck's motion per fix is the
  fix interval clamped to 120–1000 ms (`LOCATION_ANIMATION_MINIMUM_MILLISECONDS`,
  `LOCATION_ANIMATION_MAXIMUM_MILLISECONDS`), so the drawn pose advances every
  animation frame rather than every fix.
- **What to sample.** `window.mapNavigationSnapshot()` for the guidance
  values, `window.mapLocationPose()` for the drawn puck pose (its
  `isAnimating` flag says whether a motion is in flight), and
  `window.mapLastFrameStats()` for `drawnTiles` / `missingTiles` on the last
  settled frame. The Advanced pane's facts line reports the same three
  numbers.
- **Reading it.** A run is healthy when the banner's distance phrase changes
  across the sample window, the pose's per-frame ground speed stays near the
  trip's own pace (28 m/s at 1×) instead of hopping, and `missingTiles` sits
  at zero on settled frames. A missing tile on a frame that follows a camera
  move is the scheduler still fetching; a missing tile on a settled frame is
  a provider or pacing problem, and the diagnostics event stream names the
  request that failed.
- **Live tiles.** A keyed MapQuest raster base measures the provider path as
  well as the renderer: each new tile is a network round trip, and the
  source's declared budget (eight immediate requests, eight per second by
  default) paces the burst. Measure with the same three hooks against a live
  keyed session; the harness's own fixtures never leave the machine.
- **The follow-trip budget.** `platform/web/perf/run-follow-trip-budget.mjs`
  drives the same user path and reports per second: requested, drawn and
  missing tiles, the frames that reported a miss, and the provider requests
  the diagnostics ring recorded. It runs outside `npm test` because the live
  mode reaches the network; add `--live` for the live raster source and set
  `PROVIDER_API_KEY` for one keyed session. The committed regression is
  `follow_trip_tile_coverage.spec.mjs`, which fails when a sampled frame after
  the grace period reports a missing tile or the guidance stops advancing.
- **What `missingTiles` means.** A requested tile is missing only when the
  scheduler could not resolve it from its own or an ancestor's pooled
  imagery. A tile the frame culls as off-screen but has imagery for is a
  draw-list property, not a gap; `missingTileAddresses` names the tiles that
  actually lagged, bounded to sixteen.
- **Measured.** On the fixture-backed trip at a 640 × 480 viewport: about
  65 frames per second, 16–24 tiles requested per frame at zoom 17, and every
  requested tile covered after the start grace. The same holds at 2× (66 m/s
  of ground speed) and 4×; at 8× the route leaves the recorded z17 tile set
  at its western edge (address `17/21295/*` has no committed fixture), so the
  fixture run reports gaps for the last seconds. With the live raster source
  the same addresses answer with no missing tiles, but the live demo advances
  through its own route path more slowly, so at-speed 8× coverage is verified
  on the published demo rather than in the offline harness. The remaining
  distance fell from 1403 m to 963 m over 12 s at 1×, 1396 m to 746 m at 2×,
  and 1396 m to 309 m at 4×.

## What is not done yet

- **One instanced tile draw over a `texture_2d_array`.** The buffers are now
  persistent and reused, but each tile still binds its own texture and draws
  a quad per tile. Cheap and correct either way: the raster path already
  measures 0.2 ms, so this is a cleanliness change at this scene size.
- **Moving the overlay-revision tessellation off the presenting frame**, the
  remaining tail cost the p99 column attributes.

The `wasm-opt` step has landed: `scripts/build-wasm.sh` runs `wasm-opt -Oz`
after `wasm-bindgen` and the browser package is 91.1 % of the unoptimized
module.
