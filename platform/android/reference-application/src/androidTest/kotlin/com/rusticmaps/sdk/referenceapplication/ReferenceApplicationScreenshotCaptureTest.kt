package com.rusticmaps.sdk.referenceapplication

import android.content.Context
import android.graphics.Bitmap
import android.os.SystemClock
import android.view.MotionEvent
import androidx.test.core.app.ActivityScenario
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.rusticmaps.sdk.referenceapplication.ReferenceApplicationState.HomeSheetState
import com.rusticmaps.sdk.referenceapplication.ReferenceApplicationState.ReadinessState
import com.rusticmaps.sdk.referenceapplication.map.MapLayerBaseSelection
import java.io.File
import java.io.FileOutputStream
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assume
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.rustic_maps.ProviderContract

/**
 * Captures the published Android reference-application screenshots from keyed
 * runs: the plan sheet with both endpoints chosen, the provider section with
 * the applied provider serving every capability, the navigation frame at the
 * first instruction with the 1× speed selected, the styled vector base with
 * its place labels, and the layers control closed and open over the
 * credential's raster base.
 *
 * The capture is opt-in so the connected suite stays read-only and fast, and
 * it needs a session credential because the provider section documents the
 * applied state:
 *
 * ```
 * ./gradlew :reference-application:assembleDebugAndroidTest
 * adb install -r reference-application/build/outputs/apk/debug/reference-application-debug.apk
 * adb install -r reference-application/build/outputs/apk/androidTest/debug/reference-application-debug-androidTest.apk
 * adb shell am instrument -w \
 *   -e class 'com.rusticmaps.sdk.referenceapplication.ReferenceApplicationScreenshotCaptureTest#capturesTheKeyedPlanSheetProviderSectionNavigationAndVectorFrames' \
 *   -e captureReferenceScreenshots true \
 *   -e providerApiKey "$PROVIDER_API_KEY" -e providerKey=<provider-key> \
 *   com.rusticmaps.sdk.referenceapplication.test/androidx.test.runner.AndroidJUnitRunner
 * adb pull /sdcard/Android/data/com.rusticmaps.sdk.referenceapplication/files/reference-screenshots
 * ```
 *
 * Run the test through `adb shell am instrument`, not the Gradle connected
 * task: Gradle uninstalls the application after the test and with it the
 * external capture directory, before the host can pull it.
 *
 * The credential reaches the application through the documented launch-intent
 * extra (`providerApiKey`), is applied through the public registration path,
 * and is never persisted or logged. Every state is reached through the same
 * public user-path methods the sheet's controls call, exactly like the
 * behavior suite; nothing is forced.
 */
@RunWith(AndroidJUnit4::class)
class ReferenceApplicationScreenshotCaptureTest {

    /** The application scenario the capture launched, closed afterwards. */
    private var applicationScenario: ActivityScenario<MainActivity>? = null

    @After
    fun closesTheApplication() {
        applicationScenario?.close()
        applicationScenario = null
    }

