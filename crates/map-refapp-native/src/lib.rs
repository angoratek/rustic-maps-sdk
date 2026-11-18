//! Refapp-only UniFFI bindings over the shared reference-application math.
//!
//! The Kotlin and Swift reference applications call the same follow-camera,
//! course-median, easing, scale-bar and trace-cursor values instead of keeping
//! per-platform mirrors. Every member delegates to [`map_refapp_core`]; this
//! crate only lowers the shared types across the UniFFI boundary.
//!
//! The crate is deliberately outside every published package: it is not a
//! dependency of `rustic-maps-native-core`, and `scripts/build-android.sh` and
//! `scripts/build-ios.sh` assert that the AAR and the XCFramework file sets do
//! not carry it.
//!
//! # Re-entrancy and threading
//!
//! The UniFFI scaffolding calls every interface method through `&self`, while
//! the shared state transitions need `&mut self`. Each wrapper therefore holds
//! its delegate behind a mutex and releases the lock before returning, so a
//! call can never hold two wrappers' locks at once. The methods are
//! synchronous and never await while a lock is held.

// The scaffolding generated from the UDL builds its metadata in large const
// arrays (uniffi 0.31's codegen pattern); the attribute cannot scope into the
// machine-generated macro expansion, so the lint is allowed crate-wide.
#![allow(clippy::large_const_arrays)]

use std::sync::{Mutex, MutexGuard};

use map_refapp_core as core;

/// Locks a wrapper's delegate, recovering the value from a poisoned lock.
///
/// The shared math has no partial-update path that could leave a value
/// inconsistent, so a panic in another thread cannot corrupt the delegate;
/// recovering keeps the demo usable instead of failing every later call.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
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

impl From<core::PreTripCamera> for PreTripCamera {
    fn from(camera: core::PreTripCamera) -> Self {
        Self {
            center_latitude_degrees: camera.center_latitude_degrees,
            center_longitude_degrees: camera.center_longitude_degrees,
            zoom_level: camera.zoom_level,
            bearing_degrees: camera.bearing_degrees,
            pitch_degrees: camera.pitch_degrees,
        }
    }
}

/// One chosen scale-bar scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelectedScaleBarScale {
    /// The bar's labelled distance in metres.
    pub distance_meters: f64,
    /// The bar's physical width for that distance, in viewport pixels.
    pub width_pixels: f64,
}

impl From<core::SelectedScaleBarScale> for SelectedScaleBarScale {
    fn from(scale: core::SelectedScaleBarScale) -> Self {
        Self {
            distance_meters: scale.distance_meters,
            width_pixels: scale.width_pixels,
        }
    }
}

/// One step of an easing move.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EasedValue {
    /// The current eased value.
    pub value: f64,
    /// Whether the move finished on this step.
    pub finished: bool,
}

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
    /// The camera turns with the trip's smoothed course.
    CourseUp,
    /// The visitor chose north-up: the camera holds bearing 0.
    NorthUp,
}

impl From<core::FollowBearingMode> for FollowBearingMode {
    fn from(bearing_mode: core::FollowBearingMode) -> Self {
        match bearing_mode {
            core::FollowBearingMode::CourseUp => Self::CourseUp,
            core::FollowBearingMode::NorthUp => Self::NorthUp,
        }
    }
}

impl From<FollowBearingMode> for core::FollowBearingMode {
    fn from(bearing_mode: FollowBearingMode) -> Self {
        match bearing_mode {
            FollowBearingMode::CourseUp => Self::CourseUp,
            FollowBearingMode::NorthUp => Self::NorthUp,
        }
    }
}

impl From<core::FollowPhase> for FollowPhase {
    fn from(phase: core::FollowPhase) -> Self {
        match phase {
            core::FollowPhase::Disengaged => Self::Disengaged,
            core::FollowPhase::Engaged => Self::Engaged,
            core::FollowPhase::BrokenByGesture => Self::BrokenByGesture,
        }
    }
}

/// The navigation follow camera's state machine.
pub struct FollowCamera {
    delegate: Mutex<core::FollowCamera>,
}

impl FollowCamera {
    /// Creates a disengaged camera.
    pub fn new() -> Self {
        Self {
            delegate: Mutex::new(core::FollowCamera::new()),
        }
    }

