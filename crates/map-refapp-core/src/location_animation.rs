//! The location puck's interpolation between fixes.
//!
//! A shell hands the renderer one fix at a time; drawn as-is, the position
//! disc hops the distance between fixes and the accuracy ring jumps with it.
//! This module eases the drawn pose from the previous fix to the new one over
//! the fix interval, retargets when a fix lands mid-flight, and takes the
//! shorter arc for the course, so the puck glides instead of hopping.
//!
//! The animator holds no clock: the caller passes the time in, exactly like
//! the easing primitives beside it, so a test drives it deterministically and
//! every platform's frame pump reads the same math.

use crate::easing::signed_angular_difference_degrees;

/// The longest motion one fix may start, in milliseconds.
pub const LOCATION_ANIMATION_MAXIMUM_MILLISECONDS: f64 = 1000.0;

/// The shortest motion one fix starts, in milliseconds.
///
/// A burst of fixes closer together than this still animates for this long,
/// so a fast fix stream never degrades into teleports.
pub const LOCATION_ANIMATION_MINIMUM_MILLISECONDS: f64 = 120.0;

/// One measured position the animator receives.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocationFix {
    /// Measured latitude in degrees.
    pub latitude_degrees: f64,
    /// Measured longitude in degrees.
    pub longitude_degrees: f64,
    /// Course over ground in degrees clockwise from north, when reported.
    pub course_degrees: Option<f64>,
    /// Horizontal accuracy radius in meters, when reported.
    pub horizontal_accuracy_meters: Option<f64>,
    /// When the fix was received, in milliseconds on the caller's clock.
    pub received_at_milliseconds: f64,
}

/// One pose the renderer draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocationPose {
    /// Latitude to draw at, in degrees.
    pub latitude_degrees: f64,
    /// Longitude to draw at, in degrees.
    pub longitude_degrees: f64,
    /// Course to draw with, in degrees clockwise from north.
    pub course_degrees: Option<f64>,
    /// Accuracy radius to draw, in meters.
    pub horizontal_accuracy_meters: Option<f64>,
    /// Whether a motion is still running, so a shell knows to keep drawing.
    pub is_animating: bool,
}

impl LocationPose {
    /// The pose a fix draws when no motion is running.
    fn from_fix(fix: &LocationFix) -> Self {
        Self {
            latitude_degrees: fix.latitude_degrees,
            longitude_degrees: fix.longitude_degrees,
            course_degrees: fix.course_degrees,
            horizontal_accuracy_meters: fix.horizontal_accuracy_meters,
            is_animating: false,
        }
    }
}

/// The animator that turns a fix stream into a drawn pose.
#[derive(Debug, Default)]
pub struct LocationPoseAnimator {
    /// The pose the current motion started from.
    from_pose: Option<LocationPose>,
    /// The pose the current motion is heading to.
    to_pose: Option<LocationPose>,
    /// When the current motion started, in milliseconds.
    motion_started_at_milliseconds: f64,
    /// How long the current motion runs, in milliseconds.
    motion_duration_milliseconds: f64,
    /// When the previous fix was received, in milliseconds.
    previous_fix_received_at_milliseconds: Option<f64>,
}

impl LocationPoseAnimator {
    /// Builds an animator with no fix yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts or retargets the motion toward one fix.
    ///
    /// The first fix draws immediately. A later fix starts a motion from the
    /// pose currently drawn — never from the previous fix — so the drawn
    /// position is continuous however irregular the stream is.
    ///
    /// # Parameters
    /// - `fix` — the measured position, with its receive time.
    pub fn push_fix(&mut self, fix: LocationFix) {
        let target_pose = LocationPose::from_fix(&fix);
        let Some(from_pose) = self.pose_at(fix.received_at_milliseconds) else {
            self.from_pose = Some(target_pose);
            self.to_pose = Some(target_pose);
            self.motion_duration_milliseconds = 0.0;
            self.previous_fix_received_at_milliseconds = Some(fix.received_at_milliseconds);
            return;
        };
        let interval = self
            .previous_fix_received_at_milliseconds
            .map(|previous| fix.received_at_milliseconds - previous)
            .unwrap_or(0.0);
        self.motion_duration_milliseconds = interval.clamp(
            LOCATION_ANIMATION_MINIMUM_MILLISECONDS,
            LOCATION_ANIMATION_MAXIMUM_MILLISECONDS,
        );
        self.from_pose = Some(from_pose);
        self.to_pose = Some(target_pose);
        self.motion_started_at_milliseconds = fix.received_at_milliseconds;
        self.previous_fix_received_at_milliseconds = Some(fix.received_at_milliseconds);
    }

