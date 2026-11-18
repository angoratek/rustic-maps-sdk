package com.rusticmaps.sdk.referenceapplication.console

import com.rusticmaps.sdk.referenceapplication.ReferenceApplicationConstants
import com.rusticmaps.sdk.referenceapplication.map.DiagnosticEventLine
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.TimeZone
import uniffi.rustic_maps.FrameStatistics
import uniffi.rustic_maps.ProviderRequestTrace
import uniffi.rustic_maps.SdkLogSeverity
import uniffi.rustic_maps.TileCacheStatistics
import uniffi.rustic_maps.TrafficFlowCacheStatistics

/**
 * One console row: the SDK event's severity, subsystem and scrubbed message,
 * plus the typed provider request trace when the event reports one. The
 * provider display name is resolved from the shared provider registry at
 * construction; an event from a provider the registry does not describe (a
 * foreign implementation, key `application`) keeps its contract-provided key.
 */
data class ProviderConsoleEntry(
    val timestampMillisecondsSinceEpoch: ULong,
    val severity: SdkLogSeverity,
    val subsystemTarget: String,
    val messageLine: String,
    val providerRequestTrace: ProviderRequestTrace?,
    val providerDisplayName: String?,
) {
    /** The name a row shows for the provider: the registry name, then its key. */
    val providerName: String?
        get() = providerDisplayName ?: providerRequestTrace?.providerKey

    companion object {
        /** Builds one console entry from the listener's diagnostics line. */
        fun fromDiagnosticEventLine(
            eventLine: DiagnosticEventLine,
            providerDisplayNamesByProviderKey: Map<String, String>,
        ): ProviderConsoleEntry {
            val trace = eventLine.providerRequestTrace
            return ProviderConsoleEntry(
                timestampMillisecondsSinceEpoch = eventLine.timestampMillisecondsSinceEpoch,
                severity = severityOf(eventLine.severity),
                subsystemTarget = eventLine.subsystemTarget,
                messageLine = eventLine.messageLine,
                providerRequestTrace = trace,
                providerDisplayName = trace?.let { requestTrace ->
                    providerDisplayNamesByProviderKey[requestTrace.providerKey]
                },
            )
        }

        private fun severityOf(severityName: String): SdkLogSeverity =
            SdkLogSeverity.entries.firstOrNull { severity -> severity.name == severityName }
                ?: SdkLogSeverity.INFO
    }
}

/** One labelled field of the console's detail view. */
data class ProviderConsoleDetailRow(
    val label: String,
    val value: String,
)

/**
 * The console's level filter. The three choices are thresholds, as the
 * provider-console chapter states: Error shows errors, Warning shows warnings
 * and errors, and Info shows all three console levels. Debug and Trace stay
 * out of the console's three-level vocabulary even after the explicit debug
 * switch lowers the SDK subscription.
 */
enum class ProviderConsoleLevelFilter(
    /** The level word the filter control shows. */
    val label: String,
) {
    ERROR(ReferenceApplicationConstants.PROVIDER_CONSOLE_FILTER_ERROR_TEXT),
    WARNING(ReferenceApplicationConstants.PROVIDER_CONSOLE_FILTER_WARNING_TEXT),
    INFO(ReferenceApplicationConstants.PROVIDER_CONSOLE_FILTER_INFO_TEXT),
    ;

    /** Whether an event of [severity] passes this threshold. */
    fun includes(severity: SdkLogSeverity): Boolean = when (this) {
        ERROR -> severity == SdkLogSeverity.ERROR
        WARNING -> severity == SdkLogSeverity.WARN || severity == SdkLogSeverity.ERROR
        INFO -> severity == SdkLogSeverity.INFO ||
            severity == SdkLogSeverity.WARN ||
            severity == SdkLogSeverity.ERROR
    }
}

/**
 * The console's pure formatting rules: row text, detail fields, the tile- and
 * traffic-cache lines, the honest empty state and the share-sheet export
 * text. Nothing here calls the SDK or reads a credential; the input is the
 * diagnostics stream's already-scrubbed records and the live cache readbacks.
 */
