//! Launch-argument automation for the automated evidence pass: the same
//! user-path methods the sheet's controls call run at startup when the
//! corresponding launch argument is set (simctl has no touch injection, so
//! the plan/navigate/evidence captures use explicit launch arguments
//! instead; the states themselves stay the real SDK-driven ones, nothing
//! is forced).

import Foundation
import MapNativeCoreUI

/// Applies the application's launch automation flags after a successful
/// application start.
extension ReferenceApplicationModel {
    /// Reads the documented launch arguments from the process arguments
    /// (the `-key value` form lands in `UserDefaults` automatically) and
    /// applies their demo actions in order. The demo flags drive the
    /// control cluster's and the follow camera's same user-path methods
    /// the on-map controls call (no state is forced).
    func applyLaunchAutomation() async {
        let processDefaults = UserDefaults.standard
        if processDefaults.bool(
            forKey: ReferenceApplicationConfiguration.launchArgumentResetRememberedChoices
        ) {
            resetRememberedChoices()
        }
        if processDefaults.bool(
            forKey: ReferenceApplicationConfiguration.launchArgumentLightThemeSelection
        ) {
            themeChoice = .light
        }
        if processDefaults.bool(
            forKey: ReferenceApplicationConfiguration.launchArgumentSheetHalfState
        ) {
            homeSheetState = .half
        }
        if processDefaults.bool(
            forKey: ReferenceApplicationConfiguration.launchArgumentSearchOpen
        ) {
            homeSheetState = .full
        }
        if processDefaults.bool(
            forKey: ReferenceApplicationConfiguration.launchArgumentTrackingFixDemo
        ) {
            placeDemoTrackingFix()
        }
        if processDefaults.bool(
            forKey: ReferenceApplicationConfiguration.launchArgumentZoomInTapDemo
        ) {
            userTappedZoomInControl()
        }
        if processDefaults.bool(
            forKey: ReferenceApplicationConfiguration.launchArgumentZoomOutTapDemo
        ) {
            userTappedZoomOutControl()
        }
        if processDefaults.bool(
            forKey: ReferenceApplicationConfiguration
                .launchArgumentThreeDimensionalPitchToggleDemo
        ) {
            toggledThreeDimensionalPitch()
        }
        if processDefaults.bool(
            forKey: ReferenceApplicationConfiguration
                .launchArgumentTwoFingerRotateDemo
        ) {
            scheduleRotationDemo()
        }
        if processDefaults.bool(
            forKey: ReferenceApplicationConfiguration.launchArgumentGesturePanDemo
        ) {
            scheduleContentFollowPanDemo()
        }
        if processDefaults.bool(
            forKey: ReferenceApplicationConfiguration.launchArgumentCompassRenorthDemo
        ) {
            await runCompassRenorthDemo()
        }
        if processDefaults.bool(
            forKey: ReferenceApplicationConfiguration.launchArgumentRoutePlanningDemo
        ) {
            await runRoutePlanningDemo()
        }
        if processDefaults.bool(
            forKey: ReferenceApplicationConfiguration.launchArgumentKeylessSearchDemo
        ) {
            await runKeylessSearchDemo()
        }
        if processDefaults.bool(
            forKey: ReferenceApplicationConfiguration.launchArgumentKeylessPlanDemo
        ) {
            await runKeylessPlanDemo()
        }
        if processDefaults.bool(
            forKey: ReferenceApplicationConfiguration.launchArgumentFillPlanEndpoints
        ) {
            await runFillPlanEndpointsDemo()
        }
        if processDefaults.bool(
            forKey: ReferenceApplicationConfiguration.launchArgumentCapturePlanEndpoints
        ) {
            runCapturePlanEndpointsDemo()
        }
        // A recorded demo plan selected by its artifact scenario name: the
        // picker's own selection act, kept as the launch seam now that the
        // visible picker rows are gone. An unknown scenario name is ignored
        // rather than guessed at, so the default selection stays in force.
        if let demoPlanScenarioName = processDefaults.string(
            forKey: ReferenceApplicationConfiguration
                .launchArgumentDemoPlanScenario
        ), !demoPlanScenarioName.isEmpty,
            let demoPlan = demoPlans.first(where: { plan in
                plan.scenarioName == demoPlanScenarioName
            })
        {
            // The selection's preview fit needs the attached viewport; wait
            // out the first configure tick as the gesture demos do, so the
            // fit is never silently skipped.
            try? await Task.sleep(nanoseconds: UInt64(
                ReferenceApplicationConfiguration.gestureDemoConfigureDelaySeconds
                    * ReferenceApplicationConfiguration.nanosecondsPerSecond
            ))
            selectDemoPlan(demoPlan)
        }
        if processDefaults.bool(
            forKey: ReferenceApplicationConfiguration.launchArgumentWalkthroughDemo
        ) {
            await runWalkthroughDemo()
        }
        // The opening base map: one provider key, resolved through the
        // registry like any other selection.
        if let demoBaseMapProviderKey = processDefaults.string(
            forKey: ReferenceApplicationConfiguration.launchArgumentDemoBaseMap
        ), !demoBaseMapProviderKey.isEmpty {
            selectBaseMap(providerKey: demoBaseMapProviderKey)
        }
        if processDefaults.bool(
            forKey: ReferenceApplicationConfiguration.launchArgumentSimulatedJourneyAutostart
        ) {
            startJourneyWithAutomationFlags(processDefaults, startsPlannedRoute: true)
        }
        if processDefaults.bool(
            forKey: ReferenceApplicationConfiguration.launchArgumentSampleJourneyAutostart
        ) {
            startJourneyWithAutomationFlags(processDefaults, startsPlannedRoute: false)
        }
        // The route-following trip over a committed route: the walker's
        // speed, pause and arrival path without the network (the UI suite's
        // deterministic seam; the demo scenarios keep their recorded
        // traces).
        if processDefaults.bool(
            forKey: ReferenceApplicationConfiguration
                .launchArgumentRouteFollowingJourneyAutostart
        ), let demoRouteData = selectedDemoPlan?.journey.routeData ?? recordedJourney?.routeData {
            homeSheetState = .peek
            startRouteFollowingJourney(routeData: demoRouteData)
        }
        // The device location source is the same user-path method the Location
        // group's control runs; the simulator needs a location set for a fix.
        if processDefaults.bool(
            forKey: ReferenceApplicationConfiguration.launchArgumentDeviceLocationAutostart
        ) {
            toggleDeviceLocation()
        }
        // A shared reference flow drives the same user-path methods the
        // sheet's controls call; the flow schema's preconditions are the
        // caller's, and an unknown flow runs nothing.
        if let referenceFlowIdentifier = processDefaults.string(
            forKey: ReferenceApplicationConfiguration.launchArgumentReferenceFlow
        ), !referenceFlowIdentifier.isEmpty {
            await runReferenceFlow(referenceFlowIdentifier)
        }
        // The diagnostics open applies last: a journey's start closes the
        // sheet (§4.1), so the flag has to win over the journey flags it
        // shares a launch with.
        if processDefaults.bool(
            forKey: ReferenceApplicationConfiguration.launchArgumentDiagnosticsOpen
        ) {
            openDiagnosticsSheet()
        }
        // The provider console's open path: the open arms the feed exactly as
        // the floating control does. The unread-dot probe records at the
        // start of the session (the slower provider registration must not
        // delay it).
        if processDefaults.bool(
            forKey: ReferenceApplicationConfiguration
                .launchArgumentProviderConsoleOpen
        ) {
            openProviderConsole()
        }
    }

