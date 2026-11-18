//! The type-ahead contract (`docs/ReferenceAppUX.md §5.5`): the
//! three-character threshold, the quiet window before a request, the
//! five-row cap, the supersede rule, selection filling the field, the
//! no-match and failure states, and the credential-state provider choice.
//!
//! The controller is exercised with hand-written providers over a clock the
//! tests can observe, so no test touches the network.

import MapNativeCoreUI
import RusticMapsSDK
import XCTest

@testable import ReferenceApplication

/// A provider that records every query and answers from the test's set.
@MainActor
private final class RecordingTypeAheadProvider {
    private(set) var queries: [String] = []
    var rows: [Place] = []
    var failure: Swift.Error?

    func suggest(_ query: String) async throws -> [Place] {
        queries.append(query)
        if let failure {
            throw failure
        }
        return rows
    }
}

/// A provider whose answers the test releases one at a time, so a superseded
/// answer can arrive late and prove it is dropped.
@MainActor
private final class DeferredTypeAheadProvider {
    private(set) var queries: [String] = []
    private var continuations: [String: CheckedContinuation<[Place], Swift.Error>] = [:]

    func suggest(_ query: String) async throws -> [Place] {
        queries.append(query)
        return try await withCheckedThrowingContinuation { continuation in
            continuations[query] = continuation
        }
    }

    func answer(_ query: String, with places: [Place]) {
        continuations.removeValue(forKey: query)?.resume(returning: places)
    }
}

/// A test failure the controller formats into its one inline sentence.
private struct TypeAheadTestFailure: Swift.Error {}

@MainActor
final class SearchTypeAheadTests: XCTestCase {
    // MARK: Helpers

    /// One place with a display line; the type-ahead only reads these.
    private func place(_ name: String) -> Place {
        Place(
            structuredAddressText: name,
            coordinate: GeographicCoordinate(latitude: 38.5, longitude: -121.5),
            geocodeQuality: .point
        )
    }

