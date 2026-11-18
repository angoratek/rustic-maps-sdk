package com.rusticmaps.sdk.referenceapplication.console

import com.rusticmaps.sdk.referenceapplication.ReferenceApplicationConstants
import com.rusticmaps.sdk.referenceapplication.map.DiagnosticEventLine
import java.util.TimeZone
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.rustic_maps.FrameStatistics
import uniffi.rustic_maps.ProviderRequestTrace
import uniffi.rustic_maps.SdkLogSeverity
import uniffi.rustic_maps.TileCacheStatistics
import uniffi.rustic_maps.TrafficFlowCacheStatistics

/**
 * Unit tests of the developer console's pure rules: the newest-first
 * threshold filter, the provider/event row text, the typed detail fields, the
 * live cache lines, the honest empty state and the credential-free export.
 */
class ProviderConsoleFormattingTest {
    @Test
    fun `entries are newest first and the three levels are thresholds`() {
        val infoEntry = eventEntry(index = 1, severity = SdkLogSeverity.INFO)
        val warningEntry = eventEntry(index = 2, severity = SdkLogSeverity.WARN)
        val errorEntry = eventEntry(index = 3, severity = SdkLogSeverity.ERROR)
        val debugEntry = eventEntry(index = 4, severity = SdkLogSeverity.DEBUG)
        val entries = listOf(infoEntry, warningEntry, errorEntry, debugEntry)

        assertEquals(
            listOf(errorEntry, warningEntry, infoEntry),
            ProviderConsoleFormatting.filtered(entries, ProviderConsoleLevelFilter.INFO),
        )
        assertEquals(
            listOf(errorEntry, warningEntry),
            ProviderConsoleFormatting.filtered(entries, ProviderConsoleLevelFilter.WARNING),
        )
        assertEquals(
            listOf(errorEntry),
            ProviderConsoleFormatting.filtered(entries, ProviderConsoleLevelFilter.ERROR),
        )
    }

    @Test
    fun `a provider row names the provider contract method status duration and excerpt`() {
        val trace = providerTrace(
            responseExcerpt = "{\"routes\":[]}",
        )
        val entry = traceEntry(
            trace = trace,
            providerDisplayNames = mapOf("provider-key" to "Example Maps"),
        )

        assertEquals(
            "Example Maps · RoutingProvider · GET /route · 2xx · 42 ms",
            ProviderConsoleFormatting.rowText(entry),
        )
        assertEquals(
            "{\"routes\":[]}",
            ProviderConsoleFormatting.rowResponseExcerptText(entry),
        )
    }

    @Test
    fun `a row excerpt truncates on the row limit and collapses whitespace`() {
        val longExcerpt = "line one\nline two " + "x".repeat(200)
        val entry = traceEntry(
            trace = providerTrace(responseExcerpt = longExcerpt),
            providerDisplayNames = emptyMap(),
        )
        val rowExcerpt = requireNotNull(
            ProviderConsoleFormatting.rowResponseExcerptText(entry),
        )
        assertEquals(
            ProviderConsoleFormatting.ROW_RESPONSE_EXCERPT_CHARACTER_LIMIT - 1,
            rowExcerpt.trimEnd('…').length,
        )
        assertTrue(rowExcerpt.startsWith("line one line two"))
        assertTrue(rowExcerpt.endsWith("…"))
    }

    @Test
    fun `a plain event row keeps the scrubbed message and its clock`() {
        val entry = eventEntry(index = 0, severity = SdkLogSeverity.WARN)

        val rowText = ProviderConsoleFormatting.rowText(
            entry,
            TimeZone.getTimeZone("UTC"),
        )

        assertTrue(rowText.startsWith("00:00:00 · Warning · tile_fetch · "))
        assertTrue(rowText.endsWith("event 0"))
    }

    @Test
    fun `the detail view exposes every typed trace field`() {
        val entry = traceEntry(
            trace = providerTrace(responseExcerpt = "{\"routes\":[]}"),
            providerDisplayNames = mapOf("provider-key" to "Example Maps"),
        )

        val detailValues = ProviderConsoleFormatting.detailRows(entry)
            .associate { detailRow -> detailRow.label to detailRow.value }

        assertEquals("Example Maps", detailValues["Provider"])
        assertEquals("provider-key", detailValues["Provider key"])
        assertEquals("RoutingProvider", detailValues["Contract"])
        assertEquals("GET", detailValues["Method"])
        assertEquals("/route", detailValues["Request path"])
        assertEquals("2xx", detailValues["Status class"])
        assertEquals("success", detailValues["Outcome"])
        assertEquals("42 ms", detailValues["Duration"])
        assertEquals("128 bytes", detailValues["Response bytes"])
        assertEquals("{\"routes\":[]}", detailValues["Response excerpt"])
    }

