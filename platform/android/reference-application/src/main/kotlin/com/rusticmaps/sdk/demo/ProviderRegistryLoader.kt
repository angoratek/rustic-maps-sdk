package com.rusticmaps.sdk.demo

import android.content.res.AssetManager
import com.rusticmaps.sdk.ProviderCredentialDescriptor
import com.rusticmaps.sdk.VectorStyleDescriptor
import org.json.JSONObject

/**
 * The styled vector base a keyless start opens on (owner ruling 2026-10-31,
 * PRD §6 item 18): the provider key and committed style id the registry names
 * at its top level. Resolved against the descriptors by the reference
 * application, so a registry whose declaration is incomplete falls back to
 * the credential-less raster source rather than installing nothing.
 */
data class KeylessBaseMapDescriptor(
    /** The provider whose vector tileset the base uses. */
    val providerKey: String,
    /** The committed style id the base paints with. */
    val styleId: String,
)

/** The decoded shared provider registry. */
data class ProviderRegistry(
    /** The registry schema version the artifact declares. */
    val schemaVersion: Int,
    /** The provider descriptors in artifact order. */
    val providers: List<ProviderCredentialDescriptor>,
    /**
     * The reference apps' own `User-Agent` template (one `%s` slot for the
     * app's SDK version), or blank when the artifact declares none. It sits
     * in the shared artifact because all four surfaces identify themselves
     * the same way to the public tile services their base maps come from.
     */
    val referenceAppUserAgentTemplate: String,
    /**
     * The styled vector base a keyless start opens on, or null when the
     * artifact declares none (the keyless start then keeps the
     * credential-less raster source).
     */
    val keylessBaseMap: KeylessBaseMapDescriptor?,
)

/**
 * Decodes the shared provider registry. The repository file
 * `platform/providers.json` is copied into the application assets at build
 * time (a Gradle task, never a checked-in duplicate), so the descriptors
 * cannot drift from the registry the dev server, the playground and the
 * other refapps consume.
 *
 * The section is read tolerantly (a registry without descriptors yields no
 * rows) so an older artifact never breaks a keyless start; a missing
 * `android` availability flag reads as unavailable, the honest default.
 */
object ProviderRegistryLoader {
    /** Asset name the copy task writes the registry to inside the application. */
    const val REGISTRY_ASSET_NAME: String = "providers.json"

    /** Loads and decodes the registry from the application's assets. */
    fun load(assets: AssetManager): ProviderRegistry {
        assets.open(REGISTRY_ASSET_NAME).use { stream ->
            val registryText = stream.readBytes().decodeToString()
            return decode(registryText)
        }
    }

    /** Decodes the registry's JSON text (separate from loading for tests). */
    fun decode(registryText: String): ProviderRegistry {
        val root = JSONObject(registryText)
        val descriptorArray = root.optJSONArray("providers")
        val providers = (0 until (descriptorArray?.length() ?: 0)).map { index ->
            val descriptorObject = descriptorArray!!.getJSONObject(index)
            ProviderCredentialDescriptor(
                providerKey = descriptorObject.getString("provider_key"),
                displayName = descriptorObject.getString("display_name"),
                credentialLabel = descriptorObject.optString("credential_label", ""),
                isRegistrationAvailableOnAndroid = descriptorObject
                    .optJSONObject("registration_available")
                    ?.optBoolean("android", false)
                    ?: false,
                attributionText = descriptorObject.optString("attribution_text", ""),
                termsNote = descriptorObject.optString("terms_note", ""),
                settingsHint = descriptorObject.optString("settings_hint", ""),
                demoBaseUrl = descriptorObject.optString("demo_base_url", ""),
                demoBaseUrlNote = descriptorObject.optString("demo_base_url_note", ""),
                rasterUrlTemplate = descriptorObject.optString("raster_url_template", ""),
                maximumZoomLevel = descriptorObject.optInt("maximum_zoom_level", 0),
                rasterTileCacheCeilingDays =
                    if (descriptorObject.isNull("raster_tile_cache_ceiling_days")) {
                        null
                    } else {
                        descriptorObject.optInt("raster_tile_cache_ceiling_days")
                    },
                vectorUrlTemplate = descriptorObject.optString("url_template", ""),
                vectorStyleSchema = descriptorObject.optString("vector_style", ""),
                vectorStyles = decodeVectorStyles(descriptorObject.optJSONArray("vector_styles")),
                servedContracts = decodeServedContracts(
                    descriptorObject.optJSONArray("served_contracts"),
                ),
            )
        }
        return ProviderRegistry(
            schemaVersion = root.optInt("schema_version", 0),
            providers = providers,
            referenceAppUserAgentTemplate = root.optString(
                "reference_app_user_agent_template",
                "",
            ),
            keylessBaseMap = decodeKeylessBaseMap(root.optJSONObject("keyless_base_map")),
        )
    }

    /**
     * Decodes the registry's top-level `keyless_base_map` (null when the
     * artifact declares none, or when either half is blank — the caller then
     * keeps the credential-less raster start).
     */
    private fun decodeKeylessBaseMap(keylessBaseMapObject: JSONObject?): KeylessBaseMapDescriptor? {
        val providerKey = keylessBaseMapObject?.optString("provider_key", "").orEmpty()
        val styleId = keylessBaseMapObject?.optString("style_id", "").orEmpty()
        if (providerKey.isBlank() || styleId.isBlank()) {
            return null
        }
        return KeylessBaseMapDescriptor(providerKey = providerKey, styleId = styleId)
    }

    /**
     * Decodes a descriptor's `served_contracts` list (empty when it declares
     * none, so an older artifact offers the provider for no capability rather
     * than guessing one).
     */
    private fun decodeServedContracts(servedContractsArray: org.json.JSONArray?): List<String> =
        (0 until (servedContractsArray?.length() ?: 0)).mapNotNull { contractIndex ->
            servedContractsArray!!
                .optString(contractIndex)
                .takeIf { contractName -> contractName.isNotBlank() }
        }

    /** Decodes a descriptor's `vector_styles` list (empty when it declares none). */
    private fun decodeVectorStyles(vectorStylesArray: org.json.JSONArray?): List<VectorStyleDescriptor> =
        (0 until (vectorStylesArray?.length() ?: 0)).map { styleIndex ->
            val styleObject = vectorStylesArray!!.getJSONObject(styleIndex)
            VectorStyleDescriptor(
                styleId = styleObject.getString("style_id"),
                displayName = styleObject.getString("display_name"),
                theme = styleObject.optString("theme", ""),
                attributionText = styleObject.optString("attribution_text", ""),
                documentUrl = styleObject.optString("document_url", ""),
            )
        }
}
