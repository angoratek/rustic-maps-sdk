package com.rusticmaps.sdk.refapprn

import android.content.Context
import android.content.res.AssetManager
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import android.util.Log
import com.facebook.react.bridge.Arguments
import com.facebook.react.bridge.Promise
import com.facebook.react.bridge.WritableArray
import com.mapssdkrn.MapsSdkMapHostView
import com.mapssdkrn.MapsSdkMapViewObserver
import com.mapssdkrn.MapsSdkNativeRuntime
import com.rusticmaps.sdk.MapCameraPose
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import kotlin.math.abs
import uniffi.rustic_maps.AvoidOptions
import uniffi.rustic_maps.EdgeInsets
import uniffi.rustic_maps.GeographicCoordinate
import uniffi.rustic_maps.LocationIndicatorState
import uniffi.rustic_maps.LocationIndicatorStyle
import uniffi.rustic_maps.LocationIndicatorVisualKind
import uniffi.rustic_maps.LogEvent
import uniffi.rustic_maps.Navigator
import uniffi.rustic_maps.MapOverlay
import uniffi.rustic_maps.MapOverlayKind
import uniffi.rustic_maps.OverlayColor
import uniffi.rustic_maps.MapSdk
import uniffi.rustic_maps.SdkDiagnosticEventListener
import uniffi.rustic_maps.SdkLogSeverity
import uniffi.rustic_maps.NavigationPhase
import uniffi.rustic_maps.NavigationPhaseChange
import uniffi.rustic_maps.NavigationInstruction
import uniffi.rustic_maps.DeviationStatusChange
import uniffi.rustic_maps.DeviationStatusChangeKind
import uniffi.rustic_maps.DistancePhraseKind
import uniffi.rustic_maps.DistanceUnit
import uniffi.rustic_maps.instructionBannerText
import uniffi.rustic_maps.Place
import uniffi.rustic_maps.Route
import uniffi.rustic_maps.RouteOptions
import uniffi.rustic_maps.RouteStrategy
import uniffi.rustic_maps.SdkException
import uniffi.rustic_maps.FrameStatistics
import uniffi.rustic_maps.TransportationMode
import uniffi.rustic_maps.UnitSystem
import uniffi.rustic_maps.getSdkCompiledVersion
import uniffi.rustic_maps.offlineRouteFromArchiveBytes
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * The React Native reference application's one SDK seam, mirroring the Compose
 * reference application's architecture: the [SurfaceView] lifecycle
 * (`attach`/`resize`/`detach` on the holder callbacks, the `ANativeWindow*` via
 * the JNI shim), the explicit per-frame draw pump (one serialized
 * [MapView.draw] per composed frame with the
 * `RenderingSurfaceUnavailable` backpressure retry), the SDK-owned camera math
 * (unprojection at the viewport center), the readiness ladder, the verbatim
 * error report, and the plan picker's recorded walk trace replay through
 * [Navigator].
 *
 * Everything the React side sees travels through [eventSink] as one event
 * channel; every SDK call failure reaches the React side verbatim (§6.3: no
 * catch block ends without a state change or an error presentation). All
 * controller work runs on the main looper, like every other seam.
 */
class RefappDemoSession private constructor() : MapsSdkMapViewObserver {

    companion object {
        private val sessionTag = "RefappDemoSession"

        /** The singleton seam the view manager and the React module share. */
        val instance: RefappDemoSession by lazy { RefappDemoSession() }

        /** Structural math constant: the full circle's degree sweep. */
        private const val FULL_CIRCLE_DEGREES: Double = 360.0

        /** Milliseconds in one second (the animated camera move's unit). */
        private const val MILLISECONDS_PER_SECOND: Long = 1_000L

        /** The bundled recorded trace the shell replays (the Compose copy task's file). */
        private const val TRACE_ASSET_NAME: String = "sacramento_clean_walk.json"
    }

    // ------------------------------------------------------------------
    // Eventing to the React side
    // ------------------------------------------------------------------

    /** The React module's event sink (set once the module is built). */
    var eventSink: ((eventName: String, arguments: android.os.Bundle) -> Unit)? = null

    internal fun emitEvent(eventName: String, arguments: android.os.Bundle) {
        eventSink?.invoke(eventName, arguments)
    }

    private fun emitReadiness(readyState: RefappDemoSession.ReadinessState, detailText: String?) {
        // The chip's display text is composed from the shared templates (§6.1);
        // the JS side renders it verbatim and needs no SDK vocabulary.
        val displayText = when (readyState) {
            ReadinessState.LOADING -> RefappConstants.LOADING_STATUS_TEXT +
                RefappConstants.READINESS_LOADING_SUFFIX
            ReadinessState.READY -> RefappConstants.READY_STATUS_TEMPLATE.format(compiledVersion)
            ReadinessState.DEGRADED -> RefappConstants.DEGRADED_STATUS_TEMPLATE
                .format(detailText ?: "unknown failure")
            ReadinessState.FAILED -> RefappConstants.FAILED_STATUS_TEMPLATE
                .format(detailText ?: "unknown failure")
        }
        val arguments = android.os.Bundle()
        arguments.putString("state", readyState.name.lowercase())
        arguments.putString("text", displayText)
        if (detailText != null) {
            arguments.putString("detail", detailText)
        }
        emitEvent("RefappReadiness", arguments)
    }

    private fun emitErrorReport(variantLabel: String, detailText: String) {
        val arguments = android.os.Bundle()
        arguments.putString("variantLabel", variantLabel)
        arguments.putString("detailText", detailText)
        emitEvent("RefappErrorReport", arguments)
    }

    private fun emitFrameStatistics(statistics: FrameStatistics?) {
        val arguments = android.os.Bundle()
        if (statistics == null) {
            arguments.putBoolean("live", false)
        } else {
            arguments.putBoolean("live", true)
            arguments.putInt("requestedTileCount", statistics.requestedTileCount.toInt())
            arguments.putInt("presentTileCount", statistics.presentTileCount.toInt())
            arguments.putInt("fallbackTileCount", statistics.fallbackTileCount.toInt())
            arguments.putInt("deferredTileCount", statistics.deferredTileCount.toInt())
            arguments.putInt("unavailableTileCount", statistics.unavailableTileCount.toInt())
            arguments.putInt("overlayTriangleCount", statistics.overlayTriangleCount.toInt())
        }
        arguments.putLong("framesDrawnCount", framesDrawnCount)
        arguments.putLong("framesDeferredCount", framesDeferredCount)
        arguments.putLong("framesFailedCount", framesFailedCount)
        emitEvent("RefappFrameStatistics", arguments)
    }

    private fun emitCameraReadback() {
        val camera = hostView()?.camera() ?: return
        val arguments = android.os.Bundle()
        arguments.putString(
            "text",
            "Center %.5f , %.5f · zoom %.2f · bearing %.0f° · pitch %.0f°".format(
                camera.latitudeDegrees,
                camera.longitudeDegrees,
                camera.zoomLevel,
                camera.bearingDegrees,
                camera.pitchDegrees,
            ),
        )
        // The control cluster's read-only state (§4.2): the plane label, the
        // compass visibility and the scale bar all read these values; the
        // scale answer is omitted while the camera has none (an honest no-bar
        // state, never an invented distance).
        arguments.putDouble("zoomLevel", camera.zoomLevel)
        arguments.putDouble("bearingDegrees", camera.bearingDegrees)
        arguments.putDouble("pitchDegrees", camera.pitchDegrees)
        arguments.putBoolean("isThreeDCameraEnabled", isThreeDCameraEnabled)
        // The my-location control's re-center affordance (§4.2/§5.3): a
        // gesture-broken follow reads false until the control re-engages it.
        arguments.putBoolean("isFollowing", isTraceReplayActive && followSimulatedPosition)
        arguments.putBoolean("hasLocationFix", latestLocationFix?.snappedCoordinate != null)
        hostView()?.groundMetersPerScreenPixel()?.let { groundMetersPerScreenPixel ->
            arguments.putDouble("groundMetersPerScreenPixel", groundMetersPerScreenPixel)
        }
        emitEvent("RefappCameraReadback", arguments)
    }

    /**
     * The plan flow's state event (§4.1): the route summary card's four
     * presentation values, emitted after every plan-state transition (in
     * flight, planned, refused, cleared).
     */
    private fun emitPlanState() {
        val arguments = android.os.Bundle()
        val plannedRoute = plannedRouteTrace
        arguments.putBoolean("isPlanned", plannedRoute != null)
        arguments.putBoolean("isInFlight", isPlanInFlight)
        arguments.putString(
            "titleText",
            plannedRouteNameText ?: RefappConstants.PLANNED_ROUTE_FALLBACK_NAME,
        )
        arguments.putBoolean("isRecordedDemoRoute", plannedRouteIsRecordedDemoRoute)
        arguments.putBoolean("isOfflineArchiveRoute", plannedRouteIsOfflineArchiveRoute)
        if (plannedRoute != null) {
            arguments.putString("distanceText", formatDistanceMeters(plannedRoute.route.distanceMeters))
            arguments.putString("durationText", formatDurationSeconds(plannedRoute.route.durationSeconds))
        }
        emitEvent("RefappPlanState", arguments)
    }

    /** The offline card's cache line (§4.1's two honest lines, first line). */
    private fun emitCacheState() {
        val arguments = android.os.Bundle()
        arguments.putBoolean("isRegistered", isTileCacheRegistered)
        arguments.putString("stateText", tileCacheStateText)
        emitEvent("RefappCacheState", arguments)
    }

    // ------------------------------------------------------------------
    // Spoken guidance (§4.1/§6.3; the speech contract)
    // ------------------------------------------------------------------

    /**
     * The app-owned TextToSpeech engine (the platform voice; the Compose
     * reference application's `SpeechGuidanceSpeaker` semantics), created
     * once by the React module's initialize. The React side's speech consumer
     * owns the dedup, pacing and toggle decisions and drives this engine
     * through the bridge module's speak/stop methods.
     */
    private var speechSpeaker: RefappSpeechSpeaker? = null

    /** Builds the speech engine once (the React module's initialize hop). */
    fun initializeSpeechEngine(context: android.content.Context) {
        if (speechSpeaker != null) {
            return
        }
        speechSpeaker = RefappSpeechSpeaker(context, ::emitSpeechEngineState)
    }

    /** The speech engine's honest ready-unavailable state (§6.2). */
    private fun emitSpeechEngineState(isEngineReady: Boolean, unavailableReasonText: String?) {
        val arguments = android.os.Bundle()
        arguments.putBoolean("isEngineReady", isEngineReady)
        if (unavailableReasonText != null) {
            arguments.putString("unavailableReasonText", unavailableReasonText)
        }
        emitEvent("RefappSpeechState", arguments)
    }

