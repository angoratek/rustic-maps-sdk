//! The MapLibre demotiles style rendered end to end: the committed style
//! document over sixteen recorded z2 tiles from `demotiles.maplibre.org`,
//! captured against a committed PNG.
//!
//! The scene exercises what the style specification adoption promises — fills,
//! lines with dashes and casings, circles and wrapped symbol labels, zoom
//! rules and global layer order — on a style published for MapLibre rather
//! than one transcribed for this SDK. The tiles are committed fixtures
//! (`fixtures/maplibre-demotiles/`), so the test never touches the network.
//!
//! Comparison is tolerant like the other goldens: GPUs differ in sampler
//! behavior. A missing golden is written deliberately with `UPDATE_GOLDENS=1`;
//! the test skips when no graphics adapter exists.

use async_trait::async_trait;
use geospatial_traits::{VectorTileProvider, VectorTileSourceDescriptor};
use geospatial_types::{GeographicCoordinate, SDKError, VectorTileData};
use rustic_maps_rendering::vector_base::VectorBaseScheduler;
use rustic_maps_rendering::vector_style::vector_tile_style_from_json;
use rustic_maps_rendering::{
    CapturedFrame, DEFAULT_FIELD_OF_VIEW_DEGREES, GraphicsContextSource, HeadlessTileRenderer,
    MapCamera, TileTexturePool, ViewportPixels, WgpuGraphicsContextSource,
};
use std::path::PathBuf;
use std::sync::Arc;

/// Frame edge in pixels, the other goldens' extent.
const GOLDEN_VIEWPORT_EDGE_PIXELS: f64 = 512.0;
/// The camera's centre: the equator and prime meridian corner, so the four
/// surrounding z2 tiles fill the frame.
const SCENE_CENTER_LATITUDE_DEGREES: f64 = 0.0;
/// See [`SCENE_CENTER_LATITUDE_DEGREES`].
const SCENE_CENTER_LONGITUDE_DEGREES: f64 = 0.0;
/// The served tile zoom level, the demotiles tileset's first labelled zoom.
const SCENE_TILE_ZOOM_LEVEL: u8 = 2;
/// The scene camera's zoom, equal to the served tile zoom.
const SCENE_CAMERA_ZOOM_LEVEL: f64 = 2.0;
/// Directory of the golden fixtures, relative to the crate manifest.
const GOLDEN_DIRECTORY: &str = "fixtures/render/golden";
/// Fixture of the demotiles scene.
const DEMOTILES_GOLDEN_FILE: &str = "maplibre_demotiles_zoom2.png";
/// The style id the scheduler reports for the scene.
const DEMOTILES_STYLE_ID: &str = "demotiles";
/// The committed MapLibre demotiles style document.
const DEMOTILES_STYLE_JSON: &str =
    include_str!("../../rustic-maps-style/fixtures/styles/demotiles-style.json");

