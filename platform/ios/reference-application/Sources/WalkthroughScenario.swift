//! The worked example the tutorials publish (`docs/reference/DocumentationSnippets.md`).
//!
//! The same steps run, in the same order, with the same identifiers, on
//! Android, iOS, React Native and WebAssembly: open the map, search the two
//! plan fields, plan the route between them, draw it, start guidance on it,
//! and plan the offline demo request over the committed routing graph. The
//! tutorial chapters regenerate their code blocks from the marked regions
//! below, so the guidance cannot drift from what ships.
//!
//! The `walkthrough` launch argument drives `runWalkthrough`, which is what
//! makes these regions live code rather than documentation that happens to
//! compile.

import Foundation
import MapNativeCoreUI
import RusticMapsSDK

/// The one refusal the walkthrough reports: a plan field that resolved to no
/// place. Nothing is invented, and the caller surfaces it (§6.3).
enum WalkthroughScenarioError: Error {
    /// A search query returned no candidate place.
    case planFieldResolvedNoPlace
}

/// Runs the tutorial's worked example over an already-created SDK and view.
@MainActor
struct WalkthroughScenario {
    /// The SDK the application constructed at startup.
    let mapSdk: MapSdk
    /// The shipped map view the reference application displays.
    let mapView: MapNativeCoreUI.MapView

    /// Runs the whole example and returns the planned route, or nil when
    /// either search query resolved to no place — nothing is invented.
    func runWalkthrough(
        originSearchQuery: String,
        destinationSearchQuery: String,
        routeOptions: RouteOptions,
        offlineArchiveBytes: Data? = nil
    ) async throws -> Route? {
        // docs-snippet: begin first-map/open-the-map
        let mapCamera = MapCameraPose(
            latitudeDegrees: 38.5766,
            longitudeDegrees: -121.4944,
            zoomLevel: 12.0,
            bearingDegrees: 0.0,
            pitchDegrees: 0.0
        )
        mapView.setCamera(mapCamera)
        // docs-snippet: end first-map/open-the-map

        // docs-snippet: begin search-and-routing/search-for-places
        let originResults = try await mapSdk.forwardGeocode(addressText: originSearchQuery)
        let destinationResults = try await mapSdk.forwardGeocode(
            addressText: destinationSearchQuery
        )
        guard
            let originPlace = originResults.first,
            let destinationPlace = destinationResults.first
        else {
            throw WalkthroughScenarioError.planFieldResolvedNoPlace
        }
        // docs-snippet: end search-and-routing/search-for-places

        // docs-snippet: begin search-and-routing/plan-the-route
        let plannedRoute = try await mapSdk.calculateRoute(
            waypoints: [originPlace.coordinate, destinationPlace.coordinate],
            routeOptions: routeOptions
        )
        // docs-snippet: end search-and-routing/plan-the-route

        // docs-snippet: begin search-and-routing/draw-the-route
        let routeOverlay = MapOverlay(
            kind: .routePolyline,
            coordinates: plannedRoute.legs
                .flatMap { leg in leg.steps }
                .flatMap { step in step.shape },
            rgbaColor: nil,
            widthPixels: nil,
            ringCoordinates: nil,
            pointMarkerPositions: nil,
            pointMarkerRadiusPixels: nil,
            trafficFlowSegments: nil
        )
        _ = mapView.setOverlays([routeOverlay])
        // docs-snippet: end search-and-routing/draw-the-route

        let navigator = Navigator()
        try navigator.startNavigation(
            route: plannedRoute,
            unitSystem: .metric,
            stepAdvancePolicy: defaultStepAdvancePolicy(),
            deviationPolicy: defaultDeviationPolicy()
        )
        // The first fix is the route's own first vertex: the walker's first
        // reported position. Real consumers feed their location source here.
        guard let firstRouteCoordinate = plannedRoute.legs.first?
            .steps.first?.shape.first
        else {
            throw WalkthroughScenarioError.planFieldResolvedNoPlace
        }
        navigator.updateLocation(
            locationUpdate: NavigationLocationUpdate(
                coordinate: firstRouteCoordinate,
                horizontalAccuracyMeters: WalkthroughScenario.fixHorizontalAccuracyMeters,
                courseHeadingDegrees: nil,
                timestampSeconds: nil
            )
        )
        _ = navigator.currentTripState()

        if let offlineArchiveBytes {
            // The committed routing-grid archive's straight oneway street, the
            // same request the tutorial and the browser worked example use.
            let offlineRoute = try await offlineRouteFromArchiveBytes(
                archiveBytes: offlineArchiveBytes,
                start: GeographicCoordinate(
                    latitude: WalkthroughScenario.offlineStartLatitudeDegrees,
                    longitude: WalkthroughScenario.offlineStartLongitudeDegrees
                ),
                end: GeographicCoordinate(
                    latitude: WalkthroughScenario.offlineEndLatitudeDegrees,
                    longitude: WalkthroughScenario.offlineEndLongitudeDegrees
                )
            )
            guard !offlineRoute.legs.isEmpty else {
                throw WalkthroughScenarioError.planFieldResolvedNoPlace
            }
        }

        return plannedRoute
    }

    /// The fixed-accuracy envelope the worked example's single fix reports.
    private static let fixHorizontalAccuracyMeters = 5.0
    /// The routing-grid fixture's demo request (its straight oneway street).
    private static let offlineStartLatitudeDegrees = 0.001
    private static let offlineStartLongitudeDegrees = 0.0
    private static let offlineEndLatitudeDegrees = 0.001
    private static let offlineEndLongitudeDegrees = 0.002
}

// MARK: - The application's walkthrough entry point

extension ReferenceApplicationModel {
    /// Runs the tutorial's worked example over the live map: the same steps,
    /// in the same order and with the same identifiers the guides publish.
    ///
    /// This exists so the generated chapters describe code the application
    /// really executes; nothing here is a documentation-only path.
    func runWalkthroughDemo(
        originSearchQuery: String = ReferenceApplicationConfiguration.walkthroughOriginSearchQuery,
        destinationSearchQuery: String =
            ReferenceApplicationConfiguration.walkthroughDestinationSearchQuery
    ) async {
        guard let mapView, let mapSdk else {
            return
        }
        let scenario = WalkthroughScenario(mapSdk: mapSdk, mapView: mapView)
        // The offline step plans over the bundled routing graph; a missing
        // archive skips that step rather than inventing a route.
        let offlineArchiveBytes: Data? = Bundle.main.url(
            forResource: ReferenceApplicationConfiguration.offlineRoutingArchiveResourceName,
            withExtension: ReferenceApplicationConfiguration.offlineRoutingArchiveResourceExtension
        ).flatMap { archiveURL in try? Data(contentsOf: archiveURL) }
        do {
            _ = try await scenario.runWalkthrough(
                originSearchQuery: originSearchQuery,
                destinationSearchQuery: destinationSearchQuery,
                routeOptions: currentRouteOptions(),
                offlineArchiveBytes: offlineArchiveBytes
            )
        } catch let sdkError as SdkError {
            reportSdkError(sdkError)
        } catch {
            // Failing guidance must report, never crash (§6.3).
            reportLocalError(origin: "walkthrough", detail: String(describing: error))
        }
    }
}
