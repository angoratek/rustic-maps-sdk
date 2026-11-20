//! End-to-end tests of the contract camera's gesture members (M17's fourth
//! slice): `pan_by_screen_pixels`, `zoom_by_levels_around_screen_point` and
//! `rotate_by_degrees_around_screen_point` delegate to the `rustic-maps-rendering`
//! `CameraInteraction` seam through the binding wrapper, so the answers the
//! four shells apply are the same camera algebra — pan tracks the pointer,
//! focal zoom and rotation hold the ground point under the focal pixel, and
//! the wrapper's viewport validation rejects unusable input with the
//! verbatim message.

mod common;

use common::{coordinate, viewport};
use rustic_maps::{MapCamera, ScreenPoint, SdkError, ViewportSize};
use rustic_maps_test_support::{
    SACRAMENTO_CAMERA_ZOOM_LEVEL as FIXTURE_ZOOM_LEVEL,
    SACRAMENTO_LATITUDE_DEGREES as FIXTURE_LATITUDE_DEGREES,
    SACRAMENTO_LONGITUDE_DEGREES as FIXTURE_LONGITUDE_DEGREES,
};

/// Bearing of the fixture camera, in degrees.
const FIXTURE_BEARING_DEGREES: f64 = 25.0;
/// Pitch of the fixture camera, in degrees: steep enough that the focal
/// corrections run under a real perspective, not only top-down.
const FIXTURE_PITCH_DEGREES: f64 = 30.0;
/// Viewport width the gesture tests use, in pixels.
const VIEWPORT_WIDTH_PIXELS: f64 = 1024.0;
/// Viewport height the gesture tests use, in pixels.
const VIEWPORT_HEIGHT_PIXELS: f64 = 768.0;
/// Field of view the gesture tests use, in degrees.
const FIELD_OF_VIEW_DEGREES: f64 = 60.0;
/// Pointer travel of the pan test, in screen pixels: right and up.
const PAN_DELTA_X_PIXELS: f64 = 64.0;
/// Pointer travel of the pan test, in screen pixels: right and up.
const PAN_DELTA_Y_PIXELS: f64 = -48.0;
/// Focal pixel of the zoom test: a quarter of the viewport in from the top
/// left, visibly off the center so the hold correction has work to do.
const FOCAL_X_PIXELS: f64 = VIEWPORT_WIDTH_PIXELS * 0.25;
/// Focal pixel of the zoom test, vertical.
const FOCAL_Y_PIXELS: f64 = VIEWPORT_HEIGHT_PIXELS * 0.25;
/// Zoom delta of the focal zoom test, in levels: one level in.
const ZOOM_DELTA_LEVELS: f64 = 1.0;
/// Bearing delta of the rotate test, in degrees.
const ROTATION_DELTA_DEGREES: f64 = 45.0;
/// Bearing delta that overshoots a full turn, in degrees: 25 + 400 wraps to
/// 65.
const ROTATION_WRAP_DELTA_DEGREES: f64 = 400.0;
/// Bearing the wrapped rotation must answer, in degrees.
const EXPECTED_WRAPPED_BEARING_DEGREES: f64 = 65.0;
/// Pitch of the above-the-horizon pan test, in degrees: the test camera's
/// maximum pitch.
const MAXIMUM_PITCH_DEGREES: f64 = 75.0;
/// Headroom for a screen pixel landing under its expected position; the
/// camera tests permit sub-hundredth-pixel matrix round-trip drift.
const PIXEL_TOLERANCE: f64 = 1e-2;
/// Headroom for the zoom level arriving exactly.
const ZOOM_LEVEL_TOLERANCE: f64 = 1e-9;
/// Headroom for bearing wrap arithmetic.
const BEARING_DEGREES_TOLERANCE: f64 = 1e-9;
/// Headroom for a held ground coordinate round-tripping through the
/// projection, in degrees.
const COORDINATE_TOLERANCE_DEGREES: f64 = 1e-6;
/// Verbatim detail of the viewport reject the gesture members share, with
/// the invalid zero width spelled as the validator formats it.
const INVALID_VIEWPORT_DETAIL: &str = "viewport extent 0x768 must be positive";

fn viewport_center() -> ScreenPoint {
    ScreenPoint {
        x: VIEWPORT_WIDTH_PIXELS * 0.5,
        y: VIEWPORT_HEIGHT_PIXELS * 0.5,
    }
}

fn fixture_camera() -> MapCamera {
    MapCamera::new(
        coordinate(FIXTURE_LATITUDE_DEGREES, FIXTURE_LONGITUDE_DEGREES),
        FIXTURE_ZOOM_LEVEL,
        FIXTURE_BEARING_DEGREES,
        FIXTURE_PITCH_DEGREES,
    )
    .expect("the fixture camera's center is canonical")
}