/// The tile addresses the fixture set covers (x, y at z2), each paired with
/// its committed payload.
const DEMOTILES_TILES: [(u32, u32, &[u8]); 16] = [
    (
        0,
        0,
        include_bytes!("../fixtures/maplibre-demotiles/2_0_0.pbf"),
    ),
    (
        0,
        1,
        include_bytes!("../fixtures/maplibre-demotiles/2_0_1.pbf"),
    ),
    (
        0,
        2,
        include_bytes!("../fixtures/maplibre-demotiles/2_0_2.pbf"),
    ),
    (
        0,
        3,
        include_bytes!("../fixtures/maplibre-demotiles/2_0_3.pbf"),
    ),
    (
        1,
        0,
        include_bytes!("../fixtures/maplibre-demotiles/2_1_0.pbf"),
    ),
    (
        1,
        1,
        include_bytes!("../fixtures/maplibre-demotiles/2_1_1.pbf"),
    ),
    (
        1,
        2,
        include_bytes!("../fixtures/maplibre-demotiles/2_1_2.pbf"),
    ),
    (
        1,
        3,
        include_bytes!("../fixtures/maplibre-demotiles/2_1_3.pbf"),
    ),
    (
        2,
        0,
        include_bytes!("../fixtures/maplibre-demotiles/2_2_0.pbf"),
    ),
    (
        2,
        1,
        include_bytes!("../fixtures/maplibre-demotiles/2_2_1.pbf"),
    ),
    (
        2,
        2,
        include_bytes!("../fixtures/maplibre-demotiles/2_2_2.pbf"),
    ),
    (
        2,
        3,
        include_bytes!("../fixtures/maplibre-demotiles/2_2_3.pbf"),
    ),
    (
        3,
        0,
        include_bytes!("../fixtures/maplibre-demotiles/2_3_0.pbf"),
    ),
    (
        3,
        1,
        include_bytes!("../fixtures/maplibre-demotiles/2_3_1.pbf"),
    ),
    (
        3,
        2,
        include_bytes!("../fixtures/maplibre-demotiles/2_3_2.pbf"),
    ),
    (
        3,
        3,
        include_bytes!("../fixtures/maplibre-demotiles/2_3_3.pbf"),
    ),
];

/// Serves the committed demotiles tiles and refuses any other address, so
/// only the fixtures contribute geometry.
struct DemotilesFixtureProvider;

#[async_trait]
impl VectorTileProvider for DemotilesFixtureProvider {
    fn source_descriptor(&self) -> VectorTileSourceDescriptor {
        VectorTileSourceDescriptor {
            source_name: "demotiles".to_string(),
            attribution_text: "Map data © OpenStreetMap contributors (ODbL); tiles and style \
from MapLibre demotiles"
                .to_string(),
            minimum_zoom_level: SCENE_TILE_ZOOM_LEVEL,
            maximum_zoom_level: SCENE_TILE_ZOOM_LEVEL,
            style_hints: Vec::new(),
        }
    }

    async fn fetch_vector_tile(
        &self,
        tile_x: u32,
        tile_y: u32,
        zoom_level: u8,
    ) -> Result<VectorTileData, SDKError> {
        for (fixture_x, fixture_y, tile_bytes) in DEMOTILES_TILES {
            if zoom_level == SCENE_TILE_ZOOM_LEVEL && tile_x == fixture_x && tile_y == fixture_y {
                return VectorTileData::from_wire_bytes(
                    tile_x,
                    tile_y,
                    zoom_level,
                    tile_bytes.to_vec(),
                );
            }
        }
        Err(SDKError::ResponseParsingFailure(format!(
            "the demotiles fixture serves no {zoom_level}/{tile_x}/{tile_y}"
        )))
    }
}

/// The scene camera: top-down, north up, centred on the z2 tile corner.
fn build_camera() -> MapCamera {
    MapCamera::new(
        GeographicCoordinate {
            latitude: SCENE_CENTER_LATITUDE_DEGREES,
            longitude: SCENE_CENTER_LONGITUDE_DEGREES,
        },
        SCENE_CAMERA_ZOOM_LEVEL,
        0.0,
        0.0,
    )
}

/// Acquires the headless renderer, printing the backend, or `None` when no
/// adapter is available so CPU-only environments skip.
fn request_renderer_or_skip() -> Option<HeadlessTileRenderer> {
    match WgpuGraphicsContextSource.request_graphics_context() {
        Ok(graphics_context) => {
            println!(
                "rendering on backend {:?} with adapter {:?}",
                graphics_context.adapter_information.backend,
                graphics_context.adapter_information.name
            );
            Some(
                HeadlessTileRenderer::with_graphics_context(graphics_context)
                    .expect("the pipeline builds on the acquired context"),
            )
        }
        Err(error) => {
            println!("skipping GPU golden test: no graphics adapter available ({error})");
            None
        }
    }
}

