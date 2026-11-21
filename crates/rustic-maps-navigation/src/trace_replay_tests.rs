//! End-to-end navigation replay tests over the committed GPS-trace fixtures
//! ([`crate::trace_fixture`]): the full pipeline — route reducer, deviation
//! detection, the trace's reroute policy, instruction composition and the
//! observer broadcaster — is driven by recorded traces at their recorded
//! timing.
//!
//! Replay semantics pinned here (per trace scenario):
//!
//! - **Clean walk**: maneuvers fire in traversal order, within-step progress
//!   is monotonic up to the recorded noise bound, the trip arrives, every
//!   snapped position stays inside the raw fix's accuracy envelope, and the
//!   instructions' distance bands behave along the replay while the
//!   provider-authored text is carried verbatim.
//! - **Deviation and recovery**: the trip freezes `OffRoute` at the excursion
//!   with the progress of the last on-route fix, then a graded fix back in
//!   the allowance recovers it and re-anchors the projection.
//! - **Deviation and reroute**: the replay answers the first off-route
//!   judgment with the trace's replacement route (the reroute anchoring
//!   contract holds by construction — see the generator), the session is
//!   rebuilt with fresh decision instances, and progress continues on the new
//!   route to arrival.
//! - The broadcaster delivers the phase and deviation-topic events in the
//!   recorded order along every replay; a mid-replay cancellation stops
//!   exactly its own subscription.

use std::sync::Arc;
use std::sync::Mutex;

use proptest::prelude::*;

use geospatial_types::{GeographicCoordinate, RouteData, UnitSystem};

use crate::course_filtering::CourseFiltering;
use crate::deviation::DistanceDeviationDetector;
use crate::instruction::{
    DistancePhrase, DistanceUnit, METRIC_FAR_DISTANCE_THRESHOLD_METERS, ManeuverInstructionText,
    NavigationInstruction,
};
use crate::observer::{
    DeviationStatusChange, NavigationDispatchOutcome, NavigationPhaseChange,
    NavigationStateBroadcaster, NavigationStateChangeListener, TopicSubscription,
};
use crate::reducer::{NavigationEvent, reduce_navigation_event};
use crate::speech::{MINIMUM_INTER_UTTERANCE_SPACING_SECONDS, SpeechUtteranceKind};
use crate::state::{NavigationPhase, TripNavigationState};
use crate::step_advance::{
    DEFAULT_DISTANCE_TO_END_OF_STEP_THRESHOLD_METERS, DistanceToEndOfStepCondition,
    StepAdvanceCondition,
};
use crate::trace_fixture::{
    ALL_TRACE_SCENARIOS, CLEAN_TRACE_SCENARIO, EXCURSION_METERS, MAXIMUM_FIX_NOISE_METERS,
    NavigationTraceFixture, RECOVERY_TRACE_SCENARIO, REROUTE_TRACE_SCENARIO, RecordedRouteWalk,
    SAMPLE_SPACING_METERS, load_trace_fixture, planar_offset_coordinate,
};

/// The unit system the replay composes instructions in: the fixtures are
/// authored in metric, so the phrase assertions below speak meters.
const REPLAY_UNIT_SYSTEM: UnitSystem = UnitSystem::Metric;

/// Planar reconstruction tolerance, in meters, of the navigation geometry
/// model at fixture scale (the documented residual of the local
/// equirectangular projection at short segments).
const REPLAY_PLANAR_TOLERANCE_METERS: f64 = 0.1;

/// Tolerance, in meters, for within-step progress monotonicity along a
/// recorded trace: the radial fix noise can move a snap's along-position by
/// at most the noise clamp, and the projection adds its planar residual.
const REPLAY_MONOTONICITY_TOLERANCE_METERS: f64 =
    MAXIMUM_FIX_NOISE_METERS + REPLAY_PLANAR_TOLERANCE_METERS;

/// Tolerance, in meters, for a phrase magnitude's rounding shift: the worst
/// half increment of the metric bands below one kilometer (the moderate
/// band's 50 m increment rounds half-up, so a band's quantity may sit half an
/// increment away from the raw distance).
const REPLAY_PHRASE_ROUNDING_TOLERANCE_METERS: f64 = 25.0;

/// Upper bound, in meters, of the metric near band (the instruction module's
/// moderate-band threshold), mirrored here for the band-behavior assertions.
const REPLAY_METRIC_NEAR_BAND_UPPER_METERS: f64 = 100.0;

/// The metric moderate band's nearest-increment, in meters, mirrored from the
/// instruction module's documented band table (the constant itself is private
/// there): a trip-start phrase between the near and far thresholds must read
/// as a multiple of it, within half an increment of the declared distance.
const REPLAY_METRIC_MODERATE_ROUNDING_INCREMENT_METERS: f64 = 50.0;

/// Structural tolerance, in meters, for distances that must be (near) zero.
const REPLAY_NEAR_ZERO_TOLERANCE_METERS: f64 = 0.001;

/// The property test's fixed case count (the suite's property convention).
const REPLAY_PROPERTY_TEST_CASE_COUNT: u32 = 64;

/// The clean trace's snap envelope, in meters: the recorded radial noise
/// ceiling plus the Ferrostar-default eager advance window (the projection
/// may jump up to one advance threshold toward the next maneuver point) plus
/// the planar reconstruction tolerance.
const REPLAY_SNAP_ENVELOPE_METERS: f64 = MAXIMUM_FIX_NOISE_METERS
    + DEFAULT_DISTANCE_TO_END_OF_STEP_THRESHOLD_METERS
    + REPLAY_PLANAR_TOLERANCE_METERS;

/// The dispatch index after which the cancellation test's second instruction
/// subscription is cancelled, mid-replay.
const REPLAY_CANCELLATION_DISPATCH_INDEX: usize = 40;

/// One replayed fix: the session state after the reducer consumed the fix
/// (and, on the reroute trace, the reroute that immediately followed it),
/// plus which route epoch that state belongs to.
struct ReplayedSample {
    state: TripNavigationState,
    is_on_initial_route: bool,
}

/// Everything the replay tests need from one full trace replay.
struct TraceReplayOutcome {
    /// The state right after `StartTrip` (before any fix).
    initial_state: TripNavigationState,
    /// The state after each recorded fix, in recorded order (an off-route
    /// sample of the reroute trace carries the frozen pre-reroute state).
    sample_states: Vec<ReplayedSample>,
    /// The instruction payload of every completed dispatch, in order.
    instructions: Vec<NavigationInstruction>,
    /// The phase-topic transitions, in delivery order.
    phase_changes: Vec<NavigationPhaseChange>,
    /// The deviation-topic updates, in delivery order.
    deviation_changes: Vec<DeviationStatusChange>,
    /// Deliveries the mid-replay-cancelled second instruction subscription
    /// still received (zero when no cancellation was requested).
    cancelled_instruction_delivery_count: usize,
}

/// Recording listener capturing the delivered payload of every topic.
#[derive(Default)]
struct TraceReplayListener {
    instructions: Mutex<Vec<NavigationInstruction>>,
    deviation_changes: Mutex<Vec<DeviationStatusChange>>,
    phase_changes: Mutex<Vec<NavigationPhaseChange>>,
}

impl NavigationStateChangeListener for TraceReplayListener {
    fn on_navigation_instruction(&self, instruction: &NavigationInstruction) {
        self.instructions
            .lock()
            .expect("listener locks cannot fail")
            .push(instruction.clone());
    }

    fn on_deviation_status_change(&self, deviation_status_change: &DeviationStatusChange) {
        self.deviation_changes
            .lock()
            .expect("listener locks cannot fail")
            .push(deviation_status_change.clone());
    }

    fn on_navigation_phase_change(&self, phase_change: &NavigationPhaseChange) {
        self.phase_changes
            .lock()
            .expect("listener locks cannot fail")
            .push(*phase_change);
    }
}

