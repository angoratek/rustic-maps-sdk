//! Turn-by-turn navigation state, instructions, policies and speech.

use super::{GeographicCoordinate, Route};

/// Pace chosen by the application for a simulated trip. The SDK carries no
/// default pace; validation happens in `rustic-maps-navigation`'s route walker.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimulatedTripPace {
    /// Baseline travel speed in meters per second.
    pub speed_meters_per_second: f64,
    /// Multiplier applied to the baseline speed.
    pub speed_multiplier: f64,
}

impl SimulatedTripPace {
    /// Builds the native pace from the mirror record. Values cross unvalidated
    /// (foreign code may hand anything across); the walker refuses a pace that
    /// cannot drive a trip.
    pub(crate) fn to_native_pace(self) -> rustic_maps_navigation::SimulatedTripPace {
        rustic_maps_navigation::SimulatedTripPace {
            speed_meters_per_second: self.speed_meters_per_second,
            speed_multiplier: self.speed_multiplier,
        }
    }
}

/// One position of a simulated trip along the planned route.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimulatedTripFix {
    /// Position on the planned route's geometry.
    pub coordinate: GeographicCoordinate,
    /// Direction of travel along the active segment, in degrees clockwise
    /// from true north, or `None` when the route has no segment.
    pub course_heading_degrees: Option<f64>,
    /// Distance travelled from the route start, in meters.
    pub travelled_meters: f64,
    /// Distance left to the route end, in meters.
    pub remaining_meters: f64,
    /// Simulated elapsed time from the trip start, in seconds.
    pub elapsed_seconds: f64,
    /// Whether the fix sits at the route's final coordinate.
    pub is_arrived: bool,
}

impl SimulatedTripFix {
    /// Builds the mirror record from a native simulated-trip fix.
    pub(crate) fn from_native_fix(native_fix: &rustic_maps_navigation::SimulatedTripFix) -> Self {
        Self {
            coordinate: GeographicCoordinate::from_native_coordinate(&native_fix.coordinate),
            course_heading_degrees: native_fix.course_heading_degrees,
            travelled_meters: native_fix.travelled_meters,
            remaining_meters: native_fix.remaining_meters,
            elapsed_seconds: native_fix.elapsed_seconds,
            is_arrived: native_fix.is_arrived,
        }
    }
}

/// High-level phase of a trip that has already been started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationPhase {
    /// Following the route's step sequence.
    Navigating,
    /// The deviation detector judged the last graded fix off route; progress
    /// is frozen at the last on-route values until a graded fix recovers or a
    /// reroute replaces the route.
    OffRoute,
    /// The final step has been completed; location updates no longer change
    /// the state.
    Arrived,
}

impl NavigationPhase {
    /// Builds the mirror enum from the native navigation phase.
    pub(crate) fn from_native_navigation_phase(
        native_navigation_phase: rustic_maps_navigation::NavigationPhase,
    ) -> Self {
        match native_navigation_phase {
            rustic_maps_navigation::NavigationPhase::Navigating => Self::Navigating,
            rustic_maps_navigation::NavigationPhase::OffRoute => Self::OffRoute,
            rustic_maps_navigation::NavigationPhase::Arrived => Self::Arrived,
        }
    }
}

/// Which grade of deviation a recorded deviation carries.
///
/// The two grades of the two-grade deviation distinction: off the *current
/// step* yet still on the route line versus completely off the route. Only
/// the fully-off grade freezes navigation; the wrong-step grade never freezes
/// on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviationGradeKind {
    /// The fix projects onto neither the current step nor any future step of
    /// the remaining route line: the freezing grade. The trip's progress is
    /// frozen until a graded fix recovers or a reroute replaces the route.
    FullyOffRoute,
    /// The fix is off the current step but projects onto a future step of the
    /// remaining route line: "on route, wrong step". The trip keeps
    /// navigating and counting down, and is never frozen by this grade alone.
    OffStepOnRoute,
}

