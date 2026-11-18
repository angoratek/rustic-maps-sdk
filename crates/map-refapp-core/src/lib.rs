//! Pure behavior math and the flow-state reducer shared by the reference
//! applications.
//!
//! The crate is deliberately small and self-contained: it holds no SDK handle,
//! no clock, no input/output, no logger and no platform dependency. Every
//! public item exchanges primitives and crate-local enums only, so the same
//! source compiles for native targets and for WebAssembly.
//!
//! The crate answers the one question the four reference applications must
//! answer identically (see `docs/design/ReferenceApplicationBehavior.md`):
//! given the current flow state and an event, what should the user see next?
//! The follow-camera state machine, the circular course median, the easing
//! primitives, the scale-bar ladder and the recorded-trace cursor sit beside
//! that reducer so each platform calls shared math instead of keeping a
//! mirror. Views, gestures, timers, credentials, speech engines and lifecycle
//! stay in the platform shells.

pub mod course;
pub mod easing;
pub mod flow;
pub mod follow_camera;
pub mod location_animation;
pub mod scale_bar;
pub mod trace;

pub use course::{
    COURSE_SMOOTHING_WINDOW_FIXES, CourseMedianSmoother, FULL_CIRCLE_DEGREES, HALF_CIRCLE_DEGREES,
    angular_distance_degrees,
};
pub use easing::{
    CHASE_FIRST_FRAME_DELTA_MILLISECONDS, ExponentialValueChase,
    FOLLOW_BEARING_CHASE_TIME_CONSTANT_MILLISECONDS, FOLLOW_BEARING_SNAP_DISTANCE_DEGREES,
    SmoothstepTimedEase, bearing_off_north_degrees, signed_angular_difference_degrees,
    smoothstep_eased_progress,
};
pub use flow::{FlowEvent, FlowField, FlowOutcome, FlowState, FlowTransition, advance_flow};
pub use follow_camera::{
    FollowBearingMode, FollowCamera, FollowPhase, NAVIGATION_FOLLOW_PITCH_3D_DEGREES,
    NAVIGATION_FOLLOW_ZOOM, NAVIGATION_FOLLOW_ZOOM_MAXIMUM, PreTripCamera,
    follow_zoom_toward_trip_end,
};
pub use location_animation::{
    LOCATION_ANIMATION_MAXIMUM_MILLISECONDS, LOCATION_ANIMATION_MINIMUM_MILLISECONDS, LocationFix,
    LocationPose, LocationPoseAnimator,
};
pub use scale_bar::{
    METERS_PER_KILOMETER, SCALE_BAR_LADDER_DECADE_SCAN_RANGE,
    SCALE_BAR_LADDER_LOWER_DECADE_SCAN_RANGE, SCALE_BAR_LADDER_MULTIPLIERS,
    SCALE_BAR_WIDTH_MAXIMUM_PIXELS, SCALE_BAR_WIDTH_MINIMUM_PIXELS, SelectedScaleBarScale,
    scale_bar_distance_label_text, select_scale_bar_scale,
    select_scale_bar_scale_with_token_width_range,
};
pub use trace::{
    MILLISECONDS_PER_SECOND, MINIMUM_TRACE_DISPATCH_INTERVAL_MILLISECONDS, TraceCursor,
    trace_dispatch_interval_milliseconds,
};
