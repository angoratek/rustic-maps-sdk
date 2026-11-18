package com.rusticmaps.sdk.refapprn

import android.content.res.AssetManager
import org.json.JSONArray
import org.json.JSONObject
import uniffi.rustic_maps.BoundingBox
import uniffi.rustic_maps.DeviationPolicy
import uniffi.rustic_maps.DeviationStatusChange
import uniffi.rustic_maps.GeographicCoordinate
import uniffi.rustic_maps.Navigator
import uniffi.rustic_maps.NavigationInstruction
import uniffi.rustic_maps.NavigationLocationUpdate
import uniffi.rustic_maps.NavigationPhase
import uniffi.rustic_maps.NavigationPhaseChange
import uniffi.rustic_maps.NavigationListener
import uniffi.rustic_maps.Subscription
import uniffi.rustic_maps.Route
import uniffi.rustic_maps.RouteLeg
import uniffi.rustic_maps.RouteSimulator
import uniffi.rustic_maps.RouteStep
import uniffi.rustic_maps.SdkException
import uniffi.rustic_maps.SimulatedTripPace
import uniffi.rustic_maps.SpeechInstruction
import uniffi.rustic_maps.StepAdvancePolicy
import uniffi.rustic_maps.TripNavigationState
import uniffi.rustic_maps.UnitSystem
import uniffi.rustic_maps.defaultDeviationPolicy
import uniffi.rustic_maps.defaultStepAdvancePolicy
import kotlin.math.atan2
import kotlin.math.cos
import kotlin.math.floor

/**
 * The React Native reference application's simulated-trace layer, mirroring the
 * Compose reference application's `SimulatedTraceLoader`/`SimulatedTripDriver`
 * (`platform/android/reference-application/.../navigation/`) for the single
 * `sacramento_clean_walk` plan the RN shell replays. Field names mirror the
 * fixture's snake_case keys.
 */
data class RefappTraceSample(
    val latitudeDegrees: Double,
    val longitudeDegrees: Double,
    val horizontalAccuracyMeters: Double,
    val timestampOffsetSeconds: Double,
)

data class RefappTrace(
    val traceName: String,
    val route: Route,
    val samples: List<RefappTraceSample>,
    val sampleIntervalSeconds: Double,
    /**
     * Whether the trip walks the route's own polyline through the contract's
     * `RouteSimulator` (a search-planned route) rather than replaying the
     * recorded [samples] verbatim (the demo scenarios). A route-following
     * trace carries no samples: the walker interpolates between the route's
     * shape vertices from elapsed simulated time.
     */
    val followsRouteGeometry: Boolean = false,
) {
    companion object {
        /**
         * Builds the simulated-trip source of a planned route (the
         * search-planned trip, mirroring the Compose reference application's
         * `SimulatedTrace.fromRouteData`): the trip follows the route's own
         * polyline through the contract's `RouteSimulator`, so the position
         * moves continuously between shape vertices and the course comes
         * from the segment bearing. No vertex samples are precomputed — the
         * walker owns the interpolation.
         */
        fun fromRouteData(route: Route, traceName: String): RefappTrace = RefappTrace(
            traceName = traceName,
            route = route,
            samples = emptyList(),
            sampleIntervalSeconds = RefappConstants.SIMULATED_TRACE_SAMPLE_INTERVAL_SECONDS,
            followsRouteGeometry = true,
        )
    }
}

/** Loads the recorded synthetic walk trace bundled as an application asset. */
object RefappTraceLoader {
    fun load(assetManager: AssetManager, traceAssetName: String): RefappTrace {
        assetManager.open(traceAssetName).use { assetStream ->
            val traceJsonObject = JSONObject(assetStream.readBytes().decodeToString())
            return RefappTrace(
                traceName = traceJsonObject.getJSONObject("metadata").getString("scenario_name"),
                route = decodeRoute(traceJsonObject.getJSONObject("route")),
                samples = traceJsonObject.getJSONArray("samples").map { sampleObject ->
                    RefappTraceSample(
                        latitudeDegrees = sampleObject.getDouble("latitude"),
                        longitudeDegrees = sampleObject.getDouble("longitude"),
                        horizontalAccuracyMeters = sampleObject.getDouble("horizontal_accuracy_meters"),
                        timestampOffsetSeconds = sampleObject.getDouble("timestamp_offset_seconds"),
                    )
                },
                sampleIntervalSeconds = traceJsonObject.getJSONObject("metadata")
                    .getDouble("sample_interval_seconds"),
            )
        }
    }

