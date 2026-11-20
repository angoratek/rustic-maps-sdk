//! Device position fixes and positioning source failures.

use super::GeographicCoordinate;

/// One device position report: a coordinate with the source's optional
/// accuracy, course and timestamp.
#[derive(Debug, Clone, PartialEq)]
pub struct PositionFix {
    /// The reported coordinate on the WGS 84 ellipsoid.
    pub coordinate: GeographicCoordinate,
    /// The radius in meters of the 68% horizontal-accuracy envelope, when the
    /// source reports one.
    pub horizontal_accuracy_meters: Option<f64>,
    /// Direction of travel in degrees clockwise from true north, when the
    /// source reports one.
    pub course_heading_degrees: Option<f64>,
    /// The fix's instant in seconds since the Unix epoch, when the source
    /// reports one.
    pub timestamp_seconds: Option<f64>,
}

impl PositionFix {
    /// Converts the contract record into the controller's fix.
    pub(crate) fn to_native_position_fix(
        &self,
    ) -> Result<rustic_maps_positioning::PositionFix, crate::error::SdkError> {
        Ok(rustic_maps_positioning::PositionFix {
            coordinate: self.coordinate.to_native_coordinate(),
            horizontal_accuracy_meters: self.horizontal_accuracy_meters,
            course_heading_degrees: self.course_heading_degrees,
            timestamp_seconds: self.timestamp_seconds,
        })
    }

    /// Converts the controller's fix into the contract record.
    pub(crate) fn from_native_position_fix(
        native_fix: &rustic_maps_positioning::PositionFix,
    ) -> Self {
        Self {
            coordinate: GeographicCoordinate::from_native_coordinate(&native_fix.coordinate),
            horizontal_accuracy_meters: native_fix.horizontal_accuracy_meters,
            course_heading_degrees: native_fix.course_heading_degrees,
            timestamp_seconds: native_fix.timestamp_seconds,
        }
    }
}

/// Why a positioning source could not produce fixes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionSourceError {
    /// The user denied the permission the source needs.
    PermissionDenied,
    /// Location services are disabled on the device or by policy.
    ServiceDisabled,
    /// No provider can answer right now.
    Unavailable,
    /// The source did not answer inside its timeout.
    Timeout,
}

impl PositionSourceError {
    /// Converts the contract enum into the controller's source status.
    pub(crate) fn to_native_source_status(self) -> rustic_maps_positioning::PositionSourceStatus {
        match self {
            Self::PermissionDenied => {
                rustic_maps_positioning::PositionSourceStatus::PermissionDenied
            }
            Self::ServiceDisabled => rustic_maps_positioning::PositionSourceStatus::ServiceDisabled,
            Self::Unavailable => rustic_maps_positioning::PositionSourceStatus::Unavailable,
            Self::Timeout => rustic_maps_positioning::PositionSourceStatus::Timeout,
        }
    }

    /// Converts the controller's source status into the contract enum.
    pub(crate) fn from_native_source_status(
        status: rustic_maps_positioning::PositionSourceStatus,
    ) -> Self {
        match status {
            rustic_maps_positioning::PositionSourceStatus::PermissionDenied => {
                Self::PermissionDenied
            }
            rustic_maps_positioning::PositionSourceStatus::ServiceDisabled => Self::ServiceDisabled,
            rustic_maps_positioning::PositionSourceStatus::Unavailable => Self::Unavailable,
            rustic_maps_positioning::PositionSourceStatus::Timeout => Self::Timeout,
        }
    }
}
