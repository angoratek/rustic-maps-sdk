# Responsive tiles and smooth zoom

**Status:** implemented, owner-directed 2026-11-03 (PRD §6 item 50). Work: `PLAN.md` M65. The bounded pool, the provider pacing, the zoom cross-fade and the browser proof ship; the Android and iOS device-seam assertion remains in the M65 proof item.
**Question it answers:** why does the live demo fill its viewport one tile at a time, and how do we make zoom changes feel continuous?

## The starting state (before this work)

- **Pacing.** Both URL-template providers create their token bucket from the source's `TileSourcePacing` declaration (`everymap-providers-url-template-raster` and `-vector`). The demo's keyless raster source and its default vector base both use these providers, so a twelve-tile viewport is admitted as four immediate requests and then one every 500 ms. That pacing alone produces the observed cadence.
- **Serial warm pass.** `TileWarmTask::run` and `TileScheduler::prepare_frame` await one provider call at a time. The native surface spawns the warm task detached, so it is off the presenting frame but still one-at-a-time; the browser canvas awaits the warm pass inside the frame.
- **Browser probe.** Before every live tile the browser canvas first requests the same-origin fixture route for that address, so each tile pays a dependent round trip and consumes a connection slot before the real request.
- **Zoom.** The tile zoom is `round(camera zoom)`, so crossing a half level replaces the whole requested tile set in one frame. Old-level tiles survive only as per-child parent fallback on zoom-in (their sub-window is drawn for the new child); on zoom-out nothing retains them and uncovered areas clear to the background. The tile pipeline has a texture and sampler only, no opacity uniform, and its blend mode is off.

## Design

### A bounded parallel pool

One named constant, `TILE_FETCH_CONCURRENCY` (default 8), bounds every tile fetch pass:

- `TileWarmTask::run` keeps at most that many provider futures in flight through a `FuturesUnordered`, refilling from the planned list as each completes. Warming marks, delivery order and the unwind guard keep their semantics: a key leaves the warming set when its outcome is delivered, and a cancelled run unmarks everything it did not complete.
- `prepare_frame` collects its budgeted fetches through `buffer_unordered(TILE_FETCH_CONCURRENCY)` and inserts the results into the texture pool serially, so the pool is never mutated concurrently.
- The browser canvas already starts its network fetches detached (up to 32 in flight); the warm pass it awaits only moves staged payloads from memory into the pool, so it stays on the presenting frame. The measured browser gain comes from pacing and from the pool, not from that move.
- The per-tile fixture probe stays: it is the network-free test seam, it runs once per address rather than per frame, and removing it would make every fixture-backed case depend on a live provider. Its one extra round trip per new address is bounded by the staging cache.

### Provider pacing

The bucket defaults rise to a capacity of 8 and a refill of 8 per second, matching the pool: a normal viewport's tiles are admitted as a burst and then paced only under sustained demand. A provider that publishes a stricter budget overrides it through its source declaration; MapQuest's API bucket is measured separately from its tile usage and split only if the measurement shows contention.

### Zoom transition

A `ZoomTransition` value owned by the surface session (native) and the draw state (browser) records the tile-z change and drives the frame:

1. **Hold.** After a tile-z change, the frame keeps resolving the previous level's visible keys from the pool only — never fetching them — and draws them over the new level. They are projected with the current camera, so the existing per-tile world spans scale them automatically; no shader uniform is needed.
2. **Fade.** When at least 85% of the new level's visible keys are present (or the hold deadline expires), the previous level's per-quad opacity ramps from 1 to 0 over 250 ms. The readiness fraction keeps a couple of slow stragglers from delaying the new detail. Drawing the old level over the new one with standard alpha blending is an exact dissolve.
3. **Settled.** A running transition keeps the frame unsettled, so the native pumps and the browser pump keep drawing; canvas frame statistics expose the transition state and the present/pending counts.

The tile vertex format gains an opacity attribute, the three pipeline copies switch to alpha blending, and the retained level counts against the frame's quad budget.

## Limits and defaults

| Constant | Value | Meaning |
|---|---|---|
| `TILE_FETCH_CONCURRENCY` | 8 | Provider futures one pass keeps in flight |
| `DEFAULT_TOKEN_BUCKET_CAPACITY_TOKENS` | 8 | Immediate tile requests a URL-template provider admits |
| `DEFAULT_REFILL_TOKENS_PER_SECOND` | 8 | Sustained tile request rate a URL-template provider admits |
| `ZOOM_TRANSITION_FADE_MILLISECONDS` | 250 | Cross-fade duration |
| `ZOOM_TRANSITION_MAXIMUM_HOLD_MILLISECONDS` | 1000 | Longest the previous level waits for a late tile |

## Verification

- A recording provider asserts the pool's observed maximum concurrency is exactly the constant and the serial floor is gone; `frame_latency` changes from asserting one-at-a-time to asserting the pooled ceiling.
- A browser case crosses a tile level by wheel and by double-tap and asserts: the previous level is drawn during the hold, the new level draws when present, the fade ends, and zoom-out never clears to the background. The raster and styled-base cases ship.
- Before/after numbers (requests in flight, time to a filled viewport, frame p95 while panning) are recorded in the round entry. The measured browser numbers ship in the fit harness.
- The Android and iOS device-seam assertion of the same transition remains open in the M65 proof item.
