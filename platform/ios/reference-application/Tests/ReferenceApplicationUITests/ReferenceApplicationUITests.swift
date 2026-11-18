//! The XCUITest suite of the SwiftUI reference application:
//! the iOS analogue of the Android suite's Espresso coverage
//! (`ReferenceApplicationAndroidTest.kt`) over the same demo-machinery
//! surfaces — the readiness ladder at the peek home sheet, the demo-plan
//! state the peek names with the half sheet's removed picker rows, the trace
//! replay's recorded banner and the verbatim exit restore, the simulated
//! trip's speed/pause/resume playback and its held arrival, the offline
//! card's registered tile cache with the live tile accounting, and the
//! map layers panel's base-map rows and scene switches with the labels the
//! vector styles draw.
//!
//! The driving rule mirrors the Android suite: no state is ever forced —
//! the state moves through the application's own launch-argument demo
//! automation (`ReferenceApplicationLaunchAutomation.swift`, the same
//! user-path methods the sheet's controls call) and XCUITest's own taps
//! and drags, and the assertions read the exact lines the accessibility
//! tree renders (docs/ReferenceAppUX.md §1/§4/§6/§7). Determinism rules:
//! every await is a poll on the rendered tree with a named timeout (never
//! a screenshot wait), no test touches the network (the recorded fixtures
//! and the bundled shared demo artifact are the data), and no test logs a
//! host-local configuration value.

import MapNativeCoreUI
import XCTest

/// The condition polls' default bound (the generic assertion settle; a
/// default argument cannot reference the suite's covariant `Self`, so the
/// value lives at file scope).
private let defaultAssertionTimeoutSeconds: TimeInterval = 15

/// The XCUITest suite over the reference application's demo
/// machinery.
final class ReferenceApplicationUITests: XCTestCase {

    // MARK: The suite's named constants

    /// The plan chip's accessibility identifier (plan-unique).
    private static func planChipIdentifier(forScenarioName scenarioName: String) -> String {
        "demo-plan-chip-" + scenarioName
    }

    /// A plan chip's info affordance identifier (plan-unique).
    private static func planInfoAffordanceIdentifier(forScenarioName scenarioName: String) -> String {
        "demo-plan-info-" + scenarioName
    }

    /// The recorded demo-plan scenarios (the committed recorded traces; the
    /// fixture metadata's own names). The half sheet no longer renders chips
    /// for them; the removal assertion covers every one.
    private static let planScenarioNames = [
        "sacramento_clean_walk",
        "sacramento_deviation_recovery",
        "sacramento_deviation_reroute",
    ]
    /// The artifact's own navigation demo trace scenario (the
    /// default-selected plan).
    private static let artifactPlanScenarioName = "sacramento_clean_walk"

    /// The §5.1 fitted camera's north-up readback row (the fit contract's
    /// zeroed bearing).
    private static let fittedBearingReadback = "bearing 0.0°"
    /// The §5.1 fitted camera's top-down readback row (the fit contract's
    /// zeroed pitch).
    private static let fittedPitchReadback = "pitch 0.0°"
    /// The zero overlay-triangles readback (no overlay geometry drew).
    private static let zeroOverlayTrianglesReadback = "overlay triangles 0"
    /// The diagnostics tiles block's overlay-triangles row prefix.
    private static let overlayTrianglesRowPrefix = "overlay triangles"

    /// The camera readback row prefixes (the diagnostics Camera block's
    /// fixed row order).
    private static let cameraReadbackRowPrefixes = [
        "center", "zoom", "bearing", "pitch",
    ]
    /// The camera readback rows' indexes in the fixed order.
    private static let bearingRowIndex = 2
    private static let pitchRowIndex = 3
    /// The zoom readback row's index (the `zoom <levels>` row).
    private static let zoomRowIndex = 1
    /// The diagnostics sheet's confirmed-tap count row prefix.
    private static let mapTapCountRowPrefix = "map taps "
    /// One double tap's zoom step (`docs/ReferenceAppUX.md` §5.2).
    private static let doubleTapZoomStep: Double = 1.0
    /// The zoom comparison's tolerance: the readback renders two decimals,
    /// so half the last rendered digit is the honest bound.
    private static let cameraZoomReadoutTolerance: Double = 0.005
    /// The map tap point's normalized offsets (the middle of the map at 35%
    /// of its height, clear of the chip row and the bottom dock).
    private static let mapTapHorizontalFraction: CGFloat = 0.5
    private static let mapTapVerticalFraction: CGFloat = 0.35

    /// The offline card's cache line prefix (§4.1's honest line).
    private static let offlineCacheStatePrefix = "Tile caching offline: "
    /// The named byte budget the registration phrases (16 MiB =
    /// 16777216 bytes).
    private static let cacheBudgetPhrase = "(16 MiB budget)"
    /// The demo's named tile-cache database file (the application's own
    /// constant, inside the caches directory — never the bundle, never
    /// git).
    private static let tileCacheDatabaseFileName = "tile-cache.db"
    /// The offline card's live tile line prefix (§4.1's honest line).
    private static let offlineTileLinePrefix = "Tiles last frame: "
    /// The live tile line's requested-count label (the accounting's first
    /// number).
    private static let offlineTileRequestedLabel = "requested "
    /// The offline card's line before the first drawn frame (the live
    /// line's negation).
    private static let offlineTileNoFrameYetText = "has not drawn yet"
    /// The offline card's group label (§4.2).
    private static let offlineMapsGroupLabelText = "Offline maps"
    /// The offline card's route control (§2.5 `offline.plan_route_action`).
    private static let offlineRouteButtonIdentifier = "offline-route-button"
    /// The offline route summary's title (§2.5).
    private static let offlineRouteTitleText = "Offline demo route"
    /// The offline route summary's honesty note (§2.5).
    private static let offlineRouteNoteText =
        "Planned offline from a recorded test graph, not live data."
    /// The routing-grid fixture's two-segment route distance and duration (the
    /// facts the wasm seam's test pins: two 111 m residential segments at
    /// 13.32 s each, formatted by the summary).
    private static let offlineRouteDistanceText = "222 m"
    private static let offlineRouteDurationText = "27 s"

    /// The offline card's region-download identifiers (the pack proof drives
    /// the card's own controls, never a forced state).
    private static let offlineRegionSummaryIdentifier = "offline-region-summary"
    private static let offlineRegionStatusIdentifier = "offline-region-status"
    private static let offlineRegionDeletedStatusIdentifier =
        "offline-region-deleted-status"
    private static let offlineRegionDownloadButtonIdentifier =
        "offline-region-download-button"
    private static let offlineRegionCancelButtonIdentifier =
        "offline-region-cancel-button"
    private static let offlineRegionDeleteIdentifierPrefix = "offline-region-delete-"
    private static let offlineRegionDeleteActionText = "Delete region"
    /// The in-flight line's leading text (the progress format's first slot).
    private static let offlineRegionDownloadingStatusPrefix = "Downloading…"
    /// The terminal lines the card states (§2.5 `offline.*_status`).
    private static let offlineRegionCompletedStatusText = "Download complete"
    private static let offlineRegionCancelledStatusText =
        "Download cancelled; the stored tiles are kept for a later download."
    private static let offlineRegionDeletedStatusText = "Region deleted"
    /// The progress line's network-versus-disk split fragments (the format's
    /// third and fourth slots).
    private static let offlineRegionNetworkFetchedFragment = "fetched from the network"
    private static let offlineRegionDiskServedFragment = "from disk"
    /// The diagnostics Traffic cache block's cells row prefix (`cells 1 of 64`).
    private static let trafficCacheCellsRowPrefix = "cells "

    /// The launch credential restore's settle before the traffic switch is
    /// driven (the catalog provider installation is asynchronous).
    private static let trafficCredentialSettleSeconds: TimeInterval = 1

    /// A stored-region row's two count lines.
    private static let offlineRegionStoredTilesPrefix = "Stored tiles "
    private static let offlineRegionStoredBytesPrefix = "Stored bytes "
    /// The first progress report's bound: the region's member tiles arrive one
    /// provider request each, so the first completion settles inside it.
    private static let offlineRegionFirstProgressTimeoutSeconds: TimeInterval = 60
    /// The completed download's bound (23 provider requests on the standing
    /// network).
    private static let offlineRegionDownloadCompletionTimeoutSeconds: TimeInterval = 180

    /// The half sheet's content scroll view (the scroll-into-view helper's
    /// container).
    private static let halfSheetContentIdentifier = "home-sheet-half-content"

    /// The condition poll's cadence (seconds).
    private static let pollIntervalSeconds: TimeInterval = 0.25
    /// The element settles' bound (an act's rendered consequence).
    private static let settleTimeoutSeconds: TimeInterval = 10
    /// The turn banner's delivery bound (the replay's first instruction).
    private static let bannerDeliveryTimeoutSeconds: TimeInterval = 20
    /// The readiness ladder's settle bound on the simulator's graphics
    /// stack (the Android suite's minutes-scaled honest rule; the
    /// simulator's Metal stack settles far sooner in practice).
    private static let emulatorFirstReadyTimeoutSeconds: TimeInterval = 120
    /// The first drawn frame's bound (the ladder's ready can precede it).
    private static let firstFrameDrawnTimeoutSeconds: TimeInterval = 120
    /// The scroll-into-view helper's drag count bound.
    private static let maximumScrollAttempts = 12
    /// The search-result reveal's drag count bound.
    private let maximumSearchRevealAttempts = 4
    /// The sheet grabber's fold drag distance (points; the sheet's own
    /// fold threshold in the design's platform glue is 24).
    private static let sheetFoldDragPoints: CGFloat = 40

    /// The §5.1 direction tests' launch-argument keys (the application's
    /// demo-automation flags: the pan and rotation demos drive the drag
    /// and rotation gestures' own user-path methods).
    private static let launchArgumentGesturePanDemo = "DemoGesturePanContent"
    private static let launchArgumentTwoFingerRotateDemo = "DemoGestureRotateContent"

    /// The launch-argument key that starts a route-following trip over the
    /// selected demo plan's committed route (the contract's shared walker,
    /// network-free — the playback suite's seam).
    private static let launchArgumentRouteFollowingJourney = "DemoStartRouteFollowingJourney"

    /// The playback suite's named values: the fastest offered chip, the
    /// arrival's settle bound, and the window a paused reading must hold
    /// still.
    private static let fastestSpeedMultiplier = 8
    private static let arrivalTimeoutSeconds: TimeInterval = 30
    private static let pausedReadingStabilitySeconds: TimeInterval = 2

    /// The documented `simctl location` stream's first waypoint, which the
    /// location cases expect the camera to centre on.
    private static let locationStreamLatitudeDegrees = 38.5766
    private static let locationStreamLongitudeDegrees = -121.4944
    /// The fix tolerance: the documented stream advances over a long run, so
    /// the drawn center is allowed to drift within this bound.
    private static let locationFixToleranceDegrees = 0.02

    /// The launch preset's camera (the shared artifact's fixture center —
    /// the model's initial camera values). The direction assertions read
    /// against it.
    private static let launchCameraCenterLatitudeDegrees = 38.5766
    private static let launchCameraCenterLongitudeDegrees = -121.4944

    /// The pan demo's minimum camera-center westward move (degrees of
    /// longitude): the demo's 100-point drag trips ~300 physical pixels at
    /// the launch camera's zoom, whose longitude move renders far above
    /// the readback's six-decimal rendering.
    private static let panDemoMinimumLongitudeMoveDegrees = 0.001
    /// A pure-rightward drag's latitude standstill tolerance (the mirrored
    /// pixel keeps the Mercator latitude; only round-trip noise remains).
    private static let panDragLatitudeStandstillToleranceDegrees = 1e-6
    /// The rotate demo's drawn bearing readback: a 45° clockwise finger
    /// turn must lower the launch 0.0° bearing to its 315.0° reflection
    /// (content follows the fingers; the §5.1 direction contract).
    private static let rotatedContentFollowBearingReadback = "bearing 315.0°"

    /// The launch-argument keys the suite drives state with (the
    /// application's documented demo-automation flags, `-key value` form
    /// — the argument domain's boolean keys need the explicit value).
    private static let launchArgumentSheetHalfState = "DemoSetSheetHalf"

    /// The launch-argument key that opens the application on a base map by
    /// provider key (the application's documented `-DemoBaseMap
    /// <provider-key>` flag — the styled-base case selects the registry's
    /// credential-less demo base map through it, reusing the existing seam).
    private static let launchArgumentDemoBaseMap = "DemoBaseMap"

    /// The launch-argument key that selects a recorded demo plan by its
    /// artifact scenario name (the application's documented
    /// `-DemoPlanScenario <scenario-name>` flag — the picker's own selection
    /// act, now that the visible picker rows are gone).
    private static let launchArgumentDemoPlanScenario = "DemoPlanScenario"

    /// The non-default recorded plan the launch-selection case drives: the
    /// reroute trace, whose preview framing differs from the default preset.
    private static let launchSelectedDemoPlanScenarioName = "sacramento_deviation_reroute"

    /// The launch-argument key that clears the remembered choices at startup
    /// (the application's documented `-DemoResetRememberedChoices YES` flag).
    private static let launchArgumentResetRememberedChoices = "DemoResetRememberedChoices"

    /// The remembered search texts the persistence cases type.
    private static let rememberedOriginText = "Capitol Park"
    private static let rememberedDestinationText = "Tower Bridge"

    /// The settings sheet's height tolerance in points (the detent's own
    /// rounding).
    private static let settingsHeightTolerancePoints: CGFloat = 2

    /// The common map-control diameter check's point tolerance (layout
    /// rounding on the rendered accessibility frames).
    private static let controlDiameterTolerancePoints: CGFloat = 1

    /// The base-map section's label line while a styled vector base serves
    /// (the application's fixed consumer string; the suite asserts the exact
    /// line the screen renders).
    private static let vectorLabelsNoteText =
        "Vector styles draw land, water, roads with their names, and Latin place names."

    /// The suffix the base-map menu appends to the serving vector style's
    /// display name (the application's own serving marker).
    private static let vectorStyleServingSuffix = " · Serving"

    /// The separator between a vector style's provider name and its style
    /// name in the base-map menu (the application's `VectorBaseMapChoice`
    /// display composition).
    private static let vectorStyleNameSeparator = " · "

    /// The settings sheet's base-map menu identifier.
    private static let baseMapMenuIdentifier = "base-map-menu"

    /// The routing capability's registry name (the row this suite selects in
    /// the per-capability settings section).
    private static let routingCapabilityName = "routing"

    /// The launch-environment key that points the keyless routing provider at
    /// the loopback fixture server.
    private static let routingDeploymentBaseURLEnvironmentKey =
        "ROUTING_PROVIDER_BASE_URL"

    /// The committed OSRM route payload the routing proof serves from that
    /// fixture server (the same file the provider crate's tests parse).
    private static let osrmRouteFixtureDocumentName = "sacramento_route"

    /// The request path prefix the OSRM provider appends to its base URL.
    private static let routingRequestPathPrefix = "/route/v1/driving/"

    /// The launch-argument value that reads as `true` in the argument
    /// domain's boolean parsing.
    private static let launchArgumentBooleanTrueValue = "YES"

    /// The launch-environment key the application reads for the keyless
    /// search provider's deployment address.
    private static let deploymentBaseURLEnvironmentKey = "SEARCH_PROVIDER_BASE_URL"
    /// A loopback address nothing listens on. The cases here type into the
    /// plan fields without exercising type-ahead, so their keyless provider
    /// must fail locally instead of reaching the registry's public demo
    /// server; no request leaves the simulator.
    private static let unreachableDeploymentBaseURL = "http://127.0.0.1:1"

    /// The application under test, relaunched per test.
    private var application: XCUIApplication?

    /// The automation credential the UI suite injects, if any.
    ///
    /// The suite is hermetic by policy: it never injects a live provider
    /// credential, so every case runs keyless over the committed fixtures and
    /// the shared demo artifacts. The credential-dependent cases keep their
    /// keyed branch documented, but it is unreachable here — a live provider
    /// call from the UI suite would break both the no-network rule and the
    /// keyless distribution rule. The opt-in screenshot capture harness is a
    /// separate case and supplies its own session credential.
    private var runtimeInjectedApiKey: String? { nil }

    /// The optional provider key paired with the automation credential; nil
    /// lets the application target its registry's only iOS-available
    /// provider.
    /// The optional provider key paired with the automation credential.
    ///
    /// The suite never injects a credential, so it never injects the key that
    /// would target one either.
    private var runtimeInjectedProviderKey: String? { nil }

    /// Whether this run supplies the runtime-injected provider credential.
    private var hasRuntimeInjectedApiKey: Bool { runtimeInjectedApiKey != nil }

    // MARK: The suite's lifecycle

    override func setUpWithError() throws {
        try super.setUpWithError()
        continueAfterFailure = false
    }

    override func tearDown() {
        application?.terminate()
        application = nil
        super.tearDown()
    }

    // MARK: Test 1 — the readiness ladder at the peek home sheet

    /// The Espresso suite's launch test: the application launches to the
    /// ready ladder at the peek home sheet — the readiness chip names the
    /// ready state, no planned search and no trip run, the error strip is
    /// empty (an honest startup), and Auto is the theme default (§3).
    func testApplicationLaunchesToTheReadyLadderAtThePeekHomeSheet() throws {
        let application = launchApplication(launchArguments: [])

        if hasRuntimeInjectedApiKey {
            // §6.1: the ladder settles on the ready state (the chip's short
            // line; the full text lives in the diagnostics sheet).
            awaitReadinessChipReady(application: application)
            XCTAssertEqual(
                readinessChip(application: application).label, "Ready",
                "the readiness chip must name the ready state"
            )
        } else {
            // §6.5: a keyless launch shows either the neutral prompt or, when
            // the registry carries a credential-less base map, the ready
            // line that serving map earns. Both are documented states; a
            // renderer failure or an endless loading line is neither.
            let settledChipPredicate = NSPredicate(
                format: "label == %@ OR label == %@", "Set provider key", "Ready"
            )
            expectation(
                for: settledChipPredicate,
                evaluatedWith: readinessChip(application: application)
            )
            waitForExpectations(timeout: Self.settleTimeoutSeconds)
        }
        XCTAssertFalse(
            elementWithIdentifier("error-report", in: application).exists,
            "an honest startup reports nothing in the error strip"
        )

        // §1: the home sheet starts at peek — the destination search field
        // and the one primary action row; no navigation drawer replaces
        // the sheet and no banner stack runs a trip.
        XCTAssertTrue(
            elementWithIdentifier("home-sheet-peek-search", in: application).exists,
            "the peek home sheet's destination search field must render"
        )
        let peekStartNavigation = elementWithIdentifier(
            "home-sheet-peek-start-navigation", in: application
        )
        XCTAssertTrue(
            peekStartNavigation.exists,
            "the peek home sheet's primary action row must render"
        )
        XCTAssertEqual(
            peekStartNavigation.label,
            "Start simulated navigation",
            "the peek's one primary action starts the simulated trip (§1)"
        )
        XCTAssertFalse(
            elementWithIdentifier("navigation-drawer", in: application).exists,
            "no trip runs at launch, so no navigation drawer replaces the sheet"
        )
        XCTAssertFalse(
            elementWithIdentifier("navigation-turn-banner", in: application).exists,
            "no trip runs at launch, so no turn banner renders"
        )

        // §3: Auto is the theme default on a device that never chose, so the
        // toggle's label names the choice it switches to next (light). The
        // half sheet's control is the accessibility tree's own default
        // witness.
        dragSheetGrabber(application: application, direction: .up)
        XCTAssertTrue(
            elementWithIdentifier("theme-toggle", in: application)
                .waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the half sheet's theme row must render"
        )
        XCTAssertEqual(
            elementWithIdentifier("theme-toggle", in: application).label,
            "Switch to light theme",
            "Auto is the default choice, so the toggle names the next choice (light)"
        )
        dragSheetGrabber(application: application, direction: .down)
        XCTAssertTrue(
            elementWithIdentifier("home-sheet-peek-search", in: application)
                .waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the grabber's down gesture restores the peek fold state"
        )
    }

    // MARK: Test 2 — the keyless demo search and recorded route

    /// The keyless plan flow (§4.1): with no provider credential applied,
    /// the search view filters the shared artifact's `demo_places` and plans
    /// the artifact's recorded route for the pair — the same demo data the
    /// web page serves, never a provider call. The two launch-argument demos
    /// drive the search view's own user-path methods (the suite's
    /// no-touch-injection rule); the assertions read the rendered rows and
    /// the route summary's recorded-route note.
    func testTheKeylessDemoSearchAndRecordedRouteNeedNoProviderKey() throws {
        try XCTSkipIf(
            hasRuntimeInjectedApiKey,
            "the keyless demo flow needs a credential-less session"
        )
        let catalog = try ReferenceScenesCatalog.loaded(
            from: Bundle(for: ReferenceApplicationUITests.self)
        )
        let firstDemoPlace = try XCTUnwrap(
            catalog.demoPlaces.first,
            "the shared artifact must carry demo places"
        )
        let recordedRoute = try XCTUnwrap(
            catalog.demoRoutes.first,
            "the shared artifact must carry recorded demo routes"
        )
        XCTAssertGreaterThan(
            recordedRoute.route.distanceMeters, 0,
            "the recorded route must carry its geometry"
        )

        // The keyless search offers the artifact's demo places and says the
        // rows are demo places, never live results.
        let searchApplication = launchApplication(launchArguments: [
            "-DemoKeylessSearch", "YES",
        ])
        XCTAssertTrue(
            elementWithIdentifier("search-results-demo-heading", in: searchApplication)
                .waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the keyless result list must name its rows as demo places"
        )
        XCTAssertTrue(
            searchApplication.staticTexts[firstDemoPlace.name]
                .waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the keyless search must offer the artifact's demo place"
        )
        searchApplication.terminate()

        // The keyless plan applies the recorded pair's route and the summary
        // names it as the recorded demo route.
        let planApplication = launchApplication(launchArguments: [
            "-DemoKeylessPlan", "YES",
        ])
        XCTAssertTrue(
            elementWithIdentifier("route-summary-card", in: planApplication)
                .waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the keyless plan must present the recorded route's summary"
        )
        XCTAssertTrue(
            planApplication.staticTexts["Recorded demo route, pedestrian, fastest."].exists,
            "the route summary must name the recorded demo route"
        )
        XCTAssertFalse(
            elementWithIdentifier("error-report", in: planApplication).exists,
            "a successful keyless plan reports nothing in the error strip"
        )
    }

