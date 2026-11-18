//! Speech-instruction triggering: when a guidance line is newsworthy enough to
//! speak.
//!
//! The SDK stays out of synthesis and playback: this module only decides
//! *when* a fix is worth a spoken line, and answers with the composed
//! instruction payload's verbatim speech text, never text this module
//! authored. The decision reuses the display phrase's *existing distance
//! bands*, so a spoken "in 100 meters" can never disagree with the banner's
//! shown phrase: the thresholds below are the very bands
//! [`crate::instruction::DistancePhrase`] rounds with.
//!
//! # Emission rules (the named behaviors)
//!
//! - **Trigger bands are the distance-phrase bands.** Fix N crossed below a
//!   band's threshold (in meters) announces that band: metric 10 m, 100 m,
//!   1 km and the immediate half-increment threshold; imperial 100 ft,
//!   1 000 ft, 1 mile and the immediate threshold. Each band announces at
//!   most once per step, and only the strictest band crossed in one fix
//!   speaks: a fix that jumps two bands announces the nearer stage only.
//! - **Monotone per step.** The band memory resets only on a step advance,
//!   so distance jitter over a threshold can never re-announce a band.
//! - **Minimum inter-utterance spacing.** After any line, the next
//!   band-triggered line waits for
//!   [`MINIMUM_INTER_UTTERANCE_SPACING_SECONDS`]. The immediate band overrides
//!   it: a "turn now" stage always plays.
//! - **Minimum repeat distance.** A
//!   [`ManeuverApproach`](SpeechUtteranceKind::ManeuverApproach) line whose
//!   trigger distance lies within
//!   [`MINIMUM_UTTERANCE_REPEAT_DISTANCE_METERS`] of the previous emission's
//!   trigger distance is suppressed: two turns announced from distances closer
//!   than this read as chatter instead of guidance. The planner does not chain
//!   "then"; the near band's own silence is the honest fallback.
//! - **Transitions always speak.** An off-route recovery and an arrival emit
//!   unconditionally (the composed payload's verbatim text) — they are the
//!   guidance-resumed and you-have-arrived lines, not pacing candidates, and
//!   like every emission they refresh the spacing memory.
//! - **Every emission carries its identity.** Each emitted
//!   [`SpeechUtterance`] is stamped with a fresh, strictly increasing
//!   [`SpeechUtterance::utterance_id`] at the single emission site, and the
//!   counter
//!   survives every reset — so identical text re-emitted is a new identity,
//!   and the platform speaker's dedup and flush decisions stay keyed on its
//!   own queue state.

use web_time::Instant;

use geospatial_types::UnitSystem;

use crate::instruction::{
    IMPERIAL_FAR_DISTANCE_THRESHOLD_FEET, IMPERIAL_IMMEDIATE_DISTANCE_THRESHOLD_FEET,
    IMPERIAL_MODERATE_DISTANCE_THRESHOLD_FEET, IMPERIAL_NEAR_ROUNDING_INCREMENT_FEET,
    METERS_PER_FOOT, METRIC_FAR_DISTANCE_THRESHOLD_METERS,
    METRIC_IMMEDIATE_DISTANCE_THRESHOLD_METERS, METRIC_MODERATE_DISTANCE_THRESHOLD_METERS,
    METRIC_NEAR_ROUNDING_INCREMENT_METERS, ManeuverInstructionText,
    compose_navigation_instructions,
};
use crate::state::{NavigationPhase, TripNavigationState};

/// Minimum wall-clock spacing between two emitted utterances, in seconds.
///
/// An engine cannot pipeline lines closer than this without overlapping its
/// own audio; about 2 seconds.
pub const MINIMUM_INTER_UTTERANCE_SPACING_SECONDS: f64 = 2.0;

/// Minimum route-progress distance between two announced lines, in meters.
///
/// Two turns whose announcements sit closer than this are either chained or
/// announced as the nearer one only.
pub const MINIMUM_UTTERANCE_REPEAT_DISTANCE_METERS: f64 = 170.0;

/// The first id an emitted [`SpeechUtterance`] carries: the planner's
/// utterance counter starts here and climbs from there (never `0`, so a zero
/// id is never a legitimate emission).
pub const FIRST_SPEECH_UTTERANCE_ID: u64 = 1;

/// What triggered a [`SpeechUtterance`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeechUtteranceKind {
    /// A trigger band was crossed this fix (the far, moderate or near band).
    ManeuverApproach,
    /// The immediate band: the maneuver is imminent.
    ImmediateManeuver,
    /// Guidance just recovered from off-route; guidance resumed.
    OffRouteRecovery,
    /// The trip arrived.
    Arrival,
}

