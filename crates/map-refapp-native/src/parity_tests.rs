//! Value-parity tests for the refapp-only binding wrappers.
//!
//! Each wrapper must report exactly the shared crate's values; the platform
//! suites repeat the same expected values across the UniFFI boundary. The
//! numbers are the documented behavior values, not arbitrary snapshots.

use crate::{
    CourseMedianSmoother, ExponentialValueChase, FollowBearingMode, FollowCamera, FollowPhase,
    LocationPoseAnimator, SmoothstepTimedEase, TraceCursor, bearing_off_north_degrees,
    follow_zoom_toward_trip_end, scale_bar_distance_label, scale_bar_selection,
    smoothstep_eased_progress, trace_dispatch_interval_milliseconds,
};

const TOLERANCE: f64 = 1.0e-9;

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= TOLERANCE,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn free_functions_report_the_documented_values() {
    assert_close(smoothstep_eased_progress(0.25), 0.15625);
    assert_close(smoothstep_eased_progress(0.5), 0.5);
    assert_close(bearing_off_north_degrees(350.0), 10.0);
    assert_close(follow_zoom_toward_trip_end(0.0), 17.5);
    assert_close(follow_zoom_toward_trip_end(0.5), 17.25);
    assert_close(follow_zoom_toward_trip_end(1.0), 17.0);
}

#[test]
fn scale_bar_selection_matches_the_ladder() {
    let selection = scale_bar_selection(1.0, 50.0, 120.0).expect("the ladder fits");
    assert_close(selection.distance_meters, 100.0);
    assert_close(selection.width_pixels, 100.0);

    let deep_zoom = scale_bar_selection(0.4, 50.0, 120.0).expect("the ladder fits");
    assert_close(deep_zoom.distance_meters, 30.0);
    assert_close(deep_zoom.width_pixels, 75.0);

    assert!(scale_bar_selection(f64::NAN, 50.0, 120.0).is_none());
    assert!(scale_bar_selection(0.0, 50.0, 120.0).is_none());

    assert_eq!(scale_bar_distance_label(999.9), "999 m");
    assert_eq!(scale_bar_distance_label(1000.0), "1 km");
    assert_eq!(scale_bar_distance_label(2500.0), "3 km");
}

#[test]
fn course_median_is_circular_and_windowed() {
    let smoother = CourseMedianSmoother::new(3);
    assert_eq!(smoother.median(), None);
    for course in [1.0, 2.0, 3.0, 4.0] {
        smoother.add(course);
    }
    assert_eq!(smoother.window_fix_count(), 3);
    assert_eq!(smoother.samples(), vec![2.0, 3.0, 4.0]);
    assert_close(smoother.median().expect("a median exists"), 3.0);
    smoother.reset();
    assert_eq!(smoother.median(), None);

    let wrap_smoother = CourseMedianSmoother::new(5);
    for course in [356.0, 2.0, 358.0, 4.0, 0.0] {
        wrap_smoother.add(course);
    }
    assert_close(wrap_smoother.median().expect("a median exists"), 0.0);
}

#[test]
fn timed_ease_runs_from_start_to_target_and_finishes_once() {
    let ease = SmoothstepTimedEase::new();
    assert!(ease.advance(0.0).finished);
    ease.begin(0.0, 10.0, 20.0, 300.0);
    assert!(ease.is_running());
    let start = ease.advance(0.0);
    assert_close(start.value, 10.0);
    assert!(!start.finished);
    let middle = ease.advance(150.0);
    assert_close(middle.value, 15.0);
    assert!(!middle.finished);
    let end = ease.advance(300.0);
    assert_close(end.value, 20.0);
    assert!(end.finished);
    assert_eq!(ease.target_value(), 20.0);
    assert!(!ease.is_running());
}

#[test]
fn exponential_chase_converges_and_snaps() {
    let chase = ExponentialValueChase::new(150.0);
    assert_close(chase.time_constant_milliseconds(), 150.0);
    let first = chase.advance(0.0, 10.0, 0.0);
    assert!(first > 0.0 && first < 10.0);
    let mut current = first;
    for step in 1..=200 {
        current = chase.advance(current, 10.0, f64::from(step) * 16.0);
    }
    assert!((current - 10.0).abs() < 0.1);

    let snapping = ExponentialValueChase::new(150.0);
    assert_close(snapping.advance(9.999, 10.0, 0.0), 10.0);
}

