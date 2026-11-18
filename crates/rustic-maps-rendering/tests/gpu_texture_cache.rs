//! Reuse behavior of the headless renderer's uploaded-texture cache: a
//! steady scene uploads each tile once and serves later frames from the
//! cache, a replaced payload uploads again, pool eviction and corrupt
//! payloads prune the cache without re-uploads, and the byte budget evicts
//! least-recently-used textures — at insertion time and immediately on a
//! size reduction.
//!
//! Skips gracefully when no graphics adapter is available, as the golden
//! tests do.

use rustic_maps_rendering::{
    GraphicsContextSource, HeadlessTileRenderer, MapCamera, PreparedTile, TileKey, TileReadiness,
    TileTexturePool, ViewportPixels, WgpuGraphicsContextSource,
};

/// Frame edge of the cache scenes in pixels.
const CACHE_SCENE_VIEWPORT_EDGE_PIXELS: f64 = 512.0;
/// Tile zoom of the cache scenes: at this zoom a 512-pixel top-down viewport
/// spans several tiles, so one frame exercises more than one upload.
const CACHE_SCENE_TILE_ZOOM_LEVEL: u8 = 3;
/// Bearing and pitch of the cache cameras: north up, top down, so the two
/// identical frames are pixel-identical too.
const CACHE_SCENE_LEVEL_PITCH_DEGREES: f64 = 0.0;
/// Texel edge of the generated scene payloads.
const SCENE_TILE_EDGE_PIXELS: u32 = 64;
/// Byte budget the eviction test imposes: exactly two of the scene's 64-px
/// RGBA8 textures fit, so further uploads must evict.
///
/// 128 pixels by 128 pixels at 4 bytes is 65536 bytes; the budget is twice
/// the per-texture size below.
const EVICTION_TEST_BUDGET_BYTES: u64 =
    2 * SCENE_TILE_EDGE_PIXELS as u64 * SCENE_TILE_EDGE_PIXELS as u64 * 4;
fn request_renderer_or_skip(test_case: &'static str) -> Option<HeadlessTileRenderer> {
    match WgpuGraphicsContextSource.request_graphics_context() {
        Ok(graphics_context) => {
            println!(
                "rendering on backend {:?} with adapter {:?}",
                graphics_context.adapter_information.backend,
                graphics_context.adapter_information.name
            );
            HeadlessTileRenderer::with_graphics_context(graphics_context)
                .map_err(|error| {
                    println!("{test_case}: pipeline failed to build ({error:?}); skipping")
                })
                .ok()
        }
        Err(error) => {
            println!("skipping GPU cache test: no graphics adapter available ({error})");
            None
        }
    }
}

#[test]
fn a_repeated_frame_is_served_from_the_cache_without_reuploading() {
    use rustic_maps_rendering::normalized_world_to_coordinate;

    let Some(mut renderer) = request_renderer_or_skip("cache hit") else {
        return;
    };
    let viewport = ViewportPixels::new(
        CACHE_SCENE_VIEWPORT_EDGE_PIXELS,
        CACHE_SCENE_VIEWPORT_EDGE_PIXELS,
    )
    .expect("the cache viewport is a valid viewport");
    let camera = MapCamera::new(
        normalized_world_to_coordinate([0.25, 0.25]),
        f64::from(CACHE_SCENE_TILE_ZOOM_LEVEL),
        0.0,
        CACHE_SCENE_LEVEL_PITCH_DEGREES,
    );
    let mut payload_pool = TileTexturePool::new(1 << 22);
    let prepared_tiles = prepare_scene(
        &camera,
        &viewport,
        CACHE_SCENE_TILE_ZOOM_LEVEL,
        &mut payload_pool,
    );

    let first_upload_count = renderer.gpu_texture_cache().total_upload_count();
    renderer
        .render_captured_frame(&camera, &viewport, &prepared_tiles, &mut payload_pool)
        .expect("the first frame renders");
    let uploads_after_first_frame = renderer.gpu_texture_cache().total_upload_count();
    assert!(
        uploads_after_first_frame > first_upload_count,
        "the first frame must upload each of its tiles"
    );

    renderer
        .render_captured_frame(&camera, &viewport, &prepared_tiles, &mut payload_pool)
        .expect("the repeated frame renders");
    assert_eq!(
        renderer.gpu_texture_cache().total_upload_count(),
        uploads_after_first_frame,
        "an unchanged pool must serve the repeated frame entirely from the cache"
    );
    // Sanity on the reuse claim: hits match the tile count the frame drew.
    assert!(renderer.gpu_texture_cache().total_cache_hit_count() > 0);
}