/// The replay's step-advance condition: the Ferrostar-default eager advance.
///
/// This is deliberately the same condition the generator used to derive the
/// reroute anchor, so the replay and the recorded anchoring agree exactly.
fn replay_step_advance_condition() -> Arc<dyn StepAdvanceCondition> {
    Arc::new(DistanceToEndOfStepCondition::with_ferrostar_defaults())
}

/// Replays `fixture` through the reducer, the reroute policy and the
/// broadcaster, dispatching every recorded fix at its recorded timing.
///
/// The replay's reroute policy is the trace's own answer to a deviation: when
/// the trip is first judged off route, the replay issues `RerouteProvided`
/// immediately with the fixture's replacement route (and the condition the
/// reducer will rebuild fresh) — or keeps feeding fixes when the scenario
/// carries no replacement route (the recovery scenario). `cancel_after` (when
/// given) subscribes a second, instruction-only listener and cancels its
/// subscription after that many completed dispatches, mid-replay.
fn run_trace_replay(
    fixture: &NavigationTraceFixture,
    cancel_second_instruction_listener_after: Option<usize>,
) -> TraceReplayOutcome {
    let broadcaster = NavigationStateBroadcaster::new();
    let replay_listener = Arc::new(TraceReplayListener::default());
    let subscribed_listener: Arc<dyn NavigationStateChangeListener> = replay_listener.clone();
    let _instruction_subscription =
        broadcaster.subscribe_to_navigation_instructions(subscribed_listener.clone());
    let _deviation_subscription =
        broadcaster.subscribe_to_deviation_status(subscribed_listener.clone());
    let _phase_subscription = broadcaster.subscribe_to_phase_changes(subscribed_listener);

    let cancelled_listener =
        cancel_second_instruction_listener_after.map(|_| Arc::new(TraceReplayListener::default()));
    let cancelled_subscription: Option<TopicSubscription> =
        cancelled_listener.as_ref().map(|cancelled_listener| {
            let cancelled_subscribed_listener: Arc<dyn NavigationStateChangeListener> =
                cancelled_listener.clone();
            broadcaster.subscribe_to_navigation_instructions(cancelled_subscribed_listener)
        });

    let mut session = reduce_navigation_event(
        None,
        NavigationEvent::StartTrip {
            route: fixture.route().clone(),
            arrival_step_advance_condition: None,
            course_filtering: CourseFiltering::Raw,
            step_advance_condition: replay_step_advance_condition(),
            deviation_detector: Arc::new(DistanceDeviationDetector::with_default_threshold()),
        },
    )
    .expect("the fixture route must start a trip");
    let initial_state = session.state.clone();
    let mut completed_dispatch_count = 0_usize;
    assert_delivered(
        broadcaster.dispatch_trip_started(&initial_state, REPLAY_UNIT_SYSTEM),
        completed_dispatch_count,
    );
    completed_dispatch_count += 1;
    cancel_if_due(
        &cancelled_subscription,
        cancel_second_instruction_listener_after,
        completed_dispatch_count,
    );

    let mut sample_states = Vec::new();
    let mut is_on_initial_route = true;
    for update in fixture.iter_location_updates() {
        let previous_state = session.state.clone();
        session = reduce_navigation_event(Some(session), NavigationEvent::LocationUpdated(update))
            .expect("the traced trip must stay alive");
        let is_deviation_freshly_flagged = previous_state.phase == NavigationPhase::Navigating
            && session.state.phase == NavigationPhase::OffRoute;
        if is_deviation_freshly_flagged && fixture.reroute_route().is_some() {
            // The trace's reroute moment: the deviating fix is broadcast in
            // its own dispatch (the off-route marker is real guidance
            // feedback), then the replacement route is applied.
            assert_delivered(
                broadcaster.dispatch_location_update(
                    &previous_state,
                    &session.state,
                    REPLAY_UNIT_SYSTEM,
                ),
                completed_dispatch_count,
            );
            completed_dispatch_count += 1;
            cancel_if_due(
                &cancelled_subscription,
                cancel_second_instruction_listener_after,
                completed_dispatch_count,
            );
            sample_states.push(ReplayedSample {
                state: session.state.clone(),
                is_on_initial_route: true,
            });
            let off_route_state = session.state.clone();
            session = reduce_navigation_event(
                Some(session),
                NavigationEvent::RerouteProvided {
                    route: fixture
                        .reroute_route()
                        .expect("checked: the trace carries a replacement route")
                        .clone(),
                    step_advance_condition: replay_step_advance_condition(),
                },
            )
            .expect("the traced trip must survive the reroute");
            assert_delivered(
                broadcaster.dispatch_reroute_applied(
                    &off_route_state,
                    &session.state,
                    REPLAY_UNIT_SYSTEM,
                ),
                completed_dispatch_count,
            );
            completed_dispatch_count += 1;
            cancel_if_due(
                &cancelled_subscription,
                cancel_second_instruction_listener_after,
                completed_dispatch_count,
            );
            is_on_initial_route = false;
        } else {
            assert_delivered(
                broadcaster.dispatch_location_update(
                    &previous_state,
                    &session.state,
                    REPLAY_UNIT_SYSTEM,
                ),
                completed_dispatch_count,
            );
            completed_dispatch_count += 1;
            cancel_if_due(
                &cancelled_subscription,
                cancel_second_instruction_listener_after,
                completed_dispatch_count,
            );
            sample_states.push(ReplayedSample {
                state: session.state.clone(),
                is_on_initial_route,
            });
        }
    }

    assert_eq!(
        sample_states.len(),
        fixture.samples().len(),
        "every recorded fix must leave exactly one replayed state"
    );
    TraceReplayOutcome {
        initial_state,
        sample_states,
        instructions: replay_listener
            .instructions
            .lock()
            .expect("listener locks cannot fail")
            .clone(),
        phase_changes: replay_listener
            .phase_changes
            .lock()
            .expect("listener locks cannot fail")
            .clone(),
        deviation_changes: replay_listener
            .deviation_changes
            .lock()
            .expect("listener locks cannot fail")
            .clone(),
        cancelled_instruction_delivery_count: cancelled_listener
            .map(|cancelled_listener| {
                cancelled_listener
                    .instructions
                    .lock()
                    .expect("listener locks cannot fail")
                    .len()
            })
            .unwrap_or(0),
    }
}

/// Asserts one dispatch ran to delivery.
fn assert_delivered(outcome: NavigationDispatchOutcome, dispatch_index: usize) {
    assert_eq!(
        outcome,
        NavigationDispatchOutcome::Delivered,
        "a single-threaded replay never suppresses dispatch {dispatch_index}"
    );
}

/// Cancels the cancellation test's second subscription when the recorded
/// dispatch count reaches the recorded cutoff.
fn cancel_if_due(
    cancelled_subscription: &Option<TopicSubscription>,
    cancellation_after: Option<usize>,
    completed_dispatch_count: usize,
) {
    if let (Some(cutoff), Some(subscription)) = (cancellation_after, cancelled_subscription)
        && completed_dispatch_count == cutoff
    {
        subscription.cancel();
    }
}

/// Flattens a route's step shapes into one polyline: the same remaining-route-
/// line target the deviation detector measures against at trip start.
fn flatten_route_line(route: &RouteData) -> Vec<GeographicCoordinate> {
    route
        .legs
        .iter()
        .flat_map(|leg| leg.steps.iter())
        .flat_map(|step| step.shape.iter().copied())
        .collect()
}

