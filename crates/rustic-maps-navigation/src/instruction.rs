//! Runtime turn-by-turn instruction composition for an active trip.
//!
//! [`compose_navigation_instructions`] is a *pure* function of the current
//! [`TripNavigationState`] (whose `route` field supplies the provider-authored
//! steps) plus a [`UnitSystem`]: it produces the plain-data
//! [`NavigationInstruction`] payload a UI banner or a text-to-speech engine
//! consumes. The record is deliberately serializable — no `Arc`, no trait
//! objects — so it can cross a language boundary unchanged.
//!
//! # Provider text is never rewritten
//!
//! The emitter performs no natural-language understanding and does not
//! rephrase, summarize or spell-normalize the provider-authored instruction
//! strings ([`RouteStep::instruction_display_text`] and
//! [`RouteStep::instruction_speech_text`]). The only text it authors is the
//! live distance prefix (`"In 130 meters"`), composed as
//! `"<prefix>, <provider text>"`. With no distance left the prefix disappears
//! entirely and the payload is the provider text verbatim. Where the provider
//! authored no text at all, the payload is the explicit
//! [`ManeuverInstructionText::Unavailable`] state — nothing is invented to fill
//! it. When only one of the two variants is empty, the other (still
//! provider-authored) variant is reused for both channels rather than
//! synthesizing text: choosing which authored string to speak is not
//! authoring.
//!
//! # Off route: an explicit rerouting-required marker
//!
//! While the phase is [`NavigationPhase::OffRoute`] the reducer freezes the
//! whole projection, so the composed payload below the marker is *the last
//! on-route instruction*, unchanged across successive deviating fixes. Rather
//! than presenting that stale instruction as trustworthy guidance, the payload
//! carries an explicit [`NavigationInstruction::is_rerouting_required`] marker;
//! a UI uses it to swap the banner for a rerouting notice, and the marker
//! clears as soon as the deviation lifts or a reroute replaces the route.
//!
//! # Off route: the frozen countdown is suppressed, not presented
//!
//! A frozen projection has no trustworthy distance to the next maneuver: any
//! countdown composed off it would read as live guidance measured from where
//! the device is not. While the freezing (`FullyOffRoute`) grade holds, the
//! active maneuver's phrase is therefore the explicit
//! [`DistancePhrase::Suppressed`] state — no magnitude is phrased and the
//! distance prefix drops from both text channels — mirroring the
//! [`ManeuverInstructionText::Unavailable`] posture of composing nothing
//! rather than composing something false. The provider-authored text is
//! carried verbatim beneath the rerouting-required marker exactly as before.
//! A merely wrong-step grade (`OffStepOnRoute`) keeps counting down: its
//! fix still projects onto the route line, so the measured distance is real.
//!
//! # Distance phrasing bands
//!
//! [`DistancePhrase`] rounds the raw distance to a named, boundary-tested
//! band so successive fixes phrase identically while a user walks the span
//! ("In 130 meters" instead of "In 131 meters", "In 127 meters", …). The
//! pinned bands, rounded half-up:
//!
//! | Band | Metric | Imperial |
//! |---|---|---|
//! | Immediate | below 5 m (= half the near increment) | below 50 ft (= half the near increment) |
//! | Near | below 100 m: nearest 10 m | below 1000 ft: nearest 100 ft |
//! | Moderate | 100 m–1000 m: nearest 50 m | 1000 ft–1 mile: nearest 500 ft |
//! | Far | 1 km and up: nearest 0.1 km | 1 mile and up: nearest 0.1 mile |
//!
//! The immediate band exists because half-up rounding of a sub-increment
//! distance would otherwise phrase a 4 m approach as "In 0 meters".

use geospatial_types::{LaneGuidance, ManeuverType, RouteStep, UnitSystem};

use crate::state::{DeviationGrade, NavigationPhase, TripNavigationState};

/// Metric conversion of meters into kilometers for the far metric band.
const METERS_PER_KILOMETER: f64 = 1000.0;

/// Exact conversion factor of meters into (international) feet.
pub(crate) const METERS_PER_FOOT: f64 = 0.3048;

/// Exact conversion factor of feet into miles.
const FEET_PER_MILE: f64 = 5280.0;

/// Metric near-band rounding increment, in meters.
///
/// Metric distances below [`METRIC_MODERATE_DISTANCE_THRESHOLD_METERS`] (the
/// near band) are rounded half-up to the nearest multiple of this increment
/// and phrased in meters.
pub(crate) const METRIC_NEAR_ROUNDING_INCREMENT_METERS: f64 = 10.0;

/// Upper bound, in meters, of the metric near band (exclusive).
///
/// At this distance and above, the metric moderate band takes over (see
/// [`METRIC_MODERATE_ROUNDING_INCREMENT_METERS`]).
pub(crate) const METRIC_MODERATE_DISTANCE_THRESHOLD_METERS: f64 = 100.0;

/// Metric moderate-band rounding increment, in meters.
///
/// Metric distances from
/// [`METRIC_MODERATE_DISTANCE_THRESHOLD_METERS`] up to (but excluding)
/// [`METRIC_FAR_DISTANCE_THRESHOLD_METERS`] are rounded half-up to the nearest
/// multiple of this increment and phrased in meters.
const METRIC_MODERATE_ROUNDING_INCREMENT_METERS: f64 = 50.0;

/// Lower bound, in meters, of the metric far band (inclusive).
///
/// At this distance and above, phrasing switches to kilometers rounded to
/// [`METRIC_FAR_ROUNDING_INCREMENT_KILOMETERS`].
pub(crate) const METRIC_FAR_DISTANCE_THRESHOLD_METERS: f64 = 1000.0;

/// Metric far-band rounding increment, in kilometers.
///
/// Kilometer magnitudes in the far band are rounded half-up to the nearest
/// multiple of this increment.
const METRIC_FAR_ROUNDING_INCREMENT_KILOMETERS: f64 = 0.1;

/// Metric immediate-band threshold, in meters (exclusive).
///
/// Below half of the near-band increment, half-up rounding would produce a
/// zero magnitude; the phrase becomes [`DistancePhrase::Immediate`] instead.
pub(crate) const METRIC_IMMEDIATE_DISTANCE_THRESHOLD_METERS: f64 =
    METRIC_NEAR_ROUNDING_INCREMENT_METERS / 2.0;

/// Imperial near-band rounding increment, in feet.
///
/// Imperial distances below
/// [`IMPERIAL_MODERATE_DISTANCE_THRESHOLD_FEET`] (the near band) are rounded
/// half-up to the nearest multiple of this increment and phrased in feet.
pub(crate) const IMPERIAL_NEAR_ROUNDING_INCREMENT_FEET: f64 = 100.0;

