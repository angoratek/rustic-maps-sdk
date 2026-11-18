//! Per-topic observers over the navigation state machine.
//!
//! The module registers listeners per topic behind one
//! [`NavigationStateChangeListener`] trait, each subscription with an explicit
//! [`TopicSubscription::cancel`], so a consumer subscribes to only what it
//! needs and each subscription cancels exactly itself:
//!
//! - **instructions** (`dispatch_*` always): the composed
//!   [`NavigationInstruction`] payload of every dispatched state change,
//!   produced by [`crate::instruction::compose_navigation_instructions`];
//! - **deviation status** (on change): [`DeviationStatusChange`] as the
//!   detector's judgment enters, deepens/updates or clears — the per-fix
//!   judgment stream, including measurement drift while the phase stays
//!   `OffRoute`;
//! - **phase** (on transition): [`NavigationPhaseChange`] — one-shot
//!   transitions `OffRoute` entered/cleared, `RerouteProvided` applied, and
//!   `Arrived`.
//!
//! # Threading contract
//!
//! Dispatch is synchronous and single-threaded by contract: callbacks run on
//! the caller's thread, in the dispatch's own stack frame. A host that needs
//! callbacks on another thread performs that hop itself. Cross-thread calls
//! are data-race-free (the trait requires [`Send`] + [`Sync`]) and are at worst
//! [suppressed](NavigationDispatchOutcome::SuppressedReentrant) by the
//! re-entrancy guard, but the ordering guarantee is stated for single-threaded
//! use only.
//!
//! # Dispatch rules
//!
//! - **No lock is held across listener callbacks.** The registry lock is
//!   taken once to snapshot the (cheaply cloned) listener lists and released
//!   before the first callback runs, so a listener may freely call back into
//!   the broadcaster — including [`TopicSubscription::cancel`] — without
//!   deadlocking.
//! - **Delivery order**: within a topic, strict registration order;
//!   across topics, instructions first, then deviation status, then phase.
//! - **One dispatch in flight.** The deliberate re-entrancy bound: a
//!   dispatch that starts while another dispatch is still in flight —
//!   typically a listener driving the reducer and re-dispatching from its own
//!   callback — delivers *nothing* and reports
//!   [`NavigationDispatchOutcome::SuppressedReentrant`]. Nothing is queued
//!   (no unbounded growth, no re-entrant cascade): the state change behind a
//!   suppressed dispatch is never lost — it already happened in the reducer —
//!   but its broadcast is skipped, so a listener that re-dispatches must let
//!   the outer dispatch finish and dispatch again from the top level. The
//!   common listener shapes (recording, platform notification) are fully
//!   synchronous and unaffected.
//! - **Listener errors cannot poison the reducer.** Listener methods return
//!   nothing, so there is no error channel by construction; the dispatch
//!   runs *after* the reducer has already produced its new state and only
//!   reads the state it is handed. A listener that panics propagates the
//!   panic to the dispatch caller and cannot corrupt the trip state or wedge
//!   the broadcaster: the in-flight guard releases on unwind. A host that
//!   must contain listener panics catches them at its callback boundary.
//! - **Snapshot semantics for cancel**: a subscription cancelled during a
//!   dispatch may still receive that dispatch's callbacks (the snapshot was
//!   taken before); cancellation is guaranteed for every dispatch started
//!   after `cancel` returns.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use geospatial_types::UnitSystem;

use crate::instruction::{NavigationInstruction, compose_navigation_instructions};
use crate::state::{DeviationInfo, NavigationPhase, TripNavigationState};

/// One listener's callback surface, split by topic.
///
/// The methods have default no-op bodies: implementing only the topics a
/// consumer needs is trivial. Registered subscriptions select which method a
/// dispatch may call — subscribing to instructions delivers only
/// [`Self::on_navigation_instruction`] to that subscription, and so on.
///
/// Payloads are flat immutable records with no error channel: the listener
/// cannot fail into the broadcaster (see the module-head dispatch rules).
pub trait NavigationStateChangeListener: Send + Sync {
    /// Receives the composed [`NavigationInstruction`] payload of every
    /// dispatched state change (the instructions topic).
    fn on_navigation_instruction(&self, _instruction: &NavigationInstruction) {}

    /// Receives a deviation-status stream update (the deviation topic):
    /// [`DeviationStatusChange::OffRoute`] on entry and on every recorded
    /// measurement update while the deviation persists,
    /// [`DeviationStatusChange::OnRoute`] when it clears.
    fn on_deviation_status_change(&self, _deviation_status_change: &DeviationStatusChange) {}

    /// Receives one [`NavigationPhaseChange`] per transition (the phase
    /// topic): `OffRoute` entered or cleared, a reroute applied, arrival.
    fn on_navigation_phase_change(&self, _navigation_phase_change: &NavigationPhaseChange) {}

    /// Receives one speech-trigger utterance the controller's pacing policy
    /// judged newsworthy. Never emitted while no listener subscribes to the
    /// topic; the composed speech text is carried verbatim and the platform
    /// owns playback.
    fn on_speech_utterance(&self, _speech_utterance: &crate::speech::SpeechUtterance) {}
}

/// A deviation-topic update: the detector's judgment changed between the
/// previous and the current trip state.
#[derive(Debug, Clone, PartialEq)]
pub enum DeviationStatusChange {
    /// The trip is judged deviating. Delivered when a deviation first enters
    /// and again whenever the recorded measurement or grade is updated by a
    /// further graded fix while any deviation persists. The carried
    /// [`DeviationInfo`]'s grade distinguishes the freezing fully-off grade
    /// from a merely wrong-step `OffStepOnRoute` record: the wrong-step grade
    /// is delivered as a deviation-topic update while the trip phase stays
    /// `Navigating` — the phase topic stays silent for it.
    OffRoute(DeviationInfo),
    /// A previously recorded deviation cleared; the trip is judged on route.
    OnRoute,
}

