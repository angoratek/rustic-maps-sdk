package com.rusticmaps.sdk.referenceapplication.map

import com.rusticmaps.sdk.ux.ReferenceAppUXDimensions
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.map_refapp_core.CourseMedianSmoother
import uniffi.map_refapp_core.ExponentialValueChase
import uniffi.map_refapp_core.FollowCamera
import uniffi.map_refapp_core.FollowPhase
import uniffi.map_refapp_core.SmoothstepTimedEase
import uniffi.map_refapp_core.TraceCursor
import uniffi.map_refapp_core.bearingOffNorthDegrees
import uniffi.map_refapp_core.followZoomTowardTripEnd
import uniffi.map_refapp_core.scaleBarDistanceLabel
import uniffi.map_refapp_core.scaleBarSelection
import uniffi.map_refapp_core.traceDispatchIntervalMilliseconds

/**
 * The map-logics unit tests (JVM): the generated refapp-core bindings report
 * the same behavior values the deleted platform copies did — the follow
 * camera's state machine (engage, gesture-break, fix re-engage, trip-end
 * restore, zoom clamp), the circular course median, the camera easings, the
 * scale bar's ladder fit, the compass's off-north distance and the recorded
 * trace cursor and dispatch interval (docs/ReferenceAppUX.md
 * §4.2/§5.3/§2.4).
 */
class FollowCameraTest {
    @Test
    fun `trip start captures the pre-trip camera and applies the fixed follow zoom`() {
        val followCamera = FollowCamera()
        followCamera.engageTrip(
            currentCenterLatitudeDegrees = 47.3,
            currentCenterLongitudeDegrees = 8.5,
            currentZoomLevel = 15.0,
            currentBearingDegrees = 10.0,
            currentPitchDegrees = 0.0,
        )
        assertEquals(FollowPhase.ENGAGED, followCamera.phase())
        assertEquals(
            ReferenceAppUXDimensions.NAVIGATION_FOLLOW_ZOOM,
            followCamera.targetZoomLevel(),
            0.0,
        )
        assertNull(followCamera.targetBearingDegrees())
        val preTripCamera = followCamera.preTripCamera()!!
        assertEquals(15.0, preTripCamera.zoomLevel, 0.0)
        assertEquals(0.0, preTripCamera.pitchDegrees, 0.0)
    }

    @Test
    fun `a location fix retargets center, smoothed bearing and ramped zoom`() {
        val followCamera = FollowCamera()
        followCamera.engageTrip(47.3, 8.5, 15.0, 10.0, 0.0)
        followCamera.onFix(
            latitudeDegrees = 47.4,
            longitudeDegrees = 8.6,
            courseHeadingDegrees = 90.0,
            remainingDistanceFraction = 1.0,
        )
        assertEquals(47.4, followCamera.targetCenterLatitudeDegrees(), 0.0)
        assertEquals(8.6, followCamera.targetCenterLongitudeDegrees(), 0.0)
        assertEquals(90.0, followCamera.targetBearingDegrees()!!, 0.0)
        assertEquals(
            ReferenceAppUXDimensions.NAVIGATION_FOLLOW_ZOOM,
            followCamera.targetZoomLevel(),
            ZOOM_ASSERTION_TOLERANCE,
        )
    }

    @Test
    fun `a user gesture breaks follow immediately and a later fix keeps it broken`() {
        val followCamera = FollowCamera()
        followCamera.engageTrip(47.3, 8.5, 15.0, 10.0, 0.0)
        followCamera.onFix(47.4, 8.6, 90.0, 1.0)
        followCamera.breakOnGesture()
        assertEquals(FollowPhase.BROKEN_BY_GESTURE, followCamera.phase())
        // The fix keeps the targets current but never re-engages follow: the
        // user's panned view stands until the my-location control re-engages
        // it (§5.3's re-center rule).
        followCamera.onFix(47.5, 8.7, 95.0, 0.8)
        assertEquals(FollowPhase.BROKEN_BY_GESTURE, followCamera.phase())
        assertEquals(47.5, followCamera.targetCenterLatitudeDegrees(), 0.0)
        // The window {90, 95} ties its circular median; the earlier sample wins.
        assertEquals(90.0, followCamera.targetBearingDegrees()!!, 0.0)
    }

    @Test
    fun `the my-location control re-engages a broken follow`() {
        val followCamera = FollowCamera()
        followCamera.engageTrip(47.3, 8.5, 15.0, 10.0, 0.0)
        followCamera.breakOnGesture()
        // A later fix keeps follow broken; the explicit re-engage resumes it.
        followCamera.onFix(47.5, 8.7, 95.0, 0.8)
        followCamera.reengage()
        assertEquals(FollowPhase.ENGAGED, followCamera.phase())
    }

