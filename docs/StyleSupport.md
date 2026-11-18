# Style support

The SDK adopts the **full source/layer model** of the MapLibre style specification, not
a curated subset. The renderer keeps its own wgpu pipeline; the style document, its
expressions and its evaluation semantics are the SDK's style surface on all four
targets. This page records what ships today, the target design, the remaining gaps, what
is mirrored from MapLibre and the licensing position.

## 1. What this document decides

- A style is a **MapLibre style document** (version 8): `sources`, `layers`, `glyphs`,
  `sprite` and the root properties. The data-only artifact format becomes a
  compatibility shim for one migration step.
- Styling compiles into **render plans** per layer: a geometry kind, a filter
  expression, paint resolved per feature at a zoom, and layout resolved for the symbol
  pass.
- Expressions are evaluated by one provider-neutral, wasm-safe crate. Legacy `{stops,
  type, base, property, default}` functions normalize into expressions at parse time.
- The renderer gains real line joins and caps, dashes, line offsets and gap widths, fill
  outlines, circle strokes, per-feature data-driven paint and true text/icon layout.
- Layer order is **global across tiles**, not per tile. Page overlays stay above every
  style layer, matching the existing draw contract and MapLibre's annotation model.

## 2. Verified inventory of what ships today

### 2.1 The shipped style model

The model is in `crates/rustic-maps-rendering/src/vector_style.rs`. `VectorTileStyle`
holds an optional flat `background` RGBA and a `Vec<VectorLayerStyle>`. Each layer has:

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

The seven committed artifacts are JSON files under
`crates/rustic-maps-rendering/fixtures/render/vector-style*.json`. Their schema is the
artifact schema, not the style specification: top-level `background` and `layers`, layer
fields such as `geometry_kind`, `rgba`, `width_pixels` and `filter: {attribute_name,
expected_text, matches_any, number_equals}`. `VectorStyleArtifact` and
`vector_style_from_json` use `#[serde(deny_unknown_fields)]`, pinned by
`deny_unknown_fields keeps the upstream expression surface out`. The in-code fallback
`in_code_default_vector_tile_style()` duplicates the Carto light artifact value for
value.

Width and size scaling:

- `width_pixels_at_zoom` and `text_size_pixels_at_zoom` linearly interpolate between
  ascending stops and clamp outside the stop range.
- The vector base scheduler evaluates the style at the camera zoom quantized to four
  buckets per level (`VECTOR_BASE_STYLE_ZOOM_QUANTIZATION_STEPS_PER_LEVEL = 4.0`), so a
  continuous zoom changes widths and sizes in quarter-level steps.
- Coordinates convert from tile-local extent space through the tile's web-mercator
  address in `tile_local_point_to_coordinate`; paint values are per layer, never per
  feature.
- `overlay_primitives_for_vector_tile_at_zoom` groups overlays into one
  `VectorOverlayBatch` per style layer, but `cached_frame` flattens and concatenates
  them **per tile**. There is no global layer pass.

### 2.2 Renderer capabilities

Geometry primitives (`crates/rustic-maps-rendering/src/overlay.rs`):

- `RoutePolylineOverlay`: screen-space-thickened segment quads. The styled stroke
  (`PolylineStroke`: cap, join, dash array, offset, gap width, blur) rides on it, so a
  layer that declares any line presentation property gets the styled stroke and every
  other overlay keeps the default route stroke. `LINE_MITER_LIMIT` is 2; sharper corners
  fall back to bevel. The web canvas detects such a layer with
  `overlays_require_camera_space_strokes` and assembles it at the frame's camera pose,
  because a dash phase and cap are screen-space.
- `FilledPolygonOverlay`: ear-clip triangulation with holes bridge-spliced into the
  outer ring, plus an optional one-pixel boundary ring over the outer ring and every
  hole, disabled by `fill-antialias: false`.
- `PointMarkersOverlay`: screen-facing octagons with an optional stroke drawn outside
  the radius, a blur fringe, a screen displacement and
  `circle-pitch-scale`/`circle-pitch-alignment`. A layer that declares neither pitch
  property keeps this renderer's screen-facing default, documented on
  `DEFAULT_POINT_MARKER_PITCH_SCALE`.
- `PointPinsOverlay`, `TrafficFlowOverlay`: SDK annotations, not style primitives.
- `WorldSpaceOverlayGeometry::tessellate` caches a camera-independent triangle list;
  screen-space widths and radii are baked into it, so a width change requires
  re-tessellation.

Draw order (`crates/rustic-maps-rendering/src/headless.rs` and the browser canvas): tile
quads, one solid-color overlay pass (vector base and page overlays concatenated), the
label pass (halo then text), the location indicator. The overlay pass has alpha blending
but no per-layer opacity or blend mode. A style's `sprite` compiles to a typed spec,
`icon-image` and its sibling icon properties compile with spec defaults, and a MapLibre
sprite index plus RGBA atlas load through an injected `SpriteSource`.

Label and glyph pipeline (`crates/rustic-maps-rendering/src/labels.rs`;
`crates/rustic-maps-glyphs`):

- Per tile, `label_candidates_for_vector_tile` reads symbol layers into candidates. Text
  is the first non-empty attribute from the fallbacks; `text-field` expressions,
  `format` and `{name}` tokens sit on the style path.
