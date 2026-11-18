//! The reference-application behavior model's unit cases: the plan fields'
//! swap action, typed-text plan resolution (assigned place → exact demo
//! place → first provider result), and the diagnostics pane's off-by-default
//! policy with its bounded, per-session debug opt-in. Every case runs the
//! application's own model methods; no case reaches the network or supplies a
//! provider credential.

import MapNativeCoreUI
import RusticMapsSDK
import XCTest

@testable import ReferenceApplication

@MainActor
final class ReferenceApplicationBehaviorTests: XCTestCase {

    /// The synthetic places the swap case assigns (no provider involved).
    private static let firstPlace = Place(
        structuredAddressText: "First place",
        coordinate: GeographicCoordinate(latitude: 38.5, longitude: -121.5),
        geocodeQuality: .point
    )
    private static let secondPlace = Place(
        structuredAddressText: "Second place",
        coordinate: GeographicCoordinate(latitude: 38.6, longitude: -121.4),
        geocodeQuality: .point
    )

    /// Whether this run supplies a runtime provider credential; the typed
    /// text case needs the keyless demo path.
    private var hasRuntimeInjectedApiKey: Bool {
        let runtimeCredential = ProcessInfo.processInfo.environment["PROVIDER_API_KEY"]?
            .trimmingCharacters(in: .whitespacesAndNewlines)
        return runtimeCredential?.isEmpty == false
    }

    // MARK: The swap action

    /// Swapping exchanges the two fields and the places resolved from them;
    /// it never plans by itself.
    func testSwapExchangesBothFieldsAndTheirResolvedPlaces() {
        let model = ReferenceApplicationModel()
        model.searchFieldAssignmentTarget = .origin
        model.assignSearchResult(Self.firstPlace)
        model.searchFieldAssignmentTarget = .destination
        model.assignSearchResult(Self.secondPlace)
        XCTAssertEqual(model.originSearchText, Self.firstPlace.structuredAddressText)
        XCTAssertEqual(
            model.destinationSearchText,
            Self.secondPlace.structuredAddressText
        )

        model.swapPlanFields()

        XCTAssertEqual(model.originSearchText, Self.secondPlace.structuredAddressText)
        XCTAssertEqual(model.destinationSearchText, Self.firstPlace.structuredAddressText)
        XCTAssertEqual(model.originCoordinate, Self.secondPlace.coordinate)
        XCTAssertEqual(model.destinationCoordinate, Self.firstPlace.coordinate)
        XCTAssertNil(model.plannedRoute, "swapping the fields never plans a route")
    }

    // MARK: Typed-text plan resolution

    /// Typed text is resolved, not rejected: Plan route resolves each field
    /// from an exact demo-place name and applies the recorded demo route,
    /// without a result tap and without a provider call.
    func testTypedTextResolvesExactDemoPlaceNamesWithoutAResultTap() async throws {
        try XCTSkipIf(
            hasRuntimeInjectedApiKey,
            "the keyless typed-text case needs a credential-less session"
        )
        let model = ReferenceApplicationModel()
        await model.startApplication()
        try XCTSkipIf(
            model.hasAppliedProviderCredential,
            "a remembered credential would route this case through the provider"
        )
        let recordedRoute = try XCTUnwrap(
            model.catalog?.demoRoutes.first,
            "the shared artifact must carry a recorded demo route"
        )

        model.updateOriginSearchText(recordedRoute.originName)
        model.updateDestinationSearchText(recordedRoute.destinationName)
        XCTAssertNil(model.originCoordinate, "typing does not assign a place")
        XCTAssertNil(model.destinationCoordinate, "typing does not assign a place")

        await model.planRouteFromFields()

        XCTAssertNotNil(
            model.plannedRoute,
            "typed demo-place names must resolve and plan without a result tap"
        )
        XCTAssertTrue(
            model.plannedRouteIsRecordedDemoRoute,
            "the keyless plan must apply the artifact's recorded route"
        )
        XCTAssertEqual(
            model.originSearchText, recordedRoute.originName,
            "the resolved field keeps the demo place's name"
        )
    }