impl DeviationGradeKind {
    /// Builds the mirror enum from the native deviation grade.
    pub(crate) fn from_native_deviation_grade(
        native_deviation_grade: rustic_maps_navigation::DeviationGrade,
    ) -> Self {
        match native_deviation_grade {
            rustic_maps_navigation::DeviationGrade::FullyOffRoute => Self::FullyOffRoute,
            rustic_maps_navigation::DeviationGrade::OffStepOnRoute => Self::OffStepOnRoute,
        }
    }
}

/// Plain-data description of an active deviation from the route.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeviationInfo {
    /// Which grade of deviation this is (see [`DeviationGradeKind`]).
    pub grade_kind: DeviationGradeKind,
    /// Distance, in meters, from the raw fix that triggered (or most recently
    /// confirmed) the deviation to the remaining route line, as measured and
    /// judged by the deviation detector.
    pub deviation_from_route_meters: f64,
}

impl DeviationInfo {
    /// Builds the mirror record from the native deviation description.
    pub(crate) fn from_native_deviation_info(
        native_deviation_info: &rustic_maps_navigation::DeviationInfo,
    ) -> Self {
        Self {
            grade_kind: DeviationGradeKind::from_native_deviation_grade(
                native_deviation_info.grade,
            ),
            deviation_from_route_meters: native_deviation_info.deviation_from_route_meters,
        }
    }
}

/// Location of one route step in traversal order across a route's legs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteStepPosition {
    /// Index of the leg that owns this step in the route's legs.
    pub leg_index: u64,
    /// Index of the step within its leg in the leg's steps.
    pub step_index: u64,
}

impl RouteStepPosition {
    /// Builds the mirror record from the native step position.
    pub(crate) fn from_native_route_step_position(
        native_route_step_position: &rustic_maps_navigation::RouteStepPosition,
    ) -> Self {
        Self {
            leg_index: native_route_step_position.leg_index as u64,
            step_index: native_route_step_position.step_index as u64,
        }
    }
}

/// Progress along the remaining trip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TripProgress {
    /// Distance, in meters, from the snapped position to the end of the
    /// current step — the point of the next maneuver.
    pub distance_to_next_maneuver_meters: f64,
    /// Total remaining distance, in meters, including the current step's
    /// remainder.
    pub distance_remaining_meters: f64,
    /// Total remaining travel time, in seconds, measured the same way as
    /// `distance_remaining_meters`.
    pub duration_remaining_seconds: f64,
}

impl TripProgress {
    /// Builds the mirror record from the native progress record.
    pub(crate) fn from_native_trip_progress(
        native_trip_progress: &rustic_maps_navigation::TripProgress,
    ) -> Self {
        Self {
            distance_to_next_maneuver_meters: native_trip_progress.distance_to_next_maneuver_meters,
            distance_remaining_meters: native_trip_progress.distance_remaining_meters,
            duration_remaining_seconds: native_trip_progress.duration_remaining_seconds,
        }
    }
}

/// The full navigable state of an active trip, read-only for foreign code.
///
/// The steps of [`Self::route`] are carried as traversal-ordered indices
/// (`step_positions`), never duplicated; the route's steps themselves arrive
/// through the [`super::Route`] record, which carries no lane guidance. The
/// live step-advance condition and deviation detector are not records and
/// cannot cross the FFI; they are configured through the navigation policies
/// on the controller, matching the native session's pairing.
#[derive(Debug, Clone, PartialEq)]
pub struct TripNavigationState {
    /// Current phase of the trip.
    pub phase: NavigationPhase,
    /// The route being navigated.
    pub route: Route,
    /// All steps across every leg, flattened in traversal order.
    pub step_positions: Vec<RouteStepPosition>,
    /// Index into `step_positions` of the step currently traversed.
    pub current_step_position: u64,
    /// Progress along the remaining trip.
    pub progress: TripProgress,
    /// Position snapped onto the current step's shape; `None` until the first
    /// location update has been processed.
    pub snapped_coordinate: Option<GeographicCoordinate>,
    /// Active deviation, as judged by the deviation detector. Set exactly
    /// while the phase is `OffRoute`.
    pub deviation: Option<DeviationInfo>,
    /// Whether a reroute recalculation is currently in flight (the reroute
    /// gate's observable readback). This is a controller wall-clock
    /// fact, not reducer state, so the pure conversion below cannot know it;
    /// it always builds `false` here and the only caller — the exported
    /// controller's trip-state readback — overlays its gate's live value.
    pub is_calculating_new_route: bool,
}