object ProviderConsoleFormatting {
    /** Characters of a response excerpt a row shows before truncating it. */
    const val ROW_RESPONSE_EXCERPT_CHARACTER_LIMIT: Int = 96

    /** The export text's byte ceiling (256 KiB); the share sheet owns the text. */
    const val EXPORT_MAXIMUM_BYTE_COUNT: Int = 256 * 1024

    /** The placeholder a credential-bearing assignment is replaced with. */
    const val REDACTED_PLACEHOLDER: String = "<redacted>"

    private const val EXPORT_NOTICE_SEPARATOR: String = "\n"
    private const val EXPORT_LINE_TERMINATOR: String = "\n"

    /** The filter's empty-state sentence (the honest no-events line). */
    fun emptyStateText(filter: ProviderConsoleLevelFilter): String = when (filter) {
        ProviderConsoleLevelFilter.ERROR ->
            ReferenceApplicationConstants.PROVIDER_CONSOLE_EMPTY_ERROR_TEXT
        ProviderConsoleLevelFilter.WARNING ->
            ReferenceApplicationConstants.PROVIDER_CONSOLE_EMPTY_WARNING_TEXT
        ProviderConsoleLevelFilter.INFO ->
            ReferenceApplicationConstants.PROVIDER_CONSOLE_EMPTY_INFO_TEXT
    }

    /**
     * The stream the console lists: newest first (the listener holds it oldest
     * first) with the level threshold applied.
     */
    fun filtered(
        entries: List<ProviderConsoleEntry>,
        filter: ProviderConsoleLevelFilter,
    ): List<ProviderConsoleEntry> =
        entries.asReversed().filter { entry -> filter.includes(entry.severity) }

    /** The one-line row text: a provider request row or a plain event row. */
    fun rowText(
        entry: ProviderConsoleEntry,
        timeZone: TimeZone = TimeZone.getDefault(),
    ): String {
        val trace = entry.providerRequestTrace
        if (trace != null) {
            return "${entry.providerName} · ${trace.providerContract} · " +
                "${trace.httpMethod} ${trace.requestPath} · ${trace.statusClass} · " +
                "${trace.durationMilliseconds} ms"
        }
        return "${clockText(entry.timestampMillisecondsSinceEpoch, timeZone)} · " +
            "${severityLabel(entry.severity)} · ${entry.subsystemTarget} · " +
            "${entry.messageLine}"
    }

    /**
     * The truncated response excerpt a provider row shows beneath its text, or
     * null when the event carries no captured excerpt (capture is opt-in in
     * the SDK) or the captured prefix is blank.
     */
    fun rowResponseExcerptText(entry: ProviderConsoleEntry): String? {
        val excerpt = entry.providerRequestTrace?.responseExcerpt ?: return null
        val collapsedExcerpt = excerpt.replace(WHITESPACE_PATTERN, " ").trim()
        if (collapsedExcerpt.isEmpty()) {
            return null
        }
        if (collapsedExcerpt.length <= ROW_RESPONSE_EXCERPT_CHARACTER_LIMIT) {
            return collapsedExcerpt
        }
        return collapsedExcerpt.take(ROW_RESPONSE_EXCERPT_CHARACTER_LIMIT - 1) + "…"
    }

