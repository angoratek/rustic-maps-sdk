package com.rusticmaps.sdk.referenceapplication

import android.content.Context
import android.content.res.AssetManager
import android.os.SystemClock
import android.view.Choreographer
import uniffi.map_refapp_core.LocationPoseAnimator
import com.rusticmaps.sdk.DeviceLocationSource
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.SnapshotStateList
import androidx.compose.runtime.snapshots.SnapshotStateMap
import com.rusticmaps.sdk.MapCameraPose
import com.rusticmaps.sdk.MapsSdkFacade
import com.rusticmaps.sdk.MapView as ShippedMapView
import com.rusticmaps.sdk.MapViewFailure
import com.rusticmaps.sdk.MapViewFailurePhase
import com.rusticmaps.sdk.MapViewListener
import com.rusticmaps.sdk.demo.CameraPreset
import com.rusticmaps.sdk.demo.OverlayScene
import com.rusticmaps.sdk.ProviderCredentialDescriptor
import com.rusticmaps.sdk.demo.ProviderRegistry
import com.rusticmaps.sdk.demo.ReferenceSceneDemoPlace
import com.rusticmaps.sdk.demo.ReferenceSceneDemoRoute
import com.rusticmaps.sdk.demo.ReferenceScenesArtifact
import com.rusticmaps.sdk.VectorStyleDescriptor
import com.rusticmaps.sdk.navigation.CollectingNavigationStateListener
import com.rusticmaps.sdk.referenceapplication.console.ProviderConsoleEntry
import com.rusticmaps.sdk.referenceapplication.console.ProviderConsoleLevelFilter
import com.rusticmaps.sdk.referenceapplication.map.CollectingDiagnosticEventListener
import com.rusticmaps.sdk.referenceapplication.map.DeviceLocationPanelStatus
import com.rusticmaps.sdk.referenceapplication.map.MapLayerBaseSelection
import com.rusticmaps.sdk.referenceapplication.map.MapLayerTrafficAvailability
import com.rusticmaps.sdk.referenceapplication.map.MapLayersPanelState
import com.rusticmaps.sdk.referenceapplication.navigation.SimulatedProcessedFix
import com.rusticmaps.sdk.referenceapplication.navigation.SimulatedTrace
import com.rusticmaps.sdk.referenceapplication.navigation.SimulatedTripDriver
import com.rusticmaps.sdk.referenceapplication.speech.SpeechGuidanceSpeaker
import com.rusticmaps.sdk.referenceapplication.walkthrough.WalkthroughScenario
import com.rusticmaps.sdk.ux.ReferenceAppUXDimensions
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.map_refapp_core.FollowBearingMode
import uniffi.map_refapp_core.FollowCamera
import uniffi.map_refapp_core.FollowPhase
import uniffi.rustic_maps.AvoidOptions
import uniffi.rustic_maps.EdgeInsets
import uniffi.rustic_maps.GeocodeQuality
import uniffi.rustic_maps.GeographicCoordinate
import uniffi.rustic_maps.LocationIndicatorState
import uniffi.rustic_maps.LocationIndicatorStyle
import uniffi.rustic_maps.LocationIndicatorVisualKind
import uniffi.rustic_maps.Navigator
import uniffi.rustic_maps.MapOverlay
import uniffi.rustic_maps.MapOverlayKind
import uniffi.rustic_maps.OverlayColor
import uniffi.rustic_maps.MapSdk
import uniffi.rustic_maps.NavigationPhase
import uniffi.rustic_maps.NavigationPhaseChange
import uniffi.rustic_maps.OfflineManager
import uniffi.rustic_maps.Place
import uniffi.rustic_maps.PositionController
import uniffi.rustic_maps.PositionFix
import uniffi.rustic_maps.PositionSourceError
import uniffi.rustic_maps.ProviderContract
import uniffi.rustic_maps.RasterImageFormat
import uniffi.rustic_maps.RasterTileResolution
import uniffi.rustic_maps.Route
import uniffi.rustic_maps.RouteOptions
import uniffi.rustic_maps.SdkException
import uniffi.rustic_maps.SdkLogSeverity
import uniffi.rustic_maps.SpeechInstructionKind
import uniffi.rustic_maps.TileCacheStatistics
import uniffi.rustic_maps.TrafficFlowCacheStatistics
import uniffi.rustic_maps.TrafficFlowCongestionLevel
import uniffi.rustic_maps.TrafficFlowData
import uniffi.rustic_maps.UnitSystem
import uniffi.rustic_maps.UrlTemplateRasterTileSource
import uniffi.rustic_maps.ProviderVectorTileSource
import uniffi.rustic_maps.UrlTemplateVectorTileSource
import uniffi.rustic_maps.VectorTileSourceStatus
import uniffi.rustic_maps.getSdkCompiledVersion
import uniffi.rustic_maps.offlineRouteFromArchiveBytes

/**
 * The UI state of the reference application: every presentation value the
 * screen renders lives in Compose snapshot state here, and every SDK call
 * runs through this one class so failures are reported exactly once, verbatim
 * (docs/ReferenceAppUX.md §6.3: no catch block may end without a state change
 * or an error presentation).
 *
 * The v2 interface shape (§1/§4.1/§4.2) is one bottom sheet with the peek/
 * half/full states, the plan flow (search view → route summary card), the
 * navigation drawer during a simulated trip, the offline-maps card over the
 * registered tile cache, and the diagnostics sheet behind the readiness chip.
 *
 * The application starts keyless (§6.5): provider credentials are entered on
 * the settings surface and applied at runtime. This class owns the credential
 * state (the registry's descriptor rows, the masked drafts, the per-provider
 * applied status, the opt-in remember toggle) and the settings visibility; it
 * is the only caller of [MapsSdkFacade.applyProviderCredential] in the app.
 */