    @Test
    fun capturesTheKeyedPlanSheetProviderSectionNavigationAndVectorFrames() {
        val captureArguments = InstrumentationRegistry.getArguments()
        Assume.assumeTrue(
            "screenshot capture is opt-in",
            captureArguments.getString(CAPTURE_ARGUMENT) == "true",
        )
        val runtimeCredential = captureArguments
            .getString(ReferenceApplicationConstants.RUNTIME_CREDENTIAL_INTENT_EXTRA)
            ?.trim()
            ?.takeIf { credential -> credential.isNotEmpty() }
        assertTrue(
            "the keyed capture needs a session credential in the " +
                ReferenceApplicationConstants.RUNTIME_CREDENTIAL_INTENT_EXTRA +
                " runner argument",
            runtimeCredential != null,
        )
        val requestedProviderKey = captureArguments
            .getString(ReferenceApplicationConstants.RUNTIME_PROVIDER_KEY_INTENT_EXTRA)
            ?.trim()
            ?.takeIf { providerKey -> providerKey.isNotEmpty() }
        clearRememberedChoices()
        val activity = launchReferenceApplication(runtimeCredential, requestedProviderKey)
        val applicationState = awaitApplicationState(activity)
        awaitOrTimeout(
            timeoutMilliseconds = READY_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the map's first ready frame",
        ) {
            applicationState.readinessState != ReadinessState.LOADING &&
                applicationState.mapView != null
        }
        // The launch-intent extra is applied through the public registration
        // path; the session then reads the provider serving every capability
        // it declares.
        val injectedProviderKey = requestedProviderKey
            ?: applicationState.defaultRuntimeInjectionProviderKey()
            ?: throw AssertionError("the registry declares no injectable provider")
        awaitOrTimeout(
            timeoutMilliseconds = ROUTE_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the injected credential's registration",
        ) {
            applicationState.isCredentialApplied(injectedProviderKey) &&
                applicationState.capabilityProviderState.rows.any { row ->
                    row.contract == ProviderContract.ROUTING &&
                        row.isRegistered &&
                        row.servingProviderKey == injectedProviderKey
                }
        }
        val recordedRoute = applicationState.artifact.demoRoutes.firstOrNull()
            ?: throw AssertionError("the shared artifact carries no recorded demo route")

        // The whole set draws the registry's styled vector base, selected
        // through the same row act the layers panel calls. The key still
        // serves every capability; its registration only decided the raster
        // slot the styled base paints over.
        val vectorChoice = applicationState.keylessVectorBaseChoice
            ?: throw AssertionError("the registry declares no keyless styled base")
        driveOnMainThread { state -> state.selectVectorBase(vectorChoice.choiceKey) }
        awaitOrTimeout(
            timeoutMilliseconds = READY_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the styled base's first drawn tile",
        ) {
            applicationState.vectorBaseStatus?.let { status ->
                status.isConfigured && status.drawnTileCount > 0u
            } == true
        }
        // Prove the frame draws the registry's styled base, not the raster
        // source the credential registered underneath it.
        val vectorStatus = applicationState.vectorBaseStatus
            ?: throw AssertionError("the styled base reported no source status")
        assertTrue(
            "the styled base must serve the registry style " +
                vectorChoice.style.styleId + ", not " + vectorStatus.styleId,
            vectorStatus.styleId == vectorChoice.style.styleId,
        )
        assertTrue(
            "the styled base must serve the registry provider " +
                vectorChoice.descriptor.providerKey + ", not " + vectorStatus.sourceName,
            vectorStatus.sourceName == vectorChoice.descriptor.providerKey,
        )
        assertTrue(
            "the styled base drew no tile in the captured frame",
            vectorStatus.drawnTileCount > 0u,
        )

        driveOnMainThread { state ->
            state.openPlanSearch()
            state.originSearchText = recordedRoute.originName
            state.destinationSearchText = recordedRoute.destinationName
        }
        driveOnMainThread { state -> state.requestHomeSheetState(HomeSheetState.FULL) }
        // The first frame of a fresh session fetches the vector base's tiles;
        // on the emulator's software renderer they arrive tens of seconds in.
        settleFrames(BASE_MAP_SETTLE_MILLISECONDS)
        captureScreenshot(PLAN_SHEET_FILE_NAME)

        // The provider section is captured over the plan surface, before a
        // trip starts, and scrolled past the base-map and credential rows to
        // the capability rows the screenshot documents.
        driveOnMainThread { state -> state.openSettings() }
        settleFrames()
        swipeUpWithinSheet()
        settleFrames()
        captureScreenshot(PROVIDER_SECTION_FILE_NAME)
        driveOnMainThread { state -> state.closeSettings() }

        // The vector base frame: the search view returns home and the sheet
        // rests at peek, so the styled base and its place labels fill the
        // phone-width frame.
        driveOnMainThread { state -> state.dismissPlanSearch() }
        driveOnMainThread { state -> state.requestHomeSheetState(HomeSheetState.PEEK) }
        settleFrames(BASE_MAP_SETTLE_MILLISECONDS)
        captureScreenshot(VECTOR_BASE_FILE_NAME)

        // The navigation frame: the first instruction, with the speed the
        // published set uses (1×).
        driveOnMainThread { state -> state.startSimulatedTrip() }
        driveOnMainThread { state ->
            state.selectSimulatedTripSpeedMultiplier(PUBLISHED_SPEED_MULTIPLIER)
        }
        awaitOrTimeout(
            timeoutMilliseconds = READY_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the trip's first instruction",
        ) {
            applicationState.isNavigationInTrip &&
                applicationState.navigationListener.instructionState.value != null &&
                applicationState.simulatedTripSpeedMultiplier ==
                PUBLISHED_SPEED_MULTIPLIER
        }
        settleFrames()
        captureScreenshot(NAVIGATION_FILE_NAME)

        val outputDirectory = screenshotDirectory()
        val capturedFiles = listOf(
            PLAN_SHEET_FILE_NAME,
            PROVIDER_SECTION_FILE_NAME,
            NAVIGATION_FILE_NAME,
            VECTOR_BASE_FILE_NAME,
        ).map { fileName -> File(outputDirectory, fileName) }
        capturedFiles.forEach { capturedFile ->
            assertTrue(
                "the capture ${capturedFile.name} was never written",
                capturedFile.isFile && capturedFile.length() > 0,
            )
        }
    }

