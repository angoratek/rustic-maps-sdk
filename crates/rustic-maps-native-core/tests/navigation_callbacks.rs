//! End-to-end tests of the navigation wiring (M6): a Rust-side
//! implementation of the exported `[Trait, WithForeign]`
//! `NavigationListener` callback contract — standing in for the
//! foreign (Kotlin/Swift) implementations the generated bindings would
//! produce — registers its subscriptions on `Navigator`, and
//! the controller's `start_navigation`, `update_location` and
//! `apply_reroute` calls drive the native `rustic-maps-navigation` reducer session
//! and dispatch into the listener's per-topic callbacks. A refused policy or
//! route record throws the navigation-input error and starts no trip.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use rustic_maps::{
    DeviationPolicy, DeviationPolicyKind, DeviationStatusChange, DeviationStatusChangeKind,
    DistancePhraseKind, NavigationInstruction, NavigationListener, NavigationLocationUpdate,
    NavigationPhase, NavigationPhaseChange, Navigator, Route, RouteLeg, RouteStep, SdkError,
    SpeechInstruction, StepAdvancePolicy, StepAdvancePolicyKind, Subscription, UnitSystem,
    default_deviation_policy, default_step_advance_policy,
};
use rustic_maps_navigation::{
    DEFAULT_DEVIATION_DISTANCE_THRESHOLD_METERS, DEFAULT_DISTANCE_FROM_STEP_THRESHOLD_METERS,
    DEFAULT_DISTANCE_TO_END_OF_STEP_THRESHOLD_METERS, DEFAULT_MINIMUM_HORIZONTAL_ACCURACY_METERS,
};

/// Latitude, in degrees, of the synthetic fixture origin (a neutral
/// mid-latitude where the equirectangular fixture math is negligible at test
/// scales, same convention as the navigation fixtures).
const TEST_ORIGIN_LATITUDE_DEGREES: f64 = 47.0;
/// Longitude, in degrees, of the synthetic fixture origin.
const TEST_ORIGIN_LONGITUDE_DEGREES: f64 = 8.0;
/// Earth's mean radius, in meters, provenance shared with the navigation
/// geometry module's own constant.
const EARTH_RADIUS_METERS: f64 = 6_371_008.8;
/// Length, in meters, of each synthetic step.
const TEST_STEP_LENGTH_METERS: f64 = 100.0;
/// Number of steps in the synthetic routes.
const TEST_STEP_COUNT: usize = 3;
/// Declared duration, in seconds, of each synthetic step.
const TEST_STEP_DURATION_SECONDS: f64 = 10.0;
/// Horizontal accuracy, in meters, of the synthetic walker's fixes: far
/// better than the default accuracy gate.
const TEST_ACCURACY_METERS: f64 = 2.0;
/// Spacing, in meters, between consecutive walker updates.
const WALKER_UPDATE_SPACING_METERS: f64 = 10.0;
/// Total length, in meters, of the synthetic route.
const TEST_ROUTE_TOTAL_LENGTH_METERS: f64 = TEST_STEP_LENGTH_METERS * TEST_STEP_COUNT as f64;
/// Lateral excursion, in meters, used to leave the synthetic route.
const TEST_OFF_ROUTE_EXCURSION_METERS: f64 = 100.0;
/// Length, in meters, of the single-step replacement route for reroutes.
const TEST_REROUTE_STEP_LENGTH_METERS: f64 = 150.0;
/// East offset, in meters, the reroute fixture anchors at (two full steps of
/// the synthetic route).
const TEST_REROUTE_ANCHOR_OFFSET_METERS: f64 = TEST_STEP_LENGTH_METERS * 2.0;
/// Step-advance threshold fixture value used by the manual-policy trips.
const MANUAL_POLICY_IGNORED_THRESHOLD_VALUE: f64 = 0.0;
/// Deviation-threshold fixture value refused by the controller boundary.
const REFUSED_NEGATIVE_THRESHOLD_METERS: f64 = -5.0;
/// Maneuver label the fixture route refuses.
const UNRECOGNIZED_MANEUVER_LABEL: &str = "sidestep";
/// Detail marker of the refused-threshold errors thrown at the boundary.
const REFUSED_THRESHOLD_DETAIL_FRAGMENT: &str = "must be finite and non-negative";
/// East offset, in meters, of the fix that crosses the near speech band: it
/// leaves 9 m to the first maneuver, under the 10 m band and above the 5 m
/// immediate band, so the crossing announces the approach line.
const NEAR_BAND_TRIGGER_FIX_EAST_METERS: f64 = 91.0;