    /// The pose to draw at `now_milliseconds`.
    ///
    /// # Returns
    /// The interpolated pose, or `None` before the first fix. A clock that
    /// runs backwards clamps to the motion's start, so a shell's frame pump
    /// never draws a position outside the segment it is moving along.
    pub fn pose_at(&mut self, now_milliseconds: f64) -> Option<LocationPose> {
        let from_pose = self.from_pose?;
        let to_pose = self.to_pose?;
        if self.motion_duration_milliseconds <= 0.0 {
            return Some(LocationPose {
                is_animating: false,
                ..to_pose
            });
        }
        let elapsed = now_milliseconds - self.motion_started_at_milliseconds;
        let linear_progress = (elapsed / self.motion_duration_milliseconds).clamp(0.0, 1.0);
        if linear_progress >= 1.0 {
            return Some(LocationPose {
                is_animating: false,
                ..to_pose
            });
        }
        Some(LocationPose {
            latitude_degrees: interpolate(
                from_pose.latitude_degrees,
                to_pose.latitude_degrees,
                linear_progress,
            ),
            longitude_degrees: interpolate(
                from_pose.longitude_degrees,
                to_pose.longitude_degrees,
                linear_progress,
            ),
            course_degrees: interpolate_course(
                from_pose.course_degrees,
                to_pose.course_degrees,
                linear_progress,
            ),
            horizontal_accuracy_meters: interpolate_optional(
                from_pose.horizontal_accuracy_meters,
                to_pose.horizontal_accuracy_meters,
                linear_progress,
            ),
            is_animating: true,
        })
    }

    /// The pose the next motion starts from, or `None` before the first fix.
    pub fn current_pose(&mut self) -> Option<LocationPose> {
        self.pose_at(self.motion_started_at_milliseconds + self.motion_duration_milliseconds)
    }