    /**
     * Captures the layers control pair from one keyed session on the
     * credential's raster base: the map with the panel closed, then the same
     * camera pose with the panel open over every section and the clear
     * action.
     */
    @Test
    fun capturesTheKeyedLayersPanelClosedAndOpenFrames() {
        val captureArguments = InstrumentationRegistry.getArguments()
        Assume.assumeTrue(
            "screenshot capture is opt-in",
            captureArguments.getString(CAPTURE_ARGUMENT) == "true",
        )
        val runtimeCredential = captureArguments
            .getString(ReferenceApplicationConstants.RUNTIME_CREDENTIAL_INTENT_EXTRA)
            ?.trim()
            ?.takeIf { credential -> credential.isNotEmpty() }
        assertTrue(
            "the keyed capture needs a session credential in the " +
                ReferenceApplicationConstants.RUNTIME_CREDENTIAL_INTENT_EXTRA +
                " runner argument",
            runtimeCredential != null,
        )
        val requestedProviderKey = captureArguments
            .getString(ReferenceApplicationConstants.RUNTIME_PROVIDER_KEY_INTENT_EXTRA)
            ?.trim()
            ?.takeIf { providerKey -> providerKey.isNotEmpty() }
        clearRememberedChoices()
        val activity = launchReferenceApplication(runtimeCredential, requestedProviderKey)
        val applicationState = awaitApplicationState(activity)
        awaitOrTimeout(
            timeoutMilliseconds = READY_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the map's first ready frame",
        ) {
            applicationState.readinessState != ReadinessState.LOADING &&
                applicationState.mapView != null
        }
        val injectedProviderKey = requestedProviderKey
            ?: applicationState.defaultRuntimeInjectionProviderKey()
            ?: throw AssertionError("the registry declares no injectable provider")
        awaitOrTimeout(
            timeoutMilliseconds = ROUTE_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the injected credential's registration",
        ) {
            applicationState.isCredentialApplied(injectedProviderKey)
        }
        // The key's raster base serves both frames. The keyed launch opens on
        // the registry's keyless styled base, so the raster row's own act
        // switches back to the credential provider's imagery.
        driveOnMainThread { state ->
            state.selectMapLayerBase(MapLayerBaseSelection.Raster(injectedProviderKey))
        }
        awaitOrTimeout(
            timeoutMilliseconds = READY_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the credential provider's raster base",
        ) {
            applicationState.activeVectorBase == null &&
                applicationState.activeBaseMap?.providerKey == injectedProviderKey
        }
        driveOnMainThread { state -> state.dismissPlanSearch() }
        driveOnMainThread { state -> state.requestHomeSheetState(HomeSheetState.PEEK) }
        settleFrames(BASE_MAP_SETTLE_MILLISECONDS)
        assertTrue(
            "the keyed pair must draw the provider's raster base, not a styled one",
            applicationState.activeVectorBase == null &&
                applicationState.activeBaseMap?.providerKey == injectedProviderKey,
        )
        assertTrue(
            "the layers panel must be closed on the first frame",
            !applicationState.mapLayersState.isPanelOpen,
        )
        captureScreenshot(LAYERS_CLOSED_FILE_NAME)

        driveOnMainThread { state -> state.openMapLayersPanel() }
        awaitOrTimeout(
            timeoutMilliseconds = READY_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the layers panel's presentation",
        ) {
            applicationState.mapLayersState.isPanelOpen &&
                applicationState.mapLayersState.hasPresentedPanel
        }
        awaitOrTimeout(
            timeoutMilliseconds = MAP_LAYERS_ROWS_SETTLE_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the serving raster base's panel row",
        ) {
            applicationState.mapLayersState.baseRows.any { row ->
                row.isServing &&
                    row.selection == MapLayerBaseSelection.Raster(injectedProviderKey)
            }
        }
        assertEquals(
            "the open panel must list Base map, Traffic, Overlays and " +
                "Device location in order",
            listOf(
                ReferenceApplicationConstants.BASE_MAP_SECTION_LABEL_TEXT,
                ReferenceApplicationConstants.MAP_LAYERS_TRAFFIC_SECTION_LABEL_TEXT,
                ReferenceApplicationConstants.MAP_LAYERS_OVERLAYS_SECTION_LABEL_TEXT,
                ReferenceApplicationConstants.MAP_LAYERS_DEVICE_LOCATION_SECTION_LABEL_TEXT,
            ),
            applicationState.mapLayersState.sectionTitles,
        )
        // The panel scrolls on a phone: the open frame shows its last base
        // rows with the traffic, overlays, clear and device-location
        // sections below them, so every section and the shared clear action
        // are in one phone-width frame.
        swipeTowardTheLayersPanelEnd()
        settleFrames()
        swipeTowardTheLayersPanelEnd()
        settleFrames()
        // A short reverse nudge lands the first visible base row fully in
        // frame instead of clipped by the panel's top edge.
        nudgeTheLayersPanelBack()
        settleFrames()
        captureScreenshot(LAYERS_OPEN_FILE_NAME)

        val outputDirectory = screenshotDirectory()
        listOf(LAYERS_CLOSED_FILE_NAME, LAYERS_OPEN_FILE_NAME).forEach { fileName ->
            val capturedFile = File(outputDirectory, fileName)
            assertTrue(
                "the capture ${capturedFile.name} was never written",
                capturedFile.isFile && capturedFile.length() > 0,
            )
        }
    }

