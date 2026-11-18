//! The reference-application flow vocabulary and reducer.
//!
//! The reducer is a total pure function:
//!
//! ```
//! use map_refapp_core::{FlowEvent, FlowState, advance_flow};
//! let transition = advance_flow(FlowState::HomePeek, FlowEvent::StartTrip);
//! assert_eq!(transition.state, FlowState::Navigating);
//! ```
//!
//! It answers one question: given the flow state and an event, what should the
//! user see next? It does not own the SDK handle, the route record, the camera
//! pose, the credential store, the speech engine or the view lifecycle. Every
//! event from every state returns either a new state or an explicit unchanged
//! transition, whose [`FlowTransition::unchanged`] flag is `true`.
//!
//! Settings and diagnostics are overlays, not flow positions. Their events
//! leave the flow state untouched and report the overlay outcome, because
//! opening either surface never moves the visitor within the plan or
//! navigation flow. [`FlowState::ProviderSettings`] and
//! [`FlowState::Diagnostics`] therefore stay in the vocabulary for a
//! platform's own overlay model, but [`advance_flow`] never returns them.

/// One reference-application flow position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FlowState {
    /// The home sheet's peek.
    HomePeek,
    /// The two-field search view.
    PlanSearch,
    /// A plan request is in flight.
    Planning,
    /// A route is present and can start.
    RoutePlanned,
    /// A simulated or device-driven trip is running.
    Navigating,
    /// The trip reached its end.
    Arrived,
    /// The provider-settings overlay vocabulary; the reducer keeps the flow
    /// position beneath the overlay and never returns this state.
    ProviderSettings,
    /// The diagnostics overlay vocabulary; the reducer keeps the flow
    /// position beneath the overlay and never returns this state.
    Diagnostics,
}

impl FlowState {
    /// Every flow state, in the design note's table order.
    pub const ALL: [FlowState; 8] = [
        FlowState::HomePeek,
        FlowState::PlanSearch,
        FlowState::Planning,
        FlowState::RoutePlanned,
        FlowState::Navigating,
        FlowState::Arrived,
        FlowState::ProviderSettings,
        FlowState::Diagnostics,
    ];

    /// The stable schema identifier.
    pub const fn id(self) -> &'static str {
        match self {
            FlowState::HomePeek => "home_peek",
            FlowState::PlanSearch => "plan_search",
            FlowState::Planning => "planning",
            FlowState::RoutePlanned => "route_planned",
            FlowState::Navigating => "navigating",
            FlowState::Arrived => "arrived",
            FlowState::ProviderSettings => "provider_settings",
            FlowState::Diagnostics => "diagnostics",
        }
    }
}

/// The plan field an event targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FlowField {
    /// The origin field.
    Origin,
    /// The destination field.
    Destination,
}

impl FlowField {
    /// Both fields, origin first, the order every route planner presents.
    pub const ALL: [FlowField; 2] = [FlowField::Origin, FlowField::Destination];

    /// The stable schema identifier.
    pub const fn id(self) -> &'static str {
        match self {
            FlowField::Origin => "origin",
            FlowField::Destination => "destination",
        }
    }
}

/// One input to the flow reducer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FlowEvent {
    /// Open the search view, armed for one field.
    OpenSearch(FlowField),
    /// Start a trip from the home peek or a planned route.
    StartTrip,
    /// A plan field's text changed.
    FieldTextChanged(FlowField),
    /// The visitor requested a search for one field.
    SearchRequested(FlowField),
    /// A search result was assigned to one field.
    ResultAssigned(FlowField),
    /// The visitor asked my location to fill one field.
    MyLocationRequested(FlowField),
    /// The visitor asked to exchange the two fields.
    SwapRequested,
    /// The visitor pressed the explicit Plan route action.
    PlanRequested,
    /// The visitor dismissed the search view.
    Done,
    /// The planner returned a route.
    PlanSucceeded,
    /// The planner reported a failure.
    PlanFailed,
    /// The visitor returned from the route card to the search view.
    EditRoute,
    /// One location fix of the active trip.
    Fix,
    /// The visitor chose a playback speed.
    SpeedChanged,
    /// The visitor paused the trip.
    Pause,
    /// The visitor resumed the trip.
    Resume,
    /// The visitor ended the trip.
    Exit,
    /// The contract reported arrival.
    Arrived,
    /// The visitor opened the provider-settings overlay.
    OpenSettings,
    /// The visitor selected a provider for a capability.
    ProviderSelected,
    /// The visitor applied a provider credential.
    CredentialApplied,
    /// The visitor opened the diagnostics overlay.
    OpenDiagnostics,
    /// The visitor toggled debug detail in the diagnostics overlay.
    DebugDetailToggled,
    /// The visitor cleared the diagnostics display list.
    Clear,
    /// The visitor closed the active overlay.
    Close,
}

