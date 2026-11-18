//! The UniFFI-facing anchored camera, wrapping `rustic-maps-rendering`'s camera; the
//! validation the core leaves to callers happens on every binding call, so
//! the binding surface can only ever receive validated camera state.

use std::sync::Arc;

use rustic_maps_rendering::CameraInteraction;
use rustic_maps_rendering::MapCamera as NativeMapCamera;

use crate::error::SdkError;
use crate::projection::{validated_bounding_box, validated_coordinate, validated_viewport_pixels};
use crate::records::{
    BoundingBox, EdgeInsets, GeographicCoordinate, ScreenPoint, TileKey, ViewportSize,
};

/// Anchored map camera (geographic center, zoom, bearing, pitch) mirroring the
/// native [`NativeMapCamera`]. The ground point under the center always sits
/// at the viewport center.
#[derive(Debug, Clone, Copy)]
pub struct MapCamera {
    native_camera: NativeMapCamera,
}

impl MapCamera {
    /// Builds a camera, rejecting an invalid center: latitude/longitude
    /// outside their canonical ranges fail with
    /// [`SdkError::InvalidCoordinateInput`]. Zoom and pitch are clamped and
    /// the bearing wraps into `[0, 360)`, exactly like the native
    /// constructor.
    pub fn new(
        center: GeographicCoordinate,
        zoom_level: f64,
        bearing_degrees: f64,
        pitch_degrees: f64,
    ) -> Result<Self, SdkError> {
        let center = validated_coordinate(center)?;
        Ok(Self {
            native_camera: NativeMapCamera::new(center, zoom_level, bearing_degrees, pitch_degrees),
        })
    }

    /// Builds the camera that frames a bounding box inside the viewport's
    /// inset region: top-down and north-up by construction (pitch and bearing
    /// zero), zoomed to the largest level keeping the whole box inside the
    /// inset area and clamped to the caller's `maximum_zoom_level`; the center
    /// is anchored on the inset region's center. Delegates to the native
    /// `fit_bounds`; coordinate/viewport validation failures and the native
    /// fit failures (insets, maximum zoom, box area) all surface under
    /// [`SdkError::InvalidCoordinateInput`] with the verbatim detail.
    pub fn fit_bounds(
        bounding_box: BoundingBox,
        viewport: ViewportSize,
        edge_insets: EdgeInsets,
        maximum_zoom_level: f64,
    ) -> Result<Self, SdkError> {
        let bounding_box = validated_bounding_box(bounding_box)?;
        let viewport = validated_viewport_pixels(viewport)?;
        rustic_maps_rendering::fit_bounds(
            &bounding_box,
            &viewport,
            edge_insets.to_native_edge_insets(),
            maximum_zoom_level,
        )
        .map(|native_camera| Self { native_camera })
        .map_err(|message| SdkError::InvalidCoordinateInput { detail: message })
    }

    /// Eases from `start_camera` toward `target_camera` over `fraction`.
    ///
    /// The SDK's one camera-interpolation curve: the fraction is clamped to
    /// `[0, 1]` and eased with smoothstep, the center travels the shortest
    /// longitude path in Mercator space, zoom and pitch interpolate linearly,
    /// and the bearing takes the shortest arc. Fractions at or below zero
    /// answer the start camera verbatim and fractions at or above one answer
    /// the target verbatim, so an animation's endpoints are exact.
    ///
    /// Delegates to the renderer's `interpolate_camera`, so a platform shell
    /// that eases its own frame pump receives the same pose the renderer
    /// would.
    ///
    /// # Parameters
    /// - `start_camera` — the pose the interpolation starts from.
    /// - `target_camera` — the pose the interpolation heads toward.
    /// - `fraction` — animation progress, clamped into `[0, 1]`.
    ///
    /// # Errors
    /// Throws `InvalidCoordinateInput` when `fraction` is not finite.
    #[allow(clippy::needless_pass_by_value)]
    pub fn interpolate(
        start_camera: Arc<Self>,
        target_camera: Arc<Self>,
        fraction: f64,
    ) -> Result<Self, SdkError> {
        if !fraction.is_finite() {
            return Err(SdkError::InvalidCoordinateInput {
                detail: "the camera interpolation fraction must be finite".to_string(),
            });
        }
        Ok(Self {
            native_camera: rustic_maps_rendering::interpolate_camera(
                &start_camera.native_camera,
                &target_camera.native_camera,
                fraction,
            ),
        })
    }

    /// Projects a coordinate to viewport pixels (origin top-left, y down).
    /// Returns `None` when the point lies behind the camera.
    pub fn project_to_screen(
        &self,
        coordinate: GeographicCoordinate,
        viewport: ViewportSize,
        field_of_view_degrees: f64,
    ) -> Result<Option<ScreenPoint>, SdkError> {
        let coordinate = validated_coordinate(coordinate)?;
        let viewport = validated_viewport_pixels(viewport)?;
        Ok(self
            .native_camera
            .project_to_screen(&coordinate, &viewport, field_of_view_degrees)
            .map(ScreenPoint::from_screen_pixels))
    }

