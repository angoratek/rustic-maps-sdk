//! The replay plans that drive a simulated journey: one per data source.
//! The recorded trace fixture replays its recorded fixes verbatim; a planned route's journey follows the route's
//! own polyline through the contract's `RouteSimulator`, so the navigator
//! the plan flow launched (§4.1: search → route → "Start simulated
//! navigation") runs with the provider's real steps, payloads and distances
//! between shape vertices. Both plans feed the same replay timer and the
//! same `Navigator`.

import Foundation
import RusticMapsSDK

/// One simulated journey's replayable contents: the trip route and the
/// source the replay consumes — the recorded fixes (recorded traces) or the
/// contract's route walker (planned routes).
struct JourneyReplayPlan {
    /// The route the trip starts on (the contract's record).
    let routeData: Route
    /// The simulated fixes in traversal order for a recorded trace; empty
    /// for a route-following plan, whose walker owns the interpolation.
    let samples: [RecordedTraceSample]
    /// The sample cadence the fixes were recorded at, in fixture seconds
    /// (unused by a route-following plan).
    let sampleIntervalSeconds: Double
    /// Whether the trip walks the route's own polyline through the
    /// contract's `RouteSimulator` rather than replaying recorded samples.
    let followsRouteGeometry: Bool

    /// The recorded trace: its fixes and cadence verbatim.
    static func fromRecordedTrace(_ recordedJourney: RecordedTraceJourney) -> JourneyReplayPlan {
        JourneyReplayPlan(
            routeData: recordedJourney.routeData,
            samples: recordedJourney.samples,
            sampleIntervalSeconds: recordedJourney.sampleIntervalSeconds,
            followsRouteGeometry: false
        )
    }

    /// The planned route's walk: no samples are precomputed — the route
    /// walker interpolates from elapsed simulated time and derives the
    /// course from the active segment's bearing.
    static func fromPlannedRoute(
        _ plannedRouteData: Route
    ) -> JourneyReplayPlan {
        JourneyReplayPlan(
            routeData: plannedRouteData,
            samples: [],
            sampleIntervalSeconds:
                ReferenceApplicationConfiguration.simulatedJourneySampleIntervalSeconds,
            followsRouteGeometry: true
        )
    }
}

/// The fix-to-fix course measure of the §5.3 follow camera for recorded
/// traces: the displacement magnitude (the haversine distance) and the
/// course-over-ground bearing (degrees clockwise from true north) between
/// two fixes.
enum JourneyReplayCourseMeasure {
    /// The great-circle displacement (meters) between the fixes.
    static func displacementMeters(
        from originCoordinate: GeographicCoordinate,
        to destinationCoordinate: GeographicCoordinate
    ) -> Double {
        let originLatitudeRadians = originCoordinate.latitude * radiansPerDegree
        let destinationLatitudeRadians = destinationCoordinate.latitude * radiansPerDegree
        let latitudeSpanRadians = destinationLatitudeRadians - originLatitudeRadians
        let longitudeSpanRadians = (destinationCoordinate.longitude - originCoordinate.longitude)
            * radiansPerDegree
        let haversineOpposite = pow(sin(latitudeSpanRadians / 2), 2)
            + cos(originLatitudeRadians) * cos(destinationLatitudeRadians)
                * pow(sin(longitudeSpanRadians / 2), 2)
        let centralAngle = 2 * asin(min(1, sqrt(haversineOpposite)))
        return ReferenceApplicationConfiguration.meanEarthRadiusMeters * centralAngle
    }

    /// The forward-azimuth bearing (degrees clockwise from true north) of
    /// the great-circle path from the origin fix to the destination fix:
    /// the initial-bearing formula on the sphere (one-shot for a fix
    /// window's course samples — the walk's per-fix legs are tiny).
    static func bearingDegrees(
        from originCoordinate: GeographicCoordinate,
        to destinationCoordinate: GeographicCoordinate
    ) -> Double {
        let originLatitudeRadians = originCoordinate.latitude * radiansPerDegree
        let destinationLatitudeRadians = destinationCoordinate.latitude * radiansPerDegree
        let longitudeSpanRadians = (destinationCoordinate.longitude - originCoordinate.longitude)
            * radiansPerDegree
        let bearingYComponent = cos(originLatitudeRadians) * sin(destinationLatitudeRadians)
            - sin(originLatitudeRadians) * cos(destinationLatitudeRadians)
                * cos(longitudeSpanRadians)
        let bearingXComponent = cos(destinationLatitudeRadians) * sin(longitudeSpanRadians)
        let bearingRadians = atan2(bearingXComponent, bearingYComponent)
        return wrappedBearingDegrees(bearingRadians * degreesPerRadian)
    }

    /// The radians in one degree (π/180): converts coordinate degrees to
    /// radians for the spherical formulas above.
    private static let radiansPerDegree: Double = Double.pi / 180

    /// The degrees in one radian (180/π): converts a bearing in radians back
    /// to the degrees the camera consumes.
    private static let degreesPerRadian: Double = 180 / Double.pi
}
