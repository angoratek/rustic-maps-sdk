//! The offline card's region-download state (crates/rustic-maps-cache/src/tile_packs.rs):
//! the demo region's tile count, the download's live progress, cancellation,
//! the stored-region list and deletion.
//!
//! The type owns every decision the card makes and reaches the SDK only
//! through the injected `OfflineRegionServicing` seam, so its unit tests drive
//! a scripted provider and never the network. The model supplies the
//! production seam over the contract's `OfflineManager`. Progress callbacks
//! arrive on an SDK worker thread; the relay hops them to the main actor
//! before the state publishes them.

import Combine
import Foundation
import RusticMapsSDK

// MARK: - Errors

/// A local failure of the offline-region seam (the SDK is not running yet).
enum OfflineRegionServiceError: Swift.Error, LocalizedError {
    /// The session has no SDK to ask (the keyless start before a provider).
    case sdkUnavailable

    var errorDescription: String? {
        "The SDK is not running yet."
    }
}

// MARK: - The SDK seam

/// The subset of the contract's `OfflineManager` the card drives. The
/// production implementation forwards to the generated manager; tests
/// provide a scripted provider so no download reaches a network.
@MainActor
protocol OfflineRegionServicing: AnyObject {
    /// Tiles the region enumerates (nothing is written).
    func regionTileCount(region: RegionDefinition) throws -> UInt64

    /// Downloads the region, reporting progress to the listener, and answers
    /// the region's record as it stands afterwards.
    func downloadRegion(
        region: RegionDefinition,
        progressListener: RegionDownloadProgressListener
    ) async throws -> RegionRecord

    /// Signals the in-flight download of the region to stop (idempotent).
    func cancelRegionDownload(regionId: String)

    /// Lists the regions stored on disk.
    func regionRecords() async throws -> [RegionRecord]

    /// Removes a stored region and answers what was removed.
    func deleteRegion(regionId: String) async throws -> RegionRemovalReport
}

/// The production seam over the generated `OfflineManager`.
@MainActor
final class SdkOfflineRegionService: OfflineRegionServicing {
    /// The contract facade over the session's `MapSdk`.
    private let manager: OfflineManager

    init(manager: OfflineManager) {
        self.manager = manager
    }

    func regionTileCount(region: RegionDefinition) throws -> UInt64 {
        try manager.regionTileCount(region: region)
    }

    func downloadRegion(
        region: RegionDefinition,
        progressListener: RegionDownloadProgressListener
    ) async throws -> RegionRecord {
        try await manager.downloadRegion(
            region: region,
            progressListener: progressListener
        )
    }

    func cancelRegionDownload(regionId: String) {
        manager.cancelRegionDownload(regionId: regionId)
    }

    func regionRecords() async throws -> [RegionRecord] {
        try await manager.regionRecords()
    }

    func deleteRegion(regionId: String) async throws -> RegionRemovalReport {
        try await manager.deleteRegion(regionId: regionId)
    }
}

// MARK: - The progress relay

/// The foreign progress callback: the SDK invokes it on an SDK worker thread,
/// never the caller's, so it hands each report to the main actor without
/// blocking the producer (the contract's one rule for callbacks).
final class OfflineRegionProgressRelay: RegionDownloadProgressListener, @unchecked Sendable {
    /// The main-actor handler the reports land on.
    private let handler: @MainActor (RegionDownloadProgress) -> Void

    init(handler: @escaping @MainActor (RegionDownloadProgress) -> Void) {
        self.handler = handler
    }

    func onRegionDownloadProgress(progress: RegionDownloadProgress) {
        let handler = self.handler
        Task { @MainActor in
            handler(progress)
        }
    }
}

// MARK: - The state

