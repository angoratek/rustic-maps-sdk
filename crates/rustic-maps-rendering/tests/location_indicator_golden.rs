//! Golden-image harness for the location-indicator layer drawn over the
//! raster tiles and overlays.
//!
//! Three indicator scenes over the recorded Sacramento tile fixtures render
//! into RGBA buffers with the same camera, viewport, and tile setup as the
//! tile goldens in `golden_image.rs` and the overlay goldens in
//! `overlay_golden.rs`, each compared against a PNG fixture under
//! `fixtures/render/golden/`: the navigation dart (white outline, accent
//! fill, over its accuracy ring), the plain tracking dot with its accuracy
//! ring, and the tracking dot with a reported heading (which stays a dot
//! because the visual kind, not the state, chooses the form).
//!
//! Two degradation tests close the slice: a cleared (`None`) indicator must
//! render byte-identically to the two-layer pipeline on the same scene (with
//! overlays present, so the third pass's absence is what is being proven),
//! and the drawn indicator must carry the disc color into the frame's center
//! pixel band, proving the layer actually painted over the tiles (the
//! fixtures' own imagery could coincidentally match there otherwise — the
//! band check pins pass order).
//!
//! The comparison reuses the golden tolerance model of `overlay_golden.rs`
//! (mean absolute per-channel error and an outlier fraction bound), since
//! GPUs differ in sampler behavior; a missing golden fixture is written on
//! first run and verified on every run after. All tests skip gracefully when
//! no graphics adapter is available, exactly like the tile goldens.

mod common;

use common::golden::{GoldenTolerance, compare_or_create_golden};
use rustic_maps_rendering::{
    DEFAULT_FIELD_OF_VIEW_DEGREES, DEFAULT_LOCATION_DISC_RGBA, GraphicsContextSource,
    HeadlessTileRenderer, LocationIndicatorLayer, LocationIndicatorState, LocationIndicatorStyle,
    LocationIndicatorVisualKind, MapCamera, MapOverlay, PreparedTile, TileTexturePool,
    WgpuGraphicsContextSource,
};
use rustic_maps_test_support::{
    SACRAMENTO_LATITUDE_DEGREES, SACRAMENTO_LONGITUDE_DEGREES, sacramento_camera,
    square_viewport as golden_viewport,
};
use std::path::PathBuf;

/// Tile zoom at which the Sacramento scenes' tiles are scheduled and rendered.
const SCENE_TILE_ZOOM_LEVEL: f64 = 14.0;

/// Fixture of the guided-course indicator scene (the overlay-and-indicator
/// composition).
const GUIDED_COURSE_INDICATOR_GOLDEN_FILE: &str = "sacramento_zoom14_location_indicator_guided.png";
/// Fixture of the plain tracking-dot indicator scene with its accuracy ring.
const TRACKING_DOT_INDICATOR_GOLDEN_FILE: &str = "sacramento_zoom14_location_indicator_dot.png";
/// Fixture of the tracking-dot scene with a reported heading, proving the
/// tracking visual stays a dot whatever the fix's course.
const TRACKING_HEADING_INDICATOR_GOLDEN_FILE: &str =
    "sacramento_zoom14_location_indicator_dot_heading.png";

/// Directory the recorded platform tiles live in, relative to the crate
/// manifest; the `record_golden_tiles` example writes exactly these files.
const TILE_FIXTURE_DIRECTORY: &str = "fixtures/render/tiles";
/// Command that records the tiles `TILE_FIXTURE_DIRECTORY` is expected to
/// hold; embedded in the error when a tile fixture is missing.
const RECORD_TILES_COMMAND: &str =
    "cargo run -p rustic-maps-rendering --example record_golden_tiles";

/// The indicator scenes' fix: the camera's own center, so the disc anchors
/// at the viewport center and the layer's top-layer painting covers the
/// scene's midpoint.
///
/// The fix's course heading: due north, so the navigation dart points
/// up-screen under the north-up camera.
const FIX_COURSE_HEADING_DEGREES: f64 = 0.0;
/// The tracking scene's reported heading: an arbitrary non-north direction,
/// so the golden proves the tracking dot ignores the course entirely.
const TRACKING_FIX_COURSE_HEADING_DEGREES: f64 = 135.0;
/// The fix's horizontal accuracy, chosen so the ring clears the draw
/// threshold and stays well inside the screen cap at the scene zoom.
const FIX_HORIZONTAL_ACCURACY_METERS: f64 = 40.0;

/// The overlay the indicator scenes draw beneath the indicator: a short
/// route line through the fix, so the third pass paints over overlay content
/// too (the pass order the top-layer rule promises).
const ROUTE_LINE_HALF_SPAN_DEGREES: f64 = 0.0015;

