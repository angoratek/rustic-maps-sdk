//! The free projection helpers of the binding surface, mirroring
//! `rustic-maps-web-bindings`' namespace functions one-to-one, plus the input
//! validation they and the camera calls share.

use crate::error::SdkError;
use crate::records::{
    BoundingBox, GeographicCoordinate, NormalizedWorldCoordinate, TileKey, ViewportSize,
};

/// Builds a validated native coordinate from the binding record, mapping the
/// SDK error to the binding error model.
pub(crate) fn validated_coordinate(
    coordinate: GeographicCoordinate,
) -> Result<geospatial_types::GeographicCoordinate, SdkError> {
    geospatial_types::GeographicCoordinate::from_degrees(coordinate.latitude, coordinate.longitude)
        .map_err(SdkError::from)
}

/// Builds a native viewport from the binding record, mapping the core's
/// validation message (the viewport validator reports plain strings) onto the
/// binding error model.
pub(crate) fn validated_viewport_pixels(
    viewport: ViewportSize,
) -> Result<rustic_maps_rendering::ViewportPixels, SdkError> {
    rustic_maps_rendering::ViewportPixels::new(viewport.width_pixels, viewport.height_pixels)
        .map_err(|message| SdkError::InvalidCoordinateInput { detail: message })
}

/// Builds a validated native bounding box from the binding record, mapping the
/// SDK error to the binding error model.
pub(crate) fn validated_bounding_box(
    bounding_box: BoundingBox,
) -> Result<geospatial_types::BoundingBox, SdkError> {
    let south_west = validated_coordinate(bounding_box.south_west)?;
    let north_east = validated_coordinate(bounding_box.north_east)?;
    geospatial_types::BoundingBox::from_corners(south_west, north_east).map_err(SdkError::from)
}

/// Maps a coordinate to normalized world coordinates in `[0, 1]`²; latitude
/// is clamped to the Mercator cutoffs, mirroring the native
/// `coordinate_to_normalized_world`.
pub fn coordinate_to_normalized_world(
    coordinate: GeographicCoordinate,
) -> Result<NormalizedWorldCoordinate, SdkError> {
    let coordinate = validated_coordinate(coordinate)?;
    Ok(NormalizedWorldCoordinate::from_normalized_world_components(
        rustic_maps_rendering::coordinate_to_normalized_world(&coordinate),
    ))
}

/// The XYZ address of the tile containing the coordinate at the zoom level,
/// mirroring the native `tile_key_for_coordinate`.
pub fn tile_key_for_coordinate(
    coordinate: GeographicCoordinate,
    zoom_level: u8,
) -> Result<TileKey, SdkError> {
    let coordinate = validated_coordinate(coordinate)?;
    Ok(TileKey::from_native_tile_key(
        rustic_maps_rendering::tile_key_for_coordinate(&coordinate, zoom_level),
    ))
}

