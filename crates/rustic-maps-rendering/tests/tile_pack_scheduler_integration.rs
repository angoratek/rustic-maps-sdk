//! The tile-pack scheduler seam (PRD FR-D/FR-F): tiles a
//! `TilePackManager` prepared must reach the scheduler through the very
//! same provider slot as ordinary cached network tiles — a pack tile and a
//! cache tile are indistinguishable to the scheduler, which needs no
//! integration at all.

use async_trait::async_trait;
use geospatial_traits::RasterTileProvider;
use geospatial_types::{
    BoundingBox, GeographicCoordinate, RasterImageFormat, RasterTileData, RasterTileResolution,
    SDKError,
};
use rustic_maps_cache::{
    CachedRasterTileProvider, SqliteResponseCache, SqliteResponseCacheConfiguration,
    TilePackDescription,
};
use rustic_maps_rendering::tile_scheduler::TileReadiness;
use rustic_maps_rendering::{TileKey, TileScheduler, tile_texture_pool::TileTexturePool};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// The namespace the cached wrapper and the pack manager share here.
const TEST_TILE_SOURCE_NAMESPACE: &str = "test-provider";

fn coordinate(latitude: f64, longitude: f64) -> GeographicCoordinate {
    GeographicCoordinate::from_degrees(latitude, longitude).unwrap()
}

/// Region spanning two zoom-3 tile columns around tile (1, 3, 3).
fn two_tile_region() -> BoundingBox {
    BoundingBox::from_corners(coordinate(38.57, -120.0), coordinate(38.58, -85.0)).unwrap()
}

/// Network-free provider double recording every request it answers.
struct ScriptedTileProvider {
    scripted_tiles: Mutex<HashMap<(u32, u32, u8), Vec<u8>>>,
    requested_tiles: Mutex<Vec<(u32, u32, u8)>>,
}

impl ScriptedTileProvider {
    fn serving_tiles(
        served_tiles: impl IntoIterator<Item = ((u32, u32, u8), Vec<u8>)>,
    ) -> Arc<Self> {
        Arc::new(Self {
            scripted_tiles: Mutex::new(served_tiles.into_iter().collect()),
            requested_tiles: Mutex::new(Vec::new()),
        })
    }

    fn recorded_requests(&self) -> Vec<(u32, u32, u8)> {
        self.requested_tiles.lock().unwrap().clone()
    }
}

#[async_trait]
impl RasterTileProvider for ScriptedTileProvider {
    async fn fetch_raster_tile(
        &self,
        tile_x: u32,
        tile_y: u32,
        zoom_level: u8,
        _image_format: RasterImageFormat,
        _tile_resolution: RasterTileResolution,
    ) -> Result<RasterTileData, SDKError> {
        self.requested_tiles
            .lock()
            .unwrap()
            .push((tile_x, tile_y, zoom_level));
        let scripted_payload = self
            .scripted_tiles
            .lock()
            .unwrap()
            .get(&(tile_x, tile_y, zoom_level))
            .cloned();
        match scripted_payload {
            Some(image_bytes) => Ok(RasterTileData {
                tile_x,
                tile_y,
                zoom_level,
                image_format: RasterImageFormat::Png,
                tile_resolution: RasterTileResolution::Standard,
                image_bytes,
            }),
            None => Err(SDKError::ResponseParsingFailure(
                "tile not scripted".to_string(),
            )),
        }
    }
}

fn tile_key(tile_x: u32, tile_y: u32, zoom_level: u8) -> TileKey {
    TileKey::new(tile_x, tile_y, zoom_level).unwrap()
}

