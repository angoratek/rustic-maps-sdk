//! The draw delegate's publication gate: a presented frame publishes its
//! readbacks only when its inputs changed, and a changed frame publishes at
//! most once per cadence window. Without it every presented frame assigns the
//! model's published properties, so SwiftUI re-evaluates the whole scene at
//! the display cadence even when nothing on screen changed.

import Foundation
import RusticMapsSDK

/// Coalesces the draw delegate's per-frame readbacks.
///
/// The gate compares the frame's tile statistics and camera readback against
/// the last published pair. An identical frame publishes nothing (and its
/// caller skips the provider-status readback); a changed frame publishes at
/// most once per ``minimumIntervalSeconds``, with a throttled change retried
/// on the next presented frame.
struct MapFramePublicationGate {
    /// The shortest interval between two publications, in seconds.
    let minimumIntervalSeconds: TimeInterval

    /// The statistics of the last published frame, or nil before the first.
    private var lastPublishedStatistics: FrameStatistics?

    /// The camera readback of the last published frame, or nil before the
    /// first.
    private var lastPublishedCamera: CameraAttributeReadback?

    /// The uptime instant of the last publication.
    private var lastPublishedAt: TimeInterval = -.infinity

    /// Builds a gate over one cadence window.
    ///
    /// - Parameter minimumIntervalSeconds: the shortest interval between two
    ///   publications.
    init(minimumIntervalSeconds: TimeInterval) {
        self.minimumIntervalSeconds = minimumIntervalSeconds
    }

    /// Answers whether a drawn frame should publish its readbacks.
    ///
    /// - Parameters:
    ///   - statistics: the presented frame's tile and overlay statistics.
    ///   - camera: the camera the frame drew with, read back from the view.
    ///   - now: the current uptime instant, in seconds.
    /// - Returns: true when the frame's inputs differ from the last published
    ///   pair and the cadence window has elapsed; the gate then records this
    ///   frame as the last publication.
    mutating func shouldPublish(
        statistics: FrameStatistics,
        camera: CameraAttributeReadback,
        now: TimeInterval
    ) -> Bool {
        if statistics == lastPublishedStatistics,
           camera == lastPublishedCamera {
            return false
        }
        guard now - lastPublishedAt >= minimumIntervalSeconds else {
            // Throttled: keep the previous publication as the baseline, so
            // the next presented frame retries this change.
            return false
        }
        lastPublishedStatistics = statistics
        lastPublishedCamera = camera
        lastPublishedAt = now
        return true
    }
}
