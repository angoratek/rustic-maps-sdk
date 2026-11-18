//! The platform-hardening behaviours: the draw delegate's publication gate,
//! the map view's application-lifecycle policy and notification wiring, the
//! Core Location acceptance rules and the diagnostics feed's stop semantics.
//! Every case runs on the simulator without a network or a provider.

import CoreLocation
import MapNativeCoreUI
import RusticMapsSDK
import UIKit
import XCTest

@testable import MapNativeCoreUI
@testable import ReferenceApplication

// MARK: - Draw delegate publication gate

/// The draw delegate publishes readbacks only when a frame's inputs changed,
/// and a changed frame publishes at most once per cadence window.
final class MapFramePublicationGateTests: XCTestCase {
    /// One frame's statistics with a distinct tile count per case.
    private func statistics(presentTileCount: UInt32 = 3) -> FrameStatistics {
        FrameStatistics(
            requestedTileCount: 4,
            presentTileCount: presentTileCount,
            fallbackTileCount: 0,
            deferredTileCount: 0,
            unavailableTileCount: 0,
            overlayTriangleCount: 0
        )
    }

    /// One camera readback with a distinct latitude per case.
    private func camera(latitudeDegrees: Double = 38.5) -> CameraAttributeReadback {
        CameraAttributeReadback(
            centerLatitudeDegrees: latitudeDegrees,
            centerLongitudeDegrees: -121.4,
            zoomLevel: 14,
            bearingDegrees: 0,
            pitchDegrees: 0
        )
    }

    func testRepeatedIdenticalFramesPublishOnlyOnce() {
        var gate = MapFramePublicationGate(minimumIntervalSeconds: 0.1)
        let frameStatistics = statistics()
        let frameCamera = camera()

        XCTAssertTrue(
            gate.shouldPublish(
                statistics: frameStatistics,
                camera: frameCamera,
                now: 1_000
            )
        )
        XCTAssertFalse(
            gate.shouldPublish(
                statistics: frameStatistics,
                camera: frameCamera,
                now: 1_000.05
            )
        )
        XCTAssertFalse(
            gate.shouldPublish(
                statistics: frameStatistics,
                camera: frameCamera,
                now: 1_100
            )
        )
    }

    func testChangedFramePublishesAfterTheCadenceWindow() {
        var gate = MapFramePublicationGate(minimumIntervalSeconds: 0.1)
        let frameCamera = camera()

        XCTAssertTrue(
            gate.shouldPublish(
                statistics: statistics(presentTileCount: 3),
                camera: frameCamera,
                now: 2_000
            )
        )
        // Changed inside the window: throttled, and the change is retried.
        XCTAssertFalse(
            gate.shouldPublish(
                statistics: statistics(presentTileCount: 4),
                camera: frameCamera,
                now: 2_000.05
            )
        )
        XCTAssertTrue(
            gate.shouldPublish(
                statistics: statistics(presentTileCount: 4),
                camera: frameCamera,
                now: 2_001
            )
        )
        // Identical to the last publication: nothing to do.
        XCTAssertFalse(
            gate.shouldPublish(
                statistics: statistics(presentTileCount: 4),
                camera: frameCamera,
                now: 2_002
            )
        )
    }

    func testCameraOnlyChangePublishes() {
        var gate = MapFramePublicationGate(minimumIntervalSeconds: 0.1)
        let frameStatistics = statistics()

        XCTAssertTrue(
            gate.shouldPublish(
                statistics: frameStatistics,
                camera: camera(latitudeDegrees: 38.5),
                now: 3_000
            )
        )
        XCTAssertTrue(
            gate.shouldPublish(
                statistics: frameStatistics,
                camera: camera(latitudeDegrees: 38.6),
                now: 3_100
            )
        )
    }
}

// MARK: - Map view application lifecycle

