# OSRM route fixtures

These are the payloads the shared provider registry serves for the keyless
OSRM demo address, so a browser or reference-application test can exercise
routing without a network call. Both files are byte-identical copies of the
committed fixtures in the provider layer's OSRM contract tests
(`everymap-providers-osrm 0.4.7`); the manifest records that version and the
test each file came from.

`sacramento_route.json` is a recorded `GET /route/v1/driving/...` response
with four steps between two points in downtown Sacramento.

`san_francisco_oakland_ferry_route.json` is a **hand-authored fixture**, not a
recording. It follows the documented OSRM route-service response shape for a
crossing between the San Francisco Ferry Building and Jack London Square in
Oakland and exists so the ferry regression case runs without a network call.
Its three steps are:

1. a driving step onto the ferry pier,
2. a step whose `mode` is `ferry` with the `notification` maneuver type and a
   named crossing,
3. a driving arrival step.

The totals, weights and geometry are internally consistent and plausible, but
they are illustrative values. The fixture's purpose is the classification and
instruction text, not a real-world distance. Its waypoint coordinates are the
ones the demo's browser request uses, so the route declared in
`platform/providers.json` serves it same-origin.
