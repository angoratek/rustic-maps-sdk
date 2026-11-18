# Style support

The SDK adopts the **full source/layer model** of the MapLibre style
specification, not a curated subset. The renderer keeps its own wgpu pipeline;
the style document, its expressions and its evaluation semantics are the SDK's
style surface on all four targets.

This document records what ships today, what the specification requires, the
Rust model, the evaluation rules, the committed styles, the licensing position
and the remaining gaps. The named icon residuals (`icon-halo-*`,
`icon-text-fit`, `icon-optional`, `icon-keep-upright`, SDF tinting and
line-placed icons) are open work recorded in `PLAN.md`.

## 1. What this document decides

- A style is a **MapLibre style document** (version 8): `sources`, `layers`,
  `glyphs`, `sprite`, and the root properties. The current data-only artifact
  format becomes a compatibility shim for one migration step and is then
  retired.
- Styling is compiled into **render plans** per layer: a geometry kind, a
  filter expression, paint properties resolved per feature at a zoom, and
  layout properties resolved for the symbol pass.
- Expressions are evaluated by a new, provider-neutral, wasm-safe crate with
  one evaluation path. Legacy `{stops, type, base, property, default}`
  functions are normalized into expressions at parse time, because the
  demotiles target still uses them.
- The renderer gains the primitives the specification actually needs: real
  line joins and caps, dashes, line offsets and gap widths, fill outlines,
  circle strokes, per-feature data-driven paint, and true text/icon layout.
  Per-feature width means the world-space overlay tessellation can no longer
  bake every screen-space size at style-build time; the implementation plan says where
  that seam changes.
- Layer order is **global across tiles**, not per tile. Page overlays stay
  above every style layer, matching the SDK's existing draw contract and
  MapLibre's annotation model.

## 2. Verified inventory of what ships today

### 2.1 The shipped style model

The model is in `crates/rustic-maps-rendering/src/vector_style.rs` (3,329
lines). `VectorTileStyle` holds an optional flat `background` RGBA and a
`Vec<VectorLayerStyle>`. Each layer has:

| Field | Meaning |
|---|---|
| `source_layer` | the decoded MVT layer name, matched by string |
| `geometry_kind` | `Fill`, `Line`, `Circle` or `Symbol` |
| `filter` | one attribute, equality plus `matches_any` plus numeric equality |
| `fill_rgba`, `line_rgba`, `circle_rgba`, `text_rgba`, `halo_rgba` | premultiplied-free RGBA constants |
| `line_width_pixels`, `circle_radius_pixels`, `text_size_pixels` | scalar constants |
| `min_zoom_level`, `max_zoom_level` | `min <= zoom < max` |
| `width_zoom_stops`, `text_size_zoom_stops` | linear zoom interpolation, clamped at the ends |
| `text_field_fallbacks` | ordered attribute names, not a text expression |
| `font_stack` | parsed and stored, **not used by the label pass** |
| `halo_width_pixels` | SDF threshold expansion, no halo blur |
| `placement` | `Point` or `Line` only |
| `priority`, `allow_overlap` | custom collision controls, not spec properties |
| `rank_zoom_stops` | stepped per-feature `rank` ceiling |
| `class_minimum_zooms` | per-`class` minimum zoom gate |

The committed artifacts are seven JSON files under
`crates/rustic-maps-rendering/fixtures/render/vector-style*.json`. Their schema
is the artifact schema, not the style specification: top-level `background`
and `layers`; layer fields such as `geometry_kind`, `rgba`, `width_pixels`,
`filter: {attribute_name, expected_text, matches_any, number_equals}`. The
parser (`VectorStyleArtifact`, `vector_style_from_json`) uses
`#[serde(deny_unknown_fields)]`; a test named
`deny_unknown_fields keeps the upstream expression surface out` pins that
today. The in-code fallback `in_code_default_vector_tile_style()` duplicates
the Carto light artifact value for value, and a test keeps the two
interchangeable.

Width and size scaling today:

- `width_pixels_at_zoom` and `text_size_pixels_at_zoom` linearly interpolate
  between ascending stops and clamp outside the stop range.
- The vector base scheduler evaluates the style at the **camera zoom
  quantized to four buckets per level**
  (`VECTOR_BASE_STYLE_ZOOM_QUANTIZATION_STEPS_PER_LEVEL = 4.0`), so a
  continuous zoom changes widths and sizes in quarter-level steps.
- Coordinates convert from tile-local extent space through the tile's
  web-mercator address in `tile_local_point_to_coordinate`; paint values are
  per layer, never per feature.
- `overlay_primitives_for_vector_tile_at_zoom` groups overlays into one
  `VectorOverlayBatch` per style layer, but `cached_frame` then flattens and
  concatenates them **per tile**. There is no global layer pass.

### 2.2 Renderer capabilities

Geometry primitives (`crates/rustic-maps-rendering/src/overlay.rs`, 3,855
lines):

- `RoutePolylineOverlay`: screen-space-thickened segment quads with a square
  join at each interior vertex and butt caps at the ends. The module
  documents that a full round-join fan is still missing. There is no dash, no
  offset, no gap width, no miter limit, no per-feature color.
- `FilledPolygonOverlay`: ear-clip triangulation with holes bridge-spliced
  into the outer ring. No outline pass, no antialias control, no pattern.
- `PointMarkersOverlay`: screen-facing octagons. No stroke, blur or pitch
  alignment.
- `PointPinsOverlay`, `TrafficFlowOverlay`: SDK annotations, not style
  primitives.
- `WorldSpaceOverlayGeometry::tessellate` caches a camera-independent
  triangle list; screen-space widths and radii are baked into that list, so a
  width change requires re-tessellation.

Draw order (`crates/rustic-maps-rendering/src/headless.rs` and the browser
canvas): tile quads, then one solid-color overlay pass (vector base overlays
and page overlays concatenated), then the label pass (halo then text), then
the location indicator. The overlay pass has alpha blending but no per-layer
opacity or blend mode. The label pass draws glyphs and sprite icons: a style's
`sprite` compiles to a typed spec, `icon-image` and its sibling icon properties
compile with spec defaults, and a MapLibre sprite index plus RGBA atlas load
through an injected `SpriteSource`.

Label and glyph pipeline (`crates/rustic-maps-rendering/src/labels.rs`,
2,323 lines; `crates/rustic-maps-glyphs`):

- Per tile, `label_candidates_for_vector_tile` reads symbol layers into
  candidates. Text is the first non-empty attribute from the fallbacks; there
  is no `text-field` expression, no `format`, no `{name}` token.
- Per frame, `LabelLayer::place` sorts globally by `(priority, style_order,
  rank)`, rejects padded axis-aligned rectangles greedily, drops repeated
  text, and caps the frame at 32 labels. Point labels center on the anchor.
  Line labels take the longest straight projected run with a fixed 20-degree
  tolerance, repeat every `text width + 90 px`, and always read upright.
- Glyphs come from one embedded Latin Noto Sans fixture
  (`default_label_font`); the style's `font_stack` is ignored. There is no
  shaping, bidi or complex-script support by design.
- Collision state is recomputed every frame. There is no persistent
  collision index and no cross-tile symbol index; deduplication is by exact
  text, which can drop two genuinely distinct places with the same name.

Style and source surfaces:

- `VectorBaseScheduler` (`crates/rustic-maps-rendering/src/vector_base.rs`)
  owns exactly one vector tileset through `VectorTileProvider` plus a URL
  template from a provider descriptor. There is no TileJSON resolution, no
  `bounds`, `scheme`, `promoteId`, `volatile`, `minzoom`/`maxzoom` source
  range, and no second source.
- The public surfaces take a **style id string**, not JSON:
  `UrlTemplateVectorTileSource.style_id` in
  `crates/rustic-maps-native-core/src/rustic_maps.udl`, and the
  `vectorTileOverlayDescriptorsForStyle` wasm export in
  `crates/rustic-maps-web-bindings/src/vector_layer.rs`.
  Seven cartographic ids exist (`carto-light`, `carto-dark`,
  `openmaptiles-light`, `openmaptiles-dark`, `bright-light`, `fiord-dark`,
  `positron-light`). An eighth, `label-coverage-probe`, is a measurement
  fixture rather than a base map: one literal white `I` at 48 point pixels on
  black, used by the browser SDF edge probe to isolate a single glyph through
  the same vector-source path.
