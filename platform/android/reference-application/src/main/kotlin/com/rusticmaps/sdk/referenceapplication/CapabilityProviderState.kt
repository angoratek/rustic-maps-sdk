package com.rusticmaps.sdk.referenceapplication

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.rusticmaps.sdk.MapsSdkFacade
import com.rusticmaps.sdk.ProviderCredentialDescriptor
import kotlinx.coroutines.CancellationException
import uniffi.rustic_maps.ProviderContract
import uniffi.rustic_maps.ProviderRegistration

/**
 * The SDK operations the per-capability settings section needs: the readback
 * of which provider serves each capability, the selection of one provider for
 * one capability, and the credential the session already holds for a provider.
 * The interface keeps the state holder testable with a hand-written fake while
 * the shipped implementation delegates to the SDK facade.
 */
interface CapabilityProviderGateway {
    /** Reports which provider serves each capability, in the SDK's stable order. */
    fun providerRegistrations(): List<ProviderRegistration>

    /**
     * Registers `providerKey` for `contract`, repeating the value the session
     * holds for a credentialed provider and passing an empty credential for a
     * keyless one. A refusal surfaces the SDK's exception verbatim.
     */
    fun configureProvider(
        contract: ProviderContract,
        providerKey: String,
        credential: String,
    )

    /** The credential the session registered the provider with, or null when it holds none. */
    fun appliedProviderCredential(providerKey: String): String?
}

/** The gateway backed by the shipped SDK facade. */
class SdkCapabilityProviderGateway(
    private val facade: MapsSdkFacade,
) : CapabilityProviderGateway {
    override fun providerRegistrations(): List<ProviderRegistration> =
        facade.providerRegistrations()

    override fun configureProvider(
        contract: ProviderContract,
        providerKey: String,
        credential: String,
    ) {
        facade.configureProvider(contract, providerKey, credential)
    }

    override fun appliedProviderCredential(providerKey: String): String? =
        facade.appliedProviderCredential(providerKey)
}

/**
 * One provider the picker offers for a capability. [requiredCredentialLabel]
 * is non-null while the provider declares a credential the session has not
 * applied yet; the picker then names what it needs instead of pretending the
 * choice is available.
 */
data class CapabilityProviderChoice(
    /** The provider's stable registry key. */
    val providerKey: String,
    /** Consumer-facing provider name. */
    val displayName: String,
    /** The credential label the provider still needs, or null when it is selectable. */
    val requiredCredentialLabel: String?,
) {
    /** Whether selecting this provider must first apply a credential. */
    val requiresCredential: Boolean
        get() = requiredCredentialLabel != null

    /**
     * The provider's credential state in the artifact's words
     * (`settings.provider_state_*`): `Ready` or `Needs {credential}`.
     */
    val stateText: String
        get() = requiredCredentialLabel
            ?.let { credentialLabel ->
                ReferenceApplicationConstants.SETTINGS_PROVIDER_STATE_NEEDS_CREDENTIAL_TEMPLATE
                    .format(credentialLabel)
            }
            ?: ReferenceApplicationConstants.SETTINGS_PROVIDER_STATE_READY_TEXT

    /**
     * The picker entry's text: the registered `{display_name} · {state}`
     * composition (`settings.provider_row`), never a bare state.
     */
    val pickerLabel: String
        get() = ReferenceApplicationConstants.SETTINGS_PROVIDER_ROW_TEMPLATE
            .format(displayName, stateText)
}

/**
 * One settings row: a capability, the provider the SDK reports serving it,
 * and the alternatives the shared registry offers for it. Raster and vector
 * tiles are read-only because the map's layer control owns their source.
 */
