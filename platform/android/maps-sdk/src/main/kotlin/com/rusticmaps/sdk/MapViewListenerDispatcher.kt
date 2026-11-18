package com.rusticmaps.sdk

import kotlinx.coroutines.CancellationException

/**
 * The one path every [MapViewListener] callback goes through.
 *
 * A listener is application code: a callback that throws must never escape
 * into the frame pump, the touch dispatch or the platform main thread, because
 * a throw out of the Choreographer callback would stop every later frame and
 * a throw out of a lifecycle callback would crash the host. Each dispatch is
 * therefore isolated:
 *
 * - the callback runs inside a `try` that catches [Throwable];
 * - a throwable is reported to the same listener through
 *   [MapViewListener.onError] as a [MapViewFailurePhase.LISTENER] failure;
 * - a throwable thrown by `onError` itself cannot be reported again, so it
 *   reaches [failureFallback] instead — the last resort, never a recursive
 *   dispatch.
 *
 * A [CancellationException] thrown by a listener is swallowed, not rethrown:
 * a listener callback is synchronous application code, not a coroutine the SDK
 * can cancel, and the dispatcher is called from the non-suspending frame pump
 * and the touch path, where a rethrow would end the pump or reach the main
 * looper. It is not a listener failure, so [MapViewListener.onError] is not
 * called for it either. The dispatcher holds no listener of its own; a
 * provider supplies the current listener (or null), so the view's liveness
 * policy stays in one place.
 *
 * @param listenerProvider the current listener, or null when none is set.
 * @param failureFallback receives a throwable that could not be reported
 *   through the error callback (the callback itself threw, or no listener
 *   remains). The view wires this to the platform log.
 */
internal class MapViewListenerDispatcher(
    private val listenerProvider: () -> MapViewListener?,
    private val failureFallback: (Throwable) -> Unit,
) {
    /**
     * Invokes one listener callback, isolating any throwable it raises.
     *
     * @param callback the listener member to call.
     */
    fun dispatch(callback: (MapViewListener) -> Unit) {
        val listener = listenerProvider() ?: return
        try {
            callback(listener)
        } catch (cancellation: CancellationException) {
            // Synchronous listener code cannot hand cancellation to the SDK;
            // swallowing it keeps the frame pump and the touch path alive.
        } catch (thrown: Throwable) {
            reportFailureSafely(MapViewFailure(MapViewFailurePhase.LISTENER, thrown))
        }
    }

    /**
     * Delivers one failure to the listener's `onError`, isolating a throw from
     * the error callback itself so reporting can never escape either.
     *
     * @param failure the failure to report.
     */
    fun reportFailure(failure: MapViewFailure) {
        reportFailureSafely(failure)
    }

    private fun reportFailureSafely(failure: MapViewFailure) {
        val listener = listenerProvider()
        if (listener == null) {
            return
        }
        try {
            listener.onError(failure)
        } catch (cancellation: CancellationException) {
            // Same rule as the callback path: a synchronous listener cannot
            // cancel the SDK, so the cancellation is swallowed rather than
            // escaping into the pump or the main thread.
        } catch (thrown: Throwable) {
            failureFallback(thrown)
        }
    }
}
