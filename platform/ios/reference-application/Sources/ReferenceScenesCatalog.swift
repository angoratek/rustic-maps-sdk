//! The shared demo-scene artifact (`platform/reference-scenes.json`) as the
//! language-neutral demo content every reference application loads
//! (`docs/ReferenceAppUX.md` §9): the camera presets feed the preset picker,
//! the overlay scenes feed the SDK overlay API and the navigation demo
//! section feeds the simulated trip. Provider descriptors live in the
//! separate shared registry (`ProviderRegistryCatalog`), not this artifact.
//! The artifact's keys are snake_case;
//! the scene kind strings mirror the wasm canvas' plain-object kinds and
//! the contract's `MapOverlayKind` variants pair with them.

import Foundation
import RusticMapsSDK

/// The camera presets of the artifact: the Sacramento demo locations with the
/// full camera each preset frames.
public struct ReferenceSceneCameraPreset: Decodable, Identifiable, Sendable {
    /// The preset identifier (the artifact's `id`).
    public let id: String
    /// The preset's human label.
    public let label: String
    /// The preset's framing note.
    public let note: String
    /// Center latitude in decimal degrees.
    public let latitudeDegrees: Double
    /// Center longitude in decimal degrees.
    public let longitudeDegrees: Double
    /// The camera zoom level.
    public let zoomLevel: Double
    /// The camera bearing in degrees.
    public let bearingDegrees: Double
    /// The camera pitch in degrees.
    public let pitchDegrees: Double

    private enum CodingKeys: String, CodingKey {
        case id, label, note
        case latitudeDegrees = "latitude_degrees"
        case longitudeDegrees = "longitude_degrees"
        case zoomLevel = "zoom_level"
        case bearingDegrees = "bearing_degrees"
        case pitchDegrees = "pitch_degrees"
    }
}

/// One geographic coordinate of a scene payload, in decimal degrees.
public struct ReferenceSceneCoordinate: Decodable, Sendable {
    /// Latitude in decimal degrees.
    public let latitudeDegrees: Double
    /// Longitude in decimal degrees.
    public let longitudeDegrees: Double

    private enum CodingKeys: String, CodingKey {
        case latitudeDegrees = "latitude_degrees"
        case longitudeDegrees = "longitude_degrees"
    }
}

/// One traffic-flow observation of the traffic scene, with its congestion
/// classification (the artifact's congestion-level strings mirror the
/// contract's congestion enum names).
public struct ReferenceSceneTrafficFlowSegment: Decodable, Sendable {
    /// The observed segment shape.
    public let shapeCoordinates: [ReferenceSceneCoordinate]
    /// The congestion classification string (`FreeFlow`/`Slow`/`Congested`/
    /// `Blocked`).
    public let congestionLevel: String

    private enum CodingKeys: String, CodingKey {
        case shapeCoordinates = "shape_coordinates"
        case congestionLevel = "congestion_level"
    }
}

/// One overlay scene of the artifact, keyed by its scene key in the
/// artifact's `overlay_scenes` dictionary (the dictionary key — not a field
/// of the entry — travels with the catalog, not this struct).
public struct ReferenceSceneOverlay: Decodable, Sendable {
    /// The scene's human label.
    public let label: String
    /// The scene's description.
    public let descriptionText: String
    /// The wasm `set_overlays` kind string of the scene.
    public let kind: String
    /// The route polyline's coordinates, route scenes only.
    public let coordinates: [ReferenceSceneCoordinate]?
    /// The filled polygon's ring, polygon scenes only.
    public let ringCoordinates: [ReferenceSceneCoordinate]?
    /// The markers' positions, marker scenes only.
    public let positions: [ReferenceSceneCoordinate]?
    /// The traffic flow observations, traffic scenes only.
    public let segments: [ReferenceSceneTrafficFlowSegment]?

    private enum CodingKeys: String, CodingKey {
        case label
        case descriptionText = "description"
        case kind, coordinates, ringCoordinates = "ring_coordinates"
        case positions, segments
    }
}

/// The navigation demo section of the artifact: which recorded synthetic
/// trace drives the simulated trip, and its provenance.
public struct ReferenceSceneNavigationDemo: Decodable, Sendable {
    /// The simulated trace reference (§9).
    public let simulatedTrace: SimulatedTrace

    private enum CodingKeys: String, CodingKey {
        case simulatedTrace = "simulated_trace"
    }

    /// The recorded synthetic trace the simulated trip replays.
    public struct SimulatedTrace: Decodable, Sendable {
        /// The trace's name.
        public let name: String
        /// The trace's fixture path, relative to the repository root.
        public let fixturePath: String
        /// The recorded sample interval in seconds.
        public let sampleIntervalSeconds: Double
        /// The trace's provenance note.
        public let provenance: String

        private enum CodingKeys: String, CodingKey {
            case name, provenance
            case fixturePath = "fixture_path"
            case sampleIntervalSeconds = "sample_interval_seconds"
        }
    }
}