/// Upper bound, in feet, of the imperial near band (exclusive).
///
/// At this distance and above, the imperial moderate band takes over (see
/// [`IMPERIAL_MODERATE_ROUNDING_INCREMENT_FEET`]).
pub(crate) const IMPERIAL_MODERATE_DISTANCE_THRESHOLD_FEET: f64 = 1000.0;

/// Imperial moderate-band rounding increment, in feet.
///
/// Imperial distances from [`IMPERIAL_MODERATE_DISTANCE_THRESHOLD_FEET`] up
/// to (but excluding) [`IMPERIAL_FAR_DISTANCE_THRESHOLD_FEET`] are rounded
/// half-up to the nearest multiple of this increment and phrased in feet.
const IMPERIAL_MODERATE_ROUNDING_INCREMENT_FEET: f64 = 500.0;

/// Lower bound, in feet, of the imperial far band (inclusive): one mile.
///
/// At this distance and above, phrasing switches to miles rounded to
/// [`IMPERIAL_FAR_ROUNDING_INCREMENT_MILES`].
pub(crate) const IMPERIAL_FAR_DISTANCE_THRESHOLD_FEET: f64 = FEET_PER_MILE;

/// Imperial far-band rounding increment, in miles.
///
/// Mile magnitudes in the far band are rounded half-up to the nearest
/// multiple of this increment.
const IMPERIAL_FAR_ROUNDING_INCREMENT_MILES: f64 = 0.1;

/// Imperial immediate-band threshold, in feet (exclusive).
///
/// Below half of the near-band increment, half-up rounding would produce a
/// zero magnitude; the phrase becomes [`DistancePhrase::Immediate`] instead.
pub(crate) const IMPERIAL_IMMEDIATE_DISTANCE_THRESHOLD_FEET: f64 =
    IMPERIAL_NEAR_ROUNDING_INCREMENT_FEET / 2.0;

/// Length unit a [`DistancePhrase::Quantity`] magnitude is expressed in.
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

/// The unit-system-aware phrasing of the distance remaining to a maneuver,
/// rounded into the named bands documented in this module's head.
///
/// A phrase is data, not formatting state, so a UI can re-render it; the
/// canonical text is available through [`DistancePhrase::display_text`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DistancePhrase {
    /// The snapped position is closer than half a near-band increment to the
    /// maneuver point; rounding would read "In 0 meters", so no distance is
    /// phrased and composed text drops the prefix entirely.
    Immediate,
    /// A rounded distance magnitude in the given unit.
    Quantity {
        /// Rounded magnitude (half-up, per the band constants).
        magnitude: f64,
        /// Unit the magnitude is expressed in.
        unit: DistanceUnit,
    },
    /// No distance is phrased because the freezing (fully-off-route) grade
    /// holds: the projection is frozen, so any countdown composed off it
    /// would be a phantom measurement (see the module-head decision).
    /// Compose-time only — [`DistancePhrase::for_distance`] never returns it.
    Suppressed,
}

impl DistancePhrase {
    /// Phrases the distance from the current position to the next maneuver
    /// in the given unit system.
    ///
    /// A (impossible-by-construction) negative distance is clamped to zero:
    /// rounding it would produce a negative magnitude rather than an error.
    pub fn for_distance(distance_meters: f64, unit_system: UnitSystem) -> Self {
        let distance_meters = distance_meters.max(0.0);
        match unit_system {
            UnitSystem::Metric => Self::phrase_metric(distance_meters),
            UnitSystem::Imperial => Self::phrase_imperial(distance_meters),
        }
    }

    /// The canonical consumer-facing text of the phrase, e.g. `"130 meters"`,
    /// `"1.5 kilometers"`, `"1500 feet"`, `"0.2 miles"`, or `"now"` for
    /// [`DistancePhrase::Immediate`]. The explicit-empty countdown line for
    /// [`DistancePhrase::Suppressed`] is the empty string: the suppression
    /// state itself is the payload's honesty, not a phrased number.
    pub fn display_text(&self) -> String {
        match self {
            Self::Immediate => "now".to_string(),
            Self::Quantity { magnitude, unit } => quantity_display_text(*magnitude, *unit),
            Self::Suppressed => String::new(),
        }
    }

    /// Phrases a metric distance through the metric bands.
    fn phrase_metric(distance_meters: f64) -> Self {
        if distance_meters < METRIC_IMMEDIATE_DISTANCE_THRESHOLD_METERS {
            return Self::Immediate;
        }
        if distance_meters < METRIC_MODERATE_DISTANCE_THRESHOLD_METERS {
            return Self::Quantity {
                magnitude: round_to_increment(
                    distance_meters,
                    METRIC_NEAR_ROUNDING_INCREMENT_METERS,
                ),
                unit: DistanceUnit::Meters,
            };
        }
        if distance_meters < METRIC_FAR_DISTANCE_THRESHOLD_METERS {
            return Self::Quantity {
                magnitude: round_to_increment(
                    distance_meters,
                    METRIC_MODERATE_ROUNDING_INCREMENT_METERS,
                ),
                unit: DistanceUnit::Meters,
            };
        }
        Self::Quantity {
            magnitude: round_to_increment(
                distance_meters / METERS_PER_KILOMETER,
                METRIC_FAR_ROUNDING_INCREMENT_KILOMETERS,
            ),
            unit: DistanceUnit::Kilometers,
        }
    }

    /// Phrases an imperial distance through the imperial bands.
    fn phrase_imperial(distance_meters: f64) -> Self {
        let distance_feet = distance_meters / METERS_PER_FOOT;
        if distance_feet < IMPERIAL_IMMEDIATE_DISTANCE_THRESHOLD_FEET {
            return Self::Immediate;
        }
        if distance_feet < IMPERIAL_MODERATE_DISTANCE_THRESHOLD_FEET {
            return Self::Quantity {
                magnitude: round_to_increment(distance_feet, IMPERIAL_NEAR_ROUNDING_INCREMENT_FEET),
                unit: DistanceUnit::Feet,
            };
        }
        if distance_feet < IMPERIAL_FAR_DISTANCE_THRESHOLD_FEET {
            return Self::Quantity {
                magnitude: round_to_increment(
                    distance_feet,
                    IMPERIAL_MODERATE_ROUNDING_INCREMENT_FEET,
                ),
                unit: DistanceUnit::Feet,
            };
        }
        Self::Quantity {
            magnitude: round_to_increment(
                distance_feet / FEET_PER_MILE,
                IMPERIAL_FAR_ROUNDING_INCREMENT_MILES,
            ),
            unit: DistanceUnit::Miles,
        }
    }
}

