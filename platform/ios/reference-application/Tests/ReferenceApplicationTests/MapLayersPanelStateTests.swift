//! The on-map layer panel's unit tests: the base rows' order and disabled
//! states, the traffic layer's debounce, one-request-in-flight cap,
//! clear-on-off and failure semantics (through a traffic-flow provider
//! registered on a real `MapSdk` from foreign), the device-location row
//! states, the route fields' "My location" chips and the remembered traffic
//! round trip. Everything is a hand-written fake: no test reaches the
//! network.

import MapNativeCoreUI
import RusticMapsSDK
import XCTest

@testable import ReferenceApplication

// MARK: - Fakes

/// A manual debounce scheduler: the test decides when a scheduled action
/// runs, so the debounce is asserted without waiting.
@MainActor
private final class ManualDebounceScheduler: MapLayersDebounceScheduling {
    /// Every delay the state asked for, in call order.
    private(set) var scheduledDelays: [Double] = []

    /// The actions still waiting to run.
    private var pendingActions: [ManualScheduledAction] = []

    /// The number of actions still waiting.
    var pendingCount: Int { pendingActions.count }

    nonisolated init() {}

    func schedule(
        afterSeconds seconds: Double,
        action: @escaping @MainActor () -> Void
    ) -> MapLayersScheduledAction {
        scheduledDelays.append(seconds)
        let scheduledAction = ManualScheduledAction(action: action)
        pendingActions.append(scheduledAction)
        return scheduledAction
    }

    /// Runs every action scheduled so far, in order.
    func runAllPending() {
        let actionsToRun = pendingActions
        pendingActions.removeAll()
        for action in actionsToRun {
            action.runIfScheduled()
        }
    }
}

/// One manually run action.
@MainActor
private final class ManualScheduledAction: MapLayersScheduledAction {
    private let action: @MainActor () -> Void
    private var isCancelled = false

    init(action: @escaping @MainActor () -> Void) {
        self.action = action
    }

    func cancel() {
        isCancelled = true
    }

    /// Runs the action unless it was cancelled.
    func runIfScheduled() {
        guard !isCancelled else { return }
        isCancelled = true
        action()
    }
}

/// A gated traffic fetcher: it suspends every call until the test releases
/// it, so the one-request-in-flight rule is observable.
@MainActor
private final class GatedTrafficFetcher {
    /// The next result every released call answers with.
    var result: Result<[TrafficFlowData], Error> =
        .success([MapLayersPanelStateTests.sampleFlowRow])

    /// Whether calls suspend until released.
    var isGated = false

    /// The number of calls the fetcher received.
    private(set) var requestCount = 0

    /// The suspended calls' continuations, oldest first.
    private var continuations: [CheckedContinuation<[TrafficFlowData], Error>] = []

    nonisolated init() {}

    func fetch(_ boundingBox: BoundingBox) async throws -> [TrafficFlowData] {
        requestCount += 1
        guard isGated else {
            return try result.get()
        }
        return try await withCheckedThrowingContinuation { continuation in
            continuations.append(continuation)
        }
    }

    /// Releases the oldest suspended call with the configured result.
    func releaseNext() {
        guard !continuations.isEmpty else { return }
        let continuation = continuations.removeFirst()
        continuation.resume(with: result)
    }
}

/// A traffic-flow provider registered from foreign: the test proves the
/// state drives the real contract call rather than a mocked closure.
private final class FakeTrafficFlowProvider: TrafficFlowProvider, @unchecked Sendable {
    /// The answers, consumed in call order; the last answer repeats so a
    /// refreshed call never runs out.
    private var results: [Result<[TrafficFlowData], Error>]

    /// The call count, guarded because the SDK calls from its own queue.
    private let lock = NSLock()
    private var callCount = 0

    /// Builds a provider that answers every call with `rows`.
    convenience init(rows: [TrafficFlowData], thrownError: Error? = nil) {
        let answer: Result<[TrafficFlowData], Error> = thrownError
            .map { .failure($0) } ?? .success(rows)
        self.init(results: [answer])
    }

