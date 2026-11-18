//! The per-capability provider state's unit tests: the SDK readback order,
//! the unconfigured row, the credential a selection hands the SDK, the
//! verbatim refusal that keeps the previous provider, the credential
//! requirement that never reaches the SDK, and the remembered choices'
//! round trip through `RememberedChoicesStore`. The SDK and the credential
//! lookup are hand-written fakes, because the generated `MapSdk` class can
//! only be constructed against a live provider registry.

import MapNativeCoreUI
import RusticMapsSDK
import XCTest

@testable import ReferenceApplication

/// A stub for the capability rows' SDK seam: recorded answers, recorded
/// configuration requests, and a successful change reflected in its own
/// readback the way the shipped registry records it.
private final class FakeCapabilityProviderSdk: CapabilityProviderConfiguring {
    /// One recorded configuration request.
    struct ConfigurationRequest: Equatable {
        /// The capability the request selected a provider for.
        let contract: ProviderContract
        /// The provider key the request named.
        let providerKey: String
        /// The credential the request carried.
        let credential: String

        static func == (
            lhs: ConfigurationRequest,
            rhs: ConfigurationRequest
        ) -> Bool {
            lhs.contract.rememberedChoiceName == rhs.contract.rememberedChoiceName
                && lhs.providerKey == rhs.providerKey
                && lhs.credential == rhs.credential
        }
    }

    /// The readback the fake answers with, in the order it stores it.
    private var registrations: [ProviderRegistration]

    /// The display names the fake reports for provider keys (the shared
    /// registry's descriptors would supply these in the application).
    private let displayNames: [String: String]

    /// The error a configuration request throws, when one is configured.
    var thrownError: Swift.Error?

    /// Every configuration request the fake received, in call order.
    private(set) var configurationRequests: [ConfigurationRequest] = []

    /// Creates the fake over one readback and its provider display names.
    init(
        registrations: [ProviderRegistration],
        displayNames: [String: String] = [:]
    ) {
        self.registrations = registrations
        self.displayNames = displayNames
    }

    func providerRegistrations() -> [ProviderRegistration] {
        registrations
    }

    func configureProvider(
        contract: ProviderContract,
        providerKey: String,
        credential: String
    ) throws {
        configurationRequests.append(
            ConfigurationRequest(
                contract: contract,
                providerKey: providerKey,
                credential: credential
            )
        )
        if let thrownError {
            throw thrownError
        }
        // A successful configuration records the selection, as the shipped
        // SDK does; the refused path above leaves the readback untouched.
        registrations = registrations.map { registration in
            guard registration.contract.rememberedChoiceName
                == contract.rememberedChoiceName
            else {
                return registration
            }
            return ProviderRegistration(
                contract: contract,
                providerKey: providerKey,
                displayName: displayNames[providerKey] ?? providerKey,
                registered: true
            )
        }
    }
}

/// A local refusal carrying its own text, so the row's verbatim rendering
/// can be asserted without a provider or the network.
private enum StubProviderSelectionError: Swift.Error, LocalizedError {
    /// The provider refused the selection with the given text.
    case refused(detailText: String)

    var errorDescription: String? {
        switch self {
        case let .refused(detailText):
            return detailText
        }
    }
}

/// The state type's unit tests.
@MainActor
final class CapabilityProviderStateTests: XCTestCase {

    // MARK: Named constants

    /// The synthetic credential the credentialed selection must pass through.
    /// It is not a provider credential and never reaches a build or a network.
    private static let syntheticCredential = "unit-test-only-credential"

    /// The synthetic public demo deployment address a keyless descriptor
    /// declares (never contacted: the fake SDK records it and no network
    /// call runs).
    private static let syntheticDemoBaseURL = "https://demo.invalid/router"

    /// The capability names the test registry entries declare (the shared
    /// registry's `served_contracts` spelling).
    private static let routingCapabilityName = "routing"

    // MARK: Row readback

