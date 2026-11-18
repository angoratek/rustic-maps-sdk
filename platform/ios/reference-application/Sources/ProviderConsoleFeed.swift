//! The provider console's read-only feed over the SDK's diagnostics stream
//! (docs/Browser.md): a second subscriber beside the
//! diagnostics pane's, never a new request path. The console keeps its rows
//! newest-first, bounded to a named maximum, and only the Warning and Error
//! events that arrive while the console is closed reach the floating
//! control's unread dot.
//!
//! Deliveries are fire-and-forget on whatever thread produced the event, so
//! they stage under a lock and the main-actor cadence merges them in one
//! batch. The feed never parses the composed message line: a provider row is
//! built from the typed `ProviderRequestTrace`, exactly as the contract
//! delivers it.

import Combine
import Foundation
import RusticMapsSDK

/// The console's level filter (docs/Browser.md "Provider request console"):
/// Error shows failures, Warning shows warnings and errors, Info shows every
/// event the console's subscription receives.
enum ProviderConsoleLevelFilter: String, CaseIterable, Identifiable, Sendable {
    case errors
    case warnings
    case information

    var id: String { rawValue }

    /// The label the segmented filter renders.
    var label: String {
        switch self {
        case .errors:
            return ReferenceApplicationConfiguration.providerConsoleLevelErrorsLabel
        case .warnings:
            return ReferenceApplicationConfiguration.providerConsoleLevelWarningsLabel
        case .information:
            return ReferenceApplicationConfiguration.providerConsoleLevelInformationLabel
        }
    }

    /// The lowest severity the filter shows: Error, Warning (and above), or
    /// Info (and above).
    var minimumSeverity: SdkLogSeverity {
        switch self {
        case .errors: return .error
        case .warnings: return .warn
        case .information: return .info
        }
    }
}

/// What the staging list did with one delivery.
private enum ProviderConsoleDeliveryCapture {
    /// The console is open: every event is kept for its list.
    case fullStream
    /// The console is closed: only warnings and errors are kept, so the
    /// unread dot can report them and the next open can render them.
    case warningOrErrorWhileClosed
    /// The console is closed and the event is informational: nothing is
    /// kept (the SDK's ring still replays it on the next open).
    case dropped
}

/// The thread-safe staging between the SDK's producer threads and the
/// main-actor merge.
private final class ProviderConsoleEventStaging: @unchecked Sendable {
    /// Guards `pending` and `capturesFullStream`.
    private let lock = NSLock()

    /// The deliveries since the last drain, oldest first.
    private var pending: [LogEvent] = []

    /// Whether the console is open and therefore keeps the full stream.
    private var capturesFullStream = false

    /// Opens or closes the full-stream capture. Closing still keeps
    /// warnings and errors for the unread dot.
    func setCapturesFullStream(_ capturesFullStream: Bool) {
        lock.lock()
        defer { lock.unlock() }
        self.capturesFullStream = capturesFullStream
    }

    /// Appends one delivery under the current capture policy.
    func append(_ event: LogEvent) -> ProviderConsoleDeliveryCapture {
        lock.lock()
        defer { lock.unlock() }
        if capturesFullStream {
            pending.append(event)
            return .fullStream
        }
        if event.severity == .warn || event.severity == .error {
            pending.append(event)
            return .warningOrErrorWhileClosed
        }
        return .dropped
    }

    /// Takes every delivery since the last drain, oldest first.
    func drain() -> [LogEvent] {
        lock.lock()
        defer { lock.unlock() }
        let drained = pending
        pending.removeAll(keepingCapacity: true)
        return drained
    }

    /// Drops the unmerged deliveries without changing the capture policy.
    func removeAll() {
        lock.lock()
        defer { lock.unlock() }
        pending.removeAll(keepingCapacity: true)
    }
}

/// The main-actor console buffer: deliveries stage on the producer thread and
/// the cadence timer merges them into the published newest-first list in one
/// batch, so the frame path never waits on SwiftUI churn.
@MainActor
final class ProviderConsoleFeed: ObservableObject {
    /// The console's rows, newest first.
    @Published private(set) var events: [LogEvent] = []

    /// Warnings delivered while the console was closed since it last opened.
    @Published private(set) var unreadWarningCount = 0

    /// Errors delivered while the console was closed since it last opened.
    @Published private(set) var unreadErrorCount = 0