/// Asserts the active maneuver's distance phrase is a metric quantity near
/// `expected_distance_meters` (the recorded noise bound plus the worst
/// rounding shift of the metric bands in play).
fn assert_active_distance_phrase_near(
    instruction: &NavigationInstruction,
    expected_distance_meters: f64,
) {
    let DistancePhrase::Quantity { magnitude, unit } = &instruction.active_maneuver.distance_phrase
    else {
        panic!(
            "expected a quantified metric phrase at this sampled moment, got \
             {instruction:?}"
        );
    };
    assert_eq!(
        unit,
        &DistanceUnit::Meters,
        "the recorded routes keep every step below the 1 km far band, so the phrase stays in meters"
    );
    let tolerance_meters = MAXIMUM_FIX_NOISE_METERS + REPLAY_PHRASE_ROUNDING_TOLERANCE_METERS;
    let distance_deviation_meters = (magnitude - expected_distance_meters).abs();
    assert!(
        distance_deviation_meters <= tolerance_meters,
        "the phrase magnitude {magnitude} must sit within {tolerance_meters} of \
         the raw remaining distance {expected_distance_meters}"
    );
}

/// Asserts the composed channels carry the provider-authored text verbatim
/// beside the live distance phrase (the prefix disappears entirely in the
/// immediate band).
fn assert_provider_text_carried_verbatim(
    instruction: &NavigationInstruction,
    replayed_sample: &ReplayedSample,
) {
    let current_step = replayed_sample
        .state
        .current_step()
        .expect("a replay delivery always composes over an active step");
    let ManeuverInstructionText::Composed {
        provider_display_text,
        provider_speech_text,
        display_text,
        speech_text,
    } = &instruction.active_maneuver.instruction
    else {
        panic!("the fixture steps are always provider-authored");
    };
    assert_eq!(
        provider_display_text, &current_step.instruction_display_text,
        "the provider display text is carried verbatim"
    );
    assert_eq!(
        provider_speech_text, &current_step.instruction_speech_text,
        "the provider speech text is carried verbatim"
    );
    let distance_prefix_text = match &instruction.active_maneuver.distance_phrase {
        DistancePhrase::Immediate => String::new(),
        distance_phrase => format!("In {}, ", distance_phrase.display_text()),
    };
    assert_eq!(
        display_text,
        &format!("{distance_prefix_text}{provider_display_text}"),
        "the display channel composes the live prefix around the provider text"
    );
    assert_eq!(
        speech_text,
        &format!("{distance_prefix_text}{provider_speech_text}"),
        "the speech channel composes the live prefix around the provider text"
    );
}

#[test]
fn clean_trace_walks_every_step_in_order_and_arrives() {
    let fixture = load_trace_fixture(CLEAN_TRACE_SCENARIO);
    let outcome = run_trace_replay(&fixture, None);

    // The step sequence is visited in traversal order without skipping, and
    // the walk ends arrived with zeroed progress.
    let mut visited_step_positions = vec![outcome.initial_state.current_step_position];
    for replayed_sample in &outcome.sample_states {
        let step_position = replayed_sample.state.current_step_position;
        if *visited_step_positions.last().expect("seeded") != step_position {
            visited_step_positions.push(step_position);
        }
    }
    let recorded_step_count: usize = fixture.route().legs.iter().map(|leg| leg.steps.len()).sum();
    assert_eq!(
        visited_step_positions,
        (0..recorded_step_count).collect::<Vec<usize>>(),
        "the clean walk must advance through every recorded step in order"
    );
    let final_sample = outcome.sample_states.last().expect("walked samples");
    assert_eq!(final_sample.state.phase, NavigationPhase::Arrived);
    assert!(
        final_sample.state.progress.distance_remaining_meters < REPLAY_NEAR_ZERO_TOLERANCE_METERS,
        "the arrived trip's remaining progress is zero"
    );

    // Within a step (and a route epoch), the distance to the maneuver and the
    // total remaining distance never grow beyond the recorded noise bound.
    for replayed_pair in outcome.sample_states.windows(2) {
        let (previous_sample, next_sample) = (&replayed_pair[0], &replayed_pair[1]);
        let is_same_epoch = previous_sample.is_on_initial_route == next_sample.is_on_initial_route;
        let is_same_step =
            previous_sample.state.current_step_position == next_sample.state.current_step_position;
        if is_same_epoch && is_same_step {
            assert!(
                next_sample.state.progress.distance_to_next_maneuver_meters
                    <= previous_sample
                        .state
                        .progress
                        .distance_to_next_maneuver_meters
                        + REPLAY_MONOTONICITY_TOLERANCE_METERS,
                "distance to the maneuver must not grow within a step beyond \
                 the recorded noise"
            );
            assert!(
                next_sample.state.progress.distance_remaining_meters
                    <= previous_sample.state.progress.distance_remaining_meters
                        + REPLAY_MONOTONICITY_TOLERANCE_METERS,
                "remaining distance must not grow within a step beyond the \
                 recorded noise"
            );
        }
    }

    // Every navigating fix's snapped position sits inside the recorded noise
    // plus the eager advance window: Ferrostar-default semantics may advance
    // the projection up to the step-end threshold before the raw fix reaches
    // the maneuver point, and the fix that follows (still inside the window)
    // then snaps to the new step's head — the only way the projection can run
    // ahead of a corridor fix. The strict accuracy-envelope property (the fix
    // snapped onto the whole route line within its own accuracy) is explored
    // by the property test below this test.
    for (replayed_sample, sample) in outcome.sample_states.iter().zip(fixture.samples()) {
        if replayed_sample.state.phase == NavigationPhase::Navigating {
            let snapped_coordinate = replayed_sample
                .state
                .snapped_coordinate
                .expect("a navigating replayed state is snapped");
            let fix_to_snap_meters = sample
                .coordinate
                .haversine_distance_meters(&snapped_coordinate);
            assert!(
                fix_to_snap_meters <= REPLAY_SNAP_ENVELOPE_METERS,
                "the snapped position must stay inside the noise plus eager \
                 advance envelope: {fix_to_snap_meters} m"
            );
        }
    }

    // The clean walk never deviates and never marks rerouting required.
    assert!(
        outcome.deviation_changes.is_empty(),
        "the clean walk must produce no deviation-topic updates"
    );
    assert!(
        outcome
            .instructions
            .iter()
            .all(|instruction| !instruction.is_rerouting_required),
        "the clean walk never marks rerouting required"
    );
}

