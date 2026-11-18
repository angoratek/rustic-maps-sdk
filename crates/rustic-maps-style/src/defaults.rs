//! Documented property defaults from the MapLibre style specification.
//!
//! Defaults live here rather than as literals in the evaluator so a property a
//! style omits and a property that fails at runtime resolve to the same
//! documented value, and so a reader can audit them against the specification
//! in one place.

use crate::color::Color;
use crate::types::{
    CirclePitchAlignment, CirclePitchScale, IconTextFit, LineCap, LineJoin, ShieldShape,
    SymbolPlacement, TextAnchor, TextJustify, TextPitchAlignment, TextRotationAlignment,
    TextTransform, TranslateAnchor, Visibility,
};

/// The only style-document version this crate accepts.
pub const STYLE_DOCUMENT_VERSION: u8 = 8;

/// The lowest zoom a layer draws at when it omits `minzoom`.
pub const DEFAULT_MINIMUM_ZOOM: f64 = 0.0;

/// The zoom a layer stops drawing at when it omits `maxzoom`.
pub const DEFAULT_MAXIMUM_ZOOM: f64 = 24.0;

/// The visibility a layer has when it omits `layout.visibility`.
pub const DEFAULT_VISIBILITY: Visibility = Visibility::Visible;

/// The default `background-color`.
pub const DEFAULT_BACKGROUND_COLOR: Color = Color::BLACK;

/// The default `background-opacity`.
pub const DEFAULT_BACKGROUND_OPACITY: f64 = 1.0;

/// The default `fill-color`.
pub const DEFAULT_FILL_COLOR: Color = Color::BLACK;

/// The default `fill-opacity`.
pub const DEFAULT_FILL_OPACITY: f64 = 1.0;

/// The default `fill-antialias`: fills keep their softened edge.
pub const DEFAULT_FILL_ANTIALIAS: bool = true;

/// The default `fill-translate`, in screen pixels.
pub const DEFAULT_FILL_TRANSLATE: [f64; 2] = [0.0, 0.0];

/// The default `fill-translate-anchor`: the displacement is measured relative
/// to the map, so it rotates with the camera's bearing.
pub const DEFAULT_FILL_TRANSLATE_ANCHOR: TranslateAnchor = TranslateAnchor::Map;

/// The default `circle-stroke-width`, in screen pixels: no stroke.
pub const DEFAULT_CIRCLE_STROKE_WIDTH: f64 = 0.0;

/// The default `circle-stroke-opacity`.
pub const DEFAULT_CIRCLE_STROKE_OPACITY: f64 = 1.0;

/// The default `circle-blur`: a hard edge.
pub const DEFAULT_CIRCLE_BLUR: f64 = 0.0;

/// The default `circle-translate`, in screen pixels.
pub const DEFAULT_CIRCLE_TRANSLATE: [f64; 2] = [0.0, 0.0];

/// The default `circle-pitch-scale`: circles lie on the ground.
pub const DEFAULT_CIRCLE_PITCH_SCALE: CirclePitchScale = CirclePitchScale::Map;

/// The default `circle-pitch-alignment`: circles face the screen.
pub const DEFAULT_CIRCLE_PITCH_ALIGNMENT: CirclePitchAlignment = CirclePitchAlignment::Viewport;

/// The default `line-color`.
pub const DEFAULT_LINE_COLOR: Color = Color::BLACK;

/// The default `line-width`, in screen pixels.
pub const DEFAULT_LINE_WIDTH: f64 = 1.0;

/// The default `line-opacity`.
pub const DEFAULT_LINE_OPACITY: f64 = 1.0;

/// The default `line-blur`, in screen pixels.
pub const DEFAULT_LINE_BLUR: f64 = 0.0;

/// The default `line-cap`.
pub const DEFAULT_LINE_CAP: LineCap = LineCap::Butt;

/// The default `line-join`.
pub const DEFAULT_LINE_JOIN: LineJoin = LineJoin::Miter;

/// The default `line-offset`, in screen pixels.
pub const DEFAULT_LINE_OFFSET: f64 = 0.0;

/// The default `line-gap-width`, in screen pixels.
pub const DEFAULT_LINE_GAP_WIDTH: f64 = 0.0;

/// The default `line-translate`, in screen pixels.
pub const DEFAULT_LINE_TRANSLATE: [f64; 2] = [0.0, 0.0];

