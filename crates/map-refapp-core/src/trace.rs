//! The recorded-trace cursor and dispatch cadence.
//!
//! A recorded trace replays its samples verbatim. The cursor owns which sample
//! is next; the timer and the speed choice stay platform-side. The dispatch
//! interval derives from the trace's recorded sample interval and the playback
//! acceleration factor the caller folds the selected speed multiplier into.

/// Milliseconds in one second.
pub const MILLISECONDS_PER_SECOND: f64 = 1000.0;

/// The shortest dispatch interval, in milliseconds, so a fast speed multiplier
/// never spins the platform timer.
pub const MINIMUM_TRACE_DISPATCH_INTERVAL_MILLISECONDS: u64 = 1;

/// A forward-only cursor over a recorded trace's samples.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceCursor {
    sample_count: usize,
    next_sample_index: usize,
}

impl TraceCursor {
    /// Creates a cursor at the first sample of a trace with `sample_count`
    /// samples.
    pub fn new(sample_count: usize) -> Self {
        Self {
            sample_count,
            next_sample_index: 0,
        }
    }

    /// The trace's sample count.
    pub fn sample_count(&self) -> usize {
        self.sample_count
    }

    /// The next sample's index, or `None` when every sample was delivered.
    pub fn next_sample_index(&self) -> Option<usize> {
        (self.next_sample_index < self.sample_count).then_some(self.next_sample_index)
    }

    /// The next sample's index without consuming it.
    pub fn peek(&self) -> Option<usize> {
        self.next_sample_index()
    }

    /// Consumes and returns the next sample's index, or `None` at the end.
    pub fn advance(&mut self) -> Option<usize> {
        let sample_index = self.next_sample_index()?;
        self.next_sample_index += 1;
        Some(sample_index)
    }

    /// Whether every sample was delivered.
    pub fn is_finished(&self) -> bool {
        self.next_sample_index >= self.sample_count
    }

    /// The number of samples not yet delivered.
    pub fn remaining_sample_count(&self) -> usize {
        self.sample_count.saturating_sub(self.next_sample_index)
    }

    /// Rewinds the cursor to the first sample.
    pub fn reset(&mut self) {
        self.next_sample_index = 0;
    }

    /// Moves the cursor to `sample_index`, clamped to the trace's bounds.
    pub fn seek_to_sample_index(&mut self, sample_index: usize) {
        self.next_sample_index = sample_index.min(self.sample_count);
    }
}

/// The replay dispatch interval for a recorded trace, in whole milliseconds.
///
/// The recorded sample interval is divided by the playback acceleration
/// factor, then floored at
/// [`MINIMUM_TRACE_DISPATCH_INTERVAL_MILLISECONDS`]. A non-finite or
/// non-positive input cannot produce a meaningful cadence and therefore takes
/// the minimum interval rather than panicking or returning zero.
pub fn trace_dispatch_interval_milliseconds(
    recorded_sample_interval_seconds: f64,
    playback_acceleration_factor: f64,
) -> u64 {
    if !recorded_sample_interval_seconds.is_finite()
        || recorded_sample_interval_seconds <= 0.0
        || !playback_acceleration_factor.is_finite()
        || playback_acceleration_factor <= 0.0
    {
        return MINIMUM_TRACE_DISPATCH_INTERVAL_MILLISECONDS;
    }
    let dispatch_seconds = recorded_sample_interval_seconds / playback_acceleration_factor;
    let dispatch_milliseconds = dispatch_seconds * MILLISECONDS_PER_SECOND;
    if !dispatch_milliseconds.is_finite() || dispatch_milliseconds <= 0.0 {
        return MINIMUM_TRACE_DISPATCH_INTERVAL_MILLISECONDS;
    }
    dispatch_milliseconds.max(MINIMUM_TRACE_DISPATCH_INTERVAL_MILLISECONDS as f64) as u64
}