    /**
     * The speech topic's delivery crossing to the React side verbatim: the
     * contract's monotonic utterance id travels as its decimal string (the
     * bridge's number conversion is lossy above 2^53), and the trigger kind in
     * its lowercase enum label.
     */
    private fun emitSpeechInstruction(speechInstruction: uniffi.rustic_maps.SpeechInstruction) {
        val arguments = android.os.Bundle()
        arguments.putString("speechText", speechInstruction.utteranceSpeechText)
        arguments.putString("speechInstructionId", speechInstruction.utteranceId.toString())
        arguments.putString("instructionKind", speechInstruction.instructionKind.name.lowercase())
        emitEvent("RefappSpeechInstruction", arguments)
    }

    /** One consumer-voiced line through the platform engine (the bridge hop). */
    fun speakGuidanceUtterance(speechText: String, engineUtteranceId: String) {
        speechSpeaker?.speak(speechText, engineUtteranceId)
    }

    /** The drawer's toggle-off path: whatever speaks, stops. */
    fun stopGuidanceSpeech() {
        speechSpeaker?.stopSpeech()
    }

    /** The speech engine's current initialization state (the mount readback). */
    fun speechEngineState(): Pair<Boolean, String?> =
        speechSpeaker?.speechEngineState() ?: Pair(false, null)

    // ------------------------------------------------------------------
    // SDK objects (built once, on the first ensureSdkStarted call)
    // ------------------------------------------------------------------

    /** The compiled SDK version the diagnostics sheet reports from the probe. */
    val compiledVersion: String
        get() = MapsSdkNativeRuntime.compiledVersion

    /** The package-owned SDK (one native instance for the whole process). */
    private val nativeSdk: MapSdk?
        get() = MapsSdkNativeRuntime.mapSdk

    private val navigationController = Navigator()

    /**
     * The scope the suspend SDK calls (geocode, route calculation) run on:
     * main-immediate like every seam this controller drives, so a failed or
     * completed call emits its state before the next frame draws.
     */
    private val sdkCallScope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

    /** The application context the opt-in secure store operates under. */
    private var providerCredentialContext: Context? = null

    /** The shared demo artifact, loaded at SDK start (§9's native half). */
    private var scenesArtifact: RefappScenesArtifact? = null

    /** The application assets the offline routing archive is read from. */
    private var applicationAssetManager: AssetManager? = null

    private val navigationListener = RefappCollectingListener(
        onInstruction = { instruction -> emitTraceUpdateFromInstruction(instruction) },
        onDeviationStatusChange = { deviationStatusChange -> emitDeviationUpdate(deviationStatusChange) },
        onPhaseChange = { phaseChange -> emitPhaseUpdate(phaseChange) },
        onSpeechInstructionDelivery = { speechInstruction ->
            emitSpeechInstruction(speechInstruction)
        },
    )

    private val traceDriver = RefappTraceDriver(
        navigationController = navigationController,
        listener = navigationListener,
        onProcessedFix = { processedFix -> handleSimulatedProcessedFix(processedFix) },
        onArrival = { onSimulatedPlaybackEnded() },
    )

    /** Whether the SDK objects are built (`createSdk`'s outcome, honest). */
    var isSdkStarted: Boolean = false
        private set

    /**
     * The React module's initialize hop: remembers the application context
     * the opt-in secure store operates under. Without it every secure-store
     * operation reports its honest absence instead of failing silently.
     */
    fun initializeProviderCredentialStore(context: Context) {
        providerCredentialContext = context.applicationContext
    }

    /**
     * Builds the SDK objects keyless — no provider is registered, so the
     * no-key state is first-class and honest
     * (docs/Providers.md (Credentials) — loads the shared
     * demo artifact's native half, and publishes the honest unregistered
     * cache line. A credential entered at runtime registers later through
     * [setProviderCredential]. Every failure of the artifact reports verbatim
     * without failing the SDK start itself (the rendering surface needs no
     * provider).
     */
    public fun ensureSdkStarted(
        assetManager: android.content.res.AssetManager,
        cacheDirectoryPath: String,
    ): Boolean {
        if (isSdkStarted) {
            return true
        }
        return try {
            // The package runtime owns the one SDK; the demo only reads it.
            nativeSdk ?: throw IllegalStateException("the package SDK has not started")
            applicationAssetManager = assetManager
            isSdkStarted = true
            isTileCacheRegistered = MapsSdkNativeRuntime.isProviderCredentialApplied
            tileCacheStateText = MapsSdkNativeRuntime.tileCacheStateText
            emitErrorReportCleared()
            loadScenesArtifact(assetManager)
            emitCacheState()
            emitPlanState()
            hostView()
            true
        } catch (startupException: Exception) {
            if (startupException is CancellationException || startupException is InterruptedException) {
                throw startupException
            }
            isSdkStarted = false
            readinessState = ReadinessState.FAILED
            readinessDetail = failureDetailTextOf(startupException)
            val failureReport = failureReportOf(startupException)
            emitReadiness(ReadinessState.FAILED, readinessDetail)
            emitErrorReport(failureReport.first, failureReport.second)
            false
        }
    }

    private fun loadScenesArtifact(assetManager: android.content.res.AssetManager) {
        try {
            scenesArtifact = RefappScenesLoader.load(assetManager)
        } catch (artifactException: Exception) {
            if (artifactException is CancellationException) {
                throw artifactException
            }
            scenesArtifact = null
            reportFailureToReact(artifactException)
        }
    }

    /**
     * Applies one runtime provider credential (§3's explicit Apply,
     * docs/Providers.md): the `mapquest` arm
     * registers the provider set with the tile cache wrapped around the
     * raster-tile contract (the offline-maps demo) using the caches path the
     * keyless start remembered, then re-emits the readiness and cache state.
     * A refusal's verbatim detail is returned to the caller and published on
     * the error channel; the credential itself is never logged. A future
     * provider adds one dispatch arm, never a view rewrite (§9).
     */
    /**
     * The provider key an automation-injected credential targets when the
     * launch intent names none: the only provider the registration dispatch
     * serves. Kept here (the dispatch arm) so callers never spell a vendor.
     */
    fun defaultRuntimeInjectionProviderKey(): String =
        MapsSdkNativeRuntime.MAPQUEST_PROVIDER_KEY

    fun setProviderCredential(providerKey: String, credential: String): Pair<Boolean, String> {
        val (isApplied, detailText) = MapsSdkNativeRuntime.setProviderCredential(
            providerKey,
            credential,
            withTileCache = true,
        )
        if (isApplied) {
            isTileCacheRegistered = true
            tileCacheStateText = MapsSdkNativeRuntime.tileCacheStateText
            emitErrorReportCleared()
            emitCacheState()
            return Pair(true, "")
        }
        isTileCacheRegistered = false
        tileCacheStateText = detailText
        emitErrorReport("ProviderCredentialRefused", detailText)
        emitCacheState()
        return Pair(false, detailText)
    }

    // ------------------------------------------------------------------
    // The opt-in secure store (§3/§5: Android Keystore AES/GCM over a private
    // preferences file; the credential is never logged)
    // ------------------------------------------------------------------

    /** The stored credential, or null when nothing is stored or it cannot be read. */
    fun getStoredProviderCredential(providerKey: String): String? {
        val preferences = providerCredentialPreferences() ?: return null
        val storedValue = preferences.getString(
            providerCredentialPreferenceKey(providerKey),
            null,
        ) ?: return null
        return try {
            val separatorIndex = storedValue.indexOf(
                RefappConstants.PROVIDER_CREDENTIAL_IV_SEPARATOR,
            )
            if (separatorIndex <= 0) {
                null
            } else {
                val initializationVector = Base64.decode(
                    storedValue.substring(0, separatorIndex),
                    Base64.NO_WRAP,
                )
                val ciphertext = Base64.decode(
                    storedValue.substring(separatorIndex + 1),
                    Base64.NO_WRAP,
                )
                val cipher = Cipher.getInstance(
                    RefappConstants.PROVIDER_CREDENTIAL_CIPHER_TRANSFORMATION,
                )
                cipher.init(
                    Cipher.DECRYPT_MODE,
                    providerCredentialSecretKey(),
                    GCMParameterSpec(
                        RefappConstants.PROVIDER_CREDENTIAL_GCM_TAG_SIZE_BITS,
                        initializationVector,
                    ),
                )
                String(cipher.doFinal(ciphertext), Charsets.UTF_8)
            }
        } catch (decryptionFailure: Exception) {
            if (decryptionFailure is CancellationException ||
                decryptionFailure is InterruptedException
            ) {
                throw decryptionFailure
            }
            // A lost Keystore key or a corrupt entry reads as an honest absence.
            null
        }
    }

    /** Persists the credential behind the Android Keystore; false on failure. */
    fun storeProviderCredential(providerKey: String, credential: String): Boolean {
        val preferences = providerCredentialPreferences() ?: return false
        return try {
            val cipher = Cipher.getInstance(
                RefappConstants.PROVIDER_CREDENTIAL_CIPHER_TRANSFORMATION,
            )
            cipher.init(Cipher.ENCRYPT_MODE, providerCredentialSecretKey())
            val ciphertext = cipher.doFinal(credential.toByteArray(Charsets.UTF_8))
            val storedValue = Base64.encodeToString(cipher.iv, Base64.NO_WRAP) +
                RefappConstants.PROVIDER_CREDENTIAL_IV_SEPARATOR +
                Base64.encodeToString(ciphertext, Base64.NO_WRAP)
            preferences.edit()
                .putString(providerCredentialPreferenceKey(providerKey), storedValue)
                .commit()
        } catch (encryptionFailure: Exception) {
            if (encryptionFailure is CancellationException ||
                encryptionFailure is InterruptedException
            ) {
                throw encryptionFailure
            }
            false
        }
    }

    /** Removes the stored credential; false when the secure store is unavailable. */
    fun clearStoredProviderCredential(providerKey: String): Boolean {
        val preferences = providerCredentialPreferences() ?: return false
        return preferences.edit()
            .remove(providerCredentialPreferenceKey(providerKey))
            .commit()
    }

    private fun providerCredentialPreferences(): android.content.SharedPreferences? =
        providerCredentialContext?.getSharedPreferences(
            RefappConstants.PROVIDER_CREDENTIAL_PREFERENCES_FILE_NAME,
            Context.MODE_PRIVATE,
        )

    private fun providerCredentialPreferenceKey(providerKey: String): String =
        RefappConstants.PROVIDER_CREDENTIAL_PREFERENCE_KEY_PREFIX + providerKey

    private fun providerCredentialSecretKey(): SecretKey {
        val keyStore = KeyStore.getInstance(
            RefappConstants.PROVIDER_CREDENTIAL_ANDROID_KEYSTORE,
        )
        keyStore.load(null)
        val existingKey = keyStore.getKey(
            RefappConstants.PROVIDER_CREDENTIAL_KEYSTORE_ALIAS,
            null,
        ) as? SecretKey
        if (existingKey != null) {
            return existingKey
        }
        val keyGenerator = KeyGenerator.getInstance(
            KeyProperties.KEY_ALGORITHM_AES,
            RefappConstants.PROVIDER_CREDENTIAL_ANDROID_KEYSTORE,
        )
        keyGenerator.init(
            KeyGenParameterSpec.Builder(
                RefappConstants.PROVIDER_CREDENTIAL_KEYSTORE_ALIAS,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
            )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(RefappConstants.PROVIDER_CREDENTIAL_KEY_SIZE_BITS)
                .build(),
        )
        return keyGenerator.generateKey()
    }

