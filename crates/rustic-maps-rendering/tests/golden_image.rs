//! Golden-image harness for the headless raster-tile renderer.
//!
//! Two deterministic scenes — Sacramento (California State Capitol) with a top-down camera and the same
//! center with a 45-degree pitched camera — are rendered into an RGBA byte
//! buffer with real recorded platform imagery and compared against PNG
//! fixtures under `fixtures/render/golden/`.
//!
//! The tile payloads come from the recorded platform tiles under
//! `fixtures/render/tiles/` (one `zoom_tile_x_tile_y.png` per tile, recorded
//! by the `record_golden_tiles` example exactly for these scenes), so the
//! goldens exercise real raster tiles rather than synthetic patterns; the
//! tests themselves never touch the network. A tile missing from disk is a
//! hard test error naming the path and the recording command.
//!
//! Golden comparison is deliberately tolerant, never pixel-exact: GPUs differ
//! in sampler behavior, so the mean absolute channel error must stay under a
//! named constant and only a small fraction of pixels may exceed an outlier
//! error. A missing golden fixture is written on first run (documented
//! behavior so the reference takes shape on this machine) and verified as
//! equal on subsequent runs.
//!
//! Both tests skip gracefully — printing the reason to stdout and returning
//! early — when no graphics adapter is available, so CPU-only environments
//! pass.

mod common;

use common::golden::{GoldenTolerance, compare_or_create_golden};
use image::GenericImageView as _;
use rustic_maps_glyphs::decode_glyph_font_stack;
use rustic_maps_rendering::vector_style::{
    VECTOR_STYLE_ID_BRIGHT_LIGHT, VectorTileAddress, vector_tile_style_from_id,
};
use rustic_maps_rendering::{
    CapturedFrame, DEFAULT_FIELD_OF_VIEW_DEGREES, GraphicsContextSource, HeadlessTileRenderer,
    LabelFont, LabelLayer, MapCamera, PreparedTile, RetainedTileLevel, TileKey, TileReadiness,
    TileTexturePool, ViewportPixels, WgpuGraphicsContextSource, label_candidates_for_vector_tile,
    normalized_world_to_coordinate,
};
use rustic_maps_vector_tiles::decode_vector_tile;
use std::path::PathBuf;

/// Frame edge in pixels for both golden scenes; small enough for a fast test,
/// and a whole power of two so tile spans map cleanly.
const GOLDEN_VIEWPORT_EDGE_PIXELS: f64 = 512.0;

/// Sacramento (California State Capitol) center, the fixed camera anchor of
/// the golden scenes.
const SACRAMENTO_LATITUDE_DEGREES: f64 = 38.5766;
/// Sacramento center longitude.
const SACRAMENTO_LONGITUDE_DEGREES: f64 = -121.4944;
/// Tile zoom at which the scene's tiles are scheduled and rendered.
const SCENE_TILE_ZOOM_LEVEL: u8 = 14;
/// Bearing of both golden cameras: north up.
const GOLDEN_BEARING_DEGREES: f64 = 0.0;
/// Pitch of the first golden camera: straight down.
const TOP_DOWN_PITCH_DEGREES: f64 = 0.0;
/// Pitch of the second golden camera.
const PITCHED_PITCH_DEGREES: f64 = 45.0;

/// Latitude of the south-east corner of the committed z14 vector fixture tile
/// (38.565348 N, the 6287 boundary). The road-name scene centers on that
/// corner: the fixture tile fills the view's upper-left quadrant and its
/// neighbour tile to the east keeps the deliberate parent-fallback fixture
/// (the z13 parent of tile 2662,6287) in the committed set.
const ROAD_NAME_SCENE_LATITUDE_DEGREES: f64 = 38.565_347_844_885_46;
/// Longitude of the fixture tile's south-east corner (121.508789 W).
const ROAD_NAME_SCENE_LONGITUDE_DEGREES: f64 = -121.508_789_062_5;

/// Fixture of the top-down scene.
const TOP_DOWN_GOLDEN_FILE: &str = "sacramento_zoom14_top_down.png";
/// Fixture of the pitched scene.
const PITCHED_GOLDEN_FILE: &str = "sacramento_zoom14_pitched45.png";
/// Fixture of the place-label scene: the top-down camera with the Bright
/// vector style's point labels drawn over the recorded raster tiles.
const PLACE_LABELS_GOLDEN_FILE: &str = "sacramento_zoom14_place_labels.png";
/// Fixture of the road-name scene: a top-down camera on the vector fixture
/// tile's south-east corner, with the Bright style's line-placed street names
/// drawn over the recorded raster tiles. The place labels are deliberately
/// left out so the rotated road names are the frame's only text.
const ROAD_NAMES_GOLDEN_FILE: &str = "sacramento_zoom14_road_names.png";
/// Fixture of the dense regional label scene: the Bright style's places and
/// road names from the committed OpenMapTiles fixture, resolved and projected
/// at zoom 13 so many names compete for one frame.
const REGIONAL_LABELS_GOLDEN_FILE: &str = "sacramento_region_zoom13_labels.png";
/// Fixture of the wide regional thinning scene: the same candidates at zoom 8,
/// where they pile into a few pixels and regional thinning leaves the most
/// important name.
const REGIONAL_THINNING_GOLDEN_FILE: &str = "sacramento_region_zoom8_labels.png";

