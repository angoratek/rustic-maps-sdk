//! The navigation wiring surface: the `NavigationListener`
//! foreign-callback contract foreign code implements to observe the
//! navigation state, the [`Navigator`] driving a native
//! `rustic-maps-navigation` session behind it, and the policy records configuring
//! the controller's step-advance and deviation strategies.
//!
//! The controller owns a [`NavigationStateBroadcaster`] and the single active
//! [`NavigationSession`](rustic_maps_navigation::NavigationSession), and feeds the
//! broadcaster's dispatch calls from the
//! exported methods, so the per-topic subscription semantics, the delivery
//! order and the outcome codes are exactly the native broadcaster's (see
//! `rustic-maps-navigation`'s `observer` module documentation).
//!
//! # What the FFI seam changes about the re-entrancy rules
//!
//! The broadcaster's own rules hold unchanged across the seam: the
//! controller's active-trip lock is taken only to clone state snapshots and
//! is released before the first dispatch, so the lock is never held across a
//! listener callback and a listener may freely call back into the controller
//! — including
//! [`Navigator::current_trip_state`] and subscription
//! management. Two properties are worth naming at this seam:
//!
//! - A foreign listener calling back into a controller-providing method while
//!   a dispatch from that controller is still in flight hits the
//!   broadcaster's single-in-flight guard and its nested dispatch is
//!   suppressed exactly as a Rust-side re-entrant dispatch would be; the
//!   state change behind the suppressed dispatch has already been applied,
//!   only its broadcast is skipped. The synchronous single-threaded callback
//!   ordering of the native seam carries over: uniffi runs these sync
//!   callbacks on the thread driving the controller call, so no cross-thread
//!   queueing is introduced.
//! - A panicking (Rust-side) listener propagates out of the dispatch on the
//!   native seam unchanged: nothing is caught inside the controller. The
//!   panic still cannot unwind into foreign code — it stays inside the
//!   exported frame whose scaffolding boundary catches it (see
//!   [`crate::panic_boundary`]); foreign listeners raising exceptions take
//!   the foreign vtable's own report, contained the same way.
//!
//! # Scope
//!
//! Trip progress and instructions are read-only for foreign code; the
//! programmatic step advance is deliberately absent — the reducer has no such
//! event yet, and the `Manual` step-advance policy consequently cannot be
//! driven across this surface.

use std::sync::{Arc, Mutex};
use web_time::Instant;

use geospatial_types::UnitSystem;
use rustic_maps_navigation::{
    DeviationDetector, DistanceDeviationDetector, DistanceFromStepCondition,
    DistanceToEndOfStepCondition, ManualStepAdvanceCondition, NavigationDispatchOutcome,
    NavigationStateBroadcaster, RerouteRecalculationGate, SpeechInstructionPlanner,
    StepAdvanceCondition, TopicSubscription, compose_navigation_instructions,
    reduce_navigation_event,
};

use crate::error::SdkError;
use crate::records::{
    DeviationPolicy, DeviationPolicyKind, DeviationStatusChange, NavigationInstruction,
    NavigationLocationUpdate, NavigationPhaseChange, ReroutePolicy, Route, SpeechInstruction,
    StepAdvancePolicy, StepAdvancePolicyKind,
};

/// State listener contract implemented by foreign code; register its
/// subscriptions on [`Navigator`] through the
/// `subscribe_to_*` methods.
///
/// The callbacks exchange the binding records of the navigation state and
/// have no error channel: a listener cannot fail into the broadcaster (the
/// native dispatch rules). Each method of one registered subscription is
/// called only for the topics the subscription was created for.
pub trait NavigationListener: Send + Sync {
    /// Receives the composed instruction payload of every dispatched state
    /// change (the instructions topic).
    fn on_navigation_instruction(&self, instruction: NavigationInstruction);

    /// Receives a deviation-status stream update (the deviation topic):
    /// off-route on entry and on every recorded measurement update while the
    /// deviation persists, on-route when it clears.
    fn on_deviation_status_change(&self, deviation_status_change: DeviationStatusChange);

