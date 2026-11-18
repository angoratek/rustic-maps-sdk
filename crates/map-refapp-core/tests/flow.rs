use map_refapp_core::{FlowEvent, FlowField, FlowOutcome, FlowState, FlowTransition, advance_flow};
use proptest::prelude::*;

fn outcome_id(transition: FlowTransition) -> &'static str {
    transition.outcome.id()
}

#[test]
fn assignment_never_plans() {
    let transition = advance_flow(
        FlowState::PlanSearch,
        FlowEvent::ResultAssigned(FlowField::Destination),
    );
    assert_eq!(transition.state, FlowState::PlanSearch);
    assert!(transition.is_unchanged());
    assert_eq!(outcome_id(transition), "assign_destination");
    assert!(transition.copy_ids.contains(&"plan.plan_route_action"));
}

#[test]
fn only_plan_requested_enters_planning() {
    let transition = advance_flow(FlowState::PlanSearch, FlowEvent::PlanRequested);
    assert_eq!(transition.state, FlowState::Planning);
    assert!(!transition.is_unchanged());
    assert_eq!(outcome_id(transition), "plan");
    assert!(transition.copy_ids.contains(&"plan.planning_in_flight"));

    for event in [
        FlowEvent::FieldTextChanged(FlowField::Origin),
        FlowEvent::SearchRequested(FlowField::Origin),
        FlowEvent::ResultAssigned(FlowField::Origin),
        FlowEvent::MyLocationRequested(FlowField::Origin),
        FlowEvent::SwapRequested,
    ] {
        let transition = advance_flow(FlowState::PlanSearch, event);
        assert_eq!(transition.state, FlowState::PlanSearch);
        assert!(transition.is_unchanged());
    }
}

#[test]
fn plan_success_and_failure_land_in_the_documented_states() {
    let succeeded = advance_flow(FlowState::Planning, FlowEvent::PlanSucceeded);
    assert_eq!(succeeded.state, FlowState::RoutePlanned);
    assert_eq!(outcome_id(succeeded), "plan_succeeded");
    assert!(succeeded.copy_ids.contains(&"route.start_action"));

    let failed = advance_flow(FlowState::Planning, FlowEvent::PlanFailed);
    assert_eq!(failed.state, FlowState::PlanSearch);
    assert_eq!(outcome_id(failed), "plan_failed");
    assert!(failed.copy_ids.contains(&"plan.plan_route_action"));
}

#[test]
fn trip_start_arrival_and_exit_land_in_the_documented_states() {
    let started_from_peek = advance_flow(FlowState::HomePeek, FlowEvent::StartTrip);
    assert_eq!(started_from_peek.state, FlowState::Navigating);
    assert_eq!(outcome_id(started_from_peek), "drive");

    let started_from_route = advance_flow(FlowState::RoutePlanned, FlowEvent::StartTrip);
    assert_eq!(started_from_route.state, FlowState::Navigating);

    let arrived = advance_flow(FlowState::Navigating, FlowEvent::Arrived);
    assert_eq!(arrived.state, FlowState::Arrived);
    assert!(arrived.copy_ids.contains(&"navigation.phase_arrived"));

    let exited = advance_flow(FlowState::Arrived, FlowEvent::Exit);
    assert_eq!(exited.state, FlowState::HomePeek);

    let exited_while_navigating = advance_flow(FlowState::Navigating, FlowEvent::Exit);
    assert_eq!(exited_while_navigating.state, FlowState::HomePeek);
}

#[test]
fn edit_route_returns_to_search_and_plan_requested_replans() {
    let editing = advance_flow(FlowState::RoutePlanned, FlowEvent::EditRoute);
    assert_eq!(editing.state, FlowState::PlanSearch);
    assert!(editing.copy_ids.contains(&"plan.origin_label"));

    let replanning = advance_flow(FlowState::RoutePlanned, FlowEvent::PlanRequested);
    assert_eq!(replanning.state, FlowState::Planning);
}

#[test]
fn search_view_open_and_done_follow_the_peek_contract() {
    let opened = advance_flow(
        FlowState::HomePeek,
        FlowEvent::OpenSearch(FlowField::Origin),
    );
    assert_eq!(opened.state, FlowState::PlanSearch);
    assert_eq!(outcome_id(opened), "search_origin");
    assert!(opened.copy_ids.contains(&"plan.origin_label"));

    let done = advance_flow(FlowState::PlanSearch, FlowEvent::Done);
    assert_eq!(done.state, FlowState::HomePeek);
    assert!(done.copy_ids.contains(&"plan.destination_hint"));
}

#[test]
fn overlays_never_move_the_flow_position() {
    let overlay_events = [
        FlowEvent::OpenSettings,
        FlowEvent::ProviderSelected,
        FlowEvent::CredentialApplied,
        FlowEvent::OpenDiagnostics,
        FlowEvent::DebugDetailToggled,
        FlowEvent::Clear,
        FlowEvent::Close,
    ];
    for state in FlowState::ALL {
        for event in overlay_events {
            let transition = advance_flow(state, event);
            assert_eq!(transition.state, state, "{state:?} moved on {event:?}");
            assert!(transition.is_unchanged(), "{state:?} changed on {event:?}");
        }
    }
}

#[test]
fn every_state_and_event_pair_is_total_and_consistent() {
    for state in FlowState::ALL {
        for event in FlowEvent::ALL {
            let transition = advance_flow(state, event);
            assert!(FlowState::ALL.contains(&transition.state));
            if transition.is_unchanged() {
                assert_eq!(transition.state, state);
            } else {
                assert_ne!(transition.outcome, FlowOutcome::Unchanged);
            }
            for copy_id in transition.copy_ids {
                assert!(
                    copy_id.contains('.'),
                    "copy id {copy_id:?} is not canonical"
                );
            }
        }
    }
}

proptest! {
    #[test]
    fn the_reducer_never_panics_for_any_state_and_event(
        state_index in 0usize..FlowState::ALL.len(),
        event_index in 0usize..FlowEvent::ALL.len(),
    ) {
        let state = FlowState::ALL[state_index];
        let event = FlowEvent::ALL[event_index];
        let transition = advance_flow(state, event);
        prop_assert!(FlowState::ALL.contains(&transition.state));
        prop_assert_eq!(transition.unchanged, transition.state == state);
    }
}
