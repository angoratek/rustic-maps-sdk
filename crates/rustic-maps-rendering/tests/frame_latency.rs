//! Frame-latency evidence for the round-10 never-await-the-network split.
//!
//! A scripted provider answers every fetch after a fixed latency, so the
//! elapsed wall time of a frame resolution directly exposes how many fetches
//! the frame path awaited serially: `N` misses at `L` latency must cost
//! roughly `(misses - 1) * L` while the fetches run back-to-back. Recording
//! the numbers in test output is the round-10 profiling evidence; the
//! assertions are the structural ones (lower bound for the serial path, an
//! upper bound on a resolution that must not touch the network) so the suite
//! stays stable on a slow CI host.
//!
//! The numbers use an artificial latency because a real host's timings fold
//! machine speed into the measurement; the serial-vs-parallel structure and
//! the resolve-without-blocking property are what the round's fix changed.

use async_trait::async_trait;
use geospatial_traits::RasterTileProvider;
use geospatial_types::{RasterImageFormat, RasterTileData, RasterTileResolution, SDKError};
use rustic_maps_rendering::tile_key::TileKey;
use rustic_maps_rendering::tile_scheduler::{
    PreparedTile, TILE_FETCH_CONCURRENCY, TileReadiness, TileScheduler,
};
use rustic_maps_rendering::tile_texture_pool::TileTexturePool;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::time::sleep;

/// Per-fetch latency the scripted provider answers after. Large enough that
/// scheduler-side overhead vanishes under it.
const SCRIPTED_FETCH_LATENCY: Duration = Duration::from_millis(40);

/// Side length of the synthetic visible grid: 25 distinct tiles, comfortably
/// under the per-frame budget so every miss fetches in one frame.
const FRAME_GRID_SIDE: u32 = 5;

/// Zoom level of the synthetic grid, as in the recorded Sacramento demo imagery.
const FRAME_GRID_ZOOM_LEVEL: u8 = 14;

/// Base x/y of the grid, arbitrary but fixed for determinism.
const FRAME_GRID_BASE_X: u32 = 8_724;
/// See [`FRAME_GRID_BASE_X`].
const FRAME_GRID_BASE_Y: u32 = 5_736;

/// The tile payload: never decoded on the scheduler path, so bytes suffice.
const SCRIPTED_PAYLOAD_BYTES: usize = 8_192;

/// Upper bound for a frame resolution that must not touch the network.
const RESOLVE_MAXIMUM_MILLISECONDS: u128 = 50;

/// A provider that answers every scripted tile after a fixed latency.
struct ScheduledLatencyTileProvider {
    served_tiles: Mutex<HashMap<TileKey, ()>>,
    fetch_count: Mutex<usize>,
    fetches_in_flight: AtomicUsize,
    maximum_fetches_in_flight: AtomicUsize,
}