    /// The rows follow the SDK's own order and derive their human wording
    /// from the contract case.
    func testRowsFollowTheSdkOrder() throws {
        let sdk = FakeCapabilityProviderSdk(
            registrations: [
                registration(
                    contract: .routing,
                    providerKey: "keyless-routing",
                    displayName: "Keyless Routing",
                    registered: true
                ),
                registration(
                    contract: .geocoding,
                    providerKey: "",
                    displayName: "",
                    registered: false
                ),
                registration(
                    contract: .distanceMatrix,
                    providerKey: "",
                    displayName: "",
                    registered: false
                ),
            ]
        )
        let state = makeState(sdk: sdk)

        XCTAssertEqual(
            state.rows.map(\.id),
            ["Routing", "Geocoding", "DistanceMatrix"],
            "the rows must stay in the SDK's order, never re-sorted"
        )
        XCTAssertEqual(
            state.rows.map(\.title),
            ["Routing", "Geocoding", "Distance matrix"],
            "each row's wording must derive from the contract case"
        )
    }

    /// An unregistered capability reads "Not configured" rather than a
    /// blank serving line.
    func testAnUnregisteredRowReadsNotConfigured() throws {
        let sdk = FakeCapabilityProviderSdk(
            registrations: [
                registration(
                    contract: .elevation,
                    providerKey: "",
                    displayName: "",
                    registered: false
                )
            ]
        )
        let state = makeState(sdk: sdk)

        XCTAssertEqual(state.rows.count, 1)
        XCTAssertEqual(
            state.rows[0].servingText,
            ReferenceApplicationConfiguration.capabilityNotConfiguredText
        )
        XCTAssertFalse(state.rows[0].isRegistered)
    }

    // MARK: Selection credential

    /// A selection hands the SDK the application's applied credential for a
    /// credentialed provider, and the empty string for a keyless one.
    func testSelectionPassesTheAppliedCredentialOnlyToACredentialedProvider() throws {
        let keylessDescriptor = try descriptor(
            providerKey: "keyless-routing",
            displayName: "Keyless Routing",
            servedContracts: [Self.routingCapabilityName],
            demoBaseURL: Self.syntheticDemoBaseURL
        )
        let credentialedDescriptor = try descriptor(
            providerKey: "credentialed-routing",
            displayName: "Credentialed Routing",
            credentialLabel: "API key",
            servedContracts: [Self.routingCapabilityName]
        )
        let sdk = FakeCapabilityProviderSdk(
            registrations: [
                registration(
                    contract: .routing,
                    providerKey: "keyless-routing",
                    displayName: "Keyless Routing",
                    registered: true
                )
            ],
            displayNames: [
                "keyless-routing": "Keyless Routing",
                "credentialed-routing": "Credentialed Routing",
            ]
        )
        let state = makeState(
            sdk: sdk,
            descriptors: [keylessDescriptor, credentialedDescriptor],
            appliedCredential: { providerKey in
                providerKey == "credentialed-routing"
                    ? Self.syntheticCredential
                    : nil
            }
        )

        state.select(contract: .routing, providerKey: "keyless-routing")
        state.select(contract: .routing, providerKey: "credentialed-routing")

        XCTAssertEqual(
            sdk.configurationRequests,
            [
                FakeCapabilityProviderSdk.ConfigurationRequest(
                    contract: .routing,
                    providerKey: "keyless-routing",
                    credential: Self.syntheticDemoBaseURL
                ),
                FakeCapabilityProviderSdk.ConfigurationRequest(
                    contract: .routing,
                    providerKey: "credentialed-routing",
                    credential: Self.syntheticCredential
                ),
            ]
        )
        XCTAssertEqual(
            state.rows[0].servingText,
            "Credentialed Routing · Ready",
            "the accepted selection must refresh the row from the readback"
        )
    }