    // MARK: Test 3 — the sheet without demo rows and the layer panel's scenes

    /// The owner-directed layout revision: the half sheet no longer renders
    /// demo-plan or demo-scene rows. The demo data and the default selection
    /// stay (the peek names the selected recorded plan), and the map layers
    /// panel is the single scene home: its switch joins and leaves the pushed
    /// overlay list — assertable on iOS through the live tile accounting's
    /// overlay triangles line, the drawn-frame witness of the pushed overlays.
    func testTheHalfSheetLeavesScenesToTheLayerPanelAndKeepsTheDemoPlanState() throws {
        try XCTSkipUnless(
            hasRuntimeInjectedApiKey,
            "requires PROVIDER_API_KEY in the test environment (runtime injection)"
        )
        let application = launchApplication(launchArguments: [])
        awaitReadinessChipReady(application: application)

        // The demo-plan state still selects the artifact's trace: the peek's
        // trip-source line names it while no planned route exists.
        XCTAssertTrue(
            elementWithIdentifier("peek-trip-source", in: application)
                .waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the peek must still name the selected demo plan"
        )

        // The removed rows stay removed: the half sheet carries no plan chip,
        // no plan info affordance and no scene chip.
        dragSheetGrabber(application: application, direction: .up)
        let halfSheetContent = elementWithIdentifier(
            Self.halfSheetContentIdentifier, in: application
        )
        XCTAssertTrue(
            halfSheetContent.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the grabber's up gesture must open the half sheet"
        )
        for planScenarioName in Self.planScenarioNames {
            XCTAssertFalse(
                elementWithIdentifier(
                    Self.planChipIdentifier(forScenarioName: planScenarioName),
                    in: application
                ).exists,
                "the half sheet must not render a demo-plan chip"
            )
            XCTAssertFalse(
                elementWithIdentifier(
                    Self.planInfoAffordanceIdentifier(
                        forScenarioName: planScenarioName
                    ),
                    in: application
                ).exists,
                "the half sheet must not render a demo-plan info affordance"
            )
        }
        XCTAssertFalse(
            elementWithIdentifier("demo-scene-chip", in: application).exists,
            "the half sheet must not render demo-scene chips"
        )

        // Back to the peek so the layer panel opens clear of the sheet.
        dragSheetGrabber(application: application, direction: .down)

        // The overlay baseline before the scene switch's own act.
        openDiagnosticsSheet(application: application)
        let overlayTrianglesLine = awaitOverlayTrianglesLine(application: application)
        XCTAssertFalse(
            overlayTrianglesLine.isEmpty,
            "the diagnostics tiles block must render the overlay triangles line"
        )
        closeDiagnostics(application: application)

        // The layers panel is the single scene home: the markers switch
        // toggles the artifact's scene on, and the pushed overlay list's
        // drawn-frame witness changes; toggling it off restores the line.
        openMapLayersPanel(in: application)
        let layersPanel = elementWithIdentifier("map-layers-panel", in: application)
        let markersSceneSwitch = elementWithIdentifier(
            "map-layer-scene-markers", in: application
        )
        scrollToMake(markersSceneSwitch, visibleIn: layersPanel)
        XCTAssertTrue(
            markersSceneSwitch.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the layer panel must render the artifact's markers scene switch"
        )
        markersSceneSwitch.tap()
        closeMapLayersPanel(in: application)
        let markersOnOverlayTrianglesLine = openDiagnosticsAndReadChanged(
            application: application,
            fromLine: overlayTrianglesLine
        )
        openMapLayersPanel(in: application)
        let reopenedLayersPanel = elementWithIdentifier(
            "map-layers-panel", in: application
        )
        let reopenedMarkersSceneSwitch = elementWithIdentifier(
            "map-layer-scene-markers", in: application
        )
        scrollToMake(reopenedMarkersSceneSwitch, visibleIn: reopenedLayersPanel)
        reopenedMarkersSceneSwitch.tap()
        closeMapLayersPanel(in: application)
        reopenDiagnosticsAndAwaitRestored(
            application: application,
            fromLine: markersOnOverlayTrianglesLine,
            toLine: overlayTrianglesLine
        )
        XCTAssertFalse(
            elementWithIdentifier("error-report", in: application).exists,
            "a scene push reports nothing in the error strip"
        )
    }

    // MARK: Test 3b — the demo-plan launch selection and the §5.1 fit contract

    /// The demo-plan selection's launch seam: with the visible picker rows
    /// gone, the application selects a recorded plan through the same
    /// `selectDemoPlan` act the chips called. The selection applies the §5.1
    /// route-preview framing (top-down, north up, the fit actually applied)
    /// and its route line rides the pushed overlay list.
    func testTheRecordedDemoPlanLaunchSelectionAppliesThePreviewFitAndRouteLine() throws {
        try XCTSkipUnless(
            hasRuntimeInjectedApiKey,
            "requires PROVIDER_API_KEY in the test environment (runtime injection)"
        )
        defer { resetRememberedChoicesAfterTest() }

        // The default start's framing (the artifact preset), read before the
        // selection so the fit's camera move has a baseline to differ from.
        let baselineApplication = launchApplication(launchArguments: [])
        awaitReadinessChipReady(application: baselineApplication)
        let baselineCameraRows = openDiagnosticsAndReadCameraRows(
            application: baselineApplication
        )
        XCTAssertTrue(
            baselineCameraRows.allSatisfy { !$0.isEmpty },
            "the baseline camera readback must render all four rows,\(baselineCameraRows)"
        )
        baselineApplication.terminate()

        // The launch seam's own act: the artifact's reroute plan is selected
        // through `selectDemoPlan`, which drops any planned route, fits the
        // recorded route and pushes its route line.
        let application = launchApplication(launchArguments: [
            "-\(Self.launchArgumentDemoPlanScenario)",
            Self.launchSelectedDemoPlanScenarioName,
        ])
        awaitReadinessChipReady(application: application)
        XCTAssertFalse(
            elementWithIdentifier("route-summary-card", in: application).exists,
            "a recorded demo plan is not a search-planned route"
        )
        XCTAssertFalse(
            elementWithIdentifier("navigation-drawer", in: application).exists,
            "selecting a recorded plan must not start a trip"
        )

        // §5.1's fit contract: the preview's framing is top down (pitch
        // 0.0°) and north up (bearing 0.0°), and the fit actually applied —
        // the drawn frames' camera readback reports the pinned fit camera,
        // which moved the framing away from the default readback.
        let fittedCameraRows = openDiagnosticsAndReadCameraRows(
            application: application,
            settleCondition: { cameraRows in
                cameraRows != baselineCameraRows
                    && cameraRows[Self.bearingRowIndex] == Self.fittedBearingReadback
                    && cameraRows[Self.pitchRowIndex] == Self.fittedPitchReadback
            },
            closesSheetAfterReading: false
        )
        XCTAssertEqual(
            fittedCameraRows[Self.bearingRowIndex], Self.fittedBearingReadback,
            "the route-preview framing must be north up"
        )
        XCTAssertEqual(
            fittedCameraRows[Self.pitchRowIndex], Self.fittedPitchReadback,
            "the route-preview framing must be top down"
        )
        XCTAssertNotEqual(
            fittedCameraRows, baselineCameraRows,
            "the route-preview framing must actually move the framing (the §5.1 fit applied)"
        )

        // The plan's route line rides the pushed overlay list (the tiles
        // block's live overlay triangles row is its drawn-frame witness).
        let overlayTrianglesLine = awaitOverlayTrianglesLine(application: application)
        XCTAssertNotEqual(
            overlayTrianglesLine, Self.zeroOverlayTrianglesReadback,
            "the selected plan's route line must ride the pushed overlay list"
        )
        closeDiagnostics(application: application)
        XCTAssertFalse(
            elementWithIdentifier("error-report", in: application).exists,
            "a plan selection reports nothing in the error strip"
        )
    }

    // MARK: Test 3 — the trace replay and the verbatim exit restore

    /// The Espresso suite's trace-replay test: the recorded journey's
    /// fixes replay through the navigation controller; the §7 banner
    /// renders a recorded step's display text (the recording's own text,
    /// distance-prefixed in the composed form); the drawer runs the
    /// Navigating phase; and the exit control's act stops the replay,
    /// restores the pre-trip camera verbatim (the attribute-getter
    /// readback is the drawn-frame witness) and returns the peek home
    /// sheet.
    func testTheTraceReplayRendersTheRecordedBannerTextAndTheExitRestoresHome() throws {
        // The diagnostics camera readback is the drawn-frame witness; a
        // keyless session has no surface and therefore no camera to read
        // back. The replay itself is provider-free, but the assertions need
        // the drawn camera, so the test runs in keyed runs only.
        try XCTSkipUnless(
            hasRuntimeInjectedApiKey,
            "requires PROVIDER_API_KEY in the test environment (runtime injection)"
        )
        let application = launchApplication(launchArguments: [])
        awaitReadinessChipReady(application: application)

        // The pre-trip camera readback, from the diagnostics sheet's
        // camera block.
        let preTripCameraRows = openDiagnosticsAndReadCameraRows(application: application)
        XCTAssertTrue(
            preTripCameraRows.allSatisfy { !$0.isEmpty },
            "the pre-trip camera readback must render all four rows,\(preTripCameraRows)"
        )

        // The recorded demo must carry steps for the banner membership
        // assertion (the test bundle decodes the same committed fixture
        // the application replays).
        let recordedStepDisplayTexts = try Self.recordedStepDisplayTexts(
            fixtureScenarioNamed: Self.artifactPlanScenarioName
        )
        XCTAssertFalse(
            recordedStepDisplayTexts.isEmpty,
            "the recorded demo trace must carry steps to assert against"
        )

        // The half sheet's sample journey control starts the simulated
        // journey (the recorded trace replays through the navigation
        // controller).
        dragSheetGrabber(application: application, direction: .up)
        let sampleJourneyButton = elementWithIdentifier(
            "sample-journey-button", in: application
        )
        XCTAssertTrue(
            sampleJourneyButton.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the half sheet's sample journey control must render"
        )
        sampleJourneyButton.tap()

        // §1: the navigation drawer replaces the home sheet during the
        // trip; its phase chip runs the Navigating phase.
        let navigationDrawer = elementWithIdentifier("navigation-drawer", in: application)
        XCTAssertTrue(
            navigationDrawer.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "a started trip must replace the home sheet with the navigation drawer"
        )
        let navigationPhaseChip = elementWithIdentifier(
            "navigation-drawer-phase", in: application
        )
        XCTAssertTrue(
            navigationPhaseChip.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the drawer's phase chip must render"
        )
        XCTAssertEqual(
            elementWithIdentifier("navigation-drawer-phase", in: application).label,
            "Navigating",
            "the replay's drawer must run the Navigating phase"
        )

        // §7.1/§7.2: the turn banner renders a recorded step's composed
        // text — the banner's maneuver line ends with the recording's own
        // provider text (the live distance prefix composes onto it).
        let turnBanner = elementWithIdentifier("navigation-turn-banner", in: application)
        XCTAssertTrue(
            turnBanner.waitForExistence(timeout: Self.bannerDeliveryTimeoutSeconds),
            "the replay's first instruction must raise the turn banner"
        )
        let renderedBannerTexts = Self.bannerLabelTexts(banner: turnBanner)
        XCTAssertFalse(
            renderedBannerTexts.isEmpty,
            "the turn banner must render the composed instruction"
        )
        XCTAssertTrue(
            recordedStepDisplayTexts.contains { stepText in
                renderedBannerTexts.contains { $0.hasSuffix(stepText) }
            },
            "the banner's rendered lines \(renderedBannerTexts) must end with the recording's own step text"
        )
        XCTAssertFalse(
            elementWithIdentifier("navigation-arrival-banner", in: application).exists,
            "the replay's first instruction is not the arrival"
        )

        // The drawer's exit control's act: the replay stops, the home
        // sheet returns at peek.
        let drawerExitButton = elementWithIdentifier(
            "navigation-drawer-exit", in: application
        )
        XCTAssertTrue(
            drawerExitButton.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the drawer's exit control must render"
        )
        drawerExitButton.tap()
        XCTAssertTrue(
            elementWithIdentifier("home-sheet-peek-search", in: application)
                .waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the exit must return the home sheet at peek"
        )
        pollForSettlement(
            timeoutDescription: "the stopped trip removing the navigation drawer and banner"
        ) {
            !navigationDrawer.exists && !turnBanner.exists
        }

        // §5.3's "applied at trip start, restored after": the drawn
        // frames' camera readback returns exactly to the pre-trip row set
        // (the verbatim restore).
        let restoredCameraRows = openDiagnosticsAndReadCameraRows(
            application: application,
            settleCondition: { cameraRows in
                cameraRows == preTripCameraRows
            }
        )
        XCTAssertEqual(
            restoredCameraRows, preTripCameraRows,
            "the exit must restore the pre-trip camera verbatim"
        )
    }

    // MARK: Test 3b — the playback's speed, pause/resume and held arrival

    /// The playback test: a route-following trip walks the contract's
    /// shared walker over a committed route (network-free through the
    /// launch automation). The drawer offers the speed row; the fastest
    /// chip accelerates the walk; Pause freezes the reading while the
    /// drawer's phase stays Navigating; Resume carries the trip to the
    /// arrival state, which holds the destination with the playback
    /// controls hidden until Exit returns the peek home sheet.
    func testTheSimulatedTripFollowsItsCourseAndOffersNorthUp() throws {
        // The recorded walk heads south-west from the start, so a camera that
        // follows the smoothed course leaves north by more than the compass
        // threshold and shows the N control. A north-up camera (a broken
        // bearing path) would never show it.
        let demoBaseMapProviderKey = (try? loadProviderRegistry())?.providers.first { descriptor in
            descriptor.registrationAvailable.ios
                && !descriptor.rasterUrlTemplate.trimmingCharacters(
                    in: .whitespacesAndNewlines
                ).isEmpty
        }?.providerKey
        var launchArguments = [
            "-DemoStartSimulatedJourney",
            Self.launchArgumentBooleanTrueValue,
        ]
        if let demoBaseMapProviderKey {
            launchArguments += [
                "-\(Self.launchArgumentDemoBaseMap)", demoBaseMapProviderKey,
            ]
        }
        let application = launchApplication(launchArguments: launchArguments)

        let navigationDrawer = elementWithIdentifier("navigation-drawer", in: application)
        XCTAssertTrue(
            navigationDrawer.waitForExistence(timeout: Self.emulatorFirstReadyTimeoutSeconds),
            "the simulated trip must present the navigation drawer"
        )
        let compassControl = elementWithIdentifier("map-compass-control", in: application)
        XCTAssertTrue(
            compassControl.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "a course-following trip must show the N compass once its bearing leaves north"
        )
    }

    func testTheHomeSheetPointsToTheLayerPanelForDeviceLocation() throws {
        let application = launchApplication(launchArguments: [
            "-\(Self.launchArgumentSheetHalfState)",
            Self.launchArgumentBooleanTrueValue,
        ])
        let locationPointer = elementWithIdentifier(
            "device-location-pointer", in: application
        )
        XCTAssertTrue(
            locationPointer.waitForExistence(timeout: Self.emulatorFirstReadyTimeoutSeconds),
            "the half sheet must point at the layer panel for device location"
        )
        XCTAssertEqual(
            locationPointer.label,
            "Device location in Map layers",
            "the settings row is a one-line pointer to the map's layer panel"
        )
    }

    // MARK: The on-map layer control

    /// The common control diameter: the zoom cluster, the layers control and
    /// the attribution info control draw the same circle size, so the map's
    /// floating controls read as one set.
    func testTheMapControlsShareOneDiameter() throws {
        let application = launchApplication(launchArguments: [])
        awaitReadinessChipReady(application: application)
        let zoomInControl = elementWithIdentifier(
            "map-control-zoom-in", in: application
        )
        let layersControl = elementWithIdentifier(
            "map-layers-control", in: application
        )
        let attributionControl = elementWithIdentifier(
            "attribution-info-control", in: application
        )
        for control in [zoomInControl, layersControl, attributionControl] {
            XCTAssertTrue(
                control.waitForExistence(timeout: Self.settleTimeoutSeconds),
                "the map controls and the attribution must render"
            )
        }
        let referenceDiameter = zoomInControl.frame.width
        XCTAssertEqual(
            zoomInControl.frame.height, referenceDiameter,
            accuracy: Self.controlDiameterTolerancePoints,
            "the zoom control must draw a circle"
        )
        for control in [layersControl, attributionControl] {
            XCTAssertEqual(
                control.frame.width, referenceDiameter,
                accuracy: Self.controlDiameterTolerancePoints,
                "every map control must share the cluster's diameter"
            )
            XCTAssertEqual(
                control.frame.height, referenceDiameter,
                accuracy: Self.controlDiameterTolerancePoints,
                "every map control must share the cluster's diameter"
            )
        }
    }

    /// The layer control opens the panel and switches the base map: a
    /// raster row takes the map, then a committed vector style takes it back,
    /// and the serving mark follows each choice.
    func testTheLayerPanelSwitchesTheBaseMap() throws {
        let registry = try loadProviderRegistry()
        guard let rasterDescriptor = registry.providers.first(where: { descriptor in
            descriptor.isCredentiallessBaseMapAvailableOnIOS
        }) else {
            throw XCTSkip("the registry declares no credential-less raster base")
        }
        guard let keylessStyle = registry.keylessBaseMap else {
            throw XCTSkip("the registry declares no keyless committed style")
        }
        let application = launchApplication(
            launchArguments: [],
            injectsRuntimeCredential: false
        )
        openMapLayersPanel(in: application)

        let rasterRow = elementWithIdentifier(
            "map-layer-base-\(rasterDescriptor.providerKey)", in: application
        )
        XCTAssertTrue(
            rasterRow.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the panel must offer the registry's credential-less raster source"
        )
        rasterRow.tap()
        XCTAssertTrue(
            waitForLabelContaining("Serving", element: rasterRow),
            "the selected raster source must carry the Serving mark"
        )

        let styleRow = elementWithIdentifier(
            "map-layer-style-\(keylessStyle.styleId)", in: application
        )
        XCTAssertTrue(
            styleRow.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the panel must offer the registry's committed vector styles"
        )
        styleRow.tap()
        XCTAssertTrue(
            waitForLabelContaining("Serving", element: styleRow),
            "the selected vector style must carry the Serving mark"
        )

        // Vector → vector: a second committed style of the same descriptor
        // takes the base, and the serving mark moves with it rather than
        // remaining on the previous style.
        guard let keylessDescriptor = registry.providers.first(where: { descriptor in
            descriptor.providerKey == keylessStyle.providerKey
        }),
            let secondStyle = keylessDescriptor.vectorStyles.first(where: { style in
                style.styleId != keylessStyle.styleId
            })
        else {
            throw XCTSkip("the keyless descriptor offers no second committed style")
        }
        let secondStyleRow = elementWithIdentifier(
            "map-layer-style-\(secondStyle.styleId)", in: application
        )
        XCTAssertTrue(
            secondStyleRow.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the panel must offer the descriptor's second committed style"
        )
        secondStyleRow.tap()
        XCTAssertTrue(
            waitForLabelContaining("Serving", element: secondStyleRow),
            "the second style must carry the Serving mark after the change"
        )
        pollForSettlement(
            timeoutDescription: "the previous style's Serving mark leaving the row"
        ) {
            !styleRow.label.contains(Self.vectorStyleServingSuffix)
        }

        // Vector → raster: the raster row takes the base back and the vector
        // style rows stop claiming the serving mark.
        rasterRow.tap()
        XCTAssertTrue(
            waitForLabelContaining("Serving", element: rasterRow),
            "the raster source must serve again after the vector base"
        )
        pollForSettlement(
            timeoutDescription: "the vector style rows leaving the Serving mark"
        ) {
            !secondStyleRow.label.contains(Self.vectorStyleServingSuffix)
        }
    }

    /// A keyless run cannot pretend the traffic layer is on: the switch is
    /// disabled and names the credential the serving descriptor needs.
    func testTheTrafficSwitchStatesItsMissingCredentialKeyless() throws {
        let registry = try loadProviderRegistry()
        guard let trafficDescriptor = registry.providers.first(where: { descriptor in
            descriptor.servedContracts.contains("traffic_flow")
        }) else {
            throw XCTSkip("the registry declares no traffic-flow descriptor")
        }
        let requiresCredential = trafficDescriptor.registrationAvailable.ios
            && !trafficDescriptor.credentialLabel.trimmingCharacters(
                in: .whitespacesAndNewlines
            ).isEmpty
        try XCTSkipUnless(
            requiresCredential,
            "the registry's traffic descriptor is credential-less"
        )
        let application = launchApplication(
            launchArguments: [],
            injectsRuntimeCredential: false
        )
        // A remembered opt-in credential from an earlier run would make the
        // switch available; the clear act returns this session to keyless.
        try clearAppliedProviderCredentials(in: application)
        openMapLayersPanel(in: application)

        let trafficSwitch = elementWithIdentifier("map-layer-traffic", in: application)
        XCTAssertTrue(
            trafficSwitch.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the panel must render the traffic switch"
        )
        XCTAssertFalse(
            trafficSwitch.isEnabled,
            "a keyless run cannot turn the traffic layer on"
        )
        let reason = elementWithIdentifier("map-layer-traffic-reason", in: application)
        XCTAssertTrue(
            reason.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the switch must state why it cannot turn on"
        )
        XCTAssertEqual(
            reason.label,
            "Needs \(trafficDescriptor.credentialLabel)",
            "the switch must name the credential the descriptor declares"
        )
    }

    // MARK: Test 4d — the traffic cache's contract readback

