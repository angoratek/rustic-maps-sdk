//! Named constants of the React Native reference application shell (the iOS
//! half of the Android shell's `RefappConstants`): every tunable value lives
//! here with its source; the repository rule forbids magic numbers. The
//! strings mirror the Compose reference application's
//! `ReferenceApplicationConstants` (docs/ReferenceAppUX.md is the design
//! language source of truth) — the v2 sheet-and-drawer language, RN-shaped.

import Foundation

/// The shell's named constants (the Kotlin `RefappConstants` mirror).
enum RefappConstants {
    // --- Readiness and draw loop (docs/ReferenceAppUX.md §6.1) ---
    /// The readiness text ends in an ellipsis while the ladder is loading.
    static let readinessLoadingSuffix = "…"
    static let loadingStatusText = "Maps SDK starting"
    static let readyStatusTemplate = "Maps SDK gpu renderer active (version %@)"
    static let degradedStatusTemplate = "Maps SDK surface active with a failed frame: %@"
    static let failedStatusTemplate = "Maps SDK failed to start: %@"

    // --- Simulated turn-by-turn demo (docs/ReferenceAppUX.md §7) ---
    /// The recorded trace samples advance five simulated seconds apart; the
    /// demo plays the walk back far faster than real time (5 s / this factor
    /// = 250 ms per fix) — the Compose reference application's documented
    /// demo factor.
    static let demoPlaybackTimeAccelerationFactor = 20.0
    /// The planned-route trip's synthetic trace values (the Compose reference
    /// application's `SimulatedTrace.fromRouteData` constants).
    static let simulatedTraceHorizontalAccuracyMeters = 8.0
    static let simulatedTraceSampleIntervalSeconds = 5.0
    /// The accuracy a device fix reports when its source carries none; the
    /// navigation reducer needs a graded fix to advance steps and judge
    /// deviations, so the accepted fix is reported at the recorded trace's own
    /// accuracy value.
    static let deviceNavigationFallbackAccuracyMeters = 8.0
    static let plannedRouteFallbackSpeedMetersPerSecond = 1.4
    /// A route-following trip walks the route's own polyline through the
    /// contract's `RouteSimulator`; the tick cadence below is how often a fix
    /// is asked for (10 fixes a second, smooth at the demo's playback
    /// speeds). The Compose reference application's constant.
    static let routeWalkTickIntervalMilliseconds = 100
    /// The drawer's selectable playback speed multipliers, applied to the
    /// route's own average speed (the `SimulatedTripPace` multiplier). The
    /// user picks one for the session; a route-following trip rebuilds its
    /// walker at the new pace without moving the travelled position.
    static let simulatedTripSpeedMultiplierChoices: [Double] = [1, 2, 4, 8]
    /// The shortest dispatch interval, so a fast multiplier never spins.
    static let minimumDispatchIntervalMilliseconds = 1
    /// The camera's zoom during a simulated trip (§2.4's `navigation-follow-zoom`).
    static let navigationFollowZoomLevel = 17.0
    /// The follow zoom's clamp toward trip end (§2.4's `navigation-follow-zoom-maximum`).
    static let navigationFollowZoomMaximumLevel = 17.5
    /// The 3D follow camera's pitch (§2.4's `navigation-follow-pitch-3d`).
    static let navigationFollowPitch3DDegrees = 45.0
    /// The follow camera's bearing chase time constant (§5.3).
    static let followBearingChaseTimeConstantSeconds = 0.15
    /// The bearing chase stops chasing residual deltas smaller than this.
    static let followBearingChaseSnapDegrees = 0.05
    /// The median course smoother's fix window (§5.3; the Compose constant).
    static let courseSmoothingWindowFixes = 5
    /// The one eased-camera-move duration (§2.4's `compass-renorth-animation`
    /// 300 ms; the follow camera's 2D/3D pitch swing rides the same cadence).
    static let easedCameraMoveDurationSeconds = 0.3
    /// The map camera's zoom bounds (the renderer's clamp, §5.1).
    static let minimumZoomLevel = 0.0
    static let maximumZoomLevel = 24.0
    /// The flat 2D camera plane's pitch (§4.2's plane toggle).
    static let twoDCameraPitchDegrees = 0.0
    /// A north-up camera's bearing (§4.2's compass control).
    static let northUpBearingDegrees = 0.0
    /// The zoom controls' step (§2.4's `map-control-zoom-step`).
    static let mapControlZoomStep = 1.0

