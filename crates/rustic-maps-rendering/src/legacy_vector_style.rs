//! Vector-tile styling: a decoded [`rustic_maps_vector_tiles::VectorTile`] becomes
//! the renderer's overlay primitives — filled polygons for polygon features,
//! polylines for line features, point markers for point features — under a
//! small explicit style.
//!
//! The style is deliberately data-only: a layer entry names the source layer,
//! the geometry kind it paints, an optional one-attribute text filter and the
//! paint values (RGBA, line width, marker radius). A symbol layer declares the
//! label style a text pass will use — ordered text-field fallbacks, font
//! stack, size stops, text and halo paint, [`VectorSymbolPlacement`], priority
//! and overlap — but paints no overlay primitive itself: the frame-level label
//! pass ([`crate::labels`]) reads it and draws its text through the glyph
//! atlas, either anchored at a point or run along a line feature. The style
//! carries no expression language. The artifact
//! [`DEFAULT_VECTOR_STYLE_ARTIFACT_JSON`] carries the default style;
//! [`default_vector_tile_style`] parses it and [`in_code_default_vector_tile_style`]
//! is the documented fallback when the artifact is missing or malformed.
//!
//! Polygon features paint every exterior ring with its interior rings
//! subtracted through the overlay's hole-aware fill (see
//! [`crate::overlay::FilledPolygonOverlay`]), and the conversion groups its
//! primitives into one [`VectorOverlayBatch`] per style layer, which a
//! dedicated vector pipeline can consume.
//!
//! Coordinates convert from tile-local extent space (`x` right, `y` down, the
//! MVT convention) through the tile's web-mercator address to geographic
//! degrees, so the overlays project exactly like every other map coordinate.

use crate::overlay::{FilledPolygonOverlay, MapOverlay, PointMarkersOverlay, RoutePolylineOverlay};
use geospatial_types::GeographicCoordinate;
use rustic_maps_vector_tiles::{
    VectorTile, VectorTileAttributeValue, VectorTileFeature, VectorTileGeometry,
    VectorTileGeometryPoint, VectorTileGeometryType, VectorTileLinearRing,
};
use serde::Deserialize;
use std::f64::consts::PI;

/// The longitude span the web-mercator world covers, in degrees.
const WORLD_LONGITUDE_SPAN_DEGREES: f64 = 360.0;
/// The longitude at the world's western edge, in degrees.
const WORLD_WEST_LONGITUDE_DEGREES: f64 = -180.0;
/// The latitude the mercator projection approaches at the world's edges;
/// re-exported from `geospatial-types` so every layer shares one value.
pub use geospatial_types::MAXIMUM_MERCATOR_LATITUDE_DEGREES as MERCATOR_MAXIMUM_LATITUDE_DEGREES;

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

/// The primitive a style layer paints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorStyleGeometryKind {
    /// Polygon features as filled exterior rings with their holes subtracted.
    Fill,
    /// LineString features as screen-space-thickened polylines.
    Line,
    /// Point features as screen-facing markers.
    Circle,
    /// A declarative label layer: its text style is data for the frame-level
    /// label pass ([`crate::labels`]), so the layer paints no overlay
    /// primitive of its own.
    Symbol,
}

/// Where a symbol layer anchors its labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorSymbolPlacement {
    /// Labels anchor at the feature's point.
    Point,
    /// Labels run along a line feature: the label pass centers the text on
    /// the feature's longest straight projected run and rotates it to that
    /// run's upright bearing.
    Line,
}

/// A one-attribute filter: the feature must carry the named attribute with one
/// of the filter's accepted values.
///
/// The accepted values are alternatives, so a style can hold several text
/// spellings for one class (`matches_any`) or match a numeric attribute such
/// as `admin_level` (`number_equals`) without enumerating its text form. A
/// filter with every alternative empty matches no feature at all, which is
/// the honest reading of an unusable filter rather than "match everything".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorAttributeFilter {
    /// The attribute key to match.
    pub attribute_name: String,
    /// The exact text value to match (empty when unused).
    pub expected_text: String,
    /// Any one of these text values also matches (empty when unused).
    pub matches_any: Vec<String>,
    /// A numeric value the attribute may carry (any integer, matched through
    /// [`VectorTileAttributeValue::Number`]; `None` when unused).
    pub number_equals: Option<i64>,
}

/// One zoom stop of a style layer's width: the width the layer paints from
/// this zoom level upward until the next stop (linear interpolation between
/// stops, the last stop's width beyond it).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VectorWidthZoomStop {
    /// The camera zoom level the stop applies from.
    pub zoom_level: f64,
    /// The line width in screen pixels at this zoom level.
    pub width_pixels: f64,
}

/// One zoom stop of a symbol layer's text size: the size the layer paints from
/// this zoom level upward until the next stop (linear interpolation between
/// stops, the last stop's size beyond it).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VectorTextSizeZoomStop {
    /// The camera zoom level the stop applies from.
    pub zoom_level: f64,
    /// The text size in screen pixels at this zoom level.
    pub size_pixels: f64,
}

/// One zoom stop of a symbol layer's feature-rank ceiling: from this zoom
/// level upward, a symbol layer draws only features whose `rank` attribute is
/// at or below [`maximum_rank`](Self::maximum_rank). Rank is ordinal, so the
/// stops step rather than interpolate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VectorRankZoomStop {
    /// The camera zoom level the stop applies from.
    pub zoom_level: f64,
    /// The highest feature rank the layer still draws at this zoom level.
    pub maximum_rank: u32,
}

/// One group of feature classes that a symbol layer holds back until a closer
/// zoom level. OpenMapTiles carries the feature's own class (`shop`,
/// `restaurant`, `bus`, …); a class listed by any group draws only from that
/// group's minimum zoom level upward, while an unlisted class follows the
/// layer's own [`min_zoom_level`](VectorLayerStyle::min_zoom_level). When two
/// groups name the same class, the higher minimum zoom level wins.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorClassMinimumZoom {
    /// The feature `class` attribute values the gate applies to.
    pub classes: Vec<String>,
    /// The lowest zoom level at which those classes draw.
    pub minimum_zoom_level: f64,
}

/// One style layer: source layer, painted kind, optional filter and paint,
/// plus the zoom span and width stops the layer is evaluated at.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorLayerStyle {
    /// The decoded layer name the style paints (`water`, `highways`, …).
    pub source_layer: String,
    /// The geometry kind the style paints.
    pub geometry_kind: VectorStyleGeometryKind,
    /// An optional one-attribute text filter.
    pub filter: Option<VectorAttributeFilter>,
    /// Fill RGBA (used by [`VectorStyleGeometryKind::Fill`]).
    pub fill_rgba: [f32; 4],
    /// Line RGBA (used by [`VectorStyleGeometryKind::Line`]).
    pub line_rgba: [f32; 4],
    /// Line width in screen pixels.
    pub line_width_pixels: f64,
    /// Marker RGBA (used by [`VectorStyleGeometryKind::Circle`]).
    pub circle_rgba: [f32; 4],
    /// Marker radius in screen pixels.
    pub circle_radius_pixels: f64,
    /// The lowest zoom level (inclusive) the layer paints at; below it the
    /// layer is invisible.
    pub min_zoom_level: f64,
    /// The highest zoom level (exclusive) the layer paints at; at or above it
    /// the layer is invisible.
    pub max_zoom_level: f64,
    /// The layer's width stops, ascending by zoom. Empty keeps
    /// `line_width_pixels` at every zoom.
    pub width_zoom_stops: Vec<VectorWidthZoomStop>,
    /// Ordered attribute keys a symbol layer tries for its label text: the
    /// first key the feature carries wins (`name:latin`, then `name`, then
    /// `ref` by default).
    pub text_field_fallbacks: Vec<String>,
    /// The font family stack a symbol layer asks for, most preferred first.
    pub font_stack: Vec<String>,
    /// Text RGBA (used by [`VectorStyleGeometryKind::Symbol`]).
    pub text_rgba: [f32; 4],
    /// Text size in screen pixels when the layer carries no size stops.
    pub text_size_pixels: f64,
    /// The layer's text-size stops, ascending by zoom. Empty keeps
    /// `text_size_pixels` at every zoom.
    pub text_size_zoom_stops: Vec<VectorTextSizeZoomStop>,
    /// Halo RGBA drawn behind the text (used by
    /// [`VectorStyleGeometryKind::Symbol`]).
    pub halo_rgba: [f32; 4],
    /// Halo width in screen pixels; zero paints no halo.
    pub halo_width_pixels: f64,
    /// Where a symbol layer anchors its labels.
    pub placement: VectorSymbolPlacement,
    /// The label's priority rank: lower ranks are more important and draw
    /// first when two labels collide.
    pub priority: u32,
    /// Whether a symbol layer may draw labels that overlap other labels.
    pub allow_overlap: bool,
    /// Feature-rank ceiling stops of a symbol layer, ascending by zoom level.
    /// Empty means no ceiling: every ranked feature draws at every zoom.
    pub rank_zoom_stops: Vec<VectorRankZoomStop>,
    /// Per-class minimum zoom levels of a symbol layer: classes that are not
    /// useful at a wide zoom (`shop`, `bus`, …) wait until the map is closer.
    /// Empty means every class follows `min_zoom_level`.
    pub class_minimum_zooms: Vec<VectorClassMinimumZoom>,
}

impl VectorLayerStyle {
    /// Builds a fill layer with the given paint.
    pub fn fill(source_layer: impl Into<String>, rgba_color: [f32; 4]) -> Self {
        Self::base(source_layer, VectorStyleGeometryKind::Fill, rgba_color)
    }

    /// Builds a line layer with the given paint and width.
    pub fn line(source_layer: impl Into<String>, rgba_color: [f32; 4], width_pixels: f64) -> Self {
        let mut layer_style = Self::base(source_layer, VectorStyleGeometryKind::Line, rgba_color);
        layer_style.line_rgba = rgba_color;
        layer_style.line_width_pixels = width_pixels;
        layer_style
    }

    /// Builds a circle layer with the given paint and radius.
    pub fn circle(
        source_layer: impl Into<String>,
        rgba_color: [f32; 4],
        radius_pixels: f64,
    ) -> Self {
        let mut layer_style = Self::base(source_layer, VectorStyleGeometryKind::Circle, rgba_color);
        layer_style.circle_rgba = rgba_color;
        layer_style.circle_radius_pixels = radius_pixels;
        layer_style
    }

    /// Builds a symbol layer with the given text paint and size. Its label
    /// fields take the documented defaults; the label pass reads them later.
    pub fn symbol(
        source_layer: impl Into<String>,
        text_rgba: [f32; 4],
        text_size_pixels: f64,
    ) -> Self {
        let mut layer_style = Self::base(source_layer, VectorStyleGeometryKind::Symbol, text_rgba);
        layer_style.text_rgba = text_rgba;
        layer_style.text_size_pixels = text_size_pixels;
        layer_style
    }

    /// Sets the layer's zoom span: the layer paints only while
    /// `min_zoom_level <= zoom < max_zoom_level`.
    pub fn with_zoom_span(mut self, min_zoom_level: f64, max_zoom_level: f64) -> Self {
        self.min_zoom_level = min_zoom_level;
        self.max_zoom_level = max_zoom_level;
        self
    }

    /// Replaces the ordered text-attribute fallbacks a symbol layer reads its
    /// label text from (for example `["ref"]` for route shields).
    pub fn with_text_field_fallbacks(mut self, fallbacks: &[&str]) -> Self {
        self.text_field_fallbacks = fallbacks
            .iter()
            .map(|fallback| (*fallback).to_string())
            .collect();
        self
    }

    /// Adds a width stop (see [`VectorWidthZoomStop`]); stops are kept
    /// ascending by zoom level.
    pub fn with_width_zoom_stop(mut self, zoom_level: f64, width_pixels: f64) -> Self {
        self.width_zoom_stops.push(VectorWidthZoomStop {
            zoom_level,
            width_pixels,
        });
        self.width_zoom_stops
            .sort_by(|first, second| first.zoom_level.total_cmp(&second.zoom_level));
        self
    }

    /// Adds a text-size stop (see [`VectorTextSizeZoomStop`]); stops are kept
    /// ascending by zoom level.
    pub fn with_text_size_zoom_stop(mut self, zoom_level: f64, size_pixels: f64) -> Self {
        self.text_size_zoom_stops.push(VectorTextSizeZoomStop {
            zoom_level,
            size_pixels,
        });
        self.text_size_zoom_stops
            .sort_by(|first, second| first.zoom_level.total_cmp(&second.zoom_level));
        self
    }

    /// Sets the symbol layer's halo paint behind the text.
    pub fn with_halo(mut self, rgba_color: [f32; 4], width_pixels: f64) -> Self {
        self.halo_rgba = rgba_color;
        self.halo_width_pixels = width_pixels;
        self
    }

    /// Sets where a symbol layer anchors its labels.
    pub fn with_placement(mut self, placement: VectorSymbolPlacement) -> Self {
        self.placement = placement;
        self
    }

    /// Sets the label priority rank (lower ranks draw first).
    pub fn with_priority(mut self, priority: u32) -> Self {
        self.priority = priority;
        self
    }

    /// Sets whether labels of this symbol layer may overlap other labels.
    pub fn with_allow_overlap(mut self, allow_overlap: bool) -> Self {
        self.allow_overlap = allow_overlap;
        self
    }

    /// Adds a feature-rank ceiling stop (see [`VectorRankZoomStop`]); stops are
    /// kept ascending by zoom level.
    pub fn with_rank_zoom_stop(mut self, zoom_level: f64, maximum_rank: u32) -> Self {
        self.rank_zoom_stops.push(VectorRankZoomStop {
            zoom_level,
            maximum_rank,
        });
        self.rank_zoom_stops
            .sort_by(|first, second| first.zoom_level.total_cmp(&second.zoom_level));
        self
    }

    /// Adds a group of feature classes held back until `minimum_zoom_level`
    /// (see [`VectorClassMinimumZoom`]).
    pub fn with_class_minimum_zoom(
        mut self,
        classes: impl IntoIterator<Item = impl Into<String>>,
        minimum_zoom_level: f64,
    ) -> Self {
        self.class_minimum_zooms.push(VectorClassMinimumZoom {
            classes: classes.into_iter().map(Into::into).collect(),
            minimum_zoom_level,
        });
        self
    }

    /// Whether the layer paints at `zoom_level`.
    pub fn is_visible_at_zoom(&self, zoom_level: f64) -> bool {
        zoom_level >= self.min_zoom_level && zoom_level < self.max_zoom_level
    }

    /// The feature-rank ceiling the layer draws under at `zoom_level`: the
    /// highest-rank feature that still earns a label. The stops step rather
    /// than interpolate, because rank is ordinal; below the first stop the
    /// first stop's ceiling applies, and a layer with no stops returns
    /// [`DEFAULT_VECTOR_SYMBOL_MAXIMUM_RANK`] (no ceiling).
    pub fn maximum_rank_at_zoom(&self, zoom_level: f64) -> u32 {
        let Some(first_stop) = self.rank_zoom_stops.first() else {
            return DEFAULT_VECTOR_SYMBOL_MAXIMUM_RANK;
        };
        let mut maximum_rank = first_stop.maximum_rank;
        for stop in &self.rank_zoom_stops {
            if zoom_level < stop.zoom_level {
                break;
            }
            maximum_rank = stop.maximum_rank;
        }
        maximum_rank
    }

    /// Whether a feature of `feature_class` (the feature's `class` attribute,
    /// when it carries one) draws at `zoom_level` under this layer's
    /// per-class minimum zooms. A feature with no class, or a class no group
    /// names, is governed by the layer's own zoom span instead.
    pub fn is_feature_class_visible_at_zoom(
        &self,
        feature_class: Option<&str>,
        zoom_level: f64,
    ) -> bool {
        let Some(feature_class) = feature_class else {
            return true;
        };
        let mut minimum_zoom_level: Option<f64> = None;
        for class_minimum_zoom in &self.class_minimum_zooms {
            if class_minimum_zoom
                .classes
                .iter()
                .any(|class| class == feature_class)
            {
                minimum_zoom_level = Some(
                    minimum_zoom_level.map_or(class_minimum_zoom.minimum_zoom_level, |current| {
                        current.max(class_minimum_zoom.minimum_zoom_level)
                    }),
                );
            }
        }
        minimum_zoom_level.is_none_or(|minimum| zoom_level >= minimum)
    }

    /// The line width the layer paints at `zoom_level`: the fixed width when
    /// the layer carries no stops, the linearly interpolated stop value
    /// between two stops, or the nearest stop's value outside the stop range.
    pub fn width_pixels_at_zoom(&self, zoom_level: f64) -> f64 {
        let Some(first_stop) = self.width_zoom_stops.first() else {
            return self.line_width_pixels;
        };
        if zoom_level <= first_stop.zoom_level {
            return first_stop.width_pixels;
        }
        for stop_pair in self.width_zoom_stops.windows(2) {
            let [lower_stop, upper_stop] = [stop_pair[0], stop_pair[1]];
            if zoom_level < upper_stop.zoom_level {
                let span = upper_stop.zoom_level - lower_stop.zoom_level;
                if span <= 0.0 {
                    return upper_stop.width_pixels;
                }
                let fraction = (zoom_level - lower_stop.zoom_level) / span;
                return lower_stop.width_pixels
                    + (upper_stop.width_pixels - lower_stop.width_pixels) * fraction;
            }
        }
        self.width_zoom_stops
            .last()
            .map(|last_stop| last_stop.width_pixels)
            .unwrap_or(self.line_width_pixels)
    }

