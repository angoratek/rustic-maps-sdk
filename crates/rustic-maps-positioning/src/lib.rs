//! Device positioning for the map SDK.
//!
//! One controller validates the fixes a host's location source reports,
//! de-duplicates them, smooths their course and tracks staleness, so every
//! consumer — the renderer's location indicator, the follow camera and the
//! navigation controller — reads one consistent position stream.
//!
//! Hosts own the operating system's positioning API and its permission
//! prompt: Android supplies `LocationManager` (or a fused engine), iOS
//! supplies Core Location, the browser supplies `navigator.geolocation`, and a
//! desktop application supplies its own source or the simulated source in
//! this crate. The controller never requests a permission and never touches
//! the network.

use geospatial_types::{GeographicCoordinate, RouteData, SDKError};
use rustic_maps_navigation::{RouteWalk, RouteWalkError, SimulatedTripPace};
use std::collections::VecDeque;

/// The fixes kept for course smoothing by default.
pub const DEFAULT_COURSE_SMOOTHING_WINDOW_FIXES: usize = 5;

/// The age after which a fix is considered stale by default, in seconds.
pub const DEFAULT_STALE_AFTER_SECONDS: f64 = 10.0;

/// Direction in degrees of a full circle.
const FULL_CIRCLE_DEGREES: f64 = 360.0;

/// One position report from a host's location source.
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

/// Why a source could not produce fixes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PositionSourceStatus {
    /// The user denied the permission the source needs.
    #[error("location permission was denied")]
    PermissionDenied,
    /// Location services are disabled on the device or by policy.
    #[error("location services are disabled")]
    ServiceDisabled,
    /// No provider can answer right now.
    #[error("no location provider is available")]
    Unavailable,
    /// The source did not answer inside its timeout.
    #[error("the location source timed out")]
    Timeout,
}

/// Why a fix was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PositioningError {
    /// The coordinate is outside its canonical latitude or longitude range.
    #[error("the position fix coordinate is invalid: {detail}")]
    InvalidCoordinate {
        /// The coordinate's validation detail.
        detail: String,
    },
    /// The accuracy is negative or not finite.
    #[error("the position fix accuracy must be a finite, non-negative number")]
    InvalidAccuracy,
    /// The course is not finite.
    #[error("the position fix course must be a finite number of degrees")]
    InvalidCourse,
    /// The timestamp is negative or not finite.
    #[error("the position fix timestamp must be finite and non-negative")]
    InvalidTimestamp,
}

impl From<SDKError> for PositioningError {
    fn from(coordinate_error: SDKError) -> Self {
        Self::InvalidCoordinate {
            detail: coordinate_error.to_string(),
        }
    }
}

/// The controller's latest accepted state.
#[derive(Debug, Clone, PartialEq)]
pub struct PositionState {
    /// The most recent accepted fix.
    pub fix: PositionFix,
    /// The smoothed course in degrees clockwise from true north, when any
    /// accepted fix carried a course.
    pub smoothed_course_heading_degrees: Option<f64>,
    /// The last source failure that has not been followed by a fix.
    pub source_status: Option<PositionSourceStatus>,
}

/// Validates, de-duplicates and smooths one host's position stream.
///
/// Feed it every fix and source failure the platform reports, then read the
/// accepted state from [`Self::state`]. The controller performs no I/O, never
/// requests a permission, never spawns a thread, and holds no internal locks:
/// call it from the thread that owns it.
#[derive(Debug)]
pub struct PositionController {
    latest_fix: Option<PositionFix>,
    smoothed_course_heading_degrees: Option<f64>,
    course_window: VecDeque<f64>,
    maximum_course_samples: usize,
    stale_after_seconds: f64,
    source_status: Option<PositionSourceStatus>,
}

impl Default for PositionController {
    fn default() -> Self {
        Self::new()
    }
}

impl PositionController {
    /// Builds a controller with the default smoothing window and staleness
    /// threshold.
    ///
    /// # Returns
    /// A controller holding no fix and reporting no source failure.
    pub fn new() -> Self {
        Self::with_course_smoothing_window(DEFAULT_COURSE_SMOOTHING_WINDOW_FIXES)
    }

    /// Builds a controller whose course average keeps a fixed number of fixes.
    ///
    /// # Parameters
    /// - `maximum_course_samples` — how many of the most recent courses the
    ///   circular average keeps. Zero is treated as one: a single sample still
    ///   carries a course.
    ///
    /// # Returns
    /// A controller holding no fix and reporting no source failure.
    pub fn with_course_smoothing_window(maximum_course_samples: usize) -> Self {
        Self {
            latest_fix: None,
            smoothed_course_heading_degrees: None,
            course_window: VecDeque::new(),
            maximum_course_samples: maximum_course_samples.max(1),
            stale_after_seconds: DEFAULT_STALE_AFTER_SECONDS,
            source_status: None,
        }
    }

