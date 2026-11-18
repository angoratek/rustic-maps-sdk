package com.rusticmaps.sdk

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.rustic_maps.UrlTemplateVectorTileSource

/**
 * Unit cover for the queued vector base install: a selection made while a
 * draw is in flight is queued, and a refusal restores the source the view
 * served before that selection instead of keeping a value the SDK never
 * accepted.
 */
class VectorBaseSourceQueueTest {
    @Test
    fun aRefusedQueuedSourceRestoresThePreviousSource() {
        val queue = VectorBaseSourceQueue()
        val previousSource = vectorSource(SOURCE_NAME_PREVIOUS)
        val queuedSource = vectorSource(SOURCE_NAME_QUEUED)
        queue.select(previousSource, isDrawInFlight = false) { }

        queue.select(queuedSource, isDrawInFlight = true) {
            throw AssertionError("a selection during a draw must be queued, not pushed")
        }
        assertEquals(queuedSource, queue.source)

        var pushedSource: UrlTemplateVectorTileSource? = null
        val refusal = try {
            queue.applyPending(isDrawInFlight = false) { source ->
                pushedSource = source
                throw IllegalStateException("the SDK refused the queued source")
            }
            null
        } catch (thrown: IllegalStateException) {
            thrown
        }

        assertEquals("the queued source was the one attempted", queuedSource, pushedSource)
        assertEquals(
            "a refusal must restore the source the view served before the queue",
            previousSource,
            queue.source,
        )
        assertEquals("the refusal reaches the caller for reporting", "the SDK refused the queued source", refusal?.message)
    }

    @Test
    fun anAcceptedQueuedSourceReplacesThePreviousSource() {
        val queue = VectorBaseSourceQueue()
        val previousSource = vectorSource(SOURCE_NAME_PREVIOUS)
        val queuedSource = vectorSource(SOURCE_NAME_QUEUED)
        queue.select(previousSource, isDrawInFlight = false) { }
        queue.select(queuedSource, isDrawInFlight = true) { }

        val applied = queue.applyPending(isDrawInFlight = false) { }

        assertTrue(applied)
        assertEquals(queuedSource, queue.source)
    }

    @Test
    fun aSecondQueuedSelectionKeepsTheFirstRollbackTarget() {
        val queue = VectorBaseSourceQueue()
        val servedSource = vectorSource(SOURCE_NAME_PREVIOUS)
        queue.select(servedSource, isDrawInFlight = false) { }
        queue.select(vectorSource(SOURCE_NAME_QUEUED), isDrawInFlight = true) { }
        queue.select(vectorSource(SOURCE_NAME_SECOND_QUEUED), isDrawInFlight = true) { }

        try {
            queue.applyPending(isDrawInFlight = false) {
                throw IllegalStateException("the SDK refused the second queued source")
            }
        } catch (refusal: IllegalStateException) {
            // Expected: the queue rethrows the refusal for the caller to report.
        }

        assertEquals(
            "the rollback target is the source the view actually served",
            servedSource,
            queue.source,
        )
    }

    @Test
    fun anImmediateSelectionAfterAQueuedOneSupersedesItAndRestoresTheServedSource() {
        val queue = VectorBaseSourceQueue()
        val servedSource = vectorSource(SOURCE_NAME_PREVIOUS)
        queue.select(servedSource, isDrawInFlight = false) { }
        queue.select(vectorSource(SOURCE_NAME_QUEUED), isDrawInFlight = true) { }

        // The draw finishes and the user chooses again before the queued
        // install runs; the SDK refuses that immediate push.
        val refusal = try {
            queue.select(vectorSource(SOURCE_NAME_LATEST), isDrawInFlight = false) {
                throw IllegalStateException("the SDK refused the immediate source")
            }
            null
        } catch (thrown: IllegalStateException) {
            thrown
        }

        assertEquals(
            "the refusal reaches the caller for reporting",
            "the SDK refused the immediate source",
            refusal?.message,
        )
        assertEquals(
            "a refusal restores the source the view actually serves, not the queued one",
            servedSource,
            queue.source,
        )
        assertFalse(
            "the superseded queued install must not run later",
            queue.applyPending(isDrawInFlight = false) {
                throw AssertionError("no queued install remains")
            },
        )
    }

    @Test
    fun aQueuedSourceWaitsWhileADrawIsStillInFlight() {
        val queue = VectorBaseSourceQueue()
        queue.select(vectorSource(SOURCE_NAME_PREVIOUS), isDrawInFlight = false) { }
        queue.select(vectorSource(SOURCE_NAME_QUEUED), isDrawInFlight = true) { }

        var didPush = false
        val applied = queue.applyPending(isDrawInFlight = true) { didPush = true }

        assertFalse(applied)
        assertFalse(didPush)
    }

    private fun vectorSource(sourceName: String): UrlTemplateVectorTileSource =
        UrlTemplateVectorTileSource(
            urlTemplate = "https://tiles.example.test/{z}/{x}/{y}.pbf",
            sourceName = sourceName,
            attributionText = "Example",
            styleId = "demo",
            minimumZoomLevel = 0u,
            maximumZoomLevel = 14u,
        )

    private companion object {
        const val SOURCE_NAME_PREVIOUS: String = "previous-base"
        const val SOURCE_NAME_QUEUED: String = "queued-base"
        const val SOURCE_NAME_SECOND_QUEUED: String = "second-queued-base"
        const val SOURCE_NAME_LATEST: String = "latest-base"
    }
}