    /// A credentialed provider with no applied credential shows the reason in
    /// the picker and never reaches the SDK.
    func testAnUncredentialedSelectionStatesTheReasonWithoutCallingTheSdk() throws {
        let credentialedDescriptor = try descriptor(
            providerKey: "credentialed-routing",
            displayName: "Credentialed Routing",
            credentialLabel: "API key",
            servedContracts: [Self.routingCapabilityName]
        )
        let sdk = FakeCapabilityProviderSdk(
            registrations: [
                registration(
                    contract: .routing,
                    providerKey: "",
                    displayName: "",
                    registered: false
                )
            ]
        )
        let state = makeState(
            sdk: sdk,
            descriptors: [credentialedDescriptor],
            appliedCredential: { _ in nil }
        )

        XCTAssertEqual(
            state.options(for: .routing).map(\.pickerLabel),
            ["Credentialed Routing · Needs API key"],
            "a credentialed provider without its credential must compose the name and state"
        )

        state.select(contract: .routing, providerKey: "credentialed-routing")

        XCTAssertTrue(
            sdk.configurationRequests.isEmpty,
            "a credential requirement is a next step, not an SDK call"
        )
        XCTAssertEqual(
            state.rowMessage(for: .routing)?.text,
            ReferenceApplicationConfiguration.capabilityNeedsCredentialText(
                credentialLabel: "API key"
            )
        )
        XCTAssertEqual(
            state.rowMessage(for: .routing)?.severity,
            CapabilityProviderRowMessage.Severity.warning
        )
    }

    /// A refusal keeps the previous provider in force and renders the SDK's
    /// own text verbatim under the row.
    func testARefusalKeepsThePreviousProviderAndStatesTheSdkText() throws {
        let previousDescriptor = try descriptor(
            providerKey: "previous-routing",
            displayName: "Previous Routing",
            servedContracts: [Self.routingCapabilityName],
            demoBaseURL: Self.syntheticDemoBaseURL
        )
        let nextDescriptor = try descriptor(
            providerKey: "next-routing",
            displayName: "Next Routing",
            servedContracts: [Self.routingCapabilityName],
            demoBaseURL: Self.syntheticDemoBaseURL
        )
        let refusalText = "the provider refused the selection"
        let sdk = FakeCapabilityProviderSdk(
            registrations: [
                registration(
                    contract: .routing,
                    providerKey: "previous-routing",
                    displayName: "Previous Routing",
                    registered: true
                )
            ],
            displayNames: [
                "previous-routing": "Previous Routing",
                "next-routing": "Next Routing",
            ]
        )
        let store = makeStore()
        store.store(
            RememberedChoices(capabilityProviders: ["Routing": "previous-routing"])
        )
        let state = makeState(
            sdk: sdk,
            descriptors: [previousDescriptor, nextDescriptor],
            store: store
        )
        state.restoreRememberedChoices()

        // The restore above succeeded; the refusal belongs to the next act.
        sdk.thrownError = StubProviderSelectionError.refused(
            detailText: refusalText
        )
        state.select(contract: .routing, providerKey: "next-routing")

        XCTAssertEqual(
            state.rows[0].servingText,
            "Previous Routing · Ready",
            "a refused selection must leave the previous provider serving"
        )
        XCTAssertEqual(
            state.rememberedProviderKeys["Routing"],
            "previous-routing",
            "a refused selection must not record the rejected provider"
        )
        XCTAssertEqual(
            state.rowMessage(for: .routing)?.text,
            refusalText,
            "the row must show the thrown text verbatim"
        )
        XCTAssertEqual(
            state.rowMessage(for: .routing)?.severity,
            CapabilityProviderRowMessage.Severity.refusal
        )
    }

    // MARK: Row composition and demo endpoints

