//! The version-8 MapLibre style document model.
//!
//! A document holds the root properties, the source declarations and the
//! ordered layer list. Layers carry a typed layout and paint struct whose
//! fields are [`Property`] values: a constant, or an expression resolved per
//! feature at the frame zoom.
//!
//! The parser deliberately degrades instead of rejecting an entire document
//! for one property it does not apply. Unknown and unimplemented properties
//! are collected in [`MapStyleDocument::unsupported`] with their layer or
//! source id, so a caller can report exactly what was skipped and a single
//! unsupported property can never blank the base map. Structural problems —
//! a missing `id`, an invalid filter, a malformed expression — are errors.

use std::collections::BTreeMap;

use serde_json::Value as JsonValue;

use crate::color::Color;
use crate::defaults::{
    DEFAULT_MAXIMUM_ZOOM, DEFAULT_MINIMUM_ZOOM, DEFAULT_SHIELD_BORDER_WIDTH_PIXELS,
    DEFAULT_SHIELD_FILL_COLOR, DEFAULT_SHIELD_MINIMUM_SIZE_PIXELS, DEFAULT_SHIELD_PADDING_PIXELS,
    DEFAULT_SHIELD_SHAPE, DEFAULT_SHIELD_TEXT_COLOR, DEFAULT_SHIELD_TEXT_SIZE_PIXELS,
    DEFAULT_TRANSITION_DELAY_MILLISECONDS, DEFAULT_TRANSITION_DURATION_MILLISECONDS,
    STYLE_DOCUMENT_VERSION,
};
use crate::error::{StyleError, UnsupportedProperty, UnsupportedScope};
use crate::expression::Expression;
use crate::filter::parse_filter;
use crate::types::{
    CirclePitchAlignment, CirclePitchScale, IconTextFit, LineCap, LineJoin, Property,
    PropertyValue, ShieldShape, ShieldSpec, SymbolPlacement, TextAnchor, TextJustify,
    TextPitchAlignment, TextRotationAlignment, TextTransform, TransitionSpec, TranslateAnchor,
    Visibility,
};

/// The projection the renderer supports.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Projection {
    /// Web-mercator, the only projection this SDK renders.
    #[default]
    Mercator,
    /// A projection the document asks for but this phase cannot render.
    Other(String),
}

/// One layer a vector tileset declares in its TileJSON `vector_layers`.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorLayerSpec {
    /// The source-layer name a style layer reads.
    pub id: String,
    /// The feature attributes the layer carries, mapped to their type names.
    pub fields: BTreeMap<String, String>,
    /// The lowest zoom the layer carries data at.
    pub minimum_zoom: Option<f64>,
    /// The highest zoom the layer carries data at.
    pub maximum_zoom: Option<f64>,
    /// The layer's human-readable description.
    pub description: Option<String>,
}

/// A vector source declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorSourceSpec {
    /// The TileJSON URL, when the source names one.
    pub url: Option<String>,
    /// Explicit tile URL templates, when the source names them.
    pub tiles: Vec<String>,
    /// The source's bounds as `[west, south, east, north]`.
    pub bounds: Option<[f64; 4]>,
    /// The tile scheme (`xyz` or `tms`).
    pub scheme: Option<String>,
    /// The source's minimum zoom.
    pub minimum_zoom: Option<f64>,
    /// The source's maximum zoom.
    pub maximum_zoom: Option<f64>,
    /// The attribution text the source declares.
    pub attribution: Option<String>,
    /// The feature-id property promotion rule.
    pub promote_id: Option<JsonValue>,
    /// Whether the source is volatile.
    pub volatile: Option<bool>,
    /// The tile encoding.
    pub encoding: Option<String>,
    /// The source layers the tileset declares, when it carries a TileJSON
    /// `vector_layers` list.
    pub vector_layers: Vec<VectorLayerSpec>,
}

/// A raster source declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct RasterSourceSpec {
    /// Explicit tile URL templates, when the source names them.
    pub tiles: Vec<String>,
    /// The source's bounds as `[west, south, east, north]`.
    pub bounds: Option<[f64; 4]>,
    /// The tile scheme (`xyz` or `tms`).
    pub scheme: Option<String>,
    /// The source's minimum zoom.
    pub minimum_zoom: Option<f64>,
    /// The source's maximum zoom.
    pub maximum_zoom: Option<f64>,
    /// The attribution text the source declares.
    pub attribution: Option<String>,
    /// The source's tile edge length, in pixels.
    pub tile_size: Option<f64>,
}

/// A GeoJSON source declaration (parsed for diagnostics; not rendered in this
/// phase).
#[derive(Debug, Clone, PartialEq)]
pub struct GeoJsonSourceSpec {
    /// The inline GeoJSON body, when the source carries one.
    pub data: Option<JsonValue>,
    /// The URL the GeoJSON body is fetched from, when the source names one.
    pub data_url: Option<String>,
    /// The attribution text the source declares.
    pub attribution: Option<String>,
    /// Whether feature ids are generated.
    pub generate_id: Option<bool>,
    /// The feature-id property promotion rule.
    pub promote_id: Option<JsonValue>,
    /// The tile buffer in pixels.
    pub buffer: Option<f64>,
    /// The simplification tolerance.
    pub tolerance: Option<f64>,
}

/// A source declaration of a type this crate does not model.
#[derive(Debug, Clone, PartialEq)]
pub struct OpaqueSourceSpec {
    /// The source's declared type (`raster-dem`, `image`, `video` or an
    /// unrecognized name).
    pub source_type: String,
    /// The declaration as it appeared in the document.
    pub definition: JsonValue,
}

/// One source the document declares.
#[derive(Debug, Clone, PartialEq)]
pub enum SourceSpec {
    /// A vector-tiles source.
    Vector(VectorSourceSpec),
    /// A raster-tile source.
    Raster(RasterSourceSpec),
    /// A GeoJSON source.
    GeoJson(GeoJsonSourceSpec),
    /// A source type this phase parses but does not model semantically.
    Opaque(OpaqueSourceSpec),
}

impl SourceSpec {
    /// The source's declared type name.
    pub fn source_type(&self) -> &str {
        match self {
            Self::Vector(_) => "vector",
            Self::Raster(_) => "raster",
            Self::GeoJson(_) => "geojson",
            Self::Opaque(opaque) => &opaque.source_type,
        }
    }
}

/// A layer's paint or geometry kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayerType {
    /// A flat background fill.
    Background,
    /// Polygon fill.
    Fill,
    /// Line geometry.
    Line,
    /// Circle markers.
    Circle,
    /// Text (and later icon) symbols.
    Symbol,
    /// Raster tiles.
    Raster,
    /// A heatmap.
    Heatmap,
    /// Fill extrusion.
    FillExtrusion,
    /// Hillshade.
    Hillshade,
    /// Colour relief.
    ColorRelief,
    /// A layer type the parser does not know.
    Unknown(String),
}

impl LayerType {
    /// The specification's spelling of the layer type.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Background => "background",
            Self::Fill => "fill",
            Self::Line => "line",
            Self::Circle => "circle",
            Self::Symbol => "symbol",
            Self::Raster => "raster",
            Self::Heatmap => "heatmap",
            Self::FillExtrusion => "fill-extrusion",
            Self::Hillshade => "hillshade",
            Self::ColorRelief => "color-relief",
            Self::Unknown(name) => name,
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        match name {
            "background" => Some(Self::Background),
            "fill" => Some(Self::Fill),
            "line" => Some(Self::Line),
            "circle" => Some(Self::Circle),
            "symbol" => Some(Self::Symbol),
            "raster" => Some(Self::Raster),
            "heatmap" => Some(Self::Heatmap),
            "fill-extrusion" => Some(Self::FillExtrusion),
            "hillshade" => Some(Self::Hillshade),
            "color-relief" => Some(Self::ColorRelief),
            _ => None,
        }
    }
}

/// A style's sprite declaration: the base URL its index document and sheet
/// are resolved from.
#[derive(Debug, Clone, PartialEq)]
pub struct SpriteSpec {
    /// The base URL the sprite index and sheet are resolved from.
    pub url: String,
}

