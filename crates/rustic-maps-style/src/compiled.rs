//! Compilation of a parsed style document into render plans.
//!
//! [`compile_style_document`] walks a [`MapStyleDocument`] once and produces a
//! [`CompiledStyle`]: the document order is fixed, every layer's zoom span is
//! resolved to a pair of numbers, each layer's paint and layout properties are
//! carried as [`Property`] values (a constant or an expression), and the filter
//! stays an [`Expression`] tree. Nothing is re-parsed per feature or per frame:
//! a caller evaluates the plan with [`CompiledLayer::resolved_paint`],
//! [`CompiledLayer::matches_filter`] and
//! [`CompiledLayer::resolved_symbol_layout`] at the frame's zoom and the
//! feature's properties.
//!
//! # Ordering
//!
//! [`CompiledLayer::order_index`] is the layer's position among the compiled
//! render layers, zero based and stable for a given document. A renderer uses
//! it as the global paint order key: every tile's layer 0 paints before every
//! tile's layer 1, so one tile's fill can never overpaint another tile's
//! earlier line.
//!
//! # Background
//!
//! A `background` layer is the canvas clear colour, not a spatial layer, so the
//! compiler folds it into [`CompiledStyle::background`] and does not give it an
//! order index. The last background layer in document order wins, matching the
//! painter's model where a later background paints over an earlier one.
//!
//! # Degradation
//!
//! A property that fails at evaluation time resolves to its documented default
//! from [`crate::defaults`]; a layer type this crate does not render compiles
//! to [`CompiledLayerKind::Other`] with [`CompiledPaint::None`] rather than
//! failing the whole document. [`CompiledStyle::unsupported`] carries the
//! document's diagnostics so a caller can report exactly what was skipped.

use crate::color::Color;
use crate::defaults::{
    DEFAULT_BACKGROUND_COLOR, DEFAULT_BACKGROUND_OPACITY, DEFAULT_CIRCLE_BLUR,
    DEFAULT_CIRCLE_COLOR, DEFAULT_CIRCLE_OPACITY, DEFAULT_CIRCLE_PITCH_ALIGNMENT,
    DEFAULT_CIRCLE_PITCH_SCALE, DEFAULT_CIRCLE_RADIUS, DEFAULT_CIRCLE_SORT_KEY,
    DEFAULT_CIRCLE_STROKE_OPACITY, DEFAULT_CIRCLE_STROKE_WIDTH, DEFAULT_CIRCLE_TRANSLATE,
    DEFAULT_FILL_ANTIALIAS, DEFAULT_FILL_COLOR, DEFAULT_FILL_OPACITY, DEFAULT_FILL_SORT_KEY,
    DEFAULT_FILL_TRANSLATE, DEFAULT_FILL_TRANSLATE_ANCHOR, DEFAULT_ICON_ALLOW_OVERLAP,
    DEFAULT_ICON_ANCHOR, DEFAULT_ICON_IGNORE_PLACEMENT, DEFAULT_ICON_OFFSET, DEFAULT_ICON_OPACITY,
    DEFAULT_ICON_PADDING, DEFAULT_ICON_ROTATE, DEFAULT_ICON_ROTATION_ALIGNMENT, DEFAULT_ICON_SIZE,
    DEFAULT_ICON_TEXT_FIT, DEFAULT_ICON_TEXT_FIT_PADDING, DEFAULT_LINE_BLUR, DEFAULT_LINE_CAP,
    DEFAULT_LINE_COLOR, DEFAULT_LINE_GAP_WIDTH, DEFAULT_LINE_JOIN, DEFAULT_LINE_OFFSET,
    DEFAULT_LINE_OPACITY, DEFAULT_LINE_SORT_KEY, DEFAULT_LINE_TRANSLATE,
    DEFAULT_LINE_TRANSLATE_ANCHOR, DEFAULT_LINE_WIDTH, DEFAULT_SYMBOL_AVOID_EDGES,
    DEFAULT_SYMBOL_PLACEMENT, DEFAULT_SYMBOL_SORT_KEY, DEFAULT_SYMBOL_SPACING,
    DEFAULT_TEXT_ALLOW_OVERLAP, DEFAULT_TEXT_ANCHOR, DEFAULT_TEXT_COLOR, DEFAULT_TEXT_HALO_BLUR,
    DEFAULT_TEXT_HALO_COLOR, DEFAULT_TEXT_HALO_WIDTH, DEFAULT_TEXT_IGNORE_PLACEMENT,
    DEFAULT_TEXT_JUSTIFY, DEFAULT_TEXT_KEEP_UPRIGHT, DEFAULT_TEXT_LETTER_SPACING,
    DEFAULT_TEXT_LINE_HEIGHT, DEFAULT_TEXT_MAX_ANGLE, DEFAULT_TEXT_MAX_WIDTH, DEFAULT_TEXT_OFFSET,
    DEFAULT_TEXT_OPACITY, DEFAULT_TEXT_PADDING, DEFAULT_TEXT_PITCH_ALIGNMENT,
    DEFAULT_TEXT_RADIAL_OFFSET, DEFAULT_TEXT_ROTATION_ALIGNMENT, DEFAULT_TEXT_SIZE,
    DEFAULT_TEXT_TRANSFORM, DEFAULT_VISIBILITY,
};
use crate::document::{
    GeoJsonSourceSpec, LayerType, MapStyleDocument, RasterSourceSpec, SourceSpec, StyleLayer,
    VectorLayerSpec, VectorSourceSpec,
};
use crate::error::{UnsupportedProperty, UnsupportedScope};
use crate::expression::EvaluationContext;
use crate::types::{
    CirclePitchAlignment, CirclePitchScale, IconTextFit, LineCap, LineJoin, Property,
    PropertyValue, ShieldShape, ShieldSpec, SymbolPlacement, TextAnchor, TextJustify,
    TextPitchAlignment, TextRotationAlignment, TextTransform, TransitionSpec, TranslateAnchor,
    Visibility,
};
use std::collections::BTreeMap;

/// The font stack a symbol layer uses when it omits `text-font`. It is the
/// single font the renderer embeds, so text lays out without a host asset.
pub const DEFAULT_COMPILED_TEXT_FONT_STACK: &[&str] = &["Noto Sans Regular"];

/// The separator the glyph protocol puts between the family names of one font
/// stack in a request segment.
pub const FONT_STACK_NAME_SEPARATOR: &str = ",";

/// The address convention a tile source's rows follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TileScheme {
    /// XYZ rows increase southward, the web-mercator convention.
    #[default]
    Xyz,
    /// TMS rows increase northward; a fetch flips the row.
    Tms,
}

impl TileScheme {
    /// The specification's spelling of the scheme.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Xyz => "xyz",
            Self::Tms => "tms",
        }
    }

    /// Reads a TileJSON `scheme` member. An absent value is `xyz`; an
    /// unrecognized value is reported and treated as `xyz`.
    pub fn from_name(
        name: Option<&str>,
        scope: &UnsupportedScope,
        unsupported: &mut Vec<UnsupportedProperty>,
    ) -> Self {
        match name {
            None | Some("xyz") => Self::Xyz,
            Some("tms") => Self::Tms,
            Some(other) => {
                unsupported.push(UnsupportedProperty::new(
                    scope.clone(),
                    "scheme",
                    format!("unknown tile scheme {other:?}; xyz is assumed"),
                ));
                Self::Xyz
            }
        }
    }
}

/// The kind of source compiled from a style declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompiledSourceKind {
    /// A vector-tiles source.
    Vector,
    /// A raster-tile source.
    Raster,
    /// A GeoJSON source.
    GeoJson,
    /// A declared source type the renderer does not draw.
    Other(String),
}

impl CompiledSourceKind {
    /// The source type's specification spelling.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Vector => "vector",
            Self::Raster => "raster",
            Self::GeoJson => "geojson",
            Self::Other(name) => name,
        }
    }
}

/// The compiled form of one source declaration.
#[derive(Debug, Clone, PartialEq)]
pub enum CompiledSource {
    /// A vector-tiles source.
    Vector(CompiledVectorSource),
    /// A raster-tile source.
    Raster(CompiledRasterSource),
    /// A GeoJSON source.
    GeoJson(CompiledGeoJsonSource),
    /// A declared source type the renderer does not draw.
    Other(CompiledOpaqueSource),
}

impl CompiledSource {
    /// The source kind.
    pub fn kind(&self) -> CompiledSourceKind {
        match self {
            Self::Vector(_) => CompiledSourceKind::Vector,
            Self::Raster(_) => CompiledSourceKind::Raster,
            Self::GeoJson(_) => CompiledSourceKind::GeoJson,
            Self::Other(opaque) => CompiledSourceKind::Other(opaque.source_type.clone()),
        }
    }

    /// The declaration's zoom span, when it carries one.
    pub fn zoom_span(&self) -> (Option<f64>, Option<f64>) {
        match self {
            Self::Vector(source) => (source.minimum_zoom, source.maximum_zoom),
            Self::Raster(source) => (source.minimum_zoom, source.maximum_zoom),
            Self::GeoJson(_) | Self::Other(_) => (None, None),
        }
    }

    /// The declaration's attribution text, when it carries one.
    pub fn attribution(&self) -> Option<&str> {
        match self {
            Self::Vector(source) => source.attribution.as_deref(),
            Self::Raster(source) => source.attribution.as_deref(),
            Self::GeoJson(source) => source.attribution.as_deref(),
            Self::Other(_) => None,
        }
    }
}

/// A compiled vector-tiles source. The `url` and `tiles` members are the
/// document's own TileJSON hints; a host resolves the actual fetch through a
/// provider binding, so a product style can leave them empty.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledVectorSource {
    /// The TileJSON URL, when the source declares one.
    pub url: Option<String>,
    /// The source's explicit tile URL templates.
    pub tiles: Vec<String>,
    /// The source's bounds as `[west, south, east, north]`.
    pub bounds: Option<[f64; 4]>,
    /// The address convention the source's rows follow.
    pub scheme: TileScheme,
    /// The lowest zoom the source declares.
    pub minimum_zoom: Option<f64>,
    /// The highest zoom the source declares.
    pub maximum_zoom: Option<f64>,
    /// The attribution text the source declares.
    pub attribution: Option<String>,
    /// The feature-id property promotion rule.
    pub promote_id: serde_json::Value,
    /// The source layers the tileset declares.
    pub vector_layers: Vec<VectorLayerSpec>,
}

impl CompiledVectorSource {
    /// Whether the tileset declares a source layer named `source_layer`.
    ///
    /// A source without `vector_layers` cannot answer, so it admits every
    /// name rather than rejecting a layer the document may still serve.
    pub fn declares_source_layer(&self, source_layer: &str) -> bool {
        self.vector_layers.is_empty()
            || self
                .vector_layers
                .iter()
                .any(|vector_layer| vector_layer.id == source_layer)
    }
}

/// A compiled raster-tile source.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledRasterSource {
    /// The source's explicit tile URL templates.
    pub tiles: Vec<String>,
    /// The source's bounds as `[west, south, east, north]`.
    pub bounds: Option<[f64; 4]>,
    /// The address convention the source's rows follow.
    pub scheme: TileScheme,
    /// The lowest zoom the source declares.
    pub minimum_zoom: Option<f64>,
    /// The highest zoom the source declares.
    pub maximum_zoom: Option<f64>,
    /// The attribution text the source declares.
    pub attribution: Option<String>,
    /// The source's tile edge length, in pixels.
    pub tile_size: Option<f64>,
}

/// A compiled GeoJSON source.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledGeoJsonSource {
    /// The inline GeoJSON body, when the source carries one.
    pub data: Option<serde_json::Value>,
    /// The URL the body is fetched from, when the source names one.
    pub data_url: Option<String>,
    /// The attribution text the source declares.
    pub attribution: Option<String>,
    /// Whether feature ids are generated.
    pub generate_id: Option<bool>,
    /// The feature-id property promotion rule.
    pub promote_id: serde_json::Value,
    /// The tile buffer in pixels.
    pub buffer: Option<f64>,
    /// The simplification tolerance.
    pub tolerance: Option<f64>,
}

/// A compiled source of a type the renderer does not draw.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledOpaqueSource {
    /// The declared source type.
    pub source_type: String,
}

/// The compiled form of a style's `sprite` declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledSprite {
    /// The base URL the host resolves the index and sheet from.
    pub url: String,
}

/// The compiled form of one style document.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledStyle {
    /// The canvas clear colour, folded from the last `background` layer.
    pub background: Option<CompiledBackground>,
    /// The sprite sheet the document declares, resolved by the host through an
    /// injected source. A style without a sprite renders its text labels and
    /// draws no icons.
    pub sprite: Option<CompiledSprite>,
    /// The glyph URL template the document declares, carrying the font-stack
    /// and range placeholders. A style without one keeps the host's own glyph
    /// source, such as the renderer's embedded Latin range.
    pub glyphs: Option<String>,
    /// The source declarations, keyed by source id and resolved from the
    /// document's declarations.
    pub sources: BTreeMap<String, CompiledSource>,
    /// The render layers in paint order.
    pub layers: Vec<CompiledLayer>,
    /// The root transition timing, when the document declares one. A paint
    /// property's own `*-transition` overrides it; a property with neither
    /// draws its new value immediately.
    pub transition: Option<TransitionSpec>,
    /// Everything the document asked for that this crate cannot apply,
    /// including a layer that names an unknown source.
    pub unsupported: Vec<UnsupportedProperty>,
}

impl CompiledStyle {
    /// The background colour and opacity resolved at `zoom`, or `None` when the
    /// style declares no background layer.
    pub fn background_at(&self, zoom: f64) -> Option<[f32; 4]> {
        let background = self.background.as_ref()?;
        let context = EvaluationContext::at_zoom(zoom);
        Some(background.resolved_rgba(&context))
    }

