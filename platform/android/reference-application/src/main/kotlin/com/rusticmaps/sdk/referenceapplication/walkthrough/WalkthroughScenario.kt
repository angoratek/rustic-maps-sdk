package com.rusticmaps.sdk.referenceapplication.walkthrough

import com.rusticmaps.sdk.MapCameraPose
import com.rusticmaps.sdk.MapView
import uniffi.rustic_maps.AvoidOptions
import uniffi.rustic_maps.GeographicCoordinate
import uniffi.rustic_maps.MapOverlay
import uniffi.rustic_maps.MapOverlayKind
import uniffi.rustic_maps.MapSdk
import uniffi.rustic_maps.NavigationLocationUpdate
import uniffi.rustic_maps.Navigator
import uniffi.rustic_maps.Place
import uniffi.rustic_maps.Route
import uniffi.rustic_maps.RouteOptions
import uniffi.rustic_maps.RouteStrategy
import uniffi.rustic_maps.TransportationMode
import uniffi.rustic_maps.UnitSystem
import uniffi.rustic_maps.defaultDeviationPolicy
import uniffi.rustic_maps.defaultStepAdvancePolicy
import uniffi.rustic_maps.offlineRouteFromArchiveBytes

/**
 * The worked example the tutorials publish (`docs/reference/DocumentationSnippets.md`).
 *
 * The same steps run, in the same order, with the same identifiers, on
 * Android, iOS, React Native and WebAssembly: open the map, search the two
 * plan fields, plan the route between them, draw it, start guidance on it,
 * and plan the offline demo request over the committed routing graph. The
 * tutorial chapters regenerate their code blocks from the marked regions
 * below, so the guidance cannot drift from what ships.
 *
 * The launch automation drives [runWalkthrough] with the `demoWalkthrough`
 * intent extra, which is what makes these regions live code rather than
 * documentation that happens to compile.
 */
internal class WalkthroughScenario(
    private val mapSdk: MapSdk,
    private val mapView: MapView,
) {
    /**
     * Runs the whole example over an already-created SDK and view.
     *
     * [transportationMode] and [routeStrategy] are the Directions control's
     * selected choices, so the worked example plans with the same mode every
     * other plan in the application uses; their defaults keep the standalone
     * snippet readable.
     *
     * Returns the planned route, or null when either search query resolved to
     * no place — nothing is invented, and the caller reports the refusal.
     */
    suspend fun runWalkthrough(
        originSearchQuery: String,
        destinationSearchQuery: String,
        offlineArchiveBytes: ByteArray? = null,
        transportationMode: TransportationMode = TransportationMode.AUTOMOBILE,
        routeStrategy: RouteStrategy = RouteStrategy.FASTEST,
    ): Route? {
        // docs-snippet: begin first-map/open-the-map
        val mapCamera = MapCameraPose(
            latitudeDegrees = 38.5766,
            longitudeDegrees = -121.4944,
            zoomLevel = 12.0,
            bearingDegrees = 0.0,
            pitchDegrees = 0.0,
        )
        mapView.setCamera(mapCamera)
        // docs-snippet: end first-map/open-the-map

        // docs-snippet: begin search-and-routing/search-for-places
        val originPlace: Place = mapSdk.forwardGeocode(originSearchQuery).firstOrNull()
            ?: return null
        val destinationPlace: Place = mapSdk.forwardGeocode(destinationSearchQuery).firstOrNull()
            ?: return null
        // docs-snippet: end search-and-routing/search-for-places

        // docs-snippet: begin search-and-routing/plan-the-route
        val routeOptions = RouteOptions(
            transportationMode = transportationMode,
            routeStrategy = routeStrategy,
            unitSystem = UnitSystem.METRIC,
            avoidOptions = AvoidOptions(
                tollRoads = false,
                limitedAccessHighways = false,
                ferries = false,
                unpavedRoads = false,
            ),
            departureTimestampSeconds = null,
        )
        val plannedRoute = mapSdk.calculateRoute(
            waypoints = listOf(originPlace.coordinate, destinationPlace.coordinate),
            routeOptions = routeOptions,
        )
        // docs-snippet: end search-and-routing/plan-the-route

        // docs-snippet: begin search-and-routing/draw-the-route
        val routeOverlay = MapOverlay(
            kind = MapOverlayKind.ROUTE_POLYLINE,
            coordinates = plannedRoute.legs
                .flatMap { leg -> leg.steps }
                .flatMap { step -> step.shape },
            rgbaColor = null,
            widthPixels = null,
            ringCoordinates = null,
            pointMarkerPositions = null,
            pointMarkerRadiusPixels = null,
            trafficFlowSegments = null,
        )
        mapView.setOverlays(listOf(routeOverlay))
        // docs-snippet: end search-and-routing/draw-the-route

        val navigator = Navigator()
        navigator.startNavigation(
            route = plannedRoute,
            unitSystem = UnitSystem.METRIC,
            stepAdvancePolicy = defaultStepAdvancePolicy(),
            deviationPolicy = defaultDeviationPolicy(),
        )
        // The first fix is the route's own first vertex: the walker's first
        // reported position. Real consumers feed their location source here.
        val firstRouteCoordinate = plannedRoute.legs.first().steps.first().shape.first()
        navigator.updateLocation(
            NavigationLocationUpdate(
                coordinate = firstRouteCoordinate,
                horizontalAccuracyMeters = WALKTHROUGH_FIX_HORIZONTAL_ACCURACY_METERS,
                courseHeadingDegrees = null,
                timestampSeconds = null,
            ),
        )
        val tripState = navigator.currentTripState()

        if (offlineArchiveBytes != null) {
            // The committed routing-grid archive's straight oneway street, the
            // same request the tutorial and the browser worked example use.
            val offlineRoute = offlineRouteFromArchiveBytes(
                archiveBytes = offlineArchiveBytes,
                start = GeographicCoordinate(
                    latitude = WALKTHROUGH_OFFLINE_START_LATITUDE_DEGREES,
                    longitude = WALKTHROUGH_OFFLINE_START_LONGITUDE_DEGREES,
                ),
                end = GeographicCoordinate(
                    latitude = WALKTHROUGH_OFFLINE_END_LATITUDE_DEGREES,
                    longitude = WALKTHROUGH_OFFLINE_END_LONGITUDE_DEGREES,
                ),
            )
            require(offlineRoute.legs.isNotEmpty()) {
                "the offline demo request must plan at least one leg"
            }
        }

        requireNotNull(tripState) { "starting navigation must produce a trip state" }
        return plannedRoute
    }

    private companion object {
        /** The fixed-accuracy envelope the worked example's single fix reports. */
        const val WALKTHROUGH_FIX_HORIZONTAL_ACCURACY_METERS: Double = 5.0

        /** The routing-grid fixture's demo request (its straight oneway street). */
        const val WALKTHROUGH_OFFLINE_START_LATITUDE_DEGREES: Double = 0.001
        const val WALKTHROUGH_OFFLINE_START_LONGITUDE_DEGREES: Double = 0.0
        const val WALKTHROUGH_OFFLINE_END_LATITUDE_DEGREES: Double = 0.001
        const val WALKTHROUGH_OFFLINE_END_LONGITUDE_DEGREES: Double = 0.002
    }
}
