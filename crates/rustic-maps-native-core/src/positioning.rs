//! The contract's device-positioning controller.
//!
//! Hosts feed the controller the fixes and failures their operating system's
//! location source reports; the controller validates the fixes, ignores
//! stationary repeats when smoothing course, tracks staleness and records
//! source failures. The renderer's location indicator, a follow camera and
//! the navigation controller all read the same stream from here.

use crate::error::SdkError;
use crate::records::{PositionFix, PositionSourceError};
use rustic_maps_positioning::PositionController as NativePositionController;
use std::sync::{Mutex, PoisonError};

/// Validates and smooths one host's position stream.
#[derive(Debug)]
pub struct PositionController {
    controller: Mutex<NativePositionController>,
}

impl Default for PositionController {
    fn default() -> Self {
        Self::new()
    }
}

impl PositionController {
    /// Builds an empty controller with the default course-smoothing window
    /// and staleness threshold.
    pub fn new() -> Self {
        Self {
            controller: Mutex::new(NativePositionController::new()),
        }
    }

    /// Accepts one fix from the host's source. The coordinate must be inside
    /// its canonical ranges, and every reported scalar must be finite
    /// (accuracy and timestamp also non-negative); an invalid fix is refused
    /// with the validator's detail and leaves the previous state in force.
    pub fn feed_fix(&self, fix: PositionFix) -> Result<(), SdkError> {
        let native_fix = fix.to_native_position_fix()?;
        self.controller
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .feed_fix(native_fix)
            .map_err(|positioning_error| SdkError::InvalidCoordinateInput {
                detail: positioning_error.to_string(),
            })
    }

    /// Records a source failure. The latest fix stays readable while the
    /// source recovers; a later successful fix clears the status.
    pub fn source_failed(&self, error: PositionSourceError) {
        self.controller
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .feed_source_failure(error.to_native_source_status());
    }

    /// The latest accepted fix, or null before the first accepted one.
    pub fn latest_fix(&self) -> Option<PositionFix> {
        self.controller
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .latest_fix()
            .map(PositionFix::from_native_position_fix)
    }

    /// The smoothed course across the most recent fixes that reported one,
    /// in degrees clockwise from true north.
    pub fn smoothed_course_degrees(&self) -> Option<f64> {
        self.controller
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .smoothed_course_heading_degrees()
    }

    /// The age after which the controller considers a fix stale, in seconds.
    pub fn stale_after_seconds(&self) -> f64 {
        self.controller
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .stale_after_seconds()
    }

    /// Whether the latest fix is older than the staleness threshold at
    /// `now_seconds`. A fix without a timestamp is never stale.
    pub fn is_stale(&self, now_seconds: f64) -> bool {
        self.controller
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_stale(now_seconds)
    }

    /// The last source failure, or null when the last event was a fix.
    pub fn source_status(&self) -> Option<PositionSourceError> {
        self.controller
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .source_status()
            .map(PositionSourceError::from_native_source_status)
    }
}
