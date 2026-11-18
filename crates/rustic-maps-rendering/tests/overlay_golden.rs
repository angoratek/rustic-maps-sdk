//! Golden-image harness for the overlay layer drawn over the raster tiles.
//!
//! Four overlay scenes over the recorded Sacramento tile fixtures (route
//! line, polygon fill, point markers, traffic flow) render into RGBA buffers with
//! the same camera, viewport, and tile setup as the tile goldens in
//! `golden_image.rs`, each compared against a PNG fixture under
//! `fixtures/render/golden/`. One synthetic scene draws a route across the
//! antimeridian at zoom 0 over generated gradient tiles — the recorded
//! fixtures sit far from the antimeridian, so this documented synthetic span
//! exercises the world-copy wrap the tile pipeline already renders.
//!
//! Two degradation tests close the slice: an empty overlay list must render
//! byte-identically to the pre-overlay pipeline (against the existing
//! top-down golden fixture), and an overlay whose coordinates project wholly
//! off the viewport must not corrupt the frame (byte-identical to the same
//! scene's route overlay without it).
//!
//! The comparison reuses the golden tolerance model of `golden_image.rs`
//! (mean absolute per-channel error and an outlier fraction bound), since
//! GPUs differ in sampler behavior; a missing golden fixture is written on
//! first run and verified on every run after. All tests skip gracefully when
//! no graphics adapter is available, exactly like the tile goldens.

use rustic_maps_rendering::{
    CapturedFrame, DEFAULT_ROUTE_LINE_RGBA, FilledPolygonOverlay, GraphicsContextSource,
    HeadlessTileRenderer, MapCamera, MapOverlay, PointMarkersOverlay, PolylineStroke, PreparedTile,
    RoutePolylineOverlay, TileKey, TileReadiness, TileTexturePool, ViewportPixels,
    WgpuGraphicsContextSource, overlay_triangle_vertices,
};
use rustic_maps_style::{LineCap, LineJoin};
use std::path::PathBuf;

/// Frame edge in pixels for every golden scene; small enough for a fast test,
/// and a whole power of two so tile spans map cleanly.
const GOLDEN_VIEWPORT_EDGE_PIXELS: f64 = 512.0;

/// Sacramento (California State Capitol) center, the fixed camera anchor of
/// the Sacramento scenes.
const SACRAMENTO_LATITUDE_DEGREES: f64 = 38.5766;
/// Sacramento center longitude.
const SACRAMENTO_LONGITUDE_DEGREES: f64 = -121.4944;
/// Tile zoom at which the Sacramento scenes' tiles are scheduled and rendered.
const SCENE_TILE_ZOOM_LEVEL: u8 = 14;
/// Bearing of the Sacramento scene camera: north up.
const GOLDEN_BEARING_DEGREES: f64 = 0.0;
/// Pitch of the Sacramento scene camera: straight down.
const TOP_DOWN_PITCH_DEGREES: f64 = 0.0;

/// Zoom of the antimeridian scene: at zoom 0 a 512-pixel viewport spans the
/// whole world, so the world-copy wrap reaches around the screen edges.
const ANTIMERIDIAN_ZOOM_LEVEL: f64 = 0.0;

/// Directory of the golden fixtures, relative to the crate manifest.
const GOLDEN_DIRECTORY: &str = "fixtures/render/golden";
/// Fixture of the route-line scene.
const ROUTE_LINE_GOLDEN_FILE: &str = "sacramento_zoom14_route_line.png";
/// Fixture of the polygon-fill scene.
const POLYGON_FILL_GOLDEN_FILE: &str = "sacramento_zoom14_polygon_fill.png";
/// Fixture of the point-markers scene.
const POINT_MARKERS_GOLDEN_FILE: &str = "sacramento_zoom14_markers.png";
/// Fixture of the point-pins scene.
const POINT_PINS_GOLDEN_FILE: &str = "sacramento_zoom14_pins.png";
/// Fixture of the traffic-flow scene.
const TRAFFIC_FLOW_GOLDEN_FILE: &str = "sacramento_zoom14_traffic_flow.png";
/// Fixture of the antimeridian route scene.
const ANTIMERIDIAN_ROUTE_GOLDEN_FILE: &str = "world_zoom0_route_antimeridian.png";
/// Existing fixture of the tiles-only top-down scene, reused to prove the
/// empty-overlay list changed nothing.
const EXISTING_TOP_DOWN_GOLDEN_FILE: &str = "sacramento_zoom14_top_down.png";

/// Directory the recorded platform tiles live in, relative to the crate
/// manifest; the `record_golden_tiles` example writes exactly these files.
const TILE_FIXTURE_DIRECTORY: &str = "fixtures/render/tiles";
/// Command that records the tiles `TILE_FIXTURE_DIRECTORY` is expected to
/// hold; embedded in the error when a tile fixture is missing.
const RECORD_TILES_COMMAND: &str =
    "cargo run -p rustic-maps-rendering --example record_golden_tiles";

