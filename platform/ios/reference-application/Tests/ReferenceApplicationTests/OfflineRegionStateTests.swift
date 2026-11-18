//! The offline card's region-download state (docs/design/TilePackManager.md):
//! the progress mapping (completed/total and the network-versus-disk split),
//! the cancellation terminal state with its partial record, the stored
//! region list and deletion refresh, the failure/resume path and the SDK's
//! own no-tile-cache refusal. Every download is served by a scripted
//! provider; no test reaches the network.

import MapNativeCoreUI
import RusticMapsSDK
import XCTest

@testable import ReferenceApplication

// MARK: - Fakes

/// A scripted offline-region provider: the test decides the tile count, the
/// stored list, the progress reports and the download's outcome.
@MainActor
private final class ScriptedOfflineRegionService: OfflineRegionServicing {
    /// The answer `regionTileCount` gives.
    var tileCountResult: Result<UInt64, Error> = .success(23)

    /// The answer `regionRecords` gives.
    var storedRecords: [RegionRecord] = []

    /// The failure `regionRecords` throws, when one is configured (the
    /// no-tile-cache refusal reaches the listing too).
    var regionRecordsError: Error?

    /// The scripted download, when one is configured; it receives the
    /// progress listener the SDK would call. The script runs on the main
    /// actor, the same context the state drives it from.
    var downloadScript:
        (@MainActor (RegionDownloadProgressListener) async throws -> RegionRecord)?

    /// The answer `deleteRegion` gives, keyed by region identifier.
    var deleteResults: [String: Result<RegionRemovalReport, Error>] = [:]

    /// The region-sizing calls received.
    private(set) var regionTileCountCallCount = 0

    /// The listing calls received.
    private(set) var regionRecordsCallCount = 0

    /// The download calls received.
    private(set) var downloadCallCount = 0

    /// The cancellation requests received, in call order.
    private(set) var cancelledRegionIds: [String] = []

    /// The deletion requests received, in call order.
    private(set) var deletedRegionIds: [String] = []

    /// The continuation the active scripted download waits on.
    private var releaseContinuation: CheckedContinuation<Void, Never>?

    /// Whether the active scripted download is suspended on its release gate.
    private(set) var isWaitingForRelease = false

    /// A default-argument-initializable fake: the default arguments of a
    /// main-actor test helper evaluate outside the actor context.
    nonisolated init() {}

    func regionTileCount(region: RegionDefinition) throws -> UInt64 {
        regionTileCountCallCount += 1
        return try tileCountResult.get()
    }

    func downloadRegion(
        region: RegionDefinition,
        progressListener: RegionDownloadProgressListener
    ) async throws -> RegionRecord {
        downloadCallCount += 1
        guard let downloadScript else {
            throw OfflineRegionServiceError.sdkUnavailable
        }
        return try await downloadScript(progressListener)
    }

    func cancelRegionDownload(regionId: String) {
        cancelledRegionIds.append(regionId)
    }

    func regionRecords() async throws -> [RegionRecord] {
        regionRecordsCallCount += 1
        if let regionRecordsError {
            throw regionRecordsError
        }
        return storedRecords
    }

    func deleteRegion(regionId: String) async throws -> RegionRemovalReport {
        deletedRegionIds.append(regionId)
        guard let result = deleteResults[regionId] else {
            return RegionRemovalReport(removedEntryCount: 0, removedPayloadBytes: 0)
        }
        return try result.get()
    }

    /// Suspends the active scripted download until `releaseDownload` runs.
    func waitForRelease() async {
        isWaitingForRelease = true
        await withCheckedContinuation { continuation in
            releaseContinuation = continuation
        }
    }

    /// Resumes the active scripted download.
    func releaseDownload() {
        isWaitingForRelease = false
        releaseContinuation?.resume()
        releaseContinuation = nil
    }
}

/// Records the failures the state reports into the application's error
/// report.
@MainActor
private final class FailureRecorder {
    /// The failures received, in call order.
    private(set) var received: [Error] = []

    /// A default-argument-initializable recorder: the default arguments of a
    /// main-actor test helper evaluate outside the actor context.
    nonisolated init() {}

    func record(_ error: Error) {
        received.append(error)
    }
}

// MARK: - Tests