    /// Receives one phase transition per dispatched trip-state transition
    /// (the phase topic).
    fn on_navigation_phase_change(&self, navigation_phase_change: NavigationPhaseChange);

    /// Receives one speech-trigger line the controller's pacing policy judged
    /// newsworthy (the speech-instruction topic). Subscribers of the
    /// `subscribe_to_speech_instructions` topic receive exactly these
    /// deliveries; the composed speech text is carried verbatim and the
    /// platform owns playback.
    fn on_speech_instruction(&self, speech_instruction: SpeechInstruction);

    /// Internally required by the uniffi foreign-handle machinery (the
    /// foreign-handle probe of the uniffi 0.31 trait-object lowering);
    /// Rust-side implementations keep the default, which reports a
    /// Rust-implemented object.
    #[doc(hidden)]
    fn uniffi_foreign_handle(&self) -> Option<::uniffi::Handle> {
        Option::None
    }
}

/// Bridges a foreign implementation of the exported
/// [`NavigationListener`] callback contract onto the native
/// `rustic-maps-navigation` listener trait: an ordinary native listener for the
/// broadcaster whose callbacks lower into the foreign implementation.
pub(crate) struct ForeignNavigationStateListener(Arc<dyn NavigationListener>);

impl ForeignNavigationStateListener {
    /// Wraps the foreign listener implementation for native registration.
    pub(crate) fn new(foreign_navigation_state_listener: Arc<dyn NavigationListener>) -> Self {
        Self(foreign_navigation_state_listener)
    }
}

impl rustic_maps_navigation::NavigationStateChangeListener for ForeignNavigationStateListener {
    fn on_navigation_instruction(
        &self,
        native_instruction: &rustic_maps_navigation::NavigationInstruction,
    ) {
        self.0.on_navigation_instruction(
            NavigationInstruction::from_native_navigation_instruction(native_instruction),
        );
    }

    fn on_speech_utterance(
        &self,
        native_speech_utterance: &rustic_maps_navigation::SpeechUtterance,
    ) {
        self.0
            .on_speech_instruction(SpeechInstruction::from_native_speech_utterance(
                native_speech_utterance,
            ));
    }

    fn on_deviation_status_change(
        &self,
        native_deviation_status_change: &rustic_maps_navigation::DeviationStatusChange,
    ) {
        self.0.on_deviation_status_change(
            DeviationStatusChange::from_native_deviation_status_change(
                native_deviation_status_change,
            ),
        );
    }

    fn on_navigation_phase_change(
        &self,
        native_phase_change: &rustic_maps_navigation::NavigationPhaseChange,
    ) {
        self.0.on_navigation_phase_change(
            NavigationPhaseChange::from_native_navigation_phase_change(native_phase_change),
        );
    }
}

/// Handle coupled to one `(topic, listener)` subscription of the
/// controller's broadcaster, or to the four-topic set
/// `add_navigation_listener` registers. Cancelling is explicit and
/// idempotent; dropping the handle does not unsubscribe.
pub struct Subscription {
    /// The native handles this binding owns: exactly one for a single-topic
    /// subscription, all four topics for a composite listener subscription.
    subscriptions: Vec<TopicSubscription>,
}

impl Subscription {
    /// Wraps one native subscription handle.
    pub(crate) fn new(subscription: TopicSubscription) -> Self {
        Self {
            subscriptions: vec![subscription],
        }
    }

    /// Wraps the four native handles of a composite listener subscription.
    pub(crate) fn composite(subscriptions: Vec<TopicSubscription>) -> Self {
        Self { subscriptions }
    }

    /// Cancels every subscription this handle owns: the listener stops
    /// receiving the subscription's topic(s) from the next dispatch on.
    /// Idempotent.
    pub fn cancel(&self) {
        for subscription in &self.subscriptions {
            subscription.cancel();
        }
    }
}