/// The mean absolute per-channel error (0..255 scale) tolerated between the
/// rendered frame and its golden fixture.
const MEAN_ABSOLUTE_ERROR_TOLERANCE: f64 = 2.0;
/// Per-pixel absolute channel error (0..255 scale) above which a pixel counts
/// as an outlier.
const OUTLIER_CHANNEL_ERROR: f64 = 24.0;
/// Fraction of outlier pixels tolerated; sampler differences gather along
/// tile edges and steep gradients, never across the whole frame.
const OUTLIER_PIXEL_FRACTION_LIMIT: f64 = 0.02;

/// Route scene waypoints: a three-point polyline entering bottom-left, bent
/// through the viewport center, and leaving upper-right.
const ROUTE_LATITUDES_DEGREES: [f64; 3] = [38.573483, 38.5766, 38.580987];
/// Route scene waypoints' longitudes (see `ROUTE_LATITUDES_DEGREES`).
const ROUTE_LONGITUDES_DEGREES: [f64; 3] = [-121.5023, -121.4944, -121.4858];

/// Polygon scene rectangle ring (south-west, south-east, north-east,
/// north-west), spanning roughly the center third of the viewport.
const POLYGON_RING_LATITUDES_DEGREES: [f64; 4] = [38.573483, 38.573483, 38.579717, 38.579717];
/// Polygon scene rectangle ring's longitudes (see
/// `POLYGON_RING_LATITUDES_DEGREES`).
const POLYGON_RING_LONGITUDES_DEGREES: [f64; 4] = [-121.4987, -121.4901, -121.4901, -121.4987];

/// Marker scene anchor positions, spread diagonally across the viewport.
const MARKER_LATITUDES_DEGREES: [f64; 3] = [38.574984, 38.5766, 38.578216];
/// Marker scene anchor longitudes (see `MARKER_LATITUDES_DEGREES`).
const MARKER_LONGITUDES_DEGREES: [f64; 3] = [-121.4971, -121.4944, -121.4917];

/// Fully off-screen route coordinates north of the Sacramento viewport; the
/// robustness test appends these and expects zero frame damage.
const OFFSCREEN_ROUTE_LATITUDES_DEGREES: [f64; 2] = [38.614815, 38.617124];
/// Off-screen route longitudes (see `OFFSCREEN_ROUTE_LATITUDES_DEGREES`).
const OFFSCREEN_ROUTE_LONGITUDES_DEGREES: [f64; 2] = [-121.4961, -121.4941];

/// Antimeridian scene route latitudes: one meridian-parallel line north of
/// the equator, spanning most of the world.
const ANTIMERIDIAN_ROUTE_LATITUDES_DEGREES: [f64; 3] = [20.0, 20.0, 20.0];
/// Antimeridian scene route longitudes: eastward past the seam (180 degrees
/// wraps to -180), so the line leaves one screen edge and continues at the
/// other through the wrapped world copy.
const ANTIMERIDIAN_ROUTE_LONGITUDES_DEGREES: [f64; 3] = [100.0, 170.0, -160.0];

/// Scene camera over Sacramento, shared by every Sacramento overlay scene.
fn sacramento_camera() -> MapCamera {
    MapCamera::new(
        geospatial_types::GeographicCoordinate::from_degrees(
            SACRAMENTO_LATITUDE_DEGREES,
            SACRAMENTO_LONGITUDE_DEGREES,
        )
        .expect("the golden scene anchor is a valid coordinate"),
        f64::from(SCENE_TILE_ZOOM_LEVEL),
        GOLDEN_BEARING_DEGREES,
        TOP_DOWN_PITCH_DEGREES,
    )
}

/// Scene camera over the antimeridian at zoom zero.
fn antimeridian_camera() -> MapCamera {
    MapCamera::new(
        geospatial_types::GeographicCoordinate::from_degrees(0.0, 0.0)
            .expect("the antimeridian scene anchor is a valid coordinate"),
        ANTIMERIDIAN_ZOOM_LEVEL,
        GOLDEN_BEARING_DEGREES,
        TOP_DOWN_PITCH_DEGREES,
    )
}

/// Builds the scene viewport.
fn golden_viewport() -> ViewportPixels {
    ViewportPixels::new(GOLDEN_VIEWPORT_EDGE_PIXELS, GOLDEN_VIEWPORT_EDGE_PIXELS)
        .expect("the golden viewport is a valid viewport")
}

/// Builds a coordinate from a latitude/longitude pair of one scene constant.
fn scene_coordinate(
    latitude_degrees: f64,
    longitude_degrees: f64,
) -> geospatial_types::GeographicCoordinate {
    geospatial_types::GeographicCoordinate::from_degrees(latitude_degrees, longitude_degrees)
        .expect("the scene coordinates are valid")
}

/// Returns the file a recorded tile lives in; see `golden_image.rs` for the
/// recording workflow the path mirrors.
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
/// command — the tests never synthesize a stand-in for real imagery and
/// never fetch.
fn load_recorded_tile_payload(tile_key: TileKey) -> Vec<u8> {
    let tile_path = recorded_tile_path(tile_key);
    if !tile_path.exists() {
        panic!(
            "recorded tile fixture {:?} is missing; run `{RECORD_TILES_COMMAND}` to record it",
            tile_path
        );
    }
    std::fs::read(&tile_path).expect("the recorded tile fixture is readable")
}