/// Scene camera over Sacramento, shared by every Sacramento indicator scene.
/// Builds a coordinate from a latitude/longitude pair of one scene constant.
fn scene_coordinate(
    latitude_degrees: f64,
    longitude_degrees: f64,
) -> geospatial_types::GeographicCoordinate {
    geospatial_types::GeographicCoordinate::from_degrees(latitude_degrees, longitude_degrees)
        .expect("the scene coordinates are valid")
}

/// Builds the scene viewport.
/// The indicator scenes' fix state: the camera's center, with the course and
/// accuracy the scene constants carry.
fn fix_state(
    course_heading_degrees: Option<f64>,
    horizontal_accuracy_meters: Option<f64>,
) -> LocationIndicatorState {
    LocationIndicatorState {
        coordinate: scene_coordinate(SACRAMENTO_LATITUDE_DEGREES, SACRAMENTO_LONGITUDE_DEGREES),
        course_heading_degrees,
        horizontal_accuracy_meters,
    }
}

/// A route overlay through the fix spanning the scene's half span.
fn through_the_fix_route_overlay() -> MapOverlay {
    MapOverlay::route_polyline(vec![
        scene_coordinate(
            SACRAMENTO_LATITUDE_DEGREES,
            SACRAMENTO_LONGITUDE_DEGREES - ROUTE_LINE_HALF_SPAN_DEGREES,
        ),
        scene_coordinate(
            SACRAMENTO_LATITUDE_DEGREES,
            SACRAMENTO_LONGITUDE_DEGREES + ROUTE_LINE_HALF_SPAN_DEGREES,
        ),
    ])
}

