//! The frame-level label pass: symbol-layer text from decoded vector tiles
//! becomes screen-space glyph quads, anchored at a point for place and POI
//! names and run along a line for road names.
//!
//! The pass is deliberately GPU-free, like the overlay assembly: the native
//! headless renderer, the native surface and the browser canvas all call the
//! same functions and upload the same vertices, so the backends cannot drift
//! in where a label lands or which labels survive.
//!
//! Three stages:
//!
//! 1. [`label_candidates_for_vector_tile`] walks a decoded tile's symbol-layer
//!    features and reads each label's text through the compiled style's
//!    text-field expression. Candidates keep the style's paint, size, priority,
//!    rank, placement and overlap rule. A symbol layer's filter also gates a
//!    feature before it becomes a candidate: the converted styles encode the
//!    zoom-stepped rank ceiling and the per-class minimum zooms as `step`
//!    expressions on `["zoom"]` inside an `all` filter. A
//!    [`SymbolPlacement::Point`] layer anchors a candidate at each of the
//!    feature's points; a [`SymbolPlacement::Line`] layer carries the feature's
//!    whole line geometry so the frame can place the text along it.
//! 2. [`LabelLayer::place`] projects the candidates through the frame camera,
//!    thins them by rank at the frame zoom, sorts them by symbol class, then
//!    feature rank, then style-layer order (the documented collision-priority
//!    rule: lower priority is more important), and greedily rejects any label
//!    whose padded glyph box intersects a label already placed. Every drawn
//!    glyph contributes its own box, so a name can sit in the gap beside a
//!    neighbour's word space; a rotated line label tests its oriented boxes.
//!    `allow_overlap` labels skip the test and never block later labels. The
//!    frame places at most
//!    [`MAXIMUM_LABELS_PER_FRAME`] labels: once that budget is spent, every
//!    remaining candidate is dropped even if it would fit, so a dense downtown
//!    cannot out-shout the places that matter. A share of the budget is kept
//!    free for line-placed names ([`RESERVED_LINE_LABEL_SLOTS`]), so a frame
//!    whose ranked places would spend the whole budget still carries street
//!    names. A label whose text repeats an
//!    already-placed label is dropped, so the buffered overlap between
//!    adjacent tiles cannot paint one place twice. The output order is a pure
//!    function of the input order and the camera, unless the caller hands the
//!    previous frame's placed labels to
//!    [`LabelLayer::place_with_history`]: among candidates that tie on the
//!    priority key, a name the previous frame placed is tried first, so a
//!    panning frame holds a name on screen instead of letting an
//!    equal-priority newcomer take its space on one frame and lose it on the
//!    next. History never reorders across symbol class, rank or style-layer
//!    tier, so the frame stays a deterministic priority order.
//!
//!    A line-placed label projects its geometry and takes its longest straight
//!    run (consecutive near-collinear segments) as the text's run: the text is
//!    centered halfway along that run, rotated to its chord bearing, flipped
//!    so it always reads upright, and skipped when the run is shorter than the
//!    text plus the collision padding at each end. Each glyph's collision box
//!    takes the label's rotation, so a road name rejects with its oriented
//!    extent, not the empty corners of an axis-aligned bound.
//! 3. [`LabelLayer::quad_vertices`] lays the glyph bitmaps out on the atlas and
//!    emits clip-space textured quads, halo first and text second. A rotated
//!    label rotates each glyph's screen-space rectangle about the label's
//!    anchor before converting it to clip space, so every backend that draws
//!    these vertices carries rotation without a second vertex format.
//!
//! Latin text is measured from the decoded glyph ranges and nothing here
//! shapes complex scripts (see `rustic-maps-glyphs`). A frame with no symbol layers or
//! no glyphs produces no vertices, so the tiles-only pipelines stay
//! byte-identical.

use crate::camera::{MapCamera, ViewportPixels};
use crate::overlay::ClipSpaceOverlayVertex;
use crate::sprite::{SpriteAtlas, SpriteTextureRectangle};
use crate::vector_style::{
    VectorTileAddress, feature_evaluation_context, feature_properties, resolved_rgba,
    tile_local_point_to_coordinate,
};
use geospatial_types::GeographicCoordinate;
use rustic_maps_glyphs::{
    GLYPH_SIGNED_DISTANCE_FIELD_BORDER_PIXELS, GlyphAtlasError, GlyphFontStack, GlyphRange,
    PackedGlyph, PackedGlyphAtlas, decode_glyph_font_stack, pack_glyph_atlas,
};
use rustic_maps_style::{
    CompiledLayer, CompiledLayerKind, CompiledStyle, IconTextFit, ResolvedPaint, ShieldShape,
    SymbolPlacement, TextAnchor, TextJustify, TextPitchAlignment, TextRotationAlignment,
    TransitionSpec,
};
use rustic_maps_vector_tiles::{VectorTile, VectorTileFeature, VectorTileGeometry};
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock};

/// The em size the glyph protocol rasterizes signed-distance-field glyphs at:
/// every glyph metric in a range payload is expressed in these 24-pixel em
/// units and scaled to the style's text size at draw time.
pub const GLYPH_FONT_EM_PIXELS: f64 = 24.0;

/// The line ascent the glyph protocol subtracts from each glyph's ink top
/// before storing it: fontnik writes `stored_top = ink_top_above_baseline -
/// ascender` at the protocol's 24-pixel em, so a renderer reconstructs the
/// baseline-relative ink top as `stored_top + ascender`. Noto Sans' ascender
/// at that size is 26 pixels (the payload's no-ink space glyph stores `-26`).
pub const GLYPH_FONT_ASCENDER_PIXELS: f64 = 26.0;

/// The transparent margin kept around a placed label's screen rectangle for
/// collision rejection, in pixels. Labels closer than this read as a single
/// run of text, so the padding is what keeps two names from touching.
pub const LABEL_COLLISION_PADDING_PIXELS: f64 = 2.0;

/// How far below the anchor a label's baseline sits, as a fraction of the
/// text size. Centering a line of text on its anchor point needs the baseline
/// below the visual center by roughly a third of the em.
pub const LABEL_BASELINE_OFFSET_FRACTION: f64 = 0.3;

/// The signed-distance value at which a glyph bitmap counts as covered.
///
/// The glyph protocol is node-fontnik's encoding,
/// `byte = 255 - (64 + 32 × distance_px)`, so the zero-distance contour — the
/// glyph outline — sits at `(256 − 64)/256 = 0.75`, not at 0.5. Thresholding at
/// 0.5 dilates every glyph by `(0.75 − 0.5) × 8 = 2` atlas pixels per side,
/// which closes counters and fattens strokes. Thresholding at 0.75 keeps the
/// outline faithful to the font.
pub const GLYPH_TEXT_SIGNED_DISTANCE_THRESHOLD: f32 = 0.75;

/// The glyph protocol's spread: one atlas pixel of distance is `1/8` of a
/// normalized distance-field unit. It converts atlas pixels into threshold
/// offsets.
pub const GLYPH_SIGNED_DISTANCE_FIELD_PIXELS_PER_UNIT: f32 = 8.0;

/// The glyph protocol's border, in atlas pixels: the distance field fades to
/// zero beyond this many pixels outside the ink box, so a halo can only reach
/// this far before it samples the border texel.
pub const GLYPH_SIGNED_DISTANCE_FIELD_MAXIMUM_OUTWARD_PIXELS: f64 =
    GLYPH_SIGNED_DISTANCE_FIELD_BORDER_PIXELS as f64;

/// The half-width of the glyph shader's anti-aliasing transition, in device
/// pixels: the fragment stage's `smoothstep` half-band is
/// `0.105 / devicePixelRatio / fontScale` in normalized field units, which is
/// 0.84 device pixels at every size and pixel ratio.
pub const GLYPH_EDGE_GAMMA_DEVICE_PIXELS: f32 = 0.105;

/// The signed-distance threshold that draws a glyph's outline shifted outward
/// by `halo_width_pixels`, clamped to the field's border.
///
/// `halo_width / fontScale` is the halo in atlas pixels, and one atlas pixel
/// is `1 / 8` field unit, so the threshold moves down by that amount. The halo
/// quad itself is not inflated: the fragment stage's lower threshold is what
/// spreads the edge.
pub fn glyph_halo_signed_distance_threshold(text_size_pixels: f64, halo_width_pixels: f64) -> f32 {
    if halo_width_pixels <= 0.0 || text_size_pixels <= 0.0 {
        return GLYPH_TEXT_SIGNED_DISTANCE_THRESHOLD;
    }
    let font_scale = text_size_pixels / GLYPH_FONT_EM_PIXELS;
    let halo_atlas_pixels = halo_width_pixels / font_scale;
    let halo_units = (halo_atlas_pixels as f32)
        .min(GLYPH_SIGNED_DISTANCE_FIELD_MAXIMUM_OUTWARD_PIXELS as f32)
        / GLYPH_SIGNED_DISTANCE_FIELD_PIXELS_PER_UNIT;
    GLYPH_TEXT_SIGNED_DISTANCE_THRESHOLD - halo_units
}

/// The fragment stage's smoothstep half-band for one label, in normalized
/// distance-field units: `0.105 / devicePixelRatio / fontScale`, where
/// `fontScale = text_size_pixels / 24` (the style size, in CSS or point
/// pixels, against the atlas' 24-pixel em). The drawn text size in device
/// pixels is the same number times the device pixel ratio; the two uses keep
/// the band at 0.84 device pixels at every size and ratio.
pub fn glyph_edge_gamma(text_size_pixels: f64, device_pixel_ratio: f64) -> f32 {
    if text_size_pixels <= 0.0 || device_pixel_ratio <= 0.0 {
        return 0.0;
    }
    let font_scale = text_size_pixels / GLYPH_FONT_EM_PIXELS;
    (f64::from(GLYPH_EDGE_GAMMA_DEVICE_PIXELS) / device_pixel_ratio / font_scale) as f32
}

/// The feature attribute a symbol layer reads as its collision rank.
pub const LABEL_RANK_ATTRIBUTE_NAME: &str = "rank";

/// The feature attribute a symbol layer reads as its class (`shop`,
/// `restaurant`, `city`, …), which a style can hold back until a closer zoom.
pub const LABEL_CLASS_ATTRIBUTE_NAME: &str = "class";

/// The maximum number of labels one frame may place. The collision pass alone
/// lets a dense downtown paint far more text than a reader can parse, so the
/// frame stops at this budget after the priority sort: the most important
/// names always win, and a busy tile can only spend the frame's remaining
/// room. Thirty-two labels is about one per 8,200 screen pixels at the 512-
/// pixel label golden (whose filtered collision pass places ten), so it leaves
/// a full-screen demo headroom while capping the nine-tile downtown demo.
pub const MAXIMUM_LABELS_PER_FRAME: usize = 32;

/// The frame label slots kept free for line-placed names (road names and any
/// other text that runs along a line), so strict class-then-rank ordering
/// cannot let a dense set of ranked places spend the whole budget and leave
/// the streets nameless.
///
/// Eight slots are a quarter of [`MAXIMUM_LABELS_PER_FRAME`]: room for the
/// streets a dense frame can legibly name beside its places, while places
/// still take the remaining three quarters. The reservation is capped by the
/// number of line-placed candidates the frame actually offers, so a frame
/// with fewer roads (or none) gives the unused slots back to the places
/// instead of under-filling the budget.
pub const RESERVED_LINE_LABEL_SLOTS: usize = 8;

/// The rank an unranked label carries: the highest value, so it sorts after
/// every ranked label of the same style layer. An unranked label is exempt
/// from regional thinning: road names and shields carry no `rank` attribute
/// and stay on the map at every zoom.
pub const DEFAULT_LABEL_RANK: u32 = u32::MAX;

/// One regional thinning band: a ranked label survives at a frame zoom below
/// `maximum_zoom_level` only when its rank is at most `maximum_rank`.
///
/// Bands are read in order and the first one whose `maximum_zoom_level` is
/// greater than the frame zoom applies; a zoom at or above the last band's
/// ceiling places every ranked label. The band edges are the zooms the
/// reference engines change their place-density rules at, so a regional frame
/// keeps cities and the most important towns while a city frame shows the
/// neighbourhood names.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LabelRankThinningBand {
    /// The frame zoom the band stops applying at (exclusive).
    pub maximum_zoom_level: f64,
    /// The highest feature rank the band keeps.
    pub maximum_rank: u32,
}

/// The regional thinning bands, widest first.
///
/// A city name carries a low rank and survives every band; a neighbourhood or
/// a quarter carries a higher rank and appears once the camera is close enough.
/// Unranked labels are exempt, so the road names and route shields the style
/// draws by class never thin out.
pub const LABEL_REGIONAL_THINNING_BANDS: &[LabelRankThinningBand] = &[
    LabelRankThinningBand {
        maximum_zoom_level: 8.0,
        maximum_rank: 6,
    },
    LabelRankThinningBand {
        maximum_zoom_level: 10.0,
        maximum_rank: 18,
    },
    LabelRankThinningBand {
        maximum_zoom_level: 12.0,
        maximum_rank: 20,
    },
    LabelRankThinningBand {
        maximum_zoom_level: 14.0,
        maximum_rank: 22,
    },
];

/// The highest feature rank a ranked label may carry at `zoom_level` before
/// regional thinning drops it. [`DEFAULT_LABEL_RANK`] (unranked labels) is
/// always allowed.
///
/// The first [`LABEL_REGIONAL_THINNING_BANDS`] entry whose
/// `maximum_zoom_level` exceeds the frame zoom applies; a frame at or beyond
/// the last band's ceiling places every ranked label.
pub fn maximum_label_rank_at_zoom(zoom_level: f64) -> u32 {
    LABEL_REGIONAL_THINNING_BANDS
        .iter()
        .find(|band| zoom_level < band.maximum_zoom_level)
        .map_or(DEFAULT_LABEL_RANK, |band| band.maximum_rank)
}

/// Whether a candidate's rank survives regional thinning at a frame zoom. An
/// unranked candidate ([`DEFAULT_LABEL_RANK`]) always survives.
fn label_passes_regional_thinning(rank: u32, zoom_level: f64) -> bool {
    rank == DEFAULT_LABEL_RANK || rank <= maximum_label_rank_at_zoom(zoom_level)
}

/// One CSS pixel per em in `icon-offset`, the unit the specification uses for
/// a symbol's offset. The spec expresses an icon offset relative to the symbol
/// em (`text-size`'s own base of 16 pixels), not the icon's sprite size, so a
/// zoomed text layer does not move its icons.
pub const ICON_OFFSET_EM_PIXELS: f64 = 16.0;

/// The smallest icon scale the geometry honours. A non-positive or degenerate
/// `icon-size` is clamped here rather than producing a zero-area quad that
/// would sample the whole atlas through a collapsed texture window.
pub const MINIMUM_ICON_SCALE: f64 = 1.0e-6;

/// Corners one icon quad contributes.
pub const ICON_CORNERS_PER_QUAD: usize = 4;

/// Indices consumed by one icon quad's two triangles.
pub const ICON_INDICES_PER_QUAD: usize = 6;

/// The bearing limit within which a line-placed label reads upright. A
/// segment whose direction would rotate the text past this angle is flipped
/// to run the other way, so a street name never paints upside down: the
/// label's rotation always lies in `[-π/2, π/2]`.
pub const LINE_LABEL_UPRIGHT_ROTATION_LIMIT_RADIANS: f64 = std::f64::consts::FRAC_PI_2;