    /// The readback on iOS: turning the traffic layer on makes the
    /// catalog provider answer through the grid cache, and the diagnostics
    /// sheet's Traffic cache block reports that answer — one provider call
    /// (a miss), no hit yet, and a held-cell count inside the budget the
    /// readback carries. Key-gated: the catalog provider needs the runtime
    /// credential.
    func testTheTrafficCacheReportsItsCountersInTheDiagnosticsSheet() throws {
        try XCTSkipUnless(
            hasRuntimeInjectedApiKey,
            "requires PROVIDER_API_KEY in the test environment (runtime injection)"
        )
        let application = launchApplication(
            launchArguments: [
                "-\(Self.launchArgumentSheetHalfState)",
                Self.launchArgumentBooleanTrueValue,
            ]
        )
        awaitReadinessChipReady(application: application)
        openMapLayersPanel(in: application)
        let trafficSwitch = elementWithIdentifier("map-layer-traffic", in: application)
        XCTAssertTrue(
            trafficSwitch.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the panel must render the traffic switch"
        )
        XCTAssertTrue(
            trafficSwitch.isEnabled,
            "an applied credential makes the traffic switch available"
        )
        // The launch credential restore settles before the switch is driven:
        // an early tap can land while the catalog provider is still being
        // installed, and the layer then turns itself off on the refusal.
        Thread.sleep(forTimeInterval: Self.trafficCredentialSettleSeconds)
        trafficSwitch.tap()
        // The switch's own reported value is the settle witness: the layer
        // turns on synchronously, then its first refresh runs after the
        // debounce.
        pollForSettlement(timeoutDescription: "the traffic layer switching on") {
            (trafficSwitch.value as? String) == "On"
        }
        closeMapLayersPanel(in: application)

        openDiagnosticsSheet(application: application)
        let sheet = diagnosticsSheet(application: application)
        let missesRow = application.staticTexts["misses 1"]
        scrollToMake(missesRow, visibleIn: sheet)
        pollForSettlement(
            timeoutDescription: "the first settle's traffic cache counters",
            timeoutSeconds: Self.firstFrameDrawnTimeoutSeconds
        ) {
            missesRow.exists
        }
        XCTAssertTrue(
            missesRow.exists,
            "the catalog provider's first settle must be one cache miss"
        )
        XCTAssertTrue(
            application.staticTexts["hits 0"].exists,
            "the first settle answers nothing from the cache"
        )
        let cellsRow = application.staticTexts.matching(
            NSPredicate(format: "label BEGINSWITH %@", Self.trafficCacheCellsRowPrefix)
        ).firstMatch
        XCTAssertTrue(cellsRow.exists, "the block must state the held cells beside the budget")
        let cellCounts = try XCTUnwrap(
            Self.trafficCacheCellCounts(from: cellsRow.label),
            "the cells row must carry its held and budget counts, '\(cellsRow.label)'"
        )
        XCTAssertGreaterThan(cellCounts.held, 0, "the first settle stores its cell")
        XCTAssertGreaterThan(cellCounts.budget, 0)
        XCTAssertLessThanOrEqual(
            cellCounts.held, cellCounts.budget,
            "the cache must never hold more cells than its budget"
        )
        closeDiagnostics(application: application)
    }

    /// A traffic cache cells row's held and budget counts from
    /// `cells 1 of 64`.
    private static func trafficCacheCellCounts(
        from rowLabel: String
    ) -> (held: UInt64, budget: UInt64)? {
        let components = rowLabel.split(separator: " ")
        guard components.count == 4,
              components[0] == "cells",
              components[2] == "of",
              let held = UInt64(components[1]),
              let budget = UInt64(components[3])
        else {
            return nil
        }
        return (held: held, budget: budget)
    }

    /// The round my-location control's off state starts the source: granting
    /// the permission lets it start itself, the first fix centres the camera
    /// and the panel's Device location row reads Active.
    ///
    /// Run with a location stream that keeps issuing fixes for the whole
    /// suite: `xcrun simctl location booted start --interval=1 --speed=0.5
    /// 38.5766,-121.4944 39.5,-121.4944`.
    func testTheRoundLocationControlStartsTheSourceAndCentresTheCamera() throws {
        let application = launchApplication(
            launchArguments: [],
            injectsRuntimeCredential: false
        )
        awaitReadinessChipReady(application: application)
        let roundControl = elementWithIdentifier(
            "map-control-my-location", in: application
        )
        XCTAssertTrue(
            roundControl.waitForExistence(timeout: Self.emulatorFirstReadyTimeoutSeconds),
            "the map control cluster must offer the my-location control"
        )
        roundControl.tap()
        grantWhenInUseLocationAuthorization(in: application)

        openMapLayersPanel(in: application)
        let locationStatus = elementWithIdentifier(
            "device-location-status", in: application
        )
        XCTAssertTrue(
            waitForLabel("Active", element: locationStatus),
            "the panel's Device location row must read Active after the round control's tap, got '\(locationStatus.exists ? locationStatus.label : "absent")'"
        )
        closeMapLayersPanel(in: application)

        let cameraRows = openDiagnosticsAndReadCameraRows(application: application)
        let centerRow = cameraRows[0]
        let centerComponents = centerRow
            .dropFirst("center ".count)
            .split(separator: ",")
            .map { component in
                Double(component.trimmingCharacters(in: .whitespaces)) ?? .nan
            }
        XCTAssertEqual(centerComponents.count, 2, "the center row must carry both axes")
        XCTAssertEqual(
            centerComponents.first ?? .nan,
            Self.locationStreamLatitudeDegrees,
            accuracy: Self.locationFixToleranceDegrees,
            "the first fix must centre the drawn camera, got '\(centerRow)'"
        )
        XCTAssertEqual(
            centerComponents.last ?? .nan,
            Self.locationStreamLongitudeDegrees,
            accuracy: Self.locationFixToleranceDegrees,
            "the first fix must centre the drawn camera, got '\(centerRow)'"
        )
        // The tracking indicator is renderer-owned and has no accessibility
        // element; the drawn camera and the Active row are its witnesses.

        // The panel's Stop action ends the source and reports the off state.
        openMapLayersPanel(in: application)
        let stopControl = elementWithIdentifier("device-location-stop", in: application)
        XCTAssertTrue(
            stopControl.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "a running source must offer the panel's Stop action"
        )
        stopControl.tap()
        XCTAssertTrue(
            waitForLabel(
                "Off",
                element: elementWithIdentifier("device-location-status", in: application)
            ),
            "stopping the source must clear the device-location row"
        )
    }

    /// Each "My location" chip fills only its own field: the origin chip
    /// leaves the destination empty, and the destination chip fills it.
    ///
    /// Run with the same location stream as the round-control case.
    func testEachMyLocationChipFillsOnlyItsOwnField() throws {
        let application = launchApplication(
            launchArguments: [
                "-DemoOpenSearch", Self.launchArgumentBooleanTrueValue,
            ],
            injectsRuntimeCredential: false
        )
        let originChip = elementWithIdentifier("origin-my-location", in: application)
        XCTAssertTrue(
            originChip.waitForExistence(timeout: Self.emulatorFirstReadyTimeoutSeconds),
            "the origin field must carry its My location chip"
        )
        XCTAssertEqual(
            originChip.label,
            "Use my location as the origin",
            "the chip's accessibility label must name its field"
        )
        originChip.tap()
        grantWhenInUseLocationAuthorization(in: application)

        let originField = application.textFields["Search an origin"]
        XCTAssertTrue(
            originField.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the search view's origin field must render"
        )
        XCTAssertTrue(
            waitForTextFieldValueContaining(",", textField: originField),
            "the origin chip must fill the origin field from the fix"
        )
        let destinationField = application.textFields["Search a destination"]
        XCTAssertTrue(
            destinationField.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the search view's destination field must render"
        )
        XCTAssertEqual(
            textFieldValue(destinationField),
            "",
            "the origin chip must not fill the destination field"
        )

        let destinationChip = elementWithIdentifier(
            "destination-my-location", in: application
        )
        XCTAssertTrue(
            destinationChip.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the destination field must carry its My location chip"
        )
        XCTAssertEqual(
            destinationChip.label,
            "Use my location as the destination",
            "the destination chip's label must name its field"
        )
        destinationChip.tap()
        XCTAssertTrue(
            waitForTextFieldValueContaining(",", textField: destinationField),
            "the destination chip must fill the destination field from the latest fix"
        )
    }

    func testThePlanFlowGatesItsRouteActionUntilBothFieldsHoldText() throws {
        let application = launchApplication(launchArguments: [
            "-DemoOpenSearch",
            Self.launchArgumentBooleanTrueValue,
        ])
        let planRouteButton = elementWithIdentifier("plan-route-button", in: application)
        XCTAssertTrue(
            planRouteButton.waitForExistence(timeout: Self.emulatorFirstReadyTimeoutSeconds)
        )
        XCTAssertFalse(
            planRouteButton.isEnabled,
            "Plan route stays disabled with both fields empty"
        )
        let reason = elementWithIdentifier("plan-route-reason", in: application)
        XCTAssertTrue(reason.waitForExistence(timeout: Self.settleTimeoutSeconds))
        XCTAssertEqual(
            reason.label,
            "Enter an origin and a destination to plan the route."
        )

        let originField = application.textFields["Search an origin"]
        XCTAssertTrue(originField.waitForExistence(timeout: Self.settleTimeoutSeconds))
        originField.tap()
        originField.typeText("Capitol Park")
        XCTAssertFalse(
            planRouteButton.isEnabled,
            "Plan route stays disabled while the destination is empty"
        )
        XCTAssertEqual(
            reason.label,
            "Enter a destination to plan the route."
        )

        let destinationField = application.textFields["Search a destination"]
        destinationField.tap()
        destinationField.typeText("Tower Bridge")
        XCTAssertTrue(
            planRouteButton.isEnabled,
            "Plan route enables once both fields hold text"
        )
    }

    /// The Directions control: Car and Fastest are selected by
    /// default, the strategy row shows for a car plan, and choosing Walk
    /// hides it and changes the mode the plan action carries. The keyless
    /// plan applies the artifact's recorded route, so the diagnostics row
    /// names the recorded path honestly beside the selected options.
    func testDirectionsDefaultToCarAndWalkChangesThePlansMode() throws {
        let application = launchApplication(
            launchArguments: [
                "-DemoOpenSearch", Self.launchArgumentBooleanTrueValue,
            ],
            injectsRuntimeCredential: false
        )
        let carChoice = elementWithIdentifier(
            "directions-mode-car", in: application
        )
        XCTAssertTrue(
            carChoice.waitForExistence(timeout: Self.emulatorFirstReadyTimeoutSeconds),
            "the plan sheet must offer the Car mode"
        )
        XCTAssertTrue(
            waitForSelected(carChoice),
            "Car must be the selected mode by default"
        )
        let walkChoice = elementWithIdentifier(
            "directions-mode-walk", in: application
        )
        XCTAssertTrue(walkChoice.waitForExistence(timeout: Self.settleTimeoutSeconds))
        XCTAssertFalse(walkChoice.isSelected, "Walk starts unselected")
        XCTAssertTrue(
            elementWithIdentifier("directions-strategy-row", in: application)
                .waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the strategy row shows for the default car plan"
        )
        XCTAssertTrue(
            waitForSelected(
                elementWithIdentifier(
                    "directions-strategy-fastest", in: application
                )
            ),
            "Fastest must be the selected strategy by default"
        )

        walkChoice.tap()
        XCTAssertTrue(waitForSelected(walkChoice))
        XCTAssertFalse(carChoice.isSelected)
        XCTAssertTrue(
            waitForNonexistence(
                elementWithIdentifier("directions-strategy-row", in: application)
            ),
            "a walk plan hides the strategy row"
        )

        // The plan action carries the selected mode; a keyless session
        // applies the artifact's recorded route and the diagnostics state
        // that honestly.
        planRecordedDemoRoute(application: application)
        let routeOptionsRow = readDirectionsDiagnosticsRow(application: application)
        XCTAssertTrue(
            routeOptionsRow.contains("pedestrian"),
            "choosing Walk must change the plan action's mode, got '\(routeOptionsRow)'"
        )
        XCTAssertTrue(
            routeOptionsRow.contains("recorded route"),
            "the keyless plan's diagnostics must state the recorded path, got '\(routeOptionsRow)'"
        )
    }

    func testTheDeviceLocationSourceShowsALiveFix() throws {
        // The simulator must have a location set before this run
        // (`xcrun simctl location booted set 38.5766,-121.4944`) and the
        // authorization must be undetermined (reset it to see the prompt).
        let application = launchApplication(launchArguments: [
            "-DemoStartDeviceLocation",
            Self.launchArgumentBooleanTrueValue,
            "-\(Self.launchArgumentSheetHalfState)",
            Self.launchArgumentBooleanTrueValue,
        ])
        let authorizationAlert = application.alerts.firstMatch
        if authorizationAlert.waitForExistence(timeout: 5) {
            for allowLabel in ["Allow While Using App", "Allow Once", "Allow"] {
                let allowButton = authorizationAlert.buttons[allowLabel]
                if allowButton.exists {
                    allowButton.tap()
                    break
                }
            }
        } else {
            let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
            for allowLabel in ["Allow While Using App", "Allow Once", "Allow"] {
                let allowButton = springboard.buttons[allowLabel]
                if allowButton.waitForExistence(timeout: 2) {
                    allowButton.tap()
                    break
                }
            }
        }

        // The source starts itself once the prompt is granted; the layer
        // panel's Device location row states it.
        openMapLayersPanel(in: application)
        let locationStatus = elementWithIdentifier(
            "device-location-status", in: application
        )
        XCTAssertTrue(
            waitForLabel("Active", element: locationStatus),
            "the panel's Device location row must read Active once a fix arrives, got '\(locationStatus.exists ? locationStatus.label : "absent")'"
        )
    }

    /// The Location group's second control: with no planned route it
    /// states what it needs and starts nothing; on the recorded
    /// Capitol Park -> Tower Bridge route the device source's own fixes
    /// drive the banner, phase and follow presentation, and stopping leaves
    /// the source the visitor started by hand running.
    func testDeviceNavigationFollowsThePlannedRouteFromLiveFixes() throws {
        // The simulator must carry a location stream before this run
        // (`xcrun simctl location start --speed=2 --interval=1
        // 38.57693,-121.49492 38.7,-121.49492`). A slow, long path keeps
        // fixes arriving for the whole case; the default 20 m/s crosses these
        // short waypoints in seconds. The authorization must be grantable
        // (reset it to see the prompt). The case stays keyless: the plan flow
        // reads the recorded demo route, never a provider.
        let application = launchApplication(
            launchArguments: [
                "-DemoStartDeviceLocation", Self.launchArgumentBooleanTrueValue,
                "-\(Self.launchArgumentSheetHalfState)", Self.launchArgumentBooleanTrueValue,
            ],
            injectsRuntimeCredential: false
        )
        grantWhenInUseLocationAuthorization(in: application)

        // The source starts itself once the prompt resolves; the layer
        // panel's Device location row reports the live source before the trip.
        openMapLayersPanel(in: application)
        XCTAssertTrue(
            waitForLabel(
                "Active",
                element: elementWithIdentifier("device-location-status", in: application)
            ),
            "the device source must report a live fix before the trip starts"
        )
        closeMapLayersPanel(in: application)
        XCTAssertTrue(
            elementWithIdentifier("device-location-pointer", in: application)
                .waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the half sheet's device-location pointer returns after the panel closes"
        )

        // Without a planned route the control states what it needs instead
        // of starting a trip nobody planned.
        let navigationStatus = elementWithIdentifier(
            "device-navigation-status", in: application
        )
        let navigationControl = elementWithIdentifier("device-navigation-control", in: application)
        XCTAssertTrue(navigationControl.waitForExistence(timeout: Self.settleTimeoutSeconds))
        XCTAssertEqual(navigationControl.label, "Navigate from my location")
        navigationControl.tap()
        XCTAssertTrue(
            waitForLabel(
                "Plan a route first; device navigation follows the route you planned.",
                element: navigationStatus
            ),
            "the keyless plan-first caption must state what device navigation needs"
        )
        XCTAssertEqual(
            navigationControl.label, "Navigate from my location",
            "the control stays idle while no route is planned"
        )

        // Plan the recorded route through the search view's own fields.
        dragSheetGrabber(application: application, direction: .up)
        planRecordedDemoRoute(application: application)
        let routeSummary = elementWithIdentifier("route-summary-card", in: application)
        XCTAssertTrue(
            routeSummary.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the recorded Capitol Park -> Tower Bridge route must present its summary"
        )

        // The source is already running, so the control starts guidance on
        // the planned route from its next accepted fix.
        let readyNavigationControl = elementWithIdentifier(
            "device-navigation-control", in: application
        )
        XCTAssertTrue(readyNavigationControl.waitForExistence(timeout: Self.settleTimeoutSeconds))
        readyNavigationControl.tap()

        let stopControl = elementWithIdentifier("device-navigation-control", in: application)
        XCTAssertTrue(
            waitForLabel("Stop device navigation", element: stopControl),
            "the control must name the active device-driven trip"
        )
        XCTAssertTrue(
            elementWithIdentifier("navigation-drawer", in: application)
                .waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the device-driven trip presents the navigation drawer"
        )
        XCTAssertTrue(
            elementWithIdentifier("navigation-turn-banner", in: application)
                .waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the turn banner reads the device fix stream"
        )
        let phaseChip = elementWithIdentifier("navigation-drawer-phase", in: application)
        XCTAssertTrue(phaseChip.waitForExistence(timeout: Self.settleTimeoutSeconds))
        XCTAssertTrue(
            waitForNonemptyLabel(element: phaseChip),
            "the trip phase is shown while the device fixes drive guidance"
        )
        let drawerStatus = elementWithIdentifier(
            "navigation-drawer-device-navigation-status", in: application
        )
        XCTAssertTrue(
            waitForLabelContaining("Position", element: drawerStatus),
            "the accepted device fix keeps the position caption current, got '\(drawerStatus.exists ? drawerStatus.label : "absent")'"
        )

        // Stopping ends the trip; the source the visitor started by hand
        // keeps running, so the half sheet still points at the layer panel
        // and the panel still reads the live source.
        stopControl.tap()
        XCTAssertTrue(
            waitForNonexistence(elementWithIdentifier("navigation-drawer", in: application)),
            "stopping device navigation ends the trip presentation"
        )
        XCTAssertTrue(
            elementWithIdentifier("sheet-grabber", in: application)
                .waitForExistence(timeout: Self.settleTimeoutSeconds)
        )
        dragSheetGrabber(application: application, direction: .up)
        XCTAssertTrue(
            elementWithIdentifier("device-location-pointer", in: application)
                .waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the half sheet's device-location pointer returns after the trip"
        )
        XCTAssertEqual(
            elementWithIdentifier("device-navigation-control", in: application).label,
            "Navigate from my location",
            "the control returns to its idle label after stopping"
        )
        // The hand-started source keeps running: the layer panel still reads
        // Active and still offers its Stop action.
        openMapLayersPanel(in: application)
        XCTAssertEqual(
            elementWithIdentifier("device-location-status", in: application).label,
            "Active",
            "the hand-started location source must keep running after the trip"
        )
        XCTAssertTrue(
            elementWithIdentifier("device-location-stop", in: application).exists,
            "the running source keeps the panel's Stop action"
        )
    }