/// The dense regional label scene's camera zoom: the z14 fixture's labels
/// resolve at z13 and its ground span halves to 256 screen pixels, so the
/// places, the motorway/trunk names and their shields share one view.
const REGIONAL_LABEL_SCENE_ZOOM_LEVEL: f64 = 13.0;
/// The wide regional thinning scene's camera zoom: every candidate from the
/// tile piles into roughly two screen pixels, so only the thinned survivors
/// reach the collision pass.
const REGIONAL_THINNING_SCENE_ZOOM_LEVEL: f64 = 8.0;
/// The committed Bright style's canvas background (`vector-style-bright-light`
/// `background`), so the label goldens sit on the style's own paper colour.
const BRIGHT_STYLE_BACKGROUND_RGBA: [f32; 4] = [0.972_549, 0.956_863, 0.941_176, 1.0];
/// The committed vector fixture tile's center, so the regional label cameras
/// keep the whole tile (and every one of its place names) in view.
const REGIONAL_LABEL_CENTER_LATITUDE_DEGREES: f64 = 38.573_937_515_575_91;
const REGIONAL_LABEL_CENTER_LONGITUDE_DEGREES: f64 = -121.519_775_390_625;

/// Zoom of the mid-fade golden's requested tile level: the recorded z15 set
/// around the zoom-15 anchor below.
const CROSS_FADE_REQUESTED_TILE_ZOOM_LEVEL: u8 = 15;
/// Zoom of the level the mid-fade golden dissolves out: the recorded z14
/// parent tiles that cover the same ground.
const CROSS_FADE_RETAINED_TILE_ZOOM_LEVEL: u8 = 14;
/// Opacity of the retired level in the mid-fade frame: half way through the
/// cross-fade.
const CROSS_FADE_RETAINED_OPACITY: f32 = 0.5;
/// The requested-level tile whose north-west corner anchors the scene, at a
/// spot where every recorded z15 tile and every z14 parent both exist.
const CROSS_FADE_ANCHOR_TILE_X: u32 = 5326;
/// See [`CROSS_FADE_ANCHOR_TILE_X`].
const CROSS_FADE_ANCHOR_TILE_Y: u32 = 12572;
/// Fixture of the mid-fade scene.
const CROSS_FADE_GOLDEN_FILE: &str = "sacramento_zoom15_cross_fade_half.png";

/// The committed Bright vector style's id (`platform/providers.json`'s
/// openfreemap descriptor offers it as `bright-light`), whose `place` symbol
/// layer names the demo's city labels.
const PLACE_LABEL_STYLE_ID: &str = VECTOR_STYLE_ID_BRIGHT_LIGHT;
/// The committed OpenFreeMap z14 vector tile the label golden reads its
/// labels from (the demo's own tile fixture; its `place` layer carries
/// Sacramento, West Sacramento, Mikon and Bryte).
const PLACE_LABEL_VECTOR_TILE_FIXTURE_PATH: &str =
    "../../fixtures/openfreemap/openmaptiles_tile_z14_x2661_y6286.pbf";
/// The vector fixture tile's address at the scene zoom.
const PLACE_LABEL_VECTOR_TILE_X: u32 = 2661;
/// The vector fixture tile's row at the scene zoom.
const PLACE_LABEL_VECTOR_TILE_Y: u32 = 6286;
/// The committed Latin glyph range the label pass draws with (see its
/// fixture README for provenance and the recording URL).
const PLACE_LABEL_GLYPH_FIXTURE_PATH: &str = "../../fixtures/glyphs/noto_sans_regular_0-255.pbf";
/// Encoded-pixel budget of the tile pool the scenes fill (each recorded tile
/// PNG is a fraction of this).
const SCENE_POOL_BUDGET_BYTES: u64 = 64 << 20;

/// Directory the recorded platform tiles live in, relative to the crate
/// manifest; the `record_golden_tiles` example writes exactly these files.
const TILE_FIXTURE_DIRECTORY: &str = "fixtures/render/tiles";
/// Command that records the tiles `TILE_FIXTURE_DIRECTORY` is expected to
/// hold; embedded in the error when a tile fixture is missing.
const RECORD_TILES_COMMAND: &str =
    "cargo run -p rustic-maps-rendering --example record_golden_tiles";
/// Edge length in texels the platform raster tiles are recorded at. At the
/// scene's tile zoom the ground edge of one tile is
/// [`GOLDEN_VIEWPORT_EDGE_PIXELS`] pixels wide, so the sampler upscales the
/// recorded texels two fold; linear filtering smooths the upscale, and scene
/// coverage comes from the tile spans, not from the texel size.
const PLATFORM_RASTER_TILE_EDGE_PIXELS: u32 = 256;

