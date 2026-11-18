//! Turn-by-turn navigation for a planned route.
//!
//! The crate is a pure event reducer: [`reduce_navigation_event`] maps
//! [`NavigationEvent`]s onto a [`NavigationSession`] without touching the
//! network, the clock or the GPU. Each location fix is judged for deviation
//! ([`deviation`]), snapped onto the route geometry ([`geometry`]), and
//! evaluated against a pluggable [`StepAdvanceCondition`]. When the driver
//! leaves the route, the caller supplies a replacement route through
//! [`NavigationEvent::RerouteProvided`] and the reducer swaps it in.
//! [`NavigationPhase`] moves through `Navigating`,
//! `OffRoute` and `Arrived`; `OffRoute` is a pause the driver can recover
//! from by returning to the route.
//!
//! Supporting modules compose the user-visible output:
//!
//! - [`instruction`] turns a trip state into plain-data banner and
//!   text-to-speech payloads.
//! - [`observer`] delivers state changes to per-topic subscribers with
//!   explicit cancellation.
//! - [`reroute`] applies a time-and-distance hysteresis before signalling
//!   that a recalculation is due.
//! - [`speech`] decides when a spoken instruction is announced.
//! - [`course_filtering`] turns a raw fix course into the trip's reported
//!   course.
//! - [`recorder`] optionally keeps a bounded log of fixes and state changes
//!   for diagnostics.
//! - [`route_walk`] walks a route at a fixed pace for simulated trips.
//!
//! The reducer is synchronous and deterministic: the same event history
//! always produces the same session state. Time is supplied by the caller in
//! the events, and nothing here spawns a thread or performs I/O.

// The reducer and observer code runs inside rustic-maps-native-core's exported FFI
// frames, so the no-`unwrap`/`expect` policy of the FFI edge applies here
// too (its `panic_boundary` module documents the scaffolding containment
// these invariants lean on). The deny is lifted for the test builds (this
// crate's test modules, compiled with `cfg(test)`), where the assertion
// shorthands are honest; the deliberately panic-expressing sites that
// remain (the poisoned-lock guard asserts in the broadcaster and the
// recorder, and the reducer's current-step invariant) carry their
// justifications inline.
#![cfg_attr(not(test), deny(clippy::expect_used, clippy::unwrap_used))]

pub mod course_filtering;
pub mod deviation;
pub mod geometry;
pub mod instruction;
pub mod location;
pub mod observer;
pub mod recorder;
pub mod reducer;
pub mod reroute;
pub mod route_walk;
pub mod speech;
pub mod state;
pub mod step_advance;

pub use course_filtering::{CourseFiltering, effective_course_heading_degrees};
pub use deviation::{
    DEFAULT_DEVIATION_DISTANCE_THRESHOLD_METERS, DeviationDetectionInputs, DeviationDetector,
    DeviationStatus, DistanceDeviationDetector,
};
pub use geometry::{PolylineSnap, snap_coordinate_to_polyline};
pub use instruction::{
    ActiveManeuverInstruction, DistancePhrase, DistanceUnit, ManeuverInstructionText,
    NavigationInstruction, UpcomingManeuverInstruction, compose_navigation_instructions,
};
pub use location::NavigationLocationUpdate;
pub use observer::{
    DeviationStatusChange, NavigationDispatchOutcome, NavigationPhaseChange,
    NavigationStateBroadcaster, NavigationStateChangeListener, TopicSubscription,
};
pub use recorder::{
    DEFAULT_NAVIGATION_RECORDER_CAPACITY, NavigationRecordedEvent, NavigationRecorder,
    NavigationRecordingEntry,
};
pub use reducer::{
    MAXIMUM_STEP_ADVANCES_PER_FIX, NavigationEvent, NavigationSession, reduce_navigation_event,
};
pub use reroute::{
    DEFAULT_REROUTE_RECALCULATION_COOLDOWN_SECONDS, DEFAULT_REROUTE_RECALCULATION_MOVEMENT_METERS,
    ReroutePolicy, RerouteRecalculationGate,
};
pub use route_walk::{RouteWalk, RouteWalkError, SimulatedTripFix, SimulatedTripPace};
pub use speech::{
    MINIMUM_INTER_UTTERANCE_SPACING_SECONDS, MINIMUM_UTTERANCE_REPEAT_DISTANCE_METERS,
    SpeechInstructionPlanner, SpeechUtterance, SpeechUtteranceKind,
};
pub use state::{
    DeviationGrade, DeviationInfo, NavigationPhase, RouteStepPosition, TripNavigationState,
    TripProgress,
};
pub use step_advance::{
    AndStepAdvanceCondition, DEFAULT_DISTANCE_FROM_STEP_THRESHOLD_METERS,
    DEFAULT_DISTANCE_TO_END_OF_STEP_THRESHOLD_METERS, DEFAULT_MINIMUM_HORIZONTAL_ACCURACY_METERS,
    DistanceFromStepCondition, DistanceToEndOfStepCondition, ManualStepAdvanceCondition,
    OrStepAdvanceCondition, StepAdvanceCondition, StepAdvanceDecision, StepAdvanceInputs,
};

/// Recorded GPS traces and their deterministic generator: the loader used by
/// the end-to-end replay tests. Test-only.
#[cfg(test)]
pub(crate) mod trace_fixture;
/// End-to-end navigation replay tests over [`trace_fixture`] traces: the full
/// reducer, deviation, reroute, instruction and observer pipeline driven by
/// the recorded data at its recorded timing. Test-only.
#[cfg(test)]
mod trace_replay_tests;

#[cfg(test)]
pub(crate) mod test_support;