/// A fixture coordinate displaced by east/north meter offsets around
/// [`TEST_ORIGIN_LATITUDE_DEGREES`]/[`TEST_ORIGIN_LONGITUDE_DEGREES`], the
/// same equirectangular conversion the navigation fixtures use.
fn east_north_coordinate(east_meters: f64, north_meters: f64) -> rustic_maps::GeographicCoordinate {
    rustic_maps::GeographicCoordinate {
        latitude: TEST_ORIGIN_LATITUDE_DEGREES + (north_meters / EARTH_RADIUS_METERS).to_degrees(),
        longitude: TEST_ORIGIN_LONGITUDE_DEGREES
            + (east_meters
                / (EARTH_RADIUS_METERS * TEST_ORIGIN_LATITUDE_DEGREES.to_radians().cos()))
            .to_degrees(),
    }
}

/// Builds the mirror record of the native `BoundingBox`.
fn spanning_bounding_box() -> rustic_maps::BoundingBox {
    rustic_maps::BoundingBox {
        south_west: east_north_coordinate(0.0, 0.0),
        north_east: east_north_coordinate(TEST_ROUTE_TOTAL_LENGTH_METERS, 0.0),
    }
}

/// Builds the mirror record of the native `RouteStep`.
fn route_step(
    start_offset_east_meters: f64,
    step_length_meters: f64,
    maneuver_label: &str,
) -> RouteStep {
    RouteStep {
        distance_meters: step_length_meters,
        duration_seconds: TEST_STEP_DURATION_SECONDS,
        maneuver_type: maneuver_label.to_string(),
        instruction_display_text: "Instruction".to_string(),
        instruction_speech_text: "Instruction".to_string(),
        shape: vec![
            east_north_coordinate(start_offset_east_meters, 0.0),
            east_north_coordinate(start_offset_east_meters + step_length_meters / 2.0, 0.0),
            east_north_coordinate(start_offset_east_meters + step_length_meters, 0.0),
        ],
    }
}

/// Builds the synthetic eastward route: three collinear steps of
/// [`TEST_STEP_LENGTH_METERS`], departure, continue, arrival, whose declared
/// distances match their geometric lengths.
fn straight_route() -> Route {
    let maneuver_labels = ["departure", "continue", "arrival"];
    let steps = (0..TEST_STEP_COUNT)
        .map(|step_number| {
            route_step(
                step_number as f64 * TEST_STEP_LENGTH_METERS,
                TEST_STEP_LENGTH_METERS,
                maneuver_labels[step_number],
            )
        })
        .collect();
    Route {
        distance_meters: TEST_ROUTE_TOTAL_LENGTH_METERS,
        duration_seconds: TEST_STEP_DURATION_SECONDS * TEST_STEP_COUNT as f64,
        bounding_box: spanning_bounding_box(),
        geometry: Vec::new(),
        legs: vec![RouteLeg {
            distance_meters: TEST_ROUTE_TOTAL_LENGTH_METERS,
            duration_seconds: TEST_STEP_DURATION_SECONDS * TEST_STEP_COUNT as f64,
            steps,
        }],
    }
}

/// Builds the single-step replacement route anchored exactly at
/// [`TEST_REROUTE_ANCHOR_OFFSET_METERS`], honoring the reroute anchoring
/// contract.
fn anchored_reroute_route() -> Route {
    let anchor_offset = TEST_REROUTE_ANCHOR_OFFSET_METERS;
    Route {
        distance_meters: TEST_REROUTE_STEP_LENGTH_METERS,
        duration_seconds: TEST_STEP_DURATION_SECONDS,
        bounding_box: rustic_maps::BoundingBox {
            south_west: east_north_coordinate(anchor_offset, 0.0),
            north_east: east_north_coordinate(anchor_offset + TEST_REROUTE_STEP_LENGTH_METERS, 0.0),
        },
        geometry: Vec::new(),
        legs: vec![RouteLeg {
            distance_meters: TEST_REROUTE_STEP_LENGTH_METERS,
            duration_seconds: TEST_STEP_DURATION_SECONDS,
            steps: vec![route_step(
                anchor_offset,
                TEST_REROUTE_STEP_LENGTH_METERS,
                "arrival",
            )],
        }],
    }
}