/// Rounds `value` half-up (away from zero) to the nearest multiple of
/// `increment`.
fn round_to_increment(value: f64, increment: f64) -> f64 {
    (value / increment).round() * increment
}

/// Formats a rounded magnitude with its unit's display convention: whole
/// numbers for meters and feet, one decimal for kilometers and miles.
fn quantity_display_text(magnitude: f64, unit: DistanceUnit) -> String {
    match unit {
        DistanceUnit::Meters => format!("{magnitude:.0} meters"),
        DistanceUnit::Kilometers => format!("{magnitude:.1} kilometers"),
        DistanceUnit::Feet => format!("{magnitude:.0} feet"),
        DistanceUnit::Miles => format!("{magnitude:.1} miles"),
    }
}

/// Builds the live distance prefix (`"In 130 meters"`) a composed instruction
/// is prefixed with; `None` for [`DistancePhrase::Immediate`] (where no
/// distance precedes the provider text) and for
/// [`DistancePhrase::Suppressed`] (where no distance may be composed at all).
fn distance_prefix_text(distance_phrase: Option<&DistancePhrase>) -> Option<String> {
    match distance_phrase {
        Some(DistancePhrase::Quantity { magnitude, unit }) => {
            Some(format!("In {}", quantity_display_text(*magnitude, *unit)))
        }
        _ => None,
    }
}

/// The composed text channels of one maneuver.
///
/// Per the module-head decision, provider-authored text is carried verbatim
/// beside the composed strings: `display_text` is the banner variant (the
/// distance prefix plus the provider *display* text) and `speech_text` is the
/// text-to-speech variant (the prefix plus the provider *speech* text). When
/// the provider left one variant empty, the other provider-authored variant
/// fills both channels; when both are empty the maneuver is
/// [`ManeuverInstructionText::Unavailable`] instead of getting invented text.
#[derive(Debug, Clone, PartialEq)]
pub enum ManeuverInstructionText {
    /// The provider authored instruction text for this maneuver.
    Composed {
        /// Provider-authored display text, verbatim and never rephrased.
        provider_display_text: String,
        /// Provider-authored speech text, verbatim and never rephrased.
        provider_speech_text: String,
        /// Banner variant: the live distance prefix plus the provider display
        /// text (falling back to the provider speech text when the display
        /// variant is empty).
        display_text: String,
        /// Text-to-speech variant: the live distance prefix plus the provider
        /// speech text (falling back to the provider display text when the
        /// speech variant is empty).
        speech_text: String,
    },
    /// The provider authored no instruction text for this maneuver; the
    /// emitter invents nothing.
    Unavailable,
}

/// Instruction payload of the maneuver currently being traversed.
///
/// This is the banner maneuver: the step the snapped position sits inside,
/// with the distance measured to that step's end — the maneuver point.
#[derive(Debug, Clone, PartialEq)]
pub struct ActiveManeuverInstruction {
    /// Composed display and speech text of the active step.
    pub instruction: ManeuverInstructionText,
    /// Unit-system-aware distance phrasing of
    /// [`TripProgress::distance_to_next_maneuver_meters`](crate::state::TripProgress::distance_to_next_maneuver_meters).
    pub distance_phrase: DistancePhrase,
    /// Structured exit index of a [`ManeuverType::Roundabout`] active step,
    /// starting at one (`None` for every other maneuver). Surfaced as data —
    /// a UI renders "take the 2nd exit" (or its localized equivalent) from
    /// this field; the emitter performs no string surgery on provider text.
    pub roundabout_exit_index: Option<u8>,
    /// The active step's lane guidance list, promoted as-is: no filtering,
    /// reordering or annotation. A step without lane guidance arrives empty.
    pub lane_guidances: Vec<LaneGuidance>,
}

/// Instruction payload of the maneuver after the active one (the "then"
/// instruction), when the route has a following step.
///
/// It carries text only: no distance is phrased for it (the active maneuver's
/// distance governs the pacing) and no lane guidance is promoted (lanes apply
/// at the active maneuver point).
#[derive(Debug, Clone, PartialEq)]
pub struct UpcomingManeuverInstruction {
    /// Composed display and speech text of the following step.
    pub instruction: ManeuverInstructionText,
    /// Structured exit index when the following step is a
    /// [`ManeuverType::Roundabout`] (see
    /// [`ActiveManeuverInstruction::roundabout_exit_index`]).
    pub roundabout_exit_index: Option<u8>,
}

/// The emitted turn-by-turn instruction payload for one
/// [`TripNavigationState`]: plain data end to end, so it can cross a language
/// boundary unchanged.
///
/// Emission is a pure function of the state: repeated composition of the same
/// state yields the same record, and the frozen state of an off-route trip
/// keeps the payload stable across successive deviating fixes.
#[derive(Debug, Clone, PartialEq)]
pub struct NavigationInstruction {
    /// The banner maneuver currently being traversed.
    pub active_maneuver: ActiveManeuverInstruction,
    /// The maneuver after the active one, where the route has a following
    /// step (always `None` on the route's final step and after arrival).
    pub upcoming_maneuver: Option<UpcomingManeuverInstruction>,
    /// Arrival marker: `true` exactly while the phase
    /// [`NavigationPhase::Arrived`] holds — the trip has completed and the
    /// remaining progress is zero.
    pub is_arrival: bool,
    /// Rerouting-required marker: `true` exactly while the phase
    /// [`NavigationPhase::OffRoute`] holds. The payload beneath the marker is
    /// the frozen last-on-route projection, and its distance phrase is
    /// suppressed (see the module documentation); the marker tells the
    /// consumer the instruction must not be presented as current guidance.
    pub is_rerouting_required: bool,
}

