//! The shared demo artifact's native half on iOS (docs/ReferenceAppUX.md §9):
//! the `platform/reference-scenes.json` file the app bundle carries (the
//! same single committed source the Android shell's Gradle copy task and the
//! web loader consume, so the bundled demo data cannot drift), decoded with
//! the snake_case keys consumed as-is (nothing rekeyed, mirroring the
//! Compose reference application's `ReferenceScenesLoader` and the Android
//! RN shell's loader).

import Foundation
import MapsSdkRn

/// The loaded artifact record the controller consumes: the sections the shell
/// needs (the Kotlin `RefappScenesArtifact` mirror), including the keyless
/// demo places and recorded routes (§4.1's keyless search and plan).
struct RefappScenesArtifact {
    /// The artifact's demo position (the first camera preset, §4.2's origin
    /// default).
    let demoPosition: GeographicCoordinate
    /// The overlay scenes by presentation order (§4.1's chip carousel rows).
    let overlayScenes: [String: RefappOverlayScene]
    /// The application-asset file name of the navigation demo's recorded
    /// trace.
    let traceAssetName: String
    /// The keyless search's demo places, in artifact order.
    let demoPlaces: [RefappDemoPlace]
    /// The keyless plan's recorded routes, one per ordered demo-place pair.
    let demoRoutes: [RefappDemoRoute]
}

/// One keyless demo place of the artifact's `demo_places` section: the
/// consumer name and note the React shell's search offers when no provider
/// credential is applied.
struct RefappDemoPlace {
    let name: String
    let note: String
    let latitudeDegrees: Double
    let longitudeDegrees: Double
}

/// One recorded route of the artifact's `demo_routes` section: the ordered
/// demo-place pair it answers and the contract's `Route` record the keyless
/// plan applies (never invented geometry).
struct RefappDemoRoute {
    let originName: String
    let destinationName: String
    let origin: GeographicCoordinate
    let destination: GeographicCoordinate
    let route: Route
}

/// One overlay scene of the shared artifact, holding exactly one payload form.
struct RefappOverlayScene {
    let sceneKey: String
    let coordinates: [GeographicCoordinate]?
    let ringCoordinates: [GeographicCoordinate]?
    let positions: [GeographicCoordinate]?
    let trafficFlowSegments: [TrafficFlowData]?
}

/// Loads the shared demo artifact from the application bundle (the resources
/// build phase's copied files).
enum RefappScenesLoader {
    static let referenceScenesResourceName = "reference-scenes"
    static let referenceScenesResourceFileExtension = "json"
    static let traceAssetFileSuffix = ".json"

    private enum ArtifactKeys {
        static let cameraPresets = "camera_presets"
        static let overlayScenes = "overlay_scenes"
        static let navigationDemo = "navigation_demo"
        static let simulatedTrace = "simulated_trace"
        static let name = "name"
        static let latitudeDegrees = "latitude_degrees"
        static let longitudeDegrees = "longitude_degrees"
        static let coordinates = "coordinates"
        static let ringCoordinates = "ring_coordinates"
        static let positions = "positions"
        static let segments = "segments"
        static let congestionLevel = "congestion_level"
        static let shapeCoordinates = "shape_coordinates"
        static let demoPlaces = "demo_places"
        static let demoRoutes = "demo_routes"
        static let note = "note"
        static let originName = "origin_name"
        static let destinationName = "destination_name"
        static let origin = "origin"
        static let destination = "destination"
        static let route = "route"
    }

