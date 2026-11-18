//! Circular course-over-ground smoothing.
//!
//! A course-over-ground is an angle, so an arithmetic median is wrong around
//! north. The smoother keeps a fixed window of recent courses and reports the
//! windowed sample that minimises the sum of angular distances to every other
//! sample in the window: the circular median.

/// One full circle in degrees, the modulus bearings wrap around.
pub const FULL_CIRCLE_DEGREES: f64 = 360.0;

/// Half a circle in degrees, the shortest-way wrap boundary.
pub const HALF_CIRCLE_DEGREES: f64 = 180.0;

/// The default number of recent fixes the median smooths over.
///
/// Five fixes smooth the walk demo's stepwise courses without lagging turns
/// noticeably at the accelerated replay cadence.
pub const COURSE_SMOOTHING_WINDOW_FIXES: usize = 5;

/// The angular distance between two bearings, in degrees, always in
/// `[0, 180]`.
pub fn angular_distance_degrees(first_degrees: f64, second_degrees: f64) -> f64 {
    let absolute_difference = (first_degrees - second_degrees).abs() % FULL_CIRCLE_DEGREES;
    if absolute_difference > HALF_CIRCLE_DEGREES {
        FULL_CIRCLE_DEGREES - absolute_difference
    } else {
        absolute_difference
    }
}

/// A fixed-window circular median over recent course-over-ground samples.
#[derive(Debug, Clone, PartialEq)]
pub struct CourseMedianSmoother {
    window_fix_count: usize,
    recent_course_degrees: Vec<f64>,
}

impl CourseMedianSmoother {
    /// Creates a smoother over `window_fix_count` recent samples.
    ///
    /// A zero-length window is valid and reports no median; it is useful only
    /// to a caller that intentionally disables smoothing.
    pub fn new(window_fix_count: usize) -> Self {
        Self {
            window_fix_count,
            recent_course_degrees: Vec::with_capacity(window_fix_count),
        }
    }

    /// Creates a smoother over the default window.
    pub fn with_default_window() -> Self {
        Self::new(COURSE_SMOOTHING_WINDOW_FIXES)
    }

    /// The window length in fixes.
    pub fn window_fix_count(&self) -> usize {
        self.window_fix_count
    }

    /// Adds one fix's course-over-ground, dropping the window's oldest sample
    /// once the window is full.
    pub fn add(&mut self, course_degrees: f64) {
        self.recent_course_degrees.push(course_degrees);
        if self.recent_course_degrees.len() > self.window_fix_count {
            self.recent_course_degrees.remove(0);
        }
    }

    /// The circular median, or `None` while the window holds no sample.
    ///
    /// Ties keep the earliest sample in the window. A window containing a
    /// non-finite sample reports `None` for that comparison; a later finite
    /// sample can still become the median.
    pub fn median(&self) -> Option<f64> {
        let mut best_median = None;
        let mut best_distance_sum = f64::INFINITY;
        for candidate_degrees in &self.recent_course_degrees {
            let distance_sum = self
                .recent_course_degrees
                .iter()
                .map(|other_degrees| angular_distance_degrees(*candidate_degrees, *other_degrees))
                .sum::<f64>();
            if distance_sum < best_distance_sum {
                best_distance_sum = distance_sum;
                best_median = Some(*candidate_degrees);
            }
        }
        best_median
    }

    /// Empties the window.
    pub fn reset(&mut self) {
        self.recent_course_degrees.clear();
    }

    /// The windowed samples, oldest first.
    pub fn samples(&self) -> &[f64] {
        &self.recent_course_degrees
    }
}

impl Default for CourseMedianSmoother {
    fn default() -> Self {
        Self::with_default_window()
    }
}
