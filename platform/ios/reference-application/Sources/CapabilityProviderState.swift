//! The settings surface's per-capability provider section: one row per
//! capability the SDK reports through `MapSdk.providerRegistrations`, in the
//! SDK's own order, with the registry's descriptors as the only source of
//! the providers a capability may choose from. The section never hardcodes a
//! capability list or a provider name: the readback names the serving
//! provider, and the shared registry's `served_contracts` decides which
//! providers a row offers.
//!
//! Raster and vector tiles are read-only here: the base-map group and the
//! map's layer control own those slots, so the row states the serving
//! provider without offering a second, conflicting selection path.

import Combine
import Foundation
import MapNativeCoreUI
import RusticMapsSDK

/// The SDK operations the capability rows need. It exists so the state type
/// can be exercised with hand-written fakes (the generated `MapSdk` class
/// cannot be instantiated against recorded answers).
protocol CapabilityProviderConfiguring: AnyObject {
    /// Reports which provider serves each capability, in the SDK's stable
    /// order.
    func providerRegistrations() -> [ProviderRegistration]

    /// Chooses the provider that serves one capability. A refused call
    /// leaves the previously serving provider in place.
    func configureProvider(
        contract: ProviderContract,
        providerKey: String,
        credential: String
    ) throws
}

/// The shipped SDK satisfies the capability rows' contract directly.
extension MapSdk: CapabilityProviderConfiguring {}

// MARK: - Contract naming

extension ProviderContract {
    /// The name the shared registry's `served_contracts` list uses for this
    /// capability (`routing`, `distance_matrix`, …).
    var registryCapabilityName: String {
        switch self {
        case .geocoding:
            return "geocoding"
        case .routing:
            return "routing"
        case .distanceMatrix:
            return "distance_matrix"
        case .placeSearch:
            return "place_search"
        case .trafficIncidents:
            return "traffic_incidents"
        case .trafficFlow:
            return "traffic_flow"
        case .elevation:
            return "elevation"
        case .rasterTiles:
            return "raster_tiles"
        case .vectorTiles:
            return "vector_tiles"
        case .offlineRouting:
            return "offline_routing"
        }
    }

    /// The capability's stable name in the remembered record (`Routing`,
    /// `DistanceMatrix`, …). It follows the contract's own case name, so a
    /// record written by one build reads back in another.
    var rememberedChoiceName: String {
        switch self {
        case .geocoding:
            return "Geocoding"
        case .routing:
            return "Routing"
        case .distanceMatrix:
            return "DistanceMatrix"
        case .placeSearch:
            return "PlaceSearch"
        case .trafficIncidents:
            return "TrafficIncidents"
        case .trafficFlow:
            return "TrafficFlow"
        case .elevation:
            return "Elevation"
        case .rasterTiles:
            return "RasterTiles"
        case .vectorTiles:
            return "VectorTiles"
        case .offlineRouting:
            return "OfflineRouting"
        }
    }

    /// The capability's human wording in the settings row. It derives from
    /// the contract case, never from a provider, so the row reads the same
    /// whoever serves it.
    var settingsRowTitle: String {
        switch self {
        case .geocoding:
            return "Geocoding"
        case .routing:
            return "Routing"
        case .distanceMatrix:
            return "Distance matrix"
        case .placeSearch:
            return "Place search"
        case .trafficIncidents:
            return "Traffic incidents"
        case .trafficFlow:
            return "Traffic flow"
        case .elevation:
            return "Elevation"
        case .rasterTiles:
            return "Raster tiles"
        case .vectorTiles:
            return "Vector tiles"
        case .offlineRouting:
            return "Offline routing"
        }
    }

    /// Whether the map's own layer control chooses this capability's
    /// provider, rather than the settings section. The two tile contracts
    /// belong to the base map, so their rows state the serving provider and
    /// carry no picker.
    var isReadOnlyCapability: Bool {
        switch self {
        case .rasterTiles, .vectorTiles:
            return true
        default:
            return false
        }
    }
}

// MARK: - Row and option records

