//! The `wasm_bindgen` surface over [`map_refapp_core`], the reference
//! applications' shared behavior math.
//!
//! The classes and free functions here mirror the crate one to one; the only
//! work this module does is spelling crate answers in the JavaScript shapes
//! the page already consumes. The page-facing names stay the ones
//! `platform/web/map_controls.mjs` has always exported, so the page and its
//! Playwright assertions keep working unchanged while the implementations move
//! to Rust.
//!
//! The timer, the view lifecycle and the camera object stay platform-side; the
//! crate owns only the pure decisions.

use js_sys::{Array, Object, Reflect};
use map_refapp_core::{
    CourseMedianSmoother as NativeCourseMedianSmoother,
    ExponentialValueChase as NativeExponentialValueChase, FollowCamera as NativeFollowCamera,
    LocationFix as NativeLocationFix, LocationPoseAnimator as NativeLocationPoseAnimator,
    SmoothstepTimedEase as NativeSmoothstepTimedEase, TraceCursor as NativeTraceCursor,
};
use wasm_bindgen::prelude::*;

use crate::refapp_math::{
    PreTripCameraValue, ScaleBarSelectionValue, follow_phase_label, scale_bar_distance_label_text,
    select_scale_bar_scale_with_width_range, trace_dispatch_interval_milliseconds,
};

/// Field name of a scale-bar selection's labelled distance.
const SCALE_BAR_SELECTION_FIELD_DISTANCE_METERS: &str = "distanceMeters";
/// Field name of a scale-bar selection's physical width.
const SCALE_BAR_SELECTION_FIELD_WIDTH_PIXELS: &str = "widthPixels";
/// Field name of a pre-trip camera's centre latitude.
const PRE_TRIP_CAMERA_FIELD_CENTER_LATITUDE_DEGREES: &str = "centerLatitudeDegrees";
/// Field name of a pre-trip camera's centre longitude.
const PRE_TRIP_CAMERA_FIELD_CENTER_LONGITUDE_DEGREES: &str = "centerLongitudeDegrees";
/// Field name of a pre-trip camera's zoom level.
const PRE_TRIP_CAMERA_FIELD_ZOOM_LEVEL: &str = "zoomLevel";
/// Field name of a pre-trip camera's bearing.
const PRE_TRIP_CAMERA_FIELD_BEARING_DEGREES: &str = "bearingDegrees";
/// Field name of a pre-trip camera's pitch.
const PRE_TRIP_CAMERA_FIELD_PITCH_DEGREES: &str = "pitchDegrees";

/// Builds the plain object the page reads a scale-bar selection from.
fn scale_bar_selection_object(selection: ScaleBarSelectionValue) -> Result<Object, JsValue> {
    let selection_object = Object::new();
    Reflect::set(
        &selection_object,
        &JsValue::from_str(SCALE_BAR_SELECTION_FIELD_DISTANCE_METERS),
        &JsValue::from(selection.distance_meters),
    )?;
    Reflect::set(
        &selection_object,
        &JsValue::from_str(SCALE_BAR_SELECTION_FIELD_WIDTH_PIXELS),
        &JsValue::from(selection.width_pixels),
    )?;
    Ok(selection_object)
}

/// Builds the plain object the page reads a captured pre-trip camera from.
fn pre_trip_camera_object(camera: PreTripCameraValue) -> Result<Object, JsValue> {
    let camera_object = Object::new();
    for (field_name, field_value) in [
        (
            PRE_TRIP_CAMERA_FIELD_CENTER_LATITUDE_DEGREES,
            camera.center_latitude_degrees,
        ),
        (
            PRE_TRIP_CAMERA_FIELD_CENTER_LONGITUDE_DEGREES,
            camera.center_longitude_degrees,
        ),
        (PRE_TRIP_CAMERA_FIELD_ZOOM_LEVEL, camera.zoom_level),
        (
            PRE_TRIP_CAMERA_FIELD_BEARING_DEGREES,
            camera.bearing_degrees,
        ),
        (PRE_TRIP_CAMERA_FIELD_PITCH_DEGREES, camera.pitch_degrees),
    ] {
        Reflect::set(
            &camera_object,
            &JsValue::from_str(field_name),
            &JsValue::from(field_value),
        )?;
    }
    Ok(camera_object)
}

