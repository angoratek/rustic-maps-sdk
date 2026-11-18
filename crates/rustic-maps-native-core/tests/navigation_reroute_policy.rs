//! Controller-seam tests of the round-11 reroute policy (Ferrostar B3) and
//! the speech utterance identity (B4) on the exported
//! `Navigator`: the gates are controller wall-clock facts —
//! never reducer state — so the end-to-end wiring is driven here over the
//! exported methods, while the boundary-pinned cooldown and movement
//! comparisons are unit-tested with injected clocks in `rustic-maps-navigation`'s
//! reroute module. The same synthetic eastward route fixtures power this
//! suite as `navigation_callbacks.rs` (tests are self-contained there by
//! convention), plus recordings of the speech topic.

use std::sync::Arc;
use std::sync::Mutex;

use rustic_maps::{
    NavigationListener, NavigationLocationUpdate, NavigationPhase, Navigator, ReroutePolicy, Route,
    RouteLeg, RouteStep, SdkError, SpeechInstruction, StepAdvancePolicy, StepAdvancePolicyKind,
    Subscription, UnitSystem, default_deviation_policy, default_reroute_policy,
    default_step_advance_policy,
};
use rustic_maps_navigation::{
    DEFAULT_REROUTE_RECALCULATION_COOLDOWN_SECONDS, DEFAULT_REROUTE_RECALCULATION_MOVEMENT_METERS,
};

/// Latitude, in degrees, of the synthetic fixture origin (the same
/// convention as the navigation fixtures).
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
/// Horizontal accuracy, in meters, of the synthetic walker's fixes.
const TEST_ACCURACY_METERS: f64 = 2.0;
/// Total length, in meters, of the synthetic route.
const TEST_ROUTE_TOTAL_LENGTH_METERS: f64 = TEST_STEP_LENGTH_METERS * TEST_STEP_COUNT as f64;
/// East offset, in meters, the reroute fixture anchors at (two full steps of
/// the synthetic route).
const TEST_REROUTE_ANCHOR_OFFSET_METERS: f64 = TEST_STEP_LENGTH_METERS * 2.0;
/// Length, in meters, of the single-step replacement route for reroutes.
const TEST_REROUTE_STEP_LENGTH_METERS: f64 = 150.0;
/// Lateral excursion, in meters, used to leave the synthetic route.
const TEST_OFF_ROUTE_EXCURSION_METERS: f64 = 100.0;
/// Step-advance threshold fixture value used by the manual-policy trips.
const MANUAL_POLICY_IGNORED_THRESHOLD_VALUE: f64 = 0.0;
/// A reroute cooldown configured away, so the in-flight and movement gates
/// can be pinned end to end without wall-clock waits.
const CONFIGURED_NO_COOLDOWN_SECONDS: f64 = 0.0;
/// A reroute movement threshold configured at the same scale as the
/// documented default, driven end to end.
const CONFIGURED_MOVEMENT_THRESHOLD_METERS: f64 = 50.0;
/// Displacement, in meters, inside [`CONFIGURED_MOVEMENT_THRESHOLD_METERS`].
const SUB_THRESHOLD_MOVEMENT_METERS: f64 = 40.0;
/// Displacement, in meters, beyond [`CONFIGURED_MOVEMENT_THRESHOLD_METERS`].
const BEYOND_THRESHOLD_MOVEMENT_METERS: f64 = 60.0;
/// Detail fragment of every refused-policy-threshold error thrown at the
/// boundary.
const REFUSED_THRESHOLD_DETAIL_FRAGMENT: &str = "must be finite and non-negative";

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
/// [`TEST_STEP_LENGTH_METERS`], departure, continue, arrival.
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
        bounding_box: rustic_maps::BoundingBox {
            south_west: east_north_coordinate(0.0, 0.0),
            north_east: east_north_coordinate(TEST_ROUTE_TOTAL_LENGTH_METERS, 0.0),
        },
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

/// A policy with both hysteresis tunables configured but not in the way
/// (isolating the in-flight gate end to end, which is behavior, never a
/// tunable).
fn unhysteretic_reroute_policy() -> ReroutePolicy {
    ReroutePolicy {
        recalculation_cooldown_seconds: CONFIGURED_NO_COOLDOWN_SECONDS,
        recalculation_movement_meters: CONFIGURED_MOVEMENT_THRESHOLD_METERS,
    }
}

