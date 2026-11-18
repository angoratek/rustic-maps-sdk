//! The idle render-state signal and the idle-gated snapshot at the render
//! boundary: a snapshot must refuse while pending tile work remains (the
//! determinism rule behind GL JS's `idle` event and MapLibre's
//! `snapshotSync`), capture byte-for-byte like the ordinary capture once the
//! frame is idle, and flip back to pending when the pool loses a requested
//! payload — including through a real draw's corrupt-payload eviction.
//!
//! Skips gracefully when no graphics adapter is available, as the golden
//! tests do.

use async_trait::async_trait;
use geospatial_traits::RasterTileProvider;
use geospatial_types::{RasterImageFormat, RasterTileData, RasterTileResolution, SDKError};
use rustic_maps_rendering::{
    DEFAULT_FIELD_OF_VIEW_DEGREES, GraphicsContextSource, HeadlessRenderError,
    HeadlessTileRenderer, MapCamera, PreparedTile, TileKey, TileReadiness, TileScheduler,
    TileTexturePool, ViewportPixels, WgpuGraphicsContextSource, prepared_frame_is_idle,
};
use std::sync::{Arc, Mutex};

/// Frame edge of the snapshot scene in pixels; a whole power of two so tile
/// spans map cleanly.
const SNAPSHOT_VIEWPORT_EDGE_PIXELS: f64 = 512.0;
/// Tile zoom of the snapshot scene: at this zoom the 512-pixel viewport
/// spans two tiles per axis, so the scene carries several tiles whose fetch
/// budget can split pending from ready.
const SNAPSHOT_TILE_ZOOM_LEVEL: u8 = 2;
/// Bearing of the snapshot camera: north up.
const SNAPSHOT_BEARING_DEGREES: f64 = 0.0;
/// Pitch of the snapshot camera: straight down.
const SNAPSHOT_PITCH_DEGREES: f64 = 0.0;
/// Encoded-pixel budget of the test pool; every scene payload fits.
const SCENE_POOL_BUDGET_BYTES: u64 = 64 << 20;
/// Per-frame fetch budget for the pending-frame fixture: one fetch per
/// frame, so a two-tile request set always leaves the surplus deferred.
const PENDING_FRAME_FETCH_BUDGET: usize = 1;
/// Per-frame fetch budget large enough to fetch every scene tile at once.
const COMPLETE_FRAME_FETCH_BUDGET: usize = 16;
/// Texel edge of the generated tile payloads; any decodable size works,
/// since the sampler draws it at the quad's own resolution.
const GENERATED_TILE_EDGE_PIXELS: u32 = 64;
/// Payload bytes that no supported image format decodes to.
const CORRUPT_TILE_PAYLOAD_BYTES: [u8; 2] = [7, 13];

/// Provider serving a deterministic decodable PNG payload for every
/// requested tile key, so the scheduler's fetches pool renderable bytes.
struct DecodableTileProvider {
    served_tiles: Mutex<Vec<TileKey>>,
}

impl DecodableTileProvider {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            served_tiles: Mutex::new(Vec::new()),
        })
    }
}

#[async_trait]
impl RasterTileProvider for DecodableTileProvider {
    async fn fetch_raster_tile(
        &self,
        tile_x: u32,
        tile_y: u32,
        zoom_level: u8,
        _image_format: RasterImageFormat,
        _tile_resolution: RasterTileResolution,
    ) -> Result<RasterTileData, SDKError> {
        let requested_tile_key = TileKey::new(tile_x, tile_y, zoom_level).unwrap();
        self.served_tiles.lock().unwrap().push(requested_tile_key);
        Ok(RasterTileData {
            tile_x,
            tile_y,
            zoom_level,
            image_format: RasterImageFormat::Png,
            tile_resolution: RasterTileResolution::Standard,
            image_bytes: generated_tile_png(requested_tile_key),
        })
    }
}