impl ScheduledLatencyTileProvider {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            served_tiles: Mutex::new(HashMap::new()),
            fetch_count: Mutex::new(0),
            fetches_in_flight: AtomicUsize::new(0),
            maximum_fetches_in_flight: AtomicUsize::new(0),
        })
    }

    fn fetch_count(&self) -> usize {
        *self.fetch_count.lock().unwrap()
    }

    /// The most provider calls the frame path kept in flight at once: the
    /// direct observation of the fetch pool's concurrency.
    fn maximum_fetches_in_flight(&self) -> usize {
        self.maximum_fetches_in_flight.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl RasterTileProvider for ScheduledLatencyTileProvider {
    async fn fetch_raster_tile(
        &self,
        tile_x: u32,
        tile_y: u32,
        zoom_level: u8,
        _image_format: RasterImageFormat,
        _tile_resolution: RasterTileResolution,
    ) -> Result<RasterTileData, SDKError> {
        let tile_key = TileKey::new(tile_x, tile_y, zoom_level).unwrap();
        {
            let mut served_tiles = self.served_tiles.lock().unwrap();
            if served_tiles.insert(tile_key, ()).is_none() {
                *self.fetch_count.lock().unwrap() += 1;
            }
        }
        let in_flight = self.fetches_in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.maximum_fetches_in_flight
            .fetch_max(in_flight, Ordering::SeqCst);
        sleep(SCRIPTED_FETCH_LATENCY).await;
        self.fetches_in_flight.fetch_sub(1, Ordering::SeqCst);
        Ok(RasterTileData {
            tile_x,
            tile_y,
            zoom_level,
            image_format: RasterImageFormat::Png,
            tile_resolution: RasterTileResolution::Standard,
            image_bytes: vec![0x5a; SCRIPTED_PAYLOAD_BYTES],
        })
    }
}

fn frame_grid_tiles() -> Vec<TileKey> {
    let mut tiles = Vec::new();
    for tile_y in 0..FRAME_GRID_SIDE {
        for tile_x in 0..FRAME_GRID_SIDE {
            tiles.push(
                TileKey::new(
                    FRAME_GRID_BASE_X + tile_x,
                    FRAME_GRID_BASE_Y + tile_y,
                    FRAME_GRID_ZOOM_LEVEL,
                )
                .unwrap(),
            );
        }
    }
    tiles
}

fn build_scheduler(provider: Arc<ScheduledLatencyTileProvider>) -> TileScheduler {
    // The same budget the surface view configures, so the measurement is
    // representative of the shipped frame path.
    TileScheduler::new(
        provider,
        TileTexturePool::new(128 << 20),
        RasterImageFormat::Png,
        RasterTileResolution::Standard,
        32,
    )
}

fn count_readiness(prepared_tiles: &[PreparedTile], readiness: TileReadiness) -> usize {
    prepared_tiles
        .iter()
        .filter(|prepared_tile| prepared_tile.readiness == readiness)
        .count()
}

/// The blocking frame path fetches its misses through the bounded pool: a cold
/// frame over a fully missed grid costs roughly `ceil(misses / concurrency)`
/// waves instead of one latency per miss.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cold_blocking_frame_fetches_its_misses_in_parallel_waves() {
    let provider = ScheduledLatencyTileProvider::new();
    let mut scheduler = build_scheduler(provider.clone());
    let frame_grid_tiles = frame_grid_tiles();

    let cold_start = Instant::now();
    let cold_frame = scheduler.prepare_frame(&frame_grid_tiles).await;
    let cold_elapsed = cold_start.elapsed();

    assert_eq!(
        provider.fetch_count(),
        frame_grid_tiles.len(),
        "a cold frame fetches every grid tile"
    );
    assert_eq!(
        cold_frame.len(),
        frame_grid_tiles.len(),
        "the returned frame covers every request, duplicates included"
    );
    assert_eq!(
        count_readiness(&cold_frame, TileReadiness::Present),
        frame_grid_tiles.len(),
        "after the blocking fetches the cold frame is fully present"
    );

    // The pooled measurement: 25 misses at eight in flight answer in four
    // waves, and the provider observes exactly the pool's window.
    let serial_floor =
        frame_grid_tiles.len().saturating_sub(1) as u128 * SCRIPTED_FETCH_LATENCY.as_millis();
    let wave_count = frame_grid_tiles.len().div_ceil(TILE_FETCH_CONCURRENCY);
    let pooled_floor = (wave_count - 1) as u128 * SCRIPTED_FETCH_LATENCY.as_millis();
    println!(
        "cold prepare_frame: {:.1?} for {} misses at {:.0?}/fetch in {} waves (serial floor {} ms)",
        cold_elapsed,
        frame_grid_tiles.len(),
        SCRIPTED_FETCH_LATENCY,
        wave_count,
        serial_floor
    );
    assert_eq!(
        provider.maximum_fetches_in_flight(),
        TILE_FETCH_CONCURRENCY,
        "the cold frame fills the fetch pool's window"
    );
    assert!(
        cold_elapsed.as_millis() >= pooled_floor,
        "the cold frame still awaits its waves (elapsed {:#?}, floor {} ms)",
        cold_elapsed,
        pooled_floor
    );
    assert!(
        cold_elapsed.as_millis() < serial_floor,
        "the cold frame must not await its fetches back-to-back (elapsed {:#?}, serial floor {} ms)",
        cold_elapsed,
        serial_floor
    );

    // A warm repeat of the same frame must not refetch anything: the whole
    // benefit of the pool on the blocking path.
    let warm_start = Instant::now();
    scheduler.prepare_frame(&frame_grid_tiles).await;
    let warm_elapsed = warm_start.elapsed();
    assert_eq!(
        provider.fetch_count(),
        frame_grid_tiles.len(),
        "the warm frame fetches nothing"
    );
    println!("warm prepare_frame: {:.1?}", warm_elapsed);
    assert!(
        warm_elapsed.as_millis() < RESOLVE_MAXIMUM_MILLISECONDS,
        "the warm frame must stay off the network (elapsed {:#?})",
        warm_elapsed
    );
}

