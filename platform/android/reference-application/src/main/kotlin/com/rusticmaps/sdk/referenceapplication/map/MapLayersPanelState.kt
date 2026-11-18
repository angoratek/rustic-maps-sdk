package com.rusticmaps.sdk.referenceapplication.map

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.rusticmaps.sdk.ProviderCredentialDescriptor
import com.rusticmaps.sdk.referenceapplication.BaseMapChoice
import com.rusticmaps.sdk.referenceapplication.ReferenceApplicationConstants
import com.rusticmaps.sdk.referenceapplication.VectorBaseMapChoice
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import uniffi.rustic_maps.BoundingBox
import uniffi.rustic_maps.SdkException
import uniffi.rustic_maps.TrafficFlowCacheStatistics
import uniffi.rustic_maps.TrafficFlowData

/**
 * The base-map row's selection: a registry descriptor's raster source or one
 * committed vector style (docs/ReferenceAppUX.md §4.2).
 */
sealed class MapLayerBaseSelection {
    /** A registry descriptor's raster source, addressed by its provider key. */
    data class Raster(val providerKey: String) : MapLayerBaseSelection()

    /** A registry descriptor's committed vector style, addressed by choice key. */
    data class Vector(val choiceKey: String) : MapLayerBaseSelection()
}

/**
 * One base-map row of the layers panel: a raster descriptor or one committed
 * vector style. The identifier is the specification's shared test handle.
 */
data class MapLayerBaseRow(
    /** The row's accessibility identifier (`map-layer-base-…` or `map-layer-style-…`). */
    val identifier: String,
    /** The consumer-facing row name. */
    val displayName: String,
    /** The selection the row performs. */
    val selection: MapLayerBaseSelection,
    /** Whether the row's base currently serves the map. */
    val isServing: Boolean,
    /** The disabled-with-reason text, or null while the row is usable. */
    val disabledReasonText: String?,
) {
    /** The row's rendered button label. */
    val buttonLabel: String
        get() = when {
            disabledReasonText != null -> "$displayName — $disabledReasonText"
            isServing -> "$displayName — " +
                ReferenceApplicationConstants.BASE_MAP_ACTIVE_STATUS_TEXT
            else -> displayName
        }
}

/**
 * The traffic switch's availability, resolved from the shared registry: a
 * descriptor serves the capability with the credentials in force, a declared
 * descriptor still needs its credential, or no descriptor declares it.
 */
sealed class MapLayerTrafficAvailability {
    /** A descriptor serves the capability with the credentials in force. */
    data class Available(
        val providerKey: String,
        val attribution: String,
    ) : MapLayerTrafficAvailability()

    /** A declared descriptor still needs its credential. */
    data class NeedsCredential(val reason: String) : MapLayerTrafficAvailability()

    /** No descriptor declares the capability. */
    data class NotConfigured(val reason: String) : MapLayerTrafficAvailability()

    /** Whether the switch can be turned on. */
    val isAvailable: Boolean
        get() = this is Available

    /** The disabled reason the switch renders, or null while it is available. */
    val reasonText: String?
        get() = when (this) {
            is Available -> null
            is NeedsCredential -> reason
            is NotConfigured -> reason
        }

    /** The serving descriptor's attribution line while it is available. */
    val attributionText: String?
        get() = (this as? Available)?.attribution
}

/**
 * The panel's device-location row states, exactly the specification's
 * wording.
 */
enum class DeviceLocationPanelStatus(
    /** The panel row's presentation text. */
    val displayText: String,
    /** Whether the source is updating (the row's Stop action is shown). */
    val isSourceRunning: Boolean,
    /** The reason the source cannot start, or null while it can. */
    val cannotStartReasonText: String?,
) {
    /** No source is running. */
    OFF(
        displayText = ReferenceApplicationConstants.DEVICE_LOCATION_PANEL_OFF_TEXT,
        isSourceRunning = false,
        cannotStartReasonText = null,
    ),

    /** The source is running and no fix arrived yet. */
    WAITING_FOR_FIX(
        displayText = ReferenceApplicationConstants.DEVICE_LOCATION_PANEL_WAITING_TEXT,
        isSourceRunning = true,
        cannotStartReasonText = null,
    ),

    /** A fix arrived; the row reads Active. */
    ACTIVE(
        displayText = ReferenceApplicationConstants.DEVICE_LOCATION_PANEL_ACTIVE_TEXT,
        isSourceRunning = true,
        cannotStartReasonText = null,
    ),

    /** The platform refused the permission. */
    PERMISSION_DENIED(
        displayText = ReferenceApplicationConstants.DEVICE_LOCATION_PANEL_PERMISSION_DENIED_TEXT,
        isSourceRunning = false,
        cannotStartReasonText =
            ReferenceApplicationConstants.DEVICE_LOCATION_PANEL_PERMISSION_DENIED_TEXT,
    ),

    /** Location services are unavailable on this device. */
    UNAVAILABLE(
        displayText = ReferenceApplicationConstants.DEVICE_LOCATION_PANEL_UNAVAILABLE_TEXT,
        isSourceRunning = false,
        cannotStartReasonText =
            ReferenceApplicationConstants.DEVICE_LOCATION_PANEL_UNAVAILABLE_TEXT,
    ),
}