    /// The layer a document id names.
    pub fn layer(&self, layer_id: &str) -> Option<&CompiledLayer> {
        self.layers.iter().find(|layer| layer.id == layer_id)
    }

    /// The compiled source a source id names.
    pub fn source(&self, source_id: &str) -> Option<&CompiledSource> {
        self.sources.get(source_id)
    }

    /// The number of render layers.
    pub fn layer_count(&self) -> usize {
        self.layers.len()
    }

    /// The font stacks the symbol layers name, each as the comma-separated
    /// family list a glyph request carries, in layer order and without
    /// duplicates.
    ///
    /// A host that resolves the style's glyph endpoint fetches one range per
    /// stack. A layout whose `text-font` resolves per feature or per zoom
    /// contributes the family list it starts from; the frame's own resolved
    /// stack is the authority for what a label draws with.
    pub fn symbol_font_stacks(&self) -> Vec<String> {
        let mut font_stacks: Vec<String> = Vec::new();
        for layer in &self.layers {
            if layer.kind != CompiledLayerKind::Symbol {
                continue;
            }
            let Property::Constant(font_families) = &layer.layout.text_font else {
                continue;
            };
            let font_stack = font_families.join(FONT_STACK_NAME_SEPARATOR);
            if !font_stacks.contains(&font_stack) {
                font_stacks.push(font_stack);
            }
        }
        font_stacks
    }

    /// How long, in seconds, a change to this style still has work to do:
    /// the longest declared root or per-property transition. Zero when the
    /// document declares no transition at all, which draws new values
    /// immediately.
    pub fn transition_lifetime_seconds(&self) -> f64 {
        let mut lifetime = self
            .transition
            .map_or(0.0, |transition| transition.lifetime_seconds());
        for layer in &self.layers {
            for transition in layer.paint.transition_specs() {
                lifetime = lifetime.max(transition.lifetime_seconds());
            }
        }
        lifetime
    }
}

/// The compiled background paint.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledBackground {
    /// The `background-color` property.
    pub color: Property<Color>,
    /// The `background-opacity` property.
    pub opacity: Property<f64>,
}

impl CompiledBackground {
    /// The background as the renderer's RGBA components: the colour channels
    /// with the opacity folded into alpha.
    pub fn resolved_rgba(&self, context: &EvaluationContext<'_>) -> [f32; 4] {
        let color = resolve(&self.color, DEFAULT_BACKGROUND_COLOR, context);
        let opacity = resolve(&self.opacity, DEFAULT_BACKGROUND_OPACITY, context);
        [
            color.red as f32,
            color.green as f32,
            color.blue as f32,
            (color.alpha * opacity) as f32,
        ]
    }
}

/// The geometry kind a compiled layer paints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompiledLayerKind {
    /// A canvas clear colour, folded into [`CompiledStyle::background`].
    Background,
    /// Polygon features as filled rings with their holes subtracted.
    Fill,
    /// LineString features as screen-space-thickened polylines.
    Line,
    /// Point features as screen-facing markers.
    Circle,
    /// Declarative labels consumed by the frame-level label pass.
    Symbol,
    /// Raster tiles, routed to the raster scheduler rather than the vector
    /// overlay pipeline.
    Raster,
    /// A layer type this renderer does not draw yet.
    Other,
}

impl CompiledLayerKind {
    /// The specification's layer-type spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Background => "background",
            Self::Fill => "fill",
            Self::Line => "line",
            Self::Circle => "circle",
            Self::Symbol => "symbol",
            Self::Raster => "raster",
            Self::Other => "other",
        }
    }
}

/// One compiled render layer.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledLayer {
    /// The document-unique layer id.
    pub id: String,
    /// The geometry kind the layer paints.
    pub kind: CompiledLayerKind,
    /// The style source id the layer reads.
    pub source: Option<String>,
    /// The resolved kind of [`Self::source`], when the document declares it.
    pub source_kind: Option<CompiledSourceKind>,
    /// The source layer name the layer reads.
    pub source_layer: Option<String>,
    /// The lowest zoom (inclusive) the layer draws at.
    pub minimum_zoom: f64,
    /// The zoom (exclusive) the layer stops drawing at.
    pub maximum_zoom: f64,
    /// The global paint order key: every layer with a lower index paints
    /// before every layer with a higher one, across all tiles.
    pub order_index: usize,
    /// The layer's filter, evaluated per feature with the frame zoom in the
    /// context (zoom-gated filters such as per-class minimum zooms are data
    /// expressions whose `step` input is `["zoom"]`).
    pub filter: Option<crate::expression::Expression>,
    /// The kind-specific paint plan.
    pub paint: CompiledPaint,
    /// The layer's layout plan.
    pub layout: CompiledLayout,
    /// The route-reference badge a symbol layer draws, when it declares one.
    pub shield: Option<CompiledShield>,
    /// Whether the document's layer declares any line presentation property
    /// (`line-cap`, `line-join`, `line-dasharray`, `line-offset`,
    /// `line-gap-width` or `line-blur`).
    ///
    /// A renderer uses this to choose between the default stroke and the
    /// styled one: a layer that declares nothing keeps the renderer's default
    /// geometry, an explicit declaration is honoured exactly.
    pub declares_line_presentation: bool,
    /// Whether the document's layer declares a circle pitch property
    /// (`circle-pitch-scale` or `circle-pitch-alignment`).
    ///
    /// A renderer that has no ground-plane geometry for an omitted property
    /// can use this to keep its own documented default for a layer that asks
    /// for nothing, while honouring an explicit request.
    pub declares_circle_pitch: bool,
    /// Whether the document's layer declares `text-max-angle`.
    ///
    /// A renderer whose default run tolerance differs from the
    /// specification's can keep its own default for a layer that asks for
    /// nothing while honouring an explicit request.
    pub declares_text_max_angle: bool,
    /// Whether the document's layer declares `symbol-spacing`.
    ///
    /// A renderer whose default repeat distance differs from the
    /// specification's (a single-tile frame names a road more often) can use
    /// this to keep its own default for a layer that asks for nothing.
    pub declares_symbol_spacing: bool,
}

impl CompiledLayer {
    /// Whether the layer's zoom span and visibility admit `zoom`.
    pub fn is_visible_at_zoom(&self, zoom: f64) -> bool {
        if zoom < self.minimum_zoom || zoom >= self.maximum_zoom {
            return false;
        }
        let context = EvaluationContext::at_zoom(zoom);
        match self.layout.visibility.evaluate(&context) {
            Ok(visibility) => visibility == Visibility::Visible,
            Err(_) => true,
        }
    }

    /// Whether a feature passes the layer's filter.
    ///
    /// A missing filter admits every feature. A filter that evaluates to a
    /// non-boolean value or fails at runtime rejects the feature rather than
    /// painting something the style did not ask for.
    pub fn matches_filter(&self, context: &EvaluationContext<'_>) -> bool {
        match &self.filter {
            None => true,
            Some(filter) => filter
                .evaluate(context)
                .map(|value| value.truthy())
                .unwrap_or(false),
        }
    }

    /// The layer's paint resolved at the context's zoom and feature.
    pub fn resolved_paint(&self, context: &EvaluationContext<'_>) -> ResolvedPaint {
        match &self.paint {
            CompiledPaint::None => ResolvedPaint::None,
            CompiledPaint::Fill(paint) => ResolvedPaint::Fill(ResolvedFillPaint {
                color: resolve(&paint.color, DEFAULT_FILL_COLOR, context),
                opacity: resolve(&paint.opacity, DEFAULT_FILL_OPACITY, context),
                outline_color: paint
                    .outline_color
                    .as_ref()
                    .map(|outline_color| resolve(outline_color, DEFAULT_FILL_COLOR, context)),
                antialias: resolve(&paint.antialias, DEFAULT_FILL_ANTIALIAS, context),
                translate: resolve_translate(&paint.translate, DEFAULT_FILL_TRANSLATE, context),
                translate_anchor: paint.translate_anchor,
            }),
            CompiledPaint::Line(paint) => ResolvedPaint::Line(ResolvedLinePaint {
                color: resolve(&paint.color, DEFAULT_LINE_COLOR, context),
                width: resolve(&paint.width, DEFAULT_LINE_WIDTH, context),
                opacity: resolve(&paint.opacity, DEFAULT_LINE_OPACITY, context),
                blur: resolve(&paint.blur, DEFAULT_LINE_BLUR, context),
                offset: resolve(&paint.offset, DEFAULT_LINE_OFFSET, context),
                gap_width: resolve(&paint.gap_width, DEFAULT_LINE_GAP_WIDTH, context),
                dasharray: resolve(&paint.dasharray, Vec::new(), context),
                translate: resolve_translate(&paint.translate, DEFAULT_LINE_TRANSLATE, context),
                translate_anchor: paint.translate_anchor,
            }),
            CompiledPaint::Circle(paint) => ResolvedPaint::Circle(ResolvedCirclePaint {
                color: resolve(&paint.color, DEFAULT_CIRCLE_COLOR, context),
                radius: resolve(&paint.radius, DEFAULT_CIRCLE_RADIUS, context),
                opacity: resolve(&paint.opacity, DEFAULT_CIRCLE_OPACITY, context),
                stroke_color: paint
                    .stroke_color
                    .as_ref()
                    .map(|stroke_color| resolve(stroke_color, DEFAULT_CIRCLE_COLOR, context)),
                stroke_width: resolve(&paint.stroke_width, DEFAULT_CIRCLE_STROKE_WIDTH, context),
                stroke_opacity: resolve(
                    &paint.stroke_opacity,
                    DEFAULT_CIRCLE_STROKE_OPACITY,
                    context,
                ),
                blur: resolve(&paint.blur, DEFAULT_CIRCLE_BLUR, context),
                translate: resolve_translate(&paint.translate, DEFAULT_CIRCLE_TRANSLATE, context),
            }),
            CompiledPaint::Symbol(paint) => {
                let mut resolved = ResolvedSymbolPaint {
                    text_color: resolve(&paint.text_color, DEFAULT_TEXT_COLOR, context),
                    text_opacity: resolve(&paint.text_opacity, DEFAULT_TEXT_OPACITY, context),
                    halo_color: resolve(&paint.halo_color, DEFAULT_TEXT_HALO_COLOR, context),
                    halo_width: resolve(&paint.halo_width, DEFAULT_TEXT_HALO_WIDTH, context),
                    halo_blur: resolve(&paint.halo_blur, DEFAULT_TEXT_HALO_BLUR, context),
                    icon_opacity: resolve(&paint.icon_opacity, DEFAULT_ICON_OPACITY, context),
                };
                // A badge carries the reference's own colour; the layer's text
                // paint governs only its other labels.
                if let Some(shield) = &self.shield {
                    resolved.text_color = shield.text_color;
                    resolved.text_opacity = 1.0;
                }
                ResolvedPaint::Symbol(resolved)
            }
        }
    }

    /// The layer's paint resolved with a style transition applied.
    ///
    /// `previous` carries the same layer under the style being replaced and
    /// the monotonic seconds elapsed since the change. The properties this
    /// renderer paints are blended: fill, line and circle colour and opacity.
    /// Everything else, a layer the previous style does not carry, and a
    /// property with no declared transition take the new value immediately.
    ///
    /// # Parameters
    /// - `context` — the feature and frame zoom both styles resolve against.
    /// - `root_transition` — the document's root timing, the default for a
    ///   property that declares none.
    /// - `previous` — the previous style's layer and the elapsed seconds since
    ///   the change; `None` draws the new values with no blend.
    pub fn resolved_transitioned_paint(
        &self,
        context: &EvaluationContext<'_>,
        root_transition: Option<TransitionSpec>,
        previous: Option<(&CompiledLayer, f64)>,
    ) -> ResolvedPaint {
        let Some((previous_layer, elapsed_seconds)) = previous else {
            return self.resolved_paint(context);
        };
        match (
            self.resolved_paint(context),
            previous_layer.resolved_paint(context),
        ) {
            (ResolvedPaint::Fill(new), ResolvedPaint::Fill(old)) => {
                let (color_transition, opacity_transition) = match &self.paint {
                    CompiledPaint::Fill(paint) => {
                        (paint.color_transition, paint.opacity_transition)
                    }
                    _ => (None, None),
                };
                let color_progress =
                    transition_progress(color_transition, root_transition, elapsed_seconds);
                let opacity_progress =
                    transition_progress(opacity_transition, root_transition, elapsed_seconds);
                ResolvedPaint::Fill(ResolvedFillPaint {
                    color: Color::interpolate(old.color, new.color, color_progress),
                    opacity: blend(old.opacity, new.opacity, opacity_progress),
                    outline_color: match (old.outline_color, new.outline_color) {
                        (Some(old_outline), Some(new_outline)) => {
                            Some(Color::interpolate(old_outline, new_outline, color_progress))
                        }
                        (_, new_outline) => new_outline,
                    },
                    antialias: new.antialias,
                    translate: new.translate,
                    translate_anchor: new.translate_anchor,
                })
            }
            (ResolvedPaint::Line(new), ResolvedPaint::Line(old)) => {
                let (color_transition, opacity_transition) = match &self.paint {
                    CompiledPaint::Line(paint) => {
                        (paint.color_transition, paint.opacity_transition)
                    }
                    _ => (None, None),
                };
                let color_progress =
                    transition_progress(color_transition, root_transition, elapsed_seconds);
                let opacity_progress =
                    transition_progress(opacity_transition, root_transition, elapsed_seconds);
                ResolvedPaint::Line(ResolvedLinePaint {
                    color: Color::interpolate(old.color, new.color, color_progress),
                    opacity: blend(old.opacity, new.opacity, opacity_progress),
                    ..new
                })
            }
            (ResolvedPaint::Circle(new), ResolvedPaint::Circle(old)) => {
                let (color_transition, opacity_transition) = match &self.paint {
                    CompiledPaint::Circle(paint) => {
                        (paint.color_transition, paint.opacity_transition)
                    }
                    _ => (None, None),
                };
                let color_progress =
                    transition_progress(color_transition, root_transition, elapsed_seconds);
                let opacity_progress =
                    transition_progress(opacity_transition, root_transition, elapsed_seconds);
                ResolvedPaint::Circle(ResolvedCirclePaint {
                    color: Color::interpolate(old.color, new.color, color_progress),
                    opacity: blend(old.opacity, new.opacity, opacity_progress),
                    ..new
                })
            }
            (current, _) => current,
        }
    }

