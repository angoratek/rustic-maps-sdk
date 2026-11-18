package com.rusticmaps.sdk

/**
 * The saveable form of a [MapView]'s camera.
 *
 * Android recreates a view after a configuration change or process death, and
 * a map that silently snapped back to the default pose would lose the user's
 * place. The view saves the adopted pose as five doubles and restores it
 * through its normal camera path, so the restored camera reports to the
 * listener exactly like a programmatic set. The codec is a plain value
 * transform with its own unit test; [MapView] only supplies the bundle keys.
 */
internal object MapViewRestorationCodec {
    /** Number of doubles in one encoded camera pose. */
    const val ENCODED_VALUE_COUNT: Int = 5

    /** Lowest latitude, in decimal degrees. */
    private const val MINIMUM_LATITUDE_DEGREES: Double = -90.0

    /** Highest latitude, in decimal degrees. */
    private const val MAXIMUM_LATITUDE_DEGREES: Double = 90.0

    /** Lowest longitude, in decimal degrees. */
    private const val MINIMUM_LONGITUDE_DEGREES: Double = -180.0

    /** Highest longitude, in decimal degrees. */
    private const val MAXIMUM_LONGITUDE_DEGREES: Double = 180.0

    /**
     * Encodes [pose] as `[latitude, longitude, zoom, bearing, pitch]`.
     *
     * @param pose the camera pose to encode.
     * @return the five-double encoding.
     */
    fun encode(pose: MapCameraPose): DoubleArray = doubleArrayOf(
        pose.latitudeDegrees,
        pose.longitudeDegrees,
        pose.zoomLevel,
        pose.bearingDegrees,
        pose.pitchDegrees,
    )

    /**
     * Decodes a pose saved by [encode], or null when the value is absent,
     * the wrong length, non-finite or outside the contract's coordinate
     * ranges. A malformed value restores nothing rather than adopting an
     * invalid camera.
     *
     * @param encoded the saved double array, possibly null.
     * @return the decoded pose, or null.
     */
    fun decode(encoded: DoubleArray?): MapCameraPose? {
        if (encoded == null || encoded.size != ENCODED_VALUE_COUNT) {
            return null
        }
        if (encoded.any { value -> !value.isFinite() }) {
            return null
        }
        val latitudeDegrees = encoded[0]
        val longitudeDegrees = encoded[1]
        if (latitudeDegrees !in MINIMUM_LATITUDE_DEGREES..MAXIMUM_LATITUDE_DEGREES) {
            return null
        }
        if (longitudeDegrees !in MINIMUM_LONGITUDE_DEGREES..MAXIMUM_LONGITUDE_DEGREES) {
            return null
        }
        return MapCameraPose(
            latitudeDegrees = latitudeDegrees,
            longitudeDegrees = longitudeDegrees,
            zoomLevel = encoded[2],
            bearingDegrees = encoded[3],
            pitchDegrees = encoded[4],
        )
    }
}