    /**
     * Decodes the trace's route record into the frozen `Route` the
     * controller's `start_navigation` consumes — the trace fixtures serialize the
     * maneuver type in the SDK's single stable label spelling, so the label
     * passes through verbatim (an unknown label fails the SDK start loudly —
     * never a guess).
     */
    fun decodeRoute(routeObject: JSONObject): Route {
        val boundingBoxObject = routeObject.getJSONObject("bounding_box")
        return Route(
            distanceMeters = routeObject.getDouble("distance_meters"),
            durationSeconds = routeObject.getDouble("duration_seconds"),
            geometry = routeObject.optJSONArray("geometry")?.map { coordinateObject ->
                GeographicCoordinate(
                    latitude = coordinateObject.getDouble("latitude"),
                    longitude = coordinateObject.getDouble("longitude"),
                )
            } ?: emptyList(),
            boundingBox = BoundingBox(
                southWest = GeographicCoordinate(
                    latitude = boundingBoxObject.getJSONObject("south_west").getDouble("latitude"),
                    longitude = boundingBoxObject.getJSONObject("south_west").getDouble("longitude"),
                ),
                northEast = GeographicCoordinate(
                    latitude = boundingBoxObject.getJSONObject("north_east").getDouble("latitude"),
                    longitude = boundingBoxObject.getJSONObject("north_east").getDouble("longitude"),
                ),
            ),
            legs = routeObject.getJSONArray("legs").map { legObject ->
                RouteLeg(
                    distanceMeters = legObject.getDouble("distance_meters"),
                    durationSeconds = legObject.getDouble("duration_seconds"),
                    steps = legObject.getJSONArray("steps").map { stepObject ->
                        RouteStep(
                            distanceMeters = stepObject.getDouble("distance_meters"),
                            durationSeconds = stepObject.getDouble("duration_seconds"),
                            maneuverType = stepObject.getString("maneuver_type"),
                            instructionDisplayText = stepObject.getString("instruction_display_text"),
                            instructionSpeechText = stepObject.getString("instruction_speech_text"),
                            shape = stepObject.getJSONArray("shape").map { coordinateObject ->
                                GeographicCoordinate(
                                    latitude = coordinateObject.getDouble("latitude"),
                                    longitude = coordinateObject.getDouble("longitude"),
                                )
                            },
                        )
                    },
                )
            },
        )
    }
}

internal inline fun <T> org.json.JSONArray.map(transform: (JSONObject) -> T): List<T> =
    (0 until length()).map { index -> transform(getJSONObject(index)) }

/**
 * The SDK listener seam's one collecting sink (the Compose app's
 * `CollectingNavigationStateListener` pattern): topics deliver synchronously on
 * the thread that drives the controller call (the demo driver's main-looper
 * ticks), and each payload is handed straight to the callback the reference
 * controller registered (which emits it to the React side). Listeners never
 * re-enter the controller.
 */
