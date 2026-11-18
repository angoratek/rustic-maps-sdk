package com.rusticmaps.sdk.referenceapplication

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.rustic_maps.RegionDefinition
import uniffi.rustic_maps.RegionDownloadPhase
import uniffi.rustic_maps.RegionDownloadProgress
import uniffi.rustic_maps.RegionDownloadProgressListener
import uniffi.rustic_maps.RegionRecord
import uniffi.rustic_maps.RegionRemovalReport

/**
 * One row of the offline card's stored-regions list: a region on disk with
 * the tile and byte footprint the card reports and the identifier its delete
 * control carries.
 */
data class OfflineStoredRegionRow(
    /** The stored region's identifier. */
    val regionId: String,
    /** The region's display name. */
    val displayName: String,
    /** The stored tile count and payload bytes in the card's wording. */
    val footprintText: String,
    /** The row's shared UI-automation identifier. */
    val deleteTestTag: String,
)

/**
 * The offline-maps card's region-download flow (docs/ReferenceAppUX.md
 * §4.1/§2.5): the demo region's tile count, the download's live progress,
 * cancellation, the stored-regions list and deletion.
 *
 * The SDK's region members are injected as function seams so this state runs
 * in a pure JVM unit test with a scripted download; no test reaches the
 * network and none needs a native library. The card itself renders the state
 * below, and the progress listener hops onto [coroutineScope]'s dispatcher
 * because the SDK calls it on a worker thread.
 */