    /// The text size the layer paints at `zoom_level`: the fixed size when the
    /// layer carries no stops, the linearly interpolated stop value between
    /// two stops, or the nearest stop's value outside the stop range.
    pub fn text_size_pixels_at_zoom(&self, zoom_level: f64) -> f64 {
        let Some(first_stop) = self.text_size_zoom_stops.first() else {
            return self.text_size_pixels;
        };
        if zoom_level <= first_stop.zoom_level {
            return first_stop.size_pixels;
        }
        for stop_pair in self.text_size_zoom_stops.windows(2) {
            let [lower_stop, upper_stop] = [stop_pair[0], stop_pair[1]];
            if zoom_level < upper_stop.zoom_level {
                let span = upper_stop.zoom_level - lower_stop.zoom_level;
                if span <= 0.0 {
                    return upper_stop.size_pixels;
                }
                let fraction = (zoom_level - lower_stop.zoom_level) / span;
                return lower_stop.size_pixels
                    + (upper_stop.size_pixels - lower_stop.size_pixels) * fraction;
            }
        }
        self.text_size_zoom_stops
            .last()
            .map(|last_stop| last_stop.size_pixels)
            .unwrap_or(self.text_size_pixels)
    }

    /// Adds the one-attribute text filter.
    pub fn with_filter(
        mut self,
        attribute_name: impl Into<String>,
        expected_text: impl Into<String>,
    ) -> Self {
        self.filter = Some(VectorAttributeFilter {
            attribute_name: attribute_name.into(),
            expected_text: expected_text.into(),
            matches_any: Vec::new(),
            number_equals: None,
        });
        self
    }

    /// Adds a filter with the full set of accepted values (several text
    /// spellings and/or a numeric equality); the artifact parser uses this
    /// form so every condition the style declares reaches the matcher.
    pub fn with_attribute_filter(mut self, filter: VectorAttributeFilter) -> Self {
        self.filter = Some(filter);
        self
    }

    fn base(
        source_layer: impl Into<String>,
        geometry_kind: VectorStyleGeometryKind,
        rgba_color: [f32; 4],
    ) -> Self {
        Self {
            source_layer: source_layer.into(),
            geometry_kind,
            filter: None,
            fill_rgba: rgba_color,
            line_rgba: rgba_color,
            line_width_pixels: DEFAULT_VECTOR_LINE_WIDTH_PIXELS,
            circle_rgba: rgba_color,
            circle_radius_pixels: DEFAULT_VECTOR_CIRCLE_RADIUS_PIXELS,
            min_zoom_level: DEFAULT_VECTOR_STYLE_MINIMUM_ZOOM_LEVEL,
            max_zoom_level: DEFAULT_VECTOR_STYLE_MAXIMUM_ZOOM_LEVEL,
            width_zoom_stops: Vec::new(),
            text_field_fallbacks: default_symbol_text_field_fallbacks(),
            font_stack: default_symbol_font_stack(),
            text_rgba: rgba_color,
            text_size_pixels: DEFAULT_VECTOR_SYMBOL_TEXT_SIZE_PIXELS,
            text_size_zoom_stops: Vec::new(),
            halo_rgba: DEFAULT_VECTOR_SYMBOL_HALO_RGBA,
            halo_width_pixels: DEFAULT_VECTOR_SYMBOL_HALO_WIDTH_PIXELS,
            placement: DEFAULT_VECTOR_SYMBOL_PLACEMENT,
            priority: DEFAULT_VECTOR_SYMBOL_PRIORITY,
            allow_overlap: DEFAULT_VECTOR_SYMBOL_ALLOW_OVERLAP,
            rank_zoom_stops: Vec::new(),
            class_minimum_zooms: Vec::new(),
        }
    }
}

/// The default line width a fill/circle layer carries when it paints a line.
pub const DEFAULT_VECTOR_LINE_WIDTH_PIXELS: f64 = 1.5;
/// The zoom level from which a layer paints unless it declares a `min_zoom`.
pub const DEFAULT_VECTOR_STYLE_MINIMUM_ZOOM_LEVEL: f64 = 0.0;
/// The zoom level at which a layer stops painting unless it declares a
/// `max_zoom` (one past the renderer's own zoom ceiling).
pub const DEFAULT_VECTOR_STYLE_MAXIMUM_ZOOM_LEVEL: f64 = 24.0;
/// The default marker radius a fill/line layer carries when it paints circles.
pub const DEFAULT_VECTOR_CIRCLE_RADIUS_PIXELS: f64 = 3.0;
/// The paint a fill, line or circle layer uses when the artifact omits `rgba`
/// (opaque black; every built-in style carries its own paint).
pub const DEFAULT_VECTOR_STYLE_PAINT_RGBA: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
/// The ordered text-attribute fallbacks a symbol layer uses when the style
/// omits them: the Latin name, then the local name. A route reference is not a
/// fallback: a layer without a shield block paints no reference rather than a
/// road name.
pub const DEFAULT_VECTOR_SYMBOL_TEXT_FIELD_FALLBACKS: &[&str] = &["name:latin", "name"];
/// The font stack a symbol layer asks for when the style omits one.
pub const DEFAULT_VECTOR_SYMBOL_FONT_STACK: &[&str] = &["Noto Sans Regular"];
/// The text size a symbol layer paints at when it carries no text-size stops.
pub const DEFAULT_VECTOR_SYMBOL_TEXT_SIZE_PIXELS: f64 = 12.0;
/// The text colour a symbol layer uses when the style omits `text_rgba`.
pub const DEFAULT_VECTOR_SYMBOL_TEXT_RGBA: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
/// The halo colour a symbol layer uses when the style omits `halo_rgba`.
pub const DEFAULT_VECTOR_SYMBOL_HALO_RGBA: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
/// The halo width in screen pixels a symbol layer uses when the style omits
/// it; zero paints no halo.
pub const DEFAULT_VECTOR_SYMBOL_HALO_WIDTH_PIXELS: f64 = 1.0;
/// The placement a symbol layer uses when the style omits one.
pub const DEFAULT_VECTOR_SYMBOL_PLACEMENT: VectorSymbolPlacement = VectorSymbolPlacement::Point;
/// The priority rank a symbol layer uses when the style omits one; lower
/// ranks are more important and draw first.
pub const DEFAULT_VECTOR_SYMBOL_PRIORITY: u32 = 0;
/// Whether a symbol layer allows overlapping labels when the style omits it.
pub const DEFAULT_VECTOR_SYMBOL_ALLOW_OVERLAP: bool = false;
/// The feature-rank ceiling a symbol layer uses when it carries no rank stops:
/// the highest possible rank, so every ranked feature passes (no ceiling).
pub const DEFAULT_VECTOR_SYMBOL_MAXIMUM_RANK: u32 = u32::MAX;

/// The default symbol text-field fallbacks as owned values.
fn default_symbol_text_field_fallbacks() -> Vec<String> {
    DEFAULT_VECTOR_SYMBOL_TEXT_FIELD_FALLBACKS
        .iter()
        .map(|attribute_name| (*attribute_name).to_string())
        .collect()
}

/// The default symbol font stack as owned values.
fn default_symbol_font_stack() -> Vec<String> {
    DEFAULT_VECTOR_SYMBOL_FONT_STACK
        .iter()
        .map(|font_family| (*font_family).to_string())
        .collect()
}

/// A whole-tile style: one layer entry per painted source layer, plus the
/// style's own background.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct VectorTileStyle {
    /// The style's canvas background, or `None` when the style paints its own
    /// full ground fill (the browser base then keeps the page's clear color).
    /// A named style carries its upstream background, so the vector base
    /// clears to the color the style was designed on.
    pub background: Option<[f32; 4]>,
    /// The style's layer entries, applied in order.
    pub layers: Vec<VectorLayerStyle>,
}

/// One style layer's overlay primitives: the grouped result of
/// [`overlay_primitives_for_vector_tile`], keyed by the style that paints
/// them. The overlay pipeline draws one flat vertex list per frame, so a batch
/// lets a dedicated vector pipeline draw one pass per style without
/// recollecting the tile's features.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorOverlayBatch {
    /// The style layer whose paint every overlay in the batch uses.
    pub layer_style: VectorLayerStyle,
    /// The layer's overlay primitives, in feature order.
    pub overlays: Vec<MapOverlay>,
}

/// The built-in data-only style artifact the default style loads from.
pub const DEFAULT_VECTOR_STYLE_ARTIFACT_JSON: &str =
    include_str!("../fixtures/render/vector-style.json");

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

/// The dark OSM-Carto-like artifact: the same layer structure as the default,
/// painted for a dark map surface.
const CARTO_DARK_STYLE_ARTIFACT_JSON: &str =
    include_str!("../fixtures/render/vector-style-dark.json");
/// The OpenMapTiles light artifact: the layer and attribute vocabulary of the
/// OpenMapTiles schema.
const OPENMAPTILES_LIGHT_STYLE_ARTIFACT_JSON: &str =
    include_str!("../fixtures/render/vector-style-openmaptiles-light.json");
/// The OpenMapTiles dark artifact.
const OPENMAPTILES_DARK_STYLE_ARTIFACT_JSON: &str =
    include_str!("../fixtures/render/vector-style-openmaptiles-dark.json");
/// The Bright light artifact: the Bright palette curated to the overlay
/// pipeline's fill, line and circle kinds.
const BRIGHT_LIGHT_STYLE_ARTIFACT_JSON: &str =
    include_str!("../fixtures/render/vector-style-bright-light.json");
/// The Fiord dark artifact, curated to the same overlay kinds.
const FIORD_DARK_STYLE_ARTIFACT_JSON: &str =
    include_str!("../fixtures/render/vector-style-fiord-dark.json");
/// The Positron light artifact, curated to the same overlay kinds.
const POSITRON_LIGHT_STYLE_ARTIFACT_JSON: &str =
    include_str!("../fixtures/render/vector-style-positron-light.json");

/// The style artifacts a style id can name; `None` names no built-in style.
fn vector_style_artifact_json(style_id: &str) -> Option<&'static str> {
    match style_id {
        VECTOR_STYLE_ID_CARTO_LIGHT => Some(DEFAULT_VECTOR_STYLE_ARTIFACT_JSON),
        VECTOR_STYLE_ID_CARTO_DARK => Some(CARTO_DARK_STYLE_ARTIFACT_JSON),
        VECTOR_STYLE_ID_OPENMAPTILES_LIGHT => Some(OPENMAPTILES_LIGHT_STYLE_ARTIFACT_JSON),
        VECTOR_STYLE_ID_OPENMAPTILES_DARK => Some(OPENMAPTILES_DARK_STYLE_ARTIFACT_JSON),
        VECTOR_STYLE_ID_BRIGHT_LIGHT => Some(BRIGHT_LIGHT_STYLE_ARTIFACT_JSON),
        VECTOR_STYLE_ID_FIORD_DARK => Some(FIORD_DARK_STYLE_ARTIFACT_JSON),
        VECTOR_STYLE_ID_POSITRON_LIGHT => Some(POSITRON_LIGHT_STYLE_ARTIFACT_JSON),
        _ => None,
    }
}

/// Parses the built-in style artifact a style id names.
///
/// # Parameters
/// - `style_id` — one of the exported `VECTOR_STYLE_ID_*` identifiers.
///
/// # Returns
/// The parsed style.
///
/// # Errors
/// An unknown id and a malformed artifact both report the typed artifact
/// error rather than a guess; only the default style has an in-code fallback.
pub fn vector_tile_style_from_id(
    style_id: &str,
) -> Result<VectorTileStyle, VectorStyleArtifactError> {
    let Some(artifact_json) = vector_style_artifact_json(style_id) else {
        return Err(VectorStyleArtifactError {
            detail: format!(
                "no committed vector style carries the id {style_id:?}; known ids are \
                 {VECTOR_STYLE_ID_CARTO_LIGHT}, {VECTOR_STYLE_ID_CARTO_DARK}, \
                 {VECTOR_STYLE_ID_OPENMAPTILES_LIGHT}, {VECTOR_STYLE_ID_OPENMAPTILES_DARK}, \
                 {VECTOR_STYLE_ID_BRIGHT_LIGHT}, {VECTOR_STYLE_ID_FIORD_DARK}, \
                 {VECTOR_STYLE_ID_POSITRON_LIGHT}"
            ),
        });
    };
    vector_tile_style_from_json(artifact_json)
}

/// One filtered area fill of the fallback style: the source layer, the exact
/// attribute filter and the fill paint. Every entry mirrors one artifact
/// layer entry, in the artifact's order.
struct FallbackAreaFill {
    source_layer: &'static str,
    attribute_name: &'static str,
    expected_text: &'static str,
    rgba_color: [f32; 4],
}

/// One fallback road fill class: the highway class, its paint, the zoom it
/// first paints at and its width stops. The committed Carto artifact carries
/// exactly these values, so the parsed artifact and the in-code fallback stay
/// interchangeable.
struct FallbackRoadFillStyle {
    highway_class: &'static str,
    rgba_color: [f32; 4],
    minimum_zoom_level: f64,
    width_zoom_stops: &'static [(f64, f64)],
}

/// One fallback road casing class, painted under the fills.
struct FallbackRoadCasingStyle {
    highway_class: &'static str,
    minimum_zoom_level: f64,
    width_zoom_stops: &'static [(f64, f64)],
}

/// The fallback's area fills: land use, natural ground and leisure, each
/// filtered on the class attribute its source layer carries.
const FALLBACK_AREA_FILLS: &[FallbackAreaFill] = &[
    FallbackAreaFill {
        source_layer: "landuse",
        attribute_name: "landuse",
        expected_text: "residential",
        rgba_color: [0.929412, 0.917647, 0.894118, 1.0],
    },
    FallbackAreaFill {
        source_layer: "landuse",
        attribute_name: "landuse",
        expected_text: "commercial",
        rgba_color: [0.949020, 0.913725, 0.905882, 1.0],
    },
    FallbackAreaFill {
        source_layer: "landuse",
        attribute_name: "landuse",
        expected_text: "industrial",
        rgba_color: [0.921569, 0.909804, 0.886275, 1.0],
    },
    FallbackAreaFill {
        source_layer: "landuse",
        attribute_name: "landuse",
        expected_text: "brownfield",
        rgba_color: [0.905882, 0.890196, 0.850980, 1.0],
    },
    FallbackAreaFill {
        source_layer: "landuse",
        attribute_name: "landuse",
        expected_text: "greenfield",
        rgba_color: [0.905882, 0.890196, 0.850980, 1.0],
    },
    FallbackAreaFill {
        source_layer: "landuse",
        attribute_name: "landuse",
        expected_text: "construction",
        rgba_color: [0.905882, 0.890196, 0.850980, 1.0],
    },
    FallbackAreaFill {
        source_layer: "landuse",
        attribute_name: "landuse",
        expected_text: "farmland",
        rgba_color: [0.913725, 0.941176, 0.847059, 1.0],
    },
    FallbackAreaFill {
        source_layer: "landuse",
        attribute_name: "landuse",
        expected_text: "meadow",
        rgba_color: [0.894118, 0.933333, 0.831373, 1.0],
    },
    FallbackAreaFill {
        source_layer: "landuse",
        attribute_name: "landuse",
        expected_text: "grass",
        rgba_color: [0.894118, 0.933333, 0.831373, 1.0],
    },
    FallbackAreaFill {
        source_layer: "landuse",
        attribute_name: "landuse",
        expected_text: "forest",
        rgba_color: [0.843137, 0.901961, 0.803922, 1.0],
    },
    FallbackAreaFill {
        source_layer: "landuse",
        attribute_name: "landuse",
        expected_text: "cemetery",
        rgba_color: [0.878431, 0.913725, 0.847059, 1.0],
    },
    FallbackAreaFill {
        source_layer: "landuse",
        attribute_name: "landuse",
        expected_text: "religious",
        rgba_color: [0.913725, 0.901961, 0.878431, 1.0],
    },
    FallbackAreaFill {
        source_layer: "natural",
        attribute_name: "natural",
        expected_text: "wood",
        rgba_color: [0.843137, 0.901961, 0.803922, 1.0],
    },
    FallbackAreaFill {
        source_layer: "natural",
        attribute_name: "natural",
        expected_text: "scrub",
        rgba_color: [0.882353, 0.925490, 0.843137, 1.0],
    },
    FallbackAreaFill {
        source_layer: "natural",
        attribute_name: "natural",
        expected_text: "heath",
        rgba_color: [0.894118, 0.933333, 0.839216, 1.0],
    },
    FallbackAreaFill {
        source_layer: "natural",
        attribute_name: "natural",
        expected_text: "sand",
        rgba_color: [0.945098, 0.921569, 0.843137, 1.0],
    },
    FallbackAreaFill {
        source_layer: "natural",
        attribute_name: "natural",
        expected_text: "wetland",
        rgba_color: [0.847059, 0.909804, 0.890196, 1.0],
    },
    FallbackAreaFill {
        source_layer: "leisure",
        attribute_name: "leisure",
        expected_text: "park",
        rgba_color: [0.850980, 0.921569, 0.815686, 1.0],
    },
    FallbackAreaFill {
        source_layer: "leisure",
        attribute_name: "leisure",
        expected_text: "garden",
        rgba_color: [0.850980, 0.921569, 0.815686, 1.0],
    },
    FallbackAreaFill {
        source_layer: "leisure",
        attribute_name: "leisure",
        expected_text: "pitch",
        rgba_color: [0.811765, 0.901961, 0.756863, 1.0],
    },
    FallbackAreaFill {
        source_layer: "leisure",
        attribute_name: "leisure",
        expected_text: "golf_course",
        rgba_color: [0.850980, 0.913725, 0.803922, 1.0],
    },
    FallbackAreaFill {
        source_layer: "leisure",
        attribute_name: "leisure",
        expected_text: "stadium",
        rgba_color: [0.882353, 0.894118, 0.909804, 1.0],
    },
    FallbackAreaFill {
        source_layer: "leisure",
        attribute_name: "leisure",
        expected_text: "marina",
        rgba_color: [0.827451, 0.894118, 0.925490, 1.0],
    },
    FallbackAreaFill {
        source_layer: "leisure",
        attribute_name: "leisure",
        expected_text: "swimming_pool",
        rgba_color: [0.654902, 0.803922, 0.890196, 1.0],
    },
];