/// The policy record configuring manual-only step advance; the threshold
/// fields are ignored by the manual kind.
fn manual_step_advance_policy() -> StepAdvancePolicy {
    StepAdvancePolicy {
        policy_kind: StepAdvancePolicyKind::Manual,
        advance_threshold_meters: MANUAL_POLICY_IGNORED_THRESHOLD_VALUE,
        minimum_horizontal_accuracy_meters: MANUAL_POLICY_IGNORED_THRESHOLD_VALUE,
    }
}

/// A graded fix on the route line at `east_meters`.
fn route_fix(east_meters: f64) -> NavigationLocationUpdate {
    NavigationLocationUpdate {
        coordinate: east_north_coordinate(east_meters, 0.0),
        horizontal_accuracy_meters: Some(TEST_ACCURACY_METERS),
        course_heading_degrees: None,
        timestamp_seconds: None,
    }
}

/// A graded fix displaced `north_meters` north of the route line at
/// `east_meters`.
fn off_route_fix(east_meters: f64, north_meters: f64) -> NavigationLocationUpdate {
    NavigationLocationUpdate {
        coordinate: east_north_coordinate(east_meters, north_meters),
        horizontal_accuracy_meters: Some(TEST_ACCURACY_METERS),
        course_heading_degrees: None,
        timestamp_seconds: None,
    }
}

/// A listener recording every callback on every topic, standing in for the
/// foreign implementations.
#[derive(Default)]
struct RecordingListener {
    instruction_records: Mutex<Vec<NavigationInstruction>>,
    deviation_changes: Mutex<Vec<DeviationStatusChange>>,
    phase_changes: Mutex<Vec<NavigationPhaseChange>>,
    speech_instructions: Mutex<Vec<SpeechInstruction>>,
}

impl NavigationListener for RecordingListener {
    fn on_navigation_instruction(&self, instruction: NavigationInstruction) {
        self.instruction_records.lock().unwrap().push(instruction);
    }

    fn on_deviation_status_change(&self, deviation_status_change: DeviationStatusChange) {
        self.deviation_changes
            .lock()
            .unwrap()
            .push(deviation_status_change);
    }

    fn on_navigation_phase_change(&self, navigation_phase_change: NavigationPhaseChange) {
        self.phase_changes
            .lock()
            .unwrap()
            .push(navigation_phase_change);
    }

    fn on_speech_instruction(&self, speech_instruction: SpeechInstruction) {
        self.speech_instructions
            .lock()
            .unwrap()
            .push(speech_instruction);
    }
}

/// A listener driving one bounded re-entrant location update inside its own
/// instruction callback, mirroring the foreign listener that owns its own fix
/// pump.
struct ReentrantDrivingListener {
    controller: Arc<Navigator>,
    remaining_drive_budget: AtomicUsize,
    observed_dispatches: AtomicUsize,
}

impl NavigationListener for ReentrantDrivingListener {
    fn on_navigation_instruction(&self, _instruction: NavigationInstruction) {
        let remaining_drive_budget = self.remaining_drive_budget.load(Ordering::SeqCst);
        if remaining_drive_budget == 0 {
            return;
        }
        self.remaining_drive_budget
            .store(remaining_drive_budget - 1, Ordering::SeqCst);
        // The nested drive happens while the outer dispatch is still in
        // flight.
        self.controller
            .update_location(route_fix(TEST_REROUTE_ANCHOR_OFFSET_METERS / 2.0));
        self.observed_dispatches.fetch_add(1, Ordering::SeqCst);
    }

    fn on_deviation_status_change(&self, _deviation_status_change: DeviationStatusChange) {}

    fn on_navigation_phase_change(&self, _navigation_phase_change: NavigationPhaseChange) {}

    fn on_speech_instruction(&self, _speech_instruction: SpeechInstruction) {}
}

/// Subscribes the recording listener to all three topics and keeps every
/// subscription alive for the test's duration.
fn subscribe_to_all_topics(
    controller: &Navigator,
    listener: &Arc<RecordingListener>,
) -> [Arc<Subscription>; 3] {
    [
        controller.subscribe_to_navigation_instructions(
            Arc::clone(listener) as Arc<dyn NavigationListener>
        ),
        controller
            .subscribe_to_deviation_status(Arc::clone(listener) as Arc<dyn NavigationListener>),
        controller.subscribe_to_phase_changes(Arc::clone(listener) as Arc<dyn NavigationListener>),
    ]
}