/// Fills the pool with every scene tile's recorded payload plus the shared
/// parent for the fallback draw, and marks every tile ready (the last one via
/// its parent), exactly as the tile goldens prepare their scenes.
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

    let mut payload_pool = TileTexturePool::new(64 << 20);
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

/// Edge length in pixels of the synthetic tile payloads the antimeridian
/// scene draws; any decodable size works, since the sampler draws it at the
/// quad's own resolution.
const SYNTHETIC_TILE_EDGE_PIXELS: u32 = 64;

/// Generates a deterministic valid PNG payload for a tile: a distinct
/// axis-aligned gradient per tile, enough to leave a visible trace behind the
/// overlay content. The antimeridian scene has no recorded real imagery by
/// design (see the module notes), so its tiles are documented synthesis.
fn synthetic_tile_payload(tile_key: TileKey) -> Vec<u8> {
    let tile_image = image::RgbaImage::from_fn(
        SYNTHETIC_TILE_EDGE_PIXELS,
        SYNTHETIC_TILE_EDGE_PIXELS,
        |pixel_x, pixel_y| {
            let east_fraction = pixel_x % u32::from(u8::MAX);
            let south_fraction = pixel_y % u32::from(u8::MAX);
            image::Rgba([
                east_fraction as u8,
                south_fraction as u8,
                (tile_key.tile_x + tile_key.tile_y) as u8,
                u8::MAX,
            ])
        },
    );
    let mut encoded_bytes = Vec::new();
    tile_image
        .write_to(
            &mut std::io::Cursor::new(&mut encoded_bytes),
            image::ImageFormat::Png,
        )
        .expect("the generated tile payload always encodes");
    encoded_bytes
}

/// Prepares the antimeridian scene: the zoom-0 world root tile with a
/// synthetic payload, marked present.
fn fill_pool_and_prepare_antimeridian() -> (TileTexturePool, Vec<PreparedTile>) {
    let camera = antimeridian_camera();
    let scene_tile_keys = camera.visible_tile_keys(
        &golden_viewport(),
        rustic_maps_rendering::DEFAULT_FIELD_OF_VIEW_DEGREES,
        camera.tile_zoom_level(),
    );
    let mut payload_pool = TileTexturePool::new(1 << 20);
    for tile_key in &scene_tile_keys {
        payload_pool.insert(*tile_key, synthetic_tile_payload(*tile_key));
    }
    let prepared_tiles = scene_tile_keys
        .into_iter()
        .map(|tile_key| PreparedTile {
            requested_tile_key: tile_key,
            readiness: TileReadiness::Present,
        })
        .collect();
    (payload_pool, prepared_tiles)
}

/// Renders a Sacramento scene with the given overlays and its recorded tiles.
fn render_sacramento_scene(
    renderer: &mut HeadlessTileRenderer,
    overlays: &[MapOverlay],
) -> CapturedFrame {
    let camera = sacramento_camera();
    let (mut payload_pool, prepared_tiles) = fill_pool_and_prepare(&camera);
    renderer
        .render_captured_frame_with_overlays(
            &camera,
            &golden_viewport(),
            &prepared_tiles,
            &mut payload_pool,
            overlays,
        )
        .expect("the golden scene always renders")
}