/// Fill paint of the fallback water layer, mirroring the artifact's entry.
const FALLBACK_WATER_FILL_RGBA: [f32; 4] = [0.654902, 0.803922, 0.890196, 1.0];
/// Line width of the fallback waterway layer, in screen pixels.
const FALLBACK_WATERWAY_LINE_WIDTH_PIXELS: f64 = 1.8;
/// Fill paint of the fallback building layer, mirroring the artifact.
const FALLBACK_BUILDING_FILL_RGBA: [f32; 4] = [0.870588, 0.843137, 0.807843, 1.0];
/// Line paint of the fallback boundary layer, mirroring the artifact.
const FALLBACK_BOUNDARY_LINE_RGBA: [f32; 4] = [0.752941, 0.721569, 0.788235, 1.0];
/// Line width of the fallback boundary layer, in screen pixels.
const FALLBACK_BOUNDARY_LINE_WIDTH_PIXELS: f64 = 1.1;
/// Line paint of the fallback railway layer, mirroring the artifact.
const FALLBACK_RAILWAY_LINE_RGBA: [f32; 4] = [0.776471, 0.752941, 0.729412, 1.0];
/// Line width of the fallback railway layer, in screen pixels.
const FALLBACK_RAILWAY_LINE_WIDTH_PIXELS: f64 = 1.2;
/// Circle paint of the fallback place layer, mirroring the artifact's entry.
const FALLBACK_PLACE_CIRCLE_RGBA: [f32; 4] = [0.490196, 0.509804, 0.545098, 0.6];
/// Marker radius of the fallback place layer, in screen pixels.
const FALLBACK_PLACE_CIRCLE_RADIUS_PIXELS: f64 = 1.6;
/// Text paint of the fallback places symbol layer, mirroring the artifact.
const FALLBACK_PLACES_SYMBOL_TEXT_RGBA: [f32; 4] = [0.301961, 0.317647, 0.341176, 1.0];
/// Text size of the fallback places symbol layer, in screen pixels.
const FALLBACK_PLACES_SYMBOL_TEXT_SIZE_PIXELS: f64 = 12.0;
/// Halo paint of the fallback places symbol layer, mirroring the artifact.
const FALLBACK_PLACES_SYMBOL_HALO_RGBA: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
/// Halo width of the fallback places symbol layer, in screen pixels.
const FALLBACK_PLACES_SYMBOL_HALO_WIDTH_PIXELS: f64 = 1.3;
/// Lowest zoom of the fallback places symbol layer (city names appear early).
const FALLBACK_PLACES_SYMBOL_MINIMUM_ZOOM_LEVEL: f64 = 6.0;
/// Highest zoom of the fallback places symbol layer.
const FALLBACK_PLACES_SYMBOL_MAXIMUM_ZOOM_LEVEL: f64 = 24.0;
/// Priority rank of the fallback places symbol layer.
const FALLBACK_PLACES_SYMBOL_PRIORITY: u32 = 1;

/// Highest zoom of a fallback road-label layer.
const FALLBACK_ROAD_LABEL_MAXIMUM_ZOOM_LEVEL: f64 = 24.0;

/// One fallback road-label class: the highway class it labels, the zoom it
/// appears at, its text size and stops, and its collision priority. The
/// artifact's road labels use exactly these values, so the parsed artifact and
/// the in-code fallback stay interchangeable.
struct FallbackRoadLabelTier {
    highway_class: &'static str,
    minimum_zoom_level: f64,
    text_size_pixels: f64,
    text_size_zoom_stops: &'static [(f64, f64)],
    priority: u32,
}

/// Text size of a fallback route shield, in screen pixels (its size stops
/// carry the zoom progression).
const FALLBACK_SHIELD_TEXT_SIZE_PIXELS: f64 = 11.0;
/// Lowest zoom of a fallback route shield.
const FALLBACK_SHIELD_MINIMUM_ZOOM_LEVEL: f64 = 5.0;
/// Highest zoom of a fallback route shield (road names take over above it).
const FALLBACK_SHIELD_MAXIMUM_ZOOM_LEVEL: f64 = 12.0;
/// Collision priority of a fallback route shield: draw before place labels.
const FALLBACK_SHIELD_PRIORITY: u32 = 0;

/// The fallback's road-label classes, most important first.
const FALLBACK_ROAD_LABEL_TIERS: &[FallbackRoadLabelTier] = &[
    FallbackRoadLabelTier {
        highway_class: "motorway",
        minimum_zoom_level: 10.0,
        text_size_pixels: 14.0,
        text_size_zoom_stops: &[(12.0, 12.0), (16.0, 16.0), (18.0, 18.0)],
        priority: 2,
    },
    FallbackRoadLabelTier {
        highway_class: "trunk",
        minimum_zoom_level: 11.0,
        text_size_pixels: 13.0,
        text_size_zoom_stops: &[(12.0, 11.0), (16.0, 15.0), (18.0, 17.0)],
        priority: 2,
    },
    FallbackRoadLabelTier {
        highway_class: "primary",
        minimum_zoom_level: 12.0,
        text_size_pixels: 12.0,
        text_size_zoom_stops: &[(12.0, 10.0), (16.0, 14.0), (18.0, 16.0)],
        priority: 2,
    },
    FallbackRoadLabelTier {
        highway_class: "secondary",
        minimum_zoom_level: 13.0,
        text_size_pixels: 11.0,
        text_size_zoom_stops: &[(12.0, 9.0), (16.0, 13.0), (18.0, 15.0)],
        priority: 2,
    },
    FallbackRoadLabelTier {
        highway_class: "tertiary",
        minimum_zoom_level: 14.0,
        text_size_pixels: 10.0,
        text_size_zoom_stops: &[(12.0, 8.0), (16.0, 12.0), (18.0, 13.0)],
        priority: 2,
    },
    FallbackRoadLabelTier {
        highway_class: "residential",
        minimum_zoom_level: 15.0,
        text_size_pixels: 9.0,
        text_size_zoom_stops: &[(12.0, 8.0), (16.0, 11.0), (18.0, 12.0)],
        priority: 3,
    },
    FallbackRoadLabelTier {
        highway_class: "unclassified",
        minimum_zoom_level: 15.0,
        text_size_pixels: 9.0,
        text_size_zoom_stops: &[(12.0, 8.0), (16.0, 11.0), (18.0, 12.0)],
        priority: 3,
    },
];

/// The fallback canvas background: OSM Carto's light paper colour, so a
/// styled base that hides the raster layer has the style's own surface rather
/// than the clear colour.
const FALLBACK_CANVAS_BACKGROUND_RGBA: [f32; 4] = [0.94902, 0.937255, 0.913725, 1.0];

/// The casing paint of the fallback's motorway and trunk tiers.
const FALLBACK_ROAD_CASING_RGBA: [f32; 4] = [0.831373, 0.603922, 0.333333, 1.0];

/// The fallback's motorway and trunk casings, painted under every fill.
const FALLBACK_ROAD_CASINGS: &[FallbackRoadCasingStyle] = &[
    FallbackRoadCasingStyle {
        highway_class: "motorway",
        minimum_zoom_level: 4.0,
        width_zoom_stops: &[
            (4.0, 1.4),
            (6.0, 1.9),
            (8.0, 2.7),
            (10.0, 3.7),
            (12.0, 4.9),
            (14.0, 6.3),
            (16.0, 7.7),
            (18.0, 9.2),
        ],
    },
    FallbackRoadCasingStyle {
        highway_class: "motorway_link",
        minimum_zoom_level: 4.0,
        width_zoom_stops: &[
            (4.0, 1.1),
            (6.0, 1.5),
            (8.0, 2.2),
            (10.0, 3.0),
            (12.0, 4.0),
            (14.0, 5.1),
            (16.0, 6.3),
            (18.0, 7.5),
        ],
    },
    FallbackRoadCasingStyle {
        highway_class: "trunk",
        minimum_zoom_level: 4.0,
        width_zoom_stops: &[
            (4.0, 1.2),
            (6.0, 1.7),
            (8.0, 2.4),
            (10.0, 3.3),
            (12.0, 4.3),
            (14.0, 5.5),
            (16.0, 6.7),
            (18.0, 8.0),
        ],
    },
    FallbackRoadCasingStyle {
        highway_class: "trunk_link",
        minimum_zoom_level: 4.0,
        width_zoom_stops: &[
            (4.0, 1.0),
            (6.0, 1.4),
            (8.0, 2.0),
            (10.0, 2.7),
            (12.0, 3.5),
            (14.0, 4.5),
            (16.0, 5.5),
            (18.0, 6.5),
        ],
    },
];

/// The fallback's road fills, in the artifact's class order. Widths thin with
/// zoom so a regional frame draws a hierarchy (a hairline primary, a ~2 px
/// motorway) instead of every class at a street-width constant.
const FALLBACK_ROAD_FILLS: &[FallbackRoadFillStyle] = &[
    FallbackRoadFillStyle {
        highway_class: "motorway",
        rgba_color: [0.964706, 0.662745, 0.309804, 1.0],
        minimum_zoom_level: 4.0,
        width_zoom_stops: &[
            (4.0, 0.9),
            (6.0, 1.3),
            (8.0, 2.0),
            (10.0, 2.9),
            (12.0, 3.9),
            (14.0, 5.1),
            (16.0, 6.3),
            (18.0, 7.6),
        ],
    },
    FallbackRoadFillStyle {
        highway_class: "motorway_link",
        rgba_color: [0.964706, 0.662745, 0.309804, 1.0],
        minimum_zoom_level: 4.0,
        width_zoom_stops: &[
            (4.0, 0.7),
            (6.0, 1.0),
            (8.0, 1.6),
            (10.0, 2.3),
            (12.0, 3.1),
            (14.0, 4.1),
            (16.0, 5.0),
            (18.0, 6.1),
        ],
    },
    FallbackRoadFillStyle {
        highway_class: "trunk",
        rgba_color: [0.964706, 0.662745, 0.309804, 1.0],
        minimum_zoom_level: 4.0,
        width_zoom_stops: &[
            (4.0, 0.8),
            (6.0, 1.1),
            (8.0, 1.7),
            (10.0, 2.5),
            (12.0, 3.3),
            (14.0, 4.3),
            (16.0, 5.3),
            (18.0, 6.3),
        ],
    },
    FallbackRoadFillStyle {
        highway_class: "trunk_link",
        rgba_color: [0.964706, 0.662745, 0.309804, 1.0],
        minimum_zoom_level: 4.0,
        width_zoom_stops: &[
            (4.0, 0.6),
            (6.0, 0.9),
            (8.0, 1.4),
            (10.0, 2.0),
            (12.0, 2.6),
            (14.0, 3.4),
            (16.0, 4.2),
            (18.0, 5.0),
        ],
    },
    FallbackRoadFillStyle {
        highway_class: "primary",
        rgba_color: [0.976471, 0.768627, 0.415686, 1.0],
        minimum_zoom_level: 7.0,
        width_zoom_stops: &[
            (7.0, 0.7),
            (9.0, 1.0),
            (11.0, 1.5),
            (14.0, 2.6),
            (16.0, 3.5),
            (18.0, 4.4),
        ],
    },
    FallbackRoadFillStyle {
        highway_class: "primary_link",
        rgba_color: [0.976471, 0.768627, 0.415686, 1.0],
        minimum_zoom_level: 7.0,
        width_zoom_stops: &[
            (7.0, 0.6),
            (9.0, 0.8),
            (11.0, 1.2),
            (14.0, 2.1),
            (16.0, 2.8),
            (18.0, 3.5),
        ],
    },
    FallbackRoadFillStyle {
        highway_class: "secondary",
        rgba_color: [0.984314, 0.878431, 0.690196, 1.0],
        minimum_zoom_level: 9.0,
        width_zoom_stops: &[
            (9.0, 0.6),
            (11.0, 0.9),
            (13.0, 1.3),
            (16.0, 2.2),
            (18.0, 2.9),
        ],
    },
    FallbackRoadFillStyle {
        highway_class: "secondary_link",
        rgba_color: [0.984314, 0.878431, 0.690196, 1.0],
        minimum_zoom_level: 9.0,
        width_zoom_stops: &[
            (9.0, 0.5),
            (11.0, 0.7),
            (13.0, 1.0),
            (16.0, 1.8),
            (18.0, 2.3),
        ],
    },
    FallbackRoadFillStyle {
        highway_class: "tertiary",
        rgba_color: [1.0, 1.0, 1.0, 1.0],
        minimum_zoom_level: 11.0,
        width_zoom_stops: &[(11.0, 0.5), (13.0, 0.8), (16.0, 1.5), (18.0, 2.2)],
    },
    FallbackRoadFillStyle {
        highway_class: "tertiary_link",
        rgba_color: [1.0, 1.0, 1.0, 1.0],
        minimum_zoom_level: 11.0,
        width_zoom_stops: &[(11.0, 0.4), (13.0, 0.6), (16.0, 1.2), (18.0, 1.8)],
    },
    FallbackRoadFillStyle {
        highway_class: "residential",
        rgba_color: [1.0, 1.0, 1.0, 1.0],
        minimum_zoom_level: 12.0,
        width_zoom_stops: &[(12.0, 0.4), (14.0, 0.7), (16.0, 1.2), (18.0, 1.8)],
    },
    FallbackRoadFillStyle {
        highway_class: "unclassified",
        rgba_color: [1.0, 1.0, 1.0, 1.0],
        minimum_zoom_level: 12.0,
        width_zoom_stops: &[(12.0, 0.4), (14.0, 0.7), (16.0, 1.2), (18.0, 1.8)],
    },
    FallbackRoadFillStyle {
        highway_class: "living_street",
        rgba_color: [1.0, 1.0, 1.0, 1.0],
        minimum_zoom_level: 12.0,
        width_zoom_stops: &[(12.0, 0.4), (14.0, 0.7), (16.0, 1.2), (18.0, 1.8)],
    },
    FallbackRoadFillStyle {
        highway_class: "service",
        rgba_color: [0.984314, 0.972549, 0.952941, 1.0],
        minimum_zoom_level: 15.0,
        width_zoom_stops: &[(15.0, 0.4), (18.0, 0.9)],
    },
    FallbackRoadFillStyle {
        highway_class: "pedestrian",
        rgba_color: [0.968627, 0.94902, 0.917647, 1.0],
        minimum_zoom_level: 14.0,
        width_zoom_stops: &[(14.0, 0.5), (18.0, 1.2)],
    },
];

// Walking and cycling paths are deliberately absent: this style carries no
// zoom-dependent width or visibility, and at the demo's zoom a sidewalk
// network would paint over the street grid. A symbol/zoom-function style
// slice is the documented later seam.

/// Why a style artifact could not be parsed into a [`VectorTileStyle`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorStyleArtifactError {
    /// The underlying parser detail (a malformed artifact reports the JSON
    /// error and its location).
    pub detail: String,
}

impl std::fmt::Display for VectorStyleArtifactError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "vector style artifact invalid: {}", self.detail)
    }
}

impl std::error::Error for VectorStyleArtifactError {}

/// Parses a data-only style artifact into a [`VectorTileStyle`].
///
/// The artifact names no providers and carries no URLs: a layer entry has a
/// `source_layer`, a `geometry_kind` (`fill`, `line`, `circle` or `symbol`),
/// the kind's paint, an optional one-attribute `filter`, and the kind's
/// `width_pixels` or `radius_pixels` (the documented defaults apply when
/// omitted). A symbol entry declares its label style — `text_field_fallbacks`,
/// `font_stack`, `text_rgba`, `text_size_pixels`, `text_size_zoom_stops`,
/// `halo_rgba`, `halo_width_pixels`, `placement`, `priority`, `allow_overlap`,
/// `rank_zoom_stops` and `class_minimum_zooms` — and paints no overlay
/// primitive of its own; the label pass draws its text.
pub fn vector_tile_style_from_json(
    artifact_json: &str,
) -> Result<VectorTileStyle, VectorStyleArtifactError> {
    let artifact: VectorStyleArtifact =
        serde_json::from_str(artifact_json).map_err(|parse_error| VectorStyleArtifactError {
            detail: parse_error.to_string(),
        })?;
    Ok(VectorTileStyle {
        background: artifact.background,
        layers: artifact
            .layers
            .into_iter()
            .map(VectorStyleArtifactLayer::into_layer_style)
            .collect(),
    })
}

