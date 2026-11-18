//! The pure event reducer driving an active trip.
//!
//! [`reduce_navigation_event`] maps one event onto the previous session and
//! returns the next one; it has no I/O, no clock and no mutable globals, so
//! every outcome is reproducible from the event history. The live
//! [`StepAdvanceCondition`] and [`DeviationDetector`] instances travel through
//! [`NavigationSession`], pairing state with its decision objects because they
//! are swapped for their [`StepAdvanceCondition::fresh_instance`] (or
//! [`DeviationDetector::fresh_instance`]) counterparts at reset points, and the
//! exported [`crate::state::TripNavigationState`] must stay free of trait
//! objects.
//!
//! # Reducer semantics
//!
//! - [`NavigationEvent::StartTrip`] flattens the route and reports the declared
//!   step values as remaining progress until the first fix arrives.
//! - [`NavigationEvent::LocationUpdated`] is judged for deviation first:
//!   a fix the detector grades *fully* off route *freezes* the session — the
//!   snapped coordinate and progress stay at the last on-route values, and
//!   neither the step-advance condition is consulted nor the step advances.
//!   Deviating positions must not drive route progress. A fix graded *off the
//!   current step but on the route* ([`DeviationGrade::OffStepOnRoute`], a
//!   projection onto a future step) never freezes on its own: the grade is
//!   recorded in the state, the trip stays `Navigating` — no reroute request,
//!   instructions keep counting down — and the fix is then processed normally
//!   (the step-advance condition reconciles the step; a delayed condition does
//!   so within the very fix, via the multi-step advance loop below). A fix
//!   without an accuracy report is not judged at all: it can neither raise nor
//!   clear a deviation, because one threshold rule cannot conservatively decide
//!   both directions. A graded fix inside the allowance lifts an active
//!   deviation (an `OffRoute` trip recovering from the route line, or a
//!   lingering wrong-step grade clearing) and the update then proceeds normally
//!   (condition consulted, position snapped, progress recomputed).
//!   [`crate::state::NavigationPhase::OffRoute`] is therefore a pause the user
//!   may recover from by returning to the route, not a terminal state.
//! - **Arrival-aware advance:** the condition consulted for a hop is not always
//!   the trip's own. Once the remaining steps — counted with the current step
//!   itself included — shrink to the arrival window
//!   ([`ARRIVAL_CONDITION_REMAINING_STEPS_BOUND`]), a separately configured
//!   *arrival* step-advance condition is consulted instead (falling back to the
//!   trip's own condition when none was configured, which preserves the
//!   behavior of every trip that has none). Arrival itself is reachable only
//!   through that final-step verdict: an `Advance` while on the final step
//!   cannot pop another step — it declares
//!   [`NavigationPhase::Arrived`] and
//!   breaks the loop ahead of any further consult, so neither the loop's
//!   defensive bound nor a still advancing condition can walk past the route's
//!   end.
//! - The trip's course-filtering policy stamps an effective course
//!   ([`TripNavigationState::course_heading_degrees`]) during each processed
//!   fix's position snap; see [`crate::course_filtering`].
//! - One fix may advance through *several* steps: after each single-hop advance
//!   — which swaps in the fresh condition instance — the freshly computed
//!   inputs decide whether the same fix advances the next hop too. The loop is
//!   bounded by [`MAXIMUM_STEP_ADVANCES_PER_FIX`] and terminates by
//!   construction: a hop can end the trip only by arriving, and every other
//!   break is the condition's `Remain`.
//! - [`NavigationEvent::RerouteProvided`] rebuilds the session for the same
//!   trip from a fresh route. Route computation policy lives at the caller's
//!   event source; the reducer stays policy-free and pure. See the event
//!   documentation for the route-anchoring contract it implies.
//! - Updates after the phase is `Arrived` are ignored; reroutes offered to an
//!   arrived trip are ignored as well.

use std::sync::Arc;

use geospatial_types::{GeographicCoordinate, RouteData};

use crate::course_filtering::{CourseFiltering, effective_course_heading_degrees};
use crate::deviation::{DeviationDetectionInputs, DeviationDetector, DeviationStatus};
use crate::geometry::snap_coordinate_to_polyline;
use crate::location::NavigationLocationUpdate;
use crate::state::{
    DeviationGrade, DeviationInfo, NavigationPhase, TripNavigationState, TripProgress,
};
use crate::step_advance::{StepAdvanceCondition, StepAdvanceDecision, StepAdvanceInputs};

/// Defensive ceiling, in steps, on how many hops a single position fix may
/// advance through in the multi-step advance loop.
///
/// The loop's real terminators are the advance condition itself (a `Remain`
/// verdict ends the walk) and the trip's arrival on the final step, so the
/// bound is not what makes the loop safe — it exists to guarantee a bounded
/// response even against a pathological condition that advances on every
/// consult (any real fix is unboundedly faster than such a condition could
/// run out of steps). It is set far above any legitimate fix-to-step skip: a
/// single jump across a 1,000-step route stays well inside it.
pub const MAXIMUM_STEP_ADVANCES_PER_FIX: usize = 4096;

/// How few steps may remain — counted with the current step itself included —
/// while the trip consults its separately configured arrival step-advance
/// condition instead of its ordinary one.
///
/// The bound is 2, so the arrival condition governs the final step and the one
/// before it: arrival decisions deserve their own (typically narrower, more
/// eager or more pedestrian-calibrated) policy well before the maneuver
/// point. A trip that declares no arrival condition runs its ordinary condition
/// to the end (an explicit `None` in the `StartTrip` event).
pub const ARRIVAL_CONDITION_REMAINING_STEPS_BOUND: usize = 2;

/// Inputs the reducer accepts for one active trip.
pub enum NavigationEvent {
    /// Begin navigating the given route with the given advance strategy.
    ///
    /// The condition and the deviation detector are supplied per trip rather
    /// than through global configuration, so changing either policy is an
    /// explicit event in the history.
    StartTrip {
        /// The route to navigate.
        route: RouteData,
        /// The strategy deciding when to move between steps.
        step_advance_condition: Arc<dyn StepAdvanceCondition>,
        /// The strategy deciding when to move onto (and complete) the final
        /// steps, applied inside the arrival window
        /// ([`ARRIVAL_CONDITION_REMAINING_STEPS_BOUND`]); `None` keeps the
        /// ordinary condition for every step. Swapped with
        /// [`StepAdvanceCondition::fresh_instance`] on every advance hop, like
        /// the ordinary condition.
        arrival_step_advance_condition: Option<Arc<dyn StepAdvanceCondition>>,
        /// The policy deciding how the trip's course is reported
        /// (see [`crate::course_filtering`]).
        course_filtering: CourseFiltering,
        /// The strategy deciding when the trip is judged off route.
        deviation_detector: Arc<dyn DeviationDetector>,
    },
    /// Replace the route of the active trip (a reroute).
    ///
    /// This event is the caller's answer to a detected deviation — or a
    /// proactive route swap. The `remote-first, offline-fallback` policy (ask
    /// a remote router for a replacement route, fall back to an offline
    /// graph) decides *where* the replacement comes from and lives at the
    /// integrator/event-source layer; the reducer only applies it, staying
    /// pure and policy-free.
    ///
    /// # Anchoring contract
    ///
    /// [`Self::RerouteProvided`] rebuilds the session exactly as
    /// [`Self::StartTrip`] does, so the replacement route must be authored to
    /// *begin at the trip's current snapped coordinate* — which is where a
    /// reroute router anchors its request anyway. The reducer performs no
    /// trimming or re-ghosting of the new route's geometry; if the anchoring
    /// contract is violated, the next graded fix will be judged against the
    /// remaining route line by the deviation detector (and would flag a
    /// deviation), so a mis-authored reroute is at worst flagged, never
    /// silently mis-projected.
    ///
    /// The replacement advance condition must be fresh from the caller's side;
    /// the reducer additionally rebuilds it via
    /// [`StepAdvanceCondition::fresh_instance`] as a defensive reset so a
    /// mid-schedule instance carried over from the old route cannot corrupt
    /// the new step sequence (the double-advance contract).
    RerouteProvided {
        /// The replacement route, anchored at the current snapped coordinate.
        route: RouteData,
        /// The fresh strategy deciding when to move between steps of the new
        /// route.
        step_advance_condition: Arc<dyn StepAdvanceCondition>,
    },
    /// A new position fix was received while a trip is active.
    LocationUpdated(
        /// The fix to judge, snap and evaluate.
        NavigationLocationUpdate,
    ),
}

/// An active trip: the plain-data state plus the live decision instances.
///
/// This pairing (rather than embedding the conditions in the state) is what
/// keeps [`TripNavigationState`] serializable end to end. A session is not
/// meant to be shared between threads while it is reduced; pass it by value
/// from one event to the next.
#[derive(Clone)]
pub struct NavigationSession {
    /// The plain-data record consumers and UI read.
    pub state: TripNavigationState,
    /// The condition consulted on the next location update.
    pub step_advance_condition: Arc<dyn StepAdvanceCondition>,
    /// The separately configured arrival-window condition, `None` when the
    /// trip runs its ordinary condition to the end; consulted (instead of
    /// [`Self::step_advance_condition`]) while the remaining step count is
    /// within [`ARRIVAL_CONDITION_REMAINING_STEPS_BOUND`].
    pub arrival_step_advance_condition: Option<Arc<dyn StepAdvanceCondition>>,
    /// The trip's course-filtering policy (see [`crate::course_filtering`]).
    pub course_filtering: CourseFiltering,
    /// The detector consulted on the next location update.
    pub deviation_detector: Arc<dyn DeviationDetector>,
}

impl core::fmt::Debug for NavigationSession {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("NavigationSession")
            .field("state", &self.state)
            .field("step_advance_condition", &"Arc<dyn StepAdvanceCondition>")
            .field(
                "arrival_step_advance_condition",
                &"Option<Arc<dyn StepAdvanceCondition>>",
            )
            .field("course_filtering", &self.course_filtering)
            .field("deviation_detector", &"Arc<dyn DeviationDetector>")
            .finish()
    }
}

/// Reduces one event onto the previous session.
///
/// `None` means no trip is active: `LocationUpdated` and `RerouteProvided`
/// before any `StartTrip` are dropped, and `StartTrip` begins a session.
/// `LocationUpdated` and `RerouteProvided` on an arrived trip leave the
/// session untouched.
///
/// # Parameters
/// - `session` — the previous session, or `None` before the trip starts.
/// - `event` — the event to apply.
///
/// # Returns
/// The next session, or `None` when the event applies to no active trip.
///
/// # Panics
/// The reducer itself does not panic on any input, but a user-supplied
/// [`StepAdvanceCondition`] or [`DeviationDetector`] that panics propagates
/// the panic to the caller. The reducer is pure and synchronous: it performs
/// no I/O, reads no clock and spawns no thread.
pub fn reduce_navigation_event(
    session: Option<NavigationSession>,
    event: NavigationEvent,
) -> Option<NavigationSession> {
    match event {
        NavigationEvent::StartTrip {
            route,
            step_advance_condition,
            arrival_step_advance_condition,
            course_filtering,
            deviation_detector,
        } => Some(NavigationSession {
            state: build_trip_state(route),
            // The route has no traversed step yet: both condition instances
            // and the course policy install as declared (the reducer rebuilds
            // the *arrival* instance fresh on every advance hop, exactly as
            // the ordinary one).
            step_advance_condition,
            arrival_step_advance_condition,
            course_filtering,
            deviation_detector,
        }),
        NavigationEvent::RerouteProvided {
            route,
            step_advance_condition,
        } => {
            let mut session = session?;
            // An arrived trip ignores reroutes: the destination was reached,
            // and a replacement route cannot unreach it.
            if session.state.phase != NavigationPhase::Arrived {
                session = apply_reroute(session, route, step_advance_condition);
            }
            Some(session)
        }
        NavigationEvent::LocationUpdated(update) => {
            let mut session = session?;
            // An arrived trip keeps its session untouched; navigating and
            // off-route trips consume fixes (consuming a fix is the only way
            // an off-route trip recovers).
            if session.state.phase != NavigationPhase::Arrived {
                session = process_location_update(session, update);
            }
            Some(session)
        }
    }
}