/// Builds the scene camera over Sacramento.
fn sacramento_camera(pitch_degrees: f64) -> MapCamera {
    camera_at(
        SACRAMENTO_LATITUDE_DEGREES,
        SACRAMENTO_LONGITUDE_DEGREES,
        pitch_degrees,
    )
}

/// Builds a scene camera at a fixed center and the scene's zoom, north up.
fn camera_at(latitude_degrees: f64, longitude_degrees: f64, pitch_degrees: f64) -> MapCamera {
    MapCamera::new(
        geospatial_types::GeographicCoordinate::from_degrees(latitude_degrees, longitude_degrees)
            .expect("the golden scene anchor is a valid coordinate"),
        f64::from(SCENE_TILE_ZOOM_LEVEL),
        GOLDEN_BEARING_DEGREES,
        pitch_degrees,
    )
}

/// Builds the scene viewport.
fn golden_viewport() -> ViewportPixels {
    ViewportPixels::new(GOLDEN_VIEWPORT_EDGE_PIXELS, GOLDEN_VIEWPORT_EDGE_PIXELS)
        .expect("the golden viewport is a valid viewport")
}

/// Returns the file a recorded tile lives in, and the record command's
/// workspace root the path is relative to.
fn recorded_tile_path(tile_key: TileKey) -> PathBuf {
    let file_name = format!(
        "{}_{}_{}.png",
        tile_key.zoom_level, tile_key.tile_x, tile_key.tile_y
    );
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(TILE_FIXTURE_DIRECTORY)
        .join(file_name)
}

/// Loads a recorded platform tile payload from the tile fixture directory.
/// A missing fixture is a hard test error naming the path and the recording
/// command — the tests never synthesize a stand-in and never fetch.
fn load_recorded_tile_payload(tile_key: TileKey) -> Vec<u8> {
    let tile_path = recorded_tile_path(tile_key);
    if !tile_path.exists() {
        panic!(
            "recorded tile fixture {:?} is missing; run `{RECORD_TILES_COMMAND}` to record it",
            tile_path
        );
    }
    let encoded_payload_bytes =
        std::fs::read(&tile_path).expect("the recorded tile fixture is readable");
    let decoded_payload = image::load_from_memory(&encoded_payload_bytes).unwrap_or_else(|error| {
        panic!("recorded tile fixture {tile_path:?} is undecodable: {error}")
    });
    let (payload_width_pixels, payload_height_pixels) = decoded_payload.dimensions();
    assert_eq!(
        (payload_width_pixels, payload_height_pixels),
        (
            PLATFORM_RASTER_TILE_EDGE_PIXELS,
            PLATFORM_RASTER_TILE_EDGE_PIXELS
        ),
        "recorded tile fixture {tile_path:?} has an unexpected texel edge; \
         the platform imagery changed shape, so re-record and re-review the goldens"
    );
    encoded_payload_bytes
}

/// Fills the pool with every scene tile's recorded payload plus the shared
/// parent for the fallback draw, and marks every tile ready (the last one via
/// its parent).
fn fill_pool_and_prepare(camera: &MapCamera) -> (TileTexturePool, Vec<PreparedTile>) {
    let scene_tile_keys = camera.visible_tile_keys(
        &golden_viewport(),
        rustic_maps_rendering::DEFAULT_FIELD_OF_VIEW_DEGREES,
        SCENE_TILE_ZOOM_LEVEL,
    );
    let fallback_tile = *scene_tile_keys
        .last()
        .expect("the golden scene always shows at least one tile");
    let fallback_parent = fallback_tile
        .parent_tile_key()
        .expect("the fallback tile is not the world root");

    let mut payload_pool = TileTexturePool::new(SCENE_POOL_BUDGET_BYTES);
    for tile_key in &scene_tile_keys {
        payload_pool.insert(*tile_key, load_recorded_tile_payload(*tile_key));
    }
    payload_pool.insert(fallback_parent, load_recorded_tile_payload(fallback_parent));

    let prepared_tiles = scene_tile_keys
        .into_iter()
        .map(|tile_key| {
            if tile_key == fallback_tile {
                PreparedTile {
                    requested_tile_key: tile_key,
                    readiness: TileReadiness::FallsBackTo {
                        fallback_tile_key: fallback_parent,
                        zoom_level_difference: 1,
                    },
                }
            } else {
                PreparedTile {
                    requested_tile_key: tile_key,
                    readiness: TileReadiness::Present,
                }
            }
        })
        .collect();
    (payload_pool, prepared_tiles)
}

/// Renders the scene without touching any fixture, for debugging.
fn render_scene(renderer: &mut HeadlessTileRenderer, pitch_degrees: f64) -> CapturedFrame {
    let camera = sacramento_camera(pitch_degrees);
    let (mut payload_pool, prepared_tiles) = fill_pool_and_prepare(&camera);
    renderer
        .render_captured_frame(
            &camera,
            &golden_viewport(),
            &prepared_tiles,
            &mut payload_pool,
        )
        .expect("the golden scene always renders")
}