    /// Builds a provider from an explicit answer sequence.
    init(results: [Result<[TrafficFlowData], Error>]) {
        self.results = results
    }

    var calls: Int {
        lock.lock()
        defer { lock.unlock() }
        return callCount
    }

    func fetchTrafficFlow(boundingBox: BoundingBox) async throws -> [TrafficFlowData] {
        lock.lock()
        callCount += 1
        let result = results.count > 1 ? results.removeFirst() : results[0]
        lock.unlock()
        return try result.get()
    }
}

// MARK: - The tests

@MainActor
final class MapLayersPanelStateTests: XCTestCase {

    // MARK: Named constants

    /// The capability name the traffic descriptor declares (the shared
    /// registry's spelling).
    private static let trafficFlowCapabilityName = "traffic_flow"

    /// A synthetic credential label; never a real credential.
    private static let apiKeyCredentialLabel = "API key"

    /// The sample camera bounds every fake camera seam answers with.
    private static let sampleBounds = BoundingBox(
        southWest: GeographicCoordinate(latitude: 38.5, longitude: -121.6),
        northEast: GeographicCoordinate(latitude: 38.6, longitude: -121.4)
    )

    /// The sample device fix.
    private static let sampleFixCoordinate = GeographicCoordinate(
        latitude: 38.5766,
        longitude: -121.4944
    )

    /// The sample fix's display text (the chip writes it into a field).
    private static let sampleFixDisplayText = "38.57660, -121.49440"

    /// One traffic-flow row the fakes answer with.
    fileprivate static let sampleFlowRow = TrafficFlowData(
        shapeCoordinates: [
            GeographicCoordinate(latitude: 38.57, longitude: -121.49),
            GeographicCoordinate(latitude: 38.58, longitude: -121.49),
        ],
        currentSpeedKilometersPerHour: 30,
        freeFlowSpeedKilometersPerHour: 60,
        congestionLevel: .slow
    )

    // MARK: Base rows

    /// Raster descriptors come first, then every committed vector style; a
    /// credentialed descriptor without its key is disabled with the
    /// registry's own wording.
    func testBaseRowsAreOrderedAndStateTheirDisabledReason() throws {
        let keylessDescriptor = try descriptor(
            providerKey: "keyless-base",
            displayName: "Keyless Base",
            credentialLabel: "",
            servedContracts: ["raster_tiles"],
            rasterUrlTemplate: "https://fixtures.example/{z}/{x}/{y}.png"
        )
        let credentialedDescriptor = try descriptor(
            providerKey: "credentialed-base",
            displayName: "Credentialed Base",
            credentialLabel: Self.apiKeyCredentialLabel,
            servedContracts: ["raster_tiles"],
            rasterUrlTemplate: "https://fixtures.example/{z}/{x}/{y}.png"
        )
        let vectorDescriptor = try descriptor(
            providerKey: "vector-base",
            displayName: "Vector Base",
            credentialLabel: "",
            servedContracts: ["vector_tiles"],
            vectorUrlTemplate: "https://fixtures.example/{z}/{x}/{y}.mvt",
            vectorStyles: [
                styleDescriptor(styleId: "bright-light", displayName: "Bright"),
                styleDescriptor(styleId: "positron", displayName: "Positron"),
            ]
        )
        let rasterChoices = [
            BaseMapChoice(descriptor: keylessDescriptor, isServable: true),
            BaseMapChoice(descriptor: credentialedDescriptor, isServable: false),
        ]
        let vectorChoices = vectorDescriptor.vectorStyles.map { style in
            VectorBaseMapChoice(descriptor: vectorDescriptor, style: style)
        }
        let state = makeState()

        state.refreshBaseRows(
            rasterChoices: rasterChoices,
            vectorChoices: vectorChoices,
            activeRasterProviderKey: "keyless-base",
            activeVectorChoiceKey: nil
        )

        XCTAssertEqual(
            state.baseRows.map(\.identifier),
            [
                "map-layer-base-keyless-base",
                "map-layer-base-credentialed-base",
                "map-layer-style-bright-light",
                "map-layer-style-positron",
            ],
            "the rows must be raster descriptors first, then every vector style"
        )
        XCTAssertTrue(
            state.baseRows[0].isServing,
            "the active raster provider's row carries the Serving mark"
        )
        XCTAssertNil(state.baseRows[0].disabledReasonText)
        XCTAssertEqual(
            state.baseRows[1].disabledReasonText,
            "Needs \(Self.apiKeyCredentialLabel)",
            "a credentialed descriptor without its key says what it needs"
        )
        XCTAssertFalse(state.baseRows[1].isServing)

        state.refreshBaseRows(
            rasterChoices: rasterChoices,
            vectorChoices: vectorChoices,
            activeRasterProviderKey: nil,
            activeVectorChoiceKey: "vector-base/positron"
        )
        XCTAssertFalse(state.baseRows[0].isServing)
        XCTAssertTrue(state.baseRows[3].isServing)
    }

