//! The provider console's unit cases: the level filter, the provider row's
//! typed fields, the detail view's trace and cache rows, the credential-free
//! bounded export, the honest empty state, and the feed's newest-first bound
//! and unread Warning/Error dot. Every case runs in the simulator without a
//! network, a provider credential or a provider call.

import MapNativeCoreUI
import RusticMapsSDK
import XCTest

@testable import ReferenceApplication

@MainActor
final class ProviderConsoleTests: XCTestCase {
    // MARK: Fixtures

    /// A provider request trace with distinct, recognizable values.
    private static func providerTrace(
        providerKey: String = "probe-provider",
        statusClass: String = "4xx",
        responseExcerpt: String? = "{\"error\":\"rate limited\"}"
    ) -> ProviderRequestTrace {
        ProviderRequestTrace(
            providerKey: providerKey,
            providerContract: "RoutingProvider",
            httpMethod: "GET",
            requestPath: "/route/v1/driving/a;b?key=<redacted>&units=metric",
            statusClass: statusClass,
            outcome: "client-error",
            durationMilliseconds: 137,
            responseBytes: 2_048,
            responseExcerpt: responseExcerpt
        )
    }

    /// One log event with a named severity and timestamp.
    private static func event(
        index: UInt64,
        severity: SdkLogSeverity = .info,
        subsystemTarget: String = "provider_observation",
        messageLine: String = "provider request completed",
        providerRequestTrace: ProviderRequestTrace? = nil
    ) -> LogEvent {
        LogEvent(
            timestampMillisecondsSinceEpoch: 1_000 + index,
            severity: severity,
            subsystemTarget: subsystemTarget,
            messageLine: messageLine,
            providerRequestTrace: providerRequestTrace
        )
    }

    /// A distinct provider display name from the shared registry, when the
    /// application resource loads; the fallback assertion covers a missing one.
    private func firstRegistryDisplayName() -> (key: String, displayName: String)? {
        guard let catalog = try? ProviderRegistryCatalog.loadedFromApplicationBundle(),
              let descriptor = catalog.providers.first
        else {
            return nil
        }
        return (descriptor.providerKey, descriptor.displayName)
    }

    // MARK: Level filter

    func testLevelFilterSelectsErrorWarningAndInfo() {
        let events = [
            Self.event(index: 1, severity: .error),
            Self.event(index: 2, severity: .warn),
            Self.event(index: 3, severity: .info),
            Self.event(index: 4, severity: .debug),
        ]

        let errorRows = ProviderConsoleFormatting.visibleEvents(
            events, filter: .errors
        )
        XCTAssertEqual(errorRows.map(\.severity), [.error])

        let warningRows = ProviderConsoleFormatting.visibleEvents(
            events, filter: .warnings
        )
        XCTAssertEqual(warningRows.map(\.severity), [.error, .warn])

        let infoRows = ProviderConsoleFormatting.visibleEvents(
            events, filter: .information
        )
        XCTAssertEqual(
            infoRows.map(\.severity), [.error, .warn, .info],
            "Info shows the three documented levels and not debug detail"
        )
    }

    // MARK: Rows

    func testProviderRowNamesProviderContractMethodStatusDurationAndExcerpt() {
        let trace = Self.providerTrace()
        let event = Self.event(index: 0, providerRequestTrace: trace)

        let rowText = ProviderConsoleFormatting.rowText(
            for: event,
            descriptors: []
        )

        for expectedFragment in [
            "probe-provider",
            "RoutingProvider",
            "GET",
            "4xx",
            "137 ms",
            "{\"error\":\"rate limited\"}",
        ] {
            XCTAssertTrue(
                rowText.contains(expectedFragment),
                "the row carries \(expectedFragment): \(rowText)"
            )
        }
    }