/// Every XYZ tile whose area intersects the spanning box, ordered row-major
/// from the north, mirroring the native `tiles_covering`.
pub fn tiles_covering(bounding_box: BoundingBox, zoom_level: u8) -> Result<Vec<TileKey>, SdkError> {
    let bounding_box = validated_bounding_box(bounding_box)?;
    Ok(
        rustic_maps_rendering::tiles_covering(&bounding_box, zoom_level)
            .iter()
            .map(|tile_key| TileKey::from_native_tile_key(*tile_key))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        coordinate_to_normalized_world, tiles_covering, validated_coordinate,
        validated_viewport_pixels,
    };
    use crate::projection::tile_key_for_coordinate;
    use crate::records::{BoundingBox, GeographicCoordinate, TileKey, ViewportSize};

    /// Sacramento's latitude in decimal degrees; the coordinate projection
    /// tests anchor on its known normalized-world and tile addresses: the
    /// California State Capitol / Capitol Park, downtown Sacramento.
    const SACRAMENTO_LATITUDE: f64 = 38.5766;
    /// Sacramento's longitude in decimal degrees.
    const SACRAMENTO_LONGITUDE: f64 = -121.4944;
    /// Known normalized-world x of Sacramento.
    const SACRAMENTO_NORMALIZED_X: f64 = 0.162_515_556;
    /// Known normalized-world y of Sacramento.
    const SACRAMENTO_NORMALIZED_Y: f64 = 0.383_688_050;
    /// Known tile column of Sacramento at zoom 14.
    const SACRAMENTO_TILE_X: u32 = 2662;
    /// Known tile row of Sacramento at zoom 14.
    const SACRAMENTO_TILE_Y: u32 = 6286;
    /// Zoom level of the known Sacramento tile address.
    const SACRAMENTO_TILE_ZOOM_LEVEL: u8 = 14;

    fn coordinate(latitude: f64, longitude: f64) -> GeographicCoordinate {
        GeographicCoordinate {
            latitude,
            longitude,
        }
    }

    #[test]
    fn validated_coordinate_accepts_canonical_degrees() {
        let validated = validated_coordinate(coordinate(SACRAMENTO_LATITUDE, SACRAMENTO_LONGITUDE))
            .expect("canonical degrees are valid");
        assert_eq!(validated.latitude, SACRAMENTO_LATITUDE);
        assert_eq!(validated.longitude, SACRAMENTO_LONGITUDE);
    }

    #[test]
    fn validated_coordinate_rejects_out_of_range_input() {
        assert!(validated_coordinate(coordinate(95.0, 0.0)).is_err());
        assert!(validated_coordinate(coordinate(0.0, -200.0)).is_err());
    }

    #[test]
    fn validated_viewport_rejects_non_positive_extents() {
        assert!(
            validated_viewport_pixels(ViewportSize {
                width_pixels: 0.0,
                height_pixels: 600.0,
            })
            .is_err()
        );
        assert!(
            validated_viewport_pixels(ViewportSize {
                width_pixels: 800.0,
                height_pixels: -1.0,
            })
            .is_err()
        );
    }

    #[test]
    fn validated_viewport_accepts_positive_extents() {
        let viewport = validated_viewport_pixels(ViewportSize {
            width_pixels: 800.0,
            height_pixels: 600.0,
        })
        .expect("positive extents are valid");
        assert_eq!(viewport.width_pixels, 800.0);
        assert_eq!(viewport.height_pixels, 600.0);
    }

    #[test]
    fn maps_known_coordinate_to_normalized_world() {
        let normalized_world =
            coordinate_to_normalized_world(coordinate(SACRAMENTO_LATITUDE, SACRAMENTO_LONGITUDE))
                .expect("canonical coordinate is valid");
        assert!((normalized_world.x - SACRAMENTO_NORMALIZED_X).abs() < 1e-6);
        assert!((normalized_world.y - SACRAMENTO_NORMALIZED_Y).abs() < 1e-6);
    }

    #[test]
    fn coordinate_to_normalized_world_rejects_invalid_input() {
        assert!(coordinate_to_normalized_world(coordinate(91.0, 0.0)).is_err());
    }

    #[test]
    fn tile_key_follows_the_known_grid_address() {
        let tile_key = tile_key_for_coordinate(
            coordinate(SACRAMENTO_LATITUDE, SACRAMENTO_LONGITUDE),
            SACRAMENTO_TILE_ZOOM_LEVEL,
        )
        .expect("canonical coordinate is valid");
        assert_eq!(
            tile_key,
            TileKey {
                tile_x: SACRAMENTO_TILE_X,
                tile_y: SACRAMENTO_TILE_Y,
                zoom_level: SACRAMENTO_TILE_ZOOM_LEVEL,
            }
        );
    }

    #[test]
    fn tile_key_rejects_invalid_coordinate() {
        assert!(tile_key_for_coordinate(coordinate(0.0, 180.5), 3).is_err());
    }

    #[test]
    fn tiles_covering_lists_the_four_zoom_one_tiles_row_major_from_the_north() {
        // A box across the equator and prime meridian touches all four
        // zoom-1 tiles, with the northern row first.
        let bounding_box = BoundingBox {
            south_west: coordinate(-1.0, -1.0),
            north_east: coordinate(1.0, 1.0),
        };
        let covering_tiles = tiles_covering(bounding_box, 1).expect("ordered corners are valid");
        let expected_tile_x_columns = [0, 1, 0, 1];
        let expected_tile_y_rows = [0, 0, 1, 1];
        for (index, tile_key) in covering_tiles.iter().enumerate() {
            assert_eq!(
                tile_key.tile_x, expected_tile_x_columns[index],
                "{tile_key:?}"
            );
            assert_eq!(tile_key.tile_y, expected_tile_y_rows[index], "{tile_key:?}");
        }
        assert_eq!(covering_tiles.len(), 4);
    }

    #[test]
    fn tiles_covering_collapses_a_small_high_zoom_box_to_one_tile() {
        // The Sacramento box spans a fraction of one degree; at zoom 14 it falls
        // inside the single known Sacramento tile.
        let bounding_box = BoundingBox {
            south_west: coordinate(38.575561, -121.4951),
            north_east: coordinate(38.576715, -121.4941),
        };
        let covering_tiles = tiles_covering(bounding_box, 14).expect("ordered corners are valid");
        assert_eq!(covering_tiles.len(), 1);
    }

    #[test]
    fn tiles_covering_rejects_inverted_corners() {
        let bounding_box = BoundingBox {
            south_west: coordinate(49.0, 11.0),
            north_east: coordinate(45.0, 5.0),
        };
        assert!(tiles_covering(bounding_box, 2).is_err());
    }
}
