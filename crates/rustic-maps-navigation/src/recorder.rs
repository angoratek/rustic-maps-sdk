//! Opt-in navigation recording for field diagnostics and debugging.
//!
//! A [`NavigationRecorder`] implements the per-topic
//! [`NavigationStateChangeListener`] surface and additionally records the raw
//! position fixes delivered to the reducer through an explicit
//! [`NavigationRecorder::record_location_fix`] channel (the raw fixes are not a
//! listener topic — the listener surface starts downstream of them). Every
//! recording is stamped with a strictly increasing sequence number and, when
//! the clock is available, an instant in seconds since the Unix epoch, and is
//! kept in a bounded ring that silently drops its oldest entry once full. A
//! recorder therefore never grows unbounded and never slows the trip down
//! beyond one short locked section per recorded item.
//!
//! Recording is opt-in: nothing records unless a consumer holds a recorder and
//! subscribes it or feeds it fixes. Recording stops when the recorder is
//! dropped from every subscription.
//!
//! The drained entries are the plain-data payload types the other navigation
//! modules emit.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use web_time::{SystemTime, UNIX_EPOCH};

use crate::instruction::NavigationInstruction;
use crate::location::NavigationLocationUpdate;
use crate::observer::{
    DeviationStatusChange, NavigationPhaseChange, NavigationStateChangeListener,
};

/// The default ring capacity, in entries, of a [`NavigationRecorder`].
///
/// Provenance: a debugging buffer, sized so a representative trip's whole
/// state-change stream (one instruction payload per dispatched state, the
/// deviation stream, phase transitions and the fix stream) stays intact
/// while a single entry costs only its plain-data payload. Capacity is
/// explicit at construction when a session needs a tighter or wider ring.
pub const DEFAULT_NAVIGATION_RECORDER_CAPACITY: usize = 1024;

/// The smallest capacity a [`NavigationRecorder`] can be configured with:
/// a capacity of `0` would record nothing while still paying the recording
/// cost, so it is clamped up to one entry instead.
const MINIMUM_NAVIGATION_RECORDER_CAPACITY: usize = 1;

/// One state change or fix a [`NavigationRecorder`] observed over the
/// navigation stream, plain data end to end.
///
/// The variants are the payloads the state machine actually emits — the
/// composed instruction payload, the detector's deviation-stream judgment,
/// one-shot phase transitions and the raw fix — so a drained recording is
/// read against the very types the live consumers consume. The speech
/// topic is deliberately not recorded: an utterance is derived playback
/// scheduling over the recorded instruction stream, not a state change.
#[derive(Debug, Clone, PartialEq)]
pub enum NavigationRecordedEvent {
    /// A raw position fix reached the reducer ([`NavigationRecorder::record_location_fix`]).
    LocationUpdated(NavigationLocationUpdate),
    /// The instructions topic delivered a composed instruction payload.
    InstructionDelivered(NavigationInstruction),
    /// The deviation topic delivered a judgment change.
    DeviationStatusChanged(DeviationStatusChange),
    /// The phase topic delivered a one-shot transition.
    PhaseChanged(NavigationPhaseChange),
}

/// One recorded item in a [`NavigationRecorder`]'s ring.
#[derive(Debug, Clone, PartialEq)]
pub struct NavigationRecordingEntry {
    /// Position of this entry in the recorder's overall stream, starting at
    /// zero and strictly increasing across the recorder's lifetime:
    /// drained entries always come back ordered by this number.
    pub sequence_number: u64,
    /// Wall-clock instant of the recording in seconds since the Unix
    /// epoch, or `None` when the platform clock cannot read it.
    pub recorded_at_seconds_since_epoch: Option<f64>,
    /// The recorded payload.
    pub event: NavigationRecordedEvent,
}

/// A cheaply cloneable, bounded, opt-in navigation recorder.
///
/// Clones share one ring (an `Arc` of the recording core), so the recorder
/// can be handed to the reducer-driving code and the broadcasting code as
/// one recording stream. `Send + Sync` end to end: the state-listener
/// surface requires it, and the recording lock is held only across the
/// bounded push.
#[derive(Clone)]
pub struct NavigationRecorder {
    core: Arc<RecorderCore>,
}