/// How far a segment's bearing may turn from its run's first segment and
/// still belong to the same straight run, in radians (20 degrees, a ninth of a
/// half turn). Map data splits a street into short blocks between
/// intersections, so a label must run along the straight sequence of segments,
/// not one block alone; the tolerance keeps a name off a road that bends away
/// mid-run.
pub const LINE_LABEL_STRAIGHT_RUN_ANGLE_TOLERANCE_RADIANS: f64 = std::f64::consts::PI / 9.0;

/// The gap kept between two repeated names along one long road, in pixels.
///
/// A street long enough to cross the frame is named more than once, as the
/// reference engines do: a reader then finds the name wherever they look
/// along the road instead of only at its midpoint. MapLibre's `symbol-spacing`
/// default is 250 pixels, but one frame here covers a single tile, where that
/// gap would leave nearly every road named once and the repetition invisible;
/// 90 pixels puts a name roughly every 120 pixels along a run, which is about
/// what Google Maps and HERE carry at street zooms.
pub const LINE_LABEL_REPEAT_SPACING_PIXELS: f64 = 90.0;

/// The corner radius of a rounded-rectangle badge as a fraction of the
/// badge's shorter half-extent, so a small badge keeps proportionally soft
/// corners.
pub const SHIELD_ROUNDED_RECTANGLE_CORNER_FRACTION: f64 = 0.35;

/// The arc segments a rounded-rectangle badge draws per corner. Eight
/// segments keep the outline smooth at the badge's largest size without
/// costing a meaningful number of triangles.
pub const SHIELD_ROUNDED_RECTANGLE_CORNER_SEGMENTS: usize = 8;

/// A shield badge's outline in normalized badge coordinates: `x` in
/// `-1.0..=1.0` rightward, `y` in `-1.0..=1.0` downward, scaled by the badge's
/// half-extents.
///
/// The outline is a classic two-lobed highway badge: two rounded shoulders
/// with a shallow center notch, flanks that taper inward, and a point at the
/// bottom. The points are listed clockwise from the upper-left shoulder.
pub const SHIELD_OUTLINE: [[f64; 2]; 12] = [
    [-0.92, -0.80],
    [-0.52, -1.00],
    [0.00, -0.86],
    [0.52, -1.00],
    [0.92, -0.80],
    [1.00, -0.25],
    [0.80, 0.45],
    [0.38, 0.85],
    [0.00, 1.00],
    [-0.38, 0.85],
    [-0.80, 0.45],
    [-1.00, -0.25],
];

/// A route-reference badge a label candidate draws.
///
/// The badge is sized in screen pixels around the reference's measured glyphs
/// at placement time; a candidate without drawable glyphs is skipped before a
/// badge is ever considered.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LabelShield {
    /// The badge outline.
    pub shape: ShieldShape,
    /// The badge fill colour, `[red, green, blue, alpha]` in `0.0..=1.0`.
    pub fill_rgba: [f32; 4],
    /// The outline colour; absent draws no outline.
    pub border_rgba: Option<[f32; 4]>,
    /// The outline width in screen pixels; zero draws no outline.
    pub border_width_pixels: f64,
    /// The inset between the reference's glyphs and the badge edge, in screen
    /// pixels.
    pub padding_pixels: f64,
    /// The smallest badge width and height, in screen pixels.
    pub minimum_size_pixels: f64,
}

/// A route-reference badge that survived placement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacedShield {
    /// The badge outline.
    pub shape: ShieldShape,
    /// The badge fill colour, `[red, green, blue, alpha]` in `0.0..=1.0`.
    pub fill_rgba: [f32; 4],
    /// The outline colour; absent draws no outline.
    pub border_rgba: Option<[f32; 4]>,
    /// The outline width in drawing-buffer pixels; zero draws no outline.
    pub border_width_pixels: f64,
    /// The badge's drawn width in drawing-buffer pixels.
    pub width_pixels: f64,
    /// The badge's drawn height in drawing-buffer pixels.
    pub height_pixels: f64,
}

/// One icon candidate read from a symbol layer's `icon-image` layout: the
/// sprite sheet entry to draw and the geometry that positions it. An icon
/// whose name the injected sprite atlas does not serve is simply not drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct MapIconCandidate {
    /// The sprite index entry the layer names.
    pub image_name: String,
    /// The display scale relative to the sheet rectangle.
    pub size: f64,
    /// Where the icon sits relative to the symbol anchor.
    pub anchor: TextAnchor,
    /// The displacement from the anchor, in CSS pixels.
    pub offset_pixels: [f64; 2],
    /// The clockwise rotation in degrees.
    pub rotation_degrees: f64,
    /// What the rotation follows.
    pub rotation_alignment: TextRotationAlignment,
    /// The transparent margin kept around the collision box, in pixels.
    pub padding_pixels: f64,
    /// How the icon is stretched to fit its text block. `None` draws the sheet
    /// rectangle at its own aspect ratio.
    pub text_fit: IconTextFit,
    /// The inset kept between a fitted icon and the text block,
    /// `[top, right, bottom, left]` in CSS pixels.
    pub text_fit_padding_pixels: [f64; 4],
    /// The icon's opacity, `[0, 1]`, multiplied into the drawn color.
    pub opacity: f32,
    /// Whether the icon may overlap other symbols.
    pub allow_overlap: bool,
    /// Whether the icon ignores collision placement entirely.
    pub ignore_placement: bool,
}

/// One label candidate read from a decoded tile's symbol layers, before the
/// frame projects and places it.
#[derive(Debug, Clone, PartialEq)]
pub struct MapLabelCandidate {
    /// The label text, read through the style's ordered fallbacks.
    pub text: String,
    /// The geographic anchor the label is centered on. A point-placed
    /// candidate anchors here; a line-placed candidate carries its line's
    /// first vertex, and the frame re-derives the anchor from the projected
    /// segment.
    pub anchor: GeographicCoordinate,
    /// The line geometry a line-placed candidate runs along, in the tile's
    /// order. Empty for a point-placed candidate (and a line with fewer than
    /// two vertices places like a point).
    pub line_coordinates: Vec<GeographicCoordinate>,
    /// The text paint.
    pub text_rgba: [f32; 4],
    /// The text size in screen pixels at the frame's zoom.
    pub text_size_pixels: f64,
    /// The halo paint drawn behind the text.
    pub halo_rgba: [f32; 4],
    /// The halo width in screen pixels; zero paints no halo.
    pub halo_width_pixels: f64,
    /// The style layer's priority rank (lower draws first).
    pub priority: u32,
    /// The feature's own rank (lower is more important); unranked labels carry
    /// [`DEFAULT_LABEL_RANK`].
    pub rank: u32,
    /// Whether this label may overlap others.
    pub allow_overlap: bool,
    /// Whether this label ignores collision placement entirely.
    pub ignore_placement: bool,
    /// Whether a placement crossing the viewport edge is dropped.
    pub avoid_edges: bool,
    /// The text's displacement from its anchor, in CSS pixels.
    pub text_offset_pixels: [f64; 2],
    /// The transparent margin kept around the text's collision boxes, in CSS
    /// pixels (the style's `text-padding`; the resolved default is the renderer
    /// default).
    pub text_padding_pixels: f64,
    /// Whether the text lies on the map plane (`Map`), facing the screen
    /// (`Viewport`), or follows the placement (`Auto`, already resolved by the
    /// style compile). A map-aligned label foreshortens vertically as the
    /// camera pitches.
    pub text_pitch_alignment: TextPitchAlignment,
    /// The gap kept between repeated names along a line, in screen pixels.
    pub repeat_spacing_pixels: f64,
    /// Whether the label is placed once at its line's midpoint instead of
    /// being repeated along it.
    pub line_center_only: bool,
    /// The wrapping width in ems; the text size scales it to pixels.
    pub text_max_width_ems: f64,
    /// One line's height in ems.
    pub text_line_height_ems: f64,
    /// The extra space between glyphs, in ems.
    pub text_letter_spacing_ems: f64,
    /// How wrapped lines are aligned within the text block.
    pub text_justify: TextJustify,
    /// Where the text block sits relative to the symbol anchor.
    pub text_anchor: TextAnchor,
    /// What a point-placed label's rotation follows.
    pub text_rotation_alignment: TextRotationAlignment,
    /// Whether a rotated label is flipped to read upright.
    pub text_keep_upright: bool,
    /// Candidate anchors a point label tries, in order; empty uses
    /// [`MapLabelCandidate::text_anchor`].
    pub text_variable_anchors: Vec<TextAnchor>,
    /// The radial distance a variable-anchored label sits from its symbol,
    /// in ems.
    pub text_radial_offset_ems: f64,
    /// The sharpest turn a line label's straight run tolerates, in radians.
    pub text_max_angle_radians: f64,
    /// The candidate's style-layer position, the first sort key.
    pub style_order: usize,
    /// The sprite icon the candidate draws, when its layer names one.
    pub icon: Option<MapIconCandidate>,
    /// The route-reference badge the candidate draws, when its layer declares
    /// one. The badge encloses the candidate's own text.
    pub shield: Option<LabelShield>,
}

/// One label that survived collision rejection, in draw order.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedLabel {
    /// The label text.
    pub text: String,
    /// The label's anchor in viewport pixels (origin top-left, y down).
    pub anchor_screen_pixels: [f64; 2],
    /// The text's baseline rotation in radians (clockwise, y down); zero for
    /// a point-placed label and the segment's upright bearing for a
    /// line-placed one.
    pub rotation_radians: f64,
    /// Whether this label was placed along a line feature (a road name)
    /// rather than at a point.
    pub placed_along_line: bool,
    /// The text size in screen pixels.
    pub text_size_pixels: f64,
    /// The vertical foreshortening a map-plane-aligned label carries at the
    /// frame's pitch (`cos(pitch)`), `1.0` for a screen-facing label. The
    /// glyph height and line offsets scale by it; the advance stays unscaled,
    /// so a pitched map-plane label reads as lying on the ground.
    pub pitch_scale_y: f64,
    /// The text paint.
    pub text_rgba: [f32; 4],
    /// The halo paint drawn behind the text.
    pub halo_rgba: [f32; 4],
    /// The halo width in screen pixels; zero paints no halo.
    pub halo_width_pixels: f64,
    /// The label's wrapped lines, top to bottom.
    pub lines: Vec<LabelTextLine>,
    /// One line's height in screen pixels.
    pub line_height_pixels: f64,
    /// The widest line's width in screen pixels.
    pub block_width_pixels: f64,
    /// The extra space between glyphs, in screen pixels.
    pub letter_spacing_pixels: f64,
    /// How the lines are aligned within the block.
    pub text_justify: TextJustify,
    /// The style layer's priority rank.
    pub priority: u32,
    /// The sprite icon placed with the label, when the layer names one.
    pub icon: Option<PlacedIcon>,
    /// The route-reference badge placed beneath the label's text, when the
    /// layer declares one.
    pub shield: Option<PlacedShield>,
}

/// One sprite icon that survived collision rejection.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedIcon {
    /// The sprite index entry that was drawn.
    pub image_name: String,
    /// The icon's center in viewport pixels (origin top-left, y down).
    pub anchor_screen_pixels: [f64; 2],
    /// The icon's clockwise rotation in radians.
    pub rotation_radians: f64,
    /// The icon's drawn width in screen pixels.
    pub width_pixels: f64,
    /// The icon's drawn height in screen pixels.
    pub height_pixels: f64,
    /// The atlas window the icon samples.
    pub texture_rectangle: SpriteTextureRectangle,
    /// The icon's opacity, `[0, 1]`.
    pub opacity: f32,
}

/// One line of a wrapped label's text.
#[derive(Debug, Clone, PartialEq)]
pub struct LabelTextLine {
    /// The line's text.
    pub text: String,
    /// The line's advance width in screen pixels, letter spacing included.
    pub width_pixels: f64,
}

/// A label's text block: its wrapped lines and the box they occupy.
#[derive(Debug, Clone, PartialEq)]
pub struct LabelTextBlock {
    /// The lines, top to bottom.
    pub lines: Vec<LabelTextLine>,
    /// The widest line's advance width in screen pixels.
    pub width_pixels: f64,
    /// The block's total height in screen pixels.
    pub height_pixels: f64,
    /// One line's height in screen pixels.
    pub line_height_pixels: f64,
}

/// The measured extent of one label's text at one size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LabelTextMetrics {
    /// Total advance width in screen pixels.
    pub width_pixels: f64,
    /// Line height in screen pixels (the text size).
    pub height_pixels: f64,
}

/// A packed glyph atlas plus the metrics the label pass measures text with.
#[derive(Debug, Clone)]
pub struct LabelFont {
    /// The font stack name the payload declares.
    font_stack_name: Arc<str>,
    /// The packed `R8Unorm` atlas and every glyph's metrics. Shared so a
    /// per-frame label layer clone never copies the atlas page.
    atlas: Arc<PackedGlyphAtlas>,
    /// A deterministic identity for the packed pixels and metrics, so a
    /// backend uploads the atlas once per distinct font and never per frame.
    revision: u64,
    /// How many Latin glyphs sit at the front of the packed atlas. The packer
    /// orders glyphs as the Latin ranges first, then every other range, each
    /// group ascending by codepoint, so this index splits the atlas into the
    /// two sorted groups a glyph lookup binary-searches.
    latin_glyph_count: usize,
}

impl PartialEq for LabelFont {
    fn eq(&self, other: &Self) -> bool {
        self.revision == other.revision
    }
}

impl LabelFont {
    /// Packs one decoded glyph range into the atlas the label pass measures
    /// and draws with.
    ///
    /// # Parameters
    /// - `font_stack` — the decoded glyph range to pack.
    ///
    /// # Errors
    /// A page too small for the range reports the packer's typed error rather
    /// than dropping glyphs.
    pub fn from_glyph_font_stack(font_stack: &GlyphFontStack) -> Result<Self, GlyphAtlasError> {
        let atlas = pack_glyph_atlas(&font_stack.glyphs, Default::default())?;
        Ok(Self {
            font_stack_name: Arc::from(font_stack.name.as_str()),
            revision: atlas_revision(&atlas),
            latin_glyph_count: latin_glyph_prefix_length(&atlas),
            atlas: Arc::new(atlas),
        })
    }

    /// The font stack name the payload declares.
    pub fn font_stack_name(&self) -> &str {
        &self.font_stack_name
    }

    /// The packed atlas, for a backend uploading its pixels.
    pub fn atlas(&self) -> &PackedGlyphAtlas {
        &self.atlas
    }

    /// The atlas' content revision; a backend uploads the texture when this
    /// changes and reuses it otherwise.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The packed glyph for a codepoint, if the range serves it.
    ///
    /// The lookup binary-searches the atlas group that can contain the
    /// codepoint (the Latin ranges or every other range), so measuring text
    /// stays logarithmic in the glyph count rather than scanning the page's
    /// whole glyph list per character.
    pub fn glyph(&self, codepoint: u32) -> Option<&PackedGlyph> {
        let glyphs = &self.atlas.glyphs;
        let latin_glyph_count = self.latin_glyph_count.min(glyphs.len());
        let searchable_glyphs = if GlyphRange::is_latin_codepoint(codepoint) {
            &glyphs[..latin_glyph_count]
        } else {
            &glyphs[latin_glyph_count..]
        };
        let glyph_index = searchable_glyphs
            .binary_search_by_key(&codepoint, |glyph| glyph.codepoint)
            .ok()?;
        searchable_glyphs.get(glyph_index)
    }