    /// Every row and picker entry composes the provider's display name with
    /// its credential state, and the serving descriptor's demo address and
    /// terms travel with the row.
    func testProviderRowsAndPickerEntriesComposeNameAndState() throws {
        let keylessDescriptor = try descriptor(
            providerKey: "keyless-routing",
            displayName: "Keyless Routing",
            servedContracts: [Self.routingCapabilityName],
            demoBaseURL: Self.syntheticDemoBaseURL
        )
        let credentialedDescriptor = try descriptor(
            providerKey: "credentialed-routing",
            displayName: "Credentialed Routing",
            credentialLabel: "API key",
            servedContracts: [Self.routingCapabilityName]
        )
        let sdk = FakeCapabilityProviderSdk(
            registrations: [
                registration(
                    contract: .routing,
                    providerKey: "keyless-routing",
                    displayName: "Keyless Routing",
                    registered: true
                )
            ]
        )
        let state = makeState(
            sdk: sdk,
            descriptors: [keylessDescriptor, credentialedDescriptor]
        )

        XCTAssertEqual(
            state.rows[0].servingText,
            "Keyless Routing · Ready",
            "the serving line must compose the name and its state"
        )
        XCTAssertEqual(
            state.options(for: .routing).map(\.pickerLabel),
            [
                "Keyless Routing · Ready",
                "Credentialed Routing · Needs API key",
            ],
            "every picker entry must compose the name and its state"
        )
        XCTAssertEqual(
            state.servingDemoEndpointText(for: .routing),
            "Demo server: \(Self.syntheticDemoBaseURL)",
            "the routing row must state where a keyless provider sends requests"
        )
        XCTAssertEqual(
            state.servingTermsNote(for: .routing),
            "test terms",
            "the routing row must state the serving descriptor's terms"
        )
    }

    /// A keyless provider whose descriptor declares no demo deployment
    /// address is refused with a stated reason instead of reaching an
    /// undeclared localhost server.
    func testAKeylessProviderWithoutADemoAddressIsRefusedWithoutCallingTheSdk() throws {
        let keylessDescriptor = try descriptor(
            providerKey: "keyless-routing",
            displayName: "Keyless Routing",
            servedContracts: [Self.routingCapabilityName]
        )
        let sdk = FakeCapabilityProviderSdk(
            registrations: [
                registration(
                    contract: .routing,
                    providerKey: "",
                    displayName: "",
                    registered: false
                )
            ]
        )
        let state = makeState(sdk: sdk, descriptors: [keylessDescriptor])

        XCTAssertFalse(
            state.options(for: .routing)[0].isSelectable,
            "a keyless provider without a demo address cannot be selected"
        )
        XCTAssertEqual(
            state.options(for: .routing)[0].pickerLabel,
            "Keyless Routing · Not configured",
            "the picker entry must state the missing configuration"
        )

        state.select(contract: .routing, providerKey: "keyless-routing")

        XCTAssertTrue(
            sdk.configurationRequests.isEmpty,
            "a missing demo address is a next step, not an SDK call"
        )
        XCTAssertEqual(
            state.rowMessage(for: .routing)?.severity,
            CapabilityProviderRowMessage.Severity.warning
        )
    }