    /// The layer's symbol layout resolved against one feature.
    ///
    /// The text field is evaluated against the feature; the size and placement
    /// values are camera properties and may vary with `context.zoom`. A layer
    /// that omits the text field resolves to the empty string, which the label
    /// pass treats as "no label".
    pub fn resolved_symbol_layout(&self, context: &EvaluationContext<'_>) -> ResolvedSymbolLayout {
        let placement = resolve(
            &self.layout.symbol_placement,
            DEFAULT_SYMBOL_PLACEMENT,
            context,
        );
        let text_pitch_alignment = resolve(
            &self.layout.text_pitch_alignment,
            DEFAULT_TEXT_PITCH_ALIGNMENT,
            context,
        )
        .resolved_for_placement(placement);
        ResolvedSymbolLayout {
            text: resolved_symbol_text(self, context),
            font_stack: resolve(&self.layout.text_font, default_text_font_stack(), context),
            // A badge declares the reference's own size; the layer's text size
            // governs only its other labels.
            text_size: match &self.shield {
                Some(shield) => shield.text_size_pixels,
                None => resolve(&self.layout.text_size, DEFAULT_TEXT_SIZE, context),
            },
            text_max_width: resolve(&self.layout.text_max_width, DEFAULT_TEXT_MAX_WIDTH, context),
            text_line_height: resolve(
                &self.layout.text_line_height,
                DEFAULT_TEXT_LINE_HEIGHT,
                context,
            ),
            text_letter_spacing: resolve(
                &self.layout.text_letter_spacing,
                DEFAULT_TEXT_LETTER_SPACING,
                context,
            ),
            text_justify: resolve(&self.layout.text_justify, DEFAULT_TEXT_JUSTIFY, context),
            text_anchor: resolve(&self.layout.text_anchor, DEFAULT_TEXT_ANCHOR, context),
            text_rotation_alignment: resolve(
                &self.layout.text_rotation_alignment,
                DEFAULT_TEXT_ROTATION_ALIGNMENT,
                context,
            ),
            text_keep_upright: resolve(
                &self.layout.text_keep_upright,
                DEFAULT_TEXT_KEEP_UPRIGHT,
                context,
            ),
            variable_anchors: resolve(&self.layout.text_variable_anchor, Vec::new(), context),
            radial_offset: resolve(
                &self.layout.text_radial_offset,
                DEFAULT_TEXT_RADIAL_OFFSET,
                context,
            ),
            max_angle_degrees: resolve(
                &self.layout.text_max_angle,
                DEFAULT_TEXT_MAX_ANGLE,
                context,
            ),
            text_transform: resolve(&self.layout.text_transform, DEFAULT_TEXT_TRANSFORM, context),
            placement,
            symbol_spacing: resolve(&self.layout.symbol_spacing, DEFAULT_SYMBOL_SPACING, context),
            allow_overlap: resolve(
                &self.layout.text_allow_overlap,
                DEFAULT_TEXT_ALLOW_OVERLAP,
                context,
            ),
            ignore_placement: resolve(
                &self.layout.text_ignore_placement,
                DEFAULT_TEXT_IGNORE_PLACEMENT,
                context,
            ),
            avoid_edges: resolve(
                &self.layout.symbol_avoid_edges,
                DEFAULT_SYMBOL_AVOID_EDGES,
                context,
            ),
            text_offset: resolve_translate(&self.layout.text_offset, DEFAULT_TEXT_OFFSET, context),
            text_padding: resolve(&self.layout.text_padding, DEFAULT_TEXT_PADDING, context),
            text_pitch_alignment,
            icon_text_fit: resolve(&self.layout.icon_text_fit, DEFAULT_ICON_TEXT_FIT, context),
            icon_text_fit_padding: resolve_padding_quad(
                &self.layout.icon_text_fit_padding,
                DEFAULT_ICON_TEXT_FIT_PADDING,
                context,
            ),
            sort_key: resolve(
                &self.layout.symbol_sort_key,
                DEFAULT_SYMBOL_SORT_KEY,
                context,
            ),
            icon_image: self
                .layout
                .icon_image
                .as_ref()
                .and_then(|property| property.evaluate(context).ok())
                .filter(|image_name| !image_name.is_empty()),
            icon_size: resolve(&self.layout.icon_size, DEFAULT_ICON_SIZE, context),
            icon_anchor: resolve(&self.layout.icon_anchor, DEFAULT_ICON_ANCHOR, context),
            icon_offset: resolve_translate(&self.layout.icon_offset, DEFAULT_ICON_OFFSET, context),
            icon_rotate_degrees: resolve(&self.layout.icon_rotate, DEFAULT_ICON_ROTATE, context),
            icon_rotation_alignment: resolve(
                &self.layout.icon_rotation_alignment,
                DEFAULT_ICON_ROTATION_ALIGNMENT,
                context,
            ),
            icon_padding: resolve(&self.layout.icon_padding, DEFAULT_ICON_PADDING, context),
            icon_allow_overlap: resolve(
                &self.layout.icon_allow_overlap,
                DEFAULT_ICON_ALLOW_OVERLAP,
                context,
            ),
            icon_ignore_placement: resolve(
                &self.layout.icon_ignore_placement,
                DEFAULT_ICON_IGNORE_PLACEMENT,
                context,
            ),
        }
    }

    /// The layer's line cap resolved at `context`'s zoom.
    pub fn resolved_line_cap(&self, context: &EvaluationContext<'_>) -> LineCap {
        resolve(&self.layout.line_cap, DEFAULT_LINE_CAP, context)
    }

    /// The layer's line join resolved at `context`'s zoom.
    pub fn resolved_line_join(&self, context: &EvaluationContext<'_>) -> LineJoin {
        resolve(&self.layout.line_join, DEFAULT_LINE_JOIN, context)
    }

    /// The order a line feature paints in within its layer, resolved at the
    /// context's zoom and feature. Lower values paint first; features that
    /// resolve the same key keep their source order.
    pub fn resolved_line_sort_key(&self, context: &EvaluationContext<'_>) -> f64 {
        resolve(&self.layout.line_sort_key, DEFAULT_LINE_SORT_KEY, context)
    }

    /// The order a fill feature paints in within its layer, resolved at the
    /// context's zoom and feature. Lower values paint first; features that
    /// resolve the same key keep their source order.
    pub fn resolved_fill_sort_key(&self, context: &EvaluationContext<'_>) -> f64 {
        resolve(&self.layout.fill_sort_key, DEFAULT_FILL_SORT_KEY, context)
    }

    /// The order a circle feature paints in within its layer, resolved at the
    /// context's zoom and feature. Lower values paint first; features that
    /// resolve the same key keep their source order.
    pub fn resolved_circle_sort_key(&self, context: &EvaluationContext<'_>) -> f64 {
        resolve(
            &self.layout.circle_sort_key,
            DEFAULT_CIRCLE_SORT_KEY,
            context,
        )
    }

    /// The layer's circle pitch-scale behaviour resolved at the context's
    /// zoom and feature.
    pub fn resolved_circle_pitch_scale(&self, context: &EvaluationContext<'_>) -> CirclePitchScale {
        resolve(
            &self.layout.circle_pitch_scale,
            DEFAULT_CIRCLE_PITCH_SCALE,
            context,
        )
    }

    /// The layer's circle pitch alignment resolved at the context's zoom and
    /// feature.
    pub fn resolved_circle_pitch_alignment(
        &self,
        context: &EvaluationContext<'_>,
    ) -> CirclePitchAlignment {
        resolve(
            &self.layout.circle_pitch_alignment,
            DEFAULT_CIRCLE_PITCH_ALIGNMENT,
            context,
        )
    }
}

/// A layer's kind-specific paint plan.
#[derive(Debug, Clone, PartialEq)]
pub enum CompiledPaint {
    /// The layer paints nothing (a background layer or an unimplemented kind).
    None,
    /// Fill paint.
    Fill(CompiledFillPaint),
    /// Line paint.
    Line(CompiledLinePaint),
    /// Circle paint.
    Circle(CompiledCirclePaint),
    /// Symbol paint.
    Symbol(CompiledSymbolPaint),
}

impl CompiledPaint {
    /// The declared per-property transitions this paint carries. The root
    /// transition is not included; a caller combines the two per property.
    pub fn transition_specs(&self) -> Vec<TransitionSpec> {
        let mut transitions = Vec::new();
        match self {
            Self::Fill(paint) => {
                transitions.extend(paint.color_transition);
                transitions.extend(paint.opacity_transition);
            }
            Self::Line(paint) => {
                transitions.extend(paint.color_transition);
                transitions.extend(paint.opacity_transition);
            }
            Self::Circle(paint) => {
                transitions.extend(paint.color_transition);
                transitions.extend(paint.opacity_transition);
            }
            Self::None | Self::Symbol(_) => {}
        }
        transitions
    }
}

/// Fill paint properties.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledFillPaint {
    /// The fill colour.
    pub color: Property<Color>,
    /// The fill opacity.
    pub opacity: Property<f64>,
    /// The explicitly declared edge colour; `None` paints no outline.
    pub outline_color: Option<Property<Color>>,
    /// Whether the fill edge is antialiased.
    pub antialias: Property<bool>,
    /// The fill's displacement from its geometry, in screen pixels.
    pub translate: Property<Vec<f64>>,
    /// The frame of reference the displacement is measured against.
    pub translate_anchor: TranslateAnchor,
    /// The declared `fill-color-transition`.
    pub color_transition: Option<TransitionSpec>,
    /// The declared `fill-opacity-transition`.
    pub opacity_transition: Option<TransitionSpec>,
}

/// Line paint properties.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledLinePaint {
    /// The line colour.
    pub color: Property<Color>,
    /// The line width in screen pixels.
    pub width: Property<f64>,
    /// The line opacity.
    pub opacity: Property<f64>,
    /// The declared `line-color-transition`.
    pub color_transition: Option<TransitionSpec>,
    /// The declared `line-opacity-transition`.
    pub opacity_transition: Option<TransitionSpec>,
    /// The line blur in screen pixels.
    pub blur: Property<f64>,
    /// The perpendicular offset in screen pixels.
    pub offset: Property<f64>,
    /// The width of the gap the stroke is drawn around, in screen pixels.
    pub gap_width: Property<f64>,
    /// The dash pattern in line-width units.
    pub dasharray: Property<Vec<f64>>,
    /// The line's displacement from its geometry, `[x, y]` in screen pixels.
    pub translate: Property<Vec<f64>>,
    /// The frame of reference the displacement is measured against.
    pub translate_anchor: TranslateAnchor,
}

/// Circle paint properties.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledCirclePaint {
    /// The circle colour.
    pub color: Property<Color>,
    /// The circle radius in screen pixels.
    pub radius: Property<f64>,
    /// The circle opacity.
    pub opacity: Property<f64>,
    /// The declared `circle-color-transition`.
    pub color_transition: Option<TransitionSpec>,
    /// The declared `circle-opacity-transition`.
    pub opacity_transition: Option<TransitionSpec>,
    /// The explicitly declared stroke colour; `None` paints no stroke.
    pub stroke_color: Option<Property<Color>>,
    /// The stroke width in screen pixels, drawn outside the radius.
    pub stroke_width: Property<f64>,
    /// The stroke opacity.
    pub stroke_opacity: Property<f64>,
    /// How far the circle's edge is softened, in screen pixels.
    pub blur: Property<f64>,
    /// The circle's displacement from its anchor, in screen pixels.
    pub translate: Property<Vec<f64>>,
}

/// Symbol paint properties.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledSymbolPaint {
    /// The text colour.
    pub text_color: Property<Color>,
    /// The text opacity.
    pub text_opacity: Property<f64>,
    /// The halo colour drawn behind the text.
    pub halo_color: Property<Color>,
    /// The halo width in screen pixels; zero paints no halo.
    pub halo_width: Property<f64>,
    /// The halo blur in screen pixels.
    pub halo_blur: Property<f64>,
    /// The icon opacity; text opacity stays `text_opacity`.
    pub icon_opacity: Property<f64>,
}

/// A compiled route-reference badge.
///
/// Every member is a constant, so the badge needs no per-feature evaluation.
/// The badge's text colour and size replace the layer's own when the layer
/// declares a badge, so the reference draws in the badge's declared paint.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledShield {
    /// The badge outline.
    pub shape: ShieldShape,
    /// The badge fill colour.
    pub fill_color: Color,
    /// The outline colour; absent draws no outline.
    pub border_color: Option<Color>,
    /// The outline width in screen pixels; zero draws no outline.
    pub border_width_pixels: f64,
    /// The reference's own text colour inside the badge.
    pub text_color: Color,
    /// The reference's own text size in screen pixels.
    pub text_size_pixels: f64,
    /// The inset between the reference's glyphs and the badge edge, in screen
    /// pixels.
    pub padding_pixels: f64,
    /// The smallest badge width and height in screen pixels.
    pub minimum_size_pixels: f64,
}

impl From<&ShieldSpec> for CompiledShield {
    fn from(shield: &ShieldSpec) -> Self {
        Self {
            shape: shield.shape,
            fill_color: shield.fill_color,
            border_color: shield.border_color,
            border_width_pixels: shield.border_width_pixels,
            text_color: shield.text_color,
            text_size_pixels: shield.text_size_pixels,
            padding_pixels: shield.padding_pixels,
            minimum_size_pixels: shield.minimum_size_pixels,
        }
    }
}