/// Builds the label candidates for a scene: the committed OpenFreeMap vector
/// tile's Bright-style symbol text resolved at `style_zoom_level`. Both
/// fixtures are read from disk; nothing here touches the network.
fn sacramento_label_candidates_at_style_zoom(
    style_zoom_level: f64,
) -> Vec<rustic_maps_rendering::MapLabelCandidate> {
    let vector_tile_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(PLACE_LABEL_VECTOR_TILE_FIXTURE_PATH);
    let vector_tile_bytes = std::fs::read(&vector_tile_path).unwrap_or_else(|error| {
        panic!("the vector fixture {vector_tile_path:?} is unreadable: {error}")
    });
    let decoded_tile = decode_vector_tile(&vector_tile_bytes).unwrap_or_else(|decode_error| {
        panic!("the vector fixture {vector_tile_path:?} is undecodable: {decode_error}")
    });
    let style = vector_tile_style_from_id(PLACE_LABEL_STYLE_ID)
        .expect("the committed Bright style artifact loads");
    label_candidates_for_vector_tile(
        &decoded_tile,
        VectorTileAddress {
            zoom_level: SCENE_TILE_ZOOM_LEVEL,
            tile_x: PLACE_LABEL_VECTOR_TILE_X,
            tile_y: PLACE_LABEL_VECTOR_TILE_Y,
        },
        &style,
        style_zoom_level,
    )
}

fn sacramento_label_candidates() -> Vec<rustic_maps_rendering::MapLabelCandidate> {
    sacramento_label_candidates_at_style_zoom(f64::from(SCENE_TILE_ZOOM_LEVEL))
}

/// Decodes and packs the committed Latin glyph range the label scenes draw
/// with. The fixture is read from disk, never fetched.
fn sacramento_label_font() -> LabelFont {
    let glyph_fixture_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(PLACE_LABEL_GLYPH_FIXTURE_PATH);
    let glyph_range_bytes = std::fs::read(&glyph_fixture_path).unwrap_or_else(|error| {
        panic!("the glyph fixture {glyph_fixture_path:?} is unreadable: {error}")
    });
    let font_stack = decode_glyph_font_stack(&glyph_range_bytes).unwrap_or_else(|decode_error| {
        panic!("the glyph fixture {glyph_fixture_path:?} is undecodable: {decode_error}")
    });
    LabelFont::from_glyph_font_stack(&font_stack)
        .expect("the committed Latin range fits the default atlas page")
}

/// Builds the place-label layer for the scene: the Bright-style point labels
/// projected and collision-placed at the scene camera. Line-placed candidates
/// are dropped so the frame stays the point-label golden it always was.
fn sacramento_place_label_layer(camera: &MapCamera) -> LabelLayer {
    let point_candidates: Vec<rustic_maps_rendering::MapLabelCandidate> =
        sacramento_label_candidates()
            .into_iter()
            .filter(|candidate| candidate.line_coordinates.is_empty())
            .collect();
    LabelLayer::place(
        camera,
        &golden_viewport(),
        DEFAULT_FIELD_OF_VIEW_DEGREES,
        &point_candidates,
        &sacramento_label_font(),
    )
}

/// Builds the road-name layer for the scene: the Bright-style line-placed
/// street names placed along their longest straight projected run at the
/// scene camera.
fn sacramento_road_name_label_layer(camera: &MapCamera) -> LabelLayer {
    let line_candidates: Vec<rustic_maps_rendering::MapLabelCandidate> =
        sacramento_label_candidates()
            .into_iter()
            .filter(|candidate| !candidate.line_coordinates.is_empty())
            .collect();
    LabelLayer::place(
        camera,
        &golden_viewport(),
        DEFAULT_FIELD_OF_VIEW_DEGREES,
        &line_candidates,
        &sacramento_label_font(),
    )
}

/// Acquires the headless renderer for the golden tests, printing the backend,
/// or `None` when no adapter is available so CPU-only environments skip.
fn request_renderer_or_skip() -> Option<HeadlessTileRenderer> {
    match WgpuGraphicsContextSource.request_graphics_context() {
        Ok(graphics_context) => {
            println!(
                "rendering on backend {:?} with adapter {:?}",
                graphics_context.adapter_information.backend,
                graphics_context.adapter_information.name
            );
            let renderer = HeadlessTileRenderer::with_graphics_context(graphics_context)
                .expect("the pipeline builds on the acquired context");
            Some(renderer)
        }
        Err(error) => {
            println!("skipping GPU golden test: no graphics adapter available ({error})");
            None
        }
    }
}

#[test]
fn sacramento_top_down_scene_matches_its_golden_fixture() {
    let Some(mut renderer) = request_renderer_or_skip() else {
        return;
    };
    let captured_frame = render_scene(&mut renderer, TOP_DOWN_PITCH_DEGREES);
    compare_or_create_golden(
        &captured_frame,
        TOP_DOWN_GOLDEN_FILE,
        "the top-down scene",
        GoldenTolerance::STANDARD,
    );
}