impl TripNavigationState {
    /// Builds the mirror record from the native trip state.
    pub(crate) fn from_native_trip_navigation_state(
        native_trip_navigation_state: &rustic_maps_navigation::TripNavigationState,
    ) -> Self {
        Self {
            phase: NavigationPhase::from_native_navigation_phase(
                native_trip_navigation_state.phase,
            ),
            route: Route::from_native_route_data(&native_trip_navigation_state.route),
            step_positions: native_trip_navigation_state
                .step_positions
                .iter()
                .map(RouteStepPosition::from_native_route_step_position)
                .collect(),
            current_step_position: native_trip_navigation_state.current_step_position as u64,
            progress: TripProgress::from_native_trip_progress(
                &native_trip_navigation_state.progress,
            ),
            snapped_coordinate: native_trip_navigation_state
                .snapped_coordinate
                .as_ref()
                .map(GeographicCoordinate::from_native_coordinate),
            deviation: native_trip_navigation_state
                .deviation
                .as_ref()
                .map(DeviationInfo::from_native_deviation_info),
            // A pure conversion cannot know the controller's wall-clock gate
            // state; the controller's trip-state readback overlays the live
            // value.
            is_calculating_new_route: false,
        }
    }
}

/// Length unit a distance-phrase magnitude is expressed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistanceUnit {
    /// Meters, used by the metric near and moderate bands.
    Meters,
    /// Kilometers, used by the metric far band.
    Kilometers,
    /// Feet, used by the imperial near and moderate bands.
    Feet,
    /// Miles, used by the imperial far band.
    Miles,
}

impl DistanceUnit {
    /// Builds the mirror enum from the native distance unit.
    pub(crate) fn from_native_distance_unit(
        native_distance_unit: rustic_maps_navigation::DistanceUnit,
    ) -> Self {
        match native_distance_unit {
            rustic_maps_navigation::DistanceUnit::Meters => Self::Meters,
            rustic_maps_navigation::DistanceUnit::Kilometers => Self::Kilometers,
            rustic_maps_navigation::DistanceUnit::Feet => Self::Feet,
            rustic_maps_navigation::DistanceUnit::Miles => Self::Miles,
        }
    }
}

/// Which form a distance phrase takes. UDL enums carry no payloads, so the
/// phrase mirrors its three forms under this kind plus the nullable fields of
/// the [`DistancePhrase`] record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistancePhraseKind {
    /// The position is closer than half a near-band increment to the maneuver
    /// point; no distance is phrased and composed text drops the prefix. The
    /// record's magnitude is `None` on this kind.
    Immediate,
    /// A rounded distance magnitude in a specific unit; the record's
    /// magnitude and unit are set on this kind.
    Quantity,
    /// No distance is phrased because the freezing (fully-off-route) deviation
    /// grade holds: the projection is frozen, so any countdown composed off it
    /// would be a phantom measurement. The record's magnitude is `None` on
    /// this kind, and the composed text channels carry no distance prefix.
    Suppressed,
}

/// The unit-system-aware phrasing of the distance remaining to a maneuver,
/// rounded into the native distance phrasing bands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DistancePhrase {
    /// Which form the phrase takes (see [`DistancePhraseKind`]).
    pub phrase_kind: DistancePhraseKind,
    /// Rounded magnitude of the phrase (half-up, per the native bands);
    /// `None` for the immediate and suppressed forms.
    pub magnitude: Option<f64>,
    /// Unit the magnitude is expressed in; `None` for the immediate and
    /// suppressed forms.
    pub magnitude_unit: Option<DistanceUnit>,
}