/// Layout properties a compiled layer carries.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledLayout {
    /// Whether the layer is drawn.
    pub visibility: Property<Visibility>,
    /// How line ends are drawn.
    pub line_cap: Property<LineCap>,
    /// How line segments meet.
    pub line_join: Property<LineJoin>,
    /// The order line features paint in within the layer; lower values first.
    pub line_sort_key: Property<f64>,
    /// The order fill features paint in within the layer; lower values first.
    pub fill_sort_key: Property<f64>,
    /// The order circle features paint in within the layer; lower values
    /// first.
    pub circle_sort_key: Property<f64>,
    /// How a circle's radius follows the ground plane when the map is pitched.
    pub circle_pitch_scale: Property<CirclePitchScale>,
    /// Whether circle geometry lies on the ground or faces the screen.
    pub circle_pitch_alignment: Property<CirclePitchAlignment>,
    /// The symbol text field.
    pub text_field: Property<String>,
    /// The symbol font stack, most preferred first.
    pub text_font: Property<Vec<String>>,
    /// The text size in screen pixels.
    pub text_size: Property<f64>,
    /// The text wrapping width in ems.
    pub text_max_width: Property<f64>,
    /// The line height of wrapped text, in ems.
    pub text_line_height: Property<f64>,
    /// The extra space between glyphs, in ems.
    pub text_letter_spacing: Property<f64>,
    /// How wrapped lines are aligned within the text block.
    pub text_justify: Property<TextJustify>,
    /// Where the text block sits relative to the symbol anchor.
    pub text_anchor: Property<TextAnchor>,
    /// What a point-placed label's rotation follows.
    pub text_rotation_alignment: Property<TextRotationAlignment>,
    /// Whether a rotated label is flipped to read upright.
    pub text_keep_upright: Property<bool>,
    /// Candidate anchors a point label tries, in order.
    pub text_variable_anchor: Property<Vec<TextAnchor>>,
    /// The distance a variable-anchored label sits from its symbol, in ems.
    pub text_radial_offset: Property<f64>,
    /// The sharpest turn a line label's straight run tolerates, in degrees.
    pub text_max_angle: Property<f64>,
    /// The text case transformation.
    pub text_transform: Property<TextTransform>,
    /// Where a symbol layer places its text.
    pub symbol_placement: Property<SymbolPlacement>,
    /// The distance between repeated line labels, in screen pixels.
    pub symbol_spacing: Property<f64>,
    /// Whether text may overlap other labels.
    pub text_allow_overlap: Property<bool>,
    /// Whether text ignores collision placement entirely.
    pub text_ignore_placement: Property<bool>,
    /// Whether labels crossing the viewport edge are dropped.
    pub symbol_avoid_edges: Property<bool>,
    /// The text's displacement from its anchor, `[x, y]` in ems.
    pub text_offset: Property<Vec<f64>>,
    /// The transparent margin kept around the text's collision box, in screen
    /// pixels.
    pub text_padding: Property<f64>,
    /// What the text is aligned to when the map is pitched.
    pub text_pitch_alignment: Property<TextPitchAlignment>,
    /// The symbol placement order key; lower values place first.
    pub symbol_sort_key: Property<f64>,
    /// The sprite icon a symbol layer draws, when it names one at parse time.
    pub icon_image: Option<Property<String>>,
    /// The icon's display scale relative to its sprite-sheet size.
    pub icon_size: Property<f64>,
    /// Where the icon sits relative to the symbol anchor.
    pub icon_anchor: Property<TextAnchor>,
    /// The icon's displacement from its anchor, `[x, y]` in ems.
    pub icon_offset: Property<Vec<f64>>,
    /// The icon's clockwise rotation in degrees.
    pub icon_rotate: Property<f64>,
    /// What the icon's rotation follows.
    pub icon_rotation_alignment: Property<TextRotationAlignment>,
    /// The transparent margin kept around the icon's collision box, in pixels.
    pub icon_padding: Property<f64>,
    /// Whether the icon may overlap other symbols.
    pub icon_allow_overlap: Property<bool>,
    /// Whether the icon ignores collision placement entirely.
    pub icon_ignore_placement: Property<bool>,
    /// How the icon is stretched to fit the text block.
    pub icon_text_fit: Property<IconTextFit>,
    /// The inset kept between a fitted icon and the text block,
    /// `[top, right, bottom, left]` in screen pixels.
    pub icon_text_fit_padding: Property<Vec<f64>>,
}

/// A layer's paint resolved for one feature and zoom.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedPaint {
    /// The layer paints nothing.
    None,
    /// Resolved fill paint.
    Fill(ResolvedFillPaint),
    /// Resolved line paint.
    Line(ResolvedLinePaint),
    /// Resolved circle paint.
    Circle(ResolvedCirclePaint),
    /// Resolved symbol paint.
    Symbol(ResolvedSymbolPaint),
}

/// Fill paint resolved at one zoom and feature.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedFillPaint {
    /// The resolved colour.
    pub color: Color,
    /// The resolved opacity.
    pub opacity: f64,
    /// The resolved edge colour, when the layer declares one.
    pub outline_color: Option<Color>,
    /// Whether the edge is antialiased.
    pub antialias: bool,
    /// The resolved displacement in screen pixels.
    pub translate: [f64; 2],
    /// The frame of reference the displacement is measured against.
    pub translate_anchor: TranslateAnchor,
}

/// Line paint resolved at one zoom and feature.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedLinePaint {
    /// The resolved colour.
    pub color: Color,
    /// The resolved width in screen pixels.
    pub width: f64,
    /// The resolved opacity.
    pub opacity: f64,
    /// The resolved blur in screen pixels.
    pub blur: f64,
    /// The resolved perpendicular offset in screen pixels.
    pub offset: f64,
    /// The resolved gap width in screen pixels; zero paints a solid stroke.
    pub gap_width: f64,
    /// The resolved dash pattern in line-width units.
    pub dasharray: Vec<f64>,
    /// The resolved displacement from the geometry, `[x, y]` in screen pixels.
    pub translate: [f64; 2],
    /// The frame of reference the displacement is measured against.
    pub translate_anchor: TranslateAnchor,
}

/// Circle paint resolved at one zoom and feature.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedCirclePaint {
    /// The resolved colour.
    pub color: Color,
    /// The resolved radius in screen pixels.
    pub radius: f64,
    /// The resolved opacity.
    pub opacity: f64,
    /// The resolved stroke colour, when the layer declares one.
    pub stroke_color: Option<Color>,
    /// The resolved stroke width in screen pixels.
    pub stroke_width: f64,
    /// The resolved stroke opacity.
    pub stroke_opacity: f64,
    /// The resolved edge softening in screen pixels.
    pub blur: f64,
    /// The resolved displacement in screen pixels.
    pub translate: [f64; 2],
}

/// Symbol paint resolved at one zoom and feature.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedSymbolPaint {
    /// The resolved text colour.
    pub text_color: Color,
    /// The resolved text opacity.
    pub text_opacity: f64,
    /// The resolved halo colour.
    pub halo_color: Color,
    /// The resolved halo width in screen pixels.
    pub halo_width: f64,
    /// The resolved halo blur in screen pixels.
    pub halo_blur: f64,
    /// The resolved icon opacity.
    pub icon_opacity: f64,
}

/// Symbol layout resolved against one feature.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedSymbolLayout {
    /// The resolved label text; empty means no label.
    pub text: String,
    /// The resolved font stack.
    pub font_stack: Vec<String>,
    /// The resolved text size in screen pixels.
    pub text_size: f64,
    /// The resolved wrapping width in ems.
    pub text_max_width: f64,
    /// The resolved line height in ems.
    pub text_line_height: f64,
    /// The resolved letter spacing in ems.
    pub text_letter_spacing: f64,
    /// The resolved justification.
    pub text_justify: TextJustify,
    /// The resolved text anchor.
    pub text_anchor: TextAnchor,
    /// The resolved rotation alignment.
    pub text_rotation_alignment: TextRotationAlignment,
    /// Whether a rotated label is flipped upright.
    pub text_keep_upright: bool,
    /// Candidate anchors a point label tries, in order.
    pub variable_anchors: Vec<TextAnchor>,
    /// The resolved radial offset in ems.
    pub radial_offset: f64,
    /// The resolved line-label maximum angle in degrees.
    pub max_angle_degrees: f64,
    /// The resolved case transformation.
    pub text_transform: TextTransform,
    /// The resolved placement.
    pub placement: SymbolPlacement,
    /// The resolved line-label spacing in screen pixels.
    pub symbol_spacing: f64,
    /// Whether the label may overlap others.
    pub allow_overlap: bool,
    /// Whether the label ignores collision placement entirely.
    pub ignore_placement: bool,
    /// Whether a label crossing the viewport edge is dropped.
    pub avoid_edges: bool,
    /// The text's displacement from its anchor, `[x, y]` in ems.
    pub text_offset: [f64; 2],
    /// The transparent margin kept around the text's collision box, in screen
    /// pixels.
    pub text_padding: f64,
    /// The resolved text pitch alignment, with `auto` resolved against the
    /// placement.
    pub text_pitch_alignment: TextPitchAlignment,
    /// How the icon is stretched to fit the text block.
    pub icon_text_fit: IconTextFit,
    /// The inset kept between a fitted icon and the text block,
    /// `[top, right, bottom, left]` in screen pixels.
    pub icon_text_fit_padding: [f64; 4],
    /// The placement order key; lower values place first.
    pub sort_key: f64,
    /// The sprite icon a symbol layer draws, when it names one.
    pub icon_image: Option<String>,
    /// The icon's display scale relative to its sprite-sheet size.
    pub icon_size: f64,
    /// Where the icon sits relative to the symbol anchor.
    pub icon_anchor: TextAnchor,
    /// The icon's displacement from its anchor, `[x, y]` in ems.
    pub icon_offset: [f64; 2],
    /// The icon's clockwise rotation in degrees.
    pub icon_rotate_degrees: f64,
    /// What the icon's rotation follows.
    pub icon_rotation_alignment: TextRotationAlignment,
    /// The transparent margin kept around the icon's collision box, in pixels.
    pub icon_padding: f64,
    /// Whether the icon may overlap other symbols.
    pub icon_allow_overlap: bool,
    /// Whether the icon ignores collision placement entirely.
    pub icon_ignore_placement: bool,
}

/// Compiles a parsed document into render plans.
///
/// The compile step never fails: a property that cannot be represented takes
/// its documented default, and a layer type the renderer does not paint compiles
/// to [`CompiledLayerKind::Other`] with [`CompiledPaint::None`]. Diagnostics
/// from the parse remain available through [`CompiledStyle::unsupported`].
pub fn compile_style_document(document: &MapStyleDocument) -> CompiledStyle {
    let mut unsupported = document.unsupported.clone();
    let sources = compile_sources(document, &mut unsupported);
    let mut background = None;
    let mut layers = Vec::with_capacity(document.layers.len());
    let mut order_index = 0;
    for layer in &document.layers {
        if layer.layer_type == LayerType::Background {
            background = Some(compile_background(layer));
            continue;
        }
        let kind = compiled_layer_kind(layer.layer_type.clone());
        // A known layer type this renderer cannot paint is reported once per
        // layer, so a real style never loses a whole layer kind silently. An
        // unknown type already carries the parser's own diagnostic.
        if matches!(
            layer.layer_type,
            LayerType::Heatmap
                | LayerType::FillExtrusion
                | LayerType::Hillshade
                | LayerType::ColorRelief
        ) {
            unsupported.push(UnsupportedProperty::new(
                UnsupportedScope::Layer {
                    layer_id: layer.id.clone(),
                },
                "type",
                format!(
                    "the layer type {:?} is not painted by this renderer and compiles \
                     without paint",
                    layer.layer_type.as_str()
                ),
            ));
        }
        let source_kind = resolve_layer_source(layer, &sources, &mut unsupported);
        layers.push(CompiledLayer {
            id: layer.id.clone(),
            kind,
            source: layer.source.clone(),
            source_kind,
            source_layer: layer.source_layer.clone(),
            minimum_zoom: layer.min_zoom,
            maximum_zoom: layer.max_zoom,
            order_index,
            filter: layer.filter.clone(),
            paint: compile_paint(kind, layer),
            layout: compile_layout(layer),
            shield: layer.shield.as_ref().map(CompiledShield::from),
            declares_line_presentation: layer.layout.line_cap.is_some()
                || layer.layout.line_join.is_some()
                || layer.paint.line_dasharray.is_some()
                || layer.paint.line_offset.is_some()
                || layer.paint.line_gap_width.is_some()
                || layer.paint.line_blur.is_some(),
            declares_circle_pitch: layer.layout.circle_pitch_scale.is_some()
                || layer.layout.circle_pitch_alignment.is_some(),
            declares_text_max_angle: layer.layout.text_max_angle.is_some(),
            declares_symbol_spacing: layer.layout.symbol_spacing.is_some(),
        });
        order_index += 1;
    }
    CompiledStyle {
        background,
        sprite: document.sprite.as_ref().map(|sprite| CompiledSprite {
            url: sprite.url.clone(),
        }),
        glyphs: document.glyphs.clone(),
        sources,
        layers,
        transition: document.transition,
        unsupported,
    }
}

/// Resolves one layer's source against the compiled source declarations.
///
/// A non-background layer must name a source, and that source must exist.
/// Both failures are diagnostics, never a silent drop: the layer still
/// compiles, and a caller can report exactly why it paints nothing.
fn resolve_layer_source(
    layer: &StyleLayer,
    sources: &BTreeMap<String, CompiledSource>,
    unsupported: &mut Vec<UnsupportedProperty>,
) -> Option<CompiledSourceKind> {
    let scope = UnsupportedScope::Layer {
        layer_id: layer.id.clone(),
    };
    let Some(source_id) = layer.source.as_deref() else {
        unsupported.push(UnsupportedProperty::new(
            scope,
            "source",
            "a layer other than background must name a source",
        ));
        return None;
    };
    match sources.get(source_id) {
        Some(source) => Some(source.kind()),
        None => {
            unsupported.push(UnsupportedProperty::new(
                scope,
                "source",
                format!("unknown source id {source_id:?}"),
            ));
            None
        }
    }
}