    func testTheSimulatedTripPausesResumesAndHoldsItsArrival() throws {
        // The trip itself needs no provider, but the arrival's centred-
        // destination assertion reads the drawn camera. The registry's
        // credential-less demo base map (the same seam the styled-base case
        // launches with) gives the keyless run a surface to read.
        let demoBaseMapProviderKey = (try? loadProviderRegistry())?.providers.first { descriptor in
            descriptor.registrationAvailable.ios
                && !descriptor.rasterUrlTemplate.trimmingCharacters(
                    in: .whitespacesAndNewlines
                ).isEmpty
        }?.providerKey
        var launchArguments = [
            "-\(Self.launchArgumentRouteFollowingJourney)",
            Self.launchArgumentBooleanTrueValue,
        ]
        if let demoBaseMapProviderKey {
            launchArguments += [
                "-\(Self.launchArgumentDemoBaseMap)", demoBaseMapProviderKey,
            ]
        }
        let application = launchApplication(launchArguments: launchArguments)

        // §1: the trip replaces the home sheet with the navigation drawer.
        let navigationDrawer = elementWithIdentifier("navigation-drawer", in: application)
        XCTAssertTrue(
            navigationDrawer.waitForExistence(timeout: Self.emulatorFirstReadyTimeoutSeconds),
            "the route-following trip must present the navigation drawer"
        )
        let phaseChip = elementWithIdentifier("navigation-drawer-phase", in: application)
        XCTAssertTrue(
            phaseChip.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the running trip's phase chip must render"
        )
        XCTAssertEqual(
            phaseChip.label, "Navigating",
            "the route-following trip must run the Navigating phase"
        )
        XCTAssertFalse(
            elementWithIdentifier("navigation-drawer-arrived", in: application).exists,
            "the trip starts short of its arrival"
        )

        // The speed row carries one chip per offered multiplier; the fastest
        // one is tapped so the committed walk arrives inside the suite's
        // bound.
        let speedRow = elementWithIdentifier("navigation-speed-row", in: application)
        XCTAssertTrue(
            speedRow.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the drawer's playback-speed row must render"
        )
        let fastestSpeedChip = elementWithIdentifier(
            "navigation-speed-chip-\(Self.fastestSpeedMultiplier)", in: application
        )
        XCTAssertTrue(
            fastestSpeedChip.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the fastest offered speed chip must render"
        )
        XCTAssertEqual(
            fastestSpeedChip.label, "\(Self.fastestSpeedMultiplier)×",
            "the chip carries the canonical multiplier label"
        )
        fastestSpeedChip.tap()

        // The pause act is asserted where the reading is fine-grained: the
        // ETA line switches from kilometers to meters under one kilometer,
        // where a running trip moves the number every tick.
        let etaLine = elementWithIdentifier("navigation-drawer-eta-line", in: application)
        pollForSettlement(
            timeoutDescription: "the trip's remaining distance entering the meters phrase",
            timeoutSeconds: Self.arrivalTimeoutSeconds
        ) {
            etaLine.exists && etaLine.label.contains(" m ·")
        }
        let pauseControl = elementWithIdentifier("navigation-playback-pause", in: application)
        XCTAssertTrue(
            pauseControl.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the drawer's pause control must render"
        )
        XCTAssertEqual(pauseControl.label, "Pause", "the running trip offers Pause")
        pauseControl.tap()
        pollForSettlement(
            timeoutDescription: "the paused playback control reading Resume"
        ) {
            pauseControl.exists && pauseControl.label == "Resume"
        }
        let pausedEtaLine = etaLine.label
        Thread.sleep(forTimeInterval: Self.pausedReadingStabilitySeconds)
        XCTAssertEqual(
            etaLine.label, pausedEtaLine,
            "a paused trip's reading must freeze at its current fix"
        )
        XCTAssertEqual(
            phaseChip.label, "Navigating",
            "pausing the playback does not end the trip"
        )

        // Resume: the reading moves again and the trip reaches its arrival.
        pauseControl.tap()
        pollForSettlement(
            timeoutDescription: "the resumed playback control reading Pause"
        ) {
            pauseControl.exists && pauseControl.label == "Pause"
        }
        let arrivedLine = elementWithIdentifier("navigation-drawer-arrived", in: application)
        XCTAssertTrue(
            arrivedLine.waitForExistence(timeout: Self.arrivalTimeoutSeconds),
            "the trip must reach its arrival state within the playback bound"
        )
        XCTAssertEqual(arrivedLine.label, "Arrived", "the arrival drawer says Arrived")
        XCTAssertFalse(
            elementWithIdentifier("navigation-playback-pause", in: application).exists,
            "the arrival state hides the playback control"
        )
        XCTAssertFalse(
            elementWithIdentifier("navigation-speed-row", in: application).exists,
            "the arrival state hides the speed row"
        )
        XCTAssertFalse(
            elementWithIdentifier("home-sheet-peek-search", in: application).exists,
            "the arrival presentation keeps the trip in place"
        )

        // §7.1/§7.2: the arrival outlives the playback (only Exit empties it).
        Thread.sleep(forTimeInterval: Self.pausedReadingStabilitySeconds)
        XCTAssertTrue(arrivedLine.exists, "the arrival state must hold")
        XCTAssertTrue(navigationDrawer.exists, "the arrival state must hold its drawer")

        // The destination stays the camera centre (the drawn-frame readback;
        // the launch needs a surface to read, keyed or the keyless demo base).
        if hasRuntimeInjectedApiKey || demoBaseMapProviderKey != nil {
            let destination = try XCTUnwrap(
                Self.recordedRouteEndCoordinate(
                    fixtureScenarioNamed: Self.artifactPlanScenarioName
                ),
                "the committed route fixture must carry a final shape coordinate"
            )
            let expectedCenterRow = "center "
                + String(format: "%.6f", destination.latitude) + ", "
                + String(format: "%.6f", destination.longitude)
            // The camera block can sit below the sheet's fold, so the poll
            // reads the rows through the scroll-aware read.
            openDiagnosticsSheet(application: application)
            let diagnosticsSheetElement = diagnosticsSheet(application: application)
            let diagnosticsScrollView = diagnosticsSheetElement.scrollViews.firstMatch
            pollForSettlement(
                timeoutDescription:
                    "the arrival's drawn camera centre reading the destination"
            ) {
                readCameraReadbackRowsWithScrolling(
                    sheet: diagnosticsSheetElement,
                    scrollView: diagnosticsScrollView
                ).first == expectedCenterRow
            }
            let cameraRows = readCameraReadbackRowsWithScrolling(
                sheet: diagnosticsSheetElement,
                scrollView: diagnosticsScrollView
            )
            closeDiagnostics(application: application)
            XCTAssertEqual(
                cameraRows.first, expectedCenterRow,
                "the arrival must hold the route's destination as the camera centre, was \(cameraRows)"
            )
        }

        // The exit's act: the trip tears down and the peek home sheet returns.
        elementWithIdentifier("navigation-drawer-exit", in: application).tap()
        XCTAssertTrue(
            elementWithIdentifier("home-sheet-peek-search", in: application)
                .waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the exit must return the home sheet at peek"
        )
        XCTAssertTrue(
            pollForNonExistence(navigationDrawer, within: Self.settleTimeoutSeconds),
            "the exit must remove the navigation drawer"
        )
    }

    // MARK: Test 3c — the playback's seek

    /// The seek test: the drawer's scrubber moves the trip along the
    /// route's own geometry. Seeking to the end presents the walker's own
    /// arrival; seeking back leaves it and the paused trip stays paused.
    func testTheSimulatedTripSeeksToTheEndAndBackOutOfTheArrival() throws {
        var launchArguments = [
            "-\(Self.launchArgumentRouteFollowingJourney)",
            Self.launchArgumentBooleanTrueValue,
        ]
        let demoBaseMapProviderKey = (try? loadProviderRegistry())?.providers.first { descriptor in
            descriptor.registrationAvailable.ios
                && !descriptor.rasterUrlTemplate.trimmingCharacters(
                    in: .whitespacesAndNewlines
                ).isEmpty
        }?.providerKey
        if let demoBaseMapProviderKey {
            launchArguments += [
                "-\(Self.launchArgumentDemoBaseMap)", demoBaseMapProviderKey,
            ]
        }
        let application = launchApplication(launchArguments: launchArguments)

        let navigationDrawer = elementWithIdentifier("navigation-drawer", in: application)
        XCTAssertTrue(
            navigationDrawer.waitForExistence(timeout: Self.emulatorFirstReadyTimeoutSeconds),
            "the route-following trip must present the navigation drawer"
        )
        let seekSlider = elementWithIdentifier("navigation-seek-slider", in: application)
        XCTAssertTrue(
            seekSlider.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the route-following trip's seek control must render"
        )
        // Pause first, so each seek's own position is what the assertions
        // read (a running walker legitimately moves on).
        let pauseControl = elementWithIdentifier("navigation-playback-pause", in: application)
        XCTAssertTrue(
            pauseControl.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the drawer's pause control must render"
        )
        pauseControl.tap()
        pollForSettlement(
            timeoutDescription: "the paused playback control reading Resume"
        ) {
            pauseControl.exists && pauseControl.label == "Resume"
        }

        let arrivedLine = elementWithIdentifier("navigation-drawer-arrived", in: application)
        XCTAssertFalse(arrivedLine.exists, "the trip starts short of its arrival")
        seekSlider.adjust(toNormalizedSliderPosition: 1.0)
        XCTAssertTrue(
            arrivedLine.waitForExistence(timeout: Self.arrivalTimeoutSeconds),
            "seeking to the end must present the walker's arrival"
        )
        XCTAssertEqual(arrivedLine.label, "Arrived", "the arrival drawer says Arrived")

        // Backwards: the seek leaves the arrival and the paused trip holds
        // the new position.
        seekSlider.adjust(toNormalizedSliderPosition: 0.5)
        pollForSettlement(
            timeoutDescription: "a backwards seek leaving the arrival state"
        ) {
            !arrivedLine.exists
        }
        let phaseChip = elementWithIdentifier("navigation-drawer-phase", in: application)
        XCTAssertTrue(
            phaseChip.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the trip returns to its navigating contents"
        )
        XCTAssertEqual(
            phaseChip.label, "Navigating",
            "a backwards seek leaves the arrived state"
        )
        let pausedEtaLine = elementWithIdentifier("navigation-drawer-eta-line", in: application).label
        Thread.sleep(forTimeInterval: Self.pausedReadingStabilitySeconds)
        XCTAssertEqual(
            elementWithIdentifier("navigation-drawer-eta-line", in: application).label,
            pausedEtaLine,
            "seeking while paused keeps the trip paused at the new position"
        )
    }

    // MARK: Test 4 — the offline card's registered cache and live accounting

    /// The Espresso suite's offline-card test: the two honest lines
    /// reflect the application's own registration (the registered cache's
    /// path and the named byte budget) and the live tile statistics of
    /// the drawn frames.
    func testTheOfflineCardReflectsTheRegisteredTileCacheAndTheLiveTileAccounting() throws {
        try XCTSkipUnless(
            hasRuntimeInjectedApiKey,
            "requires PROVIDER_API_KEY in the test environment (runtime injection)"
        )
        let application = launchApplication(
            launchArguments: [
                "-\(Self.launchArgumentSheetHalfState)",
                Self.launchArgumentBooleanTrueValue,
            ]
        )
        awaitReadinessChipReady(application: application)

        let halfSheetContent = elementWithIdentifier(
            Self.halfSheetContentIdentifier, in: application
        )
        XCTAssertTrue(
            halfSheetContent.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the half sheet's working content must render"
        )

        // §4.1: the offline card's group row names the feature (rows name
        // the feature, never the seam).
        let offlineMapsGroupLabel = application.staticTexts[
            Self.offlineMapsGroupLabelText
        ]
        scrollToMake(offlineMapsGroupLabel, visibleIn: halfSheetContent)
        XCTAssertTrue(
            offlineMapsGroupLabel.exists,
            "the half sheet's offline maps card must render"
        )

        // The registered cache's line: the honest path and the byte
        // budget the application's own registration ran with.
        let offlineCacheStateLine = elementWithIdentifier(
            "offline-cache-state", in: application
        )
        scrollToMake(offlineCacheStateLine, visibleIn: halfSheetContent)
        XCTAssertTrue(
            offlineCacheStateLine.exists,
            "the offline card's cache line must present the registration"
        )
        XCTAssertTrue(
            offlineCacheStateLine.label.hasPrefix(Self.offlineCacheStatePrefix),
            "the offline card's cache line must name the registration, '\(offlineCacheStateLine.label)'"
        )
        XCTAssertTrue(
            offlineCacheStateLine.label.contains(Self.cacheBudgetPhrase),
            "the registered cache must carry the demo's named byte budget"
        )
        XCTAssertTrue(
            offlineCacheStateLine.label.contains(Self.tileCacheDatabaseFileName),
            "the registered cache must be the demo's named cache database, '\(offlineCacheStateLine.label)'"
        )

        // The live tile line: the latest drawn frame's statistics (the
        // honest accounting the renderer reports per frame).
        let liveTileLine = elementWithIdentifier("offline-tile-statistics", in: application)
        scrollToMake(liveTileLine, visibleIn: halfSheetContent)
        pollForSettlement(
            timeoutDescription: "the first drawn frame's tile statistics settling into the live tile line",
            timeoutSeconds: Self.firstFrameDrawnTimeoutSeconds
        ) {
            liveTileLine.exists
                && liveTileLine.label.hasPrefix(Self.offlineTileLinePrefix)
                && liveTileLine.label.contains(Self.offlineTileRequestedLabel)
                && !liveTileLine.label.contains(Self.offlineTileNoFrameYetText)
        }
    }

    // MARK: Test 4b — the offline card plans over the bundled test graph

    /// The Espresso suite's offline route test: the route control plans over
    /// the committed compiled archive bundled with the app, and the route
    /// summary presents the result with its recorded-test-graph honesty note.
    /// The plan needs no provider credential, so this case runs keyless.
    func testTheOfflineCardPlansARouteFromTheBundledTestGraph() throws {
        let application = launchApplication(
            launchArguments: [
                "-\(Self.launchArgumentSheetHalfState)",
                Self.launchArgumentBooleanTrueValue,
            ]
        )
        let halfSheetContent = elementWithIdentifier(
            Self.halfSheetContentIdentifier, in: application
        )
        XCTAssertTrue(
            halfSheetContent.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the half sheet's working content must render"
        )

        let offlineRouteButton = elementWithIdentifier(
            Self.offlineRouteButtonIdentifier, in: application
        )
        scrollToMake(offlineRouteButton, visibleIn: halfSheetContent)
        XCTAssertTrue(
            offlineRouteButton.exists,
            "the offline card's route control must render keyless"
        )
        offlineRouteButton.tap()

        let routeSummary = elementWithIdentifier("route-summary-card", in: application)
        XCTAssertTrue(
            routeSummary.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the offline control must produce a route summary"
        )
        // The summary's identifier propagates to its child texts, so the
        // content is asserted by its own value rather than a child identifier.
        XCTAssertTrue(
            application.staticTexts[Self.offlineRouteTitleText]
                .waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the offline route summary must name the recorded test graph's route"
        )
        XCTAssertTrue(
            application.staticTexts[Self.offlineRouteDistanceText].exists,
            "the offline route summary must carry the archive route's distance"
        )
        XCTAssertTrue(
            application.staticTexts[Self.offlineRouteDurationText].exists,
            "the offline route summary must carry the archive route's duration"
        )
        XCTAssertTrue(
            application.staticTexts[Self.offlineRouteNoteText]
                .waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the offline route summary must carry the §2.5 honesty note"
        )
    }

    // MARK: Test 4c — the region pack's download, restart and deletion

    /// The pack proof on the simulator, driven through the card's own
    /// controls: Download starts the demo region, an early Cancel keeps the
    /// partial pack, a relaunch lists that pack from the store in a fresh
    /// process, the resumed download reports the kept tiles as served from
    /// disk rather than fetched, and Delete removes the pack. The simulator
    /// shares its host's network, so the disk-served split and the fresh-process
    /// listing stand in for the Android proof's airplane-mode relaunch.
    func testTheOfflineRegionDownloadSurvivesARelaunchAndDeletesItsPack() throws {
        try XCTSkipUnless(
            hasRuntimeInjectedApiKey,
            "requires PROVIDER_API_KEY in the test environment (runtime injection)"
        )
        var application = launchApplication(
            launchArguments: [
                "-\(Self.launchArgumentSheetHalfState)",
                Self.launchArgumentBooleanTrueValue,
            ]
        )
        awaitReadinessChipReady(application: application)

        let halfSheetContent = elementWithIdentifier(
            Self.halfSheetContentIdentifier, in: application
        )
        XCTAssertTrue(
            halfSheetContent.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the half sheet's working content must render"
        )

        // The demo region's summary names the region and its enumerated tile
        // count; the count is read from the rendered line, never hardcoded.
        let regionSummary = elementWithIdentifier(
            Self.offlineRegionSummaryIdentifier, in: application
        )
        scrollToMake(regionSummary, visibleIn: halfSheetContent)
        XCTAssertTrue(
            regionSummary.exists,
            "the offline card must name the shared artifact's demo region"
        )
        let expectedTileCount = try XCTUnwrap(
            Self.demoRegionTileCount(from: regionSummary.label),
            "the demo region summary must end with its enumerated tile count, '\(regionSummary.label)'"
        )
        XCTAssertGreaterThan(expectedTileCount, 0)

        // Isolation: a previous run's pack would make the first download a
        // cache hit, so every stored region is removed through the card.
        deleteEveryStoredRegion(in: application, visibleIn: halfSheetContent)
        XCTAssertEqual(
            storedRegionDeleteButtons(in: application).count, 0,
            "the card must list no stored region before the download"
        )

        // The first download runs against an empty pack; canceling after the
        // first fetched tile leaves the rest unstored and the pack kept.
        let downloadButton = elementWithIdentifier(
            Self.offlineRegionDownloadButtonIdentifier, in: application
        )
        scrollToMake(downloadButton, visibleIn: halfSheetContent)
        XCTAssertTrue(
            downloadButton.isEnabled,
            "the card offers Download once the tile cache is registered"
        )
        downloadButton.tap()

        let statusLine = elementWithIdentifier(
            Self.offlineRegionStatusIdentifier, in: application
        )
        pollForSettlement(
            timeoutDescription: "the first tile's fetched progress report",
            timeoutSeconds: Self.offlineRegionFirstProgressTimeoutSeconds
        ) {
            statusLine.exists
                && statusLine.label.hasPrefix(Self.offlineRegionDownloadingStatusPrefix)
                && statusLine.label.contains(Self.offlineRegionNetworkFetchedFragment)
        }
        XCTAssertTrue(
            statusLine.label.contains(Self.offlineRegionNetworkFetchedFragment),
            "the first download must fetch a member tile from the network, '\(statusLine.label)'"
        )

        let cancelButton = elementWithIdentifier(
            Self.offlineRegionCancelButtonIdentifier, in: application
        )
        XCTAssertTrue(
            cancelButton.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "a running download must offer Cancel"
        )
        cancelButton.tap()
        pollForSettlement(
            timeoutDescription: "the cancellation's terminal line"
        ) {
            statusLine.exists && statusLine.label == Self.offlineRegionCancelledStatusText
        }
        XCTAssertEqual(
            statusLine.label, Self.offlineRegionCancelledStatusText,
            "the cancelled download must keep the partial pack and say so"
        )
        attachScreenshot(named: "offline-region-cancelled", of: application)

        // The partial pack is on disk. Terminate and relaunch: the stored
        // region list is read from the store by a fresh process, which is the
        // simulator's stand-in for the Android proof's offline relaunch.
        application.terminate()
        application = launchApplication(
            launchArguments: [
                "-\(Self.launchArgumentSheetHalfState)",
                Self.launchArgumentBooleanTrueValue,
            ]
        )
        awaitReadinessChipReady(application: application)
        let relaunchedSheet = elementWithIdentifier(
            Self.halfSheetContentIdentifier, in: application
        )
        XCTAssertTrue(
            relaunchedSheet.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the relaunched half sheet must render"
        )

        let storedTilesLine = storedRegionLine(
            withPrefix: Self.offlineRegionStoredTilesPrefix, in: application
        )
        scrollToMake(storedTilesLine, visibleIn: relaunchedSheet)
        XCTAssertTrue(
            storedTilesLine.exists,
            "the relaunched application must list the kept pack from the store"
        )
        let keptCounts = try XCTUnwrap(
            Self.storedTileCounts(from: storedTilesLine.label),
            "the stored row must carry its stored and member counts, '\(storedTilesLine.label)'"
        )
        XCTAssertGreaterThan(
            keptCounts.present, 0,
            "the cancelled download must have kept its fetched tiles"
        )
        XCTAssertEqual(
            keptCounts.member, expectedTileCount,
            "the kept pack must claim the demo region's enumerated tiles"
        )
        XCTAssertLessThan(
            keptCounts.present, expectedTileCount,
            "the cancelled download must leave the region partial"
        )
        XCTAssertTrue(
            storedRegionLine(
                withPrefix: Self.offlineRegionStoredBytesPrefix, in: application
            ).exists,
            "the stored row must state the pack's payload bytes"
        )
        attachScreenshot(named: "offline-region-kept-after-relaunch", of: application)

        // Resume: the kept tiles are reported as served from disk, the rest
        // fetched from the provider, and the download completes.
        let resumedDownloadButton = elementWithIdentifier(
            Self.offlineRegionDownloadButtonIdentifier, in: application
        )
        scrollToMake(resumedDownloadButton, visibleIn: relaunchedSheet)
        resumedDownloadButton.tap()
        let resumedStatusLine = elementWithIdentifier(
            Self.offlineRegionStatusIdentifier, in: application
        )
        pollForSettlement(
            timeoutDescription: "the resumed download reporting kept tiles as served from disk",
            timeoutSeconds: Self.offlineRegionDownloadCompletionTimeoutSeconds
        ) {
            guard resumedStatusLine.exists else { return false }
            let diskServed = Self.diskServedTileCount(from: resumedStatusLine.label)
            return resumedStatusLine.label.contains(Self.offlineRegionDiskServedFragment)
                && (diskServed ?? 0) > 0
        }
        XCTAssertGreaterThan(
            Self.diskServedTileCount(from: resumedStatusLine.label) ?? 0, 0,
            "the resumed download must report the kept tiles as served from disk, '\(resumedStatusLine.label)'"
        )
        pollForSettlement(
            timeoutDescription: "the resumed download's completion",
            timeoutSeconds: Self.offlineRegionDownloadCompletionTimeoutSeconds
        ) {
            resumedStatusLine.exists
                && resumedStatusLine.label == Self.offlineRegionCompletedStatusText
        }

        // Every member tile is stored, and the row's payload is a real size.
        let completedTilesLine = storedRegionLine(
            withPrefix: Self.offlineRegionStoredTilesPrefix, in: application
        )
        scrollToMake(completedTilesLine, visibleIn: relaunchedSheet)
        let completedCounts = try XCTUnwrap(
            Self.storedTileCounts(from: completedTilesLine.label),
            "the completed row must carry its stored and member counts, '\(completedTilesLine.label)'"
        )
        XCTAssertEqual(
            completedCounts.present, expectedTileCount,
            "the completed pack must store every enumerated tile"
        )
        XCTAssertEqual(completedCounts.member, expectedTileCount)
        let completedBytesLine = storedRegionLine(
            withPrefix: Self.offlineRegionStoredBytesPrefix, in: application
        )
        scrollToMake(completedBytesLine, visibleIn: relaunchedSheet)
        XCTAssertTrue(
            completedBytesLine.exists,
            "the completed row must state the pack's payload bytes"
        )
        attachScreenshot(named: "offline-region-complete", of: application)

        // Delete the stored region while the app runs and the pack is whole.
        let deleteButton = storedRegionDeleteButtons(in: application).element(boundBy: 0)
        scrollToMake(deleteButton, visibleIn: relaunchedSheet)
        XCTAssertTrue(
            deleteButton.exists,
            "the stored row must offer Delete"
        )
        deleteButton.tap()
        let deletedStatusLine = elementWithIdentifier(
            Self.offlineRegionDeletedStatusIdentifier, in: application
        )
        XCTAssertTrue(
            deletedStatusLine.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the deletion must state its outcome"
        )
        XCTAssertEqual(
            deletedStatusLine.label, Self.offlineRegionDeletedStatusText
        )
        XCTAssertTrue(
            pollForNonExistence(
                storedTilesLine, within: Self.settleTimeoutSeconds
            ),
            "the deleted region must leave the stored list"
        )
        attachScreenshot(named: "offline-region-deleted", of: application)
    }