/// A phase-transition event delivered on the phase topic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationPhaseChange {
    /// The trip entered [`NavigationPhase::OffRoute`] from [`NavigationPhase::Navigating`].
    OffRouteEntered,
    /// A graded fix back on the route line lifted the deviation and returned
    /// the trip to [`NavigationPhase::Navigating`].
    OffRouteCleared,
    /// A [`crate::reducer::NavigationEvent::RerouteProvided`] event replaced
    /// the route (delivered only when the event actually applied — an
    /// ignored reroute produces no transition).
    RerouteApplied,
    /// The final step completed; the trip reached [`NavigationPhase::Arrived`].
    Arrived,
}

/// Outcome of one dispatch call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationDispatchOutcome {
    /// The dispatch ran and delivered to all subscribed topics in order.
    Delivered,
    /// The dispatch was suppressed by the re-entrancy guard because another
    /// dispatch is still in flight (see the module-head dispatch rules).
    /// Nothing was delivered; the state change behind this dispatch has
    /// already been applied by the reducer.
    SuppressedReentrant,
}

/// Per-topic observer registry: registers listeners, cancels subscriptions,
/// and dispatches state changes.
///
/// Cheaply cloneable (an `Arc` of the registry core), so the broadcaster can
/// be shared with a listener for the re-entrant drive pattern or passed to
/// another component without lifetime gymnastics.
#[derive(Clone, Default)]
pub struct NavigationStateBroadcaster {
    core: Arc<BroadcasterCore>,
}

/// The shared registry core behind [`NavigationStateBroadcaster`].
struct BroadcasterCore {
    /// The registry lock. Held only for registration, cancellation and list
    /// snapshots — never across a listener callback.
    registry: Mutex<RegistryState>,
    /// The single-dispatch-in-flight flag backing the re-entrancy guard.
    dispatch_in_flight: AtomicBool,
}

impl Default for BroadcasterCore {
    fn default() -> Self {
        Self {
            registry: Mutex::new(RegistryState::default()),
            dispatch_in_flight: AtomicBool::new(false),
        }
    }
}

/// The locked registry state: one listener list per topic plus the
/// registration id counter.
#[derive(Default)]
struct RegistryState {
    instruction_registrations: Vec<ListenerRegistration>,
    deviation_registrations: Vec<ListenerRegistration>,
    phase_registrations: Vec<ListenerRegistration>,
    speech_registrations: Vec<ListenerRegistration>,
    next_registration_id: u64,
}

/// One registered listener in one topic list.
struct ListenerRegistration {
    registration_id: u64,
    listener: Arc<dyn NavigationStateChangeListener>,
}

/// Handle coupled to exactly one `(topic, listener)` registration.
///
/// Calling [`TopicSubscription::cancel`] removes exactly that registration —
/// a listener subscribed to several topics (or twice to one) loses only the
/// subscription the handle names. Cancellation is explicit and idempotent;
/// dropping the handle does *not* unsubscribe, so a long-lived consumer keeps
/// delivering until it cancels.
#[derive(Clone)]
pub struct TopicSubscription {
    core: Arc<BroadcasterCore>,
    registration_id: u64,
}

impl TopicSubscription {
    /// Cancels this subscription: the named listener stops receiving the
    /// subscription's topic from the next dispatch on. Idempotent.
    pub fn cancel(&self) {
        #[allow(clippy::expect_used)] // poisoned-lock assert: justified below
        let mut registry_state = self.core.registry.lock().expect(
            "the registry mutex cannot be poisoned: no listener code runs while it is held",
        );
        let RegistryState {
            instruction_registrations,
            deviation_registrations,
            phase_registrations,
            speech_registrations,
            ..
        } = &mut *registry_state;
        // Registration ids are unique across all topics, so filtering every
        // list removes exactly this subscription and nothing else.
        instruction_registrations
            .retain(|registration| registration.registration_id != self.registration_id);
        deviation_registrations
            .retain(|registration| registration.registration_id != self.registration_id);
        phase_registrations
            .retain(|registration| registration.registration_id != self.registration_id);
        speech_registrations
            .retain(|registration| registration.registration_id != self.registration_id);
    }
}

impl NavigationStateBroadcaster {
    /// Builds an empty broadcaster with no subscriptions.
    pub fn new() -> Self {
        Self::default()
    }

    /// Subscribes `listener` to the instructions topic (every dispatched
    /// state change's [`NavigationInstruction`] payload), in registration
    /// order.
    pub fn subscribe_to_navigation_instructions(
        &self,
        listener: Arc<dyn NavigationStateChangeListener>,
    ) -> TopicSubscription {
        self.register(listener, |registry_state| {
            &mut registry_state.instruction_registrations
        })
    }

    /// Subscribes `listener` to the deviation-status topic, in registration
    /// order.
    pub fn subscribe_to_deviation_status(
        &self,
        listener: Arc<dyn NavigationStateChangeListener>,
    ) -> TopicSubscription {
        self.register(listener, |registry_state| {
            &mut registry_state.deviation_registrations
        })
    }