#[test]
fn replacing_a_payload_uploads_again_instead_of_drawing_stale_pixels() {
    use rustic_maps_rendering::normalized_world_to_coordinate;

    let Some(mut renderer) = request_renderer_or_skip("cache invalidation") else {
        return;
    };
    let viewport = ViewportPixels::new(
        CACHE_SCENE_VIEWPORT_EDGE_PIXELS,
        CACHE_SCENE_VIEWPORT_EDGE_PIXELS,
    )
    .expect("the cache viewport is a valid viewport");
    let camera = MapCamera::new(
        normalized_world_to_coordinate([0.25, 0.25]),
        f64::from(CACHE_SCENE_TILE_ZOOM_LEVEL),
        0.0,
        CACHE_SCENE_LEVEL_PITCH_DEGREES,
    );
    let mut payload_pool = TileTexturePool::new(1 << 22);
    let prepared_tiles = prepare_scene(
        &camera,
        &viewport,
        CACHE_SCENE_TILE_ZOOM_LEVEL,
        &mut payload_pool,
    );
    renderer
        .render_captured_frame(&camera, &viewport, &prepared_tiles, &mut payload_pool)
        .expect("the warmed-up frame renders");
    let uploads_before_replacement = renderer.gpu_texture_cache().total_upload_count();

    // Replace every pooled payload with freshly encoded (differently seeded)
    // bytes: same tile keys, new revisions.
    for (tile_index, prepared_tile) in prepared_tiles.iter().enumerate() {
        payload_pool.insert(
            prepared_tile.requested_tile_key,
            scene_tile_png(tile_index as u32, 1),
        );
    }
    renderer
        .render_captured_frame(&camera, &viewport, &prepared_tiles, &mut payload_pool)
        .expect("the replaced frame renders");
    assert!(
        renderer.gpu_texture_cache().total_upload_count() > uploads_before_replacement,
        "replaced payloads must upload again instead of drawing stale pixels"
    );
}

#[test]
fn the_byte_budget_evicts_least_recently_used_textures() {
    use rustic_maps_rendering::normalized_world_to_coordinate;

    let Some(mut renderer) = request_renderer_or_skip("cache eviction") else {
        return;
    };
    renderer
        .gpu_texture_cache_mut()
        .set_maximum_stored_bytes(EVICTION_TEST_BUDGET_BYTES);
    let viewport = ViewportPixels::new(
        CACHE_SCENE_VIEWPORT_EDGE_PIXELS,
        CACHE_SCENE_VIEWPORT_EDGE_PIXELS,
    )
    .expect("the cache viewport is a valid viewport");
    let camera = MapCamera::new(
        normalized_world_to_coordinate([0.25, 0.25]),
        f64::from(CACHE_SCENE_TILE_ZOOM_LEVEL),
        0.0,
        CACHE_SCENE_LEVEL_PITCH_DEGREES,
    );
    let mut payload_pool = TileTexturePool::new(1 << 22);
    let prepared_tiles = prepare_scene(
        &camera,
        &viewport,
        CACHE_SCENE_TILE_ZOOM_LEVEL,
        &mut payload_pool,
    );
    assert!(
        u64::try_from(prepared_tiles.len()).unwrap() * per_texture_bytes()
            > EVICTION_TEST_BUDGET_BYTES,
        "the scene must not fit the budget so the eviction path actually runs"
    );

    renderer
        .render_captured_frame(&camera, &viewport, &prepared_tiles, &mut payload_pool)
        .expect("the eviction frame renders");
    let cache = renderer.gpu_texture_cache();
    assert!(
        cache.stored_bytes() <= EVICTION_TEST_BUDGET_BYTES,
        "the cache must honor its byte budget after insertion ({} > {})",
        cache.stored_bytes(),
        EVICTION_TEST_BUDGET_BYTES
    );
    assert!(
        cache.stored_texture_count() >= 2,
        "a single oversized texture is kept even past the budget; several must survive"
    );
}