/// A layer's layout properties.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LayerLayout {
    /// Whether the layer is drawn.
    pub visibility: Option<Property<Visibility>>,
    /// How line ends are drawn.
    pub line_cap: Option<Property<LineCap>>,
    /// How line segments meet.
    pub line_join: Option<Property<LineJoin>>,
    /// The order a line feature paints in within its layer; lower values
    /// paint first.
    pub line_sort_key: Option<Property<f64>>,
    /// The order a fill feature paints in within its layer; lower values
    /// paint first.
    pub fill_sort_key: Option<Property<f64>>,
    /// The order a circle feature paints in within its layer; lower values
    /// paint first.
    pub circle_sort_key: Option<Property<f64>>,
    /// Whether circle radius follows the ground plane as the map is pitched.
    pub circle_pitch_scale: Option<Property<CirclePitchScale>>,
    /// Whether circle geometry lies on the ground or faces the screen.
    pub circle_pitch_alignment: Option<Property<CirclePitchAlignment>>,
    /// A symbol layer's text field.
    pub text_field: Option<Property<String>>,
    /// A symbol layer's font stack, most preferred first.
    pub text_font: Option<Property<Vec<String>>>,
    /// A symbol layer's text size, in screen pixels.
    pub text_size: Option<Property<f64>>,
    /// A symbol layer's wrapping width, in ems.
    pub text_max_width: Option<Property<f64>>,
    /// The line height of wrapped text, in ems.
    pub text_line_height: Option<Property<f64>>,
    /// The extra space between glyphs, in ems.
    pub text_letter_spacing: Option<Property<f64>>,
    /// How wrapped lines are aligned within the text block.
    pub text_justify: Option<Property<TextJustify>>,
    /// Where the text block sits relative to the symbol anchor.
    pub text_anchor: Option<Property<TextAnchor>>,
    /// What a point-placed label's rotation follows.
    pub text_rotation_alignment: Option<Property<TextRotationAlignment>>,
    /// Whether a rotated label is flipped to read upright.
    pub text_keep_upright: Option<Property<bool>>,
    /// Candidate anchors a point label tries, in order.
    pub text_variable_anchor: Option<Property<Vec<TextAnchor>>>,
    /// The distance a variable-anchored label sits from its symbol, in ems.
    pub text_radial_offset: Option<Property<f64>>,
    /// The sharpest turn a line label's straight run tolerates, in degrees.
    pub text_max_angle: Option<Property<f64>>,
    /// A symbol layer's case transformation.
    pub text_transform: Option<Property<TextTransform>>,
    /// Where a symbol layer places its text.
    pub symbol_placement: Option<Property<SymbolPlacement>>,
    /// The distance between repeated line labels, in screen pixels.
    pub symbol_spacing: Option<Property<f64>>,
    /// Whether a symbol layer's text may overlap other labels.
    pub text_allow_overlap: Option<Property<bool>>,
    /// Whether a symbol layer's text ignores collision placement entirely.
    pub text_ignore_placement: Option<Property<bool>>,
    /// Whether labels that would cross the viewport edge are dropped.
    pub symbol_avoid_edges: Option<Property<bool>>,
    /// The text's displacement from its anchor, `[x, y]` in ems.
    pub text_offset: Option<Property<Vec<f64>>>,
    /// The transparent margin kept around the text's collision box, in screen
    /// pixels.
    pub text_padding: Option<Property<f64>>,
    /// What the text is aligned to when the map is pitched.
    pub text_pitch_alignment: Option<Property<TextPitchAlignment>>,
    /// The symbol layer's placement order key; lower values place first.
    pub symbol_sort_key: Option<Property<f64>>,
    /// The sprite icon a symbol layer draws, when it names one.
    pub icon_image: Option<Property<String>>,
    /// The icon's display scale relative to its sprite-sheet size.
    pub icon_size: Option<Property<f64>>,
    /// Where the icon sits relative to the symbol anchor.
    pub icon_anchor: Option<Property<TextAnchor>>,
    /// The icon's displacement from its anchor, `[x, y]` in ems.
    pub icon_offset: Option<Property<Vec<f64>>>,
    /// The icon's clockwise rotation in degrees.
    pub icon_rotate: Option<Property<f64>>,
    /// What the icon's rotation follows.
    pub icon_rotation_alignment: Option<Property<TextRotationAlignment>>,
    /// The transparent margin kept around the icon's collision box, in pixels.
    pub icon_padding: Option<Property<f64>>,
    /// Whether the icon may overlap other symbols.
    pub icon_allow_overlap: Option<Property<bool>>,
    /// Whether the icon ignores collision placement entirely.
    pub icon_ignore_placement: Option<Property<bool>>,
    /// How the icon is stretched to fit its text block.
    pub icon_text_fit: Option<Property<IconTextFit>>,
    /// The inset kept between a fitted icon and the text block,
    /// `[top, right, bottom, left]` in screen pixels.
    pub icon_text_fit_padding: Option<Property<Vec<f64>>>,
}

/// A layer's paint properties.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LayerPaint {
    /// The background fill colour.
    pub background_color: Option<Property<Color>>,
    /// The background opacity.
    pub background_opacity: Option<Property<f64>>,
    /// The fill colour.
    pub fill_color: Option<Property<Color>>,
    /// The fill opacity.
    pub fill_opacity: Option<Property<f64>>,
    /// The colour of the fill's edge; an absent value paints no explicit
    /// outline.
    pub fill_outline_color: Option<Property<Color>>,
    /// Whether the fill edge is antialiased.
    pub fill_antialias: Option<Property<bool>>,
    /// The fill's displacement from its geometry, in screen pixels.
    pub fill_translate: Option<Property<Vec<f64>>>,
    /// The frame of reference the fill's displacement is measured against.
    pub fill_translate_anchor: Option<TranslateAnchor>,
    /// The declared `fill-color-transition`.
    pub fill_color_transition: Option<TransitionSpec>,
    /// The declared `fill-opacity-transition`.
    pub fill_opacity_transition: Option<TransitionSpec>,
    /// The line colour.
    pub line_color: Option<Property<Color>>,
    /// The line width, in screen pixels.
    pub line_width: Option<Property<f64>>,
    /// The line opacity.
    pub line_opacity: Option<Property<f64>>,
    /// The declared `line-color-transition`.
    pub line_color_transition: Option<TransitionSpec>,
    /// The declared `line-opacity-transition`.
    pub line_opacity_transition: Option<TransitionSpec>,
    /// The line blur, in screen pixels.
    pub line_blur: Option<Property<f64>>,
    /// The distance the line is shifted perpendicular to its path, in screen
    /// pixels; a positive value shifts the line to the right of its direction.
    pub line_offset: Option<Property<f64>>,
    /// The width of the gap a line is drawn around, in screen pixels; the
    /// stroke paints outside the gap on both sides.
    pub line_gap_width: Option<Property<f64>>,
    /// The dash pattern, in line-width units.
    pub line_dasharray: Option<Property<Vec<f64>>>,
    /// The circle colour.
    pub circle_color: Option<Property<Color>>,
    /// The circle radius, in screen pixels.
    pub circle_radius: Option<Property<f64>>,
    /// The circle opacity.
    pub circle_opacity: Option<Property<f64>>,
    /// The declared `circle-color-transition`.
    pub circle_color_transition: Option<TransitionSpec>,
    /// The declared `circle-opacity-transition`.
    pub circle_opacity_transition: Option<TransitionSpec>,
    /// The colour of the circle's stroke; an absent value paints no stroke.
    pub circle_stroke_color: Option<Property<Color>>,
    /// The stroke width in screen pixels, drawn outside the circle radius.
    pub circle_stroke_width: Option<Property<f64>>,
    /// The stroke opacity.
    pub circle_stroke_opacity: Option<Property<f64>>,
    /// How much the circle's edge is softened, in screen pixels.
    pub circle_blur: Option<Property<f64>>,
    /// The circle's displacement from its anchor, in screen pixels.
    pub circle_translate: Option<Property<Vec<f64>>>,
    /// The text colour.
    pub text_color: Option<Property<Color>>,
    /// The text opacity.
    pub text_opacity: Option<Property<f64>>,
    /// The text halo colour.
    pub text_halo_color: Option<Property<Color>>,
    /// The text halo width, in screen pixels.
    pub text_halo_width: Option<Property<f64>>,
    /// The text halo blur, in screen pixels.
    pub text_halo_blur: Option<Property<f64>>,
    /// The icon opacity.
    pub icon_opacity: Option<Property<f64>>,
    /// The line's displacement from its geometry, in screen pixels.
    pub line_translate: Option<Property<Vec<f64>>>,
    /// The frame of reference the line's displacement is measured against.
    pub line_translate_anchor: Option<TranslateAnchor>,
}

/// One layer of a style document.
#[derive(Debug, Clone, PartialEq)]
pub struct StyleLayer {
    /// The layer's document-unique id.
    pub id: String,
    /// The layer's geometry kind.
    pub layer_type: LayerType,
    /// The source id the layer reads.
    pub source: Option<String>,
    /// The source layer the layer reads.
    pub source_layer: Option<String>,
    /// The lowest zoom (inclusive) the layer draws at.
    pub min_zoom: f64,
    /// The zoom (exclusive) the layer stops drawing at.
    pub max_zoom: f64,
    /// The layer's filter, when it declares one.
    pub filter: Option<Expression>,
    /// The layer's layout properties.
    pub layout: LayerLayout,
    /// The layer's paint properties.
    pub paint: LayerPaint,
    /// The route-reference badge a symbol layer draws, when it declares one.
    pub shield: Option<ShieldSpec>,
    /// The layer's free-form metadata.
    pub metadata: Option<JsonValue>,
}

impl StyleLayer {
    /// Whether the layer draws at `zoom`.
    pub fn is_visible_at_zoom(&self, zoom: f64) -> bool {
        zoom >= self.min_zoom && zoom < self.max_zoom
    }
}