    /** Whether the tile-cache registration succeeded (the offline card's state). */
    var isTileCacheRegistered: Boolean = false
        private set

    /** The cache line's text: the registered file and budget, or the verbatim refusal. */
    var tileCacheStateText: String = RefappConstants.CACHE_NOT_REGISTERED_TEXT
        private set

    private fun emitErrorReportCleared() {
        emitErrorReport("", "")
    }

    // ------------------------------------------------------------------
    // Readiness ladder (§6.1) and the error report (§6.3)
    // ------------------------------------------------------------------

    enum class ReadinessState { LOADING, READY, DEGRADED, FAILED }

    var readinessState: ReadinessState = ReadinessState.LOADING
        private set

    var readinessDetail: String? = null
        private set

    private var framesDrawnCount: Long = 0
    private var framesDeferredCount: Long = 0
    private var framesFailedCount: Long = 0
    private var latestTileStatistics: FrameStatistics? = null
    private var framesSinceStatisticsEmit: Long = 0

    /** Only the error currently on the React strip is cleared back (no flood). */
    private var lastReportedError: String? = null

    /** The verbatim detail text of any failure (§6.3's mapping). */
    private fun failureDetailTextOf(failure: Throwable): String = when (failure) {
        is SdkException.InvalidCoordinateInput -> failure.detail
        is SdkException.NetworkRequestFailed -> failure.detail
        is SdkException.ResponseParsingFailure -> failure.detail
        is SdkException.ProviderRequestRejected -> failure.detail
        is SdkException.ProviderNotConfigured -> "no '${failure.contractName}' provider is configured"
        is SdkException.InvalidRenderInput -> failure.detail
        is SdkException.RenderingSurfaceUnavailable -> failure.detail
        is SdkException.InvalidNavigationInput -> failure.detail
        is SdkException.CacheOperationFailure -> failure.detail
        is SdkException.OfflineRoutingFailure -> failure.detail
        is SdkException.RateLimitExhausted -> "rate limit exhausted; retry after the next refill"
        else -> failure.message ?: failure::class.simpleName ?: "unknown failure"
    }

    private fun failureReportOf(failure: Throwable): Pair<String, String> = Pair(
        failure::class.simpleName ?: "UnknownException",
        failureDetailTextOf(failure),
    )

    private fun reportFailureToReact(failure: Throwable) {
        val failureReport = failureReportOf(failure)
        lastReportedError = failureReport.first
        Log.w(sessionTag, "reported failure: ${failureReport.first}: ${failureReport.second}")
        emitErrorReport(failureReport.first, failureReport.second)
    }

    private fun clearErrorOnReact() {
        emitErrorReportCleared()
    }

    // ------------------------------------------------------------------
    // The package MapView (surface, pump, gestures and camera live there)
    // ------------------------------------------------------------------

    /** The host view the demo drives; it also receives the camera readback. */
    private var observedHostView: MapsSdkMapHostView? = null

    /** Whether a camera command has landed before the host view existed. */
    private var hasCameraIntent: Boolean = false

    private fun hostView(): MapsSdkMapHostView? {
        val currentHostView = MapsSdkMapHostView.currentHostView()
        if (currentHostView !== observedHostView) {
            observedHostView?.removeObserver(this)
            observedHostView = currentHostView
            currentHostView?.addObserver(this)
            if (currentHostView != null && hasCameraIntent) {
                currentHostView.setCamera(desiredCameraPose())
            }
        }
        return currentHostView
    }

    /** The user-facing attach retry (the package view's own retry path). */
    fun retrySurfaceAttach() {
        hostView()?.retryAttach()
        clearErrorOnReact()
    }

    // ------------------------------------------------------------------
    // Camera state (the SDK owns the camera math; the app owns the values)
    // ------------------------------------------------------------------

    private var centerLatitudeDegrees: Double = 0.0
    private var centerLongitudeDegrees: Double = 0.0
    private var zoomLevel: Double = 0.0
    private var bearingDegrees: Double = 0.0
    private var pitchDegrees: Double = 0.0
    private var followSimulatedPosition: Boolean = false

    /** The 2D/3D plane the camera targets (§4.2's plane toggle). */
    private var isThreeDCameraEnabled: Boolean = false

    /** The camera pose the demo's controls last asked the package view for. */
    private fun desiredCameraPose(): MapCameraPose = MapCameraPose(
        latitudeDegrees = centerLatitudeDegrees,
        longitudeDegrees = centerLongitudeDegrees,
        zoomLevel = zoomLevel,
        bearingDegrees = bearingDegrees,
        pitchDegrees = pitchDegrees,
    )

    /** Applies the desired pose to the package view (a set, or an eased move). */
    private fun applyDesiredCamera(animated: Boolean = false) {
        hasCameraIntent = true
        val packageHostView = hostView() ?: return
        val pose = desiredCameraPose()
        if (animated) {
            packageHostView.animateCamera(
                pose,
                (RefappConstants.EASED_CAMERA_MOVE_DURATION_SECONDS * MILLISECONDS_PER_SECOND).toLong(),
            )
        } else {
            packageHostView.setCamera(pose)
        }
    }

    /** Adopts the package view's current camera into the demo's intent state. */
    private fun syncCameraStateFromHost() {
        val camera = hostView()?.camera() ?: return
        centerLatitudeDegrees = camera.latitudeDegrees
        centerLongitudeDegrees = camera.longitudeDegrees
        zoomLevel = camera.zoomLevel
        bearingDegrees = camera.bearingDegrees
        pitchDegrees = camera.pitchDegrees
    }

    /** A camera change signal (the package view owns the math and the pump). */
    private fun markCameraRevision() {
    }

    /**
     * Applies one camera preset (the artifact's camera presets, fed from the
     * React side, which requires the JSON artifact — the Metro asset).
     */
    fun applyCameraPreset(
        presetLatitudeDegrees: Double,
        presetLongitudeDegrees: Double,
        presetZoomLevel: Double,
        presetBearingDegrees: Double,
        presetPitchDegrees: Double,
    ) {
        centerLatitudeDegrees = presetLatitudeDegrees
        centerLongitudeDegrees = presetLongitudeDegrees
        zoomLevel = presetZoomLevel
        bearingDegrees = presetBearingDegrees
        pitchDegrees = presetPitchDegrees
        applyDesiredCamera()
        clearErrorOnReact()
    }

    /**
     * Drag-pan (§5.2): the pixel delta crosses the package view's contract
     * camera member, so the RN shell applies the same gesture algebra as every
     * other shell. Gestures are platform glue, the math is the SDK's.
     */
    fun panByPixels(deltaXPixels: Double, deltaYPixels: Double) {
        breakFollowOnGesture()
        hostView()?.panByScreenPixels(deltaXPixels, deltaYPixels, true)
        syncCameraStateFromHost()
    }

    // ------------------------------------------------------------------
    // The map control cluster (§4.2)
    // ------------------------------------------------------------------

    /** One zoom control step (§2.4's `map-control-zoom-step`; +1 or −1). */
    fun zoomByControlStep(stepSign: Double) {
        hostView()?.zoomByLevels(stepSign * RefappConstants.MAP_CONTROL_ZOOM_STEP)
        syncCameraStateFromHost()
    }

    fun zoomInControl() = zoomByControlStep(1.0)

    fun zoomOutControl() = zoomByControlStep(-1.0)

    /**
     * The my-location control (§4.2): centers on the indicator's latest fix;
     * during a trip it also re-engages the follow camera.
     */
    fun centerOnLatestLocation() {
        val latestFix = latestLocationFix?.snappedCoordinate ?: return
        centerLatitudeDegrees = latestFix.latitude
        centerLongitudeDegrees = latestFix.longitude
        if (isTraceReplayActive) {
            followSimulatedPosition = true
        }
        applyDesiredCamera()
    }

    /** The 2D/3D toggle (§4.2): swings the pitch between the two planes. */
    fun toggleCameraPlane() {
        isThreeDCameraEnabled = !isThreeDCameraEnabled
        pitchDegrees = cameraPitchTargetDegrees()
        applyDesiredCamera(animated = true)
    }

    /**
     * The compass tap (§2.4): eases north up over the token's 300 ms; a
     * camera edit breaks the follow camera like any gesture.
     */
    fun easeCameraNorthUp() {
        breakFollowOnGesture()
        bearingDegrees = RefappConstants.NORTH_UP_BEARING_DEGREES
        applyDesiredCamera(animated = true)
    }

    /**
     * A user gesture (§5.3: drag/pinch/rotate) breaks follow immediately; it
     * stays broken until the my-location control re-engages it — a later fix
     * does not.
     */
    fun breakFollowOnGesture() {
        if (isTraceReplayActive) {
            followSimulatedPosition = false
        }
    }

    private fun cameraPitchTargetDegrees(): Double =
        if (isThreeDCameraEnabled) {
            RefappConstants.NAVIGATION_FOLLOW_PITCH_3D_DEGREES
        } else {
            RefappConstants.TWO_D_CAMERA_PITCH_DEGREES
        }

    /** One native camera-change notification from the package host view. */
    override fun onCameraChanged(camera: MapCameraPose, isGestureDriven: Boolean) {
        centerLatitudeDegrees = camera.latitudeDegrees
        centerLongitudeDegrees = camera.longitudeDegrees
        zoomLevel = camera.zoomLevel
        bearingDegrees = camera.bearingDegrees
        pitchDegrees = camera.pitchDegrees
        if (isGestureDriven) {
            breakFollowOnGesture()
        }
        emitCameraReadback()
    }

    override fun onMapReady() {
        emitCameraReadback()
    }

    // ------------------------------------------------------------------
    // Search (geocode) and route planning (§4.1's plan flow)
    // ------------------------------------------------------------------

    /** Whether a provider search is running (one search at a time). */
    var isSearchInFlight: Boolean = false
        private set

