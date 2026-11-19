package com.rusticmaps.sdk

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.rustic_maps.PositionController
import uniffi.rustic_maps.PositionFix

/**
 * The one acceptance rule both platform location sources apply: a fix with a
 * negative horizontal accuracy or an age beyond the controller's staleness
 * bound is dropped at the source, a fresh valid fix is forwarded, and a
 * controller refusal reaches the failure callback.
 */
class DeviceLocationSourceAcceptanceTest {
    @Test
    fun theAcceptanceRuleMatchesTheControllerAndTheFailureCallback() {
        val negativeAccuracyController = PositionController()
        deliver(negativeAccuracyController, horizontalAccuracyMeters = -1.0)
        assertNull(
            "a negative horizontal accuracy is dropped at the source",
            negativeAccuracyController.latestFix(),
        )

        val staleController = PositionController()
        deliver(
            staleController,
            timestampSeconds = NOW_SECONDS - staleController.staleAfterSeconds() - 1.0,
        )
        assertNull(
            "a fix beyond the controller's staleness bound is dropped at the source",
            staleController.latestFix(),
        )

        val freshController = PositionController()
        val acceptedFixes = mutableListOf<PositionFix>()
        deliver(freshController, onFix = { fix -> acceptedFixes.add(fix) })
        assertNotNull(
            "a fresh valid fix is forwarded to the controller",
            freshController.latestFix(),
        )
        assertEquals("the forwarded fix reaches the accepted-fix callback", 1, acceptedFixes.size)

        val refusingController = PositionController()
        val failures = mutableListOf<String>()
        deliver(
            refusingController,
            latitude = INVALID_LATITUDE_DEGREES,
            onPositioningFailure = { detail -> failures.add(detail) },
        )
        assertNull("the controller refuses an invalid coordinate", refusingController.latestFix())
        assertEquals(
            "the controller refusal reaches the failure callback once",
            1,
            failures.size,
        )
        assertTrue(failures.single().isNotBlank())
    }

    /** Feeds one fix at the demo coordinate through the shared delivery path. */
    private fun deliver(
        controller: PositionController,
        latitude: Double = DEMO_LATITUDE_DEGREES,
        longitude: Double = DEMO_LONGITUDE_DEGREES,
        horizontalAccuracyMeters: Double? = 5.0,
        timestampSeconds: Double = NOW_SECONDS,
        onPositioningFailure: ((String) -> Unit)? = null,
        onFix: ((PositionFix) -> Unit)? = null,
    ) {
        deliverPositionFix(
            positionController = controller,
            onPositioningFailure = onPositioningFailure,
            onFix = onFix,
            latitude = latitude,
            longitude = longitude,
            horizontalAccuracyMeters = horizontalAccuracyMeters,
            courseHeadingDegrees = null,
            timestampSeconds = timestampSeconds,
            nowSeconds = NOW_SECONDS,
        )
    }

    private companion object {
        const val DEMO_LATITUDE_DEGREES = 38.5766
        const val DEMO_LONGITUDE_DEGREES = -121.4944
        const val NOW_SECONDS = 1_000_000.0
        const val INVALID_LATITUDE_DEGREES = 100.0
    }
}
