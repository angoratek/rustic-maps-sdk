//! GPU-free location-indicator assembly: the position fix drawn as a
//! screen-facing two-mode puck over its optional ground-projected accuracy
//! ring, in the same clip-space stream pattern the overlay layer uses (see
//! [`crate::overlay`]).
//!
//! The indicator is the third draw layer of a frame — the explicit draw
//! order is tiles, then overlays, then the indicator
//! ([`LOCATION_INDICATOR_TOP_LAYER_DRAW_ORDER`]) — so the fix's visuals
//! always paint over route lines and polygons without the overlay list
//! growing or reordering. The vertices reuse the
//! [`crate::overlay::ClipSpaceOverlayVertex`] type and draw through the
//! existing overlay solid-color pipeline (a shader/layout-identical third
//! pass), alpha-blended over what the first two layers drew.
//!
//! The puck has two modes:
//!
//! * **Tracking mode** ([`LocationIndicatorVisualKind::TrackingDot`], or a
//!   fix without a course): a soft dot — an accent core disc, a white ring
//!   around it, and a subtle dark halo under both for legibility on light
//!   and dark maps.
//! * **Navigation mode** ([`LocationIndicatorVisualKind::GuidedCourseArrow`]
//!   with a reported course): an up-course dart with a white outline and the
//!   accent fill inside the same white ring and halo, rotating with the
//!   course. Without a course the navigation form degrades to the tracking
//!   dot.
//!
//! Layer semantics:
//!
//! * **Screen anchoring.** The puck is a screen-space shape anchored at the
//!   fix's projected position — the fix's pixel stays fixed under zoom and
//!   pan while the layers beneath move, exactly like point markers do
//!   (center anchoring, "screen-facing"). At pitch this keeps the puck
//!   legible where a ground-projected dot would foreshorten to a sliver, and
//!   the dart rotates with the camera's bearing so a course heading reads as
//!   the same compass direction it reads on the ground.
//! * **Accuracy ring.** A ground-projected circle of the fix's horizontal
//!   accuracy — the honest reading of "this fix is this accurate" — drawn
//!   through the filled-polygon machinery, so it foreshortens into the
//!   ellipse the ground shows at pitch and splits at world-copy boundaries
//!   like every other ground shape. A fix more accurate than
//!   [`MINIMUM_ACCURACY_RING_DRAW_METERS`] draws no ring (a circle inside
//!   the puck itself carries no information), and an absurdly coarse fix
//!   clamps to [`MAXIMUM_ACCURACY_RING_DIAMETER_PIXELS`] on screen instead
//!   of flooding the viewport.
//! * **World copies.** The screen-facing shapes anchor once per visible
//!   world-copy window like the tile spans' fragments, so at the zoom < 1
//!   world-copy views the indicator draws in every copy that renders the
//!   fix; the ground ring splits across windows in the polygon machinery.
//! * **Behind the camera.** A screen-shape anchor whose homogeneous weight
//!   is not positive (the fix behind the camera plane) draws no puck — the
//!   screen-space shapes' documented skip — while the ground-projected ring
//!   still projects by genuine corners and leaves near-plane straddling to
//!   the GPU's frustum clip.

use std::f64::consts::TAU;

use glam::DVec4;

use crate::camera::{MapCamera, ViewportPixels};
use crate::overlay::{
    ClipSpaceOverlayVertex, FilledPolygonStyle, OverlayFrameProjection, WORLD_COPY_WINDOW_OFFSETS,
    clip_position_to_screen_pixels, filled_polygon_triangles, push_triangle,
    screen_pixels_to_clip_position,
};
use crate::projection::coordinate_to_normalized_world;
use geospatial_types::GeographicCoordinate;

/// The WGS-84 equatorial radius the accuracy ring's meters map through (the
/// camera's equatorial-circumference constant divided by 2π, so the two
/// radii cannot drift apart).
const EARTH_EQUATORIAL_RADIUS_METERS: f64 =
    crate::camera::EARTH_EQUATORIAL_CIRCUMFERENCE_METERS / TAU;

/// Default accent-core radius in screen pixels: about 11 px, small enough
/// not to hide the map beneath the puck and large enough to read at a
/// glance.
pub const DEFAULT_LOCATION_DISC_RADIUS_PIXELS: f64 = 11.0;

/// Default accent fill color: an opaque blue, used for the tracking core
/// disc and the navigation dart's fill.
pub const DEFAULT_LOCATION_DISC_RGBA: [f32; 4] = [0.13, 0.47, 0.87, 1.0];

/// Default ring color: opaque white, used for the ring around the puck and
/// for the navigation dart's outline.
pub const DEFAULT_LOCATION_RING_RGBA: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

/// Default halo color: near-black at low alpha, drawn under the puck so the
/// white ring reads on a light map as well as on a dark one.
pub const DEFAULT_LOCATION_HALO_RGBA: [f32; 4] = [0.03, 0.06, 0.10, 0.30];

/// Default accuracy-ring color: the accent blue with low alpha, so the
/// underlying tiles stay readable beneath the ring's fill.
pub const DEFAULT_ACCURACY_RING_RGBA: [f32; 4] = [0.13, 0.47, 0.87, 0.25];

/// Circle-fan resolution of the puck's discs and rings: 24 sides, so they
/// read round at every radius the style admits.
pub const LOCATION_DISC_FAN_SIDES: usize = 24;

/// Arc resolution of the accuracy ring's ground-space circle: 32 straight
/// sides keep the drawn ellipse sub-pixel at the clamped diameter.
pub const LOCATION_ACCURACY_RING_FAN_SIDES: usize = 32;

/// Width of the white ring around the accent core in screen pixels.
pub const LOCATION_PUCK_RING_WIDTH_PIXELS: f64 = 3.0;

/// Width of the dark halo outside the white ring in screen pixels.
pub const LOCATION_PUCK_HALO_WIDTH_PIXELS: f64 = 2.0;

/// Width of the navigation dart's white outline in screen pixels.
pub const LOCATION_NAVIGATION_DART_OUTLINE_WIDTH_PIXELS: f64 = 2.5;

/// How far the navigation dart's tip reaches, as a fraction of the accent
/// core radius. Below 1.0 so the dart sits inside the ring.
pub const LOCATION_NAVIGATION_DART_TIP_RADIUS_RATIO: f64 = 0.86;

