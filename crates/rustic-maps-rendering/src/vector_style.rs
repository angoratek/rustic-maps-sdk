//! Vector-tile styling: a decoded [`rustic_maps_vector_tiles::VectorTile`] becomes
//! the renderer's overlay primitives under a compiled style plan.
//!
//! The style document and its expressions live in `rustic-maps-style`; this
//! module is the renderer's consumer. [`vector_tile_style_from_id`] parses a
//! committed version-8 style document and compiles it once into a
//! [`CompiledStyle`](rustic_maps_style::CompiledStyle). Each frame then resolves
//! the compiled plan at the camera's fractional zoom, so paint expressions
//! change continuously with the camera without re-parsing the document.
//!
//! Polygon features paint every exterior ring with its interior rings
//! subtracted through the overlay's hole-aware fill (see
//! [`crate::overlay::FilledPolygonOverlay`]); line features become styled
//! polylines and point features become screen-facing markers. Primitives are
//! grouped into one [`VectorOverlayBatch`] per style layer and one bucket per
//! layer in [`vector_tile_overlay_layers_at_zoom`], so a frame can paint every
//! tile's layer 0 before any tile's layer 1: global style order across the
//! visible tile set.
//!
//! Coordinates convert from tile-local extent space (`x` right, `y` down, the
//! MVT convention) through the tile's web-mercator address to geographic
//! degrees, so the overlays project exactly like every other map coordinate.
//!
//! Symbol layers paint no overlay primitive: their labels are data for the
//! frame-level label pass ([`crate::labels`]), which reads the same compiled
//! plan, filter and layout.

use crate::overlay::{
    DEFAULT_POINT_MARKER_PITCH_ALIGNMENT, DEFAULT_POINT_MARKER_PITCH_SCALE, FilledPolygonOverlay,
    MapOverlay, PointMarkersOverlay, PolylineStroke, RoutePolylineOverlay,
};
use geospatial_types::GeographicCoordinate;
use rustic_maps_style::{
    Color, CompiledLayer, CompiledLayerKind, CompiledSourceKind, CompiledStyle, EvaluationContext,
    GeometryType, MapStyleDocument, ResolvedPaint, Value, compile_style_document,
};
use rustic_maps_vector_tiles::{
    VectorTile, VectorTileAttributeValue, VectorTileFeature, VectorTileGeometry,
    VectorTileGeometryPoint, VectorTileGeometryType, VectorTileLayer, VectorTileLinearRing,
};
use std::collections::BTreeMap;
use std::f64::consts::PI;

/// The longitude span the web-mercator world covers, in degrees.
const WORLD_LONGITUDE_SPAN_DEGREES: f64 = 360.0;
/// The longitude at the world's western edge, in degrees.
const WORLD_WEST_LONGITUDE_DEGREES: f64 = -180.0;
/// The latitude the mercator projection approaches at the world's edges;
/// re-exported from `geospatial-types` so every layer shares one value.
pub use geospatial_types::MAXIMUM_MERCATOR_LATITUDE_DEGREES as MERCATOR_MAXIMUM_LATITUDE_DEGREES;

/// The text colour a label descriptor carries when it omits one: opaque
/// black, the specification's `text-color` default.
pub const DEFAULT_VECTOR_SYMBOL_TEXT_RGBA: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
/// The halo colour a label descriptor carries when it omits one: opaque
/// white.
pub const DEFAULT_VECTOR_SYMBOL_HALO_RGBA: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
/// Whether a label descriptor that omits the rule may overlap other labels:
/// no, text collides and is dropped when it cannot be placed.
pub const DEFAULT_VECTOR_SYMBOL_ALLOW_OVERLAP: bool = false;

/// The tile address a decoded tile belongs to (its web-mercator position).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VectorTileAddress {
    /// The tile's zoom level.
    pub zoom_level: u8,
    /// The tile's column at that zoom.
    pub tile_x: u32,
    /// The tile's row at that zoom.
    pub tile_y: u32,
}

/// One style layer's overlay primitives from one tile.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorOverlayBatch {
    /// The compiled layer's global paint order key.
    pub layer_index: usize,
    /// The compiled layer's id.
    pub layer_id: String,
    /// The layer's overlay primitives, in feature order.
    pub overlays: Vec<MapOverlay>,
}

/// The built-in style document the default style loads from.
pub const DEFAULT_VECTOR_STYLE_DOCUMENT_JSON: &str =
    include_str!("../fixtures/render/styles/vector-style.json");

/// The style ids the built-in styles answer, keyed by the tile schema and
/// the theme. A provider registry descriptor names only the schema half
/// (`carto` or `openmaptiles`); the page joins it with the active theme.
pub const VECTOR_STYLE_ID_CARTO_LIGHT: &str = "carto-light";
/// The OSM-Carto-like schema's dark palette.
pub const VECTOR_STYLE_ID_CARTO_DARK: &str = "carto-dark";
/// The OpenMapTiles schema's light palette.
pub const VECTOR_STYLE_ID_OPENMAPTILES_LIGHT: &str = "openmaptiles-light";
/// The OpenMapTiles schema's dark palette.
pub const VECTOR_STYLE_ID_OPENMAPTILES_DARK: &str = "openmaptiles-dark";
/// OpenFreeMap's **Bright** style (the OpenMapTiles schema's rich light
/// palette): the named light style the vector base offers.
pub const VECTOR_STYLE_ID_BRIGHT_LIGHT: &str = "bright-light";
/// OpenFreeMap's **Fiord** style: the named dark palette with legible road
/// classes.
pub const VECTOR_STYLE_ID_FIORD_DARK: &str = "fiord-dark";
/// OpenFreeMap's **Positron** style: the quiet, deliberately monochrome light
/// palette.
pub const VECTOR_STYLE_ID_POSITRON_LIGHT: &str = "positron-light";
/// The browser SDF coverage probe's measurement fixture: a literal white `I`
/// at 48 point pixels on black, painted over the committed OpenMapTiles
/// fixture. It is not a cartographic base map; the probe uses it to isolate
/// one glyph edge through the same vector-source path the cartographic styles
/// use.
pub const VECTOR_STYLE_ID_LABEL_COVERAGE_PROBE: &str = "label-coverage-probe";

/// The dark OSM-Carto-like document: the same layer structure as the default,
/// painted for a dark map surface.
const CARTO_DARK_STYLE_DOCUMENT_JSON: &str =
    include_str!("../fixtures/render/styles/vector-style-dark.json");
