//! The readiness ladder and the provider-echoing error report
//! (docs/ReferenceAppUX.md §6.1, §6.3): the states the readiness chip
//! renders, the full diagnostic text behind a degraded or failed state, and
//! the error report the screen shows until the next successful action clears
//! it. The state type owns every transition and the contract-error wording;
//! the application only feeds it observed events.

import Combine
import Foundation
import RusticMapsSDK

/// The readiness ladder's states, exactly the §6.1 table plus the neutral
/// keyless state of §6.5: with no provider credential the application has
/// not failed — it waits for a credential instead.
enum ReadinessState: String, Equatable {
    /// SDK module/provider registration still resolving.
    case loading
    /// The GPU surface renderer is active.
    case ready
    /// A documented capability substitute fired (none exists on this
    /// native path today; the state exists per the ladder).
    case degraded
    /// No provider credential is configured; the settings surface waits for
    /// one (§6.5: a first-class state, never a renderer failure).
    case keyless
    /// The SDK failed to start; the report names the failure verbatim.
    case failed
}

/// One reported failure in the error report (§6.3): the provider-echoing
/// label line (the `SdkError` variant name when the failure crossed the
/// contract, the failure origin otherwise) and the verbatim detail.
struct ReferenceApplicationErrorReport: Equatable {
    /// The label line (variant name, or severity + origin).
    let labelLine: String
    /// The failure detail echoed verbatim — never paraphrased, truncated or
    /// swallowed.
    let detailText: String
}

/// The readiness ladder and the error report as one state type
/// (docs/ReferenceAppUX.md §6.1, §6.3).
///
/// The type owns the ladder's current state, the live text the diagnostics
/// sheet states, and the error report the screen renders until the next
/// successful action clears it. The keyless chip prompt replaces the loading
/// line while no credential is in force (§6.5). No method reaches the network
/// or the SDK: callers hand in the failure they already observed.
@MainActor
final class ReadinessReportState: ObservableObject {
    /// The ladder's current state.
    @Published private(set) var readinessState: ReadinessState = .loading

    /// The full readiness line the diagnostics sheet states.
    @Published private(set) var readinessText: String =
        ReferenceApplicationConfiguration.startingReadinessText

    /// The provider-echoing error report, visible until the next successful
    /// action clears it (§6.3).
    @Published private(set) var errorReport: ReferenceApplicationErrorReport?

    /// The readiness chip's short line (§4.2: the four states; the full
    /// readiness text lives in the diagnostics sheet).
    var readinessChipText: String {
        switch readinessState {
        case .loading:
            return ReferenceApplicationConfiguration.readinessChipLoadingText
        case .ready:
            return ReferenceApplicationConfiguration.readinessChipReadyText
        case .degraded:
            return ReferenceApplicationConfiguration.readinessChipDegradedPrefix
                + readinessText
        case .keyless:
            return ReferenceApplicationConfiguration.readinessChipKeylessText
        case .failed:
            return ReferenceApplicationConfiguration.readinessChipFailedPrefix
                + readinessText
        }
    }

    /// Applies one ladder transition with its live text.
    func note(_ state: ReadinessState, _ text: String) {
        readinessState = state
        readinessText = text
    }

    /// Labels the contract's error variants by their own name (the label
    /// line of §6.3: "the error block's label line shows the variant name
    /// when one is available").
    func sdkErrorVariantName(_ sdkError: SdkError) -> String {
        switch sdkError {
        case .InvalidCoordinateInput: return "InvalidCoordinateInput"
        case .NetworkRequestFailed: return "NetworkRequestFailed"
        case .ResponseParsingFailure: return "ResponseParsingFailure"
        case .ProviderRequestRejected: return "ProviderRequestRejected"
        case .RateLimitExhausted: return "RateLimitExhausted"
        case .ProviderNotConfigured: return "ProviderNotConfigured"
        case .InvalidNavigationInput: return "InvalidNavigationInput"
        case .InvalidRenderInput: return "InvalidRenderInput"
        case .RenderingSurfaceUnavailable: return "RenderingSurfaceUnavailable"
        case .CacheOperationFailure: return "CacheOperationFailure"
        case .OfflineRoutingFailure: return "OfflineRoutingFailure"
        }
    }

    /// The verbatim detail text of a contract error (the detail string the
    /// SDK contract carries across the binding unchanged). The two sentences
    /// the contract does not carry as a detail string are the shared
    /// reference-tokens artifact's `strings.rate_limit_exhausted_detail` and
    /// `strings.provider_not_configured_detail_template`
    /// (platform/reference-tokens.json); scripts/check_ux_parity.mjs gates the
    /// text against that artifact.
    func sdkFailureDetailText(_ sdkError: SdkError) -> String {
        switch sdkError {
        case .InvalidCoordinateInput(let detail): return detail
        case .NetworkRequestFailed(let detail): return detail
        case .ResponseParsingFailure(let detail): return detail
        case .ProviderRequestRejected(let detail): return detail
        // platform/reference-tokens.json strings.rate_limit_exhausted_detail
        case .RateLimitExhausted: return "rate limit exhausted; retry after the next refill"
        // platform/reference-tokens.json strings.provider_not_configured_detail_template
        case .ProviderNotConfigured(let contractName):
            return ReferenceApplicationConfiguration.providerNotConfiguredDetailText(
                contractName: contractName
            )
        case .InvalidNavigationInput(let detail): return detail
        case .InvalidRenderInput(let detail): return detail
        case .RenderingSurfaceUnavailable(let detail): return detail
        // The cache and offline variants carry their own verbatim reason.
        case .CacheOperationFailure(let detail): return detail
        case .OfflineRoutingFailure(let detail): return detail
        }
    }

    /// Echoes a contract failure into the error report, appending any
    /// caller-supplied next-step sentence to the verbatim detail.
    func reportSdkError(_ sdkError: SdkError, detailSuffix: String = "") {
        errorReport = ReferenceApplicationErrorReport(
            labelLine: sdkErrorVariantName(sdkError),
            detailText: sdkFailureDetailText(sdkError) + detailSuffix
        )
    }

    /// Echoes a local (non-contract) failure into the error report: the
    /// label line names the origin, the detail echoes verbatim.
    func reportLocalError(origin: String, detail: String) {
        errorReport = ReferenceApplicationErrorReport(
            labelLine: origin,
            detailText: detail
        )
    }

    /// Clears the error report on the next successful action (§6.3).
    func clearErrorReport() {
        errorReport = nil
    }
}
