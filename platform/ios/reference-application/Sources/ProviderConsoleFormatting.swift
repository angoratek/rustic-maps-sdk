//! The provider console's text: row lines, detail sections and the export
//! payload (docs/Browser.md "Provider request console").
//! Everything here is pure so the formatting, the filter, the honest cache
//! rows and the credential-free export are unit-testable without a view.
//!
//! A provider row never parses the composed message line: it reads the typed
//! `ProviderRequestTrace` the contract delivers. The one place the composed
//! line is used is an event that is not a provider request (a renderer or
//! cache line), and that line is already scrubbed by the SDK's sink.

import Foundation
import MapNativeCoreUI
import RusticMapsSDK

/// One labeled detail row.
struct ProviderConsoleDetailRow: Equatable, Identifiable {
    let label: String
    let value: String

    var id: String { label }
}

/// One titled detail section.
struct ProviderConsoleDetailSection: Equatable, Identifiable {
    let title: String
    let rows: [ProviderConsoleDetailRow]

    var id: String { title }
}

/// The console's text formatting, filtering and export.
enum ProviderConsoleFormatting {
    /// The characters a provider row shows from the response excerpt before
    /// its detail view (the event itself caps the excerpt at 512 bytes).
    static let rowExcerptCharacterLimit: Int =
        ReferenceApplicationConfiguration.providerConsoleRowExcerptCharacterLimit

    /// The ellipsis a truncated row excerpt ends with.
    static let truncationEllipsis = "…"

    /// The section titles and row labels of the detail view.
    private static let requestSectionTitle = "Request"
    private static let eventSectionTitle = "Event"
    private static let tileCacheSectionTitle = "Tile cache"
    private static let trafficCacheSectionTitle =
        ReferenceApplicationConfiguration.trafficCacheDiagnosticsBlockTitle

    /// The sections a detail view renders for one event: the typed trace
    /// fields (or the generic event facts), then the live tile-cache and
    /// traffic-cache readbacks.
    static func detailSections(
        for event: LogEvent,
        descriptors: [ProviderDescriptor],
        tileCacheState: OfflineTileCacheState?,
        tileCacheUncachedNote: String?,
        tileStatistics: FrameStatistics?,
        tileCacheStatistics: TileCacheStatistics?,
        trafficCacheStatistics: TrafficFlowCacheStatistics?
    ) -> [ProviderConsoleDetailSection] {
        [
            requestSection(
                for: event,
                descriptors: descriptors
            ),
            tileCacheSection(
                state: tileCacheState,
                uncachedNote: tileCacheUncachedNote,
                statistics: tileStatistics,
                cacheStatistics: tileCacheStatistics
            ),
            trafficCacheSection(statistics: trafficCacheStatistics),
        ]
    }

    /// The event's provider request section, or the generic event facts when
    /// the event is not a provider request.
    private static func requestSection(
        for event: LogEvent,
        descriptors: [ProviderDescriptor]
    ) -> ProviderConsoleDetailSection {
        guard let trace = event.providerRequestTrace else {
            return ProviderConsoleDetailSection(
                title: eventSectionTitle,
                rows: [
                    ProviderConsoleDetailRow(
                        label: "Time",
                        value: eventTimeText(event)
                    ),
                    ProviderConsoleDetailRow(
                        label: "Severity",
                        value: ReferenceApplicationConfiguration
                            .diagnosticsSeverityLabel(event.severity)
                    ),
                    ProviderConsoleDetailRow(
                        label: "Subsystem",
                        value: event.subsystemTarget
                    ),
                    ProviderConsoleDetailRow(
                        label: "Message",
                        value: event.messageLine
                    ),
                ]
            )
        }
        var rows: [ProviderConsoleDetailRow] = [
            ProviderConsoleDetailRow(
                label: "Provider",
                value: displayName(
                    forProviderKey: trace.providerKey,
                    descriptors: descriptors
                )
            ),
            ProviderConsoleDetailRow(
                label: "Provider key",
                value: trace.providerKey
            ),
            ProviderConsoleDetailRow(
                label: "Contract",
                value: trace.providerContract
            ),
            ProviderConsoleDetailRow(
                label: "Method",
                value: trace.httpMethod
            ),
            ProviderConsoleDetailRow(
                label: "Path",
                value: trace.requestPath
            ),
            ProviderConsoleDetailRow(
                label: "Status class",
                value: trace.statusClass
            ),
            ProviderConsoleDetailRow(
                label: "Outcome",
                value: trace.outcome
            ),
            ProviderConsoleDetailRow(
                label: "Duration",
                value: "\(trace.durationMilliseconds) ms"
            ),
            ProviderConsoleDetailRow(
                label: "Response bytes",
                value: "\(trace.responseBytes)"
            ),
        ]
        rows.append(
            ProviderConsoleDetailRow(
                label: "Response excerpt",
                value: trace.responseExcerpt
                    ?? ReferenceApplicationConfiguration
                        .providerConsoleResponseExcerptOffText
            )
        )
        return ProviderConsoleDetailSection(title: requestSectionTitle, rows: rows)
    }

