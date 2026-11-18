package com.rusticmaps.sdk

import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Unit cover for [MapViewListenerDispatcher]: a throwing listener callback is
 * contained and reported, the dispatcher keeps working afterwards (the frame
 * pump's survival property), and a throw from the error callback cannot recurse.
 */
class MapViewListenerDispatcherTest {
    @Test
    fun aThrowingCallbackIsReportedAndLaterCallbacksStillRun() {
        val listener = ThrowingOnceListener()
        val fallbackThrowables = mutableListOf<Throwable>()
        val dispatcher = MapViewListenerDispatcher(
            listenerProvider = { listener },
            failureFallback = { throwable -> fallbackThrowables.add(throwable) },
        )

        dispatcher.dispatch { target -> target.onFrameStatistics(FRAME_STATISTICS) }

        assertEquals(
            "the throw must reach the error surface, not escape",
            MapViewFailurePhase.LISTENER,
            listener.reportedFailures.single().phase,
        )
        assertTrue(
            "the reported failure must carry the callback's own throwable",
            listener.reportedFailures.single().cause is IllegalStateException,
        )
        assertTrue(fallbackThrowables.isEmpty())

        // The pump keeps going: a later dispatch still reaches the listener.
        dispatcher.dispatch { target -> target.onMapReady() }
        assertTrue("a healthy callback after a throwing one must still run", listener.receivedMapReady)
    }

    @Test
    fun aThrowingErrorCallbackFallsBackInsteadOfRecursing() {
        val listener = ThrowingErrorListener()
        val fallbackThrowables = mutableListOf<Throwable>()
        val dispatcher = MapViewListenerDispatcher(
            listenerProvider = { listener },
            failureFallback = { throwable -> fallbackThrowables.add(throwable) },
        )

        dispatcher.dispatch { target -> target.onFrameDeferred() }

        assertEquals(
            "the error callback's own throw must reach the fallback exactly once",
            1,
            fallbackThrowables.size,
        )
        assertEquals(
            "the callback failure was reported once before the error callback threw",
            1,
            listener.errorInvocations,
        )
    }

    @Test
    fun noListenerIsANoOp() {
        val dispatcher = MapViewListenerDispatcher(
            listenerProvider = { null },
            failureFallback = { throw AssertionError("no fallback is expected without a listener") },
        )

        dispatcher.dispatch { target -> target.onMapReady() }
        dispatcher.reportFailure(
            MapViewFailure(MapViewFailurePhase.DRAW, IllegalStateException("draw failed")),
        )
    }

    @Test
    fun aListenerThrowingCancellationIsSwallowedWithoutReportingAFailure() {
        val cancellation = CancellationException("the listener's own cancellation")
        val listener = ThrowingCancellationListener(cancellation)
        val dispatcher = MapViewListenerDispatcher(
            listenerProvider = { listener },
            failureFallback = { throw AssertionError("cancellation is not a listener failure") },
        )

        // A synchronous listener callback cannot hand cancellation to the
        // SDK: the dispatch must return normally and report nothing.
        dispatcher.dispatch { target -> target.onMapReady() }

        assertTrue("cancellation must not reach the error callback", listener.reportedFailures.isEmpty())
        // The pump keeps going: a later dispatch still reaches the listener.
        dispatcher.dispatch { target -> target.onFrameDeferred() }
        assertEquals(1, listener.deferredInvocations)
    }

    @Test
    fun aTouchPathListenerThrowingCancellationCannotEscape() {
        val listener = ThrowingCancellationListener(CancellationException("the touch handler cancelled"))
        val dispatcher = MapViewListenerDispatcher(
            listenerProvider = { listener },
            failureFallback = { throw AssertionError("cancellation is not a listener failure") },
        )

        // The touch path dispatches synchronously; a rethrow here would reach
        // the main looper.
        dispatcher.dispatch { target ->
            target.onMapClick(uniffi.rustic_maps.GeographicCoordinate(latitude = 0.0, longitude = 0.0))
        }
        assertTrue(listener.reportedFailures.isEmpty())
    }

    @Test
    fun aThrowingFrameIsReportedAndLaterFramesStillRun() = runBlocking {
        val channel = Channel<Long>(Channel.UNLIMITED)
        val processedFrames = CopyOnWriteArrayList<Long>()
        val reportedFailures = CopyOnWriteArrayList<Throwable>()
        val secondFrameProcessed = CompletableDeferred<Unit>()
        val consumer = FramePumpConsumer(
            pumpScope = CoroutineScope(Dispatchers.Default),
            frameChannel = channel,
            canPump = { true },
            pumpOneFrame = { frameTimeNanos ->
                processedFrames.add(frameTimeNanos)
                if (frameTimeNanos == THROWING_FRAME_TIME_NANOS) {
                    throw IllegalStateException("the frame's draw failed")
                }
                if (frameTimeNanos == FOLLOWING_FRAME_TIME_NANOS) {
                    secondFrameProcessed.complete(Unit)
                }
            },
            reportFailure = { thrown -> reportedFailures.add(thrown) },
        )

        val consumerJob = consumer.ensureRunning()
            ?: throw AssertionError("the consumer must start")

        channel.send(THROWING_FRAME_TIME_NANOS)
        channel.send(FOLLOWING_FRAME_TIME_NANOS)
        withTimeout(TEST_TIMEOUT_MILLISECONDS) { secondFrameProcessed.await() }

        assertEquals(
            "the frame after a throwing one must still run",
            listOf(THROWING_FRAME_TIME_NANOS, FOLLOWING_FRAME_TIME_NANOS),
            processedFrames.toList(),
        )
        assertEquals(1, reportedFailures.size)
        assertTrue(reportedFailures.single() is IllegalStateException)

        channel.close()
        withTimeout(TEST_TIMEOUT_MILLISECONDS) { consumerJob.join() }
        assertFalse(
            "an exited consumer must not leave the pump marked running",
            consumer.isRunning,
        )
    }

    @Test
    fun aCancellationThrowingListenerDoesNotStopTheFramePump() = runBlocking {
        val listener = ThrowingCancellationListener(CancellationException("a listener cancelled"))
        val dispatcher = MapViewListenerDispatcher(
            listenerProvider = { listener },
            failureFallback = { throw AssertionError("cancellation is not a listener failure") },
        )
        val channel = Channel<Long>(Channel.UNLIMITED)
        val processedFrames = CopyOnWriteArrayList<Long>()
        val secondFrameProcessed = CompletableDeferred<Unit>()
        val consumer = FramePumpConsumer(
            pumpScope = CoroutineScope(Dispatchers.Default),
            frameChannel = channel,
            canPump = { true },
            pumpOneFrame = { frameTimeNanos ->
                processedFrames.add(frameTimeNanos)
                dispatcher.dispatch { target -> target.onFrameStatistics(FRAME_STATISTICS) }
                if (frameTimeNanos == FOLLOWING_FRAME_TIME_NANOS) {
                    secondFrameProcessed.complete(Unit)
                }
            },
            reportFailure = { throw AssertionError("cancellation must not be reported", it) },
        )

        val consumerJob = consumer.ensureRunning()
            ?: throw AssertionError("the consumer must start")

        channel.send(THROWING_FRAME_TIME_NANOS)
        channel.send(FOLLOWING_FRAME_TIME_NANOS)
        withTimeout(TEST_TIMEOUT_MILLISECONDS) { secondFrameProcessed.await() }

        assertEquals(
            "a cancellation-throwing listener must not stop later frames",
            listOf(THROWING_FRAME_TIME_NANOS, FOLLOWING_FRAME_TIME_NANOS),
            processedFrames.toList(),
        )

        channel.close()
        withTimeout(TEST_TIMEOUT_MILLISECONDS) { consumerJob.join() }
    }

    private class ThrowingOnceListener : MapViewListener {
        val reportedFailures = mutableListOf<MapViewFailure>()
        var receivedMapReady: Boolean = false

        override fun onFrameStatistics(statistics: uniffi.rustic_maps.FrameStatistics) {
            throw IllegalStateException("the application's statistics handler failed")
        }

        override fun onMapReady() {
            receivedMapReady = true
        }

        override fun onError(failure: MapViewFailure) {
            reportedFailures.add(failure)
        }
    }

    private class ThrowingErrorListener : MapViewListener {
        var errorInvocations: Int = 0

        override fun onFrameDeferred() {
            throw IllegalStateException("the application's deferred handler failed")
        }

        override fun onError(failure: MapViewFailure) {
            errorInvocations += 1
            throw IllegalStateException("the application's error handler failed too")
        }
    }

    private class ThrowingCancellationListener(
        private val cancellation: CancellationException,
    ) : MapViewListener {
        val reportedFailures = mutableListOf<MapViewFailure>()
        var deferredInvocations: Int = 0

        override fun onMapReady() {
            throw cancellation
        }

        override fun onMapClick(coordinate: uniffi.rustic_maps.GeographicCoordinate) {
            throw cancellation
        }

        override fun onFrameDeferred() {
            deferredInvocations += 1
        }

        override fun onError(failure: MapViewFailure) {
            reportedFailures.add(failure)
        }
    }

    private companion object {
        const val THROWING_FRAME_TIME_NANOS: Long = 1_000_000_000L
        const val FOLLOWING_FRAME_TIME_NANOS: Long = 2_000_000_000L
        const val TEST_TIMEOUT_MILLISECONDS: Long = 5_000L

        val FRAME_STATISTICS: uniffi.rustic_maps.FrameStatistics =
            uniffi.rustic_maps.FrameStatistics(
                requestedTileCount = 0u,
                presentTileCount = 0u,
                fallbackTileCount = 0u,
                deferredTileCount = 0u,
                unavailableTileCount = 0u,
                overlayTriangleCount = 0u,
            )
    }
}