/// Half the dart's rear width, as a fraction of the accent core radius.
pub const LOCATION_NAVIGATION_DART_BASE_HALF_WIDTH_RATIO: f64 = 0.58;

/// How far the dart's two rear corners sit down-course, as a fraction of the
/// accent core radius.
pub const LOCATION_NAVIGATION_DART_BASE_OFFSET_RATIO: f64 = 0.56;

/// How far the dart's tail notch sits down-course, as a fraction of the
/// accent core radius. Smaller than
/// [`LOCATION_NAVIGATION_DART_BASE_OFFSET_RATIO`], which gives the dart its
/// swept-back tail.
pub const LOCATION_NAVIGATION_DART_NOTCH_OFFSET_RATIO: f64 = 0.18;

/// Fixes more accurate than this radius (in meters) draw no accuracy ring:
/// the circle would sit entirely inside the default puck, carrying no
/// information.
pub const MINIMUM_ACCURACY_RING_DRAW_METERS: f64 = 10.0;

/// Screen-diameter cap of the accuracy ring: an absurdly coarse fix clamps
/// its ring to this diameter instead of flooding the viewport.
pub const MAXIMUM_ACCURACY_RING_DIAMETER_PIXELS: f64 = 512.0;

/// Explicit draw order of the location indicator layer: the top layer of a
/// frame's explicit three-layer order — tiles (0), overlays (1), the
/// indicator (2).
pub const LOCATION_INDICATOR_TOP_LAYER_DRAW_ORDER: u8 = 2;

/// A presentable position fix: the state half of one
/// [`LocationIndicatorLayer`].
#[derive(Debug, Clone, PartialEq)]
pub struct LocationIndicatorState {
    /// Fix position.
    pub coordinate: GeographicCoordinate,
    /// Course over ground in degrees clockwise from true north; `None` when
    /// the source does not report one (the layer then draws the tracking dot
    /// whatever the style's visual kind).
    pub course_heading_degrees: Option<f64>,
    /// Horizontal accuracy radius in meters; the accuracy ring renders when
    /// set, shown by the style, and at or above the draw threshold.
    pub horizontal_accuracy_meters: Option<f64>,
}

/// Which visual form a location indicator draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocationIndicatorVisualKind {
    /// Tracking mode: the soft dot — an accent core, a white ring and a dark
    /// halo — plus its accuracy ring when the fix carries usable accuracy.
    TrackingDot,
    /// Navigation mode: the course-directed dart inside the same ring and
    /// halo. The dart renders when the fix carries a course and degrades to
    /// the tracking dot without one.
    GuidedCourseArrow,
}

/// Style of the location indicator; `None` fields keep the documented
/// module defaults (the overlay style pattern).
#[derive(Debug, Clone, PartialEq)]
pub struct LocationIndicatorStyle {
    /// Which visual form the layer draws.
    pub visual_kind: LocationIndicatorVisualKind,
    /// Accent-core radius in screen pixels; the ring and halo are added
    /// outside it. `None` keeps the default.
    pub radius_pixels: Option<f64>,
    /// Accent fill color of the core disc and the navigation dart; `None`
    /// keeps the default.
    pub rgba_color: Option<[f32; 4]>,
    /// Ring color: the ring around the puck and the navigation dart's
    /// outline; `None` keeps the default white.
    pub ring_rgba_color: Option<[f32; 4]>,
    /// Halo color drawn under the puck; `None` keeps the default.
    pub halo_rgba_color: Option<[f32; 4]>,
    /// Whether the accuracy ring draws at all; `None` keeps the default
    /// `true`.
    pub show_accuracy_ring: Option<bool>,
}

impl LocationIndicatorStyle {
    /// The style with the documented module defaults everywhere the style's
    /// `None`s are (the platform binding's optional style fields map onto
    /// this directly).
    pub fn with_defaults(visual_kind: LocationIndicatorVisualKind) -> Self {
        Self {
            visual_kind,
            radius_pixels: None,
            rgba_color: None,
            ring_rgba_color: None,
            halo_rgba_color: None,
            show_accuracy_ring: None,
        }
    }

    /// The applied accent-core radius (the style's override or the module
    /// default).
    fn disc_radius_pixels(&self) -> f64 {
        self.radius_pixels
            .unwrap_or(DEFAULT_LOCATION_DISC_RADIUS_PIXELS)
    }

    /// The applied accent fill color (the style's override or the module
    /// default).
    fn accent_rgba(&self) -> [f32; 4] {
        self.rgba_color.unwrap_or(DEFAULT_LOCATION_DISC_RGBA)
    }

    /// The applied ring color (the style's override or the module default).
    fn ring_rgba(&self) -> [f32; 4] {
        self.ring_rgba_color.unwrap_or(DEFAULT_LOCATION_RING_RGBA)
    }

    /// The applied halo color (the style's override or the module default).
    fn halo_rgba(&self) -> [f32; 4] {
        self.halo_rgba_color.unwrap_or(DEFAULT_LOCATION_HALO_RGBA)
    }

    /// Whether the accuracy ring draws (the style's override or the
    /// default `true`).
    fn show_accuracy_ring(&self) -> bool {
        self.show_accuracy_ring.unwrap_or(true)
    }
}

/// The state and style of one location-indicator layer; assemble its
/// vertices with [`location_indicator_vertices`].
#[derive(Debug, Clone, PartialEq)]
pub struct LocationIndicatorLayer {
    /// The fix the layer draws.
    pub state: LocationIndicatorState,
    /// The style the layer draws with.
    pub style: LocationIndicatorStyle,
}

impl LocationIndicatorLayer {
    /// Builds the layer from a fix and a style.
    pub fn new(state: LocationIndicatorState, style: LocationIndicatorStyle) -> Self {
        Self { state, style }
    }
}

