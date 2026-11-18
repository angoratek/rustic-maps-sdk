package com.rusticmaps.sdk

import java.util.concurrent.ConcurrentHashMap
import uniffi.rustic_maps.MapSdk
import uniffi.rustic_maps.ProviderContract
import uniffi.rustic_maps.ProviderRegistration
import uniffi.rustic_maps.RouteCapabilities
import uniffi.rustic_maps.UrlTemplateRasterTileSource

/**
 * Thin facade over the uniffi-generated [MapSdk]: constructs the SDK keyless
 * and installs provider registrations only when a runtime credential is
 * applied (the design language's §6.5 and the provider-credential guide).
 *
 * No provider credential is a build input. [applyProviderCredential] is the
 * one credential entry point: it reads the provider's descriptor from the
 * shared provider registry (decoded by the application) and registers every
 * capability that descriptor declares through [MapSdk.configureProvider], so a
 * new provider is a registry descriptor plus the SDK's own registration arm,
 * never an edit here.
 *
 * A refused credential surfaces verbatim from the generated call as
 * [uniffi.rustic_maps.SdkException] to the caller.
 *
 * The offline-maps demo registers the credential's provider set a second time,
 * with the raster-tile contract wrapped in the disk response cache
 * (`setProviderCredentialWithTileCache`): re-registration replaces every slot,
 * including the cached raster-tile slot, so the demo runs one call with the
 * cache file and budget once it holds them.
 *
 * @param providerDescriptors the descriptors of the shared provider registry,
 *   in artifact order. The facade reads availability, credential labels and
 *   declared capabilities from them and never names a provider itself.
 */
