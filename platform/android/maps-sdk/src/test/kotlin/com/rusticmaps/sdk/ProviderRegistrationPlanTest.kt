package com.rusticmaps.sdk

import com.rusticmaps.sdk.ProviderCredentialDescriptor
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.rustic_maps.ProviderContract

/**
 * Unit tests of the descriptor-driven registration rules. They exercise the
 * pure Kotlin planner, so they never construct the native SDK object.
 */
class ProviderRegistrationPlanTest {
    @Test
    fun declaredCapabilitiesMapToContractValuesInRegistryOrder() {
        val descriptor = descriptor(
            servedContracts = listOf(
                "geocoding",
                "routing",
                "distance_matrix",
                "place_search",
                "traffic_incidents",
                "traffic_flow",
                "raster_tiles",
            ),
        )

        assertEquals(
            listOf(
                ProviderContract.GEOCODING,
                ProviderContract.ROUTING,
                ProviderContract.DISTANCE_MATRIX,
                ProviderContract.PLACE_SEARCH,
                ProviderContract.TRAFFIC_INCIDENTS,
                ProviderContract.TRAFFIC_FLOW,
                ProviderContract.RASTER_TILES,
            ),
            ProviderRegistrationPlan.contractsFor(descriptor),
        )
    }

    @Test
    fun registryNamesTheContractDoesNotDefineAreSkipped() {
        val descriptor = descriptor(
            servedContracts = listOf("routing", "a-future-capability"),
        )

        assertEquals(
            listOf(ProviderContract.ROUTING),
            ProviderRegistrationPlan.contractsFor(descriptor),
        )
    }

    @Test
    fun aSingleCapabilityDescriptorMapsToThatCapability() {
        val descriptor = descriptor(servedContracts = listOf("routing"))

        assertEquals(
            listOf(ProviderContract.ROUTING),
            ProviderRegistrationPlan.contractsFor(descriptor),
        )
    }

    @Test
    fun aDescriptorWithoutCapabilitiesMapsToNothing() {
        val descriptor = descriptor(servedContracts = emptyList())

        assertTrue(ProviderRegistrationPlan.contractsFor(descriptor).isEmpty())
    }

    @Test
    fun descriptorServesMatchesTheDeclaredCapability() {
        val descriptor = descriptor(
            servedContracts = listOf("routing", "raster_tiles"),
        )

        assertTrue(
            ProviderRegistrationPlan.descriptorServes(descriptor, ProviderContract.ROUTING),
        )
        assertTrue(
            ProviderRegistrationPlan.descriptorServes(descriptor, ProviderContract.RASTER_TILES),
        )
        assertFalse(
            ProviderRegistrationPlan.descriptorServes(descriptor, ProviderContract.GEOCODING),
        )
    }

    @Test
    fun credentialedRasterTileProviderRequiresAnAppliedCredential() {
        val credentialedRaster = descriptor(
            providerKey = "full-service",
            credentialLabel = "API key",
            servedContracts = listOf("raster_tiles"),
            rasterTileCacheCeilingDays = 30,
        )
        val descriptors = listOf(credentialedRaster)

        assertSame(
            credentialedRaster,
            ProviderRegistrationPlan.credentialedRasterTileProvider(
                descriptors,
                setOf("full-service"),
            ),
        )
        assertNull(
            ProviderRegistrationPlan.credentialedRasterTileProvider(descriptors, emptySet()),
        )
    }

    @Test
    fun aKeylessRasterSourceIsNeverTheCacheOwner() {
        val keylessRaster = descriptor(
            providerKey = "keyless-source",
            credentialLabel = "",
            servedContracts = listOf("raster_tiles"),
        )

        assertNull(
            ProviderRegistrationPlan.credentialedRasterTileProvider(
                listOf(keylessRaster),
                setOf("keyless-source"),
            ),
        )
    }

    @Test
    fun aCredentialedProviderWithoutRasterTilesIsNeverTheCacheOwner() {
        val credentialedRouter = descriptor(
            providerKey = "full-service",
            credentialLabel = "API key",
            servedContracts = listOf("routing"),
        )

        assertNull(
            ProviderRegistrationPlan.credentialedRasterTileProvider(
                listOf(credentialedRouter),
                setOf("full-service"),
            ),
        )
    }

    /** One registry descriptor shaped like the shared artifact's rows. */
    private fun descriptor(
        providerKey: String = "full-service",
        credentialLabel: String = "API key",
        servedContracts: List<String> = emptyList(),
        isRegistrationAvailableOnAndroid: Boolean = true,
        rasterTileCacheCeilingDays: Int? = null,
    ): ProviderCredentialDescriptor = ProviderCredentialDescriptor(
        providerKey = providerKey,
        displayName = "Full Service",
        credentialLabel = credentialLabel,
        isRegistrationAvailableOnAndroid = isRegistrationAvailableOnAndroid,
        attributionText = "",
        termsNote = "",
        settingsHint = "",
        rasterUrlTemplate = "",
        maximumZoomLevel = 0,
        rasterTileCacheCeilingDays = rasterTileCacheCeilingDays,
        vectorUrlTemplate = "",
        vectorStyleSchema = "",
        vectorStyles = emptyList(),
        servedContracts = servedContracts,
    )
}
