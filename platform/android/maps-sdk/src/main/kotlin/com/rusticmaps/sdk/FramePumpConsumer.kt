package com.rusticmaps.sdk

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.channels.ReceiveChannel
import kotlinx.coroutines.launch

/**
 * The frame pump's consumer loop: it drains the conflated frame channel and
 * runs one frame per tick.
 *
 * Application code runs inside [pumpOneFrame] (the listener dispatcher calls
 * into it and the native draw can fail), so one throwing frame must never end
 * the loop. A throwable is reported through [reportFailure] and the next frame
 * still runs. [CancellationException] is the only throwable that ends the loop,
 * because it means the consumer's own scope is being cancelled.
 *
 * [isRunning] tracks whether a consumer coroutine is alive and is cleared in
 * the loop's `finally`, so a loop that exits for any reason can be started
 * again by the next resume trigger instead of leaving the pump permanently
 * dead.
 *
 * @param pumpScope the scope the consumer coroutine launches into.
 * @param frameChannel the conflated frame-time channel the display callback feeds.
 * @param canPump whether the view is currently able to draw a frame.
 * @param pumpOneFrame runs one frame; it may throw.
 * @param reportFailure isolates and reports a throwable raised by one frame.
 */
internal class FramePumpConsumer(
    private val pumpScope: CoroutineScope,
    private val frameChannel: ReceiveChannel<Long>,
    private val canPump: () -> Boolean,
    private val pumpOneFrame: suspend (Long) -> Unit,
    private val reportFailure: (Throwable) -> Unit,
) {
    /** Whether a consumer coroutine is currently draining the channel. */
    var isRunning: Boolean = false
        private set

    /**
     * Starts the consumer when it is not already running.
     *
     * @return the started job, or null when a consumer was already running.
     */
    fun ensureRunning(): Job? {
        if (isRunning) {
            return null
        }
        isRunning = true
        return pumpScope.launch {
            try {
                for (frameTimeNanos in frameChannel) {
                    if (!canPump()) {
                        continue
                    }
                    try {
                        pumpOneFrame(frameTimeNanos)
                    } catch (cancellation: CancellationException) {
                        throw cancellation
                    } catch (thrown: Throwable) {
                        reportFailure(thrown)
                    }
                }
            } finally {
                isRunning = false
            }
        }
    }
}
