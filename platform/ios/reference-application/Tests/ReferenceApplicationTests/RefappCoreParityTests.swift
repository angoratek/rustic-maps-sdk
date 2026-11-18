//! Value-parity tests for the generated refapp-only bindings.
//!
//! The expectations are the documented behavior values the deleted platform
//! copies produced: the follow camera's state machine, the circular course
//! median, the smoothstep and exponential easings, the scale-bar ladder, the
//! compass's off-north distance, and the recorded trace cursor and dispatch
//! interval.

import MapNativeCoreUI
import XCTest

@testable import ReferenceApplication

final class RefappCoreParityTests: XCTestCase {

    // MARK: Free functions

    func testFreeFunctionsReportTheDocumentedValues() {
        XCTAssertEqual(smoothstepEasedProgress(linearProgress: 0.25), 0.15625, accuracy: 1.0e-9)
        XCTAssertEqual(smoothstepEasedProgress(linearProgress: 0.5), 0.5, accuracy: 1.0e-9)
        XCTAssertEqual(bearingOffNorthDegrees(bearingDegrees: 350), 10, accuracy: 1.0e-9)
        XCTAssertEqual(
            followZoomTowardTripEnd(remainingDistanceFraction: 0),
            17.5,
            accuracy: 1.0e-9
        )
        XCTAssertEqual(
            followZoomTowardTripEnd(remainingDistanceFraction: 0.5),
            17.25,
            accuracy: 1.0e-9
        )
        XCTAssertEqual(
            followZoomTowardTripEnd(remainingDistanceFraction: 1),
            17,
            accuracy: 1.0e-9
        )
    }

    // MARK: Scale bar

    func testScaleBarPicksTheLargestLadderDistanceFittingTheWidthRange() throws {
        let oneMeterPerPixel = try XCTUnwrap(
            scaleBarSelection(
                groundMetersPerScreenPixel: 1,
                minimumWidthPixels: 50,
                maximumWidthPixels: 120
            )
        )
        XCTAssertEqual(oneMeterPerPixel.distanceMeters, 100, accuracy: 1.0e-9)
        XCTAssertEqual(oneMeterPerPixel.widthPixels, 100, accuracy: 1.0e-9)

        let deepZoom = try XCTUnwrap(
            scaleBarSelection(
                groundMetersPerScreenPixel: 0.4,
                minimumWidthPixels: 50,
                maximumWidthPixels: 120
            )
        )
        XCTAssertEqual(deepZoom.distanceMeters, 30, accuracy: 1.0e-9)
        XCTAssertEqual(deepZoom.widthPixels, 75, accuracy: 1.0e-9)

        XCTAssertNil(
            scaleBarSelection(
                groundMetersPerScreenPixel: 0,
                minimumWidthPixels: 50,
                maximumWidthPixels: 120
            )
        )
        XCTAssertNil(
            scaleBarSelection(
                groundMetersPerScreenPixel: -1,
                minimumWidthPixels: 50,
                maximumWidthPixels: 120
            )
        )
    }

    func testScaleBarLabelsItsLadderDistancesMetric() {
        XCTAssertEqual(scaleBarDistanceLabel(distanceMeters: 100), "100 m")
        XCTAssertEqual(scaleBarDistanceLabel(distanceMeters: 3), "3 m")
        XCTAssertEqual(scaleBarDistanceLabel(distanceMeters: 1000), "1 km")
        XCTAssertEqual(scaleBarDistanceLabel(distanceMeters: 2000), "2 km")
        XCTAssertEqual(scaleBarDistanceLabel(distanceMeters: 50_000), "50 km")
    }

    func testScaleBarReadoutAdaptsTheGeneratedSelection() throws {
        let readout = try XCTUnwrap(
            ScaleBarReadout.readout(
                groundMetersPerPixel: 1,
                minimumWidthPixels: 50,
                maximumWidthPixels: 120
            )
        )
        XCTAssertEqual(readout.distanceMeters, 100, accuracy: 1.0e-9)
        XCTAssertEqual(readout.barWidthPixels, 100, accuracy: 1.0e-9)
    }

    // MARK: Course median

    func testCourseMedianIsCircularAndWindowed() throws {
        let smoother = CourseMedianSmoother(windowFixCount: 3)
        XCTAssertNil(smoother.median())
        for course in [1.0, 2.0, 3.0, 4.0] {
            smoother.add(courseDegrees: course)
        }
        XCTAssertEqual(smoother.windowFixCount(), 3)
        XCTAssertEqual(smoother.samples(), [2, 3, 4])
        XCTAssertEqual(try XCTUnwrap(smoother.median()), 3, accuracy: 1.0e-9)

        smoother.reset()
        XCTAssertNil(smoother.median())

        let wrapSmoother = CourseMedianSmoother(windowFixCount: 5)
        for course in [356.0, 2.0, 358.0, 4.0, 0.0] {
            wrapSmoother.add(courseDegrees: course)
        }
        XCTAssertEqual(try XCTUnwrap(wrapSmoother.median()), 0, accuracy: 1.0e-9)
    }

    // MARK: Easing

    func testTimedEaseRunsFromStartToTargetAndFinishesOnce() {
        let ease = SmoothstepTimedEase()
        XCTAssertTrue(ease.advance(nowMilliseconds: 0).finished)
        ease.begin(
            startMilliseconds: 0,
            fromValue: 10,
            toValue: 20,
            durationMilliseconds: 300
        )
        XCTAssertTrue(ease.isRunning())
        let start = ease.advance(nowMilliseconds: 0)
        XCTAssertEqual(start.value, 10, accuracy: 1.0e-9)
        XCTAssertFalse(start.finished)
        let middle = ease.advance(nowMilliseconds: 150)
        XCTAssertEqual(middle.value, 15, accuracy: 1.0e-9)
        XCTAssertFalse(middle.finished)
        let end = ease.advance(nowMilliseconds: 300)
        XCTAssertEqual(end.value, 20, accuracy: 1.0e-9)
        XCTAssertTrue(end.finished)
        XCTAssertEqual(ease.targetValue(), 20, accuracy: 1.0e-9)
        XCTAssertFalse(ease.isRunning())
    }