impl DistancePhrase {
    /// Builds the mirror record from the native distance phrase.
    pub(crate) fn from_native_distance_phrase(
        native_distance_phrase: &rustic_maps_navigation::DistancePhrase,
    ) -> Self {
        match *native_distance_phrase {
            rustic_maps_navigation::DistancePhrase::Immediate => Self {
                phrase_kind: DistancePhraseKind::Immediate,
                magnitude: None,
                magnitude_unit: None,
            },
            rustic_maps_navigation::DistancePhrase::Quantity { magnitude, unit } => Self {
                phrase_kind: DistancePhraseKind::Quantity,
                magnitude: Some(magnitude),
                magnitude_unit: Some(DistanceUnit::from_native_distance_unit(unit)),
            },
            rustic_maps_navigation::DistancePhrase::Suppressed => Self {
                phrase_kind: DistancePhraseKind::Suppressed,
                magnitude: None,
                magnitude_unit: None,
            },
        }
    }
}

/// Which form an instruction text takes. UDL enums carry no payloads, so the
/// text mirrors its two forms under this kind plus the nullable fields of the
/// [`ManeuverInstructionText`] record; all four text fields are `None` on the
/// unavailable kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManeuverInstructionTextKind {
    /// The provider authored instruction text for this maneuver.
    Composed,
    /// The provider authored no instruction text for this maneuver; nothing
    /// is invented to fill it.
    Unavailable,
}

/// The composed text channels of one maneuver: provider-authored text is
/// carried verbatim beside the composed strings (the composed display variant
/// is the live distance prefix plus the provider display text, the composed
/// speech variant the prefix plus the provider speech text, and an empty
/// provider variant falls back to the other, still provider-authored one).
#[derive(Debug, Clone, PartialEq)]
pub struct ManeuverInstructionText {
    /// Which form the text takes (see [`ManeuverInstructionTextKind`]).
    pub text_kind: ManeuverInstructionTextKind,
    /// Provider-authored display text, verbatim and never rephrased.
    pub provider_display_text: Option<String>,
    /// Provider-authored speech text, verbatim and never rephrased.
    pub provider_speech_text: Option<String>,
    /// Banner variant: the live distance prefix plus the display variant.
    pub composed_display_text: Option<String>,
    /// Text-to-speech variant: the live distance prefix plus the speech
    /// variant.
    pub composed_speech_text: Option<String>,
}

impl ManeuverInstructionText {
    /// Builds the mirror record from the native text enum.
    pub(crate) fn from_native_maneuver_instruction_text(
        native_maneuver_instruction_text: &rustic_maps_navigation::ManeuverInstructionText,
    ) -> Self {
        match native_maneuver_instruction_text {
            rustic_maps_navigation::ManeuverInstructionText::Composed {
                provider_display_text,
                provider_speech_text,
                display_text,
                speech_text,
            } => Self {
                text_kind: ManeuverInstructionTextKind::Composed,
                provider_display_text: Some(provider_display_text.clone()),
                provider_speech_text: Some(provider_speech_text.clone()),
                composed_display_text: Some(display_text.clone()),
                composed_speech_text: Some(speech_text.clone()),
            },
            rustic_maps_navigation::ManeuverInstructionText::Unavailable => Self {
                text_kind: ManeuverInstructionTextKind::Unavailable,
                provider_display_text: None,
                provider_speech_text: None,
                composed_display_text: None,
                composed_speech_text: None,
            },
        }
    }
}

/// Instruction payload of the maneuver currently being traversed (the banner
/// maneuver: the step the snapped position sits inside). Mirrors the native
/// active maneuver except for its lane-guidance list, which stays native
/// until the route records carry lane guidance (see [`RouteStep`](super::RouteStep)).
#[derive(Debug, Clone, PartialEq)]
pub struct ActiveManeuverInstruction {
    /// Composed display and speech text of the active step.
    pub instruction: ManeuverInstructionText,
    /// Unit-system-aware distance phrasing of the distance to the next
    /// maneuver.
    pub distance_phrase: DistancePhrase,
    /// Structured exit index of a roundabout active step, starting at one
    /// (`None` for every other maneuver).
    pub roundabout_exit_index: Option<u8>,
}

impl ActiveManeuverInstruction {
    /// Builds the mirror record from the native active maneuver instruction.
    pub(crate) fn from_native_active_maneuver_instruction(
        native_active_maneuver_instruction: &rustic_maps_navigation::ActiveManeuverInstruction,
    ) -> Self {
        Self {
            instruction: ManeuverInstructionText::from_native_maneuver_instruction_text(
                &native_active_maneuver_instruction.instruction,
            ),
            distance_phrase: DistancePhrase::from_native_distance_phrase(
                &native_active_maneuver_instruction.distance_phrase,
            ),
            roundabout_exit_index: native_active_maneuver_instruction.roundabout_exit_index,
        }
    }
}