impl FlowEvent {
    /// Every event, in the design note's table order.
    pub const ALL: [FlowEvent; 25] = [
        FlowEvent::OpenSearch(FlowField::Origin),
        FlowEvent::StartTrip,
        FlowEvent::FieldTextChanged(FlowField::Origin),
        FlowEvent::SearchRequested(FlowField::Origin),
        FlowEvent::ResultAssigned(FlowField::Origin),
        FlowEvent::MyLocationRequested(FlowField::Origin),
        FlowEvent::SwapRequested,
        FlowEvent::PlanRequested,
        FlowEvent::Done,
        FlowEvent::PlanSucceeded,
        FlowEvent::PlanFailed,
        FlowEvent::EditRoute,
        FlowEvent::Fix,
        FlowEvent::SpeedChanged,
        FlowEvent::Pause,
        FlowEvent::Resume,
        FlowEvent::Exit,
        FlowEvent::Arrived,
        FlowEvent::OpenSettings,
        FlowEvent::ProviderSelected,
        FlowEvent::CredentialApplied,
        FlowEvent::OpenDiagnostics,
        FlowEvent::DebugDetailToggled,
        FlowEvent::Clear,
        FlowEvent::Close,
    ];

    /// The stable event identifier.
    pub const fn id(self) -> &'static str {
        match self {
            FlowEvent::OpenSearch(FlowField::Origin) => "open_search_origin",
            FlowEvent::OpenSearch(FlowField::Destination) => "open_search_destination",
            FlowEvent::StartTrip => "start_trip",
            FlowEvent::FieldTextChanged(FlowField::Origin) => "field_text_changed_origin",
            FlowEvent::FieldTextChanged(FlowField::Destination) => "field_text_changed_destination",
            FlowEvent::SearchRequested(FlowField::Origin) => "search_requested_origin",
            FlowEvent::SearchRequested(FlowField::Destination) => "search_requested_destination",
            FlowEvent::ResultAssigned(FlowField::Origin) => "result_assigned_origin",
            FlowEvent::ResultAssigned(FlowField::Destination) => "result_assigned_destination",
            FlowEvent::MyLocationRequested(FlowField::Origin) => "my_location_requested_origin",
            FlowEvent::MyLocationRequested(FlowField::Destination) => {
                "my_location_requested_destination"
            }
            FlowEvent::SwapRequested => "swap_requested",
            FlowEvent::PlanRequested => "plan_requested",
            FlowEvent::Done => "done",
            FlowEvent::PlanSucceeded => "plan_succeeded",
            FlowEvent::PlanFailed => "plan_failed",
            FlowEvent::EditRoute => "edit_route",
            FlowEvent::Fix => "fix",
            FlowEvent::SpeedChanged => "speed_changed",
            FlowEvent::Pause => "pause",
            FlowEvent::Resume => "resume",
            FlowEvent::Exit => "exit",
            FlowEvent::Arrived => "arrived",
            FlowEvent::OpenSettings => "open_settings",
            FlowEvent::ProviderSelected => "provider_selected",
            FlowEvent::CredentialApplied => "credential_applied",
            FlowEvent::OpenDiagnostics => "open_diagnostics",
            FlowEvent::DebugDetailToggled => "debug_detail_toggled",
            FlowEvent::Clear => "clear",
            FlowEvent::Close => "close",
        }
    }
}

