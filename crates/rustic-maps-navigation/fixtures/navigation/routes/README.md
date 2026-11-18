# Recorded Route Fixtures

The route geometry the navigation traces replay. Each file is one
`RecordedRouteFixtureDocument`: the ordered demo place pair and the route the
routing provider returned for it, recorded live with the shared pedestrian
route options by `crates/rustic-maps-native-core/examples/record_demo_routes.rs` into
`platform/reference-scenes.json`'s `demo_routes` member (no API key appears in
the file).

The committed route is the recorded step geometry stitched at the steps'
shared vertices: every distinct recorded coordinate is kept in order, a
single-vertex maneuver step is folded into the preceding step (it carries no
traversable geometry), and the declared distances are recomputed from the
resulting polyline. No vertex is invented. The provider's recorded step texts
and durations are preserved.

`crates/rustic-maps-navigation/src/trace_fixture.rs` holds the loader, the stitching
rule and the drift gate
(`committed_route_fixtures_match_the_recorded_demo_routes`), which fails if a
committed vertex drifts from the artifact's `demo_routes` payload. Re-record
the files after the artifact changes with:

```sh
cargo test -p rustic-maps-navigation regenerate_recorded_route_fixtures -- --ignored
```