class MapsSdkFacade(
    private val providerDescriptors: List<ProviderCredentialDescriptor>,
) : AutoCloseable {
    /** The generated SDK object every surface, provider call and camera builds from. */
    val nativeSdk: MapSdk = MapSdk()

    /**
     * Releases the native SDK handle. The facade owns the one [MapSdk] it
     * constructed; the host calls this from its teardown path after the map
     * view that reads the SDK has been destroyed. Closing twice is a no-op
     * because the generated object drops its handle on the first call.
     */
    override fun close() {
        nativeSdk.close()
    }

    /**
     * The credential each provider key was registered with in this session.
     * It is session memory (never written to disk by this facade) so a
     * per-capability selection, a base-map restore or the tile-cache
     * registration can repeat the credential the application already holds
     * instead of asking for it again. It is never logged.
     */
    private val appliedProviderCredentials: ConcurrentHashMap<String, String> =
        ConcurrentHashMap()

    /**
     * Registers every capability the descriptor for `providerKey` declares
     * with `credential`, one [MapSdk.configureProvider] call per capability.
     *
     * The capabilities come from the registry descriptor, never from a
     * provider name in this file. An unknown key, or one this platform does
     * not register, is refused with a clear [IllegalArgumentException] naming
     * the key (a provider without a registration must never pretend to work).
     * Each capability registers independently, so a refusal at one capability
     * leaves the previously serving provider in that slot.
     * A blank credential and the SDK's own validation failures both surface
     * verbatim to the caller. The credential is retained in memory after the
     * last capability registers and is never logged.
     */
    fun applyProviderCredential(providerKey: String, credential: String) {
        require(credential.isNotBlank()) {
            "a provider credential must not be blank"
        }
        val descriptor = registrationDescriptorFor(providerKey)
        val contracts = ProviderRegistrationPlan.contractsFor(descriptor)
        if (contracts.isEmpty()) {
            throw IllegalArgumentException(
                "provider '$providerKey' has no registration on this platform yet",
            )
        }
        contracts.forEach { contract ->
            nativeSdk.configureProvider(contract, providerKey, credential)
        }
        appliedProviderCredentials[providerKey] = credential
    }

    /**
     * Chooses the provider that serves one capability
     * ([ProviderContract]), leaving every other capability untouched. The
     * credential repeats the value the application already holds for a
     * credentialed provider, and is empty for a provider that needs none. A
     * refused call surfaces the SDK's own exception verbatim and leaves the
     * previously serving provider in place.
     */
    fun configureProvider(contract: ProviderContract, providerKey: String, credential: String) {
        nativeSdk.configureProvider(contract, providerKey, credential)
        if (credential.isNotBlank()) {
            appliedProviderCredentials[providerKey] = credential
        }
    }

    /** Reports which provider serves each capability, in the SDK's stable order. */
    fun providerRegistrations(): List<ProviderRegistration> = nativeSdk.providerRegistrations()

    /**
     * The serving routing provider's capabilities: which transport modes it
     * can route for and whether it can minimize distance. Throws the SDK's
     * verbatim exception — `ProviderNotConfigured` while no routing provider
     * is installed — so the caller states the reason instead of guessing.
     */
    fun routeCapabilities(): RouteCapabilities = nativeSdk.routeCapabilities()

    /**
     * The credential this session registered the provider with, or null when
     * the application holds none (a keyless provider, or one not applied
     * yet). Session memory only.
     */
    fun appliedProviderCredential(providerKey: String): String? =
        appliedProviderCredentials[providerKey]

    /**
     * Registers a provider-neutral base map (M9's native base-map switch): a
     * raster tileset the shared provider registry declares as a `{z}/{x}/{y}`
     * URL template, served by the SDK itself with no vendor code involved.
     * The registration replaces the raster-tile slot only, so the
     * credential-registered contracts (geocoding, routing, search, traffic)
     * keep serving whichever base map is active.
     *
     * A malformed template, a blank source name or an inverted zoom range
     * throws the SDK's verbatim [uniffi.rustic_maps.SdkException]; the
     * previously serving source stays installed.
     */
    fun applyUrlTemplateRasterTileSource(source: UrlTemplateRasterTileSource) {
        nativeSdk.registerUrlTemplateRasterTileSource(source)
    }

    /**
     * Re-registers the applied credential's raster-tile capability so its base
     * map serves again — the switch back from a credential-less source. Only
     * the raster-tile slot moves, so the provider's other capabilities keep
     * serving. A cached provider installed in that slot stops serving, so the
     * caller re-registers the cache afterwards when the offline demo owns one.
     *
     * Refuses honestly with an [IllegalArgumentException] when `providerKey`
     * has no registration on this platform or declares no raster-tile
     * capability, and with an [IllegalStateException] when no credential for
     * it has been applied in this session.
     */
    fun restoreCredentialedBaseMap(providerKey: String) {
        val descriptor = registrationDescriptorFor(providerKey)
        require(ProviderRegistrationPlan.descriptorServes(descriptor, ProviderContract.RASTER_TILES)) {
            "provider '$providerKey' has no credential-based base map on this platform"
        }
        val credential = appliedProviderCredentials[providerKey] ?: throw IllegalStateException(
            "no credential is applied for provider '$providerKey'; " +
                "set one in Settings before selecting its base map",
        )
        nativeSdk.configureProvider(ProviderContract.RASTER_TILES, providerKey, credential)
    }

    /**
     * Re-registers the applied credentialed raster-tile provider with its
     * raster-tile contract wrapped in the disk response cache opened at
     * `tileCacheDatabasePath` under the absolute byte budget
     * `tileCacheAbsoluteMaximumBytes` (§4.1's offline maps card).
     *
     * The provider is found from the session's applied credentials and the
     * registry descriptor that declares raster tiles; its key and the
     * credential already held for it are passed to the SDK, so the facade
     * names no provider itself. Refuses honestly with an
     * [IllegalStateException] when no credentialed raster-tile provider has
     * been applied: the cache variant repeats the key and credential, so
     * registering it keyless is never attempted.
     * Any SDK refusal surfaces verbatim to the caller.
     */
    fun registerTileCacheWithProviders(
        tileCacheDatabasePath: String,
        tileCacheAbsoluteMaximumBytes: ULong,
    ) {
        val rasterTileProvider = ProviderRegistrationPlan.credentialedRasterTileProvider(
            providerDescriptors,
            appliedProviderCredentials.keys,
        ) ?: throw IllegalStateException(
            "no credential is applied for a provider whose raster tiles the SDK can cache; " +
                "set one in Settings before registering the tile cache",
        )
        // A registration and a credential application may run on different
        // threads, so read the credential once rather than probing the map
        // and then reading it again.
        val rasterCredential = appliedProviderCredentials[rasterTileProvider.providerKey]
            ?: throw IllegalStateException(
                "no credential is applied for a provider whose raster tiles the SDK can cache; " +
                    "set one in Settings before registering the tile cache",
            )
        nativeSdk.setProviderCredentialWithTileCache(
            rasterTileProvider.providerKey,
            rasterCredential,
            tileCacheDatabasePath,
            tileCacheAbsoluteMaximumBytes,
        )
    }

    /**
     * The descriptor whose credentials this platform registers for
     * `providerKey`, or a clear refusal when the shared registry declares none.
     */
    private fun registrationDescriptorFor(providerKey: String): ProviderCredentialDescriptor =
        providerDescriptors.firstOrNull { descriptor ->
            descriptor.providerKey == providerKey && descriptor.isRegistrationAvailableOnAndroid
        } ?: throw IllegalArgumentException(
            "provider '$providerKey' has no registration on this platform yet",
        )
}
