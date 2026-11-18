//! The browser navigation seam: a wasm `Navigator` class driving the same
//! `rustic-maps-navigation` reducer session the native contract's `Navigator` drives,
//! so the page follows a trip live instead of previewing a static route
//! layout.
//!
//! The class accepts the lower-camel-case plain objects the rest of the seam
//! exchanges: a `Route` exactly as `calculateRoute` answers it (the shape is
//! read through the same parser as the simulated-trip seam), and a
//! `NavigationLocationUpdate` mirroring the contract's dictionary
//! (`coordinate`, `horizontalAccuracyMeters`, `courseHeadingDegrees`,
//! `timestampSeconds`). `startNavigation` builds the session with the SDK's
//! documented defaults, matching `rustic-maps-native-core`'s `start_navigation`: the
//! distance-to-end-of-step condition with the Ferrostar-recommended driving
//! thresholds, the default distance-threshold deviation detector, no separate
//! arrival condition and raw course reporting.
//!
//! # Scope against the frozen contract
//!
//! The contract's `Navigator` carries the listener, speech and
//! reroute-recalculation members on top of the trip-driving core. The wasm
//! surface has no foreign-listener seam, so the class exposes the driving and
//! readback core: start, location updates, reroutes, stop and the trip
//! snapshot, plus a JavaScript event bridge (`addNavigationListener`) for the
//! same navigation topics. `hasActiveTrip` is the one wasm-only helper. The
//! snapshot is a presentation-ready projection of the contract's
//! `TripNavigationState` and `NavigationInstruction`.
//!
//! # Panic-freedom
//!
//! Every exported path answers `null` (no trip) or throws an error carrying
//! the verbatim detail and a stable code; no exported path unwraps. The controller's own state is a pure
//! reducer session, and the reducer's `StartTrip`/`LocationUpdated` events
//! always answer a session, so the conditional updates below never need an
//! invariant panic.

use std::sync::Arc;

use geospatial_types::route::RouteData;
use geospatial_types::{GeographicCoordinate, UnitSystem};
use rustic_maps_navigation::{
    CourseFiltering, DeviationDetector, DistanceDeviationDetector, DistanceToEndOfStepCondition,
    ManeuverInstructionText, NavigationEvent, NavigationLocationUpdate, NavigationPhase,
    NavigationSession, StepAdvanceCondition, TripNavigationState, compose_navigation_instructions,
    reduce_navigation_event,
};

/// `NavigatorTripState.phase` label for the contract's `Navigating` phase.
const PHASE_LABEL_NAVIGATING: &str = "navigating";
/// `NavigatorTripState.phase` label for the contract's `OffRoute` phase.
const PHASE_LABEL_OFF_ROUTE: &str = "offRoute";
/// `NavigatorTripState.phase` label for the contract's `Arrived` phase.
const PHASE_LABEL_ARRIVED: &str = "arrived";
/// `NavigatorDeviation.statusChangeKind` label: the snapshot only carries a
/// deviation while the phase is off route, so the status update's kind is
/// always the contract's `OffRoute` variant.
const DEVIATION_STATUS_CHANGE_KIND_OFF_ROUTE: &str = "OffRoute";

/// The step-advance condition `startNavigation` installs: the SDK's default
/// distance-to-end-of-step condition with the Ferrostar-recommended driving
/// thresholds (`rustic-maps-native-core`'s `default_step_advance_policy` values).
fn default_step_advance_condition() -> Arc<dyn StepAdvanceCondition> {
    Arc::new(DistanceToEndOfStepCondition {
        advance_threshold_meters:
            rustic_maps_navigation::DEFAULT_DISTANCE_TO_END_OF_STEP_THRESHOLD_METERS,
        minimum_horizontal_accuracy_meters:
            rustic_maps_navigation::DEFAULT_MINIMUM_HORIZONTAL_ACCURACY_METERS,
    })
}

/// The deviation detector `startNavigation` installs: the SDK default
/// distance-threshold detector (`rustic-maps-native-core`'s `default_deviation_policy`
/// value).
fn default_deviation_detector() -> Arc<dyn DeviationDetector> {
    Arc::new(DistanceDeviationDetector {
        deviation_threshold_meters:
            rustic_maps_navigation::DEFAULT_DEVIATION_DISTANCE_THRESHOLD_METERS,
    })
}

/// Labels one native navigation phase with the snapshot's phase string.
fn phase_label(native_phase: NavigationPhase) -> &'static str {
    match native_phase {
        NavigationPhase::Navigating => PHASE_LABEL_NAVIGATING,
        NavigationPhase::OffRoute => PHASE_LABEL_OFF_ROUTE,
        NavigationPhase::Arrived => PHASE_LABEL_ARRIVED,
    }
}

/// The banner text of one composed maneuver: the provider-authored display
/// variant with the live distance prefix where one was composed, or the empty
/// string when the provider authored no text (nothing is invented).
fn maneuver_display_text(instruction: &ManeuverInstructionText) -> String {
    match instruction {
        ManeuverInstructionText::Composed { display_text, .. } => display_text.clone(),
        ManeuverInstructionText::Unavailable => String::new(),
    }
}

/// The provider-authored banner text of one maneuver, verbatim, when the
/// provider authored one. The native cards show this beside the separately
/// phrased distance; the composed variant repeats the distance inside the
/// sentence, so a banner that already shows the phrase reads it twice.
/// The maneuver's text-to-speech variant, or an empty string when the
/// provider authored no text (the emitter invents nothing).
fn maneuver_speech_text(instruction: &ManeuverInstructionText) -> String {
    match instruction {
        ManeuverInstructionText::Composed { speech_text, .. } => speech_text.clone(),
        ManeuverInstructionText::Unavailable => String::new(),
    }
}

fn maneuver_provider_display_text(instruction: &ManeuverInstructionText) -> Option<String> {
    match instruction {
        ManeuverInstructionText::Composed {
            provider_display_text,
            ..
        } if !provider_display_text.is_empty() => Some(provider_display_text.clone()),
        ManeuverInstructionText::Composed { .. } | ManeuverInstructionText::Unavailable => None,
    }
}

