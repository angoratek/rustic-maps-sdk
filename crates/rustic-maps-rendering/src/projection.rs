//! Web-mercator projection: geographic coordinates, normalized world space,
//! and the XYZ tile grid.
//!
//! Normalized world coordinates span `[0, 1]` in both axes — x grows eastward
//! from the antimeridian, y grows southward from the north pole — so a single
//! multiplication by [`zoom_scale_pixels`] places geometry in screen-scale
//! units. All math stays in `f64` because world units reach ~8.6e9 pixels at
//! zoom 24, beyond `f32` mantissa precision.

use crate::tile_key::TileKey;
use geospatial_types::{BoundingBox, GeographicCoordinate};
use std::f64::consts::PI;

/// Pixel size of one tile edge at zoom 0; two of these tile the whole world.
pub const MERCATOR_TILE_SIZE_PIXELS: f64 = 512.0;

/// Latitude cutoff of the Mercator projection, where the map square closes;
/// re-exported from `geospatial-types` so every layer shares one value.
pub use geospatial_types::MAXIMUM_MERCATOR_LATITUDE_DEGREES;

/// Highest zoom level the XYZ tile grid can address; re-exported from
/// `geospatial-types` so projection, camera and tile validation share one
/// bound.
pub use geospatial_types::MAXIMUM_TILE_ZOOM_LEVEL;

/// Guard so a right-hand range boundary landing exactly on a tile edge maps
/// to the tile on the far side of that edge, mirroring `floor` semantics.
pub(crate) const TILE_BOUNDARY_EPSILON: f64 = 1e-9;

/// Maps a coordinate to normalized world coordinates in `[0, 1]²`. Latitude is
/// clamped to the Mercator cutoffs.
pub fn coordinate_to_normalized_world(coordinate: &GeographicCoordinate) -> [f64; 2] {
    let latitude_degrees = coordinate.latitude.clamp(
        -MAXIMUM_MERCATOR_LATITUDE_DEGREES,
        MAXIMUM_MERCATOR_LATITUDE_DEGREES,
    );
    let latitude_radians = latitude_degrees.to_radians();
    let mercator_y = (latitude_radians.tan() + 1.0 / latitude_radians.cos()).ln() / PI;
    let y = (1.0 - mercator_y) / 2.0;
    let x = (coordinate.longitude + 180.0) / 360.0;
    [x, y.clamp(0.0, 1.0)]
}

/// Reverses [`coordinate_to_normalized_world`]; longitude wraps on integer
/// multiples of the world width and latitude clamps to the Mercator cutoffs.
pub fn normalized_world_to_coordinate(normalized_world: [f64; 2]) -> GeographicCoordinate {
    let wrapped_x = normalized_world[0].rem_euclid(1.0);
    let clamped_y = normalized_world[1].clamp(0.0, 1.0);
    let latitude_degrees = ((PI * (1.0 - 2.0 * clamped_y)).sinh()).atan().to_degrees();
    GeographicCoordinate {
        longitude: wrapped_x * 360.0 - 180.0,
        latitude: latitude_degrees,
    }
}

/// Pixel length of one world unit at the given zoom level.
pub fn zoom_scale_pixels(zoom_level: f64) -> f64 {
    MERCATOR_TILE_SIZE_PIXELS * zoom_level.exp2()
}

/// The XYZ address of the tile containing the coordinate at the zoom level.
/// Zoom levels above [`MAXIMUM_TILE_ZOOM_LEVEL`] are clamped to it.
pub fn tile_key_for_coordinate(coordinate: &GeographicCoordinate, zoom_level: u8) -> TileKey {
    let normalized_world = coordinate_to_normalized_world(coordinate);
    let zoom_level = zoom_level.min(MAXIMUM_TILE_ZOOM_LEVEL);
    let tiles_per_axis = 1u64 << zoom_level;
    let maximum_tile_index = (tiles_per_axis - 1).min(u64::from(u32::MAX));
    let tile_x =
        ((normalized_world[0] * tiles_per_axis as f64) as u64).min(maximum_tile_index) as u32;
    let tile_y =
        ((normalized_world[1] * tiles_per_axis as f64) as u64).min(maximum_tile_index) as u32;
    TileKey {
        tile_x,
        tile_y,
        zoom_level,
    }
}