    /// Subscribes `listener` to the phase-transition topic, in registration
    /// order.
    pub fn subscribe_to_phase_changes(
        &self,
        listener: Arc<dyn NavigationStateChangeListener>,
    ) -> TopicSubscription {
        self.register(listener, |registry_state| {
            &mut registry_state.phase_registrations
        })
    }

    /// Subscribes `listener` to the speech-trigger topic, in registration
    /// order.
    pub fn subscribe_to_speech_utterances(
        &self,
        listener: Arc<dyn NavigationStateChangeListener>,
    ) -> TopicSubscription {
        self.register(listener, |registry_state| {
            &mut registry_state.speech_registrations
        })
    }

    /// Dispatches the state produced by a
    /// [`NavigationEvent::StartTrip`](crate::reducer::NavigationEvent::StartTrip)
    /// event: the initial instruction payload is delivered; no deviation or
    /// phase transitions exist yet.
    pub fn dispatch_trip_started(
        &self,
        current_state: &TripNavigationState,
        unit_system: UnitSystem,
    ) -> NavigationDispatchOutcome {
        self.dispatch_state_change(None, current_state, false, unit_system)
    }

    /// Dispatches the transition a
    /// [`NavigationEvent::LocationUpdated`](crate::reducer::NavigationEvent::LocationUpdated)
    /// event produced: `previous_state` is the state before the fix,
    /// `current_state` the state after it.
    pub fn dispatch_location_update(
        &self,
        previous_state: &TripNavigationState,
        current_state: &TripNavigationState,
        unit_system: UnitSystem,
    ) -> NavigationDispatchOutcome {
        self.dispatch_state_change(Some(previous_state), current_state, false, unit_system)
    }

    /// Dispatches the transition a
    /// [`NavigationEvent::RerouteProvided`](crate::reducer::NavigationEvent::RerouteProvided)
    /// event produced. The `RerouteApplied` phase transition is delivered
    /// only when the event actually applied — a reroute ignored by the
    /// reducer (an arrived trip) leaves the states equal and yields no
    /// transition.
    pub fn dispatch_reroute_applied(
        &self,
        previous_state: &TripNavigationState,
        current_state: &TripNavigationState,
        unit_system: UnitSystem,
    ) -> NavigationDispatchOutcome {
        self.dispatch_state_change(Some(previous_state), current_state, true, unit_system)
    }

    /// Registers a listener on one topic list and returns its subscription.
    fn register(
        &self,
        listener: Arc<dyn NavigationStateChangeListener>,
        topic_list: impl FnOnce(&mut RegistryState) -> &mut Vec<ListenerRegistration>,
    ) -> TopicSubscription {
        let registration_id = {
            #[allow(clippy::expect_used)] // poisoned-lock assert: justified below
            let mut registry_state = self.core.registry.lock().expect(
                "the registry mutex cannot be poisoned: no listener code runs while it is held",
            );
            let registration_id = registry_state.next_registration_id;
            registry_state.next_registration_id += 1;
            topic_list(&mut registry_state).push(ListenerRegistration {
                registration_id,
                listener,
            });
            registration_id
        };
        TopicSubscription {
            core: Arc::clone(&self.core),
            registration_id,
        }
    }

    /// Dispatches one speech-trigger utterance to the speech topic, in
    /// registration order, under the same single-in-flight guard as the
    /// state-change dispatches.
    pub fn dispatch_speech_utterance(
        &self,
        speech_utterance: &crate::speech::SpeechUtterance,
    ) -> NavigationDispatchOutcome {
        let entered_dispatch = self
            .core
            .dispatch_in_flight
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok();
        if !entered_dispatch {
            return NavigationDispatchOutcome::SuppressedReentrant;
        }
        let in_flight_guard = DispatchInFlightGuard {
            dispatch_in_flight: &self.core.dispatch_in_flight,
        };

        let speech_listeners = {
            #[allow(clippy::expect_used)] // poisoned-lock assert: justified below
            let registry_state = self.core.registry.lock().expect(
                "the registry mutex cannot be poisoned: no listener code runs while it is held",
            );
            snapshot_listener_list(&registry_state.speech_registrations)
        };

        for listener in &speech_listeners {
            listener.on_speech_utterance(speech_utterance);
        }

        drop(in_flight_guard);
        NavigationDispatchOutcome::Delivered
    }

    /// The shared dispatch: delivers the instruction payload, then deviation
    /// status, then phase transitions — each topic in registration order —
    /// holding no lock across any callback.
    fn dispatch_state_change(
        &self,
        previous_state: Option<&TripNavigationState>,
        current_state: &TripNavigationState,
        is_reroute_event: bool,
        unit_system: UnitSystem,
    ) -> NavigationDispatchOutcome {
        let entered_dispatch = self
            .core
            .dispatch_in_flight
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok();
        if !entered_dispatch {
            return NavigationDispatchOutcome::SuppressedReentrant;
        }
        // The guard releases the in-flight flag on every exit path, including
        // an unwind out of a panicking listener.
        let in_flight_guard = DispatchInFlightGuard {
            dispatch_in_flight: &self.core.dispatch_in_flight,
        };

        let (instruction_listeners, deviation_listeners, phase_listeners) = {
            #[allow(clippy::expect_used)] // poisoned-lock assert: justified below
            let registry_state = self.core.registry.lock().expect(
                "the registry mutex cannot be poisoned: no listener code runs while it is held",
            );
            (
                snapshot_listener_list(&registry_state.instruction_registrations),
                snapshot_listener_list(&registry_state.deviation_registrations),
                snapshot_listener_list(&registry_state.phase_registrations),
            )
        };

        let instruction = compose_navigation_instructions(current_state, unit_system);
        for listener in &instruction_listeners {
            listener.on_navigation_instruction(&instruction);
        }

        if let Some(deviation_status_change) =
            deviation_status_change_between(previous_state, current_state)
        {
            for listener in &deviation_listeners {
                listener.on_deviation_status_change(&deviation_status_change);
            }
        }

        for phase_change in phase_changes_between(previous_state, current_state, is_reroute_event) {
            for listener in &phase_listeners {
                listener.on_navigation_phase_change(&phase_change);
            }
        }

        drop(in_flight_guard);
        NavigationDispatchOutcome::Delivered
    }
}

