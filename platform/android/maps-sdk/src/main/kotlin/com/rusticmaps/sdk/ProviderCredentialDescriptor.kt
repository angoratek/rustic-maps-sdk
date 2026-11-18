package com.rusticmaps.sdk

/** The registry's contract name for a provider's vector tileset. */
private const val VECTOR_TILES_CONTRACT_NAME: String = "vector_tiles"

/**
 * One vector style a registry descriptor offers: the committed style id the
 * SDK paints the base with, the consumer-facing style name a base-map
 * dropdown shows, the theme the style belongs to (`light`/`dark`), and the
 * attribution line the style's design requires.
 *
 * The record is part of [ProviderCredentialDescriptor], the registry shape the
 * published SDK facade accepts; the loader that decodes the shared artifact
 * stays in the reference application.
 */
data class VectorStyleDescriptor(
    /** The committed style id the SDK paints with (`bright-light`, …). */
    val styleId: String,
    /** Consumer-facing style name (`Bright`, `Fiord`, `Positron`). */
    val displayName: String,
    /** The theme the style was designed on. */
    val theme: String,
    /** The attribution line the style's design requires. */
    val attributionText: String,
    /**
     * The provider-published style document this style paints with, or blank
     * when the SDK carries the style. A hosted document is fetched with the
     * registered credential and kept for the session, so the application
     * ships no vendor style file.
     */
    val documentUrl: String = "",
)

/**
 * One provider descriptor of the shared provider registry:
 * a settings surface renders one row per descriptor and offers the provider
 * in each capability picker its `served_contracts` declares, so a new provider
 * is a registry entry plus a registration dispatch arm, never a view rewrite.
 * The descriptor carries labels and availability only — never a credential
 * value.
 *
 * The type lives in the published package because [MapsSdkFacade] takes the
 * decoded descriptors; the decoder itself is reference-application code.
 */
data class ProviderCredentialDescriptor(
    /** Stable dispatch key the credential applier switches on. */
    val providerKey: String,
    /** Consumer-facing provider name. */
    val displayName: String,
    /** Consumer-facing label of the credential field. */
    val credentialLabel: String,
    /** Whether this platform has a registration arm for the provider. */
    val isRegistrationAvailableOnAndroid: Boolean,
    /** The provider's attribution/terms line. */
    val attributionText: String,
    /** The provider's terms note (the attribution panel's second block). */
    val termsNote: String,
    /** Where to create a credential, shown under the field. */
    val settingsHint: String,
    /**
     * The public demo deployment address of a keyless provider (the registry's
     * `demo_base_url`), or blank when the descriptor declares none. The
     * reference applications pass it as the contract's deployment-address
     * argument; a keyless provider without one is offered only honestly, never
     * with an empty address that would silently reach a local default.
     */
    val demoBaseUrl: String = "",
    /** The descriptor's note about the demo address, or blank when it declares none. */
    val demoBaseUrlNote: String = "",
    /**
     * The provider's raster-tile URL template (`{z}/{x}/{y}`), or blank when
     * the provider publishes no raster source of that shape. A keyless
     * descriptor with a template serves its own base map without any
     * credential; a settings surface offers it in the base-map section.
     */
    val rasterUrlTemplate: String,
    /** The provider's highest served tile zoom level (the descriptor default of zero). */
    val maximumZoomLevel: Int,
    /**
     * The raster-tile caching ceiling the provider's terms declare, in days,
     * or null when it declares none. Only a provider that declares one may
     * install the SDK's disk tile cache; another credentialed raster
     * provider registers its tiles uncached.
     */
    val rasterTileCacheCeilingDays: Int? = null,
    /**
     * The provider's vector-tile URL template (`{z}/{x}/{y}`), or blank when
     * the provider publishes no vector tileset.
     */
    val vectorUrlTemplate: String,
    /**
     * The tile schema the provider's vector tileset carries (`carto`,
     * `openmaptiles`); informational, because the styles the base offers are
     * named by the descriptor's own list.
     */
    val vectorStyleSchema: String,
    /** The vector styles the registry offers for this provider, in artifact order. */
    val vectorStyles: List<VectorStyleDescriptor>,
    /**
     * The capabilities the provider serves, named with the contract's
     * snake_case storage names (`geocoding`, `routing`, …). A settings
     * surface offers the provider only for the capabilities it declares,
     * never for one this build cannot serve.
     */
    val servedContracts: List<String>,
) {
    /**
     * Whether this platform can register the provider as a base map without a
     * credential: the provider-neutral URL-template raster arm exists on all
     * three native platforms.
     */
    val isCredentiallessBaseMapAvailableOnAndroid: Boolean
        get() = rasterUrlTemplate.isNotBlank() && isRegistrationAvailableOnAndroid

    /**
     * Whether the provider offers a styled vector base: a vector tileset URL
     * template plus at least one committed style. The vector base is served
     * by the shipped `MapView` on every platform through the additive contract
     * member, so no registration arm gates it.
     */
    val offersVectorBase: Boolean
        get() = vectorUrlTemplate.isNotBlank() && vectorStyles.isNotEmpty()

    /**
     * Whether the provider offers a styled vector base from style documents
     * it publishes: it declares the vector-tile contract, this platform
     * registers it, and it names at least one style. The rows are listed
     * whether or not the credential is applied; without it they state
     * `Needs <credential label>` and cannot be selected.
     */
    val offersHostedVectorBase: Boolean
        get() = servedContracts.contains(VECTOR_TILES_CONTRACT_NAME) &&
            vectorStyles.isNotEmpty() &&
            isRegistrationAvailableOnAndroid
}