/// The recorder's shared core: the immutable capacity and the locked
/// recording state.
struct RecorderCore {
    /// Entry capacity of the bounded ring (the construction clamped
    /// capacity, never below [`MINIMUM_NAVIGATION_RECORDER_CAPACITY`]).
    capacity: usize,
    /// The ring itself, held only across one push or drain.
    recording: Mutex<RecorderRecording>,
}

/// The lock-protected recording state: the ring, the next sequence number
/// and the overwrite counter.
#[derive(Default)]
struct RecorderRecording {
    entries: VecDeque<NavigationRecordingEntry>,
    next_sequence_number: u64,
    entries_overwritten: u64,
}

impl NavigationRecorder {
    /// Builds a recorder with the [`DEFAULT_NAVIGATION_RECORDER_CAPACITY`].
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_NAVIGATION_RECORDER_CAPACITY)
    }

    /// Builds a recorder holding at most a fixed number of entries.
    ///
    /// # Parameters
    /// - `capacity` — the maximum retained entries. The oldest entries are
    ///   dropped as the ring overflows, and each drop is counted in
    ///   [`Self::entries_overwritten`]. A `0` capacity is clamped up to
    ///   [`MINIMUM_NAVIGATION_RECORDER_CAPACITY`].
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            core: Arc::new(RecorderCore {
                capacity: capacity.max(MINIMUM_NAVIGATION_RECORDER_CAPACITY),
                recording: Mutex::new(RecorderRecording::default()),
            }),
        }
    }

    /// Records one raw position fix through the fix channel: the listener
    /// topics start downstream of the raw fixes, so the reducer's caller
    /// records them here.
    ///
    /// # Parameters
    /// - `location_update` — the fix exactly as it was handed to the reducer.
    pub fn record_location_fix(&self, location_update: &NavigationLocationUpdate) {
        self.record(NavigationRecordedEvent::LocationUpdated(*location_update));
    }

    /// How many entries the ring currently holds (its capacity's ceiling).
    pub fn entry_count(&self) -> usize {
        self.locked().entries.len()
    }

    /// How many entries the ring has dropped to keep its capacity once
    /// full, over the recorder's lifetime.
    pub fn entries_overwritten(&self) -> u64 {
        self.locked().entries_overwritten
    }

    /// Empties the ring and returns its entries in sequence order.
    ///
    /// # Returns
    /// The drained entries, owned by the caller. The recorder keeps recording
    /// into the emptied ring, so a debug session can drain as often as it
    /// likes.
    pub fn drain(&self) -> Vec<NavigationRecordingEntry> {
        let mut recording = self.locked();
        std::mem::take(&mut recording.entries).into_iter().collect()
    }

    /// The lock-protected recording state.
    ///
    /// The mutex cannot be poisoned by construction: it is held only
    /// across infallible ring operations (a bounded push, a pop, a drain),
    /// and no listener or dispatch code runs under it.
    #[allow(clippy::expect_used)] // poisoned-lock assert: justified inline
    fn locked(&self) -> std::sync::MutexGuard<'_, RecorderRecording> {
        self.core.recording.lock().expect(
            "the recorder mutex cannot be poisoned: only infallible ring operations \
             (a bounded push, a pop, a drain) run while it is held",
        )
    }

    /// The recording core of every channel: a short locked section stamps
    /// the entry, pushes it and drops the oldest entry when the ring is at
    /// capacity. The overwrite is silent (the counter is the trace), so a
    /// recording session can never stall or grow unbounded.
    fn record(&self, event: NavigationRecordedEvent) {
        let mut recording = self.locked();
        let sequence_number = recording.next_sequence_number;
        recording.next_sequence_number += 1;
        recording.entries.push_back(NavigationRecordingEntry {
            sequence_number,
            recorded_at_seconds_since_epoch: current_unix_seconds(),
            event,
        });
        if recording.entries.len() > self.core.capacity {
            recording.entries.pop_front();
            recording.entries_overwritten += 1;
        }
    }
}

impl Default for NavigationRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl NavigationStateChangeListener for NavigationRecorder {
    fn on_navigation_instruction(&self, instruction: &NavigationInstruction) {
        self.record(NavigationRecordedEvent::InstructionDelivered(
            instruction.clone(),
        ));
    }

