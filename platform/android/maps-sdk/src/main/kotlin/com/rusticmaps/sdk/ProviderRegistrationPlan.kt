package com.rusticmaps.sdk

import uniffi.rustic_maps.ProviderContract
import uniffi.rustic_maps.providerContractFromRegistryName

/**
 * The descriptor-driven registration rules [MapsSdkFacade] applies.
 *
 * The shared provider registry names the capabilities a provider serves with
 * the frozen contract's snake_case storage names. The frozen contract owns the
 * one mapping from those names onto [ProviderContract] values; this object
 * asks it and answers the two questions the facade asks: every capability one
 * credential installs, and which applied credential owns the SDK's disk
 * raster-tile cache.
 *
 * The rules live apart from the generated SDK object so a unit test exercises
 * them without constructing it; the name mapping itself is a contract call.
 */
internal object ProviderRegistrationPlan {
    /**
     * The capability contracts `descriptor` declares, in registry order.
     * Names the frozen contract does not define are skipped, so a registry
     * that gains a capability after this wrapper ships stays readable.
     */
    fun contractsFor(descriptor: ProviderCredentialDescriptor): List<ProviderContract> =
        descriptor.servedContracts.mapNotNull { contractName ->
            providerContractFromRegistryName(contractName)
        }

    /**
     * Whether `descriptor` declares the capability [contract], judged by the
     * registry's storage names.
     */
    fun descriptorServes(
        descriptor: ProviderCredentialDescriptor,
        contract: ProviderContract,
    ): Boolean = descriptor.servedContracts.any { contractName ->
        providerContractFromRegistryName(contractName) == contract
    }

    /**
     * The applied provider whose raster tiles the SDK's disk cache serves, or
     * null when this session holds no such credential.
     *
     * A descriptor qualifies when the application applied a credential for it
     * and its terms declare a raster-tile caching ceiling. A blank credential
     * label marks a keyless source, which the SDK serves uncached, and a
     * credentialed raster provider without a declared ceiling is registered
     * without the cache rather than refused.
     */
    fun credentialedRasterTileProvider(
        descriptors: List<ProviderCredentialDescriptor>,
        appliedProviderKeys: Set<String>,
    ): ProviderCredentialDescriptor? = descriptors.firstOrNull { descriptor ->
        descriptor.providerKey in appliedProviderKeys &&
            descriptor.rasterTileCacheCeilingDays != null &&
            descriptorServes(descriptor, ProviderContract.RASTER_TILES)
    }
}