/// A partially cached frame blocks on only its misses, through the same pool —
/// the pan/zoom shape that used to cost one latency per new tile.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn partially_cached_blocking_frame_fetches_its_misses_in_parallel() {
    let provider = ScheduledLatencyTileProvider::new();
    let mut scheduler = build_scheduler(provider.clone());
    let frame_grid_tiles = frame_grid_tiles();

    // Half a viewport already pooled (a previous frame's fetches); the pan
    // reveals the other half.
    let cached_tiles = &frame_grid_tiles[..frame_grid_tiles.len() / 2];
    let mut pool = TileTexturePool::new(128 << 20);
    for tile_key in cached_tiles {
        pool.insert(*tile_key, vec![0x5a; SCRIPTED_PAYLOAD_BYTES]);
    }
    *scheduler.texture_pool_mut() = pool;

    let misses: Vec<TileKey> = frame_grid_tiles
        .iter()
        .filter(|tile_key| !scheduler.texture_pool().contains(tile_key))
        .copied()
        .collect();
    let miss_count = misses.len();

    let pan_start = Instant::now();
    scheduler.prepare_frame(&frame_grid_tiles).await;
    let pan_elapsed = pan_start.elapsed();

    assert_eq!(
        provider.fetch_count(),
        miss_count,
        "the pan fetches exactly the misses"
    );
    let serial_floor = miss_count.saturating_sub(1) as u128 * SCRIPTED_FETCH_LATENCY.as_millis();
    let wave_count = miss_count.div_ceil(TILE_FETCH_CONCURRENCY);
    let pooled_floor = (wave_count - 1) as u128 * SCRIPTED_FETCH_LATENCY.as_millis();
    println!(
        "partially cached prepare_frame: {:#?} for {} misses at {:.0?}/fetch in {} waves (serial floor {} ms)",
        pan_elapsed, miss_count, SCRIPTED_FETCH_LATENCY, wave_count, serial_floor
    );
    assert_eq!(
        provider.maximum_fetches_in_flight(),
        TILE_FETCH_CONCURRENCY.min(miss_count),
        "the pan fills the fetch pool's window with its misses"
    );
    assert!(
        pan_elapsed.as_millis() >= pooled_floor,
        "the pan frame still awaits its waves (elapsed {:#?}, floor {} ms)",
        pan_elapsed,
        pooled_floor
    );
    assert!(
        pan_elapsed.as_millis() < serial_floor,
        "the pan frame must not await its misses back-to-back (elapsed {:#?}, serial floor {} ms)",
        pan_elapsed,
        serial_floor
    );
}

/// The split path's structural guarantees, measured: `resolve_frame` alone
/// never blocks on the scripted latency (even over a fully missed grid), the
/// detached warm fetch proceeds without the frame awaiting it, and warmed
/// detail arrives through the next resolve. The assertions are the
/// structural bounds (the resolve upper bound, the eventual-presence
/// property); the printed numbers are the round's AFTER evidence against the
/// blocking path's serial floors above.
mod split_path {
    use super::{
        FRAME_GRID_BASE_X, FRAME_GRID_BASE_Y, FRAME_GRID_ZOOM_LEVEL, RESOLVE_MAXIMUM_MILLISECONDS,
        SCRIPTED_FETCH_LATENCY, ScheduledLatencyTileProvider, build_scheduler, count_readiness,
        frame_grid_tiles,
    };
    use geospatial_types::RasterImageFormat;
    use geospatial_types::RasterTileResolution;
    use rustic_maps_rendering::TilePrefetchPlanner;
    use rustic_maps_rendering::camera::{DEFAULT_FIELD_OF_VIEW_DEGREES, MapCamera, ViewportPixels};
    use rustic_maps_rendering::projection::normalized_world_to_coordinate;
    use rustic_maps_rendering::tile_key::TileKey;
    use rustic_maps_rendering::tile_scheduler::{TileReadiness, TileScheduler};
    use rustic_maps_rendering::tile_texture_pool::TileTexturePool;
    use std::time::{Duration, Instant};
    use tokio::time::sleep;

    /// The test viewport: one square viewport over the synthetic grid.
    const TEST_VIEWPORT_EDGE_PIXELS: f64 = 512.0;

