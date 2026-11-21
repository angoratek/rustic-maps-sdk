//! The React Native reference application's simulated-trace layer on iOS,
//! mirroring the Android shell's `RefappTraceReplay.kt` (which mirrors the
//! Compose reference application's `SimulatedTraceLoader`/`SimulatedTripDriver`).
//! Field names and file names mirror the fixtures' snake_case keys, so the
//! same committed trace set drives the demo on every platform.

import Foundation
import RusticMapsSdk

/// One recorded trace sample (the fixture's `samples` entry).
struct RefappTraceSample {
    let latitudeDegrees: Double
    let longitudeDegrees: Double
    let horizontalAccuracyMeters: Double
    let timestampOffsetSeconds: Double
}

/// The recorded synthetic walk: its route record (the frozen `Route` the
/// trip start consumes), its samples, and its recorded sampling interval.
struct RefappTrace {
    let traceName: String
    let route: Route
    let samples: [RefappTraceSample]
    let sampleIntervalSeconds: Double
    /// Whether the trip walks the route's own polyline through the contract's
    /// `RouteSimulator` (a search-planned route) rather than replaying the
    /// recorded `samples` verbatim (the demo scenarios). A route-following
    /// trace carries no samples: the walker interpolates between the route's
    /// shape vertices from elapsed simulated time.
    let followsRouteGeometry: Bool

    init(
        traceName: String,
        route: Route,
        samples: [RefappTraceSample],
        sampleIntervalSeconds: Double,
        followsRouteGeometry: Bool = false
    ) {
        self.traceName = traceName
        self.route = route
        self.samples = samples
        self.sampleIntervalSeconds = sampleIntervalSeconds
        self.followsRouteGeometry = followsRouteGeometry
    }

    /// Builds the simulated-trip source of a planned route (the
    /// search-planned trip, mirroring the Compose reference application's
    /// `SimulatedTrace.fromRouteData`): the trip follows the route's own
    /// polyline through the contract's `RouteSimulator`, so the position
    /// moves continuously between shape vertices and the course comes from
    /// the segment bearing. No vertex samples are precomputed — the walker
    /// owns the interpolation.
    static func fromRouteData(route: Route, traceName: String) -> RefappTrace {
        RefappTrace(
            traceName: traceName,
            route: route,
            samples: [],
            sampleIntervalSeconds: RefappConstants.simulatedTraceSampleIntervalSeconds,
            followsRouteGeometry: true
        )
    }
}

/// Loads the recorded synthetic walk trace bundled as an application
/// resource (the resources build phase's copied fixture files, the same
/// single committed sources the Android copy tasks bundle).
enum RefappTraceLoader {
    static func load(traceAssetName: String) throws -> RefappTrace {
        guard let assetURL = Bundle.main.url(
            forResource: traceAssetName,
            withExtension: nil
        ) else {
            throw RefappArtifactError(
                detailText: "the recorded trace asset '\(traceAssetName)' was not found in the app bundle"
            )
        }
        let traceData = try Data(contentsOf: assetURL)
        guard let traceObject = try JSONSerialization.jsonObject(
            with: traceData, options: .fragmentsAllowed
        ) as? [String: Any] else {
            throw RefappArtifactError(
                detailText: "the recorded trace '\(traceAssetName)' is not a JSON object"
            )
        }
        guard let metadataObject = traceObject["metadata"] as? [String: Any],
              let traceName = metadataObject["scenario_name"] as? String,
              let recordedSampleIntervalSeconds = metadataObject["sample_interval_seconds"] as? Double,
              let routeObject = traceObject["route"] as? [String: Any],
              let sampleObjects = traceObject["samples"] as? [[String: Any]]
        else {
            throw RefappArtifactError(
                detailText: "the recorded trace '\(traceAssetName)' does not carry the recorded shape"
            )
        }
        let samples: [RefappTraceSample] = sampleObjects.compactMap { sampleObject in
            guard let latitudeDegrees = sampleObject["latitude"] as? Double,
                  let longitudeDegrees = sampleObject["longitude"] as? Double,
                  let horizontalAccuracyMeters = sampleObject["horizontal_accuracy_meters"] as? Double,
                  let timestampOffsetSeconds = sampleObject["timestamp_offset_seconds"] as? Double
            else {
                return nil
            }
            return RefappTraceSample(
                latitudeDegrees: latitudeDegrees,
                longitudeDegrees: longitudeDegrees,
                horizontalAccuracyMeters: horizontalAccuracyMeters,
                timestampOffsetSeconds: timestampOffsetSeconds
            )
        }
        return RefappTrace(
            traceName: traceName,
            route: try decodeRoute(routeObject),
            samples: samples,
            sampleIntervalSeconds: recordedSampleIntervalSeconds
        )
    }

