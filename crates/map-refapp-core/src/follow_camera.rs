//! The navigation follow-camera state machine.
//!
//! While a guided trip runs the camera holds a fixed deep zoom, centres on the
//! guided fix and eases its bearing toward a median-smoothed
//! course-over-ground. A user gesture breaks follow immediately; a later fix
//! re-targets the state but does not snap the visitor's panned view back. The
//! my-location control re-engages explicitly. The pre-trip camera is captured
//! at trip start and restored at trip end.

use crate::course::{COURSE_SMOOTHING_WINDOW_FIXES, CourseMedianSmoother};

/// The follow camera's fixed deep zoom at trip start.
pub const NAVIGATION_FOLLOW_ZOOM: f64 = 17.0;

/// The follow camera's zoom clamp toward trip end.
pub const NAVIGATION_FOLLOW_ZOOM_MAXIMUM: f64 = 17.5;

/// The follow camera's pitch in the 3D plane toggle.
pub const NAVIGATION_FOLLOW_PITCH_3D_DEGREES: f64 = 45.0;

/// The follow camera's engage, gesture-break and re-engage states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FollowPhase {
    /// No trip is running.
    Disengaged,
    /// The camera follows the trip's fixes.
    Engaged,
    /// A user gesture broke follow; only an explicit re-engage resumes it.
    BrokenByGesture,
}

/// How the follow camera treats its bearing while a trip runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FollowBearingMode {
    /// The camera turns with the trip's smoothed course — the default.
    CourseUp,
    /// The visitor chose north-up: the camera stays at bearing 0 whatever
    /// the fixes report, until course-up is chosen again.
    NorthUp,
}

/// The camera values captured at trip start and restored at trip end.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreTripCamera {
    /// Camera centre latitude in degrees.
    pub center_latitude_degrees: f64,
    /// Camera centre longitude in degrees.
    pub center_longitude_degrees: f64,
    /// Camera zoom level.
    pub zoom_level: f64,
    /// Camera bearing in degrees clockwise from north.
    pub bearing_degrees: f64,
    /// Camera pitch in degrees.
    pub pitch_degrees: f64,
}

/// The follow camera's targets and phase.
#[derive(Debug, Clone, PartialEq)]
pub struct FollowCamera {
    phase: FollowPhase,
    pre_trip_camera: Option<PreTripCamera>,
    target_center_latitude_degrees: f64,
    target_center_longitude_degrees: f64,
    target_zoom_level: f64,
    target_bearing_degrees: Option<f64>,
    bearing_mode: FollowBearingMode,
    course_smoother: CourseMedianSmoother,
}

impl FollowCamera {
    /// Creates a disengaged camera with the default course-smoothing window.
    pub fn new() -> Self {
        Self::with_smoothing_window(COURSE_SMOOTHING_WINDOW_FIXES)
    }

    /// Creates a disengaged camera with a specific course-smoothing window.
    pub fn with_smoothing_window(window_fix_count: usize) -> Self {
        Self {
            phase: FollowPhase::Disengaged,
            pre_trip_camera: None,
            target_center_latitude_degrees: 0.0,
            target_center_longitude_degrees: 0.0,
            target_zoom_level: 0.0,
            target_bearing_degrees: None,
            bearing_mode: FollowBearingMode::CourseUp,
            course_smoother: CourseMedianSmoother::new(window_fix_count),
        }
    }

    /// The current follow phase.
    pub fn phase(&self) -> FollowPhase {
        self.phase
    }

    /// The captured pre-trip camera, or `None` outside a trip.
    pub fn pre_trip_camera(&self) -> Option<PreTripCamera> {
        self.pre_trip_camera
    }

    /// The target centre latitude in degrees.
    pub fn target_center_latitude_degrees(&self) -> f64 {
        self.target_center_latitude_degrees
    }

    /// The target centre longitude in degrees.
    pub fn target_center_longitude_degrees(&self) -> f64 {
        self.target_center_longitude_degrees
    }

    /// The target zoom level.
    pub fn target_zoom_level(&self) -> f64 {
        self.target_zoom_level
    }

    /// The target bearing in degrees, or `None` until a fix reports a course.
    pub fn target_bearing_degrees(&self) -> Option<f64> {
        self.target_bearing_degrees
    }

    /// How the camera treats its bearing: course-up or north-up.
    pub fn bearing_mode(&self) -> FollowBearingMode {
        self.bearing_mode
    }