#[test]
fn sacramento_pitched_scene_matches_its_golden_fixture() {
    let Some(mut renderer) = request_renderer_or_skip() else {
        return;
    };
    let captured_frame = render_scene(&mut renderer, PITCHED_PITCH_DEGREES);
    compare_or_create_golden(
        &captured_frame,
        PITCHED_GOLDEN_FILE,
        "the pitched scene",
        GoldenTolerance::STANDARD,
    );
}

// --- SDF coverage profile (M30 Slice D) --------------------------------
//
// A glyph rendered on its own over the clear colour, with no halo and no
// tiles, isolates the shader's distance-field edge. The profile test renders
// the letter `I` at two sizes and asserts the two facts the SDF math exists
// for: a stroke interior reaches the text colour at (almost) full coverage,
// and the transition across a vertical stroke edge spans at most the
// documented ~1.68-device-pixel band (2 pixels, rounded).

/// The isolated label viewport; a power of two so the glyph sits on a clean
/// pixel grid.
const COVERAGE_PROFILE_VIEWPORT_EDGE_PIXELS: f64 = 256.0;
/// The two text sizes the profile renders (device pixels).
const COVERAGE_PROFILE_TEXT_SIZES_PIXELS: [f64; 2] = [36.0, 48.0];
/// Coverage a stroke interior must reach: the text colour at 98 % or more.
const COVERAGE_INTERIOR_MINIMUM: f64 = 0.98;
/// The coverage band counted as an edge pixel: above background, below the
/// interior floor. The near-full pixels just under the interior floor are
/// part of the plateau, not the transition.
const COVERAGE_EDGE_MINIMUM: f64 = 0.05;
/// The upper end of the edge band (the interior floor's complement).
const COVERAGE_EDGE_MAXIMUM: f64 = 0.95;
/// The widest intermediate run (in pixels) a vertical stroke edge may span:
/// the shader's half-band is 0.84 device pixels, so a full transition is
/// ~1.68 pixels and rounds to at most two sampled pixels.
const COVERAGE_MAXIMUM_EDGE_RUN_PIXELS: usize = 2;

/// Renders one white `I` on the clear colour at the given text size.
fn render_coverage_profile_frame(
    renderer: &mut HeadlessTileRenderer,
    text_size_pixels: f64,
) -> CapturedFrame {
    let center = geospatial_types::GeographicCoordinate::from_degrees(
        SACRAMENTO_LATITUDE_DEGREES,
        SACRAMENTO_LONGITUDE_DEGREES,
    )
    .expect("the profile camera's center is a canonical coordinate");
    let camera = MapCamera::new(center, f64::from(SCENE_TILE_ZOOM_LEVEL), 0.0, 0.0);
    let viewport = ViewportPixels::new(
        COVERAGE_PROFILE_VIEWPORT_EDGE_PIXELS,
        COVERAGE_PROFILE_VIEWPORT_EDGE_PIXELS,
    )
    .expect("the profile viewport is positive");
    let candidate = rustic_maps_rendering::MapLabelCandidate {
        text: "I".to_string(),
        anchor: center,
        line_coordinates: Vec::new(),
        text_rgba: [1.0, 1.0, 1.0, 1.0],
        text_size_pixels,
        halo_rgba: [0.0, 0.0, 0.0, 0.0],
        halo_width_pixels: 0.0,
        priority: 0,
        rank: 0,
        allow_overlap: true,
        ignore_placement: false,
        avoid_edges: false,
        text_offset_pixels: [0.0, 0.0],
        text_padding_pixels: rustic_maps_rendering::LABEL_COLLISION_PADDING_PIXELS,
        text_pitch_alignment: rustic_maps_style::TextPitchAlignment::Viewport,
        repeat_spacing_pixels: rustic_maps_rendering::LINE_LABEL_REPEAT_SPACING_PIXELS,
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
        text_max_angle_radians:
            rustic_maps_rendering::LINE_LABEL_STRAIGHT_RUN_ANGLE_TOLERANCE_RADIANS,
        style_order: 0,
        icon: None,
        shield: None,
    };
    let label_layer = LabelLayer::place(
        &camera,
        &viewport,
        DEFAULT_FIELD_OF_VIEW_DEGREES,
        &[candidate],
        &sacramento_label_font(),
    );
    let mut pool = TileTexturePool::new(0);
    renderer
        .render_captured_frame_with_labels(&camera, &viewport, &[], &mut pool, Some(&label_layer))
        .expect("the isolated label frame always renders")
}

