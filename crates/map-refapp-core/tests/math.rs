use map_refapp_core::{
    CourseMedianSmoother, ExponentialValueChase, FollowBearingMode, FollowCamera, FollowPhase,
    NAVIGATION_FOLLOW_ZOOM, NAVIGATION_FOLLOW_ZOOM_MAXIMUM, PreTripCamera,
    SCALE_BAR_WIDTH_MAXIMUM_PIXELS, SCALE_BAR_WIDTH_MINIMUM_PIXELS, SmoothstepTimedEase,
    TraceCursor, bearing_off_north_degrees, follow_zoom_toward_trip_end,
    scale_bar_distance_label_text, select_scale_bar_scale,
    select_scale_bar_scale_with_token_width_range, signed_angular_difference_degrees,
    smoothstep_eased_progress, trace_dispatch_interval_milliseconds,
};

const FLOAT_TOLERANCE: f64 = 1.0e-9;

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= FLOAT_TOLERANCE,
        "expected {expected}, got {actual}"
    );
}

#[test]
fn smoothstep_eases_both_ends_and_clamps() {
    assert_close(smoothstep_eased_progress(0.0), 0.0);
    assert_close(smoothstep_eased_progress(0.5), 0.5);
    assert_close(smoothstep_eased_progress(1.0), 1.0);
    assert_close(smoothstep_eased_progress(-2.0), 0.0);
    assert_close(smoothstep_eased_progress(3.0), 1.0);
    assert_close(smoothstep_eased_progress(0.25), 0.15625);
}

#[test]
fn bearing_off_north_wraps_into_the_shortest_direction() {
    assert_close(bearing_off_north_degrees(0.0), 0.0);
    assert_close(bearing_off_north_degrees(90.0), 90.0);
    assert_close(bearing_off_north_degrees(180.0), 180.0);
    assert_close(bearing_off_north_degrees(270.0), 90.0);
    assert_close(bearing_off_north_degrees(-90.0), 90.0);
    assert_close(bearing_off_north_degrees(350.0), 10.0);
    assert_close(bearing_off_north_degrees(725.0), 5.0);
}

#[test]
fn signed_angular_difference_takes_the_short_way() {
    assert_close(signed_angular_difference_degrees(10.0, 350.0), 20.0);
    assert_close(signed_angular_difference_degrees(350.0, 10.0), -20.0);
    assert_close(signed_angular_difference_degrees(0.0, 0.0), 0.0);
    assert_close(signed_angular_difference_degrees(180.0, 0.0), 180.0);
    assert_close(signed_angular_difference_degrees(-180.0, 0.0), -180.0);
}

#[test]
fn timed_ease_runs_from_start_to_target_and_finishes_once() {
    let mut ease = SmoothstepTimedEase::new();
    assert!(ease.advance(0.0).1);
    ease.begin(0.0, 10.0, 20.0, 300.0);
    assert!(ease.is_running());
    assert_close(ease.advance(0.0).0, 10.0);
    assert!(!ease.advance(0.0).1);
    let (mid_value, mid_finished) = ease.advance(150.0);
    assert_close(mid_value, 15.0);
    assert!(!mid_finished);
    let (final_value, finished) = ease.advance(300.0);
    assert_close(final_value, 20.0);
    assert!(finished);
    assert!(!ease.is_running());
    let (after_value, after_finished) = ease.advance(400.0);
    assert_close(after_value, 20.0);
    assert!(after_finished);
}

#[test]
fn exponential_chase_converges_and_snaps() {
    let mut chase = ExponentialValueChase::new(150.0);
    let first = chase.advance(0.0, 10.0, 0.0);
    assert!(first > 0.0 && first < 10.0);
    let mut current = first;
    for step in 1..=200 {
        current = chase.advance(current, 10.0, f64::from(step) * 16.0);
    }
    assert!((current - 10.0).abs() < 0.1);

    let mut snapping_chase = ExponentialValueChase::new(150.0);
    assert_close(snapping_chase.advance(9.999, 10.0, 0.0), 10.0);

    let mut wrap_chase = ExponentialValueChase::new(150.0);
    let wrapped = wrap_chase.advance(350.0, 10.0, 0.0);
    assert!((340.0..=360.0).contains(&wrapped));

    let mut disabled_chase = ExponentialValueChase::new(0.0);
    assert_close(disabled_chase.advance(350.0, 10.0, 0.0), 10.0);
}

