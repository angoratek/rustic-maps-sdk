//! Step-advance conditions.
//!
//! A [`StepAdvanceCondition`] is a pure decision object consulted by the
//! reducer on every location update. Conditions are passed into the reducer as
//! `Arc<dyn StepAdvanceCondition>` parameters and never live inside the
//! exported [`TripNavigationState`], which must stay plain data for a language
//! binding.
//!
//! # The fresh-instance contract
//!
//! [`StepAdvanceCondition::fresh_instance`] returns the same configuration with
//! any internal state reset. The reducer swaps the active condition for
//! `fresh_instance()` the moment it advances. This prevents a rapid
//! double advance: a condition that tracks progress within a step (for example,
//! one awaiting several consecutive fixes) would otherwise carry saturated
//! internal state across the advance and immediately advance again on the next
//! update. Stateless conditions can simply return a clone of themselves, but
//! the contract is uniform so composite conditions can reset every nested
//! condition recursively.
//!
//! # Combinators
//!
//! [`OrStepAdvanceCondition`] and [`AndStepAdvanceCondition`] wrap two inner
//! conditions and decide on their combined verdicts. Both consult every inner
//! condition on every update (even when the outcome is already decided for
//! `And`): a stateful inner condition must observe every update for its
//! internal schedule to stay sound. `fresh_instance` rebuilds both inner
//! conditions fresh, so the reducer's whole-composite reset on advance also
//! resets each child.

use std::sync::Arc;

use geospatial_types::{GeographicCoordinate, RouteStep};

use crate::state::TripNavigationState;

/// Threshold, in meters from the end of the current step, below which
/// `DistanceToEndOfStepCondition` advances. It is large enough to absorb GNSS
/// noise near the maneuver and small enough not to swallow short steps.
pub const DEFAULT_DISTANCE_TO_END_OF_STEP_THRESHOLD_METERS: f64 = 20.0;

/// Threshold, in meters from the current step's polyline, beyond which
/// `DistanceFromStepCondition` advances.
pub const DEFAULT_DISTANCE_FROM_STEP_THRESHOLD_METERS: f64 = 15.0;

/// Fixes with a reported horizontal accuracy worse than this, in meters, may
/// never trigger an advance in the distance conditions.
pub const DEFAULT_MINIMUM_HORIZONTAL_ACCURACY_METERS: f64 = 15.0;

/// Decision the reducer acts on after consulting a condition.
///
/// Distinct from a boolean only in name: it keeps the reducer's `match`
/// explicit and leaves room for later decision kinds without changing the
/// trait signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepAdvanceDecision {
    /// Stay on the current step.
    Remain,
    /// Move to the next step (or finish the trip on the final step).
    Advance,
}

/// Geometry and accuracy context the reducer computes once per update.
///
/// Conditions receive precomputed measurements instead of route internals so
/// they stay testable against synthetic inputs and the snapping error model
/// (see [`crate::geometry`]) is owned by the reducer alone.
///
/// Distances use the reported position fix (before snapping).
#[derive(Debug, Clone, PartialEq)]
pub struct StepAdvanceInputs<'a> {
    /// The raw position fix of this update, before snapping.
    pub coordinate: &'a GeographicCoordinate,
    /// Reported horizontal accuracy of the fix, in meters; `None` when the
    /// fix source does not report one.
    pub horizontal_accuracy_meters: Option<f64>,
    /// The step being traversed.
    pub current_step: &'a RouteStep,
    /// The step after the current one, when the current step is not the final step.
    pub next_step: Option<&'a RouteStep>,
    /// The fix snapped onto the current step's shape, when the shape allowed
    /// snapping.
    pub snapped_coordinate: Option<&'a GeographicCoordinate>,
    /// Straight-line distance, in meters, from the raw fix to the terminal
    /// coordinate of the current step's shape; `None` when the shape defines
    /// geometry.
    pub distance_to_end_of_step_meters: Option<f64>,
    /// Distance, in meters, from the raw fix to the current step's polyline;
    /// `None` when the shape defines geometry.
    pub distance_from_step_meters: Option<f64>,
}