    /// Polls a main-actor condition until it holds or the timeout passes.
    private func waitUntil(
        timeout: TimeInterval = 2,
        _ condition: @MainActor () -> Bool
    ) async -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if condition() {
                return true
            }
            try? await Task.sleep(nanoseconds: 20_000_000)
        }
        return condition()
    }

    // MARK: The rule

    func testFewerThanThreeCharactersNeverAskTheProvider() async {
        let provider = RecordingTypeAheadProvider()
        let controller = SearchTypeAheadController(suggest: { query in
            try await provider.suggest(query)
        })

        controller.queryChanged("ab")
        try? await Task.sleep(nanoseconds: 500_000_000)

        XCTAssertEqual(
            provider.queries, [],
            "a query below the three-character threshold must not reach the provider"
        )
        XCTAssertEqual(controller.phase, .idle)
        XCTAssertTrue(controller.suggestions.isEmpty)
    }

    func testAQueryWaitsForTheQuietWindowBeforeAskingTheProvider() async {
        let provider = RecordingTypeAheadProvider()
        let controller = SearchTypeAheadController(suggest: { query in
            try await provider.suggest(query)
        })

        let startedAt = Date()
        controller.queryChanged("Cap")
        try? await Task.sleep(nanoseconds: 100_000_000)
        XCTAssertEqual(
            provider.queries, [],
            "no request may start before the quiet window passes"
        )

        let asked = await waitUntil { !provider.queries.isEmpty }
        XCTAssertTrue(asked, "the query must reach the provider after the quiet window")
        XCTAssertEqual(provider.queries, ["Cap"])
        XCTAssertGreaterThanOrEqual(
            Date().timeIntervalSince(startedAt) * 1000,
            AppLayout.autocompleteDebounceMilliseconds * 0.9,
            "the request must wait out the shared 250 ms window"
        )
    }

    func testKeystrokesInsideTheQuietWindowCoalesceIntoOneRequest() async {
        let provider = RecordingTypeAheadProvider()
        let controller = SearchTypeAheadController(suggest: { query in
            try await provider.suggest(query)
        })

        controller.queryChanged("Cap")
        try? await Task.sleep(nanoseconds: 60_000_000)
        controller.queryChanged("Capi")
        try? await Task.sleep(nanoseconds: 60_000_000)
        controller.queryChanged("Capitol")

        let asked = await waitUntil { !provider.queries.isEmpty }
        XCTAssertTrue(asked)
        XCTAssertEqual(
            provider.queries, ["Capitol"],
            "only the query still current when the window closes may ask"
        )
    }

    func testAtMostFiveSuggestionsAreListed() async {
        let provider = RecordingTypeAheadProvider()
        provider.rows = (1...7).map { place("Suggestion \($0)") }
        let controller = SearchTypeAheadController(suggest: { query in
            try await provider.suggest(query)
        })

        controller.queryChanged("Seven")
        let listed = await waitUntil { !controller.suggestions.isEmpty }

        XCTAssertTrue(listed)
        XCTAssertEqual(
            controller.suggestions.count,
            AppLayout.autocompleteMaximumResults,
            "the list must stop at the shared five-row cap"
        )
        XCTAssertEqual(controller.phase, .suggestions)
    }

    func testALateSupersededAnswerNeverReplacesTheList() async {
        let provider = DeferredTypeAheadProvider()
        let controller = SearchTypeAheadController(suggest: { query in
            try await provider.suggest(query)
        })

        controller.queryChanged("Capitol")
        let firstAsked = await waitUntil { provider.queries == ["Capitol"] }
        XCTAssertTrue(firstAsked)

        controller.queryChanged("Tower")
        let secondAsked = await waitUntil {
            provider.queries == ["Capitol", "Tower"]
        }
        XCTAssertTrue(secondAsked)

        // The older answer arrives after the newer query took over.
        provider.answer("Capitol", with: [place("Capitol Park")])
        try? await Task.sleep(nanoseconds: 100_000_000)
        XCTAssertEqual(
            controller.phase, .idle,
            "a superseded answer must leave the list exactly as it was"
        )
        XCTAssertTrue(controller.suggestions.isEmpty)

        provider.answer("Tower", with: [place("Tower Bridge")])
        let listed = await waitUntil { controller.phase == .suggestions }

        XCTAssertTrue(listed)
        XCTAssertEqual(
            controller.suggestions.map(\.structuredAddressText),
            ["Tower Bridge"],
            "the current query's answer is the only one that may fill the list"
        )
    }

    func testNoMatchesShowsTheOneRowState() async {
        let provider = RecordingTypeAheadProvider()
        provider.rows = []
        let controller = SearchTypeAheadController(suggest: { query in
            try await provider.suggest(query)
        })

        controller.queryChanged("Nowhere")
        let noMatches = await waitUntil { controller.phase == .noMatches }

        XCTAssertTrue(noMatches)
        XCTAssertTrue(controller.suggestions.isEmpty)
    }

    func testAFailureKeepsTheStateAndALaterKeystrokeRetries() async {
        let provider = RecordingTypeAheadProvider()
        provider.failure = TypeAheadTestFailure()
        let controller = SearchTypeAheadController(suggest: { query in
            try await provider.suggest(query)
        })

        controller.queryChanged("Fail")
        let failed = await waitUntil {
            if case .failed = controller.phase { return true }
            return false
        }
        XCTAssertTrue(failed)
        guard case let .failed(sentence) = controller.phase else {
            return XCTFail("the failure state must carry its one sentence")
        }
        XCTAssertTrue(
            sentence.contains("Suggestions are unavailable"),
            "the inline line must be the documented single sentence"
        )
        XCTAssertTrue(controller.suggestions.isEmpty)

        provider.failure = nil
        provider.rows = [place("Capitol Park")]
        controller.queryChanged("Fail again")
        let listed = await waitUntil { controller.phase == .suggestions }

        XCTAssertTrue(listed, "a later keystroke must retry the provider")
    }

    // MARK: Selection fills the field

    func testChoosingASuggestionReportsTheDisplayLineAndClosesTheList() async {
        let provider = RecordingTypeAheadProvider()
        provider.rows = [place("Capitol Park")]
        let controller = SearchTypeAheadController(suggest: { query in
            try await provider.suggest(query)
        })
        var selectedDisplayLine: String?
        controller.onSelect = { selectedPlace in
            selectedDisplayLine = selectedPlace.structuredAddressText
        }

        controller.queryChanged("Cap")
        let listed = await waitUntil { controller.phase == .suggestions }
        XCTAssertTrue(listed)
        guard let listedSuggestion = controller.suggestions.first else {
            return XCTFail("the provider's answer must produce one row")
        }
        controller.select(listedSuggestion)

        XCTAssertEqual(selectedDisplayLine, "Capitol Park")
        XCTAssertEqual(controller.phase, .idle)
        XCTAssertTrue(controller.suggestions.isEmpty)
    }

    func testTheModelFillsTheChosenFieldExactlyAsTypedText() {
        let model = ReferenceApplicationModel()
        let suggestion = place("Tower Bridge")

        model.assignTypeAheadSuggestion(suggestion, to: .destination)

        XCTAssertEqual(model.destinationSearchText, "Tower Bridge")
        XCTAssertEqual(model.destinationAssignedText, "Tower Bridge")
        XCTAssertEqual(model.destinationCoordinate, suggestion.coordinate)
        XCTAssertEqual(model.searchFieldAssignmentTarget, .destination)
        XCTAssertEqual(model.originSearchText, "")
    }

    // MARK: The provider choice

    func testNoCredentialChoosesTheRegistrysKeylessSearchProvider() throws {
        let descriptors = try decodedDescriptors()
        let choice = SearchTypeAheadProviderChooser.resolve(
            descriptors: descriptors,
            appliedCredentialKeys: [],
            registrations: []
        )

        guard case let .register(descriptor, deploymentBaseURL) = choice else {
            return XCTFail("a keyless session must install the registry's keyless provider")
        }
        XCTAssertEqual(descriptor.providerKey, "keyless-search")
        XCTAssertEqual(deploymentBaseURL, "https://keyless.example")
    }

    func testTheAutomationDeploymentOverrideReplacesTheRegistrysDemoAddress() throws {
        let descriptors = try decodedDescriptors()
        let choice = SearchTypeAheadProviderChooser.resolve(
            descriptors: descriptors,
            appliedCredentialKeys: [],
            registrations: [],
            deploymentBaseURLOverride: "http://127.0.0.1:4321"
        )

        guard case let .register(_, deploymentBaseURL) = choice else {
            return XCTFail("a keyless session must install the registry's keyless provider")
        }
        XCTAssertEqual(deploymentBaseURL, "http://127.0.0.1:4321")
    }

    func testAConfiguredCredentialUsesItsRegisteredProvider() throws {
        let descriptors = try decodedDescriptors()
        let choice = SearchTypeAheadProviderChooser.resolve(
            descriptors: descriptors,
            appliedCredentialKeys: ["credentialed-search"],
            registrations: [
                ProviderRegistration(
                    contract: .placeSearch,
                    providerKey: "credentialed-search",
                    displayName: "Credentialed Search",
                    registered: true
                )
            ]
        )

        XCTAssertEqual(
            choice,
            SearchTypeAheadProviderChoice.alreadyServing(
                providerKey: "credentialed-search"
            )
        )
    }

    func testAConfiguredCredentialWithoutAPlaceSearchRegistrationStaysUnconfigured() throws {
        let descriptors = try decodedDescriptors()
        let choice = SearchTypeAheadProviderChooser.resolve(
            descriptors: descriptors,
            appliedCredentialKeys: ["credentialed-search"],
            registrations: [
                ProviderRegistration(
                    contract: .placeSearch,
                    providerKey: "",
                    displayName: "",
                    registered: false
                )
            ]
        )

        XCTAssertNil(
            choice,
            "a credential session must not silently install the keyless provider"
        )
    }

    func testTheKeylessProviderIsChosenEvenWhenACredentialedOneComesFirst() throws {
        let descriptors = try decodedDescriptors()
        let keylessChoice = SearchTypeAheadProviderChooser.resolve(
            descriptors: descriptors,
            appliedCredentialKeys: [],
            registrations: []
        )

        guard case let .register(descriptor, _) = keylessChoice else {
            return XCTFail("the keyless provider must be the selected one")
        }
        XCTAssertEqual(descriptor.providerKey, "keyless-search")
    }

    // MARK: The test registry

    /// Two place-search descriptors in registry order: the credentialed one
    /// first, the keyless one second, alongside a provider for another
    /// capability so the chooser must filter by the contract.
    private func decodedDescriptors() throws -> [ProviderDescriptor] {
        let registryJSON = """
        {
          "schema_version": 1,
          "providers": [
            {
              "provider_key": "credentialed-search",
              "display_name": "Credentialed Search",
              "credential_label": "API key",
              "served_contracts": ["place_search", "geocoding"],
              "registration_available": {
                "android": true, "ios": true, "react_native": true, "wasm": true
              },
              "attribution_text": "Credentialed Search attribution",
              "terms_note": "Credentialed Search terms",
              "settings_hint": "Enter the key."
            },
            {
              "provider_key": "keyless-search",
              "display_name": "Keyless Search",
              "credential_label": "",
              "served_contracts": ["place_search"],
              "registration_available": {
                "android": true, "ios": true, "react_native": true, "wasm": true
              },
              "attribution_text": "Keyless Search attribution",
              "terms_note": "Keyless Search terms",
              "demo_base_url": "https://keyless.example",
              "demo_base_url_note": "Public demo deployment.",
              "settings_hint": "Use the public deployment."
            },
            {
              "provider_key": "routing-only",
              "display_name": "Routing Only",
              "credential_label": "",
              "served_contracts": ["routing"],
              "registration_available": {
                "android": true, "ios": true, "react_native": true, "wasm": true
              },
              "attribution_text": "Routing attribution",
              "terms_note": "Routing terms",
              "settings_hint": ""
            }
          ]
        }
        """
        let catalog = try ProviderRegistryCatalog.decoded(
            from: Data(registryJSON.utf8)
        )
        return catalog.providers
    }
}