/// The lifecycle policy releases the surface on background and memory
/// pressure and re-attaches on foreground; the notifications drive it.
@MainActor
final class MapViewLifecycleTests: XCTestCase {
    func testPolicyReleasesOnBackgroundAndMemoryPressure() {
        XCTAssertEqual(
            MapViewLifecyclePolicy.action(for: .didEnterBackground),
            .releaseSurfaceAndPausePump
        )
        XCTAssertEqual(
            MapViewLifecyclePolicy.action(for: .didReceiveMemoryWarning),
            .releaseSurfaceAndRetryAfterMemoryPressure
        )
        XCTAssertEqual(
            MapViewLifecyclePolicy.action(for: .willEnterForeground),
            .resumeAndAttach
        )
        XCTAssertEqual(
            MapViewLifecyclePolicy.action(for: .renderingPaused(true)),
            .releaseSurfaceAndPausePump
        )
        XCTAssertEqual(
            MapViewLifecyclePolicy.action(for: .renderingPaused(false)),
            .resumeAndAttach
        )
    }

    func testApplicationNotificationsDriveTheLifecycleActions() {
        let mapView = MapView(frame: CGRect(x: 0, y: 0, width: 64, height: 64))
        defer { mapView.teardown() }

        NotificationCenter.default.post(
            name: UIApplication.didEnterBackgroundNotification,
            object: nil
        )
        XCTAssertEqual(
            mapView.lastAppliedLifecycleAction,
            .releaseSurfaceAndPausePump
        )

        NotificationCenter.default.post(
            name: UIApplication.willEnterForegroundNotification,
            object: nil
        )
        XCTAssertEqual(mapView.lastAppliedLifecycleAction, .resumeAndAttach)

        NotificationCenter.default.post(
            name: UIApplication.didReceiveMemoryWarningNotification,
            object: nil
        )
        XCTAssertEqual(
            mapView.lastAppliedLifecycleAction,
            .releaseSurfaceAndRetryAfterMemoryPressure
        )
    }

    func testExplicitPauseReleasesTheSurfaceAndResumeReattaches() {
        let mapView = MapView(frame: CGRect(x: 0, y: 0, width: 64, height: 64))
        defer { mapView.teardown() }

        mapView.setRenderingPaused(true)
        XCTAssertEqual(
            mapView.lastAppliedLifecycleAction,
            .releaseSurfaceAndPausePump
        )

        mapView.setRenderingPaused(false)
        XCTAssertEqual(mapView.lastAppliedLifecycleAction, .resumeAndAttach)
    }
}

// MARK: - Draw generation guard

/// An in-flight draw publishes only while the render generation it started
/// under is current; any attach, resize, detach, rebuild, teardown or newly
/// adopted pose drops the stale result.
final class RenderGenerationGateTests: XCTestCase {
    func testACompletedDrawPublishesWhileItsGenerationIsCurrent() {
        var gate = RenderGenerationGate()
        let generation = gate.beginDraw()

        XCTAssertTrue(gate.isCurrent(generation))
    }

    func testAStateChangeDropsTheInFlightDrawAndTheNextDrawIsCurrent() {
        var gate = RenderGenerationGate()
        let generation = gate.beginDraw()

        gate.invalidateInFlightDraw()

        XCTAssertFalse(gate.isCurrent(generation))
        let nextGeneration = gate.beginDraw()
        XCTAssertTrue(gate.isCurrent(nextGeneration))
    }

    func testEveryInvalidationAdvancesTheGeneration() {
        var gate = RenderGenerationGate()
        let firstGeneration = gate.beginDraw()
        gate.invalidateInFlightDraw()
        let secondGeneration = gate.beginDraw()

        XCTAssertNotEqual(firstGeneration, secondGeneration)
        XCTAssertTrue(gate.isCurrent(secondGeneration))
    }
}

// MARK: - Vector base install record

/// The wrapper's vector base record keeps the intended source across native
/// view rebuilds and restores the source still served when a queued install
/// is refused.
final class VectorTileSourceInstallRecordTests: XCTestCase {
    /// One source distinguishable by its name.
    private func source(_ name: String) -> UrlTemplateVectorTileSource {
        UrlTemplateVectorTileSource(
            urlTemplate: "https://tiles.example.org/{z}/{x}/{y}.pbf",
            sourceName: name,
            attributionText: "Example tiles",
            styleId: "streets-light",
            minimumZoomLevel: 0,
            maximumZoomLevel: 14
        )
    }

