//! Camera model: geographic center, zoom, bearing, and pitch, projected into
//! a perspective viewport whose ground magnification matches the zoom scale.
//!
//! The camera uses an anchored perspective model: the ground point under
//! [`MapCamera::center`] always sits at the viewport center, the camera hovers
//! above it at the altitude implied by the zoom level, and bearing and pitch
//! rotate and tilt the view around that anchor. Projection math runs on
//! `f64` matrices because world units reach ~8.6e9 pixels at zoom 24.

use crate::projection::{
    MERCATOR_TILE_SIZE_PIXELS, TILE_BOUNDARY_EPSILON, coordinate_to_normalized_world,
    normalized_world_to_coordinate, tile_key_for_coordinate, zoom_scale_pixels,
};
use crate::tile_key::TileKey;
use geospatial_types::{BoundingBox, GeographicCoordinate};
use glam::{DMat4, DVec3, DVec4};
use std::collections::HashSet;

/// Field of view used when a caller does not configure one.
pub const DEFAULT_FIELD_OF_VIEW_DEGREES: f64 = 60.0;

/// Largest usable pitch; steeper tilts push most of the viewport below the
/// horizon where no ground is visible.
pub const MAXIMUM_PITCH_DEGREES: f64 = 75.0;

/// Smallest and largest zoom levels the camera accepts.
pub const MINIMUM_ZOOM_LEVEL: f64 = 0.0;
pub const MAXIMUM_ZOOM_LEVEL: f64 = 24.0;

/// Near clip plane distance as a fraction of the far plane. The ground plane
/// needs only enough near depth to stay numerically sane at high zoom, where
/// world coordinates reach ~1e9 pixels.
const NEAR_PLANE_TO_FAR_PLANE_RATIO: f64 = 1e-5;

/// The far clip plane reaches this multiple of the camera altitude, far
/// beyond any ground detail the anchored zoom implies.
const FAR_PLANE_ALTITUDE_MULTIPLIER: f64 = 100.0;

/// Screen-pixel NDC depth sentinels for the two-point ray inversion: depth
/// one sits on the near plane, depth `MAXIMUM_NORMALIZED_DEPTH` on the far
/// plane (or behind it).
const MINIMUM_NORMALIZED_DEPTH: f64 = -1.0;
const MAXIMUM_NORMALIZED_DEPTH: f64 = 1.0;

/// Guard on the inverted clip weights: below this the two reference points
/// degenerate and the pixel ray is meaningless.
const MINIMUM_CLIP_WEIGHT: f64 = 1e-12;

/// Squared-length guard rejecting degenerate screen rays whose two reference
/// points coincide after inversion.
const MINIMUM_RAY_LENGTH_SQUARED: f64 = 1e-24;

/// Largest ray z-component still counted as descending toward the ground
/// plane; anything flatter is at or above the horizon, where unprojection
/// reports no ground.
const MINIMUM_DOWNWARD_RAY_COMPONENT: f64 = -1e-12;

/// Half-extent of the ground quad the frustum clip starts from, as a multiple
/// of the far plane — comfortably beyond any ground the clip can leave.
const GROUND_CLIP_REACH_MULTIPLIER: f64 = 8.0;

/// Viewport dimensions in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewportPixels {
    /// Horizontal extent in pixels.
    pub width_pixels: f64,
    /// Vertical extent in pixels.
    pub height_pixels: f64,
}

impl ViewportPixels {
    /// Builds a viewport, rejecting non-positive extents.
    ///
    /// # Parameters
    /// - `width_pixels` — horizontal extent in physical pixels.
    /// - `height_pixels` — vertical extent in physical pixels.
    ///
    /// # Errors
    /// Returns a description of the refused extent when either dimension is
    /// not strictly positive.
    pub fn new(width_pixels: f64, height_pixels: f64) -> Result<Self, String> {
        if width_pixels <= 0.0 || height_pixels <= 0.0 {
            return Err(format!(
                "viewport extent {width_pixels}x{height_pixels} must be positive"
            ));
        }
        Ok(Self {
            width_pixels,
            height_pixels,
        })
    }

    fn aspect_ratio(&self) -> f64 {
        self.width_pixels / self.height_pixels
    }
}

/// Screen-space insets kept clear of map content: distances from each
/// viewport edge in pixels. The leading map SDKs carry insets as a first-class
/// camera property; bounds fitting (see [`fit_bounds`]) anchors its framing
/// and zoom to the inset region so UI overlays — route cards, attribution,
/// sheet panels — never cover the content the camera guarantees visible.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeInsetsPixels {
    /// Pixels kept clear from the left viewport edge.
    pub left: f64,
    /// Pixels kept clear from the top viewport edge.
    pub top: f64,
    /// Pixels kept clear from the right viewport edge.
    pub right: f64,
    /// Pixels kept clear from the bottom viewport edge.
    pub bottom: f64,
}

impl EdgeInsetsPixels {
    /// Insets equal on both axes: `horizontal_pixels` from left and right,
    /// `vertical_pixels` from top and bottom.
    pub fn symmetric(horizontal_pixels: f64, vertical_pixels: f64) -> Self {
        Self {
            left: horizontal_pixels,
            top: vertical_pixels,
            right: horizontal_pixels,
            bottom: vertical_pixels,
        }
    }

    /// No insets — the whole viewport is available content area.
    pub fn zero() -> Self {
        Self::symmetric(0.0, 0.0)
    }
}

/// Viewer position and orientation anchored to a geographic center.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapCamera {
    /// Ground point rendered at the viewport center.
    pub center: GeographicCoordinate,
    /// Zoom level; each unit doubles the on-screen scale.
    pub zoom_level: f64,
    /// Compass direction (degrees clockwise from north) the top of the
    /// viewport points to.
    pub bearing_degrees: f64,
    /// Tilt from straight-down in degrees; zero is top-down.
    pub pitch_degrees: f64,
}

/// The Web Mercator world square's ground-equivalent length: the WGS-84
/// equatorial circumference the projection stretches onto one world edge.
pub const EARTH_EQUATORIAL_CIRCUMFERENCE_METERS: f64 = 40_075_016.686;

impl MapCamera {
    /// Ground meters represented by one screen pixel at the camera's
    /// center: the equatorial ground meters per pixel at the camera's zoom
    /// level times the cosine of the center latitude, so the answer shrinks
    /// toward the poles exactly like the projected ground shrinks in the
    /// Web Mercator projection. At any nonzero pitch this is the
    /// flat-centered approximation (the helper folds no unprojection of the
    /// pitch into the answer) — see the contract's member documentation.
    pub fn ground_meters_per_screen_pixel(&self) -> f64 {
        let world_pixels_per_edge = MERCATOR_TILE_SIZE_PIXELS * 2_f64.powf(self.zoom_level);
        let equatorial_meters_per_pixel =
            EARTH_EQUATORIAL_CIRCUMFERENCE_METERS / world_pixels_per_edge;
        equatorial_meters_per_pixel * self.center.latitude.to_radians().cos()
    }
    /// Builds a camera, clamping zoom and pitch and wrapping the bearing into
    /// `[0, 360)`.
    ///
    /// # Parameters
    /// - `center` — the ground coordinate rendered at the viewport center.
    /// - `zoom_level` — zoom level, clamped to the SDK's supported range.
    /// - `bearing_degrees` — compass direction, wrapped into `[0, 360)`.
    /// - `pitch_degrees` — tilt above the ground, clamped to the supported
    ///   range.
    ///
    /// # Returns
    /// The normalized camera pose.
    pub fn new(
        center: GeographicCoordinate,
        zoom_level: f64,
        bearing_degrees: f64,
        pitch_degrees: f64,
    ) -> Self {
        Self {
            center,
            zoom_level: zoom_level.clamp(MINIMUM_ZOOM_LEVEL, MAXIMUM_ZOOM_LEVEL),
            bearing_degrees: bearing_degrees.rem_euclid(360.0),
            pitch_degrees: pitch_degrees.clamp(0.0, MAXIMUM_PITCH_DEGREES),
        }
    }

    /// Projects a coordinate to viewport pixels (origin top-left, y down).
    ///
    /// # Parameters
    /// - `coordinate` — the geographic point to project.
    /// - `viewport` — the viewport the camera renders into.
    /// - `field_of_view_degrees` — vertical field of view of the projection.
    ///
    /// # Returns
    /// The pixel position, or `None` when the point lies behind the camera.
    pub fn project_to_screen(
        &self,
        coordinate: &GeographicCoordinate,
        viewport: &ViewportPixels,
        field_of_view_degrees: f64,
    ) -> Option<[f64; 2]> {
        let view_projection = self.view_projection_matrix(viewport, field_of_view_degrees);
        let clip_position = view_projection * self.ground_position_vector(coordinate);
        if clip_position.w <= 0.0 {
            return None;
        }
        let normalized_device = clip_position.truncate() / clip_position.w;
        Some([
            (normalized_device.x + 1.0) * 0.5 * viewport.width_pixels,
            (1.0 - (normalized_device.y + 1.0) * 0.5) * viewport.height_pixels,
        ])
    }

