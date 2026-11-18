package com.rusticmaps.sdk.referenceapplication.map

import com.rusticmaps.sdk.ProviderCredentialDescriptor
import com.rusticmaps.sdk.VectorStyleDescriptor
import com.rusticmaps.sdk.referenceapplication.BaseMapChoice
import com.rusticmaps.sdk.referenceapplication.ReferenceApplicationConstants
import com.rusticmaps.sdk.referenceapplication.VectorBaseMapChoice
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.rustic_maps.BoundingBox
import uniffi.rustic_maps.GeographicCoordinate
import uniffi.rustic_maps.TrafficFlowData

/**
 * Unit tests of the on-map layer panel's state machine: the base rows' order
 * and serving marks, the traffic switch's availability resolution and the
 * remembered traffic choice's restore rule. Pure Kotlin with hand-written
 * descriptors and seams; no test reaches the network.
 */
class MapLayersPanelStateTest {
    @Test
    fun `base rows list raster sources first then vector styles and mark the serving row`() {
        val servableRaster = BaseMapChoice(
            descriptor = descriptor(providerKey = "source-a", credentialLabel = ""),
            isServable = true,
        )
        val credentialedRaster = BaseMapChoice(
            descriptor = descriptor(providerKey = "source-b", credentialLabel = CREDENTIAL_LABEL),
            isServable = false,
        )
        val vectorChoice = VectorBaseMapChoice(
            descriptor = descriptor(providerKey = "source-c", credentialLabel = ""),
            style = VectorStyleDescriptor(
                styleId = STYLE_ID,
                displayName = STYLE_DISPLAY_NAME,
                theme = "light",
                attributionText = "Style attribution",
            ),
        )
        val state = newState()
        state.refreshBaseRows(
            rasterChoices = listOf(servableRaster, credentialedRaster),
            vectorChoices = listOf(vectorChoice),
            activeRasterProviderKey = servableRaster.providerKey,
            activeVectorChoiceKey = null,
            keylessVectorChoiceKey = null,
        )
        assertEquals(
            listOf(
                ReferenceApplicationConstants.MAP_LAYER_BASE_ROW_TEST_TAG_PREFIX + "source-a",
                ReferenceApplicationConstants.MAP_LAYER_BASE_ROW_TEST_TAG_PREFIX + "source-b",
                ReferenceApplicationConstants.MAP_LAYER_STYLE_ROW_TEST_TAG_PREFIX + STYLE_ID,
            ),
            state.baseRows.map { row -> row.identifier },
        )
        assertTrue("the active raster row serves", state.baseRows[0].isServing)
        assertFalse(state.baseRows[1].isServing)
        assertFalse(state.baseRows[2].isServing)
        assertEquals(
            ReferenceApplicationConstants.BASE_MAP_NEEDS_CREDENTIAL_TEMPLATE
                .format(CREDENTIAL_LABEL),
            state.baseRows[1].disabledReasonText,
        )
        assertEquals(
            "${servableRaster.descriptor.displayName} — " +
                ReferenceApplicationConstants.BASE_MAP_ACTIVE_STATUS_TEXT,
            state.baseRows[0].buttonLabel,
        )

        // A vector style serves in place of the raster row.
        state.refreshBaseRows(
            rasterChoices = listOf(servableRaster, credentialedRaster),
            vectorChoices = listOf(vectorChoice),
            activeRasterProviderKey = null,
            activeVectorChoiceKey = vectorChoice.choiceKey,
            keylessVectorChoiceKey = null,
        )
        assertFalse("the raster row leaves serving under a vector base", state.baseRows[0].isServing)
        assertTrue("the vector row serves", state.baseRows[2].isServing)
    }

    @Test
    fun `a credentialed vector row states what its base needs`() {
        val hostedVectorChoice = VectorBaseMapChoice(
            descriptor = descriptor(
                providerKey = "hosted-vector",
                credentialLabel = CREDENTIAL_LABEL,
            ),
            style = VectorStyleDescriptor(
                styleId = STYLE_ID,
                displayName = STYLE_DISPLAY_NAME,
                theme = "light",
                attributionText = "Style attribution",
            ),
            isServable = false,
        )
        val state = newState()
        state.refreshBaseRows(
            rasterChoices = emptyList(),
            vectorChoices = listOf(hostedVectorChoice),
            activeRasterProviderKey = null,
            activeVectorChoiceKey = null,
            keylessVectorChoiceKey = null,
        )
        val hostedRow = state.baseRows.single()
        assertEquals(
            ReferenceApplicationConstants.BASE_MAP_NEEDS_CREDENTIAL_TEMPLATE
                .format(CREDENTIAL_LABEL),
            hostedRow.disabledReasonText,
        )
        assertEquals(
            "${hostedVectorChoice.displayName} — " +
                ReferenceApplicationConstants.BASE_MAP_NEEDS_CREDENTIAL_TEMPLATE
                    .format(CREDENTIAL_LABEL),
            hostedRow.buttonLabel,
        )
    }