/// A nullable number as the page spells it: `null` when the crate has none.
fn nullable_number(value: Option<f64>) -> JsValue {
    match value {
        Some(number) => JsValue::from(number),
        None => JsValue::NULL,
    }
}

/// Smoothstep easing (`3t² − 2t³`), clamped into `[0, 1]`.
#[wasm_bindgen(js_name = smoothstepEasedProgress)]
pub fn smoothstep_eased_progress(linear_progress: f64) -> f64 {
    map_refapp_core::smoothstep_eased_progress(linear_progress)
}

/// The camera bearing's smallest off-north angular distance, in degrees.
#[wasm_bindgen(js_name = bearingOffNorthDegrees)]
pub fn bearing_off_north_degrees(bearing_degrees: f64) -> f64 {
    map_refapp_core::bearing_off_north_degrees(bearing_degrees)
}

/// The shortest signed angular difference `to − from`, in degrees.
#[wasm_bindgen(js_name = signedAngularDifferenceDegrees)]
pub fn signed_angular_difference_degrees(to_degrees: f64, from_degrees: f64) -> f64 {
    map_refapp_core::signed_angular_difference_degrees(to_degrees, from_degrees)
}

/// The follow camera's zoom toward trip end.
#[wasm_bindgen(js_name = followZoomTowardTripEnd)]
pub fn follow_zoom_toward_trip_end(remaining_distance_fraction: f64) -> f64 {
    map_refapp_core::follow_zoom_toward_trip_end(remaining_distance_fraction)
}

/// Selects the scale-bar ladder distance using the design language's token
/// width range.
///
/// Returns a `{ distanceMeters, widthPixels }` object, or `null` when the
/// ground answer is not usable.
#[wasm_bindgen(js_name = selectScaleBarScale)]
pub fn select_scale_bar_scale(ground_meters_per_screen_pixel: f64) -> Result<JsValue, JsValue> {
    match crate::refapp_math::select_scale_bar_scale(ground_meters_per_screen_pixel) {
        Some(selection) => Ok(scale_bar_selection_object(selection)?.into()),
        None => Ok(JsValue::NULL),
    }
}

/// Selects the scale-bar ladder distance that fits an explicit width range.
#[wasm_bindgen(js_name = selectScaleBarScaleWithWidthRange)]
pub fn select_scale_bar_scale_from_width_range(
    ground_meters_per_screen_pixel: f64,
    minimum_width_pixels: f64,
    maximum_width_pixels: f64,
) -> Result<JsValue, JsValue> {
    match select_scale_bar_scale_with_width_range(
        ground_meters_per_screen_pixel,
        minimum_width_pixels,
        maximum_width_pixels,
    ) {
        Some(selection) => Ok(scale_bar_selection_object(selection)?.into()),
        None => Ok(JsValue::NULL),
    }
}

/// The selected ladder distance's metric label.
#[wasm_bindgen(js_name = scaleBarDistanceLabelText)]
pub fn scale_bar_distance_label_text_from_meters(distance_meters: f64) -> String {
    scale_bar_distance_label_text(distance_meters)
}

/// The replay dispatch interval for a recorded trace, in milliseconds.
#[wasm_bindgen(js_name = traceDispatchIntervalMilliseconds)]
pub fn trace_dispatch_interval_milliseconds_from_playback(
    recorded_sample_interval_seconds: f64,
    playback_acceleration_factor: f64,
) -> f64 {
    trace_dispatch_interval_milliseconds(
        recorded_sample_interval_seconds,
        playback_acceleration_factor,
    )
}

/// The navigation follow-camera state machine and its targets.
#[wasm_bindgen(js_name = FollowCamera)]
pub struct FollowCamera {
    native: NativeFollowCamera,
}

#[wasm_bindgen]
impl FollowCamera {
    /// Creates a disengaged camera with the default course-smoothing window.
    #[wasm_bindgen(constructor)]
    pub fn new() -> FollowCamera {
        Self {
            native: NativeFollowCamera::new(),
        }
    }

    /// Applies the trip's follow camera and captures the pre-trip pose.
    #[wasm_bindgen(js_name = engageTrip)]
    pub fn engage_trip(
        &mut self,
        current_center_latitude_degrees: f64,
        current_center_longitude_degrees: f64,
        current_zoom_level: f64,
        current_bearing_degrees: f64,
        current_pitch_degrees: f64,
    ) {
        self.native.engage_trip(
            current_center_latitude_degrees,
            current_center_longitude_degrees,
            current_zoom_level,
            current_bearing_degrees,
            current_pitch_degrees,
        );
    }