/// Compiles every source declaration; the keys keep the document's order.
fn compile_sources(
    document: &MapStyleDocument,
    unsupported: &mut Vec<UnsupportedProperty>,
) -> BTreeMap<String, CompiledSource> {
    document
        .sources
        .iter()
        .map(|(source_id, source_spec)| {
            (
                source_id.clone(),
                compile_source(source_id, source_spec, unsupported),
            )
        })
        .collect()
}

/// Compiles one source declaration into its renderer-neutral plan.
fn compile_source(
    source_id: &str,
    source_spec: &SourceSpec,
    unsupported: &mut Vec<UnsupportedProperty>,
) -> CompiledSource {
    let scope = UnsupportedScope::Source {
        source_id: source_id.to_string(),
    };
    match source_spec {
        SourceSpec::Vector(vector_source) => {
            CompiledSource::Vector(compile_vector_source(vector_source, &scope, unsupported))
        }
        SourceSpec::Raster(raster_source) => {
            CompiledSource::Raster(compile_raster_source(raster_source, &scope, unsupported))
        }
        SourceSpec::GeoJson(geojson_source) => {
            CompiledSource::GeoJson(compile_geojson_source(geojson_source, &scope, unsupported))
        }
        SourceSpec::Opaque(opaque_source) => CompiledSource::Other(CompiledOpaqueSource {
            source_type: opaque_source.source_type.clone(),
        }),
    }
}

/// Compiles a vector source, including its TileJSON `vector_layers`.
fn compile_vector_source(
    source: &VectorSourceSpec,
    scope: &UnsupportedScope,
    unsupported: &mut Vec<UnsupportedProperty>,
) -> CompiledVectorSource {
    CompiledVectorSource {
        url: source.url.clone(),
        tiles: source.tiles.clone(),
        bounds: source.bounds,
        scheme: TileScheme::from_name(source.scheme.as_deref(), scope, unsupported),
        minimum_zoom: source.minimum_zoom,
        maximum_zoom: source.maximum_zoom,
        attribution: source.attribution.clone(),
        promote_id: source.promote_id.clone().unwrap_or(serde_json::Value::Null),
        vector_layers: source.vector_layers.clone(),
    }
}

/// Compiles a raster source.
fn compile_raster_source(
    source: &RasterSourceSpec,
    scope: &UnsupportedScope,
    unsupported: &mut Vec<UnsupportedProperty>,
) -> CompiledRasterSource {
    CompiledRasterSource {
        tiles: source.tiles.clone(),
        bounds: source.bounds,
        scheme: TileScheme::from_name(source.scheme.as_deref(), scope, unsupported),
        minimum_zoom: source.minimum_zoom,
        maximum_zoom: source.maximum_zoom,
        attribution: source.attribution.clone(),
        tile_size: source.tile_size,
    }
}

/// Compiles a GeoJSON source.
fn compile_geojson_source(
    source: &GeoJsonSourceSpec,
    _scope: &UnsupportedScope,
    _unsupported: &mut Vec<UnsupportedProperty>,
) -> CompiledGeoJsonSource {
    CompiledGeoJsonSource {
        data: source.data.clone(),
        data_url: source.data_url.clone(),
        attribution: source.attribution.clone(),
        generate_id: source.generate_id,
        promote_id: source.promote_id.clone().unwrap_or(serde_json::Value::Null),
        buffer: source.buffer,
        tolerance: source.tolerance,
    }
}

/// Maps a document layer type to the renderer's compiled kind.
fn compiled_layer_kind(layer_type: LayerType) -> CompiledLayerKind {
    match layer_type {
        LayerType::Background => CompiledLayerKind::Background,
        LayerType::Fill => CompiledLayerKind::Fill,
        LayerType::Line => CompiledLayerKind::Line,
        LayerType::Circle => CompiledLayerKind::Circle,
        LayerType::Symbol => CompiledLayerKind::Symbol,
        LayerType::Raster => CompiledLayerKind::Raster,
        _ => CompiledLayerKind::Other,
    }
}

/// Builds the background plan; an omitted property takes its documented
/// default.
fn compile_background(layer: &StyleLayer) -> CompiledBackground {
    CompiledBackground {
        color: layer
            .paint
            .background_color
            .clone()
            .unwrap_or(Property::Constant(DEFAULT_BACKGROUND_COLOR)),
        opacity: layer
            .paint
            .background_opacity
            .clone()
            .unwrap_or(Property::Constant(DEFAULT_BACKGROUND_OPACITY)),
    }
}

/// Builds the kind-specific paint plan; every omitted property becomes the
/// documented default so a resolved paint never has to know what was absent.
fn compile_paint(kind: CompiledLayerKind, layer: &StyleLayer) -> CompiledPaint {
    let paint = &layer.paint;
    match kind {
        CompiledLayerKind::Fill => CompiledPaint::Fill(CompiledFillPaint {
            color: or_default(&paint.fill_color, DEFAULT_FILL_COLOR),
            opacity: or_default(&paint.fill_opacity, DEFAULT_FILL_OPACITY),
            outline_color: paint.fill_outline_color.clone(),
            antialias: or_default(&paint.fill_antialias, DEFAULT_FILL_ANTIALIAS),
            translate: or_default(&paint.fill_translate, DEFAULT_FILL_TRANSLATE.to_vec()),
            translate_anchor: paint
                .fill_translate_anchor
                .unwrap_or(DEFAULT_FILL_TRANSLATE_ANCHOR),
            color_transition: paint.fill_color_transition,
            opacity_transition: paint.fill_opacity_transition,
        }),
        CompiledLayerKind::Line => CompiledPaint::Line(CompiledLinePaint {
            color: or_default(&paint.line_color, DEFAULT_LINE_COLOR),
            width: or_default(&paint.line_width, DEFAULT_LINE_WIDTH),
            opacity: or_default(&paint.line_opacity, DEFAULT_LINE_OPACITY),
            color_transition: paint.line_color_transition,
            opacity_transition: paint.line_opacity_transition,
            blur: or_default(&paint.line_blur, DEFAULT_LINE_BLUR),
            offset: or_default(&paint.line_offset, DEFAULT_LINE_OFFSET),
            gap_width: or_default(&paint.line_gap_width, DEFAULT_LINE_GAP_WIDTH),
            dasharray: or_default(&paint.line_dasharray, Vec::new()),
            translate: or_default(&paint.line_translate, DEFAULT_LINE_TRANSLATE.to_vec()),
            translate_anchor: paint
                .line_translate_anchor
                .unwrap_or(DEFAULT_LINE_TRANSLATE_ANCHOR),
        }),
        CompiledLayerKind::Circle => CompiledPaint::Circle(CompiledCirclePaint {
            color: or_default(&paint.circle_color, DEFAULT_CIRCLE_COLOR),
            radius: or_default(&paint.circle_radius, DEFAULT_CIRCLE_RADIUS),
            opacity: or_default(&paint.circle_opacity, DEFAULT_CIRCLE_OPACITY),
            color_transition: paint.circle_color_transition,
            opacity_transition: paint.circle_opacity_transition,
            stroke_color: paint.circle_stroke_color.clone(),
            stroke_width: or_default(&paint.circle_stroke_width, DEFAULT_CIRCLE_STROKE_WIDTH),
            stroke_opacity: or_default(&paint.circle_stroke_opacity, DEFAULT_CIRCLE_STROKE_OPACITY),
            blur: or_default(&paint.circle_blur, DEFAULT_CIRCLE_BLUR),
            translate: or_default(&paint.circle_translate, DEFAULT_CIRCLE_TRANSLATE.to_vec()),
        }),
        CompiledLayerKind::Symbol => CompiledPaint::Symbol(CompiledSymbolPaint {
            text_color: or_default(&paint.text_color, DEFAULT_TEXT_COLOR),
            text_opacity: or_default(&paint.text_opacity, DEFAULT_TEXT_OPACITY),
            halo_color: or_default(&paint.text_halo_color, DEFAULT_TEXT_HALO_COLOR),
            halo_width: or_default(&paint.text_halo_width, DEFAULT_TEXT_HALO_WIDTH),
            halo_blur: or_default(&paint.text_halo_blur, DEFAULT_TEXT_HALO_BLUR),
            icon_opacity: or_default(&paint.icon_opacity, DEFAULT_ICON_OPACITY),
        }),
        CompiledLayerKind::Background | CompiledLayerKind::Other | CompiledLayerKind::Raster => {
            CompiledPaint::None
        }
    }
}

/// Builds the layout plan with the documented defaults filled in.
fn compile_layout(layer: &StyleLayer) -> CompiledLayout {
    let layout = &layer.layout;
    CompiledLayout {
        visibility: or_default(&layout.visibility, DEFAULT_VISIBILITY),
        line_cap: or_default(&layout.line_cap, DEFAULT_LINE_CAP),
        line_join: or_default(&layout.line_join, DEFAULT_LINE_JOIN),
        line_sort_key: or_default(&layout.line_sort_key, DEFAULT_LINE_SORT_KEY),
        fill_sort_key: or_default(&layout.fill_sort_key, DEFAULT_FILL_SORT_KEY),
        circle_sort_key: or_default(&layout.circle_sort_key, DEFAULT_CIRCLE_SORT_KEY),
        circle_pitch_scale: or_default(&layout.circle_pitch_scale, DEFAULT_CIRCLE_PITCH_SCALE),
        circle_pitch_alignment: or_default(
            &layout.circle_pitch_alignment,
            DEFAULT_CIRCLE_PITCH_ALIGNMENT,
        ),
        text_field: or_default(&layout.text_field, String::new()),
        text_font: or_default(&layout.text_font, default_text_font_stack()),
        text_size: or_default(&layout.text_size, DEFAULT_TEXT_SIZE),
        text_max_width: or_default(&layout.text_max_width, DEFAULT_TEXT_MAX_WIDTH),
        text_line_height: or_default(&layout.text_line_height, DEFAULT_TEXT_LINE_HEIGHT),
        text_letter_spacing: or_default(&layout.text_letter_spacing, DEFAULT_TEXT_LETTER_SPACING),
        text_justify: or_default(&layout.text_justify, DEFAULT_TEXT_JUSTIFY),
        text_anchor: or_default(&layout.text_anchor, DEFAULT_TEXT_ANCHOR),
        text_rotation_alignment: or_default(
            &layout.text_rotation_alignment,
            DEFAULT_TEXT_ROTATION_ALIGNMENT,
        ),
        text_keep_upright: or_default(&layout.text_keep_upright, DEFAULT_TEXT_KEEP_UPRIGHT),
        text_variable_anchor: or_default(&layout.text_variable_anchor, Vec::new()),
        text_radial_offset: or_default(&layout.text_radial_offset, DEFAULT_TEXT_RADIAL_OFFSET),
        text_max_angle: or_default(&layout.text_max_angle, DEFAULT_TEXT_MAX_ANGLE),
        text_transform: or_default(&layout.text_transform, DEFAULT_TEXT_TRANSFORM),
        symbol_placement: or_default(&layout.symbol_placement, DEFAULT_SYMBOL_PLACEMENT),
        symbol_spacing: or_default(&layout.symbol_spacing, DEFAULT_SYMBOL_SPACING),
        text_allow_overlap: or_default(&layout.text_allow_overlap, DEFAULT_TEXT_ALLOW_OVERLAP),
        text_ignore_placement: or_default(
            &layout.text_ignore_placement,
            DEFAULT_TEXT_IGNORE_PLACEMENT,
        ),
        symbol_avoid_edges: or_default(&layout.symbol_avoid_edges, DEFAULT_SYMBOL_AVOID_EDGES),
        text_offset: or_default(&layout.text_offset, DEFAULT_TEXT_OFFSET.to_vec()),
        text_padding: or_default(&layout.text_padding, DEFAULT_TEXT_PADDING),
        text_pitch_alignment: or_default(
            &layout.text_pitch_alignment,
            DEFAULT_TEXT_PITCH_ALIGNMENT,
        ),
        symbol_sort_key: or_default(&layout.symbol_sort_key, DEFAULT_SYMBOL_SORT_KEY),
        icon_image: layout.icon_image.clone(),
        icon_size: or_default(&layout.icon_size, DEFAULT_ICON_SIZE),
        icon_anchor: or_default(&layout.icon_anchor, DEFAULT_ICON_ANCHOR),
        icon_offset: or_default(&layout.icon_offset, DEFAULT_ICON_OFFSET.to_vec()),
        icon_rotate: or_default(&layout.icon_rotate, DEFAULT_ICON_ROTATE),
        icon_rotation_alignment: or_default(
            &layout.icon_rotation_alignment,
            DEFAULT_ICON_ROTATION_ALIGNMENT,
        ),
        icon_padding: or_default(&layout.icon_padding, DEFAULT_ICON_PADDING),
        icon_allow_overlap: or_default(&layout.icon_allow_overlap, DEFAULT_ICON_ALLOW_OVERLAP),
        icon_ignore_placement: or_default(
            &layout.icon_ignore_placement,
            DEFAULT_ICON_IGNORE_PLACEMENT,
        ),
        icon_text_fit: or_default(&layout.icon_text_fit, DEFAULT_ICON_TEXT_FIT),
        icon_text_fit_padding: or_default(
            &layout.icon_text_fit_padding,
            DEFAULT_ICON_TEXT_FIT_PADDING.to_vec(),
        ),
    }
}

/// The default font stack as owned values.
fn default_text_font_stack() -> Vec<String> {
    DEFAULT_COMPILED_TEXT_FONT_STACK
        .iter()
        .map(|font_family| (*font_family).to_string())
        .collect()
}

/// A present property, or its default as a constant.
fn or_default<T: PropertyValue>(property: &Option<Property<T>>, default: T) -> Property<T> {
    property.clone().unwrap_or(Property::Constant(default))
}