/// The one active trip: the native session plus the unit system its
/// instruction payloads are composed in.
struct ActiveTrip {
    session: rustic_maps_navigation::NavigationSession,
    unit_system: UnitSystem,
}

/// The exported navigation controller: drives the native reducer's session,
/// broadcasts every state change to the registered foreign listeners, and
/// answers read-only state and instruction queries.
///
/// The active-trip lock is never held across a broadcaster dispatch (see the
/// module-head notes on the re-entrancy rules across the seam).
pub struct Navigator {
    active_trip: Mutex<Option<ActiveTrip>>,
    broadcaster: NavigationStateBroadcaster,
    /// The controller's speech-trigger policy memory: one planner for the
    /// controller's whole lifetime, so every emitted utterance id stays
    /// unique across trips and reroutes. Its band memory resets with each
    /// trip (`reset`); the composition counter deliberately does not. The
    /// lock is held only over the evaluation itself — never across a
    /// broadcaster dispatch or a listener callback.
    speech_planner: Mutex<SpeechInstructionPlanner>,
    /// The controller's reroute-recalculation hysteresis gate: the
    /// request-in-flight, cooldown and
    /// movement thresholds every reroute-request signal must pass, held as a
    /// wall-clock controller fact — never reducer state. The lock is held
    /// only over the single gate read or write, never nested with the
    /// active-trip lock and never across a broadcaster dispatch or a
    /// listener callback.
    reroute_gate: Mutex<RerouteRecalculationGate>,
}

impl Navigator {
    /// Builds the controller with no active trip, no subscriptions and the
    /// default reroute-recalculation policy.
    pub fn new() -> Self {
        Self {
            active_trip: Mutex::new(None),
            broadcaster: NavigationStateBroadcaster::new(),
            speech_planner: Mutex::new(SpeechInstructionPlanner::new()),
            reroute_gate: Mutex::new(RerouteRecalculationGate::new()),
        }
    }
}

impl Default for Navigator {
    /// Builds the empty controller, exactly like [`Navigator::new`].
    fn default() -> Self {
        Self::new()
    }
}

impl Navigator {
    /// Subscribes the listener to the instructions topic: the composed
    /// instruction payload of every dispatched state change, in registration
    /// order.
    pub fn subscribe_to_navigation_instructions(
        &self,
        listener: Arc<dyn NavigationListener>,
    ) -> Arc<Subscription> {
        self.subscribe(listener, |broadcaster, listener| {
            broadcaster.subscribe_to_navigation_instructions(listener)
        })
    }

    /// Subscribes the listener to the deviation-status topic, in registration
    /// order.
    pub fn subscribe_to_deviation_status(
        &self,
        listener: Arc<dyn NavigationListener>,
    ) -> Arc<Subscription> {
        self.subscribe(listener, |broadcaster, listener| {
            broadcaster.subscribe_to_deviation_status(listener)
        })
    }

    /// Subscribes the listener to the phase-transition topic, in registration
    /// order.
    pub fn subscribe_to_phase_changes(
        &self,
        listener: Arc<dyn NavigationListener>,
    ) -> Arc<Subscription> {
        self.subscribe(listener, |broadcaster, listener| {
            broadcaster.subscribe_to_phase_changes(listener)
        })
    }

    /// Subscribes the listener to the speech-instruction trigger topic, in
    /// registration order.
    pub fn subscribe_to_speech_instructions(
        &self,
        listener: Arc<dyn NavigationListener>,
    ) -> Arc<Subscription> {
        self.subscribe(listener, |broadcaster, listener| {
            broadcaster.subscribe_to_speech_utterances(listener)
        })
    }