/// A parsed version-8 style document.
#[derive(Debug, Clone, PartialEq)]
pub struct MapStyleDocument {
    /// The document version, always [`STYLE_DOCUMENT_VERSION`].
    pub version: u8,
    /// The optional style id.
    pub id: Option<String>,
    /// The optional style name.
    pub name: Option<String>,
    /// The optional free-form metadata.
    pub metadata: Option<JsonValue>,
    /// The optional initial center as `[longitude, latitude]`.
    pub center: Option<[f64; 2]>,
    /// The optional initial zoom.
    pub zoom: Option<f64>,
    /// The optional initial bearing.
    pub bearing: Option<f64>,
    /// The optional initial pitch.
    pub pitch: Option<f64>,
    /// The projection the document requests.
    pub projection: Projection,
    /// The source declarations, keyed by source id.
    pub sources: BTreeMap<String, SourceSpec>,
    /// The optional sprite definition.
    pub sprite: Option<SpriteSpec>,
    /// The optional glyph URL template.
    pub glyphs: Option<String>,
    /// The root transition timing that supplies defaults for paint
    /// properties that do not declare their own; `None` when the document
    /// declares none, which draws new values immediately.
    pub transition: Option<TransitionSpec>,
    /// The layers, in document order.
    pub layers: Vec<StyleLayer>,
    /// Properties the parser recognized or saw but did not apply.
    pub unsupported: Vec<UnsupportedProperty>,
}

impl MapStyleDocument {
    /// Parses a style document from JSON text.
    ///
    /// # Errors
    /// Invalid JSON and every structural document error report
    /// [`StyleError`].
    pub fn parse_json(json_text: &str) -> Result<Self, StyleError> {
        let json_value: JsonValue =
            serde_json::from_str(json_text).map_err(|json_error| StyleError::Json {
                detail: json_error.to_string(),
            })?;
        Self::from_json_value(&json_value)
    }

    /// Parses a style document from a JSON value.
    ///
    /// # Errors
    /// A non-object root, a version other than 8, a missing or malformed
    /// structural member, an invalid source, an invalid layer, an invalid
    /// filter or an invalid expression reports [`StyleError`].
    pub fn from_json_value(root: &JsonValue) -> Result<Self, StyleError> {
        let object = root.as_object().ok_or(StyleError::DocumentNotObject)?;
        let version = object
            .get("version")
            .and_then(JsonValue::as_u64)
            .ok_or_else(|| StyleError::Member {
                member: "version".to_string(),
                detail: "a style document must declare a numeric version".to_string(),
            })?;
        if version != u64::from(STYLE_DOCUMENT_VERSION) {
            return Err(StyleError::UnsupportedVersion { version });
        }

        let mut unsupported = Vec::new();
        collect_root_diagnostics(object, &mut unsupported);

        let sources = parse_sources(object.get("sources"), &mut unsupported)?;
        let layers_value = object.get("layers").ok_or_else(|| StyleError::Member {
            member: "layers".to_string(),
            detail: "a style document must declare a layers array".to_string(),
        })?;
        let layers_array = layers_value.as_array().ok_or_else(|| StyleError::Member {
            member: "layers".to_string(),
            detail: "layers must be an array".to_string(),
        })?;
        let mut layers = Vec::with_capacity(layers_array.len());
        for (layer_index, layer) in layers_array.iter().enumerate() {
            let mut parsed_layer = parse_layer(layer, layer_index, &mut unsupported)?;
            // A filter the engine cannot evaluate is reported once the layer
            // is parsed. A filter that evaluates to `null` is false, so the
            // layer admits every feature rather than rejecting all of them.
            if let Some(filter) = parsed_layer.filter.take() {
                let scope = UnsupportedScope::Layer {
                    layer_id: parsed_layer.id.clone(),
                };
                parsed_layer.filter = usable_filter_expression(filter, &scope, &mut unsupported);
            }
            layers.push(parsed_layer);
        }

        let projection = parse_projection(object.get("projection"), &mut unsupported);

        Ok(Self {
            version: STYLE_DOCUMENT_VERSION,
            id: object
                .get("id")
                .and_then(JsonValue::as_str)
                .map(str::to_string),
            name: object
                .get("name")
                .and_then(JsonValue::as_str)
                .map(str::to_string),
            metadata: object.get("metadata").cloned(),
            center: parse_center(object.get("center"), &mut unsupported),
            zoom: parse_optional_number(object.get("zoom"), "zoom", &mut unsupported),
            bearing: parse_optional_number(object.get("bearing"), "bearing", &mut unsupported),
            pitch: parse_optional_number(object.get("pitch"), "pitch", &mut unsupported),
            projection,
            sources,
            sprite: parse_sprite(object.get("sprite"), &mut unsupported),
            glyphs: object
                .get("glyphs")
                .and_then(JsonValue::as_str)
                .map(str::to_string),
            transition: parse_transition(object.get("transition"), &mut unsupported),
            layers,
            unsupported,
        })
    }

    /// The unsupported-property diagnostics that belong to one layer.
    pub fn unsupported_properties_on_layer(&self, layer_id: &str) -> Vec<&UnsupportedProperty> {
        self.unsupported
            .iter()
            .filter(|property| {
                matches!(
                    &property.scope,
                    UnsupportedScope::Layer { layer_id: scope_layer_id }
                        if scope_layer_id == layer_id
                )
            })
            .collect()
    }

    /// The unsupported-property diagnostics that belong to the document root.
    pub fn unsupported_document_properties(&self) -> Vec<&UnsupportedProperty> {
        self.unsupported
            .iter()
            .filter(|property| matches!(property.scope, UnsupportedScope::Document))
            .collect()
    }

    /// Whether any property was reported as unsupported.
    pub fn has_unsupported_properties(&self) -> bool {
        !self.unsupported.is_empty()
    }

    /// The layer with `layer_id`, when the document declares one.
    pub fn layer(&self, layer_id: &str) -> Option<&StyleLayer> {
        self.layers.iter().find(|layer| layer.id == layer_id)
    }
}

fn collect_root_diagnostics(
    object: &serde_json::Map<String, JsonValue>,
    unsupported: &mut Vec<UnsupportedProperty>,
) {
    for property_name in object.keys() {
        match property_name.as_str() {
            "version" | "id" | "name" | "metadata" | "center" | "zoom" | "bearing" | "pitch"
            | "projection" | "sources" | "sprite" | "glyphs" | "transition" | "layers" => {}
            "light" | "sky" | "terrain" => unsupported.push(UnsupportedProperty::new(
                UnsupportedScope::Document,
                property_name,
                "parsed but not rendered in this phase",
            )),
            _ => unsupported.push(UnsupportedProperty::new(
                UnsupportedScope::Document,
                property_name,
                "unknown top-level property",
            )),
        }
    }
}

fn parse_center(
    json_value: Option<&JsonValue>,
    unsupported: &mut Vec<UnsupportedProperty>,
) -> Option<[f64; 2]> {
    let json_value = json_value?;
    let coordinates = json_value.as_array()?;
    if coordinates.len() < 2 {
        unsupported.push(UnsupportedProperty::new(
            UnsupportedScope::Document,
            "center",
            "center must hold a longitude and a latitude",
        ));
        return None;
    }
    let longitude = coordinates[0].as_f64();
    let latitude = coordinates[1].as_f64();
    match (longitude, latitude) {
        (Some(longitude), Some(latitude)) => Some([longitude, latitude]),
        _ => {
            unsupported.push(UnsupportedProperty::new(
                UnsupportedScope::Document,
                "center",
                "center coordinates must be numbers",
            ));
            None
        }
    }
}

fn parse_optional_number(
    json_value: Option<&JsonValue>,
    property_name: &str,
    unsupported: &mut Vec<UnsupportedProperty>,
) -> Option<f64> {
    let json_value = json_value?;
    let number = json_value.as_f64();
    if number.is_none() {
        unsupported.push(UnsupportedProperty::new(
            UnsupportedScope::Document,
            property_name,
            "the property must be a number",
        ));
    }
    number
}

fn parse_projection(
    json_value: Option<&JsonValue>,
    unsupported: &mut Vec<UnsupportedProperty>,
) -> Projection {
    let Some(json_value) = json_value else {
        return Projection::Mercator;
    };
    let Some(projection_type) = json_value
        .as_object()
        .and_then(|object| object.get("type"))
        .and_then(JsonValue::as_str)
    else {
        unsupported.push(UnsupportedProperty::new(
            UnsupportedScope::Document,
            "projection",
            "projection must be an object with a type",
        ));
        return Projection::Mercator;
    };
    if projection_type == "mercator" {
        Projection::Mercator
    } else {
        unsupported.push(UnsupportedProperty::new(
            UnsupportedScope::Document,
            "projection",
            format!("projection {projection_type:?} is not rendered; only mercator is"),
        ));
        Projection::Other(projection_type.to_string())
    }
}