/// The offline card's region-download state: one demo region, its live
/// progress, the terminal status and the stored-region list.
@MainActor
final class OfflineRegionState: ObservableObject {
    /// The download's presentation phase. The contract's terminal phases map
    /// one-to-one; `idle` is the card's state before a download or after the
    /// no-cache refusal.
    enum Phase: Equatable {
        /// No download has run in this session.
        case idle
        /// A download is running.
        case downloading
        /// Every member tile is stored.
        case completed
        /// The application cancelled; the partial region is kept.
        case cancelled
        /// The download stopped on an error; the partial region is kept.
        case failed
    }

    /// One progress report's counts, captured for display and tests.
    struct ProgressSnapshot: Equatable {
        /// The region the report belongs to.
        let regionId: String
        /// Tiles the region enumerates.
        let totalTileCount: UInt64
        /// Tiles downloaded and refreshed so far.
        let completedTileCount: UInt64
        /// Completed tiles the provider had to fetch.
        let networkFetchedTileCount: UInt64
        /// Completed tiles the store already held.
        let diskServedTileCount: UInt64
    }

    /// One stored region as the card lists it.
    struct StoredRegion: Identifiable, Equatable {
        /// The region's identifier.
        let regionId: String
        /// The region's display name.
        let displayName: String
        /// Tile fingerprints the region claims membership for.
        let memberTileCount: UInt64
        /// Members whose entries are live on disk right now.
        let presentTileCount: UInt64
        /// Payload bytes of the live members.
        let presentPayloadBytes: UInt64
        /// The retention the region's entries carry, in seconds.
        let effectiveTimeToLiveSeconds: UInt64

        /// One row per region identifier.
        var id: String { regionId }

        /// The row's stored-tile line (`Stored tiles 23 / 23`).
        var tilesLineText: String {
            String(
                format: ReferenceApplicationConfiguration.offlineRegionStoredTilesLineFormat,
                ReferenceApplicationConfiguration.offlineRegionStoredTilesLabel,
                String(presentTileCount),
                String(memberTileCount)
            )
        }

        /// The row's payload-size line (`Stored bytes 1.2 MiB`).
        var bytesLineText: String {
            String(
                format: ReferenceApplicationConfiguration.offlineRegionStoredBytesLineFormat,
                ReferenceApplicationConfiguration.offlineRegionStoredBytesLabel,
                ReferenceApplicationConfiguration.binaryByteFootprintPhrase(
                    bytes: presentPayloadBytes
                )
            )
        }

        /// The row's retention line (`Retained 30 days`).
        var retentionLineText: String {
            String(
                format: ReferenceApplicationConfiguration.offlineRegionRetentionLineFormat,
                String(
                    effectiveTimeToLiveSeconds
                        / ReferenceApplicationConfiguration.secondsPerDay
                )
            )
        }

        /// Builds the row from a contract record.
        init(record: RegionRecord) {
            regionId = record.regionId
            displayName = record.displayName
            memberTileCount = record.memberTileCount
            presentTileCount = record.presentTileCount
            presentPayloadBytes = record.presentPayloadBytes
            effectiveTimeToLiveSeconds = record.effectiveTimeToLiveSeconds
        }
    }

    /// The demo region's definition from the shared artifact, or nil when the
    /// artifact declares none.
    let demoRegion: RegionDefinition?

    /// The demo region's enumerated tile count; nil until the count is read
    /// (or when no tile cache is installed).
    @Published private(set) var demoRegionTileCount: UInt64?

    /// The latest progress report's counts.
    @Published private(set) var progress: ProgressSnapshot?

    /// The download's presentation phase.
    @Published private(set) var phase: Phase = .idle

    /// Whether a download call is in flight (the Cancel control's gate).
    @Published private(set) var isDownloading = false

    /// The regions stored on disk, in the store's listing order.
    @Published private(set) var storedRegions: [StoredRegion] = []

    /// What the last deletion removed, when one ran.
    @Published private(set) var lastRemovalReport: RegionRemovalReport?

    /// The line after a successful deletion; cleared by the next action.
    @Published private(set) var removedStatusText: String?

    /// The SDK's own refusal detail when no tile cache is installed; nil
    /// while the cache is available. The card states it verbatim rather than
    /// faking progress.
    @Published private(set) var tileCacheRefusalText: String?