#[test]
fn clean_trace_composes_provider_instructions_at_sampled_moments() {
    let fixture = load_trace_fixture(CLEAN_TRACE_SCENARIO);
    let outcome = run_trace_replay(&fixture, None);
    let deliveries = &outcome.instructions;

    // Delivery map: index 0 is the trip start (declared progress), 1 + the
    // sample index is the delivery after that fix.
    assert_eq!(
        deliveries.len(),
        1 + fixture.samples().len(),
        "every dispatch delivers one instruction payload"
    );

    // The trip start phrases the recorded first step's declared distance in
    // the metric moderate band (the step sits between the 100 m near threshold
    // and the 1 km far threshold): a nearest-50 multiple within half an
    // increment of the declared distance. The provider-authored text follows.
    let first_step_declared_distance_meters = fixture.route().legs[0].steps[0].distance_meters;
    assert!(
        (REPLAY_METRIC_NEAR_BAND_UPPER_METERS..METRIC_FAR_DISTANCE_THRESHOLD_METERS)
            .contains(&first_step_declared_distance_meters),
        "the recorded first step must sit in the moderate band for this pin"
    );
    let trip_start_delivery = &deliveries[0];
    let DistancePhrase::Quantity {
        magnitude: trip_start_magnitude,
        unit: trip_start_unit,
    } = trip_start_delivery.active_maneuver.distance_phrase
    else {
        panic!("the trip start phrases a quantified distance");
    };
    assert_eq!(trip_start_unit, DistanceUnit::Meters);
    assert_eq!(
        trip_start_magnitude % REPLAY_METRIC_MODERATE_ROUNDING_INCREMENT_METERS,
        0.0,
        "the trip-start phrase must be a moderate-band multiple of \
         {REPLAY_METRIC_MODERATE_ROUNDING_INCREMENT_METERS} m"
    );
    assert!(
        (trip_start_magnitude - first_step_declared_distance_meters).abs()
            <= REPLAY_METRIC_MODERATE_ROUNDING_INCREMENT_METERS / 2.0,
        "the trip-start phrase must round the declared first-step distance"
    );
    assert_provider_text_carried_verbatim(
        trip_start_delivery,
        &ReplayedSample {
            state: outcome.initial_state.clone(),
            is_on_initial_route: true,
        },
    );

    // After the first fix the countdown is the declared distance minus one
    // recorded spacing (the fix's along-route advance), still a metric band
    // read of it.
    let first_fix_delivery = &deliveries[1];
    assert_active_distance_phrase_near(
        first_fix_delivery,
        first_step_declared_distance_meters - SAMPLE_SPACING_METERS,
    );

    // A mid-step fix phrases the near band (below the 100 m threshold): pick
    // the first replay moment still on the first step and inside that band, so
    // the phrase is the near band's read of the raw remaining distance.
    let mid_step_sample_index = outcome
        .sample_states
        .iter()
        .position(|replayed_sample| {
            replayed_sample.state.current_step_position == 0
                && replayed_sample
                    .state
                    .progress
                    .distance_to_next_maneuver_meters
                    < REPLAY_METRIC_NEAR_BAND_UPPER_METERS
                && replayed_sample
                    .state
                    .progress
                    .distance_to_next_maneuver_meters
                    > DEFAULT_DISTANCE_TO_END_OF_STEP_THRESHOLD_METERS
        })
        .expect("the recorded first step must give a near-band moment on the walk");
    let mid_step_raw_distance_meters = outcome.sample_states[mid_step_sample_index]
        .state
        .progress
        .distance_to_next_maneuver_meters;
    assert_active_distance_phrase_near(
        &deliveries[1 + mid_step_sample_index],
        mid_step_raw_distance_meters,
    );

    // Along the whole replay: the provider-authored text is carried verbatim,
    // every quantified phrase is a metric Meters read near the raw remaining
    // distance of its own state (no recorded step crosses the 1 km far-band
    // threshold, so a kilometer phrase is a fixture drift), and the clean
    // trace never deviates.
    for (replayed_sample, delivery) in outcome.sample_states.iter().zip(deliveries.iter().skip(1)) {
        assert_provider_text_carried_verbatim(delivery, replayed_sample);
        let raw_distance_to_maneuver_meters = replayed_sample
            .state
            .progress
            .distance_to_next_maneuver_meters;
        match &delivery.active_maneuver.distance_phrase {
            DistancePhrase::Immediate => {
                assert!(
                    raw_distance_to_maneuver_meters < REPLAY_METRIC_NEAR_BAND_UPPER_METERS,
                    "the immediate band must only phrase near the maneuver"
                );
            }
            DistancePhrase::Quantity { magnitude, unit } => {
                assert_eq!(
                    unit,
                    &DistanceUnit::Meters,
                    "the recorded route's steps stay below the 1 km far band, so the phrase stays in meters"
                );
                let phrase_deviation_meters = (magnitude - raw_distance_to_maneuver_meters).abs();
                assert!(
                    phrase_deviation_meters
                        <= MAXIMUM_FIX_NOISE_METERS + REPLAY_PHRASE_ROUNDING_TOLERANCE_METERS,
                    "the phrase magnitude must be the raw distance's band read"
                );
            }
            // The clean trace never deviates, so the compose-time suppression
            // state must never appear in its replayed instructions.
            DistancePhrase::Suppressed => {
                panic!("the clean trace's replay must never suppress a distance phrase")
            }
        }
    }

    // The upcoming maneuver carries the following step's provider text while
    // the route has one after the active step.
    let navigation_starts_upcoming = &deliveries[1]
        .upcoming_maneuver
        .as_ref()
        .expect("while the first step is active the second step is upcoming");
    let second_step = outcome
        .sample_states
        .first()
        .and_then(|first_sample| first_sample.state.step_at(1))
        .expect("the fixture route has a second step");
    let ManeuverInstructionText::Composed {
        display_text: upcoming_display_text,
        ..
    } = &navigation_starts_upcoming.instruction
    else {
        panic!("the fixture's second step is provider-authored");
    };
    assert_eq!(
        upcoming_display_text, &second_step.instruction_display_text,
        "the upcoming maneuver carries the following step's provider text"
    );

    // The final (arrived) delivery carries the arrival marker, the immediate
    // phrase and no upcoming maneuver.
    let arrived_sample_index = outcome
        .sample_states
        .iter()
        .position(|replayed_sample| replayed_sample.state.phase == NavigationPhase::Arrived)
        .expect("the clean walk arrives");
    let arrived_delivery = &deliveries[1 + arrived_sample_index];
    assert!(arrived_delivery.is_arrival);
    assert_eq!(
        arrived_delivery.active_maneuver.distance_phrase,
        DistancePhrase::Immediate
    );
    assert!(arrived_delivery.upcoming_maneuver.is_none());
}

#[test]
fn deviation_trace_freezes_then_recovers_and_reanchors() {
    let fixture = load_trace_fixture(RECOVERY_TRACE_SCENARIO);
    let outcome = run_trace_replay(&fixture, None);

    // The phase topic records exactly the freeze, the recovery and the arrival.
    assert_eq!(
        outcome.phase_changes,
        vec![
            NavigationPhaseChange::OffRouteEntered,
            NavigationPhaseChange::OffRouteCleared,
            NavigationPhaseChange::Arrived,
        ]
    );

    // The deviation topic opens with the excursion's measurement and closes
    // with the recovery's clear; every recorded measurement is beyond the
    // threshold (only beyond-allowance fixes freeze the trip).
    assert_eq!(
        outcome
            .deviation_changes
            .last()
            .expect("the recovery trace deviates"),
        &DeviationStatusChange::OnRoute
    );
    let first_deviation_measurement = match outcome.deviation_changes.first() {
        Some(DeviationStatusChange::OffRoute(deviation_info)) => {
            deviation_info.deviation_from_route_meters
        }
        other => panic!(
            "the first deviation-topic update must be the off-route judgment, \
             got {other:?}"
        ),
    };
    assert!(
        (first_deviation_measurement - EXCURSION_METERS).abs()
            <= MAXIMUM_FIX_NOISE_METERS + REPLAY_PLANAR_TOLERANCE_METERS,
        "the recorded measurement must follow the excursion: {first_deviation_measurement}"
    );
    for deviation_status_change in &outcome.deviation_changes {
        let DeviationStatusChange::OffRoute(deviation_info) = deviation_status_change else {
            continue;
        };
        assert!(
            deviation_info.deviation_from_route_meters > fixture.deviation_threshold_meters(),
            "every recorded off-route measurement is beyond the threshold"
        );
    }

    // While frozen, the projection and progress of the last on-route fix
    // stand: no step advance, no snap, no recompute.
    let first_off_route_index = outcome
        .sample_states
        .iter()
        .position(|replayed_sample| replayed_sample.state.phase == NavigationPhase::OffRoute)
        .expect("the recovery trace freezes");
    let recovery_index = outcome
        .sample_states
        .iter()
        .skip(first_off_route_index)
        .position(|replayed_sample| replayed_sample.state.phase == NavigationPhase::Navigating)
        .expect("the recovery trace recovers")
        + first_off_route_index;
    assert!(
        first_off_route_index > 0,
        "the excursion starts after on-route fixes"
    );
    let last_on_route_state = &outcome.sample_states[first_off_route_index - 1].state;
    for frozen_sample in &outcome.sample_states[first_off_route_index..recovery_index] {
        assert_eq!(frozen_sample.state.progress, last_on_route_state.progress);
        assert_eq!(
            frozen_sample.state.snapped_coordinate,
            last_on_route_state.snapped_coordinate
        );
        assert_eq!(
            frozen_sample.state.current_step_position,
            last_on_route_state.current_step_position
        );
    }

    // The recovering fix re-anchors the projection: the first navigating
    // state after the freeze snaps from the recovering fix again (that fix
    // re-entered the deviation allowance — threshold widened additively by
    // its accuracy — so the re-anchor is at most that allowance from the raw
    // fix), and progress measures the route again.
    let recovery_sample = &outcome.sample_states[recovery_index];
    let recovery_fix_coordinate = fixture.samples()[recovery_index].coordinate;
    let recovery_snapped_coordinate = recovery_sample
        .state
        .snapped_coordinate
        .expect("the recovery re-anchors the projection");
    let recovery_allowance_meters =
        fixture.deviation_threshold_meters() + fixture.sample_accuracy_meters();
    assert!(
        recovery_fix_coordinate.haversine_distance_meters(&recovery_snapped_coordinate)
            <= recovery_allowance_meters + REPLAY_PLANAR_TOLERANCE_METERS,
        "the re-anchored projection stays within the recovery allowance of the \
         recovering fix"
    );
    assert!(recovery_sample.state.deviation.is_none());

    // The recovered walk completes the trip.
    assert_eq!(
        outcome
            .sample_states
            .last()
            .expect("walked samples")
            .state
            .phase,
        NavigationPhase::Arrived
    );
}

