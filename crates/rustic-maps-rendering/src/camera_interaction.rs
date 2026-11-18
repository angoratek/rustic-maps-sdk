//! Camera adjustments driven by platform gesture input.
//!
//! A platform recognizes gestures (pointer capture, momentum, pinch to
//! zoom, and so on) and forwards plain deltas; [`CameraInteraction`] owns the
//! camera adjustment those deltas imply. Gesture recognition itself, delta
//! shaping (for example wheel ticks to zoom levels), inertia after a gesture
//! ends, and any zoom-range policy beyond the camera's own clamp stay on the
//! platform side.
//!
//! Each method takes the current camera in and returns the adjusted camera,
//! so the caller keeps the live pose itself. The methods reuse
//! the anchored camera's clamping and projection math rather than duplicating
//! it.
//!
//! The focal-point correction is exact, not iterative: a screen pixel's
//! ground ray direction depends only on bearing, pitch, field of view and
//! viewport — never on zoom or camera position — so after a zoom or rotate
//! delta, translating the camera anchor by the ground offset between where
//! the candidate camera projects the focal pixel and the held ground point
//! puts the world point back under the focal pixel at any pitch.

use crate::camera::{MapCamera, ViewportPixels};
use crate::projection::{coordinate_to_normalized_world, normalized_world_to_coordinate};
use geospatial_types::GeographicCoordinate;

/// Gesture-delta camera adjustments. Every method takes the pose in, answers
/// the resulting pose out, and never mutates: [`MapCamera`] is small and
/// `Copy`, so callers keep the live pose itself while applying
/// deltas.
///
/// Pan, focal zoom and rotation are the supported deltas; the camera's own
/// clamps (zoom `[0, 24]`, pitch
/// `[0, 75]°`, bearing wrapped into `[0, 360)`) and Mercator latitude bounds
/// always hold on the answer.
pub trait CameraInteraction {
    /// Pans by a pointer travel of `delta_pixels` (screen pixels, x right /
    /// y down): the ground point the pointer just moved over becomes the new
    /// camera center, so map content tracks the pointer exactly. Answers the
    /// camera unchanged when the delta's ground ray misses the plane (above
    /// the horizon at steep pitch) — the whole-delta rule a pointer-driven map
    /// applies.
    fn pan_by_screen_pixels(
        &self,
        delta_pixels: [f64; 2],
        viewport: &ViewportPixels,
        field_of_view_degrees: f64,
    ) -> MapCamera;

    /// Zooms by `delta_zoom_levels` (levels; positive zooms in) keeping the
    /// world point under `focal_point` fixed under it. Answers the zoom
    /// without the focal correction when the focal ray misses the ground
    /// plane (above the horizon at steep pitch). Zoom clamps to the camera's
    /// own level range before the focal correction applies, so a clamped
    /// zoom still holds the focal point.
    fn zoom_by_levels_around_screen_point(
        &self,
        delta_zoom_levels: f64,
        focal_point: [f64; 2],
        viewport: &ViewportPixels,
        field_of_view_degrees: f64,
    ) -> MapCamera;

    /// Rotates the bearing by `delta_bearing_degrees` (degrees clockwise,
    /// wrapping into `[0, 360)`) keeping the world point under `focal_point`
    /// fixed under it. Answers the rotation without the focal correction
    /// when the focal ray misses the ground plane — rotating about the focal
    /// pixel, the touch-point turn a two-finger gesture implies.
    fn rotate_by_degrees_around_screen_point(
        &self,
        delta_bearing_degrees: f64,
        focal_point: [f64; 2],
        viewport: &ViewportPixels,
        field_of_view_degrees: f64,
    ) -> MapCamera;
}

impl CameraInteraction for MapCamera {
    fn pan_by_screen_pixels(
        &self,
        delta_pixels: [f64; 2],
        viewport: &ViewportPixels,
        field_of_view_degrees: f64,
    ) -> MapCamera {
        let viewport_center = viewport_center_pixels(viewport);
        let ground_before =
            self.screen_to_ground_coordinate(viewport_center, viewport, field_of_view_degrees);
        let ground_after = self.screen_to_ground_coordinate(
            [
                viewport_center[0] - delta_pixels[0],
                viewport_center[1] - delta_pixels[1],
            ],
            viewport,
            field_of_view_degrees,
        );
        let (ground_before, ground_after) = match (ground_before, ground_after) {
            (Some(before), Some(after)) => (before, after),
            // A ray above the horizon carries no ground to pan with; the
            // whole delta drops (the reference consumer's rule).
            _ => return *self,
        };
        let center_shifted =
            normalized_world_shifted_by_ground_delta(&self.center, &ground_before, &ground_after);
        MapCamera::new(
            center_shifted,
            self.zoom_level,
            self.bearing_degrees,
            self.pitch_degrees,
        )
    }