/// One utterance the trigger policy judged newsworthy. The speech text is
/// the composed instruction payload's speech channel verbatim — the platform
/// owns playback and synthesizes nothing new.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeechUtterance {
    /// The verbatim composed speech text.
    pub utterance_speech_text: String,
    /// The live distance to the maneuver, in meters, at emission time.
    pub trigger_distance_meters: f64,
    /// The utterance identity: a stable monotonic id composed at emission,
    /// strictly increasing over every line the composing planner's owning
    /// controller has emitted since construction, never reused, and stable for
    /// the single delivery of one line. Identical text re-emitted gets a new
    /// id, so the id identifies *which emission* a delivery belongs to; the
    /// deduplication and flush decisions stay at the platform speaker, whose
    /// text-to-speech API keys its queue state on the id.
    pub utterance_id: u64,
    /// What triggered this line.
    pub kind: SpeechUtteranceKind,
}

/// One trigger band: crossing strictly below its threshold while approaching
/// the maneuver announces the band.
#[derive(Debug, Clone, Copy)]
struct SpeechTriggerBand {
    threshold_meters: f64,
    kind: SpeechUtteranceKind,
}

/// Number of trigger bands per unit system: the far, moderate, near and
/// immediate bands, always in descending-threshold order.
const TRIGGER_BAND_COUNT: usize = 4;

/// The trigger bands in the given unit system: descending thresholds, the
/// immediate band last. Metric thresholds are the distance-phrase band
/// thresholds in meters; imperial ones are the imperial band thresholds
/// converted to meters, so the two unit systems differ exactly as the
/// composed phrases differ.
fn trigger_bands(unit_system: UnitSystem) -> [SpeechTriggerBand; TRIGGER_BAND_COUNT] {
    match unit_system {
        UnitSystem::Metric => [
            SpeechTriggerBand {
                threshold_meters: METRIC_FAR_DISTANCE_THRESHOLD_METERS,
                kind: SpeechUtteranceKind::ManeuverApproach,
            },
            SpeechTriggerBand {
                threshold_meters: METRIC_MODERATE_DISTANCE_THRESHOLD_METERS,
                kind: SpeechUtteranceKind::ManeuverApproach,
            },
            SpeechTriggerBand {
                threshold_meters: METRIC_NEAR_ROUNDING_INCREMENT_METERS,
                kind: SpeechUtteranceKind::ManeuverApproach,
            },
            SpeechTriggerBand {
                threshold_meters: METRIC_IMMEDIATE_DISTANCE_THRESHOLD_METERS,
                kind: SpeechUtteranceKind::ImmediateManeuver,
            },
        ],
        UnitSystem::Imperial => [
            SpeechTriggerBand {
                threshold_meters: IMPERIAL_FAR_DISTANCE_THRESHOLD_FEET * METERS_PER_FOOT,
                kind: SpeechUtteranceKind::ManeuverApproach,
            },
            SpeechTriggerBand {
                threshold_meters: IMPERIAL_MODERATE_DISTANCE_THRESHOLD_FEET * METERS_PER_FOOT,
                kind: SpeechUtteranceKind::ManeuverApproach,
            },
            SpeechTriggerBand {
                threshold_meters: IMPERIAL_NEAR_ROUNDING_INCREMENT_FEET * METERS_PER_FOOT,
                kind: SpeechUtteranceKind::ManeuverApproach,
            },
            SpeechTriggerBand {
                threshold_meters: IMPERIAL_IMMEDIATE_DISTANCE_THRESHOLD_FEET * METERS_PER_FOOT,
                kind: SpeechUtteranceKind::ImmediateManeuver,
            },
        ],
    }
}

/// The previous utterance the spacing and repeat gates read.
#[derive(Debug, Clone, Copy)]
struct PreviousEmission {
    at: Instant,
    trigger_distance_meters: f64,
}

/// Stateful per-trip speech trigger policy: consumed by the navigation
/// controller on every dispatched fix and reset on trip start, reroute and
/// arrival.
#[derive(Debug)]
pub struct SpeechInstructionPlanner {
    /// Bands already announced for the current step, indexed like the fixed
    /// band order of [`trigger_bands`].
    announced_bands: [bool; TRIGGER_BAND_COUNT],
    previous_emission: Option<PreviousEmission>,
    /// The id the next emitted utterance carries. Composed at emission
    /// ([`SpeechInstructionPlanner::record_emission`]) and never reset: the
    /// ids stay strictly increasing across every emission this planner has
    /// ever produced — across trip starts, reroutes and arrivals — so a
    /// platform speaker reading two deliveries can always tell which is
    /// newer and can never mistake one emission for a replay of another.
    next_utterance_id: u64,
}

impl Default for SpeechInstructionPlanner {
    /// A fresh planner, exactly like [`SpeechInstructionPlanner::new`].
    fn default() -> Self {
        Self::new()
    }
}

impl SpeechInstructionPlanner {
    /// Builds the planner with nothing announced, no emission memory and the
    /// utterance counter at [`FIRST_SPEECH_UTTERANCE_ID`].
    ///
    /// # Returns
    /// A planner ready for the first fix of a trip.
    pub fn new() -> Self {
        Self {
            announced_bands: [false; TRIGGER_BAND_COUNT],
            previous_emission: None,
            next_utterance_id: FIRST_SPEECH_UTTERANCE_ID,
        }
    }

