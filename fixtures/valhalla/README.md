# Valhalla route fixture

`route_sacramento.json` is a recorded Valhalla `/route` response for a
Sacramento trip. The shared provider registry serves it for the keyless
Valhalla demo address, so a browser or reference-application test can exercise
routing without a network call. The file is a byte-identical copy of the
committed fixture in the provider layer's Valhalla contract tests
(`everymap-providers-valhalla 0.4.7`); the manifest records that version and
the test it came from.