class RefappCollectingListener(
    private val onInstruction: (NavigationInstruction) -> Unit,
    private val onDeviationStatusChange: (DeviationStatusChange) -> Unit,
    private val onPhaseChange: (NavigationPhaseChange) -> Unit,
    // The callback name stays `SpeechInstructionDelivery`: the interface
    // method's own name (`onSpeechInstruction`) would collide with the
    // constructor property and recurse the override into itself — the
    // property/method name shadow the sibling topics' callbacks avoid too.
    private val onSpeechInstructionDelivery: (SpeechInstruction) -> Unit,
) : NavigationListener {
    /** Latest delivered payloads (the read-only queries only seed state). */
    var latestInstruction: NavigationInstruction? = null
    var latestDeviationStatusChange: DeviationStatusChange? = null
    var latestPhaseChange: NavigationPhaseChange? = null
    var latestSpeechInstruction: SpeechInstruction? = null

    override fun onNavigationInstruction(instruction: NavigationInstruction) {
        latestInstruction = instruction
        onInstruction(instruction)
    }

    override fun onDeviationStatusChange(deviationStatusChange: DeviationStatusChange) {
        latestDeviationStatusChange = deviationStatusChange
        onDeviationStatusChange(deviationStatusChange)
    }

    override fun onNavigationPhaseChange(navigationPhaseChange: NavigationPhaseChange) {
        latestPhaseChange = navigationPhaseChange
        onPhaseChange(navigationPhaseChange)
    }

    override fun onSpeechInstruction(speechInstruction: SpeechInstruction) {
        // The speech topic's delivery crosses to the React side verbatim (the
        // contract's platform half, RN-shaped): the shell's speech
        // consumer — the jest-testable JS module — owns the dedup, pacing and
        // toggle decisions, and drives the app-owned TextToSpeech engine
        // through the bridge module. This side records the delivery and never
        // voices anything itself.
        latestSpeechInstruction = speechInstruction
        onSpeechInstructionDelivery(speechInstruction)
    }

    /**
     * Forgets every delivered payload (the Compose application's re-subscription
     * replay rule): a new trip must not read the previous trip's arrived phase
     * or instruction as its own.
     */
    fun reset() {
        latestInstruction = null
        latestDeviationStatusChange = null
        latestPhaseChange = null
        latestSpeechInstruction = null
    }
}

/**
 * One processed simulated fix (the Compose reference application's
 * `SimulatedProcessedFix` mirror): the reducer's snapped coordinate plus the
 * fix's own course-over-ground and horizontal accuracy report — the fields
 * the renderer-owned location indicator's state carries (§4.2: the renderer
 * owns the visuals, the controller only the state).
 */
data class RefappProcessedFix(
    val snappedCoordinate: GeographicCoordinate?,
    val courseHeadingDegrees: Double?,
    val horizontalAccuracyMeters: Double?,
)

/**
 * Drives the simulated turn-by-turn demo through
 * [Navigator.updateLocation], mirroring the Compose app's
 * `SimulatedTripDriver`. Two playback sources share the driver:
 * - a search-planned route is walked with the contract's [RouteSimulator]:
 *   one simulator per trip (and one rebuilt at each pace change), its fixes
 *   asked for by **elapsed simulated time**, so the position moves
 *   continuously between the route's shape vertices and the course is the
 *   segment bearing;
 * - a recorded demo trace replays its own samples at the documented demo
 *   acceleration ([RefappConstants.DEMO_PLAYBACK_TIME_ACCELERATION_FACTOR]
 *   times real time, divided by the selected speed multiplier).
 *
 * Playback can be paused and resumed at the current fix and runs at the
 * selected speed multiplier; an arrival holds [hasArrived] so the
 * presentation outlives the playback and the trip never snaps back. The
 * driver runs on the main looper, like every other seam this shell drives.
 * The clean-walk trace never deviates, so no reroute policy applies here.
 */