    /// Subscribes one listener to all four navigation topics — instructions,
    /// deviation status, phase changes and speech instructions — and answers
    /// the composite subscription that cancels all four. The listener is
    /// wrapped once and registered on each topic list, so every topic
    /// dispatches to the same foreign object on its own method; the
    /// composite `cancel()` removes every registration (idempotent, like a
    /// single-topic subscription).
    pub fn add_navigation_listener(
        &self,
        listener: Arc<dyn NavigationListener>,
    ) -> Arc<Subscription> {
        let native_listener: Arc<dyn rustic_maps_navigation::NavigationStateChangeListener> =
            Arc::new(ForeignNavigationStateListener::new(listener));
        let subscriptions = vec![
            self.broadcaster
                .subscribe_to_navigation_instructions(Arc::clone(&native_listener)),
            self.broadcaster
                .subscribe_to_deviation_status(Arc::clone(&native_listener)),
            self.broadcaster
                .subscribe_to_phase_changes(Arc::clone(&native_listener)),
            self.broadcaster
                .subscribe_to_speech_utterances(native_listener),
        ];
        Arc::new(Subscription::composite(subscriptions))
    }

    /// Evaluates the active trip's speech-trigger policy over one dispatch's
    /// state transition and delivers the utterances to the speech topic. A
    /// suppressed state-change dispatch skips the evaluation — the policy
    /// memory keeps its state, so the next delivered dispatch sees the
    /// transition whole. The planner lock is held only over the evaluation
    /// itself, never across a dispatch or listener callback.
    fn dispatch_speech_policies(
        &self,
        previous_state: &rustic_maps_navigation::TripNavigationState,
        current_state: &rustic_maps_navigation::TripNavigationState,
        unit_system: UnitSystem,
        dispatch_outcome: NavigationDispatchOutcome,
    ) {
        if dispatch_outcome != NavigationDispatchOutcome::Delivered {
            return;
        }
        let mut speech_planner = crate::panic_boundary::state_lock_guard(
            &self.speech_planner,
            "the navigation controller's speech planner",
        );
        let utterances = speech_planner.evaluate_state_transition(
            Some(previous_state),
            current_state,
            unit_system,
            Instant::now(),
        );
        drop(speech_planner);
        for utterance in &utterances {
            self.broadcaster.dispatch_speech_utterance(utterance);
        }
    }

    /// Evaluates the reroute-recalculation gates over one reroute-request
    /// signal at the wall clock: `true` when every gate passed and a
    /// calculation is now in flight (readable as the trip state's
    /// `is_calculating_new_route`); `false` when the signal was refused
    /// (a calculation in flight, inside the cooldown window, or inside the
    /// movement threshold — check the trip state for which fact applies).
    /// A signal without an open, still-navigable trip (no trip, or the trip
    /// has arrived) is refused before the gates are consulted.
    ///
    /// The trip's eligibility is read under a short active-trip lock
    /// released before the gate lock; neither is held across a dispatch or a
    /// listener callback. A passing caller owes a settle: the eventual
    /// `apply_reroute` (applied or ignored) or
    /// [`Navigator::cancel_reroute_calculation`].
    pub fn begin_reroute_calculation(&self) -> bool {
        let trip_is_navigable = {
            let active_trip = crate::panic_boundary::state_lock_guard(
                &self.active_trip,
                "the navigation controller's active trip",
            );
            active_trip.as_ref().is_some_and(|trip| {
                trip.session.state.phase != rustic_maps_navigation::NavigationPhase::Arrived
            })
        };
        if !trip_is_navigable {
            return false;
        }
        crate::panic_boundary::state_lock_guard(
            &self.reroute_gate,
            "the navigation controller's reroute gate",
        )
        .begin_calculation(Instant::now())
    }

    /// Settles an in-flight reroute calculation without applying a route
    /// (the integrator abandoned the request): the request-in-flight gate
    /// reopens. The cooldown keeps running from the calculation's beginning
    /// and the movement reference stays where it was. Idempotent; without an
    /// active trip the gate is still settled (the gate is controller
    /// configuration and memory, not trip state).
    pub fn cancel_reroute_calculation(&self) {
        crate::panic_boundary::state_lock_guard(
            &self.reroute_gate,
            "the navigation controller's reroute gate",
        )
        .settle_calculation();
    }

