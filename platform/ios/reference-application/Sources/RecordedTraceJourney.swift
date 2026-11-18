//! Decoding the recorded synthetic navigation trace (the journey fixture
//! `sacramento_clean_walk.json`, bundled straight from the committed fixture)
//! into the contract's `Route` plus the location samples the simulated
//! journey replays. The fixture's `maneuver_type` carries the contract's
//! stable snake-case label directly, and `startNavigation` still refuses an
//! unknown label loudly.

import Foundation
import RusticMapsSDK

/// One decoded location sample of the recorded trace.
struct RecordedTraceSample {
    /// The recorded fix coordinate.
    let coordinate: GeographicCoordinate
    /// The recorded horizontal accuracy in meters.
    let horizontalAccuracyMeters: Double
    /// The recorded timestamp offset in seconds from the journey start.
    let timestampOffsetSeconds: Double
}

/// The decoded recorded trace: the trip route plus the replay samples.
struct RecordedTraceJourney {
    /// The trace file's own metadata interval (the sample cadence the
    /// generator recorded, in seconds).
    let sampleIntervalSeconds: Double
    /// The route the journey follows, as the contract's record.
    let routeData: Route
    /// The recorded fixes in traversal order.
    let samples: [RecordedTraceSample]
}

extension RecordedTraceJourney {
    /// Decodes the recorded fixture JSON into the journey value.
    static func decoded(from data: Data) throws -> RecordedTraceJourney {
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let fixtureDocument = try decoder.decode(FixtureDocument.self, from: data)
        return RecordedTraceJourney(
            sampleIntervalSeconds: fixtureDocument.metadata.sampleIntervalSeconds,
            routeData: try fixtureDocument.route.sdkRouteData(),
            samples: fixtureDocument.samples.map(\.sample)
        )
    }
}

/// The recorded fixture's decoded document shape (private to this module;
/// the public journey value above carries the consumed contract values).
private struct FixtureDocument: Decodable {
    let metadata: Metadata
    let route: FixtureRoute
    let samples: [FixtureSample]

    struct Metadata: Decodable {
        let scenarioName: String
        let sampleIntervalSeconds: Double
    }

    struct FixtureRoute: Decodable {
        let distanceMeters: Double
        let durationSeconds: Double
        let boundingBox: FixtureBoundingBox
        let legs: [FixtureLeg]

        /// The contract's route record for the fixture route.
        func sdkRouteData() throws -> Route {
            Route(
                distanceMeters: distanceMeters,
                durationSeconds: durationSeconds,
                boundingBox: BoundingBox(
                    southWest: boundingBox.southWest.sdkCoordinate,
                    northEast: boundingBox.northEast.sdkCoordinate
                ),
                // The trace records per-step shapes, so the route-level
                // geometry is empty (the contract's documented shape for it).
                geometry: [],
                legs: try legs.map(\.sdkRouteLeg)
            )
        }
    }

    struct FixtureBoundingBox: Decodable {
        let southWest: FixtureCoordinate
        let northEast: FixtureCoordinate
    }

    struct FixtureLeg: Decodable {
        let distanceMeters: Double
        let durationSeconds: Double
        let steps: [FixtureStep]

        var sdkRouteLeg: RouteLeg {
            RouteLeg(
                distanceMeters: distanceMeters,
                durationSeconds: durationSeconds,
                steps: steps.map(\.sdkRouteStep)
            )
        }
    }

    struct FixtureStep: Decodable {
        let distanceMeters: Double
        let durationSeconds: Double
        /// The step's maneuver in the contract's stable label spelling,
        /// passed through verbatim (an unknown label fails the trip start
        /// loudly — never guessed).
        let maneuverType: String
        let instructionDisplayText: String
        let instructionSpeechText: String
        let shape: [FixtureCoordinate]

        var sdkRouteStep: RouteStep {
            RouteStep(
                distanceMeters: distanceMeters,
                durationSeconds: durationSeconds,
                maneuverType: maneuverType,
                instructionDisplayText: instructionDisplayText,
                instructionSpeechText: instructionSpeechText,
                shape: shape.map(\.sdkCoordinate)
            )
        }
    }

    struct FixtureCoordinate: Decodable {
        let latitude: Double
        let longitude: Double

        var sdkCoordinate: GeographicCoordinate {
            GeographicCoordinate(latitude: latitude, longitude: longitude)
        }
    }

    struct FixtureSample: Decodable {
        let latitude: Double
        let longitude: Double
        let horizontalAccuracyMeters: Double
        let timestampOffsetSeconds: Double

        var sample: RecordedTraceSample {
            RecordedTraceSample(
                coordinate: GeographicCoordinate(latitude: latitude, longitude: longitude),
                horizontalAccuracyMeters: horizontalAccuracyMeters,
                timestampOffsetSeconds: timestampOffsetSeconds
            )
        }
    }
}