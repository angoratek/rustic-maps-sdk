//! GPU-free predictive prefetch planning: the bounded tile set a caller
//! warms ahead of motion so the pool covers the camera's next poses.
//!
//! The planner emits an ordered request list — the camera's visible set at
//! its tile zoom first, then the ring of tiles ahead along the bearing
//! vector ([`PREFETCH_RING_TILES`] viewport-heights ahead), then a
//! navigation route corridor's tiles when navigation feeds one, then one
//! coarser fallback zoom under everything ([`PREFETCH_FALLBACK_ZOOM_BELOW`]
//! below the camera's tile zoom) — deduplicated against the pool's contents
//! and an already-warming set, capped at
//! [`PREFETCH_MAXIMUM_CONCURRENT_FETCHES`]. The list feeds
//! [`crate::tile_scheduler::TileScheduler::warm_tiles`] or its detached
//! [`crate::tile_scheduler::TileScheduler::plan_warm_tiles`] twin, whose own
//! per-call budget keeps the concurrent fetches bounded.
//!
//! The navigation-corridor entries keep exactly the road corridor warm, and
//! the effect is visible through the frame statistics a caller collects.

use crate::camera::MapCamera;
use crate::camera::ViewportPixels;
use crate::projection::tile_key_for_coordinate;
use crate::tile_key::TileKey;
use crate::tile_texture_pool::TileTexturePool;
use geospatial_types::GeographicCoordinate;
use std::collections::HashSet;

/// How far ahead the ring reaches, in viewport-heights along the camera's
/// bearing vector.
pub const PREFETCH_RING_TILES: f64 = 2.0;

/// Coarser-zoom offset of the single fallback layer under everything,
/// expressed as the distance below the camera's tile zoom (a zoom 14 view
/// prefetches its tiles' zoom 10 ancestors).
pub const PREFETCH_FALLBACK_ZOOM_BELOW: u8 = 4;

/// How many prefetch request tiles the planner emits at most: the cap that
/// keeps prefetch's concurrent fetches bounded (the caller's warm task
/// applies its own per-call budget inside this).
pub const PREFETCH_MAXIMUM_CONCURRENT_FETCHES: usize = 48;

/// Plans the prefetch request list for one camera pose (see the module
/// documentation for the ring's shape). Planning is state-free: the tuning
/// constants are module level, so the planner carries no fields of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TilePrefetchPlanner {}

impl TilePrefetchPlanner {
    /// Builds the planner.
    pub fn new() -> Self {
        Self {}
    }

    /// Emits the prioritized prefetch request list for the camera's pose:
    /// the visible set, the ahead ring, the route corridor, the coarse
    /// fallback — deduplicated against the pool's contents and the
    /// already-warming set, first-occurrence order kept, and capped at
    /// [`PREFETCH_MAXIMUM_CONCURRENT_FETCHES`].
    pub fn prefetch_request_tiles(
        &self,
        camera: &MapCamera,
        viewport: &ViewportPixels,
        field_of_view_degrees: f64,
        route_corridor_ahead: Option<&[GeographicCoordinate]>,
        payload_pool: &TileTexturePool,
        already_warming_tile_keys: &HashSet<TileKey>,
    ) -> Vec<TileKey> {
        let tile_zoom_level = camera.tile_zoom_level();
        let mut prioritized_tiles = Vec::new();

        // The camera's own visible set: the next frames' first need.
        prioritized_tiles.extend(camera.visible_tile_keys(
            viewport,
            field_of_view_degrees,
            tile_zoom_level,
        ));

        // The ahead ring: the camera displaced PREFETCH_RING_TILES
        // viewport-heights up-screen (up-course for the bearing-aligned
        // camera), projected back to the ground and listed at the same tile
        // zoom. A pose whose displaced ray misses the ground (a steep pitch
        // against the horizon) skips its ring — the graceful degradation.
        let ring_center_screen = [
            viewport.width_pixels * 0.5,
            viewport.height_pixels * (0.5 - PREFETCH_RING_TILES),
        ];
        if let Some(ground_ahead_center) =
            camera.screen_to_ground_coordinate(ring_center_screen, viewport, field_of_view_degrees)
        {
            let ahead_camera = MapCamera::new(
                ground_ahead_center,
                camera.zoom_level,
                camera.bearing_degrees,
                camera.pitch_degrees,
            );
            prioritized_tiles.extend(ahead_camera.visible_tile_keys(
                viewport,
                field_of_view_degrees,
                tile_zoom_level,
            ));
        }

        // The navigation corridor's tiles at the camera's tile zoom, when
        // navigation feeds the geometry ahead.
        if let Some(route_corridor_ahead) = route_corridor_ahead {
            prioritized_tiles.extend(route_corridor_ahead.iter().map(|corridor_coordinate| {
                tile_key_for_coordinate(corridor_coordinate, tile_zoom_level)
            }));
        }

        // The single coarser fallback zoom under everything at the camera's
        // tile zoom, by address halving per zoom difference (the ancestor
        // chain's own rule). The fallback list is built over a snapshot of
        // the camera-zoom entries, then appended, so no list is borrowed
        // while it is appended to.
        let fallback_zoom_level = tile_zoom_level.saturating_sub(PREFETCH_FALLBACK_ZOOM_BELOW);
        let zoom_level_shift = u32::from(tile_zoom_level - fallback_zoom_level);
        let camera_zoom_tiles = prioritized_tiles
            .iter()
            .filter(|tile_key| tile_key.zoom_level == tile_zoom_level)
            .copied()
            .collect::<Vec<TileKey>>();
        for zoomed_tile_key in camera_zoom_tiles {
            if let Ok(fallback_tile_key) = TileKey::new(
                zoomed_tile_key.tile_x >> zoom_level_shift,
                zoomed_tile_key.tile_y >> zoom_level_shift,
                fallback_zoom_level,
            ) {
                prioritized_tiles.push(fallback_tile_key);
            }
        }

        // Deduplication against each other, the pool's contents and the
        // already-warming set (first occurrence keeps the priority order),
        // then the concurrency cap.
        let mut requested_tile_keys = Vec::new();
        let mut seen_tile_keys = HashSet::new();
        for tile_key in prioritized_tiles {
            if !seen_tile_keys.insert(tile_key) {
                continue;
            }
            if payload_pool.contains(&tile_key) || already_warming_tile_keys.contains(&tile_key) {
                continue;
            }
            requested_tile_keys.push(tile_key);
            if requested_tile_keys.len() == PREFETCH_MAXIMUM_CONCURRENT_FETCHES {
                break;
            }
        }
        requested_tile_keys
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PREFETCH_FALLBACK_ZOOM_BELOW, PREFETCH_MAXIMUM_CONCURRENT_FETCHES, PREFETCH_RING_TILES,
        TilePrefetchPlanner,
    };
    use crate::camera::{DEFAULT_FIELD_OF_VIEW_DEGREES, MapCamera, ViewportPixels};
    use crate::projection::tile_key_for_coordinate;
    use crate::tile_key::TileKey;
    use crate::tile_texture_pool::TileTexturePool;
    use geospatial_types::GeographicCoordinate;
    use std::collections::HashSet;