    /// Whether the installed cache answered the region sizing call.
    @Published private(set) var isTileCacheAvailable = true

    /// Produces the SDK seam on first use (the model resolves it over the
    /// session's `MapSdk`).
    private let serviceProvider: @MainActor () throws -> OfflineRegionServicing

    /// Echoes a typed SDK failure into the application's §6.3 error report.
    private let reportFailure: @MainActor (Error) -> Void

    /// The resolved service, kept because the manager is cheap and always
    /// reads the cache installation in force.
    private var cachedService: OfflineRegionServicing?

    /// The in-flight download (held so the state can outlive the tap).
    private var downloadTask: Task<Void, Never>?

    /// Whether the in-flight download was asked to stop.
    private var hasRequestedCancellation = false

    init(
        demoRegion: RegionDefinition?,
        serviceProvider: @escaping @MainActor () throws -> OfflineRegionServicing,
        reportFailure: @escaping @MainActor (Error) -> Void
    ) {
        self.demoRegion = demoRegion
        self.serviceProvider = serviceProvider
        self.reportFailure = reportFailure
    }

    /// The demo region's summary line: its name, zoom range and — once read —
    /// the enumerated tile count.
    var demoRegionSummaryText: String? {
        guard let demoRegion else { return nil }
        let minimumZoom = String(demoRegion.minimumZoomLevel)
        let maximumZoom = String(demoRegion.maximumZoomLevel)
        if let demoRegionTileCount {
            return String(
                format: ReferenceApplicationConfiguration.offlineRegionDemoSummaryFormat,
                demoRegion.displayName,
                minimumZoom,
                maximumZoom,
                String(demoRegionTileCount)
            )
        }
        return String(
            format:
                ReferenceApplicationConfiguration.offlineRegionDemoSummaryWithoutCountFormat,
            demoRegion.displayName,
            minimumZoom,
            maximumZoom
        )
    }

    /// The status line under the region row: the live progress counts, or the
    /// terminal phase's line. Nil while idle.
    var statusLineText: String? {
        switch phase {
        case .idle:
            return nil
        case .downloading:
            let snapshot = progress
            return String(
                format: ReferenceApplicationConfiguration.offlineRegionDownloadProgressFormat,
                ReferenceApplicationConfiguration.offlineRegionDownloadingStatusText,
                String(snapshot?.completedTileCount ?? 0),
                String(snapshot?.totalTileCount ?? demoRegionTileCount ?? 0),
                String(snapshot?.networkFetchedTileCount ?? 0),
                String(snapshot?.diskServedTileCount ?? 0)
            )
        case .completed:
            return ReferenceApplicationConfiguration.offlineRegionCompletedStatusText
        case .cancelled:
            return ReferenceApplicationConfiguration.offlineRegionCancelledStatusText
        case .failed:
            return ReferenceApplicationConfiguration.offlineRegionFailedStatusText
        }
    }

    /// Reads the demo region's tile count and the stored-region list. Safe to
    /// call whenever the tile cache may have changed.
    func refresh() async {
        refreshRegionTileCount()
        await refreshStoredRegions()
    }

    /// Reads the demo region's enumerated tile count. A missing tile cache is
    /// stated as the SDK's own refusal; the card then offers no download.
    func refreshRegionTileCount() {
        guard let demoRegion else { return }
        do {
            let service = try resolveService()
            demoRegionTileCount = try service.regionTileCount(region: demoRegion)
            isTileCacheAvailable = true
            tileCacheRefusalText = nil
        } catch {
            if Self.isTileCacheNotConfigured(error) {
                demoRegionTileCount = nil
                isTileCacheAvailable = false
                tileCacheRefusalText = Self.tileCacheRefusalDetailText(error)
                progress = nil
                phase = .idle
            } else {
                reportFailure(error)
            }
        }
    }

