//! The shared camera state and contract-member math behind the browser
//! `MapCamera` and `MapView` bindings.
//!
//! The state store, the neutral pose, the fit, the interpolation and the
//! geographic-bounds readback are host-testable so `cargo test` exercises the
//! same rules the wasm exports carry; the `wasm_bindgen` wrappers over them
//! live in [`crate::exports`] and [`crate::canvas`].

use std::cell::Cell;

use geospatial_types::{BoundingBox, GeographicCoordinate};
use rustic_maps_rendering::{
    EdgeInsetsPixels, MapCamera, ViewportPixels, fit_bounds, interpolate_camera,
};

/// Latitude of the view's camera before any host intent, in decimal degrees:
/// the same neutral origin the native surface starts from.
pub(crate) const DEFAULT_CAMERA_LATITUDE_DEGREES: f64 = 0.0;

/// Longitude of the view's camera before any host intent, in decimal degrees.
pub(crate) const DEFAULT_CAMERA_LONGITUDE_DEGREES: f64 = 0.0;

/// Zoom level of the view's camera before any host intent: the whole-world
/// framing the native surface starts at.
pub(crate) const DEFAULT_CAMERA_ZOOM_LEVEL: f64 = 2.0;

/// Least number of viewport corners that must unproject for a usable
/// geographic-bounds box; below it the ground-scale fallback answers instead.
const MINIMUM_UNPROJECTED_BOUNDS_CORNER_COUNT: usize = 3;

/// Meters covered by one degree of latitude on the WGS 84 mean sphere; the
/// steep-pitch bounds fallback converts the camera's ground scale with it.
const METERS_PER_DEGREE_LATITUDE: f64 = 111_320.0;

/// The neutral camera pose a freshly constructed browser view holds: the
/// native neutral pose (zero latitude and longitude, zoom 2, north-up and
/// top-down).
pub(crate) fn neutral_camera() -> MapCamera {
    let center = GeographicCoordinate {
        latitude: DEFAULT_CAMERA_LATITUDE_DEGREES,
        longitude: DEFAULT_CAMERA_LONGITUDE_DEGREES,
    };
    MapCamera::new(center, DEFAULT_CAMERA_ZOOM_LEVEL, 0.0, 0.0)
}

/// The single camera a browser view holds, mirroring the native surface's
/// `camera`/`set_camera` pair. The pose is an immutable value type, so the
/// store swaps whole poses and never mutates one in place.
pub(crate) struct CameraStore {
    /// The pose every following frame and readback reports.
    camera: Cell<MapCamera>,
}

impl CameraStore {
    /// A store holding the neutral pose.
    pub(crate) fn new() -> Self {
        Self {
            camera: Cell::new(neutral_camera()),
        }
    }

    /// The pose the store holds.
    pub(crate) fn camera(&self) -> MapCamera {
        self.camera.get()
    }

    /// Adopts `camera` as the stored pose. The pose is already validated by
    /// its constructor, so adopting it cannot fail.
    pub(crate) fn set_camera(&self, camera: MapCamera) {
        self.camera.set(camera);
    }
}

impl Default for CameraStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Eases from `start_camera` toward `target_camera` over `fraction`,
/// delegating to the SDK's one interpolation curve.
///
/// # Errors
/// Returns a description when `fraction` is not finite; the core curve would
/// otherwise produce a non-finite pose instead of a refused input.
pub(crate) fn interpolate_cameras(
    start_camera: &MapCamera,
    target_camera: &MapCamera,
    fraction: f64,
) -> Result<MapCamera, String> {
    if !fraction.is_finite() {
        return Err("the interpolation fraction must be finite".to_string());
    }
    Ok(interpolate_camera(start_camera, target_camera, fraction))
}

/// The camera that frames `bounding_box` inside `viewport`'s inset region,
/// mirroring the contract's `fit_bounds` constructor.
///
/// # Errors
/// Returns a description of the unusable input when the viewport, insets,
/// maximum zoom or box are refused by the core fit.
pub(crate) fn fit_camera(
    bounding_box: &BoundingBox,
    viewport: &ViewportPixels,
    edge_insets: EdgeInsetsPixels,
    maximum_zoom_level: f64,
) -> Result<MapCamera, String> {
    fit_bounds(bounding_box, viewport, edge_insets, maximum_zoom_level)
}

