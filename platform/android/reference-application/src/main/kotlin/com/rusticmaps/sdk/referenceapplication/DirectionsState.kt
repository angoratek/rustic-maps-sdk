package com.rusticmaps.sdk.referenceapplication

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.rusticmaps.sdk.MapsSdkFacade
import kotlinx.coroutines.CancellationException
import uniffi.rustic_maps.AvoidOptions
import uniffi.rustic_maps.RouteOptions
import uniffi.rustic_maps.RouteStrategy
import uniffi.rustic_maps.TransportationMode
import uniffi.rustic_maps.UnitSystem

/**
 * The two transport modes the Directions control offers (docs/design/DirectionsModesAndStrategy.md).
 *
 * The contract expresses three modes; Car and Walk are the two every shipped
 * routing provider can honour, with Car the default because the defect this
 * control fixes was a pedestrian profile answering an automobile trip. The
 * stored value is the contract enum's own spelling, so the remembered record
 * survives a binding regeneration unchanged.
 */
enum class DirectionsTransportationMode(
    /** The contract enum's stable snake-case spelling, used by the record. */
    val storageValue: String,
    /** The consumer word the Directions control and the plan options line show. */
    val displayText: String,
) {
    /** Automobile routing (the default). */
    CAR(storageValue = "automobile", displayText = "Car"),
    /** Pedestrian routing. */
    WALK(storageValue = "pedestrian", displayText = "Walk"),
    ;

    /** The contract value this choice carries to the plan call. */
    val transportationMode: TransportationMode
        get() = when (this) {
            CAR -> TransportationMode.AUTOMOBILE
            WALK -> TransportationMode.PEDESTRIAN
        }

    companion object {
        /**
         * The choice a remembered record's value names, or null for a value
         * this build does not offer (a mode added by another revision).
         */
        fun fromStorageValue(storageValue: String?): DirectionsTransportationMode? =
            entries.firstOrNull { mode -> mode.storageValue == storageValue }

        /**
         * The choice a contract mode maps to, or null for a mode the control
         * does not offer (bicycle today).
         */
        fun fromTransportationMode(
            transportationMode: TransportationMode,
        ): DirectionsTransportationMode? = when (transportationMode) {
            TransportationMode.AUTOMOBILE -> CAR
            TransportationMode.PEDESTRIAN -> WALK
            TransportationMode.BICYCLE -> null
        }
    }
}

/**
 * The two route strategies the Directions control offers
 * (docs/design/DirectionsModesAndStrategy.md): Fastest is the default, Shortest is
 * disabled with its reason when the serving provider reports no
 * shortest-path support (a provider without a route-service equivalent).
 */
enum class DirectionsRouteStrategy(
    /** The contract enum's stable snake-case spelling, used by the record. */
    val storageValue: String,
    /** The consumer word the Directions control and the plan options line show. */
    val displayText: String,
) {
    /** Minimize travel time (the default). */
    FASTEST(storageValue = "fastest", displayText = "Fastest"),
    /** Minimize traveled distance. */
    SHORTEST(storageValue = "shortest", displayText = "Shortest"),
    ;

    /** The contract value this choice carries to the plan call. */
    val routeStrategy: RouteStrategy
        get() = when (this) {
            FASTEST -> RouteStrategy.FASTEST
            SHORTEST -> RouteStrategy.SHORTEST
        }

    companion object {
        /**
         * The choice a remembered record's value names, or null for a value
         * this build does not offer.
         */
        fun fromStorageValue(storageValue: String?): DirectionsRouteStrategy? =
            entries.firstOrNull { strategy -> strategy.storageValue == storageValue }
    }
}

/**
 * What the serving routing provider says it can honour: the modes it can
 * route for, whether it can minimize distance, and — when the read itself
 * failed — the verbatim reason a surface states instead of pretending the
 * choices are available.
 */
data class DirectionsCapabilities(
    /** The transport modes the provider can route for. */
    val supportedTransportationModes: Set<DirectionsTransportationMode>,
    /** Whether the provider can minimize distance instead of travel time. */
    val supportsShortestRouteStrategy: Boolean,
    /**
     * The verbatim reason no capability answer is available (for example the
     * contract's `ProviderNotConfigured` sentence while no routing provider is
     * installed), or null when the read succeeded.
     */
    val unavailableReasonText: String? = null,
) {
    /** Whether [mode] can be chosen. */
    fun supports(mode: DirectionsTransportationMode): Boolean =
        unavailableReasonText == null && mode in supportedTransportationModes

    companion object {
        /**
         * The answer a surface assumes before the first read: every offered
         * mode and strategy is selectable, so the control is usable rather
         * than blank. A real read replaces it.
         */
        val allSupported: DirectionsCapabilities = DirectionsCapabilities(
            supportedTransportationModes = DirectionsTransportationMode.entries.toSet(),
            supportsShortestRouteStrategy = true,
        )
    }
}

