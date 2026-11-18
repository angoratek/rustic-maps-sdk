//! The search fields' type-ahead (`docs/design/Autocomplete.md`): the
//! three-character threshold, the quiet window before a provider request,
//! the five-row cap, cancellation of a superseded request, and the four
//! visible states — nothing, suggestions, one no-match row, or one inline
//! failure sentence.
//!
//! The controller owns only the contract call (`MapSdk.suggest`); which
//! provider answers is a startup decision. [`SearchTypeAheadProviderChooser`]
//! reads the shared registry and the SDK's own registration readback, so a
//! credential state picks its provider without a provider name in this
//! application.

import Combine
import Foundation
import MapNativeCoreUI
import RusticMapsSDK

/// The consumer wording of the type-ahead's two non-result states.
enum SearchTypeAhead {
    /// The one row a query with no candidates shows (never an empty list).
    static let noMatchesText = ReferenceApplicationConfiguration.planAutocompleteNoMatchesText

    /// The one inline sentence a provider failure shows. The provider's own
    /// detail rides inside it, so the sentence stays honest without becoming
    /// a second report block.
    static func failureSentence(detail: String) -> String {
        let trimmedDetail = detail.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedDetail.isEmpty else {
            return ReferenceApplicationConfiguration.planAutocompleteUnavailableText
        }
        return "Suggestions are unavailable: \(trimmedDetail)"
    }
}

/// The failure a type-ahead call reports when the SDK is not running (the
/// readiness ladder states the real reason elsewhere).
enum SearchTypeAheadError: Swift.Error, LocalizedError {
    /// No `MapSdk` exists to resolve the place-search provider.
    case sdkUnavailable

    var errorDescription: String? {
        "the SDK is not running; the readiness ladder states why"
    }
}

/// One search field's type-ahead: debounces the typed query, asks the
/// provider once the quiet window passes, caps what it shows, and drops any
/// answer that a newer query superseded.
@MainActor
final class SearchTypeAheadController: ObservableObject {
    /// The list's visible state (`.suggestions` carries rows,
    /// `.noMatches` one row, `.failed` one sentence, `.idle` nothing).
    enum Phase: Equatable {
        /// No query worth asking for, and nothing to show.
        case idle
        /// The capped candidate rows are listed.
        case suggestions
        /// The provider answered with no candidates.
        case noMatches
        /// The provider failed; the sentence is the one inline line.
        case failed(String)
    }

    /// The provider call the controller debounces.
    typealias Suggest = @MainActor (String) async throws -> [Place]
    /// The formatter of a provider failure's one-sentence text.
    typealias FailureText = @MainActor (Swift.Error) -> String

    /// The rows currently listed (at most `maximumResults`).
    @Published private(set) var suggestions: [Place] = []
    /// The list's visible state.
    @Published private(set) var phase: Phase = .idle
    /// What choosing a row does; the owning field fills from it.
    var onSelect: ((Place) -> Void)?

    /// The provider call.
    private let suggest: Suggest
    /// The failure text formatter.
    private let failureText: FailureText
    /// The trimmed query length that starts a request.
    private let minimumQueryCharacters: Int
    /// The quiet period before a request.
    private let debounceMilliseconds: TimeInterval
    /// The row cap.
    private let maximumResults: Int
    /// The debounce-and-request task; canceled by every newer query.
    private var pendingTask: Task<Void, Never>?
    /// The newest query's sequence number. An answer applies only when its
    /// number still matches, so a late superseded answer is dropped.
    private var requestGeneration = 0

    /// Creates one field's controller over the SDK's suggest call.
    ///
    /// - Parameters:
    ///   - suggest: The provider call to debounce.
    ///   - minimumQueryCharacters: The trimmed length that starts a request.
    ///   - debounceMilliseconds: The quiet period before a request.
    ///   - maximumResults: The row cap.
    ///   - failureText: The formatter of a failure's inline sentence.
    init(
        suggest: @escaping Suggest,
        minimumQueryCharacters: Int = AppLayout.autocompleteMinimumQueryCharacters,
        debounceMilliseconds: TimeInterval = AppLayout.autocompleteDebounceMilliseconds,
        maximumResults: Int = AppLayout.autocompleteMaximumResults,
        failureText: @escaping FailureText = { error in
            SearchTypeAhead.failureSentence(detail: String(describing: error))
        }
    ) {
        self.suggest = suggest
        self.minimumQueryCharacters = minimumQueryCharacters
        self.debounceMilliseconds = debounceMilliseconds
        self.maximumResults = maximumResults
        self.failureText = failureText
    }

    /// Records a new query. The previous task is canceled first; a query
    /// shorter than the threshold clears the list without a request, and a
    /// longer one asks the provider after the quiet window.
    func queryChanged(_ text: String) {
        pendingTask?.cancel()
        pendingTask = nil
        requestGeneration &+= 1
        let generation = requestGeneration
        let trimmedQuery = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmedQuery.count >= minimumQueryCharacters else {
            suggestions = []
            phase = .idle
            return
        }
        pendingTask = Task { [weak self] in
            guard let self else { return }
            do {
                try await Task.sleep(nanoseconds: Self.nanoseconds(
                    forMilliseconds: self.debounceMilliseconds
                ))
            } catch {
                return
            }
            guard !Task.isCancelled, generation == self.requestGeneration else { return }
            await self.request(query: trimmedQuery, generation: generation)
        }
    }