/// The style constructed in code, the documented fallback when the artifact
/// is missing or malformed. It mirrors
/// [`DEFAULT_VECTOR_STYLE_ARTIFACT_JSON`] so the two paths paint the same
/// demo, which a test pins.
pub fn in_code_default_vector_tile_style() -> VectorTileStyle {
    let mut layers = Vec::new();
    // Ground cover first: land use, natural areas and leisure surfaces paint
    // under everything else, in the artifact's filtered-fill order.
    for area_fill in FALLBACK_AREA_FILLS {
        layers.push(
            VectorLayerStyle::fill(area_fill.source_layer, area_fill.rgba_color)
                .with_filter(area_fill.attribute_name, area_fill.expected_text),
        );
    }
    layers.push(VectorLayerStyle::fill("water", FALLBACK_WATER_FILL_RGBA));
    layers.push(VectorLayerStyle::line(
        "waterways",
        FALLBACK_WATER_FILL_RGBA,
        FALLBACK_WATERWAY_LINE_WIDTH_PIXELS,
    ));
    layers.push(VectorLayerStyle::fill(
        "buildings",
        FALLBACK_BUILDING_FILL_RGBA,
    ));
    layers.push(VectorLayerStyle::line(
        "railways",
        FALLBACK_RAILWAY_LINE_RGBA,
        FALLBACK_RAILWAY_LINE_WIDTH_PIXELS,
    ));
    // Road casings paint as one block under every fill, so intersecting
    // roads read as continuous ribbons instead of cased-on-cased seams.
    for casing in FALLBACK_ROAD_CASINGS {
        let mut road_casing = VectorLayerStyle::line(
            "highways",
            FALLBACK_ROAD_CASING_RGBA,
            casing.width_zoom_stops.first().map_or(0.0, |stop| stop.1),
        )
        .with_filter("highway", casing.highway_class)
        .with_zoom_span(
            casing.minimum_zoom_level,
            DEFAULT_VECTOR_STYLE_MAXIMUM_ZOOM_LEVEL,
        );
        for (zoom_level, width_pixels) in casing.width_zoom_stops {
            road_casing = road_casing.with_width_zoom_stop(*zoom_level, *width_pixels);
        }
        layers.push(road_casing);
    }
    for fill in FALLBACK_ROAD_FILLS {
        let mut road_fill = VectorLayerStyle::line(
            "highways",
            fill.rgba_color,
            fill.width_zoom_stops.first().map_or(0.0, |stop| stop.1),
        )
        .with_filter("highway", fill.highway_class)
        .with_zoom_span(
            fill.minimum_zoom_level,
            DEFAULT_VECTOR_STYLE_MAXIMUM_ZOOM_LEVEL,
        );
        for (zoom_level, width_pixels) in fill.width_zoom_stops {
            road_fill = road_fill.with_width_zoom_stop(*zoom_level, *width_pixels);
        }
        layers.push(road_fill);
    }
    layers.push(VectorLayerStyle::line(
        "boundaries",
        FALLBACK_BOUNDARY_LINE_RGBA,
        FALLBACK_BOUNDARY_LINE_WIDTH_PIXELS,
    ));
    layers.push(VectorLayerStyle::circle(
        "places",
        FALLBACK_PLACE_CIRCLE_RGBA,
        FALLBACK_PLACE_CIRCLE_RADIUS_PIXELS,
    ));
    layers.push(
        VectorLayerStyle::symbol(
            "places",
            FALLBACK_PLACES_SYMBOL_TEXT_RGBA,
            FALLBACK_PLACES_SYMBOL_TEXT_SIZE_PIXELS,
        )
        .with_text_size_zoom_stop(8.0, 10.0)
        .with_text_size_zoom_stop(14.0, 14.0)
        .with_text_size_zoom_stop(18.0, 18.0)
        .with_halo(
            FALLBACK_PLACES_SYMBOL_HALO_RGBA,
            FALLBACK_PLACES_SYMBOL_HALO_WIDTH_PIXELS,
        )
        .with_zoom_span(
            FALLBACK_PLACES_SYMBOL_MINIMUM_ZOOM_LEVEL,
            FALLBACK_PLACES_SYMBOL_MAXIMUM_ZOOM_LEVEL,
        )
        .with_placement(VectorSymbolPlacement::Point)
        .with_priority(FALLBACK_PLACES_SYMBOL_PRIORITY),
    );
    // Road names paint last, over every line, along the road's own geometry.
    for road_label_tier in FALLBACK_ROAD_LABEL_TIERS {
        let mut road_label = VectorLayerStyle::symbol(
            "highways",
            FALLBACK_PLACES_SYMBOL_TEXT_RGBA,
            road_label_tier.text_size_pixels,
        );
        for (zoom_level, size_pixels) in road_label_tier.text_size_zoom_stops {
            road_label = road_label.with_text_size_zoom_stop(*zoom_level, *size_pixels);
        }
        layers.push(
            road_label
                .with_halo(
                    FALLBACK_PLACES_SYMBOL_HALO_RGBA,
                    FALLBACK_PLACES_SYMBOL_HALO_WIDTH_PIXELS,
                )
                .with_zoom_span(
                    road_label_tier.minimum_zoom_level,
                    FALLBACK_ROAD_LABEL_MAXIMUM_ZOOM_LEVEL,
                )
                .with_placement(VectorSymbolPlacement::Line)
                .with_priority(road_label_tier.priority)
                .with_filter("highway", road_label_tier.highway_class),
        );
    }
    // Route shields: the recorded route relations carry road and transit refs
    // at regional zooms, so the thin road lines are still identifiable. The
    // reference reads the `routes` layer and draws no halo of its own.
    layers.push(
        VectorLayerStyle::symbol(
            "routes",
            FALLBACK_PLACES_SYMBOL_TEXT_RGBA,
            FALLBACK_SHIELD_TEXT_SIZE_PIXELS,
        )
        .with_text_field_fallbacks(&["ref"])
        .with_halo([1.0, 1.0, 1.0, 0.0], 0.0)
        .with_zoom_span(
            FALLBACK_SHIELD_MINIMUM_ZOOM_LEVEL,
            FALLBACK_SHIELD_MAXIMUM_ZOOM_LEVEL,
        )
        .with_placement(VectorSymbolPlacement::Line)
        .with_priority(FALLBACK_SHIELD_PRIORITY),
    );
    VectorTileStyle {
        background: Some(FALLBACK_CANVAS_BACKGROUND_RGBA),
        layers,
    }
}

/// The style to paint with: the artifact when it parses, otherwise the
/// in-code fallback via [`in_code_default_vector_tile_style`].
pub fn vector_tile_style_or_fallback(artifact_json: &str) -> VectorTileStyle {
    vector_tile_style_from_json(artifact_json)
        .unwrap_or_else(|_| in_code_default_vector_tile_style())
}

/// The default style: the built-in artifact parsed by
/// [`vector_tile_style_or_fallback`], so a missing artifact degrades to the
/// in-code defaults instead of failing the render.
pub fn default_vector_tile_style() -> VectorTileStyle {
    vector_tile_style_or_fallback(DEFAULT_VECTOR_STYLE_ARTIFACT_JSON)
}

/// The artifact's parsed shape; every field is data, never a code path.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VectorStyleArtifact {
    /// The style's canvas background, or `None` when the style paints its own
    /// full ground fill.
    #[serde(default)]
    background: Option<[f32; 4]>,
    /// The artifact's layer entries, applied in order.
    layers: Vec<VectorStyleArtifactLayer>,
}

/// One artifact layer entry, in the artifact's kind-specific paint form.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VectorStyleArtifactLayer {
    /// The decoded layer name the style paints.
    source_layer: String,
    /// The geometry kind the style paints.
    geometry_kind: VectorStyleArtifactGeometryKind,
    /// The layer's RGBA paint (fill, line and circle layers; the documented
    /// default when omitted). A symbol layer paints `text_rgba` instead.
    #[serde(default)]
    rgba: Option<[f32; 4]>,
    /// An optional one-attribute text filter.
    #[serde(default)]
    filter: Option<VectorStyleArtifactFilter>,
    /// Line width in screen pixels (line layers; the documented default when
    /// omitted).
    #[serde(default)]
    width_pixels: Option<f64>,
    /// Marker radius in screen pixels (circle layers; the documented default
    /// when omitted).
    #[serde(default)]
    radius_pixels: Option<f64>,
    /// The lowest zoom level the layer paints at.
    #[serde(default)]
    min_zoom: Option<f64>,
    /// The highest zoom level the layer paints at.
    #[serde(default)]
    max_zoom: Option<f64>,
    /// Width stops for line layers: `{zoom, width_pixels}` entries evaluated
    /// at the tile's zoom (linear interpolation between stops).
    #[serde(default)]
    width_zoom_stops: Vec<VectorStyleArtifactWidthStop>,
    /// Ordered text-attribute fallbacks of a symbol layer.
    #[serde(default)]
    text_field_fallbacks: Option<Vec<String>>,
    /// The font family stack of a symbol layer, most preferred first.
    #[serde(default)]
    font_stack: Option<Vec<String>>,
    /// Text paint of a symbol layer (the documented default when omitted).
    #[serde(default)]
    text_rgba: Option<[f32; 4]>,
    /// Text size in screen pixels of a symbol layer (the documented default
    /// when omitted).
    #[serde(default)]
    text_size_pixels: Option<f64>,
    /// Text-size stops of a symbol layer: `{zoom, size_pixels}` entries
    /// evaluated at the tile's zoom (linear interpolation between stops).
    #[serde(default)]
    text_size_zoom_stops: Vec<VectorStyleArtifactTextSizeStop>,
    /// Halo paint behind a symbol layer's text (the documented default when
    /// omitted).
    #[serde(default)]
    halo_rgba: Option<[f32; 4]>,
    /// Halo width in screen pixels of a symbol layer (the documented default
    /// when omitted).
    #[serde(default)]
    halo_width_pixels: Option<f64>,
    /// Where a symbol layer anchors its labels.
    #[serde(default)]
    placement: Option<VectorStyleArtifactSymbolPlacement>,
    /// Priority rank of a symbol layer (lower ranks draw first).
    #[serde(default)]
    priority: Option<u32>,
    /// Whether a symbol layer may draw overlapping labels.
    #[serde(default)]
    allow_overlap: Option<bool>,
    /// Feature-rank ceiling stops of a symbol layer (`{zoom, maximum_rank}`
    /// entries, evaluated as steps at the layer's zoom).
    #[serde(default)]
    rank_zoom_stops: Vec<VectorStyleArtifactRankStop>,
    /// Per-class minimum zoom levels of a symbol layer: `{classes, min_zoom}`
    /// groups that hold a class back until the map is closer.
    #[serde(default)]
    class_minimum_zooms: Vec<VectorStyleArtifactClassMinimumZoom>,
    /// The route-reference badge the artifact declares for a symbol layer.
    /// The legacy label model predates badges and keeps the parsed value only
    /// so the data stays forward-compatible; the compiled style renders it.
    #[serde(default)]
    shield: Option<serde_json::Value>,
}

/// One artifact width stop of a line layer.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VectorStyleArtifactWidthStop {
    /// The zoom level the stop applies from.
    zoom: f64,
    /// The line width in screen pixels at that zoom level.
    width_pixels: f64,
}

/// One artifact text-size stop of a symbol layer.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VectorStyleArtifactTextSizeStop {
    /// The zoom level the stop applies from.
    zoom: f64,
    /// The text size in screen pixels at that zoom level.
    size_pixels: f64,
}

/// One artifact feature-rank ceiling stop of a symbol layer.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VectorStyleArtifactRankStop {
    /// The zoom level the stop applies from.
    zoom: f64,
    /// The highest feature rank the layer draws at that zoom level.
    maximum_rank: u32,
}

/// One artifact group of feature classes held back until a minimum zoom.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VectorStyleArtifactClassMinimumZoom {
    /// The feature `class` attribute values the gate applies to.
    classes: Vec<String>,
    /// The lowest zoom level at which those classes draw.
    min_zoom: f64,
}

/// The artifact's geometry-kind spelling (`fill`, `line`, `circle`, `symbol`).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum VectorStyleArtifactGeometryKind {
    /// Polygon features, painted as holes-aware filled rings.
    Fill,
    /// LineString features, painted as thickened polylines.
    Line,
    /// Point features, painted as screen-facing markers.
    Circle,
    /// A declarative label layer; it paints no overlay primitive yet.
    Symbol,
}

/// The artifact's symbol-placement spelling (`point`, `line`).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum VectorStyleArtifactSymbolPlacement {
    /// Labels anchor at the feature's point.
    Point,
    /// Labels run along a line feature.
    Line,
}

/// The artifact's one-attribute text filter.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VectorStyleArtifactFilter {
    /// The attribute key to match.
    attribute_name: String,
    /// The exact text value to match (empty when the filter only uses the
    /// other alternatives).
    #[serde(default)]
    expected_text: String,
    /// Any one of these text values also matches (empty when unused).
    #[serde(default)]
    matches_any: Vec<String>,
    /// A numeric value the attribute may carry (`admin_level: 2`).
    #[serde(default)]
    number_equals: Option<i64>,
}