/**
 * The on-map layer panel's state machine
 * (docs/ReferenceAppUX.md §4.2): the panel's open state, the base-map
 * rows, the traffic layer's debounced capability calls, the device-location
 * status row and the remembered traffic choice.
 *
 * The state owns every decision the panel makes and reaches the rest of the
 * application only through injected seams, so its tests drive it with
 * hand-written fakes and a traffic-flow provider registered on a real
 * `MapSdk` (never the network). The model supplies the production seams: the
 * SDK's `fetch_traffic_flow` capability, the shipped map view's camera
 * bounds, the base-map selection path and the existing remembered record.
 */
class MapLayersPanelState(
    /** The scope the debounced refresh runs in (the application's main scope). */
    private val coroutineScope: CoroutineScope,
    /** The capability call the layer performs. */
    private val fetchTrafficFlow: suspend (BoundingBox) -> List<TrafficFlowData>,
    /** The settled camera's geographic bounds. */
    private val cameraGeographicBounds: () -> BoundingBox?,
    /** Installs the one traffic overlay from the capability's rows. */
    private val installTrafficOverlay: (List<TrafficFlowData>) -> Unit,
    /** Removes the traffic overlay. */
    private val clearTrafficOverlay: () -> Unit,
    /** Reads the remembered traffic choice from the shared record. */
    private val loadRememberedTrafficChoice: () -> Boolean,
    /** Records the traffic choice in the remembered record. */
    private val persistTrafficChoice: () -> Unit,
    /** Reads the serving traffic-flow cache's counters and budget. */
    private val readTrafficCacheStatistics: () -> TrafficFlowCacheStatistics?,
    /** The verbatim detail text of a failed capability call. */
    private val failureText: (Throwable) -> String,
) {
    // -------------------------------------------------------------------
    // Published state
    // -------------------------------------------------------------------

    /** Whether the panel is presented. */
    var isPanelOpen: Boolean by mutableStateOf(false)
        private set

    /** The base-map rows, in the specification's order: raster first, then vector styles. */
    var baseRows: List<MapLayerBaseRow> by mutableStateOf(emptyList())
        private set

    /** The traffic switch's availability. */
    var trafficAvailability: MapLayerTrafficAvailability by mutableStateOf(
        MapLayerTrafficAvailability.NotConfigured(
            reason = ReferenceApplicationConstants.CAPABILITY_PROVIDER_NOT_CONFIGURED_TEXT,
        ),
    )
        private set

    /** Whether the traffic layer is on. */
    var isTrafficEnabled: Boolean by mutableStateOf(false)
        private set

    /** Whether a traffic request is in flight (the debounce's one-request cap). */
    var isTrafficRefreshing: Boolean by mutableStateOf(false)
        private set

    /** The panel's status line (a failed refresh, a refusal or a location reason). */
    var statusLineText: String? by mutableStateOf(null)
        private set

    /** The device-location row's state. */
    var locationStatus: DeviceLocationPanelStatus by mutableStateOf(
        DeviceLocationPanelStatus.OFF,
    )
        private set

    /** Whether the collapsed control currently holds focus (the close returns it). */
    var isControlFocused: Boolean by mutableStateOf(false)
        private set

    /** Whether the panel composition has presented (the UI-suite settle handle). */
    var hasPresentedPanel: Boolean by mutableStateOf(false)
        private set

    /** The number of traffic capability calls this session (the test handle). */
    var trafficFetchCount: Int by mutableStateOf(0)
        private set

    /** The number of traffic overlays installed this session. */
    var trafficOverlayInstallCount: Int by mutableStateOf(0)
        private set

    /**
     * The serving traffic-flow cache's counters and budget, read after each
     * refresh; null while the layer is off or the provider answers uncached.
     * The diagnostics surface shows them and the suite reads them.
     */
    var trafficCacheStatistics: TrafficFlowCacheStatistics? by mutableStateOf(null)
        private set

    /** The panel's section titles, in the order the specification fixes them. */
    val sectionTitles: List<String> = listOf(
        ReferenceApplicationConstants.BASE_MAP_SECTION_LABEL_TEXT,
        ReferenceApplicationConstants.MAP_LAYERS_TRAFFIC_SECTION_LABEL_TEXT,
        ReferenceApplicationConstants.MAP_LAYERS_OVERLAYS_SECTION_LABEL_TEXT,
        ReferenceApplicationConstants.MAP_LAYERS_DEVICE_LOCATION_SECTION_LABEL_TEXT,
    )

    // -------------------------------------------------------------------
    // Base-map identity (what the control's active dot compares against)
    // -------------------------------------------------------------------

    /** The first raster source that can serve: the default base. */
    private var defaultRasterProviderKey: String? = null

    /** The raster source currently serving, or null under a vector base. */
    private var activeRasterProviderKey: String? = null

    /** The styled vector base the SDK reports serving. */
    private var activeVectorChoiceKey: String? = null

    /** The registry's declared keyless styled base, when this build offers it. */
    private var keylessVectorChoiceKey: String? = null

    // -------------------------------------------------------------------
    // Debounce and request bookkeeping
    // -------------------------------------------------------------------

    /** The pending debounce job, if any. */
    private var debounceJob: Job? = null

    /** Whether a camera change arrived while a request was in flight. */
    private var hasPendingRefresh: Boolean = false

    /** The camera pose key of the last requested refresh. */
    private var lastRequestedCameraPoseKey: String? = null

    /** Invalidates answers that belong to a layer state the user already left. */
    private var trafficRequestGeneration: Int = 0

    // -------------------------------------------------------------------
    // Panel presentation
    // -------------------------------------------------------------------

    /** Opens the panel. The caller refreshes the rows first. */
    fun openPanel() {
        isPanelOpen = true
    }

    /** Closes the panel; the caller returns focus to the collapsed control. */
    fun closePanel() {
        isPanelOpen = false
    }

    /** Records whether the collapsed control holds focus. */
    fun noteControlFocused(isFocused: Boolean) {
        isControlFocused = isFocused
    }

    /** Records that the panel composition has presented. */
    fun notePanelPresented() {
        hasPresentedPanel = true
    }

    /** Whether the collapsed control shows its non-default-layer dot. */
    fun isNonDefaultLayerActive(enabledSceneCount: Int): Boolean {
        if (isTrafficEnabled || enabledSceneCount > 0) {
            return true
        }
        val keylessChoiceKey = keylessVectorChoiceKey
        if (keylessChoiceKey != null) {
            return activeVectorChoiceKey != keylessChoiceKey
        }
        if (activeVectorChoiceKey != null) {
            return true
        }
        return activeRasterProviderKey != defaultRasterProviderKey
    }

    // -------------------------------------------------------------------
    // Base map
    // -------------------------------------------------------------------

    /**
     * Rebuilds the base-map rows from the current choices and selection.
     * Raster descriptors come first, then every committed vector style; a
     * credentialed raster descriptor whose key is not applied is disabled
     * with `Needs <credential label>`.
     */
    fun refreshBaseRows(
        rasterChoices: List<BaseMapChoice>,
        vectorChoices: List<VectorBaseMapChoice>,
        activeRasterProviderKey: String?,
        activeVectorChoiceKey: String?,
        keylessVectorChoiceKey: String?,
    ) {
        this.defaultRasterProviderKey = rasterChoices
            .firstOrNull { choice -> choice.isServable }
            ?.providerKey
        this.activeRasterProviderKey = activeRasterProviderKey
        this.activeVectorChoiceKey = activeVectorChoiceKey
        this.keylessVectorChoiceKey = keylessVectorChoiceKey
        val rows = buildList {
            rasterChoices.forEach { choice ->
                add(
                    MapLayerBaseRow(
                        identifier = ReferenceApplicationConstants
                            .MAP_LAYER_BASE_ROW_TEST_TAG_PREFIX + choice.providerKey,
                        displayName = choice.descriptor.displayName,
                        selection = MapLayerBaseSelection.Raster(choice.providerKey),
                        isServing = activeVectorChoiceKey == null &&
                            choice.providerKey == activeRasterProviderKey,
                        disabledReasonText = if (choice.isServable) {
                            null
                        } else {
                            ReferenceApplicationConstants.BASE_MAP_NEEDS_CREDENTIAL_TEMPLATE
                                .format(choice.descriptor.credentialLabel)
                        },
                    ),
                )
            }
            vectorChoices.forEach { choice ->
                add(
                    MapLayerBaseRow(
                        identifier = ReferenceApplicationConstants
                            .MAP_LAYER_STYLE_ROW_TEST_TAG_PREFIX + choice.style.styleId,
                        displayName = choice.displayName,
                        selection = MapLayerBaseSelection.Vector(choice.choiceKey),
                        isServing = choice.choiceKey == activeVectorChoiceKey,
                        disabledReasonText = if (choice.isServable) {
                            null
                        } else {
                            ReferenceApplicationConstants.BASE_MAP_NEEDS_CREDENTIAL_TEMPLATE
                                .format(choice.descriptor.credentialLabel)
                        },
                    ),
                )
            }
        }
        baseRows = rows
    }

    // -------------------------------------------------------------------
    // Traffic configuration
    // -------------------------------------------------------------------

    /**
     * Resolves the traffic switch from the shared registry: the first
     * descriptor declaring the capability serves it; a credential it does
     * not yet have, or no declaration at all, states the reason plainly.
     */
    fun updateTrafficConfiguration(
        providerDescriptors: List<ProviderCredentialDescriptor>,
        appliedProviderKeys: Set<String>,
    ) {
        val descriptor = providerDescriptors.firstOrNull { candidate ->
            ReferenceApplicationConstants.TRAFFIC_FLOW_CAPABILITY_NAME in
                candidate.servedContracts &&
                candidate.isRegistrationAvailableOnAndroid
        }
        if (descriptor == null) {
            trafficAvailability = MapLayerTrafficAvailability.NotConfigured(
                reason = ReferenceApplicationConstants.CAPABILITY_PROVIDER_NOT_CONFIGURED_TEXT,
            )
            turnTrafficOffIfEnabled(remember = false)
            return
        }
        val requiresCredential = descriptor.credentialLabel.trim().isNotEmpty() &&
            descriptor.isRegistrationAvailableOnAndroid
        if (requiresCredential && descriptor.providerKey !in appliedProviderKeys) {
            trafficAvailability = MapLayerTrafficAvailability.NeedsCredential(
                reason = ReferenceApplicationConstants.BASE_MAP_NEEDS_CREDENTIAL_TEMPLATE
                    .format(descriptor.credentialLabel),
            )
            turnTrafficOffIfEnabled(remember = false)
            return
        }
        trafficAvailability = MapLayerTrafficAvailability.Available(
            providerKey = descriptor.providerKey,
            attribution = descriptor.attributionText,
        )
    }

    /**
     * Restores the remembered traffic choice once a descriptor that serves
     * the capability is registered; a keyless session keeps the choice for a
     * later launch instead of pretending the layer is on.
     */
    fun restoreRememberedTrafficIfAvailable() {
        if (isTrafficEnabled || !trafficAvailability.isAvailable) {
            return
        }
        if (!loadRememberedTrafficChoice()) {
            return
        }
        setTrafficEnabled(true, remember = false)
    }

    /**
     * Turns the layer on or off. A turn-on the registry cannot serve states
     * the reason instead of pretending; turning off clears the overlay and
     * any scheduled refresh.
     */
    fun setTrafficEnabled(enabled: Boolean, remember: Boolean = true) {
        if (enabled && !trafficAvailability.isAvailable) {
            statusLineText = trafficAvailability.reasonText
            return
        }
        isTrafficEnabled = enabled
        trafficRequestGeneration += 1
        if (remember) {
            persistTrafficChoice()
        }
        if (enabled) {
            scheduleTrafficRefresh()
        } else {
            cancelScheduledRefresh()
            clearTrafficOverlay()
            lastRequestedCameraPoseKey = null
            statusLineText = null
            trafficCacheStatistics = null
        }
    }

    /**
     * Notes the camera's settled pose: an unchanged pose schedules nothing, a
     * changed one restarts the debounce.
     */
    fun noteCameraPoseChanged(poseKey: String) {
        if (!isTrafficEnabled) {
            return
        }
        if (poseKey == lastRequestedCameraPoseKey) {
            return
        }
        lastRequestedCameraPoseKey = poseKey
        scheduleTrafficRefresh()
    }

    /** The serving descriptor's attribution line while the layer is on. */
    val trafficAttributionText: String?
        get() = if (isTrafficEnabled) trafficAvailability.attributionText else null

    /** Schedules one refresh after the debounce; a newer schedule replaces it. */
    private fun scheduleTrafficRefresh() {
        if (!isTrafficEnabled) {
            return
        }
        debounceJob?.cancel()
        debounceJob = coroutineScope.launch {
            delay(ReferenceApplicationConstants.TRAFFIC_REFRESH_DEBOUNCE_MILLISECONDS)
            debounceJob = null
            beginTrafficRefresh()
        }
    }

    /** Cancels the pending debounce and drops a pending refresh. */
    private fun cancelScheduledRefresh() {
        debounceJob?.cancel()
        debounceJob = null
        hasPendingRefresh = false
    }

    /**
     * Starts one capability call unless another is in flight; a call that
     * arrives while one is running is coalesced into one follow-up.
     */
    private suspend fun beginTrafficRefresh() {
        if (!isTrafficEnabled) {
            return
        }
        if (isTrafficRefreshing) {
            hasPendingRefresh = true
            return
        }
        val bounds = cameraGeographicBounds()
        if (bounds == null) {
            statusLineText =
                ReferenceApplicationConstants.MAP_LAYERS_CAMERA_BOUNDS_UNAVAILABLE_TEXT
            return
        }
        isTrafficRefreshing = true
        trafficFetchCount += 1
        val requestGeneration = trafficRequestGeneration
        try {
            val trafficFlowSegments = fetchTrafficFlow(bounds)
            // A toggle-off (or a new toggle-on) while the request was in
            // flight drops its answer: the overlay it would install no longer
            // belongs to the layer state in force.
            if (isTrafficEnabled && requestGeneration == trafficRequestGeneration) {
                installTrafficOverlay(trafficFlowSegments)
                trafficOverlayInstallCount += 1
                statusLineText = null
                // The call settled, so the cache's counters now describe it:
                // a repeat over the same ground reads as a hit, not a fetch.
                trafficCacheStatistics = readTrafficCacheStatistics()
            }
        } catch (refreshException: CancellationException) {
            throw refreshException
        } catch (refreshException: Exception) {
            if (requestGeneration == trafficRequestGeneration) {
                val detailText = failureText(refreshException)
                statusLineText = ReferenceApplicationConstants
                    .MAP_LAYERS_TRAFFIC_REFRESH_FAILURE_TEMPLATE
                    .format(detailText)
                if (refreshException is SdkException.ProviderNotConfigured) {
                    // The switch turns off with the capability's own reason.
                    setTrafficEnabled(false)
                    statusLineText = detailText
                }
            }
        } finally {
            isTrafficRefreshing = false
            if (hasPendingRefresh) {
                hasPendingRefresh = false
                scheduleTrafficRefresh()
            }
        }
    }

    /** Turns the layer off without recording the change. */
    private fun turnTrafficOffIfEnabled(remember: Boolean) {
        if (!isTrafficEnabled) {
            return
        }
        setTrafficEnabled(false, remember = remember)
    }

    // -------------------------------------------------------------------
    // Device location
    // -------------------------------------------------------------------

    /** Updates the row after the source reports a state change. */
    fun noteLocationStatus(status: DeviceLocationPanelStatus) {
        locationStatus = status
        when (status) {
            DeviceLocationPanelStatus.PERMISSION_DENIED,
            DeviceLocationPanelStatus.UNAVAILABLE,
            -> statusLineText = status.displayText

            DeviceLocationPanelStatus.OFF -> statusLineText = null
            DeviceLocationPanelStatus.WAITING_FOR_FIX,
            DeviceLocationPanelStatus.ACTIVE,
            -> Unit
        }
    }
}