/// Walks the controller along the synthetic route with evenly spaced fixes.
fn walk_controller(controller: &Navigator) {
    let total_updates_for_walker =
        (TEST_ROUTE_TOTAL_LENGTH_METERS / WALKER_UPDATE_SPACING_METERS) as usize;
    for update_number in 1..=total_updates_for_walker {
        controller.update_location(route_fix(
            update_number as f64 * WALKER_UPDATE_SPACING_METERS,
        ));
    }
}

#[test]
fn start_navigation_dispatches_the_initial_instruction_and_the_state_is_readable() {
    let controller = Navigator::new();
    let listener = Arc::new(RecordingListener::default());
    let _subscriptions = subscribe_to_all_topics(&controller, &listener);

    controller
        .start_navigation(
            straight_route(),
            UnitSystem::Metric,
            default_step_advance_policy(),
            default_deviation_policy(),
        )
        .expect("the fixture route and policies must start");

    let trip_state = controller
        .current_trip_state()
        .expect("a started trip must report its state");
    assert_eq!(trip_state.phase, NavigationPhase::Navigating);
    assert_eq!(trip_state.current_step_position, 0);
    assert_eq!(trip_state.step_positions.len(), TEST_STEP_COUNT);
    assert!(trip_state.snapped_coordinate.is_none());
    assert!(trip_state.deviation.is_none());
    assert!(
        (trip_state.progress.distance_remaining_meters - TEST_ROUTE_TOTAL_LENGTH_METERS).abs()
            < 1e-6
    );

    // The initial dispatch carries the instruction payload only.
    let delivered_instructions = listener.instruction_records.lock().unwrap();
    assert_eq!(delivered_instructions.len(), 1);
    let initial_instruction = &delivered_instructions[0];
    assert!(!initial_instruction.is_arrival);
    assert!(!initial_instruction.is_rerouting_required);
    assert_eq!(
        initial_instruction
            .active_maneuver
            .distance_phrase
            .phrase_kind,
        DistancePhraseKind::Quantity
    );
    assert!(
        (initial_instruction
            .active_maneuver
            .distance_phrase
            .magnitude
            .expect("the initial phrase names the full first step")
            - TEST_STEP_LENGTH_METERS)
            .abs()
            < 1e-6
    );
    assert!(listener.deviation_changes.lock().unwrap().is_empty());
    assert!(listener.phase_changes.lock().unwrap().is_empty());
}

#[test]
fn default_policies_expose_the_native_defaults() {
    let default_step_policy = default_step_advance_policy();
    assert_eq!(
        default_step_policy.policy_kind,
        StepAdvancePolicyKind::DistanceToEndOfStep
    );
    assert_eq!(
        default_step_policy.advance_threshold_meters,
        DEFAULT_DISTANCE_TO_END_OF_STEP_THRESHOLD_METERS
    );
    assert_eq!(
        default_step_policy.minimum_horizontal_accuracy_meters,
        DEFAULT_MINIMUM_HORIZONTAL_ACCURACY_METERS
    );

    let default_deviation_policy_record = default_deviation_policy();
    assert_eq!(
        default_deviation_policy_record.policy_kind,
        DeviationPolicyKind::DistanceThreshold
    );
    assert_eq!(
        default_deviation_policy_record.deviation_threshold_meters,
        DEFAULT_DEVIATION_DISTANCE_THRESHOLD_METERS
    );
    // The named constants are genuinely the shipped thresholds, not invented
    // values: sanity against the constants this surface documents.
    assert_eq!(DEFAULT_DISTANCE_FROM_STEP_THRESHOLD_METERS, 15.0);
}