/// The OpenMapTiles light document: the layer and attribute vocabulary of the
/// OpenMapTiles schema.
const OPENMAPTILES_LIGHT_STYLE_DOCUMENT_JSON: &str =
    include_str!("../fixtures/render/styles/vector-style-openmaptiles-light.json");
/// The OpenMapTiles dark document.
const OPENMAPTILES_DARK_STYLE_DOCUMENT_JSON: &str =
    include_str!("../fixtures/render/styles/vector-style-openmaptiles-dark.json");
/// The Bright light document: the Bright palette curated to the overlay
/// pipeline's fill, line, circle and symbol kinds.
const BRIGHT_LIGHT_STYLE_DOCUMENT_JSON: &str =
    include_str!("../fixtures/render/styles/vector-style-bright-light.json");
/// The Fiord dark document, curated to the same overlay kinds.
const FIORD_DARK_STYLE_DOCUMENT_JSON: &str =
    include_str!("../fixtures/render/styles/vector-style-fiord-dark.json");
/// The Positron light document, curated to the same overlay kinds.
const POSITRON_LIGHT_STYLE_DOCUMENT_JSON: &str =
    include_str!("../fixtures/render/styles/vector-style-positron-light.json");
/// The label coverage probe document: one white `I` on black.
const LABEL_COVERAGE_PROBE_STYLE_DOCUMENT_JSON: &str =
    include_str!("../fixtures/render/styles/vector-style-label-coverage-probe.json");

/// The style documents a style id can name; `None` names no built-in style.
fn vector_style_document_json(style_id: &str) -> Option<&'static str> {
    match style_id {
        VECTOR_STYLE_ID_CARTO_LIGHT => Some(DEFAULT_VECTOR_STYLE_DOCUMENT_JSON),
        VECTOR_STYLE_ID_CARTO_DARK => Some(CARTO_DARK_STYLE_DOCUMENT_JSON),
        VECTOR_STYLE_ID_OPENMAPTILES_LIGHT => Some(OPENMAPTILES_LIGHT_STYLE_DOCUMENT_JSON),
        VECTOR_STYLE_ID_OPENMAPTILES_DARK => Some(OPENMAPTILES_DARK_STYLE_DOCUMENT_JSON),
        VECTOR_STYLE_ID_BRIGHT_LIGHT => Some(BRIGHT_LIGHT_STYLE_DOCUMENT_JSON),
        VECTOR_STYLE_ID_FIORD_DARK => Some(FIORD_DARK_STYLE_DOCUMENT_JSON),
        VECTOR_STYLE_ID_POSITRON_LIGHT => Some(POSITRON_LIGHT_STYLE_DOCUMENT_JSON),
        VECTOR_STYLE_ID_LABEL_COVERAGE_PROBE => Some(LABEL_COVERAGE_PROBE_STYLE_DOCUMENT_JSON),
        _ => None,
    }
}

/// Parses and compiles the built-in style document a style id names.
///
/// # Parameters
/// - `style_id` — one of the exported `VECTOR_STYLE_ID_*` identifiers.
///
/// # Returns
/// The compiled style, ready to resolve at a frame zoom and feature.
///
/// # Errors
/// An unknown id and a malformed document both report the typed artifact
/// error rather than a guess.
pub fn vector_tile_style_from_id(
    style_id: &str,
) -> Result<CompiledStyle, VectorStyleArtifactError> {
    let Some(style_json) = vector_style_document_json(style_id) else {
        return Err(VectorStyleArtifactError {
            detail: format!(
                "no committed vector style carries the id {style_id:?}; known ids are \
                 {VECTOR_STYLE_ID_CARTO_LIGHT}, {VECTOR_STYLE_ID_CARTO_DARK}, \
                 {VECTOR_STYLE_ID_OPENMAPTILES_LIGHT}, {VECTOR_STYLE_ID_OPENMAPTILES_DARK}, \
                 {VECTOR_STYLE_ID_BRIGHT_LIGHT}, {VECTOR_STYLE_ID_FIORD_DARK}, \
                 {VECTOR_STYLE_ID_POSITRON_LIGHT}, {VECTOR_STYLE_ID_LABEL_COVERAGE_PROBE}"
            ),
        });
    };
    vector_tile_style_from_json(style_json)
}

/// Why a style document could not be parsed into a compiled style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorStyleArtifactError {
    /// The underlying parser detail.
    pub detail: String,
}

impl std::fmt::Display for VectorStyleArtifactError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "vector style artifact invalid: {}", self.detail)
    }
}

impl std::error::Error for VectorStyleArtifactError {}

/// Parses and compiles a version-8 style document.
///
/// The compile step is infallible; only a malformed document reports an error.
pub fn vector_tile_style_from_json(
    style_json: &str,
) -> Result<CompiledStyle, VectorStyleArtifactError> {
    let document = MapStyleDocument::parse_json(style_json).map_err(|style_error| {
        VectorStyleArtifactError {
            detail: style_error.to_string(),
        }
    })?;
    Ok(compile_style_document(&document))
}

/// The default style, compiled from the committed default document.
pub fn default_vector_tile_style() -> CompiledStyle {
    vector_tile_style_or_fallback(DEFAULT_VECTOR_STYLE_DOCUMENT_JSON)
}

/// Compiles a style document, falling back to the committed default document
/// when the supplied one is malformed.
///
/// The default document is itself compiled at most once per call and is a
/// compile-time constant, so this cannot loop; an empty style is the last
/// resort if the committed default is ever removed from the build.
pub fn vector_tile_style_or_fallback(style_json: &str) -> CompiledStyle {
    vector_tile_style_from_json(style_json).unwrap_or_else(|_| {
        vector_tile_style_from_json(DEFAULT_VECTOR_STYLE_DOCUMENT_JSON)
            .unwrap_or_else(|_| empty_compiled_style())
    })
}

/// The style that paints nothing, used only when the committed default
/// document is unreadable.
fn empty_compiled_style() -> CompiledStyle {
    CompiledStyle {
        background: None,
        sprite: None,
        glyphs: None,
        sources: std::collections::BTreeMap::new(),
        layers: Vec::new(),
        transition: None,
        unsupported: Vec::new(),
    }
}

