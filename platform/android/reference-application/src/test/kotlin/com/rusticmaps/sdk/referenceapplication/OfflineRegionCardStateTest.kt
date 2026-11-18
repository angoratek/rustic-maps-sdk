package com.rusticmaps.sdk.referenceapplication

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.rustic_maps.BoundingBox
import uniffi.rustic_maps.GeographicCoordinate
import uniffi.rustic_maps.RasterImageFormat
import uniffi.rustic_maps.RasterTileResolution
import uniffi.rustic_maps.RegionDefinition
import uniffi.rustic_maps.RegionDownloadPhase
import uniffi.rustic_maps.RegionDownloadProgress
import uniffi.rustic_maps.RegionDownloadProgressListener
import uniffi.rustic_maps.RegionRecord
import uniffi.rustic_maps.RegionRemovalReport

/**
 * Unit tests of the offline card's region flow: the progress mapping, the
 * cancelled terminal state, the stored-region list and delete refresh, the
 * no-cache refusal, and the failed download that keeps its partial region
 * and offers a resume. Every SDK member is a scripted seam; no test reaches
 * the network and none needs a native library.
 */
class OfflineRegionCardStateTest {
    @Test
    fun `the tile cache registration reads the region count off the caller thread`() = runBlocking {
        val callerThreadName = Thread.currentThread().name
        val tileCountThreadNames = mutableListOf<String>()
        val state = newState(
            tileCount = { _ ->
                tileCountThreadNames.add(Thread.currentThread().name)
                REGION_TILE_COUNT
            },
        )

        state.loadDemoRegionAfterTileCacheRegistration(DEMO_REGION)

        assertEquals(REGION_TILE_COUNT, state.demoRegionTileCount)
        assertEquals(1, tileCountThreadNames.size)
        assertNotEquals(
            "the SQLite-backed tile count must run off the dispatcher that publishes state",
            callerThreadName,
            tileCountThreadNames.single(),
        )
    }

    @Test
    fun `a refused post-registration tile count is published on the caller thread`() = runBlocking {
        val callerThreadName = Thread.currentThread().name
        var failureThreadName: String? = null
        val state = newState(
            tileCount = {
                failureThreadName = Thread.currentThread().name
                throw IllegalStateException("the cache refused the region")
            },
        )

        state.loadDemoRegionAfterTileCacheRegistration(DEMO_REGION)

        assertNull(state.demoRegionTileCount)
        assertEquals("the cache refused the region", state.tileCacheRefusalReason)
        assertNotEquals(
            "the refusal must be read off the state-publishing dispatcher",
            callerThreadName,
            failureThreadName,
        )
    }

    @Test
    fun `download progress maps completed total and the network-versus-disk split`() {
        val gate = CompletableDeferred<Unit>()
        val state = newState(
            download = { _, listener ->
                listener.onRegionDownloadProgress(
                    progress(completed = 0UL, network = 0UL, disk = 0UL),
                )
                listener.onRegionDownloadProgress(
                    progress(completed = 1UL, network = 1UL, disk = 0UL),
                )
                listener.onRegionDownloadProgress(
                    progress(completed = 2UL, network = 1UL, disk = 1UL),
                )
                gate.await()
                record(presentTileCount = REGION_TILE_COUNT)
            },
        )
        state.loadDemoRegion(DEMO_REGION)
        state.downloadDemoRegion()
        awaitUntil("the scripted download's last progress report") {
            state.progress?.completedTileCount == 2UL
        }

        assertEquals(REGION_TILE_COUNT, state.demoRegionTileCount)
        assertTrue(state.isDownloading)
        assertEquals(
            "Downloading… 2 / 23 tiles · network 1 · disk 1",
            state.downloadStatusText,
        )

        gate.complete(Unit)
        awaitDownloadSettled(state)
        assertFalse(state.isDownloading)
        assertEquals(
            "Download complete · Stored tiles 23 · Stored bytes 48000",
            state.downloadStatusText,
        )
        assertTrue(state.canDownload)
    }

    @Test
    fun `cancellation ends in the cancelled terminal state with the partial count`() {
        val gate = CompletableDeferred<Unit>()
        var capturedListener: RegionDownloadProgressListener? = null
        var cancelCallCount = 0
        var cancelRegionId: String? = null
        val partialRecord = record(presentTileCount = 3UL)
        val state = newState(
            download = { _, listener ->
                capturedListener = listener
                listener.onRegionDownloadProgress(
                    progress(completed = 3UL, network = 2UL, disk = 1UL),
                )
                gate.await()
                partialRecord
            },
            cancel = { regionId ->
                cancelCallCount += 1
                cancelRegionId = regionId
                capturedListener?.onRegionDownloadProgress(
                    progress(
                        completed = 3UL,
                        network = 2UL,
                        disk = 1UL,
                        phase = RegionDownloadPhase.CANCELLED,
                    ),
                )
                gate.complete(Unit)
            },
        )
        state.loadDemoRegion(DEMO_REGION)
        state.downloadDemoRegion()
        awaitUntil("the scripted download's first progress report") {
            state.progress?.completedTileCount == 3UL
        }
        state.cancelDownload()
        awaitDownloadSettled(state)

        assertEquals(1, cancelCallCount)
        assertEquals(DEMO_REGION.regionId, cancelRegionId)
        assertFalse("the cancelled download is no longer in flight", state.isDownloading)
        assertEquals(
            "Download cancelled · 3 / 23 tiles stored",
            state.downloadStatusText,
        )
        assertEquals(3UL, state.terminalRecord?.presentTileCount)
        assertTrue("a cancelled region can resume", state.canDownload)
    }