    /**
     * Runs the plan search on one field's text: the result rows travel to the
     * React side through the promise (a `structuredAddressText`-plus-coordinate
     * row per provider result, the Compose app's row semantics) — never from
     * guesses; a provider failure reports itself in the error strip and
     * rejects the promise (§4.1).
     */
    fun searchPlaces(queryText: String, promise: Promise) {
        if (!isSdkStarted) {
            promise.reject("searchUnavailable", RefappConstants.SEARCH_BEFORE_SDK_STATE_TEXT)
            return
        }
        if (isSearchInFlight) {
            promise.reject("searchInFlight", RefappConstants.SEARCH_IN_FLIGHT_TEXT)
            return
        }
        if (queryText.isBlank()) {
            promise.resolve(Arguments.createArray())
            return
        }
        val constructedSdk = nativeSdk ?: return
        isSearchInFlight = true
        sdkCallScope.launch {
            try {
                val results = constructedSdk.forwardGeocode(queryText)
                val resultRows: WritableArray = Arguments.createArray()
                results.take(RefappConstants.SEARCH_RESULT_ROW_LIMIT).forEach { placeResult ->
                    val row = Arguments.createMap()
                    row.putString("structuredAddressText", placeResult.structuredAddressText)
                    row.putDouble("latitudeDegrees", placeResult.coordinate.latitude)
                    row.putDouble("longitudeDegrees", placeResult.coordinate.longitude)
                    row.putString("qualityText", placeResult.geocodeQuality.name.lowercase())
                    resultRows.pushMap(row)
                }
                clearErrorOnReact()
                promise.resolve(resultRows)
            } catch (searchException: Exception) {
                if (searchException is CancellationException ||
                    searchException is InterruptedException
                ) {
                    // A cancelled search is composition teardown, not a demo
                    // failure: rethrow, never report (the false-positive rule).
                    throw searchException
                }
                // Exception, not SdkException alone: native-internal failures
                // (uniffi InternalException) are no SdkException variant, and
                // failing search must report, never crash (§6.3).
                reportFailureToReact(searchException)
                promise.reject("searchFailed", failureDetailTextOf(searchException))
            } finally {
                isSearchInFlight = false
            }
        }
    }

    /** The resolved origin coordinate, or null while the demo-position default stands. */
    private var originPlaceCoordinate: GeographicCoordinate? = null

    /** The resolved destination coordinate, or null while none is picked. */
    private var destinationPlaceCoordinate: GeographicCoordinate? = null

    /** The planned route display name (the summary card's title). */
    private var plannedRouteNameText: String? = null

    /** The search-planned route's trip source, or null while none is planned. */
    private var plannedRouteTrace: RefappTrace? = null

    /**
     * Whether the planned route is the shared artifact's recorded demo route
     * rather than a live planning result (the summary card says so).
     */
    private var plannedRouteIsRecordedDemoRoute: Boolean = false

    /**
     * Whether the planned route came from the bundled offline test graph:
     * the route summary says it is recorded test data, not live data.
     */
    private var plannedRouteIsOfflineArchiveRoute: Boolean = false

    /** Whether a route calculation is running (one plan at a time). */
    var isPlanInFlight: Boolean = false
        private set

    /** Whether the running SDK session has a provider credential applied. */
    fun isProviderCredentialApplied(): Boolean =
        MapsSdkNativeRuntime.isProviderCredentialApplied

    /** The active route the demo drives: the planned route, else the picker's trace. */
    private val activeRoute: Route?
        get() = activeLoadedTrace?.route

    /**
     * Assigns one provider result row to the plan field the search serves;
     * there is no default origin (§4.1: the two-field flow asks for one).
     * Assignment alone never plans: the explicit Plan route action is the
     * only path that plans, and the promise resolves `true` once assigned.
     */
    fun assignSearchResult(
        assignToOrigin: Boolean,
        placeNameText: String,
        latitudeDegrees: Double,
        longitudeDegrees: Double,
        promise: Promise,
    ) {
        val assignedCoordinate = GeographicCoordinate(
            latitude = latitudeDegrees,
            longitude = longitudeDegrees,
        )
        if (assignToOrigin) {
            originPlaceCoordinate = assignedCoordinate
        } else {
            destinationPlaceCoordinate = assignedCoordinate
            plannedRouteNameText = placeNameText
        }
        clearErrorOnReact()
        promise.resolve(true)
    }

    /**
     * The explicit Plan route action (§4.1): plans between the two assigned
     * coordinates with the plan sheet's Directions choice
     * (PRD.md §6 item 36). A missing field rejects with a
     * reason naming it; nothing is invented.
     */
    fun planRoute(transportationModeOrdinal: Int, routeStrategyOrdinal: Int, promise: Promise) {
        val resolvedOriginCoordinate = originPlaceCoordinate
        val resolvedDestinationCoordinate = destinationPlaceCoordinate
        if (resolvedOriginCoordinate == null) {
            promise.reject("planUnavailable", RefappConstants.PLAN_ORIGIN_UNRESOLVED_TEXT)
            return
        }
        if (resolvedDestinationCoordinate == null) {
            promise.reject("planUnavailable", RefappConstants.PLAN_DESTINATION_UNRESOLVED_TEXT)
            return
        }
        val transportationModes = TransportationMode.values()
        val routeStrategies = RouteStrategy.values()
        if (transportationModeOrdinal !in transportationModes.indices) {
            promise.reject(
                "planUnavailable",
                RefappConstants.PLAN_UNKNOWN_DIRECTIONS_CHOICE_TEXT,
            )
            return
        }
        if (routeStrategyOrdinal !in routeStrategies.indices) {
            promise.reject(
                "planUnavailable",
                RefappConstants.PLAN_UNKNOWN_DIRECTIONS_CHOICE_TEXT,
            )
            return
        }
        planRouteBetween(
            waypoints = listOf(resolvedOriginCoordinate, resolvedDestinationCoordinate),
            planNameText = plannedRouteNameText ?: RefappConstants.PLANNED_ROUTE_FALLBACK_NAME,
            routeOptions = referenceRouteOptions(
                transportationModes[transportationModeOrdinal],
                routeStrategies[routeStrategyOrdinal],
            ),
            promise = promise,
        )
    }

    /**
     * The keyless plan (§4.1): applies the shared artifact's recorded route
     * for the ordered demo-place pair — no provider call and no invented
     * geometry. A pair the artifact does not record reports the named reason
     * and rejects, exactly as the explicit live plan reports a refusal.
     */
    fun planRecordedDemoRoute(originName: String, destinationName: String, promise: Promise) {
        if (isPlanInFlight) {
            promise.reject("planInFlight", RefappConstants.PLAN_ALREADY_RUNNING_TEXT)
            return
        }
        val recordedRoute = scenesArtifact?.demoRoutes?.firstOrNull { demoRoute ->
            demoRoute.originName == originName && demoRoute.destinationName == destinationName
        }
        if (recordedRoute == null) {
            val detailText = RefappConstants.NO_RECORDED_ROUTE_HINT_TEMPLATE
                .format(originName, destinationName)
            emitErrorReport("MissingRecordedRoute", detailText)
            promise.reject("noRecordedRoute", detailText)
            return
        }
        isPlanInFlight = true
        emitPlanState()
        try {
            plannedRouteTrace = RefappTrace.fromRouteData(
                route = recordedRoute.route,
                traceName = RefappConstants.PLANNED_ROUTE_TRACE_NAME,
            )
            plannedRouteNameText = destinationName
            plannedRouteIsRecordedDemoRoute = true
            plannedRouteIsOfflineArchiveRoute = false
            applyRoutePreviewFraming(recordedRoute.route)
            pushOverlaysToSurface()
            clearErrorOnReact()
            promise.resolve(true)
        } catch (planException: Exception) {
            if (planException is CancellationException ||
                planException is InterruptedException
            ) {
                throw planException
            }
            plannedRouteTrace = null
            plannedRouteNameText = null
            plannedRouteIsRecordedDemoRoute = false
            plannedRouteIsOfflineArchiveRoute = false
            reportFailureToReact(planException)
            promise.reject("routePlanFailed", failureDetailTextOf(planException))
        } finally {
            isPlanInFlight = false
            emitPlanState()
        }
    }

    /**
     * The offline card's route control (§2.5 `offline.plan_route_action`):
     * reads the committed compiled routing archive from the app's assets and
     * plans the demo request over it through the contract's
     * `offlineRouteFromArchiveBytes` free function. The route feeds the same
     * summary card, overlay line and simulated trip as a live plan, and the
     * summary says it is recorded test data, not live data. No provider and no
     * network round trip are involved.
     */
    fun planOfflineRouteFromArchive(promise: Promise) {
        if (isPlanInFlight) {
            promise.reject("planInFlight", RefappConstants.PLAN_ALREADY_RUNNING_TEXT)
            return
        }
        val assetManager = applicationAssetManager
        if (assetManager == null) {
            promise.reject("planUnavailable", RefappConstants.SEARCH_BEFORE_SDK_STATE_TEXT)
            return
        }
        val archiveBytes = try {
            assetManager.open(RefappConstants.OFFLINE_ROUTING_ARCHIVE_ASSET_NAME)
                .use { archiveStream -> archiveStream.readBytes() }
        } catch (archiveException: Exception) {
            // A missing or unreadable bundled archive reports verbatim; it is
            // a build-asset failure, never a silent no-op.
            reportFailureToReact(archiveException)
            promise.reject("offlineArchiveUnavailable", failureDetailTextOf(archiveException))
            return
        }
        isPlanInFlight = true
        emitPlanState()
        sdkCallScope.launch {
            try {
                val route = offlineRouteFromArchiveBytes(
                    archiveBytes = archiveBytes,
                    start = GeographicCoordinate(
                        latitude = RefappConstants.OFFLINE_ROUTE_START_LATITUDE_DEGREES,
                        longitude = RefappConstants.OFFLINE_ROUTE_START_LONGITUDE_DEGREES,
                    ),
                    end = GeographicCoordinate(
                        latitude = RefappConstants.OFFLINE_ROUTE_END_LATITUDE_DEGREES,
                        longitude = RefappConstants.OFFLINE_ROUTE_END_LONGITUDE_DEGREES,
                    ),
                )
                plannedRouteTrace = RefappTrace.fromRouteData(
                    route = route,
                    traceName = RefappConstants.PLANNED_ROUTE_TRACE_NAME,
                )
                plannedRouteNameText = RefappConstants.OFFLINE_ROUTE_TITLE_TEXT
                plannedRouteIsRecordedDemoRoute = false
                plannedRouteIsOfflineArchiveRoute = true
                applyRoutePreviewFraming(route)
                pushOverlaysToSurface()
                clearErrorOnReact()
                promise.resolve(true)
            } catch (routeException: Exception) {
                if (routeException is CancellationException ||
                    routeException is InterruptedException
                ) {
                    // A cancelled plan is composition teardown, not a demo
                    // failure: rethrow, never report (the false-positive rule).
                    throw routeException
                }
                // A refused archive (invalid bytes, an out-of-graph
                // coordinate, a disconnected pair) reports its verbatim
                // detail through the same error channel a live refusal uses.
                plannedRouteTrace = null
                plannedRouteNameText = null
                plannedRouteIsRecordedDemoRoute = false
                plannedRouteIsOfflineArchiveRoute = false
                reportFailureToReact(routeException)
                promise.reject("offlineRoutePlanFailed", failureDetailTextOf(routeException))
            } finally {
                // The one settle-time emission: it must run after the in-flight
                // flag clears, else the summary renders the planning line.
                isPlanInFlight = false
                emitPlanState()
            }
        }
    }

