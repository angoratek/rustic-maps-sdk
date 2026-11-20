# openstreetmap-us fixtures

Both vector tiles are **synthetic**: this repository authors them and no tile
is downloaded from any service. They carry neutral geometry and no
OpenStreetMap data, but they use the Sourdough schema's published layer names
so the decode, styling, golden and performance paths stay exercised.

Regenerate both files, and the z7/z8 performance copies, with:

```sh
node scripts/generate-openstreetmap-us-fixtures.mjs
```

The generator writes the Mapbox Vector Tile protobuf bytes directly and prints
each file's byte count and SHA-256.

- `sourdough_tile_z6_x18_y27.mvt` — the decode fixture. Eleven
  Sourdough-schema layers (`aeroways`, `boundaries`, `highways`, `landuse`,
  `leisure`, `natural`, `places`, `power`, `railways`, `routes`, `water`) in
  wire order, one feature each, covering point, line-string and polygon
  geometry. The Tile message leads with the repeated `layers` field (wire key
  `0x1a`) and the first layer is `aeroways`.

- `osmus_tile_z9_x83_y196.mvt` — the regional fixture: the same layer
  vocabulary over the demo's region. Its `routes` layer carries a road ref
  (`80`) and a transit ref (`Gold`) for the Carto shield path; its water and
  motorway geometry carries redundant vertices so the low-zoom simplification
  gate has something to remove. The two committed files under
  `platform/web/perf/fixtures/vector/` are byte-identical copies of this tile,
  written by the same generator run.

Attribution for the schema and the provider contract: map data
© OpenStreetMap contributors (<https://www.openstreetmap.org/copyright>),
ODbL. Tests never touch the network.

The SHA-256 of each file is recorded in
[`../provenance.json`](../provenance.json) and checked by
`platform/web/tests/fixture_provenance.mjs`.