- `platform/providers.json` names each provider's tile URL, attribution,
  fixture routes and style ids. Provider URLs never enter the renderer.
- Background is a flat clear color. Mercator is the only projection.

### 2.3 Acceptance fixtures today

Goldens that must keep their meaning through the migration:

- `sacramento_zoom14_bright_vector_base.png`
- `sacramento_zoom14_openmaptiles_light_vector_base.png`
- `sacramento_region_zoom9_carto_light.png`
- `sacramento_zoom14_place_labels.png`
- `sacramento_zoom14_road_names.png`
- the raster goldens that compose with the vector base

The harness tolerates GPU sampler differences (mean absolute error 2.0,
outlier channel error 24.0 over at most 2 percent of pixels). Missing goldens
are written only with `UPDATE_GOLDENS=1`. The vector fixtures are committed
MVT tiles under `fixtures/openfreemap` and `fixtures/openstreetmap-us`; tests
never touch the network.

## 3. Upstream reference material

Fetched 2026-11-01. These are the exact pages the design relies on.

| Material | URL | Notes |
|---|---|---|
| Style specification root | <https://maplibre.org/maplibre-style-spec/> | top-level properties and section index |
| Layers | <https://maplibre.org/maplibre-style-spec/layers/> | layer types, common properties, paint/layout tables |
| Sources | <https://maplibre.org/maplibre-style-spec/sources/> | six source types, TileJSON and URL tokens |
| Expressions | <https://maplibre.org/maplibre-style-spec/expressions/> | operator list, `step`/`interpolate`, type and zoom rules |
| Root | <https://maplibre.org/maplibre-style-spec/root/> | version 8, sprite, glyphs, projection, transition |
| Deprecated functions | <https://maplibre.org/maplibre-style-spec/deprecations/> | legacy `stops`/`type`/`base`/`property` semantics |
| MapLibre Native repository | <https://github.com/maplibre/maplibre-native> | BSD-2-Clause; current `src/mln` layout |
| Native style implementation | <https://github.com/maplibre/maplibre-native/tree/main/src/mln/style> | parser, properties, expression, layers, sources |
| Native layer implementations | <https://github.com/maplibre/maplibre-native/tree/main/src/mln/style/layers> | per-type `*_layer_impl` and `*_layer_properties` |
| Native renderer | <https://github.com/maplibre/maplibre-native/tree/main/src/mln/renderer> | `tile_pyramid`, `bucket`, `render_orchestrator`, property evaluators |
| Native text | <https://github.com/maplibre/maplibre-native/tree/main/src/mln/text> | `placement`, `collision_index`, `cross_tile_symbol_index`, shaping |
| Native design book | <https://maplibre.org/maplibre-native/docs/book/design/> | renderer modularization note; partially outdated |
| Native ten-thousand-foot view | <https://maplibre.org/maplibre-native/docs/book/design/ten-thousand-foot-view.html> | map/style/source/tile/actor model |
| Style-spec repository | <https://github.com/maplibre/maplibre-style-spec> | BSD-3-Clause, `LICENSE.txt` |
| Demo tiles repository | <https://github.com/maplibre/demotiles> | BSD-3-Clause; styles, tiles, fonts |
| Demo tiles style | <https://demotiles.maplibre.org/style.json> | the concrete target: 8 layers, 2 sources |

The demotiles style is the concrete target and it is deliberately demanding:
it uses legacy `{"stops": ...}` functions for `line-width`, `line-opacity`,
`text-size`, `text-field` and `text-transform`; a `match` expression for
country fills; `{name}` token text fields; `line-dasharray`; round joins and
caps; `text-max-width`; and a GeoJSON source beside a vector source. A parser
that only accepts modern expressions cannot load it.

The Native repository confirms the semantic vocabulary this design mirrors:
style layers compile into tile buckets; paint and layout properties are
resolved through property evaluators; symbol placement and collision live in
`src/mln/text`; tile scheduling lives in `renderer/tile_pyramid`. The exact
per-file C++ logic was not read line by line, so the parts that depend on
fine-grained behavior must confirm against the source or its render tests.

## 4. Gap analysis

Status values: **SUPPORTED** (a style property works today), **PARTIAL**
(some cases work, with a different or narrower model), **ABSENT**. The
responsible file is the one that would have to change. Effort: S under a day,
M a few days, L a week or so, XL more than a week. Risk is the chance of
changing existing golden output or destabilizing the pipeline.

### 4.1 Document, root and sources

| Feature | Status | Responsibility | Effort | Risk |
|---|---|---|---|---|
| `version: 8` document root | ABSENT | new `rustic-maps-style/src/document.rs` | M | low |
| `name`, `metadata` | ABSENT | same | S | low |
| root `center`/`zoom`/`bearing`/`pitch` | ABSENT | SDK camera owns the view; parse and ignore | S | low |
| `projection` mercator | SUPPORTED as renderer behavior | not style-controlled | S | low |
| `projection` globe / vertical-perspective | ABSENT | `rustic-maps-rendering/src/projection.rs` | XL | high |
| `sprite` | ABSENT | new sprite module + canvas/headless pipelines | L | high |
| `glyphs` URL | PARTIAL: provider descriptor only | `vector_base.rs`, provider traits | M | medium |
| `transition`, cross-fade | PARTIAL: root and per-property timings blend the painted colour and opacity subset | render plans + frame clock | M | medium |
| `light`, `sky`, `terrain`, `state` | ABSENT | deferred | XL | high |
| one vector source (tiles in a URL template) | PARTIAL | `vector_base.rs` | M | medium |
| TileJSON `url`, `bounds`, `scheme`, `minzoom`/`maxzoom`, `volatile`, `encoding` | ABSENT | source resolver | M | medium |
| multiple sources in one style | ABSENT | `vector_base.rs`, frame assembly | L | high |
| `promoteId` and feature ids | PARTIAL: the decoder keeps `identifier` | style crate + feature-state | M | medium |
| GeoJSON source | ABSENT | new source implementation | L | high |
| image / video / raster-dem source | ABSENT | deferred | XL | high |
| raster / hillshade / color-relief layers inside a style | ABSENT (raster path exists outside styles) | style compile + raster scheduler | L | medium |

### 4.2 Layer common model

| Feature | Status | Responsibility | Effort | Risk |
|---|---|---|---|---|
| `id` and layer identity | ABSENT (index order only) | style crate | S | low |
| `source` binding plus `source-layer` | PARTIAL (`source_layer` only) | style crate + scheduler | M | medium |
| `minzoom` / `maxzoom` | SUPPORTED | `vector_style.rs` | none | none |
| `visibility` | ABSENT | style crate | S | low |
| filter expressions (`all`, `any`, `!`, `==`, `!=`, `<`, `<=`, `>`, `>=`, `in`, `has`, `$type`) | PARTIAL: one attribute equality | new `filter.rs` | M | medium |
| filter zoom restrictions | ABSENT | expression analyzer | S | medium |
| `line-sort-key`, `symbol-sort-key` | SUPPORTED | render plan ordering | none | none |
| `fill-sort-key`, `circle-sort-key` | SUPPORTED (stable per-layer feature order) | `vector_style.rs` `overlays_for_layer` | none | none |
| global layer order across tiles | ABSENT (per-tile concatenation) | `vector_base.rs` frame assembly | S | medium |
| data-driven paint (per feature) | SUPPORTED (resolved per feature from expressions) | render plan + tessellation | none | none |
| legacy functions (`stops`, `base`, `type`, `property`, `default`) | PARTIAL: linear zoom stops on width/size only | new `legacy_function.rs` | M | low |
| `step` | PARTIAL: rank ceilings only | expression engine | S | low |
| `interpolate` linear | PARTIAL: width and text size only | expression engine | M | low |
| `interpolate` exponential / cubic-bezier | ABSENT | expression engine | M | low |
| `interpolate-hcl` / `interpolate-lab` | ABSENT | expression engine | M | low |
| `let` / `var` / `coalesce` / `case` / `match` | ABSENT | expression engine | M | low |
| math, string, color, lookup operators | ABSENT | expression engine | L | low |
| type checking and default-on-failure | ABSENT | expression engine | M | medium |
| layout evaluated at integer zoom | ABSENT (uses fractional) | render plan evaluation | M | medium |
| paint evaluated continuously at frame zoom | PARTIAL (quarter-level bucket) | scheduler cache key | M | medium |
| `feature-state`, `global-state`, `heatmap-density`, `line-progress` | ABSENT | deferred | L | high |

