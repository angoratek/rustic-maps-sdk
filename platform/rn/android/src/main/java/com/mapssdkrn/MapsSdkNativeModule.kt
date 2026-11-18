package com.mapssdkrn

import android.app.Activity
import com.rusticmaps.sdk.MapCameraPose
import com.facebook.react.bridge.LifecycleEventListener
import com.facebook.react.bridge.Arguments
import com.facebook.react.bridge.Promise
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.bridge.ReactContextBaseJavaModule
import com.facebook.react.bridge.ReactMethod
import com.facebook.react.bridge.UiThreadUtil
import com.facebook.react.bridge.WritableArray
import com.facebook.react.bridge.WritableMap
import com.facebook.react.modules.core.DeviceEventManagerModule
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import kotlin.math.floor
import uniffi.rustic_maps.GeographicCoordinate
import uniffi.rustic_maps.PositionFix
import uniffi.rustic_maps.PositionSourceError
import uniffi.rustic_maps.RasterImageFormat
import uniffi.rustic_maps.RasterTileResolution
import uniffi.rustic_maps.UrlTemplateRasterTileSource

/**
 * The React Native package's native module (`MapsSdkNative` in JavaScript).
 *
 * It owns the package's one `MapSdk` (see [MapsSdkNativeRuntime]), proxies the
 * contract's provider/search/diagnostics members, forwards the `<MapView>`
 * imperative calls to the addressed host view, and installs the single view
 * event channel. Every method runs on the main looper (the SDK view's
 * contract) and every failure is rejected verbatim — no silent catch.
 */