impl VectorStyleArtifactLayer {
    /// Builds the renderer's layer style from the artifact entry.
    fn into_layer_style(self) -> VectorLayerStyle {
        let paint_rgba = self.rgba.unwrap_or(DEFAULT_VECTOR_STYLE_PAINT_RGBA);
        let mut layer_style = match self.geometry_kind {
            VectorStyleArtifactGeometryKind::Fill => {
                VectorLayerStyle::fill(self.source_layer, paint_rgba)
            }
            VectorStyleArtifactGeometryKind::Line => VectorLayerStyle::line(
                self.source_layer,
                paint_rgba,
                self.width_pixels
                    .unwrap_or(DEFAULT_VECTOR_LINE_WIDTH_PIXELS),
            ),
            VectorStyleArtifactGeometryKind::Circle => VectorLayerStyle::circle(
                self.source_layer,
                paint_rgba,
                self.radius_pixels
                    .unwrap_or(DEFAULT_VECTOR_CIRCLE_RADIUS_PIXELS),
            ),
            VectorStyleArtifactGeometryKind::Symbol => {
                let mut symbol_layer = VectorLayerStyle::symbol(
                    self.source_layer,
                    self.text_rgba.unwrap_or(DEFAULT_VECTOR_SYMBOL_TEXT_RGBA),
                    self.text_size_pixels
                        .unwrap_or(DEFAULT_VECTOR_SYMBOL_TEXT_SIZE_PIXELS),
                );
                if let Some(text_field_fallbacks) = self.text_field_fallbacks {
                    symbol_layer.text_field_fallbacks = text_field_fallbacks;
                }
                if let Some(font_stack) = self.font_stack {
                    symbol_layer.font_stack = font_stack;
                }
                symbol_layer.halo_rgba = self.halo_rgba.unwrap_or(DEFAULT_VECTOR_SYMBOL_HALO_RGBA);
                symbol_layer.halo_width_pixels = self
                    .halo_width_pixels
                    .unwrap_or(DEFAULT_VECTOR_SYMBOL_HALO_WIDTH_PIXELS);
                symbol_layer.placement = match self.placement {
                    Some(VectorStyleArtifactSymbolPlacement::Line) => VectorSymbolPlacement::Line,
                    Some(VectorStyleArtifactSymbolPlacement::Point) | None => {
                        VectorSymbolPlacement::Point
                    }
                };
                symbol_layer.priority = self.priority.unwrap_or(DEFAULT_VECTOR_SYMBOL_PRIORITY);
                symbol_layer.allow_overlap = self
                    .allow_overlap
                    .unwrap_or(DEFAULT_VECTOR_SYMBOL_ALLOW_OVERLAP);
                for text_size_stop in self.text_size_zoom_stops {
                    symbol_layer = symbol_layer
                        .with_text_size_zoom_stop(text_size_stop.zoom, text_size_stop.size_pixels);
                }
                for rank_stop in self.rank_zoom_stops {
                    symbol_layer =
                        symbol_layer.with_rank_zoom_stop(rank_stop.zoom, rank_stop.maximum_rank);
                }
                for class_minimum_zoom in self.class_minimum_zooms {
                    symbol_layer = symbol_layer.with_class_minimum_zoom(
                        class_minimum_zoom.classes,
                        class_minimum_zoom.min_zoom,
                    );
                }
                symbol_layer
            }
        };
        if let Some(filter) = self.filter {
            layer_style = layer_style.with_attribute_filter(VectorAttributeFilter {
                attribute_name: filter.attribute_name,
                expected_text: filter.expected_text,
                matches_any: filter.matches_any,
                number_equals: filter.number_equals,
            });
        }
        layer_style = layer_style.with_zoom_span(
            self.min_zoom
                .unwrap_or(DEFAULT_VECTOR_STYLE_MINIMUM_ZOOM_LEVEL),
            self.max_zoom
                .unwrap_or(DEFAULT_VECTOR_STYLE_MAXIMUM_ZOOM_LEVEL),
        );
        for width_stop in self.width_zoom_stops {
            layer_style =
                layer_style.with_width_zoom_stop(width_stop.zoom, width_stop.width_pixels);
        }
        layer_style
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

/// Converts one decoded tile into one overlay batch per style layer that
/// painted at least one feature.
///
/// A layer style paints only features of its declared geometry kind whose
/// filter (when set) matches. Polygon features contribute one filled overlay
/// per exterior ring (positive shoelace area in the tile's `y`-down space),
/// carrying that exterior's following interior rings (negative area) as the
/// fill's subtracted holes. Every feature keeps its own overlay primitive, so
/// per-feature geometry stays representable; the batch is the style grouping
/// a dedicated vector pipeline would consume.
pub fn overlay_primitives_for_vector_tile(
    tile: &VectorTile,
    tile_address: VectorTileAddress,
    style: &VectorTileStyle,
) -> Vec<VectorOverlayBatch> {
    overlay_primitives_for_vector_tile_at_zoom(
        tile,
        tile_address,
        style,
        f64::from(tile_address.zoom_level),
    )
}

/// The same conversion as [`overlay_primitives_for_vector_tile`], with the
/// style's zoom span and width stops evaluated at an explicit `style_zoom_level`
/// rather than at the tile's own zoom level.
///
/// The base-map scheduler styles a cached tile at the camera's fractional zoom
/// level, so a pan across a width stop restyles the already-fetched tile
/// instead of fetching it again; the tile's address still converts every
/// coordinate, because the geometry belongs to the tile, not to the zoom the
/// paint is evaluated at.
pub fn overlay_primitives_for_vector_tile_at_zoom(
    tile: &VectorTile,
    tile_address: VectorTileAddress,
    style: &VectorTileStyle,
    style_zoom_level: f64,
) -> Vec<VectorOverlayBatch> {
    let mut batches = Vec::new();
    for layer_style in &style.layers {
        let overlays = overlays_for_layer_style(tile, tile_address, layer_style, style_zoom_level);
        if !overlays.is_empty() {
            batches.push(VectorOverlayBatch {
                layer_style: layer_style.clone(),
                overlays,
            });
        }
    }
    batches
}

/// Converts one decoded tile into its flat overlay primitive list under the
/// style (the batches of [`overlay_primitives_for_vector_tile`] in style
/// order, concatenated).
pub fn vector_tile_overlays(
    tile: &VectorTile,
    tile_address: VectorTileAddress,
    style: &VectorTileStyle,
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
    style: &VectorTileStyle,
    style_zoom_level: f64,
) -> Vec<MapOverlay> {
    overlay_primitives_for_vector_tile_at_zoom(tile, tile_address, style, style_zoom_level)
        .into_iter()
        .flat_map(|batch| batch.overlays)
        .collect()
}

/// Every overlay one style layer paints from the tile, in feature order.
fn overlays_for_layer_style(
    tile: &VectorTile,
    tile_address: VectorTileAddress,
    layer_style: &VectorLayerStyle,
    style_zoom_level: f64,
) -> Vec<MapOverlay> {
    let mut overlays = Vec::new();
    if !layer_style.is_visible_at_zoom(style_zoom_level) {
        return overlays;
    }
    let painted_width_pixels = layer_style.width_pixels_at_zoom(style_zoom_level);
    for layer in tile
        .layers
        .iter()
        .filter(|layer| layer.name == layer_style.source_layer)
    {
        for feature in &layer.features {
            if !feature_matches_filter(feature, layer_style.filter.as_ref()) {
                continue;
            }
            match (&feature.geometry, layer_style.geometry_kind) {
                (VectorTileGeometry::Polygons(rings), VectorStyleGeometryKind::Fill) => {
                    overlays.extend(filled_polygon_overlays_for_rings(
                        rings,
                        layer.extent,
                        tile_address,
                        layer_style.fill_rgba,
                    ));
                }
                (VectorTileGeometry::LineStrings(line_strings), VectorStyleGeometryKind::Line) => {
                    for line_string in line_strings {
                        let coordinates = line_string
                            .iter()
                            .map(|point| {
                                tile_local_point_to_coordinate(tile_address, layer.extent, point)
                            })
                            .collect();
                        overlays.push(MapOverlay::RoutePolyline(RoutePolylineOverlay {
                            coordinates,
                            rgba_color: layer_style.line_rgba,
                            width_pixels: painted_width_pixels,
                            stroke: None,
                            translate_pixels: [0.0, 0.0],
                            // The specification's line-translate-anchor default.
                            translate_anchor: rustic_maps_style::TranslateAnchor::Map,
                        }));
                    }
                }
                (VectorTileGeometry::Points(points), VectorStyleGeometryKind::Circle) => {
                    let positions = points
                        .iter()
                        .map(|point| {
                            tile_local_point_to_coordinate(tile_address, layer.extent, point)
                        })
                        .collect();
                    overlays.push(MapOverlay::PointMarkers(PointMarkersOverlay {
                        positions,
                        rgba_color: layer_style.circle_rgba,
                        radius_pixels: layer_style.circle_radius_pixels,
                        rgba_stroke_color: None,
                        stroke_width_pixels: 0.0,
                        blur_pixels: 0.0,
                        translate_pixels: [0.0, 0.0],
                        pitch_scale: rustic_maps_style::CirclePitchScale::Viewport,
                        pitch_alignment: rustic_maps_style::CirclePitchAlignment::Viewport,
                    }));
                }
                // A style paints only its declared kind; a feature of
                // another kind is simply not this layer's paint, and a
                // symbol layer's labels belong to the frame-level label
                // pass (S3), not to the tile's overlay primitives.
                _ => {}
            }
        }
    }
    overlays
}

/// Groups a polygon feature's rings into filled overlays: each exterior ring
/// owns its immediately following interior rings, the specification's order.
/// A leading interior ring (malformed ring order) has no exterior to subtract
/// from and is skipped rather than painted as a fill of its own.
fn filled_polygon_overlays_for_rings(
    rings: &[VectorTileLinearRing],
    layer_extent: u64,
    tile_address: VectorTileAddress,
    rgba_color: [f32; 4],
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
                rgba_color,
                rgba_outline_color: None,
                antialias: true,
                translate_pixels: [0.0, 0.0],
                translate_anchor: rustic_maps_style::TranslateAnchor::Viewport,
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

/// Whether the feature carries the filter's attribute with the expected text.
pub(crate) fn feature_matches_filter(
    feature: &VectorTileFeature,
    filter: Option<&VectorAttributeFilter>,
) -> bool {
    let Some(attribute_filter) = filter else {
        return true;
    };
    let expected_text = attribute_filter.expected_text.as_str();
    feature
        .attributes
        .iter()
        .any(|(attribute_name, attribute_value)| {
            if attribute_name != &attribute_filter.attribute_name {
                return false;
            }
            match attribute_value {
                VectorTileAttributeValue::Text(text) => {
                    (!expected_text.is_empty() && text == expected_text)
                        || attribute_filter
                            .matches_any
                            .iter()
                            .any(|accepted_text| text == accepted_text)
                }
                numeric_value => attribute_filter
                    .number_equals
                    .is_some_and(|expected_number| {
                        numeric_value.numeric_value() == Some(expected_number as f64)
                    }),
            }
        })
}

/// The pre-compiler label-candidate walk, kept beside the legacy style model
/// so the migration parity test can compare the old and new label passes field
/// by field. It reproduces the old label pass exactly: filter, per-class
/// minimum zoom, rank ceiling, ordered text fallbacks, then placement.
pub(crate) fn legacy_label_candidates_for_vector_tile(
    tile: &VectorTile,
    tile_address: VectorTileAddress,
    style: &VectorTileStyle,
    style_zoom_level: f64,
) -> Vec<crate::labels::MapLabelCandidate> {
    let mut candidates = Vec::new();
    for (style_order, layer_style) in style.layers.iter().enumerate() {
        if layer_style.geometry_kind != VectorStyleGeometryKind::Symbol
            || !layer_style.is_visible_at_zoom(style_zoom_level)
        {
            continue;
        }
        let text_size_pixels = layer_style.text_size_pixels_at_zoom(style_zoom_level);
        let maximum_rank = layer_style.maximum_rank_at_zoom(style_zoom_level);
        for layer in tile
            .layers
            .iter()
            .filter(|layer| layer.name == layer_style.source_layer)
        {
            for feature in &layer.features {
                if !feature_matches_filter(feature, layer_style.filter.as_ref()) {
                    continue;
                }
                let feature_class = legacy_label_class_for_feature(feature);
                if !layer_style.is_feature_class_visible_at_zoom(feature_class, style_zoom_level) {
                    continue;
                }
                let rank = legacy_label_rank_for_feature(feature);
                if rank > maximum_rank {
                    continue;
                }
                let Some(text) =
                    legacy_label_text_for_feature(feature, &layer_style.text_field_fallbacks)
                else {
                    continue;
                };
                match layer_style.placement {
                    VectorSymbolPlacement::Point => {
                        let VectorTileGeometry::Points(points) = &feature.geometry else {
                            continue;
                        };
                        for point in points {
                            candidates.push(crate::labels::MapLabelCandidate {
                                text: text.clone(),
                                anchor: tile_local_point_to_coordinate(
                                    tile_address,
                                    layer.extent,
                                    point,
                                ),
                                line_coordinates: Vec::new(),
                                text_rgba: layer_style.text_rgba,
                                text_size_pixels,
                                halo_rgba: layer_style.halo_rgba,
                                halo_width_pixels: layer_style.halo_width_pixels,
                                priority: layer_style.priority,
                                rank,
                                allow_overlap: layer_style.allow_overlap,
                                ignore_placement: false,
                                avoid_edges: false,
                                text_offset_pixels: [0.0, 0.0],
                                text_padding_pixels: crate::labels::LABEL_COLLISION_PADDING_PIXELS,
                                text_pitch_alignment:
                                    rustic_maps_style::TextPitchAlignment::Viewport,
                                repeat_spacing_pixels:
                                    crate::labels::LINE_LABEL_REPEAT_SPACING_PIXELS,
                                line_center_only: false,
                                text_max_width_ems: 10.0,
                                text_line_height_ems: 1.2,
                                text_letter_spacing_ems: 0.0,
                                text_justify: rustic_maps_style::TextJustify::Auto,
                                text_anchor: rustic_maps_style::TextAnchor::Center,
                                text_rotation_alignment:
                                    rustic_maps_style::TextRotationAlignment::Auto,
                                text_keep_upright: true,
                                text_variable_anchors: Vec::new(),
                                text_radial_offset_ems: 0.0,
                                text_max_angle_radians:
                                    crate::labels::LINE_LABEL_STRAIGHT_RUN_ANGLE_TOLERANCE_RADIANS,
                                style_order,
                                icon: None,
                                shield: None,
                            });
                        }
                    }
                    VectorSymbolPlacement::Line => {
                        let VectorTileGeometry::LineStrings(line_strings) = &feature.geometry
                        else {
                            continue;
                        };
                        for line_string in line_strings {
                            if line_string.len() < 2 {
                                continue;
                            }
                            let line_coordinates: Vec<GeographicCoordinate> = line_string
                                .iter()
                                .map(|point| {
                                    tile_local_point_to_coordinate(
                                        tile_address,
                                        layer.extent,
                                        point,
                                    )
                                })
                                .collect();
                            let anchor = line_coordinates[0];
                            candidates.push(crate::labels::MapLabelCandidate {
                                text: text.clone(),
                                anchor,
                                line_coordinates,
                                text_rgba: layer_style.text_rgba,
                                text_size_pixels,
                                halo_rgba: layer_style.halo_rgba,
                                halo_width_pixels: layer_style.halo_width_pixels,
                                priority: layer_style.priority,
                                rank,
                                allow_overlap: layer_style.allow_overlap,
                                ignore_placement: false,
                                avoid_edges: false,
                                text_offset_pixels: [0.0, 0.0],
                                text_padding_pixels: crate::labels::LABEL_COLLISION_PADDING_PIXELS,
                                text_pitch_alignment:
                                    rustic_maps_style::TextPitchAlignment::Viewport,
                                repeat_spacing_pixels:
                                    crate::labels::LINE_LABEL_REPEAT_SPACING_PIXELS,
                                line_center_only: false,
                                text_max_width_ems: 10.0,
                                text_line_height_ems: 1.2,
                                text_letter_spacing_ems: 0.0,
                                text_justify: rustic_maps_style::TextJustify::Auto,
                                text_anchor: rustic_maps_style::TextAnchor::Center,
                                text_rotation_alignment:
                                    rustic_maps_style::TextRotationAlignment::Auto,
                                text_keep_upright: true,
                                text_variable_anchors: Vec::new(),
                                text_radial_offset_ems: 0.0,
                                text_max_angle_radians:
                                    crate::labels::LINE_LABEL_STRAIGHT_RUN_ANGLE_TOLERANCE_RADIANS,
                                style_order,
                                icon: None,
                                shield: None,
                            });
                        }
                    }
                }
            }
        }
    }
    candidates
}

/// The first ordered text fallback the feature carries as a non-empty text
/// attribute, or `None` when it carries none.
fn legacy_label_text_for_feature(
    feature: &VectorTileFeature,
    text_field_fallbacks: &[String],
) -> Option<String> {
    for fallback in text_field_fallbacks {
        let carried_text =
            feature
                .attributes
                .iter()
                .find_map(|(attribute_name, attribute_value)| match attribute_value {
                    VectorTileAttributeValue::Text(text)
                        if attribute_name == fallback && !text.is_empty() =>
                    {
                        Some(text.clone())
                    }
                    _ => None,
                });
        if carried_text.is_some() {
            return carried_text;
        }
    }
    None
}

/// The feature's `class` attribute as text, or `None` when it carries none.
fn legacy_label_class_for_feature(feature: &VectorTileFeature) -> Option<&str> {
    feature
        .attributes
        .iter()
        .find_map(|(attribute_name, attribute_value)| match attribute_value {
            VectorTileAttributeValue::Text(text) if attribute_name == "class" => {
                Some(text.as_str())
            }
            _ => None,
        })
}

/// The feature's `rank` attribute as a whole number, or the maximum when it
/// carries none.
fn legacy_label_rank_for_feature(feature: &VectorTileFeature) -> u32 {
    feature
        .attributes
        .iter()
        .find(|(attribute_name, _)| attribute_name == "rank")
        .and_then(|(_, attribute_value)| attribute_value.numeric_value())
        .map(|rank| {
            if rank <= 0.0 {
                0
            } else if rank >= f64::from(u32::MAX) {
                u32::MAX
            } else {
                rank as u32
            }
        })
        .unwrap_or(u32::MAX)
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
    use rustic_maps_vector_tiles::decode_vector_tile;

    const SOURDOUGH_TILE_BYTES: &[u8] =
        include_bytes!("../../../fixtures/openstreetmap-us/sourdough_tile_z6_x18_y27.mvt");
    const SOURDOUGH_TILE_ADDRESS: VectorTileAddress = VectorTileAddress {
        zoom_level: 6,
        tile_x: 18,
        tile_y: 27,
    };

    #[test]
    fn tile_local_conversion_matches_the_web_mercator_grid() {
        let world_tile = VectorTileAddress {
            zoom_level: 0,
            tile_x: 0,
            tile_y: 0,
        };
        let corner = tile_local_point_to_coordinate(
            world_tile,
            4096,
            &VectorTileGeometryPoint {
                x_coordinate: 0,
                y_coordinate: 0,
            },
        );
        assert!((corner.longitude - -180.0).abs() < 1e-9);
        assert!((corner.latitude - MERCATOR_MAXIMUM_LATITUDE_DEGREES).abs() < 1e-6);
        let center = tile_local_point_to_coordinate(
            world_tile,
            4096,
            &VectorTileGeometryPoint {
                x_coordinate: 2048,
                y_coordinate: 2048,
            },
        );
        assert!(center.longitude.abs() < 1e-9);
        assert!(center.latitude.abs() < 1e-9);
    }

    #[test]
    fn every_style_thins_its_roads_below_the_city_zoom() {
        for style_id in [
            VECTOR_STYLE_ID_CARTO_LIGHT,
            VECTOR_STYLE_ID_CARTO_DARK,
            VECTOR_STYLE_ID_OPENMAPTILES_LIGHT,
            VECTOR_STYLE_ID_OPENMAPTILES_DARK,
            VECTOR_STYLE_ID_BRIGHT_LIGHT,
            VECTOR_STYLE_ID_FIORD_DARK,
            VECTOR_STYLE_ID_POSITRON_LIGHT,
        ] {
            let style = vector_tile_style_from_id(style_id).expect("the style parses");
            let road_layers: Vec<&VectorLayerStyle> = style
                .layers
                .iter()
                .filter(|layer| {
                    layer.source_layer == "highways"
                        && layer.geometry_kind == VectorStyleGeometryKind::Line
                })
                .collect();
            let class_of = |layer: &VectorLayerStyle| {
                layer
                    .filter
                    .as_ref()
                    .map(|filter| filter.expected_text.clone())
                    .unwrap_or_default()
            };

            // Below the city zoom every visible road is a tapered version of
            // its city weight, and the minor classes are absent.
            for layer in &road_layers {
                if layer.is_visible_at_zoom(9.0) {
                    let regional_width = layer.width_pixels_at_zoom(9.0);
                    let city_width = layer.width_pixels_at_zoom(14.0);
                    assert!(
                        regional_width <= city_width * 0.6 + 0.05,
                        "{style_id} paints {} at {regional_width} px at zoom 9 (city {city_width})",
                        class_of(layer)
                    );
                }
            }
            let visible_classes: std::collections::BTreeSet<String> = road_layers
                .iter()
                .filter(|layer| layer.is_visible_at_zoom(9.0))
                .map(|layer| class_of(layer))
                .collect();
            for hidden_class in [
                "minor",
                "residential",
                "service",
                "track",
                "path",
                "pedestrian",
            ] {
                assert!(
                    !visible_classes.contains(hidden_class),
                    "{style_id} must not paint {hidden_class} at zoom 9"
                );
            }
        }
    }

    #[test]
    fn the_legacy_carto_model_reads_route_references_as_plain_text_at_regional_zooms() {
        use super::legacy_label_candidates_for_vector_tile;
        // The synthetic z9 OpenStreetMap US-schema tile over the demo's
        // region: its `routes` layer carries road and transit refs (80, Gold).
        // The legacy model predates badges, so it reads those refs as plain
        // text; the compiled Carto style turns the same candidates into
        // badges, which the parity harness and goldens verify.
        let tile_bytes: &[u8] =
            include_bytes!("../../../fixtures/openstreetmap-us/osmus_tile_z9_x83_y196.mvt");
        let tile = decode_vector_tile(tile_bytes).expect("the tile decodes");
        let tile_address = VectorTileAddress {
            zoom_level: 9,
            tile_x: 83,
            tile_y: 196,
        };
        for style_id in [VECTOR_STYLE_ID_CARTO_LIGHT, VECTOR_STYLE_ID_CARTO_DARK] {
            let style = vector_tile_style_from_id(style_id).expect("the Carto style parses");
            let candidates =
                legacy_label_candidates_for_vector_tile(&tile, tile_address, &style, 9.0);
            assert!(
                candidates
                    .iter()
                    .any(|candidate| ["5", "80", "99"].contains(&candidate.text.as_str())),
                "{style_id} must show the route references at zoom 9"
            );
        }
    }

    #[test]
    fn the_carto_styles_draw_a_road_hierarchy_at_regional_zooms() {
        for style_id in [VECTOR_STYLE_ID_CARTO_LIGHT, VECTOR_STYLE_ID_CARTO_DARK] {
            let style = vector_tile_style_from_id(style_id).expect("the Carto style parses");
            let road_layers: Vec<&VectorLayerStyle> = style
                .layers
                .iter()
                .filter(|layer| {
                    layer.source_layer == "highways"
                        && layer.geometry_kind == VectorStyleGeometryKind::Line
                })
                .collect();
            let class_of = |layer: &VectorLayerStyle| {
                layer
                    .filter
                    .as_ref()
                    .map(|filter| filter.expected_text.clone())
                    .unwrap_or_default()
            };

            // A regional frame (the demo's 3 km scale) paints only the upper
            // classes, and none of them as a thick ribbon.
            for layer in &road_layers {
                if layer.is_visible_at_zoom(9.0) {
                    assert!(
                        layer.width_pixels_at_zoom(9.0) <= 3.5,
                        "{style_id} paints {} at {} px at zoom 9",
                        class_of(layer),
                        layer.width_pixels_at_zoom(9.0)
                    );
                }
            }
            let visible_classes: std::collections::BTreeSet<String> = road_layers
                .iter()
                .filter(|layer| layer.is_visible_at_zoom(9.0))
                .map(|layer| class_of(layer))
                .collect();
            assert!(visible_classes.contains("motorway"), "{style_id}");
            assert!(visible_classes.contains("secondary"), "{style_id}");
            for hidden_class in ["tertiary", "residential", "service", "pedestrian"] {
                assert!(
                    !visible_classes.contains(hidden_class),
                    "{style_id} must not paint {hidden_class} at zoom 9"
                );
            }

            // The motorway fill (the narrower of its casing and fill layers)
            // interpolates to about 2.5 px there.
            let motorway_fill_width = road_layers
                .iter()
                .filter(|layer| class_of(layer) == "motorway")
                .map(|layer| layer.width_pixels_at_zoom(9.0))
                .fold(f64::INFINITY, f64::min);
            assert!(
                (motorway_fill_width - 2.45).abs() < 0.01,
                "{style_id} motorway fill at zoom 9 is {motorway_fill_width}"
            );

            // The street grid appears at its class zoom and stays thin.
            let residential = road_layers
                .iter()
                .find(|layer| class_of(layer) == "residential")
                .expect("the Carto style paints residential streets");
            assert!(!residential.is_visible_at_zoom(11.0));
            assert!(residential.is_visible_at_zoom(12.0));
            assert!((residential.width_pixels_at_zoom(16.0) - 1.2).abs() < 0.01);
        }
    }

    #[test]
    fn the_carto_styles_label_named_roads_along_their_lines_and_named_places() {
        use super::legacy_label_candidates_for_vector_tile;
        use rustic_maps_vector_tiles::{
            DEFAULT_TILE_EXTENT, VectorTileAttributeValue, VectorTileFeature, VectorTileGeometry,
            VectorTileGeometryPoint, VectorTileGeometryType, VectorTileLayer,
        };

        // A minimal Carto-schema tile: one named motorway and one named place.
        // The road is a line, so the style must place its name along the line
        // rather than at a point.
        let tile = VectorTile {
            layers: vec![
                VectorTileLayer {
                    name: "highways".to_string(),
                    extent: DEFAULT_TILE_EXTENT,
                    version: 2,
                    features: vec![VectorTileFeature {
                        identifier: None,
                        geometry_type: VectorTileGeometryType::LineString,
                        geometry: VectorTileGeometry::LineStrings(vec![vec![
                            VectorTileGeometryPoint {
                                x_coordinate: 0,
                                y_coordinate: 0,
                            },
                            VectorTileGeometryPoint {
                                x_coordinate: 2048,
                                y_coordinate: 1024,
                            },
                        ]]),
                        attributes: vec![
                            (
                                "highway".to_string(),
                                VectorTileAttributeValue::Text("motorway".to_string()),
                            ),
                            (
                                "name".to_string(),
                                VectorTileAttributeValue::Text("Test Road".to_string()),
                            ),
                        ],
                    }],
                },
                VectorTileLayer {
                    name: "places".to_string(),
                    extent: DEFAULT_TILE_EXTENT,
                    version: 2,
                    features: vec![VectorTileFeature {
                        identifier: None,
                        geometry_type: VectorTileGeometryType::Point,
                        geometry: VectorTileGeometry::Points(vec![VectorTileGeometryPoint {
                            x_coordinate: 2048,
                            y_coordinate: 1024,
                        }]),
                        attributes: vec![(
                            "name".to_string(),
                            VectorTileAttributeValue::Text("Testville".to_string()),
                        )],
                    }],
                },
            ],
        };

        for style_id in [VECTOR_STYLE_ID_CARTO_LIGHT, VECTOR_STYLE_ID_CARTO_DARK] {
            let style = vector_tile_style_from_id(style_id).expect("the Carto style parses");
            let candidates = legacy_label_candidates_for_vector_tile(
                &tile,
                SOURDOUGH_TILE_ADDRESS,
                &style,
                SOURDOUGH_TILE_ADDRESS.zoom_level as f64 + 9.0,
            );
            assert!(
                candidates
                    .iter()
                    .any(|candidate| candidate.text == "Test Road"
                        && !candidate.line_coordinates.is_empty()),
                "{style_id} must place a named road's text along its line"
            );
            assert!(
                candidates
                    .iter()
                    .any(|candidate| candidate.text == "Testville"),
                "{style_id} must place a named place's text"
            );
        }
    }

    #[test]
    fn the_sourdough_fixture_paints_fill_line_and_circle_layers() {
        let tile = decode_vector_tile(SOURDOUGH_TILE_BYTES).unwrap();
        let style = VectorTileStyle {
            background: None,
            layers: vec![
                VectorLayerStyle::fill("water", [0.2, 0.5, 0.9, 0.4]),
                VectorLayerStyle::line("highways", [0.9, 0.6, 0.2, 0.9], 2.0),
                VectorLayerStyle::circle("places", [0.9, 0.2, 0.2, 0.9], 3.0),
            ],
        };
        let overlays = vector_tile_overlays(&tile, SOURDOUGH_TILE_ADDRESS, &style);
        assert!(
            overlays
                .iter()
                .any(|overlay| matches!(overlay, MapOverlay::FilledPolygon(_)))
        );
        assert!(
            overlays
                .iter()
                .any(|overlay| matches!(overlay, MapOverlay::RoutePolyline(_)))
        );
        assert!(
            overlays
                .iter()
                .any(|overlay| matches!(overlay, MapOverlay::PointMarkers(_)))
        );
        // Every painted coordinate lands inside the fixture tile's bounds.
        for overlay in &overlays {
            match overlay {
                MapOverlay::FilledPolygon(polygon) => {
                    for coordinate in &polygon.ring_coordinates {
                        assert_tile_bounds_contain(coordinate);
                    }
                    for hole_ring in &polygon.hole_ring_coordinates {
                        for coordinate in hole_ring {
                            assert_tile_bounds_contain(coordinate);
                        }
                    }
                }
                MapOverlay::RoutePolyline(polyline) => {
                    for coordinate in &polyline.coordinates {
                        assert_tile_bounds_contain(coordinate);
                    }
                }
                MapOverlay::PointMarkers(markers) => {
                    for coordinate in &markers.positions {
                        assert_tile_bounds_contain(coordinate);
                    }
                }
                MapOverlay::PointPins(pins) => {
                    for coordinate in &pins.positions {
                        assert_tile_bounds_contain(coordinate);
                    }
                }
                MapOverlay::TrafficFlow(_) => {}
            }
        }
    }

    #[test]
    fn a_filter_selects_only_matching_features() {
        let tile = decode_vector_tile(SOURDOUGH_TILE_BYTES).unwrap();
        let style = VectorTileStyle {
            background: None,
            layers: vec![
                VectorLayerStyle::fill("water", [0.2, 0.5, 0.9, 0.4])
                    .with_filter("no_such_attribute", "no_such_value"),
            ],
        };
        assert!(vector_tile_overlays(&tile, SOURDOUGH_TILE_ADDRESS, &style).is_empty());
    }

    /// One attribute-carrying feature for the filter-matching cases (the
    /// geometry is irrelevant to a filter, so an empty point list is legal
    /// input).
    fn feature_with_attributes(
        attributes: &[(&str, VectorTileAttributeValue)],
    ) -> VectorTileFeature {
        VectorTileFeature {
            identifier: None,
            geometry_type: VectorTileGeometryType::Point,
            geometry: VectorTileGeometry::Points(Vec::new()),
            attributes: attributes
                .iter()
                .map(|(attribute_name, attribute_value)| {
                    ((*attribute_name).to_string(), attribute_value.clone())
                })
                .collect(),
        }
    }

    /// A filter over one text attribute with the given exact value.
    fn text_filter(attribute_name: &str, expected_text: &str) -> VectorAttributeFilter {
        VectorAttributeFilter {
            attribute_name: attribute_name.to_string(),
            expected_text: expected_text.to_string(),
            matches_any: Vec::new(),
            number_equals: None,
        }
    }

    #[test]
    fn a_filter_accepts_any_declared_text_or_numeric_value() {
        let feature = feature_with_attributes(&[
            (
                "class",
                VectorTileAttributeValue::Text("residential".to_string()),
            ),
            ("admin_level", VectorTileAttributeValue::Unsigned(2)),
            ("tunnel", VectorTileAttributeValue::Signed(1)),
        ]);
        // The exact text still selects, exactly.
        assert!(feature_matches_filter(
            &feature,
            Some(&text_filter("class", "residential"))
        ));
        assert!(!feature_matches_filter(
            &feature,
            Some(&text_filter("class", "commercial"))
        ));
        // Several accepted spellings select any one of them.
        let several_texts = VectorAttributeFilter {
            attribute_name: "class".to_string(),
            expected_text: String::new(),
            matches_any: vec!["residential".to_string(), "commercial".to_string()],
            number_equals: None,
        };
        assert!(feature_matches_filter(&feature, Some(&several_texts)));
        // A numeric attribute matches its integer whatever the wire type.
        for numeric_variant in [
            VectorTileAttributeValue::Unsigned(2),
            VectorTileAttributeValue::Signed(2),
            VectorTileAttributeValue::ZigZagSigned(2),
            VectorTileAttributeValue::Double(2.0),
            VectorTileAttributeValue::Float(2.0),
        ] {
            let numeric_feature = feature_with_attributes(&[("admin_level", numeric_variant)]);
            let numeric_filter = VectorAttributeFilter {
                attribute_name: "admin_level".to_string(),
                expected_text: String::new(),
                matches_any: Vec::new(),
                number_equals: Some(2),
            };
            assert!(feature_matches_filter(
                &numeric_feature,
                Some(&numeric_filter)
            ));
        }
        let other_number = VectorAttributeFilter {
            number_equals: Some(4),
            ..text_filter("admin_level", "")
        };
        assert!(!feature_matches_filter(&feature, Some(&other_number)));
        // A text value must not match a numeric filter and vice versa.
        let text_as_number = VectorAttributeFilter {
            number_equals: Some(2),
            ..text_filter("class", "")
        };
        assert!(!feature_matches_filter(&feature, Some(&text_as_number)));
        let number_as_text = text_filter("admin_level", "2");
        assert!(!feature_matches_filter(&feature, Some(&number_as_text)));
        // A filter that accepts no value selects nothing, never everything.
        let empty_filter = text_filter("class", "");
        assert!(!feature_matches_filter(&feature, Some(&empty_filter)));
        // No filter selects every feature.
        assert!(feature_matches_filter(&feature, None));
    }

    #[test]
    fn an_artifact_filter_carries_several_accepted_values() {
        let style = vector_tile_style_from_json(
            r#"{"layers": [
                {"source_layer": "landuse", "geometry_kind": "fill",
                 "rgba": [0.5, 0.5, 0.5, 1.0],
                 "filter": {"attribute_name": "class",
                            "matches_any": ["residential", "commercial"]}},
                {"source_layer": "boundary", "geometry_kind": "line",
                 "rgba": [0.5, 0.5, 0.5, 1.0], "width_pixels": 2.0,
                 "filter": {"attribute_name": "admin_level", "number_equals": 2}}
            ]}"#,
        )
        .expect("the artifact with the extended filter parses");
        let text_filter = style.layers[0]
            .filter
            .as_ref()
            .expect("the landuse layer carries its filter");
        assert_eq!(text_filter.expected_text, "");
        assert_eq!(text_filter.matches_any, vec!["residential", "commercial"]);
        assert_eq!(text_filter.number_equals, None);
        let numeric_filter = style.layers[1]
            .filter
            .as_ref()
            .expect("the boundary layer carries its filter");
        assert_eq!(numeric_filter.attribute_name, "admin_level");
        assert_eq!(numeric_filter.number_equals, Some(2));
        assert!(numeric_filter.matches_any.is_empty());
    }

    #[test]
    fn polygon_features_with_interior_rings_carry_their_holes() {
        let tile = decode_vector_tile(SOURDOUGH_TILE_BYTES).unwrap();
        let style = VectorTileStyle {
            background: None,
            layers: vec![VectorLayerStyle::fill("water", [0.2, 0.5, 0.9, 0.4])],
        };
        let overlays = vector_tile_overlays(&tile, SOURDOUGH_TILE_ADDRESS, &style);
        let holed_polygons: Vec<&FilledPolygonOverlay> = overlays
            .iter()
            .filter_map(|overlay| match overlay {
                MapOverlay::FilledPolygon(polygon) if !polygon.hole_ring_coordinates.is_empty() => {
                    Some(polygon)
                }
                _ => None,
            })
            .collect();
        assert!(
            !holed_polygons.is_empty(),
            "the fixture's water layer has interior rings to subtract"
        );
        for (polygon_index, polygon) in holed_polygons.iter().enumerate() {
            for (hole_index, hole_ring) in polygon.hole_ring_coordinates.iter().enumerate() {
                assert!(
                    !hole_ring.is_empty(),
                    "a decoded interior ring never converts to an empty hole"
                );
                for (vertex_index, hole_coordinate) in hole_ring.iter().enumerate() {
                    assert!(
                        point_lies_within_or_on_ring(hole_coordinate, &polygon.ring_coordinates),
                        "polygon {polygon_index} hole {hole_index} vertex {vertex_index} \
                         ({}) must lie inside or on its exterior ring",
                        hole_coordinate.longitude
                    );
                }
            }
        }
    }

    #[test]
    fn the_water_layer_assembles_into_finite_overlay_geometry() {
        // The fixture's water layer includes the 150-ring multipolygon, so
        // this runs the bridge splicing and ear clipping over the real
        // messy data end to end, then checks the assembly still emits finite
        // clip-space triangles.
        use crate::camera::{DEFAULT_FIELD_OF_VIEW_DEGREES, MapCamera, ViewportPixels};
        use crate::overlay::overlay_triangle_vertices;
        use crate::projection::normalized_world_to_coordinate;

        let tile = decode_vector_tile(SOURDOUGH_TILE_BYTES).unwrap();
        let style = VectorTileStyle {
            background: None,
            layers: vec![VectorLayerStyle::fill("water", [0.2, 0.5, 0.9, 0.4])],
        };
        let overlays = vector_tile_overlays(&tile, SOURDOUGH_TILE_ADDRESS, &style);
        let tiles_per_axis = f64::from(1u32 << SOURDOUGH_TILE_ADDRESS.zoom_level);
        let camera_center = normalized_world_to_coordinate([
            (f64::from(SOURDOUGH_TILE_ADDRESS.tile_x) + 0.5) / tiles_per_axis,
            (f64::from(SOURDOUGH_TILE_ADDRESS.tile_y) + 0.5) / tiles_per_axis,
        ]);
        let camera = MapCamera::new(camera_center, 6.0, 0.0, 0.0);
        let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
        let vertices =
            overlay_triangle_vertices(&camera, &viewport, DEFAULT_FIELD_OF_VIEW_DEGREES, &overlays);
        assert!(
            !vertices.is_empty(),
            "the water polygons assemble into triangles"
        );
        assert!(
            vertices.iter().all(|vertex| vertex
                .clip_position
                .iter()
                .all(|component| component.is_finite())),
            "no assembled corner may be non-finite"
        );
    }

    #[test]
    fn a_fill_layer_without_holes_paints_its_exteriors_unchanged() {
        let tile = decode_vector_tile(SOURDOUGH_TILE_BYTES).unwrap();
        let landuse_layer = tile
            .layers
            .iter()
            .find(|layer| layer.name == "landuse")
            .expect("the fixture carries a landuse layer");
        let exterior_ring_count = landuse_layer
            .features
            .iter()
            .map(|feature| match &feature.geometry {
                VectorTileGeometry::Polygons(rings) => {
                    rings.iter().filter(|ring| ring_is_exterior(ring)).count()
                }
                _ => 0,
            })
            .sum::<usize>();
        let style = VectorTileStyle {
            background: None,
            layers: vec![VectorLayerStyle::fill("landuse", [0.2, 0.5, 0.9, 0.4])],
        };
        let overlays = vector_tile_overlays(&tile, SOURDOUGH_TILE_ADDRESS, &style);
        assert_eq!(
            overlays.len(),
            exterior_ring_count,
            "one fill per exterior ring, with no ring added or dropped"
        );
        assert!(overlays.iter().all(|overlay| match overlay {
            MapOverlay::FilledPolygon(polygon) => polygon.hole_ring_coordinates.is_empty(),
            _ => false,
        }));
    }

    #[test]
    fn overlay_primitives_group_into_one_batch_per_painted_style() {
        let tile = decode_vector_tile(SOURDOUGH_TILE_BYTES).unwrap();
        let style = VectorTileStyle {
            background: None,
            layers: vec![
                VectorLayerStyle::fill("water", [0.2, 0.5, 0.9, 0.4]),
                VectorLayerStyle::line("highways", [0.9, 0.6, 0.2, 0.9], 2.0),
                VectorLayerStyle::circle("places", [0.9, 0.2, 0.2, 0.9], 3.0),
            ],
        };
        let batches = overlay_primitives_for_vector_tile(&tile, SOURDOUGH_TILE_ADDRESS, &style);
        assert_eq!(
            batches.len(),
            style.layers.len(),
            "every style paints features in this fixture, so each gets one batch"
        );
        for (batch, layer_style) in batches.iter().zip(&style.layers) {
            assert_eq!(&batch.layer_style, layer_style);
            assert!(
                !batch.overlays.is_empty(),
                "a batch is only produced for a style that painted something"
            );
        }
        let flattened: Vec<MapOverlay> = batches
            .iter()
            .flat_map(|batch| batch.overlays.iter().cloned())
            .collect();
        assert_eq!(
            flattened,
            vector_tile_overlays(&tile, SOURDOUGH_TILE_ADDRESS, &style),
            "the flat conversion is the batches concatenated in style order"
        );
    }

    #[test]
    fn the_committed_style_artifact_parses_to_the_in_code_defaults() {
        assert_eq!(
            vector_tile_style_from_json(DEFAULT_VECTOR_STYLE_ARTIFACT_JSON)
                .expect("the committed artifact parses"),
            in_code_default_vector_tile_style()
        );
        assert_eq!(
            default_vector_tile_style(),
            in_code_default_vector_tile_style()
        );
    }

    #[test]
    fn every_committed_style_id_parses_and_an_unknown_id_is_refused() {
        for style_id in [
            VECTOR_STYLE_ID_CARTO_LIGHT,
            VECTOR_STYLE_ID_CARTO_DARK,
            VECTOR_STYLE_ID_OPENMAPTILES_LIGHT,
            VECTOR_STYLE_ID_OPENMAPTILES_DARK,
        ] {
            let style = vector_tile_style_from_id(style_id)
                .unwrap_or_else(|style_error| panic!("{style_id}: {style_error}"));
            assert!(!style.layers.is_empty(), "{style_id} paints no layer");
        }
        // The palettes really differ: the dark carto artifact is not a copy of
        // the light one.
        let light_style = vector_tile_style_from_id(VECTOR_STYLE_ID_CARTO_LIGHT)
            .expect("the light carto artifact parses");
        let dark_style = vector_tile_style_from_id(VECTOR_STYLE_ID_CARTO_DARK)
            .expect("the dark carto artifact parses");
        assert_ne!(light_style, dark_style);
        let unknown_refusal =
            vector_tile_style_from_id("no-such-style").expect_err("an unknown style id is refused");
        assert!(unknown_refusal.to_string().contains("no-such-style"));
    }

    #[test]
    fn a_layer_paints_only_inside_its_zoom_span() {
        // MapLibre's minzoom/maxzoom: below `minzoom` and at or above
        // `maxzoom` the layer is invisible; the tile's own zoom is what the
        // style evaluates against (the base refetches at that zoom).
        let tile = decode_vector_tile(SOURDOUGH_TILE_BYTES).unwrap();
        let base_style = || VectorTileStyle {
            background: None,
            layers: vec![
                VectorLayerStyle::line("highways", [0.9, 0.6, 0.2, 0.9], 2.0)
                    .with_zoom_span(7.0, 12.0),
            ],
        };
        let at_tile_zoom_six = vector_tile_overlays(&tile, SOURDOUGH_TILE_ADDRESS, &base_style());
        assert!(
            at_tile_zoom_six.is_empty(),
            "a layer with minzoom 7 stays invisible on the zoom-6 tile"
        );
        let visible_address = VectorTileAddress {
            zoom_level: 8,
            ..SOURDOUGH_TILE_ADDRESS
        };
        let at_zoom_eight = vector_tile_overlays(&tile, visible_address, &base_style());
        assert!(
            !at_zoom_eight.is_empty(),
            "the same layer paints inside its zoom span"
        );
    }

    #[test]
    fn width_stops_interpolate_at_the_tile_zoom() {
        let layer_style = VectorLayerStyle::line("highways", [0.9, 0.6, 0.2, 0.9], 2.0)
            .with_width_zoom_stop(10.0, 2.0)
            .with_width_zoom_stop(14.0, 6.0);
        assert_eq!(layer_style.width_pixels_at_zoom(10.0), 2.0);
        assert_eq!(layer_style.width_pixels_at_zoom(12.0), 4.0);
        assert_eq!(layer_style.width_pixels_at_zoom(14.0), 6.0);
        assert_eq!(
            layer_style.width_pixels_at_zoom(18.0),
            6.0,
            "beyond the last stop the last width holds"
        );
        assert_eq!(
            layer_style.width_pixels_at_zoom(6.0),
            2.0,
            "below the first stop the first width holds"
        );
    }

    #[test]
    fn the_named_styles_carry_their_background_and_paint_the_schema_tile() {
        // The three named styles (Bright light, Fiord dark, Positron quiet
        // light) are curated transcriptions of the OpenFreeMap style family;
        // each parses, carries the upstream background the canvas clears to,
        // and paints the committed OpenMapTiles-schema tile with fills and
        // lines.
        let openmaptiles_tile_bytes: &[u8] =
            include_bytes!("../../../fixtures/openfreemap/openmaptiles_tile_z14_x2661_y6286.pbf");
        let tile = decode_vector_tile(openmaptiles_tile_bytes).unwrap();
        for (style_id, expected_background) in [
            (
                VECTOR_STYLE_ID_BRIGHT_LIGHT,
                [0.972549, 0.956863, 0.941176, 1.0],
            ),
            (
                VECTOR_STYLE_ID_FIORD_DARK,
                [0.270588, 0.317647, 0.431373, 1.0],
            ),
            (VECTOR_STYLE_ID_POSITRON_LIGHT, [0.949, 0.953, 0.941, 1.0]),
        ] {
            let style = vector_tile_style_from_id(style_id)
                .unwrap_or_else(|failure| panic!("{style_id}: {failure:?}"));
            assert_eq!(
                style.background,
                Some(expected_background),
                "{style_id} carries its upstream background"
            );
            let overlays = vector_tile_overlays(
                &tile,
                VectorTileAddress {
                    zoom_level: 14,
                    tile_x: 2661,
                    tile_y: 6286,
                },
                &style,
            );
            assert!(
                overlays
                    .iter()
                    .any(|overlay| matches!(overlay, MapOverlay::FilledPolygon(_))),
                "{style_id} paints fills"
            );
            assert!(
                overlays
                    .iter()
                    .any(|overlay| matches!(overlay, MapOverlay::RoutePolyline(_))),
                "{style_id} paints lines"
            );
        }
    }

    #[test]
    fn the_openmaptiles_style_paints_the_committed_schema_tile() {
        // The OpenFreeMap tile is OpenMapTiles-schema data: its layers are
        // landcover/landuse/park/water/waterway/building/boundary/
        // transportation/place, so this pins the second schemas' vocabulary.
        let openmaptiles_tile_bytes: &[u8] =
            include_bytes!("../../../fixtures/openfreemap/openmaptiles_tile_z14_x2661_y6286.pbf");
        let tile = decode_vector_tile(openmaptiles_tile_bytes).unwrap();
        let style = vector_tile_style_from_id(VECTOR_STYLE_ID_OPENMAPTILES_LIGHT)
            .expect("the openmaptiles light artifact parses");
        let overlays = vector_tile_overlays(
            &tile,
            VectorTileAddress {
                zoom_level: 14,
                tile_x: 2661,
                tile_y: 6286,
            },
            &style,
        );
        assert!(
            overlays
                .iter()
                .any(|overlay| matches!(overlay, MapOverlay::FilledPolygon(_)))
        );
        assert!(
            overlays
                .iter()
                .any(|overlay| matches!(overlay, MapOverlay::RoutePolyline(_)))
        );
        assert!(
            overlays
                .iter()
                .any(|overlay| matches!(overlay, MapOverlay::PointMarkers(_)))
        );
    }

    #[test]
    fn a_missing_or_malformed_artifact_falls_back_to_the_in_code_defaults() {
        assert!(vector_tile_style_from_json("").is_err());
        assert!(vector_tile_style_from_json("{not json").is_err());
        assert_eq!(
            vector_tile_style_or_fallback(""),
            in_code_default_vector_tile_style()
        );
        assert_eq!(
            vector_tile_style_or_fallback("{not json"),
            in_code_default_vector_tile_style()
        );
    }

    #[test]
    fn the_style_artifacts_are_provider_neutral_data() {
        for artifact_json in [
            DEFAULT_VECTOR_STYLE_ARTIFACT_JSON,
            CARTO_DARK_STYLE_ARTIFACT_JSON,
            OPENMAPTILES_LIGHT_STYLE_ARTIFACT_JSON,
            OPENMAPTILES_DARK_STYLE_ARTIFACT_JSON,
            BRIGHT_LIGHT_STYLE_ARTIFACT_JSON,
            FIORD_DARK_STYLE_ARTIFACT_JSON,
            POSITRON_LIGHT_STYLE_ARTIFACT_JSON,
        ] {
            let artifact_text = artifact_json.to_lowercase();
            for forbidden_fragment in ["http", "mapquest", "openstreetmap", "url"] {
                assert!(
                    !artifact_text.contains(forbidden_fragment),
                    "the artifact must stay data-only and provider-neutral: {forbidden_fragment}"
                );
            }
        }
    }

    #[test]
    fn an_artifact_layer_carries_its_filter_and_kind_specific_paint() {
        let artifact_json = r#"{
            "layers": [
                {
                    "source_layer": "landuse",
                    "geometry_kind": "fill",
                    "rgba": [0.1, 0.2, 0.3, 0.4],
                    "filter": { "attribute_name": "class", "expected_text": "park" }
                }
            ]
        }"#;
        let style = vector_tile_style_from_json(artifact_json).expect("the ad-hoc artifact parses");
        assert_eq!(style.layers.len(), 1);
        let layer_style = &style.layers[0];
        assert_eq!(layer_style.source_layer, "landuse");
        assert_eq!(layer_style.geometry_kind, VectorStyleGeometryKind::Fill);
        assert_eq!(
            layer_style.filter,
            Some(VectorAttributeFilter {
                attribute_name: "class".to_string(),
                expected_text: "park".to_string(),
                matches_any: Vec::new(),
                number_equals: None,
            })
        );
        assert_eq!(layer_style.fill_rgba, [0.1, 0.2, 0.3, 0.4]);
    }

