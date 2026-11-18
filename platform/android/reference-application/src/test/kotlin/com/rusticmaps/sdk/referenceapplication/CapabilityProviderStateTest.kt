package com.rusticmaps.sdk.referenceapplication

import com.rusticmaps.sdk.ProviderCredentialDescriptor
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.rustic_maps.ProviderContract
import uniffi.rustic_maps.ProviderRegistration

/**
 * Unit tests of the per-capability settings state holder: the rows follow the
 * SDK's readback order, an unconfigured capability reads honestly, a selection
 * repeats the session's credential only for a credentialed provider, a
 * refusal leaves the previous provider serving, and the remembered choice
 * survives the record's JSON codec. Pure Kotlin with hand-written fakes.
 */
class CapabilityProviderStateTest {
    @Test
    fun `rows follow the order the sdk reports`() {
        val gateway = FakeCapabilityProviderGateway(
            registrations = listOf(
                registration(ProviderContract.ROUTING, providerKey = "", displayName = ""),
                registration(ProviderContract.GEOCODING, providerKey = "", displayName = ""),
                registration(ProviderContract.RASTER_TILES, providerKey = "", displayName = ""),
            ),
        )
        val capabilityProviderState = CapabilityProviderState(
            gateway = gateway,
            descriptors = emptyList(),
        )
        assertEquals(
            listOf(
                ProviderContract.ROUTING,
                ProviderContract.GEOCODING,
                ProviderContract.RASTER_TILES,
            ),
            capabilityProviderState.rows.map { row -> row.contract },
        )
        assertEquals(
            listOf("Routing", "Geocoding", "Raster Tiles"),
            capabilityProviderState.rows.map { row -> row.capabilityDisplayName },
        )
    }

    @Test
    fun `an unregistered capability reads Not configured`() {
        val gateway = FakeCapabilityProviderGateway(
            registrations = listOf(
                registration(ProviderContract.GEOCODING, providerKey = "", displayName = ""),
            ),
        )
        val capabilityProviderState = CapabilityProviderState(
            gateway = gateway,
            descriptors = emptyList(),
        )
        val geocodingRow = capabilityProviderState.rows.single()
        assertEquals(
            ReferenceApplicationConstants.CAPABILITY_PROVIDER_NOT_CONFIGURED_TEXT,
            geocodingRow.servingStatusText,
        )
    }

    @Test
    fun `a selection repeats the held credential only for a credentialed provider`() {
        val gateway = FakeCapabilityProviderGateway(
            registrations = listOf(
                registration(ProviderContract.ROUTING, providerKey = "", displayName = ""),
                registration(ProviderContract.GEOCODING, providerKey = "", displayName = ""),
            ),
            heldCredentials = mapOf(CREDENTIALED_PROVIDER_KEY to HELD_CREDENTIAL),
        )
        val capabilityProviderState = CapabilityProviderState(
            gateway = gateway,
            descriptors = listOf(
                descriptor(
                    providerKey = CREDENTIALED_PROVIDER_KEY,
                    credentialLabel = CREDENTIAL_LABEL,
                    servedContracts = listOf("geocoding"),
                ),
                descriptor(
                    providerKey = KEYLESS_ROUTING_PROVIDER_KEY,
                    credentialLabel = "",
                    servedContracts = listOf("routing"),
                    demoBaseUrl = DEMO_BASE_URL,
                ),
            ),
        )
        capabilityProviderState.selectProvider(
            ProviderContract.GEOCODING,
            CREDENTIALED_PROVIDER_KEY,
        )
        capabilityProviderState.selectProvider(
            ProviderContract.ROUTING,
            KEYLESS_ROUTING_PROVIDER_KEY,
        )
        assertEquals(
            listOf(
                ConfigureCall(ProviderContract.GEOCODING, CREDENTIALED_PROVIDER_KEY, HELD_CREDENTIAL),
                ConfigureCall(ProviderContract.ROUTING, KEYLESS_ROUTING_PROVIDER_KEY, DEMO_BASE_URL),
            ),
            gateway.configureCalls,
        )
        assertEquals(
            "Sample Keyless Provider · Ready",
            capabilityProviderState.rows.first { row -> row.contract == ProviderContract.ROUTING }
                .servingStatusText,
        )
    }

