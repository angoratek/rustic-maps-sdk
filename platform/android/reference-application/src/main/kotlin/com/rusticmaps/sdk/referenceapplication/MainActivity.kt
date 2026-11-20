package com.rusticmaps.sdk.referenceapplication

import android.content.res.AssetManager
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.systemBarsPadding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import com.rusticmaps.sdk.MapsSdkFacade
import com.rusticmaps.sdk.demo.ProviderRegistryLoader
import com.rusticmaps.sdk.demo.ReferenceSceneDemoPlan
import com.rusticmaps.sdk.demo.ReferenceScenesLoader
import com.rusticmaps.sdk.referenceapplication.navigation.SimulatedTraceLoader
import com.rusticmaps.sdk.referenceapplication.navigation.SimulatedTrace
import com.rusticmaps.sdk.referenceapplication.speech.SpeechGuidanceSpeaker
import com.rusticmaps.sdk.referenceapplication.DemoPlan
import com.rusticmaps.sdk.ux.LocalReferenceAppUXExtendedColors
import com.rusticmaps.sdk.ux.ReferenceAppUXDimensions
import com.rusticmaps.sdk.ux.ReferenceAppUXTheme
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel

/**
 * Hosts the reference application: the design-language theme applied at first
 * paint (dark default, docs/ReferenceAppUX.md §3), the app state constructed
 * keyless (provider credentials are entered at runtime, §6.5), the demo plans
 * over the bundled recorded traces, the platform speech engine, and the
 * opt-in secure credential store.
 */
class MainActivity : ComponentActivity() {
    /**
     * The automation read-back seam of the connected-Android-test suite
     * (round 12): the constructed [ReferenceApplicationState] is mirrored
     * here so the androidTest sources can drive the same user-path methods
     * the sheet's controls call and assert the exact values the screen
     * renders — the Android analogue of the iOS reference app's launch-
     * argument demo automation (`ReferenceApplicationLaunchAutomation.swift`),
     * which drives the same methods because `simctl` cannot inject touch.
     * Android instrumentation drives them directly; no state is ever forced.
     * Null until the startup composition built the state.
     */
    internal var automationApplicationState: ReferenceApplicationState? = null
        private set

    /**
     * Mirrored verbatim startup-failure detail (round 12's same seam): the
     * suite fails fast with this text instead of timing out when the
     * application never constructed (a refused key, a refused provider set).
     */
    internal var automationStartupFailureText: String? = null
        private set

    /** Notes the constructed application state for the suite (the mirror above). */
    internal fun noteConstructedApplicationStateForAutomation(applicationState: ReferenceApplicationState) {
        automationApplicationState = applicationState
    }

    /** Notes the verbatim startup failure for the suite (the mirror above). */
    internal fun noteStartupFailureForAutomation(failureText: String) {
        automationStartupFailureText = failureText
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Software-renderer environment workaround (documented, demo only):
        // this AVD's graphics stack is ANGLE over SwiftShader Vulkan, whose
        // swapchains advertise only the `Inherit` composite-alpha mode, which
        // the SDK's `Opaque` surface request rejects straight out. wgpu
        // selects the backend at device creation, so the GL (EGL/GLES) backend
        // is requested before the first SDK call; on hardware this line is a
        // no-op for the default Vulkan selection only inside this demo.
        android.system.Os.setenv("WGPU_BACKEND", "gl", false)
        setContent {
            ReferenceApplicationRoot()
        }
    }
}

/**
 * Constructs the application state once (the keyless SDK facade, the shared
 * demo artifact, the loaded demo plans, the speech engine, the secure
 * credential store) and renders the screen; a construction failure surfaces
 * verbatim in the failed readiness state while the theme already applies
 * (deterministic first paint, §3).
 *
 * No provider credential is a build input: the SDK is constructed with an
 * unconfigured registry, and a credential the user chose to remember is
 * loaded from the platform secure store and applied at startup (§6.5). The
 * offline tile cache registers only after a successful credential
 * application.
 */