    /// Unprojects a viewport pixel back onto the ground plane. Returns `None`
    /// when the ray misses the ground (typically below the horizon at pitch).
    pub fn screen_to_ground_coordinate(
        &self,
        screen_point: ScreenPoint,
        viewport: ViewportSize,
        field_of_view_degrees: f64,
    ) -> Result<Option<GeographicCoordinate>, SdkError> {
        let viewport = validated_viewport_pixels(viewport)?;
        Ok(self
            .native_camera
            .screen_to_ground_coordinate(
                [screen_point.x, screen_point.y],
                &viewport,
                field_of_view_degrees,
            )
            .map(|native_coordinate| {
                GeographicCoordinate::from_native_coordinate(&native_coordinate)
            }))
    }

    /// Pans by a pointer travel of `delta_x_pixels` x `delta_y_pixels`
    /// (screen pixels, x right / y down): the ground point the pointer just
    /// moved over becomes the new camera center, so map content tracks the
    /// pointer exactly. Answers the camera unchanged when the delta's ground
    /// ray misses the plane (above the horizon at steep pitch). Delegates to
    /// the native `CameraInteraction` seam; the viewport is validated here
    /// like everywhere else.
    pub fn pan_by_screen_pixels(
        &self,
        delta_x_pixels: f64,
        delta_y_pixels: f64,
        viewport: ViewportSize,
        field_of_view_degrees: f64,
    ) -> Result<Arc<Self>, SdkError> {
        let viewport = validated_viewport_pixels(viewport)?;
        Ok(Arc::new(Self {
            native_camera: self.native_camera.pan_by_screen_pixels(
                [delta_x_pixels, delta_y_pixels],
                &viewport,
                field_of_view_degrees,
            ),
        }))
    }

    /// Zooms by `delta_zoom_levels` (levels; positive zooms in) keeping the
    /// world point under `focal_point` fixed under it, clamped through the
    /// native camera's own zoom range. Answers the zoom without the focal
    /// correction when the focal ray misses the ground plane.
    pub fn zoom_by_levels_around_screen_point(
        &self,
        delta_zoom_levels: f64,
        focal_point: ScreenPoint,
        viewport: ViewportSize,
        field_of_view_degrees: f64,
    ) -> Result<Arc<Self>, SdkError> {
        let viewport = validated_viewport_pixels(viewport)?;
        Ok(Arc::new(Self {
            native_camera: self.native_camera.zoom_by_levels_around_screen_point(
                delta_zoom_levels,
                [focal_point.x, focal_point.y],
                &viewport,
                field_of_view_degrees,
            ),
        }))
    }

    /// Rotates the bearing by `delta_bearing_degrees` (degrees clockwise,
    /// wrapping into `[0, 360)`) keeping the world point under `focal_point`
    /// fixed under it. Answers the rotation without the focal correction
    /// when the focal ray misses the ground plane.
    pub fn rotate_by_degrees_around_screen_point(
        &self,
        delta_bearing_degrees: f64,
        focal_point: ScreenPoint,
        viewport: ViewportSize,
        field_of_view_degrees: f64,
    ) -> Result<Arc<Self>, SdkError> {
        let viewport = validated_viewport_pixels(viewport)?;
        Ok(Arc::new(Self {
            native_camera: self.native_camera.rotate_by_degrees_around_screen_point(
                delta_bearing_degrees,
                [focal_point.x, focal_point.y],
                &viewport,
                field_of_view_degrees,
            ),
        }))
    }

    /// Ground meters represented by one screen pixel at the camera's center
    /// (the scale bars' helper). The viewport and field-of-view
    /// arguments mirror the projection methods' shape — call sites pass the
    /// same camera context — and the viewport is validated here like
    /// everywhere else; the honest flat-centered approximation itself is
    /// independent of both (see the contract's member documentation).
    pub fn ground_meters_per_screen_pixel(
        &self,
        viewport: ViewportSize,
        field_of_view_degrees: f64,
    ) -> Result<f64, SdkError> {
        validated_viewport_pixels(viewport)?;
        let _ = field_of_view_degrees;
        Ok(self.native_camera.ground_meters_per_screen_pixel())
    }

    /// Every tile at `tile_zoom_level` whose area intersects the visible
    /// ground, sorted row-major from the north, mirroring the native
    /// `visible_tile_keys`.
    pub fn visible_tile_keys(
        &self,
        viewport: ViewportSize,
        field_of_view_degrees: f64,
        tile_zoom_level: u8,
    ) -> Result<Vec<TileKey>, SdkError> {
        let viewport = validated_viewport_pixels(viewport)?;
        Ok(self
            .native_camera
            .visible_tile_keys(&viewport, field_of_view_degrees, tile_zoom_level)
            .iter()
            .map(|tile_key| TileKey::from_native_tile_key(*tile_key))
            .collect())
    }