    /// Measures a label's text at `text_size_pixels`.
    ///
    /// Characters the font does not serve contribute no advance and paint
    /// nothing, exactly as the quad emitter skips them, so a name with one
    /// out-of-range character still draws its remaining text rather than
    /// disappearing. `None` is reported only when the font serves none of the
    /// characters at all — a label that would paint nothing is not placed.
    pub fn measure_text(&self, text: &str, text_size_pixels: f64) -> Option<LabelTextMetrics> {
        let width_pixels = self.measure_line_width(text, text_size_pixels, 0.0)?;
        Some(LabelTextMetrics {
            width_pixels,
            height_pixels: text_size_pixels,
        })
    }

    /// One line's advance width, letter spacing included between served
    /// glyphs; `None` when the font serves none of the line's characters.
    fn measure_line_width(
        &self,
        text: &str,
        text_size_pixels: f64,
        letter_spacing_pixels: f64,
    ) -> Option<f64> {
        let scale = text_size_pixels / GLYPH_FONT_EM_PIXELS;
        let mut width_pixels = 0.0;
        let mut served_glyph_count = 0_usize;
        for character in text.chars() {
            let Some(glyph) = self.glyph(u32::from(character)) else {
                continue;
            };
            if served_glyph_count > 0 {
                width_pixels += letter_spacing_pixels;
            }
            served_glyph_count += 1;
            width_pixels += f64::from(glyph.advance) * scale;
        }
        if served_glyph_count == 0 {
            return None;
        }
        Some(width_pixels)
    }

    /// Measures a label's text as a wrapped block.
    ///
    /// Words are laid out greedily: a word that would push the line past
    /// `maximum_width_pixels` starts the next one, and a word wider than the
    /// limit on its own keeps that line. `line_height_ems` multiplies the
    /// text size for one line's height. The block width is the widest line,
    /// so a centered block is centered on its longest line. `None` is
    /// reported when the font serves no character of any line.
    pub fn measure_wrapped_text(
        &self,
        text: &str,
        text_size_pixels: f64,
        maximum_width_pixels: f64,
        line_height_ems: f64,
        letter_spacing_pixels: f64,
    ) -> Option<LabelTextBlock> {
        let mut line_texts: Vec<String> = Vec::new();
        let mut current_line = String::new();
        for word in text.split_whitespace() {
            let candidate_line = if current_line.is_empty() {
                word.to_string()
            } else {
                format!("{current_line} {word}")
            };
            let candidate_width =
                self.measure_line_width(&candidate_line, text_size_pixels, letter_spacing_pixels);
            let exceeds_limit = candidate_width
                .is_some_and(|candidate_width| candidate_width > maximum_width_pixels);
            if !current_line.is_empty() && exceeds_limit {
                line_texts.push(std::mem::take(&mut current_line));
                current_line = word.to_string();
            } else {
                current_line = candidate_line;
            }
        }
        if !current_line.is_empty() {
            line_texts.push(current_line);
        }
        if line_texts.is_empty() {
            return None;
        }
        let mut lines = Vec::with_capacity(line_texts.len());
        let mut widest_line_pixels = 0.0_f64;
        let mut served_any_line = false;
        for line_text in line_texts {
            let width_pixels = self
                .measure_line_width(&line_text, text_size_pixels, letter_spacing_pixels)
                .unwrap_or(0.0);
            served_any_line |= width_pixels > 0.0;
            widest_line_pixels = widest_line_pixels.max(width_pixels);
            lines.push(LabelTextLine {
                text: line_text,
                width_pixels,
            });
        }
        if !served_any_line {
            return None;
        }
        let line_height_pixels = text_size_pixels * line_height_ems;
        Some(LabelTextBlock {
            height_pixels: line_height_pixels * lines.len() as f64,
            lines,
            width_pixels: widest_line_pixels,
            line_height_pixels,
        })
    }
}

/// How many Latin glyphs sit at the front of a packed atlas: the packer
/// orders the Latin ranges first, then every other range, each group ascending
/// by codepoint, so the first non-Latin glyph's index splits the atlas into
/// the two sorted groups a binary-search glyph lookup searches.
fn latin_glyph_prefix_length(atlas: &PackedGlyphAtlas) -> usize {
    atlas
        .glyphs
        .iter()
        .position(|glyph| !GlyphRange::is_latin_codepoint(glyph.codepoint))
        .unwrap_or(atlas.glyphs.len())
}

/// A deterministic fingerprint of a packed atlas: the page dimensions, every
/// glyph's codepoint and rectangle, and the page pixels. Computed once when
/// the atlas is packed, never per frame.
fn atlas_revision(atlas: &PackedGlyphAtlas) -> u64 {
    let mut hasher = DefaultHasher::new();
    atlas.width_pixels.hash(&mut hasher);
    atlas.height_pixels.hash(&mut hasher);
    for glyph in &atlas.glyphs {
        glyph.codepoint.hash(&mut hasher);
        glyph.bitmap_rectangle.x_pixels.hash(&mut hasher);
        glyph.bitmap_rectangle.y_pixels.hash(&mut hasher);
        glyph.bitmap_rectangle.width_pixels.hash(&mut hasher);
        glyph.bitmap_rectangle.height_pixels.hash(&mut hasher);
    }
    atlas.pixels.hash(&mut hasher);
    hasher.finish()
}

/// The Latin glyph range the SDK's default label font decodes: a glyph-protocol
/// `0-255` payload in Noto Sans Regular, embedded in the library. Embedding it
/// gives every surface a glyph source without a network fetch or a
/// platform-specific asset.
pub const DEFAULT_LATIN_GLYPH_RANGE_PBF_BYTES: &[u8] =
    include_bytes!("../../../fixtures/glyphs/noto_sans_regular_0-255.pbf");

/// The default label font, decoded and packed on first use and then shared by
/// every frame and every view in the process.
static DEFAULT_LABEL_FONT: OnceLock<LabelFont> = OnceLock::new();

/// The SDK's default label font: [`DEFAULT_LATIN_GLYPH_RANGE_PBF_BYTES`]
/// decoded and packed into one atlas the first time a frame asks for it, then
/// reused. The packed atlas rides in an [`Arc`], so handing the font to
/// [`LabelLayer::place`] never re-copies the page, and the renderer's atlas
/// upload happens once per distinct revision.
///
/// The embedded bytes are part of the library; should they ever fail to
/// decode or pack (a corrupted build rather than a normal path), the in-code
/// fallback is an empty font stack: every label measures no glyph and the pass
/// paints nothing, so a map still draws instead of failing. This mirrors the
/// vector style's in-code fallback when its embedded artifact is unreadable.
/// A caller with another glyph source keeps using [`LabelLayer::place`] with
/// its own [`LabelFont`].
pub fn default_label_font() -> &'static LabelFont {
    DEFAULT_LABEL_FONT.get_or_init(|| {
        decode_glyph_font_stack(DEFAULT_LATIN_GLYPH_RANGE_PBF_BYTES)
            .ok()
            .and_then(|font_stack| LabelFont::from_glyph_font_stack(&font_stack).ok())
            .unwrap_or_else(empty_label_font)
    })
}

#[cfg(test)]
mod measurement_tests;

/// The default font's documented in-code fallback: a font stack with no
/// glyphs, which packs into a valid empty atlas page. Labels measured against
/// it carry no servable glyph and are skipped, so the frame paints the map
/// without text.
fn empty_label_font() -> LabelFont {
    match LabelFont::from_glyph_font_stack(&GlyphFontStack::default()) {
        Ok(font) => font,
        // An empty stack has nothing to overflow, so this arm is unreachable
        // in practice; keeping the fallback total means a corrupted embedded
        // fixture still paints a map rather than panicking at draw time.
        Err(_) => {
            let atlas = PackedGlyphAtlas {
                width_pixels: 1,
                height_pixels: 1,
                pixels: vec![0],
                glyphs: Vec::new(),
            };
            LabelFont {
                font_stack_name: Arc::from(""),
                revision: atlas_revision(&atlas),
                latin_glyph_count: 0,
                atlas: Arc::new(atlas),
            }
        }
    }
}

// One label per distinct text is placed in a frame, and that rule also
// covers route-reference badges: two candidates carrying the same reference
// deduplicate, whether they are repeated candidates of one feature or two
// features far apart. `PRD.md §6 item 57` states the rule for
// shields.

/// A label layer ready to draw: the placed labels and the font that measures
/// and rasterizes them.
#[derive(Debug, Clone)]
pub struct LabelLayer {
    labels: Vec<PlacedLabel>,
    font: LabelFont,
    /// The device pixels one point/CSS pixel covers: 1.0 for the headless
    /// golden at one device pixel per point, the display scale on a native
    /// surface, the canvas ratio in the browser. It sets the drawn text size
    /// and the fragment stage's edge gamma; the halo width already arrives in
    /// the same points the style declares.
    device_pixel_ratio: f64,
    /// The sprite atlas the placed icons sample, when the frame supplied one.
    sprite: Option<Arc<SpriteAtlas>>,
}

impl LabelLayer {
    /// Projects and places the frame's candidates: thin ranked labels by the
    /// frame zoom, sort by symbol class (lower is more important, the field's
    /// documented collision rule), then feature rank, then style order; drop
    /// repeated text; reject a candidate any of whose padded glyph boxes
    /// collides with an already accepted label unless the candidate allows
    /// overlap; stop once the frame has placed
    /// [`MAXIMUM_LABELS_PER_FRAME`] labels. A line-placed candidate takes its
    /// longest straight projected run as the text's run and repeats along it,
    /// each label rotated to the segment beneath it; every other candidate
    /// centers on its anchor, unrotated. Repeated names of one candidate are
    /// placed together, and the text is recorded once so a later candidate
    /// carrying the same name — the same street in a neighbouring tile — is
    /// still rejected. The result keeps the sorted order, so the same
    /// candidates always paint in the same sequence.
    pub fn place(
        camera: &MapCamera,
        viewport: &ViewportPixels,
        field_of_view_degrees: f64,
        candidates: &[MapLabelCandidate],
        font: &LabelFont,
    ) -> Self {
        Self::place_with_device_pixel_ratio(
            camera,
            viewport,
            field_of_view_degrees,
            candidates,
            font,
            1.0,
        )
    }

    /// [`Self::place`] for a surface whose drawing buffer is not one device
    /// pixel per point: the drawn glyph size and the edge gamma both follow
    /// `device_pixel_ratio`, so the text keeps its declared point size and its
    /// one-pixel anti-aliased edge on a 2× or 3× surface.
    pub fn place_with_device_pixel_ratio(
        camera: &MapCamera,
        viewport: &ViewportPixels,
        field_of_view_degrees: f64,
        candidates: &[MapLabelCandidate],
        font: &LabelFont,
        device_pixel_ratio: f64,
    ) -> Self {
        Self::place_with_history(
            camera,
            viewport,
            field_of_view_degrees,
            candidates,
            font,
            device_pixel_ratio,
            &[],
        )
    }

    /// [`Self::place_with_device_pixel_ratio`] carrying a persistent collision
    /// index across frames: `previous_labels` are the labels the previous frame
    /// of the same view placed. The memory breaks ties among candidates that
    /// share the whole priority key (symbol class, then rank, then style-layer
    /// tier): an equal-priority name the previous frame placed is tried before
    /// its equal-priority newcomers, in the order the previous frame held
    /// them. A remembered name therefore keeps its spot while it stays
    /// visible, so panning half a street name off the edge cannot let an
    /// equal-priority neighbour take its place for one frame and lose it the
    /// next. The history never overrides a better symbol class, rank or
    /// style-layer tier, so two identical frames always place the same
    /// priority winners.
    ///
    /// The memory is the label text, the pass's own identity for a name (a
    /// frame drops repeated text), so it survives a camera move without
    /// carrying stale screen geometry: the remembered candidate is projected
    /// and collides like any other at the new pose, and a name that is no
    /// longer among the candidates is simply forgotten.
    pub fn place_with_history(
        camera: &MapCamera,
        viewport: &ViewportPixels,
        field_of_view_degrees: f64,
        candidates: &[MapLabelCandidate],
        font: &LabelFont,
        device_pixel_ratio: f64,
        previous_labels: &[PlacedLabel],
    ) -> Self {
        Self::place_with_history_and_sprite(
            camera,
            viewport,
            field_of_view_degrees,
            candidates,
            font,
            device_pixel_ratio,
            previous_labels,
            None,
        )
    }