/// A listener recording the speech topic only, for the utterance-identity
/// wiring test.
#[derive(Default)]
struct RecordingSpeechListener {
    speech_instructions: Mutex<Vec<SpeechInstruction>>,
}

impl NavigationListener for RecordingSpeechListener {
    fn on_navigation_instruction(&self, _instruction: rustic_maps::NavigationInstruction) {}

    fn on_deviation_status_change(
        &self,
        _deviation_status_change: rustic_maps::DeviationStatusChange,
    ) {
    }

    fn on_navigation_phase_change(
        &self,
        _navigation_phase_change: rustic_maps::NavigationPhaseChange,
    ) {
    }

    fn on_speech_instruction(&self, speech_instruction: SpeechInstruction) {
        self.speech_instructions
            .lock()
            .unwrap()
            .push(speech_instruction);
    }
}

/// Starts a manual-advance trip on the straight route.
fn start_manual_trip(controller: &Navigator) {
    controller
        .start_navigation(
            straight_route(),
            UnitSystem::Metric,
            manual_step_advance_policy(),
            default_deviation_policy(),
        )
        .expect("the fixture route and policies must start");
}

/// Starts the default-policy trip used by the utterance-identity wiring
/// test.
fn start_default_trip(controller: &Navigator) {
    controller
        .start_navigation(
            straight_route(),
            UnitSystem::Metric,
            default_step_advance_policy(),
            default_deviation_policy(),
        )
        .expect("the fixture route and policies must start");
}

/// Walks the controller along the synthetic route at 10-meter spacing.
fn walk_controller(controller: &Navigator) {
    const WALKER_UPDATE_SPACING_METERS: f64 = 10.0;
    let total_updates_for_walker =
        (TEST_ROUTE_TOTAL_LENGTH_METERS / WALKER_UPDATE_SPACING_METERS) as usize;
    for update_number in 1..=total_updates_for_walker {
        controller.update_location(route_fix(
            update_number as f64 * WALKER_UPDATE_SPACING_METERS,
        ));
    }
}

#[test]
fn default_reroute_policy_matches_the_named_native_constants() {
    let default_reroute_policy_record = default_reroute_policy();
    assert_eq!(
        default_reroute_policy_record.recalculation_cooldown_seconds,
        DEFAULT_REROUTE_RECALCULATION_COOLDOWN_SECONDS
    );
    assert_eq!(
        default_reroute_policy_record.recalculation_movement_meters,
        DEFAULT_REROUTE_RECALCULATION_MOVEMENT_METERS
    );
    // The named constants are genuinely the shipped thresholds, not invented
    // values: sanity against the Ferrostar-recommended calibrations the
    // surface documents.
    assert_eq!(DEFAULT_REROUTE_RECALCULATION_COOLDOWN_SECONDS, 5.0);
    assert_eq!(DEFAULT_REROUTE_RECALCULATION_MOVEMENT_METERS, 50.0);
}

#[test]
fn refused_reroute_policies_throw_and_leave_the_policy_in_force_untouched() {
    let controller = Navigator::new();
    controller
        .configure_reroute_policy(unhysteretic_reroute_policy())
        .expect("the well-formed policy must configure");

    let mut refused_policy = default_reroute_policy();
    refused_policy.recalculation_cooldown_seconds = -1.0;
    let refused_negative = controller.configure_reroute_policy(refused_policy);
    match refused_negative {
        Err(SdkError::InvalidNavigationInput { detail }) => {
            assert!(detail.contains(REFUSED_THRESHOLD_DETAIL_FRAGMENT));
        }
        other => panic!("a negative cooldown must be refused, got {other:?}"),
    }

    refused_policy.recalculation_cooldown_seconds = f64::NAN;
    refused_policy.recalculation_movement_meters = 1.0;
    match controller.configure_reroute_policy(refused_policy) {
        Err(SdkError::InvalidNavigationInput { detail }) => {
            assert!(detail.contains(REFUSED_THRESHOLD_DETAIL_FRAGMENT));
        }
        other => panic!("a NaN cooldown must be refused, got {other:?}"),
    }

    refused_policy.recalculation_cooldown_seconds = 0.0;
    refused_policy.recalculation_movement_meters = f64::NEG_INFINITY;
    match controller.configure_reroute_policy(refused_policy) {
        Err(SdkError::InvalidNavigationInput { detail }) => {
            assert!(detail.contains(REFUSED_THRESHOLD_DETAIL_FRAGMENT));
        }
        other => panic!("a non-finite movement threshold must be refused, got {other:?}"),
    }

    // The refusals leave the earlier configured policy in force: the
    // movement gate still fires at 50 meters and the cooldown stands
    // configured away.
    start_manual_trip(&controller);
    controller.update_location(route_fix(0.0));
    assert!(
        controller.begin_reroute_calculation(),
        "the trip's first attempt must pass"
    );
    controller.cancel_reroute_calculation();
    controller.update_location(route_fix(SUB_THRESHOLD_MOVEMENT_METERS));
    assert!(
        !controller.begin_reroute_calculation(),
        "the configured movement gate must still be in force after refusals"
    );
}