    /**
     * Plans the route through the two resolved waypoints (§4.1): builds the
     * simulated trip source over the route's own shape, fits the preview
     * framing and pushes the overlay set — then emits the plan state the
     * route summary card renders.
     */
    private fun planRouteBetween(
        waypoints: List<GeographicCoordinate>,
        planNameText: String,
        routeOptions: RouteOptions,
        promise: Promise,
    ) {
        if (isPlanInFlight) {
            promise.reject("planInFlight", RefappConstants.PLAN_ALREADY_RUNNING_TEXT)
            return
        }
        val constructedSdk = nativeSdk
        if (constructedSdk == null) {
            promise.reject("planUnavailable", RefappConstants.SEARCH_BEFORE_SDK_STATE_TEXT)
            return
        }
        isPlanInFlight = true
        emitPlanState()
        sdkCallScope.launch {
            try {
                val route = constructedSdk.calculateRoute(waypoints, routeOptions)
                val tripTrace = RefappTrace.fromRouteData(
                    route = route,
                    traceName = RefappConstants.PLANNED_ROUTE_TRACE_NAME,
                )
                plannedRouteTrace = tripTrace
                plannedRouteNameText = planNameText
                plannedRouteIsRecordedDemoRoute = false
                plannedRouteIsOfflineArchiveRoute = false
                applyRoutePreviewFraming(route)
                pushOverlaysToSurface()
                clearErrorOnReact()
                promise.resolve(true)
            } catch (routeException: Exception) {
                if (routeException is CancellationException ||
                    routeException is InterruptedException
                ) {
                    // A cancelled route plan is composition teardown, not a
                    // demo failure: rethrow, never report (false positive rule).
                    throw routeException
                }
                // Exception, not SdkException alone: native-internal failures
                // (uniffi InternalException) are no SdkException variant, and
                // failing routing must report, never crash (§6.3).
                plannedRouteTrace = null
                plannedRouteNameText = null
                plannedRouteIsRecordedDemoRoute = false
                plannedRouteIsOfflineArchiveRoute = false
                reportFailureToReact(routeException)
                promise.reject("routePlanFailed", failureDetailTextOf(routeException))
            } finally {
                // The one settle-time emission (both paths): it must run
                // AFTER the in-flight flag clears, else the summary card
                // renders the planning line forever.
                isPlanInFlight = false
                emitPlanState()
            }
        }
    }

    /** The routing options of one plan: the plan sheet's Directions choice with
     * metric display units and no avoided road features. */
    private fun referenceRouteOptions(
        transportationMode: TransportationMode,
        routeStrategy: RouteStrategy,
    ): RouteOptions = RouteOptions(
        transportationMode = transportationMode,
        routeStrategy = routeStrategy,
        unitSystem = UnitSystem.METRIC,
        avoidOptions = AvoidOptions(
            tollRoads = false,
            limitedAccessHighways = false,
            ferries = false,
            unpavedRoads = false,
        ),
        departureTimestampSeconds = null,
    )

    /**
     * Fits the planned route between the waypoints (§5.1 camera fitting): the
     * bottom inset clears the half-state sheet so the preview line stays
     * visible above it, and the fitted camera's values read back through the
     * attribute getters.
     */
    private fun applyRoutePreviewFraming(route: Route) {
        val packageHostView = hostView() ?: return
        val viewportWidthPixels = packageHostView.viewportWidthPixels
        val viewportHeightPixels = packageHostView.viewportHeightPixels
        if (viewportWidthPixels == 0 || viewportHeightPixels == 0) {
            return
        }
        packageHostView.fitBounds(
            boundingBox = route.boundingBox,
            edgeInsets = EdgeInsets(
                left = RefappConstants.ROUTE_PREVIEW_INSET_LEFT_PIXELS,
                top = RefappConstants.ROUTE_PREVIEW_INSET_TOP_PIXELS,
                right = RefappConstants.ROUTE_PREVIEW_INSET_RIGHT_PIXELS,
                bottom = viewportHeightPixels.toDouble() *
                    RefappConstants.SHEET_HALF_HEIGHT_FRACTION,
            ),
            maximumZoomLevel = RefappConstants.MAXIMUM_FIT_BOUNDS_ZOOM_LEVEL,
        )
        syncCameraStateFromHost()
        clearErrorOnReact()
    }

    // ------------------------------------------------------------------
    // Scene overlays (§9: the shared artifact feeds the SDK overlay API)
    // ------------------------------------------------------------------

    /** Toggle state per artifact overlay scene key (route/polygon/markers/traffic). */
    private val sceneToggleSelections: MutableMap<String, Boolean> =
        RefappConstants.OVERLAY_SCENE_KEYS.associateWith { false }.toMutableMap()

    /** Count of overlays the last surface push carried (diagnostics fact). */
    var pushedOverlayCount: Int = 0
        private set

    /** Sets one scene chip's selection (§4.1's chip carousel) and repushes. */
    fun setSceneOverlayToggle(sceneKey: String, isSelected: Boolean) {
        if (!sceneToggleSelections.containsKey(sceneKey)) {
            return
        }
        sceneToggleSelections[sceneKey] = isSelected
        pushOverlaysToSurface()
    }

    /** The Clear control: every chip unselected and the surface overlay list empty. */
    fun clearSceneOverlays() {
        sceneToggleSelections.keys.forEach { sceneKey -> sceneToggleSelections[sceneKey] = false }
        pushOverlaysToSurface()
    }

    private fun sceneDescriptor(sceneKey: String, scene: RefappOverlayScene): MapOverlay? {
        if (sceneToggleSelections[sceneKey] != true) {
            return null
        }
        return when {
            scene.coordinates != null -> MapOverlay(
                kind = MapOverlayKind.ROUTE_POLYLINE,
                coordinates = scene.coordinates,
                rgbaColor = null,
                widthPixels = null,
                ringCoordinates = null,
                pointMarkerPositions = null,
                pointMarkerRadiusPixels = null,
                trafficFlowSegments = null,
            )
            scene.ringCoordinates != null -> MapOverlay(
                kind = MapOverlayKind.FILLED_POLYGON,
                coordinates = null,
                rgbaColor = null,
                widthPixels = null,
                ringCoordinates = scene.ringCoordinates,
                pointMarkerPositions = null,
                pointMarkerRadiusPixels = null,
                trafficFlowSegments = null,
            )
            scene.positions != null -> MapOverlay(
                kind = MapOverlayKind.POINT_MARKERS,
                coordinates = null,
                rgbaColor = null,
                widthPixels = null,
                ringCoordinates = null,
                pointMarkerPositions = scene.positions,
                pointMarkerRadiusPixels = null,
                trafficFlowSegments = null,
            )
            scene.trafficFlowSegments != null -> MapOverlay(
                kind = MapOverlayKind.TRAFFIC_FLOW,
                coordinates = null,
                rgbaColor = null,
                widthPixels = null,
                ringCoordinates = null,
                pointMarkerPositions = null,
                pointMarkerRadiusPixels = null,
                trafficFlowSegments = scene.trafficFlowSegments,
            )
            else -> null
        }
    }

    /**
     * The whole overlay list of the current frame: the demo scenes of the
     * shared artifact plus the active plan's route line. Overlay style falls
     * to the documented rustic-maps-rendering defaults — GPU-surface paint is a
     * renderer constant, not a UX token (§2.2).
     */
    private fun overlayDescriptorList(): List<MapOverlay> = buildList {
        RefappConstants.OVERLAY_SCENE_KEYS.forEach { sceneKey ->
            scenesArtifact?.overlayScenes?.get(sceneKey)?.let { scene ->
                sceneDescriptor(sceneKey, scene)?.let { descriptor -> add(descriptor) }
            }
        }
        activeRoute?.let { activeRouteValue ->
            add(
                MapOverlay(
                    kind = MapOverlayKind.ROUTE_POLYLINE,
                    coordinates = activeRouteValue.legs
                        .flatMap { leg -> leg.steps }
                        .flatMap { step -> step.shape },
                    rgbaColor = null,
                    widthPixels = null,
                    ringCoordinates = null,
                    pointMarkerPositions = null,
                    pointMarkerRadiusPixels = null,
                    trafficFlowSegments = null,
                ),
            )
            // The route's A/B endpoint pins (§4.1), the same additive overlay
            // kind and colors the other shells draw.
            val originCoordinate = activeRouteValue.legs.firstOrNull()
                ?.steps?.firstOrNull()?.shape?.firstOrNull()
            val destinationCoordinate = activeRouteValue.legs.lastOrNull()
                ?.steps?.lastOrNull()?.shape?.lastOrNull()
            if (originCoordinate != null) {
                add(
                    endPointPinDescriptor(
                        anchorCoordinate = originCoordinate,
                        pinColor = OverlayColor(red = 0.13, green = 0.62, blue = 0.29, alpha = 1.0),
                    ),
                )
            }
            if (destinationCoordinate != null) {
                add(
                    endPointPinDescriptor(
                        anchorCoordinate = destinationCoordinate,
                        pinColor = OverlayColor(red = 0.85, green = 0.16, blue = 0.20, alpha = 1.0),
                    ),
                )
            }
        }
    }

    /** One endpoint pin descriptor at the documented default head radius. */
    private fun endPointPinDescriptor(
        anchorCoordinate: GeographicCoordinate,
        pinColor: OverlayColor,
    ): MapOverlay = MapOverlay(
        kind = MapOverlayKind.POINT_PINS,
        coordinates = null,
        rgbaColor = pinColor,
        widthPixels = null,
        ringCoordinates = null,
        pointMarkerPositions = listOf(anchorCoordinate),
        pointMarkerRadiusPixels = null,
        trafficFlowSegments = null,
    )

    /** Pushes the current overlay list to the surface; a refusal reports verbatim. */
    fun pushOverlaysToSurface() {
        try {
            val overlayList = overlayDescriptorList()
            hostView()?.setOverlays(overlayList)
            pushedOverlayCount = overlayList.size
            clearErrorOnReact()
        } catch (overlayException: Exception) {
            if (overlayException is CancellationException) {
                // A cancelled push is composition teardown, not a demo
                // failure: rethrow, never report (the false-positive rule).
                throw overlayException
            }
            reportFailureToReact(overlayException)
        }
    }

    // ------------------------------------------------------------------
    // The renderer-owned location indicator (§4.2)
    // ------------------------------------------------------------------

    /**
     * The latest simulated fix the indicator shows, or null when clear (no
     * fix renders nothing — never a stale last position, §4.2).
     */
    private var latestLocationFix: RefappProcessedFix? = null

    /**
     * Pushes the location indicator for one fix to the renderer (§4.2: the
     * visuals are renderer-owned; the app only sets state/style): the guided
     * trip draws the `GuidedCourseArrow` wedge along the reported course,
     * tracking draws the `TrackingDot`, and the accuracy ring follows the
     * fix's own report.
     */
    private fun pushLocationIndicator(guided: Boolean, fix: RefappProcessedFix) {
        val snappedCoordinate = fix.snappedCoordinate ?: return
        try {
            hostView()?.setLocationIndicator(
                state = LocationIndicatorState(
                    coordinate = GeographicCoordinate(
                        latitude = snappedCoordinate.latitude,
                        longitude = snappedCoordinate.longitude,
                    ),
                    courseHeadingDegrees = if (guided) fix.courseHeadingDegrees else null,
                    horizontalAccuracyMeters = fix.horizontalAccuracyMeters,
                ),
                style = LocationIndicatorStyle(
                    visualKind = if (guided) {
                        LocationIndicatorVisualKind.GUIDED_COURSE_ARROW
                    } else {
                        LocationIndicatorVisualKind.TRACKING_DOT
                    },
                    radiusPixels = null,
                    rgbaColor = null,
                    ringRgbaColor = null,
                    haloRgbaColor = null,
                    showAccuracyRing = true,
                ),
            )
            latestLocationFix = fix
        } catch (indicatorException: Exception) {
            if (indicatorException is CancellationException) {
                throw indicatorException
            }
            reportFailureToReact(indicatorException)
        }
    }