    /// [`Self::place_with_history`] with a sprite atlas for the candidates'
    /// `icon-image` entries.
    ///
    /// A candidate's icon and text collide as separate boxes: each must find
    /// room under its own overlap rule, and the icon's own `icon-padding`
    /// inflates its box. An icon the atlas does not serve contributes no box
    /// and no draw, so a missing sprite or a missing entry leaves the text
    /// exactly as it would have drawn.
    ///
    /// Placement order is explicit: a candidate's symbol class
    /// (`priority`, the style's `symbol-sort-key`) first, then its feature
    /// rank, then its style-layer order (the declared class/zoom tier), then
    /// the previous frame's position among the candidates that still tie, with
    /// the caller's order breaking the remaining ties. A route shield
    /// therefore draws before a place name, and a more important ranked name
    /// beats a lower-ranked one even when the style lists their layers the
    /// other way round; the history can only reorder names that are otherwise
    /// equally important.
    ///
    /// Regional thinning runs before the order: at a wide frame zoom a ranked
    /// candidate above [`maximum_label_rank_at_zoom`] is dropped, so the
    /// remaining places compete for the room in importance order. Unranked
    /// labels (road names and shields) are exempt.
    #[allow(clippy::too_many_arguments)]
    pub fn place_with_history_and_sprite(
        camera: &MapCamera,
        viewport: &ViewportPixels,
        field_of_view_degrees: f64,
        candidates: &[MapLabelCandidate],
        font: &LabelFont,
        device_pixel_ratio: f64,
        previous_labels: &[PlacedLabel],
        sprite: Option<&Arc<SpriteAtlas>>,
    ) -> Self {
        // Regional thinning is a frame policy, not style data: the same
        // candidate list at a wider camera keeps only the important ranks.
        let mut ordered_candidates: Vec<&MapLabelCandidate> = candidates
            .iter()
            .filter(|candidate| label_passes_regional_thinning(candidate.rank, camera.zoom_level))
            .collect();
        // The history is a preference only among candidates that tie on the
        // whole priority key. A remembered name therefore keeps its slot
        // against an equal-priority newcomer, but it can never jump ahead of a
        // better symbol class, rank or style-layer tier. Ordering the history
        // first regardless of rank made the previous frame's winner beat its
        // own higher-ranked competitor, and two identical frames could settle
        // on different label sets.
        let mut remembered_positions: HashMap<&str, usize> = HashMap::new();
        for (position, remembered) in previous_labels.iter().enumerate() {
            if remembered.text.is_empty() {
                continue;
            }
            remembered_positions
                .entry(remembered.text.as_str())
                .or_insert(position);
        }
        // Symbol class first, then feature rank, then the style layer's own
        // class/zoom tier, then the remembered position within the ties.
        // `sort_by_key` is stable, so candidates that tie on the whole key keep
        // the caller's own order — the frame's tiles list deterministically.
        // This is the documented rule: class before rank, shields before names.
        ordered_candidates.sort_by_key(|candidate| {
            (
                candidate.priority,
                candidate.rank,
                candidate.style_order,
                remembered_positions
                    .get(candidate.text.as_str())
                    .copied()
                    .unwrap_or(usize::MAX),
            )
        });

        let mut occupied_boxes: Vec<LabelScreenBox> = Vec::new();
        let mut placed_labels: Vec<PlacedLabel> = Vec::new();
        let mut placed_texts: Vec<String> = Vec::new();
        // Line-placed names get a reserved share of the budget, but only as
        // many slots as the frame has line candidates to fill: a frame with
        // few roads gives the unused slots back to the places.
        let line_candidate_count = ordered_candidates
            .iter()
            .filter(|candidate| !candidate.text.is_empty() && candidate.line_coordinates.len() >= 2)
            .count();
        let place_label_budget =
            MAXIMUM_LABELS_PER_FRAME - RESERVED_LINE_LABEL_SLOTS.min(line_candidate_count);
        for candidate in ordered_candidates {
            if placed_labels.len() >= MAXIMUM_LABELS_PER_FRAME {
                break;
            }
            let placed_along_line =
                !candidate.text.is_empty() && candidate.line_coordinates.len() >= 2;
            if !placed_along_line && placed_labels.len() >= place_label_budget {
                // Keep the reserved slots for the line names later in the
                // priority order instead of ending the frame early.
                continue;
            }
            if !candidate.text.is_empty() && placed_texts.iter().any(|text| text == &candidate.text)
            {
                continue;
            }
            // The style's size is a point/CSS size; the drawing buffer draws
            // device pixels, so the glyphs scale by the surface ratio while the
            // style numbers stay the source of truth for gamma and halo math.
            let text_size_pixels = candidate.text_size_pixels * device_pixel_ratio;
            let letter_spacing_pixels = candidate.text_letter_spacing_ems * text_size_pixels;
            // A line-placed name stays on one line, the specification's rule;
            // wrapping applies to point-placed text. An icon-only symbol has
            // no text block at all.
            let text_block: Option<LabelTextBlock> = if candidate.text.is_empty() {
                None
            } else if placed_along_line {
                font.measure_line_width(&candidate.text, text_size_pixels, letter_spacing_pixels)
                    .map(|width_pixels| LabelTextBlock {
                        lines: vec![LabelTextLine {
                            text: candidate.text.clone(),
                            width_pixels,
                        }],
                        width_pixels,
                        height_pixels: text_size_pixels,
                        line_height_pixels: text_size_pixels,
                    })
            } else {
                font.measure_wrapped_text(
                    &candidate.text,
                    text_size_pixels,
                    candidate.text_max_width_ems * text_size_pixels,
                    candidate.text_line_height_ems,
                    letter_spacing_pixels,
                )
            };
            // Icons ride point placement. A line-placed icon would need a
            // run-relative anchor the specification leaves to the renderer;
            // a line symbol keeps its text and draws no icon rather than
            // anchoring the icon at an arbitrary vertex.
            let icon_candidate = if placed_along_line {
                None
            } else {
                candidate.icon.as_ref()
            };
            // A map-plane-aligned label foreshortens vertically as the camera
            // pitches; a screen-facing one keeps its measured height. `Auto`
            // is resolved by the style compile, and any remaining value keeps
            // the screen-facing default.
            let pitch_scale_y = match candidate.text_pitch_alignment {
                TextPitchAlignment::Map => camera.pitch_degrees.to_radians().cos().max(0.0),
                TextPitchAlignment::Viewport | TextPitchAlignment::Auto => 1.0,
            };
            // A badge encloses the measured text block plus its own padding,
            // and never shrinks below its declared minimum. The text block is
            // absent when the font serves none of the reference's glyphs, so
            // no badge is ever drawn around a run of placeholders.
            let placed_shield =
                text_block
                    .as_ref()
                    .zip(candidate.shield)
                    .map(|(text_block, shield)| {
                        let padding_pixels = shield.padding_pixels * device_pixel_ratio;
                        let minimum_size_pixels = shield.minimum_size_pixels * device_pixel_ratio;
                        PlacedShield {
                            shape: shield.shape,
                            fill_rgba: shield.fill_rgba,
                            border_rgba: shield.border_rgba,
                            border_width_pixels: shield.border_width_pixels * device_pixel_ratio,
                            width_pixels: (text_block.width_pixels + 2.0 * padding_pixels)
                                .max(minimum_size_pixels),
                            height_pixels: (text_block.height_pixels * pitch_scale_y
                                + 2.0 * padding_pixels)
                                .max(minimum_size_pixels),
                        }
                    });
            let icon_placement = icon_candidate.and_then(|icon_candidate| {
                icon_placement_at(
                    camera,
                    viewport,
                    field_of_view_degrees,
                    &candidate.anchor,
                    icon_candidate,
                    text_block
                        .as_ref()
                        .map(|text_block| [text_block.width_pixels, text_block.height_pixels]),
                    sprite.map(Arc::as_ref),
                    device_pixel_ratio,
                )
            });
            if text_block.is_none() && icon_placement.is_none() {
                continue;
            }
            // An icon shares the anchor with its text, so a variable anchor
            // cannot move one without the other; an icon-bearing candidate
            // keeps its declared anchor.
            let uses_variable_anchor = text_block.is_some()
                && !placed_along_line
                && icon_placement.is_none()
                && !candidate.text_variable_anchors.is_empty();
            let placements = if let Some(text_block) = &text_block {
                if placed_along_line {
                    let mut line_placements = label_placements_along_line(
                        camera,
                        viewport,
                        field_of_view_degrees,
                        &candidate.line_coordinates,
                        text_block.width_pixels,
                        candidate.repeat_spacing_pixels,
                        candidate.line_center_only,
                        candidate.text_max_angle_radians,
                    );
                    if candidate.text_rotation_alignment == TextRotationAlignment::Viewport {
                        for placement in &mut line_placements {
                            placement.rotation_radians = 0.0;
                        }
                    }
                    line_placements
                } else {
                    // A point label follows the map's bearing only when its
                    // style asks; `auto` is the viewport alignment for point
                    // placement.
                    let point_rotation_radians = match candidate.text_rotation_alignment {
                        TextRotationAlignment::Map => camera.bearing_degrees.to_radians(),
                        TextRotationAlignment::Viewport | TextRotationAlignment::Auto => 0.0,
                    };
                    let point_rotation_radians = if candidate.text_keep_upright {
                        upright_rotation_radians(point_rotation_radians)
                    } else {
                        point_rotation_radians
                    };
                    let anchor_candidates: Vec<TextAnchor> =
                        if candidate.text_variable_anchors.is_empty() {
                            vec![candidate.text_anchor]
                        } else {
                            candidate.text_variable_anchors.clone()
                        };
                    match camera.project_to_screen(
                        &candidate.anchor,
                        viewport,
                        field_of_view_degrees,
                    ) {
                        Some(anchor_screen_pixels) => anchor_candidates
                            .iter()
                            .map(|text_anchor| {
                                let mut anchor_offset = text_anchor_center_offset(
                                    *text_anchor,
                                    text_block.width_pixels,
                                    text_block.height_pixels,
                                );
                                // A variable anchor pushes the block outward by
                                // the radial offset; a fixed anchor ignores it.
                                if uses_variable_anchor {
                                    let radial_direction = radial_offset_direction(*text_anchor);
                                    anchor_offset[0] += radial_direction[0]
                                        * candidate.text_radial_offset_ems
                                        * text_size_pixels;
                                    anchor_offset[1] += radial_direction[1]
                                        * candidate.text_radial_offset_ems
                                        * text_size_pixels;
                                }
                                LabelPlacement {
                                    anchor_screen_pixels: [
                                        anchor_screen_pixels[0] + anchor_offset[0],
                                        anchor_screen_pixels[1] + anchor_offset[1],
                                    ],
                                    rotation_radians: point_rotation_radians,
                                }
                            })
                            .collect(),
                        None => Vec::new(),
                    }
                }
            } else {
                // An icon-only symbol still needs one placement to drive the
                // acceptance loop; the icon's own geometry is independent of
                // it and is measured from the candidate anchor.
                camera
                    .project_to_screen(&candidate.anchor, viewport, field_of_view_degrees)
                    .map(|anchor_screen_pixels| {
                        vec![LabelPlacement {
                            anchor_screen_pixels,
                            rotation_radians: 0.0,
                        }]
                    })
                    .unwrap_or_default()
            };
            // A text offset is a screen-space displacement from the anchor
            // in ems; the candidate carries it already scaled to CSS pixels.
            let placements: Vec<LabelPlacement> = placements
                .into_iter()
                .map(|mut placement| {
                    placement.anchor_screen_pixels[0] +=
                        candidate.text_offset_pixels[0] * device_pixel_ratio;
                    placement.anchor_screen_pixels[1] +=
                        candidate.text_offset_pixels[1] * device_pixel_ratio;
                    placement
                })
                .collect();
            let text_is_free = candidate.allow_overlap || candidate.ignore_placement;
            let icon_is_free = icon_candidate
                .map(|icon_candidate| {
                    icon_candidate.allow_overlap || icon_candidate.ignore_placement
                })
                .unwrap_or(true);
            let mut placed_any = false;
            for placement in placements {
                if placed_labels.len() >= MAXIMUM_LABELS_PER_FRAME {
                    break;
                }
                // Collision uses one box per drawn glyph, not one box around
                // the whole text block: a name can then sit in the gap beside
                // a neighbour's word space, as the reference engines allow.
                // Each box carries the label's collision padding, and a
                // rotated label's boxes keep the label's rotation, so a road
                // name rejects with its oriented extent rather than the empty
                // corners of an axis-aligned bound.
                let text_boxes: Vec<LabelScreenBox> = text_block
                    .as_ref()
                    .map(|text_block| {
                        glyph_collision_boxes(
                            font,
                            text_block,
                            text_size_pixels,
                            letter_spacing_pixels,
                            candidate.text_justify,
                            candidate.text_padding_pixels * device_pixel_ratio,
                            pitch_scale_y,
                            placement,
                        )
                    })
                    .unwrap_or_default();
                // A badge is a candidate like any label: its own padded, rotated
                // screen box takes part in collision, so two badges (or a badge
                // and a neighbouring name) cannot overlap just because their
                // glyph runs sit in the gap beside one another.
                let badge_box = placed_shield.map(|shield| {
                    LabelScreenBox::around_with_padding(
                        placement.anchor_screen_pixels,
                        shield.width_pixels,
                        shield.height_pixels * pitch_scale_y,
                        placement.rotation_radians,
                        0.0,
                    )
                });
                let icon_box =
                    icon_placement
                        .zip(icon_candidate)
                        .map(|(icon_placement, icon_candidate)| {
                            LabelScreenBox::around_with_padding(
                                icon_placement.anchor_screen_pixels,
                                icon_placement.width_pixels,
                                icon_placement.height_pixels,
                                icon_placement.rotation_radians,
                                icon_candidate.padding_pixels * device_pixel_ratio,
                            )
                        });
                let inside_viewport = |screen_box: &LabelScreenBox| {
                    if candidate.avoid_edges {
                        screen_box.is_inside_viewport(viewport)
                    } else {
                        screen_box.intersects_viewport(viewport)
                    }
                };
                let text_crosses_viewport = if candidate.avoid_edges {
                    !text_boxes.iter().all(inside_viewport)
                        || badge_box.is_some_and(|screen_box| !inside_viewport(&screen_box))
                } else {
                    !text_boxes.is_empty() && !text_boxes.iter().any(inside_viewport)
                };
                if text_crosses_viewport
                    || icon_box
                        .as_ref()
                        .is_some_and(|screen_box| !inside_viewport(screen_box))
                {
                    continue;
                }
                let text_collides = !text_is_free
                    && text_boxes.iter().any(|screen_box| {
                        occupied_boxes
                            .iter()
                            .any(|occupied| occupied.intersects(screen_box))
                    });
                let badge_collides = badge_box.is_some_and(|screen_box| {
                    !text_is_free
                        && occupied_boxes
                            .iter()
                            .any(|occupied| occupied.intersects(&screen_box))
                });
                let icon_collides = icon_box.is_some_and(|screen_box| {
                    !icon_is_free
                        && occupied_boxes
                            .iter()
                            .any(|occupied| occupied.intersects(&screen_box))
                });
                if text_collides || badge_collides || icon_collides {
                    continue;
                }
                if !text_is_free {
                    occupied_boxes.extend(text_boxes);
                    if let Some(screen_box) = badge_box {
                        occupied_boxes.push(screen_box);
                    }
                }
                if let Some(screen_box) = icon_box
                    && !icon_is_free
                {
                    occupied_boxes.push(screen_box);
                }
                placed_any = true;
                placed_labels.push(PlacedLabel {
                    text: candidate.text.clone(),
                    anchor_screen_pixels: placement.anchor_screen_pixels,
                    rotation_radians: placement.rotation_radians,
                    placed_along_line,
                    text_size_pixels,
                    pitch_scale_y,
                    text_rgba: candidate.text_rgba,
                    halo_rgba: candidate.halo_rgba,
                    halo_width_pixels: candidate.halo_width_pixels * device_pixel_ratio,
                    lines: text_block
                        .as_ref()
                        .map(|text_block| text_block.lines.clone())
                        .unwrap_or_default(),
                    line_height_pixels: text_block
                        .as_ref()
                        .map(|text_block| text_block.line_height_pixels)
                        .unwrap_or_default(),
                    block_width_pixels: text_block
                        .as_ref()
                        .map(|text_block| text_block.width_pixels)
                        .unwrap_or_default(),
                    letter_spacing_pixels,
                    text_justify: candidate.text_justify,
                    priority: candidate.priority,
                    icon: icon_placement.zip(icon_candidate).map(
                        |(icon_placement, icon_candidate)| PlacedIcon {
                            image_name: icon_candidate.image_name.clone(),
                            anchor_screen_pixels: icon_placement.anchor_screen_pixels,
                            rotation_radians: icon_placement.rotation_radians,
                            width_pixels: icon_placement.width_pixels,
                            height_pixels: icon_placement.height_pixels,
                            texture_rectangle: icon_placement.texture_rectangle,
                            opacity: icon_candidate.opacity,
                        },
                    ),
                    shield: placed_shield,
                });
                // A variable-anchor label tries its anchors in order and
                // takes the first that fits, once.
                if uses_variable_anchor {
                    break;
                }
            }
            if placed_any && !candidate.text.is_empty() {
                placed_texts.push(candidate.text.clone());
            }
        }
        Self {
            labels: placed_labels,
            font: font.clone(),
            device_pixel_ratio,
            sprite: sprite.cloned(),
        }
    }

    /// The placed labels, in draw order.
    pub fn labels(&self) -> &[PlacedLabel] {
        &self.labels
    }

    /// The font the layer draws with.
    pub fn font(&self) -> &LabelFont {
        &self.font
    }

    /// The sprite atlas the placed icons sample, when the frame supplied one.
    pub fn sprite(&self) -> Option<&SpriteAtlas> {
        self.sprite.as_deref()
    }

    /// Whether the layer draws anything at all.
    pub fn is_empty(&self) -> bool {
        self.labels.is_empty()
    }