    /// A resolved pair the artifact does not record refuses with the honest
    /// no-recorded-route line instead of inventing geometry.
    func testAResolvedPairWithoutARecordedRouteRefusesHonestly() async {
        let model = ReferenceApplicationModel()
        let unrecordedOrigin = Place(
            structuredAddressText: "Unrecorded origin",
            coordinate: GeographicCoordinate(latitude: 38.5, longitude: -121.5),
            geocodeQuality: .point
        )
        let unrecordedDestination = Place(
            structuredAddressText: "Unrecorded destination",
            coordinate: GeographicCoordinate(latitude: 38.6, longitude: -121.4),
            geocodeQuality: .point
        )
        model.searchFieldAssignmentTarget = .origin
        model.assignSearchResult(unrecordedOrigin)
        model.searchFieldAssignmentTarget = .destination
        model.assignSearchResult(unrecordedDestination)

        await model.planRouteFromFields()

        XCTAssertNil(model.plannedRoute, "no recorded pair means no planned route")
        XCTAssertEqual(
            model.errorReport?.detailText,
            "No recorded demo route from Unrecorded origin to Unrecorded destination; apply a provider key for live routing."
        )
    }

    // MARK: Diagnostics policy

    /// Diagnostics are off by default: no subscription, no cadence timer and
    /// no debug detail until the visitor opens the pane.
    func testDiagnosticsAreOffByDefault() {
        let model = ReferenceApplicationModel()

        XCTAssertFalse(model.isDiagnosticsSheetOpen)
        XCTAssertFalse(
            model.diagnosticsEventsBuffer.isRefreshCadenceActive,
            "no cadence timer may run before the pane opens"
        )
        XCTAssertFalse(
            model.diagnosticsEventsBuffer.isAcceptingDeliveries,
            "no delivery may be recorded before the pane opens"
        )
        XCTAssertFalse(model.diagnosticsShowsDebugEvents)
        XCTAssertEqual(
            model.diagnosticsVisibleMinimumSeverity,
            ReferenceApplicationConfiguration.diagnosticsEventsMinimumSeverity
        )
    }

    /// The explicit Show debug events switch lowers the pane's display
    /// minimum to Debug for the session; turning it off hides debug rows but
    /// never raises the SDK gate.
    func testShowDebugEventsLowersTheDisplayMinimumForTheSession() {
        let model = ReferenceApplicationModel()
        XCTAssertEqual(model.diagnosticsVisibleMinimumSeverity, .info)

        model.setDiagnosticsShowsDebugEvents(true)

        XCTAssertTrue(model.diagnosticsShowsDebugEvents)
        XCTAssertEqual(model.diagnosticsVisibleMinimumSeverity, .debug)

        model.setDiagnosticsShowsDebugEvents(false)

        XCTAssertFalse(model.diagnosticsShowsDebugEvents)
        XCTAssertEqual(
            model.diagnosticsVisibleMinimumSeverity, .info,
            "turning the switch off only hides debug rows; the session gate stays lowered"
        )
    }

    /// A gesture-settled write that is still waiting on its debounce is
    /// written when the scene leaves the active state, so the last camera
    /// gesture before backgrounding is not lost.
    func testLeavingTheActiveSceneFlushesAPendingGestureChoicePersist() {
        let suiteName = "pending-choice-flush-\(UUID().uuidString)"
        guard let defaults = UserDefaults(suiteName: suiteName) else {
            return XCTFail("the test defaults domain must be creatable")
        }
        defer { defaults.removePersistentDomain(forName: suiteName) }
        let model = ReferenceApplicationModel(
            rememberedChoicesStore: RememberedChoicesStore(defaults: defaults)
        )
        XCTAssertNil(
            defaults.string(forKey: RememberedChoicesStore.recordDefaultsKey),
            "the test domain starts without a remembered record"
        )

        model.schedulePersistRememberedChoicesAfterCameraGesture()
        // Backgrounding arrives before the debounce interval elapses.
        model.flushPendingChoicePersist()

        XCTAssertNotNil(
            defaults.string(forKey: RememberedChoicesStore.recordDefaultsKey),
            "the pending gesture choice is written before the scene backgrounds"
        )
    }

    /// The pane's bounded stream and opt-in values are the documented
    /// shared values: a 100-event window, a 100-event backlog flush and a
    /// 500 ms merge cadence.
    func testDiagnosticsStreamValuesAreBounded() {
        XCTAssertEqual(
            ReferenceApplicationConfiguration.diagnosticsEventWindowCount, 100
        )
        XCTAssertEqual(
            ReferenceApplicationConfiguration.diagnosticsBacklogFlushLimit, 100
        )
        XCTAssertEqual(
            ReferenceApplicationConfiguration.diagnosticsRefreshCadenceMilliseconds,
            500
        )
        XCTAssertEqual(
            ReferenceApplicationConfiguration.diagnosticsEventsMinimumSeverity,
            .info
        )
        XCTAssertEqual(
            ReferenceApplicationConfiguration.diagnosticsEventsDebugSeverity,
            .debug
        )
    }
}