#[test]
fn reroute_trace_rebuilds_the_trip_on_the_replacement_route() {
    let fixture = load_trace_fixture(REROUTE_TRACE_SCENARIO);
    let replacement_route = fixture
        .reroute_route()
        .expect("the reroute trace carries its replacement route");
    let outcome = run_trace_replay(&fixture, None);

    // The phase topic records exactly the flag, the reroute, the lift of the
    // frozen deviation and the arrival on the new route.
    assert_eq!(
        outcome.phase_changes,
        vec![
            NavigationPhaseChange::OffRouteEntered,
            NavigationPhaseChange::RerouteApplied,
            NavigationPhaseChange::OffRouteCleared,
            NavigationPhaseChange::Arrived,
        ]
    );

    // The deviation topic records the excursion and its lift, once each.
    assert_eq!(outcome.deviation_changes.len(), 2);
    let first_deviation_measurement = match &outcome.deviation_changes[0] {
        DeviationStatusChange::OffRoute(deviation_info) => {
            deviation_info.deviation_from_route_meters
        }
        other => panic!("expected the off-route judgment, got {other:?}"),
    };
    assert!(
        (first_deviation_measurement - EXCURSION_METERS).abs()
            <= MAXIMUM_FIX_NOISE_METERS + REPLAY_PLANAR_TOLERANCE_METERS
    );
    assert_eq!(outcome.deviation_changes[1], DeviationStatusChange::OnRoute);

    // The off-route marker is delivered exactly for the deviating fix's
    // dispatch (the reroute already rebuilt the session).
    let off_route_delivery_count = outcome
        .instructions
        .iter()
        .filter(|instruction| instruction.is_rerouting_required)
        .count();
    assert_eq!(
        off_route_delivery_count, 1,
        "the frozen off-route instruction is delivered exactly once before the reroute"
    );

    // From the reroute on, every replayed sample navigates the replacement
    // route with the deviation lifted…
    let first_rerouted_sample_index = outcome
        .sample_states
        .iter()
        .position(|replayed_sample| !replayed_sample.is_on_initial_route)
        .expect("the reroute trace reroutes");
    for rerouted_sample in &outcome.sample_states[first_rerouted_sample_index..] {
        assert_eq!(&rerouted_sample.state.route, replacement_route);
        assert!(rerouted_sample.state.deviation.is_none());
    }

    // …and the replacement route honors both recorded-geometry contracts: it
    // starts at the reducer's frozen snap from the deviating fix, and it ends
    // at the original recorded route's own destination.
    let off_route_state = &outcome.sample_states[first_rerouted_sample_index - 1].state;
    assert_eq!(off_route_state.phase, NavigationPhase::OffRoute);
    let reroute_anchor_coordinate = off_route_state
        .snapped_coordinate
        .expect("the frozen off-route state keeps its last on-route snap");
    let replacement_first_coordinate = replacement_route
        .legs
        .last()
        .and_then(|leg| leg.steps.first())
        .and_then(|step| step.shape.first())
        .expect("the replacement route has geometry");
    assert!(
        reroute_anchor_coordinate.haversine_distance_meters(replacement_first_coordinate)
            <= REPLAY_NEAR_ZERO_TOLERANCE_METERS,
        "the replacement route must start at the reducer's exact frozen snapped coordinate"
    );
    let replacement_last_coordinate = replacement_route
        .legs
        .last()
        .and_then(|leg| leg.steps.last())
        .and_then(|step| step.shape.last())
        .expect("the replacement route has geometry");
    let recorded_last_coordinate = fixture
        .route()
        .legs
        .last()
        .and_then(|leg| leg.steps.last())
        .and_then(|step| step.shape.last())
        .expect("the recorded route has geometry");
    assert_eq!(
        replacement_last_coordinate, recorded_last_coordinate,
        "the replacement route must end at the original recorded destination"
    );

    // …whose recorded steps the walker traverses in order, to arrival.
    let replacement_step_count: usize = replacement_route
        .legs
        .iter()
        .map(|leg| leg.steps.len())
        .sum();
    let mut replacement_visited_steps = vec![0_usize];
    for rerouted_sample in &outcome.sample_states[first_rerouted_sample_index..] {
        let step_position = rerouted_sample.state.current_step_position;
        if *replacement_visited_steps.last().expect("seeded") != step_position {
            replacement_visited_steps.push(step_position);
        }
    }
    assert_eq!(
        replacement_visited_steps,
        (0..replacement_step_count).collect::<Vec<usize>>(),
        "the replacement route's recorded steps are traversed in order"
    );
    assert_eq!(
        outcome
            .sample_states
            .last()
            .expect("walked samples")
            .state
            .phase,
        NavigationPhase::Arrived,
        "the rerouted trip arrives on the new route"
    );

    // Progress continues coherently on the new route: within each step of the
    // new route the remaining distance never grows beyond the recorded noise.
    for replayed_pair in outcome.sample_states.windows(2) {
        let (previous_sample, next_sample) = (&replayed_pair[0], &replayed_pair[1]);
        let is_same_epoch = previous_sample.is_on_initial_route == next_sample.is_on_initial_route;
        let is_same_step =
            previous_sample.state.current_step_position == next_sample.state.current_step_position;
        if is_same_epoch && is_same_step {
            assert!(
                next_sample.state.progress.distance_remaining_meters
                    <= previous_sample.state.progress.distance_remaining_meters
                        + REPLAY_MONOTONICITY_TOLERANCE_METERS
            );
        }
    }
}

#[test]
fn broadcast_replay_delivers_topics_in_recorded_order_and_cancellation_halts_a_topic() {
    let fixture = load_trace_fixture(REROUTE_TRACE_SCENARIO);
    let outcome = run_trace_replay(&fixture, Some(REPLAY_CANCELLATION_DISPATCH_INDEX));

    // The full instruction stream: the trip start, one per recorded fix, plus
    // the reroute's own dispatch. Exactly the cancelled cutoff's worth went to
    // the second subscription before its cancel took hold.
    assert_eq!(
        outcome.instructions.len(),
        1 + fixture.samples().len() + 1,
        "the trip start, every fix dispatch and the reroute dispatch deliver"
    );
    assert_eq!(
        outcome.cancelled_instruction_delivery_count, REPLAY_CANCELLATION_DISPATCH_INDEX,
        "the cancelled subscription received exactly the dispatches before its cancel"
    );

    // The topic delivery order along the whole recorded replay is the one the
    // reducer's semantics produce.
    assert_eq!(
        outcome.phase_changes,
        vec![
            NavigationPhaseChange::OffRouteEntered,
            NavigationPhaseChange::RerouteApplied,
            NavigationPhaseChange::OffRouteCleared,
            NavigationPhaseChange::Arrived,
        ]
    );
    assert_eq!(outcome.deviation_changes.len(), 2);
    assert_eq!(outcome.deviation_changes[1], DeviationStatusChange::OnRoute);
}