/// Builds the initial state for a trip, before any location fix.
///
/// Shared by [`NavigationEvent::StartTrip`] and
/// [`NavigationEvent::RerouteProvided`]: both rebuild the trip's route
/// projection from scratch.
fn build_trip_state(route: RouteData) -> TripNavigationState {
    let step_positions = TripNavigationState::build_step_positions(&route);
    // A route without traversable steps has nothing to navigate; represent
    // that as an already-arrived trip rather than wedging in `Navigating`.
    let phase = if step_positions.is_empty() {
        NavigationPhase::Arrived
    } else {
        NavigationPhase::Navigating
    };
    let mut state = TripNavigationState {
        phase,
        route,
        step_positions,
        current_step_position: 0,
        progress: TripProgress::default(),
        deviation: None,
        snapped_coordinate: None,
        course_heading_degrees: None,
    };
    state.progress = compute_progress(&state);
    state
}

/// Rebuilds the session for the same trip from the replacement route onward.
fn apply_reroute(
    mut session: NavigationSession,
    route: RouteData,
    step_advance_condition: Arc<dyn StepAdvanceCondition>,
) -> NavigationSession {
    session.state = build_trip_state(route);
    // Defensive reset: the caller may hand back a condition instance that was
    // mid-schedule on the old route (for example the same `Arc` still held in
    // its own state); a mid-schedule instance would corrupt the new step
    // sequence (the double-advance contract).
    session.step_advance_condition = step_advance_condition.fresh_instance();
    // The arrival-window condition is a trip configuration: it persists
    // across the route swap, rebuilt fresh for the new step sequence under
    // the same contract as the ordinary condition and the deviation detector.
    if let Some(arrival_step_advance_condition) = &session.arrival_step_advance_condition {
        session.arrival_step_advance_condition =
            Some(arrival_step_advance_condition.fresh_instance());
    }
    // The course-filtering policy likewise persists (it is a trip-level
    // policy, not a per-route one).
    // The deviation detector's configuration persists across the route swap,
    // but its instance is rebuilt so a future stateful detector's tracking
    // restarts with the new route context (same contract as the condition
    // swap on advance).
    session.deviation_detector = session.deviation_detector.fresh_instance();
    session
}

/// Applies one fix to a navigating session: judge deviation, consult, then act.
fn process_location_update(
    mut session: NavigationSession,
    update: NavigationLocationUpdate,
) -> NavigationSession {
    // Deviation judgment comes first, and only for fixes carrying an accuracy
    // report: one threshold rule cannot conservatively decide both raising
    // and clearing a deviation, so an ungraded fix can do neither (it still
    // snaps and progresses when the trip is not already off route, as it did
    // before deviation detection landed).
    let is_fix_accuracy_graded = update.horizontal_accuracy_meters.is_some();
    if is_fix_accuracy_graded {
        let remaining_route_line = remaining_route_line(&session.state);
        let distance_from_route_meters =
            snap_coordinate_to_polyline(&update.coordinate, &remaining_route_line).map(
                |remaining_route_line_snap| remaining_route_line_snap.distance_to_polyline_meters,
            );
        // The two-grade judgment's second measurement: the current step's own
        // polyline, where the remaining route line's closest projection may
        // not sit (a future-step projection is the "on route, wrong step"
        // grade's shape).
        let current_step = session.state.current_step();
        let distance_from_current_step_meters = current_step.and_then(|current_step| {
            snap_coordinate_to_polyline(&update.coordinate, &current_step.shape)
                .map(|current_step_snap| current_step_snap.distance_to_polyline_meters)
        });
        let detection_inputs = DeviationDetectionInputs {
            coordinate: &update.coordinate,
            horizontal_accuracy_meters: update.horizontal_accuracy_meters,
            distance_from_route_meters,
            distance_from_current_step_meters,
        };
        match session
            .deviation_detector
            .deviation_status(&detection_inputs, &session.state)
        {
            // Freeze: the snapped coordinate and progress stay at the last
            // on-route values, the step does not advance, and the condition
            // is not consulted (a deviating fix must not drive route
            // progress). Only the phase and the recorded measurement change.
            DeviationStatus::OffRoute {
                deviation_from_route_meters,
            } => {
                session.state.phase = NavigationPhase::OffRoute;
                session.state.deviation = Some(DeviationInfo {
                    grade: DeviationGrade::FullyOffRoute,
                    deviation_from_route_meters,
                });
                return session;
            }
            // On route, wrong step: the trip never freezes on this grade —
            // record it and process the fix normally below (a frozen trip the
            // grade recovers lifts the freeze here).
            DeviationStatus::OffStepOnRoute {
                deviation_from_route_meters,
            } => {
                session.state.deviation = Some(DeviationInfo {
                    grade: DeviationGrade::OffStepOnRoute,
                    deviation_from_route_meters,
                });
                session.state.phase = NavigationPhase::Navigating;
            }
            // Fully on route: an active full deviation lifts (recovery), a
            // lingering wrong-step grade clears.
            DeviationStatus::OnRoute => {
                session.state.deviation = None;
                session.state.phase = NavigationPhase::Navigating;
            }
        }
    } else if session.state.phase == NavigationPhase::OffRoute {
        // Reaching this point on an off-route trip means an ungraded fix: it
        // can neither raise nor clear the judgment, so the deviation holds
        // and the session stays frozen. (A graded fix within the allowance
        // — or graded onto a future step — has already lifted the freeze
        // above.)
        return session;
    }

    // The multi-step advance loop (the reference's speed-run semantics): the
    // same fix is re-consulted — with freshly computed inputs — after every
    // single-hop advance, until the condition says to remain or the trip
    // arrives. Iterative, never recursive, and bounded by
    // `MAXIMUM_STEP_ADVANCES_PER_FIX` as a defensive ceiling.
    let mut applied_advances = 0_usize;
    loop {
        let decision = {
            let state = &session.state;
            // The caller guarantees the phase is `Navigating` with a traversal
            // built from the route, so the current step always resolves.
            #[allow(clippy::expect_used)]
            // internal invariant, upheld by the caller contract (documented above)
            let current_step = state
                .current_step()
                .expect("navigating session must always have a current step");
            let snap = snap_coordinate_to_polyline(&update.coordinate, &current_step.shape);
            // Straight-line distance from the raw fix to the maneuver point,
            // mirroring Ferrostar's eager condition which measures the raw
            // location, while progress (below) measures the snapped position.
            let distance_to_end_of_step_meters =
                current_step.shape.last().map(|terminal_coordinate| {
                    update
                        .coordinate
                        .haversine_distance_meters(terminal_coordinate)
                });
            let distance_from_step_meters = snap
                .as_ref()
                .map(|snapped| snapped.distance_to_polyline_meters);
            let inputs = StepAdvanceInputs {
                coordinate: &update.coordinate,
                horizontal_accuracy_meters: update.horizontal_accuracy_meters,
                current_step,
                next_step: state.step_at(state.current_step_position + 1),
                snapped_coordinate: snap.as_ref().map(|snapped| &snapped.snapped_coordinate),
                distance_to_end_of_step_meters,
                distance_from_step_meters,
            };
            // The arrival-aware condition selection: inside the arrival
            // window the separately configured arrival condition decides the
            // hop (falling back to the ordinary condition without one).
            let consulted_condition = if remaining_steps_within_arrival_window(state) {
                session
                    .arrival_step_advance_condition
                    .as_ref()
                    .cloned()
                    .unwrap_or_else(|| Arc::clone(&session.step_advance_condition))
            } else {
                Arc::clone(&session.step_advance_condition)
            };
            consulted_condition.should_advance_step(&inputs, state)
        };

        if decision != StepAdvanceDecision::Advance {
            break;
        }
        if session.state.is_on_final_step() {
            session.state.phase = NavigationPhase::Arrived;
            break;
        }
        session.state.current_step_position += 1;
        // Each hop swaps in the fresh condition instance exactly as the
        // single-advance slice did: a stale instance's internal progress can
        // never leak from one step into the next (the double-advance
        // contract). The arrival-window condition, when configured, is
        // swapped under the same contract.
        session.step_advance_condition = session.step_advance_condition.fresh_instance();
        if let Some(arrival_step_advance_condition) = &session.arrival_step_advance_condition {
            session.arrival_step_advance_condition =
                Some(arrival_step_advance_condition.fresh_instance());
        }
        applied_advances += 1;
        if applied_advances >= MAXIMUM_STEP_ADVANCES_PER_FIX {
            break;
        }
    }
    session.state = apply_location_to_state(session.state, &update, session.course_filtering);
    session
}

/// Whether the trip's remaining steps — the current one itself included — sit
/// inside the arrival window: the separately configured arrival condition
/// governs these steps.
fn remaining_steps_within_arrival_window(state: &TripNavigationState) -> bool {
    state.step_positions.len() - state.current_step_position
        <= ARRIVAL_CONDITION_REMAINING_STEPS_BOUND
}

/// The polyline of the route remaining from the current step onward: the
/// current step's shape plus every following step's shape, in traversal order.
///
/// Deviation is measured against this line rather than only the current step:
/// a lagging step — eager advance semantics, pauses at intersections — is then
/// never mistaken for a deviation. Consecutive steps may repeat their shared boundary coordinate,
/// which degenerates to a zero-length segment the snapping projection already
/// treats as a point.
fn remaining_route_line(state: &TripNavigationState) -> Vec<GeographicCoordinate> {
    let mut remaining_route_line = Vec::new();
    for step_position in state
        .step_positions
        .iter()
        .skip(state.current_step_position)
    {
        if let Some(step) = state.step_for_position(step_position) {
            remaining_route_line.extend_from_slice(&step.shape);
        }
    }
    remaining_route_line
}

/// Snaps the fix onto the current step and recomputes progress from it.
///
/// When the current step's shape carries no geometry, snapping is impossible;
/// the previous snapped position is retained so the progress projection
/// degrades continuously instead of flickering. The trip's effective course
/// is stamped from the same decision: the fix's snap under the trip's
/// course-filtering policy ([`effective_course_heading_degrees`]).
fn apply_location_to_state(
    mut state: TripNavigationState,
    update: &NavigationLocationUpdate,
    course_filtering: CourseFiltering,
) -> TripNavigationState {
    let snapped = state.current_step().and_then(|current_step| {
        snap_coordinate_to_polyline(&update.coordinate, &current_step.shape)
    });
    state.snapped_coordinate = snapped
        .as_ref()
        .map(|snapped| snapped.snapped_coordinate)
        .or(state.snapped_coordinate);
    state.course_heading_degrees = effective_course_heading_degrees(
        course_filtering,
        update.course_heading_degrees,
        snapped.as_ref().and_then(|snapped| snapped.bearing_degrees),
        state.course_heading_degrees,
    );
    state.progress = compute_progress(&state);
    state
}

/// Recomputes [`TripProgress`] for the state's current step.
///
/// `distance_to_next_maneuver_meters` measures the snapped position against
/// the step's terminal coordinate; without a snap it
/// falls back to the provider-declared step distance. The remaining total
/// adds the provider-declared distances and durations of the steps after the
/// current one, and the current step's remaining duration is scaled linearly
/// along the step — an approximation, since providers do not distribute speed
/// uniformly within a step.
fn compute_progress(state: &TripNavigationState) -> TripProgress {
    if state.phase == NavigationPhase::Arrived {
        return TripProgress::default();
    }
    let Some(current_step) = state.current_step() else {
        return TripProgress::default();
    };

    let distance_to_next_maneuver_meters = match state.snapped_coordinate {
        Some(snapped_coordinate) => current_step
            .shape
            .last()
            .map(|terminal_coordinate| {
                snapped_coordinate.haversine_distance_meters(terminal_coordinate)
            })
            .unwrap_or(current_step.distance_meters),
        None => current_step.distance_meters,
    };
    let traveled_fraction = if current_step.distance_meters > 0.0 {
        // Structural fraction of the step left to walk; a snap is inside the
        // step geometry, and the clamp keeps noisy declared totals honest.
        (distance_to_next_maneuver_meters / current_step.distance_meters).clamp(0.0, 1.0)
    } else {
        // A zero-distance step is instantaneously complete.
        1.0
    };
    let duration_to_next_maneuver_seconds = current_step.duration_seconds * traveled_fraction;

    let distance_remaining_meters = distance_to_next_maneuver_meters
        + following_declared_distance_meters(state, state.current_step_position);
    let duration_remaining_seconds = duration_to_next_maneuver_seconds
        + following_declared_duration_seconds(state, state.current_step_position);

    TripProgress {
        distance_to_next_maneuver_meters,
        distance_remaining_meters,
        duration_remaining_seconds,
    }
}

