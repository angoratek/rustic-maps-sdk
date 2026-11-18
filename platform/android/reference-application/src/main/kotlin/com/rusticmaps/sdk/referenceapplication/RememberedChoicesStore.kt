package com.rusticmaps.sdk.referenceapplication

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject

/**
 * The explicit choices the reference application remembers across launches
 * (docs/design/ReferenceAppSettingsAndPersistence.md): the theme, the base map, the
 * demo plan and overlay scenes, the camera preset, the simulated-trip speed
 * multiplier and the last search texts. Transient state — the open sheet, the
 * current camera pose, a route result — is never part of this record, and the
 * provider credential stays in its own opt-in store.
 */
data class RememberedChoices(
    /** The three-way theme choice (§3). */
    val themeChoice: ThemeChoice = ThemeChoice.AUTO,
    /** The base map's provider key, or null while none was chosen. */
    val baseMapProviderKey: String? = null,
    /** The styled vector base's style id; null names the provider's raster source. */
    val baseMapStyleId: String? = null,
    /** The demo-plan picker's selected scenario name. */
    val demoPlanScenarioName: String? = null,
    /** The enabled overlay-scene keys of the shared artifact. */
    val enabledSceneKeys: Set<String> = emptySet(),
    /** The selected camera preset's identifier. */
    val cameraPresetIdentifier: String? = null,
    /** The simulated-trip playback speed multiplier. */
    val simulatedTripSpeedMultiplier: Double? = null,
    /** The origin field's last text. */
    val originSearchText: String = "",
    /** The destination field's last text. */
    val destinationSearchText: String = "",
    /** The last text submitted to the plan search. */
    val lastSearchQueryText: String = "",
    /**
     * The provider chosen for each capability: the capability's storage name
     * (`Routing`) mapped to the provider's registry key. Empty while no
     * capability was chosen individually.
     */
    val capabilityProviders: Map<String, String> = emptyMap(),
    /** The Directions control's remembered transport mode (Car by default). */
    val directionsTransportationMode: DirectionsTransportationMode =
        DirectionsTransportationMode.CAR,
    /** The Directions control's remembered route strategy (Fastest by default). */
    val directionsRouteStrategy: DirectionsRouteStrategy =
        DirectionsRouteStrategy.FASTEST,
    /** Whether the map's traffic layer was on (off by default). */
    val trafficLayerEnabled: Boolean = false,
    /** Whether the device location source was on (off by default). */
    val deviceLocationEnabled: Boolean = false,
)

/**
 * The device-local store of [RememberedChoices]: one versioned JSON value in
 * this application's private preferences, written on every explicit choice
 * and read once at startup. A record written by another version is dropped
 * rather than guessed at, and unknown keys are ignored.
 */
class RememberedChoicesStore(context: Context) {
    private val preferences = context.applicationContext.getSharedPreferences(
        PREFERENCES_NAME,
        Context.MODE_PRIVATE,
    )

    /** The remembered record, or null when this device never made one. */
    fun load(): RememberedChoices? {
        val recordText = preferences.getString(RECORD_PREFERENCE_KEY, null) ?: return null
        val record = try {
            JSONObject(recordText)
        } catch (malformedRecord: Exception) {
            return null
        }
        if (record.optInt(VERSION_FIELD, -1) != RECORD_VERSION) {
            return null
        }
        val directions = decodeDirections(record)
        return RememberedChoices(
            themeChoice = ThemeChoice.fromStorageValue(
                record.optString(THEME_FIELD).takeIf { themeText ->
                    themeText.isNotBlank()
                },
            ),
            baseMapProviderKey = record.optJSONObject(BASE_MAP_FIELD)
                ?.optString(PROVIDER_KEY_FIELD)
                ?.takeIf { providerKey -> providerKey.isNotBlank() },
            baseMapStyleId = record.optJSONObject(BASE_MAP_FIELD)
                ?.optString(STYLE_ID_FIELD)
                ?.takeIf { styleId -> styleId.isNotBlank() },
            demoPlanScenarioName = record.optJSONObject(DEMO_PLAN_FIELD)
                ?.optString(SCENARIO_NAME_FIELD)
                ?.takeIf { scenarioName -> scenarioName.isNotBlank() },
            enabledSceneKeys = record.optJSONArray(SCENES_FIELD)
                .sceneKeysOrEmpty(),
            cameraPresetIdentifier = record.optJSONObject(CAMERA_PRESET_FIELD)
                ?.optString(IDENTIFIER_FIELD)
                ?.takeIf { identifier -> identifier.isNotBlank() },
            simulatedTripSpeedMultiplier = record
                .takeIf { recordObject -> recordObject.has(SPEED_MULTIPLIER_FIELD) }
                ?.optDouble(SPEED_MULTIPLIER_FIELD),
            originSearchText = record.optJSONObject(SEARCH_FIELD)
                ?.optString(ORIGIN_FIELD)
                .orEmpty(),
            destinationSearchText = record.optJSONObject(SEARCH_FIELD)
                ?.optString(DESTINATION_FIELD)
                .orEmpty(),
            lastSearchQueryText = record.optJSONObject(SEARCH_FIELD)
                ?.optString(QUERY_FIELD)
                .orEmpty(),
            capabilityProviders = decodeCapabilityProviders(record),
            directionsTransportationMode = directions.transportationMode,
            directionsRouteStrategy = directions.routeStrategy,
            trafficLayerEnabled = record.optBoolean(TRAFFIC_LAYER_ENABLED_FIELD, false),
            deviceLocationEnabled = record.optBoolean(DEVICE_LOCATION_ENABLED_FIELD, false),
        )
    }