    /** Clears the renderer's location layer (§4.2's no-fix state). */
    private fun clearLocationIndicatorOnSurface() {
        if (latestLocationFix == null) {
            return
        }
        try {
            hostView()?.clearLocationIndicator()
            latestLocationFix = null
        } catch (indicatorException: Exception) {
            if (indicatorException is CancellationException) {
                throw indicatorException
            }
            reportFailureToReact(indicatorException)
        }
    }

    // ------------------------------------------------------------------
    // The navigation follow camera (§5.3)
    // ------------------------------------------------------------------

    /** The camera values captured at trip start and restored at trip end. */
    private data class PreTripCamera(
        val centerLatitudeDegrees: Double,
        val centerLongitudeDegrees: Double,
        val zoomLevel: Double,
        val bearingDegrees: Double,
        val pitchDegrees: Double,
    )

    /** The captured pre-trip camera, or null outside a trip. */
    private var preTripCamera: PreTripCamera? = null

    /** The follow camera's bearing target: the median-smoothed course, or null. */
    private var followTargetBearingDegrees: Double? = null

    /** The median-smoothed course-over-ground (§5.3; the Compose constant window). */
    private val courseMedianSmoother = CourseMedianSmoother(
        RefappConstants.COURSE_SMOOTHING_WINDOW_FIXES,
    )

    /**
     * One simulated fix of the active trip: the follow camera's targets
     * (center, the median-smoothed bearing and the ramped zoom) update on
     * every fix, but the camera only moves while follow is engaged — a
     * gesture-broken follow keeps the user's panned view until the
     * my-location control re-engages it (§5.3). The renderer-owned location
     * indicator follows the fix in the guided wedge form regardless.
     */
    private fun handleSimulatedProcessedFix(processedFix: RefappProcessedFix) {
        val snappedCoordinate = processedFix.snappedCoordinate ?: return
        if (isTraceReplayActive && traceDriver.isTripActive) {
            processedFix.courseHeadingDegrees?.let { courseHeadingDegrees ->
                courseMedianSmoother.add(courseHeadingDegrees)
                followTargetBearingDegrees = courseMedianSmoother.median()
            }
            if (followSimulatedPosition) {
                centerLatitudeDegrees = snappedCoordinate.latitude
                centerLongitudeDegrees = snappedCoordinate.longitude
                zoomLevel = followZoomTowardTripEnd(
                    remainingDistanceFractionOf(activeRoute?.distanceMeters ?: 0.0),
                )
                pitchDegrees = cameraPitchTargetDegrees()
                followTargetBearingDegrees?.let { targetBearingDegrees ->
                    bearingDegrees = targetBearingDegrees
                }
                applyDesiredCamera()
            }
        }
        // The guided wedge rides the *playing* trip (a trailing fix behind the
        // playback's end arrives during the arrived presentation, where §7.1
        // shows the tracking dot, not the guided wedge).
        pushLocationIndicator(guided = traceDriver.isTripActive, fix = processedFix)
        // The progress line advances with every fix (the drawer's ETA row).
        emitTraceUpdateFromListener()
    }

    /** The remaining-distance fraction of the active trip's route (the zoom ramp's input). */
    private fun remainingDistanceFractionOf(totalRouteDistanceMeters: Double): Double {
        val remainingMeters = traceDriver.latestTripState?.progress
            ?.distanceRemainingMeters ?: totalRouteDistanceMeters
        return if (totalRouteDistanceMeters > 0.0) {
            remainingMeters / totalRouteDistanceMeters
        } else {
            1.0
        }
    }

    /**
     * The follow zoom toward trip end (§5.3): the fixed deep zoom at the
     * trip's start, ramping linearly with the *travelled* fraction to the
     * maximum at arrival (the Compose application's documented ramp).
     */
    private fun followZoomTowardTripEnd(remainingDistanceFraction: Double): Double {
        val clampedRemainingFraction = remainingDistanceFraction.coerceIn(0.0, 1.0)
        val travelledFraction = 1.0 - clampedRemainingFraction
        return RefappConstants.NAVIGATION_FOLLOW_ZOOM_LEVEL +
            (RefappConstants.NAVIGATION_FOLLOW_ZOOM_MAXIMUM_LEVEL -
                RefappConstants.NAVIGATION_FOLLOW_ZOOM_LEVEL) *
            travelledFraction
    }

    /** Restores the captured pre-trip camera (§5.3: restored after). */
    private fun restorePreTripCamera() {
        val restorableCamera = preTripCamera ?: return
        centerLatitudeDegrees = restorableCamera.centerLatitudeDegrees
        centerLongitudeDegrees = restorableCamera.centerLongitudeDegrees
        zoomLevel = restorableCamera.zoomLevel
        bearingDegrees = restorableCamera.bearingDegrees
        pitchDegrees = restorableCamera.pitchDegrees
        followTargetBearingDegrees = null
        courseMedianSmoother.reset()
        isThreeDCameraEnabled = false
        preTripCamera = null
        // The package view snaps to the restored pose (its own animation was
        // never started for the trip's pitch or compass swings).
        applyDesiredCamera()
    }

    // ------------------------------------------------------------------
    // Simulated turn-by-turn navigation (§7)
    // ------------------------------------------------------------------

    // ------------------------------------------------------------------
    // The demo-plan picker's traces (§4.1; the Compose application's demo plans)
    // ------------------------------------------------------------------

    /** The loaded plan traces by their application-asset name (one load per plan). */
    private val loadedPlanTraces: MutableMap<String, RefappTrace> = mutableMapOf()

    /**
     * The plan picker's selected recorded walk (the Compose application's
     * picker default is the artifact's own trace; the picker stays on the
     * artifact framing until the first pick).
     */
    private var selectedPlanTrace: RefappTrace? = null

    /** The default plan's asset name: the artifact's trace, or the built-in demo trace. */
    private val defaultTraceAssetName: String
        get() = scenesArtifact?.traceAssetName ?: TRACE_ASSET_NAME

    /** Loads (or takes from the cache) one plan trace by its application-asset name. */
    private fun loadedTraceFor(
        assetManager: android.content.res.AssetManager,
        traceAssetName: String,
    ): RefappTrace =
        loadedPlanTraces.getOrPut(traceAssetName) { RefappTraceLoader.load(assetManager, traceAssetName) }

    /**
     * Loads one picker plan and applies it (§4.1; the Compose application's
     * `selectDemoPlan`): the selected plan becomes the trip source "Start
     * simulated navigation" drives, a search-planned route is dropped, the
     * route preview frames the plan's own route, and the overlay set repushes
     * with the plan's route line. A pick while the simulated trip runs is
     * refused (the drawer owns the dock during a trip) — never a silent start
     * of a half-selected plan.
     */
    fun selectTraceReplayPlan(
        assetManager: android.content.res.AssetManager,
        traceAssetName: String,
        promise: Promise,
    ) {
        if (isTraceReplayActive) {
            promise.reject("tripRunning", RefappConstants.PLAN_SELECT_TRIP_RUNNING_TEXT)
            return
        }
        val trace = try {
            loadedTraceFor(assetManager, traceAssetName)
        } catch (traceLoadException: Exception) {
            if (traceLoadException is CancellationException ||
                traceLoadException is InterruptedException
            ) {
                throw traceLoadException
            }
            // Exception, not SdkException alone: the asset decode failures are
            // JsonSyntaxException and FileNotFoundException shapes, and a
            // refused pick reports verbatim (§6.3), never a guessed plan.
            reportFailureToReact(traceLoadException)
            promise.reject(
                "traceUnavailable",
                RefappConstants.PLAN_SELECT_TRACE_UNAVAILABLE_TEXT + ": " +
                    failureDetailTextOf(traceLoadException),
            )
            return
        }
        selectedPlanTrace = trace
        plannedRouteTrace = null
        plannedRouteNameText = null
        plannedRouteIsRecordedDemoRoute = false
        plannedRouteIsOfflineArchiveRoute = false
        applyRoutePreviewFraming(trace.route)
        pushOverlaysToSurface()
        clearErrorOnReact()
        emitPlanState()
        emitTraceState()
        promise.resolve(true)
    }

    /**
     * The trip source "Start simulated navigation" drives: the search-planned
     * route when one is planned, otherwise the picker's selected plan,
     * otherwise the recorded artifact trace (loaded from the application asset
     * on first use — the Compose app's artifact-trace plan).
     */
    private fun activeRouteTrace(
        assetManager: android.content.res.AssetManager,
    ): RefappTrace {
        plannedRouteTrace?.let { plannedTrace -> return plannedTrace }
        selectedPlanTrace?.let { selectedTrace -> return selectedTrace }
        return loadedTraceFor(assetManager, defaultTraceAssetName)
    }

    /**
     * The loaded trace currently on the demo (the progress line's and the
     * trace state's honest fallback): the planned route, the selected plan, or
     * the loaded artifact trace (null before the first start).
     */
    private val activeLoadedTrace: RefappTrace?
        get() = plannedRouteTrace
            ?: selectedPlanTrace
            ?: loadedPlanTraces[defaultTraceAssetName]

    /**
     * Starts the simulated trip on the active plan (the route summary's
     * primary act): the follow camera engages at trip start (§5.3) — the deep
     * follow zoom applies immediately, the 3D pitch applies at start, and the
     * bearing eases toward the median course once fixes report it. A start
     * failure leaves the home sheet with the verbatim report, never a
     * half-started navigation mode.
     */
    fun startTraceReplay(assetManager: android.content.res.AssetManager) {
        // One trip at a time: a device-driven trip ends before the replay
        // starts (the location source itself belongs to the React side and is
        // stopped by the shell).
        stopDeviceNavigation()
        val trace = try {
            activeRouteTrace(assetManager)
        } catch (traceLoadException: Exception) {
            reportFailureToReact(traceLoadException)
            return
        }
        // The pre-trip camera is captured at trip start and restored after.
        preTripCamera = PreTripCamera(
            centerLatitudeDegrees = centerLatitudeDegrees,
            centerLongitudeDegrees = centerLongitudeDegrees,
            zoomLevel = zoomLevel,
            bearingDegrees = bearingDegrees,
            pitchDegrees = pitchDegrees,
        )
        isTraceReplayActive = true
        // The active trace's route and endpoint pins draw beneath the trip
        // even before a plan chip was tapped: the start action itself makes
        // the route the trip follows visible (the same route preview the
        // other shells show).
        pushOverlaysToSurface()
        followSimulatedPosition = true
        followTargetBearingDegrees = null
        courseMedianSmoother.reset()
        zoomLevel = RefappConstants.NAVIGATION_FOLLOW_ZOOM_LEVEL
        pitchDegrees = RefappConstants.NAVIGATION_FOLLOW_PITCH_3D_DEGREES
        isThreeDCameraEnabled = true
        applyDesiredCamera()
        traceDriver.startTrip(trace)
        if (traceDriver.startFailure != null) {
            reportFailureToReact(traceDriver.startFailure!!)
        } else {
            clearErrorOnReact()
        }
        emitTraceUpdateFromListener()
        emitTraceState()
        emitPlaybackState()
    }

