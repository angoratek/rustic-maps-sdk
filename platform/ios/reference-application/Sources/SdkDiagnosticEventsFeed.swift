//! The diagnostics sheet's recent-events readout (docs/ReferenceAppUX.md
//! §4.1): the contract's `SdkDiagnosticEventListener` foreign callback
//! bounded to a main-actor buffer the sheet renders oldest-first. The live
//! Debug stream runs at several events per presented frame, so the buffer's
//! published list refreshes on a slower cadence (§2.4's pane cadence) and the
//! frame pump never waits on the sheet's SwiftUI churn.
//!
//! The feed is lazy: the live subscription is armed the first time the sheet
//! opens, not at application start, and the buffer accepts deliveries only
//! while the sheet's cadence is running. Producer threads append to a locked
//! staging list with no task allocation per event; the 2 Hz merge timer
//! drains that list in one batch. Stopping the feed refuses later deliveries
//! and invalidates the merge timer.

import Combine
import Foundation
import RusticMapsSDK

/// The thread-safe staging between the SDK's producer threads and the
/// main-actor merge: deliveries append under a lock, and the merge drains them
/// in one batch. The SDK delivers fire-and-forget on whatever thread produced
/// the event, so this is the one place the listener may allocate.
private final class DiagnosticEventStaging: @unchecked Sendable {
    /// Guards `isAccepting` and `pending`.
    private let lock = NSLock()

    /// Whether deliveries are recorded; false before the sheet opens and
    /// after it closes.
    private var isAccepting = false

    /// The deliveries since the last drain, oldest first.
    private var pending: [LogEvent] = []

    /// Starts or stops accepting deliveries; stopping drops the unmerged
    /// side, because the sheet closed and its readout is being cleared.
    func setAccepting(_ accepting: Bool) {
        lock.lock()
        defer { lock.unlock() }
        isAccepting = accepting
        if !accepting {
            pending.removeAll(keepingCapacity: true)
        }
    }

    /// Appends one delivery when the feed is accepting.
    func append(_ event: LogEvent) {
        lock.lock()
        defer { lock.unlock() }
        guard isAccepting else { return }
        pending.append(event)
    }

    /// Takes every delivery since the last drain, oldest first.
    func drain() -> [LogEvent] {
        lock.lock()
        defer { lock.unlock() }
        let drained = pending
        pending.removeAll(keepingCapacity: true)
        return drained
    }

    /// Drops the unmerged side without changing the acceptance state.
    func removeAll() {
        lock.lock()
        defer { lock.unlock() }
        pending.removeAll(keepingCapacity: true)
    }

    /// Whether the feed currently accepts deliveries (the test seam).
    var isAcceptingDeliveries: Bool {
        lock.lock()
        defer { lock.unlock() }
        return isAccepting
    }
}

/// The main-actor buffer of recent structured diagnostic events.
///
/// Deliveries are staged on the producer thread and the cadence timer merges
/// them into the published, capacity-bounded oldest-first list in one batch,
/// so the events keep flowing without a SwiftUI change or a task per event.
@MainActor
final class SdkDiagnosticEventsBuffer: ObservableObject {
    /// The oldest-first events of the readout, newest at the end.
    @Published private(set) var events: [LogEvent] = []

    /// The producer-side staging (append-only from the listener's threads).
    private let staging = DiagnosticEventStaging()

    /// The repeating merge timer, armed by ``startRefreshCadence()`` and
    /// invalidated by ``stopRefreshCadence()``.
    private var refreshTimer: Timer?

    private var capacity: Int {
        ReferenceApplicationConfiguration.diagnosticsEventsBufferedCapacity
    }

    init() {}

    /// Whether the cadence timer is armed (the test seam for the feed's
    /// lifecycle).
    var isRefreshCadenceActive: Bool { refreshTimer != nil }

    /// Whether deliveries are currently recorded (the test seam for the
    /// feed's lifecycle).
    var isAcceptingDeliveries: Bool { staging.isAcceptingDeliveries }

    /// Starts the feed: accepts deliveries and arms the cadence merge. Idempotent;
    /// the first call owns the one timer.
    func startRefreshCadence() {
        staging.setAccepting(true)
        guard refreshTimer == nil else { return }
        let scheduledRefreshTimer = Timer.scheduledTimer(
            withTimeInterval:
                ReferenceApplicationConfiguration
                    .diagnosticsEventsRefreshCadenceSeconds,
            repeats: true
        ) { [weak self] _ in
            Task { @MainActor [weak self] in
                self?.mergePendingEvents()
            }
        }
        refreshTimer = scheduledRefreshTimer
    }

    /// Stops the feed: refuses later deliveries and invalidates the merge
    /// timer, so a closed sheet costs neither a timer firing nor a staged
    /// event.
    func stopRefreshCadence() {
        staging.setAccepting(false)
        refreshTimer?.invalidate()
        refreshTimer = nil
    }

    /// Records one delivered event from the listener's thread, without a
    /// main-actor hop or a per-event task. The cadence merge publishes it.
    nonisolated func enqueue(event: LogEvent) {
        staging.append(event)
    }

    /// Merges the staged deliveries into the published list (one SwiftUI
    /// change per tick, the batch capacity-trimmed so a whole cadence of
    /// floods past the capacity never accumulates in the published list).
    private func mergePendingEvents() {
        let deliveries = staging.drain()
        guard !deliveries.isEmpty else { return }
        events.append(contentsOf: deliveries)
        if events.count > capacity {
            events.removeFirst(events.count - capacity)
        }
    }

    func clear() {
        staging.removeAll()
        events = []
    }
}

/// The listener of the contract's diagnostic stream: the FFI delivers
/// fire-and-forget on whatever thread produced the event; the listener hands
/// the event straight to the locked staging list, so the producer never blocks
/// and no task is allocated per event (the fire-and-forget contract's one
/// rule).
final class MainActorDiagnosticEventListener: SdkDiagnosticEventListener {
    /// The buffer the events land in (appended oldest-first).
    private weak var buffer: SdkDiagnosticEventsBuffer?

    init(buffer: SdkDiagnosticEventsBuffer) {
        self.buffer = buffer
    }

    func onDiagnosticEvent(event: LogEvent) {
        buffer?.enqueue(event: event)
    }
}