#[test]
fn begin_refuses_without_a_trip() {
    let controller = Navigator::new();
    assert!(
        !controller.begin_reroute_calculation(),
        "no trip means no recalculation signal can pass"
    );
    assert!(controller.current_trip_state().is_none());
}

#[test]
fn begin_refuses_on_an_arrived_trip() {
    let controller = Navigator::new();
    controller
        .start_navigation(
            straight_route(),
            UnitSystem::Metric,
            default_step_advance_policy(),
            default_deviation_policy(),
        )
        .expect("the fixture route and policies must start");
    walk_controller(&controller);
    assert_eq!(
        controller
            .current_trip_state()
            .expect("the walked trip must report its state")
            .phase,
        NavigationPhase::Arrived
    );
    assert!(
        !controller.begin_reroute_calculation(),
        "an arrived trip takes no reroute signal"
    );
}

#[test]
fn begin_opens_the_in_flight_readback_and_duplicate_begins_refuse() {
    let controller = Navigator::new();
    controller
        .configure_reroute_policy(unhysteretic_reroute_policy())
        .expect("the well-formed policy must configure");
    start_manual_trip(&controller);

    // Nothing in flight before the first attempt.
    assert!(
        !controller
            .current_trip_state()
            .expect("the started trip must report its state")
            .is_calculating_new_route
    );

    assert!(
        controller.begin_reroute_calculation(),
        "the trip's first attempt has no in-flight, cooldown or movement memory"
    );
    assert!(
        controller
            .current_trip_state()
            .expect("the started trip must report its state")
            .is_calculating_new_route,
        "the in-flight readback is the controller's gate fact"
    );

    assert!(
        !controller.begin_reroute_calculation(),
        "a duplicate begin while one is in flight must be refused"
    );

    controller.cancel_reroute_calculation();
    assert!(
        !controller
            .current_trip_state()
            .expect("the started trip must report its state")
            .is_calculating_new_route,
        "a cancellation must clear the in-flight readback"
    );
}

#[test]
fn cancelled_calculation_is_still_gated_by_the_default_cooldown() {
    let controller = Navigator::new();
    controller
        .configure_reroute_policy(default_reroute_policy())
        .expect("the default policy must configure");
    start_manual_trip(&controller);

    assert!(controller.begin_reroute_calculation());
    controller.cancel_reroute_calculation();
    assert!(
        !controller.begin_reroute_calculation(),
        "an abandoned request still runs its cooldown from the beginning"
    );

    // A new trip forgets the gate memory (the policy persists): the next
    // trip's first attempt passes immediately.
    start_manual_trip(&controller);
    assert!(
        controller.begin_reroute_calculation(),
        "a trip start must reset the gate memory but keep the configured policy"
    );
    controller.cancel_reroute_calculation();
    assert!(
        !controller.begin_reroute_calculation(),
        "the kept policy must still gate the new trip's second attempt"
    );
}

#[test]
fn apply_reroute_settles_the_in_flight_calculation() {
    let controller = Navigator::new();
    controller
        .configure_reroute_policy(unhysteretic_reroute_policy())
        .expect("the well-formed policy must configure");
    start_manual_trip(&controller);
    controller.update_location(route_fix(TEST_REROUTE_ANCHOR_OFFSET_METERS));
    controller.update_location(off_route_fix(
        TEST_REROUTE_ANCHOR_OFFSET_METERS,
        TEST_OFF_ROUTE_EXCURSION_METERS,
    ));
    assert!(
        controller.begin_reroute_calculation(),
        "the signal that asked the integrator for the replacement route"
    );
    assert!(
        controller
            .current_trip_state()
            .expect("the deviated trip must report its state")
            .is_calculating_new_route
    );

    controller
        .apply_reroute(anchored_reroute_route(), manual_step_advance_policy())
        .expect("the anchored reroute must apply");

    assert!(
        !controller
            .current_trip_state()
            .expect("the rerouted trip must report its state")
            .is_calculating_new_route,
        "applying the requested route settles the in-flight calculation"
    );
    // The gate reopens on the integrator's next qualifying signal — settle
    // reopens only the in-flight gate, the movement memory keeps its
    // reference (the abandoned off-route fix), so the next begin waits for
    // a fix clearly beyond the threshold from it.
    controller.update_location(route_fix(
        BEYOND_THRESHOLD_MOVEMENT_METERS + TEST_REROUTE_ANCHOR_OFFSET_METERS,
    ));
    assert!(
        controller.begin_reroute_calculation(),
        "a settle must reopen the request-in-flight gate"
    );
    assert!(
        controller
            .current_trip_state()
            .expect("the rerouted trip must report its state")
            .is_calculating_new_route
    );
}

