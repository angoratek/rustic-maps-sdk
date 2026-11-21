package com.rusticmaps.sdk.referenceapplication

import android.Manifest
import android.content.Intent
import android.graphics.PointF
import android.os.SystemClock
import android.view.InputDevice
import android.view.MotionEvent
import androidx.test.core.app.ActivityScenario
import androidx.test.core.app.ApplicationProvider
import androidx.test.espresso.Espresso
import androidx.test.espresso.assertion.ViewAssertions
import androidx.test.espresso.matcher.ViewMatchers
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.rusticmaps.sdk.MapCameraPose
import com.rusticmaps.sdk.referenceapplication.ReferenceApplicationState.HomeSheetState
import com.rusticmaps.sdk.referenceapplication.map.MapLayerBaseSelection
import com.rusticmaps.sdk.referenceapplication.navigation.SimulatedTrace
import com.rusticmaps.sdk.ux.ReferenceAppUXDimensions
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assume
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.rustic_maps.AvoidOptions
import uniffi.rustic_maps.BoundingBox
import uniffi.rustic_maps.GeocodeQuality
import uniffi.rustic_maps.GeographicCoordinate
import uniffi.rustic_maps.NavigationPhase
import uniffi.rustic_maps.Place
import uniffi.rustic_maps.ProviderContract
import uniffi.rustic_maps.Route
import uniffi.rustic_maps.RouteOptions
import uniffi.rustic_maps.RouteStrategy
import uniffi.rustic_maps.TrafficFlowCongestionLevel
import uniffi.rustic_maps.TrafficFlowData
import uniffi.rustic_maps.TransportationMode
import uniffi.rustic_maps.UnitSystem
import uniffi.rustic_maps.instructionBannerText

/**
 * The connected UI-automation suite of the Compose reference application
 * (round 12): instrumentation drives the same user-path methods the sheet's
 * controls call — the Android analogue of the iOS reference app's launch-
 * argument demo automation (`ReferenceApplicationLaunchAutomation.swift`,
 * written there because `simctl` cannot inject touch) — and asserts the exact
 * values the screen renders (docs/ReferenceAppUX.md §1/§4/§6/§7). No state
 * is ever forced; every drive goes through the public user-path methods of
 * [ReferenceApplicationState], read back through MainActivity's documented
 * automation seam.
 *
 * Determinism rules of the suite: no screenshot waits (every await is a poll
 * on Compose snapshot state with a named timeout), no live provider call
 * from the tests (the demos replay the committed recorded traces and the
 * bundled shared demo artifact; the search flow under test would geocode
 * live, so it is deliberately not exercised here), and no test enters or logs
 * a provider credential by default: the application starts keyless, and a
 * credential reaches it only through the launch-intent automation seam when
 * the instrumentation run supplies one (§6.5).
 */
@RunWith(AndroidJUnit4::class)
class ReferenceApplicationAndroidTest {

    /** The one application scenario the running test launched, closed after each test. */
    private var applicationScenario: ActivityScenario<MainActivity>? = null

    /**
     * Restores the idle home state, closes the scenario, and revokes the
     * location runtime permission after every case so no grant reaches the
     * next one.
     */
    @After
    fun restoresTheApplicationToItsIdleHomeState() {
        try {
            // Isolation first: no case may inherit a remembered credential or
            // an explicit choice from the case before it (or from an earlier
            // run).
            clearPersistedCredentialStore()
            clearRememberedChoices()
            val scenario = applicationScenario ?: return
            applicationScenario = null
            try {
                scenario.onActivity { activity ->
                    val applicationState = activity.automationApplicationState ?: return@onActivity
                    // A trip left running would keep replaying into a torn-down
                    // composition: the suite stops it the way the user's exit
                    // control does, then collapses the sheet toward peek — the
                    // demo's idle home state (no navigation, no automation state
                    // carrying over into the next test).
                    if (applicationState.isNavigationInTrip || applicationState.tripDriver.isTripActive) {
                        applicationState.exitSimulatedTrip()
                    }
                    // A location source a case started by hand must not keep
                    // listening into the next test.
                    if (applicationState.isDeviceLocationActive) {
                        applicationState.stopDeviceLocation()
                    }
                    if (applicationState.mapLayersState.isTrafficEnabled) {
                        applicationState.setTrafficLayerEnabled(false)
                    }
                    if (applicationState.mapLayersState.isPanelOpen) {
                        applicationState.closeMapLayersPanel()
                    }
                    applicationState.dismissDiagnostics()
                    applicationState.requestHomeSheetState(HomeSheetState.PEEK)
                }
            } finally {
                scenario.close()
            }
        } finally {
            // A runtime permission outlives a test case, and revoking it inside
            // the process kills the target process that hosts this runner. The
            // module's host-side reset (`resetTargetFineLocationPermission` and
            // `resetTargetCoarseLocationPermission`) revokes the location
            // grants before every connected run, so each run starts from the
            // ungranted state.
        }
    }

    @Test
    fun applicationLaunchesToTheReadyLadderOrTheNeutralKeylessPrompt() {
        // A previous session on this device may have remembered explicit
        // choices; clear them so this case pins the never-chose defaults.
        clearRememberedChoices()
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        Espresso.onView(ViewMatchers.isRoot()).check(ViewAssertions.matches(ViewMatchers.isDisplayed()))
        if (hasRuntimeInjectedApiKey()) {
            assertEquals(ReadinessState.READY, applicationState.readinessState)
            assertTrue(
                "the readiness chip must name the gpu renderer with the compiled version",
                applicationState.readinessChipText.startsWith(READY_STATUS_TEXT_PREFIX),
            )
        } else {
            assertTrue("a keyless start registers no provider", applicationState.isProviderKeyless)
            // The registry's credential-less base map serves the map (M24), so
            // the chip carries the ready line; a registry without one shows the
            // neutral keyless prompt instead.
            val registryCarriesKeylessBaseMap = applicationState.baseMapChoices.any { choice ->
                choice.descriptor.rasterUrlTemplate.isNotBlank()
            }
            if (registryCarriesKeylessBaseMap) {
                assertEquals(ReadinessState.READY, applicationState.readinessState)
            } else {
                assertEquals(
                    ReferenceApplicationConstants.KEYLESS_CREDENTIAL_STATUS_TEXT,
                    applicationState.readinessChipText,
                )
            }
        }
        // §3: three-way theme choice with Auto the default on a device that
        // never chose; Auto resolves against the live system scheme.
        assertEquals(ThemeChoice.AUTO, applicationState.themeChoice)
        assertTrue(applicationState.resolvesToDarkTheme(true))
        assertFalse(applicationState.resolvesToDarkTheme(false))
        // §1: the home sheet starts at peek; no planned search and no trip.
        assertEquals(HomeSheetState.PEEK, applicationState.homeSheetState)
        assertFalse(applicationState.isNavigationInTrip)
        assertEquals("", applicationState.navigationPhaseChip)
        // §9: the shared artifact's overlay scenes all start off.
        applicationState.sceneToggleStates.values.forEach { isSceneSelected ->
            assertFalse(isSceneSelected)
        }
        if (hasRuntimeInjectedApiKey()) {
            // A frame really drew on this emulator's software graphics stack —
            // the readiness ladder's honest accounting, not a screenshot.
            awaitOrTimeout(
                timeoutMilliseconds = FIRST_FRAME_DRAWN_TIMEOUT_MILLISECONDS,
                timeoutDescription = "the first drawn frame on the emulator graphics stack",
            ) {
                applicationState.framesDrawnCount > 0
            }
        }
        assertNull("an honest startup reports nothing in the error strip", applicationState.errorReport)
    }

    /**
     * A keyless start installs no tile cache, so the offline card states the
     * SDK's own `TileCache` refusal and starts no region download. The case
     * stays off the network by construction: it never asks for a download,
     * and it is skipped when the automation supplied a credential (that run
     * would install a cache and could fetch).
     */
    @Test
    fun keylessOfflineCardStatesTheTileCacheRefusalWithoutStartingDownload() {
        Assume.assumeFalse(
            "a runtime credential installs a tile cache; this case pins the keyless refusal",
            hasRuntimeInjectedApiKey(),
        )
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        val offlineRegionState = applicationState.offlineRegionState

        assertEquals(
            "no 'TileCache' provider is configured",
            offlineRegionState.tileCacheRefusalReason,
        )
        assertNull("no progress is faked without a cache", offlineRegionState.progress)
        assertFalse(offlineRegionState.canDownload)

        offlineRegionState.downloadDemoRegion()
        assertNull("the refusal starts no download", offlineRegionState.progress)
    }

    @Test
    fun theDemoPlanAndSceneStateDriveThePlannedRouteDemo() {
        Assume.assumeTrue(
            "the recorded-route preview needs the runtime-injected provider key",
            hasRuntimeInjectedApiKey(),
        )
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        val initialCameraValues = readApplicationCameraValues(applicationState)

        // The demo-plan state's own act (the visible picker row is gone; the
        // data and the selection stay, and the layer panel is the scene
        // home): selecting the clean walk applies the route-preview framing
        // over the recorded trace's route — a live demo of the planned route
        // without any live route provider call (the recorded fixture is the
        // route).
        // The state's rows and copy come from the shared artifact; the test
        // selects the artifact's own navigation demo row by name.
        val cleanWalkDefinition = applicationState.artifact.demoPlans.first { planDefinition ->
            planDefinition.scenarioName == applicationState.artifact.navigationDemo.traceName
        }
        val cleanWalkPlan = applicationState.demoPlans.firstOrNull { demoPlan ->
            demoPlan.label == cleanWalkDefinition.label
        } ?: error("${cleanWalkDefinition.label} plan missing from the picker")
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.selectDemoPlan(cleanWalkPlan)
        }
        assertEquals(cleanWalkDefinition.label, applicationState.selectedDemoPlan.label)
        assertEquals(
            cleanWalkDefinition.scenarioName,
            applicationState.selectedDemoPlan.scenarioName,
        )
        assertNull("the picker's recorded demo plans are not search-planned routes", applicationState.plannedRoute)
        assertFalse(applicationState.isNavigationInTrip)
        // §5.1's fit contract: the preview's framing is top-down and north up.
        assertEquals(0.0, applicationState.bearingDegrees, CAMERA_READBACK_TOLERANCE_DEGREES)
        assertEquals(0.0, applicationState.pitchDegrees, CAMERA_READBACK_TOLERANCE_DEGREES)
        // The fit actually applied: the preview framing moved the camera
        // values away from the initial demo preset the app starts on.
        assertTrue(
            "the route-preview framing must change the camera values",
            readApplicationCameraValues(applicationState).differsFrom(initialCameraValues),
        )