/// The plain, presentation-ready trip snapshot the class answers. It mirrors
/// the contract's `TripNavigationState`/`NavigationInstruction` pair as the
/// fields the browser TBT card consumes.
#[derive(Clone, PartialEq)]
struct TripSnapshot {
    /// Snapshot phase label (one of the `PHASE_LABEL_*` constants).
    phase: &'static str,
    /// The composed instruction payload of the current state.
    instruction: InstructionSnapshot,
    /// The active deviation, present exactly while the phase is off route.
    deviation: Option<DeviationSnapshot>,
    /// Position snapped onto the current step, absent until the first fix.
    snapped_coordinate: Option<GeographicCoordinate>,
    /// Whether the trip has arrived.
    is_arrived: bool,
    /// Traversal-ordered index of the step currently traversed.
    current_step_index: u64,
}

/// The composed instruction payload of one snapshot.
#[derive(Clone, PartialEq)]
struct InstructionSnapshot {
    /// The maneuver currently being traversed.
    active_maneuver: ActiveManeuverSnapshot,
    /// The maneuver after the active one, when the route has a next step.
    upcoming_maneuver: Option<UpcomingManeuverSnapshot>,
    /// Progress along the remaining trip.
    progress: ProgressSnapshot,
    /// The contract instruction's arrival marker.
    is_arrival: bool,
    /// The contract instruction's rerouting-required marker.
    is_rerouting_required: bool,
}

/// The active maneuver's two banner lines.
#[derive(Clone, PartialEq)]
struct ActiveManeuverSnapshot {
    /// The canonical distance phrase text (`"130 meters"`, `"now"`, …).
    distance_phrase_text: String,
    /// The composed instruction text (distance prefix plus provider text).
    display_text: String,
    /// The provider-authored banner text, verbatim, when the provider
    /// authored one (the native cards' second banner line).
    provider_display_text: Option<String>,
    /// The text-to-speech variant (the provider's speech text with the same
    /// distance prefix), for the platform's speech engine. Read by the
    /// wasm-gated snapshot builder only; the native test build carries it as
    /// data.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    speech_text: String,
}

/// The upcoming maneuver's banner text.
#[derive(Clone, PartialEq)]
struct UpcomingManeuverSnapshot {
    /// The composed instruction text of the following step.
    display_text: String,
    /// The text-to-speech variant of the following step. Read by the
    /// wasm-gated snapshot builder only.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    speech_text: String,
}

/// Progress along the remaining trip, matching the contract's `TripProgress`.
#[derive(Clone, PartialEq)]
struct ProgressSnapshot {
    /// Distance to the end of the current step (the next maneuver), in meters.
    distance_to_next_maneuver_meters: f64,
    /// Total remaining distance, in meters.
    distance_remaining_meters: f64,
    /// Total remaining travel time, in seconds.
    duration_remaining_seconds: f64,
}

/// The active deviation, matching the contract's `DeviationStatusChange`
/// shape where the snapshot flattens it.
#[derive(Clone, PartialEq)]
struct DeviationSnapshot {
    /// Always the contract's `OffRoute` variant (see the module docs).
    status_change_kind: &'static str,
    /// Distance from the raw fix to the remaining route line, in meters.
    distance_from_route_meters: f64,
}

impl TripSnapshot {
    /// Projects one reducer state into the snapshot the browser reads.
    fn from_trip_state(trip_state: &TripNavigationState, unit_system: UnitSystem) -> Self {
        let instruction = compose_navigation_instructions(trip_state, unit_system);
        // The snapshot carries a deviation only while the phase is off route:
        // the wrong-step grade leaves the phase navigating and is guidance
        // feedback, not a deviation-status update in the contract's sense.
        let deviation = match trip_state.phase {
            NavigationPhase::OffRoute => {
                trip_state
                    .deviation
                    .as_ref()
                    .map(|deviation_info| DeviationSnapshot {
                        status_change_kind: DEVIATION_STATUS_CHANGE_KIND_OFF_ROUTE,
                        distance_from_route_meters: deviation_info.deviation_from_route_meters,
                    })
            }
            NavigationPhase::Navigating | NavigationPhase::Arrived => None,
        };
        Self {
            phase: phase_label(trip_state.phase),
            instruction: InstructionSnapshot {
                active_maneuver: ActiveManeuverSnapshot {
                    distance_phrase_text: instruction
                        .active_maneuver
                        .distance_phrase
                        .display_text(),
                    display_text: maneuver_display_text(&instruction.active_maneuver.instruction),
                    provider_display_text: maneuver_provider_display_text(
                        &instruction.active_maneuver.instruction,
                    ),
                    speech_text: maneuver_speech_text(&instruction.active_maneuver.instruction),
                },
                upcoming_maneuver: instruction.upcoming_maneuver.as_ref().map(|upcoming| {
                    UpcomingManeuverSnapshot {
                        display_text: maneuver_display_text(&upcoming.instruction),
                        speech_text: maneuver_speech_text(&upcoming.instruction),
                    }
                }),
                progress: ProgressSnapshot {
                    distance_to_next_maneuver_meters: trip_state
                        .progress
                        .distance_to_next_maneuver_meters,
                    distance_remaining_meters: trip_state.progress.distance_remaining_meters,
                    duration_remaining_seconds: trip_state.progress.duration_remaining_seconds,
                },
                is_arrival: instruction.is_arrival,
                is_rerouting_required: instruction.is_rerouting_required,
            },
            deviation,
            snapped_coordinate: trip_state.snapped_coordinate,
            is_arrived: trip_state.phase == NavigationPhase::Arrived,
            current_step_index: trip_state.current_step_position as u64,
        }
    }
}

/// The one active trip: the reducer session plus the unit system its
/// instruction text is composed in.
struct ActiveTrip {
    /// The reducer's session.
    session: NavigationSession,
    /// Unit system the instructions are phrased in.
    unit_system: UnitSystem,
}

impl ActiveTrip {
    /// Begins a session exactly like the native controller's
    /// `start_navigation` with the SDK default policies.
    fn start(route: RouteData, unit_system: UnitSystem) -> Option<Self> {
        let session = reduce_navigation_event(
            None,
            NavigationEvent::StartTrip {
                route,
                step_advance_condition: default_step_advance_condition(),
                deviation_detector: default_deviation_detector(),
                // The arrival-aware advance condition is not an FFI policy and
                // the frozen contract carries no parameter for it.
                arrival_step_advance_condition: None,
                // Raw course reporting, matching the native controller.
                course_filtering: CourseFiltering::Raw,
            },
        )?;
        Some(Self {
            session,
            unit_system,
        })
    }