#[test]
fn movement_threshold_gates_the_begin_end_to_end() {
    let controller = Navigator::new();
    controller
        .configure_reroute_policy(ReroutePolicy {
            recalculation_cooldown_seconds: CONFIGURED_NO_COOLDOWN_SECONDS,
            recalculation_movement_meters: CONFIGURED_MOVEMENT_THRESHOLD_METERS,
        })
        .expect("the well-formed policy must configure");
    start_manual_trip(&controller);

    // The reference fix anchors at the origin.
    controller.update_location(route_fix(0.0));
    assert!(controller.begin_reroute_calculation());
    controller.cancel_reroute_calculation();

    // 40 meters from the reference: refused by the movement gate.
    controller.update_location(route_fix(SUB_THRESHOLD_MOVEMENT_METERS));
    assert!(
        !controller.begin_reroute_calculation(),
        "a fix inside the movement threshold must be refused"
    );
    assert!(
        !controller
            .current_trip_state()
            .expect("the trip must report its state")
            .is_calculating_new_route,
        "a refused signal must not raise the in-flight readback"
    );

    // 60 meters from the same reference (displacements measure against the
    // last calculation's reference, not a running walk): passes.
    controller.update_location(route_fix(BEYOND_THRESHOLD_MOVEMENT_METERS));
    assert!(controller.begin_reroute_calculation());
}

#[test]
fn speech_utterance_ids_are_monotonic_across_trips() {
    let controller = Navigator::new();
    let listener = Arc::new(RecordingSpeechListener::default());
    let speech_subscription: Arc<Subscription> = controller
        .subscribe_to_speech_instructions(Arc::clone(&listener) as Arc<dyn NavigationListener>);

    // Two consecutive trips on the same route: the band triggers announce
    // within each trip, and the second trip's ids must continue the first
    // trip's counter (never reused), while the pacing memory itself resets.
    start_default_trip(&controller);
    walk_controller(&controller);
    let first_trip_announcement_ids: Vec<u64> = listener
        .speech_instructions
        .lock()
        .unwrap()
        .iter()
        .map(|speech_instruction| speech_instruction.utterance_id)
        .collect();

    start_default_trip(&controller);
    walk_controller(&controller);
    let speech_instructions = listener.speech_instructions.lock().unwrap().clone();
    assert!(
        speech_instructions.len() >= 2,
        "both trips must have announced ({:?})",
        speech_instructions
            .iter()
            .map(|speech_instruction| speech_instruction.utterance_id)
            .collect::<Vec<_>>()
    );
    // Every id is strictly increasing across both trips.
    assert!(
        speech_instructions
            .windows(2)
            .all(|utterance_pair| utterance_pair[1].utterance_id > utterance_pair[0].utterance_id),
        "utterance ids must be monotonic across trips: {:?}",
        speech_instructions
            .iter()
            .map(|speech_instruction| speech_instruction.utterance_id)
            .collect::<Vec<_>>()
    );
    // The second trip's announcements all carry ids above every first-trip
    // id (the counter survived the trip boundary and never restarted).
    let second_trip_announcement_ids = &speech_instructions[first_trip_announcement_ids.len()..];
    let highest_first_trip_id = first_trip_announcement_ids
        .last()
        .copied()
        .expect("the first trip must have announced");
    assert!(
        second_trip_announcement_ids
            .iter()
            .all(|speech_instruction| speech_instruction.utterance_id > highest_first_trip_id),
        "the second trip's ids must continue past the first trip's: \
         {first_trip_announcement_ids:?} then {second_trip_announcement_ids:?}"
    );

    speech_subscription.cancel();
}