    /// A provider-published vector family is listed without its key and
    /// states what it needs, exactly as a credentialed raster row does.
    func testAHostedVectorRowWithoutItsKeyStatesWhatItNeeds() throws {
        let hostedDescriptor = try descriptor(
            providerKey: "hosted-vector-base",
            displayName: "Hosted Vector Base",
            credentialLabel: Self.apiKeyCredentialLabel,
            servedContracts: ["vector_tiles"],
            vectorStyles: [styleDescriptor(styleId: "streets-light", displayName: "Streets")]
        )
        let hostedChoice = VectorBaseMapChoice(
            descriptor: hostedDescriptor,
            style: hostedDescriptor.vectorStyles[0],
            isServable: false
        )
        let state = makeState()
        state.refreshBaseRows(
            rasterChoices: [],
            vectorChoices: [hostedChoice],
            activeRasterProviderKey: nil,
            activeVectorChoiceKey: nil
        )
        let hostedRow = try XCTUnwrap(state.baseRows.first)
        XCTAssertEqual(hostedRow.identifier, "map-layer-style-streets-light")
        XCTAssertEqual(
            hostedRow.disabledReasonText,
            "Needs \(Self.apiKeyCredentialLabel)"
        )
    }

    // MARK: Traffic availability

    /// The switch reads the shared registry: available with a descriptor
    /// and its credential, `Needs <label>` without the credential, and
    /// `Not configured` when nothing declares the capability.
    func testTrafficAvailabilityFollowsTheRegistry() throws {
        let state = makeState()
        state.updateTrafficConfiguration(
            providerDescriptors: [],
            appliedProviderKeys: []
        )
        XCTAssertEqual(
            state.trafficAvailability,
            .notConfigured(reasonText: "Not configured")
        )

        let descriptor = try trafficDescriptor(
            credentialLabel: Self.apiKeyCredentialLabel
        )
        state.updateTrafficConfiguration(
            providerDescriptors: [descriptor],
            appliedProviderKeys: []
        )
        XCTAssertEqual(
            state.trafficAvailability,
            .needsCredential(
                reasonText: "Needs \(Self.apiKeyCredentialLabel)"
            )
        )
        XCTAssertFalse(state.trafficAvailability.isAvailable)

        state.updateTrafficConfiguration(
            providerDescriptors: [descriptor],
            appliedProviderKeys: [descriptor.providerKey]
        )
        XCTAssertTrue(state.trafficAvailability.isAvailable)
        XCTAssertEqual(
            state.trafficAvailability.attributionText,
            descriptor.attributionText
        )
    }

    // MARK: Traffic debounce, in-flight cap and clear-on-off