    /// Attaches a named screenshot of the current screen to the test result,
    /// so a keyed pack-proof run carries the same evidence the Android
    /// captures record.
    private func attachScreenshot(named name: String, of application: XCUIApplication) {
        let attachment = XCTAttachment(screenshot: application.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    /// Every rendered stored-region row's Delete control, in the card's order.
    private func storedRegionDeleteButtons(
        in application: XCUIApplication
    ) -> XCUIElementQuery {
        application.descendants(matching: .any).matching(
            NSPredicate(
                format: "identifier BEGINSWITH %@ OR label == %@",
                Self.offlineRegionDeleteIdentifierPrefix,
                Self.offlineRegionDeleteActionText
            )
        )
    }

    /// The first static text whose label carries the given stored-row prefix.
    private func storedRegionLine(
        withPrefix linePrefix: String,
        in application: XCUIApplication
    ) -> XCUIElement {
        application.staticTexts.matching(
            NSPredicate(format: "label BEGINSWITH %@", linePrefix)
        ).firstMatch
    }

    /// Removes every stored region the card lists through its own Delete
    /// control, waiting for each row to leave the list before the next tap.
    private func deleteEveryStoredRegion(
        in application: XCUIApplication,
        visibleIn container: XCUIElement
    ) {
        let deleteButtons = storedRegionDeleteButtons(in: application)
        var remainingDeletionCount = deleteButtons.count
        while remainingDeletionCount > 0 {
            let deleteButton = deleteButtons.element(boundBy: 0)
            scrollToMake(deleteButton, visibleIn: container)
            guard deleteButton.exists else { return }
            deleteButton.tap()
            let deletedRowCount = remainingDeletionCount
            pollForSettlement(
                timeoutDescription: "the stored region's row leaving the list"
            ) {
                deleteButtons.count < deletedRowCount
            }
            let remainingAfterDeletion = deleteButtons.count
            guard remainingAfterDeletion < deletedRowCount else { return }
            remainingDeletionCount = remainingAfterDeletion
        }
    }

    /// The demo region's enumerated tile count from its summary line
    /// (`Downtown Sacramento · zoom 12–16 · 23 tiles`).
    private static func demoRegionTileCount(from summaryLabel: String) -> UInt64? {
        let countSuffix = " tiles"
        guard summaryLabel.hasSuffix(countSuffix) else { return nil }
        let withoutSuffix = summaryLabel.dropLast(countSuffix.count)
        guard let lastSeparator = withoutSuffix.lastIndex(of: " ") else { return nil }
        return UInt64(withoutSuffix[withoutSuffix.index(after: lastSeparator)...])
    }

    /// A stored row's counts from its `Stored tiles 23 / 23` line.
    private static func storedTileCounts(
        from lineLabel: String
    ) -> (present: UInt64, member: UInt64)? {
        let components = lineLabel.split(separator: " ")
        guard components.count == 5,
              components[3] == "/",
              let present = UInt64(components[2]),
              let member = UInt64(components[4])
        else {
            return nil
        }
        return (present: present, member: member)
    }

    /// The tiles a progress line reports as served from disk (the format's
    /// trailing `… · 23 from disk` slot).
    private static func diskServedTileCount(
        from progressLabel: String
    ) -> UInt64? {
        let fragmentSuffix = " " + offlineRegionDiskServedFragment
        guard progressLabel.hasSuffix(fragmentSuffix) else { return nil }
        let withoutSuffix = progressLabel.dropLast(fragmentSuffix.count)
        guard let lastSeparator = withoutSuffix.lastIndex(of: " ") else { return nil }
        return UInt64(withoutSuffix[withoutSuffix.index(after: lastSeparator)...])
    }

    // MARK: Test 5 — the §5.1 gesture directions (content follows the pointer)

    /// The drag-pan direction (§5.1): a rightward drag must move the map
    /// content **right** — the camera center wests. The launch-argument
    /// pan demo drives the drag gesture's own user-path method (no state
    /// forced, no touch injection); the assertion reads the diagnostics
    /// sheet's camera block after the pan settles into the drawn readback.
    func testTheGestureDragPanMovesTheContentWithThePointer() throws {
        try XCTSkipUnless(
            hasRuntimeInjectedApiKey,
            "requires PROVIDER_API_KEY in the test environment (runtime injection)"
        )
        let application = launchApplication(launchArguments: [
            "-\(Self.launchArgumentGesturePanDemo)",
            Self.launchArgumentBooleanTrueValue,
        ])
        awaitReadinessChipReady(application: application)

        let minimumPannedLongitude = Self.launchCameraCenterLongitudeDegrees
            - Self.panDemoMinimumLongitudeMoveDegrees
        let pannedCameraRows = openDiagnosticsAndReadCameraRows(
            application: application,
            settleCondition: { camera_rows in
                guard let center_coordinates = Self.centerRowCoordinates(of: camera_rows[0]) else {
                    return false
                }
                return center_coordinates.longitude <= minimumPannedLongitude
            }
        )
        guard let panned_center = Self.centerRowCoordinates(of: pannedCameraRows[0]) else {
            XCTFail(
                "the panned camera readback must render a center row, '\(pannedCameraRows.first ?? "")'"
            )
            return
        }
        XCTAssertLessThan(
            panned_center.longitude,
            minimumPannedLongitude,
            "a rightward drag must move the content right (the camera center wests)"
        )
        XCTAssertEqual(
            panned_center.latitude,
            Self.launchCameraCenterLatitudeDegrees,
            accuracy: Self.panDragLatitudeStandstillToleranceDegrees,
            "a pure-rightward drag must not move the camera center north or south"
        )
    }

    /// The two-finger rotation direction (§5.1): a clockwise rotation must
    /// rotate the map content clockwise — the bearing (the compass
    /// direction the screen top faces) lowers to the delta's reflection.
    /// The launch-argument rotate demo drives the rotation gesture's own
    /// user-path method; the assertion reads the diagnostics sheet's
    /// bearing row after the turn settles into the drawn readback.
    func testTheTwoFingerRotationMovesTheContentWithTheFingers() throws {
        try XCTSkipUnless(
            hasRuntimeInjectedApiKey,
            "requires PROVIDER_API_KEY in the test environment (runtime injection)"
        )
        let application = launchApplication(launchArguments: [
            "-\(Self.launchArgumentTwoFingerRotateDemo)",
            Self.launchArgumentBooleanTrueValue,
        ])
        awaitReadinessChipReady(application: application)

        let rotatedCameraRows = openDiagnosticsAndReadCameraRows(
            application: application,
            settleCondition: { camera_rows in
                camera_rows[Self.bearingRowIndex]
                    == Self.rotatedContentFollowBearingReadback
            }
        )
        XCTAssertEqual(
            rotatedCameraRows[Self.bearingRowIndex],
            Self.rotatedContentFollowBearingReadback,
            "a clockwise two-finger rotation must rotate the content clockwise (the bearing lowers to the delta's reflection)"
        )
    }

    // MARK: Test 6 — the tap contract (one tap once, a double tap one level)

    /// The Espresso suite's
    /// `aSingleTapReportsOnceAndADoubleTapZoomsExactlyOneLevel` mirrored on
    /// iOS with real taps: a single tap reports exactly one confirmed click
    /// and leaves the camera alone, while a double tap at the same point
    /// zooms exactly one level and reports no click of its own (the shipped
    /// view's single recognizer waits for the double tap to fail). The
    /// assertions read the diagnostics sheet's camera rows and its
    /// confirmed-tap count; the keyless base map draws the frame the tap
    /// unprojects through, so no provider credential is involved.
    func testASingleTapReportsOnceAndADoubleTapZoomsExactlyOneLevel() throws {
        let application = launchApplication(launchArguments: [])
        awaitReadinessChipReady(application: application)

        let initialZoomLevel = try XCTUnwrap(
            readCameraZoomLevel(application: application),
            "the diagnostics camera block must render the drawn zoom readback"
        )
        let initialTapCount = try XCTUnwrap(
            readMapTapCount(application: application),
            "the diagnostics sheet must render the confirmed-tap count"
        )
        XCTAssertEqual(
            initialTapCount, 0,
            "a fresh launch reports no confirmed map click"
        )

        let mapTapCoordinate = application.coordinate(
            withNormalizedOffset: CGVector(
                dx: Self.mapTapHorizontalFraction,
                dy: Self.mapTapVerticalFraction
            )
        )

        // One tap: exactly one click, no camera move.
        mapTapCoordinate.tap()
        openDiagnosticsSheet(application: application)
        pollForSettlement(
            timeoutDescription: "the confirmed single tap's one map click"
        ) {
            mapTapCountRow(application: application).label
                == Self.mapTapCountLabel(tapCount: initialTapCount + 1)
        }
        let singleTapZoomLevel = try XCTUnwrap(
            cameraZoomLevelFromOpenSheet(application: application),
            "the open sheet must render the camera zoom after the single tap"
        )
        XCTAssertEqual(
            singleTapZoomLevel, initialZoomLevel,
            accuracy: Self.cameraZoomReadoutTolerance,
            "a single tap must not move the camera"
        )
        closeDiagnostics(application: application)

        // One double tap: exactly one zoom level, and still exactly one
        // confirmed click overall (the double tap's first touch never
        // reaches the single-tap callback).
        mapTapCoordinate.doubleTap()
        openDiagnosticsSheet(application: application)
        let expectedZoomLevel = initialZoomLevel + Self.doubleTapZoomStep
        pollForSettlement(
            timeoutDescription: "the double tap's one-level zoom"
        ) {
            guard let zoomLevel = cameraZoomLevelFromOpenSheet(application: application) else {
                return false
            }
            return abs(zoomLevel - expectedZoomLevel) <= Self.cameraZoomReadoutTolerance
        }
        let doubleTapZoomLevel = try XCTUnwrap(
            cameraZoomLevelFromOpenSheet(application: application),
            "the open sheet must render the camera zoom after the double tap"
        )
        XCTAssertEqual(
            doubleTapZoomLevel, expectedZoomLevel,
            accuracy: Self.cameraZoomReadoutTolerance,
            "a double tap must zoom exactly one level"
        )
        // The confirmed-single-tap window has passed by the time the zoom
        // settles: a late click from the double tap's first touch would have
        // landed by now.
        XCTAssertEqual(
            mapTapCountRow(application: application).label,
            Self.mapTapCountLabel(tapCount: initialTapCount + 1),
            "one double tap must not also report a map click"
        )
        closeDiagnostics(application: application)
    }

    // MARK: Test 7 — the styled vector base

    /// The Espresso suite's
    /// `selectingACommittedVectorStyleServesTheStyledBaseAndStatesTheLabelLimit`
    /// mirrored on iOS: the application launches on the shared registry's
    /// credential-less demo base map, the map layers panel offers the
    /// registry's committed vector styles, and selecting one serves the styled
    /// vector tileset — the base-map section states the labels the styles
    /// draw, the serving row names the selected style, and no failure state is
    /// reported. The demo base map and the vector style are both read from the
    /// shared registry artifact (bundled into the test bundle), never
    /// hardcoded, and the run is keyless.
    func testSelectingACommittedVectorStyleServesTheStyledBaseAndStatesTheLabelLimit() throws {
        let providerRegistry = try loadProviderRegistry()
        // The demo base map: the registry's credential-less raster source on
        // iOS (the same descriptor a keyless start selects).
        guard let demoBaseMapDescriptor = providerRegistry.providers.first(
            where: { descriptor in
                descriptor.registrationAvailable.ios
                    && !descriptor.rasterUrlTemplate.trimmingCharacters(
                        in: .whitespacesAndNewlines
                    ).isEmpty
            }
        ) else {
            throw XCTSkip(
                "the shared provider registry declares no credential-less base map for iOS"
            )
        }
        // The first committed vector style the panel offers (the registry's
        // artifact order is the provider order and its style order is the
        // panel's order).
        let vectorBaseOffers = providerRegistry.providers
            .filter { $0.offersVectorBase }
            .compactMap { descriptor in
                descriptor.vectorStyles.first.map { style in
                    (descriptor: descriptor, style: style)
                }
            }
        guard let firstVectorBaseOffer = vectorBaseOffers.first else {
            throw XCTSkip(
                "the shared provider registry declares no committed vector style"
            )
        }
        // The row's label is the provider's display name and the style's own
        // name (`VectorBaseMapChoice.displayName`).
        let vectorStyleDisplayName =
            firstVectorBaseOffer.descriptor.displayName
            + Self.vectorStyleNameSeparator
            + firstVectorBaseOffer.style.displayName

        let application = launchApplication(launchArguments: [
            "-\(Self.launchArgumentDemoBaseMap)", demoBaseMapDescriptor.providerKey,
        ])
        // The demo base map's first presented frame settles the ladder on
        // the ready state; the styled base is asserted against a running map.
        awaitReadinessChipReady(application: application)

        // The map layers panel owns the base-map selection: the styled
        // base's row's own act is the panel row named by the shared registry
        // descriptor.
        openMapLayersPanel(in: application)
        let layersPanel = elementWithIdentifier("map-layers-panel", in: application)
        let styleRow = elementWithIdentifier(
            "map-layer-style-\(firstVectorBaseOffer.style.styleId)",
            in: application
        )
        scrollToMake(styleRow, visibleIn: layersPanel)
        XCTAssertTrue(
            styleRow.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the layers panel must offer the registry's committed vector style, '\(vectorStyleDisplayName)'"
        )
        XCTAssertTrue(
            styleRow.label.contains(vectorStyleDisplayName),
            "the vector style row must name the style, was '\(styleRow.label)'"
        )
        styleRow.tap()

        // The stated limit travels with the base that has it, so a user never
        // reads the absence of labels as a failure.
        let vectorLabelsNote = application.staticTexts[Self.vectorLabelsNoteText]
        pollForSettlement(
            timeoutDescription: "the base-map section's vector labels note"
        ) {
            vectorLabelsNote.exists
        }
        XCTAssertTrue(
            vectorLabelsNote.exists,
            "the base-map section must state the labels the styles draw, '\(Self.vectorLabelsNoteText)'"
        )

        // The serving row names the selected style and carries the Serving
        // mark, so the selection is the base in force.
        XCTAssertTrue(
            waitForLabelContaining("Serving", element: styleRow),
            "the selected vector style must be the serving base, was '\(styleRow.label)'"
        )
        closeMapLayersPanel(in: application)

        // No failure state: the chip is neither failed nor degraded and the
        // error strip stays empty. The styled base is an accepted base, not a
        // renderer failure.
        let styledReadinessLabel = readinessChip(application: application).label
        XCTAssertFalse(
            styledReadinessLabel.hasPrefix("Failed:"),
            "the styled vector base must not put the ladder in the failed state, '\(styledReadinessLabel)'"
        )
        XCTAssertFalse(
            styledReadinessLabel.hasPrefix("Degraded:"),
            "the styled vector base must not put the ladder in the degraded state, '\(styledReadinessLabel)'"
        )
        XCTAssertFalse(
            elementWithIdentifier("error-report", in: application).exists,
            "an accepted styled base reports nothing in the error strip"
        )

        // The ladder settles back to ready on the frames the styled base
        // draws: the chip reports the presented surface, not the state the
        // base-map change entered (the shell used to
        // stop at the loading line once the view's single ready report had
        // passed).
        awaitReadinessChipReady(application: application)
        XCTAssertEqual(
            readinessChip(application: application).label, "Ready",
            "the readiness ladder must settle on the ready state once the styled base presents"
        )

        // Visual evidence for the provider audit: the styled base as it
        // presents, kept with the run so a raster fallback could not pass for
        // a vector base in review.
        let styledBaseScreenshot = XCTAttachment(screenshot: application.screenshot())
        styledBaseScreenshot.name = "vector base \(vectorStyleDisplayName)"
        styledBaseScreenshot.lifetime = .keepAlways
        add(styledBaseScreenshot)
    }

    // MARK: Test 7b — the keyless start's styled base

    /// The keyless start opens on the registry's declared styled vector base:
    /// the credential-less raster source still registers so the surface
    /// attaches, and the declared style is installed over it before the user
    /// touches anything. The declaration, provider and style names all come
    /// from the shared registry artifact, never a hardcoded provider.
    func testTheKeylessStartOpensOnTheRegistrysDeclaredStyledBase() throws {
        try XCTSkipIf(
            hasRuntimeInjectedApiKey,
            "the keyless start's styled base needs a credential-less session"
        )
        let providerRegistry = try loadProviderRegistry()
        guard let declared = providerRegistry.keylessBaseMap,
              let declaredDescriptor = providerRegistry.providers.first(
                  where: { $0.providerKey == declared.providerKey }
              ),
              let declaredStyle = declaredDescriptor.vectorStyles.first(
                  where: { $0.styleId == declared.styleId }
              )
        else {
            throw XCTSkip(
                "the shared provider registry declares no resolvable keyless styled base"
            )
        }
        let expectedBaseDisplayName =
            declaredDescriptor.displayName + Self.vectorStyleNameSeparator
            + declaredStyle.displayName
        let application = launchApplication(launchArguments: [])
        awaitReadinessChipReady(application: application)

        openMapLayersPanel(in: application)
        let layersPanel = elementWithIdentifier("map-layers-panel", in: application)
        let servingRow = elementWithIdentifier(
            "map-layer-style-\(declared.styleId)", in: application
        )
        scrollToMake(servingRow, visibleIn: layersPanel)
        XCTAssertTrue(
            servingRow.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the layers panel must render the registry's declared style row"
        )
        // The install follows the surface attach, so the row is polled until
        // it names the registry's declared style rather than the raster
        // source selected first to give the surface something to attach to.
        pollForSettlement(
            timeoutDescription: "the declared style row naming the keyless styled base"
        ) {
            servingRow.exists
                && servingRow.label.contains(expectedBaseDisplayName)
                && servingRow.label.contains("Serving")
        }
        XCTAssertTrue(
            servingRow.label.contains(expectedBaseDisplayName)
                && servingRow.label.contains("Serving"),
            "a keyless start must open on the registry's declared styled base, '\(expectedBaseDisplayName)', was '\(servingRow.label)'"
        )
        closeMapLayersPanel(in: application)
        XCTAssertFalse(
            elementWithIdentifier("error-report", in: application).exists,
            "the keyless styled base reports nothing in the error strip"
        )
        awaitReadinessChipReady(application: application)
    }

    // MARK: Test 8 — remembered choices, reset and settings sizing

    /// The explicit choices survive a relaunch: the theme, the base map and
    /// the plan fields are restored from the versioned `UserDefaults` record,
    /// and the remembered pair fills the fields without planning a route.
    func testRememberedChoicesSurviveRelaunch() throws {
        defer { resetRememberedChoicesAfterTest() }
        let providerRegistry = try loadProviderRegistry()
        let vectorOffers = providerRegistry.providers
            .filter { $0.offersVectorBase }
            .flatMap { descriptor in
                descriptor.vectorStyles.map { style in
                    (descriptor: descriptor, style: style)
                }
            }
        let keylessBaseMap = providerRegistry.keylessBaseMap
        guard let rememberedBaseMap = vectorOffers.first(where: { offer in
            offer.descriptor.providerKey != keylessBaseMap?.providerKey
                || offer.style.styleId != keylessBaseMap?.styleId
        }) else {
            throw XCTSkip(
                "the registry offers no vector style beyond the keyless default"
            )
        }
        let rememberedBaseMapLabel = rememberedBaseMap.descriptor.displayName
            + Self.vectorStyleNameSeparator + rememberedBaseMap.style.displayName

        let application = launchFreshApplication(
            launchArguments: ["-\(Self.launchArgumentSheetHalfState)", Self.launchArgumentBooleanTrueValue]
        )
        // The theme toggle cycles Auto -> Light; the label names the next choice.
        let themeToggle = elementWithIdentifier("theme-toggle", in: application)
        XCTAssertTrue(
            themeToggle.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the half sheet's theme row must render"
        )
        XCTAssertEqual(themeToggle.label, "Switch to light theme")
        themeToggle.tap()
        XCTAssertTrue(
            waitForLabel("Switch to dark theme", element: themeToggle),
            "the explicit Light choice must be in force"
        )

        // Choose a styled base map that the keyless start did not already
        // serve, through the map layers panel's own row.
        openMapLayersPanel(in: application)
        let layersPanel = elementWithIdentifier("map-layers-panel", in: application)
        let rememberedBaseMapRow = elementWithIdentifier(
            "map-layer-style-\(rememberedBaseMap.style.styleId)", in: application
        )
        scrollToMake(rememberedBaseMapRow, visibleIn: layersPanel)
        XCTAssertTrue(
            rememberedBaseMapRow.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the layers panel must offer '\(rememberedBaseMapLabel)'"
        )
        rememberedBaseMapRow.tap()
        XCTAssertTrue(
            waitForLabelContaining("Serving", element: rememberedBaseMapRow),
            "the selected base map must serve after its row's tap"
        )
        closeMapLayersPanel(in: application)

        // Type both plan fields through the search view; typing never plans.
        typeRememberedPlanFields(in: application)

        application.terminate()
        let relaunched = launchApplication(
            launchArguments: [
                "-\(Self.launchArgumentSheetHalfState)",
                Self.launchArgumentBooleanTrueValue,
            ],
            resetsRememberedChoices: false
        )
        let relaunchedThemeToggle = elementWithIdentifier(
            "theme-toggle", in: relaunched
        )
        XCTAssertTrue(
            relaunchedThemeToggle.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the relaunched half sheet's theme row must render"
        )
        XCTAssertTrue(
            waitForLabel("Switch to dark theme", element: relaunchedThemeToggle),
            "the explicit Light choice must be restored"
        )

        openMapLayersPanel(in: relaunched)
        let relaunchedLayersPanel = elementWithIdentifier(
            "map-layers-panel", in: relaunched
        )
        let relaunchedBaseMapRow = elementWithIdentifier(
            "map-layer-style-\(rememberedBaseMap.style.styleId)", in: relaunched
        )
        scrollToMake(relaunchedBaseMapRow, visibleIn: relaunchedLayersPanel)
        XCTAssertTrue(
            relaunchedBaseMapRow.waitForExistence(timeout: Self.settleTimeoutSeconds)
        )
        XCTAssertTrue(
            waitForLabelContaining("Serving", element: relaunchedBaseMapRow),
            "the remembered base map must be serving, was '\(relaunchedBaseMapRow.label)'"
        )
        closeMapLayersPanel(in: relaunched)

        openSearchViewArmedForOrigin(in: relaunched)
        XCTAssertEqual(
            textFieldValue(relaunched.textFields["Search an origin"]),
            Self.rememberedOriginText,
            "the remembered origin text must fill the field"
        )
        XCTAssertEqual(
            textFieldValue(relaunched.textFields["Search a destination"]),
            Self.rememberedDestinationText,
            "the remembered destination text must fill the field"
        )
        XCTAssertFalse(
            elementWithIdentifier("route-summary-card", in: relaunched).exists,
            "a remembered pair must never plan a route by itself"
        )
    }

    /// The proof: a vector style switch, the traffic overlay scene and a
    /// centered device fix all return after a relaunch. The style and scene
    /// choices come from the versioned record, and the device source restarts
    /// only because the record remembered it.
    ///
    /// The simulator must carry a location before this run:
    /// `xcrun simctl location booted set 38.6012,-121.4512`.
    func testTheStyleTrafficAndCenteredDeviceFixSurviveRelaunch() throws {
        defer { resetRememberedChoicesAfterTest() }
        let providerRegistry = try loadProviderRegistry()
        let vectorOffers = providerRegistry.providers
            .filter { $0.offersVectorBase }
            .flatMap { descriptor in
                descriptor.vectorStyles.map { style in
                    (descriptor: descriptor, style: style)
                }
            }
        let keylessBaseMap = providerRegistry.keylessBaseMap
        guard let rememberedBaseMap = vectorOffers.first(where: { offer in
            offer.descriptor.providerKey != keylessBaseMap?.providerKey
                || offer.style.styleId != keylessBaseMap?.styleId
        }) else {
            throw XCTSkip(
                "the registry offers no vector style beyond the keyless default"
            )
        }

        let application = launchFreshApplication(
            launchArguments: [],
            injectsRuntimeCredential: false
        )
        awaitReadinessChipReady(application: application)

        // A style switch the keyless start did not already serve.
        openMapLayersPanel(in: application)
        let layersPanel = elementWithIdentifier("map-layers-panel", in: application)
        let styleRow = elementWithIdentifier(
            "map-layer-style-\(rememberedBaseMap.style.styleId)", in: application
        )
        scrollToMake(styleRow, visibleIn: layersPanel)
        XCTAssertTrue(
            styleRow.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the layers panel must offer the recorded vector style"
        )
        styleRow.tap()
        XCTAssertTrue(
            waitForLabelContaining("Serving", element: styleRow),
            "the selected style must serve"
        )

        // The artifact's traffic overlay scene, the fixture-backed traffic
        // layer both native targets carry.
        let trafficSceneSwitch = elementWithIdentifier(
            "map-layer-scene-traffic", in: application
        )
        scrollToMake(trafficSceneSwitch, visibleIn: layersPanel)
        XCTAssertTrue(
            trafficSceneSwitch.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the layers panel must offer the traffic scene switch"
        )
        XCTAssertEqual(
            trafficSceneSwitch.value as? String, "Off",
            "the traffic scene starts off on a never-chose device"
        )
        trafficSceneSwitch.tap()
        pollForSettlement(timeoutDescription: "the traffic scene switching on") {
            (trafficSceneSwitch.value as? String) == "On"
        }
        closeMapLayersPanel(in: application)

        // The round control starts the device source and centres on its fix.
        let roundControl = elementWithIdentifier(
            "map-control-my-location", in: application
        )
        XCTAssertTrue(
            roundControl.waitForExistence(timeout: Self.emulatorFirstReadyTimeoutSeconds),
            "the map control cluster must offer the my-location control"
        )
        roundControl.tap()
        grantWhenInUseLocationAuthorization(in: application)
        openMapLayersPanel(in: application)
        XCTAssertTrue(
            waitForLabel(
                "Active",
                element: elementWithIdentifier("device-location-status", in: application)
            ),
            "the device source must report an active fix"
        )
        closeMapLayersPanel(in: application)

        let fix = try readDeviceFix(in: application)
        assertCameraCenteredOnDeviceFix(in: application, fix: fix)

        application.terminate()
        let relaunched = launchApplication(
            launchArguments: [],
            injectsRuntimeCredential: false,
            resetsRememberedChoices: false
        )

        // The style is serving again from the record.
        openMapLayersPanel(in: relaunched)
        let relaunchedLayersPanel = elementWithIdentifier(
            "map-layers-panel", in: relaunched
        )
        let relaunchedStyleRow = elementWithIdentifier(
            "map-layer-style-\(rememberedBaseMap.style.styleId)", in: relaunched
        )
        scrollToMake(relaunchedStyleRow, visibleIn: relaunchedLayersPanel)
        XCTAssertTrue(
            relaunchedStyleRow.waitForExistence(timeout: Self.settleTimeoutSeconds)
        )
        XCTAssertTrue(
            waitForLabelContaining("Serving", element: relaunchedStyleRow),
            "the remembered style must serve after the relaunch"
        )

        // The traffic scene is on again from the record.
        let relaunchedTrafficSceneSwitch = elementWithIdentifier(
            "map-layer-scene-traffic", in: relaunched
        )
        scrollToMake(relaunchedTrafficSceneSwitch, visibleIn: relaunchedLayersPanel)
        XCTAssertTrue(
            relaunchedTrafficSceneSwitch.waitForExistence(
                timeout: Self.settleTimeoutSeconds
            )
        )
        XCTAssertEqual(
            relaunchedTrafficSceneSwitch.value as? String, "On",
            "the remembered traffic scene must be on after the relaunch"
        )

        // The device source is running again and its fix centres the camera.
        XCTAssertTrue(
            waitForLabel(
                "Active",
                element: elementWithIdentifier("device-location-status", in: relaunched)
            ),
            "the remembered device source must report an active fix"
        )
        closeMapLayersPanel(in: relaunched)
        let relaunchedFix = try readDeviceFix(in: relaunched)
        XCTAssertEqual(
            relaunchedFix.latitude, fix.latitude, accuracy: Self.locationFixToleranceDegrees,
            "the restored source must report the same simulator position"
        )
        assertCameraCenteredOnDeviceFix(in: relaunched, fix: relaunchedFix)
    }

    /// Reads the device source's reported position from the half sheet's
    /// status line, or fails when no fix arrives.
    private func readDeviceFix(
        in application: XCUIApplication
    ) throws -> (latitude: Double, longitude: Double) {
        dragSheetGrabber(application: application, direction: .up)
        let statusText = elementWithIdentifier(
            "device-navigation-status", in: application
        )
        XCTAssertTrue(
            statusText.waitForExistence(timeout: Self.emulatorFirstReadyTimeoutSeconds),
            "the half sheet must report the source's position"
        )
        var fix: (latitude: Double, longitude: Double)?
        pollForSettlement(
            timeoutDescription: "the device source's reported position",
            timeoutSeconds: Self.emulatorFirstReadyTimeoutSeconds
        ) {
            fix = Self.parseDeviceFixStatus(statusText.label)
            return fix != nil
        }
        guard let fix else {
            throw XCTSkip("the simulator did not report a device fix")
        }
        // Back to peek so the readiness chip is hittable for the camera read.
        dragSheetGrabber(application: application, direction: .down)
        return fix
    }

    /// The half sheet's position line, e.g. `Position 38.60120, -121.45120.`
    private static func parseDeviceFixStatus(
        _ label: String
    ) -> (latitude: Double, longitude: Double)? {
        let statusPrefix = "Position "
        guard label.hasPrefix(statusPrefix) else { return nil }
        let components = String(label.dropFirst(statusPrefix.count))
            .trimmingCharacters(in: CharacterSet(charactersIn: ". "))
            .split(separator: ",")
        guard components.count == 2,
              let latitude = Double(
                String(components[0]).trimmingCharacters(in: .whitespaces)
              ),
              let longitude = Double(
                String(components[1]).trimmingCharacters(in: .whitespaces)
              )
        else {
            return nil
        }
        return (latitude, longitude)
    }

    /// The diagnostics Camera block's center row must match the device fix.
    private func assertCameraCenteredOnDeviceFix(
        in application: XCUIApplication,
        fix: (latitude: Double, longitude: Double)
    ) {
        openDiagnosticsSheet(application: application)
        let sheet = diagnosticsSheet(application: application)
        var centerRow = ""
        pollForSettlement(
            timeoutDescription: "the camera centre on the device fix",
            timeoutSeconds: Self.emulatorFirstReadyTimeoutSeconds
        ) {
            centerRow = readCameraReadbackRows(sheet: sheet).first ?? ""
            let components = centerRow
                .dropFirst("center ".count)
                .split(separator: ",")
                .compactMap { component in
                    Double(String(component).trimmingCharacters(in: .whitespaces))
                }
            guard components.count == 2 else { return false }
            return abs(components[0] - fix.latitude)
                    < Self.locationFixToleranceDegrees
                && abs(components[1] - fix.longitude)
                    < Self.locationFixToleranceDegrees
        }
        closeDiagnostics(application: application)
    }

    /// The reset row clears the record; the next launch returns to the
    /// never-chose defaults.
    func testResetRememberedChoicesClearsTheRecord() throws {
        defer { resetRememberedChoicesAfterTest() }
        let application = launchFreshApplication(
            launchArguments: ["-\(Self.launchArgumentSheetHalfState)", Self.launchArgumentBooleanTrueValue]
        )
        let themeToggle = elementWithIdentifier("theme-toggle", in: application)
        XCTAssertTrue(themeToggle.waitForExistence(timeout: Self.settleTimeoutSeconds))
        themeToggle.tap()
        XCTAssertTrue(waitForLabel("Switch to dark theme", element: themeToggle))
        typeRememberedPlanFields(in: application)

        openProviderSettings(application: application)
        let settingsSheet = elementWithIdentifier("provider-settings-sheet", in: application)
        let resetRow = elementWithIdentifier(
            "provider-settings-reset-remembered-choices", in: application
        )
        scrollToMake(resetRow, visibleIn: settingsSheet)
        XCTAssertTrue(resetRow.exists && resetRow.isHittable, "the reset row must be reachable")
        resetRow.tap()
        elementWithIdentifier("provider-settings-close", in: application).tap()

        application.terminate()
        let relaunched = launchApplication(
            launchArguments: [
                "-\(Self.launchArgumentSheetHalfState)",
                Self.launchArgumentBooleanTrueValue,
            ],
            resetsRememberedChoices: false
        )
        let relaunchedThemeToggle = elementWithIdentifier(
            "theme-toggle", in: relaunched
        )
        XCTAssertTrue(
            relaunchedThemeToggle.waitForExistence(timeout: Self.settleTimeoutSeconds)
        )
        XCTAssertTrue(
            waitForLabel("Switch to light theme", element: relaunchedThemeToggle),
            "the reset must restore the Auto default"
        )
        openSearchViewArmedForOrigin(in: relaunched)
        XCTAssertEqual(
            textFieldValue(application.textFields["Search an origin"]), "",
            "the reset must clear the remembered origin text"
        )
        XCTAssertEqual(
            textFieldValue(application.textFields["Search a destination"]), "",
            "the reset must clear the remembered destination text"
        )
    }

    /// The settings surface stays inside the documented height fraction, its
    /// Close action stays visible and its last section is reachable by
    /// scrolling inside the sheet.
    func testSettingsSurfaceFitsTheViewportAndScrollsInternally() throws {
        let application = launchFreshApplication(launchArguments: [])
        openProviderSettings(application: application)
        let settingsSheet = elementWithIdentifier("provider-settings-sheet", in: application)
        let window = application.windows.firstMatch
        XCTAssertTrue(settingsSheet.exists)
        let maximumHeight = window.frame.height
            * AppLayout.phonePanelMaximumHeightFraction
        XCTAssertLessThanOrEqual(
            settingsSheet.frame.height,
            maximumHeight + Self.settingsHeightTolerancePoints,
            "the settings sheet must respect the documented height fraction"
        )
        XCTAssertGreaterThanOrEqual(settingsSheet.frame.minY, 0)
        XCTAssertLessThanOrEqual(
            settingsSheet.frame.maxY,
            window.frame.maxY + Self.settingsHeightTolerancePoints,
            "the settings sheet must stay above the screen's bottom edge"
        )
        XCTAssertTrue(
            elementWithIdentifier("provider-settings-close", in: application).isHittable,
            "the pinned Close action must stay visible"
        )
        // The base-map selection lives in the map layers panel now; the
        // settings surface must not carry its old menu row.
        XCTAssertFalse(
            elementWithIdentifier(Self.baseMapMenuIdentifier, in: application).exists,
            "the settings surface must not render a base-map row"
        )
        let resetRow = elementWithIdentifier(
            "provider-settings-reset-remembered-choices", in: application
        )
        scrollToMake(resetRow, visibleIn: settingsSheet)
        XCTAssertTrue(
            resetRow.exists && resetRow.isHittable,
            "the last settings section must be reachable inside the sheet"
        )
    }

    // MARK: Test 8b — the per-capability provider section

    /// The capability section offers one row per SDK capability, and the
    /// routing row selects a keyless provider from the shared registry: the
    /// choice reaches the SDK and the row then names the serving provider.
    /// The registry's own descriptor supplies the provider's key and display
    /// name, so the case never hardcodes a provider (keyless run: the suite
    /// injects no credential).
    func testSelectingAKeylessRoutingProviderShowsItAsTheServingProvider() throws {
        try XCTSkipIf(
            hasRuntimeInjectedApiKey,
            "a keyless routing provider needs a credential-less session"
        )
        let providerRegistry = try loadProviderRegistry()
        // The routing row's eligible keyless provider: a registry descriptor
        // that serves routing, registers on iOS and declares no credential.
        // Artifact order makes the first such descriptor the row's first
        // option.
        guard let keylessRoutingDescriptor = providerRegistry.providers.first(
            where: { descriptor in
                descriptor.servedContracts.contains(Self.routingCapabilityName)
                    && descriptor.registrationAvailable.ios
                    && descriptor.credentialLabel.trimmingCharacters(
                        in: .whitespacesAndNewlines
                    ).isEmpty
            }
        ) else {
            throw XCTSkip(
                "the shared provider registry declares no keyless routing provider for iOS"
            )
        }
        try XCTSkipUnless(
            keylessRoutingDescriptor.hasDemoBaseURL,
            "the keyless routing descriptor declares no demo deployment address"
        )

        let application = launchApplication(launchArguments: [])
        awaitReadinessChipReady(application: application)
        openProviderSettings(application: application)

        // The capability section's routing row, reached inside the sheet's
        // own scrolling content.
        let settingsSheet = elementWithIdentifier(
            "provider-settings-sheet", in: application
        )
        let routingRow = elementWithIdentifier(
            "capability-provider-row-Routing", in: application
        )
        XCTAssertTrue(
            routingRow.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the settings sheet must render one row per SDK capability"
        )
        let routingPicker = elementWithIdentifier(
            "capability-provider-picker-Routing", in: application
        )
        scrollToMake(routingPicker, visibleIn: settingsSheet)
        XCTAssertTrue(
            routingPicker.exists && routingPicker.isHittable,
            "the routing capability's picker must be reachable in the sheet"
        )

        // The picker's own selection act: the menu item composed from the
        // shared registry's descriptor and its credential state.
        routingPicker.tap()
        let keylessRoutingButton = application.buttons.matching(
            NSPredicate(
                format: "label BEGINSWITH %@", keylessRoutingDescriptor.displayName
            )
        ).firstMatch
        XCTAssertTrue(
            keylessRoutingButton.waitForExistence(
                timeout: Self.settleTimeoutSeconds
            ),
            "the routing picker must offer the registry's keyless provider, '\(keylessRoutingDescriptor.displayName)'"
        )
        XCTAssertTrue(
            keylessRoutingButton.label.contains(" · "),
            "the picker entry must compose the provider name and its state"
        )
        keylessRoutingButton.tap()

        // The accepted selection refreshes the row from the SDK readback, so
        // the row names the provider now serving the capability.
        pollForSettlement(
            timeoutDescription: "the routing row naming the selected keyless provider"
        ) {
            routingPicker.exists
                && routingPicker.label.contains(
                    keylessRoutingDescriptor.displayName
                )
        }
        XCTAssertTrue(
            routingPicker.label.contains(keylessRoutingDescriptor.displayName),
            "the routing row must name the selected provider, '\(keylessRoutingDescriptor.displayName)', was '\(routingPicker.label)'"
        )
    }

    // MARK: Test 8c — the remembered routing choice and a fixture-backed plan

    /// The proof: the settings routing row selects the registry's keyless
    /// provider, the plan resolves from the committed OSRM payload served by
    /// the loopback fixture server, and a relaunch restores the same serving
    /// provider and plans from the fixture again. Every provider fact comes
    /// from the shared registry; no request leaves the simulator.
    func testTheRememberedKeylessRoutingChoicePlansFromTheFixtureAfterRelaunch() throws {
        try XCTSkipIf(
            hasRuntimeInjectedApiKey,
            "a keyless routing provider needs a credential-less session"
        )
        let providerRegistry = try loadProviderRegistry()
        guard let keylessRoutingDescriptor = providerRegistry.providers.first(
            where: { descriptor in
                descriptor.servedContracts.contains(Self.routingCapabilityName)
                    && descriptor.registrationAvailable.ios
                    && descriptor.credentialLabel.trimmingCharacters(
                        in: .whitespacesAndNewlines
                    ).isEmpty
            }
        ) else {
            throw XCTSkip(
                "the shared provider registry declares no keyless routing provider for iOS"
            )
        }
        try XCTSkipUnless(
            keylessRoutingDescriptor.hasDemoBaseURL,
            "the keyless routing descriptor declares no demo deployment address"
        )

        // The committed OSRM payload answers every route request; the same
        // fixture is the expected answer's source, so the facts below are
        // derived from it rather than copied.
        let routeFixtureBody = try fixtureData(
            named: Self.osrmRouteFixtureDocumentName
        )
        let expectedRouteFacts = try Self.routeFacts(fromFixtureBody: routeFixtureBody)
        let server = try FixtureSearchServer { _, _ in
            FixtureSearchResponse(
                statusCode: 200,
                body: routeFixtureBody,
                delaySeconds: 0
            )
        }
        let port = try server.start()
        defer { server.stop() }
        let routingDeploymentBaseURL = "http://127.0.0.1:\(port)"

        // First run: choose the keyless routing provider through the settings
        // row's own picker, then plan the demo pair through it.
        let application = launchApplication(
            launchArguments: [
                "-DemoOpenSearch", Self.launchArgumentBooleanTrueValue,
            ],
            injectsRuntimeCredential: false,
            routingDeploymentBaseURL: routingDeploymentBaseURL
        )
        selectKeylessRoutingProvider(
            keylessRoutingDescriptor,
            in: application
        )
        closeProviderSettings(application: application)
        planTypedDemoPair(application: application)
        XCTAssertTrue(
            elementWithIdentifier("route-summary-card", in: application)
                .waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the selected routing provider must plan a route"
        )
        XCTAssertTrue(
            application.staticTexts[expectedRouteFacts.distance].exists,
            "the summary must carry the fixture route's distance, expected '\(expectedRouteFacts.distance)'"
        )
        XCTAssertTrue(
            application.staticTexts[expectedRouteFacts.duration].exists,
            "the summary must carry the fixture route's duration, expected '\(expectedRouteFacts.duration)'"
        )
        XCTAssertFalse(
            elementWithIdentifier("route-summary-recorded-note", in: application).exists,
            "a provider plan must not claim the recorded demo route"
        )
        XCTAssertTrue(
            server.requests().contains { request in
                request.path.hasPrefix(Self.routingRequestPathPrefix)
            },
            "the plan must ask the selected provider at its fixture deployment"
        )
        let requestsAfterFirstPlan = server.requests().count

        // Relaunch without clearing the record: the settings row restores the
        // choice and the plan reaches the fixture again.
        application.terminate()
        let relaunchedApplication = launchApplication(
            launchArguments: [
                "-DemoOpenSearch", Self.launchArgumentBooleanTrueValue,
            ],
            injectsRuntimeCredential: false,
            resetsRememberedChoices: false,
            routingDeploymentBaseURL: routingDeploymentBaseURL
        )
        openProviderSettings(application: relaunchedApplication)
        let restoredRoutingPicker = elementWithIdentifier(
            "capability-provider-picker-Routing", in: relaunchedApplication
        )
        XCTAssertTrue(
            restoredRoutingPicker.waitForExistence(
                timeout: Self.settleTimeoutSeconds
            ),
            "the settings sheet must render the routing row after the relaunch"
        )
        XCTAssertTrue(
            restoredRoutingPicker.label.contains(
                keylessRoutingDescriptor.displayName
            ),
            "the relaunch must restore the serving routing provider, was '\(restoredRoutingPicker.label)'"
        )
        closeProviderSettings(application: relaunchedApplication)
        // The remembered pair fills the fields on launch; the plan action
        // therefore runs without a second typing pass.
        XCTAssertTrue(
            waitForTextFieldValueContaining(
                "Capitol Park",
                textField: relaunchedApplication.textFields["Search an origin"]
            ),
            "the relaunch must restore the remembered origin text"
        )
        XCTAssertTrue(
            waitForTextFieldValueContaining(
                "Tower Bridge",
                textField: relaunchedApplication.textFields["Search a destination"]
            ),
            "the relaunch must restore the remembered destination text"
        )
        tapPlanRoute(application: relaunchedApplication)
        XCTAssertTrue(
            elementWithIdentifier("route-summary-card", in: relaunchedApplication)
                .waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the restored routing provider must plan the route again"
        )
        XCTAssertTrue(
            relaunchedApplication.staticTexts[expectedRouteFacts.distance].exists,
            "the restored plan must carry the fixture route's distance"
        )
        XCTAssertTrue(
            server.requests().count > requestsAfterFirstPlan,
            "the restored choice must reach the provider's fixture deployment again"
        )
        XCTAssertFalse(
            elementWithIdentifier("error-report", in: relaunchedApplication).exists,
            "the restored fixture-backed plan reports nothing in the error strip"
        )
    }

    /// Selects the registry's keyless routing provider through the settings
    /// row's picker and awaits the row naming it.
    private func selectKeylessRoutingProvider(
        _ keylessRoutingDescriptor: ProviderDescriptor,
        in application: XCUIApplication
    ) {
        openProviderSettings(application: application)
        let settingsSheet = elementWithIdentifier(
            "provider-settings-sheet", in: application
        )
        let routingPicker = elementWithIdentifier(
            "capability-provider-picker-Routing", in: application
        )
        scrollToMake(routingPicker, visibleIn: settingsSheet)
        XCTAssertTrue(
            routingPicker.exists && routingPicker.isHittable,
            "the routing capability's picker must be reachable in the sheet"
        )
        routingPicker.tap()
        let keylessRoutingButton = application.buttons.matching(
            NSPredicate(
                format: "label BEGINSWITH %@",
                keylessRoutingDescriptor.displayName
            )
        ).firstMatch
        XCTAssertTrue(
            keylessRoutingButton.waitForExistence(
                timeout: Self.settleTimeoutSeconds
            ),
            "the routing picker must offer the registry's keyless provider"
        )
        keylessRoutingButton.tap()
        pollForSettlement(
            timeoutDescription: "the routing row naming the selected keyless provider"
        ) {
            routingPicker.exists
                && routingPicker.label.contains(
                    keylessRoutingDescriptor.displayName
                )
        }
    }

    /// Closes the provider settings sheet through its own close control.
    private func closeProviderSettings(application: XCUIApplication) {
        let closeControl = elementWithIdentifier(
            "provider-settings-close", in: application
        )
        XCTAssertTrue(
            closeControl.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the settings sheet must offer its close control"
        )
        closeControl.tap()
        XCTAssertTrue(
            waitForNonexistence(
                elementWithIdentifier("provider-settings-sheet", in: application)
            ),
            "the close control must dismiss the settings sheet"
        )
    }

    /// Types the artifact's demo pair into the plan fields and taps Plan
    /// route; exact demo-place names resolve without a search provider.
    private func planTypedDemoPair(application: XCUIApplication) {
        let originField = application.textFields["Search an origin"]
        XCTAssertTrue(
            originField.waitForExistence(timeout: Self.settleTimeoutSeconds)
        )
        originField.tap()
        originField.typeText("Capitol Park\n")
        let destinationField = application.textFields["Search a destination"]
        XCTAssertTrue(
            destinationField.waitForExistence(timeout: Self.settleTimeoutSeconds)
        )
        destinationField.tap()
        destinationField.typeText("Tower Bridge\n")
        tapPlanRoute(application: application)
    }

    /// Taps the plan surface's explicit Plan route action.
    private func tapPlanRoute(application: XCUIApplication) {
        let planRouteButton = elementWithIdentifier(
            "plan-route-button", in: application
        )
        XCTAssertTrue(
            planRouteButton.waitForExistence(timeout: Self.settleTimeoutSeconds)
        )
        XCTAssertTrue(
            planRouteButton.isEnabled,
            "both resolved demo names must enable Plan route"
        )
        planRouteButton.tap()
    }

    /// Reads one committed fixture document from the UI test bundle.
    private func fixtureData(named documentName: String) throws -> Data {
        let bundle = Bundle(for: Self.self)
        guard let fixtureURL = bundle.url(
            forResource: documentName,
            withExtension: "json"
        ) else {
            throw FixtureSearchServerError.fixtureMissing(
                documentName: documentName
            )
        }
        return try Data(contentsOf: fixtureURL)
    }

    /// The distance and duration phrases the route summary renders, derived
    /// from the committed OSRM payload's first route.
    private static func routeFacts(
        fromFixtureBody fixtureBody: Data
    ) throws -> (distance: String, duration: String) {
        let payload = try XCTUnwrap(
            JSONSerialization.jsonObject(with: fixtureBody) as? [String: Any],
            "the OSRM fixture must decode as a JSON object"
        )
        let routes = try XCTUnwrap(
            payload["routes"] as? [[String: Any]],
            "the OSRM fixture must carry its routes"
        )
        let route = try XCTUnwrap(routes.first, "the fixture carries no route")
        let distanceMeters = try XCTUnwrap(
            route["distance"] as? Double,
            "the fixture route must carry its distance"
        )
        let durationSeconds = try XCTUnwrap(
            route["duration"] as? Double,
            "the fixture route must carry its duration"
        )
        let distancePhrase = distanceMeters < 1000
            ? "\(Int(distanceMeters.rounded())) m"
            : String(format: "%.1f km", distanceMeters / 1000)
        let wholeMinutes = Int((durationSeconds / 60).rounded(.down))
        let remainderSeconds = durationSeconds - Double(wholeMinutes) * 60
        let durationPhrase = wholeMinutes > 0
            ? "\(wholeMinutes) min \(Int(remainderSeconds.rounded())) s"
            : "\(Int(durationSeconds.rounded())) s"
        return (distance: distancePhrase, duration: durationPhrase)
    }

    // MARK: Test 8c — provider rows, typed-text planning and diagnostics

    /// Every provider picker entry composes the display name and the
    /// credential state; the routing row states the demo address and terms
    /// when the serving descriptor declares them.
    func testProviderPickerEntriesComposeNameAndState() throws {
        try XCTSkipIf(
            hasRuntimeInjectedApiKey,
            "the keyless composition case needs a credential-less session"
        )
        let providerRegistry = try loadProviderRegistry()
        guard let routingDescriptor = providerRegistry.providers.first(
            where: { descriptor in
                descriptor.servedContracts.contains(Self.routingCapabilityName)
                    && descriptor.registrationAvailable.ios
            }
        ) else {
            throw XCTSkip("the registry declares no iOS routing provider")
        }

        let application = launchApplication(launchArguments: [])
        awaitReadinessChipReady(application: application)
        openProviderSettings(application: application)
        let routingPicker = elementWithIdentifier(
            "capability-provider-picker-Routing", in: application
        )
        XCTAssertTrue(
            routingPicker.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the settings sheet must render the routing row"
        )
        routingPicker.tap()
        let routingEntry = application.buttons.matching(
            NSPredicate(format: "label BEGINSWITH %@", routingDescriptor.displayName)
        ).firstMatch
        XCTAssertTrue(
            routingEntry.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the picker must offer the registry's routing provider"
        )
        XCTAssertTrue(
            routingEntry.label.contains(" · "),
            "a picker entry must compose the provider name and its state, was '\(routingEntry.label)'"
        )
    }

    /// Typed text is resolved, not rejected: typing the exact demo-place
    /// names and tapping Plan route plans the recorded route without a
    /// result tap.
    func testTypedDemoPlaceNamesPlanTheRecordedRouteWithoutAResultTap() throws {
        try XCTSkipIf(
            hasRuntimeInjectedApiKey,
            "the keyless typed-text case needs a credential-less session"
        )
        let application = launchApplication(launchArguments: [
            "-DemoOpenSearch", Self.launchArgumentBooleanTrueValue,
        ])
        let originField = application.textFields["Search an origin"]
        XCTAssertTrue(
            originField.waitForExistence(timeout: Self.settleTimeoutSeconds)
        )
        originField.tap()
        originField.typeText("Capitol Park\n")
        let destinationField = application.textFields["Search a destination"]
        XCTAssertTrue(
            destinationField.waitForExistence(timeout: Self.settleTimeoutSeconds)
        )
        destinationField.tap()
        destinationField.typeText("Tower Bridge\n")

        let planRouteButton = elementWithIdentifier("plan-route-button", in: application)
        XCTAssertTrue(planRouteButton.waitForExistence(timeout: Self.settleTimeoutSeconds))
        XCTAssertTrue(
            planRouteButton.isEnabled,
            "both typed fields must enable Plan route"
        )
        planRouteButton.tap()

        XCTAssertTrue(
            elementWithIdentifier("route-summary-card", in: application)
                .waitForExistence(timeout: Self.settleTimeoutSeconds),
            "typed demo-place names must resolve and plan without a result tap"
        )
        XCTAssertTrue(
            application.staticTexts["Recorded demo route, pedestrian, fastest."].exists,
            "the typed plan must apply the artifact's recorded route"
        )
    }

    /// The diagnostics pane starts with no listener: the pane opens on the
    /// Info stream with no debug detail, and the explicit Show debug events
    /// switch reveals the Debug opt-in and its per-session scope note.
    func testDiagnosticsOpenAtInfoAndTheDebugSwitchIsExplicit() throws {
        let application = launchApplication(launchArguments: [
            "-DemoOpenDiagnostics", Self.launchArgumentBooleanTrueValue,
        ])
        let diagnosticsSheet = elementWithIdentifier(
            "diagnostics-sheet", in: application
        )
        XCTAssertTrue(
            diagnosticsSheet.waitForExistence(timeout: Self.emulatorFirstReadyTimeoutSeconds),
            "the readiness chip's destination must open"
        )
        let debugToggle = elementWithIdentifier(
            "diagnostics-debug-toggle", in: application
        )
        XCTAssertTrue(
            debugToggle.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the pane must offer the explicit debug switch"
        )
        XCTAssertTrue(
            application.staticTexts["Show debug events"].exists,
            "debug detail must start off"
        )
        XCTAssertFalse(
            elementWithIdentifier("diagnostics-debug-scope-note", in: application).exists,
            "the per-session scope note appears only after the switch lowers the gate"
        )
        // The pane's clear action is present with the bounded stream.
        XCTAssertTrue(
            elementWithIdentifier("diagnostics-clear", in: application).exists,
            "the pane must offer Clear beside the bounded stream"
        )
    }

    // MARK: The suite's shared driving helpers

    /// Launches with the remembered choices cleared first, so a persistence
    /// case starts from the never-chose defaults.
    private func launchFreshApplication(
        launchArguments: [String],
        injectsRuntimeCredential: Bool = true
    ) -> XCUIApplication {
        launchApplication(
            launchArguments: launchArguments,
            injectsRuntimeCredential: injectsRuntimeCredential,
            resetsRememberedChoices: true
        )
    }

    /// Restores the never-chose defaults for the following case (a relaunch
    /// with the record reset, then a clean exit).
    private func resetRememberedChoicesAfterTest() {
        application?.terminate()
        let cleanupApplication = XCUIApplication()
        cleanupApplication.launchArguments = [
            "-\(Self.launchArgumentResetRememberedChoices)",
            Self.launchArgumentBooleanTrueValue,
        ]
        cleanupApplication.launch()
        cleanupApplication.terminate()
        application = nil
    }

    /// Types both plan fields through the search view without submitting:
    /// typing never plans, and the fields' last text is what the record keeps.
    private func typeRememberedPlanFields(in application: XCUIApplication) {
        openSearchViewArmedForOrigin(in: application)
        let originField = application.textFields["Search an origin"]
        XCTAssertTrue(
            originField.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the search view's origin field must render"
        )
        originField.tap()
        originField.typeText(Self.rememberedOriginText)
        let destinationField = application.textFields["Search a destination"]
        XCTAssertTrue(
            destinationField.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the search view's destination field must render"
        )
        destinationField.tap()
        destinationField.typeText(Self.rememberedDestinationText)
    }

    /// Opens the search view armed for the origin through the peek entry.
    private func openSearchViewArmedForOrigin(in application: XCUIApplication) {
        dragSheetGrabber(application: application, direction: .down)
        let setOriginControl = elementWithIdentifier(
            "home-sheet-peek-set-origin", in: application
        )
        XCTAssertTrue(
            setOriginControl.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the peek's origin entry must render"
        )
        setOriginControl.tap()
        XCTAssertTrue(
            elementWithIdentifier("search-view-origin-field", in: application)
                .waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the origin entry must open the search view"
        )
    }

    /// A text field's typed value; an untouched field reports its placeholder
    /// as the value, which this helper maps back to the empty string.
    private func textFieldValue(_ textField: XCUIElement) -> String {
        let value = textField.value as? String ?? ""
        return value == textField.placeholderValue ? "" : value
    }

    /// Launches a fresh application instance with the given launch
    /// arguments (the documented demo-automation flags, `-key` form; no
    /// flags means the plain launch the ladder test covers). A case that
    /// must stay keyless opts out of the runtime credential seam, so its
    /// search and plan flow reads the shared artifact's recorded route
    /// rather than reaching a provider. Every ordinary launch clears the
    /// remembered choices first, so one case cannot inherit another's; the
    /// persistence cases pass `resetsRememberedChoices: false` on relaunch.
    private func launchApplication(
        launchArguments: [String],
        injectsRuntimeCredential: Bool = true,
        resetsRememberedChoices: Bool = true,
        routingDeploymentBaseURL: String? = nil
    ) -> XCUIApplication {
        let application = XCUIApplication()
        application.launchArguments = (
            resetsRememberedChoices
                ? [
                    "-\(Self.launchArgumentResetRememberedChoices)",
                    Self.launchArgumentBooleanTrueValue,
                ]
                : []
        ) + launchArguments
        if injectsRuntimeCredential, let runtimeCredential = runtimeInjectedApiKey {
            // The automation credential seam (§6.5): the app applies it at
            // launch and never persists it.
            application.launchEnvironment["PROVIDER_API_KEY"] = runtimeCredential
        }
        if injectsRuntimeCredential, let runtimeProviderKey = runtimeInjectedProviderKey {
            application.launchEnvironment["PROVIDER_KEY"] = runtimeProviderKey
        }
        // Point the keyless search provider at a dead loopback address: the
        // cases that type into the plan fields must not reach the registry's
        // public demo server. The dedicated type-ahead cases start their own
        // fixture server and set this variable themselves.
        application.launchEnvironment[Self.deploymentBaseURLEnvironmentKey] =
            Self.unreachableDeploymentBaseURL
        // The routing proof points the keyless routing provider at its own
        // loopback fixture server; every other case keeps the registry's
        // declared demo address.
        if let routingDeploymentBaseURL {
            application.launchEnvironment[
                Self.routingDeploymentBaseURLEnvironmentKey
            ] = routingDeploymentBaseURL
        }
        application.launch()
        self.application = application
        return application
    }

    /// Grants the when-in-use authorization prompt the device source raises
    /// (the system alert or Springboard's, depending on the runner).
    private func grantWhenInUseLocationAuthorization(in application: XCUIApplication) {
        let authorizationAlert = application.alerts.firstMatch
        if authorizationAlert.waitForExistence(timeout: 5) {
            for allowLabel in ["Allow While Using App", "Allow Once", "Allow"] {
                let allowButton = authorizationAlert.buttons[allowLabel]
                if allowButton.exists {
                    allowButton.tap()
                    return
                }
            }
        } else {
            let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
            for allowLabel in ["Allow While Using App", "Allow Once", "Allow"] {
                let allowButton = springboard.buttons[allowLabel]
                if allowButton.waitForExistence(timeout: 2) {
                    allowButton.tap()
                    return
                }
            }
        }
    }

    /// Plans the shared artifact's recorded Capitol Park -> Tower Bridge
    /// route through the search view's own fields, submit controls, result
    /// rows and explicit Plan route action (the keyless path; no provider
    /// call, no invented geometry).
    private func planRecordedDemoRoute(application: XCUIApplication) {
        let originField = application.textFields["Search an origin"]
        XCTAssertTrue(originField.waitForExistence(timeout: Self.settleTimeoutSeconds))
        originField.tap()
        originField.typeText("Capitol Park\n")
        let originResult = searchResultRow(containing: "Capitol Park", in: application)
        XCTAssertTrue(originResult.waitForExistence(timeout: Self.settleTimeoutSeconds))
        revealSearchResult(originResult, in: application)
        originResult.tap()

        let destinationField = application.textFields["Search a destination"]
        XCTAssertTrue(destinationField.waitForExistence(timeout: Self.settleTimeoutSeconds))
        destinationField.tap()
        destinationField.typeText("Tower Bridge\n")
        let destinationResult = searchResultRow(containing: "Tower Bridge", in: application)
        XCTAssertTrue(destinationResult.waitForExistence(timeout: Self.settleTimeoutSeconds))
        revealSearchResult(destinationResult, in: application)
        destinationResult.tap()

        let planRouteButton = elementWithIdentifier("plan-route-button", in: application)
        XCTAssertTrue(planRouteButton.waitForExistence(timeout: Self.settleTimeoutSeconds))
        XCTAssertTrue(planRouteButton.isEnabled, "both assigned fields enable Plan route")
        planRouteButton.tap()
    }

    /// Brings a search result row above the keyboard: the plan surface's own
    /// scroll dismisses the keyboard, so the row becomes hittable.
    private func revealSearchResult(
        _ result: XCUIElement,
        in application: XCUIApplication
    ) {
        guard !result.isHittable else { return }
        // The header's dismiss control hides the keyboard; the row below it
        // then sits inside the sheet's visible area. A swipe of the plan
        // surface is the fallback for a row pushed further down.
        let keyboardDismiss = elementWithIdentifier(
            "search-keyboard-dismiss", in: application
        )
        let didDismiss = keyboardDismiss.exists
        if didDismiss {
            keyboardDismiss.tap()
        }
        let searchScrollView = application.scrollViews.firstMatch
        for _ in 0 ..< maximumSearchRevealAttempts {
            if result.isHittable {
                return
            }
            guard searchScrollView.exists else { continue }
            searchScrollView.swipeUp()
        }
        XCTAssertTrue(
            result.isHittable,
            "the search result row must be reachable (frame \(result.frame), screen \(application.frame), scroll \(searchScrollView.frame), dismiss \(didDismiss), keyboards \(application.keyboards.count))"
        )
    }

    /// The result row whose rendered label names `placeName`.
    private func searchResultRow(
        containing placeName: String,
        in application: XCUIApplication
    ) -> XCUIElement {
        application.descendants(matching: .any)
            .matching(identifier: "search-result-row")
            .matching(NSPredicate(format: "label CONTAINS %@", placeName))
            .firstMatch
    }

    /// Polls until the element's label equals `expectedLabel`.
    private func waitForLabel(
        _ expectedLabel: String,
        element: XCUIElement,
        timeout: TimeInterval = defaultAssertionTimeoutSeconds
    ) -> Bool {
        let labelPredicate = NSPredicate { _, _ in
            element.exists && element.label == expectedLabel
        }
        let labelExpectation = XCTNSPredicateExpectation(
            predicate: labelPredicate,
            object: nil
        )
        return XCTWaiter.wait(for: [labelExpectation], timeout: timeout) == .completed
    }

    /// Polls until the element's label contains `expectedText`.
    private func waitForLabelContaining(
        _ expectedText: String,
        element: XCUIElement,
        timeout: TimeInterval = defaultAssertionTimeoutSeconds
    ) -> Bool {
        let labelPredicate = NSPredicate { _, _ in
            element.exists && element.label.contains(expectedText)
        }
        let labelExpectation = XCTNSPredicateExpectation(
            predicate: labelPredicate,
            object: nil
        )
        return XCTWaiter.wait(for: [labelExpectation], timeout: timeout) == .completed
    }

    /// Polls until the element renders a non-empty label.
    private func waitForNonemptyLabel(
        element: XCUIElement,
        timeout: TimeInterval = defaultAssertionTimeoutSeconds
    ) -> Bool {
        let labelPredicate = NSPredicate { _, _ in
            element.exists && !element.label.isEmpty
        }
        let labelExpectation = XCTNSPredicateExpectation(
            predicate: labelPredicate,
            object: nil
        )
        return XCTWaiter.wait(for: [labelExpectation], timeout: timeout) == .completed
    }

    /// Polls until the element leaves the accessibility tree.
    private func waitForNonexistence(
        _ element: XCUIElement,
        timeout: TimeInterval = defaultAssertionTimeoutSeconds
    ) -> Bool {
        let nonexistencePredicate = NSPredicate { _, _ in !element.exists }
        let nonexistenceExpectation = XCTNSPredicateExpectation(
            predicate: nonexistencePredicate,
            object: nil
        )
        return XCTWaiter.wait(for: [nonexistenceExpectation], timeout: timeout) == .completed
    }

    /// The shared provider registry, decoded from the test bundle's copy of
    /// the single committed artifact (`platform/providers.json`) with the
    /// application's own schema type. The styled-base case names the demo base
    /// map and the vector style from it, so a registry edit is a data change,
    /// not a test rewrite.
    private func loadProviderRegistry() throws -> ProviderRegistryCatalog {
        let testBundle = Bundle(for: ReferenceApplicationUITests.self)
        guard let registryURL = testBundle.url(
            forResource: "providers", withExtension: "json"
        ) else {
            throw XCTSkip(
                "the shared provider registry resource is missing from the UI test bundle"
            )
        }
        return try ProviderRegistryCatalog.decoded(
            from: Data(contentsOf: registryURL)
        )
    }

    /// Opens the provider-settings sheet (the gear control's own tap act).
    private func openProviderSettings(application: XCUIApplication) {
        let settingsGear = elementWithIdentifier(
            "provider-settings-gear", in: application
        )
        XCTAssertTrue(
            settingsGear.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the gear control must open the provider-settings sheet"
        )
        settingsGear.tap()
        XCTAssertTrue(
            elementWithIdentifier("provider-settings-sheet", in: application)
                .waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the gear's tap must present the provider-settings sheet"
        )
    }

    /// Awaits the readiness ladder's ready state (the first honest settle;
    /// the chip's text is the ladder's rendered short line). The bound is
    /// the Android suite's minutes-scaled emulator settle bound.
    private func awaitReadinessChipReady(application: XCUIApplication) {
        let readinessChipElement = readinessChip(application: application)
        let readinessReadyPredicate = NSPredicate { _, _ in
            readinessChipElement.exists && readinessChipElement.label == "Ready"
        }
        let readinessReadyExpectation = XCTNSPredicateExpectation(
            predicate: readinessReadyPredicate,
            object: nil
        )
        XCTAssertEqual(
            XCTWaiter.wait(
                for: [readinessReadyExpectation],
                timeout: Self.emulatorFirstReadyTimeoutSeconds
            ),
            .completed,
            "the readiness ladder must settle on the ready state, '\(readinessChipElement.exists ? readinessChipElement.label : "absent")'"
        )
    }

    private func readinessChip(application: XCUIApplication) -> XCUIElement {
        elementWithIdentifier("readiness-chip", in: application)
    }

    /// The camera readback's `center <lat>, <lon>` row parsed into degrees
    /// (the application renders dot-decimal `String(format:)` numbers, and
    /// `Double`'s parser is locale-free), `nil` when the row is absent or
    /// unparseable.
    private static func centerRowCoordinates(
        of centerRow: String
    ) -> (latitude: Double, longitude: Double)? {
        let coordinateText = centerRow.dropFirst("center ".count)
        let coordinateParts = coordinateText
            .split(separator: ",")
            .map { $0.trimmingCharacters(in: .whitespaces) }
        guard coordinateParts.count == 2,
            let latitude_degrees = Double(coordinateParts[0]),
            let longitude_degrees = Double(coordinateParts[1])
        else {
            return nil
        }
        return (latitude_degrees, longitude_degrees)
    }

    /// The diagnostics sheet's element (its accessibility identifier
    /// scopes the readback row queries to the sheet's own subtree).
    private func diagnosticsSheet(application: XCUIApplication) -> XCUIElement {
        elementWithIdentifier("diagnostics-sheet", in: application)
    }

    /// Opens the diagnostics sheet (the readiness chip's own tap act).
    private func openDiagnosticsSheet(application: XCUIApplication) {
        let readinessChipElement = readinessChip(application: application)
        XCTAssertTrue(
            readinessChipElement.exists && readinessChipElement.isHittable,
            "the readiness chip must be the diagnostics sheet's opening control"
        )
        readinessChipElement.tap()
        XCTAssertTrue(
            diagnosticsSheet(application: application)
                .waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the readiness chip's tap must open the diagnostics sheet"
        )
    }

    /// Opens the diagnostics sheet (the readiness chip's own tap act) and
    /// reads the camera block's four readback rows, optionally waiting for
    /// the rows to satisfy the given settle condition first (each
    /// condition poll is a re-snapshot of the rendered tree — the readback
    /// is per drawn frame, so the wait is the drawn-frame settles).
    private func openDiagnosticsAndReadCameraRows(
        application: XCUIApplication,
        settleCondition: (([String]) -> Bool)? = nil,
        closesSheetAfterReading: Bool = true
    ) -> [String] {
        openDiagnosticsSheet(application: application)
        let diagnosticsSheetElement = diagnosticsSheet(application: application)
        XCTAssertTrue(
            diagnosticsSheetElement.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the readiness chip's tap must open the diagnostics sheet"
        )
        let sheetScrollView = diagnosticsSheetElement.scrollViews.firstMatch
        if let settleCondition {
            pollForSettlement(
                timeoutDescription: "the camera readback rows satisfying the awaited condition"
            ) {
                settleCondition(readCameraReadbackRows(sheet: diagnosticsSheetElement))
            }
        }
        let cameraRows = readCameraReadbackRowsWithScrolling(
            sheet: diagnosticsSheetElement,
            scrollView: sheetScrollView
        )
        if closesSheetAfterReading {
            closeDiagnostics(application: application)
        }
        return cameraRows
    }

    private func closeDiagnostics(application: XCUIApplication) {
        let closeControl = elementWithIdentifier("diagnostics-close", in: application)
        if closeControl.exists {
            closeControl.tap()
        }
        XCTAssertTrue(
            pollForNonExistence(
                elementWithIdentifier("diagnostics-sheet", in: application),
                within: Self.settleTimeoutSeconds
            ),
            "the diagnostics close control must dismiss the sheet"
        )
    }

    /// The diagnostics sheet's camera readback rows (the Camera block's
    /// lines — center, zoom, bearing and pitch, in the block's fixed
    /// order); an unrendered row reads as the empty string.
    private func readCameraReadbackRows(sheet: XCUIElement) -> [String] {
        Self.cameraReadbackRowPrefixes.map { rowPrefix in
            let rowQuery = sheet.staticTexts
                .matching(NSPredicate(format: "label BEGINSWITH %@", rowPrefix + " "))
                .firstMatch
            guard rowQuery.exists, !rowQuery.label.isEmpty else { return "" }
            return rowQuery.label
        }
    }

    /// The camera readback rows' scrolled read: the sheet's content
    /// scrolls until every row's label is readable (the Events backlog
    /// can push the Camera block below the fold; the content is not
    /// lazy, so off-screen labels usually read anyway — the scroll is the
    /// fallback, bounded).
    private func readCameraReadbackRowsWithScrolling(
        sheet: XCUIElement,
        scrollView: XCUIElement?
    ) -> [String] {
        for _ in 0 ..< Self.maximumScrollAttempts {
            let cameraRows = readCameraReadbackRows(sheet: sheet)
            if cameraRows.allSatisfy({ !$0.isEmpty }) {
                return cameraRows
            }
            guard let scrollView, scrollView.exists else { break }
            scrollDownElement(scrollView)
        }
        return readCameraReadbackRows(sheet: sheet)
    }

    /// The diagnostics sheet's confirmed-tap count row (the sheet's own
    /// accessibility identifier; the row's label is `map taps <count>`).
    private func mapTapCountRow(application: XCUIApplication) -> XCUIElement {
        elementWithIdentifier("diagnostics-map-tap-count", in: application)
    }

    /// The row's rendered label for one expected count.
    private static func mapTapCountLabel(tapCount: Int) -> String {
        mapTapCountRowPrefix + String(tapCount)
    }

    /// The camera zoom level parsed from a `zoom <levels>` readback row
    /// (the application renders two decimals; the parser is locale-free),
    /// `nil` when the row is absent or unparseable.
    private static func zoomLevel(fromCameraRow zoomRow: String) -> Double? {
        guard zoomRow.hasPrefix("zoom ") else { return nil }
        return Double(zoomRow.dropFirst("zoom ".count))
    }

    /// The diagnostics sheet's camera zoom readback, read through the
    /// sheet's own open/read/close cycle.
    private func readCameraZoomLevel(application: XCUIApplication) -> Double? {
        let cameraRows = openDiagnosticsAndReadCameraRows(application: application)
        return Self.zoomLevel(fromCameraRow: cameraRows[Self.zoomRowIndex])
    }

    /// The camera zoom readback from an already-open diagnostics sheet
    /// (scrolling the sheet when the Events backlog pushes the Camera
    /// block below the fold).
    private func cameraZoomLevelFromOpenSheet(
        application: XCUIApplication
    ) -> Double? {
        let sheet = diagnosticsSheet(application: application)
        let cameraRows = readCameraReadbackRowsWithScrolling(
            sheet: sheet,
            scrollView: sheet.scrollViews.firstMatch
        )
        return Self.zoomLevel(fromCameraRow: cameraRows[Self.zoomRowIndex])
    }

    /// The diagnostics sheet's confirmed-tap count (opens the sheet, reads
    /// the row, closes the sheet so the next act can reach the map).
    private func readMapTapCount(application: XCUIApplication) -> Int? {
        openDiagnosticsSheet(application: application)
        let tapCountRow = mapTapCountRow(application: application)
        XCTAssertTrue(
            tapCountRow.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the diagnostics sheet must render the confirmed-tap count row"
        )
        let tapCountLabel = tapCountRow.label
        closeDiagnostics(application: application)
        guard tapCountLabel.hasPrefix(Self.mapTapCountRowPrefix) else { return nil }
        return Int(tapCountLabel.dropFirst(Self.mapTapCountRowPrefix.count))
    }

    /// The toggled-on scene's read seam: opens the diagnostics sheet and
    /// awaits the overlay triangles line settling away from the given
    /// line (the scene joining the pushed overlay list), then closes the
    /// sheet again (the chips toggle under a dismissed sheet).
    private func openDiagnosticsAndReadChanged(
        application: XCUIApplication,
        fromLine: String
    ) -> String {
        openDiagnosticsSheet(application: application)
        let changedLine = awaitChangedOverlayTrianglesLine(
            application: application,
            fromLine: fromLine
        )
        closeDiagnostics(application: application)
        return changedLine
    }

    /// The toggled-off scene's read seam: opens the diagnostics sheet and
    /// awaits the overlay triangles line settling back to the given line
    /// (the scene leaving the pushed overlay list), then closes the sheet.
    private func reopenDiagnosticsAndAwaitRestored(
        application: XCUIApplication,
        fromLine: String,
        toLine: String
    ) {
        openDiagnosticsSheet(application: application)
        awaitRestoredOverlayTrianglesLine(
            application: application,
            fromLine: fromLine,
            toLine: toLine
        )
        closeDiagnostics(application: application)
    }

    /// The tiles block's overlay-triangles row query (the drawn-frame
    /// witness of the pushed overlay list; a prefix label match — the
    /// row's own live count trails the label).
    private func overlayTrianglesRow(
        application: XCUIApplication
    ) -> XCUIElement {
        diagnosticsSheet(application: application).staticTexts
            .matching(NSPredicate(
                format: "label BEGINSWITH %@",
                Self.overlayTrianglesRowPrefix + " "
            ))
            .firstMatch
    }

    /// The tiles block's overlay-triangles line (the drawn-frame witness
    /// of the pushed overlay list), awaited until it renders, scrolling
    /// the sheet when the line sits below the fold.
    private func awaitOverlayTrianglesLine(
        application: XCUIApplication
    ) -> String {
        let overlayTrianglesRow = overlayTrianglesRow(application: application)
        let sheetScrollView: XCUIElement? = diagnosticsSheet(application: application)
            .scrollViews.firstMatch
        for _ in 0 ..< Self.maximumScrollAttempts {
            if overlayTrianglesRow.exists, !overlayTrianglesRow.label.isEmpty {
                return overlayTrianglesRow.label
            }
            if let sheetScrollView, sheetScrollView.exists {
                scrollDownElement(sheetScrollView)
            }
        }
        return overlayTrianglesRow.exists ? overlayTrianglesRow.label : ""
    }

    /// Waits until the overlay triangles line differs from the given line
    /// (a pushed overlay joining the live accounting), then returns the
    /// settled line.
    private func awaitChangedOverlayTrianglesLine(
        application: XCUIApplication,
        fromLine: String
    ) -> String {
        let lineQuery = overlayTrianglesRow(application: application)
        pollForSettlement(
            timeoutDescription: "the toggled-on scene joining the pushed overlay list"
        ) {
            lineQuery.exists
                && !lineQuery.label.isEmpty
                && lineQuery.label != fromLine
        }
        return lineQuery.exists ? lineQuery.label : fromLine
    }

    /// Waits until the overlay triangles line returns to the given
    /// restored line (a toggled-off overlay leaving the live accounting).
    private func awaitRestoredOverlayTrianglesLine(
        application: XCUIApplication,
        fromLine: String,
        toLine: String
    ) {
        let lineQuery = overlayTrianglesRow(application: application)
        pollForSettlement(
            timeoutDescription: "the toggled-off scene leaving the pushed overlay list"
        ) {
            lineQuery.exists
                && !lineQuery.label.isEmpty
                && lineQuery.label != fromLine
                && lineQuery.label == toLine
        }
    }

    /// One downward scroll drag on an element (the scroll containers'
    /// shared drag shape).
    private func scrollDownElement(_ container: XCUIElement) {
        let containerCenter = container.coordinate(
            withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)
        )
        let dragStart = containerCenter.withOffset(CGVector(dx: 0, dy: 100))
        let dragFinish = containerCenter.withOffset(CGVector(dx: 0, dy: -140))
        dragStart.press(forDuration: 0.05, thenDragTo: dragFinish)
        Thread.sleep(forTimeInterval: Self.pollIntervalSeconds)
    }

    /// The banner's rendered text labels (the composed §7.1 form's slots —
    /// the distance phrase and the maneuver line — plus the banner's own
    /// flattened label, whichever shape the accessibility tree takes).
    private static func bannerLabelTexts(banner: XCUIElement) -> [String] {
        var bannerLabelTexts = banner.staticTexts
            .allElementsBoundByIndex
            .map(\.label)
        if !banner.label.isEmpty {
            bannerLabelTexts.append(banner.label)
        }
        return bannerLabelTexts.filter { !$0.isEmpty }
    }

    /// The recorded fixture's step display texts, decoded from the same
    /// committed fixture the test bundle carries (network-free; the
    /// fixture is the single source of the recorded content).
    private static func recordedStepDisplayTexts(
        fixtureScenarioNamed scenarioName: String
    ) throws -> [String] {
        guard let fixtureURL = Bundle(for: ReferenceApplicationUITests.self)
            .url(forResource: scenarioName, withExtension: "json") else {
            throw NSError(
                domain: "ReferenceApplicationUITests",
                code: 1,
                userInfo: [
                    NSLocalizedDescriptionKey:
                        "the recorded trace fixture '\(scenarioName).json' is not bundled with the test target"
                ]
            )
        }
        let fixtureDocument = try JSONSerialization.jsonObject(with: Data(contentsOf: fixtureURL))
        guard
            let fixtureDocumentDictionary = fixtureDocument as? [String: Any],
            let fixtureRoute = fixtureDocumentDictionary["route"] as? [String: Any],
            let fixtureLegs = fixtureRoute["legs"] as? [[String: Any]]
        else {
            return []
        }
        return fixtureLegs.flatMap { fixtureLeg in
            (fixtureLeg["steps"] as? [[String: Any]] ?? []).compactMap { fixtureStep in
                fixtureStep["instruction_display_text"] as? String
            }
        }
    }

    /// The recorded fixture's final route shape coordinate, decoded from
    /// the same committed fixture the application walks (the arrival's
    /// centred-destination assertion; network-free).
    private static func recordedRouteEndCoordinate(
        fixtureScenarioNamed scenarioName: String
    ) -> (latitude: Double, longitude: Double)? {
        guard let fixtureURL = Bundle(for: ReferenceApplicationUITests.self)
            .url(forResource: scenarioName, withExtension: "json"),
            let fixtureData = try? Data(contentsOf: fixtureURL),
            let fixtureDocument = try? JSONSerialization.jsonObject(with: fixtureData),
            let fixtureDocumentDictionary = fixtureDocument as? [String: Any],
            let fixtureRoute = fixtureDocumentDictionary["route"] as? [String: Any],
            let fixtureLegs = fixtureRoute["legs"] as? [[String: Any]]
        else {
            return nil
        }
        let finalShapeCoordinate = fixtureLegs
            .flatMap { fixtureLeg in
                (fixtureLeg["steps"] as? [[String: Any]] ?? []).flatMap { fixtureStep in
                    fixtureStep["shape"] as? [[String: Any]] ?? []
                }
            }
            .last
        guard
            let finalShapeCoordinate,
            let latitude = finalShapeCoordinate["latitude"] as? Double,
            let longitude = finalShapeCoordinate["longitude"] as? Double
        else {
            return nil
        }
        return (latitude, longitude)
    }

    /// The home sheet's grabber drag (the fold gesture's own target; the
    /// sheet's only gesture target, §4.2). The grabber is not a control —
    /// its identifier exists for the suite's fold act alone.
    private func dragSheetGrabber(
        application: XCUIApplication,
        direction: SheetFoldDirection
    ) {
        let grabber = elementWithIdentifier("sheet-grabber", in: application)
        guard grabber.exists else {
            XCTFail("the home sheet's grabber must render for the fold act")
            return
        }
        let dragDeltaPoints: CGFloat = direction == .up
            ? -Self.sheetFoldDragPoints
            : Self.sheetFoldDragPoints
        let dragStart = grabber.coordinate(
            withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)
        )
        let dragFinish = dragStart.withOffset(
            CGVector(dx: 0, dy: dragDeltaPoints)
        )
        dragStart.press(forDuration: 0.05, thenDragTo: dragFinish)
    }