/// What the user should see after one event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FlowOutcome {
    /// The event changed nothing the flow can observe.
    Unchanged,
    /// The search view opened for one field.
    OpenSearch(FlowField),
    /// A trip started.
    StartTrip,
    /// A plan field's text changed.
    FieldTextChanged(FlowField),
    /// A field's search was requested.
    SearchRequested(FlowField),
    /// A result was assigned to one field.
    ResultAssigned(FlowField),
    /// My location was asked to fill one field.
    MyLocationRequested(FlowField),
    /// The two fields were exchanged.
    Swap,
    /// The explicit plan action was accepted.
    PlanRequested,
    /// The planner returned a route.
    PlanSucceeded,
    /// The planner reported a failure.
    PlanFailed,
    /// The search view was dismissed.
    Done,
    /// The route card returned to the search view.
    EditRoute,
    /// One location fix was applied.
    Fix,
    /// The playback speed changed.
    SpeedChanged,
    /// The trip paused.
    Pause,
    /// The trip resumed.
    Resume,
    /// The trip ended.
    Exit,
    /// The trip arrived.
    Arrived,
    /// The settings overlay opened.
    OpenSettings,
    /// A provider was selected.
    ProviderSelected,
    /// A provider credential was applied.
    CredentialApplied,
    /// The diagnostics overlay opened.
    OpenDiagnostics,
    /// The debug-detail switch changed.
    DebugDetailToggled,
    /// The diagnostics display list was cleared.
    Clear,
    /// The active overlay closed.
    Close,
}

impl FlowOutcome {
    /// The stable schema step identifier this outcome matches.
    pub const fn id(self) -> &'static str {
        match self {
            FlowOutcome::Unchanged => "unchanged",
            FlowOutcome::OpenSearch(FlowField::Origin) => "search_origin",
            FlowOutcome::OpenSearch(FlowField::Destination) => "search_destination",
            FlowOutcome::StartTrip => "drive",
            FlowOutcome::FieldTextChanged(FlowField::Origin) => "field_text_changed_origin",
            FlowOutcome::FieldTextChanged(FlowField::Destination) => {
                "field_text_changed_destination"
            }
            FlowOutcome::SearchRequested(FlowField::Origin) => "search_origin",
            FlowOutcome::SearchRequested(FlowField::Destination) => "search_destination",
            FlowOutcome::ResultAssigned(FlowField::Origin) => "assign_origin",
            FlowOutcome::ResultAssigned(FlowField::Destination) => "assign_destination",
            FlowOutcome::MyLocationRequested(FlowField::Origin) => "my_location_origin",
            FlowOutcome::MyLocationRequested(FlowField::Destination) => "my_location_destination",
            FlowOutcome::Swap => "swap",
            FlowOutcome::PlanRequested => "plan",
            FlowOutcome::PlanSucceeded => "plan_succeeded",
            FlowOutcome::PlanFailed => "plan_failed",
            FlowOutcome::Done => "done",
            FlowOutcome::EditRoute => "edit",
            FlowOutcome::Fix => "fix",
            FlowOutcome::SpeedChanged => "speed_changed",
            FlowOutcome::Pause => "pause",
            FlowOutcome::Resume => "resume",
            FlowOutcome::Exit => "exit",
            FlowOutcome::Arrived => "arrived",
            FlowOutcome::OpenSettings => "open_settings",
            FlowOutcome::ProviderSelected => "select_provider",
            FlowOutcome::CredentialApplied => "apply_credential",
            FlowOutcome::OpenDiagnostics => "open_diagnostics",
            FlowOutcome::DebugDetailToggled => "toggle_debug_detail",
            FlowOutcome::Clear => "clear",
            FlowOutcome::Close => "close",
        }
    }
}

/// The reducer's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowTransition {
    /// The flow state after the event.
    pub state: FlowState,
    /// What the user should see.
    pub outcome: FlowOutcome,
    /// The copy IDs the platform must render for this transition.
    pub copy_ids: &'static [&'static str],
    /// Whether the event left the flow state unchanged.
    pub unchanged: bool,
}

impl FlowTransition {
    /// Whether the event left the flow state unchanged.
    pub const fn is_unchanged(&self) -> bool {
        self.unchanged
    }
}