    /// Assembles the layer's clip-space quad vertices: one halo pass (only
    /// labels with a positive halo width) followed by the text pass. Both
    /// lists draw non-indexed through the textured glyph pipeline. A halo
    /// quad is the glyph's bitmap rectangle inflated by the halo width while
    /// keeping the glyph's texture window, which stretches the bitmap's soft
    /// edge outward; the fragment stage's lower halo threshold turns that
    /// stretched edge into the outline.
    ///
    /// A rotated label lays its glyphs out in the label's own unrotated frame
    /// and rotates each glyph rectangle about the label anchor, so rotation
    /// reaches every backend through the shared clip-space positions.
    pub fn quad_vertices(&self, viewport: &ViewportPixels) -> LabelQuadVertices {
        let mut halo_vertices = Vec::new();
        let mut text_vertices = Vec::new();
        let mut badge_vertices = Vec::new();
        for label in &self.labels {
            if let Some(shield) = &label.shield {
                append_shield_badge_vertices(&mut badge_vertices, viewport, label, shield);
            }
            if label.rotation_radians == 0.0 {
                self.append_axis_aligned_label_quads(
                    &mut halo_vertices,
                    &mut text_vertices,
                    viewport,
                    label,
                );
            } else {
                self.append_rotated_label_quads(
                    &mut halo_vertices,
                    &mut text_vertices,
                    viewport,
                    label,
                );
            }
        }
        LabelQuadVertices {
            halo: halo_vertices,
            text: text_vertices,
            badge: badge_vertices,
        }
    }

    /// Assembles the placed icons' clip-space quads. Each icon is a centered
    /// rectangle rotated about its own anchor and mapped to its normalized
    /// atlas window, in the same corner and index order the shared textured
    /// pipeline draws. A layer with no atlas or no placed icon yields no
    /// quads, so the icon pass costs nothing on a text-only frame.
    pub fn icon_quad_vertices(&self, viewport: &ViewportPixels) -> IconQuadVertices {
        let mut quads = Vec::new();
        for label in &self.labels {
            let Some(icon) = &label.icon else {
                continue;
            };
            let half_width_pixels = icon.width_pixels / 2.0;
            let half_height_pixels = icon.height_pixels / 2.0;
            let local_corners = [
                [-half_width_pixels, -half_height_pixels],
                [half_width_pixels, -half_height_pixels],
                [-half_width_pixels, half_height_pixels],
                [half_width_pixels, half_height_pixels],
            ];
            let (rotation_sine, rotation_cosine) = icon.rotation_radians.sin_cos();
            let screen_corners = local_corners.map(|local_corner| {
                [
                    icon.anchor_screen_pixels[0] + local_corner[0] * rotation_cosine
                        - local_corner[1] * rotation_sine,
                    icon.anchor_screen_pixels[1]
                        + local_corner[0] * rotation_sine
                        + local_corner[1] * rotation_cosine,
                ]
            });
            let texture_corners = [
                [
                    icon.texture_rectangle.minimum_u,
                    icon.texture_rectangle.minimum_v,
                ],
                [
                    icon.texture_rectangle.maximum_u,
                    icon.texture_rectangle.minimum_v,
                ],
                [
                    icon.texture_rectangle.minimum_u,
                    icon.texture_rectangle.maximum_v,
                ],
                [
                    icon.texture_rectangle.maximum_u,
                    icon.texture_rectangle.maximum_v,
                ],
            ];
            quads.push(std::array::from_fn(|corner_index| ClipSpaceIconVertex {
                clip_position: [
                    (2.0 * screen_corners[corner_index][0] / viewport.width_pixels - 1.0) as f32,
                    (1.0 - 2.0 * screen_corners[corner_index][1] / viewport.height_pixels) as f32,
                    0.0,
                    1.0,
                ],
                texture_coordinates: texture_corners[corner_index],
                opacity: icon.opacity,
            }));
        }
        IconQuadVertices { quads }
    }

    /// Lays out one unrotated label's glyph quads: the baseline runs along
    /// the screen's `x` axis from the label's centered pen origin.
    fn append_axis_aligned_label_quads(
        &self,
        halo_vertices: &mut Vec<ClipSpaceGlyphVertex>,
        text_vertices: &mut Vec<ClipSpaceGlyphVertex>,
        viewport: &ViewportPixels,
        label: &PlacedLabel,
    ) {
        let scale = label.text_size_pixels / GLYPH_FONT_EM_PIXELS;
        // The style's own (point/CSS) sizes drive the SDF threshold and gamma;
        // the placed sizes are already in drawing-buffer pixels.
        let style_text_size_pixels = label.text_size_pixels / self.device_pixel_ratio;
        let style_halo_width_pixels = label.halo_width_pixels / self.device_pixel_ratio;
        let edge_gamma = glyph_edge_gamma(style_text_size_pixels, self.device_pixel_ratio);
        let halo_threshold =
            glyph_halo_signed_distance_threshold(style_text_size_pixels, style_halo_width_pixels);
        let baseline_offset_pixels =
            label.text_size_pixels * LABEL_BASELINE_OFFSET_FRACTION * label.pitch_scale_y;
        let line_count = label.lines.len() as f64;
        for (line_index, line) in label.lines.iter().enumerate() {
            // Lines stack around the anchor: line 0 is the top line.
            let line_offset_pixels = (line_index as f64 - (line_count - 1.0) / 2.0)
                * label.line_height_pixels
                * label.pitch_scale_y;
            let baseline_y =
                label.anchor_screen_pixels[1] + line_offset_pixels + baseline_offset_pixels;
            let mut pen_x = match label.text_justify {
                TextJustify::Left => label.anchor_screen_pixels[0] - label.block_width_pixels / 2.0,
                TextJustify::Right => {
                    label.anchor_screen_pixels[0] + label.block_width_pixels / 2.0
                        - line.width_pixels
                }
                TextJustify::Auto | TextJustify::Center => {
                    label.anchor_screen_pixels[0] - line.width_pixels / 2.0
                }
            };
            for character in line.text.chars() {
                let Some(glyph) = self.font.glyph(u32::from(character)) else {
                    continue;
                };
                let advance_pixels = f64::from(glyph.advance) * scale;
                if glyph.bitmap_rectangle.width_pixels > 0
                    && glyph.bitmap_rectangle.height_pixels > 0
                {
                    let bitmap_left_pixels = pen_x
                        + f64::from(glyph.left - GLYPH_SIGNED_DISTANCE_FIELD_BORDER_PIXELS as i32)
                            * scale;
                    let bitmap_top_pixels = baseline_y
                        - (f64::from(glyph.top)
                            + GLYPH_FONT_ASCENDER_PIXELS
                            + f64::from(GLYPH_SIGNED_DISTANCE_FIELD_BORDER_PIXELS))
                            * scale
                            * label.pitch_scale_y;
                    let bitmap_width_pixels =
                        f64::from(glyph.bitmap_rectangle.width_pixels) * scale;
                    let bitmap_height_pixels = f64::from(glyph.bitmap_rectangle.height_pixels)
                        * scale
                        * label.pitch_scale_y;
                    if label.halo_width_pixels > 0.0 {
                        // The halo keeps the text quad's geometry: its lower
                        // threshold is what spreads the outline, exactly as
                        // MapLibre Native's `symbol_sdf` shader does.
                        append_glyph_quad(
                            halo_vertices,
                            viewport,
                            [bitmap_left_pixels, bitmap_top_pixels],
                            [bitmap_width_pixels, bitmap_height_pixels],
                            glyph,
                            halo_threshold,
                            edge_gamma,
                            &label.halo_rgba,
                        );
                    }
                    append_glyph_quad(
                        text_vertices,
                        viewport,
                        [bitmap_left_pixels, bitmap_top_pixels],
                        [bitmap_width_pixels, bitmap_height_pixels],
                        glyph,
                        GLYPH_TEXT_SIGNED_DISTANCE_THRESHOLD,
                        edge_gamma,
                        &label.text_rgba,
                    );
                }
                pen_x += advance_pixels + label.letter_spacing_pixels;
            }
        }
    }

    /// Lays out one rotated label's glyph quads: the baseline runs along the
    /// label's rotation from a centered pen origin, and each glyph rectangle
    /// rotates about the label anchor before it becomes clip-space corners.
    fn append_rotated_label_quads(
        &self,
        halo_vertices: &mut Vec<ClipSpaceGlyphVertex>,
        text_vertices: &mut Vec<ClipSpaceGlyphVertex>,
        viewport: &ViewportPixels,
        label: &PlacedLabel,
    ) {
        let scale = label.text_size_pixels / GLYPH_FONT_EM_PIXELS;
        let style_text_size_pixels = label.text_size_pixels / self.device_pixel_ratio;
        let style_halo_width_pixels = label.halo_width_pixels / self.device_pixel_ratio;
        let edge_gamma = glyph_edge_gamma(style_text_size_pixels, self.device_pixel_ratio);
        let halo_threshold =
            glyph_halo_signed_distance_threshold(style_text_size_pixels, style_halo_width_pixels);
        let baseline_offset_pixels =
            label.text_size_pixels * LABEL_BASELINE_OFFSET_FRACTION * label.pitch_scale_y;
        let line_count = label.lines.len() as f64;
        for (line_index, line) in label.lines.iter().enumerate() {
            // Line offsets and pen origins are authored in the label's own
            // unrotated frame; `append_rotated_glyph_quad` rotates the whole
            // offset about the anchor.
            let line_offset_pixels = (line_index as f64 - (line_count - 1.0) / 2.0)
                * label.line_height_pixels
                * label.pitch_scale_y;
            let mut pen_offset_pixels = match label.text_justify {
                TextJustify::Left => -label.block_width_pixels / 2.0,
                TextJustify::Right => label.block_width_pixels / 2.0 - line.width_pixels,
                TextJustify::Auto | TextJustify::Center => -line.width_pixels / 2.0,
            };
            for character in line.text.chars() {
                let Some(glyph) = self.font.glyph(u32::from(character)) else {
                    continue;
                };
                let advance_pixels = f64::from(glyph.advance) * scale;
                if glyph.bitmap_rectangle.width_pixels > 0
                    && glyph.bitmap_rectangle.height_pixels > 0
                {
                    let bitmap_offset_pixels = [
                        pen_offset_pixels
                            + f64::from(
                                glyph.left - GLYPH_SIGNED_DISTANCE_FIELD_BORDER_PIXELS as i32,
                            ) * scale,
                        line_offset_pixels + baseline_offset_pixels
                            - (f64::from(glyph.top)
                                + GLYPH_FONT_ASCENDER_PIXELS
                                + f64::from(GLYPH_SIGNED_DISTANCE_FIELD_BORDER_PIXELS))
                                * scale
                                * label.pitch_scale_y,
                    ];
                    let bitmap_width_pixels =
                        f64::from(glyph.bitmap_rectangle.width_pixels) * scale;
                    let bitmap_height_pixels = f64::from(glyph.bitmap_rectangle.height_pixels)
                        * scale
                        * label.pitch_scale_y;
                    if label.halo_width_pixels > 0.0 {
                        append_rotated_glyph_quad(
                            halo_vertices,
                            viewport,
                            label.anchor_screen_pixels,
                            label.rotation_radians,
                            bitmap_offset_pixels,
                            [bitmap_width_pixels, bitmap_height_pixels],
                            glyph,
                            halo_threshold,
                            edge_gamma,
                            &label.halo_rgba,
                        );
                    }
                    append_rotated_glyph_quad(
                        text_vertices,
                        viewport,
                        label.anchor_screen_pixels,
                        label.rotation_radians,
                        bitmap_offset_pixels,
                        [bitmap_width_pixels, bitmap_height_pixels],
                        glyph,
                        GLYPH_TEXT_SIGNED_DISTANCE_THRESHOLD,
                        edge_gamma,
                        &label.text_rgba,
                    );
                }
                pen_offset_pixels += advance_pixels + label.letter_spacing_pixels;
            }
        }
    }
}

/// Appends one placed badge's solid triangles: the outline color first, then
/// the fill inset by the outline width, so the outline reads as a ring around
/// the reference. A badge without an outline emits only its fill. The badge is
/// centered on the label anchor in the label's own rotated frame, so it follows
/// the feature's geometry rather than staying screen-anchored.
fn append_shield_badge_vertices(
    vertices: &mut Vec<ClipSpaceOverlayVertex>,
    viewport: &ViewportPixels,
    label: &PlacedLabel,
    shield: &PlacedShield,
) {
    let half_width_pixels = shield.width_pixels / 2.0;
    let half_height_pixels = shield.height_pixels / 2.0;
    if half_width_pixels <= 0.0 || half_height_pixels <= 0.0 {
        return;
    }
    let draws_border = shield.border_width_pixels > 0.0
        && shield
            .border_rgba
            .is_some_and(|border_rgba| border_rgba[3] > 0.0);
    if draws_border && let Some(border_rgba) = shield.border_rgba {
        append_badge_outline(
            vertices,
            viewport,
            label,
            shield.shape,
            half_width_pixels,
            half_height_pixels,
            border_rgba,
        );
    }
    let inset_pixels = if draws_border {
        shield.border_width_pixels
    } else {
        0.0
    };
    let inner_half_width_pixels = half_width_pixels - inset_pixels;
    let inner_half_height_pixels = half_height_pixels - inset_pixels;
    if inner_half_width_pixels > 0.0 && inner_half_height_pixels > 0.0 {
        append_badge_outline(
            vertices,
            viewport,
            label,
            shield.shape,
            inner_half_width_pixels,
            inner_half_height_pixels,
            shield.fill_rgba,
        );
    }
}

/// Appends one badge outline as a triangle fan from the badge center. The
/// outline's points are laid out in the badge's own frame and rotated about the
/// label anchor, so a line-placed badge turns with its road.
fn append_badge_outline(
    vertices: &mut Vec<ClipSpaceOverlayVertex>,
    viewport: &ViewportPixels,
    label: &PlacedLabel,
    shape: ShieldShape,
    half_width_pixels: f64,
    half_height_pixels: f64,
    rgba_color: [f32; 4],
) {
    let outline = badge_outline(shape, half_width_pixels, half_height_pixels);
    if outline.len() < 3 {
        return;
    }
    let (rotation_sine, rotation_cosine) = label.rotation_radians.sin_cos();
    let to_screen = |local_point: [f64; 2]| {
        [
            label.anchor_screen_pixels[0] + local_point[0] * rotation_cosine
                - local_point[1] * rotation_sine,
            label.anchor_screen_pixels[1]
                + local_point[0] * rotation_sine
                + local_point[1] * rotation_cosine,
        ]
    };
    let center = label.anchor_screen_pixels;
    vertices.reserve(outline.len() * 3);
    for point_index in 0..outline.len() {
        let next_index = (point_index + 1) % outline.len();
        let triangle = [
            center,
            to_screen(outline[point_index]),
            to_screen(outline[next_index]),
        ];
        for screen_point in triangle {
            vertices.push(ClipSpaceOverlayVertex {
                clip_position: [
                    (2.0 * screen_point[0] / viewport.width_pixels - 1.0) as f32,
                    (1.0 - 2.0 * screen_point[1] / viewport.height_pixels) as f32,
                    0.0,
                    1.0,
                ],
                rgba_color,
            });
        }
    }
}

/// One badge's outline in its own unrotated frame, scaled to the badge's
/// half-extents.
fn badge_outline(
    shape: ShieldShape,
    half_width_pixels: f64,
    half_height_pixels: f64,
) -> Vec<[f64; 2]> {
    match shape {
        ShieldShape::Shield => SHIELD_OUTLINE
            .iter()
            .map(|outline_point| {
                [
                    outline_point[0] * half_width_pixels,
                    outline_point[1] * half_height_pixels,
                ]
            })
            .collect(),
        ShieldShape::RoundedRectangle => {
            rounded_rectangle_outline(half_width_pixels, half_height_pixels)
        }
    }
}