    /// Decodes the trace's route record into the frozen `Route` the
    /// controller's `startNavigation` consumes — the trace fixtures serialize the
    /// maneuver type in the SDK's single stable label spelling, so the label
    /// passes through verbatim (an unknown label fails the SDK start loudly —
    /// never a guess).
    static func decodeRoute(_ routeObject: [String: Any]) throws -> Route {
        guard let distanceMeters = routeObject["distance_meters"] as? Double,
              let durationSeconds = routeObject["duration_seconds"] as? Double,
              let boundingBoxObject = routeObject["bounding_box"] as? [String: Any],
              let southWestObject = boundingBoxObject["south_west"] as? [String: Any],
              let northEastObject = boundingBoxObject["north_east"] as? [String: Any],
              let legObjects = routeObject["legs"] as? [[String: Any]]
        else {
            throw RefappArtifactError(
                detailText: "the trace's route record does not carry the recorded route shape"
            )
        }
        let legs: [RouteLeg] = try legObjects.map { legObject in
            guard let legDistanceMeters = legObject["distance_meters"] as? Double,
                  let legDurationSeconds = legObject["duration_seconds"] as? Double,
                  let stepObjects = legObject["steps"] as? [[String: Any]]
            else {
                throw RefappArtifactError(
                    detailText: "the trace's route leg does not carry the recorded steps"
                )
            }
            let steps: [RouteStep] = try stepObjects.map { stepObject in
                guard let stepDistanceMeters = stepObject["distance_meters"] as? Double,
                      let stepDurationSeconds = stepObject["duration_seconds"] as? Double,
                      let maneuverType = stepObject["maneuver_type"] as? String,
                      let instructionDisplayText = stepObject["instruction_display_text"] as? String,
                      let instructionSpeechText = stepObject["instruction_speech_text"] as? String,
                      let shapeObjects = stepObject["shape"] as? [[String: Any]]
                else {
                    throw RefappArtifactError(
                        detailText: "the trace's route step does not carry the recorded shape"
                    )
                }
                let shape: [GeographicCoordinate] = try shapeObjects.map { coordinateObject in
                    guard let latitudeDegrees = coordinateObject["latitude"] as? Double,
                          let longitudeDegrees = coordinateObject["longitude"] as? Double
                    else {
                        throw RefappArtifactError(
                            detailText: "the trace's route step shape coordinate is not a coordinate record"
                        )
                    }
                    return GeographicCoordinate(
                        latitude: latitudeDegrees,
                        longitude: longitudeDegrees
                    )
                }
                return RouteStep(
                    distanceMeters: stepDistanceMeters,
                    durationSeconds: stepDurationSeconds,
                    maneuverType: maneuverType,
                    instructionDisplayText: instructionDisplayText,
                    instructionSpeechText: instructionSpeechText,
                    shape: shape
                )
            }
            return RouteLeg(
                distanceMeters: legDistanceMeters,
                durationSeconds: legDurationSeconds,
                steps: steps
            )
        }
        return Route(
            distanceMeters: distanceMeters,
            durationSeconds: durationSeconds,
            boundingBox: BoundingBox(
                southWest: try decodeCoordinate(southWestObject),
                northEast: try decodeCoordinate(northEastObject)
            ),
            geometry: try (routeObject["geometry"] as? [[String: Any]] ?? []).map {
                try decodeCoordinate($0)
            },
            legs: legs
        )
    }

    private static func decodeCoordinate(_ coordinateObject: [String: Any]) throws -> GeographicCoordinate {
        guard let latitudeDegrees = coordinateObject["latitude"] as? Double,
              let longitudeDegrees = coordinateObject["longitude"] as? Double
        else {
            throw RefappArtifactError(
                detailText: "the trace's route record bound is not a coordinate record"
            )
        }
        return GeographicCoordinate(latitude: latitudeDegrees, longitude: longitudeDegrees)
    }