    /// Refreshes the stored-region list. A missing tile cache states the
    /// refusal and an empty list.
    func refreshStoredRegions() async {
        do {
            let service = try resolveService()
            let records = try await service.regionRecords()
            storedRegions = records.map(StoredRegion.init(record:))
            isTileCacheAvailable = true
            tileCacheRefusalText = nil
        } catch {
            if Self.isTileCacheNotConfigured(error) {
                storedRegions = []
                isTileCacheAvailable = false
                tileCacheRefusalText = Self.tileCacheRefusalDetailText(error)
            } else {
                reportFailure(error)
            }
        }
    }

    /// Starts the demo region's download. A second tap while one runs is
    /// ignored. The Download control stays available after a failure or a
    /// cancellation, and the SDK resumes from the tiles already on disk.
    func download() {
        guard let demoRegion, !isDownloading else { return }
        isDownloading = true
        hasRequestedCancellation = false
        removedStatusText = nil
        phase = .downloading
        progress = ProgressSnapshot(
            regionId: demoRegion.regionId,
            totalTileCount: demoRegionTileCount ?? 0,
            completedTileCount: 0,
            networkFetchedTileCount: 0,
            diskServedTileCount: 0
        )
        let relay = OfflineRegionProgressRelay { [weak self] progress in
            self?.receiveProgress(progress)
        }
        downloadTask = Task { [weak self] in
            guard let self else { return }
            do {
                let service = try self.resolveService()
                let record = try await service.downloadRegion(
                    region: demoRegion,
                    progressListener: relay
                )
                await self.finishDownload(with: record)
            } catch {
                await self.finishDownloadFailure(error)
            }
        }
    }

    /// Signals the in-flight download to stop. Cancellation is a successful
    /// partial result: the partial region is kept and shown in the list.
    func cancelDownload() {
        guard isDownloading, let demoRegion else { return }
        hasRequestedCancellation = true
        do {
            try resolveService().cancelRegionDownload(regionId: demoRegion.regionId)
        } catch {
            reportFailure(error)
        }
    }

    /// Deletes one stored region and refreshes the list.
    func deleteRegion(_ region: StoredRegion) async {
        guard !isDownloading else { return }
        do {
            let service = try resolveService()
            lastRemovalReport = try await service.deleteRegion(regionId: region.regionId)
            removedStatusText =
                ReferenceApplicationConfiguration.offlineRegionDeletedStatusText
            await refreshStoredRegions()
        } catch {
            if Self.isTileCacheNotConfigured(error) {
                isTileCacheAvailable = false
                tileCacheRefusalText = Self.tileCacheRefusalDetailText(error)
            } else {
                reportFailure(error)
            }
        }
    }

    /// The listener's entry point: a report that arrives after the download
    /// call settled is stale (the completion already set the terminal phase
    /// from the returned record or the thrown error) and is dropped rather
    /// than allowed to overwrite the settled state.
    func receiveProgress(_ progress: RegionDownloadProgress) {
        guard isDownloading else { return }
        applyProgress(progress)
    }

    /// Maps one progress report into the published counts and phase. The
    /// listener's reports belong to the demo region only; a foreign region's
    /// report is ignored.
    func applyProgress(_ progress: RegionDownloadProgress) {
        guard progress.regionId == demoRegion?.regionId else { return }
        self.progress = ProgressSnapshot(
            regionId: progress.regionId,
            totalTileCount: progress.totalTileCount,
            completedTileCount: progress.completedTileCount,
            networkFetchedTileCount: progress.networkFetchedTileCount,
            diskServedTileCount: progress.diskServedTileCount
        )
        switch progress.phase {
        case .downloading:
            phase = .downloading
        case .completed:
            phase = .completed
        case .cancelled:
            phase = .cancelled
        case .failed:
            phase = .failed
        }
    }

    // MARK: Completion