    /// Forgets all memory. Call this on trip start, a reroute and arrival. The
    /// utterance counter is deliberately *not* forgotten — the ids remain
    /// strictly increasing across the reset, so a re-emitted line reads as a
    /// new emission, never as a replay.
    pub fn reset(&mut self) {
        self.announced_bands = [false; TRIGGER_BAND_COUNT];
        self.previous_emission = None;
    }

    /// Evaluates one dispatched fix against the trigger policy.
    ///
    /// # Parameters
    /// - `previous_state` — the state before the fix, or `None` when the trip
    ///   has no previous state (which resets the planner and emits nothing).
    /// - `current_state` — the state after the fix.
    /// - `unit_system` — whether distances are phrased in metric or imperial
    ///   units, matching the banner.
    /// - `now` — the fix's processing instant, used for the spacing gate.
    ///
    /// # Returns
    /// The utterances emitted by this fix, at most one: the recovery and
    /// arrival transitions first, then the band trigger.
    ///
    /// The planner is stateful and takes `&mut self`; call it from one thread
    /// at a time, in the order the fixes were processed.
    pub fn evaluate_state_transition(
        &mut self,
        previous_state: Option<&TripNavigationState>,
        current_state: &TripNavigationState,
        unit_system: UnitSystem,
        now: Instant,
    ) -> Vec<SpeechUtterance> {
        let Some(previous_state) = previous_state else {
            self.reset();
            return Vec::new();
        };

        // A step advance refreshes the band memory: each trigger band
        // announces at most once per step.
        if previous_state.current_step_position != current_state.current_step_position {
            self.announced_bands = [false; TRIGGER_BAND_COUNT];
        }

        // Transition lines always speak: guidance resumed, arrival reached.
        if previous_state.phase == NavigationPhase::OffRoute
            && current_state.phase == NavigationPhase::Navigating
        {
            return match composed_active_speech_text(current_state, unit_system) {
                Some(utterance_speech_text) => vec![self.record_emission(
                    utterance_speech_text,
                    current_state.progress.distance_to_next_maneuver_meters,
                    SpeechUtteranceKind::OffRouteRecovery,
                    now,
                )],
                None => Vec::new(),
            };
        }
        if previous_state.phase == NavigationPhase::Navigating
            && current_state.phase == NavigationPhase::Arrived
        {
            return match composed_active_speech_text(current_state, unit_system) {
                Some(utterance_speech_text) => vec![self.record_emission(
                    utterance_speech_text,
                    0.0,
                    SpeechUtteranceKind::Arrival,
                    now,
                )],
                None => Vec::new(),
            };
        }

        // Band triggers while both fixes report on-route progress on the
        // same traversed step's distance: the deviation freeze holds the
        // projection, so an off-route fix triggers nothing (the recovery
        // line speaks instead when it lifts).
        if current_state.phase != NavigationPhase::Navigating {
            return Vec::new();
        }
        self.evaluate_band_triggers(previous_state, current_state, unit_system, now)
    }

    /// Judges the band crossings between two fixes' distances: every crossed
    /// band is marked announced (monotone), and the strictest unannounced one
    /// either earns the utterance (after the emission gates) or is dropped.
    fn evaluate_band_triggers(
        &mut self,
        previous_state: &TripNavigationState,
        current_state: &TripNavigationState,
        unit_system: UnitSystem,
        now: Instant,
    ) -> Vec<SpeechUtterance> {
        let previous_distance_meters = previous_state.progress.distance_to_next_maneuver_meters;
        let current_distance_meters = current_state.progress.distance_to_next_maneuver_meters;
        let trigger_bands = trigger_bands(unit_system);
        // Strictest crossed band: a fix that jumps two bands announces the
        // nearest stage only (OsmAnd's prompt-stage ladder).
        let mut crossed_bands = [false; TRIGGER_BAND_COUNT];
        let mut strictest_crossed_band_index: Option<usize> = None;
        for (band_index, trigger_band) in trigger_bands.iter().enumerate() {
            let already_announced = self.announced_bands[band_index];
            let crossed = !already_announced
                && previous_distance_meters > trigger_band.threshold_meters
                && current_distance_meters <= trigger_band.threshold_meters;
            crossed_bands[band_index] = crossed;
            if crossed {
                strictest_crossed_band_index = Some(band_index);
            }
        }
        // Monotone memory: every band crossed this fix is marked announced,
        // gated or not, so a later fix can never re-announce one.
        for (band_index, &crossed) in crossed_bands.iter().enumerate() {
            if crossed {
                self.announced_bands[band_index] = true;
            }
        }
        let Some(strictest_crossed_band_index) = strictest_crossed_band_index else {
            return Vec::new();
        };
        let trigger_band = trigger_bands[strictest_crossed_band_index];
        if !self.passes_emission_gates(&trigger_band, current_distance_meters, now) {
            return Vec::new();
        }
        match composed_active_speech_text(current_state, unit_system) {
            Some(utterance_speech_text) => vec![self.record_emission(
                utterance_speech_text,
                current_distance_meters,
                trigger_band.kind,
                now,
            )],
            None => Vec::new(),
        }
    }