    /// Forgets every fix, so the next one draws immediately.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Linear interpolation between two scalars.
fn interpolate(from: f64, to: f64, progress: f64) -> f64 {
    from + (to - from) * progress
}

/// Linear interpolation where an absent endpoint holds the other one.
fn interpolate_optional(from: Option<f64>, to: Option<f64>, progress: f64) -> Option<f64> {
    match (from, to) {
        (Some(from_value), Some(to_value)) => Some(interpolate(from_value, to_value, progress)),
        (Some(from_value), None) => Some(from_value),
        (None, Some(to_value)) => Some(to_value),
        (None, None) => None,
    }
}

/// Interpolates a course along the shorter arc, so 359° → 1° turns forward.
fn interpolate_course(from: Option<f64>, to: Option<f64>, progress: f64) -> Option<f64> {
    match (from, to) {
        (Some(from_degrees), Some(to_degrees)) => {
            let difference = signed_angular_difference_degrees(to_degrees, from_degrees);
            Some(normalize_degrees(from_degrees + difference * progress))
        }
        (Some(from_degrees), None) => Some(from_degrees),
        (None, Some(to_degrees)) => Some(to_degrees),
        (None, None) => None,
    }
}

/// Wraps an angle into `[0, 360)`.
fn normalize_degrees(angle_degrees: f64) -> f64 {
    let wrapped = angle_degrees % 360.0;
    if wrapped < 0.0 {
        wrapped + 360.0
    } else {
        wrapped
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LOCATION_ANIMATION_MAXIMUM_MILLISECONDS, LOCATION_ANIMATION_MINIMUM_MILLISECONDS,
        LocationFix, LocationPoseAnimator,
    };

    /// One fix at the given position and time.
    fn fix_at(latitude_degrees: f64, received_at_milliseconds: f64) -> LocationFix {
        LocationFix {
            latitude_degrees,
            longitude_degrees: 0.0,
            course_degrees: Some(0.0),
            horizontal_accuracy_meters: Some(10.0),
            received_at_milliseconds,
        }
    }

    #[test]
    fn the_first_fix_draws_immediately() {
        let mut animator = LocationPoseAnimator::new();
        animator.push_fix(fix_at(1.0, 0.0));
        let pose = animator.pose_at(0.0).expect("the first fix draws");
        assert_eq!(pose.latitude_degrees, 1.0);
        assert!(!pose.is_animating);
    }

    #[test]
    fn a_motion_is_half_way_at_half_its_duration() {
        let mut animator = LocationPoseAnimator::new();
        animator.push_fix(fix_at(0.0, 0.0));
        animator.push_fix(fix_at(1.0, 500.0));
        let pose = animator
            .pose_at(500.0 + 250.0)
            .expect("the motion is running");
        assert!((pose.latitude_degrees - 0.5).abs() < 1e-9);
        assert!(pose.is_animating);
        let settled = animator.pose_at(500.0 + 500.0).expect("the motion ends");
        assert_eq!(settled.latitude_degrees, 1.0);
        assert!(!settled.is_animating);
    }

    #[test]
    fn a_fix_arriving_mid_flight_retargets_without_a_jump() {
        let mut animator = LocationPoseAnimator::new();
        animator.push_fix(fix_at(0.0, 0.0));
        animator.push_fix(fix_at(1.0, 500.0));
        let before = animator.pose_at(750.0).expect("the motion is running");
        // The new fix lands at the moment the pose is sampled.
        animator.push_fix(fix_at(2.0, 750.0));
        let after = animator.pose_at(750.0).expect("the new motion starts");
        assert!(
            (after.latitude_degrees - before.latitude_degrees).abs() < 1e-9,
            "the drawn position is continuous across a retarget"
        );
        // The new motion runs over the 250 ms between the fixes, so its
        // half-way pose is half of the 0.5 → 2.0 segment.
        let half_way = animator.pose_at(750.0 + 125.0).expect("the motion runs");
        assert!((half_way.latitude_degrees - 1.25).abs() < 1e-9);
        let settled = animator.pose_at(750.0 + 250.0).expect("the motion ends");
        assert_eq!(settled.latitude_degrees, 2.0);
    }

    #[test]
    fn the_duration_is_the_fix_interval_clamped_to_the_band() {
        let mut animator = LocationPoseAnimator::new();
        animator.push_fix(fix_at(0.0, 0.0));
        animator.push_fix(fix_at(1.0, 5000.0));
        let near_end = animator
            .pose_at(5000.0 + LOCATION_ANIMATION_MAXIMUM_MILLISECONDS - 1.0)
            .expect("the motion runs");
        assert!(near_end.is_animating);
        let settled = animator
            .pose_at(5000.0 + LOCATION_ANIMATION_MAXIMUM_MILLISECONDS)
            .expect("the motion ends");
        assert!(!settled.is_animating);

        let mut burst = LocationPoseAnimator::new();
        burst.push_fix(fix_at(0.0, 0.0));
        burst.push_fix(fix_at(1.0, 10.0));
        let early = burst
            .pose_at(10.0 + LOCATION_ANIMATION_MINIMUM_MILLISECONDS - 1.0)
            .expect("the short motion still runs");
        assert!(early.is_animating);
    }

    #[test]
    fn the_course_takes_the_shorter_arc() {
        let mut animator = LocationPoseAnimator::new();
        animator.push_fix(LocationFix {
            course_degrees: Some(359.0),
            ..fix_at(0.0, 0.0)
        });
        animator.push_fix(LocationFix {
            course_degrees: Some(1.0),
            ..fix_at(1.0, 500.0)
        });
        let pose = animator.pose_at(750.0).expect("the motion runs");
        let course = pose.course_degrees.expect("the course is reported");
        assert!(
            course >= 359.0 || course <= 1.0,
            "one degree forward, not a full turn, got {course}"
        );
        let settled = animator.pose_at(1000.0).expect("the motion ends");
        assert_eq!(settled.course_degrees, Some(1.0));
    }

    #[test]
    fn accuracy_eases_between_the_fixes() {
        let mut animator = LocationPoseAnimator::new();
        animator.push_fix(LocationFix {
            horizontal_accuracy_meters: Some(10.0),
            ..fix_at(0.0, 0.0)
        });
        animator.push_fix(LocationFix {
            horizontal_accuracy_meters: Some(20.0),
            ..fix_at(1.0, 500.0)
        });
        let pose = animator.pose_at(750.0).expect("the motion runs");
        assert_eq!(pose.horizontal_accuracy_meters, Some(15.0));
    }

    #[test]
    fn a_clock_running_backwards_clamps_to_the_start() {
        let mut animator = LocationPoseAnimator::new();
        animator.push_fix(fix_at(0.0, 0.0));
        animator.push_fix(fix_at(1.0, 500.0));
        let pose = animator.pose_at(100.0).expect("the motion is clamped");
        assert_eq!(pose.latitude_degrees, 0.0);
    }

    #[test]
    fn a_reset_makes_the_next_fix_draw_immediately() {
        let mut animator = LocationPoseAnimator::new();
        animator.push_fix(fix_at(0.0, 0.0));
        animator.reset();
        assert!(animator.pose_at(0.0).is_none());
        animator.push_fix(fix_at(5.0, 10.0));
        let pose = animator.pose_at(10.0).expect("the fix draws");
        assert_eq!(pose.latitude_degrees, 5.0);
        assert!(!pose.is_animating);
    }
}
