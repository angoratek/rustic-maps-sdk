# Route shields

**Status:** design, owner ruling 2026-11-04 (PRD §6 item 57). Work: `PLAN.md` M74.
**Question it answers:** how does a road or transit route reference become the
shaped badge a map reads at a glance instead of a plain text label?

## Outcome

A styled base draws a route reference as a badge — a shield or rounded
rectangle with the reference centred inside, in the style's own colours — and
the reference no longer appears as a road name. A committed golden frame shows
a road ref and a transit ref side by side.

## Why it is not a text label

A symbol layer's fallback chain used to end in `ref`, so a road reference
painted as ordinary line-placed text in the same colour and halo as a street
name. Two things were wrong with that: a visitor cannot tell a route number
from a street name, and the style had no way to say what a route reference
should look like. Every map a visitor compares against draws a badge.

## Schema reality

Route references come from a tileset's `routes` source layer. The OpenMapTiles
schema (and the OpenFreeMap styles built on it: Bright, Fiord and Positron)
carries no `routes` layer; its road references live on `transportation_name`,
which also carries the road names. Those styles therefore declare no
route-reference layer, and the fallback-chain change means they paint no route
reference at all rather than a road name. The Carto-schema documents (light and
dark) do carry `routes`, and their committed OpenStreetMap US z9 regional tile
has both road refs and transit refs. The live proof is that regional Carto
frame, not a Bright z14 frame.

## Data

Route references come from the tileset's `routes` source layer, not from
`highways.ref`: a feature carries `ref` (or, in the OpenMapTiles schema, `ref`
plus `network` and `route_1`), and the layer's own filter decides which routes
it paints. The highway name stays on `highways`; a road's own name and its
route reference are different labels with different fates.

## Style

A symbol layer may declare a shield block:

| Property | Meaning |
|---|---|
| `shape` | `shield` (the two-lobed badge) or `rounded_rectangle`; a style may omit it, and the renderer then draws a rounded rectangle |
| `fill_rgba` | Badge fill |
| `border_rgba`, `border_width_pixels` | Optional outline |
| `text_rgba`, `text_size_pixels` | The reference's own colour and size |
| `padding_pixels` | Inset between the reference and the badge edge |
| `minimum_size_pixels` | Smallest badge that still clears the reference's glyphs, so a long ref grows the badge rather than spilling |

The shield block is additive: a layer that declares none keeps every existing
field and paints no badge, and its text fallbacks no longer include a route
reference. The Carto light and dark documents, whose tilesets carry `routes`,
give their `routes` layer the block; the OpenMapTiles-schema documents (Bright,
Fiord, Positron and the OpenMapTiles pair), whose tilesets carry no `routes`
layer, declare no route-reference layer.

## Rendering

The badge is drawn by the symbol pipeline, not by the host's overlay API:

1. The layer's candidates are collected exactly as text labels are (placement,
   collision, priority), with the reference's text metrics measuring the glyph
   run the atlas will draw.
2. Each surviving candidate tessellates its badge in screen space — the shape's
   outline at the measured size plus the padding — and the badge goes into the
   frame's fragment list beneath its own text.
3. The reference's glyphs draw centred inside the badge in the shield's text
   colour, not the layer's text colour.

Because the badge is a candidate like any label, it collides, fades and
prioritises with the rest of the frame's labels; no separate placement pass is
introduced.

## Rules

- A symbol layer with a shield block never falls back to `ref` as *text*: its
  label text comes from the shield path. The default fallback chain drops `ref`
  so a style without a shield block paints no reference at all rather than a
  misleading name.
- A reference longer than the badge can hold at the minimum size grows the
  badge; a reference with no glyph coverage (`?` runs) is skipped, so a badge
  never shows a placeholder.
- A frame places one label per distinct text, and badges follow the same
  rule: two candidates carrying the same reference deduplicate even when their
  features lie far apart, so a repeated candidate of one feature draws once.
- The badge is drawn from the tile's own geometry, so panning and zooming move
  it with the feature; it is not a screen-anchored overlay.

## Tests

- A style-parsing test: a layer with a shield block keeps its properties; a
  layer without one keeps today's fields and paints no badge.
- A tessellation test: a rounded rectangle and a shield for a two-glyph and a
  four-glyph reference produce a badge that encloses the measured text plus
  padding, at the minimum size when the text is small.
- A collision test: two candidates whose glyph boxes stay apart but whose
  badges overlap reject the lower-priority one, and a place name is rejected by
  an adjacent badge's own box.
- A tolerant image golden: the committed OpenStreetMap US z9 Carto regional
  frame with a road ref and a transit ref, compared on the style goldens' GPU
  tolerances (`sacramento_region_zoom9_carto_light.png`).
- A byte-exact geometry golden: the same frame's badge vertex stream, its exact
  vertex count and clip-space extent, and the encoded bytes compared
  file-to-file (`sacramento_region_zoom9_carto_light_badges.bin`). A one-float
  change in the tessellation fails it.
- The style-layer coverage gate (`scripts/check-style-layer-coverage.mjs`)
  stops allow-listing the `routes` layer as unpainted.

## Risks

- **Glyph coverage.** A transit ref can mix scripts; the badge measures what
  the atlas can draw and skips the candidate when nothing can be drawn, rather
  than drawing a box of question marks.
- **Density.** A dense route network can produce many badges; the existing
  label budget and priority rank order them, and the reserved line slots keep
  them from crowding out street names.