/// Composes the instruction payload for the current trip state.
///
/// The route is read from [`TripNavigationState::route`], so the signature is
/// effectively the (route, state) pair the emission contract names; the
/// function is pure and has no interior state.
///
/// # Parameters
/// - `state` — the current trip state, including the route and current step.
/// - `unit_system` — whether distances are phrased in metric or imperial
///   units.
///
/// # Returns
/// The banner and speech payload for the state. When the route has no current
/// step, the instruction text reports
/// [`ManeuverInstructionText::Unavailable`]; the arrival and rerouting markers
/// follow the state's phase.
pub fn compose_navigation_instructions(
    trip_state: &TripNavigationState,
    unit_system: UnitSystem,
) -> NavigationInstruction {
    let current_step = trip_state.current_step();
    let upcoming_step = trip_state.step_at(trip_state.current_step_position + 1);
    // A frozen projection has no trustworthy distance to the next maneuver,
    // so the phrase is suppressed instead of composed off it (see the
    // module-head decision); a merely wrong-step grade keeps counting down.
    let suppresses_distance_phrase = trip_state.phase == NavigationPhase::OffRoute
        || trip_state
            .deviation
            .is_some_and(|deviation_info| deviation_info.grade == DeviationGrade::FullyOffRoute);
    let distance_phrase = if suppresses_distance_phrase {
        DistancePhrase::Suppressed
    } else {
        DistancePhrase::for_distance(
            trip_state.progress.distance_to_next_maneuver_meters,
            unit_system,
        )
    };
    NavigationInstruction {
        active_maneuver: ActiveManeuverInstruction {
            instruction: compose_step_instruction(current_step, Some(&distance_phrase)),
            distance_phrase,
            roundabout_exit_index: roundabout_exit_index_of(current_step),
            lane_guidances: current_step
                .map(|step| step.lane_guidances.clone())
                .unwrap_or_default(),
        },
        upcoming_maneuver: upcoming_step.map(|upcoming_step| UpcomingManeuverInstruction {
            instruction: compose_step_instruction(Some(upcoming_step), None),
            roundabout_exit_index: roundabout_exit_index_of(Some(upcoming_step)),
        }),
        is_arrival: trip_state.phase == NavigationPhase::Arrived,
        is_rerouting_required: trip_state.phase == NavigationPhase::OffRoute,
    }
}

/// Composes one step's display and speech channels, prefixing the provider
/// text with the live distance phrase where one is supplied (the upcoming
/// maneuver gets none).
fn compose_step_instruction(
    step: Option<&RouteStep>,
    distance_phrase: Option<&DistancePhrase>,
) -> ManeuverInstructionText {
    let Some(step) = step else {
        return ManeuverInstructionText::Unavailable;
    };
    if step.instruction_display_text.is_empty() && step.instruction_speech_text.is_empty() {
        return ManeuverInstructionText::Unavailable;
    }
    let distance_prefix = distance_prefix_text(distance_phrase);
    ManeuverInstructionText::Composed {
        provider_display_text: step.instruction_display_text.clone(),
        provider_speech_text: step.instruction_speech_text.clone(),
        display_text: compose_channel(
            distance_prefix.as_deref(),
            &step.instruction_display_text,
            &step.instruction_speech_text,
        ),
        speech_text: compose_channel(
            distance_prefix.as_deref(),
            &step.instruction_speech_text,
            &step.instruction_display_text,
        ),
    }
}

/// Joins the distance prefix with one channel's provider text, falling back
/// to the sibling (still provider-authored) variant when the channel's own
/// text is empty.
fn compose_channel(
    distance_prefix: Option<&str>,
    primary_text: &str,
    fallback_text: &str,
) -> String {
    let chosen_text = if primary_text.is_empty() {
        fallback_text
    } else {
        primary_text
    };
    match distance_prefix {
        Some(distance_prefix) => format!("{distance_prefix}, {chosen_text}"),
        None => chosen_text.to_string(),
    }
}