    @Test
    fun `the cache rows read the live statistics and say when one is absent`() {
        assertEquals(
            "Tile cache · hits 6 · misses 2 · stored 4096 bytes",
            ProviderConsoleFormatting.tileCacheRowText(
                TileCacheStatistics(
                    hitCount = 6UL,
                    missCount = 2UL,
                    storedByteCount = 4096UL,
                ),
            ),
        )
        assertEquals(
            "Tile cache · hits 6 · misses 2 · stored unavailable",
            ProviderConsoleFormatting.tileCacheRowText(
                TileCacheStatistics(
                    hitCount = 6UL,
                    missCount = 2UL,
                    storedByteCount = null,
                ),
            ),
        )
        assertEquals(
            ReferenceApplicationConstants.PROVIDER_CONSOLE_TILE_CACHE_EMPTY_TEXT,
            ProviderConsoleFormatting.tileCacheRowText(null),
        )
        assertEquals(
            "Last frame · requested 4 · drawn 3 · fallback 1 · missing 2",
            ProviderConsoleFormatting.tileFrameRowText(
                FrameStatistics(
                    requestedTileCount = 4u,
                    presentTileCount = 3u,
                    fallbackTileCount = 1u,
                    deferredTileCount = 0u,
                    unavailableTileCount = 2u,
                    overlayTriangleCount = 0u,
                ),
            ),
        )
        assertEquals(
            ReferenceApplicationConstants.PROVIDER_CONSOLE_TILE_FRAME_EMPTY_TEXT,
            ProviderConsoleFormatting.tileFrameRowText(null),
        )
        assertEquals(
            "Traffic cache · Example Maps · hits 2 · misses 1 · held cells 5 · evictions 0 · " +
                "grid zoom 12 · entry budget 64 · TTL 300 s",
            ProviderConsoleFormatting.trafficCacheRowText(
                TrafficFlowCacheStatistics(
                    hitCount = 2UL,
                    missCount = 1UL,
                    entryCount = 5UL,
                    evictionCount = 0UL,
                    segmentCount = 7UL,
                    gridZoomLevel = 12u,
                    maximumEntryCount = 64UL,
                    timeToLiveSeconds = 300UL,
                ),
                providerDisplayName = "Example Maps",
            ),
        )
        assertEquals(
            ReferenceApplicationConstants.PROVIDER_CONSOLE_TRAFFIC_CACHE_EMPTY_TEXT,
            ProviderConsoleFormatting.trafficCacheRowText(null, providerDisplayName = null),
        )
    }

    @Test
    fun `the empty state names the filter it describes`() {
        assertEquals(
            ReferenceApplicationConstants.PROVIDER_CONSOLE_EMPTY_ERROR_TEXT,
            ProviderConsoleFormatting.emptyStateText(ProviderConsoleLevelFilter.ERROR),
        )
        assertEquals(
            ReferenceApplicationConstants.PROVIDER_CONSOLE_EMPTY_WARNING_TEXT,
            ProviderConsoleFormatting.emptyStateText(ProviderConsoleLevelFilter.WARNING),
        )
        assertEquals(
            ReferenceApplicationConstants.PROVIDER_CONSOLE_EMPTY_INFO_TEXT,
            ProviderConsoleFormatting.emptyStateText(ProviderConsoleLevelFilter.INFO),
        )
    }