/// The luminance coverage each pixel reached between the clear colour and
/// the text colour, computed from the frame's own corner and brightest pixel.
fn coverage_profile(frame: &CapturedFrame) -> CoverageProfile {
    let pixel_luminance =
        |pixel: &[u8]| (f64::from(pixel[0]) + f64::from(pixel[1]) + f64::from(pixel[2])) / 3.0;
    let pixels: Vec<f64> = frame
        .rgba_bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| pixel_luminance(pixel))
        .collect();
    let width = frame.width_pixels as usize;
    let height = frame.height_pixels as usize;
    // The clear colour: every corner is background in this scene.
    let corner_pixels = [
        pixels[0],
        pixels[width - 1],
        pixels[(height - 1) * width],
        pixels[height * width - 1],
    ];
    let background_luminance = corner_pixels.iter().sum::<f64>() / corner_pixels.len() as f64;
    let text_luminance = pixels.iter().copied().fold(background_luminance, f64::max);
    assert!(
        text_luminance > background_luminance + 1.0,
        "the profile frame must contain drawn text"
    );
    let coverage_per_pixel: Vec<f64> = pixels
        .iter()
        .map(|luminance| {
            ((*luminance - background_luminance) / (text_luminance - background_luminance))
                .clamp(0.0, 1.0)
        })
        .collect();
    CoverageProfile {
        width,
        height,
        coverage_per_pixel,
    }
}

/// The per-pixel coverage of one profile frame.
struct CoverageProfile {
    width: usize,
    height: usize,
    coverage_per_pixel: Vec<f64>,
}

impl CoverageProfile {
    fn coverage_at(&self, x: usize, y: usize) -> f64 {
        self.coverage_per_pixel[y * self.width + x]
    }

    /// The widest run of interior pixels on any row, with the intermediate
    /// run lengths immediately to its left and right (the stroke's two SDF
    /// edges). Returns `None` when no pixel reaches the interior floor.
    fn widest_interior_run_edges(&self) -> Option<(usize, usize)> {
        let mut best: Option<(usize, usize, usize)> = None;
        for y in 0..self.height {
            let mut x = 0;
            while x < self.width {
                if self.coverage_at(x, y) < COVERAGE_INTERIOR_MINIMUM {
                    x += 1;
                    continue;
                }
                let run_start = x;
                while x < self.width && self.coverage_at(x, y) >= COVERAGE_INTERIOR_MINIMUM {
                    x += 1;
                }
                let run_length = x - run_start;
                if best.is_none_or(|(best_length, _, _)| run_length > best_length) {
                    best = Some((run_length, run_start, y));
                }
            }
        }
        let (_, run_start, y) = best?;
        let mut left_edge_length = 0;
        let mut left_x = run_start;
        while left_x > 0
            && self.coverage_at(left_x - 1, y) > COVERAGE_EDGE_MINIMUM
            && self.coverage_at(left_x - 1, y) < COVERAGE_EDGE_MAXIMUM
        {
            left_edge_length += 1;
            left_x -= 1;
        }
        let mut right_edge_length = 0;
        let mut right_x = run_start
            + self.coverage_per_pixel[y * self.width + run_start..]
                .iter()
                .take_while(|coverage| **coverage >= COVERAGE_INTERIOR_MINIMUM)
                .count();
        while right_x < self.width
            && self.coverage_at(right_x, y) > COVERAGE_EDGE_MINIMUM
            && self.coverage_at(right_x, y) < COVERAGE_EDGE_MAXIMUM
        {
            right_edge_length += 1;
            right_x += 1;
        }
        Some((left_edge_length, right_edge_length))
    }
}

#[test]
fn a_label_stroke_reaches_full_coverage_and_its_edge_stays_within_two_pixels() {
    let Some(mut renderer) = request_renderer_or_skip() else {
        return;
    };
    for text_size_pixels in COVERAGE_PROFILE_TEXT_SIZES_PIXELS {
        let frame = render_coverage_profile_frame(&mut renderer, text_size_pixels);
        let profile = coverage_profile(&frame);
        let interior_pixel_count = profile
            .coverage_per_pixel
            .iter()
            .filter(|coverage| **coverage >= COVERAGE_INTERIOR_MINIMUM)
            .count();
        assert!(
            interior_pixel_count > 0,
            "the {text_size_pixels}px stroke never reaches {COVERAGE_INTERIOR_MINIMUM} coverage"
        );
        let (left_edge, right_edge) = profile
            .widest_interior_run_edges()
            .expect("the profile found an interior run");
        assert!(
            left_edge <= COVERAGE_MAXIMUM_EDGE_RUN_PIXELS
                && right_edge <= COVERAGE_MAXIMUM_EDGE_RUN_PIXELS,
            "the {text_size_pixels}px stroke edge spans {left_edge}/{right_edge} pixels, \
             wider than the documented {COVERAGE_MAXIMUM_EDGE_RUN_PIXELS}-pixel band"
        );
        println!(
            "coverage profile {text_size_pixels}px: {interior_pixel_count} full pixels, edges \
             {left_edge}/{right_edge} px"
        );
    }
}

