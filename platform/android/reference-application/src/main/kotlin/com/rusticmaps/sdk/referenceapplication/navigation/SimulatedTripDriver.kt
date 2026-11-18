package com.rusticmaps.sdk.referenceapplication.navigation

import android.content.res.AssetManager
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import org.json.JSONException
import org.json.JSONObject
import uniffi.map_refapp_core.TraceCursor
import uniffi.map_refapp_core.traceDispatchIntervalMilliseconds
import uniffi.rustic_maps.BoundingBox
import uniffi.rustic_maps.GeographicCoordinate
import uniffi.rustic_maps.Navigator
import uniffi.rustic_maps.NavigationLocationUpdate
import uniffi.rustic_maps.NavigationPhase
import uniffi.rustic_maps.Subscription
import uniffi.rustic_maps.NavigationPhaseChange
import uniffi.rustic_maps.Route
import uniffi.rustic_maps.RouteLeg
import uniffi.rustic_maps.RouteSimulator
import uniffi.rustic_maps.RouteStep
import uniffi.rustic_maps.SimulatedTripPace
import uniffi.rustic_maps.StepAdvancePolicy
import uniffi.rustic_maps.TripNavigationState
import uniffi.rustic_maps.UnitSystem
import uniffi.rustic_maps.SdkException
import uniffi.rustic_maps.defaultDeviationPolicy
import uniffi.rustic_maps.defaultStepAdvancePolicy
import com.rusticmaps.sdk.navigation.CollectingNavigationStateListener
import com.rusticmaps.sdk.referenceapplication.ReferenceApplicationConstants
import kotlin.math.atan2
import kotlin.math.cos

/**
 * One recorded position fix of the synthetic walk trace. Field names mirror
 * the fixture's snake_case sample keys.
 */
data class SimulatedTraceSample(
    val latitudeDegrees: Double,
    val longitudeDegrees: Double,
    val horizontalAccuracyMeters: Double,
    val timestampOffsetSeconds: Double,
)

/**
 * The recorded trace fixture payload: the route the trip starts on, the
 * ordered samples replayed through the controller, and the recorded sample
 * interval the demo playback accelerates. The reroute scenario additionally
 * carries the replacement route the replay applies the moment the trip is
 * first judged off route (§7.5: a reroute is a state transition, not an
 * error).
 */
data class SimulatedTrace(
    val traceName: String,
    val route: Route,
    val samples: List<SimulatedTraceSample>,
    val sampleIntervalSeconds: Double,
    val rerouteRoute: Route? = null,
    /**
     * Whether the trip walks the route's own polyline through the contract's
     * [RouteSimulator] (a search-planned route) rather than replaying the
     * recorded [samples] verbatim (the demo scenarios). A route-following
     * trace carries no samples: the walker interpolates between the route's
     * shape vertices from elapsed simulated time.
     */
    val followsRouteGeometry: Boolean = false,
) {
    companion object {
        /**
         * Builds the simulated-trip source of a planned route: the trip
         * follows the route's own polyline through the contract's
         * [RouteSimulator], so the position moves continuously between shape
         * vertices and the course comes from the segment bearing. No vertex
         * samples are precomputed — the walker owns the interpolation.
         */
        fun fromRouteData(route: Route, traceName: String): SimulatedTrace = SimulatedTrace(
            traceName = traceName,
            route = route,
            samples = emptyList(),
            sampleIntervalSeconds = ReferenceApplicationConstants.SIMULATED_TRACE_SAMPLE_INTERVAL_SECONDS,
            rerouteRoute = null,
            followsRouteGeometry = true,
        )
    }
}

/**
 * Loads the recorded synthetic walk trace bundled as an application asset.
 * The fixture is the single committed source
 * (`crates/rustic-maps-navigation/fixtures/navigation/traces/sacramento_clean_walk.json`,
 * byte-locked by `rustic-maps-navigation`'s own tests); the Gradle copy task bundles
 * the identical data, so the app cannot present different demo guidance.
 */