    /// Decodes the artifact (the loader's only entry point): a malformed or
    /// missing artifact throws verbatim — the caller reports it, never
    /// guessing a placeholder scene.
    static func load() throws -> RefappScenesArtifact {
        guard let artifactURL = Bundle.main.url(
            forResource: referenceScenesResourceName,
            withExtension: referenceScenesResourceFileExtension
        ) else {
            throw RefappArtifactError(
                detailText: "the \(referenceScenesResourceName).\(referenceScenesResourceFileExtension) demo artifact was not found in the app bundle"
            )
        }
        let artifactData = try Data(contentsOf: artifactURL)
        guard let artifactObject = try JSONSerialization.jsonObject(
            with: artifactData, options: .fragmentsAllowed
        ) as? [String: Any] else {
            throw RefappArtifactError(
                detailText: "the \(referenceScenesResourceName) demo artifact is not a JSON object"
            )
        }
        guard let cameraPresets = artifactObject[ArtifactKeys.cameraPresets] as? [[String: Any]],
              let firstPresetPosition = cameraPresets.first,
              let presetLatitude = firstPresetPosition[ArtifactKeys.latitudeDegrees] as? Double,
              let presetLongitude = firstPresetPosition[ArtifactKeys.longitudeDegrees] as? Double
        else {
            throw RefappArtifactError(
                detailText: "the \(referenceScenesResourceName) demo artifact carries no camera preset position"
            )
        }
        guard let navigationDemo = artifactObject[ArtifactKeys.navigationDemo] as? [String: Any],
              let simulatedTrace = navigationDemo[ArtifactKeys.simulatedTrace] as? [String: Any],
              let navigationTraceName = simulatedTrace[ArtifactKeys.name] as? String
        else {
            throw RefappArtifactError(
                detailText: "the \(referenceScenesResourceName) demo artifact carries no navigation demo trace name"
            )
        }
        let overlaySceneObject = artifactObject[ArtifactKeys.overlayScenes] as? [String: Any] ?? [:]
        var overlayScenes: [String: RefappOverlayScene] = [:]
        for sceneKey in RefappConstants.overlaySceneKeys {
            guard let sceneObject = overlaySceneObject[sceneKey] as? [String: Any] else {
                continue
            }
            overlayScenes[sceneKey] = RefappOverlayScene(
                sceneKey: sceneKey,
                coordinates: decodeOptionalCoordinateList(
                    sceneObject[ArtifactKeys.coordinates]
                ),
                ringCoordinates: decodeOptionalCoordinateList(
                    sceneObject[ArtifactKeys.ringCoordinates]
                ),
                positions: decodeOptionalCoordinateList(
                    sceneObject[ArtifactKeys.positions]
                ),
                trafficFlowSegments: try decodeOptionalTrafficSegmentList(
                    sceneObject[ArtifactKeys.segments]
                ),
            )
        }
        return RefappScenesArtifact(
            demoPosition: GeographicCoordinate(
                latitude: presetLatitude,
                longitude: presetLongitude
            ),
            overlayScenes: overlayScenes,
            traceAssetName: navigationTraceName + traceAssetFileSuffix,
            demoPlaces: decodeDemoPlaces(artifactObject[ArtifactKeys.demoPlaces]),
            demoRoutes: try decodeDemoRoutes(artifactObject[ArtifactKeys.demoRoutes])
        )
    }

    /// One artifact decode failure carrying the loader's verbatim detail (§6.3).
    struct RefappArtifactError: LocalizedError, Swift.Error {
        let detailText: String
        var errorDescription: String? { detailText }
    }

    // ------------------------------------------------------------------
    // Section decoders (the snake_case keys consumed as-is)
    // ------------------------------------------------------------------

    /// Decodes the artifact's demo places (the keyless search's rows).
    private static func decodeDemoPlaces(_ payload: Any?) -> [RefappDemoPlace] {
        guard let placeObjects = payload as? [[String: Any]] else {
            return []
        }
        return placeObjects.compactMap { placeObject in
            guard let name = placeObject[ArtifactKeys.name] as? String,
                  let note = placeObject[ArtifactKeys.note] as? String,
                  let latitudeDegrees = placeObject[ArtifactKeys.latitudeDegrees] as? Double,
                  let longitudeDegrees = placeObject[ArtifactKeys.longitudeDegrees] as? Double
            else {
                return nil
            }
            return RefappDemoPlace(
                name: name,
                note: note,
                latitudeDegrees: latitudeDegrees,
                longitudeDegrees: longitudeDegrees
            )
        }
    }