    func testARefusedQueuedInstallRestoresThePreviousSourceForTheNextSwap() {
        var record = VectorTileSourceInstallRecord()
        let servedSource = source("served")
        let refusedSource = source("refused")
        _ = record.recordSelection(servedSource)

        // A selection queued behind a draw records the served source as its
        // rollback target.
        let previousSource = record.recordSelection(refusedSource)
        record.queueInstall(rollbackSource: previousSource)
        XCTAssertEqual(record.recordedSource, refusedSource)

        // The asynchronous install throws; the native view still serves the
        // previous source, so the record returns to it.
        record.settleQueuedInstall(succeeded: false)
        XCTAssertEqual(
            record.recordedSource,
            servedSource,
            "a refused queued install must roll the record back to the served source"
        )
        XCTAssertFalse(record.isInstallPending)
    }

    func testAnAcceptedQueuedInstallIsReAppliedAfterTheNextSwap() {
        var record = VectorTileSourceInstallRecord()
        _ = record.recordSelection(source("served"))
        let acceptedSource = source("accepted")
        let previousSource = record.recordSelection(acceptedSource)
        record.queueInstall(rollbackSource: previousSource)

        record.settleQueuedInstall(succeeded: true)

        XCTAssertEqual(
            record.recordedSource,
            acceptedSource,
            "the next native-view rebuild re-applies the accepted source"
        )
        XCTAssertFalse(record.isInstallPending)
    }

    func testASecondQueuedSelectionKeepsTheSourceTheNativeViewServes() {
        var record = VectorTileSourceInstallRecord()
        let servedSource = source("served")
        _ = record.recordSelection(servedSource)

        let firstQueuedSource = source("first")
        let firstRollback = record.recordSelection(firstQueuedSource)
        record.queueInstall(rollbackSource: firstRollback)

        // A second selection lands before the first queued install settles.
        let secondQueuedSource = source("second")
        let secondRollback = record.recordSelection(secondQueuedSource)
        record.queueInstall(rollbackSource: secondRollback)

        record.settleQueuedInstall(succeeded: false)

        XCTAssertEqual(
            record.recordedSource,
            servedSource,
            "the rollback target stays the source the native view actually serves"
        )
    }

    func testAnImmediateSelectionAfterAQueuedOneDiscardsItAndAnswersTheServedSource() {
        var record = VectorTileSourceInstallRecord()
        let servedSource = source("served")
        _ = record.recordSelection(servedSource)
        _ = record.recordSelection(source("queued"))
        record.queueInstall(rollbackSource: servedSource)

        // The draw finishes and the user chooses again before the queued
        // install runs; that immediate selection supersedes it.
        _ = record.recordSelection(source("latest"))
        let served = record.discardQueuedInstall()

        XCTAssertEqual(
            served,
            servedSource,
            "the source the native view actually serves is the queued rollback target"
        )
        XCTAssertFalse(
            record.isInstallPending,
            "the superseded queued install must not run later"
        )
    }
}

// MARK: - Core Location acceptance

/// A location manager whose authorization and update state the test owns.
private final class FakeLocationManager: CLLocationManager {
    /// The status the source reads when it starts or follows a change.
    var stubbedAuthorizationStatus: CLAuthorizationStatus = .notDetermined

    /// Whether the source asked the manager to stop updating.
    private(set) var didStopUpdating = false

    /// The background-update flag the source set, kept in memory so the test
    /// bundle never asks Core Location to validate a capability its plist
    /// does not declare.
    private var stubbedAllowsBackgroundLocationUpdates = false

    override var authorizationStatus: CLAuthorizationStatus {
        stubbedAuthorizationStatus
    }

    override var allowsBackgroundLocationUpdates: Bool {
        get { stubbedAllowsBackgroundLocationUpdates }
        set { stubbedAllowsBackgroundLocationUpdates = newValue }
    }

    override func stopUpdatingLocation() {
        didStopUpdating = true
    }
}