/**
 * The SDK operation the Directions control needs: the serving routing
 * provider's capability answer. The interface keeps the state holder testable
 * with a hand-written fake while the shipped implementation delegates to the
 * SDK facade.
 */
interface DirectionsCapabilitiesGateway {
    /**
     * Reads the capabilities of the serving routing provider. Throws the
     * contract's `SdkException` (its `ProviderNotConfigured` variant while no
     * routing provider is installed).
     */
    fun routeCapabilities(): DirectionsCapabilities
}

/** The gateway backed by the shipped SDK facade. */
class SdkDirectionsCapabilitiesGateway(
    private val facade: MapsSdkFacade,
) : DirectionsCapabilitiesGateway {
    override fun routeCapabilities(): DirectionsCapabilities {
        val contractCapabilities = facade.routeCapabilities()
        return DirectionsCapabilities(
            supportedTransportationModes = contractCapabilities.supportedTransportationModes
                .mapNotNull(DirectionsTransportationMode::fromTransportationMode)
                .toSet(),
            supportsShortestRouteStrategy =
                contractCapabilities.supportsShortestRouteStrategy,
        )
    }
}

/**
 * The Directions control's state (docs/design/DirectionsModesAndStrategy.md): the
 * selected transport mode and route strategy, the serving provider's
 * capability answer, and the disabled-with-reason lines a choice the provider
 * cannot honour carries.
 *
 * Both choices are remembered through [onSelectionChanged] and re-applied to
 * every route request the application makes. The plan options fall back to a
 * mode or strategy the provider supports so a remembered Shortest choice
 * never reaches a provider that would silently ignore it; the remembered
 * choice itself is kept for a provider that can honour it later.
 */