    /// The tile-cache section: the registration state, the installed store's
    /// exact payload counters and, on a separate row, the last frame's tile
    /// pipeline counts. The counters describe the cache; the frame counts
    /// describe what the frame drew, so they never stand in for each other.
    private static func tileCacheSection(
        state: OfflineTileCacheState?,
        uncachedNote: String?,
        statistics: FrameStatistics?,
        cacheStatistics: TileCacheStatistics?
    ) -> ProviderConsoleDetailSection {
        var rows: [ProviderConsoleDetailRow] = []
        if let state {
            rows.append(
                ProviderConsoleDetailRow(
                    label: "Status",
                    value: "registered (\(state.budgetPhrase))"
                )
            )
        } else if let uncachedNote {
            rows.append(
                ProviderConsoleDetailRow(label: "Status", value: uncachedNote)
            )
        } else {
            rows.append(
                ProviderConsoleDetailRow(
                    label: "Status",
                    value: ReferenceApplicationConfiguration
                        .providerConsoleTileCacheUnregisteredText
                )
            )
        }
        if let cacheStatistics {
            rows.append(contentsOf: [
                ProviderConsoleDetailRow(
                    label: "Hits",
                    value: "\(cacheStatistics.hitCount)"
                ),
                ProviderConsoleDetailRow(
                    label: "Misses",
                    value: "\(cacheStatistics.missCount)"
                ),
                ProviderConsoleDetailRow(
                    label: "Stored bytes",
                    value: cacheStatistics.storedByteCount.map { "\($0)" }
                        ?? ReferenceApplicationConfiguration
                            .providerConsoleTileCacheUnavailableValueText
                ),
            ])
        } else {
            rows.append(
                ProviderConsoleDetailRow(
                    label: "Counters",
                    value: ReferenceApplicationConfiguration
                        .providerConsoleTileCacheUnavailableValueText
                )
            )
        }
        if let statistics {
            rows.append(
                ProviderConsoleDetailRow(
                    label: ReferenceApplicationConfiguration
                        .providerConsoleTileFrameRowLabel,
                    value: "\(statistics.requestedTileCount) requested · "
                        + "\(statistics.presentTileCount) drawn · "
                        + "\(statistics.fallbackTileCount) parent fallback · "
                        + "\(statistics.unavailableTileCount) unavailable"
                )
            )
        } else {
            rows.append(
                ProviderConsoleDetailRow(
                    label: ReferenceApplicationConfiguration
                        .providerConsoleTileFrameRowLabel,
                    value: ReferenceApplicationConfiguration
                        .tileStatisticsUnpreparedText
                )
            )
        }
        return ProviderConsoleDetailSection(
            title: tileCacheSectionTitle,
            rows: rows
        )
    }