    /// The test zoom: fine enough for a multi-tile visible set, coarse
    /// enough for the fallback zoom to stay in range.
    const TEST_ZOOM_LEVEL: f64 = 14.0;

    /// The test viewport edge; a whole power of two so tile spans map
    /// cleanly.
    const TEST_VIEWPORT_EDGE_PIXELS: f64 = 512.0;

    fn coordinate(latitude_degrees: f64, longitude_degrees: f64) -> GeographicCoordinate {
        GeographicCoordinate::from_degrees(latitude_degrees, longitude_degrees)
            .expect("the test coordinates are valid")
    }

    fn square_viewport() -> ViewportPixels {
        ViewportPixels::new(TEST_VIEWPORT_EDGE_PIXELS, TEST_VIEWPORT_EDGE_PIXELS)
            .expect("the test viewport is valid")
    }

    fn test_camera() -> MapCamera {
        MapCamera::new(coordinate(0.0, 0.0), TEST_ZOOM_LEVEL, 0.0, 0.0)
    }

    fn planned_tiles(
        camera: &MapCamera,
        route_corridor_ahead: Option<&[GeographicCoordinate]>,
        payload_pool: &TileTexturePool,
    ) -> Vec<TileKey> {
        TilePrefetchPlanner::new().prefetch_request_tiles(
            camera,
            &square_viewport(),
            DEFAULT_FIELD_OF_VIEW_DEGREES,
            route_corridor_ahead,
            payload_pool,
            &HashSet::new(),
        )
    }

    fn test_view_tiles() -> Vec<TileKey> {
        test_camera().visible_tile_keys(
            &square_viewport(),
            DEFAULT_FIELD_OF_VIEW_DEGREES,
            TEST_ZOOM_LEVEL as u8,
        )
    }

    #[test]
    fn the_plan_leads_with_the_camera_visible_set() {
        let view_tiles = test_view_tiles();
        assert!(view_tiles.len() > 1, "the test view spans several tiles");
        let plan = planned_tiles(&test_camera(), None, &TileTexturePool::new(1 << 20));
        assert!(plan.len() >= view_tiles.len());
        for (planned_tile, view_tile) in plan.iter().zip(&view_tiles) {
            assert_eq!(planned_tile, view_tile, "the visible set leads the plan");
        }
    }