    /// Replaces the staleness threshold, in seconds.
    ///
    /// # Parameters
    /// - `stale_after_seconds` — the age beyond which [`Self::is_stale`]
    ///   reports true. A non-finite or non-positive value is ignored and the
    ///   previous threshold stays in force.
    pub fn set_stale_after_seconds(&mut self, stale_after_seconds: f64) {
        if stale_after_seconds.is_finite() && stale_after_seconds > 0.0 {
            self.stale_after_seconds = stale_after_seconds;
        }
    }

    /// Accepts one fix from the host's source.
    ///
    /// A fix that repeats the previous coordinate exactly is ignored for the
    /// course window: a stationary receiver must not invent a heading. A
    /// successful fix clears the last source failure.
    ///
    /// # Parameters
    /// - `fix` — the source's position report. The coordinate must be inside
    ///   its canonical ranges, and every reported scalar must be finite
    ///   (accuracy and timestamp also non-negative).
    ///
    /// # Returns
    /// `Ok(())` when the fix was accepted and became the latest state.
    ///
    /// # Errors
    /// Returns [`PositioningError::InvalidCoordinate`] when the latitude or
    /// longitude is outside its range, [`PositioningError::InvalidAccuracy`]
    /// when the accuracy is negative or not finite,
    /// [`PositioningError::InvalidCourse`] when the course is not finite, and
    /// [`PositioningError::InvalidTimestamp`] when the timestamp is negative or
    /// not finite. A refused fix leaves the controller unchanged.
    pub fn feed_fix(&mut self, fix: PositionFix) -> Result<(), PositioningError> {
        let validated_coordinate =
            GeographicCoordinate::from_degrees(fix.coordinate.latitude, fix.coordinate.longitude)?;
        if let Some(accuracy) = fix.horizontal_accuracy_meters
            && (!accuracy.is_finite() || accuracy < 0.0)
        {
            return Err(PositioningError::InvalidAccuracy);
        }
        if let Some(course) = fix.course_heading_degrees
            && !course.is_finite()
        {
            return Err(PositioningError::InvalidCourse);
        }
        if let Some(timestamp) = fix.timestamp_seconds
            && (!timestamp.is_finite() || timestamp < 0.0)
        {
            return Err(PositioningError::InvalidTimestamp);
        }
        let previous_coordinate = self.latest_fix.as_ref().map(|latest| latest.coordinate);
        let accepted_fix = PositionFix {
            coordinate: validated_coordinate,
            ..fix
        };
        if let Some(course) = accepted_fix.course_heading_degrees
            && previous_coordinate != Some(accepted_fix.coordinate)
        {
            self.record_course_sample(normalized_bearing_degrees(course));
        }
        self.latest_fix = Some(accepted_fix);
        self.source_status = None;
        Ok(())
    }

    /// Records a source failure.
    ///
    /// The latest fix stays available (with its own timestamp) so a consumer
    /// can show the last known position while the source recovers; a later
    /// successful fix clears the status.
    ///
    /// # Parameters
    /// - `status` — why the source could not produce a fix.
    pub fn feed_source_failure(&mut self, status: PositionSourceStatus) {
        self.source_status = Some(status);
    }

    /// The latest accepted fix, or `None` before the first accepted one.
    ///
    /// # Returns
    /// A borrow of the latest accepted fix. It remains available after a
    /// recorded source failure, so a consumer can keep showing the last known
    /// position.
    pub fn latest_fix(&self) -> Option<&PositionFix> {
        self.latest_fix.as_ref()
    }

    /// The smoothed course across the most recent fixes that reported one.
    ///
    /// # Returns
    /// The circular mean in degrees clockwise from true north, or `None` while
    /// no accepted fix has carried a course.
    pub fn smoothed_course_heading_degrees(&self) -> Option<f64> {
        self.smoothed_course_heading_degrees
    }

    /// The last source failure, or `None` when the last event was a fix.
    ///
    /// # Returns
    /// The recorded failure until a later fix clears it.
    pub fn source_status(&self) -> Option<PositionSourceStatus> {
        self.source_status
    }

    /// Whether the latest fix is older than the staleness threshold.
    ///
    /// # Parameters
    /// - `now_seconds` — the current instant in seconds since the Unix epoch.
    ///
    /// # Returns
    /// `true` when the latest fix is older than the configured threshold. A
    /// fix without a timestamp is never stale: the source did not claim an
    /// age. Before the first fix, nothing is stale.
    pub fn is_stale(&self, now_seconds: f64) -> bool {
        match self
            .latest_fix
            .as_ref()
            .and_then(|fix| fix.timestamp_seconds)
        {
            Some(fix_timestamp) => now_seconds - fix_timestamp > self.stale_after_seconds,
            None => false,
        }
    }

