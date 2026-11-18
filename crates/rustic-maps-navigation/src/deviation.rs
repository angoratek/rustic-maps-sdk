//! Deviation (off-route) detection, the second decision policy beside the
//! [`crate::step_advance`] conditions.
//!
//! A [`DeviationDetector`] is consulted by the reducer on every location
//! update, with the measurements the reducer already computed. The live
//! detector instance travels in
//! [`NavigationSession`](crate::reducer::NavigationSession) beside the state,
//! never inside the exported plain-data record.
//!
//! # Judgment semantics
//!
//! The shipped detector is a pure per-fix predicate over the measured
//! distance: a fix is off route when its distance to the remaining route line
//! exceeds the configured threshold widened by the fix's reported horizontal
//! accuracy. Widening is deliberate: the accuracy radius is exactly the
//! plausible gap between the raw fix and the truth, so a genuinely on-route
//! position may legitimately read up to one accuracy radius away from the
//! line. A widened allowance suppresses those false flags gracefully instead of
//! failing abruptly at a hard gate. The measurement is taken against the
//! *remaining route line* (the current step onward) rather than only the
//! current step, so a lagging step — eager advance semantics, pauses at
//! intersections — is never mistaken for a deviation.
//!
//! # Two grades
//!
//! A fix beyond the allowance from the remaining route line is judged fully
//! off route. A fix inside the remainder's allowance but beyond the allowance
//! from the **current step's** own polyline projects onto a future step and is
//! reported as [`DeviationStatus::OffStepOnRoute`] ("on route, wrong step").
//! A shortcut, a self-intersecting route, or a lagging step advance can all
//! legitimately put a fix on a later step, so the wrong-step grade is guidance
//! feedback: the reducer neither freezes on it nor asks for a reroute, and the
//! step-advance condition (with a delayed condition, within the very fix that
//! produced the grade, via the reducer's multi-step advance loop) reconciles
//! the step. Only [`DeviationStatus::OffRoute`] is the freezing grade.
//!
//! # Hysteresis
//!
//! The shipped detector uses one threshold for both raising and clearing a
//! deviation, so the flag can flicker around the allowance edge. Hysteresis (a
//! wider clearance than detection, or a minimum off-route time) is not part of
//! this detector.
//!
//! The reducer's contract with the detector, documented in
//! [`crate::reducer`]: a fix without an accuracy report is never judged at
//! all — it can neither raise nor clear a deviation.

use std::sync::Arc;

use geospatial_types::GeographicCoordinate;

use crate::state::TripNavigationState;

/// Deviation threshold, in meters from the remaining route line, beyond which
/// a fix is judged off route before any accuracy widening.
///
/// This is the default deviation distance of the shipped detector.
pub const DEFAULT_DEVIATION_DISTANCE_THRESHOLD_METERS: f64 = 25.0;

/// Verdict the detector returns for one position fix.
///
/// A per-fix judgment rather than a boolean so the caller learns *how far*
/// the fix sits from the route; the reducer records that measurement in the
/// state while the deviation stays active, and keys its freeze decision off
/// the grade.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DeviationStatus {
    /// The fix sits within the effective deviation allowance of both the
    /// current step and the remaining route line.
    OnRoute,
    /// The fix sits beyond the effective deviation allowance of the current
    /// step's polyline but within the allowance of the remaining route line:
    /// the fix projects onto a *future* step ("on route, wrong step"; see the
    /// module documentation). The reducer neither freezes nor asks for a
    /// reroute on this grade.
    OffStepOnRoute {
        /// Measured distance, in meters, from the raw fix to the remaining
        /// route line (small by this grade's construction — the fix projects
        /// onto the route, just not onto the current step).
        deviation_from_route_meters: f64,
    },
    /// The fix sits beyond the effective deviation allowance: it projects
    /// onto neither the current step nor any future step.
    OffRoute {
        /// Measured distance, in meters, from the raw fix to the remaining
        /// route line.
        deviation_from_route_meters: f64,
    },
}

