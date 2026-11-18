# Recorded Navigation Traces

GPS-trace fixtures for the end-to-end navigation replay tests in
`crates/rustic-maps-navigation` (see `src/trace_fixture.rs` for the loader, the
generator and the format documentation; `src/trace_replay_tests.rs` for the
replays). Each JSON file is one replayed trip: metadata, the route geometry,
an optional replacement (reroute) route, and the sequence of GPS fixes that
drives the trip at its recorded timing.

## Provenance

Every trace records `metadata.provenance` in the file itself. A trace is
**recorded route geometry plus synthetic fixes**, never a live GPS capture:

- **Route geometry is recorded.** Each scenario's start route is a committed
  file under `../routes/` (see its README): the provider's pedestrian route
  for an ordered pair of the shared demo places, taken from
  `platform/reference-scenes.json`'s `demo_routes` member — the same recorded
  routes the reference applications plan with. The generator never
  synthesizes a street; the stitched step shapes carry the recorded vertices.
- **Fixes are synthetic.** They are generated along that route from the seed
  in `metadata.generator_seed` by rustic-maps-navigation's trace_fixture generator (a
  SplitMix64 PRNG with a radially clamped Gaussian fix noise of at most 6 m).
- **The reroute route is derived, not invented.** It is the recorded route's
  remaining polyline from the reducer's frozen snapped coordinate: the first
  vertex is exactly that snap and the last is the recorded route's own
  destination.

`committed_route_fixtures_match_the_recorded_demo_routes` fails if the
committed route geometry drifts from the recorded `demo_routes` payload.

## Trace inventory

| File | Scenario | Route | Reroute route | Samples | Covers |
| --- | --- | --- | --- | --- | --- |
| `sacramento_clean_walk.json` | `sacramento_clean_walk` | 5 steps, ~1.4 km (Capitol Park → Tower Bridge) | none | 143 | Deviation-free walk: maneuvers in order, arrival |
| `sacramento_deviation_recovery.json` | `sacramento_deviation_recovery` | 2 steps, ~0.6 km (Golden 1 Center → Capitol Park) | none | 64 | 60 m excursion freezes the trip, then recovery |
| `sacramento_deviation_reroute.json` | `sacramento_deviation_reroute` | 5 steps, ~1.4 km (Tower Bridge → Capitol Park) | 3 steps, ~1.2 km | 144 | Excursion freezes the trip, replacement route continues it |

## Sample cadence

Fixes are generated every 10 m of travel at a 2 m/s walk cadence — that is,
`metadata.sample_interval_seconds` = 5 s between fixes, one fix per sample;
the final fix sits inside the eager-advance window of the route's end so the
trip always arrives. The noise ceiling (6 m radial, clamped) stays under the
recorded horizontal accuracy (8 m), so every sample's snap stays inside its
own accuracy envelope except inside the eager step-advance window (see
`trace_replay_tests.rs` for the documented exception). At 10 m spacing a
replayed trace stays a few hundred samples — each file is 15–37 KB.

## Format (short form)

```json
{
  "metadata": {
    "scenario_name": "...",
    "provenance": "...",
    "generator_seed": 0,
    "deviation_threshold_meters": 25.0,
    "horizontal_accuracy_meters": 8.0,
    "sample_interval_seconds": 5.0
  },
  "route": { ...RouteData... },
  "reroute_route": { ...RouteData... },
  "samples": [
    { "latitude": 0.0, "longitude": 0.0, "horizontal_accuracy_meters": 0.0,
      "timestamp_offset_seconds": 0.0 }
  ]
}
```

- `metadata.deviation_threshold_meters` must equal the crate's default
  deviation threshold (the loader validates this: the traces are calibrated
  against the default detection rule).
- `route` / `reroute_route` are the crate's `RouteData` JSON (see
  `crates/geospatial-types`); the loader validates the declared distances and
  bounding boxes against the geometry. `route` is copied verbatim from the
  scenario's committed recorded-route fixture.
- `samples` is strictly increasing in `timestamp_offset_seconds`; each fix
  records its horizontal accuracy explicitly so the replay can use graded
  fixes only. The reroute trace's replacement route is anchored (by
  construction, at generation time) at the reducer's exact snapped
  coordinate at the recorded deviating fix and ends at the recorded
  destination.

## Regeneration

The committed files are byte-identical with what the generator produces from
their recorded seeds and route fixtures; an always-on test fails if they
drift:

```sh
cargo test -p rustic-maps-navigation regenerated_trace_bytes_match_the_committed_fixtures
```

To regenerate the traces after changing the generator or the scenarios, and
the route fixtures first after re-recording `demo_routes`:

```sh
cargo test -p rustic-maps-navigation regenerate_recorded_route_fixtures -- --ignored
cargo test -p rustic-maps-navigation regenerate_trace_fixtures -- --ignored
```