/// A rounded rectangle's outline in the badge's own frame, clockwise from the
/// top-right corner's arc start.
fn rounded_rectangle_outline(half_width_pixels: f64, half_height_pixels: f64) -> Vec<[f64; 2]> {
    let corner_radius_pixels =
        SHIELD_ROUNDED_RECTANGLE_CORNER_FRACTION * half_width_pixels.min(half_height_pixels);
    if corner_radius_pixels <= 0.0 {
        return vec![
            [-half_width_pixels, -half_height_pixels],
            [half_width_pixels, -half_height_pixels],
            [half_width_pixels, half_height_pixels],
            [-half_width_pixels, half_height_pixels],
        ];
    }
    // Corner centers and their arc start/end angles, clockwise from the
    // upper-right corner in screen coordinates (y down).
    let quarter_turn_radians = std::f64::consts::FRAC_PI_2;
    let corners = [
        (
            half_width_pixels - corner_radius_pixels,
            -half_height_pixels + corner_radius_pixels,
            -quarter_turn_radians,
            0.0,
        ),
        (
            half_width_pixels - corner_radius_pixels,
            half_height_pixels - corner_radius_pixels,
            0.0,
            quarter_turn_radians,
        ),
        (
            -half_width_pixels + corner_radius_pixels,
            half_height_pixels - corner_radius_pixels,
            quarter_turn_radians,
            2.0 * quarter_turn_radians,
        ),
        (
            -half_width_pixels + corner_radius_pixels,
            -half_height_pixels + corner_radius_pixels,
            2.0 * quarter_turn_radians,
            3.0 * quarter_turn_radians,
        ),
    ];
    let mut outline =
        Vec::with_capacity(corners.len() * (SHIELD_ROUNDED_RECTANGLE_CORNER_SEGMENTS + 1));
    for (center_x, center_y, start_angle_radians, end_angle_radians) in corners {
        for segment_index in 0..=SHIELD_ROUNDED_RECTANGLE_CORNER_SEGMENTS {
            let progress = segment_index as f64 / SHIELD_ROUNDED_RECTANGLE_CORNER_SEGMENTS as f64;
            let angle_radians =
                start_angle_radians + (end_angle_radians - start_angle_radians) * progress;
            outline.push([
                center_x + corner_radius_pixels * angle_radians.cos(),
                center_y + corner_radius_pixels * angle_radians.sin(),
            ]);
        }
    }
    outline
}

/// One glyph quad's textured vertex, as the GPU pipeline consumes it:
/// clip-space position, atlas texture coordinates, paint, and the
/// signed-distance threshold the fragment stage compares against.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipSpaceGlyphVertex {
    /// Position in clip space, `[x, y, z, w]`.
    pub clip_position: [f32; 4],
    /// Atlas texture coordinates (origin top-left, `u` right, `v` down).
    pub texture_coordinates: [f32; 2],
    /// Paint, components in `[0, 1]`.
    pub rgba_color: [f32; 4],
    /// The signed-distance value the fragment stage treats as covered.
    pub signed_distance_threshold: f32,
    /// The fragment stage's smoothstep half-band in distance-field units
    /// ([`glyph_edge_gamma`]): 0.84 device pixels at every size and ratio.
    pub signed_distance_gamma: f32,
}

/// The three passes of one label layer: badge fill behind, halo quads, then
/// text quads in front.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LabelQuadVertices {
    /// The badge pass, drawn first through the solid overlay pipeline.
    pub badge: Vec<ClipSpaceOverlayVertex>,
    /// The halo pass, drawn after the badges.
    pub halo: Vec<ClipSpaceGlyphVertex>,
    /// The text pass, drawn last.
    pub text: Vec<ClipSpaceGlyphVertex>,
}

impl LabelQuadVertices {
    /// Whether any pass draws anything.
    pub fn is_empty(&self) -> bool {
        self.badge.is_empty() && self.halo.is_empty() && self.text.is_empty()
    }
}

/// One icon quad's textured vertex. The layout matches the renderer's tile
/// quad vertex (clip position, texture coordinates, opacity), so a backend
/// draws icons through its existing textured-quad pipeline with the sprite
/// atlas bound instead of a tile.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipSpaceIconVertex {
    /// Position in clip space, `[x, y, z, w]`.
    pub clip_position: [f32; 4],
    /// Atlas texture coordinates (origin top-left, `u` right, `v` down).
    pub texture_coordinates: [f32; 2],
    /// The icon's alpha, multiplied into the sampled texel's alpha.
    pub opacity: f32,
}

/// The icon pass of one label layer: one four-corner quad per placed icon.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IconQuadVertices {
    /// The icon quads, each in `top-left, top-right, bottom-left,
    /// bottom-right` order.
    pub quads: Vec<[ClipSpaceIconVertex; 4]>,
}

impl IconQuadVertices {
    /// Whether the pass draws anything.
    pub fn is_empty(&self) -> bool {
        self.quads.is_empty()
    }

    /// Serializes the quads into the vertex and index byte streams the
    /// renderer's textured-quad buffers expect: little-endian floats, four
    /// corners per quad, and the shared `0, 1, 2, 2, 1, 3` index pattern.
    pub fn encoded_buffers(&self) -> (Vec<u8>, Vec<u8>) {
        let vertex_stride = std::mem::size_of::<ClipSpaceIconVertex>();
        let mut vertex_bytes =
            Vec::with_capacity(self.quads.len() * vertex_stride * ICON_CORNERS_PER_QUAD);
        let mut index_bytes = Vec::with_capacity(
            self.quads.len() * ICON_INDICES_PER_QUAD * std::mem::size_of::<u16>(),
        );
        for (quad_index, quad) in self.quads.iter().enumerate() {
            let base_corner_index = u16::try_from(quad_index * ICON_CORNERS_PER_QUAD)
                .expect("the icon quad list is bounded by the label budget");
            for vertex in quad {
                for component in vertex
                    .clip_position
                    .iter()
                    .chain(vertex.texture_coordinates.iter())
                    .chain(std::iter::once(&vertex.opacity))
                {
                    vertex_bytes.extend_from_slice(&component.to_le_bytes());
                }
            }
            for index in [
                base_corner_index,
                base_corner_index + 1,
                base_corner_index + 2,
                base_corner_index + 2,
                base_corner_index + 1,
                base_corner_index + 3,
            ] {
                index_bytes.extend_from_slice(&index.to_le_bytes());
            }
        }
        (vertex_bytes, index_bytes)
    }
}

/// Where one label sits on screen: its anchor and the rotation its baseline
/// runs at.
#[derive(Debug, Clone, Copy, PartialEq)]
struct LabelPlacement {
    /// The label's center in viewport pixels.
    anchor_screen_pixels: [f64; 2],
    /// The baseline rotation in radians (clockwise, y down).
    rotation_radians: f64,
}

/// Where one icon sits on screen: its center, drawn size, rotation and the
/// atlas window it samples.
#[derive(Debug, Clone, Copy, PartialEq)]
struct LabelIconPlacement {
    /// The icon's center in viewport pixels.
    anchor_screen_pixels: [f64; 2],
    /// The clockwise rotation in radians.
    rotation_radians: f64,
    /// The drawn width in screen pixels.
    width_pixels: f64,
    /// The drawn height in screen pixels.
    height_pixels: f64,
    /// The atlas window the icon samples.
    texture_rectangle: SpriteTextureRectangle,
}

/// Projects and sizes one sprite icon at the candidate's anchor.
///
/// A missing atlas, a missing index entry, an icon rectangle outside the sheet
/// and a camera that cannot project the anchor all return `None`, so the
/// caller draws no icon and never samples outside the atlas.
#[allow(clippy::too_many_arguments)]
fn icon_placement_at(
    camera: &MapCamera,
    viewport: &ViewportPixels,
    field_of_view_degrees: f64,
    anchor: &GeographicCoordinate,
    icon_candidate: &MapIconCandidate,
    text_block_size_pixels: Option<[f64; 2]>,
    sprite: Option<&SpriteAtlas>,
    device_pixel_ratio: f64,
) -> Option<LabelIconPlacement> {
    let sprite = sprite?;
    let icon = sprite.icon(&icon_candidate.image_name)?;
    let display_size_pixels = icon.display_size_pixels(icon_candidate.size.max(MINIMUM_ICON_SCALE));
    let mut width_pixels = display_size_pixels[0] * device_pixel_ratio;
    let mut height_pixels = display_size_pixels[1] * device_pixel_ratio;
    // `icon-text-fit` stretches the icon's drawn extent (the sheet rectangle
    // keeps its own texels) to the text block's width, height or both, plus
    // the declared inset on each side.
    if let (Some([text_width_pixels, text_height_pixels]), true) = (
        text_block_size_pixels,
        icon_candidate.text_fit != IconTextFit::None,
    ) {
        let [top, right, bottom, left] = icon_candidate.text_fit_padding_pixels;
        let fitted_width_pixels = text_width_pixels + (left + right) * device_pixel_ratio;
        let fitted_height_pixels = text_height_pixels + (top + bottom) * device_pixel_ratio;
        if matches!(
            icon_candidate.text_fit,
            IconTextFit::Width | IconTextFit::Both
        ) {
            width_pixels = fitted_width_pixels;
        }
        if matches!(
            icon_candidate.text_fit,
            IconTextFit::Height | IconTextFit::Both
        ) {
            height_pixels = fitted_height_pixels;
        }
    }
    if width_pixels <= 0.0 || height_pixels <= 0.0 {
        return None;
    }
    let projected_anchor_pixels =
        camera.project_to_screen(anchor, viewport, field_of_view_degrees)?;
    let center_offset =
        text_anchor_center_offset(icon_candidate.anchor, width_pixels, height_pixels);
    let rotation_radians = match icon_candidate.rotation_alignment {
        TextRotationAlignment::Map => camera.bearing_degrees.to_radians(),
        TextRotationAlignment::Viewport | TextRotationAlignment::Auto => 0.0,
    } + icon_candidate.rotation_degrees.to_radians();
    Some(LabelIconPlacement {
        anchor_screen_pixels: [
            projected_anchor_pixels[0]
                + center_offset[0]
                + icon_candidate.offset_pixels[0] * device_pixel_ratio,
            projected_anchor_pixels[1]
                + center_offset[1]
                + icon_candidate.offset_pixels[1] * device_pixel_ratio,
        ],
        rotation_radians,
        width_pixels,
        height_pixels,
        texture_rectangle: sprite.texture_rectangle(icon),
    })
}

/// Places a line-placed label's run along the longest straight run of its
/// projected geometry, or reports an empty list when no run can carry it.
///
/// Vertices the camera cannot project break the polyline: runs are measured
/// over the stretches where every vertex projected. Within one stretch, a
/// straight run is the longest sequence of consecutive segments whose bearing
/// stays within [`LINE_LABEL_STRAIGHT_RUN_ANGLE_TOLERANCE_RADIANS`] of the
/// run's first segment (map data splits a street into short blocks, so a name
/// needs the block sequence, not one block). Of the stretches' runs the
/// longest wins (ties keep the earlier one).
///
/// The text repeats along that run the way the reference engines name a long
/// street: one label centers on the run when only one fits, otherwise labels
/// spread evenly from [`LABEL_COLLISION_PADDING_PIXELS`] past the first end to
/// the same distance before the last, with at least
/// [`LINE_LABEL_REPEAT_SPACING_PIXELS`] of clear space between neighbours.
/// Each label rotates to the bearing of the segment under its own anchor
/// (flipped past [`LINE_LABEL_UPRIGHT_ROTATION_LIMIT_RADIANS`] so the text
/// always reads upright), so a name follows the road's curve rather than one
/// chord across the whole run.
/// The outward direction one text anchor points, used to push a
/// variable-anchored label away from its symbol by the radial offset.
fn radial_offset_direction(text_anchor: TextAnchor) -> [f64; 2] {
    let diagonal = std::f64::consts::FRAC_1_SQRT_2;
    match text_anchor {
        TextAnchor::Center => [0.0, 0.0],
        TextAnchor::Left => [-1.0, 0.0],
        TextAnchor::Right => [1.0, 0.0],
        TextAnchor::Top => [0.0, -1.0],
        TextAnchor::Bottom => [0.0, 1.0],
        TextAnchor::TopLeft => [-diagonal, -diagonal],
        TextAnchor::TopRight => [diagonal, -diagonal],
        TextAnchor::BottomLeft => [-diagonal, diagonal],
        TextAnchor::BottomRight => [diagonal, diagonal],
    }
}

/// How far the block's center sits from the symbol anchor for one
/// `text-anchor`, in screen pixels.
fn text_anchor_center_offset(
    text_anchor: TextAnchor,
    block_width_pixels: f64,
    block_height_pixels: f64,
) -> [f64; 2] {
    let horizontal_shift_pixels = match text_anchor {
        TextAnchor::Left | TextAnchor::TopLeft | TextAnchor::BottomLeft => block_width_pixels / 2.0,
        TextAnchor::Right | TextAnchor::TopRight | TextAnchor::BottomRight => {
            -block_width_pixels / 2.0
        }
        TextAnchor::Center | TextAnchor::Top | TextAnchor::Bottom => 0.0,
    };
    let vertical_shift_pixels = match text_anchor {
        TextAnchor::Top | TextAnchor::TopLeft | TextAnchor::TopRight => block_height_pixels / 2.0,
        TextAnchor::Bottom | TextAnchor::BottomLeft | TextAnchor::BottomRight => {
            -block_height_pixels / 2.0
        }
        TextAnchor::Center | TextAnchor::Left | TextAnchor::Right => 0.0,
    };
    [horizontal_shift_pixels, vertical_shift_pixels]
}

/// The nearest rotation that reads upright: a label turned more than a
/// quarter turn is flipped by half a turn.
fn upright_rotation_radians(rotation_radians: f64) -> f64 {
    let mut upright = rotation_radians;
    while upright > std::f64::consts::FRAC_PI_2 {
        upright -= std::f64::consts::PI;
    }
    while upright < -std::f64::consts::FRAC_PI_2 {
        upright += std::f64::consts::PI;
    }
    upright
}