/// Whether this run may write a missing golden (a deliberate regeneration).
fn golden_regeneration_is_requested() -> bool {
    std::env::var("UPDATE_GOLDENS").is_ok_and(|value| value == "1")
}

/// Compares a captured frame against its golden, writing it when the
/// regeneration flag is set and it is missing.
fn compare_or_create_golden(captured_frame: &CapturedFrame, golden_file_name: &str) {
    let golden_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(GOLDEN_DIRECTORY)
        .join(golden_file_name);
    if !golden_path.exists() {
        if !golden_regeneration_is_requested() {
            panic!(
                "golden fixture is missing at {golden_path:?}; regenerate deliberately with \
                 UPDATE_GOLDENS=1 and commit the result"
            );
        }
        image::save_buffer(
            &golden_path,
            &captured_frame.rgba_bytes,
            captured_frame.width_pixels,
            captured_frame.height_pixels,
            image::ColorType::Rgba8,
        )
        .expect("the golden fixture is written on regeneration");
        println!(
            "golden fixture was missing; wrote {golden_path:?} - run the test again to verify"
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
        if pixel_maximum_error > 24.0 {
            outlier_pixel_count += 1;
        }
    }
    let mean_absolute_error = absolute_error_sum / (f64::from(pixel_count) * 4.0);
    let outlier_pixel_fraction = outlier_pixel_count as f64 / f64::from(pixel_count);
    assert!(
        mean_absolute_error <= 2.0,
        "the demotiles frame drifted from {golden_file_name}: mean channel error \
         {mean_absolute_error:.3} exceeds 2.0"
    );
    assert!(
        outlier_pixel_fraction <= 0.02,
        "the demotiles frame drifted from {golden_file_name}: {outlier_pixel_fraction:.4} of \
         pixels exceed the outlier error, above 0.02"
    );
}

#[tokio::test]
async fn the_maplibre_demotiles_style_renders_end_to_end() {
    let Some(mut renderer) = request_renderer_or_skip() else {
        return;
    };
    let camera = build_camera();
    let viewport = ViewportPixels::new(GOLDEN_VIEWPORT_EDGE_PIXELS, GOLDEN_VIEWPORT_EDGE_PIXELS)
        .expect("the viewport extent is valid");
    let compiled_style =
        vector_tile_style_from_json(DEMOTILES_STYLE_JSON).expect("the demotiles style compiles");
    let mut scheduler = VectorBaseScheduler::new(
        Arc::new(DemotilesFixtureProvider),
        DEMOTILES_STYLE_ID,
        compiled_style,
    );
    let visible_tile_keys = camera.visible_tile_keys(
        &viewport,
        DEFAULT_FIELD_OF_VIEW_DEGREES,
        SCENE_TILE_ZOOM_LEVEL,
    );
    let vector_base_frame = scheduler
        .resolve_frame(&visible_tile_keys, camera.zoom_level)
        .await;
    assert!(
        vector_base_frame.drawn_tile_count > 0,
        "the recorded demotiles tiles must draw"
    );
    let label_layer =
        vector_base_frame.label_layer(&camera, &viewport, DEFAULT_FIELD_OF_VIEW_DEGREES);
    assert!(
        label_layer
            .as_ref()
            .is_some_and(|label_layer| !label_layer.is_empty()),
        "the demotiles style must place at least one label"
    );
    if let Some(background) = vector_base_frame.background {
        renderer.set_clear_color_rgba(background);
    }
    let mut pool = TileTexturePool::new(0);
    let captured_frame = renderer
        .render_captured_frame_layers_and_labels(
            &camera,
            &viewport,
            &[],
            &mut pool,
            &vector_base_frame.overlays,
            None,
            label_layer.as_ref(),
        )
        .expect("the demotiles frame renders");
    compare_or_create_golden(&captured_frame, DEMOTILES_GOLDEN_FILE);
}