    @Test
    fun `a keyless provider without a demo address refuses honestly without calling the sdk`() {
        val gateway = FakeCapabilityProviderGateway(
            registrations = listOf(
                registration(ProviderContract.ROUTING, providerKey = "", displayName = ""),
            ),
        )
        val capabilityProviderState = CapabilityProviderState(
            gateway = gateway,
            descriptors = listOf(
                descriptor(
                    providerKey = KEYLESS_ROUTING_PROVIDER_KEY,
                    credentialLabel = "",
                    servedContracts = listOf("routing"),
                ),
            ),
        )
        assertEquals(
            "Sample Keyless Provider · Ready",
            capabilityProviderState.rows.single().choices.single().pickerLabel,
        )
        capabilityProviderState.selectProvider(
            ProviderContract.ROUTING,
            KEYLESS_ROUTING_PROVIDER_KEY,
        )
        assertTrue(gateway.configureCalls.isEmpty())
        assertEquals(
            ReferenceApplicationConstants.SETTINGS_PROVIDER_NO_DEMO_ENDPOINT_TEMPLATE
                .format(KEYLESS_ROUTING_PROVIDER_NAME),
            capabilityProviderState.rows.single().detailText,
        )
    }

    @Test
    fun `the serving line composes the provider name with its credential state`() {
        val gateway = FakeCapabilityProviderGateway(
            registrations = listOf(
                registration(
                    ProviderContract.ROUTING,
                    providerKey = CREDENTIALED_PROVIDER_KEY,
                    displayName = "Sample Credentialed Provider",
                ),
            ),
        )
        val capabilityProviderState = CapabilityProviderState(
            gateway = gateway,
            descriptors = listOf(
                descriptor(
                    providerKey = CREDENTIALED_PROVIDER_KEY,
                    credentialLabel = CREDENTIAL_LABEL,
                    servedContracts = listOf("routing"),
                ),
            ),
        )
        assertEquals(
            "Sample Credentialed Provider · Needs $CREDENTIAL_LABEL",
            capabilityProviderState.rows.single().servingStatusText,
        )
    }

    @Test
    fun `a keyless serving row states its demo endpoint and note`() {
        val gateway = FakeCapabilityProviderGateway(
            registrations = listOf(
                registration(
                    ProviderContract.ROUTING,
                    providerKey = KEYLESS_ROUTING_PROVIDER_KEY,
                    displayName = KEYLESS_ROUTING_PROVIDER_NAME,
                ),
            ),
        )
        val capabilityProviderState = CapabilityProviderState(
            gateway = gateway,
            descriptors = listOf(
                descriptor(
                    providerKey = KEYLESS_ROUTING_PROVIDER_KEY,
                    credentialLabel = "",
                    servedContracts = listOf("routing"),
                    demoBaseUrl = DEMO_BASE_URL,
                    demoBaseUrlNote = DEMO_BASE_URL_NOTE,
                ),
            ),
        )
        val routingRow = capabilityProviderState.rows.single()
        assertEquals(
            ReferenceApplicationConstants.SETTINGS_ROUTING_DEMO_ENDPOINT_TEMPLATE
                .format(DEMO_BASE_URL),
            routingRow.demoEndpointText,
        )
        assertEquals(DEMO_BASE_URL_NOTE, routingRow.demoEndpointNote)
    }