    /// One artifact/trace decode failure carrying the verbatim detail (§6.3).
    struct RefappArtifactError: LocalizedError, Swift.Error {
        let detailText: String
        var errorDescription: String? { detailText }
    }
}

/// The SDK listener seam's one collecting sink (the Compose app's
/// `CollectingNavigationStateListener` pattern): topics deliver synchronously
/// on the thread that drives the controller call (the demo driver's
/// main-run-loop ticks), and each payload is handed straight to the callback
/// the reference controller registered (which emits it to the React side).
/// Listeners never re-enter the controller.
final class RefappCollectingListener: NavigationListener, @unchecked Sendable {
    private let onInstruction: (NavigationInstruction) -> Void
    private let onDeviationStatusChange: (DeviationStatusChange) -> Void
    private let onPhaseChange: (NavigationPhaseChange) -> Void
    private let onSpeechInstructionDelivery: (SpeechInstruction) -> Void

    /// Latest delivered payloads (the read-only queries only seed state).
    private(set) var latestInstruction: NavigationInstruction?
    private(set) var latestDeviationStatusChange: DeviationStatusChange?
    private(set) var latestPhaseChange: NavigationPhaseChange?
    private(set) var latestSpeechInstruction: SpeechInstruction?

    init(
        onInstruction: @escaping (NavigationInstruction) -> Void,
        onDeviationStatusChange: @escaping (DeviationStatusChange) -> Void,
        onPhaseChange: @escaping (NavigationPhaseChange) -> Void,
        onSpeechInstructionDelivery: @escaping (SpeechInstruction) -> Void
    ) {
        self.onInstruction = onInstruction
        self.onDeviationStatusChange = onDeviationStatusChange
        self.onPhaseChange = onPhaseChange
        self.onSpeechInstructionDelivery = onSpeechInstructionDelivery
    }

    func onNavigationInstruction(instruction: NavigationInstruction) {
        latestInstruction = instruction
        onInstruction(instruction)
    }

    func onDeviationStatusChange(deviationStatusChange: DeviationStatusChange) {
        latestDeviationStatusChange = deviationStatusChange
        onDeviationStatusChange(deviationStatusChange)
    }

    func onNavigationPhaseChange(navigationPhaseChange: NavigationPhaseChange) {
        latestPhaseChange = navigationPhaseChange
        onPhaseChange(navigationPhaseChange)
    }

    func onSpeechInstruction(speechInstruction: SpeechInstruction) {
        // The speech topic's delivery crosses to the React side verbatim (the
        // contract's platform half, RN-shaped): the shell's speech
        // consumer — the jest-testable JS module — owns the dedup, pacing and
        // toggle decisions, and drives the app-owned speech engine through
        // the bridge module. This side records the delivery and never voices
        // anything itself.
        latestSpeechInstruction = speechInstruction
        onSpeechInstructionDelivery(speechInstruction)
    }

    /// Forgets every delivered payload (the Compose application's
    /// re-subscription replay rule): a new trip must not read the previous
    /// trip's arrived phase or instruction as its own.
    func reset() {
        latestInstruction = nil
        latestDeviationStatusChange = nil
        latestPhaseChange = nil
        latestSpeechInstruction = nil
    }
}

/// One processed simulated fix (the Compose reference application's
/// `SimulatedProcessedFix` mirror): the reducer's snapped coordinate plus the
/// fix's own course-over-ground and horizontal accuracy report — the fields
/// the renderer-owned location indicator's state carries (§4.2).
struct RefappProcessedFix {
    let snappedCoordinate: GeographicCoordinate?
    let courseHeadingDegrees: Double?
    let horizontalAccuracyMeters: Double?
}