/// The default `line-translate-anchor`: the displacement is measured relative
/// to the map, so it rotates with the camera's bearing.
pub const DEFAULT_LINE_TRANSLATE_ANCHOR: TranslateAnchor = TranslateAnchor::Map;

/// The default `line-sort-key`; a feature that omits the key sorts as zero,
/// so features without a declared order keep their source order.
pub const DEFAULT_LINE_SORT_KEY: f64 = 0.0;

/// The default `fill-sort-key`; a feature that omits the key sorts as zero,
/// so features without a declared order keep their source order.
pub const DEFAULT_FILL_SORT_KEY: f64 = 0.0;

/// The default `circle-sort-key`; a feature that omits the key sorts as zero,
/// so features without a declared order keep their source order.
pub const DEFAULT_CIRCLE_SORT_KEY: f64 = 0.0;

/// The `duration` a root `transition` block uses when it omits the member,
/// in milliseconds, as the specification documents.
pub const DEFAULT_TRANSITION_DURATION_MILLISECONDS: f64 = 300.0;

/// The `delay` a `transition` block uses when it omits the member, in
/// milliseconds.
pub const DEFAULT_TRANSITION_DELAY_MILLISECONDS: f64 = 0.0;

/// The default `circle-radius`, in screen pixels.
pub const DEFAULT_CIRCLE_RADIUS: f64 = 5.0;

/// The default `circle-color`.
pub const DEFAULT_CIRCLE_COLOR: Color = Color::BLACK;

/// The default `circle-opacity`.
pub const DEFAULT_CIRCLE_OPACITY: f64 = 1.0;

/// The default `text-size`, in screen pixels.
pub const DEFAULT_TEXT_SIZE: f64 = 16.0;

/// The default `text-max-width`, in ems.
pub const DEFAULT_TEXT_MAX_WIDTH: f64 = 10.0;

/// The default `text-color`.
pub const DEFAULT_TEXT_COLOR: Color = Color::BLACK;

/// The default `text-opacity`.
pub const DEFAULT_TEXT_OPACITY: f64 = 1.0;

/// The default `text-halo-color`.
pub const DEFAULT_TEXT_HALO_COLOR: Color = Color::TRANSPARENT;

/// The default `text-halo-width`, in screen pixels.
pub const DEFAULT_TEXT_HALO_WIDTH: f64 = 0.0;

/// The default `text-halo-blur`, in screen pixels.
pub const DEFAULT_TEXT_HALO_BLUR: f64 = 0.0;

/// The default `symbol-placement`.
pub const DEFAULT_SYMBOL_PLACEMENT: SymbolPlacement = SymbolPlacement::Point;

/// The default `symbol-spacing`, in screen pixels.
pub const DEFAULT_SYMBOL_SPACING: f64 = 250.0;

/// The default `text-allow-overlap`: text collides and is dropped when it
/// cannot be placed.
pub const DEFAULT_TEXT_ALLOW_OVERLAP: bool = false;

/// The default `text-ignore-placement`: text takes part in collision
/// placement.
pub const DEFAULT_TEXT_IGNORE_PLACEMENT: bool = false;

/// The default `symbol-avoid-edges`: a label may cross the viewport edge.
pub const DEFAULT_SYMBOL_AVOID_EDGES: bool = false;

/// The default `text-offset`, `[x, y]` in ems.
pub const DEFAULT_TEXT_OFFSET: [f64; 2] = [0.0, 0.0];

/// The default `text-padding`, in screen pixels: the transparent margin kept
/// around a label's glyphs for collision placement.
pub const DEFAULT_TEXT_PADDING: f64 = 2.0;

/// The default `text-pitch-alignment`: the placement decides, so a
/// point-placed label faces the screen and a line-placed label lies on the
/// ground.
pub const DEFAULT_TEXT_PITCH_ALIGNMENT: TextPitchAlignment = TextPitchAlignment::Auto;

/// The placement order key a symbol layer uses when it omits
/// `symbol-sort-key`; the renderer treats a missing key as this value.
pub const DEFAULT_SYMBOL_SORT_KEY: f64 = 0.0;

/// The default `text-line-height`, in ems.
pub const DEFAULT_TEXT_LINE_HEIGHT: f64 = 1.2;

/// The default `text-letter-spacing`, in ems.
pub const DEFAULT_TEXT_LETTER_SPACING: f64 = 0.0;