/// The interpolation fraction for one property: its own declared transition,
/// else the document's root transition, else one (draw the new value).
fn transition_progress(
    declared: Option<TransitionSpec>,
    root_transition: Option<TransitionSpec>,
    elapsed_seconds: f64,
) -> f64 {
    declared
        .or(root_transition)
        .map_or(1.0, |transition| transition.progress_at(elapsed_seconds))
}

/// Linear interpolation between two scalars.
fn blend(from: f64, to: f64, fraction: f64) -> f64 {
    from + (to - from) * fraction
}

/// Resolves a property, substituting the documented default on failure.
fn resolve<T: PropertyValue>(
    property: &Property<T>,
    default: T,
    context: &EvaluationContext<'_>,
) -> T {
    property.evaluate(context).unwrap_or(default)
}

/// Resolves a two-component translate property, falling back to the
/// documented value when the property yields anything but exactly two
/// numbers.
fn resolve_translate(
    property: &Property<Vec<f64>>,
    default: [f64; 2],
    context: &EvaluationContext<'_>,
) -> [f64; 2] {
    let components = resolve(property, default.to_vec(), context);
    match components.as_slice() {
        [x_component, y_component] => [*x_component, *y_component],
        _ => default,
    }
}

/// Resolves a four-component padding property (`[top, right, bottom, left]`),
/// falling back to the documented value when the property yields anything but
/// four finite non-negative numbers. A negative inset would grow the fitted
/// icon instead of padding it, so it clamps to zero.
fn resolve_padding_quad(
    property: &Property<Vec<f64>>,
    default: [f64; 4],
    context: &EvaluationContext<'_>,
) -> [f64; 4] {
    let components = resolve(property, default.to_vec(), context);
    match components.as_slice() {
        [top, right, bottom, left]
            if [top, right, bottom, left]
                .iter()
                .all(|component| component.is_finite()) =>
        {
            [top.max(0.0), right.max(0.0), bottom.max(0.0), left.max(0.0)]
        }
        _ => default,
    }
}

/// Resolves a symbol layer's final label text: the text-field expression
/// evaluated against the feature, legacy `{property}` tokens substituted, and
/// the layer's case transformation applied.
///
/// A layer with a badge takes its label from the shield path instead: the
/// feature's `ref` attribute, the route reference the badge encloses. Its
/// text-field is not consulted, so a badge never paints a road name.
fn resolved_symbol_text(layer: &CompiledLayer, context: &EvaluationContext<'_>) -> String {
    let text = match &layer.shield {
        Some(_) => shield_reference_text(context).unwrap_or_default(),
        None => layer
            .layout
            .text_field
            .evaluate(context)
            .unwrap_or_default(),
    };
    let text = substitute_text_tokens(&text, context);
    match resolve(
        &layer.layout.text_transform,
        DEFAULT_TEXT_TRANSFORM,
        context,
    ) {
        TextTransform::Uppercase => text.to_uppercase(),
        TextTransform::Lowercase => text.to_lowercase(),
        TextTransform::None => text,
    }
}

/// The feature's route reference, the badge's label: the `ref` attribute as
/// text. An absent or non-textual `ref` reports `None`, so the badge candidate
/// is skipped rather than painted empty.
fn shield_reference_text(context: &EvaluationContext<'_>) -> Option<String> {
    let reference_value = context.properties?.get("ref")?;
    <String as PropertyValue>::from_value(reference_value).filter(|reference| !reference.is_empty())
}

/// Replaces `{property}` tokens in a text-field literal with the feature's
/// values, the specification's legacy token syntax. An unknown token or one
/// with no feature context renders as the empty string.
fn substitute_text_tokens(text: &str, context: &EvaluationContext<'_>) -> String {
    if !text.contains('{') {
        return text.to_string();
    }
    let mut substituted = String::with_capacity(text.len());
    let mut remaining = text;
    while let Some(open_index) = remaining.find('{') {
        substituted.push_str(&remaining[..open_index]);
        let after_open = &remaining[open_index + 1..];
        let Some(close_index) = after_open.find('}') else {
            substituted.push_str(&remaining[open_index..]);
            return substituted;
        };
        let token_name = &after_open[..close_index];
        if let Some(properties) = context.properties
            && let Some(token_value) = properties.get(token_name)
        {
            substituted.push_str(&token_value_text(token_value));
        }
        remaining = &after_open[close_index + 1..];
    }
    substituted.push_str(remaining);
    substituted
}

