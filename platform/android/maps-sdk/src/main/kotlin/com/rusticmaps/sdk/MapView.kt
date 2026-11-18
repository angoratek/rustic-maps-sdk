package com.rusticmaps.sdk

import android.content.Context
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.Parcelable
import android.util.AttributeSet
import android.util.Log
import android.view.Choreographer
import android.view.GestureDetector
import android.view.MotionEvent
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.View
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineExceptionHandler
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.channels.Channel
import uniffi.rustic_maps.BoundingBox
import uniffi.rustic_maps.EdgeInsets
import uniffi.rustic_maps.FrameStatistics
import uniffi.rustic_maps.GeographicCoordinate
import uniffi.rustic_maps.LocationIndicatorState
import uniffi.rustic_maps.LocationIndicatorStyle
import uniffi.rustic_maps.MapCamera
import uniffi.rustic_maps.MapOverlay
import uniffi.rustic_maps.MapSdk
import uniffi.rustic_maps.MapView as NativeMapView
import uniffi.rustic_maps.ProviderVectorTileSource
import uniffi.rustic_maps.ScreenPoint
import uniffi.rustic_maps.SdkException
import uniffi.rustic_maps.SurfaceHandle
import uniffi.rustic_maps.UrlTemplateVectorTileSource
import uniffi.rustic_maps.VectorTileSourceStatus
import uniffi.rustic_maps.ViewportSize
import kotlin.math.atan2
import kotlin.math.hypot
import kotlin.math.log2
import kotlin.math.max
import kotlin.math.min

/**
 * The camera values a [MapView] reads back from the SDK's anchored camera.
 *
 * The SDK owns the math; this plain value is what the wrapper exposes so an
 * integrator never holds a `MapCamera` handle and never has to close one.
 * Zoom and pitch are the SDK's clamped values, and bearing is wrapped into
 * `[0, 360)` exactly as the contract constructor defines.
 */
data class MapCameraPose(
    /** Camera center latitude in decimal degrees. */
    val latitudeDegrees: Double,
    /** Camera center longitude in decimal degrees. */
    val longitudeDegrees: Double,
    /** Zoom level. */
    val zoomLevel: Double,
    /** Bearing in degrees clockwise from true north. */
    val bearingDegrees: Double,
    /** Pitch in degrees; zero is top-down. */
    val pitchDegrees: Double,
)

/** Which wrapper operation produced a [MapViewFailure]. */
enum class MapViewFailurePhase {
    /** Acquiring the native window or attaching the surface. */
    SURFACE,
    /** Reconfiguring an attached surface. */
    RESIZE,
    /** Drawing a frame or querying idle settlement. */
    DRAW,
    /** Reading, building or transforming the camera. */
    CAMERA,
    /** Pushing the overlay list. */
    OVERLAY,
    /** Pushing or clearing the location layer. */
    LOCATION_INDICATOR,
    /** Installing, clearing, reloading or reading the base-map sources. */
    BASE_MAP,
    /** Unprojecting a tap. */
    TAP,
    /**
     * An application listener callback threw. The wrapper isolates every
     * dispatch and reports the throwable here instead of letting it escape
     * into the frame pump or the main thread.
     */
    LISTENER,
}

/**
 * One refused [MapView] operation carrying its verbatim SDK failure.
 *
 * The wrapper never swallows an SDK exception: every failure reaches
 * [MapViewListener.onError] with the phase it occurred in and the original
 * throwable, so an app can present the SDK's own message.
 */
class MapViewFailure(
    /** The wrapper operation that failed. */
    val phase: MapViewFailurePhase,
    /** The verbatim SDK or platform failure (also reachable as `cause`). */
    override val cause: Throwable,
) : Exception(cause.message, cause)

/**
 * The callbacks a [MapView] delivers on the main thread.
 *
 * Every member has an empty default: an integrator overrides only what it
 * needs. The interface is compiled with JVM default methods: the module passes
 * `-jvm-default=no-compatibility` (real default methods, no generated
 * `DefaultImpls` holder), and [JvmDefaultWithCompatibility] on the interface
 * restores the compatibility bridge for callers compiled against the earlier
 * shape. A Java or Kotlin implementation that overrides only some members
 * therefore never hits an `AbstractMethodError`.
 */
@JvmDefaultWithCompatibility
interface MapViewListener {
    /** The first frame drew and presented successfully. */
    fun onMapReady() {}

    /**
     * The camera changed: a set, an animation step, a gesture or a fit.
     *
     * @param camera the adopted pose.
     * @param isGestureDriven true when a pan, pinch or rotate produced the
     *   change, which is the hook a follow camera uses to break on gesture.
     */
    fun onCameraChanged(camera: MapCameraPose, isGestureDriven: Boolean) {}

    /** A tap unprojected onto the ground plane; null misses report nothing. */
    fun onMapClick(coordinate: GeographicCoordinate) {}

    /** An SDK or platform operation failed; the failure is never swallowed. */
    fun onError(failure: MapViewFailure) {}

    /** One presented frame's tile and overlay accounting. */
    fun onFrameStatistics(statistics: FrameStatistics) {}

    /**
     * A frame was skipped because a draw or resize held the render state
     * (`RenderingSurfaceUnavailable` backpressure); the next tick retries.
     */
    fun onFrameDeferred() {}

    /** The pump paused because `isFrameSettled` answered true. */
    fun onFrameSettled() {}
}

/**
 * The lifecycle-aware Android map view shipped in the AAR: it owns the
 * platform surface, the surface-pointer shim, the Choreographer frame pump,
 * resize, the camera value store and animation, gestures and the overlay and
 * location-layer bookkeeping. The app owns only its widgets and its own
 * camera intent.
 *
 * ## Drawing
 *
 * There is no SDK-internal render loop: this view pumps explicit frames from
 * the Choreographer and follows the documented idle-settled discipline
 * (the public API guide §10):
 *
 * 1. resize when the capped pixel extent changed, never every frame;
 * 2. ask `isFrameSettled`; when it answers true the pump stops entirely
 *    instead of burning frames on an unchanged, fully presented view;
 * 3. otherwise draw with at most one draw in flight
 *    (`RenderingSurfaceUnavailable` retries on the next tick);
 * 4. report statistics, readiness and failures.
 *
 * The pump resumes on a camera set or animation, a gesture, an overlay or
 * location mutation, a surface create/change, window attach or visibility,
 * [setRenderingPaused]`(false)`, [retryAttach] and a one-second watchdog.
 * When only unavailable tiles remain with an unchanged pose, the pump drops
 * to that one-second retry cadence rather than spinning against the
 * provider.
 *
 * ## Lifecycle
 *
 * Hosts call [onStart], [onStop] and [onDestroy] from the matching lifecycle
 * callbacks (the Compose wrapper does this automatically). The view also
 * pauses on window detach or invisibility. [onDestroy] releases the surface
 * and the native objects; the view is not reusable afterwards.
 *
 * ## State restoration
 *
 * [onSaveInstanceState] saves the adopted camera beside the superclass state
 * and [onRestoreInstanceState] restores it through the normal camera path, so
 * a configuration change or process recreation keeps the user's place. The
 * platform restores a view only when it has a stable id: a host that relies on
 * automatic hierarchy restoration assigns one with [View.setId]; a host that
 * manages restoration itself can call the two methods directly.
 *
 * All members are expected on the main thread.
 */
