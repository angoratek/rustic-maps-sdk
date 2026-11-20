use super::*;
use crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES;
use crate::vector_style::{
    VECTOR_STYLE_ID_BRIGHT_LIGHT, vector_tile_style_from_id, vector_tile_style_from_json,
};
use rustic_maps_glyphs::{Glyph, decode_glyph_font_stack};
use rustic_maps_test_support::{SACRAMENTO_LATITUDE_DEGREES, SACRAMENTO_LONGITUDE_DEGREES};
use rustic_maps_vector_tiles::decode_vector_tile;

/// Compiles a small inline style document for the symbol-pass tests.
fn compiled_style(style_json: &str) -> CompiledStyle {
    vector_tile_style_from_json(style_json).expect("the test style document compiles")
}

/// The bundled OpenFreeMap z14 vector tile over Sacramento: the demo
/// tile whose place layer names Sacramento, West Sacramento, Mikon and
/// Bryte.
const OPENFREEMAP_TILE_BYTES: &[u8] =
    include_bytes!("../../../../fixtures/openfreemap/openmaptiles_tile_z14_x2661_y6286.pbf");
/// The bundled Latin glyph range.
const GLYPH_RANGE_BYTES: &[u8] =
    include_bytes!("../../../../fixtures/glyphs/noto_sans_regular_0-255.pbf");
/// How far past the viewport edge the avoid-edges test places a label, in
/// screen pixels: enough to cross, too little to miss the frame.
const LABEL_EDGE_TOLERANCE_PIXELS: f64 = 4.0;

const TILE_ADDRESS: VectorTileAddress = VectorTileAddress {
    zoom_level: 14,
    tile_x: 2661,
    tile_y: 6286,
};
/// The center of the sample tile (`2661.5`, `6286.5` in z14 tile units):
/// a camera there shows the whole tile in the test viewport.
const FIXTURE_TILE_CENTER_LATITUDE_DEGREES: f64 = 38.573_937_515_575_92;
/// The sample tile center's longitude.
const FIXTURE_TILE_CENTER_LONGITUDE_DEGREES: f64 = -121.519_775_390_625;

fn fixture_font() -> LabelFont {
    let font_stack = decode_glyph_font_stack(GLYPH_RANGE_BYTES).expect("the fixture decodes");
    LabelFont::from_glyph_font_stack(&font_stack).expect("the Latin range packs")
}

#[test]
fn the_embedded_default_glyph_range_is_the_committed_fixture() {
    assert_eq!(
        DEFAULT_LATIN_GLYPH_RANGE_PBF_BYTES, GLYPH_RANGE_BYTES,
        "the embedded glyph source must stay byte-identical to the fixture the \
         golden and browser label suites read"
    );
}

#[test]
fn the_default_label_font_serves_latin_glyphs_and_is_shared() {
    let font = default_label_font();
    assert!(
        font.glyph(u32::from('S')).is_some(),
        "the default font must serve the Latin letters place names use"
    );
    let metrics = font
        .measure_text("Sacramento", 13.0)
        .expect("every character of the demo name has a glyph");
    assert!(metrics.width_pixels > 0.0);
    assert!(
        std::ptr::eq(font, default_label_font()),
        "the packed atlas is decoded and packed once, then shared"
    );
}

#[test]
fn glyph_lookup_finds_glyphs_in_both_packed_groups() {
    // The packer orders the Latin ranges first, then every other range;
    // the binary-search lookup must find a glyph in either group and
    // miss cleanly for an unserved codepoint.
    let font_stack = GlyphFontStack {
        name: "Test Glyphs".to_string(),
        range: "mixed".to_string(),
        glyphs: vec![
            Glyph {
                codepoint: u32::from('A'),
                advance: 6,
                ..Glyph::default()
            },
            Glyph {
                codepoint: u32::from('B'),
                advance: 7,
                ..Glyph::default()
            },
            Glyph {
                codepoint: u32::from('Ω'),
                advance: 8,
                ..Glyph::default()
            },
        ],
    };
    let font = LabelFont::from_glyph_font_stack(&font_stack).expect("the glyph set packs");
    assert_eq!(
        font.glyph(u32::from('A')).map(|glyph| glyph.advance),
        Some(6)
    );
    assert_eq!(
        font.glyph(u32::from('B')).map(|glyph| glyph.advance),
        Some(7)
    );
    assert_eq!(
        font.glyph(u32::from('Ω')).map(|glyph| glyph.advance),
        Some(8)
    );
    assert!(
        font.glyph(u32::from('Z')).is_none(),
        "an unserved codepoint misses without scanning a linear list"
    );
}

fn sacramento_camera() -> MapCamera {
    MapCamera::new(
        GeographicCoordinate::from_degrees(
            SACRAMENTO_LATITUDE_DEGREES,
            SACRAMENTO_LONGITUDE_DEGREES,
        )
        .unwrap(),
        14.0,
        0.0,
        0.0,
    )
}

fn bright_candidates() -> Vec<MapLabelCandidate> {
    bright_candidates_at_zoom(14.0)
}

fn bright_candidates_at_zoom(style_zoom_level: f64) -> Vec<MapLabelCandidate> {
    let tile = decode_vector_tile(OPENFREEMAP_TILE_BYTES).expect("the fixture decodes");
    let style = vector_tile_style_from_id(VECTOR_STYLE_ID_BRIGHT_LIGHT).unwrap();
    label_candidates_for_vector_tile(&tile, TILE_ADDRESS, &style, style_zoom_level)
}

fn candidate_texts(candidates: &[MapLabelCandidate]) -> Vec<&str> {
    candidates
        .iter()
        .map(|candidate| candidate.text.as_str())
        .collect()
}

#[test]
fn the_demo_tile_reads_the_four_documented_place_names() {
    let candidates = bright_candidates();
    let texts = candidate_texts(&candidates);
    for expected_text in ["Sacramento", "West Sacramento", "Mikon", "Bryte"] {
        assert!(
            texts.contains(&expected_text),
            "the Bright symbol layers must read {expected_text:?} from the demo tile"
        );
    }
    assert!(
        texts.contains(&"Downtown"),
        "a neighbourhood name keeps drawing at the demo zoom"
    );
}

#[test]
fn placing_the_frame_keeps_sacramento_and_its_font_is_available() {
    let font = fixture_font();
    assert!(font.glyph(u32::from('S')).is_some());
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let layer = LabelLayer::place(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &bright_candidates(),
        &font,
    );
    assert!(
        layer
            .labels()
            .iter()
            .any(|label| label.text == "Sacramento"),
        "the Sacramento label anchors at the frame center"
    );
    let vertices = layer.quad_vertices(&viewport);
    assert!(
        !vertices.text.is_empty(),
        "placed labels produce text quads"
    );
    assert_eq!(
        vertices.text.len() % 6,
        0,
        "every glyph contributes two triangles"
    );
    for vertex in &vertices.text {
        assert!(
            vertex
                .clip_position
                .iter()
                .all(|component| component.is_finite())
        );
        assert_eq!(
            vertex.signed_distance_threshold,
            GLYPH_TEXT_SIGNED_DISTANCE_THRESHOLD
        );
    }
}

/// A point-placed route-reference candidate carrying one badge.
fn shield_candidate(text: &str, shape: ShieldShape, minimum_size_pixels: f64) -> MapLabelCandidate {
    MapLabelCandidate {
        text: text.to_string(),
        anchor: GeographicCoordinate::from_degrees(
            SACRAMENTO_LATITUDE_DEGREES,
            SACRAMENTO_LONGITUDE_DEGREES,
        )
        .unwrap(),
        line_coordinates: Vec::new(),
        text_rgba: [0.0, 0.0, 0.0, 1.0],
        text_size_pixels: 12.0,
        halo_rgba: [1.0, 1.0, 1.0, 1.0],
        halo_width_pixels: 0.0,
        priority: 0,
        rank: DEFAULT_LABEL_RANK,
        allow_overlap: true,
        ignore_placement: true,
        avoid_edges: false,
        text_offset_pixels: [0.0, 0.0],
        text_padding_pixels: LABEL_COLLISION_PADDING_PIXELS,
        text_pitch_alignment: TextPitchAlignment::Viewport,
        repeat_spacing_pixels: LINE_LABEL_REPEAT_SPACING_PIXELS,
        line_center_only: false,
        text_max_width_ems: 10.0,
        text_line_height_ems: 1.2,
        text_letter_spacing_ems: 0.0,
        text_justify: TextJustify::Auto,
        text_anchor: TextAnchor::Center,
        text_rotation_alignment: TextRotationAlignment::Viewport,
        text_keep_upright: true,
        text_variable_anchors: Vec::new(),
        text_radial_offset_ems: 0.0,
        text_max_angle_radians: LINE_LABEL_STRAIGHT_RUN_ANGLE_TOLERANCE_RADIANS,
        style_order: 0,
        icon: None,
        shield: Some(LabelShield {
            shape,
            fill_rgba: [1.0, 1.0, 1.0, 1.0],
            border_rgba: Some([0.0, 0.0, 0.0, 1.0]),
            border_width_pixels: 1.0,
            padding_pixels: 3.0,
            minimum_size_pixels,
        }),
    }
}

/// Places one shield candidate and returns the layer and its badge.
fn place_shield_candidate(
    text: &str,
    shape: ShieldShape,
    minimum_size_pixels: f64,
) -> (LabelLayer, PlacedShield) {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let layer = LabelLayer::place(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &[shield_candidate(text, shape, minimum_size_pixels)],
        &font,
    );
    let placed = layer.labels().first().expect("the shield candidate places");
    let shield = placed.shield.expect("the placed label carries its badge");
    (layer, shield)
}

/// A line-placed route-reference candidate carrying the same badge as
/// [`shield_candidate`], along the supplied segment.
fn line_shield_candidate(
    text: &str,
    line_coordinates: Vec<GeographicCoordinate>,
) -> MapLabelCandidate {
    let mut candidate = shield_candidate(text, ShieldShape::Shield, 0.0);
    candidate.anchor = line_coordinates[0];
    candidate.line_coordinates = line_coordinates;
    // A line label follows the road's bearing; `auto` keeps the rotation
    // the upright segment rule produces.
    candidate.text_rotation_alignment = TextRotationAlignment::Auto;
    candidate
}

/// The axis-aligned screen extent of one badge pass.
fn badge_screen_extent(
    vertices: &[ClipSpaceOverlayVertex],
    viewport: &ViewportPixels,
) -> ([f64; 2], [f64; 2]) {
    let mut minimum = [f64::INFINITY; 2];
    let mut maximum = [f64::NEG_INFINITY; 2];
    for vertex in vertices {
        let screen_x = f64::from(vertex.clip_position[0] + 1.0) / 2.0 * viewport.width_pixels;
        let screen_y = (1.0 - f64::from(vertex.clip_position[1])) / 2.0 * viewport.height_pixels;
        minimum[0] = minimum[0].min(screen_x);
        minimum[1] = minimum[1].min(screen_y);
        maximum[0] = maximum[0].max(screen_x);
        maximum[1] = maximum[1].max(screen_y);
    }
    (minimum, maximum)
}

#[test]
fn a_rounded_rectangle_badge_encloses_its_reference_and_padding() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let (layer, shield) = place_shield_candidate("50", ShieldShape::RoundedRectangle, 0.0);
    let reference_width_pixels = font.measure_text("50", 12.0).unwrap().width_pixels;
    let one_line_height_pixels = 12.0 * 1.2;
    assert!(
        (shield.width_pixels - (reference_width_pixels + 6.0)).abs() < 1e-9,
        "a two-glyph badge is its reference plus padding at each side"
    );
    assert!(
        (shield.height_pixels - (one_line_height_pixels + 6.0)).abs() < 1e-9,
        "a two-glyph badge is one line high plus padding"
    );
    let vertices = layer.quad_vertices(&viewport);
    assert!(!vertices.badge.is_empty(), "the badge emits triangles");
    assert!(!vertices.text.is_empty(), "the reference still draws");
    let (minimum, maximum) = badge_screen_extent(&vertices.badge, &viewport);
    assert!((maximum[0] - minimum[0] - shield.width_pixels).abs() < 1e-3);
    assert!((maximum[1] - minimum[1] - shield.height_pixels).abs() < 1e-3);
}

#[test]
fn a_shield_badge_grows_past_its_minimum_for_a_long_reference() {
    // A minimum wide enough to dominate a two-glyph reference but not a
    // four-glyph one, so one test proves both the floor and the growth.
    const BADGE_MINIMUM_SIZE_PIXELS: f64 = 24.0;
    let font = fixture_font();
    let two_glyph_reference_width_pixels = font.measure_text("50", 12.0).unwrap().width_pixels;
    let four_glyph_reference_width_pixels = font.measure_text("US 50", 12.0).unwrap().width_pixels;
    let (_, small_shield) =
        place_shield_candidate("50", ShieldShape::Shield, BADGE_MINIMUM_SIZE_PIXELS);
    assert!(
        (small_shield.width_pixels - BADGE_MINIMUM_SIZE_PIXELS).abs() < 1e-9
            && (small_shield.height_pixels - BADGE_MINIMUM_SIZE_PIXELS).abs() < 1e-9,
        "a short reference fills the badge's minimum size"
    );
    let (_, long_shield) =
        place_shield_candidate("US 50", ShieldShape::Shield, BADGE_MINIMUM_SIZE_PIXELS);
    assert!(
        long_shield.width_pixels > BADGE_MINIMUM_SIZE_PIXELS,
        "a four-glyph reference grows the badge past its minimum"
    );
    assert!(
        (long_shield.width_pixels - (four_glyph_reference_width_pixels + 6.0)).abs() < 1e-9,
        "the grown badge still tracks the measured reference plus padding"
    );
    assert!(two_glyph_reference_width_pixels < four_glyph_reference_width_pixels);
}