    /**
     * Whether guidance currently follows the device source's own fixes on the
     * planned route (the Location group's second control).
     */
    var isDeviceNavigationActive: Boolean = false
        private set

    /**
     * Starts guidance on the planned route from this device's fixes. The
     * location source itself belongs to the React side, which starts it
     * through the same permission and source path the location control uses;
     * this method starts the trip and reuses the session's trip presentation,
     * so the banner, the ETA line, the location indicator and the follow
     * camera all read the controller's own stream. Without a planned route it
     * answers `false` and starts nothing.
     */
    fun startDeviceNavigation(): Boolean {
        if (isDeviceNavigationActive) {
            return true
        }
        val planned = plannedRouteTrace ?: return false
        // One trip at a time: the simulated presentation and its camera end
        // before the device-driven one engages.
        if (isTraceReplayActive) {
            exitTraceReplay()
        }
        // The pre-trip camera is captured at trip start and restored after.
        preTripCamera = PreTripCamera(
            centerLatitudeDegrees = centerLatitudeDegrees,
            centerLongitudeDegrees = centerLongitudeDegrees,
            zoomLevel = zoomLevel,
            bearingDegrees = bearingDegrees,
            pitchDegrees = pitchDegrees,
        )
        traceDriver.startDeviceTrip(planned.route)
        val startFailure = traceDriver.startFailure
        if (startFailure != null) {
            reportFailureToReact(startFailure)
            restorePreTripCamera()
            emitTraceState()
            emitPlaybackState()
            emitDeviceNavigationState()
            return false
        }
        isTraceReplayActive = true
        isDeviceNavigationActive = true
        // The planned route's line and endpoint pins draw beneath the trip.
        pushOverlaysToSurface()
        followSimulatedPosition = true
        followTargetBearingDegrees = null
        courseMedianSmoother.reset()
        zoomLevel = RefappConstants.NAVIGATION_FOLLOW_ZOOM_LEVEL
        // The follow camera's 3D course-follow pitch is the plane toggle's: a
        // device-driven trip does not turn the toggle on by itself.
        pitchDegrees = cameraPitchTargetDegrees()
        applyDesiredCamera()
        clearErrorOnReact()
        emitTraceUpdateFromListener()
        emitTraceState()
        emitPlaybackState()
        emitDeviceNavigationState()
        return true
    }

    /**
     * Ends device-driven guidance. The location source belongs to the React
     * side, which stops it only when this mode started it.
     */
    fun stopDeviceNavigation() {
        if (!isDeviceNavigationActive) {
            return
        }
        isDeviceNavigationActive = false
        traceDriver.stopTrip()
        speechSpeaker?.releaseJourneySpeech()
        isTraceReplayActive = false
        followSimulatedPosition = false
        restorePreTripCamera()
        clearLocationIndicatorOnSurface()
        markCameraRevision()
        emitTraceState()
        emitPlaybackState()
        emitDeviceNavigationState()
    }

    /**
     * One accepted device fix of a device-driven trip, delivered through the
     * driver's same fix path as a simulated fix. A refused fix ends the mode
     * honestly rather than leaving a stale trip on screen (§6.3).
     */
    fun deliverDeviceNavigationFix(
        latitudeDegrees: Double,
        longitudeDegrees: Double,
        courseHeadingDegrees: Double?,
        horizontalAccuracyMeters: Double?,
    ) {
        if (!isDeviceNavigationActive) {
            return
        }
        traceDriver.deliverDeviceFix(
            coordinate = GeographicCoordinate(
                latitude = latitudeDegrees,
                longitude = longitudeDegrees,
            ),
            courseHeadingDegrees = courseHeadingDegrees,
            horizontalAccuracyMeters = horizontalAccuracyMeters
                ?: RefappConstants.DEVICE_NAVIGATION_FALLBACK_ACCURACY_METERS,
        )
        traceDriver.tripDeliveryFailure?.let { deliveryFailure ->
            stopDeviceNavigation()
            reportFailureToReact(deliveryFailure)
        }
    }

    private fun emitDeviceNavigationState() {
        val arguments = android.os.Bundle()
        arguments.putBoolean("isActive", isDeviceNavigationActive)
        emitEvent("RefappDeviceNavigationState", arguments)
    }

    /** The drawer's pause act: the playback holds its current fix (§7.2). */
    fun pauseTraceReplay() {
        traceDriver.pauseTrip()
        emitPlaybackState()
    }

    /** The drawer's resume act: the playback continues from its current fix. */
    fun resumeTraceReplay() {
        traceDriver.resumeTrip()
        emitPlaybackState()
    }

    /**
     * The drawer's seek act: moves the presented trip to a travelled
     * distance along the route through the walker, so the camera, the
     * indicator, the ETA line and the arrival follow as on a normal fix.
     */
    fun seekTraceReplay(travelledMeters: Double) {
        traceDriver.seekToTravelledMeters(travelledMeters)
        emitTraceUpdateFromListener()
        emitPlaybackState()
    }

    /**
     * The drawer's speed act: a route-following trip rebuilds its walker at
     * the new pace and re-anchors the clock at the travelled distance, so the
     * position never jumps; a recorded trace's replay cadence follows the
     * choice. The resulting state is published through the playback event.
     */
    fun selectTraceReplaySpeedMultiplier(speedMultiplier: Double) {
        traceDriver.selectSpeedMultiplier(speedMultiplier)
        emitPlaybackState()
    }

    /**
     * The drawer's exit control: stop the replay, restore the captured
     * pre-trip camera (§5.3: restored after), clear the location indicator
     * (the demo's simulated device stops reporting); the planned route and
     * its summary card stay for the sheet's return to half.
     */
    fun exitTraceReplay() {
        // The drawer's exit control ends whichever trip is presented; a
        // device-driven one stops through its own path (the shell stops the
        // location source only when this mode started it).
        if (isDeviceNavigationActive) {
            stopDeviceNavigation()
            return
        }
        traceDriver.stopTrip()
        // The trip's speech lifecycle ends: the audio focus equivalent
        // abandons once the engine drained (an in-flight utterance keeps it
        // until its completion).
        speechSpeaker?.releaseJourneySpeech()
        isTraceReplayActive = false
        followSimulatedPosition = false
        restorePreTripCamera()
        clearLocationIndicatorOnSurface()
        markCameraRevision()
        emitTraceState()
        emitPlaybackState()
    }

    /**
     * The simulated replay's playback end without a user exit.
     *
     * An arrival keeps the destination presentation: the trip stays presented
     * with the destination centred and the indicator on the tracking dot, and
     * only the drawer's exit control empties it (§7.1/§7.2: the arrival state
     * replaces the drawer's contents and is not a snap back). Any other end (a
     * recorded trace whose samples drained behind a reroute) restores the
     * captured pre-trip camera as before.
     */
    private fun onSimulatedPlaybackEnded() {
        if (!isTraceReplayActive) {
            return
        }
        // The trip's speech lifecycle ended with the playback: the audio focus
        // equivalent abandons once the engine drained (an in-flight utterance
        // keeps it until its completion).
        speechSpeaker?.releaseJourneySpeech()
        val arrivedFix = latestLocationFix
        if (traceDriver.hasArrived) {
            // The arrival state stays visible: the destination remains the
            // camera centre (the trip does not snap back to the pre-trip
            // framing) and the indicator turns into the standing tracking dot.
            arrivedFix?.snappedCoordinate?.let { arrivedCoordinate ->
                centerLatitudeDegrees = arrivedCoordinate.latitude
                centerLongitudeDegrees = arrivedCoordinate.longitude
                applyDesiredCamera()
            }
            if (arrivedFix != null) {
                pushLocationIndicator(guided = false, fix = arrivedFix)
            }
            emitPlaybackState()
            return
        }
        restorePreTripCamera()
        if (arrivedFix != null) {
            pushLocationIndicator(guided = false, fix = arrivedFix)
        }
        emitTraceUpdateFromListener()
        emitPlaybackState()
    }

    /** Whether the trace replay is currently driven (the drawer's state). */
    var isTraceReplayActive: Boolean = false
        private set

    private fun emitTraceState() {
        val arguments = android.os.Bundle()
        arguments.putBoolean("isActive", isTraceReplayActive)
        arguments.putString(
            "traceName",
            activeLoadedTrace?.traceName ?: "",
        )
        emitEvent("RefappTraceState", arguments)
    }

    /**
     * The drawer's playback state (§7.2): whether a presented trip is paused,
     * its selected speed multiplier and whether it has arrived. Kept apart
     * from the trace state so a pause or a pace change never drops the last
     * guidance update (the trace state listener clears it).
     */
    private fun emitPlaybackState() {
        val arguments = android.os.Bundle()
        arguments.putBoolean("isActive", isTraceReplayActive)
        arguments.putBoolean("isPaused", traceDriver.isTripPaused)
        arguments.putDouble("speedMultiplier", traceDriver.speedMultiplier)
        arguments.putBoolean("hasArrived", traceDriver.hasArrived)
        arguments.putBoolean("isSeekable", traceDriver.isSeekable)
        arguments.putDouble("totalMeters", traceDriver.seekableDistanceMeters)
        emitEvent("RefappPlaybackState", arguments)
    }

    /** Formats the distance phrase of the active maneuver (§7.1 live prefix). */
    private fun formatDistancePhrase(phrase: uniffi.rustic_maps.DistancePhrase): String =
        when (phrase.phraseKind) {
            DistancePhraseKind.IMMEDIATE -> RefappConstants.DISTANCE_PHRASE_IMMEDIATE_TEXT
            DistancePhraseKind.QUANTITY -> "%.0f %s".format(
                phrase.magnitude ?: 0.0,
                distanceUnitSymbol(phrase.magnitudeUnit ?: DistanceUnit.METERS),
            )
            else -> RefappConstants.DISTANCE_PHRASE_IMMEDIATE_TEXT
        }

    private fun distanceUnitSymbol(distanceUnit: DistanceUnit): String = when (distanceUnit) {
        DistanceUnit.METERS -> "m"
        DistanceUnit.KILOMETERS -> "km"
        DistanceUnit.FEET -> "ft"
        DistanceUnit.MILES -> "mi"
        else -> "m"
    }