    /** Remembers the complete record, replacing any earlier one. */
    fun store(choices: RememberedChoices) {
        val record = JSONObject()
            .put(VERSION_FIELD, RECORD_VERSION)
            .put(THEME_FIELD, choices.themeChoice.storageValue)
        if (choices.baseMapProviderKey != null) {
            val baseMap = JSONObject()
                .put(PROVIDER_KEY_FIELD, choices.baseMapProviderKey)
            choices.baseMapStyleId?.let { styleId -> baseMap.put(STYLE_ID_FIELD, styleId) }
            record.put(BASE_MAP_FIELD, baseMap)
        }
        choices.demoPlanScenarioName?.let { scenarioName ->
            record.put(
                DEMO_PLAN_FIELD,
                JSONObject().put(SCENARIO_NAME_FIELD, scenarioName),
            )
        }
        record.put(SCENES_FIELD, JSONArray(choices.enabledSceneKeys.sorted()))
        choices.cameraPresetIdentifier?.let { identifier ->
            record.put(
                CAMERA_PRESET_FIELD,
                JSONObject().put(IDENTIFIER_FIELD, identifier),
            )
        }
        choices.simulatedTripSpeedMultiplier?.let { multiplier ->
            record.put(SPEED_MULTIPLIER_FIELD, multiplier)
        }
        record.put(
            SEARCH_FIELD,
            JSONObject()
                .put(ORIGIN_FIELD, choices.originSearchText)
                .put(DESTINATION_FIELD, choices.destinationSearchText)
                .put(QUERY_FIELD, choices.lastSearchQueryText),
        )
        record.put(
            CAPABILITY_PROVIDERS_FIELD,
            encodeCapabilityProviders(choices.capabilityProviders),
        )
        encodeDirections(
            record = record,
            transportationMode = choices.directionsTransportationMode,
            routeStrategy = choices.directionsRouteStrategy,
        )
        record.put(TRAFFIC_LAYER_ENABLED_FIELD, choices.trafficLayerEnabled)
        record.put(DEVICE_LOCATION_ENABLED_FIELD, choices.deviceLocationEnabled)
        preferences.edit()
            .putString(RECORD_PREFERENCE_KEY, record.toString())
            .apply()
    }

    /** Clears the remembered record; the credential store is separate. */
    fun clear() {
        preferences.edit()
            .remove(RECORD_PREFERENCE_KEY)
            .apply()
    }

    /** The scene keys a JSON array carries; malformed or non-text entries are dropped. */
    private fun JSONArray?.sceneKeysOrEmpty(): Set<String> {
        if (this == null) {
            return emptySet()
        }
        return buildSet {
            for (index in 0 until length()) {
                optString(index)
                    .takeIf { sceneKey -> sceneKey.isNotBlank() }
                    ?.let { sceneKey -> add(sceneKey) }
            }
        }
    }