#[test]
fn a_shield_badge_draws_its_outline_ring_and_fill_in_two_fans() {
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let (layer, shield) = place_shield_candidate("I 80", ShieldShape::Shield, 0.0);
    let vertices = layer.quad_vertices(&viewport);
    let border_fan_vertices = SHIELD_OUTLINE.len() * 3;
    assert_eq!(
        vertices.badge.len(),
        border_fan_vertices * 2,
        "an outlined shield draws the outline and the inset fill"
    );
    let (minimum, maximum) = badge_screen_extent(&vertices.badge, &viewport);
    assert!((maximum[0] - minimum[0] - shield.width_pixels).abs() < 1e-3);
    assert!((maximum[1] - minimum[1] - shield.height_pixels).abs() < 1e-3);
    // The reference draws above the badge: the two passes are separate so a
    // renderer emits the badge first and the glyphs after it.
    assert!(!vertices.text.is_empty());
}

#[test]
fn a_line_placed_badge_stays_upright_for_every_segment_direction() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    // Two points on each side of the fixture anchor, walked east, west,
    // north and south. A westbound segment's raw bearing is π (upside
    // down); the upright rule must flip it, and the badge's bottom point
    // must then stay at or below its anchor rather than above it.
    let segments: [(&str, Vec<GeographicCoordinate>); 4] = [
        (
            "eastbound",
            vec![
                coordinate_offset_from_sacramento(-0.004, 0.0),
                coordinate_offset_from_sacramento(0.004, 0.0),
            ],
        ),
        (
            "westbound",
            vec![
                coordinate_offset_from_sacramento(0.004, 0.0),
                coordinate_offset_from_sacramento(-0.004, 0.0),
            ],
        ),
        (
            "northbound",
            vec![
                coordinate_offset_from_sacramento(0.0, -0.004),
                coordinate_offset_from_sacramento(0.0, 0.004),
            ],
        ),
        (
            "southbound",
            vec![
                coordinate_offset_from_sacramento(0.0, 0.004),
                coordinate_offset_from_sacramento(0.0, -0.004),
            ],
        ),
    ];
    for (direction, line_coordinates) in segments {
        let layer = LabelLayer::place(
            &sacramento_camera(),
            &viewport,
            crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
            &[line_shield_candidate("I 80", line_coordinates)],
            &font,
        );
        assert!(
            !layer.labels().is_empty(),
            "{direction}: the line badge places"
        );
        for placed in layer.labels() {
            assert!(
                placed.shield.is_some(),
                "{direction}: every placed line label carries its badge"
            );
            assert!(
                (-LINE_LABEL_UPRIGHT_ROTATION_LIMIT_RADIANS
                    ..=LINE_LABEL_UPRIGHT_ROTATION_LIMIT_RADIANS)
                    .contains(&placed.rotation_radians),
                "{direction}: the rotation {} stays upright",
                placed.rotation_radians
            );
            // The outline's point at local [0, 1] is the bottom of the
            // badge in its own frame; after rotating by an upright angle
            // its vertical offset is cos(rotation) * 1, never negative.
            let mapped_y = SHIELD_OUTLINE[8][0] * placed.rotation_radians.sin()
                + SHIELD_OUTLINE[8][1] * placed.rotation_radians.cos();
            assert!(
                mapped_y >= -1e-12,
                "{direction}: the badge point never flips above the anchor (offset {mapped_y})"
            );
        }
    }
}

#[test]
fn two_candidates_carrying_the_same_reference_place_one_badge() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let near = shield_candidate("I 80", ShieldShape::RoundedRectangle, 0.0);
    // The second candidate belongs to a distinct feature far enough away
    // that both badges would fit independently; the frame still places
    // one label per distinct reference text.
    let mut far = near.clone();
    far.anchor = coordinate_offset_from_sacramento(0.008, 0.008);
    let layer = LabelLayer::place(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &[near, far],
        &font,
    );
    let placed_badge_texts: Vec<&str> = layer
        .labels()
        .iter()
        .filter(|label| label.shield.is_some())
        .map(|label| label.text.as_str())
        .collect();
    assert_eq!(
        placed_badge_texts.len(),
        1,
        "one frame places one badge per distinct reference text"
    );
    assert_eq!(placed_badge_texts[0], "I 80");
}

/// A shield whose minimum size dominates a short reference, so a badge
/// test can place two boxes whose glyph runs stay apart.
fn large_rounded_rectangle_shield() -> LabelShield {
    LabelShield {
        shape: ShieldShape::RoundedRectangle,
        fill_rgba: [1.0, 1.0, 1.0, 1.0],
        border_rgba: Some([0.0, 0.0, 0.0, 1.0]),
        border_width_pixels: 1.0,
        padding_pixels: 3.0,
        minimum_size_pixels: 40.0,
    }
}

/// A collidable point candidate: same geometry as the fixture anchor, with
/// the caller's text offset, priority and optional badge.
fn collidable_candidate(
    text: &str,
    priority: u32,
    offset_pixels: [f64; 2],
    shield: Option<LabelShield>,
) -> MapLabelCandidate {
    let mut candidate = shield_candidate(text, ShieldShape::RoundedRectangle, 0.0);
    candidate.priority = priority;
    candidate.allow_overlap = false;
    candidate.ignore_placement = false;
    candidate.text_offset_pixels = offset_pixels;
    candidate.shield = shield;
    candidate
}

#[test]
fn a_badge_box_collides_even_when_the_glyph_boxes_do_not() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let shield = large_rounded_rectangle_shield();
    let first = collidable_candidate("5", 0, [0.0, 0.0], Some(shield));
    // 19 px apart: the short glyph runs do not touch (each half-extent is
    // about 5 px with the collision padding), but the 40 px badges do.
    let second = collidable_candidate("6", 1, [19.0, 0.0], Some(shield));
    let layer = LabelLayer::place(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &[first, second],
        &font,
    );
    assert_eq!(
        layer.labels().len(),
        1,
        "the lower-priority badge overlaps an occupied badge box"
    );
    assert_eq!(layer.labels()[0].text, "5");
}

#[test]
fn a_place_name_is_rejected_by_an_adjacent_badge_box() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let badge_offset_pixels = 18.0;
    let reference_half_width_pixels =
        font.measure_text("5", 12.0).unwrap().width_pixels / 2.0 + LABEL_COLLISION_PADDING_PIXELS;
    let name_half_width_pixels =
        font.measure_text("Rio", 12.0).unwrap().width_pixels / 2.0 + LABEL_COLLISION_PADDING_PIXELS;
    assert!(
        badge_offset_pixels - reference_half_width_pixels > name_half_width_pixels,
        "the name's glyph box must clear the reference's glyph box"
    );
    assert!(
        name_half_width_pixels > badge_offset_pixels - 20.0,
        "the name must land inside the 40 px badge's half-extent"
    );
    let badge = collidable_candidate("5", 0, [0.0, 0.0], Some(large_rounded_rectangle_shield()));
    // 18 px to the right: the short name clears the reference's own glyph
    // box but lands inside the badge's padded box.
    let name = collidable_candidate("Rio", 1, [badge_offset_pixels, 0.0], None);
    let layer = LabelLayer::place(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &[badge, name],
        &font,
    );
    assert_eq!(
        layer.labels().len(),
        1,
        "the place name collides with the badge box beside its glyphs"
    );
    assert_eq!(layer.labels()[0].text, "5");
}

fn fixture_icon_sprite_atlas() -> SpriteAtlas {
    let index = crate::sprite::SpriteIndex::parse_json(
        r#"{ "railway": { "x": 0, "y": 0, "width": 16, "height": 16, "pixelRatio": 1 } }"#,
    )
    .expect("the fixture sprite index parses");
    SpriteAtlas::from_index_and_rgba(index, 16, 16, vec![255; 16 * 16 * 4])
        .expect("the fixture sheet is 16 by 16 RGBA")
}

fn fixture_icon_candidate() -> MapIconCandidate {
    MapIconCandidate {
        image_name: "railway".to_string(),
        size: 1.5,
        anchor: TextAnchor::Bottom,
        offset_pixels: [0.0, -ICON_OFFSET_EM_PIXELS / 2.0],
        rotation_degrees: 0.0,
        rotation_alignment: TextRotationAlignment::Auto,
        padding_pixels: 2.0,
        text_fit: IconTextFit::None,
        text_fit_padding_pixels: [0.0; 4],
        opacity: 1.0,
        allow_overlap: false,
        ignore_placement: false,
    }
}

fn candidate_with_icon(
    text: &str,
    anchor: GeographicCoordinate,
    icon: Option<MapIconCandidate>,
) -> MapLabelCandidate {
    MapLabelCandidate {
        text: text.to_string(),
        anchor,
        line_coordinates: Vec::new(),
        text_rgba: [0.0, 0.0, 0.0, 1.0],
        text_size_pixels: 11.0,
        halo_rgba: [1.0, 1.0, 1.0, 1.0],
        halo_width_pixels: 1.5,
        priority: 1,
        rank: 1,
        allow_overlap: false,
        ignore_placement: false,
        avoid_edges: false,
        text_offset_pixels: [0.0, 0.0],
        text_padding_pixels: LABEL_COLLISION_PADDING_PIXELS,
        text_pitch_alignment: TextPitchAlignment::Viewport,
        repeat_spacing_pixels: LINE_LABEL_REPEAT_SPACING_PIXELS,
        line_center_only: false,
        text_max_width_ems: 10.0,
        text_line_height_ems: 1.2,
        text_letter_spacing_ems: 0.0,
        text_justify: TextJustify::Auto,
        text_anchor: TextAnchor::Top,
        text_rotation_alignment: TextRotationAlignment::Auto,
        text_keep_upright: true,
        text_variable_anchors: Vec::new(),
        text_radial_offset_ems: 0.0,
        text_max_angle_radians: LINE_LABEL_STRAIGHT_RUN_ANGLE_TOLERANCE_RADIANS,
        style_order: 0,
        icon,
        shield: None,
    }
}

#[test]
fn an_icon_places_with_its_text_and_both_boxes_collide() {
    let font = fixture_font();
    let atlas = fixture_icon_sprite_atlas();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let anchor = GeographicCoordinate::from_degrees(
        SACRAMENTO_LATITUDE_DEGREES,
        SACRAMENTO_LONGITUDE_DEGREES,
    )
    .unwrap();
    let candidate = candidate_with_icon(
        "Central Pacific Freight Depot",
        anchor,
        Some(fixture_icon_candidate()),
    );
    let layer = LabelLayer::place_with_history_and_sprite(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &[candidate],
        &font,
        1.0,
        &[],
        Some(&std::sync::Arc::new(atlas)),
    );
    assert_eq!(layer.labels().len(), 1, "the candidate places");
    let label = &layer.labels()[0];
    assert_eq!(label.text, "Central Pacific Freight Depot");
    let placed_icon = label.icon.as_ref().expect("the icon places with its text");
    assert_eq!(placed_icon.image_name, "railway");
    assert_eq!(placed_icon.width_pixels, 24.0);
    assert_eq!(placed_icon.height_pixels, 24.0);
    assert_eq!(placed_icon.rotation_radians, 0.0);
    let quads = layer.icon_quad_vertices(&viewport);
    assert_eq!(quads.quads.len(), 1, "one placed icon draws one quad");
    let (vertex_bytes, index_bytes) = quads.encoded_buffers();
    assert_eq!(
        vertex_bytes.len(),
        ICON_CORNERS_PER_QUAD * std::mem::size_of::<ClipSpaceIconVertex>()
    );
    assert_eq!(
        index_bytes.len(),
        ICON_INDICES_PER_QUAD * std::mem::size_of::<u16>()
    );
    for vertex in &quads.quads[0] {
        assert!(
            vertex
                .clip_position
                .iter()
                .all(|component| component.is_finite())
        );
    }
}