/// Renders the antimeridian scene with the given overlays and its synthetic
/// z0 tile.
fn render_antimeridian_scene(
    renderer: &mut HeadlessTileRenderer,
    overlays: &[MapOverlay],
) -> CapturedFrame {
    let camera = antimeridian_camera();
    let (mut payload_pool, prepared_tiles) = fill_pool_and_prepare_antimeridian();
    renderer
        .render_captured_frame_with_overlays(
            &camera,
            &golden_viewport(),
            &prepared_tiles,
            &mut payload_pool,
            overlays,
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

/// Compares a captured frame against its golden fixture, writing the fixture
/// on first run and asserting the tolerance on every run after.
/** Whether this run may write a missing golden (a deliberate regeneration). */
fn golden_regeneration_is_requested() -> bool {
    std::env::var("UPDATE_GOLDENS").is_ok_and(|value| value == "1")
}

fn compare_or_create_golden(
    captured_frame: &CapturedFrame,
    golden_file_name: &str,
    scene_description: &str,
) {
    let golden_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(GOLDEN_DIRECTORY)
        .join(golden_file_name);
    if !golden_path.exists() {
        // A missing golden is a failure, not a quiet regeneration: a deleted
        // or never-committed fixture must not pass. Regenerating one is a
        // deliberate act - `UPDATE_GOLDENS=1 cargo test` - and the operator
        // then reviews and commits the new image.
        if !golden_regeneration_is_requested() {
            panic!(
                "golden fixture is missing at {golden_path:?}; regenerate deliberately with \
                 UPDATE_GOLDENS=1 and commit the result"
            );
        }
        std::fs::create_dir_all(
            golden_path
                .parent()
                .expect("the fixture directory has a parent"),
        )
        .expect("the golden fixture directory is writable");
        image::save_buffer(
            &golden_path,
            &captured_frame.rgba_bytes,
            captured_frame.width_pixels,
            captured_frame.height_pixels,
            image::ColorType::Rgba8,
        )
        .expect("the golden fixture is written deterministically on first run");
        println!(
            "golden fixture for {scene_description} was missing; wrote a deterministic reference \
             at {golden_path:?} - run the test again to verify it"
        );
        return;
    }

    let golden_image = image::open(&golden_path)
        .unwrap_or_else(|error| panic!("golden fixture {golden_path:?} is unreadable: {error}"))
        .to_rgba8();
    assert_eq!(
        (golden_image.width(), golden_image.height()),
        (captured_frame.width_pixels, captured_frame.height_pixels),
        "golden fixture {golden_path:?} has a different size than the rendered frame"
    );
    let golden_bytes = golden_image.as_raw();
    let pixel_count = captured_frame.width_pixels * captured_frame.height_pixels;
    let mut absolute_error_sum: f64 = 0.0;
    let mut outlier_pixel_count: u64 = 0;
    for (captured_pixel, golden_pixel) in captured_frame
        .rgba_bytes
        .as_chunks::<4>()
        .0
        .iter()
        .zip(golden_bytes.as_chunks::<4>().0)
    {
        let mut pixel_maximum_error: f64 = 0.0;
        for channel in 0..4 {
            let channel_error = f64::from(captured_pixel[channel].abs_diff(golden_pixel[channel]));
            absolute_error_sum += channel_error;
            pixel_maximum_error = pixel_maximum_error.max(channel_error);
        }
        if pixel_maximum_error > OUTLIER_CHANNEL_ERROR {
            outlier_pixel_count += 1;
        }
    }
    let mean_absolute_error = absolute_error_sum / (f64::from(pixel_count) * 4.0);
    let outlier_pixel_fraction = outlier_pixel_count as f64 / f64::from(pixel_count);
    println!(
        "{scene_description}: mean absolute channel error {mean_absolute_error:.4} \
         (limit {MEAN_ABSOLUTE_ERROR_TOLERANCE}), outlier fraction \
         {outlier_pixel_fraction:.5} (limit {OUTLIER_PIXEL_FRACTION_LIMIT})"
    );
    assert!(
        mean_absolute_error <= MEAN_ABSOLUTE_ERROR_TOLERANCE,
        "the rendered {scene_description} frame drifted from its golden fixture: \
         mean absolute channel error {mean_absolute_error}"
    );
    assert!(
        outlier_pixel_fraction <= OUTLIER_PIXEL_FRACTION_LIMIT,
        "the rendered {scene_description} frame drifted from its golden fixture: \
         outlier fraction {outlier_pixel_fraction} exceeds the limit"
    );
}

/// The antimeridian route scenes' overlays.
fn antimeridian_route_overlays() -> Vec<MapOverlay> {
    vec![MapOverlay::route_polyline(
        ANTIMERIDIAN_ROUTE_LATITUDES_DEGREES
            .iter()
            .zip(ANTIMERIDIAN_ROUTE_LONGITUDES_DEGREES)
            .map(|(latitude_degrees, longitude_degrees)| {
                scene_coordinate(*latitude_degrees, longitude_degrees)
            })
            .collect(),
    )]
}

#[test]
fn route_line_overlay_matches_its_golden_fixture() {
    let Some(mut renderer) = request_renderer_or_skip() else {
        return;
    };
    let route_coordinates: Vec<geospatial_types::GeographicCoordinate> = ROUTE_LATITUDES_DEGREES
        .iter()
        .zip(ROUTE_LONGITUDES_DEGREES)
        .map(|(latitude_degrees, longitude_degrees)| {
            scene_coordinate(*latitude_degrees, longitude_degrees)
        })
        .collect();
    let captured_frame = render_sacramento_scene(
        &mut renderer,
        &[MapOverlay::route_polyline(route_coordinates)],
    );
    compare_or_create_golden(
        &captured_frame,
        ROUTE_LINE_GOLDEN_FILE,
        "the route-line scene",
    );
}

#[test]
fn polygon_fill_overlay_matches_its_golden_fixture() {
    let Some(mut renderer) = request_renderer_or_skip() else {
        return;
    };
    let ring_coordinates: Vec<geospatial_types::GeographicCoordinate> =
        POLYGON_RING_LATITUDES_DEGREES
            .iter()
            .zip(POLYGON_RING_LONGITUDES_DEGREES)
            .map(|(latitude_degrees, longitude_degrees)| {
                scene_coordinate(*latitude_degrees, longitude_degrees)
            })
            .collect();
    let captured_frame = render_sacramento_scene(
        &mut renderer,
        &[MapOverlay::filled_polygon(ring_coordinates)],
    );
    compare_or_create_golden(
        &captured_frame,
        POLYGON_FILL_GOLDEN_FILE,
        "the polygon-fill scene",
    );
}

#[test]
fn point_markers_overlay_matches_its_golden_fixture() {
    let Some(mut renderer) = request_renderer_or_skip() else {
        return;
    };
    let marker_positions: Vec<geospatial_types::GeographicCoordinate> = MARKER_LATITUDES_DEGREES
        .iter()
        .zip(MARKER_LONGITUDES_DEGREES)
        .map(|(latitude_degrees, longitude_degrees)| {
            scene_coordinate(*latitude_degrees, longitude_degrees)
        })
        .collect();
    let captured_frame = render_sacramento_scene(
        &mut renderer,
        &[MapOverlay::point_markers(marker_positions)],
    );
    compare_or_create_golden(
        &captured_frame,
        POINT_MARKERS_GOLDEN_FILE,
        "the point-markers scene",
    );
}

#[test]
fn point_pins_overlay_matches_its_golden_fixture() {
    let Some(mut renderer) = request_renderer_or_skip() else {
        return;
    };
    let pin_positions: Vec<geospatial_types::GeographicCoordinate> = MARKER_LATITUDES_DEGREES
        .iter()
        .zip(MARKER_LONGITUDES_DEGREES)
        .map(|(latitude_degrees, longitude_degrees)| {
            scene_coordinate(*latitude_degrees, longitude_degrees)
        })
        .collect();
    let captured_frame =
        render_sacramento_scene(&mut renderer, &[MapOverlay::point_pins(pin_positions)]);
    compare_or_create_golden(
        &captured_frame,
        POINT_PINS_GOLDEN_FILE,
        "the point-pins scene",
    );
}

#[test]
fn traffic_flow_overlay_matches_its_golden_fixture() {
    let Some(mut renderer) = request_renderer_or_skip() else {
        return;
    };
    let captured_frame = render_sacramento_scene(
        &mut renderer,
        &[MapOverlay::traffic_flow(traffic_scene_flows())],
    );
    compare_or_create_golden(
        &captured_frame,
        TRAFFIC_FLOW_GOLDEN_FILE,
        "the traffic-flow scene",
    );
}

/// The traffic scene's flow observations: one congested corridor, one slow,
/// one free-flowing, and one blocked — each congestion level paints its named
/// color.
fn traffic_scene_flows() -> Vec<geospatial_types::TrafficFlowData> {
    use geospatial_types::TrafficFlowCongestionLevel;
    let flow_shape = |latitudes: [f64; 2], longitudes: [f64; 2]| {
        vec![
            scene_coordinate(latitudes[0], longitudes[0]),
            scene_coordinate(latitudes[1], longitudes[1]),
        ]
    };
    vec![
        geospatial_types::TrafficFlowData::from_congestion_level(
            flow_shape([38.579024; 2], [-121.5021, -121.4871]),
            TrafficFlowCongestionLevel::FreeFlow,
        ),
        geospatial_types::TrafficFlowData::from_congestion_level(
            flow_shape([38.574984; 2], [-121.5016, -121.4876]),
            TrafficFlowCongestionLevel::Slow,
        ),
        geospatial_types::TrafficFlowData::from_congestion_level(
            flow_shape([38.573252, 38.580179], [-121.4951; 2]),
            TrafficFlowCongestionLevel::Congested,
        ),
        geospatial_types::TrafficFlowData::from_congestion_level(
            flow_shape([38.573829, 38.579602], [-121.4911; 2]),
            TrafficFlowCongestionLevel::Blocked,
        ),
    ]
}

#[test]
fn an_empty_overlay_list_renders_the_pre_overlay_frame_byte_identically() {
    let Some(mut renderer) = request_renderer_or_skip() else {
        return;
    };
    let overlay_empty_frame = render_sacramento_scene(&mut renderer, &[]);
    let legacy_frame = {
        let camera = sacramento_camera();
        let (mut payload_pool, prepared_tiles) = fill_pool_and_prepare(&camera);
        renderer
            .render_captured_frame(
                &camera,
                &golden_viewport(),
                &prepared_tiles,
                &mut payload_pool,
            )
            .expect("the golden scene always renders")
    };
    assert_eq!(
        overlay_empty_frame.rgba_bytes, legacy_frame.rgba_bytes,
        "the overlay pipeline with an empty list must not touch the frame"
    );
    // Against today's fixtures: the empty-overlay frame stays exactly the
    // recorded top-down reference.
    compare_or_create_golden(
        &overlay_empty_frame,
        EXISTING_TOP_DOWN_GOLDEN_FILE,
        "the empty-overlay top-down scene",
    );
}

#[test]
fn an_offscreen_overlay_coordinate_does_not_corrupt_the_frame() {
    let Some(mut renderer) = request_renderer_or_skip() else {
        return;
    };
    let route_coordinates: Vec<geospatial_types::GeographicCoordinate> = ROUTE_LATITUDES_DEGREES
        .iter()
        .zip(ROUTE_LONGITUDES_DEGREES)
        .map(|(latitude_degrees, longitude_degrees)| {
            scene_coordinate(*latitude_degrees, longitude_degrees)
        })
        .collect();
    let offscreen_coordinates: Vec<geospatial_types::GeographicCoordinate> =
        OFFSCREEN_ROUTE_LATITUDES_DEGREES
            .iter()
            .zip(OFFSCREEN_ROUTE_LONGITUDES_DEGREES)
            .map(|(latitude_degrees, longitude_degrees)| {
                scene_coordinate(*latitude_degrees, longitude_degrees)
            })
            .collect();
    // Precondition: both appended points project outside the viewport, above
    // its top edge.
    let viewport = golden_viewport();
    for coordinate in &offscreen_coordinates {
        let screen_position = sacramento_camera()
            .project_to_screen(
                coordinate,
                &viewport,
                rustic_maps_rendering::DEFAULT_FIELD_OF_VIEW_DEGREES,
            )
            .expect("an off-screen ground point still projects");
        assert!(
            screen_position[1] < -10.0,
            "the appended point {coordinate:?} must project above the viewport, at \
             {screen_position:?}"
        );
    }
    let baseline_frame = render_sacramento_scene(
        &mut renderer,
        &[MapOverlay::route_polyline(route_coordinates.clone())],
    );
    let with_offscreen_frame = render_sacramento_scene(
        &mut renderer,
        &[
            MapOverlay::route_polyline(route_coordinates),
            MapOverlay::route_polyline(offscreen_coordinates),
        ],
    );
    assert_eq!(
        baseline_frame.rgba_bytes, with_offscreen_frame.rgba_bytes,
        "an overlay wholly outside the viewport must leave the frame untouched"
    );
}

#[test]
fn a_route_crossing_the_antimeridian_matches_its_golden_fixture() {
    let Some(mut renderer) = request_renderer_or_skip() else {
        return;
    };
    // Precondition of the seam exercise: the route's middle segment must
    // split against a world-copy window — the unwrapped east end runs past
    // the camera's own copy.
    let camera = antimeridian_camera();
    let viewport = golden_viewport();
    let route_triangulation = overlay_triangle_vertices(
        &camera,
        &viewport,
        rustic_maps_rendering::DEFAULT_FIELD_OF_VIEW_DEGREES,
        &antimeridian_route_overlays(),
    );
    assert!(
        route_triangulation.len() >= 18,
        "the seam-crossing route must produce the split fragments plus the \
         camera-copy segments: {} vertices",
        route_triangulation.len()
    );
    let captured_frame = render_antimeridian_scene(&mut renderer, &antimeridian_route_overlays());
    compare_or_create_golden(
        &captured_frame,
        ANTIMERIDIAN_ROUTE_GOLDEN_FILE,
        "the antimeridian route scene",
    );
    // The wrapped world copy carries the route's eastward continuation: the
    // line must touch both screen edges' columns, where the seam's two world
    // copies meet.
    let route_color_channels = route_color_bytes();
    let seam_row_band: std::ops::Range<u32> = 200..260;
    let left_edge_columns = 0..8;
    let right_edge_columns = (captured_frame.width_pixels - 8)..captured_frame.width_pixels;
    for (edge_description, columns) in [
        ("left", left_edge_columns.clone()),
        ("right", right_edge_columns.clone()),
    ] {
        let seam_pixels = captured_frame
            .rgba_bytes
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .filter(|(pixel_index, _)| {
                let pixel_x = *pixel_index as u32 % captured_frame.width_pixels;
                let pixel_y = *pixel_index as u32 / captured_frame.width_pixels;
                columns.contains(&pixel_x) && seam_row_band.contains(&pixel_y)
            })
            .count();
        let route_colored_seam_pixels = captured_frame
            .rgba_bytes
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .filter(|(pixel_index, pixel)| {
                let pixel_x = *pixel_index as u32 % captured_frame.width_pixels;
                let pixel_y = *pixel_index as u32 / captured_frame.width_pixels;
                columns.contains(&pixel_x)
                    && seam_row_band.contains(&pixel_y)
                    && pixel[..3]
                        .iter()
                        .zip(route_color_channels)
                        .all(|(channel, expected)| {
                            i16::from(*channel).abs_diff(i16::from(expected)) <= 8
                        })
            })
            .count();
        assert!(
            seam_pixels > 0 && route_colored_seam_pixels > 0,
            "the route must draw into the {edge_description} screen edge columns where \
             the world copies wrap: {route_colored_seam_pixels} route-colored pixels"
        );
    }
}

/// The route line color as stored bytes, to probe captured pixels against.
fn route_color_bytes() -> [u8; 3] {
    [
        (DEFAULT_ROUTE_LINE_RGBA[0] * f32::from(u8::MAX)) as u8,
        (DEFAULT_ROUTE_LINE_RGBA[1] * f32::from(u8::MAX)) as u8,
        (DEFAULT_ROUTE_LINE_RGBA[2] * f32::from(u8::MAX)) as u8,
    ]
}

/// The styled line-semantics scene's golden: one presentation per row over
/// the synthetic z0 tile, so the strokes are the frame's only feature.
const LINE_SEMANTICS_GOLDEN_FILE: &str = "line_semantics_strokes.png";
/// Each row's centre latitude, top to bottom, at the zoom-0 antimeridian
/// camera whose viewport spans the mercator world.
const LINE_SEMANTICS_LATITUDES_DEGREES: [f64; 6] = [60.0, 35.0, 10.0, -15.0, -40.0, -60.0];
/// Each stroke's longitude half-span east and west of the prime meridian.
const LINE_SEMANTICS_HALF_SPAN_DEGREES: f64 = 60.0;
/// The latitude each stroke rises at its centre, making the join visible.
const LINE_SEMANTICS_BEND_DEGREES: f64 = 10.0;
/// Each row's line width in screen pixels.
const LINE_SEMANTICS_WIDTHS_PIXELS: [f64; 6] = [8.0, 8.0, 8.0, 6.0, 4.0, 4.0];
/// The perpendicular offset the offset row declares.
const LINE_SEMANTICS_OFFSET_PIXELS: f64 = 10.0;
/// The gap the casing row leaves between its two bands.
const LINE_SEMANTICS_GAP_PIXELS: f64 = 10.0;
/// The dash pattern the dashed row declares, in line-width units.
const LINE_SEMANTICS_DASH_ARRAY: [f64; 2] = [2.0, 2.0];
/// One distinct opaque colour per row.
const LINE_SEMANTICS_COLORS: [[f32; 4]; 6] = [
    [0.90, 0.20, 0.20, 1.0],
    [0.95, 0.55, 0.10, 1.0],
    [0.95, 0.85, 0.10, 1.0],
    [0.20, 0.70, 0.30, 1.0],
    [0.10, 0.55, 0.90, 1.0],
    [0.55, 0.30, 0.85, 1.0],
];

/// The stroke one row demonstrates: miter, round join with round caps, bevel
/// join with square caps, dashes, an offset, and a casing gap.
fn line_semantics_stroke(row_index: usize) -> PolylineStroke {
    let mut stroke = PolylineStroke {
        cap: LineCap::Butt,
        join: LineJoin::Miter,
        dash_array: Vec::new(),
        offset_pixels: 0.0,
        gap_width_pixels: 0.0,
        blur_pixels: 0.0,
    };
    match row_index {
        0 => {}
        1 => {
            stroke.join = LineJoin::Round;
            stroke.cap = LineCap::Round;
        }
        2 => {
            stroke.join = LineJoin::Bevel;
            stroke.cap = LineCap::Square;
        }
        3 => stroke.dash_array = LINE_SEMANTICS_DASH_ARRAY.to_vec(),
        4 => stroke.offset_pixels = LINE_SEMANTICS_OFFSET_PIXELS,
        5 => stroke.gap_width_pixels = LINE_SEMANTICS_GAP_PIXELS,
        other => panic!("row {other} has no presentation"),
    }
    stroke
}

#[test]
fn styled_line_semantics_match_their_golden_fixture() {
    let Some(mut renderer) = request_renderer_or_skip() else {
        return;
    };
    let overlays: Vec<MapOverlay> = LINE_SEMANTICS_LATITUDES_DEGREES
        .iter()
        .enumerate()
        .map(|(row_index, latitude_degrees)| {
            let mut overlay = RoutePolylineOverlay::new(vec![
                scene_coordinate(*latitude_degrees, -LINE_SEMANTICS_HALF_SPAN_DEGREES),
                scene_coordinate(latitude_degrees + LINE_SEMANTICS_BEND_DEGREES, 0.0),
                scene_coordinate(*latitude_degrees, LINE_SEMANTICS_HALF_SPAN_DEGREES),
            ]);
            overlay.rgba_color = LINE_SEMANTICS_COLORS[row_index];
            overlay.width_pixels = LINE_SEMANTICS_WIDTHS_PIXELS[row_index];
            overlay.stroke = Some(line_semantics_stroke(row_index));
            MapOverlay::RoutePolyline(overlay)
        })
        .collect();
    let captured_frame = render_antimeridian_scene(&mut renderer, &overlays);
    compare_or_create_golden(
        &captured_frame,
        LINE_SEMANTICS_GOLDEN_FILE,
        "the line-semantics scene",
    );
}

/// The fill-and-circle paint scene's golden: an outlined fill, a translated
/// fill and three markers (plain, stroked, blurred) over the synthetic tile.
const FILL_AND_CIRCLE_GOLDEN_FILE: &str = "fill_and_circle_paint.png";
/// The scene's region latitudes, north and south.
const FILL_SCENE_NORTH_LATITUDE_DEGREES: f64 = 60.0;
/// See [`FILL_SCENE_NORTH_LATITUDE_DEGREES`].
const FILL_SCENE_SOUTH_LATITUDE_DEGREES: f64 = 30.0;
/// The outlined fill's western longitude.
const OUTLINED_FILL_WEST_LONGITUDE_DEGREES: f64 = -150.0;
/// The outlined fill's eastern longitude.
const OUTLINED_FILL_EAST_LONGITUDE_DEGREES: f64 = -90.0;
/// The translated fill's western longitude.
const TRANSLATED_FILL_WEST_LONGITUDE_DEGREES: f64 = 90.0;
/// The translated fill's eastern longitude.
const TRANSLATED_FILL_EAST_LONGITUDE_DEGREES: f64 = 150.0;
/// The translated fill's screen displacement, in pixels.
const TRANSLATED_FILL_PIXELS: [f64; 2] = [20.0, 12.0];
/// The three markers' longitude anchors.
const FILL_SCENE_MARKER_LONGITUDES_DEGREES: [f64; 3] = [-60.0, 0.0, 60.0];
/// The markers' radius in screen pixels.
const FILL_SCENE_MARKER_RADIUS_PIXELS: f64 = 20.0;
/// The stroked marker's stroke width.
const FILL_SCENE_MARKER_STROKE_PIXELS: f64 = 6.0;
/// The blurred marker's edge softening.
const FILL_SCENE_MARKER_BLUR_PIXELS: f64 = 8.0;
/// The outlined fill's fill colour.
const OUTLINED_FILL_RGBA: [f32; 4] = [0.20, 0.55, 0.85, 0.55];
/// The outlined fill's edge colour.
const OUTLINED_FILL_OUTLINE_RGBA: [f32; 4] = [0.95, 0.95, 0.95, 1.0];
/// The translated fill's fill colour.
const TRANSLATED_FILL_RGBA: [f32; 4] = [0.30, 0.75, 0.35, 0.55];
/// The plain marker's colour.
const FILL_SCENE_MARKER_RGBA: [f32; 4] = [0.90, 0.25, 0.20, 1.0];
/// The stroked marker's stroke colour.
const FILL_SCENE_MARKER_STROKE_RGBA: [f32; 4] = [0.10, 0.10, 0.10, 1.0];
/// The blurred marker's colour.
const FILL_SCENE_BLURRED_MARKER_RGBA: [f32; 4] = [0.75, 0.35, 0.85, 1.0];

fn fill_scene_ring(
    west_longitude_degrees: f64,
    east_longitude_degrees: f64,
) -> Vec<geospatial_types::GeographicCoordinate> {
    vec![
        scene_coordinate(FILL_SCENE_SOUTH_LATITUDE_DEGREES, west_longitude_degrees),
        scene_coordinate(FILL_SCENE_SOUTH_LATITUDE_DEGREES, east_longitude_degrees),
        scene_coordinate(FILL_SCENE_NORTH_LATITUDE_DEGREES, east_longitude_degrees),
        scene_coordinate(FILL_SCENE_NORTH_LATITUDE_DEGREES, west_longitude_degrees),
    ]
}

#[test]
fn fill_and_circle_paint_match_their_golden_fixture() {
    let Some(mut renderer) = request_renderer_or_skip() else {
        return;
    };
    let mut outlined_fill = FilledPolygonOverlay::new(fill_scene_ring(
        OUTLINED_FILL_WEST_LONGITUDE_DEGREES,
        OUTLINED_FILL_EAST_LONGITUDE_DEGREES,
    ));
    outlined_fill.rgba_color = OUTLINED_FILL_RGBA;
    outlined_fill.rgba_outline_color = Some(OUTLINED_FILL_OUTLINE_RGBA);

    let mut translated_fill = FilledPolygonOverlay::new(fill_scene_ring(
        TRANSLATED_FILL_WEST_LONGITUDE_DEGREES,
        TRANSLATED_FILL_EAST_LONGITUDE_DEGREES,
    ));
    translated_fill.rgba_color = TRANSLATED_FILL_RGBA;
    translated_fill.translate_pixels = TRANSLATED_FILL_PIXELS;

    let marker_positions: Vec<geospatial_types::GeographicCoordinate> =
        FILL_SCENE_MARKER_LONGITUDES_DEGREES
            .iter()
            .map(|longitude_degrees| scene_coordinate(0.0, *longitude_degrees))
            .collect();
    let mut plain_markers = PointMarkersOverlay::new(vec![marker_positions[0]]);
    plain_markers.rgba_color = FILL_SCENE_MARKER_RGBA;
    plain_markers.radius_pixels = FILL_SCENE_MARKER_RADIUS_PIXELS;
    let mut stroked_markers = PointMarkersOverlay::new(vec![marker_positions[1]]);
    stroked_markers.rgba_color = FILL_SCENE_MARKER_RGBA;
    stroked_markers.radius_pixels = FILL_SCENE_MARKER_RADIUS_PIXELS;
    stroked_markers.rgba_stroke_color = Some(FILL_SCENE_MARKER_STROKE_RGBA);
    stroked_markers.stroke_width_pixels = FILL_SCENE_MARKER_STROKE_PIXELS;
    let mut blurred_markers = PointMarkersOverlay::new(vec![marker_positions[2]]);
    blurred_markers.rgba_color = FILL_SCENE_BLURRED_MARKER_RGBA;
    blurred_markers.radius_pixels = FILL_SCENE_MARKER_RADIUS_PIXELS;
    blurred_markers.blur_pixels = FILL_SCENE_MARKER_BLUR_PIXELS;

    let overlays = vec![
        MapOverlay::FilledPolygon(outlined_fill),
        MapOverlay::FilledPolygon(translated_fill),
        MapOverlay::PointMarkers(plain_markers),
        MapOverlay::PointMarkers(stroked_markers),
        MapOverlay::PointMarkers(blurred_markers),
    ];
    let captured_frame = render_antimeridian_scene(&mut renderer, &overlays);
    compare_or_create_golden(
        &captured_frame,
        FILL_AND_CIRCLE_GOLDEN_FILE,
        "the fill-and-circle paint scene",
    );
}