    @Test
    fun `the stored-region list refreshes after a delete`() {
        var storedRecords = listOf(
            record(regionId = "region-a", presentTileCount = 4UL),
            record(regionId = "region-b", presentTileCount = 7UL),
        )
        var removedRegionId: String? = null
        val state = newState(
            records = { storedRecords },
            delete = { regionId ->
                removedRegionId = regionId
                storedRecords = storedRecords.filterNot { record -> record.regionId == regionId }
                RegionRemovalReport(
                    removedEntryCount = 4UL,
                    removedPayloadBytes = 1024UL,
                )
            },
        )
        state.loadDemoRegion(DEMO_REGION)
        awaitUntil("the initial stored-region listing") {
            state.storedRegionRows.size == 2
        }

        assertEquals(listOf("region-a", "region-b"), state.storedRegionRows.map { it.regionId })
        assertEquals(
            "Stored tiles 4 · Stored bytes 48000",
            state.storedRegionRows.first().footprintText,
        )
        assertEquals(
            ReferenceApplicationConstants.OFFLINE_REGION_DELETE_TEST_TAG_PREFIX + "region-a",
            state.storedRegionRows.first().deleteTestTag,
        )

        state.deleteStoredRegion("region-a")
        awaitUntil("the delete's refreshed listing") {
            state.storedRegionRows.map { it.regionId } == listOf("region-b")
        }

        assertEquals("region-a", removedRegionId)
        assertEquals(listOf("region-b"), state.storedRegionRows.map { it.regionId })
        assertEquals(
            ReferenceApplicationConstants.OFFLINE_DELETED_STATUS_TEXT,
            state.deletedStatusText,
        )
        assertEquals(4UL, state.lastRemovalReport?.removedEntryCount)
        assertEquals(1024UL, state.lastRemovalReport?.removedPayloadBytes)
    }

    @Test
    fun `no tile cache states the sdk refusal and starts no download`() {
        val refusalDetail = "no 'TileCache' provider is configured"
        var downloadCallCount = 0
        val state = newState(
            tileCount = { throw IllegalStateException(refusalDetail) },
            download = { _, _ ->
                downloadCallCount += 1
                record(presentTileCount = REGION_TILE_COUNT)
            },
            records = { throw IllegalStateException(refusalDetail) },
        )
        state.loadDemoRegion(DEMO_REGION)
        awaitUntil("the stored-region listing's refusal") {
            state.storedRegionsFailureReason != null
        }

        assertEquals(refusalDetail, state.tileCacheRefusalReason)
        assertEquals(
            "the stored-region listing states the same refusal",
            refusalDetail,
            state.storedRegionsFailureReason,
        )
        assertNull(state.demoRegionTileCount)
        assertNull("no progress is faked without a cache", state.progress)
        assertNull(state.downloadStatusText)
        assertFalse(state.canDownload)

        state.downloadDemoRegion()
        assertEquals("the card starts nothing without a cache", 0, downloadCallCount)
        assertNull(state.progress)
    }

    @Test
    fun `a failed download keeps the partial region and downloads again to resume`() {
        var downloadAttemptCount = 0
        var storedRecords: List<RegionRecord> = emptyList()
        val state = newState(
            download = { _, listener ->
                downloadAttemptCount += 1
                listener.onRegionDownloadProgress(
                    progress(completed = 1UL, network = 1UL, disk = 0UL),
                )
                if (downloadAttemptCount == 1) {
                    // The SDK keeps the fetched tile and the membership; the
                    // scripted store reports the partial record on the next
                    // listing, exactly as the real store does.
                    storedRecords = listOf(record(presentTileCount = 1UL))
                    throw IllegalStateException("provider request failed")
                }
                record(presentTileCount = REGION_TILE_COUNT)
            },
            records = { storedRecords },
        )
        state.loadDemoRegion(DEMO_REGION)
        state.downloadDemoRegion()
        awaitDownloadSettled(state)

        assertEquals(
            "Download failed: provider request failed",
            state.downloadStatusText,
        )
        assertEquals(
            "the failed prepare keeps the fetched tile count",
            1UL,
            state.progress?.completedTileCount,
        )
        assertTrue("a failed region can resume", state.canDownload)
        awaitUntil("the partial region's refreshed listing") {
            state.storedRegionRows.map { it.regionId } == listOf(DEMO_REGION.regionId)
        }

        state.downloadDemoRegion()
        awaitDownloadSettled(state)

        assertEquals("the second attempt resumed", 2, downloadAttemptCount)
        assertEquals(
            "Download complete · Stored tiles 23 · Stored bytes 48000",
            state.downloadStatusText,
        )
        assertNull(state.downloadFailureReason)
    }