    #[test]
    fn the_ring_reaches_tiles_ahead_up_course() {
        // A north-up camera: the ring center sits
        // PREFETCH_RING_TILES viewport-heights up-screen, so the plan must
        // contain tiles north (smaller tile rows) than the visible set's
        // own northernmost row.
        let smallest_planned_row = test_view_tiles()
            .iter()
            .map(|tile_key| tile_key.tile_y)
            .min()
            .expect("the view lists tiles");
        let ahead_center = test_camera()
            .screen_to_ground_coordinate(
                [
                    TEST_VIEWPORT_EDGE_PIXELS * 0.5,
                    TEST_VIEWPORT_EDGE_PIXELS * (0.5 - PREFETCH_RING_TILES),
                ],
                &square_viewport(),
                DEFAULT_FIELD_OF_VIEW_DEGREES,
            )
            .expect("the ring center's ray reaches the ground at this pitch");
        let ahead_tile = tile_key_for_coordinate(&ahead_center, TEST_ZOOM_LEVEL as u8);
        assert!(
            ahead_tile.tile_y < smallest_planned_row,
            "the ring center's tile must lie north of the visible set"
        );
        let plan = planned_tiles(&test_camera(), None, &TileTexturePool::new(1 << 20));
        assert!(
            plan.iter()
                .any(|tile_key| tile_key.tile_y < smallest_planned_row),
            "the plan must contain ring tiles ahead of the visible set"
        );
    }

    #[test]
    fn the_plan_carries_the_coarse_fallback_zoom_under_everything() {
        let fallback_zoom_level =
            (TEST_ZOOM_LEVEL as u8).saturating_sub(PREFETCH_FALLBACK_ZOOM_BELOW);
        assert!(
            fallback_zoom_level < TEST_ZOOM_LEVEL as u8,
            "the test zoom leaves room below for the fallback layer"
        );
        let plan = planned_tiles(&test_camera(), None, &TileTexturePool::new(1 << 20));
        assert!(
            plan.iter()
                .any(|tile_key| tile_key.zoom_level == fallback_zoom_level),
            "the plan must contain the coarse fallback layer"
        );
        // The fallback tile is exactly the camera center's tile address
        // halved per zoom difference.
        let center_tile = tile_key_for_coordinate(&coordinate(0.0, 0.0), TEST_ZOOM_LEVEL as u8);
        let expected_fallback = TileKey::new(
            center_tile.tile_x >> u32::from(PREFETCH_FALLBACK_ZOOM_BELOW),
            center_tile.tile_y >> u32::from(PREFETCH_FALLBACK_ZOOM_BELOW),
            fallback_zoom_level,
        )
        .expect("the fallback address stays inside its grid");
        assert!(plan.contains(&expected_fallback));
    }

    #[test]
    fn pooled_tiles_are_deduplicated_out_of_the_plan() {
        let view_tiles = test_view_tiles();
        let mut payload_pool = TileTexturePool::new(1 << 20);
        for view_tile in &view_tiles {
            payload_pool.insert(*view_tile, b"payload".to_vec());
        }
        let plan = planned_tiles(&test_camera(), None, &payload_pool);
        for view_tile in &view_tiles {
            assert!(
                !plan.contains(view_tile),
                "a pooled tile must not be requested again: {view_tile:?}"
            );
        }
        assert!(!plan.is_empty(), "the ring and fallback still plan");
    }

    #[test]
    fn already_warming_tiles_are_deduplicated_out_of_the_plan() {
        let view_tiles = test_view_tiles();
        let already_warming_tile_keys: HashSet<TileKey> = view_tiles.iter().copied().collect();
        let warm_free_plan = TilePrefetchPlanner::new().prefetch_request_tiles(
            &test_camera(),
            &square_viewport(),
            DEFAULT_FIELD_OF_VIEW_DEGREES,
            None,
            &TileTexturePool::new(1 << 20),
            &already_warming_tile_keys,
        );
        for warming_tile in &view_tiles {
            assert!(
                !warm_free_plan.contains(warming_tile),
                "a warming tile must not be requested again: {warming_tile:?}"
            );
        }
    }

    #[test]
    fn the_route_corridor_tiles_enter_the_plan() {
        // A corridor reaching well west of the visible set at the camera's
        // zoom: several tiles of roadway the plan must include.
        let corridor: Vec<GeographicCoordinate> = (-40..-32)
            .map(|tile_step| coordinate(0.0, f64::from(tile_step) * 0.0015))
            .collect();
        let plan = planned_tiles(
            &test_camera(),
            Some(&corridor),
            &TileTexturePool::new(1 << 20),
        );
        for corridor_tile in &corridor {
            let expected_tile = tile_key_for_coordinate(corridor_tile, TEST_ZOOM_LEVEL as u8);
            assert!(
                plan.contains(&expected_tile),
                "the corridor's tile must enter the plan: {expected_tile:?}"
            );
        }
    }

    #[test]
    fn the_plan_stays_inside_the_concurrency_cap() {
        // A long corridor that would otherwise exceed the cap.
        let wide_corridor: Vec<GeographicCoordinate> = (-60..60)
            .map(|tile_step| coordinate(0.0, f64::from(tile_step) * 0.002))
            .collect();
        let plan = planned_tiles(
            &test_camera(),
            Some(&wide_corridor),
            &TileTexturePool::new(1 << 20),
        );
        assert!(plan.len() <= PREFETCH_MAXIMUM_CONCURRENT_FETCHES);
    }
}