    /// Runs one shared reference flow through the application's own
    /// user-path methods. The flows are the schema's four scenarios; an
    /// unknown identifier is ignored rather than guessed at.
    private func runReferenceFlow(_ flowIdentifier: String) async {
        switch flowIdentifier {
        case ReferenceApplicationConfiguration.referenceFlowPlanAndNavigate:
            await runKeylessPlanDemo()
            if plannedRoute != nil {
                homeSheetState = .peek
                startSimulatedJourney()
            }
        case ReferenceApplicationConfiguration.referenceFlowMyLocationField:
            homeSheetState = .full
            mapLayersState.myLocationChipTapped(for: .origin)
            mapLayersState.myLocationChipTapped(for: .destination)
        case ReferenceApplicationConfiguration.referenceFlowRoutingProviderSwitch:
            if let routingOption = capabilityProviderState
                .options(for: .routing)
                .first(where: { $0.hasDemoBaseURL })
            {
                capabilityProviderState.select(
                    contract: .routing,
                    providerKey: routingOption.providerKey
                )
            }
            isProviderSettingsSheetOpen = true
        case ReferenceApplicationConfiguration.referenceFlowDiagnosticsOptIn:
            openDiagnosticsSheet()
        default:
            break
        }
    }

    /// The journey launch flags' shared application: the off-route toggle
    /// applied to the journey that starts, the journey itself, and the
    /// scripted gesture-break demo scheduled into the running trip (the
    /// §5.3 break/re-engage pair, captureable without touch injection).
    private func startJourneyWithAutomationFlags(
        _ processDefaults: UserDefaults,
        startsPlannedRoute: Bool
    ) {
        if processDefaults.bool(
            forKey: ReferenceApplicationConfiguration.launchArgumentOffRouteSimulation
        ) {
            isOffRouteSimulationActive = true
        }
        if startsPlannedRoute {
            homeSheetState = .peek
            startSimulatedJourney()
        } else {
            homeSheetState = .peek
            startSampleJourney()
        }
        if processDefaults.bool(
            forKey: ReferenceApplicationConfiguration.launchArgumentGestureBreakFollowDemo
        ) {
            scheduleGestureBreakFollowDemo()
        }
    }