class DirectionsState(
    /** The capability readback the control renders. */
    private val gateway: DirectionsCapabilitiesGateway,
    initiallySelectedMode: DirectionsTransportationMode = DirectionsTransportationMode.CAR,
    initiallySelectedStrategy: DirectionsRouteStrategy = DirectionsRouteStrategy.FASTEST,
    /** Persists the choice after the user picks one; a restore never fires it. */
    private val onSelectionChanged: () -> Unit = {},
    /** The verbatim reason a failed capability read is annotated with. */
    private val readFailureReason: (Exception) -> String = { failure ->
        failure.message ?: failure::class.simpleName ?: "the routing provider is unavailable"
    },
) {
    /** The last capability answer, or the all-supported assumption before the first read. */
    var capabilities: DirectionsCapabilities by mutableStateOf(DirectionsCapabilities.allSupported)
        private set

    /** The selected transport mode (Car unless a stored choice or a pick says otherwise). */
    var selectedTransportationMode: DirectionsTransportationMode by mutableStateOf(initiallySelectedMode)
        private set

    /** The selected route strategy (Fastest unless a stored choice or a pick says otherwise). */
    var selectedRouteStrategy: DirectionsRouteStrategy by mutableStateOf(initiallySelectedStrategy)
        private set

    /**
     * Re-reads the serving provider's capabilities. The plan sheet calls this
     * when it opens and the application calls it after a provider change, so
     * a newly installed (or removed) routing provider's answer is what the
     * control renders.
     */
    fun refresh() {
        capabilities = try {
            gateway.routeCapabilities()
        } catch (readException: Exception) {
            if (readException is CancellationException) {
                throw readException
            }
            DirectionsCapabilities(
                supportedTransportationModes = emptySet(),
                supportsShortestRouteStrategy = false,
                unavailableReasonText = readFailureReason(readException),
            )
        }
    }

    /** Restores a remembered pair without firing the persistence callback. */
    fun restoreSelections(
        transportationMode: DirectionsTransportationMode,
        routeStrategy: DirectionsRouteStrategy,
    ) {
        selectedTransportationMode = transportationMode
        selectedRouteStrategy = routeStrategy
    }

    /** Returns both choices to their defaults without firing the persistence callback. */
    fun resetToDefaults() {
        selectedTransportationMode = DirectionsTransportationMode.CAR
        selectedRouteStrategy = DirectionsRouteStrategy.FASTEST
    }

    /** The user's mode pick; a mode the provider cannot honour changes nothing. */
    fun selectTransportationMode(transportationMode: DirectionsTransportationMode) {
        if (!capabilities.supports(transportationMode)) {
            return
        }
        if (selectedTransportationMode == transportationMode) {
            return
        }
        selectedTransportationMode = transportationMode
        onSelectionChanged()
    }

    /** The user's strategy pick; Shortest changes nothing while it is unsupported. */
    fun selectRouteStrategy(routeStrategy: DirectionsRouteStrategy) {
        if (routeStrategy == DirectionsRouteStrategy.SHORTEST &&
            !capabilities.supportsShortestRouteStrategy
        ) {
            return
        }
        if (selectedRouteStrategy == routeStrategy) {
            return
        }
        selectedRouteStrategy = routeStrategy
        onSelectionChanged()
    }

    /** Whether the mode's control is enabled. */
    fun isTransportationModeAvailable(transportationMode: DirectionsTransportationMode): Boolean =
        capabilities.supports(transportationMode)

    /** Whether the Shortest control is enabled. */
    fun isShortestRouteStrategyAvailable(): Boolean =
        capabilities.unavailableReasonText == null &&
            capabilities.supportsShortestRouteStrategy

    /**
     * The disabled-with-reason line a mode carries, or null while it is
     * selectable. A failed capability read reports its own verbatim reason
     * (the contract's `ProviderNotConfigured` sentence while no routing
     * provider is installed); a provider that answered but omits the mode
     * says so directly.
     */
    fun transportationModeUnavailableReason(
        transportationMode: DirectionsTransportationMode,
    ): String? {
        if (isTransportationModeAvailable(transportationMode)) {
            return null
        }
        return capabilities.unavailableReasonText
            ?: ReferenceApplicationConstants.DIRECTIONS_MODE_UNAVAILABLE_TEMPLATE
                .format(transportationMode.displayText)
    }

    /** The disabled-with-reason line Shortest carries, or null while it is selectable. */
    fun routeStrategyUnavailableReason(): String? {
        if (isShortestRouteStrategyAvailable()) {
            return null
        }
        return capabilities.unavailableReasonText
            ?: ReferenceApplicationConstants.DIRECTIONS_STRATEGY_UNAVAILABLE_TEMPLATE
                .format(DirectionsRouteStrategy.SHORTEST.displayText)
    }

    /**
     * The transport mode the plan call carries: the selection while the
     * provider supports it, otherwise the first offered mode the provider
     * does support, otherwise the selection (no provider is installed, so no
     * provider call follows).
     */
    val effectiveTransportationMode: DirectionsTransportationMode
        get() = if (capabilities.supports(selectedTransportationMode)) {
            selectedTransportationMode
        } else {
            DirectionsTransportationMode.entries.firstOrNull { mode -> capabilities.supports(mode) }
                ?: selectedTransportationMode
        }

    /**
     * The route strategy the plan call carries: Shortest only for an
     * automobile plan whose provider supports it, Fastest otherwise. The
     * strategy control is an automobile-only choice
     * (docs/design/DirectionsModesAndStrategy.md), so a walk plan always carries
     * Fastest even while a remembered Shortest choice waits for a car plan.
     */
    val effectiveRouteStrategy: DirectionsRouteStrategy
        get() = if (effectiveTransportationMode == DirectionsTransportationMode.CAR &&
            selectedRouteStrategy == DirectionsRouteStrategy.SHORTEST &&
            capabilities.supportsShortestRouteStrategy
        ) {
            DirectionsRouteStrategy.SHORTEST
        } else {
            DirectionsRouteStrategy.FASTEST
        }

    /**
     * The route options every plan in the application carries: the selected
     * mode and strategy, the metric unit system the shared demo uses, and the
     * application's avoid options (`false` everywhere: the demo avoids
     * nothing so the plan follows the provider's fastest path).
     */
    fun routeOptions(
        unitSystem: UnitSystem,
        avoidOptions: AvoidOptions,
    ): RouteOptions = RouteOptions(
        transportationMode = effectiveTransportationMode.transportationMode,
        routeStrategy = effectiveRouteStrategy.routeStrategy,
        unitSystem = unitSystem,
        avoidOptions = avoidOptions,
        departureTimestampSeconds = null,
    )

    /** The route summary's option line: the effective mode and strategy in consumer words. */
    val routeOptionsSummaryText: String
        get() = ReferenceApplicationConstants.ROUTE_OPTIONS_SUMMARY_TEMPLATE.format(
            effectiveTransportationMode.displayText.lowercase(),
            effectiveRouteStrategy.displayText.lowercase(),
        )
}