/// One capability row: the SDK readback plus the wording the settings
/// surface renders.
struct CapabilityProviderRow: Identifiable, Equatable {
    /// The capability this row describes.
    let contract: ProviderContract
    /// The SDK's provider key for the serving provider; empty when none is
    /// installed.
    let providerKey: String
    /// The registry display name the SDK reports for the serving provider;
    /// empty when none is installed.
    let displayName: String
    /// Whether a provider is installed for the capability.
    let isRegistered: Bool
    /// The serving descriptor's credential label, blank for a keyless
    /// provider.
    let credentialLabel: String
    /// Whether the application holds the serving descriptor's credential.
    let hasAppliedCredential: Bool

    /// One row per capability.
    var id: String { contract.rememberedChoiceName }

    /// The capability's human wording.
    var title: String { contract.settingsRowTitle }

    /// The serving provider's credential state, or the honest unconfigured
    /// line.
    var servingStateText: String {
        guard isRegistered else {
            return ReferenceApplicationConfiguration.capabilityNotConfiguredText
        }
        let requiresCredential = !credentialLabel.trimmingCharacters(
            in: .whitespacesAndNewlines
        ).isEmpty
        if !requiresCredential || hasAppliedCredential {
            return ReferenceApplicationConfiguration.providerStateReadyText
        }
        return ReferenceApplicationConfiguration.capabilityNeedsCredentialText(
            credentialLabel: credentialLabel
        )
    }

    /// The composed serving line: the provider's display name and its
    /// credential state, never one without the other.
    var servingText: String {
        guard isRegistered else {
            return servingStateText
        }
        return ReferenceApplicationConfiguration.providerRowText(
            provider: displayName,
            state: servingStateText
        )
    }

    /// Whether the map's layer control owns this capability's provider.
    var isReadOnly: Bool { contract.isReadOnlyCapability }
}

/// One provider a capability row may select: a registry descriptor that
/// serves the capability and registers on iOS, plus whether the application
/// already holds the credential it needs.
struct CapabilityProviderOption: Identifiable, Equatable {
    /// The registry's stable dispatch key.
    let providerKey: String
    /// The provider's display name.
    let displayName: String
    /// The descriptor's credential label; blank for a keyless provider.
    let credentialLabel: String
    /// Whether the application holds an applied credential for this key.
    let hasAppliedCredential: Bool
    /// The public demo deployment address a keyless provider hands the SDK,
    /// or blank when the descriptor declares none.
    let demoBaseURL: String
    /// The one-line note the settings row shows beside the demo address.
    let demoBaseURLNote: String
    /// The descriptor's terms note.
    let termsNote: String

    /// One option per provider key.
    var id: String { providerKey }

