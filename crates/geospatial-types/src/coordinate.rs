//! Geographic coordinate record and distance mathematics.

use serde::{Deserialize, Serialize};

/// A position on the WGS 84 ellipsoid expressed in decimal degrees.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GeographicCoordinate {
    /// Latitude in decimal degrees, valid within `[-90.0, 90.0]`.
    pub latitude: f64,
    /// Longitude in decimal degrees, valid within `[-180.0, 180.0]`.
    pub longitude: f64,
}

impl GeographicCoordinate {
    /// Mean Earth radius in meters used by the haversine formula.
    const EARTH_RADIUS_METERS: f64 = 6_371_008.8;

    /// Builds a coordinate from decimal degrees.
    ///
    /// # Parameters
    /// - `latitude` — degrees north of the equator, within `[-90.0, 90.0]`.
    /// - `longitude` — degrees east of the prime meridian, within
    ///   `[-180.0, 180.0]`.
    ///
    /// # Returns
    /// A position on the WGS 84 ellipsoid.
    ///
    /// # Errors
    /// Returns [`crate::SDKError::InvalidCoordinateInput`] when either
    /// component is outside its range. Coordinates built with this function
    /// are always safe to pass to the rest of the SDK.
    pub fn from_degrees(latitude: f64, longitude: f64) -> Result<Self, crate::SDKError> {
        if !(-90.0..=90.0).contains(&latitude) {
            return Err(crate::SDKError::InvalidCoordinateInput(format!(
                "latitude {latitude} outside [-90, 90]"
            )));
        }
        if !(-180.0..=180.0).contains(&longitude) {
            return Err(crate::SDKError::InvalidCoordinateInput(format!(
                "longitude {longitude} outside [-180, 180]"
            )));
        }
        Ok(Self {
            latitude,
            longitude,
        })
    }

    /// Great-circle distance to `other`, in meters, computed with the
    /// haversine formula.
    ///
    /// # Parameters
    /// - `other` — the coordinate to measure to.
    ///
    /// # Returns
    /// The distance along a mean Earth radius of 6,371,008.8 meters. The
    /// result is symmetric and never negative. The method does not validate
    /// its inputs; build both coordinates with [`Self::from_degrees`] to stay
    /// inside the canonical ranges.
    pub fn haversine_distance_meters(&self, other: &Self) -> f64 {
        let latitude_radians_self = self.latitude.to_radians();
        let latitude_radians_other = other.latitude.to_radians();
        let delta_latitude = latitude_radians_other - latitude_radians_self;
        let delta_longitude = (other.longitude - self.longitude).to_radians();

        let sine_half_delta_latitude = (delta_latitude / 2.0).sin();
        let sine_half_delta_longitude = (delta_longitude / 2.0).sin();

        let haversine_ratio = sine_half_delta_latitude * sine_half_delta_latitude
            + latitude_radians_self.cos()
                * latitude_radians_other.cos()
                * sine_half_delta_longitude
                * sine_half_delta_longitude;

        2.0 * Self::EARTH_RADIUS_METERS * haversine_ratio.clamp(0.0, 1.0).sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::GeographicCoordinate;
    use proptest::prelude::*;

    #[test]
    fn rejects_coordinates_outside_canonical_ranges() {
        assert!(GeographicCoordinate::from_degrees(91.0, 0.0).is_err());
        assert!(GeographicCoordinate::from_degrees(0.0, 180.5).is_err());
        assert!(GeographicCoordinate::from_degrees(0.0, 0.0).is_ok());
        assert!(GeographicCoordinate::from_degrees(90.0, 180.0).is_ok());
    }

    #[test]
    fn zero_distance_for_identical_coordinates() {
        let coordinate = GeographicCoordinate::from_degrees(38.5766, -121.4944).unwrap();
        assert!(coordinate.haversine_distance_meters(&coordinate) < 1e-6);
    }

    #[test]
    fn known_distance_between_sacramento_and_san_francisco() {
        let capitol = GeographicCoordinate::from_degrees(38.5766, -121.4944).unwrap();
        let san_francisco = GeographicCoordinate::from_degrees(37.7749, -122.4194).unwrap();
        let distance = capitol.haversine_distance_meters(&san_francisco);
        // Published great-circle distance is roughly 120 km.
        assert!((117_000.0..=123_500.0).contains(&distance));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        #[test]
        fn distance_is_symmetric(
            latitude_a in -90.0f64..=90.0,
            longitude_a in -180.0f64..=180.0,
            latitude_b in -90.0f64..=90.0,
            longitude_b in -180.0f64..=180.0,
        ) {
            let coordinate_a = GeographicCoordinate { latitude: latitude_a, longitude: longitude_a };
            let coordinate_b = GeographicCoordinate { latitude: latitude_b, longitude: longitude_b };
            let forward = coordinate_a.haversine_distance_meters(&coordinate_b);
            let backward = coordinate_b.haversine_distance_meters(&coordinate_a);
            assert!((forward - backward).abs() < 1e-6);
            assert!(forward <= std::f64::consts::PI * 6_371_008.8 + 1e-6);
        }

        #[test]
        fn distance_shrinks_when_points_converge(
            latitude in -90.0f64..=90.0,
            longitude in -180.0f64..=180.0,
            shift in 0.0f64..0.01,
        ) {
            let origin = GeographicCoordinate { latitude, longitude };
            let shifted = GeographicCoordinate {
                latitude: (latitude + shift).clamp(-90.0, 90.0),
                longitude: (longitude + shift).clamp(-180.0, 180.0),
            };
            let far = GeographicCoordinate {
                latitude: origin.latitude.sin().abs() * 80.0,
                longitude: origin.longitude.cos().abs() * 170.0,
            };
            let near = origin.haversine_distance_meters(&shifted);
            let distant = origin.haversine_distance_meters(&far);
            assert!(near < distant);
        }
    }
}