    /// Nearest whole zoom level a tile fetch should use for this camera,
    /// mirroring the native `tile_zoom_level`.
    pub fn tile_zoom_level(&self) -> u8 {
        self.native_camera.tile_zoom_level()
    }

    /// The camera's geographic center, verbatim as the camera was built.
    pub fn center(&self) -> GeographicCoordinate {
        GeographicCoordinate::from_native_coordinate(&self.native_camera.center)
    }

    /// The camera's zoom level, verbatim as the camera was built (clamped
    /// values included: `fit_bounds`' readback path reads the actually
    /// fitting level here).
    pub fn zoom_level(&self) -> f64 {
        self.native_camera.zoom_level
    }

    /// The camera's bearing in degrees within `[0, 360)`.
    pub fn bearing_degrees(&self) -> f64 {
        self.native_camera.bearing_degrees
    }

    /// The camera's pitch in degrees.
    pub fn pitch_degrees(&self) -> f64 {
        self.native_camera.pitch_degrees
    }

    /// Borrows the wrapped native camera for the surface seam: the surface
    /// view's `draw` passes the validated pose to the rustic-maps-rendering
    /// renderer, which assembles its quads from the native camera directly.
    /// The wrapper stays the validation boundary for every binding call.
    /// Native-only with the surface view that consumes it.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn native_camera(&self) -> &NativeMapCamera {
        &self.native_camera
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{MapCamera, ViewportSize};
    use crate::SDK_COMPILED_VERSION;
    use crate::error::SdkError;
    use crate::get_sdk_compiled_version;
    use crate::records::{BoundingBox, EdgeInsets, GeographicCoordinate, ScreenPoint, TileKey};

    /// Sacramento's latitude in decimal degrees: the California State
    /// Capitol / Capitol Park, downtown Sacramento.
    const SACRAMENTO_LATITUDE: f64 = 38.5766;
    /// Sacramento's longitude in decimal degrees.
    const SACRAMENTO_LONGITUDE: f64 = -121.4944;
    /// Viewport width the camera tests use, in pixels.
    const VIEWPORT_WIDTH_PIXELS: f64 = 1024.0;
    /// Viewport height the camera tests use, in pixels.
    const VIEWPORT_HEIGHT_PIXELS: f64 = 768.0;
    /// Tolerance for pixel positions near the viewport center.
    const CENTER_PIXEL_TOLERANCE: f64 = 1e-2;
    /// Tolerance for the screen-to-ground round trip, in degrees.
    const COORDINATE_TOLERANCE_DEGREES: f64 = 1e-6;
    /// Known tile column of the Sacramento anchor at zoom 14.
    const SACRAMENTO_TILE_X: u32 = 2662;
    /// Known tile row of the Sacramento anchor at zoom 14.
    const SACRAMENTO_TILE_Y: u32 = 6286;
    /// Zoom level of the known Sacramento tile address.
    const SACRAMENTO_TILE_ZOOM_LEVEL: u8 = 14;
    /// Rounding headroom for fitted corners hitting an inset edge, in pixels,
    /// mirroring the native `fit_bounds` camera tests' tolerance.
    const FIT_BOUNDS_CORNER_TOLERANCE_PIXELS: f64 = 0.5;
    /// Horizontal inset the fit-bounds tests keep clear on both sides, in
    /// pixels.
    const FIT_INSET_X_PIXELS: f64 = 100.0;
    /// Vertical inset the fit-bounds tests keep clear on both sides, in
    /// pixels.
    const FIT_INSET_Y_PIXELS: f64 = 50.0;
    /// Zoom clamp the fit-bounds rejection tests use, small enough to bind.
    const FIT_MAXIMUM_ZOOM_LEVEL_CLAMPED: f64 = 3.0;
    /// Inset the asymmetric-inset center-anchoring test keeps clear from the
    /// left edge, in pixels.
    const ASYMMETRIC_LEFT_INSET_PIXELS: f64 = 300.0;
    /// Inset the asymmetric-inset center-anchoring test keeps clear from the
    /// three other edges, in pixels.
    const ASYMMETRIC_EDGE_INSET_PIXELS: f64 = 50.0;
    /// Verbatim detail of the native fit rejects for unusable insets.
    const UNSAFE_INSETS_DETAIL: &str = "insets must be finite and non-negative";
    /// Verbatim detail fragment of the core box reject for inverted corners.
    const INVERTED_CORNERS_DETAIL: &str = "bounding box corners are inverted";
    /// Verbatim detail fragment of the native fit reject for a box spanning
    /// no positive world area.
    const NO_POSITIVE_AREA_DETAIL_FRAGMENT: &str = "must span a positive, finite world area";
    /// Verbatim detail of the native fit reject for a non-finite zoom clamp.
    const NON_FINITE_MAXIMUM_ZOOM_DETAIL: &str = "the maximum zoom level must be finite";
    /// Verbatim detail the interpolation rejects a non-finite fraction with.
    const NON_FINITE_INTERPOLATION_FRACTION_DETAIL: &str =
        "the camera interpolation fraction must be finite";