@Composable
private fun ReferenceApplicationRoot() {
    var applicationState: ReferenceApplicationState? by remember { mutableStateOf(null) }
    var startupFailureText: String? by remember { mutableStateOf(null) }
    // The screen-level composition scope the SDK's suspend calls launch into;
    // the main dispatcher is immediate so the first frame lands this paint.
    // Remembered once per composition: a fresh scope on every recomposition
    // would leak every launched job.
    val applicationScope = remember {
        CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    }
    val applicationContext = LocalContext.current
    val applicationAssetManager = applicationContext.assets

    LaunchedEffect(Unit) {
        try {
            val providerRegistry = ProviderRegistryLoader.load(applicationAssetManager)
            // The facade drives provider selection from the shared registry's
            // descriptors, so it is constructed with the decoded registry.
            val facade = MapsSdkFacade(providerRegistry.providers)
            val scenesArtifact = ReferenceScenesLoader.load(applicationAssetManager)
            // The picker rows come from the shared artifact's `demo_plans`
            // section, so the labels and descriptions cannot drift from the
            // rows the other reference applications show.
            val demoPlans = scenesArtifact.demoPlans.map { planDefinition ->
                buildDemoPlan(
                    assetManager = applicationAssetManager,
                    planDefinition = planDefinition,
                    scenarioNameOfTheArtifactTrace = scenesArtifact.navigationDemo.traceName,
                )
            }
            val speechSpeaker = SpeechGuidanceSpeaker(applicationContext)
            val applicationStateResult = ReferenceApplicationState(
                sdkFacade = facade,
                artifact = scenesArtifact,
                providerRegistry = providerRegistry,
                demoPlans = demoPlans,
                speechSpeaker = speechSpeaker,
                coroutineScope = applicationScope,
                providerCredentialStore = ProviderCredentialStore(applicationContext),
                themeChoiceStore = ThemeChoiceStore(applicationContext),
                rememberedChoicesStore = RememberedChoicesStore(applicationContext),
                tileCacheDirectoryPath = applicationContext.cacheDir.absolutePath,
                assetManager = applicationAssetManager,
                applicationContext = applicationContext,
            )
            // An opted-in credential from a previous run is applied here; the
            // offline tile cache then registers inside the state (§6.5).
            applicationStateResult.restorePersistedCredentials()
            // The explicit choices from a previous run apply before the first
            // frame: theme and base map restore silently, the demo plan and
            // scenes fill their pickers, and the remembered search pair fills
            // the fields without planning a route.
            applicationStateResult.restoreRememberedChoices()
            // Open on the registry's keyless base map when nothing else serves
            // one (M24): a keyless run would otherwise draw an empty surface.
            applicationStateResult.selectKeylessBaseMapWhenUncredentialed()
            // The remembered base map applies after that default, because the
            // keyless raster registration replaces the vector slot. A
            // remembered choice always wins over the application's default.
            applicationStateResult.restoreRememberedBaseMap()
            applicationState = applicationStateResult
            // The androidTest seam (round 12): the composition's context is
            // the MainActivity; the mirror stays per-activity.
            (applicationContext as? MainActivity)?.noteConstructedApplicationStateForAutomation(
                applicationStateResult,
            )
            // The automation credential seam (§6.5): a launch-intent extra
            // supplies a provider key at runtime for local and CI UI
            // automation. It is applied through the public registration path,
            // never persisted by the injection and never logged; it is not a
            // build-time key seam.
            val runtimeIntent = (applicationContext as? MainActivity)?.intent
            val runtimeCredential = runtimeIntent
                ?.getStringExtra(ReferenceApplicationConstants.RUNTIME_CREDENTIAL_INTENT_EXTRA)
            if (!runtimeCredential.isNullOrBlank()) {
                // The provider key is generic: the launch intent may name one,
                // otherwise the only provider the registry declares available
                // on this platform is the target (never a vendor assumption).
                val runtimeProviderKey = runtimeIntent
                    .getStringExtra(ReferenceApplicationConstants.RUNTIME_PROVIDER_KEY_INTENT_EXTRA)
                    ?.takeIf { providerKey -> providerKey.isNotBlank() }
                    ?: applicationStateResult.defaultRuntimeInjectionProviderKey()
                if (runtimeProviderKey != null) {
                    applicationStateResult.applyRuntimeInjectedCredential(
                        runtimeProviderKey,
                        runtimeCredential,
                    )
                }
            }
            // Type-ahead's provider slot (docs/ReferenceAppUX.md §5.5): a
            // credentialed session already holds the provider its credential
            // installed, so only a keyless one registers the registry's
            // credential-less search provider. The call reads the SDK's own
            // registration readback and names no provider itself.
            applicationStateResult.registerKeylessSearchProviderWhenUncredentialed()
            // The tutorial's worked example (M25): the automation reads one
            // boolean extra and the state runs the same steps the guides
            // publish, once the shipped map view exists.
            val isWalkthroughRequested = runtimeIntent
                ?.getBooleanExtra(ReferenceApplicationConstants.WALKTHROUGH_INTENT_EXTRA, false) == true
            if (isWalkthroughRequested) {
                applicationStateResult.isWalkthroughRequested = true
            }
            // The shared flow schema's launch automation (M64): the intent
            // names one flow ID and the state records it; the instrumented
            // suite drives the same user-path methods and asserts the outcome.
            runtimeIntent
                ?.getStringExtra(ReferenceApplicationConstants.REFERENCE_FLOW_INTENT_EXTRA)
                ?.takeIf { flowId -> flowId.isNotBlank() }
                ?.let { flowId -> applicationStateResult.requestReferenceFlow(flowId) }
            // The opening base map: one provider key from the launch intent,
            // resolved through the registry like any other selection. Used
            // with a keyless source so the app shows imagery without a
            // credential.
            val demoBaseMapProviderKey = runtimeIntent
                ?.getStringExtra(ReferenceApplicationConstants.DEMO_BASE_MAP_INTENT_EXTRA)
            if (!demoBaseMapProviderKey.isNullOrBlank()) {
                applicationStateResult.selectBaseMap(demoBaseMapProviderKey)
            }
        } catch (startupException: Exception) {
            startupFailureText = startupException.message ?: startupException::class.simpleName
            (applicationContext as? MainActivity)?.noteStartupFailureForAutomation(
                startupFailureText ?: "unknown startup failure",
            )
        }
    }

    // The composition leaves when the activity is destroyed: release the
    // platform resources the state owns (the location source, the speech
    // engine and the native SDK handle) and cancel the scope that owns every
    // in-flight job. The state is the same instance the screen rendered.
    DisposableEffect(Unit) {
        onDispose {
            // Stop the in-flight work first, then release the platform
            // resources: a pending SDK call must not reach a closed handle.
            applicationScope.cancel()
            applicationState?.shutdown()
        }
    }

    // §3: the choice resolves against the live system scheme while it is
    // Auto; an explicit Light or Dark choice ignores the system.
    val systemInDarkTheme = isSystemInDarkTheme()
    val isDarkTheme = applicationState?.resolvesToDarkTheme(systemInDarkTheme)
        ?: systemInDarkTheme
    ReferenceAppUXTheme(isDarkTheme = isDarkTheme) {
        Surface(
            modifier = Modifier
                .fillMaxSize()
                .systemBarsPadding(),
            color = MaterialTheme.colorScheme.surface,
            contentColor = MaterialTheme.colorScheme.onSurface,
        ) {
            val resolvedState = applicationState
            if (resolvedState != null) {
                ReferenceApplicationScreen(applicationState = resolvedState)
            } else {
                // The loading line of the readiness ladder (§6.4:
                // deterministic text, no spinners), or the failed text echoing
                // the verbatim construction failure.
                StartupSurface(failedText = startupFailureText)
            }
        }
    }
}