/// Drives the simulated turn-by-turn demo through `Navigator.updateLocation`,
/// mirroring the Compose app's `SimulatedTripDriver`. Two playback sources
/// share the driver:
/// - a search-planned route is walked with the contract's `RouteSimulator`:
///   one simulator per trip (and one rebuilt at each pace change), its fixes
///   asked for by **elapsed simulated time**, so the position moves
///   continuously between the route's shape vertices and the course is the
///   segment bearing;
/// - a recorded demo trace replays its own samples at the documented demo
///   acceleration (`RefappConstants.demoPlaybackTimeAccelerationFactor`
///   times real time, divided by the selected speed multiplier).
///
/// Playback can be paused and resumed at the current fix and runs at the
/// selected speed multiplier; an arrival holds `hasArrived` so the
/// presentation outlives the playback and the trip never snaps back. The
/// driver runs on the main thread, like every other seam this shell drives.
/// The clean-walk trace never deviates, so no reroute policy applies here.
final class RefappTraceDriver: @unchecked Sendable {
    private let navigationController: Navigator
    private let listener: RefappCollectingListener
    private let onProcessedFix: (RefappProcessedFix) -> Void
    private let onArrival: () -> Void

    /// Whether a simulated trip is presented and driven (a paused trip stays
    /// active).
    private(set) var isTripActive = false

    /// Whether the active trip's playback is paused at its current fix.
    private(set) var isTripPaused = false

    /// Whether the active trip reached the route's destination. The arrival
    /// presentation outlives the playback (the trip does not snap back), and
    /// is reset when the next trip starts.
    private(set) var hasArrived = false

    /// The playback speed multiplier the trip advances at — one of
    /// `RefappConstants.simulatedTripSpeedMultiplierChoices`. The choice
    /// persists across trips (the user picked it for the session).
    private(set) var speedMultiplier =
        RefappConstants.simulatedTripSpeedMultiplierChoices[0]

    /// The controller's latest read-only trip state, refreshed with every fix.
    private(set) var latestTripState: TripNavigationState?

    /// The verbatim start failure, shown when the trip could not start.
    private(set) var startFailure: SdkError?

    /// The verbatim failure of a mid-trip fix delivery; stops the replay.
    private(set) var tripDeliveryFailure: Error?

    private var subscriptions: [Subscription] = []
    private var activeTrace: RefappTrace?
    private var sampleIndex = 0
    private var dispatchTimer: Timer?

    /// The route walker of a route-following trip, or nil for a recorded trace.
    private var routeSimulator: RouteSimulator?

    /// The route-following walk's clock, in simulated seconds since the trip
    /// start.
    private var elapsedSimulatedSeconds = 0.0

    /// The distance the active trip has travelled, in meters (pace-change
    /// anchor).
    private(set) var travelledDistanceMeters = 0.0

    /// The route-following walk's total length, in meters; 0 when not seekable.
    private(set) var seekableDistanceMeters = 0.0

    /// Whether the presented trip walks the route's own geometry (the seek).
    var isSeekable: Bool { routeSimulator != nil }

    /// Whether the walk's start fix (elapsed zero) has already been delivered.
    private var hasDeliveredRouteWalkStartFix = false

    init(
        navigationController: Navigator,
        listener: RefappCollectingListener,
        onProcessedFix: @escaping (RefappProcessedFix) -> Void,
        onArrival: @escaping () -> Void
    ) {
        self.navigationController = navigationController
        self.listener = listener
        self.onProcessedFix = onProcessedFix
        self.onArrival = onArrival
    }

    /// Starts (or replaces) the simulated trip: subscribes the topics first
    /// so the initial instruction dispatch of `startNavigation` is received,
    /// builds the route walker when the trip follows the route's own
    /// geometry, then drives the fixes. Throws nothing: start failures
    /// surface verbatim through `startFailure` (§6.3).
    func startTrip(trace: RefappTrace) {
        guard Thread.isMainThread else {
            fatalError("RefappTraceDriver.startTrip runs on the main thread only")
        }
        stopTrip()
        do {
            try startGuidance(route: trace.route)
            if trace.followsRouteGeometry {
                // One walker per trip, over the route the visitor planned.
                routeSimulator = try buildRouteSimulator(
                    route: trace.route,
                    multiplier: speedMultiplier
                )
            }
        } catch let sdkError as SdkError {
            cancelSubscriptions()
            // The route walker is built after `startNavigation` succeeded, so
            // a refused pace/route must end the newly started trip too rather
            // than leave a half-started one (a no-op when the start itself
            // failed).
            navigationController.stopNavigation()
            routeSimulator = nil
            startFailure = sdkError
            return
        } catch {
            cancelSubscriptions()
            navigationController.stopNavigation()
            routeSimulator = nil
            startFailure = .InvalidNavigationInput(detail: String(describing: error))
            return
        }
        isTripActive = true
        isTripPaused = false
        hasArrived = false
        startFailure = nil
        tripDeliveryFailure = nil
        activeTrace = trace
        sampleIndex = 0
        seekableDistanceMeters = routeSimulator?.totalDistanceMeters() ?? 0
        elapsedSimulatedSeconds = 0.0
        travelledDistanceMeters = 0.0
        hasDeliveredRouteWalkStartFix = false
        scheduleNextDispatch()
    }