    /// Consumes one position fix through the reducer.
    fn with_location_update(mut self, update: NavigationLocationUpdate) -> Self {
        if let Some(session) = reduce_navigation_event(
            Some(self.session.clone()),
            NavigationEvent::LocationUpdated(update),
        ) {
            self.session = session;
        }
        self
    }

    /// Replaces the route through the reducer's reroute event (an arrived
    /// trip ignores it).
    fn with_route(mut self, route: RouteData) -> Self {
        if let Some(session) = reduce_navigation_event(
            Some(self.session.clone()),
            NavigationEvent::RerouteProvided {
                route,
                step_advance_condition: default_step_advance_condition(),
            },
        ) {
            self.session = session;
        }
        self
    }

    /// Projects the session's current state.
    fn snapshot(&self) -> TripSnapshot {
        TripSnapshot::from_trip_state(&self.session.state, self.unit_system)
    }
}

/// The trip-driving core behind the wasm class, split out so tests can
/// exercise it without a JavaScript runtime. It owns at most one active trip
/// and keeps no speech-planning memory.
struct NavigatorController {
    /// The one active trip, or `None` between trips.
    active_trip: Option<ActiveTrip>,
}

impl NavigatorController {
    /// Builds a controller with no active trip.
    fn new() -> Self {
        Self { active_trip: None }
    }

    /// Begins a trip, replacing any active one.
    fn start_navigation(&mut self, route: RouteData, unit_system: UnitSystem) {
        self.active_trip = ActiveTrip::start(route, unit_system);
    }

    /// Consumes one fix and answers the resulting snapshot; `None` without an
    /// active trip (the fix is dropped, the reducer's own semantics).
    fn update_location(&mut self, update: NavigationLocationUpdate) -> Option<TripSnapshot> {
        let trip = self.active_trip.take()?.with_location_update(update);
        let snapshot = trip.snapshot();
        self.active_trip = Some(trip);
        Some(snapshot)
    }

    /// Replaces the active trip's route; without an active trip the reroute is
    /// dropped.
    fn apply_reroute(&mut self, route: RouteData) {
        if let Some(trip) = self.active_trip.take() {
            self.active_trip = Some(trip.with_route(route));
        }
    }

    /// Ends the active trip.
    fn stop_navigation(&mut self) {
        self.active_trip = None;
    }

    /// The current snapshot, or `None` without an active trip.
    fn current_trip_state(&self) -> Option<TripSnapshot> {
        self.active_trip.as_ref().map(ActiveTrip::snapshot)
    }

    /// Whether a trip is active.
    fn has_active_trip(&self) -> bool {
        self.active_trip.is_some()
    }
}

// ---------------------------------------------------------------------------
// The `wasm_bindgen` surface. Only the JavaScript object exchange lives here;
// the trip-driving core above stays testable on the host.
// ---------------------------------------------------------------------------

#[cfg(target_arch = "wasm32")]
mod javascript_surface {
    use super::*;
    use crate::exports::{
        ERROR_CODE_INVALID_NAVIGATION_INPUT, failure_with_code, geographic_coordinate_object,
        javascript_object_field, object_from_fields, required_javascript_object,
        route_data_from_javascript, validated_javascript_coordinate,
    };
    use crate::generated_contract_field_names::*;
    use js_sys::Object;
    use wasm_bindgen::prelude::*;

    /// Accepts `"metric"` as the unit-system argument.
    const UNIT_SYSTEM_LABEL_METRIC: &str = "metric";
    /// Accepts `"imperial"` as the unit-system argument.
    const UNIT_SYSTEM_LABEL_IMPERIAL: &str = "imperial";

    /// Field name of a trip snapshot's instruction payload.
    const STATE_FIELD_INSTRUCTION: &str = "instruction";

    // --- Browser navigation events (M31) ---
    /// Event field naming one navigation topic.
    const EVENT_FIELD_TOPIC: &str = "topic";
    /// Event field carrying the trip snapshot the event describes.
    const EVENT_FIELD_SNAPSHOT: &str = "snapshot";
    /// Event field carrying the speech text of a speech-topic event.
    const EVENT_FIELD_SPEECH_TEXT: &str = "speechText";
    /// Event field naming a phase transition on a phase-topic event.
    const EVENT_FIELD_PHASE_CHANGE: &str = "phaseChange";
    /// The instructions topic (every dispatched fix).
    const EVENT_TOPIC_INSTRUCTION: &str = "instruction";
    /// The phase topic (one event per transition).
    const EVENT_TOPIC_PHASE: &str = "phase";
    /// The deviation topic (entry, updates and clearing).
    const EVENT_TOPIC_DEVIATION: &str = "deviation";
    /// The speech topic (a new non-empty instruction).
    const EVENT_TOPIC_SPEECH: &str = "speech";
    /// The wasm-only stop topic (the trip ended).
    const EVENT_TOPIC_STOP: &str = "stop";
    /// Phase transition: the trip entered the off-route phase.
    const EVENT_PHASE_CHANGE_OFF_ROUTE_ENTERED: &str = "offRouteEntered";
    /// Phase transition: a graded fix lifted the deviation.
    const EVENT_PHASE_CHANGE_OFF_ROUTE_CLEARED: &str = "offRouteCleared";
    /// Phase transition: the trip arrived.
    const EVENT_PHASE_CHANGE_ARRIVED: &str = "arrived";
    /// Phase transition: the route was replaced by a reroute.
    const EVENT_PHASE_CHANGE_REROUTE_APPLIED: &str = "rerouteApplied";