    /** The detail view's labelled fields, in reading order. */
    fun detailRows(
        entry: ProviderConsoleEntry,
        timeZone: TimeZone = TimeZone.getDefault(),
    ): List<ProviderConsoleDetailRow> {
        val rows = mutableListOf(
            ProviderConsoleDetailRow(
                ReferenceApplicationConstants.PROVIDER_CONSOLE_DETAIL_TIME_LABEL_TEXT,
                clockText(entry.timestampMillisecondsSinceEpoch, timeZone),
            ),
            ProviderConsoleDetailRow(
                ReferenceApplicationConstants.PROVIDER_CONSOLE_DETAIL_LEVEL_LABEL_TEXT,
                severityLabel(entry.severity),
            ),
            ProviderConsoleDetailRow(
                ReferenceApplicationConstants.PROVIDER_CONSOLE_DETAIL_SUBSYSTEM_LABEL_TEXT,
                entry.subsystemTarget,
            ),
        )
        entry.providerRequestTrace?.let { trace ->
            rows += ProviderConsoleDetailRow(
                ReferenceApplicationConstants.PROVIDER_CONSOLE_DETAIL_PROVIDER_LABEL_TEXT,
                entry.providerName.orEmpty(),
            )
            rows += ProviderConsoleDetailRow(
                ReferenceApplicationConstants.PROVIDER_CONSOLE_DETAIL_PROVIDER_KEY_LABEL_TEXT,
                trace.providerKey,
            )
            rows += ProviderConsoleDetailRow(
                ReferenceApplicationConstants.PROVIDER_CONSOLE_DETAIL_CONTRACT_LABEL_TEXT,
                trace.providerContract,
            )
            rows += ProviderConsoleDetailRow(
                ReferenceApplicationConstants.PROVIDER_CONSOLE_DETAIL_METHOD_LABEL_TEXT,
                trace.httpMethod,
            )
            rows += ProviderConsoleDetailRow(
                ReferenceApplicationConstants.PROVIDER_CONSOLE_DETAIL_PATH_LABEL_TEXT,
                trace.requestPath,
            )
            rows += ProviderConsoleDetailRow(
                ReferenceApplicationConstants.PROVIDER_CONSOLE_DETAIL_STATUS_LABEL_TEXT,
                trace.statusClass,
            )
            rows += ProviderConsoleDetailRow(
                ReferenceApplicationConstants.PROVIDER_CONSOLE_DETAIL_OUTCOME_LABEL_TEXT,
                trace.outcome,
            )
            rows += ProviderConsoleDetailRow(
                ReferenceApplicationConstants.PROVIDER_CONSOLE_DETAIL_DURATION_LABEL_TEXT,
                "${trace.durationMilliseconds} ms",
            )
            rows += ProviderConsoleDetailRow(
                ReferenceApplicationConstants.PROVIDER_CONSOLE_DETAIL_BYTES_LABEL_TEXT,
                "${trace.responseBytes} bytes",
            )
            rows += ProviderConsoleDetailRow(
                ReferenceApplicationConstants.PROVIDER_CONSOLE_DETAIL_EXCERPT_LABEL_TEXT,
                trace.responseExcerpt
                    ?: ReferenceApplicationConstants.PROVIDER_CONSOLE_EXCERPT_NOT_CAPTURED_TEXT,
            )
        }
        rows += ProviderConsoleDetailRow(
            ReferenceApplicationConstants.PROVIDER_CONSOLE_DETAIL_MESSAGE_LABEL_TEXT,
            entry.messageLine,
        )
        return rows
    }

    /**
     * The tile-cache line: the installed payload cache's exact hits, misses
     * and occupancy. The same store answers both the raster base and the
     * vector base, so the counters cover every payload lookup. An absent
     * readback means no tile cache is installed, and the line says so instead
     * of showing zeros.
     */
    fun tileCacheRowText(statistics: TileCacheStatistics?): String {
        if (statistics == null) {
            return ReferenceApplicationConstants.PROVIDER_CONSOLE_TILE_CACHE_EMPTY_TEXT
        }
        val occupancyText = statistics.storedByteCount?.let { storedByteCount ->
            ReferenceApplicationConstants.PROVIDER_CONSOLE_TILE_CACHE_OCCUPANCY_TEMPLATE.format(
                storedByteCount.toLong(),
            )
        } ?: ReferenceApplicationConstants.PROVIDER_CONSOLE_TILE_CACHE_OCCUPANCY_UNAVAILABLE_TEXT
        return ReferenceApplicationConstants.PROVIDER_CONSOLE_TILE_CACHE_TEMPLATE.format(
            statistics.hitCount.toLong(),
            statistics.missCount.toLong(),
            occupancyText,
        )
    }