    companion object {
        /**
         * Encodes the per-capability provider choices as the record's
         * `capabilityProviders` object, in the map's key order. The field is
         * always written, so a capability the user chose and then reset
         * leaves no stale key behind.
         */
        internal fun encodeCapabilityProviders(
            capabilityProviders: Map<String, String>,
        ): JSONObject {
            val encodedProviders = JSONObject()
            capabilityProviders.toSortedMap().forEach { (capabilityName, providerKey) ->
                encodedProviders.put(capabilityName, providerKey)
            }
            return encodedProviders
        }

        /**
         * Decodes the record's optional `capabilityProviders` object. A record
         * written before the field existed carries none and yields an empty
         * map; a malformed entry is dropped rather than guessed at.
         */
        internal fun decodeCapabilityProviders(record: JSONObject): Map<String, String> {
            val encodedProviders = record.optJSONObject(CAPABILITY_PROVIDERS_FIELD)
                ?: return emptyMap()
            return buildMap {
                encodedProviders.keys().forEach { capabilityName ->
                    encodedProviders.optString(capabilityName)
                        .takeIf { providerKey -> providerKey.isNotBlank() }
                        ?.let { providerKey -> put(capabilityName, providerKey) }
                }
            }
        }

        /** The private preferences file the record lives in. */
        const val PREFERENCES_NAME: String = "rusticMapsSdk.rememberedChoices"

        /** The key inside that file. */
        const val RECORD_PREFERENCE_KEY: String = "rememberedChoices"

        /** The record schema version (a record of another version is dropped). */
        private const val RECORD_VERSION: Int = 1

        private const val VERSION_FIELD = "version"
        private const val THEME_FIELD = "theme"
        private const val BASE_MAP_FIELD = "baseMap"
        private const val PROVIDER_KEY_FIELD = "providerKey"
        private const val STYLE_ID_FIELD = "styleId"
        private const val DEMO_PLAN_FIELD = "demoPlan"
        private const val SCENARIO_NAME_FIELD = "scenarioName"
        private const val SCENES_FIELD = "scenes"
        private const val CAMERA_PRESET_FIELD = "cameraPreset"
        private const val IDENTIFIER_FIELD = "id"
        private const val SPEED_MULTIPLIER_FIELD = "simulatedTripSpeedMultiplier"
        private const val SEARCH_FIELD = "search"
        private const val ORIGIN_FIELD = "origin"
        private const val DESTINATION_FIELD = "destination"
        private const val QUERY_FIELD = "query"
        internal const val CAPABILITY_PROVIDERS_FIELD = "capabilityProviders"

        /** The record's traffic-layer field; absent in older records. */
        internal const val TRAFFIC_LAYER_ENABLED_FIELD: String = "trafficLayerEnabled"

        /** The record's device-location field; absent in older records. */
        internal const val DEVICE_LOCATION_ENABLED_FIELD: String = "deviceLocationEnabled"

        /** The record's Directions fields; absent in older records. */
        internal const val DIRECTIONS_TRANSPORTATION_MODE_FIELD: String =
            "directionsTransportationMode"
        internal const val DIRECTIONS_ROUTE_STRATEGY_FIELD: String =
            "directionsRouteStrategy"

        /**
         * Encodes the Directions choices into the record's own top-level
         * fields. Both are always written, so a reset choice leaves no stale
         * value behind and an older record that lacks them still loads with
         * their defaults.
         */
        internal fun encodeDirections(
            record: JSONObject,
            transportationMode: DirectionsTransportationMode,
            routeStrategy: DirectionsRouteStrategy,
        ) {
            record
                .put(DIRECTIONS_TRANSPORTATION_MODE_FIELD, transportationMode.storageValue)
                .put(DIRECTIONS_ROUTE_STRATEGY_FIELD, routeStrategy.storageValue)
        }

        /**
         * Decodes the record's optional Directions fields. A record written
         * before the fields existed carries none, and a value this build does
         * not offer falls back to that choice's default rather than guessing.
         */
        internal fun decodeDirections(record: JSONObject): RememberedDirections =
            RememberedDirections(
                transportationMode = DirectionsTransportationMode.fromStorageValue(
                    record.optString(DIRECTIONS_TRANSPORTATION_MODE_FIELD),
                ) ?: DirectionsTransportationMode.CAR,
                routeStrategy = DirectionsRouteStrategy.fromStorageValue(
                    record.optString(DIRECTIONS_ROUTE_STRATEGY_FIELD),
                ) ?: DirectionsRouteStrategy.FASTEST,
            )
    }
}

/** The Directions choices a record carries, with their defaults. */
data class RememberedDirections(
    /** The remembered transport mode (Car when absent). */
    val transportationMode: DirectionsTransportationMode = DirectionsTransportationMode.CAR,
    /** The remembered route strategy (Fastest when absent). */
    val routeStrategy: DirectionsRouteStrategy = DirectionsRouteStrategy.FASTEST,
)