/// Assembles the location-indicator layer of one frame into the flat
/// clip-space triangle list the overlay pipeline draws. A `None` layer (the
/// cleared state) produces no vertices; a fix whose anchor projects behind
/// the camera produces no puck for any window, while its accuracy ring (a
/// ground-projected polygon) still assembles with genuine corners for the
/// GPU's frustum clip.
pub fn location_indicator_vertices(
    camera: &MapCamera,
    viewport: &ViewportPixels,
    field_of_view_degrees: f64,
    location_indicator_layer: Option<&LocationIndicatorLayer>,
) -> Vec<ClipSpaceOverlayVertex> {
    let Some(location_indicator_layer) = location_indicator_layer else {
        return Vec::new();
    };
    let frame_projection =
        OverlayFrameProjection::for_frame(camera, viewport, field_of_view_degrees);
    let normalized_world =
        coordinate_to_normalized_world(&location_indicator_layer.state.coordinate);
    let wrapped_center_x = frame_projection.wrapped_x(normalized_world[0]);
    let mut vertices = Vec::new();

    // The accuracy ring is a ground-projected circle, drawn first so the
    // screen-facing puck paints over it.
    fill_accuracy_ring_triangles(
        camera,
        &frame_projection,
        viewport,
        &location_indicator_layer.state,
        &location_indicator_layer.style,
        &mut vertices,
    );

    // The screen-facing shapes anchor once per visible world-copy window,
    // like the tile spans' fragments: the fix's wrapped x must lie inside
    // the window's span (the segments' shared half-open rule) before its
    // anchor projects in that copy.
    for window_offset in WORLD_COPY_WINDOW_OFFSETS {
        let window_offset_f64 = f64::from(window_offset);
        let [window_start_x, window_end_x] = frame_projection.window_x_range(window_offset_f64);
        if wrapped_center_x < window_start_x || wrapped_center_x >= window_end_x {
            continue;
        }
        let anchor_clip_position = frame_projection.ground_clip_position(
            window_offset_f64,
            wrapped_center_x,
            normalized_world[1],
        );
        if anchor_clip_position.w <= 0.0 {
            continue;
        }
        let style = &location_indicator_layer.style;
        let accent_radius_pixels = style.disc_radius_pixels();
        let ring_outer_radius_pixels = accent_radius_pixels + LOCATION_PUCK_RING_WIDTH_PIXELS;
        let halo_outer_radius_pixels = ring_outer_radius_pixels + LOCATION_PUCK_HALO_WIDTH_PIXELS;

        // Concentric order, outermost first: the halo, the white ring, then
        // the accent form inside the ring.
        screen_facing_disc_triangles(
            &anchor_clip_position,
            viewport,
            halo_outer_radius_pixels,
            style.halo_rgba(),
            &mut vertices,
        );
        screen_facing_ring_triangles(
            &anchor_clip_position,
            viewport,
            accent_radius_pixels,
            ring_outer_radius_pixels,
            style.ring_rgba(),
            &mut vertices,
        );
        let course_heading_degrees = location_indicator_layer.state.course_heading_degrees;
        match (style.visual_kind, course_heading_degrees) {
            (LocationIndicatorVisualKind::GuidedCourseArrow, Some(course_heading_degrees)) => {
                screen_facing_navigation_dart_triangles(
                    &anchor_clip_position,
                    viewport,
                    camera.bearing_degrees,
                    course_heading_degrees,
                    accent_radius_pixels,
                    style,
                    &mut vertices,
                );
            }
            _ => {
                screen_facing_disc_triangles(
                    &anchor_clip_position,
                    viewport,
                    accent_radius_pixels,
                    style.accent_rgba(),
                    &mut vertices,
                );
            }
        }
    }
    vertices
}

/// The screen-space direction of a course heading under a camera bearing: a
/// north-up camera shows north up (`[0, -1]` in y-down screen pixels), and
/// the heading rotates by the camera's bearing so the dart reads as the
/// same compass direction as the map beneath it.
fn course_screen_direction(course_heading_degrees: f64, camera_bearing_degrees: f64) -> [f64; 2] {
    let relative_radians = (course_heading_degrees - camera_bearing_degrees).to_radians();
    [relative_radians.sin(), -relative_radians.cos()]
}

/// Assembles the accuracy ring for the layer's fix when the style shows
/// rings and the accuracy clears the draw threshold; an over-large ring
/// clamps to the screen-diameter cap through the camera's ground meters per
/// screen pixel at the center.
fn fill_accuracy_ring_triangles(
    camera: &MapCamera,
    frame_projection: &OverlayFrameProjection,
    viewport: &ViewportPixels,
    state: &LocationIndicatorState,
    style: &LocationIndicatorStyle,
    vertices: &mut Vec<ClipSpaceOverlayVertex>,
) {
    if !style.show_accuracy_ring() {
        return;
    }
    let Some(horizontal_accuracy_meters) = state.horizontal_accuracy_meters else {
        return;
    };
    if !horizontal_accuracy_meters.is_finite()
        || horizontal_accuracy_meters < MINIMUM_ACCURACY_RING_DRAW_METERS
    {
        return;
    }
    // The screen-diameter clamp re-expressed in ground meters along the
    // fix's own parallel: the ring is ground-projected, so the screen cap
    // must apply to the ground circle the projection draws, and the flat-
    // centered meters-per-pixel is the shared honest approximation (see the
    // camera helper's contract text).
    let clamp_radius_meters =
        MAXIMUM_ACCURACY_RING_DIAMETER_PIXELS * 0.5 * camera.ground_meters_per_screen_pixel();
    let radius_meters = horizontal_accuracy_meters.min(clamp_radius_meters);
    filled_polygon_triangles(
        frame_projection,
        viewport,
        &accuracy_ring_coordinates(&state.coordinate, radius_meters),
        &[],
        FilledPolygonStyle {
            rgba_color: DEFAULT_ACCURACY_RING_RGBA,
            rgba_outline_color: None,
            antialias: true,
            translate_pixels: [0.0, 0.0],
            translate_anchor: rustic_maps_style::TranslateAnchor::Viewport,
        },
        vertices,
    );
}

/// A flat-earth approximation circle around the fix: the radius maps to
/// latitude and longitude offsets through the WGS-84 equatorial radius and
/// the fix's own parallel, which at the sub-kilometer accuracies the
/// indicator draws is exact to far below a pixel of the ring's own extent.
fn accuracy_ring_coordinates(
    fix_coordinate: &GeographicCoordinate,
    radius_meters: f64,
) -> Vec<GeographicCoordinate> {
    let latitude_radius_radians = radius_meters / EARTH_EQUATORIAL_RADIUS_METERS;
    let longitude_radius_radians =
        latitude_radius_radians / fix_coordinate.latitude.to_radians().cos();
    (0..LOCATION_ACCURACY_RING_FAN_SIDES)
        .map(|side_index| {
            let side_angle = side_index as f64 * TAU / LOCATION_ACCURACY_RING_FAN_SIDES as f64;
            GeographicCoordinate::from_degrees(
                fix_coordinate.latitude + (latitude_radius_radians * side_angle.sin()).to_degrees(),
                fix_coordinate.longitude
                    + (longitude_radius_radians * side_angle.cos()).to_degrees(),
            )
            .expect("a fix inside the Mercator latitudes always admits a one-radius ring")
        })
        .collect()
}