/// The location source refuses stale and unknown-accuracy fixes, follows the
/// consumer mode's accuracy, keeps a `.locationUnknown` failure transient and
/// follows authorization changes while updates are active.
final class DeviceLocationSourceTests: XCTestCase {
    /// One fix at the demo coordinate with an adjustable age and accuracy.
    private func makeLocation(
        ageSeconds: TimeInterval = 0,
        horizontalAccuracy: CLLocationAccuracy = 5
    ) -> CLLocation {
        CLLocation(
            coordinate: CLLocationCoordinate2D(
                latitude: 38.5766,
                longitude: -121.4944
            ),
            altitude: 0,
            horizontalAccuracy: horizontalAccuracy,
            verticalAccuracy: 5,
            course: -1,
            speed: 1,
            timestamp: Date().addingTimeInterval(-ageSeconds)
        )
    }

    func testFreshFixIsAccepted() {
        let controller = PositionController()
        let source = DeviceLocationSource(positionController: controller)

        source.locationManager(
            CLLocationManager(),
            didUpdateLocations: [makeLocation()]
        )

        XCTAssertNotNil(controller.latestFix())
        XCTAssertNil(controller.sourceStatus())
    }

    func testStaleFixIsRefused() {
        let controller = PositionController()
        let source = DeviceLocationSource(positionController: controller)

        source.locationManager(
            CLLocationManager(),
            didUpdateLocations: [makeLocation(ageSeconds: 11)]
        )

        XCTAssertNil(controller.latestFix())
    }

    func testFixInsideTheStalenessBoundIsAccepted() {
        let controller = PositionController()
        let source = DeviceLocationSource(positionController: controller)

        source.locationManager(
            CLLocationManager(),
            didUpdateLocations: [makeLocation(ageSeconds: 5)]
        )

        XCTAssertNotNil(controller.latestFix())
    }

    func testNegativeAccuracyFixIsRefused() {
        let controller = PositionController()
        let source = DeviceLocationSource(positionController: controller)

        source.locationManager(
            CLLocationManager(),
            didUpdateLocations: [makeLocation(horizontalAccuracy: -1)]
        )

        XCTAssertNil(controller.latestFix())
    }

    func testLocationUnknownIsTransientAndASubsequentFixIsAccepted() {
        let controller = PositionController()
        let source = DeviceLocationSource(positionController: controller)

        source.locationManager(
            CLLocationManager(),
            didFailWithError: CLError(.locationUnknown)
        )
        XCTAssertNil(controller.sourceStatus())

        source.locationManager(
            CLLocationManager(),
            didUpdateLocations: [makeLocation()]
        )
        XCTAssertNotNil(controller.latestFix())
        XCTAssertNil(controller.sourceStatus())
    }

    func testConsumerModeSelectsTheDesiredAccuracy() {
        let source = DeviceLocationSource(
            positionController: PositionController(),
            consumerMode: .idle
        )
        XCTAssertEqual(
            source.configuredDesiredAccuracy,
            kCLLocationAccuracyNearestTenMeters
        )

        source.consumerMode = .navigation
        XCTAssertEqual(
            source.configuredDesiredAccuracy,
            kCLLocationAccuracyBestForNavigation
        )

        source.consumerMode = .idle
        XCTAssertEqual(
            source.configuredDesiredAccuracy,
            kCLLocationAccuracyNearestTenMeters
        )
    }

    func testARevocationWhileActiveStopsUpdatesAndReportsTheResolvedStatus() {
        let controller = PositionController()
        let manager = FakeLocationManager()
        manager.stubbedAuthorizationStatus = .authorizedAlways
        var resolvedStatuses: [CLAuthorizationStatus] = []
        let source = DeviceLocationSource(
            positionController: controller,
            locationManager: manager,
            declaresLocationBackgroundMode: { false },
            onAuthorizationResolved: { resolvedStatus in
                resolvedStatuses.append(resolvedStatus)
            }
        )
        XCTAssertTrue(source.start())
        XCTAssertTrue(source.isActive)

        manager.stubbedAuthorizationStatus = .denied
        source.locationManagerDidChangeAuthorization(manager)

        XCTAssertFalse(source.isActive, "a revocation stops the running updates")
        XCTAssertTrue(manager.didStopUpdating)
        XCTAssertEqual(controller.sourceStatus(), .permissionDenied)
        XCTAssertEqual(
            resolvedStatuses,
            [.denied],
            "the resolved callback tells the application the permission changed"
        )
    }