/// Parses the root `sprite` declaration. The specification allows a bare URL
/// or an object keyed by pixel ratio; a bare string and an object's `default`
/// member name the same sheet. An object without `default` is a diagnostic and
/// leaves the style without a sprite, so every other layer still draws. A
/// density-specific URL (`@2x`) is reported rather than silently ignored: the
/// host resolves the one URL the style declares.
fn parse_sprite(
    json_value: Option<&JsonValue>,
    unsupported: &mut Vec<UnsupportedProperty>,
) -> Option<SpriteSpec> {
    let json_value = json_value?;
    match json_value {
        JsonValue::String(url) => Some(SpriteSpec { url: url.clone() }),
        JsonValue::Object(object) => {
            for property_name in object.keys() {
                if property_name != "default" {
                    unsupported.push(UnsupportedProperty::new(
                        UnsupportedScope::Document,
                        "sprite",
                        format!("the {property_name:?} sprite URL is not loaded; only default is"),
                    ));
                }
            }
            match object.get("default").and_then(JsonValue::as_str) {
                Some(url) => Some(SpriteSpec {
                    url: url.to_string(),
                }),
                None => {
                    unsupported.push(UnsupportedProperty::new(
                        UnsupportedScope::Document,
                        "sprite",
                        "the sprite object must declare a default URL",
                    ));
                    None
                }
            }
        }
        _ => {
            unsupported.push(UnsupportedProperty::new(
                UnsupportedScope::Document,
                "sprite",
                "sprite must be a URL string or an object with a default URL",
            ));
            None
        }
    }
}

fn parse_sources(
    json_value: Option<&JsonValue>,
    unsupported: &mut Vec<UnsupportedProperty>,
) -> Result<BTreeMap<String, SourceSpec>, StyleError> {
    let Some(json_value) = json_value else {
        return Ok(BTreeMap::new());
    };
    let Some(object) = json_value.as_object() else {
        return Err(StyleError::Member {
            member: "sources".to_string(),
            detail: "sources must be an object keyed by source id".to_string(),
        });
    };
    let mut sources = BTreeMap::new();
    for (source_id, source_json) in object {
        sources.insert(
            source_id.clone(),
            parse_source(source_id, source_json, unsupported)?,
        );
    }
    Ok(sources)
}

fn parse_source(
    source_id: &str,
    json_value: &JsonValue,
    unsupported: &mut Vec<UnsupportedProperty>,
) -> Result<SourceSpec, StyleError> {
    let object = json_value.as_object().ok_or_else(|| StyleError::Source {
        source_id: source_id.to_string(),
        detail: "a source declaration must be an object".to_string(),
    })?;
    let source_type = object
        .get("type")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| StyleError::Source {
            source_id: source_id.to_string(),
            detail: "a source declaration must declare its type".to_string(),
        })?;
    let scope = UnsupportedScope::Source {
        source_id: source_id.to_string(),
    };
    match source_type {
        "vector" => {
            let mut specification = VectorSourceSpec {
                url: None,
                tiles: Vec::new(),
                bounds: None,
                scheme: None,
                minimum_zoom: None,
                maximum_zoom: None,
                attribution: None,
                promote_id: None,
                volatile: None,
                encoding: None,
                vector_layers: Vec::new(),
            };
            for (property_name, property_value) in object {
                match property_name.as_str() {
                    "type" => {}
                    "url" => specification.url = property_value.as_str().map(str::to_string),
                    "tiles" => {
                        specification.tiles = parse_tile_templates(property_value);
                    }
                    "bounds" => specification.bounds = parse_bounds(property_value),
                    "scheme" => specification.scheme = property_value.as_str().map(str::to_string),
                    "minzoom" => specification.minimum_zoom = property_value.as_f64(),
                    "maxzoom" => specification.maximum_zoom = property_value.as_f64(),
                    "attribution" => {
                        specification.attribution = property_value.as_str().map(str::to_string)
                    }
                    "promoteId" => specification.promote_id = Some(property_value.clone()),
                    "volatile" => specification.volatile = property_value.as_bool(),
                    "encoding" => {
                        specification.encoding = property_value.as_str().map(str::to_string)
                    }
                    "vector_layers" => {
                        specification.vector_layers =
                            parse_vector_layers(property_value, &scope, unsupported)
                    }
                    _ => unsupported.push(UnsupportedProperty::new(
                        scope.clone(),
                        property_name,
                        "unknown vector source property",
                    )),
                }
            }
            Ok(SourceSpec::Vector(specification))
        }
        "raster" => {
            let mut specification = RasterSourceSpec {
                tiles: Vec::new(),
                bounds: None,
                scheme: None,
                minimum_zoom: None,
                maximum_zoom: None,
                attribution: None,
                tile_size: None,
            };
            for (property_name, property_value) in object {
                match property_name.as_str() {
                    "type" => {}
                    "tiles" => specification.tiles = parse_tile_templates(property_value),
                    "bounds" => specification.bounds = parse_bounds(property_value),
                    "scheme" => specification.scheme = property_value.as_str().map(str::to_string),
                    "minzoom" => specification.minimum_zoom = property_value.as_f64(),
                    "maxzoom" => specification.maximum_zoom = property_value.as_f64(),
                    "attribution" => {
                        specification.attribution = property_value.as_str().map(str::to_string)
                    }
                    "tileSize" => specification.tile_size = property_value.as_f64(),
                    _ => unsupported.push(UnsupportedProperty::new(
                        scope.clone(),
                        property_name,
                        "unknown raster source property",
                    )),
                }
            }
            Ok(SourceSpec::Raster(specification))
        }
        "geojson" => {
            let mut specification = GeoJsonSourceSpec {
                data: None,
                data_url: None,
                attribution: None,
                generate_id: None,
                promote_id: None,
                buffer: None,
                tolerance: None,
            };
            for (property_name, property_value) in object {
                match property_name.as_str() {
                    "type" => {}
                    "data" => match property_value.as_str() {
                        Some(url) => specification.data_url = Some(url.to_string()),
                        None => specification.data = Some(property_value.clone()),
                    },
                    "attribution" => {
                        specification.attribution = property_value.as_str().map(str::to_string)
                    }
                    "generateId" => specification.generate_id = property_value.as_bool(),
                    "promoteId" => specification.promote_id = Some(property_value.clone()),
                    "buffer" => specification.buffer = property_value.as_f64(),
                    "tolerance" => specification.tolerance = property_value.as_f64(),
                    _ => unsupported.push(UnsupportedProperty::new(
                        scope.clone(),
                        property_name,
                        "unknown GeoJSON source property",
                    )),
                }
            }
            Ok(SourceSpec::GeoJson(specification))
        }
        other => {
            // `raster-dem`, `image` and `video` sources need terrain, image
            // placement or video decoding this renderer does not perform; an
            // unrecognized type is likewise carried only so its layers can be
            // named. Either way the source paints nothing and says so.
            unsupported.push(UnsupportedProperty::new(
                scope.clone(),
                "type",
                format!(
                    "the source type {other:?} is not rendered by this renderer; layers bound \
                     to it paint nothing"
                ),
            ));
            Ok(SourceSpec::Opaque(OpaqueSourceSpec {
                source_type: other.to_string(),
                definition: json_value.clone(),
            }))
        }
    }
}

fn parse_bounds(json_value: &JsonValue) -> Option<[f64; 4]> {
    let bounds: Vec<f64> = json_value
        .as_array()?
        .iter()
        .map(JsonValue::as_f64)
        .collect::<Option<Vec<f64>>>()?;
    match bounds.as_slice() {
        [west, south, east, north] => Some([*west, *south, *east, *north]),
        _ => None,
    }
}