class MapsSdkNativeModule(
    private val reactContext: ReactApplicationContext,
) : ReactContextBaseJavaModule(reactContext), LifecycleEventListener {

    init {
        // A launch-intent credential can arrive before an activity exists; the
        // first resume retries it once the intent is reachable.
        reactContext.addLifecycleEventListener(this)
    }

    override fun onHostResume() {
        MapsSdkNativeRuntime.applyLaunchInjectedCredential(reactContext.currentActivity)
    }

    override fun onHostPause() = Unit

    override fun onHostDestroy() = Unit

    companion object {
        /** The JavaScript module name. */
        const val MODULE_NAME: String = "MapsSdkNative"

        /** The promise rejection code every SDK failure carries. */
        private const val FAILURE_CODE: String = "MapsSdkNativeFailure"

        /** The rejection code an invalid tile-cache time-to-live carries. */
        private const val TIME_TO_LIVE_FAILURE_CODE: String = "MapsSdkInvalidTimeToLive"

        /** The view-not-found rejection detail. */
        private const val VIEW_NOT_FOUND_TEXT: String = "the native MapView was not found"

        /** The event the component listens to for a refused view operation. */
        private const val VIEW_FAILURE_EVENT_NAME: String = "error"

        /** The failure phases the component's `MapViewFailurePhase` declares. */
        private const val CAMERA_FAILURE_PHASE: String = "camera"
        private const val LOCATION_INDICATOR_FAILURE_PHASE: String = "locationIndicator"
        private const val BASE_MAP_FAILURE_PHASE: String = "baseMap"
        private const val SURFACE_FAILURE_PHASE: String = "surface"

        /** The generated contract's image-format ordinals (PNG, JPEG). */
        private const val RASTER_IMAGE_FORMAT_PNG_ORDINAL: Int = 0
        private const val RASTER_IMAGE_FORMAT_JPEG_ORDINAL: Int = 1

        /** The generated contract's tile-resolution ordinals (standard, retina). */
        private const val RASTER_TILE_RESOLUTION_STANDARD_ORDINAL: Int = 0
        private const val RASTER_TILE_RESOLUTION_RETINA_ORDINAL: Int = 1

        /** The inclusive zoom-level range a `u8` contract field can carry. */
        private const val MINIMUM_ZOOM_LEVEL: Int = 0
        private const val MAXIMUM_ZOOM_LEVEL: Int = 255
    }

    override fun getName(): String = MODULE_NAME

    private val sdkCallScope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

    override fun initialize() {
        super.initialize()
        MapsSdkNativeEventChannel.sink = { eventName, payload ->
            emitToJavaScript(eventName, payload)
        }
    }

    /**
     * Releases everything the module owns before React Native drops it: the
     * event sink, the lifecycle listener, the coroutine scope and the host
     * device-location source. `invalidate()` is the current teardown hook;
     * the removed `onCatalystInstanceDestroy()` override never ran on this
     * React Native line.
     */
    override fun invalidate() {
        MapsSdkNativeEventChannel.sink = null
        reactContext.removeLifecycleEventListener(this)
        sdkCallScope.cancel()
        MapsSdkNativeRuntime.stopDeviceLocation()
        super.invalidate()
    }

    private fun emitToJavaScript(eventName: String, payload: WritableMap) {
        UiThreadUtil.runOnUiThread {
            reactContext
                .getJSModule(DeviceEventManagerModule.RCTDeviceEventEmitter::class.java)
                ?.emit(eventName, payload)
        }
    }

    // ------------------------------------------------------------------
    // SDK lifecycle and contract proxy
    // ------------------------------------------------------------------

    @ReactMethod
    fun initialize(promise: Promise) {
        UiThreadUtil.runOnUiThread {
            MapsSdkNativeRuntime.ensureStarted(reactContext)
                .onFailure { startupFailure ->
                    promise.reject(
                        FAILURE_CODE,
                        startupFailure.message ?: startupFailure.toString(),
                        startupFailure,
                    )
                }
                .onSuccess {
                    MapsSdkNativeRuntime.applyLaunchInjectedCredential(
                        reactContext.currentActivity,
                    )
                    promise.resolve(null)
                }
        }
    }

    @ReactMethod
    fun getCompiledVersion(promise: Promise) {
        promise.resolve(MapsSdkNativeRuntime.compiledVersion)
    }

    @ReactMethod
    fun defaultProviderKey(promise: Promise) {
        promise.resolve(MapsSdkNativeRuntime.MAPQUEST_PROVIDER_KEY)
    }

    @ReactMethod
    fun setProviderCredential(
        providerKey: String,
        credential: String,
        promise: Promise,
    ) {
        UiThreadUtil.runOnUiThread {
            promise.resolveCredentialResult(
                MapsSdkNativeRuntime.setProviderCredential(
                    providerKey,
                    credential,
                    withTileCache = false,
                ),
            )
        }
    }

    @ReactMethod
    fun setProviderCredentialWithTileCache(
        providerKey: String,
        credential: String,
        promise: Promise,
    ) {
        UiThreadUtil.runOnUiThread {
            promise.resolveCredentialResult(
                MapsSdkNativeRuntime.setProviderCredential(
                    providerKey,
                    credential,
                    withTileCache = true,
                ),
            )
        }
    }

    @ReactMethod
    fun registerUrlTemplateRasterTileSource(sourceJson: String, promise: Promise) {
        UiThreadUtil.runOnUiThread {
            try {
                MapsSdkNativeRuntime.registerUrlTemplateRasterTileSource(
                    urlTemplateRasterTileSourceFromJson(sourceJson),
                )
                promise.resolveCredentialResult(Pair(true, ""))
            } catch (registrationFailure: Exception) {
                promise.reject(
                    FAILURE_CODE,
                    registrationFailure.message ?: registrationFailure.toString(),
                    registrationFailure,
                )
            }
        }
    }

    @ReactMethod
    fun restoreCredentialedBaseMap(providerKey: String, promise: Promise) {
        UiThreadUtil.runOnUiThread {
            promise.resolveCredentialResult(
                MapsSdkNativeRuntime.restoreCredentialedBaseMap(providerKey),
            )
        }
    }

    /** Chooses the provider that serves one capability. */
    @ReactMethod
    fun configureProvider(
        contractOrdinal: Int,
        providerKey: String,
        credential: String,
        promise: Promise,
    ) {
        UiThreadUtil.runOnUiThread {
            try {
                MapsSdkNativeRuntime.configureProvider(
                    MapsSdkNativeJson.providerContractFromOrdinal(contractOrdinal),
                    providerKey,
                    credential,
                )
                promise.resolve(null)
            } catch (configurationFailure: Exception) {
                promise.reject(
                    FAILURE_CODE,
                    configurationFailure.message ?: configurationFailure.toString(),
                    configurationFailure,
                )
            }
        }
    }

    /** Reports which provider serves each capability, as JSON. */
    @ReactMethod
    fun providerRegistrations(promise: Promise) {
        UiThreadUtil.runOnUiThread {
            try {
                promise.resolve(
                    MapsSdkNativeJson.providerRegistrationsToJson(
                        MapsSdkNativeRuntime.providerRegistrations(),
                    ),
                )
            } catch (registrationFailure: Exception) {
                promise.reject(
                    FAILURE_CODE,
                    registrationFailure.message ?: registrationFailure.toString(),
                    registrationFailure,
                )
            }
        }
    }

    /** Reports the serving routing provider's capabilities, as JSON. */
    @ReactMethod
    fun routeCapabilities(promise: Promise) {
        UiThreadUtil.runOnUiThread {
            try {
                promise.resolve(
                    MapsSdkNativeJson.routeCapabilitiesToJson(
                        MapsSdkNativeRuntime.routeCapabilities(),
                    ),
                )
            } catch (capabilitiesFailure: Exception) {
                promise.reject(
                    FAILURE_CODE,
                    capabilitiesFailure.message ?: capabilitiesFailure.toString(),
                    capabilitiesFailure,
                )
            }
        }
    }

    /** Decodes the base-map tile source the typed JavaScript proxy sends. */
    private fun urlTemplateRasterTileSourceFromJson(
        sourceJson: String,
    ): UrlTemplateRasterTileSource {
        val sourceObject = org.json.JSONObject(sourceJson)
        return UrlTemplateRasterTileSource(
            urlTemplate = sourceObject.getString("urlTemplate"),
            sourceName = sourceObject.getString("sourceName"),
            minimumZoomLevel = zoomLevelFromJson(sourceObject, "minimumZoomLevel"),
            maximumZoomLevel = zoomLevelFromJson(sourceObject, "maximumZoomLevel"),
            imageFormat = when (sourceObject.getInt("imageFormat")) {
                RASTER_IMAGE_FORMAT_PNG_ORDINAL -> RasterImageFormat.PNG
                RASTER_IMAGE_FORMAT_JPEG_ORDINAL -> RasterImageFormat.JPEG
                else -> throw IllegalArgumentException(
                    "unknown raster image format ordinal " +
                        sourceObject.getInt("imageFormat"),
                )
            },
            tileResolution = when (sourceObject.getInt("tileResolution")) {
                RASTER_TILE_RESOLUTION_STANDARD_ORDINAL -> RasterTileResolution.STANDARD
                RASTER_TILE_RESOLUTION_RETINA_ORDINAL -> RasterTileResolution.RETINA
                else -> throw IllegalArgumentException(
                    "unknown raster tile resolution ordinal " +
                        sourceObject.getInt("tileResolution"),
                )
            },
            userAgent = sourceObject.getString("userAgent"),
            tokenBucketCapacityTokens = optionalDoubleFromJson(
                sourceObject,
                "tokenBucketCapacityTokens",
            ),
            refillTokensPerSecond = optionalDoubleFromJson(
                sourceObject,
                "refillTokensPerSecond",
            ),
        )
    }

    private fun optionalDoubleFromJson(
        sourceObject: org.json.JSONObject,
        fieldName: String,
    ): Double? =
        if (sourceObject.has(fieldName) && !sourceObject.isNull(fieldName)) {
            sourceObject.getDouble(fieldName)
        } else {
            null
        }

    private fun zoomLevelFromJson(sourceObject: org.json.JSONObject, fieldName: String): UByte {
        val zoomLevel = sourceObject.getInt(fieldName)
        require(zoomLevel in MINIMUM_ZOOM_LEVEL..MAXIMUM_ZOOM_LEVEL) {
            "$fieldName must be within $MINIMUM_ZOOM_LEVEL..$MAXIMUM_ZOOM_LEVEL (was $zoomLevel)"
        }
        return zoomLevel.toUByte()
    }

    @ReactMethod
    fun search(queryText: String, searchBoundaryJson: String?, promise: Promise) {
        runSdkCall(promise) {
            MapsSdkNativeJson.placesToJson(
                MapsSdkNativeRuntime.search(
                    queryText,
                    searchBoundaryJson?.let { MapsSdkNativeJson.boundingBoxFromJson(it) },
                ),
            )
        }
    }

    @ReactMethod
    fun suggest(queryText: String, searchBoundaryJson: String?, promise: Promise) {
        runSdkCall(promise) {
            MapsSdkNativeJson.placesToJson(
                MapsSdkNativeRuntime.suggest(
                    queryText,
                    searchBoundaryJson?.let { MapsSdkNativeJson.boundingBoxFromJson(it) },
                ),
            )
        }
    }

    @ReactMethod
    fun forwardGeocode(addressText: String, promise: Promise) {
        runSdkCall(promise) {
            MapsSdkNativeJson.placesToJson(
                MapsSdkNativeRuntime.forwardGeocode(addressText),
            )
        }
    }

    @ReactMethod
    fun calculateRoute(
        waypointsJson: String,
        routeOptionsJson: String,
        promise: Promise,
    ) {
        runSdkCall(promise) {
            MapsSdkNativeJson.routeToJson(
                MapsSdkNativeRuntime.calculateRoute(
                    MapsSdkNativeJson.coordinatesFromJson(waypointsJson),
                    MapsSdkNativeJson.routeOptionsFromJson(routeOptionsJson),
                ),
            )
        }
    }

    @ReactMethod
    fun reverseGeocode(coordinateJson: String, promise: Promise) {
        runSdkCall(promise) {
            val coordinateObject = org.json.JSONObject(coordinateJson)
            MapsSdkNativeJson.placesToJson(
                MapsSdkNativeRuntime.reverseGeocode(
                    GeographicCoordinate(
                        latitude = coordinateObject.getDouble("latitude"),
                        longitude = coordinateObject.getDouble("longitude"),
                    ),
                ),
            )
        }
    }

    /** Calculates the contract's distance matrix. */
    @ReactMethod
    fun calculateDistanceMatrix(
        originsJson: String,
        destinationsJson: String,
        transportationMode: Int,
        promise: Promise,
    ) {
        runSdkCall(promise) {
            MapsSdkNativeJson.distanceMatrixToJson(
                MapsSdkNativeRuntime.calculateDistanceMatrix(
                    MapsSdkNativeJson.coordinatesFromJson(originsJson),
                    MapsSdkNativeJson.coordinatesFromJson(destinationsJson),
                    MapsSdkNativeJson.transportationModeFromOrdinal(transportationMode),
                ),
            )
        }
    }

    /** Fetches the incidents intersecting a bounding box. */
    @ReactMethod
    fun fetchTrafficIncidents(boundingBoxJson: String, promise: Promise) {
        runSdkCall(promise) {
            MapsSdkNativeJson.trafficIncidentsToJson(
                MapsSdkNativeRuntime.fetchTrafficIncidents(
                    MapsSdkNativeJson.boundingBoxFromJson(boundingBoxJson),
                ),
            )
        }
    }

    /** Fetches per-segment traffic flow inside a bounding box. */
    @ReactMethod
    fun fetchTrafficFlow(boundingBoxJson: String, promise: Promise) {
        runSdkCall(promise) {
            MapsSdkNativeJson.trafficFlowToJson(
                MapsSdkNativeRuntime.fetchTrafficFlow(
                    MapsSdkNativeJson.boundingBoxFromJson(boundingBoxJson),
                ),
            )
        }
    }

    @ReactMethod
    fun tileCacheTerms(promise: Promise) {
        UiThreadUtil.runOnUiThread {
            try {
                promise.resolve(
                    MapsSdkNativeJson.tileCacheTermsToJson(
                        MapsSdkNativeRuntime.tileCacheTerms(),
                    ),
                )
            } catch (termsFailure: Exception) {
                promise.reject(
                    FAILURE_CODE,
                    termsFailure.message ?: termsFailure.toString(),
                    termsFailure,
                )
            }
        }
    }

    /** Reads the serving traffic-flow cache's counters as JSON, or `null`. */
    @ReactMethod
    fun trafficFlowCacheStatistics(promise: Promise) {
        UiThreadUtil.runOnUiThread {
            try {
                val statistics = MapsSdkNativeRuntime.trafficFlowCacheStatistics()
                promise.resolve(
                    statistics?.let { reportedStatistics ->
                        MapsSdkNativeJson.trafficFlowCacheStatisticsToJson(
                            reportedStatistics,
                        )
                    } ?: "null",
                )
            } catch (statisticsFailure: Exception) {
                promise.reject(
                    FAILURE_CODE,
                    statisticsFailure.message ?: statisticsFailure.toString(),
                    statisticsFailure,
                )
            }
        }
    }

    /** Reads the installed tile payload cache's exact counters as JSON, or `null`. */
    @ReactMethod
    fun tileCacheStatistics(promise: Promise) {
        UiThreadUtil.runOnUiThread {
            try {
                val statistics = MapsSdkNativeRuntime.tileCacheStatistics()
                promise.resolve(
                    statistics?.let { reportedStatistics ->
                        MapsSdkNativeJson.tileCacheStatisticsToJson(
                            reportedStatistics,
                        )
                    } ?: "null",
                )
            } catch (statisticsFailure: Exception) {
                promise.reject(
                    FAILURE_CODE,
                    statisticsFailure.message ?: statisticsFailure.toString(),
                    statisticsFailure,
                )
            }
        }
    }

    @ReactMethod
    fun setTileCacheTimeToLive(timeToLiveSeconds: Double, promise: Promise) {
        UiThreadUtil.runOnUiThread {
            try {
                MapsSdkNativeRuntime.setTileCacheTimeToLive(
                    wholeTimeToLiveSeconds(timeToLiveSeconds),
                )
                promise.resolve(null)
            } catch (invalidTimeToLive: InvalidTimeToLiveException) {
                promise.reject(
                    TIME_TO_LIVE_FAILURE_CODE,
                    invalidTimeToLive.message,
                    invalidTimeToLive,
                )
            } catch (timeToLiveFailure: Exception) {
                promise.reject(
                    FAILURE_CODE,
                    timeToLiveFailure.message ?: timeToLiveFailure.toString(),
                    timeToLiveFailure,
                )
            }
        }
    }

    /**
     * Converts the bridge's `number` into whole seconds. Refuses NaN,
     * infinities, non-positive values, fractions and values at or beyond
     * `2^63` before any conversion: `Double.toLong()` saturates there, and
     * the contract's `u64` conversion would then wrap.
     */
    private fun wholeTimeToLiveSeconds(timeToLiveSeconds: Double): Long {
        if (!timeToLiveSeconds.isFinite() ||
            timeToLiveSeconds <= 0.0 ||
            timeToLiveSeconds != floor(timeToLiveSeconds) ||
            timeToLiveSeconds >= Long.MAX_VALUE.toDouble()
        ) {
            throw InvalidTimeToLiveException()
        }
        return timeToLiveSeconds.toLong()
    }

    // ------------------------------------------------------------------
    // Offline regions
    // ------------------------------------------------------------------

    /** How many tiles the region enumerates, for the pre-download warning. */
    @ReactMethod
    fun regionTileCount(regionJson: String, promise: Promise) {
        UiThreadUtil.runOnUiThread {
            try {
                promise.resolve(
                    MapsSdkNativeRuntime.regionTileCount(
                        MapsSdkNativeJson.regionDefinitionFromJson(regionJson),
                    ).toLong(),
                )
            } catch (countFailure: Exception) {
                promise.reject(
                    FAILURE_CODE,
                    countFailure.message ?: countFailure.toString(),
                    countFailure,
                )
            }
        }
    }

    /** Downloads one region; progress crosses on the region event channel. */
    @ReactMethod
    fun downloadRegion(regionJson: String, promise: Promise) {
        runSdkCall(promise) {
            MapsSdkNativeJson.regionRecordToJson(
                MapsSdkNativeRuntime.downloadRegion(
                    MapsSdkNativeJson.regionDefinitionFromJson(regionJson),
                ),
            )
        }
    }

    /** Cancels the region's in-flight download; idempotent. */
    @ReactMethod
    fun cancelRegionDownload(regionId: String, promise: Promise) {
        UiThreadUtil.runOnUiThread {
            try {
                MapsSdkNativeRuntime.cancelRegionDownload(regionId)
                promise.resolve(null)
            } catch (cancelFailure: Exception) {
                promise.reject(
                    FAILURE_CODE,
                    cancelFailure.message ?: cancelFailure.toString(),
                    cancelFailure,
                )
            }
        }
    }

    /** Every region stored on disk, as JSON. */
    @ReactMethod
    fun regionRecords(promise: Promise) {
        runSdkCall(promise) {
            MapsSdkNativeJson.regionRecordsToJson(
                MapsSdkNativeRuntime.regionRecords(),
            )
        }
    }

    /** One stored region's record as JSON, or null when none is stored. */
    @ReactMethod
    fun regionRecord(regionId: String, promise: Promise) {
        runNullableSdkCall(promise) {
            MapsSdkNativeRuntime.regionRecord(regionId)
                ?.let(MapsSdkNativeJson::regionRecordToJson)
        }
    }

    /** Deletes one stored region and answers what was removed, as JSON. */
    @ReactMethod
    fun deleteRegion(regionId: String, promise: Promise) {
        runSdkCall(promise) {
            MapsSdkNativeJson.regionRemovalReportToJson(
                MapsSdkNativeRuntime.deleteRegion(regionId),
            )
        }
    }

    @ReactMethod
    fun latestProviderFailureDiagnostics(promise: Promise) {
        UiThreadUtil.runOnUiThread {
            try {
                promise.resolve(
                    MapsSdkNativeJson.providerFailureDiagnosticsToJson(
                        MapsSdkNativeRuntime.latestProviderFailureDiagnostics(),
                    ),
                )
            } catch (diagnosticsFailure: Exception) {
                promise.reject(
                    FAILURE_CODE,
                    diagnosticsFailure.message ?: diagnosticsFailure.toString(),
                    diagnosticsFailure,
                )
            }
        }
    }

    @ReactMethod
    fun diagnosticsLines(promise: Promise) {
        UiThreadUtil.runOnUiThread {
            val lines: WritableArray = Arguments.createArray()
            MapsSdkNativeRuntime.diagnosticsLines().forEach { line -> lines.pushString(line) }
            promise.resolve(lines)
        }
    }

    // ------------------------------------------------------------------
    // Device positioning
    // ------------------------------------------------------------------

    /** Starts the host device-location source; false when it cannot start. */
    @ReactMethod
    fun startDeviceLocation(promise: Promise) {
        UiThreadUtil.runOnUiThread {
            try {
                promise.resolve(MapsSdkNativeRuntime.startDeviceLocation(reactContext))
            } catch (startFailure: Exception) {
                promise.reject(
                    FAILURE_CODE,
                    startFailure.message ?: startFailure.toString(),
                    startFailure,
                )
            }
        }
    }

    /** Stops and clears the host device-location source. */
    @ReactMethod
    fun stopDeviceLocation(promise: Promise) {
        UiThreadUtil.runOnUiThread {
            try {
                promise.resolve(MapsSdkNativeRuntime.stopDeviceLocation())
            } catch (stopFailure: Exception) {
                promise.reject(
                    FAILURE_CODE,
                    stopFailure.message ?: stopFailure.toString(),
                    stopFailure,
                )
            }
        }
    }

    /** The controller's latest accepted fix as JSON, or null. */
    @ReactMethod
    fun latestDeviceFix(promise: Promise) {
        UiThreadUtil.runOnUiThread {
            try {
                promise.resolve(
                    MapsSdkNativeRuntime.latestDeviceFix()?.let(::positionFixJson),
                )
            } catch (fixFailure: Exception) {
                promise.reject(
                    FAILURE_CODE,
                    fixFailure.message ?: fixFailure.toString(),
                    fixFailure,
                )
            }
        }
    }

    /** The smoothed course in degrees clockwise from true north, or null. */
    @ReactMethod
    fun deviceLocationSmoothedCourseDegrees(promise: Promise) {
        UiThreadUtil.runOnUiThread {
            try {
                promise.resolve(
                    MapsSdkNativeRuntime.deviceLocationSmoothedCourseDegrees(),
                )
            } catch (courseFailure: Exception) {
                promise.reject(
                    FAILURE_CODE,
                    courseFailure.message ?: courseFailure.toString(),
                    courseFailure,
                )
            }
        }
    }

    /** The last device-location source failure, or null. */
    @ReactMethod
    fun deviceLocationSourceStatus(promise: Promise) {
        UiThreadUtil.runOnUiThread {
            try {
                promise.resolve(
                    MapsSdkNativeRuntime.deviceLocationSourceStatus()
                        ?.let(::positionSourceErrorName),
                )
            } catch (statusFailure: Exception) {
                promise.reject(
                    FAILURE_CODE,
                    statusFailure.message ?: statusFailure.toString(),
                    statusFailure,
                )
            }
        }
    }

    /** Encodes one contract fix as the JSON record the typed surface declares. */
    private fun positionFixJson(fix: PositionFix): String {
        val coordinateObject = org.json.JSONObject()
        coordinateObject.put("latitude", fix.coordinate.latitude)
        coordinateObject.put("longitude", fix.coordinate.longitude)
        val fixObject = org.json.JSONObject()
        fixObject.put("coordinate", coordinateObject)
        fixObject.put(
            "horizontalAccuracyMeters",
            fix.horizontalAccuracyMeters ?: org.json.JSONObject.NULL,
        )
        fixObject.put(
            "courseHeadingDegrees",
            fix.courseHeadingDegrees ?: org.json.JSONObject.NULL,
        )
        fixObject.put(
            "timestampSeconds",
            fix.timestampSeconds ?: org.json.JSONObject.NULL,
        )
        return fixObject.toString()
    }

    /** The lower-camel name the typed JavaScript surface declares. */
    private fun positionSourceErrorName(error: PositionSourceError): String =
        when (error) {
            PositionSourceError.PERMISSION_DENIED -> "permissionDenied"
            PositionSourceError.SERVICE_DISABLED -> "serviceDisabled"
            PositionSourceError.UNAVAILABLE -> "unavailable"
            PositionSourceError.TIMEOUT -> "timeout"
        }

    // ------------------------------------------------------------------
    // MapView imperative calls (addressed by the view identifier)
    // ------------------------------------------------------------------

    @ReactMethod
    fun setCamera(viewIdentifier: String, cameraJson: String) {
        withHostView(viewIdentifier, CAMERA_FAILURE_PHASE) { hostView ->
            hostView.setCamera(MapsSdkNativeJson.cameraFromJson(cameraJson))
        }
    }

    @ReactMethod
    fun animateCamera(
        viewIdentifier: String,
        cameraJson: String,
        durationMilliseconds: Double,
    ) {
        withHostView(viewIdentifier, CAMERA_FAILURE_PHASE) { hostView ->
            hostView.animateCamera(
                MapsSdkNativeJson.cameraFromJson(cameraJson),
                durationMilliseconds.toLong(),
            )
        }
    }

    @ReactMethod
    fun fitBounds(
        viewIdentifier: String,
        boundingBoxJson: String,
        edgeInsetsJson: String,
        maximumZoomLevel: Double,
    ) {
        withHostView(viewIdentifier, CAMERA_FAILURE_PHASE) { hostView ->
            hostView.fitBounds(
                MapsSdkNativeJson.boundingBoxFromJson(boundingBoxJson),
                MapsSdkNativeJson.edgeInsetsFromJson(edgeInsetsJson),
                maximumZoomLevel,
            )
        }
    }

    @ReactMethod
    fun panByScreenPixels(
        viewIdentifier: String,
        deltaXPixels: Double,
        deltaYPixels: Double,
        isGestureDriven: Boolean,
    ) {
        withHostView(viewIdentifier, CAMERA_FAILURE_PHASE) { hostView ->
            hostView.panByScreenPixels(deltaXPixels, deltaYPixels, isGestureDriven)
        }
    }

    @ReactMethod
    fun zoomByLevels(viewIdentifier: String, deltaZoomLevels: Double) {
        withHostView(viewIdentifier, CAMERA_FAILURE_PHASE) { hostView ->
            hostView.zoomByLevels(deltaZoomLevels)
        }
    }

    @ReactMethod
    fun addOverlay(
        viewIdentifier: String,
        overlayIdentifier: String,
        overlayJson: String,
        promise: Promise,
    ) {
        withHostViewPromise(viewIdentifier, promise) { hostView ->
            hostView.addOverlay(
                overlayIdentifier,
                MapsSdkNativeJson.overlayFromJson(overlayJson),
            )
        }
    }

    @ReactMethod
    fun updateOverlay(
        viewIdentifier: String,
        overlayIdentifier: String,
        overlayJson: String,
        promise: Promise,
    ) {
        withHostViewPromise(viewIdentifier, promise) { hostView ->
            hostView.updateOverlay(
                overlayIdentifier,
                MapsSdkNativeJson.overlayFromJson(overlayJson),
            )
        }
    }

    @ReactMethod
    fun removeOverlay(
        viewIdentifier: String,
        overlayIdentifier: String,
        promise: Promise,
    ) {
        withHostViewPromise(viewIdentifier, promise) { hostView ->
            hostView.removeOverlay(overlayIdentifier)
        }
    }

    @ReactMethod
    fun setOverlays(viewIdentifier: String, overlaysJson: String, promise: Promise) {
        withHostViewPromise(viewIdentifier, promise) { hostView ->
            val overlaysArray = org.json.JSONArray(overlaysJson)
            hostView.setOverlays(
                (0 until overlaysArray.length()).map { overlayIndex ->
                    MapsSdkNativeJson.overlayFromJson(
                        overlaysArray.getJSONObject(overlayIndex).toString(),
                    )
                },
            )
        }
    }

    @ReactMethod
    fun clearOverlays(viewIdentifier: String, promise: Promise) {
        withHostViewPromise(viewIdentifier, promise) { hostView ->
            hostView.clearOverlays()
        }
    }

    @ReactMethod
    fun setLocationIndicator(
        viewIdentifier: String,
        stateJson: String,
        styleJson: String,
    ) {
        withHostView(viewIdentifier, LOCATION_INDICATOR_FAILURE_PHASE) { hostView ->
            hostView.setLocationIndicator(
                MapsSdkNativeJson.locationIndicatorStateFromJson(stateJson),
                MapsSdkNativeJson.locationIndicatorStyleFromJson(styleJson),
            )
        }
    }

    @ReactMethod
    fun clearLocationIndicator(viewIdentifier: String) {
        withHostView(viewIdentifier, LOCATION_INDICATOR_FAILURE_PHASE) { hostView ->
            hostView.clearLocationIndicator()
        }
    }

    @ReactMethod
    fun retryAttach(viewIdentifier: String) {
        withHostView(viewIdentifier, SURFACE_FAILURE_PHASE) { hostView ->
            hostView.retryAttach()
        }
    }

    @ReactMethod
    fun setVectorTileSource(viewIdentifier: String, sourceJson: String, promise: Promise) {
        withHostViewPromise(viewIdentifier, promise) { hostView ->
            hostView.setVectorTileSource(
                MapsSdkNativeJson.vectorTileSourceFromJson(sourceJson),
            )
        }
    }

    @ReactMethod
    fun setProviderVectorTileSourceFromDocument(
        viewIdentifier: String,
        sourceJson: String,
        styleDocumentUrl: String,
        promise: Promise,
    ) {
        sdkCallScope.launch {
            val hostView = MapsSdkMapHostView.find(viewIdentifier)
            if (hostView == null) {
                promise.reject(FAILURE_CODE, VIEW_NOT_FOUND_TEXT)
                return@launch
            }
            try {
                promise.resolve(
                    hostView.setProviderVectorTileSourceFromDocument(
                        MapsSdkNativeJson.providerVectorTileSourceFromJson(sourceJson),
                        styleDocumentUrl,
                    ),
                )
            } catch (cancellation: CancellationException) {
                throw cancellation
            } catch (callFailure: Exception) {
                promise.reject(
                    FAILURE_CODE,
                    callFailure.message ?: callFailure.toString(),
                    callFailure,
                )
            }
        }
    }

    @ReactMethod
    fun clearVectorTileSource(viewIdentifier: String, promise: Promise) {
        withHostViewPromise(viewIdentifier, promise) { hostView ->
            hostView.clearVectorTileSource()
        }
    }

    @ReactMethod
    fun getVectorTileSourceStatus(viewIdentifier: String, promise: Promise) {
        withHostViewPromise(viewIdentifier, promise) { hostView ->
            hostView.vectorTileSourceStatus()
                ?.let(MapsSdkNativeJson::vectorTileSourceStatusToJson)
                ?: "{}"
        }
    }

    @ReactMethod
    fun reloadTileSource(viewIdentifier: String) {
        withHostView(viewIdentifier, BASE_MAP_FAILURE_PHASE) { hostView ->
            hostView.reloadTileSource()
        }
    }

    @ReactMethod
    fun setRenderingPaused(viewIdentifier: String, paused: Boolean) {
        withHostView(viewIdentifier, SURFACE_FAILURE_PHASE) { hostView ->
            hostView.setRenderingPaused(paused)
        }
    }

    @ReactMethod
    fun getCamera(viewIdentifier: String, promise: Promise) {
        withHostViewPromise(viewIdentifier, promise) { hostView ->
            val camera: MapCameraPose = hostView.camera()
            val cameraObject = org.json.JSONObject()
            cameraObject.put("latitudeDegrees", camera.latitudeDegrees)
            cameraObject.put("longitudeDegrees", camera.longitudeDegrees)
            cameraObject.put("zoomLevel", camera.zoomLevel)
            cameraObject.put("bearingDegrees", camera.bearingDegrees)
            cameraObject.put("pitchDegrees", camera.pitchDegrees)
            cameraObject.toString()
        }
    }

    /**
     * The first-frame readiness readback. The `mapReady` event fires once; a
     * JavaScript subscriber that attaches after it reads the same state here.
     */
    @ReactMethod
    fun isMapReady(viewIdentifier: String, promise: Promise) {
        withHostViewPromise(viewIdentifier, promise) { hostView ->
            hostView.hasDrawnFirstFrame
        }
    }

    @ReactMethod
    fun groundMetersPerScreenPixel(viewIdentifier: String, promise: Promise) {
        withHostViewPromise(viewIdentifier, promise) { hostView ->
            hostView.groundMetersPerScreenPixel()
        }
    }

    // ------------------------------------------------------------------
    // Helpers
    // ------------------------------------------------------------------

    /**
     * Runs one address-addressed command on the main looper. A malformed
     * JavaScript payload throws while it is decoded; the failure reaches the
     * view's `error` event instead of escaping the looper and killing the
     * process.
     */
    private fun withHostView(
        viewIdentifier: String,
        failurePhase: String,
        action: (MapsSdkMapHostView) -> Unit,
    ) {
        UiThreadUtil.runOnUiThread {
            val hostView = MapsSdkMapHostView.find(viewIdentifier) ?: return@runOnUiThread
            try {
                action(hostView)
            } catch (actionFailure: Exception) {
                val failurePayload = Arguments.createMap()
                failurePayload.putString("failurePhase", failurePhase)
                failurePayload.putString(
                    "failureMessage",
                    actionFailure.message ?: actionFailure.toString(),
                )
                MapsSdkNativeEventChannel.emit(
                    viewIdentifier,
                    VIEW_FAILURE_EVENT_NAME,
                    failurePayload,
                )
            }
        }
    }

    private fun <ResultType> withHostViewPromise(
        viewIdentifier: String,
        promise: Promise,
        action: (MapsSdkMapHostView) -> ResultType,
    ) {
        UiThreadUtil.runOnUiThread {
            val hostView = MapsSdkMapHostView.find(viewIdentifier)
            if (hostView == null) {
                promise.reject(FAILURE_CODE, VIEW_NOT_FOUND_TEXT)
                return@runOnUiThread
            }
            try {
                promise.resolve(action(hostView))
            } catch (actionFailure: Exception) {
                promise.reject(
                    FAILURE_CODE,
                    actionFailure.message ?: actionFailure.toString(),
                    actionFailure,
                )
            }
        }
    }

    private fun runSdkCall(
        promise: Promise,
        call: suspend () -> String,
    ) {
        sdkCallScope.launch {
            try {
                promise.resolve(call())
            } catch (cancellation: CancellationException) {
                throw cancellation
            } catch (callFailure: Exception) {
                promise.reject(
                    FAILURE_CODE,
                    callFailure.message ?: callFailure.toString(),
                    callFailure,
                )
            }
        }
    }

    /** Runs one nullable SDK call; null resolves as JavaScript null. */
    private fun runNullableSdkCall(
        promise: Promise,
        call: suspend () -> String?,
    ) {
        sdkCallScope.launch {
            try {
                promise.resolve(call())
            } catch (cancellation: CancellationException) {
                throw cancellation
            } catch (callFailure: Exception) {
                promise.reject(
                    FAILURE_CODE,
                    callFailure.message ?: callFailure.toString(),
                    callFailure,
                )
            }
        }
    }

    private fun Promise.resolveCredentialResult(result: Pair<Boolean, String>) {
        val resultMap = Arguments.createMap()
        resultMap.putBoolean("isApplied", result.first)
        resultMap.putString("detailText", result.second)
        resolve(resultMap)
    }
}