    /// One camera settle yields one request after the debounce; installing
    /// the overlay counts once.
    func testTrafficRefreshesOncePerSettledCamera() async {
        let scheduler = ManualDebounceScheduler()
        let fetcher = GatedTrafficFetcher()
        let recorder = TrafficOverlayRecorder()
        let state = makeState(
            scheduler: scheduler,
            fetcher: fetcher,
            overlayRecorder: recorder,
            appliedProviderKeys: ["traffic-provider"]
        )

        state.setTrafficEnabled(true)
        XCTAssertEqual(
            scheduler.scheduledDelays,
            [MapLayersPanelState.trafficRefreshDebounceSeconds]
        )
        XCTAssertEqual(state.trafficFetchCount, 0, "the debounce must hold the call")

        scheduler.runAllPending()
        await settleAsyncTasks()

        XCTAssertEqual(state.trafficFetchCount, 1)
        XCTAssertEqual(state.trafficOverlayInstallCount, 1)
        XCTAssertEqual(recorder.installedSegments.count, 1)
        XCTAssertEqual(
            recorder.installedSegments.first?.count,
            1,
            "the capability's rows are the overlay's segments"
        )
    }

    /// A camera change while a request is in flight is coalesced: the
    /// second request waits for the first to finish.
    func testTrafficAllowsOneRequestInFlight() async {
        let scheduler = ManualDebounceScheduler()
        let fetcher = GatedTrafficFetcher()
        fetcher.isGated = true
        let recorder = TrafficOverlayRecorder()
        let state = makeState(
            scheduler: scheduler,
            fetcher: fetcher,
            overlayRecorder: recorder,
            appliedProviderKeys: ["traffic-provider"]
        )

        state.setTrafficEnabled(true)
        scheduler.runAllPending()
        await settleAsyncTasks()
        XCTAssertEqual(state.trafficFetchCount, 1)
        XCTAssertTrue(state.isTrafficRefreshing)

        state.noteCameraPoseChanged(poseKey: "pose-b")
        scheduler.runAllPending()
        await settleAsyncTasks()
        XCTAssertEqual(
            state.trafficFetchCount,
            1,
            "a second request must wait for the one in flight"
        )

        fetcher.releaseNext()
        await settleAsyncTasks()
        XCTAssertEqual(state.trafficOverlayInstallCount, 1)
        XCTAssertFalse(state.isTrafficRefreshing)
        XCTAssertEqual(
            scheduler.pendingCount,
            1,
            "the coalesced change schedules one follow-up"
        )

        scheduler.runAllPending()
        await settleAsyncTasks()
        XCTAssertEqual(state.trafficFetchCount, 2)
        fetcher.releaseNext()
        await settleAsyncTasks()
        XCTAssertEqual(state.trafficOverlayInstallCount, 2)
    }

    /// Turning the layer off clears its overlay and drops an answer that
    /// arrives while the request was in flight.
    func testTurningTrafficOffClearsTheOverlayAndDropsTheAnswer() async {
        let scheduler = ManualDebounceScheduler()
        let fetcher = GatedTrafficFetcher()
        fetcher.isGated = true
        let recorder = TrafficOverlayRecorder()
        let state = makeState(
            scheduler: scheduler,
            fetcher: fetcher,
            overlayRecorder: recorder,
            appliedProviderKeys: ["traffic-provider"]
        )

        state.setTrafficEnabled(true)
        scheduler.runAllPending()
        await settleAsyncTasks()
        XCTAssertEqual(state.trafficFetchCount, 1)

        state.setTrafficEnabled(false)
        XCTAssertTrue(recorder.installedSegments.isEmpty)
        XCTAssertNil(state.statusLineText)

        fetcher.releaseNext()
        await settleAsyncTasks()
        XCTAssertEqual(
            state.trafficOverlayInstallCount,
            0,
            "a toggle-off drops the in-flight answer"
        )
        XCTAssertTrue(recorder.installedSegments.isEmpty)
    }