    #[test]
    fn an_artifact_symbol_layer_carries_its_label_style() {
        let artifact_json = r#"{
            "layers": [
                {
                    "source_layer": "place",
                    "geometry_kind": "symbol",
                    "text_field_fallbacks": ["name:latin", "name", "ref"],
                    "font_stack": ["Noto Sans Regular", "Noto Sans Bold"],
                    "text_rgba": [0.1, 0.2, 0.3, 1.0],
                    "text_size_pixels": 12.0,
                    "text_size_zoom_stops": [
                        {"zoom": 14, "size_pixels": 16.0},
                        {"zoom": 10, "size_pixels": 11.0}
                    ],
                    "halo_rgba": [1.0, 1.0, 1.0, 0.8],
                    "halo_width_pixels": 1.5,
                    "min_zoom": 8,
                    "max_zoom": 20,
                    "placement": "line",
                    "priority": 3,
                    "allow_overlap": true,
                    "rank_zoom_stops": [
                        {"zoom": 16, "maximum_rank": 20},
                        {"zoom": 12, "maximum_rank": 4}
                    ],
                    "class_minimum_zooms": [
                        {"classes": ["shop", "cafe"], "min_zoom": 16}
                    ]
                }
            ]
        }"#;
        let style = vector_tile_style_from_json(artifact_json).expect("the symbol artifact parses");
        let layer_style = &style.layers[0];
        assert_eq!(layer_style.geometry_kind, VectorStyleGeometryKind::Symbol);
        assert_eq!(
            layer_style.text_field_fallbacks,
            vec![
                "name:latin".to_string(),
                "name".to_string(),
                "ref".to_string()
            ]
        );
        assert_eq!(
            layer_style.font_stack,
            vec![
                "Noto Sans Regular".to_string(),
                "Noto Sans Bold".to_string()
            ]
        );
        assert_eq!(layer_style.text_rgba, [0.1, 0.2, 0.3, 1.0]);
        assert_eq!(layer_style.text_size_pixels, 12.0);
        assert_eq!(
            layer_style.text_size_zoom_stops,
            vec![
                VectorTextSizeZoomStop {
                    zoom_level: 10.0,
                    size_pixels: 11.0,
                },
                VectorTextSizeZoomStop {
                    zoom_level: 14.0,
                    size_pixels: 16.0,
                },
            ],
            "size stops are kept ascending by zoom"
        );
        assert_eq!(layer_style.halo_rgba, [1.0, 1.0, 1.0, 0.8]);
        assert_eq!(layer_style.halo_width_pixels, 1.5);
        assert_eq!(layer_style.min_zoom_level, 8.0);
        assert_eq!(layer_style.max_zoom_level, 20.0);
        assert_eq!(layer_style.placement, VectorSymbolPlacement::Line);
        assert_eq!(layer_style.priority, 3);
        assert!(layer_style.allow_overlap);
        assert_eq!(
            layer_style.rank_zoom_stops,
            vec![
                VectorRankZoomStop {
                    zoom_level: 12.0,
                    maximum_rank: 4,
                },
                VectorRankZoomStop {
                    zoom_level: 16.0,
                    maximum_rank: 20,
                },
            ],
            "rank stops are kept ascending by zoom"
        );
        assert_eq!(
            layer_style.class_minimum_zooms,
            vec![VectorClassMinimumZoom {
                classes: vec!["shop".to_string(), "cafe".to_string()],
                minimum_zoom_level: 16.0,
            }]
        );
    }

    #[test]
    fn a_symbol_layer_defaults_its_omitted_label_fields() {
        let artifact_json = r#"{
            "layers": [
                {"source_layer": "place", "geometry_kind": "symbol"}
            ]
        }"#;
        let style = vector_tile_style_from_json(artifact_json).expect("the minimal symbol parses");
        let layer_style = &style.layers[0];
        assert_eq!(
            layer_style.text_field_fallbacks,
            DEFAULT_VECTOR_SYMBOL_TEXT_FIELD_FALLBACKS
                .iter()
                .map(|attribute_name| attribute_name.to_string())
                .collect::<Vec<String>>()
        );
        assert_eq!(
            layer_style.font_stack,
            DEFAULT_VECTOR_SYMBOL_FONT_STACK
                .iter()
                .map(|font_family| font_family.to_string())
                .collect::<Vec<String>>()
        );
        assert_eq!(layer_style.text_rgba, DEFAULT_VECTOR_SYMBOL_TEXT_RGBA);
        assert_eq!(
            layer_style.text_size_pixels,
            DEFAULT_VECTOR_SYMBOL_TEXT_SIZE_PIXELS
        );
        assert!(layer_style.text_size_zoom_stops.is_empty());
        assert_eq!(layer_style.halo_rgba, DEFAULT_VECTOR_SYMBOL_HALO_RGBA);
        assert_eq!(
            layer_style.halo_width_pixels,
            DEFAULT_VECTOR_SYMBOL_HALO_WIDTH_PIXELS
        );
        assert_eq!(layer_style.placement, DEFAULT_VECTOR_SYMBOL_PLACEMENT);
        assert_eq!(layer_style.priority, DEFAULT_VECTOR_SYMBOL_PRIORITY);
        assert_eq!(
            layer_style.allow_overlap,
            DEFAULT_VECTOR_SYMBOL_ALLOW_OVERLAP
        );
        assert!(layer_style.rank_zoom_stops.is_empty());
        assert!(layer_style.class_minimum_zooms.is_empty());
    }

    #[test]
    fn symbol_text_size_stops_interpolate_at_the_tile_zoom() {
        let symbol_layer = VectorLayerStyle::symbol("place", [0.0, 0.0, 0.0, 1.0], 12.0)
            .with_text_size_zoom_stop(10.0, 10.0)
            .with_text_size_zoom_stop(14.0, 16.0);
        assert_eq!(symbol_layer.text_size_pixels_at_zoom(6.0), 10.0);
        assert_eq!(symbol_layer.text_size_pixels_at_zoom(10.0), 10.0);
        assert_eq!(symbol_layer.text_size_pixels_at_zoom(12.0), 13.0);
        assert_eq!(symbol_layer.text_size_pixels_at_zoom(14.0), 16.0);
        assert_eq!(symbol_layer.text_size_pixels_at_zoom(18.0), 16.0);
        let fixed_size_layer = VectorLayerStyle::symbol("place", [0.0, 0.0, 0.0, 1.0], 9.0);
        assert_eq!(fixed_size_layer.text_size_pixels_at_zoom(12.0), 9.0);
    }

    #[test]
    fn symbol_rank_ceiling_stops_step_at_the_tile_zoom() {
        let symbol_layer = VectorLayerStyle::symbol("poi", [0.0, 0.0, 0.0, 1.0], 12.0)
            .with_rank_zoom_stop(14.0, 6)
            .with_rank_zoom_stop(16.0, 16)
            .with_rank_zoom_stop(18.0, DEFAULT_VECTOR_SYMBOL_MAXIMUM_RANK);
        assert_eq!(symbol_layer.maximum_rank_at_zoom(8.0), 6);
        assert_eq!(symbol_layer.maximum_rank_at_zoom(14.0), 6);
        assert_eq!(
            symbol_layer.maximum_rank_at_zoom(15.9),
            6,
            "rank is ordinal, so a stop holds until the next one"
        );
        assert_eq!(symbol_layer.maximum_rank_at_zoom(16.0), 16);
        assert_eq!(symbol_layer.maximum_rank_at_zoom(18.0), u32::MAX);
        let unlimited_layer = VectorLayerStyle::symbol("poi", [0.0, 0.0, 0.0, 1.0], 12.0);
        assert_eq!(
            unlimited_layer.maximum_rank_at_zoom(14.0),
            DEFAULT_VECTOR_SYMBOL_MAXIMUM_RANK
        );
    }

    #[test]
    fn symbol_class_minimum_zooms_gate_only_the_classes_they_name() {
        let symbol_layer = VectorLayerStyle::symbol("poi", [0.0, 0.0, 0.0, 1.0], 12.0)
            .with_class_minimum_zoom(["shop", "cafe"], 16.0)
            .with_class_minimum_zoom(["parking"], 17.0);
        assert!(!symbol_layer.is_feature_class_visible_at_zoom(Some("shop"), 14.0));
        assert!(symbol_layer.is_feature_class_visible_at_zoom(Some("shop"), 16.0));
        assert!(!symbol_layer.is_feature_class_visible_at_zoom(Some("parking"), 16.0));
        assert!(symbol_layer.is_feature_class_visible_at_zoom(Some("parking"), 17.0));
        assert!(
            symbol_layer.is_feature_class_visible_at_zoom(Some("museum"), 14.0),
            "a class no group names follows the layer's own zoom span"
        );
        assert!(symbol_layer.is_feature_class_visible_at_zoom(None, 14.0));
    }

    #[test]
    fn a_class_named_by_two_groups_takes_the_higher_minimum_zoom() {
        let symbol_layer = VectorLayerStyle::symbol("poi", [0.0, 0.0, 0.0, 1.0], 12.0)
            .with_class_minimum_zoom(["shop"], 15.0)
            .with_class_minimum_zoom(["shop"], 17.0);
        assert!(!symbol_layer.is_feature_class_visible_at_zoom(Some("shop"), 16.0));
        assert!(symbol_layer.is_feature_class_visible_at_zoom(Some("shop"), 17.0));
    }

    #[test]
    fn the_named_styles_hold_shops_back_until_zoom_sixteen() {
        let shop_classes = ["shop", "restaurant", "cafe", "fast_food", "bar"];
        for style_id in [
            VECTOR_STYLE_ID_BRIGHT_LIGHT,
            VECTOR_STYLE_ID_FIORD_DARK,
            VECTOR_STYLE_ID_POSITRON_LIGHT,
        ] {
            let style = vector_tile_style_from_id(style_id)
                .unwrap_or_else(|error| panic!("the committed {style_id} style loads: {error}"));
            let poi_layer = style
                .layers
                .iter()
                .find(|layer_style| {
                    layer_style.geometry_kind == VectorStyleGeometryKind::Symbol
                        && layer_style.source_layer == "poi"
                })
                .unwrap_or_else(|| panic!("{style_id} declares a poi symbol layer"));
            for shop_class in shop_classes {
                assert!(
                    !poi_layer.is_feature_class_visible_at_zoom(Some(shop_class), 14.0),
                    "{style_id} must keep {shop_class:?} labels out of the z14 demo"
                );
                assert!(
                    poi_layer.is_feature_class_visible_at_zoom(Some(shop_class), 16.0),
                    "{style_id} must draw {shop_class:?} labels once the map is close enough"
                );
            }
            assert_eq!(
                poi_layer.maximum_rank_at_zoom(14.0),
                6,
                "{style_id} must draw only the important places at the demo zoom"
            );
            assert!(
                poi_layer.is_feature_class_visible_at_zoom(Some("museum"), 14.0),
                "{style_id} keeps landmark classes at the demo zoom"
            );
        }
    }

    #[test]
    fn a_symbol_layer_declares_labels_but_paints_no_overlay() {
        let tile = decode_vector_tile(SOURDOUGH_TILE_BYTES).unwrap();
        let style = VectorTileStyle {
            background: None,
            layers: vec![VectorLayerStyle::symbol(
                "places",
                [0.1, 0.2, 0.3, 1.0],
                12.0,
            )],
        };
        assert!(
            vector_tile_overlays(&tile, SOURDOUGH_TILE_ADDRESS, &style).is_empty(),
            "a symbol layer is label data; the label pass paints it later"
        );
        assert!(
            overlay_primitives_for_vector_tile(&tile, SOURDOUGH_TILE_ADDRESS, &style).is_empty()
        );
    }

    #[test]
    fn upstream_maplibre_symbol_json_is_still_rejected() {
        let upstream_maplibre_json = r#"{
            "layers": [
                {
                    "source_layer": "place",
                    "geometry_kind": "symbol",
                    "layout": {"text-field": "{name:latin}"}
                }
            ]
        }"#;
        assert!(
            vector_tile_style_from_json(upstream_maplibre_json).is_err(),
            "deny_unknown_fields keeps the upstream expression surface out"
        );
    }

    #[test]
    fn the_named_styles_declare_their_symbol_layers() {
        let carto_light = vector_tile_style_from_id(VECTOR_STYLE_ID_CARTO_LIGHT)
            .expect("the carto light artifact parses");
        assert!(carto_light.layers.iter().any(|layer_style| {
            layer_style.geometry_kind == VectorStyleGeometryKind::Symbol
                && layer_style.source_layer == "places"
        }));
        for style_id in [
            VECTOR_STYLE_ID_BRIGHT_LIGHT,
            VECTOR_STYLE_ID_FIORD_DARK,
            VECTOR_STYLE_ID_POSITRON_LIGHT,
        ] {
            let style = vector_tile_style_from_id(style_id)
                .unwrap_or_else(|style_error| panic!("{style_id}: {style_error}"));
            for label_source_layer in ["place", "poi"] {
                let symbol_layer = style
                    .layers
                    .iter()
                    .find(|layer_style| {
                        layer_style.geometry_kind == VectorStyleGeometryKind::Symbol
                            && layer_style.source_layer == label_source_layer
                    })
                    .unwrap_or_else(|| {
                        panic!("{style_id} declares no {label_source_layer} symbol layer")
                    });
                assert_eq!(
                    symbol_layer.text_field_fallbacks,
                    vec!["name:latin".to_string(), "name".to_string()],
                    "{style_id} tries the Latin name before the local name"
                );
                assert!(!symbol_layer.font_stack.is_empty());
                assert!(!symbol_layer.text_size_zoom_stops.is_empty());
                assert!(symbol_layer.halo_width_pixels > 0.0);
                assert!(symbol_layer.max_zoom_level > symbol_layer.min_zoom_level);
            }
        }
    }

    #[test]
    fn the_named_styles_declare_a_line_placed_road_name_band_per_class() {
        // Each named style labels the roads it paints: one line-placement
        // symbol layer per road class it distinguishes, with the name → ref
        // fallbacks, a halo over the road paint and a size that steps down
        // the road hierarchy.
        for style_id in [
            VECTOR_STYLE_ID_BRIGHT_LIGHT,
            VECTOR_STYLE_ID_FIORD_DARK,
            VECTOR_STYLE_ID_POSITRON_LIGHT,
        ] {
            let style = vector_tile_style_from_id(style_id)
                .unwrap_or_else(|style_error| panic!("{style_id}: {style_error}"));
            let road_name_layers: Vec<&VectorLayerStyle> = style
                .layers
                .iter()
                .filter(|layer_style| {
                    layer_style.geometry_kind == VectorStyleGeometryKind::Symbol
                        && layer_style.source_layer == "transportation_name"
                        && layer_style.placement == VectorSymbolPlacement::Line
                })
                .collect();
            assert!(
                !road_name_layers.is_empty(),
                "{style_id} declares no line-placed transportation_name symbol layer"
            );
            for layer_style in &road_name_layers {
                assert_eq!(
                    layer_style.text_field_fallbacks,
                    vec!["name:latin".to_string(), "name".to_string()],
                    "{style_id} paints a road name, never a route reference, as text"
                );
                assert!(layer_style.halo_width_pixels > 0.0);
                assert!(layer_style.text_size_pixels_at_zoom(PROBE_ZOOM_LEVEL) > 0.0);
                assert!(
                    layer_style.is_visible_at_zoom(PROBE_ZOOM_LEVEL),
                    "{style_id} road names paint at the demo zoom"
                );
                assert!(
                    layer_style.priority > PLACE_SYMBOL_PRIORITY,
                    "{style_id} city names outrank road names"
                );
                assert!(
                    layer_style.filter.is_some(),
                    "{style_id} road name layers are per-class bands"
                );
            }
            for road_class in ["motorway", "primary", "secondary", "tertiary", "minor"] {
                assert!(
                    road_name_layers.iter().any(|layer_style| {
                        layer_style.filter.as_ref().is_some_and(|attribute_filter| {
                            attribute_filter.attribute_name == "class"
                                && attribute_filter.expected_text == road_class
                        })
                    }),
                    "{style_id} labels the {road_class} class"
                );
            }
            // The bands really differ: the motorway name is larger than the
            // minor-road name at the demo zoom.
            let size_for_class = |road_class: &str| {
                road_name_layers
                    .iter()
                    .find(|layer_style| {
                        layer_style.filter.as_ref().is_some_and(|attribute_filter| {
                            attribute_filter.expected_text == road_class
                        })
                    })
                    .map(|layer_style| layer_style.text_size_pixels_at_zoom(PROBE_ZOOM_LEVEL))
                    .unwrap_or_default()
            };
            assert!(
                size_for_class("motorway") > size_for_class("primary")
                    && size_for_class("primary") >= size_for_class("secondary")
                    && size_for_class("secondary") >= size_for_class("tertiary")
                    && size_for_class("tertiary") >= size_for_class("minor"),
                "{style_id} road name sizes step down the road hierarchy"
            );
        }
    }

    /// The zoom the road-name bands are compared at: the demo camera's zoom,
    /// where every built-in style shows its roads.
    const PROBE_ZOOM_LEVEL: f64 = 14.0;
    /// The priority the named styles' city-name symbol layers carry; road
    /// names must draw after them.
    const PLACE_SYMBOL_PRIORITY: u32 = 1;

    /// Tolerance of the hole containment check against the exterior boundary:
    /// real tile data can emit a hole that touches its exterior at a vertex
    /// (common at coastlines), which is legal even-odd fill but not strictly
    /// interior. Degrees at tile scale: well below a millimetre.
    const RING_BOUNDARY_TOLERANCE_DEGREES: f64 = 1e-9;

    /// Whether a coordinate lies inside a polygon ring or on its boundary
    /// (even-odd ray cast in longitude/latitude, plus a boundary tolerance).
    fn point_lies_within_or_on_ring(
        point: &GeographicCoordinate,
        ring_coordinates: &[GeographicCoordinate],
    ) -> bool {
        point_lies_within_ring(point, ring_coordinates)
            || ring_coordinates
                .iter()
                .enumerate()
                .any(|(index, edge_start)| {
                    let edge_end = &ring_coordinates[(index + 1) % ring_coordinates.len()];
                    let edge_longitude = edge_end.longitude - edge_start.longitude;
                    let edge_latitude = edge_end.latitude - edge_start.latitude;
                    let edge_length_squared =
                        edge_longitude * edge_longitude + edge_latitude * edge_latitude;
                    let offset_longitude = point.longitude - edge_start.longitude;
                    let offset_latitude = point.latitude - edge_start.latitude;
                    let projection = if edge_length_squared == 0.0 {
                        0.0
                    } else {
                        ((offset_longitude * edge_longitude + offset_latitude * edge_latitude)
                            / edge_length_squared)
                            .clamp(0.0, 1.0)
                    };
                    let closest_longitude = edge_start.longitude + projection * edge_longitude;
                    let closest_latitude = edge_start.latitude + projection * edge_latitude;
                    (point.longitude - closest_longitude).hypot(point.latitude - closest_latitude)
                        <= RING_BOUNDARY_TOLERANCE_DEGREES
                })
    }

    /// Whether a coordinate lies inside a polygon ring (even-odd ray cast in
    /// longitude/latitude).
    fn point_lies_within_ring(
        point: &GeographicCoordinate,
        ring_coordinates: &[GeographicCoordinate],
    ) -> bool {
        let mut inside = false;
        let mut previous_index = ring_coordinates.len().saturating_sub(1);
        for current_index in 0..ring_coordinates.len() {
            let current = ring_coordinates[current_index];
            let previous = ring_coordinates[previous_index];
            let crosses_parallel =
                (current.latitude > point.latitude) != (previous.latitude > point.latitude);
            if crosses_parallel {
                let boundary_longitude = (previous.longitude - current.longitude)
                    * (point.latitude - current.latitude)
                    / (previous.latitude - current.latitude)
                    + current.longitude;
                if point.longitude < boundary_longitude {
                    inside = !inside;
                }
            }
            previous_index = current_index;
        }
        inside
    }

    /// MVT producers buffer geometry beyond the nominal extent so features
    /// do not clip at the tile boundary, and label layers (`places`) anchor
    /// their points well past the edge for placement. This sample tile's
    /// farthest
    /// anchor is about a quarter-tile out; the check allows a third of a tile.
    const TILE_BUFFER_TOLERANCE_FRACTION: f64 = 0.34;

    fn assert_tile_bounds_contain(coordinate: &GeographicCoordinate) {
        let north_west = tile_local_point_to_coordinate(
            SOURDOUGH_TILE_ADDRESS,
            4096,
            &VectorTileGeometryPoint {
                x_coordinate: 0,
                y_coordinate: 0,
            },
        );
        let south_east = tile_local_point_to_coordinate(
            SOURDOUGH_TILE_ADDRESS,
            4096,
            &VectorTileGeometryPoint {
                x_coordinate: 4096,
                y_coordinate: 4096,
            },
        );
        let longitude_margin =
            (south_east.longitude - north_west.longitude) * TILE_BUFFER_TOLERANCE_FRACTION;
        let latitude_margin =
            (north_west.latitude - south_east.latitude) * TILE_BUFFER_TOLERANCE_FRACTION;
        assert!(
            coordinate.longitude >= north_west.longitude - longitude_margin,
            "longitude {} outside [{}, {}] (margin {longitude_margin})",
            coordinate.longitude,
            north_west.longitude,
            south_east.longitude
        );
        assert!(
            coordinate.longitude <= south_east.longitude + longitude_margin,
            "longitude {} outside [{}, {}] (margin {longitude_margin})",
            coordinate.longitude,
            north_west.longitude,
            south_east.longitude
        );
        assert!(
            coordinate.latitude <= north_west.latitude + latitude_margin,
            "latitude {} outside [{}, {}] (margin {latitude_margin})",
            coordinate.latitude,
            south_east.latitude,
            north_west.latitude
        );
        assert!(
            coordinate.latitude >= south_east.latitude - latitude_margin,
            "latitude {} outside [{}, {}] (margin {latitude_margin})",
            coordinate.latitude,
            south_east.latitude,
            north_west.latitude
        );
    }
}