    @Test
    fun `a provider whose credential is not applied reports the reason without calling the sdk`() {
        val gateway = FakeCapabilityProviderGateway(
            registrations = listOf(
                registration(ProviderContract.GEOCODING, providerKey = "", displayName = ""),
            ),
        )
        val capabilityProviderState = CapabilityProviderState(
            gateway = gateway,
            descriptors = listOf(
                descriptor(
                    providerKey = CREDENTIALED_PROVIDER_KEY,
                    credentialLabel = CREDENTIAL_LABEL,
                    servedContracts = listOf("geocoding"),
                ),
            ),
        )
        val geocodingRow = capabilityProviderState.rows.single()
        assertEquals(
            "Sample Credentialed Provider · Needs $CREDENTIAL_LABEL",
            geocodingRow.choices.single().pickerLabel,
        )
        capabilityProviderState.selectProvider(
            ProviderContract.GEOCODING,
            CREDENTIALED_PROVIDER_KEY,
        )
        assertTrue(gateway.configureCalls.isEmpty())
        assertEquals(
            ReferenceApplicationConstants.CAPABILITY_PROVIDER_NEEDS_CREDENTIAL_TEMPLATE
                .format(CREDENTIAL_LABEL),
            capabilityProviderState.rows.single().detailText,
        )
    }

    @Test
    fun `a refusal leaves the previous provider serving and shows the message verbatim`() {
        val gateway = FakeCapabilityProviderGateway(
            registrations = listOf(
                registration(ProviderContract.ROUTING, providerKey = "", displayName = ""),
            ),
        )
        val capabilityProviderState = CapabilityProviderState(
            gateway = gateway,
            descriptors = listOf(
                descriptor(
                    providerKey = KEYLESS_ROUTING_PROVIDER_KEY,
                    credentialLabel = "",
                    servedContracts = listOf("routing"),
                    demoBaseUrl = DEMO_BASE_URL,
                ),
                descriptor(
                    providerKey = ALTERNATE_ROUTING_PROVIDER_KEY,
                    credentialLabel = "",
                    servedContracts = listOf("routing"),
                    demoBaseUrl = DEMO_BASE_URL,
                ),
            ),
        )
        capabilityProviderState.selectProvider(
            ProviderContract.ROUTING,
            KEYLESS_ROUTING_PROVIDER_KEY,
        )
        assertEquals(
            "$KEYLESS_ROUTING_PROVIDER_NAME · Ready",
            capabilityProviderState.rows.single().servingStatusText,
        )
        gateway.refusalMessage = REFUSAL_MESSAGE
        capabilityProviderState.selectProvider(
            ProviderContract.ROUTING,
            ALTERNATE_ROUTING_PROVIDER_KEY,
        )
        val routingRow = capabilityProviderState.rows.single()
        assertEquals("$KEYLESS_ROUTING_PROVIDER_NAME · Ready", routingRow.servingStatusText)
        assertEquals(REFUSAL_MESSAGE, routingRow.detailText)
    }

    @Test
    fun `read-only tile rows carry no picker and state where their source changes`() {
        val gateway = FakeCapabilityProviderGateway(
            registrations = listOf(
                registration(
                    ProviderContract.RASTER_TILES,
                    providerKey = KEYLESS_ROUTING_PROVIDER_KEY,
                    displayName = KEYLESS_ROUTING_PROVIDER_NAME,
                ),
                registration(ProviderContract.VECTOR_TILES, providerKey = "", displayName = ""),
            ),
        )
        val capabilityProviderState = CapabilityProviderState(
            gateway = gateway,
            descriptors = listOf(
                descriptor(
                    providerKey = ALTERNATE_ROUTING_PROVIDER_KEY,
                    credentialLabel = "",
                    servedContracts = listOf("routing"),
                ),
            ),
        )
        capabilityProviderState.rows.forEach { row ->
            assertTrue(row.isReadOnly)
            assertTrue(row.choices.isEmpty())
            assertEquals(
                ReferenceApplicationConstants.CAPABILITY_PROVIDER_READ_ONLY_NOTE_TEXT,
                row.readOnlyNoteText,
            )
        }
    }