    fn coordinate(latitude: f64, longitude: f64) -> GeographicCoordinate {
        GeographicCoordinate {
            latitude,
            longitude,
        }
    }

    fn camera_center() -> GeographicCoordinate {
        coordinate(47.0, 8.5)
    }

    fn viewport() -> ViewportSize {
        ViewportSize {
            width_pixels: VIEWPORT_WIDTH_PIXELS,
            height_pixels: VIEWPORT_HEIGHT_PIXELS,
        }
    }

    fn bounded_box(
        south_west_latitude: f64,
        south_west_longitude: f64,
        north_east_latitude: f64,
        north_east_longitude: f64,
    ) -> BoundingBox {
        BoundingBox {
            south_west: coordinate(south_west_latitude, south_west_longitude),
            north_east: coordinate(north_east_latitude, north_east_longitude),
        }
    }

    fn symmetric_edge_insets() -> EdgeInsets {
        EdgeInsets {
            left: FIT_INSET_X_PIXELS,
            top: FIT_INSET_Y_PIXELS,
            right: FIT_INSET_X_PIXELS,
            bottom: FIT_INSET_Y_PIXELS,
        }
    }

    #[test]
    fn fit_bounds_frames_the_box_inside_the_inset_region() {
        let california_box = bounded_box(32.5, -124.4, 42.0, -114.1);
        let camera = MapCamera::fit_bounds(
            california_box,
            viewport(),
            symmetric_edge_insets(),
            f64::MAX,
        )
        .expect("box, viewport and insets are all valid");
        let insets = symmetric_edge_insets();
        let mut touches_left = false;
        let mut touches_right = false;
        let mut touches_top = false;
        let mut touches_bottom = false;
        for box_corner in [california_box.south_west, california_box.north_east] {
            let corner_screen = camera
                .project_to_screen(box_corner, viewport(), 60.0)
                .expect("viewport is valid")
                .expect("a fitted top-down box corner is always in front of the camera");
            assert!(
                corner_screen.x >= insets.left - FIT_BOUNDS_CORNER_TOLERANCE_PIXELS,
                "corner {box_corner:?} at x {} pierces the left inset",
                corner_screen.x
            );
            assert!(
                corner_screen.x
                    <= VIEWPORT_WIDTH_PIXELS - insets.right + FIT_BOUNDS_CORNER_TOLERANCE_PIXELS,
                "corner {box_corner:?} at x {} pierces the right inset",
                corner_screen.x
            );
            assert!(
                corner_screen.y >= insets.top - FIT_BOUNDS_CORNER_TOLERANCE_PIXELS,
                "corner {box_corner:?} at y {} pierces the top inset",
                corner_screen.y
            );
            assert!(
                corner_screen.y
                    <= VIEWPORT_HEIGHT_PIXELS - insets.bottom + FIT_BOUNDS_CORNER_TOLERANCE_PIXELS,
                "corner {box_corner:?} at y {} pierces the bottom inset",
                corner_screen.y
            );
            touches_left |= corner_screen.x <= insets.left + FIT_BOUNDS_CORNER_TOLERANCE_PIXELS;
            touches_right |= corner_screen.x
                >= VIEWPORT_WIDTH_PIXELS - insets.right - FIT_BOUNDS_CORNER_TOLERANCE_PIXELS;
            touches_top |= corner_screen.y <= insets.top + FIT_BOUNDS_CORNER_TOLERANCE_PIXELS;
            touches_bottom |= corner_screen.y
                >= VIEWPORT_HEIGHT_PIXELS - insets.bottom - FIT_BOUNDS_CORNER_TOLERANCE_PIXELS;
        }
        // The binding axis fits exactly between its insets; the slack axis
        // sits strictly inside.
        assert!((touches_left && touches_right) || (touches_top && touches_bottom));
    }