    @Test
    fun `the trip end restores the captured pre-trip camera and disengages`() {
        val followCamera = FollowCamera()
        followCamera.engageTrip(47.3, 8.5, 15.0, 10.0, 0.0)
        followCamera.onFix(47.4, 8.6, 90.0, 0.2)
        val restoredCamera = followCamera.restorePreTripCamera()!!
        assertEquals(15.0, restoredCamera.zoomLevel, 0.0)
        assertEquals(10.0, restoredCamera.bearingDegrees, 0.0)
        assertEquals(47.3, restoredCamera.centerLatitudeDegrees, 0.0)
        assertEquals(0.0, restoredCamera.pitchDegrees, 0.0)
        assertEquals(FollowPhase.DISENGAGED, followCamera.phase())
        assertNull(followCamera.preTripCamera())
    }

    @Test
    fun `the follow zoom ramps from the base toward the maximum toward trip end`() {
        assertEquals(
            ReferenceAppUXDimensions.NAVIGATION_FOLLOW_ZOOM,
            followZoomTowardTripEnd(1.0),
            ZOOM_ASSERTION_TOLERANCE,
        )
        assertEquals(
            (ReferenceAppUXDimensions.NAVIGATION_FOLLOW_ZOOM +
                ReferenceAppUXDimensions.NAVIGATION_FOLLOW_ZOOM_MAXIMUM) / 2.0,
            followZoomTowardTripEnd(0.5),
            ZOOM_ASSERTION_TOLERANCE,
        )
        assertEquals(
            ReferenceAppUXDimensions.NAVIGATION_FOLLOW_ZOOM_MAXIMUM,
            followZoomTowardTripEnd(0.0),
            ZOOM_ASSERTION_TOLERANCE,
        )
        // Fractions beyond the trip's ends clamp.
        assertEquals(
            ReferenceAppUXDimensions.NAVIGATION_FOLLOW_ZOOM,
            followZoomTowardTripEnd(2.0),
            ZOOM_ASSERTION_TOLERANCE,
        )
        assertEquals(
            ReferenceAppUXDimensions.NAVIGATION_FOLLOW_ZOOM_MAXIMUM,
            followZoomTowardTripEnd(-1.0),
            ZOOM_ASSERTION_TOLERANCE,
        )
    }

    @Test
    fun `the course median smooths over the named window with circular courses`() {
        val courseMedianSmoother = CourseMedianSmoother(5u)
        assertNull(courseMedianSmoother.median())
        listOf(100.0, 96.0, 104.0, 98.0, 102.0).forEach { course ->
            courseMedianSmoother.add(course)
        }
        assertEquals(100.0, courseMedianSmoother.median()!!, 0.0)
        // The window drops its oldest: the median rides forward.
        courseMedianSmoother.add(200.0)
        assertEquals(102.0, courseMedianSmoother.median()!!, 0.0)
        // Circular wrap: a north-crossing window medians across the wrap.
        courseMedianSmoother.reset()
        listOf(356.0, 2.0, 358.0, 4.0, 0.0).forEach { course ->
            courseMedianSmoother.add(course)
        }
        // The circular median is the windowed sample minimizing the total
        // angular distance — here the wrap-window's true center, 0 degrees.
        assertEquals(0.0, courseMedianSmoother.median()!!, 0.0)
    }

    @Test
    fun `the smoothstep ease completes over its duration and rests`() {
        val pitchSwingEase = SmoothstepTimedEase()
        pitchSwingEase.begin(
            startMilliseconds = 1_000.0,
            fromValue = 0.0,
            toValue = 45.0,
            durationMilliseconds = 300.0,
        )
        val midStep = pitchSwingEase.advance(1_000.0 + 150.0)
        // The smoothstep midpoint is exactly halfway.
        assertEquals(22.5, midStep.value, 0.5)
        assertFalse(midStep.finished)
        val endStep = pitchSwingEase.advance(1_000.0 + 400.0)
        assertEquals(45.0, endStep.value, 0.0)
        assertTrue(endStep.finished)
    }

    @Test
    fun `the chase advances toward the target and snaps under the snap distance`() {
        val followBearingChase = ExponentialValueChase(timeConstantMilliseconds = 150.0)
        val firstChasedValue = followBearingChase.advance(
            current = 0.0,
            target = 10.0,
            nowMilliseconds = 0.0,
        )
        assertTrue(firstChasedValue in 0.0..10.0 && firstChasedValue != 10.0)
        // A target inside the snap distance resolves exactly to the target.
        assertEquals(
            0.04,
            followBearingChase.advance(
                current = 0.0,
                target = 0.04,
                nowMilliseconds = 50.0,
            ),
            0.0,
        )
        // The shortest way around: 350 degrees reached from 0 through the
        // (+) wrap, never the long way through 180.
        val wrappedChasedValue = followBearingChase.advance(
            current = 0.0,
            target = 350.0,
            nowMilliseconds = 100.0,
        )
        assertTrue(wrappedChasedValue > 180.0 || wrappedChasedValue <= 0.05)
    }