/// Draws one screen-facing disc: a [`LOCATION_DISC_FAN_SIDES`]-side triangle
/// fan centered on the anchor's screen position — the point-marker fan
/// pattern, screen-facing and center-anchored (see [`crate::overlay`]).
fn screen_facing_disc_triangles(
    anchor_clip_position: &DVec4,
    viewport: &ViewportPixels,
    radius_pixels: f64,
    rgba_color: [f32; 4],
    vertices: &mut Vec<ClipSpaceOverlayVertex>,
) {
    let center_screen = clip_position_to_screen_pixels(*anchor_clip_position, viewport);
    let center_corner =
        screen_pixels_to_clip_position(center_screen, *anchor_clip_position, viewport);
    for side_index in 0..LOCATION_DISC_FAN_SIDES {
        let start_angle = side_index as f64 * TAU / LOCATION_DISC_FAN_SIDES as f64;
        let end_angle = (side_index + 1) as f64 * TAU / LOCATION_DISC_FAN_SIDES as f64;
        let ring_corner_start = screen_pixels_to_clip_position(
            [
                center_screen[0] + radius_pixels * start_angle.cos(),
                center_screen[1] + radius_pixels * start_angle.sin(),
            ],
            *anchor_clip_position,
            viewport,
        );
        let ring_corner_end = screen_pixels_to_clip_position(
            [
                center_screen[0] + radius_pixels * end_angle.cos(),
                center_screen[1] + radius_pixels * end_angle.sin(),
            ],
            *anchor_clip_position,
            viewport,
        );
        push_triangle(
            vertices,
            &[center_corner, ring_corner_start, ring_corner_end],
            rgba_color,
        );
    }
}

/// Draws one screen-facing ring: a [`LOCATION_DISC_FAN_SIDES`]-segment
/// annulus between `inner_radius_pixels` and `outer_radius_pixels`, centered
/// on the anchor's screen position. Each segment is two triangles, so the
/// ring is a true annulus rather than a disc hiding the map inside it.
fn screen_facing_ring_triangles(
    anchor_clip_position: &DVec4,
    viewport: &ViewportPixels,
    inner_radius_pixels: f64,
    outer_radius_pixels: f64,
    rgba_color: [f32; 4],
    vertices: &mut Vec<ClipSpaceOverlayVertex>,
) {
    let center_screen = clip_position_to_screen_pixels(*anchor_clip_position, viewport);
    for side_index in 0..LOCATION_DISC_FAN_SIDES {
        let start_angle = side_index as f64 * TAU / LOCATION_DISC_FAN_SIDES as f64;
        let end_angle = (side_index + 1) as f64 * TAU / LOCATION_DISC_FAN_SIDES as f64;
        let inner_start = screen_pixels_to_clip_position(
            [
                center_screen[0] + inner_radius_pixels * start_angle.cos(),
                center_screen[1] + inner_radius_pixels * start_angle.sin(),
            ],
            *anchor_clip_position,
            viewport,
        );
        let inner_end = screen_pixels_to_clip_position(
            [
                center_screen[0] + inner_radius_pixels * end_angle.cos(),
                center_screen[1] + inner_radius_pixels * end_angle.sin(),
            ],
            *anchor_clip_position,
            viewport,
        );
        let outer_start = screen_pixels_to_clip_position(
            [
                center_screen[0] + outer_radius_pixels * start_angle.cos(),
                center_screen[1] + outer_radius_pixels * start_angle.sin(),
            ],
            *anchor_clip_position,
            viewport,
        );
        let outer_end = screen_pixels_to_clip_position(
            [
                center_screen[0] + outer_radius_pixels * end_angle.cos(),
                center_screen[1] + outer_radius_pixels * end_angle.sin(),
            ],
            *anchor_clip_position,
            viewport,
        );
        push_triangle(vertices, &[inner_start, outer_start, outer_end], rgba_color);
        push_triangle(vertices, &[inner_start, outer_end, inner_end], rgba_color);
    }
}

/// Draws the navigation dart: the white outline first, then the accent fill
/// on top, both swept back from an up-course tip and rotated with the
/// course. The outline is the same dart scaled outward by
/// [`LOCATION_NAVIGATION_DART_OUTLINE_WIDTH_PIXELS`], which keeps the visible
/// border even at the tip and along the swept tail.
fn screen_facing_navigation_dart_triangles(
    anchor_clip_position: &DVec4,
    viewport: &ViewportPixels,
    camera_bearing_degrees: f64,
    course_heading_degrees: f64,
    accent_radius_pixels: f64,
    style: &LocationIndicatorStyle,
    vertices: &mut Vec<ClipSpaceOverlayVertex>,
) {
    let center_screen = clip_position_to_screen_pixels(*anchor_clip_position, viewport);
    let forward = course_screen_direction(course_heading_degrees, camera_bearing_degrees);
    let outline_radius_pixels =
        accent_radius_pixels + LOCATION_NAVIGATION_DART_OUTLINE_WIDTH_PIXELS;
    let outline_corners = navigation_dart_corners(center_screen, forward, outline_radius_pixels);
    push_navigation_dart_triangles(
        anchor_clip_position,
        viewport,
        &outline_corners,
        style.ring_rgba(),
        vertices,
    );
    let fill_corners = navigation_dart_corners(center_screen, forward, accent_radius_pixels);
    push_navigation_dart_triangles(
        anchor_clip_position,
        viewport,
        &fill_corners,
        style.accent_rgba(),
        vertices,
    );
}