/// Converts one tile-local extent point into a geographic coordinate through
/// the tile's web-mercator address. Vertices at the extent boundary are legal
/// (they land on the next tile's edge); the latitude clamp keeps the projection
/// finite at the world's polar edges.
pub fn tile_local_point_to_coordinate(
    tile_address: VectorTileAddress,
    layer_extent: u64,
    point: &VectorTileGeometryPoint,
) -> GeographicCoordinate {
    let extent = layer_extent.max(1) as f64;
    let tiles_per_axis = (1u64 << tile_address.zoom_level) as f64;
    let world_x =
        (f64::from(tile_address.tile_x) + point.x_coordinate as f64 / extent) / tiles_per_axis;
    let world_y =
        (f64::from(tile_address.tile_y) + point.y_coordinate as f64 / extent) / tiles_per_axis;
    let longitude_degrees = world_x * WORLD_LONGITUDE_SPAN_DEGREES + WORLD_WEST_LONGITUDE_DEGREES;
    let latitude_degrees = (PI * (1.0 - 2.0 * world_y)).sinh().atan().to_degrees();
    GeographicCoordinate {
        latitude: latitude_degrees.clamp(
            -MERCATOR_MAXIMUM_LATITUDE_DEGREES,
            MERCATOR_MAXIMUM_LATITUDE_DEGREES,
        ),
        longitude: longitude_degrees,
    }
}

/// Converts a decoded tile into one batch per style layer whose filter and
/// geometry contribute at least one primitive. The tile's own zoom level is
/// the style zoom.
pub fn overlay_primitives_for_vector_tile(
    tile: &VectorTile,
    tile_address: VectorTileAddress,
    style: &CompiledStyle,
) -> Vec<VectorOverlayBatch> {
    overlay_primitives_for_vector_tile_at_zoom(
        tile,
        tile_address,
        style,
        f64::from(tile_address.zoom_level),
    )
}

/// The same conversion as [`overlay_primitives_for_vector_tile`], with the
/// style's zoom expressions evaluated at an explicit `style_zoom_level`.
///
/// The base-map scheduler styles a cached tile at the camera's fractional zoom
/// level, so a pan across a paint stop restyles the already-fetched tile
/// instead of fetching it again; the tile's address still converts every
/// coordinate, because the geometry belongs to the tile, not to the zoom the
/// paint is evaluated at.
pub fn overlay_primitives_for_vector_tile_at_zoom(
    tile: &VectorTile,
    tile_address: VectorTileAddress,
    style: &CompiledStyle,
    style_zoom_level: f64,
) -> Vec<VectorOverlayBatch> {
    let mut batches = Vec::new();
    for layer in &style.layers {
        let overlays = overlays_for_layer(tile, tile_address, layer, style_zoom_level);
        if !overlays.is_empty() {
            batches.push(VectorOverlayBatch {
                layer_index: layer.order_index,
                layer_id: layer.id.clone(),
                overlays,
            });
        }
    }
    batches
}

/// The tile's overlays bucketed by compiled layer index: bucket `index`
/// holds every primitive the style layer whose `order_index` is `index`
/// paints from this tile, and an empty bucket means that layer painted
/// nothing. A frame concatenates the buckets layer by layer across all tiles.
pub fn vector_tile_overlay_layers_at_zoom(
    tile: &VectorTile,
    tile_address: VectorTileAddress,
    style: &CompiledStyle,
    style_zoom_level: f64,
) -> Vec<Vec<MapOverlay>> {
    let mut layers = vec![Vec::new(); style.layer_count()];
    for layer in &style.layers {
        if let Some(bucket) = layers.get_mut(layer.order_index) {
            *bucket = overlays_for_layer(tile, tile_address, layer, style_zoom_level);
        }
    }
    layers
}

/// [`vector_tile_overlay_layers_at_zoom`] with a style transition applied.
///
/// The buckets come from the new `style` in its global document order; a
/// painted colour or opacity blends from `previous_style`'s resolved value
/// over the declared transition as `elapsed_seconds` advances. The caller
/// bypasses its styled-geometry cache while a transition is running, because
/// the resolved paint changes every frame.
pub fn vector_tile_overlay_layers_at_zoom_with_transition(
    tile: &VectorTile,
    tile_address: VectorTileAddress,
    style: &CompiledStyle,
    previous_style: &CompiledStyle,
    elapsed_seconds: f64,
    style_zoom_level: f64,
) -> Vec<Vec<MapOverlay>> {
    let mut layers = vec![Vec::new(); style.layer_count()];
    for layer in &style.layers {
        if let Some(bucket) = layers.get_mut(layer.order_index) {
            *bucket = overlays_for_layer_with_transition(
                tile,
                tile_address,
                layer,
                style_zoom_level,
                Some(previous_style),
                style.transition,
                elapsed_seconds,
            );
        }
    }
    layers
}

/// Converts one decoded tile into its flat overlay primitive list under the
/// style (the batches of [`overlay_primitives_for_vector_tile`] in style
/// order, concatenated).
pub fn vector_tile_overlays(
    tile: &VectorTile,
    tile_address: VectorTileAddress,
    style: &CompiledStyle,
) -> Vec<MapOverlay> {
    vector_tile_overlays_at_zoom(
        tile,
        tile_address,
        style,
        f64::from(tile_address.zoom_level),
    )
}

/// The flat overlay primitive list of [`overlay_primitives_for_vector_tile_at_zoom`]
/// (its batches in style order, concatenated).
pub fn vector_tile_overlays_at_zoom(
    tile: &VectorTile,
    tile_address: VectorTileAddress,
    style: &CompiledStyle,
    style_zoom_level: f64,
) -> Vec<MapOverlay> {
    overlay_primitives_for_vector_tile_at_zoom(tile, tile_address, style, style_zoom_level)
        .into_iter()
        .flat_map(|batch| batch.overlays)
        .collect()
}

/// Every overlay one compiled layer paints from the tile, in feature order.
///
/// This is the single-layer seam a multi-source frame assembles with: the
/// caller passes the tile fetched from the layer's own source binding, so
/// layers bound to different sources never read each other's geometry.
pub fn overlays_for_compiled_layer_at_zoom(
    tile: &VectorTile,
    tile_address: VectorTileAddress,
    layer: &CompiledLayer,
    style_zoom_level: f64,
) -> Vec<MapOverlay> {
    overlays_for_layer(tile, tile_address, layer, style_zoom_level)
}

/// The tile layers a compiled layer reads.
///
/// A layer names its `source-layer`; a GeoJSON source carries its features in
/// one implicit layer, so a GeoJSON-bound layer without `source-layer` reads
/// that single layer. A vector layer without `source-layer` reads nothing.
pub fn tile_layers_for_compiled_layer<'a>(
    tile: &'a VectorTile,
    layer: &CompiledLayer,
) -> Vec<&'a VectorTileLayer> {
    // A GeoJSON source carries every feature in one implicit layer, and the
    // style specification ignores `source-layer` for such a source.
    if layer.source_kind == Some(CompiledSourceKind::GeoJson) && tile.layers.len() == 1 {
        return tile.layers.iter().collect();
    }
    match layer.source_layer.as_deref() {
        Some(source_layer) => tile
            .layers
            .iter()
            .filter(|tile_layer| tile_layer.name == source_layer)
            .collect(),
        None => Vec::new(),
    }
}

