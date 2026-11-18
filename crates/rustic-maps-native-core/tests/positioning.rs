//! The contract's positioning controller: validation, course smoothing across
//! north, source failures and staleness, exercised through the same records a
//! platform binding uses.

use rustic_maps::{GeographicCoordinate, PositionController, PositionFix, PositionSourceError};

/// Builds one fix at the demo anchor, with the given course.
fn fix(course_heading_degrees: Option<f64>) -> PositionFix {
    PositionFix {
        coordinate: GeographicCoordinate {
            latitude: 38.5766,
            longitude: -121.4944,
        },
        horizontal_accuracy_meters: Some(5.0),
        course_heading_degrees,
        timestamp_seconds: Some(100.0),
    }
}

#[test]
fn a_new_controller_has_no_fix() {
    let controller = PositionController::new();
    assert!(controller.latest_fix().is_none());
    assert!(controller.smoothed_course_degrees().is_none());
    assert_eq!(controller.source_status(), None);
}

#[test]
fn fixes_are_validated_and_courses_smooth_across_north() {
    let controller = PositionController::new();
    controller
        .feed_fix(fix(Some(350.0)))
        .expect("a valid fix is accepted");
    let mut second_fix = fix(Some(10.0));
    second_fix.coordinate.latitude = 38.5767;
    controller
        .feed_fix(second_fix)
        .expect("a valid fix is accepted");
    let smoothed = controller
        .smoothed_course_degrees()
        .expect("the fixes carried courses");
    assert!(
        smoothed < 5.0 || smoothed > 355.0,
        "350 and 10 average to about north, got {smoothed}"
    );

    let mut invalid_fix = fix(None);
    invalid_fix.horizontal_accuracy_meters = Some(-1.0);
    assert!(
        controller.feed_fix(invalid_fix).is_err(),
        "a negative accuracy is refused"
    );
    assert!(controller.latest_fix().is_some());
}

#[test]
fn source_failures_and_staleness_are_reported() {
    let controller = PositionController::new();
    controller
        .feed_fix(fix(None))
        .expect("a valid fix is accepted");
    assert!(!controller.is_stale(104.0));
    assert!(controller.is_stale(111.0));

    controller.source_failed(PositionSourceError::PermissionDenied);
    assert_eq!(
        controller.source_status(),
        Some(PositionSourceError::PermissionDenied)
    );
    let mut recovered_fix = fix(None);
    recovered_fix.coordinate.latitude = 38.5767;
    controller
        .feed_fix(recovered_fix)
        .expect("a recovering fix is accepted");
    assert_eq!(controller.source_status(), None);
}
