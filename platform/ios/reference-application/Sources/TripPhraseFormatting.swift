//! The metric distance and duration phrases the route summary card and the
//! navigation drawer render (the contract's `Route` records,
//! "formatted, never paraphrased" per docs/ReferenceAppUX.md §4.1 — the
//! demo's unit system is metric).

import Foundation

/// The metric phrasing the demo's trip records format with.
enum TripPhraseFormatting {
    /// The distance threshold (meters) above which meters phrase as
    /// kilometers.
    private static let kilometerThresholdMeters: Double =
        ReferenceApplicationConfiguration.tripEtaKilometerThresholdMeters

    /// The seconds in one phrased minute.
    private static let secondsPerMinute: Double = 60

    /// The meters in one phrased kilometer.
    private static let metersPerKilometer: Double = 1000

    /// A distance in meters as the metric phrase the cards render
    /// ("850 m" under the boundary, "3.7 km" at and above it).
    static func metricDistancePhrase(meters: Double) -> String {
        guard meters >= kilometerThresholdMeters else {
            return "\(meters.decimalReadout(0)) m"
        }
        return "\((meters / metersPerKilometer).decimalReadout(1)) km"
    }

    /// A duration in seconds as the phrase every platform renders: whole
    /// minutes and the remaining seconds, or seconds alone under a minute
    /// ("45 s", "12 min 30 s"). Android, React Native and the browser format
    /// the same value the same way, so one trip reads identically wherever it
    /// is shown.
    static func durationPhrase(seconds: Double) -> String {
        guard seconds >= secondsPerMinute else {
            return "\(seconds.decimalReadout(0)) s"
        }
        let wholeMinutes = Int((seconds / secondsPerMinute).rounded(.down))
        let remainderSeconds = seconds - Double(wholeMinutes) * secondsPerMinute
        return "\(wholeMinutes) min \(remainderSeconds.decimalReadout(0)) s"
    }
}