data class CapabilityProviderRow(
    /** The capability this row configures. */
    val contract: ProviderContract,
    /** The capability in human words, derived from the contract enum. */
    val capabilityDisplayName: String,
    /** The serving provider's registry key, or null when none is installed. */
    val servingProviderKey: String?,
    /**
     * The serving provider's display name (the shared registry's name for the
     * key when it declares one), or null when none is installed.
     */
    val servingProviderDisplayName: String?,
    /** Whether the SDK reports a provider installed for this capability. */
    val isRegistered: Boolean,
    /** Whether the row is read-only because another surface owns the source. */
    val isReadOnly: Boolean,
    /** The one-line note a read-only row carries, or null when it has a picker. */
    val readOnlyNoteText: String?,
    /** The providers this registry offers for the capability, in registry order. */
    val choices: List<CapabilityProviderChoice>,
    /** A refusal or required-credential reason shown verbatim in the row. */
    val detailText: String?,
    /**
     * The serving keyless provider's public demo address, rendered as the
     * artifact's `settings.routing_demo_endpoint` line, or null when the
     * provider is credentialed or declares no demo address.
     */
    val demoEndpointText: String? = null,
    /** The descriptor's own note about the demo address, or null. */
    val demoEndpointNote: String? = null,
    /** The serving descriptor's terms note, when it declares one. */
    val providerTermsNote: String? = null,
    /** The serving provider's credential state in the artifact's words. */
    val servingStateText: String =
        ReferenceApplicationConstants.SETTINGS_PROVIDER_STATE_READY_TEXT,
) {
    /**
     * The row's serving line: the registered `{display_name} · {state}`
     * composition (`settings.provider_row`), or the honest unconfigured
     * wording when no provider is registered for the capability.
     */
    val servingStatusText: String
        get() {
            if (!isRegistered) {
                return ReferenceApplicationConstants.SETTINGS_PROVIDER_STATE_NOT_CONFIGURED_TEXT
            }
            val displayName = servingProviderDisplayName
                ?.takeIf { name -> name.isNotBlank() }
                ?: servingProviderKey?.takeIf { key -> key.isNotBlank() }
                ?: return ReferenceApplicationConstants.SETTINGS_PROVIDER_STATE_NOT_CONFIGURED_TEXT
            return ReferenceApplicationConstants.SETTINGS_PROVIDER_ROW_TEMPLATE
                .format(displayName, servingStateText)
        }
}

/**
 * The settings section's state holder: one row per SDK capability readback,
 * a picker of the shared registry's eligible providers, and the remembered
 * per-capability choices. Nothing here names a provider — the rows come from
 * the SDK and the alternatives from the shared registry — so a further
 * provider or capability needs no change in this class.
 *
 * A selection registers the provider through the SDK's
 * `configureProvider` (repeating the credential the session already holds),
 * refreshes the rows from the SDK's own readback, and remembers the choice.
 * A refused selection leaves the previous provider serving and shows the
 * thrown message verbatim; a provider whose credential is not applied yet
 * reports that reason without calling the SDK.
 */