#[test]
fn follow_camera_tracks_gesture_breaks_and_restores() {
    let camera = FollowCamera::new();
    assert_eq!(camera.phase(), FollowPhase::Disengaged);

    camera.engage_trip(38.0, -121.0, 12.0, 15.0, 0.0);
    assert_eq!(camera.phase(), FollowPhase::Engaged);
    let captured = camera
        .pre_trip_camera()
        .expect("the trip captured a camera");
    assert_close(captured.center_latitude_degrees, 38.0);
    assert_close(captured.zoom_level, 12.0);
    assert_close(camera.target_zoom_level(), 17.0);
    assert_eq!(camera.target_bearing_degrees(), None);

    camera.on_fix(38.0001, -121.0001, Some(90.0), 0.8);
    assert_close(camera.target_center_latitude_degrees(), 38.0001);
    assert_close(
        camera.target_bearing_degrees().expect("a bearing exists"),
        90.0,
    );

    camera.break_on_gesture();
    assert_eq!(camera.phase(), FollowPhase::BrokenByGesture);
    camera.on_fix(38.0002, -121.0002, Some(92.0), 0.7);
    assert_eq!(camera.phase(), FollowPhase::BrokenByGesture);
    assert_close(camera.target_center_latitude_degrees(), 38.0002);
    assert_close(
        camera.target_bearing_degrees().expect("a bearing exists"),
        90.0,
    );

    camera.reengage();
    assert_eq!(camera.phase(), FollowPhase::Engaged);

    let restored = camera
        .restore_pre_trip_camera()
        .expect("a camera was captured");
    assert_close(restored.zoom_level, 12.0);
    assert_eq!(camera.phase(), FollowPhase::Disengaged);
    assert_eq!(camera.pre_trip_camera(), None);
    assert_eq!(camera.target_bearing_degrees(), None);
}

#[test]
fn trace_cursor_advances_saturates_and_seeks() {
    let cursor = TraceCursor::new(3);
    assert_eq!(cursor.sample_count(), 3);
    assert_eq!(cursor.next_sample_index(), Some(0));
    assert_eq!(cursor.peek(), Some(0));
    assert_eq!(cursor.advance(), Some(0));
    assert_eq!(cursor.advance(), Some(1));
    assert_eq!(cursor.advance(), Some(2));
    assert_eq!(cursor.advance(), None);
    assert!(cursor.is_finished());
    assert_eq!(cursor.remaining_sample_count(), 0);

    cursor.reset();
    assert_eq!(cursor.remaining_sample_count(), 3);
    cursor.seek_to_sample_index(99);
    assert!(cursor.is_finished());
    cursor.seek_to_sample_index(1);
    assert_eq!(cursor.advance(), Some(1));

    assert_eq!(trace_dispatch_interval_milliseconds(5.0, 20.0), 250);
    assert_eq!(trace_dispatch_interval_milliseconds(0.0001, 1.0), 1);
    assert_eq!(trace_dispatch_interval_milliseconds(f64::NAN, 20.0), 1);
}

#[test]
fn the_location_pose_animator_reports_the_shared_motion() {
    let animator = LocationPoseAnimator::new();
    assert!(animator.pose_at(0.0).is_none());

    animator.push_fix(0.0, 0.0, Some(359.0), Some(10.0), 0.0);
    animator.push_fix(1.0, 2.0, Some(1.0), Some(20.0), 500.0);

    let half_way = animator
        .pose_at(750.0)
        .expect("the motion runs between the fixes");
    assert_close(half_way.latitude_degrees, 0.5);
    assert_close(half_way.longitude_degrees, 1.0);
    assert_close(half_way.horizontal_accuracy_meters.expect("accuracy"), 15.0);
    assert_close(half_way.course_degrees.expect("course"), 0.0);
    assert!(half_way.is_animating);

    let settled = animator.pose_at(1000.0).expect("the motion ends");
    assert_close(settled.latitude_degrees, 1.0);
    assert!(!settled.is_animating);

    animator.reset();
    assert!(animator.pose_at(1000.0).is_none());
}

#[test]
fn the_follow_camera_holds_north_up_across_fixes() {
    let camera = FollowCamera::new();
    camera.engage_trip(38.0, -121.0, 12.0, 15.0, 0.0);
    assert_eq!(camera.bearing_mode(), FollowBearingMode::CourseUp);

    camera.on_fix(38.0001, -121.0001, Some(90.0), 0.8);
    assert_close(camera.target_bearing_degrees().unwrap(), 90.0);

    camera.set_bearing_mode(FollowBearingMode::NorthUp);
    assert_eq!(camera.bearing_mode(), FollowBearingMode::NorthUp);
    camera.on_fix(38.0002, -121.0002, Some(120.0), 0.7);
    assert_close(camera.target_bearing_degrees().unwrap(), 0.0);

    camera.set_bearing_mode(FollowBearingMode::CourseUp);
    assert_eq!(camera.bearing_mode(), FollowBearingMode::CourseUp);
    camera.on_fix(38.0003, -121.0003, Some(140.0), 0.6);
    assert_close(camera.target_bearing_degrees().unwrap(), 140.0);
}