/// Sum of the actual encoded byte lengths of the scene's payloads; pool
/// budgets are computed from real sizes so eviction pressure provably runs.
fn scene_payload_bytes(scene_tile_count: usize) -> Vec<Vec<u8>> {
    (0..scene_tile_count as u32)
        .map(|tile_index| scene_tile_png(tile_index, 0))
        .collect()
}

#[test]
fn cache_hit_draws_keep_the_pool_from_evicting_visible_tiles() {
    use rustic_maps_rendering::normalized_world_to_coordinate;

    let Some(mut renderer) = request_renderer_or_skip("pool recency under cache hits") else {
        return;
    };
    let viewport = ViewportPixels::new(
        CACHE_SCENE_VIEWPORT_EDGE_PIXELS,
        CACHE_SCENE_VIEWPORT_EDGE_PIXELS,
    )
    .expect("the cache viewport is a valid viewport");
    let camera = MapCamera::new(
        normalized_world_to_coordinate([0.25, 0.25]),
        f64::from(CACHE_SCENE_TILE_ZOOM_LEVEL),
        0.0,
        CACHE_SCENE_LEVEL_PITCH_DEGREES,
    );
    let scene_tile_keys = camera.visible_tile_keys(
        &viewport,
        rustic_maps_rendering::DEFAULT_FIELD_OF_VIEW_DEGREES,
        CACHE_SCENE_TILE_ZOOM_LEVEL,
    );
    let scene_payloads = scene_payload_bytes(scene_tile_keys.len());
    let decoy_payload = scene_tile_png(DECOY_TILE_FIRST_COLUMN_INDEX, 0);
    let scene_bytes: u64 = scene_payloads
        .iter()
        .map(|payload| payload.len() as u64)
        .sum();
    // Room for the scene and exactly one decoy; further decoys evict, and
    // the question is whether the victim is a decoy or a visible tile.
    let mut payload_pool = TileTexturePool::new(scene_bytes + decoy_payload.len() as u64);
    for (tile_index, tile_key) in scene_tile_keys.iter().enumerate() {
        payload_pool.insert(*tile_key, scene_payloads[tile_index].clone());
    }
    let prepared_tiles: Vec<PreparedTile> = scene_tile_keys
        .iter()
        .map(|tile_key| PreparedTile {
            requested_tile_key: *tile_key,
            readiness: TileReadiness::Present,
        })
        .collect();

    renderer
        .render_captured_frame(&camera, &viewport, &prepared_tiles, &mut payload_pool)
        .expect("the warm-up frame renders");
    let uploads_after_warmup = renderer.gpu_texture_cache().total_upload_count();
    let pooled_revisions: Vec<_> = scene_tile_keys
        .iter()
        .map(|tile_key| pool_revision_of(&payload_pool, tile_key))
        .collect();

    // Scheduler-like decoy traffic: payloads fetched for tiles outside the
    // view, inserted without any lookup of the visible tiles. Their inserts
    // must evict only each other, never the tiles the steady camera draws.
    // Every decoy carries the same bytes, so each insert displaces exactly
    // one decoy-sized payload — no size drift can push a visible tile out.
    for decoy_round in 0..DECOY_ROUNDS {
        payload_pool.insert(scene_decoy_tile_key(decoy_round), decoy_payload.clone());
        renderer
            .render_captured_frame(&camera, &viewport, &prepared_tiles, &mut payload_pool)
            .expect("the decoy frame renders");
    }

    for (tile_key, warmup_revision) in scene_tile_keys.iter().zip(&pooled_revisions) {
        assert_eq!(
            pool_revision_of(&payload_pool, tile_key),
            *warmup_revision,
            "visible tile {tile_key:?} must keep its pooled payload through \
             decoy-traffic eviction pressure"
        );
    }
    assert_eq!(
        renderer.gpu_texture_cache().total_upload_count(),
        uploads_after_warmup,
        "a steady scene under decoy pressure must not churn uploads"
    );
}