// ---------------------------------------------------------------------------
// Mathematical consistency invariants (round-14 owner finding: "the simulated
// drive instructions still seem off compared to where the location indicator
// is"). Every distance figure any guidance channel displays is anchored on the
// reducer's projection, so these tests reconcile the displayed numbers against
// the route geometry independently of the reducer's own arithmetic.
// ---------------------------------------------------------------------------

/// Tolerance, in meters, for recomputing a progress figure the reducer itself
/// derived by the same haversine arithmetic: the reconciliation must land on
/// the displayed value up to floating-point residue only.
const REPLAY_PROGRESS_RECOMPUTATION_TOLERANCE_METERS: f64 = 0.01;

/// Tolerance, in seconds, for recomputing the progress card's duration
/// figures (same arithmetic as the reducer, so floating-point residue only).
const REPLAY_PROGRESS_RECOMPUTATION_TOLERANCE_SECONDS: f64 = 0.01;

/// Tolerance, in seconds, of the fabricated wall-clock spacing the speech
/// assertions replay the planner with: it deliberately exceeds the inter
/// utterance spacing gate so the band semantics are asserted unmasked.
const REPLAY_SPEECH_SYNTHETIC_SPACING_SECONDS: f64 = 10.0 * MINIMUM_INTER_UTTERANCE_SPACING_SECONDS;

/// Tolerance, in meters, between a speech trigger distance and the progress
/// anchor it rides: the planner reads the state's field directly, so the
/// reconciliation may only carry floating-point residue.
const REPLAY_SPEECH_ANCHOR_TOLERANCE_METERS: f64 = 0.01;

/// The delivery index, in the replay's instruction stream, of the payload
/// dispatched for `sample_index`: the trip start's payload at index 0, then
/// one payload per recorded fix, plus — on the reroute trace only — the
/// reroute-applied payload once the trace's single deviating fix has been
/// dispatched in its own delivery.
fn replay_delivery_index(
    fixture: &NavigationTraceFixture,
    outcome: &TraceReplayOutcome,
    sample_index: usize,
) -> usize {
    let deviating_sample_index = if fixture.reroute_route().is_some() {
        Some(
            outcome
                .sample_states
                .iter()
                .position(|replayed_sample| {
                    replayed_sample.state.phase == NavigationPhase::OffRoute
                })
                .expect("the reroute trace flags exactly the deviation it answers"),
        )
    } else {
        None
    };
    1 + sample_index
        + usize::from(
            deviating_sample_index
                .is_some_and(|deviating_sample_index| sample_index > deviating_sample_index),
        )
}

/// Every displayed distance rides the projected route geometry: the banner's
/// countdown anchor is the snapped position's distance to the active step's
/// end, the progress card is that anchor plus the following declared steps,
/// and the raw fix (the raw-fix surfaces' indicator anchor) sits close enough
/// to the snap that both readings agree within the advance window's envelope.
#[test]
fn displayed_distances_reconcile_with_the_route_geometry_from_both_anchor_surfaces() {
    for scenario_name in ALL_TRACE_SCENARIOS {
        let fixture = load_trace_fixture(scenario_name);
        let outcome = run_trace_replay(&fixture, None);

        let mut anchor_states = vec![&outcome.initial_state];
        anchor_states.extend(
            outcome
                .sample_states
                .iter()
                .map(|replayed_sample| &replayed_sample.state),
        );

        for anchor_state in anchor_states {
            if anchor_state.phase == NavigationPhase::Arrived {
                // The arrived state reports the contract's zeroed progress
                // against the final step; the counting-down invariants cover
                // the guidance phases.
                continue;
            }
            let current_step = anchor_state.current_step().expect("replaying step");
            let terminal_coordinate = current_step.shape.last().expect("a step has a shape");

            // The banner anchor: the snapped position's distance to the step's
            // end, recomputed from the payload coordinates alone.
            if let Some(snapped_coordinate) = anchor_state.snapped_coordinate {
                assert!(
                    (anchor_state.progress.distance_to_next_maneuver_meters
                        - snapped_coordinate.haversine_distance_meters(terminal_coordinate))
                    .abs()
                        <= REPLAY_PROGRESS_RECOMPUTATION_TOLERANCE_METERS,
                    "{scenario_name}: the banner countdown must anchor on the displayed snap"
                );
            }

            // The progress card: the anchor plus the following declared steps.
            let following_steps_distance_meters: f64 = anchor_state
                .step_positions
                .iter()
                .enumerate()
                .filter(|(position, _)| *position > anchor_state.current_step_position)
                .filter_map(|(_, step_position)| anchor_state.step_for_position(step_position))
                .map(|step| step.distance_meters)
                .sum();
            assert!(
                (anchor_state.progress.distance_remaining_meters
                    - (anchor_state.progress.distance_to_next_maneuver_meters
                        + following_steps_distance_meters))
                    .abs()
                    <= REPLAY_PROGRESS_RECOMPUTATION_TOLERANCE_METERS,
                "{scenario_name}: the progress card distance must equal the banner anchor plus the following declared steps"
            );

            let traveled_fraction = (anchor_state.progress.distance_to_next_maneuver_meters
                / current_step.distance_meters)
                .clamp(0.0, 1.0);
            let following_steps_duration_seconds: f64 = anchor_state
                .step_positions
                .iter()
                .enumerate()
                .filter(|(position, _)| *position > anchor_state.current_step_position)
                .filter_map(|(_, step_position)| anchor_state.step_for_position(step_position))
                .map(|step| step.duration_seconds)
                .sum();
            assert!(
                (anchor_state.progress.duration_remaining_seconds
                    - (current_step.duration_seconds * traveled_fraction
                        + following_steps_duration_seconds))
                    .abs()
                    <= REPLAY_PROGRESS_RECOMPUTATION_TOLERANCE_SECONDS,
                "{scenario_name}: the progress card duration must match the linear in-step scale"
            );
        }

        // The raw-fix reconciliation is indexed by the recorded fix (the
        // initial state has none).
        for (sample_index, (replayed_sample, trace_sample)) in outcome
            .sample_states
            .iter()
            .zip(fixture.samples())
            .enumerate()
        {
            let state = &replayed_sample.state;
            if state.phase != NavigationPhase::Navigating {
                // OffRoute freezes against the moving raw fix, and arrival has
                // stopped counting down; the guidance-phase samples carry the
                // countdown invariants.
                continue;
            }
            let current_step = state.current_step().expect("replaying step");
            let terminal_coordinate = current_step.shape.last().expect("a step has a shape");
            let raw_to_terminal_meters = trace_sample
                .coordinate
                .haversine_distance_meters(terminal_coordinate);
            let raw_surface_disagreement_meters =
                (raw_to_terminal_meters - state.progress.distance_to_next_maneuver_meters).abs();
            assert!(
                raw_surface_disagreement_meters <= REPLAY_SNAP_ENVELOPE_METERS,
                "{scenario_name}: sample {sample_index} — the raw-fix surface's implied countdown must agree with the banner within the advance envelope"
            );
        }
    }
}