- Per frame, `LabelLayer::place` sorts globally by `(priority, style_order, rank)`,
  rejects padded axis-aligned rectangles greedily, drops repeated text, and caps the
  frame at 32 labels. Point labels center on the anchor. Line labels take the longest
  straight projected run with a fixed 20-degree tolerance, repeat every `text width + 90
  px`, and always read upright.
- Glyphs come from one embedded Latin Noto Sans fixture (`default_label_font`); there is
  no shaping, bidi or complex-script support by design.
- Collision state carries the placed names into the next frame's pass, so a pan holds
  them in place; the cross-tile symbol index is absent.

Style and source surfaces:

- `VectorBaseScheduler` (`crates/rustic-maps-rendering/src/vector_base.rs`) owns exactly
  one vector tileset through `VectorTileProvider` plus a URL template from a provider
  descriptor. There is no TileJSON resolution, no `bounds`, `scheme`, `promoteId`,
  `volatile`, `minzoom`/`maxzoom` source range, and no second source.
- The public surfaces take a **style id string**, not JSON:
  `UrlTemplateVectorTileSource.style_id` in
  `crates/rustic-maps-native-core/src/rustic_maps.udl`, and the
  `vectorTileOverlayDescriptorsForStyle` wasm export in
  `crates/rustic-maps-web-bindings/src/vector_layer.rs`. Seven cartographic ids exist
  (`carto-light`, `carto-dark`, `openmaptiles-light`, `openmaptiles-dark`,
  `bright-light`, `fiord-dark`, `positron-light`). An eighth, `label-coverage-probe`, is
  a measurement fixture: one literal white `I` at 48 point pixels on black, used by the
  browser SDF edge probe.
- `platform/providers.json` names each provider's tile URL, attribution, fixture routes
  and style ids. Provider URLs never enter the renderer.
- Background is a flat clear color. Mercator is the only projection.

### 2.3 Acceptance fixtures today

Goldens that keep their meaning through the migration:
`sacramento_zoom14_bright_vector_base.png`,
`sacramento_zoom14_openmaptiles_light_vector_base.png`,
`sacramento_region_zoom9_carto_light.png`, `sacramento_zoom14_place_labels.png`,
`sacramento_zoom14_road_names.png`, `line_semantics_strokes.png` (the line cap/join/dash
set), `maplibre_demotiles_zoom2.png` and `world_zoom2_demotiles.png` (the demotiles
scene), and the raster goldens that compose with the vector base.

The harness tolerates GPU sampler differences (mean absolute error 2.0, outlier channel
error 24.0 over at most 2 percent of pixels). Missing goldens are written only with
`UPDATE_GOLDENS=1`. The vector fixtures are committed MVT tiles under
`fixtures/openfreemap` and `fixtures/openstreetmap-us`; tests never touch the network.

## 3. Upstream reference material

The design relies on these pages:

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

The demotiles style is the concrete target: it uses legacy `{"stops": ...}` functions
for `line-width`, `line-opacity`, `text-size`, `text-field` and `text-transform`; a
`match` expression for country fills; `{name}` token text fields; `line-dasharray`;
round joins and caps; `text-max-width`; and a GeoJSON source beside a vector source. A
parser that accepts only modern expressions cannot load it.

The Native repository confirms the semantic vocabulary this design mirrors: style layers
compile into tile buckets; paint and layout properties resolve through property
evaluators; symbol placement and collision live in `src/mln/text`; tile scheduling lives
in `renderer/tile_pyramid`.

## 4. Gap analysis

Status values: **SUPPORTED** (a style property works today), **PARTIAL** (some cases
work, with a different or narrower model), **ABSENT**. The responsible file is the one
that would have to change.

### 4.1 Document, root and sources

| Feature | Status | Responsibility |
|---|---|---|
| `version: 8` document root; `name`, `metadata` | ABSENT | new `rustic-maps-style/src/document.rs` |
| root `center`/`zoom`/`bearing`/`pitch` | ABSENT; the SDK camera owns the view, so parse and ignore | same |
| `projection` mercator | SUPPORTED as renderer behavior, not style-controlled | renderer |
| `projection` globe / vertical-perspective | ABSENT | `rustic-maps-rendering/src/projection.rs` |
| `sprite` | ABSENT; the sprite atlas loads outside the document | new sprite module + canvas/headless pipelines |
| `glyphs` URL | PARTIAL: provider descriptor only | `vector_base.rs`, provider traits |
| `transition`, per-property `*-transition`, cross-fade | PARTIAL: root and per-property timings blend fill, line and circle colour and opacity | render plans + frame clock |
| `light`, `sky`, `terrain`, `state` | ABSENT | deferred |
| one vector source (tiles in a URL template) | PARTIAL | `vector_base.rs` |
| TileJSON `url`, `bounds`, `scheme`, `attribution`, `minzoom`/`maxzoom`, `volatile`, `encoding` | ABSENT | source resolver |
| multiple sources in one style; GeoJSON source | ABSENT | `vector_base.rs`, frame assembly; new source implementation |
| `promoteId` and feature ids | PARTIAL: the decoder keeps `identifier` | style crate + feature-state |
| image / video / raster-dem source | ABSENT | deferred |
| raster / hillshade / color-relief layers inside a style | ABSENT (the raster path exists outside styles) | style compile + raster scheduler |

### 4.2 Layer common model