### 4.3 Fill

| Feature | Status | Responsibility | Effort | Risk |
|---|---|---|---|---|
| `fill-color` | SUPPORTED (constant and expression) | `vector_style.rs` | none | none |
| `fill-opacity` | SUPPORTED | render plan | none | none |
| `fill-outline-color` | SUPPORTED (one-pixel boundary ring; absent paints none) | `overlay.rs` `emit_filled_polygon_outline` | none | none |
| `fill-antialias` | SUPPORTED (`false` paints no outline) | overlay pipeline | none | none |
| `fill-translate` | SUPPORTED (screen pixels) | overlay assembly | none | none |
| `fill-translate-anchor` | SUPPORTED (`map` rotates the displacement with the camera bearing; `viewport` keeps screen pixels) | overlay assembly | none | none |
| `fill-pattern` | ABSENT | pattern atlas | L | high |
| `fill-sort-key` | SUPPORTED (stable per-layer feature order) | `vector_style.rs` `overlays_for_layer` | none | none |
| `fill-layer-opacity` / `fill-layer-blend` | ABSENT | draw passes | L | high |

### 4.4 Line

| Feature | Status | Responsibility | Effort | Risk |
|---|---|---|---|---|
| `line-color` | SUPPORTED (constant) | `vector_style.rs` | none | none |
| `line-width` | PARTIAL (constant + linear stops) | render plan + tessellation | M | medium |
| `line-opacity` | PARTIAL (alpha channel) | render plan | S | low |
| `line-join` (`miter` default, `bevel`, `round`) | SUPPORTED | `overlay.rs` `push_stroke_join` | none | none |
| `line-cap` (`butt` default, `round`, `square`) | SUPPORTED | `overlay.rs` `push_stroke_cap` | none | none |
| the miter limit (fixed at 2 by the specification) | SUPPORTED (sharper corners fall back to bevel) | `overlay.rs` `LINE_MITER_LIMIT` | none | none |
| `line-dasharray` | SUPPORTED (on/off runs in line-width units, phase across segments) | `overlay.rs` `dash_on_runs` | none | none |
| `line-offset` / `line-gap-width` | SUPPORTED (perpendicular offset; two casing bands outside the gap) | `overlay.rs` `stroke_bands` | none | none |
| `line-blur` | PARTIAL (a wider, fainter fringe painted under the core stroke) | `overlay.rs` `LINE_BLUR_FRINGE_ALPHA_SCALE` | M | medium |
| `line-sort-key` | SUPPORTED (stable per-layer feature order) | `vector_style.rs` `overlays_for_layer` | none | none |
| `line-pattern` / `line-gradient` | ABSENT | deferred | XL | high |
| data-driven width/color/opacity | SUPPORTED (resolved per feature from expressions) | `vector_style.rs` | none | none |

### 4.5 Circle and background