/// The delivered banner phrases agree with the progress anchor under the
/// metric band rules, the frozen off-route payload is suppressed, and the
/// then-line is the following step's text (without a distance).
#[test]
fn banner_phrases_match_the_metric_bands_and_the_then_line_carries_the_following_step() {
    for scenario_name in ALL_TRACE_SCENARIOS {
        let fixture = load_trace_fixture(scenario_name);
        let outcome = run_trace_replay(&fixture, None);

        for (sample_index, replayed_sample) in outcome.sample_states.iter().enumerate() {
            let delivered =
                &outcome.instructions[replay_delivery_index(&fixture, &outcome, sample_index)];
            let state = &replayed_sample.state;
            if state.phase == NavigationPhase::OffRoute {
                assert!(
                    delivered.active_maneuver.distance_phrase == DistancePhrase::Suppressed
                        && delivered.is_rerouting_required,
                    "{scenario_name}: sample {sample_index} — the frozen off-route payload must be the suppressed rerouting-required marker"
                );
                continue;
            }
            // The delivered banner phrase re-derives from the same progress
            // anchor the state carries, under the metric bands.
            let recomputed_phrase = DistancePhrase::for_distance(
                state.progress.distance_to_next_maneuver_meters,
                REPLAY_UNIT_SYSTEM,
            );
            assert_eq!(
                delivered.active_maneuver.distance_phrase, recomputed_phrase,
                "{scenario_name}: sample {sample_index} — the banner phrase must re-derive from the displayed countdown"
            );
            if let DistancePhrase::Quantity {
                magnitude,
                unit: DistanceUnit::Meters,
            } = &delivered.active_maneuver.distance_phrase
            {
                let rounding_shift_meters =
                    (magnitude - state.progress.distance_to_next_maneuver_meters).abs();
                assert!(
                    rounding_shift_meters <= REPLAY_PHRASE_ROUNDING_TOLERANCE_METERS,
                    "{scenario_name}: sample {sample_index} — the phrase must round the displayed countdown, not drift from it (shift {rounding_shift_meters})"
                );
            }
            // The then-line is the following step's text, as the following
            // step is present; on the final step there is no then-line.
            let following_maneuver = delivered.upcoming_maneuver.as_ref();
            let following_step = state.step_at(state.current_step_position + 1);
            assert_eq!(
                following_maneuver.is_some(),
                following_step.is_some(),
                "{scenario_name}: sample {sample_index} — the then-line must exist exactly when a following step does"
            );
            if let (Some(following_maneuver), Some(following_step)) =
                (following_maneuver, following_step)
            {
                let ManeuverInstructionText::Composed {
                    display_text: upcoming_display_text,
                    ..
                } = &following_maneuver.instruction
                else {
                    panic!("the fixture's following step is provider-authored");
                };
                assert_eq!(
                    upcoming_display_text, &following_step.instruction_display_text,
                    "{scenario_name}: sample {sample_index} — the then-line must carry the following step's text verbatim"
                );
            }
        }
    }
}

/// Step flips are punctual: within one route epoch, the first fix whose raw
/// straight-line distance to the active step's end falls inside the eager
/// advance threshold is exactly the fix the step flips on — no earlier fix
/// could have triggered, and the flipped-to step is the very next one.
///
/// The accuracy gate never binds here (the generator records 8 m accuracy
/// against the 15 m gate), so the raw-distance reading alone derives the
/// schedule.
#[test]
fn step_flips_occur_exactly_at_the_eager_advance_threshold_within_each_epoch() {
    for scenario_name in ALL_TRACE_SCENARIOS {
        let fixture = load_trace_fixture(scenario_name);
        let outcome = run_trace_replay(&fixture, None);

        let mut epoch_sample_range_start_index = 0_usize;
        let mut previous_flip_sample_index = 0_usize;
        let mut previous_step_position = outcome.initial_state.current_step_position;
        let mut previous_is_on_initial_route = outcome.sample_states[0].is_on_initial_route;
        for (sample_index, replayed_sample) in outcome.sample_states.iter().enumerate() {
            let is_epoch_boundary =
                replayed_sample.is_on_initial_route != previous_is_on_initial_route;
            if is_epoch_boundary {
                epoch_sample_range_start_index = sample_index;
                previous_flip_sample_index = sample_index;
                previous_step_position = replayed_sample.state.current_step_position;
                previous_is_on_initial_route = replayed_sample.is_on_initial_route;
                continue;
            }
            let is_flip = replayed_sample.state.current_step_position != previous_step_position;
            if is_flip {
                // The advance loop walks exactly one step per fix on these
                // traces: a multi-step jump would mean a step fell entirely
                // inside the threshold window, which the 10 m sampling and the
                // declared step lengths forbid.
                assert_eq!(
                    replayed_sample.state.current_step_position,
                    previous_step_position + 1,
                    "{scenario_name}: sample {sample_index} — the step flip must advance exactly one step"
                );
                for between_sample_index in
                    previous_flip_sample_index.max(epoch_sample_range_start_index)..sample_index
                {
                    assert_between_sample_could_not_advance(
                        &fixture,
                        &outcome,
                        previous_step_position,
                        between_sample_index,
                        scenario_name,
                    );
                }
                assert_flip_sample_is_at_or_inside_the_threshold(
                    &fixture,
                    &outcome,
                    sample_index,
                    scenario_name,
                );
                previous_flip_sample_index = sample_index;
                previous_step_position = replayed_sample.state.current_step_position;
            }
        }
    }
}

/// Asserts that a recorded fix that stayed on the step `expected_step_position`
/// could not have triggered the eager advance on it: its raw straight-line
/// distance to that step's end sits outside the threshold.
fn assert_between_sample_could_not_advance(
    fixture: &NavigationTraceFixture,
    outcome: &TraceReplayOutcome,
    expected_step_position: usize,
    between_sample_index: usize,
    scenario_name: &str,
) {
    let replayed_sample = &outcome.sample_states[between_sample_index];
    assert_eq!(
        replayed_sample.state.current_step_position, expected_step_position,
        "{scenario_name}: sample {between_sample_index} — the between-fixes ledger must be consistent"
    );
    let step = replayed_sample
        .state
        .current_step()
        .expect("replaying step");
    let terminal_coordinate = step.shape.last().expect("a step has a shape");
    let raw_to_terminal_meters = fixture.samples()[between_sample_index]
        .coordinate
        .haversine_distance_meters(terminal_coordinate);
    assert!(
        raw_to_terminal_meters > DEFAULT_DISTANCE_TO_END_OF_STEP_THRESHOLD_METERS,
        "{scenario_name}: sample {between_sample_index} — a fix still {raw_to_terminal_meters} m from the step end must not have advanced (threshold {} m)",
        DEFAULT_DISTANCE_TO_END_OF_STEP_THRESHOLD_METERS
    );
}

/// Asserts that the recorded fix a step flipped on sits inside the eager
/// advance threshold, measured as the condition measures it: the raw fix's
/// straight-line distance to the active step's end.
fn assert_flip_sample_is_at_or_inside_the_threshold(
    fixture: &NavigationTraceFixture,
    outcome: &TraceReplayOutcome,
    flip_sample_index: usize,
    scenario_name: &str,
) {
    let flip_sample = &outcome.sample_states[flip_sample_index];
    let previous_step = flip_sample
        .state
        .step_at(flip_sample.state.current_step_position - 1)
        .expect("the flipped-from step exists");
    let terminal_coordinate = previous_step.shape.last().expect("a step has a shape");
    let raw_to_terminal_meters = fixture.samples()[flip_sample_index]
        .coordinate
        .haversine_distance_meters(terminal_coordinate);
    assert!(
        raw_to_terminal_meters <= DEFAULT_DISTANCE_TO_END_OF_STEP_THRESHOLD_METERS,
        "{scenario_name}: sample {flip_sample_index} — the flip must occur inside the threshold (raw distance {raw_to_terminal_meters} m)"
    );
}