    /// A user gesture (drag, pinch or rotate) breaks follow immediately.
    #[wasm_bindgen(js_name = breakOnGesture)]
    pub fn break_on_gesture(&mut self) {
        self.native.break_on_gesture();
    }

    /// Holds the camera north-up whatever the fixes report.
    #[wasm_bindgen(js_name = northUp)]
    pub fn north_up(&mut self) {
        self.native
            .set_bearing_mode(map_refapp_core::FollowBearingMode::NorthUp);
    }

    /// Resumes the eased course chase on the next fix.
    #[wasm_bindgen(js_name = courseUp)]
    pub fn course_up(&mut self) {
        self.native
            .set_bearing_mode(map_refapp_core::FollowBearingMode::CourseUp);
    }

    /// Whether the camera is holding north-up.
    #[wasm_bindgen(js_name = isNorthUp)]
    pub fn is_north_up(&self) -> bool {
        self.native.bearing_mode() == map_refapp_core::FollowBearingMode::NorthUp
    }

    /// Applies one location fix of the active trip.
    #[wasm_bindgen(js_name = onFix)]
    pub fn on_fix(
        &mut self,
        latitude_degrees: f64,
        longitude_degrees: f64,
        course_heading_degrees: Option<f64>,
        remaining_distance_fraction: f64,
    ) {
        self.native.on_fix(
            latitude_degrees,
            longitude_degrees,
            course_heading_degrees,
            remaining_distance_fraction,
        );
    }

    /// The my-location control's explicit re-engage.
    #[wasm_bindgen(js_name = reengage)]
    pub fn reengage(&mut self) {
        self.native.reengage();
    }

    /// Ends the trip, returning the captured pre-trip camera or `null`.
    #[wasm_bindgen(js_name = restorePreTripCamera)]
    pub fn restore_pre_trip_camera(&mut self) -> Result<JsValue, JsValue> {
        match self.native.restore_pre_trip_camera() {
            Some(camera) => Ok(pre_trip_camera_object(camera.into())?.into()),
            None => Ok(JsValue::NULL),
        }
    }

    /// The current follow phase.
    #[wasm_bindgen(getter)]
    pub fn phase(&self) -> String {
        follow_phase_label(self.native.phase()).to_string()
    }

    /// The captured pre-trip camera, or `null` outside a trip.
    #[wasm_bindgen(getter, js_name = preTripCamera)]
    pub fn pre_trip_camera(&self) -> Result<JsValue, JsValue> {
        match self.native.pre_trip_camera() {
            Some(camera) => Ok(pre_trip_camera_object(camera.into())?.into()),
            None => Ok(JsValue::NULL),
        }
    }

    /// The target centre latitude in degrees.
    #[wasm_bindgen(getter, js_name = targetCenterLatitudeDegrees)]
    pub fn target_center_latitude_degrees(&self) -> f64 {
        self.native.target_center_latitude_degrees()
    }

    /// The target centre longitude in degrees.
    #[wasm_bindgen(getter, js_name = targetCenterLongitudeDegrees)]
    pub fn target_center_longitude_degrees(&self) -> f64 {
        self.native.target_center_longitude_degrees()
    }

    /// The target zoom level.
    #[wasm_bindgen(getter, js_name = targetZoomLevel)]
    pub fn target_zoom_level(&self) -> f64 {
        self.native.target_zoom_level()
    }

    /// The target bearing in degrees, or `null` until a fix reports a course.
    #[wasm_bindgen(getter, js_name = targetBearingDegrees)]
    pub fn target_bearing_degrees(&self) -> JsValue {
        nullable_number(self.native.target_bearing_degrees())
    }
}

impl Default for FollowCamera {
    fn default() -> Self {
        Self::new()
    }
}

/// A fixed-window circular median over recent course-over-ground samples.
#[wasm_bindgen(js_name = CourseMedianSmoother)]
pub struct CourseMedianSmoother {
    native: NativeCourseMedianSmoother,
}

#[wasm_bindgen]
impl CourseMedianSmoother {
    /// Creates a smoother over `window_fix_count` recent samples.
    #[wasm_bindgen(constructor)]
    pub fn new(window_fix_count: usize) -> CourseMedianSmoother {
        Self {
            native: NativeCourseMedianSmoother::new(window_fix_count),
        }
    }

