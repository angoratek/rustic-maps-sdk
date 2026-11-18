//! Axis-aligned rectangular bounding box record.

use crate::coordinate::GeographicCoordinate;
use serde::{Deserialize, Serialize};

/// Rectangular area between a south-west and a north-east corner.
///
/// Boxes never span the antimeridian in this model; an application that needs
/// a crossing region encodes it as two adjacent boxes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BoundingBox {
    /// South-west corner (minimum latitude and longitude).
    pub south_west: GeographicCoordinate,
    /// North-east corner (maximum latitude and longitude).
    pub north_east: GeographicCoordinate,
}

impl BoundingBox {
    /// Builds a bounding box from its two opposite corners.
    ///
    /// # Parameters
    /// - `south_west` — the corner with the minimum latitude and longitude.
    /// - `north_east` — the corner with the maximum latitude and longitude.
    ///
    /// # Returns
    /// A box whose edges are included in the area.
    ///
    /// # Errors
    /// Returns [`crate::SDKError::InvalidCoordinateInput`] when either corner
    /// is not a valid coordinate, or when the south-west corner lies north or
    /// east of the north-east corner.
    pub fn from_corners(
        south_west: GeographicCoordinate,
        north_east: GeographicCoordinate,
    ) -> Result<Self, crate::SDKError> {
        let _ = GeographicCoordinate::from_degrees(south_west.latitude, south_west.longitude)?;
        let _ = GeographicCoordinate::from_degrees(north_east.latitude, north_east.longitude)?;
        if south_west.latitude > north_east.latitude || south_west.longitude > north_east.longitude
        {
            return Err(crate::SDKError::InvalidCoordinateInput(
                "bounding box corners are inverted".to_string(),
            ));
        }
        Ok(Self {
            south_west,
            north_east,
        })
    }

    /// Reports whether the coordinate lies within the closed box.
    pub fn contains_coordinate(&self, coordinate: &GeographicCoordinate) -> bool {
        coordinate.latitude >= self.south_west.latitude
            && coordinate.latitude <= self.north_east.latitude
            && coordinate.longitude >= self.south_west.longitude
            && coordinate.longitude <= self.north_east.longitude
    }
}

#[cfg(test)]
mod tests {
    use super::BoundingBox;
    use crate::coordinate::GeographicCoordinate;

    fn coordinate(latitude: f64, longitude: f64) -> GeographicCoordinate {
        GeographicCoordinate::from_degrees(latitude, longitude).unwrap()
    }

    #[test]
    fn contains_points_inside_and_on_edges() {
        let bounding_box =
            BoundingBox::from_corners(coordinate(45.0, 5.0), coordinate(49.0, 11.0)).unwrap();
        assert!(bounding_box.contains_coordinate(&coordinate(47.0, 8.0)));
        assert!(bounding_box.contains_coordinate(&coordinate(45.0, 5.0)));
        assert!(bounding_box.contains_coordinate(&coordinate(49.0, 11.0)));
        assert!(!bounding_box.contains_coordinate(&coordinate(44.9, 8.0)));
        assert!(!bounding_box.contains_coordinate(&coordinate(47.0, 11.1)));
    }

    #[test]
    fn rejects_inverted_corners() {
        assert!(BoundingBox::from_corners(coordinate(49.0, 11.0), coordinate(45.0, 5.0)).is_err());
    }
}