    /// The emission gates for one trigger band: the immediate band speaks
    /// unconditionally (a safety line), the approach lines wait out the
    /// spacing gate and clear the repeat-distance gate.
    fn passes_emission_gates(
        &self,
        trigger_band: &SpeechTriggerBand,
        current_distance_meters: f64,
        now: Instant,
    ) -> bool {
        // The immediate band always speaks: an imminent maneuver is a safety
        // line, and OsmAnd's "turn now" and Organic Maps' last-stage
        // announcements are played regardless of pacing.
        if trigger_band.kind == SpeechUtteranceKind::ImmediateManeuver {
            return true;
        }
        let Some(previous_emission) = self.previous_emission else {
            return true;
        };
        // The spacing gate: wall-clock seconds between lines.
        let elapsed_seconds = now
            .saturating_duration_since(previous_emission.at)
            .as_secs_f64();
        if elapsed_seconds < MINIMUM_INTER_UTTERANCE_SPACING_SECONDS {
            return false;
        }
        // The repeat-distance gate: the previous line's trigger distance must
        // have been far enough up the approach for the new one to read as
        // fresh guidance and not a stutter.
        (previous_emission.trigger_distance_meters - current_distance_meters).abs()
            >= MINIMUM_UTTERANCE_REPEAT_DISTANCE_METERS
    }

    /// Composes one earned utterance: the utterance id is stamped here — the
    /// single emission site — from the planner's monotonic counter, so every
    /// emitted line carries a fresh, never-reused id.
    fn record_emission(
        &mut self,
        utterance_speech_text: String,
        trigger_distance_meters: f64,
        kind: SpeechUtteranceKind,
        now: Instant,
    ) -> SpeechUtterance {
        self.previous_emission = Some(PreviousEmission {
            at: now,
            trigger_distance_meters,
        });
        let composed_utterance_id = self.next_utterance_id;
        self.next_utterance_id += 1;
        SpeechUtterance {
            utterance_speech_text,
            trigger_distance_meters,
            // Ferrostar composes theirs from a UUID at emission; a
            // controller-ordered integer is stable and monotonic in the same
            // role.
            utterance_id: composed_utterance_id,
            kind,
        }
    }
}

