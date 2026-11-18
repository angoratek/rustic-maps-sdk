//! Golden-image harness for a declared style transition.
//!
//! Two one-layer styles paint the committed OpenMapTiles z14 fixture's water
//! fills in different colours and opacities. The incoming style declares a
//! 400 ms root `transition`; the frames at the change, half way through it and
//! at its end are rendered through the shared `VectorBaseScheduler`. The
//! half-way frame is the committed golden, the frame at the change must equal
//! the outgoing style's settled frame, and the frame at the end must equal a
//! fresh scheduler drawing the incoming style with the untimed path — so the
//! transition blends between the two settled styles instead of inventing a
//! third, and the no-transition path stays byte-identical.
//!
//! Comparison is tolerant in the same way as the other goldens: GPUs differ
//! in sampler behavior. A missing golden is written deliberately with
//! `UPDATE_GOLDENS=1`; the test skips when no graphics adapter exists.

use async_trait::async_trait;
use geospatial_traits::{VectorTileProvider, VectorTileSourceDescriptor};
use geospatial_types::{GeographicCoordinate, SDKError, VectorTileData};
use rustic_maps_rendering::vector_base::{VectorBaseFrame, VectorBaseScheduler};
use rustic_maps_rendering::vector_style::vector_tile_style_from_json;
use rustic_maps_rendering::{
    CapturedFrame, DEFAULT_FIELD_OF_VIEW_DEGREES, GraphicsContextSource, HeadlessTileRenderer,
    MapCamera, TileKey, TileTexturePool, ViewportPixels, WgpuGraphicsContextSource,
};
use rustic_maps_style::CompiledStyle;
use std::path::PathBuf;
use std::sync::Arc;

/// Frame edge in pixels (the other golden scenes' extent).
const GOLDEN_VIEWPORT_EDGE_PIXELS: f64 = 512.0;
/// The committed OpenMapTiles z14 fixture's center: the camera keeps exactly
/// the served tile in view.
const TILE_CENTER_LATITUDE_DEGREES: f64 = 38.573_937_515_575_91;
const TILE_CENTER_LONGITUDE_DEGREES: f64 = -121.519_775_390_625;
/// The fixture tile's address.
const FIXTURE_TILE_KEY: TileKey = TileKey {
    tile_x: 2661,
    tile_y: 6286,
    zoom_level: 14,
};
/// The committed fixture the served tile answers with.
const FIXTURE_TILE_BYTES: &[u8] =
    include_bytes!("../../../fixtures/openfreemap/openmaptiles_tile_z14_x2661_y6286.pbf");
/// The transition the incoming style declares, in milliseconds.
const TRANSITION_DURATION_MILLISECONDS: f64 = 400.0;
/// The mid-frame golden the half-way blend pins.
const TRANSITION_MID_FRAME_GOLDEN_FILE: &str = "style_transition_cross_fade_half.png";
/// The clear colour every captured frame shares, so the translucent blend is
/// legible against the surface rather than black.
const GOLDEN_BACKGROUND_RGBA: [f32; 4] = [0.90, 0.90, 0.90, 1.0];
/// Tolerances mirrored from the other vector-base goldens.
const MEAN_ABSOLUTE_ERROR_TOLERANCE: f64 = 2.0;
const OUTLIER_CHANNEL_ERROR: f64 = 24.0;
const OUTLIER_PIXEL_FRACTION_LIMIT: f64 = 0.02;

/// A provider serving the one committed tile; any other address fails, so
/// only the fixture contributes geometry.
struct FixtureTileProvider;

#[async_trait]
impl VectorTileProvider for FixtureTileProvider {
    fn source_descriptor(&self) -> VectorTileSourceDescriptor {
        VectorTileSourceDescriptor {
            source_name: "golden-vector-transition".to_string(),
            attribution_text: "Committed golden vector tile".to_string(),
            minimum_zoom_level: 0,
            maximum_zoom_level: 14,
            style_hints: Vec::new(),
        }
    }

