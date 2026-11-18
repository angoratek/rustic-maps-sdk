//! Graceful-degradation behavior of the headless renderer: one undecodable
//! pooled payload must skip only its own quad, evict the corrupt bytes, and
//! leave every later frame healthy — never a hard error for the frame.
//!
//! Skips gracefully when no graphics adapter is available, as the golden
//! tests do.

use rustic_maps_rendering::{
    CLEAR_COLOR_RGBA, GraphicsContextSource, HeadlessTileRenderer, MapCamera, PreparedTile,
    TileKey, TileReadiness, TileTexturePool, ViewportPixels, WgpuGraphicsContextSource,
};

/// Frame edge of the degradation scene in pixels.
const DEGRADATION_VIEWPORT_EDGE_PIXELS: f64 = 512.0;
/// Tile zoom of the degradation scene: at this zoom a 512-pixel viewport
/// spans two tiles per axis, so one corrupt payload coexists with recoverable
/// neighbors in one frame.
const DEGRADATION_TILE_ZOOM_LEVEL: u8 = 2;
/// Bearing and pitch of the degradation camera: north up, top down.
const DEGRADATION_LEVEL_PITCH_DEGREES: f64 = 0.0;
/// Payload bytes that no PNG nor any other supported image format decodes to.
const CORRUPT_TILE_PAYLOAD_BYTES: [u8; 4] = [7, 13, 42, 200];
/// Texel edge of the recoverable tiles' generated payloads; any decodable
/// size works, since the sampler draws it at the quad's own resolution.
const RECOVERABLE_TILE_EDGE_PIXELS: u32 = 64;

#[test]
fn an_undecodable_tile_payload_skips_its_quad_and_evicts_the_bytes() {
    match WgpuGraphicsContextSource.request_graphics_context() {
        Ok(graphics_context) => {
            println!(
                "rendering on backend {:?} with adapter {:?}",
                graphics_context.adapter_information.backend,
                graphics_context.adapter_information.name
            );
            let mut renderer = HeadlessTileRenderer::with_graphics_context(graphics_context)
                .expect("the pipeline builds on the acquired context");
            exercise_corrupt_payload_degradation(&mut renderer);
        }
        Err(error) => {
            println!("skipping GPU degradation test: no graphics adapter available ({error})");
        }
    }
}

fn exercise_corrupt_payload_degradation(renderer: &mut HeadlessTileRenderer) {
    let viewport = ViewportPixels::new(
        DEGRADATION_VIEWPORT_EDGE_PIXELS,
        DEGRADATION_VIEWPORT_EDGE_PIXELS,
    )
    .expect("the degradation viewport is a valid viewport");
    let camera = MapCamera::new(
        rustic_maps_rendering::normalized_world_to_coordinate([0.25, 0.25]),
        f64::from(DEGRADATION_TILE_ZOOM_LEVEL),
        0.0,
        DEGRADATION_LEVEL_PITCH_DEGREES,
    );
    let scene_tile_keys = camera.visible_tile_keys(
        &viewport,
        rustic_maps_rendering::DEFAULT_FIELD_OF_VIEW_DEGREES,
        DEGRADATION_TILE_ZOOM_LEVEL,
    );
    assert!(
        scene_tile_keys.len() >= 2,
        "the scene must span several tiles so one corrupt payload coexists with recoverable ones"
    );
    let corrupt_tile_key = scene_tile_keys[0];

    let mut payload_pool = TileTexturePool::new(1 << 20);
    for tile_key in &scene_tile_keys {
        if *tile_key == corrupt_tile_key {
            payload_pool.insert(*tile_key, CORRUPT_TILE_PAYLOAD_BYTES.to_vec());
        } else {
            payload_pool.insert(*tile_key, recoverable_tile_png(*tile_key));
        }
    }
    let prepared_tiles = scene_tile_keys
        .iter()
        .map(|tile_key| PreparedTile {
            requested_tile_key: *tile_key,
            readiness: TileReadiness::Present,
        })
        .collect::<Vec<PreparedTile>>();

    let captured_frame = renderer
        .render_captured_frame(&camera, &viewport, &prepared_tiles, &mut payload_pool)
        .expect("a corrupt payload must degrade the frame, not fail it");
    assert!(
        !payload_pool.contains(&corrupt_tile_key),
        "the corrupt payload must leave the pool so the scheduler refetches it"
    );
    for tile_key in scene_tile_keys.iter().skip(1) {
        assert!(
            payload_pool.contains(tile_key),
            "the corrupt payload's eviction must not disturb its healthy neighbors"
        );
    }
    let pixel_count = captured_frame.width_pixels * captured_frame.height_pixels;
    assert_eq!(
        captured_frame.rgba_bytes.len(),
        pixel_count as usize * 4,
        "the degraded frame is still a complete captured frame"
    );
    assert!(
        captured_frame
            .rgba_bytes
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| *pixel != clear_color_bytes()),
        "the healthy tiles must still have rendered over the clear color"
    );
}

/// The clear color as stored bytes, to compare against captured pixels.
fn clear_color_bytes() -> [u8; 4] {
    [
        (CLEAR_COLOR_RGBA[0] * f32::from(u8::MAX)) as u8,
        (CLEAR_COLOR_RGBA[1] * f32::from(u8::MAX)) as u8,
        (CLEAR_COLOR_RGBA[2] * f32::from(u8::MAX)) as u8,
        (CLEAR_COLOR_RGBA[3] * f32::from(u8::MAX)) as u8,
    ]
}

/// Generates a deterministic valid PNG payload for a tile: a distinct
/// axis-aligned gradient per tile, enough to leave a visible trace when drawn.
fn recoverable_tile_png(tile_key: TileKey) -> Vec<u8> {
    let tile_image = image::RgbaImage::from_fn(
        RECOVERABLE_TILE_EDGE_PIXELS,
        RECOVERABLE_TILE_EDGE_PIXELS,
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
