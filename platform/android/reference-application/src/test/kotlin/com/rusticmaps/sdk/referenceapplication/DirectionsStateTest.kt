package com.rusticmaps.sdk.referenceapplication

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.rustic_maps.AvoidOptions
import uniffi.rustic_maps.RouteStrategy
import uniffi.rustic_maps.SdkException
import uniffi.rustic_maps.TransportationMode
import uniffi.rustic_maps.UnitSystem

/**
 * Unit tests of the Directions control's state holder
 * (PRD.md §6 item 36): the Car/Fastest defaults, the plan
 * options the selected choices produce (the ferry-route regression), the
 * capability-driven disabling with its reason, and the remembered pair's
 * round trip through the record codec. Pure Kotlin with a hand-written
 * capability gateway.
 */
class DirectionsStateTest {
    @Test
    fun `the control starts on car and fastest`() {
        val directionsState = DirectionsState(gateway = FakeDirectionsCapabilitiesGateway())
        assertEquals(DirectionsTransportationMode.CAR, directionsState.selectedTransportationMode)
        assertEquals(DirectionsRouteStrategy.FASTEST, directionsState.selectedRouteStrategy)
        assertEquals(
            "both modes start selectable before the first capability read",
            setOf(DirectionsTransportationMode.CAR, DirectionsTransportationMode.WALK),
            directionsState.capabilities.supportedTransportationModes,
        )
        assertTrue(directionsState.isTransportationModeAvailable(DirectionsTransportationMode.CAR))
        assertTrue(directionsState.isShortestRouteStrategyAvailable())
    }

    @Test
    fun `with car selected the plan call carries automobile and fastest`() {
        val directionsState = DirectionsState(gateway = FakeDirectionsCapabilitiesGateway())
        val routeOptions = directionsState.routeOptions(
            unitSystem = UnitSystem.METRIC,
            avoidOptions = avoidNothing,
        )
        assertEquals(
            "the plan call must carry AUTOMOBILE, not the old hardcoded pedestrian mode",
            TransportationMode.AUTOMOBILE,
            routeOptions.transportationMode,
        )
        assertEquals(RouteStrategy.FASTEST, routeOptions.routeStrategy)
        assertEquals(UnitSystem.METRIC, routeOptions.unitSystem)
    }

    @Test
    fun `choosing walk carries pedestrian and persists the choice`() {
        var persistCount = 0
        val directionsState = DirectionsState(
            gateway = FakeDirectionsCapabilitiesGateway(),
            onSelectionChanged = { persistCount += 1 },
        )
        directionsState.refresh()
        directionsState.selectTransportationMode(DirectionsTransportationMode.WALK)
        assertEquals(DirectionsTransportationMode.WALK, directionsState.selectedTransportationMode)
        assertEquals(
            TransportationMode.PEDESTRIAN,
            directionsState.routeOptions(UnitSystem.METRIC, avoidNothing).transportationMode,
        )
        assertEquals(1, persistCount)
    }

    @Test
    fun `a walk plan carries fastest even while shortest is remembered`() {
        val directionsState = DirectionsState(gateway = FakeDirectionsCapabilitiesGateway())
        directionsState.refresh()
        directionsState.restoreSelections(
            DirectionsTransportationMode.WALK,
            DirectionsRouteStrategy.SHORTEST,
        )
        val routeOptions = directionsState.routeOptions(UnitSystem.METRIC, avoidNothing)
        assertEquals(TransportationMode.PEDESTRIAN, routeOptions.transportationMode)
        assertEquals(
            "the strategy choice is an automobile-only option",
            RouteStrategy.FASTEST,
            routeOptions.routeStrategy,
        )
    }