    /// A failed refresh keeps the previous overlay and states the failure
    /// verbatim; the switch stays on. The failing answer comes from a
    /// traffic-flow provider registered on the SDK from foreign.
    func testAFailedRefreshKeepsThePreviousOverlay() async {
        let failureText = "the flow service refused the box"
        let sdk = MapSdk()
        let provider = FakeTrafficFlowProvider(
            results: [
                .success([Self.sampleFlowRow]),
                .failure(SdkError.NetworkRequestFailed(detail: failureText)),
            ]
        )
        sdk.registerTrafficFlowProviderFromForeign(trafficFlowProvider: provider)
        let scheduler = ManualDebounceScheduler()
        let recorder = TrafficOverlayRecorder()
        let state = makeState(
            scheduler: scheduler,
            fetchTrafficFlow: { boundingBox in
                try await sdk.fetchTrafficFlow(boundingBox: boundingBox)
            },
            overlayRecorder: recorder,
            appliedProviderKeys: ["traffic-provider"]
        )

        state.setTrafficEnabled(true)
        scheduler.runAllPending()
        let didInstall = await awaitUntil { state.trafficOverlayInstallCount == 1 }
        XCTAssertTrue(didInstall)

        state.noteCameraPoseChanged(poseKey: "pose-b")
        scheduler.runAllPending()
        let didFail = await awaitUntil {
            state.statusLineText?.contains(failureText) == true
        }
        XCTAssertTrue(didFail)

        XCTAssertEqual(
            state.trafficOverlayInstallCount,
            1,
            "a failed refresh keeps the previous overlay in force"
        )
        XCTAssertEqual(recorder.installedSegments.count, 1)
        XCTAssertTrue(state.isTrafficEnabled)
    }

    /// A fake traffic-flow provider registered from foreign serves the real
    /// capability call; a `ProviderNotConfigured` failure turns the switch
    /// off with its reason.
    func testFakeProviderRegisteredFromForeignDrivesTheLayer() async {
        let sdk = MapSdk()
        let provider = FakeTrafficFlowProvider(rows: [Self.sampleFlowRow])
        sdk.registerTrafficFlowProviderFromForeign(trafficFlowProvider: provider)
        let scheduler = ManualDebounceScheduler()
        let recorder = TrafficOverlayRecorder()
        let state = makeState(
            scheduler: scheduler,
            fetchTrafficFlow: { boundingBox in
                try await sdk.fetchTrafficFlow(boundingBox: boundingBox)
            },
            overlayRecorder: recorder,
            appliedProviderKeys: ["traffic-provider"]
        )

        state.setTrafficEnabled(true)
        scheduler.runAllPending()
        let didCallProvider = await awaitUntil { provider.calls == 1 }
        XCTAssertTrue(
            didCallProvider,
            "the registered fake provider must serve the capability call"
        )
        let didInstallOverlay = await awaitUntil { state.trafficOverlayInstallCount == 1 }
        XCTAssertTrue(
            didInstallOverlay,
            "the fake provider's rows must install one overlay"
        )
        XCTAssertEqual(recorder.installedSegments.first?.count, 1)
    }

    /// The contract's missing-provider variant turns the switch off with its
    /// own reason instead of pretending the layer is drawing.
    func testProviderNotConfiguredTurnsTheSwitchOffWithItsReason() async {
        let sdk = MapSdk()
        let provider = FakeTrafficFlowProvider(
            rows: [],
            thrownError: SdkError.ProviderNotConfigured(
                contractName: "TrafficFlow"
            )
        )
        sdk.registerTrafficFlowProviderFromForeign(trafficFlowProvider: provider)
        let scheduler = ManualDebounceScheduler()
        let state = makeState(
            scheduler: scheduler,
            fetchTrafficFlow: { boundingBox in
                try await sdk.fetchTrafficFlow(boundingBox: boundingBox)
            },
            overlayRecorder: TrafficOverlayRecorder(),
            appliedProviderKeys: ["traffic-provider"]
        )

        state.setTrafficEnabled(true)
        scheduler.runAllPending()
        let didTurnOff = await awaitUntil { !state.isTrafficEnabled }
        XCTAssertTrue(
            didTurnOff,
            "ProviderNotConfigured must turn the switch off, status '\(state.statusLineText ?? "nil")', refreshing \(state.isTrafficRefreshing), calls \(provider.calls)"
        )
        XCTAssertTrue(
            state.statusLineText?.contains("ProviderNotConfigured") == true,
            "the reason must be the contract's own text, got '\(state.statusLineText ?? "nil")'"
        )
    }

