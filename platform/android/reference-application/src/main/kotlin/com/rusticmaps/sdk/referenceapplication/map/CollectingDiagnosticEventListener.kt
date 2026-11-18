package com.rusticmaps.sdk.referenceapplication.map

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.SnapshotStateList
import androidx.compose.runtime.mutableStateListOf
import com.rusticmaps.sdk.referenceapplication.ReferenceApplicationConstants
import uniffi.rustic_maps.LogEvent
import uniffi.rustic_maps.ProviderRequestTrace
import uniffi.rustic_maps.SdkDiagnosticEventListener
import uniffi.rustic_maps.SdkLogSeverity

/**
 * One consumer-facing diagnostics line of the recent-events readout
 * (docs/ReferenceAppUX.md §4.1): the event's subsystem target, severity and
 * the verbatim detail line (diagnostics detail is the one place SDK jargon is
 * *required* verbatim — §1), plus the typed provider request trace when the
 * event reports one. The trace is the SDK's already-redacted record; the
 * listener copies it unchanged.
 */
data class DiagnosticEventLine(
    val subsystemTarget: String,
    val severity: String,
    val messageLine: String,
    val timestampMillisecondsSinceEpoch: ULong,
    val providerRequestTrace: ProviderRequestTrace?,
)

/**
 * The bounded display window of the diagnostics pane
 * (docs/design/ReferenceApplicationBehavior.md, "The bounded stream").
 *
 * The foreign [SdkDiagnosticEventListener] callbacks run on the SDK's
 * producing threads fire-and-forget: they only append into a pending buffer
 * behind a lock (and raise the console's one snapshot dot for a Warning or
 * Error) and never block (the contract's listener rule). A coalescing cadence
 * — driven by the console, not by this class — publishes the buffer into the
 * Compose snapshot state at most once per refresh interval, keeps the newest
 * [WINDOW_EVENT_LIMIT] events, and drops the oldest. Nothing here writes a
 * console line, and no event is collected before the pane subscribes.
 */
class CollectingDiagnosticEventListener(
    private val windowEventLimit: Int = WINDOW_EVENT_LIMIT,
) : SdkDiagnosticEventListener {
    /** The events, oldest first; every publish window keeps the newest rows. */
    val eventLines: SnapshotStateList<DiagnosticEventLine> = mutableStateListOf()

    /** Events received since the last publish, oldest first. */
    private val pendingEventLines: MutableList<DiagnosticEventLine> = mutableListOf()

    /** The lock guarding the callback-thread buffer. */
    private val pendingLock = Any()

    /**
     * Whether a Warning or Error has arrived since the console last marked the
     * stream seen. The dot is raised directly from the delivery callback (the
     * console's display loop stops while the console is closed, so waiting for
     * a publish would lose the alert); the console marks it seen on open and
     * after every publish while it is open, so only events that arrived while
     * it was closed raise the dot.
     */
    var hasUnseenSevereEvent: Boolean by mutableStateOf(false)
        private set

    /**
     * The count the backlog flush reported (the contract returns the number
     * of events it drained into the listener), as the pane's backlog line.
     */
    var flushedEventCount: UInt? by mutableStateOf(null)
        private set

    override fun onDiagnosticEvent(diagnosticLogEvent: LogEvent) {
        val eventLine = DiagnosticEventLine(
            subsystemTarget = diagnosticLogEvent.subsystemTarget,
            severity = diagnosticLogEvent.severity.name,
            messageLine = diagnosticLogEvent.messageLine,
            timestampMillisecondsSinceEpoch = diagnosticLogEvent.timestampMillisecondsSinceEpoch,
            providerRequestTrace = diagnosticLogEvent.providerRequestTrace,
        )
        synchronized(pendingLock) {
            pendingEventLines.add(eventLine)
            while (pendingEventLines.size > windowEventLimit) {
                pendingEventLines.removeAt(0)
            }
        }
        if (
            diagnosticLogEvent.severity == SdkLogSeverity.WARN ||
            diagnosticLogEvent.severity == SdkLogSeverity.ERROR
        ) {
            // The dot must survive the console being closed, when the display
            // loop is stopped; a single snapshot boolean is a non-blocking
            // write the delivery thread can make itself.
            hasUnseenSevereEvent = true
        }
    }

    /**
     * Publishes the pending buffer into the readout's snapshot state, keeping
     * the newest [windowEventLimit] rows. The pane's refresh loop calls this at
     * the shared coalescing cadence, so a burst of SDK events costs one
     * recomposition rather than one per event.
     */
    fun publishPendingEvents() {
        val publishedEvents = synchronized(pendingLock) {
            if (pendingEventLines.isEmpty()) {
                return
            }
            pendingEventLines.toList().also { pendingEventLines.clear() }
        }
        eventLines.addAll(publishedEvents)
        while (eventLines.size > windowEventLimit) {
            eventLines.removeAt(0)
        }
    }

    /**
     * Marks the stream seen for the floating console control's dot: called
     * when the console opens and after each publish while it stays open, so
     * the dot only reports Warning/Error events that arrived while closed.
     */
    fun markSevereEventsSeen() {
        hasUnseenSevereEvent = false
    }

    /** Records the backlog flush's returned count for the pane's line. */
    fun noteBacklogFlushed(flushedCount: UInt) {
        flushedEventCount = flushedCount
    }

    /** The pane's Clear action: empties the displayed window and its backlog count. */
    fun clear() {
        synchronized(pendingLock) {
            pendingEventLines.clear()
        }
        eventLines.clear()
        flushedEventCount = null
        hasUnseenSevereEvent = false
    }

    private companion object {
        /**
         * The artifact's shared `diagnostics-event-window` value: the most
         * recent events the pane displays. The SDK ring's own bound (512 per
         * subsystem) lives in the SDK; this is the consumer readout's smaller
         * window.
         */
        const val WINDOW_EVENT_LIMIT: Int =
            ReferenceApplicationConstants.DIAGNOSTICS_EVENT_WINDOW
    }
}