    @Test
    fun `choosing shortest carries shortest and persists the choice`() {
        var persistCount = 0
        val directionsState = DirectionsState(
            gateway = FakeDirectionsCapabilitiesGateway(),
            onSelectionChanged = { persistCount += 1 },
        )
        directionsState.refresh()
        directionsState.selectRouteStrategy(DirectionsRouteStrategy.SHORTEST)
        assertEquals(DirectionsRouteStrategy.SHORTEST, directionsState.selectedRouteStrategy)
        assertEquals(
            RouteStrategy.SHORTEST,
            directionsState.routeOptions(UnitSystem.METRIC, avoidNothing).routeStrategy,
        )
        assertEquals(1, persistCount)
    }

    @Test
    fun `a mode the provider does not support is disabled with a reason`() {
        val directionsState = DirectionsState(
            gateway = FakeDirectionsCapabilitiesGateway(
                answer = DirectionsCapabilities(
                    supportedTransportationModes = setOf(DirectionsTransportationMode.CAR),
                    supportsShortestRouteStrategy = true,
                ),
            ),
        )
        directionsState.refresh()
        assertFalse(directionsState.isTransportationModeAvailable(DirectionsTransportationMode.WALK))
        assertEquals(
            ReferenceApplicationConstants.DIRECTIONS_MODE_UNAVAILABLE_TEMPLATE
                .format(ReferenceApplicationConstants.DIRECTIONS_MODE_WALK_TEXT),
            directionsState.transportationModeUnavailableReason(
                DirectionsTransportationMode.WALK,
            ),
        )
        // The unsupported pick changes nothing; the plan keeps the supported mode.
        directionsState.selectTransportationMode(DirectionsTransportationMode.WALK)
        assertEquals(DirectionsTransportationMode.CAR, directionsState.selectedTransportationMode)
        assertEquals(
            TransportationMode.AUTOMOBILE,
            directionsState.routeOptions(UnitSystem.METRIC, avoidNothing).transportationMode,
        )
    }

    @Test
    fun `shortest is disabled with its reason when the provider reports no support`() {
        val directionsState = DirectionsState(
            gateway = FakeDirectionsCapabilitiesGateway(
                answer = DirectionsCapabilities(
                    supportedTransportationModes = DirectionsTransportationMode.entries.toSet(),
                    supportsShortestRouteStrategy = false,
                ),
            ),
        )
        directionsState.refresh()
        assertFalse(directionsState.isShortestRouteStrategyAvailable())
        assertEquals(
            ReferenceApplicationConstants.DIRECTIONS_STRATEGY_UNAVAILABLE_TEMPLATE
                .format(ReferenceApplicationConstants.DIRECTIONS_STRATEGY_SHORTEST_TEXT),
            directionsState.routeStrategyUnavailableReason(),
        )
        // A remembered Shortest choice never reaches a provider that cannot
        // honour it: the effective strategy falls back to Fastest.
        directionsState.restoreSelections(
            DirectionsTransportationMode.CAR,
            DirectionsRouteStrategy.SHORTEST,
        )
        assertEquals(
            RouteStrategy.FASTEST,
            directionsState.routeOptions(UnitSystem.METRIC, avoidNothing).routeStrategy,
        )
    }

    @Test
    fun `a failed capability read disables both choices with the contract reason`() {
        val directionsState = DirectionsState(
            gateway = FakeDirectionsCapabilitiesGateway(
                failure = SdkException.ProviderNotConfigured(contractName = "RoutingProvider"),
            ),
            readFailureReason = { failure ->
                (failure as? SdkException.ProviderNotConfigured)
                    ?.let { notConfigured -> "no '${notConfigured.contractName}' provider is configured" }
                    ?: failure.message.orEmpty()
            },
        )
        directionsState.refresh()
        assertEquals(
            "no 'RoutingProvider' provider is configured",
            directionsState.capabilities.unavailableReasonText,
        )
        assertFalse(directionsState.isTransportationModeAvailable(DirectionsTransportationMode.CAR))
        assertFalse(directionsState.isTransportationModeAvailable(DirectionsTransportationMode.WALK))
        assertFalse(directionsState.isShortestRouteStrategyAvailable())
        assertEquals(
            "no 'RoutingProvider' provider is configured",
            directionsState.transportationModeUnavailableReason(DirectionsTransportationMode.CAR),
        )
        // The remembered Car choice stands; it reaches a provider only after
        // one is installed.
        assertEquals(DirectionsTransportationMode.CAR, directionsState.effectiveTransportationMode)
    }