    @Test
    fun `the picker offers only providers that serve the capability on this platform`() {
        val gateway = FakeCapabilityProviderGateway(
            registrations = listOf(
                registration(ProviderContract.ROUTING, providerKey = "", displayName = ""),
            ),
        )
        val unavailableRoutingDescriptor = descriptor(
            providerKey = ALTERNATE_ROUTING_PROVIDER_KEY,
            credentialLabel = "",
            servedContracts = listOf("routing"),
        ).copy(isRegistrationAvailableOnAndroid = false)
        val capabilityProviderState = CapabilityProviderState(
            gateway = gateway,
            descriptors = listOf(
                descriptor(
                    providerKey = KEYLESS_ROUTING_PROVIDER_KEY,
                    credentialLabel = "",
                    servedContracts = listOf("routing"),
                ),
                unavailableRoutingDescriptor,
                descriptor(
                    providerKey = CREDENTIALED_PROVIDER_KEY,
                    credentialLabel = CREDENTIAL_LABEL,
                    servedContracts = listOf("geocoding"),
                ),
            ),
        )
        assertEquals(
            listOf(KEYLESS_ROUTING_PROVIDER_KEY),
            capabilityProviderState.rows.single().choices.map { choice -> choice.providerKey },
        )
    }

    @Test
    fun `a remembered selection round-trips through the record codec`() {
        val gateway = FakeCapabilityProviderGateway(
            registrations = listOf(
                registration(ProviderContract.ROUTING, providerKey = "", displayName = ""),
            ),
        )
        var rememberedProviderKeys: Map<String, String> = emptyMap()
        val capabilityProviderState = CapabilityProviderState(
            gateway = gateway,
            descriptors = listOf(
                descriptor(
                    providerKey = KEYLESS_ROUTING_PROVIDER_KEY,
                    credentialLabel = "",
                    servedContracts = listOf("routing"),
                    demoBaseUrl = DEMO_BASE_URL,
                ),
            ),
            onRememberedProviderKeysChanged = { keys -> rememberedProviderKeys = keys },
        )
        capabilityProviderState.selectProvider(
            ProviderContract.ROUTING,
            KEYLESS_ROUTING_PROVIDER_KEY,
        )
        assertEquals(mapOf("Routing" to KEYLESS_ROUTING_PROVIDER_KEY), rememberedProviderKeys)

        val record = JSONObject().put(
            RememberedChoicesStore.CAPABILITY_PROVIDERS_FIELD,
            RememberedChoicesStore.encodeCapabilityProviders(rememberedProviderKeys),
        )
        assertEquals(
            rememberedProviderKeys,
            RememberedChoicesStore.decodeCapabilityProviders(record),
        )
        // A record written before the field existed carries no choices.
        assertEquals(
            emptyMap<String, String>(),
            RememberedChoicesStore.decodeCapabilityProviders(JSONObject()),
        )
    }

    @Test
    fun `a remembered selection is re-applied at startup`() {
        val gateway = FakeCapabilityProviderGateway(
            registrations = listOf(
                registration(ProviderContract.ROUTING, providerKey = "", displayName = ""),
            ),
        )
        val capabilityProviderState = CapabilityProviderState(
            gateway = gateway,
            descriptors = listOf(
                descriptor(
                    providerKey = KEYLESS_ROUTING_PROVIDER_KEY,
                    credentialLabel = "",
                    servedContracts = listOf("routing"),
                    demoBaseUrl = DEMO_BASE_URL,
                ),
            ),
            initiallyRememberedProviderKeys = mapOf(
                "Routing" to KEYLESS_ROUTING_PROVIDER_KEY,
            ),
        )
        capabilityProviderState.applyRememberedProviderKeys()
        assertEquals(
            listOf(ConfigureCall(ProviderContract.ROUTING, KEYLESS_ROUTING_PROVIDER_KEY, DEMO_BASE_URL)),
            gateway.configureCalls,
        )
        assertEquals(
            "$KEYLESS_ROUTING_PROVIDER_NAME · Ready",
            capabilityProviderState.rows.single().servingStatusText,
        )
    }

