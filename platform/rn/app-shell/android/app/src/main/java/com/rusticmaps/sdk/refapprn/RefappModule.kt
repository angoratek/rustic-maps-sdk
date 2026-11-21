package com.rusticmaps.sdk.refapprn

import android.os.Bundle
import com.facebook.react.bridge.Promise
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.bridge.ReactContextBaseJavaModule
import com.facebook.react.bridge.ReactMethod
import com.facebook.react.modules.core.DeviceEventManagerModule
import com.facebook.react.bridge.UiThreadUtil
import uniffi.rustic_maps.SdkLogSeverity

/**
 * The React Native bridge module of the reference application (`RefappMap` in
 * JavaScript): thin platform glue over the [RefappDemoSession] singleton —
 * the keyless SDK start, the runtime provider credential (the credential is
 * entered at runtime and never baked into the app,
 * docs/Providers.md (Credentials), its opt-in secure
 * store, the camera presets, the simulated trace replay, the attach retry
 * control, and the diagnostics lines. All SDK-driving methods hop to the main
 * looper (`MapView`'s contract) and report nothing silently: a refused
 * call publishes its verbatim failure through the controller's error-report
 * event (§6.3).
 */
class RefappModule(private val reactContext: ReactApplicationContext) :
    ReactContextBaseJavaModule(reactContext) {

    companion object {
        const val MODULE_NAME: String = "RefappMap"

        /** The preferences file the three-way theme choice lives in (§3). */
        private const val THEME_CHOICE_PREFERENCES_NAME: String = "rusticMapsSdk.themeChoice"

        /** The key inside that file; the value is `light`, `dark` or `auto`. */
        private const val THEME_CHOICE_PREFERENCE_KEY: String = "themeChoice"

        /** The preferences file the versioned remembered-choices record lives in. */
        private const val PREFERENCES_NAME: String = "rusticMapsSdk.referenceAppPreferences"

        /** The key inside that file; the value is the record's JSON text. */
        private const val PREFERENCES_KEY: String = "preferences"
    }

    override fun getName(): String = MODULE_NAME

    override fun initialize() {
        super.initialize()
        // The speech engine (the app-owned TextToSpeech voice) builds once at
        // module start so its initialization state is honest before the first
        // trip; the ready/unavailable state travels the event channel.
        RefappDemoSession.instance.initializeSpeechEngine(reactContext)
        RefappDemoSession.instance.initializeProviderCredentialStore(reactContext)
        RefappDemoSession.instance.eventSink = { eventName, arguments ->
            emitToJavaScript(eventName, arguments)
        }
    }

    override fun onCatalystInstanceDestroy() {
        RefappDemoSession.instance.eventSink = null
        super.onCatalystInstanceDestroy()
    }

    /** One event channel to the React side (named events, bundle payloads). */
    private fun emitToJavaScript(eventName: String, arguments: Bundle) {
        UiThreadUtil.runOnUiThread {
            reactContext
                .getJSModule(DeviceEventManagerModule.RCTDeviceEventEmitter::class.java)
                ?.emit(eventName, arguments.toReactMap())
        }
    }

    @ReactMethod
    fun createSdk() {
        UiThreadUtil.runOnUiThread {
            // The package native module owns the one MapSdk; the demo session
            // only loads its shared scene artifact once the SDK is up. Apply
            // the launch-intent automation credential here as well: the
            // package module's lifecycle callback can run before this shell
            // starts the SDK, and the same call is idempotent once it did.
            com.angoratek.rusticmaps.MapsSdkNativeRuntime.ensureStarted(reactContext)
            com.angoratek.rusticmaps.MapsSdkNativeRuntime.applyLaunchInjectedCredential(
                reactContext.currentActivity,
            )
            RefappDemoSession.instance.ensureSdkStarted(
                reactContext.assets,
                reactContext.cacheDir.absolutePath,
            )
        }
    }

    /**
     * Applies one runtime provider credential (§3's explicit Apply); the
     * result carries the verbatim refusal detail when the SDK refused it.
     */
    @ReactMethod
    fun setProviderCredential(providerKey: String, credential: String, promise: Promise) {
        UiThreadUtil.runOnUiThread {
            val (isApplied, detailText) =
                RefappDemoSession.instance.setProviderCredential(providerKey, credential)
            val result = com.facebook.react.bridge.Arguments.createMap()
            result.putBoolean("isApplied", isApplied)
            result.putString("detailText", detailText)
            promise.resolve(result)
        }
    }

    /** The opt-in secure store's value (null when nothing is stored). */
    @ReactMethod
    fun getStoredProviderCredential(providerKey: String, promise: Promise) {
        UiThreadUtil.runOnUiThread {
            promise.resolve(
                RefappDemoSession.instance.getStoredProviderCredential(providerKey),
            )
        }
    }

    /** Persists the credential behind the Android Keystore; resolves false on failure. */
    @ReactMethod
    fun storeProviderCredential(providerKey: String, credential: String, promise: Promise) {
        UiThreadUtil.runOnUiThread {
            promise.resolve(
                RefappDemoSession.instance.storeProviderCredential(providerKey, credential),
            )
        }
    }

    /** Removes the stored credential; resolves false when the store is unavailable. */
    @ReactMethod
    fun clearStoredProviderCredential(providerKey: String, promise: Promise) {
        UiThreadUtil.runOnUiThread {
            promise.resolve(
                RefappDemoSession.instance.clearStoredProviderCredential(providerKey),
            )
        }
    }

    /** The device-remembered theme choice (§3), or null when never chosen. */
    @ReactMethod
    fun loadThemeChoice(promise: Promise) {
        val preferences = reactContext.getSharedPreferences(
            THEME_CHOICE_PREFERENCES_NAME,
            android.content.Context.MODE_PRIVATE,
        )
        promise.resolve(preferences.getString(THEME_CHOICE_PREFERENCE_KEY, null))
    }

    /** Remembers one theme choice on this device (§3). */
    @ReactMethod
    fun storeThemeChoice(themeChoice: String) {
        reactContext.getSharedPreferences(
            THEME_CHOICE_PREFERENCES_NAME,
            android.content.Context.MODE_PRIVATE,
        ).edit()
            .putString(THEME_CHOICE_PREFERENCE_KEY, themeChoice)
            .apply()
    }

    /** The versioned remembered-choices record as JSON, or null when unset. */
    @ReactMethod
    fun loadPreferences(promise: Promise) {
        val preferences = reactContext.getSharedPreferences(
            PREFERENCES_NAME,
            android.content.Context.MODE_PRIVATE,
        )
        promise.resolve(preferences.getString(PREFERENCES_KEY, null))
    }

    /** Remembers the versioned choices record on this device. */
    @ReactMethod
    fun storePreferences(preferencesJson: String) {
        reactContext.getSharedPreferences(
            PREFERENCES_NAME,
            android.content.Context.MODE_PRIVATE,
        ).edit()
            .putString(PREFERENCES_KEY, preferencesJson)
            .apply()
    }

    /**
     * Clears the remembered-choices record and the theme choice an earlier
     * build stored in its own key, so a reset never leaves a choice behind
     * for the next launch.
     */
    @ReactMethod
    fun clearPreferences() {
        reactContext.getSharedPreferences(
            PREFERENCES_NAME,
            android.content.Context.MODE_PRIVATE,
        ).edit()
            .remove(PREFERENCES_KEY)
            .apply()
        reactContext.getSharedPreferences(
            THEME_CHOICE_PREFERENCES_NAME,
            android.content.Context.MODE_PRIVATE,
        ).edit()
            .remove(THEME_CHOICE_PREFERENCE_KEY)
            .apply()
    }

    @ReactMethod
    fun applyCameraPreset(
        latitudeDegrees: Double,
        longitudeDegrees: Double,
        zoomLevel: Double,
        bearingDegrees: Double,
        pitchDegrees: Double,
    ) {
        UiThreadUtil.runOnUiThread {
            RefappDemoSession.instance.applyCameraPreset(
                latitudeDegrees,
                longitudeDegrees,
                zoomLevel,
                bearingDegrees,
                pitchDegrees,
            )
        }
    }

    /** One zoom control step (§4.2's cluster; the React side repeats on hold). */
    @ReactMethod
    fun zoomInControl() {
        UiThreadUtil.runOnUiThread {
            RefappDemoSession.instance.zoomInControl()
        }
    }

    @ReactMethod
    fun zoomOutControl() {
        UiThreadUtil.runOnUiThread {
            RefappDemoSession.instance.zoomOutControl()
        }
    }

    /** The my-location control (§4.2): centers on the latest fix. */
    @ReactMethod
    fun myLocationControl() {
        UiThreadUtil.runOnUiThread {
            RefappDemoSession.instance.centerOnLatestLocation()
        }
    }

    /** The 2D/3D plane toggle (§4.2). */
    @ReactMethod
    fun cameraPlaneControl() {
        UiThreadUtil.runOnUiThread {
            RefappDemoSession.instance.toggleCameraPlane()
        }
    }

    /** The compass tap (§2.4): eases the camera north up. */
    @ReactMethod
    fun compassControl() {
        UiThreadUtil.runOnUiThread {
            RefappDemoSession.instance.easeCameraNorthUp()
        }
    }

    /** The plan search (§4.1): the result rows resolve through the promise. */
    @ReactMethod
    fun searchPlaces(queryText: String, promise: Promise) {
        UiThreadUtil.runOnUiThread {
            RefappDemoSession.instance.searchPlaces(queryText, promise)
        }
    }

    /** One result row's plan-field assignment; both resolved ends plan the route. */
    @ReactMethod
    fun assignSearchResult(
        assignToOrigin: Boolean,
        placeNameText: String,
        latitudeDegrees: Double,
        longitudeDegrees: Double,
        promise: Promise,
    ) {
        UiThreadUtil.runOnUiThread {
            RefappDemoSession.instance.assignSearchResult(
                assignToOrigin,
                placeNameText,
                latitudeDegrees,
                longitudeDegrees,
                promise,
            )
        }
    }

    /** The explicit Plan route action (§4.1): plans between the two assigned
     * plan fields with the Directions choice; a missing field rejects with a
     * reason naming it. */
    @ReactMethod
    fun planRoute(transportationMode: Int, routeStrategy: Int, promise: Promise) {
        UiThreadUtil.runOnUiThread {
            RefappDemoSession.instance.planRoute(
                transportationMode,
                routeStrategy,
                promise,
            )
        }
    }

    /** The keyless plan (§4.1): applies the artifact's recorded route for the
     * ordered demo-place pair; no provider call is involved. */
    @ReactMethod
    fun planRecordedDemoRoute(originName: String, destinationName: String, promise: Promise) {
        UiThreadUtil.runOnUiThread {
            RefappDemoSession.instance.planRecordedDemoRoute(
                originName,
                destinationName,
                promise,
            )
        }
    }

    /** The offline card's route control (§2.5 `offline.plan_route_action`):
     * plans over the bundled committed test graph; no provider or network. */
    @ReactMethod
    fun planOfflineRouteFromArchive(promise: Promise) {
        UiThreadUtil.runOnUiThread {
            RefappDemoSession.instance.planOfflineRouteFromArchive(promise)
        }
    }

    /** Whether the running SDK session has a provider credential applied. */
    @ReactMethod
    fun isProviderCredentialApplied(promise: Promise) {
        promise.resolve(RefappDemoSession.instance.isProviderCredentialApplied())
    }

    /** One demo-scene chip's selection over the SDK overlay API (§4.1). */
    @ReactMethod
    fun setSceneOverlayToggle(sceneKey: String, isSelected: Boolean) {
        UiThreadUtil.runOnUiThread {
            RefappDemoSession.instance.setSceneOverlayToggle(sceneKey, isSelected)
        }
    }

    /** The scene chips' Clear control. */
    @ReactMethod
    fun clearSceneOverlays() {
        UiThreadUtil.runOnUiThread {
            RefappDemoSession.instance.clearSceneOverlays()
        }
    }

    @ReactMethod
    fun startTraceReplay() {
        UiThreadUtil.runOnUiThread {
            RefappDemoSession.instance.startTraceReplay(reactContext.assets)
        }
    }

    /**
     * Starts guidance on the planned route from the device's own fixes; the
     * resolved value says whether a planned route let it start (the React side
     * states the plan-first sentence when it did not).
     */
    @ReactMethod
    fun startDeviceNavigation(promise: Promise) {
        UiThreadUtil.runOnUiThread {
            promise.resolve(RefappDemoSession.instance.startDeviceNavigation())
        }
    }

    /** Ends device-driven guidance (the React side stops the source it owns). */
    @ReactMethod
    fun stopDeviceNavigation() {
        UiThreadUtil.runOnUiThread {
            RefappDemoSession.instance.stopDeviceNavigation()
        }
    }

    /**
     * One accepted device fix into the device-driven trip's navigator. A null
     * course or accuracy means the source reported none (the session supplies
     * the reducer's fallback accuracy).
     */
    @ReactMethod
    fun deliverDeviceNavigationFix(
        latitudeDegrees: Double,
        longitudeDegrees: Double,
        courseHeadingDegrees: Double?,
        horizontalAccuracyMeters: Double?,
    ) {
        UiThreadUtil.runOnUiThread {
            RefappDemoSession.instance.deliverDeviceNavigationFix(
                latitudeDegrees,
                longitudeDegrees,
                courseHeadingDegrees,
                horizontalAccuracyMeters,
            )
        }
    }

    /** One demo-plan pick: the recorded walk the Start control replays (§4.1). */
    @ReactMethod
    fun selectTraceReplayPlan(traceAssetName: String, promise: Promise) {
        UiThreadUtil.runOnUiThread {
            RefappDemoSession.instance.selectTraceReplayPlan(
                reactContext.assets,
                traceAssetName,
                promise,
            )
        }
    }

    @ReactMethod
    fun exitTraceReplay() {
        UiThreadUtil.runOnUiThread {
            RefappDemoSession.instance.exitTraceReplay()
        }
    }

    /** The drawer's pause act (§7.2): the playback holds its current fix. */
    @ReactMethod
    fun pauseTraceReplay() {
        UiThreadUtil.runOnUiThread {
            RefappDemoSession.instance.pauseTraceReplay()
        }
    }

    /** The drawer's resume act: the playback continues from its current fix. */
    @ReactMethod
    fun resumeTraceReplay() {
        UiThreadUtil.runOnUiThread {
            RefappDemoSession.instance.resumeTraceReplay()
        }
    }

    /**
     * The drawer's seek act: moves the trip to a travelled distance
     * along the route through the walker.
     */
    @ReactMethod
    fun seekTraceReplay(travelledMeters: Double) {
        UiThreadUtil.runOnUiThread {
            RefappDemoSession.instance.seekTraceReplay(travelledMeters)
        }
    }

    /**
     * The drawer's speed act: one of the offered multipliers. A
     * route-following trip rebuilds its walker at the new pace without moving
     * the travelled position.
     */
    @ReactMethod
    fun selectTraceReplaySpeedMultiplier(speedMultiplier: Double) {
        UiThreadUtil.runOnUiThread {
            RefappDemoSession.instance.selectTraceReplaySpeedMultiplier(speedMultiplier)
        }
    }

    /** One verbatim guidance line into the speech engine (the consumer's speak hop). */
    @ReactMethod
    fun speakUtterance(speechText: String, engineUtteranceId: String) {
        UiThreadUtil.runOnUiThread {
            RefappDemoSession.instance.speakGuidanceUtterance(speechText, engineUtteranceId)
        }
    }

    /** The speech consumer's toggle-off path: whatever speaks, stops. */
    @ReactMethod
    fun stopSpeech() {
        UiThreadUtil.runOnUiThread {
            RefappDemoSession.instance.stopGuidanceSpeech()
        }
    }

    /** The speech engine's initialization state, read back at the React mount (§6.2). */
    @ReactMethod
    fun getSpeechEngineState(promise: Promise) {
        UiThreadUtil.runOnUiThread {
            val (isEngineReady, unavailableReasonText) =
                RefappDemoSession.instance.speechEngineState()
            val state = com.facebook.react.bridge.Arguments.createMap()
            state.putBoolean("isEngineReady", isEngineReady)
            if (unavailableReasonText != null) {
                state.putString("unavailableReasonText", unavailableReasonText)
            }
            promise.resolve(state)
        }
    }

    @ReactMethod
    fun retrySurfaceAttach() {
        UiThreadUtil.runOnUiThread {
            RefappDemoSession.instance.retrySurfaceAttach()
        }
    }

    @ReactMethod
    fun getDiagnosticsLines(promise: Promise) {
        UiThreadUtil.runOnUiThread {
            // The bridge converts native arrays only: the string list becomes a
            // WritableArray (a raw Kotlin `List` is not a convertible argument
            // shape — it crashed the React conversion on device).
            val lines = com.facebook.react.bridge.Arguments.createArray()
            for (line in com.angoratek.rusticmaps.MapsSdkNativeRuntime.diagnosticsLines()) {
                lines.pushString(line)
            }
            for (line in RefappDemoSession.instance.diagnosticsLines()) {
                lines.pushString(line)
            }
            promise.resolve(lines)
        }
    }

    /**
     * Starts the opt-in diagnostics stream at the given severity ordinal
     * (`SdkLogSeverity`: Trace=0, Debug=1, Info=2, Warn=3, Error=4) and
     * flushes the bounded backlog. Called only when the pane opens.
     */
    @ReactMethod
    fun startDiagnosticEvents(
        minimumSeverityOrdinal: Double,
        maximumBacklogCount: Double,
    ) {
        UiThreadUtil.runOnUiThread {
            val minimumSeverity = when (minimumSeverityOrdinal.toInt()) {
                0 -> SdkLogSeverity.TRACE
                1 -> SdkLogSeverity.DEBUG
                3 -> SdkLogSeverity.WARN
                4 -> SdkLogSeverity.ERROR
                else -> SdkLogSeverity.INFO
            }
            RefappDemoSession.instance.startDiagnosticEvents(
                minimumSeverity,
                maximumBacklogCount.toInt(),
            )
        }
    }

    /** Removes the diagnostics listener when the pane closes. */
    @ReactMethod
    fun stopDiagnosticEvents() {
        UiThreadUtil.runOnUiThread {
            RefappDemoSession.instance.stopDiagnosticEvents()
        }
    }
}

/** The React-side map type of the controller's event payloads. */
private fun Bundle.toReactMap(): com.facebook.react.bridge.WritableMap =
    com.facebook.react.bridge.Arguments.fromBundle(this)