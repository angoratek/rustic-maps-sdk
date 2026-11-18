// Host-JVM smoke test: exercises the generated Kotlin bindings through JNA
// against the Apple (`aarch64-apple-darwin`) cdylib built by
// scripts/build-android.sh. It proves the bindings load the library, pass
// records and check checksums; the Android `.so` libraries themselves are
// only loadable on Android, so an Android-runtime smoke stays an on-device
// concern for a later slice.
package com.rusticmaps.sdk.smoke

import uniffi.rustic_maps.BoundingBox
import uniffi.rustic_maps.EdgeInsets
import uniffi.rustic_maps.GeographicCoordinate
import uniffi.rustic_maps.MapCamera
import uniffi.rustic_maps.MapSdk
import uniffi.rustic_maps.MapView as NativeMapView
import uniffi.rustic_maps.SdkException
import uniffi.rustic_maps.ViewportSize
import uniffi.rustic_maps.coordinateToNormalizedWorld
import uniffi.rustic_maps.getSdkCompiledVersion
import uniffi.rustic_maps.tilesCovering

/// The viewport of the smoke projections, in physical pixels.
val SMOKE_VIEWPORT_PIXELS = ViewportSize(
    widthPixels = 640.0,
    heightPixels = 480.0,
)

/// Half the default camera field of view, in degrees.
const val SMOKE_FIELD_OF_VIEW_DEGREES = 60.0

fun main() {
    val compiledVersion = getSdkCompiledVersion()
    check(compiledVersion.isNotBlank()) {
        "the compiled SDK version must be readable through the FFI, got `$compiledVersion`"
    }

    val sanFrancisco = GeographicCoordinate(latitude = 37.77, longitude = -122.42)
    val normalized = coordinateToNormalizedWorld(sanFrancisco)
    check(normalized.x in 0.0..1.0 && normalized.y in 0.0..1.0) {
        "normalized world coordinate outside [0, 1]^2: ${normalized.x}, ${normalized.y}"
    }

    val tiles = tilesCovering(
        boundingBox = BoundingBox(
            southWest = GeographicCoordinate(latitude = 37.0, longitude = -123.0),
            northEast = GeographicCoordinate(latitude = 38.0, longitude = -122.0),
        ),
        zoomLevel = 4u,
    )
    check(tiles.isNotEmpty()) { "tile covering must return at least one tile" }

    MapCamera(
        center = sanFrancisco,
        zoomLevel = 12.0,
        bearingDegrees = 270.0,
        pitchDegrees = 0.0,
    ).use { camera ->
        val screenPoint = camera.projectToScreen(
            coordinate = sanFrancisco,
            viewport = SMOKE_VIEWPORT_PIXELS,
            fieldOfViewDegrees = SMOKE_FIELD_OF_VIEW_DEGREES,
        )
        checkNotNull(screenPoint) { "the camera center must project onto the viewport" }

        val ground = camera.screenToGroundCoordinate(
            screenPoint = screenPoint,
            viewport = SMOKE_VIEWPORT_PIXELS,
            fieldOfViewDegrees = SMOKE_FIELD_OF_VIEW_DEGREES,
        )
        checkNotNull(ground) { "the projected camera center must unproject back onto the ground" }
    }

    try {
        coordinateToNormalizedWorld(
            coordinate = GeographicCoordinate(latitude = 91.0, longitude = 0.0),
        )
        error("an out-of-range latitude must throw an SdkException")
    } catch (error: SdkException) {
        check(error is SdkException.InvalidCoordinateInput) { "expected the invalid-input variant, got $error" }
    }

    smokeCameraContract(sanFrancisco)
    smokeSurfaceViewCameraContract(sanFrancisco)

    println(
        "JVM smoke passed: version=$compiledVersion, " +
            "normalized=(${normalized.x}, ${normalized.y}), " +
            "tiles[0]=${tiles.firstOrNull()}, projections OK, camera contract OK",
    )
}

/**
 * Proves the additive camera members the platform wrappers now delegate to:
 * interpolation endpoints and midpoint, fit-bounds framing and its readback.
 */