    fn zoom_by_levels_around_screen_point(
        &self,
        delta_zoom_levels: f64,
        focal_point: [f64; 2],
        viewport: &ViewportPixels,
        field_of_view_degrees: f64,
    ) -> MapCamera {
        let held_ground =
            self.screen_to_ground_coordinate(focal_point, viewport, field_of_view_degrees);
        let zoomed = MapCamera::new(
            self.center,
            self.zoom_level + delta_zoom_levels,
            self.bearing_degrees,
            self.pitch_degrees,
        );
        camera_holding_focal_ground_point(
            zoomed,
            held_ground,
            focal_point,
            viewport,
            field_of_view_degrees,
        )
    }

    fn rotate_by_degrees_around_screen_point(
        &self,
        delta_bearing_degrees: f64,
        focal_point: [f64; 2],
        viewport: &ViewportPixels,
        field_of_view_degrees: f64,
    ) -> MapCamera {
        let held_ground =
            self.screen_to_ground_coordinate(focal_point, viewport, field_of_view_degrees);
        let rotated = MapCamera::new(
            self.center,
            self.zoom_level,
            self.bearing_degrees + delta_bearing_degrees,
            self.pitch_degrees,
        );
        camera_holding_focal_ground_point(
            rotated,
            held_ground,
            focal_point,
            viewport,
            field_of_view_degrees,
        )
    }
}

/// The viewport center in pixels (origin top-left, y down).
fn viewport_center_pixels(viewport: &ViewportPixels) -> [f64; 2] {
    [viewport.width_pixels * 0.5, viewport.height_pixels * 0.5]
}

/// Normalized-world ground shift below which a delta is treated as the
/// identity: the base coordinate stays verbatim instead of a Mercator round
/// trip whose last-ulp inversion noise would otherwise move the center
/// (e.g. a zero wheel delta, or a rotation about the viewport center). A
/// shift this small is ~0.04 millimeters on the ground.
const IDENTITY_GROUND_DELTA_TOLERANCE: f64 = 1e-12;

/// The `base` coordinate shifted by the normalized-world delta from `before`
/// to `after`: longitude takes the shortest wrap-around path, latitude rides
/// the Mercator difference and clamps at the map square like any conversion.
/// An (effectively) zero delta answers `base` verbatim — the conversion
/// round trip alone would drift the last mantissa digits of the latitude.
fn normalized_world_shifted_by_ground_delta(
    base: &GeographicCoordinate,
    before: &GeographicCoordinate,
    after: &GeographicCoordinate,
) -> GeographicCoordinate {
    let base_world = coordinate_to_normalized_world(base);
    let before_world = coordinate_to_normalized_world(before);
    let after_world = coordinate_to_normalized_world(after);
    let x_delta = crate::camera::normalized_shortest_delta(before_world[0], after_world[0]);
    let y_delta = after_world[1] - before_world[1];
    if x_delta.abs() <= IDENTITY_GROUND_DELTA_TOLERANCE
        && y_delta.abs() <= IDENTITY_GROUND_DELTA_TOLERANCE
    {
        return *base;
    }
    normalized_world_to_coordinate([base_world[0] + x_delta, base_world[1] + y_delta])
}

/// The `candidate` camera shifted so the world point `held_ground` stays
/// under `focal_point`: the pixel ray of `focal_point` keeps its direction
/// across the shift (bearing, pitch, field of view and viewport are
/// unchanged), so the anchor translation that puts `held_ground` back under
/// the focal pixel is the ground offset between held and where `candidate`
/// unprojects the focal pixel. Answers `candidate` unchanged when either ray
/// misses the ground plane — the focal correction is then impossible, not
/// skipped-on-purpose.
fn camera_holding_focal_ground_point(
    candidate: MapCamera,
    held_ground: Option<GeographicCoordinate>,
    focal_point: [f64; 2],
    viewport: &ViewportPixels,
    field_of_view_degrees: f64,
) -> MapCamera {
    let candidate_focal_ground =
        candidate.screen_to_ground_coordinate(focal_point, viewport, field_of_view_degrees);
    let (held_ground, candidate_focal_ground) = match (held_ground, candidate_focal_ground) {
        (Some(held), Some(focal_ground)) => (held, focal_ground),
        _ => return candidate,
    };
    let center_shifted = normalized_world_shifted_by_ground_delta(
        &candidate.center,
        &candidate_focal_ground,
        &held_ground,
    );
    MapCamera::new(
        center_shifted,
        candidate.zoom_level,
        candidate.bearing_degrees,
        candidate.pitch_degrees,
    )
}