    /// Starts guidance on `route` from the device's own fixes: the same topic
    /// subscriptions and `startNavigation` call as a simulated trip, without a
    /// playback loop. `deliverDeviceFix` feeds each accepted fix through the
    /// same `deliverFix` path, so the banner, the ETA line and the follow
    /// camera all read the controller's own stream. Throws nothing: a start
    /// failure surfaces verbatim through `startFailure`.
    func startDeviceTrip(route: Route) {
        guard Thread.isMainThread else {
            fatalError("RefappTraceDriver.startDeviceTrip runs on the main thread only")
        }
        stopTrip()
        do {
            try startGuidance(route: route)
        } catch let sdkError as SdkError {
            cancelSubscriptions()
            navigationController.stopNavigation()
            startFailure = sdkError
            return
        } catch {
            cancelSubscriptions()
            navigationController.stopNavigation()
            startFailure = .InvalidNavigationInput(detail: String(describing: error))
            return
        }
        isTripActive = true
        isTripPaused = false
        hasArrived = false
        startFailure = nil
        tripDeliveryFailure = nil
        activeTrace = nil
        sampleIndex = 0
        seekableDistanceMeters = 0
        elapsedSimulatedSeconds = 0.0
        travelledDistanceMeters = 0.0
        hasDeliveredRouteWalkStartFix = false
    }

    /// Subscribes the four navigation topics before starting the guidance
    /// session, so the session's initial instruction dispatch is received,
    /// then starts the trip at the app's unit system and policies. A simulated
    /// trace and the device's fixes share this start path.
    private func startGuidance(route: Route) throws {
        let stepAdvancePolicy = defaultStepAdvancePolicy()
        let deviationPolicy = defaultDeviationPolicy()
        subscriptions = [
            navigationController.subscribeToNavigationInstructions(listener: listener),
            navigationController.subscribeToDeviationStatus(listener: listener),
            navigationController.subscribeToPhaseChanges(listener: listener),
            navigationController.subscribeToSpeechInstructions(listener: listener),
        ]
        try navigationController.startNavigation(
            route: route,
            unitSystem: .metric,
            stepAdvancePolicy: stepAdvancePolicy,
            deviationPolicy: deviationPolicy
        )
    }

    /// One accepted device fix of a device-driven trip, delivered through the
    /// same `deliverFix` path a simulated fix uses: the controller reports the
    /// snapped state the banner and ETA read, and the session's fix callback
    /// drives the follow camera and the location indicator. An arrival ends
    /// the active flag the way the playback loop does, leaving the arrival
    /// presentation in place.
    func deliverDeviceFix(
        coordinate: GeographicCoordinate,
        courseHeadingDegrees: Double?,
        horizontalAccuracyMeters: Double
    ) {
        // A fix that arrives after the mode ended must not reach the
        // controller; after an arrival the source keeps reporting, so the
        // indicator can follow the device while the destination presentation
        // stays.
        guard isTripActive || hasArrived else { return }
        deliverFix(
            coordinate: coordinate,
            courseHeadingDegrees: courseHeadingDegrees,
            horizontalAccuracyMeters: horizontalAccuracyMeters,
            walkerDeclaredArrival: false
        )
        if hasArrived {
            isTripActive = false
        }
    }

    /// Pauses the active trip at its current fix (the presentation holds).
    func pauseTrip() {
        if isTripActive {
            isTripPaused = true
        }
    }

    /// Resumes a paused active trip from its current fix.
    func resumeTrip() {
        isTripPaused = false
    }