const COPY_NONE: &[&str] = &[];
const COPY_OPEN_SEARCH_ORIGIN: &[&str] = &[
    "plan.search_heading",
    "plan.origin_label",
    "plan.origin_hint",
];
const COPY_OPEN_SEARCH_DESTINATION: &[&str] = &[
    "plan.search_heading",
    "plan.destination_label",
    "plan.destination_hint",
];
const COPY_SEARCHING: &[&str] = &["plan.searching_in_flight"];
const COPY_PLAN_READY: &[&str] = &["plan.plan_route_action"];
const COPY_MY_LOCATION: &[&str] = &["plan.my_location_action"];
const COPY_SWAP: &[&str] = &["plan.swap_action"];
const COPY_PLANNING: &[&str] = &["plan.planning_in_flight"];
const COPY_ROUTE_PLANNED: &[&str] = &["route.start_action", "route.edit_action"];
const COPY_PLAN_FAILED: &[&str] = &["plan.plan_route_action"];
const COPY_PEEK: &[&str] = &["peek.primary_action", "plan.destination_hint"];
const COPY_EDIT: &[&str] = &["plan.origin_label", "plan.destination_label"];
const COPY_DRIVE: &[&str] = &["navigation.phase_navigating", "navigation.eta_prefix"];
const COPY_FIX: &[&str] = &["navigation.eta_prefix"];
const COPY_SPEED_CHANGED: &[&str] = &["navigation.speed_multiplier_label"];
const COPY_AFTER_PAUSE: &[&str] = &["navigation.resume_action"];
const COPY_AFTER_RESUME: &[&str] = &["navigation.pause_action"];
const COPY_ARRIVED: &[&str] = &["navigation.phase_arrived"];
const COPY_EXIT: &[&str] = &["peek.primary_action", "plan.destination_hint"];
const COPY_PROVIDER_ROW: &[&str] = &["settings.provider_row"];
const COPY_CREDENTIAL_APPLIED: &[&str] = &["settings.credential_applied"];
const COPY_DIAGNOSTICS_TITLE: &[&str] = &["diagnostics.title"];
const COPY_DEBUG_TOGGLE: &[&str] = &[
    "diagnostics.show_debug_events",
    "diagnostics.hide_debug_events",
];
const COPY_CLEAR: &[&str] = &["diagnostics.clear_action"];
const COPY_CLOSE: &[&str] = &["settings.close_action"];

const fn changed(
    state: FlowState,
    outcome: FlowOutcome,
    copy_ids: &'static [&'static str],
) -> FlowTransition {
    FlowTransition {
        state,
        outcome,
        copy_ids,
        unchanged: false,
    }
}

const fn unchanged_with(
    state: FlowState,
    outcome: FlowOutcome,
    copy_ids: &'static [&'static str],
) -> FlowTransition {
    FlowTransition {
        state,
        outcome,
        copy_ids,
        unchanged: true,
    }
}

const fn unchanged(state: FlowState) -> FlowTransition {
    unchanged_with(state, FlowOutcome::Unchanged, COPY_NONE)
}

/// The settings and diagnostics overlays never move the flow position.
fn overlay_transition(state: FlowState, event: FlowEvent) -> Option<FlowTransition> {
    let transition = match event {
        FlowEvent::OpenSettings => unchanged_with(state, FlowOutcome::OpenSettings, COPY_NONE),
        FlowEvent::ProviderSelected => {
            unchanged_with(state, FlowOutcome::ProviderSelected, COPY_PROVIDER_ROW)
        }
        FlowEvent::CredentialApplied => unchanged_with(
            state,
            FlowOutcome::CredentialApplied,
            COPY_CREDENTIAL_APPLIED,
        ),
        FlowEvent::OpenDiagnostics => {
            unchanged_with(state, FlowOutcome::OpenDiagnostics, COPY_DIAGNOSTICS_TITLE)
        }
        FlowEvent::DebugDetailToggled => {
            unchanged_with(state, FlowOutcome::DebugDetailToggled, COPY_DEBUG_TOGGLE)
        }
        FlowEvent::Clear => unchanged_with(state, FlowOutcome::Clear, COPY_CLEAR),
        FlowEvent::Close => unchanged_with(state, FlowOutcome::Close, COPY_CLOSE),
        _ => return None,
    };
    Some(transition)
}

