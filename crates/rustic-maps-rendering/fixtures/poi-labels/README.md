# POI label fixtures

## `openmaptiles_poi_z14_x2661_y6286.pbf`

A small OpenMapTiles-schema vector tile carrying one layer, `poi`, with eight
named points of interest selected from the committed OpenFreeMap z14 tile
`fixtures/openfreemap/openmaptiles_tile_z14_x2661_y6286.pbf`:

| Name | Class | Rank |
|---|---|---|
| W Capitol Ave & Jefferson Blvd (EB) | bus | 1 |
| Raley's | grocery | 6 |
| Arthur F Turner Community Library (Yolo County Library) | library | 3 |
| Chevron | shop | 1 |
| Sutter Health Park | stadium | 1 |
| Leiva Park | park | 1 |
| 7-Eleven | shop | 1 |
| West Sacramento City Hall | town_hall | 2 |

The extraction keeps each feature's original point geometry and tile-local
coordinates; the key and value tables were pruned to the three attributes the
label pass reads (`name`, `class`, `rank`), so the whole tile is under one
kilobyte. Map data © OpenStreetMap contributors, licensed under the Open
Database License (ODbL); the source tile is served by OpenFreeMap with the
OpenMapTiles schema, which names its point-of-interest layer `poi`. Tiles from
the demo providers therefore carry this layer, and the audit
(`scripts/check-style-layer-coverage.mjs`) treats its `name` attribute as a
name-bearing purpose.

The style fixture
`crates/rustic-maps-style/fixtures/styles/poi-labels-style.json` paints the
layer as a symbol layer: `text-field` reads the feature's `name`, and
`icon-image` selects one of the committed `fixture://icon-sprite` entries for
the bus, railway, park and water classes. The headless golden
`crates/rustic-maps-rendering/tests/poi_labels_golden.rs` renders the tile over
the recorded Sacramento raster imagery. Tests never touch the network; the
fixture is committed and served from memory.