#[test]
fn a_missing_sprite_or_icon_degrades_to_text_without_drawing() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let anchor = GeographicCoordinate::from_degrees(
        SACRAMENTO_LATITUDE_DEGREES,
        SACRAMENTO_LONGITUDE_DEGREES,
    )
    .unwrap();
    let mut missing_icon = fixture_icon_candidate();
    missing_icon.image_name = "not-in-the-sheet".to_string();
    let candidate = candidate_with_icon("Sacramento", anchor, Some(missing_icon));
    let without_sprite = LabelLayer::place(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        std::slice::from_ref(&candidate),
        &font,
    );
    assert_eq!(without_sprite.labels().len(), 1, "the text still places");
    assert!(without_sprite.labels()[0].icon.is_none());
    assert!(without_sprite.icon_quad_vertices(&viewport).is_empty());

    let atlas = fixture_icon_sprite_atlas();
    let with_sprite = LabelLayer::place_with_history_and_sprite(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &[candidate],
        &font,
        1.0,
        &[],
        Some(&std::sync::Arc::new(atlas)),
    );
    assert_eq!(with_sprite.labels().len(), 1, "the text still places");
    assert!(
        with_sprite.labels()[0].icon.is_none(),
        "an icon the sheet does not serve draws nothing"
    );

    let icon_only = candidate_with_icon("", anchor, Some(fixture_icon_candidate()));
    let no_icon_sheet = LabelLayer::place(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &[icon_only],
        &font,
    );
    assert!(
        no_icon_sheet.labels().is_empty(),
        "an icon-only candidate with no atlas contributes nothing"
    );
}

#[test]
fn an_icon_only_candidate_places_and_reserves_its_box() {
    let font = fixture_font();
    let atlas = std::sync::Arc::new(fixture_icon_sprite_atlas());
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let anchor = GeographicCoordinate::from_degrees(
        SACRAMENTO_LATITUDE_DEGREES,
        SACRAMENTO_LONGITUDE_DEGREES,
    )
    .unwrap();
    let first = candidate_with_icon("", anchor, Some(fixture_icon_candidate()));
    let mut second = candidate_with_icon("", anchor, Some(fixture_icon_candidate()));
    second.style_order = 1;
    let layer = LabelLayer::place_with_history_and_sprite(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &[first, second],
        &font,
        1.0,
        &[],
        Some(&atlas),
    );
    assert_eq!(
        layer.labels().len(),
        1,
        "two icons on one anchor must not overlap"
    );
    assert_eq!(layer.labels()[0].text, "");
    assert!(layer.labels()[0].icon.is_some());
}

#[test]
fn icon_geometry_honours_anchor_offset_and_rotation() {
    let atlas = fixture_icon_sprite_atlas();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let anchor = GeographicCoordinate::from_degrees(
        SACRAMENTO_LATITUDE_DEGREES,
        SACRAMENTO_LONGITUDE_DEGREES,
    )
    .unwrap();
    let camera = sacramento_camera();
    let field_of_view_degrees = crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES;
    let projected = camera
        .project_to_screen(&anchor, &viewport, field_of_view_degrees)
        .expect("the frame center projects");
    let candidate = fixture_icon_candidate();
    let placement = icon_placement_at(
        &camera,
        &viewport,
        field_of_view_degrees,
        &anchor,
        &candidate,
        None,
        Some(&atlas),
        1.0,
    )
    .expect("the fixture icon places");
    // A bottom anchor puts the icon's bottom edge on the symbol anchor, so
    // its center sits half its height above; the -0.5 em offset moves it
    // one further half of an em up.
    let expected_y = projected[1] - placement.height_pixels / 2.0 - ICON_OFFSET_EM_PIXELS / 2.0;
    assert!((placement.anchor_screen_pixels[1] - expected_y).abs() < 1e-9);
    assert!((placement.anchor_screen_pixels[0] - projected[0]).abs() < 1e-9);

    let mut rotated = candidate.clone();
    rotated.anchor = TextAnchor::Center;
    rotated.offset_pixels = [0.0, 0.0];
    rotated.rotation_degrees = 90.0;
    let rotated_placement = icon_placement_at(
        &camera,
        &viewport,
        field_of_view_degrees,
        &anchor,
        &rotated,
        None,
        Some(&atlas),
        1.0,
    )
    .expect("the rotated icon places");
    assert!(
        (rotated_placement.rotation_radians - std::f64::consts::FRAC_PI_2).abs() < 1e-9,
        "icon-rotate reaches the drawn quad"
    );
    let rotated_box = LabelScreenBox::around_with_padding(
        rotated_placement.anchor_screen_pixels,
        rotated_placement.width_pixels,
        rotated_placement.height_pixels,
        rotated_placement.rotation_radians,
        candidate.padding_pixels,
    );
    let rotated_width = 2.0 * rotated_box.axis_aligned_half_extents()[0];
    let expected_width = rotated_placement.height_pixels + 2.0 * candidate.padding_pixels;
    assert!(
        (rotated_width - expected_width).abs() < 1e-9,
        "a quarter turn swaps the collision box's extents"
    );
}

#[test]
fn colliding_labels_are_rejected_unless_overlap_is_allowed() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let anchor = GeographicCoordinate::from_degrees(
        SACRAMENTO_LATITUDE_DEGREES,
        SACRAMENTO_LONGITUDE_DEGREES,
    )
    .unwrap();
    let candidate = |text: &str, allow_overlap: bool| MapLabelCandidate {
        text: text.to_string(),
        anchor,
        line_coordinates: Vec::new(),
        text_rgba: [0.0, 0.0, 0.0, 1.0],
        text_size_pixels: 13.0,
        halo_rgba: [1.0, 1.0, 1.0, 1.0],
        halo_width_pixels: 1.5,
        priority: 1,
        rank: 1,
        allow_overlap,
        ignore_placement: false,
        avoid_edges: false,
        text_offset_pixels: [0.0, 0.0],
        text_padding_pixels: LABEL_COLLISION_PADDING_PIXELS,
        text_pitch_alignment: TextPitchAlignment::Viewport,
        repeat_spacing_pixels: LINE_LABEL_REPEAT_SPACING_PIXELS,
        line_center_only: false,
        text_max_width_ems: 10.0,
        text_line_height_ems: 1.2,
        text_letter_spacing_ems: 0.0,
        text_justify: rustic_maps_style::TextJustify::Auto,
        text_anchor: rustic_maps_style::TextAnchor::Center,
        text_rotation_alignment: rustic_maps_style::TextRotationAlignment::Auto,
        text_keep_upright: true,
        text_variable_anchors: Vec::new(),
        text_radial_offset_ems: 0.0,
        text_max_angle_radians: LINE_LABEL_STRAIGHT_RUN_ANGLE_TOLERANCE_RADIANS,
        style_order: 0,
        icon: None,
        shield: None,
    };
    let rejected = LabelLayer::place(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &[
            candidate("Sacramento", false),
            candidate("West Sacramento", false),
        ],
        &font,
    );
    assert_eq!(rejected.labels().len(), 1);

    let overlapping = LabelLayer::place(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &[
            candidate("Sacramento", false),
            candidate("West Sacramento", true),
        ],
        &font,
    );
    assert_eq!(overlapping.labels().len(), 2);
    assert_eq!(overlapping.labels()[0].text, "Sacramento");
}

fn remembered_candidate(text: &str, priority: u32) -> MapLabelCandidate {
    let anchor = GeographicCoordinate::from_degrees(
        SACRAMENTO_LATITUDE_DEGREES,
        SACRAMENTO_LONGITUDE_DEGREES,
    )
    .unwrap();
    MapLabelCandidate {
        text: text.to_string(),
        anchor,
        line_coordinates: Vec::new(),
        text_rgba: [0.0, 0.0, 0.0, 1.0],
        text_size_pixels: 13.0,
        halo_rgba: [1.0, 1.0, 1.0, 1.0],
        halo_width_pixels: 1.5,
        priority,
        rank: 1,
        allow_overlap: false,
        ignore_placement: false,
        avoid_edges: false,
        text_offset_pixels: [0.0, 0.0],
        text_padding_pixels: LABEL_COLLISION_PADDING_PIXELS,
        text_pitch_alignment: TextPitchAlignment::Viewport,
        repeat_spacing_pixels: LINE_LABEL_REPEAT_SPACING_PIXELS,
        line_center_only: false,
        text_max_width_ems: 10.0,
        text_line_height_ems: 1.2,
        text_letter_spacing_ems: 0.0,
        text_justify: rustic_maps_style::TextJustify::Auto,
        text_anchor: rustic_maps_style::TextAnchor::Center,
        text_rotation_alignment: rustic_maps_style::TextRotationAlignment::Auto,
        text_keep_upright: true,
        text_variable_anchors: Vec::new(),
        text_radial_offset_ems: 0.0,
        text_max_angle_radians: LINE_LABEL_STRAIGHT_RUN_ANGLE_TOLERANCE_RADIANS,
        style_order: 0,
        icon: None,
        shield: None,
    }
}

#[test]
fn a_remembered_label_keeps_its_spot_against_an_equal_priority_newcomer() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let first_frame = LabelLayer::place(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &[remembered_candidate("Sacramento", 1)],
        &font,
    );
    assert_eq!(first_frame.labels().len(), 1);
    assert_eq!(first_frame.labels()[0].text, "Sacramento");

    // Both names anchor at the frame center, so only one fits. They tie on
    // the whole priority key, so without a memory the caller's order
    // decides and the newcomer listed first takes the spot.
    let fresh = LabelLayer::place(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &[
            remembered_candidate("West Sacramento", 1),
            remembered_candidate("Sacramento", 1),
        ],
        &font,
    );
    assert_eq!(fresh.labels().len(), 1);
    assert_eq!(fresh.labels()[0].text, "West Sacramento");

    // The memory is the tie-break among equal-priority names: the name the
    // previous frame placed is tried first and keeps the spot.
    let continued = LabelLayer::place_with_history(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &[
            remembered_candidate("West Sacramento", 1),
            remembered_candidate("Sacramento", 1),
        ],
        &font,
        1.0,
        first_frame.labels(),
    );
    assert_eq!(continued.labels().len(), 1);
    assert_eq!(continued.labels()[0].text, "Sacramento");
}

#[test]
fn a_remembered_lower_priority_label_cannot_displace_a_higher_priority_newcomer() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    // The previous frame placed the place name while it was alone among the
    // candidates. That history must not let it jump the better symbol
    // class when both names contest the same slot.
    let first_frame = LabelLayer::place(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &[remembered_candidate("Sacramento", 1)],
        &font,
    );
    let candidates = [
        remembered_candidate("Sacramento", 1),
        remembered_candidate("West Sacramento", 0),
    ];
    let contested = LabelLayer::place_with_history(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &candidates,
        &font,
        1.0,
        first_frame.labels(),
    );
    assert_eq!(contested.labels().len(), 1);
    assert_eq!(
        contested.labels()[0].text,
        "West Sacramento",
        "a remembered lower-priority name must not override the rank order"
    );

    // Feeding the priority winner back as history reproduces it exactly:
    // the contested slot stays with the more important name.
    let repeated = LabelLayer::place_with_history(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &candidates,
        &font,
        1.0,
        contested.labels(),
    );
    assert_eq!(
        repeated.labels(),
        contested.labels(),
        "an identical frame with the winner's history must place the same labels"
    );
}

#[test]
fn identical_frames_converge_to_one_priority_ordered_label_set() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    // The two label sets the regional frame oscillated between: the city
    // name the previous frame happened to hold in one phase, and the
    // higher-priority shield that shares its contested slot. Whatever the
    // previous frame placed, every identical frame must settle on the
    // shield.
    let candidates = [
        remembered_candidate("Sacramento", 1),
        remembered_candidate("I 5", 0),
    ];
    let city_only = LabelLayer::place(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &[remembered_candidate("Sacramento", 1)],
        &font,
    );
    let mut history = city_only.labels().to_vec();
    let mut previous = LabelLayer::place_with_history(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &candidates,
        &font,
        1.0,
        &history,
    );
    assert_eq!(
        previous.labels()[0].text,
        "I 5",
        "the priority order places the shield, not the remembered city name"
    );
    history = previous.labels().to_vec();
    for frame in 0..4 {
        let next = LabelLayer::place_with_history(
            &sacramento_camera(),
            &viewport,
            crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
            &candidates,
            &font,
            1.0,
            &history,
        );
        assert_eq!(
            next.labels(),
            previous.labels(),
            "identical frame {frame} must reproduce the same placements"
        );
        history = next.labels().to_vec();
        previous = next;
    }
}

#[test]
fn a_remembered_label_the_candidates_dropped_does_not_block_their_successor() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let first_frame = LabelLayer::place(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &[remembered_candidate("Sacramento", 1)],
        &font,
    );
    let continued = LabelLayer::place_with_history(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &[remembered_candidate("West Sacramento", 0)],
        &font,
        1.0,
        first_frame.labels(),
    );
    assert_eq!(continued.labels().len(), 1);
    assert_eq!(
        continued.labels()[0].text,
        "West Sacramento",
        "a remembered name that is no longer a candidate is forgotten"
    );
}