    func testAnAlwaysUpgradeWhileActiveReconfiguresBackgroundLocation() {
        let controller = PositionController()
        let manager = FakeLocationManager()
        manager.stubbedAuthorizationStatus = .authorizedWhenInUse
        let source = DeviceLocationSource(
            positionController: controller,
            enablesBackgroundLocationUpdates: true,
            locationManager: manager,
            declaresLocationBackgroundMode: { true }
        )
        XCTAssertTrue(source.start())
        XCTAssertTrue(source.isActive)
        XCTAssertFalse(
            source.isBackgroundLocationConfigured,
            "when-in-use authorization does not carry background updates"
        )

        manager.stubbedAuthorizationStatus = .authorizedAlways
        source.locationManagerDidChangeAuthorization(manager)

        XCTAssertTrue(source.isActive, "an upgrade keeps the updates running")
        XCTAssertTrue(
            source.isBackgroundLocationConfigured,
            "an Always upgrade enables the background capability it declared"
        )

        manager.stubbedAuthorizationStatus = .authorizedWhenInUse
        source.locationManagerDidChangeAuthorization(manager)

        XCTAssertFalse(
            source.isBackgroundLocationConfigured,
            "a downgrade to when-in-use turns the background capability off again"
        )
    }
}

// MARK: - Diagnostics feed lifecycle

/// The diagnostics feed is lazy and batched: stopping it invalidates the
/// merge timer, refuses later deliveries, and a burst drains in one merge.
@MainActor
final class SdkDiagnosticEventsFeedTests: XCTestCase {
    /// One structured event for the staging list.
    private func makeEvent(index: Int) -> LogEvent {
        LogEvent(
            timestampMillisecondsSinceEpoch: UInt64(index),
            severity: .info,
            subsystemTarget: "test",
            messageLine: "line \(index)",
            // The contract's log event carries the provider request trace;
            // these feed cases exercise the event lines only.
            providerRequestTrace: nil
        )
    }

    /// Long enough for at least one merge at the configured cadence.
    private func waitForMerge() async {
        let cadenceNanoseconds = UInt64(
            ReferenceApplicationConfiguration.diagnosticsEventsRefreshCadenceSeconds
                * 1_000_000_000
        )
        try? await Task.sleep(nanoseconds: cadenceNanoseconds * 2)
    }

    func testStopInvalidatesTheTimerAndRefusesDeliveries() async {
        let buffer = SdkDiagnosticEventsBuffer()
        buffer.startRefreshCadence()
        XCTAssertTrue(buffer.isRefreshCadenceActive)
        XCTAssertTrue(buffer.isAcceptingDeliveries)

        buffer.stopRefreshCadence()
        XCTAssertFalse(buffer.isRefreshCadenceActive)
        XCTAssertFalse(buffer.isAcceptingDeliveries)

        buffer.enqueue(event: makeEvent(index: 0))
        await waitForMerge()
        XCTAssertTrue(buffer.events.isEmpty)
    }

    func testRestartRearmsTheTimerAndAcceptsDeliveries() async {
        let buffer = SdkDiagnosticEventsBuffer()
        buffer.startRefreshCadence()
        buffer.stopRefreshCadence()
        buffer.startRefreshCadence()
        XCTAssertTrue(buffer.isRefreshCadenceActive)

        buffer.enqueue(event: makeEvent(index: 0))
        await waitForMerge()
        XCTAssertEqual(buffer.events.count, 1)

        buffer.stopRefreshCadence()
    }

    func testABurstOfDeliveriesMergesInOneBatch() async {
        let buffer = SdkDiagnosticEventsBuffer()
        buffer.startRefreshCadence()
        defer { buffer.stopRefreshCadence() }

        let listener = MainActorDiagnosticEventListener(buffer: buffer)
        for index in 0..<20 {
            listener.onDiagnosticEvent(event: makeEvent(index: index))
        }

        await waitForMerge()
        XCTAssertEqual(buffer.events.count, 20)
    }
}