/// Builds the mid-fade scene camera: the corner shared by the recorded z15
/// tiles, at the requested level's own zoom so one tile spans 512 screen
/// pixels and the 512-pixel viewport shows a 2x2 block around it.
fn cross_fade_camera() -> MapCamera {
    let tiles_per_axis = 2f64.powi(i32::from(CROSS_FADE_REQUESTED_TILE_ZOOM_LEVEL));
    let normalized_center = [
        f64::from(CROSS_FADE_ANCHOR_TILE_X) / tiles_per_axis,
        f64::from(CROSS_FADE_ANCHOR_TILE_Y) / tiles_per_axis,
    ];
    MapCamera::new(
        normalized_world_to_coordinate(normalized_center),
        f64::from(CROSS_FADE_REQUESTED_TILE_ZOOM_LEVEL),
        GOLDEN_BEARING_DEGREES,
        TOP_DOWN_PITCH_DEGREES,
    )
}

/// Fills the pool with both the requested z15 tiles and their z14 parents,
/// and prepares every requested tile as present — the mid-fade frame half
/// way through the dissolve.
fn fill_cross_fade_pool_and_prepare(camera: &MapCamera) -> (TileTexturePool, Vec<PreparedTile>) {
    let requested_tile_keys = camera.visible_tile_keys(
        &golden_viewport(),
        DEFAULT_FIELD_OF_VIEW_DEGREES,
        CROSS_FADE_REQUESTED_TILE_ZOOM_LEVEL,
    );
    let retained_tile_keys = camera.visible_tile_keys(
        &golden_viewport(),
        DEFAULT_FIELD_OF_VIEW_DEGREES,
        CROSS_FADE_RETAINED_TILE_ZOOM_LEVEL,
    );
    let mut payload_pool = TileTexturePool::new(SCENE_POOL_BUDGET_BYTES);
    for tile_key in requested_tile_keys.iter().chain(retained_tile_keys.iter()) {
        payload_pool.insert(*tile_key, load_recorded_tile_payload(*tile_key));
    }
    let prepared_tiles = requested_tile_keys
        .into_iter()
        .map(|tile_key| PreparedTile {
            requested_tile_key: tile_key,
            readiness: TileReadiness::Present,
        })
        .collect();
    (payload_pool, prepared_tiles)
}

#[test]
fn a_mid_fade_frame_dissolves_the_retired_level_into_the_new_one() {
    let Some(mut renderer) = request_renderer_or_skip() else {
        return;
    };
    let camera = cross_fade_camera();
    let (mut payload_pool, prepared_tiles) = fill_cross_fade_pool_and_prepare(&camera);
    let captured_frame = renderer
        .render_captured_frame_layers_and_labels_with_retained_level(
            &camera,
            &golden_viewport(),
            &prepared_tiles,
            &mut payload_pool,
            &[],
            None,
            None,
            Some(RetainedTileLevel {
                tile_zoom_level: CROSS_FADE_RETAINED_TILE_ZOOM_LEVEL,
                opacity: CROSS_FADE_RETAINED_OPACITY,
            }),
        )
        .expect("the mid-fade scene always renders");
    compare_or_create_golden(
        &captured_frame,
        CROSS_FADE_GOLDEN_FILE,
        "the mid-fade cross-fade scene",
        GoldenTolerance::STANDARD,
    );
}

#[test]
fn sacramento_place_labels_scene_matches_its_golden_fixture() {
    let Some(mut renderer) = request_renderer_or_skip() else {
        return;
    };
    let camera = sacramento_camera(TOP_DOWN_PITCH_DEGREES);
    let (mut payload_pool, prepared_tiles) = fill_pool_and_prepare(&camera);
    let label_layer = sacramento_place_label_layer(&camera);
    assert!(
        !label_layer.is_empty(),
        "the Bright style's place layer draws at least one label over Sacramento"
    );
    let captured_frame = renderer
        .render_captured_frame_with_labels(
            &camera,
            &golden_viewport(),
            &prepared_tiles,
            &mut payload_pool,
            Some(&label_layer),
        )
        .expect("the place-label scene always renders");
    compare_or_create_golden(
        &captured_frame,
        PLACE_LABELS_GOLDEN_FILE,
        "the place-label scene",
        GoldenTolerance::STANDARD,
    );
}

#[test]
fn sacramento_road_names_scene_matches_its_golden_fixture() {
    let Some(mut renderer) = request_renderer_or_skip() else {
        return;
    };
    let camera = camera_at(
        ROAD_NAME_SCENE_LATITUDE_DEGREES,
        ROAD_NAME_SCENE_LONGITUDE_DEGREES,
        TOP_DOWN_PITCH_DEGREES,
    );
    let (mut payload_pool, prepared_tiles) = fill_pool_and_prepare(&camera);
    let label_layer = sacramento_road_name_label_layer(&camera);
    assert!(
        !label_layer.is_empty(),
        "the Bright style's transportation_name layers draw at least one street name"
    );
    assert!(
        label_layer
            .labels()
            .iter()
            .all(|label| label.placed_along_line),
        "the road-name scene paints only line-placed labels"
    );
    let captured_frame = renderer
        .render_captured_frame_with_labels(
            &camera,
            &golden_viewport(),
            &prepared_tiles,
            &mut payload_pool,
            Some(&label_layer),
        )
        .expect("the road-name scene always renders");
    compare_or_create_golden(
        &captured_frame,
        ROAD_NAMES_GOLDEN_FILE,
        "the road-name scene",
        GoldenTolerance::STANDARD,
    );
}