    /// The producer-side staging.
    private let staging = ProviderConsoleEventStaging()

    /// The repeating merge timer, armed while the console is open.
    private var refreshTimer: Timer?

    /// The rows' bound; the feed keeps no more than this many.
    private let capacity: Int

    init(
        capacity: Int = ReferenceApplicationConfiguration.providerConsoleMaximumRows
    ) {
        self.capacity = capacity
    }

    /// Whether the cadence timer is armed (the test seam).
    var isRefreshCadenceActive: Bool { refreshTimer != nil }

    /// Whether a Warning or Error has arrived since the console last opened.
    var hasUnreadWarningOrError: Bool {
        unreadWarningCount > 0 || unreadErrorCount > 0
    }

    /// The unread dot's color chooses the more severe of the two counts: an
    /// unread error outranks an unread warning.
    var unreadDotCarriesError: Bool { unreadErrorCount > 0 }

    /// Opens the console: keeps the full stream, resets the unread dot,
    /// starts the merge cadence and publishes the staged deliveries.
    func open() {
        staging.setCapturesFullStream(true)
        unreadWarningCount = 0
        unreadErrorCount = 0
        startRefreshCadence()
        mergePendingEvents()
    }

    /// Closes the console: stops the cadence, then keeps only warnings and
    /// errors (for the dot) and publishes what was staged.
    func close() {
        staging.setCapturesFullStream(false)
        stopRefreshCadence()
        mergePendingEvents()
    }

    /// Empties the published and staged lists without changing the capture
    /// policy (the console clears before it replays the ring on open).
    func clear() {
        staging.removeAll()
        events = []
    }

    /// Publishes the staged deliveries immediately, for the synchronous
    /// backlog flush an open performs.
    func mergePendingDeliveries() {
        mergePendingEvents()
    }

    /// Records one delivered event from the listener's thread. Only a
    /// warning or error delivered while the console is closed schedules a
    /// main-actor hop; the high-volume informational stream never allocates
    /// a task per event.
    nonisolated func record(event: LogEvent) {
        switch staging.append(event) {
        case .fullStream, .dropped:
            return
        case .warningOrErrorWhileClosed:
            let isError = event.severity == .error
            Task { @MainActor [weak self] in
                guard let self else { return }
                if isError {
                    self.unreadErrorCount += 1
                } else {
                    self.unreadWarningCount += 1
                }
            }
        }
    }

    /// Merges the staged deliveries into the published list, newest first,
    /// capacity-trimmed so a flood never grows the published list past the
    /// bound.
    private func mergePendingEvents() {
        let deliveries = staging.drain()
        guard !deliveries.isEmpty else { return }
        let merged = (Array(deliveries.reversed()) + events).enumerated()
        events = merged
            .sorted { first, second in
                if first.element.timestampMillisecondsSinceEpoch
                    == second.element.timestampMillisecondsSinceEpoch
                {
                    return first.offset < second.offset
                }
                return first.element.timestampMillisecondsSinceEpoch
                    > second.element.timestampMillisecondsSinceEpoch
            }
            .map(\.element)
        if events.count > capacity {
            events.removeLast(events.count - capacity)
        }
    }

    /// Arms the cadence merge; the first call owns the one timer.
    private func startRefreshCadence() {
        guard refreshTimer == nil else { return }
        refreshTimer = Timer.scheduledTimer(
            withTimeInterval:
                ReferenceApplicationConfiguration
                    .diagnosticsEventsRefreshCadenceSeconds,
            repeats: true
        ) { [weak self] _ in
            Task { @MainActor [weak self] in
                self?.mergePendingEvents()
            }
        }
    }

    /// Invalidates the cadence merge.
    private func stopRefreshCadence() {
        refreshTimer?.invalidate()
        refreshTimer = nil
    }
}

/// The console's listener of the contract's diagnostics stream: the FFI
/// delivers fire-and-forget on whatever thread produced the event, and the
/// listener hands it straight to the locked staging list.
final class ProviderConsoleDiagnosticEventListener: SdkDiagnosticEventListener {
    /// The feed the events land in.
    private weak var feed: ProviderConsoleFeed?

    init(feed: ProviderConsoleFeed) {
        self.feed = feed
    }

    func onDiagnosticEvent(event: LogEvent) {
        feed?.record(event: event)
    }
}