@MainActor
final class OfflineRegionStateTests: XCTestCase {
    private let demoRegionId = "downtown-sacramento"
    private let demoRegionName = "Downtown Sacramento"

    func testRefreshReadsTheDemoRegionsEnumeratedTileCount() async {
        let service = ScriptedOfflineRegionService()
        service.tileCountResult = .success(23)
        let state = makeState(service: service)

        await state.refresh()

        XCTAssertEqual(state.demoRegionTileCount, 23)
        XCTAssertEqual(service.regionTileCountCallCount, 1)
        XCTAssertEqual(
            state.demoRegionSummaryText,
            "Downtown Sacramento · zoom 12–16 · 23 tiles"
        )
        XCTAssertTrue(state.isTileCacheAvailable)
        XCTAssertNil(state.tileCacheRefusalText)
    }

    func testProgressMappingCarriesCompletedTotalAndTheNetworkVersusDiskSplit() {
        let state = makeState()

        state.applyProgress(
            makeProgress(completed: 5, network: 4, disk: 1, phase: .downloading)
        )

        let progress = state.progress
        XCTAssertEqual(progress?.totalTileCount, 23)
        XCTAssertEqual(progress?.completedTileCount, 5)
        XCTAssertEqual(progress?.networkFetchedTileCount, 4)
        XCTAssertEqual(progress?.diskServedTileCount, 1)
        XCTAssertEqual(state.phase, .downloading)
        XCTAssertEqual(
            state.statusLineText,
            "Downloading… 5/23 tiles · 4 fetched from the network · 1 from disk"
        )
    }

    func testProgressFromAnotherRegionIsIgnored() {
        let state = makeState()

        state.applyProgress(
            makeProgress(
                regionId: "another-region",
                completed: 3,
                network: 3,
                disk: 0,
                phase: .downloading
            )
        )

        XCTAssertNil(state.progress)
        XCTAssertEqual(state.phase, .idle)
        XCTAssertNil(state.statusLineText)
    }

    func testCompletedTerminalReportShowsTheTerminalLine() {
        let state = makeState()

        state.applyProgress(
            makeProgress(completed: 23, network: 20, disk: 3, phase: .completed)
        )

        XCTAssertEqual(state.phase, .completed)
        XCTAssertEqual(state.statusLineText, "Download complete")
        XCTAssertEqual(state.progress?.completedTileCount, 23)
        XCTAssertEqual(state.progress?.networkFetchedTileCount, 20)
        XCTAssertEqual(state.progress?.diskServedTileCount, 3)
    }

    func testCancellationEndsInTheCancelledTerminalStateWithThePartialRecord() async {
        let service = ScriptedOfflineRegionService()
        let partialRecord = makeRecord(presentTileCount: 5, payloadBytes: 1_024)
        service.downloadScript = { [weak service] progressListener in
            guard let service else {
                throw OfflineRegionServiceError.sdkUnavailable
            }
            progressListener.onRegionDownloadProgress(
                progress: self.makeProgress(
                    completed: 2, network: 2, disk: 0, phase: .downloading
                )
            )
            await service.waitForRelease()
            service.storedRecords = [partialRecord]
            progressListener.onRegionDownloadProgress(
                progress: self.makeProgress(
                    completed: 5, network: 4, disk: 1, phase: .cancelled
                )
            )
            return partialRecord
        }
        let state = makeState(service: service)
        state.download()

        let didStart = await awaitUntil { state.isDownloading }
        XCTAssertTrue(didStart, "the scripted download must start")

        let didReachTheGate = await awaitUntil { service.isWaitingForRelease }
        XCTAssertTrue(didReachTheGate, "the scripted download must reach its gate")

        state.cancelDownload()
        XCTAssertEqual(service.cancelledRegionIds, [demoRegionId])

        service.releaseDownload()
        let didCancel = await awaitUntil {
            state.phase == .cancelled && !state.isDownloading
        }
        XCTAssertTrue(
            didCancel,
            "cancellation must settle on the cancelled phase, got \(state.phase)"
        )
        XCTAssertEqual(
            state.statusLineText,
            "Download cancelled; the stored tiles are kept for a later download."
        )
        XCTAssertEqual(state.storedRegions.count, 1)
        XCTAssertEqual(state.storedRegions.first?.presentTileCount, 5)
        XCTAssertEqual(state.storedRegions.first?.memberTileCount, 23)
    }