    /// Adds one fix's course-over-ground, dropping the oldest sample.
    #[wasm_bindgen]
    pub fn add(&mut self, course_degrees: f64) {
        self.native.add(course_degrees);
    }

    /// The circular median, or `null` while the window holds no sample.
    #[wasm_bindgen]
    pub fn median(&self) -> JsValue {
        nullable_number(self.native.median())
    }

    /// Empties the window.
    #[wasm_bindgen]
    pub fn reset(&mut self) {
        self.native.reset();
    }
}

/// A linear-in-time, smoothstep-eased value move.
#[wasm_bindgen(js_name = SmoothstepTimedEase)]
pub struct SmoothstepTimedEase {
    native: NativeSmoothstepTimedEase,
}

#[wasm_bindgen]
impl SmoothstepTimedEase {
    /// Creates an idle ease with no target.
    #[wasm_bindgen(constructor)]
    pub fn new() -> SmoothstepTimedEase {
        Self {
            native: NativeSmoothstepTimedEase::new(),
        }
    }

    /// Begins, or restarts, an ease from one value to another.
    #[wasm_bindgen]
    pub fn begin(
        &mut self,
        start_milliseconds: f64,
        from_value: f64,
        to_value: f64,
        duration_milliseconds: f64,
    ) {
        self.native.begin(
            start_milliseconds,
            from_value,
            to_value,
            duration_milliseconds,
        );
    }

    /// Stops an in-flight ease without applying anything.
    #[wasm_bindgen]
    pub fn reset(&mut self) {
        self.native.reset();
    }

    /// Whether an ease is in flight.
    #[wasm_bindgen(getter, js_name = is_running)]
    pub fn is_running(&self) -> bool {
        self.native.is_running()
    }

    /// The ease's target value.
    #[wasm_bindgen(getter, js_name = target_value)]
    pub fn target_value(&self) -> f64 {
        self.native.target_value()
    }

    /// Advances the ease at `now_milliseconds`.
    ///
    /// Returns `[value, finished]`: the current eased value and whether the
    /// move just finished on this call.
    #[wasm_bindgen]
    pub fn advance(&mut self, now_milliseconds: f64) -> Array {
        let (value, finished) = self.native.advance(now_milliseconds);
        let result = Array::new();
        result.push(&JsValue::from(value));
        result.push(&JsValue::from(finished));
        result
    }
}

impl Default for SmoothstepTimedEase {
    fn default() -> Self {
        Self::new()
    }
}

/// An exponential chase toward a moving target.
#[wasm_bindgen(js_name = ExponentialValueChase)]
pub struct ExponentialValueChase {
    native: NativeExponentialValueChase,
}

#[wasm_bindgen]
impl ExponentialValueChase {
    /// Creates a chase with the given time constant in milliseconds.
    #[wasm_bindgen(constructor)]
    pub fn new(time_constant_milliseconds: f64) -> ExponentialValueChase {
        Self {
            native: NativeExponentialValueChase::new(time_constant_milliseconds),
        }
    }

    /// Advances the chase toward `target`, returning the next current value.
    #[wasm_bindgen]
    pub fn advance(&mut self, current: f64, target: f64, now_milliseconds: f64) -> f64 {
        self.native.advance(current, target, now_milliseconds)
    }

    /// Clears the frame clock so the next call is treated as a first frame.
    #[wasm_bindgen]
    pub fn reset(&mut self) {
        self.native.reset();
    }
}

/// A forward-only cursor over a recorded trace's samples.
#[wasm_bindgen(js_name = TraceCursor)]
pub struct TraceCursor {
    native: NativeTraceCursor,
}

#[wasm_bindgen]
impl TraceCursor {
    /// Creates a cursor at the first sample of a trace with `sample_count`
    /// samples.
    #[wasm_bindgen(constructor)]
    pub fn new(sample_count: usize) -> TraceCursor {
        Self {
            native: NativeTraceCursor::new(sample_count),
        }
    }

    /// The trace's sample count.
    #[wasm_bindgen(getter, js_name = sampleCount)]
    pub fn sample_count(&self) -> usize {
        self.native.sample_count()
    }

    /// The next sample's index, or `null` when every sample was delivered.
    #[wasm_bindgen(js_name = nextSampleIndex)]
    pub fn next_sample_index(&self) -> JsValue {
        nullable_number(self.native.next_sample_index().map(|index| index as f64))
    }