    #[test]
    fn fit_bounds_anchors_the_box_on_the_asymmetric_inset_center() {
        // Inset far harder on the left: the box center must land at the inset
        // region's center, right of the viewport center.
        let california_box = bounded_box(32.5, -124.4, 42.0, -114.1);
        let edge_insets = EdgeInsets {
            left: ASYMMETRIC_LEFT_INSET_PIXELS,
            top: ASYMMETRIC_EDGE_INSET_PIXELS,
            right: ASYMMETRIC_EDGE_INSET_PIXELS,
            bottom: ASYMMETRIC_EDGE_INSET_PIXELS,
        };
        let camera = MapCamera::fit_bounds(california_box, viewport(), edge_insets, f64::MAX)
            .expect("box, viewport and insets are all valid");
        // A country-scale box spans enough latitude that its mercator-normalized
        // center differs from the arithmetic latitude mean; the anchor guarantee
        // is on the normalized-world center the fit itself uses.
        let south_west_world = crate::coordinate_to_normalized_world(california_box.south_west)
            .expect("box corner is canonical");
        let north_east_world = crate::coordinate_to_normalized_world(california_box.north_east)
            .expect("box corner is canonical");
        let box_center_coordinate = rustic_maps_rendering::normalized_world_to_coordinate([
            (south_west_world.x + north_east_world.x) * 0.5,
            (south_west_world.y + north_east_world.y) * 0.5,
        ]);
        let box_center_screen = camera
            .project_to_screen(
                GeographicCoordinate {
                    latitude: box_center_coordinate.latitude,
                    longitude: box_center_coordinate.longitude,
                },
                viewport(),
                60.0,
            )
            .expect("viewport is valid")
            .expect("the box center renders in view");
        let inset_center_x =
            edge_insets.left + (VIEWPORT_WIDTH_PIXELS - edge_insets.left - edge_insets.right) * 0.5;
        let inset_center_y =
            edge_insets.top + (VIEWPORT_HEIGHT_PIXELS - edge_insets.top - edge_insets.bottom) * 0.5;
        assert!(
            (box_center_screen.x - inset_center_x).abs() < FIT_BOUNDS_CORNER_TOLERANCE_PIXELS,
            "box center at x {} must sit on the inset center {inset_center_x}",
            box_center_screen.x
        );
        assert!(
            (box_center_screen.y - inset_center_y).abs() < FIT_BOUNDS_CORNER_TOLERANCE_PIXELS,
            "box center at y {} must sit on the inset center {inset_center_y}",
            box_center_screen.y
        );
    }

    #[test]
    fn fit_bounds_clamps_the_zoom_to_the_maximum() {
        // A country-scale box with a maximal zoom clamp lands on the clamp.
        let switzerland_box = bounded_box(45.8, 5.9, 47.9, 10.5);
        let camera = MapCamera::fit_bounds(
            switzerland_box,
            viewport(),
            symmetric_edge_insets(),
            FIT_MAXIMUM_ZOOM_LEVEL_CLAMPED,
        )
        .expect("box, viewport and insets are all valid");
        assert_eq!(camera.tile_zoom_level(), 3);
    }

    #[test]
    fn fit_bounds_rejects_unusable_insets_with_the_verbatim_detail() {
        let california_box = bounded_box(32.5, -124.4, 42.0, -114.1);
        let negative_insets = EdgeInsets {
            left: -1.0,
            ..symmetric_edge_insets()
        };
        assert_eq!(
            MapCamera::fit_bounds(california_box, viewport(), negative_insets, f64::MAX)
                .expect_err("a negative inset must throw"),
            SdkError::InvalidCoordinateInput {
                detail: UNSAFE_INSETS_DETAIL.to_string()
            }
        );
        let nan_insets = EdgeInsets {
            top: f64::NAN,
            ..symmetric_edge_insets()
        };
        assert_eq!(
            MapCamera::fit_bounds(california_box, viewport(), nan_insets, f64::MAX)
                .expect_err("an NaN inset must throw"),
            SdkError::InvalidCoordinateInput {
                detail: UNSAFE_INSETS_DETAIL.to_string()
            }
        );
        // Insets that leave no inset area in the viewport also fit nowhere.
        let exhaustive_insets = EdgeInsets {
            left: VIEWPORT_WIDTH_PIXELS * 0.5,
            top: VIEWPORT_HEIGHT_PIXELS * 0.5,
            right: VIEWPORT_WIDTH_PIXELS * 0.5,
            bottom: VIEWPORT_HEIGHT_PIXELS * 0.5,
        };
        match MapCamera::fit_bounds(california_box, viewport(), exhaustive_insets, f64::MAX) {
            Err(SdkError::InvalidCoordinateInput { detail }) => {
                assert!(detail.contains("leave no inset area"), "{detail}");
            }
            other => panic!("exhaustive insets must throw, got {other:?}"),
        }
    }

    #[test]
    fn fit_bounds_rejects_non_finite_maximum_zoom_with_the_verbatim_detail() {
        let california_box = bounded_box(32.5, -124.4, 42.0, -114.1);
        assert_eq!(
            MapCamera::fit_bounds(
                california_box,
                viewport(),
                symmetric_edge_insets(),
                f64::INFINITY
            )
            .expect_err("an infinite zoom clamp must throw"),
            SdkError::InvalidCoordinateInput {
                detail: NON_FINITE_MAXIMUM_ZOOM_DETAIL.to_string()
            }
        );
    }