    /// The phase-topic transition name for one snapshot pair, if it names a
    /// transition the native phase topic reports.
    fn phase_change_name(
        previous_phase: Option<&str>,
        current_phase: &str,
    ) -> Option<&'static str> {
        if current_phase == PHASE_LABEL_ARRIVED {
            return Some(EVENT_PHASE_CHANGE_ARRIVED);
        }
        if current_phase == PHASE_LABEL_OFF_ROUTE && previous_phase != Some(PHASE_LABEL_OFF_ROUTE) {
            return Some(EVENT_PHASE_CHANGE_OFF_ROUTE_ENTERED);
        }
        if previous_phase == Some(PHASE_LABEL_OFF_ROUTE) && current_phase == PHASE_LABEL_NAVIGATING
        {
            return Some(EVENT_PHASE_CHANGE_OFF_ROUTE_CLEARED);
        }
        None
    }
    /// Field name of a trip snapshot's arrival flag.
    const STATE_FIELD_IS_ARRIVED: &str = "isArrived";
    /// Field name of a trip snapshot's current step index.
    const STATE_FIELD_CURRENT_STEP_INDEX: &str = "currentStepIndex";

    /// Field name of an instruction payload's active maneuver.
    const INSTRUCTION_FIELD_ACTIVE_MANEUVER: &str = "activeManeuver";
    /// Field name of an instruction payload's upcoming maneuver.
    const INSTRUCTION_FIELD_UPCOMING_MANEUVER: &str = "upcomingManeuver";
    /// Field name of an instruction payload's progress.
    const INSTRUCTION_FIELD_PROGRESS: &str = "progress";
    /// Field name of an instruction payload's arrival marker.
    const INSTRUCTION_FIELD_IS_ARRIVAL: &str = "isArrival";
    /// Field name of an instruction payload's rerouting-required marker.
    const INSTRUCTION_FIELD_IS_REROUTING_REQUIRED: &str = "isReroutingRequired";

    /// Field name of a maneuver's distance phrase text.
    const MANEUVER_FIELD_DISTANCE_PHRASE_TEXT: &str = "distancePhraseText";
    /// Field name of a maneuver's display text.
    const MANEUVER_FIELD_DISPLAY_TEXT: &str = "displayText";
    /// The maneuver's text-to-speech variant: the provider's speech text with
    /// the same distance prefix, so the page can speak exactly the contract's
    /// speech channel instead of reusing the banner sentence.
    const MANEUVER_FIELD_SPEECH_TEXT: &str = "speechText";
    /// Field name of a maneuver's provider-authored display text.
    const MANEUVER_FIELD_PROVIDER_DISPLAY_TEXT: &str = "providerDisplayText";

    /// Field name of a deviation's measured distance from the route.
    const DEVIATION_FIELD_DISTANCE_FROM_ROUTE_METERS: &str = "distanceFromRouteMeters";

    /// Parses the unit-system argument, refusing any other spelling.
    fn unit_system_from_javascript(unit_system: &str) -> Result<UnitSystem, String> {
        match unit_system {
            UNIT_SYSTEM_LABEL_METRIC => Ok(UnitSystem::Metric),
            UNIT_SYSTEM_LABEL_IMPERIAL => Ok(UnitSystem::Imperial),
            other => Err(format!(
                "the unit system must be \"{UNIT_SYSTEM_LABEL_METRIC}\" or \
                 \"{UNIT_SYSTEM_LABEL_IMPERIAL}\", not `{other}`"
            )),
        }
    }

    /// Reads one nullable finite JavaScript number field: absent, `null` and
    /// `undefined` all answer `None`.
    fn optional_javascript_number_field(
        object: &Object,
        field_name: &str,
    ) -> Result<Option<f64>, String> {
        let field_value = javascript_object_field(object, field_name)?;
        if field_value.is_null() || field_value.is_undefined() {
            return Ok(None);
        }
        let field_number = field_value
            .as_f64()
            .ok_or_else(|| format!("the field {field_name} must be a number"))?;
        if !field_number.is_finite() {
            return Err(format!("the field {field_name} must be finite"));
        }
        Ok(Some(field_number))
    }

    /// Parses one plain location-update object into the native fix record.
    fn navigation_location_update_from_javascript(
        update: &JsValue,
    ) -> Result<NavigationLocationUpdate, String> {
        let update_object = required_javascript_object(update, "the location update")?;
        Ok(NavigationLocationUpdate {
            coordinate: validated_javascript_coordinate(&javascript_object_field(
                update_object,
                LOCATION_UPDATE_FIELD_COORDINATE,
            )?)?,
            horizontal_accuracy_meters: optional_javascript_number_field(
                update_object,
                LOCATION_UPDATE_FIELD_HORIZONTAL_ACCURACY_METERS,
            )?,
            course_heading_degrees: optional_javascript_number_field(
                update_object,
                LOCATION_UPDATE_FIELD_COURSE_HEADING_DEGREES,
            )?,
            timestamp_seconds: optional_javascript_number_field(
                update_object,
                LOCATION_UPDATE_FIELD_TIMESTAMP_SECONDS,
            )?,
        })
    }

    /// Builds the plain `{distancePhraseText, displayText, providerDisplayText}`
    /// maneuver object; the two provider-or-composed fields are `null` where
    /// their variant does not exist.
    fn maneuver_object(
        distance_phrase_text: Option<&str>,
        display_text: &str,
        provider_display_text: Option<&str>,
        speech_text: &str,
    ) -> Result<Object, JsValue> {
        let mut fields = vec![
            (MANEUVER_FIELD_DISPLAY_TEXT, JsValue::from_str(display_text)),
            (MANEUVER_FIELD_SPEECH_TEXT, JsValue::from_str(speech_text)),
        ];
        if let Some(distance_phrase_text) = distance_phrase_text {
            fields.push((
                MANEUVER_FIELD_DISTANCE_PHRASE_TEXT,
                JsValue::from_str(distance_phrase_text),
            ));
        }
        fields.push((
            MANEUVER_FIELD_PROVIDER_DISPLAY_TEXT,
            provider_display_text.map_or(JsValue::NULL, JsValue::from_str),
        ));
        object_from_fields(&fields)
    }

    /// Builds the plain progress object.
    fn progress_object(progress: &ProgressSnapshot) -> Result<Object, JsValue> {
        object_from_fields(&[
            (
                PROGRESS_FIELD_DISTANCE_TO_NEXT_MANEUVER_METERS,
                JsValue::from(progress.distance_to_next_maneuver_meters),
            ),
            (
                PROGRESS_FIELD_DISTANCE_REMAINING_METERS,
                JsValue::from(progress.distance_remaining_meters),
            ),
            (
                PROGRESS_FIELD_DURATION_REMAINING_SECONDS,
                JsValue::from(progress.duration_remaining_seconds),
            ),
        ])
    }

    /// Builds the plain instruction object of one snapshot.
    fn instruction_object(instruction: &InstructionSnapshot) -> Result<Object, JsValue> {
        let upcoming_maneuver = match &instruction.upcoming_maneuver {
            Some(upcoming) => {
                maneuver_object(None, &upcoming.display_text, None, &upcoming.speech_text)?.into()
            }
            None => JsValue::NULL,
        };
        object_from_fields(&[
            (
                INSTRUCTION_FIELD_ACTIVE_MANEUVER,
                maneuver_object(
                    Some(&instruction.active_maneuver.distance_phrase_text),
                    &instruction.active_maneuver.display_text,
                    instruction.active_maneuver.provider_display_text.as_deref(),
                    &instruction.active_maneuver.speech_text,
                )?
                .into(),
            ),
            (INSTRUCTION_FIELD_UPCOMING_MANEUVER, upcoming_maneuver),
            (
                INSTRUCTION_FIELD_PROGRESS,
                progress_object(&instruction.progress)?.into(),
            ),
            (
                INSTRUCTION_FIELD_IS_ARRIVAL,
                JsValue::from(instruction.is_arrival),
            ),
            (
                INSTRUCTION_FIELD_IS_REROUTING_REQUIRED,
                JsValue::from(instruction.is_rerouting_required),
            ),
        ])
    }

    /// Builds the plain deviation object, or `null` without one.
    fn deviation_value(deviation: Option<&DeviationSnapshot>) -> Result<JsValue, JsValue> {
        match deviation {
            Some(deviation) => Ok(object_from_fields(&[
                (
                    DEVIATION_FIELD_STATUS_CHANGE_KIND,
                    JsValue::from_str(deviation.status_change_kind),
                ),
                (
                    DEVIATION_FIELD_DISTANCE_FROM_ROUTE_METERS,
                    JsValue::from(deviation.distance_from_route_meters),
                ),
            ])?
            .into()),
            None => Ok(JsValue::NULL),
        }
    }

    /// Builds the plain snapped-coordinate object, or `null` when the trip has
    /// not processed a fix yet.
    fn snapped_coordinate_value(
        snapped_coordinate: Option<&GeographicCoordinate>,
    ) -> Result<JsValue, JsValue> {
        match snapped_coordinate {
            Some(coordinate) => Ok(geographic_coordinate_object(coordinate)?.into()),
            None => Ok(JsValue::NULL),
        }
    }

    /// Builds the plain trip-snapshot object the class answers.
    fn snapshot_object(snapshot: &TripSnapshot) -> Result<Object, JsValue> {
        object_from_fields(&[
            (STATE_FIELD_PHASE, JsValue::from_str(snapshot.phase)),
            (
                STATE_FIELD_INSTRUCTION,
                instruction_object(&snapshot.instruction)?.into(),
            ),
            (
                STATE_FIELD_DEVIATION,
                deviation_value(snapshot.deviation.as_ref())?,
            ),
            (
                STATE_FIELD_SNAPPED_COORDINATE,
                snapped_coordinate_value(snapshot.snapped_coordinate.as_ref())?,
            ),
            (STATE_FIELD_IS_ARRIVED, JsValue::from(snapshot.is_arrived)),
            (
                STATE_FIELD_CURRENT_STEP_INDEX,
                JsValue::from(snapshot.current_step_index as f64),
            ),
        ])
    }

    /// Turn-by-turn navigation over a planned route, mirroring the driving and
    /// readback core of the contract's `Navigator`.
    ///
    /// Build it with `new Navigator()`; `startNavigation(route, unitSystem)`
    /// begins a trip over a `Route` exactly as `calculateRoute` answers it
    /// (`unitSystem` is `"metric"` or `"imperial"`), `updateLocation(update)`
    /// consumes one `NavigationLocationUpdate` and answers the trip snapshot
    /// (or `null` without a trip), `applyReroute(route)` swaps the active
    /// trip's route, `stopNavigation()` clears it, and `currentTripState()`
    /// answers the latest snapshot (`null` without a trip).
    ///
    /// The snapshot's `phase` is `"navigating"`, `"offRoute"` or `"arrived"`
    /// (the contract's `Navigating`/`OffRoute`/`Arrived`); `instruction` is the
    /// composed payload (`activeManeuver` with `distancePhraseText` and
    /// `displayText`, an optional `upcomingManeuver`, `progress`, and the
    /// `isArrival`/`isReroutingRequired` markers); `deviation` is
    /// `{statusChangeKind, distanceFromRouteMeters}` exactly while the phase is
    /// off route; `snappedCoordinate` is `{latitude, longitude}` once a fix has
    /// been processed. The event bridge replaces the contract's
    /// foreign-listener trait and delivers the same topics; `hasActiveTrip()`
    /// is the wasm-only helper.
    #[wasm_bindgen(js_name = Navigator)]
    pub struct Navigator {
        /// The trip-driving core.
        controller: NavigatorController,
        /// The JavaScript listeners registered through
        /// [`Navigator::add_navigation_listener`], in registration order.
        listeners: Vec<(u32, js_sys::Function)>,
        /// The identifier the next registered listener receives.
        next_listener_id: u32,
        /// The snapshot the last emitted event described, so a dispatch can
        /// report only what changed (the wasm seam has no native observer
        /// registry; the diff plays that role).
        last_emitted_snapshot: Option<TripSnapshot>,
    }

    #[wasm_bindgen]
    impl Navigator {
        /// Builds the controller with no active trip.
        #[wasm_bindgen(constructor)]
        pub fn new() -> Navigator {
            Navigator {
                controller: NavigatorController::new(),
                listeners: Vec::new(),
                next_listener_id: 1,
                last_emitted_snapshot: None,
            }
        }

        /// Registers one navigation listener and answers its identifier. The
        /// callback receives one plain event per topic: `{topic, snapshot,
        /// speechText?, phaseChange?}` where `topic` is `"instruction"`,
        /// `"phase"`, `"deviation"`, `"speech"` or `"stop"`. This is the wasm
        /// seam's event bridge (the contract's foreign-listener trait cannot
        /// cross the wasm-bindgen boundary); the topics and payloads mirror
        /// the native navigation listener's.
        #[wasm_bindgen(js_name = addNavigationListener)]
        pub fn add_navigation_listener(&mut self, listener: js_sys::Function) -> u32 {
            let listener_id = self.next_listener_id;
            self.next_listener_id += 1;
            self.listeners.push((listener_id, listener));
            listener_id
        }

        /// Removes one listener; answers whether it was registered.
        #[wasm_bindgen(js_name = removeNavigationListener)]
        pub fn remove_navigation_listener(&mut self, listener_id: u32) -> bool {
            let previous_length = self.listeners.len();
            self.listeners
                .retain(|(registered_id, _)| *registered_id != listener_id);
            self.listeners.len() != previous_length
        }

        /// Begins navigating `route` with the SDK default policies, replacing
        /// any active trip. Throws a message for a route the conversion cannot
        /// parse or a unit system other than `"metric"`/`"imperial"`.
        #[wasm_bindgen(js_name = startNavigation)]
        pub fn start_navigation(
            &mut self,
            route: JsValue,
            unit_system: &str,
        ) -> Result<(), JsValue> {
            let native_route = route_data_from_javascript(&route).map_err(|message| {
                failure_with_code(ERROR_CODE_INVALID_NAVIGATION_INPUT, &message)
            })?;
            let native_unit_system =
                unit_system_from_javascript(unit_system).map_err(|message| {
                    failure_with_code(ERROR_CODE_INVALID_NAVIGATION_INPUT, &message)
                })?;
            self.controller
                .start_navigation(native_route, native_unit_system);
            let snapshot = self.controller.current_trip_state();
            if let Some(snapshot) = &snapshot {
                self.emit_navigation_event(EVENT_TOPIC_INSTRUCTION, Some(snapshot), None, None);
                self.emit_navigation_event(EVENT_TOPIC_PHASE, Some(snapshot), None, None);
                let speech_text = &snapshot.instruction.active_maneuver.speech_text;
                if !speech_text.is_empty() {
                    self.emit_navigation_event(
                        EVENT_TOPIC_SPEECH,
                        Some(snapshot),
                        Some(speech_text),
                        None,
                    );
                }
            }
            self.last_emitted_snapshot = snapshot;
            Ok(())
        }

        /// Delivers one position fix to the active trip and answers the
        /// resulting snapshot, or `null` without an active trip (the fix is
        /// then dropped). Throws a message for a malformed update object.
        #[wasm_bindgen(js_name = updateLocation)]
        pub fn update_location(&mut self, update: JsValue) -> Result<JsValue, JsValue> {
            let native_update =
                navigation_location_update_from_javascript(&update).map_err(|message| {
                    failure_with_code(ERROR_CODE_INVALID_NAVIGATION_INPUT, &message)
                })?;
            match self.controller.update_location(native_update) {
                Some(snapshot) => {
                    let previous_snapshot = self.last_emitted_snapshot.clone();
                    self.emit_snapshot_changes(previous_snapshot.as_ref(), &snapshot);
                    self.last_emitted_snapshot = Some(snapshot.clone());
                    Ok(snapshot_object(&snapshot)?.into())
                }
                None => Ok(JsValue::NULL),
            }
        }

        /// Replaces the active trip's route (a reroute); without an active trip
        /// the route is dropped. Throws a message for a route the conversion
        /// cannot parse.
        #[wasm_bindgen(js_name = applyReroute)]
        pub fn apply_reroute(&mut self, route: JsValue) -> Result<(), JsValue> {
            let native_route = route_data_from_javascript(&route).map_err(|message| {
                failure_with_code(ERROR_CODE_INVALID_NAVIGATION_INPUT, &message)
            })?;
            self.controller.apply_reroute(native_route);
            if let Some(snapshot) = self.controller.current_trip_state() {
                // A reroute is a phase-topic transition even while the
                // snapshot's phase stays navigating; the instruction is
                // re-announced because its route changed.
                self.emit_navigation_event(
                    EVENT_TOPIC_PHASE,
                    Some(&snapshot),
                    None,
                    Some(EVENT_PHASE_CHANGE_REROUTE_APPLIED),
                );
                self.emit_navigation_event(EVENT_TOPIC_INSTRUCTION, Some(&snapshot), None, None);
                self.last_emitted_snapshot = Some(snapshot);
            }
            Ok(())
        }

        /// Ends the active trip: `currentTripState()` answers `null` afterwards
        /// and location updates and reroutes are dropped until the next
        /// `startNavigation`.
        #[wasm_bindgen(js_name = stopNavigation)]
        pub fn stop_navigation(&mut self) {
            self.controller.stop_navigation();
            self.emit_navigation_event(EVENT_TOPIC_STOP, None, None, None);
            self.last_emitted_snapshot = None;
        }

        /// Emits the topics one state change touched: every dispatched fix
        /// carries the instruction topic (the native observer's rule), a
        /// changed phase carries the phase topic, a changed deviation carries
        /// the deviation topic, and a new non-empty instruction announces the
        /// speech topic.
        fn emit_snapshot_changes(
            &self,
            previous_snapshot: Option<&TripSnapshot>,
            current_snapshot: &TripSnapshot,
        ) {
            let previous_phase = previous_snapshot.map(|previous| previous.phase);
            let phase_changed = previous_phase != Some(current_snapshot.phase);
            let previous_deviation =
                previous_snapshot.and_then(|previous| previous.deviation.as_ref());
            let deviation_changed = previous_deviation != current_snapshot.deviation.as_ref();
            let previous_speech_text = previous_snapshot
                .map(|previous| previous.instruction.active_maneuver.speech_text.as_str());
            let speech_text = current_snapshot
                .instruction
                .active_maneuver
                .speech_text
                .as_str();

            self.emit_navigation_event(EVENT_TOPIC_INSTRUCTION, Some(current_snapshot), None, None);
            if phase_changed {
                let phase_change = phase_change_name(previous_phase, current_snapshot.phase);
                self.emit_navigation_event(
                    EVENT_TOPIC_PHASE,
                    Some(current_snapshot),
                    None,
                    phase_change,
                );
            }
            if deviation_changed {
                self.emit_navigation_event(
                    EVENT_TOPIC_DEVIATION,
                    Some(current_snapshot),
                    None,
                    None,
                );
            }
            if !speech_text.is_empty() && previous_speech_text != Some(speech_text) {
                self.emit_navigation_event(
                    EVENT_TOPIC_SPEECH,
                    Some(current_snapshot),
                    Some(speech_text),
                    None,
                );
            }
        }

        /// Builds one event object and hands it to every live listener. A
        /// listener exception or an object-building failure never disturbs the
        /// trip: navigation is the primary job, events are best effort.
        fn emit_navigation_event(
            &self,
            topic: &str,
            snapshot: Option<&TripSnapshot>,
            speech_text: Option<&str>,
            phase_change: Option<&str>,
        ) {
            if self.listeners.is_empty() {
                return;
            }
            let snapshot_value = match snapshot {
                Some(snapshot) => match snapshot_object(snapshot) {
                    Ok(snapshot_object) => JsValue::from(snapshot_object),
                    Err(_) => JsValue::NULL,
                },
                None => JsValue::NULL,
            };
            let mut fields = vec![
                (EVENT_FIELD_TOPIC, JsValue::from_str(topic)),
                (EVENT_FIELD_SNAPSHOT, snapshot_value),
            ];
            if let Some(speech_text) = speech_text {
                fields.push((EVENT_FIELD_SPEECH_TEXT, JsValue::from_str(speech_text)));
            }
            if let Some(phase_change) = phase_change {
                fields.push((EVENT_FIELD_PHASE_CHANGE, JsValue::from_str(phase_change)));
            }
            let Ok(event) = object_from_fields(&fields) else {
                return;
            };
            for (_, listener) in &self.listeners {
                let _ = listener.call1(&JsValue::NULL, &event);
            }
        }

        /// The latest trip snapshot, or `null` without an active trip.
        #[wasm_bindgen(js_name = currentTripState)]
        pub fn current_trip_state(&self) -> Result<JsValue, JsValue> {
            match self.controller.current_trip_state() {
                Some(snapshot) => Ok(snapshot_object(&snapshot)?.into()),
                None => Ok(JsValue::NULL),
            }
        }

        /// Whether a trip is active.
        #[wasm_bindgen(js_name = hasActiveTrip)]
        pub fn has_active_trip(&self) -> bool {
            self.controller.has_active_trip()
        }
    }

    impl Default for Navigator {
        /// Builds the empty controller, exactly like [`Navigator::new`].
        fn default() -> Self {
            Self::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use geospatial_types::route::{RouteLeg, RouteStep};
    use geospatial_types::{BoundingBox, ManeuverType};

    /// Latitude, in degrees, of the synthetic fixture origin.
    const TEST_ORIGIN_LATITUDE_DEGREES: f64 = 47.0;
    /// Longitude, in degrees, of the synthetic fixture origin.
    const TEST_ORIGIN_LONGITUDE_DEGREES: f64 = 8.0;
    /// Earth radius, in meters, of the fixture's flat-earth offset.
    const TEST_EARTH_RADIUS_METERS: f64 = 6_371_000.0;
    /// Length, in meters, of each synthetic route step.
    const TEST_STEP_LENGTH_METERS: f64 = 100.0;
    /// Travel time, in seconds, of each synthetic route step.
    const TEST_STEP_DURATION_SECONDS: f64 = 10.0;
    /// Number of steps in the synthetic route.
    const TEST_STEP_COUNT: usize = 2;
    /// Horizontal accuracy, in meters, of the synthetic fixes.
    const TEST_ACCURACY_METERS: f64 = 2.0;
    /// East offset, in meters, of the on-route fix.
    const TEST_ROUTE_FIX_EAST_METERS: f64 = 10.0;
    /// North offset, in meters, of the off-route fix: far beyond the default
    /// deviation allowance (the 25 m threshold widened by the fix accuracy).
    const TEST_OFF_ROUTE_NORTH_METERS: f64 = 200.0;

    /// A coordinate displaced east/north of the fixture origin.
    fn offset_coordinate(east_meters: f64, north_meters: f64) -> GeographicCoordinate {
        let mut coordinate = GeographicCoordinate {
            latitude: TEST_ORIGIN_LATITUDE_DEGREES,
            longitude: TEST_ORIGIN_LONGITUDE_DEGREES,
        };
        coordinate.longitude += (east_meters
            / (TEST_EARTH_RADIUS_METERS * TEST_ORIGIN_LATITUDE_DEGREES.to_radians().cos()))
        .to_degrees();
        coordinate.latitude += (north_meters / TEST_EARTH_RADIUS_METERS).to_degrees();
        coordinate
    }

    /// An eastward route whose step texts start with `step_text_prefix`.
    fn synthetic_route(step_text_prefix: &str) -> RouteData {
        let total_length_meters = TEST_STEP_LENGTH_METERS * TEST_STEP_COUNT as f64;
        let steps = (0..TEST_STEP_COUNT)
            .map(|step_number| {
                let start_offset = step_number as f64 * TEST_STEP_LENGTH_METERS;
                RouteStep {
                    distance_meters: TEST_STEP_LENGTH_METERS,
                    duration_seconds: TEST_STEP_DURATION_SECONDS,
                    maneuver_type: if step_number == 0 {
                        ManeuverType::Departure
                    } else {
                        ManeuverType::Arrival
                    },
                    instruction_display_text: format!("{step_text_prefix} step {step_number}"),
                    instruction_speech_text: format!("{step_text_prefix} step {step_number}"),
                    shape: vec![
                        offset_coordinate(start_offset, 0.0),
                        offset_coordinate(start_offset + TEST_STEP_LENGTH_METERS, 0.0),
                    ],
                    lane_guidances: Vec::new(),
                }
            })
            .collect();
        RouteData {
            distance_meters: total_length_meters,
            duration_seconds: TEST_STEP_DURATION_SECONDS * TEST_STEP_COUNT as f64,
            bounding_box: BoundingBox {
                south_west: offset_coordinate(0.0, 0.0),
                north_east: offset_coordinate(total_length_meters, 0.0),
            },
            geometry: Vec::new(),
            legs: vec![RouteLeg {
                distance_meters: total_length_meters,
                duration_seconds: TEST_STEP_DURATION_SECONDS * TEST_STEP_COUNT as f64,
                steps,
            }],
        }
    }

    /// A graded fix displaced east/north of the route line.
    fn route_fix(east_meters: f64, north_meters: f64) -> NavigationLocationUpdate {
        NavigationLocationUpdate {
            coordinate: offset_coordinate(east_meters, north_meters),
            horizontal_accuracy_meters: Some(TEST_ACCURACY_METERS),
            course_heading_degrees: None,
            timestamp_seconds: None,
        }
    }

    #[test]
    fn one_fix_on_the_route_answers_a_navigating_snapshot_with_an_instruction() {
        let mut controller = NavigatorController::new();
        assert!(!controller.has_active_trip());
        controller.start_navigation(synthetic_route("Synthetic"), UnitSystem::Metric);
        assert!(controller.has_active_trip());

        let snapshot = controller
            .update_location(route_fix(TEST_ROUTE_FIX_EAST_METERS, 0.0))
            .expect("an active trip answers a snapshot");
        assert_eq!(snapshot.phase, PHASE_LABEL_NAVIGATING);
        assert_eq!(snapshot.current_step_index, 0);
        assert!(!snapshot.is_arrived);
        assert!(!snapshot.instruction.active_maneuver.display_text.is_empty());
        assert!(
            snapshot
                .instruction
                .active_maneuver
                .display_text
                .contains("Synthetic step 0")
        );
        // The provider text is carried verbatim beside the composed sentence,
        // so a banner that already shows the distance phrase does not repeat
        // it (the native cards' two-line shape).
        assert_eq!(
            snapshot
                .instruction
                .active_maneuver
                .provider_display_text
                .as_deref(),
            Some("Synthetic step 0"),
        );
        assert!(
            !snapshot
                .instruction
                .active_maneuver
                .distance_phrase_text
                .is_empty()
        );
        assert!(snapshot.instruction.upcoming_maneuver.is_some());
        assert!(
            !snapshot
                .instruction
                .upcoming_maneuver
                .as_ref()
                .expect("the second step is the upcoming maneuver")
                .display_text
                .is_empty()
        );
        assert!(
            snapshot
                .instruction
                .progress
                .distance_to_next_maneuver_meters
                .is_finite()
        );
        assert!(snapshot.instruction.progress.distance_remaining_meters > 0.0);
        assert!(snapshot.instruction.progress.duration_remaining_seconds > 0.0);
        assert!(!snapshot.instruction.is_arrival);
        assert!(!snapshot.instruction.is_rerouting_required);
        assert!(snapshot.deviation.is_none());
        assert!(snapshot.snapped_coordinate.is_some());
    }

    #[test]
    fn an_off_route_fix_beyond_the_deviation_threshold_reports_off_route() {
        let mut controller = NavigatorController::new();
        controller.start_navigation(synthetic_route("Synthetic"), UnitSystem::Metric);
        controller
            .update_location(route_fix(TEST_ROUTE_FIX_EAST_METERS, 0.0))
            .expect("the on-route fix answers a snapshot");

        let snapshot = controller
            .update_location(route_fix(
                TEST_ROUTE_FIX_EAST_METERS,
                TEST_OFF_ROUTE_NORTH_METERS,
            ))
            .expect("the off-route fix answers a snapshot");
        assert_eq!(snapshot.phase, PHASE_LABEL_OFF_ROUTE);
        assert!(snapshot.instruction.is_rerouting_required);
        assert!(!snapshot.instruction.active_maneuver.display_text.is_empty());
        let deviation = snapshot
            .deviation
            .as_ref()
            .expect("an off-route trip carries its deviation");
        assert_eq!(
            deviation.status_change_kind,
            DEVIATION_STATUS_CHANGE_KIND_OFF_ROUTE
        );
        assert!(deviation.distance_from_route_meters > TEST_OFF_ROUTE_NORTH_METERS / 2.0);
    }

    #[test]
    fn stopping_the_navigation_clears_the_trip_state() {
        let mut controller = NavigatorController::new();
        controller.start_navigation(synthetic_route("Synthetic"), UnitSystem::Metric);
        controller
            .update_location(route_fix(TEST_ROUTE_FIX_EAST_METERS, 0.0))
            .expect("an active trip answers a snapshot");

        controller.stop_navigation();
        assert!(!controller.has_active_trip());
        assert!(controller.current_trip_state().is_none());
        assert!(
            controller
                .update_location(route_fix(TEST_ROUTE_FIX_EAST_METERS, 0.0))
                .is_none()
        );
    }

    #[test]
    fn a_second_start_replaces_the_active_trip() {
        let mut controller = NavigatorController::new();
        controller.start_navigation(synthetic_route("First"), UnitSystem::Metric);
        controller
            .update_location(route_fix(TEST_ROUTE_FIX_EAST_METERS, 0.0))
            .expect("an active trip answers a snapshot");

        controller.start_navigation(synthetic_route("Replacement"), UnitSystem::Metric);
        let snapshot = controller
            .current_trip_state()
            .expect("the replacement trip is active");
        assert_eq!(snapshot.phase, PHASE_LABEL_NAVIGATING);
        assert_eq!(snapshot.current_step_index, 0);
        assert!(
            snapshot
                .instruction
                .active_maneuver
                .display_text
                .contains("Replacement step 0")
        );
    }

    #[test]
    fn a_reroute_replaces_the_active_trips_route() {
        let mut controller = NavigatorController::new();
        controller.start_navigation(synthetic_route("First"), UnitSystem::Metric);
        controller
            .update_location(route_fix(TEST_ROUTE_FIX_EAST_METERS, 0.0))
            .expect("an active trip answers a snapshot");

        controller.apply_reroute(synthetic_route("Rerouted"));
        let snapshot = controller
            .current_trip_state()
            .expect("the rerouted trip is active");
        assert!(
            snapshot
                .instruction
                .active_maneuver
                .display_text
                .contains("Rerouted step 0")
        );
    }
}