    /// Unprojects a viewport pixel back onto the ground plane.
    ///
    /// # Parameters
    /// - `screen_point` — the pixel to unproject, origin top-left and y down.
    /// - `viewport` — the viewport the camera renders into.
    /// - `field_of_view_degrees` — vertical field of view of the projection.
    ///
    /// # Returns
    /// The ground coordinate under the pixel, or `None` when the ray misses
    /// the ground (typically below the horizon at pitch).
    pub fn screen_to_ground_coordinate(
        &self,
        screen_point: [f64; 2],
        viewport: &ViewportPixels,
        field_of_view_degrees: f64,
    ) -> Option<GeographicCoordinate> {
        let (camera_eye, camera_forward, _, far_plane) =
            self.camera_frame(viewport, field_of_view_degrees);
        let view_projection_inverse = self
            .view_projection_matrix(viewport, field_of_view_degrees)
            .inverse();
        let normalized_x = 2.0 * screen_point[0] / viewport.width_pixels - 1.0;
        let normalized_y = 1.0 - 2.0 * screen_point[1] / viewport.height_pixels;

        // Both reference points sit on the pixel's world ray; whichever depth
        // the inverse produces, the ray direction is oriented toward the
        // camera forward so the ground walk is unambiguous even when the far
        // clip plane lies nearer than depth 1 (high zoom).
        let depth_one_clip = view_projection_inverse
            * DVec4::new(normalized_x, normalized_y, MINIMUM_NORMALIZED_DEPTH, 1.0);
        let far_plane_clip = view_projection_inverse
            * DVec4::new(normalized_x, normalized_y, MAXIMUM_NORMALIZED_DEPTH, 1.0);
        if depth_one_clip.w.abs() < MINIMUM_CLIP_WEIGHT
            || far_plane_clip.w.abs() < MINIMUM_CLIP_WEIGHT
        {
            return None;
        }
        let mut ray_direction = (far_plane_clip / far_plane_clip.w).truncate()
            - (depth_one_clip / depth_one_clip.w).truncate();
        if ray_direction.dot(camera_forward) < 0.0 {
            ray_direction = -ray_direction;
        }
        if ray_direction.length_squared() < MINIMUM_RAY_LENGTH_SQUARED {
            return None;
        }
        let ray_direction = ray_direction.normalize();
        // Only rays that descend toward the ground plane reach it.
        if ray_direction.z >= MINIMUM_DOWNWARD_RAY_COMPONENT {
            return None;
        }
        // The far clip bounds which ground pixels are actually renderable.
        let ground_distance = -camera_eye.z / ray_direction.z;
        if ground_distance > far_plane {
            return None;
        }
        let ground_world = camera_eye + ray_direction * ground_distance;
        let zoom_scale = zoom_scale_pixels(self.zoom_level);
        // Convert the y-north camera world space back to tile-convention
        // normalized world space (y southward).
        let normalized_world = [ground_world.x / zoom_scale, -ground_world.y / zoom_scale];
        Some(normalized_world_to_coordinate(normalized_world))
    }

    /// The coordinate's position in camera world space: x east, y **north**,
    /// z up on the ground plane. The tile-convention y-south normalized world
    /// space of [`crate::projection`] flips to y-north exactly here, so the
    /// camera frame stays in a right-handed geographic system. Longitude wraps
    /// to the nearest world copy around the camera center, so a view across
    /// the antimeridian renders the adjacent world copy.
    fn ground_position_vector(&self, coordinate: &GeographicCoordinate) -> DVec4 {
        let zoom_scale = zoom_scale_pixels(self.zoom_level);
        let normalized_world = coordinate_to_normalized_world(coordinate);
        let center_normalized_world = coordinate_to_normalized_world(&self.center);
        let wrapped_x_offset =
            normalized_shortest_delta(center_normalized_world[0], normalized_world[0]);
        DVec4::new(
            (center_normalized_world[0] + wrapped_x_offset) * zoom_scale,
            -normalized_world[1] * zoom_scale,
            0.0,
            1.0,
        )
    }

    /// The camera's eye position, forward direction, up direction, and far
    /// clip plane in world space. Ground magnification at the center equals
    /// `zoom_scale_pixels(zoom_level)` pixels per normalized world unit.
    fn camera_frame(
        &self,
        viewport: &ViewportPixels,
        field_of_view_degrees: f64,
    ) -> (DVec3, DVec3, DVec3, f64) {
        let pitch_radians = self
            .pitch_degrees
            .clamp(0.0, MAXIMUM_PITCH_DEGREES)
            .to_radians();
        let bearing_radians = self.bearing_degrees.to_radians();
        let field_of_view_radians = field_of_view_degrees.to_radians();

        // Altitude above the anchor in the zoom-scaled pixel frame. World
        // coordinates already scale with zoom (x pixels per normalized unit),
        // so the half-viewport of `height_pixels` spans `(height/2)` pixels of
        // ground at center distance under half the field of view.
        let camera_altitude = (viewport.height_pixels * 0.5) / (field_of_view_radians * 0.5).tan();

        let bearing_rotation = DMat4::from_rotation_z(-bearing_radians);
        let anchor = DVec3::new(
            self.ground_position_vector(&self.center).x,
            self.ground_position_vector(&self.center).y,
            0.0,
        );
        let camera_eye = anchor
            + bearing_rotation.transform_point3(DVec3::new(
                0.0,
                -camera_altitude * pitch_radians.sin(),
                camera_altitude * pitch_radians.cos(),
            ));
        let camera_forward = bearing_rotation.transform_point3(DVec3::new(
            0.0,
            pitch_radians.sin(),
            -pitch_radians.cos(),
        ));
        // At pitch zero the screen-top direction must face north. World space
        // is converted to y-north at the boundary (see
        // `ground_position_vector`), so screen-top at pitch zero is +y.
        let camera_up = bearing_rotation.transform_point3(DVec3::new(
            0.0,
            pitch_radians.cos(),
            pitch_radians.sin(),
        ));
        let far_plane = camera_altitude * FAR_PLANE_ALTITUDE_MULTIPLIER;
        (camera_eye, camera_forward, camera_up, far_plane)
    }

    /// Perspective-view matrix matching the anchored camera. Ground
    /// magnification at the center equals `zoom_scale_pixels(zoom_level)`
    /// pixels per normalized world unit.
    pub fn view_projection_matrix(
        &self,
        viewport: &ViewportPixels,
        field_of_view_degrees: f64,
    ) -> DMat4 {
        let (camera_eye, camera_forward, camera_up, far_plane) =
            self.camera_frame(viewport, field_of_view_degrees);

        let view_matrix = DMat4::look_to_rh(camera_eye, camera_forward, camera_up);
        let near_plane = far_plane * NEAR_PLANE_TO_FAR_PLANE_RATIO;
        let projection_matrix = DMat4::perspective_rh(
            field_of_view_degrees.to_radians(),
            viewport.aspect_ratio(),
            near_plane,
            far_plane,
        );
        projection_matrix * view_matrix
    }

    /// Nearest whole zoom level a tile fetch should use for this camera:
    /// fractional camera zooms round to the tile grid that best matches the
    /// on-screen scale.
    pub fn tile_zoom_level(&self) -> u8 {
        (self
            .zoom_level
            .round()
            .clamp(MINIMUM_ZOOM_LEVEL, MAXIMUM_ZOOM_LEVEL)) as u8
    }

    /// Every tile at `tile_zoom_level` whose area intersects the visible
    /// ground: the view frustum cut by the ground plane, wrapped to the
    /// camera's world copy across the antimeridian. The footprint reduces to
    /// its axis-aligned normalized-world bounding box, so bearing-rotated
    /// views also list the covering corner tiles. Returns the tiles sorted
    /// row-major from the north.
    pub fn visible_tile_keys(
        &self,
        viewport: &ViewportPixels,
        field_of_view_degrees: f64,
        tile_zoom_level: u8,
    ) -> Vec<TileKey> {
        let zoom_scale = zoom_scale_pixels(self.zoom_level);
        let ground_polygon = self.clipped_ground_polygon_vertices(viewport, field_of_view_degrees);
        if ground_polygon.is_empty() {
            return vec![tile_key_for_coordinate(&self.center, tile_zoom_level)];
        }

        // y grows south in normalized world space; ground points convert from
        // the y-north camera world space exactly at the camera boundary. Tiles
        // clamp to the map square in y; x runs on raw world-copy indices and
        // wraps below, so a view reaching past the antimeridian naturally
        // lists tiles from both copies.
        let tiles_per_axis = (1u64 << tile_zoom_level) as f64;
        let minimum_y = ground_polygon
            .iter()
            .map(|vertex| -vertex.y / zoom_scale)
            .fold(f64::INFINITY, f64::min)
            .max(0.0);
        let maximum_y = ground_polygon
            .iter()
            .map(|vertex| -vertex.y / zoom_scale)
            .fold(f64::NEG_INFINITY, f64::max)
            .min(1.0);
        let minimum_column =
            ((minimum_y * tiles_per_axis).floor().max(0.0) as u64).min((tiles_per_axis as u64) - 1);
        let maximum_column = ((maximum_y * tiles_per_axis - TILE_BOUNDARY_EPSILON)
            .floor()
            .max(0.0) as u64)
            .min((tiles_per_axis as u64) - 1);

        let minimum_x = ground_polygon
            .iter()
            .map(|vertex| vertex.x / zoom_scale)
            .fold(f64::INFINITY, f64::min);
        let maximum_x = ground_polygon
            .iter()
            .map(|vertex| vertex.x / zoom_scale)
            .fold(f64::NEG_INFINITY, f64::max);
        let minimum_index = (minimum_x * tiles_per_axis).floor() as i64;
        // The right-hand boundary stays exclusive, mirroring floor semantics.
        let maximum_index = (maximum_x * tiles_per_axis - TILE_BOUNDARY_EPSILON).floor() as i64;

        // Raw indices may exceed the grid or go negative near the antimeridian
        // and at low zoom, where the view reaches past one world copy; both
        // wrap back onto the grid modulo the copy count.
        let tiles_per_axis_index = tiles_per_axis as i64;
        let mut visible_tiles: HashSet<TileKey> = HashSet::new();
        for row in minimum_column..=maximum_column {
            for raw_index in minimum_index..=maximum_index {
                let wrapped_index = raw_index.rem_euclid(tiles_per_axis_index) as u32;
                if let Ok(tile_key) = TileKey::new(wrapped_index, row as u32, tile_zoom_level) {
                    visible_tiles.insert(tile_key);
                }
            }
        }
        let mut visible_tiles: Vec<TileKey> = visible_tiles.into_iter().collect();
        visible_tiles.sort_by_key(|tile_key| (tile_key.tile_y, tile_key.tile_x));
        visible_tiles
    }

