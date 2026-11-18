package com.rusticmaps.sdk.referenceapplication.map

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.rustic_maps.LogEvent
import uniffi.rustic_maps.ProviderRequestTrace
import uniffi.rustic_maps.SdkLogSeverity

/**
 * Unit tests of the diagnostics pane's bounded window: events coalesce until
 * a publish, the window keeps the newest rows, and Clear empties both the
 * displayed rows and the backlog count. Nothing is collected before the pane
 * subscribes because the listener is only connected on open.
 */
class CollectingDiagnosticEventListenerTest {
    @Test
    fun `events stay pending until the pane publishes them`() {
        val listener = CollectingDiagnosticEventListener()
        listener.onDiagnosticEvent(event("first"))
        assertTrue(listener.eventLines.isEmpty())

        listener.publishPendingEvents()
        assertEquals(listOf("first"), listener.eventLines.map { line -> line.messageLine })
    }

    @Test
    fun `a burst coalesces into one publish`() {
        val listener = CollectingDiagnosticEventListener()
        repeat(12) { index -> listener.onDiagnosticEvent(event("event $index")) }
        listener.publishPendingEvents()
        assertEquals(12, listener.eventLines.size)
        listener.publishPendingEvents()
        assertEquals("a second publish has nothing pending", 12, listener.eventLines.size)
    }

    @Test
    fun `the window keeps the newest events`() {
        val listener = CollectingDiagnosticEventListener(windowEventLimit = 5)
        repeat(9) { index -> listener.onDiagnosticEvent(event("event $index")) }
        listener.publishPendingEvents()
        assertEquals(
            listOf("event 4", "event 5", "event 6", "event 7", "event 8"),
            listener.eventLines.map { line -> line.messageLine },
        )
    }

    @Test
    fun `clear empties the rows and the backlog count`() {
        val listener = CollectingDiagnosticEventListener()
        listener.onDiagnosticEvent(event("first"))
        listener.publishPendingEvents()
        listener.noteBacklogFlushed(3u)

        listener.clear()
        assertTrue(listener.eventLines.isEmpty())
        assertNull(listener.flushedEventCount)
    }

    @Test
    fun `a warning raises the unseen dot immediately and it clears when seen`() {
        val listener = CollectingDiagnosticEventListener()
        listener.onDiagnosticEvent(event("info"))
        assertFalse(listener.hasUnseenSevereEvent)

        listener.onDiagnosticEvent(event("warning", severity = SdkLogSeverity.WARN))
        // The console's display loop is stopped while it is cached closed, so
        // the dot cannot wait for a publish.
        assertTrue(listener.hasUnseenSevereEvent)

        listener.markSevereEventsSeen()
        assertFalse(listener.hasUnseenSevereEvent)
    }

    @Test
    fun `an info event never raises the unseen dot`() {
        val listener = CollectingDiagnosticEventListener()
        listener.onDiagnosticEvent(event("info", severity = SdkLogSeverity.INFO))
        assertFalse(listener.hasUnseenSevereEvent)
    }

    @Test
    fun `the typed provider request trace is preserved verbatim`() {
        val trace = ProviderRequestTrace(
            providerKey = "provider-key",
            providerContract = "RoutingProvider",
            httpMethod = "GET",
            requestPath = "/route",
            statusClass = "2xx",
            outcome = "success",
            durationMilliseconds = 42UL,
            responseBytes = 128UL,
            responseExcerpt = "{\"ok\":true}",
        )
        val listener = CollectingDiagnosticEventListener()
        listener.onDiagnosticEvent(event("routed", providerRequestTrace = trace))
        listener.publishPendingEvents()
        assertEquals(trace, listener.eventLines.single().providerRequestTrace)
    }

    private fun event(
        messageLine: String,
        severity: SdkLogSeverity = SdkLogSeverity.INFO,
        providerRequestTrace: ProviderRequestTrace? = null,
    ): LogEvent = LogEvent(
        timestampMillisecondsSinceEpoch = 1UL,
        severity = severity,
        subsystemTarget = "tile_fetch",
        messageLine = messageLine,
        providerRequestTrace = providerRequestTrace,
    )
}