    /// The sheet's fold directions (the grabber's drag gesture's own
    /// directions).
    private enum SheetFoldDirection {
        /// The sheet grows one fold state.
        case up
        /// The sheet collapses one fold state.
        case down
    }

    /// Scrolls a scrollable container toward its content's end until the
    /// element exists and is hittable; returns when it became visible or
    /// the drag count is exhausted.
    private func scrollToMake(_ element: XCUIElement, visibleIn container: XCUIElement) {
        guard container.exists else { return }
        for _ in 0 ..< Self.maximumScrollAttempts {
            if element.exists, element.isHittable {
                return
            }
            scrollDownElement(container)
        }
    }

    /// Polls a condition closure on the test thread until it holds or the
    /// given timeout elapses (the Espresso suite's `awaitOrTimeout` shape
    /// — the waits are never screenshot waits: every poll is a fresh
    /// accessibility-tree snapshot of the exact values the screen
    /// renders). No failing poll is a test failure itself; the caller's
    /// assertion is the verdict.
    private func pollForSettlement(
        timeoutDescription: String,
        timeoutSeconds: TimeInterval = defaultAssertionTimeoutSeconds,
        file: StaticString = #file,
        line: UInt = #line,
        condition: () -> Bool
    ) {
        let deadline = Date().addingTimeInterval(timeoutSeconds)
        while !condition() {
            if Date() > deadline {
                XCTFail(
                    "\(timeoutDescription) did not settle within \(Int(timeoutSeconds)) seconds",
                    file: file,
                    line: line
                )
                return
            }
            Thread.sleep(forTimeInterval: Self.pollIntervalSeconds)
        }
    }