    /// Swaps the reroute-recalculation hysteresis policy in force, keeping
    /// the gate's memory as it stands. Both thresholds must be finite and
    /// non-negative (`InvalidNavigationInput` otherwise, and the policy in
    /// force is then untouched).
    pub fn configure_reroute_policy(&self, reroute_policy: ReroutePolicy) -> Result<(), SdkError> {
        validated_policy_threshold(
            reroute_policy.recalculation_cooldown_seconds,
            "reroute recalculation cooldown seconds",
        )?;
        validated_policy_threshold(
            reroute_policy.recalculation_movement_meters,
            "reroute recalculation movement threshold meters",
        )?;
        crate::panic_boundary::state_lock_guard(
            &self.reroute_gate,
            "the navigation controller's reroute gate",
        )
        .set_policy(rustic_maps_navigation::ReroutePolicy {
            recalculation_cooldown_seconds: reroute_policy.recalculation_cooldown_seconds,
            recalculation_movement_meters: reroute_policy.recalculation_movement_meters,
        });
        Ok(())
    }

    /// Registers the wrapped foreign listener on one topic list.
    fn subscribe(
        &self,
        listener: Arc<dyn NavigationListener>,
        topic_registration: impl FnOnce(
            &NavigationStateBroadcaster,
            Arc<dyn rustic_maps_navigation::NavigationStateChangeListener>,
        ) -> TopicSubscription,
    ) -> Arc<Subscription> {
        let native_listener: Arc<dyn rustic_maps_navigation::NavigationStateChangeListener> =
            Arc::new(ForeignNavigationStateListener::new(listener));
        Arc::new(Subscription::new(topic_registration(
            &self.broadcaster,
            native_listener,
        )))
    }

    /// Begins navigating `route` with the given policies, replacing any
    /// active trip. The initial instruction payload is dispatched; no
    /// deviation or phase transitions exist yet.
    ///
    /// A route without traversable steps represents an already-arrived trip
    /// (the reducer's own semantics), not a rejection.
    pub fn start_navigation(
        &self,
        route: Route,
        unit_system: crate::records::UnitSystem,
        step_advance_policy: StepAdvancePolicy,
        deviation_policy: DeviationPolicy,
    ) -> Result<(), SdkError> {
        let step_advance_condition = build_step_advance_condition(&step_advance_policy)?;
        let deviation_detector = build_deviation_detector(&deviation_policy)?;
        let native_route = route.into_native_route_data()?;
        let native_unit_system = unit_system.to_native_unit_system();
        let session = crate::panic_boundary::required_invariant(
            reduce_navigation_event(
                // A fresh trip replaces any active session outright (the
                // reducer reads no previous session for a StartTrip event).
                None,
                rustic_maps_navigation::NavigationEvent::StartTrip {
                    route: native_route,
                    step_advance_condition,
                    deviation_detector,
                    // The arrival-aware advance condition (Ferrostar's B5
                    // semantics) is not yet an FFI-configurable policy: the
                    // frozen UDL contract carries no parameter for it, and
                    // `None` keeps the trip's ordinary condition end-to-end.
                    arrival_step_advance_condition: None,
                    // Raw course reporting preserves today's behavior; the
                    // SnapToRoute policy (Ferrostar's B7) is internal-only
                    // until its FFI surface is an agreed addition.
                    course_filtering: rustic_maps_navigation::CourseFiltering::Raw,
                },
            ),
            "a StartTrip event always produces a session",
        );
        let dispatched_state = session.state.clone();
        *crate::panic_boundary::state_lock_guard(
            &self.active_trip,
            "the navigation controller's active trip",
        ) = Some(ActiveTrip {
            session,
            unit_system: native_unit_system,
        });
        // The speech policy restarts with the trip: nothing is announced at
        // trip start (the initial instruction payload is the dispatched
        // instructions topic's own line, and the platforms speak it). The
        // planner's band memory resets, while its utterance counter survives
        // so ids are never reused across trips. The reroute gate forgets its
        // trip memory (nothing in flight, no cooldown, no fix reference)
        // while the configured policy persists.
        crate::panic_boundary::state_lock_guard(
            &self.speech_planner,
            "the navigation controller's speech planner",
        )
        .reset();
        crate::panic_boundary::state_lock_guard(
            &self.reroute_gate,
            "the navigation controller's reroute gate",
        )
        .reset();
        self.broadcaster
            .dispatch_trip_started(&dispatched_state, native_unit_system);
        Ok(())
    }

