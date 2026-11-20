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
  `0x0a`).

- `osmus_tile_z9_x83_y196.mvt` — one real regional Sourdough-schema vector
  tile for the Sacramento region, the tile the z9 Carto golden and the
  regional label goldens render. Its address is
  `https://tiles.openstreetmap.us/vector/sourdough/9/83/196.mvt`, the same
  service template as the z6 tile. No capture note records the original
  fetch date; the file was lifted into this repository with the initial
  public snapshot on **2026-11-17**, which is the date its provenance entry
  records. The two committed files under `platform/web/perf/fixtures/vector/`
  are byte-identical copies of this tile, not independent captures.

License/attribution for both tiles: map data © OpenStreetMap contributors
(<https://www.openstreetmap.org/copyright>), tiles served by OpenStreetMap US
(<https://tiles.openstreetmap.us>) under the usage policy at
<https://tiles.openstreetmap.us/usage-policy/> (free low-volume non-commercial
tier). Tests never touch the network; these committed tiles are the only
live-service artifacts in the slice.

The SHA-256 of each file is recorded in
[`../provenance.json`](../provenance.json) and checked by
`platform/web/tests/fixture_provenance.mjs`.