    async fn fetch_vector_tile(
        &self,
        tile_x: u32,
        tile_y: u32,
        zoom_level: u8,
    ) -> Result<VectorTileData, SDKError> {
        let tile_key = TileKey::new(tile_x, tile_y, zoom_level)?;
        if tile_key != FIXTURE_TILE_KEY {
            return Err(SDKError::ResponseParsingFailure(format!(
                "the transition golden provider serves no {zoom_level}/{tile_x}/{tile_y}"
            )));
        }
        VectorTileData::from_wire_bytes(tile_x, tile_y, zoom_level, FIXTURE_TILE_BYTES.to_vec())
    }
}

/// A one-layer style over the fixture's water fills, with an optional root
/// `transition` duration.
fn transition_style(fill_color: &str, fill_opacity: f64, transition: bool) -> CompiledStyle {
    let transition_block = if transition {
        format!(r#", "transition": {{ "duration": {TRANSITION_DURATION_MILLISECONDS} }}"#)
    } else {
        String::new()
    };
    vector_tile_style_from_json(&format!(
        r#"{{
            "version": 8,
            "sources": {{ "base": {{ "type": "vector" }} }}{transition_block},
            "layers": [
                {{
                    "id": "water",
                    "type": "fill",
                    "source": "base",
                    "source-layer": "water",
                    "paint": {{ "fill-color": "{fill_color}", "fill-opacity": {fill_opacity} }}
                }}
            ]
        }}"#
    ))
    .expect("the transition fixture style parses")
}

/// Builds the fixture scene's camera (top-down, north up).
fn build_camera() -> MapCamera {
    MapCamera::new(
        GeographicCoordinate {
            latitude: TILE_CENTER_LATITUDE_DEGREES,
            longitude: TILE_CENTER_LONGITUDE_DEGREES,
        },
        14.0,
        0.0,
        0.0,
    )
}

/// Renders a resolved vector-base frame into the shared golden viewport.
fn render_frame(
    renderer: &mut HeadlessTileRenderer,
    camera: &MapCamera,
    viewport: &ViewportPixels,
    vector_base_frame: &VectorBaseFrame,
) -> CapturedFrame {
    renderer.set_clear_color_rgba(GOLDEN_BACKGROUND_RGBA);
    let mut pool = TileTexturePool::new(0);
    renderer
        .render_captured_frame_layers_and_labels(
            camera,
            viewport,
            &[],
            &mut pool,
            &vector_base_frame.overlays,
            None,
            None,
        )
        .expect("the transition frame renders")
}

/// Renders a fresh scheduler's settled frame for the named style through the
/// untimed path.
async fn settled_frame(style: CompiledStyle) -> Option<(HeadlessTileRenderer, CapturedFrame)> {
    let mut renderer = request_renderer_or_skip()?;
    let viewport = ViewportPixels::new(GOLDEN_VIEWPORT_EDGE_PIXELS, GOLDEN_VIEWPORT_EDGE_PIXELS)
        .expect("the viewport extent is valid");
    let camera = build_camera();
    let mut scheduler = VectorBaseScheduler::new(Arc::new(FixtureTileProvider), "settled", style);
    let visible_tile_keys = camera.visible_tile_keys(
        &viewport,
        DEFAULT_FIELD_OF_VIEW_DEGREES,
        FIXTURE_TILE_KEY.zoom_level,
    );
    let frame = scheduler
        .resolve_frame(&visible_tile_keys, camera.zoom_level)
        .await;
    assert!(frame.drawn_tile_count > 0, "the fixture tile must draw");
    let captured = render_frame(&mut renderer, &camera, &viewport, &frame);
    Some((renderer, captured))
}