    #[test]
    fn fit_bounds_rejects_inverted_and_arealess_boxes_with_the_verbatim_detail() {
        let inverted_box = bounded_box(42.0, -114.1, 32.5, -124.4);
        assert_eq!(
            MapCamera::fit_bounds(inverted_box, viewport(), symmetric_edge_insets(), f64::MAX)
                .expect_err("an inverted box must throw"),
            SdkError::InvalidCoordinateInput {
                detail: INVERTED_CORNERS_DETAIL.to_string()
            }
        );
        let arealess_box = bounded_box(38.5766, -121.4944, 38.5766, -121.4944);
        match MapCamera::fit_bounds(arealess_box, viewport(), symmetric_edge_insets(), f64::MAX) {
            Err(SdkError::InvalidCoordinateInput { detail }) => {
                assert!(
                    detail.contains(NO_POSITIVE_AREA_DETAIL_FRAGMENT),
                    "{detail}"
                );
            }
            other => panic!("an arealess box must throw, got {other:?}"),
        }
    }

    #[test]
    fn fit_bounds_rejects_an_invalid_box_corner_with_the_verbatim_detail() {
        let out_of_range_box = BoundingBox {
            south_west: coordinate(32.5, -124.4),
            north_east: coordinate(91.0, -114.1),
        };
        match MapCamera::fit_bounds(
            out_of_range_box,
            viewport(),
            symmetric_edge_insets(),
            f64::MAX,
        ) {
            Err(SdkError::InvalidCoordinateInput { detail }) => {
                assert!(detail.contains("latitude 91"), "{detail}");
            }
            other => panic!("a non-canonical corner must throw, got {other:?}"),
        }
    }

    #[test]
    fn fit_bounds_rejects_an_invalid_viewport_with_the_verbatim_detail() {
        let california_box = bounded_box(32.5, -124.4, 42.0, -114.1);
        let invalid_viewport = ViewportSize {
            width_pixels: 0.0,
            height_pixels: VIEWPORT_HEIGHT_PIXELS,
        };
        match MapCamera::fit_bounds(
            california_box,
            invalid_viewport,
            symmetric_edge_insets(),
            f64::MAX,
        ) {
            Err(SdkError::InvalidCoordinateInput { detail }) => {
                assert!(detail.contains("viewport extent"), "{detail}");
            }
            other => panic!("a non-positive viewport must throw, got {other:?}"),
        }
    }

    #[test]
    fn constructor_rejects_an_invalid_center() {
        assert!(matches!(
            MapCamera::new(coordinate(91.0, 0.0), 10.0, 0.0, 0.0),
            Err(SdkError::InvalidCoordinateInput { detail })
                if detail.contains("latitude 91")
        ));
    }

    #[test]
    fn constructor_clamps_zoom_and_pitch_like_the_core() {
        let extreme = MapCamera::new(coordinate(47.0, 8.5), 90.0, -10.0, 200.0)
            .expect("canonical center is valid");
        // Zoom clamps to 24, so the fetch zoom level is 24.
        assert_eq!(extreme.tile_zoom_level(), 24);
        let sub_zero = MapCamera::new(coordinate(47.0, 8.5), -3.0, -700.0, -5.0)
            .expect("canonical center is valid");
        // Zoom clamps to 0 and the bearing wraps (-700 + 720 = 20), which
        // keeps the fetch grid at the ground level.
        assert_eq!(sub_zero.tile_zoom_level(), 0);
    }

    #[test]
    fn attribute_getters_read_the_constructed_state_back() {
        let wrapped_bearing = MapCamera::new(coordinate(47.0, 8.5), 11.25, 400.0, 33.0)
            .expect("canonical center is valid");
        // The bearing wraps like the native constructor: 400 - 360 = 40.
        assert_eq!(wrapped_bearing.center().latitude, 47.0);
        assert_eq!(wrapped_bearing.center().longitude, 8.5);
        assert!((wrapped_bearing.zoom_level() - 11.25).abs() < 1e-12);
        assert!((wrapped_bearing.bearing_degrees() - 40.0).abs() < 1e-12);
        assert!((wrapped_bearing.pitch_degrees() - 33.0).abs() < 1e-12);
    }

    #[test]
    fn fit_bounds_readback_reads_the_framing_it_chose() {
        let california_box = bounded_box(32.5, -124.4, 42.0, -114.1);
        let fitted =
            MapCamera::fit_bounds(california_box, viewport(), symmetric_edge_insets(), 12.0)
                .expect("box, viewport and insets are all valid");
        // Fitted cameras are top-down and north-up by construction.
        assert!((fitted.pitch_degrees()).abs() < 1e-12);
        assert!((fitted.bearing_degrees()).abs() < 1e-12);
        // The chosen zoom is read back exactly, and the anchored center is
        // inside the framed box (the inset center under these symmetric
        // insets, but the framing contract asserts containment, not form).
        let chosen_zoom = fitted.zoom_level();
        assert!(chosen_zoom > 0.0 && chosen_zoom <= 12.0);
        let center = fitted.center();
        assert!(center.latitude >= california_box.south_west.latitude);
        assert!(center.latitude <= california_box.north_east.latitude);
        assert!(center.longitude >= california_box.south_west.longitude);
        assert!(center.longitude <= california_box.north_east.longitude);
    }