    /// Whether the element leaves the rendered tree within the timeout
    /// (a `waitForNonExistence`-shaped poll).
    private func pollForNonExistence(
        _ element: XCUIElement,
        within timeoutSeconds: TimeInterval
    ) -> Bool {
        let deadline = Date().addingTimeInterval(timeoutSeconds)
        while element.exists {
            if Date() > deadline {
                return false
            }
            Thread.sleep(forTimeInterval: Self.pollIntervalSeconds)
        }
        return true
    }

    /// Whether the element satisfies the given trait condition within the
    /// settle bound.
    private func pollElement(
        _ element: XCUIElement,
        condition: (XCUIElement) -> Bool
    ) -> Bool {
        let deadline = Date().addingTimeInterval(Self.settleTimeoutSeconds)
        while !condition(element) {
            if Date() > deadline {
                return false
            }
            Thread.sleep(forTimeInterval: Self.pollIntervalSeconds)
        }
        return true
    }

    /// The application subtree's element search: any element type with the
    /// accessibility identifier (the SwiftUI elements' traits vary — the
    /// button, other and static-text queries all descend from this one).
    /// Clears every applied provider credential through the settings
    /// surface's own Clear act, so a case that must be keyless cannot inherit
    /// a credential a previous run opted to remember.
    private func clearAppliedProviderCredentials(
        in application: XCUIApplication
    ) throws {
        let registry = try loadProviderRegistry()
        openProviderSettings(application: application)
        for descriptor in registry.providers
        where descriptor.registrationAvailable.ios {
            let clearControl = elementWithIdentifier(
                "provider-credential-clear-\(descriptor.providerKey)",
                in: application
            )
            if clearControl.exists && clearControl.isEnabled {
                clearControl.tap()
            }
        }
        let closeControl = elementWithIdentifier(
            "provider-settings-close", in: application
        )
        XCTAssertTrue(
            closeControl.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the settings sheet must offer its close control"
        )
        closeControl.tap()
        XCTAssertTrue(
            waitForNonexistence(
                elementWithIdentifier("provider-settings-sheet", in: application)
            ),
            "the settings close control must dismiss the sheet"
        )
    }