private fun smokeCameraContract(sanFrancisco: GeographicCoordinate) {
    val startCamera = MapCamera(
        center = sanFrancisco,
        zoomLevel = 10.0,
        bearingDegrees = 350.0,
        pitchDegrees = 0.0,
    )
    val targetCamera = MapCamera(
        center = GeographicCoordinate(latitude = 38.0, longitude = -122.0),
        zoomLevel = 14.0,
        bearingDegrees = 10.0,
        pitchDegrees = 30.0,
    )
    try {
        MapCamera.interpolate(
            startCamera = startCamera,
            targetCamera = targetCamera,
            fraction = 0.0,
        ).use { startEndpoint ->
            check(startEndpoint.zoomLevel() == startCamera.zoomLevel()) {
                "fraction zero must answer the start camera verbatim"
            }
        }
        MapCamera.interpolate(
            startCamera = startCamera,
            targetCamera = targetCamera,
            fraction = 1.0,
        ).use { targetEndpoint ->
            check(targetEndpoint.zoomLevel() == targetCamera.zoomLevel()) {
                "fraction one must answer the target camera verbatim"
            }
        }
        MapCamera.interpolate(
            startCamera = startCamera,
            targetCamera = targetCamera,
            fraction = 0.5,
        ).use { midpoint ->
            check(midpoint.zoomLevel() in startCamera.zoomLevel()..targetCamera.zoomLevel()) {
                "the interpolation midpoint must lie between the endpoint zooms"
            }
            // The bearing takes the shortest arc across the wrap, so the
            // midpoint sits at north (within floating-point noise).
            val midpointBearing = midpoint.bearingDegrees()
            check(midpointBearing < 1.0 || midpointBearing > 359.0) {
                "350 to 10 degrees must pass through north, got $midpointBearing"
            }
        }

        val framingBox = BoundingBox(
            southWest = GeographicCoordinate(latitude = 37.0, longitude = -123.0),
            northEast = GeographicCoordinate(latitude = 38.0, longitude = -122.0),
        )
        MapCamera.fitBounds(
            boundingBox = framingBox,
            viewport = SMOKE_VIEWPORT_PIXELS,
            edgeInsets = EdgeInsets(left = 0.0, top = 0.0, right = 0.0, bottom = 0.0),
            maximumZoomLevel = 17.0,
        ).use { fitted ->
            check(fitted.zoomLevel() in 0.0..17.0) {
                "the fitted zoom must respect the maximum, got ${fitted.zoomLevel()}"
            }
            val fittedCenter = fitted.center()
            check(fittedCenter.latitude in 37.0..38.0 && fittedCenter.longitude in -123.0..-122.0) {
                "the fitted center must lie inside the framed box"
            }
        }
    } finally {
        startCamera.close()
        targetCamera.close()
    }
}

/**
 * Proves the surface view's own camera readback and the detached answers of
 * `fit_bounds` and `camera_geographic_bounds`, the members each platform
 * wrapper calls instead of keeping a pose.
 */
private fun smokeSurfaceViewCameraContract(sanFrancisco: GeographicCoordinate) {
    MapSdk().use { mapSdk ->
        NativeMapView(mapSdk).use { surfaceView ->
            MapCamera(
                center = sanFrancisco,
                zoomLevel = 11.5,
                bearingDegrees = 45.0,
                pitchDegrees = 0.0,
            ).use { adopted ->
                surfaceView.setCamera(adopted)
                surfaceView.camera().use { readback ->
                    check(readback.zoomLevel() == adopted.zoomLevel()) {
                        "the surface view must report the pose set_camera adopted"
                    }
                    check(readback.bearingDegrees() == adopted.bearingDegrees()) {
                        "the surface view must report the adopted bearing"
                    }
                }
            }
            val framingBox = BoundingBox(
                southWest = GeographicCoordinate(latitude = 37.0, longitude = -123.0),
                northEast = GeographicCoordinate(latitude = 38.0, longitude = -122.0),
            )
            check(
                surfaceView.fitBounds(
                    boundingBox = framingBox,
                    edgeInsets = EdgeInsets(left = 0.0, top = 0.0, right = 0.0, bottom = 0.0),
                    maximumZoomLevel = 17.0,
                ) == null,
            ) {
                "fit_bounds must answer null while no viewport is attached"
            }
            check(surfaceView.cameraGeographicBounds() == null) {
                "camera_geographic_bounds must answer null while no viewport is attached"
            }
        }
    }
}