/// One style layer whose `source-layer` the decoded tile does not carry.
///
/// A style document is written for one tileset schema. Serving it tiles built
/// from a different schema leaves every layer reading an absent source layer,
/// so the map paints a background and fragments while each layer looks
/// well-formed. This diagnostic names the mismatch instead of letting a frame
/// paint nothing without an explanation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorStyleSourceMismatch {
    /// The style layer that reads the absent source layer.
    pub layer_id: String,
    /// The source-layer name the style layer reads.
    pub source_layer: String,
    /// The source-layer names the served tile was decoded from, in wire order.
    pub available_source_layers: Vec<String>,
    /// The sentence a status readback or diagnostics ring can show.
    pub detail: String,
}

impl std::fmt::Display for VectorStyleSourceMismatch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.detail)
    }
}

/// Every style layer whose `source-layer` the tile does not carry.
///
/// The comparison covers the layers this renderer paints (fill, line, circle
/// and symbol) that name a source layer and read a vector source. A GeoJSON
/// source carries its features in one implicit layer and is skipped, and a
/// style layer without a `source-layer` reads nothing by design and is not a
/// mismatch.
///
/// # Parameters
/// - `tile` — one decoded tile fetched from the provider the style was
///   installed for.
/// - `style` — the compiled document installed over that provider.
///
/// # Returns
/// One named mismatch per affected layer, in style order. An empty list means
/// every painted source layer is present. A vector-base install can report the
/// list once per source and tile address instead of painting fragments.
pub fn vector_style_source_layer_mismatches(
    tile: &VectorTile,
    style: &CompiledStyle,
) -> Vec<VectorStyleSourceMismatch> {
    let available_source_layers: Vec<String> = tile
        .layers
        .iter()
        .map(|tile_layer| tile_layer.name.clone())
        .collect();
    let mut mismatches = Vec::new();
    for layer in &style.layers {
        if !matches!(
            layer.kind,
            CompiledLayerKind::Fill
                | CompiledLayerKind::Line
                | CompiledLayerKind::Circle
                | CompiledLayerKind::Symbol
        ) || layer.source_kind == Some(CompiledSourceKind::GeoJson)
        {
            continue;
        }
        let Some(source_layer) = layer.source_layer.as_deref() else {
            continue;
        };
        if available_source_layers
            .iter()
            .any(|tile_layer_name| tile_layer_name == source_layer)
        {
            continue;
        }
        mismatches.push(VectorStyleSourceMismatch {
            layer_id: layer.id.clone(),
            source_layer: source_layer.to_string(),
            available_source_layers: available_source_layers.clone(),
            detail: format!(
                "the style layer {:?} reads the source layer {source_layer:?}, which the \
                 served tileset does not carry; the tile's layers are [{}]",
                layer.id,
                available_source_layers.join(", ")
            ),
        });
    }
    mismatches
}

fn overlays_for_layer(
    tile: &VectorTile,
    tile_address: VectorTileAddress,
    layer: &CompiledLayer,
    style_zoom_level: f64,
) -> Vec<MapOverlay> {
    overlays_for_layer_with_transition(tile, tile_address, layer, style_zoom_level, None, None, 0.0)
}

/// [`overlays_for_layer`] with a style transition applied.
///
/// When `previous_style` is `Some`, a painted colour or opacity blends from
/// the previous style's resolved value toward the new one over the declared
/// transition as `elapsed_seconds` advances. The previous style's layer is
/// matched by id; a layer it does not carry takes the new value immediately.
fn overlays_for_layer_with_transition(
    tile: &VectorTile,
    tile_address: VectorTileAddress,
    layer: &CompiledLayer,
    style_zoom_level: f64,
    previous_style: Option<&CompiledStyle>,
    root_transition: Option<rustic_maps_style::TransitionSpec>,
    elapsed_seconds: f64,
) -> Vec<MapOverlay> {
    if !matches!(
        layer.kind,
        CompiledLayerKind::Fill | CompiledLayerKind::Line | CompiledLayerKind::Circle
    ) || !layer.is_visible_at_zoom(style_zoom_level)
    {
        return Vec::new();
    }
    let mut feature_overlays: Vec<(f64, Vec<MapOverlay>)> = Vec::new();
    for tile_layer in tile_layers_for_compiled_layer(tile, layer) {
        for feature in &tile_layer.features {
            let properties = feature_properties(feature);
            let context = feature_evaluation_context(feature, &properties, style_zoom_level);
            if !layer.matches_filter(&context) {
                continue;
            }
            let resolved_paint = match previous_style {
                Some(previous_style) => layer.resolved_transitioned_paint(
                    &context,
                    root_transition,
                    previous_style
                        .layer(&layer.id)
                        .map(|previous_layer| (previous_layer, elapsed_seconds)),
                ),
                None => layer.resolved_paint(&context),
            };
            let mut overlays = Vec::new();
            match (&feature.geometry, layer.kind) {
                (VectorTileGeometry::Polygons(rings), CompiledLayerKind::Fill) => {
                    if let ResolvedPaint::Fill(paint) = resolved_paint {
                        overlays.extend(filled_polygon_overlays_for_rings(
                            rings,
                            tile_layer.extent,
                            tile_address,
                            &paint,
                        ));
                    }
                }
                (VectorTileGeometry::LineStrings(line_strings), CompiledLayerKind::Line) => {
                    if let ResolvedPaint::Line(paint) = resolved_paint {
                        let rgba_color = resolved_rgba(paint.color, paint.opacity);
                        // A layer that declares a line presentation gets the
                        // specification's stroke; one that declares nothing
                        // keeps the renderer's default route stroke, so a
                        // style that predates these properties draws exactly
                        // as before.
                        let stroke = layer.declares_line_presentation.then(|| PolylineStroke {
                            cap: layer.resolved_line_cap(&context),
                            join: layer.resolved_line_join(&context),
                            dash_array: paint.dasharray.clone(),
                            offset_pixels: paint.offset,
                            gap_width_pixels: paint.gap_width,
                            blur_pixels: paint.blur,
                        });
                        for line_string in line_strings {
                            let coordinates = line_string
                                .iter()
                                .map(|point| {
                                    tile_local_point_to_coordinate(
                                        tile_address,
                                        tile_layer.extent,
                                        point,
                                    )
                                })
                                .collect();
                            overlays.push(MapOverlay::RoutePolyline(RoutePolylineOverlay {
                                coordinates,
                                rgba_color,
                                width_pixels: paint.width,
                                stroke: stroke.clone(),
                                translate_pixels: paint.translate,
                                translate_anchor: paint.translate_anchor,
                            }));
                        }
                    }
                }
                (VectorTileGeometry::Points(points), CompiledLayerKind::Circle) => {
                    if let ResolvedPaint::Circle(paint) = resolved_paint {
                        let positions = points
                            .iter()
                            .map(|point| {
                                tile_local_point_to_coordinate(
                                    tile_address,
                                    tile_layer.extent,
                                    point,
                                )
                            })
                            .collect();
                        overlays.push(MapOverlay::PointMarkers(PointMarkersOverlay {
                            positions,
                            rgba_color: resolved_rgba(paint.color, paint.opacity),
                            radius_pixels: paint.radius,
                            rgba_stroke_color: paint.stroke_color.map(|stroke_color| {
                                resolved_rgba(stroke_color, paint.stroke_opacity)
                            }),
                            stroke_width_pixels: paint.stroke_width,
                            blur_pixels: paint.blur,
                            translate_pixels: paint.translate,
                            // A layer that declares neither property keeps
                            // this renderer's screen-facing default, which
                            // the cached world-space path can express.
                            pitch_scale: if layer.declares_circle_pitch {
                                layer.resolved_circle_pitch_scale(&context)
                            } else {
                                DEFAULT_POINT_MARKER_PITCH_SCALE
                            },
                            pitch_alignment: if layer.declares_circle_pitch {
                                layer.resolved_circle_pitch_alignment(&context)
                            } else {
                                DEFAULT_POINT_MARKER_PITCH_ALIGNMENT
                            },
                        }));
                    }
                }
                // A style paints only its declared kind; a feature of
                // another kind is not this layer's paint, and a symbol
                // layer's labels belong to the frame-level label pass.
                _ => {}
            }
            if !overlays.is_empty() {
                let sort_key = match layer.kind {
                    CompiledLayerKind::Fill => layer.resolved_fill_sort_key(&context),
                    CompiledLayerKind::Line => layer.resolved_line_sort_key(&context),
                    CompiledLayerKind::Circle => layer.resolved_circle_sort_key(&context),
                    _ => 0.0,
                };
                feature_overlays.push((sort_key, overlays));
            }
        }
    }
    // The layer kind's `*-sort-key` orders features within the layer; a
    // stable sort keeps equal keys in source order, and features that declare
    // no key all resolve to the documented default.
    feature_overlays.sort_by(|(left_key, _), (right_key, _)| left_key.total_cmp(right_key));
    feature_overlays
        .into_iter()
        .flat_map(|(_, overlays)| overlays)
        .collect()
}