#[allow(clippy::too_many_arguments)]
fn label_placements_along_line(
    camera: &MapCamera,
    viewport: &ViewportPixels,
    field_of_view_degrees: f64,
    line_coordinates: &[GeographicCoordinate],
    text_width_pixels: f64,
    repeat_spacing_pixels: f64,
    line_center_only: bool,
    angle_tolerance_radians: f64,
) -> Vec<LabelPlacement> {
    let mut projected_stretch: Vec<[f64; 2]> = Vec::new();
    let mut longest_run: Option<StraightRun> = None;
    for coordinate in line_coordinates {
        let Some(screen_point) =
            camera.project_to_screen(coordinate, viewport, field_of_view_degrees)
        else {
            consider_longest_run(
                &projected_stretch,
                &mut longest_run,
                angle_tolerance_radians,
            );
            projected_stretch.clear();
            continue;
        };
        projected_stretch.push(screen_point);
    }
    consider_longest_run(
        &projected_stretch,
        &mut longest_run,
        angle_tolerance_radians,
    );
    let Some(run) = longest_run else {
        return Vec::new();
    };

    let required_run_pixels = text_width_pixels + 2.0 * LABEL_COLLISION_PADDING_PIXELS;
    if run.length_pixels < required_run_pixels {
        return Vec::new();
    }
    let first_anchor_pixels = text_width_pixels / 2.0 + LABEL_COLLISION_PADDING_PIXELS;
    let last_anchor_pixels = run.length_pixels - first_anchor_pixels;
    let repeat_stride_pixels = text_width_pixels + repeat_spacing_pixels;
    let label_count = if line_center_only || last_anchor_pixels <= first_anchor_pixels {
        1
    } else {
        let fitting_spans = ((last_anchor_pixels - first_anchor_pixels) / repeat_stride_pixels)
            .floor()
            .max(0.0);
        (fitting_spans as usize).saturating_add(1)
    };
    let mut placements = Vec::with_capacity(label_count);
    for label_index in 0..label_count {
        let anchor_pixels = if label_count == 1 {
            run.length_pixels / 2.0
        } else {
            let fraction = label_index as f64 / (label_count - 1) as f64;
            first_anchor_pixels + (last_anchor_pixels - first_anchor_pixels) * fraction
        };
        let Some(placement) = placement_at_arc_length(&run.points, anchor_pixels) else {
            continue;
        };
        placements.push(placement);
    }
    placements
}

/// One straight run of a projected polyline: the screen points it spans and
/// its arc length.
#[derive(Debug, Clone)]
struct StraightRun {
    /// The run's projected vertices, in order.
    points: Vec<[f64; 2]>,
    /// The run's total length in screen pixels.
    length_pixels: f64,
}

/// The placement of one label centered at `anchor_pixels` along a projected
/// polyline, walking its segments from the start.
///
/// The anchor sits on the segment that contains the target distance and the
/// rotation follows that segment's own bearing, so a repeated name turns with
/// the road under it. Reports `None` only for a polyline with no segment of
/// positive length.
fn placement_at_arc_length(points: &[[f64; 2]], anchor_pixels: f64) -> Option<LabelPlacement> {
    let mut traveled_pixels = 0.0;
    let mut last_segment: Option<([f64; 2], [f64; 2])> = None;
    for segment_points in points.windows(2) {
        let [segment_start, segment_end] = [segment_points[0], segment_points[1]];
        let segment_length_pixels =
            (segment_end[0] - segment_start[0]).hypot(segment_end[1] - segment_start[1]);
        if segment_length_pixels <= 0.0 {
            continue;
        }
        last_segment = Some((segment_start, segment_end));
        if traveled_pixels + segment_length_pixels >= anchor_pixels {
            let fraction_pixels =
                ((anchor_pixels - traveled_pixels) / segment_length_pixels).clamp(0.0, 1.0);
            let anchor_screen_pixels = [
                segment_start[0] + (segment_end[0] - segment_start[0]) * fraction_pixels,
                segment_start[1] + (segment_end[1] - segment_start[1]) * fraction_pixels,
            ];
            return Some(LabelPlacement {
                anchor_screen_pixels,
                rotation_radians: upright_segment_rotation(segment_start, segment_end),
            });
        }
        traveled_pixels += segment_length_pixels;
    }
    let (segment_start, segment_end) = last_segment?;
    Some(LabelPlacement {
        anchor_screen_pixels: segment_end,
        rotation_radians: upright_segment_rotation(segment_start, segment_end),
    })
}

/// The baseline rotation for a segment, flipped by a half turn when it would
/// put the text upside down, so the result always lies in
/// `[-π/2, π/2]`.
fn upright_segment_rotation(segment_start: [f64; 2], segment_end: [f64; 2]) -> f64 {
    let segment_rotation_radians =
        (segment_end[1] - segment_start[1]).atan2(segment_end[0] - segment_start[0]);
    if segment_rotation_radians > LINE_LABEL_UPRIGHT_ROTATION_LIMIT_RADIANS {
        segment_rotation_radians - std::f64::consts::PI
    } else if segment_rotation_radians < -LINE_LABEL_UPRIGHT_ROTATION_LIMIT_RADIANS {
        segment_rotation_radians + std::f64::consts::PI
    } else {
        segment_rotation_radians
    }
}

/// Appends the longest straight run of one projected stretch to
/// `longest_run` when it beats the runs already measured. A stretch with fewer
/// than two points has no run.
fn consider_longest_run(
    projected_stretch: &[[f64; 2]],
    longest_run: &mut Option<StraightRun>,
    angle_tolerance_radians: f64,
) {
    let mut run_start_index = 0;
    while run_start_index + 1 < projected_stretch.len() {
        let reference_bearing = segment_bearing(
            projected_stretch[run_start_index],
            projected_stretch[run_start_index + 1],
        );
        let mut run_end_index = run_start_index;
        let mut run_length_pixels = 0.0;
        while run_end_index + 1 < projected_stretch.len() {
            let segment_bearing = segment_bearing(
                projected_stretch[run_end_index],
                projected_stretch[run_end_index + 1],
            );
            let bearing_difference = (segment_bearing - reference_bearing + std::f64::consts::PI)
                .rem_euclid(2.0 * std::f64::consts::PI)
                - std::f64::consts::PI;
            if bearing_difference.abs() > angle_tolerance_radians {
                break;
            }
            run_length_pixels += (projected_stretch[run_end_index + 1][0]
                - projected_stretch[run_end_index][0])
                .hypot(
                    projected_stretch[run_end_index + 1][1] - projected_stretch[run_end_index][1],
                );
            run_end_index += 1;
        }
        if run_end_index > run_start_index {
            let is_longest = match longest_run {
                None => true,
                Some(existing_run) => run_length_pixels > existing_run.length_pixels,
            };
            if is_longest {
                *longest_run = Some(StraightRun {
                    points: projected_stretch[run_start_index..=run_end_index].to_vec(),
                    length_pixels: run_length_pixels,
                });
            }
        }
        run_start_index = run_end_index.max(run_start_index + 1);
    }
}

/// The bearing of the segment from `start` to `end`, in radians.
fn segment_bearing(start: [f64; 2], end: [f64; 2]) -> f64 {
    (end[1] - start[1]).atan2(end[0] - start[0])
}

/// One label's padded screen-space collision box, in its own oriented frame.
///
/// A point or line label contributes one box per drawn glyph, and an icon one
/// box of its own. The half extents already include the collision padding (or
/// the icon's `icon-padding`), so the padding travels with each box. The box
/// keeps the label's rotation and two boxes are tested with the
/// separating-axis theorem, so a rotated road name rejects with its oriented
/// extent instead of the empty corners of an axis-aligned bound.
#[derive(Debug, Clone, Copy)]
struct LabelScreenBox {
    /// The box center in viewport pixels (origin top-left, y down).
    center_screen_pixels: [f64; 2],
    /// Half the padded box width along the box's local x axis.
    half_width_pixels: f64,
    /// Half the padded box height along the box's local y axis.
    half_height_pixels: f64,
    /// The clockwise rotation of the box's local axes, in radians.
    rotation_radians: f64,
}

impl LabelScreenBox {
    /// The collision box around a centered glyph or icon, padded by the
    /// caller's declared margin: text carries its `text-padding` and an icon
    /// its `icon-padding`, both measured in screen pixels.
    fn around_with_padding(
        anchor_screen_pixels: [f64; 2],
        width_pixels: f64,
        height_pixels: f64,
        rotation_radians: f64,
        padding_pixels: f64,
    ) -> Self {
        Self {
            center_screen_pixels: anchor_screen_pixels,
            half_width_pixels: width_pixels / 2.0 + padding_pixels,
            half_height_pixels: height_pixels / 2.0 + padding_pixels,
            rotation_radians,
        }
    }

    /// The box's two unit axes in screen space: local x then local y.
    fn axes(&self) -> [[f64; 2]; 2] {
        let (rotation_sine, rotation_cosine) = self.rotation_radians.sin_cos();
        [
            [rotation_cosine, rotation_sine],
            [-rotation_sine, rotation_cosine],
        ]
    }

    /// How far the box extends along a unit axis.
    fn projection_radius(&self, axis: [f64; 2]) -> f64 {
        let [x_axis, y_axis] = self.axes();
        self.half_width_pixels * (axis[0] * x_axis[0] + axis[1] * x_axis[1]).abs()
            + self.half_height_pixels * (axis[0] * y_axis[0] + axis[1] * y_axis[1]).abs()
    }

    /// Whether the two oriented boxes share any area (the separating-axis
    /// theorem over their four candidate axes; touching edges do not count).
    fn intersects(&self, other: &Self) -> bool {
        let [self_x_axis, self_y_axis] = self.axes();
        let [other_x_axis, other_y_axis] = other.axes();
        let center_delta = [
            other.center_screen_pixels[0] - self.center_screen_pixels[0],
            other.center_screen_pixels[1] - self.center_screen_pixels[1],
        ];
        for axis in [self_x_axis, self_y_axis, other_x_axis, other_y_axis] {
            let center_distance = (center_delta[0] * axis[0] + center_delta[1] * axis[1]).abs();
            if center_distance >= self.projection_radius(axis) + other.projection_radius(axis) {
                return false;
            }
        }
        true
    }

    /// The half extents of the box's axis-aligned screen bound.
    fn axis_aligned_half_extents(&self) -> [f64; 2] {
        let (rotation_sine, rotation_cosine) = self.rotation_radians.sin_cos();
        [
            self.half_width_pixels * rotation_cosine.abs()
                + self.half_height_pixels * rotation_sine.abs(),
            self.half_width_pixels * rotation_sine.abs()
                + self.half_height_pixels * rotation_cosine.abs(),
        ]
    }

    /// Whether any part of the box is inside the viewport.
    fn intersects_viewport(&self, viewport: &ViewportPixels) -> bool {
        let [half_width_pixels, half_height_pixels] = self.axis_aligned_half_extents();
        self.center_screen_pixels[0] - half_width_pixels < viewport.width_pixels
            && self.center_screen_pixels[0] + half_width_pixels > 0.0
            && self.center_screen_pixels[1] - half_height_pixels < viewport.height_pixels
            && self.center_screen_pixels[1] + half_height_pixels > 0.0
    }

    /// Whether the box lies entirely inside the viewport; a
    /// `symbol-avoid-edges` label must pass this instead of
    /// [`Self::intersects_viewport`]. The oriented box is stricter than its
    /// axis-aligned bound, so testing the bound never keeps an off-screen
    /// glyph.
    fn is_inside_viewport(&self, viewport: &ViewportPixels) -> bool {
        let [half_width_pixels, half_height_pixels] = self.axis_aligned_half_extents();
        self.center_screen_pixels[0] - half_width_pixels >= 0.0
            && self.center_screen_pixels[0] + half_width_pixels <= viewport.width_pixels
            && self.center_screen_pixels[1] - half_height_pixels >= 0.0
            && self.center_screen_pixels[1] + half_height_pixels <= viewport.height_pixels
    }
}

/// The drawn rectangle of one glyph relative to the label's anchor, in the
/// label's own unrotated frame.
#[derive(Debug, Clone, Copy)]
struct GlyphScreenRectangle {
    /// The rectangle's center offset from the label anchor, in pixels.
    center_offset_pixels: [f64; 2],
    /// The drawn bitmap width in screen pixels.
    width_pixels: f64,
    /// The drawn bitmap height in screen pixels.
    height_pixels: f64,
}

/// Lays out one text block in the label's own unrotated frame and returns one
/// rectangle per drawn glyph, mirroring the quad emitter's pen walk (the same
/// advances, bearings, line stacking and justification), so collision measures
/// exactly the bitmaps the frame draws.
fn glyph_screen_rectangles(
    font: &LabelFont,
    text_block: &LabelTextBlock,
    text_size_pixels: f64,
    letter_spacing_pixels: f64,
    text_justify: TextJustify,
    pitch_scale_y: f64,
) -> Vec<GlyphScreenRectangle> {
    let scale = text_size_pixels / GLYPH_FONT_EM_PIXELS;
    let baseline_offset_pixels = text_size_pixels * LABEL_BASELINE_OFFSET_FRACTION * pitch_scale_y;
    let line_count = text_block.lines.len() as f64;
    let mut rectangles = Vec::new();
    for (line_index, line) in text_block.lines.iter().enumerate() {
        let line_offset_pixels = (line_index as f64 - (line_count - 1.0) / 2.0)
            * text_block.line_height_pixels
            * pitch_scale_y;
        let mut pen_offset_pixels = match text_justify {
            TextJustify::Left => -text_block.width_pixels / 2.0,
            TextJustify::Right => text_block.width_pixels / 2.0 - line.width_pixels,
            TextJustify::Auto | TextJustify::Center => -line.width_pixels / 2.0,
        };
        for character in line.text.chars() {
            let Some(glyph) = font.glyph(u32::from(character)) else {
                continue;
            };
            let advance_pixels = f64::from(glyph.advance) * scale;
            if glyph.bitmap_rectangle.width_pixels > 0 && glyph.bitmap_rectangle.height_pixels > 0 {
                let bitmap_left_pixels = pen_offset_pixels
                    + f64::from(glyph.left - GLYPH_SIGNED_DISTANCE_FIELD_BORDER_PIXELS as i32)
                        * scale;
                let bitmap_top_pixels = line_offset_pixels + baseline_offset_pixels
                    - (f64::from(glyph.top)
                        + GLYPH_FONT_ASCENDER_PIXELS
                        + f64::from(GLYPH_SIGNED_DISTANCE_FIELD_BORDER_PIXELS))
                        * scale
                        * pitch_scale_y;
                let width_pixels = f64::from(glyph.bitmap_rectangle.width_pixels) * scale;
                let height_pixels =
                    f64::from(glyph.bitmap_rectangle.height_pixels) * scale * pitch_scale_y;
                rectangles.push(GlyphScreenRectangle {
                    center_offset_pixels: [
                        bitmap_left_pixels + width_pixels / 2.0,
                        bitmap_top_pixels + height_pixels / 2.0,
                    ],
                    width_pixels,
                    height_pixels,
                });
            }
            pen_offset_pixels += advance_pixels + letter_spacing_pixels;
        }
    }
    rectangles
}

/// The padded collision boxes of one placement's glyphs, rotated with the
/// label about its anchor. A block whose glyphs all lack bitmaps falls back to
/// one box around the measured text block, so the label still reserves space.
#[allow(clippy::too_many_arguments)]
fn glyph_collision_boxes(
    font: &LabelFont,
    text_block: &LabelTextBlock,
    text_size_pixels: f64,
    letter_spacing_pixels: f64,
    text_justify: TextJustify,
    padding_pixels: f64,
    pitch_scale_y: f64,
    placement: LabelPlacement,
) -> Vec<LabelScreenBox> {
    let glyph_rectangles = glyph_screen_rectangles(
        font,
        text_block,
        text_size_pixels,
        letter_spacing_pixels,
        text_justify,
        pitch_scale_y,
    );
    if glyph_rectangles.is_empty() {
        return vec![LabelScreenBox::around_with_padding(
            placement.anchor_screen_pixels,
            text_block.width_pixels,
            text_block.height_pixels * pitch_scale_y,
            placement.rotation_radians,
            padding_pixels,
        )];
    }
    let (rotation_sine, rotation_cosine) = placement.rotation_radians.sin_cos();
    glyph_rectangles
        .into_iter()
        .map(|glyph_rectangle| {
            let [offset_x, offset_y] = glyph_rectangle.center_offset_pixels;
            let center_screen_pixels = [
                placement.anchor_screen_pixels[0] + offset_x * rotation_cosine
                    - offset_y * rotation_sine,
                placement.anchor_screen_pixels[1]
                    + offset_x * rotation_sine
                    + offset_y * rotation_cosine,
            ];
            LabelScreenBox::around_with_padding(
                center_screen_pixels,
                glyph_rectangle.width_pixels,
                glyph_rectangle.height_pixels,
                placement.rotation_radians,
                padding_pixels,
            )
        })
        .collect()
}