    /// Ends the active trip and clears the trip state: `current_trip_state`
    /// and `current_instructions` answer `None` afterwards, and location
    /// updates, reroutes, reroute-request signals and speech evaluations are
    /// dropped until `start_navigation` begins a new trip. The speech-trigger
    /// band memory and the reroute-recalculation gate are reset with the
    /// trip — the utterance identifier counter is deliberately not reset, so
    /// identifiers stay monotonic across a stop, and the configured reroute
    /// policy persists. Listener subscriptions are controller configuration,
    /// not trip state: they are untouched and serve the next trip. Stopping
    /// without an active trip is a no-op.
    ///
    /// No event is dispatched for the stop: the native reducer has no stop
    /// transition, and the phase channel has no stop variant, so
    /// inventing one here would be a semantics the reducer does not own.
    pub fn stop_navigation(&self) {
        // The three locks are taken sequentially and never nested: the trip
        // is cleared first, then the two pieces of controller memory are
        // reset.
        *crate::panic_boundary::state_lock_guard(
            &self.active_trip,
            "the navigation controller's active trip",
        ) = None;
        crate::panic_boundary::state_lock_guard(
            &self.speech_planner,
            "the navigation controller's speech planner",
        )
        .reset();
        crate::panic_boundary::state_lock_guard(
            &self.reroute_gate,
            "the navigation controller's reroute gate",
        )
        .reset();
    }

    /// Delivers one position fix to the active trip; without one the fix is
    /// dropped (the reducer's own semantics for event-before-trip).
    /// The fix is judged for deviation, snapped and evaluated by the active
    /// step-advance condition exactly as the native reducer documents, and
    /// the resulting transition is broadcast. A broadcast suppressed by the
    /// broadcaster's re-entrancy guard is not reported here; the state change
    /// behind it has already been applied.
    pub fn update_location(&self, location_update: NavigationLocationUpdate) {
        let native_update = location_update.into_native_location_update();
        // The reroute movement gate measures raw fixes, so every consumed fix
        // feeds it (its own short lock, taken and released before the
        // active-trip lock — the two are never held together).
        crate::panic_boundary::state_lock_guard(
            &self.reroute_gate,
            "the navigation controller's reroute gate",
        )
        .record_fix(native_update.coordinate);
        let (previous_state, current_state, unit_system) = {
            // The lock guards only the session swap; it is released before the
            // dispatch below, so a listener may call back into the controller.
            let mut active_trip = crate::panic_boundary::state_lock_guard(
                &self.active_trip,
                "the navigation controller's active trip",
            );
            let Some(trip) = active_trip.as_mut() else {
                return;
            };
            let previous_state = trip.session.state.clone();
            let session = crate::panic_boundary::required_invariant(
                reduce_navigation_event(
                    Some(trip.session.clone()),
                    rustic_maps_navigation::NavigationEvent::LocationUpdated(native_update),
                ),
                "an active trip's location update always produces a session",
            );
            trip.session = session;
            (previous_state, trip.session.state.clone(), trip.unit_system)
        };
        let dispatch_outcome =
            self.broadcaster
                .dispatch_location_update(&previous_state, &current_state, unit_system);
        self.dispatch_speech_policies(
            &previous_state,
            &current_state,
            unit_system,
            dispatch_outcome,
        );
    }