#[test]
fn a_lower_priority_label_wins_over_an_earlier_style_layer() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let anchor = GeographicCoordinate::from_degrees(
        SACRAMENTO_LATITUDE_DEGREES,
        SACRAMENTO_LONGITUDE_DEGREES,
    )
    .unwrap();
    let label = |text: &str, priority: u32, style_order: usize| MapLabelCandidate {
        text: text.to_string(),
        anchor,
        line_coordinates: Vec::new(),
        text_rgba: [0.0, 0.0, 0.0, 1.0],
        text_size_pixels: 13.0,
        halo_rgba: [1.0, 1.0, 1.0, 1.0],
        halo_width_pixels: 1.5,
        priority,
        rank: 1,
        allow_overlap: false,
        ignore_placement: false,
        avoid_edges: false,
        text_offset_pixels: [0.0, 0.0],
        text_padding_pixels: LABEL_COLLISION_PADDING_PIXELS,
        text_pitch_alignment: TextPitchAlignment::Viewport,
        repeat_spacing_pixels: LINE_LABEL_REPEAT_SPACING_PIXELS,
        line_center_only: false,
        text_max_width_ems: 10.0,
        text_line_height_ems: 1.2,
        text_letter_spacing_ems: 0.0,
        text_justify: rustic_maps_style::TextJustify::Auto,
        text_anchor: rustic_maps_style::TextAnchor::Center,
        text_rotation_alignment: rustic_maps_style::TextRotationAlignment::Auto,
        text_keep_upright: true,
        text_variable_anchors: Vec::new(),
        text_radial_offset_ems: 0.0,
        text_max_angle_radians: LINE_LABEL_STRAIGHT_RUN_ANGLE_TOLERANCE_RADIANS,
        style_order,
        icon: None,
        shield: None,
    };
    // The point-of-interest layer lists first in the style but ranks less
    // important, so the city name must survive the collision.
    let layer = LabelLayer::place(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &[label("Coffee Shop", 2, 0), label("Sacramento", 1, 1)],
        &font,
    );
    assert_eq!(layer.labels().len(), 1);
    assert_eq!(layer.labels()[0].text, "Sacramento");
}

/// A point candidate at the Sacramento camera centre whose only varied
/// fields are its text and its collision rank, used by the ordering and
/// thinning cases.
fn ranked_candidate(text: &str, rank: u32) -> MapLabelCandidate {
    let anchor = GeographicCoordinate::from_degrees(
        SACRAMENTO_LATITUDE_DEGREES,
        SACRAMENTO_LONGITUDE_DEGREES,
    )
    .unwrap();
    let mut candidate = point_candidate(text, anchor, [0.0, 0.0], false, false);
    candidate.rank = rank;
    candidate
}

#[test]
fn a_shield_class_is_placed_before_a_higher_ranked_place_name() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    // The style declares shields at priority 0 and place names at priority
    // 1; the shield is unranked and the place name has the best possible
    // rank, so only class-before-rank keeps the shield on the map.
    let mut shield = ranked_candidate("I 80", DEFAULT_LABEL_RANK);
    shield.priority = 0;
    let mut place = ranked_candidate("Sacramento", 0);
    place.priority = 1;
    let layer = LabelLayer::place(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &[place, shield],
        &font,
    );
    assert_eq!(layer.labels().len(), 1);
    assert_eq!(layer.labels()[0].text, "I 80");
}

#[test]
fn a_lower_rank_wins_over_an_earlier_style_layer_in_the_same_class() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    // Both are place names of the same class. The later style layer holds
    // the more important feature, and rank must decide before the layer's
    // own order does.
    let mut early_layer = ranked_candidate("Lower rank", 9);
    early_layer.priority = 1;
    early_layer.style_order = 0;
    let mut later_layer = ranked_candidate("Higher rank", 5);
    later_layer.priority = 1;
    later_layer.style_order = 10;
    let layer = LabelLayer::place(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &[early_layer, later_layer],
        &font,
    );
    assert_eq!(layer.labels().len(), 1);
    assert_eq!(layer.labels()[0].text, "Higher rank");
}

#[test]
fn regional_thinning_bands_keep_cities_at_wide_zooms() {
    assert_eq!(maximum_label_rank_at_zoom(6.0), 6);
    assert_eq!(maximum_label_rank_at_zoom(7.999), 6);
    assert_eq!(maximum_label_rank_at_zoom(8.0), 18);
    assert_eq!(maximum_label_rank_at_zoom(9.5), 18);
    assert_eq!(maximum_label_rank_at_zoom(10.0), 20);
    assert_eq!(maximum_label_rank_at_zoom(13.0), 22);
    assert_eq!(maximum_label_rank_at_zoom(14.0), DEFAULT_LABEL_RANK);
    assert_eq!(maximum_label_rank_at_zoom(18.0), DEFAULT_LABEL_RANK);
    assert!(
        label_passes_regional_thinning(DEFAULT_LABEL_RANK, 6.0),
        "an unranked road name or shield never thins out"
    );
    assert!(label_passes_regional_thinning(18, 8.0));
    assert!(!label_passes_regional_thinning(19, 8.0));
}

#[test]
fn a_wide_camera_drops_lower_ranked_labels_and_a_close_camera_keeps_them() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let anchor = GeographicCoordinate::from_degrees(
        SACRAMENTO_LATITUDE_DEGREES,
        SACRAMENTO_LONGITUDE_DEGREES,
    )
    .unwrap();
    // Every candidate overlaps the others; `allow_overlap` takes collision
    // out of the picture so the frame's label list shows thinning alone.
    let candidates: Vec<MapLabelCandidate> = [
        ("Sacramento", 4),
        ("West Sacramento", 12),
        ("Downtown", 17),
        ("Old Sacramento", 21),
        ("Swanston Palms", 24),
    ]
    .into_iter()
    .map(|(text, rank)| {
        let mut candidate = point_candidate(text, anchor, [0.0, 0.0], false, false);
        candidate.allow_overlap = true;
        candidate.rank = rank;
        candidate
    })
    .collect();
    let texts_at_zoom = |zoom_level: f64| {
        let camera = MapCamera::new(anchor, zoom_level, 0.0, 0.0);
        let layer = LabelLayer::place(
            &camera,
            &viewport,
            crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
            &candidates,
            &font,
        );
        layer
            .labels()
            .iter()
            .map(|label| label.text.clone())
            .collect::<Vec<String>>()
    };
    let regional_texts = texts_at_zoom(8.0);
    assert!(regional_texts.contains(&"Sacramento".to_string()));
    assert!(regional_texts.contains(&"West Sacramento".to_string()));
    assert!(regional_texts.contains(&"Downtown".to_string()));
    assert!(
        !regional_texts.contains(&"Old Sacramento".to_string()),
        "rank 21 sits above the zoom-8 band's ceiling"
    );
    assert!(
        !regional_texts.contains(&"Swanston Palms".to_string()),
        "rank 24 sits above the zoom-8 band's ceiling"
    );
    assert_eq!(
        texts_at_zoom(16.0).len(),
        candidates.len(),
        "a city zoom places every ranked label"
    );
}

#[test]
fn each_drawn_glyph_contributes_its_own_collision_box() {
    let font = fixture_font();
    let block = font
        .measure_wrapped_text("A B", 24.0, 10_000.0, 1.2, 0.0)
        .expect("the fixture font measures the block");
    let placement = LabelPlacement {
        anchor_screen_pixels: [100.0, 100.0],
        rotation_radians: 0.0,
    };
    let boxes = glyph_collision_boxes(
        &font,
        &block,
        24.0,
        0.0,
        rustic_maps_style::TextJustify::Auto,
        LABEL_COLLISION_PADDING_PIXELS,
        1.0,
        placement,
    );
    assert_eq!(
        boxes.len(),
        2,
        "the space has no bitmap, so the two letters contribute one box each"
    );
    // Each box covers one letter rather than the whole measured line; a
    // single block box would be as wide as the block.
    for screen_box in &boxes {
        assert!(
            2.0 * screen_box.half_width_pixels < block.width_pixels,
            "a glyph box must not span the whole text block"
        );
    }
    assert_ne!(
        boxes[0].center_screen_pixels, boxes[1].center_screen_pixels,
        "the two letters keep their own centers"
    );
}

#[test]
fn a_rotated_collision_box_uses_its_oriented_extent() {
    // A long, thin box turned a quarter turn stands vertically. A probe
    // beside its narrow waist sits inside the axis-aligned bound but
    // outside the oriented box; only the oriented test rejects the empty
    // corner the axis-aligned bound would have reserved.
    let vertical_box = LabelScreenBox::around_with_padding(
        [0.0, 0.0],
        100.0,
        10.0,
        std::f64::consts::FRAC_PI_2,
        0.0,
    );
    let horizontal_probe = LabelScreenBox::around_with_padding([40.0, 0.0], 1.0, 1.0, 0.0, 0.0);
    assert!(
        !vertical_box.intersects(&horizontal_probe),
        "the probe lies outside the rotated box's true extent"
    );
    let [half_width_pixels, _] = vertical_box.axis_aligned_half_extents();
    assert!(
        half_width_pixels < 6.0,
        "the quarter turn swaps the box's axis-aligned extents"
    );
    // A probe on the box's own vertical run does collide.
    let on_box_probe = LabelScreenBox::around_with_padding([0.0, 30.0], 1.0, 1.0, 0.0, 0.0);
    assert!(vertical_box.intersects(&on_box_probe));
}

#[test]
fn repeated_text_is_painted_once_across_buffered_tiles() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let mut candidates = bright_candidates();
    let duplicated = candidates.clone();
    candidates.extend(duplicated);
    let layer = LabelLayer::place(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &candidates,
        &font,
    );
    let sacramento_count = layer
        .labels()
        .iter()
        .filter(|label| label.text == "Sacramento")
        .count();
    assert_eq!(sacramento_count, 1);
}

#[test]
fn a_name_straddling_two_adjacent_tiles_is_painted_once() {
    // The same tile payload at two adjacent addresses stands for one place
    // named in both tiles' buffered overlap. The camera sits on the shared
    // edge so both copies project inside the frame; cross-tile suppression
    // must still paint the name once.
    let tile = decode_vector_tile(OPENFREEMAP_TILE_BYTES).expect("the fixture decodes");
    let style = vector_tile_style_from_id(VECTOR_STYLE_ID_BRIGHT_LIGHT)
        .expect("the committed Bright style loads");
    let east_tile_address = VectorTileAddress {
        tile_x: TILE_ADDRESS.tile_x + 1,
        ..TILE_ADDRESS
    };
    let west_candidates = label_candidates_for_vector_tile(&tile, TILE_ADDRESS, &style, 14.0);
    let east_candidates = label_candidates_for_vector_tile(&tile, east_tile_address, &style, 14.0);
    for candidates in [&west_candidates, &east_candidates] {
        assert!(
            candidate_texts(candidates).contains(&"Sacramento"),
            "each tile's buffered copy carries the straddling place name"
        );
    }
    let mut straddling_candidates = west_candidates;
    straddling_candidates.extend(east_candidates);

    let sacramento_anchors: Vec<&GeographicCoordinate> = straddling_candidates
        .iter()
        .filter(|candidate| candidate.text == "Sacramento")
        .map(|candidate| &candidate.anchor)
        .collect();
    assert_eq!(
        sacramento_anchors.len(),
        2,
        "the two tile copies carry the name at two shifted anchors"
    );
    let midpoint_latitude_degrees =
        (sacramento_anchors[0].latitude + sacramento_anchors[1].latitude) / 2.0;
    let midpoint_longitude_degrees =
        (sacramento_anchors[0].longitude + sacramento_anchors[1].longitude) / 2.0;
    let straddle_camera = MapCamera::new(
        GeographicCoordinate::from_degrees(midpoint_latitude_degrees, midpoint_longitude_degrees)
            .unwrap(),
        14.0,
        0.0,
        0.0,
    );
    let viewport = ViewportPixels::new(1024.0, 1024.0).unwrap();
    let layer = LabelLayer::place(
        &straddle_camera,
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &straddling_candidates,
        &fixture_font(),
    );
    assert_eq!(
        layer
            .labels()
            .iter()
            .filter(|label| label.text == "Sacramento")
            .count(),
        1,
        "a name repeated by two tiles over the same place is painted once"
    );
}

#[test]
fn the_demo_zoom_holds_shops_back_and_caps_the_place_rank() {
    let demo_zoom_candidates = bright_candidates_at_zoom(14.0);
    let demo_zoom_texts = candidate_texts(&demo_zoom_candidates);
    for shop_text in ["7-Eleven", "McDonald's", "Chevron"] {
        assert!(
            !demo_zoom_texts.contains(&shop_text),
            "a shop label must not compete at the demo zoom, found {shop_text:?}"
        );
    }
    for important_text in [
        "Sutter Health Park",
        "Westfield Village Elementary School",
        "Sacramento",
        "Downtown",
    ] {
        assert!(
            demo_zoom_texts.contains(&important_text),
            "an important place name must survive the demo zoom, missing {important_text:?}"
        );
    }

    let close_zoom_candidates = bright_candidates_at_zoom(18.0);
    let close_zoom_texts = candidate_texts(&close_zoom_candidates);
    for shop_text in ["7-Eleven", "McDonald's", "Chevron"] {
        assert!(
            close_zoom_texts.contains(&shop_text),
            "a shop label appears once the map is close enough, missing {shop_text:?}"
        );
    }
    let mid_zoom_candidates = bright_candidates_at_zoom(16.0);
    assert!(
        !candidate_texts(&mid_zoom_candidates).contains(&"Sacramento History Museum"),
        "rank 19 stays above the zoom-16 ceiling"
    );
    assert!(
        close_zoom_texts.contains(&"Sacramento History Museum"),
        "the zoom-18 frame drops the rank ceiling and reads the museum"
    );

    let wide_zoom_candidates = bright_candidates_at_zoom(8.0);
    let wide_zoom_texts = candidate_texts(&wide_zoom_candidates);
    assert!(
        wide_zoom_texts.contains(&"Sacramento"),
        "a city name keeps drawing at a wide zoom"
    );
    assert!(
        wide_zoom_texts.contains(&"Downtown"),
        "a neighbourhood name keeps drawing at a wide zoom"
    );
}

