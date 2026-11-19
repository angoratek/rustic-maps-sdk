package com.rusticmaps.sdk

import com.rusticmaps.sdk.ProviderCredentialDescriptor
import java.io.File
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.rustic_maps.ProviderContract
import uniffi.rustic_maps.providerContractFromRegistryName
import uniffi.rustic_maps.registryNameForProviderContract

/**
 * Unit tests of the descriptor-driven registration rules. They exercise the
 * planner over the contract-owned registry-name mapping without constructing
 * the SDK object; the generated mapping calls load the host cdylib
 * `scripts/build-android.sh` stages for the JVM suite.
 */
class ProviderRegistrationPlanTest {
    @Test
    fun everyRegistryDeclaredCapabilityMapsToAContract() {
        val registryNames = sharedRegistryServedContractNames()

        assertTrue("the shared registry must declare capabilities", registryNames.isNotEmpty())
        for (registryName in registryNames) {
            val contract = providerContractFromRegistryName(registryName)
            assertNotNull(
                "the registry declares '$registryName' but no contract maps it",
                contract,
            )
            assertEquals(
                listOf(contract),
                ProviderRegistrationPlan.contractsFor(descriptor(servedContracts = listOf(registryName))),
            )
        }
    }

    @Test
    fun everyContractRoundTripsThroughItsRegistryName() {
        for (contract in ProviderContract.values()) {
            val registryName = registryNameForProviderContract(contract)
            assertEquals(
                contract,
                providerContractFromRegistryName(registryName),
            )
        }
    }

    @Test
    fun aRegistryNameTheContractDoesNotDefineMapsToNothing() {
        assertNull(providerContractFromRegistryName("a-future-capability"))
        assertTrue(
            ProviderRegistrationPlan
                .contractsFor(descriptor(servedContracts = listOf("a-future-capability")))
                .isEmpty(),
        )
    }

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

    /**
     * Every capability name the shared provider registry declares across its
     * descriptors, read from the artifact's single committed source (the
     * `repository.root` system property names the checkout).
     */
    private fun sharedRegistryServedContractNames(): Set<String> {
        val repositoryRoot = requireNotNull(System.getProperty("repository.root")) {
            "the unit suite needs the repository.root system property"
        }
        val registry = JSONObject(
            File(repositoryRoot, "platform/providers.json").readText(),
        )
        val providers = registry.getJSONArray("providers")
        val registryNames = linkedSetOf<String>()
        for (providerIndex in 0 until providers.length()) {
            val servedContracts = providers
                .getJSONObject(providerIndex)
                .optJSONArray("served_contracts") ?: continue
            for (contractIndex in 0 until servedContracts.length()) {
                registryNames += servedContracts.getString(contractIndex)
            }
        }
        return registryNames
    }
}