    /// The compass re-north demo: the camera turns to the demo's
    /// off-north bearing through the rotation gesture's same user-path
    /// method, settles one drawn frame past the turn, then runs the
    /// compass control's exact tap action — the pump eases north up.
    private func runCompassRenorthDemo() async {
        // The rotation member needs an attached viewport; wait out the
        // map's configure delay before the off-north turn.
        try? await Task.sleep(nanoseconds: UInt64(
            ReferenceApplicationConfiguration.gestureDemoConfigureDelaySeconds
                * ReferenceApplicationConfiguration.nanosecondsPerSecond
        ))
        userRotatedMapDegrees(
            ReferenceApplicationConfiguration.compassRenorthDemoBearingDegrees
        )
        try? await Task.sleep(nanoseconds: UInt64(
            ReferenceApplicationConfiguration.compassRenorthDemoSettleSeconds
                * ReferenceApplicationConfiguration.nanosecondsPerSecond
        ))
        compassControlTapped()
    }

    /// The scripted follow-break demo: the drag gesture's user-path method
    /// pans the map `gestureBreakFollowDelaySeconds` into the journey, so
    /// the §5.3 gesture break is live and the next fix re-engages follow.
    private func scheduleGestureBreakFollowDemo() {
        Task { @MainActor [weak self] in
            try? await Task.sleep(nanoseconds: UInt64(
                ReferenceApplicationConfiguration.gestureBreakFollowDelaySeconds
                    * ReferenceApplicationConfiguration.nanosecondsPerSecond
            ))
            self?.userDraggedMapPixels(
                ReferenceApplicationConfiguration.gestureBreakFollowDragDeltaPoints
            )
        }
    }

    /// The content-follows-pointer direction demo (§5.1's regression pin):
    /// the drag gesture's same user-path method applies the demo's
    /// rightward drag once the map surface configured (the method no-ops
    /// before that — the follow-break demo's same scheduling pattern), so
    /// the drawn camera readback reports the content-follows-pointer pan.
    private func scheduleContentFollowPanDemo() {
        Task { @MainActor [weak self] in
            try? await Task.sleep(nanoseconds: UInt64(
                ReferenceApplicationConfiguration.gestureDemoConfigureDelaySeconds
                    * ReferenceApplicationConfiguration.nanosecondsPerSecond
            ))
            self?.userDraggedMapPixels(
                ReferenceApplicationConfiguration.gesturePanDemoDragDeltaPoints
            )
        }
    }