#[test]
fn walker_fixes_advance_through_the_route_and_arrive() {
    let controller = Navigator::new();
    let listener = Arc::new(RecordingListener::default());
    let _subscriptions = subscribe_to_all_topics(&controller, &listener);

    controller
        .start_navigation(
            straight_route(),
            UnitSystem::Metric,
            default_step_advance_policy(),
            default_deviation_policy(),
        )
        .expect("the fixture route and policies must start");
    walk_controller(&controller);

    let trip_state = controller
        .current_trip_state()
        .expect("the walked trip must still report its state");
    assert_eq!(trip_state.phase, NavigationPhase::Arrived);

    // Every dispatched state change — the trip start plus every walker
    // update — delivered its instruction payload; the phase topic saw only
    // the arrival; no deviation was ever judged.
    let total_updates_for_walker =
        (TEST_ROUTE_TOTAL_LENGTH_METERS / WALKER_UPDATE_SPACING_METERS) as usize;
    assert_eq!(
        listener.instruction_records.lock().unwrap().len(),
        total_updates_for_walker + 1
    );
    assert_eq!(
        *listener.phase_changes.lock().unwrap(),
        vec![NavigationPhaseChange::Arrived]
    );
    assert!(listener.deviation_changes.lock().unwrap().is_empty());
}

#[test]
fn deviation_excursion_flags_then_clears_across_the_topics() {
    let controller = Navigator::new();
    let listener = Arc::new(RecordingListener::default());
    let _subscriptions = subscribe_to_all_topics(&controller, &listener);

    controller
        .start_navigation(
            straight_route(),
            UnitSystem::Metric,
            manual_step_advance_policy(),
            default_deviation_policy(),
        )
        .expect("the fixture route and policies must start");
    controller.update_location(route_fix(50.0));
    controller.update_location(off_route_fix(50.0, TEST_OFF_ROUTE_EXCURSION_METERS));

    let deviated_trip_state = controller
        .current_trip_state()
        .expect("the deviated trip must still report its state");
    assert_eq!(deviated_trip_state.phase, NavigationPhase::OffRoute);
    let deviation = deviated_trip_state
        .deviation
        .expect("an off-route trip must record its deviation");
    assert!((deviation.deviation_from_route_meters - TEST_OFF_ROUTE_EXCURSION_METERS).abs() < 1e-6);
    assert!(
        controller
            .current_instructions()
            .expect("the deviated trip composes an instruction")
            .is_rerouting_required,
        "the off-route trip must mark its instruction not-presentable"
    );

    // The default detector's allowance widened by the fixture accuracy is
    // well below the fixture excursion, so the excursion flags off route.
    let effective_allowance_meters =
        DEFAULT_DEVIATION_DISTANCE_THRESHOLD_METERS + TEST_ACCURACY_METERS;
    assert!(TEST_OFF_ROUTE_EXCURSION_METERS > effective_allowance_meters);

    controller.update_location(route_fix(70.0));
    let recovered_trip_state = controller.current_trip_state().unwrap();
    assert_eq!(recovered_trip_state.phase, NavigationPhase::Navigating);
    assert!(recovered_trip_state.deviation.is_none());
    assert!(
        !controller
            .current_instructions()
            .expect("the recovered trip composes an instruction")
            .is_rerouting_required
    );

    // The deviation topic streamed the entry (its measurement is the
    // equirectangular-fixture excursion, compared up to haversine rounding)
    // and the clearing.
    let recorded_deviation_changes = listener.deviation_changes.lock().unwrap();
    assert_eq!(recorded_deviation_changes.len(), 2);
    assert_eq!(
        recorded_deviation_changes[0].status_change_kind,
        DeviationStatusChangeKind::OffRoute
    );
    let recorded_excursion_meters = recorded_deviation_changes[0]
        .deviation_info
        .expect("the off-route update carries its measurement")
        .deviation_from_route_meters;
    assert!((recorded_excursion_meters - TEST_OFF_ROUTE_EXCURSION_METERS).abs() < 1e-6);
    assert_eq!(
        recorded_deviation_changes[1].status_change_kind,
        DeviationStatusChangeKind::OnRoute
    );
    assert!(recorded_deviation_changes[1].deviation_info.is_none());
    let phase_changes_after_recovery = listener.phase_changes.lock().unwrap();
    assert_eq!(
        *phase_changes_after_recovery,
        vec![
            NavigationPhaseChange::OffRouteEntered,
            NavigationPhaseChange::OffRouteCleared
        ]
    );
}

