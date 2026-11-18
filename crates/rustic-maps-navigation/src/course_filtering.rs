//! Course filtering: how a position fix's reported course becomes the trip's
//! reported course.
//!
//! A coarse GNSS fix's course over ground is often the least trustworthy
//! field the fix carries — a momentary standstill or an urban canyon reads as
//! an arbitrary bearing, while the trip's own route line is a much stronger
//! statement about the real direction of travel. The policy is chosen once
//! per trip:
//!
//! - [`CourseFiltering::Raw`] reports the fix's course verbatim — the tunnel
//!   or canyon exit hatch where a garbage route projection must not override
//!   a correct physical course.
//! - [`CourseFiltering::SnapToRoute`] projects the course onto the route line:
//!   while the fix is not judged fully off route, the reported course is the
//!   bearing of the route line at the user's snapped position — every
//!   materially disagreeing raw course is replaced in one step, and the course
//!   stops jittering against the very line the position was already snapped
//!   onto. Without a route line to trust (the fix did not snap, or the
//!   projected segment carries no direction), the fix's own course remains the
//!   fallback, and an unreported course retains the trip's previous course
//!   (the same degradation continuity the snapped position keeps).
//!
//! The filtering is decided per trip, like the step-advance and deviation
//! policies ([`crate::reducer::NavigationEvent::StartTrip`] carries it), and
//! the reducer stamps every fix's effective course into
//! [`TripNavigationState::course_heading_degrees`](crate::state::TripNavigationState::course_heading_degrees)
//! during the same position snap pass, so consumers (a course dart, a follow
//! camera) read one course field instead of re-filtering raw courses per
//! platform.

/// How the trip reports its course.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum CourseFiltering {
    /// The fix's reported course travels with the fix unchanged: the trust in
    /// the physical course overrides what the route line says (the raw-course
    /// escape hatch for tunnels and canyons).
    #[default]
    Raw,
    /// The reported course is snapped onto the route line's direction at the
    /// user's projected position (the module documentation's semantics).
    SnapToRoute,
}

/// Computes the effective course of one processed fix under the trip's
/// course filtering.
///
/// `raw_course_heading_degrees` is the fix's own reported course;
/// `snapped_bearing_degrees` the route line's bearing at the fix's snapped
/// position (per [`PolylineSnap::bearing_degrees`]); `previous_course_heading_degrees`
/// the effective course currently on the trip's state, retained when nothing
/// newer can be reported (the snapped-position retention rule). `None` on
/// either source means "not reported here".
pub fn effective_course_heading_degrees(
    course_filtering: CourseFiltering,
    raw_course_heading_degrees: Option<f64>,
    snapped_bearing_degrees: Option<f64>,
    previous_course_heading_degrees: Option<f64>,
) -> Option<f64> {
    match course_filtering {
        CourseFiltering::Raw => raw_course_heading_degrees,
        CourseFiltering::SnapToRoute => snapped_bearing_degrees
            .or(raw_course_heading_degrees)
            .or(previous_course_heading_degrees),
    }
}

#[cfg(test)]
mod tests {
    use super::{CourseFiltering, effective_course_heading_degrees};

    /// The raw course under test: a fix reporting some physically plausible
    /// but route-irrelevant bearing.
    const RAW_COURSE_DEGREES: f64 = 200.0;
    /// The route line's bearing at the snapped position under test (due
    /// eastward, the synthetic walker routes' direction).
    const ROUTE_BEARING_DEGREES: f64 = 90.0;
    /// The previous effective course for the retention arms of the fallback
    /// chain.
    const PREVIOUS_COURSE_DEGREES: f64 = 42.0;
    /// Tolerance for course measurements compared by value.
    const BEARING_TOLERANCE_DEGREES: f64 = 1e-6;

    fn assert_close(actual: Option<f64>, expected: f64, explanation: &str) {
        let actual = actual.expect("a course must be reported in this case");
        assert!(
            (actual - expected).abs() < BEARING_TOLERANCE_DEGREES,
            "{explanation}: expected {expected}, got {actual}"
        );
    }

    #[test]
    fn raw_filtering_reports_the_fixs_own_course_verbatim() {
        let course = effective_course_heading_degrees(
            CourseFiltering::Raw,
            Some(RAW_COURSE_DEGREES),
            Some(ROUTE_BEARING_DEGREES),
            Some(PREVIOUS_COURSE_DEGREES),
        );
        assert_close(
            course,
            RAW_COURSE_DEGREES,
            "Raw must never rewrite the reported course",
        );
    }

    #[test]
    fn raw_filtering_reports_nothing_without_a_report() {
        assert_eq!(
            effective_course_heading_degrees(
                CourseFiltering::Raw,
                None,
                Some(ROUTE_BEARING_DEGREES),
                Some(PREVIOUS_COURSE_DEGREES)
            ),
            None,
            "Raw without a reported course reports none"
        );
    }

    #[test]
    fn snap_to_route_reports_the_route_bearing_while_it_exists() {
        let course = effective_course_heading_degrees(
            CourseFiltering::SnapToRoute,
            Some(RAW_COURSE_DEGREES),
            Some(ROUTE_BEARING_DEGREES),
            Some(PREVIOUS_COURSE_DEGREES),
        );
        assert_close(
            course,
            ROUTE_BEARING_DEGREES,
            "on route, the snapped route bearing replaces the raw course",
        );
    }

    #[test]
    fn snap_to_route_falls_back_to_the_fix_without_a_route_bearing() {
        // The fix did not snap onto a directed line (off route, or a segment
        // without a representable direction): the fix's own course remains.
        let course = effective_course_heading_degrees(
            CourseFiltering::SnapToRoute,
            Some(RAW_COURSE_DEGREES),
            None,
            Some(PREVIOUS_COURSE_DEGREES),
        );
        assert_close(
            course,
            RAW_COURSE_DEGREES,
            "off the route line, the raw course is the fall-back",
        );
    }

    #[test]
    fn snap_to_route_falls_back_to_the_previous_course_without_any_report() {
        // Neither a route bearing nor a fresh fix course: the trip's last
        // effective course is retained (the position snap's retention rule).
        assert_eq!(
            effective_course_heading_degrees(
                CourseFiltering::SnapToRoute,
                None,
                None,
                Some(PREVIOUS_COURSE_DEGREES)
            ),
            Some(PREVIOUS_COURSE_DEGREES),
            "an unreported fix retains the trip's last course"
        );
        assert_eq!(
            effective_course_heading_degrees(CourseFiltering::SnapToRoute, None, None, None),
            None,
            "and an unreported fix on a courseless trip stays courseless"
        );
    }

    #[test]
    fn the_default_filtering_is_raw() {
        assert_eq!(
            CourseFiltering::default(),
            CourseFiltering::Raw,
            "the tripless default must keep the raw-course behavior"
        );
    }
}