    /** A hand-written gateway whose registration readback follows successful calls. */
    private class FakeCapabilityProviderGateway(
        registrations: List<ProviderRegistration>,
        private val heldCredentials: Map<String, String> = emptyMap(),
    ) : CapabilityProviderGateway {
        private val currentRegistrations: MutableList<ProviderRegistration> =
            registrations.toMutableList()

        /** Every configure call, in order, for assertions. */
        val configureCalls: MutableList<ConfigureCall> = mutableListOf()

        /** The message the next configure call refuses with, or null to accept. */
        var refusalMessage: String? = null

        override fun providerRegistrations(): List<ProviderRegistration> =
            currentRegistrations.toList()

        override fun configureProvider(
            contract: ProviderContract,
            providerKey: String,
            credential: String,
        ) {
            configureCalls.add(ConfigureCall(contract, providerKey, credential))
            refusalMessage?.let { message -> throw IllegalStateException(message) }
            val descriptor = DESCRIPTORS_BY_KEY.getValue(providerKey)
            val registrationIndex = currentRegistrations.indexOfFirst { registration ->
                registration.contract == contract
            }
            if (registrationIndex >= 0) {
                currentRegistrations[registrationIndex] = registration(
                    contract = contract,
                    providerKey = providerKey,
                    displayName = descriptor,
                )
            }
        }

        override fun appliedProviderCredential(providerKey: String): String? =
            heldCredentials[providerKey]
    }

    private data class ConfigureCall(
        val contract: ProviderContract,
        val providerKey: String,
        val credential: String,
    )

    private companion object {
        const val CREDENTIALED_PROVIDER_KEY = "sample-credentialed-provider"
        const val KEYLESS_ROUTING_PROVIDER_KEY = "sample-keyless-provider"
        const val ALTERNATE_ROUTING_PROVIDER_KEY = "alternate-keyless-provider"
        const val KEYLESS_ROUTING_PROVIDER_NAME = "Sample Keyless Provider"
        const val CREDENTIAL_LABEL = "API key"
        const val HELD_CREDENTIAL = "held-credential-value"
        const val REFUSAL_MESSAGE = "the provider refused this selection"
        const val DEMO_BASE_URL = "https://demo.example.test"
        const val DEMO_BASE_URL_NOTE = "Public demo server, rate-limited and not for production use."

        /** The display names the fake reports back after a successful selection. */
        val DESCRIPTORS_BY_KEY: Map<String, String> = mapOf(
            CREDENTIALED_PROVIDER_KEY to "Sample Credentialed Provider",
            KEYLESS_ROUTING_PROVIDER_KEY to KEYLESS_ROUTING_PROVIDER_NAME,
            ALTERNATE_ROUTING_PROVIDER_KEY to "Alternate Keyless Provider",
        )

        fun registration(
            contract: ProviderContract,
            providerKey: String,
            displayName: String,
        ): ProviderRegistration = ProviderRegistration(
            contract = contract,
            providerKey = providerKey,
            displayName = displayName,
            registered = providerKey.isNotBlank(),
        )

        fun descriptor(
            providerKey: String,
            credentialLabel: String,
            servedContracts: List<String>,
            demoBaseUrl: String = "",
            demoBaseUrlNote: String = "",
        ): ProviderCredentialDescriptor = ProviderCredentialDescriptor(
            providerKey = providerKey,
            displayName = DESCRIPTORS_BY_KEY.getValue(providerKey),
            credentialLabel = credentialLabel,
            isRegistrationAvailableOnAndroid = true,
            attributionText = "",
            termsNote = "",
            settingsHint = "",
            demoBaseUrl = demoBaseUrl,
            demoBaseUrlNote = demoBaseUrlNote,
            rasterUrlTemplate = "",
            maximumZoomLevel = 0,
            vectorUrlTemplate = "",
            vectorStyleSchema = "",
            vectorStyles = emptyList(),
            servedContracts = servedContracts,
        )
    }
}