#[tokio::test]
async fn a_prepared_pack_serves_its_tiles_through_the_same_cached_scheduler_path() {
    let pack_tile_a = (1, 3, 3);
    let pack_tile_b = (2, 3, 3);
    // A tile inside no pack: it exercises the ordinary cache-then-network
    // side of the same provider slot.
    let outside_tile = (6, 6, 3);
    let scripted_tile_provider = ScriptedTileProvider::serving_tiles(
        [pack_tile_a, pack_tile_b, outside_tile].map(|tile_address| (tile_address, vec![7; 64])),
    );
    let database_path = std::env::temp_dir().join(format!(
        "rustic-maps-rendering-pack-scheduler-{}.sqlite",
        std::process::id()
    ));
    std::fs::remove_file(&database_path).ok();
    let response_cache = SqliteResponseCache::open(SqliteResponseCacheConfiguration {
        database_path,
        ..SqliteResponseCacheConfiguration::default()
    })
    .unwrap();
    let cached_raster_tile_provider = Arc::new(CachedRasterTileProvider::new(
        scripted_tile_provider.clone(),
        response_cache.clone(),
        TEST_TILE_SOURCE_NAMESPACE,
    ));
    let tile_pack_manager = rustic_maps_cache::TilePackManager::new(
        cached_raster_tile_provider.clone(),
        response_cache.clone(),
        None,
    );
    let pack_record = tile_pack_manager
        .prepare_tile_pack(
            &TilePackDescription {
                pack_id: "sacramento-west".to_string(),
                display_name: "Sacramento west".to_string(),
                region_bounding_box: two_tile_region(),
                minimum_zoom_level: 3,
                maximum_zoom_level: 3,
                image_format: RasterImageFormat::Png,
                tile_resolution: RasterTileResolution::Standard,
                requested_time_to_live_seconds: None,
            },
            &mut |_| {},
        )
        .await
        .unwrap();
    assert_eq!(pack_record.present_tile_count, 2);
    // The prepared tile is a live store entry under the scheme's own
    // fingerprint — the same key the scheduler's provider slot reads.
    let pack_tile_fingerprint = rustic_maps_cache::cached_providers::tile_request_descriptor(
        TEST_TILE_SOURCE_NAMESPACE,
        1,
        3,
        3,
        RasterImageFormat::Png,
        RasterTileResolution::Standard,
    )
    .to_fingerprint();
    let prepare_time_lookup = <SqliteResponseCache as rustic_maps_core::CachedResponseStore<
        RasterTileData,
    >>::lookup_response(
        response_cache.as_ref(), &pack_tile_fingerprint
    )
    .unwrap();
    assert!(
        prepare_time_lookup.is_some(),
        "the prepared tile is a live store entry"
    );

    // The renderer's scheduler over the same registered cached provider:
    // the pack tiles resolve from the pool after one frame, without the
    // wrapped provider answering them again, exactly like the network tile
    // the scheduler fetched and cached itself.
    let mut tile_scheduler = TileScheduler::new(
        cached_raster_tile_provider,
        TileTexturePool::new(1 << 20),
        RasterImageFormat::Png,
        RasterTileResolution::Standard,
        8,
    );
    let requests_before_frame = scripted_tile_provider.recorded_requests();
    assert_eq!(
        requests_before_frame,
        vec![pack_tile_a, pack_tile_b],
        "the pack download's own fetches; the scheduler starts after them"
    );
    let first_frame = tile_scheduler
        .prepare_frame(&[tile_key(1, 3, 3), tile_key(2, 3, 3), tile_key(6, 6, 3)])
        .await;
    for prepared_tile in &first_frame {
        assert_eq!(
            prepared_tile.readiness,
            TileReadiness::Present,
            "pack tile and cached network tile resolve identically: {:?}",
            prepared_tile.requested_tile_key
        );
    }
    assert!(tile_scheduler.is_idle(&[tile_key(1, 3, 3), tile_key(2, 3, 3), tile_key(6, 6, 3)]));
    // One further provider request: the outside tile's cache miss. The pack
    // tiles were served by the store the scheduler's provider slot reads —
    // the FR-D indistinguishability, with zero scheduler changes — so no
    // pack tile is requested again.
    assert_eq!(
        scripted_tile_provider.recorded_requests(),
        vec![pack_tile_a, pack_tile_b, outside_tile]
    );
}