/// A property value as token text: strings verbatim, whole numbers without a
/// fractional part, booleans spelled out, and anything else empty.
fn token_value_text(value: &crate::types::Value) -> String {
    match value {
        crate::types::Value::String(text) => text.clone(),
        crate::types::Value::Number(number) => {
            if number.fract() == 0.0 {
                format!("{number:.0}")
            } else {
                number.to_string()
            }
        }
        crate::types::Value::Boolean(flag) => flag.to_string(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Value;

    const FILL_AND_SYMBOL_STYLE_JSON: &str = r##"{
        "version": 8,
        "sources": { "base": { "type": "vector" } },
        "layers": [
            {
                "id": "water",
                "type": "fill",
                "source": "base",
                "source-layer": "water",
                "minzoom": 4,
                "maxzoom": 20,
                "filter": ["==", ["get", "class"], "lake"],
                "paint": { "fill-color": [10, 20, 30, 1], "fill-opacity": 0.5 }
            },
            {
                "id": "road",
                "type": "line",
                "source": "base",
                "source-layer": "transportation",
                "paint": {
                    "line-color": "#ff0000",
                    "line-width": ["interpolate", ["linear"], ["zoom"], 10, 2, 14, 6]
                }
            },
            {
                "id": "place",
                "type": "symbol",
                "source": "base",
                "source-layer": "place",
                "layout": {
                    "text-field": ["get", "name"],
                    "text-size": ["step", ["zoom"], 10, 12, 14],
                    "symbol-placement": "point",
                    "symbol-sort-key": 2,
                    "text-allow-overlap": true
                },
                "paint": { "text-color": "#112233", "text-halo-width": 1.5 }
            },
            {
                "id": "ground",
                "type": "background",
                "paint": { "background-color": [240, 230, 220, 1] }
            }
        ]
    }"##;

    fn compiled_fixture() -> CompiledStyle {
        let document = MapStyleDocument::parse_json(FILL_AND_SYMBOL_STYLE_JSON)
            .expect("the fixture style parses");
        compile_style_document(&document)
    }

    #[test]
    fn compilation_keeps_document_order_and_background_separate() {
        let style = compiled_fixture();
        let ids: Vec<&str> = style.layers.iter().map(|layer| layer.id.as_str()).collect();
        assert_eq!(ids, ["water", "road", "place"]);
        for (index, layer) in style.layers.iter().enumerate() {
            assert_eq!(layer.order_index, index);
        }
        let background = style.background.as_ref().expect("the background compiles");
        let context = EvaluationContext::at_zoom(0.0);
        let rgba = background.resolved_rgba(&context);
        assert_eq!(rgba[0], (240.0_f64 / 255.0) as f32);
        assert_eq!(rgba[3], 1.0);
    }

    #[test]
    fn paint_expressions_resolve_at_the_frame_zoom() {
        let style = compiled_fixture();
        let road = style.layer("road").expect("the road layer compiles");
        let at_ten = road.resolved_paint(&EvaluationContext::at_zoom(10.0));
        let at_twelve = road.resolved_paint(&EvaluationContext::at_zoom(12.0));
        match (at_ten, at_twelve) {
            (ResolvedPaint::Line(ten), ResolvedPaint::Line(twelve)) => {
                assert!((ten.width - 2.0).abs() < 1e-12);
                assert!((twelve.width - 4.0).abs() < 1e-12);
                assert_eq!(ten.color, Color::new(1.0, 0.0, 0.0, 1.0));
            }
            other => panic!("expected line paint, got {other:?}"),
        }
    }

    #[test]
    fn line_layout_and_casing_paint_carry_the_declared_values() {
        let document = MapStyleDocument::parse_json(
            r#"{
                "version": 8,
                "sources": { "base": { "type": "vector" } },
                "layers": [
                    {
                        "id": "casing",
                        "type": "line",
                        "source": "base",
                        "source-layer": "transportation",
                        "layout": {
                            "line-cap": "round",
                            "line-join": "bevel",
                            "line-sort-key": ["get", "rank"]
                        },
                        "paint": {
                            "line-width": 3,
                            "line-offset": 4,
                            "line-gap-width": 10,
                            "line-blur": 0.5,
                            "line-dasharray": [2, 1]
                        }
                    }
                ]
            }"#,
        )
        .expect("the casing style parses");
        let style = compile_style_document(&document);
        let casing = style.layer("casing").expect("the casing layer compiles");
        let context = EvaluationContext::at_zoom(12.0);
        assert_eq!(casing.resolved_line_cap(&context), LineCap::Round);
        assert_eq!(casing.resolved_line_join(&context), LineJoin::Bevel);
        match casing.resolved_paint(&context) {
            ResolvedPaint::Line(paint) => {
                assert_eq!(paint.width, 3.0);
                assert_eq!(paint.offset, 4.0);
                assert_eq!(paint.gap_width, 10.0);
                assert_eq!(paint.blur, 0.5);
                assert_eq!(paint.dasharray, vec![2.0, 1.0]);
            }
            other => panic!("expected line paint, got {other:?}"),
        }
        let mut properties = std::collections::BTreeMap::new();
        properties.insert("rank".to_string(), Value::from(3.0));
        let feature_context = EvaluationContext::at_zoom(12.0)
            .with_feature(&properties, crate::expression::GeometryType::LineString);
        assert_eq!(casing.resolved_line_sort_key(&feature_context), 3.0);
    }

    #[test]
    fn fill_and_circle_paint_carry_outline_stroke_translation_and_pitch() {
        let document = MapStyleDocument::parse_json(
            r##"{
                "version": 8,
                "sources": { "base": { "type": "vector" } },
                "layers": [
                    {
                        "id": "water",
                        "type": "fill",
                        "source": "base",
                        "source-layer": "water",
                        "paint": {
                            "fill-color": "#102030",
                            "fill-opacity": 0.8,
                            "fill-outline-color": "#405060",
                            "fill-antialias": false,
                            "fill-translate": [3, -2]
                        }
                    },
                    {
                        "id": "poi",
                        "type": "circle",
                        "source": "base",
                        "source-layer": "poi",
                        "layout": {
                            "circle-pitch-scale": "viewport",
                            "circle-pitch-alignment": "map"
                        },
                        "paint": {
                            "circle-radius": 6,
                            "circle-stroke-color": "#ffffff",
                            "circle-stroke-width": 2,
                            "circle-stroke-opacity": 0.5,
                            "circle-blur": 0.75,
                            "circle-translate": [1, 2]
                        }
                    }
                ]
            }"##,
        )
        .expect("the fill and circle style parses");
        let style = compile_style_document(&document);
        let context = EvaluationContext::at_zoom(0.0);
        let water = style.layer("water").expect("the water layer compiles");
        match water.resolved_paint(&context) {
            ResolvedPaint::Fill(paint) => {
                assert!(!paint.antialias);
                assert_eq!(paint.translate, [3.0, -2.0]);
                assert_eq!(
                    paint.outline_color,
                    Some(Color::from_css("#405060").expect("the outline colour parses"))
                );
                assert!((paint.opacity - 0.8).abs() < 1e-12);
            }
            other => panic!("expected fill paint, got {other:?}"),
        }
        let poi = style.layer("poi").expect("the poi layer compiles");
        assert_eq!(
            poi.resolved_circle_pitch_scale(&context),
            CirclePitchScale::Viewport
        );
        assert_eq!(
            poi.resolved_circle_pitch_alignment(&context),
            CirclePitchAlignment::Map
        );
        match poi.resolved_paint(&context) {
            ResolvedPaint::Circle(paint) => {
                assert_eq!(paint.stroke_width, 2.0);
                assert_eq!(paint.stroke_opacity, 0.5);
                assert_eq!(paint.blur, 0.75);
                assert_eq!(paint.translate, [1.0, 2.0]);
                assert_eq!(
                    paint.stroke_color,
                    Some(Color::from_css("#ffffff").expect("the stroke colour parses"))
                );
            }
            other => panic!("expected circle paint, got {other:?}"),
        }
    }

    #[test]
    fn symbol_text_applies_tokens_transform_offset_and_edge_rules() {
        let document = MapStyleDocument::parse_json(
            r##"{
                "version": 8,
                "sources": { "base": { "type": "vector" } },
                "layers": [
                    {
                        "id": "place",
                        "type": "symbol",
                        "source": "base",
                        "source-layer": "place",
                        "layout": {
                            "text-field": "{name} [{count}] {missing}",
                            "text-transform": "uppercase",
                            "text-offset": [1, 2],
                            "symbol-avoid-edges": true,
                            "text-ignore-placement": true
                        }
                    }
                ]
            }"##,
        )
        .expect("the symbol style parses");
        let style = compile_style_document(&document);
        let place = style.layer("place").expect("the place layer compiles");
        let mut properties = std::collections::BTreeMap::new();
        properties.insert("name".to_string(), Value::from("Main Street"));
        properties.insert("count".to_string(), Value::from(7.0));
        let context = EvaluationContext::at_zoom(12.0)
            .with_feature(&properties, crate::expression::GeometryType::Point);
        let layout = place.resolved_symbol_layout(&context);
        assert_eq!(layout.text, "MAIN STREET [7] ");
        assert_eq!(layout.text_offset, [1.0, 2.0]);
        assert!(layout.avoid_edges);
        assert!(layout.ignore_placement);
    }

    #[test]
    fn symbol_text_layout_carries_wrapping_line_height_spacing_and_justify() {
        let document = MapStyleDocument::parse_json(
            r#"{
                "version": 8,
                "sources": { "base": { "type": "vector" } },
                "layers": [
                    {
                        "id": "place",
                        "type": "symbol",
                        "source": "base",
                        "source-layer": "place",
                        "layout": {
                            "text-field": ["get", "name"],
                            "text-max-width": 6,
                            "text-line-height": 1.4,
                            "text-letter-spacing": 0.25,
                            "text-justify": "right"
                        }
                    }
                ]
            }"#,
        )
        .expect("the symbol style parses");
        let style = compile_style_document(&document);
        let place = style.layer("place").expect("the place layer compiles");
        let mut properties = std::collections::BTreeMap::new();
        properties.insert("name".to_string(), Value::from("West Sacramento"));
        let context = EvaluationContext::at_zoom(12.0)
            .with_feature(&properties, crate::expression::GeometryType::Point);
        let layout = place.resolved_symbol_layout(&context);
        assert_eq!(layout.text_max_width, 6.0);
        assert_eq!(layout.text_line_height, 1.4);
        assert_eq!(layout.text_letter_spacing, 0.25);
        assert_eq!(layout.text_justify, TextJustify::Right);
    }

    #[test]
    fn symbol_text_anchor_and_rotation_resolve() {
        let document = MapStyleDocument::parse_json(
            r#"{
                "version": 8,
                "sources": { "base": { "type": "vector" } },
                "layers": [
                    {
                        "id": "place",
                        "type": "symbol",
                        "source": "base",
                        "source-layer": "place",
                        "layout": {
                            "text-field": ["get", "name"],
                            "text-anchor": "top-left",
                            "text-rotation-alignment": "map",
                            "text-keep-upright": false
                        }
                    }
                ]
            }"#,
        )
        .expect("the symbol style parses");
        let style = compile_style_document(&document);
        let place = style.layer("place").expect("the place layer compiles");
        let mut properties = std::collections::BTreeMap::new();
        properties.insert("name".to_string(), Value::from("Sacramento"));
        let context = EvaluationContext::at_zoom(12.0)
            .with_feature(&properties, crate::expression::GeometryType::Point);
        let layout = place.resolved_symbol_layout(&context);
        assert_eq!(layout.text_anchor, TextAnchor::TopLeft);
        assert_eq!(layout.text_rotation_alignment, TextRotationAlignment::Map);
        assert!(!layout.text_keep_upright);
    }

    #[test]
    fn symbol_variable_anchor_radial_offset_and_max_angle_resolve() {
        let document = MapStyleDocument::parse_json(
            r#"{
                "version": 8,
                "sources": { "base": { "type": "vector" } },
                "layers": [
                    {
                        "id": "place",
                        "type": "symbol",
                        "source": "base",
                        "source-layer": "place",
                        "layout": {
                            "text-field": ["get", "name"],
                            "text-variable-anchor": ["top", "bottom", "left"],
                            "text-radial-offset": 1.5,
                            "text-max-angle": 60
                        }
                    }
                ]
            }"#,
        )
        .expect("the symbol style parses");
        let style = compile_style_document(&document);
        let place = style.layer("place").expect("the place layer compiles");
        let mut properties = std::collections::BTreeMap::new();
        properties.insert("name".to_string(), Value::from("Sacramento"));
        let context = EvaluationContext::at_zoom(12.0)
            .with_feature(&properties, crate::expression::GeometryType::Point);
        let layout = place.resolved_symbol_layout(&context);
        assert_eq!(
            layout.variable_anchors,
            vec![TextAnchor::Top, TextAnchor::Bottom, TextAnchor::Left]
        );
        assert_eq!(layout.radial_offset, 1.5);
        assert_eq!(layout.max_angle_degrees, 60.0);
        assert!(place.declares_text_max_angle);
    }

    #[test]
    fn filters_evaluate_against_feature_properties() {
        let style = compiled_fixture();
        let water = style.layer("water").expect("the water layer compiles");
        let mut properties = std::collections::BTreeMap::new();
        properties.insert("class".to_string(), Value::from("lake"));
        let context = EvaluationContext::at_zoom(10.0)
            .with_feature(&properties, crate::expression::GeometryType::Polygon);
        assert!(water.matches_filter(&context));
        properties.insert("class".to_string(), Value::from("river"));
        let context = EvaluationContext::at_zoom(10.0)
            .with_feature(&properties, crate::expression::GeometryType::Polygon);
        assert!(!water.matches_filter(&context));
    }

    #[test]
    fn symbol_layout_carries_the_sort_key_and_overlap_rule() {
        let style = compiled_fixture();
        let place = style.layer("place").expect("the place layer compiles");
        assert_eq!(place.kind, CompiledLayerKind::Symbol);
        let context = EvaluationContext::at_zoom(12.0);
        let layout = place.resolved_symbol_layout(&context);
        assert_eq!(layout.text_size, 14.0);
        assert_eq!(layout.sort_key, 2.0);
        assert!(layout.allow_overlap);
        assert_eq!(layout.placement, SymbolPlacement::Point);
        assert_eq!(
            place.resolved_paint(&context),
            ResolvedPaint::Symbol(ResolvedSymbolPaint {
                text_color: Color::from_css("#112233").expect("the fixture colour parses"),
                text_opacity: 1.0,
                halo_color: DEFAULT_TEXT_HALO_COLOR,
                halo_width: 1.5,
                halo_blur: 0.0,
                icon_opacity: 1.0,
            })
        );
    }

    #[test]
    fn zoom_spans_are_resolved_and_inclusive_at_the_lower_edge() {
        let style = compiled_fixture();
        let water = style.layer("water").expect("the water layer compiles");
        assert_eq!(water.minimum_zoom, 4.0);
        assert_eq!(water.maximum_zoom, 20.0);
        assert!(!water.is_visible_at_zoom(3.999));
        assert!(water.is_visible_at_zoom(4.0));
        assert!(water.is_visible_at_zoom(19.999));
        assert!(!water.is_visible_at_zoom(20.0));
    }

    #[test]
    fn an_unimplemented_layer_type_compiles_to_other_without_failing() {
        let document = MapStyleDocument::parse_json(
            r#"{
                "version": 8,
                "sources": { "base": { "type": "vector" } },
                "layers": [
                    { "id": "relief", "type": "hillshade", "source": "base" }
                ]
            }"#,
        )
        .expect("the document parses");
        let style = compile_style_document(&document);
        assert_eq!(style.layers.len(), 1);
        assert_eq!(style.layers[0].kind, CompiledLayerKind::Other);
        assert_eq!(style.layers[0].paint, CompiledPaint::None);
        assert!(
            style.unsupported.iter().any(|diagnostic| {
                matches!(
                    &diagnostic.scope,
                    UnsupportedScope::Layer { layer_id } if layer_id == "relief"
                ) && diagnostic.property_name == "type"
                    && diagnostic.detail.contains("hillshade")
            }),
            "a known layer kind this renderer cannot paint is named: {:?}",
            style.unsupported
        );
    }

    #[test]
    fn every_unrendered_layer_kind_is_reported_once() {
        let document = MapStyleDocument::parse_json(
            r#"{
                "version": 8,
                "sources": { "base": { "type": "vector" } },
                "layers": [
                    { "id": "heat", "type": "heatmap", "source": "base" },
                    { "id": "blocks", "type": "fill-extrusion", "source": "base" },
                    { "id": "relief", "type": "hillshade", "source": "base" },
                    { "id": "terrain", "type": "color-relief", "source": "base" }
                ]
            }"#,
        )
        .expect("the document parses");
        let style = compile_style_document(&document);
        for kind in ["heatmap", "fill-extrusion", "hillshade", "color-relief"] {
            let diagnostic_count = style
                .unsupported
                .iter()
                .filter(|diagnostic| {
                    diagnostic.property_name == "type" && diagnostic.detail.contains(kind)
                })
                .count();
            assert_eq!(
                diagnostic_count, 1,
                "the {kind} layer kind is reported exactly once"
            );
        }
    }

    #[test]
    fn the_symbol_layout_extensions_resolve_declared_and_default_values() {
        let document = MapStyleDocument::parse_json(
            r#"{
                "version": 8,
                "sources": { "base": { "type": "vector" } },
                "layers": [
                    {
                        "id": "labels",
                        "type": "symbol",
                        "source": "base",
                        "source-layer": "places",
                        "layout": {
                            "text-field": "declared",
                            "text-padding": 4,
                            "text-pitch-alignment": "map",
                            "icon-image": "park",
                            "icon-text-fit": "both",
                            "icon-text-fit-padding": [1, 2, 3, 4]
                        },
                        "paint": { "icon-opacity": 0.5 }
                    },
                    {
                        "id": "plain",
                        "type": "symbol",
                        "source": "base",
                        "source-layer": "places",
                        "layout": { "text-field": "plain" }
                    },
                    {
                        "id": "roads",
                        "type": "line",
                        "source": "base",
                        "source-layer": "roads",
                        "paint": {
                            "line-translate": [3, -2],
                            "line-translate-anchor": "viewport"
                        }
                    }
                ]
            }"#,
        )
        .expect("the style parses");
        let style = compile_style_document(&document);
        assert!(
            style.unsupported.is_empty(),
            "the extensions are applied, not reported: {:?}",
            style.unsupported
        );
        let context = EvaluationContext::at_zoom(10.0);
        let declared = style
            .layer("labels")
            .expect("the labels layer compiles")
            .resolved_symbol_layout(&context);
        assert_eq!(declared.text_padding, 4.0);
        assert_eq!(declared.text_pitch_alignment, TextPitchAlignment::Map);
        assert_eq!(declared.icon_text_fit, IconTextFit::Both);
        assert_eq!(declared.icon_text_fit_padding, [1.0, 2.0, 3.0, 4.0]);
        let ResolvedPaint::Symbol(paint) = style
            .layer("labels")
            .expect("the labels layer compiles")
            .resolved_paint(&context)
        else {
            panic!("a symbol layer resolves symbol paint");
        };
        assert_eq!(paint.icon_opacity, 0.5);

        let plain = style
            .layer("plain")
            .expect("the plain layer compiles")
            .resolved_symbol_layout(&context);
        assert_eq!(plain.text_padding, DEFAULT_TEXT_PADDING);
        assert_eq!(
            plain.text_pitch_alignment,
            TextPitchAlignment::Viewport,
            "an omitted auto alignment faces the screen for a point-placed label"
        );
        assert_eq!(plain.icon_text_fit, IconTextFit::None);
        assert_eq!(plain.icon_text_fit_padding, [0.0; 4]);

        let ResolvedPaint::Line(line) = style
            .layer("roads")
            .expect("the roads layer compiles")
            .resolved_paint(&context)
        else {
            panic!("a line layer resolves line paint");
        };
        assert_eq!(line.translate, [3.0, -2.0]);
        assert_eq!(line.translate_anchor, TranslateAnchor::Viewport);
    }

    #[test]
    fn a_fill_pattern_degrades_with_a_named_diagnostic() {
        let document = MapStyleDocument::parse_json(
            r##"{
                "version": 8,
                "sources": { "base": { "type": "vector" } },
                "layers": [
                    {
                        "id": "water",
                        "type": "fill",
                        "source": "base",
                        "paint": { "fill-color": "#336699", "fill-pattern": "waves" }
                    }
                ]
            }"##,
        )
        .expect("the style parses");
        let style = compile_style_document(&document);
        let pattern_diagnostic = style
            .unsupported
            .iter()
            .find(|diagnostic| diagnostic.property_name == "fill-pattern")
            .expect("the dropped pattern is reported");
        assert!(
            pattern_diagnostic.detail.contains("draws its colour"),
            "the diagnostic names the degradation: {}",
            pattern_diagnostic.detail
        );
    }

    #[test]
    fn several_sources_and_their_tilejson_fields_are_modelled() {
        let document = MapStyleDocument::parse_json(
            r#"{
                "version": 8,
                "sources": {
                    "roads": {
                        "type": "vector",
                        "url": "https://example.test/tiles.json",
                        "tiles": ["https://example.test/{z}/{x}/{y}.mvt"],
                        "minzoom": 4,
                        "maxzoom": 14,
                        "bounds": [-123.0, 37.0, -121.0, 39.0],
                        "scheme": "tms",
                        "attribution": "Road data",
                        "vector_layers": [
                            {
                                "id": "transportation",
                                "fields": { "class": "String", "rank": "Number" },
                                "minzoom": 5,
                                "maxzoom": 14,
                                "description": "Roads"
                            }
                        ]
                    },
                    "imagery": {
                        "type": "raster",
                        "tiles": ["https://example.test/{z}/{x}/{y}.png"],
                        "tileSize": 256,
                        "scheme": "xyz"
                    },
                    "places": { "type": "geojson", "data": { "type": "FeatureCollection", "features": [] } }
                },
                "layers": [
                    { "id": "roads", "type": "line", "source": "roads", "source-layer": "transportation" },
                    { "id": "imagery", "type": "raster", "source": "imagery" },
                    { "id": "places", "type": "circle", "source": "places" }
                ]
            }"#,
        )
        .expect("the document parses");
        let style = compile_style_document(&document);
        assert_eq!(style.sources.len(), 3);
        let Some(CompiledSource::Vector(roads)) = style.source("roads") else {
            panic!("roads compiles to a vector source");
        };
        assert_eq!(roads.scheme, TileScheme::Tms);
        assert_eq!(roads.minimum_zoom, Some(4.0));
        assert_eq!(roads.maximum_zoom, Some(14.0));
        assert_eq!(roads.bounds, Some([-123.0, 37.0, -121.0, 39.0]));
        assert_eq!(roads.attribution.as_deref(), Some("Road data"));
        assert_eq!(roads.vector_layers.len(), 1);
        assert_eq!(roads.vector_layers[0].id, "transportation");
        assert_eq!(
            roads.vector_layers[0]
                .fields
                .get("class")
                .map(String::as_str),
            Some("String")
        );
        assert!(roads.declares_source_layer("transportation"));
        let Some(CompiledSource::Raster(imagery)) = style.source("imagery") else {
            panic!("imagery compiles to a raster source");
        };
        assert_eq!(imagery.tile_size, Some(256.0));
        assert_eq!(imagery.scheme, TileScheme::Xyz);
        assert_eq!(
            style.layers[0].source_kind,
            Some(CompiledSourceKind::Vector)
        );
        assert_eq!(
            style.layers[1].source_kind,
            Some(CompiledSourceKind::Raster)
        );
        assert_eq!(style.layers[1].kind, CompiledLayerKind::Raster);
        assert_eq!(
            style.layers[2].source_kind,
            Some(CompiledSourceKind::GeoJson)
        );
        assert!(style.unsupported.is_empty());
    }

    #[test]
    fn a_sprite_declaration_compiles_to_its_base_url() {
        let document = MapStyleDocument::parse_json(
            r#"{
                "version": 8,
                "sprite": "fixture://icons",
                "sources": { "base": { "type": "vector" } },
                "layers": []
            }"#,
        )
        .expect("the style parses");
        let style = compile_style_document(&document);
        let sprite = style.sprite.as_ref().expect("the sprite compiles");
        assert_eq!(sprite.url, "fixture://icons");

        let object_sprite = MapStyleDocument::parse_json(
            r#"{
                "version": 8,
                "sprite": { "default": "fixture://object-icons", "@2x": "fixture://object-icons@2x" },
                "sources": { "base": { "type": "vector" } },
                "layers": []
            }"#,
        )
        .expect("the object sprite style parses");
        let object_style = compile_style_document(&object_sprite);
        assert_eq!(
            object_style
                .sprite
                .as_ref()
                .map(|sprite| sprite.url.as_str()),
            Some("fixture://object-icons")
        );
    }

    #[test]
    fn a_glyph_endpoint_compiles_and_the_symbol_font_stacks_are_distinct() {
        let document = MapStyleDocument::parse_json(
            r#"{
                "version": 8,
                "glyphs": "https://glyphs.example.test/{fontstack}/{range}.pbf",
                "sources": { "base": { "type": "vector" } },
                "layers": [
                    {
                        "id": "place-labels",
                        "type": "symbol",
                        "source": "base",
                        "source-layer": "place",
                        "layout": { "text-field": ["get", "name"], "text-font": ["Noto Sans Regular", "Noto Sans Bold"] }
                    },
                    {
                        "id": "road-labels",
                        "type": "symbol",
                        "source": "base",
                        "source-layer": "road",
                        "layout": { "text-field": ["get", "name"], "text-font": ["Noto Sans Regular", "Noto Sans Bold"] }
                    },
                    {
                        "id": "road-lines",
                        "type": "line",
                        "source": "base",
                        "source-layer": "road"
                    }
                ]
            }"#,
        )
        .expect("the style parses");
        let style = compile_style_document(&document);
        assert_eq!(
            style.glyphs.as_deref(),
            Some("https://glyphs.example.test/{fontstack}/{range}.pbf")
        );
        assert_eq!(
            style.symbol_font_stacks(),
            vec!["Noto Sans Regular,Noto Sans Bold".to_string()],
            "two symbol layers naming one stack contribute one entry, and a line layer contributes none"
        );
    }

    #[test]
    fn icon_layout_properties_resolve_with_the_specification_defaults() {
        let document = MapStyleDocument::parse_json(
            r#"{
                "version": 8,
                "sources": { "base": { "type": "vector" } },
                "layers": [
                    {
                        "id": "poi",
                        "type": "symbol",
                        "source": "base",
                        "source-layer": "poi",
                        "layout": {
                            "icon-image": ["get", "icon"],
                            "icon-size": 1.5,
                            "icon-anchor": "bottom",
                            "icon-offset": [1, -2],
                            "icon-rotate": 30,
                            "icon-rotation-alignment": "map",
                            "icon-padding": 4,
                            "icon-allow-overlap": true,
                            "icon-ignore-placement": true
                        }
                    },
                    {
                        "id": "plain",
                        "type": "symbol",
                        "source": "base",
                        "source-layer": "place"
                    }
                ]
            }"#,
        )
        .expect("the icon style parses");
        let style = compile_style_document(&document);
        let mut properties = std::collections::BTreeMap::new();
        properties.insert("icon".to_string(), Value::from("railway"));
        let context = EvaluationContext::at_zoom(12.0)
            .with_feature(&properties, crate::expression::GeometryType::Point);
        let poi = style.layer("poi").expect("the poi layer compiles");
        let layout = poi.resolved_symbol_layout(&context);
        assert_eq!(layout.icon_image.as_deref(), Some("railway"));
        assert_eq!(layout.icon_size, 1.5);
        assert_eq!(layout.icon_anchor, TextAnchor::Bottom);
        assert_eq!(layout.icon_offset, [1.0, -2.0]);
        assert_eq!(layout.icon_rotate_degrees, 30.0);
        assert_eq!(layout.icon_rotation_alignment, TextRotationAlignment::Map);
        assert_eq!(layout.icon_padding, 4.0);
        assert!(layout.icon_allow_overlap);
        assert!(layout.icon_ignore_placement);

        let plain = style.layer("plain").expect("the plain layer compiles");
        let plain_layout = plain.resolved_symbol_layout(&context);
        assert!(plain_layout.icon_image.is_none());
        assert_eq!(plain_layout.icon_size, 1.0);
        assert_eq!(plain_layout.icon_anchor, TextAnchor::Center);
        assert_eq!(plain_layout.icon_offset, [0.0, 0.0]);
        assert_eq!(plain_layout.icon_rotate_degrees, 0.0);
        assert_eq!(
            plain_layout.icon_rotation_alignment,
            TextRotationAlignment::Auto
        );
        assert_eq!(plain_layout.icon_padding, 2.0);
        assert!(!plain_layout.icon_allow_overlap);
        assert!(!plain_layout.icon_ignore_placement);
    }

    #[test]
    fn a_layer_naming_an_unknown_source_is_a_diagnostic_not_a_silent_drop() {
        let document = MapStyleDocument::parse_json(
            r#"{
                "version": 8,
                "sources": { "base": { "type": "vector" } },
                "layers": [
                    { "id": "missing", "type": "line", "source": "elsewhere" },
                    { "id": "sourceless", "type": "fill" }
                ]
            }"#,
        )
        .expect("the document parses");
        let style = compile_style_document(&document);
        assert_eq!(style.layers.len(), 2);
        assert_eq!(style.layers[0].source_kind, None);
        let unknown_source_diagnostic = style
            .unsupported
            .iter()
            .find(|property| {
                property.property_name == "source"
                    && matches!(
                        &property.scope,
                        UnsupportedScope::Layer { layer_id } if layer_id == "missing"
                    )
            })
            .expect("the unknown source is reported");
        assert!(unknown_source_diagnostic.detail.contains("elsewhere"));
        assert!(style.unsupported.iter().any(|property| {
            property.property_name == "source"
                && matches!(
                    &property.scope,
                    UnsupportedScope::Layer { layer_id } if layer_id == "sourceless"
                )
        }));
    }

    fn fill_style(fill_color: &str, fill_opacity: f64, transition: &str) -> CompiledStyle {
        let document = MapStyleDocument::parse_json(&format!(
            r#"{{
                "version": 8,
                "sources": {{ "base": {{ "type": "vector" }} }}{transition},
                "layers": [
                    {{
                        "id": "water",
                        "type": "fill",
                        "source": "base",
                        "source-layer": "water",
                        "layout": {{ "fill-sort-key": ["get", "rank"] }},
                        "paint": {{
                            "fill-color": "{fill_color}",
                            "fill-opacity": {fill_opacity},
                            "fill-translate": [5, -3],
                            "fill-translate-anchor": "viewport"
                        }}
                    }}
                ]
            }}"#
        ))
        .expect("the transition fixture style parses");
        compile_style_document(&document)
    }

    #[test]
    fn fill_layout_carries_the_sort_key_and_translate_anchor() {
        let style = fill_style("#ff0000", 0.6, "");
        let layer = style.layer("water").expect("the water layer compiles");
        let context = EvaluationContext::at_zoom(0.0);
        match layer.resolved_paint(&context) {
            ResolvedPaint::Fill(paint) => {
                assert_eq!(paint.translate, [5.0, -3.0]);
                assert_eq!(paint.translate_anchor, TranslateAnchor::Viewport);
            }
            other => panic!("expected fill paint, got {other:?}"),
        }
        assert_eq!(
            layer.resolved_fill_sort_key(&context),
            DEFAULT_FILL_SORT_KEY
        );
        assert_eq!(
            layer.resolved_circle_sort_key(&context),
            DEFAULT_CIRCLE_SORT_KEY
        );
    }

    #[test]
    fn an_absent_transition_leaves_the_style_with_no_lifetime() {
        let style = fill_style("#ff0000", 0.6, "");
        assert_eq!(style.transition, None);
        assert_eq!(style.transition_lifetime_seconds(), 0.0);
    }

    #[test]
    fn a_property_transition_overrides_the_root_and_blends_over_its_own_duration() {
        let previous = fill_style("#ff0000", 0.6, "");
        let document = MapStyleDocument::parse_json(
            r##"{
                "version": 8,
                "sources": { "base": { "type": "vector" } },
                "transition": { "duration": 400 },
                "layers": [
                    {
                        "id": "water",
                        "type": "fill",
                        "source": "base",
                        "source-layer": "water",
                        "paint": {
                            "fill-color": "#0000ff",
                            "fill-opacity": 0.2,
                            "fill-opacity-transition": { "duration": 1000 }
                        }
                    }
                ]
            }"##,
        )
        .expect("the override fixture style parses");
        let current = compile_style_document(&document);
        assert_eq!(current.transition_lifetime_seconds(), 1.0);
        let previous_layer = previous.layer("water").expect("the previous layer");
        let current_layer = current.layer("water").expect("the current layer");
        let context = EvaluationContext::at_zoom(0.0);
        // Half of the opacity's 1000 ms: the colour has landed at the root's
        // 400 ms, the opacity has not.
        match current_layer.resolved_transitioned_paint(
            &context,
            current.transition,
            Some((previous_layer, 0.5)),
        ) {
            ResolvedPaint::Fill(paint) => {
                assert!((paint.color.blue - 1.0).abs() < 1e-12);
                assert!((paint.color.red).abs() < 1e-12);
                assert!((paint.opacity - 0.4).abs() < 1e-12);
            }
            other => panic!("expected fill paint, got {other:?}"),
        }
        // At the start the previous values are still fully in force.
        match current_layer.resolved_transitioned_paint(
            &context,
            current.transition,
            Some((previous_layer, 0.0)),
        ) {
            ResolvedPaint::Fill(paint) => {
                assert!((paint.color.red - 1.0).abs() < 1e-12);
                assert!((paint.opacity - 0.6).abs() < 1e-12);
            }
            other => panic!("expected fill paint, got {other:?}"),
        }
    }

    #[test]
    fn a_shield_layer_takes_its_text_and_paint_from_the_shield_block() {
        let document = MapStyleDocument::parse_json(
            r##"{
                "version": 8,
                "sources": { "base": { "type": "vector" } },
                "layers": [{
                    "id": "shields",
                    "type": "symbol",
                    "source": "base",
                    "source-layer": "routes",
                    "layout": { "text-field": ["get", "name"], "text-size": 10 },
                    "paint": { "text-color": "#111111" },
                    "shield": {
                        "shape": "rounded_rectangle",
                        "fill_rgba": "#ffffff",
                        "text_rgba": "#ff0000",
                        "text_size_pixels": 11,
                        "padding_pixels": 3,
                        "minimum_size_pixels": 18
                    }
                }]
            }"##,
        )
        .expect("the style parses");
        let style = compile_style_document(&document);
        let layer = style.layers.first().expect("the layer compiles");
        let shield = layer.shield.as_ref().expect("the shield compiles");
        assert_eq!(shield.shape, ShieldShape::RoundedRectangle);
        assert_eq!(shield.text_color, Color::new(1.0, 0.0, 0.0, 1.0));
        assert!((shield.text_size_pixels - 11.0).abs() < 1e-12);

        let mut properties = BTreeMap::new();
        properties.insert(
            "name".to_string(),
            Value::String("West Capitol Avenue".to_string()),
        );
        properties.insert("ref".to_string(), Value::String("I 80".to_string()));
        let context = EvaluationContext::at_zoom(14.0)
            .with_feature(&properties, crate::GeometryType::LineString);
        let layout = layer.resolved_symbol_layout(&context);
        assert_eq!(layout.text, "I 80");
        assert!((layout.text_size - 11.0).abs() < 1e-12);
        match layer.resolved_paint(&context) {
            ResolvedPaint::Symbol(paint) => {
                assert_eq!(paint.text_color, Color::new(1.0, 0.0, 0.0, 1.0));
                assert!((paint.text_opacity - 1.0).abs() < 1e-12);
            }
            other => panic!("expected symbol paint, got {other:?}"),
        }
    }

    #[test]
    fn a_shield_layer_without_a_reference_paints_no_label() {
        let document = MapStyleDocument::parse_json(
            r##"{
                "version": 8,
                "sources": { "base": { "type": "vector" } },
                "layers": [{
                    "id": "shields",
                    "type": "symbol",
                    "source": "base",
                    "source-layer": "routes",
                    "layout": { "text-field": ["get", "name"] },
                    "shield": {}
                }]
            }"##,
        )
        .expect("the style parses");
        let style = compile_style_document(&document);
        let layer = style.layers.first().expect("the layer compiles");
        let mut properties = BTreeMap::new();
        properties.insert(
            "name".to_string(),
            Value::String("West Capitol Avenue".to_string()),
        );
        let context = EvaluationContext::at_zoom(14.0)
            .with_feature(&properties, crate::GeometryType::LineString);
        assert_eq!(layer.resolved_symbol_layout(&context).text, "");
    }
}