    /// Polls until the element reports the selected accessibility trait.
    private func waitForSelected(
        _ element: XCUIElement,
        timeout: TimeInterval = defaultAssertionTimeoutSeconds
    ) -> Bool {
        let selectedPredicate = NSPredicate { _, _ in
            element.exists && element.isSelected
        }
        let selectedExpectation = XCTNSPredicateExpectation(
            predicate: selectedPredicate,
            object: nil
        )
        return XCTWaiter.wait(for: [selectedExpectation], timeout: timeout)
            == .completed
    }

    /// Reads the diagnostics sheet's Directions row, scrolling the sheet when
    /// the blocks above push it below the fold.
    private func readDirectionsDiagnosticsRow(
        application: XCUIApplication
    ) -> String {
        openDiagnosticsSheet(application: application)
        let sheet = diagnosticsSheet(application: application)
        let scrollView = sheet.scrollViews.firstMatch
        let routeOptionsRow = elementWithIdentifier(
            "diagnostics-route-options", in: application
        )
        for _ in 0 ..< Self.maximumScrollAttempts {
            if routeOptionsRow.exists {
                break
            }
            guard scrollView.exists else { break }
            scrollDownElement(scrollView)
        }
        let rowText = routeOptionsRow.exists ? routeOptionsRow.label : ""
        closeDiagnostics(application: application)
        return rowText
    }

    /// Opens the map's layer panel through the collapsed round control.
    private func openMapLayersPanel(in application: XCUIApplication) {
        let layersControl = elementWithIdentifier(
            "map-layers-control", in: application
        )
        XCTAssertTrue(
            layersControl.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the map must offer the collapsed layer control"
        )
        layersControl.tap()
        XCTAssertTrue(
            elementWithIdentifier("map-layers-panel", in: application)
                .waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the layer control must open the layers panel"
        )
    }

    /// Closes the layers panel through its close control.
    private func closeMapLayersPanel(in application: XCUIApplication) {
        let closeControl = elementWithIdentifier(
            "map-layers-close", in: application
        )
        XCTAssertTrue(
            closeControl.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the layers panel must offer its close control"
        )
        closeControl.tap()
        XCTAssertTrue(
            waitForNonexistence(
                elementWithIdentifier("map-layers-panel", in: application)
            ),
            "the close control must dismiss the layers panel"
        )
    }

    /// Polls until a text field's value contains `expectedText`.
    private func waitForTextFieldValueContaining(
        _ expectedText: String,
        textField: XCUIElement,
        timeout: TimeInterval = defaultAssertionTimeoutSeconds
    ) -> Bool {
        let valuePredicate = NSPredicate { _, _ in
            let value = textField.value as? String ?? ""
            return textField.exists && value.contains(expectedText)
        }
        let valueExpectation = XCTNSPredicateExpectation(
            predicate: valuePredicate,
            object: nil
        )
        return XCTWaiter.wait(for: [valueExpectation], timeout: timeout) == .completed
    }

    private func elementWithIdentifier(
        _ identifier: String,
        in application: XCUIApplication
    ) -> XCUIElement {
        application.descendants(matching: .any)
            .matching(identifier: identifier)
            .firstMatch
    }
}