    /// An unreachable routing demo server is reported verbatim with the
    /// documented next step, and the previous provider keeps serving.
    func testAnUnreachableRoutingServerIsReportedVerbatimWithTheNextStep() throws {
        let previousDescriptor = try descriptor(
            providerKey: "previous-routing",
            displayName: "Previous Routing",
            servedContracts: [Self.routingCapabilityName],
            demoBaseURL: Self.syntheticDemoBaseURL
        )
        let nextDescriptor = try descriptor(
            providerKey: "next-routing",
            displayName: "Next Routing",
            servedContracts: [Self.routingCapabilityName],
            demoBaseURL: Self.syntheticDemoBaseURL
        )
        let sdk = FakeCapabilityProviderSdk(
            registrations: [
                registration(
                    contract: .routing,
                    providerKey: "previous-routing",
                    displayName: "Previous Routing",
                    registered: true
                )
            ],
            displayNames: [
                "previous-routing": "Previous Routing",
                "next-routing": "Next Routing",
            ]
        )
        let state = makeState(
            sdk: sdk,
            descriptors: [previousDescriptor, nextDescriptor],
            failureText: { error in
                // The application's formatter reduces a contract error to
                // its verbatim detail, exactly as the live model does.
                if let sdkError = error as? SdkError,
                   case let .NetworkRequestFailed(detail) = sdkError
                {
                    return detail
                }
                return CapabilityProviderState.defaultFailureText(error)
            }
        )
        let unreachableDetail = "The demo server at \(Self.syntheticDemoBaseURL) refused the connection."
        sdk.thrownError = SdkError.NetworkRequestFailed(detail: unreachableDetail)

        state.select(contract: .routing, providerKey: "next-routing")

        XCTAssertEqual(
            state.rows[0].servingText,
            "Previous Routing · Ready",
            "an unreachable server must not replace the previous provider"
        )
        XCTAssertEqual(
            state.rowMessage(for: .routing)?.text,
            unreachableDetail
                + " "
                + ReferenceApplicationConfiguration.routingUnreachableNextStepText,
            "the row must state the provider's verbatim detail and the next step"
        )
        XCTAssertEqual(
            state.rowMessage(for: .routing)?.severity,
            CapabilityProviderRowMessage.Severity.refusal
        )
    }

    // MARK: Remembered choices

    /// A recorded capability choice rides the versioned record and applies
    /// on the next launch through the same selection path; a record written
    /// before the field existed still loads with an empty map.
    func testRememberedChoicesRoundTripThroughTheStore() throws {
        let store = makeStore()
        XCTAssertNil(store.load(), "a device that never chose has no record")

        let keylessDescriptor = try descriptor(
            providerKey: "keyless-routing",
            displayName: "Keyless Routing",
            servedContracts: [Self.routingCapabilityName],
            demoBaseURL: Self.syntheticDemoBaseURL
        )
        let firstSdk = FakeCapabilityProviderSdk(
            registrations: [
                registration(
                    contract: .routing,
                    providerKey: "",
                    displayName: "",
                    registered: false
                )
            ],
            displayNames: ["keyless-routing": "Keyless Routing"]
        )
        let firstState = makeState(
            sdk: firstSdk,
            descriptors: [keylessDescriptor],
            store: store
        )
        firstState.select(contract: .routing, providerKey: "keyless-routing")

        let storedRecord = try XCTUnwrap(store.load())
        XCTAssertEqual(
            storedRecord.capabilityProviders,
            ["Routing": "keyless-routing"],
            "the accepted choice must ride the remembered record"
        )
        XCTAssertEqual(
            RememberedChoicesStore.recordVersion,
            1,
            "the added field must not bump the record version"
        )

        // The next launch reads the same record: the row starts unconfigured
        // and the restore applies the remembered provider through the SDK.
        let secondSdk = FakeCapabilityProviderSdk(
            registrations: [
                registration(
                    contract: .routing,
                    providerKey: "",
                    displayName: "",
                    registered: false
                )
            ],
            displayNames: ["keyless-routing": "Keyless Routing"]
        )
        let secondState = makeState(
            sdk: secondSdk,
            descriptors: [keylessDescriptor],
            store: store
        )
        secondState.restoreRememberedChoices()

        XCTAssertEqual(
            secondSdk.configurationRequests,
            [
                FakeCapabilityProviderSdk.ConfigurationRequest(
                    contract: .routing,
                    providerKey: "keyless-routing",
                    credential: Self.syntheticDemoBaseURL
                )
            ],
            "the remembered provider must apply through the same selection path"
        )
        XCTAssertEqual(secondState.rows[0].servingText, "Keyless Routing · Ready")

        // A record written before per-capability choices existed carries no
        // field; it must still load, with an empty map.
        let legacyDefaults = makeDefaults()
        let legacyStore = RememberedChoicesStore(defaults: legacyDefaults)
        let legacyRecordText = """
            {"version":1,"theme":"auto","scenes":[],"search":{"origin":"",\
            "destination":"","query":""}}
            """
        legacyDefaults.set(
            legacyRecordText,
            forKey: RememberedChoicesStore.recordDefaultsKey
        )
        let legacyRecord = try XCTUnwrap(legacyStore.load())
        XCTAssertEqual(
            legacyRecord.capabilityProviders,
            [:],
            "a record without the field must decode to an empty map"
        )
    }