    // --- Demo-plan picker (§4.1) ---
    /// The pick attempt while the simulated trip is running (the drawer owns
    /// the dock).
    static let planSelectTripRunningText =
        "the simulated navigation trip is running; exit it before picking a plan"
    /// The pick's trace-load refusal (the verbatim asset load failure).
    static let planSelectTraceUnavailableText = "the demo walk trace could not be loaded"

    // --- Navigation drawer (§7.2 v2 docks) ---
    static let navigationPhaseLabelNavigating = "Navigating"
    static let navigationPhaseLabelOffRoute = "Off route"
    static let navigationPhaseLabelArrived = "Arrived"

    // --- Turn-banner and progress composition (§7.1) ---
    /// The banner's honest no-text line on the `Unavailable` instruction form.
    static let noInstructionText = "No instruction text provided"
    /// The "then" line's prefix (§7.1).
    static let thenLinePrefix = "Then: "
    /// Distance phrase word of the `Immediate` form (§7.1: `Immediate`
    /// renders no number).
    static let distancePhraseImmediateText = "Now"
    /// Roundabout suffix appended to the banner text when an exit index is live.
    static let roundaboutSuffixTemplate = " (exit %d)"
    /// Deviation strip text (§7.5).
    static let deviationTextTemplate =
        "Off route — %@ from the route line; " +
        "guidance is frozen until a graded fix recovers or a reroute applies."

    // --- Distance and duration formatting (the Compose screen's helpers) ---
    static let metersPerKilometer = 1000.0
    static let secondsPerMinute = 60.0

    // --- Spoken guidance (§4.1/§6.3; the speech contract) ---
    /// The speech language tag the demo's guidance text is authored in.
    static let speechLanguageTag = "en-US"

    // --- Statistics throttling (§4.1's offline/live tile line) ---
    /// One statistics event per N composed frames keeps the JS thread
    /// unswamped.
    static let statisticsEventEveryNFrames = 30

    // --- Route preview framing (§4.1's fit over the planned route) ---
    /// Insets the route-preview `fit_bounds` keeps clear of map content, in
    /// physical surface pixels (the Android shell's same named constants).
    static let routePreviewInsetLeftPixels = 30.0
    static let routePreviewInsetTopPixels = 48.0
    static let routePreviewInsetRightPixels = 30.0
    /// The route-preview fit's zoom clamp (the SDK's own maximum; Compose
    /// constant).
    static let maximumFitBoundsZoomLevel = 24.0

    // --- Offline maps (§4.1's card over the registered tile cache) ---
    /// The tile cache's file inside the app's caches directory; the SDK's
    /// `setProviderCredentialWithTileCache` opens (or creates) the SQLite
    /// response cache there (the Compose reference application's file).
    static let tileCacheDatabaseFileName = "reference-tile-cache.sqlite"
    /// The cache's named 16 MiB byte budget (the Compose application's demo
    /// budget).
    static let tileCacheAbsoluteMaximumBytes: UInt64 = 16_777_216

    // --- The offline archive route (§2.5's offline routing copy) ---
    /// The committed compiled routing archive the route control reads from
    /// the application bundle.
    static let offlineRoutingArchiveResourceName = "routing_grid"
    static let offlineRoutingArchiveResourceExtension = "rmog"
    // The offline archive's demo request: the routing-grid fixture's straight
    // oneway street (the wasm seam's test pair). The contract call takes the
    // archive profile's defaults (automobile, fastest, metric).
    static let offlineRouteStartLatitudeDegrees = 0.001
    static let offlineRouteStartLongitudeDegrees = 0.0
    static let offlineRouteEndLatitudeDegrees = 0.001
    static let offlineRouteEndLongitudeDegrees = 0.002
    /// The route summary's title for an offline archive route (§2.5).
    static let offlineRouteTitleText = "Offline demo route"

