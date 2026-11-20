//! Coordinates, viewport geometry and tile addressing.

use rustic_maps_rendering::TileKey as RenderingTileKey;

/// A position on the WGS 84 ellipsoid in decimal degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeographicCoordinate {
    /// Latitude in decimal degrees, valid within `[-90, 90]`.
    pub latitude: f64,
    /// Longitude in decimal degrees, valid within `[-180, 180]`.
    pub longitude: f64,
}

impl GeographicCoordinate {
    /// Builds the mirror record from a validated native coordinate.
    pub(crate) fn from_native_coordinate(
        native_coordinate: &geospatial_types::GeographicCoordinate,
    ) -> Self {
        Self {
            latitude: native_coordinate.latitude,
            longitude: native_coordinate.longitude,
        }
    }

    /// Builds the native coordinate from the mirror record. The mirror never
    /// carries canonical-degree guarantees (foreign implementations may hand
    /// any value across), so this is a plain field copy without validation;
    /// the call paths that need canonical degrees validate on their own
    /// boundary.
    pub(crate) fn to_native_coordinate(self) -> geospatial_types::GeographicCoordinate {
        geospatial_types::GeographicCoordinate {
            latitude: self.latitude,
            longitude: self.longitude,
        }
    }
}

/// Rectangular area between a south-west and a north-east corner; boxes
/// never span the antimeridian (encoded as two adjacent boxes instead).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundingBox {
    /// South-west corner (minimum latitude and longitude).
    pub south_west: GeographicCoordinate,
    /// North-east corner (maximum latitude and longitude).
    pub north_east: GeographicCoordinate,
}

impl BoundingBox {
    /// Builds the mirror record from a native bounding box.
    pub(crate) fn from_native_bounding_box(
        native_bounding_box: &geospatial_types::BoundingBox,
    ) -> Self {
        Self {
            south_west: GeographicCoordinate::from_native_coordinate(
                &native_bounding_box.south_west,
            ),
            north_east: GeographicCoordinate::from_native_coordinate(
                &native_bounding_box.north_east,
            ),
        }
    }

    /// Builds the native bounding box from the mirror record. As with the
    /// coordinate converters this is a plain field copy without box
    /// validation; the call paths needing canonical boxes validate on their
    /// own boundary.
    pub(crate) fn to_native_bounding_box(self) -> geospatial_types::BoundingBox {
        geospatial_types::BoundingBox {
            south_west: self.south_west.to_native_coordinate(),
            north_east: self.north_east.to_native_coordinate(),
        }
    }
}

/// Viewport dimensions in physical pixels.
#[derive(Debug, Clone, Copy)]
pub struct ViewportSize {
    /// Horizontal extent in pixels.
    pub width_pixels: f64,
    /// Vertical extent in pixels.
    pub height_pixels: f64,
}

/// Screen-space insets kept clear of map content: distances from each
/// viewport edge in pixels, so UI overlays — route cards, attribution, sheet
/// panels — never cover the content the camera guarantees visible.
#[derive(Debug, Clone, Copy)]
pub struct EdgeInsets {
    /// Pixels kept clear from the left viewport edge.
    pub left: f64,
    /// Pixels kept clear from the top viewport edge.
    pub top: f64,
    /// Pixels kept clear from the right viewport edge.
    pub right: f64,
    /// Pixels kept clear from the bottom viewport edge.
    pub bottom: f64,
}

impl EdgeInsets {
    /// Builds the native inset set from the mirror record, for the native
    /// `fit_bounds` constructor.
    pub(crate) fn to_native_edge_insets(self) -> rustic_maps_rendering::EdgeInsetsPixels {
        rustic_maps_rendering::EdgeInsetsPixels {
            left: self.left,
            top: self.top,
            right: self.right,
            bottom: self.bottom,
        }
    }
}

/// Position on the viewport in physical pixels (origin top-left, y down).
#[derive(Debug, Clone, Copy)]
pub struct ScreenPoint {
    /// Horizontal position in pixels.
    pub x: f64,
    /// Vertical position in pixels.
    pub y: f64,
}

impl ScreenPoint {
    /// Builds a screen point from the native projection's two-component
    /// result in viewport pixels.
    pub(crate) fn from_screen_pixels(screen_pixels: [f64; 2]) -> Self {
        Self {
            x: screen_pixels[0],
            y: screen_pixels[1],
        }
    }
}

/// A world position in normalized world coordinates, both axes in `[0, 1]`
/// (x eastward from the antimeridian, y southward from the north pole).
#[derive(Debug, Clone, Copy)]
pub struct NormalizedWorldCoordinate {
    /// Eastward position from the antimeridian.
    pub x: f64,
    /// Southward position from the north pole.
    pub y: f64,
}

impl NormalizedWorldCoordinate {
    /// Builds a normalized world coordinate from the native projection's
    /// two-component result.
    pub(crate) fn from_normalized_world_components(normalized_world_components: [f64; 2]) -> Self {
        Self {
            x: normalized_world_components[0],
            y: normalized_world_components[1],
        }
    }
}

/// One web-mercator XYZ tile address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileKey {
    /// Tile column, in `0..2^zoom_level`.
    pub tile_x: u32,
    /// Tile row, counted from the north, in `0..2^zoom_level`.
    pub tile_y: u32,
    /// Zoom level this tile belongs to.
    pub zoom_level: u8,
}

impl TileKey {
    /// Builds a tile key record from the native `TileKey`.
    pub(crate) fn from_native_tile_key(tile_key: RenderingTileKey) -> Self {
        Self {
            tile_x: tile_key.tile_x,
            tile_y: tile_key.tile_y,
            zoom_level: tile_key.zoom_level,
        }
    }
}