    /** Waits until the state's download has left its in-flight phase. */
    private fun awaitDownloadSettled(state: OfflineRegionCardState) {
        awaitUntil("the download's terminal phase") {
            !state.isDownloading
        }
    }

    /**
     * Polls snapshot state until [condition] holds. The state's listener hop
     * is asynchronous in production; the test waits on the observable state
     * rather than assuming a dispatcher runs the hop inline.
     */
    private fun awaitUntil(description: String, condition: () -> Boolean) {
        val deadlineMilliseconds =
            System.currentTimeMillis() + DOWNLOAD_SETTLE_TIMEOUT_MILLISECONDS
        while (!condition()) {
            if (System.currentTimeMillis() > deadlineMilliseconds) {
                throw AssertionError("$description did not settle in time")
            }
            Thread.sleep(1)
        }
    }

    /** The card state over inert, scripted seams. */
    private fun newState(
        tileCount: (RegionDefinition) -> ULong = { REGION_TILE_COUNT },
        download: suspend (RegionDefinition, RegionDownloadProgressListener) -> RegionRecord = { _, _ ->
            record(presentTileCount = REGION_TILE_COUNT)
        },
        cancel: (String) -> Unit = {},
        records: suspend () -> List<RegionRecord> = { emptyList() },
        delete: suspend (String) -> RegionRemovalReport = {
            RegionRemovalReport(removedEntryCount = 0UL, removedPayloadBytes = 0UL)
        },
    ): OfflineRegionCardState = OfflineRegionCardState(
        coroutineScope = CoroutineScope(Dispatchers.Unconfined),
        regionTileCount = tileCount,
        downloadRegion = download,
        cancelRegionDownload = cancel,
        listRegionRecords = records,
        deleteRegion = delete,
        failureText = { failure -> failure.message ?: "unknown failure" },
    )

    /** One progress report with the demo region's identity. */
    private fun progress(
        completed: ULong,
        network: ULong,
        disk: ULong,
        phase: RegionDownloadPhase = RegionDownloadPhase.DOWNLOADING,
    ): RegionDownloadProgress = RegionDownloadProgress(
        regionId = DEMO_REGION.regionId,
        totalTileCount = REGION_TILE_COUNT,
        completedTileCount = completed,
        networkFetchedTileCount = network,
        diskServedTileCount = disk,
        phase = phase,
    )

    /** One stored-region record with the demo region's shape. */
    private fun record(
        regionId: String = DEMO_REGION.regionId,
        presentTileCount: ULong,
    ): RegionRecord = RegionRecord(
        regionId = regionId,
        displayName = DEMO_REGION.displayName,
        boundingBox = DEMO_REGION.boundingBox,
        minimumZoomLevel = DEMO_REGION.minimumZoomLevel,
        maximumZoomLevel = DEMO_REGION.maximumZoomLevel,
        imageFormat = DEMO_REGION.imageFormat,
        tileResolution = DEMO_REGION.tileResolution,
        effectiveTimeToLiveSeconds = DEMO_REGION.timeToLiveSeconds ?: 0UL,
        memberTileCount = REGION_TILE_COUNT,
        presentTileCount = presentTileCount,
        presentPayloadBytes = 48_000UL,
    )

    private companion object {
        const val REGION_TILE_COUNT: ULong = 23UL

        /** The longest a scripted download may take to settle in a unit test. */
        const val DOWNLOAD_SETTLE_TIMEOUT_MILLISECONDS: Long = 5_000L

        /** The shared artifact's `downtown-sacramento` demo region. */
        val DEMO_REGION: RegionDefinition = RegionDefinition(
            regionId = "downtown-sacramento",
            displayName = "Downtown Sacramento",
            boundingBox = BoundingBox(
                southWest = GeographicCoordinate(latitude = 38.566, longitude = -121.508),
                northEast = GeographicCoordinate(latitude = 38.582, longitude = -121.488),
            ),
            minimumZoomLevel = 12u.toUByte(),
            maximumZoomLevel = 16u.toUByte(),
            imageFormat = RasterImageFormat.PNG,
            tileResolution = RasterTileResolution.STANDARD,
            timeToLiveSeconds = 2_592_000UL,
        )
    }
}