    /// The view frustum clipped to the ground plane, in camera world space
    /// (x east, y north, z up on the ground). Starts from a ground quad well
    /// beyond the far plane and clips against the six frustum half-spaces, so
    /// above-horizon and out-of-range ground drops out exactly rather than by
    /// sampling.
    fn clipped_ground_polygon_vertices(
        &self,
        viewport: &ViewportPixels,
        field_of_view_degrees: f64,
    ) -> Vec<DVec3> {
        let (camera_eye, camera_forward, camera_up, far_plane) =
            self.camera_frame(viewport, field_of_view_degrees);
        let camera_right = camera_forward.cross(camera_up);
        let field_of_view_radians = field_of_view_degrees.to_radians();
        let vertical_slope = (field_of_view_radians * 0.5).tan();
        let horizontal_slope = viewport.aspect_ratio() * vertical_slope;
        // Side planes pass through the eye; near and far sit along the look
        // axis. A point stays when `normal · (point − eye) ≥ bias`.
        let near_plane = far_plane * NEAR_PLANE_TO_FAR_PLANE_RATIO;
        let frustum_planes: [(DVec3, f64); 6] = [
            (camera_forward * horizontal_slope + camera_right, 0.0),
            (camera_forward * horizontal_slope - camera_right, 0.0),
            (camera_forward * vertical_slope + camera_up, 0.0),
            (camera_forward * vertical_slope - camera_up, 0.0),
            (camera_forward, near_plane),
            (-camera_forward, -far_plane),
        ];

        let reach = far_plane * GROUND_CLIP_REACH_MULTIPLIER;
        let mut polygon = vec![
            DVec3::new(camera_eye.x - reach, camera_eye.y - reach, 0.0),
            DVec3::new(camera_eye.x + reach, camera_eye.y - reach, 0.0),
            DVec3::new(camera_eye.x + reach, camera_eye.y + reach, 0.0),
            DVec3::new(camera_eye.x - reach, camera_eye.y + reach, 0.0),
        ];
        for (plane_normal, plane_bias) in frustum_planes {
            let signed_distance =
                |vertex: DVec3| plane_normal.dot(vertex - camera_eye) - plane_bias;
            let mut clipped_polygon = Vec::with_capacity(polygon.len() + 1);
            for edge_index in 0..polygon.len() {
                let start_vertex = polygon[edge_index];
                let end_vertex = polygon[(edge_index + 1) % polygon.len()];
                let start_distance = signed_distance(start_vertex);
                let end_distance = signed_distance(end_vertex);
                let start_inside = start_distance >= 0.0;
                let end_inside = end_distance >= 0.0;
                if start_inside {
                    clipped_polygon.push(start_vertex);
                }
                if start_inside != end_inside {
                    let crossing_fraction = start_distance / (start_distance - end_distance);
                    clipped_polygon.push(start_vertex.lerp(end_vertex, crossing_fraction));
                }
            }
            polygon = clipped_polygon;
            if polygon.is_empty() {
                return polygon;
            }
        }
        polygon
    }
}

/// Camera interpolation over progress `0.0..=1.0` (values outside clamp).
///
/// The center interpolates linearly in mercator space along the shortest
/// longitude path, zoom interpolates in log space, and the bearing takes the
/// shortest arc.
pub fn interpolate_camera(
    start_camera: &MapCamera,
    target_camera: &MapCamera,
    progress: f64,
) -> MapCamera {
    let eased_progress = smoothstep_progress(progress);
    // Endpoints return the input verbatim so an anchored start (or settle) is
    // free of mercator round-trip noise.
    if eased_progress <= 0.0 {
        return *start_camera;
    }
    if eased_progress >= 1.0 {
        return *target_camera;
    }
    let start_normalized = coordinate_to_normalized_world(&start_camera.center);
    let target_normalized = coordinate_to_normalized_world(&target_camera.center);
    let longitude_delta = normalized_shortest_delta(start_normalized[0], target_normalized[0]);
    let interpolated_center = normalized_world_to_coordinate([
        start_normalized[0] + longitude_delta * eased_progress,
        start_normalized[1] + (target_normalized[1] - start_normalized[1]) * eased_progress,
    ]);
    let bearing_delta = normalized_shortest_delta(
        start_camera.bearing_degrees / 360.0,
        target_camera.bearing_degrees / 360.0,
    );
    let interpolated_bearing =
        (start_camera.bearing_degrees + bearing_delta * 360.0 * eased_progress).rem_euclid(360.0);
    MapCamera {
        center: interpolated_center,
        zoom_level: start_camera.zoom_level
            + (target_camera.zoom_level - start_camera.zoom_level) * eased_progress,
        bearing_degrees: interpolated_bearing,
        pitch_degrees: start_camera.pitch_degrees
            + (target_camera.pitch_degrees - start_camera.pitch_degrees) * eased_progress,
    }
}

/// Signed wrap-around delta in normalized space: the value in `[-0.5, 0.5)`
/// such that `start + delta ≡ target (mod 1)`. Shared with the renderer, which
/// must wrap tile spans into the camera's world copy the same way.
pub(crate) fn normalized_shortest_delta(start: f64, target: f64) -> f64 {
    let forward_delta = (target - start).rem_euclid(1.0);
    if forward_delta > 0.5 {
        forward_delta - 1.0
    } else {
        forward_delta
    }
}

/// Smoothstep progress shaping shared by every camera animation: the eased
/// progress used to travel along the pose curve.
fn smoothstep_progress(progress: f64) -> f64 {
    let clamped_progress = progress.clamp(0.0, 1.0);
    clamped_progress * clamped_progress * (3.0 - 2.0 * clamped_progress)
}

/// Default zoom-out tightness of the fly-to arc (`curve`, ρ in the fly-to
/// derivation): larger values dip the zoom deeper below the endpoints' levels
/// as the map travels along a long flight path. The arc also self-limits when
/// the unclamped tightness would dip the zoom past the SDK's minimum zoom
/// level.
pub const DEFAULT_FLY_TO_ZOOM_OUT_CURVE: f64 = 1.42;

/// Default travel speed of the fly-to arc, in screenfulls of the arc per
/// second (`speed` in the fly-to derivation): the derived duration is the arc
/// length divided by this.
pub const DEFAULT_FLY_TO_TRAVEL_SPEED: f64 = 1.2;

/// Flight-path length below which the arc degenerates to a pure zoom: a
/// center this close to already being over the target needs no ascent-descent
/// zoom relief (the fly-to derivation's path epsilon).
const FLY_TO_DEGENERATE_PATH_PIXEL_TOLERANCE: f64 = 0.000002;

/// Visible-span difference below which a degenerate-path fly resolves
/// instantly instead of animating (the fly-to derivation's identical-span
/// epsilon).
const FLY_TO_IDENTICAL_SPAN_TOLERANCE: f64 = 0.000001;

/// Tuning knobs for the fly-to camera arc; every field has a documented
/// default via [`Default`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlyToOptions {
    /// Largest zoom level the arc may reach toward; both the target zoom and
    /// the derived arc respect the SDK zoom range and this ceiling.
    pub maximum_zoom_level: f64,
    /// The zoom-out tightness of the arc (see
    /// [`DEFAULT_FLY_TO_ZOOM_OUT_CURVE`]).
    pub zoom_out_curve: f64,
    /// The travel speed the derived duration assumes, in screenfulls of the
    /// arc per second (see [`DEFAULT_FLY_TO_TRAVEL_SPEED`]).
    pub travel_speed: f64,
    /// Explicit duration override; `None` derives the duration from the arc
    /// length over `travel_speed`.
    pub duration_seconds: Option<f64>,
}

impl Default for FlyToOptions {
    /// The default settings: tightness 1.42, speed 1.2, derived duration, and
    /// no stronger zoom ceiling than the SDK's maximum.
    fn default() -> Self {
        Self {
            maximum_zoom_level: MAXIMUM_ZOOM_LEVEL,
            zoom_out_curve: DEFAULT_FLY_TO_ZOOM_OUT_CURVE,
            travel_speed: DEFAULT_FLY_TO_TRAVEL_SPEED,
            duration_seconds: None,
        }
    }
}

/// Shape of the fly-to arc between its endpoints (see [`fly_to_animation`]).
#[derive(Debug, Clone, Copy, PartialEq)]
enum FlyToArcShape {
    /// Center and zoom both travel: the hyperbolic ascent-descent arc.
    TravelingArc,
    /// Center holds still while the zoom rises or falls exponentially; the
    /// sign drives the span toward the target's (wider spans zoom out).
    PureZoom { span_growth_sign: f64 },
    /// Endpoints coincide; the animation resolves instantly (zero duration).
    Instantaneous,
}

/// The fly-to pose parameters: everything `camera_at` needs to evaluate the
/// arc. Distances are measured in "screenfulls" of the initial viewport span
/// (the fly-to derivation's unit), which is what makes the pose independent of
/// the viewport except through the path length.
#[derive(Debug, Clone, Copy)]
struct FlyToPathPose {
    initial_visible_span_pixels: f64,
    start_normalized_center: [f64; 2],
    normalized_center_delta: [f64; 2],
    start_zoom_level: f64,
    /// The flight-path length in pixels at the start zoom's scale (u₁ in the
    /// fly-to derivation); the traveled center fraction divides by it.
    flight_path_pixels: f64,
    /// Zoom-out factor at the arc's start (r₀ in the fly-to derivation).
    arc_start_zoom_out_factor: f64,
    arc_zoom_out_curve: f64,
    /// Total arc length the eased progress travels (S in the derivation).
    arc_path_length_in_screenfulls: f64,
    arc_shape: FlyToArcShape,
}

