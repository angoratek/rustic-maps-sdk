# Valhalla route fixture

`route_sacramento.json` is a recorded Valhalla `/route` response for a
Sacramento trip. The shared provider registry serves it for the keyless
Valhalla demo address, so a browser or reference-application test can exercise
routing without a network call; the provider layer's Valhalla contract tests
use their own copy.
