//! Easing primitives of the platform frame pump (docs/ReferenceAppUX.md §5.3:
//! eased camera moves interpolate in the platform's frame pump — no new
//! contract surface), mirroring the Compose reference application's
//! `CameraEasing.kt` `SmoothstepTimedEase` and the Android shell's
//! `RefappCameraEasing.kt`. Pure: the pump calls the ease with the
//! `CADisplayLink` frame clock's nanosecond timestamps, and the duration is a
//! named constant of `RefappConstants`.

import Foundation

/// The smoothstep-timed single-value ease (the Compose application's
/// `SmoothstepTimedEase`).
final class SmoothstepTimedEase {
    private var startFrameNanos: Double = 0
    private var fromValue: Double = 0
    private var toValue: Double = 0
    private var durationNanos: Double = 0
    private var running = false

    /// Begins (or restarts) the ease from `from` to `to` at `startFrameNanos`.
    func begin(startFrameNanos: Double, from: Double, to: Double, durationNanos: Double) {
        self.startFrameNanos = startFrameNanos
        fromValue = from
        toValue = to
        self.durationNanos = durationNanos
        running = true
    }

    /// Whether an ease is in flight.
    var isRunning: Bool { running }

    /// Stops an in-flight ease without applying anything.
    func reset() {
        running = false
    }

    /// Advances the ease at `nowFrameNanos`: the current eased value, then
    /// `isFinished` when the move just finished this call (the caller then
    /// stops).
    func advance(nowFrameNanos: Double) -> (easedValue: Double, isFinished: Bool) {
        if !running {
            return (toValue, true)
        }
        let elapsedNanos = nowFrameNanos - startFrameNanos
        if elapsedNanos >= durationNanos {
            let finalValue = toValue
            running = false
            return (finalValue, true)
        }
        let easedValue = fromValue + (toValue - fromValue) * smoothstepEasedProgress(
            elapsedNanos / durationNanos
        )
        return (easedValue, false)
    }

    /// Smoothstep easing (`3t²−2t³`): eased in, eased out, structural math.
    private func smoothstepEasedProgress(_ linearProgress: Double) -> Double {
        let clampedProgress = max(0.0, min(1.0, linearProgress))
        return clampedProgress * clampedProgress * (3.0 - 2.0 * clampedProgress)
    }
}