        // The active plan's route line rides the overlay push (the state act
        // toggles one shared-artifact scene and the push follows).
        awaitOrTimeout(
            timeoutMilliseconds = OVERLAY_PUSH_SETTLE_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the route-line overlay push of the selected plan",
        ) {
            applicationState.pushedOverlayCount == PLANNED_ROUTE_LINE_OVERLAY_COUNT
        }
        driveUserPathOnMainThread { applicationUserState ->
            // The scene toggle's own act (the layer panel's switch): the
            // markers scene toggles on, the artifact's markers scene joins
            // the pushed overlay list.
            applicationUserState.sceneToggleStates[MARKERS_SCENE_KEY] = true
        }
        awaitOrTimeout(
            timeoutMilliseconds = OVERLAY_PUSH_SETTLE_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the markers scene joining the pushed overlay list",
        ) {
            applicationState.pushedOverlayCount == PLANNED_ROUTE_LINE_OVERLAY_COUNT + SCENE_OVERLAY_COUNT
        }
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.sceneToggleStates[MARKERS_SCENE_KEY] = false
        }
        awaitOrTimeout(
            timeoutMilliseconds = OVERLAY_PUSH_SETTLE_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the toggled-off scene leaving the pushed overlay list",
        ) {
            applicationState.pushedOverlayCount == PLANNED_ROUTE_LINE_OVERLAY_COUNT
        }
        assertNull("a scene push reports nothing in the error strip", applicationState.errorReport)
    }

    @Test
    fun theTraceReplayRendersTheRecordingsStepInstructionsAndTheExitRestoresHome() {
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        val preTripCameraValues = readApplicationCameraValues(applicationState)
        val selectedPlan = applicationState.selectedDemoPlan
        val recordedStepDisplayTexts = selectedPlan.trace.route.legs
            .flatMap { leg -> leg.steps }
            .map { step -> step.instructionDisplayText }
        assertTrue(
            "the recorded demo trace must carry steps to assert against",
            recordedStepDisplayTexts.isNotEmpty(),
        )

        // The peek primary button's own act (§4.2): the trip replays the
        // artifact trace's recorded fixes through the navigation controller.
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.startSimulatedTrip()
        }
        assertFalse(
            "the trip start must not half-start",
            applicationState.tripDriver.startFailure != null,
        )
        awaitOrTimeout(
            timeoutMilliseconds = TRIP_FIRST_INSTRUCTION_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the replay's first instruction delivery",
        ) {
            applicationState.navigationListener.instructionState.value != null
        }

        // §7.1/§7.2: the turn banner renders a recorded step's display text
        // (the exact composed form the banner card renders), the drawer runs
        // the Navigating phase, and the replay's fixes already drive the trip
        // state before the test exits the trip.
        val deliveredInstruction = applicationState.navigationListener.instructionState.value ?: error("no instruction delivered")
        val providerStepText = deliveredInstruction.activeManeuver.instruction.providerDisplayText
            ?: throw AssertionError("the delivered maneuver carried no provider display text")
        // The reducer's replay is provider-text-based: the composed banner
        // prefixes the live distance phrase onto the recording's own text.
        assertTrue(
            "the banner's provider step text must be the recording's text, was '$providerStepText'",
            providerStepText in recordedStepDisplayTexts,
        )
        val renderedBannerText = instructionBannerText(
            composedDisplayText = deliveredInstruction.activeManeuver.instruction.composedDisplayText,
            providerDisplayText = deliveredInstruction.activeManeuver.instruction.providerDisplayText,
            roundaboutExitIndex = deliveredInstruction.activeManeuver.roundaboutExitIndex,
        )
        assertTrue(
            "the banner must render the distance-composed step text, was '$renderedBannerText'",
            renderedBannerText.endsWith(providerStepText),
        )
        assertTrue(applicationState.isNavigationInTrip)
        assertEquals(NAVIGATING_PHASE_CHIP_TEXT, applicationState.navigationPhaseChip)
        awaitOrTimeout(
            timeoutMilliseconds = TRIP_FIRST_INSTRUCTION_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the replay's first processed trip state",
        ) {
            applicationState.tripDriver.latestTripState != null
        }

        // The drawer's exit control's own act: the replay stops, the pre-trip
        // camera restores verbatim, the location layer clears, home returns.
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.exitSimulatedTrip()
        }
        assertFalse(applicationState.isNavigationInTrip)
        assertFalse(applicationState.tripDriver.isTripActive)
        assertEquals("", applicationState.navigationPhaseChip)
        assertNull(applicationState.latestLocationFix)
        assertEquals(HomeSheetState.PEEK, applicationState.homeSheetState)
        val restoredCameraValues = readApplicationCameraValues(applicationState)
        assertTrue(
            "the exit must restore the pre-trip camera verbatim",
            restoredCameraValues == preTripCameraValues,
        )
        // Arrived must not present after the exit: the phase topic stays reset.
        assertNull(applicationState.navigationListener.phaseChangeState.value)
        assertTrue(
            "the exit must stop the replay short of the arrival phase",
            applicationState.tripDriver.latestTripState?.phase != NavigationPhase.ARRIVED,
        )
    }

    /**
     * The seek path (M27): a route-following trip over a committed demo
     * route's geometry is walked by the contract's shared walker. Seeking to
     * the route's end presents the walker's own arrival, and seeking back
     * leaves it while the paused trip stays paused at the new position.
     */
    @Test
    fun seekingToTheEndArrivesAndSeekingBackLeavesTheArrival() {
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        val demoRoute = applicationState.selectedDemoPlan.trace.route
        // The route-following source: the same walker a search-planned trip
        // uses, over committed geometry, so the run needs no provider.
        driveUserPathOnMainThread { userPathState ->
            userPathState.tripDriver.startTrip(
                CoroutineScope(Dispatchers.Main),
                SimulatedTrace.fromRouteData(demoRoute, "seek-assertion"),
            )
        }
        awaitOrTimeout(
            timeoutMilliseconds = TRIP_FIRST_INSTRUCTION_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the route-following walker's seekable range",
        ) {
            applicationState.tripDriver.seekableDistanceMeters > 0.0
        }
        val totalDistanceMeters = applicationState.tripDriver.seekableDistanceMeters

        driveUserPathOnMainThread { userPathState -> userPathState.pauseSimulatedTrip() }
        driveUserPathOnMainThread { userPathState ->
            userPathState.seekSimulatedTrip(totalDistanceMeters)
        }
        awaitOrTimeout(
            timeoutMilliseconds = TRIP_FIRST_INSTRUCTION_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the seek to the route's end presenting the arrival",
        ) {
            applicationState.tripDriver.hasArrived
        }
        assertTrue(
            "a seek to the end must land exactly on the route's total distance",
            kotlin.math.abs(
                applicationState.tripDriver.travelledDistanceMeters - totalDistanceMeters,
            ) <= SEEK_POSITION_TOLERANCE_METERS,
        )

        driveUserPathOnMainThread { userPathState ->
            userPathState.seekSimulatedTrip(totalDistanceMeters / 2)
        }
        awaitOrTimeout(
            timeoutMilliseconds = TRIP_FIRST_INSTRUCTION_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the backwards seek leaving the arrival",
        ) {
            !applicationState.tripDriver.hasArrived
        }
        assertEquals(
            "the backwards seek lands at the requested distance",
            totalDistanceMeters / 2,
            applicationState.tripDriver.travelledDistanceMeters,
            SEEK_POSITION_TOLERANCE_METERS,
        )
        assertTrue(
            "seeking while paused keeps the trip paused",
            applicationState.tripDriver.isTripPaused,
        )
        val heldDistanceMeters = applicationState.tripDriver.travelledDistanceMeters
        Thread.sleep(READINESS_POLL_INTERVAL_MILLISECONDS)
        assertEquals(
            "a paused trip holds the seek's position",
            heldDistanceMeters,
            applicationState.tripDriver.travelledDistanceMeters,
            SEEK_POSITION_TOLERANCE_METERS,
        )
        driveUserPathOnMainThread { userPathState -> userPathState.exitSimulatedTrip() }
    }

    @Test
    fun theLiveSearchAndRouteFlowRunsWhenAKeyIsInjected() {
        Assume.assumeTrue(
            "requires the runtime-injected provider key",
            hasRuntimeInjectedApiKey(),
        )
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        // The plan flow's own user path: search and assign one result per
        // field, then the explicit Plan route action (§4.1: a field that
        // holds no resolved place keeps Plan route disabled, and assignment
        // never plans by itself).
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.openPlanSearch()
            applicationUserState.destinationSearchText = LIVE_SEARCH_QUERY_TEXT
            applicationUserState.runPlanSearch(
                ReferenceApplicationState.PlanSearchField.DESTINATION,
            )
        }
        awaitOrTimeout(
            timeoutMilliseconds = LIVE_PROVIDER_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the live provider's destination geocode results",
        ) {
            applicationState.searchResults.isNotEmpty()
        }
        val firstDestinationResult = applicationState.searchResults.first()
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.assignSearchResult(firstDestinationResult)
            assertFalse(
                "assignment alone never plans (the explicit Plan route action does)",
                applicationState.plannedRoute != null,
            )
        }
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.originSearchText = LIVE_ORIGIN_SEARCH_QUERY_TEXT
            applicationUserState.runPlanSearch(
                ReferenceApplicationState.PlanSearchField.ORIGIN,
            )
        }
        awaitOrTimeout(
            timeoutMilliseconds = LIVE_PROVIDER_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the live provider's origin geocode results",
        ) {
            applicationState.searchResults.isNotEmpty()
        }
        val firstOriginResult = applicationState.searchResults.first()
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.assignSearchResult(firstOriginResult)
            assertTrue(
                "both resolved fields must enable the Plan route action",
                applicationState.canPlanRoute,
            )
        }
        // The explicit action plans the two resolved places.
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.planRouteFromPlanFields()
        }
        awaitOrTimeout(
            timeoutMilliseconds = LIVE_PROVIDER_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the live provider's planned route",
        ) {
            applicationState.plannedRoute != null
        }
        // The plan carried the Directions control's default Car mode to the
        // SDK, never the old hardcoded pedestrian mode.
        assertEquals(
            TransportationMode.AUTOMOBILE,
            applicationState.lastPlanRouteOptions?.transportationMode,
        )
        assertNull("a successful live flow reports nothing in the error strip", applicationState.errorReport)
    }

    /**
     * The Directions control (PRD.md §6 item 36): it starts
     * on Car and Fastest, carries the selected mode in its plan options, and
     * reflects the serving provider's capability answer — the contract's own
     * reason while no routing provider is installed, and the offered choices
     * once one is, with Shortest disabled and annotated when unsupported.
     */
    @Test
    fun theDirectionsControlDefaultsToCarAndReportsTheProviderCapabilities() {
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.openPlanSearch()
        }
        val directionsState = applicationState.directionsState
        assertEquals(
            "the Directions control defaults to Car",
            DirectionsTransportationMode.CAR,
            directionsState.selectedTransportationMode,
        )
        assertEquals(
            "the Directions control defaults to Fastest",
            DirectionsRouteStrategy.FASTEST,
            directionsState.selectedRouteStrategy,
        )
        assertEquals(
            "the plan options carry the default automobile mode",
            TransportationMode.AUTOMOBILE,
            directionsState.routeOptions(UnitSystem.METRIC, avoidNothingOptions())
                .transportationMode,
        )
        if (hasRuntimeInjectedApiKey()) {
            assertTrue(
                "the live routing provider must support car routing",
                directionsState.isTransportationModeAvailable(
                    DirectionsTransportationMode.CAR,
                ),
            )
            if (directionsState.capabilities.supportsShortestRouteStrategy) {
                driveUserPathOnMainThread { applicationUserState ->
                    applicationUserState.directionsState.selectRouteStrategy(
                        DirectionsRouteStrategy.SHORTEST,
                    )
                }
                assertEquals(
                    DirectionsRouteStrategy.SHORTEST,
                    directionsState.selectedRouteStrategy,
                )
            } else {
                assertFalse(
                    "a provider without shortest support disables Shortest",
                    directionsState.isShortestRouteStrategyAvailable(),
                )
                assertNotNull(
                    "the disabled Shortest choice carries its reason",
                    directionsState.routeStrategyUnavailableReason(),
                )
            }
        } else {
            assertEquals(
                "the keyless session states the contract's not-configured reason",
                "no '${ROUTING_CONTRACT_NAME}' provider is configured",
                directionsState.capabilities.unavailableReasonText,
            )
            assertFalse(
                "no routing provider means no selectable mode",
                directionsState.isTransportationModeAvailable(
                    DirectionsTransportationMode.CAR,
                ),
            )
            assertFalse(
                directionsState.isTransportationModeAvailable(
                    DirectionsTransportationMode.WALK,
                ),
            )
        }
        // Walk is selectable when the provider offers it, and it carries the
        // pedestrian mode to the plan options.
        if (directionsState.isTransportationModeAvailable(DirectionsTransportationMode.WALK)) {
            driveUserPathOnMainThread { applicationUserState ->
                applicationUserState.directionsState.selectTransportationMode(
                    DirectionsTransportationMode.WALK,
                )
            }
            assertEquals(
                DirectionsTransportationMode.WALK,
                directionsState.selectedTransportationMode,
            )
            assertEquals(
                TransportationMode.PEDESTRIAN,
                directionsState.routeOptions(UnitSystem.METRIC, avoidNothingOptions())
                    .transportationMode,
            )
            driveUserPathOnMainThread { applicationUserState ->
                applicationUserState.directionsState.selectTransportationMode(
                    DirectionsTransportationMode.CAR,
                )
            }
        }
    }

    /**
     * The ferry-route regression, through the SDK path: with Car selected (the
     * default) a live plan's route request carries AUTOMOBILE, never the old
     * hardcoded pedestrian profile.
     */
    @Test
    fun aCarPlanCarriesTheAutomobileModeToTheSdk() {
        Assume.assumeTrue(
            "the live plan needs the runtime-injected provider key",
            hasRuntimeInjectedApiKey(),
        )
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        assertEquals(
            DirectionsTransportationMode.CAR,
            applicationState.directionsState.selectedTransportationMode,
        )
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.openPlanSearch()
            applicationUserState.destinationSearchText = LIVE_SEARCH_QUERY_TEXT
            applicationUserState.runPlanSearch(
                ReferenceApplicationState.PlanSearchField.DESTINATION,
            )
        }
        awaitOrTimeout(
            timeoutMilliseconds = LIVE_PROVIDER_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the live provider's destination geocode results",
        ) {
            applicationState.searchResults.isNotEmpty()
        }
        val destinationResult = applicationState.searchResults.first()
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.assignSearchResult(destinationResult)
            applicationUserState.originSearchText = LIVE_ORIGIN_SEARCH_QUERY_TEXT
            applicationUserState.runPlanSearch(
                ReferenceApplicationState.PlanSearchField.ORIGIN,
            )
        }
        awaitOrTimeout(
            timeoutMilliseconds = LIVE_PROVIDER_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the live provider's origin geocode results",
        ) {
            applicationState.searchResults.isNotEmpty()
        }
        val originResult = applicationState.searchResults.first()
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.assignSearchResult(originResult)
            applicationUserState.planRouteFromPlanFields()
        }
        awaitOrTimeout(
            timeoutMilliseconds = LIVE_PROVIDER_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the Car plan reaching the SDK",
        ) {
            applicationState.lastPlanRouteOptions != null
        }
        assertEquals(
            "the plan call must carry AUTOMOBILE with Car selected",
            TransportationMode.AUTOMOBILE,
            applicationState.lastPlanRouteOptions?.transportationMode,
        )
        assertEquals(
            RouteStrategy.FASTEST,
            applicationState.lastPlanRouteOptions?.routeStrategy,
        )
    }

    /**
     * The plan action's disabled-with-reason line names the empty field; once
     * both fields hold text the action is enabled, and editing a resolved
     * field's text clears only its assignment because the typed text resolves
     * on press (docs/ReferenceAppUX.md).
     */
    @Test
    fun thePlanActionNamesTheMissingFieldWhileDisabled() {
        Assume.assumeFalse(
            "the keyless demo places answer this flow without a provider",
            hasRuntimeInjectedApiKey(),
        )
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        val recordedRoute = applicationState.artifact.demoRoutes.firstOrNull()
            ?: throw AssertionError("the shared artifact carries no recorded demo route")
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.openPlanSearch(
                ReferenceApplicationState.PlanSearchField.ORIGIN,
            )
            applicationUserState.planRouteFromPlanFields()
        }
        assertFalse(applicationState.canPlanRoute)
        assertNull("a disabled plan action plans nothing", applicationState.plannedRoute)
        assertEquals(
            ReferenceApplicationConstants.PLAN_ROUTE_NEEDS_BOTH_FIELDS_TEXT,
            applicationState.planRouteReasonText,
        )

        // Resolve the origin only: the reason names the destination.
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.originSearchText = recordedRoute.originName
            applicationUserState.runPlanSearch(
                ReferenceApplicationState.PlanSearchField.ORIGIN,
            )
        }
        val originResult = applicationState.searchResults.firstOrNull()
            ?: throw AssertionError("the artifact origin search produced no result")
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.assignSearchResult(originResult)
        }
        assertTrue(applicationState.isOriginResolved)
        assertFalse(applicationState.isDestinationResolved)
        assertFalse(applicationState.canPlanRoute)
        assertEquals(
            ReferenceApplicationConstants.PLAN_ROUTE_NEEDS_DESTINATION_TEXT,
            applicationState.planRouteReasonText,
        )

        // Both resolved: the action enables and states no reason.
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.destinationSearchText = recordedRoute.destinationName
            applicationUserState.runPlanSearch(
                ReferenceApplicationState.PlanSearchField.DESTINATION,
            )
        }
        val destinationResult = applicationState.searchResults.firstOrNull()
            ?: throw AssertionError("the artifact destination search produced no result")
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.assignSearchResult(destinationResult)
        }
        assertTrue(applicationState.canPlanRoute)
        assertEquals("", applicationState.planRouteReasonText)

        // Editing the destination's text away from its assigned place clears
        // the assignment, but the action stays enabled: the typed text is
        // resolved on press rather than requiring another result tap.
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.destinationSearchText = EDITED_DESTINATION_TEXT
        }
        assertFalse(applicationState.isDestinationResolved)
        assertTrue(
            "typed text keeps Plan route enabled",
            applicationState.canPlanRoute,
        )
        assertEquals("", applicationState.planRouteReasonText)
    }

    /**
     * The shared result list assigns to the field whose search produced it,
     * and the sheet states which field the next result fills
     * (docs/ReferenceAppUX.md §4.1).
     */
    @Test
    fun aSearchResultAssignsToTheFieldThatSearched() {
        Assume.assumeFalse(
            "the keyless demo places answer this flow without a provider",
            hasRuntimeInjectedApiKey(),
        )
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        val recordedRoute = applicationState.artifact.demoRoutes.firstOrNull()
            ?: throw AssertionError("the shared artifact carries no recorded demo route")
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.openPlanSearch(
                ReferenceApplicationState.PlanSearchField.ORIGIN,
            )
        }
        assertTrue(
            applicationState.isPlanSearchFieldActive(
                ReferenceApplicationState.PlanSearchField.ORIGIN,
            ),
        )
        assertEquals(
            ReferenceApplicationConstants.SEARCH_ASSIGNMENT_HINT_TEMPLATE.format("origin"),
            applicationState.searchAssignmentHintText,
        )
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.originSearchText = recordedRoute.originName
            applicationUserState.runPlanSearch(
                ReferenceApplicationState.PlanSearchField.ORIGIN,
            )
        }
        val originResult = applicationState.searchResults.firstOrNull()
            ?: throw AssertionError("the artifact origin search produced no result")
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.assignSearchResult(originResult)
        }
        assertEquals(originResult, applicationState.originPlace)
        assertNull(
            "the origin search's result must not fill the destination",
            applicationState.destinationPlace,
        )

        // The destination search takes over the result list and its rows fill
        // the destination field.
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.destinationSearchText = recordedRoute.destinationName
            applicationUserState.runPlanSearch(
                ReferenceApplicationState.PlanSearchField.DESTINATION,
            )
        }
        assertTrue(
            applicationState.isPlanSearchFieldActive(
                ReferenceApplicationState.PlanSearchField.DESTINATION,
            ),
        )
        assertEquals(
            ReferenceApplicationConstants.SEARCH_ASSIGNMENT_HINT_TEMPLATE
                .format("destination"),
            applicationState.searchAssignmentHintText,
        )
        val destinationResult = applicationState.searchResults.firstOrNull()
            ?: throw AssertionError("the artifact destination search produced no result")
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.assignSearchResult(destinationResult)
        }
        assertEquals(destinationResult, applicationState.destinationPlace)
        assertEquals(originResult, applicationState.originPlace)
    }

    @Test
    fun theKeylessPlanFlowSearchesTheArtifactPlacesAndPlansTheRecordedRoute() {
        Assume.assumeFalse(
            "the keyless demo flow needs a credential-less session",
            hasRuntimeInjectedApiKey(),
        )
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        val recordedRoute = applicationState.artifact.demoRoutes.firstOrNull()
            ?: throw AssertionError("the shared artifact carries no recorded demo route")
        // The keyless search filters the artifact's demo places by the typed
        // text and never calls the provider; the rows say they are demo
        // places (§4.1's keyless search).
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.openPlanSearch()
            applicationUserState.originSearchText = recordedRoute.originName
            applicationUserState.runPlanSearch(
                ReferenceApplicationState.PlanSearchField.ORIGIN,
            )
        }
        assertEquals(1, applicationState.searchResults.size)
        assertEquals(recordedRoute.originName, applicationState.searchResults.first().structuredAddressText)
        assertTrue(
            "the keyless result list is the artifact's demo places",
            applicationState.searchResultsAreDemoPlaces,
        )
        assertNull(applicationState.demoSearchNoMatchText)
        val originSearchResult = applicationState.searchResults.first()
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.assignSearchResult(originSearchResult)
            applicationUserState.destinationSearchText = recordedRoute.destinationName
            applicationUserState.runPlanSearch(
                ReferenceApplicationState.PlanSearchField.DESTINATION,
            )
        }
        assertEquals(1, applicationState.searchResults.size)
        assertEquals(
            recordedRoute.destinationName,
            applicationState.searchResults.first().structuredAddressText,
        )
        val destinationSearchResult = applicationState.searchResults.first()
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.assignSearchResult(destinationSearchResult)
        }
        assertNull(
            "assignment alone never plans (the explicit Plan route action does)",
            applicationState.plannedRoute,
        )
        // The explicit Plan route action applies the recorded route without a
        // provider call, and the summary names it as the recorded demo route.
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.planRouteFromPlanFields()
        }
        awaitOrTimeout(
            timeoutMilliseconds = APPLICATION_STATE_CONSTRUCTION_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the keyless recorded-route plan",
        ) {
            applicationState.plannedRoute != null
        }
        val plannedRoute = applicationState.plannedRoute
            ?: throw AssertionError("the keyless plan produced no route")
        assertTrue("the keyless plan is the artifact's recorded route", plannedRoute.isRecordedDemoRoute)
        assertEquals(recordedRoute.route, plannedRoute.route)
        assertNull("a successful keyless flow reports nothing in the error strip", applicationState.errorReport)

        // A pair the artifact does not record refuses with the named reason
        // instead of failing silently (the same place twice has no route).
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.destinationSearchText = recordedRoute.originName
            applicationUserState.runPlanSearch(
                ReferenceApplicationState.PlanSearchField.DESTINATION,
            )
        }
        val originAsDestinationResult = applicationState.searchResults.firstOrNull()
            ?: throw AssertionError("the artifact origin search produced no result")
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.assignSearchResult(originAsDestinationResult)
            applicationUserState.planRouteFromPlanFields()
        }
        awaitOrTimeout(
            timeoutMilliseconds = APPLICATION_STATE_CONSTRUCTION_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the missing recorded-route refusal",
        ) {
            applicationState.errorReport?.detailText != null
        }
        assertEquals(
            ReferenceApplicationConstants.NO_RECORDED_ROUTE_HINT_TEMPLATE
                .format(recordedRoute.originName, recordedRoute.originName),
            applicationState.errorReport?.detailText,
        )
    }

    // ---------------------------------------------------------------
    // Type-ahead (docs/ReferenceAppUX.md §5.5)
    // ---------------------------------------------------------------

    /**
     * The rule's first two clauses: a query shorter than the shared minimum
     * asks nobody, and a keystroke restarts the shared quiet timer so only
     * the query still current when it fires reaches the provider.
     */
    @Test
    fun typeAheadWaitsForTheMinimumLengthThenTheQuietPeriod() {
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        val fixtureProvider = FixtureSearchPlaceProvider(
            resultsForQuery = { queryText -> listOf(fixturePlace(queryText)) },
        )
        driveUserPathOnMainThread { userPathState ->
            userPathState.openPlanSearch(ReferenceApplicationState.PlanSearchField.ORIGIN)
            userPathState.nativeMapSdk.registerSearchPlaceProviderFromForeign(fixtureProvider)
        }

        val belowThresholdQuery = "x".repeat(
            ReferenceAppUXDimensions.AUTOCOMPLETE_MINIMUM_QUERY_CHARACTERS - 1,
        )
        driveUserPathOnMainThread { userPathState ->
            userPathState.onPlanSearchQueryChanged(
                ReferenceApplicationState.PlanSearchField.ORIGIN,
                belowThresholdQuery,
            )
        }
        Thread.sleep(ReferenceAppUXDimensions.AUTOCOMPLETE_DEBOUNCE_MILLISECONDS * 2)
        assertEquals(
            "fewer characters than the shared minimum must ask no provider",
            0,
            fixtureProvider.callCount,
        )
        assertFalse(applicationState.autocompleteViewState(
            ReferenceApplicationState.PlanSearchField.ORIGIN,
        ).isVisible)

        val firstQuery = "a".repeat(ReferenceAppUXDimensions.AUTOCOMPLETE_MINIMUM_QUERY_CHARACTERS)
        driveUserPathOnMainThread { userPathState ->
            userPathState.onPlanSearchQueryChanged(
                ReferenceApplicationState.PlanSearchField.ORIGIN,
                firstQuery,
            )
        }
        Thread.sleep(ReferenceAppUXDimensions.AUTOCOMPLETE_DEBOUNCE_MILLISECONDS / 4)
        assertEquals(
            "no request may leave before the shared quiet period elapses",
            0,
            fixtureProvider.callCount,
        )
        val secondQuery = "b".repeat(ReferenceAppUXDimensions.AUTOCOMPLETE_MINIMUM_QUERY_CHARACTERS)
        driveUserPathOnMainThread { userPathState ->
            userPathState.onPlanSearchQueryChanged(
                ReferenceApplicationState.PlanSearchField.ORIGIN,
                secondQuery,
            )
        }
        Thread.sleep(ReferenceAppUXDimensions.AUTOCOMPLETE_DEBOUNCE_MILLISECONDS / 4)
        assertEquals(
            "a keystroke restarts the quiet timer rather than letting the first query through",
            0,
            fixtureProvider.callCount,
        )
        awaitOrTimeout(
            timeoutMilliseconds = AUTOCOMPLETE_SETTLE_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the debounced type-ahead request",
        ) {
            fixtureProvider.callCount == 1
        }
        assertEquals(
            "only the query still current when the timer fired may ask the provider",
            listOf(secondQuery),
            fixtureProvider.queries,
        )
        awaitOrTimeout(
            timeoutMilliseconds = AUTOCOMPLETE_SETTLE_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the type-ahead suggestion rows",
        ) {
            applicationState.autocompleteViewState(
                ReferenceApplicationState.PlanSearchField.ORIGIN,
            ).suggestions.isNotEmpty()
        }
    }

    /**
     * The shared row cap: a provider answer longer than the cap reaches the
     * field as at most five rows, in the provider's own order.
     */
    @Test
    fun typeAheadShowsAtMostTheSharedRowCap() {
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        val rowCap = ReferenceAppUXDimensions.AUTOCOMPLETE_MAXIMUM_RESULTS
        val fixtureProvider = FixtureSearchPlaceProvider(
            resultsForQuery = { queryText ->
                (1..rowCap + 3).map { rowIndex ->
                    fixturePlace("$queryText suggestion $rowIndex")
                }
            },
        )
        driveUserPathOnMainThread { userPathState ->
            userPathState.openPlanSearch(ReferenceApplicationState.PlanSearchField.DESTINATION)
            userPathState.nativeMapSdk.registerSearchPlaceProviderFromForeign(fixtureProvider)
            userPathState.onPlanSearchQueryChanged(
                ReferenceApplicationState.PlanSearchField.DESTINATION,
                CAP_RESPONSE_QUERY_TEXT,
            )
        }
        awaitOrTimeout(
            timeoutMilliseconds = AUTOCOMPLETE_SETTLE_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the capped type-ahead rows",
        ) {
            applicationState.autocompleteViewState(
                ReferenceApplicationState.PlanSearchField.DESTINATION,
            ).suggestions.size == rowCap
        }
        val viewState = applicationState.autocompleteViewState(
            ReferenceApplicationState.PlanSearchField.DESTINATION,
        )
        assertEquals(
            "the provider's longer answer must be capped at the shared row count",
            rowCap,
            viewState.suggestions.size,
        )
        assertEquals(rowCap, viewState.renderedRowCount)
        assertEquals(
            (1..rowCap).map { rowIndex -> "$CAP_RESPONSE_QUERY_TEXT suggestion $rowIndex" },
            viewState.suggestions.map { place -> place.structuredAddressText },
        )
    }

    /**
     * Rule 4: a new query cancels the request in flight, and a late answer
     * from the superseded query never replaces the newer list.
     */
    @Test
    fun aNewQueryCancelsTheSupersededRequestAndItsLateAnswerIsDropped() {
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        val fixtureProvider = FixtureSearchPlaceProvider(
            resultsForQuery = { queryText -> listOf(fixturePlace("$queryText result")) },
            delayMillisecondsForQuery = { queryText ->
                if (queryText == SUPERSEDED_QUERY_TEXT) {
                    AUTOCOMPLETE_SLOW_FIXTURE_DELAY_MILLISECONDS
                } else {
                    0L
                }
            },
        )
        driveUserPathOnMainThread { userPathState ->
            userPathState.openPlanSearch(ReferenceApplicationState.PlanSearchField.ORIGIN)
            userPathState.nativeMapSdk.registerSearchPlaceProviderFromForeign(fixtureProvider)
            userPathState.onPlanSearchQueryChanged(
                ReferenceApplicationState.PlanSearchField.ORIGIN,
                SUPERSEDED_QUERY_TEXT,
            )
        }
        awaitOrTimeout(
            timeoutMilliseconds = AUTOCOMPLETE_SETTLE_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the superseded query reaching the provider",
        ) {
            fixtureProvider.callCount == 1
        }
        // The first answer is still in the provider's hands; a new query
        // replaces it before it can return.
        driveUserPathOnMainThread { userPathState ->
            userPathState.onPlanSearchQueryChanged(
                ReferenceApplicationState.PlanSearchField.ORIGIN,
                CURRENT_QUERY_TEXT,
            )
        }
        awaitOrTimeout(
            timeoutMilliseconds = AUTOCOMPLETE_SETTLE_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the current query's type-ahead rows",
        ) {
            applicationState.autocompleteViewState(
                ReferenceApplicationState.PlanSearchField.ORIGIN,
            ).suggestions.singleOrNull()?.structuredAddressText == "$CURRENT_QUERY_TEXT result"
        }
        // The superseded provider call finishes after the replacement; its
        // answer must be dropped rather than replacing the current list.
        Thread.sleep(AUTOCOMPLETE_SUPERSEDED_ANSWER_TIMEOUT_MILLISECONDS)
        val settledSuggestions = applicationState.autocompleteViewState(
            ReferenceApplicationState.PlanSearchField.ORIGIN,
        ).suggestions
        assertEquals(
            "the late answer of a superseded query must never reach the list",
            listOf("$CURRENT_QUERY_TEXT result"),
            settledSuggestions.map { place -> place.structuredAddressText },
        )
        assertFalse(
            "a superseded request is cancelled, so the field is idle",
            applicationState.isAutocompleteRequestInFlight(
                ReferenceApplicationState.PlanSearchField.ORIGIN,
            ),
        )
        assertEquals(
            "both queries reached the provider, and only the current one survived",
            listOf(SUPERSEDED_QUERY_TEXT, CURRENT_QUERY_TEXT),
            fixtureProvider.queries,
        )
    }

    /**
     * Rule 5: a chosen suggestion fills the field with its display line,
     * closes the list, and leaves Plan route to resolve the same place the
     * typed-text path would.
     */
    @Test
    fun choosingASuggestionFillsTheFieldClosesTheListAndPlans() {
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        val recordedRoute = applicationState.artifact.demoRoutes.firstOrNull()
            ?: throw AssertionError("the shared artifact carries no recorded demo route")
        val fixtureProvider = FixtureSearchPlaceProvider(
            resultsForQuery = {
                listOf(
                    fixturePlace(recordedRoute.originName),
                    fixturePlace(recordedRoute.destinationName),
                )
            },
        )
        driveUserPathOnMainThread { userPathState ->
            userPathState.openPlanSearch(ReferenceApplicationState.PlanSearchField.ORIGIN)
            userPathState.nativeMapSdk.registerSearchPlaceProviderFromForeign(fixtureProvider)
            userPathState.onPlanSearchQueryChanged(
                ReferenceApplicationState.PlanSearchField.ORIGIN,
                SELECTION_QUERY_TEXT,
            )
        }
        awaitOrTimeout(
            timeoutMilliseconds = AUTOCOMPLETE_SETTLE_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the selection case's suggestion rows",
        ) {
            applicationState.autocompleteViewState(
                ReferenceApplicationState.PlanSearchField.ORIGIN,
            ).suggestions.size == 2
        }
        val chosenSuggestion = applicationState.autocompleteViewState(
            ReferenceApplicationState.PlanSearchField.ORIGIN,
        ).suggestions.first { place ->
            place.structuredAddressText == recordedRoute.originName
        }
        driveUserPathOnMainThread { userPathState ->
            userPathState.selectAutocompleteSuggestion(
                ReferenceApplicationState.PlanSearchField.ORIGIN,
                chosenSuggestion,
            )
        }
        assertEquals(
            "the suggestion's display line fills the field",
            recordedRoute.originName,
            applicationState.originSearchText,
        )
        assertEquals(chosenSuggestion, applicationState.originPlace)
        val closedViewState = applicationState.autocompleteViewState(
            ReferenceApplicationState.PlanSearchField.ORIGIN,
        )
        assertFalse("choosing a suggestion closes the list", closedViewState.isVisible)
        assertTrue(closedViewState.suggestions.isEmpty())
        assertEquals(0, closedViewState.renderedRowCount)

        // The field's text resolves exactly as typed text does: the assigned
        // place stands, the destination's exact demo name resolves, and Plan
        // route applies the artifact's recorded pair.
        driveUserPathOnMainThread { userPathState ->
            userPathState.destinationSearchText = recordedRoute.destinationName
        }
        driveUserPathOnMainThread { userPathState -> userPathState.planRouteFromPlanFields() }
        awaitOrTimeout(
            timeoutMilliseconds = AUTOCOMPLETE_SETTLE_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the plan resolved from the chosen suggestion",
        ) {
            applicationState.plannedRoute != null
        }
        val plannedRoute = applicationState.plannedRoute
            ?: throw AssertionError("the chosen suggestion produced no planned route")
        assertTrue(plannedRoute.isRecordedDemoRoute)
        assertEquals(recordedRoute.route, plannedRoute.route)
    }

    /**
     * The empty and failure states: no matches is one row, never an empty
     * list, and a provider failure keeps the text with one inline sentence
     * that a later keystroke retries; nothing reaches the error strip.
     */
    @Test
    fun typeAheadStatesNoMatchesInOneRowAndReportsAFailureInline() {
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        val fixtureProvider = FixtureSearchPlaceProvider(
            resultsForQuery = { queryText ->
                when (queryText) {
                    FAILING_QUERY_TEXT ->
                        throw uniffi.rustic_maps.SdkException.NetworkRequestFailed(
                            AUTOCOMPLETE_FIXTURE_FAILURE_DETAIL_TEXT,
                        )
                    NO_MATCH_QUERY_TEXT -> emptyList()
                    else -> listOf(fixturePlace("$queryText result"))
                }
            },
        )
        driveUserPathOnMainThread { userPathState ->
            userPathState.openPlanSearch(ReferenceApplicationState.PlanSearchField.ORIGIN)
            userPathState.nativeMapSdk.registerSearchPlaceProviderFromForeign(fixtureProvider)
            userPathState.onPlanSearchQueryChanged(
                ReferenceApplicationState.PlanSearchField.ORIGIN,
                FAILING_QUERY_TEXT,
            )
        }
        awaitOrTimeout(
            timeoutMilliseconds = AUTOCOMPLETE_SETTLE_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the provider failure's inline sentence",
        ) {
            applicationState.autocompleteViewState(
                ReferenceApplicationState.PlanSearchField.ORIGIN,
            ).failureText != null
        }
        val failureViewState = applicationState.autocompleteViewState(
            ReferenceApplicationState.PlanSearchField.ORIGIN,
        )
        assertEquals(FAILING_QUERY_TEXT, applicationState.originSearchText)
        assertEquals(
            AUTOCOMPLETE_FIXTURE_FAILURE_DETAIL_TEXT,
            failureViewState.failureText,
        )
        assertFalse("a failure renders no suggestion list", failureViewState.isVisible)
        assertTrue(failureViewState.suggestions.isEmpty())
        assertNull(
            "a type-ahead failure stays inline and never fills the error strip",
            applicationState.errorReport,
        )

        driveUserPathOnMainThread { userPathState ->
            userPathState.onPlanSearchQueryChanged(
                ReferenceApplicationState.PlanSearchField.ORIGIN,
                NO_MATCH_QUERY_TEXT,
            )
        }
        awaitOrTimeout(
            timeoutMilliseconds = AUTOCOMPLETE_SETTLE_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the one no-match row",
        ) {
            applicationState.autocompleteViewState(
                ReferenceApplicationState.PlanSearchField.ORIGIN,
            ).showsNoMatchRow
        }
        val noMatchViewState = applicationState.autocompleteViewState(
            ReferenceApplicationState.PlanSearchField.ORIGIN,
        )
        assertNull("the retry clears the previous failure sentence", noMatchViewState.failureText)
        assertTrue(noMatchViewState.suggestions.isEmpty())
        assertEquals("no matches is one row, not an empty list", 1, noMatchViewState.renderedRowCount)

        driveUserPathOnMainThread { userPathState ->
            userPathState.onPlanSearchQueryChanged(
                ReferenceApplicationState.PlanSearchField.ORIGIN,
                WORKING_QUERY_TEXT,
            )
        }
        awaitOrTimeout(
            timeoutMilliseconds = AUTOCOMPLETE_SETTLE_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the retry's suggestion row",
        ) {
            applicationState.autocompleteViewState(
                ReferenceApplicationState.PlanSearchField.ORIGIN,
            ).suggestions.isNotEmpty()
        }
        val recoveredViewState = applicationState.autocompleteViewState(
            ReferenceApplicationState.PlanSearchField.ORIGIN,
        )
        assertNull(recoveredViewState.failureText)
        assertFalse(recoveredViewState.showsNoMatchRow)
    }

    /**
     * The keyless side of the provider choice: the application registers the
     * shared registry's credential-less place-search provider at startup, so
     * the demo offers type-ahead before any key is entered.
     */
    @Test
    fun theTypeAheadUsesTheKeylessSearchProviderWithoutACredential() {
        Assume.assumeFalse(
            "the keyless provider choice is asserted only without an injected key",
            hasRuntimeInjectedApiKey(),
        )
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        val placeSearchStorageName = ProviderContract.PLACE_SEARCH.name.lowercase()
        val keylessSearchDescriptor = applicationState.providerRegistry.providers
            .firstOrNull { descriptor ->
                descriptor.isRegistrationAvailableOnAndroid &&
                    descriptor.credentialLabel.isBlank() &&
                    descriptor.demoBaseUrl.isNotBlank() &&
                    placeSearchStorageName in descriptor.servedContracts
            }
        Assume.assumeTrue(
            "the shared registry must declare a credential-less place-search provider",
            keylessSearchDescriptor != null,
        )
        val descriptor = keylessSearchDescriptor
            ?: throw AssertionError("no credential-less place-search provider in the registry")
        assertEquals(
            "the keyless session's type-ahead slot must be the registry's keyless search provider",
            descriptor.providerKey,
            applicationState.autocompleteProviderKey,
        )
        val placeSearchRow = applicationState.capabilityProviderState.rows.firstOrNull { row ->
            row.contract == ProviderContract.PLACE_SEARCH
        }
        assertNotNull("the capability readback must carry a place-search row", placeSearchRow)
        assertTrue(
            "the keyless search provider must be registered, not merely offered",
            placeSearchRow!!.isRegistered,
        )
        assertEquals(descriptor.providerKey, placeSearchRow.servingProviderKey)
    }

    /**
     * The keyed side of the provider choice: a credentialed provider the
     * settings surface's picker selects takes the type-ahead slot from the
     * keyless one. The credential is seeded through the same facade method the
     * settings selection repeats, so no request leaves the device.
     */
    @Test
    fun selectingTheCredentialedSearchProviderMovesTheTypeAheadSlot() {
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        val placeSearchStorageName = ProviderContract.PLACE_SEARCH.name.lowercase()
        val credentialedSearchDescriptor = applicationState.providerRegistry.providers
            .firstOrNull { descriptor ->
                descriptor.isRegistrationAvailableOnAndroid &&
                    descriptor.credentialLabel.isNotBlank() &&
                    placeSearchStorageName in descriptor.servedContracts
            }
        Assume.assumeTrue(
            "the shared registry must declare a credentialed place-search provider",
            credentialedSearchDescriptor != null,
        )
        val descriptor = credentialedSearchDescriptor
            ?: throw AssertionError("no credentialed place-search provider in the registry")
        // Seed the session's credential the way the credential setting does,
        // for the place-search capability only: no raster registration and no
        // provider request is involved.
        driveUserPathOnMainThread { userPathState ->
            userPathState.sdkFacade.configureProvider(
                ProviderContract.PLACE_SEARCH,
                descriptor.providerKey,
                CREDENTIALED_SLOT_FIXTURE_CREDENTIAL,
            )
            userPathState.selectCapabilityProvider(
                ProviderContract.PLACE_SEARCH,
                descriptor.providerKey,
            )
        }
        assertEquals(
            "the credentialed provider's selection must take the type-ahead slot",
            descriptor.providerKey,
            applicationState.autocompleteProviderKey,
        )
        val placeSearchRow = applicationState.capabilityProviderState.rows.first { row ->
            row.contract == ProviderContract.PLACE_SEARCH
        }
        assertTrue(placeSearchRow.isRegistered)
        assertEquals(descriptor.providerKey, placeSearchRow.servingProviderKey)
    }

    @Test
    fun theApplicationStartsKeylessAndTheOfflineCardStaysHonest() {
        Assume.assumeFalse(
            "the keyless state is asserted only without an injected key",
            hasRuntimeInjectedApiKey(),
        )
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        // §6.5: the app ships and starts keyless — no provider is registered
        // from a build-time credential, and the dock says what to do next in
        // neutral product language rather than reporting a renderer failure.
        assertTrue(
            "a keyless start must register no provider",
            applicationState.appliedProviderKeys.isEmpty(),
        )
        assertEquals(
            ReferenceApplicationConstants.KEYLESS_CREDENTIAL_STATUS_TEXT,
            applicationState.credentialStatusLineText,
        )
        // The offline card stays honest: the tile cache registers only after a
        // credential application, so it is simply not registered yet.
        assertNull(applicationState.tileCacheRegisteredPath)
        // A credential-less base map serves uncached, and the card says so
        // (M24); with no base map at all there is nothing to explain.
        val registryCarriesKeylessBaseMap = applicationState.baseMapChoices.any { choice ->
            choice.descriptor.rasterUrlTemplate.isNotBlank()
        }
        if (registryCarriesKeylessBaseMap) {
            assertEquals(
                ReferenceApplicationConstants.BASE_MAP_UNCACHED_NOTE_TEXT,
                applicationState.tileCacheFailureReason,
            )
        } else {
            assertNull(applicationState.tileCacheFailureReason)
        }
        if (registryCarriesKeylessBaseMap) {
            // A credential-less base map serves uncached; the card says that
            // rather than claiming no cache exists for no reason.
            assertEquals(
                ReferenceApplicationConstants.BASE_MAP_UNCACHED_NOTE_TEXT,
                applicationState.offlineCacheStateText,
            )
        } else {
            assertEquals(
                ReferenceApplicationConstants.OFFLINE_MAPS_NOT_REGISTERED_TEXT,
                applicationState.offlineCacheStateText,
            )
            // Without a provider no surface attaches and no frame draws: the
            // tile line states that honestly instead of inventing counts.
            assertEquals(
                ReferenceApplicationConstants.OFFLINE_TILES_PENDING_TEXT,
                applicationState.offlineLiveTileLineText,
            )
        }
        // The gear's own user path opens the settings surface, whose rows are
        // the shared provider registry's descriptors.
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.openSettings()
        }
        assertTrue("the gear must open the settings surface", applicationState.isSettingsOpen)
        assertTrue(
            "the settings rows must come from the shared provider registry's descriptors",
            applicationState.providerCredentialDescriptors.any { descriptor ->
                descriptor.isRegistrationAvailableOnAndroid
            },
        )
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.closeSettings()
        }
        assertFalse(applicationState.isSettingsOpen)
        assertNull("a keyless start reports nothing in the error strip", applicationState.errorReport)
    }

    /**
     * The per-capability settings section: opening the settings sheet through
     * the gear's own user path and choosing a keyless provider for the routing
     * capability from that row's picker makes it the provider the SDK reports
     * serving routing. The provider comes from the shared registry (a keyless
     * descriptor that registers on Android and declares the routing
     * capability), so the case names no vendor and runs keyless.
     */
    @Test
    fun aKeylessRoutingProviderSelectedInSettingsServesTheRoutingCapability() {
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        val keylessRoutingDescriptor =
            applicationState.providerRegistry.providers.firstOrNull { descriptor ->
                descriptor.isRegistrationAvailableOnAndroid &&
                    descriptor.credentialLabel.isBlank() &&
                    "routing" in descriptor.servedContracts
            }
        Assume.assumeTrue(
            "the shared registry must offer a keyless routing provider",
            keylessRoutingDescriptor != null,
        )
        val routingRow = applicationState.capabilityProviderState.rows.firstOrNull { row ->
            row.contract == ProviderContract.ROUTING
        }
        assertNotNull("the capability readback must carry a routing row", routingRow)
        Assume.assumeTrue(
            "the routing row must offer the registry's keyless provider",
            routingRow!!.choices.any { choice ->
                choice.providerKey == keylessRoutingDescriptor!!.providerKey
            },
        )
        val routingDescriptor = keylessRoutingDescriptor
            ?: throw AssertionError("the shared registry offers no keyless routing provider")
        // The gear's own user path opens the settings surface, whose
        // capability section is the readback-driven row list.
        driveUserPathOnMainThread { userPathState -> userPathState.openSettings() }
        assertTrue("the gear must open the settings surface", applicationState.isSettingsOpen)
        driveUserPathOnMainThread { userPathState ->
            userPathState.selectCapabilityProvider(
                ProviderContract.ROUTING,
                routingDescriptor.providerKey,
            )
        }
        val selectedRoutingRow = applicationState.capabilityProviderState.rows.first { row ->
            row.contract == ProviderContract.ROUTING
        }
        if (routingDescriptor.demoBaseUrl.isBlank()) {
            // The descriptor has no public demo address yet: the row refuses
            // honestly instead of registering an empty deployment address, and
            // no provider moves into the routing slot.
            assertFalse(selectedRoutingRow.isRegistered)
            assertEquals(
                ReferenceApplicationConstants.SETTINGS_PROVIDER_NO_DEMO_ENDPOINT_TEMPLATE
                    .format(routingDescriptor.displayName),
                selectedRoutingRow.detailText,
            )
            return
        }
        assertEquals(
            "the routing row composes the provider name and its state",
            routingDescriptor.displayName + " · " +
                ReferenceApplicationConstants.SETTINGS_PROVIDER_STATE_READY_TEXT,
            selectedRoutingRow.servingStatusText,
        )
        assertEquals(
            routingDescriptor.providerKey,
            selectedRoutingRow.servingProviderKey,
        )
        assertEquals(
            "the keyless row states the demo deployment it uses",
            ReferenceApplicationConstants.SETTINGS_ROUTING_DEMO_ENDPOINT_TEMPLATE
                .format(routingDescriptor.demoBaseUrl),
            selectedRoutingRow.demoEndpointText,
        )
        assertNull(
            "an accepted selection reports no refusal",
            selectedRoutingRow.detailText,
        )
    }

    /**
     * The keyed counterpart of the keyless startup: a runtime-injected
     * credential registers the provider over the demo's disk tile cache, and
     * the keyless-base startup must not replace that registration (which
     * silently dropped the cache before the round-43 guard).
     */
    @Test
    fun theKeyedStartKeepsItsProviderRegistrationAndTileCache() {
        Assume.assumeTrue(
            "the keyed registration is asserted only with an injected key",
            hasRuntimeInjectedApiKey(),
        )
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        assertTrue(
            "a keyed start must register the injected provider",
            applicationState.appliedProviderKeys.isNotEmpty(),
        )
        // The cache registration opens its SQLite store on a background
        // dispatcher and publishes the state back on main, so the path is
        // observable a moment after the credential itself applies.
        awaitOrTimeout(
            timeoutMilliseconds = APPLICATION_STATE_CONSTRUCTION_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the credential's disk tile cache registration",
        ) {
            applicationState.tileCacheRegisteredPath != null
        }
        assertNull(
            "a keyed start must not report the keyless uncached note",
            applicationState.tileCacheFailureReason,
        )
        assertNull("a keyed start reports nothing in the error strip", applicationState.errorReport)
    }

    /**
     * The keyless start opens on the registry's declared styled vector base
     * (owner ruling 2026-10-31, PRD §6 item 18): the credential-less raster
     * source still registers so the surface attaches, and the declared style
     * is installed over it before the first draw. The declaration is read from
     * the shared registry, never hardcoded.
     */
    @Test
    fun theKeylessStartOpensOnTheRegistrysDeclaredStyledBase() {
        Assume.assumeFalse(
            "the keyless start is asserted only without an injected key",
            hasRuntimeInjectedApiKey(),
        )
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        val keylessChoice = applicationState.keylessVectorBaseChoice
        Assume.assumeTrue(
            "the shared registry must declare a keyless styled base",
            keylessChoice != null,
        )
        if (keylessChoice == null) {
            return
        }
        awaitOrTimeout(
            timeoutMilliseconds = VECTOR_BASE_SERVE_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the keyless styled base's SDK status to report it serving",
        ) {
            applicationState.activeVectorBase?.choiceKey == keylessChoice.choiceKey
        }
        assertEquals(
            "a keyless start must install the registry's declared styled base",
            keylessChoice.choiceKey,
            applicationState.activeVectorBase?.choiceKey,
        )
        assertEquals(
            "the attribution must be the styled base's own design line",
            keylessChoice.attributionText,
            applicationState.attributionText,
        )
        assertNull(
            "the keyless default styled base reports nothing in the error strip",
            applicationState.errorReport,
        )
    }

    /**
     * The offline card's route control (§2.5 `offline.plan_route_action`): the
     * bundled recorded test graph answers the demo request over the contract's
     * `offlineRouteFromArchiveBytes` free function, and the result is a real
     * planned route with its own distance and duration. No provider credential
     * is involved, so the test runs keyless.
     */
    @Test
    fun theOfflineCardPlansARouteFromTheBundledTestGraph() {
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.planOfflineRouteFromArchive()
        }
        awaitOrTimeout(
            timeoutMilliseconds = OFFLINE_ROUTE_PLAN_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the offline archive route plan",
        ) {
            applicationState.plannedRoute != null || applicationState.errorReport != null
        }
        assertNull(
            "the bundled archive must plan without an error report, was " +
                "${applicationState.errorReport?.variantLabel}/${applicationState.errorReport?.detailText}",
            applicationState.errorReport,
        )
        val plannedRoute = applicationState.plannedRoute
            ?: throw AssertionError("the offline control produced no planned route")
        assertTrue(
            "the planned route must be the offline archive's answer",
            plannedRoute.isOfflineArchiveRoute,
        )
        assertFalse(
            "the offline archive route is not the artifact's recorded demo route",
            plannedRoute.isRecordedDemoRoute,
        )
        assertTrue(
            "the offline route must carry its distance, was ${plannedRoute.route.distanceMeters}",
            plannedRoute.route.distanceMeters > 0.0,
        )
        assertTrue(
            "the offline route must carry its duration, was ${plannedRoute.route.durationSeconds}",
            plannedRoute.route.durationSeconds > 0.0,
        )
        assertEquals(
            ReferenceApplicationConstants.OFFLINE_ROUTE_TITLE_TEXT,
            plannedRoute.nameText,
        )
    }

    @Test
    fun theDragPanMovesTheMapContentWithThePointer() {
        Assume.assumeTrue(
            "the pan needs a drawn surface, so it needs the runtime-injected key",
            hasRuntimeInjectedApiKey(),
        )
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        // The pan's user-path method unprojects through the drawn camera,
        // so the first drawn frame must have settled first.
        awaitOrTimeout(
            timeoutMilliseconds = FIRST_FRAME_DRAWN_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the first drawn frame before the pan",
        ) {
            applicationState.framesDrawnCount > 0
        }
        val prePanCameraValues = readApplicationCameraValues(applicationState)

        // The pan gestures' own user-path method (§5.2's drag-pan): a
        // rightward drag must move the content right — the camera center
        // wests, so the longitude drops — and the ground point under the
        // pointer sticks to it, so the latitude barely moves. The launch
        // camera is top-down and north up (the artifact's fixture-center
        // preset), so screen east is longitude east throughout.
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.panByPixels(PAN_DRAG_RIGHTWARD_PIXELS, 0.0)
        }
        val cameraAfterRightwardPan = readApplicationCameraValues(applicationState)
        assertTrue(
            "a rightward drag must move the content right (the camera center wests), " +
                "was ${cameraAfterRightwardPan.centerLongitudeDegrees}",
            cameraAfterRightwardPan.centerLongitudeDegrees <
                prePanCameraValues.centerLongitudeDegrees - PAN_MOVE_DEGREES_MINIMUM,
        )
        assertEquals(
            "a pure-rightward drag must not move the camera center north or south",
            prePanCameraValues.centerLatitudeDegrees,
            cameraAfterRightwardPan.centerLatitudeDegrees,
            PAN_STANDSTILL_TOLERANCE_DEGREES,
        )

        // The vertical axis: a downward drag moves the content down — the
        // camera center norths, so the latitude rises — with the longitude
        // barely moving.
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.panByPixels(0.0, PAN_DRAG_DOWNWARD_PIXELS)
        }
        val cameraAfterDownwardPan = readApplicationCameraValues(applicationState)
        assertTrue(
            "a downward drag must move the content down (the camera center norths), " +
                "was ${cameraAfterDownwardPan.centerLatitudeDegrees}",
            cameraAfterDownwardPan.centerLatitudeDegrees >
                cameraAfterRightwardPan.centerLatitudeDegrees + PAN_MOVE_DEGREES_MINIMUM,
        )
        assertEquals(
            "a pure-downward drag must not move the camera center east or west",
            cameraAfterRightwardPan.centerLongitudeDegrees,
            cameraAfterDownwardPan.centerLongitudeDegrees,
            PAN_STANDSTILL_TOLERANCE_DEGREES,
        )
    }

    /**
     * A camera animation in flight is cancelled by a gesture (§5.2/§5.3): the
     * view's action-down takes over the camera intent, so the animation stops
     * advancing where it stands and never reaches its target.
     *
     * The animation is the shipped `animateCamera` verb driven to a pose far
     * from the launch camera; the cancel is a real pointer tap injected into
     * the view, the same event a visitor's finger delivers. The camera values
     * are the ones the screen renders, so this pins the wrapper's gesture-
     * versus-animation rule on a real surface, which the JVM unit suite's stub
     * `android.jar` cannot drive.
     */
    @Test
    fun aGestureCancelsARunningCameraAnimation() {
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        Assume.assumeTrue(
            "the animation needs the shipped MapView and a drawn surface",
            applicationState.mapView != null,
        )
        // The animation advances inside the frame pump, so the surface must be
        // attached and drawn first (the keyless raster base serves it).
        awaitOrTimeout(
            timeoutMilliseconds = FIRST_FRAME_DRAWN_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the first drawn frame before the camera animation",
        ) {
            applicationState.framesDrawnCount > 0
        }

        val startLongitudeDegrees = applicationState.centerLongitudeDegrees
        val targetLongitudeDegrees =
            startLongitudeDegrees + CAMERA_ANIMATION_TARGET_LONGITUDE_OFFSET_DEGREES
        driveUserPathOnMainThread { applicationUserState ->
            val mapView = applicationUserState.mapView ?: return@driveUserPathOnMainThread
            mapView.animateCamera(
                MapCameraPose(
                    latitudeDegrees = applicationUserState.centerLatitudeDegrees,
                    longitudeDegrees = targetLongitudeDegrees,
                    zoomLevel = applicationUserState.zoomLevel,
                    bearingDegrees = applicationUserState.bearingDegrees,
                    pitchDegrees = applicationUserState.pitchDegrees,
                ),
                CAMERA_ANIMATION_DURATION_MILLISECONDS,
            )
        }

        // The animation is genuinely running once the rendered center has
        // advanced toward the target; a no-op animation fails here rather than
        // passing vacuously below.
        awaitOrTimeout(
            timeoutMilliseconds = CAMERA_ANIMATION_PROGRESS_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the running camera animation's first progress",
        ) {
            applicationState.centerLongitudeDegrees >
                startLongitudeDegrees + CAMERA_ANIMATION_PROGRESS_MINIMUM_DEGREES
        }

        // One real pointer tap: the view's action-down cancels the animation.
        injectMapTaps(tapCount = SINGLE_TAP_COUNT)

        // Let the frame already in flight land, then sample the pose twice: an
        // animation that kept running would advance between the samples.
        Thread.sleep(CAMERA_ANIMATION_CANCEL_SETTLE_MILLISECONDS)
        val cancelledLongitudeDegrees = applicationState.centerLongitudeDegrees
        Thread.sleep(CAMERA_ANIMATION_STILLNESS_MILLISECONDS)
        assertEquals(
            "a gesture must stop the running animation where it stands",
            cancelledLongitudeDegrees,
            applicationState.centerLongitudeDegrees,
            CAMERA_ANIMATION_STILLNESS_TOLERANCE_DEGREES,
        )
        assertTrue(
            "a cancelled animation must never reach its target, " +
                "was ${applicationState.centerLongitudeDegrees} of $targetLongitudeDegrees",
            targetLongitudeDegrees - applicationState.centerLongitudeDegrees >
                CAMERA_ANIMATION_TARGET_REMAINING_MINIMUM_DEGREES,
        )
    }

    /**
     * The tap contract on the Compose-hosted shipped MapView (§5.2): one
     * confirmed single tap reports exactly one map click and leaves the camera
     * alone, while one double tap zooms exactly the app's one-level step
     * around the tapped point and reports no click of its own (the view's
     * confirmed-single-tap rule). The taps are real pointer events injected at
     * the map's screen point; the suite reads the values the diagnostics
     * sheet renders.
     */
    @Test
    fun aSingleTapReportsOnceAndADoubleTapZoomsExactlyOneLevel() {
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        // The gesture contract is base-agnostic, so the keyless run reads it on
        // the credential-less raster base: the emulator's software renderer
        // draws that layer at a stable rate, whereas the styled vector base's
        // feature count makes the injected pointer events queue behind frames.
        if (!hasRuntimeInjectedApiKey()) {
            val framesDrawnBeforeBaseSelection = applicationState.framesDrawnCount
            val rasterChoice = applicationState.baseMapChoices.firstOrNull { choice ->
                choice.isServable && choice.descriptor.rasterUrlTemplate.isNotBlank()
            }
            Assume.assumeTrue(
                "the shared registry must declare a credential-less raster base",
                rasterChoice != null,
            )
            if (rasterChoice != null) {
                driveUserPathOnMainThread { applicationUserState ->
                    applicationUserState.selectBaseMap(rasterChoice.providerKey)
                }
                awaitOrTimeout(
                    timeoutMilliseconds = FIRST_FRAME_DRAWN_TIMEOUT_MILLISECONDS,
                    timeoutDescription = "a drawn frame on the raster base before the tap gestures",
                ) {
                    applicationState.activeVectorBase == null &&
                        applicationState.framesDrawnCount > framesDrawnBeforeBaseSelection
                }
            }
        }
        // The tap unprojects through the drawn camera and the double tap needs
        // a viewport, so the first frame must have settled. The keyless base
        // map serves it, so this runs without a provider credential.
        awaitOrTimeout(
            timeoutMilliseconds = FIRST_FRAME_DRAWN_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the first drawn frame before the tap gestures",
        ) {
            applicationState.framesDrawnCount > 0
        }
        val initialZoomLevel = applicationState.zoomLevel
        val initialClickCount = applicationState.mapClickCount

        // One tap: exactly one click, no camera move.
        injectMapTaps(tapCount = SINGLE_TAP_COUNT)
        awaitOrTimeout(
            timeoutMilliseconds = TAP_CONFIRMATION_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the confirmed single tap's one map click",
        ) {
            applicationState.mapClickCount == initialClickCount + 1
        }
        assertEquals(
            "a single tap must not move the camera",
            initialZoomLevel,
            applicationState.zoomLevel,
            CAMERA_ZOOM_TOLERANCE,
        )

        // A double tap at the same point zooms exactly one level, and its
        // first tap is not also reported as a click.
        injectMapTaps(tapCount = DOUBLE_TAP_COUNT)
        awaitOrTimeout(
            timeoutMilliseconds = TAP_CONFIRMATION_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the double tap's one-level zoom",
        ) {
            kotlin.math.abs(
                applicationState.zoomLevel -
                    (initialZoomLevel + ReferenceAppUXDimensions.MAP_CONTROL_ZOOM_STEP),
            ) <= CAMERA_ZOOM_TOLERANCE
        }
        assertEquals(
            "a double tap must zoom exactly the app's one-level step",
            initialZoomLevel + ReferenceAppUXDimensions.MAP_CONTROL_ZOOM_STEP,
            applicationState.zoomLevel,
            CAMERA_ZOOM_TOLERANCE,
        )
        // The confirmed-single-tap window has passed: a late click from the
        // double tap's first touch would have landed by now.
        Thread.sleep(TAP_CONFIRMATION_TIMEOUT_MILLISECONDS)
        assertEquals(
            "one double tap must not also report a map click",
            initialClickCount + SINGLE_TAP_COUNT,
            applicationState.mapClickCount,
        )
        assertNull("the tap gestures report nothing in the error strip", applicationState.errorReport)
    }

    @Test
    fun selectingACommittedVectorStyleServesTheStyledBaseAndStatesTheLabelLimit() {
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        val vectorChoice = applicationState.vectorBaseMapChoices.firstOrNull()
        Assume.assumeTrue(
            "the shared registry must offer a vector tileset with committed styles",
            vectorChoice != null,
        )
        if (vectorChoice == null) {
            return
        }
        Assume.assumeTrue(
            "installing a vector base needs the shipped MapView",
            applicationState.mapView != null,
        )
        // The row's user-path act: the SDK validates the source, so a refusal
        // would leave no selection recorded and this test would fail honestly.
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.selectVectorBase(vectorChoice.choiceKey)
        }
        // The install can be queued behind an in-flight draw; the SDK's own
        // status readback is the authority on what serves, not the request.
        awaitOrTimeout(
            timeoutMilliseconds = VECTOR_BASE_SERVE_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the styled vector base's SDK status to report it serving",
        ) {
            applicationState.activeVectorBase?.choiceKey == vectorChoice.choiceKey
        }
        assertEquals(
            "the layers panel row must name the base the SDK reports serving (error: " +
                "${applicationState.errorReport?.variantLabel}/${applicationState.errorReport?.detailText})",
            vectorChoice.choiceKey,
            applicationState.activeVectorBase?.choiceKey,
        )
        val vectorStatus = applicationState.vectorBaseStatus
        assertEquals(
            "the SDK must report the selected style's source and style id",
            vectorChoice.descriptor.providerKey,
            vectorStatus?.sourceName,
        )
        assertEquals(
            "the SDK must report the selected committed style",
            vectorChoice.style.styleId,
            vectorStatus?.styleId,
        )
        // The drawn base agrees with the row: a frame with the styled vector
        // base requests no raster tiles, because the raster layer is hidden
        // while the vector base serves. Before the fix this stayed at the last
        // raster frame's counts under a row naming the vector style, which is
        // the perception bug this test pins.
        awaitOrTimeout(
            timeoutMilliseconds = VECTOR_BASE_SERVE_TIMEOUT_MILLISECONDS,
            timeoutDescription = "a frame drawn with the raster layer hidden under the vector base",
        ) {
            applicationState.latestTileStatistics?.requestedTileCount?.toInt() == 0
        }
        assertEquals(
            "a vector-base frame must request no raster tiles",
            0,
            applicationState.latestTileStatistics?.requestedTileCount?.toInt(),
        )
        assertEquals(
            "a vector-base frame must draw no raster tiles",
            0,
            applicationState.latestTileStatistics?.presentTileCount?.toInt(),
        )
        assertEquals(
            "the attribution must be the active style's own design line",
            vectorChoice.attributionText,
            applicationState.attributionText,
        )
        assertEquals(
            "the base-map section must state the labels limit",
            ReferenceApplicationConstants.BASE_MAP_VECTOR_LABELS_NOTE_TEXT,
            applicationState.vectorBaseLabelsNote,
        )
        // Leave the raster base serving again so the following tests start
        // from the suite's documented idle base map.
        val rasterChoice = applicationState.baseMapChoices.firstOrNull { it.isServable }
        if (rasterChoice != null) {
            driveUserPathOnMainThread { applicationUserState ->
                applicationUserState.selectBaseMap(rasterChoice.providerKey)
            }
            assertNull(
                "a raster selection must remove the vector base",
                applicationState.activeVectorBase,
            )
        }
    }

    /**
     * The M49 native proof: clearing the styled vector base while a draw is
     * under way returns the raster layer on the very next frame. The pan
     * schedules a frame and the raster selection follows in the same
     * main-thread turn, so the clear lands behind the in-flight draw (the
     * shipped view's deferred-clear rule); the next drawn frame must request
     * and draw raster tiles again and the SDK must report no vector source.
     */
    @Test
    fun clearingTheVectorBaseBehindADrawReturnsTheRasterOnTheNextFrame() {
        Assume.assumeTrue(
            "the raster return needs drawn tiles, so it needs the runtime-injected key",
            hasRuntimeInjectedApiKey(),
        )
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        val vectorChoice = applicationState.vectorBaseMapChoices.firstOrNull()
        Assume.assumeTrue(
            "the shared registry must offer a vector tileset with committed styles",
            vectorChoice != null,
        )
        Assume.assumeTrue(
            "clearing a vector base needs the shipped MapView",
            applicationState.mapView != null,
        )
        val rasterChoice = applicationState.baseMapChoices.firstOrNull { it.isServable }
        Assume.assumeTrue(
            "the session must offer a servable raster base to return to",
            rasterChoice != null,
        )
        if (vectorChoice == null || rasterChoice == null) {
            return
        }

        // Serve the vector base and let a frame draw it: the raster layer is
        // hidden, so the frame requests no raster tiles.
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.selectVectorBase(vectorChoice.choiceKey)
        }
        awaitOrTimeout(
            timeoutMilliseconds = VECTOR_BASE_SERVE_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the styled vector base's SDK status to report it serving",
        ) {
            applicationState.activeVectorBase?.choiceKey == vectorChoice.choiceKey &&
                applicationState.latestTileStatistics?.requestedTileCount?.toInt() == 0
        }
        val framesDrawnBeforeClear = applicationState.framesDrawnCount

        // Schedule a frame with a pan and clear the base in the same turn: the
        // clear follows the in-flight draw instead of interleaving with it.
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.panByPixels(PAN_DRAG_RIGHTWARD_PIXELS, 0.0)
            applicationUserState.selectBaseMap(rasterChoice.providerKey)
        }
        awaitOrTimeout(
            timeoutMilliseconds = FIRST_FRAME_DRAWN_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the frame after the clear requesting raster tiles again",
        ) {
            applicationState.framesDrawnCount > framesDrawnBeforeClear &&
                (applicationState.latestTileStatistics?.requestedTileCount?.toInt() ?: 0) > 0
        }
        assertTrue(
            "the frame after the clear must request raster tiles, was " +
                "${applicationState.latestTileStatistics?.requestedTileCount}",
            (applicationState.latestTileStatistics?.requestedTileCount?.toInt() ?: 0) > 0,
        )
        assertNull(
            "the SDK must report no vector source once the base is cleared",
            applicationState.vectorBaseStatus?.takeIf { it.isConfigured },
        )
        assertNull(
            "the raster selection must remove the vector base",
            applicationState.activeVectorBase,
        )
        awaitOrTimeout(
            timeoutMilliseconds = FIRST_FRAME_DRAWN_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the raster layer drawing its tiles again",
        ) {
            (applicationState.latestTileStatistics?.presentTileCount?.toInt() ?: 0) > 0
        }
        assertTrue(
            "the raster layer must draw tiles again, was " +
                "${applicationState.latestTileStatistics?.presentTileCount}",
            (applicationState.latestTileStatistics?.presentTileCount?.toInt() ?: 0) > 0,
        )
        assertNull("the clear leaves no failure in the error strip", applicationState.errorReport)
    }

    /**
     * The M49 vector → vector step: a second committed style of the same
     * tileset takes the base, and the SDK's status readback moves to that
     * style instead of naming the previous one.
     */
    @Test
    fun switchingBetweenCommittedVectorStylesMovesTheServingBase() {
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        val tilesetChoice = applicationState.vectorBaseMapChoices.firstOrNull()
        Assume.assumeTrue(
            "the shared registry must offer a vector tileset with committed styles",
            tilesetChoice != null,
        )
        Assume.assumeTrue(
            "switching styles needs the shipped MapView",
            applicationState.mapView != null,
        )
        if (tilesetChoice == null) {
            return
        }
        val styleChoices = applicationState.vectorBaseMapChoices.filter { choice ->
            choice.descriptor.providerKey == tilesetChoice.descriptor.providerKey
        }
        Assume.assumeTrue(
            "the tileset must offer two committed styles to switch between",
            styleChoices.size >= 2,
        )
        if (styleChoices.size < 2) {
            return
        }
        val firstStyle = styleChoices[0]
        val secondStyle = styleChoices[1]

        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.selectVectorBase(firstStyle.choiceKey)
        }
        awaitOrTimeout(
            timeoutMilliseconds = VECTOR_BASE_SERVE_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the first committed style serving",
        ) {
            applicationState.activeVectorBase?.choiceKey == firstStyle.choiceKey
        }

        // Vector → vector: the second style takes the base, and the readback
        // names only the new style.
        driveUserPathOnMainThread { applicationUserState ->
            applicationUserState.selectVectorBase(secondStyle.choiceKey)
        }
        awaitOrTimeout(
            timeoutMilliseconds = VECTOR_BASE_SERVE_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the second committed style serving",
        ) {
            applicationState.activeVectorBase?.choiceKey == secondStyle.choiceKey
        }
        assertEquals(
            "the status readback must name the serving style, not the previous one",
            secondStyle.style.styleId,
            applicationState.vectorBaseStatus?.styleId,
        )
        assertEquals(
            "the serving style's tileset must stay the configured source",
            secondStyle.descriptor.providerKey,
            applicationState.vectorBaseStatus?.sourceName,
        )

        // Leave the raster base serving again so the following tests start
        // from the suite's documented idle base map.
        val rasterChoice = applicationState.baseMapChoices.firstOrNull { it.isServable }
        if (rasterChoice != null) {
            driveUserPathOnMainThread { applicationUserState ->
                applicationUserState.selectBaseMap(rasterChoice.providerKey)
            }
            assertNull(
                "a raster selection must remove the vector base",
                applicationState.vectorBaseStatus?.takeIf { it.isConfigured },
            )
        }
    }

    /**
     * The explicit choices survive a relaunch: the theme, the base map and
     * the plan fields are restored from the versioned preferences record,
     * and the remembered pair fills the fields without planning a route.
     */
    @Test
    fun rememberedChoicesSurviveActivityRecreation() {
        clearRememberedChoices()
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        // A credentialed family without its key is listed but cannot
        // install, so the switch tests pick a servable choice.
        val vectorChoice = applicationState.vectorBaseMapChoices.firstOrNull { choice ->
            choice.isServable &&
                choice.choiceKey != applicationState.activeVectorBase?.choiceKey
        }
        Assume.assumeTrue(
            "the registry must offer a vector base different from the startup default",
            vectorChoice != null,
        )
        if (vectorChoice == null) {
            return
        }

        driveUserPathOnMainThread { userPathState -> userPathState.cycleThemeChoice() }
        assertEquals(ThemeChoice.LIGHT, applicationState.themeChoice)
        // The Directions choices are remembered with the rest of the record
        // when the serving provider offers them (a keyless session disables
        // them, and the defaults are what a relaunch restores).
        driveUserPathOnMainThread { userPathState ->
            // Open the plan surface so the capability read has run: a keyless
            // session then cannot select the disabled choices.
            userPathState.openPlanSearch()
            if (userPathState.directionsState.isTransportationModeAvailable(
                    DirectionsTransportationMode.WALK,
                )
            ) {
                userPathState.directionsState.selectTransportationMode(
                    DirectionsTransportationMode.WALK,
                )
            }
            if (userPathState.directionsState.isShortestRouteStrategyAvailable()) {
                userPathState.directionsState.selectRouteStrategy(
                    DirectionsRouteStrategy.SHORTEST,
                )
            }
        }
        val expectedDirectionsTransportationMode =
            applicationState.directionsState.selectedTransportationMode
        val expectedDirectionsRouteStrategy =
            applicationState.directionsState.selectedRouteStrategy
        driveUserPathOnMainThread { userPathState ->
            userPathState.selectVectorBase(vectorChoice.choiceKey)
        }
        awaitOrTimeout(
            timeoutMilliseconds = VECTOR_BASE_SERVE_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the selected vector base to be recorded",
        ) {
            applicationState.selectedVectorBaseChoiceKey == vectorChoice.choiceKey
        }
        driveUserPathOnMainThread { userPathState ->
            userPathState.originSearchText = REMEMBERED_ORIGIN_TEXT
            userPathState.destinationSearchText = REMEMBERED_DESTINATION_TEXT
            userPathState.runPlanSearch(ReferenceApplicationState.PlanSearchField.DESTINATION)
        }
        assertEquals(REMEMBERED_DESTINATION_TEXT, applicationState.lastSearchQueryText)

        val recreatedActivity = recreateApplicationAndAwaitStateConstruction()
        val restoredState = awaitSettledState(recreatedActivity)
        assertEquals(ThemeChoice.LIGHT, restoredState.themeChoice)
        assertEquals(
            "the remembered Directions mode must survive the relaunch",
            expectedDirectionsTransportationMode,
            restoredState.directionsState.selectedTransportationMode,
        )
        assertEquals(
            "the remembered Directions strategy must survive the relaunch",
            expectedDirectionsRouteStrategy,
            restoredState.directionsState.selectedRouteStrategy,
        )
        awaitOrTimeout(
            timeoutMilliseconds = VECTOR_BASE_SERVE_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the remembered vector base to be installed again",
        ) {
            restoredState.selectedVectorBaseChoiceKey == vectorChoice.choiceKey
        }
        assertEquals(REMEMBERED_ORIGIN_TEXT, restoredState.originSearchText)
        assertEquals(REMEMBERED_DESTINATION_TEXT, restoredState.destinationSearchText)
        assertEquals(REMEMBERED_DESTINATION_TEXT, restoredState.lastSearchQueryText)
        assertNull(
            "a remembered plan pair fills the fields but never plans a route",
            restoredState.plannedRoute,
        )
        assertNull("the restore reports nothing in the error strip", restoredState.errorReport)
    }

    /**
     * The M41 proof: a vector style switch, the traffic overlay scene and a
     * centered device fix all return after the activity is recreated. The
     * style and scene choices come from the versioned record, and the device
     * source restarts only because the record remembered it.
     *
     * The emulator must carry a location before this case runs
     * (`adb emu geo fix -121.4512 38.6012`); a run without a fix skips
     * honestly instead of asserting a position the device never produced.
     */
    @Test
    fun theStyleTrafficAndCenteredDeviceFixSurviveActivityRecreation() {
        clearRememberedChoices()
        InstrumentationRegistry.getInstrumentation().uiAutomation.grantRuntimePermission(
            ApplicationProvider.getApplicationContext<android.content.Context>().packageName,
            Manifest.permission.ACCESS_FINE_LOCATION,
        )
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        // A credentialed family without its key is listed but cannot
        // install, so the switch tests pick a servable choice.
        val vectorChoice = applicationState.vectorBaseMapChoices.firstOrNull { choice ->
            choice.isServable &&
                choice.choiceKey != applicationState.activeVectorBase?.choiceKey
        }
        Assume.assumeTrue(
            "the registry must offer a vector base different from the startup default",
            vectorChoice != null,
        )
        if (vectorChoice == null) {
            return
        }

        // The style switch the launch default does not already serve.
        driveUserPathOnMainThread { userPathState ->
            userPathState.selectVectorBase(vectorChoice.choiceKey)
        }
        awaitOrTimeout(
            timeoutMilliseconds = VECTOR_BASE_SERVE_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the selected vector base to serve",
        ) {
            applicationState.activeVectorBase?.choiceKey == vectorChoice.choiceKey
        }

        // The artifact's traffic overlay scene, the fixture-backed traffic
        // layer both native targets carry.
        driveUserPathOnMainThread { userPathState ->
            userPathState.setSceneToggle(TRAFFIC_SCENE_KEY, true)
        }
        awaitOrTimeout(
            timeoutMilliseconds = OVERLAY_PUSH_SETTLE_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the traffic scene overlay to push",
        ) {
            applicationState.sceneToggleStates[TRAFFIC_SCENE_KEY] == true &&
                applicationState.pushedOverlayCount >=
                PLANNED_ROUTE_LINE_OVERLAY_COUNT + SCENE_OVERLAY_COUNT
        }
        val trafficOverlayCount = applicationState.pushedOverlayCount

        // The round control's act: start the source, which centres the camera
        // on its first accepted fix.
        driveUserPathOnMainThread { userPathState -> userPathState.startDeviceLocation() }
        Assume.assumeTrue(
            "the SDK location source is available on this emulator",
            applicationState.isDeviceLocationActive,
        )
        val fixDeadline =
            SystemClock.elapsedRealtime() + DEVICE_LOCATION_FIX_TIMEOUT_MILLISECONDS
        while (applicationState.latestLocationFix == null &&
            SystemClock.elapsedRealtime() < fixDeadline
        ) {
            Thread.sleep(READINESS_POLL_INTERVAL_MILLISECONDS)
        }
        Assume.assumeTrue(
            "the emulator must provide a location fix",
            applicationState.latestLocationFix != null,
        )
        val fixCoordinate = applicationState.latestLocationFix?.snappedCoordinate
            ?: throw AssertionError("the accepted fix carries no coordinate")
        assertEquals(
            "the active source centres the camera on its fix",
            fixCoordinate.latitude,
            applicationState.centerLatitudeDegrees,
            DEVICE_CENTER_TOLERANCE_DEGREES,
        )
        assertEquals(
            fixCoordinate.longitude,
            applicationState.centerLongitudeDegrees,
            DEVICE_CENTER_TOLERANCE_DEGREES,
        )
        assertEquals(
            "the device-location choice joins the remembered record",
            true,
            readRememberedChoices()?.deviceLocationEnabled,
        )

        val recreatedActivity = recreateApplicationAndAwaitStateConstruction()
        val restoredState = awaitSettledState(recreatedActivity)
        awaitOrTimeout(
            timeoutMilliseconds = VECTOR_BASE_SERVE_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the remembered vector base to serve again",
        ) {
            restoredState.activeVectorBase?.choiceKey == vectorChoice.choiceKey
        }
        assertEquals(
            "the traffic scene switch restores from the record",
            true,
            restoredState.sceneToggleStates[TRAFFIC_SCENE_KEY],
        )
        awaitOrTimeout(
            timeoutMilliseconds = OVERLAY_PUSH_SETTLE_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the restored traffic scene overlay to push",
        ) {
            restoredState.pushedOverlayCount == trafficOverlayCount
        }
        awaitOrTimeout(
            timeoutMilliseconds = DEVICE_LOCATION_FIX_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the remembered device source to centre on a fix",
        ) {
            restoredState.isDeviceLocationActive &&
                restoredState.latestLocationFix != null &&
                kotlin.math.abs(
                    restoredState.centerLatitudeDegrees - fixCoordinate.latitude,
                ) < DEVICE_CENTER_TOLERANCE_DEGREES &&
                kotlin.math.abs(
                    restoredState.centerLongitudeDegrees - fixCoordinate.longitude,
                ) < DEVICE_CENTER_TOLERANCE_DEGREES
        }
    }

    /** The reset row clears the record and the next launch returns to defaults. */
    @Test
    fun resetRememberedChoicesClearsTheRecordAndReturnsToDefaults() {
        clearRememberedChoices()
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        driveUserPathOnMainThread { userPathState -> userPathState.cycleThemeChoice() }
        driveUserPathOnMainThread { userPathState ->
            userPathState.originSearchText = REMEMBERED_ORIGIN_TEXT
            userPathState.destinationSearchText = REMEMBERED_DESTINATION_TEXT
        }
        assertNotNull(readRememberedChoices())

        driveUserPathOnMainThread { userPathState -> userPathState.resetRememberedChoices() }
        assertNull("the reset row clears the whole record", readRememberedChoices())
        assertEquals(ThemeChoice.AUTO, applicationState.themeChoice)
        assertEquals("", applicationState.originSearchText)
        assertEquals("", applicationState.destinationSearchText)

        val recreatedActivity = recreateApplicationAndAwaitStateConstruction()
        val restoredState = awaitSettledState(recreatedActivity)
        assertEquals(ThemeChoice.AUTO, restoredState.themeChoice)
        assertEquals("", restoredState.originSearchText)
        assertEquals("", restoredState.destinationSearchText)
        assertNull(restoredState.plannedRoute)
    }

    /**
     * The M40 proof on Android: the settings routing row selects the shared
     * registry's keyless routing provider, the remembered choice survives a
     * relaunch, and a fixture-backed routing provider registered from the
     * test answers the plan (the same foreign-registration seam the
     * type-ahead cases use, returning the shared artifact's committed demo
     * route). The row is asserted before the fixture replaces the SDK slot,
     * so the remembered choice is the provider the readback names; the plan's
     * route and the fixture's recorded call are the successful plan's
     * evidence. No network is touched.
     */
    @Test
    fun aRememberedKeylessRoutingChoicePlansFromTheFixtureAfterRelaunch() {
        clearRememberedChoices()
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        val keylessRoutingDescriptor =
            applicationState.providerRegistry.providers.firstOrNull { descriptor ->
                descriptor.isRegistrationAvailableOnAndroid &&
                    descriptor.credentialLabel.isBlank() &&
                    "routing" in descriptor.servedContracts
            }
        Assume.assumeTrue(
            "the shared registry must offer a keyless routing provider",
            keylessRoutingDescriptor != null,
        )
        val routingDescriptor = keylessRoutingDescriptor
            ?: throw AssertionError("the shared registry offers no keyless routing provider")
        val recordedRoute = applicationState.artifact.demoRoutes.firstOrNull()
            ?: throw AssertionError("the shared artifact carries no recorded demo route")

        // The gear's own user path opens the settings surface; the routing
        // row's picker is the same act the screen calls.
        driveUserPathOnMainThread { userPathState -> userPathState.openSettings() }
        driveUserPathOnMainThread { userPathState ->
            userPathState.selectCapabilityProvider(
                ProviderContract.ROUTING,
                routingDescriptor.providerKey,
            )
        }
        val selectedRoutingRow = applicationState.capabilityProviderState.rows.first { row ->
            row.contract == ProviderContract.ROUTING
        }
        assertTrue(
            "the selected keyless provider must register the routing slot",
            selectedRoutingRow.isRegistered,
        )
        assertEquals(
            routingDescriptor.providerKey,
            selectedRoutingRow.servingProviderKey,
        )

        // The fixture-backed registration seam: a routing provider written in
        // the test answers with the shared artifact's committed route, so the
        // app's normal plan path runs without a provider request leaving the
        // device.
        val firstFixtureProvider = FixtureRoutingProvider(recordedRoute.route)
        driveUserPathOnMainThread { userPathState ->
            userPathState.nativeMapSdk.registerRoutingProviderFromForeign(
                firstFixtureProvider,
            )
            userPathState.openPlanSearch()
            userPathState.originSearchText = recordedRoute.originName
            userPathState.destinationSearchText = recordedRoute.destinationName
            userPathState.planRouteFromPlanFields()
        }
        awaitOrTimeout(
            timeoutMilliseconds = APPLICATION_STATE_CONSTRUCTION_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the fixture-backed route plan",
        ) {
            applicationState.plannedRoute != null || applicationState.errorReport != null
        }
        assertNull(
            "the fixture-backed plan must not report a failure, was " +
                "${applicationState.errorReport?.variantLabel}/" +
                "${applicationState.errorReport?.detailText}",
            applicationState.errorReport,
        )
        val plannedRoute = applicationState.plannedRoute
            ?: throw AssertionError("the fixture-backed plan produced no route")
        assertFalse(
            "a provider plan is not the artifact's recorded-route fallback",
            plannedRoute.isRecordedDemoRoute,
        )
        assertEquals(
            "the plan must carry the fixture's committed route",
            recordedRoute.route,
            plannedRoute.route,
        )
        assertEquals(
            "the plan must ask the fixture routing provider once",
            1,
            firstFixtureProvider.callCount,
        )

        // Relaunch: the remembered capability record restores the keyless
        // routing choice, and the row names it again without a new selection.
        val recreatedActivity = recreateApplicationAndAwaitStateConstruction()
        val restoredState = awaitSettledState(recreatedActivity)
        val restoredRoutingRow = restoredState.capabilityProviderState.rows
            .first { row -> row.contract == ProviderContract.ROUTING }
        assertTrue(
            "the remembered keyless choice must register the routing slot again",
            restoredRoutingRow.isRegistered,
        )
        assertEquals(
            "the relaunch must restore the remembered serving provider",
            routingDescriptor.providerKey,
            restoredRoutingRow.servingProviderKey,
        )

        // The restored session's plan runs through the fixture seam again.
        val restoredFixtureProvider = FixtureRoutingProvider(recordedRoute.route)
        driveUserPathOnMainThread { userPathState ->
            userPathState.nativeMapSdk.registerRoutingProviderFromForeign(
                restoredFixtureProvider,
            )
            userPathState.openPlanSearch()
            userPathState.originSearchText = recordedRoute.originName
            userPathState.destinationSearchText = recordedRoute.destinationName
            userPathState.planRouteFromPlanFields()
        }
        awaitOrTimeout(
            timeoutMilliseconds = APPLICATION_STATE_CONSTRUCTION_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the restored session's fixture-backed route plan",
        ) {
            restoredState.plannedRoute != null || restoredState.errorReport != null
        }
        assertNull(
            "the restored fixture-backed plan must not report a failure, was " +
                "${restoredState.errorReport?.variantLabel}/" +
                "${restoredState.errorReport?.detailText}",
            restoredState.errorReport,
        )
        assertEquals(
            "the restored session must plan the fixture's committed route",
            recordedRoute.route,
            restoredState.plannedRoute?.route,
        )
        assertEquals(
            "the restored session must ask the fixture routing provider once",
            1,
            restoredFixtureProvider.callCount,
        )
    }

    /**
     * The settings surface fits the viewport at the documented maximum height
     * fraction and scrolls its content column internally.
     */
    @Test
    fun settingsSurfaceFitsTheViewportAndScrollsInternally() {
        clearRememberedChoices()
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        driveUserPathOnMainThread { userPathState -> userPathState.openSettings() }
        awaitOrTimeout(
            timeoutMilliseconds = SETTINGS_MEASUREMENT_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the settings panel's measured layout",
        ) {
            applicationState.settingsViewportHeightPixels > 0 &&
                applicationState.settingsSheetHeightPixels > 0
        }
        val viewportHeightPixels = applicationState.settingsViewportHeightPixels
        val sheetHeightPixels = applicationState.settingsSheetHeightPixels
        val maximumHeightPixels =
            (viewportHeightPixels * ReferenceAppUXDimensions.PANEL_MAXIMUM_HEIGHT_FRACTION).toInt()
        assertTrue(
            "the settings panel must stay inside the viewport, was $sheetHeightPixels of $viewportHeightPixels",
            sheetHeightPixels <= viewportHeightPixels,
        )
        assertTrue(
            "the settings panel must respect the documented height fraction, was " +
                "$sheetHeightPixels of the $maximumHeightPixels cap",
            sheetHeightPixels <= maximumHeightPixels + SETTINGS_HEIGHT_TOLERANCE_PIXELS,
        )
        // When the content overflows, the same column the user scrolls must
        // move under a swipe; when it fits, every section is already reachable.
        if (applicationState.settingsContentMaximumScrollPixels > 0) {
            Espresso.onView(ViewMatchers.isRoot())
                .perform(androidx.test.espresso.action.ViewActions.swipeUp())
            awaitOrTimeout(
                timeoutMilliseconds = SETTINGS_MEASUREMENT_TIMEOUT_MILLISECONDS,
                timeoutDescription = "the settings content's internal scroll",
            ) {
                applicationState.settingsContentScrollOffsetPixels > 0
            }
        }
    }

    // ---------------------------------------------------------------
    // The suite's shared driving helpers
    // ---------------------------------------------------------------

    /** Launches the application and awaits either the state or its failure. */
    @Test
    fun theLocationControlStartsInactiveAndReportsDeniedPermissionHonestly() {
        launchReferenceApplicationAndAwaitStateConstruction()
        applicationScenario?.onActivity { activity ->
            val applicationState = activity.automationApplicationState
            assertNotNull(applicationState)
            applicationState!!.requestHomeSheetState(HomeSheetState.HALF)
            assertFalse(applicationState.isDeviceLocationActive)
            assertNull(applicationState.deviceLocationStatusText)

            // The permission denial the sheet's launcher reports reaches the
            // controller and the status line.
            applicationState.onDeviceLocationPermissionResult(false)
            assertFalse(applicationState.isDeviceLocationActive)
            assertEquals(
                ReferenceApplicationConstants.DEVICE_LOCATION_PERMISSION_DENIED_TEXT,
                applicationState.deviceLocationStatusText,
            )
        }
    }

    @Test
    fun theDeviceNavigationControlRequiresAPlanAndNavigatesThePlannedRoute() {
        // The plan flow below uses the artifact's recorded places; a keyed
        // run would need a live provider to plan the pair.
        Assume.assumeFalse(
            "the device-navigation case plans through the artifact's places",
            hasRuntimeInjectedApiKey(),
        )
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        driveUserPathOnMainThread { userPathState ->
            userPathState.requestHomeSheetState(HomeSheetState.HALF)
        }
        assertFalse(applicationState.isDeviceLocationActive)
        assertFalse(applicationState.isDeviceNavigationActive)

        // Without a planned route the control states what it needs and starts
        // nothing.
        driveUserPathOnMainThread { userPathState -> userPathState.startDeviceNavigation() }
        assertFalse(applicationState.isDeviceNavigationActive)
        assertFalse(applicationState.isNavigationInTrip)
        assertEquals(
            ReferenceApplicationConstants.DEVICE_NAVIGATION_PLAN_FIRST_TEXT,
            applicationState.deviceLocationStatusText,
        )

        // Plan the artifact's recorded route through the same keyless flow
        // the plan tests drive.
        val recordedRoute = applicationState.artifact.demoRoutes.firstOrNull()
            ?: throw AssertionError("the shared artifact carries no recorded demo route")
        driveUserPathOnMainThread { userPathState ->
            userPathState.openPlanSearch()
            userPathState.originSearchText = recordedRoute.originName
            userPathState.runPlanSearch(ReferenceApplicationState.PlanSearchField.ORIGIN)
        }
        val originSearchResult = applicationState.searchResults.firstOrNull()
            ?: throw AssertionError("the artifact origin search produced no result")
        driveUserPathOnMainThread { userPathState ->
            userPathState.assignSearchResult(originSearchResult)
            userPathState.destinationSearchText = recordedRoute.destinationName
            userPathState.runPlanSearch(ReferenceApplicationState.PlanSearchField.DESTINATION)
        }
        val destinationSearchResult = applicationState.searchResults.firstOrNull()
            ?: throw AssertionError("the artifact destination search produced no result")
        driveUserPathOnMainThread { userPathState ->
            userPathState.assignSearchResult(destinationSearchResult)
            userPathState.planRouteFromPlanFields()
        }
        awaitOrTimeout(
            timeoutMilliseconds = APPLICATION_STATE_CONSTRUCTION_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the keyless recorded-route plan",
        ) {
            applicationState.plannedRoute != null
        }
        assertNull(applicationState.errorReport)

        // The sheet's launcher asks for the runtime permission; grant it here
        // and skip honestly when the emulator has no enabled provider.
        InstrumentationRegistry.getInstrumentation().uiAutomation.grantRuntimePermission(
            ApplicationProvider.getApplicationContext<android.content.Context>().packageName,
            android.Manifest.permission.ACCESS_FINE_LOCATION,
        )
        driveUserPathOnMainThread { userPathState -> userPathState.startDeviceNavigation() }
        Assume.assumeTrue(
            "the SDK location source is available on this emulator",
            applicationState.isDeviceLocationActive,
        )
        assertTrue("the device-driven trip must start", applicationState.isDeviceNavigationActive)
        assertTrue(applicationState.isNavigationInTrip)
        assertTrue(applicationState.tripDriver.isTripActive)
        assertNull(applicationState.tripDriver.startFailure)
        assertEquals(
            "the follow camera takes the fixed deep zoom at trip start",
            ReferenceAppUXDimensions.NAVIGATION_FOLLOW_ZOOM,
            applicationState.zoomLevel,
            0.0,
        )

        // Stopping ends the trip and the source this mode started.
        driveUserPathOnMainThread { userPathState -> userPathState.stopDeviceNavigation() }
        assertFalse(applicationState.isDeviceNavigationActive)
        assertFalse(applicationState.isNavigationInTrip)
        assertFalse(applicationState.tripDriver.isTripActive)
        assertFalse(applicationState.isDeviceLocationActive)
        assertEquals(
            ReferenceApplicationConstants.DEVICE_LOCATION_OFF_TEXT,
            applicationState.deviceLocationStatusText,
        )

        // A location source the visitor started by hand stays running when
        // the trip ends.
        driveUserPathOnMainThread { userPathState -> userPathState.startDeviceLocation() }
        Assume.assumeTrue(
            "the SDK location source is available on this emulator",
            applicationState.isDeviceLocationActive,
        )
        driveUserPathOnMainThread { userPathState -> userPathState.startDeviceNavigation() }
        assertTrue(applicationState.isDeviceNavigationActive)
        driveUserPathOnMainThread { userPathState -> userPathState.stopDeviceNavigation() }
        assertFalse(applicationState.isDeviceNavigationActive)
        assertTrue(
            "a hand-started location source keeps running after the trip stops",
            applicationState.isDeviceLocationActive,
        )
        driveUserPathOnMainThread { userPathState -> userPathState.stopDeviceLocation() }
    }

    // -------------------------------------------------------------------
    // The on-map layer panel (docs/ReferenceAppUX.md §4.2)
    // -------------------------------------------------------------------

    @Test
    fun theMapLayersPanelOpensAndListsItsSectionsInOrder() {
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        driveUserPathOnMainThread { userPathState -> userPathState.openMapLayersPanel() }
        assertTrue(
            "the collapsed control's act opens the panel",
            applicationState.mapLayersState.isPanelOpen,
        )
        assertEquals(
            "the panel's sections follow the specification's order",
            listOf(
                ReferenceApplicationConstants.BASE_MAP_SECTION_LABEL_TEXT,
                ReferenceApplicationConstants.MAP_LAYERS_TRAFFIC_SECTION_LABEL_TEXT,
                ReferenceApplicationConstants.MAP_LAYERS_OVERLAYS_SECTION_LABEL_TEXT,
                ReferenceApplicationConstants.MAP_LAYERS_DEVICE_LOCATION_SECTION_LABEL_TEXT,
            ),
            applicationState.mapLayersState.sectionTitles,
        )
        // The rows are rendered from the shared registry and artifact at
        // runtime, never from a copied provider or scene name.
        assertTrue(
            "the base section lists the registry's sources",
            applicationState.mapLayersState.baseRows.isNotEmpty(),
        )
        assertEquals(
            "the base rows are raster sources first, then vector styles",
            applicationState.baseMapChoices.size + applicationState.vectorBaseMapChoices.size,
            applicationState.mapLayersState.baseRows.size,
        )
        assertTrue(
            "the overlays section has one row per artifact scene",
            applicationState.artifact.overlayScenes.rows.isNotEmpty(),
        )
        assertEquals(
            "every artifact scene is addressed by its own key",
            applicationState.sceneToggleStates.keys.toSet(),
            applicationState.artifact.overlayScenes.rows.map { row -> row.sceneKey }.toSet(),
        )
        assertEquals(
            "the device-location row starts off",
            ReferenceApplicationConstants.DEVICE_LOCATION_PANEL_OFF_TEXT,
            applicationState.mapLayersState.locationStatus.displayText,
        )
        driveUserPathOnMainThread { userPathState -> userPathState.closeMapLayersPanel() }
        assertFalse(applicationState.mapLayersState.isPanelOpen)
    }

    @Test
    fun theDefaultBaseRowReadsServing() {
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        driveUserPathOnMainThread { userPathState -> userPathState.openMapLayersPanel() }
        // The serving base is the SDK's own readback: a keyless start may
        // install the registry's declared styled base a frame after the raster
        // source, so the expected row is derived while polling.
        awaitOrTimeout(
            timeoutMilliseconds = MAP_LAYERS_ROWS_SETTLE_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the serving base row to resolve",
        ) {
            val activeVectorBase = applicationState.activeVectorBase
            val activeBaseMapProviderKey = applicationState.activeBaseMap?.providerKey
            val expectedServingRowIdentifier = when {
                activeVectorBase != null ->
                    ReferenceApplicationConstants.MAP_LAYER_STYLE_ROW_TEST_TAG_PREFIX +
                        activeVectorBase.style.styleId

                activeBaseMapProviderKey != null ->
                    ReferenceApplicationConstants.MAP_LAYER_BASE_ROW_TEST_TAG_PREFIX +
                        activeBaseMapProviderKey

                else -> null
            }
            expectedServingRowIdentifier != null &&
                applicationState.mapLayersState.baseRows.any { row ->
                    row.identifier == expectedServingRowIdentifier && row.isServing
                }
        }
        val servingRows = applicationState.mapLayersState.baseRows.filter { row -> row.isServing }
        assertEquals("exactly one base serves the map", 1, servingRows.size)
        val servingRow = servingRows.single()
        assertTrue(
            "the serving row carries the Serving mark",
            servingRow.buttonLabel.contains(
                ReferenceApplicationConstants.BASE_MAP_ACTIVE_STATUS_TEXT,
            ),
        )
    }

    @Test
    fun aVectorStyleSwitchServesItAndLeavesTheRasterRowUnselected() {
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        val vectorChoice = applicationState.vectorBaseMapChoices.firstOrNull()
        Assume.assumeTrue(
            "the shared registry must offer a vector tileset with committed styles",
            vectorChoice != null,
        )
        if (vectorChoice == null) {
            return
        }
        Assume.assumeTrue(
            "installing a vector base needs the shipped MapView",
            applicationState.mapView != null,
        )
        driveUserPathOnMainThread { userPathState -> userPathState.openMapLayersPanel() }
        // The panel row's own act, dispatched through the state: the SDK
        // validates the source, so a refusal would leave no selection
        // recorded and this test would fail honestly.
        driveUserPathOnMainThread { userPathState ->
            userPathState.selectMapLayerBase(
                MapLayerBaseSelection.Vector(vectorChoice.choiceKey),
            )
        }
        awaitOrTimeout(
            timeoutMilliseconds = VECTOR_BASE_SERVE_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the styled vector base's SDK status to report it serving",
        ) {
            applicationState.activeVectorBase?.choiceKey == vectorChoice.choiceKey
        }
        awaitOrTimeout(
            timeoutMilliseconds = MAP_LAYERS_ROWS_SETTLE_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the panel's vector row to report Serving",
        ) {
            applicationState.mapLayersState.baseRows.any { row ->
                row.identifier == ReferenceApplicationConstants
                    .MAP_LAYER_STYLE_ROW_TEST_TAG_PREFIX + vectorChoice.style.styleId &&
                    row.isServing
            }
        }
        val vectorRow = applicationState.mapLayersState.baseRows.single { row ->
            row.identifier == ReferenceApplicationConstants
                .MAP_LAYER_STYLE_ROW_TEST_TAG_PREFIX + vectorChoice.style.styleId
        }
        assertTrue(vectorRow.isServing)
        assertTrue(
            "the vector row carries the Serving mark",
            vectorRow.buttonLabel.contains(
                ReferenceApplicationConstants.BASE_MAP_ACTIVE_STATUS_TEXT,
            ),
        )
        assertTrue(
            "the raster rows stay unselected under a vector base",
            applicationState.mapLayersState.baseRows
                .filter { row -> row.selection is MapLayerBaseSelection.Raster }
                .none { row -> row.isServing },
        )
        // Leave the raster base serving again so the following tests start
        // from the suite's documented idle base map.
        applicationState.baseMapChoices.firstOrNull { choice -> choice.isServable }?.let { choice ->
            driveUserPathOnMainThread { userPathState ->
                userPathState.selectMapLayerBase(
                    MapLayerBaseSelection.Raster(choice.providerKey),
                )
            }
        }
    }

    @Test
    fun theTrafficSwitchReadsNeedsCredentialWhileKeyless() {
        Assume.assumeFalse(
            "the keyless case needs a run without a runtime-injected provider key",
            hasRuntimeInjectedApiKey(),
        )
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        val trafficDescriptor = applicationState.providerRegistry.providers.firstOrNull { descriptor ->
            ReferenceApplicationConstants.TRAFFIC_FLOW_CAPABILITY_NAME in
                descriptor.servedContracts &&
                descriptor.isRegistrationAvailableOnAndroid
        }
        Assume.assumeTrue(
            "the shared registry must declare a traffic-flow descriptor",
            trafficDescriptor != null,
        )
        if (trafficDescriptor == null) {
            return
        }
        driveUserPathOnMainThread { userPathState -> userPathState.openMapLayersPanel() }
        val trafficAvailability = applicationState.mapLayersState.trafficAvailability
        assertFalse(
            "a keyless session cannot turn the traffic layer on",
            trafficAvailability.isAvailable,
        )
        assertEquals(
            "the switch names the credential it still needs",
            ReferenceApplicationConstants.BASE_MAP_NEEDS_CREDENTIAL_TEMPLATE
                .format(trafficDescriptor.credentialLabel),
            trafficAvailability.reasonText,
        )
        driveUserPathOnMainThread { userPathState -> userPathState.setTrafficLayerEnabled(true) }
        assertFalse(
            "the switch never pretends a keyless layer is on",
            applicationState.mapLayersState.isTrafficEnabled,
        )
        assertEquals(
            "the panel states why the layer cannot turn on",
            ReferenceApplicationConstants.BASE_MAP_NEEDS_CREDENTIAL_TEMPLATE
                .format(trafficDescriptor.credentialLabel),
            applicationState.mapLayersState.statusLineText,
        )
    }

    @Test
    fun theTrafficSwitchInstallsFlowPolylinesWithACredential() {
        Assume.assumeTrue(
            "installing live flow polylines needs the runtime-injected provider key",
            hasRuntimeInjectedApiKey(),
        )
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        driveUserPathOnMainThread { userPathState -> userPathState.openMapLayersPanel() }
        assertTrue(
            "an applied credential makes the traffic switch available",
            applicationState.mapLayersState.trafficAvailability.isAvailable,
        )
        // The capability call resolves the registered provider; this fake
        // serves the committed row shape same-process, so the assertion never
        // reaches the network.
        val trafficFlowProvider = FixedTrafficFlowProvider(sampleTrafficFlowSegments())
        applicationState.nativeMapSdk.registerTrafficFlowProviderFromForeign(trafficFlowProvider)
        driveUserPathOnMainThread { userPathState -> userPathState.setTrafficLayerEnabled(true) }
        awaitOrTimeout(
            timeoutMilliseconds = TRAFFIC_OVERLAY_INSTALL_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the traffic layer's debounced first refresh",
        ) {
            applicationState.mapLayersState.trafficOverlayInstallCount > 0
        }
        assertTrue(
            "the switch reports the layer on",
            applicationState.mapLayersState.isTrafficEnabled,
        )
        assertTrue(
            "the capability's flow segments reached the overlay list",
            applicationState.trafficFlowSegments.isNotEmpty(),
        )
        assertTrue(
            "a flow segment carries its road geometry",
            applicationState.trafficFlowSegments.all { segment ->
                segment.shapeCoordinates.size >= MINIMUM_FLOW_SEGMENT_COORDINATE_COUNT
            },
        )
        assertNull(
            "a served refresh reports nothing in the error strip",
            applicationState.errorReport,
        )
        driveUserPathOnMainThread { userPathState -> userPathState.setTrafficLayerEnabled(false) }
        assertTrue(
            "turning the layer off clears the flow polylines",
            applicationState.trafficFlowSegments.isEmpty(),
        )
    }

    /**
     * The M50 native proof: two settles over the same ground cost one
     * catalog-provider call, the repeat is answered from the grid cache, and a
     * scripted pan across cells never lets the cache hold more than its
     * budget. The counters are the SDK's own readback, not the app's call
     * count, so the proof is about the provider, not the shell.
     */
    @Test
    fun theTrafficCacheAnswersARepeatedSettleWithOneProviderCallAndStaysInsideItsBudget() {
        Assume.assumeTrue(
            "the catalog provider's traffic cache needs the runtime-injected key",
            hasRuntimeInjectedApiKey(),
        )
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        driveUserPathOnMainThread { userPathState -> userPathState.openMapLayersPanel() }
        Assume.assumeTrue(
            "an applied credential makes the traffic switch available",
            applicationState.mapLayersState.trafficAvailability.isAvailable,
        )

        driveUserPathOnMainThread { userPathState -> userPathState.setTrafficLayerEnabled(true) }
        awaitOrTimeout(
            timeoutMilliseconds = TRAFFIC_CACHE_SETTLE_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the first settle's traffic cache readback",
        ) {
            applicationState.mapLayersState.trafficCacheStatistics != null
        }
        val firstStatistics = applicationState.mapLayersState.trafficCacheStatistics
            ?: error("the catalog provider answers through the grid cache")
        assertEquals(
            "the first settle asks the provider once",
            1UL,
            firstStatistics.missCount,
        )
        assertEquals(
            "the first settle answers nothing from the cache",
            0UL,
            firstStatistics.hitCount,
        )
        assertTrue(
            "the first settle stores the cell it asked for",
            firstStatistics.entryCount > 0UL,
        )
        assertTrue(
            "the readback must carry the entry budget",
            firstStatistics.maximumEntryCount > 0UL,
        )
        assertEquals(
            "the first settle evicts nothing",
            0UL,
            firstStatistics.evictionCount,
        )

        // A second settle over the same ground: a small nudge stays inside the
        // grid cell, so the cache answers it without asking the provider.
        val fetchCountBeforeRepeat = applicationState.mapLayersState.trafficFetchCount
        driveUserPathOnMainThread { userPathState ->
            userPathState.panByPixels(PAN_DRAG_RIGHTWARD_PIXELS, 0.0)
        }
        awaitOrTimeout(
            timeoutMilliseconds = TRAFFIC_CACHE_SETTLE_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the repeated settle answered from the cache",
        ) {
            applicationState.mapLayersState.trafficFetchCount > fetchCountBeforeRepeat &&
                (applicationState.mapLayersState.trafficCacheStatistics?.hitCount ?: 0UL) >= 1UL
        }
        val repeatStatistics = applicationState.mapLayersState.trafficCacheStatistics
            ?: error("the repeated settle keeps the cache readback")
        assertEquals(
            "the repeat must not ask the provider again",
            1UL,
            repeatStatistics.missCount,
        )
        assertEquals("the repeat is answered as one hit", 1UL, repeatStatistics.hitCount)

        // A scripted pan across cells: every settle stores what it asked for,
        // and the cache never holds more cells than its budget.
        repeat(TRAFFIC_CACHE_PAN_SETTLE_COUNT) {
            val fetchCountBeforePan = applicationState.mapLayersState.trafficFetchCount
            driveUserPathOnMainThread { userPathState ->
                userPathState.panByPixels(TRAFFIC_CACHE_PAN_STEP_PIXELS, 0.0)
            }
            awaitOrTimeout(
                timeoutMilliseconds = TRAFFIC_CACHE_SETTLE_TIMEOUT_MILLISECONDS,
                timeoutDescription = "the panned settle's cache readback",
            ) {
                applicationState.mapLayersState.trafficFetchCount > fetchCountBeforePan
            }
            val pannedStatistics = applicationState.mapLayersState.trafficCacheStatistics
                ?: error("the panned settle keeps the cache readback")
            assertTrue(
                "the cache must never hold more cells than its budget, held " +
                    "${pannedStatistics.entryCount} of ${pannedStatistics.maximumEntryCount}",
                pannedStatistics.entryCount <= pannedStatistics.maximumEntryCount,
            )
        }

        driveUserPathOnMainThread { userPathState -> userPathState.setTrafficLayerEnabled(false) }
        assertNull(
            "turning the layer off clears the cache readback",
            applicationState.mapLayersState.trafficCacheStatistics,
        )
    }

    @Test
    fun aRememberedTrafficChoiceRestoresAfterRecreation() {
        Assume.assumeTrue(
            "the remembered traffic choice restores once a provider serves it",
            hasRuntimeInjectedApiKey(),
        )
        clearRememberedChoices()
        val activity = launchReferenceApplicationAndAwaitStateConstruction(
            clearsRememberedChoices = false,
        )
        val applicationState = awaitSettledState(activity)
        assertTrue(
            "the applied credential makes the traffic switch available",
            applicationState.mapLayersState.trafficAvailability.isAvailable,
        )
        val trafficFlowProvider = FixedTrafficFlowProvider(sampleTrafficFlowSegments())
        applicationState.nativeMapSdk.registerTrafficFlowProviderFromForeign(trafficFlowProvider)
        driveUserPathOnMainThread { userPathState -> userPathState.setTrafficLayerEnabled(true) }
        assertTrue(applicationState.mapLayersState.isTrafficEnabled)
        assertEquals(
            "the traffic choice joins the remembered record",
            true,
            readRememberedChoices()?.trafficLayerEnabled,
        )

        val recreatedActivity = recreateApplicationAndAwaitStateConstruction()
        val restoredState = recreatedActivity.automationApplicationState
            ?: error("the recreated application state never constructed")
        // The suite runs keyless: no runtime credential is re-applied after a
        // recreation, so this case exercises only the remembered-choice path.
        awaitOrTimeout(
            timeoutMilliseconds = APPLICATION_STATE_CONSTRUCTION_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the restored credential to make the traffic switch available",
        ) {
            restoredState.mapLayersState.trafficAvailability.isAvailable
        }
        // Register the same-process flow provider before the restored layer's
        // 400 ms debounce fires, so its refresh resolves this provider instead
        // of a live one.
        driveUserPathOnMainThread { userPathState ->
            userPathState.nativeMapSdk.registerTrafficFlowProviderFromForeign(
                FixedTrafficFlowProvider(sampleTrafficFlowSegments()),
            )
        }
        awaitSettledState(recreatedActivity)
        awaitOrTimeout(
            timeoutMilliseconds = TRAFFIC_OVERLAY_INSTALL_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the remembered traffic layer to restore",
        ) {
            restoredState.mapLayersState.isTrafficEnabled
        }
        assertTrue(
            "the remembered traffic choice restores after recreation",
            restoredState.mapLayersState.isTrafficEnabled,
        )
        // Leave the recreated session's layer off so the following tests
        // start from the suite's documented idle state.
        driveUserPathOnMainThread { userPathState -> userPathState.setTrafficLayerEnabled(false) }
    }

    @Test
    fun theMapLayersPanelClosesOnBackAndReturnsFocus() {
        val activity = launchReferenceApplicationAndAwaitStateConstruction()
        val applicationState = awaitSettledState(activity)
        // Compose grants focus navigation outside touch mode; a hardware key
        // event puts the emulator into that mode, so the focus handback the
        // panel performs is observable here.
        InstrumentationRegistry.getInstrumentation()
            .sendKeyDownUpSync(android.view.KeyEvent.KEYCODE_DPAD_DOWN)
        driveUserPathOnMainThread { userPathState -> userPathState.openMapLayersPanel() }
        assertTrue(applicationState.mapLayersState.isPanelOpen)
        // The close path's focus return applies after the panel composition
        // has presented; wait for it before the back action.
        awaitOrTimeout(
            timeoutMilliseconds = MAP_LAYERS_FOCUS_RETURN_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the panel composition to present",
        ) {
            applicationState.mapLayersState.hasPresentedPanel
        }
        Espresso.pressBack()
        awaitOrTimeout(
            timeoutMilliseconds = MAP_LAYERS_FOCUS_RETURN_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the platform back action to close the panel",
        ) {
            !applicationState.mapLayersState.isPanelOpen
        }
        awaitOrTimeout(
            timeoutMilliseconds = MAP_LAYERS_FOCUS_RETURN_TIMEOUT_MILLISECONDS,
            timeoutDescription = "focus to return to the collapsed control",
        ) {
            applicationState.mapLayersState.isControlFocused
        }
        assertTrue(applicationState.mapLayersState.isControlFocused)
        // The focus handback needs keyboard (non-touch) input mode; restore
        // touch mode with one touch so the pointer-injection cases that follow
        // still receive their taps.
        injectMapTaps(tapCount = SINGLE_TAP_COUNT)
    }

    /**
     * The shared `plan_and_navigate` flow: the launch extra names it, the two
     * fields hold typed text that is never assigned from a result row, Plan
     * route resolves both, and the start drives the trip with the banner, ETA
     * line and speed controls presented.
     */
    @Test
    fun thePlanAndNavigateFlowResolvesTypedTextAndStartsTheTrip() {
        Assume.assumeFalse(
            "the keyless flow plans the artifact's recorded route",
            hasRuntimeInjectedApiKey(),
        )
        val activity = launchReferenceApplicationAndAwaitStateConstruction(
            referenceFlowId = ReferenceApplicationConstants.REFERENCE_FLOW_PLAN_AND_NAVIGATE,
        )
        val applicationState = awaitSettledState(activity)
        assertEquals(
            ReferenceApplicationConstants.REFERENCE_FLOW_PLAN_AND_NAVIGATE,
            applicationState.requestedReferenceFlowId,
        )
        val recordedRoute = applicationState.artifact.demoRoutes.firstOrNull()
            ?: throw AssertionError("the shared artifact carries no recorded demo route")
        assertEquals(
            "the peek states the demo-plan fallback before a route is planned",
            ReferenceApplicationConstants.PLAN_TRIP_SOURCE_DEMO_PLAN_TEMPLATE
                .format(applicationState.selectedDemoPlan.label),
            applicationState.tripSourceText,
        )
        driveUserPathOnMainThread { userPathState ->
            userPathState.openPlanSearch()
            // Typed text only: no result row is tapped and no search runs.
            userPathState.originSearchText = recordedRoute.originName
            userPathState.destinationSearchText = recordedRoute.destinationName
        }
        assertTrue("typed text enables Plan route", applicationState.canPlanRoute)
        assertEquals("", applicationState.planRouteReasonText)
        // The Swap action exchanges the pair without planning.
        driveUserPathOnMainThread { userPathState -> userPathState.swapPlanFields() }
        assertEquals(recordedRoute.destinationName, applicationState.originSearchText)
        assertEquals(recordedRoute.originName, applicationState.destinationSearchText)
        assertNull("Swap never plans", applicationState.plannedRoute)
        driveUserPathOnMainThread { userPathState -> userPathState.swapPlanFields() }
        assertEquals(recordedRoute.originName, applicationState.originSearchText)
        assertEquals(recordedRoute.destinationName, applicationState.destinationSearchText)
        driveUserPathOnMainThread { userPathState -> userPathState.planRouteFromPlanFields() }
        awaitOrTimeout(
            timeoutMilliseconds = APPLICATION_STATE_CONSTRUCTION_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the typed-text plan resolution",
        ) {
            applicationState.plannedRoute != null
        }
        val plannedRoute = applicationState.plannedRoute
            ?: throw AssertionError("the typed-text plan produced no route")
        assertEquals(recordedRoute.route, plannedRoute.route)
        assertEquals(
            ReferenceApplicationConstants.PLAN_TRIP_SOURCE_PLANNED_ROUTE_TEMPLATE
                .format(recordedRoute.destinationName),
            applicationState.tripSourceText,
        )
        awaitOrTimeout(
            timeoutMilliseconds = EMULATOR_FIRST_READY_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the shipped map view",
        ) {
            applicationState.mapView != null
        }
        driveUserPathOnMainThread { userPathState -> userPathState.startSimulatedTrip() }
        awaitOrTimeout(
            timeoutMilliseconds = EMULATOR_FIRST_READY_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the trip's first instruction",
        ) {
            applicationState.isNavigationInTrip &&
                applicationState.navigationListener.instructionState.value != null
        }
        assertTrue(applicationState.isNavigationInTrip)
        assertTrue(applicationState.isSimulatedTripSeekable)
        assertTrue(
            ReferenceApplicationConstants.SIMULATED_TRIP_SPEED_MULTIPLIER_CHOICES.isNotEmpty(),
        )
        driveUserPathOnMainThread { userPathState -> userPathState.exitSimulatedTrip() }
        assertFalse(applicationState.isNavigationInTrip)
    }

    /**
     * The `my_location_field` flow: each field's chip fills that field and
     * only that field. The emulator's fix is assumed; the fill path itself is
     * the same public method the chip calls.
     */
    @Test
    fun theMyLocationFieldFlowFillsOnlyTheFieldThatAsked() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        // Grant the runtime permission for this run only: the module's
        // host-side reset revokes the location grants before every connected
        // run, so no case inherits a grant from an earlier run.
        instrumentation.uiAutomation.grantRuntimePermission(
            instrumentation.targetContext.packageName,
            Manifest.permission.ACCESS_FINE_LOCATION,
        )
        val activity = launchReferenceApplicationAndAwaitStateConstruction(
            referenceFlowId = ReferenceApplicationConstants.REFERENCE_FLOW_MY_LOCATION_FIELD,
        )
        val applicationState = awaitSettledState(activity)
        assertEquals(
            ReferenceApplicationConstants.REFERENCE_FLOW_MY_LOCATION_FIELD,
            applicationState.requestedReferenceFlowId,
        )
        awaitOrTimeout(
            timeoutMilliseconds = EMULATOR_FIRST_READY_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the shipped map view",
        ) {
            applicationState.mapView != null
        }
        driveUserPathOnMainThread { userPathState -> userPathState.startDeviceLocation() }
        val producedFix = runCatching {
            awaitOrTimeout(
                timeoutMilliseconds = EMULATOR_FIRST_READY_TIMEOUT_MILLISECONDS,
                timeoutDescription = "an emulator location fix",
            ) {
                applicationState.latestLocationFix != null
            }
        }.isSuccess
        Assume.assumeTrue("the emulator produced no location fix", producedFix)

        driveUserPathOnMainThread { userPathState ->
            userPathState.fillPlanFieldFromMyLocation(
                ReferenceApplicationState.PlanSearchField.ORIGIN,
            )
        }
        assertEquals(
            ReferenceApplicationConstants.PLAN_MY_LOCATION_ACTION_TEXT,
            applicationState.originSearchText,
        )
        assertTrue(applicationState.isOriginResolved)
        assertEquals(
            "the origin chip must not fill the destination",
            "",
            applicationState.destinationSearchText,
        )
        assertFalse(applicationState.canPlanRoute)

        driveUserPathOnMainThread { userPathState ->
            userPathState.fillPlanFieldFromMyLocation(
                ReferenceApplicationState.PlanSearchField.DESTINATION,
            )
        }
        assertEquals(
            ReferenceApplicationConstants.PLAN_MY_LOCATION_ACTION_TEXT,
            applicationState.destinationSearchText,
        )
        assertTrue(applicationState.isDestinationResolved)
        assertEquals(
            "the destination chip must not overwrite the origin",
            ReferenceApplicationConstants.PLAN_MY_LOCATION_ACTION_TEXT,
            applicationState.originSearchText,
        )
        assertNull("the My location chips never plan", applicationState.plannedRoute)
    }

    /**
     * The `routing_provider_switch` flow: the routing row composes the
     * registry provider's name and state, and a keyless selection registers the
     * descriptor's public demo address (or refuses honestly when the registry
     * declares none).
     */
    @Test
    fun theRoutingProviderSwitchShowsTheComposedRowAndDemoEndpoint() {
        val activity = launchReferenceApplicationAndAwaitStateConstruction(
            referenceFlowId = ReferenceApplicationConstants.REFERENCE_FLOW_ROUTING_PROVIDER_SWITCH,
        )
        val applicationState = awaitSettledState(activity)
        assertEquals(
            ReferenceApplicationConstants.REFERENCE_FLOW_ROUTING_PROVIDER_SWITCH,
            applicationState.requestedReferenceFlowId,
        )
        val routingRow = applicationState.capabilityProviderState.rows.firstOrNull { row ->
            row.contract == ProviderContract.ROUTING
        } ?: throw AssertionError("the capability readback carries no routing row")
        // A genuinely keyless choice: the descriptor declares no credential
        // at all. In a keyed session an applied credential makes a
        // credentialed provider selectable too, and its row is the composed
        // Ready line rather than a demo endpoint.
        val keylessRoutingDescriptor = routingRow.choices.firstOrNull { choice ->
            !choice.requiresCredential &&
                applicationState.providerRegistry.providers.any { provider ->
                    provider.providerKey == choice.providerKey &&
                        provider.credentialLabel.isBlank()
                }
        }
        Assume.assumeTrue(
            "the shared registry must offer a keyless routing choice",
            keylessRoutingDescriptor != null,
        )
        val routingChoice = keylessRoutingDescriptor
            ?: throw AssertionError("the routing row offers no keyless choice")
        assertEquals(
            routingChoice.displayName + " · " +
                ReferenceApplicationConstants.SETTINGS_PROVIDER_STATE_READY_TEXT,
            routingChoice.pickerLabel,
        )
        driveUserPathOnMainThread { userPathState ->
            userPathState.selectCapabilityProvider(
                ProviderContract.ROUTING,
                routingChoice.providerKey,
            )
        }
        val selectedRoutingRow = applicationState.capabilityProviderState.rows.first { row ->
            row.contract == ProviderContract.ROUTING
        }
        val descriptor = applicationState.providerRegistry.providers.first { provider ->
            provider.providerKey == routingChoice.providerKey
        }
        if (descriptor.demoBaseUrl.isBlank()) {
            assertEquals(
                ReferenceApplicationConstants.SETTINGS_PROVIDER_NO_DEMO_ENDPOINT_TEMPLATE
                    .format(descriptor.displayName),
                selectedRoutingRow.detailText,
            )
        } else {
            assertEquals(
                descriptor.displayName + " · " +
                    ReferenceApplicationConstants.SETTINGS_PROVIDER_STATE_READY_TEXT,
                selectedRoutingRow.servingStatusText,
            )
            assertTrue(
                "the keyless selection registers the demo deployment, so Plan route "
                    + "asks that provider instead of the recorded fallback",
                applicationState.capabilityProviderState.hasRegisteredRoutingProvider,
            )
            assertEquals(
                ReferenceApplicationConstants.SETTINGS_ROUTING_DEMO_ENDPOINT_TEMPLATE
                    .format(descriptor.demoBaseUrl),
                selectedRoutingRow.demoEndpointText,
            )
        }
    }

    /**
     * The `diagnostics_opt_in` flow: no listener exists before the pane opens,
     * the first open starts one Info stream, the explicit switch lowers it to
     * Debug, and Clear empties the bounded window.
     */
    @Test
    fun theDiagnosticsOptInFlowStartsNoStreamUntilThePaneOpens() {
        val activity = launchReferenceApplicationAndAwaitStateConstruction(
            referenceFlowId = ReferenceApplicationConstants.REFERENCE_FLOW_DIAGNOSTICS_OPT_IN,
        )
        val applicationState = awaitSettledState(activity)
        assertEquals(
            ReferenceApplicationConstants.REFERENCE_FLOW_DIAGNOSTICS_OPT_IN,
            applicationState.requestedReferenceFlowId,
        )
        assertFalse(
            "no diagnostics listener engages before the pane opens",
            applicationState.hasEngagedDiagnosticsEventStream,
        )
        assertTrue(applicationState.diagnosticEventListener.eventLines.isEmpty())
        assertNull(applicationState.diagnosticsSubscriptionSeverity)

        driveUserPathOnMainThread { userPathState -> userPathState.openDiagnostics() }
        assertTrue(applicationState.isDiagnosticsOpen)
        assertTrue(applicationState.hasEngagedDiagnosticsEventStream)
        assertEquals(
            uniffi.rustic_maps.SdkLogSeverity.INFO,
            applicationState.diagnosticsSubscriptionSeverity,
        )
        assertFalse(applicationState.isDiagnosticsDebugDetailEnabled)

        driveUserPathOnMainThread { userPathState ->
            userPathState.updateDiagnosticsDebugDetailEnabled(true)
        }
        assertTrue(applicationState.isDiagnosticsDebugDetailEnabled)
        assertEquals(
            uniffi.rustic_maps.SdkLogSeverity.DEBUG,
            applicationState.diagnosticsSubscriptionSeverity,
        )
        assertEquals(
            "the display window is the shared bounded value",
            ReferenceApplicationConstants.DIAGNOSTICS_EVENT_WINDOW,
            100,
        )
        driveUserPathOnMainThread { userPathState -> userPathState.clearDiagnosticsEvents() }
        assertTrue(applicationState.diagnosticEventListener.eventLines.isEmpty())
        driveUserPathOnMainThread { userPathState -> userPathState.dismissDiagnostics() }
        assertFalse(applicationState.isDiagnosticsOpen)
    }

    private fun launchReferenceApplicationAndAwaitStateConstruction(
        clearsRememberedChoices: Boolean = true,
        referenceFlowId: String? = null,
    ): MainActivity {
        if (clearsRememberedChoices) {
            clearRememberedChoices()
        }
        val launchIntent = Intent(
            ApplicationProvider.getApplicationContext<android.content.Context>(),
            MainActivity::class.java,
        )
        referenceFlowId?.let { flowId ->
            launchIntent.putExtra(
                ReferenceApplicationConstants.REFERENCE_FLOW_INTENT_EXTRA,
                flowId,
            )
        }
        val scenario = ActivityScenario.launch<MainActivity>(launchIntent)
        applicationScenario = scenario
        var launchedActivity: MainActivity? = null
        scenario.onActivity { activity -> launchedActivity = activity }
        val activity = launchedActivity ?: error("the scenario never produced the application activity")
        awaitOrTimeout(
            timeoutMilliseconds = APPLICATION_STATE_CONSTRUCTION_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the application state construction",
        ) {
            activity.automationApplicationState != null || activity.automationStartupFailureText != null
        }
        val applicationState = activity.automationApplicationState
        return if (applicationState != null) {
            activity
        } else {
            throw AssertionError(
                "the application never constructed: " +
                    (activity.automationStartupFailureText ?: "unknown startup failure"),
            )
        }
    }

    /**
     * Whether this run supplies a runtime-injected provider credential.
     *
     * The suite is hermetic by policy: it never injects a credential, so every
     * case runs keyless over the committed fixtures and the shared demo
     * artifacts. The credential-dependent cases below keep their keyed branch
     * documented, but it is unreachable here — a live provider call from the
     * instrumentation suite would break the no-network rule and the keyless
     * distribution rule alike.
     */
    private fun hasRuntimeInjectedApiKey(): Boolean = false

    /**
     * Awaits the first honest settle: the ready ladder when the automation
     * supplied a credential, otherwise the neutral keyless prompt (§6.5).
     */
    private fun awaitSettledState(activity: MainActivity): ReferenceApplicationState {
        val applicationState = activity.automationApplicationState ?: error("no application state constructed")
        if (hasRuntimeInjectedApiKey()) {
            awaitOrTimeout(
                timeoutMilliseconds = EMULATOR_FIRST_READY_TIMEOUT_MILLISECONDS,
                timeoutDescription = "the readiness ladder's ready state on the emulator graphics stack",
            ) {
                applicationState.readinessState != ReadinessState.LOADING
            }
            assertEquals(
                "the readiness ladder settled on the ready state",
                ReadinessState.READY,
                applicationState.readinessState,
            )
        } else {
            // Without a credential the application still opens: the registry's
            // credential-less base map serves the map (M24), so the ladder
            // reaches READY and the chip carries the ready line. The neutral
            // keyless prompt is what a registry with no keyless source shows,
            // and this assertion pins whichever of the two this build's
            // registry implies.
            val registryCarriesKeylessBaseMap = applicationState.baseMapChoices.any { choice ->
                choice.descriptor.rasterUrlTemplate.isNotBlank()
            }
            awaitOrTimeout(
                timeoutMilliseconds = APPLICATION_STATE_CONSTRUCTION_TIMEOUT_MILLISECONDS,
                timeoutDescription = "the keyless readiness settle",
            ) {
                applicationState.readinessState != ReadinessState.LOADING
            }
            if (registryCarriesKeylessBaseMap) {
                assertEquals(
                    "a keyless base map serves, so the ladder reaches the ready state",
                    ReadinessState.READY,
                    applicationState.readinessState,
                )
            } else {
                assertEquals(
                    ReferenceApplicationConstants.KEYLESS_CREDENTIAL_STATUS_TEXT,
                    applicationState.readinessChipText,
                )
            }
        }
        return applicationState
    }

    /** Drives one user-path act on the main thread (the same thread the screen drives them on). */
    private fun driveUserPathOnMainThread(action: (ReferenceApplicationState) -> Unit) {
        val scenario = applicationScenario ?: error("no application scenario started")
        scenario.onActivity { activity ->
            val applicationState = activity.automationApplicationState ?: return@onActivity
            action(applicationState)
        }
    }

    /** Clears both preference stores the app reads at launch. */
    private fun clearRememberedChoices() {
        val context = ApplicationProvider.getApplicationContext<android.content.Context>()
        context.getSharedPreferences(
            RememberedChoicesStore.PREFERENCES_NAME,
            android.content.Context.MODE_PRIVATE,
        ).edit().clear().commit()
        context.getSharedPreferences(
            ThemeChoiceStore.PREFERENCES_NAME,
            android.content.Context.MODE_PRIVATE,
        ).edit().clear().commit()
    }

    /**
     * Removes every provider's remembered credential from the application's
     * opt-in secure store. The suite runs keyless, so this only clears a copy
     * an earlier run (or a developer's device) left behind — a case must never
     * begin with a provider session it did not create.
     */
    private fun clearPersistedCredentialStore() {
        val context = ApplicationProvider.getApplicationContext<android.content.Context>()
        val providerDescriptors = com.rusticmaps.sdk.demo.ProviderRegistryLoader
            .load(context.assets)
            .providers
        val credentialStore = ProviderCredentialStore(context)
        providerDescriptors.forEach { descriptor ->
            credentialStore.clear(descriptor.providerKey)
        }
    }

    /** Reads the record the running application wrote. */
    private fun readRememberedChoices(): RememberedChoices? =
        RememberedChoicesStore(
            ApplicationProvider.getApplicationContext<android.content.Context>(),
        ).load()

    /**
     * Recreates the activity and awaits its freshly constructed state — the
     * relaunch path the record must survive. The process stays alive, so the
     * only state the new activity can read is what the stores hold.
     */
    private fun recreateApplicationAndAwaitStateConstruction(): MainActivity {
        val scenario = applicationScenario ?: error("no application scenario started")
        scenario.recreate()
        var recreatedActivity: MainActivity? = null
        scenario.onActivity { activity -> recreatedActivity = activity }
        val activity = recreatedActivity
            ?: error("the recreated scenario never produced the application activity")
        awaitOrTimeout(
            timeoutMilliseconds = APPLICATION_STATE_CONSTRUCTION_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the recreated application state construction",
        ) {
            activity.automationApplicationState != null ||
                activity.automationStartupFailureText != null
        }
        if (activity.automationApplicationState == null) {
            throw AssertionError(
                "the recreated application never constructed: " +
                    (activity.automationStartupFailureText ?: "unknown startup failure"),
            )
        }
        return activity
    }

    /** The plan flows avoid nothing, so the provider's fastest path stands. */
    private fun avoidNothingOptions(): AvoidOptions = AvoidOptions(
        tollRoads = false,
        limitedAccessHighways = false,
        ferries = false,
        unpavedRoads = false,
    )

    /** Reads the camera values the screen renders (the snapshot-state view). */
    private fun readApplicationCameraValues(applicationState: ReferenceApplicationState): ApplicationCameraValues =
        ApplicationCameraValues(
            centerLatitudeDegrees = applicationState.centerLatitudeDegrees,
            centerLongitudeDegrees = applicationState.centerLongitudeDegrees,
            zoomLevel = applicationState.zoomLevel,
            bearingDegrees = applicationState.bearingDegrees,
            pitchDegrees = applicationState.pitchDegrees,
        )

    /**
     * Polls a state condition on the test thread until it holds or the named
     * timeout elapses. The waits are never screenshot waits: the polled values
     * are the exact ones the screen renders.
     */
    private fun awaitOrTimeout(
        timeoutMilliseconds: Long,
        timeoutDescription: String,
        condition: () -> Boolean,
    ) {
        val deadlineMilliseconds = android.os.SystemClock.elapsedRealtime() + timeoutMilliseconds
        while (!condition()) {
            if (android.os.SystemClock.elapsedRealtime() > deadlineMilliseconds) {
                throw AssertionError("$timeoutDescription did not settle within $timeoutMilliseconds ms")
            }
            Thread.sleep(READINESS_POLL_INTERVAL_MILLISECONDS)
        }
    }

    /**
     * Injects [tapCount] real pointer taps at one point inside the shipped
     * MapView (the Compose host forwards them to the view's own gesture
     * detector). A double tap's two presses sit inside the platform's
     * double-tap window but above its minimum gap, so the view recognizes the
     * gesture rather than two separate clicks.
     */
    private fun injectMapTaps(tapCount: Int) {
        val tapPoint = mapViewScreenPoint()
        val uiAutomation = InstrumentationRegistry.getInstrumentation().uiAutomation
        repeat(tapCount) { tapIndex ->
            val downTime = SystemClock.uptimeMillis()
            injectPointerEvent(
                uiAutomation,
                MotionEvent.obtain(
                    downTime,
                    downTime,
                    MotionEvent.ACTION_DOWN,
                    tapPoint.x,
                    tapPoint.y,
                    0,
                ),
            )
            Thread.sleep(TAP_PRESS_DURATION_MILLISECONDS)
            injectPointerEvent(
                uiAutomation,
                MotionEvent.obtain(
                    downTime,
                    SystemClock.uptimeMillis(),
                    MotionEvent.ACTION_UP,
                    tapPoint.x,
                    tapPoint.y,
                    0,
                ),
            )
            if (tapIndex < tapCount - 1) {
                Thread.sleep(TAP_GAP_MILLISECONDS)
            }
        }
    }

    /** Injects one touchscreen event into the window; a refusal fails the test. */
    private fun injectPointerEvent(
        uiAutomation: android.app.UiAutomation,
        event: MotionEvent,
    ) {
        try {
            event.source = InputDevice.SOURCE_TOUCHSCREEN
            assertTrue("the input system must accept the map tap event", uiAutomation.injectInputEvent(event, true))
        } finally {
            event.recycle()
        }
    }

    /**
     * The screen point the tap gestures use: the middle of the map at 35% of
     * its height, clear of the readiness chip, the compass and the bottom
     * sheet dock.
     */
    private fun mapViewScreenPoint(): PointF {
        var tapPoint: PointF? = null
        Espresso.onView(
            ViewMatchers.isAssignableFrom(com.rusticmaps.sdk.MapView::class.java),
        ).check { view, noViewFoundException ->
            if (noViewFoundException != null) {
                throw noViewFoundException
            }
            val matchedView = view ?: throw AssertionError("the MapView check produced no view")
            val locationOnScreen = IntArray(2)
            matchedView.getLocationOnScreen(locationOnScreen)
            tapPoint = PointF(
                locationOnScreen[0] + matchedView.width * MAP_TAP_HORIZONTAL_FRACTION,
                locationOnScreen[1] + matchedView.height * MAP_TAP_VERTICAL_FRACTION,
            )
        }
        return tapPoint ?: throw AssertionError("the shipped MapView is not in the view hierarchy")
    }

    /** The camera values one comparison covers (the screen renders all five). */
    private data class ApplicationCameraValues(
        val centerLatitudeDegrees: Double,
        val centerLongitudeDegrees: Double,
        val zoomLevel: Double,
        val bearingDegrees: Double,
        val pitchDegrees: Double,
    ) {
        /** Whether any one camera value differs (an applied fit always moves at least one). */
        fun differsFrom(other: ApplicationCameraValues): Boolean =
            centerLatitudeDegrees != other.centerLatitudeDegrees ||
                centerLongitudeDegrees != other.centerLongitudeDegrees ||
                zoomLevel != other.zoomLevel ||
                bearingDegrees != other.bearingDegrees ||
                pitchDegrees != other.pitchDegrees
    }

    /**
     * A traffic-flow provider registered from foreign: it answers every
     * capability call with the committed row shape, so the traffic-layer
     * cases exercise the real SDK call path without a network round trip.
     */
    private class FixedTrafficFlowProvider(
        private val trafficFlowSegments: List<TrafficFlowData>,
    ) : uniffi.rustic_maps.TrafficFlowProvider {
        override suspend fun fetchTrafficFlow(
            boundingBox: BoundingBox,
        ): List<TrafficFlowData> = trafficFlowSegments
    }

    /**
     * A place-search provider registered from foreign: it answers every
     * type-ahead call from the case's own committed values, records what the
     * SDK asked, and can hold one answer behind a delay so the cancellation
     * and superseded-answer rules are observable without a network round trip.
     */
    private class FixtureSearchPlaceProvider(
        private val resultsForQuery: (String) -> List<Place>,
        private val delayMillisecondsForQuery: (String) -> Long = { 0L },
    ) : uniffi.rustic_maps.SearchPlaceProvider {
        private val receivedQueries = CopyOnWriteArrayList<String>()

        @Volatile
        private var receivedCallCount: Int = 0

        /** How many type-ahead calls reached the fixture. */
        val callCount: Int
            get() = receivedCallCount

        /** The queries the fixture received, in call order. */
        val queries: List<String>
            get() = receivedQueries.toList()

        override suspend fun searchAutocomplete(
            queryText: String,
            searchBoundary: BoundingBox?,
        ): List<Place> {
            receivedCallCount += 1
            receivedQueries.add(queryText)
            val delayMilliseconds = delayMillisecondsForQuery(queryText)
            if (delayMilliseconds > 0) {
                delay(delayMilliseconds)
            }
            return resultsForQuery(queryText)
        }

        override suspend fun searchPlaces(
            queryText: String,
            searchBoundary: BoundingBox?,
        ): List<Place> = searchAutocomplete(queryText, searchBoundary)
    }

    /**
     * A routing provider registered from foreign: it answers every plan with
     * the shared artifact's committed demo route and records how many plans
     * reached it, so the capability proof runs without a network round trip.
     */
    private class FixtureRoutingProvider(
        private val committedRoute: Route,
    ) : uniffi.rustic_maps.RoutingProvider {
        private val receivedPlanCount = java.util.concurrent.atomic.AtomicInteger(0)

        /** How many plan calls reached the fixture. */
        val callCount: Int
            get() = receivedPlanCount.get()

        override suspend fun calculateRoute(
            waypoints: List<GeographicCoordinate>,
            routeOptions: RouteOptions,
        ): Route {
            receivedPlanCount.incrementAndGet()
            return committedRoute
        }
    }

    /** The committed flow row shape the traffic-layer cases install. */
    private fun sampleTrafficFlowSegments(): List<TrafficFlowData> = listOf(
        TrafficFlowData(
            shapeCoordinates = listOf(
                GeographicCoordinate(latitude = FLOW_SEGMENT_START_LATITUDE_DEGREES, longitude = FLOW_SEGMENT_START_LONGITUDE_DEGREES),
                GeographicCoordinate(latitude = FLOW_SEGMENT_END_LATITUDE_DEGREES, longitude = FLOW_SEGMENT_END_LONGITUDE_DEGREES),
            ),
            currentSpeedKilometersPerHour = FLOW_SEGMENT_CURRENT_SPEED_KILOMETERS_PER_HOUR,
            freeFlowSpeedKilometersPerHour = FLOW_SEGMENT_FREE_FLOW_SPEED_KILOMETERS_PER_HOUR,
            congestionLevel = TrafficFlowCongestionLevel.CONGESTED,
        ),
    )

    private companion object {
        /** The readiness poll's cadence. */
        const val READINESS_POLL_INTERVAL_MILLISECONDS: Long = 250

        /** The type-ahead suite's settle bound (one debounce plus a fixture call). */
        const val AUTOCOMPLETE_SETTLE_TIMEOUT_MILLISECONDS: Long = 10_000

        /**
         * The superseded query's fixture delay: longer than one quiet period,
         * so its answer is still in the provider's hands when the new query
         * starts.
         */
        const val AUTOCOMPLETE_SLOW_FIXTURE_DELAY_MILLISECONDS: Long = 800

        /** How long the case waits past the slow fixture's delay to see a late answer dropped. */
        const val AUTOCOMPLETE_SUPERSEDED_ANSWER_TIMEOUT_MILLISECONDS: Long = 1_500

        /** The cap case's query; the fixture answers it with more rows than the cap. */
        const val CAP_RESPONSE_QUERY_TEXT: String = "cap response"

        /** The query whose answer the newer query supersedes. */
        const val SUPERSEDED_QUERY_TEXT: String = "Alpha query"

        /** The newer query; its answer must be the one the field keeps. */
        const val CURRENT_QUERY_TEXT: String = "Beta query"

        /** The selection case's query; the fixture answers with the recorded pair. */
        const val SELECTION_QUERY_TEXT: String = "route pair"

        /** The query whose fixture answer raises the transport failure. */
        const val FAILING_QUERY_TEXT: String = "Fail me"

        /** The query the fixture answers with an empty list. */
        const val NO_MATCH_QUERY_TEXT: String = "No match"

        /** The query after the failure; a later keystroke retries. */
        const val WORKING_QUERY_TEXT: String = "Works"

        /** The fixture failure's verbatim detail, the field's one inline sentence. */
        const val AUTOCOMPLETE_FIXTURE_FAILURE_DETAIL_TEXT: String = "fixture provider unavailable"

        /** The fixture credential the settings-selection case seeds for its capability. */
        const val CREDENTIALED_SLOT_FIXTURE_CREDENTIAL: String = "fixture-credential"

        /** Every fixture suggestion's coordinate (a stable committed point). */
        const val AUTOCOMPLETE_FIXTURE_LATITUDE_DEGREES: Double = 38.576
        const val AUTOCOMPLETE_FIXTURE_LONGITUDE_DEGREES: Double = -121.494

        /**
         * One fixture suggestion row: the display line the field fills from
         * and a point-quality coordinate, the same row shape the contract
         * returns.
         */
        fun fixturePlace(displayText: String): Place = Place(
            structuredAddressText = displayText,
            coordinate = GeographicCoordinate(
                latitude = AUTOCOMPLETE_FIXTURE_LATITUDE_DEGREES,
                longitude = AUTOCOMPLETE_FIXTURE_LONGITUDE_DEGREES,
            ),
            geocodeQuality = GeocodeQuality.POINT,
        )

        /** The keyed live-provider flow's bound (one network round trip each). */
        const val LIVE_PROVIDER_TIMEOUT_MILLISECONDS: Long = 60_000

        /** The live search's query (a place the recorded fixtures also cover). */
        const val LIVE_SEARCH_QUERY_TEXT: String = "Sacramento,California"
        // The origin field's live query of the same provider (§4.1's two
        // fields; the fixture router serves both).
        const val LIVE_ORIGIN_SEARCH_QUERY_TEXT: String = "Sacramento,California"

        /** The application state construction's bound (the facade construction is synchronous). */
        const val APPLICATION_STATE_CONSTRUCTION_TIMEOUT_MILLISECONDS: Long = 30_000

        /**
         * The readiness ladder's settle bound on the emulator's software
         * graphics stack (the SwiftShader-over-ANGLE first accepted surface
         * frame is slower than hardware, minutes-scaled).
         */
        const val EMULATOR_FIRST_READY_TIMEOUT_MILLISECONDS: Long = 120_000

        /** The first drawn frame's bound (the ladder's ready can precede it). */
        const val FIRST_FRAME_DRAWN_TIMEOUT_MILLISECONDS: Long = 120_000

        /**
         * The styled vector base's serve bound: the SDK installs the source on
         * the next pump tick and the first vector frame fetches its visible
         * tiles, so this is bounded like a first frame (the emulator's software
         * graphics stack plus live vector tiles).
         */
        const val VECTOR_BASE_SERVE_TIMEOUT_MILLISECONDS: Long = 120_000

        /** The replay's first instruction/fix delivery bound. */
        const val TRIP_FIRST_INSTRUCTION_TIMEOUT_MILLISECONDS: Long = 10_000

        /** One overlay push's settle bound after a chip act. */
        const val OVERLAY_PUSH_SETTLE_TIMEOUT_MILLISECONDS: Long = 5_000

        /** The offline archive route plan's bound (the archive is 552 bytes). */
        const val OFFLINE_ROUTE_PLAN_TIMEOUT_MILLISECONDS: Long = 30_000

        /** The route line's own overlay slot in the pushed overlay list. */
        const val PLANNED_ROUTE_LINE_OVERLAY_COUNT: Int = 1

        /** One toggled scene contributes one overlay descriptor. */
        const val SCENE_OVERLAY_COUNT: Int = 1

        /** The remembered plan pair the persistence cases type. */
        const val REMEMBERED_ORIGIN_TEXT: String = "Capitol Park"
        const val REMEMBERED_DESTINATION_TEXT: String = "Tower Bridge"

        /** The artifact's traffic overlay scene key (the native proof's layer). */
        const val TRAFFIC_SCENE_KEY: String = "traffic"

        /** How long the emulator has to deliver a location fix. */
        const val DEVICE_LOCATION_FIX_TIMEOUT_MILLISECONDS: Long = 30_000

        /** How close the restored camera centre must be to the fix, in degrees. */
        const val DEVICE_CENTER_TOLERANCE_DEGREES: Double = 1e-3

        /** The edited destination text that clears its resolved place. */
        const val EDITED_DESTINATION_TEXT: String = "Tower Bridge, edited"

        /**
         * The contract's routing-slot name, the value its
         * `ProviderNotConfigured` detail carries when no routing provider is
         * installed.
         */
        const val ROUTING_CONTRACT_NAME: String = "RoutingProvider"

        /** The settings measurement's settle bound. */
        const val SETTINGS_MEASUREMENT_TIMEOUT_MILLISECONDS: Long = 30_000

        /** The layer panel's rows settle bound (a base switch refreshes them). */
        const val MAP_LAYERS_ROWS_SETTLE_TIMEOUT_MILLISECONDS: Long = 15_000

        /** The traffic layer's debounced first refresh bound (400 ms debounce). */
        const val TRAFFIC_OVERLAY_INSTALL_TIMEOUT_MILLISECONDS: Long = 20_000
        /** The traffic cache's settle readbacks (one debounce plus the call). */
        const val TRAFFIC_CACHE_SETTLE_TIMEOUT_MILLISECONDS: Long = 30_000
        /** A pan wide enough to cross the cache's zoom-12 grid cells. */
        const val TRAFFIC_CACHE_PAN_STEP_PIXELS: Double = 4_000.0
        /** The scripted pan's settle count. */
        const val TRAFFIC_CACHE_PAN_SETTLE_COUNT: Int = 5

        /** The panel close/focus handback's settle bound. */
        const val MAP_LAYERS_FOCUS_RETURN_TIMEOUT_MILLISECONDS: Long = 10_000

        /** A traffic-flow segment's minimum coordinate count (a polyline). */
        const val MINIMUM_FLOW_SEGMENT_COORDINATE_COUNT: Int = 2

        /** The synthetic flow segment's geometry and speeds. */
        const val FLOW_SEGMENT_START_LATITUDE_DEGREES: Double = 38.576
        const val FLOW_SEGMENT_START_LONGITUDE_DEGREES: Double = -121.494
        const val FLOW_SEGMENT_END_LATITUDE_DEGREES: Double = 38.578
        const val FLOW_SEGMENT_END_LONGITUDE_DEGREES: Double = -121.492
        const val FLOW_SEGMENT_CURRENT_SPEED_KILOMETERS_PER_HOUR: Double = 12.0
        const val FLOW_SEGMENT_FREE_FLOW_SPEED_KILOMETERS_PER_HOUR: Double = 40.0

        /** One pixel of rounding slack on the settings height cap. */
        const val SETTINGS_HEIGHT_TOLERANCE_PIXELS: Int = 1

        /** The seek assertion's tolerance on the requested distance, in meters. */
        const val SEEK_POSITION_TOLERANCE_METERS: Double = 1.0

        /** The camera read-back tolerance of the fit contract's zeroed values. */
        const val CAMERA_READBACK_TOLERANCE_DEGREES: Double = 0.0

        /** The markers scene's key in the shared artifact's overlay scenes. */
        const val MARKERS_SCENE_KEY: String = "markers"

        /** The rendered Navigating phase chip (ReferenceApplicationState's label). */
        const val NAVIGATING_PHASE_CHIP_TEXT: String = "Navigating"

        /** The offline card's live tile line prefix (§4.1's honest line). */
        const val OFFLINE_TILE_LINE_PREFIX: String = "Tiles: requested "

        /**
         * The readiness status line's ready prefix (the chip itself renders
         * the canonical "Ready"; the status line names the gpu renderer and
         * the compiled version).
         */
        const val READY_STATUS_TEXT_PREFIX: String = "Maps SDK gpu renderer active (version "

        /** The pan direction test's rightward drag (screen pixels). */
        const val PAN_DRAG_RIGHTWARD_PIXELS: Double = 100.0

        /** The pan direction test's downward drag (screen pixels). */
        const val PAN_DRAG_DOWNWARD_PIXELS: Double = 100.0

        /**
         * The pan direction's minimum camera-center move in degrees: a
         * 100-pixel drag at the launch camera's zoom 14 is hundreds of
         * ground meters, far above the readback's float noise.
         */
        const val PAN_MOVE_DEGREES_MINIMUM: Double = 1e-3

        /**
         * The pan direction's standstill tolerance on the untouched axis
         * (the mirrored pixel keeps the Mercator latitude; only projection
         * round-trip noise remains).
         */
        const val PAN_STANDSTILL_TOLERANCE_DEGREES: Double = 1e-6

        /** One tap's press duration (short enough for the tap recognizer). */
        const val TAP_PRESS_DURATION_MILLISECONDS: Long = 50

        /**
         * The gap between a double tap's first release and second press: above
         * the platform's double-tap minimum (40 ms) and well inside its 300 ms
         * window.
         */
        const val TAP_GAP_MILLISECONDS: Long = 80

        /** The confirmed-single-tap window's bound (the platform's 300 ms plus slack). */
        const val TAP_CONFIRMATION_TIMEOUT_MILLISECONDS: Long = 3_000

        /** One tap's count in the single-tap case. */
        const val SINGLE_TAP_COUNT: Int = 1

        /** A double tap is two presses. */
        const val DOUBLE_TAP_COUNT: Int = 2

        /** The camera zoom comparison's tolerance (a double tap is exactly one level). */
        const val CAMERA_ZOOM_TOLERANCE: Double = 1e-6

        /**
         * The gesture-cancel case's animation length: long enough that a
         * cancelled animation is unmistakably short of its target, while the
         * case still settles in seconds.
         */
        const val CAMERA_ANIMATION_DURATION_MILLISECONDS: Long = 6_000

        /** How far east of the launch camera the cancelled animation aims, in degrees. */
        const val CAMERA_ANIMATION_TARGET_LONGITUDE_OFFSET_DEGREES: Double = 8.0

        /** The progress that proves the animation is running before the gesture. */
        const val CAMERA_ANIMATION_PROGRESS_MINIMUM_DEGREES: Double = 0.02

        /** How long the running animation's first progress may take (software rendering). */
        const val CAMERA_ANIMATION_PROGRESS_TIMEOUT_MILLISECONDS: Long = 30_000

        /** How long the in-flight frame after the gesture may take to land. */
        const val CAMERA_ANIMATION_CANCEL_SETTLE_MILLISECONDS: Long = 600

        /** The stillness window: a running animation would adopt several poses here. */
        const val CAMERA_ANIMATION_STILLNESS_MILLISECONDS: Long = 1_500

        /** The cancelled pose's read-back tolerance (nothing should move at all). */
        const val CAMERA_ANIMATION_STILLNESS_TOLERANCE_DEGREES: Double = 1e-6

        /**
         * The minimum distance the cancelled animation must stay short of its
         * target; an uncancelled animation would arrive (zero remaining).
         */
        const val CAMERA_ANIMATION_TARGET_REMAINING_MINIMUM_DEGREES: Double = 1.0

        /** The map tap point's horizontal fraction of the view (centered). */
        const val MAP_TAP_HORIZONTAL_FRACTION: Float = 0.5f

        /** The map tap point's vertical fraction of the view (clear of the docks). */
        const val MAP_TAP_VERTICAL_FRACTION: Float = 0.35f
    }
}