    /**
     * Launches the application with the automation credential on the launch
     * intent (the documented `providerApiKey` extra), applied through the
     * public registration path and never persisted by the injection.
     */
    private fun launchReferenceApplication(
        runtimeCredential: String?,
        requestedProviderKey: String?,
    ): MainActivity {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val launchIntent = android.content.Intent(context, MainActivity::class.java)
        if (runtimeCredential != null) {
            launchIntent.putExtra(
                ReferenceApplicationConstants.RUNTIME_CREDENTIAL_INTENT_EXTRA,
                runtimeCredential,
            )
        }
        if (requestedProviderKey != null) {
            launchIntent.putExtra(
                ReferenceApplicationConstants.RUNTIME_PROVIDER_KEY_INTENT_EXTRA,
                requestedProviderKey,
            )
        }
        val scenario = ActivityScenario.launch<MainActivity>(launchIntent)
        applicationScenario = scenario
        var launchedActivity: MainActivity? = null
        scenario.onActivity { launchedActivity = it }
        return launchedActivity
            ?: throw AssertionError("the scenario never produced the application activity")
    }

    /** Awaits the application state the activity constructs. */
    private fun awaitApplicationState(activity: MainActivity): ReferenceApplicationState {
        awaitOrTimeout(
            timeoutMilliseconds = STATE_CONSTRUCTION_TIMEOUT_MILLISECONDS,
            timeoutDescription = "the application state construction",
        ) {
            activity.automationApplicationState != null
        }
        return activity.automationApplicationState
            ?: throw AssertionError("no application state constructed")
    }

    /** Drives one user-path act on the main thread, as the screen does. */
    private fun driveOnMainThread(action: (ReferenceApplicationState) -> Unit) {
        val scenario = applicationScenario ?: error("no application scenario started")
        scenario.onActivity { activity ->
            activity.automationApplicationState?.let(action)
        }
    }

    /** Clears both preference stores the app reads at launch. */
    private fun clearRememberedChoices() {
        val context = ApplicationProvider.getApplicationContext<Context>()
        context.getSharedPreferences(
            RememberedChoicesStore.PREFERENCES_NAME,
            Context.MODE_PRIVATE,
        ).edit().clear().commit()
        context.getSharedPreferences(
            ThemeChoiceStore.PREFERENCES_NAME,
            Context.MODE_PRIVATE,
        ).edit().clear().commit()
    }

    /** Lets Compose and the map's frame pump present the driven state. */
    private fun settleFrames(settleMilliseconds: Long = CAPTURE_SETTLE_MILLISECONDS) {
        InstrumentationRegistry.getInstrumentation().waitForIdleSync()
        SystemClock.sleep(settleMilliseconds)
    }