| Feature | Status | Responsibility | Effort | Risk |
|---|---|---|---|---|
| `circle-radius`, `circle-color` | SUPPORTED (constant and expression) | `vector_style.rs` | none | none |
| `circle-opacity` | SUPPORTED | render plan | none | none |
| `circle-blur` | PARTIAL (a wider, fainter fringe under the marker) | `overlay.rs` | M | medium |
| `circle-stroke-width/color/opacity` | SUPPORTED (stroke drawn outside the radius) | marker pipeline | none | none |
| `circle-translate` | SUPPORTED (screen pixels) | marker pipeline | none | none |
| `circle-pitch-scale`, `circle-pitch-alignment` | SUPPORTED (`map` behaviour assembles in camera space; an omitted value keeps this renderer's screen-facing default) | marker pipeline | none | none |
| `circle-sort-key` | SUPPORTED (stable per-layer feature order) | `vector_style.rs` `overlays_for_layer` | none | none |
| `background-color` | SUPPORTED (flat clear color) | `vector_base.rs` | none | none |
| `background-opacity` | SUPPORTED (folded into the clear color's alpha) | clear color path | none | none |
| `background-pattern` | ABSENT | pattern atlas | L | high |

### 4.6 Symbol and text

| Feature | Status | Responsibility | Effort | Risk |
|---|---|---|---|---|
| `symbol-placement` `point` / `line` | PARTIAL | `labels.rs` | none | none |
| `symbol-placement` `line-center` | SUPPORTED (one placement at the run midpoint) | `labels.rs` | none | none |
| `symbol-spacing` | SUPPORTED (the declared value; the renderer keeps a 90 px default for a layer that omits it) | `labels.rs` | none | none |
| `symbol-avoid-edges` | SUPPORTED (a label whose padded box would cross the viewport edge is dropped) | placement | none | none |
| `symbol-sort-key` | SUPPORTED (mapped to the placement priority) | placement | none | none |
| `symbol-z-order` | ABSENT (draw order follows placement order) | placement | M | medium |
| `text-field` expression / `format` / `{token}` | SUPPORTED (expressions; `format` flattened to its text; legacy `{property}` tokens substituted) | `labels.rs`, style crate | none | none |
| `text-font` | PARTIAL: parsed, ignored | glyph font selection | M | medium |
| `text-size` | PARTIAL: linear stops, fractional zoom | render plan | S | low |
| `text-max-width` (wrapping) | SUPPORTED (greedy word wrapping; line-placed names stay on one line) | `labels.rs` | none | none |
| `text-line-height`, `text-letter-spacing` | SUPPORTED | glyph layout | none | none |
| `text-justify` | SUPPORTED (`auto`/`center` center each line; `left`/`right` align to the block) | glyph layout | none | none |
| `text-transform` | SUPPORTED (none, uppercase, lowercase) | style resolution | none | none |
| `text-offset` | SUPPORTED (a screen-pixel displacement from the anchor) | placement | none | none |
| `text-anchor` | SUPPORTED (all nine positions; line-placed names ignore it) | placement | none | none |
| `text-radial-offset`, `text-variable-anchor` | SUPPORTED (anchors tried in order, the first non-colliding one kept; the radial offset pushes the block outward in ems) | placement | none | none |
| `text-rotation-alignment`, `text-keep-upright` | SUPPORTED (map or viewport rotation for point labels, `auto` by placement; keep-upright flips past a quarter turn) | placement | none | none |
| `text-max-angle` | SUPPORTED (a declared angle sets the run tolerance; the renderer keeps its 20-degree default for a layer that omits it) | placement | none | none |
| `text-padding` | PARTIAL: fixed 2 px | placement | S | low |
| `text-allow-overlap`, `text-ignore-placement` | SUPPORTED (both skip collision rejection) | placement | none | none |
| `text-overlap`, `text-optional` | ABSENT | placement | M | medium |
| `text-color`, `text-halo-color`, `text-halo-width` | SUPPORTED | labels | none | none |
| `text-halo-blur`, `text-opacity`, `text-translate` | ABSENT | glyph shader | M | medium |
| icon properties and sprite atlas | ABSENT | new icon pipeline | XL | high |
| persistent collision index and cross-tile symbol index | PARTIAL: placements persist across frames, greedy per frame; cross-tile index absent | placement | M | high |
| complex-script shaping, bidi | ABSENT by design | deferred | XL | high |

#### Route-reference badges

A symbol layer may declare a layer-level `shield` block. The block is additive:
a layer without one keeps every existing field and paints no badge. Its
members are `shape` (`shield` for the two-lobed highway badge or
`rounded_rectangle`; omitted draws a rounded rectangle), `fill_rgba`, an
optional `border_rgba` with `border_width_pixels`, `text_rgba`,
`text_size_pixels`, `padding_pixels` and `minimum_size_pixels`.

A layer with a badge reads its label from the tile's route-reference attribute
`ref` through the shield path; it never consults `text-field`, and the
reference draws centred in the badge in the block's own colour and size. The
badge tessellates in screen space around the measured glyph run plus the
padding, grows past `minimum_size_pixels` for a long reference, and emits into
the label layer's badge pass beneath its own text. Badge candidates keep the
existing placement, collision, priority, fade and reserved-line-slot rules, and
a reference whose glyphs the atlas cannot serve is skipped rather than painted
as placeholders. The default symbol text fallbacks stop at the local name, so a
layer without a badge paints no route reference at all.

The OpenMapTiles schema and the OpenFreeMap styles built on it (Bright, Fiord
and Positron) carry no `routes` source layer: their road references live on
`transportation_name`, which also carries the names. Those documents therefore
declare no route-reference layer. The Carto light and dark documents do carry a
`routes` layer and declare the badge block. The committed OpenStreetMap US z9
tile is the only fixture with road and transit refs, so the proof is the
regional Carto frame, not a Bright z14 frame.

`crates/rustic-maps-rendering/fixtures/render/golden/sacramento_region_zoom9_carto_light.png`
pins the regional Carto frame with road and transit reference badges, and the
native styled-base suite asserts that both a road ref and a transit ref place
as badges. The same suite pins the frame's badge vertex stream byte-for-byte
(`sacramento_region_zoom9_carto_light_badges.bin`) with its exact vertex count
and clip-space extent, so a one-float tessellation change fails even when the
tolerant image compare absorbs it.

### 4.7 Layer types outside the current model

| Feature | Status | Responsibility | Effort | Risk |
|---|---|---|---|---|
| `heatmap` | ABSENT | deferred | XL | high |
| `fill-extrusion` | ABSENT | deferred | XL | high |
| `raster`, `hillshade`, `color-relief` as style layers | ABSENT | style compile + raster path | L | medium |

## 5. Target design

### 5.1 Crate layout

Add one crate, `crates/rustic-maps-style/`, and keep the renderer free of
document parsing.

```
crates/rustic-maps-style/
  src/lib.rs
  src/document.rs        // MapStyleDocument, sources, layers, root
  src/types.rs           // Property<T>, Value, Color, Padding, Anchor
  src/color.rs           // CSS color parsing and RGB/HCL/LAB interpolation
  src/expression/
    mod.rs               // Expression, ExpressionKind, parser
    evaluate.rs          // EvaluationContext and evaluation
    interpolation.rs     // linear, exponential, cubic-bezier
    operators.rs         // decision, math, string, lookup, type conversion
  src/filter.rs          // legacy filter arrays and expression filters
  src/legacy_function.rs // stops/type/base/property/default normalization
  src/defaults.rs        // documented property defaults
  src/error.rs           // parse and evaluation diagnostics
  tests/
    demotiles_style.rs
    expression_conformance.rs
    filter_conformance.rs
  fixtures/styles/demotiles-style.json
```

Why a new crate rather than extending `rustic-maps-vector-tiles` or
`rustic-maps-rendering`:

- `rustic-maps-vector-tiles` is the MVT decoder and must stay data-only and
  free of style concerns.
- The style model is renderer-facing but GPU-free, wasm-safe and usable by
  tests and tooling that do not link wgpu.
- The renderer depends on it in one direction; no provider crate is aware of
  it, which preserves provider genericity.

The renderer keeps `vector_style.rs` as the **compiler** for one migration
step. It gains:

- `compile_style_document(document, source_bindings) -> Result<CompiledStyle, StyleError>`
- `CompiledStyle` holds ordered `CompiledLayer` values: kind, source id,
  source layer, `FilterExpression`, resolved paint descriptors, layout
  descriptors and sort key.
- Renderer-side responsibilities: paint resolves per feature at a zoom and
  symbol layout resolves at integer zoom. The implementation keeps them in
  `vector_style.rs` for paint, `labels.rs` for symbol layout and placement and
  `overlay.rs` for assembly; the GPU-free assembly seams gained the new
  semantics rather than being bypassed.

### 5.2 The style document model

```
MapStyleDocument {
    version: u8,                       // must be 8
    name: Option<String>,
    metadata: Option<serde_json::Value>,
    center/zoom/bearing/pitch: Option<...>, // parsed, the SDK camera wins
    projection: Projection,            // mercator only
    sources: BTreeMap<String, SourceSpec>,
    sprite: Option<SpriteSpec>,
    glyphs: Option<String>,
    transition: Option<TransitionSpec>,
    layers: Vec<StyleLayer>,
    unsupported: Vec<UnsupportedProperty>, // diagnostics, never silent
}

SourceSpec = Vector(VectorSourceSpec) | Raster(RasterSourceSpec)
           | GeoJson(GeoJsonSourceSpec) | RasterDem(...) | Image(...) | Video(...)

StyleLayer {
    id: String,
    kind: LayerKind,
    source: Option<String>,
    source_layer: Option<String>,
    min_zoom: f64,                     // 0
    max_zoom: f64,                     // 24
    visibility: Visibility,            // visible
    filter: Option<FilterExpression>,
    layout: LayerLayout,
    paint: LayerPaint,
}
```

`LayerPaint` and `LayerLayout` are per-kind structs of `Property<T>`:

```
enum Property<T> {
    Constant(T),
    Expression(Expression),   // typed by the property's T
}
```

Only the properties this plan implements get typed fields. Every other
property the parser recognizes is accepted, recorded in
`unsupported`/diagnostics with its layer id, and does not fail the document.
That is what lets a Bright style with an unimplemented `icon-image` layer
still draw its roads, and lets the diagnostic surface name exactly what is
missing. MapLibre's own behavior for unknown properties is to reject the
style; this SDK deliberately degrades instead, because one style layer must
never blank the base map. The difference is recorded here as a product
choice, not an oversight.

Property defaults live in `defaults.rs` and mirror the specification
(`fill-color` `#000000`, `line-width` 1, `line-cap` `butt`, `line-join`
`miter`, `text-size` 16, `text-max-width` 10 ems, `text-line-height` 1.2,
`text-padding` 2, `symbol-spacing` 250, `circle-radius` 5, and so on). No
literal defaults are scattered in the evaluator.

### 5.3 Expressions

`Expression` is a typed tree. The parser accepts the JSON array form, plus
the legacy function object form through `legacy_function.rs`, so both
normalize into the same tree.

Operator set, in landing order:

1. **Core**: `literal`, `get`, `has`, `geometry-type`, `id`, `properties`,
   `zoom`, `let`, `var`, `all`, `any`, `!`, `==`, `!=`, `<`, `<=`, `>`, `>=`,
   `case`, `match`, `coalesce`, `step`, `interpolate`, `to-string`,
   `to-number`, `to-boolean`, `to-color`, `rgb`, `rgba`, `concat`, `upcase`,
   `downcase`, `length`, `at`, `in`, `index-of`, `slice`, `+`, `-`, `*`, `/`,
   `%`, `^`, `min`, `max`, `round`, `floor`, `ceil`, `abs`, `sqrt`, `ln`,
   `log10`, `log2`, `sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `pi`, `e`,
   `ln2`.
2. **Text**: `format` and the token form `"{name}"` for `text-field`, then
   `collator`/`resolved-locale` only if a real localization need appears.
3. **State**: `feature-state`, `global-state`, `heatmap-density`,
   `line-progress`, `accumulated`, `within`, `distance`,
   `is-supported-script`, `number-format` are parsed but return `null` or a
   diagnostic until the owning work lands.

Every expression is analyzed at parse time into one of four kinds:
`Constant`, `Camera`, `DataDriven`, `CameraAndData`. The analysis enforces the
specification's restrictions: in a layout or paint property, `["zoom"]` may
only be the direct input of a top-level `step` or `interpolate`, optionally
wrapped in `let`; a filter must be a data expression returning a boolean.
A violation is a parse diagnostic that disables the property, never a panic.

### 5.4 Filters

Filters are expressions returning boolean. Both spellings are accepted:

- modern: `["==", ["get", "class"], "motorway"]`
- legacy: `["all", ["==", "class", "motorway"], ["!=", "class", "rail"]]`

The legacy array filter is normalized by rewriting bare strings used as
input into `["get", name]`, and `$type` into `["geometry-type"]`. The
normalized tree is the same `Expression` used by paint. The existing
`VectorAttributeFilter` and `feature_matches_filter` are retired after the
compatibility step.

Coercion follows the specification, not Rust's `PartialEq`: `==` compares
numbers numerically and strings lexically, and a mixed comparison is false
unless a conversion operator produced the value. Property values come from
the decoder's `VectorTileAttributeValue` variants (text, float, double,
signed, unsigned, zigzag signed, boolean per the decoder's seven variants).
The exact legacy coercion table is a conformance-test item, not an
assumption.

### 5.5 Evaluation rules

Evaluation context:

```
EvaluationContext {
    zoom: f64,                 // frame zoom for paint, floor(zoom) for layout
    feature: Option<&VectorTileFeature>,
    source_id: Option<&str>,
    source_layer: Option<&str>,
    global_state: Option<&GlobalState>,
    feature_state: Option<&FeatureState>,
}
```

Rules:

- **Paint** camera expressions resolve at the frame's fractional zoom, so a
  width or size changes continuously with the camera. Layout camera
  expressions resolve at the integer zoom the specification requires.
- `step` returns the output of the greatest stop at or below the input; below
  the first stop it returns the first output.
- `interpolate` clamps outside the stop range and blends between adjacent
  stops. `["linear"]` uses the fraction directly. `["exponential", base]`
  uses `(base^(z - z0) - 1) / (base^(z1 - z0) - 1)`, with `base == 1`
  reduced to linear. `["cubic-bezier", x1, y1, x2, y2]` solves the x curve
  for the fraction then evaluates the y curve. Stops must be strictly
  ascending numeric literals, validated at parse time.
- Colors interpolate per channel in the default space; `interpolate-hcl`
  and `interpolate-lab` are separate operators. The exact default space and
  premultiplication rule must be confirmed against the upstream expression
  test suite before the style-document work completes; this document does not pretend to know it
  beyond the specification's wording.
- A property expression that fails at runtime (type error, missing property
  under a non-`coalesce` path, unsupported operator) falls back to the
  property's default and emits one diagnostic per layer and property, not per
  feature, so a dense tile cannot flood the log.
- `minzoom`/`maxzoom`: visible when `minzoom <= zoom < maxzoom`.

The scheduler's quarter-level style bucket is replaced by a cache key that
includes a hash of the resolved style revision plus the resolved paint
quantized to a named constant (initially 1/16 zoom). Geometry is
re-tessellated when that key changes. This keeps the cache bounded while
making width and size changes visually continuous. If measurement shows
per-frame re-tessellation dominates, the plan permits a fixed bucket
plus cross-fade, but the default is the finer key.

### 5.6 Compilation to render plans

`compile_style_document` produces, in document order:

```
CompiledLayer {
    id, kind, source, source_layer,
    filter,
    paint: PaintKind,            // FillPaint, LinePaint, CirclePaint, ...
    layout: LayoutKind,          // SymbolLayout, or empty
    sort_key: Property<f64>,
    order_index: usize,
}
```

Tessellation inputs:

- **Fill**: resolve `fill-color` and `fill-opacity` per feature; emit the
  hole-aware `FilledPolygonOverlay` with the resolved RGBA. `fill-outline`
  emits a boundary polyline with the resolved color, drawn immediately after
  the fill in the same layer.
- **Line**: resolve `line-color`, `line-width`, `line-opacity`, `line-offset`,
  `line-gap-width`, `line-blur`, `line-join`, `line-cap`, `line-dasharray`
  per feature. Emit a styled-line primitive that carries the resolved width,
  color and join/cap/dash parameters. One primitive per feature, so
  data-driven values survive.
- **Circle**: resolve radius, color, opacity, blur, stroke width and stroke
  color per feature; emit one styled marker primitive per feature.
- **Symbol**: resolve text and icon fields, size, font, transform, wrapping,
  spacing, angle, anchor and overlap rules per feature into candidates; the
  frame-level placement module consumes them.
- **Background**: the clear color; pattern and opacity later.

The existing `RoutePolylineOverlay` remains the page-overlay type for routes
and traffic. Style lines get their own primitive so that a style change
cannot accidentally gain route defaults, and so the renderer can add dash
and join data without widening the annotation API.

### 5.7 Layer order and composition with page overlays

The frame assembly changes from "per tile, flatten all layers" to:

1. Collect `Vec<StyledLayerBatch>` per tile into a frame-level map keyed by
   the layer's document position.
2. Flatten in document order: every tile's layer 0, then every tile's layer
   1, and so on. Within a layer, sort features by `*-sort-key`, then by
   deterministic source order.
3. Append the page's own overlays after every style layer, preserving the
   existing contract: routes, markers, traffic and the location indicator
   stay above the base map.

This is the single largest correctness fix in the source/layer model: a fill
from one tile can no longer paint over a road that belongs to another tile's
earlier layer.

Page overlays do not participate in style collision or sort keys. MapLibre's
annotations behave the same way; a style-driven icon layer that should
receive collision must be a symbol layer, not a page overlay.

### 5.8 Sources

A style source is a declaration, not a vendor URL baked into the renderer.
The host binds each source id to a registered provider or fixture through
`SourceBinding { provider_key, url_template, attribution_text, glyphs_url,
minimum_zoom, maximum_zoom }`. This preserves three standing rules:

- provider URLs do not leak into the renderer or the reference apps;
- credentials stay in the provider registration path;
- a test binds the same source id to a fixture provider and never touches the
  network.

The sources work implements multiple vector sources and GeoJSON. GeoJSON is consumed
through a small tiling step at the source boundary: features are clipped to
the same tile grid the vector source uses, so downstream buckets and symbol
placement see one feature model. The exact GeoJSON tiler is a design item for
that work; it must stay deterministic for goldens.

### 5.9 Symbol placement

The placement module is modeled on MapLibre Native's stages:

1. **Layout**: shape text, resolve `text-field` with `format`, apply
   `text-transform`, wrap at `text-max-width`, measure glyph boxes with the
   selected font stack, choose anchors and offsets, and compute line glyph
   placement with `text-max-angle`, `symbol-spacing` and
   `text-keep-upright`.
2. **Placement**: a frame-global pass with a persistent collision index.
   Icons and text collide as separate boxes unless `icon-optional` ties
   them. `symbol-sort-key` (and the legacy priority field, mapped to the
   spec key) orders placement. `*-ignore-placement` symbols are drawn but
   do not block; `*-allow-overlap`/`overlap: always` symbols ignore existing
   boxes.
3. **Cross-tile dedupe**: a cross-tile symbol index keyed by source feature
   id (or a stable text-plus-geometry hash when no id exists) prevents the
   buffered overlap between tiles from painting one place twice. The current
   text-only dedupe is the fallback when ids are absent; it can drop two
   distinct same-named places, which the index fixes.
4. **Draw**: glyph quads for text, sprite quads for icons, halo first.

The persistent index must be invalidated on style revision, source change,
zoom-level change and glyph/sprite atlas revision. That invalidation set is
part of the verification, not an implementation detail left to
chance.

### 5.10 Colors and units

- Parse CSS color forms the styles in the wild use: `#rgb`, `#rgba`,
  `#rrggbb`, `#rrggbbaa`, `rgb()`, `rgba()`, `hsl()`, `hsla()` and the
  named colors. Store as linear-space RGBA? The renderer's existing colors
  are sRGB components in `[0, 1]`; the parser preserves sRGB components and
  converts only where the shader expects otherwise. This choice must be
  checked against the current golden output before the first style document
  renders.
- Units follow the specification: `line-width` in screen pixels,
  `line-dasharray` in line-width units, `text-size` in pixels,
  `text-max-width`, `text-line-height`, `text-letter-spacing` and
  `text-radial-offset` in ems, `text-offset` and `icon-offset` in ems,
  `*padding` in pixels, `*translate` in pixels.
- Device pixel ratio scales drawing-buffer sizes at the surface boundary, as
  today; style numbers remain point/CSS pixels.

## 6. Migration path for the committed styles

The migration must keep rendering identical where the old model and the
specification agree, and prove it with the existing goldens. The road
hierarchy, per-class minimum zooms and width curves (Appendix A) become
`minzoom` values and `interpolate` expressions in the converted documents, so
that work is carried forward rather than relearned.

1. **Convert, do not retype.** Add `scripts/convert-vector-style-artifacts.mjs`
   (or a Rust example if JS is the wrong home) that reads each committed
   artifact and writes a version-8 style document:
   - one vector source `base`;
   - `source_layer` becomes `source: "base"` plus `source-layer`;
   - `geometry_kind` becomes the layer `type`;
   - `rgba` becomes `fill-color`/`line-color`/`circle-color`/`text-color`
     with the same numeric channels;
   - `filter` becomes an equivalent `all`/`==` expression;
   - `width_zoom_stops` and `text_size_zoom_stops` become
     `["interpolate", ["linear"], ["zoom"], ...]`;
   - `min_zoom`/`max_zoom` become `minzoom`/`maxzoom`;
   - symbol fields map to `text-field` fallbacks, `text-size`,
     `text-halo-color`, `text-halo-width`, `symbol-placement`,
     `symbol-sort-key` and the equivalent allow-overlap spelling;
   - `class_minimum_zooms` and `rank_zoom_stops` have no direct spec
     equivalent; they become an `all` filter with `case`/`step` on `["zoom"]`
     and the feature `class`/`rank`, preserving the exact behavior.
   The converter is committed and rerunnable; the generated documents are
   committed beside it so a reader never has to run Node to see the style.
2. **Dual-run parity.** A new test iterates the seven styles, the committed
   fixture tiles and zoom levels 9, 12, 14, 16 and 18, and compares:
   - `overlay_primitives_for_vector_tile_at_zoom` old versus new, overlays
     compared structurally (kind, RGBA, width, coordinates within 1e-9);
   - `label_candidates_for_vector_tile` old versus new, compared field by
     field.
   The test fails on any difference. It is the migration's proof, not a
   formality.
3. **Goldens unchanged.** Run all five vector-base goldens without
   `UPDATE_GOLDENS=1`. Single-tile scenes must be identical. The multi-tile
   road-name scene may change only at tile seams because global layer order
   is intentionally different; if it changes, the diff must be inspected and
   recorded in the change notes, never blind-regenerated.
4. **Retire the old path.** After parity and a green full gate, delete the
   legacy artifact parser and the in-code fallback table, or keep the
   fallback as a version-8 document constant. The decision is made when
   The compilation work lands; the design prefers one parser.

The committed product styles remain provider-neutral: their source id is
resolved through the registry, and no vendor URL is copied into a style
document. The demotiles fixture is the one test-owned document that carries
a public URL, and it is rewritten to a fixture binding at load time.

## 7. What is mirrored from MapLibre Native, and what is not

Mirrored, reimplemented from the specification and the published design
(no C++ copied):

- The document model: sources, layers, layout/paint split, root properties.
- Expression semantics: typed parsing, zoom restrictions, `step`,
  `interpolate` curves, legacy function normalization, filter normalization.
- The bucket idea: per tile, per layer, per zoom, compiled once and cached;
  paint resolved at the frame zoom, layout at integer zoom.
- Layer and feature ordering, including sort keys and global layer order.
- Symbol semantics: text layout, anchors, line placement with a maximum
  angle, spacing, keep-upright, collision boxes, ignore-placement, optional,
  a persistent collision index and a cross-tile symbol index.
- Line semantics: joins, caps, miter/round limits, dash units relative to
  width, offset and gap width.
- The demotiles style as the acceptance target.

Deliberately not mirrored:

- MapLibre Native's C++ actor/threading architecture, tile pyramid
  implementation, GL/Metal/Vulkan/Dawn backends and render orchestrator. This
  SDK keeps its own wgpu pipeline and `VectorBaseScheduler`.
- HarfBuzz shaping, bidi and complex-script text. The SDK documents Latin-only
  support in `rustic-maps-glyphs`; full adoption does not change that without
  a separate dependency decision.
- The full glyph/sprite HTTP cache stack. The SDK keeps its provider
  transport, cache policy and same-origin fixture rule.
- Fill-extrusion, heatmap, hillshade, color-relief, terrain, sky, light,
  globe projection and video. They are parsed for diagnostics and deferred;
  the source/layer model is complete without them, their renderers are not.
- `within` (Turf-derived), `collator`/`resolved-locale`,
  `is-supported-script` and per-feature expression error reporting at the
  feature level. These are deferred with explicit diagnostics.

## 8. Licensing

Facts verified 2026-11-01:

- **MapLibre Native** is BSD-2-Clause (`LICENSE.md` in the repository). This
  design copies no C++ source and no shader text. Semantics are reimplemented
  from the public specification and the design book. No new licence file is
  required for that, and the existing "Prior art & inspiration" credit in
  `docs/THIRD_PARTY_NOTICES.md` already names MapLibre. If any file is later
  ported verbatim, the change must commit a `maplibre-native-LICENSE.md` beside
  the other licence texts under `docs/THIRD_PARTY_LICENSES/` with the
  BSD-2-Clause text, and change the notices entry to say logic was ported.
- **MapLibre style specification** is BSD-3-Clause (`LICENSE.txt` in
  `maplibre-style-spec`). The design reads the published specification. It
  does not copy the specification text or its JSON schema. If a copy of the
  specification, its schema or its expression test vectors is committed, the
  change must commit
  `docs/THIRD_PARTY_LICENSES/maplibre-style-spec-LICENSE.txt` (BSD-3-Clause)
  and add a notices entry that names the copied material.
- **MapLibre demotiles** is BSD-3-Clause, Copyright (c) 2021 MapLibre. The
  acceptance target is a copy of `https://demotiles.maplibre.org/style.json`,
  so committing it as `crates/rustic-maps-style/fixtures/styles/demotiles-style.json`
  requires:
  - `docs/THIRD_PARTY_LICENSES/maplibre-demotiles-LICENSE.txt` with the
    BSD-3-Clause text;
  - a notices entry under "Prior art & inspiration" naming the style, the
    repository and the copyright, and carrying the style's own metadata
    attribution (`This style was generated on MapTiler Cloud. Usage is
    governed by the license terms in
    https://github.com/maplibre/demotiles/blob/gh-pages/LICENSE`);
  - a data entry: vector tiles © OpenStreetMap contributors (ODbL) via the
    OSM US Tile Service, and the Natural Earth provenance of the country
    data.
  The committed fixture is test data, never a product default, and its
  source URLs are rebound to the fixture provider.
- **Fonts.** The demotiles style asks for `Open Sans Semibold`. Open Sans is
  SIL OFL-1.1 (`OFL.txt` in `googlefonts/opensans`); Noto Sans is OFL-1.1
  (`LICENSE` in `notofonts/noto-fonts`). A committed Open Sans glyph-range
  fixture would require `docs/THIRD_PARTY_LICENSES/open-sans-OFL.txt` and a
  notices entry; no Open Sans fixture is committed, so no text is owed today.
  The committed
  `fixtures/glyphs/noto_sans_regular_0-255.pbf`
  carries its SIL OFL-1.1 text at
  `docs/THIRD_PARTY_LICENSES/noto-sans-OFL-1.1.txt` with a notices entry.
- **Our code** remains MIT OR Apache-2.0. No third-party license changes the
  SDK's own license.
- `scripts/generate-third-party-notices` derives only the Cargo graph. The
  hand-maintained "Prior art & inspiration" section and
  `docs/THIRD_PARTY_LICENSES/` are the correct homes for these entries; the
  Ferrostar entry is the precedent.

## 9. Implementation order

Six parts in landing order. Each part compiles standalone, and verification
is an observable behavior or image rather than "it compiles". All six have
landed, including the icon and sprite pipeline.

### The style document, expressions and filters

Add `crates/rustic-maps-style` with the document model, CSS colors, typed
expressions, interpolation, filter normalization and legacy-function
normalization. Parse the demotiles style as a fixture. No renderer behavior
changes; the compatibility shim stays the paint path.

Files: new crate and tests; `Cargo.toml` workspace member and
`workspace.dependencies`; demotiles fixture and licence artifacts.

Verify: `cargo test -p rustic-maps-style` parses every demotiles layer with
zero unsupported properties on the eight layers it renders; expression and
filter conformance cases pass; wasm32 clippy passes for the new crate; no
existing golden changes.

### Compilation and committed styles

Rewrite `vector_style.rs` into a compiler from the document to
`CompiledStyle`. Convert the seven committed styles with the committed
converter. Implement frame-level global layer ordering in `vector_base.rs`.
Keep the legacy parser beside the new one for the parity test only.

Files: `crates/rustic-maps-rendering/src/vector_style.rs`,
`vector_base.rs`, `lib.rs`; `fixtures/render/vector-style-*.json` or a new
`fixtures/render/styles/`; `tests/legacy_style_parity.rs`; the converter
script; `platform/providers.json` style ids if their file paths move.

Verify: the parity test is structurally identical for seven styles, five
zoom levels and the committed fixture tiles; all five vector goldens pass
without regeneration; a new cross-tile test proves a later tile's fill
cannot overpaint an earlier tile's line.

### Sources

Multiple vector sources per style, TileJSON fields (`url`, `bounds`,
`scheme`, source `minzoom`/`maxzoom`, `attribution`, `promoteId`), a source
binding layer that keeps provider URLs and credentials in the provider
registration path, GeoJSON sources tiled deterministically, and style
`raster` layers routed to the existing raster scheduler.

Files: `rustic-maps-style/src/document.rs` (source specs),
`rustic-maps-rendering/src/vector_base.rs` (multi-source frame assembly),
new `src/source_binding.rs` and `src/geojson_source.rs`, provider trait
additions if a source needs a glyph or sprite URL, UDL additions for a style
document and source bindings (addition-only; regenerated bindings in the same
commit), the wasm façade and `platform/web/tests/contract_sync.mjs`.

Verify: a two-source style renders features from both sources in one frame;
a GeoJSON fixture renders to a committed golden; a style raster layer
composites over the vector base in the browser golden; every source URL in
the test resolves to a fixture, and a test fails if a request leaves the
origin.

### Line paint and layout

Real `line-join` (`miter` with `line-miter-limit`, `bevel`, `round`),
`line-cap` (`butt`, `round`, `square`), `line-dasharray` in width units,
`line-offset`, `line-gap-width`, `line-blur`, `line-sort-key`, and
per-feature data-driven color, width and opacity. Introduce the styled-line
primitive and the arc-length tessellation the dash and offset need.

**Landed.** `PolylineStroke` (cap, join, dash array, offset, gap width,
blur) rides on `RoutePolylineOverlay`; `None` keeps the renderer's default
route stroke, so every committed style and route golden renders unchanged,
and a layer that declares any line presentation property gets the styled
stroke. The assembly (`overlay.rs`) implements miter (limit 2, bevel
beyond), bevel and round joins, butt/square/round caps, dash runs with
phase carried across segments, offsets, two-band casings and a documented
blur fringe. `line-sort-key` orders features stably within a layer and
paint resolves per feature from expressions. The web canvas detects a
styled overlay with `overlays_require_camera_space_strokes` and assembles
the list at the frame's camera pose, because a dash phase and cap are
screen-space; every other overlay keeps the cached world-space path. A new
headless golden (`line_semantics_strokes.png`) pins the cap/join/dash set.

Files: `crates/rustic-maps-rendering/src/overlay.rs` (tessellation),
`style_paint.rs`, `vector_style.rs` (compile),
`crates/rustic-maps-rendering/src/headless.rs` and
`crates/rustic-maps-web-bindings/src/canvas.rs` if the fragment shader needs
a blur or dash term, plus new line goldens.

Verify: a cap/join/dash fixture golden set matches; the demotiles coastline
layer draws round caps and joins without a seam at any bend in a rendered
scene; the existing road goldens remain within tolerance.

### Fill, circle, background and data-driven paint

`fill-opacity`, `fill-outline-color`, `fill-antialias`, `fill-translate`,
`fill-sort-key`; `circle-opacity`, `circle-blur`, `circle-stroke-*`,
`circle-translate`, `circle-pitch-*`, `circle-sort-key`;
`background-opacity`; per-feature fill/circle paint from expressions; layer
sort keys for fill and circle. Transitions and cross-fade are implemented
only where the work fits; anything left moves to the residual backlog with the
reason recorded.

**Landed.** Filled polygons carry an optional edge colour painted as a
one-pixel boundary ring over the outer ring and every hole (disabled by
`fill-antialias: false`), a screen-pixel displacement applied corner by
corner, and opacity folded into the fill's alpha. Point markers carry a
stroke drawn outside the radius, a blur fringe under the marker, a screen
displacement, and `circle-pitch-scale`/`circle-pitch-alignment`: an explicit
`map` behaviour projects ground offsets so a pitched camera sees the ellipse,
and the web canvas assembles such layers at the camera pose. A layer that
declares neither pitch property keeps this renderer's screen-facing default
(the specification's default is `map`), documented on
`DEFAULT_POINT_MARKER_PITCH_SCALE`, so the cached world-space path still
serves every unchanged style. Background opacity is folded into the clear
colour. `fill-sort-key` and `circle-sort-key` order features within their
layer exactly like `line-sort-key` (stable, lower keys first). A
`fill-translate-anchor` of `map` rotates the displacement with the camera
bearing; `viewport` keeps it in screen pixels. A declared root `transition`
or per-property `*-transition` blends fill, line and circle colour and opacity
from the replaced style's resolved value over the declared duration and delay;
the scheduler drives it from the monotonic frame clock, bypasses its styled
geometry cache while blending, and applies a missing or zero-duration
transition immediately. Background and symbol cross-fade and the remaining
paint properties stay in the backlog.

Files: `overlay.rs`, `style_paint.rs`, `vector_style.rs`, golden fixtures.

Verify: a scene with `fill-opacity` over raster tiles, a fill outline, a
circle with a stroke and a data-driven country fill from a `match`
expression renders to committed goldens; the existing fill and marker
goldens stay within tolerance.

### Symbol layout, placement and icons

**Landed so far.** Text resolution (transform, tokens, `format`), `text-offset`,
`text-anchor`, `text-rotation-alignment`/`text-keep-upright`, wrapping with
`text-line-height`/`text-letter-spacing`/`text-justify`, `text-variable-anchor`
with `text-radial-offset`, `text-max-angle`, `line-center`, the overlap rules,
a declared `symbol-spacing` and a persistent collision index (the placed names
are carried into the next frame's pass, so a pan holds them in place instead of
letting newcomers take their space); the MapLibre demotiles style now renders
end to end over sixteen committed z2 tiles (`maplibre_demotiles_zoom2.png`);
the icon/sprite pipeline followed: the `sprite` declaration compiles, the
`icon-*` properties compile with spec defaults, a MapLibre sprite index and
RGBA atlas load through an injected `SpriteSource`, and icons draw through the
textured tile pipeline with text and icon collision boxes kept separate.
Remaining (named in `PLAN.md` §2): `icon-halo-*`, `icon-text-fit`,
`icon-optional`, `icon-keep-upright`, SDF-icon tinting and line-placed icons.

`text-field` expressions and `format`, `{token}` fallback, `text-transform`,
`text-max-width` wrapping, `text-line-height`, `text-letter-spacing`,
`text-justify`, `text-anchor`/`text-offset`/`text-radial-offset`,
`text-rotation-alignment`, `text-keep-upright`, `text-max-angle`,
`symbol-spacing`, `symbol-placement: line-center`, `symbol-sort-key`,
`symbol-avoid-edges`, the overlap/ignore-placement/optional matrix, font
stack selection from the style's `text-font` and the cross-tile symbol index;
then the icon pipeline with a sprite atlas
(`icon-image`, `icon-size`, `icon-rotate`, `icon-anchor`, `icon-offset`,
`icon-halo-*`, `icon-text-fit` deferred if necessary).

Files: `crates/rustic-maps-rendering/src/labels.rs` (layout and placement),
`crates/rustic-maps-rendering/src/sprite.rs`,
`crates/rustic-maps-glyphs` (font stack cache),
`crates/rustic-maps-web-bindings/src/canvas.rs` (sprite and text draws),
`rustic-maps-style` (`text-field` expressions), new glyph and sprite
fixtures with OFL licence texts, the demotiles end-to-end scene.

Verify: the demotiles style renders a world frame with country fills from
its `match` expression, dashed geolines, line-placed country labels and
wrapped text, pinned as `world_zoom2_demotiles.png`; a road-name scene keeps
its existing golden within tolerance; a collision test proves a lower
`symbol-sort-key` wins and an `ignore-placement` symbol does not block.

The symbol work is the largest and may need two passes, split at the icon
boundary into text/placement first and icons second; the other five parts are
each one pass.

## 10. Residual backlog

These are real parts of the specification that this plan deliberately leaves
open. They belong in the backlog, not in a silent omission:

- heatmap, fill-extrusion, hillshade, color-relief and raster-dem layer
  rendering, terrain, sky, light and globe projection;
- line-pattern, fill-pattern, background-pattern, line-gradient and the
  pattern atlas;
- feature-state and global-state driven styling;
- data-driven layout properties, variable anchors and variable-anchor offsets;
- per-layer blend modes and full transition/cross-fade semantics;
- complex-script shaping, bidi and font fallback beyond one font stack;
- `within`, `collator`, `resolved-locale`, `is-supported-script`,
  `number-format` and `distance`;
- video and image sources;
- a style-diff/incremental update API equivalent to MapLibre's
  `setStyle` diffing.

## 11. What could not be verified, and open questions

- The exact default color interpolation space and whether alpha is
  premultiplied during interpolation. The specification text does not settle
  it; the upstream expression test suite does. The style-document work must settle it
  with conformance cases before the first golden.
- Legacy filter coercion rules for mixed string/number comparisons. The
  `deprecations` page describes the function model but not the coercion
  table; the conformance suite must define it.
- Whether MapLibre Native evaluates line width at the frame's fractional
  zoom or at the tile's integer zoom. The published design book does not say,
  and the C++ property evaluator files were not read line by line. This
  decides whether the finer cache key in §5.5 is parity or an enhancement.
- ~~Whether `fill-sort-key` and `line-sort-key` paint higher keys above or
  below.~~ Resolved by following the layers page: ascending paint order, so a
  higher key paints later and appears above a lower one, for line, fill and
  circle alike.
- The exact demotiles font payload's license basis. The repository's
  `LICENSE` is BSD-3-Clause and the font directory carries no per-font
  licence file; Open Sans upstream is OFL-1.1, but the served PBF's terms are
  not stated on the page.
- The current MapLibre Native release tag. The README references Android
  artifact 11.11.0; no release tag was verified.
- Whether the existing Noto Sans glyph fixture is distributed under OFL-1.1
  with the required notice. Provenance is recorded; the licence text is not
  in `docs/THIRD_PARTY_LICENSES/`.
- The performance cost of per-feature paint resolution and re-tessellation at
  the finer zoom key on mobile GPUs. The compilation and line work must measure it
  against the existing frame-latency test
  (`crates/rustic-maps-rendering/tests/frame_latency.rs`) before the key is
  final.
- The exact UDL shape for a style document and source bindings. This design
  requires an addition-only contract change with regenerated Kotlin, Swift
  and React Native bindings and a synced wasm façade, but the naming belongs
  with the sources work.

## Appendix A — Road hierarchy, label placement and vector-pipeline behaviour

These rules were refined after the initial style adoption and are part of the
current behaviour. The committed artifacts carry them as `minzoom` values and
`interpolate` width stops; the renderer resolves them per frame.

### A.1 Road hierarchy

Every road layer declares a per-class minimum zoom and a width curve. The
committed styles evaluate to this target, in screen pixels:

| Class | Min zoom | Width at its first zoom | Width at z16 | Casing |
|---|---|---|---|---|
| motorway, trunk (and links) | 4 | 0.9 | 6.3 | yes, ~1.4 px over the fill, same stops |
| primary (and links) | 7 | 0.7 | 3.5 | no |
| secondary (and links) | 9 | 0.6 | 2.2 | no |
| tertiary (and links) | 11 | 0.5 | 1.5 | no |
| residential, unclassified, living street | 12 | 0.4 | 1.2 | no |
| service | 15 | 0.4 | 0.9 | no |
| pedestrian | 14 | 0.5 | 1.2 | no |

Links paint 0.8 of their parent class. Stops run at the class's first zoom and
then at 8, 10, 12, 14, 16 and 18, so interpolation is smooth. At zoom 9 only
motorway, trunk, primary and secondary paint; widths are the acceptance values
below. The non-Carto styles (Bright, Fiord, Positron, OpenMapTiles) use the
same mechanism adapted to their schema's class names, anchored on each layer's
existing city-zoom width so the z14 goldens render unchanged.

Carto light evaluates as:

| Class | Min zoom | Width at z9 | Width at z16 | Casing |
|---|---|---|---|---|
| motorway | 4 | 2.45 (casing 3.20) | 6.30 (casing 7.70) | yes |
| trunk | 4 | 2.10 (casing 2.85) | 5.30 (casing 6.70) | yes |
| primary | 7 | 1.00 | 3.50 | no |
| secondary | 9 | 0.60 | 2.20 | no |
| tertiary | 11 | hidden | 1.50 | no |
| residential, unclassified, living street | 12 | hidden | 1.20 | no |
| service | 15 | hidden | 0.57 | no |
| pedestrian | 14 | hidden | 0.85 | no |

Unit tests assert `width_pixels_at_zoom` and `is_visible_at_zoom` per class for
every artifact. The regional frame is pinned by
`sacramento_region_zoom9_carto_light.png` (native) and
`sacramento_region_zoom9_carto_light_browser.png`; the browser golden compares
within a mean absolute channel error of 2.0 and an outlier fraction of 0.02.

### A.2 Label placement

Every drawn glyph contributes one collision box measured from the packed
atlas' bearings and advances, padded by `LABEL_COLLISION_PADDING_PIXELS` (2 px)
on every side. A line-placed name keeps the segment rotation on each box; two
boxes are tested with the separating-axis theorem.

Candidates are placed in this order, first key first:

1. **Symbol class** (the style's `symbol-sort-key`): route shields class 0,
   place names class 1, motorway-to-tertiary road names class 2, residential
   and unclassified road names class 3.
2. **Feature rank** (the feature's `rank`, lower first; unranked last within
   its class).
3. **Style-layer order** (the declared class/zoom tier).
4. **The previous frame's placement**, only among candidates that tie on the
   keys above. The memory never reorders across class, rank or tier, so a
   remembered name cannot override a more important newcomer.
5. **The caller's candidate order**, so the frame's tile list stays
   deterministic.

Before ordering, a ranked label survives only when its rank is at most the
frame zoom's band ceiling (`LABEL_REGIONAL_THINNING_BANDS`):

| Frame zoom | Highest rank kept |
|---|---|
| below 8 | 6 |
| 8 to below 10 | 18 |
| 10 to below 12 | 20 |
| 12 to below 14 | 22 |
| 14 and above | every rank |

Unranked labels are exempt, so road names and shields never thin. The style's
own zoom-stepped rank ceilings and per-class minimum zooms apply first and
stay authoritative where they are tighter. Names repeat along a long run,
spaced at least 90 px and rotated to their segment.

Typography follows the committed Bright style unchanged: place text 10/13/18 px
at z10/z14/z18, POI 10/14 px at z14/z18, road text 10–18 px by class, halos
1.5 px (places and POI) and 1.3 px (roads). The renderer's SDF coverage
threshold is 0.75, the edge half-band 0.105 field units, collision padding 2 px,
line height 1.2 em and letter spacing 0. The one approximation is baseline
placement at 0.3 em below the visual centre; the shift stays under one pixel
at Bright's sizes.

### A.3 Vector fetching and drawing

- **Persistent cache.** Vector payloads share the installed SQLite tile cache
  at the 30-day vector TTL, plus a TTL'd in-memory LRU. Native caching needs the
  tile cache installed before the source.
- **Prefetch.** After a frame settles, prefetch same-zoom neighbours and coarse
  parents, parents first, budget 32 tiles, at most one plan per 500 ms,
  detached and cancellable.
- **Progressive draw.** A missing child tile is covered by its nearest cached
  ancestor; the frame never blocks on a fetch.
- **Motion grading.** While a camera gesture is active, every frame paints the
  decoded tiles and the last settled style-zoom bucket's geometry and skips
  label assembly. The first still frame resolves the style bucket and labels at
  the final pose. The browser reports the gesture through
  `setCameraGestureActive`; the native surface reads the camera pose each draw.
  Measured at the browser frame-budget scene, `renderFrame` p50 fell from
  8.0–8.1 ms ungraded to 1.4–1.8 ms graded.

The regression fixtures are the z9 Carto golden, the dense regional label
golden (`sacramento_region_zoom13_labels.png`), the wide thinning golden
(`sacramento_region_zoom8_labels.png`) and the browser frame-budget scene in
`docs/Browser.md`.