    /// The current follow phase.
    pub fn phase(&self) -> FollowPhase {
        lock(&self.delegate).phase().into()
    }

    /// The captured pre-trip camera, or null outside a trip.
    pub fn pre_trip_camera(&self) -> Option<PreTripCamera> {
        lock(&self.delegate).pre_trip_camera().map(Into::into)
    }

    /// The target centre latitude in degrees.
    pub fn target_center_latitude_degrees(&self) -> f64 {
        lock(&self.delegate).target_center_latitude_degrees()
    }

    /// The target centre longitude in degrees.
    pub fn target_center_longitude_degrees(&self) -> f64 {
        lock(&self.delegate).target_center_longitude_degrees()
    }

    /// The target zoom level.
    pub fn target_zoom_level(&self) -> f64 {
        lock(&self.delegate).target_zoom_level()
    }

    /// The target bearing in degrees, or null until a fix reports a course.
    pub fn target_bearing_degrees(&self) -> Option<f64> {
        lock(&self.delegate).target_bearing_degrees()
    }

    /// Applies the trip's follow camera.
    pub fn engage_trip(
        &self,
        current_center_latitude_degrees: f64,
        current_center_longitude_degrees: f64,
        current_zoom_level: f64,
        current_bearing_degrees: f64,
        current_pitch_degrees: f64,
    ) {
        lock(&self.delegate).engage_trip(
            current_center_latitude_degrees,
            current_center_longitude_degrees,
            current_zoom_level,
            current_bearing_degrees,
            current_pitch_degrees,
        );
    }

    /// How the camera treats its bearing.
    pub fn bearing_mode(&self) -> FollowBearingMode {
        lock(&self.delegate).bearing_mode().into()
    }

    /// Chooses how the camera treats its bearing.
    pub fn set_bearing_mode(&self, bearing_mode: FollowBearingMode) {
        lock(&self.delegate).set_bearing_mode(bearing_mode.into());
    }

    /// A user gesture breaks follow immediately.
    pub fn break_on_gesture(&self) {
        lock(&self.delegate).break_on_gesture();
    }

    /// Applies one location fix of the active trip.
    pub fn on_fix(
        &self,
        latitude_degrees: f64,
        longitude_degrees: f64,
        course_heading_degrees: Option<f64>,
        remaining_distance_fraction: f64,
    ) {
        lock(&self.delegate).on_fix(
            latitude_degrees,
            longitude_degrees,
            course_heading_degrees,
            remaining_distance_fraction,
        );
    }

    /// The my-location control's explicit re-engage.
    pub fn reengage(&self) {
        lock(&self.delegate).reengage();
    }

    /// Ends the trip and returns the captured pre-trip camera to restore.
    pub fn restore_pre_trip_camera(&self) -> Option<PreTripCamera> {
        lock(&self.delegate)
            .restore_pre_trip_camera()
            .map(Into::into)
    }
}

impl Default for FollowCamera {
    fn default() -> Self {
        Self::new()
    }
}

/// A fixed-window circular median over recent course-over-ground samples.
pub struct CourseMedianSmoother {
    delegate: Mutex<core::CourseMedianSmoother>,
}

impl CourseMedianSmoother {
    /// Creates a smoother over `window_fix_count` recent samples.
    pub fn new(window_fix_count: u32) -> Self {
        Self {
            delegate: Mutex::new(core::CourseMedianSmoother::new(window_fix_count as usize)),
        }
    }

    /// The window length in fixes.
    pub fn window_fix_count(&self) -> u32 {
        lock(&self.delegate).window_fix_count() as u32
    }

    /// Adds one fix's course-over-ground.
    pub fn add(&self, course_degrees: f64) {
        lock(&self.delegate).add(course_degrees);
    }

    /// The circular median, or null while the window holds no sample.
    pub fn median(&self) -> Option<f64> {
        lock(&self.delegate).median()
    }

    /// Empties the window.
    pub fn reset(&self) {
        lock(&self.delegate).reset();
    }

    /// The windowed samples, oldest first.
    pub fn samples(&self) -> Vec<f64> {
        lock(&self.delegate).samples().to_vec()
    }
}