    /// The current state, or `None` before the first accepted fix.
    ///
    /// # Returns
    /// A snapshot of the latest fix, its smoothed course and any active source
    /// failure.
    pub fn state(&self) -> Option<PositionState> {
        Some(PositionState {
            fix: self.latest_fix.clone()?,
            smoothed_course_heading_degrees: self.smoothed_course_heading_degrees,
            source_status: self.source_status,
        })
    }

    /// Appends one course sample and recomputes the circular average.
    fn record_course_sample(&mut self, course_degrees: f64) {
        self.course_window.push_back(course_degrees);
        while self.course_window.len() > self.maximum_course_samples {
            self.course_window.pop_front();
        }
        self.smoothed_course_heading_degrees = circular_mean_degrees(&self.course_window);
    }
}

/// The circular mean of a set of bearings: the average unit vector's angle.
/// Bearings either side of north (for example 350° and 10°) average to 0°,
/// which a linear mean would place at 180°.
fn circular_mean_degrees(bearings: &VecDeque<f64>) -> Option<f64> {
    if bearings.is_empty() {
        return None;
    }
    let (sum_sine, sum_cosine) = bearings.iter().fold((0.0, 0.0), |accumulator, bearing| {
        let bearing_radians = bearing.to_radians();
        (
            accumulator.0 + bearing_radians.sin(),
            accumulator.1 + bearing_radians.cos(),
        )
    });
    if sum_sine.abs() < f64::EPSILON && sum_cosine.abs() < f64::EPSILON {
        // Opposite headings cancel exactly; keep the most recent sample
        // rather than answering a meaningless angle.
        return bearings.back().copied();
    }
    Some(normalized_bearing_degrees(
        sum_sine.atan2(sum_cosine).to_degrees(),
    ))
}

/// Wraps a bearing into `[0, 360)`.
fn normalized_bearing_degrees(bearing_degrees: f64) -> f64 {
    bearing_degrees.rem_euclid(FULL_CIRCLE_DEGREES)
}

/// A position source that walks a route at a fixed pace: the deterministic
/// source for demos, tests and desktop applications that do not consume an
/// operating-system location source.
#[derive(Debug)]
pub struct SimulatedPositionSource {
    route_walk: RouteWalk,
    accuracy_meters: f64,
}

impl SimulatedPositionSource {
    /// Builds the source over a route and pace.
    ///
    /// # Parameters
    /// - `route` — the planned route to traverse.
    /// - `pace` — the travel speed used to convert elapsed time into distance.
    /// - `accuracy_meters` — the horizontal accuracy reported by every
    ///   generated fix. A non-finite or negative value is replaced by zero.
    ///
    /// # Errors
    /// Returns [`RouteWalkError::InvalidPace`] when the pace's speed or
    /// multiplier is non-finite or not strictly positive, and
    /// [`RouteWalkError::NoUsableGeometry`] when the route carries no usable
    /// geometry to walk.
    pub fn new(
        route: &RouteData,
        pace: SimulatedTripPace,
        accuracy_meters: f64,
    ) -> Result<Self, RouteWalkError> {
        Ok(Self {
            route_walk: RouteWalk::new(route, pace)?,
            accuracy_meters: if accuracy_meters.is_finite() && accuracy_meters >= 0.0 {
                accuracy_meters
            } else {
                0.0
            },
        })
    }

    /// The fix at an elapsed time of simulated travel.
    ///
    /// # Parameters
    /// - `elapsed_seconds` — seconds since the start of the simulated trip. A
    ///   negative value is clamped to the start and a value past the route's
    ///   end is clamped to its final fix.
    ///
    /// # Returns
    /// A fix on the route polyline carrying the configured accuracy, the
    /// segment's course and the elapsed time as its timestamp.
    pub fn fix_at_elapsed_seconds(&self, elapsed_seconds: f64) -> PositionFix {
        let simulated_fix = self.route_walk.fix_at_elapsed_seconds(elapsed_seconds);
        PositionFix {
            coordinate: simulated_fix.coordinate,
            horizontal_accuracy_meters: Some(self.accuracy_meters),
            course_heading_degrees: simulated_fix.course_heading_degrees,
            timestamp_seconds: Some(simulated_fix.elapsed_seconds),
        }
    }

    /// The route's total simulated duration, in seconds.
    ///
    /// # Returns
    /// The time the configured pace needs to traverse the whole route.
    pub fn total_duration_seconds(&self) -> f64 {
        self.route_walk.total_duration_seconds()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PositionController, PositionFix, PositionSourceStatus, PositioningError,
        SimulatedPositionSource,
    };
    use geospatial_types::{
        BoundingBox, GeographicCoordinate, ManeuverType, RouteData, RouteLeg, RouteStep,
    };
    use rustic_maps_navigation::SimulatedTripPace;