/// The composed active step's speech text of one trip state, verbatim from
/// the provider composition. `None` when the provider authored nothing
/// speakable (the emitter invents nothing).
fn composed_active_speech_text(
    trip_state: &TripNavigationState,
    unit_system: UnitSystem,
) -> Option<String> {
    let navigation_instruction = compose_navigation_instructions(trip_state, unit_system);
    match navigation_instruction.active_maneuver.instruction {
        ManeuverInstructionText::Composed { speech_text, .. } => {
            (!speech_text.is_empty()).then_some(speech_text)
        }
        ManeuverInstructionText::Unavailable => None,
    }
}
#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use geospatial_types::{
        BoundingBox, GeographicCoordinate, ManeuverType, RouteData, RouteLeg, RouteStep,
        TurnDirection, UnitSystem,
    };

    use super::{
        MINIMUM_INTER_UTTERANCE_SPACING_SECONDS, SpeechInstructionPlanner, SpeechUtteranceKind,
    };
    use crate::state::{
        DeviationGrade, DeviationInfo, NavigationPhase, TripNavigationState, TripProgress,
    };

    /// Speech text every fixture step carries (provider-authored; this
    /// module must carry it verbatim into every utterance).
    const FIXTURE_SPEECH_TEXT: &str = "Turn left onto Testing Street";
    /// Travel time of one synthetic fixture step, in seconds.
    const FIXTURE_STEP_DURATION_SECONDS: f64 = 20.0;
    /// Deviation measurement of the fixture off-route state, in meters.
    const FIXTURE_OFF_ROUTE_DEVIATION_METERS: f64 = 30.0;
    /// Distance of the fixture route's first step, in meters (clear of every
    /// metric band threshold).
    const FIXTURE_FIRST_STEP_DISTANCE_METERS: f64 = 1_500.0;
    /// Distance of the fixture route's second step, in meters.
    const FIXTURE_SECOND_STEP_DISTANCE_METERS: f64 = 800.0;

    /// Fixtures' anchor coordinate (unused by the band math, needed by
    /// `RouteData`'s bounding box).
    fn fixture_coordinate() -> GeographicCoordinate {
        GeographicCoordinate {
            latitude: 47.376_9,
            longitude: 8.541_7,
        }
    }

    /// Fixture step: provider speech text authored, display empty (the
    /// composition's fallback reuses speech for both channels).
    fn fixture_step(distance_meters: f64) -> RouteStep {
        RouteStep {
            distance_meters,
            duration_seconds: FIXTURE_STEP_DURATION_SECONDS,
            maneuver_type: ManeuverType::Turn(TurnDirection::Left),
            instruction_display_text: String::new(),
            instruction_speech_text: FIXTURE_SPEECH_TEXT.to_string(),
            shape: Vec::new(),
            lane_guidances: Vec::new(),
        }
    }

    /// The two-step fixture route the states below traverse.
    fn fixture_route() -> RouteData {
        let step_distances = [
            FIXTURE_FIRST_STEP_DISTANCE_METERS,
            FIXTURE_SECOND_STEP_DISTANCE_METERS,
        ];
        RouteData {
            distance_meters: step_distances.iter().sum(),
            duration_seconds: step_distances.len() as f64 * FIXTURE_STEP_DURATION_SECONDS,
            bounding_box: BoundingBox {
                south_west: fixture_coordinate(),
                north_east: fixture_coordinate(),
            },
            geometry: Vec::new(),
            legs: vec![RouteLeg {
                distance_meters: step_distances.iter().sum(),
                duration_seconds: step_distances.len() as f64 * FIXTURE_STEP_DURATION_SECONDS,
                steps: step_distances.iter().map(|d| fixture_step(*d)).collect(),
            }],
        }
    }

    /// A trip state at `distance_to_maneuver_meters` on the step at
    /// `step_position`, everything else in the fixture defaults.
    fn navigating_state(
        step_position: usize,
        distance_to_maneuver_meters: f64,
    ) -> TripNavigationState {
        let route = fixture_route();
        TripNavigationState {
            phase: NavigationPhase::Navigating,
            step_positions: TripNavigationState::build_step_positions(&route),
            progress: TripProgress {
                distance_to_next_maneuver_meters: distance_to_maneuver_meters,
                distance_remaining_meters: distance_to_maneuver_meters,
                duration_remaining_seconds: FIXTURE_STEP_DURATION_SECONDS,
            },
            snapped_coordinate: Some(fixture_coordinate()),
            deviation: None,
            course_heading_degrees: None,
            current_step_position: step_position,
            route,
        }
    }

    /// An off-route clone of `state` (the deviation freeze's recorded shape).
    fn off_route_state(state: &TripNavigationState) -> TripNavigationState {
        let mut off_route_state = state.clone();
        off_route_state.phase = NavigationPhase::OffRoute;
        off_route_state.deviation = Some(DeviationInfo {
            grade: DeviationGrade::FullyOffRoute,
            deviation_from_route_meters: FIXTURE_OFF_ROUTE_DEVIATION_METERS,
        });
        off_route_state
    }

    /// A state clone with `phase` set (for the arrival transition).
    fn state_in_phase(state: &TripNavigationState, phase: NavigationPhase) -> TripNavigationState {
        let mut phased_state = state.clone();
        phased_state.phase = phase;
        phased_state
    }

    /// Evaluates one transition and returns its kinds.
    fn transition_kinds(
        planner: &mut SpeechInstructionPlanner,
        previous_state: &TripNavigationState,
        current_state: &TripNavigationState,
        now: Instant,
    ) -> Vec<SpeechUtteranceKind> {
        planner
            .evaluate_state_transition(Some(previous_state), current_state, UnitSystem::Metric, now)
            .iter()
            .map(|utterance| utterance.kind)
            .collect()
    }

    /// The metric ladder announces the far, moderate, near and immediate
    /// bands in order, each exactly once, and the strictest band crossed in
    /// one fix only.
    #[test]
    fn metric_ladder_announces_each_band_once_monotone() {
        let mut planner = SpeechInstructionPlanner::new();
        let base = Instant::now();
        let step_length = 1.0;
        let _ = step_length;
        let fixes = [
            (1_500.0, 1_000.0), // far band crossed
            (1_000.0, 900.0),   // no crossing
            (900.0, 90.0),      // moderate crossed (jumps two; strictest only)
            (90.0, 101.0),      // distance jitter upward: no crossing
            (101.0, 9.0),       // near crossed (moderate re-entry announced)
            (9.0, 3.0),         // immediate crossed
            (3.0, 2.0),         // inside the immediate band: no re-announce
        ];
        let mut emitted: Vec<SpeechUtteranceKind> = Vec::new();
        for (fix_index, (previous_distance, current_distance)) in fixes.iter().enumerate() {
            let now = base + Duration::from_secs_f64(fix_index as f64 * 10.0);
            let previous_state = navigating_state(0, *previous_distance);
            let current_state = navigating_state(0, *current_distance);
            emitted.extend(transition_kinds(
                &mut planner,
                &previous_state,
                &current_state,
                now,
            ));
        }
        // The near-band crossing (101 -> 9) is suppressed by the
        // repeat-distance gate: it sits 81 meters under the moderate line
        // just announced, inside the repeat distance — the immediate line is
        // the next spoken stage, as the Organic Maps spacing rule dictates.
        assert_eq!(
            emitted,
            vec![
                SpeechUtteranceKind::ManeuverApproach,
                SpeechUtteranceKind::ManeuverApproach,
                SpeechUtteranceKind::ImmediateManeuver,
            ]
        );
    }

    /// The trigger distance recorded on an emitted band line is the live
    /// distance at emission, and the speech text is the composed payload's.
    #[test]
    fn emitted_line_carries_live_distance_and_composed_text() {
        let mut planner = SpeechInstructionPlanner::new();
        let base = Instant::now();
        let utterances = planner.evaluate_state_transition(
            Some(&navigating_state(0, 1_100.0)),
            &navigating_state(0, 90.0),
            UnitSystem::Metric,
            base,
        );
        assert_eq!(utterances.len(), 1);
        assert_eq!(utterances[0].kind, SpeechUtteranceKind::ManeuverApproach);
        assert_eq!(utterances[0].trigger_distance_meters, 90.0);
        assert!(
            utterances[0]
                .utterance_speech_text
                .contains(FIXTURE_SPEECH_TEXT),
            "the composed line carries the provider speech text: {}",
            utterances[0].utterance_speech_text
        );
    }

    /// The spacing gate suppresses a band line that fires too soon after the
    /// previous emission; the immediate band still speaks.
    #[test]
    fn spacing_gate_suppresses_the_approach_line_but_not_the_immediate_one() {
        let mut planner = SpeechInstructionPlanner::new();
        let base = Instant::now();
        // The far band speaks at the first emission.
        let far_kinds = transition_kinds(
            &mut planner,
            &navigating_state(0, 1_100.0),
            &navigating_state(0, 950.0),
            base,
        );
        assert_eq!(far_kinds, vec![SpeechUtteranceKind::ManeuverApproach]);
        // The moderate band crosses half a second later: suppressed by the
        // spacing gate.
        let suppressed_kinds = transition_kinds(
            &mut planner,
            &navigating_state(0, 950.0),
            &navigating_state(0, 90.0),
            base + Duration::from_secs_f64(MINIMUM_INTER_UTTERANCE_SPACING_SECONDS * 0.25),
        );
        assert!(suppressed_kinds.is_empty());
        // The immediate band crosses still within the spacing window of the
        // last emission: the safety line speaks regardless.
        let immediate_kinds = transition_kinds(
            &mut planner,
            &navigating_state(0, 90.0),
            &navigating_state(0, 3.0),
            base + Duration::from_secs_f64(MINIMUM_INTER_UTTERANCE_SPACING_SECONDS * 0.5),
        );
        assert_eq!(
            immediate_kinds,
            vec![SpeechUtteranceKind::ImmediateManeuver]
        );
    }

    /// The repeat-distance gate suppresses an approach line whose trigger
    /// distance sits within the repeat distance of the previous emission;
    /// a step advance resets the band memory without lifting the gate, and
    /// the immediate band overrides.
    #[test]
    fn repeat_distance_gate_suppresses_chatter_across_a_step_advance() {
        let mut planner = SpeechInstructionPlanner::new();
        let base = Instant::now();
        // Step zero: announce the moderate band close to the maneuver.
        let moderate_kinds = transition_kinds(
            &mut planner,
            &navigating_state(0, 120.0),
            &navigating_state(0, 90.0),
            base,
        );
        assert_eq!(moderate_kinds, vec![SpeechUtteranceKind::ManeuverApproach]);
        // The trip advances into step one (its distance re-enters the
        // moderate band as the fix walks it down): the band is freshly
        // unannounced, but the trigger distance lies within the repeat
        // distance of the previous line — suppressed.
        let advanced_kinds = transition_kinds(
            &mut planner,
            &navigating_state(0, 600.0),
            &navigating_state(1, 94.0),
            base + Duration::from_secs_f64(MINIMUM_INTER_UTTERANCE_SPACING_SECONDS * 2.0),
        );
        assert!(advanced_kinds.is_empty());
        // The immediate band overrides the gate on the same approach.
        let immediate_kinds = transition_kinds(
            &mut planner,
            &navigating_state(1, 94.0),
            &navigating_state(1, 4.0),
            base + Duration::from_secs_f64(MINIMUM_INTER_UTTERANCE_SPACING_SECONDS * 3.0),
        );
        assert_eq!(
            immediate_kinds,
            vec![SpeechUtteranceKind::ImmediateManeuver]
        );
    }

    /// The imperial thresholds are the imperial phrase bands, not the metric
    /// ones: one mile and one thousand feet.
    #[test]
    fn imperial_thresholds_are_the_imperial_bands() {
        let mut planner = SpeechInstructionPlanner::new();
        let base = Instant::now();
        // One mile crossing: fires under imperial, not under metric.
        let imperial_mile_kinds = planner
            .evaluate_state_transition(
                Some(&navigating_state(0, 1_700.0)),
                &navigating_state(0, 1_609.0),
                UnitSystem::Imperial,
                base,
            )
            .iter()
            .map(|utterance| utterance.kind)
            .collect::<Vec<_>>();
        assert_eq!(
            imperial_mile_kinds,
            vec![SpeechUtteranceKind::ManeuverApproach]
        );
        // The same crossings under metric trip only the far band (1 000 m).
        let mut metric_planner = SpeechInstructionPlanner::new();
        let metric_kinds = metric_planner
            .evaluate_state_transition(
                Some(&navigating_state(0, 1_700.0)),
                &navigating_state(0, 1_609.0),
                UnitSystem::Metric,
                base,
            )
            .iter()
            .map(|utterance| utterance.kind)
            .collect::<Vec<_>>();
        assert!(
            metric_kinds.is_empty(),
            "1 609 m crossing is not below the 1 000 m metric band"
        );
        // One thousand feet (about 305 m) crossing fires under imperial too.
        let feeter = SpeechInstructionPlanner::new();
        let _ = feeter;
        let imperial_feet_kinds = planner
            .evaluate_state_transition(
                Some(&navigating_state(0, 400.0)),
                &navigating_state(0, 304.0),
                UnitSystem::Imperial,
                base + Duration::from_secs_f64(MINIMUM_INTER_UTTERANCE_SPACING_SECONDS * 2.0),
            )
            .iter()
            .map(|utterance| utterance.kind)
            .collect::<Vec<_>>();
        assert_eq!(
            imperial_feet_kinds,
            vec![SpeechUtteranceKind::ManeuverApproach],
            "the 1 000 ft band crossed"
        );
    }

    /// A recovery from off-route emits the guidance-resumed line with the
    /// composed current speech text, and an arrival emits the arrival line.
    #[test]
    fn recovery_and_arrival_transitions_speak_unconditionally() {
        let mut planner = SpeechInstructionPlanner::new();
        let base = Instant::now();
        let on_route_state = navigating_state(0, 300.0);
        let off_route_state = off_route_state(&on_route_state);
        // Off route entered: no band lines while the projection is frozen.
        let entering_kinds =
            transition_kinds(&mut planner, &on_route_state, &off_route_state, base);
        assert!(entering_kinds.is_empty());
        // Recovery: the guidance line, unconditionally.
        let recovery = planner.evaluate_state_transition(
            Some(&off_route_state),
            &on_route_state,
            UnitSystem::Metric,
            base + Duration::from_secs(1),
        );
        assert_eq!(recovery.len(), 1);
        assert_eq!(recovery[0].kind, SpeechUtteranceKind::OffRouteRecovery,);
        assert!(
            recovery[0]
                .utterance_speech_text
                .contains(FIXTURE_SPEECH_TEXT)
        );
        // Arrival: the arrival line, unconditionally.
        let arrived_state = state_in_phase(&on_route_state, NavigationPhase::Arrived);
        let arrival_kinds = transition_kinds(
            &mut planner,
            &on_route_state,
            &arrived_state,
            base + Duration::from_secs(2),
        );
        assert_eq!(arrival_kinds, vec![SpeechUtteranceKind::Arrival]);
    }

    /// A provider that authored no text produces no utterance (the emitter
    /// invents nothing), while the band monotone memory is still consumed.
    #[test]
    fn no_provider_text_produces_no_utterance() {
        let mut planner = SpeechInstructionPlanner::new();
        let base = Instant::now();
        let step_distances = [800.0];
        let bare_route = RouteData {
            distance_meters: step_distances[0],
            duration_seconds: FIXTURE_STEP_DURATION_SECONDS,
            bounding_box: BoundingBox {
                south_west: fixture_coordinate(),
                north_east: fixture_coordinate(),
            },
            geometry: Vec::new(),
            legs: vec![RouteLeg {
                distance_meters: step_distances[0],
                duration_seconds: FIXTURE_STEP_DURATION_SECONDS,
                steps: vec![RouteStep {
                    distance_meters: step_distances[0],
                    duration_seconds: FIXTURE_STEP_DURATION_SECONDS,
                    maneuver_type: ManeuverType::Turn(TurnDirection::Left),
                    instruction_display_text: String::new(),
                    instruction_speech_text: String::new(),
                    shape: Vec::new(),
                    lane_guidances: Vec::new(),
                }],
            }],
        };
        let bare_state = |distance| TripNavigationState {
            phase: NavigationPhase::Navigating,
            step_positions: TripNavigationState::build_step_positions(&bare_route),
            progress: TripProgress {
                distance_to_next_maneuver_meters: distance,
                distance_remaining_meters: distance,
                duration_remaining_seconds: FIXTURE_STEP_DURATION_SECONDS,
            },
            snapped_coordinate: Some(fixture_coordinate()),
            deviation: None,
            course_heading_degrees: None,
            current_step_position: 0,
            route: bare_route.clone(),
        };
        let utterances = planner.evaluate_state_transition(
            Some(&bare_state(1_000.0)),
            &bare_state(90.0),
            UnitSystem::Metric,
            base,
        );
        assert!(utterances.is_empty());
    }

    /// Every emitted line carries a strictly increasing, never-reused utterance
    /// id composed at emission, starting at
    /// [`FIRST_SPEECH_UTTERANCE_ID`].
    #[test]
    fn utterance_ids_are_monotonic_and_never_reused() {
        use super::FIRST_SPEECH_UTTERANCE_ID;

        let mut planner = SpeechInstructionPlanner::new();
        let base = Instant::now();
        // One approach line, then the immediate line, then a recovery and an
        // arrival — the four emission paths — collect their ids in order.
        let approach_line = planner.evaluate_state_transition(
            Some(&navigating_state(0, 1_100.0)),
            &navigating_state(0, 950.0),
            UnitSystem::Metric,
            base,
        );
        let immediate_line = planner.evaluate_state_transition(
            Some(&navigating_state(0, 950.0)),
            &navigating_state(0, 3.0),
            UnitSystem::Metric,
            base + Duration::from_secs(1),
        );
        let on_route_state = navigating_state(0, 900.0);
        let recovery_line = planner.evaluate_state_transition(
            Some(&off_route_state(&on_route_state)),
            &on_route_state,
            UnitSystem::Metric,
            base + Duration::from_secs(2),
        );
        let arrival_line = planner.evaluate_state_transition(
            Some(&on_route_state),
            &state_in_phase(&on_route_state, NavigationPhase::Arrived),
            UnitSystem::Metric,
            base + Duration::from_secs(3),
        );

        assert_eq!(approach_line[0].utterance_id, FIRST_SPEECH_UTTERANCE_ID);
        let emitted_ids = [
            approach_line[0].utterance_id,
            immediate_line[0].utterance_id,
            recovery_line[0].utterance_id,
            arrival_line[0].utterance_id,
        ];
        assert!(
            emitted_ids
                .windows(2)
                .all(|id_pair| id_pair[1] > id_pair[0]),
            "ids must climb strictly, never reused, in emission order: {emitted_ids:?}"
        );
    }

    /// Identical text re-emitted carries a new id: the id identifies the
    /// emission, so the platform's dedup keys on its own queue state and the
    /// id can never mark a genuine re-emission a replay.
    #[test]
    fn identical_text_re_emitted_carries_a_new_id() {
        let mut planner = SpeechInstructionPlanner::new();
        let base = Instant::now();
        // First recovery line...
        let on_route_state = navigating_state(0, 300.0);
        let first_recovery = planner.evaluate_state_transition(
            Some(&off_route_state(&on_route_state)),
            &on_route_state,
            UnitSystem::Metric,
            base,
        );
        // ...off route again, then recovered once more: same composed text, a
        // second legitimate emission of the identical line.
        let second_recovery = planner.evaluate_state_transition(
            Some(&off_route_state(&on_route_state)),
            &on_route_state,
            UnitSystem::Metric,
            base + Duration::from_secs(10),
        );
        // (The re-entering transition between the two recoveries spoke
        // nothing: the projection freeze triggers no line.)
        assert_eq!(first_recovery.len(), 1);
        assert_eq!(second_recovery.len(), 1);
        assert_eq!(
            first_recovery[0].utterance_speech_text, second_recovery[0].utterance_speech_text,
            "both transitions compose the same provider text"
        );
        assert_ne!(
            first_recovery[0].utterance_id, second_recovery[0].utterance_id,
            "a re-emitted identical line must carry a new id, not a replay"
        );
    }

    /// The id counter survives the reset (trip start, reroute, arrival): ids
    /// climb across the reset, never restarting.
    #[test]
    fn reset_forgets_band_memory_but_not_the_utterance_id_counter() {
        let mut planner = SpeechInstructionPlanner::new();
        let base = Instant::now();
        let first_line = planner.evaluate_state_transition(
            Some(&navigating_state(0, 1_100.0)),
            &navigating_state(0, 950.0),
            UnitSystem::Metric,
            base,
        );
        let first_line_id = first_line[0].utterance_id;

        planner.reset();
        // A fresh (post-reset) band crossing emits again from a remembered
        // counter.
        let post_reset_line = planner.evaluate_state_transition(
            Some(&navigating_state(0, 1_100.0)),
            &navigating_state(0, 950.0),
            UnitSystem::Metric,
            base + Duration::from_secs(10),
        );
        assert_eq!(post_reset_line.len(), 1);
        assert!(
            post_reset_line[0].utterance_id > first_line_id,
            "the counter must survive the reset: {} > {}",
            post_reset_line[0].utterance_id,
            first_line_id
        );
    }
}