    /// The traffic-cache section: the live hit/miss counters beside the held
    /// cells and their budget, or the honest absence while no traffic
    /// provider serves.
    private static func trafficCacheSection(
        statistics: TrafficFlowCacheStatistics?
    ) -> ProviderConsoleDetailSection {
        guard let statistics else {
            return ProviderConsoleDetailSection(
                title: trafficCacheSectionTitle,
                rows: [
                    ProviderConsoleDetailRow(
                        label: "Status",
                        value: ReferenceApplicationConfiguration
                            .providerConsoleTrafficCacheUnavailableText
                    ),
                ]
            )
        }
        return ProviderConsoleDetailSection(
            title: trafficCacheSectionTitle,
            rows: [
                ProviderConsoleDetailRow(
                    label: "Hits",
                    value: "\(statistics.hitCount)"
                ),
                ProviderConsoleDetailRow(
                    label: "Misses",
                    value: "\(statistics.missCount)"
                ),
                ProviderConsoleDetailRow(
                    label: "Held cells",
                    value: "\(statistics.entryCount) of \(statistics.maximumEntryCount)"
                ),
                ProviderConsoleDetailRow(
                    label: "Evictions",
                    value: "\(statistics.evictionCount)"
                ),
                ProviderConsoleDetailRow(
                    label: "Grid zoom",
                    value: "z\(statistics.gridZoomLevel)"
                ),
                ProviderConsoleDetailRow(
                    label: "Time to live",
                    value: "\(statistics.timeToLiveSeconds) s"
                ),
            ]
        )
    }

    // MARK: Rows

    /// One console row. A provider request names the provider, the contract,
    /// the method, the status class and the duration, with the truncated
    /// response excerpt beneath it; every other event renders the scrubbed
    /// line the diagnostics pane shows.
    static func rowText(
        for event: LogEvent,
        descriptors: [ProviderDescriptor]
    ) -> String {
        let eventTime = eventTimeText(event)
        guard let trace = event.providerRequestTrace else {
            return eventTime
                + " · "
                + ReferenceApplicationConfiguration
                    .diagnosticsSeverityLabel(event.severity)
                + " · "
                + event.subsystemTarget
                + " · "
                + event.messageLine
        }
        let requestLine = eventTime
            + " · "
            + displayName(
                forProviderKey: trace.providerKey,
                descriptors: descriptors
            )
            + " · "
            + trace.providerContract
            + " · "
            + trace.httpMethod
            + " "
            + trace.requestPath
            + " · "
            + trace.statusClass
            + " · "
            + "\(trace.durationMilliseconds) ms"
        guard let excerpt = rowExcerpt(trace.responseExcerpt) else {
            return requestLine
        }
        return requestLine + "\n" + excerpt
    }

    /// The excerpt's one-line row rendering: whitespace collapsed, truncated
    /// on a character boundary.
    static func rowExcerpt(_ excerpt: String?) -> String? {
        guard let excerpt else { return nil }
        let collapsed = excerpt
            .split(whereSeparator: \.isWhitespace)
            .joined(separator: " ")
        guard !collapsed.isEmpty else { return nil }
        guard collapsed.count > rowExcerptCharacterLimit else {
            return collapsed
        }
        return String(collapsed.prefix(rowExcerptCharacterLimit))
            + truncationEllipsis
    }

    // MARK: Filtering and empty state

    /// The events the level filter shows, newest first (the feed's own
    /// order).
    static func visibleEvents(
        _ events: [LogEvent],
        filter: ProviderConsoleLevelFilter
    ) -> [LogEvent] {
        events.filter { event in
            ReferenceApplicationConfiguration.diagnosticsSeverityMeets(
                event.severity,
                minimum: filter.minimumSeverity
            )
        }
    }

    /// The honest empty state: no event is recorded yet, and the actions that
    /// issue a provider request are named.
    static var emptyStateText: String {
        ReferenceApplicationConfiguration.providerConsoleEmptyText
    }

    // MARK: Export