    func testExponentialChaseConvergesAndSnaps() {
        let chase = ExponentialValueChase(timeConstantMilliseconds: 150)
        XCTAssertEqual(chase.timeConstantMilliseconds(), 150, accuracy: 1.0e-9)
        let first = chase.advance(current: 0, target: 10, nowMilliseconds: 0)
        XCTAssertTrue(first > 0 && first < 10)

        var current = first
        for step in 1...200 {
            current = chase.advance(
                current: current,
                target: 10,
                nowMilliseconds: Double(step) * 16
            )
        }
        XCTAssertLessThan(abs(current - 10), 0.1)

        let snapping = ExponentialValueChase(timeConstantMilliseconds: 150)
        XCTAssertEqual(
            snapping.advance(current: 9.999, target: 10, nowMilliseconds: 0),
            10,
            accuracy: 1.0e-9
        )
    }

    // MARK: Follow camera

    func testFollowCameraTracksGestureBreaksAndRestores() throws {
        let camera = FollowCamera()
        XCTAssertEqual(camera.phase(), .disengaged)

        camera.engageTrip(
            currentCenterLatitudeDegrees: 38,
            currentCenterLongitudeDegrees: -121,
            currentZoomLevel: 12,
            currentBearingDegrees: 15,
            currentPitchDegrees: 0
        )
        XCTAssertEqual(camera.phase(), .engaged)
        let captured = try XCTUnwrap(camera.preTripCamera())
        XCTAssertEqual(captured.centerLatitudeDegrees, 38, accuracy: 1.0e-9)
        XCTAssertEqual(captured.zoomLevel, 12, accuracy: 1.0e-9)
        XCTAssertEqual(camera.targetZoomLevel(), 17, accuracy: 1.0e-9)
        XCTAssertNil(camera.targetBearingDegrees())

        camera.onFix(
            latitudeDegrees: 38.0001,
            longitudeDegrees: -121.0001,
            courseHeadingDegrees: 90,
            remainingDistanceFraction: 0.8
        )
        XCTAssertEqual(
            camera.targetCenterLatitudeDegrees(),
            38.0001,
            accuracy: 1.0e-9
        )
        XCTAssertEqual(try XCTUnwrap(camera.targetBearingDegrees()), 90, accuracy: 1.0e-9)

        camera.breakOnGesture()
        XCTAssertEqual(camera.phase(), .brokenByGesture)
        camera.onFix(
            latitudeDegrees: 38.0002,
            longitudeDegrees: -121.0002,
            courseHeadingDegrees: 92,
            remainingDistanceFraction: 0.7
        )
        XCTAssertEqual(camera.phase(), .brokenByGesture)
        XCTAssertEqual(
            camera.targetCenterLatitudeDegrees(),
            38.0002,
            accuracy: 1.0e-9
        )
        XCTAssertEqual(try XCTUnwrap(camera.targetBearingDegrees()), 90, accuracy: 1.0e-9)

        camera.reengage()
        XCTAssertEqual(camera.phase(), .engaged)

        let restored = try XCTUnwrap(camera.restorePreTripCamera())
        XCTAssertEqual(restored.zoomLevel, 12, accuracy: 1.0e-9)
        XCTAssertEqual(camera.phase(), .disengaged)
        XCTAssertNil(camera.preTripCamera())
        XCTAssertNil(camera.targetBearingDegrees())
    }

    // MARK: Trace cursor and dispatch interval

    func testTraceCursorAdvancesAndSaturates() throws {
        let cursor = TraceCursor(sampleCount: 3)
        XCTAssertEqual(cursor.sampleCount(), 3)
        XCTAssertEqual(cursor.nextSampleIndex(), 0)
        XCTAssertEqual(cursor.peek(), 0)
        XCTAssertEqual(cursor.advance(), 0)
        XCTAssertEqual(cursor.advance(), 1)
        XCTAssertEqual(cursor.advance(), 2)
        XCTAssertNil(cursor.advance())
        XCTAssertTrue(cursor.isFinished())
        XCTAssertEqual(cursor.remainingSampleCount(), 0)

        cursor.reset()
        XCTAssertEqual(cursor.remainingSampleCount(), 3)
        cursor.seekToSampleIndex(sampleIndex: 99)
        XCTAssertTrue(cursor.isFinished())
        cursor.seekToSampleIndex(sampleIndex: 1)
        XCTAssertEqual(cursor.advance(), 1)
    }

    func testTraceDispatchIntervalFollowsTheAccelerationFactor() {
        XCTAssertEqual(
            traceDispatchIntervalMilliseconds(
                recordedSampleIntervalSeconds: 5,
                playbackAccelerationFactor: 20
            ),
            250
        )
        XCTAssertEqual(
            traceDispatchIntervalMilliseconds(
                recordedSampleIntervalSeconds: 1,
                playbackAccelerationFactor: 1
            ),
            1000
        )
        XCTAssertEqual(
            traceDispatchIntervalMilliseconds(
                recordedSampleIntervalSeconds: 0.0001,
                playbackAccelerationFactor: 1
            ),
            1
        )
        XCTAssertEqual(
            traceDispatchIntervalMilliseconds(
                recordedSampleIntervalSeconds: 5,
                playbackAccelerationFactor: 0
            ),
            1
        )
    }
}