    /**
     * The last-frame line: the settled frame's tile pipeline accounting, kept
     * apart from the cache counters because it describes the frame, not the
     * store.
     */
    fun tileFrameRowText(statistics: FrameStatistics?): String {
        if (statistics == null) {
            return ReferenceApplicationConstants.PROVIDER_CONSOLE_TILE_FRAME_EMPTY_TEXT
        }
        return ReferenceApplicationConstants.PROVIDER_CONSOLE_TILE_FRAME_TEMPLATE.format(
            statistics.requestedTileCount.toLong(),
            statistics.presentTileCount.toLong(),
            statistics.fallbackTileCount.toLong(),
            statistics.unavailableTileCount.toLong(),
        )
    }

    /**
     * The traffic-cache line: the serving cache's hits, misses, held cells and
     * evictions, prefixed by the serving provider's display name. An absent
     * statistic means no traffic provider is registered or the provider owns
     * its freshness policy; the line says so instead of showing zeros.
     */
    fun trafficCacheRowText(
        statistics: TrafficFlowCacheStatistics?,
        providerDisplayName: String?,
    ): String {
        if (statistics == null) {
            return ReferenceApplicationConstants.PROVIDER_CONSOLE_TRAFFIC_CACHE_EMPTY_TEXT
        }
        return ReferenceApplicationConstants.PROVIDER_CONSOLE_TRAFFIC_CACHE_TEMPLATE.format(
            providerDisplayName
                ?: ReferenceApplicationConstants.PROVIDER_CONSOLE_TRAFFIC_PROVIDER_FALLBACK_TEXT,
            statistics.hitCount.toLong(),
            statistics.missCount.toLong(),
            statistics.entryCount.toLong(),
            statistics.evictionCount.toLong(),
            statistics.gridZoomLevel.toLong(),
            statistics.maximumEntryCount.toLong(),
            statistics.timeToLiveSeconds.toLong(),
        )
    }

    /**
     * The share-sheet export: the filtered rows newest first, each provider
     * row followed by its typed outcome/size/excerpt line. The text is capped
     * at [EXPORT_MAXIMUM_BYTE_COUNT] and says when it truncates. A final
     * defensive pass redacts credential-shaped assignments, so a regression in
     * an upstream event can never put a credential into a shared file.
     */
    fun exportText(
        entries: List<ProviderConsoleEntry>,
        filter: ProviderConsoleLevelFilter,
    ): String {
        val visibleEntries = filtered(entries, filter)
        val noticeBytes = (
            EXPORT_NOTICE_SEPARATOR +
                ReferenceApplicationConstants.PROVIDER_CONSOLE_EXPORT_TRUNCATION_NOTICE_TEXT +
                EXPORT_LINE_TERMINATOR
            ).toByteArray(Charsets.UTF_8).size
        val lineBudget = EXPORT_MAXIMUM_BYTE_COUNT - noticeBytes
        val builder = CappedExportBuilder(lineBudget)

        builder.appendLine(ReferenceApplicationConstants.PROVIDER_CONSOLE_EXPORT_HEADER_TEXT)
        builder.appendLine("${ReferenceApplicationConstants.PROVIDER_CONSOLE_DETAIL_LEVEL_LABEL_TEXT}: ${filter.label}")
        builder.appendLine(
            "${ReferenceApplicationConstants.PROVIDER_CONSOLE_EXPORT_EVENTS_LABEL_TEXT}: " +
                "${visibleEntries.size}",
        )
        builder.appendLine("")
        var wasTruncated = false
        for (entry in visibleEntries) {
            if (!builder.appendLine(credentialFreeText(rowText(entry)))) {
                wasTruncated = true
                break
            }
            entry.providerRequestTrace?.let { trace ->
                val excerpt = trace.responseExcerpt
                    ?: ReferenceApplicationConstants.PROVIDER_CONSOLE_EXCERPT_NOT_CAPTURED_TEXT
                val traceLine = "  ${ReferenceApplicationConstants.PROVIDER_CONSOLE_DETAIL_OUTCOME_LABEL_TEXT}: " +
                    "${trace.outcome} · ${ReferenceApplicationConstants.PROVIDER_CONSOLE_DETAIL_BYTES_LABEL_TEXT}: " +
                    "${trace.responseBytes} · ${ReferenceApplicationConstants.PROVIDER_CONSOLE_DETAIL_EXCERPT_LABEL_TEXT}: " +
                    "${excerpt.replace(WHITESPACE_PATTERN, " ").trim()}"
                if (!builder.appendLine(credentialFreeText(traceLine))) {
                    wasTruncated = true
                    break
                }
            }
        }
        if (wasTruncated) {
            builder.appendLine(
                ReferenceApplicationConstants.PROVIDER_CONSOLE_EXPORT_TRUNCATION_NOTICE_TEXT,
            )
        }
        return builder.build()
    }