    /// Settles the state after a successful (or cancelled) download call and
    /// refreshes the stored list so the partial or complete record is shown.
    private func finishDownload(with record: RegionRecord) async {
        isDownloading = false
        downloadTask = nil
        progress = ProgressSnapshot(
            regionId: record.regionId,
            totalTileCount: record.memberTileCount,
            completedTileCount: record.presentTileCount,
            networkFetchedTileCount: progress?.networkFetchedTileCount ?? 0,
            diskServedTileCount: progress?.diskServedTileCount ?? 0
        )
        if record.presentTileCount >= record.memberTileCount {
            phase = .completed
        } else if hasRequestedCancellation {
            phase = .cancelled
        } else {
            // The contract reports an incomplete prepare as a typed failure;
            // if a caller still returns one, state it honestly.
            phase = .failed
            reportFailure(
                OfflineRegionStateError.incompletePack(
                    regionId: record.regionId,
                    presentTileCount: record.presentTileCount,
                    memberTileCount: record.memberTileCount
                )
            )
        }
        await refreshStoredRegions()
    }

    /// Settles the state after a failed download call. A cancellation request
    /// is the contract's successful partial result; every other error is
    /// echoed verbatim and leaves the partial region resumable.
    private func finishDownloadFailure(_ error: Error) async {
        isDownloading = false
        downloadTask = nil
        if Self.isTileCacheNotConfigured(error) {
            isTileCacheAvailable = false
            tileCacheRefusalText = Self.tileCacheRefusalDetailText(error)
            demoRegionTileCount = nil
            progress = nil
            phase = .idle
            await refreshStoredRegions()
            return
        }
        if hasRequestedCancellation {
            phase = .cancelled
        } else {
            phase = .failed
            reportFailure(error)
        }
        await refreshStoredRegions()
    }

    // MARK: Helpers

    /// Resolves (and caches) the SDK seam.
    private func resolveService() throws -> OfflineRegionServicing {
        if let cachedService {
            return cachedService
        }
        let service = try serviceProvider()
        cachedService = service
        return service
    }

    /// Whether a thrown error is the contract's missing-`TileCache` refusal.
    private static func isTileCacheNotConfigured(_ error: Error) -> Bool {
        if let sdkError = error as? SdkError,
           case let .ProviderNotConfigured(contractName) = sdkError
        {
            return contractName == ReferenceApplicationConfiguration.tileCacheContractName
        }
        // The generated error's description reflects its own variant; the
        // fallback keeps the classification correct when the binding module
        // is linked into both the application and its test host.
        let reflected = String(reflecting: error)
        return reflected.contains("ProviderNotConfigured")
            && reflected.contains(ReferenceApplicationConfiguration.tileCacheContractName)
    }

    /// The contract's canonical missing-provider sentence for a tile-cache
    /// refusal. The generated error's own `errorDescription` reflects the
    /// enum case ("SdkError.ProviderNotConfigured(contractName: …)"), so the
    /// card renders the shared sentence instead, the same text the §6.3
    /// error report uses.
    private static func tileCacheRefusalDetailText(_ error: Error) -> String {
        if let sdkError = error as? SdkError,
           case let .ProviderNotConfigured(contractName) = sdkError
        {
            return ReferenceApplicationConfiguration.providerNotConfiguredDetailText(
                contractName: contractName
            )
        }
        // The cast can fail when the binding module is linked into both the
        // application and its test host; the refusal has already been
        // classified, so the tile-cache contract name is the honest answer.
        return ReferenceApplicationConfiguration.providerNotConfiguredDetailText(
            contractName: ReferenceApplicationConfiguration.tileCacheContractName
        )
    }
}

/// A defect the contract promises never to answer: a successful return whose
/// record still names missing member tiles.
enum OfflineRegionStateError: Swift.Error, LocalizedError {
    /// The returned record is incomplete without a cancellation.
    case incompletePack(
        regionId: String,
        presentTileCount: UInt64,
        memberTileCount: UInt64
    )

    var errorDescription: String? {
        switch self {
        case let .incompletePack(regionId, presentTileCount, memberTileCount):
            return """
                offline region '\(regionId)' returned with \(presentTileCount) of \
                \(memberTileCount) tiles stored
                """
        }
    }
}