    func testFailedDownloadKeepsThePartialRecordReportsTheTypedErrorAndResumes()
        async
    {
        let service = ScriptedOfflineRegionService()
        let failureRecorder = FailureRecorder()
        let partialRecord = makeRecord(presentTileCount: 7, payloadBytes: 2_048)
        let refusal = SdkError.CacheOperationFailure(
            detail: "the store refused a write for 16 tiles"
        )
        service.downloadScript = { [weak service] progressListener in
            progressListener.onRegionDownloadProgress(
                progress: self.makeProgress(
                    completed: 7, network: 7, disk: 0, phase: .downloading
                )
            )
            service?.storedRecords = [partialRecord]
            throw refusal
        }
        let state = makeState(service: service, failureRecorder: failureRecorder)
        state.download()

        let didFail = await awaitUntil { state.phase == .failed && !state.isDownloading }
        XCTAssertTrue(didFail, "the refusal must settle on the failed phase")
        XCTAssertEqual(state.statusLineText, "Download failed")
        XCTAssertEqual(failureRecorder.received.count, 1)
        guard let reportedError = failureRecorder.received.first as? SdkError,
              case let .CacheOperationFailure(detail) = reportedError
        else {
            return XCTFail("the state must report the typed cache failure verbatim")
        }
        XCTAssertEqual(detail, "the store refused a write for 16 tiles")
        XCTAssertEqual(state.storedRegions.first?.presentTileCount, 7)

        // A failure leaves Download available and the next call resumes: the
        // scripted provider answers a complete record this time.
        let completeRecord = makeRecord(presentTileCount: 23, payloadBytes: 4_096)
        service.downloadScript = { [weak service] progressListener in
            progressListener.onRegionDownloadProgress(
                progress: self.makeProgress(
                    completed: 23, network: 16, disk: 7, phase: .completed
                )
            )
            service?.storedRecords = [completeRecord]
            return completeRecord
        }
        state.download()
        let didComplete = await awaitUntil {
            state.phase == .completed && !state.isDownloading
        }
        XCTAssertTrue(didComplete, "the second download must complete")
        XCTAssertEqual(service.downloadCallCount, 2)
        XCTAssertEqual(state.storedRegions.first?.presentTileCount, 23)
        XCTAssertEqual(state.statusLineText, "Download complete")
    }

    func testMissingTileCacheStatesTheSDKRefusalWithoutFakingProgress() async {
        let service = ScriptedOfflineRegionService()
        let failureRecorder = FailureRecorder()
        let refusal = SdkError.ProviderNotConfigured(contractName: "TileCache")
        service.tileCountResult = .failure(refusal)
        service.regionRecordsError = refusal
        service.downloadScript = { _ in
            throw refusal
        }
        let state = makeState(service: service, failureRecorder: failureRecorder)

        await state.refresh()

        XCTAssertFalse(state.isTileCacheAvailable)
        XCTAssertEqual(state.tileCacheRefusalText, "no 'TileCache' provider is configured")
        XCTAssertNil(state.demoRegionTileCount)
        XCTAssertNil(state.progress)
        XCTAssertEqual(state.phase, .idle)
        XCTAssertNil(state.statusLineText)

        state.download()
        let didSettle = await awaitUntil { !state.isDownloading }
        XCTAssertTrue(didSettle)
        // The refusal is a configuration state, not a faked progress run and
        // not a download failure: the phase stays idle and counts stay empty.
        XCTAssertEqual(state.phase, .idle)
        XCTAssertNil(state.progress)
        XCTAssertEqual(state.tileCacheRefusalText, "no 'TileCache' provider is configured")
        XCTAssertTrue(failureRecorder.received.isEmpty)
    }