/// Instruction payload of the maneuver after the active one (the "then"
/// instruction), text-carrying only.
#[derive(Debug, Clone, PartialEq)]
pub struct UpcomingManeuverInstruction {
    /// Composed display and speech text of the following step.
    pub instruction: ManeuverInstructionText,
    /// Structured exit index when the following step is a roundabout.
    pub roundabout_exit_index: Option<u8>,
}

impl UpcomingManeuverInstruction {
    /// Builds the mirror record from the native upcoming maneuver instruction.
    pub(crate) fn from_native_upcoming_maneuver_instruction(
        native_upcoming_maneuver_instruction: &rustic_maps_navigation::UpcomingManeuverInstruction,
    ) -> Self {
        Self {
            instruction: ManeuverInstructionText::from_native_maneuver_instruction_text(
                &native_upcoming_maneuver_instruction.instruction,
            ),
            roundabout_exit_index: native_upcoming_maneuver_instruction.roundabout_exit_index,
        }
    }
}

/// The emitted turn-by-turn instruction payload for one trip state, mirroring
/// the native instruction record one-to-one (except the native active
/// maneuver's lane-guidance list, which stays native until the route records
/// carry lane guidance). Emission is pure in the native core, so the
/// same state yields the same payload on every read.
#[derive(Debug, Clone, PartialEq)]
pub struct NavigationInstruction {
    /// The banner maneuver currently being traversed.
    pub active_maneuver: ActiveManeuverInstruction,
    /// The maneuver after the active one, where the route has a following
    /// step (always `None` on the route's final step and after arrival).
    pub upcoming_maneuver: Option<UpcomingManeuverInstruction>,
    /// `true` exactly while the trip has arrived and the remaining progress
    /// is zero.
    pub is_arrival: bool,
    /// `true` exactly while the trip is off route: the payload beneath the
    /// marker is the frozen last-on-route projection and must not be
    /// presented as current guidance.
    pub is_rerouting_required: bool,
}

impl NavigationInstruction {
    /// Builds the mirror record from the native instruction payload.
    pub(crate) fn from_native_navigation_instruction(
        native_navigation_instruction: &rustic_maps_navigation::NavigationInstruction,
    ) -> Self {
        Self {
            active_maneuver: ActiveManeuverInstruction::from_native_active_maneuver_instruction(
                &native_navigation_instruction.active_maneuver,
            ),
            upcoming_maneuver: native_navigation_instruction
                .upcoming_maneuver
                .as_ref()
                .map(UpcomingManeuverInstruction::from_native_upcoming_maneuver_instruction),
            is_arrival: native_navigation_instruction.is_arrival,
            is_rerouting_required: native_navigation_instruction.is_rerouting_required,
        }
    }
}

/// Which form a deviation-status update takes. UDL enums carry no payloads,
/// so the update mirrors its two forms under this kind plus the nullable
/// field of the [`DeviationStatusChange`] record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviationStatusChangeKind {
    /// The trip is judged off route; the record's deviation description is
    /// set on this kind.
    OffRoute,
    /// A previously recorded deviation cleared; the trip is judged on route.
    OnRoute,
}

/// A deviation-topic update: the detector's judgment changed between the
/// previous and the current trip state — entry, measurement drift while the
/// deviation persists, and clearing.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviationStatusChange {
    /// Which form the update takes (see [`DeviationStatusChangeKind`]).
    pub status_change_kind: DeviationStatusChangeKind,
    /// The recorded measurement of the deviation, set exactly on the
    /// off-route kind.
    pub deviation_info: Option<DeviationInfo>,
}