#[tokio::test]
async fn a_declared_style_transition_blends_between_the_two_settled_styles() {
    let Some((mut renderer, settled_outgoing)) =
        settled_frame(transition_style("#ff0000", 0.6, false)).await
    else {
        return;
    };
    let viewport = ViewportPixels::new(GOLDEN_VIEWPORT_EDGE_PIXELS, GOLDEN_VIEWPORT_EDGE_PIXELS)
        .expect("the viewport extent is valid");
    let camera = build_camera();
    let visible_tile_keys = camera.visible_tile_keys(
        &viewport,
        DEFAULT_FIELD_OF_VIEW_DEGREES,
        FIXTURE_TILE_KEY.zoom_level,
    );
    let mut scheduler = VectorBaseScheduler::new(
        Arc::new(FixtureTileProvider),
        "outgoing",
        transition_style("#ff0000", 0.6, false),
    );
    scheduler
        .resolve_frame(&visible_tile_keys, camera.zoom_level)
        .await;
    scheduler.set_compiled_style("incoming", transition_style("#0000ff", 0.2, true));

    // The change's first timed frame is still entirely the outgoing style.
    let at_start = scheduler.cached_frame_at_time(&visible_tile_keys, camera.zoom_level, 10.0);
    let start_captured = render_frame(&mut renderer, &camera, &viewport, &at_start);
    assert_eq!(
        start_captured.rgba_bytes, settled_outgoing.rgba_bytes,
        "the frame at the change must equal the outgoing style's settled frame"
    );

    // Half the declared duration later the blend is half way.
    let at_half = scheduler.cached_frame_at_time(
        &visible_tile_keys,
        camera.zoom_level,
        10.0 + TRANSITION_DURATION_MILLISECONDS / 2000.0,
    );
    let half_captured = render_frame(&mut renderer, &camera, &viewport, &at_half);
    compare_or_create_golden(&half_captured, TRANSITION_MID_FRAME_GOLDEN_FILE);

    // At the declared duration the change has landed and the transition has
    // stopped running. The frame must equal the untimed path's settled frame.
    let at_end = scheduler.cached_frame_at_time(
        &visible_tile_keys,
        camera.zoom_level,
        10.0 + TRANSITION_DURATION_MILLISECONDS / 1000.0,
    );
    assert!(!scheduler.style_transition_running());
    let end_captured = render_frame(&mut renderer, &camera, &viewport, &at_end);
    let (_, settled_incoming) = settled_frame(transition_style("#0000ff", 0.2, true))
        .await
        .expect("the renderer was already available");
    assert_eq!(
        end_captured.rgba_bytes, settled_incoming.rgba_bytes,
        "the settled transition frame must equal the untimed path's frame"
    );
}

/// Acquires the renderer or prints why the CPU-only environment skips.
fn request_renderer_or_skip() -> Option<HeadlessTileRenderer> {
    match WgpuGraphicsContextSource.request_graphics_context() {
        Ok(graphics_context) => Some(
            HeadlessTileRenderer::with_graphics_context(graphics_context)
                .expect("the pipeline builds on the acquired context"),
        ),
        Err(error) => {
            println!("skipping GPU golden test: no graphics adapter available ({error})");
            None
        }
    }
}

/// Compares against (or deliberately writes) the scene's golden.
fn compare_or_create_golden(captured_frame: &CapturedFrame, golden_file_name: &str) {
    let golden_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/render/golden")
        .join(golden_file_name);
    if !golden_path.exists() {
        if std::env::var("UPDATE_GOLDENS").is_err() {
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
        .expect("the golden fixture is written");
        println!("wrote {golden_path:?}; review and commit it");
        return;
    }
    let golden_image = image::open(&golden_path)
        .expect("the golden fixture is readable")
        .to_rgba8();
    let golden_bytes = golden_image.as_raw();
    let pixel_count = captured_frame.width_pixels * captured_frame.height_pixels;
    let mut absolute_error_sum = 0.0_f64;
    let mut outlier_pixel_count = 0_u64;
    for (captured_pixel, golden_pixel) in captured_frame
        .rgba_bytes
        .as_chunks::<4>()
        .0
        .iter()
        .zip(golden_bytes.as_chunks::<4>().0)
    {
        let mut pixel_maximum_error = 0.0_f64;
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
    assert!(
        mean_absolute_error <= MEAN_ABSOLUTE_ERROR_TOLERANCE,
        "the transition frame drifted from {golden_path:?}: mean absolute error \
         {mean_absolute_error}"
    );
    assert!(
        outlier_pixel_fraction <= OUTLIER_PIXEL_FRACTION_LIMIT,
        "the transition frame drifted from {golden_path:?}: outlier fraction \
         {outlier_pixel_fraction}"
    );
}