    func testStoredRegionListRefreshReadsCountsBytesAndRetention() async {
        let service = ScriptedOfflineRegionService()
        service.storedRecords = [
            makeRecord(
                presentTileCount: 23,
                payloadBytes: 1_048_576,
                effectiveTimeToLiveSeconds: 2_592_000
            )
        ]
        let state = makeState(service: service)

        await state.refreshStoredRegions()

        XCTAssertEqual(state.storedRegions.count, 1)
        let storedRegion = state.storedRegions[0]
        XCTAssertEqual(storedRegion.displayName, demoRegionName)
        XCTAssertEqual(storedRegion.tilesLineText, "Stored tiles 23 / 23")
        XCTAssertEqual(storedRegion.bytesLineText, "Stored bytes 1.0 MiB")
        XCTAssertEqual(storedRegion.retentionLineText, "Retained 30 days")
    }

    func testDeletingARegionRefreshesTheListAndShowsTheDeletedLine() async {
        let service = ScriptedOfflineRegionService()
        service.storedRecords = [
            makeRecord(presentTileCount: 23, payloadBytes: 1_048_576)
        ]
        service.deleteResults[demoRegionId] = .success(
            RegionRemovalReport(removedEntryCount: 23, removedPayloadBytes: 1_048_576)
        )
        let state = makeState(service: service)
        await state.refreshStoredRegions()
        let storedRegion = state.storedRegions[0]

        // The deletion clears the provider's list, as the SDK removes the
        // membership and the tiles.
        service.storedRecords = []
        service.deleteResults[demoRegionId] = .success(
            RegionRemovalReport(removedEntryCount: 23, removedPayloadBytes: 1_048_576)
        )

        await state.deleteRegion(storedRegion)

        XCTAssertEqual(service.deletedRegionIds, [demoRegionId])
        XCTAssertTrue(state.storedRegions.isEmpty)
        XCTAssertEqual(state.removedStatusText, "Region deleted")
        XCTAssertEqual(state.lastRemovalReport?.removedEntryCount, 23)
        XCTAssertEqual(state.lastRemovalReport?.removedPayloadBytes, 1_048_576)
    }

    // MARK: Helpers

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

    /// Builds the state under test over the scripted provider.
    private func makeState(
        service: ScriptedOfflineRegionService = ScriptedOfflineRegionService(),
        failureRecorder: FailureRecorder = FailureRecorder()
    ) -> OfflineRegionState {
        OfflineRegionState(
            demoRegion: makeDefinition(),
            serviceProvider: { service },
            reportFailure: { error in failureRecorder.record(error) }
        )
    }

    /// The shared artifact's demo region, with the committed values.
    private func makeDefinition() -> RegionDefinition {
        RegionDefinition(
            regionId: demoRegionId,
            displayName: demoRegionName,
            boundingBox: BoundingBox(
                southWest: GeographicCoordinate(latitude: 38.566, longitude: -121.508),
                northEast: GeographicCoordinate(latitude: 38.582, longitude: -121.488)
            ),
            minimumZoomLevel: 12,
            maximumZoomLevel: 16,
            imageFormat: .png,
            tileResolution: .standard,
            timeToLiveSeconds: 2_592_000
        )
    }

    /// One stored-region record with the test's live counts.
    private func makeRecord(
        presentTileCount: UInt64,
        payloadBytes: UInt64,
        effectiveTimeToLiveSeconds: UInt64 = 2_592_000
    ) -> RegionRecord {
        RegionRecord(
            regionId: demoRegionId,
            displayName: demoRegionName,
            boundingBox: BoundingBox(
                southWest: GeographicCoordinate(latitude: 38.566, longitude: -121.508),
                northEast: GeographicCoordinate(latitude: 38.582, longitude: -121.488)
            ),
            minimumZoomLevel: 12,
            maximumZoomLevel: 16,
            imageFormat: .png,
            tileResolution: .standard,
            effectiveTimeToLiveSeconds: effectiveTimeToLiveSeconds,
            memberTileCount: 23,
            presentTileCount: presentTileCount,
            presentPayloadBytes: payloadBytes
        )
    }

    /// One progress report.
    private func makeProgress(
        regionId: String? = nil,
        completed: UInt64,
        network: UInt64,
        disk: UInt64,
        phase: RegionDownloadPhase
    ) -> RegionDownloadProgress {
        RegionDownloadProgress(
            regionId: regionId ?? demoRegionId,
            totalTileCount: 23,
            completedTileCount: completed,
            networkFetchedTileCount: network,
            diskServedTileCount: disk,
            phase: phase
        )
    }
}