    /// Chooses one suggestion: the owning field fills from it, then the list
    /// closes and any pending request is dropped.
    func select(_ place: Place) {
        onSelect?(place)
        clear()
    }

    /// Empties the list and drops any pending or in-flight request.
    func clear() {
        pendingTask?.cancel()
        pendingTask = nil
        requestGeneration &+= 1
        suggestions = []
        phase = .idle
    }

    /// Runs the provider call for one current query. The generation guard is
    /// the supersede rule: an answer from an older query never assigns.
    private func request(query: String, generation: Int) async {
        do {
            let rows = try await suggest(query)
            guard generation == requestGeneration else { return }
            let cappedRows = Array(rows.prefix(maximumResults))
            suggestions = cappedRows
            phase = cappedRows.isEmpty ? .noMatches : .suggestions
        } catch is CancellationError {
            return
        } catch {
            guard generation == requestGeneration, !Task.isCancelled else { return }
            suggestions = []
            phase = .failed(failureText(error))
        }
    }

    /// The debounce duration in the sleep call's unit. The shared token is
    /// expressed in milliseconds, so the conversion is explicit.
    private static func nanoseconds(forMilliseconds milliseconds: TimeInterval) -> UInt64 {
        UInt64(max(0, milliseconds) * nanosecondsPerMillisecond)
    }

    /// Nanoseconds in one millisecond.
    private static let nanosecondsPerMillisecond: Double = 1_000_000
}

/// Which provider serves one field's type-ahead. The choice is read, never
/// named: the SDK's registration readback reports what serves, and the shared
/// registry names the keyless provider to install when nothing does.
enum SearchTypeAheadProviderChoice: Equatable {
    /// A provider already serves place search (the credentialed provider
    /// after a credential is applied, or a selection made in the settings
    /// surface).
    case alreadyServing(providerKey: String)
    /// No provider serves place search and no credential is configured: the
    /// registry's keyless descriptor is installed at its deployment address.
    case register(
        descriptor: ProviderDescriptor,
        deploymentBaseURL: String
    )
}

/// Reads the type-ahead provider from the credential state, the shared
/// registry and the SDK's own registration readback.
enum SearchTypeAheadProviderChooser {
    /// The capability name the registry's `served_contracts` list uses for
    /// place search (the contract's own spelling, never a provider's).
    static let placeSearchContractName = "place_search"

    /// Resolves the provider for the current session.
    ///
    /// - A registered place-search provider always serves: with a credential
    ///   configured that is the credentialed provider, and after a settings
    ///   selection it is the chosen one.
    /// - With no credential and no registration, the registry's first keyless
    ///   place-search descriptor is installed. The automation deployment
    ///   override replaces its public demo address when a test supplies one.
    /// - With a credential configured but no place-search registration, the
    ///   session stays unconfigured rather than silently installing the
    ///   keyless provider over the credential.
    ///
    /// - Parameters:
    ///   - descriptors: The shared registry's descriptors, in artifact order.
    ///   - appliedCredentialKeys: The provider keys with a credential applied
    ///     in this session.
    ///   - registrations: The SDK's `providerRegistrations()` readback.
    ///   - deploymentBaseURLOverride: An automation-only deployment address
    ///     that replaces the keyless descriptor's demo address.
    /// - Returns: The choice, or nil when no provider can serve type-ahead.
    static func resolve(
        descriptors: [ProviderDescriptor],
        appliedCredentialKeys: Set<String>,
        registrations: [ProviderRegistration],
        deploymentBaseURLOverride: String? = nil
    ) -> SearchTypeAheadProviderChoice? {
        if let servingRegistration = registrations.first(where: { registration in
            registration.contract == .placeSearch
                && registration.registered
                && !registration.providerKey.isEmpty
        }) {
            return .alreadyServing(providerKey: servingRegistration.providerKey)
        }
        guard appliedCredentialKeys.isEmpty else { return nil }
        guard let keylessDescriptor = descriptors.first(where: { descriptor in
            descriptor.servedContracts.contains(placeSearchContractName)
                && descriptor.registrationAvailable.ios
                && descriptor.credentialLabel.trimmingCharacters(
                    in: .whitespacesAndNewlines
                ).isEmpty
                && descriptor.hasDemoBaseURL
        }) else {
            return nil
        }
        let trimmedOverride = deploymentBaseURLOverride?.trimmingCharacters(
            in: .whitespacesAndNewlines
        )
        let deploymentBaseURL: String
        if let trimmedOverride, !trimmedOverride.isEmpty {
            deploymentBaseURL = trimmedOverride
        } else {
            deploymentBaseURL = keylessDescriptor.demoBaseURL
        }
        return .register(
            descriptor: keylessDescriptor,
            deploymentBaseURL: deploymentBaseURL
        )
    }
}