    private fun phaseLabelOf(phase: NavigationPhase?): String = when (phase) {
        NavigationPhase.ARRIVED -> RefappConstants.NAVIGATION_PHASE_LABEL_ARRIVED
        NavigationPhase.OFF_ROUTE -> RefappConstants.NAVIGATION_PHASE_LABEL_OFF_ROUTE
        NavigationPhase.NAVIGATING -> RefappConstants.NAVIGATION_PHASE_LABEL_NAVIGATING
        null -> ""
    }

    private fun emitTraceUpdateFromInstruction(instruction: NavigationInstruction) {
        val arguments = android.os.Bundle()
        arguments.putString("phase", phaseLabelOf(navigationListener.latestPhase()))
        arguments.putBoolean("isArrival", instruction.isArrival)
        arguments.putBoolean("isReroutingRequired", instruction.isReroutingRequired)
        val activeManeuver = instruction.activeManeuver
        arguments.putString(
            "bannerHeadlineText",
            formatDistancePhrase(activeManeuver.distancePhrase),
        )
        arguments.putString(
            "bannerDisplayText",
            instructionBannerText(
                composedDisplayText = activeManeuver.instruction.composedDisplayText,
                providerDisplayText = activeManeuver.instruction.providerDisplayText,
                roundaboutExitIndex = activeManeuver.roundaboutExitIndex,
            ),
        )
        arguments.putString(
            "upcomingDisplayText",
            instruction.upcomingManeuver?.let { upcoming ->
                RefappConstants.THEN_LINE_PREFIX + instructionBannerText(
                    composedDisplayText = upcoming.instruction.composedDisplayText,
                    providerDisplayText = upcoming.instruction.providerDisplayText,
                    roundaboutExitIndex = upcoming.roundaboutExitIndex,
                )
            },
        )
        arguments.putString(
            "progressLineText",
            traceDriver.latestTripState?.progress?.let { progress ->
                "Distance remaining ${formatDistanceMeters(progress.distanceRemainingMeters)} · " +
                    "Duration remaining ${formatDurationSeconds(progress.durationRemainingSeconds)}"
            } ?: activeLoadedTrace?.let { trace ->
                "Distance remaining ${formatDistanceMeters(trace.route.distanceMeters)} · " +
                    "Duration remaining ${formatDurationSeconds(trace.route.durationSeconds)}"
            },
        )
        arguments.putDouble("travelledMeters", traceDriver.travelledDistanceMeters)
        emitEvent("RefappTraceUpdate", arguments)
    }

    private fun RefappCollectingListener.latestPhase(): NavigationPhase? =
        latestPhaseChange.let { phaseChange ->
            when (phaseChange) {
                NavigationPhaseChange.ARRIVED -> NavigationPhase.ARRIVED
                NavigationPhaseChange.OFF_ROUTE_ENTERED -> NavigationPhase.OFF_ROUTE
                NavigationPhaseChange.OFF_ROUTE_CLEARED -> NavigationPhase.NAVIGATING
                else -> traceDriver.latestTripState?.phase
            }
        }

    private fun emitTraceUpdateFromListener() {
        val instruction = navigationListener.latestInstruction ?: return
        emitTraceUpdateFromInstruction(instruction)
    }

    private fun emitDeviationUpdate(deviationStatusChange: DeviationStatusChange) {
        if (deviationStatusChange.statusChangeKind != DeviationStatusChangeKind.OFF_ROUTE) {
            return
        }
        val deviationDistanceMeters = deviationStatusChange.deviationInfo?.deviationFromRouteMeters ?: 0.0
        val arguments = android.os.Bundle()
        arguments.putString(
            "text",
            RefappConstants.DEVIATION_TEXT_TEMPLATE.format(
                formatDistanceMeters(deviationDistanceMeters),
            ),
        )
        emitEvent("RefappDeviationAlert", arguments)
    }

    private fun emitPhaseUpdate(phaseChange: NavigationPhaseChange) {
        // The phase chip is the state form (§7.2): recomputed from the
        // controller's read-only state after every phase-topic update.
        val phase = when (phaseChange) {
            NavigationPhaseChange.ARRIVED -> NavigationPhase.ARRIVED
            NavigationPhaseChange.OFF_ROUTE_ENTERED -> NavigationPhase.OFF_ROUTE
            NavigationPhaseChange.OFF_ROUTE_CLEARED -> NavigationPhase.NAVIGATING
            NavigationPhaseChange.REROUTE_APPLIED -> traceDriver.latestTripState?.phase
            else -> traceDriver.latestTripState?.phase
        }
        val arguments = android.os.Bundle()
        arguments.putString("phase", phaseLabelOf(phase))
        emitEvent("RefappPhaseChange", arguments)
    }

    // ------------------------------------------------------------------
    // Diagnostics (§4.1: the readiness chip's sheet; §6.4 honesty)
    // ------------------------------------------------------------------

    /** The one live diagnostics listener, present only while the pane is open. */
    private var diagnosticsListener: SdkDiagnosticEventListener? = null

    /**
     * Registers the diagnostics listener at the given minimum severity and
     * flushes the bounded backlog. Called only when the diagnostics pane
     * opens (and again when the explicit debug switch lowers the gate); no
     * listener exists before then.
     */
    fun startDiagnosticEvents(
        minimumSeverity: SdkLogSeverity,
        maximumBacklogCount: Int,
    ) {
        val sdk = nativeSdk ?: return
        stopDiagnosticEvents()
        val listener = object : SdkDiagnosticEventListener {
            override fun onDiagnosticEvent(event: LogEvent) {
                // The SDK delivers events from its own threads; the React
                // event channel is touched on the main looper only.
                com.facebook.react.bridge.UiThreadUtil.runOnUiThread {
                    val arguments = android.os.Bundle()
                    arguments.putString(
                        "timestampMillisecondsSinceEpoch",
                        event.timestampMillisecondsSinceEpoch.toString(),
                    )
                    arguments.putString("severity", event.severity.name.lowercase())
                    arguments.putString("subsystemTarget", event.subsystemTarget)
                    arguments.putString("messageLine", event.messageLine)
                    // The typed provider request trace crosses as a nested
                    // payload; the record is already redacted at emission, so
                    // no credential travels with it.
                    event.providerRequestTrace?.let { trace ->
                        val traceArguments = android.os.Bundle()
                        traceArguments.putString("providerKey", trace.providerKey)
                        traceArguments.putString(
                            "providerContract",
                            trace.providerContract,
                        )
                        traceArguments.putString("httpMethod", trace.httpMethod)
                        traceArguments.putString("requestPath", trace.requestPath)
                        traceArguments.putString("statusClass", trace.statusClass)
                        traceArguments.putString("outcome", trace.outcome)
                        traceArguments.putString(
                            "durationMilliseconds",
                            trace.durationMilliseconds.toString(),
                        )
                        traceArguments.putString(
                            "responseBytes",
                            trace.responseBytes.toString(),
                        )
                        trace.responseExcerpt?.let { responseExcerpt ->
                            traceArguments.putString(
                                "responseExcerpt",
                                responseExcerpt,
                            )
                        }
                        arguments.putBundle("providerRequestTrace", traceArguments)
                    }
                    emitEvent("RefappDiagnosticEvent", arguments)
                }
            }
        }
        diagnosticsListener = listener
        sdk.subscribeToDiagnosticEvents(listener, minimumSeverity)
        sdk.flushRecentDiagnosticEvents(
            listener,
            minimumSeverity,
            maximumBacklogCount.coerceAtLeast(0).toUInt(),
        )
    }

    /** Removes the listener; the SDK's already-lowered gate stays for the session. */
    fun stopDiagnosticEvents() {
        diagnosticsListener = null
    }

    /** The diagnostics lines the sheet renders (read-only queries, no state). */
    fun diagnosticsLines(): List<String> {
        val cameraReadback = hostView()?.camera()?.let { camera ->
            "Camera %.5f , %.5f · zoom %.2f · bearing %.0f° · pitch %.0f°".format(
                camera.latitudeDegrees,
                camera.longitudeDegrees,
                camera.zoomLevel,
                camera.bearingDegrees,
                camera.pitchDegrees,
            )
        } ?: "Camera: not built yet"
        val tileLine = latestTileStatistics?.let { statistics ->
            "Tiles: requested ${statistics.requestedTileCount} · drawn " +
                "${statistics.presentTileCount} · fallback ${statistics.fallbackTileCount} · " +
                "deferred ${statistics.deferredTileCount} · missing ${statistics.unavailableTileCount}"
        } ?: "Tile accounting starts with the first frame…"
        val planLine = plannedRouteTrace?.let { plannedTrace ->
            "Plan: ${plannedRouteNameText ?: RefappConstants.PLANNED_ROUTE_FALLBACK_NAME} · " +
                "${formatDistanceMeters(plannedTrace.route.distanceMeters)} · " +
                formatDurationSeconds(plannedTrace.route.durationSeconds)
        } ?: "No planned route (a search assignment plans one)"
        return listOf(
            "Renderer: GPU surface renderer (software graphics stack on this emulator: SwiftShader over ANGLE — an expected emulator fact, not a failure)",
            cameraReadback,
            tileLine,
            "Frames: drawn $framesDrawnCount · deferred $framesDeferredCount · failed $framesFailedCount",
            "Overlays pushed: $pushedOverlayCount",
            "Cache: $tileCacheStateText",
            planLine,
        )
    }
}

/**
 * The median-smoothed course-over-ground (§5.3): the circular median of the
 * named constant window of recent fixes (the Compose application's
 * `CourseMedianSmoother`).
 */
private class CourseMedianSmoother(private val windowFixCount: Int) {
    private val recentCourseDegrees = ArrayDeque<Double>()

    /** Adds one fix's course-over-ground, dropping the window's oldest. */
    fun add(courseDegreeValue: Double) {
        recentCourseDegrees.addLast(courseDegreeValue)
        while (recentCourseDegrees.size > windowFixCount) {
            recentCourseDegrees.removeFirst()
        }
    }

    /** The circular median, or null with no windowed sample. */
    fun median(): Double? {
        if (recentCourseDegrees.isEmpty()) {
            return null
        }
        val windowedList = recentCourseDegrees.toList()
        return windowedList.minByOrNull { candidateDegrees ->
            windowedList.sumOf { otherDegrees ->
                angularDistanceDegrees(candidateDegrees, otherDegrees)
            }
        }
    }

    fun reset() {
        recentCourseDegrees.clear()
    }

    private fun angularDistanceDegrees(firstDegrees: Double, secondDegrees: Double): Double {
        val absoluteDifference = abs(firstDegrees - secondDegrees) % FULL_CIRCLE_DEGREES
        return if (absoluteDifference > FULL_CIRCLE_DEGREES / 2.0) {
            FULL_CIRCLE_DEGREES - absoluteDifference
        } else {
            absoluteDifference
        }
    }

    private companion object {
        /** Structural math constant: the full circle's degree sweep. */
        const val FULL_CIRCLE_DEGREES: Double = 360.0
    }
}