    /// Replaces the route of the active trip (a reroute). The replacement
    /// route must be anchored at the trip's current snapped coordinate (the
    /// reducer's anchoring contract; no trimming happens here). The step
    /// advance strategy for the new route is configured by
    /// `step_advance_policy`; the active deviation policy persists.
    ///
    /// A reroute offered to an arrived trip is ignored by the reducer and
    /// produces no phase transition; without an active trip the event is
    /// dropped.
    pub fn apply_reroute(
        &self,
        route: Route,
        step_advance_policy: StepAdvancePolicy,
    ) -> Result<(), SdkError> {
        let step_advance_condition = build_step_advance_condition(&step_advance_policy)?;
        let native_route = route.into_native_route_data()?;
        let (previous_state, current_state, unit_system, reroute_applied) = {
            let mut active_trip = crate::panic_boundary::state_lock_guard(
                &self.active_trip,
                "the navigation controller's active trip",
            );
            let Some(trip) = active_trip.as_mut() else {
                return Ok(());
            };
            let previous_state = trip.session.state.clone();
            let session = crate::panic_boundary::required_invariant(
                reduce_navigation_event(
                    Some(trip.session.clone()),
                    rustic_maps_navigation::NavigationEvent::RerouteProvided {
                        route: native_route,
                        step_advance_condition,
                    },
                ),
                "an active trip's reroute always produces a session",
            );
            let reroute_applied = previous_state != session.state;
            trip.session = session;
            (
                previous_state,
                trip.session.state.clone(),
                trip.unit_system,
                reroute_applied,
            )
        };
        let _ =
            self.broadcaster
                .dispatch_reroute_applied(&previous_state, &current_state, unit_system);
        // The in-flight calculation is settled whichever way the reducer
        // disposed of the route (applied or ignored): the integrator acted
        // on its request, so the request-in-flight gate reopens. The
        // cooldown and movement memory stay anchored at the calculation's
        // beginning.
        crate::panic_boundary::state_lock_guard(
            &self.reroute_gate,
            "the navigation controller's reroute gate",
        )
        .settle_calculation();
        if reroute_applied {
            // The route (and its step ladder) is new: the speech band memory
            // restarts, while the utterance counter survives so ids are
            // never reused.
            crate::panic_boundary::state_lock_guard(
                &self.speech_planner,
                "the navigation controller's speech planner",
            )
            .reset();
        }
        Ok(())
    }

    /// The plain state of the active trip, or `None` without one. The
    /// `is_calculating_new_route` fact is the controller's own reroute gate
    /// readback (a wall-clock controller fact the pure trip state cannot
    /// carry); the two reads are short, sequential and never nested.
    pub fn current_trip_state(&self) -> Option<crate::records::TripNavigationState> {
        let is_calculating_new_route = crate::panic_boundary::state_lock_guard(
            &self.reroute_gate,
            "the navigation controller's reroute gate",
        )
        .is_calculating_new_route();
        let mut trip_state = crate::panic_boundary::state_lock_guard(
            &self.active_trip,
            "the navigation controller's active trip",
        )
        .as_ref()
        .map(|trip| {
            crate::records::TripNavigationState::from_native_trip_navigation_state(
                &trip.session.state,
            )
        })?;
        trip_state.is_calculating_new_route = is_calculating_new_route;
        Some(trip_state)
    }

    /// The composed turn-by-turn instruction payload of the current trip
    /// state, or `None` without an active trip. Emission is pure in the
    /// native core, so repeated reads of an unchanged state return the same
    /// payload.
    pub fn current_instructions(&self) -> Option<NavigationInstruction> {
        let active_trip = crate::panic_boundary::state_lock_guard(
            &self.active_trip,
            "the navigation controller's active trip",
        );
        let trip = active_trip.as_ref()?;
        Some(NavigationInstruction::from_native_navigation_instruction(
            &compose_navigation_instructions(&trip.session.state, trip.unit_system),
        ))
    }
}