#[test]
fn reroute_recovers_an_off_route_trip_and_reports_the_transition() {
    let controller = Navigator::new();
    let listener = Arc::new(RecordingListener::default());
    let _subscriptions = subscribe_to_all_topics(&controller, &listener);

    controller
        .start_navigation(
            straight_route(),
            UnitSystem::Metric,
            manual_step_advance_policy(),
            default_deviation_policy(),
        )
        .expect("the fixture route and policies must start");
    controller.update_location(route_fix(TEST_REROUTE_ANCHOR_OFFSET_METERS));
    controller.update_location(off_route_fix(
        TEST_REROUTE_ANCHOR_OFFSET_METERS,
        TEST_OFF_ROUTE_EXCURSION_METERS,
    ));
    assert_eq!(
        controller
            .current_trip_state()
            .expect("the deviated trip must still report its state")
            .phase,
        NavigationPhase::OffRoute
    );

    controller
        .apply_reroute(anchored_reroute_route(), manual_step_advance_policy())
        .expect("the anchored reroute must apply");

    let rerouted_trip_state = controller.current_trip_state().unwrap();
    assert_eq!(rerouted_trip_state.phase, NavigationPhase::Navigating);
    assert!(rerouted_trip_state.deviation.is_none());
    assert_eq!(rerouted_trip_state.step_positions.len(), 1);
    // Progress restarts on the replacement route's own declaration.
    assert!(
        (rerouted_trip_state.progress.distance_remaining_meters - TEST_REROUTE_STEP_LENGTH_METERS)
            .abs()
            < 1e-6
    );

    // The reroute both replaced the route and lifted the deviation, so the
    // phase topic saw exactly this ordered triple: the excursion's own
    // entry, then the reroute's apply, then the lifted deviation's clear.
    assert_eq!(
        *listener.phase_changes.lock().unwrap(),
        vec![
            NavigationPhaseChange::OffRouteEntered,
            NavigationPhaseChange::RerouteApplied,
            NavigationPhaseChange::OffRouteCleared
        ]
    );
    let deviation_changes = listener.deviation_changes.lock().unwrap();
    match deviation_changes.last() {
        Some(DeviationStatusChange {
            status_change_kind: DeviationStatusChangeKind::OnRoute,
            deviation_info: None,
        }) => {}
        other => panic!("the reroute must clear the deviation topic, got {other:?}"),
    }
}

#[test]
fn cancelled_subscription_stops_receiving_its_topic() {
    let controller = Navigator::new();
    let listener = Arc::new(RecordingListener::default());
    let instructions_subscription = controller
        .subscribe_to_navigation_instructions(Arc::clone(&listener) as Arc<dyn NavigationListener>);
    let _phase_subscription =
        controller.subscribe_to_phase_changes(Arc::clone(&listener) as Arc<dyn NavigationListener>);

    instructions_subscription.cancel();
    controller
        .start_navigation(
            straight_route(),
            UnitSystem::Metric,
            default_step_advance_policy(),
            default_deviation_policy(),
        )
        .expect("the fixture route and policies must start");

    // The cancelled instructions subscription went quiet; the phase
    // subscription of the same listener still receives (nothing here, as a
    // trip start is not a transition).
    assert!(listener.instruction_records.lock().unwrap().is_empty());
    assert!(listener.phase_changes.lock().unwrap().is_empty());
    // And cancelling again is idempotent.
    instructions_subscription.cancel();
}

