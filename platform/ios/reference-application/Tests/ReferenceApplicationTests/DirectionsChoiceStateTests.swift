//! The Directions choice's unit tests (docs/design/DirectionsModesAndStrategy.md):
//! the Car/Fastest defaults, the capability-driven disabling with a
//! hand-written reporter, the keyless `ProviderNotConfigured` reason, the
//! remembered round trip through `RememberedChoicesStore`, and the ferry-route
//! regression — a plan call through a routing provider registered from
//! foreign carries `.automobile`, and switching to Walk carries
//! `.pedestrian`. No test reaches the network.

import MapNativeCoreUI
import RusticMapsSDK
import XCTest

@testable import ReferenceApplication

// MARK: - Fakes

/// A hand-written capability reporter: the answer the state reads.
private final class FakeRouteCapabilityReporter: RouteCapabilityReporting {
    /// The answer the fake throws or returns.
    var result: Result<RouteCapabilities, Error>

    /// The number of capability reads.
    private(set) var readCount = 0

    init(result: Result<RouteCapabilities, Error>) {
        self.result = result
    }

    func routeCapabilities() throws -> RouteCapabilities {
        readCount += 1
        return try result.get()
    }
}

/// A routing provider registered from foreign: it records every request's
/// options and answers a minimal valid route.
private final class FakeRoutingProvider: RoutingProvider, @unchecked Sendable {
    /// Every request's options, in call order.
    private let lock = NSLock()
    private var recordedOptions: [RouteOptions] = []

    var receivedOptions: [RouteOptions] {
        lock.lock()
        defer { lock.unlock() }
        return recordedOptions
    }

    func calculateRoute(
        waypoints: [GeographicCoordinate],
        routeOptions: RouteOptions
    ) async throws -> Route {
        lock.lock()
        recordedOptions.append(routeOptions)
        lock.unlock()
        return Route(
            distanceMeters: 100,
            durationSeconds: 10,
            boundingBox: BoundingBox(
                southWest: GeographicCoordinate(latitude: 0, longitude: 0),
                northEast: GeographicCoordinate(latitude: 1, longitude: 1)
            ),
            geometry: [],
            legs: []
        )
    }
}

// MARK: - Tests

@MainActor
final class DirectionsChoiceStateTests: XCTestCase {

    /// The avoidance options every test request carries.
    private static let avoidOptions = AvoidOptions(
        tollRoads: false,
        limitedAccessHighways: false,
        ferries: false,
        unpavedRoads: false
    )

    /// The two waypoints a test plan uses.
    private static let waypoints = [
        GeographicCoordinate(latitude: 38.5766, longitude: -121.4944),
        GeographicCoordinate(latitude: 38.58, longitude: -121.49),
    ]

    // MARK: Defaults

    /// Car and Fastest are the defaults, the strategy row shows, and the
    /// options a plan carries are exactly automobile/fastest.
    func testDefaultsAreCarAndFastest() {
        let state = DirectionsChoiceState(rememberedChoicesStore: makeStore())

        XCTAssertEqual(state.mode, .car)
        XCTAssertEqual(state.strategy, .fastest)
        XCTAssertTrue(state.isStrategyRowVisible)
        XCTAssertTrue(state.hasCapabilityReport == false)

        let options = state.routeOptions(
            unitSystem: .metric,
            avoidOptions: Self.avoidOptions
        )
        XCTAssertEqual(options.transportationMode, .automobile)
        XCTAssertEqual(options.routeStrategy, .fastest)
        XCTAssertEqual(options.unitSystem, .metric)
    }

    // MARK: Capability report

    /// A provider that cannot walk and cannot route shortest disables those
    /// choices with a reason; selecting a disabled choice is a no-op.
    func testCapabilityReportDisablesUnsupportedChoices() {
        let reporter = FakeRouteCapabilityReporter(
            result: .success(
                RouteCapabilities(
                    supportedTransportationModes: [.automobile],
                    supportsShortestRouteStrategy: false
                )
            )
        )
        let state = DirectionsChoiceState(rememberedChoicesStore: makeStore())
        state.attach(reporter: reporter)
        state.refreshCapabilities()

        XCTAssertTrue(state.hasCapabilityReport)
        XCTAssertNil(state.disabledReason(for: .car))
        XCTAssertEqual(
            state.disabledReason(for: .walk),
            "Walk is not offered by the serving routing provider."
        )
        XCTAssertEqual(
            state.shortestDisabledReason,
            "Shortest is not offered by the serving routing provider."
        )

        state.selectMode(.walk)
        state.selectStrategy(.shortest)
        XCTAssertEqual(state.mode, .car, "a disabled mode must not be selected")
        XCTAssertEqual(
            state.strategy, .fastest,
            "a disabled strategy must not be selected"
        )
    }

    /// A remembered choice the new provider cannot honour falls back to the
    /// provider's supported choice.
    func testUnsupportedRememberedSelectionFallsBack() {
        let store = makeStore()
        store.store(
            RememberedChoices(
                directionsMode: .walk,
                directionsStrategy: .shortest
            )
        )
        let state = DirectionsChoiceState(rememberedChoicesStore: store)
        XCTAssertEqual(state.mode, .walk)
        XCTAssertEqual(state.strategy, .shortest)

        let reporter = FakeRouteCapabilityReporter(
            result: .success(
                RouteCapabilities(
                    supportedTransportationModes: [.automobile],
                    supportsShortestRouteStrategy: false
                )
            )
        )
        state.attach(reporter: reporter)
        state.refreshCapabilities()

        XCTAssertEqual(state.mode, .car)
        XCTAssertEqual(state.strategy, .fastest)
        XCTAssertEqual(store.load()?.directionsMode, .car)
        XCTAssertEqual(store.load()?.directionsStrategy, .fastest)
    }

