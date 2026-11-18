# MapLibre demotiles tile fixtures

Sixteen z2 vector tiles, `2_<x>_<y>.pbf` for x and y in 0..3, recorded from
`https://demotiles.maplibre.org/tiles/2/<x>/<y>.pbf` on 2026-11-02. They back
the end-to-end golden that renders the MapLibre demotiles style over a real
public tileset; the tests that read them never touch the network.

The tiles carry the `geolines`, `centroids` and `countries` layers the style
names. Data is © OpenStreetMap contributors under the Open Database License
(ODbL); the country geometry derives from Natural Earth. The style itself is
the MapLibre demotiles style, BSD 3-Clause, Copyright (c) 2021 MapLibre; its
licence text is committed at
`docs/THIRD_PARTY_LICENSES/maplibre-demotiles-LICENSE.txt` and its attribution
is recorded in `docs/THIRD_PARTY_NOTICES.md`.
