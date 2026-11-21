package com.angoratek.rusticmaps

import android.content.Context
import android.widget.FrameLayout
import com.rusticmaps.sdk.MapCameraPose
import com.rusticmaps.sdk.MapView
import com.rusticmaps.sdk.MapViewFailure
import com.rusticmaps.sdk.MapViewFailurePhase
import com.rusticmaps.sdk.MapViewListener
import com.facebook.react.bridge.Arguments
import com.facebook.react.bridge.WritableMap
import uniffi.rustic_maps.BoundingBox
import uniffi.rustic_maps.EdgeInsets
import uniffi.rustic_maps.FrameStatistics
import uniffi.rustic_maps.GeographicCoordinate
import uniffi.rustic_maps.LocationIndicatorState
import uniffi.rustic_maps.LocationIndicatorStyle
import uniffi.rustic_maps.MapOverlay
import uniffi.rustic_maps.MapSdk
import uniffi.rustic_maps.ProviderVectorTileSource
import uniffi.rustic_maps.UrlTemplateVectorTileSource
import uniffi.rustic_maps.VectorTileSourceStatus
import uniffi.rustic_maps.ViewportSize
import java.lang.ref.WeakReference

/**
 * One native observer of the host view's listener events (the reference
 * shell's demo session registers itself to mirror the camera readback and
 * break its follow camera on a gesture; the React Native event channel is
 * always registered too).
 */
interface MapsSdkMapViewObserver {
    /** The first frame drew and presented successfully. */
    fun onMapReady() {}

    /** The adopted camera changed (a set, animation step, gesture or fit). */
    fun onCameraChanged(camera: MapCameraPose, isGestureDriven: Boolean) {}

    /** A tap unprojected onto the ground plane; a miss reports nothing. */
    fun onMapClick(coordinate: GeographicCoordinate) {}

    /** An SDK or platform operation failed; the failure is never swallowed. */
    fun onError(phase: MapViewFailurePhase, message: String) {}

    /** One presented frame's tile and overlay accounting. */
    fun onFrameStatistics(statistics: FrameStatistics) {}
}

/**
 * The React Native package's native map host: a [FrameLayout] wrapping the
 * AAR's lifecycle-aware [MapView], padded by the safe-area insets JavaScript
 * passes (the same surface-extent rule the reference shell's host used), and
 * drawing from the package's one [MapsSdkNativeRuntime] SDK object.
 *
 * The host translates the SDK widget's listener into (a) the package's event
 * channel for JavaScript and (b) zero or more native observers. The widget
 * owns the surface, the frame pump, gestures, camera animation, overlays and
 * the location layer; this class owns only the bridge glue.
 */
class MapsSdkMapHostView(context: Context) : FrameLayout(context), MapViewListener {

    companion object {
        /** Live hosts keyed by the JavaScript-side stable identifier. */
        private val liveHostViews = mutableMapOf<String, WeakReference<MapsSdkMapHostView>>()

        /** The host registered under the identifier, or null. */
        fun find(viewIdentifier: String): MapsSdkMapHostView? {
            val reference = liveHostViews[viewIdentifier] ?: return null
            return reference.get()
        }

        /** The most recently registered live host (the reference shell's view). */
        fun currentHostView(): MapsSdkMapHostView? =
            liveHostViews.values.lastOrNull { reference -> reference.get() != null }?.get()
    }

    /** The AAR's lifecycle-aware map widget. */
    val mapView: MapView = MapView(context)

    private val observers = mutableListOf<MapsSdkMapViewObserver>()

    private var insetLeftPixels: Int = 0
    private var insetTopPixels: Int = 0
    private var insetRightPixels: Int = 0
    private var insetBottomPixels: Int = 0

    /** The JavaScript-side identifier events and commands are keyed on. */
    var viewIdentifier: String? = null
        set(value) {
            field?.let { previousIdentifier -> liveHostViews.remove(previousIdentifier) }
            field = value
            value?.let { currentIdentifier ->
                liveHostViews[currentIdentifier] = WeakReference(this)
            }
        }

    /** Whether touch gestures pan, pinch and rotate the camera. */
    var isGestureEnabled: Boolean
        get() = mapView.isGestureEnabled
        set(value) {
            mapView.isGestureEnabled = value
        }

    /**
     * Whether the widget has drawn and presented its first frame since the
     * current surface attached. It answers the same readiness the `mapReady`
     * event reports, so a JavaScript subscriber that attaches after the event
     * can still learn the map is ready.
     */
    var hasDrawnFirstFrame: Boolean = false
        private set