/**
 * Builds one demo plan of the picker from its shared-artifact definition and
 * bundled recorded trace: the artifact names the consumer-facing label, the
 * info-affordance description and the fixture path (the rows name the
 * feature, never the seam — §4.2); the plan is marked when its trace is the
 * artifact's own navigation demo (the plan the demo starts on before any
 * pick).
 */
private fun buildDemoPlan(
    assetManager: android.content.res.AssetManager,
    planDefinition: ReferenceSceneDemoPlan,
    scenarioNameOfTheArtifactTrace: String,
): DemoPlan {
    val loadedTrace = SimulatedTraceLoader.load(
        assetManager = assetManager,
        traceAssetName = planDefinition.traceAssetName,
    )
    return DemoPlan(
        label = planDefinition.label,
        descriptionText = planDefinition.description,
        trace = loadedTrace,
        isArtifactTrace = loadedTrace.traceName == scenarioNameOfTheArtifactTrace,
    )
}

@Composable
private fun StartupSurface(failedText: String?) {
    val extendedColors = LocalReferenceAppUXExtendedColors.current
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(ReferenceAppUXDimensions.GutterInline),
        verticalArrangement = Arrangement.Center,
    ) {
        Text(
            text = if (failedText == null) {
                ReferenceApplicationConstants.LOADING_STATUS_TEXT +
                    ReferenceApplicationConstants.READINESS_LOADING_SUFFIX
            } else {
                ReferenceApplicationConstants.FAILED_STATUS_TEMPLATE.format(failedText)
            },
            style = MaterialTheme.typography.bodySmall,
            color = if (failedText == null) extendedColors.textSecondary else extendedColors.danger,
        )
    }
}