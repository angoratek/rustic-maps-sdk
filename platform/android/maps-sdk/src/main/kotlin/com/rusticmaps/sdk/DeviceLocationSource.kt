package com.rusticmaps.sdk

import android.Manifest
import android.annotation.SuppressLint
import android.content.Context
import android.content.pm.PackageManager
import android.location.Location
import android.location.LocationListener
import android.location.LocationManager
import android.os.Looper
import uniffi.rustic_maps.GeographicCoordinate
import uniffi.rustic_maps.PositionController
import uniffi.rustic_maps.PositionFix
import uniffi.rustic_maps.PositionSourceError
import uniffi.rustic_maps.SdkException

/**
 * Feeds the platform's location stream into the contract's
 * [PositionController].
 *
 * The application owns the runtime permission prompt (the manifest declares
 * `ACCESS_FINE_LOCATION` or `ACCESS_COARSE_LOCATION`) and calls [start] after
 * the user grants it. The source listens to the enabled `LocationManager`
 * providers on the main looper, converts every fix to the contract record and
 * pushes it into the controller; [stop] removes the updates. Permission and
 * service failures are reported to the controller as well as the application,
 * so the controller's state and the UI agree.
 *
 * The controller is passed in rather than created here: one controller per map
 * session, shared by the location indicator, the follow camera and the
 * navigator.
 */
class DeviceLocationSource(
    context: Context,
    val positionController: PositionController,
    private val onPositioningFailure: ((String) -> Unit)? = null,
    private val onFix: ((PositionFix) -> Unit)? = null,
    private val updateIntervalMilliseconds: Long = DEFAULT_UPDATE_INTERVAL_MILLISECONDS,
    private val minimumDistanceMeters: Float = DEFAULT_MINIMUM_DISTANCE_METERS,
) {
    private val applicationContext = context.applicationContext
    private val locationManager =
        applicationContext.getSystemService(Context.LOCATION_SERVICE) as? LocationManager
    private var locationListener: LocationListener? = null

    /** Whether the application holds a location permission this source can use. */
    fun hasLocationPermission(): Boolean =
        applicationContext.checkSelfPermission(Manifest.permission.ACCESS_FINE_LOCATION) ==
            PackageManager.PERMISSION_GRANTED ||
            applicationContext.checkSelfPermission(Manifest.permission.ACCESS_COARSE_LOCATION) ==
            PackageManager.PERMISSION_GRANTED

    /** Whether the source is currently listening for updates. */
    val isActive: Boolean
        get() = locationListener != null

    /**
     * Starts listening to the enabled providers. Answers false when the
     * permission is missing or no provider is enabled; the controller records
     * the matching source failure in both cases. A second call while active is
     * a no-op that answers true.
     */
    @SuppressLint("MissingPermission")
    // The permission is checked at the top of the function
    // (`hasLocationPermission`) and a race that revokes it between the check
    // and the registration is caught as SecurityException below; lint cannot
    // see either guard across the helper.
    fun start(): Boolean {
        if (isActive) {
            return true
        }
        if (!hasLocationPermission()) {
            positionController.sourceFailed(PositionSourceError.PERMISSION_DENIED)
            return false
        }
        val manager = locationManager
            ?: run {
                positionController.sourceFailed(PositionSourceError.UNAVAILABLE)
                return false
            }
        val enabledProviders = ENABLED_PROVIDER_NAMES.filter { providerName ->
            runCatching { manager.isProviderEnabled(providerName) }.getOrDefault(false)
        }
        if (enabledProviders.isEmpty()) {
            positionController.sourceFailed(PositionSourceError.SERVICE_DISABLED)
            return false
        }
        val listener = LocationListener { location -> deliver(location) }
        try {
            for (providerName in enabledProviders) {
                manager.requestLocationUpdates(
                    providerName,
                    updateIntervalMilliseconds,
                    minimumDistanceMeters,
                    listener,
                    Looper.getMainLooper(),
                )
            }
        } catch (securityException: SecurityException) {
            manager.removeUpdates(listener)
            positionController.sourceFailed(PositionSourceError.PERMISSION_DENIED)
            return false
        }
        locationListener = listener
        return true
    }

    /** Stops listening; a later [start] resumes the same controller. */
    fun stop() {
        val listener = locationListener ?: return
        locationManager?.removeUpdates(listener)
        locationListener = null
    }

    /** Pushes one platform fix into the controller. */
    private fun deliver(location: Location) {
        val positionFix = PositionFix(
            coordinate = GeographicCoordinate(
                latitude = location.latitude,
                longitude = location.longitude,
            ),
            horizontalAccuracyMeters = if (location.hasAccuracy()) {
                location.accuracy.toDouble()
            } else {
                null
            },
            courseHeadingDegrees = if (location.hasBearing()) {
                location.bearing.toDouble()
            } else {
                null
            },
            timestampSeconds = location.time / MILLISECONDS_PER_SECOND,
        )
        try {
            positionController.feedFix(positionFix)
            onFix?.invoke(positionFix)
        } catch (sdkException: SdkException) {
            onPositioningFailure?.invoke(sdkException.message ?: "the position fix was refused")
        }
    }

    private companion object {
        /** Providers the source listens to when the device enables them. */
        val ENABLED_PROVIDER_NAMES = listOf(
            LocationManager.GPS_PROVIDER,
            LocationManager.NETWORK_PROVIDER,
        )

        /** The shortest interval between delivered fixes. */
        const val DEFAULT_UPDATE_INTERVAL_MILLISECONDS: Long = 1_000

        /** The shortest movement between delivered fixes, in meters. */
        const val DEFAULT_MINIMUM_DISTANCE_METERS: Float = 1f

        /** Milliseconds in one second (the controller's timestamp unit). */
        const val MILLISECONDS_PER_SECOND: Double = 1_000.0
    }
}