/// A point candidate at the Sacramento camera centre with the shared
/// paint, differing only in the fields a test exercises.
fn point_candidate(
    text: &str,
    anchor: GeographicCoordinate,
    text_offset_pixels: [f64; 2],
    ignore_placement: bool,
    avoid_edges: bool,
) -> MapLabelCandidate {
    MapLabelCandidate {
        text: text.to_string(),
        anchor,
        line_coordinates: Vec::new(),
        text_rgba: [0.0, 0.0, 0.0, 1.0],
        text_size_pixels: 13.0,
        halo_rgba: [1.0, 1.0, 1.0, 1.0],
        halo_width_pixels: 1.5,
        priority: 0,
        rank: 1,
        allow_overlap: false,
        ignore_placement,
        avoid_edges,
        text_offset_pixels,
        text_padding_pixels: LABEL_COLLISION_PADDING_PIXELS,
        text_pitch_alignment: TextPitchAlignment::Viewport,
        repeat_spacing_pixels: LINE_LABEL_REPEAT_SPACING_PIXELS,
        line_center_only: false,
        text_max_width_ems: 10.0,
        text_line_height_ems: 1.2,
        text_letter_spacing_ems: 0.0,
        text_justify: rustic_maps_style::TextJustify::Auto,
        text_anchor: rustic_maps_style::TextAnchor::Center,
        text_rotation_alignment: rustic_maps_style::TextRotationAlignment::Auto,
        text_keep_upright: true,
        text_variable_anchors: Vec::new(),
        text_radial_offset_ems: 0.0,
        text_max_angle_radians: LINE_LABEL_STRAIGHT_RUN_ANGLE_TOLERANCE_RADIANS,
        style_order: 0,
        icon: None,
        shield: None,
    }
}

#[test]
fn a_text_offset_shifts_a_point_labels_anchor() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let camera = sacramento_camera();
    let anchor = GeographicCoordinate::from_degrees(
        SACRAMENTO_LATITUDE_DEGREES,
        SACRAMENTO_LONGITUDE_DEGREES,
    )
    .unwrap();
    let projected = camera
        .project_to_screen(
            &anchor,
            &viewport,
            crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        )
        .expect("the camera projects its own centre");
    let layer = LabelLayer::place(
        &camera,
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &[point_candidate(
            "Sacramento",
            anchor,
            [10.0, -6.0],
            false,
            false,
        )],
        &font,
    );
    let placed = &layer.labels()[0];
    assert!((placed.anchor_screen_pixels[0] - projected[0] - 10.0).abs() < 1e-6);
    assert!((placed.anchor_screen_pixels[1] - projected[1] + 6.0).abs() < 1e-6);
}

#[test]
fn ignore_placement_keeps_overlapping_labels() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let camera = sacramento_camera();
    let anchor = GeographicCoordinate::from_degrees(
        SACRAMENTO_LATITUDE_DEGREES,
        SACRAMENTO_LONGITUDE_DEGREES,
    )
    .unwrap();
    let candidates = [
        point_candidate("Sacramento", anchor, [0.0, 0.0], true, false),
        point_candidate("West Sacramento", anchor, [0.0, 0.0], true, false),
    ];
    let layer = LabelLayer::place(
        &camera,
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &candidates,
        &font,
    );
    assert_eq!(layer.labels().len(), 2);
}

#[test]
fn avoid_edges_drops_a_label_that_crosses_the_viewport() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let camera = sacramento_camera();
    let anchor = GeographicCoordinate::from_degrees(
        SACRAMENTO_LATITUDE_DEGREES,
        SACRAMENTO_LONGITUDE_DEGREES,
    )
    .unwrap();
    // The offset pushes the text just past the left edge, where it still
    // intersects the viewport but no longer lies inside it.
    let metrics = font
        .measure_text("Sacramento", 13.0)
        .expect("the fixture font measures Latin text");
    let half_width_pixels = metrics.width_pixels / 2.0 + LABEL_COLLISION_PADDING_PIXELS;
    let offset_x_pixels =
        -(viewport.width_pixels / 2.0) + half_width_pixels - LABEL_EDGE_TOLERANCE_PIXELS;
    let place = |avoid_edges: bool| {
        LabelLayer::place(
            &camera,
            &viewport,
            crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
            &[point_candidate(
                "Sacramento",
                anchor,
                [offset_x_pixels, 0.0],
                false,
                avoid_edges,
            )],
            &font,
        )
    };
    assert_eq!(place(false).labels().len(), 1);
    assert_eq!(place(true).labels().len(), 0);
}

#[test]
fn a_line_center_placement_places_one_label_at_the_run_midpoint() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let camera = sacramento_camera();
    let metrics = font
        .measure_text("Sacramento", 13.0)
        .expect("the fixture font measures Latin text");
    let line = vec![
        coordinate_offset_from_sacramento(-0.02, 0.0),
        coordinate_offset_from_sacramento(0.02, 0.0),
    ];
    let repeated = label_placements_along_line(
        &camera,
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &line,
        metrics.width_pixels,
        LINE_LABEL_REPEAT_SPACING_PIXELS,
        false,
        LINE_LABEL_STRAIGHT_RUN_ANGLE_TOLERANCE_RADIANS,
    );
    let centered = label_placements_along_line(
        &camera,
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &line,
        metrics.width_pixels,
        LINE_LABEL_REPEAT_SPACING_PIXELS,
        true,
        LINE_LABEL_STRAIGHT_RUN_ANGLE_TOLERANCE_RADIANS,
    );
    assert!(repeated.len() > 1, "the long run repeats its name");
    assert_eq!(centered.len(), 1);
}

#[test]
fn a_long_label_wraps_at_the_declared_width() {
    let font = fixture_font();
    let block = font
        .measure_wrapped_text("West Sacramento City Hall", 13.0, 60.0, 1.2, 0.0)
        .expect("the fixture font measures the label");
    assert!(block.lines.len() >= 2, "lines {:?}", block.lines);
    for line in &block.lines {
        assert!(
            line.width_pixels <= block.width_pixels + 1e-9,
            "line {:?} wider than the block",
            line
        );
    }
    assert!((block.line_height_pixels - 13.0 * 1.2).abs() < 1e-9);
    assert!(
        (block.height_pixels - block.line_height_pixels * block.lines.len() as f64).abs() < 1e-9
    );
}

#[test]
fn letter_spacing_widens_a_measured_line() {
    let font = fixture_font();
    let plain = font
        .measure_text("Sacramento", 13.0)
        .expect("the fixture font measures the label");
    let spaced = font
        .measure_wrapped_text("Sacramento", 13.0, 10_000.0, 1.2, 2.0)
        .expect("the fixture font measures the label");
    let served_glyph_count = "Sacramento".chars().count() as f64;
    assert!(
        (spaced.width_pixels - (plain.width_pixels + 2.0 * (served_glyph_count - 1.0))).abs()
            < 1e-9
    );
}

#[test]
fn a_variable_anchor_takes_the_first_anchor_that_fits() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let camera = sacramento_camera();
    let anchor = GeographicCoordinate::from_degrees(
        SACRAMENTO_LATITUDE_DEGREES,
        SACRAMENTO_LONGITUDE_DEGREES,
    )
    .unwrap();
    // The first label blocks the space above the symbol, where the
    // candidate's `top` anchor would place it; the second, with
    // [top, bottom], must fall through to `bottom`.
    let mut occupying_candidate = point_candidate("Sacramento", anchor, [0.0, -30.0], false, false);
    occupying_candidate.text_anchor = TextAnchor::Center;
    let mut variable_candidate =
        point_candidate("West Sacramento", anchor, [0.0, 0.0], false, false);
    variable_candidate.text_variable_anchors = vec![TextAnchor::Top, TextAnchor::Bottom];
    let layer = LabelLayer::place(
        &camera,
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &[occupying_candidate, variable_candidate],
        &font,
    );
    let projected = camera
        .project_to_screen(
            &anchor,
            &viewport,
            crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        )
        .expect("the camera projects its own centre");
    let placed = layer
        .labels()
        .iter()
        .find(|label| label.text == "West Sacramento")
        .expect("the variable-anchor label is placed");
    assert!(
        placed.anchor_screen_pixels[1] > projected[1],
        "the bottom fallback sits below the symbol"
    );
}

#[test]
fn a_radial_offset_pushes_a_variable_anchor_away() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let camera = sacramento_camera();
    let anchor = GeographicCoordinate::from_degrees(
        SACRAMENTO_LATITUDE_DEGREES,
        SACRAMENTO_LONGITUDE_DEGREES,
    )
    .unwrap();
    let place_with_radial_offset = |radial_offset_ems| {
        let mut candidate = point_candidate("Sacramento", anchor, [0.0, 0.0], false, false);
        candidate.text_variable_anchors = vec![TextAnchor::Top];
        candidate.text_radial_offset_ems = radial_offset_ems;
        LabelLayer::place(
            &camera,
            &viewport,
            crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
            &[candidate],
            &font,
        )
    };
    let without_radial = place_with_radial_offset(0.0).labels()[0].anchor_screen_pixels;
    let with_radial = place_with_radial_offset(2.0).labels()[0].anchor_screen_pixels;
    assert!(
        (with_radial[1] - without_radial[1] + 2.0 * 13.0).abs() < 1e-9,
        "the radial offset lifts the block by two ems"
    );
}

#[test]
fn a_wider_max_angle_keeps_a_bent_line_in_one_run() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let camera = sacramento_camera();
    let metrics = font
        .measure_text("Sacramento", 13.0)
        .expect("the fixture font measures Latin text");
    let line = vec![
        coordinate_offset_from_sacramento(-0.02, 0.0),
        coordinate_offset_from_sacramento(0.0, 0.0),
        coordinate_offset_from_sacramento(0.02, 0.01),
    ];
    let placements_at = |tolerance| {
        label_placements_along_line(
            &camera,
            &viewport,
            crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
            &line,
            metrics.width_pixels,
            LINE_LABEL_REPEAT_SPACING_PIXELS,
            false,
            tolerance,
        )
        .len()
    };
    let narrow = placements_at(20.0_f64.to_radians());
    let wide = placements_at(60.0_f64.to_radians());
    assert!(wide >= narrow, "narrow {narrow} wide {wide}");
    assert!(wide > narrow, "the wider tolerance keeps the longer run");
}

#[test]
fn text_anchor_offsets_the_block_from_the_symbol_anchor() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let camera = sacramento_camera();
    let anchor = GeographicCoordinate::from_degrees(
        SACRAMENTO_LATITUDE_DEGREES,
        SACRAMENTO_LONGITUDE_DEGREES,
    )
    .unwrap();
    let place_with_anchor = |text_anchor| {
        let mut candidate = point_candidate("Sacramento", anchor, [0.0, 0.0], false, false);
        candidate.text_anchor = text_anchor;
        LabelLayer::place(
            &camera,
            &viewport,
            crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
            &[candidate],
            &font,
        )
    };
    let centered = place_with_anchor(TextAnchor::Center);
    let top_left = place_with_anchor(TextAnchor::TopLeft);
    let centered_anchor = centered.labels()[0].anchor_screen_pixels;
    let top_left_anchor = top_left.labels()[0].anchor_screen_pixels;
    let block = font
        .measure_wrapped_text("Sacramento", 13.0, 130.0, 1.2, 0.0)
        .expect("the fixture font measures the label");
    assert!((top_left_anchor[0] - centered_anchor[0] - block.width_pixels / 2.0).abs() < 1e-9);
    assert!((top_left_anchor[1] - centered_anchor[1] - block.height_pixels / 2.0).abs() < 1e-9);
}

#[test]
fn map_rotation_alignment_turns_a_point_label_with_the_bearing() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let camera = MapCamera::new(
        GeographicCoordinate::from_degrees(
            SACRAMENTO_LATITUDE_DEGREES,
            SACRAMENTO_LONGITUDE_DEGREES,
        )
        .unwrap(),
        14.0,
        30.0,
        0.0,
    );
    let anchor = GeographicCoordinate::from_degrees(
        SACRAMENTO_LATITUDE_DEGREES,
        SACRAMENTO_LONGITUDE_DEGREES,
    )
    .unwrap();
    let place_with_rotation = |alignment, keep_upright| {
        let mut candidate = point_candidate("Sacramento", anchor, [0.0, 0.0], false, false);
        candidate.text_rotation_alignment = alignment;
        candidate.text_keep_upright = keep_upright;
        LabelLayer::place(
            &camera,
            &viewport,
            crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
            &[candidate],
            &font,
        )
    };
    let map_aligned = place_with_rotation(TextRotationAlignment::Map, true);
    assert!((map_aligned.labels()[0].rotation_radians - 30.0_f64.to_radians()).abs() < 1e-9);
    let viewport_aligned = place_with_rotation(TextRotationAlignment::Viewport, true);
    assert_eq!(viewport_aligned.labels()[0].rotation_radians, 0.0);
}