class MapView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
    defStyleAttr: Int = 0,
) : SurfaceView(context, attrs, defStyleAttr), SurfaceHolder.Callback2 {

    // ------------------------------------------------------------------
    // Public state
    // ------------------------------------------------------------------

    /**
     * The SDK whose registered raster-tile provider the view draws from.
     *
     * Setting it creates the native view; setting it while attached rebuilds
     * the native view and re-attaches on the next pump tick. Null until the
     * host supplies one: the view shows its background surface and draws
     * nothing.
     */
    var sdk: MapSdk? = null
        set(value) {
            if (field === value) {
                return
            }
            field = value
            rebuildNativeMapView()
        }

    /**
     * The listener.
     *
     * Liveness policy: the view holds an ordinary strong reference until the
     * host replaces it or the view is destroyed ([onDestroy]). A weak
     * reference would let a listener the host still owns be collected
     * silently, leaving the map alive with no callbacks and no diagnostic;
     * a strong reference is safe because the view itself lives in the host's
     * view hierarchy, so the listener cannot outlive the host that owns both.
     * The counterpart of that guarantee is the documented lifecycle contract:
     * the host always calls [onDestroy] when it releases the view (the Compose
     * wrapper does so on release and on the host's destroy), and that call
     * clears the reference. A host that detaches the view without destroying
     * it keeps its listener by design, ready for reattachment; one that never
     * destroys the view retains it until it sets [listener] to null.
     */
    var listener: MapViewListener? = null

    /**
     * The one path every listener callback takes; a throwing callback is
     * isolated and reported instead of escaping into the pump.
     */
    private val listenerDispatcher: MapViewListenerDispatcher = MapViewListenerDispatcher(
        listenerProvider = { listener },
        failureFallback = { thrown ->
            Log.e(LOG_TAG, "a MapView listener threw while reporting an error", thrown)
        },
    )

    /** Whether touch gestures pan, pinch and rotate the camera. */
    var isGestureEnabled: Boolean = true

    /**
     * The camera the pump draws with; a projection of the contract camera
     * value the SDK holds, so the wrapper stores no pose of its own.
     */
    val camera: MapCameraPose
        get() = storedCamera?.let(::readCameraPose) ?: DEFAULT_CAMERA_POSE

    /**
     * The render viewport in pixels, or null before the surface attached.
     * This is the capped extent the frames render at, not the raw surface
     * extent; gestures and taps scale between the two.
     */
    val viewportSize: ViewportSize?
        get() = if (attachedWidthPixels > 0 && attachedHeightPixels > 0) {
            ViewportSize(
                widthPixels = attachedWidthPixels.toDouble(),
                heightPixels = attachedHeightPixels.toDouble(),
            )
        } else {
            null
        }

    /** The render viewport width in pixels (zero before attach). */
    val viewportWidthPixels: Int
        get() = attachedWidthPixels

    /** The render viewport height in pixels (zero before attach). */
    val viewportHeightPixels: Int
        get() = attachedHeightPixels

    // ------------------------------------------------------------------
    // Surface and native view state
    // ------------------------------------------------------------------

    private var nativeMapView: NativeMapView? = null
    /**
     * The vector base source installed before the native view existed; the
     * native view is rebuilt on every SDK swap, and the contract keeps the
     * source across detach/attach, so the wrapper re-applies it like the
     * overlay list. The queue also owns the rollback of a refused queued
     * install.
     */
    private val vectorBaseSourceQueue: VectorBaseSourceQueue = VectorBaseSourceQueue()
    private var availableSurface: Surface? = null
    private var surfaceWidthPixels: Int = 0
    private var surfaceHeightPixels: Int = 0
    private var attachedWidthPixels: Int = 0
    private var attachedHeightPixels: Int = 0
    private var acquiredNativeWindowPointer: Long = 0L
    private var requiresAttach: Boolean = false
    private var retryAttachRequested: Boolean = false
    private var isAttached: Boolean = false

    // ------------------------------------------------------------------
    // Camera state
    // ------------------------------------------------------------------

    /**
     * The contract camera the view holds. It is the single source of the
     * camera state: every readback, gesture and animation replaces it, and
     * the native view receives the same value through `set_camera`. Null
     * until the first camera intent, which reads as the contract's neutral
     * default pose.
     */
    private var storedCamera: MapCamera? = null
    private var cameraRevision: Long = 0L
    private var cameraAnimation: CameraAnimation? = null

    // ------------------------------------------------------------------
    // Overlay and location state
    // ------------------------------------------------------------------

    private var overlayDescriptors: List<MapOverlay> = emptyList()
    /** The identifier-addressed overlay bookkeeping; every mutation rolls back a refused push. */
    private val overlayIdBookkeeping: OverlayIdBookkeeping = OverlayIdBookkeeping()
    private var locationIndicatorState: LocationIndicatorState? = null
    private var locationIndicatorStyle: LocationIndicatorStyle? = null

    // ------------------------------------------------------------------
    // Pump state
    // ------------------------------------------------------------------

    /**
     * The last-resort reporter for a throwable that escapes the pump's own
     * per-frame isolation (the consumer loop). Reporting it keeps the
     * supervisor scope alive, so the next resume trigger starts a fresh
     * consumer instead of leaving the pump permanently dead.
     */
    private val pumpExceptionHandler: CoroutineExceptionHandler = CoroutineExceptionHandler { _, thrown ->
        if (thrown !is CancellationException) {
            reportFailure(MapViewFailurePhase.DRAW, thrown)
        }
    }
    private val pumpScope: CoroutineScope =
        CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate + pumpExceptionHandler)
    private val frameChannel: Channel<Long> = Channel(Channel.CONFLATED)
    /** The isolated consumer loop; it reports a throwing frame and keeps rendering. */
    private val pumpConsumer: FramePumpConsumer = FramePumpConsumer(
        pumpScope = pumpScope,
        frameChannel = frameChannel,
        canPump = ::canPump,
        pumpOneFrame = ::pumpOneFrame,
        reportFailure = { thrown -> reportFailure(MapViewFailurePhase.DRAW, thrown) },
    )
    private val frameCallback: Choreographer.FrameCallback = object : Choreographer.FrameCallback {
        override fun doFrame(frameTimeNanos: Long) {
            if (!isFrameCallbackPosted) {
                return
            }
            // Re-post before feeding the consumer: a consumer that pauses the
            // pump inside `trySend` removes this just-posted callback, so no
            // stale frame is left queued.
            choreographerInstance.postFrameCallback(this)
            frameChannel.trySend(frameTimeNanos)
        }
    }
    private val watchdogRunnable: Runnable = Runnable { resumePump(resetBackoff = false) }
    private val mainHandler: Handler = Handler(Looper.getMainLooper())
    private var isFrameCallbackPosted: Boolean = false
    private var isDrawInFlight: Boolean = false
    private var isBackoffActive: Boolean = false
    private var consecutiveUnavailableOnlyFrames: Int = 0
    private var hasReportedMapReady: Boolean = false
    private var isRenderingPaused: Boolean = false
    private var isLifecycleStarted: Boolean = true
    private var isWindowAttached: Boolean = false
    private var isWindowVisible: Boolean = true
    private var isDestroyed: Boolean = false

    // ------------------------------------------------------------------
    // Gesture state
    // ------------------------------------------------------------------

    private var lastPanX: Float = 0f
    private var lastPanY: Float = 0f
    private var hasTwoPointerBaseline: Boolean = false
    private var lastSpanPixels: Double = 0.0
    private var lastAngleRadians: Double = 0.0

    private val tapDetector: GestureDetector = GestureDetector(
        context,
        object : GestureDetector.SimpleOnGestureListener() {
            override fun onDown(event: MotionEvent): Boolean = true

            // Confirmed, not `onSingleTapUp`: a double tap must zoom without
            // also reporting a map click for its first tap.
            override fun onSingleTapConfirmed(event: MotionEvent): Boolean {
                reportTapAt(event.x.toDouble(), event.y.toDouble())
                return true
            }

            override fun onDoubleTap(event: MotionEvent): Boolean {
                if (viewportSize == null) {
                    return false
                }
                transformCamera(
                    zoomLevels = DOUBLE_TAP_ZOOM_STEP,
                    focalPoint = ScreenPoint(
                        x = surfacePositionToViewport(
                            event.x.toDouble(),
                            surfaceWidthPixels,
                            attachedWidthPixels,
                        ),
                        y = surfacePositionToViewport(
                            event.y.toDouble(),
                            surfaceHeightPixels,
                            attachedHeightPixels,
                        ),
                    ),
                )
                return true
            }
        },
    )

    init {
        holder.addCallback(this)
        isFocusable = true
        isClickable = true
    }

    // ------------------------------------------------------------------
    // Camera API
    // ------------------------------------------------------------------

    /** Sets the camera immediately; a refused pose reports and keeps the old one. */
    fun setCamera(camera: MapCameraPose) {
        cancelCameraAnimation()
        val adoptedCamera = cameraFromPoseOrNull(camera) ?: return
        adoptCamera(adoptedCamera, isGestureDriven = false)
    }

    /**
     * Animates the camera to [camera] over [durationMillis] through the
     * contract's interpolation curve. A non-positive duration is an immediate
     * set. A gesture cancel drops the animation where it stands.
     */
    fun animateCamera(
        camera: MapCameraPose,
        durationMillis: Long = DEFAULT_CAMERA_ANIMATION_DURATION_MILLIS,
    ) {
        val targetCamera = cameraFromPoseOrNull(camera) ?: return
        cancelCameraAnimation()
        if (durationMillis <= 0L) {
            adoptCamera(targetCamera, isGestureDriven = false)
            return
        }
        val startCamera = cameraFromPoseOrNull(this.camera)
        if (startCamera == null) {
            targetCamera.close()
            return
        }
        cameraAnimation = CameraAnimation(
            startNanos = System.nanoTime(),
            durationNanos = durationMillis * NANOS_PER_MILLISECOND,
            fromCamera = startCamera,
            toCamera = targetCamera,
        )
        resumePump(resetBackoff = true)
    }

    /**
     * Frames [boundingBox] inside the attached viewport's inset region through
     * the contract's `fit_bounds` member (top-down and north-up by
     * construction) and adopts the fitted camera. Without an attached viewport
     * the call changes nothing.
     */
    fun fitBounds(
        boundingBox: BoundingBox,
        edgeInsets: EdgeInsets = ZERO_EDGE_INSETS,
        maximumZoomLevel: Double = DEFAULT_FIT_BOUNDS_MAXIMUM_ZOOM_LEVEL,
    ) {
        val nativeView = nativeMapView ?: return
        val fittedCamera = try {
            nativeView.fitBounds(
                boundingBox = boundingBox,
                edgeInsets = edgeInsets,
                maximumZoomLevel = maximumZoomLevel,
            )
        } catch (failure: Throwable) {
            if (failure is CancellationException) {
                throw failure
            }
            reportFailure(MapViewFailurePhase.CAMERA, failure)
            return
        }
        if (fittedCamera != null) {
            adoptCamera(
                fittedCamera,
                isGestureDriven = false,
                syncToNativeView = false,
            )
        }
    }

    /**
     * Ground meters represented by one screen pixel at the camera center
     * (the scale bar's helper), or null before the surface attached.
     */
    fun groundMetersPerScreenPixel(): Double? {
        val viewport = viewportSize ?: return null
        val currentCamera = currentCameraOrNull() ?: return null
        return try {
            currentCamera.groundMetersPerScreenPixel(
                viewport = viewport,
                fieldOfViewDegrees = DEFAULT_FIELD_OF_VIEW_DEGREES,
            )
        } catch (failure: Throwable) {
            if (failure is CancellationException) {
                throw failure
            }
            reportFailure(MapViewFailurePhase.CAMERA, failure)
            null
        }
    }

    /**
     * The geographic bounds of the attached viewport's current camera pose,
     * or null before the surface attached or when the projection cannot
     * answer.
     *
     * The contract unprojects the four viewport corners through the current
     * camera; a steep pitch can put the far corner below the horizon, in
     * which case the camera's own ground scale bounds a box around its
     * center. The traffic layer asks for these bounds once the camera
     * settles.
     */
    fun cameraGeographicBounds(): BoundingBox? {
        val nativeView = nativeMapView ?: return null
        return try {
            nativeView.cameraGeographicBounds()
        } catch (failure: Throwable) {
            if (failure is CancellationException) {
                throw failure
            }
            reportFailure(MapViewFailurePhase.CAMERA, failure)
            null
        }
    }

    /**
     * Pans by a screen-pixel delta through the contract camera's
     * `panByScreenPixels` (the programmatic twin of a drag).
     */
    fun panByScreenPixels(
        deltaXPixels: Double,
        deltaYPixels: Double,
        isGestureDriven: Boolean = true,
    ) {
        transformCamera(
            panXPixels = deltaXPixels,
            panYPixels = deltaYPixels,
            isGestureDriven = isGestureDriven,
        )
    }

    /**
     * Zooms by [deltaZoomLevels] around the viewport's center — the control
     * cluster's zoom buttons go through the same contract member as a pinch.
     */
    fun zoomByLevels(deltaZoomLevels: Double) {
        val viewport = viewportSize ?: return
        transformCamera(
            zoomLevels = deltaZoomLevels,
            focalPoint = ScreenPoint(
                x = viewport.widthPixels / 2.0,
                y = viewport.heightPixels / 2.0,
            ),
            isGestureDriven = false,
        )
    }

    // ------------------------------------------------------------------
    // Overlay API (addressed by caller id)
    // ------------------------------------------------------------------

    /**
     * Adds or replaces the overlay held under [id]; answers whether it was
     * accepted. A refusal restores the bookkeeping exactly as it was.
     */
    fun addOverlay(id: String, overlay: MapOverlay): Boolean =
        overlayIdBookkeeping.add(id, overlay, ::replaceOverlays)

    /**
     * Replaces an existing overlay under [id] and answers whether it was
     * accepted; an unknown id is a caller error. A refusal restores the
     * bookkeeping exactly as it was.
     */
    fun updateOverlay(id: String, overlay: MapOverlay): Boolean {
        require(overlayIdBookkeeping.contains(id)) {
            "no overlay is registered under the id '$id'; use addOverlay for a new id"
        }
        return overlayIdBookkeeping.update(id, overlay, ::replaceOverlays)
    }

    /**
     * Removes the overlay under [id] (an unknown id is a no-op); answers
     * acceptance. A refusal keeps the overlay in the bookkeeping, matching
     * the list the SDK still draws.
     */
    fun removeOverlay(id: String): Boolean =
        overlayIdBookkeeping.remove(id, ::replaceOverlays)

    /**
     * Replaces the whole overlay list and resets the id bookkeeping: later
     * [addOverlay]/[updateOverlay]/[removeOverlay] calls start from an empty
     * id map. Answers whether the SDK accepted the list; a refusal restores
     * the previous bookkeeping.
     */
    fun setOverlays(overlays: List<MapOverlay>): Boolean {
        val previousBookkeeping = overlayIdBookkeeping.snapshot()
        overlayIdBookkeeping.clear()
        if (replaceOverlays(overlays)) {
            return true
        }
        overlayIdBookkeeping.restore(previousBookkeeping)
        return false
    }

    /**
     * Clears every overlay and the id bookkeeping; answers acceptance. A
     * refusal restores the previous bookkeeping.
     */
    fun clearOverlays(): Boolean {
        val previousBookkeeping = overlayIdBookkeeping.snapshot()
        overlayIdBookkeeping.clear()
        if (replaceOverlays(emptyList())) {
            return true
        }
        overlayIdBookkeeping.restore(previousBookkeeping)
        return false
    }

    // ------------------------------------------------------------------
    // Location layer API
    // ------------------------------------------------------------------

    /** Sets the renderer-owned location layer; a refused push reports. */
    fun setLocationIndicator(state: LocationIndicatorState, style: LocationIndicatorStyle) {
        locationIndicatorState = state
        locationIndicatorStyle = style
        val nativeView = nativeMapView ?: return
        try {
            nativeView.setLocationIndicator(state, style)
            resumePump(resetBackoff = true)
        } catch (failure: Throwable) {
            if (failure is CancellationException) {
                throw failure
            }
            reportFailure(MapViewFailurePhase.LOCATION_INDICATOR, failure)
        }
    }

    /** Clears the location layer; a refused clear reports. */
    fun clearLocationIndicator() {
        locationIndicatorState = null
        locationIndicatorStyle = null
        val nativeView = nativeMapView ?: return
        try {
            nativeView.clearLocationIndicator()
            resumePump(resetBackoff = true)
        } catch (failure: Throwable) {
            if (failure is CancellationException) {
                throw failure
            }
            reportFailure(MapViewFailurePhase.LOCATION_INDICATOR, failure)
        }
    }

    // ------------------------------------------------------------------
    // Vector base map API
    // ------------------------------------------------------------------

    /**
     * Installs a vector base map: a provider-neutral vector tileset painted
     * with one of the SDK's committed styles, drawn in place of the raster
     * tiles (the raster layer is hidden while a vector base serves) and
     * beneath every overlay. The source outlives attach/detach cycles, so an
     * install before the surface exists is carried into the first frame.
     * Answers whether the SDK accepted it; a refusal reports through
     * [MapViewListener.onError] and leaves the previously installed base in
     * force.
     */
    fun setVectorTileSource(source: UrlTemplateVectorTileSource): Boolean {
        // The stored source survives a native-view rebuild (an SDK swap), so
        // it is recorded before the push and rolled back by a refusal.
        val nativeView = nativeMapView
        if (nativeView == null) {
            vectorBaseSourceQueue.record(source)
            return true
        }
        return try {
            vectorBaseSourceQueue.select(source, isDrawInFlight) { nextSource ->
                if (nextSource == null) {
                    nativeView.clearVectorTileSource()
                } else {
                    nativeView.setVectorTileSource(nextSource)
                }
            }
            resumePump(resetBackoff = true)
            true
        } catch (failure: Throwable) {
            if (failure is CancellationException) {
                throw failure
            }
            reportFailure(MapViewFailurePhase.BASE_MAP, failure)
            false
        }
    }

    /**
     * Installs a provider-backed vector base painted with the style document
     * the provider publishes. The provider must be registered for vector
     * tiles first, and the document is fetched through that provider's
     * authenticated client, so the application never handles the credential.
     * Answers whether the SDK accepted it; a refusal reports through
     * [MapViewListener.onError] and leaves the previously installed base in
     * force.
     */
    suspend fun setProviderVectorTileSourceFromDocument(
        source: ProviderVectorTileSource,
        styleDocumentUrl: String,
    ): Boolean {
        val nativeView = nativeMapView ?: return false
        return try {
            nativeView.setProviderVectorTileSourceFromDocument(source, styleDocumentUrl)
            resumePump(resetBackoff = true)
            true
        } catch (failure: Throwable) {
            if (failure is CancellationException) {
                throw failure
            }
            reportFailure(MapViewFailurePhase.BASE_MAP, failure)
            false
        }
    }

    /** Removes the vector base map; answers whether the SDK accepted the clear. */
    fun clearVectorTileSource(): Boolean {
        val nativeView = nativeMapView
        if (nativeView == null) {
            vectorBaseSourceQueue.record(null)
            return true
        }
        return try {
            vectorBaseSourceQueue.select(null, isDrawInFlight) { nextSource ->
                if (nextSource == null) {
                    nativeView.clearVectorTileSource()
                } else {
                    nativeView.setVectorTileSource(nextSource)
                }
            }
            resumePump(resetBackoff = true)
            true
        } catch (failure: Throwable) {
            if (failure is CancellationException) {
                throw failure
            }
            reportFailure(MapViewFailurePhase.BASE_MAP, failure)
            false
        }
    }

    /**
     * The vector base source's status — the configured style, name and
     * attribution plus the last frame's drawn and failed tile counts — or
     * null before the native view exists.
     */
    fun vectorTileSourceStatus(): VectorTileSourceStatus? =
        try {
            nativeMapView?.vectorTileSourceStatus()
        } catch (failure: Throwable) {
            if (failure is CancellationException) {
                throw failure
            }
            reportFailure(MapViewFailurePhase.BASE_MAP, failure)
            null
        }

    // ------------------------------------------------------------------
    // Lifecycle API
    // ------------------------------------------------------------------

    /** Host lifecycle start: resumes the pump. */
    fun onStart() {
        if (isDestroyed) {
            return
        }
        isLifecycleStarted = true
        resumePump(resetBackoff = true)
    }

    /** Host lifecycle stop: pauses the pump but keeps the surface attached. */
    fun onStop() {
        isLifecycleStarted = false
        stopFrameCallback()
        cancelWatchdog()
    }

    /**
     * Host lifecycle destroy: detaches, releases the native window and closes
     * the native view. Call it from the host's `onDestroy`; the view is not
     * reusable afterwards.
     */
    fun onDestroy() {
        if (isDestroyed) {
            return
        }
        isDestroyed = true
        isLifecycleStarted = false
        stopFrameCallback()
        cancelWatchdog()
        detachSurface()
        closeNativeMapView()
        cancelCameraAnimation()
        closeStoredCamera()
        pumpScope.cancel()
        holder.removeCallback(this)
        // Explicit liveness policy: a destroyed view holds no listener (a
        // later call cannot deliver into a torn-down host).
        listener = null
    }

    /**
     * Pauses or resumes drawing explicitly (a host may release the GPU
     * surface in the background). Visibility and lifecycle pauses are
     * automatic and independent.
     */
    fun setRenderingPaused(paused: Boolean) {
        if (isRenderingPaused == paused) {
            return
        }
        isRenderingPaused = paused
        if (paused) {
            stopFrameCallback()
            cancelWatchdog()
        } else {
            resumePump(resetBackoff = true)
        }
    }

    /**
     * Re-resolves the SDK's raster-tile provider and drops every pooled tile,
     * so a provider, credential or source change reaches the next frame
     * without detaching the surface. The attached view holds its provider and
     * keeps its tiles by coordinate, so without this call a base-map switch
     * leaves the previous imagery on screen until the camera moves onto
     * unfetched tiles.
     *
     * The camera, overlays, location indicator and listeners keep their state,
     * unlike an attach/detach cycle. A failure surfaces through
     * [MapViewListener.onError].
     */
    fun reloadTileSource() {
        if (isDestroyed) {
            return
        }
        val nativeView = nativeMapView ?: return
        try {
            nativeView.reloadTileSource()
            resumePump(resetBackoff = true)
        } catch (failure: Throwable) {
            if (failure is CancellationException) {
                throw failure
            }
            reportFailure(MapViewFailurePhase.BASE_MAP, failure)
        }
    }

    /** Retries a failed attach; the error surfaced through [MapViewListener.onError]. */
    fun retryAttach() {
        if (isDestroyed || availableSurface == null) {
            return
        }
        requiresAttach = true
        retryAttachRequested = true
        resumePump(resetBackoff = true)
    }

    // ------------------------------------------------------------------
    // View lifecycle overrides
    // ------------------------------------------------------------------

    /**
     * Saves the adopted camera pose beside the superclass state, so a
     * configuration change or process recreation restores the user's place.
     */
    override fun onSaveInstanceState(): Parcelable =
        Bundle().apply {
            putParcelable(SUPER_STATE_KEY, super.onSaveInstanceState())
            putDoubleArray(CAMERA_POSE_STATE_KEY, MapViewRestorationCodec.encode(camera))
        }

    /**
     * Restores the superclass state and the saved camera pose. A missing or
     * malformed pose restores only the superclass state (the view keeps its
     * current pose).
     */
    override fun onRestoreInstanceState(state: Parcelable?) {
        if (state is Bundle) {
            @Suppress("DEPRECATION")
            super.onRestoreInstanceState(state.getParcelable(SUPER_STATE_KEY))
            MapViewRestorationCodec.decode(state.getDoubleArray(CAMERA_POSE_STATE_KEY))?.let { pose ->
                applyCameraPose(pose, isGestureDriven = false)
            }
            return
        }
        super.onRestoreInstanceState(state)
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        isWindowAttached = true
        isWindowVisible = windowVisibility == View.VISIBLE
        if (isWindowVisible) {
            resumePump(resetBackoff = true)
        }
    }

    override fun onDetachedFromWindow() {
        isWindowAttached = false
        stopFrameCallback()
        cancelWatchdog()
        detachSurface()
        super.onDetachedFromWindow()
    }

    override fun onWindowVisibilityChanged(visibility: Int) {
        super.onWindowVisibilityChanged(visibility)
        isWindowVisible = visibility == View.VISIBLE
        if (isWindowVisible) {
            resumePump(resetBackoff = true)
        } else {
            stopFrameCallback()
            cancelWatchdog()
        }
    }

    // ------------------------------------------------------------------
    // SurfaceHolder.Callback2
    //
    // The callback members are abstract on `SurfaceHolder.Callback2` itself
    // and `SurfaceView` supplies no implementation of them, so these overrides
    // have no superclass body to run (unlike the View and Activity lifecycle
    // overrides, which always call super).
    // ------------------------------------------------------------------

    override fun surfaceCreated(holder: SurfaceHolder) {
        availableSurface = holder.surface
        surfaceWidthPixels = holder.surfaceFrame.width()
        surfaceHeightPixels = holder.surfaceFrame.height()
        requiresAttach = true
        retryAttachRequested = true
        isAttached = false
        resumePump(resetBackoff = true)
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        availableSurface = holder.surface
        surfaceWidthPixels = width
        surfaceHeightPixels = height
        if (!isAttached) {
            requiresAttach = true
            retryAttachRequested = true
        }
        resumePump(resetBackoff = true)
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        // After this callback returns the surface is invalid: stop the pump,
        // detach synchronously and only then release the acquired window.
        stopFrameCallback()
        cancelWatchdog()
        detachSurface()
        availableSurface = null
        requiresAttach = false
    }

    override fun surfaceRedrawNeeded(holder: SurfaceHolder) {
        resumePump(resetBackoff = true)
    }

    override fun surfaceRedrawNeededAsync(holder: SurfaceHolder, drawingFinished: Runnable) {
        resumePump(resetBackoff = true)
        drawingFinished.run()
    }

    // ------------------------------------------------------------------
    // Touch handling
    // ------------------------------------------------------------------

    override fun onTouchEvent(event: MotionEvent): Boolean {
        if (!isGestureEnabled) {
            return super.onTouchEvent(event)
        }
        tapDetector.onTouchEvent(event)
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                // A gesture is a new camera intent: it takes over from a
                // running animation rather than fighting it frame by frame.
                cancelCameraAnimation()
                lastPanX = event.x
                lastPanY = event.y
                hasTwoPointerBaseline = false
            }
            MotionEvent.ACTION_POINTER_DOWN -> {
                cancelCameraAnimation()
                if (event.pointerCount >= TWO_POINTER_COUNT) {
                    captureTwoPointerBaseline(event)
                }
            }
            MotionEvent.ACTION_MOVE -> {
                if (event.pointerCount >= TWO_POINTER_COUNT) {
                    handleTwoPointerMove(event)
                } else {
                    val deltaXPixels = event.x - lastPanX
                    val deltaYPixels = event.y - lastPanY
                    lastPanX = event.x
                    lastPanY = event.y
                    if (deltaXPixels != 0f || deltaYPixels != 0f) {
                        transformCamera(
                            panXPixels = surfaceDeltaToViewport(deltaXPixels.toDouble(), surfaceWidthPixels, attachedWidthPixels),
                            panYPixels = surfaceDeltaToViewport(deltaYPixels.toDouble(), surfaceHeightPixels, attachedHeightPixels),
                        )
                    }
                }
            }
            MotionEvent.ACTION_POINTER_UP -> {
                if (event.pointerCount - 1 >= TWO_POINTER_COUNT) {
                    captureTwoPointerBaseline(event, removedPointerIndex = event.actionIndex)
                } else {
                    hasTwoPointerBaseline = false
                    val remainingIndex = if (event.actionIndex == 0) 1 else 0
                    lastPanX = event.getX(remainingIndex)
                    lastPanY = event.getY(remainingIndex)
                }
            }
            MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                hasTwoPointerBaseline = false
            }
        }
        return true
    }

    private fun captureTwoPointerBaseline(event: MotionEvent, removedPointerIndex: Int = -1) {
        val remainingIndices = (0 until event.pointerCount).filter { index ->
            index != removedPointerIndex
        }
        if (remainingIndices.size < TWO_POINTER_COUNT) {
            hasTwoPointerBaseline = false
            return
        }
        val firstIndex = remainingIndices[0]
        val secondIndex = remainingIndices[1]
        val deltaXPixels = event.getX(secondIndex) - event.getX(firstIndex)
        val deltaYPixels = event.getY(secondIndex) - event.getY(firstIndex)
        lastSpanPixels = hypot(deltaXPixels.toDouble(), deltaYPixels.toDouble())
        lastAngleRadians = atan2(deltaYPixels.toDouble(), deltaXPixels.toDouble())
        hasTwoPointerBaseline = true
    }

    private fun handleTwoPointerMove(event: MotionEvent) {
        if (!hasTwoPointerBaseline) {
            captureTwoPointerBaseline(event)
            return
        }
        val firstIndex = 0
        val secondIndex = 1
        val deltaXPixels = event.getX(secondIndex) - event.getX(firstIndex)
        val deltaYPixels = event.getY(secondIndex) - event.getY(firstIndex)
        val spanPixels = hypot(deltaXPixels.toDouble(), deltaYPixels.toDouble())
        val angleRadians = atan2(deltaYPixels.toDouble(), deltaXPixels.toDouble())
        if (lastSpanPixels < MINIMUM_PINCH_SPAN_PIXELS || spanPixels < MINIMUM_PINCH_SPAN_PIXELS) {
            lastSpanPixels = spanPixels
            lastAngleRadians = angleRadians
            return
        }
        val zoomLevels = log2(spanPixels / lastSpanPixels)
        val rotateDegrees = normalizeDegrees((angleRadians - lastAngleRadians) * DEGREES_PER_RADIAN)
        lastSpanPixels = spanPixels
        lastAngleRadians = angleRadians
        val focalPoint = ScreenPoint(
            x = surfacePositionToViewport(
                ((event.getX(firstIndex) + event.getX(secondIndex)) / 2f).toDouble(),
                surfaceWidthPixels,
                attachedWidthPixels,
            ),
            y = surfacePositionToViewport(
                ((event.getY(firstIndex) + event.getY(secondIndex)) / 2f).toDouble(),
                surfaceHeightPixels,
                attachedHeightPixels,
            ),
        )
        transformCamera(
            zoomLevels = zoomLevels,
            rotateDegrees = rotateDegrees,
            focalPoint = focalPoint,
        )
    }

    private fun reportTapAt(surfaceXPixels: Double, surfaceYPixels: Double) {
        val viewport = viewportSize ?: return
        val camera = currentCameraOrNull() ?: return
        val screenPoint = ScreenPoint(
            x = surfacePositionToViewport(surfaceXPixels, surfaceWidthPixels, attachedWidthPixels),
            y = surfacePositionToViewport(surfaceYPixels, surfaceHeightPixels, attachedHeightPixels),
        )
        try {
            val coordinate = camera.screenToGroundCoordinate(
                screenPoint = screenPoint,
                viewport = viewport,
                fieldOfViewDegrees = DEFAULT_FIELD_OF_VIEW_DEGREES,
            )
            if (coordinate != null) {
                listenerDispatcher.dispatch { target -> target.onMapClick(coordinate) }
            }
        } catch (failure: Throwable) {
            if (failure is CancellationException) {
                throw failure
            }
            reportFailure(MapViewFailurePhase.TAP, failure)
        }
    }

    // ------------------------------------------------------------------
    // Camera internals
    // ------------------------------------------------------------------

    /**
     * Builds the contract camera for [pose]. The constructor clamps zoom and
     * pitch and wraps the bearing, so this is the only normalization the
     * wrapper performs.
     */
    private fun cameraFromPoseOrNull(pose: MapCameraPose): MapCamera? =
        try {
            MapCamera(
                center = GeographicCoordinate(
                    latitude = pose.latitudeDegrees,
                    longitude = pose.longitudeDegrees,
                ),
                zoomLevel = pose.zoomLevel,
                bearingDegrees = pose.bearingDegrees,
                pitchDegrees = pose.pitchDegrees,
            )
        } catch (failure: Throwable) {
            if (failure is CancellationException) {
                throw failure
            }
            reportFailure(MapViewFailurePhase.CAMERA, failure)
            null
        }

    private fun applyCameraPose(pose: MapCameraPose, isGestureDriven: Boolean) {
        val adoptedCamera = cameraFromPoseOrNull(pose) ?: return
        adoptCamera(adoptedCamera, isGestureDriven = isGestureDriven)
    }

    /**
     * Makes [camera] the wrapper's camera state and hands it to the native
     * view, which keeps its own copy. The readback and the callback are
     * projections of this one value.
     */
    private fun adoptCamera(
        camera: MapCamera,
        isGestureDriven: Boolean,
        syncToNativeView: Boolean = true,
    ) {
        if (syncToNativeView) {
            nativeMapView?.setCamera(camera)
        }
        val replacedCamera = storedCamera
        storedCamera = camera
        cameraRevision += 1
        val pose = readCameraPose(camera)
        listenerDispatcher.dispatch { target -> target.onCameraChanged(pose, isGestureDriven) }
        if (replacedCamera !== camera) {
            replacedCamera?.close()
        }
        resumePump(resetBackoff = true)
    }

    private fun readCameraPose(camera: MapCamera): MapCameraPose {
        val center = camera.center()
        return MapCameraPose(
            latitudeDegrees = center.latitude,
            longitudeDegrees = center.longitude,
            zoomLevel = camera.zoomLevel(),
            bearingDegrees = camera.bearingDegrees(),
            pitchDegrees = camera.pitchDegrees(),
        )
    }

    /**
     * The camera state the pump draws with. Before the first camera intent it
     * lazily adopts the contract's neutral default, which is the pose the
     * native view itself starts from.
     */
    private fun currentCameraOrNull(): MapCamera? {
        storedCamera?.let { return it }
        val defaultCamera = cameraFromPoseOrNull(DEFAULT_CAMERA_POSE) ?: return null
        storedCamera = defaultCamera
        return defaultCamera
    }

    private fun transformCamera(
        panXPixels: Double = 0.0,
        panYPixels: Double = 0.0,
        zoomLevels: Double = 0.0,
        rotateDegrees: Double = 0.0,
        focalPoint: ScreenPoint? = null,
        isGestureDriven: Boolean = true,
    ) {
        val viewport = viewportSize ?: return
        val hasPan = panXPixels != 0.0 || panYPixels != 0.0
        val zoomFocalPoint = if (zoomLevels != 0.0) focalPoint else null
        val rotateFocalPoint = if (rotateDegrees != 0.0) focalPoint else null
        if (!hasPan && zoomFocalPoint == null && rotateFocalPoint == null) {
            return
        }
        val startingCamera = currentCameraOrNull() ?: return
        var workingCamera = startingCamera
        var ownsWorkingCamera = false
        try {
            if (hasPan) {
                val panned = workingCamera.panByScreenPixels(
                    deltaXPixels = panXPixels,
                    deltaYPixels = panYPixels,
                    viewport = viewport,
                    fieldOfViewDegrees = DEFAULT_FIELD_OF_VIEW_DEGREES,
                )
                if (ownsWorkingCamera) {
                    workingCamera.close()
                }
                workingCamera = panned
                ownsWorkingCamera = true
            }
            if (zoomFocalPoint != null) {
                val zoomed = workingCamera.zoomByLevelsAroundScreenPoint(
                    deltaZoomLevels = zoomLevels,
                    focalPoint = zoomFocalPoint,
                    viewport = viewport,
                    fieldOfViewDegrees = DEFAULT_FIELD_OF_VIEW_DEGREES,
                )
                if (ownsWorkingCamera) {
                    workingCamera.close()
                }
                workingCamera = zoomed
                ownsWorkingCamera = true
            }
            if (rotateFocalPoint != null) {
                val rotated = workingCamera.rotateByDegreesAroundScreenPoint(
                    deltaBearingDegrees = rotateDegrees,
                    focalPoint = rotateFocalPoint,
                    viewport = viewport,
                    fieldOfViewDegrees = DEFAULT_FIELD_OF_VIEW_DEGREES,
                )
                if (ownsWorkingCamera) {
                    workingCamera.close()
                }
                workingCamera = rotated
                ownsWorkingCamera = true
            }
            adoptCamera(workingCamera, isGestureDriven = isGestureDriven)
            ownsWorkingCamera = false
        } catch (failure: Throwable) {
            if (failure is CancellationException) {
                throw failure
            }
            reportFailure(MapViewFailurePhase.CAMERA, failure)
        } finally {
            if (ownsWorkingCamera) {
                workingCamera.close()
            }
        }
    }

    /** Drops a running camera animation; the adopted pose stays where it is. */
    private fun cancelCameraAnimation() {
        cameraAnimation?.close()
        cameraAnimation = null
    }

    private fun advanceCameraAnimation(frameTimeNanos: Long) {
        val animation = cameraAnimation ?: return
        val elapsedNanos = frameTimeNanos - animation.startNanos
        if (elapsedNanos < 0L) {
            return
        }
        val fraction = (elapsedNanos.toDouble() / animation.durationNanos).coerceIn(0.0, 1.0)
        val nextCamera = try {
            MapCamera.interpolate(
                startCamera = animation.fromCamera,
                targetCamera = animation.toCamera,
                fraction = fraction,
            )
        } catch (failure: Throwable) {
            if (failure is CancellationException) {
                throw failure
            }
            reportFailure(MapViewFailurePhase.CAMERA, failure)
            cancelCameraAnimation()
            return
        }
        if (fraction >= 1.0) {
            cameraAnimation = null
            animation.close()
        }
        adoptCamera(nextCamera, isGestureDriven = false)
    }

    // ------------------------------------------------------------------
    // Overlay internals
    // ------------------------------------------------------------------

    private fun replaceOverlays(descriptors: List<MapOverlay>): Boolean {
        val nativeView = nativeMapView
        if (nativeView == null) {
            overlayDescriptors = descriptors
            return true
        }
        return try {
            nativeView.setOverlays(descriptors)
            overlayDescriptors = descriptors
            resumePump(resetBackoff = true)
            true
        } catch (failure: Throwable) {
            if (failure is CancellationException) {
                throw failure
            }
            reportFailure(MapViewFailurePhase.OVERLAY, failure)
            false
        }
    }

    // ------------------------------------------------------------------
    // Native view construction
    // ------------------------------------------------------------------

    private fun rebuildNativeMapView() {
        stopFrameCallback()
        cancelWatchdog()
        detachSurface()
        closeNativeMapView()
        hasReportedMapReady = false
        val suppliedSdk = sdk
        if (suppliedSdk != null && !isDestroyed) {
            // The generated constructor loads the JNI library: a missing or
            // unloadable library is a LinkageError, not an Exception, and it
            // must reach the failure surface instead of the main thread.
            val builtNativeView = try {
                NativeMapView(suppliedSdk)
            } catch (failure: Throwable) {
                reportFailure(MapViewFailurePhase.SURFACE, failure)
                null
            }
            nativeMapView = builtNativeView
            if (builtNativeView != null) {
                applyStoredStateToNativeView()
                if (availableSurface != null) {
                    requiresAttach = true
                    retryAttachRequested = true
                    resumePump(resetBackoff = true)
                }
            }
        }
    }

    private fun applyStoredStateToNativeView() {
        val nativeView = nativeMapView ?: return
        applyDevicePixelRatio(nativeView, MapViewFailurePhase.SURFACE)
        // The fresh native view starts at the contract default; hand it the
        // pose the wrapper holds so its readback and following frames match.
        storedCamera?.let(nativeView::setCamera)
        try {
            nativeView.setOverlays(overlayDescriptors)
        } catch (failure: Throwable) {
            if (failure is CancellationException) {
                throw failure
            }
            reportFailure(MapViewFailurePhase.OVERLAY, failure)
        }
        val state = locationIndicatorState
        val style = locationIndicatorStyle
        if (state != null && style != null) {
            try {
                nativeView.setLocationIndicator(state, style)
            } catch (failure: Throwable) {
                if (failure is CancellationException) {
                    throw failure
                }
                reportFailure(MapViewFailurePhase.LOCATION_INDICATOR, failure)
            }
        }
        val vectorTileSource = vectorBaseSourceQueue.source
        if (vectorTileSource != null) {
            try {
                nativeView.setVectorTileSource(vectorTileSource)
            } catch (failure: Throwable) {
                if (failure is CancellationException) {
                    throw failure
                }
                reportFailure(MapViewFailurePhase.BASE_MAP, failure)
            }
        }
    }

    /**
     * Reports the display's device pixels per dp to the SDK: the labels and
     * overlay screen sizes are declared in dp/points, so the surface scales
     * them by this ratio. Called whenever the effective scale is applied — at
     * attach, after a resize and on a rebuilt native view.
     */
    private fun applyDevicePixelRatio(
        nativeView: NativeMapView,
        failurePhase: MapViewFailurePhase,
    ) {
        try {
            nativeView.setDevicePixelRatio(displayDensity())
        } catch (failure: Throwable) {
            if (failure is CancellationException) {
                throw failure
            }
            reportFailure(failurePhase, failure)
        }
    }

    /** The display's device pixels per dp, or the identity ratio before it resolves. */
    private fun displayDensity(): Double {
        val density = resources.displayMetrics.density
        return if (density.isFinite() && density > 0f) {
            density.toDouble()
        } else {
            DEFAULT_DEVICE_PIXEL_RATIO
        }
    }

    // ------------------------------------------------------------------
    // Frame pump
    // ------------------------------------------------------------------

    private fun canPump(): Boolean =
        !isDestroyed && isLifecycleStarted && isWindowAttached && isWindowVisible &&
            !isRenderingPaused && nativeMapView != null && availableSurface != null

    private fun resumePump(resetBackoff: Boolean) {
        if (!canPump()) {
            return
        }
        cancelWatchdog()
        if (resetBackoff) {
            isBackoffActive = false
            consecutiveUnavailableOnlyFrames = 0
        }
        ensurePumpConsumer()
        if (!isFrameCallbackPosted) {
            isFrameCallbackPosted = true
            choreographerInstance.postFrameCallback(frameCallback)
        }
    }

    private fun ensurePumpConsumer() {
        pumpConsumer.ensureRunning()
    }

    private suspend fun pumpOneFrame(frameTimeNanos: Long) {
        val surface = availableSurface ?: return
        val nativeView = nativeMapView ?: return
        if (requiresAttach) {
            if (retryAttachRequested) {
                retryAttachRequested = false
                tryAttach(nativeView, surface)
            }
            return
        }
        if (!isAttached) {
            return
        }
        val renderWidthPixels = cappedRenderExtent(surfaceWidthPixels)
        val renderHeightPixels = cappedRenderExtent(surfaceHeightPixels)
        if (renderWidthPixels <= 0 || renderHeightPixels <= 0) {
            return
        }
        if (attachedWidthPixels != renderWidthPixels || attachedHeightPixels != renderHeightPixels) {
            tryResize(nativeView, renderWidthPixels, renderHeightPixels)
            return
        }
        applyPendingVectorTileSourceInstall(nativeView)
        advanceCameraAnimation(frameTimeNanos)
        val camera = currentCameraOrNull() ?: return
        val isSettledNow = try {
            nativeView.isFrameSettled(camera)
        } catch (failure: Throwable) {
            if (failure is CancellationException) {
                throw failure
            }
            reportFailure(MapViewFailurePhase.DRAW, failure)
            false
        }
        if (isSettledNow) {
            listenerDispatcher.dispatch { target -> target.onFrameSettled() }
            pauseForSettled()
            return
        }
        if (isDrawInFlight) {
            return
        }
        isDrawInFlight = true
        val drawnCameraRevision = cameraRevision
        try {
            val statistics = nativeView.draw(camera)
            handleDrawnFrame(statistics, drawnCameraRevision)
        } catch (busyFailure: SdkException.RenderingSurfaceUnavailable) {
            // Documented backpressure of the serialized-draw rule: retry next tick.
            consecutiveUnavailableOnlyFrames = 0
            listenerDispatcher.dispatch { target -> target.onFrameDeferred() }
        } catch (failure: Throwable) {
            if (failure is CancellationException) {
                throw failure
            }
            reportFailure(MapViewFailurePhase.DRAW, failure)
            pauseForRetry()
        } finally {
            isDrawInFlight = false
        }
    }

    /**
     * Hands the queued vector base source to the native view once no frame is
     * in flight, then keeps the pump awake so the change is drawn.
     *
     * A source the SDK still refuses is reported through the listener and the
     * queue is dropped: retrying a refusal forever would spin the pump.
     */
    private fun applyPendingVectorTileSourceInstall(nativeView: NativeMapView?) {
        val targetView = nativeView ?: return
        try {
            val applied = vectorBaseSourceQueue.applyPending(isDrawInFlight) { nextSource ->
                if (nextSource == null) {
                    targetView.clearVectorTileSource()
                } else {
                    targetView.setVectorTileSource(nextSource)
                }
            }
            if (applied) {
                resumePump(resetBackoff = true)
            }
        } catch (failure: Throwable) {
            if (failure is CancellationException) {
                throw failure
            }
            // The native view still serves its previous source, and the queue
            // rolled its record back to it rather than keeping a value the
            // SDK refused (a later selection must not inherit it).
            reportFailure(MapViewFailurePhase.BASE_MAP, failure)
        }
    }

    private fun handleDrawnFrame(statistics: FrameStatistics, drawnCameraRevision: Long) {
        listenerDispatcher.dispatch { target -> target.onFrameStatistics(statistics) }
        if (!hasReportedMapReady) {
            hasReportedMapReady = true
            listenerDispatcher.dispatch { target -> target.onMapReady() }
        }
        val nothingDeferred = statistics.deferredTileCount == 0u
        val somethingUnavailable = statistics.unavailableTileCount > 0u
        // "Unchanged pose" is judged against the camera this frame actually
        // drew, not against a gesture that landed while the draw was awaiting
        // its tiles.
        val poseUnchanged = drawnCameraRevision == cameraRevision
        if (nothingDeferred && somethingUnavailable && poseUnchanged) {
            if (isBackoffActive) {
                pauseForBackoff()
                return
            }
            consecutiveUnavailableOnlyFrames += 1
            if (consecutiveUnavailableOnlyFrames >= UNAVAILABLE_BACKOFF_FRAME_COUNT) {
                isBackoffActive = true
                pauseForBackoff()
            }
        } else {
            consecutiveUnavailableOnlyFrames = 0
            isBackoffActive = false
        }
    }

    private fun pauseForSettled() {
        // The settled state is represented by the stopped frame callback:
        // any resume trigger restarts it, and the watchdog re-checks cheaply.
        stopFrameCallback()
        scheduleWatchdog()
    }

    private fun pauseForBackoff() {
        stopFrameCallback()
        scheduleWatchdog()
    }

    private fun pauseForRetry() {
        stopFrameCallback()
        scheduleWatchdog()
    }

    private fun stopFrameCallback() {
        if (isFrameCallbackPosted) {
            choreographerInstance.removeFrameCallback(frameCallback)
            isFrameCallbackPosted = false
        }
    }

    private fun scheduleWatchdog() {
        cancelWatchdog()
        mainHandler.postDelayed(watchdogRunnable, WATCHDOG_INTERVAL_MILLIS)
    }

    private fun cancelWatchdog() {
        mainHandler.removeCallbacks(watchdogRunnable)
    }

    // ------------------------------------------------------------------
    // Attach / resize / detach
    // ------------------------------------------------------------------

    private fun tryAttach(nativeView: NativeMapView, surface: Surface) {
        val renderWidthPixels = cappedRenderExtent(surfaceWidthPixels)
        val renderHeightPixels = cappedRenderExtent(surfaceHeightPixels)
        if (renderWidthPixels <= 0 || renderHeightPixels <= 0) {
            return
        }
        val nativeWindowPointer = try {
            NativeWindowBridge.nativeWindowPointerFromSurface(surface)
        } catch (linkageError: LinkageError) {
            // The hand-written JNI shim lives in the same shared object as the
            // generated bindings; a missing or unresolved symbol is a
            // LinkageError (the class initializer's UnsatisfiedLinkError is
            // one). Report it as a surface failure and wait for an explicit
            // retry instead of letting it kill the frame pump or the app.
            reportFailure(MapViewFailurePhase.SURFACE, linkageError)
            requiresAttach = true
            retryAttachRequested = false
            stopFrameCallback()
            cancelWatchdog()
            return
        }
        acquiredNativeWindowPointer = nativeWindowPointer
        val surfaceHandle = SurfaceHandle(
            appkitViewPointer = null,
            iosViewPointer = null,
            androidNativeWindowPointer = if (nativeWindowPointer != 0L) {
                nativeWindowPointer.toULong()
            } else {
                null
            },
        )
        try {
            nativeView.attach(
                surfaceHandle = surfaceHandle,
                widthPixels = renderWidthPixels.toUInt(),
                heightPixels = renderHeightPixels.toUInt(),
            )
            attachedWidthPixels = renderWidthPixels
            attachedHeightPixels = renderHeightPixels
            requiresAttach = false
            isAttached = true
            applyStoredStateToNativeView()
            resumePump(resetBackoff = true)
        } catch (failure: Throwable) {
            if (failure is CancellationException) {
                throw failure
            }
            // The failed attach must not leak the acquired window reference:
            // release it here, then surface the verbatim failure and wait for
            // an explicit retry (a registration may have arrived meanwhile).
            releaseAcquiredNativeWindow()
            requiresAttach = true
            retryAttachRequested = false
            reportFailure(MapViewFailurePhase.SURFACE, failure)
            stopFrameCallback()
            cancelWatchdog()
        }
    }

    private fun tryResize(nativeView: NativeMapView, widthPixels: Int, heightPixels: Int) {
        try {
            nativeView.resize(widthPixels.toUInt(), heightPixels.toUInt())
            attachedWidthPixels = widthPixels
            attachedHeightPixels = heightPixels
            applyDevicePixelRatio(nativeView, MapViewFailurePhase.RESIZE)
            resumePump(resetBackoff = true)
        } catch (busyFailure: SdkException.RenderingSurfaceUnavailable) {
            // A mid-draw resize is documented backpressure: the next tick retries.
        } catch (failure: Throwable) {
            if (failure is CancellationException) {
                throw failure
            }
            reportFailure(MapViewFailurePhase.RESIZE, failure)
            pauseForRetry()
        }
    }

    private fun detachSurface() {
        if (isAttached) {
            // Teardown must not throw: a detach failure is reported through
            // the log, and the wrapper still releases the window below.
            try {
                nativeMapView?.detach()
            } catch (failure: Throwable) {
                Log.e(LOG_TAG, "detaching the native map view failed", failure)
            }
            isAttached = false
        }
        releaseAcquiredNativeWindow()
        requiresAttach = false
        retryAttachRequested = false
        attachedWidthPixels = 0
        attachedHeightPixels = 0
    }

    /** Closes the native map view, isolating a teardown failure from the host. */
    private fun closeNativeMapView() {
        try {
            nativeMapView?.close()
        } catch (failure: Throwable) {
            Log.e(LOG_TAG, "closing the native map view failed", failure)
        }
        nativeMapView = null
    }

    /**
     * Closes the camera value the wrapper holds. It survives detach and
     * native-view rebuilds, so only teardown releases it.
     */
    private fun closeStoredCamera() {
        try {
            storedCamera?.close()
        } catch (failure: Throwable) {
            Log.e(LOG_TAG, "closing the stored camera failed", failure)
        }
        storedCamera = null
    }

    private fun releaseAcquiredNativeWindow() {
        val pointer = acquiredNativeWindowPointer
        if (pointer != 0L) {
            try {
                NativeWindowBridge.releaseNativeWindow(pointer)
            } catch (linkageError: LinkageError) {
                // Teardown releases unconditionally; an unresolved JNI symbol
                // must not crash the detach path (the window is dropped with
                // the surface either way).
                Log.e(LOG_TAG, "releasing the native window failed", linkageError)
            }
            acquiredNativeWindowPointer = 0L
        }
    }

    // ------------------------------------------------------------------
    // Helpers
    // ------------------------------------------------------------------

    private fun reportFailure(phase: MapViewFailurePhase, cause: Throwable) {
        listenerDispatcher.reportFailure(MapViewFailure(phase, cause))
    }

    /** The surface extent, capped to the downlevel texture limit per axis. */
    private fun cappedRenderExtent(surfaceExtentPixels: Int): Int =
        min(surfaceExtentPixels, MAXIMUM_RENDER_EXTENT_PIXELS)

    private fun surfaceDeltaToViewport(
        surfaceDeltaPixels: Double,
        surfaceExtentPixels: Int,
        viewportExtentPixels: Int,
    ): Double = if (surfaceExtentPixels > 0) {
        surfaceDeltaPixels * viewportExtentPixels / surfaceExtentPixels
    } else {
        surfaceDeltaPixels
    }

    private fun surfacePositionToViewport(
        surfacePositionPixels: Double,
        surfaceExtentPixels: Int,
        viewportExtentPixels: Int,
    ): Double = surfaceDeltaToViewport(surfacePositionPixels, surfaceExtentPixels, viewportExtentPixels)

    /**
     * One camera move in flight. Both endpoint handles are owned by the
     * animation (the start is a readback, never the adopted value) and are
     * closed when the move finishes or a gesture cancels it.
     */
    private class CameraAnimation(
        val startNanos: Long,
        val durationNanos: Double,
        val fromCamera: MapCamera,
        val toCamera: MapCamera,
    ) {
        fun close() {
            fromCamera.close()
            toCamera.close()
        }
    }

    private companion object {
        /** The platform log tag for isolated listener and teardown failures. */
        const val LOG_TAG: String = "RusticMapsMapView"

        /** The bundle key of the superclass's saved view state. */
        const val SUPER_STATE_KEY: String = "com.rusticmaps.sdk.MapView.super-state"

        /** The bundle key of the encoded camera pose. */
        const val CAMERA_POSE_STATE_KEY: String = "com.rusticmaps.sdk.MapView.camera-pose"

        /** The SDK's default perspective field of view (`rustic-maps-rendering`). */
        const val DEFAULT_FIELD_OF_VIEW_DEGREES: Double = 60.0

        /**
         * `wgpu::Limits::downlevel_defaults()`'s `max_texture_dimension_2d`:
         * a surface buffer larger than this is rejected by the downlevel
         * adapter, so the render extent is capped per axis (the drawable
         * cap the public API guide §10 requires).
         */
        const val MAXIMUM_RENDER_EXTENT_PIXELS: Int = 2048

        /**
         * The identity drawing-buffer ratio: one device pixel per dp. Used
         * until the display reports its density.
         */
        const val DEFAULT_DEVICE_PIXEL_RATIO: Double = 1.0

        /** Cheap idle insurance when the pump is settled or backing off. */
        const val WATCHDOG_INTERVAL_MILLIS: Long = 1_000L

        /**
         * Consecutive unavailable-only, pose-unchanged frames before the
         * pump drops to the watchdog's one-second retry cadence: a provider
         * that keeps rejecting tiles must not become a 60 fps retry spin.
         */
        const val UNAVAILABLE_BACKOFF_FRAME_COUNT: Int = 3

        /** Default camera animation duration (the design language's §5.3). */
        const val DEFAULT_CAMERA_ANIMATION_DURATION_MILLIS: Long = 300L

        /**
         * One double tap's zoom step (the design language's §5.2): the
         * gesture zooms exactly one level around the tapped point, the same
         * contract focal-zoom member the pinch recognizer feeds.
         */
        const val DOUBLE_TAP_ZOOM_STEP: Double = 1.0

        /** The SDK's maximum zoom level, the default fit-bounds ceiling. */
        const val DEFAULT_FIT_BOUNDS_MAXIMUM_ZOOM_LEVEL: Double = 24.0

        /** The initial pose before any host camera intent. */
        val DEFAULT_CAMERA_POSE: MapCameraPose = MapCameraPose(
            latitudeDegrees = 0.0,
            longitudeDegrees = 0.0,
            zoomLevel = 2.0,
            bearingDegrees = 0.0,
            pitchDegrees = 0.0,
        )

        /** No insets, the natural default of [fitBounds]. */
        val ZERO_EDGE_INSETS: EdgeInsets = EdgeInsets(
            left = 0.0,
            top = 0.0,
            right = 0.0,
            bottom = 0.0,
        )

        /** Number of pointers a pinch or rotate needs. */
        const val TWO_POINTER_COUNT: Int = 2

        /** Below this span the pinch/rotate ratio is dominated by noise. */
        const val MINIMUM_PINCH_SPAN_PIXELS: Double = 16.0

        /** Radians-to-degrees conversion of the two-finger angle. */
        const val DEGREES_PER_RADIAN: Double = 180.0 / Math.PI

        /** One full turn in degrees. */
        const val FULL_CIRCLE_DEGREES: Double = 360.0

        /** Nanoseconds in one millisecond. */
        const val NANOS_PER_MILLISECOND: Double = 1_000_000.0

        /** The Choreographer must be created on a looper thread (the UI thread). */
        val choreographerInstance: Choreographer = Choreographer.getInstance()
    }
}

/** Wraps an angle in degrees into `[0, 360)`. */
private fun normalizeDegrees(angleDegrees: Double): Double {
    val wrapped = angleDegrees % 360.0
    return if (wrapped < 0.0) wrapped + 360.0 else wrapped
}