/// One recorded demo plan of the artifact's `demo_plans` section: the
/// consumer-facing label and info-affordance description beside the trace
/// the plan replays (the rows name the feature, never the seam).
public struct ReferenceSceneDemoPlan: Decodable, Sendable {
    /// The fixture metadata's own `scenario_name`.
    public let scenario: String
    /// The fixture path, relative to the repository root.
    public let fixturePath: String
    /// The consumer-facing plan name (the chip's label).
    public let label: String
    /// The description behind the plan's info affordance.
    public let description: String

    private enum CodingKeys: String, CodingKey {
        case scenario, label, description
        case fixturePath = "fixture_path"
    }

    /// The bundled asset name the plan replays (the fixture path's basename).
    public var traceAssetName: String {
        (fixturePath as NSString).lastPathComponent
    }
}

/// One keyless demo place of the artifact's `demo_places` section: the
/// consumer name and note the plan search offers when no provider credential
/// is applied, plus its coordinate.
public struct ReferenceSceneDemoPlace: Decodable, Sendable {
    /// The place's consumer name (also its result row's display text).
    public let name: String
    /// The note shown under the name (the artifact's demo content copy).
    public let note: String
    /// Latitude in decimal degrees.
    public let latitudeDegrees: Double
    /// Longitude in decimal degrees.
    public let longitudeDegrees: Double

    private enum CodingKeys: String, CodingKey {
        case name, note
        case latitudeDegrees = "latitude_degrees"
        case longitudeDegrees = "longitude_degrees"
    }

    /// The place's resolved coordinate.
    public var coordinate: GeographicCoordinate {
        GeographicCoordinate(latitude: latitudeDegrees, longitude: longitudeDegrees)
    }
}

/// One offline region of the artifact's `offline_regions` section: the
/// identity, area, zoom range and tile form the reference application's
/// offline card offers for download.
public struct ReferenceSceneOfflineRegion: Decodable, Sendable {
    /// Stable identifier the region is stored and managed under.
    public let regionId: String
    /// Human-readable region name.
    public let displayName: String
    /// The region's geographic bounds.
    public let boundingBox: ReferenceSceneBoundingBoxRecord
    /// Lowest zoom level the region downloads.
    public let minimumZoomLevel: UInt8
    /// Highest zoom level the region downloads.
    public let maximumZoomLevel: UInt8
    /// Encoded format the stored tile payloads carry (the contract enum's
    /// own case name, e.g. `Png`).
    public let imageFormat: String
    /// Pixel resolution the stored tile payloads carry (the contract enum's
    /// own case name, e.g. `Standard`).
    public let tileResolution: String
    /// Requested retention in seconds; null takes the category default.
    public let timeToLiveSeconds: UInt64?
    /// The artifact's demo note (why this region and its values).
    public let note: String?

    private enum CodingKeys: String, CodingKey {
        case note
        case regionId = "region_id"
        case displayName = "display_name"
        case boundingBox = "bounding_box"
        case minimumZoomLevel = "minimum_zoom_level"
        case maximumZoomLevel = "maximum_zoom_level"
        case imageFormat = "image_format"
        case tileResolution = "tile_resolution"
        case timeToLiveSeconds = "time_to_live_seconds"
    }
}

/// One recorded route of the artifact's `demo_routes` section: the ordered
/// demo-place pair it answers (by name) and the route geometry the keyless
/// plan presents — never invented, always the artifact's own record.
public struct ReferenceSceneDemoRoute: Decodable, Sendable {
    /// The origin demo place's name.
    public let originName: String
    /// The destination demo place's name.
    public let destinationName: String
    /// The recorded origin coordinate.
    public let origin: ReferenceSceneRouteCoordinate
    /// The recorded destination coordinate.
    public let destination: ReferenceSceneRouteCoordinate
    /// The recorded route geometry, decoded into the contract's record.
    public let route: ReferenceSceneRouteRecord

    private enum CodingKeys: String, CodingKey {
        case origin, destination, route
        case originName = "origin_name"
        case destinationName = "destination_name"
    }
}

/// One recorded route's geometry as the artifact serializes it (snake_case
/// keys) and its conversion into the contract's `Route` record, so the app
/// draws and drives a recorded route exactly as it does a live one.
public struct ReferenceSceneRouteRecord: Decodable, Sendable {
    /// Total distance in meters.
    public let distanceMeters: Double
    /// Total travel time in seconds.
    public let durationSeconds: Double
    /// Geographic bounds of the full route geometry.
    public let boundingBox: ReferenceSceneBoundingBoxRecord
    /// Legs, one per consecutive waypoint pair.
    public let legs: [ReferenceSceneRouteLegRecord]

    private enum CodingKeys: String, CodingKey {
        case distanceMeters = "distance_meters"
        case durationSeconds = "duration_seconds"
        case boundingBox = "bounding_box"
        case legs
    }

    /// The contract's route record built from the recorded geometry.
    public var contractRoute: Route {
        Route(
            distanceMeters: distanceMeters,
            durationSeconds: durationSeconds,
            boundingBox: BoundingBox(
                southWest: boundingBox.southWest.coordinate,
                northEast: boundingBox.northEast.coordinate
            ),
            // The artifact records per-step shapes, so the route-level
            // geometry is empty (the contract's documented shape for it).
            geometry: [],
            legs: legs.map(\.contractLeg)
        )
    }
}