/// Converts the compiled colour and opacity into the renderer's RGBA
/// components, folding opacity into the alpha channel.
pub(crate) fn resolved_rgba(color: Color, opacity: f64) -> [f32; 4] {
    [
        color.red as f32,
        color.green as f32,
        color.blue as f32,
        (color.alpha * opacity) as f32,
    ]
}

/// The feature's attributes as the style evaluator's property map.
pub(crate) fn feature_properties(feature: &VectorTileFeature) -> BTreeMap<String, Value> {
    feature
        .attributes
        .iter()
        .map(|(attribute_name, attribute_value)| {
            (
                attribute_name.clone(),
                attribute_value_to_style_value(attribute_value),
            )
        })
        .collect()
}

/// One decoder attribute value as the style evaluator's value: text, a number
/// for every numeric wire variant, and a boolean.
fn attribute_value_to_style_value(attribute_value: &VectorTileAttributeValue) -> Value {
    match attribute_value {
        VectorTileAttributeValue::Text(text) => Value::String(text.clone()),
        VectorTileAttributeValue::Boolean(boolean) => Value::Boolean(*boolean),
        numeric_value => Value::Number(numeric_value.numeric_value().unwrap_or_default()),
    }
}

/// The feature's geometry kind as the style evaluator reports it.
pub(crate) fn feature_geometry_type(feature: &VectorTileFeature) -> GeometryType {
    match feature.geometry_type {
        VectorTileGeometryType::LineString => GeometryType::LineString,
        VectorTileGeometryType::Polygon => GeometryType::Polygon,
        VectorTileGeometryType::Point | VectorTileGeometryType::Unknown => GeometryType::Point,
    }
}

/// An evaluation context for one feature at `zoom`: its properties, geometry
/// kind and no external state.
pub(crate) fn feature_evaluation_context<'a>(
    feature: &VectorTileFeature,
    properties: &'a BTreeMap<String, Value>,
    zoom: f64,
) -> EvaluationContext<'a> {
    EvaluationContext::at_zoom(zoom).with_feature(properties, feature_geometry_type(feature))
}

/// Groups a polygon feature's rings into filled overlays: each exterior ring
/// owns its immediately following interior rings, the specification's order.
/// A leading interior ring (malformed ring order) has no exterior to subtract
/// from and is skipped rather than painted as a fill of its own.
fn filled_polygon_overlays_for_rings(
    rings: &[VectorTileLinearRing],
    layer_extent: u64,
    tile_address: VectorTileAddress,
    paint: &rustic_maps_style::ResolvedFillPaint,
) -> Vec<MapOverlay> {
    let mut overlays = Vec::new();
    let mut current_exterior_index: Option<usize> = None;
    for ring in rings {
        let ring_coordinates: Vec<GeographicCoordinate> = ring
            .vertices
            .iter()
            .map(|point| tile_local_point_to_coordinate(tile_address, layer_extent, point))
            .collect();
        if ring_is_exterior(ring) {
            overlays.push(MapOverlay::FilledPolygon(FilledPolygonOverlay {
                ring_coordinates,
                hole_ring_coordinates: Vec::new(),
                rgba_color: resolved_rgba(paint.color, paint.opacity),
                rgba_outline_color: paint
                    .outline_color
                    .map(|outline_color| resolved_rgba(outline_color, paint.opacity)),
                antialias: paint.antialias,
                translate_pixels: paint.translate,
                // A zero displacement is identical under either anchor, so
                // it takes the renderer's own default and keeps a style that
                // translates nothing structurally equal to the legacy parser.
                translate_anchor: if paint.translate == [0.0, 0.0] {
                    rustic_maps_style::TranslateAnchor::Viewport
                } else {
                    paint.translate_anchor
                },
            }));
            current_exterior_index = Some(overlays.len() - 1);
        } else if let Some(exterior_index) = current_exterior_index
            && let Some(MapOverlay::FilledPolygon(overlay)) = overlays.get_mut(exterior_index)
        {
            overlay.hole_ring_coordinates.push(ring_coordinates);
        }
    }
    overlays
}