/// The four screen-space corners of the navigation dart, in traversal
/// order: tip, right rear corner, tail notch, left rear corner. `forward` is
/// the unit screen direction of the course; the dart's lateral axis is its
/// clockwise perpendicular.
fn navigation_dart_corners(
    center_screen: [f64; 2],
    forward: [f64; 2],
    radius_pixels: f64,
) -> [[f64; 2]; 4] {
    let right = [-forward[1], forward[0]];
    let tip_reach = radius_pixels * LOCATION_NAVIGATION_DART_TIP_RADIUS_RATIO;
    let base_half_width = radius_pixels * LOCATION_NAVIGATION_DART_BASE_HALF_WIDTH_RATIO;
    let base_offset = radius_pixels * LOCATION_NAVIGATION_DART_BASE_OFFSET_RATIO;
    let notch_offset = radius_pixels * LOCATION_NAVIGATION_DART_NOTCH_OFFSET_RATIO;
    [
        [
            center_screen[0] + forward[0] * tip_reach,
            center_screen[1] + forward[1] * tip_reach,
        ],
        [
            center_screen[0] + right[0] * base_half_width - forward[0] * base_offset,
            center_screen[1] + right[1] * base_half_width - forward[1] * base_offset,
        ],
        [
            center_screen[0] - forward[0] * notch_offset,
            center_screen[1] - forward[1] * notch_offset,
        ],
        [
            center_screen[0] - right[0] * base_half_width - forward[0] * base_offset,
            center_screen[1] - right[1] * base_half_width - forward[1] * base_offset,
        ],
    ]
}