/// The structured exit index of a roundabout step, `None` otherwise.
fn roundabout_exit_index_of(step: Option<&RouteStep>) -> Option<u8> {
    match step {
        Some(RouteStep {
            maneuver_type: ManeuverType::Roundabout(exit_index),
            ..
        }) => Some(*exit_index),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use geospatial_types::{
        BoundingBox, LaneGuidance, ManeuverType, RouteData, RouteLeg, RouteStep, TurnDirection,
        UnitSystem,
    };

    use super::{
        DistancePhrase, DistanceUnit, ManeuverInstructionText, UpcomingManeuverInstruction,
        compose_navigation_instructions,
    };
    use crate::state::{NavigationPhase, TripNavigationState, TripProgress};
    use crate::test_support::offset_coordinate;

    /// Length, in meters, of each synthetic step.
    const TEST_STEP_LENGTH_METERS: f64 = 300.0;
    /// Declared duration, in seconds, of each synthetic step.
    const TEST_STEP_DURATION_SECONDS: f64 = 20.0;
    /// Float tolerance for asserting rounded magnitudes, in the phrase's own
    /// unit: rounding never lands a magnitude further away than half of the
    /// smallest increment (0.1 km / 0.1 mile), far below this ceiling.
    const TEST_MAGNITUDE_TOLERANCE: f64 = 1e-6;

    /// Builds one plain step pointing east from `start_offset_east_meters`.
    fn eastward_step(start_offset_east_meters: f64, maneuver_type: ManeuverType) -> RouteStep {
        RouteStep {
            distance_meters: TEST_STEP_LENGTH_METERS,
            duration_seconds: TEST_STEP_DURATION_SECONDS,
            maneuver_type,
            instruction_display_text: "Instruction".to_string(),
            instruction_speech_text: "Instruction".to_string(),
            shape: vec![
                offset_coordinate(start_offset_east_meters, 0.0),
                offset_coordinate(start_offset_east_meters + TEST_STEP_LENGTH_METERS, 0.0),
            ],
            lane_guidances: Vec::new(),
        }
    }

    /// Builds a one-leg route from the given steps.
    fn route_with_steps(steps: Vec<RouteStep>) -> RouteData {
        let total_distance_meters = steps.iter().map(|step| step.distance_meters).sum();
        let total_duration_seconds = steps.iter().map(|step| step.duration_seconds).sum();
        let bounding_box = BoundingBox {
            south_west: offset_coordinate(0.0, 0.0),
            north_east: offset_coordinate(total_distance_meters, 0.0),
        };
        RouteData {
            distance_meters: total_distance_meters,
            duration_seconds: total_duration_seconds,
            bounding_box,
            geometry: Vec::new(),
            legs: vec![RouteLeg {
                distance_meters: total_distance_meters,
                duration_seconds: total_duration_seconds,
                steps,
            }],
        }
    }

    /// Builds a trip state navigating `route`, positioned on the step at
    /// `current_step_position` with the given progress snapshot. The step
    /// positions are always built from the state's own route, mirroring what
    /// the reducer guarantees.
    fn navigating_state(route: RouteData, current_step_position: usize) -> TripNavigationState {
        let step_positions = TripNavigationState::build_step_positions(&route);
        let current_step_distance_meters =
            route.legs[0].steps[current_step_position].distance_meters;
        TripNavigationState {
            phase: NavigationPhase::Navigating,
            route,
            step_positions,
            current_step_position,
            progress: TripProgress {
                distance_to_next_maneuver_meters: current_step_distance_meters,
                distance_remaining_meters: current_step_distance_meters,
                duration_remaining_seconds: TEST_STEP_DURATION_SECONDS,
            },
            snapped_coordinate: Some(offset_coordinate(0.0, 0.0)),
            deviation: None,
            course_heading_degrees: None,
        }
    }

    /// Rewrites the active step's instruction texts in place on the route.
    fn with_active_step_texts(
        mut state: TripNavigationState,
        display_text: &str,
        speech_text: &str,
    ) -> TripNavigationState {
        let step_position = &state.step_positions[state.current_step_position];
        let step = &mut state.route.legs[step_position.leg_index].steps[step_position.step_index];
        step.instruction_display_text = display_text.to_string();
        step.instruction_speech_text = speech_text.to_string();
        state
    }

    fn assert_quantity_magnitude(
        distance_phrase: &DistancePhrase,
        expected_magnitude: f64,
        expected_unit: DistanceUnit,
    ) {
        let DistancePhrase::Quantity { magnitude, unit } = distance_phrase else {
            panic!("expected a quantified phrase, got {distance_phrase:?}");
        };
        assert_eq!(unit, &expected_unit);
        assert!(
            (magnitude - expected_magnitude).abs() < TEST_MAGNITUDE_TOLERANCE,
            "expected magnitude {expected_magnitude}, got {magnitude}"
        );
    }

    #[test]
    fn metric_distances_below_half_the_near_increment_phrase_immediate() {
        // 4.9 m rounds to zero under the 10 m near increment; the immediate
        // band keeps the phrase from reading "In 0 meters".
        let phrase = DistancePhrase::for_distance(4.9, UnitSystem::Metric);
        assert_eq!(phrase, DistancePhrase::Immediate);
        let phrase = DistancePhrase::for_distance(5.0, UnitSystem::Metric);
        assert_quantity_magnitude(&phrase, 10.0, DistanceUnit::Meters);
        assert_eq!(phrase.display_text(), "10 meters");
    }

    #[test]
    fn metric_near_band_rounds_to_the_nearest_ten_meters() {
        let phrase = DistancePhrase::for_distance(14.9, UnitSystem::Metric);
        assert_quantity_magnitude(&phrase, 10.0, DistanceUnit::Meters);
        let phrase = DistancePhrase::for_distance(15.0, UnitSystem::Metric);
        assert_quantity_magnitude(&phrase, 20.0, DistanceUnit::Meters);
        let phrase = DistancePhrase::for_distance(94.9, UnitSystem::Metric);
        assert_quantity_magnitude(&phrase, 90.0, DistanceUnit::Meters);
        let phrase = DistancePhrase::for_distance(95.0, UnitSystem::Metric);
        assert_quantity_magnitude(&phrase, 100.0, DistanceUnit::Meters);
    }

    #[test]
    fn metric_near_and_moderate_bands_meet_at_one_hundred_meters() {
        let near_phrase = DistancePhrase::for_distance(99.9, UnitSystem::Metric);
        assert_quantity_magnitude(&near_phrase, 100.0, DistanceUnit::Meters);
        let moderate_phrase = DistancePhrase::for_distance(100.0, UnitSystem::Metric);
        assert_quantity_magnitude(&moderate_phrase, 100.0, DistanceUnit::Meters);
    }

    #[test]
    fn metric_moderate_band_rounds_to_the_nearest_fifty_meters() {
        let phrase = DistancePhrase::for_distance(124.9, UnitSystem::Metric);
        assert_quantity_magnitude(&phrase, 100.0, DistanceUnit::Meters);
        let phrase = DistancePhrase::for_distance(125.0, UnitSystem::Metric);
        assert_quantity_magnitude(&phrase, 150.0, DistanceUnit::Meters);
        let phrase = DistancePhrase::for_distance(974.9, UnitSystem::Metric);
        assert_quantity_magnitude(&phrase, 950.0, DistanceUnit::Meters);
        let phrase = DistancePhrase::for_distance(975.0, UnitSystem::Metric);
        assert_quantity_magnitude(&phrase, 1000.0, DistanceUnit::Meters);
        assert_eq!(phrase.display_text(), "1000 meters");
    }

    #[test]
    fn metric_moderate_and_far_bands_meet_at_one_kilometer() {
        let moderate_phrase = DistancePhrase::for_distance(999.9, UnitSystem::Metric);
        assert_quantity_magnitude(&moderate_phrase, 1000.0, DistanceUnit::Meters);
        let far_phrase = DistancePhrase::for_distance(1000.0, UnitSystem::Metric);
        assert_quantity_magnitude(&far_phrase, 1.0, DistanceUnit::Kilometers);
        assert_eq!(far_phrase.display_text(), "1.0 kilometers");
    }

    #[test]
    fn metric_far_band_rounds_to_the_nearest_tenth_of_a_kilometer() {
        let phrase = DistancePhrase::for_distance(1049.0, UnitSystem::Metric);
        assert_quantity_magnitude(&phrase, 1.0, DistanceUnit::Kilometers);
        let phrase = DistancePhrase::for_distance(1050.0, UnitSystem::Metric);
        assert_quantity_magnitude(&phrase, 1.1, DistanceUnit::Kilometers);
        let phrase = DistancePhrase::for_distance(1500.0, UnitSystem::Metric);
        assert_eq!(phrase.display_text(), "1.5 kilometers");
    }

    #[test]
    fn imperial_distances_below_fifty_feet_phrase_immediate() {
        let below_threshold =
            DistancePhrase::for_distance(49.0 * super::METERS_PER_FOOT, UnitSystem::Imperial);
        assert_eq!(below_threshold, DistancePhrase::Immediate);
        let at_threshold =
            DistancePhrase::for_distance(50.0 * super::METERS_PER_FOOT, UnitSystem::Imperial);
        assert_quantity_magnitude(&at_threshold, 100.0, DistanceUnit::Feet);
        assert_eq!(at_threshold.display_text(), "100 feet");
    }

    #[test]
    fn imperial_near_band_rounds_to_the_nearest_hundred_feet() {
        let phrase =
            DistancePhrase::for_distance(149.0 * super::METERS_PER_FOOT, UnitSystem::Imperial);
        assert_quantity_magnitude(&phrase, 100.0, DistanceUnit::Feet);
        let phrase =
            DistancePhrase::for_distance(150.0 * super::METERS_PER_FOOT, UnitSystem::Imperial);
        assert_quantity_magnitude(&phrase, 200.0, DistanceUnit::Feet);
        let phrase =
            DistancePhrase::for_distance(999.0 * super::METERS_PER_FOOT, UnitSystem::Imperial);
        assert_quantity_magnitude(&phrase, 1000.0, DistanceUnit::Feet);
    }

    #[test]
    fn imperial_near_and_moderate_bands_meet_at_one_thousand_feet() {
        let near_phrase =
            DistancePhrase::for_distance(999.0 * super::METERS_PER_FOOT, UnitSystem::Imperial);
        assert_quantity_magnitude(&near_phrase, 1000.0, DistanceUnit::Feet);
        let moderate_phrase =
            DistancePhrase::for_distance(1000.0 * super::METERS_PER_FOOT, UnitSystem::Imperial);
        assert_quantity_magnitude(&moderate_phrase, 1000.0, DistanceUnit::Feet);
    }

    #[test]
    fn imperial_moderate_band_rounds_to_the_nearest_five_hundred_feet() {
        let phrase =
            DistancePhrase::for_distance(1249.0 * super::METERS_PER_FOOT, UnitSystem::Imperial);
        assert_quantity_magnitude(&phrase, 1000.0, DistanceUnit::Feet);
        let phrase =
            DistancePhrase::for_distance(1250.0 * super::METERS_PER_FOOT, UnitSystem::Imperial);
        assert_quantity_magnitude(&phrase, 1500.0, DistanceUnit::Feet);
        assert_eq!(phrase.display_text(), "1500 feet");
        // The half-up midpoint inside the moderate band's upper reaches; the
        // nearest-increment rule may legitimately overshoot toward the far
        // threshold, and that is pinned here.
        let phrase =
            DistancePhrase::for_distance(5249.0 * super::METERS_PER_FOOT, UnitSystem::Imperial);
        assert_quantity_magnitude(&phrase, 5000.0, DistanceUnit::Feet);
        let phrase =
            DistancePhrase::for_distance(5279.0 * super::METERS_PER_FOOT, UnitSystem::Imperial);
        assert_quantity_magnitude(&phrase, 5500.0, DistanceUnit::Feet);
    }

    #[test]
    fn imperial_moderate_and_far_bands_meet_at_one_mile() {
        let far_phrase =
            DistancePhrase::for_distance(5280.0 * super::METERS_PER_FOOT, UnitSystem::Imperial);
        assert_quantity_magnitude(&far_phrase, 1.0, DistanceUnit::Miles);
        assert_eq!(far_phrase.display_text(), "1.0 miles");
    }

    #[test]
    fn imperial_far_band_rounds_to_the_nearest_tenth_of_a_mile() {
        let phrase =
            DistancePhrase::for_distance(5400.0 * super::METERS_PER_FOOT, UnitSystem::Imperial);
        assert_quantity_magnitude(&phrase, 1.0, DistanceUnit::Miles);
        let phrase =
            DistancePhrase::for_distance(5610.0 * super::METERS_PER_FOOT, UnitSystem::Imperial);
        assert_quantity_magnitude(&phrase, 1.1, DistanceUnit::Miles);
        assert_eq!(phrase.display_text(), "1.1 miles");
    }

    #[test]
    fn unit_system_selects_the_phrase_convention() {
        let distance_meters = 300.0;
        let metric_phrase = DistancePhrase::for_distance(distance_meters, UnitSystem::Metric);
        assert_quantity_magnitude(&metric_phrase, 300.0, DistanceUnit::Meters);
        let imperial_phrase = DistancePhrase::for_distance(distance_meters, UnitSystem::Imperial);
        assert_quantity_magnitude(&imperial_phrase, 1000.0, DistanceUnit::Feet);
    }

    #[test]
    fn active_maneuver_combines_the_distance_prefix_with_the_provider_text() {
        let route = route_with_steps(vec![
            eastward_step(0.0, ManeuverType::Departure),
            eastward_step(TEST_STEP_LENGTH_METERS, ManeuverType::Arrival),
        ]);
        let state = with_active_step_texts(
            navigating_state(route, 0),
            "Turn left onto Harbor Street",
            "Turn left. Onto Harbor Street.",
        );
        let instruction = compose_navigation_instructions(&state, UnitSystem::Metric);
        let ManeuverInstructionText::Composed {
            provider_display_text,
            provider_speech_text,
            display_text,
            speech_text,
        } = &instruction.active_maneuver.instruction
        else {
            panic!("the authored step must compose");
        };
        assert_eq!(provider_display_text, "Turn left onto Harbor Street");
        assert_eq!(provider_speech_text, "Turn left. Onto Harbor Street.");
        // The live distance prefix is authored by the emitter; the provider
        // text follows it verbatim in each channel.
        assert_eq!(display_text, "In 300 meters, Turn left onto Harbor Street");
        assert_eq!(speech_text, "In 300 meters, Turn left. Onto Harbor Street.");
    }

    #[test]
    fn immediate_distance_drops_the_prefix_entirely() {
        let route = route_with_steps(vec![
            eastward_step(0.0, ManeuverType::Departure),
            eastward_step(TEST_STEP_LENGTH_METERS, ManeuverType::Arrival),
        ]);
        let mut state = navigating_state(route, 0);
        state.progress.distance_to_next_maneuver_meters = 3.0;
        let instruction = compose_navigation_instructions(&state, UnitSystem::Metric);
        assert_eq!(
            instruction.active_maneuver.distance_phrase,
            DistancePhrase::Immediate
        );
        let ManeuverInstructionText::Composed { display_text, .. } =
            &instruction.active_maneuver.instruction
        else {
            panic!("the authored step must compose");
        };
        assert_eq!(display_text, "Instruction");
    }

    #[test]
    fn empty_provider_text_composes_the_explicit_unavailable_state() {
        let route = route_with_steps(vec![
            eastward_step(0.0, ManeuverType::Departure),
            eastward_step(TEST_STEP_LENGTH_METERS, ManeuverType::Arrival),
        ]);
        let state = with_active_step_texts(navigating_state(route, 0), "", "");
        let instruction = compose_navigation_instructions(&state, UnitSystem::Metric);
        assert_eq!(
            instruction.active_maneuver.instruction,
            ManeuverInstructionText::Unavailable
        );
    }

    #[test]
    fn an_empty_display_variant_falls_back_to_the_provider_speech_variant() {
        let route = route_with_steps(vec![
            eastward_step(0.0, ManeuverType::Departure),
            eastward_step(TEST_STEP_LENGTH_METERS, ManeuverType::Arrival),
        ]);
        let state = with_active_step_texts(navigating_state(route, 0), "", "Spoken instruction");
        let instruction = compose_navigation_instructions(&state, UnitSystem::Metric);
        let ManeuverInstructionText::Composed {
            display_text,
            speech_text,
            ..
        } = &instruction.active_maneuver.instruction
        else {
            panic!("the step with speech text must compose");
        };
        // Both channels reuse the authored speech variant; nothing is invented.
        assert_eq!(display_text, "In 300 meters, Spoken instruction");
        assert_eq!(speech_text, "In 300 meters, Spoken instruction");
    }

    #[test]
    fn upcoming_maneuver_composes_without_a_distance_prefix() {
        let route = route_with_steps(vec![
            eastward_step(0.0, ManeuverType::Departure),
            eastward_step(TEST_STEP_LENGTH_METERS, ManeuverType::Arrival),
        ]);
        let state = navigating_state(route, 0);
        let instruction = compose_navigation_instructions(&state, UnitSystem::Metric);
        let Some(UpcomingManeuverInstruction { instruction, .. }) = &instruction.upcoming_maneuver
        else {
            panic!("a mid-route state must carry an upcoming maneuver");
        };
        let ManeuverInstructionText::Composed { display_text, .. } = instruction else {
            panic!("the authored upcoming step must compose");
        };
        assert_eq!(display_text, "Instruction");
    }

    #[test]
    fn the_final_step_has_no_upcoming_maneuver() {
        let route = route_with_steps(vec![
            eastward_step(0.0, ManeuverType::Departure),
            eastward_step(TEST_STEP_LENGTH_METERS, ManeuverType::Arrival),
        ]);
        let state = navigating_state(route.clone(), 1);
        let instruction = compose_navigation_instructions(&state, UnitSystem::Metric);
        assert!(instruction.upcoming_maneuver.is_none());
        assert!(!instruction.is_arrival);
    }

    #[test]
    fn lane_guidance_list_is_promoted_as_is() {
        let lane_guidances = vec![
            LaneGuidance {
                lane_index: 0,
                permitted_directions: vec![TurnDirection::Straight],
                is_recommended: false,
            },
            LaneGuidance {
                lane_index: 1,
                permitted_directions: vec![TurnDirection::Left, TurnDirection::Straight],
                is_recommended: true,
            },
        ];
        let mut step = eastward_step(
            TEST_STEP_LENGTH_METERS,
            ManeuverType::Turn(TurnDirection::Left),
        );
        step.lane_guidances = lane_guidances.clone();
        let route = route_with_steps(vec![eastward_step(0.0, ManeuverType::Departure), step]);
        let state = navigating_state(route, 1);
        let instruction = compose_navigation_instructions(&state, UnitSystem::Metric);
        assert_eq!(
            instruction.active_maneuver.lane_guidances, lane_guidances,
            "lane guidance must be promoted verbatim, in order"
        );
    }

    #[test]
    fn roundabout_exit_index_is_surfaced_as_structured_data() {
        let mut step = eastward_step(TEST_STEP_LENGTH_METERS, ManeuverType::Roundabout(2));
        step.instruction_display_text = "At the roundabout continue straight".to_string();
        let route = route_with_steps(vec![eastward_step(0.0, ManeuverType::Departure), step]);
        let state = navigating_state(route, 1);
        let instruction = compose_navigation_instructions(&state, UnitSystem::Metric);
        assert_eq!(instruction.active_maneuver.roundabout_exit_index, Some(2));
        // The provider text itself is untouched: no "2nd exit" surgery.
        let ManeuverInstructionText::Composed { display_text, .. } =
            &instruction.active_maneuver.instruction
        else {
            panic!("the authored step must compose");
        };
        assert_eq!(
            display_text,
            "In 300 meters, At the roundabout continue straight"
        );
        // And a non-roundabout maneuver carries no exit index.
        let plain_route = route_with_steps(vec![
            eastward_step(0.0, ManeuverType::Departure),
            eastward_step(TEST_STEP_LENGTH_METERS, ManeuverType::Arrival),
        ]);
        let plain_instruction =
            compose_navigation_instructions(&navigating_state(plain_route, 1), UnitSystem::Metric);
        assert_eq!(
            plain_instruction.active_maneuver.roundabout_exit_index,
            None
        );
    }

    #[test]
    fn off_route_emission_marks_rerouting_required_over_the_frozen_payload() {
        let route = route_with_steps(vec![
            eastward_step(0.0, ManeuverType::Departure),
            eastward_step(TEST_STEP_LENGTH_METERS, ManeuverType::Arrival),
        ]);
        let on_route_instruction = compose_navigation_instructions(
            &navigating_state(route.clone(), 0),
            UnitSystem::Metric,
        );
        let mut deviated_state = navigating_state(route, 0);
        deviated_state.phase = NavigationPhase::OffRoute;
        deviated_state.deviation = Some(crate::state::DeviationInfo {
            grade: crate::state::DeviationGrade::FullyOffRoute,
            deviation_from_route_meters: 100.0,
        });
        let deviated_instruction =
            compose_navigation_instructions(&deviated_state, UnitSystem::Metric);
        assert!(deviated_instruction.is_rerouting_required);
        assert!(!on_route_instruction.is_rerouting_required);
        // The frozen projection has no trustworthy distance, so the countdown
        // is suppressed rather than presented off it.
        assert_eq!(
            deviated_instruction.active_maneuver.distance_phrase,
            DistancePhrase::Suppressed
        );
        let ManeuverInstructionText::Composed {
            provider_display_text,
            provider_speech_text,
            display_text,
            speech_text,
        } = &deviated_instruction.active_maneuver.instruction
        else {
            panic!("the frozen step must still compose");
        };
        // Provider text remains verbatim beneath the marker; both composed
        // channels drop the distance prefix.
        assert_eq!(provider_display_text.as_str(), "Instruction");
        assert_eq!(provider_speech_text.as_str(), "Instruction");
        assert_eq!(display_text, "Instruction");
        assert_eq!(speech_text, "Instruction");
    }

    #[test]
    fn successive_off_route_fixes_compose_identical_instructions() {
        let route = route_with_steps(vec![
            eastward_step(0.0, ManeuverType::Departure),
            eastward_step(TEST_STEP_LENGTH_METERS, ManeuverType::Arrival),
        ]);
        let mut first_deviated_state = navigating_state(route.clone(), 0);
        first_deviated_state.phase = NavigationPhase::OffRoute;
        first_deviated_state.deviation = Some(crate::state::DeviationInfo {
            grade: crate::state::DeviationGrade::FullyOffRoute,
            deviation_from_route_meters: 100.0,
        });
        // A deeper excursion updates only the deviation measurement; the
        // frozen projection itself is untouched.
        let mut deeper_deviated_state = first_deviated_state.clone();
        deeper_deviated_state.deviation = Some(crate::state::DeviationInfo {
            grade: crate::state::DeviationGrade::FullyOffRoute,
            deviation_from_route_meters: 250.0,
        });
        assert_eq!(
            compose_navigation_instructions(&first_deviated_state, UnitSystem::Metric),
            compose_navigation_instructions(&deeper_deviated_state, UnitSystem::Metric),
        );
    }

    #[test]
    fn the_wrong_step_grade_keeps_counting_down() {
        // A fix off the current step but on the route line ("on route, wrong
        // step") is not the freezing grade: its projection is real, so the
        // distance phrase composes normally and keeps the live prefix.
        let route = route_with_steps(vec![
            eastward_step(0.0, ManeuverType::Departure),
            eastward_step(TEST_STEP_LENGTH_METERS, ManeuverType::Arrival),
        ]);
        let mut wrong_step_state = navigating_state(route.clone(), 0);
        wrong_step_state.deviation = Some(crate::state::DeviationInfo {
            grade: crate::state::DeviationGrade::OffStepOnRoute,
            deviation_from_route_meters: 0.0,
        });
        let instruction = compose_navigation_instructions(&wrong_step_state, UnitSystem::Metric);
        assert!(!instruction.is_rerouting_required);
        // The payload is identical to what the same state with no recorded
        // grade composes: the wrong-step grade alone changes nothing.
        assert_eq!(
            instruction,
            compose_navigation_instructions(&navigating_state(route, 0), UnitSystem::Metric)
        );
        assert_eq!(
            instruction.active_maneuver.distance_phrase,
            DistancePhrase::Quantity {
                magnitude: TEST_STEP_LENGTH_METERS,
                unit: DistanceUnit::Meters
            }
        );
        let ManeuverInstructionText::Composed { display_text, .. } =
            &instruction.active_maneuver.instruction
        else {
            panic!("the authored step must compose");
        };
        assert_eq!(display_text, "In 300 meters, Instruction");
    }

    #[test]
    fn the_fully_off_grade_suppresses_the_frozen_countdown() {
        // The suppression is keyed off the grade itself, not the phase marker:
        // only the fully-off grade — whose projection is frozen — loses its
        // countdown.
        let route = route_with_steps(vec![
            eastward_step(0.0, ManeuverType::Departure),
            eastward_step(TEST_STEP_LENGTH_METERS, ManeuverType::Arrival),
        ]);
        let mut fully_off_state = navigating_state(route, 0);
        fully_off_state.deviation = Some(crate::state::DeviationInfo {
            grade: crate::state::DeviationGrade::FullyOffRoute,
            deviation_from_route_meters: 100.0,
        });
        let instruction = compose_navigation_instructions(&fully_off_state, UnitSystem::Metric);
        assert_eq!(
            instruction.active_maneuver.distance_phrase,
            DistancePhrase::Suppressed
        );
        assert_eq!(
            instruction.active_maneuver.distance_phrase.display_text(),
            "",
            "the suppressed countdown line is explicit-empty"
        );
        let ManeuverInstructionText::Composed { display_text, .. } =
            &instruction.active_maneuver.instruction
        else {
            panic!("the authored step must compose");
        };
        assert_eq!(display_text, "Instruction");
    }

    #[test]
    fn distance_phrasing_never_returns_the_suppressed_state() {
        // Suppression is a compose-time decision off the deviation grade; the
        // phrase band mapper itself always phrases a magnitude or the
        // immediate state.
        for distance_meters in [0.0_f64, 4.9, 5.0, 300.0, 1000.0, 15_000.0] {
            for unit_system in [UnitSystem::Metric, UnitSystem::Imperial] {
                assert_ne!(
                    DistancePhrase::for_distance(distance_meters, unit_system),
                    DistancePhrase::Suppressed
                );
            }
        }
    }

    #[test]
    fn recovered_navigation_clears_the_rerouting_required_marker() {
        let route = route_with_steps(vec![
            eastward_step(0.0, ManeuverType::Departure),
            eastward_step(TEST_STEP_LENGTH_METERS, ManeuverType::Arrival),
        ]);
        let mut recovered_state = navigating_state(route, 0);
        recovered_state.phase = NavigationPhase::Navigating;
        let instruction = compose_navigation_instructions(&recovered_state, UnitSystem::Metric);
        assert!(!instruction.is_rerouting_required);
        assert!(!instruction.is_arrival);
    }

    #[test]
    fn arrival_emission_carries_the_arrival_marker() {
        let route = route_with_steps(vec![
            eastward_step(0.0, ManeuverType::Departure),
            eastward_step(TEST_STEP_LENGTH_METERS, ManeuverType::Arrival),
        ]);
        let step_positions = TripNavigationState::build_step_positions(&route);
        let arrived_state = TripNavigationState {
            phase: NavigationPhase::Arrived,
            route,
            step_positions,
            current_step_position: 1,
            // The reducer zeroes the progress at arrival, so the phrase is
            // immediate: no distance remains to a maneuver.
            progress: TripProgress::default(),
            snapped_coordinate: None,
            deviation: None,
            course_heading_degrees: None,
        };
        let instruction = compose_navigation_instructions(&arrived_state, UnitSystem::Metric);
        assert!(instruction.is_arrival);
        assert!(!instruction.is_rerouting_required);
        assert!(instruction.upcoming_maneuver.is_none());
        assert_eq!(
            instruction.active_maneuver.distance_phrase,
            DistancePhrase::Immediate
        );
        assert!(instruction.active_maneuver.lane_guidances.is_empty());
    }

    #[test]
    fn a_route_without_traversable_steps_composes_an_arrival_only_payload() {
        let route = route_with_steps(Vec::new());
        let step_positions = TripNavigationState::build_step_positions(&route);
        let arrived_state = TripNavigationState {
            phase: NavigationPhase::Arrived,
            route,
            step_positions,
            current_step_position: 0,
            progress: TripProgress::default(),
            snapped_coordinate: None,
            deviation: None,
            course_heading_degrees: None,
        };
        let instruction = compose_navigation_instructions(&arrived_state, UnitSystem::Metric);
        assert!(instruction.is_arrival);
        assert_eq!(
            instruction.active_maneuver.instruction,
            ManeuverInstructionText::Unavailable
        );
    }

    #[test]
    fn composition_is_pure_over_the_same_state() {
        let route = route_with_steps(vec![
            eastward_step(0.0, ManeuverType::Departure),
            eastward_step(TEST_STEP_LENGTH_METERS, ManeuverType::Arrival),
        ]);
        let state = navigating_state(route, 0);
        let first = compose_navigation_instructions(&state, UnitSystem::Metric);
        let second = compose_navigation_instructions(&state, UnitSystem::Metric);
        assert_eq!(first, second);
    }
}