    /// Selects the playback speed multiplier — one of
    /// `RefappConstants.simulatedTripSpeedMultiplierChoices`. A
    /// route-following trip rebuilds its walker at the new pace and re-anchors
    /// the clock at the distance already covered, so a mid-trip pace change
    /// never teleports the position. A recorded trace's replay cadence follows
    /// the choice. A refusal stops the playback honestly (§6.3).
    func selectSpeedMultiplier(_ multiplier: Double) {
        let isOfferedChoice = RefappConstants.simulatedTripSpeedMultiplierChoices
            .contains(multiplier)
        guard isOfferedChoice, multiplier != speedMultiplier else { return }
        if let trace = activeTrace, trace.followsRouteGeometry, isTripActive {
            do {
                let rebuiltSimulator = try buildRouteSimulator(
                    route: trace.route,
                    multiplier: multiplier
                )
                // Keep the travelled distance continuous: the rebuilt walker's
                // clock restarts at the distance the trip already covered, so
                // a mid-trip pace change never teleports the position.
                let currentFix = rebuiltSimulator.fixAtTravelledMeters(
                    travelledMeters: travelledDistanceMeters
                )
                routeSimulator = rebuiltSimulator
                elapsedSimulatedSeconds = currentFix.elapsedSeconds
            } catch let sdkError as SdkError {
                tripDeliveryFailure = sdkError
                isTripActive = false
                return
            } catch {
                tripDeliveryFailure = error
                isTripActive = false
                return
            }
        }
        speedMultiplier = multiplier
        if isTripActive && !isTripPaused && !hasArrived {
            // Re-arm the pending tick at the new cadence (the route tick stays
            // fixed; the recorded trace's interval divides by the multiplier).
            scheduleNextDispatch()
        }
    }

    /// Moves the presented trip to `targetMeters` along the route's own
    /// geometry (the seek). The walker supplies the fix, so the camera, the
    /// indicator, the ETA line and the arrival all read exactly what a normal
    /// fix at that distance produces.
    ///
    /// A paused trip stays paused; a running trip continues from the new
    /// position. Seeking backwards out of the arrival restarts the navigation
    /// session at the seek position, because the contract's reducer ignores
    /// fixes once it reports `Arrived`.
    func seekToTravelledMeters(_ targetMeters: Double) {
        guard let trace = activeTrace, let routeSimulator else { return }
        guard trace.followsRouteGeometry else { return }
        let targetFix = routeSimulator.fixAtTravelledMeters(
            travelledMeters: min(
                max(targetMeters, 0),
                routeSimulator.totalDistanceMeters()
            )
        )
        if hasArrived && !targetFix.isArrived {
            do {
                listener.reset()
                try navigationController.startNavigation(
                    route: trace.route,
                    unitSystem: .metric,
                    stepAdvancePolicy: defaultStepAdvancePolicy(),
                    deviationPolicy: defaultDeviationPolicy()
                )
            } catch {
                isTripActive = false
                tripDeliveryFailure = error
                return
            }
        }
        hasArrived = false
        isTripActive = true
        elapsedSimulatedSeconds = targetFix.elapsedSeconds
        travelledDistanceMeters = targetFix.travelledMeters
        hasDeliveredRouteWalkStartFix = true
        deliverFix(
            coordinate: targetFix.coordinate,
            courseHeadingDegrees: targetFix.courseHeadingDegrees,
            horizontalAccuracyMeters:
                RefappConstants.simulatedTraceHorizontalAccuracyMeters,
            walkerDeclaredArrival: targetFix.isArrived
        )
        if tripDeliveryFailure != nil {
            isTripActive = false
            return
        }
        if hasArrived {
            isTripActive = false
            onArrival()
            return
        }
        scheduleNextDispatch()
    }

    /// Stops the replay, cancels the subscriptions; the trip state stays
    /// as-is.
    func stopTrip() {
        isTripActive = false
        isTripPaused = false
        hasArrived = false
        dispatchTimer?.invalidate()
        dispatchTimer = nil
        cancelSubscriptions()
        listener.reset()
        activeTrace = nil
        routeSimulator = nil
        elapsedSimulatedSeconds = 0.0
        travelledDistanceMeters = 0.0
        seekableDistanceMeters = 0.0
        hasDeliveredRouteWalkStartFix = false
        sampleIndex = 0
    }

