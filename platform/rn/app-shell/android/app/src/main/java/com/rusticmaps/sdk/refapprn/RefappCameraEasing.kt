package com.rusticmaps.sdk.refapprn

/**
 * Easing primitives of the platform frame pump (docs/ReferenceAppUX.md §5.3:
 * eased camera moves interpolate in the platform's frame pump — no new
 * contract surface), mirroring the Compose reference application's
 * `CameraEasing.kt` `SmoothstepTimedEase`. Pure: the pump calls the ease with
 * the Choreographer frame clock's nanosecond timestamps, and the duration is
 * a named constant of [RefappConstants].
 */
class SmoothstepTimedEase {
    private var startFrameNanos: Long = 0
    private var fromValue: Double = 0.0
    private var toValue: Double = 0.0
    private var durationNanos: Long = 0
    private var running: Boolean = false

    /** Begins (or restarts) the ease from `fromValue` to `toValue` at `startFrameNanos`. */
    fun begin(startFrameNanos: Long, from: Double, to: Double, durationNanos: Long) {
        this.startFrameNanos = startFrameNanos
        fromValue = from
        toValue = to
        running = true
        this.durationNanos = durationNanos
    }

    /** Whether an ease is in flight. */
    val isRunning: Boolean
        get() = running

    /** Stops an in-flight ease without applying anything. */
    fun reset() {
        running = false
    }

    /**
     * Advances the ease at `nowFrameNanos`: the current eased value, then true
     * when the move just finished this call (the caller then stops).
     */
    fun advance(nowFrameNanos: Long): Pair<Double, Boolean> {
        if (!running) {
            return toValue to true
        }
        val elapsedNanos = nowFrameNanos - startFrameNanos
        if (elapsedNanos >= durationNanos) {
            val finalValue = toValue
            running = false
            return finalValue to true
        }
        val easedValue = fromValue + (toValue - fromValue) *
            smoothstepEasedProgress(elapsedNanos.toDouble() / durationNanos.toDouble())
        return easedValue to false
    }

    /** Smoothstep easing (`3t²−2t³`): eased in, eased out, structural math. */
    private fun smoothstepEasedProgress(linearProgress: Double): Double {
        val clampedProgress = linearProgress.coerceIn(0.0, 1.0)
        return clampedProgress * clampedProgress * (3.0 - 2.0 * clampedProgress)
    }
}