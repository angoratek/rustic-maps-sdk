//! Easing primitives used by the reference applications' camera moves.
//!
//! The platform frame pump supplies every timestamp; these helpers are pure.
//! All durations are milliseconds so the same values travel across the four
//! targets without a nanosecond conversion at each call site.

use crate::course::{FULL_CIRCLE_DEGREES, HALF_CIRCLE_DEGREES};

/// The follow camera bearing's exponential chase time constant.
pub const FOLLOW_BEARING_CHASE_TIME_CONSTANT_MILLISECONDS: f64 = 150.0;

/// The chase snaps to its target under this remaining angular distance; the
/// eased value is visually identical to the target.
pub const FOLLOW_BEARING_SNAP_DISTANCE_DEGREES: f64 = 0.05;

/// The first frame of a chase carries one display frame's worth of catching
/// up: one 60 Hz frame.
pub const CHASE_FIRST_FRAME_DELTA_MILLISECONDS: f64 = 1000.0 / 60.0;

/// Smoothstep easing (`3t² − 2t³`): eased in and eased out.
///
/// The input is clamped into `[0, 1]`, so a caller may pass an unclamped
/// progress fraction.
pub fn smoothstep_eased_progress(linear_progress: f64) -> f64 {
    let clamped_progress = linear_progress.clamp(0.0, 1.0);
    clamped_progress * clamped_progress * (3.0 - 2.0 * clamped_progress)
}

/// The camera bearing's smallest off-north angular distance in degrees,
/// always in `[0, 180]`.
pub fn bearing_off_north_degrees(bearing_degrees: f64) -> f64 {
    let normalized_degrees =
        ((bearing_degrees % FULL_CIRCLE_DEGREES) + FULL_CIRCLE_DEGREES) % FULL_CIRCLE_DEGREES;
    if normalized_degrees <= HALF_CIRCLE_DEGREES {
        normalized_degrees
    } else {
        FULL_CIRCLE_DEGREES - normalized_degrees
    }
}

/// The shortest signed angular difference `to − from` in degrees, wrapped to
/// `[-180, 180]`.
pub fn signed_angular_difference_degrees(to_degrees: f64, from_degrees: f64) -> f64 {
    let mut difference = (to_degrees - from_degrees) % FULL_CIRCLE_DEGREES;
    if difference > HALF_CIRCLE_DEGREES {
        difference -= FULL_CIRCLE_DEGREES;
    }
    if difference < -HALF_CIRCLE_DEGREES {
        difference += FULL_CIRCLE_DEGREES;
    }
    difference
}

/// A linear-in-time, smoothstep-eased value move begun with a frame-clock
/// timestamp.
///
/// The compass's eased north-up return and the 2D/3D pitch swing both run as
/// this timed ease over the design language's one eased-camera-move duration.
#[derive(Debug, Clone, PartialEq)]
pub struct SmoothstepTimedEase {
    start_milliseconds: Option<f64>,
    from_value: f64,
    to_value: f64,
    duration_milliseconds: f64,
}

impl SmoothstepTimedEase {
    /// Creates an idle ease with no target.
    pub fn new() -> Self {
        Self {
            start_milliseconds: None,
            from_value: 0.0,
            to_value: 0.0,
            duration_milliseconds: 0.0,
        }
    }

    /// Begins, or restarts, an ease from one value to another.
    pub fn begin(
        &mut self,
        start_milliseconds: f64,
        from_value: f64,
        to_value: f64,
        duration_milliseconds: f64,
    ) {
        self.start_milliseconds = Some(start_milliseconds);
        self.from_value = from_value;
        self.to_value = to_value;
        self.duration_milliseconds = duration_milliseconds;
    }

    /// Stops an in-flight ease without applying anything.
    pub fn reset(&mut self) {
        self.start_milliseconds = None;
    }

    /// Whether an ease is in flight.
    pub fn is_running(&self) -> bool {
        self.start_milliseconds.is_some()
    }

    /// The ease's target value.
    pub fn target_value(&self) -> f64 {
        self.to_value
    }

    /// Advances the ease at `now_milliseconds`.
    ///
    /// Returns the current eased value and whether the move finished on this
    /// call. An idle ease reports its target and `true`.
    pub fn advance(&mut self, now_milliseconds: f64) -> (f64, bool) {
        let Some(start_milliseconds) = self.start_milliseconds else {
            return (self.to_value, true);
        };
        let elapsed_milliseconds = now_milliseconds - start_milliseconds;
        if self.duration_milliseconds <= 0.0 || elapsed_milliseconds >= self.duration_milliseconds {
            self.start_milliseconds = None;
            return (self.to_value, true);
        }
        let progress = elapsed_milliseconds / self.duration_milliseconds;
        let eased_value = self.from_value
            + (self.to_value - self.from_value) * smoothstep_eased_progress(progress);
        (eased_value, false)
    }
}

impl Default for SmoothstepTimedEase {
    fn default() -> Self {
        Self::new()
    }
}

/// An exponential chase toward a moving target.
///
/// The follow camera's bearing chases the median-smoothed course this way: a
/// time-constant chase settles on its own when the target stops moving, and
/// snaps when the remaining angular distance falls under
/// [`FOLLOW_BEARING_SNAP_DISTANCE_DEGREES`].
#[derive(Debug, Clone, PartialEq)]
pub struct ExponentialValueChase {
    time_constant_milliseconds: f64,
    last_frame_milliseconds: Option<f64>,
}

impl ExponentialValueChase {
    /// Creates a chase with the given time constant in milliseconds.
    pub fn new(time_constant_milliseconds: f64) -> Self {
        Self {
            time_constant_milliseconds,
            last_frame_milliseconds: None,
        }
    }

    /// Creates a chase with the follow camera's token time constant.
    pub fn with_follow_bearing_time_constant() -> Self {
        Self::new(FOLLOW_BEARING_CHASE_TIME_CONSTANT_MILLISECONDS)
    }

    /// The chase's time constant in milliseconds.
    pub fn time_constant_milliseconds(&self) -> f64 {
        self.time_constant_milliseconds
    }

    /// Advances the chase toward `target` at `now_milliseconds`, returning the
    /// next current value.
    ///
    /// The first call after a reset folds one display frame's worth of
    /// catching up. A frame clock that goes backwards is treated as a first
    /// frame. A non-finite or non-positive time constant snaps to the target
    /// rather than producing a meaningless value.
    pub fn advance(&mut self, current: f64, target: f64, now_milliseconds: f64) -> f64 {
        let last_milliseconds = self.last_frame_milliseconds;
        self.last_frame_milliseconds = Some(now_milliseconds);
        let delta_time_milliseconds = match last_milliseconds {
            Some(last) if now_milliseconds >= last => now_milliseconds - last,
            _ => CHASE_FIRST_FRAME_DELTA_MILLISECONDS,
        };
        let delta_degrees = signed_angular_difference_degrees(target, current);
        if delta_degrees.abs() <= FOLLOW_BEARING_SNAP_DISTANCE_DEGREES
            || !self.time_constant_milliseconds.is_finite()
            || self.time_constant_milliseconds <= 0.0
        {
            return target;
        }
        let follow_factor =
            1.0 - (-delta_time_milliseconds / self.time_constant_milliseconds).exp();
        current + delta_degrees * follow_factor
    }

    /// Clears the frame clock so the next call is treated as a first frame.
    pub fn reset(&mut self) {
        self.last_frame_milliseconds = None;
    }
}