    /// Advances the playback one tick: a route-following trip asks its
    /// `RouteSimulator` for the fix at the advanced simulated clock; a
    /// recorded trace delivers its next sample. A paused trip keeps its
    /// current fix; an arrival ends the loop with `hasArrived` set (the
    /// arrival presentation outlives the playback).
    private func advancePlayback() {
        guard runOnMainIfSafe() else { return }
        guard isTripActive, let trace = activeTrace else { return }
        if isTripPaused || hasArrived {
            scheduleNextDispatch()
            return
        }
        if trace.followsRouteGeometry {
            guard let routeSimulator else {
                // A route-following trip always builds its walker before the
                // loop starts; a missing one ends the playback rather than
                // leaving the trip stuck as "active".
                isTripActive = false
                return
            }
            if hasDeliveredRouteWalkStartFix {
                elapsedSimulatedSeconds += routeWalkTickSeconds
                    * RefappConstants.demoPlaybackTimeAccelerationFactor
            }
            hasDeliveredRouteWalkStartFix = true
            let fix = routeSimulator.fixAtElapsedSeconds(
                elapsedSeconds: elapsedSimulatedSeconds
            )
            travelledDistanceMeters = fix.travelledMeters
            deliverFix(
                coordinate: fix.coordinate,
                courseHeadingDegrees: fix.courseHeadingDegrees,
                horizontalAccuracyMeters:
                    RefappConstants.simulatedTraceHorizontalAccuracyMeters,
                walkerDeclaredArrival: fix.isArrived
            )
            if tripDeliveryFailure != nil {
                isTripActive = false
                return
            }
            if hasArrived {
                isTripActive = false
                onArrival()
                return
            }
        } else {
            guard sampleIndex < trace.samples.count else {
                // The samples ran out without arrival: the demo driver stops
                // replaying but keeps the trip's state (the documented exit).
                isTripActive = false
                return
            }
            deliverSample(trace: trace)
            if tripDeliveryFailure != nil {
                isTripActive = false
                return
            }
            if hasArrived {
                isTripActive = false
                onArrival()
                return
            }
        }
        scheduleNextDispatch()
    }

    /// Arms the pending playback tick at the interval of the active source.
    private func scheduleNextDispatch() {
        dispatchTimer?.invalidate()
        dispatchTimer = nil
        guard isTripActive else { return }
        dispatchTimer = Timer.scheduledTimer(
            withTimeInterval: Double(dispatchIntervalMillis())
                / GeometryConstants.millisecondsPerSecond,
            repeats: false
        ) { [weak self] _ in
            self?.advancePlayback()
        }
    }

    /// One recorded sample of the demo replay, with its planar course.
    private func deliverSample(trace: RefappTrace) {
        let sample = trace.samples[sampleIndex]
        let previousSample = sampleIndex > 0 ? trace.samples[sampleIndex - 1] : nil
        let sampleCourseHeadingDegrees = previousSample.map { headingDegreesFrom($0, sample) }
        deliverFix(
            coordinate: GeographicCoordinate(
                latitude: sample.latitudeDegrees,
                longitude: sample.longitudeDegrees
            ),
            courseHeadingDegrees: sampleCourseHeadingDegrees,
            horizontalAccuracyMeters: sample.horizontalAccuracyMeters,
            walkerDeclaredArrival: false
        )
        sampleIndex += 1
    }

    /// Feeds one fix to the controller and reports its processed state. A
    /// failed fix delivery must stop the replay honestly (§6.3), not crash the
    /// demo; an arrival sets `hasArrived` whether the walker or the
    /// controller's phase declared it.
    private func deliverFix(
        coordinate: GeographicCoordinate,
        courseHeadingDegrees: Double?,
        horizontalAccuracyMeters: Double,
        walkerDeclaredArrival: Bool
    ) {
        let snappedCoordinate: GeographicCoordinate?
        do {
            try navigationController.updateLocation(
                locationUpdate: NavigationLocationUpdate(
                    coordinate: coordinate,
                    horizontalAccuracyMeters: horizontalAccuracyMeters,
                    courseHeadingDegrees: courseHeadingDegrees,
                    timestampSeconds: nil
                )
            )
            latestTripState = navigationController.currentTripState()
            snappedCoordinate = latestTripState?.snappedCoordinate
        } catch {
            isTripActive = false
            latestTripState = nil
            tripDeliveryFailure = error
            snappedCoordinate = nil
        }
        onProcessedFix(
            RefappProcessedFix(
                snappedCoordinate: snappedCoordinate,
                courseHeadingDegrees: courseHeadingDegrees,
                horizontalAccuracyMeters: horizontalAccuracyMeters
            )
        )
        if walkerDeclaredArrival || latestTripState?.phase == .arrived {
            hasArrived = true
        }
    }