/// The feature's geometry kind (the decoder's own classification).
pub fn feature_geometry_kind(feature: &VectorTileFeature) -> VectorTileGeometryType {
    feature.geometry_type
}

/// The ring's signed shoelace area in tile-local space; exterior rings are
/// positive (the specification's `y`-down convention), holes negative.
fn ring_is_exterior(ring: &VectorTileLinearRing) -> bool {
    let vertices = &ring.vertices;
    if vertices.len() < 3 {
        return false;
    }
    let mut doubled_area = 0.0;
    for vertex_index in 0..vertices.len() {
        let current = &vertices[vertex_index];
        let next = &vertices[(vertex_index + 1) % vertices.len()];
        doubled_area += current.x_coordinate as f64 * next.y_coordinate as f64
            - next.x_coordinate as f64 * current.y_coordinate as f64;
    }
    doubled_area > 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A style whose one line layer sorts by `rank`, paints a width from
    /// `width` and declares a round cap.
    fn sorted_line_style() -> CompiledStyle {
        let document = MapStyleDocument::parse_json(
            r#"{
                "version": 8,
                "sources": { "base": { "type": "vector" } },
                "layers": [
                    {
                        "id": "roads",
                        "type": "line",
                        "source": "base",
                        "source-layer": "roads",
                        "layout": { "line-cap": "round", "line-sort-key": ["get", "rank"] },
                        "paint": { "line-width": ["get", "width"] }
                    }
                ]
            }"#,
        )
        .expect("the fixture style parses");
        compile_style_document(&document)
    }

    /// Two road features in source order: the higher rank first, so a sort is
    /// observable.
    fn tile_with_two_roads() -> VectorTile {
        let line = |y_coordinate: i64| {
            VectorTileGeometry::LineStrings(vec![vec![
                VectorTileGeometryPoint {
                    x_coordinate: 0,
                    y_coordinate,
                },
                VectorTileGeometryPoint {
                    x_coordinate: 100,
                    y_coordinate,
                },
            ]])
        };
        let road = |identifier: u64, rank: i64, width: i64, y_coordinate: i64| VectorTileFeature {
            identifier: Some(identifier),
            geometry_type: VectorTileGeometryType::LineString,
            geometry: line(y_coordinate),
            attributes: vec![
                ("rank".to_string(), VectorTileAttributeValue::Signed(rank)),
                ("width".to_string(), VectorTileAttributeValue::Signed(width)),
            ],
        };
        VectorTile {
            layers: vec![VectorTileLayer {
                name: "roads".to_string(),
                extent: 4096,
                version: 2,
                features: vec![road(1, 2, 20, 100), road(2, 1, 10, 200)],
            }],
        }
    }

    fn route_polyline(overlay: &MapOverlay) -> &RoutePolylineOverlay {
        match overlay {
            MapOverlay::RoutePolyline(route_polyline) => route_polyline,
            other => panic!("expected a route polyline, got {other:?}"),
        }
    }

    #[test]
    fn line_sort_keys_order_features_and_paint_resolves_per_feature() {
        let style = sorted_line_style();
        let layer = style.layer("roads").expect("the roads layer compiles");
        let overlays = overlays_for_compiled_layer_at_zoom(
            &tile_with_two_roads(),
            VectorTileAddress {
                zoom_level: 0,
                tile_x: 0,
                tile_y: 0,
            },
            layer,
            0.0,
        );
        assert_eq!(overlays.len(), 2);
        // The lower sort key paints first even though its feature is second
        // in source order; every feature resolves its own width.
        assert_eq!(route_polyline(&overlays[0]).width_pixels, 10.0);
        assert_eq!(route_polyline(&overlays[1]).width_pixels, 20.0);
    }

    #[test]
    fn a_declared_line_presentation_becomes_a_styled_stroke() {
        let style = sorted_line_style();
        let layer = style.layer("roads").expect("the roads layer compiles");
        let overlays = overlays_for_compiled_layer_at_zoom(
            &tile_with_two_roads(),
            VectorTileAddress {
                zoom_level: 0,
                tile_x: 0,
                tile_y: 0,
            },
            layer,
            0.0,
        );
        let stroke = route_polyline(&overlays[0])
            .stroke
            .as_ref()
            .expect("the declared line-cap makes the stroke styled");
        assert_eq!(stroke.cap, rustic_maps_style::LineCap::Round);
        assert!(stroke.dash_array.is_empty());
    }

    /// Two polygon features at different latitudes, keyed by `rank`.
    fn tile_with_two_polygons() -> VectorTile {
        let polygon = |y_coordinate: i64| {
            VectorTileGeometry::Polygons(vec![VectorTileLinearRing {
                vertices: vec![
                    VectorTileGeometryPoint {
                        x_coordinate: 0,
                        y_coordinate,
                    },
                    VectorTileGeometryPoint {
                        x_coordinate: 100,
                        y_coordinate,
                    },
                    VectorTileGeometryPoint {
                        x_coordinate: 100,
                        y_coordinate: y_coordinate + 100,
                    },
                    VectorTileGeometryPoint {
                        x_coordinate: 0,
                        y_coordinate: y_coordinate + 100,
                    },
                ],
            }])
        };
        let feature = |identifier: u64, rank: i64, y_coordinate: i64| VectorTileFeature {
            identifier: Some(identifier),
            geometry_type: VectorTileGeometryType::Polygon,
            geometry: polygon(y_coordinate),
            attributes: vec![("rank".to_string(), VectorTileAttributeValue::Signed(rank))],
        };
        VectorTile {
            layers: vec![VectorTileLayer {
                name: "landcover".to_string(),
                extent: 4096,
                version: 2,
                features: vec![feature(1, 2, 100), feature(2, 1, 200)],
            }],
        }
    }

    /// Two point features at different latitudes, keyed by `rank`.
    fn tile_with_two_points() -> VectorTile {
        let point = |y_coordinate: i64| {
            VectorTileGeometry::Points(vec![VectorTileGeometryPoint {
                x_coordinate: 0,
                y_coordinate,
            }])
        };
        let feature = |identifier: u64, rank: i64, y_coordinate: i64| VectorTileFeature {
            identifier: Some(identifier),
            geometry_type: VectorTileGeometryType::Point,
            geometry: point(y_coordinate),
            attributes: vec![("rank".to_string(), VectorTileAttributeValue::Signed(rank))],
        };
        VectorTile {
            layers: vec![VectorTileLayer {
                name: "pois".to_string(),
                extent: 4096,
                version: 2,
                features: vec![feature(1, 2, 100), feature(2, 1, 200)],
            }],
        }
    }

    /// Compiles a one-layer style over the `base` source with the given
    /// layout, paint and layer type.
    fn single_layer_style(
        layer_type: &str,
        source_layer: &str,
        layout: &str,
        paint: &str,
    ) -> CompiledStyle {
        let document = MapStyleDocument::parse_json(&format!(
            r#"{{
                "version": 8,
                "sources": {{ "base": {{ "type": "vector" }} }},
                "layers": [
                    {{
                        "id": "layer",
                        "type": "{layer_type}",
                        "source": "base",
                        "source-layer": "{source_layer}",
                        "layout": {layout},
                        "paint": {paint}
                    }}
                ]
            }}"#
        ))
        .expect("the fixture style parses");
        compile_style_document(&document)
    }

    fn layer_overlays(style: &CompiledStyle, tile: &VectorTile) -> Vec<MapOverlay> {
        let layer = style.layer("layer").expect("the layer compiles");
        overlays_for_compiled_layer_at_zoom(
            tile,
            VectorTileAddress {
                zoom_level: 0,
                tile_x: 0,
                tile_y: 0,
            },
            layer,
            0.0,
        )
    }

    #[test]
    fn fill_sort_keys_order_features_within_the_layer() {
        let style = single_layer_style(
            "fill",
            "landcover",
            r#"{ "fill-sort-key": ["get", "rank"] }"#,
            r#"{ "fill-opacity": 0.5 }"#,
        );
        let overlays = layer_overlays(&style, &tile_with_two_polygons());
        assert_eq!(overlays.len(), 2);
        let first_latitude = match &overlays[0] {
            MapOverlay::FilledPolygon(fill) => fill.ring_coordinates[0].latitude,
            other => panic!("expected a filled polygon, got {other:?}"),
        };
        let second_latitude = match &overlays[1] {
            MapOverlay::FilledPolygon(fill) => fill.ring_coordinates[0].latitude,
            other => panic!("expected a filled polygon, got {other:?}"),
        };
        // The lower sort key paints first even though its feature is second
        // in source order, so the southern (lower-latitude) polygon leads.
        assert!(first_latitude < second_latitude);
    }

    #[test]
    fn circle_sort_keys_order_features_within_the_layer() {
        let style = single_layer_style(
            "circle",
            "pois",
            r#"{ "circle-sort-key": ["get", "rank"] }"#,
            r#"{ "circle-radius": 5 }"#,
        );
        let overlays = layer_overlays(&style, &tile_with_two_points());
        assert_eq!(overlays.len(), 2);
        let first_latitude = match &overlays[0] {
            MapOverlay::PointMarkers(markers) => markers.positions[0].latitude,
            other => panic!("expected point markers, got {other:?}"),
        };
        let second_latitude = match &overlays[1] {
            MapOverlay::PointMarkers(markers) => markers.positions[0].latitude,
            other => panic!("expected point markers, got {other:?}"),
        };
        assert!(first_latitude < second_latitude);
    }

    /// The neutral Mapbox-schema document and tile committed under
    /// `fixtures/mapbox/`: the Streets source-layer names and current
    /// attributes, authored for this repository.
    const MAPBOX_NEUTRAL_STYLE_DOCUMENT: &str =
        include_str!("../../../fixtures/mapbox/vendor_style_document.json");
    const MAPBOX_NEUTRAL_TILE_BYTES: &[u8] =
        include_bytes!("../../../fixtures/mapbox/vendor_style_tile.pbf");
    /// The neutral OMV-schema document and tile committed under
    /// `fixtures/here/`.
    const HERE_NEUTRAL_STYLE_DOCUMENT: &str =
        include_str!("../../../fixtures/here/vendor_style_document.json");
    const HERE_NEUTRAL_TILE_BYTES: &[u8] =
        include_bytes!("../../../fixtures/here/vendor_style_tile.mvt");

    const SPRITE_INDEX_BYTES: &[u8] =
        include_bytes!("../../rustic-maps-style/fixtures/sprites/icon-sprite.json");
    const SPRITE_SHEET_BYTES: &[u8] =
        include_bytes!("../../rustic-maps-style/fixtures/sprites/icon-sprite.png");

    /// The sprite icon's sheet rectangle holds at least one visible texel: the
    /// icon pass samples exactly these pixels.
    fn sprite_icon_has_visible_pixels(sprite_atlas: &crate::sprite::SpriteAtlas, icon_name: &str) {
        let icon = sprite_atlas
            .icon(icon_name)
            .unwrap_or_else(|| panic!("the committed sprite index declares {icon_name}"));
        let width = sprite_atlas.width_pixels();
        let mut visible_texels = 0;
        for y_offset in 0..icon.height_pixels {
            for x_offset in 0..icon.width_pixels {
                let x = icon.x_pixels + x_offset;
                let y = icon.y_pixels + y_offset;
                let alpha_index =
                    ((y * width + x) * crate::sprite::SPRITE_ATLAS_PIXEL_BYTES + 3) as usize;
                if sprite_atlas.rgba_pixels()[alpha_index] > 0 {
                    visible_texels += 1;
                }
            }
        }
        assert!(
            visible_texels > 0,
            "the {icon_name} icon's sheet rectangle carries visible pixels"
        );
    }

    #[test]
    fn the_mapbox_neutral_fixture_paints_roads_water_text_and_icons() {
        let style = vector_tile_style_from_json(MAPBOX_NEUTRAL_STYLE_DOCUMENT)
            .expect("the neutral Streets-schema document compiles");
        // The committed document keeps document-relative resource addresses so
        // the fixture server can serve them; the provider's `mapbox://` form is
        // covered by the provider and hosted-resource tests and by the live
        // smoke.
        assert_eq!(
            style.sprite.as_ref().map(|sprite| sprite.url.as_str()),
            Some("sprite")
        );
        assert_eq!(
            style.glyphs.as_deref(),
            Some("glyphs/{fontstack}/{range}.pbf")
        );
        for applied_property in [
            "text-padding",
            "text-pitch-alignment",
            "icon-text-fit",
            "icon-text-fit-padding",
            "icon-opacity",
            "line-translate",
            "line-translate-anchor",
        ] {
            assert!(
                !style
                    .unsupported
                    .iter()
                    .any(|diagnostic| diagnostic.property_name == applied_property),
                "{applied_property} is applied, not reported: {:?}",
                style.unsupported
            );
        }

        let tile = rustic_maps_vector_tiles::decode_vector_tile(MAPBOX_NEUTRAL_TILE_BYTES)
            .expect("the neutral Streets-schema tile decodes");
        let tile_address = VectorTileAddress {
            zoom_level: 14,
            tile_x: 2661,
            tile_y: 6286,
        };
        assert!(
            vector_style_source_layer_mismatches(&tile, &style).is_empty(),
            "the neutral tile carries every source layer the document reads"
        );
        let batches = overlay_primitives_for_vector_tile_at_zoom(&tile, tile_address, &style, 14.0);
        let road = batches
            .iter()
            .find(|batch| batch.layer_id == "road")
            .expect("the road layer paints from the tile");
        assert_eq!(road.overlays.len(), 2, "both road features paint");
        assert!(
            road.overlays
                .iter()
                .all(|overlay| matches!(overlay, MapOverlay::RoutePolyline(_))),
            "road features become polylines"
        );
        let water = batches
            .iter()
            .find(|batch| batch.layer_id == "water")
            .expect("the water layer paints from the tile");
        assert!(matches!(
            water.overlays.as_slice(),
            [MapOverlay::FilledPolygon(_)]
        ));

        let road_layer = style.layer("road").expect("the road layer compiles");
        let road_context = EvaluationContext::at_zoom(14.0);
        let ResolvedPaint::Line(road_paint) = road_layer.resolved_paint(&road_context) else {
            panic!("the road layer resolves line paint");
        };
        assert_eq!(road_paint.translate, [1.5, -1.0]);
        assert_eq!(
            road_paint.translate_anchor,
            rustic_maps_style::TranslateAnchor::Viewport
        );

        let candidates =
            crate::labels::label_candidates_for_vector_tile(&tile, tile_address, &style, 14.0);
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate.text == "Neutral Platz"),
            "the place label paints text"
        );
        let poi = candidates
            .iter()
            .find(|candidate| candidate.text == "Neutral Park")
            .expect("the poi label paints text");
        assert_eq!(
            poi.icon.as_ref().map(|icon| icon.image_name.as_str()),
            Some("park"),
            "the poi label resolves its sprite icon"
        );

        let sprite_atlas = crate::sprite::SpriteAtlas::from_index_and_sheet_png(
            SPRITE_INDEX_BYTES,
            SPRITE_SHEET_BYTES,
        )
        .expect("the committed sprite decodes");
        sprite_icon_has_visible_pixels(&sprite_atlas, "park");
    }

    #[test]
    fn the_here_neutral_fixture_paints_roads_water_text_and_icons() {
        let style = vector_tile_style_from_json(HERE_NEUTRAL_STYLE_DOCUMENT)
            .expect("the neutral OMV-schema document compiles");
        let tile = rustic_maps_vector_tiles::decode_vector_tile(HERE_NEUTRAL_TILE_BYTES)
            .expect("the neutral OMV-schema tile decodes");
        let tile_address = VectorTileAddress {
            zoom_level: 6,
            tile_x: 18,
            tile_y: 27,
        };
        assert!(
            vector_style_source_layer_mismatches(&tile, &style).is_empty(),
            "the neutral tile carries every source layer the document reads"
        );
        let batches = overlay_primitives_for_vector_tile_at_zoom(&tile, tile_address, &style, 6.0);
        let batch_ids: Vec<&str> = batches
            .iter()
            .map(|batch| batch.layer_id.as_str())
            .collect();
        for painted_layer in ["water", "roads"] {
            assert!(
                batch_ids.contains(&painted_layer),
                "{painted_layer} paints from the tile: {batch_ids:?}"
            );
        }
        let candidates =
            crate::labels::label_candidates_for_vector_tile(&tile, tile_address, &style, 6.0);
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate.text == "Neutral Ort"),
            "the places layer paints text"
        );
        let poi = candidates
            .iter()
            .find(|candidate| candidate.text == "Neutral Park")
            .expect("the pois layer paints text");
        assert_eq!(
            poi.icon.as_ref().map(|icon| icon.image_name.as_str()),
            Some("park")
        );
        let sprite_atlas = crate::sprite::SpriteAtlas::from_index_and_sheet_png(
            SPRITE_INDEX_BYTES,
            SPRITE_SHEET_BYTES,
        )
        .expect("the committed sprite decodes");
        sprite_icon_has_visible_pixels(&sprite_atlas, "park");
    }

    #[test]
    fn a_tile_from_another_schema_is_reported_as_a_named_mismatch() {
        // The OpenMapTiles fixture's source layers (`transportation`, `place`)
        // are not the Streets schema's; the mismatch names the layers instead
        // of painting fragments.
        let style = vector_tile_style_from_json(MAPBOX_NEUTRAL_STYLE_DOCUMENT)
            .expect("the neutral Streets-schema document compiles");
        let openmaptiles_tile = rustic_maps_vector_tiles::decode_vector_tile(include_bytes!(
            "../../../fixtures/openfreemap/openmaptiles_tile_z14_x2661_y6286.pbf"
        ))
        .expect("the OpenMapTiles fixture decodes");
        let mismatches = vector_style_source_layer_mismatches(&openmaptiles_tile, &style);
        let missing_layers: Vec<&str> = mismatches
            .iter()
            .map(|mismatch| mismatch.source_layer.as_str())
            .collect();
        for expected in ["road", "place_label", "poi_label"] {
            assert!(
                missing_layers.contains(&expected),
                "{expected} is reported absent: {missing_layers:?}"
            );
        }
        assert!(
            mismatches
                .iter()
                .all(|mismatch| mismatch.detail.contains("does not carry")),
            "each mismatch names the absent source layer"
        );
    }

    #[test]
    fn a_layer_that_declares_no_line_presentation_keeps_the_default_stroke() {
        let document = MapStyleDocument::parse_json(
            r#"{
                "version": 8,
                "sources": { "base": { "type": "vector" } },
                "layers": [
                    {
                        "id": "roads",
                        "type": "line",
                        "source": "base",
                        "source-layer": "roads",
                        "paint": { "line-width": 2 }
                    }
                ]
            }"#,
        )
        .expect("the fixture style parses");
        let style = compile_style_document(&document);
        let layer = style.layer("roads").expect("the roads layer compiles");
        assert!(!layer.declares_line_presentation);
        let overlays = overlays_for_compiled_layer_at_zoom(
            &tile_with_two_roads(),
            VectorTileAddress {
                zoom_level: 0,
                tile_x: 0,
                tile_y: 0,
            },
            layer,
            0.0,
        );
        assert!(route_polyline(&overlays[0]).stroke.is_none());
    }
}