/// One coordinate of the recorded route geometry. The route record uses the
/// contract fixture's own `latitude`/`longitude` keys, unlike `demo_places`'
/// degrees-suffixed keys.
public struct ReferenceSceneRouteCoordinate: Decodable, Sendable {
    /// Latitude in decimal degrees.
    public let latitude: Double
    /// Longitude in decimal degrees.
    public let longitude: Double

    /// The contract's coordinate record.
    public var coordinate: GeographicCoordinate {
        GeographicCoordinate(latitude: latitude, longitude: longitude)
    }
}

/// One recorded route's geographic bounds.
public struct ReferenceSceneBoundingBoxRecord: Decodable, Sendable {
    /// South-west corner (minimum latitude and longitude).
    public let southWest: ReferenceSceneRouteCoordinate
    /// North-east corner (maximum latitude and longitude).
    public let northEast: ReferenceSceneRouteCoordinate

    private enum CodingKeys: String, CodingKey {
        case southWest = "south_west"
        case northEast = "north_east"
    }
}

/// One recorded route leg.
public struct ReferenceSceneRouteLegRecord: Decodable, Sendable {
    /// Leg distance in meters.
    public let distanceMeters: Double
    /// Leg travel time in seconds.
    public let durationSeconds: Double
    /// Maneuver steps in traversal order.
    public let steps: [ReferenceSceneRouteStepRecord]

    private enum CodingKeys: String, CodingKey {
        case distanceMeters = "distance_meters"
        case durationSeconds = "duration_seconds"
        case steps
    }

    /// The contract's leg record built from the recorded geometry.
    public var contractLeg: RouteLeg {
        RouteLeg(
            distanceMeters: distanceMeters,
            durationSeconds: durationSeconds,
            steps: steps.map(\.contractStep)
        )
    }
}

/// One recorded route maneuver step.
public struct ReferenceSceneRouteStepRecord: Decodable, Sendable {
    /// Step distance in meters.
    public let distanceMeters: Double
    /// Step travel time in seconds.
    public let durationSeconds: Double
    /// The contract's stable maneuver label.
    public let maneuverType: String
    /// Human-readable instruction in the display language.
    public let instructionDisplayText: String
    /// Instruction tuned for text-to-speech engines.
    public let instructionSpeechText: String
    /// Step geometry in traversal order.
    public let shape: [ReferenceSceneRouteCoordinate]

    private enum CodingKeys: String, CodingKey {
        case distanceMeters = "distance_meters"
        case durationSeconds = "duration_seconds"
        case maneuverType = "maneuver_type"
        case instructionDisplayText = "instruction_display_text"
        case instructionSpeechText = "instruction_speech_text"
        case shape
    }

    /// The contract's step record built from the recorded geometry.
    public var contractStep: RouteStep {
        RouteStep(
            distanceMeters: distanceMeters,
            durationSeconds: durationSeconds,
            maneuverType: maneuverType,
            instructionDisplayText: instructionDisplayText,
            instructionSpeechText: instructionSpeechText,
            shape: shape.map(\.coordinate)
        )
    }
}

/// The decoded shared demo artifact. Loaded from the package's processed
/// bundle resource (the artifact is copied from the single source at
/// packaging time by `scripts/build-ios.sh`, so the data cannot drift).
public struct ReferenceScenesCatalog: Decodable, Sendable {
    /// The artifact's declaration block (name, version, purpose).
    public let artifact: ArtifactHeader
    /// The Sacramento demo camera presets, in artifact order.
    public let cameraPresets: [ReferenceSceneCameraPreset]
    /// The overlay scenes keyed by their scene keys.
    public let overlayScenes: [String: ReferenceSceneOverlay]
    /// The navigation demo section.
    public let navigationDemo: ReferenceSceneNavigationDemo
    /// The recorded demo plans the picker offers, in artifact order.
    public let demoPlans: [ReferenceSceneDemoPlan]
    /// The keyless search's demo places, in artifact order.
    public let demoPlaces: [ReferenceSceneDemoPlace]
    /// The keyless plan's recorded routes, one per ordered demo-place pair.
    public let demoRoutes: [ReferenceSceneDemoRoute]
    /// The offline card's demo regions, in artifact order.
    public let offlineRegions: [ReferenceSceneOfflineRegion]

    private enum CodingKeys: String, CodingKey {
        case artifact, cameraPresets = "camera_presets"
        case overlayScenes = "overlay_scenes"
        case navigationDemo = "navigation_demo"
        case demoPlans = "demo_plans"
        case demoPlaces = "demo_places"
        case demoRoutes = "demo_routes"
        case offlineRegions = "offline_regions"
    }

    /// The artifact's declaration block.
    public struct ArtifactHeader: Decodable, Sendable {
        /// The artifact name.
        public let name: String
        /// The artifact version.
        public let version: Int
        /// The artifact purpose note.
        public let purpose: String
    }
}