    /// The exported text for the visible rows. It is composed only from the
    /// console's fields, redacted against the shared token vocabulary, and
    /// bounded to the named byte limit with an honest truncation note.
    static func exportText(
        for events: [LogEvent],
        descriptors: [ProviderDescriptor],
        filter: ProviderConsoleLevelFilter
    ) -> String {
        var lines: [String] = [
            ReferenceApplicationConfiguration.providerConsoleExportHeaderText,
            ReferenceApplicationConfiguration
                .providerConsoleExportFilterPrefix + filter.label,
            ReferenceApplicationConfiguration
                .providerConsoleExportRowCountPrefix + "\(events.count)",
            "",
        ]
        for event in events {
            lines.append(rowText(for: event, descriptors: descriptors))
            if let trace = event.providerRequestTrace {
                lines.append("  outcome \(trace.outcome)")
                lines.append("  path \(trace.requestPath)")
                lines.append("  response bytes \(trace.responseBytes)")
                if let excerpt = trace.responseExcerpt {
                    lines.append("  excerpt \(excerpt)")
                }
            }
            lines.append("")
        }
        let credentialFreeText = ProviderConsoleRedaction.credentialFree(
            lines.joined(separator: "\n")
        )
        return boundedForExport(credentialFreeText)
    }

    /// Caps the export at the named UTF-8 byte budget, taking whole
    /// characters and appending the truncation note.
    static func boundedForExport(_ text: String) -> String {
        let maximumByteCount = ReferenceApplicationConfiguration
            .providerConsoleExportMaximumUTF8ByteCount
        guard text.utf8.count > maximumByteCount else { return text }
        let note = ReferenceApplicationConfiguration
            .providerConsoleExportTruncationNote
        let contentByteBudget = maximumByteCount - note.utf8.count
        var boundedText = ""
        var usedByteCount = 0
        for character in text {
            let characterByteCount = String(character).utf8.count
            guard usedByteCount + characterByteCount <= contentByteBudget else {
                break
            }
            boundedText.append(character)
            usedByteCount += characterByteCount
        }
        return boundedText + note
    }

    // MARK: Shared helpers

    /// The provider's registry display name, or its key when the registry
    /// carries no descriptor for it.
    static func displayName(
        forProviderKey providerKey: String,
        descriptors: [ProviderDescriptor]
    ) -> String {
        descriptors.first { $0.providerKey == providerKey }?.displayName
            ?? providerKey
    }

    /// The event's local wall-clock time.
    static func eventTimeText(_ event: LogEvent) -> String {
        ReferenceApplicationConfiguration.diagnosticsEventTimeFormatter
            .string(
                from: Date(
                    timeIntervalSince1970:
                        Double(event.timestampMillisecondsSinceEpoch) / 1000
                )
            )
    }
}

/// The export's defensive redaction: the SDK already removes the credential
/// from the observed path and the excerpt, and this repeats the same token
/// vocabulary over the composed text so a future field cannot leak by
/// accident.
enum ProviderConsoleRedaction {
    /// The credential-bearing parameter names the provider layer redacts.
    static let credentialParameterNames = [
        "key",
        "apiKey",
        "api_key",
        "access_token",
        "session",
        "signature",
    ]

    /// The placeholder a redacted value is replaced with.
    static let redactionPlaceholder = "<redacted>"

    /// Returns `text` with the credential-bearing query values, form
    /// assignments and JSON members removed.
    static func credentialFree(_ text: String) -> String {
        var redactedText = text
        for parameterName in credentialParameterNames {
            let escapedName = NSRegularExpression.escapedPattern(
                for: parameterName
            )
            let replacements: [(pattern: String, template: String)] = [
                (
                    "(?i)([?&;]\\s*\(escapedName)=)[^&\\s\"']+",
                    "$1\(redactionPlaceholder)"
                ),
                (
                    "(?i)(\\b\(escapedName)=)[^&\\s\"']+",
                    "$1\(redactionPlaceholder)"
                ),
                (
                    "(?i)(\"\(escapedName)\"\\s*:\\s*\")[^\"]*(\")",
                    "$1\(redactionPlaceholder)$2"
                ),
            ]
            for replacement in replacements {
                redactedText = redactedText.replacingOccurrences(
                    of: replacement.pattern,
                    with: replacement.template,
                    options: .regularExpression
                )
            }
        }
        return redactedText
    }
}