/// Builds the native step-advance condition behind a policy record, refusing
/// non-finite or negative threshold values.
pub(crate) fn build_step_advance_condition(
    step_advance_policy: &StepAdvancePolicy,
) -> Result<Arc<dyn StepAdvanceCondition>, SdkError> {
    match step_advance_policy.policy_kind {
        StepAdvancePolicyKind::Manual => Ok(Arc::new(ManualStepAdvanceCondition)),
        StepAdvancePolicyKind::DistanceToEndOfStep => Ok(Arc::new(DistanceToEndOfStepCondition {
            advance_threshold_meters: validated_policy_threshold(
                step_advance_policy.advance_threshold_meters,
                "step-advance distance threshold",
            )?,
            minimum_horizontal_accuracy_meters: validated_policy_threshold(
                step_advance_policy.minimum_horizontal_accuracy_meters,
                "step-advance minimum horizontal accuracy",
            )?,
        })),
        StepAdvancePolicyKind::DistanceFromStep => Ok(Arc::new(DistanceFromStepCondition {
            advance_threshold_meters: validated_policy_threshold(
                step_advance_policy.advance_threshold_meters,
                "step-advance distance threshold",
            )?,
            minimum_horizontal_accuracy_meters: validated_policy_threshold(
                step_advance_policy.minimum_horizontal_accuracy_meters,
                "step-advance minimum horizontal accuracy",
            )?,
        })),
    }
}

/// Builds the native deviation detector behind a policy record, refusing a
/// non-finite or negative threshold.
pub(crate) fn build_deviation_detector(
    deviation_policy: &DeviationPolicy,
) -> Result<Arc<dyn DeviationDetector>, SdkError> {
    match deviation_policy.policy_kind {
        DeviationPolicyKind::DistanceThreshold => Ok(Arc::new(DistanceDeviationDetector {
            deviation_threshold_meters: validated_policy_threshold(
                deviation_policy.deviation_threshold_meters,
                "deviation distance threshold",
            )?,
        })),
    }
}

/// Validates one policy threshold: policies are configured by foreign code,
/// where a NaN or negative threshold would silently disable or permanently
/// flag the strategy, so the controller boundary refuses them instead.
fn validated_policy_threshold(
    policy_threshold_meters: f64,
    threshold_description: &str,
) -> Result<f64, SdkError> {
    if !policy_threshold_meters.is_finite() || policy_threshold_meters.is_sign_negative() {
        return Err(SdkError::InvalidNavigationInput {
            detail: format!("{threshold_description} must be finite and non-negative"),
        });
    }
    Ok(policy_threshold_meters)
}

/// The SDK default step-advance policy: the distance-to-end-of-step condition
/// with the recommended driving defaults.
pub fn default_step_advance_policy() -> StepAdvancePolicy {
    StepAdvancePolicy {
        policy_kind: StepAdvancePolicyKind::DistanceToEndOfStep,
        advance_threshold_meters:
            rustic_maps_navigation::DEFAULT_DISTANCE_TO_END_OF_STEP_THRESHOLD_METERS,
        minimum_horizontal_accuracy_meters:
            rustic_maps_navigation::DEFAULT_MINIMUM_HORIZONTAL_ACCURACY_METERS,
    }
}

/// The SDK default deviation policy: the distance-threshold detector with the
/// native default deviation threshold.
pub fn default_deviation_policy() -> DeviationPolicy {
    DeviationPolicy {
        policy_kind: DeviationPolicyKind::DistanceThreshold,
        deviation_threshold_meters:
            rustic_maps_navigation::DEFAULT_DEVIATION_DISTANCE_THRESHOLD_METERS,
    }
}

/// The SDK default reroute-recalculation policy: the recommended hysteresis
/// (5 s cooldown, 50 m movement threshold).
pub fn default_reroute_policy() -> ReroutePolicy {
    ReroutePolicy {
        recalculation_cooldown_seconds:
            rustic_maps_navigation::DEFAULT_REROUTE_RECALCULATION_COOLDOWN_SECONDS,
        recalculation_movement_meters:
            rustic_maps_navigation::DEFAULT_REROUTE_RECALCULATION_MOVEMENT_METERS,
    }
}