    func testProviderRowUsesTheRegistryDisplayName() {
        guard let displayName = firstRegistryDisplayName() else {
            return XCTAssertTrue(
                ProviderConsoleFormatting.displayName(
                    forProviderKey: "unregistered",
                    descriptors: []
                ) == "unregistered",
                "an unregistered provider falls back to its key"
            )
        }
        let event = Self.event(
            index: 0,
            providerRequestTrace: Self.providerTrace(
                providerKey: displayName.key
            )
        )
        let descriptors = (try? ProviderRegistryCatalog
            .loadedFromApplicationBundle())?.providers ?? []
        let rowText = ProviderConsoleFormatting.rowText(
            for: event,
            descriptors: descriptors
        )
        XCTAssertTrue(
            rowText.contains(displayName.displayName),
            "the row names the provider by its registry display name: \(rowText)"
        )
    }

    func testRowExcerptCollapsesWhitespaceAndTruncatesOnACharacterBoundary() {
        XCTAssertEqual(
            ProviderConsoleFormatting.rowExcerpt("line one\n  line two"),
            "line one line two"
        )
        XCTAssertNil(
            ProviderConsoleFormatting.rowExcerpt(nil),
            "no captured excerpt renders no excerpt line"
        )
        XCTAssertNil(ProviderConsoleFormatting.rowExcerpt("   "))

        let longExcerpt = String(
            repeating: "a",
            count: ProviderConsoleFormatting.rowExcerptCharacterLimit + 20
        )
        let truncated = ProviderConsoleFormatting.rowExcerpt(longExcerpt)
        XCTAssertNotNil(truncated)
        XCTAssertEqual(
            truncated?.count,
            ProviderConsoleFormatting.rowExcerptCharacterLimit
                + ProviderConsoleFormatting.truncationEllipsis.count
        )
        XCTAssertTrue(
            truncated?.hasSuffix(
                ProviderConsoleFormatting.truncationEllipsis
            ) == true
        )
    }

    func testNonProviderRowUsesTheScrubbedSubsystemLine() {
        let event = Self.event(
            index: 0,
            severity: .warn,
            subsystemTarget: "tile_fetch",
            messageLine: "tile fetch retried"
        )
        let rowText = ProviderConsoleFormatting.rowText(
            for: event,
            descriptors: []
        )
        XCTAssertTrue(rowText.contains("warn"))
        XCTAssertTrue(rowText.contains("tile_fetch"))
        XCTAssertTrue(rowText.contains("tile fetch retried"))
    }

    // MARK: Detail

    func testDetailSectionsCarryEveryTypedTraceField() {
        let trace = Self.providerTrace()
        let event = Self.event(index: 0, providerRequestTrace: trace)
        let sections = ProviderConsoleFormatting.detailSections(
            for: event,
            descriptors: [],
            tileCacheState: nil,
            tileCacheUncachedNote: nil,
            tileStatistics: nil,
            tileCacheStatistics: nil,
            trafficCacheStatistics: nil
        )

        let requestSection = sections.first { $0.title == "Request" }
        XCTAssertNotNil(requestSection)
        let requestValues = requestSection?.rows.map(\.value) ?? []
        for expectedValue in [
            "probe-provider",
            "RoutingProvider",
            "GET",
            trace.requestPath,
            "4xx",
            "client-error",
            "137 ms",
            "2048",
            "{\"error\":\"rate limited\"}",
        ] {
            XCTAssertTrue(
                requestValues.contains(expectedValue),
                "the detail carries \(expectedValue): \(requestValues)"
            )
        }
    }