impl<'a> StepAdvanceInputs<'a> {
    /// Whether this fix may trigger an advance at all: a fix without an
    /// accuracy report or with accuracy worse than the minimum must not, so a
    /// wildly wrong position cannot skip a maneuver. A fix without an
    /// accuracy report is treated as unusable as well — conservative, because
    /// an advance is irreversible.
    pub fn is_accuracy_within_minimum_meters(
        &self,
        minimum_horizontal_accuracy_meters: f64,
    ) -> bool {
        self.horizontal_accuracy_meters
            .is_some_and(|reported_accuracy_meters| {
                reported_accuracy_meters <= minimum_horizontal_accuracy_meters
            })
    }
}

/// Decides when the reducer moves from the current step to the next one.
///
/// Implementations must be [`Send`] and [`Sync`] because Arc'd conditions are
/// shared with platform wrappers and worker threads.
pub trait StepAdvanceCondition: Send + Sync {
    /// Consulted by the reducer on every location update.
    fn should_advance_step(
        &self,
        inputs: &StepAdvanceInputs,
        current_state: &TripNavigationState,
    ) -> StepAdvanceDecision;

    /// Returns the same configuration with any internal state reset.
    ///
    /// The reducer installs this instance whenever it advances, so internal
    /// progress (fix counters, entry/exit flags, and so on) cannot leak from
    /// one step into the next; see the module documentation for why the reset
    /// prevents rapid double-advance.
    fn fresh_instance(&self) -> Arc<dyn StepAdvanceCondition>;
}

/// Never advances by itself: step changes happen only when driven
/// programmatically from outside the reducer.
///
/// Useful for custom behavior where a host platform owns the advance decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManualStepAdvanceCondition;

impl Default for ManualStepAdvanceCondition {
    fn default() -> Self {
        Self
    }
}

impl StepAdvanceCondition for ManualStepAdvanceCondition {
    fn should_advance_step(
        &self,
        _inputs: &StepAdvanceInputs,
        _current_state: &TripNavigationState,
    ) -> StepAdvanceDecision {
        StepAdvanceDecision::Remain
    }

    fn fresh_instance(&self) -> Arc<dyn StepAdvanceCondition> {
        Arc::new(*self)
    }
}

/// Advances when the position fix comes within a threshold of the end of the
/// current step, eager-advance semantics.
///
/// The advance happens as soon as the fix is close enough to the maneuver
/// point, which makes the instruction switch punctual but sensitive to noisy
/// fixes near the step end (hence the accuracy gate).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DistanceToEndOfStepCondition {
    /// Distance, in meters, from the end of the step below which to advance.
    pub advance_threshold_meters: f64,
    /// Fixes with a reported accuracy worse than this, in meters, never
    /// advance.
    pub minimum_horizontal_accuracy_meters: f64,
}

impl DistanceToEndOfStepCondition {
    /// Builds the condition with the SDK's recommended defaults for a driving
    /// profile.
    pub fn with_ferrostar_defaults() -> Self {
        Self {
            advance_threshold_meters: DEFAULT_DISTANCE_TO_END_OF_STEP_THRESHOLD_METERS,
            minimum_horizontal_accuracy_meters: DEFAULT_MINIMUM_HORIZONTAL_ACCURACY_METERS,
        }
    }
}

impl StepAdvanceCondition for DistanceToEndOfStepCondition {
    fn should_advance_step(
        &self,
        inputs: &StepAdvanceInputs,
        _current_state: &TripNavigationState,
    ) -> StepAdvanceDecision {
        let may_consider =
            inputs.is_accuracy_within_minimum_meters(self.minimum_horizontal_accuracy_meters);
        let is_within_threshold =
            inputs
                .distance_to_end_of_step_meters
                .is_some_and(|distance_to_end_meters| {
                    distance_to_end_meters <= self.advance_threshold_meters
                });
        if may_consider && is_within_threshold {
            StepAdvanceDecision::Advance
        } else {
            StepAdvanceDecision::Remain
        }
    }

