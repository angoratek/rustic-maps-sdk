//! Position-fix record consumed by the navigation reducer.

use geospatial_types::GeographicCoordinate;

/// One raw position fix delivered to the navigation state machine.
///
/// Plain data by design: this record crosses the language-binding boundary, so
/// it stays serializable with no trait objects inside.
/// Optionality mirrors how platform fix pipelines report fields — a fix
/// always has a coordinate, but accuracy, course and a timestamp may be
/// absent depending on the source.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NavigationLocationUpdate {
    /// Raw position on the WGS 84 ellipsoid, before route snapping.
    pub coordinate: GeographicCoordinate,
    /// Radius, in meters, of the 68% horizontal-accuracy envelope of this fix
    /// (`None` when the source does not report accuracy).
    pub horizontal_accuracy_meters: Option<f64>,
    /// Direction of travel in degrees clockwise from true north
    /// (`None` when the source does not report a course).
    pub course_heading_degrees: Option<f64>,
    /// Fix instant in seconds since the Unix epoch (`None` for untimed sources).
    pub timestamp_seconds: Option<f64>,
}

impl NavigationLocationUpdate {
    /// Builds a fix carrying only its coordinate; the remaining metadata
    /// starts as unreported and can be filled in by the caller afterwards.
    pub fn from_coordinate(coordinate: GeographicCoordinate) -> Self {
        Self {
            coordinate,
            horizontal_accuracy_meters: None,
            course_heading_degrees: None,
            timestamp_seconds: None,
        }
    }
}