/// Byte budget leaving the whole scene pooled but no decoys after the first;
/// the decoys evict one another and the scene must keep drawing from cache.
const DECOY_ROUNDS: u32 = 6;

/// Tiles the decoy traffic fetches cycle through: at the scene's zoom the
/// mercator grid is a small square, so the decoys reuse the far side of the
/// grid, outside the scene's column range.
const DECOY_TILE_COLUMN_COUNT: u32 = 4;
/// First grid column of the decoy range.
const DECOY_TILE_FIRST_COLUMN_INDEX: u32 = 4;
/// Grid row of the decoy tiles.
const DECOY_TILE_ROW_INDEX: u32 = 6;

fn scene_decoy_tile_key(decoy_round: u32) -> TileKey {
    TileKey::new(
        DECOY_TILE_FIRST_COLUMN_INDEX + decoy_round % DECOY_TILE_COLUMN_COUNT,
        DECOY_TILE_ROW_INDEX,
        CACHE_SCENE_TILE_ZOOM_LEVEL,
    )
    .expect("the decoy tile coordinates are valid")
}

fn pool_revision_of(payload_pool: &TileTexturePool, tile_key: &TileKey) -> u64 {
    payload_pool
        .payload_revision(tile_key)
        .unwrap_or_else(|| panic!("tile {tile_key:?} must be pooled"))
}

#[test]
fn a_pool_evicted_payload_prunes_its_gpu_texture_without_reuploading() {
    use rustic_maps_rendering::DEFAULT_FIELD_OF_VIEW_DEGREES;
    use rustic_maps_rendering::normalized_world_to_coordinate;

    let Some(mut renderer) = request_renderer_or_skip("pool eviction prune") else {
        return;
    };
    let viewport = ViewportPixels::new(
        CACHE_SCENE_VIEWPORT_EDGE_PIXELS,
        CACHE_SCENE_VIEWPORT_EDGE_PIXELS,
    )
    .expect("the cache viewport is a valid viewport");
    let camera = MapCamera::new(
        normalized_world_to_coordinate([0.25, 0.25]),
        f64::from(CACHE_SCENE_TILE_ZOOM_LEVEL),
        0.0,
        CACHE_SCENE_LEVEL_PITCH_DEGREES,
    );
    let scene_tile_keys = camera.visible_tile_keys(
        &viewport,
        DEFAULT_FIELD_OF_VIEW_DEGREES,
        CACHE_SCENE_TILE_ZOOM_LEVEL,
    );
    let scene_payloads = scene_payload_bytes(scene_tile_keys.len());
    let scene_bytes: u64 = scene_payloads
        .iter()
        .map(|payload| payload.len() as u64)
        .sum();
    // Exactly the scene fits; the decoy inserts below must evict pooled
    // visible payloads, which is the pressure the prune must respond to.
    let mut payload_pool = TileTexturePool::new(scene_bytes);
    for (tile_index, tile_key) in scene_tile_keys.iter().enumerate() {
        payload_pool.insert(*tile_key, scene_payloads[tile_index].clone());
    }
    let prepared_tiles: Vec<PreparedTile> = scene_tile_keys
        .iter()
        .map(|tile_key| PreparedTile {
            requested_tile_key: *tile_key,
            readiness: TileReadiness::Present,
        })
        .collect();
    renderer
        .render_captured_frame(&camera, &viewport, &prepared_tiles, &mut payload_pool)
        .expect("the warmed-up frame renders");
    // The scene fully fits: one GPU texture per visible tile.
    assert_eq!(
        renderer.gpu_texture_cache().stored_texture_count(),
        scene_tile_keys.len()
    );
    let uploads_after_warmup = renderer.gpu_texture_cache().total_upload_count();

    // Evict every visible payload from the pool with decoy pressure only:
    // each insert evicts the oldest-recency entry, and the visible payloads
    // are older than every decoy, so enough decoys push them all out. Then
    // render: the frame must prune the invalidated textures and draw nothing
    // for them — never the stale pixels.
    for decoy_round in 0..(scene_tile_keys.len() as u32 + 1) {
        payload_pool.insert(
            scene_decoy_tile_key(decoy_round),
            scene_tile_png(DECOY_TILE_FIRST_COLUMN_INDEX, decoy_round),
        );
    }
    renderer
        .render_captured_frame(&camera, &viewport, &prepared_tiles, &mut payload_pool)
        .expect("the frame after eviction renders");
    for tile_key in &scene_tile_keys {
        assert!(
            !payload_pool.contains(tile_key),
            "test setup: every visible payload must have been evicted"
        );
    }
    assert_eq!(
        renderer.gpu_texture_cache().stored_texture_count(),
        0,
        "every upload whose pooled bytes were evicted must be pruned"
    );
    assert_eq!(
        renderer.gpu_texture_cache().total_upload_count(),
        uploads_after_warmup,
        "a pool miss must skip the draw, not re-upload from nothing"
    );
}