    // MARK: Device location states

    /// The row's five states render the specification's wording and only
    /// the permission denial and unavailability disable the chips.
    func testDeviceLocationStatesRenderTheirWording() {
        let state = makeState()
        XCTAssertEqual(state.locationStatus, .off)
        XCTAssertEqual(state.locationStatus.displayText, "Off")
        XCTAssertFalse(state.locationStatus.isSourceRunning)
        XCTAssertNil(state.myLocationChipDisabledReason)

        state.setLocationStatus(.waitingForFix)
        XCTAssertEqual(state.locationStatus.displayText, "Waiting for a fix")
        XCTAssertTrue(state.locationStatus.isSourceRunning)

        state.setLocationStatus(.active)
        XCTAssertEqual(state.locationStatus.displayText, "Active")

        state.setLocationStatus(.permissionDenied)
        XCTAssertEqual(state.locationStatus.displayText, "Permission denied")
        XCTAssertEqual(state.myLocationChipDisabledReason, "Permission denied")
        XCTAssertEqual(state.statusLineText, "Permission denied")

        state.setLocationStatus(.unavailable)
        XCTAssertEqual(state.locationStatus.displayText, "Unavailable")
        XCTAssertEqual(state.myLocationChipDisabledReason, "Unavailable")

        state.setLocationStatus(.off)
        XCTAssertEqual(state.statusLineText, nil)
    }

    // MARK: My-location chips

    /// A chip starts the source when it is off and fills only its own field
    /// once a fix arrives; the other field stays empty.
    func testAChipStartsTheSourceAndFillsOnlyItsOwnField() {
        var startCount = 0
        let fillRecorder = RouteFieldFillRecorder()
        let state = makeState(
            startDeviceLocation: {
                startCount += 1
                return .started
            },
            fillRouteField: { field, displayText in
                fillRecorder.record(field, displayText)
            }
        )

        state.myLocationChipTapped(for: .origin)
        XCTAssertEqual(startCount, 1)
        XCTAssertEqual(state.locationStatus, .waitingForFix)
        XCTAssertTrue(state.pendingChipFields.contains(.origin))
        XCTAssertTrue(fillRecorder.fieldTargets.isEmpty)

        state.noteDeviceFix(coordinate: Self.sampleFixCoordinate)
        XCTAssertEqual(fillRecorder.fieldTargets, [.origin])
        XCTAssertEqual(fillRecorder.displayTexts, [Self.sampleFixDisplayText])
        XCTAssertTrue(state.hasMyLocationFill(for: .origin))
        XCTAssertFalse(state.hasMyLocationFill(for: .destination))
        XCTAssertEqual(
            state.myLocationChipLabel(for: .origin),
            "My location \(MapLayersPanelState.myLocationChipFilledMark)"
        )
    }

    /// A chip tapped while a fix is already known fills its field
    /// immediately, and a denied source disables it with the reason.
    func testAChipFillsFromTheLatestFixAndRespectsADenial() {
        let fillRecorder = RouteFieldFillRecorder()
        var startCount = 0
        let state = makeState(
            startDeviceLocation: {
                startCount += 1
                return .started
            },
            fillRouteField: { field, displayText in
                fillRecorder.record(field, displayText)
            }
        )
        state.noteDeviceFix(coordinate: Self.sampleFixCoordinate)

        state.setLocationStatus(.permissionDenied)
        state.myLocationChipTapped(for: .destination)
        XCTAssertEqual(
            startCount,
            0,
            "a denied source must not start the source again from a chip"
        )
        XCTAssertTrue(fillRecorder.fieldTargets.isEmpty)
        XCTAssertEqual(state.statusLineText, "Permission denied")

        state.setLocationStatus(.active)
        state.myLocationChipTapped(for: .destination)
        XCTAssertEqual(fillRecorder.fieldTargets, [.destination])
        XCTAssertEqual(fillRecorder.displayTexts, [Self.sampleFixDisplayText])
    }