class CapabilityProviderState(
    private val gateway: CapabilityProviderGateway,
    private val descriptors: List<ProviderCredentialDescriptor>,
    initiallyRememberedProviderKeys: Map<String, String> = emptyMap(),
    private val onRememberedProviderKeysChanged: (Map<String, String>) -> Unit = {},
) {
    /** The capability storage names ("Routing") mapped to the chosen provider keys. */
    private val rememberedProviderKeys: MutableMap<String, String> =
        initiallyRememberedProviderKeys.toMutableMap()

    /** Per-capability refusal or required-credential text, cleared on success. */
    private val rowDetailTexts: MutableMap<ProviderContract, String> = mutableMapOf()

    /** One row per SDK readback entry, in the order the SDK returns them. */
    var rows: List<CapabilityProviderRow> by mutableStateOf(emptyList())
        private set

    init {
        refresh()
    }

    /** Re-reads the SDK's registrations and rebuilds every row. */
    fun refresh() {
        rows = gateway.providerRegistrations().map { registration ->
            buildRow(registration)
        }
    }

    /**
     * The picker's act: registers the chosen provider for the capability.
     * Raster and vector tiles are read-only, and a provider the registry does
     * not offer for the capability is ignored.
     */
    fun selectProvider(contract: ProviderContract, providerKey: String) {
        val row = rows.firstOrNull { row -> row.contract == contract } ?: return
        if (row.isReadOnly) {
            return
        }
        val choice = row.choices.firstOrNull { choice -> choice.providerKey == providerKey } ?: return
        val requiredCredentialLabel = choice.requiredCredentialLabel
        if (requiredCredentialLabel != null) {
            rowDetailTexts[contract] =
                ReferenceApplicationConstants.CAPABILITY_PROVIDER_NEEDS_CREDENTIAL_TEMPLATE
                    .format(requiredCredentialLabel)
            refresh()
            return
        }
        val descriptor = descriptors.firstOrNull { descriptor ->
            descriptor.providerKey == providerKey
        } ?: return
        // The contract reads a keyless provider's credential argument as its
        // deployment address. A keyless provider therefore passes the shared
        // registry's public demo address, never an empty value (which the
        // provider crate would silently read as a local default). Without a
        // declared address the selection refuses honestly and the previous
        // provider keeps serving.
        val credential = if (descriptor.credentialLabel.isNotBlank()) {
            gateway.appliedProviderCredential(providerKey).orEmpty()
        } else {
            descriptor.demoBaseUrl.takeIf { demoBaseUrl -> demoBaseUrl.isNotBlank() }
                ?: run {
                    rowDetailTexts[contract] =
                        ReferenceApplicationConstants.SETTINGS_PROVIDER_NO_DEMO_ENDPOINT_TEMPLATE
                            .format(descriptor.displayName)
                    refresh()
                    return
                }
        }
        try {
            gateway.configureProvider(contract, providerKey, credential)
        } catch (selectionException: Exception) {
            if (selectionException is CancellationException) {
                throw selectionException
            }
            rowDetailTexts[contract] = selectionException.message ?: selectionException.toString()
            refresh()
            return
        }
        rowDetailTexts.remove(contract)
        rememberedProviderKeys[recordStorageNameOf(contract)] = providerKey
        onRememberedProviderKeysChanged(rememberedProviderKeys.toMap())
        refresh()
    }

    /**
     * Re-applies the remembered per-capability choices at startup. It runs
     * after the credential application, so a credentialed provider finds the
     * value the session already holds. A choice the SDK refuses reports in
     * its row and keeps the previously serving provider.
     */
    fun applyRememberedProviderKeys() {
        if (rememberedProviderKeys.isEmpty()) {
            return
        }
        rows.forEach { row ->
            val providerKey = rememberedProviderKeys[recordStorageNameOf(row.contract)] ?: return@forEach
            if (!row.isReadOnly) {
                selectProvider(row.contract, providerKey)
            }
        }
    }

    /** The remembered choices, for the record the application persists. */
    fun rememberedProviderKeys(): Map<String, String> = rememberedProviderKeys.toMap()

    /**
     * Whether the SDK reports a provider registered for routing. A keyless
     * selection (the public demo deployment) counts exactly like a
     * credentialed one, so the plan flow asks the registered provider instead
     * of falling back to a recorded route.
     */
    val hasRegisteredRoutingProvider: Boolean
        get() = rows.any { row -> row.contract == ProviderContract.ROUTING && row.isRegistered }

    /**
     * Records that a route plan's provider was unreachable: the routing row
     * adds the artifact's retry-or-own-deployment next step while the error
     * strip carries the provider's typed failure verbatim. The previous
     * provider keeps serving; nothing falls back.
     */
    fun noteRoutingUnreachable() {
        rowDetailTexts[ProviderContract.ROUTING] =
            ReferenceApplicationConstants.SETTINGS_ROUTING_UNREACHABLE_NEXT_STEP_TEXT
        refresh()
    }

    /** Clears the unreachable next step after a plan succeeds. */
    fun clearRoutingFailure() {
        if (rowDetailTexts.remove(ProviderContract.ROUTING) != null) {
            refresh()
        }
    }

    /**
     * Forgets every per-capability choice without touching the running
     * session: the SDK has no unregister operation, so the providers it
     * already installed keep serving until the process restarts.
     */
    fun clearRememberedProviderKeys() {
        rememberedProviderKeys.clear()
    }

    /** Builds one row from the SDK's registration and the shared registry. */
    private fun buildRow(registration: ProviderRegistration): CapabilityProviderRow {
        val isReadOnly = registration.contract in READ_ONLY_CONTRACTS
        val descriptor = descriptors.firstOrNull { candidate ->
            candidate.providerKey == registration.providerKey
        }
        val servingStateText = servingStateTextOf(descriptor, registration.providerKey)
        val demoBaseUrl = if (descriptor != null && descriptor.credentialLabel.isBlank()) {
            descriptor.demoBaseUrl.takeIf { candidateUrl -> candidateUrl.isNotBlank() }
        } else {
            null
        }
        return CapabilityProviderRow(
            contract = registration.contract,
            capabilityDisplayName = capabilityDisplayNameOf(registration.contract),
            servingProviderKey = registration.providerKey
                .takeIf { providerKey -> providerKey.isNotBlank() },
            servingProviderDisplayName = descriptor
                ?.displayName
                ?.takeIf { displayName -> displayName.isNotBlank() }
                ?: registration.displayName
                    .takeIf { displayName -> displayName.isNotBlank() },
            isRegistered = registration.registered,
            isReadOnly = isReadOnly,
            readOnlyNoteText = if (isReadOnly) {
                ReferenceApplicationConstants.CAPABILITY_PROVIDER_READ_ONLY_NOTE_TEXT
            } else {
                null
            },
            choices = if (isReadOnly) emptyList() else offeredChoices(registration.contract),
            detailText = rowDetailTexts[registration.contract],
            demoEndpointText = demoBaseUrl?.let { url ->
                ReferenceApplicationConstants.SETTINGS_ROUTING_DEMO_ENDPOINT_TEMPLATE.format(url)
            },
            demoEndpointNote = descriptor?.demoBaseUrlNote?.takeIf { note -> note.isNotBlank() },
            providerTermsNote = descriptor?.termsNote?.takeIf { note -> note.isNotBlank() },
            servingStateText = servingStateText,
        )
    }

    /**
     * The serving provider's credential state in the artifact's words: a
     * keyless descriptor and a credentialed descriptor whose key this session
     * holds both read `Ready`; a declared-but-unapplied credential reads
     * `Needs {credential}`.
     */
    private fun servingStateTextOf(
        descriptor: ProviderCredentialDescriptor?,
        providerKey: String,
    ): String {
        if (descriptor == null || descriptor.credentialLabel.isBlank()) {
            return ReferenceApplicationConstants.SETTINGS_PROVIDER_STATE_READY_TEXT
        }
        val holdsCredential = !gateway.appliedProviderCredential(providerKey).isNullOrBlank()
        return if (holdsCredential) {
            ReferenceApplicationConstants.SETTINGS_PROVIDER_STATE_READY_TEXT
        } else {
            ReferenceApplicationConstants.SETTINGS_PROVIDER_STATE_NEEDS_CREDENTIAL_TEMPLATE
                .format(descriptor.credentialLabel)
        }
    }

    /** The registry's providers that serve the capability and register on Android. */
    private fun offeredChoices(contract: ProviderContract): List<CapabilityProviderChoice> {
        val capabilityStorageName = capabilityStorageNameOf(contract)
        return descriptors
            .filter { descriptor ->
                descriptor.isRegistrationAvailableOnAndroid &&
                    capabilityStorageName in descriptor.servedContracts
            }
            .map { descriptor ->
                CapabilityProviderChoice(
                    providerKey = descriptor.providerKey,
                    displayName = descriptor.displayName,
                    requiredCredentialLabel = descriptor.credentialLabel.takeIf { credentialLabel ->
                        credentialLabel.isNotBlank() &&
                            gateway.appliedProviderCredential(descriptor.providerKey).isNullOrBlank()
                    },
                )
            }
    }

    private companion object {
        /** The capabilities the map's layer control owns, so their rows are read-only. */
        val READ_ONLY_CONTRACTS: Set<ProviderContract> = setOf(
            ProviderContract.RASTER_TILES,
            ProviderContract.VECTOR_TILES,
        )

        /**
         * The capability's contract storage name (`routing`, `distance_matrix`),
         * matching the names the shared registry's `served_contracts` uses.
         */
        fun capabilityStorageNameOf(contract: ProviderContract): String =
            contract.name.lowercase()

        /**
         * The capability's record storage name (`Routing`), the spelling the
         * remembered record maps to a provider key.
         */
        fun recordStorageNameOf(contract: ProviderContract): String =
            capabilityStorageNameOf(contract)
                .split('_')
                .joinToString(separator = "") { word ->
                    word.replaceFirstChar { character -> character.uppercase() }
                }

        /** The capability in human words (`Routing`, `Distance Matrix`). */
        fun capabilityDisplayNameOf(contract: ProviderContract): String =
            capabilityStorageNameOf(contract)
                .split('_')
                .joinToString(separator = " ") { word ->
                    word.replaceFirstChar { character -> character.uppercase() }
                }
    }
}