/// Renders a Sacramento scene: the recorded tiles, the route overlay, and the
/// given indicator layer.
fn render_indicator_scene(
    renderer: &mut HeadlessTileRenderer,
    camera: &MapCamera,
    location_indicator_layer: Option<&LocationIndicatorLayer>,
) -> rustic_maps_rendering::CapturedFrame {
    let golden_viewport = golden_viewport();
    let scene_tile_keys = camera.visible_tile_keys(
        &golden_viewport,
        DEFAULT_FIELD_OF_VIEW_DEGREES,
        SCENE_TILE_ZOOM_LEVEL as u8,
    );
    let mut payload_pool = TileTexturePool::new(64 << 20);
    for tile_key in &scene_tile_keys {
        let tile_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(TILE_FIXTURE_DIRECTORY)
            .join(format!(
                "{}_{}_{}.png",
                tile_key.zoom_level, tile_key.tile_x, tile_key.tile_y
            ));
        assert!(
            tile_path.exists(),
            "recorded tile fixture {tile_path:?} is missing; run `{RECORD_TILES_COMMAND}` to \
             record it"
        );
        payload_pool.insert(
            *tile_key,
            std::fs::read(&tile_path).expect("the recorded tile fixture is readable"),
        );
    }
    let prepared_tiles: Vec<PreparedTile> = scene_tile_keys
        .into_iter()
        .map(|tile_key| PreparedTile {
            requested_tile_key: tile_key,
            readiness: rustic_maps_rendering::TileReadiness::Present,
        })
        .collect();
    let overlays = [through_the_fix_route_overlay()];
    renderer
        .render_captured_frame_layers(
            camera,
            &golden_viewport,
            &prepared_tiles,
            &mut payload_pool,
            &overlays,
            location_indicator_layer,
        )
        .expect("the golden scene always renders")
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
fn the_guided_course_indicator_matches_its_golden_fixture() {
    let Some(mut renderer) = request_renderer_or_skip() else {
        return;
    };
    let indicator_layer = LocationIndicatorLayer::new(
        fix_state(
            Some(FIX_COURSE_HEADING_DEGREES),
            Some(FIX_HORIZONTAL_ACCURACY_METERS),
        ),
        LocationIndicatorStyle::with_defaults(LocationIndicatorVisualKind::GuidedCourseArrow),
    );
    let captured_frame =
        render_indicator_scene(&mut renderer, &sacramento_camera(), Some(&indicator_layer));
    compare_or_create_golden(
        &captured_frame,
        GUIDED_COURSE_INDICATOR_GOLDEN_FILE,
        "the guided-course indicator scene",
        GoldenTolerance::STANDARD,
    );
}

#[test]
fn the_tracking_dot_indicator_matches_its_golden_fixture() {
    let Some(mut renderer) = request_renderer_or_skip() else {
        return;
    };
    let indicator_layer = LocationIndicatorLayer::new(
        fix_state(None, Some(FIX_HORIZONTAL_ACCURACY_METERS)),
        LocationIndicatorStyle::with_defaults(LocationIndicatorVisualKind::TrackingDot),
    );
    let captured_frame =
        render_indicator_scene(&mut renderer, &sacramento_camera(), Some(&indicator_layer));
    compare_or_create_golden(
        &captured_frame,
        TRACKING_DOT_INDICATOR_GOLDEN_FILE,
        "the tracking-dot indicator scene",
        GoldenTolerance::STANDARD,
    );
}

#[test]
fn the_tracking_dot_with_a_heading_matches_its_golden_fixture() {
    let Some(mut renderer) = request_renderer_or_skip() else {
        return;
    };
    let indicator_layer = LocationIndicatorLayer::new(
        fix_state(
            Some(TRACKING_FIX_COURSE_HEADING_DEGREES),
            Some(FIX_HORIZONTAL_ACCURACY_METERS),
        ),
        LocationIndicatorStyle::with_defaults(LocationIndicatorVisualKind::TrackingDot),
    );
    let captured_frame =
        render_indicator_scene(&mut renderer, &sacramento_camera(), Some(&indicator_layer));
    compare_or_create_golden(
        &captured_frame,
        TRACKING_HEADING_INDICATOR_GOLDEN_FILE,
        "the tracking-dot-with-heading indicator scene",
        GoldenTolerance::STANDARD,
    );
}

#[test]
fn a_cleared_indicator_leaves_the_two_layer_frame_byte_identical() {
    let Some(mut renderer) = request_renderer_or_skip() else {
        return;
    };
    let camera = sacramento_camera();
    let cleared_indicator_frame = render_indicator_scene(&mut renderer, &camera, None);
    let two_layer_frame = {
        let golden_viewport = golden_viewport();
        let scene_tile_keys = camera.visible_tile_keys(
            &golden_viewport,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
            SCENE_TILE_ZOOM_LEVEL as u8,
        );
        let mut payload_pool = TileTexturePool::new(64 << 20);
        for tile_key in &scene_tile_keys {
            let tile_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join(TILE_FIXTURE_DIRECTORY)
                .join(format!(
                    "{}_{}_{}.png",
                    tile_key.zoom_level, tile_key.tile_x, tile_key.tile_y
                ));
            assert!(
                tile_path.exists(),
                "recorded tile fixture {tile_path:?} is missing; run `{RECORD_TILES_COMMAND}` to \
                 record it"
            );
            payload_pool.insert(
                *tile_key,
                std::fs::read(&tile_path).expect("the recorded tile fixture is readable"),
            );
        }
        let prepared_tiles: Vec<PreparedTile> = scene_tile_keys
            .into_iter()
            .map(|tile_key| PreparedTile {
                requested_tile_key: tile_key,
                readiness: rustic_maps_rendering::TileReadiness::Present,
            })
            .collect();
        renderer
            .render_captured_frame_with_overlays(
                &camera,
                &golden_viewport,
                &prepared_tiles,
                &mut payload_pool,
                &[through_the_fix_route_overlay()],
            )
            .expect("the golden scene always renders")
    };
    assert_eq!(
        cleared_indicator_frame.rgba_bytes, two_layer_frame.rgba_bytes,
        "a cleared indicator must leave the two-layer pipeline's frame untouched"
    );
}

#[test]
fn the_indicator_layer_paints_the_disc_color_over_the_scene_center() {
    // The pass-order proof: the fix anchors at the viewport center, so the
    // center pixel band must carry the disc color (the opaque blue), not the
    // tile or overlay content beneath.
    let Some(mut renderer) = request_renderer_or_skip() else {
        return;
    };
    let indicator_layer = LocationIndicatorLayer::new(
        fix_state(None, None),
        LocationIndicatorStyle::with_defaults(LocationIndicatorVisualKind::TrackingDot),
    );
    let captured_frame =
        render_indicator_scene(&mut renderer, &sacramento_camera(), Some(&indicator_layer));
    let center_pixel_offset = (captured_frame.height_pixels / 2 * captured_frame.width_pixels
        + captured_frame.width_pixels / 2) as usize;
    let center_pixel =
        &captured_frame.rgba_bytes[center_pixel_offset * 4..center_pixel_offset * 4 + 4];
    for (channel, expected) in center_pixel[..3].iter().zip(DEFAULT_LOCATION_DISC_RGBA) {
        let expected_byte = (expected * f32::from(u8::MAX)) as u8;
        assert!(
            i16::from(*channel).abs_diff(i16::from(expected_byte)) <= 8,
            "the frame's center pixel must carry the disc color: {center_pixel:?}"
        );
    }
}