    // MARK: Remembered traffic

    /// The traffic choice rides the versioned record; a record written
    /// before the field existed decodes to off.
    func testRememberedTrafficRoundTripsThroughTheStore() throws {
        let store = makeStore()
        XCTAssertNil(store.load())

        store.store(RememberedChoices(trafficLayerEnabled: true))
        XCTAssertEqual(try XCTUnwrap(store.load()).trafficLayerEnabled, true)

        store.store(RememberedChoices(trafficLayerEnabled: false))
        XCTAssertEqual(try XCTUnwrap(store.load()).trafficLayerEnabled, false)

        let legacyDefaults = makeDefaults()
        let legacyStore = RememberedChoicesStore(defaults: legacyDefaults)
        legacyDefaults.set(
            """
            {"version":1,"theme":"auto","scenes":[],"search":{"origin":"",\
            "destination":"","query":""}}
            """,
            forKey: RememberedChoicesStore.recordDefaultsKey
        )
        XCTAssertEqual(
            try XCTUnwrap(legacyStore.load()).trafficLayerEnabled,
            false,
            "a record without the field must decode to off"
        )
    }

    // MARK: Helpers

    /// Waits for the state's main-actor tasks to run their immediate work.
    private func settleAsyncTasks() async {
        for _ in 0..<4 {
            await Task.yield()
        }
        try? await Task.sleep(nanoseconds: 20_000_000)
        for _ in 0..<4 {
            await Task.yield()
        }
    }

    /// Polls a main-actor condition until it holds or the timeout expires.
    private func awaitUntil(
        timeoutSeconds: Double = 5,
        _ condition: @MainActor () -> Bool
    ) async -> Bool {
        let deadline = Date().addingTimeInterval(timeoutSeconds)
        while Date() < deadline {
            if condition() {
                return true
            }
            try? await Task.sleep(nanoseconds: 20_000_000)
        }
        return condition()
    }

    /// The plan fields' swap exchanges the chips' fill marks, so a filled
    /// origin follows its text into the destination.
    func testSwapExchangesTheChipsFillMarks() {
        let state = makeState()
        state.myLocationChipTapped(for: .origin)
        state.noteDeviceFix(coordinate: Self.sampleFixCoordinate)
        XCTAssertTrue(state.hasMyLocationFill(for: .origin))
        XCTAssertFalse(state.hasMyLocationFill(for: .destination))

        state.swapMyLocationFills()

        XCTAssertFalse(state.hasMyLocationFill(for: .origin))
        XCTAssertTrue(state.hasMyLocationFill(for: .destination))
    }

    /// Creates the state under test over the manual scheduler and the
    /// registry's traffic descriptor in force.
    private func makeState(
        scheduler: ManualDebounceScheduler = ManualDebounceScheduler(),
        fetcher: GatedTrafficFetcher = GatedTrafficFetcher(),
        fetchTrafficFlow: ((BoundingBox) async throws -> [TrafficFlowData])? = nil,
        overlayRecorder: TrafficOverlayRecorder = TrafficOverlayRecorder(),
        startDeviceLocation: @escaping () -> DeviceLocationStartResult = { .started },
        fillRouteField: @escaping (SearchFieldAssignmentTarget, String) -> Void = { _, _ in },
        appliedProviderKeys: Set<String> = []
    ) -> MapLayersPanelState {
        let state = MapLayersPanelState(
            debounceScheduler: scheduler,
            rememberedChoicesStore: makeStore(),
            fetchTrafficFlow: fetchTrafficFlow ?? { bounds in
                try await fetcher.fetch(bounds)
            },
            cameraGeographicBounds: { Self.sampleBounds },
            installTrafficOverlay: { segments in overlayRecorder.install(segments) },
            readTrafficCacheStatistics: { nil },
            readTileCacheStatistics: { nil },
            clearTrafficOverlay: { overlayRecorder.clear() },
            startDeviceLocation: startDeviceLocation,
            fillRouteField: { field, displayText in
                fillRouteField(field, displayText)
            },
            persistTrafficChoice: {}
        )
        if let trafficDescriptor = try? trafficDescriptor(
            credentialLabel: ""
        ) {
            state.updateTrafficConfiguration(
                providerDescriptors: [trafficDescriptor],
                appliedProviderKeys: appliedProviderKeys
            )
        }
        return state
    }