    #[test]
    fn center_projects_to_the_viewport_center() {
        let camera = MapCamera::new(
            coordinate(SACRAMENTO_LATITUDE, SACRAMENTO_LONGITUDE),
            12.0,
            33.0,
            40.0,
        )
        .expect("canonical center is valid");
        let screen_point = camera
            .project_to_screen(
                coordinate(SACRAMENTO_LATITUDE, SACRAMENTO_LONGITUDE),
                viewport(),
                60.0,
            )
            .expect("viewport is valid")
            .expect("the anchor renders inside the viewport");
        assert!((screen_point.x - VIEWPORT_WIDTH_PIXELS * 0.5).abs() < CENTER_PIXEL_TOLERANCE);
        assert!((screen_point.y - VIEWPORT_HEIGHT_PIXELS * 0.5).abs() < CENTER_PIXEL_TOLERANCE);
    }

    #[test]
    fn project_then_unproject_round_trips_a_known_target() {
        let camera = MapCamera::new(
            coordinate(SACRAMENTO_LATITUDE, SACRAMENTO_LONGITUDE),
            14.0,
            25.0,
            30.0,
        )
        .expect("canonical center is valid");
        let viewport = viewport();
        let field_of_view_degrees = 45.0;
        let target = coordinate(38.5847, -121.4811);
        let screen_point = camera
            .project_to_screen(target, viewport, field_of_view_degrees)
            .expect("viewport is valid")
            .expect("target renders in view");
        let ground = camera
            .screen_to_ground_coordinate(screen_point, viewport, field_of_view_degrees)
            .expect("viewport is valid")
            .expect("pixel is on the ground");
        assert!((ground.latitude - 38.5847).abs() < COORDINATE_TOLERANCE_DEGREES);
        assert!((ground.longitude - -121.4811).abs() < COORDINATE_TOLERANCE_DEGREES);
    }

    #[test]
    fn above_the_horizon_unprojects_to_none() {
        // At maximum pitch the top of the viewport looks past the horizon.
        let camera = MapCamera::new(coordinate(30.0, 20.0), 10.0, 0.0, 75.0)
            .expect("canonical center is valid");
        let ground = camera
            .screen_to_ground_coordinate(ScreenPoint { x: 0.0, y: 0.0 }, viewport(), 60.0)
            .expect("viewport is valid");
        assert!(ground.is_none());
    }

    #[test]
    fn projection_calls_reject_invalid_input() {
        let camera =
            MapCamera::new(camera_center(), 12.0, 0.0, 0.0).expect("canonical center is valid");
        assert!(
            camera
                .project_to_screen(coordinate(91.0, 0.0), viewport(), 60.0)
                .is_err(),
            "invalid coordinate must throw"
        );
        let invalid_viewport = ViewportSize {
            width_pixels: 0.0,
            height_pixels: 100.0,
        };
        assert!(
            camera
                .project_to_screen(camera_center(), invalid_viewport, 60.0)
                .is_err(),
            "non-positive viewport extent must throw"
        );
    }

    #[test]
    fn visible_tile_keys_list_the_root_tile_for_whole_world_views() {
        // A zoom-0 world spans exactly 512 pixels, so this top-down viewport's
        // footprint covers the whole map square and only the root tile exists.
        let camera = MapCamera::new(coordinate(38.5766, -121.4944), 0.0, 0.0, 0.0)
            .expect("canonical center is valid");
        let view = ViewportSize {
            width_pixels: 512.0,
            height_pixels: 512.0,
        };
        let visible_tiles = camera
            .visible_tile_keys(view, 60.0, camera.tile_zoom_level())
            .expect("viewport is valid");
        assert_eq!(
            visible_tiles,
            vec![TileKey {
                tile_x: 0,
                tile_y: 0,
                zoom_level: 0,
            }]
        );
    }

    #[test]
    fn visible_tile_keys_include_the_center_tile_at_pitch() {
        let camera = MapCamera::new(
            coordinate(SACRAMENTO_LATITUDE, SACRAMENTO_LONGITUDE),
            14.0,
            0.0,
            45.0,
        )
        .expect("canonical center is valid");
        let visible_tiles = camera
            .visible_tile_keys(viewport(), 60.0, camera.tile_zoom_level())
            .expect("viewport is valid");
        assert!(
            visible_tiles.contains(&TileKey {
                tile_x: SACRAMENTO_TILE_X,
                tile_y: SACRAMENTO_TILE_Y,
                zoom_level: SACRAMENTO_TILE_ZOOM_LEVEL,
            }),
            "center tile missing from {visible_tiles:?}"
        );
        // Bearing 0 tilts the far edge north of the anchor; no row renders far
        // south of the known center tile's row.
        for tile_key in &visible_tiles {
            assert!(
                tile_key.tile_y <= SACRAMENTO_TILE_Y + 1,
                "footprint drifts far south: {tile_key:?}"
            );
        }
    }