    /**
     * Scrolls the open settings sheet toward its capability rows with one
     * real pointer swipe, the gesture a visitor makes. The capture harness
     * cannot compose-scroll a sheet it does not host, so it injects the
     * gesture the same way the behavior suite injects map taps.
     */
    private fun swipeUpWithinSheet() {
        val metrics = ApplicationProvider.getApplicationContext<Context>()
            .resources.displayMetrics
        val horizontalCenterPixels = metrics.widthPixels / 2
        val startYPixels = (metrics.heightPixels * SWIPE_START_HEIGHT_FRACTION).toInt()
        val endYPixels = (metrics.heightPixels * SWIPE_END_HEIGHT_FRACTION).toInt()
        val uiAutomation = InstrumentationRegistry.getInstrumentation().uiAutomation
        val downTime = SystemClock.uptimeMillis()
        fun inject(action: Int, yPixels: Int, eventTime: Long) {
            uiAutomation.injectInputEvent(
                MotionEvent.obtain(
                    downTime,
                    eventTime,
                    action,
                    horizontalCenterPixels.toFloat(),
                    yPixels.toFloat(),
                    0,
                ),
                true,
            )
        }
        inject(MotionEvent.ACTION_DOWN, startYPixels, downTime)
        for (step in 1..SWIPE_STEP_COUNT) {
            val stepY = startYPixels +
                (endYPixels - startYPixels) * step / SWIPE_STEP_COUNT
            val stepTime = downTime + SWIPE_DURATION_MILLISECONDS * step / SWIPE_STEP_COUNT
            inject(MotionEvent.ACTION_MOVE, stepY, stepTime)
        }
        inject(
            MotionEvent.ACTION_UP,
            endYPixels,
            downTime + SWIPE_DURATION_MILLISECONDS,
        )
    }

    /**
     * Scrolls the open layers panel toward its end with a real pointer
     * swipe, the gesture a visitor makes, so the sections below the fold
     * reach the frame.
     */
    private fun swipeTowardTheLayersPanelEnd() = swipeWithinTheLayersPanel(
        LAYERS_PANEL_SWIPE_START_HEIGHT_FRACTION,
        LAYERS_PANEL_SWIPE_END_HEIGHT_FRACTION,
    )

    /** A short reverse drag, landing the panel's first row inside its edge. */
    private fun nudgeTheLayersPanelBack() = swipeWithinTheLayersPanel(
        LAYERS_PANEL_NUDGE_START_HEIGHT_FRACTION,
        LAYERS_PANEL_NUDGE_END_HEIGHT_FRACTION,
    )

    private fun swipeWithinTheLayersPanel(
        startHeightFraction: Float,
        endHeightFraction: Float,
    ) {
        val metrics = ApplicationProvider.getApplicationContext<Context>()
            .resources.displayMetrics
        val horizontalCenterPixels = metrics.widthPixels / 2
        val startYPixels =
            (metrics.heightPixels * startHeightFraction).toInt()
        val endYPixels =
            (metrics.heightPixels * endHeightFraction).toInt()
        val uiAutomation = InstrumentationRegistry.getInstrumentation().uiAutomation
        val downTime = SystemClock.uptimeMillis()
        fun inject(action: Int, yPixels: Int, eventTime: Long) {
            uiAutomation.injectInputEvent(
                MotionEvent.obtain(
                    downTime,
                    eventTime,
                    action,
                    horizontalCenterPixels.toFloat(),
                    yPixels.toFloat(),
                    0,
                ),
                true,
            )
        }
        inject(MotionEvent.ACTION_DOWN, startYPixels, downTime)
        for (step in 1..SWIPE_STEP_COUNT) {
            val stepY = startYPixels +
                (endYPixels - startYPixels) * step / SWIPE_STEP_COUNT
            val stepTime = downTime + SWIPE_DURATION_MILLISECONDS * step / SWIPE_STEP_COUNT
            inject(MotionEvent.ACTION_MOVE, stepY, stepTime)
        }
        inject(
            MotionEvent.ACTION_UP,
            endYPixels,
            downTime + SWIPE_DURATION_MILLISECONDS,
        )
    }

    /** Writes one full-screen PNG into the app's external files directory. */
    private fun captureScreenshot(fileName: String) {
        val bitmap = InstrumentationRegistry.getInstrumentation()
            .uiAutomation
            .takeScreenshot()
            ?: throw AssertionError("the device produced no screenshot")
        val outputFile = File(screenshotDirectory(), fileName)
        FileOutputStream(outputFile).use { outputStream ->
            bitmap.compress(Bitmap.CompressFormat.PNG, PNG_QUALITY_PERCENT, outputStream)
        }
        bitmap.recycle()
    }