/// The tile URL templates a source declares. A template that is not a string
/// is skipped rather than failing the document; the templates themselves are
/// never fetched by this crate.
fn parse_tile_templates(json_value: &JsonValue) -> Vec<String> {
    json_value
        .as_array()
        .map(|tiles| {
            tiles
                .iter()
                .filter_map(JsonValue::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Parses the TileJSON `vector_layers` list. A malformed entry is reported as
/// a source diagnostic and skipped, so the rest of the document still loads.
fn parse_vector_layers(
    json_value: &JsonValue,
    scope: &UnsupportedScope,
    unsupported: &mut Vec<UnsupportedProperty>,
) -> Vec<VectorLayerSpec> {
    let Some(layers) = json_value.as_array() else {
        unsupported.push(UnsupportedProperty::new(
            scope.clone(),
            "vector_layers",
            "vector_layers must be an array",
        ));
        return Vec::new();
    };
    let mut parsed_layers = Vec::with_capacity(layers.len());
    for layer in layers {
        let Some(layer_id) = layer.get("id").and_then(JsonValue::as_str) else {
            unsupported.push(UnsupportedProperty::new(
                scope.clone(),
                "vector_layers",
                "a vector layer must declare a string id",
            ));
            continue;
        };
        let fields = layer
            .get("fields")
            .and_then(JsonValue::as_object)
            .map(|object| {
                object
                    .iter()
                    .filter_map(|(field_name, field_type)| {
                        field_type
                            .as_str()
                            .map(|type_name| (field_name.clone(), type_name.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        parsed_layers.push(VectorLayerSpec {
            id: layer_id.to_string(),
            fields,
            minimum_zoom: layer.get("minzoom").and_then(JsonValue::as_f64),
            maximum_zoom: layer.get("maxzoom").and_then(JsonValue::as_f64),
            description: layer
                .get("description")
                .and_then(JsonValue::as_str)
                .map(str::to_string),
        });
    }
    parsed_layers
}

fn parse_layer(
    json_value: &JsonValue,
    layer_index: usize,
    unsupported: &mut Vec<UnsupportedProperty>,
) -> Result<StyleLayer, StyleError> {
    let object = json_value.as_object().ok_or_else(|| StyleError::Layer {
        layer_index,
        detail: "a layer must be an object".to_string(),
    })?;
    let id = object
        .get("id")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| StyleError::Layer {
            layer_index,
            detail: "a layer must declare a string id".to_string(),
        })?
        .to_string();
    let layer_type_name = object
        .get("type")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| StyleError::Layer {
            layer_index,
            detail: format!("layer {id:?} must declare a type"),
        })?;
    let scope = UnsupportedScope::Layer {
        layer_id: id.clone(),
    };
    let layer_type = match LayerType::from_name(layer_type_name) {
        Some(layer_type) => layer_type,
        None => {
            unsupported.push(UnsupportedProperty::new(
                scope.clone(),
                "type",
                format!("unknown layer type {layer_type_name:?}"),
            ));
            LayerType::Unknown(layer_type_name.to_string())
        }
    };

    for property_name in object.keys() {
        match property_name.as_str() {
            "id" | "type" | "source" | "source-layer" | "minzoom" | "maxzoom" | "filter"
            | "layout" | "paint" | "shield" | "metadata" => {}
            _ => unsupported.push(UnsupportedProperty::new(
                scope.clone(),
                property_name,
                "unknown layer property",
            )),
        }
    }

    let filter = match object.get("filter") {
        None | Some(JsonValue::Null) => None,
        Some(filter_json) => Some(parse_filter(filter_json)?),
    };
    let is_symbol_layer = layer_type == LayerType::Symbol;

    Ok(StyleLayer {
        id,
        layer_type,
        source: object
            .get("source")
            .and_then(JsonValue::as_str)
            .map(str::to_string),
        source_layer: object
            .get("source-layer")
            .and_then(JsonValue::as_str)
            .map(str::to_string),
        min_zoom: object
            .get("minzoom")
            .and_then(JsonValue::as_f64)
            .unwrap_or(DEFAULT_MINIMUM_ZOOM),
        max_zoom: object
            .get("maxzoom")
            .and_then(JsonValue::as_f64)
            .unwrap_or(DEFAULT_MAXIMUM_ZOOM),
        filter,
        layout: parse_layout(object.get("layout"), &scope, unsupported),
        paint: parse_paint(object.get("paint"), &scope, unsupported),
        shield: if is_symbol_layer {
            parse_shield(object.get("shield"), &scope, unsupported)
        } else {
            // A badge belongs to the symbol pipeline; a block on a fill,
            // line, circle, raster or background layer is reported and
            // ignored rather than carried as data no renderer reads.
            if object
                .get("shield")
                .is_some_and(|shield_value| !shield_value.is_null())
            {
                unsupported.push(UnsupportedProperty::new(
                    scope.clone(),
                    "shield",
                    "a shield block is only valid on a symbol layer",
                ));
            }
            None
        },
        metadata: object.get("metadata").cloned(),
    })
}

fn parse_layout(
    json_value: Option<&JsonValue>,
    scope: &UnsupportedScope,
    unsupported: &mut Vec<UnsupportedProperty>,
) -> LayerLayout {
    let mut layout = LayerLayout::default();
    let Some(json_value) = json_value else {
        return layout;
    };
    let Some(object) = json_value.as_object() else {
        unsupported.push(UnsupportedProperty::new(
            scope.clone(),
            "layout",
            "layout must be an object",
        ));
        return layout;
    };
    for (property_name, property_value) in object {
        match property_name.as_str() {
            "visibility" => {
                layout.visibility =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "line-cap" => {
                layout.line_cap = parse_property(property_value, scope, unsupported, property_name)
            }
            "line-join" => {
                layout.line_join = parse_property(property_value, scope, unsupported, property_name)
            }
            "line-sort-key" => {
                layout.line_sort_key =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "fill-sort-key" => {
                layout.fill_sort_key =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "circle-sort-key" => {
                layout.circle_sort_key =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "circle-pitch-scale" => {
                layout.circle_pitch_scale =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "circle-pitch-alignment" => {
                layout.circle_pitch_alignment =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "text-field" => {
                layout.text_field =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "text-font" => {
                layout.text_font = parse_property(property_value, scope, unsupported, property_name)
            }
            "text-size" => {
                layout.text_size = parse_property(property_value, scope, unsupported, property_name)
            }
            "text-max-width" => {
                layout.text_max_width =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "text-line-height" => {
                layout.text_line_height =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "text-letter-spacing" => {
                layout.text_letter_spacing =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "text-justify" => {
                layout.text_justify =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "text-anchor" => {
                layout.text_anchor =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "text-rotation-alignment" => {
                layout.text_rotation_alignment =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "text-keep-upright" => {
                layout.text_keep_upright =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "text-variable-anchor" => {
                layout.text_variable_anchor =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "text-radial-offset" => {
                layout.text_radial_offset =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "text-max-angle" => {
                layout.text_max_angle =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "text-transform" => {
                layout.text_transform =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "symbol-placement" => {
                layout.symbol_placement =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "symbol-spacing" => {
                layout.symbol_spacing =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "text-allow-overlap" => {
                layout.text_allow_overlap =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "text-ignore-placement" => {
                layout.text_ignore_placement =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "symbol-avoid-edges" => {
                layout.symbol_avoid_edges =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "text-offset" => {
                layout.text_offset =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "text-padding" => {
                layout.text_padding =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "text-pitch-alignment" => {
                layout.text_pitch_alignment =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "symbol-sort-key" => {
                layout.symbol_sort_key =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "icon-image" => {
                layout.icon_image =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "icon-size" => {
                layout.icon_size = parse_property(property_value, scope, unsupported, property_name)
            }
            "icon-anchor" => {
                layout.icon_anchor =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "icon-offset" => {
                layout.icon_offset =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "icon-rotate" => {
                layout.icon_rotate =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "icon-rotation-alignment" => {
                layout.icon_rotation_alignment =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "icon-padding" => {
                layout.icon_padding =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "icon-allow-overlap" => {
                layout.icon_allow_overlap =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "icon-ignore-placement" => {
                layout.icon_ignore_placement =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "icon-text-fit" => {
                layout.icon_text_fit =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "icon-text-fit-padding" => {
                layout.icon_text_fit_padding =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            _ => unsupported.push(UnsupportedProperty::new(
                scope.clone(),
                property_name,
                "unimplemented layout property",
            )),
        }
    }
    layout
}

fn parse_paint(
    json_value: Option<&JsonValue>,
    scope: &UnsupportedScope,
    unsupported: &mut Vec<UnsupportedProperty>,
) -> LayerPaint {
    let mut paint = LayerPaint::default();
    let Some(json_value) = json_value else {
        return paint;
    };
    let Some(object) = json_value.as_object() else {
        unsupported.push(UnsupportedProperty::new(
            scope.clone(),
            "paint",
            "paint must be an object",
        ));
        return paint;
    };
    for (property_name, property_value) in object {
        match property_name.as_str() {
            "background-color" => {
                paint.background_color =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "background-opacity" => {
                paint.background_opacity =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "fill-color" => {
                paint.fill_color = parse_property(property_value, scope, unsupported, property_name)
            }
            "fill-opacity" => {
                paint.fill_opacity =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "fill-outline-color" => {
                paint.fill_outline_color =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "fill-antialias" => {
                paint.fill_antialias =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "fill-translate" => {
                paint.fill_translate =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "fill-translate-anchor" => {
                paint.fill_translate_anchor =
                    parse_translate_anchor(property_value, scope, unsupported, property_name)
            }
            // A pattern fill needs a tiled sprite decode this renderer does not
            // perform, so the fill degrades to its colour and the diagnostic
            // names the property instead of leaving the drop implicit.
            "fill-pattern" => unsupported.push(UnsupportedProperty::new(
                scope.clone(),
                property_name,
                "fill-pattern is not painted; the fill draws its colour",
            )),
            "fill-color-transition" => {
                paint.fill_color_transition =
                    parse_transition_block(property_value, scope, unsupported, property_name)
            }
            "fill-opacity-transition" => {
                paint.fill_opacity_transition =
                    parse_transition_block(property_value, scope, unsupported, property_name)
            }
            "line-color" => {
                paint.line_color = parse_property(property_value, scope, unsupported, property_name)
            }
            "line-width" => {
                paint.line_width = parse_property(property_value, scope, unsupported, property_name)
            }
            "line-opacity" => {
                paint.line_opacity =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "line-color-transition" => {
                paint.line_color_transition =
                    parse_transition_block(property_value, scope, unsupported, property_name)
            }
            "line-opacity-transition" => {
                paint.line_opacity_transition =
                    parse_transition_block(property_value, scope, unsupported, property_name)
            }
            "line-blur" => {
                paint.line_blur = parse_property(property_value, scope, unsupported, property_name)
            }
            "line-offset" => {
                paint.line_offset =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "line-gap-width" => {
                paint.line_gap_width =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "line-dasharray" => {
                paint.line_dasharray =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "line-translate" => {
                paint.line_translate =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "line-translate-anchor" => {
                paint.line_translate_anchor =
                    parse_translate_anchor(property_value, scope, unsupported, property_name)
            }
            "circle-color" => {
                paint.circle_color =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "circle-radius" => {
                paint.circle_radius =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "circle-opacity" => {
                paint.circle_opacity =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "circle-color-transition" => {
                paint.circle_color_transition =
                    parse_transition_block(property_value, scope, unsupported, property_name)
            }
            "circle-opacity-transition" => {
                paint.circle_opacity_transition =
                    parse_transition_block(property_value, scope, unsupported, property_name)
            }
            "circle-stroke-color" => {
                paint.circle_stroke_color =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "circle-stroke-width" => {
                paint.circle_stroke_width =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "circle-stroke-opacity" => {
                paint.circle_stroke_opacity =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "circle-blur" => {
                paint.circle_blur =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "circle-translate" => {
                paint.circle_translate =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "text-color" => {
                paint.text_color = parse_property(property_value, scope, unsupported, property_name)
            }
            "text-opacity" => {
                paint.text_opacity =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "text-halo-color" => {
                paint.text_halo_color =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "text-halo-width" => {
                paint.text_halo_width =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "text-halo-blur" => {
                paint.text_halo_blur =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            "icon-opacity" => {
                paint.icon_opacity =
                    parse_property(property_value, scope, unsupported, property_name)
            }
            _ => unsupported.push(UnsupportedProperty::new(
                scope.clone(),
                property_name,
                "unimplemented paint property",
            )),
        }
    }
    paint
}

/// Parses the root `transition` block. An absent value declares no
/// transition; a non-object value is reported and treated as absent.
fn parse_transition(
    json_value: Option<&JsonValue>,
    unsupported: &mut Vec<UnsupportedProperty>,
) -> Option<TransitionSpec> {
    let json_value = json_value?;
    parse_transition_body(
        json_value,
        &UnsupportedScope::Document,
        unsupported,
        "transition",
    )
}

/// Parses one property's `*-transition` block. A malformed block is reported
/// and treated as absent, so the property draws its new value immediately
/// instead of silently holding the old one.
fn parse_transition_block(
    json_value: &JsonValue,
    scope: &UnsupportedScope,
    unsupported: &mut Vec<UnsupportedProperty>,
    property_name: &str,
) -> Option<TransitionSpec> {
    parse_transition_body(json_value, scope, unsupported, property_name)
}

/// The shared `{duration, delay}` reader. Omitted members take the
/// specification's defaults; a non-number is reported and takes its default.
fn parse_transition_body(
    json_value: &JsonValue,
    scope: &UnsupportedScope,
    unsupported: &mut Vec<UnsupportedProperty>,
    property_name: &str,
) -> Option<TransitionSpec> {
    let Some(object) = json_value.as_object() else {
        unsupported.push(UnsupportedProperty::new(
            scope.clone(),
            property_name,
            "a transition must be an object with duration and delay",
        ));
        return None;
    };
    let duration_milliseconds = parse_transition_member(
        object.get("duration"),
        DEFAULT_TRANSITION_DURATION_MILLISECONDS,
        scope,
        unsupported,
        property_name,
    );
    let delay_milliseconds = parse_transition_member(
        object.get("delay"),
        DEFAULT_TRANSITION_DELAY_MILLISECONDS,
        scope,
        unsupported,
        property_name,
    );
    Some(TransitionSpec {
        duration_milliseconds: duration_milliseconds.max(0.0),
        delay_milliseconds: delay_milliseconds.max(0.0),
    })
}

/// One numeric transition member, or its documented default when absent or
/// malformed.
fn parse_transition_member(
    json_value: Option<&JsonValue>,
    default_milliseconds: f64,
    scope: &UnsupportedScope,
    unsupported: &mut Vec<UnsupportedProperty>,
    property_name: &str,
) -> f64 {
    let Some(json_value) = json_value else {
        return default_milliseconds;
    };
    match json_value.as_f64() {
        Some(value) if value.is_finite() => value,
        _ => {
            unsupported.push(UnsupportedProperty::new(
                scope.clone(),
                property_name,
                "a transition duration and delay must be finite numbers",
            ));
            default_milliseconds
        }
    }
}

/// Parses a translate anchor constant. The property is not expression-capable
/// in the specification, so an expression is reported rather than evaluated.
fn parse_translate_anchor(
    json_value: &JsonValue,
    scope: &UnsupportedScope,
    unsupported: &mut Vec<UnsupportedProperty>,
    property_name: &str,
) -> Option<TranslateAnchor> {
    json_value
        .as_str()
        .and_then(TranslateAnchor::from_name)
        .or_else(|| {
            unsupported.push(UnsupportedProperty::new(
                scope.clone(),
                property_name,
                "a translate anchor must be \"map\" or \"viewport\"",
            ));
            None
        })
}

/// A filter this renderer can evaluate, or `None` when it names an
/// unimplemented operator.
///
/// A filter that evaluates to `null` is false, so keeping an unimplemented
/// filter would reject every feature and blank the layer. The diagnostic names
/// the operator and the layer admits every feature instead.
fn usable_filter_expression(
    filter: Expression,
    scope: &UnsupportedScope,
    unsupported: &mut Vec<UnsupportedProperty>,
) -> Option<Expression> {
    if let Some(operator) = filter.first_unsupported_operator() {
        unsupported.push(UnsupportedProperty::new(
            scope.clone(),
            "filter",
            format!(
                "the filter operator {operator:?} is not implemented; the layer admits every feature"
            ),
        ));
        return None;
    }
    if filter.uses_feature_state() {
        unsupported.push(UnsupportedProperty::new(
            scope.clone(),
            "filter",
            "a feature-state filter is not implemented; the layer admits every feature",
        ));
        return None;
    }
    Some(filter)
}

fn parse_property<T: PropertyValue>(
    json_value: &JsonValue,
    scope: &UnsupportedScope,
    unsupported: &mut Vec<UnsupportedProperty>,
    property_name: &str,
) -> Option<Property<T>> {
    match Property::parse(json_value) {
        Ok(Property::Expression(expression)) => {
            if let Some(operator) = expression.first_unsupported_operator() {
                unsupported.push(UnsupportedProperty::new(
                    scope.clone(),
                    property_name,
                    format!(
                        "the expression operator {operator:?} is not implemented; the property \
                         takes its default"
                    ),
                ));
                return None;
            }
            if expression.uses_feature_state() {
                unsupported.push(UnsupportedProperty::new(
                    scope.clone(),
                    property_name,
                    "feature-state styling is not implemented; the property takes its default",
                ));
                return None;
            }
            match expression.validate_zoom_placement() {
                Ok(()) => Some(Property::Expression(expression)),
                Err(zoom_error) => {
                    unsupported.push(UnsupportedProperty::new(
                        scope.clone(),
                        property_name,
                        format!("expression disabled: {zoom_error}"),
                    ));
                    None
                }
            }
        }
        Ok(constant) => Some(constant),
        Err(parse_error) => {
            unsupported.push(UnsupportedProperty::new(
                scope.clone(),
                property_name,
                format!("property disabled: {parse_error}"),
            ));
            None
        }
    }
}

/// The smallest shield measurement the parser accepts, in screen pixels. A
/// negative declaration clamps up to it.
const MINIMUM_SHIELD_MEASUREMENT_PIXELS: f64 = 0.0;

/// The largest badge width or height a shield block may declare, in screen
/// pixels. A badge is drawn in screen space, so a larger declaration is
/// clamped before geometry is tessellated.
const MAXIMUM_SHIELD_DIMENSION_PIXELS: f64 = 4096.0;

/// The largest reference text size a shield block may declare, in screen
/// pixels.
const MAXIMUM_SHIELD_TEXT_SIZE_PIXELS: f64 = 512.0;

/// The largest inset a shield block may declare between the reference and the
/// badge edge, in screen pixels.
const MAXIMUM_SHIELD_PADDING_PIXELS: f64 = 512.0;

/// The largest outline width a shield block may declare, in screen pixels.
const MAXIMUM_SHIELD_BORDER_WIDTH_PIXELS: f64 = 256.0;

/// Parses a symbol layer's `shield` block.
///
/// Every member is a constant, so the block needs no expression tree. A
/// member that is absent takes its documented default; a member of the wrong
/// type is reported as unsupported and the default stands, so one malformed
/// badge cannot blank the layer. A measurement outside its documented bounds
/// is clamped and reported, so a hostile or mistaken document cannot produce
/// non-finite or overflowing badge geometry. A `shield` value that is not an
/// object reports the same and paints no badge.
fn parse_shield(
    json_value: Option<&JsonValue>,
    scope: &UnsupportedScope,
    unsupported: &mut Vec<UnsupportedProperty>,
) -> Option<ShieldSpec> {
    let json_value = json_value?;
    let Some(object) = json_value.as_object() else {
        unsupported.push(UnsupportedProperty::new(
            scope.clone(),
            "shield",
            "shield must be an object",
        ));
        return None;
    };
    let mut shield = ShieldSpec {
        shape: DEFAULT_SHIELD_SHAPE,
        fill_color: DEFAULT_SHIELD_FILL_COLOR,
        border_color: None,
        border_width_pixels: DEFAULT_SHIELD_BORDER_WIDTH_PIXELS,
        text_color: DEFAULT_SHIELD_TEXT_COLOR,
        text_size_pixels: DEFAULT_SHIELD_TEXT_SIZE_PIXELS,
        padding_pixels: DEFAULT_SHIELD_PADDING_PIXELS,
        minimum_size_pixels: DEFAULT_SHIELD_MINIMUM_SIZE_PIXELS,
    };
    for (member_name, member_value) in object {
        let property_name = format!("shield.{member_name}");
        match member_name.as_str() {
            "shape" => match member_value.as_str().and_then(ShieldShape::from_name) {
                Some(shape) => shield.shape = shape,
                None => unsupported.push(UnsupportedProperty::new(
                    scope.clone(),
                    &property_name,
                    format!("unknown shield shape {member_value}"),
                )),
            },
            "fill_rgba" => match Color::from_json_constant(member_value) {
                Some(fill_color) => shield.fill_color = fill_color,
                None => unsupported.push(UnsupportedProperty::new(
                    scope.clone(),
                    &property_name,
                    "fill_rgba is not a colour",
                )),
            },
            "border_rgba" => match Color::from_json_constant(member_value) {
                Some(border_color) => shield.border_color = Some(border_color),
                None => unsupported.push(UnsupportedProperty::new(
                    scope.clone(),
                    &property_name,
                    "border_rgba is not a colour",
                )),
            },
            "text_rgba" => match Color::from_json_constant(member_value) {
                Some(text_color) => shield.text_color = text_color,
                None => unsupported.push(UnsupportedProperty::new(
                    scope.clone(),
                    &property_name,
                    "text_rgba is not a colour",
                )),
            },
            "border_width_pixels" => {
                if let Some(border_width_pixels) = bounded_shield_measurement(
                    member_value,
                    &property_name,
                    MAXIMUM_SHIELD_BORDER_WIDTH_PIXELS,
                    scope,
                    unsupported,
                ) {
                    shield.border_width_pixels = border_width_pixels;
                }
            }
            "text_size_pixels" => {
                if let Some(text_size_pixels) = bounded_shield_measurement(
                    member_value,
                    &property_name,
                    MAXIMUM_SHIELD_TEXT_SIZE_PIXELS,
                    scope,
                    unsupported,
                ) {
                    shield.text_size_pixels = text_size_pixels;
                }
            }
            "padding_pixels" => {
                if let Some(padding_pixels) = bounded_shield_measurement(
                    member_value,
                    &property_name,
                    MAXIMUM_SHIELD_PADDING_PIXELS,
                    scope,
                    unsupported,
                ) {
                    shield.padding_pixels = padding_pixels;
                }
            }
            "minimum_size_pixels" => {
                if let Some(minimum_size_pixels) = bounded_shield_measurement(
                    member_value,
                    &property_name,
                    MAXIMUM_SHIELD_DIMENSION_PIXELS,
                    scope,
                    unsupported,
                ) {
                    shield.minimum_size_pixels = minimum_size_pixels;
                }
            }
            _ => unsupported.push(UnsupportedProperty::new(
                scope.clone(),
                &property_name,
                "unknown shield property",
            )),
        }
    }
    Some(shield)
}

/// A shield member's number, clamped into
/// [`MINIMUM_SHIELD_MEASUREMENT_PIXELS`]`..=`maximum`.
///
/// A non-finite value reports an unsupported property and leaves the member's
/// default in force. A finite value outside the bounds is clamped and
/// reported, so the badge geometry stays finite and bounded.
fn bounded_shield_measurement(
    member_value: &JsonValue,
    property_name: &str,
    maximum: f64,
    scope: &UnsupportedScope,
    unsupported: &mut Vec<UnsupportedProperty>,
) -> Option<f64> {
    let Some(number) = member_value.as_f64().filter(|number| number.is_finite()) else {
        unsupported.push(UnsupportedProperty::new(
            scope.clone(),
            property_name,
            format!("{property_name} is not a finite number"),
        ));
        return None;
    };
    let clamped = number.clamp(MINIMUM_SHIELD_MEASUREMENT_PIXELS, maximum);
    if clamped != number {
        unsupported.push(UnsupportedProperty::new(
            scope.clone(),
            property_name,
            format!(
                "{property_name} is outside {MINIMUM_SHIELD_MEASUREMENT_PIXELS}..={maximum} and was clamped"
            ),
        ));
    }
    Some(clamped)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIAGNOSTIC_STYLE_JSON: &str = r##"{
        "version": 8,
        "name": "diagnostics",
        "mystery-root": 1,
        "sources": {
            "base": {
                "type": "vector",
                "url": "https://example.test/tiles.json",
                "mystery-source": true
            }
        },
        "layers": [
            {
                "id": "water",
                "type": "fill",
                "source": "base",
                "source-layer": "water",
                "filter": ["==", "class", "water"],
                "layout": { "visibility": "visible", "fill-antialias": true },
                "paint": { "fill-color": "#ffffff", "fill-pattern": "waves" },
                "mystery-layer": 1
            },
            {
                "id": "roads",
                "type": "line",
                "source": "base",
                "paint": { "line-width": 2 }
            }
        ]
    }"##;

    fn property_names(properties: &[&UnsupportedProperty]) -> Vec<String> {
        let mut names: Vec<String> = properties
            .iter()
            .map(|property| property.property_name.clone())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn unknown_properties_are_reported_and_the_rest_of_the_document_survives() {
        let document = MapStyleDocument::parse_json(DIAGNOSTIC_STYLE_JSON)
            .expect("the document parses with diagnostics");
        assert_eq!(document.layers.len(), 2);
        assert_eq!(
            property_names(&document.unsupported_document_properties()),
            ["mystery-root"]
        );
        assert_eq!(
            property_names(&document.unsupported_properties_on_layer("water")),
            ["fill-antialias", "fill-pattern", "mystery-layer"]
        );
        assert!(document.unsupported_properties_on_layer("roads").is_empty());

        let water = document.layer("water").expect("the water layer parses");
        assert!(water.paint.fill_color.is_some());
        assert!(water.filter.is_some());
        let roads = document.layer("roads").expect("the roads layer parses");
        assert_eq!(
            roads
                .paint
                .line_width
                .as_ref()
                .expect("line-width")
                .evaluate(&crate::expression::EvaluationContext::at_zoom(10.0))
                .expect("line-width evaluates"),
            2.0
        );
        assert!(document.sources.contains_key("base"));
        assert!(document.unsupported.iter().any(|property| {
            property.property_name == "mystery-source"
                && matches!(
                    &property.scope,
                    UnsupportedScope::Source { source_id } if source_id == "base"
                )
        }));
    }

    #[test]
    fn only_version_eight_is_accepted() {
        let error = MapStyleDocument::parse_json(r#"{"version": 7, "layers": []}"#)
            .expect_err("version 7 is rejected");
        assert_eq!(error, StyleError::UnsupportedVersion { version: 7 });
    }

    #[test]
    fn a_layer_without_an_id_is_a_structural_error() {
        let error = MapStyleDocument::parse_json(r#"{"version": 8, "layers": [{"type": "fill"}]}"#)
            .expect_err("a layer id is required");
        assert!(matches!(error, StyleError::Layer { layer_index: 0, .. }));
    }

    #[test]
    fn a_malformed_expression_disables_only_its_property() {
        let document = MapStyleDocument::parse_json(
            r##"{
                "version": 8,
                "layers": [{
                    "id": "broken",
                    "type": "line",
                    "paint": {
                        "line-width": ["interpolate", ["linear"], ["zoom"], 0, 1, 0, 2],
                        "line-color": "#000000"
                    }
                }]
            }"##,
        )
        .expect("the document still parses");
        let layer = document.layer("broken").expect("the layer parses");
        assert!(layer.paint.line_width.is_none());
        assert!(layer.paint.line_color.is_some());
        assert_eq!(
            property_names(&document.unsupported_properties_on_layer("broken")),
            ["line-width"]
        );
    }

    #[test]
    fn an_unimplemented_expression_operator_disables_only_its_property() {
        let document = MapStyleDocument::parse_json(
            r##"{
                "version": 8,
                "layers": [{
                    "id": "roads",
                    "type": "line",
                    "paint": {
                        "line-width": ["distance", ["get", "width"]],
                        "line-color": "#000000"
                    }
                }]
            }"##,
        )
        .expect("the document still parses");
        let layer = document.layer("roads").expect("the road layer parses");
        assert!(layer.paint.line_width.is_none());
        assert!(layer.paint.line_color.is_some());
        assert_eq!(
            property_names(&document.unsupported_properties_on_layer("roads")),
            ["line-width"]
        );
        let diagnostic = document
            .unsupported_properties_on_layer("roads")
            .into_iter()
            .next()
            .expect("the operator is named");
        assert!(
            diagnostic.detail.contains("distance"),
            "the diagnostic names the operator: {}",
            diagnostic.detail
        );
    }

    #[test]
    fn a_feature_state_property_is_reported_and_takes_its_default() {
        let document = MapStyleDocument::parse_json(
            r##"{
                "version": 8,
                "layers": [{
                    "id": "highlight",
                    "type": "fill",
                    "paint": {
                        "fill-color": "red",
                        "fill-opacity": ["case", ["feature-state", "hover"], 1, 0.2]
                    }
                }]
            }"##,
        )
        .expect("the document still parses");
        let layer = document.layer("highlight").expect("the layer parses");
        assert!(layer.paint.fill_opacity.is_none());
        assert!(layer.paint.fill_color.is_some());
        let diagnostic = document
            .unsupported_properties_on_layer("highlight")
            .into_iter()
            .next()
            .expect("the property is reported");
        assert_eq!(diagnostic.property_name, "fill-opacity");
        assert!(diagnostic.detail.contains("feature-state"));
    }

    #[test]
    fn an_unimplemented_filter_operator_admits_every_feature_and_is_reported() {
        let document = MapStyleDocument::parse_json(
            r##"{
                "version": 8,
                "layers": [{
                    "id": "roads",
                    "type": "line",
                    "filter": ["within", ["literal", {"type": "Polygon", "coordinates": []}]],
                    "paint": { "line-color": "#000000" }
                }]
            }"##,
        )
        .expect("the document still parses");
        let layer = document.layer("roads").expect("the layer parses");
        assert!(
            layer.filter.is_none(),
            "a filter the renderer cannot evaluate is dropped, not kept as a reject-all"
        );
        let diagnostics = property_names(&document.unsupported_properties_on_layer("roads"));
        assert_eq!(diagnostics, ["filter"]);
    }

    #[test]
    fn an_unrendered_source_type_is_reported_for_its_source_id() {
        let document = MapStyleDocument::parse_json(
            r##"{
                "version": 8,
                "sources": {
                    "dem": { "type": "raster-dem", "url": "https://example.test/dem.json" },
                    "photo": { "type": "image", "url": "https://example.test/a.png" },
                    "clip": { "type": "video", "urls": [] }
                },
                "layers": []
            }"##,
        )
        .expect("the document parses with diagnostics");
        for (source_id, source_type) in
            [("dem", "raster-dem"), ("photo", "image"), ("clip", "video")]
        {
            let diagnostic = document
                .unsupported
                .iter()
                .find(|diagnostic| {
                    matches!(
                        &diagnostic.scope,
                        UnsupportedScope::Source { source_id: scope_source_id }
                            if scope_source_id == source_id
                    ) && diagnostic.property_name == "type"
                })
                .unwrap_or_else(|| panic!("the {source_type} source is reported"));
            assert!(
                diagnostic.detail.contains(source_type),
                "the diagnostic names the source type: {}",
                diagnostic.detail
            );
        }
    }

    #[test]
    fn a_shield_block_parses_and_a_layer_without_one_keeps_its_fields() {
        let document = MapStyleDocument::parse_json(
            r##"{
                "version": 8,
                "sources": { "base": { "type": "vector" } },
                "layers": [
                    {
                        "id": "shields",
                        "type": "symbol",
                        "source": "base",
                        "source-layer": "routes",
                        "layout": { "text-field": ["get", "ref"] },
                        "paint": { "text-color": "#111111", "text-size": 10 },
                        "shield": {
                            "shape": "shield",
                            "fill_rgba": "#ffffff",
                            "border_rgba": "#123456",
                            "border_width_pixels": 1.5,
                            "text_rgba": "#ff0000",
                            "text_size_pixels": 11,
                            "padding_pixels": 3,
                            "minimum_size_pixels": 18
                        }
                    },
                    {
                        "id": "names",
                        "type": "symbol",
                        "source": "base",
                        "source-layer": "transportation_name",
                        "layout": { "text-field": ["get", "name"] }
                    }
                ]
            }"##,
        )
        .expect("the document parses");
        let shield = document
            .layer("shields")
            .expect("the shield layer parses")
            .shield
            .expect("the shield block parses");
        assert_eq!(shield.shape, ShieldShape::Shield);
        assert_eq!(shield.fill_color, Color::WHITE);
        assert_eq!(
            shield.border_color,
            Some(Color::new(
                0x12 as f64 / 255.0,
                0x34 as f64 / 255.0,
                0x56 as f64 / 255.0,
                1.0
            ))
        );
        assert!((shield.border_width_pixels - 1.5).abs() < 1e-12);
        assert_eq!(shield.text_color, Color::new(1.0, 0.0, 0.0, 1.0));
        assert!((shield.text_size_pixels - 11.0).abs() < 1e-12);
        assert!((shield.padding_pixels - 3.0).abs() < 1e-12);
        assert!((shield.minimum_size_pixels - 18.0).abs() < 1e-12);
        assert!(
            document
                .layer("names")
                .expect("the name layer parses")
                .shield
                .is_none()
        );
    }

    #[test]
    fn an_omitted_shield_member_takes_its_documented_default() {
        let document = MapStyleDocument::parse_json(
            r##"{
                "version": 8,
                "layers": [{
                    "id": "shields",
                    "type": "symbol",
                    "source-layer": "routes",
                    "shield": {}
                }]
            }"##,
        )
        .expect("the document parses");
        let shield = document
            .layer("shields")
            .expect("the layer parses")
            .shield
            .expect("the shield block parses");
        assert_eq!(shield.shape, DEFAULT_SHIELD_SHAPE);
        assert_eq!(shield.fill_color, DEFAULT_SHIELD_FILL_COLOR);
        assert_eq!(shield.border_color, None);
        assert!((shield.border_width_pixels - DEFAULT_SHIELD_BORDER_WIDTH_PIXELS).abs() < 1e-12);
        assert_eq!(shield.text_color, DEFAULT_SHIELD_TEXT_COLOR);
        assert!((shield.text_size_pixels - DEFAULT_SHIELD_TEXT_SIZE_PIXELS).abs() < 1e-12);
        assert!((shield.padding_pixels - DEFAULT_SHIELD_PADDING_PIXELS).abs() < 1e-12);
        assert!((shield.minimum_size_pixels - DEFAULT_SHIELD_MINIMUM_SIZE_PIXELS).abs() < 1e-12);
    }

    #[test]
    fn an_unknown_shield_shape_and_a_wrong_typed_member_are_reported() {
        let document = MapStyleDocument::parse_json(
            r##"{
                "version": 8,
                "layers": [{
                    "id": "shields",
                    "type": "symbol",
                    "source-layer": "routes",
                    "shield": { "shape": "hexagon", "padding_pixels": "wide" }
                }]
            }"##,
        )
        .expect("the document parses");
        let shield = document
            .layer("shields")
            .expect("the layer parses")
            .shield
            .expect("the shield block parses");
        assert_eq!(
            shield.shape, DEFAULT_SHIELD_SHAPE,
            "an unknown shape leaves the documented default in force"
        );
        assert!(
            (shield.padding_pixels - DEFAULT_SHIELD_PADDING_PIXELS).abs() < 1e-12,
            "a wrong-typed member leaves the documented default in force"
        );
        let reported: Vec<&str> = document
            .unsupported
            .iter()
            .map(|property| property.property_name.as_str())
            .collect();
        assert!(reported.contains(&"shield.shape"));
        assert!(reported.contains(&"shield.padding_pixels"));
    }

    #[test]
    fn shield_measurements_clamp_to_their_documented_bounds() {
        let document = MapStyleDocument::parse_json(
            r##"{
                "version": 8,
                "layers": [{
                    "id": "shields",
                    "type": "symbol",
                    "source-layer": "routes",
                    "shield": {
                        "border_width_pixels": -4,
                        "text_size_pixels": 1e300,
                        "padding_pixels": -2,
                        "minimum_size_pixels": 1e300
                    }
                }]
            }"##,
        )
        .expect("the document parses");
        let shield = document
            .layer("shields")
            .expect("the layer parses")
            .shield
            .expect("the shield block parses");
        assert_eq!(
            shield.border_width_pixels,
            MINIMUM_SHIELD_MEASUREMENT_PIXELS
        );
        assert_eq!(shield.padding_pixels, MINIMUM_SHIELD_MEASUREMENT_PIXELS);
        assert_eq!(shield.text_size_pixels, MAXIMUM_SHIELD_TEXT_SIZE_PIXELS);
        assert_eq!(shield.minimum_size_pixels, MAXIMUM_SHIELD_DIMENSION_PIXELS);
        assert!(
            shield.text_size_pixels.is_finite()
                && shield.padding_pixels.is_finite()
                && shield.minimum_size_pixels.is_finite()
                && shield.border_width_pixels.is_finite(),
            "a parsed shield never carries geometry that cannot be tessellated"
        );
        let reported: Vec<&str> = document
            .unsupported
            .iter()
            .map(|property| property.property_name.as_str())
            .collect();
        for member_name in [
            "shield.border_width_pixels",
            "shield.text_size_pixels",
            "shield.padding_pixels",
            "shield.minimum_size_pixels",
        ] {
            assert!(
                reported.contains(&member_name),
                "a clamped member is reported: {member_name}"
            );
        }
    }

    #[test]
    fn a_shield_block_on_a_non_symbol_layer_is_reported_and_ignored() {
        let document = MapStyleDocument::parse_json(
            r##"{
                "version": 8,
                "layers": [{
                    "id": "water",
                    "type": "fill",
                    "source-layer": "water",
                    "shield": { "shape": "shield" }
                }]
            }"##,
        )
        .expect("the document parses");
        assert!(
            document
                .layer("water")
                .expect("the layer parses")
                .shield
                .is_none(),
            "a fill layer paints no badge"
        );
        assert!(
            document
                .unsupported
                .iter()
                .any(|property| property.property_name == "shield"
                    && property.detail.contains("symbol layer")),
            "the ignored block is reported with its reason"
        );
    }
}