#[test]
fn add_navigation_listener_delivers_every_topic_and_cancels_all_four() {
    let controller = Navigator::new();
    let listener = Arc::new(RecordingListener::default());
    let subscription =
        controller.add_navigation_listener(Arc::clone(&listener) as Arc<dyn NavigationListener>);

    controller
        .start_navigation(
            straight_route(),
            UnitSystem::Metric,
            // Manual advance keeps the near-band fix inside the first step,
            // so the speech planner sees its band crossing rather than a
            // step advance reset.
            manual_step_advance_policy(),
            default_deviation_policy(),
        )
        .expect("the fixture route and policies must start");
    // The instructions topic delivered the trip's initial payload.
    assert_eq!(listener.instruction_records.lock().unwrap().len(), 1);

    // A fix crossing the near speech band delivers the speech topic.
    let on_route_fix = route_fix(NEAR_BAND_TRIGGER_FIX_EAST_METERS);
    controller.update_location(on_route_fix);
    assert_eq!(
        listener.speech_instructions.lock().unwrap().len(),
        1,
        "the composite listener must be subscribed to the speech topic"
    );

    // An excursion off the route delivers the deviation and phase topics.
    controller.update_location(off_route_fix(
        TEST_STEP_LENGTH_METERS - WALKER_UPDATE_SPACING_METERS,
        TEST_OFF_ROUTE_EXCURSION_METERS,
    ));
    assert_eq!(
        listener.deviation_changes.lock().unwrap().len(),
        1,
        "the composite listener must be subscribed to the deviation topic"
    );
    assert_eq!(
        *listener.phase_changes.lock().unwrap(),
        vec![NavigationPhaseChange::OffRouteEntered],
        "the composite listener must be subscribed to the phase topic"
    );

    let delivered_instruction_count = listener.instruction_records.lock().unwrap().len();
    let delivered_speech_count = listener.speech_instructions.lock().unwrap().len();
    let delivered_deviation_count = listener.deviation_changes.lock().unwrap().len();
    let delivered_phase_count = listener.phase_changes.lock().unwrap().len();

    // The composite cancel removes all four registrations: a fix that would
    // otherwise recover the deviation (OnRoute plus OffRouteCleared) and
    // deliver an instruction payload reaches nothing.
    subscription.cancel();
    controller.update_location(route_fix(
        TEST_STEP_LENGTH_METERS - WALKER_UPDATE_SPACING_METERS,
    ));
    assert_eq!(
        listener.instruction_records.lock().unwrap().len(),
        delivered_instruction_count
    );
    assert_eq!(
        listener.speech_instructions.lock().unwrap().len(),
        delivered_speech_count
    );
    assert_eq!(
        listener.deviation_changes.lock().unwrap().len(),
        delivered_deviation_count
    );
    assert_eq!(
        listener.phase_changes.lock().unwrap().len(),
        delivered_phase_count
    );
    // And cancelling the composite again is idempotent.
    subscription.cancel();
}

#[test]
fn stop_navigation_clears_the_trip_state_and_drops_later_updates() {
    let controller = Navigator::new();
    let listener = Arc::new(RecordingListener::default());
    let _subscription =
        controller.add_navigation_listener(Arc::clone(&listener) as Arc<dyn NavigationListener>);

    // Stopping without an active trip is a no-op.
    controller.stop_navigation();
    assert!(controller.current_trip_state().is_none());
    assert!(controller.current_instructions().is_none());

    controller
        .start_navigation(
            straight_route(),
            UnitSystem::Metric,
            default_step_advance_policy(),
            default_deviation_policy(),
        )
        .expect("the fixture route and policies must start");
    assert!(controller.current_trip_state().is_some());
    assert!(controller.current_instructions().is_some());
    let delivered_instruction_count = listener.instruction_records.lock().unwrap().len();

    controller.stop_navigation();

    assert!(
        controller.current_trip_state().is_none(),
        "stop_navigation ends the active trip"
    );
    assert!(
        controller.current_instructions().is_none(),
        "a stopped trip has no instruction payload"
    );
    assert!(
        !controller.begin_reroute_calculation(),
        "a stopped trip refuses reroute-request signals"
    );
    controller.update_location(route_fix(WALKER_UPDATE_SPACING_METERS));
    assert!(controller.current_trip_state().is_none());
    assert_eq!(
        listener.instruction_records.lock().unwrap().len(),
        delivered_instruction_count,
        "a stopped trip dispatches nothing"
    );
    assert!(listener.phase_changes.lock().unwrap().is_empty());

    // The listener subscription survived the stop: the next trip delivers.
    controller
        .start_navigation(
            straight_route(),
            UnitSystem::Metric,
            default_step_advance_policy(),
            default_deviation_policy(),
        )
        .expect("a new trip starts after a stop");
    assert!(controller.current_trip_state().is_some());
    assert_eq!(
        listener.instruction_records.lock().unwrap().len(),
        delivered_instruction_count + 1,
        "the subscription is controller configuration and serves the next trip"
    );
}

