# Rendering fixture groups

The rendering crate's fixtures fall into four groups. Every file's source,
recording date and SHA-256 is recorded in
[`../../../../fixtures/provenance.json`](../../../../fixtures/provenance.json) and
checked by `platform/web/tests/fixture_provenance.mjs`.

## `tiles/` — recorded raster imagery

216 real MapQuest Tiles API raster tiles. The standard files are
`zoom_tile_x_tile_y.png` and the retina files add `@2x`, the Tiles API's
double-resolution form for the same logical tile span. They were recorded live
with a runtime credential and no credential is stored in a file.

- z14 and z15 come from
  `crates/rustic-maps-rendering/examples/record_golden_tiles.rs` and
  `record_playground_tiles.rs`;
- z16 and z17 come from `record_navigation_corridor_tiles.rs`, which records
  the reference demos' navigation-follow corridor.

`13_1331_3143.png` is the exception the recorder examples do not produce: it
is part of the live-recorded raster set re-recorded with the rotated key at
the Sacramento re-anchor, and the test suite shares it as the TomTom raster
fixture, but no recorder in this repository records this z13 address. Its
original capture command and date are not recorded; the manifest says so
rather than inventing an address.

The live capture date for the raster set is not recorded either; the
provenance manifest records the date the files were lifted into this
repository. Regeneration is idempotent: a recorder skips a tile whose file
already exists.

## `golden/` — reference captures

PNG and binary captures rendered in this repository by the golden tests
(`golden_image.rs`, `demotiles_golden.rs`, `vector_base_golden.rs`,
`overlay_golden.rs`, `icon_sprite_golden.rs`, `poi_labels_golden.rs`,
`location_indicator_golden.rs`, `style_transition_golden.rs`). They are the
reference the renderer compares against within its documented sampler
tolerance, not inputs from a live service. A golden is rewritten only when a
test or recorder deliberately asks for it.

## `styles/` — style documents

The seven built-in style documents plus the browser's label-coverage probe.
Bright, Fiord and Positron are transcribed from
`hyperknot/openfreemap-styles` with the attribution and change statement
`PRD.md` §6 item 8 records; the OSM-Carto-like and OpenMapTiles documents are
authored in this repository; the probe is a test-only document. The exact
upstream commit each transcription started from is not recorded, and the
manifest says so.

The seven `vector-style*.json` files directly under `render/` are legacy
in-repository artifacts retained for the pre-compiler parity tests; each
derives from the same-named document under `styles/`.

## Sibling groups

- `../maplibre-demotiles/` — 16 recorded z2 MapLibre demotiles tiles.
- `../poi-labels/` — one derived extract of the committed OpenFreeMap z14 tile.

See each directory's README for the source URL and licence.