/// Generates a deterministic valid PNG payload for a tile: a distinct
/// axis-aligned gradient per tile, enough to leave a visible trace when
/// drawn (the same generation the degradation tests render from).
fn generated_tile_png(tile_key: TileKey) -> Vec<u8> {
    let tile_image = image::RgbaImage::from_fn(
        GENERATED_TILE_EDGE_PIXELS,
        GENERATED_TILE_EDGE_PIXELS,
        |pixel_x, pixel_y| {
            image::Rgba([
                (pixel_x % u32::from(u8::MAX)) as u8,
                (pixel_y % u32::from(u8::MAX)) as u8,
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

/// Builds the scene camera and viewport, and returns them with the tile set
/// the camera's footprint lists at the scene zoom.
fn snapshot_scene() -> (MapCamera, ViewportPixels, Vec<TileKey>) {
    let camera = MapCamera::new(
        rustic_maps_rendering::normalized_world_to_coordinate([0.25, 0.25]),
        f64::from(SNAPSHOT_TILE_ZOOM_LEVEL),
        SNAPSHOT_BEARING_DEGREES,
        SNAPSHOT_PITCH_DEGREES,
    );
    let viewport =
        ViewportPixels::new(SNAPSHOT_VIEWPORT_EDGE_PIXELS, SNAPSHOT_VIEWPORT_EDGE_PIXELS)
            .expect("the snapshot viewport is a valid viewport");
    let scene_tile_keys = camera.visible_tile_keys(
        &viewport,
        DEFAULT_FIELD_OF_VIEW_DEGREES,
        SNAPSHOT_TILE_ZOOM_LEVEL,
    );
    assert!(
        scene_tile_keys.len() >= 2,
        "the scene must span several tiles so a fetch budget can split pending from ready"
    );
    (camera, viewport, scene_tile_keys)
}

#[tokio::test]
async fn a_pending_frame_refuses_the_snapshot_and_an_idle_one_matches_the_ordinary_capture() {
    match WgpuGraphicsContextSource.request_graphics_context() {
        Ok(graphics_context) => {
            println!(
                "rendering on backend {:?} with adapter {:?}",
                graphics_context.adapter_information.backend,
                graphics_context.adapter_information.name
            );
            let mut renderer = HeadlessTileRenderer::with_graphics_context(graphics_context)
                .expect("the pipeline builds on the acquired context");
            exercise_idle_gating_and_capture(&mut renderer).await;
        }
        Err(error) => {
            println!("skipping GPU idle-snapshot test: no graphics adapter available ({error})");
        }
    }
}

async fn exercise_idle_gating_and_capture(renderer: &mut HeadlessTileRenderer) {
    let (camera, viewport, scene_tile_keys) = snapshot_scene();

    // Mid-fetch state: the per-frame budget defers the surplus, so the frame
    // is pending and the idle-gated snapshot refuses before any GPU work.
    let mut pending_scheduler = TileScheduler::new(
        DecodableTileProvider::new(),
        TileTexturePool::new(SCENE_POOL_BUDGET_BYTES),
        RasterImageFormat::Png,
        RasterTileResolution::Standard,
        PENDING_FRAME_FETCH_BUDGET,
    );
    let pending_frame = pending_scheduler.prepare_frame(&scene_tile_keys).await;
    assert_eq!(
        pending_frame.len(),
        scene_tile_keys.len(),
        "the prepared frame reports every requested tile"
    );
    assert!(
        !prepared_frame_is_idle(&pending_frame),
        "the budget left a tile deferred, so the frame is pending"
    );
    let pending_pool = pending_scheduler.texture_pool_mut();
    let refusal = renderer
        .snapshot_frame_when_idle(&camera, &viewport, &pending_frame, pending_pool, &[])
        .expect_err("a pending frame must refuse the snapshot");
    match refusal {
        HeadlessRenderError::PendingTileWork { pending_tile_count } => {
            assert_eq!(
                pending_tile_count,
                scene_tile_keys.len() - PENDING_FRAME_FETCH_BUDGET,
                "the refusal must count exactly the tiles still lacking their own payload"
            );
        }
        other => panic!("expected a pending-work refusal, got {other:?}"),
    }

    // After the fetches complete: fully present verdicts, an idle signal,
    // and a snapshot that captures. The idle snapshot must be byte-for-byte
    // the ordinary capture's output — the gate decides *whether* to render,
    // never *what*.
    let mut tile_scheduler = TileScheduler::new(
        DecodableTileProvider::new(),
        TileTexturePool::new(SCENE_POOL_BUDGET_BYTES),
        RasterImageFormat::Png,
        RasterTileResolution::Standard,
        COMPLETE_FRAME_FETCH_BUDGET,
    );
    let idle_frame = tile_scheduler.prepare_frame(&scene_tile_keys).await;
    assert!(
        prepared_frame_is_idle(&idle_frame),
        "a fully fetched frame is idle"
    );
    assert!(tile_scheduler.is_idle(&scene_tile_keys));
    let idle_pool = tile_scheduler.texture_pool_mut();
    let snapshot_frame = renderer
        .snapshot_frame_when_idle(&camera, &viewport, &idle_frame, idle_pool, &[])
        .expect("an idle frame captures");
    let ordinary_frame = renderer
        .render_captured_frame(&camera, &viewport, &idle_frame, idle_pool)
        .expect("the ordinary capture renders the same frame");
    assert_eq!(
        snapshot_frame, ordinary_frame,
        "the gated snapshot must capture exactly what the ordinary capture renders"
    );
}

#[tokio::test]
async fn a_draw_turns_a_corrupt_payload_into_pending_work_again() {
    match WgpuGraphicsContextSource.request_graphics_context() {
        Ok(graphics_context) => {
            println!(
                "rendering on backend {:?} with adapter {:?}",
                graphics_context.adapter_information.backend,
                graphics_context.adapter_information.name
            );
            let mut renderer = HeadlessTileRenderer::with_graphics_context(graphics_context)
                .expect("the pipeline builds on the acquired context");
            exercise_eviction_flip_after_draw(&mut renderer).await;
        }
        Err(error) => {
            println!("skipping GPU idle-snapshot test: no graphics adapter available ({error})");
        }
    }
}

async fn exercise_eviction_flip_after_draw(renderer: &mut HeadlessTileRenderer) {
    let (camera, viewport, scene_tile_keys) = snapshot_scene();
    let mut tile_scheduler = TileScheduler::new(
        DecodableTileProvider::new(),
        TileTexturePool::new(SCENE_POOL_BUDGET_BYTES),
        RasterImageFormat::Png,
        RasterTileResolution::Standard,
        COMPLETE_FRAME_FETCH_BUDGET,
    );
    let idle_frame = tile_scheduler.prepare_frame(&scene_tile_keys).await;
    assert!(tile_scheduler.is_idle(&scene_tile_keys));

    // Corrupt one fetched payload's bytes (as if the platform returned bad
    // imagery): the pool still holds the tile, so the idle signal stays true
    // until a draw discovers the corruption.
    let corrupt_tile_key = scene_tile_keys[0];
    tile_scheduler
        .texture_pool_mut()
        .insert(corrupt_tile_key, CORRUPT_TILE_PAYLOAD_BYTES.to_vec());
    assert!(tile_scheduler.is_idle(&scene_tile_keys));

    let degraded_snapshot = renderer
        .snapshot_frame_when_idle(
            &camera,
            &viewport,
            &idle_frame,
            tile_scheduler.texture_pool_mut(),
            &[],
        )
        .expect("a frame whose payloads are all pooled still captures over corrupt bytes");
    assert_eq!(
        degraded_snapshot.rgba_bytes.len(),
        (degraded_snapshot.width_pixels * degraded_snapshot.height_pixels) as usize * 4,
        "the degraded frame is still a complete captured frame"
    );

    // The draw evicted the undecodable payload, so the same request set
    // flips back to pending until the next prepare frame refetches it.
    assert!(
        !tile_scheduler.is_idle(&scene_tile_keys),
        "the corrupt payload's eviction must turn the idle signal back to pending"
    );
    let post_draw_refusal = renderer
        .snapshot_frame_when_idle(
            &camera,
            &viewport,
            &idle_frame,
            tile_scheduler.texture_pool_mut(),
            &[],
        )
        .expect_err("the evicted payload's area is pending work again");
    match post_draw_refusal {
        HeadlessRenderError::PendingTileWork { pending_tile_count } => {
            assert_eq!(pending_tile_count, 1, "exactly the corrupt tile");
        }
        other => panic!("expected a pending-work refusal, got {other:?}"),
    }

    // The refetch resolves the frame again.
    let refetched_frame = tile_scheduler.prepare_frame(&scene_tile_keys).await;
    assert!(tile_scheduler.is_idle(&scene_tile_keys));
    assert!(
        renderer
            .snapshot_frame_when_idle(
                &camera,
                &viewport,
                &refetched_frame,
                tile_scheduler.texture_pool_mut(),
                &[],
            )
            .is_ok()
    );
}

/// A fallback frame never captures through the idle gate, even though the
/// ancestor's pixels would already cover the area: the child's own detail is
/// still owed. The ordinary capture renders it unchanged.
#[tokio::test]
async fn a_parent_fallback_frame_refuses_the_snapshot() {
    match WgpuGraphicsContextSource.request_graphics_context() {
        Ok(graphics_context) => {
            println!(
                "rendering on backend {:?} with adapter {:?}",
                graphics_context.adapter_information.backend,
                graphics_context.adapter_information.name
            );
            let mut renderer = HeadlessTileRenderer::with_graphics_context(graphics_context)
                .expect("the pipeline builds on the acquired context");
            exercise_fallback_refusal(&mut renderer);
        }
        Err(error) => {
            println!("skipping GPU idle-snapshot test: no graphics adapter available ({error})");
        }
    }
}

fn exercise_fallback_refusal(renderer: &mut HeadlessTileRenderer) {
    let (camera, viewport, scene_tile_keys) = snapshot_scene();
    let child_tile_key = scene_tile_keys[0];
    let parent_tile_key = child_tile_key
        .parent_tile_key()
        .expect("a non-root child has a parent");
    let mut payload_pool = TileTexturePool::new(SCENE_POOL_BUDGET_BYTES);
    payload_pool.insert(parent_tile_key, generated_tile_png(parent_tile_key));
    let fallback_frame = vec![PreparedTile {
        requested_tile_key: child_tile_key,
        readiness: TileReadiness::FallsBackTo {
            fallback_tile_key: parent_tile_key,
            zoom_level_difference: 1,
        },
    }];
    let refusal = renderer
        .snapshot_frame_when_idle(&camera, &viewport, &fallback_frame, &mut payload_pool, &[])
        .expect_err("a fallback frame must refuse the snapshot");
    match refusal {
        HeadlessRenderError::PendingTileWork { pending_tile_count } => {
            assert_eq!(pending_tile_count, 1);
        }
        other => panic!("expected a pending-work refusal, got {other:?}"),
    }
    // The parent fallback does render through the ordinary capture — the
    // gate decides only whether the frame will be deterministic.
    let fallback_capture = renderer
        .render_captured_frame(&camera, &viewport, &fallback_frame, &mut payload_pool)
        .expect("the fallback frame renders through the ordinary capture");
    assert_eq!(
        fallback_capture.rgba_bytes.len(),
        (fallback_capture.width_pixels * fallback_capture.height_pixels) as usize * 4,
        "the fallback frame is still a complete captured frame"
    );
}