object SimulatedTraceLoader {
    /** Loads and decodes the trace fixture from the application assets. */
    fun load(assetManager: AssetManager, traceAssetName: String): SimulatedTrace {
        assetManager.open(traceAssetName).use { assetStream ->
            val traceJsonObject = JSONObject(assetStream.readBytes().decodeToString())
            val routeObject = traceJsonObject.getJSONObject("route")
            val metadataObject = traceJsonObject.getJSONObject("metadata")
            val samples = traceJsonObject.getJSONArray("samples").map { sampleObject ->
                SimulatedTraceSample(
                    latitudeDegrees = sampleObject.getDouble("latitude"),
                    longitudeDegrees = sampleObject.getDouble("longitude"),
                    horizontalAccuracyMeters = sampleObject.getDouble("horizontal_accuracy_meters"),
                    timestampOffsetSeconds = sampleObject.getDouble("timestamp_offset_seconds"),
                )
            }
            return SimulatedTrace(
                traceName = metadataObject.getString("scenario_name"),
                route = decodeRoute(routeObject),
                samples = samples,
                sampleIntervalSeconds = metadataObject.getDouble("sample_interval_seconds"),
                rerouteRoute = traceJsonObject.optJSONObject("reroute_route")?.let(::decodeRoute),
            )
        }
    }