    fn coordinate(latitude: f64, longitude: f64) -> GeographicCoordinate {
        GeographicCoordinate {
            latitude,
            longitude,
        }
    }

    fn fix(latitude: f64, longitude: f64, course: Option<f64>) -> PositionFix {
        PositionFix {
            coordinate: coordinate(latitude, longitude),
            horizontal_accuracy_meters: Some(5.0),
            course_heading_degrees: course,
            timestamp_seconds: Some(0.0),
        }
    }

    fn route_over(shape: Vec<GeographicCoordinate>) -> RouteData {
        RouteData {
            distance_meters: 0.0,
            duration_seconds: 0.0,
            bounding_box: BoundingBox {
                south_west: coordinate(0.0, 0.0),
                north_east: coordinate(1.0, 1.0),
            },
            geometry: Vec::new(),
            legs: vec![RouteLeg {
                distance_meters: 0.0,
                duration_seconds: 0.0,
                steps: vec![RouteStep {
                    distance_meters: 0.0,
                    duration_seconds: 0.0,
                    maneuver_type: ManeuverType::Continue,
                    instruction_display_text: String::new(),
                    instruction_speech_text: String::new(),
                    shape,
                    lane_guidances: Vec::new(),
                }],
            }],
        }
    }

    #[test]
    fn invalid_fixes_are_refused() {
        let mut controller = PositionController::new();
        assert!(matches!(
            controller.feed_fix(fix(91.0, 0.0, None)),
            Err(PositioningError::InvalidCoordinate { .. })
        ));
        assert!(matches!(
            controller.feed_fix(PositionFix {
                coordinate: coordinate(0.0, 0.0),
                horizontal_accuracy_meters: Some(-1.0),
                course_heading_degrees: None,
                timestamp_seconds: None,
            }),
            Err(PositioningError::InvalidAccuracy)
        ));
        assert!(controller.latest_fix().is_none());
    }

    #[test]
    fn course_smoothing_wraps_around_north() {
        let mut controller = PositionController::with_course_smoothing_window(3);
        controller
            .feed_fix(fix(38.0, -121.0, Some(350.0)))
            .expect("a valid fix is accepted");
        controller
            .feed_fix(fix(38.001, -121.0, Some(10.0)))
            .expect("a valid fix is accepted");
        let smoothed = controller
            .smoothed_course_heading_degrees()
            .expect("the fixes carried courses");
        assert!(
            smoothed < 5.0 || smoothed > 355.0,
            "350 and 10 average to about north, got {smoothed}"
        );
    }

    #[test]
    fn a_stationary_fix_does_not_change_the_course() {
        let mut controller = PositionController::new();
        controller
            .feed_fix(fix(38.0, -121.0, Some(90.0)))
            .expect("a valid fix is accepted");
        controller
            .feed_fix(fix(38.0, -121.0, Some(270.0)))
            .expect("a valid repeat is accepted");
        assert_eq!(controller.smoothed_course_heading_degrees(), Some(90.0));
    }

    #[test]
    fn staleness_and_source_failures_are_reported() {
        let mut controller = PositionController::new();
        controller.set_stale_after_seconds(5.0);
        controller
            .feed_fix(PositionFix {
                timestamp_seconds: Some(100.0),
                ..fix(38.0, -121.0, None)
            })
            .expect("a valid fix is accepted");
        assert!(!controller.is_stale(104.0));
        assert!(controller.is_stale(106.0));
        controller.feed_source_failure(PositionSourceStatus::Timeout);
        assert_eq!(
            controller.source_status(),
            Some(PositionSourceStatus::Timeout)
        );
        controller
            .feed_fix(fix(38.001, -121.0, None))
            .expect("a recovering fix is accepted");
        assert_eq!(controller.source_status(), None);
    }

    #[test]
    fn the_simulated_source_walks_its_route_with_a_course() {
        let route = route_over(vec![
            coordinate(38.5769, -121.4950),
            coordinate(38.5764, -121.4950),
        ]);
        let source = SimulatedPositionSource::new(
            &route,
            SimulatedTripPace {
                speed_meters_per_second: 1.4,
                speed_multiplier: 1.0,
            },
            5.0,
        )
        .expect("the route walks");
        let start_fix = source.fix_at_elapsed_seconds(0.0);
        let later_fix = source.fix_at_elapsed_seconds(5.0);
        assert!(later_fix.coordinate.latitude < start_fix.coordinate.latitude);
        let course = start_fix
            .course_heading_degrees
            .expect("a segment carries a course");
        assert!(
            (course - 180.0).abs() < 1.0,
            "southbound travel reports about 180 degrees, got {course}"
        );
        assert!(source.total_duration_seconds() > 0.0);
    }
}