    init {
        addView(
            mapView,
            LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT),
        )
        mapView.listener = this
        MapsSdkNativeRuntime.registerHostView(this)
    }

    /** Attaches the package-owned SDK the widget draws from. */
    fun attachPackageSdk(sdk: MapSdk?) {
        if (mapView.sdk !== sdk) {
            mapView.sdk = sdk
        }
    }

    /** Registers one native observer (the React channel is independent). */
    fun addObserver(observer: MapsSdkMapViewObserver) {
        if (!observers.contains(observer)) {
            observers.add(observer)
        }
    }

    /** Removes one native observer. */
    fun removeObserver(observer: MapsSdkMapViewObserver) {
        observers.remove(observer)
    }

    // ------------------------------------------------------------------
    // Safe-area insets (physical pixels)
    // ------------------------------------------------------------------

    fun setTopBarInsetPixels(value: Int) {
        insetTopPixels = value
        applyInsets()
    }

    fun setBottomBarInsetPixels(value: Int) {
        insetBottomPixels = value
        applyInsets()
    }

    fun setLeftBarInsetPixels(value: Int) {
        insetLeftPixels = value
        applyInsets()
    }

    fun setRightBarInsetPixels(value: Int) {
        insetRightPixels = value
        applyInsets()
    }

    private fun applyInsets() {
        setPadding(insetLeftPixels, insetTopPixels, insetRightPixels, insetBottomPixels)
    }

    // ------------------------------------------------------------------
    // Imperative calls (the package module addresses the view by identifier)
    // ------------------------------------------------------------------

    fun setCamera(camera: MapCameraPose) {
        mapView.setCamera(camera)
    }

    fun animateCamera(camera: MapCameraPose, durationMilliseconds: Long) {
        mapView.animateCamera(camera, durationMilliseconds)
    }

    fun fitBounds(
        boundingBox: BoundingBox,
        edgeInsets: EdgeInsets,
        maximumZoomLevel: Double,
    ) {
        mapView.fitBounds(boundingBox, edgeInsets, maximumZoomLevel)
    }

    fun panByScreenPixels(deltaXPixels: Double, deltaYPixels: Double, isGestureDriven: Boolean) {
        mapView.panByScreenPixels(deltaXPixels, deltaYPixels, isGestureDriven)
    }

    fun zoomByLevels(deltaZoomLevels: Double) {
        mapView.zoomByLevels(deltaZoomLevels)
    }

    fun rotateByDegrees(clockwiseDegrees: Double) {
        mapView.rotateByDegrees(clockwiseDegrees)
    }

    fun addOverlay(overlayIdentifier: String, overlay: MapOverlay): Boolean =
        mapView.addOverlay(overlayIdentifier, overlay)

    fun updateOverlay(overlayIdentifier: String, overlay: MapOverlay): Boolean =
        mapView.updateOverlay(overlayIdentifier, overlay)

    fun removeOverlay(overlayIdentifier: String): Boolean =
        mapView.removeOverlay(overlayIdentifier)

    fun setOverlays(overlays: List<MapOverlay>): Boolean = mapView.setOverlays(overlays)

    fun clearOverlays(): Boolean = mapView.clearOverlays()

    fun setLocationIndicator(state: LocationIndicatorState, style: LocationIndicatorStyle) {
        mapView.setLocationIndicator(state, style)
    }

    fun clearLocationIndicator() {
        mapView.clearLocationIndicator()
    }

    // MARK: Vector base map

    /** Installs one styled vector base; answers whether the SDK accepted it. */
    fun setVectorTileSource(source: UrlTemplateVectorTileSource): Boolean =
        mapView.setVectorTileSource(source)

    /**
     * Installs a provider-backed base painted with the style document the
     * provider publishes; answers whether the SDK accepted it.
     */
    suspend fun setProviderVectorTileSourceFromDocument(
        source: ProviderVectorTileSource,
        styleDocumentUrl: String,
    ): Boolean = mapView.setProviderVectorTileSourceFromDocument(source, styleDocumentUrl)

    /** Removes the vector base; answers whether the SDK accepted the clear. */
    fun clearVectorTileSource(): Boolean = mapView.clearVectorTileSource()

    /** The vector base status, or null before the native view exists. */
    fun vectorTileSourceStatus(): VectorTileSourceStatus? = mapView.vectorTileSourceStatus()

    fun retryAttach() {
        mapView.retryAttach()
    }

    /** Re-resolves the raster-tile provider and drops the pooled tiles. */
    fun reloadTileSource() {
        mapView.reloadTileSource()
    }

    fun setRenderingPaused(paused: Boolean) {
        mapView.setRenderingPaused(paused)
    }

    /** The last adopted camera pose, or the widget's initial default. */
    fun camera(): MapCameraPose = mapView.camera

    /** Ground meters represented by one screen pixel, or null before attach. */
    fun groundMetersPerScreenPixel(): Double? = mapView.groundMetersPerScreenPixel()

    /** The render viewport width in pixels (zero before attach). */
    val viewportWidthPixels: Int
        get() = mapView.viewportWidthPixels

    /** The render viewport height in pixels (zero before attach). */
    val viewportHeightPixels: Int
        get() = mapView.viewportHeightPixels

    /** The capped physical-pixel extent the surface is configured to, or null before attach. */
    fun viewportPixelExtent(): ViewportSize? = mapView.viewportPixelExtent

    /** The display's device pixels per dp (the SDK's drawing-buffer scale). */
    val displayScale: Double
        get() = mapView.displayScale

    /** Releases the surface and the native objects (the manager's drop hook). */
    fun onDestroy() {
        MapsSdkNativeRuntime.unregisterHostView(this)
        viewIdentifier = null
        observers.clear()
        mapView.listener = null
        mapView.onDestroy()
        removeView(mapView)
    }

    // ------------------------------------------------------------------
    // MapViewListener: translate into the event channel and the observers
    // ------------------------------------------------------------------

    override fun onMapReady() {
        hasDrawnFirstFrame = true
        observers.toList().forEach { observer -> observer.onMapReady() }
        emitEvent("mapReady", null)
    }

    override fun onCameraChanged(camera: MapCameraPose, isGestureDriven: Boolean) {
        observers.toList().forEach { observer ->
            observer.onCameraChanged(camera, isGestureDriven)
        }
        val payload = Arguments.createMap()
        payload.putMap("camera", cameraToWritableMap(camera))
        payload.putBoolean("isGestureDriven", isGestureDriven)
        emitEvent("cameraChanged", payload)
    }

    override fun onMapClick(coordinate: GeographicCoordinate) {
        observers.toList().forEach { observer -> observer.onMapClick(coordinate) }
        val payload = Arguments.createMap()
        payload.putMap("coordinate", coordinateToWritableMap(coordinate))
        emitEvent("mapClick", payload)
    }

    override fun onError(failure: MapViewFailure) {
        val phaseLabel = failurePhaseLabel(failure.phase)
        val message = failure.message ?: failure.cause.toString()
        observers.toList().forEach { observer -> observer.onError(failure.phase, message) }
        val payload = Arguments.createMap()
        payload.putString("failurePhase", phaseLabel)
        payload.putString("failureMessage", message)
        emitEvent("error", payload)
    }

    override fun onFrameStatistics(statistics: FrameStatistics) {
        observers.toList().forEach { observer -> observer.onFrameStatistics(statistics) }
        // The JavaScript event carries the six accounting fields nested under
        // `statistics`, exactly like the iOS emitter: `MapView.tsx` reads
        // `nativeEvent.statistics`, so a flat payload renders no frames.
        val statisticsMap = Arguments.createMap()
        statisticsMap.putInt("requestedTileCount", statistics.requestedTileCount.toInt())
        statisticsMap.putInt("presentTileCount", statistics.presentTileCount.toInt())
        statisticsMap.putInt("fallbackTileCount", statistics.fallbackTileCount.toInt())
        statisticsMap.putInt("deferredTileCount", statistics.deferredTileCount.toInt())
        statisticsMap.putInt("unavailableTileCount", statistics.unavailableTileCount.toInt())
        statisticsMap.putInt("overlayTriangleCount", statistics.overlayTriangleCount.toInt())
        val payload = Arguments.createMap()
        payload.putMap("statistics", statisticsMap)
        emitEvent("frameStatistics", payload)
    }

    override fun onFrameDeferred() {
        emitEvent("frameDeferred", null)
    }

    override fun onFrameSettled() {
        emitEvent("frameSettled", null)
    }

    private fun emitEvent(eventName: String, payload: WritableMap?) {
        val currentIdentifier = viewIdentifier ?: return
        MapsSdkNativeEventChannel.emit(currentIdentifier, eventName, payload)
    }

    private fun cameraToWritableMap(camera: MapCameraPose): WritableMap {
        val cameraMap = Arguments.createMap()
        cameraMap.putDouble("latitudeDegrees", camera.latitudeDegrees)
        cameraMap.putDouble("longitudeDegrees", camera.longitudeDegrees)
        cameraMap.putDouble("zoomLevel", camera.zoomLevel)
        cameraMap.putDouble("bearingDegrees", camera.bearingDegrees)
        cameraMap.putDouble("pitchDegrees", camera.pitchDegrees)
        return cameraMap
    }

    private fun coordinateToWritableMap(coordinate: GeographicCoordinate): WritableMap {
        val coordinateMap = Arguments.createMap()
        coordinateMap.putDouble("latitude", coordinate.latitude)
        coordinateMap.putDouble("longitude", coordinate.longitude)
        return coordinateMap
    }

    private fun failurePhaseLabel(phase: MapViewFailurePhase): String = when (phase) {
        MapViewFailurePhase.SURFACE -> "surface"
        MapViewFailurePhase.RESIZE -> "resize"
        MapViewFailurePhase.DRAW -> "draw"
        MapViewFailurePhase.CAMERA -> "camera"
        MapViewFailurePhase.OVERLAY -> "overlay"
        MapViewFailurePhase.LOCATION_INDICATOR -> "locationIndicator"
        MapViewFailurePhase.BASE_MAP -> "baseMap"
        MapViewFailurePhase.TAP -> "tap"
        MapViewFailurePhase.LISTENER -> "listener"
    }
}