/// Measurements and fix context for one deviation judgment.
///
/// As with [`crate::step_advance::StepAdvanceInputs`], the reducer computes
/// the geometry once per update; the detector judges measurements, never route
/// internals, and stays testable against synthetic numbers alone.
///
/// The two distance measurements are taken against the same shapes on every
/// judgment: the remaining route line (the current step's shape onward) and —
/// for the two-grade distinction — the current step's polyline alone.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviationDetectionInputs<'a> {
    /// The raw position fix of this update, before any snapping.
    pub coordinate: &'a GeographicCoordinate,
    /// Reported horizontal accuracy of the fix, in meters. By the reducer's
    /// contract this is always reported; an implementation that still sees
    /// `None` must treat the fix's accuracy as perfect (zero extra allowance).
    pub horizontal_accuracy_meters: Option<f64>,
    /// Distance, in meters, from the raw fix to the remaining route line
    /// (the current step's shape onward, per the reducer's measurement).
    /// `None` when the route defines no geometry at all.
    pub distance_from_route_meters: Option<f64>,
    /// Distance, in meters, from the raw fix to the *current step's* polyline
    /// alone; `None` when the current step defines no geometry.
    ///
    /// Always at least [`Self::distance_from_route_meters`], which takes the
    /// minimum over the current step and every future step. The two-grade
    /// judgment reads exactly that relation: a fix inside the remaining
    /// route's allowance but outside the current step's own allowance is on a
    /// future step.
    pub distance_from_current_step_meters: Option<f64>,
}

/// Decides for each position fix whether the trip has left the route.
///
/// Implementations must be [`Send`] and [`Sync`] because `Arc`'d detectors are
/// shared with platform wrappers and worker threads, exactly like the
/// step-advance conditions.
pub trait DeviationDetector: Send + Sync {
    /// Judges one fix. The reducer calls this with every fix that carries an
    /// accuracy report, before the step-advance condition is consulted.
    fn deviation_status(
        &self,
        inputs: &DeviationDetectionInputs,
        current_state: &TripNavigationState,
    ) -> DeviationStatus;

    /// Returns the same configuration with any internal state reset.
    ///
    /// The shipped detector is stateless, so this is a re-construction of the
    /// configuration, but the contract is uniform so the reducer can reset a
    /// future stateful detector when the route context changes (for example on
    /// [`crate::reducer::NavigationEvent::RerouteProvided`]) without knowing
    /// its internals.
    fn fresh_instance(&self) -> Arc<dyn DeviationDetector>;
}

/// Flags a deviation when the raw fix is farther from the remaining route line
/// than the configured threshold, widened by the fix's reported accuracy.
///
/// The shipped default detector; see the module documentation for the widening
/// rule and the deliberate absence of hysteresis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DistanceDeviationDetector {
    /// Distance, in meters, from the remaining route line beyond which a fix
    /// is judged off route (before any accuracy widening, strictly greater).
    pub deviation_threshold_meters: f64,
}

impl DistanceDeviationDetector {
    /// Builds the detector with the SDK's default deviation threshold.
    pub fn with_default_threshold() -> Self {
        Self {
            deviation_threshold_meters: DEFAULT_DEVIATION_DISTANCE_THRESHOLD_METERS,
        }
    }
}

impl DeviationDetector for DistanceDeviationDetector {
    fn deviation_status(
        &self,
        inputs: &DeviationDetectionInputs,
        _current_state: &TripNavigationState,
    ) -> DeviationStatus {
        let Some(distance_from_route_meters) = inputs.distance_from_route_meters else {
            return DeviationStatus::OnRoute;
        };
        let accuracy_widening_meters = inputs.horizontal_accuracy_meters.unwrap_or(0.0);
        // Strictly greater, in the sense of the delayed step-advance
        // condition: a fix exactly at the effective allowance is judged on
        // route, only beyond it flags.
        let effective_allowance_meters = self.deviation_threshold_meters + accuracy_widening_meters;
        if distance_from_route_meters > effective_allowance_meters {
            return DeviationStatus::OffRoute {
                deviation_from_route_meters: distance_from_route_meters,
            };
        }
        // Inside the remaining route line's allowance. The two-grade
        // distinction: a fix that still sits beyond the allowance from the
        // current step's own polyline projects onto a *future* step — on
        // route, wrong step. An unmeasurable current step (no shape) cannot
        // prove the fix off it, so the fix stays on route.
        match inputs.distance_from_current_step_meters {
            Some(distance_from_current_step_meters)
                if distance_from_current_step_meters > effective_allowance_meters =>
            {
                DeviationStatus::OffStepOnRoute {
                    deviation_from_route_meters: distance_from_route_meters,
                }
            }
            _ => DeviationStatus::OnRoute,
        }
    }