/// Appends one glyph bitmap as two screen-space triangles (the shared
/// `0, 1, 2, 2, 1, 3` corner pattern) converted to clip space.
// The arguments mirror `append_rotated_glyph_quad`'s, minus the anchor and
// rotation; a struct would only move the same values to the call site.
#[allow(clippy::too_many_arguments)]
fn append_glyph_quad(
    vertices: &mut Vec<ClipSpaceGlyphVertex>,
    viewport: &ViewportPixels,
    top_left_pixels: [f64; 2],
    size_pixels: [f64; 2],
    glyph: &PackedGlyph,
    signed_distance_threshold: f32,
    signed_distance_gamma: f32,
    rgba_color: &[f32; 4],
) {
    let corners = axis_aligned_corners(top_left_pixels, size_pixels);
    append_glyph_quad_corners(
        vertices,
        viewport,
        &corners,
        glyph.uv_rectangle,
        signed_distance_threshold,
        signed_distance_gamma,
        rgba_color,
    );
}

/// Appends one rotated glyph bitmap: the glyph rectangle is laid out in the
/// label's own frame (offset from the anchor), rotated about the anchor by
/// `rotation_radians`, and emitted as the same two screen-space triangles the
/// unrotated path draws.
// The arguments mirror `append_glyph_quad`'s, plus the anchor and rotation;
// splitting them into a struct would only move the same values to the call
// site.
#[allow(clippy::too_many_arguments)]
fn append_rotated_glyph_quad(
    vertices: &mut Vec<ClipSpaceGlyphVertex>,
    viewport: &ViewportPixels,
    anchor_screen_pixels: [f64; 2],
    rotation_radians: f64,
    top_left_offset_pixels: [f64; 2],
    size_pixels: [f64; 2],
    glyph: &PackedGlyph,
    signed_distance_threshold: f32,
    signed_distance_gamma: f32,
    rgba_color: &[f32; 4],
) {
    let (rotation_sine, rotation_cosine) = rotation_radians.sin_cos();
    let local_corners = axis_aligned_corners(top_left_offset_pixels, size_pixels);
    let rotated_corners = local_corners.map(|local_corner| {
        [
            anchor_screen_pixels[0] + local_corner[0] * rotation_cosine
                - local_corner[1] * rotation_sine,
            anchor_screen_pixels[1]
                + local_corner[0] * rotation_sine
                + local_corner[1] * rotation_cosine,
        ]
    });
    append_glyph_quad_corners(
        vertices,
        viewport,
        &rotated_corners,
        glyph.uv_rectangle,
        signed_distance_threshold,
        signed_distance_gamma,
        rgba_color,
    );
}

/// The four screen-space corners of an axis-aligned rectangle in the
/// `top-left, top-right, bottom-left, bottom-right` order the shared index
/// pattern consumes.
fn axis_aligned_corners(top_left_pixels: [f64; 2], size_pixels: [f64; 2]) -> [[f64; 2]; 4] {
    let right_pixels = top_left_pixels[0] + size_pixels[0];
    let bottom_pixels = top_left_pixels[1] + size_pixels[1];
    [
        [top_left_pixels[0], top_left_pixels[1]],
        [right_pixels, top_left_pixels[1]],
        [top_left_pixels[0], bottom_pixels],
        [right_pixels, bottom_pixels],
    ]
}

/// Emits one glyph bitmap from its four screen-space corners, in the shared
/// `top-left, top-right, bottom-left, bottom-right` order, as two triangles.
fn append_glyph_quad_corners(
    vertices: &mut Vec<ClipSpaceGlyphVertex>,
    viewport: &ViewportPixels,
    screen_corners: &[[f64; 2]; 4],
    texture_coordinates: rustic_maps_glyphs::GlyphUvRectangle,
    signed_distance_threshold: f32,
    signed_distance_gamma: f32,
    rgba_color: &[f32; 4],
) {
    let corner_texture_coordinates = [
        [texture_coordinates.minimum_u, texture_coordinates.minimum_v],
        [texture_coordinates.maximum_u, texture_coordinates.minimum_v],
        [texture_coordinates.minimum_u, texture_coordinates.maximum_v],
        [texture_coordinates.maximum_u, texture_coordinates.maximum_v],
    ];
    for corner_index in [0usize, 1, 2, 2, 1, 3] {
        let screen_position = screen_corners[corner_index];
        vertices.push(ClipSpaceGlyphVertex {
            clip_position: [
                (2.0 * screen_position[0] / viewport.width_pixels - 1.0) as f32,
                (1.0 - 2.0 * screen_position[1] / viewport.height_pixels) as f32,
                0.0,
                1.0,
            ],
            texture_coordinates: corner_texture_coordinates[corner_index],
            rgba_color: *rgba_color,
            signed_distance_threshold,
            signed_distance_gamma,
        });
    }
}

/// Reads the label candidates from one decoded tile under the style's symbol
/// layers, at an explicit style zoom level (the frame camera's zoom, so size
/// stops evaluate where the camera is rather than where the tile belongs).
///
/// A feature becomes a candidate only when its `rank` passes the layer's
/// zoom-stepped rank ceiling and its `class` is visible at `style_zoom_level`
/// under the layer's per-class minimum zooms, so a busy poi layer draws its
/// important places at a wide zoom and its shops only once the map is closer.
/// A point-placed symbol layer then produces one candidate per feature point;
/// a line-placed one produces one candidate carrying each feature's line
/// geometry, which the frame places along its longest projected segment.
/// Features of a geometry kind the placement cannot use paint nothing for
/// that symbol layer, exactly like the overlay conversion.
pub fn label_candidates_for_vector_tile(
    tile: &VectorTile,
    tile_address: VectorTileAddress,
    style: &CompiledStyle,
    style_zoom_level: f64,
) -> Vec<MapLabelCandidate> {
    let mut candidates = Vec::new();
    for layer in &style.layers {
        candidates.extend(label_candidates_for_compiled_layer_at_zoom(
            tile,
            tile_address,
            layer,
            style_zoom_level,
        ));
    }
    candidates
}

/// Reads the label candidates one compiled symbol layer contributes from one
/// decoded tile.
///
/// A multi-source frame styles each source's tiles independently and calls this
/// per layer, so a symbol layer never reads geometry fetched for another
/// source. A non-symbol layer, an invisible layer and a layer without text
/// contribute nothing.
pub fn label_candidates_for_compiled_layer_at_zoom(
    tile: &VectorTile,
    tile_address: VectorTileAddress,
    layer: &rustic_maps_style::CompiledLayer,
    style_zoom_level: f64,
) -> Vec<MapLabelCandidate> {
    label_candidates_for_compiled_layer_at_zoom_with_transition(
        tile,
        tile_address,
        layer,
        style_zoom_level,
        None,
    )
}

/// [`label_candidates_for_compiled_layer_at_zoom`] with a style transition
/// applied to each candidate's paint.
///
/// `transition` carries the same layer under the style being replaced, the
/// document's root timing and the elapsed seconds since the change. Every
/// candidate's text, halo and icon colours and opacities blend from the
/// previous style's values, so a style change cross-fades its labels over the
/// declared transition instead of swapping them at the first frame.
pub fn label_candidates_for_compiled_layer_at_zoom_with_transition(
    tile: &VectorTile,
    tile_address: VectorTileAddress,
    layer: &CompiledLayer,
    style_zoom_level: f64,
    transition: Option<(&CompiledLayer, Option<TransitionSpec>, f64)>,
) -> Vec<MapLabelCandidate> {
    let mut candidates = Vec::new();
    if layer.kind != CompiledLayerKind::Symbol || !layer.is_visible_at_zoom(style_zoom_level) {
        return candidates;
    }
    for tile_layer in crate::vector_style::tile_layers_for_compiled_layer(tile, layer) {
        for feature in &tile_layer.features {
            let properties = feature_properties(feature);
            let context = feature_evaluation_context(feature, &properties, style_zoom_level);
            if !layer.matches_filter(&context) {
                continue;
            }
            let layout = layer.resolved_symbol_layout(&context);
            let resolved_paint = match transition {
                Some((previous_layer, root_transition, elapsed_seconds)) => layer
                    .resolved_transitioned_paint(
                        &context,
                        root_transition,
                        Some((previous_layer, elapsed_seconds)),
                    ),
                None => layer.resolved_paint(&context),
            };
            let ResolvedPaint::Symbol(paint) = resolved_paint else {
                continue;
            };
            let icon = layout
                .icon_image
                .as_ref()
                .map(|image_name| MapIconCandidate {
                    image_name: image_name.clone(),
                    size: layout.icon_size,
                    anchor: layout.icon_anchor,
                    offset_pixels: [
                        layout.icon_offset[0] * ICON_OFFSET_EM_PIXELS,
                        layout.icon_offset[1] * ICON_OFFSET_EM_PIXELS,
                    ],
                    rotation_degrees: layout.icon_rotate_degrees,
                    rotation_alignment: layout.icon_rotation_alignment,
                    padding_pixels: layout.icon_padding,
                    text_fit: layout.icon_text_fit,
                    text_fit_padding_pixels: layout.icon_text_fit_padding,
                    opacity: paint.icon_opacity as f32,
                    allow_overlap: layout.icon_allow_overlap,
                    ignore_placement: layout.icon_ignore_placement,
                });
            if layout.text.is_empty() && icon.is_none() {
                continue;
            }
            let text_rgba = resolved_rgba(paint.text_color, paint.text_opacity);
            let halo_rgba = resolved_rgba(paint.halo_color, 1.0);
            let shield = layer.shield.as_ref().map(|shield| LabelShield {
                shape: shield.shape,
                fill_rgba: resolved_rgba(shield.fill_color, 1.0),
                border_rgba: shield
                    .border_color
                    .map(|border_color| resolved_rgba(border_color, 1.0)),
                border_width_pixels: shield.border_width_pixels,
                padding_pixels: shield.padding_pixels,
                minimum_size_pixels: shield.minimum_size_pixels,
            });
            let priority = symbol_sort_key_priority(layout.sort_key);
            let rank = label_rank_for_feature(feature);
            let ignore_placement = layout.ignore_placement;
            let avoid_edges = layout.avoid_edges;
            let text_offset_pixels = [
                layout.text_offset[0] * layout.text_size,
                layout.text_offset[1] * layout.text_size,
            ];
            let repeat_spacing_pixels = if layer.declares_symbol_spacing {
                layout.symbol_spacing
            } else {
                LINE_LABEL_REPEAT_SPACING_PIXELS
            };
            let line_center_only = layout.placement == SymbolPlacement::LineCenter;
            let text_max_width_ems = layout.text_max_width;
            let text_line_height_ems = layout.text_line_height;
            let text_letter_spacing_ems = layout.text_letter_spacing;
            let text_justify = layout.text_justify;
            let text_anchor = layout.text_anchor;
            let text_rotation_alignment = layout.text_rotation_alignment;
            let text_keep_upright = layout.text_keep_upright;
            let text_variable_anchors = layout.variable_anchors.clone();
            let text_radial_offset_ems = layout.radial_offset;
            let text_max_angle_radians = if layer.declares_text_max_angle {
                layout
                    .max_angle_degrees
                    .to_radians()
                    .clamp(0.0, std::f64::consts::PI)
            } else {
                LINE_LABEL_STRAIGHT_RUN_ANGLE_TOLERANCE_RADIANS
            };
            match layout.placement {
                SymbolPlacement::Point => {
                    let VectorTileGeometry::Points(points) = &feature.geometry else {
                        continue;
                    };
                    for point in points {
                        candidates.push(MapLabelCandidate {
                            text: layout.text.clone(),
                            anchor: tile_local_point_to_coordinate(
                                tile_address,
                                tile_layer.extent,
                                point,
                            ),
                            line_coordinates: Vec::new(),
                            text_rgba,
                            text_size_pixels: layout.text_size,
                            halo_rgba,
                            halo_width_pixels: paint.halo_width,
                            priority,
                            rank,
                            allow_overlap: layout.allow_overlap,
                            ignore_placement,
                            avoid_edges,
                            text_offset_pixels,
                            text_padding_pixels: layout.text_padding,
                            text_pitch_alignment: layout.text_pitch_alignment,
                            repeat_spacing_pixels,
                            line_center_only,
                            text_max_width_ems,
                            text_line_height_ems,
                            text_letter_spacing_ems,
                            text_justify,
                            text_anchor,
                            text_rotation_alignment,
                            text_keep_upright,
                            text_variable_anchors: text_variable_anchors.clone(),
                            text_radial_offset_ems,
                            text_max_angle_radians,
                            style_order: layer.order_index,
                            icon: icon.clone(),
                            shield,
                        });
                    }
                }
                SymbolPlacement::Line | SymbolPlacement::LineCenter => {
                    let VectorTileGeometry::LineStrings(line_strings) = &feature.geometry else {
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
                                    tile_layer.extent,
                                    point,
                                )
                            })
                            .collect();
                        let anchor = line_coordinates[0];
                        candidates.push(MapLabelCandidate {
                            text: layout.text.clone(),
                            anchor,
                            line_coordinates,
                            text_rgba,
                            text_size_pixels: layout.text_size,
                            halo_rgba,
                            halo_width_pixels: paint.halo_width,
                            priority,
                            rank,
                            allow_overlap: layout.allow_overlap,
                            ignore_placement,
                            avoid_edges,
                            text_offset_pixels,
                            text_padding_pixels: layout.text_padding,
                            text_pitch_alignment: layout.text_pitch_alignment,
                            repeat_spacing_pixels,
                            line_center_only,
                            text_max_width_ems,
                            text_line_height_ems,
                            text_letter_spacing_ems,
                            text_justify,
                            text_anchor,
                            text_rotation_alignment,
                            text_keep_upright,
                            text_variable_anchors: text_variable_anchors.clone(),
                            text_radial_offset_ems,
                            text_max_angle_radians,
                            style_order: layer.order_index,
                            icon: icon.clone(),
                            shield,
                        });
                    }
                }
            }
        }
    }
    candidates
}

/// The compiled placement key as the candidate's whole-number priority. The
/// style specification allows a fractional sort key; the label pass orders by
/// an integer column, so a fractional key truncates toward zero and a negative
/// or oversized key clamps into the column's range.
fn symbol_sort_key_priority(sort_key: f64) -> u32 {
    if !sort_key.is_finite() || sort_key <= 0.0 {
        0
    } else if sort_key >= f64::from(u32::MAX) {
        u32::MAX
    } else {
        sort_key as u32
    }
}

/// The feature's `rank` attribute as a whole number, or [`DEFAULT_LABEL_RANK`]
/// when it carries none (negative ranks clamp to zero).
fn label_rank_for_feature(feature: &VectorTileFeature) -> u32 {
    feature
        .attributes
        .iter()
        .find(|(attribute_name, _)| attribute_name == LABEL_RANK_ATTRIBUTE_NAME)
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
        .unwrap_or(DEFAULT_LABEL_RANK)
}

#[cfg(test)]
mod tests;