/// A linear-in-time, smoothstep-eased value move.
pub struct SmoothstepTimedEase {
    delegate: Mutex<core::SmoothstepTimedEase>,
}

impl SmoothstepTimedEase {
    /// Creates an idle ease with no target.
    pub fn new() -> Self {
        Self {
            delegate: Mutex::new(core::SmoothstepTimedEase::new()),
        }
    }

    /// Begins, or restarts, an ease from one value to another.
    pub fn begin(
        &self,
        start_milliseconds: f64,
        from_value: f64,
        to_value: f64,
        duration_milliseconds: f64,
    ) {
        lock(&self.delegate).begin(
            start_milliseconds,
            from_value,
            to_value,
            duration_milliseconds,
        );
    }

    /// Stops an in-flight ease without applying anything.
    pub fn reset(&self) {
        lock(&self.delegate).reset();
    }

    /// Whether an ease is in flight.
    pub fn is_running(&self) -> bool {
        lock(&self.delegate).is_running()
    }

    /// The ease's target value.
    pub fn target_value(&self) -> f64 {
        lock(&self.delegate).target_value()
    }

    /// Advances the ease at `now_milliseconds`.
    pub fn advance(&self, now_milliseconds: f64) -> EasedValue {
        let (value, finished) = lock(&self.delegate).advance(now_milliseconds);
        EasedValue { value, finished }
    }
}

impl Default for SmoothstepTimedEase {
    fn default() -> Self {
        Self::new()
    }
}

/// An exponential chase toward a moving target.
pub struct ExponentialValueChase {
    delegate: Mutex<core::ExponentialValueChase>,
}

impl ExponentialValueChase {
    /// Creates a chase with the given time constant in milliseconds.
    pub fn new(time_constant_milliseconds: f64) -> Self {
        Self {
            delegate: Mutex::new(core::ExponentialValueChase::new(time_constant_milliseconds)),
        }
    }

    /// The chase's time constant in milliseconds.
    pub fn time_constant_milliseconds(&self) -> f64 {
        lock(&self.delegate).time_constant_milliseconds()
    }

    /// Advances the chase toward `target` at `now_milliseconds`.
    pub fn advance(&self, current: f64, target: f64, now_milliseconds: f64) -> f64 {
        lock(&self.delegate).advance(current, target, now_milliseconds)
    }

    /// Clears the frame clock so the next call is treated as a first frame.
    pub fn reset(&self) {
        lock(&self.delegate).reset();
    }
}

/// A forward-only cursor over a recorded trace's samples.
pub struct TraceCursor {
    delegate: Mutex<core::TraceCursor>,
}

impl TraceCursor {
    /// Creates a cursor at the first sample of a trace.
    pub fn new(sample_count: u32) -> Self {
        Self {
            delegate: Mutex::new(core::TraceCursor::new(sample_count as usize)),
        }
    }

    /// The trace's sample count.
    pub fn sample_count(&self) -> u32 {
        lock(&self.delegate).sample_count() as u32
    }

    /// The next sample's index, or null when every sample was delivered.
    pub fn next_sample_index(&self) -> Option<u32> {
        lock(&self.delegate)
            .next_sample_index()
            .map(|index| index as u32)
    }

    /// The next sample's index without consuming it.
    pub fn peek(&self) -> Option<u32> {
        lock(&self.delegate).peek().map(|index| index as u32)
    }

    /// Consumes and returns the next sample's index, or null at the end.
    pub fn advance(&self) -> Option<u32> {
        lock(&self.delegate).advance().map(|index| index as u32)
    }

    /// Whether every sample was delivered.
    pub fn is_finished(&self) -> bool {
        lock(&self.delegate).is_finished()
    }

    /// The number of samples not yet delivered.
    pub fn remaining_sample_count(&self) -> u32 {
        lock(&self.delegate).remaining_sample_count() as u32
    }

    /// Rewinds the cursor to the first sample.
    pub fn reset(&self) {
        lock(&self.delegate).reset();
    }

    /// Moves the cursor to `sample_index`, clamped to the trace's bounds.
    pub fn seek_to_sample_index(&self, sample_index: u32) {
        lock(&self.delegate).seek_to_sample_index(sample_index as usize);
    }
}