/// Sums the provider-declared distances of the steps after `from_position`.
fn following_declared_distance_meters(state: &TripNavigationState, from_position: usize) -> f64 {
    state
        .step_positions
        .iter()
        .enumerate()
        .filter(|(position, _)| *position > from_position)
        .filter_map(|(_, step_position)| state.step_for_position(step_position))
        .map(|step| step.distance_meters)
        .sum()
}

/// Sums the provider-declared durations of the steps after `from_position`.
fn following_declared_duration_seconds(state: &TripNavigationState, from_position: usize) -> f64 {
    state
        .step_positions
        .iter()
        .enumerate()
        .filter(|(position, _)| *position > from_position)
        .filter_map(|(_, step_position)| state.step_for_position(step_position))
        .map(|step| step.duration_seconds)
        .sum()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use proptest::prelude::*;

    use geospatial_types::{BoundingBox, ManeuverType, RouteData, RouteLeg, RouteStep};

    use super::{NavigationEvent, NavigationSession, reduce_navigation_event};
    use crate::course_filtering::CourseFiltering;
    use crate::deviation::{
        DEFAULT_DEVIATION_DISTANCE_THRESHOLD_METERS, DeviationDetector, DistanceDeviationDetector,
    };
    use crate::location::NavigationLocationUpdate;
    use crate::state::{DeviationGrade, NavigationPhase};
    use crate::step_advance::{
        DEFAULT_MINIMUM_HORIZONTAL_ACCURACY_METERS, DistanceFromStepCondition,
        DistanceToEndOfStepCondition, ManualStepAdvanceCondition, OrStepAdvanceCondition,
    };
    use crate::test_support::{
        NeverDeviatingDeviationDetector, OneShotAdvanceCondition, TEST_ACCURACY_METERS,
        UpdatesBeforeAdvanceCondition, offset_coordinate,
    };

    /// Length, in meters, of each synthetic step under test.
    const TEST_STEP_LENGTH_METERS: f64 = 100.0;
    /// Number of steps in the synthetic three-step routes.
    const TEST_STEP_COUNT: usize = 3;
    /// Declared duration, in seconds, of each synthetic step.
    const TEST_STEP_DURATION_SECONDS: f64 = 10.0;
    /// Spacing, in meters, between consecutive walker updates.
    const WALKER_UPDATE_SPACING_METERS: f64 = 10.0;
    /// Total length, in meters, of the synthetic route under test.
    const TEST_ROUTE_TOTAL_LENGTH_METERS: f64 = TEST_STEP_LENGTH_METERS * TEST_STEP_COUNT as f64;
    /// Monotonicity tolerance, in meters, accommodating haversine rounding.
    const MONOTONICITY_TOLERANCE_METERS: f64 = 1e-6;
    /// Structural tolerance for distances that must be (near) zero.
    const NEAR_ZERO_TOLERANCE_METERS: f64 = 0.001;
    /// Lateral excursion, in meters, used to leave the synthetic route.
    const TEST_OFF_ROUTE_EXCURSION_METERS: f64 = 100.0;
    /// Length, in meters, of the single-step replacement route for reroutes.
    const TEST_REROUTE_STEP_LENGTH_METERS: f64 = 150.0;

    /// Raw course, in degrees, the course-filtering fixture reports: a
    /// southerly reading that materially disagrees with the eastward test
    /// routes' `TEST_EASTWARD_ROUTE_BEARING_DEGREES`.
    const TEST_RAW_COURSE_DEGREES: f64 = 200.0;
    /// Bearing, in degrees, of the straight eastward test routes.
    const TEST_EASTWARD_ROUTE_BEARING_DEGREES: f64 = 90.0;
    /// Effective-course tolerance, in degrees, for the course-filtering
    /// tests (the snap's planar bearing reconstruction rounding at test
    /// scale).
    const TEST_COURSE_TOLERANCE_DEGREES: f64 = 0.001;
    /// Observed-fix count the arrival-window counting condition requires
    /// before each of its advances (two consults per step: one remaining,
    /// one advancing — the pin that proves the fresh-instance swap inside
    /// the window).
    const TEST_ARRIVAL_OBSERVATIONS_BEFORE_ADVANCE: usize = 1;
    /// The first step position inside the arrival window of the
    /// [`straight_three_step_route`] (the reference counts remaining steps
    /// with the current one included, so the window holds the last two).
    const TEST_ARRIVAL_WINDOW_FIRST_STEP_POSITION: usize = TEST_STEP_COUNT - 2;

    /// Step count of the stack-safety replay route (the reference stack-safety
    /// suite's round number).
    const TEST_STACK_SAFETY_STEP_COUNT: usize = 1000;
    /// Length, in meters, of each stack-safety replay step: short steps make
    /// one huge jump cross very many of them.
    const TEST_STACK_SAFETY_STEP_LENGTH_METERS: f64 = 10.0;
    /// Along-route offset, in meters, the stack-safety replay's single huge
    /// jump lands at: the boundary between steps
    /// [`TEST_STACK_SAFETY_JUMP_TARGET_STEP_POSITION`] and the following one.
    const TEST_STACK_SAFETY_JUMP_TARGET_ALONG_METERS: f64 =
        TEST_STACK_SAFETY_STEP_LENGTH_METERS * 801.0;
    /// Flattened position of the step the huge jump's walker must land on.
    const TEST_STACK_SAFETY_JUMP_TARGET_STEP_POSITION: usize = 800;
    /// Delayed-advance threshold, in meters, of the stack-safety replay's
    /// step-advance condition: tighter than one step's length so the loop's
    /// final hop settles on the step the jumped fix itself sits on.
    const TEST_STACK_SAFETY_DELAYED_THRESHOLD_METERS: f64 = 5.0;
    /// Stack size, in bytes, of the dedicated thread the stack-safety replay
    /// runs on (the reference suite's small-stack convention: mobile stacks
    /// are only a fraction of a desktop thread's default).
    const TEST_STACK_SAFETY_THREAD_STACK_SIZE_BYTES: usize = 256 * 1024;

    /// Lateral offset, in meters, of the wrong-step fixture's fix: on the
    /// fixture's second (northward) street, beyond the deviation allowance
    /// from the first street.
    const TEST_WRONG_STEP_LATERAL_METERS: f64 = 30.0;

    /// Length, in meters, of the two-street fixture's first (eastward) step.
    const TEST_TWO_STREET_EAST_METERS: f64 = 200.0;
    /// Length, in meters, of the two-street fixture's second (northward)
    /// step.
    const TEST_TWO_STREET_NORTH_METERS: f64 = 300.0;

    /// The default detector: every fixture trip in this module deviates
    /// exactly as production defaults would.
    fn default_deviation_detector() -> Arc<dyn DeviationDetector> {
        Arc::new(DistanceDeviationDetector::with_default_threshold())
    }

    /// The named constant of the fixture walker's effective deviation
    /// allowance: the default threshold widened by the standard test accuracy.
    fn tester_effective_allowance_meters() -> f64 {
        DEFAULT_DEVIATION_DISTANCE_THRESHOLD_METERS + TEST_ACCURACY_METERS
    }

    /// Builds an exact route: three collinear eastward steps of
    /// [`TEST_STEP_LENGTH_METERS`], each with three shape points, whose
    /// declared distances match their geometric lengths.
    fn straight_three_step_route() -> RouteData {
        let mut route_leg_steps = Vec::new();
        for step_number in 0..TEST_STEP_COUNT {
            let start_offset = step_number as f64 * TEST_STEP_LENGTH_METERS;
            route_leg_steps.push(RouteStep {
                distance_meters: TEST_STEP_LENGTH_METERS,
                duration_seconds: TEST_STEP_DURATION_SECONDS,
                maneuver_type: if step_number == 0 {
                    ManeuverType::Departure
                } else if step_number == TEST_STEP_COUNT - 1 {
                    ManeuverType::Arrival
                } else {
                    ManeuverType::Continue
                },
                instruction_display_text: format!("Step {step_number} of {TEST_STEP_COUNT}"),
                instruction_speech_text: format!("Step {step_number} of {TEST_STEP_COUNT}"),
                shape: vec![
                    offset_coordinate(start_offset, 0.0),
                    offset_coordinate(start_offset + TEST_STEP_LENGTH_METERS / 2.0, 0.0),
                    offset_coordinate(start_offset + TEST_STEP_LENGTH_METERS, 0.0),
                ],
                lane_guidances: Vec::new(),
            });
        }
        let route_leg = RouteLeg {
            distance_meters: TEST_ROUTE_TOTAL_LENGTH_METERS,
            duration_seconds: TEST_STEP_DURATION_SECONDS * TEST_STEP_COUNT as f64,
            steps: route_leg_steps,
        };
        RouteData {
            distance_meters: TEST_ROUTE_TOTAL_LENGTH_METERS,
            duration_seconds: TEST_STEP_DURATION_SECONDS * TEST_STEP_COUNT as f64,
            bounding_box: BoundingBox {
                south_west: offset_coordinate(0.0, 0.0),
                north_east: offset_coordinate(TEST_ROUTE_TOTAL_LENGTH_METERS, 0.0),
            },
            geometry: Vec::new(),
            legs: vec![route_leg],
        }
    }

    /// Walks the synthetic route with evenly spaced fixes and returns the
    /// session after each one, in order.
    fn walk_route(mut session: NavigationSession) -> Vec<NavigationSession> {
        let mut update_offsets = Vec::new();
        let total_updates_for_walker =
            (TEST_ROUTE_TOTAL_LENGTH_METERS / WALKER_UPDATE_SPACING_METERS) as usize;
        for update_number in 1..=total_updates_for_walker {
            update_offsets.push(update_number as f64 * WALKER_UPDATE_SPACING_METERS);
        }
        update_offsets.push(TEST_ROUTE_TOTAL_LENGTH_METERS);

        let mut walked_sessions = Vec::new();
        for update_offset in update_offsets {
            let update = NavigationLocationUpdate {
                coordinate: offset_coordinate(update_offset, 0.0),
                horizontal_accuracy_meters: Some(TEST_ACCURACY_METERS),
                course_heading_degrees: None,
                timestamp_seconds: None,
            };
            session =
                reduce_navigation_event(Some(session), NavigationEvent::LocationUpdated(update))
                    .expect("walked session must stay alive");
            walked_sessions.push(session.clone());
        }
        walked_sessions
    }

    /// Builds a graded fix on the route line at `east_meters`.
    fn route_fix(east_meters: f64) -> NavigationLocationUpdate {
        NavigationLocationUpdate {
            coordinate: offset_coordinate(east_meters, 0.0),
            horizontal_accuracy_meters: Some(TEST_ACCURACY_METERS),
            course_heading_degrees: None,
            timestamp_seconds: None,
        }
    }

    /// A two-step route with an L-shaped geometry: an eastward first street
    /// and a northward second street, so a fix on the second street is
    /// far beyond the deviation allowance from the first step's polyline
    /// while sitting exactly on the remaining route line — the wrong-step
    /// fixture's shape.
    fn two_street_route() -> RouteData {
        let first_street_steps = vec![RouteStep {
            distance_meters: TEST_TWO_STREET_EAST_METERS,
            duration_seconds: TEST_STEP_DURATION_SECONDS,
            maneuver_type: ManeuverType::Departure,
            instruction_display_text: "East street".to_string(),
            instruction_speech_text: "East street".to_string(),
            shape: vec![
                offset_coordinate(0.0, 0.0),
                offset_coordinate(TEST_TWO_STREET_EAST_METERS / 2.0, 0.0),
                offset_coordinate(TEST_TWO_STREET_EAST_METERS, 0.0),
            ],
            lane_guidances: Vec::new(),
        }];
        let second_street_steps = vec![RouteStep {
            distance_meters: TEST_TWO_STREET_NORTH_METERS,
            duration_seconds: TEST_STEP_DURATION_SECONDS * 3.0,
            maneuver_type: ManeuverType::Arrival,
            instruction_display_text: "North street".to_string(),
            instruction_speech_text: "North street".to_string(),
            shape: vec![
                offset_coordinate(TEST_TWO_STREET_EAST_METERS, 0.0),
                offset_coordinate(
                    TEST_TWO_STREET_EAST_METERS,
                    TEST_TWO_STREET_NORTH_METERS / 2.0,
                ),
                offset_coordinate(TEST_TWO_STREET_EAST_METERS, TEST_TWO_STREET_NORTH_METERS),
            ],
            lane_guidances: Vec::new(),
        }];
        let first_street_leg = RouteLeg {
            distance_meters: TEST_TWO_STREET_EAST_METERS,
            duration_seconds: TEST_STEP_DURATION_SECONDS,
            steps: first_street_steps,
        };
        let second_street_leg = RouteLeg {
            distance_meters: TEST_TWO_STREET_NORTH_METERS,
            duration_seconds: TEST_STEP_DURATION_SECONDS * 3.0,
            steps: second_street_steps,
        };
        RouteData {
            distance_meters: TEST_TWO_STREET_EAST_METERS + TEST_TWO_STREET_NORTH_METERS,
            duration_seconds: TEST_STEP_DURATION_SECONDS * 4.0,
            bounding_box: BoundingBox {
                south_west: offset_coordinate(0.0, 0.0),
                north_east: offset_coordinate(
                    TEST_TWO_STREET_EAST_METERS,
                    TEST_TWO_STREET_NORTH_METERS,
                ),
            },
            geometry: Vec::new(),
            legs: vec![first_street_leg, second_street_leg],
        }
    }

    /// A generic fix on the wrong-step fixture's geometry with its own
    /// coordinate: [`wrong_step_fix`] with the east offset stated.
    fn two_street_walker_fix(east_meters: f64, north_meters: f64) -> NavigationLocationUpdate {
        NavigationLocationUpdate {
            coordinate: offset_coordinate(east_meters, north_meters),
            horizontal_accuracy_meters: Some(TEST_ACCURACY_METERS),
            course_heading_degrees: None,
            timestamp_seconds: None,
        }
    }

    /// The wrong-step fixture's fix: on the second street's first segment
    /// ([`TEST_WRONG_STEP_LATERAL_METERS`] north of the first street's
    /// terminal) — beyond the deviation allowance from the first step's
    /// polyline, exactly on the remaining route line.
    fn wrong_step_fix() -> NavigationLocationUpdate {
        two_street_walker_fix(TEST_TWO_STREET_EAST_METERS, TEST_WRONG_STEP_LATERAL_METERS)
    }

    /// Builds the 1,000-step collinear route of the stack-safety replay: each
    /// step [`TEST_STACK_SAFETY_STEP_LENGTH_METERS`] long, three shape points
    /// per step as anywhere else.
    fn stack_safety_route() -> RouteData {
        let total_length_meters =
            TEST_STACK_SAFETY_STEP_LENGTH_METERS * TEST_STACK_SAFETY_STEP_COUNT as f64;
        let route_leg_steps = (0..TEST_STACK_SAFETY_STEP_COUNT)
            .map(|step_number| {
                let start_offset = step_number as f64 * TEST_STACK_SAFETY_STEP_LENGTH_METERS;
                let terminal_offset = start_offset + TEST_STACK_SAFETY_STEP_LENGTH_METERS;
                let maneuver_type = if step_number == 0 {
                    ManeuverType::Departure
                } else if step_number == TEST_STACK_SAFETY_STEP_COUNT - 1 {
                    ManeuverType::Arrival
                } else {
                    ManeuverType::Continue
                };
                RouteStep {
                    distance_meters: TEST_STACK_SAFETY_STEP_LENGTH_METERS,
                    duration_seconds: TEST_STEP_DURATION_SECONDS,
                    maneuver_type,
                    instruction_display_text: format!("Stack-safety step {step_number}"),
                    instruction_speech_text: format!("Stack-safety step {step_number}"),
                    shape: vec![
                        offset_coordinate(start_offset, 0.0),
                        offset_coordinate(
                            start_offset + TEST_STACK_SAFETY_STEP_LENGTH_METERS / 2.0,
                            0.0,
                        ),
                        offset_coordinate(terminal_offset, 0.0),
                    ],
                    lane_guidances: Vec::new(),
                }
            })
            .collect::<Vec<_>>();
        let route_leg = RouteLeg {
            distance_meters: total_length_meters,
            duration_seconds: TEST_STEP_DURATION_SECONDS * TEST_STACK_SAFETY_STEP_COUNT as f64,
            steps: route_leg_steps,
        };
        RouteData {
            distance_meters: total_length_meters,
            duration_seconds: TEST_STEP_DURATION_SECONDS * TEST_STACK_SAFETY_STEP_COUNT as f64,
            bounding_box: BoundingBox {
                south_west: offset_coordinate(0.0, 0.0),
                north_east: offset_coordinate(total_length_meters, 0.0),
            },
            geometry: Vec::new(),
            legs: vec![route_leg],
        }
    }

    /// Builds a graded fix displaced `north_meters` north of the route line at
    /// `east_meters`: off route whenever the excursion exceeds the effective
    /// deviation allowance.
    fn off_route_fix(east_meters: f64, north_meters: f64) -> NavigationLocationUpdate {
        NavigationLocationUpdate {
            coordinate: offset_coordinate(east_meters, north_meters),
            horizontal_accuracy_meters: Some(TEST_ACCURACY_METERS),
            course_heading_degrees: None,
            timestamp_seconds: None,
        }
    }

    /// The same fix with its accuracy report removed.
    fn ungraded_fix(fix: &NavigationLocationUpdate) -> NavigationLocationUpdate {
        NavigationLocationUpdate {
            horizontal_accuracy_meters: None,
            ..*fix
        }
    }

    /// Starts a manual-advance trip on [`straight_three_step_route`] with the
    /// default deviation detector.
    fn start_manual_trip() -> NavigationSession {
        reduce_navigation_event(
            None,
            NavigationEvent::StartTrip {
                route: straight_three_step_route(),
                arrival_step_advance_condition: None,
                course_filtering: CourseFiltering::Raw,
                step_advance_condition: Arc::new(ManualStepAdvanceCondition),
                deviation_detector: default_deviation_detector(),
            },
        )
        .unwrap()
    }

    /// The east offset the reroute fixture's walker has reached: exactly two
    /// full steps of the synthetic route.
    const TEST_REROUTE_ANCHOR_OFFSET_METERS: f64 = TEST_STEP_LENGTH_METERS * 2.0;

    /// Builds a single-step replacement route that starts exactly at the
    /// reroute walker's position (two full steps in, per
    /// [`TEST_REROUTE_ANCHOR_OFFSET_METERS`]) and runs collinearly eastward:
    /// the reroute fixture honoring the reroute anchoring contract.
    fn single_step_reroute_route() -> RouteData {
        let origin_coordinate = offset_coordinate(TEST_REROUTE_ANCHOR_OFFSET_METERS, 0.0);
        let terminal_coordinate = offset_coordinate(
            TEST_REROUTE_ANCHOR_OFFSET_METERS + TEST_REROUTE_STEP_LENGTH_METERS,
            0.0,
        );
        let route_leg = RouteLeg {
            distance_meters: TEST_REROUTE_STEP_LENGTH_METERS,
            duration_seconds: TEST_STEP_DURATION_SECONDS,
            steps: vec![RouteStep {
                distance_meters: TEST_REROUTE_STEP_LENGTH_METERS,
                duration_seconds: TEST_STEP_DURATION_SECONDS,
                maneuver_type: ManeuverType::Arrival,
                instruction_display_text: "Rerouted step".to_string(),
                instruction_speech_text: "Rerouted step".to_string(),
                shape: vec![origin_coordinate, terminal_coordinate],
                lane_guidances: Vec::new(),
            }],
        };
        RouteData {
            distance_meters: TEST_REROUTE_STEP_LENGTH_METERS,
            duration_seconds: TEST_STEP_DURATION_SECONDS,
            bounding_box: BoundingBox {
                south_west: origin_coordinate,
                north_east: terminal_coordinate,
            },
            geometry: Vec::new(),
            legs: vec![route_leg],
        }
    }

    #[test]
    fn start_trip_reports_declared_progress_before_any_fix() {
        let session = start_manual_trip();
        let state = &session.state;
        assert_eq!(state.phase, NavigationPhase::Navigating);
        assert_eq!(state.current_step_position, 0);
        assert_eq!(state.step_positions.len(), TEST_STEP_COUNT);
        assert!(state.snapped_coordinate.is_none());
        assert!(
            (state.progress.distance_to_next_maneuver_meters - TEST_STEP_LENGTH_METERS).abs()
                < NEAR_ZERO_TOLERANCE_METERS
        );
        assert!(
            (state.progress.distance_remaining_meters - TEST_ROUTE_TOTAL_LENGTH_METERS).abs()
                < NEAR_ZERO_TOLERANCE_METERS
        );
        assert!(
            (state.progress.duration_remaining_seconds
                - TEST_STEP_DURATION_SECONDS * TEST_STEP_COUNT as f64)
                .abs()
                < NEAR_ZERO_TOLERANCE_METERS
        );
    }

    #[test]
    fn start_trip_without_traversable_steps_is_arrived() {
        let route = geospatial_types::RouteData {
            distance_meters: 0.0,
            duration_seconds: 0.0,
            bounding_box: BoundingBox {
                south_west: offset_coordinate(0.0, 0.0),
                north_east: offset_coordinate(0.0, 0.0),
            },
            geometry: Vec::new(),
            legs: Vec::new(),
        };
        let session = reduce_navigation_event(
            None,
            NavigationEvent::StartTrip {
                route,
                arrival_step_advance_condition: None,
                course_filtering: CourseFiltering::Raw,
                step_advance_condition: Arc::new(ManualStepAdvanceCondition),
                deviation_detector: default_deviation_detector(),
            },
        )
        .unwrap();
        assert_eq!(session.state.phase, NavigationPhase::Arrived);
    }

    #[test]
    fn location_update_before_any_trip_is_ignored() {
        let outcome = reduce_navigation_event(
            None,
            NavigationEvent::LocationUpdated(NavigationLocationUpdate::from_coordinate(
                offset_coordinate(0.0, 0.0),
            )),
        );
        assert!(outcome.is_none(), "no trip: the fix has nowhere to land");
    }

    #[test]
    fn walker_fires_maneuvers_in_order_and_arrives() {
        let session = reduce_navigation_event(
            None,
            NavigationEvent::StartTrip {
                route: straight_three_step_route(),
                arrival_step_advance_condition: None,
                course_filtering: CourseFiltering::Raw,
                step_advance_condition: Arc::new(
                    DistanceToEndOfStepCondition::with_ferrostar_defaults(),
                ),
                deviation_detector: default_deviation_detector(),
            },
        )
        .unwrap();
        let walked_sessions = walk_route(session);

        // The walker's first fix sits on step zero; the walk must visit every
        // step exactly once, in order, and finish arrived.
        let mut last_seen_position = 0;
        for walked_session in &walked_sessions {
            if walked_session.state.phase == NavigationPhase::Arrived {
                break;
            }
            assert!(walked_session.state.current_step_position >= last_seen_position);
            last_seen_position = walked_session.state.current_step_position;
        }
        assert_eq!(last_seen_position, TEST_STEP_COUNT - 1);
        let final_session = walked_sessions.last().unwrap();
        assert_eq!(final_session.state.phase, NavigationPhase::Arrived);
        assert!(
            final_session.state.progress.distance_remaining_meters < NEAR_ZERO_TOLERANCE_METERS
        );

        // Within each step the distance to that step's maneuver never grows;
        // across an advance boundary it legitimately resets to the next
        // step's full length, so only same-step neighbors are compared.
        for pair in walked_sessions.windows(2) {
            let (previous_session, next_session) = (&pair[0], &pair[1]);
            if previous_session.state.current_step_position
                == next_session.state.current_step_position
            {
                assert!(
                    next_session.state.progress.distance_to_next_maneuver_meters
                        <= previous_session
                            .state
                            .progress
                            .distance_to_next_maneuver_meters
                            + MONOTONICITY_TOLERANCE_METERS,
                    "distance to the maneuver must not grow within a step: {} -> {}",
                    previous_session
                        .state
                        .progress
                        .distance_to_next_maneuver_meters,
                    next_session.state.progress.distance_to_next_maneuver_meters
                );
            }
        }
        // …while the total remaining distance is monotonic over the whole walk.
        for pair in walked_sessions.windows(2) {
            let (previous_session, next_session) = (&pair[0], &pair[1]);
            assert!(
                next_session.state.progress.distance_remaining_meters
                    <= previous_session.state.progress.distance_remaining_meters
                        + MONOTONICITY_TOLERANCE_METERS,
                "remaining distance must be non-increasing: {} -> {}",
                previous_session.state.progress.distance_remaining_meters,
                next_session.state.progress.distance_remaining_meters
            );
        }
    }

    #[test]
    fn accuracy_gated_fix_does_not_advance() {
        let session = reduce_navigation_event(
            None,
            NavigationEvent::StartTrip {
                route: straight_three_step_route(),
                arrival_step_advance_condition: None,
                course_filtering: CourseFiltering::Raw,
                step_advance_condition: Arc::new(
                    DistanceToEndOfStepCondition::with_ferrostar_defaults(),
                ),
                deviation_detector: default_deviation_detector(),
            },
        )
        .unwrap();
        // 5 m before the first maneuver point, inside the eager threshold,
        // but with an accuracy envelope wider than the condition minimum.
        let inaccurate_update = NavigationLocationUpdate {
            coordinate: offset_coordinate(TEST_STEP_LENGTH_METERS - 5.0, 0.0),
            horizontal_accuracy_meters: Some(TEST_ROUTE_TOTAL_LENGTH_METERS),
            course_heading_degrees: None,
            timestamp_seconds: None,
        };
        let gated_session = reduce_navigation_event(
            Some(session.clone()),
            NavigationEvent::LocationUpdated(inaccurate_update),
        )
        .unwrap();
        assert_eq!(gated_session.state.current_step_position, 0);
        assert_eq!(gated_session.state.phase, NavigationPhase::Navigating);

        // The identical fix with an accurate envelope advances.
        let accurate_update = NavigationLocationUpdate {
            horizontal_accuracy_meters: Some(TEST_ACCURACY_METERS),
            ..inaccurate_update
        };
        let advanced_session = reduce_navigation_event(
            Some(gated_session),
            NavigationEvent::LocationUpdated(accurate_update),
        )
        .unwrap();
        assert_eq!(advanced_session.state.current_step_position, 1);
    }

    #[test]
    fn fix_without_any_accuracy_report_never_advances() {
        let session = reduce_navigation_event(
            None,
            NavigationEvent::StartTrip {
                route: straight_three_step_route(),
                arrival_step_advance_condition: None,
                course_filtering: CourseFiltering::Raw,
                step_advance_condition: Arc::new(
                    DistanceToEndOfStepCondition::with_ferrostar_defaults(),
                ),
                deviation_detector: default_deviation_detector(),
            },
        )
        .unwrap();
        let update_without_accuracy = NavigationLocationUpdate {
            coordinate: offset_coordinate(TEST_STEP_LENGTH_METERS - 5.0, 0.0),
            horizontal_accuracy_meters: None,
            course_heading_degrees: None,
            timestamp_seconds: None,
        };
        let updated_session = reduce_navigation_event(
            Some(session),
            NavigationEvent::LocationUpdated(update_without_accuracy),
        )
        .unwrap();
        assert_eq!(updated_session.state.current_step_position, 0);
    }

    #[test]
    fn manual_condition_never_advances_on_location_updates_alone() {
        let session = reduce_navigation_event(
            None,
            NavigationEvent::StartTrip {
                route: straight_three_step_route(),
                arrival_step_advance_condition: None,
                course_filtering: CourseFiltering::Raw,
                step_advance_condition: Arc::new(ManualStepAdvanceCondition),
                deviation_detector: default_deviation_detector(),
            },
        )
        .unwrap();
        let walked_sessions = walk_route(session);
        // The walker walks the entire route, yet the step never changes.
        for walked_session in &walked_sessions {
            assert_eq!(walked_session.state.current_step_position, 0);
            assert_eq!(walked_session.state.phase, NavigationPhase::Navigating);
        }
        // Even so, position and progress do track the walk; with the step
        // never advancing, later fixes clamp onto the current step's terminal.
        let final_session = walked_sessions.last().unwrap();
        let final_snap = final_session.state.snapped_coordinate.unwrap();
        let current_step_terminal = offset_coordinate(TEST_STEP_LENGTH_METERS, 0.0);
        assert!(
            final_snap.haversine_distance_meters(&current_step_terminal)
                < NEAR_ZERO_TOLERANCE_METERS,
            "manual navigation must still snap to the current step terminal"
        );
    }

    #[test]
    fn stateful_condition_resets_on_every_advance() {
        let session = reduce_navigation_event(
            None,
            NavigationEvent::StartTrip {
                route: straight_three_step_route(),
                arrival_step_advance_condition: None,
                course_filtering: CourseFiltering::Raw,
                step_advance_condition: Arc::new(UpdatesBeforeAdvanceCondition::requiring(3)),
                deviation_detector: Arc::new(NeverDeviatingDeviationDetector),
            },
        )
        .unwrap();
        let walked_sessions = walk_route(session);

        // The counter condition advances on the evaluation where it has
        // already observed `minimum` earlier evaluations. With the multi-step
        // advance loop, an advance's fresh instance is immediately consulted
        // with the same fix (and that observation counts on the fresh
        // instance's own counter), so the schedule compresses: the advances
        // land at updates 4, 7 and 10 — any stale condition instance (an
        // un-reset counter carried across a hop) would cascade further,
        // sooner.
        let expected_positions = [0usize, 0, 0, 1, 1, 1, 2, 2, 2, 2, 2, 2];
        for (walked_session, expected_position) in walked_sessions.iter().zip(expected_positions) {
            assert_eq!(
                walked_session.state.current_step_position, expected_position,
                "a stale condition instance would cascade through the steps"
            );
        }
        // The tail of the walk is the arrived session.
        assert!(
            walked_sessions[expected_positions.len()..]
                .iter()
                .all(|walked_session| walked_session.state.phase == NavigationPhase::Arrived)
        );
    }

    #[test]
    fn updates_after_arrival_are_ignored() {
        let session = reduce_navigation_event(
            None,
            NavigationEvent::StartTrip {
                route: straight_three_step_route(),
                arrival_step_advance_condition: None,
                course_filtering: CourseFiltering::Raw,
                step_advance_condition: Arc::new(
                    DistanceToEndOfStepCondition::with_ferrostar_defaults(),
                ),
                deviation_detector: default_deviation_detector(),
            },
        )
        .unwrap();
        let arrived_session = walk_route(session).pop().unwrap();
        assert_eq!(arrived_session.state.phase, NavigationPhase::Arrived);
        let stray_update = NavigationLocationUpdate {
            coordinate: offset_coordinate(-1_000_000.0, -1_000_000.0),
            horizontal_accuracy_meters: Some(TEST_ACCURACY_METERS),
            course_heading_degrees: None,
            timestamp_seconds: None,
        };
        let after_stray_session = reduce_navigation_event(
            Some(arrived_session.clone()),
            NavigationEvent::LocationUpdated(stray_update),
        )
        .unwrap();
        assert_eq!(after_stray_session.state, arrived_session.state);
    }

    #[test]
    fn deviating_fix_flags_the_trip_and_freezes_progress() {
        let session = start_manual_trip();
        // Walk onto the route first so there is live progress to freeze.
        let on_route_session = reduce_navigation_event(
            Some(session),
            NavigationEvent::LocationUpdated(route_fix(50.0)),
        )
        .unwrap();
        let frozen_state = on_route_session.state.clone();
        let deviated_session = reduce_navigation_event(
            Some(on_route_session),
            NavigationEvent::LocationUpdated(off_route_fix(50.0, TEST_OFF_ROUTE_EXCURSION_METERS)),
        )
        .unwrap();
        assert_eq!(deviated_session.state.phase, NavigationPhase::OffRoute);
        let deviation = deviated_session
            .state
            .deviation
            .expect("an off-route trip must record its deviation");
        assert_eq!(deviation.grade, DeviationGrade::FullyOffRoute);
        assert!(
            (deviation.deviation_from_route_meters - TEST_OFF_ROUTE_EXCURSION_METERS).abs()
                < NEAR_ZERO_TOLERANCE_METERS
        );
        // Frozen: the projection and progress of the last on-route fix stand.
        assert_eq!(deviated_session.state.progress, frozen_state.progress);
        assert_eq!(
            deviated_session.state.snapped_coordinate,
            frozen_state.snapped_coordinate
        );
        assert_eq!(
            deviated_session.state.current_step_position,
            frozen_state.current_step_position
        );
    }

    #[test]
    fn deeper_off_route_fix_stays_frozen_with_the_latest_measurement() {
        let session = start_manual_trip();
        let deviated_session = reduce_navigation_event(
            Some(session),
            NavigationEvent::LocationUpdated(off_route_fix(50.0, TEST_OFF_ROUTE_EXCURSION_METERS)),
        )
        .unwrap();
        let frozen_state = deviated_session.state.clone();
        let deeper_session = reduce_navigation_event(
            Some(deviated_session),
            NavigationEvent::LocationUpdated(off_route_fix(
                60.0,
                TEST_OFF_ROUTE_EXCURSION_METERS * 2.0,
            )),
        )
        .unwrap();
        assert_eq!(deeper_session.state.phase, NavigationPhase::OffRoute);
        let deviation = deeper_session.state.deviation.unwrap();
        assert_eq!(
            deviation.grade,
            DeviationGrade::FullyOffRoute,
            "the deeper fix must keep the freezing grade, not the wrong-step grade"
        );
        assert!(
            (deviation.deviation_from_route_meters - TEST_OFF_ROUTE_EXCURSION_METERS * 2.0).abs()
                < NEAR_ZERO_TOLERANCE_METERS,
            "the recorded measurement follows the latest deviating fix"
        );
        assert_eq!(deeper_session.state.progress, frozen_state.progress);
        assert_eq!(
            deeper_session.state.snapped_coordinate,
            frozen_state.snapped_coordinate
        );
    }

    #[test]
    fn deviation_lifts_when_the_walker_returns_to_the_route() {
        let session = start_manual_trip();
        let deviated_session = reduce_navigation_event(
            Some(session),
            NavigationEvent::LocationUpdated(off_route_fix(50.0, TEST_OFF_ROUTE_EXCURSION_METERS)),
        )
        .unwrap();
        assert_eq!(deviated_session.state.phase, NavigationPhase::OffRoute);
        let recovered_session = reduce_navigation_event(
            Some(deviated_session),
            NavigationEvent::LocationUpdated(route_fix(70.0)),
        )
        .unwrap();
        assert_eq!(recovered_session.state.phase, NavigationPhase::Navigating);
        assert!(
            recovered_session.state.deviation.is_none(),
            "recovery must clear the deviation"
        );
        let snapped_coordinate = recovered_session.state.snapped_coordinate.unwrap();
        assert!(
            snapped_coordinate.haversine_distance_meters(&offset_coordinate(70.0, 0.0))
                < NEAR_ZERO_TOLERANCE_METERS,
            "the recovering fix re-anchors the projection onto the route"
        );
        assert!(
            (recovered_session
                .state
                .progress
                .distance_to_next_maneuver_meters
                - (TEST_STEP_LENGTH_METERS - 70.0))
                .abs()
                < NEAR_ZERO_TOLERANCE_METERS
        );
        assert!(
            (recovered_session.state.progress.distance_remaining_meters
                - (TEST_ROUTE_TOTAL_LENGTH_METERS - 70.0))
                .abs()
                < NEAR_ZERO_TOLERANCE_METERS
        );
    }

    #[test]
    fn deviating_fix_cannot_advance_the_step() {
        // The delayed condition would advance on any fix more than its
        // threshold away from the step polyline, so a frozen (unconsulted)
        // deviating fix is distinguishable from an evaluated one: the on-route
        // fix stays put, the deviating one must not advance despite qualifying.
        let session = reduce_navigation_event(
            None,
            NavigationEvent::StartTrip {
                route: straight_three_step_route(),
                arrival_step_advance_condition: None,
                course_filtering: CourseFiltering::Raw,
                step_advance_condition: Arc::new(
                    DistanceFromStepCondition::with_ferrostar_defaults(),
                ),
                deviation_detector: default_deviation_detector(),
            },
        )
        .unwrap();
        let first_session = reduce_navigation_event(
            Some(session),
            NavigationEvent::LocationUpdated(route_fix(10.0)),
        )
        .unwrap();
        assert_eq!(first_session.state.current_step_position, 0);
        let deviated_session = reduce_navigation_event(
            Some(first_session),
            NavigationEvent::LocationUpdated(off_route_fix(10.0, TEST_OFF_ROUTE_EXCURSION_METERS)),
        )
        .unwrap();
        assert_eq!(deviated_session.state.phase, NavigationPhase::OffRoute);
        assert_eq!(
            deviated_session.state.current_step_position, 0,
            "the deviating fix must not consult the advance condition, let alone advance"
        );
    }

    #[test]
    fn wrong_step_grade_records_without_freezing() {
        // A fix on a future step (the two-street fixture's north street) is
        // graded `OffStepOnRoute`: the trip stays `Navigating`, the grade is
        // recorded — no freeze, no reroute-triggering grade — and the
        // projection is recomputed from the fix (with the eager condition the
        // step does not reconcile within this fix, so the snap clamps onto
        // the current step's past-the-end terminal, exactly as an on-route
        // fix beyond the step's end always has).
        let session = reduce_navigation_event(
            None,
            NavigationEvent::StartTrip {
                route: two_street_route(),
                arrival_step_advance_condition: None,
                course_filtering: CourseFiltering::Raw,
                step_advance_condition: Arc::new(
                    DistanceToEndOfStepCondition::with_ferrostar_defaults(),
                ),
                deviation_detector: default_deviation_detector(),
            },
        )
        .unwrap();
        let on_route_session = reduce_navigation_event(
            Some(session),
            NavigationEvent::LocationUpdated(two_street_walker_fix(
                TEST_TWO_STREET_EAST_METERS / 2.0,
                0.0,
            )),
        )
        .unwrap();
        assert_eq!(on_route_session.state.phase, NavigationPhase::Navigating);
        assert!(on_route_session.state.deviation.is_none());
        let frozen_snapshot = on_route_session.state.clone();

        let wrong_step_session = reduce_navigation_event(
            Some(on_route_session),
            NavigationEvent::LocationUpdated(wrong_step_fix()),
        )
        .unwrap();
        assert_eq!(
            wrong_step_session.state.phase,
            NavigationPhase::Navigating,
            "the wrong-step grade must not freeze the trip"
        );
        let deviation = wrong_step_session
            .state
            .deviation
            .expect("the wrong-step grade must be recorded in the state");
        assert_eq!(deviation.grade, DeviationGrade::OffStepOnRoute);
        assert!(
            deviation.deviation_from_route_meters < NEAR_ZERO_TOLERANCE_METERS,
            "the wrong-step fix projects onto the remaining route line"
        );
        // Not frozen: the fix was processed normally — its projection onto
        // the current step (beyond the end, so clamped to the step's
        // terminal) replaced the previous fix's snap, and the progress was
        // recomputed from it. The eager condition cannot reconcile the step
        // within this fix (reconciliation is the advance condition's
        // contract, see the reconciling test below), so the countdown reads
        // zero clamped at the terminal rather than the frozen one-hundred.
        assert_eq!(
            wrong_step_session.state.snapped_coordinate,
            Some(offset_coordinate(TEST_TWO_STREET_EAST_METERS, 0.0))
        );
        assert_ne!(wrong_step_session.state, frozen_snapshot);
        assert!(
            wrong_step_session
                .state
                .progress
                .distance_to_next_maneuver_meters
                < NEAR_ZERO_TOLERANCE_METERS,
            "the countdown continues to be computed, not held frozen"
        );
    }

    #[test]
    fn wrong_step_grade_reconciles_to_the_projected_step_within_one_fix() {
        // The delayed condition advances away from the stale step; the
        // multi-step advance loop re-consults it for the next step with the
        // same fix, which now projects within the delayed threshold of its
        // own step — the trip reconciles to the wrong-step fix's step in
        // exactly this one fix.
        let session = reduce_navigation_event(
            None,
            NavigationEvent::StartTrip {
                route: two_street_route(),
                arrival_step_advance_condition: None,
                course_filtering: CourseFiltering::Raw,
                step_advance_condition: Arc::new(
                    DistanceFromStepCondition::with_ferrostar_defaults(),
                ),
                deviation_detector: default_deviation_detector(),
            },
        )
        .unwrap();
        let on_route_session = reduce_navigation_event(
            Some(session),
            NavigationEvent::LocationUpdated(two_street_walker_fix(
                TEST_TWO_STREET_EAST_METERS / 2.0,
                0.0,
            )),
        )
        .unwrap();
        assert_eq!(on_route_session.state.current_step_position, 0);

        let reconciled_session = reduce_navigation_event(
            Some(on_route_session),
            NavigationEvent::LocationUpdated(wrong_step_fix()),
        )
        .unwrap();
        assert_eq!(reconciled_session.state.phase, NavigationPhase::Navigating);
        assert_eq!(
            reconciled_session.state.current_step_position, 1,
            "one fix advances away from the stale step and settles on the fix's own step"
        );
        assert!(
            reconciled_session.state.deviation.is_some(),
            "the recorded wrong-step grade lags until the next graded fix (\
             the documented one-fix deviation lag)"
        );
        let snapped_coordinate = reconciled_session.state.snapped_coordinate.unwrap();
        assert!(
            snapped_coordinate.haversine_distance_meters(&wrong_step_fix().coordinate)
                < NEAR_ZERO_TOLERANCE_METERS,
            "the reconciled projection sits on the fix's own step"
        );
        let expected_countdown_meters =
            TEST_TWO_STREET_NORTH_METERS - TEST_WRONG_STEP_LATERAL_METERS;
        assert!(
            (reconciled_session
                .state
                .progress
                .distance_to_next_maneuver_meters
                - expected_countdown_meters)
                .abs()
                < NEAR_ZERO_TOLERANCE_METERS,
            "the countdown keeps counting down toward the reconciled maneuver"
        );
    }

    #[test]
    fn wrong_step_grade_lifts_a_frozen_trip() {
        // A graded fix that projects onto a future step — while still within
        // the remaining route line's allowance — recovers a frozen trip: the
        // fully-off grade's freeze holds only while fixes stay fully off.
        let session = reduce_navigation_event(
            None,
            NavigationEvent::StartTrip {
                route: two_street_route(),
                arrival_step_advance_condition: None,
                course_filtering: CourseFiltering::Raw,
                step_advance_condition: Arc::new(ManualStepAdvanceCondition),
                deviation_detector: default_deviation_detector(),
            },
        )
        .unwrap();
        let frozen_session = reduce_navigation_event(
            Some(session),
            NavigationEvent::LocationUpdated(two_street_walker_fix(
                TEST_TWO_STREET_EAST_METERS / 2.0,
                TEST_OFF_ROUTE_EXCURSION_METERS,
            )),
        )
        .unwrap();
        assert_eq!(frozen_session.state.phase, NavigationPhase::OffRoute);
        assert_eq!(
            frozen_session
                .state
                .deviation
                .map(|deviation_info| deviation_info.grade),
            Some(DeviationGrade::FullyOffRoute)
        );

        let lifted_session = reduce_navigation_event(
            Some(frozen_session),
            NavigationEvent::LocationUpdated(wrong_step_fix()),
        )
        .unwrap();
        assert_eq!(
            lifted_session.state.phase,
            NavigationPhase::Navigating,
            "a fix back on the route line lifts the freeze even from a stale step"
        );
        let deviation = lifted_session.state.deviation.unwrap();
        assert_eq!(deviation.grade, DeviationGrade::OffStepOnRoute);
    }

    #[test]
    fn wrong_step_grade_clears_when_the_fix_returns_to_the_current_step() {
        let session = reduce_navigation_event(
            None,
            NavigationEvent::StartTrip {
                route: two_street_route(),
                arrival_step_advance_condition: None,
                course_filtering: CourseFiltering::Raw,
                step_advance_condition: Arc::new(ManualStepAdvanceCondition),
                deviation_detector: default_deviation_detector(),
            },
        )
        .unwrap();
        let wrong_step_session = reduce_navigation_event(
            Some(session),
            NavigationEvent::LocationUpdated(wrong_step_fix()),
        )
        .unwrap();
        assert_eq!(
            wrong_step_session
                .state
                .deviation
                .map(|deviation_info| deviation_info.grade),
            Some(DeviationGrade::OffStepOnRoute)
        );
        let returned_session = reduce_navigation_event(
            Some(wrong_step_session),
            NavigationEvent::LocationUpdated(two_street_walker_fix(
                TEST_TWO_STREET_EAST_METERS / 2.0,
                0.0,
            )),
        )
        .unwrap();
        assert_eq!(returned_session.state.phase, NavigationPhase::Navigating);
        assert!(
            returned_session.state.deviation.is_none(),
            "a fix the current step accounts for clears the wrong-step grade"
        );
    }

    #[test]
    fn one_fix_advances_through_many_steps_and_terminates_stack_safely() {
        // The reference stack-safety suite's convention, adapted: the replay
        // runs on a deliberately small thread stack (a fraction of a desktop
        // thread's default; mobile stacks are small), over a 1,000-step route
        // where one huge jump must advance through very many steps — the
        // reducer loop is iterative, so the replay must terminate and land on
        // the step the jumped fix projects onto, not recurse off the stack.
        let route = stack_safety_route();
        let jump_update = NavigationLocationUpdate {
            coordinate: offset_coordinate(TEST_STACK_SAFETY_JUMP_TARGET_ALONG_METERS, 0.0),
            horizontal_accuracy_meters: Some(TEST_ACCURACY_METERS),
            course_heading_degrees: None,
            timestamp_seconds: None,
        };
        let replay_thread = std::thread::Builder::new()
            .stack_size(TEST_STACK_SAFETY_THREAD_STACK_SIZE_BYTES)
            .spawn(move || {
                let started_session = reduce_navigation_event(
                    None,
                    NavigationEvent::StartTrip {
                        route,
                        arrival_step_advance_condition: None,
                        course_filtering: CourseFiltering::Raw,
                        step_advance_condition: Arc::new(DistanceFromStepCondition {
                            advance_threshold_meters: TEST_STACK_SAFETY_DELAYED_THRESHOLD_METERS,
                            minimum_horizontal_accuracy_meters:
                                DEFAULT_MINIMUM_HORIZONTAL_ACCURACY_METERS,
                        }),
                        deviation_detector: default_deviation_detector(),
                    },
                )
                .unwrap();
                reduce_navigation_event(
                    Some(started_session),
                    NavigationEvent::LocationUpdated(jump_update),
                )
                .unwrap()
            })
            .expect("the stack-safety thread must spawn");
        let jumped_session = replay_thread
            .join()
            .expect("the stack-safety replay must not panic on its small stack");

        let jump_target_state = &jumped_session.state;
        assert_eq!(
            jump_target_state.phase,
            NavigationPhase::Navigating,
            "the jumped fix sits on the route line and must not freeze the trip"
        );
        assert_eq!(
            jump_target_state.current_step_position, TEST_STACK_SAFETY_JUMP_TARGET_STEP_POSITION,
            "the loop hops the fix onto the step whose shape holds it and stops"
        );
        let snapped_coordinate = jump_target_state.snapped_coordinate.unwrap();
        assert!(
            snapped_coordinate.haversine_distance_meters(&jump_update.coordinate)
                < NEAR_ZERO_TOLERANCE_METERS,
            "the final projection is the jumped fix's own position"
        );
    }

    #[test]
    fn ungraded_fix_cannot_flag_or_clear_a_deviation() {
        let session = start_manual_trip();
        // An ungraded fix far off the route still snaps and progresses (it
        // only steps out of the deviation judgment), so it never flags.
        let ungraded_excursion =
            ungraded_fix(&off_route_fix(50.0, TEST_OFF_ROUTE_EXCURSION_METERS));
        let ungraded_session = reduce_navigation_event(
            Some(session),
            NavigationEvent::LocationUpdated(ungraded_excursion),
        )
        .unwrap();
        assert_eq!(ungraded_session.state.phase, NavigationPhase::Navigating);
        assert!(ungraded_session.state.deviation.is_none());
        assert!(ungraded_session.state.snapped_coordinate.is_some());

        // A graded fix flags the deviation…
        let deviated_session = reduce_navigation_event(
            Some(ungraded_session),
            NavigationEvent::LocationUpdated(off_route_fix(50.0, TEST_OFF_ROUTE_EXCURSION_METERS)),
        )
        .unwrap();
        assert_eq!(deviated_session.state.phase, NavigationPhase::OffRoute);
        // …and an ungraded fix back on the line can not lift it: the session
        // stays frozen exactly as it was.
        let untouched_session = reduce_navigation_event(
            Some(deviated_session.clone()),
            NavigationEvent::LocationUpdated(ungraded_fix(&route_fix(50.0))),
        )
        .unwrap();
        assert_eq!(untouched_session.state, deviated_session.state);
    }

    #[test]
    fn reroute_replaces_the_route_from_the_current_position() {
        let session = start_manual_trip();
        let walked_session = reduce_navigation_event(
            Some(session),
            NavigationEvent::LocationUpdated(route_fix(TEST_REROUTE_ANCHOR_OFFSET_METERS)),
        )
        .unwrap();
        let rerouted_session = reduce_navigation_event(
            Some(walked_session),
            NavigationEvent::RerouteProvided {
                route: single_step_reroute_route(),
                step_advance_condition: Arc::new(ManualStepAdvanceCondition),
            },
        )
        .unwrap();
        let state = &rerouted_session.state;
        assert_eq!(state.phase, NavigationPhase::Navigating);
        assert!(state.deviation.is_none(), "the reroute lifts any deviation");
        assert_eq!(state.current_step_position, 0);
        assert_eq!(state.step_positions.len(), 1);
        // Before the next fix, the new route's declared values are the truth.
        assert!(
            (state.progress.distance_to_next_maneuver_meters - TEST_REROUTE_STEP_LENGTH_METERS)
                .abs()
                < NEAR_ZERO_TOLERANCE_METERS
        );
        // Continuing along the new route: the walker's next fix lands inside
        // the new step, and progress is coherent with the NEW geometry only.
        let continued_session = reduce_navigation_event(
            Some(rerouted_session),
            NavigationEvent::LocationUpdated(route_fix(
                TEST_REROUTE_ANCHOR_OFFSET_METERS + WALKER_UPDATE_SPACING_METERS,
            )),
        )
        .unwrap();
        let progress = &continued_session.state.progress;
        assert!(
            (progress.distance_to_next_maneuver_meters
                - (TEST_REROUTE_STEP_LENGTH_METERS - WALKER_UPDATE_SPACING_METERS))
                .abs()
                < NEAR_ZERO_TOLERANCE_METERS,
            "progress must measure the new route from the rerouted position"
        );
        assert_eq!(
            progress.distance_remaining_meters,
            progress.distance_to_next_maneuver_meters
        );
        assert!(progress.distance_to_next_maneuver_meters >= 0.0);
        assert!(progress.distance_remaining_meters >= 0.0);
    }

    #[test]
    fn reroute_from_an_off_route_trip_recovers() {
        let session = start_manual_trip();
        let deviated_session = reduce_navigation_event(
            Some(session),
            NavigationEvent::LocationUpdated(off_route_fix(50.0, TEST_OFF_ROUTE_EXCURSION_METERS)),
        )
        .unwrap();
        assert_eq!(deviated_session.state.phase, NavigationPhase::OffRoute);
        let rerouted_session = reduce_navigation_event(
            Some(deviated_session),
            NavigationEvent::RerouteProvided {
                route: single_step_reroute_route(),
                step_advance_condition: Arc::new(ManualStepAdvanceCondition),
            },
        )
        .unwrap();
        assert_eq!(rerouted_session.state.phase, NavigationPhase::Navigating);
        assert!(rerouted_session.state.deviation.is_none());
    }

    #[test]
    fn reroute_after_arrival_is_ignored() {
        let session = reduce_navigation_event(
            None,
            NavigationEvent::StartTrip {
                route: straight_three_step_route(),
                arrival_step_advance_condition: None,
                course_filtering: CourseFiltering::Raw,
                step_advance_condition: Arc::new(
                    DistanceToEndOfStepCondition::with_ferrostar_defaults(),
                ),
                deviation_detector: default_deviation_detector(),
            },
        )
        .unwrap();
        let arrived_session = walk_route(session).pop().unwrap();
        assert_eq!(arrived_session.state.phase, NavigationPhase::Arrived);
        let rerouted_session = reduce_navigation_event(
            Some(arrived_session.clone()),
            NavigationEvent::RerouteProvided {
                route: single_step_reroute_route(),
                step_advance_condition: Arc::new(ManualStepAdvanceCondition),
            },
        )
        .unwrap();
        assert_eq!(rerouted_session.state, arrived_session.state);
    }

    #[test]
    fn reroute_without_an_active_trip_is_dropped() {
        let outcome = reduce_navigation_event(
            None,
            NavigationEvent::RerouteProvided {
                route: single_step_reroute_route(),
                step_advance_condition: Arc::new(ManualStepAdvanceCondition),
            },
        );
        assert!(
            outcome.is_none(),
            "a reroute has no meaning without a trip to replace the route of"
        );
    }

    #[test]
    fn reroute_installs_a_fresh_advance_condition() {
        let session = start_manual_trip();
        // The caller hands back a condition instance that already fired on the
        // old route; the reducer must rebuild it fresh.
        let rerouted_session = reduce_navigation_event(
            Some(session),
            NavigationEvent::RerouteProvided {
                route: single_step_reroute_route(),
                step_advance_condition: Arc::new(OneShotAdvanceCondition::consumed()),
            },
        )
        .unwrap();
        let first_session = reduce_navigation_event(
            Some(rerouted_session),
            NavigationEvent::LocationUpdated(route_fix(
                TEST_REROUTE_ANCHOR_OFFSET_METERS + WALKER_UPDATE_SPACING_METERS,
            )),
        )
        .unwrap();
        // The replacement route has a single (arrival) step, and the freshly
        // rebuilt condition fires on the first fix: the trip completes. Had
        // the reducer installed the consumed instance as-is, the fix would
        // never have advanced at all.
        assert_eq!(first_session.state.phase, NavigationPhase::Arrived);
    }

    #[test]
    fn combinator_keeps_the_stateful_child_schedule_across_advances() {
        let session = reduce_navigation_event(
            None,
            NavigationEvent::StartTrip {
                route: straight_three_step_route(),
                arrival_step_advance_condition: None,
                course_filtering: CourseFiltering::Raw,
                step_advance_condition: Arc::new(OrStepAdvanceCondition {
                    first_condition: Arc::new(ManualStepAdvanceCondition),
                    second_condition: Arc::new(UpdatesBeforeAdvanceCondition::requiring(3)),
                }),
                deviation_detector: Arc::new(NeverDeviatingDeviationDetector),
            },
        )
        .unwrap();
        let walked_sessions = walk_route(session);
        // Identical schedule to the bare counting condition: a stale child
        // instance would cascade further, sooner. The advances land at
        // updates 4, 7 and 10 (the multi-step advance loop consults an
        // advance's fresh child instance with the same fix, compressing the
        // schedule accordingly).
        let expected_positions = [0usize, 0, 0, 1, 1, 1, 2, 2, 2, 2, 2, 2];
        for (walked_session, expected_position) in walked_sessions.iter().zip(expected_positions) {
            assert_eq!(
                walked_session.state.current_step_position, expected_position,
                "the composite's fresh children must keep the schedule intact"
            );
        }
    }

    // -------------------------------------------------------------------
    // B5: the arrival-aware step advance (round 11)
    // -------------------------------------------------------------------

    /// Applies one on-route fix at `east_meters` and returns the new session.
    fn apply_on_route_fix(session: NavigationSession, east_meters: f64) -> NavigationSession {
        reduce_navigation_event(
            Some(session),
            NavigationEvent::LocationUpdated(route_fix(east_meters)),
        )
        .expect("a navigating trip must stay alive")
    }

    /// Inside the arrival window the separately configured arrival condition
    /// decides the hop: a trip condition that would happily keep walking is
    /// overruled, and the walk stalls on the first in-window step for as
    /// long as the arrival condition holds it (the reference's
    /// arrival-selector semantics, the current step's remaining-steps count
    /// itself included).
    #[test]
    fn the_arrival_condition_governs_the_last_two_steps() {
        let mut session = reduce_navigation_event(
            None,
            NavigationEvent::StartTrip {
                route: straight_three_step_route(),
                arrival_step_advance_condition: Some(Arc::new(ManualStepAdvanceCondition)),
                course_filtering: CourseFiltering::Raw,
                step_advance_condition: Arc::new(OneShotAdvanceCondition::unused()),
                deviation_detector: Arc::new(NeverDeviatingDeviationDetector),
            },
        )
        .expect("the fixture route must start a trip");
        session = apply_on_route_fix(session, 0.0);
        assert_eq!(
            session.state.current_step_position, TEST_ARRIVAL_WINDOW_FIRST_STEP_POSITION,
            "inside the arrival window only the arrival condition decides the hop"
        );
        assert_eq!(session.state.phase, NavigationPhase::Navigating);
        // No later fix un-stalls the walk: the window never closes on its
        // own and the manual arrival condition never fires.
        session = apply_on_route_fix(session, WALKER_UPDATE_SPACING_METERS);
        session = apply_on_route_fix(session, WALKER_UPDATE_SPACING_METERS * 2.0);
        assert_eq!(
            session.state.current_step_position,
            TEST_ARRIVAL_WINDOW_FIRST_STEP_POSITION
        );
    }

    /// One `Advance` verdict on the final step completes the trip, and an
    /// arrived trip consumes nothing further: the loop's arrival arm breaks
    /// ahead of any advance (no step follows the route end), so a consumed
    /// or absent verdict can never pop past the route end.
    #[test]
    fn an_arrival_verdict_on_the_final_step_completes_the_trip() {
        // A single-step route makes step 0 the final step; the trip's own
        // manual condition can never advance anything on its own.
        let mut session = reduce_navigation_event(
            None,
            NavigationEvent::StartTrip {
                route: single_step_reroute_route(),
                arrival_step_advance_condition: Some(Arc::new(OneShotAdvanceCondition::unused())),
                course_filtering: CourseFiltering::Raw,
                step_advance_condition: Arc::new(ManualStepAdvanceCondition),
                deviation_detector: default_deviation_detector(),
            },
        )
        .expect("the single-step route must start a trip");
        session = apply_on_route_fix(session, TEST_REROUTE_ANCHOR_OFFSET_METERS);
        assert_eq!(session.state.phase, NavigationPhase::Arrived);
        assert_eq!(session.state.current_step_position, 0);
        // Fixes after arrival leave the trip untouched (the consumed arrival
        // instance stays consumed and no further state is applied).
        session = apply_on_route_fix(
            session,
            TEST_REROUTE_ANCHOR_OFFSET_METERS + TEST_REROUTE_STEP_LENGTH_METERS / 2.0,
        );
        assert_eq!(session.state.phase, NavigationPhase::Arrived);
        assert_eq!(session.state.current_step_position, 0);
    }

    /// The arrival window's full schedule under a counting arrival
    /// condition, pinning every contract of the mechanism at once: the
    /// window opens at the first in-window step within the same fix (the
    /// loop re-consults the same fix after every hop), the arrival
    /// condition is rebuilt fresh for every hop inside the window exactly
    /// as the ordinary condition is, and the arrival verdict alone breaks
    /// the loop — the schedule never walks past the route end.
    #[test]
    fn the_arrival_schedule_pins_the_fresh_instance_and_window_contracts() {
        let mut session = reduce_navigation_event(
            None,
            NavigationEvent::StartTrip {
                route: straight_three_step_route(),
                arrival_step_advance_condition: Some(Arc::new(
                    UpdatesBeforeAdvanceCondition::requiring(
                        TEST_ARRIVAL_OBSERVATIONS_BEFORE_ADVANCE,
                    ),
                )),
                course_filtering: CourseFiltering::Raw,
                step_advance_condition: Arc::new(OneShotAdvanceCondition::unused()),
                deviation_detector: Arc::new(NeverDeviatingDeviationDetector),
            },
        )
        .expect("the fixture route must start a trip");

        // The eager trip condition walks to the window on the first fix.
        session = apply_on_route_fix(session, 0.0);
        assert_eq!(
            session.state.current_step_position,
            TEST_ARRIVAL_WINDOW_FIRST_STEP_POSITION
        );

        // The window consult runs within the first fix itself (the loop's
        // follow-up iteration after the eager hop), so the counting
        // arrival condition's first consult stays on the same step.
        session = apply_on_route_fix(session, WALKER_UPDATE_SPACING_METERS);
        assert_eq!(session.state.current_step_position, TEST_STEP_COUNT - 1);
        assert_eq!(session.state.phase, NavigationPhase::Navigating);

        // The fresh final-step instance's second consult fires the arrival
        // verdict: the trip completes from that verdict alone. A stale
        // (non-rebuilt) instance would have kept its counter and been one
        // verdict out of phase.
        session = apply_on_route_fix(session, WALKER_UPDATE_SPACING_METERS * 2.0);
        assert_eq!(session.state.phase, NavigationPhase::Arrived);
        assert_eq!(session.state.current_step_position, TEST_STEP_COUNT - 1);
    }

    /// Configuring the trip's own condition as the arrival condition
    /// explicitly behaves exactly like configuring none (the fallback):
    /// the fallback exists so an unconfigured trip keeps its existing
    /// behavior, and the equivalence pins that the fallback really consults
    /// the same condition, not some third policy.
    #[test]
    fn an_explicit_arrival_condition_matching_the_trip_condition_walks_identically() {
        let start_unconfigured = reduce_navigation_event(
            None,
            NavigationEvent::StartTrip {
                route: straight_three_step_route(),
                arrival_step_advance_condition: None,
                course_filtering: CourseFiltering::Raw,
                step_advance_condition: Arc::new(OneShotAdvanceCondition::unused()),
                deviation_detector: Arc::new(NeverDeviatingDeviationDetector),
            },
        )
        .expect("the fixture route must start a trip");
        let start_explicit = reduce_navigation_event(
            None,
            NavigationEvent::StartTrip {
                route: straight_three_step_route(),
                arrival_step_advance_condition: Some(Arc::new(OneShotAdvanceCondition::unused())),
                course_filtering: CourseFiltering::Raw,
                step_advance_condition: Arc::new(OneShotAdvanceCondition::unused()),
                deviation_detector: Arc::new(NeverDeviatingDeviationDetector),
            },
        )
        .expect("the fixture route must start a trip");

        // One fix each: both configurations speed-run to arrival through
        // one consult per step (the eager condition advancing on the final
        // step completes the trip).
        let mut unconfigured = apply_on_route_fix(start_unconfigured, 0.0);
        let mut explicit = apply_on_route_fix(start_explicit, 0.0);
        assert_eq!(
            unconfigured.state.current_step_position,
            TEST_STEP_COUNT - 1
        );
        assert_eq!(explicit.state.current_step_position, TEST_STEP_COUNT - 1);
        assert_eq!(unconfigured.state.phase, NavigationPhase::Arrived);
        assert_eq!(explicit.state.phase, NavigationPhase::Arrived);

        // Arrival holds afterwards for both.
        unconfigured = apply_on_route_fix(unconfigured, WALKER_UPDATE_SPACING_METERS);
        explicit = apply_on_route_fix(explicit, WALKER_UPDATE_SPACING_METERS);
        assert_eq!(unconfigured.state.phase, NavigationPhase::Arrived);
        assert_eq!(explicit.state.phase, NavigationPhase::Arrived);
    }

    // -------------------------------------------------------------------
    // B7: course filtering (round 11)
    // -------------------------------------------------------------------

    /// Builds a graded on-route fix at `east_meters` carrying the fixture's
    /// deliberately conflicting raw course.
    fn conflicting_course_fix(east_meters: f64) -> NavigationLocationUpdate {
        NavigationLocationUpdate {
            coordinate: offset_coordinate(east_meters, 0.0),
            horizontal_accuracy_meters: Some(TEST_ACCURACY_METERS),
            course_heading_degrees: Some(TEST_RAW_COURSE_DEGREES),
            timestamp_seconds: None,
        }
    }

    /// Applies one arbitrary fix and returns the new session.
    fn apply_fix(
        session: NavigationSession,
        update: NavigationLocationUpdate,
    ) -> NavigationSession {
        reduce_navigation_event(Some(session), NavigationEvent::LocationUpdated(update))
            .expect("a navigating trip must stay alive")
    }

    /// Under `SnapToRoute` the effective course is the snapped position's
    /// route bearing when the fix's own course disagrees with it: the
    /// follow-camera heading follows the road, not the noisy fix report.
    #[test]
    fn snap_to_route_replaces_a_conflicting_raw_course_with_the_route_bearing() {
        let session = reduce_navigation_event(
            None,
            NavigationEvent::StartTrip {
                route: straight_three_step_route(),
                arrival_step_advance_condition: None,
                course_filtering: CourseFiltering::SnapToRoute,
                step_advance_condition: Arc::new(ManualStepAdvanceCondition),
                deviation_detector: default_deviation_detector(),
            },
        )
        .expect("the fixture route must start a trip");
        let session = apply_fix(
            session,
            conflicting_course_fix(TEST_STEP_LENGTH_METERS / 2.0),
        );
        let course_heading_degrees = session
            .state
            .course_heading_degrees
            .expect("a snapped fix must stamp an effective course");
        assert!(
            (course_heading_degrees - TEST_EASTWARD_ROUTE_BEARING_DEGREES).abs()
                < TEST_COURSE_TOLERANCE_DEGREES,
            "the snapped bearing must replace a conflicting raw course, got {course_heading_degrees}"
        );
        assert_eq!(session.state.phase, NavigationPhase::Navigating);
    }

    /// Under `Raw` the reported course is stamped verbatim: the policy is
    /// the no-op default that preserves the pre-filtering behavior.
    #[test]
    fn raw_course_filtering_reports_the_reported_course_verbatim() {
        let session = reduce_navigation_event(
            None,
            NavigationEvent::StartTrip {
                route: straight_three_step_route(),
                arrival_step_advance_condition: None,
                course_filtering: CourseFiltering::Raw,
                step_advance_condition: Arc::new(ManualStepAdvanceCondition),
                deviation_detector: default_deviation_detector(),
            },
        )
        .expect("the fixture route must start a trip");
        let session = apply_fix(
            session,
            conflicting_course_fix(TEST_STEP_LENGTH_METERS / 2.0),
        );
        assert_eq!(
            session.state.course_heading_degrees,
            Some(TEST_RAW_COURSE_DEGREES)
        );
    }

    /// A fully off-route fix freezes every measurement of the trip state —
    /// the stamped effective course included: the freeze's early return
    /// happens before the course stamping, so a deviating fix cannot
    /// overwrite the last on-route heading even under `SnapToRoute`.
    #[test]
    fn a_fully_off_route_fix_freezes_the_effective_course() {
        let session = reduce_navigation_event(
            None,
            NavigationEvent::StartTrip {
                route: straight_three_step_route(),
                arrival_step_advance_condition: None,
                course_filtering: CourseFiltering::SnapToRoute,
                step_advance_condition: Arc::new(ManualStepAdvanceCondition),
                deviation_detector: default_deviation_detector(),
            },
        )
        .expect("the fixture route must start a trip");
        let session = apply_fix(
            session,
            conflicting_course_fix(TEST_STEP_LENGTH_METERS / 2.0),
        );
        let frozen_course_heading_degrees = session
            .state
            .course_heading_degrees
            .expect("the on-route fix must have stamped the effective course");

        let deviating_fix = NavigationLocationUpdate {
            coordinate: offset_coordinate(
                TEST_STEP_LENGTH_METERS / 2.0,
                TEST_OFF_ROUTE_EXCURSION_METERS,
            ),
            horizontal_accuracy_meters: Some(TEST_ACCURACY_METERS),
            course_heading_degrees: Some(TEST_RAW_COURSE_DEGREES),
            timestamp_seconds: None,
        };
        let frozen_session = apply_fix(session, deviating_fix);
        assert_eq!(
            frozen_session
                .state
                .deviation
                .map(|deviation_info| deviation_info.grade),
            Some(DeviationGrade::FullyOffRoute),
            "the off-route fix must freeze the trip"
        );
        assert_eq!(
            frozen_session.state.course_heading_degrees,
            Some(frozen_course_heading_degrees),
            "the frozen fix must not overwrite the effective course"
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        /// Along any increasing walk of the synthetic route, the remaining
        /// distance never grows (up to haversine rounding).
        #[test]
        fn remaining_distance_never_increases_along_a_walk(
            walker_fractions in prop::collection::vec(0.0f64..1.0, 16),
        ) {
            let mut walker_offsets = walker_fractions_to_offsets(&walker_fractions);
            walker_offsets.sort_by(|first_offset, second_offset| {
                first_offset.partial_cmp(second_offset).unwrap()
            });

            let session = reduce_navigation_event(
                None,
                NavigationEvent::StartTrip {
                    route: straight_three_step_route(),
                    arrival_step_advance_condition: None,
                    course_filtering: CourseFiltering::Raw,
                    step_advance_condition: Arc::new(
                        DistanceToEndOfStepCondition::with_ferrostar_defaults(),
                    ),
                    deviation_detector: default_deviation_detector(),
                },
            )
            .unwrap();
            let mut session = session;
            let mut previous_remaining_meters =
                session.state.progress.distance_remaining_meters;
            for walker_offset in walker_offsets {
                session = reduce_navigation_event(
                    Some(session),
                    NavigationEvent::LocationUpdated(NavigationLocationUpdate {
                        coordinate: offset_coordinate(walker_offset, 0.0),
                        horizontal_accuracy_meters: Some(TEST_ACCURACY_METERS),
                        course_heading_degrees: None,
                        timestamp_seconds: None,
                    }),
                )
                .unwrap();
                let remaining_meters = session.state.progress.distance_remaining_meters;
                prop_assert!(
                    remaining_meters <= previous_remaining_meters + MONOTONICITY_TOLERANCE_METERS,
                    "remaining distance grew: {previous_remaining_meters} -> {remaining_meters} at offset {walker_offset}"
                );
                previous_remaining_meters = remaining_meters;
            }
        }

        /// Deviation judging is memoryless in this slice: on a trip whose
        /// step never advances (manual advance), each graded fix's verdict is
        /// exactly that fix's own — the freeze flag tracks the walk's
        /// remaining-route measurement update by update, and the deviating
        /// state is recorded whenever either grade fires (fully off route:
        /// beyond the remaining route line's allowance; on route, wrong step:
        /// beyond the current step's allowance while inside the remaining
        /// line's), clearing as soon as the walker re-enters both allowances.
        #[test]
        fn off_route_flag_tracks_each_fixs_own_verdict(
            walker_fractions in prop::collection::vec(0.0f64..1.0, 16),
            lateral_fractions in prop::collection::vec(0.0f64..1.0, 16),
        ) {
            let mut walker_offsets = walker_fractions_to_offsets(&walker_fractions);
            walker_offsets.sort_by(|first_offset, second_offset| {
                first_offset.partial_cmp(second_offset).unwrap()
            });

            let mut session = start_manual_trip();
            for (walker_offset, lateral_fraction) in walker_offsets.iter().zip(&lateral_fractions) {
                let lateral_offset_meters = lateral_fraction * TEST_OFF_ROUTE_EXCURSION_METERS;
                session = reduce_navigation_event(
                    Some(session),
                    NavigationEvent::LocationUpdated(off_route_fix(
                        *walker_offset,
                        lateral_offset_meters,
                    )),
                )
                .unwrap();
                // Both grade measurements are exactly these, against the east
                // west multi-step line and its first step: the perpendicular
                // lateral for the remaining route line, and (past the step's
                // terminal) the clamped distance to the terminal for the
                // current step.
                let distance_from_route_meters = lateral_offset_meters;
                let distance_from_current_step_meters = if *walker_offset <= TEST_STEP_LENGTH_METERS
                {
                    lateral_offset_meters
                } else {
                    (walker_offset - TEST_STEP_LENGTH_METERS)
                        .hypot(lateral_offset_meters)
                };
                let effective_allowance_meters = tester_effective_allowance_meters();
                let is_fully_off_route =
                    distance_from_route_meters > effective_allowance_meters;
                let is_off_step_on_route = !is_fully_off_route
                    && distance_from_current_step_meters > effective_allowance_meters;
                prop_assert_eq!(
                    session.state.phase == NavigationPhase::OffRoute,
                    is_fully_off_route,
                    "at east {} with lateral offset {}: \
                     the freeze flag must track the fix's own fully-off verdict",
                    walker_offset,
                    lateral_offset_meters
                );
                prop_assert_eq!(
                    session.state.deviation.is_some(),
                    is_fully_off_route || is_off_step_on_route,
                    "at east {} with lateral offset {}: \
                     a recorded deviation must exist exactly when a grade fires",
                    walker_offset,
                    lateral_offset_meters
                );
                if is_fully_off_route || is_off_step_on_route {
                    let deviation = session.state.deviation.unwrap();
                    prop_assert_eq!(
                        deviation.grade,
                        if is_fully_off_route {
                            DeviationGrade::FullyOffRoute
                        } else {
                            DeviationGrade::OffStepOnRoute
                        }
                    );
                    prop_assert!(
                        (deviation.deviation_from_route_meters - distance_from_route_meters).abs()
                            < NEAR_ZERO_TOLERANCE_METERS,
                        "the recorded measurement must equal the remaining-route excursion"
                    );
                }
            }
        }
    }

    /// Maps walk fractions onto meter offsets across the synthetic route.
    fn walker_fractions_to_offsets(walker_fractions: &[f64]) -> Vec<f64> {
        walker_fractions
            .iter()
            .map(|walker_fraction| walker_fraction * TEST_ROUTE_TOTAL_LENGTH_METERS)
            .collect()
    }
}