impl DeviationStatusChange {
    /// Builds the mirror record from the native deviation-status update.
    pub(crate) fn from_native_deviation_status_change(
        native_deviation_status_change: &rustic_maps_navigation::DeviationStatusChange,
    ) -> Self {
        match native_deviation_status_change {
            rustic_maps_navigation::DeviationStatusChange::OffRoute(deviation_info) => Self {
                status_change_kind: DeviationStatusChangeKind::OffRoute,
                deviation_info: Some(DeviationInfo::from_native_deviation_info(deviation_info)),
            },
            rustic_maps_navigation::DeviationStatusChange::OnRoute => Self {
                status_change_kind: DeviationStatusChangeKind::OnRoute,
                deviation_info: None,
            },
        }
    }
}

/// A phase-transition event delivered on the phase topic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationPhaseChange {
    /// The trip entered the off-route phase from the navigating phase.
    OffRouteEntered,
    /// A graded fix back on the route line lifted the deviation and returned
    /// the trip to the navigating phase.
    OffRouteCleared,
    /// A reroute event replaced the route (delivered only when the event
    /// actually applied — an ignored reroute produces no transition).
    RerouteApplied,
    /// The final step completed; the trip reached the arrived phase.
    Arrived,
}

impl NavigationPhaseChange {
    /// Builds the mirror enum from the native phase transition.
    pub(crate) fn from_native_navigation_phase_change(
        native_navigation_phase_change: &rustic_maps_navigation::NavigationPhaseChange,
    ) -> Self {
        match *native_navigation_phase_change {
            rustic_maps_navigation::NavigationPhaseChange::OffRouteEntered => Self::OffRouteEntered,
            rustic_maps_navigation::NavigationPhaseChange::OffRouteCleared => Self::OffRouteCleared,
            rustic_maps_navigation::NavigationPhaseChange::RerouteApplied => Self::RerouteApplied,
            rustic_maps_navigation::NavigationPhaseChange::Arrived => Self::Arrived,
        }
    }
}

/// One raw position fix delivered to the navigation controller; mirrors the
/// native fix record field-for-field.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NavigationLocationUpdate {
    /// Raw position on the WGS 84 ellipsoid, before route snapping.
    pub coordinate: GeographicCoordinate,
    /// Radius, in meters, of the 68% horizontal-accuracy envelope of this fix
    /// (`None` when the source does not report accuracy). Fixes without an
    /// accuracy report never trigger a step advance and are never judged by
    /// the deviation detector.
    pub horizontal_accuracy_meters: Option<f64>,
    /// Direction of travel in degrees clockwise from true north (`None` when
    /// the source does not report a course).
    pub course_heading_degrees: Option<f64>,
    /// Fix instant in seconds since the Unix epoch (`None` for untimed
    /// sources).
    pub timestamp_seconds: Option<f64>,
}

impl NavigationLocationUpdate {
    /// Builds the native fix record from the mirror record.
    pub(crate) fn into_native_location_update(
        self,
    ) -> rustic_maps_navigation::NavigationLocationUpdate {
        rustic_maps_navigation::NavigationLocationUpdate {
            coordinate: self.coordinate.to_native_coordinate(),
            horizontal_accuracy_meters: self.horizontal_accuracy_meters,
            course_heading_degrees: self.course_heading_degrees,
            timestamp_seconds: self.timestamp_seconds,
        }
    }
}

/// Which step-advance strategy a navigation policy configures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepAdvancePolicyKind {
    /// Steps change only when driven programmatically; this binding surface
    /// exposes no programmatic step advance, so the trip tracks progress
    /// without changing steps.
    Manual,
    /// Advances when the position fix comes within the threshold of the end
    /// of the current step, eager-advance semantics
    /// (`DistanceToEndOfStepCondition`). Thresholds from
    /// [crate::navigation::default_step_advance_policy] are the documented
    /// defaults.
    DistanceToEndOfStep,
    /// Advances when the position fix moves farther from the current step's
    /// polyline than the threshold, delayed-advance semantics
    /// (`DistanceFromStepCondition`).
    DistanceFromStep,
}

