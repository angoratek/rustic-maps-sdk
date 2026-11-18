# openstreetmap-us fixtures

- `sourdough_tile_z6_x18_y27.mvt` — one real Sourdough-schema vector tile
  (Mapbox Vector Tile protobuf), fetched from
  `https://tiles.openstreetmap.us/vector/sourdough/6/18/27.mvt` (the
  `{z}/{x}/{y}.mvt` template of `https://tiles.openstreetmap.us/vector/sourdough.json`)
  on **2026-10-29**. The service delivered the tile gzip-wrapped
  (`Content-Encoding: gzip`, `Content-Type: application/x-protobuf`,
  `Cache-Control: public, max-age=86400`); the fixture stores the
  gzip-decoded payload so the MVT decoder slice can consume it directly.
  Envelope pin: the Tile message leads with the repeated `layers` field
  (wire key `0x1a`) and the first layer is `aeroways` (`name` field,
  `0x0a`). License/attribution: map data © OpenStreetMap contributors
  (https://www.openstreetmap.org/copyright), tiles served by OpenStreetMap
  US (https://tiles.openstreetmap.us) under the usage policy at
  https://tiles.openstreetmap.us/usage-policy/ (free low-volume
  non-commercial tier). Tests themselves never touch the network; this
  committed tile is the only live-service artifact in the slice
  (single exploratory fetch).