/// The speech channel rides the same progress anchor every other channel
/// displays from, its utterance ids increase monotonically across reroute
/// resets, and it voices the corrected end-anchored pairing: a step's text
/// is the provider's end-anchored text (the maneuver at the step's end —
/// the same mapping the trace fixtures author), so every approach line
/// speaks the maneuver the countdown points at in words, not the street
/// already being walked.
///
/// The planner is driven with a fabricated wall clock spaced far beyond the
/// inter-utterance spacing gate, so the band semantics assert unmasked here
/// (the accelerated platform demos dispatch real instants ~0.25 s apart, and
/// their spacing gate eats the follow-up bands — a documented pacing fact).
#[test]
fn speech_utterances_ride_the_progress_anchor_and_voice_the_active_step() {
    for scenario_name in ALL_TRACE_SCENARIOS {
        let fixture = load_trace_fixture(scenario_name);
        let outcome = run_trace_replay(&fixture, None);

        let mut speech_planner = crate::speech::SpeechInstructionPlanner::new();
        let mut fabricated_instant = std::time::Instant::now();
        let mut previous_state = outcome.initial_state.clone();
        let mut emitted_utterances: Vec<(usize, crate::speech::SpeechUtterance)> = Vec::new();
        let mut previous_utterance_id: Option<u64> = None;
        for (sample_index, replayed_sample) in outcome.sample_states.iter().enumerate() {
            fabricated_instant +=
                std::time::Duration::from_secs_f64(REPLAY_SPEECH_SYNTHETIC_SPACING_SECONDS);
            for utterance in speech_planner.evaluate_state_transition(
                Some(&previous_state),
                &replayed_sample.state,
                REPLAY_UNIT_SYSTEM,
                fabricated_instant,
            ) {
                if utterance.kind != SpeechUtteranceKind::Arrival {
                    // The spoken countdown reads the exact field every other
                    // channel displays from: the displayed progress anchor.
                    let emitting_state = &replayed_sample.state;
                    let anchor_shift_meters = (utterance.trigger_distance_meters
                        - emitting_state.progress.distance_to_next_maneuver_meters)
                        .abs();
                    assert!(
                        anchor_shift_meters <= REPLAY_SPEECH_ANCHOR_TOLERANCE_METERS,
                        "{scenario_name}: sample {sample_index} — the spoken countdown must ride the displayed progress anchor (shift {anchor_shift_meters})"
                    );
                }
                if let Some(previous_utterance_id) = previous_utterance_id {
                    assert!(
                        utterance.utterance_id > previous_utterance_id,
                        "{scenario_name}: sample {sample_index} — utterance ids must increase monotonically ({} then {})",
                        previous_utterance_id,
                        utterance.utterance_id
                    );
                }
                previous_utterance_id = Some(utterance.utterance_id);
                emitted_utterances.push((sample_index, utterance));
            }
            previous_state = replayed_sample.state.clone();
        }

        if scenario_name == CLEAN_TRACE_SCENARIO {
            assert_eq!(
                emitted_utterances.len(),
                2,
                "{scenario_name}: the clean walk voices exactly one approach line and the arrival"
            );
            let (approach_sample_index, approach_utterance) = &emitted_utterances[0];
            assert_eq!(
                approach_utterance.kind,
                SpeechUtteranceKind::ManeuverApproach
            );
            assert!(
                approach_utterance.trigger_distance_meters <= REPLAY_METRIC_NEAR_BAND_UPPER_METERS
                    && approach_utterance.trigger_distance_meters
                        > DEFAULT_DISTANCE_TO_END_OF_STEP_THRESHOLD_METERS,
                "{scenario_name}: the approach band crossing must sit between the eager advance threshold (which pre-empts the tight bands) and the 100 m band"
            );
            // The corrected pairing: the spoken line is the banner — the
            // active step's text — and that text is END-anchored by the
            // fixture generator (the maneuver at the step's end), so the
            // spoken approach line names the very maneuver the countdown
            // points at. The clean walk's single approach line rides the
            // first step inside the near band, so it speaks the turn at
            // that step's end, one maneuver ahead of the street being
            // walked — not the departed maneuver's text.
            let delivered = &outcome.instructions
                [replay_delivery_index(&fixture, &outcome, *approach_sample_index)];
            let ManeuverInstructionText::Composed {
                speech_text: banner_speech_text,
                ..
            } = &delivered.active_maneuver.instruction
            else {
                panic!("the fixture steps are always provider-authored");
            };
            assert_eq!(
                &approach_utterance.utterance_speech_text, banner_speech_text,
                "{scenario_name}: the spoken approach line must voice the banner text"
            );
            let recorded_first_step = &fixture.route().legs[0].steps[0];
            assert!(
                approach_utterance
                    .utterance_speech_text
                    .ends_with(&recorded_first_step.instruction_speech_text),
                "{scenario_name}: the spoken approach line must voice the recorded first \
                 step's provider text (the maneuver at the traversed step's end) beneath \
                 its live distance prefix, not synthesized prose: {}",
                approach_utterance.utterance_speech_text
            );
            let (arrival_sample_index, arrival_utterance) = &emitted_utterances[1];
            assert_eq!(arrival_utterance.kind, SpeechUtteranceKind::Arrival);
            // The arrival rides the fix where the final step's advance
            // fires; a trace may keep recording past arrival, so the
            // invariant is the first Arrived state, not the last sample.
            let first_arrived_sample_index = outcome
                .sample_states
                .iter()
                .position(|replayed_sample| replayed_sample.state.phase == NavigationPhase::Arrived)
                .expect("the clean walk arrives");
            assert_eq!(
                *arrival_sample_index, first_arrived_sample_index,
                "{scenario_name}: the arrival line must ride the arriving fix"
            );
        } else {
            assert!(
                emitted_utterances
                    .iter()
                    .any(|(_, utterance)| utterance.kind == SpeechUtteranceKind::OffRouteRecovery),
                "{scenario_name}: the recovery and reroute walks voice a resumption line"
            );
            let (_, arrival_utterance) =
                emitted_utterances.last().expect("the walk arrives, voiced");
            assert_eq!(arrival_utterance.kind, SpeechUtteranceKind::Arrival);
        }
    }
}

proptest! {
    #![proptest_config(
        ProptestConfig::with_cases(REPLAY_PROPERTY_TEST_CASE_COUNT)
    )]
    /// The corridor property along the recorded clean trace: a synthetic fix
    /// near the loaded route line (its along-fraction and noise are property
    /// inputs) never snaps further than the fix's accuracy allowance from its
    /// raw position — the same envelope the deterministic replay asserts
    /// sample by sample, explored here over a property grid.
    #[test]
    fn raw_fixes_near_the_clean_trace_route_snap_within_the_accuracy_envelope(
        along_fraction in 0.0_f64..=1.0,
        noise_bearing_radians in 0.0_f64..std::f64::consts::TAU,
        noise_distance_fraction in 0.0_f64..=1.0,
    ) {
        let fixture = load_trace_fixture(CLEAN_TRACE_SCENARIO);
        let route_line = flatten_route_line(fixture.route());
        // The along-fraction resolves through the recorded route's own planar
        // walk (the same walk the generator records fixes against); the fix is
        // then displaced radially by up to the recorded noise clamp, the worst
        // case the committed samples can express.
        let recorded_route_walk = RecordedRouteWalk::from_route(fixture.route());
        let along_meters =
            recorded_route_walk.walk.total_length_meters() * along_fraction;
        let (route_point, _, _) = recorded_route_walk.walk.position_at_along(along_meters);
        let noise_max_meters =
            noise_distance_fraction * MAXIMUM_FIX_NOISE_METERS;
        let fix_coordinate = planar_offset_coordinate(
            &recorded_route_walk.base_coordinate,
            route_point.0 + noise_max_meters * noise_bearing_radians.cos(),
            route_point.1 + noise_max_meters * noise_bearing_radians.sin(),
        );
        let snap = crate::snap_coordinate_to_polyline(&fix_coordinate, &route_line)
            .expect("the fixture route line is never empty");
        let fix_to_snap_meters = fix_coordinate
            .haversine_distance_meters(&snap.snapped_coordinate);
        prop_assert!(
            fix_to_snap_meters
                <= fixture.sample_accuracy_meters() + REPLAY_PLANAR_TOLERANCE_METERS,
            "a fix {noise_max_meters} m off the route line snapped {fix_to_snap_meters} m away"
        );
    }
}