#[test]
fn refused_policy_thresholds_throw_and_start_no_trip() {
    let controller = Navigator::new();
    let listener = Arc::new(RecordingListener::default());
    let _subscriptions = subscribe_to_all_topics(&controller, &listener);

    let refused_step_policy = StepAdvancePolicy {
        advance_threshold_meters: REFUSED_NEGATIVE_THRESHOLD_METERS,
        ..default_step_advance_policy()
    };
    let step_failure = controller
        .start_navigation(
            straight_route(),
            UnitSystem::Metric,
            refused_step_policy,
            default_deviation_policy(),
        )
        .expect_err("a negative step-advance threshold must be refused");
    assert!(matches!(
        step_failure,
        SdkError::InvalidNavigationInput { .. }
    ));

    let refused_deviation_policy = DeviationPolicy {
        deviation_threshold_meters: REFUSED_NEGATIVE_THRESHOLD_METERS,
        ..default_deviation_policy()
    };
    let deviation_failure = controller
        .start_navigation(
            straight_route(),
            UnitSystem::Metric,
            default_step_advance_policy(),
            refused_deviation_policy,
        )
        .expect_err("a negative deviation threshold must be refused");
    match &deviation_failure {
        SdkError::InvalidNavigationInput { detail } => assert!(
            detail.contains(REFUSED_THRESHOLD_DETAIL_FRAGMENT),
            "the refusal must describe the refused threshold: {detail}"
        ),
        other => panic!("unexpected failure variant: {other:?}"),
    }

    // No trip started on either refusal, and the listener received nothing.
    assert!(controller.current_trip_state().is_none());
    assert!(listener.instruction_records.lock().unwrap().is_empty());
}

#[test]
fn route_record_with_an_unknown_maneuver_label_is_refused() {
    let controller = Navigator::new();
    let mut refused_route = straight_route();
    refused_route.legs[0].steps[1].maneuver_type = UNRECOGNIZED_MANEUVER_LABEL.to_string();

    let refusal = controller
        .start_navigation(
            refused_route,
            UnitSystem::Metric,
            default_step_advance_policy(),
            default_deviation_policy(),
        )
        .expect_err("an unknown maneuver label must be refused");
    match &refusal {
        SdkError::InvalidNavigationInput { detail } => assert!(
            detail.contains(UNRECOGNIZED_MANEUVER_LABEL),
            "the refusal must name the offending label: {detail}"
        ),
        other => panic!("unexpected failure variant: {other:?}"),
    }
    assert!(controller.current_trip_state().is_none());
}

#[test]
fn reentrant_listener_drive_is_suppressed_and_the_state_stays_applied() {
    let controller = Arc::new(Navigator::new());
    /// Drive rounds the listener performs inside its own callback before
    /// falling quiet: exactly the structural budget of the native
    /// re-entrancy test, keeping any recursive cascade finite.
    const REENTRANT_DRIVE_BUDGET: usize = 1;
    let listener = Arc::new(ReentrantDrivingListener {
        controller: Arc::clone(&controller),
        remaining_drive_budget: AtomicUsize::new(REENTRANT_DRIVE_BUDGET),
        observed_dispatches: AtomicUsize::new(0),
    });
    let _instructions_subscription = controller
        .subscribe_to_navigation_instructions(Arc::clone(&listener) as Arc<dyn NavigationListener>);

    controller
        .start_navigation(
            straight_route(),
            UnitSystem::Metric,
            manual_step_advance_policy(),
            default_deviation_policy(),
        )
        .expect("the fixture route and policies must start");

    // The trip-start dispatch delivered; its single instruction callback drove
    // one nested location update. The nested dispatch was suppressed by the
    // broadcaster's single-in-flight guard (no callback ran inside it — the
    // counted dispatches stay at the outer one), but the state change it
    // carried was applied by the controller.
    assert_eq!(listener.observed_dispatches.load(Ordering::SeqCst), 1);
    let driven_trip_state = controller
        .current_trip_state()
        .expect("the driven trip must still report its state");
    assert!(
        driven_trip_state.snapped_coordinate.is_some(),
        "the suppressed nested update must still have projected its fix"
    );

    // After the guard released, a fresh top-level dispatch delivers the
    // callback normally again; the listener's budget is exhausted by then, so
    // it stays quiet this time (it is not re-armed).
    controller.update_location(route_fix(20.0));
    assert_eq!(listener.observed_dispatches.load(Ordering::SeqCst), 1);
}

#[test]
fn location_updates_without_a_trip_are_dropped_and_reads_are_none() {
    let controller = Navigator::new();
    controller.update_location(route_fix(10.0));
    assert!(controller.current_trip_state().is_none());
    assert!(controller.current_instructions().is_none());

    // And the controller is still usable afterwards.
    controller
        .start_navigation(
            straight_route(),
            UnitSystem::Metric,
            default_step_advance_policy(),
            default_deviation_policy(),
        )
        .expect("the fixture route and policies must start");
    assert!(
        controller
            .current_trip_state()
            .expect("the started trip must report its state")
            .phase
            == NavigationPhase::Navigating
    );
}