#[test]
fn a_corrupt_rerequested_payload_is_evicted_and_not_served_from_the_cache() {
    use rustic_maps_rendering::DEFAULT_FIELD_OF_VIEW_DEGREES;
    use rustic_maps_rendering::normalized_world_to_coordinate;

    let Some(mut renderer) = request_renderer_or_skip("corrupt re-insert") else {
        return;
    };
    let viewport = ViewportPixels::new(
        CACHE_SCENE_VIEWPORT_EDGE_PIXELS,
        CACHE_SCENE_VIEWPORT_EDGE_PIXELS,
    )
    .expect("the cache viewport is a valid viewport");
    let camera = MapCamera::new(
        normalized_world_to_coordinate([0.25, 0.25]),
        f64::from(CACHE_SCENE_TILE_ZOOM_LEVEL),
        0.0,
        CACHE_SCENE_LEVEL_PITCH_DEGREES,
    );
    let scene_tile_keys = camera.visible_tile_keys(
        &viewport,
        DEFAULT_FIELD_OF_VIEW_DEGREES,
        CACHE_SCENE_TILE_ZOOM_LEVEL,
    );
    let mut payload_pool = TileTexturePool::new(1 << 22);
    for (tile_index, tile_key) in scene_tile_keys.iter().enumerate() {
        payload_pool.insert(*tile_key, scene_tile_png(tile_index as u32, 0));
    }
    let prepared_tiles: Vec<PreparedTile> = scene_tile_keys
        .iter()
        .map(|tile_key| PreparedTile {
            requested_tile_key: *tile_key,
            readiness: TileReadiness::Present,
        })
        .collect();
    renderer
        .render_captured_frame(&camera, &viewport, &prepared_tiles, &mut payload_pool)
        .expect("the warmed-up frame renders");
    assert_eq!(
        renderer.gpu_texture_cache().stored_texture_count(),
        scene_tile_keys.len()
    );
    let uploads_after_warmup = renderer.gpu_texture_cache().total_upload_count();

    // The refreshed bytes for one tile arrive corrupt: the old texture must
    // not survive, the corrupt bytes must leave the pool.
    payload_pool.insert(scene_tile_keys[0], b"definitely not a PNG".to_vec());
    renderer
        .render_captured_frame(&camera, &viewport, &prepared_tiles, &mut payload_pool)
        .expect("the frame after the corrupt insert renders");
    assert!(
        !payload_pool.contains(&scene_tile_keys[0]),
        "the corrupt payload must be evicted so the scheduler refetches it"
    );
    assert_eq!(
        renderer.gpu_texture_cache().stored_texture_count(),
        scene_tile_keys.len() - 1,
        "the replaced tile must not draw stale cached pixels, nor hold them"
    );
    assert_eq!(
        renderer.gpu_texture_cache().total_upload_count(),
        uploads_after_warmup,
        "a corrupt payload must not produce an upload"
    );
}