class RefappTraceDriver(
    private val navigationController: Navigator,
    private val listener: RefappCollectingListener,
    private val onProcessedFix: (RefappProcessedFix) -> Unit,
    private val onArrival: () -> Unit,
) {
    /** Whether a simulated trip is presented and driven (a paused trip stays active). */
    var isTripActive: Boolean = false
        private set

    /** Whether the active trip's playback is paused at its current fix. */
    var isTripPaused: Boolean = false
        private set

    /**
     * Whether the active trip reached the route's destination. The arrival
     * presentation outlives the playback (the trip does not snap back), and
     * is reset when the next trip starts.
     */
    var hasArrived: Boolean = false
        private set

    /**
     * The playback speed multiplier the trip advances at — one of
     * [RefappConstants.SIMULATED_TRIP_SPEED_MULTIPLIER_CHOICES]. The choice
     * persists across trips (the user picked it for the session).
     */
    var speedMultiplier: Double = RefappConstants.SIMULATED_TRIP_SPEED_MULTIPLIER_CHOICES.first()
        private set

    /** The controller's latest read-only trip state, refreshed with every fix. */
    var latestTripState: TripNavigationState? = null
        private set

    /** The verbatim start failure, shown when the trip could not start. */
    var startFailure: SdkException? = null
        private set

    /** The verbatim failure of a mid-trip fix delivery; stops the replay. */
    var tripDeliveryFailure: Throwable? = null
        private set

    private var subscriptions: List<Subscription> = emptyList()
    private var activeTrace: RefappTrace? = null
    private var sampleIndex: Int = 0
    private val dispatchHandler = android.os.Handler(android.os.Looper.getMainLooper())

    /** The route walker of a route-following trip, or null for a recorded trace. */
    private var routeSimulator: RouteSimulator? = null

    /** The route-following walk's clock, in simulated seconds since the trip start. */
    private var elapsedSimulatedSeconds: Double = 0.0

    /** The distance the active trip has travelled, in meters (pace-change anchor). */
    var travelledDistanceMeters: Double = 0.0
        private set

    /** The route-following walk's total length, in meters; 0 when not seekable. */
    var seekableDistanceMeters: Double = 0.0
        private set

    /** Whether the presented trip walks the route's own geometry (the seek). */
    val isSeekable: Boolean
        get() = routeSimulator != null

    /** Whether the walk's start fix (elapsed zero) has already been delivered. */
    private var hasDeliveredRouteWalkStartFix: Boolean = false

    /**
     * Starts (or replaces) the simulated trip: subscribes the topics first so
     * the initial instruction dispatch of `start_navigation` is received,
     * builds the route walker when the trip follows the route's own geometry,
     * then drives the fixes. Throws nothing: start failures surface verbatim
     * through [startFailure] (§6.3).
     */
    fun startTrip(trace: RefappTrace) {
        stopTrip()
        try {
            startGuidance(trace.route)
            if (trace.followsRouteGeometry) {
                // One walker per trip, over the route the visitor planned.
                routeSimulator = buildRouteSimulator(trace.route, speedMultiplier)
            }
        } catch (startException: SdkException) {
            cancelSubscriptions()
            // The route walker is built after `start_navigation` succeeded, so
            // a refused pace/route must end the newly started trip too rather
            // than leave a half-started one (a no-op when the start itself
            // failed).
            navigationController.stopNavigation()
            routeSimulator = null
            startFailure = startException
            return
        }
        isTripActive = true
        isTripPaused = false
        hasArrived = false
        startFailure = null
        tripDeliveryFailure = null
        activeTrace = trace
        sampleIndex = 0
        seekableDistanceMeters = routeSimulator?.totalDistanceMeters() ?: 0.0
        elapsedSimulatedSeconds = 0.0
        travelledDistanceMeters = 0.0
        hasDeliveredRouteWalkStartFix = false
        scheduleNextTick()
    }

    /**
     * Starts guidance on [route] from the device's own fixes: the same topic
     * subscriptions and `start_navigation` call as a simulated trip, without
     * a playback loop. [deliverDeviceFix] feeds each accepted fix through the
     * same [deliverFix] path, so the banner, the ETA line and the follow
     * camera all read the controller's own stream. Throws nothing: a start
     * failure surfaces verbatim through [startFailure].
     */
    fun startDeviceTrip(route: Route) {
        stopTrip()
        try {
            startGuidance(route)
        } catch (startException: SdkException) {
            cancelSubscriptions()
            navigationController.stopNavigation()
            startFailure = startException
            return
        }
        isTripActive = true
        isTripPaused = false
        hasArrived = false
        startFailure = null
        tripDeliveryFailure = null
        latestTripState = null
        activeTrace = null
        sampleIndex = 0
        seekableDistanceMeters = 0.0
        elapsedSimulatedSeconds = 0.0
        travelledDistanceMeters = 0.0
        hasDeliveredRouteWalkStartFix = false
    }

    /**
     * Subscribes the four navigation topics before starting the guidance
     * session, so the session's initial instruction dispatch is received,
     * then starts the trip at the app's unit system and policies. A simulated
     * trace and the device's fixes share this start path.
     */
    private fun startGuidance(route: Route) {
        val deviationPolicy: DeviationPolicy = defaultDeviationPolicy()
        val stepAdvancePolicy: StepAdvancePolicy = defaultStepAdvancePolicy()
        subscriptions = listOf(
            navigationController.subscribeToNavigationInstructions(listener),
            navigationController.subscribeToDeviationStatus(listener),
            navigationController.subscribeToPhaseChanges(listener),
            navigationController.subscribeToSpeechInstructions(listener),
        )
        navigationController.startNavigation(
            route,
            UnitSystem.METRIC,
            stepAdvancePolicy,
            deviationPolicy,
        )
    }

    /**
     * One accepted device fix of a device-driven trip, delivered through the
     * same [deliverFix] path a simulated fix uses: the controller reports the
     * snapped state the banner and ETA read, and the session's fix callback
     * drives the follow camera and the location indicator. An arrival ends
     * the active flag the way the playback loop does, leaving the arrival
     * presentation in place.
     */
    fun deliverDeviceFix(
        coordinate: GeographicCoordinate,
        courseHeadingDegrees: Double?,
        horizontalAccuracyMeters: Double,
    ) {
        // A fix that arrives after the mode ended must not reach the
        // controller; after an arrival the source keeps reporting, so the
        // indicator can follow the device while the destination presentation
        // stays.
        if (!isTripActive && !hasArrived) {
            return
        }
        deliverFix(
            coordinate = coordinate,
            courseHeadingDegrees = courseHeadingDegrees,
            horizontalAccuracyMeters = horizontalAccuracyMeters,
            walkerDeclaredArrival = false,
        )
        if (hasArrived) {
            isTripActive = false
        }
    }

    /** Pauses the active trip at its current fix (the presentation holds). */
    fun pauseTrip() {
        if (isTripActive) {
            isTripPaused = true
        }
    }

    /** Resumes a paused active trip from its current fix. */
    fun resumeTrip() {
        isTripPaused = false
    }

    /**
     * Selects the playback speed multiplier — one of
     * [RefappConstants.SIMULATED_TRIP_SPEED_MULTIPLIER_CHOICES]. A
     * route-following trip rebuilds its walker at the new pace and re-anchors
     * the clock at the distance already covered, so a mid-trip pace change
     * never teleports the position. A recorded trace's replay cadence follows
     * the choice. A refusal stops the playback honestly (§6.3).
     */
    fun selectSpeedMultiplier(multiplier: Double) {
        val isOfferedChoice = RefappConstants.SIMULATED_TRIP_SPEED_MULTIPLIER_CHOICES
            .any { choice -> choice == multiplier }
        if (!isOfferedChoice || multiplier == speedMultiplier) {
            return
        }
        val trace = activeTrace
        if (trace != null && trace.followsRouteGeometry && isTripActive) {
            val rebuiltSimulator = try {
                buildRouteSimulator(trace.route, multiplier)
            } catch (rebuildException: SdkException) {
                tripDeliveryFailure = rebuildException
                isTripActive = false
                return
            }
            // Keep the travelled distance continuous: the rebuilt walker's
            // clock restarts at the distance the trip already covered, so a
            // mid-trip pace change never teleports the position.
            val currentFix = rebuiltSimulator.fixAtTravelledMeters(travelledDistanceMeters)
            routeSimulator = rebuiltSimulator
            elapsedSimulatedSeconds = currentFix.elapsedSeconds
        }
        speedMultiplier = multiplier
        if (isTripActive && !isTripPaused && !hasArrived) {
            // Re-arm the pending tick at the new cadence (the route tick stays
            // fixed; the recorded trace's interval divides by the multiplier).
            dispatchHandler.removeCallbacks(dispatchRunnable)
            scheduleNextTick()
        }
    }

    /**
     * Moves the presented trip to [targetMeters] along the route's own
     * geometry (the seek). The walker supplies the fix, so the camera, the
     * indicator, the ETA line and the arrival all read exactly what a normal
     * fix at that distance produces.
     *
     * A paused trip stays paused; a running trip continues from the new
     * position. Seeking backwards out of the arrival restarts the navigation
     * session at the seek position, because the contract's reducer ignores
     * fixes once it reports `Arrived`.
     */
    fun seekToTravelledMeters(targetMeters: Double) {
        val trace = activeTrace ?: return
        val simulator = routeSimulator ?: return
        if (!trace.followsRouteGeometry) {
            return
        }
        val targetFix = simulator.fixAtTravelledMeters(
            targetMeters.coerceIn(0.0, simulator.totalDistanceMeters()),
        )
        if (hasArrived && !targetFix.isArrived) {
            try {
                listener.reset()
                navigationController.startNavigation(
                    trace.route,
                    UnitSystem.METRIC,
                    defaultStepAdvancePolicy(),
                    defaultDeviationPolicy(),
                )
            } catch (startException: Exception) {
                isTripActive = false
                tripDeliveryFailure = startException
                return
            }
        }
        hasArrived = false
        isTripActive = true
        elapsedSimulatedSeconds = targetFix.elapsedSeconds
        travelledDistanceMeters = targetFix.travelledMeters
        hasDeliveredRouteWalkStartFix = true
        deliverFix(
            coordinate = targetFix.coordinate,
            courseHeadingDegrees = targetFix.courseHeadingDegrees,
            horizontalAccuracyMeters =
                RefappConstants.SIMULATED_TRACE_HORIZONTAL_ACCURACY_METERS,
            walkerDeclaredArrival = targetFix.isArrived,
        )
        if (tripDeliveryFailure != null) {
            isTripActive = false
            return
        }
        if (hasArrived) {
            isTripActive = false
            onArrival()
            return
        }
        dispatchHandler.removeCallbacks(dispatchRunnable)
        scheduleNextTick()
    }

    /** Stops the replay, cancels the subscriptions; the trip state stays as-is. */
    fun stopTrip() {
        isTripActive = false
        isTripPaused = false
        hasArrived = false
        dispatchHandler.removeCallbacksAndMessages(null)
        cancelSubscriptions()
        listener.reset()
        activeTrace = null
        routeSimulator = null
        elapsedSimulatedSeconds = 0.0
        travelledDistanceMeters = 0.0
        seekableDistanceMeters = 0.0
        hasDeliveredRouteWalkStartFix = false
        sampleIndex = 0
    }

    private val dispatchRunnable = Runnable { advancePlayback() }

    /**
     * Advances the playback one tick: a route-following trip asks its
     * [RouteSimulator] for the fix at the advanced simulated clock; a
     * recorded trace delivers its next sample. A paused trip keeps its
     * current fix; an arrival ends the loop with [hasArrived] set (the
     * arrival presentation outlives the playback).
     */
    private fun advancePlayback() {
        if (!isTripActive) {
            return
        }
        if (isTripPaused || hasArrived) {
            scheduleNextTick()
            return
        }
        val trace = activeTrace
        if (trace == null) {
            isTripActive = false
            return
        }
        if (trace.followsRouteGeometry) {
            val simulator = routeSimulator
            if (simulator == null) {
                // A route-following trip always builds its walker before the
                // loop starts; a missing one ends the playback rather than
                // leaving the trip stuck as "active".
                isTripActive = false
                return
            }
            if (hasDeliveredRouteWalkStartFix) {
                elapsedSimulatedSeconds += ROUTE_WALK_TICK_SECONDS *
                    RefappConstants.DEMO_PLAYBACK_TIME_ACCELERATION_FACTOR
            }
            hasDeliveredRouteWalkStartFix = true
            val fix = simulator.fixAtElapsedSeconds(elapsedSimulatedSeconds)
            travelledDistanceMeters = fix.travelledMeters
            deliverFix(
                coordinate = fix.coordinate,
                courseHeadingDegrees = fix.courseHeadingDegrees,
                horizontalAccuracyMeters =
                    RefappConstants.SIMULATED_TRACE_HORIZONTAL_ACCURACY_METERS,
                walkerDeclaredArrival = fix.isArrived,
            )
            if (tripDeliveryFailure != null) {
                isTripActive = false
                return
            }
            if (hasArrived) {
                isTripActive = false
                onArrival()
                return
            }
        } else {
            if (sampleIndex >= trace.samples.size) {
                // The samples ran out without arrival: the demo driver stops
                // replaying but keeps the trip's state (the documented exit).
                isTripActive = false
                return
            }
            deliverSample(trace)
            if (tripDeliveryFailure != null) {
                isTripActive = false
                return
            }
            if (hasArrived) {
                isTripActive = false
                onArrival()
                return
            }
        }
        scheduleNextTick()
    }

    /** Arms the pending playback tick at the interval of the active source. */
    private fun scheduleNextTick() {
        if (!isTripActive) {
            return
        }
        dispatchHandler.postDelayed(dispatchRunnable, dispatchIntervalMillis())
    }

    /** One recorded sample of the demo replay, with its planar course. */
    private fun deliverSample(trace: RefappTrace) {
        val sample = if (sampleIndex in trace.samples.indices) trace.samples[sampleIndex] else return
        val previousSample = trace.samples.getOrNull(sampleIndex - 1)
        val sampleCourseHeadingDegrees: Double? = previousSample?.let { previous ->
            headingDegreesFrom(previous, sample)
        }
        deliverFix(
            coordinate = GeographicCoordinate(
                latitude = sample.latitudeDegrees,
                longitude = sample.longitudeDegrees,
            ),
            courseHeadingDegrees = sampleCourseHeadingDegrees,
            horizontalAccuracyMeters = sample.horizontalAccuracyMeters,
            walkerDeclaredArrival = false,
        )
        sampleIndex += 1
    }

    /**
     * Feeds one fix to the controller and reports its processed state. A
     * failure stops the replay honestly (§6.3); an arrival sets [hasArrived]
     * whether the walker or the controller's phase declared it.
     */
    private fun deliverFix(
        coordinate: GeographicCoordinate,
        courseHeadingDegrees: Double?,
        horizontalAccuracyMeters: Double,
        walkerDeclaredArrival: Boolean,
    ) {
        try {
            navigationController.updateLocation(
                NavigationLocationUpdate(
                    coordinate = coordinate,
                    horizontalAccuracyMeters = horizontalAccuracyMeters,
                    courseHeadingDegrees = courseHeadingDegrees,
                    timestampSeconds = null,
                ),
            )
            latestTripState = navigationController.currentTripState()
            onProcessedFix(
                RefappProcessedFix(
                    snappedCoordinate = latestTripState?.snappedCoordinate,
                    courseHeadingDegrees = courseHeadingDegrees,
                    horizontalAccuracyMeters = horizontalAccuracyMeters,
                ),
            )
            if (walkerDeclaredArrival || latestTripState?.phase == NavigationPhase.ARRIVED) {
                hasArrived = true
            }
        } catch (deliverException: Exception) {
            // Exception, not SdkException alone: native-internal failures
            // (uniffi InternalException) are no SdkException variant, and a
            // failed fix delivery must stop the replay honestly (§6.3), not
            // crash the demo.
            isTripActive = false
            latestTripState = null
            tripDeliveryFailure = deliverException
        }
    }

    private fun cancelSubscriptions() {
        subscriptions.forEach { subscription -> subscription.cancel() }
        subscriptions = emptyList()
    }

    /**
     * The interval of the pending tick, in milliseconds: a route-following
     * trip ticks at the fixed walk interval; a recorded trace's sample
     * interval divides by the demo acceleration and the selected multiplier.
     * The floor keeps a fast multiplier from spinning the loop.
     */
    internal fun dispatchIntervalMillis(): Long {
        val trace = activeTrace
        if (trace == null || trace.followsRouteGeometry) {
            return RefappConstants.ROUTE_WALK_TICK_INTERVAL_MILLISECONDS
        }
        val demoDispatchSeconds = trace.sampleIntervalSeconds /
            (RefappConstants.DEMO_PLAYBACK_TIME_ACCELERATION_FACTOR * speedMultiplier)
        return (demoDispatchSeconds * MILLISECONDS_PER_SECOND).toLong()
            .coerceAtLeast(MINIMUM_DISPATCH_INTERVAL_MILLISECONDS)
    }

    /**
     * Builds a walker over [route] at the app's chosen pace: the route's own
     * average speed is the baseline (a walk replays at the route's own
     * duration at 1×), and the multiplier is the drawer's speed choice.
     */
    private fun buildRouteSimulator(route: Route, multiplier: Double): RouteSimulator =
        RouteSimulator(
            route,
            SimulatedTripPace(
                speedMetersPerSecond = routeSpeedMetersPerSecond(route),
                speedMultiplier = multiplier,
            ),
        )

    /**
     * The baseline speed of a planned route, in meters per second. A route
     * with no positive distance or duration (a single point, or a fixture
     * that declares no travel time) falls back to the demo walk speed; the
     * contract refuses a non-positive pace.
     */
    private fun routeSpeedMetersPerSecond(route: Route): Double =
        if (route.distanceMeters > 0.0 && route.durationSeconds > 0.0) {
            route.distanceMeters / route.durationSeconds
        } else {
            RefappConstants.PLANNED_ROUTE_FALLBACK_SPEED_METERS_PER_SECOND
        }

    /**
     * Course of travel between the previous and the current sample, in degrees
     * clockwise from true north (cheap planar bearing — consecutive samples of
     * the walk are meters apart, where the small-angle planar form is exact
     * enough to be indistinguishable from the geodetic one).
     */
    private fun headingDegreesFrom(previousSample: RefappTraceSample, sample: RefappTraceSample): Double {
        val deltaLongitude = sample.longitudeDegrees - previousSample.longitudeDegrees
        val deltaLatitude = sample.latitudeDegrees - previousSample.latitudeDegrees
        val eastwardDelta = deltaLongitude * cos(sample.latitudeDegrees * DEGREES_TO_RADIANS)
        val heading = atan2(eastwardDelta, deltaLatitude) / DEGREES_TO_RADIANS
        return (heading + FULL_CIRCLE_DEGREES) % FULL_CIRCLE_DEGREES
    }

    private companion object {
        const val MILLISECONDS_PER_SECOND: Double = 1000.0

        /**
         * One route-following walk tick, in seconds: the clock advances this
         * long (times the demo acceleration) per fix, so the position moves
         * every [RefappConstants.ROUTE_WALK_TICK_INTERVAL_MILLISECONDS].
         */
        val ROUTE_WALK_TICK_SECONDS: Double =
            RefappConstants.ROUTE_WALK_TICK_INTERVAL_MILLISECONDS / 1000.0

        /** The shortest dispatch interval, so a fast multiplier never spins. */
        const val MINIMUM_DISPATCH_INTERVAL_MILLISECONDS: Long = 1L

        /** Structural math constant: `degrees * DEGREES_TO_RADIANS = radians`. */
        const val DEGREES_TO_RADIANS: Double = kotlin.math.PI / 180.0
        const val FULL_CIRCLE_DEGREES: Double = 360.0
    }
}

/** Formats the metric distance for the readiness drawer's lines (Compose helper). */
internal fun formatDistanceMeters(distanceMeters: Double): String =
    if (distanceMeters < RefappConstants.METERS_PER_KILOMETER) {
        "%.0f m".format(distanceMeters)
    } else {
        "%.2f km".format(distanceMeters / RefappConstants.METERS_PER_KILOMETER)
    }

/** Formats the duration for the drawer's ETA-distance line (Compose helper). */
internal fun formatDurationSeconds(durationSeconds: Double): String {
    val wholeMinutes = floor(durationSeconds / RefappConstants.SECONDS_PER_MINUTE)
    val remainderSeconds = durationSeconds - wholeMinutes * RefappConstants.SECONDS_PER_MINUTE
    return if (wholeMinutes > 0.0) {
        "%.0f min %.0f s".format(wholeMinutes, remainderSeconds)
    } else {
        "%.0f s".format(durationSeconds)
    }
}