    // --- Search and route planning (§4.1's plan flow) ---
    /// The demo search shows at most this many provider result rows.
    static let searchResultRowLimit = 5
    /// The trip source a search-planned route replays (its own shape, in order).
    static let plannedRouteTraceName = "planned_route_shape"
    /// The route summary card's title fallback when no destination name crossed.
    static let plannedRouteFallbackName = "Planned route"
    /// The verbatim search attempt before the SDK objects were built.
    static let searchBeforeSdkStateText = "the SDK has not started"
    /// The refusal when a second search overlaps the running one.
    static let searchInFlightText = "a search is already in flight"
    /// The route plan's unresolved-origin refusal (the artifact's demo
    /// position is the default).
    static let planOriginUnresolvedText = "the plan origin was not resolved"
    /// The explicit Plan route action's destination-side refusal.
    static let planDestinationUnresolvedText = "the plan destination was not resolved"
    /// The refusal when a second route plan overlaps the running one.
    static let planAlreadyRunningText = "a route plan is already in flight"
    /// The refusal when the Directions choice names no contract enum value.
    static let planUnknownDirectionsChoiceText =
        "the plan's directions choice is not recognised"
    /// The keyless plan's honest refusal when the artifact records no route
    /// for the ordered pair (§2.5 `plan.no_recorded_route_hint`): the slots
    /// are the origin and destination names.
    static let noRecordedRouteHintFormat =
        "No recorded demo route from %@ to %@; apply a provider key for live routing."
    /// The plan flow's half sheet height fraction (§2.4's fit bottom inset
    /// input).
    static let sheetHalfHeightFraction = 0.5

    // --- The shared behavior model's consumer copy
    // (docs/ReferenceAppUX.md; the same values theme.ts
    // carries) ---
    static let planOriginLabelText = "Origin"
    static let planDestinationLabelText = "Destination"
    static let planSearchActionText = "Search"
    static let planMyLocationActionText = "My location"
    static let planSwapActionText = "Swap origin and destination"
    static let planMissingOriginText = "Enter an origin to plan the route."
    static let planMissingDestinationText = "Enter a destination to plan the route."
    static let planMissingBothText =
        "Enter an origin and a destination to plan the route."
    static let planMyLocationUnavailableFormat = "My location is unavailable: %@"
    static let planTripSourceDemoPlanFormat = "Selected demo plan: %@"
    static let settingsProviderRowFormat = "%@ · %@"
    static let settingsProviderStateReadyText = "Ready"
    static let settingsProviderStateNeedsCredentialFormat = "Needs %@"
    static let settingsProviderStateNotConfiguredText = "Not configured"
    static let settingsRoutingDemoEndpointFormat = "Demo server: %@"
    static let settingsRoutingUnreachableNextStepText =
        "The demo server did not answer. Retry, or enter your own deployment address."
    static let diagnosticsShowDebugEventsText = "Show debug events"
    static let diagnosticsHideDebugEventsText = "Hide debug events"
    static let diagnosticsClearActionText = "Clear"
    static let diagnosticsNoEventsText = "No SDK events recorded yet."
    static let diagnosticsStreamScopeNoteText =
        "Debug detail stays on until the app restarts."

    // --- Cache state (§4.1's two honest lines) ---
    static let cacheRegisteredTemplate = "Cache registered: %@ · %lu bytes budget"
    static let cacheNotRegisteredText = "Cache not registered"

