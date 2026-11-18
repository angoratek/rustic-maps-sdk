//! The camera the shipped `MapView` exposes is the contract's, not a second
//! copy: set, readback, interpolation midpoint, fit and bounds all go through
//! the SDK's camera members. Every case runs on the simulator without a
//! surface or a provider.

import RusticMapsSDK
import UIKit
import XCTest

@testable import MapNativeCoreUI

/// The wrapper's camera path delegates to the contract value.
@MainActor
final class MapViewCameraDelegationTests: XCTestCase {
    private let startPose = MapCameraPose(
        latitudeDegrees: 38.5,
        longitudeDegrees: -121.5,
        zoomLevel: 10,
        bearingDegrees: 0,
        pitchDegrees: 0
    )

    private let targetPose = MapCameraPose(
        latitudeDegrees: 38.7,
        longitudeDegrees: -121.3,
        zoomLevel: 14,
        bearingDegrees: 90,
        pitchDegrees: 20
    )

    private func makeMapView() -> MapNativeCoreUI.MapView {
        MapNativeCoreUI.MapView(frame: CGRect(x: 0, y: 0, width: 64, height: 64))
    }

    private func contractCamera(_ pose: MapCameraPose) throws -> MapCamera {
        try MapCamera(
            center: GeographicCoordinate(
                latitude: pose.latitudeDegrees,
                longitude: pose.longitudeDegrees
            ),
            zoomLevel: pose.zoomLevel,
            bearingDegrees: pose.bearingDegrees,
            pitchDegrees: pose.pitchDegrees
        )
    }

    private func readPose(_ camera: MapCamera) -> MapCameraPose {
        let center = camera.center()
        return MapCameraPose(
            latitudeDegrees: center.latitude,
            longitudeDegrees: center.longitude,
            zoomLevel: camera.zoomLevel(),
            bearingDegrees: camera.bearingDegrees(),
            pitchDegrees: camera.pitchDegrees()
        )
    }

    func testSetCameraReadsBackTheContractNormalizedPose() throws {
        let mapView = makeMapView()
        defer { mapView.teardown() }

        let unnormalized = MapCameraPose(
            latitudeDegrees: 38.58,
            longitudeDegrees: -121.49,
            zoomLevel: 99,
            bearingDegrees: 370,
            pitchDegrees: 120
        )
        mapView.setCamera(unnormalized)

        let expected = readPose(try contractCamera(unnormalized))
        XCTAssertEqual(mapView.camera, expected)
        // The contract clamped and wrapped instead of echoing the input.
        XCTAssertNotEqual(mapView.camera.zoomLevel, unnormalized.zoomLevel)
        XCTAssertNotEqual(mapView.camera.bearingDegrees, unnormalized.bearingDegrees)
        XCTAssertNotEqual(mapView.camera.pitchDegrees, unnormalized.pitchDegrees)
    }

    func testAnimateCameraAppliesTheContractInterpolationMidpoint() throws {
        let mapView = makeMapView()
        defer { mapView.teardown() }
        mapView.setCamera(startPose)
        mapView.animateCamera(targetPose, durationMilliseconds: 300)

        mapView.applyCameraAnimationFraction(0.5)

        let expected = readPose(
            try MapCamera.interpolate(
                startCamera: try contractCamera(startPose),
                targetCamera: try contractCamera(targetPose),
                fraction: 0.5
            )
        )
        XCTAssertEqual(mapView.camera, expected)
        XCTAssertNotEqual(mapView.camera, startPose)
        XCTAssertNotEqual(mapView.camera, targetPose)
    }

    func testAnimateCameraEndsExactlyOnTheTargetPose() throws {
        let mapView = makeMapView()
        defer { mapView.teardown() }
        mapView.setCamera(startPose)
        mapView.animateCamera(targetPose, durationMilliseconds: 300)

        mapView.applyCameraAnimationFraction(1.0)

        let expected = readPose(try contractCamera(targetPose))
        XCTAssertEqual(mapView.camera, expected)
    }

    func testGestureCancelStopsTheAnimationWhereItStands() {
        let mapView = makeMapView()
        defer { mapView.teardown() }
        mapView.setCamera(startPose)
        mapView.animateCamera(targetPose, durationMilliseconds: 300)
        mapView.applyCameraAnimationFraction(0.5)
        let midpoint = mapView.camera

        // Every gesture recognizer's begun phase calls this same seam.
        mapView.cancelCameraAnimation()
        mapView.applyCameraAnimationFraction(1.0)

        XCTAssertEqual(mapView.camera, midpoint)
    }

    func testFitBoundsWithoutAnAttachedViewportChangesNothing() {
        let mapView = makeMapView()
        defer { mapView.teardown() }
        mapView.setCamera(startPose)

        mapView.fitBounds(
            boundingBox: BoundingBox(
                southWest: GeographicCoordinate(latitude: 38.4, longitude: -121.7),
                northEast: GeographicCoordinate(latitude: 38.8, longitude: -121.2)
            )
        )

        XCTAssertEqual(mapView.camera, startPose)
    }

    func testGeographicBoundsDelegatesAndAnswersNilWithoutASurface() {
        let mapView = makeMapView()
        defer { mapView.teardown() }
        mapView.setCamera(startPose)

        XCTAssertNil(mapView.cameraGeographicBounds())
    }
}