/// The total flow reducer.
///
/// Assignment never plans: only [`FlowEvent::PlanRequested`] enters
/// [`FlowState::Planning`]. One trip runs at a time: [`FlowEvent::StartTrip`]
/// moves to [`FlowState::Navigating`] from the home peek or the route card.
/// Diagnostics and provider settings are overlays, so their events return an
/// unchanged transition at the current flow position.
pub fn advance_flow(state: FlowState, event: FlowEvent) -> FlowTransition {
    if let Some(overlay) = overlay_transition(state, event) {
        return overlay;
    }
    match state {
        FlowState::HomePeek => match event {
            FlowEvent::OpenSearch(field) => changed(
                FlowState::PlanSearch,
                FlowOutcome::OpenSearch(field),
                open_search_copy(field),
            ),
            FlowEvent::StartTrip => {
                changed(FlowState::Navigating, FlowOutcome::StartTrip, COPY_DRIVE)
            }
            _ => unchanged(state),
        },
        FlowState::PlanSearch => match event {
            FlowEvent::FieldTextChanged(field) => {
                unchanged_with(state, FlowOutcome::FieldTextChanged(field), COPY_NONE)
            }
            FlowEvent::SearchRequested(field) => {
                unchanged_with(state, FlowOutcome::SearchRequested(field), COPY_SEARCHING)
            }
            FlowEvent::ResultAssigned(field) => {
                unchanged_with(state, FlowOutcome::ResultAssigned(field), COPY_PLAN_READY)
            }
            FlowEvent::MyLocationRequested(field) => unchanged_with(
                state,
                FlowOutcome::MyLocationRequested(field),
                COPY_MY_LOCATION,
            ),
            FlowEvent::SwapRequested => unchanged_with(state, FlowOutcome::Swap, COPY_SWAP),
            FlowEvent::PlanRequested => changed(
                FlowState::Planning,
                FlowOutcome::PlanRequested,
                COPY_PLANNING,
            ),
            FlowEvent::Done => changed(FlowState::HomePeek, FlowOutcome::Done, COPY_PEEK),
            _ => unchanged(state),
        },
        FlowState::Planning => match event {
            FlowEvent::PlanSucceeded => changed(
                FlowState::RoutePlanned,
                FlowOutcome::PlanSucceeded,
                COPY_ROUTE_PLANNED,
            ),
            FlowEvent::PlanFailed => changed(
                FlowState::PlanSearch,
                FlowOutcome::PlanFailed,
                COPY_PLAN_FAILED,
            ),
            _ => unchanged(state),
        },
        FlowState::RoutePlanned => match event {
            FlowEvent::StartTrip => {
                changed(FlowState::Navigating, FlowOutcome::StartTrip, COPY_DRIVE)
            }
            FlowEvent::EditRoute => {
                changed(FlowState::PlanSearch, FlowOutcome::EditRoute, COPY_EDIT)
            }
            FlowEvent::PlanRequested => changed(
                FlowState::Planning,
                FlowOutcome::PlanRequested,
                COPY_PLANNING,
            ),
            _ => unchanged(state),
        },
        FlowState::Navigating => match event {
            FlowEvent::Fix => unchanged_with(state, FlowOutcome::Fix, COPY_FIX),
            FlowEvent::SpeedChanged => {
                unchanged_with(state, FlowOutcome::SpeedChanged, COPY_SPEED_CHANGED)
            }
            FlowEvent::Pause => unchanged_with(state, FlowOutcome::Pause, COPY_AFTER_PAUSE),
            FlowEvent::Resume => unchanged_with(state, FlowOutcome::Resume, COPY_AFTER_RESUME),
            FlowEvent::Exit => changed(FlowState::HomePeek, FlowOutcome::Exit, COPY_EXIT),
            FlowEvent::Arrived => changed(FlowState::Arrived, FlowOutcome::Arrived, COPY_ARRIVED),
            _ => unchanged(state),
        },
        FlowState::Arrived => match event {
            FlowEvent::Exit => changed(FlowState::HomePeek, FlowOutcome::Exit, COPY_EXIT),
            _ => unchanged(state),
        },
        FlowState::ProviderSettings | FlowState::Diagnostics => unchanged(state),
    }
}

const fn open_search_copy(field: FlowField) -> &'static [&'static str] {
    match field {
        FlowField::Origin => COPY_OPEN_SEARCH_ORIGIN,
        FlowField::Destination => COPY_OPEN_SEARCH_DESTINATION,
    }
}