    fn on_deviation_status_change(&self, deviation_status_change: &DeviationStatusChange) {
        self.record(NavigationRecordedEvent::DeviationStatusChanged(
            deviation_status_change.clone(),
        ));
    }

    fn on_navigation_phase_change(&self, navigation_phase_change: &NavigationPhaseChange) {
        self.record(NavigationRecordedEvent::PhaseChanged(
            *navigation_phase_change,
        ));
    }
}

/// The wall-clock instant in seconds since the Unix epoch, or `None` when
/// the platform clock cannot read it (a recorder works without a clock:
/// the sequence numbers keep the order).
fn current_unix_seconds() -> Option<f64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|elapsed| elapsed.as_secs_f64())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use geospatial_types::{BoundingBox, ManeuverType, RouteData, RouteLeg, RouteStep, UnitSystem};

    use super::{
        DEFAULT_NAVIGATION_RECORDER_CAPACITY, NavigationRecordedEvent, NavigationRecorder,
    };
    use crate::course_filtering::CourseFiltering;
    use crate::deviation::DistanceDeviationDetector;
    use crate::location::NavigationLocationUpdate;
    use crate::observer::{
        NavigationDispatchOutcome, NavigationStateBroadcaster, NavigationStateChangeListener,
    };
    use crate::reducer::{NavigationEvent, reduce_navigation_event};
    use crate::step_advance::ManualStepAdvanceCondition;
    use crate::test_support::{TEST_ACCURACY_METERS, TEST_SEGMENT_SPAN_METERS, offset_coordinate};

    /// Spacing, in meters, between consecutive recorded walker fixes.
    const TEST_RECORD_SPACING_METERS: f64 = 10.0;
    /// Fix count the basic ring tests record.
    const TEST_RECORDED_FIX_COUNT: usize = 5;
    /// Capacity of the bounded-ring tests' recorder.
    const TEST_RING_CAPACITY: usize = 2;
    /// Declared duration, in seconds, of the replay sample's single step.
    const TEST_FIXTURE_DURATION_SECONDS: f64 = 20.0;
    /// Entries the replay test drains: the start dispatch's instruction
    /// payload, the explicitly recorded fix and the fix dispatch's
    /// instruction payload — the deviation and phase topics stay silent (an
    /// on-route manual trip raises neither).
    const TEST_REPLAY_ENTRY_COUNT: usize = 3;
    /// Entries beyond capacity the default-capacity test records.
    const TEST_DEFAULT_CAPACITY_OVERWRITE_COUNT: usize = 2;
    /// The replay test's unit system (any system produces payloads; the
    /// recorder records them verbatim without reading one).
    const TEST_UNIT_SYSTEM: UnitSystem = UnitSystem::Metric;

    /// Builds a graded walker fix at `east_meters`.
    fn walker_fix(east_meters: f64) -> NavigationLocationUpdate {
        NavigationLocationUpdate {
            coordinate: offset_coordinate(east_meters, 0.0),
            horizontal_accuracy_meters: Some(TEST_ACCURACY_METERS),
            course_heading_degrees: None,
            timestamp_seconds: None,
        }
    }

    /// Builds a single traversable eastward arrival step: the replay
    /// sample's navigable geometry.
    fn single_arrival_step_route() -> RouteData {
        let route_step = RouteStep {
            distance_meters: TEST_SEGMENT_SPAN_METERS,
            duration_seconds: TEST_FIXTURE_DURATION_SECONDS,
            maneuver_type: ManeuverType::Arrival,
            instruction_display_text: "Arrive".to_string(),
            instruction_speech_text: "Arrive".to_string(),
            shape: vec![
                offset_coordinate(0.0, 0.0),
                offset_coordinate(TEST_SEGMENT_SPAN_METERS, 0.0),
            ],
            lane_guidances: Vec::new(),
        };
        let route_leg = RouteLeg {
            distance_meters: TEST_SEGMENT_SPAN_METERS,
            duration_seconds: TEST_FIXTURE_DURATION_SECONDS,
            steps: vec![route_step],
        };
        RouteData {
            distance_meters: TEST_SEGMENT_SPAN_METERS,
            duration_seconds: TEST_FIXTURE_DURATION_SECONDS,
            bounding_box: BoundingBox {
                south_west: offset_coordinate(0.0, 0.0),
                north_east: offset_coordinate(TEST_SEGMENT_SPAN_METERS, 0.0),
            },
            geometry: Vec::new(),
            legs: vec![route_leg],
        }
    }

    /// Fixes record with strictly increasing sequence numbers from zero and
    /// come out of `drain` in that order; a drain empties the ring and the
    /// next recordings start over at the emptied ring.
    #[test]
    fn fixes_record_in_sequence_order_and_drain_empties_the_ring() {
        let recorder = NavigationRecorder::new();
        assert_eq!(recorder.entry_count(), 0);
        for walker_number in 1..=TEST_RECORDED_FIX_COUNT {
            recorder.record_location_fix(&walker_fix(
                walker_number as f64 * TEST_RECORD_SPACING_METERS,
            ));
        }
        assert_eq!(recorder.entry_count(), TEST_RECORDED_FIX_COUNT);
        assert_eq!(recorder.entries_overwritten(), 0);

        let drained = recorder.drain();
        assert_eq!(drained.len(), TEST_RECORDED_FIX_COUNT);
        for (entry_number, entry) in drained.iter().enumerate() {
            assert_eq!(
                entry.sequence_number, entry_number as u64,
                "sequence numbers run from zero in recording order"
            );
            assert!(
                matches!(&entry.event, NavigationRecordedEvent::LocationUpdated(_)),
                "only fix entries must be recorded, got {entry:?}"
            );
            assert!(
                matches!(
                    &entry.event,
                    NavigationRecordedEvent::LocationUpdated(recorded_fix)
                        if recorded_fix.coordinate
                            == offset_coordinate((entry_number + 1) as f64 * TEST_RECORD_SPACING_METERS, 0.0)
                ),
                "each drained entry must carry its own fix, got {entry:?}"
            );
        }

        // The drain empties the ring and recording continues fresh.
        assert!(recorder.drain().is_empty());
        assert_eq!(recorder.entry_count(), 0);
    }

    /// A full ring silently drops its oldest entries: the drained entries
    /// are the newest ones and each drop is counted.
    #[test]
    fn the_ring_bounds_capacity_and_counts_its_overwritten_entries() {
        let recorder = NavigationRecorder::with_capacity(TEST_RING_CAPACITY);
        for walker_number in 1..=TEST_RECORDED_FIX_COUNT {
            recorder.record_location_fix(&walker_fix(
                walker_number as f64 * TEST_RECORD_SPACING_METERS,
            ));
        }
        assert_eq!(recorder.entry_count(), TEST_RING_CAPACITY);

        let drained = recorder.drain();
        assert_eq!(drained.len(), TEST_RING_CAPACITY);
        assert_eq!(
            recorder.entries_overwritten(),
            (TEST_RECORDED_FIX_COUNT - TEST_RING_CAPACITY) as u64,
            "each dropped entry must be counted"
        );
        // Only the newest entries survive: the drained sequence numbers are
        // the ring's most recent ones, in order.
        for (drained_number, entry) in drained.iter().enumerate() {
            assert_eq!(
                entry.sequence_number,
                (TEST_RECORDED_FIX_COUNT - TEST_RING_CAPACITY + drained_number) as u64
            );
        }
    }

    /// A `0` capacity is clamped to the minimum: a recorder keeps
    /// recording (one entry) rather than recording nothing while still
    /// paying the cost.
    #[test]
    fn a_zero_capacity_records_at_the_minimum() {
        let recorder = NavigationRecorder::with_capacity(0);
        for walker_number in 1..=TEST_RECORDED_FIX_COUNT {
            recorder.record_location_fix(&walker_fix(
                walker_number as f64 * TEST_RECORD_SPACING_METERS,
            ));
        }
        assert_eq!(recorder.entry_count(), 1);
        assert_eq!(
            recorder.entries_overwritten(),
            TEST_RECORDED_FIX_COUNT as u64 - 1,
            "the clamped ring keeps exactly the newest entry"
        );
    }

    /// The default capacity is the named default: recording past it
    /// overwrites, not grows.
    #[test]
    fn the_default_capacity_is_the_named_default() {
        let recorder = NavigationRecorder::default();
        let recorded_fix_count =
            DEFAULT_NAVIGATION_RECORDER_CAPACITY + TEST_DEFAULT_CAPACITY_OVERWRITE_COUNT;
        for walker_number in 0..recorded_fix_count {
            recorder.record_location_fix(&walker_fix(
                walker_number as f64 * TEST_RECORD_SPACING_METERS,
            ));
        }
        assert_eq!(recorder.entry_count(), DEFAULT_NAVIGATION_RECORDER_CAPACITY);
        assert_eq!(
            recorder.entries_overwritten(),
            TEST_DEFAULT_CAPACITY_OVERWRITE_COUNT as u64
        );
    }

    /// A real navigation walk records the stream the way a session
    /// produces it: the start dispatch's instruction payload, the raw fix
    /// the walker delivers through the explicit fix channel, and the fix
    /// dispatch's instruction payload — one shared sequence across all
    /// topics, in delivery order.
    #[test]
    fn a_broadcaster_replay_records_topics_in_delivery_order() {
        let recorder = NavigationRecorder::new();
        let broadcaster = NavigationStateBroadcaster::new();
        let recorded_listener: Arc<dyn NavigationStateChangeListener> = Arc::new(recorder.clone());
        let _instruction_subscription =
            broadcaster.subscribe_to_navigation_instructions(Arc::clone(&recorded_listener));
        let _deviation_subscription =
            broadcaster.subscribe_to_deviation_status(Arc::clone(&recorded_listener));
        let _phase_subscription = broadcaster.subscribe_to_phase_changes(recorded_listener);

        let mut session = reduce_navigation_event(
            None,
            NavigationEvent::StartTrip {
                route: single_arrival_step_route(),
                arrival_step_advance_condition: None,
                course_filtering: CourseFiltering::Raw,
                step_advance_condition: Arc::new(ManualStepAdvanceCondition),
                deviation_detector: Arc::new(DistanceDeviationDetector::with_default_threshold()),
            },
        )
        .expect("the fixture route must start a trip");
        assert_eq!(
            broadcaster.dispatch_trip_started(&session.state.clone(), TEST_UNIT_SYSTEM),
            NavigationDispatchOutcome::Delivered,
        );

        recorder.record_location_fix(&walker_fix(0.0));
        let previous_state = session.state.clone();
        session = reduce_navigation_event(
            Some(session),
            NavigationEvent::LocationUpdated(walker_fix(TEST_RECORD_SPACING_METERS)),
        )
        .expect("a navigating trip must stay alive");
        assert_eq!(
            broadcaster.dispatch_location_update(&previous_state, &session.state, TEST_UNIT_SYSTEM),
            NavigationDispatchOutcome::Delivered,
        );

        let drained = recorder.drain();
        assert_eq!(drained.len(), TEST_REPLAY_ENTRY_COUNT);
        assert_eq!(
            [
                drained[0].sequence_number,
                drained[1].sequence_number,
                drained[2].sequence_number
            ],
            [0u64, 1, 2],
            "sequence numbers run across topics in delivery order"
        );
        let start_dispatch_entry = &drained[0];
        let fix_channel_entry = &drained[1];
        let fix_dispatch_entry = &drained[2];
        assert!(
            matches!(
                &start_dispatch_entry.event,
                NavigationRecordedEvent::InstructionDelivered(_)
            ),
            "the start dispatch delivers the initial instruction payload, got {start_dispatch_entry:?}"
        );
        assert!(
            matches!(
                &fix_channel_entry.event,
                NavigationRecordedEvent::LocationUpdated(_)
            ),
            "the walker's fix lands through the explicit fix channel, got {fix_channel_entry:?}"
        );
        assert!(
            matches!(
                &fix_dispatch_entry.event,
                NavigationRecordedEvent::InstructionDelivered(_)
            ),
            "the fix dispatch delivers the instruction payload, got {fix_dispatch_entry:?}"
        );
    }
}