    /** The on-device directory the host pulls after the run. */
    private fun screenshotDirectory(): File {
        val context = ApplicationProvider.getApplicationContext<Context>()
        val directory = File(context.getExternalFilesDir(null), SCREENSHOT_DIRECTORY_NAME)
        directory.mkdirs()
        return directory
    }

    /** Polls a state condition until it holds or the named timeout elapses. */
    private fun awaitOrTimeout(
        timeoutMilliseconds: Long,
        timeoutDescription: String,
        condition: () -> Boolean,
    ) {
        val deadlineMilliseconds = SystemClock.elapsedRealtime() + timeoutMilliseconds
        while (!condition()) {
            if (SystemClock.elapsedRealtime() > deadlineMilliseconds) {
                throw AssertionError(
                    "$timeoutDescription did not settle within $timeoutMilliseconds ms",
                )
            }
            SystemClock.sleep(POLL_INTERVAL_MILLISECONDS)
        }
    }

    private companion object {
        /** The instrumentation argument that opts into the capture. */
        const val CAPTURE_ARGUMENT: String = "captureReferenceScreenshots"

        /** The on-device directory the host pulls. */
        const val SCREENSHOT_DIRECTORY_NAME: String = "reference-screenshots"

        /** The four published captures, named in page order. */
        const val PLAN_SHEET_FILE_NAME: String = "android-plan-sheet.png"
        const val PROVIDER_SECTION_FILE_NAME: String = "android-provider-section.png"
        const val NAVIGATION_FILE_NAME: String = "android-navigation.png"
        const val VECTOR_BASE_FILE_NAME: String = "android-vector-base.png"
        const val LAYERS_CLOSED_FILE_NAME: String = "android-layers-closed.png"
        const val LAYERS_OPEN_FILE_NAME: String = "android-layers-open.png"

        /** The layers panel's rows resolve from the registry after its first frame. */
        const val MAP_LAYERS_ROWS_SETTLE_TIMEOUT_MILLISECONDS: Long = 15_000

        /** The playback speed the published navigation frame shows. */
        const val PUBLISHED_SPEED_MULTIPLIER: Double = 1.0

        /** Lossless PNG encoding. */
        const val PNG_QUALITY_PERCENT: Int = 100

        /** Compose and the frame pump usually present within a few frames. */
        const val CAPTURE_SETTLE_MILLISECONDS: Long = 1_500

        /** The vector base's first tiles take this long on the emulator's renderer. */
        const val BASE_MAP_SETTLE_MILLISECONDS: Long = 12_000

        /** The swipe starts low on the screen. */
        const val SWIPE_START_HEIGHT_FRACTION: Float = 0.82f

        /** A short travel lands the capability section's first row in frame. */
        const val SWIPE_END_HEIGHT_FRACTION: Float = 0.64f

        /** The layers-panel swipe starts in its lower rows. */
        const val LAYERS_PANEL_SWIPE_START_HEIGHT_FRACTION: Float = 0.62f

        /** The layers-panel swipe travels toward its upper rows. */
        const val LAYERS_PANEL_SWIPE_END_HEIGHT_FRACTION: Float = 0.22f

        /** The reverse nudge starts high and travels a short distance down. */
        const val LAYERS_PANEL_NUDGE_START_HEIGHT_FRACTION: Float = 0.32f
        const val LAYERS_PANEL_NUDGE_END_HEIGHT_FRACTION: Float = 0.34f

        /** Move events sent for one swipe. */
        const val SWIPE_STEP_COUNT: Int = 10

        /** Wall time the swipe takes. */
        const val SWIPE_DURATION_MILLISECONDS: Long = 300

        /** State polling interval, matching the behavior suite. */
        const val POLL_INTERVAL_MILLISECONDS: Long = 250

        /** The application state construction is a startup step. */
        const val STATE_CONSTRUCTION_TIMEOUT_MILLISECONDS: Long = 30_000

        /** The first ready frame and the first trip instruction on SwiftShader. */
        const val READY_TIMEOUT_MILLISECONDS: Long = 120_000

        /** A recorded demo route plans without a provider call. */
        const val ROUTE_TIMEOUT_MILLISECONDS: Long = 30_000
    }
}