    @Test
    fun `the remembered pair round-trips through the record codec`() {
        val record = JSONObject()
        RememberedChoicesStore.encodeDirections(
            record = record,
            transportationMode = DirectionsTransportationMode.WALK,
            routeStrategy = DirectionsRouteStrategy.SHORTEST,
        )
        val decodedDirections = RememberedChoicesStore.decodeDirections(record)
        assertEquals(DirectionsTransportationMode.WALK, decodedDirections.transportationMode)
        assertEquals(DirectionsRouteStrategy.SHORTEST, decodedDirections.routeStrategy)
    }

    @Test
    fun `a record written before the directions field loads the defaults`() {
        val decodedDirections = RememberedChoicesStore.decodeDirections(JSONObject())
        assertEquals(DirectionsTransportationMode.CAR, decodedDirections.transportationMode)
        assertEquals(DirectionsRouteStrategy.FASTEST, decodedDirections.routeStrategy)
    }

    @Test
    fun `a value this build does not offer falls back to the choice default`() {
        val decodedDirections = RememberedChoicesStore.decodeDirections(
            JSONObject()
                .put(
                    RememberedChoicesStore.DIRECTIONS_TRANSPORTATION_MODE_FIELD,
                    "bicycle",
                )
                .put(RememberedChoicesStore.DIRECTIONS_ROUTE_STRATEGY_FIELD, "scenic"),
        )
        assertEquals(DirectionsTransportationMode.CAR, decodedDirections.transportationMode)
        assertEquals(DirectionsRouteStrategy.FASTEST, decodedDirections.routeStrategy)
    }

    @Test
    fun `the restored selections apply without rewriting the record`() {
        var persistCount = 0
        val directionsState = DirectionsState(
            gateway = FakeDirectionsCapabilitiesGateway(),
            onSelectionChanged = { persistCount += 1 },
        )
        directionsState.restoreSelections(
            DirectionsTransportationMode.WALK,
            DirectionsRouteStrategy.SHORTEST,
        )
        assertEquals(DirectionsTransportationMode.WALK, directionsState.selectedTransportationMode)
        assertEquals(DirectionsRouteStrategy.SHORTEST, directionsState.selectedRouteStrategy)
        assertEquals("a restore is not a user pick", 0, persistCount)
    }

    @Test
    fun `the option summary names the effective choices`() {
        val directionsState = DirectionsState(gateway = FakeDirectionsCapabilitiesGateway())
        assertEquals("car · fastest", directionsState.routeOptionsSummaryText)
        directionsState.refresh()
        directionsState.selectTransportationMode(DirectionsTransportationMode.WALK)
        assertEquals("walk · fastest", directionsState.routeOptionsSummaryText)
        assertNull(directionsState.transportationModeUnavailableReason(
            DirectionsTransportationMode.WALK,
        ))
        assertNull(directionsState.routeStrategyUnavailableReason())
    }

    /** A hand-written gateway answering one capability report or throwing one failure. */
    private class FakeDirectionsCapabilitiesGateway(
        private val answer: DirectionsCapabilities = DirectionsCapabilities.allSupported,
        private val failure: Exception? = null,
    ) : DirectionsCapabilitiesGateway {
        override fun routeCapabilities(): DirectionsCapabilities {
            failure?.let { thrownFailure -> throw thrownFailure }
            return answer
        }
    }

    private companion object {
        /** The plan flows avoid nothing, so the provider's fastest path stands. */
        val avoidNothing: AvoidOptions = AvoidOptions(
            tollRoads = false,
            limitedAccessHighways = false,
            ferries = false,
            unpavedRoads = false,
        )
    }
}
