//! Loading the shared demo artifact from a reference application's bundle
//! resource and building SDK overlay descriptors from its scenes
//! (`docs/ReferenceAppUX.md` §9 consumption rules).

import Foundation
import RusticMapsSDK

public extension ReferenceScenesCatalog {
    /// Decoding failures of the shared artifact, carrying the artifact's
    /// own naming so an application can present the failure honestly.
    enum ReferenceScenesCatalogError: Swift.Error, LocalizedError {
        /// The catalog resource was not found in the loaded bundle.
        case catalogResourceMissing(bundleDescription: String)
        /// The catalog JSON did not decode (the decoder's own failure
        /// detail is echoed verbatim).
        case catalogDecodeFailed(underlying: String)

        public var errorDescription: String? {
            switch self {
            case let .catalogResourceMissing(bundleDescription):
                return "the reference-scenes.json demo artifact was not found in \(bundleDescription)"
            case let .catalogDecodeFailed(underlying):
                return "the reference-scenes.json demo artifact failed to decode: \(underlying)"
            }
        }
    }

    /// The name the artifact ships under in the application bundle.
    static let catalogResourceName = "reference-scenes"

    /// The name the artifact file ships under in the application bundle.
    static let catalogResourceFileExtension = "json"

    /// Loads the shared demo artifact from the application bundle. The
    /// reference application bundles the artifact straight from its single
    /// source (`platform/reference-scenes.json`), so every app presents
    /// identical demo data without a copy in the published SDK package.
    static func loadedFromApplicationBundle() throws -> ReferenceScenesCatalog {
        try loaded(from: .main)
    }

    /// Loads the shared demo artifact from the supplied bundle. A bundle is
    /// explicit so a test bundle can carry the artifact without borrowing
    /// the application's.
    static func loaded(from bundle: Bundle) throws -> ReferenceScenesCatalog {
        guard let catalogURL = bundle.url(
            forResource: catalogResourceName,
            withExtension: catalogResourceFileExtension
        ) else {
            throw ReferenceScenesCatalogError.catalogResourceMissing(
                bundleDescription: bundle.bundleIdentifier ?? "an unnamed bundle"
            )
        }
        do {
            let catalogData = try Data(contentsOf: catalogURL)
            return try decoded(from: catalogData)
        } catch {
            throw ReferenceScenesCatalogError.catalogDecodeFailed(underlying: String(describing: error))
        }
    }

    /// Decodes the artifact's JSON (the structs carry the artifact's
    /// snake_case keys explicitly in their CodingKeys — no decoding
    /// strategy that could collide with declared keys).
    static func decoded(from data: Data) throws -> ReferenceScenesCatalog {
        let decoder = JSONDecoder()
        return try decoder.decode(ReferenceScenesCatalog.self, from: data)
    }
}

public extension ReferenceSceneOverlay {
    /// The overlay kind strings the artifact's `kind` field carries (the
    /// wasm canvas' plain-object kinds); the pairing to the contract's
    /// `MapOverlayKind` variants is by this table, not by string guessing.
    enum SceneKind: String, CaseIterable, Sendable {
        /// The route polyline scene kind.
        case routePolyline
        /// The filled polygon scene kind.
        case filledPolygon
        /// The point markers scene kind.
        case pointMarkers
        /// The traffic flow scene kind.
        case trafficFlow
    }

    /// The decoded scene kind, or nil when the artifact declares a kind
    /// this pairing does not know.
    var sceneKind: SceneKind? {
        SceneKind(rawValue: kind)
    }
}

public extension ReferenceSceneCoordinate {
    /// The contract's geographic coordinate record for this scene
    /// coordinate.
    var sdkCoordinate: GeographicCoordinate {
        GeographicCoordinate(latitude: latitudeDegrees, longitude: longitudeDegrees)
    }
}

public extension ReferenceSceneOverlay {
    /// The contract's overlay descriptor for this scene, ready for
    /// `MapView.setOverlays`. Null contract style fields keep the
    /// SDK's documented per-kind default styles (the design language makes
    /// in-surface paints renderer constants, not UX tokens). Throws when
    /// the artifact declares a kind this pairing does not know, or when the
    /// scene's kind-required payload field is absent — the SDK validates
    /// the whole list at `setOverlays` time but the honest application
    /// surfaces a scene that cannot convert instead of waiting for the
    /// SDK's all-or-nothing refusal to name it.
    func sdkOverlayDescriptor() throws -> MapOverlay {
        guard let decodedSceneKind = sceneKind else {
            throw ReferenceScenesCatalogOverlayConversionError.unknownSceneKind(kind)
        }
        switch decodedSceneKind {
        case .routePolyline:
            guard let sceneCoordinates = coordinates else {
                throw ReferenceScenesCatalogOverlayConversionError.missingKindPayload(
                    kindText: kind, missingField: "coordinates"
                )
            }
            return MapOverlay(
                kind: .routePolyline,
                coordinates: sceneCoordinates.map(\.sdkCoordinate),
                rgbaColor: nil,
                widthPixels: nil,
                ringCoordinates: nil,
                pointMarkerPositions: nil,
                pointMarkerRadiusPixels: nil,
                trafficFlowSegments: nil
            )
        case .filledPolygon:
            guard let sceneRing = ringCoordinates else {
                throw ReferenceScenesCatalogOverlayConversionError.missingKindPayload(
                    kindText: kind, missingField: "ring_coordinates"
                )
            }
            return MapOverlay(
                kind: .filledPolygon,
                coordinates: nil,
                rgbaColor: nil,
                widthPixels: nil,
                ringCoordinates: sceneRing.map(\.sdkCoordinate),
                pointMarkerPositions: nil,
                pointMarkerRadiusPixels: nil,
                trafficFlowSegments: nil
            )
        case .pointMarkers:
            guard let scenePositions = positions else {
                throw ReferenceScenesCatalogOverlayConversionError.missingKindPayload(
                    kindText: kind, missingField: "positions"
                )
            }
            return MapOverlay(
                kind: .pointMarkers,
                coordinates: nil,
                rgbaColor: nil,
                widthPixels: nil,
                ringCoordinates: nil,
                pointMarkerPositions: scenePositions.map(\.sdkCoordinate),
                pointMarkerRadiusPixels: nil,
                trafficFlowSegments: nil
            )
        case .trafficFlow:
            guard let sceneSegments = segments else {
                throw ReferenceScenesCatalogOverlayConversionError.missingKindPayload(
                    kindText: kind, missingField: "segment_congestion_shapes"
                )
            }
            return MapOverlay(
                kind: .trafficFlow,
                coordinates: nil,
                rgbaColor: nil,
                widthPixels: nil,
                ringCoordinates: nil,
                pointMarkerPositions: nil,
                pointMarkerRadiusPixels: nil,
                trafficFlowSegments: sceneSegments.map(\.sdkTrafficFlowData)
            )
        }
    }
}

