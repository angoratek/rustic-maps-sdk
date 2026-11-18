//! Web-mercator tile identity shared by the projection, scheduler, and pool.

use geospatial_types::SDKError;

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
    /// outside the zoom level's grid.
    pub fn new(tile_x: u32, tile_y: u32, zoom_level: u8) -> Result<Self, SDKError> {
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
    use geospatial_types::SDKError;

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
    fn parent_halves_the_address() {
        let child = TileKey::new(7, 6, 3).unwrap();
        let parent = child.parent_tile_key().unwrap();
        assert_eq!((parent.tile_x, parent.tile_y, parent.zoom_level), (3, 3, 2));
        let root = TileKey::new(0, 0, 0).unwrap();
        assert!(root.parent_tile_key().is_none());
    }
}
