package com.rusticmaps.sdk

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** Unit cover for [MapViewRestorationCodec]: a valid pose round-trips and a malformed one is refused. */
class MapViewRestorationCodecTest {
    @Test
    fun aCameraPoseRoundTrips() {
        val pose = MapCameraPose(
            latitudeDegrees = 38.5816,
            longitudeDegrees = -121.4944,
            zoomLevel = 15.5,
            bearingDegrees = 42.0,
            pitchDegrees = 30.0,
        )

        val decoded = MapViewRestorationCodec.decode(MapViewRestorationCodec.encode(pose))

        assertEquals(pose, decoded)
    }

    @Test
    fun theEncodingKeepsTheDocumentedFieldOrder() {
        val pose = MapCameraPose(
            latitudeDegrees = 1.0,
            longitudeDegrees = 2.0,
            zoomLevel = 3.0,
            bearingDegrees = 4.0,
            pitchDegrees = 5.0,
        )

        assertArrayEquals(doubleArrayOf(1.0, 2.0, 3.0, 4.0, 5.0), MapViewRestorationCodec.encode(pose), 0.0)
    }

    @Test
    fun aMissingOrWrongLengthValueRestoresNothing() {
        assertNull(MapViewRestorationCodec.decode(null))
        assertNull(MapViewRestorationCodec.decode(DoubleArray(0)))
        assertNull(MapViewRestorationCodec.decode(DoubleArray(4)))
        assertNull(MapViewRestorationCodec.decode(DoubleArray(6)))
    }

    @Test
    fun aNonFiniteValueRestoresNothing() {
        val pose = MapCameraPose(0.0, 0.0, 2.0, 0.0, 0.0)
        val nanZoom = MapViewRestorationCodec.encode(pose).also { it[2] = Double.NaN }
        val infiniteLatitude = MapViewRestorationCodec.encode(pose).also { it[0] = Double.POSITIVE_INFINITY }

        assertNull(MapViewRestorationCodec.decode(nanZoom))
        assertNull(MapViewRestorationCodec.decode(infiniteLatitude))
    }

    @Test
    fun anOutOfRangeCoordinateRestoresNothing() {
        val pose = MapCameraPose(0.0, 0.0, 2.0, 0.0, 0.0)
        val outOfRangeLatitude = MapViewRestorationCodec.encode(pose).also { it[0] = 91.0 }
        val outOfRangeLongitude = MapViewRestorationCodec.encode(pose).also { it[1] = -181.0 }

        assertNull(MapViewRestorationCodec.decode(outOfRangeLatitude))
        assertNull(MapViewRestorationCodec.decode(outOfRangeLongitude))
    }
}