    /// Whether the provider declares a credential at all.
    var requiresCredential: Bool {
        !credentialLabel.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    /// Whether the descriptor declares a public demo deployment address.
    var hasDemoBaseURL: Bool {
        !demoBaseURL.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    /// The credential state the composed label carries.
    var stateText: String {
        if !requiresCredential {
            return hasDemoBaseURL
                ? ReferenceApplicationConfiguration.providerStateReadyText
                : ReferenceApplicationConfiguration.capabilityNotConfiguredText
        }
        if hasAppliedCredential {
            return ReferenceApplicationConfiguration.providerStateReadyText
        }
        return ReferenceApplicationConfiguration.capabilityNeedsCredentialText(
            credentialLabel: credentialLabel
        )
    }

    /// Whether the selection can call the SDK now: a credentialed provider
    /// only once its credential is applied; a keyless provider only when the
    /// descriptor declares the demo deployment address the contract needs.
    var isSelectable: Bool {
        requiresCredential ? hasAppliedCredential : hasDemoBaseURL
    }

    /// The picker's label: always the provider's name and its credential
    /// state, never one without the other.
    var pickerLabel: String {
        ReferenceApplicationConfiguration.providerRowText(
            provider: displayName,
            state: stateText
        )
    }
}

/// A row's message after an act: the credential still needed, or the SDK's
/// verbatim refusal. The severity decides the color, never the text.
struct CapabilityProviderRowMessage: Equatable {
    /// What the message says about the row.
    enum Severity: Equatable {
        /// The selection needs an applied credential first.
        case warning
        /// The SDK refused the selection; the previous provider stays.
        case refusal
    }

    /// The message's severity.
    let severity: Severity
    /// The message text (a reason or the SDK's own text, verbatim).
    let text: String
}

// MARK: - The observable state

/// The per-capability provider section's state: the SDK readback rows, the
/// registry options each row offers, the selection act, and the choices the
/// device remembers. The view renders this object; it holds no selection
/// logic of its own.
@MainActor
final class CapabilityProviderState: ObservableObject {
    /// The capability rows in the SDK's own order.
    @Published private(set) var rows: [CapabilityProviderRow] = []

    /// The row messages after an act, keyed by the contract's remembered
    /// choice name. Cleared for a contract when its next selection succeeds.
    @Published private(set) var rowMessages: [String: CapabilityProviderRowMessage] = [:]

    /// The provider chosen for each capability, keyed by the contract's
    /// remembered choice name. This is the record the application persists.
    @Published private(set) var rememberedProviderKeys: [String: String] = [:]

    /// Called after a successful provider selection reaches the SDK, so a
    /// surface that depends on a capability's serving implementation can
    /// re-read it (the Directions choices follow the routing provider).
    var onProviderSelectionChanged: ((ProviderContract) -> Void)?

    /// The device-local store the choices persist through.
    private let rememberedChoicesStore: RememberedChoicesStore

    /// The SDK the rows read and the selections configure.
    private weak var sdk: CapabilityProviderConfiguring?

    /// The shared registry's descriptors, in artifact order.
    private var descriptors: [ProviderDescriptor] = []

    /// The application's already-applied credential for a provider key.
    private var appliedCredential: (String) -> String? = { _ in nil }

    /// The formatter for a thrown selection error. It must produce the SDK's
    /// own text so a refusal reads verbatim.
    private var failureText: (Swift.Error) -> String =
        CapabilityProviderState.defaultFailureText

    /// Creates the state over one remembered-choices store (the standard
    /// domain by default).
    init(rememberedChoicesStore: RememberedChoicesStore = .standard) {
        self.rememberedChoicesStore = rememberedChoicesStore
    }

    /// The SDK's failure text: the `LocalizedError` description when the
    /// error carries one, the error's own rendering otherwise.
    static func defaultFailureText(_ error: Swift.Error) -> String {
        if let localizedError = error as? LocalizedError,
           let description = localizedError.errorDescription
        {
            return description
        }
        return String(describing: error)
    }

    /// Binds the state to the running SDK and the loaded registry, and reads
    /// the choices a previous run remembered.
    func attach(
        sdk: CapabilityProviderConfiguring?,
        descriptors: [ProviderDescriptor],
        appliedCredential: @escaping (String) -> String?,
        failureText: @escaping (Swift.Error) -> String
            = CapabilityProviderState.defaultFailureText
    ) {
        self.sdk = sdk
        self.descriptors = descriptors
        self.appliedCredential = appliedCredential
        self.failureText = failureText
        rememberedProviderKeys =
            rememberedChoicesStore.load()?.capabilityProviders ?? [:]
        refresh()
    }

    /// Reads the SDK's current registration readback for every capability,
    /// preserving the SDK's order and naming.
    func refresh() {
        guard let sdk else {
            rows = []
            return
        }
        rows = sdk.providerRegistrations().map { registration in
            let descriptor = descriptors.first(
                where: { $0.providerKey == registration.providerKey }
            )
            let applied = appliedCredential(registration.providerKey)?
                .trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
            return CapabilityProviderRow(
                contract: registration.contract,
                providerKey: registration.providerKey,
                displayName: registration.displayName,
                isRegistered: registration.registered,
                credentialLabel: descriptor?.credentialLabel ?? "",
                hasAppliedCredential: !applied.isEmpty
            )
        }
    }

    /// The descriptor serving one capability row, or nil while no provider is
    /// registered or the registry no longer names it.
    func servingDescriptor(for contract: ProviderContract) -> ProviderDescriptor? {
        guard let row = rows.first(where: { $0.contract == contract }),
              row.isRegistered
        else {
            return nil
        }
        return descriptors.first(
            where: { $0.providerKey == row.providerKey }
        )
    }

    /// The demo deployment address line under a routing row, or nil when the
    /// serving provider declares none.
    func servingDemoEndpointText(for contract: ProviderContract) -> String? {
        guard let descriptor = servingDescriptor(for: contract),
              descriptor.hasDemoBaseURL
        else {
            return nil
        }
        return ReferenceApplicationConfiguration.routingDemoEndpointText(
            url: descriptor.demoBaseURL
        )
    }

    /// The serving descriptor's terms note, or nil when it declares none.
    func servingTermsNote(for contract: ProviderContract) -> String? {
        guard let termsNote = servingDescriptor(for: contract)?.termsNote
            .trimmingCharacters(in: .whitespacesAndNewlines),
            !termsNote.isEmpty
        else {
            return nil
        }
        return termsNote
    }

    /// The providers a capability row offers: every registry descriptor that
    /// serves the capability and registers on iOS, in registry order.
    func options(for contract: ProviderContract) -> [CapabilityProviderOption] {
        guard !contract.isReadOnlyCapability else { return [] }
        let capabilityName = contract.registryCapabilityName
        return descriptors.compactMap { descriptor in
            guard descriptor.servedContracts.contains(capabilityName),
                  descriptor.registrationAvailable.ios
            else {
                return nil
            }
            let applied = appliedCredential(descriptor.providerKey)?
                .trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
            return CapabilityProviderOption(
                providerKey: descriptor.providerKey,
                displayName: descriptor.displayName,
                credentialLabel: descriptor.credentialLabel,
                hasAppliedCredential: !applied.isEmpty,
                demoBaseURL: descriptor.demoBaseURL,
                demoBaseURLNote: descriptor.demoBaseURLNote,
                termsNote: descriptor.termsNote
            )
        }
    }

    /// Whether the SDK reports a provider registered for routing. A keyless
    /// selection (the public demo deployment or an automation fixture server)
    /// counts exactly like a credentialed one, so the plan flow asks the
    /// registered provider instead of falling back to a recorded route.
    var hasRegisteredRoutingProvider: Bool {
        rows.contains { row in
            row.contract == .routing && row.isRegistered
        }
    }

    /// The message under one capability row, or nil while it has none.
    func rowMessage(
        for contract: ProviderContract
    ) -> CapabilityProviderRowMessage? {
        rowMessages[contract.rememberedChoiceName]
    }

    /// Applies every remembered per-capability choice, in the SDK's row
    /// order. It runs after the application's credentials are applied, so a
    /// remembered credentialed provider can find its credential. A restore
    /// never rewrites the record.
    func restoreRememberedChoices() {
        rememberedProviderKeys =
            rememberedChoicesStore.load()?.capabilityProviders ?? [:]
        for row in rows where !row.isReadOnly {
            guard let providerKey =
                rememberedProviderKeys[row.contract.rememberedChoiceName]
            else {
                continue
            }
            applySelection(
                contract: row.contract,
                providerKey: providerKey,
                recordsChoice: false
            )
        }
    }

    /// Selects a provider for a capability: the credential the application
    /// already holds is handed to the SDK, the accepted choice is remembered,
    /// and a refusal leaves the previous provider and states the SDK's own
    /// text verbatim.
    func select(contract: ProviderContract, providerKey: String) {
        applySelection(
            contract: contract,
            providerKey: providerKey,
            recordsChoice: true
        )
    }

    /// Clears every row message (a credential application answers the
    /// "Needs <credential>" reasons).
    func clearMessages() {
        rowMessages.removeAll()
    }

    /// Forgets the in-memory choices without touching the SDK registry or the
    /// record (the record is the caller's to clear).
    func reset() {
        rememberedProviderKeys.removeAll()
        rowMessages.removeAll()
    }

    // MARK: Private acts

    /// Applies one selection. `recordsChoice` distinguishes a user act (which
    /// persists) from a startup restore (which reads what is already stored).
    private func applySelection(
        contract: ProviderContract,
        providerKey: String,
        recordsChoice: Bool
    ) {
        guard !contract.isReadOnlyCapability else { return }
        let choiceName = contract.rememberedChoiceName
        guard let option = options(for: contract).first(
            where: { $0.providerKey == providerKey }
        ) else {
            rowMessages[choiceName] = CapabilityProviderRowMessage(
                severity: .refusal,
                text: ReferenceApplicationConfiguration.unknownProviderReason(
                    providerKey: providerKey
                )
            )
            return
        }
        let credential: String
        if option.requiresCredential {
            guard let appliedCredentialText = appliedCredential(providerKey)?
                .trimmingCharacters(in: .whitespacesAndNewlines),
                !appliedCredentialText.isEmpty
            else {
                rowMessages[choiceName] = CapabilityProviderRowMessage(
                    severity: .warning,
                    text: ReferenceApplicationConfiguration
                        .capabilityNeedsCredentialText(
                            credentialLabel: option.credentialLabel
                        )
                )
                return
            }
            credential = appliedCredentialText
        } else {
            // A keyless provider's contract argument is its deployment base
            // URL. Passing the descriptor's public demo address keeps the
            // crate from falling back to an undeclared localhost server; a
            // descriptor without one is refused with the missing-descriptor
            // reason instead of reaching anywhere. An automated routing
            // session may point the deployment at its loopback fixture server
            // through the documented launch-environment override.
            let deploymentBaseURLOverride = contract == .routing
                ? ReferenceApplicationConfiguration
                    .routingDeploymentBaseURLOverride()
                : nil
            guard option.hasDemoBaseURL || deploymentBaseURLOverride != nil else {
                rowMessages[choiceName] = CapabilityProviderRowMessage(
                    severity: .warning,
                    text: ReferenceApplicationConfiguration
                        .capabilityNotConfiguredText
                        + " "
                        + ReferenceApplicationConfiguration
                            .missingDemoEndpointReason(
                                provider: option.displayName
                            )
                )
                return
            }
            credential = deploymentBaseURLOverride ?? option.demoBaseURL
        }
        guard let sdk else {
            rowMessages[choiceName] = CapabilityProviderRowMessage(
                severity: .refusal,
                text: ReferenceApplicationConfiguration
                    .credentialSdkNotRunningText
            )
            return
        }
        do {
            try sdk.configureProvider(
                contract: contract,
                providerKey: providerKey,
                credential: credential
            )
        } catch {
            // The SDK validates before it moves the slot, so the previous
            // provider still serves; only the row's message changes. A
            // routing provider that could not answer is reported verbatim
            // with the next step, never replaced by a silent fallback.
            rowMessages[choiceName] = CapabilityProviderRowMessage(
                severity: .refusal,
                text: refusalText(for: error, contract: contract)
            )
            refresh()
            return
        }
        rowMessages.removeValue(forKey: choiceName)
        if recordsChoice {
            rememberedProviderKeys[choiceName] = providerKey
            persistRememberedProviderKeys()
        }
        refresh()
        onProviderSelectionChanged?(contract)
    }

    /// The row's refusal text: the SDK's own detail verbatim, followed by the
    /// documented next step when a routing provider could not be reached.
    private func refusalText(
        for error: Swift.Error,
        contract: ProviderContract
    ) -> String {
        let verbatimText = failureText(error)
        let isRouting = contract == .routing
        let isNetwork = isNetworkRequestFailure(error, text: verbatimText)
        guard isRouting, isNetwork else {
            return verbatimText
        }
        return verbatimText
            + " "
            + ReferenceApplicationConfiguration.routingUnreachableNextStepText
    }

    /// Whether one selection failure is the contract's network variant. The
    /// app's failure formatter already reduces it to the detail text, so the
    /// check reads both the error's own rendering and the formatter's.
    private func isNetworkRequestFailure(
        _ error: Swift.Error,
        text: String
    ) -> Bool {
        String(reflecting: error).contains("NetworkRequestFailed")
            || text.contains("NetworkRequestFailed")
    }

    /// Writes the capability map into the existing remembered record,
    /// preserving every other remembered choice.
    private func persistRememberedProviderKeys() {
        var choices = rememberedChoicesStore.load() ?? RememberedChoices()
        choices.capabilityProviders = rememberedProviderKeys
        rememberedChoicesStore.store(choices)
    }
}