    /// Guards the driver against a run loop that is not the main one (the
    /// replay runs on the main run loop only; a foreign tick is dropped with
    /// no state change — the driver's single-thread contract, stated loudly
    /// in the log).
    private func runOnMainIfSafe() -> Bool {
        if Thread.isMainThread {
            return true
        }
        isTripActive = false
        return false
    }

    private func cancelSubscriptions() {
        subscriptions.forEach { subscription in subscription.cancel() }
        subscriptions = []
    }

    /// The interval of the pending tick, in milliseconds: a route-following
    /// trip ticks at the fixed walk interval; a recorded trace's sample
    /// interval divides by the demo acceleration and the selected multiplier.
    /// The floor keeps a fast multiplier from spinning the loop.
    func dispatchIntervalMillis() -> Int {
        guard let trace = activeTrace, !trace.followsRouteGeometry else {
            return RefappConstants.routeWalkTickIntervalMilliseconds
        }
        let demoDispatchSeconds = trace.sampleIntervalSeconds
            / (RefappConstants.demoPlaybackTimeAccelerationFactor * speedMultiplier)
        return max(
            Int(demoDispatchSeconds * GeometryConstants.millisecondsPerSecond),
            RefappConstants.minimumDispatchIntervalMilliseconds
        )
    }

    /// One route-following walk tick, in seconds: the clock advances this long
    /// (times the demo acceleration) per fix.
    private var routeWalkTickSeconds: Double {
        Double(RefappConstants.routeWalkTickIntervalMilliseconds)
            / GeometryConstants.millisecondsPerSecond
    }

    /// Builds a walker over `route` at the app's chosen pace: the route's own
    /// average speed is the baseline (a walk replays at the route's own
    /// duration at 1×), and the multiplier is the drawer's speed choice.
    private func buildRouteSimulator(
        route: Route,
        multiplier: Double
    ) throws -> RouteSimulator {
        try RouteSimulator(
            route: route,
            pace: SimulatedTripPace(
                speedMetersPerSecond: routeSpeedMetersPerSecond(route),
                speedMultiplier: multiplier
            )
        )
    }

    /// The baseline speed of a planned route, in meters per second. A route
    /// with no positive distance or duration (a single point, or a fixture
    /// that declares no travel time) falls back to the demo walk speed; the
    /// contract refuses a non-positive pace.
    private func routeSpeedMetersPerSecond(_ route: Route) -> Double {
        if route.distanceMeters > 0.0 && route.durationSeconds > 0.0 {
            return route.distanceMeters / route.durationSeconds
        }
        return RefappConstants.plannedRouteFallbackSpeedMetersPerSecond
    }

    /// Course of travel between the previous and the current sample, in
    /// degrees clockwise from true north (cheap planar bearing — consecutive
    /// samples of the walk are meters apart, where the small-angle planar
    /// form is exact enough to be indistinguishable from the geodetic one).
    private func headingDegreesFrom(
        _ previousSample: RefappTraceSample,
        _ sample: RefappTraceSample
    ) -> Double {
        let deltaLongitude = sample.longitudeDegrees - previousSample.longitudeDegrees
        let deltaLatitude = sample.latitudeDegrees - previousSample.latitudeDegrees
        let eastwardDelta = deltaLongitude * cos(sample.latitudeDegrees * GeometryConstants.degreesToRadians)
        let heading = atan2(eastwardDelta, deltaLatitude) / GeometryConstants.degreesToRadians
        return (heading + GeometryConstants.fullCircleDegrees).truncatingRemainder(
            dividingBy: GeometryConstants.fullCircleDegrees
        )
    }
}