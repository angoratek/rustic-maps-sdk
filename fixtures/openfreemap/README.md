# OpenFreeMap fixture

`openmaptiles_tile_z14_x2661_y6286.pbf` is one real z14 vector tile in the
OpenMapTiles schema, the tile the Bright, Fiord and Positron styles and the
label golden scenes render over.

- **Source:** `https://tiles.openfreemap.org/planet/latest/14/2661/6286.pbf`,
  the `url_template` of the `openfreemap` descriptor in
  `platform/providers.json` with the tile address substituted.
- **Recorded:** lifted into this repository with the initial public snapshot
  on **2026-11-17**.
- **Attribution:** map data © OpenStreetMap contributors, licensed under the
  Open Database License (ODbL); vector tiles served by OpenFreeMap
  (<https://openfreemap.org>).

The service's `latest` alias is a moving planet build and the tile bytes carry
no build date, so the exact planet build the alias resolved to on the capture
day is not recorded. The committed bytes are the authority: every test reads
this file and never the network. Its SHA-256 is recorded in
[`../provenance.json`](../provenance.json) and checked by
`platform/web/tests/fixture_provenance.mjs`.

The `poi-labels` fixture under
`crates/rustic-maps-rendering/fixtures/poi-labels/` is a derived extract of
this tile; see that directory's README.