    @Test
    fun `the scale bar picks the largest ladder distance fitting the width range`() {
        // One ground meter per pixel: the 100 m bar sits at 100 px, inside
        // the token range; 200 m overflows, so 100 m wins.
        val selectedScaleBarScale = scaleBarSelection(
            1.0,
            ReferenceAppUXDimensions.SCALE_BAR_WIDTH_MINIMUM_PIXELS,
            ReferenceAppUXDimensions.SCALE_BAR_WIDTH_MAXIMUM_PIXELS,
        )!!
        assertEquals(100.0, selectedScaleBarScale.distanceMeters, 0.0)
        assertEquals(100.0, selectedScaleBarScale.widthPixels, 0.0)
        // A finer camera (0.4 ground meters per pixel): the 30 m bar spans
        // 75 px (the largest fitting ladder distance under the 120 px
        // ceiling; 50 m would span 125 px and overflow).
        val deepZoomScale = scaleBarSelection(
            0.4,
            ReferenceAppUXDimensions.SCALE_BAR_WIDTH_MINIMUM_PIXELS,
            ReferenceAppUXDimensions.SCALE_BAR_WIDTH_MAXIMUM_PIXELS,
        )!!
        assertEquals(30.0, deepZoomScale.distanceMeters, 1e-9)
        assertEquals(75.0, deepZoomScale.widthPixels, 1e-9)
        // A degenerate answer selects nothing.
        assertNull(
            scaleBarSelection(
                Double.NaN,
                ReferenceAppUXDimensions.SCALE_BAR_WIDTH_MINIMUM_PIXELS,
                ReferenceAppUXDimensions.SCALE_BAR_WIDTH_MAXIMUM_PIXELS,
            ),
        )
        assertNull(
            scaleBarSelection(
                0.0,
                ReferenceAppUXDimensions.SCALE_BAR_WIDTH_MINIMUM_PIXELS,
                ReferenceAppUXDimensions.SCALE_BAR_WIDTH_MAXIMUM_PIXELS,
            ),
        )
        assertNull(
            scaleBarSelection(
                -1.0,
                ReferenceAppUXDimensions.SCALE_BAR_WIDTH_MINIMUM_PIXELS,
                ReferenceAppUXDimensions.SCALE_BAR_WIDTH_MAXIMUM_PIXELS,
            ),
        )
    }

    @Test
    fun `the scale bar labels its ladder distances metric`() {
        assertEquals("100 m", scaleBarDistanceLabel(100.0))
        assertEquals("3 m", scaleBarDistanceLabel(3.0))
        assertEquals("1 km", scaleBarDistanceLabel(1000.0))
        assertEquals("2 km", scaleBarDistanceLabel(2000.0))
        assertEquals("50 km", scaleBarDistanceLabel(50_000.0))
    }

    @Test
    fun `the compass appears only when the bearing is off north by the threshold`() {
        val compassVisibilityBearingThreshold =
            ReferenceAppUXDimensions.COMPASS_VISIBILITY_BEARING_THRESHOLD_DEGREES
        // North up: no compass.
        assertTrue(bearingOffNorthDegrees(0.0) < compassVisibilityBearingThreshold)
        assertTrue(bearingOffNorthDegrees(360.0) < compassVisibilityBearingThreshold)
        // Half a degree is under the token threshold; one degree and beyond is over.
        assertTrue(bearingOffNorthDegrees(0.5) < compassVisibilityBearingThreshold)
        assertTrue(bearingOffNorthDegrees(2.0) >= compassVisibilityBearingThreshold)
        // A westward bearing wraps to its clockwise deviation.
        assertTrue(bearingOffNorthDegrees(350.0) >= compassVisibilityBearingThreshold)
        assertTrue(bearingOffNorthDegrees(-90.0) >= compassVisibilityBearingThreshold)
        assertEquals(180.0, bearingOffNorthDegrees(180.0), 0.0)
    }

    @Test
    fun `the trace cursor advances and saturates`() {
        val traceCursor = TraceCursor(3u)
        assertEquals(0, traceCursor.nextSampleIndex()!!.toInt())
        assertEquals(0, traceCursor.peek()!!.toInt())
        assertEquals(0, traceCursor.advance()!!.toInt())
        assertEquals(1, traceCursor.advance()!!.toInt())
        assertEquals(2, traceCursor.advance()!!.toInt())
        assertNull(traceCursor.advance())
        assertTrue(traceCursor.isFinished())
        assertEquals(0, traceCursor.remainingSampleCount().toInt())
    }

    @Test
    fun `the trace dispatch interval follows the acceleration factor`() {
        assertEquals(250L, traceDispatchIntervalMilliseconds(5.0, 20.0).toLong())
        assertEquals(1000L, traceDispatchIntervalMilliseconds(1.0, 1.0).toLong())
        assertEquals(1L, traceDispatchIntervalMilliseconds(0.0001, 1.0).toLong())
        assertEquals(1L, traceDispatchIntervalMilliseconds(5.0, 0.0).toLong())
        assertEquals(1L, traceDispatchIntervalMilliseconds(Double.NaN, 20.0).toLong())
    }
}

private const val ZOOM_ASSERTION_TOLERANCE: Double = 1e-9