class OfflineRegionCardState(
    /** The scope the asynchronous SDK calls and listener hops launch into. */
    private val coroutineScope: CoroutineScope,
    /** `OfflineManager.regionTileCount`: the tiles a definition covers. */
    private val regionTileCount: (RegionDefinition) -> ULong,
    /** `OfflineManager.downloadRegion`: downloads and returns the final record. */
    private val downloadRegion:
        suspend (RegionDefinition, RegionDownloadProgressListener) -> RegionRecord,
    /** `OfflineManager.cancelRegionDownload`: signals the in-flight download. */
    private val cancelRegionDownload: (String) -> Unit,
    /** `OfflineManager.regionRecords`: every region stored on disk. */
    private val listRegionRecords: suspend () -> List<RegionRecord>,
    /** `OfflineManager.deleteRegion`: removes a region and answers what went. */
    private val deleteRegion: suspend (String) -> RegionRemovalReport,
    /** The verbatim detail of any failure (the app's §6.3 mapping). */
    private val failureText: (Throwable) -> String,
    /** Reports a download or delete failure to the screen's error strip. */
    private val reportFailure: (Throwable) -> Unit = {},
) {
    // -------------------------------------------------------------------
    // Demo region
    // -------------------------------------------------------------------

    /** The demo region the card offers, or null when the artifact carries none. */
    var demoRegion: RegionDefinition? by mutableStateOf(null)
        private set

    /** The tiles the definition covers, read before the download starts. */
    var demoRegionTileCount: ULong? by mutableStateOf(null)
        private set

    /**
     * The SDK's refusal while no tile cache is installed (`ProviderNotConfigured`
     * naming `TileCache`), stated verbatim instead of faking progress.
     */
    var tileCacheRefusalReason: String? by mutableStateOf(null)
        private set

    /** The demo region row's text: its name and the tiles its definition covers. */
    val demoRegionLineText: String?
        get() {
            val region = demoRegion ?: return null
            val tileCount = demoRegionTileCount ?: return region.displayName
            return ReferenceApplicationConstants.OFFLINE_REGION_TILE_COUNT_TEMPLATE
                .format(region.displayName, tileCount.toLong())
        }

    // -------------------------------------------------------------------
    // Download progress
    // -------------------------------------------------------------------

    /** The latest progress report, synthetic or delivered by the SDK. */
    var progress: RegionDownloadProgress? by mutableStateOf(null)
        private set

    /** The record the download returned on success or cancellation. */
    var terminalRecord: RegionRecord? by mutableStateOf(null)
        private set

    /** The verbatim reason a failed download kept its partial region. */
    var downloadFailureReason: String? by mutableStateOf(null)
        private set

    /** Whether the current download was asked to cancel (the terminal choice). */
    private var cancellationRequested: Boolean = false

    /**
     * The newest report as the SDK delivered it. The listener writes this
     * before hopping to the UI dispatcher, so the terminal state keeps the
     * honest partial counts even when the hop lands after the member returns.
     */
    @Volatile
    private var latestReportedProgress: RegionDownloadProgress? = null

    /** Whether a download is in flight. */
    val isDownloading: Boolean
        get() = progress?.phase == RegionDownloadPhase.DOWNLOADING

    /** Whether the card may start a download right now. */
    val canDownload: Boolean
        get() = demoRegion != null && tileCacheRefusalReason == null && !isDownloading

    /**
     * The card's one status line: the in-flight counts, the completed
     * footprint, the cancelled partial count, or the failed reason.
     */
    val downloadStatusText: String?
        get() {
            val current = progress ?: return null
            return when (current.phase) {
                RegionDownloadPhase.DOWNLOADING ->
                    "${ReferenceApplicationConstants.OFFLINE_DOWNLOADING_STATUS_TEXT} " +
                        progressCountsText(current)
                RegionDownloadPhase.COMPLETED ->
                    "${ReferenceApplicationConstants.OFFLINE_COMPLETED_STATUS_TEXT} · " +
                        (terminalRecord?.let(::footprintText) ?: progressCountsText(current))
                RegionDownloadPhase.CANCELLED ->
                    ReferenceApplicationConstants.OFFLINE_CANCELLED_STATUS_TEMPLATE.format(
                        ReferenceApplicationConstants.OFFLINE_CANCELLED_STATUS_TEXT,
                        current.completedTileCount.toLong(),
                        current.totalTileCount.toLong(),
                    )
                RegionDownloadPhase.FAILED ->
                    ReferenceApplicationConstants.OFFLINE_FAILED_STATUS_TEXT +
                        (downloadFailureReason?.let { reason -> ": $reason" } ?: "")
            }
        }

    // -------------------------------------------------------------------
    // Stored regions
    // -------------------------------------------------------------------

    /** Every region stored on disk, in the store's listing order. */
    var storedRegions: List<RegionRecord> by mutableStateOf(emptyList())
        private set

    /** The verbatim reason the stored-region listing failed, or null. */
    var storedRegionsFailureReason: String? by mutableStateOf(null)
        private set

    /** What the last deletion removed, for the card's deleted line. */
    var lastRemovalReport: RegionRemovalReport? by mutableStateOf(null)
        private set

    /** The stored-regions list's rows. */
    val storedRegionRows: List<OfflineStoredRegionRow>
        get() = storedRegions.map { record ->
            OfflineStoredRegionRow(
                regionId = record.regionId,
                displayName = record.displayName,
                footprintText = footprintText(record),
                deleteTestTag = ReferenceApplicationConstants
                    .OFFLINE_REGION_DELETE_TEST_TAG_PREFIX + record.regionId,
            )
        }

    /** The card's deleted line, or null before a deletion in this session. */
    val deletedStatusText: String?
        get() = if (lastRemovalReport != null) {
            ReferenceApplicationConstants.OFFLINE_DELETED_STATUS_TEXT
        } else {
            null
        }

    // -------------------------------------------------------------------
    // Actions
    // -------------------------------------------------------------------

    /**
     * Loads the demo region and reads its tile count and the stored-region
     * list. Called at startup and again after the tile cache registers, so
     * the no-cache refusal becomes a real count without restarting the app.
     */
    fun loadDemoRegion(region: RegionDefinition?) {
        demoRegion = region
        refreshDemoRegion()
        refreshStoredRegions()
    }

    /**
     * Loads the demo region after a tile-cache registration, running the
     * synchronous tile-count enumeration (which validates the region and
     * counts its tiles) on [Dispatchers.IO] instead of the main thread. The
     * result is published on the caller's dispatcher, so the card's state
     * updates in the same order as the registration that triggered them.
     * The stored-region listing follows through [refreshStoredRegions].
     */
    suspend fun loadDemoRegionAfterTileCacheRegistration(region: RegionDefinition?) {
        demoRegion = region
        if (region == null) {
            return
        }
        if (!isDownloading) {
            val tileCountOutcome = withContext(Dispatchers.IO) {
                try {
                    DemoRegionTileCountOutcome.Counted(regionTileCount(region))
                } catch (cancellation: CancellationException) {
                    throw cancellation
                } catch (thrown: Throwable) {
                    DemoRegionTileCountOutcome.Refused(thrown)
                }
            }
            when (tileCountOutcome) {
                is DemoRegionTileCountOutcome.Counted -> {
                    demoRegionTileCount = tileCountOutcome.tileCount
                    tileCacheRefusalReason = null
                }
                is DemoRegionTileCountOutcome.Refused -> {
                    demoRegionTileCount = null
                    tileCacheRefusalReason = failureText(tileCountOutcome.failure)
                }
            }
        }
        refreshStoredRegions()
    }

    /**
     * Re-reads the demo region's tile count. The synchronous SDK call only
     * validates and enumerates, so it runs on the caller's thread; a refusal
     * (no cache installed, an invalid definition) is stated, never faked.
     */
    fun refreshDemoRegion() {
        val region = demoRegion ?: return
        if (isDownloading) {
            return
        }
        try {
            demoRegionTileCount = regionTileCount(region)
            tileCacheRefusalReason = null
        } catch (regionException: CancellationException) {
            throw regionException
        } catch (regionException: Throwable) {
            demoRegionTileCount = null
            tileCacheRefusalReason = failureText(regionException)
        }
    }

    /** Re-reads the stored-region list; a listing refusal is stated. */
    fun refreshStoredRegions() {
        coroutineScope.launch {
            try {
                storedRegions = listRegionRecords()
                storedRegionsFailureReason = null
            } catch (listingException: CancellationException) {
                throw listingException
            } catch (listingException: Throwable) {
                storedRegions = emptyList()
                storedRegionsFailureReason = failureText(listingException)
            }
        }
    }

    /**
     * Starts (or resumes) the demo region's download. A region already
     * downloaded is re-prepared: present tiles are served from disk, so a
     * previous failure or cancellation resumes exactly where it stopped.
     * While no cache is installed the card keeps its refusal and starts
     * nothing.
     */
    fun downloadDemoRegion() {
        val region = demoRegion ?: return
        if (!canDownload) {
            return
        }
        cancellationRequested = false
        downloadFailureReason = null
        terminalRecord = null
        lastRemovalReport = null
        latestReportedProgress = null
        // Mark the flow in flight before the first listener report arrives;
        // the SDK follows with a zero-completion report of its own.
        progress = RegionDownloadProgress(
            regionId = region.regionId,
            totalTileCount = demoRegionTileCount ?: 0UL,
            completedTileCount = 0UL,
            networkFetchedTileCount = 0UL,
            diskServedTileCount = 0UL,
            phase = RegionDownloadPhase.DOWNLOADING,
        )
        coroutineScope.launch {
            // The SDK calls the listener on a worker thread; hop onto this
            // scope's dispatcher before touching snapshot state.
            val listener = object : RegionDownloadProgressListener {
                override fun onRegionDownloadProgress(progress: RegionDownloadProgress) {
                    latestReportedProgress = progress
                    coroutineScope.launch {
                        val currentProgress = this@OfflineRegionCardState.progress
                        // A report delivered after the download reached its
                        // terminal phase must not resurrect the in-flight line
                        // (the listener callbacks and the member's return are
                        // marshaled independently).
                        if (progress.phase == RegionDownloadPhase.DOWNLOADING &&
                            currentProgress != null &&
                            currentProgress.phase != RegionDownloadPhase.DOWNLOADING
                        ) {
                            return@launch
                        }
                        this@OfflineRegionCardState.progress = progress
                    }
                }
            }
            try {
                val record = downloadRegion(region, listener)
                terminalRecord = record
                val cancelled = cancellationRequested &&
                    record.presentTileCount < record.memberTileCount
                progress = RegionDownloadProgress(
                    regionId = record.regionId,
                    totalTileCount = record.memberTileCount,
                    completedTileCount = record.presentTileCount,
                    networkFetchedTileCount =
                        latestReportedProgress?.networkFetchedTileCount ?: 0UL,
                    diskServedTileCount =
                        latestReportedProgress?.diskServedTileCount ?: 0UL,
                    phase = if (cancelled) {
                        RegionDownloadPhase.CANCELLED
                    } else {
                        RegionDownloadPhase.COMPLETED
                    },
                )
                // A failed or cancelled prepare keeps its partial membership
                // on disk; a completed one is the new stored row.
                refreshStoredRegions()
            } catch (downloadException: CancellationException) {
                // Composition teardown, not a demo failure: rethrow, never
                // report (the app's false-positive rule).
                throw downloadException
            } catch (downloadException: Throwable) {
                // The SDK keeps the fetched tiles and the membership naming
                // the missing ones, so the card keeps its counts, states the
                // verbatim reason and leaves Download available to resume.
                downloadFailureReason = failureText(downloadException)
                val partialProgress = latestReportedProgress ?: progress
                progress = (partialProgress ?: failedProgress(region))
                    .copy(phase = RegionDownloadPhase.FAILED)
                reportFailure(downloadException)
                refreshStoredRegions()
            }
        }
    }

    /**
     * Signals the in-flight download to stop. The SDK answers with a
     * terminal `Cancelled` report and the partial record, which keeps the
     * region resumable; the call is idempotent.
     */
    fun cancelDownload() {
        val region = demoRegion ?: return
        if (!isDownloading) {
            return
        }
        cancellationRequested = true
        cancelRegionDownload(region.regionId)
    }

    /**
     * Deletes one stored region and refreshes the list. The SDK answers what
     * it removed; a tile another live region claims stays on disk.
     */
    fun deleteStoredRegion(regionId: String) {
        coroutineScope.launch {
            try {
                val removalReport = deleteRegion(regionId)
                lastRemovalReport = removalReport
                storedRegions = storedRegions.filterNot { record ->
                    record.regionId == regionId
                }
                refreshStoredRegions()
            } catch (deleteException: CancellationException) {
                throw deleteException
            } catch (deleteException: Throwable) {
                storedRegionsFailureReason = failureText(deleteException)
                reportFailure(deleteException)
            }
        }
    }

    // -------------------------------------------------------------------
    // Text composition
    // -------------------------------------------------------------------

    /** The in-flight counts: completed / total and the network-versus-disk split. */
    private fun progressCountsText(progress: RegionDownloadProgress): String =
        ReferenceApplicationConstants.OFFLINE_DOWNLOAD_PROGRESS_COUNTS_TEMPLATE.format(
            progress.completedTileCount.toLong(),
            progress.totalTileCount.toLong(),
            progress.networkFetchedTileCount.toLong(),
            progress.diskServedTileCount.toLong(),
        )

    /** A region's stored tile count and payload bytes in the card's wording. */
    private fun footprintText(record: RegionRecord): String =
        ReferenceApplicationConstants.OFFLINE_STORED_REGION_SUMMARY_TEMPLATE.format(
            ReferenceApplicationConstants.OFFLINE_STORED_TILES_LABEL_TEXT,
            record.presentTileCount.toLong(),
            ReferenceApplicationConstants.OFFLINE_STORED_BYTES_LABEL_TEXT,
            record.presentPayloadBytes.toLong(),
        )

    /** A zero-count failed report for errors that arrive before any progress. */
    private fun failedProgress(region: RegionDefinition): RegionDownloadProgress =
        RegionDownloadProgress(
            regionId = region.regionId,
            totalTileCount = demoRegionTileCount ?: 0UL,
            completedTileCount = 0UL,
            networkFetchedTileCount = 0UL,
            diskServedTileCount = 0UL,
            phase = RegionDownloadPhase.FAILED,
        )

    /** The outcome of the off-main-thread demo-region tile-count read. */
    private sealed interface DemoRegionTileCountOutcome {
        /** The cache answered the region's tile count. */
        class Counted(val tileCount: ULong) : DemoRegionTileCountOutcome

        /** The cache refused the read; the reason is published verbatim. */
        class Refused(val failure: Throwable) : DemoRegionTileCountOutcome
    }
}