/// The camera animation that flies between two cameras instead of cutting
/// between them: zoom follows the fly-to arc's exponential dip — long travels
/// zoom out along the way and zoom back in on arrival, short travels slide
/// almost monotonically — while the center, bearing, and pitch ease normally.
///
/// The arc follows the standard fly-to derivation, adapted to the SDK's
/// mercator-anchored camera: the visible span `w(s)` shrinks and grows
/// hyperbolically along the path in units of the initial viewport span, and
/// the derived duration (absent an override) is the arc length over the travel
/// speed. The outer easing stays the SDK's smoothstep (`interpolate_camera`
/// uses the same), so both pose curves share one easing story rather than the
/// usual cubic-bezier easing.
///
/// Use [`CameraAnimation::new`] for a plain cut-interpolated animation.
///
/// # Parameters
/// - `start_camera` — the pose the animation begins at.
/// - `target_camera` — the pose the animation ends at.
/// - `viewport` — the viewport the animation is evaluated against.
/// - `fly_to_options` — the zoom ceiling, arc tightness, travel speed and
///   optional duration override.
///
/// # Errors
/// Returns a description of the unusable input when a zoom level, the
/// viewport extent, the zoom-out curve or the travel speed is not finite (or
/// not positive where required).
pub fn fly_to_animation(
    start_camera: &MapCamera,
    target_camera: &MapCamera,
    viewport: &ViewportPixels,
    fly_to_options: FlyToOptions,
) -> Result<CameraAnimation, String> {
    if !start_camera.zoom_level.is_finite() || !target_camera.zoom_level.is_finite() {
        return Err("the start and target zoom levels must be finite".to_string());
    }
    if !viewport.width_pixels.is_finite()
        || !viewport.height_pixels.is_finite()
        || viewport.width_pixels <= 0.0
        || viewport.height_pixels <= 0.0
    {
        return Err("the viewport extent must be finite and positive".to_string());
    }
    if !fly_to_options.zoom_out_curve.is_finite() || fly_to_options.zoom_out_curve <= 0.0 {
        return Err("the zoom-out curve must be finite and positive".to_string());
    }
    if !fly_to_options.travel_speed.is_finite() || fly_to_options.travel_speed <= 0.0 {
        return Err("the travel speed must be finite and positive".to_string());
    }
    if let Some(duration_seconds) = fly_to_options.duration_seconds
        && !duration_seconds.is_finite()
    {
        return Err("the duration must be finite".to_string());
    }

    // The zoom ceiling applies to the arc's end like every transform update.
    let arc_maximum_zoom = fly_to_options
        .maximum_zoom_level
        .clamp(MINIMUM_ZOOM_LEVEL, MAXIMUM_ZOOM_LEVEL);
    let clamped_target_zoom = target_camera
        .zoom_level
        .clamp(MINIMUM_ZOOM_LEVEL, arc_maximum_zoom);

    // w₀ and w₁: visible spans measured in pixels with respect to the initial
    // zoom's scale. The viewport enters the pose only through w₀, and every
    // derived quantity is a span ratio from there.
    let initial_visible_span_pixels = viewport.width_pixels.max(viewport.height_pixels);
    let zoom_pixel_ratio = (clamped_target_zoom - start_camera.zoom_level).exp2();
    let final_visible_span_pixels = initial_visible_span_pixels / zoom_pixel_ratio;

    // The flight path in pixels at the start zoom's scale: the shortest
    // wrapped longitude path, matching the SDK's nearest-world-copy rule.
    let start_normalized_center = coordinate_to_normalized_world(&start_camera.center);
    let target_normalized_center = coordinate_to_normalized_world(&target_camera.center);
    let normalized_center_delta = [
        normalized_shortest_delta(start_normalized_center[0], target_normalized_center[0]),
        target_normalized_center[1] - start_normalized_center[1],
    ];
    let start_zoom_pixel_scale = zoom_scale_pixels(start_camera.zoom_level);
    let flight_path_pixels = (normalized_center_delta[0] * start_zoom_pixel_scale)
        .hypot(normalized_center_delta[1] * start_zoom_pixel_scale);

    // The arc may not zoom the visible span past the SDK's minimum zoom, so
    // the tightness yields to the w_max bound (GL JS: ρ = min(ρ, √(wmax/u1·2))).
    let minimum_zoom_pixel_ratio = (MINIMUM_ZOOM_LEVEL - start_camera.zoom_level).exp2();
    let maximum_visible_span_pixels = initial_visible_span_pixels / minimum_zoom_pixel_ratio;
    let arc_zoom_out_curve = fly_to_options
        .zoom_out_curve
        .min((maximum_visible_span_pixels / flight_path_pixels * 2.0).sqrt());
    let arc_zoom_out_curve_squared = arc_zoom_out_curve * arc_zoom_out_curve;

    // rᵢ: the zoom-out factor at one arc end; the ascent leg (false) mirrors
    // the initial span w0, the descent leg (true) mirrors the final span w1.
    let zoom_out_factor = |descent: bool| -> f64 {
        let arc_bias = (final_visible_span_pixels * final_visible_span_pixels
            - initial_visible_span_pixels * initial_visible_span_pixels
            + (if descent { -1.0 } else { 1.0 })
                * arc_zoom_out_curve_squared
                * arc_zoom_out_curve_squared
                * flight_path_pixels
                * flight_path_pixels)
            / (2.0
                * (if descent {
                    final_visible_span_pixels
                } else {
                    initial_visible_span_pixels
                })
                * arc_zoom_out_curve_squared
                * flight_path_pixels);
        ((arc_bias * arc_bias + 1.0).sqrt() - arc_bias).ln()
    };
    let arc_start_zoom_out_factor = zoom_out_factor(false);
    let arc_path_length_in_screenfulls =
        (zoom_out_factor(true) - arc_start_zoom_out_factor) / arc_zoom_out_curve;

    // When the path length cannot support both legs (GL JS: |u₁| below the
    // path epsilon, or a non-finite S), the flight collapses to a zoom-only
    // arc along an exponential span — or resolves instantly when the spans
    // also coincide.
    let (arc_shape, arc_path_length_in_screenfulls_resolved) = if flight_path_pixels.abs()
        < FLY_TO_DEGENERATE_PATH_PIXEL_TOLERANCE
        || !arc_path_length_in_screenfulls.is_finite()
    {
        if (initial_visible_span_pixels - final_visible_span_pixels).abs()
            < FLY_TO_IDENTICAL_SPAN_TOLERANCE
        {
            (FlyToArcShape::Instantaneous, 0.0)
        } else {
            let span_growth_sign = if final_visible_span_pixels < initial_visible_span_pixels {
                -1.0
            } else {
                1.0
            };
            (
                FlyToArcShape::PureZoom { span_growth_sign },
                (final_visible_span_pixels / initial_visible_span_pixels)
                    .ln()
                    .abs()
                    / arc_zoom_out_curve,
            )
        }
    } else {
        (FlyToArcShape::TravelingArc, arc_path_length_in_screenfulls)
    };

    let animation_duration_seconds = match fly_to_options.duration_seconds {
        Some(explicit_duration_seconds) => explicit_duration_seconds,
        None => arc_path_length_in_screenfulls_resolved / fly_to_options.travel_speed,
    };

    Ok(CameraAnimation {
        start_camera: *start_camera,
        target_camera: *target_camera,
        duration_seconds: animation_duration_seconds,
        path_curve: CameraPathCurve::FlyToArc(FlyToPathPose {
            initial_visible_span_pixels,
            start_normalized_center,
            normalized_center_delta,
            start_zoom_level: start_camera.zoom_level,
            flight_path_pixels,
            arc_start_zoom_out_factor,
            arc_zoom_out_curve,
            arc_path_length_in_screenfulls: arc_path_length_in_screenfulls_resolved,
            arc_shape,
        }),
    })
}

impl FlyToPathPose {
    /// The camera pose along the fly-to arc at eased progress `progress` in
    /// `0..=1` (values outside clamp); progress zero returns `start` verbatim.
    fn camera_at_progress(
        &self,
        progress: f64,
        start_camera: &MapCamera,
        target_camera: &MapCamera,
    ) -> MapCamera {
        let eased_progress = smoothstep_progress(progress);
        if eased_progress <= 0.0 {
            return *start_camera;
        }
        let arc_position = eased_progress * self.arc_path_length_in_screenfulls;
        // w(s): visible span relative to the initial span; the zoom is the
        // initial zoom plus the span's reciprocal in log-2 space.
        let visible_span_fraction = match self.arc_shape {
            FlyToArcShape::TravelingArc => {
                self.arc_start_zoom_out_factor.cosh()
                    / (self.arc_start_zoom_out_factor + self.arc_zoom_out_curve * arc_position)
                        .cosh()
            }
            FlyToArcShape::PureZoom { span_growth_sign } => {
                (span_growth_sign * self.arc_zoom_out_curve * arc_position).exp()
            }
            // A zero-duration animation resolves to the target before the
            // pose is ever consulted; this branch exists for completeness.
            FlyToArcShape::Instantaneous => return *target_camera,
        };
        let arc_zoom_level = self.start_zoom_level - visible_span_fraction.log2();
        // u(s): the center's traveled fraction along the shortest wrapped
        // path; a degenerate (zoom-only) arc holds the start center.
        let arc_center_fraction = match self.arc_shape {
            FlyToArcShape::TravelingArc => {
                self.initial_visible_span_pixels
                    * ((self.arc_start_zoom_out_factor.cosh()
                        * (self.arc_start_zoom_out_factor
                            + self.arc_zoom_out_curve * arc_position)
                            .tanh()
                        - self.arc_start_zoom_out_factor.sinh())
                        / (self.arc_zoom_out_curve * self.arc_zoom_out_curve))
                    / self.flight_path_pixels
            }
            FlyToArcShape::PureZoom { .. } | FlyToArcShape::Instantaneous => 0.0,
        };
        let clamped_center_fraction = arc_center_fraction.clamp(0.0, 1.0);
        let arc_center = if clamped_center_fraction == 0.0 {
            start_camera.center
        } else {
            normalized_world_to_coordinate([
                self.start_normalized_center[0]
                    + self.normalized_center_delta[0] * clamped_center_fraction,
                self.start_normalized_center[1]
                    + self.normalized_center_delta[1] * clamped_center_fraction,
            ])
        };
        // Angles ease with the progress itself (GL JS: bearing and pitch take
        // the eased frame fraction, not the arc position).
        let bearing_delta = normalized_shortest_delta(
            start_camera.bearing_degrees / 360.0,
            target_camera.bearing_degrees / 360.0,
        );
        let arc_bearing_degrees = (start_camera.bearing_degrees
            + bearing_delta * 360.0 * eased_progress)
            .rem_euclid(360.0);
        let arc_pitch_degrees = start_camera.pitch_degrees
            + (target_camera.pitch_degrees - start_camera.pitch_degrees) * eased_progress;
        MapCamera::new(
            arc_center,
            arc_zoom_level,
            arc_bearing_degrees,
            arc_pitch_degrees,
        )
    }
}

/// The pose curve a [`CameraAnimation`] travels between its endpoints.
#[derive(Debug, Clone, Copy)]
enum CameraPathCurve {
    /// Straight smoothstep-eased mercator interpolation ([`interpolate_camera`]).
    MercatorSmoothstep,
    /// The hyperbolic fly-to zoom arc (see [`fly_to_animation`]).
    FlyToArc(FlyToPathPose),
}

/// Time-driven camera transition between two cameras.
pub struct CameraAnimation {
    start_camera: MapCamera,
    target_camera: MapCamera,
    duration_seconds: f64,
    path_curve: CameraPathCurve,
}

impl CameraAnimation {
    /// Builds an animation; non-positive durations resolve instantly to the
    /// target camera.
    pub fn new(start_camera: MapCamera, target_camera: MapCamera, duration_seconds: f64) -> Self {
        Self {
            start_camera,
            target_camera,
            duration_seconds,
            path_curve: CameraPathCurve::MercatorSmoothstep,
        }
    }

    /// The animation's total duration in seconds; zero for animations that
    /// resolve instantly (coincident endpoints or an explicit zero/negative
    /// duration).
    pub fn duration_seconds(&self) -> f64 {
        self.duration_seconds
    }