    /**
     * Decodes the trace's route record into the frozen `Route` the
     * controller's `start_navigation` consumes — the same shapes the navigation
     * reducer's replay tests feed, with the trace's maneuver labels mapping
     * back through the contract's own conversion.
     */
    fun decodeRoute(routeObject: JSONObject): Route {
        val boundingBoxObject = routeObject.getJSONObject("bounding_box")
        return Route(
            distanceMeters = routeObject.getDouble("distance_meters"),
            durationSeconds = routeObject.getDouble("duration_seconds"),
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
            // The trace records per-step shapes, so the route-level geometry
            // is empty (the contract's documented shape for it).
            geometry = emptyList(),
            legs = routeObject.getJSONArray("legs").map { legObject ->
                RouteLeg(
                    distanceMeters = legObject.getDouble("distance_meters"),
                    durationSeconds = legObject.getDouble("duration_seconds"),
                    steps = legObject.getJSONArray("steps").map { stepObject ->
                        RouteStep(
                            distanceMeters = stepObject.getDouble("distance_meters"),
                            durationSeconds = stepObject.getDouble("duration_seconds"),
                            // The trace fixtures serialize the maneuver type in
                            // the SDK's single stable label spelling since the
                            // round-9 unification; the label passes through
                            // verbatim (an unknown label fails the SDK start
                            // loudly — never a guess).
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

private inline fun <T> org.json.JSONArray.map(transform: (JSONObject) -> T): List<T> =
    (0 until length()).map { index -> transform(getJSONObject(index)) }

/**
 * One processed simulated fix (round 10's indicator/follow wiring): the
 * reducer's snapped coordinate plus the fix's own course-over-ground and
 * horizontal accuracy report, the fields the location indicator's state
 * dictionary carries (§4.2: the renderer owns the visuals, the app the state).
 */
data class SimulatedProcessedFix(
    val snappedCoordinate: GeographicCoordinate?,
    val courseHeadingDegrees: Double?,
    val horizontalAccuracyMeters: Double?,
)

/**
 * Drives the simulated turn-by-turn demo: subscribes the collecting listener
 * to all four topics (the round-10 speech-instruction trigger topic
 * included), starts the trip on the active route, and feeds position fixes
 * through [Navigator.updateLocation].
 *
 * Two playback sources share this driver:
 * - a search-planned route is walked with the contract's [RouteSimulator]:
 *   one simulator per trip (and one rebuilt at each pace change), its fixes
 *   asked for by **elapsed simulated time**, so the position moves
 *   continuously between the route's shape vertices and the course is the
 *   segment bearing;
 * - a recorded demo trace replays its own samples on the dispatch interval
 *   (simulated time advances
 * [ReferenceApplicationConstants.DEMO_PLAYBACK_TIME_ACCELERATION_FACTOR]
 * times per tick, divided by the selected speed multiplier).
 *
 * Playback can be paused and resumed at the current fix and runs at the
 * selected speed multiplier; an arrival keeps [hasArrived] set so the
 * presentation outlives the playback and the trip never snaps back. The fix
 * callback receives the snap coordinate of every processed fix plus the
 * fix's course and accuracy report so the reference camera and the
 * renderer-owned location indicator can follow the simulated walker.
 *
 * Guidance started from the device's own fixes shares this driver's start
 * and delivery path ([startDeviceTrip] and [deliverDeviceFix]) without a
 * playback loop, so the banner, the ETA line, the location indicator and the
 * follow camera read exactly the same controller stream.
 */
class SimulatedTripDriver(
    private val navigationController: Navigator,
    private val listener: CollectingNavigationStateListener,
    private val onSnappedPosition: (SimulatedProcessedFix) -> Unit,
) {
    /**
     * Whether a simulated trip is currently presented and driven (a paused
     * trip stays active: the presentation holds its current fix).
     */
    var isTripActive: Boolean by mutableStateOf(false)
        private set

    /** Whether the active trip's playback is paused at its current fix. */
    var isTripPaused: Boolean by mutableStateOf(false)
        private set

    /**
     * Whether the active trip reached the route's destination. The arrival
     * presentation outlives the playback (the trip does not snap back), and
     * is reset when the next trip starts.
     */
    var hasArrived: Boolean by mutableStateOf(false)
        private set

    /**
     * The playback speed multiplier the trip advances at — one of
     * [ReferenceApplicationConstants.SIMULATED_TRIP_SPEED_MULTIPLIER_CHOICES].
     * The choice persists across trips (the user picked it for the session).
     */
    var speedMultiplier: Double by mutableStateOf(
        ReferenceApplicationConstants.SIMULATED_TRIP_SPEED_MULTIPLIER_CHOICES.first(),
    )
        private set

    /** The verbatim start failure, shown when the trip could not start. */
    var startFailure: SdkException? by mutableStateOf(null)
        private set

    /**
     * The distance the active trip has travelled, in meters. The drawer's seek
     * control reads it, and a pace change or a seek anchors the walk's clock
     * at it.
     */
    var travelledDistanceMeters: Double by mutableStateOf(0.0)
        private set

    /**
     * The active route-following walk's total length, in meters; 0 while the
     * presented trip cannot be seeked (a recorded demo trace replays its own
     * samples, not the route geometry).
     */
    var seekableDistanceMeters: Double by mutableStateOf(0.0)
        private set

    /**
     * Whether the presented trip can be seeked: it walks the route's own
     * geometry through a [RouteSimulator] (recorded demo traces replay their
     * samples verbatim and have no route position to seek to).
     */
    val isSeekable: Boolean
        get() = routeSimulator != null

    /**
     * The verbatim failure of a mid-trip fix delivery (position updates and
     * state reads), if any; stops the replay immediately (the trip is not
     * silently half-driven), cleared when the next trip starts.
     */
    var tripDeliveryFailure: Throwable? by mutableStateOf(null)
        private set

    /**
     * The controller's latest read-only trip state, refreshed with every
     * delivered fix (the progress card and the arrival card read it); null
     * until the first fix of a started trip is processed.
     */
    var latestTripState: TripNavigationState? by mutableStateOf(null)
        private set

    private var subscriptions: List<Subscription> = emptyList()
    private var driveJob: Job? = null
    private var activeStepAdvancePolicy: StepAdvancePolicy = defaultStepAdvancePolicy()

    /** The trace the active trip drives, or null while no trip is active. */
    private var activeTrace: SimulatedTrace? = null

    /** The route walker of a route-following trip, or null for a recorded trace. */
    private var routeSimulator: RouteSimulator? = null

    /** The route-following walk's clock, in simulated seconds since the trip start. */
    private var elapsedSimulatedSeconds: Double = 0.0

    /** The scope the playback loop runs in (re-armed by a seek out of arrival). */
    private var playbackScope: CoroutineScope? = null

    /**
     * Whether the replay already applied the trace's replacement route
     * (§7.5): the reroute is issued exactly once, at the first fix the trip
     * is judged off route on.
     */
    private var hasIssuedReroute: Boolean = false

    /**
     * Starts (or replaces) the simulated trip: subscribes the topics first so
     * the initial instruction dispatch of `start_navigation` is received,
     * builds the route walker when the trip follows the route's own geometry,
     * then drives the fixes. Throws nothing: start failures surface verbatim
     * through [startFailure] (§6.3).
     */
    fun startTrip(scope: CoroutineScope, trace: SimulatedTrace) {
        stopTrip()
        try {
            hasIssuedReroute = false
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
            seekableDistanceMeters = 0.0
            startFailure = startException
            return
        }
        activeTrace = trace
        playbackScope = scope
        seekableDistanceMeters = routeSimulator?.totalDistanceMeters() ?: 0.0
        isTripActive = true
        isTripPaused = false
        hasArrived = false
        startFailure = null
        tripDeliveryFailure = null
        latestTripState = null
        elapsedSimulatedSeconds = 0.0
        travelledDistanceMeters = 0.0
        driveJob = scope.launch {
            drivePlayback(trace)
        }
    }

    /**
     * Starts guidance on [route] from the device's own fixes: the same topic
     * subscriptions and `start_navigation` call as a simulated trip, without
     * the playback loop. [deliverDeviceFix] feeds each accepted fix through
     * the same delivery path, so the banner, the ETA line and the follow
     * camera all read the controller's own stream. Throws nothing: a start
     * failure surfaces verbatim through [startFailure].
     */
    fun startDeviceTrip(route: Route) {
        stopTrip()
        try {
            hasIssuedReroute = false
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
        elapsedSimulatedSeconds = 0.0
        travelledDistanceMeters = 0.0
        seekableDistanceMeters = 0.0
    }

    /**
     * Subscribes the four navigation topics before starting the guidance
     * session, so the session's initial instruction dispatch is received,
     * then starts the trip at the app's unit system and policies. A recorded
     * trace and the device's fixes share this start path.
     */
    private fun startGuidance(route: Route) {
        activeStepAdvancePolicy = defaultStepAdvancePolicy()
        subscriptions = listOf(
            navigationController.subscribeToNavigationInstructions(listener),
            navigationController.subscribeToDeviationStatus(listener),
            navigationController.subscribeToPhaseChanges(listener),
            navigationController.subscribeToSpeechInstructions(listener),
        )
        navigationController.startNavigation(
            route,
            UnitSystem.METRIC,
            activeStepAdvancePolicy,
            defaultDeviationPolicy(),
        )
    }

    /**
     * One accepted device fix of a device-driven trip, delivered through the
     * same [deliverTripFix] path a simulated fix uses: the controller reports
     * the snapped state the banner and ETA read, and [onSnappedPosition]
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
        deliverTripFix(
            coordinate = coordinate,
            courseHeadingDegrees = courseHeadingDegrees,
            isArrived = false,
            horizontalAccuracyMeters = horizontalAccuracyMeters,
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
     * [ReferenceApplicationConstants.SIMULATED_TRIP_SPEED_MULTIPLIER_CHOICES].
     * A route-following trip rebuilds its walker at the new pace and
     * re-anchors the clock at the position already travelled, so the fix
     * does not jump when the speed changes mid-trip. A refusal stops the
     * playback honestly (the trip is never silently half-driven, §6.3).
     */
    fun selectSpeedMultiplier(multiplier: Double) {
        val isOfferedChoice = ReferenceApplicationConstants.SIMULATED_TRIP_SPEED_MULTIPLIER_CHOICES
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
    }

    /**
     * Moves the presented trip to [targetMeters] along the route's own
     * geometry (M27's seek). The walker supplies the fix, so the camera, the
     * location indicator, the ETA line and the arrival all read exactly what a
     * normal fix at that distance produces.
     *
     * A paused trip stays paused; a running trip continues from the new
     * position. Seeking backwards out of the arrival restarts the navigation
     * session at the seek position, because the contract's reducer ignores
     * fixes once it reports `Arrived` — without the restart the ETA line and
     * guidance would stay frozen at the destination.
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
        val leavesArrival = hasArrived && !targetFix.isArrived
        if (leavesArrival) {
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
        deliverTripFix(
            coordinate = targetFix.coordinate,
            courseHeadingDegrees = targetFix.courseHeadingDegrees,
            isArrived = targetFix.isArrived,
        )
        if (hasArrived) {
            isTripActive = false
            return
        }
        if (driveJob?.isActive != true) {
            // The playback loop ended at the arrival; a backward seek resumes
            // it (paused or running, the loop's own state decides).
            driveJob = playbackScope?.launch {
                drivePlayback(trace)
            }
        }
    }

    /**
     * Stops the replay, cancels the subscriptions, resets the shared
     * collector's topic states (the re-subscription replay rule); the trip
     * state stays as-is.
     */
    fun stopTrip() {
        driveJob?.cancel()
        driveJob = null
        cancelSubscriptions()
        listener.reset()
        isTripActive = false
        isTripPaused = false
        hasArrived = false
        activeTrace = null
        routeSimulator = null
        playbackScope = null
        seekableDistanceMeters = 0.0
        elapsedSimulatedSeconds = 0.0
        travelledDistanceMeters = 0.0
    }

    private fun cancelSubscriptions() {
        subscriptions.forEach { subscription -> subscription.cancel() }
        subscriptions = emptyList()
    }

    /**
     * The playback loop shared by both sources. A route-following trip asks
     * its [RouteSimulator] for a fix at the clock advanced
     * [ReferenceApplicationConstants.DEMO_PLAYBACK_TIME_ACCELERATION_FACTOR]
     * times the tick; a recorded trace delivers its next sample. Pausing
     * keeps the current fix; an arrival ends the loop with [hasArrived] set
     * (the arrival presentation outlives the playback).
     */
    private suspend fun drivePlayback(trace: SimulatedTrace) {
        val traceCursor = TraceCursor(trace.samples.size.toUInt())
        var isFirstWalkFix = true
        while (currentCoroutineContext().isActive) {
            if (tripDeliveryFailure != null) {
                return
            }
            if (hasArrived || listener.phaseChangeState.value == NavigationPhaseChange.ARRIVED) {
                hasArrived = true
                isTripActive = false
                return
            }
            if (isTripPaused) {
                delay(ReferenceApplicationConstants.SIMULATED_TRIP_PAUSED_POLL_INTERVAL_MILLISECONDS)
                continue
            }
            if (trace.followsRouteGeometry) {
                if (!isFirstWalkFix) {
                    elapsedSimulatedSeconds +=
                        ROUTE_WALK_TICK_SECONDS *
                        ReferenceApplicationConstants.DEMO_PLAYBACK_TIME_ACCELERATION_FACTOR
                }
                isFirstWalkFix = false
                val simulator = routeSimulator
                if (simulator == null) {
                    // A route-following trip always builds its walker before
                    // the loop starts; a missing one ends the playback rather
                    // than leaving the trip stuck as "active".
                    isTripActive = false
                    return
                }
                val fix = simulator.fixAtElapsedSeconds(elapsedSimulatedSeconds)
                travelledDistanceMeters = fix.travelledMeters
                deliverTripFix(
                    coordinate = fix.coordinate,
                    courseHeadingDegrees = fix.courseHeadingDegrees,
                    isArrived = fix.isArrived,
                )
                if (hasArrived) {
                    isTripActive = false
                    return
                }
                delay(ReferenceApplicationConstants.ROUTE_WALK_TICK_INTERVAL_MILLISECONDS)
            } else {
                val nextSampleIndex = traceCursor.advance()
                if (nextSampleIndex == null) {
                    // The samples ran out without arrival (a reroute shortened
                    // them): the demo driver stops replaying but keeps the
                    // trip's state.
                    isTripActive = false
                    return
                }
                deliverSample(trace, nextSampleIndex.toInt())
                if (tripDeliveryFailure != null) {
                    return
                }
                if (hasArrived ||
                    listener.phaseChangeState.value == NavigationPhaseChange.ARRIVED
                ) {
                    hasArrived = true
                    isTripActive = false
                    return
                }
                delay(dispatchIntervalMillis(trace.sampleIntervalSeconds))
            }
        }
    }

    /** One recorded sample of the demo replay, with its planar course. */
    private fun deliverSample(trace: SimulatedTrace, sampleIndex: Int) {
        val sample = trace.samples[sampleIndex]
        val previousSample = trace.samples.getOrNull(sampleIndex - 1)
        val sampleCourseHeadingDegrees: Double? = previousSample?.let { previous ->
            headingDegreesFrom(previous, sample)
        }
        deliverTripFix(
            coordinate = GeographicCoordinate(
                latitude = sample.latitudeDegrees,
                longitude = sample.longitudeDegrees,
            ),
            courseHeadingDegrees = sampleCourseHeadingDegrees,
            isArrived = false,
            horizontalAccuracyMeters = sample.horizontalAccuracyMeters,
        )
        if (tripDeliveryFailure == null) {
            maybeIssueReroute(trace)
        }
    }

    /**
     * Feeds one fix to the controller and reports its processed state. A
     * failure stops the replay honestly (§6.3); an arrival sets
     * [hasArrived] whether the walker or the controller's phase declared it.
     */
    private fun deliverTripFix(
        coordinate: GeographicCoordinate,
        courseHeadingDegrees: Double?,
        isArrived: Boolean,
        horizontalAccuracyMeters: Double =
            ReferenceApplicationConstants.SIMULATED_TRACE_HORIZONTAL_ACCURACY_METERS,
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
            onSnappedPosition(
                SimulatedProcessedFix(
                    snappedCoordinate = latestTripState?.snappedCoordinate,
                    courseHeadingDegrees = courseHeadingDegrees,
                    horizontalAccuracyMeters = horizontalAccuracyMeters,
                ),
            )
            if (isArrived || latestTripState?.phase == NavigationPhase.ARRIVED) {
                hasArrived = true
            }
        } catch (deliverException: Exception) {
            // Exception, not SdkException alone: native-internal failures
            // (uniffi InternalException) are no SdkException variant, and a
            // failed fix delivery must stop the replay honestly (§6.3), not
            // crash the demo coroutine.
            isTripActive = false
            latestTripState = null
            tripDeliveryFailure = deliverException
        }
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
            ReferenceApplicationConstants.SIMULATED_TRIP_FALLBACK_SPEED_METERS_PER_SECOND
        }

    /**
     * The reroute policy of the replay (§7.5: a reroute is a normal state
     * transition, never an error): the first fix the trip is judged off
     * route on applies the trace's replacement route — the fixture's
     * recorded reroute route, anchored by construction at the snapped
     * coordinate the reducer holds at that moment. The remaining samples
     * then drive the replacement route.
     */
    private fun maybeIssueReroute(trace: SimulatedTrace) {
        val rerouteRoute = trace.rerouteRoute ?: return
        if (hasIssuedReroute || latestTripState?.phase != NavigationPhase.OFF_ROUTE) {
            return
        }
        hasIssuedReroute = true
        navigationController.applyReroute(rerouteRoute, activeStepAdvancePolicy)
        latestTripState = navigationController.currentTripState()
        onSnappedPosition(
            SimulatedProcessedFix(
                snappedCoordinate = latestTripState?.snappedCoordinate,
                courseHeadingDegrees = null,
                horizontalAccuracyMeters = null,
            ),
        )
    }

    /**
     * The demo dispatch interval in milliseconds for one recorded sample
     * interval, divided by the selected speed multiplier (a faster demo
     * delivers the recorded samples more often; the recorded fixes stay the
     * same). The floor keeps a fast multiplier from spinning the loop.
     */
    internal fun dispatchIntervalMillis(recordedSampleIntervalSeconds: Double): Long =
        traceDispatchIntervalMilliseconds(
            recordedSampleIntervalSeconds,
            ReferenceApplicationConstants.DEMO_PLAYBACK_TIME_ACCELERATION_FACTOR * speedMultiplier,
        ).toLong()

    /**
     * Course of travel between the previous and the current sample, in degrees
     * clockwise from true north (cheap planar bearing — consecutive samples of
     * the walk are meters apart, where the small-angle planar form is exact
     * enough to be indistinguishable from the geodetic one).
     */
    private fun headingDegreesFrom(previousSample: SimulatedTraceSample, sample: SimulatedTraceSample): Double {
        val deltaLongitude = sample.longitudeDegrees - previousSample.longitudeDegrees
        val deltaLatitude = sample.latitudeDegrees - previousSample.latitudeDegrees
        val eastwardDelta = deltaLongitude * cos(sample.latitudeDegrees * DEGREES_TO_RADIANS)
        val heading = atan2(eastwardDelta, deltaLatitude) / DEGREES_TO_RADIANS
        val normalizedHeading = (heading + FULL_CIRCLE_DEGREES) % FULL_CIRCLE_DEGREES
        return normalizedHeading
    }

    private companion object {
        /**
         * One route-following walk tick, in seconds: the clock advances this
         * long (times the demo acceleration) per fix, so the position moves
         * every
         * [ReferenceApplicationConstants.ROUTE_WALK_TICK_INTERVAL_MILLISECONDS].
         */
        val ROUTE_WALK_TICK_SECONDS: Double =
            ReferenceApplicationConstants.ROUTE_WALK_TICK_INTERVAL_MILLISECONDS / 1000.0

        /** Structural math constant: `degrees * DEGREES_TO_RADIANS = radians`. */
        const val DEGREES_TO_RADIANS: Double = kotlin.math.PI / 180.0
        const val FULL_CIRCLE_DEGREES: Double = 360.0
    }
}