    @Test
    fun `traffic reads not configured when no descriptor declares the capability`() {
        val state = newState()
        state.updateTrafficConfiguration(
            providerDescriptors = listOf(
                descriptor(providerKey = "source-a", servedContracts = listOf("routing")),
            ),
            appliedProviderKeys = emptySet(),
        )
        assertEquals(
            ReferenceApplicationConstants.CAPABILITY_PROVIDER_NOT_CONFIGURED_TEXT,
            state.trafficAvailability.reasonText,
        )
        assertFalse(state.trafficAvailability.isAvailable)
    }

    @Test
    fun `traffic reads needs credential without an applied key and is available with one`() {
        val trafficDescriptor = descriptor(
            providerKey = "source-b",
            credentialLabel = CREDENTIAL_LABEL,
            servedContracts = listOf(TRAFFIC_FLOW_CAPABILITY),
        )
        val state = newState()
        state.updateTrafficConfiguration(
            providerDescriptors = listOf(trafficDescriptor),
            appliedProviderKeys = emptySet(),
        )
        assertEquals(
            ReferenceApplicationConstants.BASE_MAP_NEEDS_CREDENTIAL_TEMPLATE
                .format(CREDENTIAL_LABEL),
            state.trafficAvailability.reasonText,
        )
        assertFalse(state.trafficAvailability.isAvailable)

        state.updateTrafficConfiguration(
            providerDescriptors = listOf(trafficDescriptor),
            appliedProviderKeys = setOf(trafficDescriptor.providerKey),
        )
        assertTrue(state.trafficAvailability.isAvailable)
        assertNull(state.trafficAvailability.reasonText)
        assertEquals(
            trafficDescriptor.attributionText,
            state.trafficAvailability.attributionText,
        )
    }

    @Test
    fun `the remembered traffic choice restores only while a provider serves it`() {
        val trafficDescriptor = descriptor(
            providerKey = "source-b",
            credentialLabel = CREDENTIAL_LABEL,
            servedContracts = listOf(TRAFFIC_FLOW_CAPABILITY),
        )
        val keylessState = newState(rememberedTrafficEnabled = true)
        keylessState.updateTrafficConfiguration(
            providerDescriptors = listOf(trafficDescriptor),
            appliedProviderKeys = emptySet(),
        )
        keylessState.restoreRememberedTrafficIfAvailable()
        assertFalse(
            "a keyless session keeps the remembered choice for a later launch",
            keylessState.isTrafficEnabled,
        )

        val keyedState = newState(rememberedTrafficEnabled = true)
        keyedState.updateTrafficConfiguration(
            providerDescriptors = listOf(trafficDescriptor),
            appliedProviderKeys = setOf(trafficDescriptor.providerKey),
        )
        keyedState.restoreRememberedTrafficIfAvailable()
        assertTrue("the remembered choice applies once a provider serves", keyedState.isTrafficEnabled)
    }

    /** The state over inert seams; the tests that need a layer state drive it directly. */
    private fun newState(rememberedTrafficEnabled: Boolean = false): MapLayersPanelState =
        MapLayersPanelState(
            coroutineScope = CoroutineScope(Dispatchers.Unconfined),
            fetchTrafficFlow = { _: BoundingBox -> emptyList<TrafficFlowData>() },
            cameraGeographicBounds = { sampleBounds },
            installTrafficOverlay = {},
            clearTrafficOverlay = {},
            loadRememberedTrafficChoice = { rememberedTrafficEnabled },
            persistTrafficChoice = {},
            readTrafficCacheStatistics = { null },
            failureText = { failure -> failure.message ?: "unknown failure" },
        )

    /** One registry descriptor with the fields the panel reads. */
    private fun descriptor(
        providerKey: String,
        credentialLabel: String = "",
        servedContracts: List<String> = emptyList(),
    ): ProviderCredentialDescriptor = ProviderCredentialDescriptor(
        providerKey = providerKey,
        displayName = "Provider ${providerKey.last()}",
        credentialLabel = credentialLabel,
        isRegistrationAvailableOnAndroid = true,
        attributionText = "Attribution for $providerKey",
        termsNote = "",
        settingsHint = "",
        rasterUrlTemplate = "",
        maximumZoomLevel = 18,
        vectorUrlTemplate = "",
        vectorStyleSchema = "",
        vectorStyles = emptyList(),
        servedContracts = servedContracts,
    )

    private companion object {
        const val CREDENTIAL_LABEL: String = "API key"
        const val TRAFFIC_FLOW_CAPABILITY: String = "traffic_flow"
        const val STYLE_ID: String = "style-a"
        const val STYLE_DISPLAY_NAME: String = "Style A"

        /** A synthetic bounding box; the unit tests never fetch. */
        val sampleBounds: BoundingBox = BoundingBox(
            southWest = GeographicCoordinate(latitude = 38.0, longitude = -121.0),
            northEast = GeographicCoordinate(latitude = 39.0, longitude = -120.0),
        )
    }
}