    /// Decodes the artifact's recorded routes (the keyless plan's geometry).
    private static func decodeDemoRoutes(_ payload: Any?) throws -> [RefappDemoRoute] {
        guard let routeObjects = payload as? [[String: Any]] else {
            return []
        }
        return try routeObjects.map { demoRouteObject in
            guard let originName = demoRouteObject[ArtifactKeys.originName] as? String,
                  let destinationName =
                    demoRouteObject[ArtifactKeys.destinationName] as? String,
                  let originObject =
                    demoRouteObject[ArtifactKeys.origin] as? [String: Any],
                  let destinationObject =
                    demoRouteObject[ArtifactKeys.destination] as? [String: Any],
                  let routeObject = demoRouteObject[ArtifactKeys.route] as? [String: Any]
            else {
                throw RefappArtifactError(
                    detailText: "the demo route record does not carry the recorded pair"
                )
            }
            guard let origin = decodeRouteCoordinateObject(originObject),
                  let destination = decodeRouteCoordinateObject(destinationObject)
            else {
                throw RefappArtifactError(
                    detailText: "the demo route record does not carry recorded coordinates"
                )
            }
            return RefappDemoRoute(
                originName: originName,
                destinationName: destinationName,
                origin: origin,
                destination: destination,
                route: try RefappTraceLoader.decodeRoute(routeObject)
            )
        }
    }

    /// Decodes one recorded-route coordinate object, or nil when malformed.
    /// The route record uses the contract fixture's own `latitude`/`longitude`
    /// keys, unlike `demo_places`' degrees-suffixed keys.
    private static func decodeRouteCoordinateObject(
        _ coordinateObject: [String: Any]
    ) -> GeographicCoordinate? {
        guard let latitude = coordinateObject["latitude"] as? Double,
              let longitude = coordinateObject["longitude"] as? Double
        else {
            return nil
        }
        return GeographicCoordinate(latitude: latitude, longitude: longitude)
    }

    private static func decodeOptionalCoordinateList(
        _ payload: Any?
    ) -> [GeographicCoordinate]? {
        guard let coordinateObjects = payload as? [[String: Any]] else {
            return nil
        }
        return coordinateObjects.compactMap { coordinateObject in
            guard let latitudeDegrees = coordinateObject[ArtifactKeys.latitudeDegrees] as? Double,
                  let longitudeDegrees = coordinateObject[ArtifactKeys.longitudeDegrees] as? Double
            else {
                return nil
            }
            return GeographicCoordinate(
                latitude: latitudeDegrees,
                longitude: longitudeDegrees
            )
        }
    }

    /// Decodes the traffic segments: the congestion-level label matching is
    /// normalized (the Compose application's helper) so the artifact's
    /// `FreeFlow` spelling meets the contract enum's `FREE_FLOW` spelling.
    private static func decodeOptionalTrafficSegmentList(
        _ payload: Any?
    ) throws -> [TrafficFlowData]? {
        guard let segmentObjects = payload as? [[String: Any]] else {
            return nil
        }
        return try segmentObjects.compactMap { segmentObject in
            guard let shapeObjects = segmentObject[ArtifactKeys.shapeCoordinates] as? [[String: Any]],
                  let congestionLevelText = segmentObject[ArtifactKeys.congestionLevel] as? String
            else {
                return nil
            }
            let shapeCoordinates: [GeographicCoordinate] = shapeObjects.compactMap { coordinateObject in
                guard let latitudeDegrees = coordinateObject[ArtifactKeys.latitudeDegrees] as? Double,
                      let longitudeDegrees = coordinateObject[ArtifactKeys.longitudeDegrees] as? Double
                else {
                    return nil
                }
                return GeographicCoordinate(
                    latitude: latitudeDegrees,
                    longitude: longitudeDegrees
                )
            }
            return TrafficFlowData(
                shapeCoordinates: shapeCoordinates,
                currentSpeedKilometersPerHour: nil,
                freeFlowSpeedKilometersPerHour: nil,
                congestionLevel: try congestionLevelOf(congestionLevelText)
            )
        }
    }

    static func normalizedCongestionLevelText(_ congestionLevelText: String) -> String {
        congestionLevelText
            .lowercased()
            .replacingOccurrences(of: "_", with: "")
            .replacingOccurrences(of: "-", with: "")
    }

    static func congestionLevelOf(_ congestionLevelText: String) throws -> TrafficFlowCongestionLevel {
        // The contract enum's case spellings, mirrored explicitly (the
        // generated Swift enum carries no `CaseIterable` conformance).
        let knownLevels: [TrafficFlowCongestionLevel] = [
            .freeFlow,
            .slow,
            .congested,
            .blocked,
        ]
        for levelCase in knownLevels {
            if normalizedCongestionLevelText("\(levelCase)") == normalizedCongestionLevelText(congestionLevelText) {
                return levelCase
            }
        }
        throw RefappArtifactError(
            detailText: "traffic congestion level '\(congestionLevelText)' matches no known level"
        )
    }
}