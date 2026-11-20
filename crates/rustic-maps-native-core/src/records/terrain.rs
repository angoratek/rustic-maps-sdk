//! Terrain elevation samples.

use super::GeographicCoordinate;
use crate::error::SdkError;

/// One terrain elevation sample at a coordinate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ElevationPoint {
    /// Position the sample belongs to.
    pub coordinate: GeographicCoordinate,
    /// Elevation above mean sea level in meters.
    pub elevation_meters: f64,
}

impl ElevationPoint {
    /// Builds the mirror record from a native elevation sample.
    pub(crate) fn from_native_elevation_point(
        native_elevation_point: &geospatial_types::ElevationPoint,
    ) -> Self {
        Self {
            coordinate: GeographicCoordinate::from_native_coordinate(
                &native_elevation_point.coordinate,
            ),
            elevation_meters: native_elevation_point.elevation_meters,
        }
    }

    /// Builds the native elevation sample from the mirror record — the
    /// foreign provider adapters' direction. The coordinate validates like
    /// every other geometry a provider returns; the elevation copies
    /// verbatim, because a provider may legitimately report a value below
    /// mean sea level.
    pub(crate) fn into_native_elevation_point(
        self,
    ) -> Result<geospatial_types::ElevationPoint, SdkError> {
        Ok(geospatial_types::ElevationPoint {
            coordinate: crate::projection::validated_coordinate(self.coordinate)?,
            elevation_meters: self.elevation_meters,
        })
    }
}