/// Releases the dispatch-in-flight flag when the dispatch leaves scope.
struct DispatchInFlightGuard<'guard> {
    dispatch_in_flight: &'guard AtomicBool,
}

impl Drop for DispatchInFlightGuard<'_> {
    fn drop(&mut self) {
        self.dispatch_in_flight.store(false, Ordering::SeqCst);
    }
}

/// Clones the listener handles of one topic list for lock-free delivery.
fn snapshot_listener_list(
    registrations: &[ListenerRegistration],
) -> Vec<Arc<dyn NavigationStateChangeListener>> {
    registrations
        .iter()
        .map(|registration| Arc::clone(&registration.listener))
        .collect()
}

/// Classifies the deviation-topic update between two states: delivered on a
/// deviation entering, updating its measurement while persisting, or
/// clearing — and never between identical deviation snapshots.
fn deviation_status_change_between(
    previous_state: Option<&TripNavigationState>,
    current_state: &TripNavigationState,
) -> Option<DeviationStatusChange> {
    let previous_state = previous_state?;
    if previous_state.deviation == current_state.deviation {
        return None;
    }
    Some(match current_state.deviation {
        Some(deviation_info) => DeviationStatusChange::OffRoute(deviation_info),
        None => DeviationStatusChange::OnRoute,
    })
}

