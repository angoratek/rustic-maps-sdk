//! Elevation record types.

use crate::coordinate::GeographicCoordinate;
use serde::{Deserialize, Serialize};

/// Elevation sampled at one coordinate.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ElevationPoint {
    /// Position the sample belongs to.
    pub coordinate: GeographicCoordinate,
    /// Elevation above mean sea level in meters.
    pub elevation_meters: f64,
}

/// Elevation samples along a route used for profile rendering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ElevationProfile {
    /// Samples in traversal order.
    pub points: Vec<ElevationPoint>,
}

impl ElevationProfile {
    /// Largest elevation gain along the profile, in meters.
    ///
    /// # Returns
    /// The greatest rise from any earlier sample to a later one, in traversal
    /// order. An empty profile reports zero. Descents do not reduce the
    /// result.
    pub fn maximum_elevation_gain_meters(&self) -> f64 {
        let mut minimum_so_far = f64::INFINITY;
        let mut maximum_gain = 0.0_f64;
        for point in &self.points {
            minimum_so_far = minimum_so_far.min(point.elevation_meters);
            maximum_gain = maximum_gain.max(point.elevation_meters - minimum_so_far);
        }
        maximum_gain
    }
}

#[cfg(test)]
mod tests {
    use super::{ElevationPoint, ElevationProfile};
    use crate::coordinate::GeographicCoordinate;

    fn point(latitude: f64, elevation_meters: f64) -> ElevationPoint {
        ElevationPoint {
            coordinate: GeographicCoordinate::from_degrees(latitude, 0.0).unwrap(),
            elevation_meters,
        }
    }

    #[test]
    fn maximum_gain_ignores_dips_after_climbs() {
        let profile = ElevationProfile {
            points: vec![
                point(0.0, 100.0),
                point(1.0, 400.0),
                point(2.0, 250.0),
                point(3.0, 380.0),
            ],
        };
        assert_eq!(profile.maximum_elevation_gain_meters(), 300.0);
    }

    #[test]
    fn empty_profile_has_zero_gain() {
        assert_eq!(
            ElevationProfile { points: vec![] }.maximum_elevation_gain_meters(),
            0.0
        );
    }
}