#[cfg(test)]
mod tests {
    use super::{CameraInteraction, viewport_center_pixels};
    use crate::camera::{
        DEFAULT_FIELD_OF_VIEW_DEGREES, MAXIMUM_PITCH_DEGREES, MAXIMUM_ZOOM_LEVEL,
        MINIMUM_ZOOM_LEVEL, MapCamera, ViewportPixels,
    };
    use geospatial_types::GeographicCoordinate;

    fn coordinate(latitude: f64, longitude: f64) -> GeographicCoordinate {
        GeographicCoordinate::from_degrees(latitude, longitude).unwrap()
    }

    fn viewport() -> ViewportPixels {
        ViewportPixels::new(1024.0, 768.0).unwrap()
    }

    /// Headroom for the focal point's world point landing back under its
    /// pixel after a delta; matrix round trips at zoom 14 permit
    /// sub-hundredth-pixel drift (the same class the camera tests carry).
    const FOCAL_HOLD_TOLERANCE_PIXELS: f64 = 1e-2;
    /// Headroom for the zoom delta arriving exactly (clamps pass the level
    /// through `MapCamera::new` unmodified in range).
    const ZOOM_LEVEL_TOLERANCE: f64 = 1e-9;
    /// Headroom for bearing wrap arithmetic on the rotate answers.
    const BEARING_DEGREES_TOLERANCE: f64 = 1e-9;

    /// The sample camera: a recorded neighborhood at a typical map zoom,
    /// pitched and turned so the corrections run under a real perspective,
    /// not only top-down.
    fn fixture_camera() -> MapCamera {
        MapCamera::new(coordinate(38.5766, -121.4944), 14.0, 25.0, 30.0)
    }

    /// The focal pixel of the shared tests: a quarter of the viewport in
    /// from the top left, visibly off the center so the hold correction has
    /// work to do.
    fn focal_point(viewport: &ViewportPixels) -> [f64; 2] {
        [viewport.width_pixels * 0.25, viewport.height_pixels * 0.25]
    }

    fn assert_points_agree(first: [f64; 2], second: [f64; 2], tolerance_pixels: f64) {
        assert!(
            (first[0] - second[0]).hypot(first[1] - second[1]) <= tolerance_pixels,
            "screen points must agree within {tolerance_pixels} px: {first:?} vs {second:?}",
        );
    }

    #[test]
    fn pan_tracks_the_pointers_ground_point_to_the_viewport_center() {
        let camera = fixture_camera();
        let viewport = viewport();
        let delta_pixels = [64.0, -48.0];
        let viewport_center = viewport_center_pixels(&viewport);
        let pointer_start = [
            viewport_center[0] - delta_pixels[0],
            viewport_center[1] - delta_pixels[1],
        ];
        let pointer_ground = camera
            .screen_to_ground_coordinate(pointer_start, &viewport, DEFAULT_FIELD_OF_VIEW_DEGREES)
            .unwrap();
        let panned =
            camera.pan_by_screen_pixels(delta_pixels, &viewport, DEFAULT_FIELD_OF_VIEW_DEGREES);
        // The pointer traveled from `pointer_start` to the center; the ground
        // point under its start must now sit at the center.
        assert_points_agree(
            panned
                .project_to_screen(&pointer_ground, &viewport, DEFAULT_FIELD_OF_VIEW_DEGREES)
                .unwrap(),
            viewport_center,
            FOCAL_HOLD_TOLERANCE_PIXELS,
        );
        // Zoom, bearing and pitch ride through unchanged.
        assert_eq!(panned.zoom_level, camera.zoom_level);
        assert_eq!(panned.bearing_degrees, camera.bearing_degrees);
        assert_eq!(panned.pitch_degrees, camera.pitch_degrees);
    }

    #[test]
    fn pan_above_the_horizon_leaves_the_camera_unchanged() {
        let camera = MapCamera::new(coordinate(30.0, 20.0), 10.0, 0.0, MAXIMUM_PITCH_DEGREES);
        let viewport = viewport();
        // The top of the viewport looks past the horizon at maximum pitch, so
        // a pointer delta reaching it unprojects no ground.
        assert!(
            camera
                .screen_to_ground_coordinate([0.0, 0.0], &viewport, DEFAULT_FIELD_OF_VIEW_DEGREES)
                .is_none()
        );
        let panned = camera.pan_by_screen_pixels(
            [viewport.width_pixels, viewport.height_pixels],
            &viewport,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
        );
        assert_eq!(panned, camera);
    }

