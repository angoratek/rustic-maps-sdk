//! Browser device positioning: the `rustic-maps-positioning` controller exposed to the
//! page.
//!
//! The page's host source — `navigator.geolocation` — reports fixes and
//! failures into this controller, which validates them, smooths their course
//! and tracks staleness. It is the same controller type the native targets
//! bind through the contract, so the browser's behaviour is not a separate
//! implementation. The class is exposed on the wasm surface only.

use crate::exports::{
    ERROR_CODE_INVALID_ARGUMENT, ERROR_CODE_INVALID_COORDINATE_INPUT, failure_with_code,
    object_from_fields,
};
use geospatial_types::GeographicCoordinate;
use js_sys::Object;
use rustic_maps_positioning::{
    PositionController as NativePositionController, PositionFix, PositionSourceStatus,
};
use wasm_bindgen::prelude::*;

/// Field name of a fix's latitude.
const POSITION_FIELD_LATITUDE: &str = "latitude";
/// Field name of a fix's longitude.
const POSITION_FIELD_LONGITUDE: &str = "longitude";
/// Field name of a fix's horizontal accuracy.
const POSITION_FIELD_HORIZONTAL_ACCURACY_METERS: &str = "horizontalAccuracyMeters";
/// Field name of a fix's course.
const POSITION_FIELD_COURSE_HEADING_DEGREES: &str = "courseHeadingDegrees";
/// Field name of a fix's timestamp.
const POSITION_FIELD_TIMESTAMP_SECONDS: &str = "timestampSeconds";

/// The source-status names the page may report, matching the controller's
/// variants.
const POSITION_SOURCE_STATUS_PERMISSION_DENIED: &str = "permissionDenied";
const POSITION_SOURCE_STATUS_SERVICE_DISABLED: &str = "serviceDisabled";
const POSITION_SOURCE_STATUS_UNAVAILABLE: &str = "unavailable";
const POSITION_SOURCE_STATUS_TIMEOUT: &str = "timeout";

/// Validates, de-duplicates and smooths the page's position fixes.
#[wasm_bindgen(js_name = PositionController)]
pub struct PositionController {
    controller: NativePositionController,
}

#[wasm_bindgen]
impl PositionController {
    /// Builds an empty controller with the default smoothing window and
    /// staleness threshold.
    #[wasm_bindgen(constructor)]
    pub fn new() -> PositionController {
        Self {
            controller: NativePositionController::new(),
        }
    }

    /// Feeds one fix from the page's geolocation source. Every scalar is
    /// optional except the coordinate; the controller validates all of them
    /// and refuses an invalid fix with an `invalidCoordinateInput` failure,
    /// keeping the
    /// previous state.
    #[wasm_bindgen(js_name = feedFix)]
    pub fn feed_fix(
        &mut self,
        latitude: f64,
        longitude: f64,
        horizontal_accuracy_meters: Option<f64>,
        course_heading_degrees: Option<f64>,
        timestamp_seconds: Option<f64>,
    ) -> Result<(), JsValue> {
        let coordinate = GeographicCoordinate::from_degrees(latitude, longitude).map_err(
            |coordinate_error| {
                failure_with_code(
                    ERROR_CODE_INVALID_COORDINATE_INPUT,
                    &coordinate_error.to_string(),
                )
            },
        )?;
        self.controller
            .feed_fix(PositionFix {
                coordinate,
                horizontal_accuracy_meters,
                course_heading_degrees,
                timestamp_seconds,
            })
            .map_err(|positioning_error| {
                failure_with_code(
                    ERROR_CODE_INVALID_COORDINATE_INPUT,
                    &positioning_error.to_string(),
                )
            })
    }

    /// Records a source failure by its documented name (`permissionDenied`,
    /// `serviceDisabled`, `unavailable`, `timeout`). An unknown name is a
    /// failure carrying an `invalidArgument` code.
    #[wasm_bindgen(js_name = sourceFailed)]
    pub fn source_failed(&mut self, status_name: &str) -> Result<(), JsValue> {
        let status = match status_name {
            POSITION_SOURCE_STATUS_PERMISSION_DENIED => PositionSourceStatus::PermissionDenied,
            POSITION_SOURCE_STATUS_SERVICE_DISABLED => PositionSourceStatus::ServiceDisabled,
            POSITION_SOURCE_STATUS_UNAVAILABLE => PositionSourceStatus::Unavailable,
            POSITION_SOURCE_STATUS_TIMEOUT => PositionSourceStatus::Timeout,
            other_status => {
                return Err(failure_with_code(
                    ERROR_CODE_INVALID_ARGUMENT,
                    &format!(
                        "the position source status must be permissionDenied, serviceDisabled, \
                         unavailable or timeout, got {other_status}"
                    ),
                ));
            }
        };
        self.controller.feed_source_failure(status);
        Ok(())
    }

    /// The latest accepted fix, or null before the first one.
    #[wasm_bindgen(js_name = latestFix)]
    pub fn latest_fix(&self) -> Result<Option<Object>, JsValue> {
        let Some(latest_fix) = self.controller.latest_fix() else {
            return Ok(None);
        };
        object_from_fields(&[
            (
                POSITION_FIELD_LATITUDE,
                JsValue::from(latest_fix.coordinate.latitude),
            ),
            (
                POSITION_FIELD_LONGITUDE,
                JsValue::from(latest_fix.coordinate.longitude),
            ),
            (
                POSITION_FIELD_HORIZONTAL_ACCURACY_METERS,
                latest_fix
                    .horizontal_accuracy_meters
                    .map_or(JsValue::NULL, JsValue::from),
            ),
            (
                POSITION_FIELD_COURSE_HEADING_DEGREES,
                latest_fix
                    .course_heading_degrees
                    .map_or(JsValue::NULL, JsValue::from),
            ),
            (
                POSITION_FIELD_TIMESTAMP_SECONDS,
                latest_fix
                    .timestamp_seconds
                    .map_or(JsValue::NULL, JsValue::from),
            ),
        ])
        .map(Some)
    }

    /// The smoothed course in degrees clockwise from true north, or null when
    /// no accepted fix carried a course.
    #[wasm_bindgen(js_name = smoothedCourseDegrees)]
    pub fn smoothed_course_degrees(&self) -> Option<f64> {
        self.controller.smoothed_course_heading_degrees()
    }

    /// Whether the latest fix is older than the staleness threshold at
    /// `now_seconds`.
    #[wasm_bindgen(js_name = isStale)]
    pub fn is_stale(&self, now_seconds: f64) -> bool {
        self.controller.is_stale(now_seconds)
    }

    /// The last source failure's name, or null when the last event was a fix.
    #[wasm_bindgen(js_name = sourceStatus)]
    pub fn source_status(&self) -> Option<String> {
        self.controller.source_status().map(|status| {
            match status {
                PositionSourceStatus::PermissionDenied => POSITION_SOURCE_STATUS_PERMISSION_DENIED,
                PositionSourceStatus::ServiceDisabled => POSITION_SOURCE_STATUS_SERVICE_DISABLED,
                PositionSourceStatus::Unavailable => POSITION_SOURCE_STATUS_UNAVAILABLE,
                PositionSourceStatus::Timeout => POSITION_SOURCE_STATUS_TIMEOUT,
            }
            .to_string()
        })
    }
}

impl Default for PositionController {
    fn default() -> Self {
        Self::new()
    }
}