/// The step-advance policy of a trip: mirrors the native decision objects
/// that cannot cross the FFI as one tagged record. The `Manual` kind ignores
/// the threshold fields; both distance kinds require finite, non-negative
/// threshold values (refused otherwise at the controller boundary).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StepAdvancePolicy {
    /// Which step-advance strategy the policy configures.
    pub policy_kind: StepAdvancePolicyKind,
    /// Distance, in meters, of the advance threshold (the distance to the
    /// step's end, or from the step's polyline, depending on the kind).
    pub advance_threshold_meters: f64,
    /// Fixes with a reported horizontal accuracy worse than this, in meters,
    /// never advance.
    pub minimum_horizontal_accuracy_meters: f64,
}

/// Which deviation-detection strategy a deviation policy configures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviationPolicyKind {
    /// Flags a deviation when the raw fix is farther from the remaining route
    /// line than the configured threshold, widened by the fix's reported
    /// accuracy (`DistanceDeviationDetector`).
    DistanceThreshold,
}

/// The deviation-detection policy of a trip: mirrors the native decision
/// object that cannot cross the FFI. The threshold must be finite and
/// non-negative (refused otherwise at the controller boundary).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeviationPolicy {
    /// Which deviation-detection strategy the policy configures.
    pub policy_kind: DeviationPolicyKind,
    /// Distance, in meters, from the remaining route line beyond which a fix
    /// is judged off route (before any accuracy widening, strictly greater).
    pub deviation_threshold_meters: f64,
}

/// The reroute-recalculation hysteresis policy of a controller: mirrors the
/// native decision values of
/// `rustic_maps_navigation::ReroutePolicy`. Both thresholds must be finite and
/// non-negative (refused otherwise at the controller boundary).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReroutePolicy {
    /// Minimum wall-clock seconds between the beginnings of two reroute
    /// calculations.
    pub recalculation_cooldown_seconds: f64,
    /// Minimum displacement, in meters, between the raw position fix current
    /// at a calculation's beginning and the one current at the previous
    /// calculation's beginning.
    pub recalculation_movement_meters: f64,
}

/// What triggered a speech-instruction line (the speech-instruction topic).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeechInstructionKind {
    /// A trigger band was crossed this fix (the far, moderate or near band).
    ManeuverApproach,
    /// The immediate band: the maneuver is imminent.
    ImmediateManeuver,
    /// Guidance just recovered from off-route; guidance resumed.
    OffRouteRecovery,
    /// The trip arrived.
    Arrival,
}

/// One speech-trigger line the navigation controller's pacing policy judged
/// newsworthy; the speech text is the composed payload's verbatim channel.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeechInstruction {
    /// The verbatim composed speech text (never rewritten by the trigger
    /// policy).
    pub utterance_speech_text: String,
    /// The live distance to the maneuver, in meters, at emission time.
    pub trigger_distance_meters: f64,
    /// The utterance identity (a controller-ordered integer, monotonic across
    /// the controller's lifetime): a stable monotonic
    /// id composed at emission — strictly increasing over every line the
    /// composing planner's owning controller has emitted since construction,
    /// never reused, and stable for the single delivery of one line.
    /// Identical text re-emitted is a new id; dedup and flush decisions stay
    /// at the platform speaker.
    pub utterance_id: u64,
    /// What triggered this line.
    pub instruction_kind: SpeechInstructionKind,
}

impl SpeechInstruction {
    /// Builds the mirror record from the native utterance.
    pub(crate) fn from_native_speech_utterance(
        native_utterance: &rustic_maps_navigation::SpeechUtterance,
    ) -> Self {
        Self {
            utterance_speech_text: native_utterance.utterance_speech_text.clone(),
            trigger_distance_meters: native_utterance.trigger_distance_meters,
            utterance_id: native_utterance.utterance_id,
            instruction_kind: match native_utterance.kind {
                rustic_maps_navigation::SpeechUtteranceKind::ManeuverApproach => {
                    SpeechInstructionKind::ManeuverApproach
                }
                rustic_maps_navigation::SpeechUtteranceKind::ImmediateManeuver => {
                    SpeechInstructionKind::ImmediateManeuver
                }
                rustic_maps_navigation::SpeechUtteranceKind::OffRouteRecovery => {
                    SpeechInstructionKind::OffRouteRecovery
                }
                rustic_maps_navigation::SpeechUtteranceKind::Arrival => {
                    SpeechInstructionKind::Arrival
                }
            },
        }
    }
}