    @Test
    fun `the export carries the filtered rows newest first and no credential`() {
        val credentialBearingEntry = eventEntry(
            index = 2,
            severity = SdkLogSeverity.ERROR,
            messageLine = "event 2 request failed: api_key=TOP-SECRET&x=1 " +
                "Authorization: Bearer TOP-SECRET",
        )
        val trace = traceEntry(
            trace = providerTrace(
                requestPath = "/route?key=TOP-SECRET",
                responseExcerpt = "{\"api_key\":\"TOP-SECRET\"}",
            ),
            providerDisplayNames = mapOf("provider-key" to "Example Maps"),
        )
        val infoEntry = eventEntry(index = 1, severity = SdkLogSeverity.INFO)
        val entries = listOf(infoEntry, trace, credentialBearingEntry)

        val exportText = ProviderConsoleFormatting.exportText(
            entries,
            ProviderConsoleLevelFilter.INFO,
        )

        assertFalse(exportText.contains("TOP-SECRET"))
        assertTrue(exportText.contains(ProviderConsoleFormatting.REDACTED_PLACEHOLDER))
        assertTrue(exportText.contains("Example Maps · RoutingProvider"))
        assertTrue(exportText.indexOf("event 2") < exportText.indexOf("event 1"))
        assertFalse(
            "the Error-only export omits the Info row",
            ProviderConsoleFormatting.exportText(
                entries,
                ProviderConsoleLevelFilter.ERROR,
            ).contains("event 1"),
        )
    }

    @Test
    fun `the export cap is honoured and reports its truncation`() {
        val entries = (0 until 4000).map { index ->
            eventEntry(
                index = index,
                severity = SdkLogSeverity.INFO,
                messageLine = "event $index " + "y".repeat(200),
            )
        }

        val exportText = ProviderConsoleFormatting.exportText(
            entries,
            ProviderConsoleLevelFilter.INFO,
        )

        assertTrue(
            exportText.toByteArray(Charsets.UTF_8).size <=
                ProviderConsoleFormatting.EXPORT_MAXIMUM_BYTE_COUNT,
        )
        assertTrue(
            exportText.contains(
                ReferenceApplicationConstants.PROVIDER_CONSOLE_EXPORT_TRUNCATION_NOTICE_TEXT,
            ),
        )
    }

    @Test
    fun `the credential scrub covers assignments JSON members and headers`() {
        val scrubbed = ProviderConsoleFormatting.credentialFreeText(
            "a api_key=one b \"apikey\":\"two\" d token=four " +
                "e retry with Bearer five c Authorization: Bearer three",
        )

        assertFalse(scrubbed.contains("one"))
        assertFalse(scrubbed.contains("two"))
        assertFalse(scrubbed.contains("three"))
        assertFalse(scrubbed.contains("four"))
        assertFalse(scrubbed.contains("five"))
        assertTrue(scrubbed.contains("<redacted>"))
    }

    @Test
    fun `the clock text formats a UTC timestamp`() {
        assertEquals(
            "00:00:00",
            ProviderConsoleFormatting.clockText(
                0UL,
                TimeZone.getTimeZone("UTC"),
            ),
        )
        assertNull(
            ProviderConsoleFormatting.rowResponseExcerptText(
                eventEntry(index = 0, severity = SdkLogSeverity.INFO),
            ),
        )
    }

    private fun eventEntry(
        index: Int,
        severity: SdkLogSeverity,
        messageLine: String = "event $index",
    ): ProviderConsoleEntry = ProviderConsoleEntry.fromDiagnosticEventLine(
        DiagnosticEventLine(
            subsystemTarget = "tile_fetch",
            severity = severity.name,
            messageLine = messageLine,
            timestampMillisecondsSinceEpoch = 0UL,
            providerRequestTrace = null,
        ),
        providerDisplayNamesByProviderKey = emptyMap(),
    )

    private fun traceEntry(
        trace: ProviderRequestTrace,
        providerDisplayNames: Map<String, String>,
    ): ProviderConsoleEntry = ProviderConsoleEntry.fromDiagnosticEventLine(
        DiagnosticEventLine(
            subsystemTarget = "provider_request",
            severity = SdkLogSeverity.INFO.name,
            messageLine = "provider request settled",
            timestampMillisecondsSinceEpoch = 0UL,
            providerRequestTrace = trace,
        ),
        providerDisplayNamesByProviderKey = providerDisplayNames,
    )

    private fun providerTrace(
        requestPath: String = "/route",
        responseExcerpt: String? = null,
    ): ProviderRequestTrace = ProviderRequestTrace(
        providerKey = "provider-key",
        providerContract = "RoutingProvider",
        httpMethod = "GET",
        requestPath = requestPath,
        statusClass = "2xx",
        outcome = "success",
        durationMilliseconds = 42UL,
        responseBytes = 128UL,
        responseExcerpt = responseExcerpt,
    )
}