/// The follow camera's zoom toward trip end.
pub fn follow_zoom_toward_trip_end(remaining_distance_fraction: f64) -> f64 {
    core::follow_zoom_toward_trip_end(remaining_distance_fraction)
}

/// Smoothstep easing (`3t² − 2t³`).
pub fn smoothstep_eased_progress(linear_progress: f64) -> f64 {
    core::smoothstep_eased_progress(linear_progress)
}

/// The bearing's smallest angular distance from north.
pub fn bearing_off_north_degrees(bearing_degrees: f64) -> f64 {
    core::bearing_off_north_degrees(bearing_degrees)
}

/// Selects the scale bar's ladder distance for one camera.
pub fn scale_bar_selection(
    ground_meters_per_screen_pixel: f64,
    minimum_width_pixels: f64,
    maximum_width_pixels: f64,
) -> Option<SelectedScaleBarScale> {
    core::select_scale_bar_scale(
        ground_meters_per_screen_pixel,
        minimum_width_pixels,
        maximum_width_pixels,
    )
    .map(Into::into)
}

/// The scale bar's metric label for one ladder distance.
pub fn scale_bar_distance_label(distance_meters: f64) -> String {
    core::scale_bar_distance_label_text(distance_meters)
}

/// The replay dispatch interval of a recorded trace.
pub fn trace_dispatch_interval_milliseconds(
    recorded_sample_interval_seconds: f64,
    playback_acceleration_factor: f64,
) -> u64 {
    core::trace_dispatch_interval_milliseconds(
        recorded_sample_interval_seconds,
        playback_acceleration_factor,
    )
}

/// One pose the platforms draw for the location puck.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocationPose {
    /// Latitude to draw at, in degrees.
    pub latitude_degrees: f64,
    /// Longitude to draw at, in degrees.
    pub longitude_degrees: f64,
    /// Course to draw with, in degrees clockwise from north, when known.
    pub course_degrees: Option<f64>,
    /// Accuracy radius to draw, in meters, when known.
    pub horizontal_accuracy_meters: Option<f64>,
    /// Whether a motion is still running, so the shell keeps drawing.
    pub is_animating: bool,
}

/// The location puck's interpolation between fixes.
#[derive(Debug)]
pub struct LocationPoseAnimator {
    delegate: Mutex<core::LocationPoseAnimator>,
}

impl LocationPoseAnimator {
    /// Creates an animator with no fix yet.
    pub fn new() -> Self {
        Self {
            delegate: Mutex::new(core::LocationPoseAnimator::new()),
        }
    }

    /// Starts or retargets the motion toward one measured position.
    pub fn push_fix(
        &self,
        latitude_degrees: f64,
        longitude_degrees: f64,
        course_degrees: Option<f64>,
        horizontal_accuracy_meters: Option<f64>,
        received_at_milliseconds: f64,
    ) {
        let mut delegate = self
            .delegate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        delegate.push_fix(core::LocationFix {
            latitude_degrees,
            longitude_degrees,
            course_degrees,
            horizontal_accuracy_meters,
            received_at_milliseconds,
        });
    }

    /// The pose to draw at `now_milliseconds`, or `None` before the first fix.
    pub fn pose_at(&self, now_milliseconds: f64) -> Option<LocationPose> {
        let mut delegate = self
            .delegate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        delegate.pose_at(now_milliseconds).map(|pose| LocationPose {
            latitude_degrees: pose.latitude_degrees,
            longitude_degrees: pose.longitude_degrees,
            course_degrees: pose.course_degrees,
            horizontal_accuracy_meters: pose.horizontal_accuracy_meters,
            is_animating: pose.is_animating,
        })
    }

    /// Forgets every fix, so the next one draws immediately.
    pub fn reset(&self) {
        self.delegate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .reset();
    }
}

impl Default for LocationPoseAnimator {
    fn default() -> Self {
        Self::new()
    }
}

// Scaffolding generated at build time from the refapp-only UDL.
uniffi::include_scaffolding!("map_refapp_core");

/// Value-parity tests of the wrappers against the shared crate's documented
/// behavior values.
#[cfg(test)]
mod parity_tests;