/// The default `text-justify`.
pub const DEFAULT_TEXT_JUSTIFY: TextJustify = TextJustify::Auto;

/// The default `text-anchor`.
pub const DEFAULT_TEXT_ANCHOR: TextAnchor = TextAnchor::Center;

/// The default `text-rotation-alignment`.
pub const DEFAULT_TEXT_ROTATION_ALIGNMENT: TextRotationAlignment = TextRotationAlignment::Auto;

/// The default `text-keep-upright`: a rotated label flips to read upright.
pub const DEFAULT_TEXT_KEEP_UPRIGHT: bool = true;

/// The default `text-radial-offset`, in ems.
pub const DEFAULT_TEXT_RADIAL_OFFSET: f64 = 0.0;

/// The specification's default `text-max-angle`, in degrees.
pub const DEFAULT_TEXT_MAX_ANGLE: f64 = 45.0;

/// The default `text-transform`.
pub const DEFAULT_TEXT_TRANSFORM: TextTransform = TextTransform::None;

/// The base of a legacy exponential function that does not declare one.
pub const DEFAULT_LEGACY_EXPONENTIAL_BASE: f64 = 1.0;

/// The default `icon-size`: the sprite sheet's own size at a pixel ratio of
/// one.
pub const DEFAULT_ICON_SIZE: f64 = 1.0;

/// The default `icon-anchor`: the icon is centered on the symbol anchor.
pub const DEFAULT_ICON_ANCHOR: TextAnchor = TextAnchor::Center;

/// The default `icon-offset`, `[x, y]` in ems.
pub const DEFAULT_ICON_OFFSET: [f64; 2] = [0.0, 0.0];

/// The default `icon-rotate`, in degrees.
pub const DEFAULT_ICON_ROTATE: f64 = 0.0;

/// The default `icon-rotation-alignment`: `auto`, which follows the placement
/// (map for a line, viewport for a point).
pub const DEFAULT_ICON_ROTATION_ALIGNMENT: TextRotationAlignment = TextRotationAlignment::Auto;

/// The default `icon-padding`, in screen pixels.
pub const DEFAULT_ICON_PADDING: f64 = 2.0;

/// The default `icon-text-fit`: the icon keeps its own aspect ratio.
pub const DEFAULT_ICON_TEXT_FIT: IconTextFit = IconTextFit::None;

/// The default `icon-text-fit-padding`, `[top, right, bottom, left]` in screen
/// pixels: no inset between the text block and a fitted icon.
pub const DEFAULT_ICON_TEXT_FIT_PADDING: [f64; 4] = [0.0, 0.0, 0.0, 0.0];

/// The default `icon-opacity`.
pub const DEFAULT_ICON_OPACITY: f64 = 1.0;

/// The default `icon-allow-overlap`: an icon collides and is dropped when it
/// cannot be placed.
pub const DEFAULT_ICON_ALLOW_OVERLAP: bool = false;

/// The default `icon-ignore-placement`: an icon takes part in collision
/// placement.
pub const DEFAULT_ICON_IGNORE_PLACEMENT: bool = false;

/// The outline a shield block draws when it omits `shape`.
pub const DEFAULT_SHIELD_SHAPE: ShieldShape = ShieldShape::RoundedRectangle;

/// The fill a shield block uses when it omits `fill_rgba`.
pub const DEFAULT_SHIELD_FILL_COLOR: Color = Color::WHITE;

/// The outline width a shield block uses when it omits `border_width_pixels`.
pub const DEFAULT_SHIELD_BORDER_WIDTH_PIXELS: f64 = 0.0;

/// The reference colour a shield block uses when it omits `text_rgba`.
pub const DEFAULT_SHIELD_TEXT_COLOR: Color = Color::BLACK;

/// The reference size a shield block uses when it omits `text_size_pixels`.
pub const DEFAULT_SHIELD_TEXT_SIZE_PIXELS: f64 = 11.0;

/// The inset a shield block uses when it omits `padding_pixels`.
pub const DEFAULT_SHIELD_PADDING_PIXELS: f64 = 4.0;

/// The minimum badge extent a shield block uses when it omits
/// `minimum_size_pixels`.
pub const DEFAULT_SHIELD_MINIMUM_SIZE_PIXELS: f64 = 0.0;