    /// The rotation demo: the rotation gesture's same user-path method
    /// runs past the map's configure delay (the contract member needs an
    /// attached viewport), so the §5.1 content-follows-fingers direction is
    /// live once the first frames drew.
    private func scheduleRotationDemo() {
        Task { @MainActor [weak self] in
            try? await Task.sleep(nanoseconds: UInt64(
                ReferenceApplicationConfiguration.gestureDemoConfigureDelaySeconds
                    * ReferenceApplicationConfiguration.nanosecondsPerSecond
            ))
            self?.userRotatedMapDegrees(
                ReferenceApplicationConfiguration.twoFingerRotateDemoDegrees
            )
        }
    }

    /// The keyless demo search: filters the artifact's demo places by the
    /// first demo place's name through the search view's own user path,
    /// then leaves the full search view showing the demo rows.
    private func runKeylessSearchDemo() async {
        guard let demoPlace = catalog?.demoPlaces.first else {
            // The artifact load already reported its own honest failure.
            return
        }
        destinationSearchText = demoPlace.name
        searchFieldAssignmentTarget = .destination
        await runSearch()
        homeSheetState = .full
    }

    /// The keyless demo plan: searches and assigns the first recorded
    /// route's endpoints through the search view's own user-path methods,
    /// then runs the explicit Plan route action — the recorded route's
    /// summary at half, never a provider call.
    private func runKeylessPlanDemo() async {
        guard await assignRecordedRouteEndpoints() else { return }
        await planRouteFromFields()
    }

    /// The keyed capture's plan sheet: both fields hold the first recorded
    /// route's endpoint names with no search provider call, so the frame
    /// shows the reference scenario rather than a live provider's best guess.
    private func runCapturePlanEndpointsDemo() {
        guard let recordedRoute = catalog?.demoRoutes.first else { return }
        originSearchText = recordedRoute.originName
        destinationSearchText = recordedRoute.destinationName
        homeSheetState = .full
    }

    /// The plan-sheet capture: the two recorded endpoints assigned through the
    /// search view's own user-path methods, the sheet left open at full.
    private func runFillPlanEndpointsDemo() async {
        guard await assignRecordedRouteEndpoints() else { return }
        homeSheetState = .full
    }

    /// Searches and assigns the first recorded route's endpoints. Returns
    /// false when the artifact or a search produced nothing, after the search
    /// already reported its own honest failure.
    private func assignRecordedRouteEndpoints() async -> Bool {
        guard let recordedRoute = catalog?.demoRoutes.first else {
            // The artifact load already reported its own honest failure.
            return false
        }
        originSearchText = recordedRoute.originName
        searchFieldAssignmentTarget = .origin
        await runSearch()
        guard let originResult = searchResults.first else { return false }
        assignSearchResult(originResult)
        destinationSearchText = recordedRoute.destinationName
        searchFieldAssignmentTarget = .destination
        await runSearch()
        guard let destinationResult = searchResults.first else { return false }
        assignSearchResult(destinationResult)
        return true
    }

    /// The route-planning demo: geocodes the canned destination query,
    /// picks its first result and plans the route to the demo position's
    /// origin (the exact path the search view drives by hand) — the model
    /// folds the sheet to half with the route summary card.
    private func runRoutePlanningDemo() async {
        destinationSearchText =
            ReferenceApplicationConfiguration.routePlanningDemoDestinationQuery
        searchFieldAssignmentTarget = .destination
        await runSearch()
        guard let firstResult = searchResults.first else {
            // The search already reported its own honest failure (§6.3).
            return
        }
        assignSearchResult(firstResult)
        await planRouteFromDemoPosition()
    }
}