    // MARK: Helpers

    /// Builds one SDK readback entry.
    private func registration(
        contract: ProviderContract,
        providerKey: String,
        displayName: String,
        registered: Bool
    ) -> ProviderRegistration {
        ProviderRegistration(
            contract: contract,
            providerKey: providerKey,
            displayName: displayName,
            registered: registered
        )
    }

    /// Decodes one registry descriptor through the shared schema, so the
    /// test exercises the same `served_contracts` decoding the application
    /// reads.
    private func descriptor(
        providerKey: String,
        displayName: String,
        credentialLabel: String = "",
        servedContracts: [String],
        demoBaseURL: String = "",
        iosAvailable: Bool = true
    ) throws -> ProviderDescriptor {
        var descriptorObject: [String: Any] = [
            "provider_key": providerKey,
            "display_name": displayName,
            "credential_label": credentialLabel,
            "served_contracts": servedContracts,
            "registration_available": [
                "android": false,
                "ios": iosAvailable,
                "react_native": false,
                "wasm": false,
            ],
            "attribution_text": "test attribution",
            "terms_note": "test terms",
            "settings_hint": "",
        ]
        if !demoBaseURL.isEmpty {
            descriptorObject["demo_base_url"] = demoBaseURL
            descriptorObject["demo_base_url_note"] = "public demo server"
        }
        let descriptorData = try JSONSerialization.data(
            withJSONObject: descriptorObject
        )
        return try JSONDecoder().decode(ProviderDescriptor.self, from: descriptorData)
    }

    /// Creates the state under test over one fake SDK and registry.
    private func makeState(
        sdk: FakeCapabilityProviderSdk,
        descriptors: [ProviderDescriptor] = [],
        appliedCredential: @escaping (String) -> String? = { _ in nil },
        failureText: @escaping (Swift.Error) -> String
            = CapabilityProviderState.defaultFailureText,
        store: RememberedChoicesStore
    ) -> CapabilityProviderState {
        let state = CapabilityProviderState(rememberedChoicesStore: store)
        state.attach(
            sdk: sdk,
            descriptors: descriptors,
            appliedCredential: appliedCredential,
            failureText: failureText
        )
        return state
    }

    /// Creates the state under test over a fresh isolated store.
    private func makeState(
        sdk: FakeCapabilityProviderSdk,
        descriptors: [ProviderDescriptor] = [],
        appliedCredential: @escaping (String) -> String? = { _ in nil },
        failureText: @escaping (Swift.Error) -> String
            = CapabilityProviderState.defaultFailureText
    ) -> CapabilityProviderState {
        makeState(
            sdk: sdk,
            descriptors: descriptors,
            appliedCredential: appliedCredential,
            failureText: failureText,
            store: makeStore()
        )
    }

    /// A store over an isolated defaults suite, so one test cannot inherit
    /// another run's record.
    private func makeStore() -> RememberedChoicesStore {
        RememberedChoicesStore(defaults: makeDefaults())
    }

    /// An isolated defaults domain, cleared before use and removed at
    /// teardown.
    private func makeDefaults() -> UserDefaults {
        let suiteName = "CapabilityProviderStateTests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suiteName)!
        defaults.removePersistentDomain(forName: suiteName)
        addTeardownBlock {
            defaults.removePersistentDomain(forName: suiteName)
        }
        return defaults
    }
}
