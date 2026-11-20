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
mod tests;