/// Triangulates the dart's concave quadrilateral: the tip-to-notch diagonal
/// splits it into a right and a left triangle, both inside the outline.
fn push_navigation_dart_triangles(
    anchor_clip_position: &DVec4,
    viewport: &ViewportPixels,
    corners: &[[f64; 2]; 4],
    rgba_color: [f32; 4],
    vertices: &mut Vec<ClipSpaceOverlayVertex>,
) {
    let clip_corner =
        |corner: [f64; 2]| screen_pixels_to_clip_position(corner, *anchor_clip_position, viewport);
    let [tip, base_right, notch, base_left] = *corners;
    push_triangle(
        vertices,
        &[
            clip_corner(tip),
            clip_corner(base_right),
            clip_corner(notch),
        ],
        rgba_color,
    );
    push_triangle(
        vertices,
        &[clip_corner(tip), clip_corner(notch), clip_corner(base_left)],
        rgba_color,
    );
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_ACCURACY_RING_RGBA, DEFAULT_LOCATION_DISC_RADIUS_PIXELS,
        DEFAULT_LOCATION_DISC_RGBA, DEFAULT_LOCATION_HALO_RGBA, DEFAULT_LOCATION_RING_RGBA,
        LOCATION_ACCURACY_RING_FAN_SIDES, LOCATION_DISC_FAN_SIDES,
        LOCATION_NAVIGATION_DART_TIP_RADIUS_RATIO, LOCATION_PUCK_HALO_WIDTH_PIXELS,
        LOCATION_PUCK_RING_WIDTH_PIXELS, LocationIndicatorLayer, LocationIndicatorState,
        LocationIndicatorStyle, LocationIndicatorVisualKind, MAXIMUM_ACCURACY_RING_DIAMETER_PIXELS,
        MINIMUM_ACCURACY_RING_DRAW_METERS, accuracy_ring_coordinates, location_indicator_vertices,
    };
    use crate::camera::{DEFAULT_FIELD_OF_VIEW_DEGREES, MapCamera, ViewportPixels};
    use crate::overlay::{ClipSpaceOverlayVertex, clip_position_to_screen_pixels};
    use geospatial_types::GeographicCoordinate;
    use glam::DVec4;
    use rustic_maps_test_support::validated_coordinate as coordinate;
    use std::f64::consts::TAU;

    /// Screen-space tolerance of the exact expectations: the assembly runs
    /// the same f64 matrix path as the camera projection and stores f32 clip
    /// positions, whose rounding error stays far below a thousandth of a
    /// pixel at the test zooms.
    const SCREEN_PIXEL_TOLERANCE: f64 = 1e-3;

    /// The test zoom: the overlay layer's own test zoom, where the f32 clip
    /// rounding stays comfortably inside the tolerance.
    const TEST_ZOOM_LEVEL: f64 = 10.0;

    /// Triangles one tracking-mode puck draws: the halo disc, the ring's two
    /// triangles per side, and the accent core disc.
    const TRACKING_PUCK_TRIANGLE_COUNT: usize = LOCATION_DISC_FAN_SIDES * 4;

    /// Triangles one navigation-mode puck draws: the halo disc, the ring's
    /// two triangles per side, and the outline and fill darts' two triangles
    /// each.
    const NAVIGATION_PUCK_TRIANGLE_COUNT: usize = LOCATION_DISC_FAN_SIDES * 3 + 4;

    fn square_viewport() -> ViewportPixels {
        ViewportPixels::new(512.0, 512.0).expect("the test viewport is valid")
    }

    fn camera(center: GeographicCoordinate, bearing_degrees: f64, pitch_degrees: f64) -> MapCamera {
        MapCamera::new(center, TEST_ZOOM_LEVEL, bearing_degrees, pitch_degrees)
    }

    fn fix_layer(
        fix: GeographicCoordinate,
        course_heading_degrees: Option<f64>,
        horizontal_accuracy_meters: Option<f64>,
        visual_kind: LocationIndicatorVisualKind,
    ) -> LocationIndicatorLayer {
        LocationIndicatorLayer::new(
            LocationIndicatorState {
                coordinate: fix,
                course_heading_degrees,
                horizontal_accuracy_meters,
            },
            LocationIndicatorStyle::with_defaults(visual_kind),
        )
    }

    fn plain_dot_layer(fix: GeographicCoordinate) -> LocationIndicatorLayer {
        fix_layer(fix, None, None, LocationIndicatorVisualKind::TrackingDot)
    }

    /// Divides an indicator vertex's clip position to viewport pixels (the
    /// screen convention the assembly targets), mirroring the overlay
    /// layer's test helper.
    fn vertex_screen_pixels(
        vertex: &ClipSpaceOverlayVertex,
        viewport: &ViewportPixels,
    ) -> [f64; 2] {
        clip_position_to_screen_pixels(
            DVec4::new(
                f64::from(vertex.clip_position[0]),
                f64::from(vertex.clip_position[1]),
                f64::from(vertex.clip_position[2]),
                f64::from(vertex.clip_position[3]),
            ),
            viewport,
        )
    }

    /// The (minimum, maximum) span of one screen component over a vertex
    /// list.
    fn vertex_component_span(
        vertices: &[ClipSpaceOverlayVertex],
        component_index: usize,
        viewport: &ViewportPixels,
    ) -> (f64, f64) {
        vertices.iter().fold(
            (f64::INFINITY, f64::NEG_INFINITY),
            |(smallest, largest), vertex| {
                let screen = vertex_screen_pixels(vertex, viewport);
                (
                    smallest.min(screen[component_index]),
                    largest.max(screen[component_index]),
                )
            },
        )
    }

    /// The projected fix's screen center, the anchor every puck shape shares.
    fn projected_fix_center(
        map_camera: &MapCamera,
        fix: &GeographicCoordinate,
        viewport: &ViewportPixels,
    ) -> [f64; 2] {
        map_camera
            .project_to_screen(fix, viewport, DEFAULT_FIELD_OF_VIEW_DEGREES)
            .expect("the camera projects its own scene point")
    }

    #[test]
    fn the_cleared_layer_draws_nothing() {
        let viewport = square_viewport();
        let map_camera = camera(coordinate(47.0, 8.5), 0.0, 0.0);
        let vertices = location_indicator_vertices(
            &map_camera,
            &viewport,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
            None,
        );
        assert!(vertices.is_empty());
    }

    #[test]
    fn the_tracking_puck_draws_halo_ring_then_core_in_concentric_order() {
        let viewport = square_viewport();
        let fix = coordinate(47.0, 8.5);
        let map_camera = camera(fix, 0.0, 0.0);
        let vertices = location_indicator_vertices(
            &map_camera,
            &viewport,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
            Some(&plain_dot_layer(fix)),
        );
        assert_eq!(
            vertices.len(),
            TRACKING_PUCK_TRIANGLE_COUNT * 3,
            "one halo disc, one ring annulus and one core disc"
        );
        let center = projected_fix_center(&map_camera, &fix, &viewport);

        // The vertex stream is the draw order: halo first, then the ring,
        // then the core.
        let halo_vertex_count = LOCATION_DISC_FAN_SIDES * 3;
        let ring_vertex_count = LOCATION_DISC_FAN_SIDES * 2 * 3;
        let core_start = halo_vertex_count + ring_vertex_count;
        let halo_outer_radius = DEFAULT_LOCATION_DISC_RADIUS_PIXELS
            + LOCATION_PUCK_RING_WIDTH_PIXELS
            + LOCATION_PUCK_HALO_WIDTH_PIXELS;
        let ring_outer_radius =
            DEFAULT_LOCATION_DISC_RADIUS_PIXELS + LOCATION_PUCK_RING_WIDTH_PIXELS;
        for vertex in &vertices[..halo_vertex_count] {
            assert_eq!(vertex.rgba_color, DEFAULT_LOCATION_HALO_RGBA);
        }
        for vertex in &vertices[halo_vertex_count..core_start] {
            assert_eq!(vertex.rgba_color, DEFAULT_LOCATION_RING_RGBA);
            let screen = vertex_screen_pixels(vertex, &viewport);
            let radius = (screen[0] - center[0]).hypot(screen[1] - center[1]);
            assert!(
                radius >= DEFAULT_LOCATION_DISC_RADIUS_PIXELS - SCREEN_PIXEL_TOLERANCE
                    && radius <= ring_outer_radius + SCREEN_PIXEL_TOLERANCE,
                "the ring annulus stays between the core and ring radii: {radius}"
            );
        }
        for vertex in &vertices[core_start..] {
            assert_eq!(vertex.rgba_color, DEFAULT_LOCATION_DISC_RGBA);
            let screen = vertex_screen_pixels(vertex, &viewport);
            let radius = (screen[0] - center[0]).hypot(screen[1] - center[1]);
            assert!(
                radius <= DEFAULT_LOCATION_DISC_RADIUS_PIXELS + SCREEN_PIXEL_TOLERANCE,
                "the core stays inside the accent radius: {radius}"
            );
        }
        // The outermost drawn corner is the halo's radius.
        let (halo_smallest_x, halo_largest_x) =
            vertex_component_span(&vertices[..halo_vertex_count], 0, &viewport);
        let halo_span_radius = (halo_largest_x - halo_smallest_x) * 0.5;
        assert!(
            (halo_span_radius - halo_outer_radius).abs() < 1e-2,
            "the halo spans the full puck radius: {halo_span_radius} vs {halo_outer_radius}"
        );
    }

    #[test]
    fn a_fine_fix_draws_no_ring_and_a_usable_fix_draws_the_ground_ring() {
        let viewport = square_viewport();
        let fix = coordinate(47.0, 8.5);
        let map_camera = camera(fix, 0.0, 0.0);
        let fine_fix_vertices = location_indicator_vertices(
            &map_camera,
            &viewport,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
            Some(&fix_layer(
                fix,
                None,
                Some(MINIMUM_ACCURACY_RING_DRAW_METERS - 1.0),
                LocationIndicatorVisualKind::TrackingDot,
            )),
        );
        assert_eq!(
            fine_fix_vertices.len(),
            TRACKING_PUCK_TRIANGLE_COUNT * 3,
            "only the puck draws"
        );
        // A fix at 40× the threshold draws a ring: the vertex list grows by
        // the ring fan, whose top-down vertical span matches the accuracy's
        // screen diameter through the camera's meters-per-pixel, and whose
        // triangles paint the low-alpha ring color.
        let accuracy_meters = MINIMUM_ACCURACY_RING_DRAW_METERS * 40.0;
        let layered_vertices = location_indicator_vertices(
            &map_camera,
            &viewport,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
            Some(&fix_layer(
                fix,
                None,
                Some(accuracy_meters),
                LocationIndicatorVisualKind::TrackingDot,
            )),
        );
        let ring_vertex_count = layered_vertices.len() - TRACKING_PUCK_TRIANGLE_COUNT * 3;
        assert!(ring_vertex_count >= 3, "the ring adds its fan");
        let expected_diameter_pixels =
            2.0 * accuracy_meters / map_camera.ground_meters_per_screen_pixel();
        let (ring_smallest_y, ring_largest_y) =
            vertex_component_span(&layered_vertices[..ring_vertex_count], 1, &viewport);
        let drawn_vertical_span = ring_largest_y - ring_smallest_y;
        assert!(
            (drawn_vertical_span - expected_diameter_pixels).abs() < expected_diameter_pixels * 0.2,
            "the ring's top-down vertical span is the accuracy diameter: \
             {drawn_vertical_span} vs {expected_diameter_pixels}"
        );
        for ring_vertex in &layered_vertices[..ring_vertex_count] {
            assert_eq!(ring_vertex.rgba_color, DEFAULT_ACCURACY_RING_RGBA);
        }
    }

    #[test]
    fn an_absurdly_coarse_fix_clamps_its_accuracy_ring_to_the_screen_cap() {
        let viewport = square_viewport();
        let fix = coordinate(47.0, 8.5);
        let map_camera = camera(fix, 0.0, 0.0);
        let coarse_accuracy_meters =
            MAXIMUM_ACCURACY_RING_DIAMETER_PIXELS * map_camera.ground_meters_per_screen_pixel();
        let vertices = location_indicator_vertices(
            &map_camera,
            &viewport,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
            Some(&fix_layer(
                fix,
                None,
                Some(coarse_accuracy_meters),
                LocationIndicatorVisualKind::TrackingDot,
            )),
        );
        let expected_center = projected_fix_center(&map_camera, &fix, &viewport);
        let puck_vertex_count = TRACKING_PUCK_TRIANGLE_COUNT * 3;
        for ring_vertex in &vertices[..vertices.len() - puck_vertex_count] {
            let ring_screen = vertex_screen_pixels(ring_vertex, &viewport);
            let distance_pixels =
                (ring_screen[0] - expected_center[0]).hypot(ring_screen[1] - expected_center[1]);
            assert!(
                distance_pixels <= MAXIMUM_ACCURACY_RING_DIAMETER_PIXELS * 0.5 + 1.0,
                "every clamped ring corner stays inside the diameter cap: {distance_pixels}"
            );
        }
    }

    #[test]
    fn the_accuracy_ring_foreshortens_with_the_ground_at_pitch() {
        let viewport = square_viewport();
        let fix = coordinate(47.0, 8.5);
        let accuracy_meters = MINIMUM_ACCURACY_RING_DRAW_METERS * 40.0;
        let top_down_vertices = location_indicator_vertices(
            &camera(fix, 0.0, 0.0),
            &viewport,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
            Some(&fix_layer(
                fix,
                None,
                Some(accuracy_meters),
                LocationIndicatorVisualKind::TrackingDot,
            )),
        );
        let pitched_vertices = location_indicator_vertices(
            &camera(fix, 0.0, 45.0),
            &viewport,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
            Some(&fix_layer(
                fix,
                None,
                Some(accuracy_meters),
                LocationIndicatorVisualKind::TrackingDot,
            )),
        );
        let ring_vertex_count = top_down_vertices.len() - TRACKING_PUCK_TRIANGLE_COUNT * 3;
        assert_eq!(
            pitched_vertices.len() - TRACKING_PUCK_TRIANGLE_COUNT * 3,
            ring_vertex_count,
            "both cameras draw the same ring fan"
        );
        let (top_down_smallest, top_down_largest) =
            vertex_component_span(&top_down_vertices[..ring_vertex_count], 1, &viewport);
        let (pitched_smallest, pitched_largest) =
            vertex_component_span(&pitched_vertices[..ring_vertex_count], 1, &viewport);
        let top_down_height = top_down_largest - top_down_smallest;
        let pitched_height = pitched_largest - pitched_smallest;
        assert!(
            pitched_height < top_down_height * 0.95,
            "the pitched ring must foreshorten vertically: {pitched_height} vs {top_down_height}"
        );
    }

    #[test]
    fn the_navigation_puck_draws_a_symmetric_outlined_dart_inside_the_ring() {
        let viewport = square_viewport();
        let fix = coordinate(47.0, 8.5);
        let map_camera = camera(fix, 0.0, 0.0);
        let layer = fix_layer(
            fix,
            Some(0.0),
            None,
            LocationIndicatorVisualKind::GuidedCourseArrow,
        );
        let vertices = location_indicator_vertices(
            &map_camera,
            &viewport,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
            Some(&layer),
        );
        assert_eq!(
            vertices.len(),
            NAVIGATION_PUCK_TRIANGLE_COUNT * 3,
            "the halo, the ring, the outline dart and the fill dart"
        );
        let center = projected_fix_center(&map_camera, &fix, &viewport);
        let dart_start = LOCATION_DISC_FAN_SIDES * 3 * 3;
        let outline_vertices = &vertices[dart_start..dart_start + 6];
        let fill_vertices = &vertices[dart_start + 6..];
        for vertex in outline_vertices {
            assert_eq!(vertex.rgba_color, DEFAULT_LOCATION_RING_RGBA);
        }
        for vertex in fill_vertices {
            assert_eq!(vertex.rgba_color, DEFAULT_LOCATION_DISC_RGBA);
        }

        // Heading north under a north-up camera: the tip is the topmost
        // corner, at the dart's documented reach.
        let (dart_smallest_y, _) = vertex_component_span(fill_vertices, 1, &viewport);
        let expected_tip_y = center[1]
            - DEFAULT_LOCATION_DISC_RADIUS_PIXELS * LOCATION_NAVIGATION_DART_TIP_RADIUS_RATIO;
        assert!(
            (dart_smallest_y - expected_tip_y).abs() < SCREEN_PIXEL_TOLERANCE,
            "the dart tip reaches up-course: {dart_smallest_y} vs {expected_tip_y}"
        );

        // Symmetry about the course axis: north-up means the vertical line
        // through the fix, so every fill corner's lateral offset has a
        // mirrored partner and their sum is zero.
        let lateral_offset_sum: f64 = fill_vertices
            .iter()
            .map(|vertex| vertex_screen_pixels(vertex, &viewport)[0] - center[0])
            .sum();
        assert!(
            lateral_offset_sum.abs() < SCREEN_PIXEL_TOLERANCE,
            "the dart is symmetric about its course axis: {lateral_offset_sum}"
        );
        // The outline is wider than the fill: its tip reaches further
        // up-course by the outline width's scaled share.
        let (outline_smallest_y, _) = vertex_component_span(outline_vertices, 1, &viewport);
        assert!(
            outline_smallest_y < dart_smallest_y,
            "the white outline extends past the accent fill"
        );
    }

    #[test]
    fn the_navigation_dart_rotates_with_the_course_and_the_camera_bearing() {
        // The same absolute heading seen from two camera bearings: a
        // north-up camera's dart points up-screen, and the same north
        // heading under a 90°-rotated camera points left-screen — the
        // compass direction reads relative to the map beneath, not to the
        // viewport frame.
        let viewport = square_viewport();
        let fix = coordinate(47.0, 8.5);
        let north_up_vertices = location_indicator_vertices(
            &camera(fix, 0.0, 0.0),
            &viewport,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
            Some(&fix_layer(
                fix,
                Some(0.0),
                None,
                LocationIndicatorVisualKind::GuidedCourseArrow,
            )),
        );
        let rotated_vertices = location_indicator_vertices(
            &camera(fix, 90.0, 0.0),
            &viewport,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
            Some(&fix_layer(
                fix,
                Some(0.0),
                None,
                LocationIndicatorVisualKind::GuidedCourseArrow,
            )),
        );
        let center = projected_fix_center(&camera(fix, 0.0, 0.0), &fix, &viewport);
        let dart_start = LOCATION_DISC_FAN_SIDES * 3 * 3 + 6;
        let north_up_fill = &north_up_vertices[dart_start..];
        let rotated_fill = &rotated_vertices[dart_start..];
        let (north_up_smallest_y, _) = vertex_component_span(north_up_fill, 1, &viewport);
        let (rotated_smallest_x, _) = vertex_component_span(rotated_fill, 0, &viewport);
        let tip_reach =
            DEFAULT_LOCATION_DISC_RADIUS_PIXELS * LOCATION_NAVIGATION_DART_TIP_RADIUS_RATIO;
        assert!(
            (north_up_smallest_y - (center[1] - tip_reach)).abs() < SCREEN_PIXEL_TOLERANCE,
            "the north-up dart's tip reaches up-screen"
        );
        assert!(
            (rotated_smallest_x - (center[0] - tip_reach)).abs() < SCREEN_PIXEL_TOLERANCE,
            "the same north heading under a 90°-rotated camera reaches left-screen"
        );

        // A 90° course under a north-up camera points right-screen.
        let east_vertices = location_indicator_vertices(
            &camera(fix, 0.0, 0.0),
            &viewport,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
            Some(&fix_layer(
                fix,
                Some(90.0),
                None,
                LocationIndicatorVisualKind::GuidedCourseArrow,
            )),
        );
        let (_, east_largest_x) = vertex_component_span(&east_vertices[dart_start..], 0, &viewport);
        assert!(
            (east_largest_x - (center[0] + tip_reach)).abs() < SCREEN_PIXEL_TOLERANCE,
            "a 90° course points right-screen: {east_largest_x} vs {}",
            center[0] + tip_reach
        );
    }

    #[test]
    fn the_navigation_form_without_a_course_degrades_to_the_tracking_dot() {
        let viewport = square_viewport();
        let fix = coordinate(47.0, 8.5);
        let map_camera = camera(fix, 0.0, 0.0);
        let without_course = location_indicator_vertices(
            &map_camera,
            &viewport,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
            Some(&fix_layer(
                fix,
                None,
                None,
                LocationIndicatorVisualKind::GuidedCourseArrow,
            )),
        );
        let tracking_dot = location_indicator_vertices(
            &map_camera,
            &viewport,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
            Some(&plain_dot_layer(fix)),
        );
        assert_eq!(
            without_course.len(),
            TRACKING_PUCK_TRIANGLE_COUNT * 3,
            "the navigation form without a course draws the tracking puck"
        );
        assert_eq!(without_course.len(), tracking_dot.len());
        for (without_course_vertex, tracking_vertex) in
            without_course.iter().zip(tracking_dot.iter())
        {
            assert_eq!(without_course_vertex.rgba_color, tracking_vertex.rgba_color);
            assert_eq!(
                without_course_vertex.clip_position,
                tracking_vertex.clip_position
            );
        }
    }

    #[test]
    fn behind_the_camera_the_puck_skips_while_the_ring_keeps_its_corners() {
        // A fix far south of a pitched, north-looking camera sits behind the
        // camera plane: the puck skips (a non-positive homogeneous weight),
        // while the ground-projected ring still leaves finite corners for
        // the GPU's frustum clip — the documented split.
        let viewport = square_viewport();
        let map_camera = camera(coordinate(0.0, 0.0), 0.0, 40.0);
        let behind_fix = coordinate(-0.3, 0.0);
        let vertices = location_indicator_vertices(
            &map_camera,
            &viewport,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
            Some(&fix_layer(
                behind_fix,
                Some(0.0),
                Some(MINIMUM_ACCURACY_RING_DRAW_METERS * 5.0),
                LocationIndicatorVisualKind::GuidedCourseArrow,
            )),
        );
        assert!(
            !vertices.is_empty(),
            "the ground-projected ring assembles for the GPU to clip"
        );
        for vertex in &vertices {
            assert!(
                vertex
                    .clip_position
                    .iter()
                    .all(|component| component.is_finite()),
                "every corner stays finite"
            );
        }
    }

    #[test]
    fn the_accuracy_ring_matches_its_geodesic_radius() {
        let fix = coordinate(47.0, 8.5);
        let radius_meters = 100.0;
        let ring = accuracy_ring_coordinates(&fix, radius_meters);
        assert_eq!(ring.len(), LOCATION_ACCURACY_RING_FAN_SIDES);
        let equatorial_radius_meters = crate::camera::EARTH_EQUATORIAL_CIRCUMFERENCE_METERS / TAU;
        for ring_point in &ring {
            let north_meters =
                (ring_point.latitude - fix.latitude).to_radians() * equatorial_radius_meters;
            let east_meters = (ring_point.longitude - fix.longitude).to_radians()
                * equatorial_radius_meters
                * fix.latitude.to_radians().cos();
            let drawn_radius_meters = north_meters.hypot(east_meters);
            assert!(
                (drawn_radius_meters - radius_meters).abs() < 1e-6,
                "every ring point sits one accuracy radius from the fix: \
                 {drawn_radius_meters}"
            );
        }
    }
}