class ReferenceApplicationState(
    /** The SDK facade the app was constructed with (keyless until a credential is applied). */
    val sdkFacade: MapsSdkFacade,
    /** The shared demo artifact (docs/ReferenceAppUX.md §9). */
    val artifact: ReferenceScenesArtifact,
    /** The shared provider registry's descriptors (platform/providers.json). */
    val providerRegistry: ProviderRegistry,
    /** The demo plans the demo-plan picker offers, each a loaded recorded trace. */
    val demoPlans: List<DemoPlan>,
    /** The speech guidance speaker (the platform TextToSpeech seam, §4.1). */
    val speechSpeaker: SpeechGuidanceSpeaker,
    /** The scope the asynchronous SDK calls launch into. */
    private val coroutineScope: CoroutineScope,
    /** The opt-in secure store backing "Remember on this device" (§6.5). */
    private val providerCredentialStore: ProviderCredentialStore,
    /** The device-local store of the three-way theme choice (§3). */
    private val themeChoiceStore: ThemeChoiceStore,
    /** The device-local store of every other explicit choice. */
    private val rememberedChoicesStore: RememberedChoicesStore,
    /**
     * The application caches directory the offline tile cache lives in. The
     * cache registers only after a credential is successfully applied, so the
     * path is held here rather than passed per call.
     */
    private val tileCacheDirectoryPath: String,
    /**
     * The application's asset manager, used to read the committed offline
     * routing archive the offline card's route control plans over. The read
     * happens on the tap, never at startup, so a missing archive reports
     * through the error strip instead of failing the launch.
     */
    private val assetManager: AssetManager,
    /** The application context the device location source checks and uses. */
    private val applicationContext: Context,
) {
    // -------------------------------------------------------------------
    // SDK objects
    // -------------------------------------------------------------------
    /** The compiled SDK version the diagnostics sheet reports (§6.1). */
    val sdkVersion: String = getSdkCompiledVersion()

    /** The SDK the shipped map view draws from. */
    val nativeMapSdk: MapSdk
        get() = sdkFacade.nativeSdk

    /**
     * The shipped, lifecycle-aware map view (M17): the screen's Compose
     * `MapView` wrapper hands it over through [onMapViewReady]. Null until
     * the composition creates it.
     */
    var mapView: ShippedMapView? by mutableStateOf(null)
        private set

    /**
     * The shipped view's callbacks: readiness, camera changes, failures and
     * frame accounting feed the same ladder and diagnostics the app-side
     * surface host used to drive.
     */
    val mapViewListener: MapViewListener = object : MapViewListener {
        override fun onMapReady() {
            readinessReport.noteReady()
            clearErrorReport()
        }

        override fun onCameraChanged(camera: MapCameraPose, isGestureDriven: Boolean) {
            applyCameraPoseFromMapView(camera, isGestureDriven)
        }

        override fun onMapClick(coordinate: GeographicCoordinate) {
            mapClickCount += 1
        }

        override fun onError(failure: MapViewFailure) {
            if (isExpectedKeylessProviderFailure(failure.cause)) {
                // Expected while keyless; the neutral prompt covers it.
                readinessReport.noteExpectedKeylessFailure()
                return
            }
            if (failure.phase == MapViewFailurePhase.DRAW) {
                framesFailedCount += 1
            }
            readinessReport.noteFailure(
                state = if (failure.phase == MapViewFailurePhase.SURFACE) {
                    ReadinessState.FAILED
                } else {
                    ReadinessState.DEGRADED
                },
                detailText = failureDetailTextOf(failure.cause),
            )
            reportFailureOf(failure.cause)
        }

        override fun onFrameStatistics(statistics: uniffi.rustic_maps.FrameStatistics) {
            latestTileStatistics = statistics
            // The vector base's honest accounting follows the frame, so the
            // base-map section can say when tiles failed and the next frame
            // retries them (M22). The refresh follows the request rather than
            // the serving base: a queued install starts out not configured and
            // becomes serving only once a later frame's status says so.
            if (selectedVectorBaseChoiceKey != null) {
                val previouslyServingChoiceKey = activeVectorBase?.choiceKey
                vectorBaseStatus = mapView?.vectorTileSourceStatus()
                // A queued install starts out not configured and serves only
                // once a later frame reports it (and its readback carries the
                // real source and style): the panel's serving mark follows the
                // serving choice, not only the configured flag.
                if (activeVectorBase?.choiceKey != previouslyServingChoiceKey) {
                    refreshMapLayersState()
                }
            }
            framesDrawnCount += 1
            if (readinessState != ReadinessState.READY) {
                readinessReport.noteReady()
            }
            clearErrorReport()
        }

        override fun onFrameDeferred() {
            framesDeferredCount += 1
        }

        override fun onFrameSettled() {
            framesSettledCount += 1
        }
    }

    /**
     * Adopts the view the screen's Compose wrapper created: applies the
     * current camera and re-pushes any overlay or location state the state
     * already holds (a recomposition can recreate the view).
     */
    fun onMapViewReady(view: ShippedMapView) {
        mapView = view
        view.setCamera(currentCameraPose())
        pushOverlaysToSurface()
        val currentFix = latestLocationFix
        if (currentFix != null) {
            pushLocationIndicator(guided = tripDriver.isTripActive, fix = currentFix)
        }
        if (isWalkthroughRequested) {
            runWalkthroughDemo()
        }
        applyPendingRememberedVectorBase()
        applyPendingKeylessVectorBase()
        refreshMapLayersState()
        // A remembered device-location source starts once the map session is
        // ready; its first accepted fix centers the camera.
        restoreRememberedDeviceLocationIfPermitted()
        // A traffic layer restored before the surface attached gets its first
        // camera settle now; an unchanged pose schedules nothing.
        mapLayersState.noteCameraPoseChanged(cameraPoseKey())
    }

    /**
     * Installs the registry's keyless styled base once the shipped map view
     * exists. The startup path calls this too, because the view may already
     * be ready when the keyless selection runs; whichever call comes second
     * finds nothing pending. An applied credential or an explicit selection
     * always wins, so neither is ever overridden here.
     */
    private fun applyPendingKeylessVectorBase() {
        val pendingChoiceKey = pendingKeylessVectorBaseChoiceKey ?: return
        if (appliedProviderKeys.isNotEmpty() || selectedVectorBaseChoiceKey != null) {
            pendingKeylessVectorBaseChoiceKey = null
            return
        }
        // The view may not exist yet at startup construction; the install
        // waits for it rather than reporting a not-ready refusal.
        if (mapView == null) {
            return
        }
        pendingKeylessVectorBaseChoiceKey = null
        if (vectorBaseMapChoices.none { choice -> choice.choiceKey == pendingChoiceKey }) {
            return
        }
        selectVectorBase(pendingChoiceKey)
    }

    /**
     * Whether the launch automation asked for the tutorial's worked example
     * (M25, `docs/Release.md`). Setting it arms the walkthrough;
     * it starts as soon as the map view exists, so an intent extra that
     * arrives before the composition finishes still runs exactly once.
     */
    var isWalkthroughRequested: Boolean = false
        set(value) {
            field = value
            if (value) {
                runWalkthroughDemo()
            }
        }

    /**
     * The reference flow a launch intent asked for, or null. The value is
     * one of [ReferenceApplicationConstants.REFERENCE_FLOW_IDS]; the flow
     * itself drives the same public user-path methods the sheet's controls
     * call, so the identifier is a test seam rather than a second
     * implementation.
     */
    var requestedReferenceFlowId: String? by mutableStateOf(null)
        private set

    /** Records a validated reference flow identifier from the launch intent. */
    fun requestReferenceFlow(flowId: String) {
        if (flowId in ReferenceApplicationConstants.REFERENCE_FLOW_IDS) {
            requestedReferenceFlowId = flowId
        }
    }

    /**
     * Runs the tutorial's worked example over the live map: the same steps,
     * in the same order and with the same identifiers the guides publish.
     *
     * This exists so the generated chapters describe code the application
     * really executes; nothing here is a documentation-only path.
     */
    fun runWalkthroughDemo(
        originSearchQuery: String = ReferenceApplicationConstants.WALKTHROUGH_ORIGIN_SEARCH_QUERY,
        destinationSearchQuery: String =
            ReferenceApplicationConstants.WALKTHROUGH_DESTINATION_SEARCH_QUERY,
    ) {
        val currentMapView = mapView ?: return
        // The offline step plans over the committed routing graph; a missing
        // archive skips that step rather than inventing a route.
        val offlineArchiveBytes = try {
            assetManager.open(ReferenceApplicationConstants.OFFLINE_ROUTING_ARCHIVE_ASSET_NAME)
                .use { archiveStream -> archiveStream.readBytes() }
        } catch (archiveException: Exception) {
            null
        }
        coroutineScope.launch {
            try {
                WalkthroughScenario(nativeMapSdk, currentMapView)
                    .runWalkthrough(
                        originSearchQuery,
                        destinationSearchQuery,
                        offlineArchiveBytes,
                        transportationMode =
                            directionsState.effectiveTransportationMode.transportationMode,
                        routeStrategy = directionsState.effectiveRouteStrategy.routeStrategy,
                    )
                clearErrorReport()
            } catch (walkthroughException: Exception) {
                if (walkthroughException is CancellationException) {
                    throw walkthroughException
                }
                // Failing guidance must report, never crash (§6.3).
                reportFailureOf(walkthroughException)
            }
        }
    }

    /** The navigation controller of the simulated trip. */
    private val navigationController = Navigator()

    /** The collecting listener all navigation topics (speech included, round 10) broadcast into. */
    val navigationListener = CollectingNavigationStateListener()

    /** The collecting listener of the diagnostics events (round 10, the sheet's readout). */
    val diagnosticEventListener = CollectingDiagnosticEventListener()

    /** Registry display names, keyed by provider key, for the console's rows. */
    private val providerDisplayNamesByProviderKey: Map<String, String> =
        providerRegistry.providers.associate { descriptor ->
            descriptor.providerKey to descriptor.displayName
        }

    /**
     * The console's stream, oldest first, with the typed provider traces and
     * the registry display names resolved. The shared listener owns the
     * bounded window; this view adds no second copy.
     */
    val providerConsoleEntries: List<ProviderConsoleEntry>
        get() = diagnosticEventListener.eventLines.map { eventLine ->
            ProviderConsoleEntry.fromDiagnosticEventLine(
                eventLine,
                providerDisplayNamesByProviderKey,
            )
        }

    /** The simulated-trip driver replaying the active plan's trace. */
    val tripDriver: SimulatedTripDriver = SimulatedTripDriver(
        navigationController = navigationController,
        listener = navigationListener,
        onSnappedPosition = { processedFix ->
            handleSimulatedProcessedFix(processedFix)
        },
    )

    // -------------------------------------------------------------------
    // Theme (docs/ReferenceAppUX.md §3: Light, Dark or Auto; Auto default)
    // -------------------------------------------------------------------
    /**
     * The record read once at construction. The theme and the base map apply
     * before the first frame; every other field is adopted by
     * [restoreRememberedChoices] once the registry-backed choices exist.
     */
    private val startupRememberedChoices: RememberedChoices? = rememberedChoicesStore.load()

    /** True while startup values are being adopted, so they are not rewritten. */
    private var isRestoringRememberedChoices: Boolean = false

    /** True while the startup opens its default base, which is not a user choice. */
    private var isApplyingAutomaticBaseMap: Boolean = false

    /** The base map the user explicitly chose (or the record restored), if any. */
    private var rememberedBaseMapProviderKey: String? = startupRememberedChoices?.baseMapProviderKey
    private var rememberedBaseMapStyleId: String? = startupRememberedChoices?.baseMapStyleId

    /**
     * The per-capability provider section's state: one row per SDK readback
     * entry, the shared registry's eligible providers, and the remembered
     * choices. It is built from the record read at startup, so a remembered
     * choice re-applies after the credentials restore.
     */
    val capabilityProviderState: CapabilityProviderState = CapabilityProviderState(
        gateway = SdkCapabilityProviderGateway(sdkFacade),
        descriptors = providerRegistry.providers,
        initiallyRememberedProviderKeys =
            startupRememberedChoices?.capabilityProviders ?: emptyMap(),
        onRememberedProviderKeysChanged = { persistRememberedChoices() },
    )

    /**
     * The Directions control's state (PRD.md §6 item 36):
     * the selected mode and strategy, the serving routing provider's
     * capability answer, and the remembered choices. It is read whenever the
     * plan sheet opens and after a provider change, so a provider the user
     * just installed (or the keyless state before one) is what the control
     * reports.
     */
    val directionsState: DirectionsState = DirectionsState(
        gateway = SdkDirectionsCapabilitiesGateway(sdkFacade),
        initiallySelectedMode = startupRememberedChoices?.directionsTransportationMode
            ?: DirectionsTransportationMode.CAR,
        initiallySelectedStrategy = startupRememberedChoices?.directionsRouteStrategy
            ?: DirectionsRouteStrategy.FASTEST,
        onSelectionChanged = { persistRememberedChoices() },
        readFailureReason = { failure -> failureDetailTextOf(failure) },
    )

    /**
     * The on-map layer panel's state machine (docs/ReferenceAppUX.md §4.2):
     * the base-map rows, the traffic layer's debounced capability calls and
     * the device-location row. It reaches the SDK, the shipped map view's
     * camera bounds, the base-map selection path and the remembered record
     * only through the seams built here.
     */
    val mapLayersState: MapLayersPanelState = MapLayersPanelState(
        coroutineScope = coroutineScope,
        fetchTrafficFlow = { boundingBox ->
            sdkFacade.nativeSdk.fetchTrafficFlow(boundingBox)
        },
        cameraGeographicBounds = { mapView?.cameraGeographicBounds() },
        installTrafficOverlay = { trafficFlowSegments ->
            this.trafficFlowSegments = trafficFlowSegments
            pushOverlaysToSurface()
        },
        clearTrafficOverlay = {
            trafficFlowSegments = emptyList()
            pushOverlaysToSurface()
        },
        loadRememberedTrafficChoice = {
            rememberedChoicesStore.load()?.trafficLayerEnabled == true
        },
        persistTrafficChoice = { persistRememberedChoices() },
        readTrafficCacheStatistics = {
            sdkFacade.nativeSdk.trafficFlowCacheStatistics()
        },
        failureText = { failure -> failureDetailTextOf(failure) },
    )

    /** The traffic layer's flow rows currently drawn; empty while the layer is off. */
    var trafficFlowSegments: List<TrafficFlowData> by mutableStateOf(emptyList())
        private set

    /**
     * Rebuilds the layer panel's base rows and traffic availability from the
     * current choices and applied credentials. Every base-map or credential
     * change calls this, so the panel never renders a stale serving mark or a
     * missing-credential row that is now servable.
     */
    fun refreshMapLayersState() {
        mapLayersState.refreshBaseRows(
            rasterChoices = baseMapChoices,
            vectorChoices = vectorBaseMapChoices,
            activeRasterProviderKey = if (activeVectorBase == null) {
                activeBaseMap?.providerKey
            } else {
                null
            },
            activeVectorChoiceKey = activeVectorBase?.choiceKey,
            keylessVectorChoiceKey = keylessVectorBaseChoice?.choiceKey,
        )
        mapLayersState.updateTrafficConfiguration(
            providerDescriptors = providerRegistry.providers,
            appliedProviderKeys = appliedProviderKeys.toSet(),
        )
    }

    /** The collapsed control's act: refresh the rows and toggle the panel. */
    fun toggleMapLayersPanel() {
        if (mapLayersState.isPanelOpen) {
            closeMapLayersPanel()
        } else {
            openMapLayersPanel()
        }
    }

    /** The collapsed control's open act. */
    fun openMapLayersPanel() {
        refreshMapLayersState()
        mapLayersState.openPanel()
    }

    /** The panel's close act (its Close control or the platform back action). */
    fun closeMapLayersPanel() {
        mapLayersState.closePanel()
    }

    /** The traffic switch's act. */
    fun setTrafficLayerEnabled(isEnabled: Boolean) {
        mapLayersState.setTrafficEnabled(isEnabled)
    }

    /** One base row's act from the layer panel. */
    fun selectMapLayerBase(selection: MapLayerBaseSelection) {
        when (selection) {
            is MapLayerBaseSelection.Raster -> selectBaseMap(selection.providerKey)
            is MapLayerBaseSelection.Vector -> selectVectorBase(selection.choiceKey)
        }
    }

    /** The demo plan the user explicitly chose (the startup default is not one). */
    private var rememberedDemoPlanScenarioName: String? =
        startupRememberedChoices?.demoPlanScenarioName

    /**
     * The choice in force — Light, Dark or Auto. A device that never chose
     * starts on Auto; an explicit choice is remembered on this device.
     */
    var themeChoice: ThemeChoice by mutableStateOf(
        startupRememberedChoices?.themeChoice ?: themeChoiceStore.load(),
    )
        private set

    /** Cycles light → dark → auto → light and remembers the new choice. */
    fun cycleThemeChoice() {
        themeChoice = themeChoice.nextChoice
        themeChoiceStore.store(themeChoice)
        persistRememberedChoices()
    }

    /** The resolved presentation theme for a system color scheme. */
    fun resolvesToDarkTheme(systemInDarkTheme: Boolean): Boolean =
        themeChoice.resolvesToDarkTheme(systemInDarkTheme)

    // -------------------------------------------------------------------
    // Remembered choices (PRD.md §6 item 67)
    // -------------------------------------------------------------------

    /**
     * Adopts the record read at construction, once the registry-backed demo
     * content exists and before the first frame or plan. The theme is already
     * in force; the base map and camera preset restore silently, the demo
     * plan and scenes fill their pickers, the speed returns to the chosen
     * pace, and the remembered search pair fills the fields without planning
     * a route — the user presses Plan route.
     */
    fun restoreRememberedChoices() {
        val remembered = startupRememberedChoices ?: return
        isRestoringRememberedChoices = true
        try {
            themeChoice = remembered.themeChoice
            remembered.demoPlanScenarioName
                ?.let { scenarioName ->
                    demoPlans.firstOrNull { demoPlan ->
                        demoPlan.scenarioName == scenarioName
                    }
                }
                ?.let { rememberedPlan -> selectedDemoPlan = rememberedPlan }
            sceneToggleStates.keys.forEach { sceneKey ->
                sceneToggleStates[sceneKey] = sceneKey in remembered.enabledSceneKeys
            }
            remembered.cameraPresetIdentifier
                ?.let { identifier ->
                    artifact.cameraPresets.firstOrNull { preset ->
                        preset.identifier == identifier
                    }
                }
                ?.let(::applyPreset)
            remembered.simulatedTripSpeedMultiplier
                ?.takeIf { multiplier ->
                    multiplier in ReferenceApplicationConstants.SIMULATED_TRIP_SPEED_MULTIPLIER_CHOICES
                }
                ?.let(::selectSimulatedTripSpeedMultiplier)
            originSearchText = remembered.originSearchText
            destinationSearchText = remembered.destinationSearchText
            lastSearchQueryText = remembered.lastSearchQueryText
            // The per-capability choices apply last: the credential restore
            // ran first, so a credentialed provider finds the value the
            // session already holds. A restore never rewrites the record.
            capabilityProviderState.applyRememberedProviderKeys()
            // The Directions choices apply before the first plan; the
            // capability read then states what the serving provider honours.
            directionsState.restoreSelections(
                remembered.directionsTransportationMode,
                remembered.directionsRouteStrategy,
            )
            directionsState.refresh()
            // The layer panel reads the same choices and registrations: its
            // base rows and the traffic switch's availability resolve now,
            // and a remembered traffic choice applies only once the
            // capability's credential is in force.
            refreshMapLayersState()
            mapLayersState.restoreRememberedTrafficIfAvailable()
            // The device-location choice restores once the shipped view
            // exists, because the source needs a map session and its first
            // fix centers the camera. A record written without the field
            // reads as off.
            remembersDeviceLocationEnabled = remembered.deviceLocationEnabled
            pendingRememberedDeviceLocationStart = remembered.deviceLocationEnabled
            restoreRememberedDeviceLocationIfPermitted()
        } finally {
            isRestoringRememberedChoices = false
        }
    }

    /** The capability row's picker act; the state holder owns the logic. */
    fun selectCapabilityProvider(contract: ProviderContract, providerKey: String) {
        capabilityProviderState.selectProvider(contract, providerKey)
        // A provider change can move the routing slot, so the Directions
        // control re-reads what the serving provider now honours.
        directionsState.refresh()
    }

    /**
     * Applies the remembered base map after the keyless default registered
     * its raster source. The keyless default is not a user choice and must not
     * remove a remembered styled base; this call therefore runs last.
     */
    fun restoreRememberedBaseMap() {
        val remembered = rememberedChoicesStore.load() ?: return
        isRestoringRememberedChoices = true
        try {
            applyRememberedBaseMap(remembered)
        } finally {
            isRestoringRememberedChoices = false
        }
    }

    /**
     * Applies the remembered base map: a styled vector base is staged for the
     * shipped view (explicit selections and the keyless default always defer
     * to it), while a raster provider that can serve is registered now. A
     * remembered credentialed base whose credential is not on this device
     * stays unapplied and keeps its record for a later launch.
     */
    private fun applyRememberedBaseMap(remembered: RememberedChoices) {
        val providerKey = remembered.baseMapProviderKey ?: return
        val styleId = remembered.baseMapStyleId
        if (styleId != null) {
            val vectorChoice = vectorBaseMapChoices.firstOrNull { choice ->
                choice.descriptor.providerKey == providerKey && choice.style.styleId == styleId
            } ?: return
            pendingRememberedVectorBaseChoiceKey = vectorChoice.choiceKey
            applyPendingRememberedVectorBase()
            return
        }
        val rasterChoice = baseMapChoices.firstOrNull { choice ->
            choice.providerKey == providerKey && choice.isServable
        } ?: return
        selectBaseMap(rasterChoice.providerKey)
    }

    /** The complete current record: the explicit choices, never transient state. */
    private fun currentRememberedChoices(): RememberedChoices = RememberedChoices(
        themeChoice = themeChoice,
        baseMapProviderKey = rememberedBaseMapProviderKey,
        baseMapStyleId = rememberedBaseMapStyleId,
        demoPlanScenarioName = rememberedDemoPlanScenarioName,
        enabledSceneKeys = sceneToggleStates
            .filterValues { isEnabled -> isEnabled }
            .keys
            .toSet(),
        cameraPresetIdentifier = selectedCameraPresetIdentifier,
        simulatedTripSpeedMultiplier = simulatedTripSpeedMultiplier,
        originSearchText = originSearchText,
        destinationSearchText = destinationSearchText,
        lastSearchQueryText = lastSearchQueryText,
        capabilityProviders = capabilityProviderState.rememberedProviderKeys(),
        directionsTransportationMode = directionsState.selectedTransportationMode,
        directionsRouteStrategy = directionsState.selectedRouteStrategy,
        trafficLayerEnabled = mapLayersState.isTrafficEnabled,
        deviceLocationEnabled = remembersDeviceLocationEnabled,
    )

    /** Writes every explicit choice after one changes. */
    private fun persistRememberedChoices() {
        if (isRestoringRememberedChoices || isApplyingAutomaticBaseMap) {
            return
        }
        rememberedChoicesStore.store(currentRememberedChoices())
    }

    /**
     * The settings surface's "Reset remembered choices" act: clears the
     * record and returns the in-memory choices to their defaults. The
     * provider credential store is untouched; a running session keeps the
     * providers it already registered.
     */
    fun resetRememberedChoices() {
        isRestoringRememberedChoices = true
        try {
            themeChoice = ThemeChoice.AUTO
            themeChoiceStore.clear()
            rememberedBaseMapProviderKey = null
            rememberedBaseMapStyleId = null
            rememberedDemoPlanScenarioName = null
            selectedCameraPresetIdentifier = null
            sceneToggleStates.keys.forEach { sceneKey -> sceneToggleStates[sceneKey] = false }
            selectedDemoPlan = demoPlans.first { demoPlan -> demoPlan.isArtifactTrace }
            ReferenceApplicationConstants.SIMULATED_TRIP_SPEED_MULTIPLIER_CHOICES.first().let {
                selectSimulatedTripSpeedMultiplier(it)
            }
            originSearchText = ""
            destinationSearchText = ""
            lastSearchQueryText = ""
            // The SDK has no unregister: the record forgets the choices, the
            // running session keeps the providers it installed.
            capabilityProviderState.clearRememberedProviderKeys()
            directionsState.resetToDefaults()
            directionsState.refresh()
            mapLayersState.setTrafficEnabled(false, remember = false)
            pendingRememberedDeviceLocationStart = false
            remembersDeviceLocationEnabled = false
            if (isDeviceLocationActive) {
                stopDeviceLocation(remember = false)
            }
            refreshMapLayersState()
        } finally {
            isRestoringRememberedChoices = false
        }
        rememberedChoicesStore.clear()
    }

    // -------------------------------------------------------------------
    // Settings surface sizing (PRD.md §6 item 67)
    // -------------------------------------------------------------------
    /** The viewport height the settings panel is capped against, in pixels. */
    var settingsViewportHeightPixels: Int by mutableStateOf(0)
        private set

    /** The measured height of the open settings panel, in pixels. */
    var settingsSheetHeightPixels: Int by mutableStateOf(0)
        private set

    /** The settings content's scrollable excess, in pixels (0 when it fits). */
    var settingsContentMaximumScrollPixels: Int by mutableStateOf(0)
        private set

    /** The settings content's current scroll offset, in pixels. */
    var settingsContentScrollOffsetPixels: Int by mutableStateOf(0)
        private set

    /** Records the settings panel's own layout facts for the UI suite. */
    fun noteSettingsSheetMetrics(
        viewportHeightPixels: Int,
        sheetHeightPixels: Int,
        maximumScrollPixels: Int,
        scrollOffsetPixels: Int,
    ) {
        settingsViewportHeightPixels = viewportHeightPixels
        settingsSheetHeightPixels = sheetHeightPixels
        settingsContentMaximumScrollPixels = maximumScrollPixels
        settingsContentScrollOffsetPixels = scrollOffsetPixels
    }


    // -------------------------------------------------------------------
    // Readiness ladder (§6.1) and the error report (§6.3)
    // -------------------------------------------------------------------

    /**
     * The readiness ladder and the error report, one domain
     * (docs/ReferenceAppUX.md §6.1, §6.3). The state type owns every transition
     * and the contract-error wording; the application only feeds it observed
     * events and renders its readbacks.
     */
    private val readinessReport = ReadinessReportState(
        sdkVersion = sdkVersion,
        isProviderKeyless = { isProviderKeyless },
    )

    /** The ladder's current state. */
    val readinessState: ReadinessState
        get() = readinessReport.readinessState

    /** Verbatim failure detail backing the degraded/failed states. */
    val readinessDetail: String?
        get() = readinessReport.readinessDetail

    /** The provider-echoing error report, or null after a successful action. */
    val errorReport: SdkExceptionReport?
        get() = readinessReport.errorReport

    /** The readiness chip's short ladder line (§6.1's four states, text form). */
    val readinessChipText: String
        get() = readinessReport.readinessChipText

    private fun reportFailure(exception: SdkException) =
        readinessReport.reportSdkFailure(exception)

    private fun clearErrorReport() = readinessReport.clearErrorReport()

    // -------------------------------------------------------------------
    // Home sheet states (docs/ReferenceAppUX.md §1/§4.2)
    // -------------------------------------------------------------------
    /** The home sheet's three states: peek, half, full. */
    enum class HomeSheetState { PEEK, HALF, FULL }

    /** Which content the sheet carries: the home content or the search view. */
    enum class SheetContentMode { HOME, SEARCH }

    var homeSheetState: HomeSheetState by mutableStateOf(HomeSheetState.PEEK)
        private set

    var sheetContentMode: SheetContentMode by mutableStateOf(SheetContentMode.HOME)
        private set

    /** The diagnostics sheet behind the readiness chip (§4.1), when open. */
    var isDiagnosticsOpen: Boolean by mutableStateOf(false)
        private set

    /** The developer console sheet opened from the floating control. */
    var isProviderConsoleOpen: Boolean by mutableStateOf(false)
        private set

    /** The console's level threshold; Info (the widest) is the default. */
    var providerConsoleLevelFilter: ProviderConsoleLevelFilter by mutableStateOf(
        ProviderConsoleLevelFilter.INFO,
    )
        private set

    /** The event whose typed detail view the console shows, or null for the list. */
    var selectedProviderConsoleEntry: ProviderConsoleEntry? by mutableStateOf(null)
        private set

    /**
     * The serving traffic-flow cache's counters, read when the console opens
     * and refreshed while it stays open. Null while no traffic provider is
     * registered or the provider owns its freshness policy.
     */
    var providerConsoleTrafficCacheStatistics: TrafficFlowCacheStatistics? by mutableStateOf(null)
        private set

    /**
     * The installed tile payload cache's exact counters, read when the console
     * opens and refreshed while it stays open. Null while no tile cache is
     * installed (a keyless session).
     */
    var providerConsoleTileCacheStatistics: TileCacheStatistics? by mutableStateOf(null)
        private set

    /** The display name of the provider serving traffic, when the registry names one. */
    val providerConsoleTrafficProviderDisplayName: String?
        get() = (mapLayersState.trafficAvailability as? MapLayerTrafficAvailability.Available)
            ?.providerKey
            ?.let { providerKey -> providerDisplayNamesByProviderKey[providerKey] ?: providerKey }

    /**
     * The floating console control's dot: a Warning or Error arrived since the
     * console last showed the stream. The stream stays opt-in, so the dot
     * reports events from the bounded stream once the console or the
     * diagnostics pane has engaged it.
     */
    val hasUnseenProviderConsoleAlert: Boolean
        get() = !isProviderConsoleOpen && diagnosticEventListener.hasUnseenSevereEvent

    /** Whether the diagnostics events were engaged for this session. */
    private var isDiagnosticsEventStreamEngaged: Boolean = false

    /** Whether the pane's explicit debug switch already lowered the SDK gate. */
    private var isDiagnosticsDebugSubscriptionEngaged: Boolean = false

    /** The severity the pane's one stream was opened at, for the UI suite. */
    var diagnosticsSubscriptionSeverity: SdkLogSeverity? by mutableStateOf(null)
        private set

    /** The pane's explicit Show debug events switch (per session). */
    var isDiagnosticsDebugDetailEnabled: Boolean by mutableStateOf(false)
        private set

    /** Whether the pane ever opened and started its bounded stream. */
    val hasEngagedDiagnosticsEventStream: Boolean
        get() = isDiagnosticsEventStreamEngaged

    /** The coalescing loop that publishes the pane's pending window. */
    private var diagnosticsRefreshJob: Job? = null

    fun openDiagnostics() {
        if (isProviderConsoleOpen) {
            dismissProviderConsole()
        }
        isDiagnosticsOpen = true
        engageDiagnosticsEventStream()
        startDiagnosticsRefreshLoop()
    }

    fun dismissDiagnostics() {
        isDiagnosticsOpen = false
        stopDiagnosticsRefreshIfNoSurfaceOpen()
    }

    /**
     * Opens the developer console: engages the same bounded diagnostics stream
     * the diagnostics pane uses (first open only), flushes the backlog, reads
     * the serving traffic cache and starts the coalescing refresh loop. The
     * Warning/Error dot is marked seen as the console shows the stream.
     */
    fun openProviderConsole() {
        if (isDiagnosticsOpen) {
            dismissDiagnostics()
        }
        isProviderConsoleOpen = true
        selectedProviderConsoleEntry = null
        diagnosticEventListener.markSevereEventsSeen()
        engageDiagnosticsEventStream()
        refreshProviderConsoleCacheStatistics()
        startDiagnosticsRefreshLoop()
    }

    fun dismissProviderConsole() {
        isProviderConsoleOpen = false
        selectedProviderConsoleEntry = null
        diagnosticEventListener.markSevereEventsSeen()
        stopDiagnosticsRefreshIfNoSurfaceOpen()
    }

    /** Applies one of the three level thresholds and returns to the list. */
    fun updateProviderConsoleLevelFilter(levelFilter: ProviderConsoleLevelFilter) {
        providerConsoleLevelFilter = levelFilter
        selectedProviderConsoleEntry = null
    }

    /** Opens the typed detail view for one event. */
    fun selectProviderConsoleEntry(entry: ProviderConsoleEntry) {
        selectedProviderConsoleEntry = entry
    }

    /** Returns from the typed detail view to the filtered stream. */
    fun closeProviderConsoleDetail() {
        selectedProviderConsoleEntry = null
    }

    /**
     * Reads the serving traffic-flow cache and the installed tile payload
     * cache for the console's cache rows. A read failure leaves the row in its
     * honest unavailable state; neither read issues a provider call.
     */
    fun refreshProviderConsoleCacheStatistics() {
        providerConsoleTrafficCacheStatistics = try {
            sdkFacade.nativeSdk.trafficFlowCacheStatistics()
        } catch (diagnosticsException: Exception) {
            if (diagnosticsException is CancellationException) {
                throw diagnosticsException
            }
            null
        }
        providerConsoleTileCacheStatistics = try {
            sdkFacade.nativeSdk.tileCacheStatistics()
        } catch (diagnosticsException: Exception) {
            if (diagnosticsException is CancellationException) {
                throw diagnosticsException
            }
            null
        }
    }

    /** Cancels the shared publish loop once neither diagnostics surface is open. */
    private fun stopDiagnosticsRefreshIfNoSurfaceOpen() {
        if (!isDiagnosticsOpen && !isProviderConsoleOpen) {
            diagnosticsRefreshJob?.cancel()
            diagnosticsRefreshJob = null
        }
    }

    /**
     * Opens the one bounded stream at Info (the first open only) and flushes
     * the SDK ring's recent backlog. Nothing subscribes before this call: the
     * application is silent until the visitor opens the diagnostics pane or
     * the developer console.
     */
    private fun engageDiagnosticsEventStream() {
        if (!isDiagnosticsEventStreamEngaged) {
            isDiagnosticsEventStreamEngaged = true
            try {
                sdkFacade.nativeSdk.subscribeToDiagnosticEvents(
                    diagnosticEventListener,
                    SdkLogSeverity.INFO,
                )
                diagnosticsSubscriptionSeverity = SdkLogSeverity.INFO
                val flushedCount = sdkFacade.nativeSdk.flushRecentDiagnosticEvents(
                    diagnosticEventListener,
                    SdkLogSeverity.INFO,
                    ReferenceApplicationConstants.DIAGNOSTICS_BACKLOG_FLUSH_EVENT_LIMIT,
                )
                diagnosticEventListener.noteBacklogFlushed(flushedCount)
            } catch (diagnosticsException: Exception) {
                if (diagnosticsException is CancellationException) {
                    throw diagnosticsException
                }
                reportFailureOf(diagnosticsException)
            }
        }
        // The backlog and any early events publish immediately; later events
        // coalesce into the 500 ms refresh loop below.
        diagnosticEventListener.publishPendingEvents()
    }

    /**
     * Lowers the pane's stream to Debug when the explicit switch turns on.
     * The SDK gate never raises back, so turning the switch off only changes
     * the pane's presentation; the note states that the detail stays on until
     * the app restarts.
     */
    fun updateDiagnosticsDebugDetailEnabled(isEnabled: Boolean) {
        if (isEnabled && !isDiagnosticsDebugSubscriptionEngaged) {
            try {
                sdkFacade.nativeSdk.subscribeToDiagnosticEvents(
                    diagnosticEventListener,
                    SdkLogSeverity.DEBUG,
                )
                isDiagnosticsDebugSubscriptionEngaged = true
                diagnosticsSubscriptionSeverity = SdkLogSeverity.DEBUG
            } catch (diagnosticsException: Exception) {
                if (diagnosticsException is CancellationException) {
                    throw diagnosticsException
                }
                reportFailureOf(diagnosticsException)
                return
            }
        }
        isDiagnosticsDebugDetailEnabled = isEnabled
    }

    /**
     * The shared Clear action: empties the display window, its backlog count
     * and the console's unseen-alert dot, and returns to the list.
     */
    fun clearDiagnosticsEvents() {
        diagnosticEventListener.clear()
        selectedProviderConsoleEntry = null
    }

    /**
     * Publishes the listener's coalesced events every refresh cadence while
     * either diagnostics surface is open. The stream itself stays bounded;
     * this is only the readouts' display cadence. While the console is open it
     * also refreshes the traffic-cache row and keeps the floating dot cleared
     * for events the console has shown.
     */
    private fun startDiagnosticsRefreshLoop() {
        diagnosticsRefreshJob?.cancel()
        diagnosticsRefreshJob = coroutineScope.launch {
            while (isActive) {
                delay(ReferenceApplicationConstants.DIAGNOSTICS_REFRESH_CADENCE_MILLISECONDS)
                diagnosticEventListener.publishPendingEvents()
                if (isProviderConsoleOpen) {
                    refreshProviderConsoleCacheStatistics()
                    diagnosticEventListener.markSevereEventsSeen()
                }
            }
        }
    }

    // -------------------------------------------------------------------
    // Provider credentials and the settings surface (§6.5)
    // -------------------------------------------------------------------

    /**
     * The registry's provider rows: one settings row per descriptor. A
     * keyless descriptor (blank credential label) renders no row, so a
     * provider registered elsewhere never paints an empty settings field here.
     */
    val providerCredentialDescriptors: List<ProviderCredentialDescriptor>
        get() = providerRegistry.providers
            .filter { descriptor -> descriptor.credentialLabel.isNotBlank() }

    // -------------------------------------------------------------------
    // Base map (M9's native base-map switch)
    // -------------------------------------------------------------------

    /**
     * The base maps the settings surface offers, in registry order: every
     * descriptor that either publishes a `{z}/{x}/{y}` raster URL template
     * the SDK serves itself, or carries a credential row (the provider's
     * raster slot comes from its registration, so it is offered before the
     * key is applied and says plainly that it needs one).
     */
    val baseMapChoices: List<BaseMapChoice>
        get() = providerRegistry.providers.mapNotNull { descriptor ->
            when {
                descriptor.isCredentiallessBaseMapAvailableOnAndroid ->
                    BaseMapChoice(descriptor, isServable = true)
                descriptor.isRegistrationAvailableOnAndroid &&
                    descriptor.credentialLabel.isNotBlank() ->
                    BaseMapChoice(
                        descriptor,
                        isServable = descriptor.providerKey in appliedProviderKeys,
                    )
                else -> null
            }
        }

    /**
     * The styled vector base maps the settings surface offers (M22), in
     * registry order and style order: every descriptor publishing a vector
     * tileset URL template plus a `vector_styles` list contributes one row per
     * style, and a provider whose styles come from documents it publishes
     * contributes its rows too. A credentialed family whose key is not applied
     * is listed with `isServable = false`, so its row states
     * `Needs <credential label>` instead of disappearing. The SDK serves these
     * through the shipped `MapView`'s vector base members.
     */
    val vectorBaseMapChoices: List<VectorBaseMapChoice>
        get() {
            // Keyless families first (the demo's default base among them),
            // then the credential-gated families, each in registry order.
            val keylessDescriptors = providerRegistry.providers
                .filter { descriptor -> descriptor.offersVectorBase }
            val hostedDescriptors = providerRegistry.providers
                .filter { descriptor ->
                    !descriptor.offersVectorBase && descriptor.offersHostedVectorBase
                }
            return (keylessDescriptors + hostedDescriptors).flatMap { descriptor ->
                val isServable = descriptor.offersVectorBase ||
                    descriptor.providerKey in appliedProviderKeys
                descriptor.vectorStyles.map { style ->
                    VectorBaseMapChoice(descriptor, style, isServable)
                }
            }
        }

    /**
     * The styled vector base the user picked this session, while the registry
     * still offers it; null when the raster base serves (the default). The
     * request is recorded when the SDK accepts the install, which may be one
     * frame before the source actually serves (a selection made while a draw
     * is in flight is queued); [activeVectorBase] is the SDK's own readback of
     * what serves.
     */
    var selectedVectorBaseChoiceKey: String? by mutableStateOf(null)
        private set

    /**
     * The registry's declared keyless styled base (owner ruling 2026-10-31,
     * PRD §6 item 18), resolved among the offered vector choices; null when
     * the registry declares none or the declaration names a provider/style
     * this build does not offer (the keyless start then keeps the
     * credential-less raster source).
     */
    val keylessVectorBaseChoice: VectorBaseMapChoice?
        get() {
            val declared = providerRegistry.keylessBaseMap ?: return null
            return vectorBaseMapChoices.firstOrNull { choice ->
                choice.descriptor.providerKey == declared.providerKey &&
                    choice.style.styleId == declared.styleId
            }
        }

    /**
     * The keyless styled base still awaiting the shipped map view: the
     * startup registers the credential-less raster source first (the surface
     * attaches through the raster provider contract), and the styled base is
     * installed as soon as the view exists. Cleared by an explicit base-map
     * choice or a credential application, which both win over the default.
     */
    private var pendingKeylessVectorBaseChoiceKey: String? = null

    /**
     * The remembered styled base still awaiting the shipped map view. It
     * wins over the keyless default and over nothing else: an explicit
     * selection clears it.
     */
    private var pendingRememberedVectorBaseChoiceKey: String? = null

    /**
     * Installs the remembered styled base once the shipped map view exists.
     * The install is a restore, not a new choice, so it never rewrites the
     * record.
     */
    private fun applyPendingRememberedVectorBase() {
        val pendingChoiceKey = pendingRememberedVectorBaseChoiceKey ?: return
        if (mapView == null) {
            return
        }
        pendingRememberedVectorBaseChoiceKey = null
        if (vectorBaseMapChoices.none { choice -> choice.choiceKey == pendingChoiceKey }) {
            return
        }
        isApplyingAutomaticBaseMap = true
        try {
            selectVectorBase(pendingChoiceKey)
        } finally {
            isApplyingAutomaticBaseMap = false
        }
    }

    /**
     * The styled vector base the SDK reports installed and serving, or null
     * while the raster base serves. Reading the SDK's status rather than the
     * request keeps the settings row truthful: a queued install the SDK later
     * refuses never leaves the row naming a base that is not on screen.
     */
    val activeVectorBase: VectorBaseMapChoice?
        get() {
            val status = vectorBaseStatus ?: return null
            if (!status.isConfigured) {
                return null
            }
            return vectorBaseMapChoices.firstOrNull { choice ->
                choice.descriptor.providerKey == status.sourceName &&
                    choice.style.styleId == status.styleId
            }
        }

    /**
     * The note the base-map section carries while a styled vector base is
     * active. The committed styles paint fills, lines and circles, their place
     * labels, and the route-reference shield badges each style declares; the
     * note states that surface rather than promising a feature a style omits.
     */
    val vectorBaseLabelsNote: String?
        get() = if (activeVectorBase != null) {
            ReferenceApplicationConstants.BASE_MAP_VECTOR_LABELS_NOTE_TEXT
        } else {
            null
        }

    /**
     * The last vector base status the view reported (M22's honest readback):
     * the configured style, its attribution and the last frame's tile counts.
     */
    var vectorBaseStatus: VectorTileSourceStatus? by mutableStateOf(null)
        private set

    /**
     * The base map actually serving the map: the user's selection while it
     * can still serve, otherwise the first choice that can. Null when nothing
     * can serve — a keyless session whose registry declares no
     * credential-less source — and the band then claims no provider at all.
     */
    val activeBaseMap: ProviderCredentialDescriptor?
        get() {
            val servableChoices = baseMapChoices.filter { choice -> choice.isServable }
            val selectedProviderKey = selectedBaseMapProviderKey
            if (selectedProviderKey != null) {
                servableChoices
                    .firstOrNull { choice -> choice.providerKey == selectedProviderKey }
                    ?.let { choice -> return choice.descriptor }
            }
            return servableChoices.firstOrNull()?.descriptor
        }

    /** The base map the user picked this session; null follows [activeBaseMap]'s default. */
    var selectedBaseMapProviderKey: String? by mutableStateOf(null)
        private set

    /**
     * The attribution line of the base map actually serving; blank claims
     * nothing. A styled vector base carries its own style's design line (the
     * registry's `vector_styles` attribution), which replaces the provider's
     * general line while that base serves.
     */
    val attributionText: String
        get() {
            val baseAttributionText = activeVectorBase?.attributionText
                ?: activeBaseMap?.attributionText.orEmpty()
            // The traffic layer's serving descriptor appends its own line for
            // as long as the layer draws (docs/ReferenceAppUX.md §4.2).
            val trafficAttributionText = mapLayersState.trafficAttributionText
            return if (
                trafficAttributionText.isNullOrBlank() ||
                trafficAttributionText == baseAttributionText
            ) {
                baseAttributionText
            } else if (baseAttributionText.isBlank()) {
                trafficAttributionText
            } else {
                "$baseAttributionText · $trafficAttributionText"
            }
        }

    /**
     * The terms notes the attribution panel lists under the attribution
     * line: the serving base map's (vector base first) terms note, or empty
     * when the registry declares none (the panel then renders the attribution
     * line alone rather than a guess).
     */
    val attributionTermsNotes: List<String>
        get() = (activeVectorBase?.descriptor ?: activeBaseMap)
            ?.termsNote
            ?.takeIf { termsNote -> termsNote.isNotBlank() }
            ?.let { termsNote -> listOf(termsNote) }
            .orEmpty()

    /**
     * The base-map row's act: registers the chosen source onto the SDK's
     * raster-tile slot — a credential-less descriptor through the
     * provider-neutral URL-template registration, a credentialed one by
     * re-applying its credential — then asks the map view to re-resolve its
     * provider so the switch shows on the next frame.
     *
     * A refusal reports verbatim and leaves the previously serving base map
     * in place: the SDK validates the whole declaration before the slot
     * moves, and the selection only records a source the SDK accepted.
     *
     * A row whose provider has not been credentialed yet is not selectable:
     * the act points at that provider's key field instead, so the reason is a
     * next step rather than a dead control.
     */
    fun selectBaseMap(providerKey: String) {
        val choice = baseMapChoices.firstOrNull { it.providerKey == providerKey } ?: return
        if (!choice.isServable) {
            credentialStatusDetails[providerKey] =
                ReferenceApplicationConstants.PROVIDER_CREDENTIAL_ENTER_HINT_TEXT
            return
        }
        val descriptor = choice.descriptor
        try {
            if (descriptor.rasterUrlTemplate.isNotBlank()) {
                sdkFacade.applyUrlTemplateRasterTileSource(
                    UrlTemplateRasterTileSource(
                        urlTemplate = descriptor.rasterUrlTemplate,
                        sourceName = descriptor.providerKey,
                        minimumZoomLevel = BASE_MAP_MINIMUM_ZOOM_LEVEL,
                        maximumZoomLevel = descriptor.maximumZoomLevel.toUByte(),
                        imageFormat = RasterImageFormat.PNG,
                        tileResolution = RasterTileResolution.STANDARD,
                        userAgent = baseMapUserAgent(),
                    ),
                )
            } else {
                sdkFacade.restoreCredentialedBaseMap(descriptor.providerKey)
                // Restoring the base map replaces only the raster-tile slot;
                // the tile-cache registration then re-registers the whole
                // credentialed set with the cached raster-tile slot.
                registerTileCacheAfterCredential()
            }
        } catch (selectionException: Exception) {
            if (selectionException is CancellationException) {
                throw selectionException
            }
            credentialStatusDetails[providerKey] = failureDetailTextOf(selectionException)
            reportFailureOf(selectionException)
            return
        }
        if (descriptor.rasterUrlTemplate.isNotBlank()) {
            // A credential-less source serves uncached: registering it dropped
            // the installation, whose terms belonged to the previous provider.
            // The offline card must say so rather than keep the old promise.
            tileCacheRegisteredPath = null
            tileCacheBudgetBytes = null
            tileCacheFailureReason = ReferenceApplicationConstants.BASE_MAP_UNCACHED_NOTE_TEXT
        }
        selectedBaseMapProviderKey = descriptor.providerKey
        // A raster selection replaces a styled vector base: the vector base
        // replaces the raster layer while installed, so leaving it in force
        // would keep painting the styled base instead of the newly selected
        // imagery.
        mapView?.clearVectorTileSource()
        selectedVectorBaseChoiceKey = null
        vectorBaseStatus = null
        // An explicit raster choice wins over the keyless styled base; the
        // startup path sets the pending default again after its own call.
        pendingKeylessVectorBaseChoiceKey = null
        if (!isApplyingAutomaticBaseMap) {
            pendingRememberedVectorBaseChoiceKey = null
            rememberedBaseMapProviderKey = descriptor.providerKey
            rememberedBaseMapStyleId = null
        }
        clearErrorReport()
        // The attached view holds the provider it resolved and keeps its
        // tiles by coordinate: without this the previous imagery would stay
        // on screen until the camera reached new tiles.
        mapView?.reloadTileSource()
        readinessReport.noteLoading()
        persistRememberedChoices()
        refreshMapLayersState()
    }

    /**
     * Opens the map on the registry's keyless base map when nothing else is
     * serving one.
     *
     * A keyless run (a first launch, any build without a credential) would
     * otherwise draw an empty surface. The choice is
     * registry-driven and provider-neutral: the first descriptor that declares
     * a credential-less raster template and is servable on this platform. A
     * credentialed or already-chosen base map always wins, so this never
     * overrides a user's selection.
     */
    fun selectKeylessBaseMapWhenUncredentialed() {
        if (selectedBaseMapProviderKey != null) {
            return
        }
        // A credentialed session already has a serving raster slot: its
        // registration installed the provider's tiles and, with them, the
        // tile cache. Selecting the credential-less source here would replace
        // that registration and silently drop the cache.
        if (appliedProviderKeys.isNotEmpty()) {
            return
        }
        val keylessChoice = baseMapChoices.firstOrNull { choice ->
            choice.isServable && choice.descriptor.rasterUrlTemplate.isNotBlank()
        } ?: return
        // The keyless start is the application's default, not a user choice:
        // it must not overwrite a remembered base map in the record.
        isApplyingAutomaticBaseMap = true
        try {
            selectBaseMap(keylessChoice.providerKey)
        } finally {
            isApplyingAutomaticBaseMap = false
        }
        // The registry's styled vector base opens the map (owner ruling
        // 2026-10-31, PRD §6 item 18): the credential-less raster source
        // above stays registered so the surface can attach, and the styled
        // base paints over it once the view exists. A remembered styled base
        // waits for the same view and wins over this default.
        if (pendingRememberedVectorBaseChoiceKey == null) {
            pendingKeylessVectorBaseChoiceKey = keylessVectorBaseChoice?.choiceKey
        }
        applyPendingRememberedVectorBase()
        applyPendingKeylessVectorBase()
    }

    /**
     * The base-map row's act for a styled vector base (M22): installs the
     * registry descriptor's vector tileset with the chosen committed style on
     * the shipped `MapView`, so every following frame draws the styled base in
     * place of the raster tiles and beneath the app's own overlays.
     *
     * The install is validated by the SDK; a refusal reports verbatim and
     * leaves the previously serving base in place, and the selection records
     * only a base the SDK accepted. The raster base stays registered but stops
     * drawing, so switching back to a raster row needs no refetch of the
     * provider registration.
     */
    fun selectVectorBase(choiceKey: String) {
        val choice = vectorBaseMapChoices.firstOrNull { it.choiceKey == choiceKey } ?: return
        val shippedMapView = mapView
        if (shippedMapView == null) {
            credentialStatusDetails[choice.descriptor.providerKey] =
                ReferenceApplicationConstants.BASE_MAP_VIEW_NOT_READY_TEXT
            return
        }
        if (choice.style.documentUrl.isNotBlank()) {
            // The provider publishes the style: the install fetches it through
            // the provider's authenticated client, so the app never handles
            // the credential.
            coroutineScope.launch {
                val accepted = shippedMapView.setProviderVectorTileSourceFromDocument(
                    ProviderVectorTileSource(
                        providerKey = choice.descriptor.providerKey,
                        styleId = choice.style.styleId,
                    ),
                    choice.style.documentUrl,
                )
                if (accepted) {
                    recordVectorBaseSelection(shippedMapView, choice)
                }
            }
            return
        }
        val accepted = shippedMapView.setVectorTileSource(
            UrlTemplateVectorTileSource(
                urlTemplate = choice.descriptor.vectorUrlTemplate,
                sourceName = choice.descriptor.providerKey,
                attributionText = choice.attributionText,
                styleId = choice.style.styleId,
                minimumZoomLevel = BASE_MAP_MINIMUM_ZOOM_LEVEL,
                maximumZoomLevel = choice.descriptor.maximumZoomLevel.toUByte(),
            ),
        )
        if (!accepted) {
            // The view reported the refusal through its failure listener; the
            // selection keeps the previous base, so this call changes nothing.
            return
        }
        recordVectorBaseSelection(shippedMapView, choice)
    }

    /** Records one accepted styled-vector-base install on the app's state. */
    private fun recordVectorBaseSelection(
        shippedMapView: ShippedMapView,
        choice: VectorBaseMapChoice,
    ) {
        selectedVectorBaseChoiceKey = choice.choiceKey
        vectorBaseStatus = shippedMapView.vectorTileSourceStatus()
        if (!isApplyingAutomaticBaseMap) {
            pendingRememberedVectorBaseChoiceKey = null
            rememberedBaseMapProviderKey = choice.descriptor.providerKey
            rememberedBaseMapStyleId = choice.style.styleId
        }
        clearErrorReport()
        readinessReport.noteLoading()
        persistRememberedChoices()
        refreshMapLayersState()
    }

    /**
     * The `User-Agent` a credential-less base map identifies this app with,
     * formatted from the shared registry's app-identity template (a public
     * tile service's usage policy requires a client that names itself and a
     * way to reach its operator). The SDK's compiled version fills the
     * template's version slot, so the identity never drifts from the build.
     */
    private fun baseMapUserAgent(): String {
        val userAgentTemplate = providerRegistry.referenceAppUserAgentTemplate
        return if (userAgentTemplate.isBlank()) {
            // No template in the artifact: send no header rather than invent
            // an identity the registry does not declare.
            ""
        } else {
            userAgentTemplate.format(sdkVersion)
        }
    }

    /**
     * The provider key an automation-injected credential applies to when the
     * launch intent names none: the only descriptor with a credential row
     * whose registration arm exists on Android. Null when the registry
     * declares zero or several such providers — the injection then has no
     * unambiguous target.
     */
    fun defaultRuntimeInjectionProviderKey(): String? =
        providerCredentialDescriptors
            .singleOrNull { descriptor -> descriptor.isRegistrationAvailableOnAndroid }
            ?.providerKey

    /** Whether the settings surface (the gear's act) is open. */
    var isSettingsOpen: Boolean by mutableStateOf(false)
        private set

    /** The masked credential field's draft text per provider key (never echoed). */
    val credentialDraftTexts: SnapshotStateMap<String, String> = mutableStateMapOf()

    /** The provider keys whose registration succeeded in this session. */
    val appliedProviderKeys: SnapshotStateList<String> = mutableStateListOf()

    /** True while no provider credential has been applied in this session. */
    val isProviderKeyless: Boolean
        get() = appliedProviderKeys.isEmpty()

    /**
     * Whether a failure is the provider-not-configured one expected while
     * keyless (§6.5's neutral prompt), resolved by the readiness state's own
     * rule.
     */
    private fun isExpectedKeylessProviderFailure(failure: Throwable): Boolean =
        readinessReport.isExpectedKeylessProviderFailure(failure)

    /**
     * Per-provider status detail: a verbatim refusal, the honest clear note,
     * or the "enter a key" nudge. Absent when the applied/not-applied template
     * is the whole truth.
     */
    val credentialStatusDetails: SnapshotStateMap<String, String> = mutableStateMapOf()

    /** "Remember on this device" (default off, §6.5). */
    var isRememberCredentialEnabled: Boolean by mutableStateOf(false)
        private set

    /** The honest secure-store failure reason, or null while storage works. */
    var credentialStoreUnavailableReason: String? by mutableStateOf(null)
        private set

    fun openSettings() {
        // The readback is the surface's source of truth: a base-map switch or
        // a registration made since the last refresh shows in its row.
        capabilityProviderState.refresh()
        isSettingsOpen = true
    }

    fun closeSettings() {
        isSettingsOpen = false
    }

    /** Updates one provider's masked draft as the user types. */
    fun updateCredentialDraft(providerKey: String, draftText: String) {
        credentialDraftTexts[providerKey] = draftText
    }

    /**
     * The row's Apply act: registers the provider named by the descriptor with
     * the drafted credential. The SDK registration and any opted-in secure-store
     * write run on a background dispatcher, so neither the native call nor the
     * keystore blocks the main thread; the row's status and the composed state
     * update back on the main thread. A refusal is shown verbatim in the row's
     * status line and the error strip; success flips the applied status,
     * registers the offline tile cache (§4.1's card), and — only with the
     * opt-in toggle on — persists the value in the platform secure store.
     */
    fun applyCredential(providerKey: String) {
        val descriptor = providerCredentialDescriptors.firstOrNull { it.providerKey == providerKey }
            ?: return
        if (!descriptor.isRegistrationAvailableOnAndroid) {
            credentialStatusDetails[providerKey] =
                ReferenceApplicationConstants.PROVIDER_CREDENTIAL_UNAVAILABLE_TEMPLATE
                    .format(descriptor.displayName)
            return
        }
        val draftedCredential = credentialDraftTexts[providerKey].orEmpty()
        if (draftedCredential.isBlank()) {
            credentialStatusDetails[providerKey] =
                ReferenceApplicationConstants.PROVIDER_CREDENTIAL_ENTER_HINT_TEXT
            return
        }
        // The opt-in is read on the main thread and carried to the worker, so
        // a concurrent toggle change cannot race the write.
        val shouldRememberCredential = isRememberCredentialEnabled
        coroutineScope.launch {
            val registrationFailure = withContext(Dispatchers.IO) {
                try {
                    registerProviderCredential(providerKey, draftedCredential)
                    null
                } catch (cancellation: CancellationException) {
                    throw cancellation
                } catch (failure: Throwable) {
                    failure
                }
            }
            if (registrationFailure != null) {
                credentialStatusDetails[providerKey] = failureDetailTextOf(registrationFailure)
                reportFailureOf(registrationFailure)
                return@launch
            }
            // The masked field holds nothing after a successful apply: the
            // value is never echoed back (docs/ReferenceAppUX.md §6.5).
            credentialDraftTexts.remove(providerKey)
            completeSuccessfulCredentialApplication(providerKey)
            val saveOutcome = withContext(Dispatchers.IO) {
                persistCredentialOnWorker(providerKey, draftedCredential, shouldRememberCredential)
            }
            applyCredentialPersistenceOutcome(saveOutcome)
        }
    }

    /**
     * The row's Clear act: removes the remembered copy and the draft, and
     * states honestly that the running session keeps its providers — the SDK
     * registry has no unregister operation (§6.5). The secure-store write
     * runs on a background dispatcher so the keystore never blocks the main
     * thread; the visible state updates immediately.
     */
    fun clearCredential(providerKey: String) {
        credentialDraftTexts.remove(providerKey)
        if (providerKey in appliedProviderKeys) {
            // The registry has no unregister: the session stays credentialed
            // until the process restarts — stated, never hidden (§6.5).
            credentialStatusDetails[providerKey] =
                ReferenceApplicationConstants.PROVIDER_CREDENTIAL_CLEAR_RESTART_NOTE_TEXT
        } else {
            credentialStatusDetails.remove(providerKey)
        }
        coroutineScope.launch {
            withContext(Dispatchers.IO) { providerCredentialStore.clear(providerKey) }
        }
    }

    /**
     * The remember toggle's act (§6.5, default off). Turning it off removes
     * every stored copy immediately so the next start is keyless; the running
     * session keeps what it applied. The store removals run on a background
     * dispatcher.
     */
    fun updateRememberCredentialPreference(isEnabled: Boolean) {
        isRememberCredentialEnabled = isEnabled
        if (!isEnabled) {
            val providerKeys = providerCredentialDescriptors.map { descriptor ->
                descriptor.providerKey
            }
            coroutineScope.launch {
                withContext(Dispatchers.IO) {
                    providerKeys.forEach { providerKey ->
                        providerCredentialStore.clear(providerKey)
                    }
                }
            }
        }
    }

    /**
     * Startup restore: loads each provider's securely stored credential (only
     * when the user opted in on a previous run — an absent entry is a keyless
     * start, not a failure) and applies it. A successful application registers
     * the offline tile cache; an unreadable store reports honestly and leaves
     * the session keyless.
     */
    fun restorePersistedCredentials() {
        providerCredentialDescriptors.forEach { descriptor ->
            when (val loadOutcome = providerCredentialStore.load(descriptor.providerKey)) {
                is ProviderCredentialLoadOutcome.Absent -> Unit
                is ProviderCredentialLoadOutcome.Unavailable -> {
                    credentialStoreUnavailableReason = loadOutcome.reason
                }
                is ProviderCredentialLoadOutcome.Loaded -> {
                    isRememberCredentialEnabled = true
                    applyCredentialValue(descriptor.providerKey, loadOutcome.credential)
                }
            }
        }
    }

    /**
     * The dock's credential line: the neutral keyless state (§6.5's exact
     * product wording) until a provider is applied, then the applied summary.
     * It never masquerades as a renderer or startup failure.
     */
    val credentialStatusLineText: String
        get() {
            val appliedDescriptors = providerCredentialDescriptors.filter { descriptor ->
                descriptor.providerKey in appliedProviderKeys
            }
            return if (appliedDescriptors.isEmpty()) {
                ReferenceApplicationConstants.KEYLESS_CREDENTIAL_STATUS_TEXT
            } else {
                appliedDescriptors.joinToString(separator = " · ") { descriptor ->
                    ReferenceApplicationConstants.PROVIDER_CREDENTIAL_APPLIED_TEMPLATE
                        .format(descriptor.displayName)
                }
            }
        }

    /** The row's status line: the applied/not-applied form or a more specific detail. */
    fun credentialStatusText(descriptor: ProviderCredentialDescriptor): String {
        if (!descriptor.isRegistrationAvailableOnAndroid) {
            return ReferenceApplicationConstants.PROVIDER_CREDENTIAL_UNAVAILABLE_TEMPLATE
                .format(descriptor.displayName)
        }
        credentialStatusDetails[descriptor.providerKey]?.let { return it }
        return if (descriptor.providerKey in appliedProviderKeys) {
            ReferenceApplicationConstants.PROVIDER_CREDENTIAL_APPLIED_TEMPLATE
                .format(descriptor.displayName)
        } else {
            ReferenceApplicationConstants.PROVIDER_CREDENTIAL_NOT_APPLIED_TEMPLATE
                .format(descriptor.displayName)
        }
    }

    /** Whether the provider's registration succeeded in this session. */
    fun isCredentialApplied(providerKey: String): Boolean = providerKey in appliedProviderKeys

    /**
     * Applies an automation-supplied credential (the launch-intent extra the
     * UI tests use, §6.5's runtime path): the value is applied through the
     * same public registration path as the settings surface, never persisted
     * by this method, and never logged. Returns whether the registration
     * succeeded; a refusal reports verbatim.
     */
    fun applyRuntimeInjectedCredential(providerKey: String, credential: String): Boolean =
        applyCredentialValue(providerKey, credential)

    /**
     * The one SDK registration call, with no state mutation: it runs on a
     * background dispatcher from the settings Apply act and directly from the
     * startup restore. A refusal (including a native linkage failure) throws
     * to the caller, which reports it.
     */
    private fun registerProviderCredential(providerKey: String, credential: String) {
        sdkFacade.applyProviderCredential(providerKey, credential)
    }

    /**
     * Applies one credential value through the facade and records the honest
     * outcome. Returns true on success; a refusal reports verbatim and never
     * clears a previously applied status.
     */
    private fun applyCredentialValue(providerKey: String, credential: String): Boolean {
        try {
            registerProviderCredential(providerKey, credential)
        } catch (cancellation: CancellationException) {
            throw cancellation
        } catch (credentialException: Throwable) {
            credentialStatusDetails[providerKey] = failureDetailTextOf(credentialException)
            reportFailureOf(credentialException)
            return false
        }
        completeSuccessfulCredentialApplication(providerKey)
        return true
    }

    /** The composed state updates one successful credential application makes. */
    private fun completeSuccessfulCredentialApplication(providerKey: String) {
        if (providerKey !in appliedProviderKeys) {
            appliedProviderKeys.add(providerKey)
        }
        // The capability pickers now offer the provider this credential
        // unblocks instead of repeating the credential it still needs.
        capabilityProviderState.refresh()
        // A credential can install the routing slot, so the Directions
        // control re-reads what the serving provider now honours.
        directionsState.refresh()
        // The traffic capability now has its credential: the switch becomes
        // available and a remembered traffic choice can apply.
        refreshMapLayersState()
        mapLayersState.restoreRememberedTrafficIfAvailable()
        // A credential wins over the keyless styled base: a launch-injected
        // key applies after the keyless startup, and its registration must
        // not end hidden under the default vector base.
        pendingKeylessVectorBaseChoiceKey = null
        credentialStatusDetails.remove(providerKey)
        clearErrorReport()
        // A credential applied after a keyless attach attempt must retry it:
        // the shipped view only re-attaches on an explicit retry.
        mapView?.retryAttach()
        readinessReport.noteLoading()
        // The offline-maps card's cache registration repeats the credential
        // and is only meaningful once one is installed (§4.1, §6.5).
        registerTileCacheAfterCredential()
    }

    /**
     * The secure-store half of one Apply, run on a background dispatcher. With
     * the opt-in off it removes any stale stored copy and answers null; with
     * it on it saves and answers the store's outcome. It reads no composed
     * state — the caller carries the opt-in value in.
     */
    private fun persistCredentialOnWorker(
        providerKey: String,
        credential: String,
        shouldRememberCredential: Boolean,
    ): ProviderCredentialSaveOutcome? {
        if (!shouldRememberCredential) {
            // Off means the value lives only in this process: remove any
            // stale stored copy from an earlier opted-in run.
            providerCredentialStore.clear(providerKey)
            return null
        }
        return providerCredentialStore.save(providerKey, credential)
    }

    /**
     * Applies a secure-store outcome back on the main thread. An unavailable
     * store is reported honestly and switches the toggle back off: the
     * session keeps the credential, the device does not.
     */
    private fun applyCredentialPersistenceOutcome(saveOutcome: ProviderCredentialSaveOutcome?) {
        when (saveOutcome) {
            null -> Unit
            is ProviderCredentialSaveOutcome.Stored -> credentialStoreUnavailableReason = null
            is ProviderCredentialSaveOutcome.Unavailable -> {
                credentialStoreUnavailableReason = saveOutcome.reason
                isRememberCredentialEnabled = false
            }
        }
    }

    /** The grabber's tap: cycle the sheet's states peek → half → full → half. */
    fun advanceSheetState() {
        homeSheetState = when (homeSheetState) {
            HomeSheetState.PEEK -> HomeSheetState.HALF
            HomeSheetState.HALF -> HomeSheetState.FULL
            HomeSheetState.FULL -> HomeSheetState.HALF
        }
        if (homeSheetState != HomeSheetState.PEEK) {
            directionsState.refresh()
        }
    }

    fun requestHomeSheetState(sheetState: HomeSheetState) {
        homeSheetState = sheetState
        if (sheetState != HomeSheetState.PEEK) {
            directionsState.refresh()
        }
    }

    /**
     * The search view's entry (§4.2): the peek's destination field opens it
     * armed for the destination, and the peek's origin entry opens it armed
     * for the origin, so either field is one tap from the home sheet — the
     * same pair of entries the React Native and iOS shells carry.
     */
    fun openPlanSearch(
        planSearchField: PlanSearchField = PlanSearchField.DESTINATION,
    ) {
        activePlanSearchField = planSearchField
        sheetContentMode = SheetContentMode.SEARCH
        homeSheetState = HomeSheetState.FULL
        // A fresh search view never carries the previous visit's chip failure.
        planFieldMyLocationStatusText = null
        // The Directions control is beside the plan action, so opening the
        // plan surface reads the serving provider's capabilities again: the
        // user may have applied a credential or switched the routing provider
        // since the sheet last opened.
        directionsState.refresh()
    }

    fun dismissPlanSearch() {
        sheetContentMode = SheetContentMode.HOME
        homeSheetState = HomeSheetState.HALF
    }

    /**
     * The platform back affordance collapses toward peek (§4.2): diagnostics
     * close first, the search view returns home, and any grown sheet state
     * steps down; a peek sheet stays.
     */
    fun collapseTowardPeek(): Boolean = when {
        isSettingsOpen -> {
            closeSettings()
            true
        }
        isDiagnosticsOpen -> {
            dismissDiagnostics()
            true
        }
        sheetContentMode == SheetContentMode.SEARCH -> {
            dismissPlanSearch()
            true
        }
        homeSheetState != HomeSheetState.PEEK -> {
            requestHomeSheetState(HomeSheetState.PEEK)
            true
        }
        else -> false
    }

    // -------------------------------------------------------------------
    // Camera state (the SDK owns the camera math; the app owns the values)
    // -------------------------------------------------------------------
    var centerLatitudeDegrees: Double by mutableStateOf(artifact.cameraPresets.first().latitudeDegrees)
    var centerLongitudeDegrees: Double by mutableStateOf(artifact.cameraPresets.first().longitudeDegrees)
    var zoomLevel: Double by mutableStateOf(artifact.cameraPresets.first().zoomLevel)
    var bearingDegrees: Double by mutableStateOf(artifact.cameraPresets.first().bearingDegrees)
    var pitchDegrees: Double by mutableStateOf(artifact.cameraPresets.first().pitchDegrees)

    /**
     * The confirmed single taps the shipped MapView reported this session
     * (§5.2: a double tap zooms and reports no click of its own). The
     * diagnostics sheet renders the count beside the camera readback, and the
     * device suite asserts one tap per confirmed click.
     */
    var mapClickCount: Int by mutableStateOf(0)
        private set

    /** Whether the camera sits in the control cluster's 3D plane (§4.2), or the 2D top-down. */
    var isThreeDCameraEnabled: Boolean by mutableStateOf(false)
        private set

    /** The selected camera preset's identifier; a direct gesture clears it. */
    var selectedCameraPresetIdentifier: String? by mutableStateOf(null)
        private set

    // --- Round 10: the follow camera (§5.3); the shipped MapView owns the
    // frame pump and the camera animation (M17) ---
    /** The follow-camera state machine (§5.3: engage / gesture-break / re-engage). */
    val followCamera = FollowCamera()

    /**
     * The generated camera object holds the phase in Rust, outside Compose's
     * snapshot system; this mirrors it whenever a transition runs so the
     * control cluster recomposes on a gesture break or a re-engage.
     */
    var followPhase: FollowPhase by mutableStateOf(FollowPhase.DISENGAGED)
        private set

    private fun syncFollowPhase() {
        followPhase = followCamera.phase()
    }

    /**
     * The scale bar's ground answer (§4.2, round 10): the shipped MapView
     * answers the camera's `ground_meters_per_screen_pixel` for the attached
     * viewport (a flat-center approximation at any nonzero pitch — the
     * contract documents that), cached for the control cluster's read-only use.
     */
    var currentGroundMetersPerScreenPixel: Double? by mutableStateOf(null)
        private set

    /**
     * The diagnostics sheet's camera readback (§4.1): the values the shipped
     * MapView read back from the SDK camera — no unprojection-based
     * approximation of the fitted center.
     */
    val cameraReadbackText: String
        get() = "Center %.5f , %.5f · zoom %.2f · bearing %.0f° · pitch %.0f°".format(
            centerLatitudeDegrees,
            centerLongitudeDegrees,
            zoomLevel,
            bearingDegrees,
            pitchDegrees,
        )

    /** The current camera values as the shipped MapView's plain pose. */
    fun currentCameraPose(): MapCameraPose = MapCameraPose(
        latitudeDegrees = centerLatitudeDegrees,
        longitudeDegrees = centerLongitudeDegrees,
        zoomLevel = zoomLevel,
        bearingDegrees = bearingDegrees,
        pitchDegrees = pitchDegrees,
    )

    /** Applies the current camera values immediately (no ease). */
    private fun pushCameraImmediately() {
        mapView?.setCamera(currentCameraPose())
    }

    /** Eases the camera to [targetPose] over the design token's move duration. */
    private fun animateCameraTo(targetPose: MapCameraPose) {
        mapView?.animateCamera(
            targetPose,
            ReferenceAppUXDimensions.EASED_CAMERA_MOVE_DURATION_MILLISECONDS,
        )
    }

    /** The pitch the 2D/3D plane state targets (§4.2's toggle; §5.3's 3D follow). */
    private fun cameraPitchTargetDegrees(): Double =
        if (isThreeDCameraEnabled) {
            ReferenceAppUXDimensions.NAVIGATION_FOLLOW_PITCH_3D_DEGREES
        } else {
            TWO_D_CAMERA_PITCH_DEGREES
        }

    /**
     * Adopts one camera pose the shipped MapView reported. A gesture-driven
     * change is one of §5.3's following-breaking gestures: while a trip is
     * presented it breaks the follow camera immediately (the my-location
     * control re-engages it; a later fix does not).
     */
    private fun applyCameraPoseFromMapView(pose: MapCameraPose, isGestureDriven: Boolean) {
        centerLatitudeDegrees = pose.latitudeDegrees
        centerLongitudeDegrees = pose.longitudeDegrees
        zoomLevel = pose.zoomLevel
        bearingDegrees = pose.bearingDegrees
        pitchDegrees = pose.pitchDegrees
        if (isGestureDriven && isNavigationInTrip) {
            followCamera.breakOnGesture()
            syncFollowPhase()
        }
        if (isGestureDriven && selectedCameraPresetIdentifier != null) {
            // A direct camera gesture is not a preset: the remembered preset
            // must not snap the camera back on the next launch.
            selectedCameraPresetIdentifier = null
            persistRememberedChoices()
        }
        refreshGroundMetersPerScreenPixel()
        // Every camera path reaches the traffic layer through the same pose
        // note; the debounce turns a settle into one request.
        mapLayersState.noteCameraPoseChanged(cameraPoseKey())
    }

    /** The traffic layer's settle key: a pose change restarts its debounce. */
    private fun cameraPoseKey(): String = java.lang.String.format(
        java.util.Locale.ROOT,
        "%.6f|%.6f|%.3f|%.2f|%.2f",
        centerLatitudeDegrees,
        centerLongitudeDegrees,
        zoomLevel,
        bearingDegrees,
        pitchDegrees,
    )

    private fun refreshGroundMetersPerScreenPixel() {
        currentGroundMetersPerScreenPixel = mapView?.groundMetersPerScreenPixel()
    }

    // -------------------------------------------------------------------
    // The map control cluster (docs/ReferenceAppUX.md §4.2, round 10)
    // -------------------------------------------------------------------

    /** One zoom control step (§2.4's `map-control-zoom-step`; +1 or −1). */
    fun zoomByControlStep(stepSign: Double) {
        val steppedZoomLevel = zoomLevel + stepSign * ReferenceAppUXDimensions.MAP_CONTROL_ZOOM_STEP
        zoomLevel = steppedZoomLevel.coerceIn(ReferenceApplicationConstants.MINIMUM_ZOOM_LEVEL, ReferenceApplicationConstants.MAXIMUM_ZOOM_LEVEL)
        pushCameraImmediately()
    }

    fun onZoomInControlRequested() = zoomByControlStep(1.0)

    fun onZoomOutControlRequested() = zoomByControlStep(-1.0)

    /**
     * The my-location control (§4.2): centers on the location indicator's
     * fix; during a trip it also re-engages a follow broken by a gesture,
     * returning the follow zoom on that re-engagement (§5.3's re-center).
     */
    fun onMyLocationControlRequested() {
        val latestFix = latestLocationFix?.snappedCoordinate ?: return
        centerLatitudeDegrees = latestFix.latitude
        centerLongitudeDegrees = latestFix.longitude
        if (isNavigationInTrip) {
            val wasBrokenByGesture = followPhase == FollowPhase.BROKEN_BY_GESTURE
            followCamera.reengage()
            syncFollowPhase()
            if (wasBrokenByGesture) {
                zoomLevel = followCamera.targetZoomLevel()
            }
        }
        pushCameraImmediately()
    }

    /** The 2D/3D toggle (§4.2): swings the pitch between the two planes. */
    fun onCameraPlaneControlRequested() {
        isThreeDCameraEnabled = !isThreeDCameraEnabled
        animateCameraTo(currentCameraPose().copy(pitchDegrees = cameraPitchTargetDegrees()))
    }

    /**
     * The compass tap (§2.4): eases north up over the token's 300 ms. In a
     * guided trip the tap is a mode toggle (M73): north-up pins the follow
     * camera's bearing at 0 across every following fix, and a second tap
     * resumes the eased course chase. Outside a trip a camera edit breaks
     * the follow camera like any gesture.
     */
    /** Whether the visitor is holding north-up during the trip (M73). */
    fun isHoldingNorthUp(): Boolean =
        followCamera.bearingMode() == FollowBearingMode.NORTH_UP

    fun onCompassControlRequested() {
        if (isNavigationInTrip) {
            if (followCamera.bearingMode() == FollowBearingMode.NORTH_UP) {
                followCamera.setBearingMode(FollowBearingMode.COURSE_UP)
            } else {
                followCamera.setBearingMode(FollowBearingMode.NORTH_UP)
            }
        }
        animateCameraTo(currentCameraPose().copy(bearingDegrees = NORTH_UP_BEARING_DEGREES))
    }

    // -------------------------------------------------------------------
    // The renderer-owned location indicator (§4.2, round 10)
    // -------------------------------------------------------------------

    // --- Device positioning (M33) ---
    /** The running device location source, or null while location is off. */
    private var deviceLocationSource: DeviceLocationSource? = null
    /** The contract controller the source feeds (one per map session). */
    private var devicePositionController: PositionController? = null
    /** Whether the device source is listening. */
    var isDeviceLocationActive: Boolean by mutableStateOf(false)
        private set

    /**
     * The latest accepted device fix's coordinate, or null while the source is
     * off. Kept apart from [latestLocationFix] (which a simulated trip also
     * updates) so a My location chip never fills a field from simulated
     * geometry.
     */
    private var latestDeviceFixCoordinate: GeographicCoordinate? = null
    /** The honest line beside the location control, when it has something to say. */
    var deviceLocationStatusText: String? by mutableStateOf(null)
        private set

    /**
     * Whether guidance currently follows the device source's own fixes on
     * the planned route (the Location group's second control).
     */
    var isDeviceNavigationActive: Boolean by mutableStateOf(false)
        private set

    /**
     * Whether starting device navigation also started the location source,
     * so stopping it does not turn off a source the visitor chose.
     */
    private var deviceNavigationStartedLocationSource: Boolean = false

    /**
     * Whether the pending runtime-permission prompt belongs to the
     * device-navigation control; a grant then starts guidance rather than
     * only the location source.
     */
    private var isDeviceNavigationPermissionRequested: Boolean = false

    /** Whether the application holds a location permission. */
    fun hasDeviceLocationPermission(): Boolean =
        deviceLocationSource?.hasLocationPermission()
            ?: (applicationContext.checkSelfPermission(android.Manifest.permission.ACCESS_FINE_LOCATION) ==
                android.content.pm.PackageManager.PERMISSION_GRANTED ||
                applicationContext.checkSelfPermission(android.Manifest.permission.ACCESS_COARSE_LOCATION) ==
                android.content.pm.PackageManager.PERMISSION_GRANTED)

    /**
     * Starts the platform location source and its contract controller, or
     * reports why it cannot start. The caller asks for the runtime permission
     * first ([onDeviceLocationPermissionResult]).
     */
    fun startDeviceLocation(remember: Boolean = true) {
        if (mapView == null || isDeviceLocationActive) {
            return
        }
        val controller = devicePositionController
            ?: PositionController().also { devicePositionController = it }
        val source = DeviceLocationSource(
            context = applicationContext,
            positionController = controller,
            onPositioningFailure = { detail -> deviceLocationStatusText = detail },
            onFix = { fix -> applyDevicePositionFix(fix) },
        )
        deviceLocationSource = source
        if (!source.start()) {
            isDeviceLocationActive = false
            val sourceError = controller.sourceStatus()
            deviceLocationStatusText = when (sourceError) {
                PositionSourceError.PERMISSION_DENIED ->
                    ReferenceApplicationConstants.DEVICE_LOCATION_PERMISSION_DENIED_TEXT
                PositionSourceError.SERVICE_DISABLED, PositionSourceError.UNAVAILABLE ->
                    ReferenceApplicationConstants.DEVICE_LOCATION_UNAVAILABLE_TEXT
                PositionSourceError.TIMEOUT ->
                    ReferenceApplicationConstants.DEVICE_LOCATION_UNAVAILABLE_TEXT
                null -> ReferenceApplicationConstants.DEVICE_LOCATION_UNAVAILABLE_TEXT
            }
            mapLayersState.noteLocationStatus(
                if (sourceError == PositionSourceError.PERMISSION_DENIED) {
                    DeviceLocationPanelStatus.PERMISSION_DENIED
                } else {
                    DeviceLocationPanelStatus.UNAVAILABLE
                },
            )
            return
        }
        isDeviceLocationActive = true
        deviceLocationStatusText = null
        mapLayersState.noteLocationStatus(DeviceLocationPanelStatus.WAITING_FOR_FIX)
        if (remember) {
            remembersDeviceLocationEnabled = true
            persistRememberedChoices()
        }
    }

    /** Stops the device source, clears its layer and reports the off state. */
    fun stopDeviceLocation(remember: Boolean = true) {
        deviceLocationSource?.stop()
        deviceLocationSource = null
        latestDeviceFixCoordinate = null
        pendingMyLocationPlanField = null
        isDeviceLocationActive = false
        clearLocationIndicatorOnSurface()
        deviceLocationStatusText = ReferenceApplicationConstants.DEVICE_LOCATION_OFF_TEXT
        mapLayersState.noteLocationStatus(DeviceLocationPanelStatus.OFF)
        if (remember) {
            remembersDeviceLocationEnabled = false
            persistRememberedChoices()
        }
    }

    /**
     * Releases the application's platform resources when the hosting activity
     * is destroyed: the device location source stops listening, a running
     * simulated trip stops, the speech engine shuts down (releasing audio
     * focus) and the native SDK handle closes. The coroutine scope is owned by
     * the host and is cancelled there.
     *
     * Teardown is best-effort and runs once: a platform component that has
     * already gone away must not turn a host teardown into a crash. Nothing is
     * persisted here — an explicit choice was recorded when the user made it.
     */
    fun shutdown() {
        if (isShutdown) {
            return
        }
        isShutdown = true
        runCatching {
            if (isDeviceLocationActive || deviceLocationSource != null) {
                stopDeviceLocation(remember = false)
            }
        }
        runCatching {
            if (isNavigationInTrip || tripDriver.isTripActive) {
                exitSimulatedTrip()
            }
        }
        runCatching { speechSpeaker.shutdown() }
        runCatching { sdkFacade.close() }
    }

    /** Whether [shutdown] already released the resources (it runs at most once). */
    private var isShutdown: Boolean = false

    /**
     * Whether the device-location choice belongs in the remembered record:
     * true while the visitor's source is on, false after they stop it.
     */
    private var remembersDeviceLocationEnabled: Boolean = false

    /**
     * Whether a remembered device-location choice is waiting for the shipped
     * map session before its source can start.
     */
    private var pendingRememberedDeviceLocationStart: Boolean = false

    /**
     * Restores the remembered device-location source once the shipped map
     * session exists. The restart happens silently only when the device
     * already holds the location permission; otherwise the choice stays in
     * the record for a later visit rather than prompting at launch.
     */
    private fun restoreRememberedDeviceLocationIfPermitted() {
        if (!pendingRememberedDeviceLocationStart || mapView == null) {
            return
        }
        pendingRememberedDeviceLocationStart = false
        if (!hasDeviceLocationPermission()) {
            return
        }
        startDeviceLocation(remember = false)
    }

    /** Applies the runtime permission result the sheet asked for. */
    fun onDeviceLocationPermissionResult(granted: Boolean) {
        val pendingPlanField = pendingMyLocationPlanField
        if (!granted && pendingPlanField != null) {
            pendingMyLocationPlanField = null
            isDeviceNavigationPermissionRequested = false
            planFieldMyLocationStatusText =
                ReferenceApplicationConstants.PLAN_MY_LOCATION_UNAVAILABLE_TEMPLATE.format(
                    ReferenceApplicationConstants.DEVICE_LOCATION_PERMISSION_DENIED_TEXT,
                )
            devicePositionController?.sourceFailed(PositionSourceError.PERMISSION_DENIED)
            isDeviceLocationActive = false
            deviceLocationStatusText =
                ReferenceApplicationConstants.DEVICE_LOCATION_PERMISSION_DENIED_TEXT
            mapLayersState.noteLocationStatus(DeviceLocationPanelStatus.PERMISSION_DENIED)
            return
        }
        if (granted) {
            // A prompt the navigation control started begins guidance on the
            // route (its own start path also starts the source), so stopping
            // the mode also stops the source it started.
            if (isDeviceNavigationPermissionRequested) {
                isDeviceNavigationPermissionRequested = false
                startDeviceNavigation()
            } else {
                startDeviceLocation()
            }
            return
        }
        isDeviceNavigationPermissionRequested = false
        devicePositionController?.sourceFailed(PositionSourceError.PERMISSION_DENIED)
        isDeviceLocationActive = false
        deviceLocationStatusText =
            ReferenceApplicationConstants.DEVICE_LOCATION_PERMISSION_DENIED_TEXT
        mapLayersState.noteLocationStatus(DeviceLocationPanelStatus.PERMISSION_DENIED)
    }

    /**
     * Records that the sheet is about to ask for the location permission on
     * the device-navigation control's behalf.
     */
    fun noteDeviceNavigationPermissionRequested() {
        isDeviceNavigationPermissionRequested = true
    }

    /**
     * The plan field whose My location chip is waiting for a fix or a
     * permission result, or null. The pending field is the only field a fix
     * fills, so the two chips never overwrite each other.
     */
    private var pendingMyLocationPlanField: PlanSearchField? = null

    /** The My location chip's honest failure line in the search view, or null. */
    var planFieldMyLocationStatusText: String? by mutableStateOf(null)
        private set

    /**
     * Records that a plan field's My location chip is about to ask for the
     * runtime location permission, so the grant fills that one field.
     */
    fun notePlanFieldMyLocationPermissionRequested(planSearchField: PlanSearchField) {
        pendingMyLocationPlanField = planSearchField
        planFieldMyLocationStatusText = null
    }

    /**
     * The My location chip's act: fills only [planSearchField] with the
     * device's latest fix. With no fix yet it starts the location source (or
     * asks the sheet to request the permission first) and remembers the field;
     * the first accepted fix then fills that field alone. It never plans and
     * never touches the other field.
     */
    fun fillPlanFieldFromMyLocation(planSearchField: PlanSearchField) {
        planFieldMyLocationStatusText = null
        val currentFixCoordinate = latestDeviceFixCoordinate
        if (currentFixCoordinate != null) {
            fillPlanFieldWithMyLocation(planSearchField, currentFixCoordinate)
            return
        }
        pendingMyLocationPlanField = planSearchField
        if (isDeviceLocationActive) {
            return
        }
        startDeviceLocation()
        if (!isDeviceLocationActive) {
            planFieldMyLocationStatusText =
                ReferenceApplicationConstants.PLAN_MY_LOCATION_UNAVAILABLE_TEMPLATE.format(
                    deviceLocationStatusText
                        ?: ReferenceApplicationConstants.DEVICE_LOCATION_UNAVAILABLE_TEXT,
                )
            pendingMyLocationPlanField = null
        }
    }

    /**
     * Fills one plan field with a My location place. The other field's text
     * and resolved place stay exactly as they were.
     */
    private fun fillPlanFieldWithMyLocation(
        planSearchField: PlanSearchField,
        coordinate: GeographicCoordinate,
    ) {
        val myLocationPlace = Place(
            structuredAddressText = ReferenceApplicationConstants.PLAN_MY_LOCATION_ACTION_TEXT,
            coordinate = coordinate,
            geocodeQuality = GeocodeQuality.POINT,
        )
        when (planSearchField) {
            PlanSearchField.ORIGIN -> {
                originPlace = myLocationPlace
                originSearchText = myLocationPlace.structuredAddressText
            }
            PlanSearchField.DESTINATION -> {
                destinationPlace = myLocationPlace
                destinationSearchText = myLocationPlace.structuredAddressText
            }
        }
        planFieldMyLocationStatusText = null
        clearErrorReport()
    }

    /**
     * Starts guidance on the planned route from this device's fixes. The
     * location source starts when it is off (through the same permission and
     * source path the location control uses), a simulated trip ends first
     * because one trip runs at a time, and every accepted fix then reaches
     * the navigator. Without a planned route the control states what it needs
     * and starts nothing.
     */
    fun startDeviceNavigation() {
        if (isDeviceNavigationActive) {
            return
        }
        val planned = plannedRoute
        if (planned == null) {
            deviceLocationStatusText =
                ReferenceApplicationConstants.DEVICE_NAVIGATION_PLAN_FIRST_TEXT
            return
        }
        // One trip at a time: the simulated presentation and its camera end
        // before the device-driven one engages.
        if (isNavigationInTrip) {
            exitSimulatedTrip()
        }
        val wasLocationSourceActive = isDeviceLocationActive
        if (!wasLocationSourceActive) {
            startDeviceLocation()
            if (!isDeviceLocationActive) {
                // The start path reported the permission or service state.
                deviceNavigationStartedLocationSource = false
                return
            }
        }
        deviceNavigationStartedLocationSource = !wasLocationSourceActive
        tripDriver.startDeviceTrip(planned.route)
        val startFailure = tripDriver.startFailure
        if (startFailure != null) {
            reportFailure(startFailure)
            if (deviceNavigationStartedLocationSource) {
                deviceNavigationStartedLocationSource = false
                stopDeviceLocation()
            }
            return
        }
        isDeviceNavigationActive = true
        deviceLocationStatusText = null
        isNavigationInTrip = true
        hasSpeechTopicSubscription = true
        followCamera.engageTrip(
            currentCenterLatitudeDegrees = centerLatitudeDegrees,
            currentCenterLongitudeDegrees = centerLongitudeDegrees,
            currentZoomLevel = zoomLevel,
            currentBearingDegrees = bearingDegrees,
            currentPitchDegrees = pitchDegrees,
        )
        syncFollowPhase()
        zoomLevel = ReferenceAppUXDimensions.NAVIGATION_FOLLOW_ZOOM
        // The follow camera's 3D course-follow pitch is the plane toggle's:
        // a device-driven trip does not turn the toggle on by itself.
        if (isThreeDCameraEnabled) {
            animateCameraTo(currentCameraPose().copy(pitchDegrees = cameraPitchTargetDegrees()))
        } else {
            pushCameraImmediately()
        }
        clearErrorReport()
        navigationPhaseChip = navigationPhaseChipOf(
            currentPhase = navigationController.currentTripState()?.phase,
            phaseChange = null,
        )
    }

    /**
     * Ends device-driven guidance. The location source stops only when this
     * mode started it; a source the visitor started by hand stays running and
     * a later fix keeps presenting the device position.
     */
    fun stopDeviceNavigation() {
        if (!isDeviceNavigationActive) {
            return
        }
        isDeviceNavigationActive = false
        tripDriver.stopTrip()
        speechSpeaker.releaseJourneySpeech()
        isNavigationInTrip = false
        hasSpeechTopicSubscription = false
        navigationPhaseChip = ""
        restoreFollowCamera()
        if (deviceNavigationStartedLocationSource) {
            deviceNavigationStartedLocationSource = false
            stopDeviceLocation()
        } else {
            deviceLocationStatusText = null
            // The hand-started source keeps reporting; show the current fix as
            // the tracking dot so the guided wedge does not outlive the trip.
            latestLocationFix?.let { currentFix ->
                pushLocationIndicator(guided = false, fix = currentFix)
            }
        }
    }

    /** Shows the device fix through the renderer's tracking dot and centres the camera. */
    private fun applyDevicePositionFix(fix: PositionFix) {
        latestDeviceFixCoordinate = fix.coordinate
        mapLayersState.noteLocationStatus(DeviceLocationPanelStatus.ACTIVE)
        val smoothedCourse = devicePositionController?.smoothedCourseDegrees()
        if (isDeviceNavigationActive) {
            // Every accepted device fix drives the route's guidance through
            // the same delivery path a simulated fix uses, so the banner, the
            // ETA line, the location indicator and the follow camera all read
            // the controller's own stream.
            tripDriver.deliverDeviceFix(
                coordinate = fix.coordinate,
                courseHeadingDegrees = smoothedCourse,
                horizontalAccuracyMeters = fix.horizontalAccuracyMeters
                    ?: ReferenceApplicationConstants.DEVICE_NAVIGATION_FALLBACK_ACCURACY_METERS,
            )
            tripDriver.tripDeliveryFailure?.let { deliveryFailure ->
                // A refused fix ends the mode honestly rather than leaving a
                // stale trip on screen (§6.3).
                stopDeviceNavigation()
                reportFailureOf(deliveryFailure)
            }
            return
        }
        pushLocationIndicator(
            guided = false,
            fix = SimulatedProcessedFix(
                snappedCoordinate = fix.coordinate,
                courseHeadingDegrees = smoothedCourse ?: fix.courseHeadingDegrees,
                horizontalAccuracyMeters = fix.horizontalAccuracyMeters,
            ),
        )
        // A My location chip waiting for this fix fills its own field now; the
        // pending field is cleared so a later fix never overwrites the value.
        pendingMyLocationPlanField?.let { pendingField ->
            pendingMyLocationPlanField = null
            fillPlanFieldWithMyLocation(pendingField, fix.coordinate)
        }
        centerLatitudeDegrees = fix.coordinate.latitude
        centerLongitudeDegrees = fix.coordinate.longitude
        zoomLevel = maxOf(zoomLevel, DEVICE_LOCATION_ZOOM_LEVEL)
        pushCameraImmediately()
    }

    /**
     * The latest simulated fix the indicator shows, or null when clear (no
     * fix renders nothing — never a stale last position, §4.2).
     */
    var latestLocationFix: SimulatedProcessedFix? by mutableStateOf(null)
        private set

    /**
     * The location puck's interpolation (M72): every fix starts an eased
     * motion toward it, and an app frame callback draws the pose while the
     * motion runs, so the disc glides instead of hopping the fix distance.
     */
    private val locationPoseAnimator = LocationPoseAnimator()

    /** Whether the current fix draws the guided wedge rather than the dot. */
    private var latestLocationGuided: Boolean = false

    /** The frame callback that draws one interpolated pose. */
    private val locationFrameCallback = Choreographer.FrameCallback {
        drawLocationPoseSnapshot()
    }

    /**
     * Pushes the location indicator for one fix to the renderer (§4.2: the
     * visuals are renderer-owned; the app only sets state/style): the
     * guided trip draws the `GuidedCourseArrow` wedge along the reported
     * course, tracking draws the `TrackingDot`, and the accuracy ring
     * follows the fix's own report.
     */
    fun pushLocationIndicator(guided: Boolean, fix: SimulatedProcessedFix) {
        val snappedCoordinate = fix.snappedCoordinate ?: return
        latestLocationFix = fix
        latestLocationGuided = guided
        // The fix enters the animator; the drawn pose comes from `poseAt`, so
        // the first fix draws at once and later ones glide (the web pump's
        // rule, from the same shared crate).
        locationPoseAnimator.pushFix(
            latitudeDegrees = snappedCoordinate.latitude,
            longitudeDegrees = snappedCoordinate.longitude,
            courseDegrees = if (guided) fix.courseHeadingDegrees else null,
            horizontalAccuracyMeters = fix.horizontalAccuracyMeters,
            receivedAtMilliseconds = SystemClock.uptimeMillis().toDouble(),
        )
        drawLocationPoseSnapshot()
    }

    /**
     * Draws the interpolated pose for this instant and keeps the frame
     * callback posted while the motion runs. The shipped view reports a
     * refused push through the listener; the state records the fix so a
     * recreated view can push it again.
     */
    private fun drawLocationPoseSnapshot() {
        val pose = locationPoseAnimator.poseAt(SystemClock.uptimeMillis().toDouble()) ?: return
        mapView?.setLocationIndicator(
            state = LocationIndicatorState(
                coordinate = GeographicCoordinate(
                    latitude = pose.latitudeDegrees,
                    longitude = pose.longitudeDegrees,
                ),
                courseHeadingDegrees = pose.courseDegrees,
                horizontalAccuracyMeters = pose.horizontalAccuracyMeters,
            ),
            style = locationIndicatorStyle(),
        )
        val choreographer = Choreographer.getInstance()
        if (pose.isAnimating) {
            choreographer.postFrameCallback(locationFrameCallback)
        } else {
            choreographer.removeFrameCallback(locationFrameCallback)
        }
    }

    /** The style the current fix draws with. */
    private fun locationIndicatorStyle(): LocationIndicatorStyle = LocationIndicatorStyle(
        visualKind = if (latestLocationGuided) {
            LocationIndicatorVisualKind.GUIDED_COURSE_ARROW
        } else {
            LocationIndicatorVisualKind.TRACKING_DOT
        },
        radiusPixels = null,
        rgbaColor = null,
        ringRgbaColor = null,
        haloRgbaColor = null,
        showAccuracyRing = true,
    )

    /** Clears the renderer's location layer (§4.2's no-fix state). */
    fun clearLocationIndicatorOnSurface() {
        if (latestLocationFix == null) {
            return
        }
        latestLocationFix = null
        Choreographer.getInstance().removeFrameCallback(locationFrameCallback)
        locationPoseAnimator.reset()
        mapView?.clearLocationIndicator()
    }

    // -------------------------------------------------------------------
    // Diagnostics recent events (round 10's subscribe/flush seam)
    // -------------------------------------------------------------------

    /**
     * Drag-pan (§5.2): a screen-pixel delta routed through the shipped
     * MapView's contract camera (the same algebra its touch recognizer uses).
     * A pan is one of §5.3's following-breaking gestures.
     */
    fun panByPixels(deltaXPixels: Double, deltaYPixels: Double) {
        mapView?.panByScreenPixels(deltaXPixels, deltaYPixels)
    }

    /** Applies one camera preset (the demo scenes' sibling camera control). */
    fun applyPreset(preset: CameraPreset) {
        centerLatitudeDegrees = preset.latitudeDegrees
        centerLongitudeDegrees = preset.longitudeDegrees
        zoomLevel = preset.zoomLevel
        bearingDegrees = preset.bearingDegrees
        pitchDegrees = preset.pitchDegrees
        selectedCameraPresetIdentifier = preset.identifier
        pushCameraImmediately()
        clearErrorReport()
        persistRememberedChoices()
    }

    /**
     * Fits the planned route between the waypoints through the shipped
     * MapView's `fitBounds` (§5.1 camera fitting): the view adopts the fitted
     * camera and reports its exact center, zoom and top-down / north-up
     * framing (bearing and pitch zero by contract) back through the listener.
     */
    fun applyRoutePreviewFraming(route: Route) {
        val view = mapView ?: return
        val viewport = view.viewportSize ?: return
        view.fitBounds(
            boundingBox = route.boundingBox,
            edgeInsets = EdgeInsets(
                left = ReferenceApplicationConstants.ROUTE_PREVIEW_INSET_LEFT_PIXELS,
                top = ReferenceApplicationConstants.ROUTE_PREVIEW_INSET_TOP_PIXELS,
                right = ReferenceApplicationConstants.ROUTE_PREVIEW_INSET_RIGHT_PIXELS,
                // The route summary card docks in the half sheet (v2 §2.3):
                // the fit's bottom inset clears that dock, so the preview
                // line stays visible above it.
                bottom = viewport.heightPixels *
                    ReferenceAppUXDimensions.SHEET_HALF_HEIGHT_FRACTION,
            ),
            maximumZoomLevel = ReferenceApplicationConstants.MAXIMUM_FIT_BOUNDS_ZOOM_LEVEL,
        )
        clearErrorReport()
    }

    // -------------------------------------------------------------------
    // Demo plans and the planned route (§4.1's plan flow)
    // -------------------------------------------------------------------

    /** The demo plan the demo-plan picker has selected (the artifact's trace first). */
    var selectedDemoPlan: DemoPlan by mutableStateOf(demoPlans.first { it.isArtifactTrace })
        private set

    /** The search-planned route's trip source, or null while none is planned. */
    var plannedRoute: PlannedRoute? by mutableStateOf(null)
        private set

    /**
     * The trip source "Start simulated navigation" drives: the search-planned
     * route when one is planned, otherwise the selected demo plan's trace.
     */
    val activeSimulatedTrace: SimulatedTrace
        get() = plannedRoute?.simulatedTrace ?: selectedDemoPlan.trace

    /** Selects a demo plan in the half sheet's picker (and drops a planned route). */
    fun selectDemoPlan(demoPlan: DemoPlan) {
        selectedDemoPlan = demoPlan
        rememberedDemoPlanScenarioName = demoPlan.scenarioName
        plannedRoute = null
        applyRoutePreviewFraming(demoPlan.trace.route)
        persistRememberedChoices()
    }

    // -------------------------------------------------------------------
    // Scene overlays (§9: the shared artifact feeds the SDK overlay API)
    // -------------------------------------------------------------------

    /** Toggle state per artifact overlay scene key (route/polygon/markers/traffic). */
    val sceneToggleStates: SnapshotStateMap<String, Boolean> = mutableStateMapOf(
        "route" to false,
        "polygon" to false,
        "markers" to false,
        "traffic" to false,
    )

    /** One scene chip's act: toggles the scene and remembers the choice. */
    fun setSceneToggle(sceneKey: String, isEnabled: Boolean) {
        sceneToggleStates[sceneKey] = isEnabled
        persistRememberedChoices()
    }

    /** The scenes carousel's clear act: turns every scene off and remembers it. */
    fun clearSceneToggles() {
        sceneToggleStates.keys.forEach { sceneKey -> sceneToggleStates[sceneKey] = false }
        persistRememberedChoices()
    }

    /** Count of overlays the last surface push carried (diagnostics fact). */
    var pushedOverlayCount: Int by mutableStateOf(0)
        private set

    private fun sceneCoordinate(sceneCoordinateInstance: com.rusticmaps.sdk.demo.SceneCoordinate) =
        GeographicCoordinate(
            latitude = sceneCoordinateInstance.latitudeDegrees,
            longitude = sceneCoordinateInstance.longitudeDegrees,
        )

    private fun coordinateOf(coordinate: GeographicCoordinate) = coordinate

    /**
     * Normalizes a congestion label for matching: the artifact writes
     * `FreeFlow` while the contract enum writes `FREE_FLOW`, so separators
     * (`_`, `-`) and case are stripped from both sides before the match.
     */
    private fun normalizedCongestionLevelText(congestionLevelText: String): String =
        congestionLevelText.lowercase().replace("_", "").replace("-", "")

    private fun congestionLevelOf(congestionLevelText: String): TrafficFlowCongestionLevel =
        TrafficFlowCongestionLevel.entries.firstOrNull { congestionLevel ->
            normalizedCongestionLevelText(congestionLevel.name) ==
                normalizedCongestionLevelText(congestionLevelText)
        } ?: throw IllegalArgumentException(
            "traffic congestion level '$congestionLevelText' matches no known level",
        )

    private fun sceneDescriptor(sceneKey: String, scene: OverlayScene): MapOverlay? {
        if (sceneToggleStates[sceneKey] != true) {
            return null
        }
        val coordinates = scene.coordinates?.map(::sceneCoordinate)
        val ringCoordinates = scene.ringCoordinates?.map(::sceneCoordinate)
        val positions = scene.positions?.map(::sceneCoordinate)
        val trafficFlowSegments = scene.trafficFlowSegments?.map { segment ->
            TrafficFlowData(
                shapeCoordinates = segment.shapeCoordinates.map(::sceneCoordinate),
                currentSpeedKilometersPerHour = null,
                freeFlowSpeedKilometersPerHour = null,
                congestionLevel = congestionLevelOf(segment.congestionLevel),
            )
        }
        return when {
            coordinates != null -> MapOverlay(
                kind = MapOverlayKind.ROUTE_POLYLINE,
                coordinates = coordinates,
                rgbaColor = null,
                widthPixels = null,
                ringCoordinates = null,
                pointMarkerPositions = null,
                pointMarkerRadiusPixels = null,
                trafficFlowSegments = null,
            )
            ringCoordinates != null -> MapOverlay(
                kind = MapOverlayKind.FILLED_POLYGON,
                coordinates = null,
                rgbaColor = null,
                widthPixels = null,
                ringCoordinates = ringCoordinates,
                pointMarkerPositions = null,
                pointMarkerRadiusPixels = null,
                trafficFlowSegments = null,
            )
            positions != null -> MapOverlay(
                kind = MapOverlayKind.POINT_MARKERS,
                coordinates = null,
                rgbaColor = null,
                widthPixels = null,
                ringCoordinates = null,
                pointMarkerPositions = positions,
                pointMarkerRadiusPixels = null,
                trafficFlowSegments = null,
            )
            trafficFlowSegments != null -> MapOverlay(
                kind = MapOverlayKind.TRAFFIC_FLOW,
                coordinates = null,
                rgbaColor = null,
                widthPixels = null,
                ringCoordinates = null,
                pointMarkerPositions = null,
                pointMarkerRadiusPixels = null,
                trafficFlowSegments = trafficFlowSegments,
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
        // The traffic layer draws over the base and under the route line, the
        // same stacking order the browser demo applies.
        if (trafficFlowSegments.isNotEmpty()) {
            add(
                MapOverlay(
                    kind = MapOverlayKind.TRAFFIC_FLOW,
                    coordinates = null,
                    rgbaColor = null,
                    widthPixels =
                        ReferenceApplicationConstants.TRAFFIC_FLOW_OVERLAY_WIDTH_PIXELS,
                    ringCoordinates = null,
                    pointMarkerPositions = null,
                    pointMarkerRadiusPixels = null,
                    trafficFlowSegments = trafficFlowSegments,
                ),
            )
        }
        sceneDescriptor("route", artifact.overlayScenes.route)?.let { add(it) }
        sceneDescriptor("polygon", artifact.overlayScenes.polygon)?.let { add(it) }
        sceneDescriptor("markers", artifact.overlayScenes.markers)?.let { add(it) }
        sceneDescriptor("traffic", artifact.overlayScenes.traffic)?.let { add(it) }
        add(
            MapOverlay(
                kind = MapOverlayKind.ROUTE_POLYLINE,
                coordinates = activeSimulatedTrace.route.legs
                    .flatMap { leg -> leg.steps }
                    .flatMap { step -> step.shape }
                    .map(::coordinateOf),
                rgbaColor = null,
                widthPixels = null,
                ringCoordinates = null,
                pointMarkerPositions = null,
                pointMarkerRadiusPixels = null,
                trafficFlowSegments = null,
            ),
        )
        // The planned route's A/B endpoint pins (§4.1): the same additive
        // overlay kind the web planner draws, colored from the same named
        // values.
        addAll(routeEndpointPinDescriptors())
    }

    /** The active route's endpoint pins: origin A (green), destination B
     * (red), each a teardrop anchored at its tip. */
    private fun routeEndpointPinDescriptors(): List<MapOverlay> {
        val route = plannedRoute?.route ?: return emptyList()
        val originCoordinate = route.legs.firstOrNull()
            ?.steps?.firstOrNull()?.shape?.firstOrNull() ?: return emptyList()
        val destinationCoordinate = route.legs.lastOrNull()
            ?.steps?.lastOrNull()?.shape?.lastOrNull() ?: return emptyList()
        return listOf(
            endpointPinDescriptor(
                anchorCoordinate = originCoordinate,
                pinColor = OverlayColor(
                    red = ReferenceApplicationConstants.ROUTE_ORIGIN_PIN_RED,
                    green = ReferenceApplicationConstants.ROUTE_ORIGIN_PIN_GREEN,
                    blue = ReferenceApplicationConstants.ROUTE_ORIGIN_PIN_BLUE,
                    alpha = ReferenceApplicationConstants.ROUTE_PIN_ALPHA,
                ),
            ),
            endpointPinDescriptor(
                anchorCoordinate = destinationCoordinate,
                pinColor = OverlayColor(
                    red = ReferenceApplicationConstants.ROUTE_DESTINATION_PIN_RED,
                    green = ReferenceApplicationConstants.ROUTE_DESTINATION_PIN_GREEN,
                    blue = ReferenceApplicationConstants.ROUTE_DESTINATION_PIN_BLUE,
                    alpha = ReferenceApplicationConstants.ROUTE_PIN_ALPHA,
                ),
            ),
        )
    }

    /** One endpoint pin descriptor at the documented default radius. */
    private fun endpointPinDescriptor(
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

    /** Pushes the current overlay list to the map view; a refusal reports verbatim. */
    fun pushOverlaysToSurface() {
        val view = mapView ?: return
        val overlayList = overlayDescriptorList()
        if (view.setOverlays(overlayList)) {
            pushedOverlayCount = overlayList.size
            clearErrorReport()
        }
    }

    /** Clears the map view's overlay list; the next frames draw tiles alone. */
    fun clearTheOverlaysOnSurface() {
        val view = mapView ?: return
        if (view.clearOverlays()) {
            pushedOverlayCount = 0
            clearErrorReport()
        }
    }

    // -------------------------------------------------------------------
    // Search (geocode) and route planning (§4.1's plan flow)
    // -------------------------------------------------------------------

    /** Which search field the shared result list currently serves. */
    enum class PlanSearchField { ORIGIN, DESTINATION }

    private val originAutocomplete = PlanAutocompleteFieldState()
    private val destinationAutocomplete = PlanAutocompleteFieldState()

    private var originSearchTextState: String by mutableStateOf("")
    private var destinationSearchTextState: String by mutableStateOf("")

    /** The origin field's text; editing it remembers the last explicit value. */
    var originSearchText: String
        get() = originSearchTextState
        set(value) {
            originSearchTextState = value
            // Any text change (typing, a suggestion, Swap, My location or a
            // restored pair) retires the previous query's suggestions and
            // cancels its request before the edit path schedules a new one.
            resetAutocomplete(PlanSearchField.ORIGIN)
            // Editing away from an assigned place clears the resolution, so
            // Plan route re-asks for a search result instead of planning the
            // stale place (docs/ReferenceAppUX.md §4.1).
            if (value != originPlace?.structuredAddressText) {
                originPlace = null
            }
            persistRememberedChoices()
        }

    /** The destination field's text; editing it remembers the last explicit value. */
    var destinationSearchText: String
        get() = destinationSearchTextState
        set(value) {
            destinationSearchTextState = value
            resetAutocomplete(PlanSearchField.DESTINATION)
            if (value != destinationPlace?.structuredAddressText) {
                destinationPlace = null
            }
            persistRememberedChoices()
        }

    /** The last text submitted to the plan search (never a provider payload). */
    var lastSearchQueryText: String by mutableStateOf("")
        private set

    /** The field whose query the shared result list serves. */
    var activePlanSearchField: PlanSearchField by mutableStateOf(PlanSearchField.DESTINATION)
        private set

    /** The resolved origin place, or null while the demo-position default stands. */
    var originPlace: Place? by mutableStateOf(null)
        private set

    /** The resolved destination place, or null while none is picked. */
    var destinationPlace: Place? by mutableStateOf(null)
        private set

    /** The shared result rows of the latest search. */
    val searchResults: SnapshotStateList<Place> = mutableStateListOf()

    /**
     * Whether [searchResults] holds the shared artifact's demo places
     * (docs/ReferenceAppUX.md §4.1's keyless search): the result list's
     * heading then says so, never presenting demo content as live results.
     */
    var searchResultsAreDemoPlaces: Boolean by mutableStateOf(false)
        private set

    /**
     * The keyless search's honest no-match line, or null while a demo match
     * stands or before a search ran (§4.1: the row list is never filled with
     * guesses).
     */
    var demoSearchNoMatchText: String? by mutableStateOf(null)
        private set

    var searchInFlight: Boolean by mutableStateOf(false)
        private set

    /**
     * The provider the SDK reports serving type-ahead, or null when none is
     * registered. The value comes from the contract's own readback: the
     * credential state decides which provider holds the slot, and this
     * application never names one (docs/ReferenceAppUX.md §5.5).
     */
    val autocompleteProviderKey: String?
        get() = nativeMapSdk.providerRegistrations()
            .firstOrNull { registration ->
                registration.contract == ProviderContract.PLACE_SEARCH && registration.registered
            }
            ?.providerKey

    /**
     * Registers the shared registry's credential-less place-search provider
     * when no other provider serves the capability yet (docs/ReferenceAppUX.md §5.5,
     * "Which provider answers"): a keyless session offers suggestions before
     * any key is entered, while a credentialed session keeps the provider its
     * credential installed. The choice is registry-driven — the first
     * Android-registerable descriptor with a blank credential label and a
     * declared demo deployment — never a provider name.
     *
     * A refusal is reported like any other SDK failure and leaves the slot
     * empty; the field then states the contract's own sentence inline.
     */
    fun registerKeylessSearchProviderWhenUncredentialed() {
        val alreadyServing = nativeMapSdk.providerRegistrations().any { registration ->
            registration.contract == ProviderContract.PLACE_SEARCH && registration.registered
        }
        if (alreadyServing) {
            return
        }
        val descriptor = keylessPlaceSearchProviderDescriptor() ?: return
        try {
            sdkFacade.configureProvider(
                ProviderContract.PLACE_SEARCH,
                descriptor.providerKey,
                descriptor.demoBaseUrl,
            )
        } catch (registrationException: Exception) {
            if (registrationException is CancellationException) {
                throw registrationException
            }
            reportFailureOf(registrationException)
        }
        capabilityProviderState.refresh()
    }

    /**
     * The field's edit path: the value the user typed reaches the field, and a
     * trimmed query that meets the shared threshold schedules one type-ahead
     * request after the shared quiet period (docs/ReferenceAppUX.md §5.5).
     * A shorter query clears the list and asks nobody.
     */
    fun onPlanSearchQueryChanged(
        planSearchField: PlanSearchField,
        queryText: String,
    ) {
        when (planSearchField) {
            PlanSearchField.ORIGIN -> originSearchText = queryText
            PlanSearchField.DESTINATION -> destinationSearchText = queryText
        }
        scheduleAutocomplete(planSearchField, queryText)
    }

    /**
     * The type-ahead list's one row act: the chosen suggestion's display line
     * fills the field, its place is held so a later Plan route resolves the
     * same place, and the list closes. Assignment never plans by itself.
     */
    fun selectAutocompleteSuggestion(
        planSearchField: PlanSearchField,
        place: Place,
    ) {
        activePlanSearchField = planSearchField
        when (planSearchField) {
            PlanSearchField.ORIGIN -> {
                originPlace = place
                originSearchText = place.structuredAddressText
            }
            PlanSearchField.DESTINATION -> {
                destinationPlace = place
                destinationSearchText = place.structuredAddressText
            }
        }
        clearErrorReport()
    }

    /** The suggestion rows, no-match row and inline sentence one field renders. */
    fun autocompleteViewState(planSearchField: PlanSearchField): PlanAutocompleteViewState {
        val state = autocompleteStateFor(planSearchField)
        return PlanAutocompleteViewState(
            suggestions = state.suggestions,
            showsNoMatchRow = state.showsNoMatchRow,
            failureText = state.failureText,
            isVisible = state.isVisible,
        )
    }

    /**
     * Whether a type-ahead request is still current for the field. A
     * superseded query's job is cancelled, so the field is idle even while
     * the provider's earlier request is still unwinding.
     */
    fun isAutocompleteRequestInFlight(planSearchField: PlanSearchField): Boolean =
        autocompleteStateFor(planSearchField).requestJob?.isActive == true

    private fun autocompleteStateFor(planSearchField: PlanSearchField): PlanAutocompleteFieldState =
        when (planSearchField) {
            PlanSearchField.ORIGIN -> originAutocomplete
            PlanSearchField.DESTINATION -> destinationAutocomplete
        }

    /**
     * Retires the field's current request and every value it produced. The
     * generation bump makes a late answer from the cancelled request drop on
     * arrival instead of replacing the newer list (docs/ReferenceAppUX.md §5.5
     * rule 4).
     */
    private fun resetAutocomplete(planSearchField: PlanSearchField) {
        val state = autocompleteStateFor(planSearchField)
        state.requestJob?.cancel()
        state.requestJob = null
        state.generation += 1
        state.suggestions = emptyList()
        state.showsNoMatchRow = false
        state.failureText = null
        state.isVisible = false
    }

    /**
     * Schedules the field's one type-ahead request: the shared quiet period
     * first, then the contract's `suggest` with the trimmed query, then at
     * most the shared row cap. Only the query still current when the timer
     * fires asks the provider; only the answer to the current query reaches
     * the list.
     */
    private fun scheduleAutocomplete(
        planSearchField: PlanSearchField,
        queryText: String,
    ) {
        resetAutocomplete(planSearchField)
        val trimmedQuery = queryText.trim()
        if (trimmedQuery.length < ReferenceAppUXDimensions.AUTOCOMPLETE_MINIMUM_QUERY_CHARACTERS) {
            return
        }
        val state = autocompleteStateFor(planSearchField)
        val queryGeneration = state.generation
        state.requestJob = coroutineScope.launch {
            try {
                delay(ReferenceAppUXDimensions.AUTOCOMPLETE_DEBOUNCE_MILLISECONDS)
                if (queryGeneration != state.generation) {
                    return@launch
                }
                val results = nativeMapSdk.suggest(trimmedQuery, null)
                if (queryGeneration != state.generation) {
                    return@launch
                }
                state.suggestions = results.take(
                    ReferenceAppUXDimensions.AUTOCOMPLETE_MAXIMUM_RESULTS,
                )
                state.showsNoMatchRow = state.suggestions.isEmpty()
                state.failureText = null
                state.isVisible = true
            } catch (cancellation: CancellationException) {
                // A superseded or torn-down request shows nothing.
                throw cancellation
            } catch (failure: Exception) {
                if (queryGeneration != state.generation) {
                    return@launch
                }
                // The field keeps its text; one inline sentence says what the
                // provider answered, and the next keystroke retries.
                state.suggestions = emptyList()
                state.showsNoMatchRow = false
                state.failureText = failureDetailTextOf(failure)
                state.isVisible = false
            }
        }
    }

    /** The registry's first credential-less place-search provider on Android. */
    private fun keylessPlaceSearchProviderDescriptor(): ProviderCredentialDescriptor? {
        val placeSearchStorageName = ProviderContract.PLACE_SEARCH.name.lowercase()
        return providerRegistry.providers.firstOrNull { descriptor ->
            descriptor.isRegistrationAvailableOnAndroid &&
                descriptor.credentialLabel.isBlank() &&
                descriptor.demoBaseUrl.isNotBlank() &&
                placeSearchStorageName in descriptor.servedContracts
        }
    }

    /**
     * Runs the plan search on one field's text. A keyless session filters the
     * shared artifact's `demo_places` by the typed text — never a provider
     * call, never an invented result; the rows say they are demo places. A
     * keyed session asks the provider, exactly as before; a provider failure
     * reports itself in the error strip and the list stays empty (§4.1).
     */
    fun runPlanSearch(planSearchField: PlanSearchField) {
        val addressText = when (planSearchField) {
            PlanSearchField.ORIGIN -> originSearchText
            PlanSearchField.DESTINATION -> destinationSearchText
        }
        if (searchInFlight || addressText.isBlank()) {
            return
        }
        activePlanSearchField = planSearchField
        lastSearchQueryText = addressText
        persistRememberedChoices()
        if (isProviderKeyless) {
            val queryLowercase = addressText.trim().lowercase()
            val matchingDemoPlaces = artifact.demoPlaces
                .filter { demoPlace -> demoPlace.name.lowercase().contains(queryLowercase) }
            searchResults.clear()
            searchResults.addAll(
                matchingDemoPlaces.take(SEARCH_RESULTS_DISPLAY_LIMIT).map(::placeFromDemoPlace),
            )
            searchResultsAreDemoPlaces = true
            demoSearchNoMatchText = if (matchingDemoPlaces.isEmpty()) {
                ReferenceApplicationConstants.DEMO_NO_MATCH_HINT_TEMPLATE
                    .format(planSearchFieldLabel(planSearchField))
            } else {
                null
            }
            clearErrorReport()
            return
        }
        searchResultsAreDemoPlaces = false
        demoSearchNoMatchText = null
        searchInFlight = true
        coroutineScope.launch {
            try {
                val results = sdkFacade.nativeSdk.forwardGeocode(addressText)
                searchResults.clear()
                searchResults.addAll(results.take(SEARCH_RESULTS_DISPLAY_LIMIT))
                clearErrorReport()
            } catch (searchException: Exception) {
                if (searchException is CancellationException) {
                    // A cancelled search is composition teardown, not a demo
                    // failure: rethrow, never report (§6.3 false-positive rule).
                    throw searchException
                }
                // Exception, not SdkException alone: native-internal failures
                // (uniffi InternalException) are no SdkException variant, and
                // failing search must report, never crash (§6.3).
                searchResults.clear()
                reportFailureOf(searchException)
            } finally {
                searchInFlight = false
            }
        }
    }

    /** The consumer field word a no-match/refusal line names. */
    private fun planSearchFieldLabel(planSearchField: PlanSearchField): String =
        when (planSearchField) {
            PlanSearchField.ORIGIN -> "origin"
            PlanSearchField.DESTINATION -> "destination"
        }

    /**
     * One artifact demo place as a result row: its name is the display text
     * and its coordinate the resolved position. The quality is `Point`
     * because a demo place is a named feature point, never a geocoded
     * street address.
     */
    private fun placeFromDemoPlace(demoPlace: ReferenceSceneDemoPlace): Place = Place(
        structuredAddressText = demoPlace.name,
        coordinate = GeographicCoordinate(
            latitude = demoPlace.latitudeDegrees,
            longitude = demoPlace.longitudeDegrees,
        ),
        geocodeQuality = GeocodeQuality.POINT,
    )

    fun isPlaceAssigned(placeResult: Place): Boolean =
        listOf(originPlace, destinationPlace).any { assigned ->
            assigned?.coordinate == placeResult.coordinate
        }

    /**
     * Assigns one provider result row to the field the current search serves.
     * Assignment alone never plans (§4.1): the explicit Plan route action is
     * the only path that plans, on every platform.
     */
    fun assignSearchResult(placeResult: Place) {
        when (activePlanSearchField) {
            PlanSearchField.ORIGIN -> {
                originPlace = placeResult
                originSearchText = placeResult.structuredAddressText
            }
            PlanSearchField.DESTINATION -> {
                destinationPlace = placeResult
                destinationSearchText = placeResult.structuredAddressText
            }
        }
    }

    /** Whether the origin field holds a resolved place (the assignment stands). */
    val isOriginResolved: Boolean
        get() = originPlace != null

    /** Whether the destination field holds a resolved place (the assignment stands). */
    val isDestinationResolved: Boolean
        get() = destinationPlace != null

    /**
     * Whether Plan route is available: both fields hold text. The typed text
     * is resolved on press (assigned place → exact demo place → the provider's
     * first result), so the action never stays disabled merely because a
     * result row was not tapped (docs/ReferenceAppUX.md).
     */
    val canPlanRoute: Boolean
        get() = originSearchText.isNotBlank() && destinationSearchText.isNotBlank()

    /** The disabled-with-reason text under Plan route, empty when enabled. */
    val planRouteReasonText: String
        get() = when {
            originSearchText.isBlank() && destinationSearchText.isBlank() ->
                ReferenceApplicationConstants.PLAN_ROUTE_NEEDS_BOTH_FIELDS_TEXT
            originSearchText.isBlank() ->
                ReferenceApplicationConstants.PLAN_ROUTE_NEEDS_ORIGIN_TEXT
            destinationSearchText.isBlank() ->
                ReferenceApplicationConstants.PLAN_ROUTE_NEEDS_DESTINATION_TEXT
            else -> ""
        }

    /** The consumer field word the shared result list currently assigns to. */
    val activePlanSearchFieldLabel: String
        get() = planSearchFieldLabel(activePlanSearchField)

    /** Whether the field whose search produced the result list marks itself. */
    fun isPlanSearchFieldActive(planSearchField: PlanSearchField): Boolean =
        activePlanSearchField == planSearchField

    /** The search view's assignment hint: which field a tapped result fills. */
    val searchAssignmentHintText: String
        get() = ReferenceApplicationConstants.SEARCH_ASSIGNMENT_HINT_TEMPLATE
            .format(activePlanSearchFieldLabel)

    /**
     * The explicit Plan route action: resolves each field's typed text
     * (assigned place → exact demo place → the provider's first result) and
     * plans through the pair. The action is enabled while both fields hold
     * text; a field that resolves to nothing reports itself in the error strip
     * instead of leaving the action dead while the reader types.
     */
    fun planRouteFromPlanFields() {
        if (!canPlanRoute || routePlanInFlight) {
            return
        }
        coroutineScope.launch {
            try {
                val resolvedOrigin = resolvePlanField(PlanSearchField.ORIGIN) ?: return@launch
                val resolvedDestination = resolvePlanField(PlanSearchField.DESTINATION)
                    ?: return@launch
                // A session with no registered routing provider plans the
                // artifact's recorded route for the pair — never a provider
                // call, never invented geometry. A pair the artifact does not
                // record refuses with a named reason instead of failing
                // silently. Selecting a keyless routing provider (its public
                // demo deployment) registers one, so the same action then
                // plans through that provider.
                val useRecordedRoute = isProviderKeyless &&
                    !capabilityProviderState.hasRegisteredRoutingProvider
                if (useRecordedRoute) {
                    val recordedRoute = recordedDemoRoute(
                        resolvedOrigin.structuredAddressText,
                        resolvedDestination.structuredAddressText,
                    )
                    if (recordedRoute == null) {
                        reportMissingRecordedRoute(
                            resolvedOrigin.structuredAddressText,
                            resolvedDestination.structuredAddressText,
                        )
                        return@launch
                    }
                    applyRecordedDemoRoute(recordedRoute)
                    return@launch
                }
                planRoute(
                    waypoints = listOf(resolvedOrigin.coordinate, resolvedDestination.coordinate),
                    planNameText = resolvedDestination.structuredAddressText,
                )
            } catch (resolveException: Exception) {
                if (resolveException is CancellationException) {
                    throw resolveException
                }
                // Exception, not SdkException alone: failing resolution must
                // report, never crash (§6.3).
                reportFailureOf(resolveException)
            }
        }
    }

    /**
     * Resolves one plan field's text in the shared order and records the
     * resolved place on the field. A field that resolves to nothing reports
     * its own field word; the other field is left untouched.
     */
    private suspend fun resolvePlanField(planSearchField: PlanSearchField): Place? {
        val typedText = when (planSearchField) {
            PlanSearchField.ORIGIN -> originSearchText
            PlanSearchField.DESTINATION -> destinationSearchText
        }
        val assignedPlace = when (planSearchField) {
            PlanSearchField.ORIGIN -> originPlace
            PlanSearchField.DESTINATION -> destinationPlace
        }
        val demoPlaceNamed: (String) -> Place? = { placeName ->
            artifact.demoPlaces
                .firstOrNull { demoPlace -> demoPlace.name.equals(placeName, ignoreCase = true) }
                ?.let(::placeFromDemoPlace)
        }
        val resolvedPlace = PlanFieldResolver.resolve(
            typedText = typedText,
            assignedPlace = assignedPlace,
            demoPlaceNamed = demoPlaceNamed,
            providerFirstResult = { queryText ->
                // A keyless session has no geocoding provider: the demo places
                // are the only resolvable names, and an unmatched query is
                // reported honestly by the caller.
                if (isProviderKeyless) {
                    null
                } else {
                    sdkFacade.nativeSdk.forwardGeocode(queryText).firstOrNull()
                }
            },
        )
        if (resolvedPlace == null) {
            reportUnresolvedPlanField(planSearchField, typedText.trim())
            return null
        }
        when (planSearchField) {
            PlanSearchField.ORIGIN -> {
                originPlace = resolvedPlace
                originSearchText = resolvedPlace.structuredAddressText
            }
            PlanSearchField.DESTINATION -> {
                destinationPlace = resolvedPlace
                destinationSearchText = resolvedPlace.structuredAddressText
            }
        }
        return resolvedPlace
    }

    /** Reports a field whose typed text resolves to nothing, naming the field. */
    private fun reportUnresolvedPlanField(
        planSearchField: PlanSearchField,
        typedText: String,
    ) {
        val detailText = if (isProviderKeyless) {
            ReferenceApplicationConstants.DEMO_NO_MATCH_HINT_TEMPLATE
                .format(planSearchFieldLabel(planSearchField))
        } else {
            ReferenceApplicationConstants.PLAN_FIELD_NO_RESULT_TEMPLATE.format(typedText)
        }
        readinessReport.reportLocalFailure(
            variantLabel = "UnresolvedPlanField",
            detailText = detailText,
        )
    }

    /**
     * The Swap action: exchanges the two fields' text and resolved places so
     * an accidentally reversed pair is corrected in one tap. It never plans
     * and never searches.
     */
    fun swapPlanFields() {
        val previousOriginText = originSearchText
        val previousDestinationText = destinationSearchText
        val previousOriginPlace = originPlace
        val previousDestinationPlace = destinationPlace
        originSearchText = previousDestinationText
        destinationSearchText = previousOriginText
        originPlace = previousDestinationPlace
        destinationPlace = previousOriginPlace
        clearErrorReport()
    }

    /** The artifact's recorded route for the ordered demo-place pair, or null. */
    private fun recordedDemoRoute(
        originName: String,
        destinationName: String,
    ): ReferenceSceneDemoRoute? =
        artifact.demoRoutes.firstOrNull { demoRoute ->
            demoRoute.originName == originName && demoRoute.destinationName == destinationName
        }

    /**
     * Applies one recorded demo route as the planned route: the contract's
     * recorded `Route` feeds the simulated trace directly and the route
     * preview framing, exactly as a live plan does — no provider call.
     */
    private fun applyRecordedDemoRoute(recordedRoute: ReferenceSceneDemoRoute) {
        plannedRoute = PlannedRoute(
            route = recordedRoute.route,
            simulatedTrace = SimulatedTrace.fromRouteData(
                route = recordedRoute.route,
                traceName = ReferenceApplicationConstants.PLANNED_ROUTE_TRACE_NAME,
            ),
            nameText = recordedRoute.destinationName,
            isRecordedDemoRoute = true,
        )
        applyRoutePreviewFraming(recordedRoute.route)
        clearErrorReport()
    }

    /** Reports a keyless pair with no recorded route, naming both places. */
    private fun reportMissingRecordedRoute(originName: String, destinationName: String) {
        readinessReport.reportLocalFailure(
            variantLabel = "MissingRecordedRoute",
            detailText = ReferenceApplicationConstants.NO_RECORDED_ROUTE_HINT_TEMPLATE
                .format(originName, destinationName),
        )
    }

    var routePlanInFlight: Boolean by mutableStateOf(false)
        private set

    /**
     * The route options the last plan call carried (the Directions choices in
     * effect when it ran), or null before the first plan. The diagnostics and
     * the UI suite read it back to prove the selected mode reached the SDK.
     */
    var lastPlanRouteOptions: RouteOptions? by mutableStateOf(null)
        private set

    /** The plan calls avoid nothing, so the route follows the provider's fastest path. */
    private val referenceAvoidOptions: AvoidOptions
        get() = AvoidOptions(
            tollRoads = false,
            limitedAccessHighways = false,
            ferries = false,
            unpavedRoads = false,
        )

    /**
     * The routing options of every plan on this shell: the Directions
     * control's selected mode and strategy, metric units (§4.1's shared
     * option set). Replacing the fixed pedestrian value is the fix for the
     * ferry-route defect the Directions control addresses.
     */
    private val referenceRouteOptions: RouteOptions
        get() = directionsState.routeOptions(
            unitSystem = UnitSystem.METRIC,
            avoidOptions = referenceAvoidOptions,
        )

    /**
     * Plans the route through the two resolved waypoints, builds the simulated
     * trip source over its own shape, fits the preview, and returns the sheet
     * to half showing the route summary card (§4.1).
     */
    private fun planRoute(waypoints: List<GeographicCoordinate>, planNameText: String) {
        if (routePlanInFlight) {
            return
        }
        routePlanInFlight = true
        coroutineScope.launch {
            try {
                val planRouteOptions = referenceRouteOptions
                lastPlanRouteOptions = planRouteOptions
                val route = sdkFacade.nativeSdk.calculateRoute(waypoints, planRouteOptions)
                val tripTrace = SimulatedTrace.fromRouteData(
                    route = route,
                    traceName = ReferenceApplicationConstants.PLANNED_ROUTE_TRACE_NAME,
                )
                plannedRoute = PlannedRoute(
                    route = route,
                    simulatedTrace = tripTrace,
                    nameText = planNameText,
                    isRecordedDemoRoute = false,
                )
                applyRoutePreviewFraming(route)
                capabilityProviderState.clearRoutingFailure()
                clearErrorReport()
            } catch (routeException: Exception) {
                if (routeException is CancellationException) {
                    // A cancelled route plan is composition teardown, not a
                    // demo failure: rethrow, never report (false positive rule).
                    throw routeException
                }
                // Exception, not SdkException alone: native-internal failures
                // (uniffi InternalException) are no SdkException variant, and
                // failing routing must report, never crash (§6.3). No
                // recorded route and no retry stand in: the error strip
                // carries the provider's own failure verbatim, and an
                // unreachable endpoint adds the retry-or-own-deployment next
                // step to the routing row.
                plannedRoute = null
                if (routeException is SdkException.NetworkRequestFailed) {
                    capabilityProviderState.noteRoutingUnreachable()
                }
                reportFailureOf(routeException)
            } finally {
                routePlanInFlight = false
            }
        }
    }

    /** The route summary card's title: the planned destination, or the demo plan. */
    val routeSummaryTitleText: String
        get() = plannedRoute?.nameText ?: selectedDemoPlan.label

    /**
     * The trip source Start will replay: the planned route when one stands,
     * otherwise the selected demo plan. The peek states it so a start without a
     * plan is never mistaken for a live route.
     */
    val tripSourceText: String
        get() = plannedRoute?.let { planned ->
            ReferenceApplicationConstants.PLAN_TRIP_SOURCE_PLANNED_ROUTE_TEMPLATE
                .format(planned.nameText)
        } ?: ReferenceApplicationConstants.PLAN_TRIP_SOURCE_DEMO_PLAN_TEMPLATE
            .format(selectedDemoPlan.label)

    var isNavigationInTrip: Boolean by mutableStateOf(false)
        private set

    // -------------------------------------------------------------------
    // Simulated turn-by-turn navigation (§7)
    // -------------------------------------------------------------------

    /** The phase chip's current text (§7.1: rendered as text, never color alone). */
    var navigationPhaseChip: String by mutableStateOf("")
        private set

    /** The speech topic's subscription state (OQ5, round 10): drives the
     * speak path between the topic delivery and the round-9 payload fallback. */
    var hasSpeechTopicSubscription: Boolean by mutableStateOf(false)
        private set

    /**
     * Starts the simulated trip on the active plan (the route summary's primary
     * act): the follow camera engages at trip start (§5.3) — the fixed deep
     * zoom applies immediately, the 3D pitch swings over the pump's ease —
     * and the trip's fixes drive center/bearing/zoom-ramp afterward. A start
     * failure leaves the home sheet with the verbatim report, never a
     * half-started navigation mode.
     */
    fun startSimulatedTrip() {
        // One trip at a time: a device-driven trip (and only the location
        // source it started) ends before the replay starts.
        stopDeviceNavigation()
        tripDriver.startTrip(coroutineScope, activeSimulatedTrace)
        if (tripDriver.startFailure != null) {
            reportFailure(tripDriver.startFailure!!)
            return
        }
        isNavigationInTrip = true
        hasSpeechTopicSubscription = true
        followCamera.engageTrip(
            currentCenterLatitudeDegrees = centerLatitudeDegrees,
            currentCenterLongitudeDegrees = centerLongitudeDegrees,
            currentZoomLevel = zoomLevel,
            currentBearingDegrees = bearingDegrees,
            currentPitchDegrees = pitchDegrees,
        )
        syncFollowPhase()
        zoomLevel = ReferenceAppUXDimensions.NAVIGATION_FOLLOW_ZOOM
        if (!isThreeDCameraEnabled) {
            isThreeDCameraEnabled = true
            animateCameraTo(currentCameraPose().copy(pitchDegrees = cameraPitchTargetDegrees()))
        } else {
            pushCameraImmediately()
        }
        clearErrorReport()
        navigationPhaseChip = navigationPhaseChipOf(
            currentPhase = navigationController.currentTripState()?.phase,
            phaseChange = null,
        )
    }

    /**
     * The drawer's exit control: stop the replay, restore the captured
     * pre-trip camera (§5.3: restored after), clear the location indicator
     * (the demo's simulated device stops reporting), return the sheet to peek.
     */
    fun exitSimulatedTrip() {
        // The drawer's exit control ends whichever trip is presented; a
        // device-driven one stops through its own path (the source stops
        // only when the mode started it).
        if (isDeviceNavigationActive) {
            stopDeviceNavigation()
            return
        }
        tripDriver.stopTrip()
        // The trip's speech lifecycle ends: the audio focus equivalent
        // abandons once the engine drained (an in-flight utterance keeps
        // it until its completion, round-11 B4).
        speechSpeaker.releaseJourneySpeech()
        isNavigationInTrip = false
        hasSpeechTopicSubscription = false
        navigationPhaseChip = ""
        restoreFollowCamera()
        clearLocationIndicatorOnSurface()
    }

    /** Whether the simulated trip's playback is paused at its current fix. */
    val isSimulatedTripPaused: Boolean
        get() = tripDriver.isTripPaused

    /** The playback speed multiplier the drawer's control selected (M27). */
    val simulatedTripSpeedMultiplier: Double
        get() = tripDriver.speedMultiplier

    /**
     * The drawer's speed control act: selects one of the named multipliers.
     * A route-following trip rebuilds its walker at the new pace and keeps
     * the travelled distance, so a mid-trip speed change never teleports the
     * position.
     */
    fun selectSimulatedTripSpeedMultiplier(multiplier: Double) {
        tripDriver.selectSpeedMultiplier(multiplier)
        persistRememberedChoices()
    }

    /** The drawer's pause control act. */
    fun pauseSimulatedTrip() {
        tripDriver.pauseTrip()
    }

    /** The drawer's resume control act (the same control once paused). */
    fun resumeSimulatedTrip() {
        tripDriver.resumeTrip()
    }

    /** Whether the presented trip walks the route's own geometry (M27's seek). */
    val isSimulatedTripSeekable: Boolean
        get() = tripDriver.isSeekable

    /** The presented trip's current distance along the route, in meters. */
    val simulatedTripTravelledMeters: Double
        get() = tripDriver.travelledDistanceMeters

    /** The presented trip's route length, in meters (the seek control's range). */
    val simulatedTripSeekableDistanceMeters: Double
        get() = tripDriver.seekableDistanceMeters

    /**
     * The drawer's seek act (M27): moves the trip to [travelledMeters] along
     * the route through the walker, so every presentation follows the fix.
     */
    fun seekSimulatedTrip(travelledMeters: Double) {
        tripDriver.seekToTravelledMeters(travelledMeters)
    }

    /**
     * The simulated replay's playback end without a user exit.
     *
     * An arrival keeps the destination presentation: the trip stays
     * presented with the destination centred and the indicator on the
     * tracking dot, and only the drawer's exit control empties it (§7.1/§7.2:
     * the arrival state replaces the drawer's contents and is not a snap
     * back). Any other end (a recorded trace whose samples drained behind a
     * reroute) restores the captured pre-trip camera as before.
     */
    fun onSimulatedPlaybackEnded() {
        // The trip's speech lifecycle ended with the playback: the audio
        // focus equivalent abandons once the engine drained (an in-flight
        // utterance keeps it until its completion, round-11 B4).
        speechSpeaker.releaseJourneySpeech()
        if (!isNavigationInTrip) {
            // A user exit (or composition) handled its own restore already;
            // only the arrival path lands here with the trip still presented.
            return
        }
        val arrivedFix = latestLocationFix
        if (tripDriver.hasArrived) {
            // The arrival state stays visible: the destination remains the
            // camera centre (the trip does not snap back to the pre-trip
            // framing) and the indicator turns into the standing tracking dot.
            val arrivedCoordinate = arrivedFix?.snappedCoordinate
            if (arrivedCoordinate != null) {
                centerLatitudeDegrees = arrivedCoordinate.latitude
                centerLongitudeDegrees = arrivedCoordinate.longitude
                pushCameraImmediately()
            }
            if (arrivedFix != null) {
                pushLocationIndicator(guided = false, fix = arrivedFix)
            }
            return
        }
        restoreFollowCamera()
        if (arrivedFix != null) {
            pushLocationIndicator(guided = false, fix = arrivedFix)
        }
    }

    /** Restores the captured pre-trip camera and the 2D plane (§5.3). */
    private fun restoreFollowCamera() {
        val restorableCamera = followCamera.restorePreTripCamera()
        syncFollowPhase()
        if (restorableCamera == null) {
            return
        }
        centerLatitudeDegrees = restorableCamera.centerLatitudeDegrees
        centerLongitudeDegrees = restorableCamera.centerLongitudeDegrees
        zoomLevel = restorableCamera.zoomLevel
        bearingDegrees = restorableCamera.bearingDegrees
        pitchDegrees = restorableCamera.pitchDegrees
        isThreeDCameraEnabled = false
        // The restored values are a camera set, which invalidates the shipped
        // view's cached camera and resumes its pump, so the restored framing
        // is actually redrawn.
        pushCameraImmediately()
    }

    /** The remaining-distance fraction of the active trip's route (§5.3's clamp input). */
    private fun remainingDistanceFractionOf(totalRouteDistanceMeters: Double): Double {
        val progress = tripDriver.latestTripState?.progress
        val remainingMeters = progress?.distanceRemainingMeters ?: totalRouteDistanceMeters
        return if (totalRouteDistanceMeters > 0.0) {
            remainingMeters / totalRouteDistanceMeters
        } else {
            1.0
        }
    }

    /**
     * One processed simulated fix (the driver's delivery callback): the
     * follow camera's targets update on every fix (§5.3), but the camera only
     * moves while follow is engaged — a gesture-broken follow keeps the
     * user's panned view until the my-location control re-engages it. The
     * renderer-owned location indicator follows the fix in the guided wedge
     * form regardless: it reports the device's position, not the camera's.
     * A fix delivered after the playback's end (a trailing sample behind the
     * arrival) must not re-engage the restored follow camera — the guard is
     * the driver's playback flag, and the late fix switches the indicator to
     * the tracking dot.
     */
    fun handleSimulatedProcessedFix(processedFix: SimulatedProcessedFix) {
        val snappedCoordinate = processedFix.snappedCoordinate ?: return
        val totalRouteDistanceMeters = activeSimulatedTrace.route.distanceMeters
        val remainingDistanceFraction = remainingDistanceFractionOf(totalRouteDistanceMeters)
        if (isNavigationInTrip && tripDriver.isTripActive) {
            followCamera.onFix(
                latitudeDegrees = snappedCoordinate.latitude,
                longitudeDegrees = snappedCoordinate.longitude,
                courseHeadingDegrees = processedFix.courseHeadingDegrees,
                remainingDistanceFraction = remainingDistanceFraction,
            )
            syncFollowPhase()
            if (followPhase == FollowPhase.ENGAGED) {
                centerLatitudeDegrees = followCamera.targetCenterLatitudeDegrees()
                centerLongitudeDegrees = followCamera.targetCenterLongitudeDegrees()
                zoomLevel = followCamera.targetZoomLevel()
                // The center and zoom follow each fix exactly; only the
                // bearing is eased toward the median-smoothed course.
                pushCameraImmediately()
                val targetBearingDegrees = followCamera.targetBearingDegrees()
                if (targetBearingDegrees != null) {
                    mapView?.animateCamera(
                        currentCameraPose().copy(bearingDegrees = targetBearingDegrees),
                        FOLLOW_BEARING_ANIMATION_DURATION_MILLIS,
                    )
                }
            }
        }
        // The guided wedge rides the *playing* trip (a trailing fix behind
        // the playback's end arrives during the arrived presentation, where
        // §7.1 shows the tracking dot, not the guided wedge).
        pushLocationIndicator(guided = tripDriver.isTripActive, fix = processedFix)
    }

    /**
     * Recomputes the phase chip from the phase topic and the controller's
     * read-only state; the screen's phase-refresh effect calls this after
     * every phase-topic update so the chip never drifts from the state form.
     */
    fun refreshNavigationPhaseChip() {
        navigationPhaseChip = navigationPhaseChipOf(
            currentPhase = navigationController.currentTripState()?.phase,
            phaseChange = navigationListener.phaseChangeState.value,
        )
    }

    /** The verbatim detail text of the current trip-delivery failure, if any. */
    fun tripDriverFailureDetailText(): String? =
        tripDriver.tripDeliveryFailure?.let(::failureDetailTextOf)

    /** Maps the phase topic onto the chip text (§7.2: the chip is the state form). */
    private fun navigationPhaseChipOf(
        currentPhase: NavigationPhase?,
        phaseChange: NavigationPhaseChange?,
    ): String = when {
        // The walker's own arrival report is authoritative for a
        // route-following trip whose reducer state lags the final fix.
        tripDriver.hasArrived -> ReferenceApplicationConstants.NAVIGATION_PHASE_LABEL_ARRIVED
        currentPhase == NavigationPhase.ARRIVED ->
            ReferenceApplicationConstants.NAVIGATION_PHASE_LABEL_ARRIVED
        currentPhase == NavigationPhase.OFF_ROUTE ->
            ReferenceApplicationConstants.NAVIGATION_PHASE_LABEL_OFF_ROUTE
        currentPhase == NavigationPhase.NAVIGATING ->
            ReferenceApplicationConstants.NAVIGATION_PHASE_LABEL_NAVIGATING
        phaseChange == NavigationPhaseChange.OFF_ROUTE_ENTERED ->
            ReferenceApplicationConstants.NAVIGATION_PHASE_LABEL_OFF_ROUTE
        phaseChange == NavigationPhaseChange.OFF_ROUTE_CLEARED ->
            ReferenceApplicationConstants.NAVIGATION_PHASE_LABEL_NAVIGATING
        phaseChange == NavigationPhaseChange.ARRIVED ->
            ReferenceApplicationConstants.NAVIGATION_PHASE_LABEL_ARRIVED
        phaseChange == NavigationPhaseChange.REROUTE_APPLIED ->
            navigationPhaseChip.ifEmpty {
                ReferenceApplicationConstants.NAVIGATION_PHASE_LABEL_NAVIGATING
            }
        else -> if (tripDriver.isTripActive) {
            ReferenceApplicationConstants.NAVIGATION_PHASE_LABEL_NAVIGATING
        } else {
            ""
        }
    }

    // -------------------------------------------------------------------
    // Per-draw tile statistics (round 9: the draw seam's accounting)
    // -------------------------------------------------------------------

    /** The latest frame's tile statistics, fed by every drawn frame. */
    var latestTileStatistics: uniffi.rustic_maps.FrameStatistics? by mutableStateOf(null)
        private set

    /** Frame counters of the demo (the diagnostics sheet's frame facts). */
    var framesDrawnCount: Long by mutableStateOf(0L)
        private set
    var framesDeferredCount: Long by mutableStateOf(0L)
        private set
    var framesFailedCount: Long by mutableStateOf(0L)
        private set

    /**
     * Pump frames the idle-settled query answered true on (round 10): the
     * presented frame stood while nothing changed — the diagnostics sheet's
     * honest accounting of the redraw discipline.
     */
    var framesSettledCount: Long by mutableStateOf(0L)
        private set

    // -------------------------------------------------------------------
    // Offline maps (§4.1's card over the registered tile cache)
    // -------------------------------------------------------------------

    /** The generated manager every region member is called through. */
    private val offlineManager: OfflineManager = OfflineManager(nativeMapSdk)

    /**
     * The offline card's region-download flow: the demo region from the
     * shared artifact, live progress, cancel, the stored-region list and
     * deletion. The listener hops onto this state's scope (the main
     * dispatcher) before touching snapshot state.
     */
    val offlineRegionState: OfflineRegionCardState = OfflineRegionCardState(
        coroutineScope = coroutineScope,
        regionTileCount = { region -> offlineManager.regionTileCount(region) },
        downloadRegion = { region, listener ->
            offlineManager.downloadRegion(region, listener)
        },
        cancelRegionDownload = { regionId ->
            offlineManager.cancelRegionDownload(regionId)
        },
        listRegionRecords = { offlineManager.regionRecords() },
        deleteRegion = { regionId -> offlineManager.deleteRegion(regionId) },
        failureText = ::failureDetailTextOf,
        reportFailure = ::reportFailureOf,
    )

    init {
        // The card states the SDK's own refusal until a credential installs a
        // tile cache; the registration path re-reads it afterwards.
        offlineRegionState.loadDemoRegion(artifact.offlineRegions.firstOrNull()?.region)
    }

    /** The registered tile cache's path, or null before the registration ran. */
    var tileCacheRegisteredPath: String? by mutableStateOf(null)
        private set

    /** The registered tile cache's absolute byte budget. */
    var tileCacheBudgetBytes: ULong? by mutableStateOf(null)
        private set

    /** The verbatim failure detail when the cache registration was refused. */
    var tileCacheFailureReason: String? by mutableStateOf(null)
        private set

    /**
     * Serializes tile-cache registrations: one credential application can ask
     * for two (the application path and the base-map selection that follows
     * it), and both open the same SQLite store. A single-lane background
     * dispatcher keeps them in request order without blocking the main thread.
     */
    private val tileCacheRegistrationDispatcher = Dispatchers.IO.limitedParallelism(
        parallelism = TILE_CACHE_REGISTRATION_LANES,
        name = "tile-cache-registration",
    )

    /**
     * Registers the provider set with the tile cache wrapped around the
     * raster-tile contract (the offline-maps demo): the cache file lives in
     * the app's caches directory under the named byte budget. Called only
     * after a credential application succeeded — the facade refuses a keyless
     * registration, and that refusal is a disabled-with-reason state (§6.2),
     * never silent.
     */
    private fun registerTileCacheAfterCredential() {
        // `java.io.File` rather than `java.nio.file.Paths`: the latter is API
        // 26+ and the application's minimum is API 24.
        val cacheDatabaseFilePath = java.io.File(
            tileCacheDirectoryPath,
            ReferenceApplicationConstants.TILE_CACHE_DATABASE_FILE_NAME,
        ).absolutePath
        // The registration opens and initialises the cache's SQLite store, so
        // it runs on a background dispatcher; the composed state is published
        // back on the main dispatcher after it settles, preserving the order
        // in which the applied-provider state was already updated.
        coroutineScope.launch {
            val registrationFailure = withContext(tileCacheRegistrationDispatcher) {
                try {
                    sdkFacade.registerTileCacheWithProviders(
                        tileCacheDatabasePath = cacheDatabaseFilePath,
                        tileCacheAbsoluteMaximumBytes =
                            ReferenceApplicationConstants.TILE_CACHE_ABSOLUTE_MAXIMUM_BYTES,
                    )
                    null
                } catch (cancellation: CancellationException) {
                    throw cancellation
                } catch (failure: Throwable) {
                    failure
                }
            }
            if (registrationFailure == null) {
                tileCacheRegisteredPath = cacheDatabaseFilePath
                tileCacheBudgetBytes =
                    ReferenceApplicationConstants.TILE_CACHE_ABSOLUTE_MAXIMUM_BYTES
                tileCacheFailureReason = null
            } else {
                tileCacheRegisteredPath = null
                tileCacheBudgetBytes = null
                tileCacheFailureReason = failureDetailTextOf(registrationFailure)
                reportFailureOf(registrationFailure)
            }
            // The card re-reads the region count and the stored-region list
            // against the cache state this registration just installed (or
            // against the refusal it just reported).
            offlineRegionState.loadDemoRegionAfterTileCacheRegistration(
                artifact.offlineRegions.firstOrNull()?.region,
            )
        }
    }

    /** The offline-maps card's cache state line (§4.1's two honest lines). */
    val offlineCacheStateText: String
        get() {
            val registeredPath = tileCacheRegisteredPath
            return if (registeredPath == null) {
                tileCacheFailureReason ?: ReferenceApplicationConstants.OFFLINE_MAPS_NOT_REGISTERED_TEXT
            } else {
                "Cache registered: $registeredPath · " +
                    "${tileCacheBudgetBytes ?: 0UL} bytes budget"
            }
        }

    /** The offline-maps card's live tile line from the latest frame's statistics. */
    val offlineLiveTileLineText: String
        get() {
            val statistics = latestTileStatistics
            return if (statistics == null) {
                ReferenceApplicationConstants.OFFLINE_TILES_PENDING_TEXT
            } else {
                "Tiles: requested ${statistics.requestedTileCount} · drawn " +
                    "${statistics.presentTileCount} · fallback ${statistics.fallbackTileCount} · " +
                    "deferred ${statistics.deferredTileCount} · missing ${statistics.unavailableTileCount}"
            }
        }

    /** Whether the offline archive route plan is in flight (the control's disabled state). */
    var offlineRoutePlanInFlight: Boolean by mutableStateOf(false)
        private set

    /**
     * The offline card's route control (§2.5 `offline.plan_route_action`):
     * reads the committed compiled routing archive from the app's assets and
     * plans the demo request over it through the contract's
     * `offlineRouteFromArchiveBytes` free function. The route feeds the same
     * summary card, overlay line and simulated trip as a live plan, and the
     * summary states that it is a recorded test graph, not live data. No
     * provider and no network round trip are involved.
     */
    fun planOfflineRouteFromArchive() {
        if (offlineRoutePlanInFlight) {
            return
        }
        val archiveBytes = try {
            assetManager.open(ReferenceApplicationConstants.OFFLINE_ROUTING_ARCHIVE_ASSET_NAME)
                .use { archiveStream -> archiveStream.readBytes() }
        } catch (archiveException: Exception) {
            // A missing or unreadable bundled archive reports verbatim; it is
            // a build-asset failure, never a silent no-op.
            reportFailureOf(archiveException)
            return
        }
        offlineRoutePlanInFlight = true
        coroutineScope.launch {
            try {
                val route = offlineRouteFromArchiveBytes(
                    archiveBytes = archiveBytes,
                    start = GeographicCoordinate(
                        latitude = ReferenceApplicationConstants.OFFLINE_ROUTE_START_LATITUDE_DEGREES,
                        longitude = ReferenceApplicationConstants.OFFLINE_ROUTE_START_LONGITUDE_DEGREES,
                    ),
                    end = GeographicCoordinate(
                        latitude = ReferenceApplicationConstants.OFFLINE_ROUTE_END_LATITUDE_DEGREES,
                        longitude = ReferenceApplicationConstants.OFFLINE_ROUTE_END_LONGITUDE_DEGREES,
                    ),
                )
                plannedRoute = PlannedRoute(
                    route = route,
                    simulatedTrace = SimulatedTrace.fromRouteData(
                        route = route,
                        traceName = ReferenceApplicationConstants.PLANNED_ROUTE_TRACE_NAME,
                    ),
                    nameText = ReferenceApplicationConstants.OFFLINE_ROUTE_TITLE_TEXT,
                    isRecordedDemoRoute = false,
                    isOfflineArchiveRoute = true,
                )
                applyRoutePreviewFraming(route)
                clearErrorReport()
            } catch (routeException: Exception) {
                if (routeException is CancellationException) {
                    // A cancelled plan is composition teardown, not a demo
                    // failure: rethrow, never report (the false-positive rule).
                    throw routeException
                }
                // A refused archive (invalid bytes, an out-of-graph
                // coordinate, a disconnected pair) reports its verbatim
                // detail through the existing error strip (§6.3).
                plannedRoute = null
                reportFailureOf(routeException)
            } finally {
                offlineRoutePlanInFlight = false
            }
        }
    }

    // -------------------------------------------------------------------
    // Surface lifecycle reporting (§6.1)
    // -------------------------------------------------------------------

    /**
     * Detail text of any failure: declared SdkException variants report their
     * mapped fields; native-internal failures (uniffi raises wgpu and other
     * Rust-side errors as InternalException, which is no SdkException variant)
     * report their class name and message verbatim (§6.3).
     */
    private fun failureDetailTextOf(failure: Throwable): String =
        readinessReport.failureDetailTextOf(failure)

    private fun reportFailureOf(failure: Throwable) =
        readinessReport.reportFailure(failure)

    fun retrySurfaceAttach() {
        mapView?.retryAttach()
        readinessReport.prepareSurfaceRetry()
    }

    // -------------------------------------------------------------------
    // Spoken guidance (the platform TextToSpeech seam, §4.1/§6.3)
    // -------------------------------------------------------------------

    /**
     * Voices the latest instruction payload's speech channel verbatim (the
     * demo runs the speaker from the screen's instruction effect when a new
     * payload arrives); the speech toggle lives in the navigation drawer.
     * The arrival state's payload carries no speech text, so arrival is
     * never spoken with invented words — the drawer's arrival card presents
     * it (§6.3's verbatim rule applied to speech).
     */
    fun speakGuidanceIfNeeded() {
        val instruction = navigationListener.instructionState.value ?: return
        val arriving = instruction.isArrival ||
            tripDriver.latestTripState?.phase == NavigationPhase.ARRIVED
        if (arriving) {
            speechSpeaker.noteArrivalWithoutSpeech()
        } else {
            speechSpeaker.speakInstruction(
                instruction.activeManeuver.instruction.providerSpeechText,
                null,
            )
        }
    }

    /**
     * Voices guidance per the round-10 speech seam (OQ5, SDK-owned
     * triggering): when the speech topic is subscribed (the round-10
     * reference subscription), only the topic's own utterance deliveries
     * speak — the controller's distance-band policy decided the fix was
     * newsworthy, and the platform owns only playback (verbatim,
     * `provider_*` speech channel, engine stop + QUEUE_FLUSH pacing — the
     * round-9 over-talking fix stays). When no listener is subscribed, the
     * platform fallback keeps speaking the latest payload line (§4.1's
     * fallback), unchanged from round 9.
     */
    fun speakTopicGuidanceIfNeeded() {
        if (!hasSpeechTopicSubscription) {
            speakGuidanceIfNeeded()
            return
        }
        val speechInstruction = navigationListener.speechInstructionState.value ?: return
        if (speechInstruction.instructionKind == SpeechInstructionKind.ARRIVAL) {
            speechSpeaker.noteArrivalWithoutSpeech()
            return
        }
        speechSpeaker.speakInstruction(
            speechInstruction.utteranceSpeechText,
            speechInstruction.utteranceId,
        )
    }

    val speechEngineReady: Boolean
        get() = speechSpeaker.speechEngineReady

    val speechEngineUnavailableReason: String?
        get() = speechSpeaker.speechEngineUnavailableReason

    fun requestSpeechEnabled(enabled: Boolean) {
        speechSpeaker.requestSpeechEnabled(enabled)
    }

    val isSpeechEnabled: Boolean
        get() = speechSpeaker.speechEnabled

    private companion object {
        /** The demo search shows at most five results. */
        const val SEARCH_RESULTS_DISPLAY_LIMIT: Int = 5

        /**
         * One lane for the tile-cache registration: the same credential
         * application can request the registration twice (the application
         * path and the base-map selection that follows it), and both open the
         * same SQLite store, so they must not run concurrently.
         */
        const val TILE_CACHE_REGISTRATION_LANES: Int = 1

        /**
         * The follow camera's eased bearing move (§5.3's eased bearing):
         * 150 ms — short enough that each fix's course step lands within the
         * demo's fix cadence while riding out the walk demo's stepwise course
         * noise. Named here because §2.4's token table fixes the control
         * easings but leaves the follow chase's smoothing to the platform.
         */
        const val FOLLOW_BEARING_ANIMATION_DURATION_MILLIS: Long = 150L

        /** The 2D plane's pitch (structural: top-down). */
        const val TWO_D_CAMERA_PITCH_DEGREES: Double = 0.0

        /** The compass's re-north target (structural: north up). */
        const val NORTH_UP_BEARING_DEGREES: Double = 0.0

        /** The camera zoom applied when the device position first centres the map. */
        const val DEVICE_LOCATION_ZOOM_LEVEL: Double = 15.0
    }
}

/** The demo-plan picker's one selectable demo plan (a loaded recorded trace). */
data class DemoPlan(
    /** Consumer-facing plan name ("Demo walk", never fixture paths — §4.2). */
    val label: String,
    /** The plan's info affordance text (behind the info affordance, §4.1). */
    val descriptionText: String,
    /** The loaded trace the plan replays. */
    val trace: SimulatedTrace,
    /** Whether this plan is the artifact's own navigation demo trace. */
    val isArtifactTrace: Boolean,
) {
    /** The plan's scenario name from the trace metadata (diagnostics only). */
    val scenarioName: String
        get() = trace.traceName
}

/** The search-planned route: the planned route plus its simulated-trip source. */
data class PlannedRoute(
    /** The route the SDK calculated (or the artifact recorded) through the pair. */
    val route: Route,
    /** The simulated trace derived from the route's own shape. */
    val simulatedTrace: SimulatedTrace,
    /** The planned destination's display text (the route summary card's title). */
    val nameText: String,
    /**
     * Whether this route is the shared artifact's recorded demo route rather
     * than a live planning result: the route summary card says so.
     */
    val isRecordedDemoRoute: Boolean,
    /**
     * Whether this route was planned over the bundled offline test graph:
     * the route summary card says it is recorded test data, not live data.
     */
    val isOfflineArchiveRoute: Boolean = false,
)

/**
 * One plan field's type-ahead snapshot: what the field renders below its text.
 *
 * [suggestions] is never longer than the shared row cap. When the provider
 * answered with no match, [showsNoMatchRow] is true and the field renders one
 * "no matches" row instead of an empty list. [failureText] is the provider's
 * one inline sentence, shown while the field keeps its text; a cancelled
 * request contributes neither.
 */
data class PlanAutocompleteViewState(
    /** The provider's suggestion rows, in its own relevance order. */
    val suggestions: List<Place>,
    /** Whether the one no-match row stands in for an empty answer. */
    val showsNoMatchRow: Boolean,
    /** The provider failure's one inline sentence, or null. */
    val failureText: String?,
    /** Whether the suggestion list (rows or the no-match row) renders. */
    val isVisible: Boolean,
) {
    /** The number of rows the field renders (never more than the shared cap). */
    val renderedRowCount: Int
        get() = if (isVisible) suggestions.size + (if (showsNoMatchRow) 1 else 0) else 0
}

/**
 * One plan field's mutable type-ahead state: the rows it renders and the
 * request that produced them. [generation] invalidates a superseded request
 * whose answer arrives after a newer query started.
 */
private class PlanAutocompleteFieldState {
    var suggestions: List<Place> by mutableStateOf(emptyList())
    var showsNoMatchRow: Boolean by mutableStateOf(false)
    var failureText: String? by mutableStateOf(null)
    var isVisible: Boolean by mutableStateOf(false)
    var generation: Long = 0L
    var requestJob: Job? = null
}

/**
 * Lowest zoom level a credential-less base map declares. The registry's
 * raster descriptors publish their ceiling only, and every web-mercator XYZ
 * tileset starts at zoom zero — the level the whole world fits one tile.
 */
private const val BASE_MAP_MINIMUM_ZOOM_LEVEL: UByte = 0u

/**
 * One base-map row of the settings sheet: the shared registry's descriptor
 * plus whether it can serve right now. A credential-registered provider is
 * offered before its key is applied and carries `isServable = false` until
 * then, so the row can say plainly what it needs instead of vanishing.
 */
data class BaseMapChoice(
    /** The descriptor the row renders and registers from. */
    val descriptor: ProviderCredentialDescriptor,
    /** Whether selecting the row can register a source today. */
    val isServable: Boolean,
) {
    /** The registry key the row and its selection are addressed by. */
    val providerKey: String
        get() = descriptor.providerKey
}

/**
 * One styled vector base the settings surface offers (M22): a registry
 * descriptor's vector tileset painted with one of the committed styles the
 * same descriptor lists. Identity is the pair — one provider serves several
 * styles — so the choice key names both halves.
 */
data class VectorBaseMapChoice(
    /** The descriptor whose tileset and zoom ceiling the base uses. */
    val descriptor: ProviderCredentialDescriptor,
    /** The committed style the base paints with. */
    val style: VectorStyleDescriptor,
    /**
     * Whether the choice can install now. False while a credentialed family's
     * key is not applied: the row is listed with its `Needs <credential
     * label>` state, exactly as a credential-less raster row is.
     */
    val isServable: Boolean = true,
) {
    /** Stable identity of this choice (`provider/style`). */
    val choiceKey: String
        get() = "${descriptor.providerKey}/${style.styleId}"

    /** The consumer-facing name the base-map row shows. */
    val displayName: String
        get() = "${descriptor.displayName} · ${style.displayName}"

    /** The attribution line the base must carry: the style's own design line. */
    val attributionText: String
        get() = style.attributionText.ifBlank { descriptor.attributionText }
}