    /// Stateless: every judgment recomputes from the measurements alone, so a
    /// fresh instance is a re-construction of the configuration.
    fn fresh_instance(&self) -> Arc<dyn DeviationDetector> {
        Arc::new(*self)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use proptest::prelude::*;

    use geospatial_types::{BoundingBox, GeographicCoordinate, RouteData};

    use super::{
        DEFAULT_DEVIATION_DISTANCE_THRESHOLD_METERS, DeviationDetectionInputs, DeviationDetector,
        DeviationStatus, DistanceDeviationDetector,
    };
    use crate::state::{NavigationPhase, RouteStepPosition, TripNavigationState, TripProgress};
    use crate::test_support::{TEST_ACCURACY_METERS, test_origin};

    /// Horizontal accuracy, in meters, of the poor-fix cases: wide enough to
    /// matter against the default threshold.
    const TEST_POOR_ACCURACY_METERS: f64 = 50.0;

    /// An empty navigating state anchoring the `current_state` parameter; the
    /// shipped detector is a per-fix predicate and consumes no state.
    fn state() -> TripNavigationState {
        TripNavigationState {
            phase: NavigationPhase::Navigating,
            route: RouteData {
                distance_meters: 0.0,
                duration_seconds: 0.0,
                bounding_box: BoundingBox {
                    south_west: test_origin(),
                    north_east: test_origin(),
                },
                geometry: Vec::new(),
                legs: Vec::new(),
            },
            step_positions: Vec::<RouteStepPosition>::new(),
            current_step_position: 0,
            progress: TripProgress::default(),
            deviation: None,
            course_heading_degrees: None,
            snapped_coordinate: None,
        }
    }

    /// Builds detection inputs from measurements alone; the anchored
    /// `coordinate` is never read by the shipped detector. The current-step
    /// measurement defaults to the remaining-line measurement (a single-step
    /// route: no future step exists to differ), so the pre-two-grade tests
    /// keep their exact readings.
    fn inputs<'a>(
        coordinate: &'a GeographicCoordinate,
        distance_from_route_meters: Option<f64>,
        horizontal_accuracy_meters: Option<f64>,
    ) -> DeviationDetectionInputs<'a> {
        inputs_with_current_step(
            coordinate,
            distance_from_route_meters,
            distance_from_route_meters,
            horizontal_accuracy_meters,
        )
    }

    /// Builds detection inputs with the current-step measurement stated
    /// separately (the multi-step cases of the two-grade judgment).
    fn inputs_with_current_step<'a>(
        coordinate: &'a GeographicCoordinate,
        distance_from_route_meters: Option<f64>,
        distance_from_current_step_meters: Option<f64>,
        horizontal_accuracy_meters: Option<f64>,
    ) -> DeviationDetectionInputs<'a> {
        DeviationDetectionInputs {
            coordinate,
            horizontal_accuracy_meters,
            distance_from_route_meters,
            distance_from_current_step_meters,
        }
    }

    #[test]
    fn default_threshold_matches_the_named_constant() {
        let detector = DistanceDeviationDetector::with_default_threshold();
        assert_eq!(
            detector.deviation_threshold_meters,
            DEFAULT_DEVIATION_DISTANCE_THRESHOLD_METERS
        );
    }

    #[test]
    fn fix_within_the_allowance_stays_on_route() {
        let detector = DistanceDeviationDetector::with_default_threshold();
        let state = state();
        let coordinate = test_origin();
        let inputs = inputs(&coordinate, Some(10.0), Some(TEST_ACCURACY_METERS));
        assert_eq!(
            detector.deviation_status(&inputs, &state),
            DeviationStatus::OnRoute
        );
    }

    #[test]
    fn fix_exactly_at_the_allowance_stays_on_route() {
        let detector = DistanceDeviationDetector::with_default_threshold();
        let state = state();
        let coordinate = test_origin();
        let allowance_meters = DEFAULT_DEVIATION_DISTANCE_THRESHOLD_METERS + TEST_ACCURACY_METERS;
        let inputs = inputs(
            &coordinate,
            Some(allowance_meters),
            Some(TEST_ACCURACY_METERS),
        );
        assert_eq!(
            detector.deviation_status(&inputs, &state),
            DeviationStatus::OnRoute,
            "the judgment is strictly beyond, not at, the allowance"
        );
    }

    #[test]
    fn fix_beyond_the_allowance_is_flagged_with_its_distance() {
        let detector = DistanceDeviationDetector::with_default_threshold();
        let state = state();
        let coordinate = test_origin();
        let inputs = inputs(&coordinate, Some(100.0), Some(TEST_ACCURACY_METERS));
        assert_eq!(
            detector.deviation_status(&inputs, &state),
            DeviationStatus::OffRoute {
                deviation_from_route_meters: 100.0
            }
        );
    }

    #[test]
    fn poor_fix_widens_the_allowance() {
        let detector = DistanceDeviationDetector::with_default_threshold();
        let state = state();
        let coordinate = test_origin();
        // 30 m from the route with a 2 m envelope is beyond the 27 m
        // effective allowance…
        let accurate_inputs = inputs(&coordinate, Some(30.0), Some(TEST_ACCURACY_METERS));
        assert_eq!(
            detector.deviation_status(&accurate_inputs, &state),
            DeviationStatus::OffRoute {
                deviation_from_route_meters: 30.0
            }
        );
        // …but the same reading with a 50 m envelope sits inside the 75 m
        // effective allowance: the fix cannot claim the truth is off the
        // route, so no deviation is raised.
        let inaccurate_inputs = inputs(&coordinate, Some(30.0), Some(TEST_POOR_ACCURACY_METERS));
        assert_eq!(
            detector.deviation_status(&inaccurate_inputs, &state),
            DeviationStatus::OnRoute
        );
    }

    #[test]
    fn fix_without_measured_distance_cannot_deviate() {
        let detector = DistanceDeviationDetector::with_default_threshold();
        let state = state();
        let coordinate = test_origin();
        let inputs = inputs(&coordinate, None, Some(TEST_ACCURACY_METERS));
        assert_eq!(
            detector.deviation_status(&inputs, &state),
            DeviationStatus::OnRoute,
            "without a measurement there is nothing to judge"
        );
    }

    #[test]
    fn fix_on_a_future_step_but_beyond_the_current_steps_allowance_is_off_step() {
        let detector = DistanceDeviationDetector::with_default_threshold();
        let state = state();
        let coordinate = test_origin();
        // The fix projects 5 m from a *future* step of the remaining route
        // line but 400 m from the current step's own polyline: on route,
        // wrong step.
        let allowance_meters = DEFAULT_DEVIATION_DISTANCE_THRESHOLD_METERS + TEST_ACCURACY_METERS;
        let inputs = inputs_with_current_step(
            &coordinate,
            Some(allowance_meters),
            Some(allowance_meters * 10.0),
            Some(TEST_ACCURACY_METERS),
        );
        assert_eq!(
            detector.deviation_status(&inputs, &state),
            DeviationStatus::OffStepOnRoute {
                deviation_from_route_meters: allowance_meters,
            },
            "a fix on a future step is the wrong-step grade, not a freeze"
        );
    }

    #[test]
    fn fix_within_the_current_steps_allowance_is_ordinary_on_route() {
        let detector = DistanceDeviationDetector::with_default_threshold();
        let state = state();
        let coordinate = test_origin();
        let inputs = inputs_with_current_step(
            &coordinate,
            Some(10.0),
            Some(10.0),
            Some(TEST_ACCURACY_METERS),
        );
        assert_eq!(
            detector.deviation_status(&inputs, &state),
            DeviationStatus::OnRoute,
            "a fix the current step accounts for is not a wrong-step reading"
        );
    }

    #[test]
    fn fix_beyond_both_allowances_is_fully_off_route_not_wrong_step() {
        let detector = DistanceDeviationDetector::with_default_threshold();
        let state = state();
        let coordinate = test_origin();
        let inputs = inputs_with_current_step(
            &coordinate,
            Some(100.0),
            Some(100.0),
            Some(TEST_ACCURACY_METERS),
        );
        assert_eq!(
            detector.deviation_status(&inputs, &state),
            DeviationStatus::OffRoute {
                deviation_from_route_meters: 100.0
            },
            "a fix beyond the remaining route line's allowance freezes regardless \
             of the current step's own reading"
        );
    }

    #[test]
    fn the_widened_allowance_gates_the_wrong_step_grade_too() {
        let detector = DistanceDeviationDetector::with_default_threshold();
        let state = state();
        let coordinate = test_origin();
        // A poor fix's widened allowance absorbs a moderate current-step
        // excursion: the same geometry an accurate fix grades wrong-step
        // stays ordinary on route for a poor one.
        let current_step_excursion_meters = 30.0;
        let accurate_inputs = inputs_with_current_step(
            &coordinate,
            Some(5.0),
            Some(current_step_excursion_meters),
            Some(TEST_ACCURACY_METERS),
        );
        assert_eq!(
            detector.deviation_status(&accurate_inputs, &state),
            DeviationStatus::OffStepOnRoute {
                deviation_from_route_meters: 5.0,
            }
        );
        let inaccurate_inputs = inputs_with_current_step(
            &coordinate,
            Some(5.0),
            Some(current_step_excursion_meters),
            Some(TEST_POOR_ACCURACY_METERS),
        );
        assert_eq!(
            detector.deviation_status(&inaccurate_inputs, &state),
            DeviationStatus::OnRoute,
            "the accuracy widening applies to the wrong-step grade as well"
        );
    }

    #[test]
    fn fix_at_the_current_steps_allowance_boundary_is_on_route() {
        let detector = DistanceDeviationDetector::with_default_threshold();
        let state = state();
        let coordinate = test_origin();
        let allowance_meters = DEFAULT_DEVIATION_DISTANCE_THRESHOLD_METERS + TEST_ACCURACY_METERS;
        let inputs = inputs_with_current_step(
            &coordinate,
            Some(allowance_meters),
            Some(allowance_meters),
            Some(TEST_ACCURACY_METERS),
        );
        assert_eq!(
            detector.deviation_status(&inputs, &state),
            DeviationStatus::OnRoute,
            "exactly at the current step's allowance is not beyond it"
        );
    }

    #[test]
    fn unmeasurable_current_step_cannot_claim_the_wrong_step_grade() {
        let detector = DistanceDeviationDetector::with_default_threshold();
        let state = state();
        let coordinate = test_origin();
        let within_allowance_meters = 10.0;
        let inputs = inputs_with_current_step(
            &coordinate,
            Some(within_allowance_meters),
            None,
            Some(TEST_ACCURACY_METERS),
        );
        assert_eq!(
            detector.deviation_status(&inputs, &state),
            DeviationStatus::OnRoute,
            "a shapeless current step cannot prove the fix off it"
        );
    }

    #[test]
    fn fresh_instance_carries_the_same_configuration() {
        let detector: Arc<dyn DeviationDetector> =
            Arc::new(DistanceDeviationDetector::with_default_threshold());
        let state = state();
        let coordinate = test_origin();
        let inputs = inputs(&coordinate, Some(100.0), Some(TEST_ACCURACY_METERS));
        let expected_status = DeviationStatus::OffRoute {
            deviation_from_route_meters: 100.0,
        };
        assert_eq!(detector.deviation_status(&inputs, &state), expected_status);
        assert_eq!(
            detector.fresh_instance().deviation_status(&inputs, &state),
            expected_status
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// The three grades partition every (remaining-line, current-step)
        /// measurement pair exactly: the judgment equals the piecewise
       /// definition — `OffRoute` beyond the remaining line's allowance,
        /// `OffStepOnRoute` inside it but beyond the current step's own
        /// allowance, `OnRoute` otherwise — with the same widening applied to
        /// both comparisons.
        #[test]
        fn the_three_grades_partition_the_measurements_exactly(
            distance_from_route_meters in 0.0f64..500.0,
            distance_from_current_step_meters in 0.0f64..500.0,
            accuracy_widening_meters in 0.0f64..100.0,
        ) {
            // The remaining route line contains the current step's polyline,
            // so its measurement can only be the nearer one.
            let remaining_line_distance = distance_from_route_meters.min(
                distance_from_current_step_meters,
            );
            let detector = DistanceDeviationDetector::with_default_threshold();
            let state = state();
            let coordinate = test_origin();
            let inputs = inputs_with_current_step(
                &coordinate,
                Some(remaining_line_distance),
                Some(distance_from_current_step_meters),
                Some(accuracy_widening_meters),
            );
            let effective_allowance_meters =
                DEFAULT_DEVIATION_DISTANCE_THRESHOLD_METERS + accuracy_widening_meters;
            let expected_status = if remaining_line_distance
                > effective_allowance_meters
            {
                DeviationStatus::OffRoute {
                    deviation_from_route_meters: remaining_line_distance,
                }
            } else if distance_from_current_step_meters
                > effective_allowance_meters
            {
                DeviationStatus::OffStepOnRoute {
                    deviation_from_route_meters: remaining_line_distance,
                }
            } else {
                DeviationStatus::OnRoute
            };
            prop_assert_eq!(detector.deviation_status(&inputs, &state), expected_status);
        }
    }
}
