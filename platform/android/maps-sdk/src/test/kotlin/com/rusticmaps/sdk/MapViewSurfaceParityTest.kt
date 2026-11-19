package com.rusticmaps.sdk

import java.lang.ref.WeakReference
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The native wrapper carries the same verb set, failure phases and listener
 * liveness as the other targets.
 *
 * The class is inspected through reflection because a JVM unit test cannot
 * construct the `SurfaceView`; that still pins the public members a Kotlin or
 * Java consumer compiles against.
 */
class MapViewSurfaceParityTest {
    @Test
    fun theCameraAndReadbackVerbSetIsPresent() {
        val mapViewClass = MapView::class.java
        assertNotNull(
            "the wrapper must expose the content-follows-fingers rotation verb",
            mapViewClass.getMethod("rotateByDegrees", Double::class.javaPrimitiveType),
        )
        listOf(
            "getHasDrawnFirstFrame",
            "getViewportSize",
            "getViewportPixelExtent",
            "getViewportWidthPixels",
            "getViewportHeightPixels",
            "getDisplayScale",
        ).forEach { readback ->
            assertNotNull(
                "the wrapper must expose the '$readback' readback",
                mapViewClass.getMethod(readback),
            )
        }
    }

    @Test
    fun theFailurePhaseSetMatchesTheOtherTargets() {
        assertEquals(
            listOf(
                "SURFACE",
                "RESIZE",
                "DRAW",
                "CAMERA",
                "OVERLAY",
                "LOCATION_INDICATOR",
                "BASE_MAP",
                "TAP",
                "LISTENER",
            ),
            MapViewFailurePhase.entries.map { phase -> phase.name },
        )
    }

    @Test
    fun theListenerIsHeldWeakly() {
        val listenerField = MapView::class.java.getDeclaredField("listenerReference")
        assertTrue(
            "the view must not retain its listener; a listener that captures its " +
                "owner would otherwise form a cycle",
            WeakReference::class.java.isAssignableFrom(listenerField.type),
        )
    }
}