    func testTileCacheSectionShowsExactCountersBesideTheLastFrame() {
        let event = Self.event(index: 0, providerRequestTrace: Self.providerTrace())
        let statistics = FrameStatistics(
            requestedTileCount: 7,
            presentTileCount: 5,
            fallbackTileCount: 1,
            deferredTileCount: 1,
            unavailableTileCount: 0,
            overlayTriangleCount: 12
        )
        let cacheStatistics = TileCacheStatistics(
            hitCount: 6,
            missCount: 2,
            storedByteCount: 4096
        )
        let sections = ProviderConsoleFormatting.detailSections(
            for: event,
            descriptors: [],
            tileCacheState: OfflineTileCacheState(
                cacheDatabasePath: "/tmp/probe.db",
                budgetPhrase: "16 MiB budget"
            ),
            tileCacheUncachedNote: nil,
            tileStatistics: statistics,
            tileCacheStatistics: cacheStatistics,
            trafficCacheStatistics: nil
        )

        let tileSection = sections.first { $0.title == "Tile cache" }
        XCTAssertNotNil(tileSection)
        let rows = tileSection?.rows ?? []
        XCTAssertTrue(
            rows.contains(ProviderConsoleDetailRow(label: "Hits", value: "6")),
            "the cache's exact hits are shown: \(rows)"
        )
        XCTAssertTrue(
            rows.contains(ProviderConsoleDetailRow(label: "Misses", value: "2")),
            "the cache's exact misses are shown: \(rows)"
        )
        XCTAssertTrue(
            rows.contains(
                ProviderConsoleDetailRow(label: "Stored bytes", value: "4096")
            ),
            "the store's byte occupancy is shown: \(rows)"
        )
        XCTAssertTrue(
            rows.contains {
                $0.label == "Last frame"
                    && $0.value.contains("7 requested")
                    && $0.value.contains("5 drawn")
            },
            "the last frame's counts stay a separate, honestly-labelled row: \(rows)"
        )
    }

    func testTrafficCacheSectionShowsHitsAndMisses() {
        let event = Self.event(index: 0, providerRequestTrace: Self.providerTrace())
        let statistics = TrafficFlowCacheStatistics(
            hitCount: 3,
            missCount: 2,
            entryCount: 4,
            evictionCount: 1,
            segmentCount: 90,
            gridZoomLevel: 12,
            maximumEntryCount: 64,
            timeToLiveSeconds: 300
        )
        let sections = ProviderConsoleFormatting.detailSections(
            for: event,
            descriptors: [],
            tileCacheState: nil,
            tileCacheUncachedNote: nil,
            tileStatistics: nil,
            tileCacheStatistics: nil,
            trafficCacheStatistics: statistics
        )

        let trafficSection = sections.first { $0.title == "Traffic cache" }
        XCTAssertNotNil(trafficSection)
        let rows = trafficSection?.rows ?? []
        XCTAssertTrue(rows.contains(ProviderConsoleDetailRow(label: "Hits", value: "3")))
        XCTAssertTrue(rows.contains(ProviderConsoleDetailRow(label: "Misses", value: "2")))
        XCTAssertTrue(
            rows.contains(
                ProviderConsoleDetailRow(label: "Held cells", value: "4 of 64")
            )
        )
    }

    func testTrafficCacheAbsenceIsHonest() {
        let event = Self.event(index: 0, providerRequestTrace: Self.providerTrace())
        let sections = ProviderConsoleFormatting.detailSections(
            for: event,
            descriptors: [],
            tileCacheState: nil,
            tileCacheUncachedNote: nil,
            tileStatistics: nil,
            tileCacheStatistics: nil,
            trafficCacheStatistics: nil
        )
        let trafficSection = sections.first { $0.title == "Traffic cache" }
        XCTAssertEqual(
            trafficSection?.rows.first?.value,
            ReferenceApplicationConfiguration
                .providerConsoleTrafficCacheUnavailableText
        )
    }

    // MARK: Export

    func testExportIsNewestFirstAndCredentialFree() {
        let credential = "SECRET_PROBE_VALUE"
        let trace = Self.providerTrace(
            responseExcerpt: "{\"apiKey\":\"\(credential)\"}"
        )
        let events = [
            Self.event(
                index: 1,
                severity: .warn,
                subsystemTarget: "provider_observation",
                messageLine: "provider request key=\(credential)",
                providerRequestTrace: trace
            ),
            Self.event(
                index: 0,
                severity: .info,
                messageLine: "older renderer event"
            ),
        ]

        let exportText = ProviderConsoleFormatting.exportText(
            for: events,
            descriptors: [],
            filter: .information
        )

        XCTAssertFalse(
            exportText.contains(credential),
            "the export never carries the credential: \(exportText)"
        )
        XCTAssertTrue(exportText.contains("<redacted>") || exportText.contains("***"))
        XCTAssertTrue(
            exportText.contains(ReferenceApplicationConfiguration
                .providerConsoleExportHeaderText)
        )
        XCTAssertTrue(
            exportText.contains("RoutingProvider"),
            "the export carries the provider fields"
        )
        let providerRowRange = exportText.range(of: "RoutingProvider")
        let olderRowRange = exportText.range(of: "older renderer event")
        XCTAssertNotNil(providerRowRange)
        XCTAssertNotNil(olderRowRange)
        if let providerRowRange, let olderRowRange {
            XCTAssertLessThan(
                providerRowRange.lowerBound,
                olderRowRange.lowerBound,
                "the export preserves the newest-first order it is given"
            )
        }
    }