    /// Stateless: every evaluation recomputes from the inputs alone, so a
    /// fresh instance is just a clone of the configuration.
    fn fresh_instance(&self) -> Arc<dyn StepAdvanceCondition> {
        Arc::new(*self)
    }
}

/// Advances when the position fix moves far enough from the current step's
/// polyline, delayed-advance semantics.
///
/// The advance fires only once the fix has left the step neighborhood
/// entirely, which is robust to pausing at intersections and to
/// self-intersecting steps — but less robust on very short steps whose start
/// sits near the previous step's end.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DistanceFromStepCondition {
    /// Distance, in meters, from the step polyline beyond which to advance
    /// (strictly greater than).
    pub advance_threshold_meters: f64,
    /// Fixes with a reported accuracy worse than this, in meters, never
    /// advance.
    pub minimum_horizontal_accuracy_meters: f64,
}

impl DistanceFromStepCondition {
    /// Builds the condition with the SDK's recommended defaults for a driving
    /// profile.
    pub fn with_ferrostar_defaults() -> Self {
        Self {
            advance_threshold_meters: DEFAULT_DISTANCE_FROM_STEP_THRESHOLD_METERS,
            minimum_horizontal_accuracy_meters: DEFAULT_MINIMUM_HORIZONTAL_ACCURACY_METERS,
        }
    }
}

impl StepAdvanceCondition for DistanceFromStepCondition {
    fn should_advance_step(
        &self,
        inputs: &StepAdvanceInputs,
        _current_state: &TripNavigationState,
    ) -> StepAdvanceDecision {
        let may_consider =
            inputs.is_accuracy_within_minimum_meters(self.minimum_horizontal_accuracy_meters);
        let is_beyond_threshold =
            inputs
                .distance_from_step_meters
                .is_some_and(|distance_from_step_meters| {
                    distance_from_step_meters > self.advance_threshold_meters
                });
        if may_consider && is_beyond_threshold {
            StepAdvanceDecision::Advance
        } else {
            StepAdvanceDecision::Remain
        }
    }

    /// Stateless: every evaluation recomputes from the inputs alone, so a
    /// fresh instance is just a clone of the configuration.
    fn fresh_instance(&self) -> Arc<dyn StepAdvanceCondition> {
        Arc::new(*self)
    }
}

/// Advances if either inner condition advances (OR).
///
/// Ideal for short-circuit strategies: for example a large-distance guard
/// reacting to the fix leaving the step neighborhood, combined with the
/// routine advance behavior as the second condition.
#[derive(Clone)]
pub struct OrStepAdvanceCondition {
    /// The first condition consulted.
    pub first_condition: Arc<dyn StepAdvanceCondition>,
    /// The second condition consulted.
    pub second_condition: Arc<dyn StepAdvanceCondition>,
}

impl StepAdvanceCondition for OrStepAdvanceCondition {
    fn should_advance_step(
        &self,
        inputs: &StepAdvanceInputs,
        current_state: &TripNavigationState,
    ) -> StepAdvanceDecision {
        // Both inner conditions are consulted even after the decision is
        // already settled: a stateful condition must observe every update for
        // its schedule to stay sound.
        let first_decision = self
            .first_condition
            .should_advance_step(inputs, current_state);
        let second_decision = self
            .second_condition
            .should_advance_step(inputs, current_state);
        if first_decision == StepAdvanceDecision::Advance
            || second_decision == StepAdvanceDecision::Advance
        {
            StepAdvanceDecision::Advance
        } else {
            StepAdvanceDecision::Remain
        }
    }