    /// How many resolve ticks the warm-run completion waits take at most.
    const WARM_COMPLETION_WAIT_TICKS: usize = 200;
    /// The completion-wait tick's duration: must cover one scripted fetch.
    const WARM_COMPLETION_WAIT_TICK: Duration = Duration::from_millis(10);

    fn test_viewport() -> ViewportPixels {
        ViewportPixels::new(TEST_VIEWPORT_EDGE_PIXELS, TEST_VIEWPORT_EDGE_PIXELS)
            .expect("the test viewport is valid")
    }

    fn grid_center_camera() -> MapCamera {
        let world_span = f64::from(1_u32 << FRAME_GRID_ZOOM_LEVEL);
        let grid_center_world = [
            (f64::from(FRAME_GRID_BASE_X) + 2.5) / world_span,
            (f64::from(FRAME_GRID_BASE_Y) + 2.5) / world_span,
        ];
        MapCamera::new(
            normalized_world_to_coordinate(grid_center_world),
            f64::from(FRAME_GRID_ZOOM_LEVEL),
            0.0,
            0.0,
        )
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_resolve_answers_off_the_network_even_over_a_fully_missed_grid() {
        let provider = ScheduledLatencyTileProvider::new();
        let mut scheduler = TileScheduler::new(
            provider,
            TileTexturePool::new(128 << 20),
            RasterImageFormat::Png,
            RasterTileResolution::Standard,
            32,
        );
        let frame_grid_tiles = frame_grid_tiles();
        let resolve_start = Instant::now();
        let resolved_frame = scheduler.resolve_frame(&frame_grid_tiles);
        let resolve_elapsed = resolve_start.elapsed();
        assert_eq!(
            count_readiness(&resolved_frame, TileReadiness::Unavailable),
            frame_grid_tiles.len(),
            "an empty pool resolves every request without fetching"
        );
        println!(
            "AFTER resolve_frame over {} misses: {:#?} (bound {} ms)",
            frame_grid_tiles.len(),
            resolve_elapsed,
            RESOLVE_MAXIMUM_MILLISECONDS
        );
        assert!(
            resolve_elapsed.as_millis() < RESOLVE_MAXIMUM_MILLISECONDS,
            "the resolve must not touch the network (elapsed {:#?})",
            resolve_elapsed
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_split_frame_resolves_while_the_detached_warm_runs() {
        let provider = ScheduledLatencyTileProvider::new();
        let mut scheduler = build_scheduler(provider.clone());
        let frame_grid_tiles = frame_grid_tiles();

        // Frame N: resolve (off the network), plan the warm, spawn it.
        let resolve_start = Instant::now();
        let resolved_frame = scheduler.resolve_frame(&frame_grid_tiles);
        let resolve_elapsed = resolve_start.elapsed();
        assert_eq!(
            count_readiness(&resolved_frame, TileReadiness::Unavailable),
            frame_grid_tiles.len(),
            "the first resolve answers from the empty pool"
        );
        let warm_task = scheduler.plan_warm_tiles(&frame_grid_tiles);
        let planned_fetch_count = warm_task.fetch_count();
        assert_eq!(planned_fetch_count, frame_grid_tiles.len());
        let warm_task_join = tokio::spawn(warm_task.run());
        println!(
            "AFTER split frame N: resolve_frame {:#?} (bound {} ms) while {} warm fetches run detached at {:.0?}/fetch",
            resolve_elapsed,
            RESOLVE_MAXIMUM_MILLISECONDS,
            planned_fetch_count,
            SCRIPTED_FETCH_LATENCY
        );
        assert!(
            resolve_elapsed.as_millis() < RESOLVE_MAXIMUM_MILLISECONDS,
            "the frame path must not await the warm fetches (elapsed {:#?})",
            resolve_elapsed
        );
        // While the warm runs, the frame's verdicts stay resolved (the
        // warming marks report Deferred, a pooled truth the draw used).
        let frame_n_start = Instant::now();
        let frame_n = scheduler.resolve_frame(&frame_grid_tiles);
        assert!(
            frame_n_start.elapsed().as_millis() < RESOLVE_MAXIMUM_MILLISECONDS,
            "the warm in flight must not block the next resolve either"
        );
        assert!(
            frame_n
                .iter()
                .any(|prepared_tile| prepared_tile.readiness == TileReadiness::Deferred),
            "tiles with a warm fetch in flight report the in-flight state"
        );

        // Frame N+1 (after the warm's deliveries drained): every requested
        // tile is present at its own zoom.
        let mut warm_join_completed = false;
        for _ in 0..WARM_COMPLETION_WAIT_TICKS {
            if warm_task_join.is_finished() {
                warm_join_completed = true;
                break;
            }
            sleep(WARM_COMPLETION_WAIT_TICK).await;
        }
        assert!(
            warm_join_completed,
            "the detached warm run must complete within the wait window of {} ms",
            WARM_COMPLETION_WAIT_TICKS as u128 * WARM_COMPLETION_WAIT_TICK.as_millis()
        );
        warm_task_join
            .await
            .expect("the scripted warm run never panics");
        let frame_n_plus_one_start = Instant::now();
        let frame_n_plus_one = scheduler.resolve_frame(&frame_grid_tiles);
        println!(
            "AFTER split frame N+1 ({} delivered): resolve_frame {:#?} (bound {} ms)",
            scheduler.delivered_warm_tile_count(),
            frame_n_plus_one_start.elapsed(),
            RESOLVE_MAXIMUM_MILLISECONDS
        );
        assert!(frame_n_plus_one_start.elapsed().as_millis() < RESOLVE_MAXIMUM_MILLISECONDS);
        assert_eq!(
            count_readiness(&frame_n_plus_one, TileReadiness::Present),
            frame_grid_tiles.len()
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_prefetched_ring_keeps_the_next_pose_off_the_network() {
        let provider = ScheduledLatencyTileProvider::new();
        let mut scheduler = build_scheduler(provider.clone());
        let grid_center_camera = grid_center_camera();
        let viewport = test_viewport();
        let prefetch_planner = TilePrefetchPlanner::new();
        let prefetch_request_tiles = prefetch_planner.prefetch_request_tiles(
            &grid_center_camera,
            &viewport,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
            None,
            scheduler.texture_pool(),
            &scheduler.warming_tile_keys(),
        );
        assert!(!prefetch_request_tiles.is_empty(), "the planner plans");
        let warm_fetch_count = scheduler.warm_tiles(&prefetch_request_tiles).await;
        println!(
            "AFTER prefetch warm: {} fetches for the camera pose's request list (planned {})",
            warm_fetch_count,
            prefetch_request_tiles.len()
        );
        assert!(warm_fetch_count > 0);
        let fetch_count_after_warm = provider.fetch_count();

        // The next pose (the ring's own center, inside the warmed ahead
        // ring's visible set): resolves fully present, with no further
        // fetch, within the resolve bound.
        let ahead_camera_center = grid_center_camera
            .screen_to_ground_coordinate(
                [
                    TEST_VIEWPORT_EDGE_PIXELS * 0.5,
                    TEST_VIEWPORT_EDGE_PIXELS * (0.5 - rustic_maps_rendering::PREFETCH_RING_TILES),
                ],
                &viewport,
                DEFAULT_FIELD_OF_VIEW_DEGREES,
            )
            .expect("the ring center's ray reaches the ground at this pitch");
        let ahead_camera = MapCamera::new(
            ahead_camera_center,
            f64::from(FRAME_GRID_ZOOM_LEVEL),
            0.0,
            0.0,
        );
        let ahead_visible_tiles: Vec<TileKey> = ahead_camera.visible_tile_keys(
            &viewport,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
            FRAME_GRID_ZOOM_LEVEL,
        );
        assert!(!ahead_visible_tiles.is_empty());
        let next_pose_start = Instant::now();
        let next_pose_frame = scheduler.resolve_frame(&ahead_visible_tiles);
        let next_pose_elapsed = next_pose_start.elapsed();
        println!(
            "AFTER prefetch: the next pose's frame of {} tiles resolved {:#?} (bound {} ms), fetches: {} -> {}",
            ahead_visible_tiles.len(),
            next_pose_elapsed,
            RESOLVE_MAXIMUM_MILLISECONDS,
            fetch_count_after_warm,
            provider.fetch_count()
        );
        assert!(
            next_pose_elapsed.as_millis() < RESOLVE_MAXIMUM_MILLISECONDS,
            "the warmed next pose must stay off the network (elapsed {:#?})",
            next_pose_elapsed
        );
        assert_eq!(
            count_readiness(&next_pose_frame, TileReadiness::Present),
            ahead_visible_tiles.len(),
            "the ahead ring's warm fetch covered the next pose's visible set"
        );
        assert_eq!(
            provider.fetch_count(),
            fetch_count_after_warm,
            "the next pose's frame fetched nothing"
        );
    }
}
