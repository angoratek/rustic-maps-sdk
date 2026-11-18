package com.rusticmaps.sdk

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The JVM-facing shape of [MapViewListener]: every callback is a real default
 * method (not only a Kotlin bridge), and the compiler still emits the
 * `DefaultImpls` holder for callers compiled against the earlier shape. The
 * pair is what keeps a Java or partially-overriding implementation from hitting
 * an `AbstractMethodError`.
 */
class MapViewListenerContractTest {
    @Test
    fun everyCallbackIsAJvmDefaultMethod() {
        val listenerClass = MapViewListener::class.java
        listOf(
            listenerClass.getMethod("onMapReady"),
            listenerClass.getMethod("onCameraChanged", MapCameraPose::class.java, Boolean::class.javaPrimitiveType),
            listenerClass.getMethod("onMapClick", uniffi.rustic_maps.GeographicCoordinate::class.java),
            listenerClass.getMethod("onError", MapViewFailure::class.java),
            listenerClass.getMethod(
                "onFrameStatistics",
                uniffi.rustic_maps.FrameStatistics::class.java,
            ),
            listenerClass.getMethod("onFrameDeferred"),
            listenerClass.getMethod("onFrameSettled"),
        ).forEach { callback ->
            assertTrue(
                "listener callback '${callback.name}' must be a JVM default method",
                callback.isDefault,
            )
        }
    }

    @Test
    fun theCompatibilityBridgeExistsForAlreadyCompiledCallers() {
        val bridge = Class.forName("${MapViewListener::class.java.name}\$DefaultImpls")
        assertNotNull("the compatibility bridge must be generated", bridge)
        assertEquals(MapViewListener::class.java.name, bridge.enclosingClass?.name)
    }
}