    /// Rebuilds both inner conditions fresh so the reducer's post-advance swap
    /// can never leak a consumed child into the next step (the double-advance
    /// contract).
    fn fresh_instance(&self) -> Arc<dyn StepAdvanceCondition> {
        Arc::new(Self {
            first_condition: self.first_condition.fresh_instance(),
            second_condition: self.second_condition.fresh_instance(),
        })
    }
}

impl core::fmt::Debug for OrStepAdvanceCondition {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("OrStepAdvanceCondition")
            .field("first_condition", &"Arc<dyn StepAdvanceCondition>")
            .field("second_condition", &"Arc<dyn StepAdvanceCondition>")
            .finish()
    }
}

/// Advances only if both inner conditions advance (AND).
#[derive(Clone)]
pub struct AndStepAdvanceCondition {
    /// The first condition consulted.
    pub first_condition: Arc<dyn StepAdvanceCondition>,
    /// The second condition consulted.
    pub second_condition: Arc<dyn StepAdvanceCondition>,
}

impl StepAdvanceCondition for AndStepAdvanceCondition {
    fn should_advance_step(
        &self,
        inputs: &StepAdvanceInputs,
        current_state: &TripNavigationState,
    ) -> StepAdvanceDecision {
        // As with the OR combinator, the second condition is consulted even
        // when the first already blocks, keeping stateful schedules observing
        // every update.
        let first_decision = self
            .first_condition
            .should_advance_step(inputs, current_state);
        let second_decision = self
            .second_condition
            .should_advance_step(inputs, current_state);
        if first_decision == StepAdvanceDecision::Advance
            && second_decision == StepAdvanceDecision::Advance
        {
            StepAdvanceDecision::Advance
        } else {
            StepAdvanceDecision::Remain
        }
    }

    /// Rebuilds both inner conditions fresh (the double-advance contract).
    fn fresh_instance(&self) -> Arc<dyn StepAdvanceCondition> {
        Arc::new(Self {
            first_condition: self.first_condition.fresh_instance(),
            second_condition: self.second_condition.fresh_instance(),
        })
    }
}