public extension ReferenceSceneTrafficFlowSegment {
    /// The contract's traffic-flow record for this scene observation. The
    /// artifact carries bucketed congestion classifications without speeds,
    /// so the speed fields stay unset exactly as the provider's records do.
    var sdkTrafficFlowData: TrafficFlowData {
        TrafficFlowData(
            shapeCoordinates: shapeCoordinates.map(\.sdkCoordinate),
            currentSpeedKilometersPerHour: nil,
            freeFlowSpeedKilometersPerHour: nil,
            congestionLevel: sdkCongestionLevel
        )
    }

    /// The contract's congestion level for the artifact's classification
    /// string (the artifact's congestion-level strings mirror the contract
    /// enum names; an unknown string maps to nothing — see the paired
    /// fallible conversion below).
    var sdkCongestionLevel: TrafficFlowCongestionLevel {
        switch congestionLevel {
        case "FreeFlow": return .freeFlow
        case "Slow": return .slow
        case "Congested": return .congested
        case "Blocked": return .blocked
        default: return .freeFlow
        }
    }
}

public extension ReferenceSceneOfflineRegion {
    /// The contract's region definition built from the artifact's record. The
    /// artifact's `image_format` and `tile_resolution` strings are the
    /// contract enum's own case names; an unknown string is a conversion
    /// failure rather than a silent fallback, because a region whose tile
    /// form does not match the base map's request would store tiles the
    /// scheduler cannot read.
    func contractRegionDefinition() throws -> RegionDefinition {
        guard let format = RasterImageFormat(artifactCaseName: imageFormat) else {
            throw ReferenceScenesCatalogOfflineRegionConversionError.unknownImageFormat(
                imageFormat
            )
        }
        guard let resolution = RasterTileResolution(artifactCaseName: tileResolution) else {
            throw ReferenceScenesCatalogOfflineRegionConversionError.unknownTileResolution(
                tileResolution
            )
        }
        return RegionDefinition(
            regionId: regionId,
            displayName: displayName,
            boundingBox: BoundingBox(
                southWest: boundingBox.southWest.coordinate,
                northEast: boundingBox.northEast.coordinate
            ),
            minimumZoomLevel: minimumZoomLevel,
            maximumZoomLevel: maximumZoomLevel,
            imageFormat: format,
            tileResolution: resolution,
            timeToLiveSeconds: timeToLiveSeconds
        )
    }
}

/// The pairing between the artifact's own enum case names and the contract's
/// raster enums (the artifact spells the contract variants, so no guessing).
private extension RasterImageFormat {
    init?(artifactCaseName: String) {
        switch artifactCaseName {
        case "Png": self = .png
        case "Jpeg": self = .jpeg
        default: return nil
        }
    }
}

private extension RasterTileResolution {
    init?(artifactCaseName: String) {
        switch artifactCaseName {
        case "Standard": self = .standard
        case "Retina": self = .retina
        default: return nil
        }
    }
}

/// Conversion failures of an artifact offline region, carrying the artifact's
/// own strings.
public enum ReferenceScenesCatalogOfflineRegionConversionError: LocalizedError, Swift.Error {
    /// The artifact's image-format string is not a contract raster format.
    case unknownImageFormat(String)
    /// The artifact's tile-resolution string is not a contract tile
    /// resolution.
    case unknownTileResolution(String)

    public var errorDescription: String? {
        switch self {
        case let .unknownImageFormat(format):
            return "the demo offline region's image format `\(format)` is not a contract raster format"
        case let .unknownTileResolution(resolution):
            return "the demo offline region's tile resolution `\(resolution)` is not a contract tile resolution"
        }
    }
}

/// Conversion failures when a shared-artifact scene cannot become an SDK
/// overlay descriptor, carrying the artifact's own names.
public enum ReferenceScenesCatalogOverlayConversionError: LocalizedError, Swift.Error {
    /// The artifact's `kind` string is not part of the documented pairing.
    case unknownSceneKind(String)
    /// The scene's kind-required payload field was absent.
    case missingKindPayload(kindText: String, missingField: String)

    public var errorDescription: String? {
        switch self {
        case let .unknownSceneKind(kindText):
            return "the demo scene kind `\(kindText)` is not part of the documented wasm-kind pairing"
        case let .missingKindPayload(kindText, missingField):
            return "the demo scene of kind `\(kindText)` is missing its `\(missingField)` payload"
        }
    }
}