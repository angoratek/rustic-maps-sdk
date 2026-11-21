//! The readiness ladder and error report's unit tests
//! (docs/ReferenceAppUX.md §6.1, §6.3): the chip's text for every ladder
//! state, the contract variants' verbatim detail mapping, the routing
//! failure's next-step suffix, local reports and the clear act. No test
//! reaches the network or the SDK.

import RusticMapsSDK
import XCTest

@testable import ReferenceApplication

@MainActor
final class ReadinessReportStateTests: XCTestCase {

    // MARK: Ladder

    /// The state starts loading, carries the starting line, and the chip
    /// shows the loading line.
    func testDefaultsAreLoadingWithTheStartingLine() {
        let state = ReadinessReportState()

        XCTAssertEqual(state.readinessState, .loading)
        XCTAssertEqual(
            state.readinessText,
            ReferenceApplicationConfiguration.startingReadinessText
        )
        XCTAssertNil(state.errorReport)
        XCTAssertEqual(
            state.readinessChipText,
            ReferenceApplicationConfiguration.readinessChipLoadingText
        )
    }

    /// Ready, keyless, degraded and failed each render their documented chip
    /// line; degraded and failed append the live detail.
    func testTheChipRendersEachLadderState() {
        let state = ReadinessReportState()

        state.note(.ready, "Maps SDK gpu renderer active (version 9.9.9)")
        XCTAssertEqual(
            state.readinessChipText,
            ReferenceApplicationConfiguration.readinessChipReadyText
        )

        state.note(.keyless, ReferenceApplicationConfiguration.keylessReadinessText)
        XCTAssertEqual(
            state.readinessChipText,
            ReferenceApplicationConfiguration.readinessChipKeylessText
        )

        state.note(.degraded, "frame texture unavailable")
        XCTAssertEqual(
            state.readinessChipText,
            ReferenceApplicationConfiguration.readinessChipDegradedPrefix
                + "frame texture unavailable"
        )

        state.note(.failed, "surface configuration refused")
        XCTAssertEqual(state.readinessState, .failed)
        XCTAssertEqual(state.readinessText, "surface configuration refused")
        XCTAssertEqual(
            state.readinessChipText,
            ReferenceApplicationConfiguration.readinessChipFailedPrefix
                + "surface configuration refused"
        )
    }

    // MARK: Error report

    /// A contract failure reports the variant name and the verbatim detail.
    func testContractFailuresReportTheVariantNameAndDetail() {
        let state = ReadinessReportState()

        state.reportSdkError(SdkError.NetworkRequestFailed(detail: "connection reset"))
        XCTAssertEqual(
            state.errorReport,
            ReferenceApplicationErrorReport(
                labelLine: "NetworkRequestFailed",
                detailText: "connection reset"
            )
        )

        state.reportSdkError(SdkError.ProviderNotConfigured(contractName: "Routing"))
        XCTAssertEqual(
            state.errorReport?.labelLine,
            "ProviderNotConfigured"
        )
        XCTAssertEqual(
            state.errorReport?.detailText,
            ReferenceApplicationConfiguration.providerNotConfiguredDetailText(
                contractName: "Routing"
            )
        )

        state.reportSdkError(SdkError.RateLimitExhausted)
        XCTAssertEqual(state.errorReport?.labelLine, "RateLimitExhausted")
        XCTAssertEqual(
            state.errorReport?.detailText,
            "rate limit exhausted; retry after the next refill"
        )
    }

    /// The routing failure's documented next step is appended after the
    /// verbatim detail, separated by one space.
    func testASuffixFollowsTheVerbatimDetail() {
        let state = ReadinessReportState()

        state.reportSdkError(
            SdkError.NetworkRequestFailed(detail: "connection reset"),
            detailSuffix: " " + ReferenceApplicationConfiguration
                .routingUnreachableNextStepText
        )

        XCTAssertEqual(state.errorReport?.labelLine, "NetworkRequestFailed")
        XCTAssertEqual(
            state.errorReport?.detailText,
            "connection reset "
                + ReferenceApplicationConfiguration.routingUnreachableNextStepText
        )
    }

    /// A local failure names its origin and echoes the detail verbatim.
    func testLocalFailuresNameTheirOrigin() {
        let state = ReadinessReportState()

        state.reportLocalError(origin: "route planning", detail: "no route found")

        XCTAssertEqual(
            state.errorReport,
            ReferenceApplicationErrorReport(
                labelLine: "route planning",
                detailText: "no route found"
            )
        )
    }

    /// The next successful action clears the report.
    func testClearingEmptiesTheReport() {
        let state = ReadinessReportState()
        state.reportLocalError(origin: "search", detail: "provider answer unreadable")

        state.clearErrorReport()

        XCTAssertNil(state.errorReport)
    }
}