/// Classifies the phase-topic transitions between two states. A reroute event
/// contributes `RerouteApplied` only when it actually applied (the states
/// differ); an ignored reroute on an arrived trip contributes nothing.
fn phase_changes_between(
    previous_state: Option<&TripNavigationState>,
    current_state: &TripNavigationState,
    is_reroute_event: bool,
) -> Vec<NavigationPhaseChange> {
    let mut phase_changes = Vec::new();
    let Some(previous_state) = previous_state else {
        // A trip start is not a transition: the fresh state's phase is its
        // origin, with no previous phase to leave.
        return phase_changes;
    };
    let did_reroute_apply = is_reroute_event && previous_state != current_state;
    if did_reroute_apply {
        phase_changes.push(NavigationPhaseChange::RerouteApplied);
    }
    match (previous_state.phase, current_state.phase) {
        (NavigationPhase::Navigating, NavigationPhase::OffRoute) => {
            phase_changes.push(NavigationPhaseChange::OffRouteEntered);
        }
        (NavigationPhase::OffRoute, NavigationPhase::Navigating) => {
            phase_changes.push(NavigationPhaseChange::OffRouteCleared);
        }
        (NavigationPhase::Navigating | NavigationPhase::OffRoute, NavigationPhase::Arrived) => {
            phase_changes.push(NavigationPhaseChange::Arrived);
        }
        _ => {}
    }
    phase_changes
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use geospatial_types::{
        BoundingBox, LaneGuidance, ManeuverType, RouteData, RouteLeg, RouteStep, UnitSystem,
    };

    use super::{
        DeviationStatusChange, NavigationDispatchOutcome, NavigationPhaseChange,
        NavigationStateBroadcaster, NavigationStateChangeListener, TopicSubscription,
    };
    use crate::course_filtering::CourseFiltering;
    use crate::deviation::{
        DEFAULT_DEVIATION_DISTANCE_THRESHOLD_METERS, DistanceDeviationDetector,
    };
    use crate::instruction::{
        DistancePhrase, DistanceUnit, ManeuverInstructionText, NavigationInstruction,
    };
    use crate::location::NavigationLocationUpdate;
    use crate::reducer::{NavigationEvent, NavigationSession, reduce_navigation_event};
    use crate::state::{
        DeviationGrade, DeviationInfo, NavigationPhase, TripNavigationState, TripProgress,
    };
    use crate::step_advance::ManualStepAdvanceCondition;
    use crate::test_support::{TEST_ACCURACY_METERS, offset_coordinate};

    /// Length, in meters, of each synthetic step.
    const TEST_STEP_LENGTH_METERS: f64 = 200.0;

    /// Records every callback on every topic, in arrival order per topic.
    #[derive(Default)]
    struct RecordingListener {
        instruction_records: Mutex<Vec<NavigationInstruction>>,
        deviation_changes: Mutex<Vec<DeviationStatusChange>>,
        phase_changes: Mutex<Vec<NavigationPhaseChange>>,
    }

    impl NavigationStateChangeListener for RecordingListener {
        fn on_navigation_instruction(&self, instruction: &NavigationInstruction) {
            self.instruction_records
                .lock()
                .unwrap()
                .push(instruction.clone());
        }

        fn on_deviation_status_change(&self, deviation_status_change: &DeviationStatusChange) {
            self.deviation_changes
                .lock()
                .unwrap()
                .push(deviation_status_change.clone());
        }

        fn on_navigation_phase_change(&self, phase_change: &NavigationPhaseChange) {
            self.phase_changes.lock().unwrap().push(*phase_change);
        }
    }

    /// Appends its tag to a shared log on one subscribed topic only.
    struct TaggedListener {
        listener_tag: u8,
        subscribed_topic: SubscribedTopic,
        delivery_log: Arc<Mutex<Vec<(u8, SubscribedTopic)>>>,
    }

    /// The topic a [`TaggedListener`] records deliveries for.
    #[derive(Clone, Copy, PartialEq)]
    enum SubscribedTopic {
        Instructions,
        Phase,
    }

    impl NavigationStateChangeListener for TaggedListener {
        fn on_navigation_instruction(&self, _instruction: &NavigationInstruction) {
            if self.subscribed_topic == SubscribedTopic::Instructions {
                self.delivery_log
                    .lock()
                    .unwrap()
                    .push((self.listener_tag, self.subscribed_topic));
            }
        }

        fn on_navigation_phase_change(&self, _phase_change: &NavigationPhaseChange) {
            if self.subscribed_topic == SubscribedTopic::Phase {
                self.delivery_log
                    .lock()
                    .unwrap()
                    .push((self.listener_tag, self.subscribed_topic));
            }
        }
    }

    /// Builds a plain eastward step of [`TEST_STEP_LENGTH_METERS`].
    fn eastward_step(start_offset_east_meters: f64, maneuver_type: ManeuverType) -> RouteStep {
        RouteStep {
            distance_meters: TEST_STEP_LENGTH_METERS,
            duration_seconds: 20.0,
            maneuver_type,
            instruction_display_text: "Instruction".to_string(),
            instruction_speech_text: "Instruction".to_string(),
            shape: vec![
                offset_coordinate(start_offset_east_meters, 0.0),
                offset_coordinate(start_offset_east_meters + TEST_STEP_LENGTH_METERS, 0.0),
            ],
            lane_guidances: Vec::<LaneGuidance>::new(),
        }
    }

    /// Builds a one-leg, one-step route (kept trivial: broadcasting tests are
    /// about delivery, not geometry).
    fn single_step_route() -> RouteData {
        let step = eastward_step(0.0, ManeuverType::Arrival);
        let bounding_box = BoundingBox {
            south_west: offset_coordinate(0.0, 0.0),
            north_east: offset_coordinate(TEST_STEP_LENGTH_METERS, 0.0),
        };
        RouteData {
            distance_meters: TEST_STEP_LENGTH_METERS,
            duration_seconds: 20.0,
            bounding_box,
            geometry: Vec::new(),
            legs: vec![RouteLeg {
                distance_meters: TEST_STEP_LENGTH_METERS,
                duration_seconds: 20.0,
                steps: vec![step],
            }],
        }
    }

    /// Builds a fresh navigating state for `route` (the reducer's own shape
    /// right after `StartTrip`).
    fn navigating_state(route: RouteData) -> TripNavigationState {
        TripNavigationState {
            phase: NavigationPhase::Navigating,
            step_positions: TripNavigationState::build_step_positions(&route),
            progress: TripProgress {
                distance_to_next_maneuver_meters: TEST_STEP_LENGTH_METERS,
                distance_remaining_meters: TEST_STEP_LENGTH_METERS,
                duration_remaining_seconds: 20.0,
            },
            snapped_coordinate: None,
            deviation: None,
            course_heading_degrees: None,
            current_step_position: 0,
            route,
        }
    }

    /// An off-route clone of `state` carrying the given deviation measurement.
    fn off_route_state(
        state: &TripNavigationState,
        deviation_from_route_meters: f64,
    ) -> TripNavigationState {
        let mut off_route_state = state.clone();
        off_route_state.phase = NavigationPhase::OffRoute;
        off_route_state.deviation = Some(DeviationInfo {
            grade: DeviationGrade::FullyOffRoute,
            deviation_from_route_meters,
        });
        off_route_state
    }

    /// A default-detector trip on [`single_step_route`], started manually.
    fn started_session() -> NavigationSession {
        reduce_navigation_event(
            None,
            NavigationEvent::StartTrip {
                route: single_step_route(),
                arrival_step_advance_condition: None,
                course_filtering: CourseFiltering::Raw,
                step_advance_condition: Arc::new(ManualStepAdvanceCondition),
                deviation_detector: Arc::new(DistanceDeviationDetector::with_default_threshold()),
            },
        )
        .unwrap()
    }

    /// A graded fix east of the sample origin.
    fn route_fix(east_meters: f64) -> NavigationLocationUpdate {
        NavigationLocationUpdate {
            coordinate: offset_coordinate(east_meters, 0.0),
            horizontal_accuracy_meters: Some(TEST_ACCURACY_METERS),
            course_heading_degrees: None,
            timestamp_seconds: None,
        }
    }

    /// A fully default deviation detector would widen the threshold by the fix's
    /// accuracy; this sample's excursion is deliberately deep enough to stay
    /// beyond it.
    fn excursion_beyond_default_allowance_meters() -> f64 {
        DEFAULT_DEVIATION_DISTANCE_THRESHOLD_METERS + TEST_ACCURACY_METERS + 1.0
    }

    #[test]
    fn instructions_deliver_on_every_dispatch_and_transitions_only_on_change() {
        let broadcaster = NavigationStateBroadcaster::new();
        let listener = Arc::new(RecordingListener::default());
        let _instructions_subscription =
            broadcaster.subscribe_to_navigation_instructions(listener.clone());
        let _deviation_subscription = broadcaster.subscribe_to_deviation_status(listener.clone());
        let _phase_subscription = broadcaster.subscribe_to_phase_changes(listener.clone());

        let route = single_step_route();
        let state = navigating_state(route.clone());

        // Trip start: the payload arrives, but there is no transition yet.
        assert_eq!(
            broadcaster.dispatch_trip_started(&state, UnitSystem::Metric),
            NavigationDispatchOutcome::Delivered
        );
        assert_eq!(listener.instruction_records.lock().unwrap().len(), 1);
        assert!(listener.deviation_changes.lock().unwrap().is_empty());
        assert!(listener.phase_changes.lock().unwrap().is_empty());

        // Identical states: the instruction re-delivers, no transition fires.
        assert_eq!(
            broadcaster.dispatch_location_update(&state, &state, UnitSystem::Metric),
            NavigationDispatchOutcome::Delivered
        );
        assert_eq!(listener.instruction_records.lock().unwrap().len(), 2);
        assert!(listener.deviation_changes.lock().unwrap().is_empty());
        assert!(listener.phase_changes.lock().unwrap().is_empty());
    }

    #[test]
    fn deviation_transitions_enter_update_and_clear() {
        let broadcaster = NavigationStateBroadcaster::new();
        let listener = Arc::new(RecordingListener::default());
        let _deviation_subscription = broadcaster.subscribe_to_deviation_status(listener.clone());
        let _phase_subscription = broadcaster.subscribe_to_phase_changes(listener.clone());

        let state = navigating_state(single_step_route());
        let excursion_meters = excursion_beyond_default_allowance_meters();
        let off_route_judged_state = off_route_state(&state, excursion_meters);
        let mut deeper_off_route_state = off_route_judged_state.clone();
        deeper_off_route_state.deviation = Some(DeviationInfo {
            grade: DeviationGrade::FullyOffRoute,
            deviation_from_route_meters: excursion_meters * 2.0,
        });
        let recovered_state = navigating_state(single_step_route());

        broadcaster.dispatch_location_update(&state, &off_route_judged_state, UnitSystem::Metric);
        broadcaster.dispatch_location_update(
            &off_route_judged_state,
            &deeper_off_route_state,
            UnitSystem::Metric,
        );
        broadcaster.dispatch_location_update(
            &deeper_off_route_state,
            &recovered_state,
            UnitSystem::Metric,
        );

        // The deviation topic streams entry, drift, and clearing...
        assert_eq!(
            *listener.deviation_changes.lock().unwrap(),
            vec![
                DeviationStatusChange::OffRoute(DeviationInfo {
                    grade: DeviationGrade::FullyOffRoute,
                    deviation_from_route_meters: excursion_meters,
                }),
                DeviationStatusChange::OffRoute(DeviationInfo {
                    grade: DeviationGrade::FullyOffRoute,
                    deviation_from_route_meters: excursion_meters * 2.0,
                }),
                DeviationStatusChange::OnRoute,
            ]
        );
        // ... while the phase topic saw the entered transition and the
        // recovery's clear (the measurement-drift dispatch fired neither).
        assert_eq!(
            *listener.phase_changes.lock().unwrap(),
            vec![
                NavigationPhaseChange::OffRouteEntered,
                NavigationPhaseChange::OffRouteCleared,
            ]
        );
    }

    #[test]
    fn phase_transitions_cover_off_route_and_arrival() {
        let broadcaster = NavigationStateBroadcaster::new();
        let listener = Arc::new(RecordingListener::default());
        let _phase_subscription = broadcaster.subscribe_to_phase_changes(listener.clone());

        let route = single_step_route();
        let state = navigating_state(route.clone());
        let excursion_meters = excursion_beyond_default_allowance_meters();
        let off_route_judged_state = off_route_state(&state, excursion_meters);
        let recovered_state = navigating_state(route.clone());
        let mut arrived_state = navigating_state(route);
        arrived_state.phase = NavigationPhase::Arrived;

        broadcaster.dispatch_location_update(&state, &off_route_judged_state, UnitSystem::Metric);
        broadcaster.dispatch_location_update(
            &off_route_judged_state,
            &recovered_state,
            UnitSystem::Metric,
        );
        broadcaster.dispatch_location_update(&recovered_state, &arrived_state, UnitSystem::Metric);
        // Off route straight into arrival is also reachable in one event: the
        // recovering fix both lifts the deviation and completes the final
        // step, so the classifier must cover that arm too.
        let deviated_before_arrival = off_route_state(&recovered_state, excursion_meters);
        broadcaster.dispatch_location_update(
            &deviated_before_arrival,
            &arrived_state,
            UnitSystem::Metric,
        );

        assert_eq!(
            *listener.phase_changes.lock().unwrap(),
            vec![
                NavigationPhaseChange::OffRouteEntered,
                NavigationPhaseChange::OffRouteCleared,
                NavigationPhaseChange::Arrived,
                NavigationPhaseChange::Arrived,
            ]
        );
    }

    #[test]
    fn reroute_applied_delivers_the_transition_and_lifts_the_deviation_events() {
        let broadcaster = NavigationStateBroadcaster::new();
        let listener = Arc::new(RecordingListener::default());
        let _deviation_subscription = broadcaster.subscribe_to_deviation_status(listener.clone());
        let _phase_subscription = broadcaster.subscribe_to_phase_changes(listener.clone());

        let state = navigating_state(single_step_route());
        let excursion_meters = excursion_beyond_default_allowance_meters();
        let off_route_judged_state = off_route_state(&state, excursion_meters);
        // A reroute anchored at the deviation: the rebuilt state is a fresh
        // navigating projection over the replacement route's own declaration.
        let mut rerouted_state = navigating_state(single_step_route());
        rerouted_state.progress = TripProgress {
            distance_to_next_maneuver_meters: TEST_STEP_LENGTH_METERS,
            distance_remaining_meters: TEST_STEP_LENGTH_METERS,
            duration_remaining_seconds: 20.0,
        };

        broadcaster.dispatch_reroute_applied(
            &off_route_judged_state,
            &rerouted_state,
            UnitSystem::Metric,
        );

        // Off-route -> navigating is a genuine double transition here: the
        // reroute both replaced the route and lifted the deviation.
        assert_eq!(
            *listener.phase_changes.lock().unwrap(),
            vec![
                NavigationPhaseChange::RerouteApplied,
                NavigationPhaseChange::OffRouteCleared,
            ]
        );
        assert_eq!(
            *listener.deviation_changes.lock().unwrap(),
            vec![DeviationStatusChange::OnRoute]
        );
    }

    #[test]
    fn reroute_ignored_on_an_arrived_trip_yields_no_transition() {
        let broadcaster = NavigationStateBroadcaster::new();
        let listener = Arc::new(RecordingListener::default());
        let _phase_subscription = broadcaster.subscribe_to_phase_changes(listener.clone());
        let _instructions_subscription =
            broadcaster.subscribe_to_navigation_instructions(listener.clone());

        let mut arrived_state = navigating_state(single_step_route());
        arrived_state.phase = NavigationPhase::Arrived;

        // The reducer ignored this reroute, so the caller dispatches with
        // equal states: no `RerouteApplied` may fire.
        assert_eq!(
            broadcaster.dispatch_reroute_applied(
                &arrived_state,
                &arrived_state,
                UnitSystem::Metric
            ),
            NavigationDispatchOutcome::Delivered
        );
        assert!(listener.phase_changes.lock().unwrap().is_empty());
        // The instruction payload still re-delivers on a no-op dispatch.
        assert_eq!(listener.instruction_records.lock().unwrap().len(), 1);
    }

    #[test]
    fn cancel_stops_delivery_to_its_own_subscription() {
        let broadcaster = NavigationStateBroadcaster::new();
        let first_listener = Arc::new(RecordingListener::default());
        let second_listener = Arc::new(RecordingListener::default());
        let first_subscription =
            broadcaster.subscribe_to_navigation_instructions(first_listener.clone());
        let _second_subscription =
            broadcaster.subscribe_to_navigation_instructions(second_listener.clone());

        let state = navigating_state(single_step_route());
        broadcaster.dispatch_trip_started(&state, UnitSystem::Metric);
        assert_eq!(first_listener.instruction_records.lock().unwrap().len(), 1);
        assert_eq!(second_listener.instruction_records.lock().unwrap().len(), 1);

        first_subscription.cancel();
        broadcaster.dispatch_trip_started(&state, UnitSystem::Metric);
        // Exactly the cancelled subscription went quiet.
        assert_eq!(first_listener.instruction_records.lock().unwrap().len(), 1);
        assert_eq!(second_listener.instruction_records.lock().unwrap().len(), 2);
    }

    #[test]
    fn cancelling_one_of_two_subscriptions_of_the_same_listener_keeps_one() {
        let broadcaster = NavigationStateBroadcaster::new();
        let listener = Arc::new(RecordingListener::default());
        let first_subscription = broadcaster.subscribe_to_navigation_instructions(listener.clone());
        let _second_subscription =
            broadcaster.subscribe_to_navigation_instructions(listener.clone());

        first_subscription.cancel();
        let state = navigating_state(single_step_route());
        broadcaster.dispatch_trip_started(&state, UnitSystem::Metric);
        // One of the two registrations is gone; exactly one delivery remains.
        assert_eq!(listener.instruction_records.lock().unwrap().len(), 1);
    }

    #[test]
    fn topics_are_isolated_per_subscription() {
        let broadcaster = NavigationStateBroadcaster::new();
        let listener = Arc::new(RecordingListener::default());
        let _instructions_subscription =
            broadcaster.subscribe_to_navigation_instructions(listener.clone());

        let state = navigating_state(single_step_route());
        let off_route_judged_state = off_route_state(&state, 100.0);
        broadcaster.dispatch_location_update(&state, &off_route_judged_state, UnitSystem::Metric);
        // Only the instruction topic received anything.
        assert_eq!(listener.instruction_records.lock().unwrap().len(), 1);
        assert!(listener.deviation_changes.lock().unwrap().is_empty());
        assert!(listener.phase_changes.lock().unwrap().is_empty());
    }

    #[test]
    fn delivery_follows_registration_order_within_a_topic() {
        let broadcaster = NavigationStateBroadcaster::new();
        let delivery_log = Arc::new(Mutex::new(Vec::new()));
        let subscribers: Vec<(u8, TopicSubscription)> = (1..=3)
            .map(|listener_tag| {
                let listener = Arc::new(TaggedListener {
                    listener_tag,
                    subscribed_topic: SubscribedTopic::Phase,
                    delivery_log: Arc::clone(&delivery_log),
                });
                (
                    listener_tag,
                    broadcaster.subscribe_to_phase_changes(listener),
                )
            })
            .collect();
        assert_eq!(subscribers.len(), 3);

        let state = navigating_state(single_step_route());
        let off_route_judged_state = off_route_state(&state, 100.0);
        broadcaster.dispatch_location_update(&state, &off_route_judged_state, UnitSystem::Metric);

        let logged_tags = delivery_log
            .lock()
            .unwrap()
            .iter()
            .map(|(listener_tag, _)| *listener_tag)
            .collect::<Vec<_>>();
        assert_eq!(logged_tags, vec![1, 2, 3], "registration order must hold");
    }

    #[test]
    fn instruction_payload_carries_the_composed_content() {
        let broadcaster = NavigationStateBroadcaster::new();
        let listener = Arc::new(RecordingListener::default());
        let _instructions_subscription =
            broadcaster.subscribe_to_navigation_instructions(listener.clone());

        let state = navigating_state(single_step_route());
        broadcaster.dispatch_trip_started(&state, UnitSystem::Metric);
        let delivered = &listener.instruction_records.lock().unwrap()[0];
        assert_eq!(
            delivered.active_maneuver.distance_phrase,
            DistancePhrase::Quantity {
                magnitude: 200.0,
                unit: DistanceUnit::Meters,
            }
        );
        assert!(delivered.active_maneuver.instruction != ManeuverInstructionText::Unavailable);
        assert!(!delivered.is_arrival);
        assert!(!delivered.is_rerouting_required);
    }

    #[test]
    fn reentrant_dispatch_from_a_listener_is_suppressed_and_bounded() {
        /// Drive rounds the listener deliberately performs inside its own
        /// callback before falling quiet — the structural ceiling that keeps
        /// recursion finite even if the suppression guard regressed.
        const REENTRANT_DRIVE_BUDGET: usize = 2;

        struct ReentrantDrivingListener {
            broadcaster: NavigationStateBroadcaster,
            trip_session: Arc<Mutex<Option<NavigationSession>>>,
            remaining_drive_budget: AtomicUsize,
            observed_outcomes: Mutex<Vec<NavigationDispatchOutcome>>,
        }

        impl ReentrantDrivingListener {
            /// Consumes one drive round from the bounded budget. The
            /// budget is test-local single-threaded state (the dispatch
            /// contract itself), so a load/store pair suffices.
            fn take_drive_round(&self) -> bool {
                let remaining = self.remaining_drive_budget.load(Ordering::SeqCst);
                if remaining == 0 {
                    return false;
                }
                self.remaining_drive_budget
                    .store(remaining - 1, Ordering::SeqCst);
                true
            }
        }

        impl NavigationStateChangeListener for ReentrantDrivingListener {
            fn on_navigation_instruction(&self, _instruction: &NavigationInstruction) {
                if !self.take_drive_round() {
                    return;
                }
                let mut session_guard = self.trip_session.lock().unwrap();
                let Some(session) = session_guard.as_ref() else {
                    return;
                };
                let previous_state = session.state.clone();
                // The listener drives the reducer (a real event, a moved
                // walker fix) and re-dispatches during its own callback.
                let walker_offset_meters = previous_state.progress.distance_remaining_meters;
                let reduced_session = reduce_navigation_event(
                    Some(session.clone()),
                    NavigationEvent::LocationUpdated(route_fix(walker_offset_meters / 2.0)),
                )
                .expect("the driven session must stay alive");
                let current_state = reduced_session.state.clone();
                *session_guard = Some(reduced_session);
                drop(session_guard);
                let outcome = self.broadcaster.dispatch_location_update(
                    &previous_state,
                    &current_state,
                    UnitSystem::Metric,
                );
                self.observed_outcomes.lock().unwrap().push(outcome);
            }
        }

        let broadcaster = NavigationStateBroadcaster::new();
        let trip_session = Arc::new(Mutex::new(Some(started_session())));
        let listener = Arc::new(ReentrantDrivingListener {
            broadcaster: broadcaster.clone(),
            trip_session: Arc::clone(&trip_session),
            remaining_drive_budget: AtomicUsize::new(REENTRANT_DRIVE_BUDGET),
            observed_outcomes: Mutex::new(Vec::new()),
        });
        let _instructions_subscription =
            broadcaster.subscribe_to_navigation_instructions(listener.clone());

        // The top-level dispatch runs; its sole instruction callback drives
        // one reducer event and attempts a re-dispatch that must be
        // suppressed (the outer dispatch is still in flight), so no cascade
        // can form and the call returns.
        let initial_state = trip_session.lock().unwrap().as_ref().unwrap().state.clone();
        assert_eq!(
            broadcaster.dispatch_trip_started(&initial_state, UnitSystem::Metric),
            NavigationDispatchOutcome::Delivered
        );
        let first_round_outcomes = listener.observed_outcomes.lock().unwrap().clone();
        assert_eq!(
            first_round_outcomes,
            vec![NavigationDispatchOutcome::SuppressedReentrant],
            "the nested dispatch must be suppressed exactly once, not cascade"
        );
        // The driven state change did land in the session even though its
        // broadcast was skipped.
        let driven_state = trip_session.lock().unwrap().as_ref().unwrap().state.clone();
        assert!(driven_state.snapped_coordinate.is_some());
        assert_ne!(driven_state, initial_state);

        // After the guard released, a fresh top-level dispatch delivers
        // normally again; its listener callback may then drive one more
        // bounded round, whose nested dispatch is suppressed in turn. The
        // test completing is itself the no-deadlock, no-infinite-recursion
        // proof; the outcome log pins the exact shape.
        broadcaster.dispatch_trip_started(&driven_state, UnitSystem::Metric);
        let outcomes_after_second_dispatch = listener.observed_outcomes.lock().unwrap().clone();
        assert_eq!(
            outcomes_after_second_dispatch,
            vec![
                NavigationDispatchOutcome::SuppressedReentrant,
                NavigationDispatchOutcome::SuppressedReentrant,
            ]
        );
    }
}