#[test]
fn pan_tracks_the_pointers_ground_point_to_the_viewport_center() {
    let camera = fixture_camera();
    let viewport = viewport();
    // The seam's contract: the pointer traveled `delta` and ended at the
    // viewport center, so the ground point it started over is the new camera
    // center and now projects back to the viewport center.
    let pointer_start = ScreenPoint {
        x: viewport_center().x - PAN_DELTA_X_PIXELS,
        y: viewport_center().y - PAN_DELTA_Y_PIXELS,
    };
    let grabbed_ground = camera
        .screen_to_ground_coordinate(pointer_start, viewport, FIELD_OF_VIEW_DEGREES)
        .expect("the viewport is valid")
        .expect("the pointer start ray reaches the ground");
    let panned = camera
        .pan_by_screen_pixels(
            PAN_DELTA_X_PIXELS,
            PAN_DELTA_Y_PIXELS,
            viewport,
            FIELD_OF_VIEW_DEGREES,
        )
        .expect("the viewport is valid");
    let followed_point = panned
        .project_to_screen(grabbed_ground, viewport, FIELD_OF_VIEW_DEGREES)
        .expect("the viewport is valid")
        .expect("the grabbed ground point stays in front of the camera");
    assert!(
        (followed_point.x - viewport_center().x).abs() < PIXEL_TOLERANCE
            && (followed_point.y - viewport_center().y).abs() < PIXEL_TOLERANCE,
        "the grabbed ground must land at the viewport center: {followed_point:?}"
    );
    // The pan's answer is anchored on that grabbed ground point.
    let panned_center = panned.center();
    assert!(
        (panned_center.latitude - grabbed_ground.latitude).abs() < COORDINATE_TOLERANCE_DEGREES
            && (panned_center.longitude - grabbed_ground.longitude).abs()
                < COORDINATE_TOLERANCE_DEGREES,
        "the panned center must be the grabbed ground: {panned_center:?} vs {grabbed_ground:?}"
    );
    // The pan carries the pose's other attributes through unchanged.
    assert!((panned.zoom_level() - camera.zoom_level()).abs() < ZOOM_LEVEL_TOLERANCE);
    assert!(
        (panned.bearing_degrees() - camera.bearing_degrees()).abs() < BEARING_DEGREES_TOLERANCE
    );
    assert!((panned.pitch_degrees() - camera.pitch_degrees()).abs() < BEARING_DEGREES_TOLERANCE);
}

#[test]
fn pan_above_the_horizon_answers_the_camera_unchanged() {
    let camera = MapCamera::new(
        coordinate(FIXTURE_LATITUDE_DEGREES, FIXTURE_LONGITUDE_DEGREES),
        FIXTURE_ZOOM_LEVEL,
        0.0,
        MAXIMUM_PITCH_DEGREES,
    )
    .expect("the camera's center is canonical");
    let viewport = viewport();
    // At maximum pitch the mirror of a full-viewport travel reaches the sky,
    // so the whole delta drops (the seam's whole-delta rule).
    let panned = camera
        .pan_by_screen_pixels(
            VIEWPORT_WIDTH_PIXELS,
            VIEWPORT_HEIGHT_PIXELS,
            viewport,
            FIELD_OF_VIEW_DEGREES,
        )
        .expect("the viewport is valid");
    assert_eq!(panned.center(), camera.center());
    assert!((panned.zoom_level() - camera.zoom_level()).abs() < ZOOM_LEVEL_TOLERANCE);
    assert!(
        (panned.bearing_degrees() - camera.bearing_degrees()).abs() < BEARING_DEGREES_TOLERANCE
    );
    assert!((panned.pitch_degrees() - camera.pitch_degrees()).abs() < BEARING_DEGREES_TOLERANCE);
}