| Feature | Status | Responsibility |
|---|---|---|
| `id` and layer identity | ABSENT (index order only) | style crate |
| `source` binding plus `source-layer` | PARTIAL (`source_layer` only) | style crate + scheduler |
| `minzoom` / `maxzoom` | SUPPORTED | `vector_style.rs` |
| `visibility` | ABSENT | style crate |
| filter expressions (`all`, `any`, `!`, `==`, `!=`, `<`, `<=`, `>`, `>=`, `in`, `has`, `$type`) | PARTIAL: one attribute equality | new `filter.rs` |
| filter zoom restrictions | ABSENT | expression analyzer |
| `line-sort-key`, `symbol-sort-key` | SUPPORTED | render plan ordering |
| `fill-sort-key`, `circle-sort-key` | SUPPORTED (stable per-layer feature order) | `vector_style.rs` `overlays_for_layer` |
| global layer order across tiles | ABSENT (per-tile concatenation) | `vector_base.rs` frame assembly |
| data-driven paint (per feature) | SUPPORTED (resolved per feature from expressions) | render plan + tessellation |
| legacy functions (`stops`, `base`, `type`, `property`, `default`) | PARTIAL: linear zoom stops on width/size only | new `legacy_function.rs` |
| `step` | PARTIAL: rank ceilings only | expression engine |
| `interpolate` linear | PARTIAL: width and text size only | expression engine |
| `interpolate` exponential / cubic-bezier; `interpolate-hcl` / `interpolate-lab` | ABSENT | expression engine |
| `let` / `var` / `coalesce` / `case` / `match`; math, string, color and lookup operators; type checking and default-on-failure | ABSENT | expression engine |
| layout evaluated at integer zoom | ABSENT (uses fractional) | render plan evaluation |
| paint evaluated continuously at frame zoom | PARTIAL (quarter-level bucket) | scheduler cache key |
| `feature-state`, `global-state`, `heatmap-density`, `line-progress` | ABSENT | deferred |

### 4.3 Fill

| Feature | Status | Responsibility |
|---|---|---|
| `fill-color` | SUPPORTED (constant and expression) | `vector_style.rs` |
| `fill-opacity` | SUPPORTED | render plan |
| `fill-outline-color` | SUPPORTED (one-pixel boundary ring; absent paints none) | `overlay.rs` `emit_filled_polygon_outline` |
| `fill-antialias` | SUPPORTED (`false` paints no outline) | overlay pipeline |
| `fill-translate` | SUPPORTED (screen pixels) | overlay assembly |
| `fill-translate-anchor` | SUPPORTED (`map` rotates the displacement with the camera bearing; `viewport` keeps screen pixels) | overlay assembly |
| `fill-pattern` | ABSENT | pattern atlas |
| `fill-sort-key` | SUPPORTED (stable per-layer feature order) | `vector_style.rs` `overlays_for_layer` |
| `fill-layer-opacity` / `fill-layer-blend` | ABSENT | draw passes |

### 4.4 Line

| Feature | Status | Responsibility |
|---|---|---|
| `line-color` | SUPPORTED (constant) | `vector_style.rs` |
| `line-width` | PARTIAL (constant + linear stops) | render plan + tessellation |
| `line-opacity` | PARTIAL (alpha channel) | render plan |
| `line-join` (`miter` default, `bevel`, `round`) | SUPPORTED | `overlay.rs` `push_stroke_join` |
| `line-cap` (`butt` default, `round`, `square`) | SUPPORTED | `overlay.rs` `push_stroke_cap` |
| the miter limit (`line-miter-limit`, fixed at 2 by the specification) | SUPPORTED (sharper corners fall back to bevel) | `overlay.rs` `LINE_MITER_LIMIT` |
| `line-dasharray` | SUPPORTED (on/off runs in line-width units, phase across segments) | `overlay.rs` `dash_on_runs` |
| `line-offset` / `line-gap-width` | SUPPORTED (perpendicular offset; two casing bands outside the gap) | `overlay.rs` `stroke_bands` |
| `line-blur` | PARTIAL (a wider, fainter fringe painted under the core stroke) | `overlay.rs` `LINE_BLUR_FRINGE_ALPHA_SCALE` |
| `line-sort-key` | SUPPORTED (stable per-layer feature order) | `vector_style.rs` `overlays_for_layer` |
| `line-pattern` / `line-gradient` | ABSENT | deferred |
| data-driven width/color/opacity | SUPPORTED (resolved per feature from expressions) | `vector_style.rs` |

### 4.5 Circle and background

