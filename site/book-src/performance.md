# Performance and profiling

This page is integration guidance: the practices that keep map rendering,
routing, and navigation responsive, and the readbacks the SDK exposes so
you can measure your own integration on your own hardware and scenes.

## Measured on this machine

The browser frame has a committed, reproducible measurement. On an Apple
Silicon host (`GPUAdapter.info: apple · metal-3`) with hardware WebGPU, a
1920 × 1080 vector base over the recorded demo tiles and a four-second pan,
`renderFrame` costs **1.4 ms at the median and 1.5 ms at p95**; every one of
120 measured frames stayed inside the 16.7 ms 60 Hz budget. The p99 sample
(8.2 ms) is the frame that re-tessellates the overlay layer, not the steady
state. The render pass's own GPU time is **0.8 ms at the median and 0.89 ms
at p99**. The raster base over the same ground costs 0.2 ms at the median,
with a 0.15 ms median GPU pass.

```bash
# from platform/web
node perf/run-frame-budget.mjs            # vector base
node perf/run-frame-budget.mjs --layer raster
```

The runner launches the host's own Chromium with hardware WebGPU and prints
the machine, adapter, scenario and per-stage distributions. Treat the table
above as a starting point for your own measurement, not a guarantee: quote
figures from your own device, power state and route mix.

## Construct the SDK once

- Build one `MapSdk` object and register your providers once at startup.
  Re-registration replaces every slot (including a cached tile slot), so
  an accidental double registration silently reopens the cache and resets
  its tuning: register, then tune with `setRasterTileCacheTimeToLive`.
- Keep a single `Navigator` around the lifetime of a trip
  screen; subscriptions are explicit and cheap, but re-subscribing
  listeners is wasted work on every frame.

## Frame the camera with the contract's helpers

- Prefer `fit_bounds` for route previews and search-result framing. It computes
  a top-down, north-up camera that keeps the whole box visible inside the
  insets you give it, instead of a hand-tuned zoom that breaks on another screen
  size.
- Give it real insets. Route cards, attribution, and sheets are overlays your
  application owns; the insets tell the camera what the map may not hide.
- Read the camera back with the attribute getters (`center`, `zoomLevel`,
  `bearingDegrees`, `pitchDegrees`) when you need to persist or compare a pose;
  a new pose is a new constructor call.

## Let the render loop stay explicit

- Frames are explicit by design: the platform view drives `draw` from its own
  cadence. Do not add a second render loop, and never call `draw` again while a
  previous call is in flight.
- Poll `isFrameSettled` in the render pump to skip `draw` when a re-draw would
  present the same pixels. This keeps battery cost near zero on a static map.
- Read `FrameStatistics` from every draw. The four readiness counts (`present`,
  `fallback`, `deferred`, `unavailable`) add up to the requested count, so a
  persistent portion of `deferred` tells you the per-frame fetch budget is the
  limiter, and a portion of `unavailable` points at provider refusals rather than
  slow ones.
- Feed gesture deltas through the SDK's camera seam instead of moving the
  viewport yourself; the SDK clamps zoom and pitch and holds the focal point.

## Size the tile cache deliberately

Register the tile cache with an absolute byte budget that fits your product;
the budget is a registration-time fact, so retuning it means re-registering.
Set the tile time-to-live live through `setRasterTileCacheTimeToLive`; a request
above the provider's ceiling is accepted and clamped, and
`rasterTileCacheTerms` displays the effective value, the ceiling, and the
default. Do not set a zero time-to-live; the call is refused because it would
expire every entry on capture.

## Keep the provider path under your quota

- The provider implementation carries a shared token-bucket limiter and bounded
  idempotent retries. Treat `RateLimitExhausted` as "retry after the next
  refill", not as an error to surface in the UI.
- Cache-then-network is the default shape of every provider call, so repeated
  identical requests inside the time-to-live cost no quota.
- Batch what the contracts batch: the distance matrix answers every
  origin-destination pair in one call.

## Use offline where it pays

The offline routing provider answers routes over a memory-mapped archive with
A\* and no network request. Compile an extract sized to the region your users
drive in, not the planet. The offline decorator retries over the offline graph
when the online provider fails, so a coverage-limited archive is graceful
degradation; you wire it around your online provider, and its maneuver texts
stay honest where the archive carries no street names.

## Profile with the SDK's own readbacks

- Subscribe to diagnostics events at `Info` severity (or `Debug` for
  frame-path detail). Events stream fire-and-forget from bounded ring
  buffers, coordinates are scrubbed at the sink, and a slow listener can
  never slow the SDK.
- Drain the backlog with the bounded flush readback from a non-frame
  context: the flush is synchronous and must stay out of the render path.
- For provider failures, read `latestProviderFailureDiagnostics` after a
  suspicious session; the ring carries the contract, method, timestamp,
  and verbatim failure text for every failed foreign-callback provider
  call.
- Measure on your target device and your real scenes. The SDK reports
  what it did (tile counts, frame statistics, cache terms, the frame's
  own stage spans in the browser, and the GPU render pass time), so a slow
  frame can be attributed to geometry, encoding, submission or the GPU
  instead of guessed at.
- In the browser, the GPU pass time arrives one frame after the pass as
  `gpuPassMilliseconds`, read back without blocking the present. WebGPU
  implementations quantize timestamps, so treat the value as a coarse
  distribution, and read it next to the frame's total. An adapter without
  timestamp-query support omits the field rather than reporting a
  placeholder; the SDK then reports the CPU-side stages alone.
- In the browser, the frame never blocks on the network: missing tiles are
  fetched on their own tasks and the frame draws what it already holds. A
  page that renders on demand should listen for the canvas's
  `map-tile-arrived` event and draw again, which is what the live demo page
  does.