/// Every XYZ tile whose area intersects the bounding box at the zoom level,
/// ordered row-major from the north.
pub fn tiles_covering(bounding_box: &BoundingBox, zoom_level: u8) -> Vec<TileKey> {
    // The west edge and the north edge live on different corners: the NW
    // corner pairs the box's minimum longitude with its maximum latitude.
    let north_west = coordinate_to_normalized_world(&GeographicCoordinate {
        longitude: bounding_box.south_west.longitude,
        latitude: bounding_box.north_east.latitude,
    });
    let south_east = coordinate_to_normalized_world(&GeographicCoordinate {
        longitude: bounding_box.north_east.longitude,
        latitude: bounding_box.south_west.latitude,
    });
    tiles_covering_normalized_world(north_west, south_east, zoom_level)
}

/// Every XYZ tile intersecting the normalized-world axis-aligned box, ordered
/// row-major from the north. Both bounds inclusive; y values clamp to the
/// map square, x values are expected to already sit in `[0, 1]`. A zoom level
/// above [`MAXIMUM_TILE_ZOOM_LEVEL`] yields no tiles: its grid count is not
/// representable.
pub(crate) fn tiles_covering_normalized_world(
    minimum: [f64; 2],
    maximum: [f64; 2],
    zoom_level: u8,
) -> Vec<TileKey> {
    if zoom_level > MAXIMUM_TILE_ZOOM_LEVEL {
        return Vec::new();
    }
    let tiles_per_axis = 2f64.powi(i32::from(zoom_level));
    let tiles_per_axis_index = (tiles_per_axis as u32).saturating_sub(1);
    let min_tile_x = (minimum[0] * tiles_per_axis).floor().max(0.0) as u32;
    let max_tile_x = (maximum[0] * tiles_per_axis - TILE_BOUNDARY_EPSILON)
        .floor()
        .max(0.0) as u32;
    let min_tile_y = (minimum[1] * tiles_per_axis).floor().max(0.0) as u32;
    let max_tile_y = (maximum[1] * tiles_per_axis - TILE_BOUNDARY_EPSILON)
        .floor()
        .max(0.0) as u32;

    let mut covered_tile_keys = Vec::new();
    for tile_y in min_tile_y..=max_tile_y.min(tiles_per_axis_index) {
        for tile_x in min_tile_x..=max_tile_x.min(tiles_per_axis_index) {
            if let Ok(tile_key) = TileKey::new(tile_x, tile_y, zoom_level) {
                covered_tile_keys.push(tile_key);
            }
        }
    }
    covered_tile_keys
}

#[cfg(test)]
mod tests {
    use super::{
        MAXIMUM_TILE_ZOOM_LEVEL, coordinate_to_normalized_world, normalized_world_to_coordinate,
        tile_key_for_coordinate, tiles_covering, zoom_scale_pixels,
    };
    use crate::tile_key::TileKey;
    use geospatial_types::BoundingBox;
    use proptest::prelude::*;
    use rustic_maps_test_support::validated_coordinate as coordinate;

    #[test]
    fn known_normalization_values() {
        let north_west = coordinate_to_normalized_world(&coordinate(0.0, -180.0));
        assert!((north_west[0] - 0.0).abs() < 1e-12);
        assert!((north_west[1] - 0.5).abs() < 1e-12);
        let sacramento = coordinate_to_normalized_world(&coordinate(38.5766, -121.4944));
        assert!((sacramento[0] - 0.162_515_556).abs() < 1e-6);
        assert!((sacramento[1] - 0.383_688_050).abs() < 1e-6);
    }