/// Builds the dense regional scene's camera over the committed vector fixture
/// tile at an explicit zoom.
fn regional_label_camera(zoom_level: f64) -> MapCamera {
    MapCamera::new(
        geospatial_types::GeographicCoordinate::from_degrees(
            REGIONAL_LABEL_CENTER_LATITUDE_DEGREES,
            REGIONAL_LABEL_CENTER_LONGITUDE_DEGREES,
        )
        .expect("the regional label anchor is a valid coordinate"),
        zoom_level,
        GOLDEN_BEARING_DEGREES,
        TOP_DOWN_PITCH_DEGREES,
    )
}

/// Places the committed fixture's labels resolved at `style_zoom_level` and
/// projected through the regional camera.
fn regional_label_layer(camera: &MapCamera, style_zoom_level: f64) -> LabelLayer {
    let candidates = sacramento_label_candidates_at_style_zoom(style_zoom_level);
    LabelLayer::place(
        camera,
        &golden_viewport(),
        DEFAULT_FIELD_OF_VIEW_DEGREES,
        &candidates,
        &sacramento_label_font(),
    )
}

/// Renders the label layer over the Bright style's own background, with no
/// raster tiles: the frame's only content is the committed fixture's text.
fn render_regional_label_frame(
    renderer: &mut HeadlessTileRenderer,
    label_layer: &LabelLayer,
    camera: &MapCamera,
) -> CapturedFrame {
    renderer.set_clear_color_rgba(BRIGHT_STYLE_BACKGROUND_RGBA);
    let mut pool = TileTexturePool::new(0);
    renderer
        .render_captured_frame_with_labels(
            camera,
            &golden_viewport(),
            &[],
            &mut pool,
            Some(label_layer),
        )
        .expect("the label-only regional frame always renders")
}

#[test]
fn sacramento_regional_labels_scene_matches_its_golden_fixture() {
    let Some(mut renderer) = request_renderer_or_skip() else {
        return;
    };
    let camera = regional_label_camera(REGIONAL_LABEL_SCENE_ZOOM_LEVEL);
    let label_layer = regional_label_layer(&camera, REGIONAL_LABEL_SCENE_ZOOM_LEVEL);
    assert!(
        !label_layer.is_empty(),
        "the Bright style's places and road names must place at the regional zoom"
    );
    // Priority order: symbol class before rank means every placed place name
    // (priority 1) precedes every road name (priority 2).
    assert!(
        label_layer
            .labels()
            .windows(2)
            .all(|labels| labels[0].priority <= labels[1].priority),
        "labels must be placed in symbol-class order: {:?}",
        label_layer
            .labels()
            .iter()
            .map(|label| (label.text.as_str(), label.priority))
            .collect::<Vec<_>>()
    );
    // Regional thinning drops the two lowest-ranked place names before the
    // collision pass runs.
    let placed_texts: Vec<&str> = label_layer
        .labels()
        .iter()
        .map(|label| label.text.as_str())
        .collect();
    for thinned_text in ["Swanston Palms", "Land Park Terrace"] {
        assert!(
            !placed_texts.contains(&thinned_text),
            "{thinned_text:?} carries a rank above the zoom-13 band and must be thinned"
        );
    }
    let captured_frame = render_regional_label_frame(&mut renderer, &label_layer, &camera);
    compare_or_create_golden(
        &captured_frame,
        REGIONAL_LABELS_GOLDEN_FILE,
        "the dense regional label scene",
        GoldenTolerance::STANDARD,
    );
}

#[test]
fn sacramento_regional_thinning_scene_matches_its_golden_fixture() {
    let Some(mut renderer) = request_renderer_or_skip() else {
        return;
    };
    let camera = regional_label_camera(REGIONAL_THINNING_SCENE_ZOOM_LEVEL);
    let label_layer = regional_label_layer(&camera, REGIONAL_THINNING_SCENE_ZOOM_LEVEL);
    // Every candidate from the z14 fixture projects into a couple of pixels at
    // zoom 8; thinning leaves only the important ranks, and the most important
    // of those takes the one spot the collision pass can give.
    assert_eq!(
        label_layer.labels().len(),
        1,
        "the wide regional frame collapses its candidate pile to one name"
    );
    assert_eq!(label_layer.labels()[0].text, "Sacramento");
    let captured_frame = render_regional_label_frame(&mut renderer, &label_layer, &camera);
    compare_or_create_golden(
        &captured_frame,
        REGIONAL_THINNING_GOLDEN_FILE,
        "the wide regional thinning scene",
        GoldenTolerance::STANDARD,
    );
}