#[test]
fn keep_upright_flips_a_label_past_a_quarter_turn() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let camera = MapCamera::new(
        GeographicCoordinate::from_degrees(
            SACRAMENTO_LATITUDE_DEGREES,
            SACRAMENTO_LONGITUDE_DEGREES,
        )
        .unwrap(),
        14.0,
        120.0,
        0.0,
    );
    let anchor = GeographicCoordinate::from_degrees(
        SACRAMENTO_LATITUDE_DEGREES,
        SACRAMENTO_LONGITUDE_DEGREES,
    )
    .unwrap();
    let mut candidate = point_candidate("Sacramento", anchor, [0.0, 0.0], false, false);
    candidate.text_rotation_alignment = TextRotationAlignment::Map;
    candidate.text_keep_upright = true;
    let layer = LabelLayer::place(
        &camera,
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &[candidate],
        &font,
    );
    let upright_rotation = layer.labels()[0].rotation_radians;
    assert!((upright_rotation - (-60.0_f64).to_radians()).abs() < 1e-9);
}

#[test]
fn justify_moves_a_short_line_within_the_block() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let camera = sacramento_camera();
    let anchor = GeographicCoordinate::from_degrees(
        SACRAMENTO_LATITUDE_DEGREES,
        SACRAMENTO_LONGITUDE_DEGREES,
    )
    .unwrap();
    let place_with_justify = |justify| {
        let mut candidate = point_candidate("West Sacramento", anchor, [0.0, 0.0], false, false);
        candidate.text_max_width_ems = 40.0 / 13.0;
        candidate.text_justify = justify;
        LabelLayer::place(
            &camera,
            &viewport,
            crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
            &[candidate],
            &font,
        )
    };
    let centered = place_with_justify(TextJustify::Center);
    let left = place_with_justify(TextJustify::Left);
    assert_eq!(centered.labels()[0].lines.len(), 2);
    let line_minimum_x = |layer: &LabelLayer, upper_half: bool| {
        let vertices = layer.quad_vertices(&viewport).text;
        let anchor_y = layer.labels()[0].anchor_screen_pixels[1];
        vertices
            .iter()
            .map(|vertex| {
                let clip = glam::DVec4::new(
                    f64::from(vertex.clip_position[0]),
                    f64::from(vertex.clip_position[1]),
                    f64::from(vertex.clip_position[2]),
                    f64::from(vertex.clip_position[3]),
                );
                let screen = crate::overlay::clip_position_to_screen_pixels(clip, &viewport);
                (screen[0], screen[1])
            })
            .filter(|(_, screen_y)| (*screen_y < anchor_y) == upper_half)
            .map(|(screen_x, _)| screen_x)
            .fold(f64::INFINITY, f64::min)
    };
    // The upper (shorter) line's start moves inward under centered
    // justification; a left-justified block starts both lines together.
    let centered_shift = line_minimum_x(&centered, true) - line_minimum_x(&centered, false);
    let left_shift = line_minimum_x(&left, true) - line_minimum_x(&left, false);
    assert!(
        centered_shift > 4.0,
        "a centered short line starts inside the block (shift {centered_shift})"
    );
    // The per-line minimum is a glyph bitmap edge, so its left bearing
    // differs by a pixel; what matters is that left justification does
    // not apply the centered inset.
    assert!(
        left_shift.abs() < 2.0,
        "a left-justified block starts both lines together (shift {left_shift})"
    );
}

#[test]
fn a_declared_symbol_spacing_changes_the_repeat_count() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let camera = sacramento_camera();
    let metrics = font
        .measure_text("Sacramento", 13.0)
        .expect("the fixture font measures Latin text");
    let line = vec![
        coordinate_offset_from_sacramento(-0.02, 0.0),
        coordinate_offset_from_sacramento(0.02, 0.0),
    ];
    let placements_at = |repeat_spacing_pixels: f64| {
        label_placements_along_line(
            &camera,
            &viewport,
            crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
            &line,
            metrics.width_pixels,
            repeat_spacing_pixels,
            false,
            LINE_LABEL_STRAIGHT_RUN_ANGLE_TOLERANCE_RADIANS,
        )
        .len()
    };
    assert!(placements_at(60.0) > placements_at(260.0));
}

#[test]
fn a_frame_places_at_most_its_label_budget_after_the_priority_sort() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(2048.0, 2048.0).unwrap();
    let candidate_count = MAXIMUM_LABELS_PER_FRAME * 2;
    let candidates: Vec<MapLabelCandidate> = (0..candidate_count)
        .map(|index| {
            let column = index % 8;
            let row = index / 8;
            MapLabelCandidate {
                text: format!("A{index}"),
                anchor: coordinate_offset_from_sacramento(
                    -0.028 + column as f64 * 0.008,
                    0.028 - row as f64 * 0.008,
                ),
                line_coordinates: Vec::new(),
                text_rgba: [0.0, 0.0, 0.0, 1.0],
                text_size_pixels: 12.0,
                halo_rgba: [1.0, 1.0, 1.0, 1.0],
                halo_width_pixels: 1.5,
                priority: 1,
                rank: index as u32,
                allow_overlap: false,
                ignore_placement: false,
                avoid_edges: false,
                text_offset_pixels: [0.0, 0.0],
                text_padding_pixels: LABEL_COLLISION_PADDING_PIXELS,
                text_pitch_alignment: TextPitchAlignment::Viewport,
                repeat_spacing_pixels: LINE_LABEL_REPEAT_SPACING_PIXELS,
                line_center_only: false,
                text_max_width_ems: 10.0,
                text_line_height_ems: 1.2,
                text_letter_spacing_ems: 0.0,
                text_justify: rustic_maps_style::TextJustify::Auto,
                text_anchor: rustic_maps_style::TextAnchor::Center,
                text_rotation_alignment: rustic_maps_style::TextRotationAlignment::Auto,
                text_keep_upright: true,
                text_variable_anchors: Vec::new(),
                text_radial_offset_ems: 0.0,
                text_max_angle_radians: LINE_LABEL_STRAIGHT_RUN_ANGLE_TOLERANCE_RADIANS,
                style_order: 0,
                icon: None,
                shield: None,
            }
        })
        .collect();
    let layer = LabelLayer::place(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &candidates,
        &font,
    );
    assert_eq!(
        layer.labels().len(),
        MAXIMUM_LABELS_PER_FRAME,
        "the frame stops at its label budget even though every candidate fits"
    );
    let last_label = layer
        .labels()
        .last()
        .expect("a budgeted frame places its last label");
    assert_eq!(
        last_label.text,
        format!("A{}", MAXIMUM_LABELS_PER_FRAME - 1),
        "the budget is spent after the priority sort, so the ranked names win"
    );
}

/// A point-placed candidate for the frame-budget tests. Overlap is allowed
/// and every candidate shares an anchor, so placement succeeds purely on
/// the budget and the test never depends on collision geometry.
fn budget_test_point_candidate(text: String, rank: u32) -> MapLabelCandidate {
    MapLabelCandidate {
        text,
        anchor: coordinate_offset_from_sacramento(0.0, 0.0),
        line_coordinates: Vec::new(),
        text_rgba: [0.0, 0.0, 0.0, 1.0],
        text_size_pixels: 12.0,
        halo_rgba: [1.0, 1.0, 1.0, 1.0],
        halo_width_pixels: 1.5,
        priority: 1,
        rank,
        allow_overlap: true,
        ignore_placement: false,
        avoid_edges: false,
        text_offset_pixels: [0.0, 0.0],
        text_padding_pixels: LABEL_COLLISION_PADDING_PIXELS,
        text_pitch_alignment: TextPitchAlignment::Viewport,
        repeat_spacing_pixels: LINE_LABEL_REPEAT_SPACING_PIXELS,
        line_center_only: false,
        text_max_width_ems: 10.0,
        text_line_height_ems: 1.2,
        text_letter_spacing_ems: 0.0,
        text_justify: rustic_maps_style::TextJustify::Auto,
        text_anchor: rustic_maps_style::TextAnchor::Center,
        text_rotation_alignment: rustic_maps_style::TextRotationAlignment::Auto,
        text_keep_upright: true,
        text_variable_anchors: Vec::new(),
        text_radial_offset_ems: 0.0,
        text_max_angle_radians: LINE_LABEL_STRAIGHT_RUN_ANGLE_TOLERANCE_RADIANS,
        style_order: 0,
        icon: None,
        shield: None,
    }
}

/// A line-placed (road-name) candidate for the frame-budget tests, ranked
/// like an unranked street name and allowed to overlap.
fn budget_test_line_candidate(text: String, latitude_offset: f64) -> MapLabelCandidate {
    let mut candidate = line_label_candidate(
        &text,
        vec![
            coordinate_offset_from_sacramento(-0.04, latitude_offset),
            coordinate_offset_from_sacramento(0.04, latitude_offset),
        ],
        12.0,
    );
    candidate.rank = DEFAULT_LABEL_RANK;
    candidate.allow_overlap = true;
    candidate
}

#[test]
fn reserved_line_slots_place_road_names_even_when_places_spend_the_budget() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(2048.0, 2048.0).unwrap();
    let place_count = MAXIMUM_LABELS_PER_FRAME + 8;
    let mut candidates: Vec<MapLabelCandidate> = (0..place_count)
        .map(|index| budget_test_point_candidate(format!("P{index}"), index as u32))
        .collect();
    let line_count = RESERVED_LINE_LABEL_SLOTS + 4;
    for index in 0..line_count {
        candidates.push(budget_test_line_candidate(
            format!("R{index}"),
            0.028 - index as f64 * 0.003,
        ));
    }
    let layer = LabelLayer::place(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &candidates,
        &font,
    );
    let placed_roads = layer
        .labels()
        .iter()
        .filter(|label| label.text.starts_with('R'))
        .count();
    let placed_places = layer.labels().len() - placed_roads;
    assert_eq!(
        layer.labels().len(),
        MAXIMUM_LABELS_PER_FRAME,
        "the frame still spends its whole budget"
    );
    assert_eq!(
        placed_roads, RESERVED_LINE_LABEL_SLOTS,
        "the reserved slots carry road names past the ranked places"
    );
    assert_eq!(
        placed_places,
        MAXIMUM_LABELS_PER_FRAME - RESERVED_LINE_LABEL_SLOTS,
        "places fill the rest of the budget"
    );
}

#[test]
fn unused_line_slots_return_to_places() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(2048.0, 2048.0).unwrap();
    let place_count = MAXIMUM_LABELS_PER_FRAME + 8;
    let mut candidates: Vec<MapLabelCandidate> = (0..place_count)
        .map(|index| budget_test_point_candidate(format!("P{index}"), index as u32))
        .collect();
    let line_count = 3;
    for index in 0..line_count {
        candidates.push(budget_test_line_candidate(
            format!("R{index}"),
            0.028 - index as f64 * 0.003,
        ));
    }
    let layer = LabelLayer::place(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &candidates,
        &font,
    );
    let placed_roads = layer
        .labels()
        .iter()
        .filter(|label| label.text.starts_with('R'))
        .count();
    let placed_places = layer.labels().len() - placed_roads;
    assert_eq!(placed_roads, line_count);
    assert_eq!(
        placed_places,
        MAXIMUM_LABELS_PER_FRAME - line_count,
        "only the slots the line names actually use are withheld from places"
    );
}

#[test]
fn a_font_without_the_glyphs_places_no_labels() {
    let empty_font_stack = GlyphFontStack::default();
    let font = LabelFont::from_glyph_font_stack(&empty_font_stack)
        .expect("an empty range packs an empty atlas");
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let layer = LabelLayer::place(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &bright_candidates(),
        &font,
    );
    assert!(layer.is_empty());
    assert!(layer.quad_vertices(&viewport).is_empty());
}

#[test]
fn a_style_without_symbol_layers_reads_no_candidates() {
    let tile = decode_vector_tile(OPENFREEMAP_TILE_BYTES).unwrap();
    let style = compiled_style(
        r#"{
            "version": 8,
            "sources": { "base": { "type": "vector" } },
            "layers": [
                {
                    "id": "water",
                    "type": "fill",
                    "source": "base",
                    "source-layer": "water",
                    "paint": { "fill-color": [51, 127.5, 229.5, 0.4] }
                }
            ]
        }"#,
    );
    assert!(label_candidates_for_vector_tile(&tile, TILE_ADDRESS, &style, 14.0).is_empty());
}