    #[test]
    fn round_trips_through_normalized_world() {
        for latitude in [-85.0, -60.0, -1.5, 0.0, 33.3, 85.0] {
            for longitude in [-180.0, -120.7, -1.0, 0.0, 15.5, 179.9] {
                let normalized_world =
                    coordinate_to_normalized_world(&coordinate(latitude, longitude));
                let recovered = normalized_world_to_coordinate(normalized_world);
                assert!((recovered.latitude - latitude).abs() < 1e-9);
                assert!((recovered.longitude - longitude).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn coordinates_beyond_the_mercator_cutoff_clamp_to_the_poles() {
        let too_far_north = coordinate_to_normalized_world(&coordinate(89.0, 0.0));
        let too_far_south = coordinate_to_normalized_world(&coordinate(-89.0, 0.0));
        assert!(
            too_far_north[1] < 1e-8,
            "north clamp misses the pole: {}",
            too_far_north[1]
        );
        assert!(too_far_south[1] > 1.0 - 1e-8);
        let pole_coordinate = normalized_world_to_coordinate([0.5, -0.5]);
        assert!((pole_coordinate.latitude - 85.051_128_78).abs() < 1e-6);
    }

    #[test]
    fn scale_doubles_per_zoom_level() {
        assert_eq!(zoom_scale_pixels(0.0), 512.0);
        assert_eq!(zoom_scale_pixels(1.0), 1024.0);
        assert_eq!(zoom_scale_pixels(2.5), 512.0 * 2.0f64.powf(2.5));
    }

    #[test]
    fn tile_address_follows_the_grid() {
        let zero_tile = tile_key_for_coordinate(&coordinate(0.0, 0.0), 0);
        assert_eq!((zero_tile.tile_x, zero_tile.tile_y), (0, 0));
        // Sacramento's Capitol anchor falls in tile (2662, 6286) at zoom 14
        // (known web-mercator address).
        let sacramento_tile = tile_key_for_coordinate(&coordinate(38.5766, -121.4944), 14);
        assert_eq!(
            (sacramento_tile.tile_x, sacramento_tile.tile_y),
            (2662, 6286)
        );
        // Right-hand boundary belongs to the eastern tile.
        let boundary_tile = tile_key_for_coordinate(&coordinate(0.0, 0.0), 3);
        assert_eq!((boundary_tile.tile_x, boundary_tile.tile_y), (4, 4));
        let just_west = tile_key_for_coordinate(&coordinate(0.0, -0.000_1), 3);
        assert_eq!(just_west.tile_x, 3);
    }

    #[test]
    fn covering_lists_intersecting_tiles_row_major_from_the_north() {
        // Zoom 1 world spans 2x2 tiles; a box across the equator and prime
        // meridian touches all four, with the northern row first.
        let world_center_box = BoundingBox {
            south_west: coordinate(-1.0, -1.0),
            north_east: coordinate(1.0, 1.0),
        };
        let covering = tiles_covering(&world_center_box, 1);
        assert_eq!(
            covering,
            vec![
                TileKey::new(0, 0, 1).unwrap(),
                TileKey::new(1, 0, 1).unwrap(),
                TileKey::new(0, 1, 1).unwrap(),
                TileKey::new(1, 1, 1).unwrap(),
            ]
        );
        // A single small box maps to exactly one high-zoom tile.
        let single_tile_box = BoundingBox {
            south_west: coordinate(38.575561, -121.4951),
            north_east: coordinate(38.576715, -121.4941),
        };
        assert_eq!(tiles_covering(&single_tile_box, 14).len(), 1);
        // Latitude beyond the Mercator cutoffs clamps instead of exploding.
        let polar_box = BoundingBox {
            south_west: coordinate(80.0, -1.0),
            north_east: coordinate(89.9, 1.0),
        };
        assert_eq!(tiles_covering(&polar_box, 2).len(), 2);
    }

    #[test]
    fn tile_address_requests_above_the_zoom_bound_clamp_to_it() {
        let tile = tile_key_for_coordinate(&coordinate(0.0, 0.0), MAXIMUM_TILE_ZOOM_LEVEL + 1);
        assert_eq!(tile.zoom_level, MAXIMUM_TILE_ZOOM_LEVEL);
    }

    #[test]
    fn covering_above_the_zoom_bound_lists_no_tiles() {
        let world_center_box = BoundingBox {
            south_west: coordinate(-1.0, -1.0),
            north_east: coordinate(1.0, 1.0),
        };
        assert!(tiles_covering(&world_center_box, MAXIMUM_TILE_ZOOM_LEVEL + 1).is_empty());
    }

    proptest! {
        #[test]
        fn normalized_round_trip_holds_everywhere(
            latitude in -84.0f64..84.0,
            longitude in -179.9f64..179.9,
        ) {
            let coordinate = coordinate(latitude, longitude);
            let recovered =
                normalized_world_to_coordinate(coordinate_to_normalized_world(&coordinate));
            prop_assert!((recovered.latitude - latitude).abs() < 1e-9);
            prop_assert!((recovered.longitude - longitude).abs() < 1e-9);
        }

        #[test]
        fn covering_tiles_always_contain_the_box_center(
            center_latitude in -80.0f64..80.0,
            center_longitude in -179.0f64..179.0,
            span_degrees in 0.01f64..8.0,
            zoom_level in 0u8..12,
        ) {
            let bounding_box = BoundingBox {
                south_west: coordinate(
                    (center_latitude - span_degrees / 2.0).max(-85.0),
                    (center_longitude - span_degrees / 2.0).max(-179.9),
                ),
                north_east: coordinate(
                    (center_latitude + span_degrees / 2.0).min(85.0),
                    (center_longitude + span_degrees / 2.0).min(179.9),
                ),
            };
            let covering = tiles_covering(&bounding_box, zoom_level);
            prop_assert!(!covering.is_empty());
            let center_tile = tile_key_for_coordinate(
                &coordinate(center_latitude, center_longitude),
                zoom_level,
            );
            prop_assert!(covering.contains(&center_tile));
        }
    }
}