    /// The camera pose after `elapsed_seconds` from the animation start.
    pub fn camera_at(&self, elapsed_seconds: f64) -> MapCamera {
        if self.duration_seconds <= 0.0 || elapsed_seconds >= self.duration_seconds {
            return self.target_camera;
        }
        match self.path_curve {
            CameraPathCurve::MercatorSmoothstep => interpolate_camera(
                &self.start_camera,
                &self.target_camera,
                elapsed_seconds / self.duration_seconds,
            ),
            CameraPathCurve::FlyToArc(path_pose) => path_pose.camera_at_progress(
                elapsed_seconds / self.duration_seconds,
                &self.start_camera,
                &self.target_camera,
            ),
        }
    }
}

/// The camera that frames a bounding box inside the viewport's inset region.
///
/// Bounds fitting is top-down and north-up by construction (pitch and bearing
/// zero), matching every leading SDK: perspective fitting is ill-defined and
/// would double-fit against the frustum clip. The zoom is the largest level
/// whose ground scale keeps the whole box inside the inset area, clamped to
/// `maximum_zoom_level`; the center is the box's normalized-world midpoint,
/// shifted so its ground point lands on the inset region's center under
/// asymmetric insets. The whole-box-inside-the-inset-area guarantee holds
/// only for the unclamped fit: when the zoom is clamped (by
/// `maximum_zoom_level`, or by the minimum zoom for boxes too large for the
/// world), the box overflows the inset region and the center-anchor
/// guarantee is the one that survives.
///
/// # Parameters
/// - `bounding_box` — the geographic box to frame.
/// - `viewport` — the viewport to frame it in.
/// - `edge_insets` — screen regions kept clear of map content.
/// - `maximum_zoom_level` — the zoom ceiling for the fit.
///
/// # Errors
/// Returns a description of the unusable input when the insets or the maximum
/// zoom are not finite, when the insets are negative or leave no inset area
/// (`left + right ≥ viewport width` and likewise vertical), or when the box
/// spans no positive world area (non-finite or reversed corners).
pub fn fit_bounds(
    bounding_box: &BoundingBox,
    viewport: &ViewportPixels,
    edge_insets: EdgeInsetsPixels,
    maximum_zoom_level: f64,
) -> Result<MapCamera, String> {
    // An inset must be finite and non-negative; the finite check refuses
    // `NaN` inputs that would otherwise produce a silent all-`NaN` camera.
    let insets_are_valid = [
        edge_insets.left,
        edge_insets.top,
        edge_insets.right,
        edge_insets.bottom,
    ]
    .iter()
    .all(|inset_pixels| inset_pixels.is_finite() && *inset_pixels >= 0.0);
    if !insets_are_valid {
        return Err("insets must be finite and non-negative".to_string());
    }
    if !maximum_zoom_level.is_finite() {
        return Err("the maximum zoom level must be finite".to_string());
    }
    let inset_available_width = viewport.width_pixels - edge_insets.left - edge_insets.right;
    let inset_available_height = viewport.height_pixels - edge_insets.top - edge_insets.bottom;
    if inset_available_width <= 0.0 || inset_available_height <= 0.0 {
        return Err(format!(
            "insets {edge_insets:?} leave no inset area in a {}x{} viewport",
            viewport.width_pixels, viewport.height_pixels
        ));
    }

    // A box never spans the antimeridian, so the normalized-world corners
    // span the box directly with no wrap to reconcile. World y grows
    // southward, so the south-west corner holds the larger y and the height
    // span takes the corner difference in the southward direction.
    let south_west_world = coordinate_to_normalized_world(&bounding_box.south_west);
    let north_east_world = coordinate_to_normalized_world(&bounding_box.north_east);
    let world_width = north_east_world[0] - south_west_world[0];
    let south_world_y = south_west_world[1];
    let north_world_y = north_east_world[1];
    let world_height = south_world_y - north_world_y;
    if !world_width.is_finite()
        || !world_height.is_finite()
        || world_width <= 0.0
        || world_height <= 0.0
    {
        return Err(format!(
            "the bounding box must span a positive, finite world area; got \
             width {world_width} and height {world_height} (reversed or \
             non-finite corners?)"
        ));
    }
    // The binding axis decides the scale: the box must fit on both.
    let fitting_scale =
        (inset_available_width / world_width).min(inset_available_height / world_height);
    // Ground magnification at the anchored camera equals the zoom scale
    // (`zoom_scale_pixels`, pixels per normalized world unit).
    let unclamped_zoom = (fitting_scale / MERCATOR_TILE_SIZE_PIXELS).log2();
    let effective_maximum_zoom = maximum_zoom_level.clamp(MINIMUM_ZOOM_LEVEL, MAXIMUM_ZOOM_LEVEL);
    let fitted_zoom = unclamped_zoom.clamp(MINIMUM_ZOOM_LEVEL, effective_maximum_zoom);

    let world_center = [
        (south_west_world[0] + north_east_world[0]) * 0.5,
        (south_world_y + north_world_y) * 0.5,
    ];
    // An asymmetric inset region's center sits displaced from the viewport
    // center; the anchored camera model puts the camera center's ground point
    // at the viewport center, so the camera shifts by half the inset
    // imbalance, converted to world units at the fitted scale.
    let fitted_zoom_scale = zoom_scale_pixels(fitted_zoom);
    let inset_center_shift_pixels = [
        (edge_insets.left - edge_insets.right) * 0.5,
        (edge_insets.top - edge_insets.bottom) * 0.5,
    ];
    let camera_world = [
        world_center[0] - inset_center_shift_pixels[0] / fitted_zoom_scale,
        world_center[1] - inset_center_shift_pixels[1] / fitted_zoom_scale,
    ];
    Ok(MapCamera::new(
        normalized_world_to_coordinate(camera_world),
        fitted_zoom,
        0.0,
        0.0,
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        CameraAnimation, DEFAULT_FIELD_OF_VIEW_DEGREES, EdgeInsetsPixels, FlyToOptions,
        MAXIMUM_PITCH_DEGREES, MAXIMUM_ZOOM_LEVEL, MapCamera, ViewportPixels, fit_bounds,
        fly_to_animation, interpolate_camera,
    };
    use crate::projection::{
        coordinate_to_normalized_world, normalized_world_to_coordinate, zoom_scale_pixels,
    };
    use geospatial_types::{BoundingBox, GeographicCoordinate};
    use proptest::prelude::*;

    fn coordinate(latitude: f64, longitude: f64) -> GeographicCoordinate {
        GeographicCoordinate::from_degrees(latitude, longitude).unwrap()
    }

    fn viewport() -> ViewportPixels {
        ViewportPixels::new(1024.0, 768.0).unwrap()
    }

    const CENTER_PIXEL_TOLERANCE: f64 = 1e-2;
    const MAGNITUDE_TOLERANCE_FRACTION: f64 = 0.05;
    const SCREEN_PIXEL_TOLERANCE: f64 = 2e-3;
    const COORDINATE_TOLERANCE_DEGREES: f64 = 1e-6;

    /// Rounding headroom for fitted corners hitting an inset edge.
    const FIT_BOUNDS_CORNER_TOLERANCE_PIXELS: f64 = 0.5;

    /// Float headroom for the fly-to arc's zoom staying inside the clamp and
    /// the endpoints' zoom levels.
    const FLY_TO_ARC_ENDPOINT_TOLERANCE: f64 = 1e-9;
    /// How far off an equal-zoom symmetric arc's midpoint center fraction the
    /// hyperbolic path may sit from exactly one half.
    const FLY_TO_MIDPOINT_FRACTION_TOLERANCE: f64 = 0.01;
    /// Animation sampling resolution shared by the fly-to tests.
    const FLY_TO_SAMPLE_COUNT: u64 = 8;
    /// Which sampled fly-to step exercises the arc's midpoint.
    const FLY_TO_MIDPOINT_SAMPLE: u64 = 4;

    #[test]
    fn camera_construction_clamps_and_wraps() {
        let camera = MapCamera::new(coordinate(47.0, 8.5), 90.0, -10.0, 200.0);
        assert_eq!(camera.zoom_level, 24.0);
        assert_eq!(camera.bearing_degrees, 350.0);
        assert_eq!(camera.pitch_degrees, MAXIMUM_PITCH_DEGREES);
        let bottom = MapCamera::new(coordinate(47.0, 8.5), -3.0, -700.0, -5.0);
        assert_eq!(bottom.zoom_level, 0.0);
        assert_eq!(bottom.bearing_degrees, 20.0);
        assert_eq!(bottom.pitch_degrees, 0.0);
    }

    #[test]
    fn center_projects_to_the_viewport_center() {
        let camera = MapCamera::new(coordinate(38.5766, -121.4944), 12.0, 33.0, 40.0);
        let screen = camera
            .project_to_screen(&camera.center, &viewport(), DEFAULT_FIELD_OF_VIEW_DEGREES)
            .unwrap();
        // World coordinates reach ~1e6 pixels here; the last few mantissa bits
        // of the matrix multiply permit sub-milli-pixel drift.
        assert!((screen[0] - 512.0).abs() < CENTER_PIXEL_TOLERANCE);
        assert!((screen[1] - 384.0).abs() < CENTER_PIXEL_TOLERANCE);
    }

    #[test]
    fn top_down_view_maps_east_right_and_north_up() {
        let camera = MapCamera::new(coordinate(10.0, 10.0), 10.0, 0.0, 0.0);
        let viewport = viewport();
        let field_of_view = DEFAULT_FIELD_OF_VIEW_DEGREES;
        // 0.05 degrees ≈ 145 screen pixels at zoom 10 for this viewport.
        let east = coordinate(10.0, 10.05);
        let north = coordinate(10.05, 10.0);
        let east_screen = camera
            .project_to_screen(&east, &viewport, field_of_view)
            .unwrap();
        let north_screen = camera
            .project_to_screen(&north, &viewport, field_of_view)
            .unwrap();
        assert!(east_screen[0] > 512.0, "east must render right of center");
        assert!(north_screen[1] < 384.0, "north must render above center");
        // Near the center the magnification matches the zoom scale up to the
        // slight perspective falloff; within 5% is enough here.
        let expected_pixels = zoom_scale_pixels(10.0) * (0.05 / 360.0);
        assert!(
            (east_screen[0] - 512.0 - expected_pixels).abs()
                < expected_pixels * MAGNITUDE_TOLERANCE_FRACTION
        );
        assert!(
            (384.0 - north_screen[1] - expected_pixels).abs()
                < expected_pixels * MAGNITUDE_TOLERANCE_FRACTION
        );
    }

    #[test]
    fn project_then_unproject_round_trips_known_camera() {
        let camera = MapCamera::new(coordinate(38.5766, -121.4944), 14.0, 25.0, 30.0);
        let viewport = viewport();
        let field_of_view = 45.0;
        let target = coordinate(38.5847, -121.4811);
        let screen = camera
            .project_to_screen(&target, &viewport, field_of_view)
            .unwrap();
        let ground = camera
            .screen_to_ground_coordinate(screen, &viewport, field_of_view)
            .unwrap();
        assert!((ground.latitude - target.latitude).abs() < 1e-6);
        assert!((ground.longitude - target.longitude).abs() < 1e-6);
    }

    #[test]
    fn above_the_horizon_has_no_ground() {
        // At maximum pitch the top of the viewport looks past the horizon
        // (the bottom edge tilts back down toward nearer ground).
        let camera = MapCamera::new(coordinate(30.0, 20.0), 10.0, 0.0, MAXIMUM_PITCH_DEGREES);
        let viewport = viewport();
        let ground = camera.screen_to_ground_coordinate(
            [0.0, 0.0],
            &viewport,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
        );
        assert!(ground.is_none());
    }

    #[test]
    fn animation_takes_the_shortest_bearing_arc() {
        let start = MapCamera::new(coordinate(48.0, 11.0), 10.0, 350.0, 0.0);
        let target = MapCamera::new(coordinate(49.0, 12.0), 12.0, 10.0, 30.0);
        let mid = interpolate_camera(&start, &target, 0.5);
        assert_eq!(mid.bearing_degrees, 0.0); // 350 -> 10 wraps through 0
        let midpoint_zoom = interpolate_camera(&start, &target, 0.5).zoom_level;
        assert!((midpoint_zoom - 11.0).abs() < 1e-9);
        let animation = CameraAnimation::new(start, target, 2.0);
        assert_eq!(animation.camera_at(0.0), start);
        assert_eq!(animation.camera_at(5.0), target);
    }

    fn fly_to_viewport() -> ViewportPixels {
        ViewportPixels::new(640.0, 480.0).unwrap()
    }

    /// Fly-to flight path between two equal zoom levels one degree of
    /// latitude apart at zoom 14, long enough for the arc's zoom-out dip.
    fn flown_test_flight_zoom_dip_animation() -> CameraAnimation {
        let start_camera = MapCamera::new(coordinate(47.0, 8.0), 14.0, 0.0, 0.0);
        let target_camera = MapCamera::new(coordinate(46.0, 8.0), 14.0, 0.0, 0.0);
        fly_to_animation(
            &start_camera,
            &target_camera,
            &fly_to_viewport(),
            FlyToOptions::default(),
        )
        .unwrap()
    }

    #[test]
    fn fly_to_animation_hits_both_endpoints_exactly() {
        let start_camera = MapCamera::new(coordinate(38.5766, -121.4944), 14.0, 22.0, 30.0);
        let target_camera = MapCamera::new(coordinate(37.7749, -122.4194), 11.0, 350.0, 45.0);
        let animation = fly_to_animation(
            &start_camera,
            &target_camera,
            &fly_to_viewport(),
            FlyToOptions::default(),
        )
        .unwrap();
        let duration_seconds = animation.duration_seconds();
        assert!(duration_seconds.is_finite() && duration_seconds > 0.0);
        assert_eq!(animation.camera_at(0.0), start_camera);
        assert_eq!(animation.camera_at(duration_seconds), target_camera);
        assert_eq!(animation.camera_at(duration_seconds * 2.0), target_camera);
    }

    #[test]
    fn fly_to_animation_zooms_out_below_the_endpoints_on_a_long_pan() {
        let animation = flown_test_flight_zoom_dip_animation();
        let duration_seconds = animation.duration_seconds();
        // The arc's zoom dips below both equal endpoint zoom levels and
        // returns to them on arrival.
        let dipped_zoom = animation.camera_at(duration_seconds * 0.5).zoom_level;
        assert!(dipped_zoom < 14.0);
        assert!((animation.camera_at(duration_seconds).zoom_level - 14.0).abs() < 1e-9);
    }

    #[test]
    fn fly_to_animation_center_travels_the_shortest_wrapped_path() {
        let start_camera = MapCamera::new(coordinate(30.0, 30.0), 10.0, 0.0, 0.0);
        let target_camera = MapCamera::new(coordinate(35.0, 32.0), 10.0, 0.0, 0.0);
        let animation = fly_to_animation(
            &start_camera,
            &target_camera,
            &fly_to_viewport(),
            FlyToOptions::default(),
        )
        .unwrap();
        let duration_seconds = animation.duration_seconds();
        let start_normalized = coordinate_to_normalized_world(&start_camera.center);
        let target_normalized = coordinate_to_normalized_world(&target_camera.center);
        let total_delta = [
            target_normalized[0] - start_normalized[0],
            target_normalized[1] - start_normalized[1],
        ];
        let mut previous_fraction = 0.0;
        for sample in 1..=FLY_TO_SAMPLE_COUNT {
            let pose = animation.camera_at(duration_seconds * sample as f64 / 8.0);
            let pose_normalized = coordinate_to_normalized_world(&pose.center);
            let traveled_fraction = (pose_normalized[0] - start_normalized[0]) / total_delta[0];
            // The traveled fraction rises monotonically toward the target
            // without overshooting it; at eased progress one half the
            // symmetric equal-zoom arc sits at the path's midpoint.
            assert!(traveled_fraction > previous_fraction);
            assert!((0.0..=1.0).contains(&traveled_fraction));
            if sample == FLY_TO_MIDPOINT_SAMPLE {
                assert!((traveled_fraction - 0.5).abs() < FLY_TO_MIDPOINT_FRACTION_TOLERANCE);
            }
            previous_fraction = traveled_fraction;
        }
    }

    #[test]
    fn fly_to_animation_zooms_monotonically_without_travel() {
        let center_camera = MapCamera::new(coordinate(38.5766, -121.4944), 4.0, 0.0, 0.0);
        let target_camera = MapCamera::new(coordinate(38.5766, -121.4944), 12.0, 0.0, 0.0);
        let animation = fly_to_animation(
            &center_camera,
            &target_camera,
            &fly_to_viewport(),
            FlyToOptions::default(),
        )
        .unwrap();
        let duration_seconds = animation.duration_seconds();
        let mut previous_zoom = center_camera.zoom_level;
        for sample in 1..=FLY_TO_SAMPLE_COUNT {
            let pose = animation.camera_at(duration_seconds * sample as f64 / 8.0);
            assert!(pose.zoom_level > previous_zoom);
            assert_eq!(pose.center, center_camera.center);
            previous_zoom = pose.zoom_level;
        }
        assert_eq!(animation.camera_at(duration_seconds), target_camera);
    }

    #[test]
    fn fly_to_animation_of_an_identical_camera_resolves_instantly() {
        let center_camera = MapCamera::new(coordinate(38.5766, -121.4944), 12.0, 10.0, 20.0);
        let animation = fly_to_animation(
            &center_camera,
            &center_camera,
            &fly_to_viewport(),
            FlyToOptions::default(),
        )
        .unwrap();
        assert_eq!(animation.duration_seconds(), 0.0);
        assert_eq!(animation.camera_at(0.0), center_camera);
        assert_eq!(animation.camera_at(1.0), center_camera);
    }

    #[test]
    fn fly_to_animation_rejects_unusable_input() {
        let center_camera = MapCamera::new(coordinate(47.0, 8.0), 10.0, 0.0, 0.0);
        let non_finite_zoom_camera = MapCamera {
            zoom_level: f64::NAN,
            ..center_camera
        };
        let non_finite_viewport = ViewportPixels {
            width_pixels: f64::NAN,
            height_pixels: 100.0,
        };
        let animation_with_non_finite_zoom = fly_to_animation(
            &center_camera,
            &non_finite_zoom_camera,
            &fly_to_viewport(),
            FlyToOptions::default(),
        );
        let animation_with_non_finite_viewport = fly_to_animation(
            &center_camera,
            &center_camera,
            &non_finite_viewport,
            FlyToOptions::default(),
        );
        let animation_with_zero_curve = fly_to_animation(
            &center_camera,
            &center_camera,
            &fly_to_viewport(),
            FlyToOptions {
                zoom_out_curve: 0.0,
                ..FlyToOptions::default()
            },
        );
        let animation_with_zero_speed = fly_to_animation(
            &center_camera,
            &center_camera,
            &fly_to_viewport(),
            FlyToOptions {
                travel_speed: 0.0,
                ..FlyToOptions::default()
            },
        );
        let animation_with_non_finite_duration = fly_to_animation(
            &center_camera,
            &center_camera,
            &fly_to_viewport(),
            FlyToOptions {
                duration_seconds: Some(f64::NAN),
                ..FlyToOptions::default()
            },
        );
        for unusable_animation in [
            animation_with_non_finite_zoom,
            animation_with_non_finite_viewport,
            animation_with_zero_curve,
            animation_with_zero_speed,
            animation_with_non_finite_duration,
        ] {
            assert!(unusable_animation.is_err());
        }
    }

    proptest! {
        /// Every reachable fly arc stays a usable camera: poses are finite,
        /// the zoom never leaves the clamped range nor rises above the
        /// endpoints' zooms, and the center arrives at the (clamped) target.
        #[test]
        fn fly_to_animation_poses_stay_a_usable_camera_over_random_flights(
            start_latitude in -80.0..80.0,
            start_longitude in -180.0..180.0,
            start_zoom in 0.0..24.0,
            target_latitude in -80.0..80.0,
            target_longitude in -180.0..180.0,
            target_zoom in 0.0..24.0,
            viewport_extent_pixels in 128.0..2048.0,
        ) {
            let start_camera = MapCamera::new(
                coordinate(start_latitude, start_longitude), start_zoom, 0.0, 0.0);
            let target_camera = MapCamera::new(
                coordinate(target_latitude, target_longitude), target_zoom, 0.0, 0.0);
            let flight_viewport = ViewportPixels::new(
                viewport_extent_pixels, viewport_extent_pixels).unwrap();
            let animation = fly_to_animation(
                &start_camera, &target_camera, &flight_viewport, FlyToOptions::default())
                .unwrap();
            let duration_seconds = animation.duration_seconds();
            prop_assert!(duration_seconds.is_finite());
            let endpoint_zoom = start_camera.zoom_level.max(target_camera.zoom_level);
            for sample in 0..=FLY_TO_SAMPLE_COUNT {
                let pose = animation.camera_at(duration_seconds * sample as f64 / 8.0);
                prop_assert!(pose.zoom_level.is_finite());
                prop_assert!(pose.zoom_level >= 0.0);
                prop_assert!(
                    pose.zoom_level <= endpoint_zoom + FLY_TO_ARC_ENDPOINT_TOLERANCE,
                    "pose zoom {} exceeded the endpoints' zoom {}",
                    pose.zoom_level, endpoint_zoom
                );
                // The eased arc endpoints resolve verbatim; an identical
                // start/target flight never travels.
                if sample == 0 {
                    prop_assert_eq!(pose.center, start_camera.center);
                }
                if sample == FLY_TO_SAMPLE_COUNT {
                    prop_assert_eq!(pose.center, target_camera.center);
                }
            }
        }
    }

    fn bounding_box(
        south_west: GeographicCoordinate,
        north_east: GeographicCoordinate,
    ) -> BoundingBox {
        BoundingBox::from_corners(south_west, north_east).unwrap()
    }

    #[test]
    fn fit_bounds_frames_the_box_inside_the_symmetric_inset_region() {
        let california_box = bounding_box(coordinate(32.5, -124.4), coordinate(42.0, -114.1));
        let viewport = viewport();
        let insets = EdgeInsetsPixels::symmetric(100.0, 50.0);
        let camera = fit_bounds(&california_box, &viewport, insets, f64::MAX).unwrap();
        assert_eq!(camera.bearing_degrees, 0.0);
        assert_eq!(camera.pitch_degrees, 0.0);
        // The projected corners land inside the inset region, with rounding
        // headroom; an unclamped fit must touch every inset edge exactly, so
        // also assert it truly fills the area in at least one axis.
        let field_of_view = DEFAULT_FIELD_OF_VIEW_DEGREES;
        let mut touches_left = false;
        let mut touches_right = false;
        let mut touches_top = false;
        let mut touches_bottom = false;
        for box_corner in [california_box.south_west, california_box.north_east] {
            let corner_screen = camera
                .project_to_screen(&box_corner, &viewport, field_of_view)
                .expect("a fitted top-down box corner is always in front of the camera");
            assert!(
                corner_screen[0] >= insets.left - FIT_BOUNDS_CORNER_TOLERANCE_PIXELS,
                "corner {box_corner:?} at x {} pierces the left inset",
                corner_screen[0]
            );
            assert!(
                corner_screen[0]
                    <= viewport.width_pixels - insets.right + FIT_BOUNDS_CORNER_TOLERANCE_PIXELS,
                "corner {box_corner:?} at x {} pierces the right inset",
                corner_screen[0]
            );
            assert!(
                corner_screen[1] >= insets.top - FIT_BOUNDS_CORNER_TOLERANCE_PIXELS,
                "corner {box_corner:?} at y {} pierces the top inset",
                corner_screen[1]
            );
            assert!(
                corner_screen[1]
                    <= viewport.height_pixels - insets.bottom + FIT_BOUNDS_CORNER_TOLERANCE_PIXELS,
                "corner {box_corner:?} at y {} pierces the bottom inset",
                corner_screen[1]
            );
            touches_left |= corner_screen[0] <= insets.left + FIT_BOUNDS_CORNER_TOLERANCE_PIXELS;
            touches_right |= corner_screen[0]
                >= viewport.width_pixels - insets.right - FIT_BOUNDS_CORNER_TOLERANCE_PIXELS;
            touches_top |= corner_screen[1] <= insets.top + FIT_BOUNDS_CORNER_TOLERANCE_PIXELS;
            touches_bottom |= corner_screen[1]
                >= viewport.height_pixels - insets.bottom - FIT_BOUNDS_CORNER_TOLERANCE_PIXELS;
        }
        // The binding axis fits exactly between its insets; the slack axis
        // sits strictly inside.
        assert!((touches_left && touches_right) || (touches_top && touches_bottom));
    }

    #[test]
    fn fit_bounds_anchors_the_box_on_the_asymmetric_inset_center() {
        // Inset far harder on the left: the box center must land at the
        // inset region's center, i.e. right of the viewport center.
        let california_box = bounding_box(coordinate(32.5, -124.4), coordinate(42.0, -114.1));
        let viewport = viewport();
        let insets = EdgeInsetsPixels {
            left: 300.0,
            top: 40.0,
            right: 40.0,
            bottom: 40.0,
        };
        let camera = fit_bounds(&california_box, &viewport, insets, f64::MAX).unwrap();
        // A country-scale box spans enough latitude that its mercator-normalized
        // center differs from the arithmetic latitude mean; the anchor guarantee
        // is on the normalized-world center `fit_bounds` itself uses.
        let box_center_coordinate = normalized_world_to_coordinate([
            (coordinate_to_normalized_world(&california_box.south_west)[0]
                + coordinate_to_normalized_world(&california_box.north_east)[0])
                * 0.5,
            (coordinate_to_normalized_world(&california_box.south_west)[1]
                + coordinate_to_normalized_world(&california_box.north_east)[1])
                * 0.5,
        ]);
        let box_center_screen = camera
            .project_to_screen(
                &box_center_coordinate,
                &viewport,
                DEFAULT_FIELD_OF_VIEW_DEGREES,
            )
            .unwrap();
        let inset_center_x =
            insets.left + (viewport.width_pixels - insets.left - insets.right) * 0.5;
        let inset_center_y =
            insets.top + (viewport.height_pixels - insets.top - insets.bottom) * 0.5;
        assert!(
            (box_center_screen[0] - inset_center_x).abs() < FIT_BOUNDS_CORNER_TOLERANCE_PIXELS,
            "box center at x {} must sit on the inset center {inset_center_x}",
            box_center_screen[0]
        );
        assert!(
            (box_center_screen[1] - inset_center_y).abs() < FIT_BOUNDS_CORNER_TOLERANCE_PIXELS,
            "box center at y {} must sit on the inset center {inset_center_y}",
            box_center_screen[1]
        );
    }

    #[test]
    fn fit_bounds_clamps_the_zoom_to_the_maximum() {
        // A country-scale box with a maximal zoom clamp lands on the clamp.
        let switzerland_box = bounding_box(coordinate(45.8, 5.9), coordinate(47.9, 10.5));
        let camera =
            fit_bounds(&switzerland_box, &viewport(), EdgeInsetsPixels::zero(), 3.0).unwrap();
        assert_eq!(camera.zoom_level, 3.0);
    }

    #[test]
    fn fit_bounds_rejects_negative_and_exhaustive_insets() {
        let california_box = bounding_box(coordinate(32.5, -124.4), coordinate(42.0, -114.1));
        let viewport = viewport();
        let negative = EdgeInsetsPixels {
            left: -1.0,
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
        };
        assert!(fit_bounds(&california_box, &viewport, negative, f64::MAX).is_err());
        let exhaustive =
            EdgeInsetsPixels::symmetric(viewport.width_pixels * 0.5, viewport.height_pixels * 0.5);
        assert!(fit_bounds(&california_box, &viewport, exhaustive, f64::MAX).is_err());
    }

    #[test]
    fn fit_bounds_rejects_non_finite_zoom_and_inset_input() {
        let california_box = bounding_box(coordinate(32.5, -124.4), coordinate(42.0, -114.1));
        let viewport = viewport();
        let nan_left_insets = EdgeInsetsPixels {
            left: f64::NAN,
            ..EdgeInsetsPixels::zero()
        };
        assert!(
            fit_bounds(&california_box, &viewport, nan_left_insets, f64::MAX).is_err(),
            "an NaN inset must be refused, not produce an NaN camera"
        );
        assert!(
            fit_bounds(
                &california_box,
                &viewport,
                EdgeInsetsPixels::zero(),
                f64::NAN
            )
            .is_err()
        );
        assert!(
            fit_bounds(
                &california_box,
                &viewport,
                EdgeInsetsPixels::zero(),
                f64::INFINITY
            )
            .is_err()
        );
    }

    #[test]
    fn fit_bounds_rejects_reversed_and_empty_boxes() {
        // The public fields accept an inverted box even though
        // `BoundingBox::from_corners` rejects one; fitting must still refuse,
        // not silently produce an all-NaN camera.
        let reversed_box = BoundingBox {
            south_west: coordinate(42.0, -114.1),
            north_east: coordinate(32.5, -124.4),
        };
        assert!(
            fit_bounds(
                &reversed_box,
                &viewport(),
                EdgeInsetsPixels::zero(),
                f64::MAX
            )
            .is_err()
        );
        let empty_box = BoundingBox {
            south_west: coordinate(38.5766, -121.4944),
            north_east: coordinate(38.5766, -121.4944),
        };
        assert!(
            fit_bounds(&empty_box, &viewport(), EdgeInsetsPixels::zero(), f64::MAX).is_err(),
            "a box without area has no meaningful fit"
        );
    }

    #[test]
    fn fit_bounds_clamped_still_anchors_the_box_on_the_asymmetric_inset_center() {
        // With the zoom clamped the box overflows the inset region, but the
        // center anchor is the guarantee that survives.
        let switzerland_box = bounding_box(coordinate(45.8, 5.9), coordinate(47.9, 10.5));
        let viewport = viewport();
        let insets = EdgeInsetsPixels {
            left: 300.0,
            top: 40.0,
            right: 40.0,
            bottom: 40.0,
        };
        let camera = fit_bounds(&switzerland_box, &viewport, insets, 3.0).unwrap();
        assert_eq!(camera.zoom_level, 3.0);
        let box_center_coordinate = coordinate(
            (switzerland_box.south_west.latitude + switzerland_box.north_east.latitude) * 0.5,
            (switzerland_box.south_west.longitude + switzerland_box.north_east.longitude) * 0.5,
        );
        let box_center_screen = camera
            .project_to_screen(
                &box_center_coordinate,
                &viewport,
                DEFAULT_FIELD_OF_VIEW_DEGREES,
            )
            .expect("a clamped top-down fit keeps the box center projectable");
        let inset_center_x =
            insets.left + (viewport.width_pixels - insets.left - insets.right) * 0.5;
        let inset_center_y =
            insets.top + (viewport.height_pixels - insets.top - insets.bottom) * 0.5;
        assert!(
            (box_center_screen[0] - inset_center_x).abs() < FIT_BOUNDS_CORNER_TOLERANCE_PIXELS,
            "box center at x {} must sit on the inset center {inset_center_x}",
            box_center_screen[0]
        );
        assert!(
            (box_center_screen[1] - inset_center_y).abs() < FIT_BOUNDS_CORNER_TOLERANCE_PIXELS,
            "box center at y {} must sit on the inset center {inset_center_y}",
            box_center_screen[1]
        );
    }

    proptest! {
        #[test]
        fn screen_round_trips_through_the_ground_plane(
            // |latitude| ≤ 70 and zoom ≥ 3 keep the whole top-down viewport
            // inside the Mercator map square; beyond the square the ground
            // plane exists but coordinates cannot represent it.
            center_latitude in -70.0f64..70.0,
            center_longitude in -179.0f64..179.0,
            zoom_level in 3.0f64..18.0,
            bearing_degrees in -180.0f64..180.0,
            pitch_degrees in 0.0f64..55.0,
            screen_x in 0.0f64..1024.0,
            screen_y in 10.0f64..758.0,
        ) {
            let camera = MapCamera::new(
                coordinate(center_latitude, center_longitude),
                zoom_level, bearing_degrees, pitch_degrees,
            );
            let viewport = viewport();
            let field_of_view = 45.0;
            if let Some(ground) = camera.screen_to_ground_coordinate(
                [screen_x, screen_y], &viewport, field_of_view,
            ) {
                let rescreened = camera
                    .project_to_screen(&ground, &viewport, field_of_view)
                    .unwrap();
                prop_assert!((rescreened[0] - screen_x).abs() < SCREEN_PIXEL_TOLERANCE, "x drift {rescreened:?} vs {screen_x}");
                prop_assert!((rescreened[1] - screen_y).abs() < SCREEN_PIXEL_TOLERANCE, "y drift {rescreened:?} vs {screen_y}");
            }
        }

        #[test]
        fn central_points_project_and_unproject_losslessly(
            // Pitch ≥ 55° pushes near-horizon points past the far clip and
            // zoom ≤ 7 pushes the pitched viewport footprint past the map
            // square edges, so the strategy stays inside the usable envelope.
            center_latitude in -70.0f64..70.0,
            center_longitude in -179.0f64..179.0,
            zoom_level in 8.0f64..18.0,
            bearing_degrees in -180.0f64..180.0,
            pitch_degrees in 0.0f64..55.0,
            latitude_offset in -0.25f64..0.25,
            longitude_offset in -0.25f64..0.25,
        ) {
            let latitude = (center_latitude + latitude_offset).clamp(-74.0, 74.0);
            let longitude = (center_longitude + longitude_offset).clamp(-179.5, 179.5);
            let camera = MapCamera::new(
                coordinate(center_latitude, center_longitude),
                zoom_level, bearing_degrees, pitch_degrees,
            );
            let viewport = viewport();
            let field_of_view = 45.0;
            let coordinate = coordinate(latitude, longitude);
            if let Some(screen) = camera.project_to_screen(&coordinate, &viewport, field_of_view) {
                // Off-viewport extensions still admit ground points just below
                // the horizon asymptote, which legitimately sit past the far
                // clip plane; `screen_to_ground_coordinate` returns `None` for
                // those, so the round trip asserts only where ground resolves.
                let within_extended_viewport = screen[0] > -viewport.width_pixels
                    && screen[0] < viewport.width_pixels * 2.0
                    && screen[1] > -viewport.height_pixels
                    && screen[1] < viewport.height_pixels * 2.0;
                if within_extended_viewport
                    && let Some(ground) =
                        camera.screen_to_ground_coordinate(screen, &viewport, field_of_view)
                {
                    prop_assert!((ground.latitude - latitude).abs() < COORDINATE_TOLERANCE_DEGREES);
                    prop_assert!((ground.longitude - longitude).abs() < COORDINATE_TOLERANCE_DEGREES);
                }
            }
        }
    }

    #[test]
    fn whole_world_top_down_view_lists_the_root_tile() {
        // A zoom-0 world spans exactly 512 pixels, so this viewport's top-down
        // footprint covers the whole map square and only the root tile exists.
        let camera = MapCamera::new(coordinate(38.5766, -121.4944), 0.0, 0.0, 0.0);
        let view = ViewportPixels::new(512.0, 512.0).unwrap();
        let visible_tiles = camera.visible_tile_keys(
            &view,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
            camera.tile_zoom_level(),
        );
        assert_eq!(
            visible_tiles,
            vec![crate::tile_key::TileKey::new(0, 0, 0).unwrap()]
        );
    }

    #[test]
    fn pitched_view_lists_tiles_north_of_the_center_and_none_far_south() {
        // Bearing 0 tilts the far edge of the footprint north of the anchor;
        // the near edge reaches only part of one tile south of it.
        let camera = MapCamera::new(coordinate(38.5766, -121.4944), 14.0, 0.0, 45.0);
        let view = viewport();
        let visible_tiles = camera.visible_tile_keys(
            &view,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
            camera.tile_zoom_level(),
        );
        let center_tile = crate::tile_key::TileKey::new(2662, 6286, 14).unwrap();
        assert!(
            visible_tiles.contains(&center_tile),
            "center tile missing from {visible_tiles:?}"
        );
        let minimum_row = visible_tiles.iter().map(|tile| tile.tile_y).min().unwrap();
        let maximum_row = visible_tiles.iter().map(|tile| tile.tile_y).max().unwrap();
        assert!(
            minimum_row < 6286,
            "no northern tiles listed: rows {minimum_row}..{maximum_row}"
        );
        assert!(
            maximum_row <= 6287,
            "footprint reaches far south of the near edge: rows {minimum_row}..{maximum_row}"
        );
        for tile in &visible_tiles {
            assert!(
                (2659..=2665).contains(&tile.tile_x),
                "footprint drifts laterally: {tile:?}"
            );
        }
    }

    proptest! {
        #[test]
        fn fit_bounds_keeps_every_box_corner_inside_the_inset_region(
            // Boxes spanning ≥ 2e-4 degrees of longitude always need a zoom
            // below the maximum, so the corner guarantee holds unclamped.
            base_latitude in -40.0f64..40.0,
            base_longitude in -170.0f64..170.0,
            box_width_degrees in 0.01f64..10.0,
            box_height_degrees in 0.01f64..10.0,
            inset_x in 10.0f64..150.0,
            inset_y in 10.0f64..150.0,
        ) {
            let camera = fit_bounds(
                &bounding_box(
                    coordinate(base_latitude, base_longitude),
                    coordinate(
                        (base_latitude + box_height_degrees).clamp(-70.0, 70.0),
                        base_longitude + box_width_degrees,
                    ),
                ),
                &viewport(),
                EdgeInsetsPixels::symmetric(inset_x, inset_y),
                MAXIMUM_ZOOM_LEVEL,
            )
            .unwrap();
            let field_of_view = DEFAULT_FIELD_OF_VIEW_DEGREES;
            let fitted_viewport = viewport();
            for box_corner_latitude in [base_latitude, base_latitude + box_height_degrees] {
                for box_corner_longitude in [base_longitude, base_longitude + box_width_degrees] {
                    let corner_screen = camera
                        .project_to_screen(
                            &coordinate(box_corner_latitude, box_corner_longitude),
                            &fitted_viewport,
                            field_of_view,
                        )
                        .unwrap();
                    let insets = EdgeInsetsPixels::symmetric(inset_x, inset_y);
                    prop_assert!(corner_screen[0] >= insets.left - FIT_BOUNDS_CORNER_TOLERANCE_PIXELS);
                    prop_assert!(corner_screen[0] <= fitted_viewport.width_pixels - insets.right + FIT_BOUNDS_CORNER_TOLERANCE_PIXELS);
                    prop_assert!(corner_screen[1] >= insets.top - FIT_BOUNDS_CORNER_TOLERANCE_PIXELS);
                    prop_assert!(corner_screen[1] <= fitted_viewport.height_pixels - insets.bottom + FIT_BOUNDS_CORNER_TOLERANCE_PIXELS);
                }
            }
        }
    }

    #[test]
    fn visible_tiles_wrap_across_the_antimeridian() {
        // Centered 1e-3 world copies west of the seam, the top-down footprint
        // crosses into the eastern world copy, whose tiles wrap to column 0.
        let camera = MapCamera::new(coordinate(0.0, 179.95), 8.0, 0.0, 0.0);
        let view = ViewportPixels::new(512.0, 512.0).unwrap();
        let visible_tiles = camera.visible_tile_keys(
            &view,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
            camera.tile_zoom_level(),
        );
        let listed_columns: std::collections::HashSet<u32> =
            visible_tiles.iter().map(|tile| tile.tile_x).collect();
        assert!(
            listed_columns.contains(&255) && listed_columns.contains(&0),
            "footprint at the seam must list both world copies: {visible_tiles:?}"
        );
        for tile in &visible_tiles {
            assert!((127..=128).contains(&tile.tile_y), "row drift: {tile:?}");
        }
    }

    proptest! {
        #[test]
        fn visible_tiles_cover_every_visible_ground_point(
            // The zoom ≥ 3 envelope keeps the top-down viewport inside the
            // Mercator map square; a successful unprojection guarantees the
            // ground point is inside the frustum on the ground plane.
            center_latitude in -70.0f64..70.0,
            center_longitude in -120.0f64..120.0,
            zoom_level in 3.0f64..11.0,
            bearing_degrees in -180.0f64..180.0,
            pitch_degrees in 0.0f64..55.0,
            screen_x in 1.0f64..1023.0,
            screen_y in 1.0f64..757.0,
        ) {
            let camera = MapCamera::new(
                coordinate(center_latitude, center_longitude),
                zoom_level, bearing_degrees, pitch_degrees,
            );
            let viewport = viewport();
            let field_of_view = 45.0;
            let tile_zoom_level = camera.tile_zoom_level();
            if let Some(ground) = camera.screen_to_ground_coordinate(
                [screen_x, screen_y], &viewport, field_of_view,
            ) {
                let visible_tiles = camera
                    .visible_tile_keys(&viewport, field_of_view, tile_zoom_level);
                let ground_tile = crate::projection::tile_key_for_coordinate(
                    &ground, tile_zoom_level,
                );
                prop_assert!(
                    visible_tiles.contains(&ground_tile),
                    "screen tile {ground_tile:?} visible but not listed in {:?}",
                    visible_tiles
                );
            }
        }
    }
}
