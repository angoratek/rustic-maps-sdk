//! Web-mercator tile identity shared by the projection, scheduler, and pool.

use geospatial_types::{MAXIMUM_TILE_ZOOM_LEVEL, SDKError};

/// One web-mercator XYZ tile address, validated on construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileKey {
    /// Tile column, in `0..2^zoom_level`.
    pub tile_x: u32,
    /// Tile row, counted from the north, in `0..2^zoom_level`.
    pub tile_y: u32,
    /// Zoom level this tile belongs to.
    pub zoom_level: u8,
}

impl TileKey {
    /// Builds a tile key, rejecting addresses outside the zoom level.
    ///
    /// # Parameters
    /// - `tile_x` — tile column, in `0..2^zoom_level`.
    /// - `tile_y` — tile row from the north, in `0..2^zoom_level`.
    /// - `zoom_level` — the zoom level the tile belongs to.
    ///
    /// # Errors
    /// Returns [`SDKError::InvalidCoordinateInput`] when either coordinate is
    /// outside the zoom level's grid, or when the zoom level exceeds
    /// [`MAXIMUM_TILE_ZOOM_LEVEL`] and its tile count is not representable.
    pub fn new(tile_x: u32, tile_y: u32, zoom_level: u8) -> Result<Self, SDKError> {
        if zoom_level > MAXIMUM_TILE_ZOOM_LEVEL {
            return Err(SDKError::InvalidCoordinateInput(format!(
                "zoom level {zoom_level} exceeds the maximum tile zoom level {MAXIMUM_TILE_ZOOM_LEVEL}"
            )));
        }
        let tiles_per_axis = 1u64 << zoom_level;
        if u64::from(tile_x) >= tiles_per_axis || u64::from(tile_y) >= tiles_per_axis {
            return Err(SDKError::InvalidCoordinateInput(format!(
                "tile ({tile_x}, {tile_y}) outside the {tiles_per_axis}x{tiles_per_axis} grid of zoom level {zoom_level}"
            )));
        }
        Ok(Self {
            tile_x,
            tile_y,
            zoom_level,
        })
    }

    /// The parent tile one level up, which shows the same area at half the
    /// detail. Root tiles return themselves.
    pub fn parent_tile_key(&self) -> Option<TileKey> {
        if self.zoom_level == 0 {
            return None;
        }
        TileKey::new(self.tile_x / 2, self.tile_y / 2, self.zoom_level - 1).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::TileKey;
    use geospatial_types::{MAXIMUM_TILE_ZOOM_LEVEL, SDKError};

    #[test]
    fn rejects_addresses_outside_the_zoom_grid() {
        assert!(TileKey::new(0, 0, 0).is_ok());
        assert!(TileKey::new(1, 0, 0).is_err());
        assert!(TileKey::new(3, 3, 2).is_ok());
        assert!(TileKey::new(4, 0, 2).is_err());
        assert!(matches!(
            TileKey::new(1, 0, 0),
            Err(SDKError::InvalidCoordinateInput(_))
        ));
    }

    #[test]
    fn accepts_the_highest_zoom_and_rejects_the_next_level() {
        assert!(TileKey::new(0, 0, MAXIMUM_TILE_ZOOM_LEVEL).is_ok());
        let at_overflow_zoom = MAXIMUM_TILE_ZOOM_LEVEL + 1;
        assert_eq!(at_overflow_zoom, 64);
        let error =
            TileKey::new(0, 0, at_overflow_zoom).expect_err("the overflow zoom must be rejected");
        assert!(matches!(
            error,
            SDKError::InvalidCoordinateInput(ref detail)
                if detail.contains(&MAXIMUM_TILE_ZOOM_LEVEL.to_string())
        ));
    }

    #[test]
    fn the_same_zoom_bound_reaches_projection_and_camera() {
        use crate::camera::{DEFAULT_FIELD_OF_VIEW_DEGREES, MapCamera, ViewportPixels};
        use crate::projection::tile_key_for_coordinate;
        use geospatial_types::GeographicCoordinate;

        let coordinate = GeographicCoordinate::from_degrees(0.0, 0.0).unwrap();
        let above_the_bound = MAXIMUM_TILE_ZOOM_LEVEL + 1;

        // Projection clamps a request above the bound back to it.
        assert_eq!(
            tile_key_for_coordinate(&coordinate, above_the_bound).zoom_level,
            MAXIMUM_TILE_ZOOM_LEVEL
        );

        // The camera returns no tiles above the bound instead of shifting out
        // of the tile-count representation.
        let camera = MapCamera::new(coordinate, 0.0, 0.0, 0.0);
        let viewport = ViewportPixels::new(1_024.0, 768.0).unwrap();
        assert!(
            camera
                .visible_tile_keys(&viewport, DEFAULT_FIELD_OF_VIEW_DEGREES, above_the_bound)
                .is_empty()
        );
    }

    #[test]
    fn parent_halves_the_address() {
        let child = TileKey::new(7, 6, 3).unwrap();
        let parent = child.parent_tile_key().unwrap();
        assert_eq!((parent.tile_x, parent.tile_y, parent.zoom_level), (3, 3, 2));
        let root = TileKey::new(0, 0, 0).unwrap();
        assert!(root.parent_tile_key().is_none());
    }
}