    /// The next sample's index without consuming it.
    #[wasm_bindgen]
    pub fn peek(&self) -> JsValue {
        nullable_number(self.native.peek().map(|index| index as f64))
    }

    /// Consumes and returns the next sample's index, or `null` at the end.
    #[wasm_bindgen]
    pub fn advance(&mut self) -> JsValue {
        nullable_number(self.native.advance().map(|index| index as f64))
    }

    /// Whether every sample was delivered.
    #[wasm_bindgen(js_name = isFinished)]
    pub fn is_finished(&self) -> bool {
        self.native.is_finished()
    }

    /// The number of samples not yet delivered.
    #[wasm_bindgen(js_name = remainingSampleCount)]
    pub fn remaining_sample_count(&self) -> usize {
        self.native.remaining_sample_count()
    }

    /// Rewinds the cursor to the first sample.
    #[wasm_bindgen]
    pub fn reset(&mut self) {
        self.native.reset();
    }

    /// Moves the cursor to `sample_index`, clamped to the trace's bounds.
    #[wasm_bindgen(js_name = seekToSampleIndex)]
    pub fn seek_to_sample_index(&mut self, sample_index: usize) {
        self.native.seek_to_sample_index(sample_index);
    }
}

/// The location puck's interpolation between fixes.
///
/// `pushFix` starts (or retargets) a motion toward the measured position;
/// `poseAt` answers the pose to draw at the given instant, so the page's
/// frame pump can draw a puck that glides instead of hopping.
#[wasm_bindgen(js_name = LocationPoseAnimator)]
pub struct LocationPoseAnimator {
    native: NativeLocationPoseAnimator,
}

#[wasm_bindgen]
impl LocationPoseAnimator {
    /// Creates an animator with no fix yet.
    #[wasm_bindgen(constructor)]
    pub fn new() -> LocationPoseAnimator {
        Self {
            native: NativeLocationPoseAnimator::new(),
        }
    }

    /// Starts or retargets the motion toward one measured position.
    #[wasm_bindgen(js_name = pushFix)]
    pub fn push_fix(
        &mut self,
        latitude_degrees: f64,
        longitude_degrees: f64,
        course_degrees: Option<f64>,
        horizontal_accuracy_meters: Option<f64>,
        received_at_milliseconds: f64,
    ) {
        self.native.push_fix(NativeLocationFix {
            latitude_degrees,
            longitude_degrees,
            course_degrees,
            horizontal_accuracy_meters,
            received_at_milliseconds,
        });
    }

    /// The pose to draw at `now_milliseconds`, or `undefined` before the first
    /// fix.
    #[wasm_bindgen(js_name = poseAt)]
    pub fn pose_at(&mut self, now_milliseconds: f64) -> Option<Object> {
        let pose = self.native.pose_at(now_milliseconds)?;
        let pose_object = Object::new();
        let fields = [
            ("latitudeDegrees", pose.latitude_degrees),
            ("longitudeDegrees", pose.longitude_degrees),
        ];
        for (field_name, field_value) in fields {
            Reflect::set(
                &pose_object,
                &JsValue::from_str(field_name),
                &JsValue::from_f64(field_value),
            )
            .expect("the pose object accepts a numeric field");
        }
        let optional_fields = [
            ("courseDegrees", pose.course_degrees),
            ("horizontalAccuracyMeters", pose.horizontal_accuracy_meters),
        ];
        for (field_name, field_value) in optional_fields {
            let field_js_value = match field_value {
                Some(value) => JsValue::from_f64(value),
                None => JsValue::NULL,
            };
            Reflect::set(
                &pose_object,
                &JsValue::from_str(field_name),
                &field_js_value,
            )
            .expect("the pose object accepts an optional field");
        }
        Reflect::set(
            &pose_object,
            &JsValue::from_str("isAnimating"),
            &JsValue::from_bool(pose.is_animating),
        )
        .expect("the pose object accepts the animating flag");
        Some(pose_object)
    }

    /// Forgets every fix, so the next one draws immediately.
    #[wasm_bindgen(js_name = reset)]
    pub fn reset(&mut self) {
        self.native.reset();
    }
}

impl Default for LocationPoseAnimator {
    fn default() -> Self {
        Self::new()
    }
}