/// The geographic bounds of `viewport` at `camera`'s pose.
///
/// The four viewport corners are unprojected first; a steep pitch can put the
/// far corner below the horizon, so when fewer than
/// [`MINIMUM_UNPROJECTED_BOUNDS_CORNER_COUNT`] corners reach the ground the
/// camera's own ground scale bounds a box around its center instead. Answers
/// `None` only when even that scale is unavailable.
pub(crate) fn camera_geographic_bounds(
    camera: &MapCamera,
    viewport: &ViewportPixels,
    field_of_view_degrees: f64,
) -> Option<BoundingBox> {
    let corner_screen_points = [
        [0.0, 0.0],
        [viewport.width_pixels, 0.0],
        [viewport.width_pixels, viewport.height_pixels],
        [0.0, viewport.height_pixels],
    ];
    let corner_coordinates: Vec<GeographicCoordinate> = corner_screen_points
        .iter()
        .filter_map(|screen_point| {
            camera.screen_to_ground_coordinate(*screen_point, viewport, field_of_view_degrees)
        })
        .collect();
    if corner_coordinates.len() >= MINIMUM_UNPROJECTED_BOUNDS_CORNER_COUNT {
        let mut south_west_latitude = f64::INFINITY;
        let mut south_west_longitude = f64::INFINITY;
        let mut north_east_latitude = f64::NEG_INFINITY;
        let mut north_east_longitude = f64::NEG_INFINITY;
        for corner_coordinate in &corner_coordinates {
            south_west_latitude = south_west_latitude.min(corner_coordinate.latitude);
            south_west_longitude = south_west_longitude.min(corner_coordinate.longitude);
            north_east_latitude = north_east_latitude.max(corner_coordinate.latitude);
            north_east_longitude = north_east_longitude.max(corner_coordinate.longitude);
        }
        return Some(BoundingBox {
            south_west: GeographicCoordinate {
                latitude: south_west_latitude,
                longitude: south_west_longitude,
            },
            north_east: GeographicCoordinate {
                latitude: north_east_latitude,
                longitude: north_east_longitude,
            },
        });
    }
    // The rendering camera always answers its flat-centered ground scale, so
    // the fallback is available whenever at least three corners missed the
    // ground.
    let meters_per_pixel = camera.ground_meters_per_screen_pixel();
    let half_height_meters = meters_per_pixel * viewport.height_pixels / 2.0;
    let half_width_meters = meters_per_pixel * viewport.width_pixels / 2.0;
    let latitude_delta_degrees = half_height_meters / METERS_PER_DEGREE_LATITUDE;
    let center = camera.center;
    let longitude_scale = METERS_PER_DEGREE_LATITUDE * center.latitude.to_radians().cos();
    let longitude_delta_degrees = if longitude_scale == 0.0 {
        latitude_delta_degrees
    } else {
        half_width_meters / longitude_scale
    };
    Some(BoundingBox {
        south_west: GeographicCoordinate {
            latitude: center.latitude - latitude_delta_degrees,
            longitude: center.longitude - longitude_delta_degrees,
        },
        north_east: GeographicCoordinate {
            latitude: center.latitude + latitude_delta_degrees,
            longitude: center.longitude + longitude_delta_degrees,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::{
        CameraStore, DEFAULT_CAMERA_LATITUDE_DEGREES, DEFAULT_CAMERA_LONGITUDE_DEGREES,
        DEFAULT_CAMERA_ZOOM_LEVEL, camera_geographic_bounds, fit_camera, interpolate_cameras,
        neutral_camera,
    };
    use geospatial_types::GeographicCoordinate;
    use rustic_maps_rendering::{EdgeInsetsPixels, MapCamera, ViewportPixels};

    /// A coordinate at decimal degrees.
    fn coordinate(latitude: f64, longitude: f64) -> GeographicCoordinate {
        GeographicCoordinate {
            latitude,
            longitude,
        }
    }

    /// A viewport at 800 by 600 pixels.
    fn viewport() -> ViewportPixels {
        ViewportPixels::new(800.0, 600.0).expect("a canonical viewport")
    }

    #[test]
    fn interpolation_answers_both_endpoints_verbatim() {
        let start = MapCamera::new(coordinate(38.0, -121.0), 10.0, 10.0, 20.0);
        let target = MapCamera::new(coordinate(39.0, -120.0), 14.0, 30.0, 40.0);
        assert_eq!(
            interpolate_cameras(&start, &target, 0.0).expect("zero is a finite fraction"),
            start
        );
        assert_eq!(
            interpolate_cameras(&start, &target, 1.0).expect("one is a finite fraction"),
            target
        );
        // Out-of-range fractions clamp to the exact endpoints like the core.
        assert_eq!(
            interpolate_cameras(&start, &target, -1.0).expect("negative finite clamps"),
            start
        );
        assert_eq!(
            interpolate_cameras(&start, &target, 2.0).expect("above one clamps"),
            target
        );
    }

    #[test]
    fn interpolation_at_the_midpoint_eases_each_component() {
        let start = MapCamera::new(coordinate(38.0, -121.0), 10.0, 10.0, 20.0);
        let target = MapCamera::new(coordinate(39.0, -120.0), 14.0, 30.0, 40.0);
        let midpoint =
            interpolate_cameras(&start, &target, 0.5).expect("one half is a finite fraction");
        // Smoothstep(0.5) is exactly one half, so zoom, bearing and pitch
        // interpolate linearly and the longitude travels the shortest path.
        assert_eq!(midpoint.zoom_level, 12.0);
        assert_eq!(midpoint.bearing_degrees, 20.0);
        assert_eq!(midpoint.pitch_degrees, 30.0);
        assert!((midpoint.center.longitude - -120.5).abs() < 1e-9);
        assert!(midpoint.center.latitude > 38.0 && midpoint.center.latitude < 39.0);
    }

    #[test]
    fn interpolation_refuses_a_non_finite_fraction() {
        let start = MapCamera::new(coordinate(0.0, 0.0), 2.0, 0.0, 0.0);
        let target = MapCamera::new(coordinate(1.0, 1.0), 3.0, 0.0, 0.0);
        assert!(interpolate_cameras(&start, &target, f64::NAN).is_err());
        assert!(interpolate_cameras(&start, &target, f64::INFINITY).is_err());
    }

    #[test]
    fn the_neutral_pose_matches_the_native_neutral_camera() {
        let expected = MapCamera::new(
            coordinate(
                DEFAULT_CAMERA_LATITUDE_DEGREES,
                DEFAULT_CAMERA_LONGITUDE_DEGREES,
            ),
            DEFAULT_CAMERA_ZOOM_LEVEL,
            0.0,
            0.0,
        );
        assert_eq!(neutral_camera(), expected);
        assert_eq!(CameraStore::new().camera(), expected);
    }

    #[test]
    fn the_camera_store_round_trips_an_adopted_pose() {
        let store = CameraStore::new();
        // The constructor wraps the bearing, and the store round-trips the
        // wrapped pose exactly.
        let adopted = MapCamera::new(coordinate(40.0, -120.0), 12.5, 370.0, 35.0);
        store.set_camera(adopted);
        assert_eq!(store.camera(), adopted);
        assert_eq!(store.camera().bearing_degrees, 10.0);
    }

    #[test]
    fn fitting_a_box_adopts_a_camera_that_frames_it() {
        let bounding_box = geospatial_types::BoundingBox {
            south_west: coordinate(38.0, -122.0),
            north_east: coordinate(39.0, -120.0),
        };
        let fitted = fit_camera(
            &bounding_box,
            &viewport(),
            EdgeInsetsPixels::symmetric(40.0, 40.0),
            17.0,
        )
        .expect("a canonical box inside the inset area fits");
        // The fitted camera is top-down and north-up, and its center sits
        // inside the box.
        assert_eq!(fitted.pitch_degrees, 0.0);
        assert_eq!(fitted.bearing_degrees, 0.0);
        assert!(fitted.center.latitude >= 38.0 && fitted.center.latitude <= 39.0);
        assert!(fitted.center.longitude >= -122.0 && fitted.center.longitude <= -120.0);
        // Adopting the fit makes it the stored pose.
        let store = CameraStore::new();
        store.set_camera(fitted);
        assert_eq!(store.camera(), fitted);
    }

    #[test]
    fn fitting_refuses_an_unusable_box_or_insets() {
        let reversed_box = geospatial_types::BoundingBox {
            south_west: coordinate(39.0, -120.0),
            north_east: coordinate(38.0, -122.0),
        };
        assert!(
            fit_camera(
                &reversed_box,
                &viewport(),
                EdgeInsetsPixels::symmetric(40.0, 40.0),
                17.0
            )
            .is_err()
        );
        let canonical_box = geospatial_types::BoundingBox {
            south_west: coordinate(38.0, -122.0),
            north_east: coordinate(39.0, -120.0),
        };
        assert!(
            fit_camera(
                &canonical_box,
                &viewport(),
                EdgeInsetsPixels::symmetric(-1.0, 0.0),
                17.0
            )
            .is_err()
        );
        assert!(
            fit_camera(
                &canonical_box,
                &viewport(),
                EdgeInsetsPixels::symmetric(40.0, 40.0),
                f64::NAN
            )
            .is_err()
        );
    }

    #[test]
    fn geographic_bounds_answer_a_sane_box_at_a_normal_pose() {
        let camera = MapCamera::new(coordinate(38.5766, -121.4944), 14.0, 0.0, 0.0);
        let bounds = camera_geographic_bounds(&camera, &viewport(), 60.0)
            .expect("a top-down camera answers its four corners");
        assert!(bounds.south_west.latitude < 38.5766);
        assert!(bounds.north_east.latitude > 38.5766);
        assert!(bounds.south_west.longitude < -121.4944);
        assert!(bounds.north_east.longitude > -121.4944);
        assert!(bounds.north_east.latitude > bounds.south_west.latitude);
        assert!(bounds.north_east.longitude > bounds.south_west.longitude);
    }

    #[test]
    fn geographic_bounds_still_answer_at_a_steep_pitch() {
        let camera = MapCamera::new(coordinate(38.5766, -121.4944), 14.0, 0.0, 70.0);
        let bounds = camera_geographic_bounds(&camera, &viewport(), 60.0)
            .expect("the ground-scale fallback bounds a steep camera");
        assert!(bounds.north_east.latitude > bounds.south_west.latitude);
        assert!(bounds.north_east.longitude > bounds.south_west.longitude);
        assert!(bounds.south_west.latitude.is_finite());
        assert!(bounds.north_east.longitude.is_finite());
    }
}