    #[test]
    fn focal_zoom_keeps_the_world_point_under_the_focal_pixel() {
        let camera = fixture_camera();
        let viewport = viewport();
        let focal = focal_point(&viewport);
        let held_ground = camera
            .screen_to_ground_coordinate(focal, &viewport, DEFAULT_FIELD_OF_VIEW_DEGREES)
            .unwrap();
        let zoomed = camera.zoom_by_levels_around_screen_point(
            1.0,
            focal,
            &viewport,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
        );
        assert!((zoomed.zoom_level - (camera.zoom_level + 1.0)).abs() < ZOOM_LEVEL_TOLERANCE);
        assert_points_agree(
            zoomed
                .project_to_screen(&held_ground, &viewport, DEFAULT_FIELD_OF_VIEW_DEGREES)
                .unwrap(),
            focal,
            FOCAL_HOLD_TOLERANCE_PIXELS,
        );
        // The center moved toward the held point's ground position (a
        // zoom-in pans the anchor toward the point being held) but zoom,
        // bearing and pitch carry through.
        assert_eq!(zoomed.bearing_degrees, camera.bearing_degrees);
        assert_eq!(zoomed.pitch_degrees, camera.pitch_degrees);
        let camera_world = crate::projection::coordinate_to_normalized_world(&camera.center);
        let zoomed_world = crate::projection::coordinate_to_normalized_world(&zoomed.center);
        // The fixture focal sits left of and above the center (northwest on
        // the ground at bearing 25°), so the anchor must move the same way.
        assert!(zoomed_world[0] < camera_world[0] || zoomed_world[1] < camera_world[1]);
    }

    #[test]
    fn focal_zoom_clamps_to_the_camera_zoom_range() {
        let camera = MapCamera::new(coordinate(38.5766, -121.4944), 14.0, 0.0, 0.0);
        let viewport = viewport();
        // Zoom-in clamp against an off-center focal: top-down, so the whole
        // held-point correction is affine and stays inside the map square.
        let held_ground = camera
            .screen_to_ground_coordinate(
                focal_point(&viewport),
                &viewport,
                DEFAULT_FIELD_OF_VIEW_DEGREES,
            )
            .unwrap();
        let zoomed_in = camera.zoom_by_levels_around_screen_point(
            100.0,
            focal_point(&viewport),
            &viewport,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
        );
        assert_eq!(zoomed_in.zoom_level, MAXIMUM_ZOOM_LEVEL);
        assert_points_agree(
            zoomed_in
                .project_to_screen(&held_ground, &viewport, DEFAULT_FIELD_OF_VIEW_DEGREES)
                .unwrap(),
            focal_point(&viewport),
            FOCAL_HOLD_TOLERANCE_PIXELS,
        );
        // Zoom-out clamp about the viewport center: at minimum zoom the view
        // footprint spans whole world copies, where the held-point
        // correction is out of square — a center focal keeps the test honest
        // and asserts the clamp itself.
        let zoomed_out = camera.zoom_by_levels_around_screen_point(
            -100.0,
            viewport_center_pixels(&viewport),
            &viewport,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
        );
        assert_eq!(zoomed_out.zoom_level, MINIMUM_ZOOM_LEVEL);
        assert_eq!(zoomed_out.center, camera.center);
        assert_eq!(zoomed_out.bearing_degrees, camera.bearing_degrees);
        assert_eq!(zoomed_out.pitch_degrees, camera.pitch_degrees);
    }

    #[test]
    fn focal_rotation_keeps_the_world_point_under_the_focal_pixel() {
        let camera = fixture_camera();
        let viewport = viewport();
        let focal = focal_point(&viewport);
        let held_ground = camera
            .screen_to_ground_coordinate(focal, &viewport, DEFAULT_FIELD_OF_VIEW_DEGREES)
            .unwrap();
        let rotated = camera.rotate_by_degrees_around_screen_point(
            45.0,
            focal,
            &viewport,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
        );
        assert!(
            (rotated.bearing_degrees - (camera.bearing_degrees + 45.0)).abs()
                < BEARING_DEGREES_TOLERANCE
        );
        assert_points_agree(
            rotated
                .project_to_screen(&held_ground, &viewport, DEFAULT_FIELD_OF_VIEW_DEGREES)
                .unwrap(),
            focal,
            FOCAL_HOLD_TOLERANCE_PIXELS,
        );
        assert_eq!(rotated.zoom_level, camera.zoom_level);
        assert_eq!(rotated.pitch_degrees, camera.pitch_degrees);
    }

    #[test]
    fn rotation_about_the_viewport_center_moves_only_the_bearing() {
        let camera = fixture_camera();
        let viewport = viewport();
        let rotated = camera.rotate_by_degrees_around_screen_point(
            450.0,
            viewport_center_pixels(&viewport),
            &viewport,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
        );
        // The 90° overshoot wraps through the camera constructor.
        assert!(
            (rotated.bearing_degrees - (camera.bearing_degrees + 90.0 + 360.0).rem_euclid(360.0))
                .abs()
                < BEARING_DEGREES_TOLERANCE
        );
        assert_eq!(rotated.center, camera.center);
        assert_eq!(rotated.zoom_level, camera.zoom_level);
        assert_eq!(rotated.pitch_degrees, camera.pitch_degrees);
    }
}