| Feature | Status | Responsibility |
|---|---|---|
| `circle-radius`, `circle-color` | SUPPORTED (constant and expression) | `vector_style.rs` |
| `circle-opacity` | SUPPORTED | render plan |
| `circle-blur` | PARTIAL (a wider, fainter fringe under the marker) | `overlay.rs` |
| `circle-stroke-width/color/opacity` | SUPPORTED (stroke drawn outside the radius) | marker pipeline |
| `circle-translate` | SUPPORTED (screen pixels) | marker pipeline |
| `circle-pitch-scale`, `circle-pitch-alignment` | SUPPORTED (`map` behaviour assembles in camera space; an omitted value keeps this renderer's screen-facing default) | marker pipeline |
| `circle-sort-key` | SUPPORTED (stable per-layer feature order) | `vector_style.rs` `overlays_for_layer` |
| `background-color` | SUPPORTED (flat clear color) | `vector_base.rs` |
| `background-opacity` | SUPPORTED (folded into the clear color's alpha) | clear color path |
| `background-pattern` | ABSENT | pattern atlas |

### 4.6 Symbol and text

| Feature | Status | Responsibility |
|---|---|---|
| `symbol-placement` `point` / `line` | PARTIAL | `labels.rs` |
| `symbol-placement` `line-center` | SUPPORTED (one placement at the run midpoint) | `labels.rs` |
| `symbol-spacing` | SUPPORTED (the declared value; the renderer keeps a 90 px default for a layer that omits it) | `labels.rs` |
| `symbol-avoid-edges` | SUPPORTED (a label whose padded box would cross the viewport edge is dropped) | placement |
| `symbol-sort-key` | SUPPORTED (mapped to the placement priority) | placement |
| `symbol-z-order` | ABSENT (draw order follows placement order) | placement |
| `text-field` expression / `format` / `{token}` | SUPPORTED (expressions; `format` flattened to its text; legacy `{property}` tokens substituted) | `labels.rs`, style crate |
| `text-font` | PARTIAL: parsed, ignored | glyph font selection |
| `text-size` | PARTIAL: linear stops, fractional zoom | render plan |
| `text-max-width` (wrapping) | SUPPORTED (greedy word wrapping; line-placed names stay on one line) | `labels.rs` |
| `text-line-height`, `text-letter-spacing`, `text-justify`, `text-transform` | SUPPORTED (`text-justify` centers each line for `auto`/`center` and aligns the block for `left`/`right`; transform is none, uppercase or lowercase) | glyph layout, style resolution |
| `text-offset`, `text-anchor` | SUPPORTED (a screen-pixel displacement from the anchor; all nine anchor positions, ignored by line-placed names) | placement |
| `text-radial-offset`, `text-variable-anchor` | SUPPORTED (anchors tried in order, the first non-colliding one kept; the radial offset pushes the block outward in ems) | placement |
| `text-rotation-alignment`, `text-keep-upright`, `text-max-angle` | SUPPORTED (map or viewport rotation for point labels, `auto` by placement; keep-upright flips past a quarter turn; a declared angle sets the run tolerance and the renderer keeps its 20-degree default) | placement |
| `text-padding` | PARTIAL: fixed 2 px | placement |
| `text-allow-overlap`, `text-ignore-placement` | SUPPORTED (both skip collision rejection) | placement |
| `text-overlap`, `text-optional` | ABSENT | placement |
| `text-color`, `text-halo-color`, `text-halo-width` | SUPPORTED | labels |
| `text-halo-blur`, `text-opacity`, `text-translate` | ABSENT | glyph shader |
| icon properties and sprite atlas | PARTIAL: the `sprite` declaration and `icon-image`, `icon-size`, `icon-rotate`, `icon-anchor`, `icon-offset` and the other `icon-*` properties compile with spec defaults and draw through the textured tile pipeline; `icon-halo-*`, `icon-text-fit`, `icon-optional`, `icon-keep-upright`, SDF tinting and line-placed icons remain | icon pipeline |
| persistent collision index and cross-tile symbol index | PARTIAL: placements persist across frames, greedy per frame; the cross-tile index is absent | placement |
| complex-script shaping, bidi | ABSENT by design | deferred |

#### Route-reference badges

A symbol layer may declare a layer-level `shield` block. The block is additive: a layer
without one keeps every existing field and paints no badge. Its members are `shape`
(`shield` for the two-lobed highway badge or `rounded_rectangle`; omitted draws a
rounded rectangle), `fill_rgba`, an optional `border_rgba` with `border_width_pixels`,
`text_rgba`, `text_size_pixels`, `padding_pixels` and `minimum_size_pixels`. A layer
with a badge reads its label from the tile's route-reference attribute `ref` through the
shield path and never consults `text-field`; the reference draws centred in the badge in
the block's own colour and size. The badge tessellates in screen space around the
measured glyph run plus the padding, grows past `minimum_size_pixels` for a long
reference, and emits into the label layer's badge pass beneath its own text. A reference
whose glyphs the atlas cannot serve is skipped rather than painted as a placeholder.

The OpenMapTiles schema and the OpenFreeMap styles built on it (Bright, Fiord and
Positron) carry no `routes` source layer: their road references live on
`transportation_name`, which also carries the names, so those documents declare no
route-reference layer. The Carto light and dark documents carry a `routes` layer and
declare the badge block. The committed OpenStreetMap US z9 tile is the only fixture with
road and transit refs, so the proof is the regional Carto frame.
`sacramento_region_zoom9_carto_light.png` pins it, the native styled-base suite asserts
both a road ref and a transit ref place as badges, and
`sacramento_region_zoom9_carto_light_badges.bin` pins the badge vertex stream
byte-for-byte with its exact vertex count and clip-space extent.

### 4.7 Layer types outside the current model

| Feature | Status | Responsibility |
|---|---|---|
| `heatmap`, `fill-extrusion` | ABSENT | deferred |
| `raster`, `hillshade`, `color-relief` as style layers | ABSENT | style compile + raster path |

## 5. Target design

### 5.1 Crate layout

Add one crate, `crates/rustic-maps-style/`, and keep the renderer free of document
parsing.

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

The crate stays separate because `rustic-maps-vector-tiles` is the MVT decoder and must
stay data-only; the style model is renderer-facing but GPU-free, wasm-safe and usable by
tests that do not link wgpu; and the renderer depends on it in one direction, so no
provider crate is aware of it. The renderer keeps `vector_style.rs` as the **compiler**
for one migration step and gains `compile_style_document(document, source_bindings) ->
Result<CompiledStyle, StyleError>`. `CompiledStyle` holds ordered `CompiledLayer`
values: kind, source id, source layer, `FilterExpression`, resolved paint descriptors,
layout descriptors and sort key. Paint resolves per feature at a zoom and symbol layout
at integer zoom, keeping those seams in `vector_style.rs`, `labels.rs` and `overlay.rs`.

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

Only implemented properties get typed fields. Every other property the parser recognizes
is accepted, recorded in `unsupported`/diagnostics with its layer id, and does not fail
the document, so a Bright style with an unimplemented `icon-image` layer still draws its
roads and the diagnostics name what is missing. MapLibre rejects unknown properties;
this SDK deliberately degrades so one style layer never blanks the base map.

Property defaults live in `defaults.rs` and mirror the specification (`fill-color`
`#000000`, `line-width` 1, `line-cap` `butt`, `line-join` `miter`, `text-size` 16,
`text-max-width` 10 ems, `text-line-height` 1.2, `text-padding` 2, `symbol-spacing` 250,
`circle-radius` 5). No literal defaults are scattered in the evaluator.

### 5.3 Expressions

`Expression` is a typed tree. The parser accepts the JSON array form and the legacy
function object form through `legacy_function.rs`, so both normalize into the same tree.
The operator set, in landing order:

1. **Core**: `literal`, `get`, `has`, `geometry-type`, `id`, `properties`, `zoom`,
   `let`, `var`, `all`, `any`, `!`, `==`, `!=`, `<`, `<=`, `>`, `>=`, `case`, `match`,
   `coalesce`, `step`, `interpolate`, `to-string`, `to-number`, `to-boolean`,
   `to-color`, `rgb`, `rgba`, `concat`, `upcase`, `downcase`, `length`, `at`, `in`,
   `index-of`, `slice`, `+`, `-`, `*`, `/`, `%`, `^`, `min`, `max`, `round`, `floor`,
   `ceil`, `abs`, `sqrt`, `ln`, `log10`, `log2`, `sin`, `cos`, `tan`, `asin`, `acos`,
   `atan`, `pi`, `e`, `ln2`.
2. **Text**: `format` and the token form `"{name}"` for `text-field`, then
   `collator`/`resolved-locale` only if a real localization need appears.
3. **State**: `feature-state`, `global-state`, `heatmap-density`, `line-progress`,
   `accumulated`, `within`, `distance`, `is-supported-script` and `number-format` are
   parsed but return `null` or a diagnostic until the owning work lands.

Every expression is analyzed at parse time into one of four kinds: `Constant`, `Camera`,
`DataDriven`, `CameraAndData`. The analysis enforces the specification's restrictions:
in a layout or paint property, `["zoom"]` may only be the direct input of a top-level
`step` or `interpolate`, optionally wrapped in `let`; a filter must be a data expression
returning a boolean. A violation is a parse diagnostic that disables the property, never
a panic.

### 5.4 Filters

Filters are expressions returning boolean. Both spellings are accepted: modern `["==",
["get", "class"], "motorway"]` and legacy `["all", ["==", "class", "motorway"], ["!=",
"class", "rail"]]`. The legacy array filter is normalized by rewriting bare strings used
as input into `["get", name]` and `$type` into `["geometry-type"]`; the normalized tree
is the same `Expression` used by paint. The existing `VectorAttributeFilter` and
`feature_matches_filter` are retired after the compatibility step.

Coercion follows the specification, not Rust's `PartialEq`: `==` compares numbers
numerically and strings lexically, and a mixed comparison is false unless a conversion
operator produced the value. Property values come from the decoder's
`VectorTileAttributeValue` variants (text, float, double, signed, unsigned, zigzag
signed, boolean).

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

- **Paint** camera expressions resolve at the frame's fractional zoom, so a width or
  size changes continuously; layout camera expressions resolve at the integer zoom the
  specification requires.
- `step` returns the output of the greatest stop at or below the input; below the first
  stop it returns the first output.
- `interpolate` clamps outside the stop range and blends between adjacent stops.
  `["linear"]` uses the fraction directly. `["exponential", base]` uses `(base^(z -
  z0) - 1) / (base^(z1 - z0) - 1)`, with `base == 1` reduced to linear.
  `["cubic-bezier", x1, y1, x2, y2]` solves the x curve for the fraction then evaluates
  the y curve. Stops must be strictly ascending numeric literals, validated at parse
  time.
- Colors interpolate per channel in the default space; `interpolate-hcl` and
  `interpolate-lab` are separate operators.
- A property expression that fails at runtime (type error, missing property under a
  non-`coalesce` path, unsupported operator) falls back to the property's default and
  emits one diagnostic per layer and property, not per feature, so a dense tile cannot
  flood the log.
- `minzoom`/`maxzoom`: visible when `minzoom <= zoom < maxzoom`.

The scheduler's quarter-level style bucket is replaced by a cache key that includes a
hash of the resolved style revision plus the resolved paint quantized to a named
constant (initially 1/16 zoom). Geometry is re-tessellated when that key changes. If
measurement shows per-frame re-tessellation dominates, the design permits a fixed bucket
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

- **Fill**: resolve `fill-color` and `fill-opacity` per feature; emit the hole-aware
  `FilledPolygonOverlay` with the resolved RGBA. `fill-outline` emits a boundary
  polyline with the resolved color immediately after the fill in the same layer.
- **Line**: resolve `line-color`, `line-width`, `line-opacity`, `line-offset`,
  `line-gap-width`, `line-blur`, `line-join`, `line-cap` and `line-dasharray` per
  feature into a styled-line primitive that carries the resolved width, color and
  join/cap/dash parameters. One primitive per feature, so data-driven values survive.
- **Circle**: resolve radius, color, opacity, blur, stroke width and stroke color per
  feature; emit one styled marker primitive per feature.
- **Symbol**: resolve text and icon fields, size, font, transform, wrapping, spacing,
  angle, anchor and overlap rules per feature into candidates; the frame-level placement
  module consumes them.
- **Background**: the clear color; pattern and opacity later.

The existing `RoutePolylineOverlay` remains the page-overlay type for routes and
traffic. Style lines get their own primitive so a style change cannot accidentally gain
route defaults, and so the renderer can add dash and join data without widening the
annotation API.

### 5.7 Layer order and composition with page overlays

Frame assembly changes from "per tile, flatten all layers" to:

1. Collect `Vec<StyledLayerBatch>` per tile into a frame-level map keyed by the layer's
   document position.
2. Flatten in document order: every tile's layer 0, then every tile's layer 1, and so
   on; within a layer, sort features by `*-sort-key` ascending, then by deterministic
   source order.
3. Append the page's own overlays after every style layer: routes, markers, traffic and
   the location indicator stay above the base map.

This is the single largest correctness fix in the source/layer model: a fill from one
tile can no longer paint over a road that belongs to another tile's earlier layer. Page
overlays do not participate in style collision or sort keys; a style-driven icon layer
that should receive collision must be a symbol layer. A higher sort key paints later and
appears above a lower one, for line, fill and circle alike.

### 5.8 Sources

A style source is a declaration, not a vendor URL baked into the renderer. The host
binds each source id to a registered provider or fixture through `SourceBinding {
provider_key, url_template, attribution_text, glyphs_url, minimum_zoom, maximum_zoom }`,
which keeps provider URLs out of the renderer and reference apps, keeps credentials in
the provider registration path, and lets a test bind the same source id to a fixture
provider without touching the network.

The sources work implements multiple vector sources and GeoJSON. GeoJSON is consumed
through a small tiling step at the source boundary: features are clipped to the same
tile grid the vector source uses, so downstream buckets and symbol placement see one
feature model. The tiler must stay deterministic for goldens.

### 5.9 Symbol placement

The placement module is modeled on MapLibre Native's stages:

1. **Layout**: shape text, resolve `text-field` with `format`, apply `text-transform`,
   wrap at `text-max-width`, measure glyph boxes with the selected font stack, choose
   anchors and offsets, and compute line glyph placement with `text-max-angle`,
   `symbol-spacing` and `text-keep-upright`.
2. **Placement**: a frame-global pass with a persistent collision index. Icons and text
   collide as separate boxes unless `icon-optional` ties them. `symbol-sort-key` (and
   the legacy priority field, mapped to the spec key) orders placement.
   `*-ignore-placement` symbols are drawn but do not block; `*-allow-overlap`/`overlap:
   always` symbols ignore existing boxes.
3. **Cross-tile dedupe**: a cross-tile symbol index keyed by source feature id (or a
   stable text-plus-geometry hash when no id exists) prevents the buffered overlap
   between tiles from painting one place twice; the current text-only dedupe is the
   fallback when ids are absent.
4. **Draw**: glyph quads for text, sprite quads for icons, halo first.

The persistent index is invalidated on style revision, source change, zoom-level change
and glyph/sprite atlas revision.

### 5.10 Colors and units

- Parse CSS color forms the styles in the wild use: `#rgb`, `#rgba`, `#rrggbb`,
  `#rrggbbaa`, `rgb()`, `rgba()`, `hsl()`, `hsla()` and the named colors. Store sRGB
  components in `[0, 1]` as the renderer's existing colors do, and convert only where
  the shader expects otherwise.
- Units follow the specification: `line-width` in screen pixels, `line-dasharray` in
  line-width units, `text-size` in pixels, `text-max-width`, `text-line-height`,
  `text-letter-spacing` and `text-radial-offset` in ems, `text-offset` and `icon-offset`
  in ems, `*padding` in pixels, `*translate` in pixels.
- Device pixel ratio scales drawing-buffer sizes at the surface boundary, as today;
  style numbers remain point/CSS pixels.

## 6. Migration of the committed styles

The migration keeps rendering identical where the old model and the specification agree,
proved by the existing goldens. The road hierarchy, per-class minimum zooms and width
curves (Appendix A) become `minzoom` values and `interpolate` expressions.
`scripts/convert-vector-style-artifacts.mjs` converts each committed artifact to a
version-8 document: one vector source `base`; `source_layer` becomes `source: "base"`
plus `source-layer`; `geometry_kind` becomes the layer `type`; `rgba` becomes the
corresponding paint color with the same channels; `filter` becomes an equivalent
`all`/`==` expression; `width_zoom_stops` and `text_size_zoom_stops` become
`["interpolate", ["linear"], ["zoom"], ...]`; `min_zoom`/`max_zoom` become
`minzoom`/`maxzoom`; symbol fields map to `text-field` fallbacks and the other symbol
properties; `class_minimum_zooms` and `rank_zoom_stops` become an `all` filter with
`case`/`step` on `["zoom"]` and the feature `class`/`rank`. The converter and its
generated documents are committed.

A dual-run parity test iterates the seven styles, the committed fixture tiles and zoom
levels 9, 12, 14, 16 and 18, comparing `overlay_primitives_for_vector_tile_at_zoom`
structurally (kind, RGBA, width, coordinates within 1e-9) and
`label_candidates_for_vector_tile` field by field, and fails on any difference. All five
vector-base goldens then run without `UPDATE_GOLDENS=1`: single-tile scenes must be
identical, and the multi-tile road-name scene may change only at tile seams because
global layer order is intentionally different, with an inspected diff recorded. After
parity and a green gate, the legacy artifact parser and in-code fallback are deleted or
kept as a version-8 document constant; the design prefers one parser.

The committed product styles stay provider-neutral: their source id resolves through the
registry, and no vendor URL is copied into a style document. The demotiles fixture is
the one test-owned document that carries a public URL, and it is rewritten to a fixture
binding at load time.

## 7. What is mirrored from MapLibre Native, and what is not

Mirrored, reimplemented from the specification and the published design (no C++ copied):
the document model (sources, layers, layout/paint split, root properties); expression
semantics (typed parsing, zoom restrictions, `step`, `interpolate` curves, legacy
function and filter normalization); the bucket idea (per tile, per layer, per zoom,
compiled once and cached; paint resolved at the frame zoom, layout at integer zoom);
layer and feature ordering including sort keys and global layer order; symbol semantics
(text layout, anchors, line placement with a maximum angle, spacing, keep-upright,
collision boxes, ignore-placement, optional, a persistent collision index and a
cross-tile symbol index); line semantics (joins, caps, miter/round limits, dash units
relative to width, offset and gap width); and the demotiles style as the acceptance
target.

Deliberately not mirrored: MapLibre Native's C++ actor/threading architecture, tile
pyramid implementation, GL/Metal/Vulkan/Dawn backends and render orchestrator, because
this SDK keeps its own wgpu pipeline and `VectorBaseScheduler`; HarfBuzz shaping, bidi
and complex-script text, because `rustic-maps-glyphs` documents Latin-only support; the
full glyph/sprite HTTP cache stack, because the SDK keeps its provider transport, cache
policy and same-origin fixture rule; fill-extrusion, heatmap, hillshade, color-relief,
terrain, sky, light, globe projection and video, which are parsed for diagnostics and
deferred; and `within` (Turf-derived), `collator`/`resolved-locale`,
`is-supported-script` and per-feature expression error reporting, which are deferred
with explicit diagnostics.

## 8. Licensing

- **MapLibre Native** is BSD-2-Clause (`LICENSE.md`). This design copies no C++ source
  and no shader text; semantics are reimplemented from the public specification and the
  design book. The existing "Prior art & inspiration" credit in
  `docs/THIRD_PARTY_NOTICES.md` names MapLibre. If any file is later ported verbatim,
  the change must commit a `maplibre-native-LICENSE.md` beside the other licence texts
  under `docs/THIRD_PARTY_LICENSES/` with the BSD-2-Clause text, and change the notices
  entry to say logic was ported.
- **MapLibre style specification** is BSD-3-Clause (`LICENSE.txt` in
  `maplibre-style-spec`). The design reads the published specification and does not copy
  its text or JSON schema. If a copy of the specification, its schema or its expression
  test vectors is committed, the change must commit
  `docs/THIRD_PARTY_LICENSES/maplibre-style-spec-LICENSE.txt` (BSD-3-Clause) and add a
  notices entry naming the copied material.
- **MapLibre demotiles** is BSD-3-Clause, Copyright (c) 2021 MapLibre. Committing
  `https://demotiles.maplibre.org/style.json` as
  `crates/rustic-maps-style/fixtures/styles/demotiles-style.json` requires
  `docs/THIRD_PARTY_LICENSES/maplibre-demotiles-LICENSE.txt` with the BSD-3-Clause text;
  a "Prior art & inspiration" notices entry naming the style, repository and copyright
  and carrying the style's own metadata attribution (`This style was generated on
  MapTiler Cloud. Usage is governed by the license terms in
  https://github.com/maplibre/demotiles/blob/gh-pages/LICENSE`); and a data entry for
  vector tiles © OpenStreetMap contributors (ODbL) via the OSM US Tile Service and the
  Natural Earth provenance of the country data. The fixture is test data, never a
  product default, and its source URLs are rebound to the fixture provider.
- **Fonts.** The demotiles style asks for `Open Sans Semibold`. Open Sans is SIL OFL-1.1
  (`OFL.txt` in `googlefonts/opensans`); Noto Sans is OFL-1.1 (`LICENSE` in
  `notofonts/noto-fonts`). A committed Open Sans glyph-range fixture would require
  `docs/THIRD_PARTY_LICENSES/open-sans-OFL.txt` and a notices entry; no Open Sans
  fixture is committed. The committed `fixtures/glyphs/noto_sans_regular_0-255.pbf`
  carries its SIL OFL-1.1 text at `docs/THIRD_PARTY_LICENSES/noto-sans-OFL-1.1.txt` with
  a notices entry.
- **Our code** remains MIT OR Apache-2.0. `scripts/generate-third-party-notices` derives
  only the Cargo graph; the hand-maintained "Prior art & inspiration" section and
  `docs/THIRD_PARTY_LICENSES/` are the correct homes for these entries, and the
  Ferrostar entry is the precedent.

## Appendix A — Road hierarchy, label placement and vector-pipeline behaviour

The committed artifacts carry these rules as `minzoom` values and `interpolate` width
stops; the renderer resolves them per frame.

### A.1 Road hierarchy

Every road layer declares a per-class minimum zoom and a width curve, in screen pixels:

| Class | Min zoom | Width at its first zoom | Width at z16 | Casing |
|---|---|---|---|---|
| motorway, trunk (and links) | 4 | 0.9 | 6.3 | yes, ~1.4 px over the fill, same stops |
| primary (and links) | 7 | 0.7 | 3.5 | no |
| secondary (and links) | 9 | 0.6 | 2.2 | no |
| tertiary (and links) | 11 | 0.5 | 1.5 | no |
| residential, unclassified, living street | 12 | 0.4 | 1.2 | no |
| service | 15 | 0.4 | 0.9 | no |
| pedestrian | 14 | 0.5 | 1.2 | no |

Links paint 0.8 of their parent class. Stops run at the class's first zoom and then at
8, 10, 12, 14, 16 and 18. At zoom 9 only motorway, trunk, primary and secondary paint.
The non-Carto styles (Bright, Fiord, Positron, OpenMapTiles) use the same mechanism
adapted to their schema's class names, anchored on each layer's existing city-zoom width
so the z14 goldens render unchanged.

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

Unit tests assert `width_pixels_at_zoom` and `is_visible_at_zoom` per class for every
artifact. The regional frame is pinned by `sacramento_region_zoom9_carto_light.png`
(native) and `sacramento_region_zoom9_carto_light_browser.png`; the browser golden
compares within a mean absolute channel error of 2.0 and an outlier fraction of 0.02.

### A.2 Label placement

Every drawn glyph contributes one collision box measured from the packed atlas' bearings
and advances, padded by `LABEL_COLLISION_PADDING_PIXELS` (2 px) on every side. A
line-placed name keeps the segment rotation on each box; two boxes are tested with the
separating-axis theorem.

Candidates are placed in this order, first key first:

1. **Symbol class** (the style's `symbol-sort-key`): route shields class 0, place names
   class 1, motorway-to-tertiary road names class 2, residential and unclassified road
   names class 3.
2. **Feature rank** (the feature's `rank`, lower first; unranked last within its class).
3. **Style-layer order** (the declared class/zoom tier).
4. **The previous frame's placement**, only among candidates that tie on the keys above;
   the memory never reorders across class, rank or tier.
5. **The caller's candidate order**, so the frame's tile list stays deterministic.

Before ordering, a ranked label survives only when its rank is at most the frame zoom's
band ceiling (`LABEL_REGIONAL_THINNING_BANDS`):

| Frame zoom | Highest rank kept |
|---|---|
| below 8 | 6 |
| 8 to below 10 | 18 |
| 10 to below 12 | 20 |
| 12 to below 14 | 22 |
| 14 and above | every rank |

Unranked labels are exempt, so road names and shields never thin. The style's own
zoom-stepped rank ceilings and per-class minimum zooms apply first and stay
authoritative where they are tighter. Names repeat along a long run, spaced at least 90
px and rotated to their segment.

Typography follows the committed Bright style unchanged: place text 10/13/18 px at
z10/z14/z18, POI 10/14 px at z14/z18, road text 10–18 px by class, halos 1.5 px (places
and POI) and 1.3 px (roads). The renderer's SDF coverage threshold is 0.75, the edge
half-band 0.105 field units, collision padding 2 px, line height 1.2 em and letter
spacing 0. Baseline placement sits 0.3 em below the visual centre; the shift stays under
one pixel at Bright's sizes.

### A.3 Vector fetching and drawing

- **Persistent cache.** Vector payloads share the installed SQLite tile cache at the
  30-day vector TTL, plus a TTL'd in-memory LRU. Native caching needs the tile cache
  installed before the source.
- **Prefetch.** After a frame settles, prefetch same-zoom neighbours and coarse parents,
  parents first, budget 32 tiles, at most one plan per 500 ms, detached and cancellable.
- **Progressive draw.** A missing child tile is covered by its nearest cached ancestor;
  the frame never blocks on a fetch.
- **Motion grading.** While a camera gesture is active, every frame paints the decoded
  tiles and the last settled style-zoom bucket's geometry and skips label assembly. The
  first still frame resolves the style bucket and labels at the final pose. The browser
  reports the gesture through `setCameraGestureActive`; the native surface reads the
  camera pose each draw. At the browser frame-budget scene, `renderFrame` p50 fell from
  8.0–8.1 ms ungraded to 1.4–1.8 ms graded.

The regression fixtures are the z9 Carto golden, the dense regional label golden
(`sacramento_region_zoom13_labels.png`), the wide thinning golden
(`sacramento_region_zoom8_labels.png`) and the browser frame-budget scene in
`docs/Browser.md`.