#[test]
fn focal_zoom_keeps_the_world_point_under_the_focal_pixel() {
    let camera = fixture_camera();
    let viewport = viewport();
    let focal_point = ScreenPoint {
        x: FOCAL_X_PIXELS,
        y: FOCAL_Y_PIXELS,
    };
    let held_ground = camera
        .screen_to_ground_coordinate(focal_point, viewport, FIELD_OF_VIEW_DEGREES)
        .expect("the viewport is valid")
        .expect("the focal ray reaches the ground");
    let zoomed = camera
        .zoom_by_levels_around_screen_point(
            ZOOM_DELTA_LEVELS,
            focal_point,
            viewport,
            FIELD_OF_VIEW_DEGREES,
        )
        .expect("the viewport is valid");
    assert!(
        (zoomed.zoom_level() - (camera.zoom_level() + ZOOM_DELTA_LEVELS)).abs()
            < ZOOM_LEVEL_TOLERANCE
    );
    let held_point_after = zoomed
        .project_to_screen(held_ground, viewport, FIELD_OF_VIEW_DEGREES)
        .expect("the viewport is valid")
        .expect("the held ground point stays in front of the camera");
    assert!(
        (held_point_after.x - focal_point.x).abs() < PIXEL_TOLERANCE
            && (held_point_after.y - focal_point.y).abs() < PIXEL_TOLERANCE,
        "the focal point must stay under its pixel: {held_point_after:?} vs {focal_point:?}"
    );
    // The held ground point also survives an unprojection round trip, and
    // bearing and pitch ride through.
    let held_ground_after = zoomed
        .screen_to_ground_coordinate(focal_point, viewport, FIELD_OF_VIEW_DEGREES)
        .expect("the viewport is valid")
        .expect("the focal ray still reaches the ground");
    assert!(
        (held_ground_after.latitude - held_ground.latitude).abs() < COORDINATE_TOLERANCE_DEGREES
            && (held_ground_after.longitude - held_ground.longitude).abs()
                < COORDINATE_TOLERANCE_DEGREES
    );
    assert!(
        (zoomed.bearing_degrees() - camera.bearing_degrees()).abs() < BEARING_DEGREES_TOLERANCE
    );
    assert!((zoomed.pitch_degrees() - camera.pitch_degrees()).abs() < BEARING_DEGREES_TOLERANCE);
}

#[test]
fn rotation_changes_the_bearing_by_the_requested_degrees() {
    let camera = fixture_camera();
    let viewport = viewport();
    // Rotating about the viewport center keeps the center anchored, so the
    // rotation's whole effect is the bearing delta.
    let rotated = camera
        .rotate_by_degrees_around_screen_point(
            ROTATION_DELTA_DEGREES,
            viewport_center(),
            viewport,
            FIELD_OF_VIEW_DEGREES,
        )
        .expect("the viewport is valid");
    assert!(
        (rotated.bearing_degrees() - (camera.bearing_degrees() + ROTATION_DELTA_DEGREES)).abs()
            < BEARING_DEGREES_TOLERANCE
    );
    assert!(
        (camera.center().latitude - rotated.center().latitude).abs() < COORDINATE_TOLERANCE_DEGREES
    );
    assert!(
        (camera.center().longitude - rotated.center().longitude).abs()
            < COORDINATE_TOLERANCE_DEGREES
    );
    assert!((rotated.zoom_level() - camera.zoom_level()).abs() < ZOOM_LEVEL_TOLERANCE);
    assert!((rotated.pitch_degrees() - camera.pitch_degrees()).abs() < BEARING_DEGREES_TOLERANCE);
    // A delta past a full turn wraps through the camera constructor.
    let wrapped = camera
        .rotate_by_degrees_around_screen_point(
            ROTATION_WRAP_DELTA_DEGREES,
            viewport_center(),
            viewport,
            FIELD_OF_VIEW_DEGREES,
        )
        .expect("the viewport is valid");
    assert!(
        (wrapped.bearing_degrees() - EXPECTED_WRAPPED_BEARING_DEGREES).abs()
            < BEARING_DEGREES_TOLERANCE
    );
}

#[test]
fn gesture_members_reject_an_unusable_viewport_with_the_verbatim_detail() {
    let camera = fixture_camera();
    let invalid_viewport = ViewportSize {
        width_pixels: 0.0,
        height_pixels: VIEWPORT_HEIGHT_PIXELS,
    };
    let expected_error = SdkError::InvalidCoordinateInput {
        detail: INVALID_VIEWPORT_DETAIL.to_string(),
    };
    assert_eq!(
        camera
            .pan_by_screen_pixels(
                PAN_DELTA_X_PIXELS,
                PAN_DELTA_Y_PIXELS,
                invalid_viewport,
                FIELD_OF_VIEW_DEGREES,
            )
            .expect_err("a zero-width viewport must throw"),
        expected_error
    );
    assert_eq!(
        camera
            .zoom_by_levels_around_screen_point(
                ZOOM_DELTA_LEVELS,
                viewport_center(),
                invalid_viewport,
                FIELD_OF_VIEW_DEGREES,
            )
            .expect_err("a zero-width viewport must throw"),
        expected_error
    );
    assert_eq!(
        camera
            .rotate_by_degrees_around_screen_point(
                ROTATION_DELTA_DEGREES,
                viewport_center(),
                invalid_viewport,
                FIELD_OF_VIEW_DEGREES,
            )
            .expect_err("a zero-width viewport must throw"),
        expected_error
    );
}
