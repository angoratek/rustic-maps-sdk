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

    /// Builds the smallest box containing every coordinate.
    ///
    /// # Parameters
    /// - `coordinates` — any iterator of coordinates. They are taken as
    ///   given, so a caller that has not validated them should use
    ///   [`BoundingBox::from_corners`] instead.
    ///
    /// # Returns
    /// `None` when the iterator is empty; otherwise the box whose corners are
    /// the minimum and maximum latitude and longitude.
    pub fn from_coordinates<I>(coordinates: I) -> Option<Self>
    where
        I: IntoIterator<Item = GeographicCoordinate>,
    {
        let mut coordinates = coordinates.into_iter();
        let first = coordinates.next()?;
        let mut south_west = first;
        let mut north_east = first;
        for coordinate in coordinates {
            south_west.latitude = south_west.latitude.min(coordinate.latitude);
            south_west.longitude = south_west.longitude.min(coordinate.longitude);
            north_east.latitude = north_east.latitude.max(coordinate.latitude);
            north_east.longitude = north_east.longitude.max(coordinate.longitude);
        }
        Some(Self {
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

    #[test]
    fn from_coordinates_covers_every_point() {
        let bounding_box = BoundingBox::from_coordinates([
            coordinate(48.0, -122.0),
            coordinate(45.0, 8.0),
            coordinate(47.0, -118.0),
        ])
        .unwrap();
        assert_eq!(bounding_box.south_west, coordinate(45.0, -122.0));
        assert_eq!(bounding_box.north_east, coordinate(48.0, 8.0));
    }

    #[test]
    fn from_coordinates_returns_none_for_an_empty_iterator() {
        assert_eq!(BoundingBox::from_coordinates(std::iter::empty()), None);
    }

    #[test]
    fn from_coordinates_handles_a_single_point() {
        let point = coordinate(38.5766, -121.4944);
        let bounding_box = BoundingBox::from_coordinates([point]).unwrap();
        assert_eq!(bounding_box.south_west, point);
        assert_eq!(bounding_box.north_east, point);
    }
}