    /** The `hh:mm:ss` clock text the console's event rows show. */
    fun clockText(
        timestampMillisecondsSinceEpoch: ULong,
        timeZone: TimeZone = TimeZone.getDefault(),
    ): String {
        val formatter = SimpleDateFormat("HH:mm:ss", Locale.US)
        formatter.timeZone = timeZone
        val timestampMilliseconds = timestampMillisecondsSinceEpoch.toLong()
        return formatter.format(Date(timestampMilliseconds))
    }

    /** The console's level word for a severity. */
    fun severityLabel(severity: SdkLogSeverity): String = when (severity) {
        SdkLogSeverity.TRACE -> "Trace"
        SdkLogSeverity.DEBUG -> "Debug"
        SdkLogSeverity.INFO -> ReferenceApplicationConstants.PROVIDER_CONSOLE_FILTER_INFO_TEXT
        SdkLogSeverity.WARN -> ReferenceApplicationConstants.PROVIDER_CONSOLE_FILTER_WARNING_TEXT
        SdkLogSeverity.ERROR -> ReferenceApplicationConstants.PROVIDER_CONSOLE_FILTER_ERROR_TEXT
    }

    /**
     * The export's defensive credential scrub. Provider paths and excerpts are
     * scrubbed upstream; this second pass protects the exported text if an
     * event ever regresses. It replaces the value of a credential-shaped
     * `name=value` or `"name": "value"` assignment and everything after an
     * `Authorization:` header.
     */
    fun credentialFreeText(text: String): String {
        val withoutAuthorizationHeader = AUTHORIZATION_HEADER_PATTERN.replace(text) { match ->
            match.groupValues[1] + REDACTED_PLACEHOLDER
        }
        val withoutBearerToken = BEARER_TOKEN_PATTERN.replace(withoutAuthorizationHeader) { match ->
            match.groupValues[1] + REDACTED_PLACEHOLDER
        }
        return CREDENTIAL_ASSIGNMENT_PATTERN.replace(withoutBearerToken) { match ->
            val name = match.groupValues[1]
            val separator = match.groupValues[2]
            val openingQuote = match.groupValues[3]
            val closingQuote = if (openingQuote.isEmpty()) "" else "\""
            name + separator + openingQuote + REDACTED_PLACEHOLDER + closingQuote
        }
    }

    /** Collapses a captured excerpt's line breaks before it renders or exports. */
    private val WHITESPACE_PATTERN: Regex = Regex("\\s+")

    private val AUTHORIZATION_HEADER_PATTERN: Regex =
        Regex("(?i)(authorization\\s*:\\s*)([^\\n\\r]*)")

    private val BEARER_TOKEN_PATTERN: Regex = Regex("(?i)(bearer\\s+)(\\S+)")

    private val CREDENTIAL_ASSIGNMENT_PATTERN: Regex = Regex(
        "(?i)\\b(api[_-]?key|apikey|access[_-]?token|token|authorization|key)" +
            "(\"?\\s*[=:]\\s*)(\"?)([^\"&\\s,}]+)",
    )
}

/**
 * Appends lines to the export text while their UTF-8 bytes fit the budget.
 * A false answer means the line did not fit and the caller stops.
 */
private class CappedExportBuilder(private val lineBudgetBytes: Int) {
    private val builder = StringBuilder()
    private var writtenByteCount: Int = 0

    fun appendLine(line: String): Boolean {
        val lineBytes = (line + "\n").toByteArray(Charsets.UTF_8).size
        if (writtenByteCount + lineBytes > lineBudgetBytes) {
            return false
        }
        builder.append(line).append('\n')
        writtenByteCount += lineBytes
        return true
    }

    fun build(): String = builder.toString()
}