    /// A keyless session states the capability read's reason and keeps both
    /// choices available for the recorded demo route.
    func testProviderNotConfiguredShowsItsReason() {
        let reasonText = "no 'Routing' provider is configured"
        let reporter = FakeRouteCapabilityReporter(
            result: .failure(
                SdkError.ProviderNotConfigured(contractName: "Routing")
            )
        )
        let state = DirectionsChoiceState(
            rememberedChoicesStore: makeStore(),
            failureText: { _ in reasonText }
        )
        state.attach(reporter: reporter)
        state.refreshCapabilities()

        XCTAssertFalse(state.hasCapabilityReport)
        XCTAssertEqual(state.capabilityReasonText, reasonText)
        XCTAssertNil(state.disabledReason(for: .walk))
        XCTAssertNil(state.shortestDisabledReason)
        XCTAssertEqual(state.mode, .car)
        XCTAssertEqual(state.strategy, .fastest)
    }

    // MARK: Remembered choices

    /// Both choices ride the versioned record under the documented keys; a
    /// missing or unknown value falls back to the defaults.
    func testRememberedChoicesRoundTripThroughTheStore() throws {
        let defaults = makeDefaults()
        let store = RememberedChoicesStore(defaults: defaults)
        let state = DirectionsChoiceState(rememberedChoicesStore: store)
        XCTAssertEqual(state.mode, .car)
        XCTAssertEqual(state.strategy, .fastest)

        state.selectMode(.walk)
        state.selectStrategy(.shortest)

        let recordText = try XCTUnwrap(
            defaults.string(forKey: RememberedChoicesStore.recordDefaultsKey)
        )
        XCTAssertTrue(
            recordText.contains("\"directionsTransportationMode\":\"pedestrian\""),
            "the record must carry the contract spelling, got \(recordText)"
        )
        XCTAssertTrue(
            recordText.contains("\"directionsRouteStrategy\":\"shortest\""),
            "the record must carry the contract spelling, got \(recordText)"
        )

        let restored = DirectionsChoiceState(rememberedChoicesStore: store)
        XCTAssertEqual(restored.mode, .walk)
        XCTAssertEqual(restored.strategy, .shortest)
        XCTAssertFalse(restored.isStrategyRowVisible)

        // A record from before the fields existed (and an unknown value)
        // decodes to the defaults.
        let legacyDefaults = makeDefaults()
        legacyDefaults.set(
            """
            {"version":1,"theme":"auto","scenes":[],"search":{"origin":"",\
            "destination":"","query":""}}
            """,
            forKey: RememberedChoicesStore.recordDefaultsKey
        )
        let legacy = DirectionsChoiceState(
            rememberedChoicesStore: RememberedChoicesStore(
                defaults: legacyDefaults
            )
        )
        XCTAssertEqual(legacy.mode, .car)
        XCTAssertEqual(legacy.strategy, .fastest)

        legacyDefaults.set(
            """
            {"version":1,"directionsTransportationMode":"hovercraft",\
            "directionsRouteStrategy":"scenic"}
            """,
            forKey: RememberedChoicesStore.recordDefaultsKey
        )
        let unknown = DirectionsChoiceState(
            rememberedChoicesStore: RememberedChoicesStore(
                defaults: legacyDefaults
            )
        )
        XCTAssertEqual(unknown.mode, .car)
        XCTAssertEqual(unknown.strategy, .fastest)
    }

    // MARK: The ferry-route regression

    /// The plan call through a routing provider registered from foreign
    /// carries `.automobile` by default and `.pedestrian` after Walk is
    /// chosen (the ferry regression: a car plan must ask for the driving
    /// profile, not the walking one).
    func testPlanCallCarriesTheSelectedModeToTheProvider() async throws {
        let sdk = MapSdk()
        let provider = FakeRoutingProvider()
        sdk.registerRoutingProviderFromForeign(routingProvider: provider)
        let state = DirectionsChoiceState(rememberedChoicesStore: makeStore())

        _ = try await sdk.calculateRoute(
            waypoints: Self.waypoints,
            routeOptions: state.routeOptions(
                unitSystem: .metric,
                avoidOptions: Self.avoidOptions
            )
        )
        XCTAssertEqual(provider.receivedOptions.count, 1)
        XCTAssertEqual(
            provider.receivedOptions.first?.transportationMode,
            .automobile,
            "the default plan must ask for automobile routing"
        )
        XCTAssertEqual(
            provider.receivedOptions.first?.routeStrategy,
            .fastest
        )

        state.selectMode(.walk)
        _ = try await sdk.calculateRoute(
            waypoints: Self.waypoints,
            routeOptions: state.routeOptions(
                unitSystem: .metric,
                avoidOptions: Self.avoidOptions
            )
        )
        XCTAssertEqual(
            provider.receivedOptions.last?.transportationMode,
            .pedestrian,
            "selecting Walk must change the request to pedestrian routing"
        )
    }

    // MARK: Helpers

    /// A store over an isolated defaults suite.
    private func makeStore() -> RememberedChoicesStore {
        RememberedChoicesStore(defaults: makeDefaults())
    }

    /// An isolated defaults domain, cleared before use and removed at
    /// teardown.
    private func makeDefaults() -> UserDefaults {
        let suiteName = "DirectionsChoiceStateTests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suiteName)!
        defaults.removePersistentDomain(forName: suiteName)
        addTeardownBlock {
            defaults.removePersistentDomain(forName: suiteName)
        }
        return defaults
    }
}