#[test]
fn follow_zoom_ramps_to_the_trip_end_clamp() {
    assert_close(follow_zoom_toward_trip_end(1.0), NAVIGATION_FOLLOW_ZOOM);
    assert_close(
        follow_zoom_toward_trip_end(0.0),
        NAVIGATION_FOLLOW_ZOOM_MAXIMUM,
    );
    assert_close(
        follow_zoom_toward_trip_end(0.5),
        (NAVIGATION_FOLLOW_ZOOM + NAVIGATION_FOLLOW_ZOOM_MAXIMUM) / 2.0,
    );
    assert_close(
        follow_zoom_toward_trip_end(-4.0),
        NAVIGATION_FOLLOW_ZOOM_MAXIMUM,
    );
    assert_close(follow_zoom_toward_trip_end(4.0), NAVIGATION_FOLLOW_ZOOM);
}

#[test]
fn course_median_is_circular_and_keeps_the_earliest_tie() {
    let mut smoother = CourseMedianSmoother::new(5);
    assert_eq!(smoother.median(), None);
    smoother.add(10.0);
    smoother.add(20.0);
    smoother.add(30.0);
    assert_close(smoother.median().unwrap(), 20.0);

    let mut wrapping = CourseMedianSmoother::new(5);
    wrapping.add(350.0);
    wrapping.add(10.0);
    assert_close(wrapping.median().unwrap(), 350.0);

    let mut windowed = CourseMedianSmoother::new(3);
    for course in [1.0, 2.0, 3.0, 4.0] {
        windowed.add(course);
    }
    assert_eq!(windowed.samples(), &[2.0, 3.0, 4.0]);
    assert_close(windowed.median().unwrap(), 3.0);

    windowed.reset();
    assert_eq!(windowed.median(), None);
    assert!(windowed.samples().is_empty());
}

#[test]
fn follow_camera_tracks_gesture_breaks_and_restores() {
    let mut camera = FollowCamera::new();
    assert_eq!(camera.phase(), FollowPhase::Disengaged);

    camera.engage_trip(38.0, -121.0, 12.0, 15.0, 0.0);
    assert_eq!(camera.phase(), FollowPhase::Engaged);
    assert_eq!(
        camera.pre_trip_camera(),
        Some(PreTripCamera {
            center_latitude_degrees: 38.0,
            center_longitude_degrees: -121.0,
            zoom_level: 12.0,
            bearing_degrees: 15.0,
            pitch_degrees: 0.0,
        })
    );
    assert_close(camera.target_zoom_level(), NAVIGATION_FOLLOW_ZOOM);
    assert_eq!(camera.target_bearing_degrees(), None);

    camera.on_fix(38.0001, -121.0001, Some(90.0), 0.8);
    assert_close(camera.target_center_latitude_degrees(), 38.0001);
    assert_close(camera.target_bearing_degrees().unwrap(), 90.0);

    camera.break_on_gesture();
    assert_eq!(camera.phase(), FollowPhase::BrokenByGesture);
    camera.on_fix(38.0002, -121.0002, Some(92.0), 0.7);
    assert_eq!(camera.phase(), FollowPhase::BrokenByGesture);
    assert_close(camera.target_center_latitude_degrees(), 38.0002);
    // The circular median is a windowed sample; the [90, 92] tie keeps 90.
    assert_close(camera.target_bearing_degrees().unwrap(), 90.0);

    camera.reengage();
    assert_eq!(camera.phase(), FollowPhase::Engaged);

    let restored = camera.restore_pre_trip_camera();
    assert_eq!(restored.unwrap().zoom_level, 12.0);
    assert_eq!(camera.phase(), FollowPhase::Disengaged);
    assert_eq!(camera.pre_trip_camera(), None);
    assert_eq!(camera.target_bearing_degrees(), None);
}