    #[test]
    fn tile_zoom_level_rounds_fractional_zoom() {
        let camera =
            MapCamera::new(camera_center(), 12.4, 0.0, 0.0).expect("canonical center is valid");
        assert_eq!(camera.tile_zoom_level(), 12);
        let camera =
            MapCamera::new(camera_center(), 12.6, 0.0, 0.0).expect("canonical center is valid");
        assert_eq!(camera.tile_zoom_level(), 13);
    }

    #[test]
    fn interpolate_reaches_both_endpoints_verbatim() {
        let start = MapCamera::new(coordinate(48.0, 11.0), 10.0, 350.0, 10.0)
            .expect("canonical center is valid");
        let target = MapCamera::new(coordinate(49.0, 12.0), 12.0, 10.0, 30.0)
            .expect("canonical center is valid");
        let at_start = MapCamera::interpolate(Arc::new(start), Arc::new(target), 0.0)
            .expect("zero is a finite fraction");
        assert_eq!(at_start.center().latitude, 48.0);
        assert_eq!(at_start.center().longitude, 11.0);
        assert_eq!(at_start.zoom_level(), 10.0);
        assert_eq!(at_start.bearing_degrees(), 350.0);
        assert_eq!(at_start.pitch_degrees(), 10.0);
        let at_target = MapCamera::interpolate(Arc::new(start), Arc::new(target), 1.0)
            .expect("one is a finite fraction");
        assert_eq!(at_target.center().latitude, 49.0);
        assert_eq!(at_target.center().longitude, 12.0);
        assert_eq!(at_target.zoom_level(), 12.0);
        assert_eq!(at_target.bearing_degrees(), 10.0);
        assert_eq!(at_target.pitch_degrees(), 30.0);
    }

    #[test]
    fn interpolate_mid_animation_takes_the_shortest_bearing_arc() {
        let start = MapCamera::new(coordinate(48.0, 11.0), 10.0, 350.0, 10.0)
            .expect("canonical center is valid");
        let target = MapCamera::new(coordinate(49.0, 12.0), 12.0, 10.0, 30.0)
            .expect("canonical center is valid");
        let midpoint = MapCamera::interpolate(Arc::new(start), Arc::new(target), 0.5)
            .expect("one half is a finite fraction");
        // Smoothstep(0.5) is 0.5, so the scalar fields sit on their midpoint;
        // the bearing crosses north (350 -> 10) instead of turning backwards.
        assert!((midpoint.zoom_level() - 11.0).abs() < 1e-9);
        assert!((midpoint.bearing_degrees()).abs() < 1e-9);
        assert!((midpoint.pitch_degrees() - 20.0).abs() < 1e-9);
        // The center travels the short way east from longitude 11 to 12.
        assert!(midpoint.center().longitude > 11.0 && midpoint.center().longitude < 12.0);
    }

    #[test]
    fn interpolate_clamps_out_of_range_progress() {
        let start = MapCamera::new(coordinate(48.0, 11.0), 10.0, 0.0, 0.0)
            .expect("canonical center is valid");
        let target = MapCamera::new(coordinate(49.0, 12.0), 12.0, 90.0, 30.0)
            .expect("canonical center is valid");
        let before_start = MapCamera::interpolate(Arc::new(start), Arc::new(target), -2.0)
            .expect("a finite fraction outside the range clamps");
        assert_eq!(before_start.center().latitude, 48.0);
        let past_target = MapCamera::interpolate(Arc::new(start), Arc::new(target), 4.0)
            .expect("a finite fraction outside the range clamps");
        assert_eq!(past_target.center().latitude, 49.0);
        assert_eq!(past_target.bearing_degrees(), 90.0);
    }

    #[test]
    fn interpolate_rejects_a_non_finite_fraction() {
        let start =
            Arc::new(MapCamera::new(camera_center(), 10.0, 0.0, 0.0).expect("canonical center"));
        let target =
            Arc::new(MapCamera::new(camera_center(), 12.0, 0.0, 0.0).expect("canonical center"));
        for non_finite_fraction in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                MapCamera::interpolate(
                    Arc::clone(&start),
                    Arc::clone(&target),
                    non_finite_fraction
                )
                .expect_err("a non-finite fraction must throw"),
                SdkError::InvalidCoordinateInput {
                    detail: NON_FINITE_INTERPOLATION_FRACTION_DETAIL.to_string()
                }
            );
        }
    }

    #[test]
    fn compiled_version_matches_cargo_package_version() {
        assert!(get_sdk_compiled_version() == crate::SDK_COMPILED_VERSION);
        assert!(!SDK_COMPILED_VERSION.trim().is_empty());
    }
}