    func testExportTruncatesAtTheByteBudget() {
        let longMessage = String(
            repeating: "x",
            count: ReferenceApplicationConfiguration
                .providerConsoleExportMaximumUTF8ByteCount + 1_000
        )
        let exportText = ProviderConsoleFormatting.exportText(
            for: [
                Self.event(
                    index: 0,
                    messageLine: longMessage
                ),
            ],
            descriptors: [],
            filter: .information
        )
        XCTAssertLessThanOrEqual(
            exportText.utf8.count,
            ReferenceApplicationConfiguration
                .providerConsoleExportMaximumUTF8ByteCount
        )
        XCTAssertTrue(
            exportText.hasSuffix(
                ReferenceApplicationConfiguration
                    .providerConsoleExportTruncationNote
            ),
            "a truncated export says so"
        )
    }

    func testEmptyStateNamesTheActions() {
        let emptyStateText = ProviderConsoleFormatting.emptyStateText
        XCTAssertTrue(emptyStateText.contains("Search"))
        XCTAssertTrue(emptyStateText.contains("plan a route"))
        XCTAssertTrue(emptyStateText.contains("load a tile"))
    }

    // MARK: Feed and unread dot

    /// Waits for the warning/error dot's main-actor update to satisfy the
    /// given condition (the update hops from the listener's thread).
    private func waitForUnreadCounts(
        _ feed: ProviderConsoleFeed,
        until condition: @escaping @MainActor () -> Bool
    ) async {
        for _ in 0..<100 {
            if condition() { return }
            try? await Task.sleep(nanoseconds: 10_000_000)
        }
    }

    func testUnreadDotTracksWarningsAndErrorsWhileClosed() async {
        let feed = ProviderConsoleFeed()
        feed.close()
        XCTAssertFalse(feed.hasUnreadWarningOrError)

        feed.record(event: Self.event(index: 0, severity: .info))
        await waitForUnreadCounts(feed) { feed.hasUnreadWarningOrError }
        XCTAssertFalse(
            feed.hasUnreadWarningOrError,
            "an informational event never raises the dot"
        )

        feed.record(event: Self.event(index: 1, severity: .warn))
        await waitForUnreadCounts(feed) { feed.hasUnreadWarningOrError }
        XCTAssertTrue(feed.hasUnreadWarningOrError)
        XCTAssertFalse(feed.unreadDotCarriesError)

        feed.record(event: Self.event(index: 2, severity: .error))
        await waitForUnreadCounts(feed) { feed.unreadDotCarriesError }
        XCTAssertTrue(feed.unreadDotCarriesError)
    }

    func testOpeningTheConsoleResetsTheUnreadDot() async {
        let feed = ProviderConsoleFeed()
        feed.close()
        feed.record(event: Self.event(index: 0, severity: .error))
        await waitForUnreadCounts(feed) { feed.hasUnreadWarningOrError }
        XCTAssertTrue(feed.hasUnreadWarningOrError)

        feed.open()
        XCTAssertFalse(feed.hasUnreadWarningOrError)
        feed.close()
    }

    func testFeedKeepsNewestFirstAndStaysBounded() {
        let feed = ProviderConsoleFeed(capacity: 3)
        feed.open()
        for index in 0..<5 {
            feed.record(
                event: Self.event(index: UInt64(index), severity: .info)
            )
        }
        feed.mergePendingDeliveries()

        XCTAssertEqual(feed.events.count, 3)
        XCTAssertEqual(
            feed.events.map(\.timestampMillisecondsSinceEpoch),
            [1_004, 1_003, 1_002],
            "the newest rows lead and the bound drops the oldest"
        )
        feed.close()
    }
}
