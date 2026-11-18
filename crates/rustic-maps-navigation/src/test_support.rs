//! Shared helpers for navigation unit tests: synthetic coordinates and route
//! builders anchored on a neutral mid-latitude origin, plus the stateful
//! conditions that prove the fresh-instance contracts.
//!
//! Test-only; compiled out of every non-test build.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::geometry::EARTH_RADIUS_METERS;
use geospatial_types::GeographicCoordinate;

use crate::deviation::{DeviationDetector, DeviationStatus};
use crate::state::TripNavigationState;
use crate::step_advance::{StepAdvanceCondition, StepAdvanceDecision, StepAdvanceInputs};

/// Latitude, in degrees, of the synthetic fixture origin (a neutral
/// mid-latitude where the equirectangular distortion documented in
/// [`crate::geometry`] is negligible at test scales).
pub(crate) const TEST_ORIGIN_LATITUDE_DEGREES: f64 = 47.0;

/// Longitude, in degrees, of the synthetic fixture origin.
pub(crate) const TEST_ORIGIN_LONGITUDE_DEGREES: f64 = 8.0;

/// Eastward span, in meters, of the straight-line synthetic test polyline.
pub(crate) const TEST_SEGMENT_SPAN_METERS: f64 = 1000.0;

/// Horizontal accuracy, in meters, of the synthetic walker's fixes: far
/// better than the default accuracy gate, so distance conditions always
/// consider the updates.
pub(crate) const TEST_ACCURACY_METERS: f64 = 2.0;

/// The fixture origin all synthetic navigation fixtures build from.
pub(crate) fn test_origin() -> GeographicCoordinate {
    GeographicCoordinate {
        latitude: TEST_ORIGIN_LATITUDE_DEGREES,
        longitude: TEST_ORIGIN_LONGITUDE_DEGREES,
    }
}

/// A coordinate displaced east of [`test_origin`] by `east_meters`.
pub(crate) fn coordinate_east_of(east_meters: f64) -> GeographicCoordinate {
    offset_coordinate(east_meters, 0.0)
}

/// A coordinate displaced by east/north meter offsets around [`test_origin`].
pub(crate) fn offset_coordinate(east_meters: f64, north_meters: f64) -> GeographicCoordinate {
    let mut coordinate = test_origin();
    coordinate.longitude += (east_meters
        / (EARTH_RADIUS_METERS * TEST_ORIGIN_LATITUDE_DEGREES.to_radians().cos()))
    .to_degrees();
    coordinate.latitude += (north_meters / EARTH_RADIUS_METERS).to_degrees();
    coordinate
}

/// A one-shot stateful condition: it advances exactly once per instance, then
/// must be replaced by a fresh instance. This is the contract the reducer
/// relies on to prevent a consumed instance from re-firing, and the one the
/// combinators rely on to prove their children are rebuilt fresh.
#[derive(Debug)]
pub(crate) struct OneShotAdvanceCondition {
    has_advanced: AtomicBool,
}

impl OneShotAdvanceCondition {
    /// Builds an instance that may still fire once.
    pub(crate) fn unused() -> Self {
        Self {
            has_advanced: AtomicBool::new(false),
        }
    }

    /// Builds an instance that has already fired; only a fresh instance can
    /// fire again.
    pub(crate) fn consumed() -> Self {
        Self {
            has_advanced: AtomicBool::new(true),
        }
    }
}

impl StepAdvanceCondition for OneShotAdvanceCondition {
    fn should_advance_step(
        &self,
        _inputs: &StepAdvanceInputs,
        _current_state: &TripNavigationState,
    ) -> StepAdvanceDecision {
        let had_already_advanced = self.has_advanced.swap(true, Ordering::SeqCst);
        if had_already_advanced {
            StepAdvanceDecision::Remain
        } else {
            StepAdvanceDecision::Advance
        }
    }

    fn fresh_instance(&self) -> Arc<dyn StepAdvanceCondition> {
        Arc::new(Self {
            has_advanced: AtomicBool::new(false),
        })
    }
}

/// A stateful condition that advances only once it has observed enough updates
/// within one instance, and resets its counter on
/// [`StepAdvanceCondition::fresh_instance`]. Used to prove that a stale
/// condition instance cannot cascade through the steps.
#[derive(Debug)]
pub(crate) struct UpdatesBeforeAdvanceCondition {
    minimum_updates_before_advance: usize,
    observed_updates: AtomicUsize,
}

impl UpdatesBeforeAdvanceCondition {
    /// Builds the condition requiring `minimum_updates_before_advance` earlier
    /// observations before its first advance.
    pub(crate) fn requiring(minimum_updates_before_advance: usize) -> Self {
        Self {
            minimum_updates_before_advance,
            observed_updates: AtomicUsize::new(0),
        }
    }
}

impl StepAdvanceCondition for UpdatesBeforeAdvanceCondition {
    fn should_advance_step(
        &self,
        _inputs: &StepAdvanceInputs,
        _current_state: &TripNavigationState,
    ) -> StepAdvanceDecision {
        let observed_so_far = self.observed_updates.fetch_add(1, Ordering::SeqCst);
        if observed_so_far >= self.minimum_updates_before_advance {
            StepAdvanceDecision::Advance
        } else {
            StepAdvanceDecision::Remain
        }
    }

    fn fresh_instance(&self) -> Arc<dyn StepAdvanceCondition> {
        Arc::new(Self::requiring(self.minimum_updates_before_advance))
    }
}

/// A deviation detector fixture that never flags. The advance-schedule tests
/// use counting conditions that legitimately decouple the step position from
/// the walker's fixes; this fixture isolates those tests' fresh-instance
/// contract from deviation semantics, which the deviation module's own tests
/// and the reducer's excursion tests specify.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NeverDeviatingDeviationDetector;

impl DeviationDetector for NeverDeviatingDeviationDetector {
    fn deviation_status(
        &self,
        _inputs: &crate::deviation::DeviationDetectionInputs,
        _current_state: &TripNavigationState,
    ) -> DeviationStatus {
        DeviationStatus::OnRoute
    }

    fn fresh_instance(&self) -> Arc<dyn DeviationDetector> {
        Arc::new(*self)
    }
}