    /// Chooses how the camera treats its bearing.
    ///
    /// North-up pins the target at bearing 0 and stops the course chase, so
    /// every following fix keeps the visitor's choice. Course-up resumes the
    /// eased chase on the next fix.
    ///
    /// # Parameters
    /// - `bearing_mode` — the mode the visitor chose.
    pub fn set_bearing_mode(&mut self, bearing_mode: FollowBearingMode) {
        self.bearing_mode = bearing_mode;
        self.course_smoother.reset();
        if bearing_mode == FollowBearingMode::NorthUp {
            self.target_bearing_degrees = Some(0.0);
        } else {
            self.target_bearing_degrees = None;
        }
    }

    /// Applies the trip's follow camera.
    ///
    /// Captures the pre-trip camera for the later restore, applies the fixed
    /// deep zoom immediately and leaves centre and bearing at their current
    /// values until the first location fix re-targets them.
    pub fn engage_trip(
        &mut self,
        current_center_latitude_degrees: f64,
        current_center_longitude_degrees: f64,
        current_zoom_level: f64,
        current_bearing_degrees: f64,
        current_pitch_degrees: f64,
    ) {
        self.pre_trip_camera = Some(PreTripCamera {
            center_latitude_degrees: current_center_latitude_degrees,
            center_longitude_degrees: current_center_longitude_degrees,
            zoom_level: current_zoom_level,
            bearing_degrees: current_bearing_degrees,
            pitch_degrees: current_pitch_degrees,
        });
        self.phase = FollowPhase::Engaged;
        self.target_center_latitude_degrees = current_center_latitude_degrees;
        self.target_center_longitude_degrees = current_center_longitude_degrees;
        self.target_zoom_level = NAVIGATION_FOLLOW_ZOOM;
        self.target_bearing_degrees = None;
        self.bearing_mode = FollowBearingMode::CourseUp;
        self.course_smoother.reset();
    }

    /// A user gesture (drag, pinch or rotate) breaks follow immediately.
    pub fn break_on_gesture(&mut self) {
        if self.phase == FollowPhase::Engaged {
            self.phase = FollowPhase::BrokenByGesture;
        }
    }

    /// Applies one location fix of the active trip.
    ///
    /// The fix always re-targets centre, smoothed bearing and ramped zoom so a
    /// re-engagement is current, but a follow broken by a gesture stays broken:
    /// only [`Self::reengage`] resumes it, so a later fix never snaps the
    /// visitor's panned view back.
    pub fn on_fix(
        &mut self,
        latitude_degrees: f64,
        longitude_degrees: f64,
        course_heading_degrees: Option<f64>,
        remaining_distance_fraction: f64,
    ) {
        if self.phase != FollowPhase::BrokenByGesture {
            self.phase = FollowPhase::Engaged;
        }
        self.target_center_latitude_degrees = latitude_degrees;
        self.target_center_longitude_degrees = longitude_degrees;
        self.target_zoom_level = follow_zoom_toward_trip_end(remaining_distance_fraction);
        if let Some(course_degrees) = course_heading_degrees {
            self.course_smoother.add(course_degrees);
            if self.bearing_mode == FollowBearingMode::CourseUp {
                self.target_bearing_degrees = self.course_smoother.median();
            }
        }
        if self.bearing_mode == FollowBearingMode::NorthUp {
            self.target_bearing_degrees = Some(0.0);
        }
    }

    /// The my-location control's explicit re-engage.
    pub fn reengage(&mut self) {
        if self.phase == FollowPhase::BrokenByGesture {
            self.phase = FollowPhase::Engaged;
        }
    }

    /// Ends the trip, returning the captured pre-trip camera for the caller to
    /// restore.
    ///
    /// The camera returns to [`FollowPhase::Disengaged`] even when no camera
    /// was captured.
    pub fn restore_pre_trip_camera(&mut self) -> Option<PreTripCamera> {
        let captured_camera = self.pre_trip_camera;
        self.phase = FollowPhase::Disengaged;
        self.pre_trip_camera = None;
        self.target_bearing_degrees = None;
        self.bearing_mode = FollowBearingMode::CourseUp;
        self.course_smoother.reset();
        captured_camera
    }
}

impl Default for FollowCamera {
    fn default() -> Self {
        Self::new()
    }
}

/// The follow camera's zoom toward trip end.
///
/// The base zoom at the trip's start ramps linearly with the travelled
/// fraction to the maximum at arrival. The remaining fraction is clamped into
/// `[0, 1]`, so a malformed progress report cannot leave the documented range.
pub fn follow_zoom_toward_trip_end(remaining_distance_fraction: f64) -> f64 {
    let clamped_remaining_fraction = remaining_distance_fraction.clamp(0.0, 1.0);
    let travelled_fraction = 1.0 - clamped_remaining_fraction;
    NAVIGATION_FOLLOW_ZOOM
        + (NAVIGATION_FOLLOW_ZOOM_MAXIMUM - NAVIGATION_FOLLOW_ZOOM) * travelled_fraction
}