impl core::fmt::Debug for AndStepAdvanceCondition {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("AndStepAdvanceCondition")
            .field("first_condition", &"Arc<dyn StepAdvanceCondition>")
            .field("second_condition", &"Arc<dyn StepAdvanceCondition>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use geospatial_types::{GeographicCoordinate, ManeuverType, RouteStep};

    use super::{
        AndStepAdvanceCondition, DEFAULT_DISTANCE_FROM_STEP_THRESHOLD_METERS,
        DEFAULT_DISTANCE_TO_END_OF_STEP_THRESHOLD_METERS,
        DEFAULT_MINIMUM_HORIZONTAL_ACCURACY_METERS, DistanceFromStepCondition,
        DistanceToEndOfStepCondition, ManualStepAdvanceCondition, OrStepAdvanceCondition,
        StepAdvanceCondition, StepAdvanceDecision, StepAdvanceInputs,
    };
    use crate::state::{NavigationPhase, TripNavigationState, TripProgress};
    use crate::test_support::{
        OneShotAdvanceCondition, TEST_SEGMENT_SPAN_METERS, offset_coordinate, test_origin,
    };

    /// A straight one-step fixture; the conditions under test treat the
    /// measurements as inputs, so the geometry only anchors the record.
    fn straight_step() -> RouteStep {
        RouteStep {
            distance_meters: TEST_SEGMENT_SPAN_METERS,
            duration_seconds: 100.0,
            maneuver_type: ManeuverType::Departure,
            instruction_display_text: "Straight ahead".to_string(),
            instruction_speech_text: "Straight ahead".to_string(),
            shape: vec![
                test_origin(),
                offset_coordinate(TEST_SEGMENT_SPAN_METERS, 0.0),
            ],
            lane_guidances: Vec::new(),
        }
    }

    fn inputs<'a>(
        current_step: &'a RouteStep,
        raw_coordinate: &'a GeographicCoordinate,
        distance_to_end_of_step_meters: Option<f64>,
        distance_from_step_meters: Option<f64>,
        horizontal_accuracy_meters: Option<f64>,
    ) -> StepAdvanceInputs<'a> {
        StepAdvanceInputs {
            coordinate: raw_coordinate,
            horizontal_accuracy_meters,
            current_step,
            next_step: None,
            snapped_coordinate: None,
            distance_to_end_of_step_meters,
            distance_from_step_meters,
        }
    }

    /// Convenience wrapper: the raw fix is the step's starting coordinate.
    fn inputs_along_step(
        current_step: &RouteStep,
        distance_to_end_of_step_meters: Option<f64>,
        distance_from_step_meters: Option<f64>,
        horizontal_accuracy_meters: Option<f64>,
    ) -> StepAdvanceInputs<'_> {
        inputs(
            current_step,
            &current_step.shape[0],
            distance_to_end_of_step_meters,
            distance_from_step_meters,
            horizontal_accuracy_meters,
        )
    }

    fn empty_state() -> TripNavigationState {
        TripNavigationState {
            phase: NavigationPhase::Navigating,
            route: geospatial_types::RouteData {
                distance_meters: 0.0,
                duration_seconds: 0.0,
                bounding_box: geospatial_types::BoundingBox {
                    south_west: offset_coordinate(0.0, 0.0),
                    north_east: offset_coordinate(0.0, 0.0),
                },
                geometry: Vec::new(),
                legs: Vec::new(),
            },
            step_positions: Vec::new(),
            current_step_position: 0,
            progress: TripProgress::default(),
            deviation: None,
            course_heading_degrees: None,
            snapped_coordinate: None,
        }
    }

    #[test]
    fn ferrostar_defaults_match_the_named_constants() {
        let condition = DistanceToEndOfStepCondition::with_ferrostar_defaults();
        assert_eq!(
            condition.advance_threshold_meters,
            DEFAULT_DISTANCE_TO_END_OF_STEP_THRESHOLD_METERS
        );
        assert_eq!(
            condition.minimum_horizontal_accuracy_meters,
            DEFAULT_MINIMUM_HORIZONTAL_ACCURACY_METERS
        );
        let delayed_condition = DistanceFromStepCondition::with_ferrostar_defaults();
        assert_eq!(
            delayed_condition.advance_threshold_meters,
            DEFAULT_DISTANCE_FROM_STEP_THRESHOLD_METERS
        );
        assert_eq!(
            delayed_condition.minimum_horizontal_accuracy_meters,
            DEFAULT_MINIMUM_HORIZONTAL_ACCURACY_METERS
        );
    }

    #[test]
    fn distance_to_end_of_step_advances_at_or_below_the_threshold() {
        let condition = DistanceToEndOfStepCondition::with_ferrostar_defaults();
        let step = straight_step();
        let state = empty_state();
        let inputs = inputs_along_step(
            &step,
            Some(DEFAULT_DISTANCE_TO_END_OF_STEP_THRESHOLD_METERS),
            Some(0.0),
            Some(DEFAULT_MINIMUM_HORIZONTAL_ACCURACY_METERS),
        );
        assert_eq!(
            condition.should_advance_step(&inputs, &state),
            StepAdvanceDecision::Advance,
            "the gate is inclusive (at the threshold still advances)"
        );
    }

    #[test]
    fn distance_to_end_of_step_stays_above_the_threshold() {
        let condition = DistanceToEndOfStepCondition::with_ferrostar_defaults();
        let step = straight_step();
        let state = empty_state();
        let inputs = inputs_along_step(
            &step,
            Some(DEFAULT_DISTANCE_TO_END_OF_STEP_THRESHOLD_METERS * 2.0),
            Some(0.0),
            Some(DEFAULT_MINIMUM_HORIZONTAL_ACCURACY_METERS),
        );
        assert_eq!(
            condition.should_advance_step(&inputs, &state),
            StepAdvanceDecision::Remain
        );
    }

    #[test]
    fn distance_to_end_of_step_blocks_worse_than_minimum_accuracy() {
        let condition = DistanceToEndOfStepCondition::with_ferrostar_defaults();
        let step = straight_step();
        let state = empty_state();
        let inputs = inputs_along_step(
            &step,
            Some(0.0),
            Some(0.0),
            Some(DEFAULT_MINIMUM_HORIZONTAL_ACCURACY_METERS * 10.0),
        );
        assert_eq!(
            condition.should_advance_step(&inputs, &state),
            StepAdvanceDecision::Remain
        );
    }

    #[test]
    fn distance_to_end_of_step_blocks_fixes_without_an_accuracy_report() {
        let condition = DistanceToEndOfStepCondition::with_ferrostar_defaults();
        let step = straight_step();
        let state = empty_state();
        let inputs = inputs_along_step(&step, Some(0.0), Some(0.0), None);
        assert_eq!(
            condition.should_advance_step(&inputs, &state),
            StepAdvanceDecision::Remain
        );
    }

    #[test]
    fn distance_to_end_of_step_cannot_evaluate_without_step_geometry() {
        let condition = DistanceToEndOfStepCondition::with_ferrostar_defaults();
        let step = straight_step();
        let shapeless_step = RouteStep {
            shape: Vec::new(),
            ..step.clone()
        };
        let state = empty_state();
        let raw_coordinate = test_origin();
        let inputs = inputs(
            &shapeless_step,
            &raw_coordinate,
            None,
            None,
            Some(DEFAULT_MINIMUM_HORIZONTAL_ACCURACY_METERS),
        );
        assert_eq!(
            condition.should_advance_step(&inputs, &state),
            StepAdvanceDecision::Remain
        );
    }

    #[test]
    fn distance_from_step_advances_strictly_beyond_the_threshold() {
        let condition = DistanceFromStepCondition::with_ferrostar_defaults();
        let step = straight_step();
        let state = empty_state();
        let beyond_threshold_inputs = inputs_along_step(
            &step,
            Some(TEST_SEGMENT_SPAN_METERS),
            Some(DEFAULT_DISTANCE_FROM_STEP_THRESHOLD_METERS * 2.0),
            Some(DEFAULT_MINIMUM_HORIZONTAL_ACCURACY_METERS),
        );
        assert_eq!(
            condition.should_advance_step(&beyond_threshold_inputs, &state),
            StepAdvanceDecision::Advance
        );
        let at_threshold_inputs = inputs_along_step(
            &step,
            Some(TEST_SEGMENT_SPAN_METERS),
            Some(DEFAULT_DISTANCE_FROM_STEP_THRESHOLD_METERS),
            Some(DEFAULT_MINIMUM_HORIZONTAL_ACCURACY_METERS),
        );
        assert_eq!(
            condition.should_advance_step(&at_threshold_inputs, &state),
            StepAdvanceDecision::Remain,
            "Ferrostar's delayed condition is strictly greater than"
        );
    }

    #[test]
    fn distance_from_step_blocks_worse_than_minimum_accuracy() {
        let condition = DistanceFromStepCondition::with_ferrostar_defaults();
        let step = straight_step();
        let state = empty_state();
        let inputs = inputs_along_step(
            &step,
            Some(TEST_SEGMENT_SPAN_METERS),
            Some(DEFAULT_DISTANCE_FROM_STEP_THRESHOLD_METERS * 2.0),
            Some(DEFAULT_MINIMUM_HORIZONTAL_ACCURACY_METERS * 10.0),
        );
        assert_eq!(
            condition.should_advance_step(&inputs, &state),
            StepAdvanceDecision::Remain
        );
    }

    #[test]
    fn manual_condition_never_advances() {
        let condition = ManualStepAdvanceCondition;
        let step = straight_step();
        let state = empty_state();
        let inputs = inputs_along_step(&step, Some(0.0), Some(0.0), Some(0.0));
        assert_eq!(
            condition.should_advance_step(&inputs, &state),
            StepAdvanceDecision::Remain
        );
    }

    /// Combination rules, mirroring the reference's OR/AND semantics over two
    /// inner conditions.
    #[test]
    fn or_condition_advances_when_any_inner_condition_advances() {
        let condition: Arc<dyn StepAdvanceCondition> = Arc::new(OrStepAdvanceCondition {
            first_condition: Arc::new(ManualStepAdvanceCondition),
            second_condition: Arc::new(DistanceToEndOfStepCondition::with_ferrostar_defaults()),
        });
        let step = straight_step();
        let state = empty_state();
        let near_end_inputs = inputs_along_step(
            &step,
            Some(0.0),
            Some(0.0),
            Some(DEFAULT_MINIMUM_HORIZONTAL_ACCURACY_METERS),
        );
        assert_eq!(
            condition.should_advance_step(&near_end_inputs, &state),
            StepAdvanceDecision::Advance
        );
        let far_inputs = inputs_along_step(
            &step,
            Some(TEST_SEGMENT_SPAN_METERS),
            Some(0.0),
            Some(DEFAULT_MINIMUM_HORIZONTAL_ACCURACY_METERS),
        );
        assert_eq!(
            condition.should_advance_step(&far_inputs, &state),
            StepAdvanceDecision::Remain
        );
    }

    #[test]
    fn or_condition_never_advances_when_both_inner_conditions_remain() {
        let condition: Arc<dyn StepAdvanceCondition> = Arc::new(OrStepAdvanceCondition {
            first_condition: Arc::new(ManualStepAdvanceCondition),
            second_condition: Arc::new(ManualStepAdvanceCondition),
        });
        let step = straight_step();
        let state = empty_state();
        let near_end_inputs = inputs_along_step(
            &step,
            Some(0.0),
            Some(0.0),
            Some(DEFAULT_MINIMUM_HORIZONTAL_ACCURACY_METERS),
        );
        assert_eq!(
            condition.should_advance_step(&near_end_inputs, &state),
            StepAdvanceDecision::Remain
        );
    }

    #[test]
    fn and_condition_never_advances_when_one_inner_condition_blocks() {
        let condition: Arc<dyn StepAdvanceCondition> = Arc::new(AndStepAdvanceCondition {
            first_condition: Arc::new(ManualStepAdvanceCondition),
            second_condition: Arc::new(DistanceToEndOfStepCondition::with_ferrostar_defaults()),
        });
        let step = straight_step();
        let state = empty_state();
        let near_end_inputs = inputs_along_step(
            &step,
            Some(0.0),
            Some(0.0),
            Some(DEFAULT_MINIMUM_HORIZONTAL_ACCURACY_METERS),
        );
        assert_eq!(
            condition.should_advance_step(&near_end_inputs, &state),
            StepAdvanceDecision::Remain
        );
    }

    #[test]
    fn and_condition_advances_when_both_inner_conditions_advance() {
        let condition: Arc<dyn StepAdvanceCondition> = Arc::new(AndStepAdvanceCondition {
            first_condition: Arc::new(DistanceToEndOfStepCondition::with_ferrostar_defaults()),
            second_condition: Arc::new(DistanceToEndOfStepCondition::with_ferrostar_defaults()),
        });
        let step = straight_step();
        let state = empty_state();
        let near_end_inputs = inputs_along_step(
            &step,
            Some(0.0),
            Some(0.0),
            Some(DEFAULT_MINIMUM_HORIZONTAL_ACCURACY_METERS),
        );
        assert_eq!(
            condition.should_advance_step(&near_end_inputs, &state),
            StepAdvanceDecision::Advance
        );
    }

    #[test]
    fn fresh_instance_resets_stateful_condition_state() {
        let mut condition: Arc<dyn StepAdvanceCondition> =
            Arc::new(OneShotAdvanceCondition::unused());
        let step = straight_step();
        let state = empty_state();
        let first_inputs = inputs_along_step(&step, Some(0.0), Some(0.0), Some(0.0));

        assert_eq!(
            condition.should_advance_step(&first_inputs, &state),
            StepAdvanceDecision::Advance
        );
        // The consumed instance must not advance again…
        assert_eq!(
            condition.should_advance_step(&first_inputs, &state),
            StepAdvanceDecision::Remain
        );
        // …but a fresh instance of the same configuration must, once.
        condition = condition.fresh_instance();
        assert_eq!(
            condition.should_advance_step(&first_inputs, &state),
            StepAdvanceDecision::Advance
        );
        assert_eq!(
            condition.should_advance_step(&first_inputs, &state),
            StepAdvanceDecision::Remain
        );
    }

    /// A one-shot stateful condition wrapped in the OR combinator: consulting
    /// it fires the child once, the same composite instance cannot fire again,
    /// and `fresh_instance` rebuilds the child fresh so it can.
    #[test]
    fn or_fresh_instance_rebuilds_children_fresh() {
        let condition: Arc<dyn StepAdvanceCondition> = Arc::new(OrStepAdvanceCondition {
            first_condition: Arc::new(ManualStepAdvanceCondition),
            second_condition: Arc::new(OneShotAdvanceCondition::unused()),
        });
        let step = straight_step();
        let state = empty_state();
        let inputs = inputs_along_step(&step, Some(0.0), Some(0.0), Some(0.0));

        assert_eq!(
            condition.should_advance_step(&inputs, &state),
            StepAdvanceDecision::Advance
        );
        // The consumed child must not fire again inside the same instance…
        assert_eq!(
            condition.should_advance_step(&inputs, &state),
            StepAdvanceDecision::Remain
        );
        // …but the rebuilt children must each be able to fire once more.
        let fresh_condition = condition.fresh_instance();
        assert_eq!(
            fresh_condition.should_advance_step(&inputs, &state),
            StepAdvanceDecision::Advance
        );
    }

    /// The AND combinator behaves the same way with two stateful children:
    /// both fire on the first consult, and only fresh children fire again.
    #[test]
    fn and_fresh_instance_rebuilds_children_fresh() {
        let condition: Arc<dyn StepAdvanceCondition> = Arc::new(AndStepAdvanceCondition {
            first_condition: Arc::new(OneShotAdvanceCondition::unused()),
            second_condition: Arc::new(OneShotAdvanceCondition::unused()),
        });
        let step = straight_step();
        let state = empty_state();
        let inputs = inputs_along_step(&step, Some(0.0), Some(0.0), Some(0.0));

        assert_eq!(
            condition.should_advance_step(&inputs, &state),
            StepAdvanceDecision::Advance
        );
        assert_eq!(
            condition.should_advance_step(&inputs, &state),
            StepAdvanceDecision::Remain
        );
        let fresh_condition = condition.fresh_instance();
        assert_eq!(
            fresh_condition.should_advance_step(&inputs, &state),
            StepAdvanceDecision::Advance
        );
    }

    /// A stateful child consumed by earlier advances must not corrupt the
    /// schedule of a composite handed in for a new round of walking.
    #[test]
    fn or_condition_preserves_stateful_child_state_while_not_advancing() {
        // The one-shot child fires on the first consult, so the composite
        // advances; on the second consult its consumed child is observed (the
        // composite did not rebuild children mid-life), which the schedule of
        // the reducer-level walk tests prove end to end.
        let condition: Arc<dyn StepAdvanceCondition> = Arc::new(OrStepAdvanceCondition {
            first_condition: Arc::new(OneShotAdvanceCondition::consumed()),
            second_condition: Arc::new(ManualStepAdvanceCondition),
        });
        let step = straight_step();
        let state = empty_state();
        let inputs = inputs_along_step(&step, Some(0.0), Some(0.0), Some(0.0));
        assert_eq!(
            condition.should_advance_step(&inputs, &state),
            StepAdvanceDecision::Remain,
            "a consumed child blocks the composite until it is rebuilt fresh"
        );
    }
}