#[test]
fn north_up_holds_across_fixes_and_course_up_resumes_the_chase() {
    let mut camera = FollowCamera::new();
    camera.engage_trip(38.0, -121.0, 12.0, 15.0, 0.0);
    assert_eq!(camera.bearing_mode(), FollowBearingMode::CourseUp);

    camera.on_fix(38.0001, -121.0001, Some(90.0), 0.8);
    assert_close(camera.target_bearing_degrees().unwrap(), 90.0);

    // The visitor chooses north-up; the next fixes keep bearing 0.
    camera.set_bearing_mode(FollowBearingMode::NorthUp);
    assert_close(camera.target_bearing_degrees().unwrap(), 0.0);
    camera.on_fix(38.0002, -121.0002, Some(120.0), 0.7);
    assert_close(camera.target_bearing_degrees().unwrap(), 0.0);
    camera.break_on_gesture();
    camera.on_fix(38.0003, -121.0003, Some(135.0), 0.6);
    camera.reengage();
    assert_close(camera.target_bearing_degrees().unwrap(), 0.0);

    // Course-up resumes the eased chase on the next fix.
    camera.set_bearing_mode(FollowBearingMode::CourseUp);
    assert_eq!(camera.target_bearing_degrees(), None);
    camera.on_fix(38.0004, -121.0004, Some(140.0), 0.5);
    assert_close(camera.target_bearing_degrees().unwrap(), 140.0);

    // A new trip starts course-up again.
    camera.set_bearing_mode(FollowBearingMode::NorthUp);
    camera.restore_pre_trip_camera();
    camera.engage_trip(38.0, -121.0, 12.0, 15.0, 0.0);
    assert_eq!(camera.bearing_mode(), FollowBearingMode::CourseUp);
    camera.on_fix(38.0005, -121.0005, Some(20.0), 0.9);
    assert_close(camera.target_bearing_degrees().unwrap(), 20.0);
}

#[test]
fn scale_bar_selects_the_largest_fitting_ladder_distance() {
    let one_meter_per_pixel = select_scale_bar_scale_with_token_width_range(1.0).unwrap();
    assert_close(one_meter_per_pixel.distance_meters, 100.0);
    assert_close(one_meter_per_pixel.width_pixels, 100.0);

    let ten_meters_per_pixel = select_scale_bar_scale_with_token_width_range(10.0).unwrap();
    assert_close(ten_meters_per_pixel.distance_meters, 1000.0);
    assert_close(ten_meters_per_pixel.width_pixels, 100.0);

    let half_meter_per_pixel = select_scale_bar_scale_with_token_width_range(0.5).unwrap();
    assert_close(half_meter_per_pixel.distance_meters, 50.0);
    assert_close(half_meter_per_pixel.width_pixels, 100.0);

    let explicit_range = select_scale_bar_scale(1.0, 10.0, 20.0).unwrap();
    assert_close(explicit_range.distance_meters, 20.0);

    assert_eq!(select_scale_bar_scale_with_token_width_range(0.0), None);
    assert_eq!(select_scale_bar_scale_with_token_width_range(-1.0), None);
    assert_eq!(
        select_scale_bar_scale_with_token_width_range(f64::NAN),
        None
    );
    assert_eq!(
        select_scale_bar_scale(
            1.0,
            SCALE_BAR_WIDTH_MAXIMUM_PIXELS,
            SCALE_BAR_WIDTH_MINIMUM_PIXELS
        ),
        None
    );
}

#[test]
fn scale_bar_label_uses_metres_under_a_kilometre() {
    assert_eq!(scale_bar_distance_label_text(999.9), "999 m");
    assert_eq!(scale_bar_distance_label_text(1000.0), "1 km");
    assert_eq!(scale_bar_distance_label_text(2500.0), "3 km");
}

#[test]
fn trace_cursor_advances_and_saturates() {
    let mut cursor = TraceCursor::new(3);
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
}

#[test]
fn trace_dispatch_interval_follows_the_acceleration_factor() {
    assert_eq!(trace_dispatch_interval_milliseconds(5.0, 20.0), 250);
    assert_eq!(trace_dispatch_interval_milliseconds(1.0, 1.0), 1000);
    assert_eq!(trace_dispatch_interval_milliseconds(0.001, 1.0), 1);
    assert_eq!(trace_dispatch_interval_milliseconds(0.0001, 1.0), 1);
    assert_eq!(trace_dispatch_interval_milliseconds(5.0, 0.0), 1);
    assert_eq!(trace_dispatch_interval_milliseconds(f64::NAN, 20.0), 1);
    assert_eq!(trace_dispatch_interval_milliseconds(5.0, f64::INFINITY), 1);
}