#[test]
fn the_halo_pass_precedes_the_text_pass() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let anchor = GeographicCoordinate::from_degrees(
        SACRAMENTO_LATITUDE_DEGREES,
        SACRAMENTO_LONGITUDE_DEGREES,
    )
    .unwrap();
    let layer = LabelLayer::place(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &[MapLabelCandidate {
            text: "Sacramento".to_string(),
            anchor,
            line_coordinates: Vec::new(),
            text_rgba: [0.0, 0.0, 0.0, 1.0],
            text_size_pixels: 13.0,
            halo_rgba: [1.0, 1.0, 1.0, 1.0],
            halo_width_pixels: 1.5,
            priority: 1,
            rank: 1,
            allow_overlap: false,
            ignore_placement: false,
            avoid_edges: false,
            text_offset_pixels: [0.0, 0.0],
            text_padding_pixels: LABEL_COLLISION_PADDING_PIXELS,
            text_pitch_alignment: TextPitchAlignment::Viewport,
            repeat_spacing_pixels: LINE_LABEL_REPEAT_SPACING_PIXELS,
            line_center_only: false,
            text_max_width_ems: 10.0,
            text_line_height_ems: 1.2,
            text_letter_spacing_ems: 0.0,
            text_justify: rustic_maps_style::TextJustify::Auto,
            text_anchor: rustic_maps_style::TextAnchor::Center,
            text_rotation_alignment: rustic_maps_style::TextRotationAlignment::Auto,
            text_keep_upright: true,
            text_variable_anchors: Vec::new(),
            text_radial_offset_ems: 0.0,
            text_max_angle_radians: LINE_LABEL_STRAIGHT_RUN_ANGLE_TOLERANCE_RADIANS,
            style_order: 0,
            icon: None,
            shield: None,
        }],
        &font,
    );
    let vertices = layer.quad_vertices(&viewport);
    assert_eq!(vertices.halo.len(), vertices.text.len());
    let expected_halo_threshold = glyph_halo_signed_distance_threshold(13.0, 1.5);
    assert!(
        vertices
            .halo
            .iter()
            .all(|vertex| vertex.signed_distance_threshold == expected_halo_threshold)
    );
    // The halo keeps the text quad's geometry: the lower threshold, not
    // an inflated rectangle, is what spreads the outline.
    assert_eq!(vertices.halo.len(), vertices.text.len());
    assert_eq!(
        vertices.halo[0].texture_coordinates,
        vertices.text[0].texture_coordinates
    );
    // Text sits on the glyph protocol's own 0.75 contour, and every vertex
    // carries the size-derived gamma (0.84 device pixels at 1×).
    assert!(
        vertices
            .text
            .iter()
            .all(|vertex| vertex.signed_distance_threshold == GLYPH_TEXT_SIGNED_DISTANCE_THRESHOLD)
    );
    let expected_gamma = glyph_edge_gamma(13.0, 1.0);
    assert!(
        vertices
            .text
            .iter()
            .all(|vertex| vertex.signed_distance_gamma == expected_gamma)
    );
    // 0.105 / (13 / 24) ≈ 0.194 field units.
    assert!(
        (expected_gamma - 0.193_846_15).abs() < 1e-5,
        "{expected_gamma}"
    );
}

#[test]
fn a_two_x_surface_draws_the_same_point_size_with_the_same_edge_band() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(1024.0, 1024.0).unwrap();
    let anchor = GeographicCoordinate::from_degrees(
        SACRAMENTO_LATITUDE_DEGREES,
        SACRAMENTO_LONGITUDE_DEGREES,
    )
    .unwrap();
    let candidate = MapLabelCandidate {
        text: "Sacramento".to_string(),
        anchor,
        line_coordinates: Vec::new(),
        text_rgba: [0.0, 0.0, 0.0, 1.0],
        text_size_pixels: 13.0,
        halo_rgba: [1.0, 1.0, 1.0, 1.0],
        halo_width_pixels: 1.5,
        priority: 1,
        rank: 1,
        allow_overlap: false,
        ignore_placement: false,
        avoid_edges: false,
        text_offset_pixels: [0.0, 0.0],
        text_padding_pixels: LABEL_COLLISION_PADDING_PIXELS,
        text_pitch_alignment: TextPitchAlignment::Viewport,
        repeat_spacing_pixels: LINE_LABEL_REPEAT_SPACING_PIXELS,
        line_center_only: false,
        text_max_width_ems: 10.0,
        text_line_height_ems: 1.2,
        text_letter_spacing_ems: 0.0,
        text_justify: rustic_maps_style::TextJustify::Auto,
        text_anchor: rustic_maps_style::TextAnchor::Center,
        text_rotation_alignment: rustic_maps_style::TextRotationAlignment::Auto,
        text_keep_upright: true,
        text_variable_anchors: Vec::new(),
        text_radial_offset_ems: 0.0,
        text_max_angle_radians: LINE_LABEL_STRAIGHT_RUN_ANGLE_TOLERANCE_RADIANS,
        style_order: 0,
        icon: None,
        shield: None,
    };
    let single_pixel_layer = LabelLayer::place(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        std::slice::from_ref(&candidate),
        &font,
    );
    let two_x_layer = LabelLayer::place_with_device_pixel_ratio(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        std::slice::from_ref(&candidate),
        &font,
        2.0,
    );
    // The placed label is drawn at twice the device size...
    let single_pixel_label = &single_pixel_layer.labels()[0];
    let two_x_label = &two_x_layer.labels()[0];
    assert_eq!(
        two_x_label.text_size_pixels,
        single_pixel_label.text_size_pixels * 2.0
    );
    assert_eq!(
        two_x_label.halo_width_pixels,
        single_pixel_label.halo_width_pixels * 2.0
    );
    // ...while the fragment stage's half-band stays 0.84 device pixels,
    // so gamma halves in field units and the halo threshold (which is in
    // the style's point units) does not move.
    let single_pixel_gamma = glyph_edge_gamma(13.0, 1.0);
    let two_x_gamma = glyph_edge_gamma(13.0, 2.0);
    assert!((two_x_gamma - single_pixel_gamma / 2.0).abs() < 1e-6);
    let single_pixel_vertices = single_pixel_layer.quad_vertices(&viewport);
    let two_x_vertices = two_x_layer.quad_vertices(&viewport);
    assert_eq!(
        two_x_vertices.text[0].signed_distance_threshold,
        single_pixel_vertices.text[0].signed_distance_threshold
    );
    assert_eq!(
        two_x_vertices.halo[0].signed_distance_threshold,
        single_pixel_vertices.halo[0].signed_distance_threshold
    );
    assert_eq!(two_x_vertices.text[0].signed_distance_gamma, two_x_gamma);
}

#[test]
fn a_negative_zero_halo_draws_only_the_text() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let anchor = GeographicCoordinate::from_degrees(
        SACRAMENTO_LATITUDE_DEGREES,
        SACRAMENTO_LONGITUDE_DEGREES,
    )
    .unwrap();
    let layer = LabelLayer::place(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &[MapLabelCandidate {
            text: "A".to_string(),
            anchor,
            line_coordinates: Vec::new(),
            text_rgba: [0.0, 0.0, 0.0, 1.0],
            text_size_pixels: 13.0,
            halo_rgba: [1.0, 1.0, 1.0, 1.0],
            halo_width_pixels: 0.0,
            priority: 1,
            rank: 1,
            allow_overlap: false,
            ignore_placement: false,
            avoid_edges: false,
            text_offset_pixels: [0.0, 0.0],
            text_padding_pixels: LABEL_COLLISION_PADDING_PIXELS,
            text_pitch_alignment: TextPitchAlignment::Viewport,
            repeat_spacing_pixels: LINE_LABEL_REPEAT_SPACING_PIXELS,
            line_center_only: false,
            text_max_width_ems: 10.0,
            text_line_height_ems: 1.2,
            text_letter_spacing_ems: 0.0,
            text_justify: rustic_maps_style::TextJustify::Auto,
            text_anchor: rustic_maps_style::TextAnchor::Center,
            text_rotation_alignment: rustic_maps_style::TextRotationAlignment::Auto,
            text_keep_upright: true,
            text_variable_anchors: Vec::new(),
            text_radial_offset_ems: 0.0,
            text_max_angle_radians: LINE_LABEL_STRAIGHT_RUN_ANGLE_TOLERANCE_RADIANS,
            style_order: 0,
            icon: None,
            shield: None,
        }],
        &font,
    );
    let vertices = layer.quad_vertices(&viewport);
    assert!(vertices.halo.is_empty());
    assert!(!vertices.text.is_empty());
}

/// A coordinate offset from the Sacramento anchor in degrees; at zoom 14
/// one hundredth of a degree of longitude spans roughly 230 viewport
/// pixels, enough for a short street name.
fn coordinate_offset_from_sacramento(
    east_degrees: f64,
    north_degrees: f64,
) -> GeographicCoordinate {
    GeographicCoordinate::from_degrees(
        SACRAMENTO_LATITUDE_DEGREES + north_degrees,
        SACRAMENTO_LONGITUDE_DEGREES + east_degrees,
    )
    .unwrap()
}

fn line_label_candidate(
    text: &str,
    line_coordinates: Vec<GeographicCoordinate>,
    text_size_pixels: f64,
) -> MapLabelCandidate {
    MapLabelCandidate {
        text: text.to_string(),
        anchor: line_coordinates[0],
        line_coordinates,
        text_rgba: [0.0, 0.0, 0.0, 1.0],
        text_size_pixels,
        halo_rgba: [1.0, 1.0, 1.0, 1.0],
        halo_width_pixels: 1.5,
        priority: 1,
        rank: 1,
        allow_overlap: false,
        ignore_placement: false,
        avoid_edges: false,
        text_offset_pixels: [0.0, 0.0],
        text_padding_pixels: LABEL_COLLISION_PADDING_PIXELS,
        text_pitch_alignment: TextPitchAlignment::Viewport,
        repeat_spacing_pixels: LINE_LABEL_REPEAT_SPACING_PIXELS,
        line_center_only: false,
        text_max_width_ems: 10.0,
        text_line_height_ems: 1.2,
        text_letter_spacing_ems: 0.0,
        text_justify: rustic_maps_style::TextJustify::Auto,
        text_anchor: rustic_maps_style::TextAnchor::Center,
        text_rotation_alignment: rustic_maps_style::TextRotationAlignment::Auto,
        text_keep_upright: true,
        text_variable_anchors: Vec::new(),
        text_radial_offset_ems: 0.0,
        text_max_angle_radians: LINE_LABEL_STRAIGHT_RUN_ANGLE_TOLERANCE_RADIANS,
        style_order: 0,
        icon: None,
        shield: None,
    }
}

#[test]
fn the_demo_tiles_street_names_place_at_the_demo_zoom() {
    // The road-name path end to end without a GPU: the Bright style's
    // transportation_name bands read the demo tile's street names, and at
    // the fixture tile's own center at the demo zoom the longest straight
    // runs carry several of them.
    let tile = decode_vector_tile(OPENFREEMAP_TILE_BYTES).unwrap();
    let style = vector_tile_style_from_id(VECTOR_STYLE_ID_BRIGHT_LIGHT)
        .expect("the committed Bright style loads");
    let line_candidates: Vec<MapLabelCandidate> =
        label_candidates_for_vector_tile(&tile, TILE_ADDRESS, &style, 14.0)
            .into_iter()
            .filter(|candidate| !candidate.line_coordinates.is_empty())
            .collect();
    assert!(!line_candidates.is_empty());
    let fixture_tile_center_camera = MapCamera::new(
        GeographicCoordinate::from_degrees(
            FIXTURE_TILE_CENTER_LATITUDE_DEGREES,
            FIXTURE_TILE_CENTER_LONGITUDE_DEGREES,
        )
        .unwrap(),
        14.0,
        0.0,
        0.0,
    );
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let layer = LabelLayer::place(
        &fixture_tile_center_camera,
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &line_candidates,
        &fixture_font(),
    );
    assert!(
        !layer.is_empty(),
        "the demo tile's street names fit the demo zoom's straight runs"
    );
    for label in layer.labels() {
        assert!(label.placed_along_line);
        assert!(
            (-crate::labels::LINE_LABEL_UPRIGHT_ROTATION_LIMIT_RADIANS
                ..=crate::labels::LINE_LABEL_UPRIGHT_ROTATION_LIMIT_RADIANS)
                .contains(&label.rotation_radians)
        );
    }
    assert!(
        !layer.quad_vertices(&viewport).text.is_empty(),
        "the placed street names rasterize into glyph quads"
    );
}

#[test]
fn a_line_symbol_layer_reads_the_demo_tiles_road_names() {
    let tile = decode_vector_tile(OPENFREEMAP_TILE_BYTES).unwrap();
    let style = compiled_style(
        r#"{
            "version": 8,
            "sources": { "base": { "type": "vector" } },
            "layers": [
                {
                    "id": "road-name",
                    "type": "symbol",
                    "source": "base",
                    "source-layer": "transportation_name",
                    "minzoom": 12,
                    "maxzoom": 18,
                    "layout": {
                        "text-field": ["coalesce", ["get", "name:latin"], ["get", "name"], ["get", "ref"]],
                        "text-size": 12,
                        "symbol-placement": "line"
                    },
                    "paint": { "text-color": [51, 51, 51, 1] }
                }
            ]
        }"#,
    );
    let candidates = label_candidates_for_vector_tile(&tile, TILE_ADDRESS, &style, 14.0);
    assert!(
        !candidates.is_empty(),
        "the demo tile's transportation_name layer carries road names"
    );
    for candidate in &candidates {
        assert!(
            candidate.line_coordinates.len() >= 2,
            "a line candidate carries its whole line geometry"
        );
        assert!(!candidate.text.is_empty());
        assert_eq!(candidate.style_order, 0);
    }
}