    /// Decodes one registry descriptor through the shared schema, so the
    /// test reads the same fields the application decodes.
    private func descriptor(
        providerKey: String,
        displayName: String,
        credentialLabel: String = "",
        servedContracts: [String],
        rasterUrlTemplate: String = "",
        vectorUrlTemplate: String = "",
        vectorStyles: [[String: Any]] = [],
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
            "terms_note": "",
            "settings_hint": "",
        ]
        if !rasterUrlTemplate.isEmpty {
            descriptorObject["raster_url_template"] = rasterUrlTemplate
        }
        if !vectorUrlTemplate.isEmpty {
            descriptorObject["url_template"] = vectorUrlTemplate
        }
        if !vectorStyles.isEmpty {
            descriptorObject["vector_styles"] = vectorStyles
        }
        let descriptorData = try JSONSerialization.data(
            withJSONObject: descriptorObject
        )
        return try JSONDecoder().decode(ProviderDescriptor.self, from: descriptorData)
    }

    /// The keyless traffic descriptor the state tests switch on.
    private func trafficDescriptor(
        credentialLabel: String
    ) throws -> ProviderDescriptor {
        try descriptor(
            providerKey: "traffic-provider",
            displayName: "Traffic Provider",
            credentialLabel: credentialLabel,
            servedContracts: [Self.trafficFlowCapabilityName]
        )
    }

    /// One vector style's JSON object (the descriptor's decode path).
    private func styleDescriptor(
        styleId: String,
        displayName: String
    ) -> [String: Any] {
        [
            "style_id": styleId,
            "display_name": displayName,
            "theme": "light",
            "attribution_text": "style attribution",
        ]
    }

    /// A store over an isolated defaults suite.
    private func makeStore() -> RememberedChoicesStore {
        RememberedChoicesStore(defaults: makeDefaults())
    }

    /// An isolated defaults domain, cleared before use and removed at
    /// teardown.
    private func makeDefaults() -> UserDefaults {
        let suiteName = "MapLayersPanelStateTests.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suiteName)!
        defaults.removePersistentDomain(forName: suiteName)
        addTeardownBlock {
            defaults.removePersistentDomain(forName: suiteName)
        }
        return defaults
    }
}

// MARK: - Recording fakes

/// Records the overlay install and clear calls.
@MainActor
private final class TrafficOverlayRecorder {
    /// Every installed segment list, in install order.
    private(set) var installedSegments: [[TrafficFlowData]] = []
    /// The number of clear calls.
    private(set) var clearCount = 0

    nonisolated init() {}

    func install(_ segments: [TrafficFlowData]) {
        installedSegments.append(segments)
    }

    func clear() {
        clearCount += 1
        installedSegments.removeAll()
    }
}

/// Records the route-field fill calls.
@MainActor
private final class RouteFieldFillRecorder {
    private(set) var fieldTargets: [SearchFieldAssignmentTarget] = []
    private(set) var displayTexts: [String] = []

    func record(_ field: SearchFieldAssignmentTarget, _ displayText: String) {
        fieldTargets.append(field)
        displayTexts.append(displayText)
    }
}
