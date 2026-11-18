package com.rusticmaps.sdk

import uniffi.rustic_maps.ProviderContract

/**
 * The descriptor-driven registration rules [MapsSdkFacade] applies.
 *
 * The shared provider registry names the capabilities a provider serves with
 * the frozen contract's snake_case storage names. This object maps those names
 * onto [ProviderContract] values and answers the two questions the facade
 * asks: every capability one credential installs, and which applied credential
 * owns the SDK's disk
 * raster-tile cache.
 *
 * The rules live apart from the generated SDK object so they can be tested
 * without loading the native library.
 */
internal object ProviderRegistrationPlan {
    /**
     * The registry's storage name for each capability the frozen contract
     * defines, in the registry's declared order. A name the contract does not
     * define is absent, so a newer artifact never breaks an older wrapper.
     */
    private val CONTRACT_NAMES: Map<String, ProviderContract> = linkedMapOf(
        "geocoding" to ProviderContract.GEOCODING,
        "routing" to ProviderContract.ROUTING,
        "distance_matrix" to ProviderContract.DISTANCE_MATRIX,
        "place_search" to ProviderContract.PLACE_SEARCH,
        "traffic_incidents" to ProviderContract.TRAFFIC_INCIDENTS,
        "traffic_flow" to ProviderContract.TRAFFIC_FLOW,
        "elevation" to ProviderContract.ELEVATION,
        "raster_tiles" to ProviderContract.RASTER_TILES,
        "vector_tiles" to ProviderContract.VECTOR_TILES,
        "offline_routing" to ProviderContract.OFFLINE_ROUTING,
    )

    /**
     * The capability contracts `descriptor` declares, in registry order.
     * Names the frozen contract does not define are skipped, so a registry
     * that gains a capability after this wrapper ships stays readable.
     */
    fun contractsFor(descriptor: ProviderCredentialDescriptor): List<ProviderContract> =
        descriptor.servedContracts.mapNotNull { contractName -> CONTRACT_NAMES[contractName] }

    /**
     * Whether `descriptor` declares the capability [contract], judged by the
     * registry's storage names.
     */
    fun descriptorServes(
        descriptor: ProviderCredentialDescriptor,
        contract: ProviderContract,
    ): Boolean = descriptor.servedContracts.any { contractName ->
        CONTRACT_NAMES[contractName] == contract
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