#[test]
fn a_line_label_centers_and_rotates_along_its_longest_straight_run() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    // Three collinear vertices are one straight run short enough for one
    // label; it centers at half of the run's arc length, which is not any
    // of the vertices.
    let line_coordinates = vec![
        coordinate_offset_from_sacramento(-0.002, 0.003),
        coordinate_offset_from_sacramento(-0.001, 0.003),
        coordinate_offset_from_sacramento(0.002, 0.003),
    ];
    let layer = LabelLayer::place(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &[line_label_candidate("N St", line_coordinates, 12.0)],
        &font,
    );
    assert_eq!(layer.labels().len(), 1);
    let label = &layer.labels()[0];
    assert!(
        label.rotation_radians.abs() < 1e-9,
        "a horizontal run places unrotated"
    );
    let expected_anchor = sacramento_camera()
        .project_to_screen(
            &coordinate_offset_from_sacramento(0.0, 0.003),
            &viewport,
            crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        )
        .unwrap();
    assert!((label.anchor_screen_pixels[0] - expected_anchor[0]).abs() < 1e-6);
    assert!((label.anchor_screen_pixels[1] - expected_anchor[1]).abs() < 1e-6);
}

#[test]
fn a_long_street_repeats_its_name_along_the_run() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    // A run long enough for several names: a street crossing the frame is
    // named more than once, so a reader finds it wherever they look.
    let line_coordinates = vec![
        coordinate_offset_from_sacramento(-0.012, 0.002),
        coordinate_offset_from_sacramento(0.0, 0.002),
        coordinate_offset_from_sacramento(0.012, 0.002),
    ];
    let layer = LabelLayer::place(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &[line_label_candidate("Capitol Mall", line_coordinates, 12.0)],
        &font,
    );
    let labels = layer.labels();
    assert!(
        labels.len() >= 2,
        "a frame-wide street carries more than one name, got {}",
        labels.len()
    );
    for label in labels {
        assert_eq!(label.text, "Capitol Mall");
        assert!(label.placed_along_line);
        assert!(
            label.rotation_radians.abs() < 1e-9,
            "every repeated name of a horizontal run places unrotated"
        );
    }
    let mut anchor_positions: Vec<f64> = labels
        .iter()
        .map(|label| label.anchor_screen_pixels[0])
        .collect();
    anchor_positions.sort_by(f64::total_cmp);
    for anchor_pair in anchor_positions.windows(2) {
        assert!(
            anchor_pair[1] - anchor_pair[0] > LINE_LABEL_REPEAT_SPACING_PIXELS,
            "repeated names keep at least the repeat spacing between anchors"
        );
    }
}

#[test]
fn a_turn_past_the_straight_run_tolerance_breaks_the_run() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    // A right angle: a short east leg and a longer north leg. The label
    // must take the north run (not straddle both legs), so its rotation
    // points along the vertical.
    let line_coordinates = vec![
        coordinate_offset_from_sacramento(-0.002, 0.0),
        coordinate_offset_from_sacramento(0.002, 0.0),
        coordinate_offset_from_sacramento(0.002, 0.012),
    ];
    let layer = LabelLayer::place(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &[line_label_candidate("3rd Street", line_coordinates, 11.0)],
        &font,
    );
    assert!(!layer.labels().is_empty());
    for label in layer.labels() {
        assert!(
            label.rotation_radians.abs() > 1.0,
            "the longer north leg rotates every name toward vertical"
        );
    }
}

#[test]
fn a_line_label_flips_its_rotation_so_the_text_reads_upright() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    // The line runs from the north-east to the south-west, a 135-degree
    // screen bearing (down and to the left); flipping it yields the
    // -45-degree upright label.
    let line_coordinates = vec![
        coordinate_offset_from_sacramento(0.008, 0.008),
        coordinate_offset_from_sacramento(-0.008, -0.008),
    ];
    let layer = LabelLayer::place(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &[line_label_candidate("Capitol Mall", line_coordinates, 12.0)],
        &font,
    );
    assert!(!layer.labels().is_empty());
    for label in layer.labels() {
        let rotation_radians = label.rotation_radians;
        assert!(
            (-LINE_LABEL_UPRIGHT_ROTATION_LIMIT_RADIANS
                ..=LINE_LABEL_UPRIGHT_ROTATION_LIMIT_RADIANS)
                .contains(&rotation_radians),
            "an upright label never rotates past a quarter turn, got {rotation_radians}"
        );
        assert!(
            rotation_radians < 0.0,
            "the up-left bearing flips to a downward-reading positive-x run"
        );
    }
}

#[test]
fn a_line_label_wider_than_every_segment_is_skipped() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    // Two hundredths of a degree is a couple of pixels at zoom 14; this
    // name cannot fit between the collision paddings.
    let line_coordinates = vec![
        coordinate_offset_from_sacramento(-0.0001, 0.0),
        coordinate_offset_from_sacramento(0.0001, 0.0),
    ];
    let layer = LabelLayer::place(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &[line_label_candidate("Sacramento", line_coordinates, 12.0)],
        &font,
    );
    assert!(layer.is_empty());
}

#[test]
fn a_rotated_label_rejects_a_colliding_line_label_like_a_point_label() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let horizontal_line = || {
        vec![
            coordinate_offset_from_sacramento(-0.004, 0.0),
            coordinate_offset_from_sacramento(0.004, 0.0),
        ]
    };
    let layer = LabelLayer::place(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &[
            line_label_candidate("N St", horizontal_line(), 12.0),
            line_label_candidate("M St", horizontal_line(), 12.0),
        ],
        &font,
    );
    let labels = layer.labels();
    assert!(!labels.is_empty());
    assert!(
        labels.iter().all(|label| label.text == "N St"),
        "the second street runs over the first and is rejected whole, repeats included"
    );
}

#[test]
fn a_rotated_label_emits_finite_text_quads() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let line_coordinates = vec![
        coordinate_offset_from_sacramento(-0.006, 0.004),
        coordinate_offset_from_sacramento(0.006, -0.004),
    ];
    let layer = LabelLayer::place(
        &sacramento_camera(),
        &viewport,
        crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &[line_label_candidate("Capitol Mall", line_coordinates, 12.0)],
        &font,
    );
    assert!(!layer.labels().is_empty());
    for label in layer.labels() {
        assert_ne!(label.rotation_radians, 0.0);
    }
    let vertices = layer.quad_vertices(&viewport);
    assert!(!vertices.text.is_empty());
    assert_eq!(vertices.halo.len(), vertices.text.len());
    for vertex in &vertices.text {
        assert!(
            vertex
                .clip_position
                .iter()
                .all(|component| component.is_finite())
        );
    }
}

/// One Sacramento point candidate carrying the caller's fidelity values.
fn fidelity_candidate(
    text: &str,
    text_padding_pixels: f64,
    text_pitch_alignment: TextPitchAlignment,
    icon: Option<MapIconCandidate>,
) -> MapLabelCandidate {
    MapLabelCandidate {
        text: text.to_string(),
        anchor: GeographicCoordinate::from_degrees(
            SACRAMENTO_LATITUDE_DEGREES,
            SACRAMENTO_LONGITUDE_DEGREES,
        )
        .unwrap(),
        line_coordinates: Vec::new(),
        text_rgba: [0.0, 0.0, 0.0, 1.0],
        text_size_pixels: 13.0,
        halo_rgba: [1.0, 1.0, 1.0, 1.0],
        halo_width_pixels: 1.0,
        priority: 0,
        rank: 0,
        allow_overlap: false,
        ignore_placement: false,
        avoid_edges: false,
        text_offset_pixels: [0.0, 0.0],
        text_padding_pixels,
        text_pitch_alignment,
        repeat_spacing_pixels: LINE_LABEL_REPEAT_SPACING_PIXELS,
        line_center_only: false,
        text_max_width_ems: 10.0,
        text_line_height_ems: 1.2,
        text_letter_spacing_ems: 0.0,
        text_justify: TextJustify::Auto,
        text_anchor: TextAnchor::Center,
        text_rotation_alignment: TextRotationAlignment::Viewport,
        text_keep_upright: true,
        text_variable_anchors: Vec::new(),
        text_radial_offset_ems: 0.0,
        text_max_angle_radians: LINE_LABEL_STRAIGHT_RUN_ANGLE_TOLERANCE_RADIANS,
        style_order: 0,
        icon,
        shield: None,
    }
}

#[test]
fn the_declared_text_padding_inflates_the_collision_boxes() {
    let font = fixture_font();
    let text_block = font
        .measure_text("Sacramento", 13.0)
        .map(|metrics| LabelTextBlock {
            lines: vec![LabelTextLine {
                text: "Sacramento".to_string(),
                width_pixels: metrics.width_pixels,
            }],
            width_pixels: metrics.width_pixels,
            height_pixels: metrics.height_pixels,
            line_height_pixels: metrics.height_pixels,
        })
        .expect("the fixture font measures the demo name");
    let placement = LabelPlacement {
        anchor_screen_pixels: [200.0, 200.0],
        rotation_radians: 0.0,
    };
    let unpadded = glyph_collision_boxes(
        &font,
        &text_block,
        13.0,
        0.0,
        TextJustify::Auto,
        0.0,
        1.0,
        placement,
    );
    let padded = glyph_collision_boxes(
        &font,
        &text_block,
        13.0,
        0.0,
        TextJustify::Auto,
        6.0,
        1.0,
        placement,
    );
    assert_eq!(unpadded.len(), padded.len());
    for (unpadded_box, padded_box) in unpadded.iter().zip(&padded) {
        assert!(
            (padded_box.half_width_pixels - unpadded_box.half_width_pixels - 6.0).abs() < 1e-9,
            "text-padding widens every glyph box by the declared margin"
        );
        assert!(
            (padded_box.half_height_pixels - unpadded_box.half_height_pixels - 6.0).abs() < 1e-9
        );
    }
}

#[test]
fn a_fitted_icon_stretches_to_the_text_block_plus_its_padding() {
    let atlas = fixture_icon_sprite_atlas();
    let camera = sacramento_camera();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let anchor = GeographicCoordinate::from_degrees(
        SACRAMENTO_LATITUDE_DEGREES,
        SACRAMENTO_LONGITUDE_DEGREES,
    )
    .unwrap();
    let mut icon = fixture_icon_candidate();
    icon.text_fit = IconTextFit::Both;
    icon.text_fit_padding_pixels = [1.0, 2.0, 3.0, 4.0];
    let placement = icon_placement_at(
        &camera,
        &viewport,
        DEFAULT_FIELD_OF_VIEW_DEGREES,
        &anchor,
        &icon,
        Some([40.0, 12.0]),
        Some(&atlas),
        1.0,
    )
    .expect("the fitted icon places");
    assert!((placement.width_pixels - (40.0 + 4.0 + 2.0)).abs() < 1e-9);
    assert!((placement.height_pixels - (12.0 + 1.0 + 3.0)).abs() < 1e-9);

    let unfitted = icon_placement_at(
        &camera,
        &viewport,
        DEFAULT_FIELD_OF_VIEW_DEGREES,
        &anchor,
        &fixture_icon_candidate(),
        Some([40.0, 12.0]),
        Some(&atlas),
        1.0,
    )
    .expect("the unfitted icon places");
    assert!(
        (unfitted.width_pixels - icon_size_pixels(&fixture_icon_candidate(), &atlas)).abs() < 1e-9,
        "icon-text-fit: none keeps the sheet's own size"
    );
}

/// The drawn width the sheet gives one fixture icon at its declared scale.
fn icon_size_pixels(candidate: &MapIconCandidate, atlas: &SpriteAtlas) -> f64 {
    atlas
        .icon(&candidate.image_name)
        .expect("the fixture atlas serves the fixture icon")
        .display_size_pixels(candidate.size)[0]
}

#[test]
fn a_map_aligned_label_foreshortens_at_pitch_while_a_viewport_label_does_not() {
    let font = fixture_font();
    let viewport = ViewportPixels::new(512.0, 512.0).unwrap();
    let pitched_camera = MapCamera::new(
        GeographicCoordinate::from_degrees(
            SACRAMENTO_LATITUDE_DEGREES,
            SACRAMENTO_LONGITUDE_DEGREES,
        )
        .unwrap(),
        14.0,
        0.0,
        60.0,
    );
    let map_aligned = LabelLayer::place(
        &pitched_camera,
        &viewport,
        DEFAULT_FIELD_OF_VIEW_DEGREES,
        &[fidelity_candidate(
            "Sacramento",
            2.0,
            TextPitchAlignment::Map,
            None,
        )],
        &font,
    );
    let viewport_aligned = LabelLayer::place(
        &pitched_camera,
        &viewport,
        DEFAULT_FIELD_OF_VIEW_DEGREES,
        &[fidelity_candidate(
            "Sacramento",
            2.0,
            TextPitchAlignment::Viewport,
            None,
        )],
        &font,
    );
    let map_scale = map_aligned.labels()[0].pitch_scale_y;
    assert!(
        (map_scale - 0.5).abs() < 1e-9,
        "cos(60°) foreshortens a map-plane label: {map_scale}"
    );
    assert_eq!(viewport_aligned.labels()[0].pitch_scale_y, 1.0);
}