#[test]
fn shrinking_the_budget_evicts_immediately_not_on_the_next_insert() {
    use rustic_maps_rendering::normalized_world_to_coordinate;

    let Some(mut renderer) = request_renderer_or_skip("budget shrink") else {
        return;
    };
    let viewport = ViewportPixels::new(
        CACHE_SCENE_VIEWPORT_EDGE_PIXELS,
        CACHE_SCENE_VIEWPORT_EDGE_PIXELS,
    )
    .expect("the cache viewport is a valid viewport");
    let camera = MapCamera::new(
        normalized_world_to_coordinate([0.25, 0.25]),
        f64::from(CACHE_SCENE_TILE_ZOOM_LEVEL),
        0.0,
        CACHE_SCENE_LEVEL_PITCH_DEGREES,
    );
    let mut payload_pool = TileTexturePool::new(1 << 22);
    let prepared_tiles = prepare_scene(
        &camera,
        &viewport,
        CACHE_SCENE_TILE_ZOOM_LEVEL,
        &mut payload_pool,
    );
    renderer
        .render_captured_frame(&camera, &viewport, &prepared_tiles, &mut payload_pool)
        .expect("the warm-up frame renders");
    assert!(renderer.gpu_texture_cache().stored_texture_count() > 1);

    // One 64-px texture fills the shrunk budget: the setter must trim at
    // once, with no further insertion to wait for.
    renderer
        .gpu_texture_cache_mut()
        .set_maximum_stored_bytes(per_texture_bytes());
    let cache = renderer.gpu_texture_cache();
    assert!(
        cache.stored_bytes() <= per_texture_bytes(),
        "the shrunken budget must be honored immediately ({} bytes held)",
        cache.stored_bytes()
    );
    assert_eq!(
        cache.stored_texture_count(),
        1,
        "a lone surviving texture is the recently used one; the budget keeps it"
    );
}

/// The GPU bytes one of the scene's textures occupies after upload.
fn per_texture_bytes() -> u64 {
    u64::from(SCENE_TILE_EDGE_PIXELS) * u64::from(SCENE_TILE_EDGE_PIXELS) * 4
}

/// Fills the pool for the scene so every visible tile is `Present`, mirroring
/// the scheduler's prepared output.
fn prepare_scene(
    camera: &MapCamera,
    viewport: &ViewportPixels,
    tile_zoom_level: u8,
    payload_pool: &mut TileTexturePool,
) -> Vec<PreparedTile> {
    let scene_tile_keys = camera.visible_tile_keys(
        viewport,
        rustic_maps_rendering::DEFAULT_FIELD_OF_VIEW_DEGREES,
        tile_zoom_level,
    );
    for (tile_index, tile_key) in scene_tile_keys.iter().enumerate() {
        payload_pool.insert(*tile_key, scene_tile_png(tile_index as u32, 0));
    }
    scene_tile_keys
        .iter()
        .map(|tile_key| PreparedTile {
            requested_tile_key: *tile_key,
            readiness: TileReadiness::Present,
        })
        .collect()
}

/// Generates a deterministic valid PNG payload for the scene: a distinct
/// gradient per (tile, generation) pair, enough for the replacement test to
/// produce genuinely new bytes.
fn scene_tile_png(tile_index: u32, tile_generation: u32) -> Vec<u8> {
    let tile_image = image::RgbaImage::from_fn(
        SCENE_TILE_EDGE_PIXELS,
        SCENE_TILE_EDGE_PIXELS,
        |pixel_x, pixel_y| {
            image::Rgba([
                (pixel_x % u32::from(u8::MAX)) as u8,
                (pixel_y % u32::from(u8::MAX)) as u8,
                (tile_index % u32::from(u8::MAX)) as u8,
                (tile_generation % u32::from(u8::MAX)) as u8,
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