    // --- Runtime provider credentials
    // (docs/Providers.md (Credentials through the coverage matrix)) ---
    /// The provider key the registration dispatch serves today.
    static let mapquestProviderKey = "mapquest"
    /// The refusal before the SDK objects were built.
    static let providerCredentialSdkNotStartedText = "the SDK has not started"
    /// The refusal when a provider has no registration arm yet.
    static let providerCredentialUnsupportedTemplate = "provider '%@' is not supported"
    /// The refusal of an empty credential at the native boundary.
    static let providerCredentialEmptyText = "the provider credential is empty"
    /// The refusal when the app's caches directory is unavailable.
    static let providerCredentialCachesDirectoryUnavailableText =
        "the app caches directory is unavailable"

    // --- The opt-in Keychain store (§5's iOS secure store) ---
    /// The `kSecClassGenericPassword` service the credential items live under.
    static let providerCredentialKeychainService =
        "com.angoratek.rustic-maps.refapp.provider-credentials"

    // --- Demo scenes (§4.1's chip carousel; the artifact's scene keys) ---
    /// The overlay scenes' keys in the artifact's presentation order.
    static let overlaySceneKeys = ["route", "polygon", "markers", "traffic"]

    // ------------------------------------------------------------------
    // The launcher-argument demo automation (the iOS-idiomatic UI-test
    // seam; the Android shell drives the same user-path methods through
    // instrumented taps — `simctl` cannot inject touches).
    // ------------------------------------------------------------------
    /// The launcher argument naming the demo plan to apply once the SDK
    /// started (the trace asset name, e.g. `sacramento_clean_walk.json`).
    static let automationPlanArgumentName = "RefappAutomationPlan"
    /// The launcher argument naming the demo-scene keys (comma separated)
    /// to toggle after the plan applied.
    static let automationScenesArgumentName = "RefappAutomationScenes"
    /// The launcher argument carrying the seconds delay from trip-ready to
    /// the simulated trip's start.
    static let automationStartSecondsArgumentName = "RefappAutomationStartSeconds"
    /// The launcher argument carrying the seconds delay from trip start to
    /// the trip's exit.
    static let automationExitSecondsArgumentName = "RefappAutomationExitSeconds"
    /// Automation delay after the SDK start lands before the demo plan
    /// applies.
    static let automationPlanApplyDelaySeconds = 2.0
    /// Automation delay after the plan applied before the scene chips toggle.
    static let automationScenesApplyDelaySeconds = 1.0
}

/// The archetypal framed camera's field of view (§2.4's shared token).
enum ReferenceFieldOfView {
    static let degrees: Double = 60.0
}

/// Structural math constants shared across the shell's geometry helpers.
enum GeometryConstants {
    /// The full circle's degree sweep.
    static let fullCircleDegrees: Double = 360.0
    /// Structural constant: `degrees * degreesToRadians = radians`.
    static let degreesToRadians: Double = Double.pi / 180.0
    /// Structural constant: `nanoseconds / nanosPerSecond = seconds`.
    static let nanosPerSecond: Double = 1_000_000_000.0
    /// Structural constant of the replay's seconds→milliseconds conversions.
    static let millisecondsPerSecond: Double = 1000.0
}

/// Formats the metric distance for the readiness drawer's lines (the Compose
/// helper's decimal point style).
func formatDistanceMeters(_ distanceMeters: Double) -> String {
    if distanceMeters < RefappConstants.metersPerKilometer {
        return String(format: "%.0f m", distanceMeters)
    }
    return String(format: "%.2f km", distanceMeters / RefappConstants.metersPerKilometer)
}

/// Formats the duration for the drawer's ETA-distance line (Compose helper).
func formatDurationSeconds(_ durationSeconds: Double) -> String {
    let wholeMinutes = floor(durationSeconds / RefappConstants.secondsPerMinute)
    let remainderSeconds = durationSeconds - wholeMinutes * RefappConstants.secondsPerMinute
    if wholeMinutes > 0.0 {
        return String(format: "%.0f min %.0f s", wholeMinutes, remainderSeconds)
    }
    return String(format: "%.0f s", durationSeconds)
}