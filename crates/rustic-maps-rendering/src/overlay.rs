//! GPU-free overlay assembly: route polylines, filled polygons, point
//! markers, and traffic-flow lines projected into clip-space solid-color
//! triangle lists drawn over the tile quads.
//!
//! Overlays are the second draw layer of a captured frame: the same camera
//! pose and viewport that assemble the tile quads
//! ([`crate::renderer::Renderer::frame_quads`]) also project overlay shapes
//! into clip-space triangles via [`overlay_triangle_vertices`], which the
//! headless renderer draws through a separate solid-color pipeline in the
//! same render pass after the tiles. The WebAssembly canvas mirrors the
//! vertex contract verbatim, so the assembly here is deliberately `wgpu`-free
//! and emits one flat, non-indexed triangle list.
//!
//! Assembly semantics:
//!
//! * **World copies / antimeridian.** Overlay geometry wraps around the
//!   camera exactly like tile spans do (see
//!   [`crate::renderer::FrameProjection`]): the first vertex of a shape sits
//!   in the camera's world copy via the shortest wrapped delta, every
//!   following vertex unwraps continuously from it, and each resulting
//!   segment or triangle is split against the camera-centered world-copy
//!   windows ([`WORLD_COPY_WINDOW_OFFSETS`], reusing the tile spans' window
//!   half-width). A shape wider than the covered window range clips to
//!   nothing beyond it; a single unwrapping step never spans more than half a
//!   world, so three windows cover every reachable fragment.
//! * **Frustum clipping.** Assembly relies on the GPU's clip-space frustum
//!   clip, the same mechanism the tile pipeline relies on for near-plane
//!   straddling spans; no extra CPU cull runs. Screen-space-thickened shapes
//!   (route lines, markers, traffic) additionally drop the segments and
//!   markers with a vertex at or behind the camera because the screen-space
//!   width offset needs a positive homogeneous weight; filled polygons emit
//!   genuine clip-space corners and keep near-plane straddling triangles for
//!   the GPU to clip exactly.
//! * **Line joins.** A polyline draws as butt-capped segment quads plus one
//!   screen-space square (the line's width) per interior vertex, oriented
//!   along the bend's bisector. The square fills the wedge the butt caps
//!   leave outside a bend, which otherwise reads as a gap between styled road
//!   segments; it never grows a miter spike at sharp angles. Endpoints keep
//!   their butt caps (a per-feature end has no neighbouring segment to join),
//!   and sharp joins can still show a sliver where the wedge exceeds the
//!   square; a full round-join fan would close that gap.
//!
//!   That default stroke is what a route line, a traffic flow and a style
//!   layer that declares no line presentation draw. A style layer that
//!   declares any of `line-cap`, `line-join`, `line-dasharray`, `line-offset`,
//!   `line-gap-width` or `line-blur` carries a [`PolylineStroke`] instead,
//!   and the assembly honours it: miter joins limited to twice the half width
//!   (a sharper corner falls back to bevel), bevel and round joins, butt,
//!   square and round caps, dash runs measured in line-width units,
//!   perpendicular offsets, two-band casings outside a gap, and a documented
//!   blur fringe painted under the core stroke.
//! * **Polygon holes.** A filled polygon carries one outer ring and any number
//!   of hole rings. Holes join the outer boundary through the standard
//!   bridge-splicing step before ear clipping ([`FilledPolygonOverlay`]
//!   documents the algorithm), so the emitted triangle stream covers exactly
//!   the ring set's even-odd interior: the overlay pipeline paints a flat
//!   triangle list with alpha blending and has no stencil test, so a hole must
//!   leave its region out of that list rather than erase it after the fact.
//! * **Markers.** A marker is a regular-octagon triangle fan centered on the
//!   projected anchor point (center anchoring, not bottom pinning): the
//!   projected coordinate is the fan's center, so a marker's center sits
//!   exactly at its geographic position. Sprites and label callouts are
//!   drawn by other layers.
//! * **Pins.** A pin is a teardrop anchored the other way: its tip sits on the
//!   projected coordinate and the circular head above it carries a hollow
//!   center. The teardrop is star-shaped around the head center, so the
//!   outline is one radial function of the angle and the hollow center is the
//!   annulus between that outline and an inner circle — again a plain triangle
//!   list, no erase pass.

use crate::camera::{MapCamera, ViewportPixels, normalized_shortest_delta};
use crate::projection::{
    coordinate_to_normalized_world, normalized_world_to_coordinate, zoom_scale_pixels,
};
use crate::renderer::CAMERA_WORLD_WINDOW_HALF_WIDTH;
use geospatial_types::{
    GeographicCoordinate, RouteData, TrafficFlowCongestionLevel, TrafficFlowData,
};
use glam::{DMat4, DVec4};
use rustic_maps_style::{
    CirclePitchAlignment, CirclePitchScale, LineCap, LineJoin, TranslateAnchor,
};
use std::f64::consts::{PI, TAU};

/// Default route-line color: opaque blue in the leading map SDK's route hue.
pub const DEFAULT_ROUTE_LINE_RGBA: [f32; 4] = [0.13, 0.47, 0.87, 1.0];

/// Default route-line width in screen pixels, drawn as screen-space-thickened
/// segment quads (constant width regardless of zoom and pitch).
pub const DEFAULT_ROUTE_LINE_WIDTH_PIXELS: f64 = 6.0;

/// Default filled-polygon color: translucent amber, so the base tiles stay
/// readable beneath the fill through the overlay pipeline's alpha blending.
pub const DEFAULT_FILLED_POLYGON_RGBA: [f32; 4] = [0.95, 0.60, 0.15, 0.55];

/// Default point-marker color: opaque red.
pub const DEFAULT_POINT_MARKER_RGBA: [f32; 4] = [0.90, 0.15, 0.25, 1.0];

/// Default point-marker radius in screen pixels; the marker is a regular
/// octagon fan of this radius around the projected anchor.
pub const DEFAULT_POINT_MARKER_RADIUS_PIXELS: f64 = 8.0;

/// Default point-marker stroke width in screen pixels: no stroke.
pub const DEFAULT_CIRCLE_STROKE_WIDTH_PIXELS: f64 = 0.0;

/// Default point-marker edge softening in screen pixels: a hard edge.
pub const DEFAULT_CIRCLE_BLUR_PIXELS: f64 = 0.0;

/// The pitch-scale behaviour a point marker has when its style omits the
/// property. The specification's default is `map`, but this renderer's
/// markers are screen-facing by default and carry no ground-plane geometry,
/// so an omitted value keeps the screen radius (the `viewport` behaviour);
/// a style that asks for `map` gets the perspective behaviour.
pub const DEFAULT_POINT_MARKER_PITCH_SCALE: CirclePitchScale = CirclePitchScale::Viewport;

/// The pitch alignment a point marker has when its style omits the property:
/// it faces the screen, the specification's default.
pub const DEFAULT_POINT_MARKER_PITCH_ALIGNMENT: CirclePitchAlignment =
    CirclePitchAlignment::Viewport;

/// Default point-pin color: opaque red, the destination-pin hue of the shared
/// reference-app design language's A/B markers.
pub const DEFAULT_POINT_PIN_RGBA: [f32; 4] = [0.85, 0.16, 0.20, 1.0];

/// Default point-pin head radius in screen pixels; the tip sits the pin's
/// height below the head center (see [`POINT_PIN_HEIGHT_RADIUS_FACTOR`]).
pub const DEFAULT_POINT_PIN_RADIUS_PIXELS: f64 = 9.0;

/// Default traffic-flow line width in screen pixels; thinner than the route
/// line, since traffic paints many parallel segments.
pub const DEFAULT_TRAFFIC_LINE_WIDTH_PIXELS: f64 = 4.0;

/// RGBA of a free-flowing traffic segment (at or above free-flow speed): green.
pub const TRAFFIC_FREE_FLOW_RGBA: [f32; 4] = [0.16, 0.68, 0.32, 1.0];

/// RGBA of a slow traffic segment (moderately below free-flow speed): amber.
pub const TRAFFIC_SLOW_RGBA: [f32; 4] = [0.96, 0.71, 0.14, 1.0];

/// RGBA of a congested traffic segment (stop-and-go): red.
pub const TRAFFIC_CONGESTED_RGBA: [f32; 4] = [0.85, 0.18, 0.13, 1.0];

/// RGBA of a blocked traffic segment (roadway effectively closed): dark maroon.
pub const TRAFFIC_BLOCKED_RGBA: [f32; 4] = [0.42, 0.09, 0.12, 1.0];

/// Vertices per triangle of the overlay layer's flat, non-indexed clip-space
/// triangle stream ([`overlay_triangle_vertices`]); the stream's length
/// divides evenly by this count into the frame's triangle tally.
pub const OVERLAY_TRIANGLE_VERTEX_COUNT: usize = 3;

/// The device-pixel-ratio multiplier that emits every overlay's declared
/// point/CSS screen sizes unchanged: the identity of
/// [`MapOverlay::scaled_to_device_pixel_ratio`], applied at emission by
/// [`overlay_triangle_vertices`].
pub const UNSCALED_DEVICE_PIXEL_RATIO: f64 = 1.0;

/// World-copy windows the assembly clips overlay geometry against, in whole
/// world-copy offsets around the camera's own copy. The camera's own window
/// (`0`) plus one neighbor on each side cover every position an unwrapping
/// chain can reach: each unwrapping step moves less than half a world, so a
/// shape reaching a second neighbor window would already be wider than three
/// world copies.
pub(crate) const WORLD_COPY_WINDOW_OFFSETS: [i32; 3] = [-1, 0, 1];

/// Screen-space length below which a thickened segment quad degenerates (both
/// endpoints project to the same pixel) and draws nothing: its perpendicular
/// direction is undefined there.
const MINIMUM_VISIBLE_SEGMENT_LENGTH_PIXELS: f64 = 1e-6;

/// Triangle pairs in the fan a round cap or join draws over one half circle:
/// each pair spans a sixteenth of a turn, which stays smooth at street widths.
const ROUND_LINE_FAN_SIDES: usize = 8;

/// The specification's fixed miter limit: a join whose miter length exceeds
/// twice the half width draws as a bevel instead, so a sharp corner never
/// grows a spike.
const LINE_MITER_LIMIT: f64 = 2.0;

/// The alpha a blurred stroke's fringe keeps relative to the core stroke. The
/// overlay pipeline has no per-fragment blur, so the fringe approximates the
/// specification's soft edge with a wider, fainter pass under the core.
const LINE_BLUR_FRINGE_ALPHA_SCALE: f32 = 0.35;

/// Most corners a triangle can keep when clipped against one world-copy
/// window: Sutherland–Hodgman adds at most one corner per cut, and a window
/// cuts the triangle with two vertical half-planes.
const MAXIMUM_WINDOW_CLIPPED_CORNERS: usize = 5;

/// Side count of the regular polygon approximating a point marker's circle:
/// an octagon fan is a cheap circle approximation.
const POINT_MARKER_FAN_SIDES: usize = 8;

/// Side count of the ring approximating a point pin's head circle and hollow
/// center. Sixteen sides keep the head's silhouette visibly round at the
/// default radius while staying far below the tessellation cost that would
/// matter for a marker set.
const POINT_PIN_RING_SIDES: usize = 16;

/// Pin height (tip to head center) as a multiple of the head radius: the
/// ratio that yields the familiar teardrop silhouette, whose tip half-angle is
/// `asin(1 / ratio)`.
const POINT_PIN_HEIGHT_RADIUS_FACTOR: f64 = 2.6;

/// Radius of the pin head's hollow center as a fraction of the head radius.
/// The hole is what keeps the pin readable over dense cartography.
const POINT_PIN_HOLLOW_RADIUS_RATIO: f64 = 0.42;

/// Doubled signed-area magnitude below which a polygon ring counts as
/// degenerate (collinear or duplicated points) and fills nothing.
const POLYGON_AREA_DEGENERACY_TOLERANCE: f64 = 1e-14;

/// Boundary tolerance of the ear-clipping containment test: a remaining
/// vertex within this doubled-area distance of every ear edge blocks the ear,
/// so ears never swallow a lying-on-the-edge vertex.
const EAR_CLIPPING_BOUNDARY_TOLERANCE: f64 = 1e-12;

/// Relative tolerance of the ear clipper's stall-recovery collinearity
/// filter: a node counts as collinear when its doubled area is at most this
/// fraction of its longer adjacent edge's squared length. A triangle this
/// thin is below a nanodegree of angular extent on every real ring, so
/// dropping it as a recovery step cannot change a visible fill.
const EAR_CLIPPING_COLLINEAR_RELATIVE_TOLERANCE: f64 = 1e-9;

/// One overlay triangle-list vertex as consumed by the GPU: a position in
/// clip space (homogeneous coordinates; the perspective divide happens on the
/// GPU) and the solid RGBA color to shade there. `repr(C)` keeps the field
/// order the vertex buffer layout expects.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipSpaceOverlayVertex {
    /// Position in clip space, `[x, y, z, w]`.
    pub clip_position: [f32; 4],
    /// Solid RGBA fill color, components in `[0, 1]`.
    pub rgba_color: [f32; 4],
}

/// A routed path drawn as a thick line: one traversal-order polyline
/// thickened to [`RoutePolylineOverlay::width_pixels`] in screen space and
/// painted solid [`RoutePolylineOverlay::rgba_color`].
///
/// Build with [`RoutePolylineOverlay::new`] for the documented default style,
/// override the public fields for a custom one, or use
/// [`RoutePolylineOverlay::from_route_data`] to flatten a calculated route's
/// leg-step geometry. A styled line layer sets [`Self::stroke`] to the
/// presentation its style declares.
#[derive(Debug, Clone, PartialEq)]
pub struct RoutePolylineOverlay {
    /// Traversal-order polyline of the route. Consecutive equal coordinates
    /// skip their (degenerate) segment.
    pub coordinates: Vec<GeographicCoordinate>,
    /// Solid RGBA line color; see [`DEFAULT_ROUTE_LINE_RGBA`] for the default.
    pub rgba_color: [f32; 4],
    /// Line width in screen pixels; see [`DEFAULT_ROUTE_LINE_WIDTH_PIXELS`].
    pub width_pixels: f64,
    /// How the line is stroked. `None` draws the renderer's default route
    /// stroke (butt-capped segment quads plus a square join per bend); a
    /// vector-style line layer that declares any line presentation property
    /// carries a [`PolylineStroke`] instead.
    pub stroke: Option<PolylineStroke>,
    /// The line's screen-space displacement from its geometry, in CSS pixels
    /// (the style's `line-translate`). Zero draws on the geometry.
    pub translate_pixels: [f64; 2],
    /// The frame of reference the displacement is measured against.
    pub translate_anchor: TranslateAnchor,
}

/// How a styled polyline is stroked, following the MapLibre style
/// specification's line properties.
///
/// Lengths in line-width units scale with the resolved line width, so a dash
/// pattern keeps its proportions at every zoom the way the specification
/// defines it.
#[derive(Debug, Clone, PartialEq)]
pub struct PolylineStroke {
    /// How the ends of a stroke are drawn.
    pub cap: LineCap,
    /// How two segments meet.
    pub join: LineJoin,
    /// Alternating on and off lengths in line-width units. An empty pattern
    /// draws a solid line; an odd-length pattern repeats its last length.
    pub dash_array: Vec<f64>,
    /// The perpendicular offset in screen pixels; a positive value shifts the
    /// stroke to the right of its path direction, the specification's
    /// convention.
    pub offset_pixels: f64,
    /// The width of the gap the stroke is drawn around, in screen pixels.
    /// Zero paints a solid stroke; a positive value paints two parallel
    /// strokes outside the gap, one on each side.
    pub gap_width_pixels: f64,
    /// The blur width in screen pixels; zero paints a hard edge.
    pub blur_pixels: f64,
}

impl RoutePolylineOverlay {
    /// Builds a route polyline with the documented default style.
    pub fn new(coordinates: Vec<GeographicCoordinate>) -> Self {
        Self {
            coordinates,
            rgba_color: DEFAULT_ROUTE_LINE_RGBA,
            width_pixels: DEFAULT_ROUTE_LINE_WIDTH_PIXELS,
            stroke: None,
            translate_pixels: [0.0, 0.0],
            translate_anchor: TranslateAnchor::Viewport,
        }
    }

    /// Flattens a calculated route's per-step shape polylines into one
    /// traversal-order route line with the documented default style. Legs and
    /// steps concatenate in order; steps share their boundary points, whose
    /// duplicate segments skip as degenerate.
    pub fn from_route_data(route_data: &RouteData) -> Self {
        let coordinates = route_data
            .legs
            .iter()
            .flat_map(|leg| leg.steps.iter())
            .flat_map(|step| step.shape.iter().copied())
            .collect();
        Self::new(coordinates)
    }
}

/// A filled polygon: one outer ring plus any hole rings, bridge-joined and
/// ear-clip triangulated in one pass and painted solid
/// [`FilledPolygonOverlay::rgba_color`] (through the overlay pipeline's alpha
/// blending, so translucent fills keep the base tiles readable).
///
/// Holes stay unfilled by construction: the assembly emits a flat,
/// non-overlapping triangle list whose triangles cover only the outer ring's
/// interior outside every hole. The overlay pipeline has no stencil test and
/// no erase operation, so subtracting the holes from the emitted triangle
/// stream is the only representation its solid-color fragment stage supports.
/// The bridge splice (`ear_clip_triangulation_with_holes`) joins each hole
/// to the outer boundary through a zero-width slit; the slit's duplicated
/// vertices are collinear, never paint a fragment of their own, and keep the
/// combined boundary a single simple ring the ear clipper can walk.
#[derive(Debug, Clone, PartialEq)]
pub struct FilledPolygonOverlay {
    /// Outer ring in traversal order, without a repeated closing vertex (a
    /// repeated one is tolerated and dropped). Self-intersecting or
    /// near-collinear rings are degenerate and may fill nothing.
    pub ring_coordinates: Vec<GeographicCoordinate>,
    /// Hole rings subtracted from the fill, each in traversal order and
    /// without a repeated closing vertex. The rings' winding is normalized
    /// before triangulation, so decoded order does not matter. A hole that
    /// falls outside the outer ring is not contained by it and may paint
    /// outside the fill; producers emit contained holes.
    pub hole_ring_coordinates: Vec<Vec<GeographicCoordinate>>,
    /// Fill color; see [`DEFAULT_FILLED_POLYGON_RGBA`] for the default.
    pub rgba_color: [f32; 4],
    /// Edge color, when the style declares one; `None` paints no outline.
    pub rgba_outline_color: Option<[f32; 4]>,
    /// Whether the fill's edge is antialiased. A disabled edge paints no
    /// outline even when an edge color is present.
    pub antialias: bool,
    /// The fill's displacement from its geometry, in screen pixels.
    pub translate_pixels: [f64; 2],
    /// The frame of reference the displacement is measured against. The
    /// renderer's own default is the viewport, so a shape built through
    /// [`FilledPolygonOverlay::new`] keeps painting unchanged; a style layer
    /// carries the specification's `map` default explicitly.
    pub translate_anchor: TranslateAnchor,
}

impl FilledPolygonOverlay {
    /// Builds a filled polygon without holes and the documented default style.
    pub fn new(ring_coordinates: Vec<GeographicCoordinate>) -> Self {
        Self {
            ring_coordinates,
            hole_ring_coordinates: Vec::new(),
            rgba_color: DEFAULT_FILLED_POLYGON_RGBA,
            rgba_outline_color: None,
            antialias: true,
            translate_pixels: [0.0, 0.0],
            translate_anchor: TranslateAnchor::Viewport,
        }
    }

    /// Adds the hole rings subtracted from the fill (see the type docs).
    pub fn with_hole_rings(
        mut self,
        hole_ring_coordinates: Vec<Vec<GeographicCoordinate>>,
    ) -> Self {
        self.hole_ring_coordinates = hole_ring_coordinates;
        self
    }
}

/// Point markers drawn as screen-facing colored octagons anchored with their
/// center at each projected position (center anchoring, not bottom pinning;
/// see the module notes). Sprite and atlas markers are drawn by other
/// layers.
#[derive(Debug, Clone, PartialEq)]
pub struct PointMarkersOverlay {
    /// Geographic anchor of every marker; each anchors independently, so a
    /// marker behind the camera or elsewhere unprojectable draws nothing
    /// without disturbing its neighbors.
    pub positions: Vec<GeographicCoordinate>,
    /// Marker color; see [`DEFAULT_POINT_MARKER_RGBA`] for the default.
    pub rgba_color: [f32; 4],
    /// Marker radius in screen pixels; see
    /// [`DEFAULT_POINT_MARKER_RADIUS_PIXELS`].
    pub radius_pixels: f64,
    /// Stroke color drawn outside the radius, when the style declares one.
    pub rgba_stroke_color: Option<[f32; 4]>,
    /// Stroke width in screen pixels, drawn outside the radius.
    pub stroke_width_pixels: f64,
    /// Edge softening in screen pixels; zero paints a hard edge.
    pub blur_pixels: f64,
    /// The marker's displacement from its anchor, in screen pixels.
    pub translate_pixels: [f64; 2],
    /// Whether the radius follows the ground plane when the map is pitched.
    pub pitch_scale: CirclePitchScale,
    /// Whether the marker lies on the ground plane or faces the screen.
    pub pitch_alignment: CirclePitchAlignment,
}

impl PointMarkersOverlay {
    /// Builds a point-marker overlay with the documented default style.
    pub fn new(positions: Vec<GeographicCoordinate>) -> Self {
        Self {
            positions,
            rgba_color: DEFAULT_POINT_MARKER_RGBA,
            radius_pixels: DEFAULT_POINT_MARKER_RADIUS_PIXELS,
            rgba_stroke_color: None,
            stroke_width_pixels: DEFAULT_CIRCLE_STROKE_WIDTH_PIXELS,
            blur_pixels: DEFAULT_CIRCLE_BLUR_PIXELS,
            translate_pixels: [0.0, 0.0],
            pitch_scale: DEFAULT_POINT_MARKER_PITCH_SCALE,
            pitch_alignment: DEFAULT_POINT_MARKER_PITCH_ALIGNMENT,
        }
    }
}

/// Teardrop pins: a screen-facing pin per anchor, drawn as a filled teardrop
/// whose point (tip) sits exactly on the projected anchor and whose circular
/// head carries a hollow center — the standard "A/B" route-endpoint marker.
///
/// The teardrop boundary is star-shaped around the head center, which lets the
/// same no-stencil triangle stream that fills a ring also leave the head's
/// center unpainted: the assembly emits the annulus between the head hole and
/// the outer teardrop boundary instead of a solid fan
/// ([`point_pin_triangles`]), so the hole needs no erase pass.
#[derive(Debug, Clone, PartialEq)]
pub struct PointPinsOverlay {
    /// Geographic anchor of every pin's tip; each pin anchors independently,
    /// so a pin behind the camera draws nothing without disturbing its
    /// neighbors.
    pub positions: Vec<GeographicCoordinate>,
    /// Pin color; see [`DEFAULT_POINT_PIN_RGBA`] for the default.
    pub rgba_color: [f32; 4],
    /// Head radius in screen pixels; see [`DEFAULT_POINT_PIN_RADIUS_PIXELS`].
    pub radius_pixels: f64,
}

impl PointPinsOverlay {
    /// Builds a point-pin overlay with the documented default style.
    pub fn new(positions: Vec<GeographicCoordinate>) -> Self {
        Self {
            positions,
            rgba_color: DEFAULT_POINT_PIN_RGBA,
            radius_pixels: DEFAULT_POINT_PIN_RADIUS_PIXELS,
        }
    }
}

/// Traffic-flow lines: every flow's shape polyline draws as a thick line
/// colored by its segment congestion classification
/// ([`traffic_congestion_rgba`]), reusing the route-line thickening
/// machinery with per-segment colors.
#[derive(Debug, Clone, PartialEq)]
pub struct TrafficFlowOverlay {
    /// Flow observations to paint.
    pub flows: Vec<TrafficFlowData>,
    /// Line width in screen pixels; see
    /// [`DEFAULT_TRAFFIC_LINE_WIDTH_PIXELS`].
    pub width_pixels: f64,
}

impl TrafficFlowOverlay {
    /// Builds a traffic-flow overlay with the documented default style.
    pub fn new(flows: Vec<TrafficFlowData>) -> Self {
        Self {
            flows,
            width_pixels: DEFAULT_TRAFFIC_LINE_WIDTH_PIXELS,
        }
    }
}

/// One overlay shape for a frame; assemble with [`overlay_triangle_vertices`].
#[derive(Debug, Clone, PartialEq)]
pub enum MapOverlay {
    /// A routed path drawn as a thick line ([`RoutePolylineOverlay`]).
    RoutePolyline(RoutePolylineOverlay),
    /// A filled polygon ([`FilledPolygonOverlay`]).
    FilledPolygon(FilledPolygonOverlay),
    /// Screen-facing point markers ([`PointMarkersOverlay`]).
    PointMarkers(PointMarkersOverlay),
    /// Screen-facing teardrop pins anchored at their tip
    /// ([`PointPinsOverlay`]).
    PointPins(PointPinsOverlay),
    /// Congestion-colored traffic-flow lines ([`TrafficFlowOverlay`]).
    TrafficFlow(TrafficFlowOverlay),
}

impl MapOverlay {
    /// Wraps a raw coordinate polyline as a route line with the documented
    /// default style; the coordinate-list counterpart of
    /// [`RoutePolylineOverlay::from_route_data`].
    pub fn route_polyline(coordinates: Vec<GeographicCoordinate>) -> Self {
        Self::RoutePolyline(RoutePolylineOverlay::new(coordinates))
    }

    /// Wraps a raw coordinate ring as a filled polygon with the documented
    /// default style.
    pub fn filled_polygon(ring_coordinates: Vec<GeographicCoordinate>) -> Self {
        Self::FilledPolygon(FilledPolygonOverlay::new(ring_coordinates))
    }

    /// Wraps raw anchor positions as point markers with the documented
    /// default style.
    pub fn point_markers(positions: Vec<GeographicCoordinate>) -> Self {
        Self::PointMarkers(PointMarkersOverlay::new(positions))
    }

    /// Wraps raw anchor positions as point pins with the documented default
    /// style.
    pub fn point_pins(positions: Vec<GeographicCoordinate>) -> Self {
        Self::PointPins(PointPinsOverlay::new(positions))
    }

    /// Wraps flow observations as a traffic-flow layer with the documented
    /// default style.
    pub fn traffic_flow(flows: Vec<TrafficFlowData>) -> Self {
        Self::TrafficFlow(TrafficFlowOverlay::new(flows))
    }

    /// Returns the overlay with its screen-space sizes — route and traffic
    /// line widths, marker and pin radii — multiplied by
    /// `device_pixel_ratio`, the drawing buffer's device pixels per
    /// point/CSS pixel. A surface that draws in device pixels while the
    /// overlay descriptors declare point sizes scales the composed list once
    /// before assembling its frame, so every line and dot keeps its declared
    /// point size on a 2× or 3× surface. A filled polygon carries no
    /// screen-space size and is returned unchanged.
    pub fn scaled_to_device_pixel_ratio(&self, device_pixel_ratio: f64) -> Self {
        let mut scaled_overlay = self.clone();
        match &mut scaled_overlay {
            MapOverlay::RoutePolyline(route_polyline) => {
                route_polyline.width_pixels *= device_pixel_ratio;
                route_polyline.translate_pixels = [
                    route_polyline.translate_pixels[0] * device_pixel_ratio,
                    route_polyline.translate_pixels[1] * device_pixel_ratio,
                ];
                if let Some(stroke) = route_polyline.stroke.as_mut() {
                    stroke.offset_pixels *= device_pixel_ratio;
                    stroke.gap_width_pixels *= device_pixel_ratio;
                    stroke.blur_pixels *= device_pixel_ratio;
                }
            }
            MapOverlay::FilledPolygon(filled_polygon) => {
                filled_polygon.translate_pixels = [
                    filled_polygon.translate_pixels[0] * device_pixel_ratio,
                    filled_polygon.translate_pixels[1] * device_pixel_ratio,
                ];
            }
            MapOverlay::PointMarkers(point_markers) => {
                point_markers.radius_pixels *= device_pixel_ratio;
                point_markers.translate_pixels = [
                    point_markers.translate_pixels[0] * device_pixel_ratio,
                    point_markers.translate_pixels[1] * device_pixel_ratio,
                ];
            }
            MapOverlay::PointPins(point_pins) => {
                point_pins.radius_pixels *= device_pixel_ratio;
            }
            MapOverlay::TrafficFlow(traffic_flow) => {
                traffic_flow.width_pixels *= device_pixel_ratio;
            }
        }
        scaled_overlay
    }

    /// Returns the overlay with every RGBA color's alpha multiplied by
    /// `opacity`, so a caller can draw a whole overlay list as a translucent
    /// layer over another one. A tile-level cross-fade uses it to dissolve a
    /// retired vector level into the level that replaced it.
    ///
    /// `opacity` is clamped into `[0, 1]`; the geometry, sizes and colors other
    /// than alpha are unchanged. A traffic-flow overlay derives its colors
    /// from each flow's congestion classification and carries no color of its
    /// own, so it is returned unchanged.
    pub fn with_opacity_multiplier(&self, opacity: f32) -> Self {
        let opacity = opacity.clamp(0.0, 1.0);
        let faded_color = |rgba_color: [f32; 4]| {
            [
                rgba_color[0],
                rgba_color[1],
                rgba_color[2],
                rgba_color[3] * opacity,
            ]
        };
        let mut faded_overlay = self.clone();
        match &mut faded_overlay {
            MapOverlay::RoutePolyline(route_polyline) => {
                route_polyline.rgba_color = faded_color(route_polyline.rgba_color);
            }
            MapOverlay::FilledPolygon(filled_polygon) => {
                filled_polygon.rgba_color = faded_color(filled_polygon.rgba_color);
                filled_polygon.rgba_outline_color =
                    filled_polygon.rgba_outline_color.map(faded_color);
            }
            MapOverlay::PointMarkers(point_markers) => {
                point_markers.rgba_color = faded_color(point_markers.rgba_color);
                point_markers.rgba_stroke_color = point_markers.rgba_stroke_color.map(faded_color);
            }
            MapOverlay::PointPins(point_pins) => {
                point_pins.rgba_color = faded_color(point_pins.rgba_color);
            }
            MapOverlay::TrafficFlow(_) => {}
        }
        faded_overlay
    }
}

/// The solid RGBA a traffic congestion classification paints with; the
/// named-constant mapping from [`TrafficFlowCongestionLevel`] to overlay
/// color.
pub fn traffic_congestion_rgba(congestion_level: TrafficFlowCongestionLevel) -> [f32; 4] {
    match congestion_level {
        TrafficFlowCongestionLevel::FreeFlow => TRAFFIC_FREE_FLOW_RGBA,
        TrafficFlowCongestionLevel::Slow => TRAFFIC_SLOW_RGBA,
        TrafficFlowCongestionLevel::Congested => TRAFFIC_CONGESTED_RGBA,
        TrafficFlowCongestionLevel::Blocked => TRAFFIC_BLOCKED_RGBA,
    }
}

/// The highest style-zoom level at which styled vector-base geometry is
/// simplified before it enters the per-tile styled cache. At city zoom and
/// above every feature is already near the screen's resolution, so the
/// simplification is skipped and the drawn geometry stays exactly the style's
/// (the committed z9 and z14 goldens are untouched).
pub const LOW_ZOOM_GEOMETRY_SIMPLIFICATION_MAXIMUM_ZOOM_LEVEL: f64 = 8.0;

/// The screen-space tolerance, in CSS pixels, ring and line simplification
/// allows at low zoom: a removed vertex lies within this distance of the
/// segment that replaces it. Half a pixel is below the rasterizer's visible
/// error at an antialiased edge and removes most tile-boundary vertices that
/// carry no shape.
pub const LOW_ZOOM_SIMPLIFICATION_TOLERANCE_PIXELS: f64 = 0.5;

/// The smallest screen-space extent, in CSS pixels, a low-zoom styled feature
/// may have and still draw. A fill or line whose bounding box is thinner than
/// this covers less than one pixel and paints as a tint at best; culling it
/// removes thousands of tiny rings from a world-scale frame without changing
/// the visible map.
pub const LOW_ZOOM_MINIMUM_FEATURE_EXTENT_PIXELS: f64 = 1.0;

/// The fewest points a simplified ring may keep; fewer is no longer an area.
const SIMPLIFIED_RING_MINIMUM_POINT_COUNT: usize = 3;

/// The fewest points a simplified line may keep; fewer is no longer a line.
const SIMPLIFIED_LINE_MINIMUM_POINT_COUNT: usize = 2;

/// Applies low-zoom level of detail to styled vector-base overlay layers in
/// place: every polygon ring and styled line is simplified to
/// [`LOW_ZOOM_SIMPLIFICATION_TOLERANCE_PIXELS`] of screen space, and a feature
/// whose extent falls below [`LOW_ZOOM_MINIMUM_FEATURE_EXTENT_PIXELS`] is
/// dropped. The tolerance is evaluated at `style_zoom_level`, the zoom the
/// style bucket already paints at, so the output is deterministic per bucket
/// and the per-tile styled cache holds the reduced geometry.
///
/// At or above [`LOW_ZOOM_GEOMETRY_SIMPLIFICATION_MAXIMUM_ZOOM_LEVEL`] the
/// layers are returned untouched. Point markers, pins and traffic-flow lines
/// are caller overlays, never styled tile geometry, and are also untouched.
pub fn simplify_overlay_layers_for_style_zoom(
    layer_overlays: &mut [Vec<MapOverlay>],
    style_zoom_level: f64,
) {
    if !style_zoom_level.is_finite()
        || style_zoom_level > LOW_ZOOM_GEOMETRY_SIMPLIFICATION_MAXIMUM_ZOOM_LEVEL
    {
        return;
    }
    let pixels_per_world_unit = zoom_scale_pixels(style_zoom_level);
    if !pixels_per_world_unit.is_finite() || pixels_per_world_unit <= 0.0 {
        return;
    }
    let tolerance_world_units = LOW_ZOOM_SIMPLIFICATION_TOLERANCE_PIXELS / pixels_per_world_unit;
    let minimum_extent_world_units = LOW_ZOOM_MINIMUM_FEATURE_EXTENT_PIXELS / pixels_per_world_unit;
    for overlays in layer_overlays.iter_mut() {
        overlays.retain_mut(|overlay| {
            simplify_low_zoom_overlay(overlay, tolerance_world_units, minimum_extent_world_units)
        });
    }
}

/// Simplifies one overlay; `false` means the feature is sub-pixel and the
/// caller drops it.
fn simplify_low_zoom_overlay(
    overlay: &mut MapOverlay,
    tolerance_world_units: f64,
    minimum_extent_world_units: f64,
) -> bool {
    match overlay {
        MapOverlay::RoutePolyline(route_polyline) => {
            let world_chain = world_chain(&route_polyline.coordinates);
            if chain_extent(&world_chain) < minimum_extent_world_units {
                return false;
            }
            let simplified = douglas_peucker(&world_chain, tolerance_world_units);
            if simplified.len() < SIMPLIFIED_LINE_MINIMUM_POINT_COUNT {
                return false;
            }
            route_polyline.coordinates = simplified
                .into_iter()
                .map(normalized_world_to_coordinate)
                .collect();
            true
        }
        MapOverlay::FilledPolygon(filled_polygon) => {
            let world_ring = world_chain(&filled_polygon.ring_coordinates);
            if chain_extent(&world_ring) < minimum_extent_world_units {
                return false;
            }
            let simplified_ring = simplify_closed_ring(&world_ring, tolerance_world_units);
            let Some(simplified_ring) = simplified_ring else {
                return false;
            };
            filled_polygon.ring_coordinates = simplified_ring
                .into_iter()
                .map(normalized_world_to_coordinate)
                .collect();
            filled_polygon.hole_ring_coordinates = filled_polygon
                .hole_ring_coordinates
                .iter()
                .filter_map(|hole_ring| {
                    simplify_closed_ring(&world_chain(hole_ring), tolerance_world_units)
                })
                .map(|simplified_hole| {
                    simplified_hole
                        .into_iter()
                        .map(normalized_world_to_coordinate)
                        .collect()
                })
                .collect();
            true
        }
        MapOverlay::PointMarkers(_) | MapOverlay::PointPins(_) | MapOverlay::TrafficFlow(_) => true,
    }
}

/// The chain's coordinates in normalized-world space.
fn world_chain(coordinates: &[GeographicCoordinate]) -> Vec<[f64; 2]> {
    coordinates
        .iter()
        .map(coordinate_to_normalized_world)
        .collect()
}

/// The larger side of the chain's axis-aligned bounding box, in world units.
fn chain_extent(world_chain: &[[f64; 2]]) -> f64 {
    if world_chain.is_empty() {
        return 0.0;
    }
    let (minimum_x, maximum_x, minimum_y, maximum_y) = world_chain.iter().fold(
        (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ),
        |(minimum_x, maximum_x, minimum_y, maximum_y), point| {
            (
                minimum_x.min(point[0]),
                maximum_x.max(point[0]),
                minimum_y.min(point[1]),
                maximum_y.max(point[1]),
            )
        },
    );
    (maximum_x - minimum_x).max(maximum_y - minimum_y)
}

/// Simplifies a ring that may repeat its closing point, preserving traversal
/// order. `None` when the ring collapses below
/// [`SIMPLIFIED_RING_MINIMUM_POINT_COUNT`] points.
fn simplify_closed_ring(world_ring: &[[f64; 2]], tolerance: f64) -> Option<Vec<[f64; 2]>> {
    let mut open_ring = world_ring;
    if open_ring.len() > 1 && open_ring.first() == open_ring.last() {
        open_ring = &open_ring[..open_ring.len() - 1];
    }
    if open_ring.len() < SIMPLIFIED_RING_MINIMUM_POINT_COUNT {
        return None;
    }
    let mut ring_with_closure = open_ring.to_vec();
    ring_with_closure.push(open_ring[0]);
    let mut simplified = douglas_peucker(&ring_with_closure, tolerance);
    if simplified.len() > 1 && simplified.first() == simplified.last() {
        simplified.pop();
    }
    (simplified.len() >= SIMPLIFIED_RING_MINIMUM_POINT_COUNT).then_some(simplified)
}

/// Douglas-Peucker simplification of one open chain in world units. The
/// endpoints are kept, and any interior point farther than `tolerance` from
/// the segment joining its current neighbours is kept as well; every dropped
/// point is within `tolerance` of the replacement segments.
fn douglas_peucker(world_chain: &[[f64; 2]], tolerance: f64) -> Vec<[f64; 2]> {
    if world_chain.len() <= 2 {
        return world_chain.to_vec();
    }
    let tolerance_squared = tolerance * tolerance;
    let last_index = world_chain.len() - 1;
    let mut keep_point = vec![false; world_chain.len()];
    keep_point[0] = true;
    keep_point[last_index] = true;
    let mut segment_stack = vec![(0_usize, last_index)];
    while let Some((segment_start_index, segment_end_index)) = segment_stack.pop() {
        let mut farthest_point_index = 0;
        let mut farthest_distance_squared = 0.0;
        for point_index in (segment_start_index + 1)..segment_end_index {
            let distance_squared = squared_distance_to_segment(
                world_chain[point_index],
                world_chain[segment_start_index],
                world_chain[segment_end_index],
            );
            if distance_squared > farthest_distance_squared {
                farthest_distance_squared = distance_squared;
                farthest_point_index = point_index;
            }
        }
        if farthest_distance_squared > tolerance_squared && farthest_point_index != 0 {
            keep_point[farthest_point_index] = true;
            segment_stack.push((segment_start_index, farthest_point_index));
            segment_stack.push((farthest_point_index, segment_end_index));
        }
    }
    world_chain
        .iter()
        .zip(keep_point)
        .filter_map(|(point, keep)| keep.then_some(*point))
        .collect()
}

/// The squared distance from `point` to the segment between `segment_start`
/// and `segment_end`; a degenerate segment measures to its single point.
fn squared_distance_to_segment(
    point: [f64; 2],
    segment_start: [f64; 2],
    segment_end: [f64; 2],
) -> f64 {
    let segment_x = segment_end[0] - segment_start[0];
    let segment_y = segment_end[1] - segment_start[1];
    let segment_length_squared = segment_x * segment_x + segment_y * segment_y;
    if segment_length_squared == 0.0 {
        let offset_x = point[0] - segment_start[0];
        let offset_y = point[1] - segment_start[1];
        return offset_x * offset_x + offset_y * offset_y;
    }
    let projection = (((point[0] - segment_start[0]) * segment_x
        + (point[1] - segment_start[1]) * segment_y)
        / segment_length_squared)
        .clamp(0.0, 1.0);
    let nearest_x = segment_start[0] + projection * segment_x;
    let nearest_y = segment_start[1] + projection * segment_y;
    let offset_x = point[0] - nearest_x;
    let offset_y = point[1] - nearest_y;
    offset_x * offset_x + offset_y * offset_y
}

/// Assembles every overlay of a frame into one flat clip-space triangle list
/// drawn over the tile quads in the same captured frame.
///
/// The shapes project through the same camera pose as the tile quads, wrap
/// around the camera's world copy like the tile spans do (see the module
/// notes), and leave frustum clipping to the GPU's clip-space clip. An empty
/// `overlays` list produces no vertices, and a shape whose geometry turns
/// out degenerate (behind the camera for screen-thickened shapes, or
/// numerically unusable anywhere) skips only that shape's own triangles.
///
/// # Parameters
/// - `camera` — the pose the overlay shapes are projected through.
/// - `viewport` — the viewport the frame targets, in physical pixels.
/// - `field_of_view_degrees` — vertical field of view of the projection.
/// - `overlays` — the shapes to assemble, in draw order.
///
/// # Returns
/// One flat, non-indexed clip-space triangle list for the whole frame.
pub fn overlay_triangle_vertices(
    camera: &MapCamera,
    viewport: &ViewportPixels,
    field_of_view_degrees: f64,
    overlays: &[MapOverlay],
) -> Vec<ClipSpaceOverlayVertex> {
    overlay_triangle_vertices_for_layers(
        camera,
        viewport,
        field_of_view_degrees,
        &[overlays],
        UNSCALED_DEVICE_PIXEL_RATIO,
    )
}

/// Assembles several ordered overlay layers into one flat clip-space triangle
/// list, scaling every overlay's declared point/CSS screen sizes by
/// `device_pixel_ratio` while the shape is emitted rather than cloning a
/// pre-scaled copy of the list.
///
/// The layers draw in order: a style-derived vector base list first, then the
/// caller's own overlays over it — the composition a frame with an installed
/// vector base draws. A layer may be empty, and an empty layer list produces
/// no vertices. The geometry, wrapping, clipping and skip rules are exactly
/// [`overlay_triangle_vertices`]'s, whose single-list form is this function
/// with one layer at [`UNSCALED_DEVICE_PIXEL_RATIO`].
///
/// # Parameters
/// - `camera` — the pose the overlay shapes are projected through.
/// - `viewport` — the viewport the frame targets, in physical pixels.
/// - `field_of_view_degrees` — vertical field of view of the projection.
/// - `overlay_layers` — the ordered overlay lists to assemble.
/// - `device_pixel_ratio` — the drawing buffer's device pixels per point/CSS
///   pixel; route and traffic line widths, marker and pin radii and a styled
///   stroke's offset, gap and blur multiply by it.
///
/// # Returns
/// One flat, non-indexed clip-space triangle list for the whole frame.
pub fn overlay_triangle_vertices_for_layers(
    camera: &MapCamera,
    viewport: &ViewportPixels,
    field_of_view_degrees: f64,
    overlay_layers: &[&[MapOverlay]],
    device_pixel_ratio: f64,
) -> Vec<ClipSpaceOverlayVertex> {
    let frame_projection =
        OverlayFrameProjection::for_frame(camera, viewport, field_of_view_degrees);
    let mut vertices = Vec::new();
    for overlay in overlay_layers.iter().flat_map(|layer| layer.iter()) {
        match overlay {
            MapOverlay::RoutePolyline(route_polyline) => {
                let width_pixels = route_polyline.width_pixels * device_pixel_ratio;
                // The style's displacement is CSS pixels and rotates with the
                // camera when the layer anchors it to the map.
                let translate_pixels = translate_pixels_for_anchor(
                    route_polyline.translate_pixels,
                    route_polyline.translate_anchor,
                    frame_projection.map_bearing_degrees(),
                );
                let translate_pixels = [
                    translate_pixels[0] * device_pixel_ratio,
                    translate_pixels[1] * device_pixel_ratio,
                ];
                if let Some(stroke) = &route_polyline.stroke {
                    styled_polyline_triangles(
                        &frame_projection,
                        viewport,
                        &route_polyline.coordinates,
                        route_polyline.rgba_color,
                        width_pixels,
                        stroke,
                        translate_pixels,
                        device_pixel_ratio,
                        &mut vertices,
                    );
                } else {
                    thickened_polyline_triangles(
                        &frame_projection,
                        viewport,
                        &route_polyline.coordinates,
                        route_polyline.rgba_color,
                        width_pixels,
                        translate_pixels,
                        &mut vertices,
                    );
                }
            }
            MapOverlay::FilledPolygon(filled_polygon) => {
                // A filled polygon carries no screen-space size to scale.
                filled_polygon_triangles(
                    &frame_projection,
                    viewport,
                    &filled_polygon.ring_coordinates,
                    &filled_polygon.hole_ring_coordinates,
                    FilledPolygonStyle {
                        rgba_color: filled_polygon.rgba_color,
                        rgba_outline_color: filled_polygon.rgba_outline_color,
                        antialias: filled_polygon.antialias,
                        translate_pixels: filled_polygon.translate_pixels,
                        translate_anchor: filled_polygon.translate_anchor,
                    },
                    &mut vertices,
                );
            }
            MapOverlay::PointMarkers(point_markers) => {
                let marker_style = PointMarkerStyle {
                    rgba_color: point_markers.rgba_color,
                    radius_pixels: point_markers.radius_pixels * device_pixel_ratio,
                    rgba_stroke_color: point_markers.rgba_stroke_color,
                    stroke_width_pixels: point_markers.stroke_width_pixels,
                    blur_pixels: point_markers.blur_pixels,
                    translate_pixels: point_markers.translate_pixels,
                    pitch_scale: point_markers.pitch_scale,
                    pitch_alignment: point_markers.pitch_alignment,
                };
                for position in &point_markers.positions {
                    point_marker_triangles(
                        &frame_projection,
                        viewport,
                        position,
                        &marker_style,
                        &mut vertices,
                    );
                }
            }
            MapOverlay::PointPins(point_pins) => {
                let radius_pixels = point_pins.radius_pixels * device_pixel_ratio;
                for position in &point_pins.positions {
                    point_pin_triangles(
                        &frame_projection,
                        viewport,
                        position,
                        point_pins.rgba_color,
                        radius_pixels,
                        &mut vertices,
                    );
                }
            }
            MapOverlay::TrafficFlow(traffic_flow) => {
                let width_pixels = traffic_flow.width_pixels * device_pixel_ratio;
                for traffic_flow_data in &traffic_flow.flows {
                    thickened_polyline_triangles(
                        &frame_projection,
                        viewport,
                        &traffic_flow_data.shape_coordinates,
                        traffic_congestion_rgba(traffic_flow_data.congestion_level),
                        width_pixels,
                        [0.0, 0.0],
                        &mut vertices,
                    );
                }
            }
        }
    }
    vertices
}

/// Whether any overlay needs the camera-space assembly, which the world-space
/// cached tessellation cannot express: a styled line stroke (its shader knows
/// only the default square join), a translated fill, or a circle whose pitch
/// behaviour is ground-based.
///
/// A caller that caches world-space geometry across frames must ask this
/// before tessellating: when it answers `true`, assemble the whole list with
/// [`overlay_triangle_vertices`] instead, which honours caps, joins, dashes,
/// offsets, casing gaps, blur, translations and ground-aligned circles.
pub fn overlays_require_camera_space_strokes(overlays: &[MapOverlay]) -> bool {
    overlays.iter().any(|overlay| match overlay {
        MapOverlay::RoutePolyline(route_polyline) => route_polyline.stroke.is_some(),
        MapOverlay::FilledPolygon(filled_polygon) => filled_polygon.translate_pixels != [0.0, 0.0],
        MapOverlay::PointMarkers(point_markers) => {
            point_markers.pitch_scale != CirclePitchScale::Viewport
                || point_markers.pitch_alignment != CirclePitchAlignment::Viewport
        }
        MapOverlay::PointPins(_) | MapOverlay::TrafficFlow(_) => false,
    })
}

/// Wraps a shape's coordinates into one continuous normalized-world chain:
/// the first vertex is placed at `reference_x + shortest wrapped delta` from
/// `reference_x`, and every following vertex unwraps from its predecessor by
/// the shortest wrapped delta, so chains crossing the antimeridian continue
/// past the world-copy boundary instead of tearing back across the viewport
/// (the same
/// wrap-as-one-unit rule the tile spans follow).
///
/// The camera-anchored overlay assembly passes the camera's normalized-world x
/// as the reference; the cached world-space tessellation passes a
/// content-defined origin so the result is independent of the camera pose.
fn unwrapped_normalized_chain(
    reference_x: f64,
    coordinates: &[GeographicCoordinate],
) -> Vec<[f64; 2]> {
    let mut points = Vec::with_capacity(coordinates.len());
    let mut previous_x: Option<f64> = None;
    for coordinate in coordinates {
        let normalized_world = coordinate_to_normalized_world(coordinate);
        let wrapped_x = match previous_x {
            None => reference_x + normalized_shortest_delta(reference_x, normalized_world[0]),
            Some(previous_x_value) => {
                previous_x_value + normalized_shortest_delta(previous_x_value, normalized_world[0])
            }
        };
        points.push([wrapped_x, normalized_world[1]]);
        previous_x = Some(wrapped_x);
    }
    points
}

/// Per-frame projection context for overlays: the view-projection matrix, the
/// world pixel scale, and the camera's normalized-world x, mirroring the tile
/// renderer's frame projection (see `crate::renderer::FrameProjection`) so
/// both layers resolve the same world-position convention per frame. Shared
/// (crate-visible) with the location-indicator layer, which projects through
/// the same conventions.
pub(crate) struct OverlayFrameProjection {
    view_projection_matrix: DMat4,
    zoom_scale_pixels: f64,
    center_normalized_x: f64,
    map_bearing_degrees: f64,
}

impl OverlayFrameProjection {
    /// Builds the projection context for one frame.
    pub(crate) fn for_frame(
        camera: &MapCamera,
        viewport: &ViewportPixels,
        field_of_view_degrees: f64,
    ) -> Self {
        Self {
            view_projection_matrix: camera.view_projection_matrix(viewport, field_of_view_degrees),
            zoom_scale_pixels: zoom_scale_pixels(camera.zoom_level),
            center_normalized_x: coordinate_to_normalized_world(&camera.center)[0],
            map_bearing_degrees: camera.bearing_degrees,
        }
    }

    /// The camera's bearing, in degrees, for a map-anchored displacement.
    pub(crate) fn map_bearing_degrees(&self) -> f64 {
        self.map_bearing_degrees
    }

    /// The wrapped normalized-world x of a raw normalized-world x: the
    /// position in `(-0.5, 0.5]` around the camera's world copy, shared with
    /// the tile spans' wrapping rule.
    pub(crate) fn wrapped_x(&self, normalized_x: f64) -> f64 {
        self.center_normalized_x + normalized_shortest_delta(self.center_normalized_x, normalized_x)
    }

    /// Wraps a shape's coordinates into one continuous normalized-world chain:
    /// the first vertex sits in the camera's world copy and every following
    /// vertex unwraps from its predecessor by the shortest wrapped delta, so
    /// chains crossing the antimeridian continue past the world-copy boundary
    /// instead of tearing back across the viewport (the same wrap-as-one-unit
    /// rule
    /// the tile spans follow).
    fn wrapped_continuous_chain(&self, coordinates: &[GeographicCoordinate]) -> Vec<[f64; 2]> {
        self.wrapped_continuous_chain_near(self.center_normalized_x, coordinates)
    }

    /// [`Self::wrapped_continuous_chain`] anchored on a caller-supplied
    /// normalized-world x rather than the camera center. A hole ring uses its
    /// outer ring's first wrapped vertex, so both rings land in the same world
    /// copy even when the polygon straddles the antimeridian and the camera
    /// center alone would resolve the hole to the neighboring copy.
    fn wrapped_continuous_chain_near(
        &self,
        reference_x: f64,
        coordinates: &[GeographicCoordinate],
    ) -> Vec<[f64; 2]> {
        unwrapped_normalized_chain(reference_x, coordinates)
    }

    /// The camera-space clip position of a ground point of one world-copy
    /// window. Camera world space is y-north pixel space (see
    /// `MapCamera::ground_position_vector`); the window's copy offset is
    /// subtracted from the wrapped x so the fragment continues the camera's
    /// own copy past the window edge, exactly as the tile fragments do.
    pub(crate) fn ground_clip_position(&self, window_offset: f64, x: f64, y: f64) -> DVec4 {
        let x_pixels = (x - window_offset) * self.zoom_scale_pixels;
        let y_pixels = -y * self.zoom_scale_pixels;
        self.view_projection_matrix * DVec4::new(x_pixels, y_pixels, 0.0, 1.0)
    }

    /// The wrapped normalized-world x-range one world-copy window spans:
    /// `[window_start_x, window_end_x)` around the camera's world copy,
    /// shared by the segment fragments' split rule and the location
    /// indicator's per-window anchor containment.
    pub(crate) fn window_x_range(&self, window_offset: f64) -> [f64; 2] {
        let window_start_x =
            self.center_normalized_x - CAMERA_WORLD_WINDOW_HALF_WIDTH + window_offset;
        [
            window_start_x,
            window_start_x + 2.0 * CAMERA_WORLD_WINDOW_HALF_WIDTH,
        ]
    }
}

/// Divides a clip position to viewport pixel coordinates (origin top-left, y
/// down), matching [`MapCamera::project_to_screen`]. Sound only for a
/// positive homogeneous weight; callers check that first where screen-space
/// math follows.
pub(crate) fn clip_position_to_screen_pixels(
    clip_position: DVec4,
    viewport: &ViewportPixels,
) -> [f64; 2] {
    let normalized_device_x = clip_position.x / clip_position.w;
    let normalized_device_y = clip_position.y / clip_position.w;
    [
        (normalized_device_x + 1.0) * 0.5 * viewport.width_pixels,
        (1.0 - (normalized_device_y + 1.0) * 0.5) * viewport.height_pixels,
    ]
}

/// Re-expands a screen-pixel offset of one clip-space reference point into a
/// clip position carrying the reference's depth and homogeneous weight. The
/// screen offset is applied in normalized-device space around the reference's
/// divided position, so per-vertex interpolation across a thickened quad ends
/// at exactly the offset screen corners; the quad's interior follows the
/// same perspective weighting as the underlying ground line.
pub(crate) fn screen_pixels_to_clip_position(
    screen_pixels: [f64; 2],
    reference_clip_position: DVec4,
    viewport: &ViewportPixels,
) -> [f64; 4] {
    let normalized_device_x = 2.0 * screen_pixels[0] / viewport.width_pixels - 1.0;
    let normalized_device_y = 1.0 - 2.0 * screen_pixels[1] / viewport.height_pixels;
    [
        normalized_device_x * reference_clip_position.w,
        normalized_device_y * reference_clip_position.w,
        reference_clip_position.z,
        reference_clip_position.w,
    ]
}

/// Pushes one solid-color triangle, skipping it entirely when any corner is
/// non-finite: an f32-overflowing corner would poison the whole draw with
/// undefined geometry, while skipping keeps the rest of the overlay healthy.
pub(crate) fn push_triangle(
    vertices: &mut Vec<ClipSpaceOverlayVertex>,
    corners: &[[f64; 4]; 3],
    rgba_color: [f32; 4],
) {
    if corners
        .iter()
        .any(|corner| corner.iter().any(|component| !component.is_finite()))
    {
        return;
    }
    for corner in corners {
        vertices.push(ClipSpaceOverlayVertex {
            clip_position: [
                corner[0] as f32,
                corner[1] as f32,
                corner[2] as f32,
                corner[3] as f32,
            ],
            rgba_color,
        });
    }
}

/// Number of world-copy instances a world-space overlay frame draws: the
/// camera's own copy plus two neighbours on each side. The cached tessellation
/// stores each shape anchored within half a world of the layer's origin, and
/// the frame folds the origin's own camera copy into the uniform, so the CPU
/// assembly's window offsets (the camera's copy and one neighbour either side)
/// map onto copies `-2..=2` even when a shape's first vertex sits in a
/// different copy from the layer origin.
pub const WORLD_SPACE_OVERLAY_WORLD_COPY_INSTANCES: u32 = 5;

/// The normalized-world x copy offsets one world-space overlay frame draws,
/// packed into [`WorldSpaceOverlayFrameUniforms::world_copy_offsets`] and
/// indexed by the vertex shader's `instance_index`.
const WORLD_SPACE_OVERLAY_WORLD_COPY_OFFSETS: [f32; WORLD_SPACE_OVERLAY_WORLD_COPY_INSTANCES
    as usize] = [-2.0, -1.0, 0.0, 1.0, 2.0];

/// Shape kinds a [`WorldSpaceOverlayVertex`] encodes; the world-space overlay
/// shader dispatches its vertex expansion on this value.
///
/// * `DIRECT` — `world_position` is the geometry's own ground point (filled
///   polygon corners); the shader projects it unchanged.
/// * `ANCHORED` — `world_position` is a screen-facing shape's anchor;
///   `shape_parameters` is the vertex's offset from the projected anchor in
///   screen pixels (markers and pins).
/// * `SEGMENT` — `world_position` is one endpoint of a thickened segment and
///   `reference_position` the other; `shape_parameters` carries the direction
///   sign and the signed half-width in pixels.
/// * `JOIN` — `world_position` is a polyline bend, `reference_position` the
///   previous vertex, `next_position` the following one; `shape_parameters`
///   carries the corner's bisector/perpendicular coefficients in pixels.
const WORLD_SPACE_OVERLAY_SHAPE_DIRECT: u32 = 0;
const WORLD_SPACE_OVERLAY_SHAPE_ANCHORED: u32 = 1;
const WORLD_SPACE_OVERLAY_SHAPE_SEGMENT: u32 = 2;
const WORLD_SPACE_OVERLAY_SHAPE_JOIN: u32 = 3;

/// One vertex of the cached world-space overlay layer: ground geometry in
/// normalized-world units relative to [`WorldSpaceOverlayGeometry`]'s origin,
/// plus the screen-space parameters the overlay vertex shader expands with
/// the frame uniform's view-projection, viewport and pixel scale.
///
/// `repr(C)` fixes the field order the GPU vertex buffer layout expects. Every
/// component stays `f32`; positions are stored as offsets from a
/// content-defined origin (the layer's first coordinate) so a cached layer
/// keeps sub-pixel precision across a pan without re-tessellation.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldSpaceOverlayVertex {
    /// Ground position offset from the layer origin, normalized world units
    /// (x east, y south).
    pub world_position: [f32; 2],
    /// Secondary ground offset: a segment's other endpoint, a join's previous
    /// vertex, or zero for the other shape kinds.
    pub reference_position: [f32; 2],
    /// Tertiary ground offset: a join's following vertex, else zero.
    pub next_position: [f32; 2],
    /// One of the `WORLD_SPACE_OVERLAY_SHAPE_*` kinds.
    pub shape_kind: u32,
    /// Kind-specific screen-pixel parameters (see the kind constants).
    pub shape_parameters: [f32; 2],
    /// Solid RGBA fill color, components in `[0, 1]`.
    pub rgba_color: [f32; 4],
}

impl WorldSpaceOverlayVertex {
    /// Bytes one vertex occupies in the packed vertex buffer.
    pub const BYTES: usize = 13 * std::mem::size_of::<f32>();
}

// The packed encoder and the GPU's `array_stride` must agree with the field
// layout, so a future field addition cannot silently misalign vertices.
const _: () =
    assert!(std::mem::size_of::<WorldSpaceOverlayVertex>() == WorldSpaceOverlayVertex::BYTES);

/// A camera-independent tessellation of the overlay list in normalized world
/// space, ready to be uploaded once and drawn every frame with
/// [`WorldSpaceOverlayFrameUniforms`].
///
/// The layer is cached across frames while the overlay content is unchanged:
/// panning, rotation and zoom change only the frame uniform (the view
/// projection, viewport extent and the origin's world-copy placement), never
/// this geometry. Screen-space line widths, joins, marker radii and pin
/// outlines are authored as per-vertex pixel parameters and expanded in the
/// vertex shader, so they still follow the CPU assembly's screen-pixel math
/// exactly while the tessellation stays camera-free.
///
/// World-copy handling moves to the frame path too: the shader recenters each
/// vertex by the drawn instance's copy offset and discards fragments outside
/// the camera's own world-copy window, reproducing the CPU assembly's window
/// split and antimeridian wrap without re-tessellating.
#[derive(Debug, Clone, PartialEq)]
pub struct WorldSpaceOverlayGeometry {
    /// Normalized-world origin every stored position is relative to.
    origin_normalized_world: [f64; 2],
    /// Flat, non-indexed triangle list.
    vertices: Vec<WorldSpaceOverlayVertex>,
}

impl WorldSpaceOverlayGeometry {
    /// Tessellates every overlay into one flat world-space triangle list.
    /// The origin is the first coordinate of the first overlay that carries
    /// one (zero for an empty list), so the cached geometry is independent of
    /// the camera pose.
    pub fn tessellate(overlays: &[MapOverlay]) -> Self {
        let origin_normalized_world =
            first_overlay_normalized_world(overlays).unwrap_or([0.0, 0.0]);
        let mut tessellation = WorldSpaceTessellation {
            origin_normalized_world,
            vertices: Vec::new(),
        };
        for overlay in overlays {
            match overlay {
                MapOverlay::RoutePolyline(route_polyline) => tessellation.push_polyline(
                    &route_polyline.coordinates,
                    route_polyline.rgba_color,
                    route_polyline.width_pixels,
                ),
                // `push_polyline` draws the default square-join stroke, so a
                // styled stroke reaching here (a caller that ignored
                // [`overlays_require_camera_space_strokes`]) still paints its
                // line, only without the declared caps, joins and dashes.
                MapOverlay::FilledPolygon(filled_polygon) => {
                    tessellation.push_filled_polygon(filled_polygon)
                }
                MapOverlay::PointMarkers(point_markers) => {
                    tessellation.push_point_markers(point_markers)
                }
                MapOverlay::PointPins(point_pins) => tessellation.push_point_pins(
                    &point_pins.positions,
                    point_pins.rgba_color,
                    point_pins.radius_pixels,
                ),
                MapOverlay::TrafficFlow(traffic_flow) => {
                    for flow in &traffic_flow.flows {
                        tessellation.push_polyline(
                            &flow.shape_coordinates,
                            traffic_congestion_rgba(flow.congestion_level),
                            traffic_flow.width_pixels,
                        );
                    }
                }
            }
        }
        Self {
            origin_normalized_world,
            vertices: tessellation.vertices,
        }
    }

    /// The normalized-world origin stored positions are relative to.
    pub fn origin_normalized_world(&self) -> [f64; 2] {
        self.origin_normalized_world
    }

    /// The flat, non-indexed world-space triangle list.
    pub fn vertices(&self) -> &[WorldSpaceOverlayVertex] {
        &self.vertices
    }

    /// Number of vertices in the triangle list.
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// Number of triangles the list draws.
    pub fn triangle_count(&self) -> usize {
        self.vertices.len() / OVERLAY_TRIANGLE_VERTEX_COUNT
    }

    /// Whether the layer draws nothing.
    pub fn is_empty(&self) -> bool {
        self.vertices.is_empty()
    }
}

/// Per-frame uniform of the world-space overlay shader: everything that
/// changes when the camera moves, while [`WorldSpaceOverlayGeometry`] stays
/// cached.
///
/// The layout is the WGSL `FrameUniforms` struct: the camera anchor's clip
/// position, the view-projection's linear part (translation removed) scaled by
/// the zoom's world-pixel scale, the layer origin relative to the camera, the
/// viewport size, and the world-copy offsets the draw's instances index.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldSpaceOverlayFrameUniforms {
    /// Clip position of the camera's own normalized-world center (its anchor
    /// ground point), computed in `f64` on the CPU.
    pub anchor_clip_position: [f32; 4],
    /// Linear part of the view-projection (translation column zeroed) with its
    /// x and y columns scaled by [`crate::projection::zoom_scale_pixels`];
    /// column-major, matching WGSL's `mat4x4<f32>`.
    pub relative_view_projection: [[f32; 4]; 4],
    /// The layer origin's nearest world copy relative to the camera in x, and
    /// the camera-to-origin y delta, both normalized world units. The shader
    /// subtracts each vertex's stored offset from this to recover the
    /// camera-relative ground position.
    pub origin_relative_normalized: [f32; 2],
    /// Viewport extent in physical pixels.
    pub viewport_size_pixels: [f32; 2],
    /// The world-copy offsets the draw's instances select, packed two `vec4`s
    /// deep (`[c0, c1, c2, c3]`, `[c4, 0, 0, 0]`).
    pub world_copy_offsets: [[f32; 4]; 2],
}

impl WorldSpaceOverlayFrameUniforms {
    /// Builds the frame uniform for a camera pose and a cached layer origin.
    ///
    /// The anchor clip position and the relative matrix are computed in `f64`
    /// and only the final `f32` uniform crosses to the GPU; the shader then
    /// adds a small camera-relative offset to that anchor, so the projection
    /// keeps its precision even when the zoom-scaled world coordinates are
    /// large (the same camera-relative trick the tile quads achieve by
    /// projecting on the CPU).
    pub fn for_frame(
        camera: &MapCamera,
        viewport: &ViewportPixels,
        field_of_view_degrees: f64,
        origin_normalized_world: [f64; 2],
    ) -> Self {
        let view_projection = camera.view_projection_matrix(viewport, field_of_view_degrees);
        let zoom_scale = zoom_scale_pixels(camera.zoom_level);
        let center_normalized_world = coordinate_to_normalized_world(&camera.center);
        let anchor_world = DVec4::new(
            center_normalized_world[0] * zoom_scale,
            -center_normalized_world[1] * zoom_scale,
            0.0,
            1.0,
        );
        let anchor_clip = view_projection * anchor_world;

        // The mapping from camera-relative normalized-world offsets to clip
        // offsets: the view-projection's linear columns, with x and y scaled
        // by the zoom's pixels-per-world-unit. The z and translation columns
        // never contribute (all ground points sit on z = 0).
        let mut relative_view_projection = DMat4::ZERO;
        relative_view_projection.x_axis = view_projection.x_axis * zoom_scale;
        relative_view_projection.y_axis = view_projection.y_axis * zoom_scale;

        let origin_relative_normalized = [
            normalized_shortest_delta(center_normalized_world[0], origin_normalized_world[0]),
            center_normalized_world[1] - origin_normalized_world[1],
        ];

        Self {
            anchor_clip_position: [
                anchor_clip.x as f32,
                anchor_clip.y as f32,
                anchor_clip.z as f32,
                anchor_clip.w as f32,
            ],
            relative_view_projection: [
                relative_view_projection
                    .x_axis
                    .to_array()
                    .map(|component| component as f32),
                relative_view_projection
                    .y_axis
                    .to_array()
                    .map(|component| component as f32),
                relative_view_projection
                    .z_axis
                    .to_array()
                    .map(|component| component as f32),
                relative_view_projection
                    .w_axis
                    .to_array()
                    .map(|component| component as f32),
            ],
            origin_relative_normalized: [
                origin_relative_normalized[0] as f32,
                origin_relative_normalized[1] as f32,
            ],
            viewport_size_pixels: [viewport.width_pixels as f32, viewport.height_pixels as f32],
            world_copy_offsets: [
                [
                    WORLD_SPACE_OVERLAY_WORLD_COPY_OFFSETS[0],
                    WORLD_SPACE_OVERLAY_WORLD_COPY_OFFSETS[1],
                    WORLD_SPACE_OVERLAY_WORLD_COPY_OFFSETS[2],
                    WORLD_SPACE_OVERLAY_WORLD_COPY_OFFSETS[3],
                ],
                [WORLD_SPACE_OVERLAY_WORLD_COPY_OFFSETS[4], 0.0, 0.0, 0.0],
            ],
        }
    }
}

/// The first coordinate any overlay carries, in normalized world space: the
/// content-defined origin that keeps cached geometry camera-independent. The
/// scan follows the overlay list order, so the origin itself is deterministic.
fn first_overlay_normalized_world(overlays: &[MapOverlay]) -> Option<[f64; 2]> {
    for overlay in overlays {
        let first_coordinate = match overlay {
            MapOverlay::RoutePolyline(route_polyline) => route_polyline.coordinates.first(),
            MapOverlay::FilledPolygon(filled_polygon) => filled_polygon.ring_coordinates.first(),
            MapOverlay::PointMarkers(point_markers) => point_markers.positions.first(),
            MapOverlay::PointPins(point_pins) => point_pins.positions.first(),
            MapOverlay::TrafficFlow(traffic_flow) => traffic_flow
                .flows
                .iter()
                .find_map(|flow| flow.shape_coordinates.first()),
        };
        if let Some(coordinate) = first_coordinate {
            return Some(coordinate_to_normalized_world(coordinate));
        }
    }
    None
}

/// Mutable assembly state for [`WorldSpaceOverlayGeometry::tessellate`]:
/// converts each shape's coordinates into origin-relative normalized-world
/// positions and emits the shader's vertex vocabulary.
struct WorldSpaceTessellation {
    origin_normalized_world: [f64; 2],
    vertices: Vec<WorldSpaceOverlayVertex>,
}

impl WorldSpaceTessellation {
    /// Wraps a shape's coordinates into a continuous normalized-world chain
    /// whose first vertex sits within half a world of the layer origin, then
    /// stores every point as an offset from that origin.
    fn anchored_chain(&self, coordinates: &[GeographicCoordinate]) -> Vec<[f64; 2]> {
        let mut chain = unwrapped_normalized_chain(self.origin_normalized_world[0], coordinates);
        self.relativize(&mut chain);
        chain
    }

    /// [`Self::anchored_chain`] anchored near a caller-supplied stored x, so a
    /// polygon's hole rings land in the same world copy as its outer ring.
    fn anchored_chain_near(
        &self,
        reference_x: f64,
        coordinates: &[GeographicCoordinate],
    ) -> Vec<[f64; 2]> {
        let mut chain =
            unwrapped_normalized_chain(self.origin_normalized_world[0] + reference_x, coordinates);
        self.relativize(&mut chain);
        chain
    }

    /// One anchored position, the chain rule for a single-coordinate shape.
    fn anchored_position(&self, coordinate: &GeographicCoordinate) -> [f64; 2] {
        let normalized_world = coordinate_to_normalized_world(coordinate);
        let anchored_x = self.origin_normalized_world[0]
            + normalized_shortest_delta(self.origin_normalized_world[0], normalized_world[0]);
        [
            anchored_x - self.origin_normalized_world[0],
            normalized_world[1] - self.origin_normalized_world[1],
        ]
    }

    fn relativize(&self, chain: &mut [[f64; 2]]) {
        for point in chain {
            point[0] -= self.origin_normalized_world[0];
            point[1] -= self.origin_normalized_world[1];
        }
    }

    fn push_vertex(
        &mut self,
        world_position: [f64; 2],
        reference_position: [f64; 2],
        next_position: [f64; 2],
        shape_kind: u32,
        shape_parameters: [f64; 2],
        rgba_color: [f32; 4],
    ) {
        self.vertices.push(WorldSpaceOverlayVertex {
            world_position: [world_position[0] as f32, world_position[1] as f32],
            reference_position: [reference_position[0] as f32, reference_position[1] as f32],
            next_position: [next_position[0] as f32, next_position[1] as f32],
            shape_kind,
            shape_parameters: [shape_parameters[0] as f32, shape_parameters[1] as f32],
            rgba_color,
        });
    }

    /// Emits a thick polyline exactly like the CPU assembly: one quad per
    /// segment (two triangles, split `0,1,2 / 2,1,3`) followed by one square
    /// join per interior vertex. The width and join direction stay as
    /// screen-pixel parameters; the shader resolves them per frame.
    fn push_polyline(
        &mut self,
        coordinates: &[GeographicCoordinate],
        rgba_color: [f32; 4],
        width_pixels: f64,
    ) {
        let points = self.anchored_chain(coordinates);
        let half_width_pixels = width_pixels * 0.5;
        for segment in points.windows(2) {
            let start = segment[0];
            let end = segment[1];
            if start == end {
                // A duplicated coordinate thickens to an empty quad on the
                // CPU; skipping it keeps the triangle tally identical.
                continue;
            }
            // Quad corners: start-offset, start-opposite, end-offset,
            // end-opposite. The direction sign makes the shader's segment
            // direction always run start to end from either endpoint.
            let corners = [
                (start, end, 1.0, half_width_pixels),
                (start, end, 1.0, -half_width_pixels),
                (end, start, -1.0, half_width_pixels),
                (end, start, -1.0, -half_width_pixels),
            ];
            for triangle in [[0_usize, 1, 2], [2, 1, 3]] {
                for corner_index in triangle {
                    let (world_position, other_endpoint, direction_sign, side_offset) =
                        corners[corner_index];
                    self.push_vertex(
                        world_position,
                        other_endpoint,
                        [0.0; 2],
                        WORLD_SPACE_OVERLAY_SHAPE_SEGMENT,
                        [direction_sign, side_offset],
                        rgba_color,
                    );
                }
            }
        }
        for bend_index in 1..points.len().saturating_sub(1) {
            let previous = points[bend_index - 1];
            let bend = points[bend_index];
            let next = points[bend_index + 1];
            // Corner coefficients of the CPU join square, in its emission
            // order: (+bisector, +perpendicular), (-, +), (-, -), (+, -).
            let corner_coefficients = [(1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)];
            for triangle in [[0_usize, 1, 2], [0, 2, 3]] {
                for corner_index in triangle {
                    let (bisector_coefficient, perpendicular_coefficient) =
                        corner_coefficients[corner_index];
                    self.push_vertex(
                        bend,
                        previous,
                        next,
                        WORLD_SPACE_OVERLAY_SHAPE_JOIN,
                        [
                            bisector_coefficient * half_width_pixels,
                            perpendicular_coefficient * half_width_pixels,
                        ],
                        rgba_color,
                    );
                }
            }
        }
    }

    /// Emits a filled polygon's ear-clip triangles as direct world-space
    /// vertices. The ear clip runs on the same continuous origin-anchored ring
    /// (holes spliced and anchored to the outer ring) as the CPU assembly, so
    /// the triangle stream, holes included, is identical.
    fn push_filled_polygon(&mut self, filled_polygon: &FilledPolygonOverlay) {
        let wrapped_ring = self.anchored_chain(&filled_polygon.ring_coordinates);
        let Some(reference_x) = wrapped_ring.first().map(|vertex| vertex[0]) else {
            return;
        };
        let wrapped_holes: Vec<Vec<[f64; 2]>> = filled_polygon
            .hole_ring_coordinates
            .iter()
            .map(|hole_ring| self.anchored_chain_near(reference_x, hole_ring))
            .collect();
        for triangle in ear_clip_triangulation_with_holes(&wrapped_ring, &wrapped_holes) {
            for corner in triangle {
                self.push_vertex(
                    corner,
                    [0.0; 2],
                    [0.0; 2],
                    WORLD_SPACE_OVERLAY_SHAPE_DIRECT,
                    [0.0; 2],
                    filled_polygon.rgba_color,
                );
            }
        }
        // The predicate keeps a translated fill off this cached path, so the
        // outline below needs no screen displacement.
        if filled_polygon.antialias
            && let Some(outline_rgba) = filled_polygon.rgba_outline_color
        {
            let mut closed_ring = filled_polygon.ring_coordinates.clone();
            if let (Some(first), Some(last)) = (closed_ring.first(), closed_ring.last())
                && first != last
            {
                closed_ring.push(*first);
            }
            if closed_ring.len() >= 2 {
                self.push_polyline(&closed_ring, outline_rgba, FILL_OUTLINE_WIDTH_PIXELS);
            }
            for hole_ring in &filled_polygon.hole_ring_coordinates {
                let mut closed_hole = hole_ring.clone();
                if let (Some(first), Some(last)) = (closed_hole.first(), closed_hole.last())
                    && first != last
                {
                    closed_hole.push(*first);
                }
                if closed_hole.len() >= 2 {
                    self.push_polyline(&closed_hole, outline_rgba, FILL_OUTLINE_WIDTH_PIXELS);
                }
            }
        }
    }

    /// Emits center-anchored octagon marker fans: every vertex carries the
    /// anchor's world position and its offset from the projected center in
    /// screen pixels.
    fn push_point_markers(&mut self, point_markers: &PointMarkersOverlay) {
        let radius_pixels = point_markers.radius_pixels;
        if !radius_pixels.is_finite() || radius_pixels <= 0.0 {
            return;
        }
        for position in &point_markers.positions {
            let anchor = self.anchored_position(position);
            if point_markers.blur_pixels > 0.0 {
                let mut fringe_rgba = point_markers.rgba_color;
                fringe_rgba[3] *= LINE_BLUR_FRINGE_ALPHA_SCALE;
                self.push_anchored_marker_fan(
                    anchor,
                    0.0,
                    radius_pixels + point_markers.blur_pixels,
                    point_markers.translate_pixels,
                    fringe_rgba,
                );
            }
            self.push_anchored_marker_fan(
                anchor,
                0.0,
                radius_pixels,
                point_markers.translate_pixels,
                point_markers.rgba_color,
            );
            if let Some(stroke_rgba) = point_markers.rgba_stroke_color
                && point_markers.stroke_width_pixels > 0.0
            {
                self.push_anchored_marker_fan(
                    anchor,
                    radius_pixels,
                    radius_pixels + point_markers.stroke_width_pixels,
                    point_markers.translate_pixels,
                    stroke_rgba,
                );
            }
        }
    }

    /// Emits one screen-facing marker fan between an inner and an outer
    /// radius, every corner carrying the anchor's world position and a screen
    /// offset, which the shader resolves per frame.
    fn push_anchored_marker_fan(
        &mut self,
        anchor: [f64; 2],
        inner_radius_pixels: f64,
        outer_radius_pixels: f64,
        translate_pixels: [f64; 2],
        rgba_color: [f32; 4],
    ) {
        for side_index in 0..POINT_MARKER_FAN_SIDES {
            let start_angle = side_index as f64 * TAU / POINT_MARKER_FAN_SIDES as f64;
            let end_angle = (side_index + 1) as f64 * TAU / POINT_MARKER_FAN_SIDES as f64;
            let ring_offset = |angle: f64, radius_pixels: f64| {
                [
                    translate_pixels[0] + radius_pixels * angle.cos(),
                    translate_pixels[1] + radius_pixels * angle.sin(),
                ]
            };
            if inner_radius_pixels <= 0.0 {
                for screen_offset in [
                    translate_pixels,
                    ring_offset(start_angle, outer_radius_pixels),
                    ring_offset(end_angle, outer_radius_pixels),
                ] {
                    self.push_vertex(
                        anchor,
                        [0.0; 2],
                        [0.0; 2],
                        WORLD_SPACE_OVERLAY_SHAPE_ANCHORED,
                        screen_offset,
                        rgba_color,
                    );
                }
            } else {
                let inner_start = ring_offset(start_angle, inner_radius_pixels);
                let inner_end = ring_offset(end_angle, inner_radius_pixels);
                let outer_start = ring_offset(start_angle, outer_radius_pixels);
                let outer_end = ring_offset(end_angle, outer_radius_pixels);
                for screen_offset in [inner_start, inner_end, outer_end] {
                    self.push_vertex(
                        anchor,
                        [0.0; 2],
                        [0.0; 2],
                        WORLD_SPACE_OVERLAY_SHAPE_ANCHORED,
                        screen_offset,
                        rgba_color,
                    );
                }
                for screen_offset in [inner_start, outer_end, outer_start] {
                    self.push_vertex(
                        anchor,
                        [0.0; 2],
                        [0.0; 2],
                        WORLD_SPACE_OVERLAY_SHAPE_ANCHORED,
                        screen_offset,
                        rgba_color,
                    );
                }
            }
        }
    }

    /// Emits tip-anchored teardrop pins as the annulus between the hollow head
    /// circle and the star-shaped teardrop boundary, exactly the CPU
    /// assembly's triangle order. Every vertex keeps the tip's world position
    /// and its screen-pixel offset from it.
    fn push_point_pins(
        &mut self,
        positions: &[GeographicCoordinate],
        rgba_color: [f32; 4],
        radius_pixels: f64,
    ) {
        if !radius_pixels.is_finite() || radius_pixels <= 0.0 {
            return;
        }
        let pin_height_pixels = radius_pixels * POINT_PIN_HEIGHT_RADIUS_FACTOR;
        let hollow_radius_pixels = radius_pixels * POINT_PIN_HOLLOW_RADIUS_RATIO;
        let tangent_half_angle = (radius_pixels / pin_height_pixels).acos();
        let tip_direction_angle = std::f64::consts::FRAC_PI_2;
        let right_tangent_angle = tip_direction_angle - tangent_half_angle;
        let left_tangent_angle = tip_direction_angle + tangent_half_angle;
        let outline_radius_pixels = |angle: f64| {
            if angle > right_tangent_angle && angle < left_tangent_angle {
                let edge_angle = if angle <= tip_direction_angle {
                    right_tangent_angle
                } else {
                    left_tangent_angle
                };
                radius_pixels / (angle - edge_angle).cos()
            } else {
                radius_pixels
            }
        };
        // Every pin corner sits relative to the *tip's* projected position:
        // the head center is above it, and the rings radiate from the head
        // center. Screen y grows downward, so the head is at -pin_height.
        let screen_offset = |outline_radius: f64, angle: f64| {
            [
                outline_radius * angle.cos(),
                outline_radius * angle.sin() - pin_height_pixels,
            ]
        };
        for position in positions {
            let anchor = self.anchored_position(position);
            for side_index in 0..POINT_PIN_RING_SIDES {
                let start_angle = side_index as f64 * TAU / POINT_PIN_RING_SIDES as f64;
                let end_angle = (side_index + 1) as f64 * TAU / POINT_PIN_RING_SIDES as f64;
                let inner_start = screen_offset(hollow_radius_pixels, start_angle);
                let inner_end = screen_offset(hollow_radius_pixels, end_angle);
                let outer_start = screen_offset(outline_radius_pixels(start_angle), start_angle);
                let outer_end = screen_offset(outline_radius_pixels(end_angle), end_angle);
                for triangle in [
                    [inner_start, inner_end, outer_end],
                    [inner_start, outer_end, outer_start],
                ] {
                    for offset in triangle {
                        self.push_vertex(
                            anchor,
                            [0.0; 2],
                            [0.0; 2],
                            WORLD_SPACE_OVERLAY_SHAPE_ANCHORED,
                            offset,
                            rgba_color,
                        );
                    }
                }
            }
        }
    }
}

/// One piece of a shape segment after splitting against a world-copy window,
/// with its continuous normalized-world endpoints and the window's copy
/// offset for [`OverlayFrameProjection::ground_clip_position`].
#[derive(Debug, Clone, Copy, PartialEq)]
struct SegmentWindowFragment {
    /// Continuous normalized-world start point of the fragment.
    start: [f64; 2],
    /// Continuous normalized-world end point of the fragment.
    end: [f64; 2],
    /// Whole-world copy offset of the window the fragment lies in.
    window_offset: f64,
}

/// Splits a shape segment against the camera-centered world-copy windows.
///
/// Each window keeps the segment's intersection with its x-range, matching
/// [`CAMERA_WORLD_WINDOW_HALF_WIDTH`] around the camera per offset. A
/// vertical segment (zero x-span) joins the one window containing its x,
/// half-open on the east so a boundary-exact segment cannot draw twice; a
/// segment touching a window only at its edge produces no fragment, mirroring
/// the tile spans' empty-fragment rule.
fn segment_window_fragments(
    frame_projection: &OverlayFrameProjection,
    segment_start: [f64; 2],
    segment_end: [f64; 2],
) -> Vec<SegmentWindowFragment> {
    let span_x = segment_end[0] - segment_start[0];
    let mut fragments = Vec::new();
    for window_offset in WORLD_COPY_WINDOW_OFFSETS {
        let window_offset_f64 = f64::from(window_offset);
        let [window_start_x, window_end_x] = frame_projection.window_x_range(window_offset_f64);
        let (start_fraction, end_fraction) = if span_x == 0.0 {
            let zero_span_inside_window =
                segment_start[0] >= window_start_x && segment_start[0] < window_end_x;
            if !zero_span_inside_window {
                continue;
            }
            (0.0, 1.0)
        } else {
            let clamped_start_fraction =
                ((window_start_x - segment_start[0]) / span_x).clamp(0.0, 1.0);
            let clamped_end_fraction = ((window_end_x - segment_start[0]) / span_x).clamp(0.0, 1.0);
            if clamped_start_fraction == clamped_end_fraction {
                continue;
            }
            (clamped_start_fraction, clamped_end_fraction)
        };
        let fragment_start = [
            segment_start[0] + span_x * start_fraction,
            segment_start[1] + (segment_end[1] - segment_start[1]) * start_fraction,
        ];
        let fragment_end = [
            segment_start[0] + span_x * end_fraction,
            segment_start[1] + (segment_end[1] - segment_start[1]) * end_fraction,
        ];
        fragments.push(SegmentWindowFragment {
            start: fragment_start,
            end: fragment_end,
            window_offset: window_offset_f64,
        });
    }
    fragments
}

/// Assembles a thick polyline: consecutive coordinate pairs thicken to
/// per-segment screen-space quads split against the world-copy windows, and
/// every interior vertex fills its joint with a screen-space square of the
/// line's width.
///
/// The join fill is what keeps a multi-segment line continuous: butt-capped
/// quads meet at the shared vertex but leave a wedge on a bend's outer side,
/// which reads as a gap between segments — the most visible case is a styled
/// road network, whose per-way geometry bends at nearly every vertex.
/// Endpoints keep their butt caps (a per-feature end has no neighbouring
/// segment to join), and the square join never grows a miter spike at sharp
/// angles the way a mitered join would.
fn thickened_polyline_triangles(
    frame_projection: &OverlayFrameProjection,
    viewport: &ViewportPixels,
    coordinates: &[GeographicCoordinate],
    rgba_color: [f32; 4],
    width_pixels: f64,
    translate_pixels: [f64; 2],
    vertices: &mut Vec<ClipSpaceOverlayVertex>,
) {
    let wrapped_points = frame_projection.wrapped_continuous_chain(coordinates);
    for segment_index in 1..wrapped_points.len() {
        for fragment in segment_window_fragments(
            frame_projection,
            wrapped_points[segment_index - 1],
            wrapped_points[segment_index],
        ) {
            thickened_segment_triangles(
                frame_projection,
                viewport,
                &fragment,
                rgba_color,
                width_pixels,
                translate_pixels,
                vertices,
            );
        }
    }
    for vertex_index in 1..wrapped_points.len().saturating_sub(1) {
        polyline_join_triangles(
            frame_projection,
            viewport,
            PolylineBend {
                previous: wrapped_points[vertex_index - 1],
                vertex: wrapped_points[vertex_index],
                next: wrapped_points[vertex_index + 1],
            },
            rgba_color,
            width_pixels,
            translate_pixels,
            vertices,
        );
    }
}

/// One interior polyline vertex and its two neighbours, in world-copy ground
/// coordinates; the join pass fills the wedge the segment quads leave there.
struct PolylineBend {
    previous: [f64; 2],
    vertex: [f64; 2],
    next: [f64; 2],
}

/// Fills one interior polyline vertex's joint with a screen-space square of
/// the line's own width, oriented along the bend's bisector. The square
/// covers the wedge the two butt-capped segment quads leave outside a bend;
/// see [`thickened_polyline_triangles`] for why the join exists.
fn polyline_join_triangles(
    frame_projection: &OverlayFrameProjection,
    viewport: &ViewportPixels,
    bend: PolylineBend,
    rgba_color: [f32; 4],
    width_pixels: f64,
    translate_pixels: [f64; 2],
    vertices: &mut Vec<ClipSpaceOverlayVertex>,
) {
    // One join lives in the world-copy window that contains it; a vertex on a
    // window's east half-open edge belongs to the following copy, matching
    // the segment split's rule.
    let window_offset = WORLD_COPY_WINDOW_OFFSETS.iter().copied().find(|offset| {
        let [window_start_x, window_end_x] = frame_projection.window_x_range(f64::from(*offset));
        bend.vertex[0] >= window_start_x && bend.vertex[0] < window_end_x
    });
    let Some(window_offset) = window_offset else {
        return;
    };
    let window_offset_f64 = f64::from(window_offset);
    let project = |point: [f64; 2]| {
        frame_projection.ground_clip_position(window_offset_f64, point[0], point[1])
    };
    let previous_clip_position = project(bend.previous);
    let vertex_clip_position = project(bend.vertex);
    let next_clip_position = project(bend.next);
    if previous_clip_position.w <= 0.0
        || vertex_clip_position.w <= 0.0
        || next_clip_position.w <= 0.0
    {
        return;
    }
    let displaced = |screen_pixels: [f64; 2]| {
        [
            screen_pixels[0] + translate_pixels[0],
            screen_pixels[1] + translate_pixels[1],
        ]
    };
    let previous_screen = displaced(clip_position_to_screen_pixels(
        previous_clip_position,
        viewport,
    ));
    let vertex_screen = displaced(clip_position_to_screen_pixels(
        vertex_clip_position,
        viewport,
    ));
    let next_screen = displaced(clip_position_to_screen_pixels(next_clip_position, viewport));
    let incoming = [
        vertex_screen[0] - previous_screen[0],
        vertex_screen[1] - previous_screen[1],
    ];
    let outgoing = [
        next_screen[0] - vertex_screen[0],
        next_screen[1] - vertex_screen[1],
    ];
    let incoming_length = incoming[0].hypot(incoming[1]);
    let outgoing_length = outgoing[0].hypot(outgoing[1]);
    if incoming_length < MINIMUM_VISIBLE_SEGMENT_LENGTH_PIXELS
        || outgoing_length < MINIMUM_VISIBLE_SEGMENT_LENGTH_PIXELS
    {
        return;
    }
    let incoming_unit = [incoming[0] / incoming_length, incoming[1] / incoming_length];
    let outgoing_unit = [outgoing[0] / outgoing_length, outgoing[1] / outgoing_length];
    let half_width_pixels = width_pixels * 0.5;
    // The bisector of the two segment directions is the direction the wedge
    // opens along. A reversal (sum near zero) has no bisector: the incoming
    // perpendicular is its 90-degree stand-in.
    let sum = [
        incoming_unit[0] + outgoing_unit[0],
        incoming_unit[1] + outgoing_unit[1],
    ];
    let sum_length = sum[0].hypot(sum[1]);
    let bisector_unit = if sum_length < MINIMUM_VISIBLE_SEGMENT_LENGTH_PIXELS {
        [-incoming_unit[1], incoming_unit[0]]
    } else {
        [sum[0] / sum_length, sum[1] / sum_length]
    };
    let perpendicular_unit = [-bisector_unit[1], bisector_unit[0]];
    let corner_screens = [
        [
            vertex_screen[0] + (bisector_unit[0] + perpendicular_unit[0]) * half_width_pixels,
            vertex_screen[1] + (bisector_unit[1] + perpendicular_unit[1]) * half_width_pixels,
        ],
        [
            vertex_screen[0] + (-bisector_unit[0] + perpendicular_unit[0]) * half_width_pixels,
            vertex_screen[1] + (-bisector_unit[1] + perpendicular_unit[1]) * half_width_pixels,
        ],
        [
            vertex_screen[0] + (-bisector_unit[0] - perpendicular_unit[0]) * half_width_pixels,
            vertex_screen[1] + (-bisector_unit[1] - perpendicular_unit[1]) * half_width_pixels,
        ],
        [
            vertex_screen[0] + (bisector_unit[0] - perpendicular_unit[0]) * half_width_pixels,
            vertex_screen[1] + (bisector_unit[1] - perpendicular_unit[1]) * half_width_pixels,
        ],
    ];
    let corner_clip_positions = corner_screens.map(|corner_screen_pixels| {
        screen_pixels_to_clip_position(corner_screen_pixels, vertex_clip_position, viewport)
    });
    push_triangle(
        vertices,
        &[
            corner_clip_positions[0],
            corner_clip_positions[1],
            corner_clip_positions[2],
        ],
        rgba_color,
    );
    push_triangle(
        vertices,
        &[
            corner_clip_positions[0],
            corner_clip_positions[2],
            corner_clip_positions[3],
        ],
        rgba_color,
    );
}

/// One stroke band of a styled polyline: the perpendicular offset of the
/// band's centreline and its width, both in screen pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
struct StrokeBand {
    offset_pixels: f64,
    width_pixels: f64,
}

/// The bands a stroke paints, with the stroke's perpendicular offset and
/// casing gap multiplied by `device_pixel_ratio` (the frame's screen scale).
/// A plain stroke is one band on the centreline; a casing
/// (`line-gap-width`) paints two parallel bands outside the gap, one on each
/// side, which is the specification's "line casing outside the path".
fn stroke_bands(
    stroke: &PolylineStroke,
    width_pixels: f64,
    device_pixel_ratio: f64,
) -> Vec<StrokeBand> {
    let half_width_pixels = width_pixels * 0.5;
    let offset_pixels = stroke.offset_pixels * device_pixel_ratio;
    if stroke.gap_width_pixels > 0.0 {
        let band_offset_pixels =
            stroke.gap_width_pixels * device_pixel_ratio * 0.5 + half_width_pixels;
        vec![
            StrokeBand {
                offset_pixels: offset_pixels - band_offset_pixels,
                width_pixels,
            },
            StrokeBand {
                offset_pixels: offset_pixels + band_offset_pixels,
                width_pixels,
            },
        ]
    } else {
        vec![StrokeBand {
            offset_pixels,
            width_pixels,
        }]
    }
}

/// Projects one normalized-world point inside one world-copy window, or
/// returns `None` when the point is at or behind the camera.
fn screen_and_clip_position(
    frame_projection: &OverlayFrameProjection,
    viewport: &ViewportPixels,
    window_offset: f64,
    point: [f64; 2],
) -> Option<([f64; 2], DVec4)> {
    let clip_position = frame_projection.ground_clip_position(window_offset, point[0], point[1]);
    if clip_position.w <= 0.0 {
        return None;
    }
    Some((
        clip_position_to_screen_pixels(clip_position, viewport),
        clip_position,
    ))
}

/// The window offset whose x-range contains `x`; the half-open edge rule
/// matches the segment fragments' split.
fn window_containing_x(frame_projection: &OverlayFrameProjection, x: f64) -> Option<f64> {
    WORLD_COPY_WINDOW_OFFSETS
        .iter()
        .copied()
        .find_map(|window_offset| {
            let window_offset_f64 = f64::from(window_offset);
            let [window_start_x, window_end_x] = frame_projection.window_x_range(window_offset_f64);
            (x >= window_start_x && x < window_end_x).then_some(window_offset_f64)
        })
}

/// Linear interpolation between two normalized-world points.
fn interpolate_world_point(start: [f64; 2], end: [f64; 2], fraction: f64) -> [f64; 2] {
    [
        start[0] + (end[0] - start[0]) * fraction,
        start[1] + (end[1] - start[1]) * fraction,
    ]
}

/// Emits one stroke band's quad between two projected screen points, with the
/// band centre offset perpendicular to the segment.
#[allow(clippy::too_many_arguments)]
fn push_stroke_band_quad(
    viewport: &ViewportPixels,
    start_screen: [f64; 2],
    start_clip_position: DVec4,
    end_screen: [f64; 2],
    end_clip_position: DVec4,
    band: &StrokeBand,
    rgba_color: [f32; 4],
    vertices: &mut Vec<ClipSpaceOverlayVertex>,
) {
    let direction = [
        end_screen[0] - start_screen[0],
        end_screen[1] - start_screen[1],
    ];
    let screen_length = direction[0].hypot(direction[1]);
    if screen_length < MINIMUM_VISIBLE_SEGMENT_LENGTH_PIXELS {
        return;
    }
    let perpendicular_unit = [-direction[1] / screen_length, direction[0] / screen_length];
    let half_width_pixels = band.width_pixels * 0.5;
    let inner_offset = band.offset_pixels - half_width_pixels;
    let outer_offset = band.offset_pixels + half_width_pixels;
    let corner_screens = [
        [
            start_screen[0] + perpendicular_unit[0] * outer_offset,
            start_screen[1] + perpendicular_unit[1] * outer_offset,
        ],
        [
            start_screen[0] + perpendicular_unit[0] * inner_offset,
            start_screen[1] + perpendicular_unit[1] * inner_offset,
        ],
        [
            end_screen[0] + perpendicular_unit[0] * outer_offset,
            end_screen[1] + perpendicular_unit[1] * outer_offset,
        ],
        [
            end_screen[0] + perpendicular_unit[0] * inner_offset,
            end_screen[1] + perpendicular_unit[1] * inner_offset,
        ],
    ];
    let corner_clip_positions = [
        (corner_screens[0], start_clip_position),
        (corner_screens[1], start_clip_position),
        (corner_screens[2], end_clip_position),
        (corner_screens[3], end_clip_position),
    ]
    .map(|(corner_screen_pixels, reference_clip_position)| {
        screen_pixels_to_clip_position(corner_screen_pixels, reference_clip_position, viewport)
    });
    push_triangle(
        vertices,
        &[
            corner_clip_positions[0],
            corner_clip_positions[1],
            corner_clip_positions[2],
        ],
        rgba_color,
    );
    push_triangle(
        vertices,
        &[
            corner_clip_positions[2],
            corner_clip_positions[1],
            corner_clip_positions[3],
        ],
        rgba_color,
    );
}

/// Emits one straight piece of a stroke band, split against the world-copy
/// windows exactly like the default stroke's segment fragments.
#[allow(clippy::too_many_arguments)]
fn push_stroke_band_piece(
    frame_projection: &OverlayFrameProjection,
    viewport: &ViewportPixels,
    piece_start: [f64; 2],
    piece_end: [f64; 2],
    band: &StrokeBand,
    translate_pixels: [f64; 2],
    rgba_color: [f32; 4],
    vertices: &mut Vec<ClipSpaceOverlayVertex>,
) {
    for window_offset in WORLD_COPY_WINDOW_OFFSETS {
        let window_offset_f64 = f64::from(window_offset);
        let [window_start_x, window_end_x] = frame_projection.window_x_range(window_offset_f64);
        let span_x = piece_end[0] - piece_start[0];
        let (start_fraction, end_fraction) = if span_x == 0.0 {
            if piece_start[0] < window_start_x || piece_start[0] >= window_end_x {
                continue;
            }
            (0.0, 1.0)
        } else {
            (
                ((window_start_x - piece_start[0]) / span_x).clamp(0.0, 1.0),
                ((window_end_x - piece_start[0]) / span_x).clamp(0.0, 1.0),
            )
        };
        if end_fraction <= start_fraction {
            continue;
        }
        let fragment_start = interpolate_world_point(piece_start, piece_end, start_fraction);
        let fragment_end = interpolate_world_point(piece_start, piece_end, end_fraction);
        let Some((start_screen, start_clip_position)) = screen_and_clip_position(
            frame_projection,
            viewport,
            window_offset_f64,
            fragment_start,
        ) else {
            continue;
        };
        let Some((end_screen, end_clip_position)) =
            screen_and_clip_position(frame_projection, viewport, window_offset_f64, fragment_end)
        else {
            continue;
        };
        // A line translate is a constant screen-space displacement: offset the
        // projected screen positions and keep the clip positions as the
        // reference frames, so `screen_pixels_to_clip_position` emits the
        // translated quad exactly.
        let start_screen = [
            start_screen[0] + translate_pixels[0],
            start_screen[1] + translate_pixels[1],
        ];
        let end_screen = [
            end_screen[0] + translate_pixels[0],
            end_screen[1] + translate_pixels[1],
        ];
        push_stroke_band_quad(
            viewport,
            start_screen,
            start_clip_position,
            end_screen,
            end_clip_position,
            band,
            rgba_color,
            vertices,
        );
    }
}

/// Emits a join at one interior vertex where the dash leaves the stroke on.
///
/// The miter join fills the wedge the segment quads leave on both sides of a
/// bend; a corner sharper than [`LINE_MITER_LIMIT`] falls back to a bevel, and
/// a round join fans across the wedge instead.
#[allow(clippy::too_many_arguments)]
fn push_stroke_join(
    frame_projection: &OverlayFrameProjection,
    viewport: &ViewportPixels,
    previous: [f64; 2],
    vertex: [f64; 2],
    next: [f64; 2],
    band: &StrokeBand,
    join: LineJoin,
    translate_pixels: [f64; 2],
    rgba_color: [f32; 4],
    vertices: &mut Vec<ClipSpaceOverlayVertex>,
) {
    let Some(window_offset) = window_containing_x(frame_projection, vertex[0]) else {
        return;
    };
    let Some((previous_screen, _)) =
        screen_and_clip_position(frame_projection, viewport, window_offset, previous)
    else {
        return;
    };
    let Some((vertex_screen, vertex_clip_position)) =
        screen_and_clip_position(frame_projection, viewport, window_offset, vertex)
    else {
        return;
    };
    let Some((next_screen, _)) =
        screen_and_clip_position(frame_projection, viewport, window_offset, next)
    else {
        return;
    };
    let translate_screen = |screen: [f64; 2]| {
        [
            screen[0] + translate_pixels[0],
            screen[1] + translate_pixels[1],
        ]
    };
    let previous_screen = translate_screen(previous_screen);
    let vertex_screen = translate_screen(vertex_screen);
    let next_screen = translate_screen(next_screen);
    let incoming = [
        vertex_screen[0] - previous_screen[0],
        vertex_screen[1] - previous_screen[1],
    ];
    let outgoing = [
        next_screen[0] - vertex_screen[0],
        next_screen[1] - vertex_screen[1],
    ];
    let incoming_length = incoming[0].hypot(incoming[1]);
    let outgoing_length = outgoing[0].hypot(outgoing[1]);
    if incoming_length < MINIMUM_VISIBLE_SEGMENT_LENGTH_PIXELS
        || outgoing_length < MINIMUM_VISIBLE_SEGMENT_LENGTH_PIXELS
    {
        return;
    }
    let incoming_unit = [incoming[0] / incoming_length, incoming[1] / incoming_length];
    let outgoing_unit = [outgoing[0] / outgoing_length, outgoing[1] / outgoing_length];
    let incoming_normal = [-incoming_unit[1], incoming_unit[0]];
    let outgoing_normal = [-outgoing_unit[1], outgoing_unit[0]];
    let cross = incoming_unit[0] * outgoing_unit[1] - incoming_unit[1] * outgoing_unit[0];
    if cross.abs() < MINIMUM_VISIBLE_SEGMENT_LENGTH_PIXELS {
        // A straight run needs no join; the quads already meet.
        return;
    }
    let half_width_pixels = band.width_pixels * 0.5;
    let boundary_offsets = [
        band.offset_pixels + half_width_pixels,
        band.offset_pixels - half_width_pixels,
    ];
    let normal_sum = [
        incoming_normal[0] + outgoing_normal[0],
        incoming_normal[1] + outgoing_normal[1],
    ];
    let normal_sum_length = normal_sum[0].hypot(normal_sum[1]);
    let bisector_unit = if normal_sum_length < MINIMUM_VISIBLE_SEGMENT_LENGTH_PIXELS {
        incoming_normal
    } else {
        [
            normal_sum[0] / normal_sum_length,
            normal_sum[1] / normal_sum_length,
        ]
    };
    let apex_screen = [
        vertex_screen[0] + bisector_unit[0] * band.offset_pixels,
        vertex_screen[1] + bisector_unit[1] * band.offset_pixels,
    ];
    let apex_clip_position =
        screen_pixels_to_clip_position(apex_screen, vertex_clip_position, viewport);
    for boundary_offset in boundary_offsets {
        let boundary_start = screen_pixels_to_clip_position(
            [
                vertex_screen[0] + incoming_normal[0] * boundary_offset,
                vertex_screen[1] + incoming_normal[1] * boundary_offset,
            ],
            vertex_clip_position,
            viewport,
        );
        let boundary_end = screen_pixels_to_clip_position(
            [
                vertex_screen[0] + outgoing_normal[0] * boundary_offset,
                vertex_screen[1] + outgoing_normal[1] * boundary_offset,
            ],
            vertex_clip_position,
            viewport,
        );
        match join {
            LineJoin::Round => {
                let start_angle = incoming_normal[1].atan2(incoming_normal[0]);
                let end_angle = outgoing_normal[1].atan2(outgoing_normal[0]);
                let sweep = wrapped_angle_difference(start_angle, end_angle);
                let sweep_direction = if cross >= 0.0 {
                    sweep.abs()
                } else {
                    -sweep.abs()
                };
                let mut previous_corner = boundary_start;
                for side_index in 1..=ROUND_LINE_FAN_SIDES {
                    let angle = start_angle
                        + sweep_direction * side_index as f64 / ROUND_LINE_FAN_SIDES as f64;
                    let next_corner = screen_pixels_to_clip_position(
                        [
                            vertex_screen[0] + angle.cos() * boundary_offset,
                            vertex_screen[1] + angle.sin() * boundary_offset,
                        ],
                        vertex_clip_position,
                        viewport,
                    );
                    push_triangle(
                        vertices,
                        &[apex_clip_position, previous_corner, next_corner],
                        rgba_color,
                    );
                    previous_corner = next_corner;
                }
            }
            LineJoin::Bevel => {
                push_triangle(
                    vertices,
                    &[apex_clip_position, boundary_start, boundary_end],
                    rgba_color,
                );
            }
            LineJoin::Miter => {
                let bisector_dot =
                    bisector_unit[0] * incoming_normal[0] + bisector_unit[1] * incoming_normal[1];
                if bisector_dot <= 1.0 / LINE_MITER_LIMIT {
                    push_triangle(
                        vertices,
                        &[apex_clip_position, boundary_start, boundary_end],
                        rgba_color,
                    );
                    continue;
                }
                let miter_distance = boundary_offset / bisector_dot;
                let miter_corner = screen_pixels_to_clip_position(
                    [
                        vertex_screen[0] + bisector_unit[0] * miter_distance,
                        vertex_screen[1] + bisector_unit[1] * miter_distance,
                    ],
                    vertex_clip_position,
                    viewport,
                );
                push_triangle(
                    vertices,
                    &[apex_clip_position, boundary_start, miter_corner],
                    rgba_color,
                );
                push_triangle(
                    vertices,
                    &[apex_clip_position, miter_corner, boundary_end],
                    rgba_color,
                );
            }
        }
    }
}

/// The signed shortest angular difference from `start_angle` to `end_angle`,
/// in radians.
fn wrapped_angle_difference(start_angle: f64, end_angle: f64) -> f64 {
    let mut difference = end_angle - start_angle;
    while difference > PI {
        difference -= TAU;
    }
    while difference < -PI {
        difference += TAU;
    }
    difference
}

/// Emits one run end's cap in the direction the run leaves the point.
///
/// `inward_unit` is the unit direction from the end point into the run, so the
/// cap covers the outward half circle or the outward half width.
#[allow(clippy::too_many_arguments)]
fn push_stroke_cap(
    frame_projection: &OverlayFrameProjection,
    viewport: &ViewportPixels,
    point: [f64; 2],
    inward_unit: [f64; 2],
    band: &StrokeBand,
    cap: LineCap,
    translate_pixels: [f64; 2],
    rgba_color: [f32; 4],
    vertices: &mut Vec<ClipSpaceOverlayVertex>,
) {
    if cap == LineCap::Butt {
        return;
    }
    let Some(window_offset) = window_containing_x(frame_projection, point[0]) else {
        return;
    };
    let Some((screen_point, clip_position)) =
        screen_and_clip_position(frame_projection, viewport, window_offset, point)
    else {
        return;
    };
    let screen_point = [
        screen_point[0] + translate_pixels[0],
        screen_point[1] + translate_pixels[1],
    ];
    let perpendicular_unit = [-inward_unit[1], inward_unit[0]];
    let half_width_pixels = band.width_pixels * 0.5;
    let centre_screen = [
        screen_point[0] + perpendicular_unit[0] * band.offset_pixels,
        screen_point[1] + perpendicular_unit[1] * band.offset_pixels,
    ];
    let centre_clip_position =
        screen_pixels_to_clip_position(centre_screen, clip_position, viewport);
    match cap {
        LineCap::Butt => {}
        LineCap::Square => {
            let outward_unit = [-inward_unit[0], -inward_unit[1]];
            let corner_screens = [
                [
                    centre_screen[0] + perpendicular_unit[0] * half_width_pixels,
                    centre_screen[1] + perpendicular_unit[1] * half_width_pixels,
                ],
                [
                    centre_screen[0] - perpendicular_unit[0] * half_width_pixels,
                    centre_screen[1] - perpendicular_unit[1] * half_width_pixels,
                ],
                [
                    centre_screen[0]
                        + outward_unit[0] * half_width_pixels
                        + perpendicular_unit[0] * half_width_pixels,
                    centre_screen[1]
                        + outward_unit[1] * half_width_pixels
                        + perpendicular_unit[1] * half_width_pixels,
                ],
                [
                    centre_screen[0] + outward_unit[0] * half_width_pixels
                        - perpendicular_unit[0] * half_width_pixels,
                    centre_screen[1] + outward_unit[1] * half_width_pixels
                        - perpendicular_unit[1] * half_width_pixels,
                ],
            ];
            let corner_clip_positions = corner_screens
                .map(|corner| screen_pixels_to_clip_position(corner, clip_position, viewport));
            push_triangle(
                vertices,
                &[
                    corner_clip_positions[0],
                    corner_clip_positions[1],
                    corner_clip_positions[2],
                ],
                rgba_color,
            );
            push_triangle(
                vertices,
                &[
                    corner_clip_positions[2],
                    corner_clip_positions[1],
                    corner_clip_positions[3],
                ],
                rgba_color,
            );
        }
        LineCap::Round => {
            let start_angle = perpendicular_unit[1].atan2(perpendicular_unit[0]);
            // The positive sweep from the incoming perpendicular passes
            // through the outward direction, whichever end of the run this is.
            let sweep = PI;
            let mut previous_corner = screen_pixels_to_clip_position(
                [
                    centre_screen[0] + perpendicular_unit[0] * half_width_pixels,
                    centre_screen[1] + perpendicular_unit[1] * half_width_pixels,
                ],
                clip_position,
                viewport,
            );
            for side_index in 1..=ROUND_LINE_FAN_SIDES {
                let angle = start_angle + sweep * side_index as f64 / ROUND_LINE_FAN_SIDES as f64;
                let next_corner = screen_pixels_to_clip_position(
                    [
                        centre_screen[0] + angle.cos() * half_width_pixels,
                        centre_screen[1] + angle.sin() * half_width_pixels,
                    ],
                    clip_position,
                    viewport,
                );
                push_triangle(
                    vertices,
                    &[centre_clip_position, previous_corner, next_corner],
                    rgba_color,
                );
                previous_corner = next_corner;
            }
        }
    }
}

/// Whether a distance along the chain falls in an "on" run of the dash
/// pattern.
fn dash_pattern_is_on_at(distance: f64, pattern: &[f64], pattern_total: f64) -> bool {
    if pattern.is_empty() || pattern_total <= 0.0 {
        return true;
    }
    let mut remaining = distance.rem_euclid(pattern_total);
    let mut index = 0;
    loop {
        let step = pattern[index % pattern.len()];
        if remaining < step {
            return index % 2 == 0;
        }
        remaining -= step;
        index += 1;
        if index > pattern.len() * 2 {
            return true;
        }
    }
}

/// The on-runs of the dash pattern over a chain of `total_length` pixels.
/// An empty pattern produces one run covering the whole chain.
fn dash_on_runs(total_length: f64, pattern: &[f64], pattern_total: f64) -> Vec<(f64, f64)> {
    if pattern.is_empty() || pattern_total <= 0.0 {
        return vec![(0.0, total_length)];
    }
    let mut runs = Vec::new();
    let mut distance = 0.0;
    let mut index = 0;
    while distance < total_length {
        let step = pattern[index % pattern.len()];
        let end = (distance + step).min(total_length);
        if index % 2 == 0 {
            runs.push((distance, end));
        }
        distance = end;
        index += 1;
    }
    runs
}

/// Assembles one styled polyline: the dash pattern's on-runs as thickened,
/// offset bands, with the stroke's joins at interior vertices and caps at run
/// ends. A blurred stroke paints a wider, fainter fringe first. The frame's
/// `device_pixel_ratio` scales the stroke's blur, offset and casing gap as
/// the shape is emitted.
#[allow(clippy::too_many_arguments)]
fn styled_polyline_triangles(
    frame_projection: &OverlayFrameProjection,
    viewport: &ViewportPixels,
    coordinates: &[GeographicCoordinate],
    rgba_color: [f32; 4],
    width_pixels: f64,
    stroke: &PolylineStroke,
    translate_pixels: [f64; 2],
    device_pixel_ratio: f64,
    vertices: &mut Vec<ClipSpaceOverlayVertex>,
) {
    if !width_pixels.is_finite() || width_pixels <= 0.0 {
        return;
    }
    let wrapped_points = frame_projection.wrapped_continuous_chain(coordinates);
    if wrapped_points.len() < 2 {
        return;
    }
    let blur_pixels = stroke.blur_pixels * device_pixel_ratio;
    if blur_pixels > 0.0 {
        let mut fringe_rgba = rgba_color;
        fringe_rgba[3] *= LINE_BLUR_FRINGE_ALPHA_SCALE;
        emit_styled_chain(
            frame_projection,
            viewport,
            &wrapped_points,
            fringe_rgba,
            width_pixels + 2.0 * blur_pixels,
            stroke,
            translate_pixels,
            device_pixel_ratio,
            vertices,
        );
    }
    emit_styled_chain(
        frame_projection,
        viewport,
        &wrapped_points,
        rgba_color,
        width_pixels,
        stroke,
        translate_pixels,
        device_pixel_ratio,
        vertices,
    );
}

/// Emits one pass of a styled chain at one width and colour.
#[allow(clippy::too_many_arguments)]
fn emit_styled_chain(
    frame_projection: &OverlayFrameProjection,
    viewport: &ViewportPixels,
    points: &[[f64; 2]],
    rgba_color: [f32; 4],
    width_pixels: f64,
    stroke: &PolylineStroke,
    translate_pixels: [f64; 2],
    device_pixel_ratio: f64,
    vertices: &mut Vec<ClipSpaceOverlayVertex>,
) {
    let bands = stroke_bands(stroke, width_pixels, device_pixel_ratio);
    // Dash distances are measured in screen pixels along the camera's own
    // world copy; a segment whose endpoints are behind the camera keeps the
    // phase unchanged.
    let mut screen_points = Vec::with_capacity(points.len());
    for point in points {
        screen_points.push(
            screen_and_clip_position(frame_projection, viewport, 0.0, *point)
                .map(|(screen_pixels, _)| screen_pixels),
        );
    }
    let mut cumulative = vec![0.0; points.len()];
    for point_index in 1..points.len() {
        let length = match (screen_points[point_index - 1], screen_points[point_index]) {
            (Some(start), Some(end)) => (end[0] - start[0]).hypot(end[1] - start[1]),
            _ => 0.0,
        };
        cumulative[point_index] = cumulative[point_index - 1] + length;
    }
    let total_length = cumulative[points.len() - 1];

    let dash_pattern: Vec<f64> = {
        let mut pattern: Vec<f64> = stroke
            .dash_array
            .iter()
            .copied()
            .filter(|length| length.is_finite() && *length > 0.0)
            .map(|length| length * width_pixels)
            .collect();
        if pattern.len() % 2 == 1
            && let Some(last_length) = pattern.last().copied()
        {
            pattern.push(last_length);
        }
        pattern
    };
    let pattern_total: f64 = dash_pattern.iter().sum();
    let runs = dash_on_runs(total_length, &dash_pattern, pattern_total);

    // The segment and fraction a distance along the chain falls in.
    let locate_distance = |distance: f64| -> (usize, f64) {
        let mut segment_index = 0;
        while segment_index + 2 < cumulative.len() && distance > cumulative[segment_index + 1] {
            segment_index += 1;
        }
        let segment_length = cumulative[segment_index + 1] - cumulative[segment_index];
        let fraction = if segment_length > 0.0 {
            ((distance - cumulative[segment_index]) / segment_length).clamp(0.0, 1.0)
        } else {
            0.0
        };
        (segment_index, fraction)
    };

    for (run_start, run_end) in &runs {
        if run_end - run_start <= MINIMUM_VISIBLE_SEGMENT_LENGTH_PIXELS {
            continue;
        }
        let (start_segment, start_fraction) = locate_distance(*run_start);
        let (end_segment, end_fraction) = locate_distance(*run_end);
        for segment_index in start_segment..=end_segment {
            let from_fraction = if segment_index == start_segment {
                start_fraction
            } else {
                0.0
            };
            let to_fraction = if segment_index == end_segment {
                end_fraction
            } else {
                1.0
            };
            if to_fraction <= from_fraction {
                continue;
            }
            let piece_start = interpolate_world_point(
                points[segment_index],
                points[segment_index + 1],
                from_fraction,
            );
            let piece_end = interpolate_world_point(
                points[segment_index],
                points[segment_index + 1],
                to_fraction,
            );
            for band in &bands {
                push_stroke_band_piece(
                    frame_projection,
                    viewport,
                    piece_start,
                    piece_end,
                    band,
                    translate_pixels,
                    rgba_color,
                    vertices,
                );
            }
        }
        // Caps at both ends of the run, pointing into the run.
        for (run_endpoint, is_start) in [(*run_start, true), (*run_end, false)] {
            let (segment_index, fraction) = locate_distance(run_endpoint);
            let segment_start = points[segment_index];
            let segment_end = points[segment_index + 1];
            let direction = [
                segment_end[0] - segment_start[0],
                segment_end[1] - segment_start[1],
            ];
            let direction_length = direction[0].hypot(direction[1]);
            if direction_length <= f64::EPSILON {
                continue;
            }
            let unit = [
                direction[0] / direction_length,
                direction[1] / direction_length,
            ];
            let inward_unit = if is_start { unit } else { [-unit[0], -unit[1]] };
            let cap_point = interpolate_world_point(segment_start, segment_end, fraction);
            for band in &bands {
                push_stroke_cap(
                    frame_projection,
                    viewport,
                    cap_point,
                    inward_unit,
                    band,
                    stroke.cap,
                    translate_pixels,
                    rgba_color,
                    vertices,
                );
            }
        }
    }

    // Joins at interior vertices the dash leaves on.
    for vertex_index in 1..points.len().saturating_sub(1) {
        let distance = cumulative[vertex_index];
        if !dash_pattern_is_on_at(distance, &dash_pattern, pattern_total) {
            continue;
        }
        for band in &bands {
            push_stroke_join(
                frame_projection,
                viewport,
                points[vertex_index - 1],
                points[vertex_index],
                points[vertex_index + 1],
                band,
                stroke.join,
                translate_pixels,
                rgba_color,
                vertices,
            );
        }
    }
}

/// Assembles one thickened segment fragment as a screen-space quad: the
/// perpendicular offset of half the line width, applied in screen pixels
/// around each endpoint's divided position, so a line keeps its pixel width
/// regardless of zoom and pitch.
///
/// A fragment with a vertex at or behind the camera draws nothing — the
/// screen-space offset needs a positive homogeneous weight — which is the
/// documented limitation next to the filled polygons' GPU-clipped near-plane
/// straddling (see the module notes).
fn thickened_segment_triangles(
    frame_projection: &OverlayFrameProjection,
    viewport: &ViewportPixels,
    fragment: &SegmentWindowFragment,
    rgba_color: [f32; 4],
    width_pixels: f64,
    translate_pixels: [f64; 2],
    vertices: &mut Vec<ClipSpaceOverlayVertex>,
) {
    let start_clip_position = frame_projection.ground_clip_position(
        fragment.window_offset,
        fragment.start[0],
        fragment.start[1],
    );
    let end_clip_position = frame_projection.ground_clip_position(
        fragment.window_offset,
        fragment.end[0],
        fragment.end[1],
    );
    if start_clip_position.w <= 0.0 || end_clip_position.w <= 0.0 {
        return;
    }
    let start_screen = {
        let screen_pixels = clip_position_to_screen_pixels(start_clip_position, viewport);
        [
            screen_pixels[0] + translate_pixels[0],
            screen_pixels[1] + translate_pixels[1],
        ]
    };
    let end_screen = {
        let screen_pixels = clip_position_to_screen_pixels(end_clip_position, viewport);
        [
            screen_pixels[0] + translate_pixels[0],
            screen_pixels[1] + translate_pixels[1],
        ]
    };
    let direction = [
        end_screen[0] - start_screen[0],
        end_screen[1] - start_screen[1],
    ];
    let screen_length = direction[0].hypot(direction[1]);
    if screen_length < MINIMUM_VISIBLE_SEGMENT_LENGTH_PIXELS {
        return;
    }
    let perpendicular_unit = [-direction[1] / screen_length, direction[0] / screen_length];
    let half_width_pixels = width_pixels * 0.5;
    let start_offset = [
        start_screen[0] + perpendicular_unit[0] * half_width_pixels,
        start_screen[1] + perpendicular_unit[1] * half_width_pixels,
    ];
    let start_opposite = [
        start_screen[0] - perpendicular_unit[0] * half_width_pixels,
        start_screen[1] - perpendicular_unit[1] * half_width_pixels,
    ];
    let end_offset = [
        end_screen[0] + perpendicular_unit[0] * half_width_pixels,
        end_screen[1] + perpendicular_unit[1] * half_width_pixels,
    ];
    let end_opposite = [
        end_screen[0] - perpendicular_unit[0] * half_width_pixels,
        end_screen[1] - perpendicular_unit[1] * half_width_pixels,
    ];
    // Corner order mirrors the tile quads: start-offset, start-opposite,
    // end-offset, end-opposite, split by the quads' `0,1,2 / 2,1,3` triangle
    // pattern, each corner re-expanded around its own endpoint's depth.
    let corner_clip_positions = [
        (start_offset, start_clip_position),
        (start_opposite, start_clip_position),
        (end_offset, end_clip_position),
        (end_opposite, end_clip_position),
    ]
    .map(|(corner_screen_pixels, reference_clip_position)| {
        screen_pixels_to_clip_position(corner_screen_pixels, reference_clip_position, viewport)
    });
    push_triangle(
        vertices,
        &[
            corner_clip_positions[0],
            corner_clip_positions[1],
            corner_clip_positions[2],
        ],
        rgba_color,
    );
    push_triangle(
        vertices,
        &[
            corner_clip_positions[2],
            corner_clip_positions[1],
            corner_clip_positions[3],
        ],
        rgba_color,
    );
}

/// The resolved paint of one point marker.
#[derive(Debug, Clone, Copy, PartialEq)]
struct PointMarkerStyle {
    /// Fill color and opacity.
    rgba_color: [f32; 4],
    /// Fill radius in screen pixels.
    radius_pixels: f64,
    /// Stroke color, when the style declares one.
    rgba_stroke_color: Option<[f32; 4]>,
    /// Stroke width in screen pixels, drawn outside the radius.
    stroke_width_pixels: f64,
    /// Edge softening in screen pixels.
    blur_pixels: f64,
    /// The marker's displacement from its anchor, in screen pixels.
    translate_pixels: [f64; 2],
    /// Whether the radius follows the ground plane when the map is pitched.
    pitch_scale: CirclePitchScale,
    /// Whether the marker lies on the ground plane or faces the screen.
    pitch_alignment: CirclePitchAlignment,
}

/// Assembles one point marker: a regular-octagon triangle fan centered on the
/// projected anchor (center anchoring; see the module notes). A viewport-
/// aligned marker is built around a single clip-space reference so the whole
/// fan stays screen-facing; a map-aligned marker projects ground offsets, so a
/// pitched camera sees the ellipse the ground circle really has.
fn point_marker_triangles(
    frame_projection: &OverlayFrameProjection,
    viewport: &ViewportPixels,
    position: &GeographicCoordinate,
    style: &PointMarkerStyle,
    vertices: &mut Vec<ClipSpaceOverlayVertex>,
) {
    if !style.radius_pixels.is_finite() || style.radius_pixels <= 0.0 {
        return;
    }
    let normalized_world = coordinate_to_normalized_world(position);
    let anchor = [
        frame_projection.wrapped_x(normalized_world[0]),
        normalized_world[1],
    ];
    // Blur paints a wider, fainter fringe under the marker.
    if style.blur_pixels > 0.0 {
        let mut fringe_rgba = style.rgba_color;
        fringe_rgba[3] *= LINE_BLUR_FRINGE_ALPHA_SCALE;
        push_marker_fan(
            frame_projection,
            viewport,
            anchor,
            0.0,
            style.radius_pixels + style.blur_pixels,
            style,
            fringe_rgba,
            vertices,
        );
    }
    push_marker_fan(
        frame_projection,
        viewport,
        anchor,
        0.0,
        style.radius_pixels,
        style,
        style.rgba_color,
        vertices,
    );
    // The specification places the stroke outside the circle's radius.
    if let Some(stroke_rgba) = style.rgba_stroke_color
        && style.stroke_width_pixels > 0.0
    {
        push_marker_fan(
            frame_projection,
            viewport,
            anchor,
            style.radius_pixels,
            style.radius_pixels + style.stroke_width_pixels,
            style,
            stroke_rgba,
            vertices,
        );
    }
}

/// Emits one marker fan between an inner and an outer radius; an inner radius
/// of zero fills a disc, a positive one paints a stroke annulus.
#[allow(clippy::too_many_arguments)]
fn push_marker_fan(
    frame_projection: &OverlayFrameProjection,
    viewport: &ViewportPixels,
    anchor: [f64; 2],
    inner_radius_pixels: f64,
    outer_radius_pixels: f64,
    style: &PointMarkerStyle,
    rgba_color: [f32; 4],
    vertices: &mut Vec<ClipSpaceOverlayVertex>,
) {
    match style.pitch_alignment {
        CirclePitchAlignment::Viewport => push_screen_marker_fan(
            frame_projection,
            viewport,
            anchor,
            inner_radius_pixels,
            outer_radius_pixels,
            style,
            rgba_color,
            vertices,
        ),
        CirclePitchAlignment::Map => push_ground_marker_fan(
            frame_projection,
            viewport,
            anchor,
            inner_radius_pixels,
            outer_radius_pixels,
            style,
            rgba_color,
            vertices,
        ),
    }
}

/// A screen-facing marker fan: the centre keeps its projected depth and every
/// ring corner offsets from it in screen pixels, scaled by the perspective
/// factor when the style asks the radius to follow the ground plane.
#[allow(clippy::too_many_arguments)]
fn push_screen_marker_fan(
    frame_projection: &OverlayFrameProjection,
    viewport: &ViewportPixels,
    anchor: [f64; 2],
    inner_radius_pixels: f64,
    outer_radius_pixels: f64,
    style: &PointMarkerStyle,
    rgba_color: [f32; 4],
    vertices: &mut Vec<ClipSpaceOverlayVertex>,
) {
    let center_clip_position = frame_projection.ground_clip_position(0.0, anchor[0], anchor[1]);
    if center_clip_position.w <= 0.0 {
        return;
    }
    let radius_scale = match style.pitch_scale {
        CirclePitchScale::Viewport => 1.0,
        CirclePitchScale::Map => ground_pixel_scale(frame_projection, viewport, anchor),
    };
    let center_screen = clip_position_to_screen_pixels(center_clip_position, viewport);
    let center_screen = [
        center_screen[0] + style.translate_pixels[0],
        center_screen[1] + style.translate_pixels[1],
    ];
    let center_corner =
        screen_pixels_to_clip_position(center_screen, center_clip_position, viewport);
    for side_index in 0..POINT_MARKER_FAN_SIDES {
        let start_angle = side_index as f64 * TAU / POINT_MARKER_FAN_SIDES as f64;
        let end_angle = (side_index + 1) as f64 * TAU / POINT_MARKER_FAN_SIDES as f64;
        let ring_corner = |angle: f64, radius_pixels: f64| {
            screen_pixels_to_clip_position(
                [
                    center_screen[0] + radius_pixels * radius_scale * angle.cos(),
                    center_screen[1] + radius_pixels * radius_scale * angle.sin(),
                ],
                center_clip_position,
                viewport,
            )
        };
        let outer_start = ring_corner(start_angle, outer_radius_pixels);
        let outer_end = ring_corner(end_angle, outer_radius_pixels);
        if inner_radius_pixels <= 0.0 {
            push_triangle(
                vertices,
                &[center_corner, outer_start, outer_end],
                rgba_color,
            );
        } else {
            let inner_start = ring_corner(start_angle, inner_radius_pixels);
            let inner_end = ring_corner(end_angle, inner_radius_pixels);
            push_triangle(vertices, &[inner_start, inner_end, outer_end], rgba_color);
            push_triangle(vertices, &[inner_start, outer_end, outer_start], rgba_color);
        }
    }
}

/// A ground-aligned marker fan: every ring corner is an offset on the ground
/// plane, projected with its own depth, so a pitched camera sees an ellipse.
#[allow(clippy::too_many_arguments)]
fn push_ground_marker_fan(
    frame_projection: &OverlayFrameProjection,
    viewport: &ViewportPixels,
    anchor: [f64; 2],
    inner_radius_pixels: f64,
    outer_radius_pixels: f64,
    style: &PointMarkerStyle,
    rgba_color: [f32; 4],
    vertices: &mut Vec<ClipSpaceOverlayVertex>,
) {
    let center_clip_position = frame_projection.ground_clip_position(0.0, anchor[0], anchor[1]);
    if center_clip_position.w <= 0.0 {
        return;
    }
    let world_radius_per_pixel = 1.0 / frame_projection.zoom_scale_pixels;
    let displaced = |clip_position: DVec4| {
        translated_clip_position(clip_position, style.translate_pixels, viewport)
    };
    let center_corner = displaced(center_clip_position);
    let ring_corner = |angle: f64, radius_pixels: f64| {
        displaced(frame_projection.ground_clip_position(
            0.0,
            anchor[0] + radius_pixels * world_radius_per_pixel * angle.cos(),
            anchor[1] + radius_pixels * world_radius_per_pixel * angle.sin(),
        ))
    };
    for side_index in 0..POINT_MARKER_FAN_SIDES {
        let start_angle = side_index as f64 * TAU / POINT_MARKER_FAN_SIDES as f64;
        let end_angle = (side_index + 1) as f64 * TAU / POINT_MARKER_FAN_SIDES as f64;
        let outer_start = ring_corner(start_angle, outer_radius_pixels);
        let outer_end = ring_corner(end_angle, outer_radius_pixels);
        if inner_radius_pixels <= 0.0 {
            push_triangle(
                vertices,
                &[center_corner, outer_start, outer_end],
                rgba_color,
            );
        } else {
            let inner_start = ring_corner(start_angle, inner_radius_pixels);
            let inner_end = ring_corner(end_angle, inner_radius_pixels);
            push_triangle(vertices, &[inner_start, inner_end, outer_end], rgba_color);
            push_triangle(vertices, &[inner_start, outer_end, outer_start], rgba_color);
        }
    }
}

/// How many screen pixels one normalized-world unit spans at `anchor`: the
/// perspective factor a screen-facing marker's radius follows when its style
/// asks for `circle-pitch-scale: map`. It equals the frame's scale on a
/// top-down camera and shrinks with pitch.
fn ground_pixel_scale(
    frame_projection: &OverlayFrameProjection,
    viewport: &ViewportPixels,
    anchor: [f64; 2],
) -> f64 {
    let world_offset = 1.0 / frame_projection.zoom_scale_pixels;
    let anchor_clip_position = frame_projection.ground_clip_position(0.0, anchor[0], anchor[1]);
    let offset_clip_position =
        frame_projection.ground_clip_position(0.0, anchor[0] + world_offset, anchor[1]);
    if anchor_clip_position.w <= 0.0 || offset_clip_position.w <= 0.0 {
        return 1.0;
    }
    let anchor_screen = clip_position_to_screen_pixels(anchor_clip_position, viewport);
    let offset_screen = clip_position_to_screen_pixels(offset_clip_position, viewport);
    let screen_distance =
        (offset_screen[0] - anchor_screen[0]).hypot(offset_screen[1] - anchor_screen[1]);
    let scale = screen_distance * frame_projection.zoom_scale_pixels;
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

/// Assembles one point pin: a teardrop outline anchored with its tip on the
/// projected coordinate and a hollow circular center in the head, emitted as
/// the annulus between the two outlines.
///
/// The teardrop is star-shaped around the head center — every ray from the
/// center crosses the outline exactly once — so both boundaries are radial
/// functions of the same angle and pair up into quads without a bridge. Inside
/// the tangent sector the outline is the straight edge running to the tip
/// (radius `R / cos(angle - tangentAngle)`); outside it is the head circle
/// itself. The hole therefore needs no erase pass, which this pipeline cannot
/// perform (see the module notes on holes).
fn point_pin_triangles(
    frame_projection: &OverlayFrameProjection,
    viewport: &ViewportPixels,
    position: &GeographicCoordinate,
    rgba_color: [f32; 4],
    radius_pixels: f64,
    vertices: &mut Vec<ClipSpaceOverlayVertex>,
) {
    let normalized_world = coordinate_to_normalized_world(position);
    let tip_clip_position = frame_projection.ground_clip_position(
        0.0,
        frame_projection.wrapped_x(normalized_world[0]),
        normalized_world[1],
    );
    if tip_clip_position.w <= 0.0 || !radius_pixels.is_finite() || radius_pixels <= 0.0 {
        return;
    }
    let tip_screen = clip_position_to_screen_pixels(tip_clip_position, viewport);
    let pin_height_pixels = radius_pixels * POINT_PIN_HEIGHT_RADIUS_FACTOR;
    let head_center_screen = [tip_screen[0], tip_screen[1] - pin_height_pixels];
    let hollow_radius_pixels = radius_pixels * POINT_PIN_HOLLOW_RADIUS_RATIO;
    // The head's tangent points sit this far around the head from the tip
    // direction (screen angles grow from +x towards +y, i.e. clockwise in the
    // displayed image, so the tip direction is a quarter turn).
    let tangent_half_angle = (radius_pixels / pin_height_pixels).acos();
    let tip_direction_angle = std::f64::consts::FRAC_PI_2;
    let right_tangent_angle = tip_direction_angle - tangent_half_angle;
    let left_tangent_angle = tip_direction_angle + tangent_half_angle;
    let outline_radius_pixels = |angle: f64| {
        if angle > right_tangent_angle && angle < left_tangent_angle {
            let edge_angle = if angle <= tip_direction_angle {
                right_tangent_angle
            } else {
                left_tangent_angle
            };
            radius_pixels / (angle - edge_angle).cos()
        } else {
            radius_pixels
        }
    };
    // Both outlines share the tip's clip-space reference, so the whole pin
    // stays screen-facing at the anchor's depth like a marker fan does.
    let corner_clip_position = |radius: f64, angle: f64| {
        screen_pixels_to_clip_position(
            [
                head_center_screen[0] + radius * angle.cos(),
                head_center_screen[1] + radius * angle.sin(),
            ],
            tip_clip_position,
            viewport,
        )
    };
    for side_index in 0..POINT_PIN_RING_SIDES {
        let start_angle = side_index as f64 * TAU / POINT_PIN_RING_SIDES as f64;
        let end_angle = (side_index + 1) as f64 * TAU / POINT_PIN_RING_SIDES as f64;
        let inner_start = corner_clip_position(hollow_radius_pixels, start_angle);
        let inner_end = corner_clip_position(hollow_radius_pixels, end_angle);
        let outer_start = corner_clip_position(outline_radius_pixels(start_angle), start_angle);
        let outer_end = corner_clip_position(outline_radius_pixels(end_angle), end_angle);
        push_triangle(vertices, &[inner_start, inner_end, outer_end], rgba_color);
        push_triangle(vertices, &[inner_start, outer_end, outer_start], rgba_color);
    }
}

/// Assembles a filled polygon: the ring set (outer boundary plus holes)
/// triangulates in continuous normalized-world space, every triangle splits
/// against the world-copy windows (convex clipping keeps the pieces convex),
/// and each clipped piece fans into triangles projected by their genuine
/// clip-space corners. Hole rings anchor to the outer ring's world copy (see
/// [`OverlayFrameProjection::wrapped_continuous_chain_near`]) so a polygon
/// straddling the antimeridian keeps its holes with its exterior. Shared
/// (crate-visible) with the location-indicator layer, whose ground-projected
/// accuracy ring reuses the exact fill machinery (without holes).
/// The width of a filled polygon's explicit outline, in screen pixels; the
/// specification draws `fill-outline-color` as a one-pixel edge.
const FILL_OUTLINE_WIDTH_PIXELS: f64 = 1.0;

/// The resolved paint of one filled polygon.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FilledPolygonStyle {
    /// Fill color and opacity.
    pub rgba_color: [f32; 4],
    /// Edge color, when the style declares one.
    pub rgba_outline_color: Option<[f32; 4]>,
    /// Whether the edge is antialiased; a disabled edge paints no outline.
    pub antialias: bool,
    /// The fill's displacement from its geometry, in screen pixels.
    pub translate_pixels: [f64; 2],
    /// The frame of reference the displacement is measured against.
    pub translate_anchor: TranslateAnchor,
}

/// The screen-pixel displacement a translate property resolves to at the
/// camera's bearing.
///
/// A `map`-anchored displacement is measured on the map, so the camera's
/// bearing rotates it into the viewport frame: at bearing zero the two
/// anchors agree, and a north displacement turns left as the map turns east
/// up. A `viewport`-anchored displacement is already in screen pixels.
pub(crate) fn translate_pixels_for_anchor(
    translate_pixels: [f64; 2],
    translate_anchor: TranslateAnchor,
    bearing_degrees: f64,
) -> [f64; 2] {
    match translate_anchor {
        TranslateAnchor::Viewport => translate_pixels,
        TranslateAnchor::Map => {
            let (bearing_sine, bearing_cosine) = bearing_degrees.to_radians().sin_cos();
            [
                translate_pixels[0] * bearing_cosine + translate_pixels[1] * bearing_sine,
                -translate_pixels[0] * bearing_sine + translate_pixels[1] * bearing_cosine,
            ]
        }
    }
}

/// Applies a screen-pixel displacement to one projected clip-space corner.
/// A corner at or behind the camera keeps its own position: the offset needs
/// a positive homogeneous weight, and the GPU still clips the triangle.
fn translated_clip_position(
    corner: DVec4,
    translate_pixels: [f64; 2],
    viewport: &ViewportPixels,
) -> [f64; 4] {
    if (translate_pixels[0] == 0.0 && translate_pixels[1] == 0.0) || corner.w <= 0.0 {
        return [corner.x, corner.y, corner.z, corner.w];
    }
    let screen_pixels = clip_position_to_screen_pixels(corner, viewport);
    screen_pixels_to_clip_position(
        [
            screen_pixels[0] + translate_pixels[0],
            screen_pixels[1] + translate_pixels[1],
        ],
        corner,
        viewport,
    )
}

pub(crate) fn filled_polygon_triangles(
    frame_projection: &OverlayFrameProjection,
    viewport: &ViewportPixels,
    ring_coordinates: &[GeographicCoordinate],
    hole_ring_coordinates: &[Vec<GeographicCoordinate>],
    style: FilledPolygonStyle,
    vertices: &mut Vec<ClipSpaceOverlayVertex>,
) {
    let wrapped_ring = frame_projection.wrapped_continuous_chain(ring_coordinates);
    let Some(reference_x) = wrapped_ring.first().map(|vertex| vertex[0]) else {
        return;
    };
    let wrapped_holes: Vec<Vec<[f64; 2]>> = hole_ring_coordinates
        .iter()
        .map(|hole_ring| frame_projection.wrapped_continuous_chain_near(reference_x, hole_ring))
        .collect();
    // A map-anchored displacement rotates with the camera's bearing; a
    // viewport-anchored one is already in screen pixels.
    let translate_pixels = translate_pixels_for_anchor(
        style.translate_pixels,
        style.translate_anchor,
        frame_projection.map_bearing_degrees(),
    );
    let triangle_minimum_x =
        |triangle: &[[f64; 2]; 3]| triangle[0][0].min(triangle[1][0]).min(triangle[2][0]);
    let triangle_maximum_x =
        |triangle: &[[f64; 2]; 3]| triangle[0][0].max(triangle[1][0]).max(triangle[2][0]);
    for triangle in ear_clip_triangulation_with_holes(&wrapped_ring, &wrapped_holes) {
        let (triangle_minimum_x, triangle_maximum_x) =
            (triangle_minimum_x(&triangle), triangle_maximum_x(&triangle));
        for window_offset in WORLD_COPY_WINDOW_OFFSETS {
            let window_offset_f64 = f64::from(window_offset);
            let [window_start_x, window_end_x] = frame_projection.window_x_range(window_offset_f64);
            // A triangle lying entirely outside one of the window's two
            // half-planes clips down to the boundary line itself — fewer than
            // three corners, and the fan below would draw nothing. Rejecting
            // it by its x-extent is exact and saves the whole clip.
            if triangle_maximum_x <= window_start_x || triangle_minimum_x >= window_end_x {
                continue;
            }
            // A triangle wholly inside both half-planes passes through the
            // clip unchanged — the overwhelmingly common case, since the
            // windows are a third of the world apart — so it projects its own
            // three corners and skips two Sutherland–Hodgman passes.
            let clipped_ring =
                if triangle_minimum_x >= window_start_x && triangle_maximum_x <= window_end_x {
                    WindowClippedTriangle {
                        corners: [triangle[0], triangle[1], triangle[2], [0.0; 2], [0.0; 2]],
                        corner_count: 3,
                    }
                } else {
                    clip_triangle_to_x_range(&triangle, window_start_x, window_end_x)
                };
            if clipped_ring.corner_count < 3 {
                continue;
            }
            // Clipped corners project by their genuine clip-space positions,
            // so a triangle straddling the near plane stays for the GPU to
            // clip exactly (see the module notes); only an f32-overflowing
            // corner drops via `push_triangle`.
            for fan_index in 1..clipped_ring.corner_count - 1 {
                let corner_clip_positions = [
                    clipped_ring.corners[0],
                    clipped_ring.corners[fan_index],
                    clipped_ring.corners[fan_index + 1],
                ]
                .map(|corner| {
                    let corner_clip_position = frame_projection.ground_clip_position(
                        window_offset_f64,
                        corner[0],
                        corner[1],
                    );
                    translated_clip_position(corner_clip_position, translate_pixels, viewport)
                });
                push_triangle(vertices, &corner_clip_positions, style.rgba_color);
            }
        }
    }
    // An explicitly declared edge colour paints the ring boundaries as a
    // one-pixel outline; a style that disables antialiasing paints no edge.
    if style.antialias
        && let Some(outline_rgba) = style.rgba_outline_color
    {
        emit_filled_polygon_outline(
            frame_projection,
            viewport,
            ring_coordinates,
            hole_ring_coordinates,
            outline_rgba,
            translate_pixels,
            vertices,
        );
    }
}

/// Paints a filled polygon's explicit outline: the outer ring and every hole
/// boundary as a closed one-pixel polyline at the fill's own displacement.
fn emit_filled_polygon_outline(
    frame_projection: &OverlayFrameProjection,
    viewport: &ViewportPixels,
    ring_coordinates: &[GeographicCoordinate],
    hole_ring_coordinates: &[Vec<GeographicCoordinate>],
    outline_rgba: [f32; 4],
    translate_pixels: [f64; 2],
    vertices: &mut Vec<ClipSpaceOverlayVertex>,
) {
    let mut closed_ring = ring_coordinates.to_vec();
    if let (Some(first), Some(last)) = (closed_ring.first(), closed_ring.last())
        && first != last
    {
        closed_ring.push(*first);
    }
    if closed_ring.len() >= 2 {
        thickened_polyline_triangles(
            frame_projection,
            viewport,
            &closed_ring,
            outline_rgba,
            FILL_OUTLINE_WIDTH_PIXELS,
            translate_pixels,
            vertices,
        );
    }
    for hole_ring in hole_ring_coordinates {
        let mut closed_hole = hole_ring.clone();
        if let (Some(first), Some(last)) = (closed_hole.first(), closed_hole.last())
            && first != last
        {
            closed_hole.push(*first);
        }
        if closed_hole.len() >= 2 {
            thickened_polyline_triangles(
                frame_projection,
                viewport,
                &closed_hole,
                outline_rgba,
                FILL_OUTLINE_WIDTH_PIXELS,
                translate_pixels,
                vertices,
            );
        }
    }
}

/// A triangle clipped against one world-copy window: a convex ring of at most
/// [`MAXIMUM_WINDOW_CLIPPED_CORNERS`] corners.
///
/// The corners live in a fixed array because this runs once per triangle per
/// window on every frame — hundreds of thousands of times during a pan — and
/// the heap allocation a `Vec` would make per call is a measurable share of
/// the frame. The arithmetic is the same Sutherland–Hodgman the
/// allocation-based clipper uses, in the same order, so the drawn vertices are
/// identical.
struct WindowClippedTriangle {
    corners: [[f64; 2]; MAXIMUM_WINDOW_CLIPPED_CORNERS],
    corner_count: usize,
}

/// Clips one triangle to `window_start_x <= x < window_end_x` without
/// allocating: one Sutherland–Hodgman pass per half-plane over fixed buffers.
fn clip_triangle_to_x_range(
    triangle: &[[f64; 2]; 3],
    window_start_x: f64,
    window_end_x: f64,
) -> WindowClippedTriangle {
    let mut first_clip = [[0.0_f64; 2]; MAXIMUM_WINDOW_CLIPPED_CORNERS];
    let first_clip_count =
        clip_ring_to_x_half_plane(triangle, true, window_start_x, &mut first_clip);
    let mut second_clip = [[0.0_f64; 2]; MAXIMUM_WINDOW_CLIPPED_CORNERS];
    let second_clip_count = clip_ring_to_x_half_plane(
        &first_clip[..first_clip_count],
        false,
        window_end_x,
        &mut second_clip,
    );
    WindowClippedTriangle {
        corners: second_clip,
        corner_count: second_clip_count,
    }
}

/// One Sutherland–Hodgman pass of a convex ring against a vertical half-plane
/// (`keep_greater` selects `x >= boundary_x`), writing into a fixed buffer.
/// [`clip_polygon_to_x_half_plane`] is the allocating form with the same
/// arithmetic; this one is the frame-path copy.
fn clip_ring_to_x_half_plane(
    polygon: &[[f64; 2]],
    keep_greater: bool,
    boundary_x: f64,
    clipped_polygon: &mut [[f64; 2]; MAXIMUM_WINDOW_CLIPPED_CORNERS],
) -> usize {
    let mut clipped_corner_count = 0;
    for edge_index in 0..polygon.len() {
        let edge_start = polygon[edge_index];
        let edge_end = polygon[(edge_index + 1) % polygon.len()];
        let start_inside = if keep_greater {
            edge_start[0] >= boundary_x
        } else {
            edge_start[0] <= boundary_x
        };
        let end_inside = if keep_greater {
            edge_end[0] >= boundary_x
        } else {
            edge_end[0] <= boundary_x
        };
        if start_inside && clipped_corner_count < MAXIMUM_WINDOW_CLIPPED_CORNERS {
            clipped_polygon[clipped_corner_count] = edge_start;
            clipped_corner_count += 1;
        }
        if start_inside != end_inside {
            let crossing_fraction = (boundary_x - edge_start[0]) / (edge_end[0] - edge_start[0]);
            // A crossing at the edge's own endpoint is a vertex on the
            // boundary; it already landed through the inside-start pushes of
            // the neighboring edges, and pushing it again would degenerate
            // the clipped ring into a zero-area fan triangle.
            if crossing_fraction > 0.0
                && crossing_fraction < 1.0
                && clipped_corner_count < MAXIMUM_WINDOW_CLIPPED_CORNERS
            {
                clipped_polygon[clipped_corner_count] = [
                    boundary_x,
                    edge_start[1] + (edge_end[1] - edge_start[1]) * crossing_fraction,
                ];
                clipped_corner_count += 1;
            }
        }
    }
    clipped_corner_count
}

/// Doubled signed area of a ring (the fan-formula sum, without the constant
/// factor): positive for one winding, negative for the other, zero for a
/// collinear ring.
fn ring_doubled_signed_area(ring: &[[f64; 2]]) -> f64 {
    ring.iter()
        .enumerate()
        .map(|(index, vertex)| {
            let next = ring[(index + 1) % ring.len()];
            vertex[0] * next[1] - next[0] * vertex[1]
        })
        .sum()
}

/// Twice the signed area of one triangle (a × b cross-product form): positive
/// for counterclockwise winding, negative for clockwise.
fn triangle_doubled_signed_area(
    first_vertex: [f64; 2],
    second_vertex: [f64; 2],
    third_vertex: [f64; 2],
) -> f64 {
    (second_vertex[0] - first_vertex[0]) * (third_vertex[1] - first_vertex[1])
        - (second_vertex[1] - first_vertex[1]) * (third_vertex[0] - first_vertex[0])
}

/// Whether a point lies inside a triangle or on its boundary, by winding-sign
/// tests against the triangle's (consistently wound) edges.
fn point_lies_within_triangle(
    point: [f64; 2],
    first_vertex: [f64; 2],
    second_vertex: [f64; 2],
    third_vertex: [f64; 2],
) -> bool {
    triangle_doubled_signed_area(point, first_vertex, second_vertex)
        >= -EAR_CLIPPING_BOUNDARY_TOLERANCE
        && triangle_doubled_signed_area(point, second_vertex, third_vertex)
            >= -EAR_CLIPPING_BOUNDARY_TOLERANCE
        && triangle_doubled_signed_area(point, third_vertex, first_vertex)
            >= -EAR_CLIPPING_BOUNDARY_TOLERANCE
}

/// Triangulates a simple polygon ring by ear clipping, over the ring's own
/// vertices in O(n²) — no external geometry dependency, consistent with the
/// crate owning its minimal math.
///
/// The ring normalizes to one winding (positive doubled signed area) first,
/// so input order does not matter. Degenerate rings (fewer than three
/// vertices, near-zero area, or a remainder with no clipgable ear — fully
/// collinear chains) contribute fewer triangles or none; a numerically stuck
/// remainder stops the loop instead of spinning, matching the documented
/// "degenerate rings may fill nothing" contract.
fn ear_clip_triangulation(ring: &[[f64; 2]]) -> Vec<[[f64; 2]; 3]> {
    let mut normalized_ring = ring.to_vec();
    if normalized_ring.len() >= 2 && normalized_ring.first() == normalized_ring.last() {
        normalized_ring.pop();
    }
    if normalized_ring.len() < 3 {
        return Vec::new();
    }
    let doubled_signed_area = ring_doubled_signed_area(&normalized_ring);
    if doubled_signed_area.abs() < POLYGON_AREA_DEGENERACY_TOLERANCE {
        return Vec::new();
    }
    if doubled_signed_area < 0.0 {
        normalized_ring.reverse();
    }
    let mut remaining_indices: Vec<usize> = (0..normalized_ring.len()).collect();
    let mut triangles = Vec::with_capacity(normalized_ring.len() - 2);
    while remaining_indices.len() > 3 {
        let ear_position = (0..remaining_indices.len())
            .find(|&position| is_ring_ear(&normalized_ring, &remaining_indices, position));
        let Some(ear_position) = ear_position else {
            break;
        };
        let corner_indices = ear_corner_indices(&remaining_indices, ear_position);
        triangles.push([
            normalized_ring[corner_indices[0]],
            normalized_ring[corner_indices[1]],
            normalized_ring[corner_indices[2]],
        ]);
        remaining_indices.remove(ear_position);
    }
    if remaining_indices.len() == 3 {
        triangles.push([
            normalized_ring[remaining_indices[0]],
            normalized_ring[remaining_indices[1]],
            normalized_ring[remaining_indices[2]],
        ]);
    }
    triangles
}

/// Triangulates a ring set with holes: the outer boundary plus every hole,
/// bridge-joined into one simple ring and ear-clip triangulated in one pass.
///
/// The hole handling follows the standard bridge-splicing construction (the
/// `earcut` algorithm's `eliminateHoles` step) with the crate's own vector
/// math: each hole joins the boundary through a zero-width slit, so the
/// emitted triangles cover the ring set's interior minus the holes and never
/// the holes themselves. Winding normalizes first (outer positive, holes
/// negative), so the decoded ring order does not matter.
///
/// A degenerate outer ring fills nothing, and a hole ring that cannot be
/// bridged (fewer than three vertices, zero area, or no visible bridge) is
/// skipped rather than approximated; a numerically stuck ear-clipping
/// remainder stops early and leaves that remainder unfilled, matching the
/// documented "degenerate rings may fill nothing" contract.
fn ear_clip_triangulation_with_holes(
    outer_ring: &[[f64; 2]],
    hole_rings: &[Vec<[f64; 2]>],
) -> Vec<[[f64; 2]; 3]> {
    if hole_rings.is_empty() {
        return ear_clip_triangulation(outer_ring);
    }
    let normalized_outer = normalized_ring(outer_ring, true);
    if normalized_outer.len() < 3 {
        return Vec::new();
    }
    let mut nodes = Vec::new();
    let Some(ring_head) = ring_nodes(&normalized_outer, &mut nodes) else {
        return Vec::new();
    };
    let mut holes: Vec<Vec<[f64; 2]>> = hole_rings
        .iter()
        .map(|hole_ring| normalized_ring(hole_ring, false))
        .filter(|hole_ring| hole_ring.len() >= 3)
        .collect();
    // Left-to-right hole order, matching the bridge search's proven order:
    // each hole's westward ray then meets only edges already part of the
    // combined boundary.
    holes.sort_by(|left, right| {
        ring_leftmost_point(left)[0]
            .partial_cmp(&ring_leftmost_point(right)[0])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for hole_ring in &holes {
        let Some(hole_head) = ring_nodes(hole_ring, &mut nodes) else {
            continue;
        };
        // The bridge search validates a bridge from the hole's leftmost
        // vertex; the splice must start the hole at that same vertex, or the
        // cycle would gain an unvalidated edge across the hole.
        let hole_leftmost = hole_leftmost_node(hole_head, &nodes);
        let Some(bridge_index) = find_hole_bridge(hole_leftmost, ring_head, &nodes) else {
            continue;
        };
        splice_hole_ring(bridge_index, hole_leftmost, &mut nodes);
    }
    ear_clip_nodes(&mut nodes, ring_head)
}

/// One node of the mutable circular ring list the hole-aware triangulator
/// clips ears from. Nodes are never freed: removal relinks the neighbors and
/// leaves the node unreachable, so arena indices stay valid across removals.
struct EarClipNode {
    /// The node's position in normalized-world space.
    vertex: [f64; 2],
    /// Arena index of the previous ring node.
    previous_index: usize,
    /// Arena index of the next ring node.
    next_index: usize,
}

/// Builds a circular node ring from a ring's vertices, dropping the repeated
/// closing vertex and consecutive duplicates. Returns the first node's arena
/// index, or `None` when fewer than three distinct vertices remain.
fn ring_nodes(points: &[[f64; 2]], nodes: &mut Vec<EarClipNode>) -> Option<usize> {
    let mut cleaned: Vec<[f64; 2]> = Vec::with_capacity(points.len());
    for &point in points {
        if cleaned.last() != Some(&point) {
            cleaned.push(point);
        }
    }
    if cleaned.len() >= 2 && cleaned.first() == cleaned.last() {
        cleaned.pop();
    }
    if cleaned.len() < 3 {
        return None;
    }
    let first_index = nodes.len();
    for &vertex in &cleaned {
        nodes.push(EarClipNode {
            vertex,
            previous_index: 0,
            next_index: 0,
        });
    }
    let vertex_count = cleaned.len();
    for position in 0..vertex_count {
        let node_index = first_index + position;
        nodes[node_index].previous_index =
            first_index + (position + vertex_count - 1) % vertex_count;
        nodes[node_index].next_index = first_index + (position + 1) % vertex_count;
    }
    Some(first_index)
}

/// Normalizes a ring to one winding: drops the repeated closing vertex and
/// consecutive duplicates, reverses the ring when its signed area has the
/// wrong sign, and returns an empty ring for fewer than three vertices or
/// near-zero area.
fn normalized_ring(ring: &[[f64; 2]], positive_winding: bool) -> Vec<[f64; 2]> {
    let mut normalized: Vec<[f64; 2]> = Vec::with_capacity(ring.len());
    for &point in ring {
        if normalized.last() != Some(&point) {
            normalized.push(point);
        }
    }
    if normalized.len() >= 2 && normalized.first() == normalized.last() {
        normalized.pop();
    }
    if normalized.len() < 3 {
        return Vec::new();
    }
    let doubled_signed_area = ring_doubled_signed_area(&normalized);
    if doubled_signed_area.abs() < POLYGON_AREA_DEGENERACY_TOLERANCE {
        return Vec::new();
    }
    if (doubled_signed_area > 0.0) != positive_winding {
        normalized.reverse();
    }
    normalized
}

/// The ring vertex with the smallest x (the bridge search's ray origin side).
fn ring_leftmost_point(ring: &[[f64; 2]]) -> [f64; 2] {
    ring.iter()
        .copied()
        .fold([f64::INFINITY, f64::INFINITY], |leftmost, point| {
            if point[0] < leftmost[0] {
                point
            } else {
                leftmost
            }
        })
}

/// Splices a hole ring into the outer ring at a bridge vertex: the outer
/// boundary turns into the hole, walks the hole ring, crosses back over the
/// zero-width bridge, and continues with the outer ring. `hole_head` must be
/// a node of the hole ring; the hole enters the cycle after `bridge_index`.
/// Returns the hole-head copy, the second cycle's head when the same wiring
/// splits a self-touching ring at a diagonal.
fn splice_hole_ring(bridge_index: usize, hole_head: usize, nodes: &mut Vec<EarClipNode>) -> usize {
    let bridge_copy_index = nodes.len();
    nodes.push(EarClipNode {
        vertex: nodes[bridge_index].vertex,
        previous_index: 0,
        next_index: 0,
    });
    let hole_head_copy_index = nodes.len();
    nodes.push(EarClipNode {
        vertex: nodes[hole_head].vertex,
        previous_index: 0,
        next_index: 0,
    });
    let bridge_next_index = nodes[bridge_index].next_index;
    let hole_last_index = nodes[hole_head].previous_index;
    // bridge -> hole head ... hole last -> hole head copy -> bridge copy ->
    // (original bridge next), the standard splitPolygon wiring.
    nodes[bridge_index].next_index = hole_head;
    nodes[hole_head].previous_index = bridge_index;
    nodes[hole_last_index].next_index = hole_head_copy_index;
    nodes[hole_head_copy_index].previous_index = hole_last_index;
    nodes[hole_head_copy_index].next_index = bridge_copy_index;
    nodes[bridge_copy_index].previous_index = hole_head_copy_index;
    nodes[bridge_copy_index].next_index = bridge_next_index;
    nodes[bridge_next_index].previous_index = bridge_copy_index;
    hole_head_copy_index
}

/// The hole ring vertex with the smallest x: the bridge search's ray origin
/// and the vertex the splice must enter the hole through.
fn hole_leftmost_node(hole_head: usize, nodes: &[EarClipNode]) -> usize {
    let mut hole_leftmost = hole_head;
    let mut hole_node = nodes[hole_head].next_index;
    while hole_node != hole_head {
        if nodes[hole_node].vertex[0] < nodes[hole_leftmost].vertex[0] {
            hole_leftmost = hole_node;
        }
        hole_node = nodes[hole_node].next_index;
    }
    hole_leftmost
}

/// The outer ring node a hole ring should bridge to, or `None` when the hole
/// does not overlap the ring at all.
///
/// This is the `earcut` hole-bridge search: cast a ray westward from the
/// hole's leftmost vertex, take the nearest boundary edge it crosses, then
/// refine over the vertices of that crossing's wedge to the one with the
/// minimum angle that is locally inside the boundary. The refinement is what
/// keeps the bridge segment from crossing the ring on concave boundaries.
fn find_hole_bridge(
    hole_leftmost: usize,
    ring_head: usize,
    nodes: &[EarClipNode],
) -> Option<usize> {
    let [hole_x, hole_y] = nodes[hole_leftmost].vertex;

    // First pass: the nearest boundary edge to the west of the hole's
    // leftmost vertex. The candidate is that edge's endpoint with the lesser
    // x, the side the hole approaches from.
    let mut nearest_crossing_x = f64::NEG_INFINITY;
    let mut bridge_index: Option<usize> = None;
    let mut ring_node = ring_head;
    loop {
        let ring_next = nodes[ring_node].next_index;
        let ring_vertex = nodes[ring_node].vertex;
        let ring_next_vertex = nodes[ring_next].vertex;
        if hole_y <= ring_vertex[1]
            && hole_y >= ring_next_vertex[1]
            && ring_next_vertex[1] != ring_vertex[1]
        {
            let crossing_x = ring_vertex[0]
                + (hole_y - ring_vertex[1]) * (ring_next_vertex[0] - ring_vertex[0])
                    / (ring_next_vertex[1] - ring_vertex[1]);
            if crossing_x <= hole_x && crossing_x > nearest_crossing_x {
                nearest_crossing_x = crossing_x;
                bridge_index = Some(if ring_vertex[0] < ring_next_vertex[0] {
                    ring_node
                } else {
                    ring_next
                });
                if crossing_x == hole_x {
                    return bridge_index;
                }
            }
        }
        ring_node = ring_next;
        if ring_node == ring_head {
            break;
        }
    }
    let initial_bridge_index = bridge_index?;
    let initial_bridge_vertex = nodes[initial_bridge_index].vertex;
    let stop_index = initial_bridge_index;
    let mut minimum_tangent = f64::INFINITY;
    let mut ring_node = initial_bridge_index;
    loop {
        let ring_vertex = nodes[ring_node].vertex;
        if hole_x >= ring_vertex[0]
            && ring_vertex[0] >= initial_bridge_vertex[0]
            && hole_x != ring_vertex[0]
            && point_lies_within_earcut_triangle(
                [
                    if hole_y < initial_bridge_vertex[1] {
                        hole_x
                    } else {
                        nearest_crossing_x
                    },
                    hole_y,
                ],
                initial_bridge_vertex,
                [
                    if hole_y < initial_bridge_vertex[1] {
                        nearest_crossing_x
                    } else {
                        hole_x
                    },
                    hole_y,
                ],
                ring_vertex,
            )
        {
            let tangent = (hole_y - ring_vertex[1]).abs() / (hole_x - ring_vertex[0]);
            if node_locally_inside(nodes, ring_node, [hole_x, hole_y]) && tangent < minimum_tangent
            {
                minimum_tangent = tangent;
                bridge_index = Some(ring_node);
            }
        }
        ring_node = nodes[ring_node].next_index;
        if ring_node == stop_index {
            break;
        }
    }
    bridge_index
}

/// Earcut's point-in-triangle test, written in the winding convention the
/// bridge search assumes (outer rings positive).
fn point_lies_within_earcut_triangle(
    point: [f64; 2],
    first: [f64; 2],
    second: [f64; 2],
    third: [f64; 2],
) -> bool {
    (third[0] - point[0]) * (first[1] - point[1]) >= (first[0] - point[0]) * (third[1] - point[1])
        && (first[0] - point[0]) * (second[1] - point[1])
            >= (second[0] - point[0]) * (first[1] - point[1])
        && (second[0] - point[0]) * (third[1] - point[1])
            >= (third[0] - point[0]) * (second[1] - point[1])
}

/// Whether the boundary is locally inside at a ring node for a candidate
/// bridge endpoint point: for a convex vertex the point must sit between the
/// node's two boundary edges, for a reflex vertex outside both.
fn node_locally_inside(nodes: &[EarClipNode], node_index: usize, point: [f64; 2]) -> bool {
    let previous_vertex = nodes[nodes[node_index].previous_index].vertex;
    let vertex = nodes[node_index].vertex;
    let next_vertex = nodes[nodes[node_index].next_index].vertex;
    if triangle_doubled_signed_area(previous_vertex, vertex, next_vertex) > 0.0 {
        triangle_doubled_signed_area(vertex, point, next_vertex) <= 0.0
            && triangle_doubled_signed_area(vertex, previous_vertex, point) <= 0.0
    } else {
        triangle_doubled_signed_area(vertex, point, previous_vertex) > 0.0
            || triangle_doubled_signed_area(vertex, next_vertex, point) > 0.0
    }
}

/// [`node_locally_inside`] with another ring node as the point to test.
fn node_locally_inside_node(nodes: &[EarClipNode], node_index: usize, target_index: usize) -> bool {
    node_locally_inside(nodes, node_index, nodes[target_index].vertex)
}

/// Whether the node at `ear_index` is a valid ear: convex, and with no
/// reflex boundary vertex inside or on its triangle. Duplicate bridge
/// vertices are collinear slits, so the reflex-only containment filter keeps
/// them from blocking every ear next to a splice.
fn is_ear_node(nodes: &[EarClipNode], ear_index: usize) -> bool {
    let previous_index = nodes[ear_index].previous_index;
    let next_index = nodes[ear_index].next_index;
    let previous_vertex = nodes[previous_index].vertex;
    let vertex = nodes[ear_index].vertex;
    let next_vertex = nodes[next_index].vertex;
    if triangle_doubled_signed_area(previous_vertex, vertex, next_vertex) <= 0.0 {
        return false;
    }
    let minimum_x = previous_vertex[0].min(vertex[0]).min(next_vertex[0]);
    let maximum_x = previous_vertex[0].max(vertex[0]).max(next_vertex[0]);
    let minimum_y = previous_vertex[1].min(vertex[1]).min(next_vertex[1]);
    let maximum_y = previous_vertex[1].max(vertex[1]).max(next_vertex[1]);
    let mut point_index = nodes[next_index].next_index;
    while point_index != previous_index {
        let point = nodes[point_index].vertex;
        if point[0] >= minimum_x
            && point[0] <= maximum_x
            && point[1] >= minimum_y
            && point[1] <= maximum_y
            && point_lies_within_earcut_triangle(point, previous_vertex, vertex, next_vertex)
        {
            let point_previous_vertex = nodes[nodes[point_index].previous_index].vertex;
            let point_next_vertex = nodes[nodes[point_index].next_index].vertex;
            if triangle_doubled_signed_area(point_previous_vertex, point, point_next_vertex) <= 0.0
            {
                return false;
            }
        }
        point_index = nodes[point_index].next_index;
    }
    true
}

/// Unlinks a node from the circular ring list, leaving it unreachable.
fn remove_ear_node(nodes: &mut [EarClipNode], node_index: usize) {
    let previous_index = nodes[node_index].previous_index;
    let next_index = nodes[node_index].next_index;
    nodes[previous_index].next_index = next_index;
    nodes[next_index].previous_index = previous_index;
}

/// Ear-clip loops over the assembled node ring, emitting each ear's triangle
/// and relinking around it. The traversal advances past the removed ear like
/// `earcut`, so well-formed rings stay near linear. Stalls recover in the
/// `earcut` order: first drop duplicate and collinear nodes, then cure local
/// self-intersections, then split the ring at a valid diagonal and clip both
/// halves. A ring that still has no ear stops instead of spinning, leaving
/// that remainder unfilled.
fn ear_clip_nodes(nodes: &mut Vec<EarClipNode>, ring_head: usize) -> Vec<[[f64; 2]; 3]> {
    let mut triangles = Vec::new();
    let mut ear_index = ring_head;
    let mut stop_index = ring_head;
    let mut filtered_once = false;
    let mut cured_once = false;
    while nodes[ear_index].previous_index != nodes[ear_index].next_index {
        let previous_index = nodes[ear_index].previous_index;
        let next_index = nodes[ear_index].next_index;
        if is_ear_node(nodes, ear_index) {
            triangles.push([
                nodes[previous_index].vertex,
                nodes[ear_index].vertex,
                nodes[next_index].vertex,
            ]);
            remove_ear_node(nodes, ear_index);
            ear_index = nodes[next_index].next_index;
            stop_index = ear_index;
            continue;
        }
        ear_index = next_index;
        if ear_index == stop_index {
            if !filtered_once {
                filtered_once = true;
                if let Some(filtered_head) = filter_ring_nodes(nodes, ear_index) {
                    ear_index = filtered_head;
                    stop_index = filtered_head;
                    continue;
                }
            }
            if !cured_once {
                cured_once = true;
                let cured_head = cure_local_intersections(nodes, ear_index, &mut triangles);
                ear_index = cured_head;
                stop_index = cured_head;
                continue;
            }
            if let Some((first_head, second_head)) = split_ring_at_valid_diagonal(nodes, ear_index)
            {
                triangles.extend(ear_clip_nodes(nodes, first_head));
                triangles.extend(ear_clip_nodes(nodes, second_head));
                return triangles;
            }
            break;
        }
    }
    triangles
}

/// The ear clipper's last resort (`earcut`'s `splitEarcut`): find any pair of
/// non-adjacent ring nodes joined by a valid diagonal, split the ring there
/// into two smaller cycles, and return both cycle heads. Splitting strictly
/// shrinks each piece, so the recursive call always terminates.
fn split_ring_at_valid_diagonal(
    nodes: &mut Vec<EarClipNode>,
    start: usize,
) -> Option<(usize, usize)> {
    let mut first = start;
    loop {
        let mut second = nodes[nodes[first].next_index].next_index;
        while second != nodes[first].previous_index {
            if is_valid_diagonal(nodes, first, second) {
                let second_head = splice_hole_ring(first, second, nodes);
                return Some((first, second_head));
            }
            second = nodes[second].next_index;
        }
        first = nodes[first].next_index;
        if first == start {
            break;
        }
    }
    None
}

/// Whether the diagonal between two ring nodes is safe to split at: the nodes
/// are non-adjacent, both ends are locally inside, the diagonal's midpoint is
/// inside the ring, it crosses no boundary edge, and it does not create
/// opposite-facing sectors (`earcut`'s `isValidDiagonal`).
fn is_valid_diagonal(nodes: &[EarClipNode], first: usize, second: usize) -> bool {
    let first_previous = nodes[first].previous_index;
    let first_next = nodes[first].next_index;
    let second_previous = nodes[second].previous_index;
    let second_next = nodes[second].next_index;
    first_next != second
        && first_previous != second
        && !diagonal_intersects_ring(nodes, first, second)
        && node_locally_inside_node(nodes, first, second)
        && node_locally_inside_node(nodes, second, first)
        && diagonal_middle_inside(nodes, first, second)
        && (triangle_doubled_signed_area(
            nodes[first_previous].vertex,
            nodes[first].vertex,
            nodes[second_previous].vertex,
        ) != 0.0
            || triangle_doubled_signed_area(
                nodes[first].vertex,
                nodes[second_previous].vertex,
                nodes[second].vertex,
            ) != 0.0
            || (nodes[first].vertex == nodes[second].vertex
                && triangle_doubled_signed_area(
                    nodes[first_previous].vertex,
                    nodes[first].vertex,
                    nodes[first_next].vertex,
                ) > 0.0
                && triangle_doubled_signed_area(
                    nodes[second_previous].vertex,
                    nodes[second].vertex,
                    nodes[second_next].vertex,
                ) > 0.0))
}

/// Whether a candidate split diagonal crosses any ring edge (`earcut`'s
/// `intersectsPolygon`).
fn diagonal_intersects_ring(nodes: &[EarClipNode], first: usize, second: usize) -> bool {
    let mut node = first;
    loop {
        let next = nodes[node].next_index;
        if node != first
            && next != first
            && node != second
            && next != second
            && segments_intersect(
                nodes[node].vertex,
                nodes[next].vertex,
                nodes[first].vertex,
                nodes[second].vertex,
            )
        {
            return true;
        }
        node = next;
        if node == first {
            break;
        }
    }
    false
}

/// Whether a candidate split diagonal's midpoint lies inside the ring
/// (even-odd ray cast; `earcut`'s `middleInside`).
fn diagonal_middle_inside(nodes: &[EarClipNode], first: usize, second: usize) -> bool {
    let midpoint = [
        (nodes[first].vertex[0] + nodes[second].vertex[0]) * 0.5,
        (nodes[first].vertex[1] + nodes[second].vertex[1]) * 0.5,
    ];
    let mut inside = false;
    let mut node = first;
    loop {
        let next = nodes[node].next_index;
        let current_vertex = nodes[node].vertex;
        let next_vertex = nodes[next].vertex;
        if (current_vertex[1] > midpoint[1]) != (next_vertex[1] > midpoint[1])
            && next_vertex[1] != current_vertex[1]
        {
            let boundary_x = (next_vertex[0] - current_vertex[0])
                * (midpoint[1] - current_vertex[1])
                / (next_vertex[1] - current_vertex[1])
                + current_vertex[0];
            if midpoint[0] < boundary_x {
                inside = !inside;
            }
        }
        node = next;
        if node == first {
            break;
        }
    }
    inside
}

/// The ear clipper's stall recovery: removes ring nodes that duplicate their
/// next node or are collinear with both neighbors, so the retried scan walks
/// a simpler boundary. Returns the head to resume from, or `None` when
/// nothing was removable.
fn filter_ring_nodes(nodes: &mut [EarClipNode], ring_head: usize) -> Option<usize> {
    let mut removed_any = false;
    let mut head = ring_head;
    loop {
        // Snapshot the current cycle so removal never invalidates the walk.
        let mut cycle_nodes = Vec::new();
        let mut node = head;
        loop {
            cycle_nodes.push(node);
            node = nodes[node].next_index;
            if node == head {
                break;
            }
        }
        if cycle_nodes.len() <= 3 {
            break;
        }
        let mut removable_node = None;
        for &candidate in &cycle_nodes {
            let previous_vertex = nodes[nodes[candidate].previous_index].vertex;
            let vertex = nodes[candidate].vertex;
            let next_vertex = nodes[nodes[candidate].next_index].vertex;
            let is_duplicate = vertex == next_vertex;
            let longer_edge_squared = (vertex[0] - previous_vertex[0])
                .hypot(vertex[1] - previous_vertex[1])
                .max((next_vertex[0] - vertex[0]).hypot(next_vertex[1] - vertex[1]))
                .powi(2);
            let is_collinear = triangle_doubled_signed_area(previous_vertex, vertex, next_vertex)
                .abs()
                <= EAR_CLIPPING_COLLINEAR_RELATIVE_TOLERANCE * longer_edge_squared;
            if is_duplicate || is_collinear {
                removable_node = Some(candidate);
                break;
            }
        }
        let Some(candidate) = removable_node else {
            break;
        };
        remove_ear_node(nodes, candidate);
        removed_any = true;
        if candidate == head {
            head = nodes[candidate].next_index;
        }
    }
    removed_any.then_some(head)
}

/// The ear clipper's second stall recovery (`earcut`'s
/// `cureLocalIntersections`): when two adjacent boundary edges cross, the
/// small triangle they form is emitted directly and the two crossing nodes
/// are removed, untangling a locally self-intersecting boundary (common in
/// real tile data where a hole touches its exterior at a vertex). Returns the
/// head to resume clipping from, after a final collinearity filter.
fn cure_local_intersections(
    nodes: &mut [EarClipNode],
    start: usize,
    triangles: &mut Vec<[[f64; 2]; 3]>,
) -> usize {
    let mut node = start;
    let mut head = start;
    loop {
        let previous_index = nodes[node].previous_index;
        let next_index = nodes[node].next_index;
        let after_next_index = nodes[next_index].next_index;
        if nodes[previous_index].vertex != nodes[after_next_index].vertex
            && segments_intersect(
                nodes[previous_index].vertex,
                nodes[node].vertex,
                nodes[next_index].vertex,
                nodes[after_next_index].vertex,
            )
            && node_locally_inside_node(nodes, previous_index, after_next_index)
            && node_locally_inside_node(nodes, after_next_index, previous_index)
        {
            triangles.push([
                nodes[previous_index].vertex,
                nodes[node].vertex,
                nodes[next_index].vertex,
            ]);
            remove_ear_node(nodes, node);
            remove_ear_node(nodes, next_index);
            node = after_next_index;
            head = after_next_index;
        }
        node = nodes[node].next_index;
        if node == head {
            break;
        }
    }
    filter_ring_nodes(nodes, node).unwrap_or(node)
}

/// The sign of a value as `-1`, `0` or `1` (a cross product's orientation
/// sign in the segment-intersection test).
fn sign_of(value: f64) -> i32 {
    if value > 0.0 {
        1
    } else if value < 0.0 {
        -1
    } else {
        0
    }
}

/// Whether a point lies within a segment's bounding box: the collinear case
/// of [`segments_intersect`].
fn point_lies_on_segment(point: [f64; 2], segment_start: [f64; 2], segment_end: [f64; 2]) -> bool {
    point[0] <= segment_start[0].max(segment_end[0])
        && point[0] >= segment_start[0].min(segment_end[0])
        && point[1] <= segment_start[1].max(segment_end[1])
        && point[1] >= segment_start[1].min(segment_end[1])
}

/// Whether two closed segments intersect, including touching and collinear
/// overlaps (the `earcut` `intersects` test the local-intersection cure
/// relies on).
fn segments_intersect(
    first_start: [f64; 2],
    first_end: [f64; 2],
    second_start: [f64; 2],
    second_end: [f64; 2],
) -> bool {
    let first_orientation = sign_of(triangle_doubled_signed_area(
        first_start,
        first_end,
        second_start,
    ));
    let second_orientation = sign_of(triangle_doubled_signed_area(
        first_start,
        first_end,
        second_end,
    ));
    let third_orientation = sign_of(triangle_doubled_signed_area(
        second_start,
        second_end,
        first_start,
    ));
    let fourth_orientation = sign_of(triangle_doubled_signed_area(
        second_start,
        second_end,
        first_end,
    ));
    if first_orientation != second_orientation && third_orientation != fourth_orientation {
        return true;
    }
    (first_orientation == 0 && point_lies_on_segment(second_start, first_start, first_end))
        || (second_orientation == 0 && point_lies_on_segment(second_end, first_start, first_end))
        || (third_orientation == 0 && point_lies_on_segment(first_start, second_start, second_end))
        || (fourth_orientation == 0 && point_lies_on_segment(first_end, second_start, second_end))
}

/// The previous, middle, and next remaining indices around a candidate ear
/// position, wrapping the current ring order.
fn ear_corner_indices(remaining_indices: &[usize], position: usize) -> [usize; 3] {
    [
        remaining_indices[(position + remaining_indices.len() - 1) % remaining_indices.len()],
        remaining_indices[position],
        remaining_indices[(position + 1) % remaining_indices.len()],
    ]
}

/// Whether the vertex at `position` of the remaining ring is a valid ear:
/// convex (positive triangle area in the normalized winding) and with no
/// other remaining vertex inside or on the candidate triangle.
fn is_ring_ear(ring: &[[f64; 2]], remaining_indices: &[usize], position: usize) -> bool {
    let corner_indices = ear_corner_indices(remaining_indices, position);
    let [previous_vertex, middle_vertex, next_vertex] = [
        ring[corner_indices[0]],
        ring[corner_indices[1]],
        ring[corner_indices[2]],
    ];
    if triangle_doubled_signed_area(previous_vertex, middle_vertex, next_vertex) <= 0.0 {
        return false;
    }
    remaining_indices
        .iter()
        .filter(|&&vertex_index| !corner_indices.contains(&vertex_index))
        .all(|&vertex_index| {
            !point_lies_within_triangle(
                ring[vertex_index],
                previous_vertex,
                middle_vertex,
                next_vertex,
            )
        })
}

#[cfg(test)]
mod tests {
    use super::{
        ClipSpaceOverlayVertex, DEFAULT_POINT_MARKER_RADIUS_PIXELS, DEFAULT_POINT_MARKER_RGBA,
        DEFAULT_POINT_PIN_RADIUS_PIXELS, DEFAULT_ROUTE_LINE_RGBA, DEFAULT_ROUTE_LINE_WIDTH_PIXELS,
        DEFAULT_TRAFFIC_LINE_WIDTH_PIXELS, FilledPolygonOverlay, MapOverlay,
        OverlayFrameProjection, PointMarkersOverlay, PointPinsOverlay, PolylineStroke,
        RoutePolylineOverlay, TrafficFlowOverlay, WORLD_SPACE_OVERLAY_WORLD_COPY_OFFSETS,
        WorldSpaceOverlayFrameUniforms, WorldSpaceOverlayGeometry, WorldSpaceOverlayVertex,
        clip_position_to_screen_pixels, ear_clip_triangulation, ear_clip_triangulation_with_holes,
        overlay_triangle_vertices, overlay_triangle_vertices_for_layers,
        overlays_require_camera_space_strokes, segment_window_fragments,
        simplify_overlay_layers_for_style_zoom, traffic_congestion_rgba,
        translate_pixels_for_anchor,
    };
    use crate::camera::{DEFAULT_FIELD_OF_VIEW_DEGREES, MapCamera, ViewportPixels};
    use crate::projection::{normalized_world_to_coordinate, zoom_scale_pixels};
    use geospatial_types::{
        GeographicCoordinate, ManeuverType, RouteData, RouteLeg, RouteStep,
        TrafficFlowCongestionLevel, TrafficFlowData,
    };
    use glam::DVec4;
    use rustic_maps_style::{CirclePitchAlignment, LineCap, LineJoin, TranslateAnchor};

    /// Golden-scene tolerance of the exact screen-space expectations: the
    /// overlay math runs on the same `f64` matrix path as the camera
    /// projection, so drift stays sub-micropixel.
    const SCREEN_PIXEL_TOLERANCE: f64 = 1e-6;
    /// Area tolerance of the triangulation coverage assertions, in doubled
    /// area units of the test rings (integer-valued areas).
    const TRIANGULATION_AREA_TOLERANCE: f64 = 1e-9;

    fn coordinate(latitude_degrees: f64, longitude_degrees: f64) -> GeographicCoordinate {
        GeographicCoordinate::from_degrees(latitude_degrees, longitude_degrees)
            .expect("the test coordinates are valid")
    }

    fn square_viewport() -> ViewportPixels {
        ViewportPixels::new(512.0, 512.0).unwrap()
    }

    fn top_down_camera(normalized_world_center: [f64; 2], zoom_level: f64) -> MapCamera {
        MapCamera::new(
            normalized_world_to_coordinate(normalized_world_center),
            zoom_level,
            0.0,
            0.0,
        )
    }

    /// Divides an overlay vertex's clip position to viewport pixels, the
    /// screen convention the assembly itself targets.
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

    /// A horizontal route over the equator centered on the camera: two points
    /// symmetric in longitude about the center, so the segment runs exactly
    /// horizontally through the viewport center.
    fn centered_horizontal_route(half_span_degrees: f64) -> RoutePolylineOverlay {
        RoutePolylineOverlay::new(vec![
            coordinate(0.0, -half_span_degrees),
            coordinate(0.0, half_span_degrees),
        ])
    }

    #[test]
    fn thickening_offsets_perpendicular_in_screen_pixels() {
        // At zoom 10 over the equator, half a span of 0.02 degrees maps to a
        // screen pixel distance shared with the camera projection; the line
        // width is the default, so the offset corners sit exactly half the
        // width above and below the segment in screen pixels.
        let half_span_degrees = 0.02;
        let viewport = square_viewport();
        let camera = top_down_camera([0.5, 0.5], 10.0);
        let route_overlay = centered_horizontal_route(half_span_degrees);
        let vertices = overlay_triangle_vertices(
            &camera,
            &viewport,
            crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
            &[MapOverlay::RoutePolyline(route_overlay)],
        );
        assert_eq!(
            vertices.len(),
            6,
            "one segment fragment draws one quad split into two triangles"
        );
        let start_screen = camera
            .project_to_screen(&coordinate(0.0, -half_span_degrees), &viewport, 60.0)
            .expect("the camera projects its own scene point");
        let end_screen = camera
            .project_to_screen(&coordinate(0.0, half_span_degrees), &viewport, 60.0)
            .expect("the camera projects its own scene point");
        let half_width = DEFAULT_ROUTE_LINE_WIDTH_PIXELS * 0.5;
        // Corner construction order of a thickened quad: start-offset (below,
        // the +perpendicular side), start-opposite, end-offset, end-opposite.
        // The two-triangle split covers each start corner once and each end
        // corner twice, which pins the quad's diagonal.
        let expected_corners = [
            [start_screen[0], start_screen[1] + half_width],
            [start_screen[0], start_screen[1] - half_width],
            [end_screen[0], end_screen[1] + half_width],
            [end_screen[0], end_screen[1] - half_width],
        ];
        let expected_appearances = [1, 2, 2, 1];
        for (corner, expected_appearance_count) in expected_corners.iter().zip(expected_appearances)
        {
            // The horizontal segment's perpendicular is vertical: all four
            // quad corners share their endpoint's screen x exactly.
            let matching_count = vertices
                .iter()
                .filter(|vertex| {
                    let screen_pixels = vertex_screen_pixels(vertex, &viewport);
                    (screen_pixels[0] - corner[0]).abs() < SCREEN_PIXEL_TOLERANCE
                        && (screen_pixels[1] - corner[1]).abs() < SCREEN_PIXEL_TOLERANCE
                })
                .count();
            assert_eq!(
                matching_count, expected_appearance_count,
                "corner {corner:?} appearance count: {vertices:?}"
            );
        }
        for vertex in &vertices {
            assert_eq!(vertex.rgba_color, DEFAULT_ROUTE_LINE_RGBA);
            assert!(
                vertex
                    .clip_position
                    .iter()
                    .all(|component| component.is_finite())
            );
        }
    }

    /// A bent route over the equator, carrying a styled stroke.
    fn bent_styled_route(stroke: PolylineStroke) -> MapOverlay {
        let mut route = RoutePolylineOverlay::new(vec![
            coordinate(0.0, -0.02),
            coordinate(0.0, 0.0),
            coordinate(0.02, 0.02),
        ]);
        route.stroke = Some(stroke);
        MapOverlay::RoutePolyline(route)
    }

    /// A horizontal route over the equator, carrying a styled stroke.
    fn horizontal_styled_route(stroke: PolylineStroke) -> MapOverlay {
        let mut route =
            RoutePolylineOverlay::new(vec![coordinate(0.0, -0.02), coordinate(0.0, 0.02)]);
        route.stroke = Some(stroke);
        MapOverlay::RoutePolyline(route)
    }

    /// A stroke with every optional presentation at its neutral value.
    fn plain_stroke(join: LineJoin, cap: LineCap) -> PolylineStroke {
        PolylineStroke {
            cap,
            join,
            dash_array: Vec::new(),
            offset_pixels: 0.0,
            gap_width_pixels: 0.0,
            blur_pixels: 0.0,
        }
    }

    /// The overlay's assembled vertex count at the shared test camera.
    fn styled_vertex_count(overlay: MapOverlay) -> usize {
        let viewport = square_viewport();
        let camera = top_down_camera([0.5, 0.5], 10.0);
        overlay_triangle_vertices(
            &camera,
            &viewport,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
            &[overlay],
        )
        .len()
    }

    /// The overlay's assembled vertices in screen pixels at the shared camera.
    fn styled_screen_positions(overlay: MapOverlay) -> Vec<[f64; 2]> {
        let viewport = square_viewport();
        let camera = top_down_camera([0.5, 0.5], 10.0);
        overlay_triangle_vertices(
            &camera,
            &viewport,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
            &[overlay],
        )
        .iter()
        .map(|vertex| vertex_screen_pixels(vertex, &viewport))
        .collect()
    }

    #[test]
    fn styled_joins_grow_from_bevel_through_miter_to_a_round_fan() {
        let bevel = styled_vertex_count(bent_styled_route(plain_stroke(
            LineJoin::Bevel,
            LineCap::Butt,
        )));
        let miter = styled_vertex_count(bent_styled_route(plain_stroke(
            LineJoin::Miter,
            LineCap::Butt,
        )));
        let round = styled_vertex_count(bent_styled_route(plain_stroke(
            LineJoin::Round,
            LineCap::Butt,
        )));
        assert!(bevel > 0);
        assert!(miter > bevel, "miter {miter} bevel {bevel}");
        assert!(round > miter, "round {round} miter {miter}");
    }

    #[test]
    fn styled_caps_add_square_quads_or_round_fans_at_both_ends() {
        let butt = styled_vertex_count(horizontal_styled_route(plain_stroke(
            LineJoin::Miter,
            LineCap::Butt,
        )));
        let square = styled_vertex_count(horizontal_styled_route(plain_stroke(
            LineJoin::Miter,
            LineCap::Square,
        )));
        let round = styled_vertex_count(horizontal_styled_route(plain_stroke(
            LineJoin::Miter,
            LineCap::Round,
        )));
        assert_eq!(butt, 6, "one segment draws one quad");
        assert_eq!(square, 6 + 2 * 6, "each square cap adds one quad");
        assert_eq!(
            round,
            6 + 2 * 8 * 3,
            "each round cap adds an eight-triangle fan"
        );
    }

    #[test]
    fn a_dash_pattern_splits_the_stroke_into_runs() {
        let solid = styled_vertex_count(horizontal_styled_route(plain_stroke(
            LineJoin::Miter,
            LineCap::Butt,
        )));
        let mut dashed_stroke = plain_stroke(LineJoin::Miter, LineCap::Butt);
        dashed_stroke.dash_array = vec![2.0, 2.0];
        let dashed = styled_vertex_count(horizontal_styled_route(dashed_stroke));
        assert_eq!(solid, 6);
        assert!(dashed > solid, "dashed {dashed} solid {solid}");
        assert_eq!(dashed % 6, 0, "every dash run is a quad");
    }

    #[test]
    fn a_line_offset_shifts_the_stroke_perpendicularly() {
        let solid_positions = styled_screen_positions(horizontal_styled_route(plain_stroke(
            LineJoin::Miter,
            LineCap::Butt,
        )));
        let mut offset_stroke = plain_stroke(LineJoin::Miter, LineCap::Butt);
        offset_stroke.offset_pixels = 5.0;
        let offset_positions = styled_screen_positions(horizontal_styled_route(offset_stroke));
        let mean_y = |positions: &[[f64; 2]]| {
            positions.iter().map(|position| position[1]).sum::<f64>() / positions.len() as f64
        };
        let shift = mean_y(&offset_positions) - mean_y(&solid_positions);
        assert!(
            (shift - 5.0).abs() < SCREEN_PIXEL_TOLERANCE,
            "the stroke shifted {shift} pixels instead of five"
        );
    }

    #[test]
    fn a_gap_width_paints_two_bands_outside_the_gap() {
        let solid = styled_vertex_count(horizontal_styled_route(plain_stroke(
            LineJoin::Miter,
            LineCap::Butt,
        )));
        let mut casing_stroke = plain_stroke(LineJoin::Miter, LineCap::Butt);
        casing_stroke.gap_width_pixels = 8.0;
        let casing = styled_vertex_count(horizontal_styled_route(casing_stroke));
        assert_eq!(casing, solid * 2, "a casing paints one band per side");
    }

    #[test]
    fn a_blurred_stroke_paints_a_fringe_under_the_core() {
        let solid = styled_vertex_count(horizontal_styled_route(plain_stroke(
            LineJoin::Miter,
            LineCap::Butt,
        )));
        let mut blurred_stroke = plain_stroke(LineJoin::Miter, LineCap::Butt);
        blurred_stroke.blur_pixels = 1.0;
        let blurred = styled_vertex_count(horizontal_styled_route(blurred_stroke));
        assert!(blurred > solid, "blurred {blurred} solid {solid}");
    }

    #[test]
    fn styled_overlays_require_the_camera_space_assembly() {
        assert!(overlays_require_camera_space_strokes(&[
            horizontal_styled_route(plain_stroke(LineJoin::Miter, LineCap::Butt),)
        ]));
        assert!(!overlays_require_camera_space_strokes(&[
            MapOverlay::route_polyline(vec![coordinate(0.0, 0.0), coordinate(0.0, 0.1)])
        ]));
    }

    /// A square ring around the equator used by the fill tests.
    fn fill_test_ring() -> Vec<GeographicCoordinate> {
        vec![
            coordinate(-0.01, -0.01),
            coordinate(-0.01, 0.01),
            coordinate(0.01, 0.01),
            coordinate(0.01, -0.01),
        ]
    }

    /// The frame's overlay vertices in screen pixels at the shared camera.
    fn overlay_screen_positions(
        camera: &MapCamera,
        viewport: &ViewportPixels,
        overlay: MapOverlay,
    ) -> Vec<[f64; 2]> {
        overlay_triangle_vertices(camera, viewport, DEFAULT_FIELD_OF_VIEW_DEGREES, &[overlay])
            .iter()
            .map(|vertex| vertex_screen_pixels(vertex, viewport))
            .collect()
    }

    #[test]
    fn a_fill_outline_paints_the_ring_boundary_when_antialiased() {
        let viewport = square_viewport();
        let camera = top_down_camera([0.5, 0.5], 10.0);
        let plain = MapOverlay::FilledPolygon(FilledPolygonOverlay::new(fill_test_ring()));
        let mut outlined_fill = FilledPolygonOverlay::new(fill_test_ring());
        outlined_fill.rgba_outline_color = Some([1.0, 1.0, 1.0, 1.0]);
        let plain_count = overlay_screen_positions(&camera, &viewport, plain).len();
        let outlined_count = overlay_screen_positions(
            &camera,
            &viewport,
            MapOverlay::FilledPolygon(outlined_fill.clone()),
        )
        .len();
        assert!(
            outlined_count > plain_count,
            "outline {outlined_count} plain {plain_count}"
        );
        outlined_fill.antialias = false;
        assert_eq!(
            overlay_screen_positions(&camera, &viewport, MapOverlay::FilledPolygon(outlined_fill))
                .len(),
            plain_count,
            "a disabled antialias paints no outline"
        );
    }

    #[test]
    fn a_fill_translate_shifts_every_corner_in_screen_pixels() {
        let viewport = square_viewport();
        let camera = top_down_camera([0.5, 0.5], 10.0);
        let mean_position = |positions: &[[f64; 2]]| {
            [
                positions.iter().map(|position| position[0]).sum::<f64>() / positions.len() as f64,
                positions.iter().map(|position| position[1]).sum::<f64>() / positions.len() as f64,
            ]
        };
        let plain_positions = overlay_screen_positions(
            &camera,
            &viewport,
            MapOverlay::FilledPolygon(FilledPolygonOverlay::new(fill_test_ring())),
        );
        let mut translated_fill = FilledPolygonOverlay::new(fill_test_ring());
        translated_fill.translate_pixels = [8.0, -4.0];
        let translated_positions = overlay_screen_positions(
            &camera,
            &viewport,
            MapOverlay::FilledPolygon(translated_fill),
        );
        let plain_mean = mean_position(&plain_positions);
        let translated_mean = mean_position(&translated_positions);
        assert!((translated_mean[0] - plain_mean[0] - 8.0).abs() < SCREEN_PIXEL_TOLERANCE);
        assert!((translated_mean[1] - plain_mean[1] + 4.0).abs() < SCREEN_PIXEL_TOLERANCE);
    }

    #[test]
    fn a_map_anchored_translate_rotates_with_the_camera_bearing() {
        // At north-up the two anchors agree.
        assert_eq!(
            translate_pixels_for_anchor([0.0, -10.0], TranslateAnchor::Map, 0.0),
            [0.0, -10.0]
        );
        // A viewport-anchored displacement never rotates.
        assert_eq!(
            translate_pixels_for_anchor([3.0, 4.0], TranslateAnchor::Viewport, 90.0),
            [3.0, 4.0]
        );
        // Facing east, a north displacement points left on screen.
        let rotated = translate_pixels_for_anchor([0.0, -10.0], TranslateAnchor::Map, 90.0);
        assert!((rotated[0] + 10.0).abs() < SCREEN_PIXEL_TOLERANCE);
        assert!(rotated[1].abs() < SCREEN_PIXEL_TOLERANCE);
    }

    #[test]
    fn a_map_anchored_fill_translate_follows_the_camera_bearing_in_assembly() {
        let viewport = square_viewport();
        let camera = MapCamera::new(normalized_world_to_coordinate([0.5, 0.5]), 10.0, 90.0, 0.0);
        let mean_position = |positions: &[[f64; 2]]| {
            [
                positions.iter().map(|position| position[0]).sum::<f64>() / positions.len() as f64,
                positions.iter().map(|position| position[1]).sum::<f64>() / positions.len() as f64,
            ]
        };
        let plain_positions = overlay_screen_positions(
            &camera,
            &viewport,
            MapOverlay::FilledPolygon(FilledPolygonOverlay::new(fill_test_ring())),
        );
        let plain_mean = mean_position(&plain_positions);
        let mut viewport_anchored = FilledPolygonOverlay::new(fill_test_ring());
        viewport_anchored.translate_pixels = [0.0, -10.0];
        viewport_anchored.translate_anchor = TranslateAnchor::Viewport;
        let viewport_positions = overlay_screen_positions(
            &camera,
            &viewport,
            MapOverlay::FilledPolygon(viewport_anchored),
        );
        let viewport_mean = mean_position(&viewport_positions);
        assert!((viewport_mean[0] - plain_mean[0]).abs() < SCREEN_PIXEL_TOLERANCE);
        assert!((viewport_mean[1] - plain_mean[1] + 10.0).abs() < SCREEN_PIXEL_TOLERANCE);
        let mut map_anchored = FilledPolygonOverlay::new(fill_test_ring());
        map_anchored.translate_pixels = [0.0, -10.0];
        map_anchored.translate_anchor = TranslateAnchor::Map;
        let map_positions =
            overlay_screen_positions(&camera, &viewport, MapOverlay::FilledPolygon(map_anchored));
        let map_mean = mean_position(&map_positions);
        assert!((map_mean[0] - plain_mean[0] + 10.0).abs() < SCREEN_PIXEL_TOLERANCE);
        assert!((map_mean[1] - plain_mean[1]).abs() < SCREEN_PIXEL_TOLERANCE);
    }

    /// How far the furthest marker corner sits from the viewport centre.
    fn marker_extent_pixels(positions: &[[f64; 2]], viewport: &ViewportPixels) -> f64 {
        let center = [viewport.width_pixels * 0.5, viewport.height_pixels * 0.5];
        positions
            .iter()
            .map(|position| (position[0] - center[0]).hypot(position[1] - center[1]))
            .fold(0.0, f64::max)
    }

    #[test]
    fn a_circle_stroke_paints_an_annulus_outside_the_radius() {
        let viewport = square_viewport();
        let camera = top_down_camera([0.5, 0.5], 10.0);
        let mut plain_markers = PointMarkersOverlay::new(vec![coordinate(0.0, 0.0)]);
        plain_markers.radius_pixels = 10.0;
        let mut stroked_markers = plain_markers.clone();
        stroked_markers.rgba_stroke_color = Some([1.0, 1.0, 1.0, 1.0]);
        stroked_markers.stroke_width_pixels = 4.0;
        let plain_extent = marker_extent_pixels(
            &overlay_screen_positions(&camera, &viewport, MapOverlay::PointMarkers(plain_markers)),
            &viewport,
        );
        let stroked_extent = marker_extent_pixels(
            &overlay_screen_positions(
                &camera,
                &viewport,
                MapOverlay::PointMarkers(stroked_markers),
            ),
            &viewport,
        );
        assert!((plain_extent - 10.0).abs() < SCREEN_PIXEL_TOLERANCE);
        assert!((stroked_extent - 14.0).abs() < SCREEN_PIXEL_TOLERANCE);
    }

    #[test]
    fn a_circle_blur_paints_a_wider_fringe() {
        let viewport = square_viewport();
        let camera = top_down_camera([0.5, 0.5], 10.0);
        let mut blurred_markers = PointMarkersOverlay::new(vec![coordinate(0.0, 0.0)]);
        blurred_markers.radius_pixels = 10.0;
        blurred_markers.blur_pixels = 3.0;
        let blurred_extent = marker_extent_pixels(
            &overlay_screen_positions(
                &camera,
                &viewport,
                MapOverlay::PointMarkers(blurred_markers),
            ),
            &viewport,
        );
        assert!((blurred_extent - 13.0).abs() < SCREEN_PIXEL_TOLERANCE);
    }

    #[test]
    fn a_map_aligned_marker_projects_an_ellipse_under_pitch() {
        let viewport = square_viewport();
        let camera = MapCamera::new(normalized_world_to_coordinate([0.5, 0.5]), 10.0, 0.0, 60.0);
        let mut markers = PointMarkersOverlay::new(vec![coordinate(0.0, 0.0)]);
        markers.radius_pixels = 40.0;
        markers.pitch_alignment = CirclePitchAlignment::Map;
        let positions = overlay_screen_positions(
            &camera,
            &viewport,
            MapOverlay::PointMarkers(markers.clone()),
        );
        let center = [viewport.width_pixels * 0.5, viewport.height_pixels * 0.5];
        let horizontal_extent = positions
            .iter()
            .map(|position| (position[0] - center[0]).abs())
            .fold(0.0, f64::max);
        let vertical_extent = positions
            .iter()
            .map(|position| (position[1] - center[1]).abs())
            .fold(0.0, f64::max);
        assert!(
            horizontal_extent > vertical_extent * 1.3,
            "horizontal {horizontal_extent} vertical {vertical_extent}"
        );
        assert!(overlays_require_camera_space_strokes(&[
            MapOverlay::PointMarkers(markers)
        ]));
    }

    #[test]
    fn markers_anchor_at_their_projected_center_with_the_default_radius() {
        // Center anchoring: every fan triangle's apex (the first vertex)
        // projects exactly onto the marker coordinate's screen position, and
        // every ring vertex sits one default radius away from it.
        let viewport = square_viewport();
        let camera = top_down_camera([0.5, 0.5], 10.0);
        let marker_position = coordinate(0.001, 0.001);
        let vertices = overlay_triangle_vertices(
            &camera,
            &viewport,
            crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
            &[MapOverlay::PointMarkers(PointMarkersOverlay::new(vec![
                marker_position,
            ]))],
        );
        assert_eq!(
            vertices.len(),
            8 * 3,
            "one marker draws one octagon fan of {sides} triangles",
            sides = 8
        );
        let expected_center = camera
            .project_to_screen(&marker_position, &viewport, 60.0)
            .expect("the camera projects its own scene point");
        for marker_triangle in vertices.as_chunks::<3>().0 {
            let apex_screen = vertex_screen_pixels(&marker_triangle[0], &viewport);
            assert!(
                (apex_screen[0] - expected_center[0]).abs() < SCREEN_PIXEL_TOLERANCE
                    && (apex_screen[1] - expected_center[1]).abs() < SCREEN_PIXEL_TOLERANCE,
                "the fan apex must anchor at the projected center: {apex_screen:?} vs \
                 {expected_center:?}"
            );
            for ring_vertex in &marker_triangle[1..] {
                let ring_screen = vertex_screen_pixels(ring_vertex, &viewport);
                let radius_pixels = (ring_screen[0] - expected_center[0])
                    .hypot(ring_screen[1] - expected_center[1]);
                assert!(
                    (radius_pixels - DEFAULT_POINT_MARKER_RADIUS_PIXELS).abs()
                        < SCREEN_PIXEL_TOLERANCE,
                    "the fan ring must keep the marker radius: {radius_pixels}"
                );
            }
            assert_eq!(
                ring_vertex_color(marker_triangle),
                DEFAULT_POINT_MARKER_RGBA
            );
        }
    }

    fn ring_vertex_color(marker_triangle: &[ClipSpaceOverlayVertex]) -> [f32; 4] {
        marker_triangle[1].rgba_color
    }

    #[test]
    fn a_convex_ring_triangulates_into_nonoverlapping_coverage() {
        let triangles = ear_clip_triangulation(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        assert_eq!(triangles.len(), 2, "a square ring yields two triangles");
        let mut doubled_area_sum = 0.0;
        for triangle in &triangles {
            let doubled_area =
                super::triangle_doubled_signed_area(triangle[0], triangle[1], triangle[2]);
            assert!(doubled_area > 0.0, "every ear keeps the normalized winding");
            doubled_area_sum += doubled_area;
        }
        assert!(
            (doubled_area_sum - 2.0).abs() < TRIANGULATION_AREA_TOLERANCE,
            "the triangles must cover the ring area exactly: {doubled_area_sum}"
        );
    }

    #[test]
    fn a_clockwise_ring_triangulates_like_a_counterclockwise_one() {
        let clockwise = ear_clip_triangulation(&[[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]]);
        let counterclockwise =
            ear_clip_triangulation(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        assert_eq!(
            clockwise.len(),
            counterclockwise.len(),
            "winding normalizes before clipping"
        );
        let clockwise_doubled_area: f64 = clockwise
            .iter()
            .map(|triangle| {
                super::triangle_doubled_signed_area(triangle[0], triangle[1], triangle[2])
            })
            .sum();
        assert!((clockwise_doubled_area - 2.0).abs() < TRIANGULATION_AREA_TOLERANCE);
    }

    #[test]
    fn a_non_convex_ring_triangulates_to_the_full_area() {
        // An L-shaped ring (one reflex vertex): five ears, and the triangles
        // must cover the non-convex area instead of its convex hull.
        let shape_ring = [
            [0.0, 0.0],
            [2.0, 0.0],
            [2.0, 1.0],
            [1.0, 1.0],
            [1.0, 2.0],
            [0.0, 2.0],
        ];
        let triangles = ear_clip_triangulation(&shape_ring);
        assert_eq!(triangles.len(), shape_ring.len() - 2);
        let doubled_area_sum: f64 = triangles
            .iter()
            .map(|triangle| {
                super::triangle_doubled_signed_area(triangle[0], triangle[1], triangle[2])
            })
            .sum();
        // The ring's doubled signed area is 6 (area 3), well below the convex
        // hull's 8 (area 4), so the sum pins the non-convex fill.
        assert!(
            (doubled_area_sum - 6.0).abs() < TRIANGULATION_AREA_TOLERANCE,
            "the triangles must cover the L-shape exactly: {doubled_area_sum}"
        );
    }

    #[test]
    fn a_closed_ring_tolerates_the_repeated_closing_vertex() {
        let triangles =
            ear_clip_triangulation(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]]);
        assert_eq!(triangles.len(), 2, "the repeated first vertex is dropped");
    }

    #[test]
    fn degenerate_rings_fill_nothing() {
        assert!(ear_clip_triangulation(&[[0.0, 0.0], [1.0, 0.0]]).is_empty());
        assert!(ear_clip_triangulation(&[[0.0, 0.0], [1.0, 0.0], [2.0, 0.0]]).is_empty());
        assert!(ear_clip_triangulation(&[]).is_empty());
    }

    #[test]
    fn a_ring_with_a_hole_fills_only_the_area_outside_the_hole() {
        let outer_ring = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        // The hole winds opposite the outer ring, the tile specification's
        // convention; the triangulator normalizes either way.
        let hole_ring = [[4.0, 4.0], [4.0, 6.0], [6.0, 6.0], [6.0, 4.0]];
        let triangles = ear_clip_triangulation_with_holes(&outer_ring, &[hole_ring.to_vec()]);
        let doubled_area_sum: f64 = triangles
            .iter()
            .map(|triangle| {
                super::triangle_doubled_signed_area(triangle[0], triangle[1], triangle[2])
            })
            .sum();
        assert!(
            (doubled_area_sum - (200.0 - 8.0)).abs() < TRIANGULATION_AREA_TOLERANCE,
            "the triangles must cover the square minus its hole: {doubled_area_sum}"
        );
        assert!(
            triangles
                .iter()
                .all(|triangle| !super::point_lies_within_triangle(
                    [5.0, 5.0],
                    triangle[0],
                    triangle[1],
                    triangle[2],
                )),
            "no triangle may cover the hole's interior"
        );
    }

    #[test]
    fn a_ring_without_holes_triangulates_exactly_like_the_hole_free_path() {
        let outer_ring = [
            [0.0, 0.0],
            [2.0, 0.0],
            [2.0, 1.0],
            [1.0, 1.0],
            [1.0, 2.0],
            [0.0, 2.0],
        ];
        assert_eq!(
            ear_clip_triangulation_with_holes(&outer_ring, &[]),
            ear_clip_triangulation(&outer_ring)
        );
    }

    #[test]
    fn a_filled_polygon_with_a_hole_draws_no_triangle_over_the_hole_center() {
        // A square centered on the camera with a centered square hole: the
        // projected hole center is the viewport center, and no assembled
        // triangle (after world-copy splitting) may cover it.
        let viewport = square_viewport();
        let camera = top_down_camera([0.5, 0.5], 10.0);
        let outer_ring = vec![
            normalized_world_to_coordinate([0.40, 0.40]),
            normalized_world_to_coordinate([0.60, 0.40]),
            normalized_world_to_coordinate([0.60, 0.60]),
            normalized_world_to_coordinate([0.40, 0.60]),
        ];
        let hole_ring = vec![
            normalized_world_to_coordinate([0.47, 0.47]),
            normalized_world_to_coordinate([0.47, 0.53]),
            normalized_world_to_coordinate([0.53, 0.53]),
            normalized_world_to_coordinate([0.53, 0.47]),
        ];
        let vertices = overlay_triangle_vertices(
            &camera,
            &viewport,
            crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
            &[MapOverlay::FilledPolygon(
                FilledPolygonOverlay::new(outer_ring).with_hole_rings(vec![hole_ring]),
            )],
        );
        assert!(
            !vertices.is_empty(),
            "the ring set still fills around the hole"
        );
        let hole_center_screen = [viewport.width_pixels * 0.5, viewport.height_pixels * 0.5];
        for triangle in vertices.as_chunks::<3>().0 {
            let screen_triangle = [
                vertex_screen_pixels(&triangle[0], &viewport),
                vertex_screen_pixels(&triangle[1], &viewport),
                vertex_screen_pixels(&triangle[2], &viewport),
            ];
            assert!(
                !screen_triangle_contains_point(screen_triangle, hole_center_screen),
                "a triangle covers the hole center: {screen_triangle:?}"
            );
        }
    }

    /// Whether a screen-space triangle contains a point (boundary inclusive).
    fn screen_triangle_contains_point(triangle: [[f64; 2]; 3], point: [f64; 2]) -> bool {
        let edge_side = |start: [f64; 2], end: [f64; 2]| {
            (end[0] - start[0]) * (point[1] - start[1])
                - (end[1] - start[1]) * (point[0] - start[0])
        };
        let sides = [
            edge_side(triangle[0], triangle[1]),
            edge_side(triangle[1], triangle[2]),
            edge_side(triangle[2], triangle[0]),
        ];
        !(sides.iter().any(|side| *side < 0.0) && sides.iter().any(|side| *side > 0.0))
    }

    #[test]
    fn a_segment_crossing_a_world_copy_window_splits_into_fragments() {
        // A camera over normalized x = 0.3 keeps its own world-copy window at
        // [-0.2, 0.8]: a segment from x = 0.7 to x = 0.9 splits at the window
        // edge into the camera-copy piece [0.7, 0.8] and the wrapped-copy
        // piece [0.8, 0.9], exactly like the tile spans' world-copy
        // fragments.
        let viewport = square_viewport();
        let camera = top_down_camera([0.3, 0.5], 10.0);
        let frame_projection = OverlayFrameProjection::for_frame(&camera, &viewport, 60.0);
        let fragments = segment_window_fragments(&frame_projection, [0.7, 0.5], [0.9, 0.5]);
        assert_eq!(fragments.len(), 2);
        assert_eq!(
            (
                fragments[0].start,
                fragments[0].end,
                fragments[0].window_offset
            ),
            ([0.7, 0.5], [0.8, 0.5], 0.0)
        );
        assert_eq!(
            (
                fragments[1].start,
                fragments[1].end,
                fragments[1].window_offset
            ),
            ([0.8, 0.5], [0.9, 0.5], 1.0)
        );
    }

    #[test]
    fn a_polygon_crossing_a_world_copy_window_fills_both_fragments() {
        // The same camera and window geometry as the segment split: a
        // triangle spanning the window edge fills one triangle per copy
        // window, so the fill continues across the copy seam like the tiles.
        let viewport = square_viewport();
        let camera = top_down_camera([0.3, 0.5], 10.0);
        let ring_coordinates = vec![
            normalized_world_to_coordinate([0.7, 0.49]),
            normalized_world_to_coordinate([0.9, 0.49]),
            normalized_world_to_coordinate([0.8, 0.51]),
        ];
        let vertices = overlay_triangle_vertices(
            &camera,
            &viewport,
            crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
            &[MapOverlay::FilledPolygon(FilledPolygonOverlay::new(
                ring_coordinates,
            ))],
        );
        assert_eq!(
            vertices.len(),
            6,
            "each world-copy fragment fans into one triangle: {vertices:?}"
        );
    }

    #[test]
    fn segments_behind_the_camera_draw_nothing() {
        // Pointed 40 degrees north from over the equator, ground points well
        // south of the eye lie behind the camera (both endpoints behind:
        // skipped; one endpoint behind: the whole segment skips, as
        // documented). The behind-camera wedge reaches from the eye's ground
        // point southward past the view cone, hence the generous latitudes.
        let viewport = square_viewport();
        let camera = MapCamera::new(coordinate(0.0, 0.0), 14.0, 0.0, 40.0);
        let behind_south_latitude = -0.03;
        let behind_south_east_latitude = -0.035;
        for latitude_degrees in [behind_south_latitude, behind_south_east_latitude] {
            let behind_coordinate = coordinate(latitude_degrees, 0.005);
            assert!(
                camera
                    .project_to_screen(&behind_coordinate, &viewport, 60.0)
                    .is_none(),
                "the test point {behind_coordinate:?} must sit behind the camera"
            );
        }
        let fully_behind = RoutePolylineOverlay::new(vec![
            coordinate(behind_south_latitude, -0.005),
            coordinate(behind_south_east_latitude, 0.005),
        ]);
        let crossing_behind = RoutePolylineOverlay::new(vec![
            coordinate(0.0, 0.0),
            coordinate(behind_south_latitude, 0.0),
        ]);
        for route_overlay in [fully_behind, crossing_behind] {
            let vertices = overlay_triangle_vertices(
                &camera,
                &viewport,
                crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
                &[MapOverlay::RoutePolyline(route_overlay)],
            );
            assert!(
                vertices.is_empty(),
                "segments with a behind-camera endpoint must skip: {vertices:?}"
            );
        }
    }

    #[test]
    fn an_empty_overlay_list_produces_no_vertices() {
        let viewport = square_viewport();
        let camera = top_down_camera([0.5, 0.5], 10.0);
        let vertices = overlay_triangle_vertices(
            &camera,
            &viewport,
            crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
            &[],
        );
        assert!(vertices.is_empty());
    }

    #[test]
    fn a_far_offscreen_route_keeps_finite_vertices() {
        // A route far outside the viewport projects to enormous but finite
        // clip positions; the GPU clip discards it, and nothing may poison
        // the other triangles of the frame.
        let viewport = square_viewport();
        let camera = top_down_camera([0.5, 0.5], 3.0);
        let far_away =
            RoutePolylineOverlay::new(vec![coordinate(80.0, 130.0), coordinate(-80.0, -130.0)]);
        let vertices = overlay_triangle_vertices(
            &camera,
            &viewport,
            crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
            &[MapOverlay::RoutePolyline(far_away)],
        );
        assert!(!vertices.is_empty(), "the far route still assembles");
        for vertex in vertices {
            assert!(
                vertex
                    .clip_position
                    .iter()
                    .all(|component| component.is_finite())
            );
        }
    }

    #[test]
    fn route_data_flattens_into_one_traversal_order_line() {
        let route_data = RouteData {
            distance_meters: 1000.0,
            duration_seconds: 60.0,
            bounding_box: geospatial_types::BoundingBox {
                south_west: coordinate(0.0, 0.0),
                north_east: coordinate(1.0, 1.0),
            },
            geometry: Vec::new(),
            legs: vec![RouteLeg {
                distance_meters: 1000.0,
                duration_seconds: 60.0,
                steps: vec![
                    RouteStep {
                        distance_meters: 500.0,
                        duration_seconds: 30.0,
                        maneuver_type: ManeuverType::Departure,
                        instruction_display_text: String::new(),
                        instruction_speech_text: String::new(),
                        shape: vec![coordinate(1.0, 1.0), coordinate(2.0, 2.0)],
                        lane_guidances: Vec::new(),
                    },
                    RouteStep {
                        distance_meters: 500.0,
                        duration_seconds: 30.0,
                        maneuver_type: ManeuverType::Arrival,
                        instruction_display_text: String::new(),
                        instruction_speech_text: String::new(),
                        shape: vec![coordinate(3.0, 3.0)],
                        lane_guidances: Vec::new(),
                    },
                ],
            }],
        };
        let route_overlay = RoutePolylineOverlay::from_route_data(&route_data);
        let flattened_longitudes: Vec<f64> = route_overlay
            .coordinates
            .iter()
            .map(|coordinate| coordinate.longitude)
            .collect();
        assert_eq!(flattened_longitudes, vec![1.0, 2.0, 3.0]);
        assert_eq!(route_overlay.rgba_color, DEFAULT_ROUTE_LINE_RGBA);
        assert_eq!(route_overlay.width_pixels, DEFAULT_ROUTE_LINE_WIDTH_PIXELS);
    }

    #[test]
    fn congestion_levels_map_to_named_traffic_colors() {
        assert_eq!(
            traffic_congestion_rgba(TrafficFlowCongestionLevel::FreeFlow),
            super::TRAFFIC_FREE_FLOW_RGBA
        );
        assert_eq!(
            traffic_congestion_rgba(TrafficFlowCongestionLevel::Slow),
            super::TRAFFIC_SLOW_RGBA
        );
        assert_eq!(
            traffic_congestion_rgba(TrafficFlowCongestionLevel::Congested),
            super::TRAFFIC_CONGESTED_RGBA
        );
        assert_eq!(
            traffic_congestion_rgba(TrafficFlowCongestionLevel::Blocked),
            super::TRAFFIC_BLOCKED_RGBA
        );
    }

    #[test]
    fn a_traffic_flow_draws_one_thickened_quad_per_segment_in_its_congestion_color() {
        let viewport = square_viewport();
        let camera = top_down_camera([0.5, 0.5], 10.0);
        let flow_shape = vec![coordinate(0.0, -0.02), coordinate(0.0, 0.02)];
        let vertices = overlay_triangle_vertices(
            &camera,
            &viewport,
            crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
            &[MapOverlay::TrafficFlow(TrafficFlowOverlay::new(vec![
                geospatial_types::TrafficFlowData::from_congestion_level(
                    flow_shape,
                    TrafficFlowCongestionLevel::Congested,
                ),
            ]))],
        );
        assert_eq!(vertices.len(), 6, "one flow segment draws one quad");
        for vertex in &vertices {
            assert_eq!(
                vertex.rgba_color,
                super::TRAFFIC_CONGESTED_RGBA,
                "the segment paints its congestion level"
            );
        }
    }

    #[test]
    fn a_point_pin_anchors_its_tip_and_keeps_its_head_center_hollow() {
        let viewport = square_viewport();
        let camera = top_down_camera([0.5, 0.5], 10.0);
        let anchor = coordinate(0.0, 0.0);
        let vertices = overlay_triangle_vertices(
            &camera,
            &viewport,
            crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
            &[MapOverlay::PointPins(PointPinsOverlay::new(vec![anchor]))],
        );
        assert_eq!(
            vertices.len(),
            super::POINT_PIN_RING_SIDES * 6,
            "one pin draws two triangles per ring side"
        );
        let anchor_screen = camera
            .project_to_screen(&anchor, &viewport, 60.0)
            .expect("the camera projects its own scene point");
        let head_center_screen = [
            anchor_screen[0],
            anchor_screen[1]
                - DEFAULT_POINT_PIN_RADIUS_PIXELS * super::POINT_PIN_HEIGHT_RADIUS_FACTOR,
        ];
        let hollow_radius_pixels =
            DEFAULT_POINT_PIN_RADIUS_PIXELS * super::POINT_PIN_HOLLOW_RADIUS_RATIO;
        // The tip is an outline vertex: the ring samples the downward angle.
        let tip_appearances = vertices
            .iter()
            .filter(|vertex| {
                let screen_pixels = vertex_screen_pixels(vertex, &viewport);
                (screen_pixels[0] - anchor_screen[0]).abs() < SCREEN_PIXEL_TOLERANCE
                    && (screen_pixels[1] - anchor_screen[1]).abs() < SCREEN_PIXEL_TOLERANCE
            })
            .count();
        assert!(
            tip_appearances >= 2,
            "the tip vertex joins both triangles of its ring side: {vertices:?}"
        );
        let covered = |screen_point: [f64; 2]| {
            vertices.as_chunks::<3>().0.iter().any(|triangle| {
                let corners =
                    [0, 1, 2].map(|corner| vertex_screen_pixels(&triangle[corner], &viewport));
                screen_point_in_triangle(screen_point, corners)
            })
        };
        assert!(
            !covered(head_center_screen),
            "the head center stays unpainted (the hollow)"
        );
        assert!(
            !covered([
                head_center_screen[0] + hollow_radius_pixels * 0.5,
                head_center_screen[1]
            ]),
            "a point inside the hollow radius stays unpainted"
        );
        assert!(
            covered([
                head_center_screen[0] + hollow_radius_pixels * 1.8,
                head_center_screen[1]
            ]),
            "a point between the hollow and the head outline paints"
        );
        assert!(
            !covered([
                head_center_screen[0] + DEFAULT_POINT_PIN_RADIUS_PIXELS * 1.25,
                head_center_screen[1]
            ]),
            "a point beyond the head outline stays unpainted"
        );
    }

    /// Point-in-triangle test on the display's pixel axes; orientation does
    /// not matter because the barycentric signs are compared as a set.
    fn screen_point_in_triangle(screen_point: [f64; 2], corners: [[f64; 2]; 3]) -> bool {
        let edge_signs: Vec<f64> = (0..3)
            .map(|edge_index| {
                let start = corners[edge_index];
                let end = corners[(edge_index + 1) % 3];
                (end[0] - start[0]) * (screen_point[1] - start[1])
                    - (end[1] - start[1]) * (screen_point[0] - start[0])
            })
            .collect();
        let has_negative = edge_signs
            .iter()
            .any(|sign| *sign < -SCREEN_PIXEL_TOLERANCE);
        let has_positive = edge_signs.iter().any(|sign| *sign > SCREEN_PIXEL_TOLERANCE);
        !(has_negative && has_positive)
    }

    #[test]
    fn the_screen_pixel_scale_matches_the_zoom() {
        // A guard on the frame projection sharing the camera's own scale
        // definition: no overlay-specific conversion may drift.
        let camera = top_down_camera([0.5, 0.5], 10.0);
        let viewport = square_viewport();
        let frame_projection = OverlayFrameProjection::for_frame(&camera, &viewport, 60.0);
        assert_eq!(frame_projection.zoom_scale_pixels, zoom_scale_pixels(10.0));
        let wrapped = frame_projection.wrapped_x(0.9);
        assert!(
            (wrapped - 0.9).abs() <= 0.5,
            "wrapped x stays within half a world of the camera center"
        );
    }

    /// Screen-pixel tolerance of the world-space parity assertions: the frame
    /// uniform crosses to the GPU as `f32`, while the CPU assembly keeps
    /// `f64` through its projection, so a sub-hundredth of a pixel is expected
    /// and a whole pixel would be a real drift.
    const WORLD_SPACE_SCREEN_TOLERANCE_PIXELS: f64 = 0.05;

    /// Reimplements the world-space overlay vertex shader in `f64` for the
    /// parity tests: the same mode dispatch and world-copy window discard,
    /// evaluated against the same frame uniform the GPU receives. Returns the
    /// final screen position, or `None` when the vertex's world copy is not
    /// the camera's own (the shader's fragment discard).
    fn world_space_vertex_screen(
        uniforms: &WorldSpaceOverlayFrameUniforms,
        vertex: &WorldSpaceOverlayVertex,
        copy_offset: f64,
    ) -> Option<[f64; 2]> {
        let origin_relative = [
            f64::from(uniforms.origin_relative_normalized[0]),
            f64::from(uniforms.origin_relative_normalized[1]),
        ];
        let window_relative_x =
            origin_relative[0] + f64::from(vertex.world_position[0]) - copy_offset;
        if !(-0.5..0.5).contains(&window_relative_x) {
            return None;
        }
        let world_to_clip = |position: [f32; 2]| {
            let relative = [
                origin_relative[0] + f64::from(position[0]) - copy_offset,
                origin_relative[1] - f64::from(position[1]),
            ];
            let mut clip_position = [0.0_f64; 4];
            for (row, component) in clip_position.iter_mut().enumerate() {
                *component = f64::from(uniforms.anchor_clip_position[row])
                    + f64::from(uniforms.relative_view_projection[0][row]) * relative[0]
                    + f64::from(uniforms.relative_view_projection[1][row]) * relative[1];
            }
            clip_position
        };
        let clip_to_screen = |clip_position: [f64; 4]| {
            let normalized_device = [
                clip_position[0] / clip_position[3],
                clip_position[1] / clip_position[3],
            ];
            [
                (normalized_device[0] + 1.0) * 0.5 * f64::from(uniforms.viewport_size_pixels[0]),
                (1.0 - (normalized_device[1] + 1.0) * 0.5)
                    * f64::from(uniforms.viewport_size_pixels[1]),
            ]
        };
        match vertex.shape_kind {
            0 => Some(clip_to_screen(world_to_clip(vertex.world_position))),
            1 => {
                let anchor_clip_position = world_to_clip(vertex.world_position);
                if anchor_clip_position[3] <= 0.0 {
                    return None;
                }
                let anchor_screen = clip_to_screen(anchor_clip_position);
                Some([
                    anchor_screen[0] + f64::from(vertex.shape_parameters[0]),
                    anchor_screen[1] + f64::from(vertex.shape_parameters[1]),
                ])
            }
            2 => {
                let self_clip_position = world_to_clip(vertex.world_position);
                let other_clip_position = world_to_clip(vertex.reference_position);
                if self_clip_position[3] <= 0.0 || other_clip_position[3] <= 0.0 {
                    return None;
                }
                let self_screen = clip_to_screen(self_clip_position);
                let other_screen = clip_to_screen(other_clip_position);
                let direction = [
                    (other_screen[0] - self_screen[0]) * f64::from(vertex.shape_parameters[0]),
                    (other_screen[1] - self_screen[1]) * f64::from(vertex.shape_parameters[0]),
                ];
                let direction_length = direction[0].hypot(direction[1]);
                if direction_length < super::MINIMUM_VISIBLE_SEGMENT_LENGTH_PIXELS {
                    return None;
                }
                let perpendicular = [
                    -direction[1] / direction_length,
                    direction[0] / direction_length,
                ];
                Some([
                    self_screen[0] + perpendicular[0] * f64::from(vertex.shape_parameters[1]),
                    self_screen[1] + perpendicular[1] * f64::from(vertex.shape_parameters[1]),
                ])
            }
            3 => {
                let previous_clip_position = world_to_clip(vertex.reference_position);
                let vertex_clip_position = world_to_clip(vertex.world_position);
                let next_clip_position = world_to_clip(vertex.next_position);
                if previous_clip_position[3] <= 0.0
                    || vertex_clip_position[3] <= 0.0
                    || next_clip_position[3] <= 0.0
                {
                    return None;
                }
                let previous_screen = clip_to_screen(previous_clip_position);
                let vertex_screen = clip_to_screen(vertex_clip_position);
                let next_screen = clip_to_screen(next_clip_position);
                let incoming = [
                    vertex_screen[0] - previous_screen[0],
                    vertex_screen[1] - previous_screen[1],
                ];
                let outgoing = [
                    next_screen[0] - vertex_screen[0],
                    next_screen[1] - vertex_screen[1],
                ];
                let incoming_length = incoming[0].hypot(incoming[1]);
                let outgoing_length = outgoing[0].hypot(outgoing[1]);
                if incoming_length < super::MINIMUM_VISIBLE_SEGMENT_LENGTH_PIXELS
                    || outgoing_length < super::MINIMUM_VISIBLE_SEGMENT_LENGTH_PIXELS
                {
                    return None;
                }
                let incoming_unit = [incoming[0] / incoming_length, incoming[1] / incoming_length];
                let outgoing_unit = [outgoing[0] / outgoing_length, outgoing[1] / outgoing_length];
                let direction_sum = [
                    incoming_unit[0] + outgoing_unit[0],
                    incoming_unit[1] + outgoing_unit[1],
                ];
                let direction_sum_length = direction_sum[0].hypot(direction_sum[1]);
                let bisector =
                    if direction_sum_length < super::MINIMUM_VISIBLE_SEGMENT_LENGTH_PIXELS {
                        [-incoming_unit[1], incoming_unit[0]]
                    } else {
                        [
                            direction_sum[0] / direction_sum_length,
                            direction_sum[1] / direction_sum_length,
                        ]
                    };
                let perpendicular = [-bisector[1], bisector[0]];
                Some([
                    vertex_screen[0]
                        + bisector[0] * f64::from(vertex.shape_parameters[0])
                        + perpendicular[0] * f64::from(vertex.shape_parameters[1]),
                    vertex_screen[1]
                        + bisector[1] * f64::from(vertex.shape_parameters[0])
                        + perpendicular[1] * f64::from(vertex.shape_parameters[1]),
                ])
            }
            _ => None,
        }
    }

    /// Asserts the cached world-space layer reproduces the clip-space
    /// assembly's screen geometry vertex by vertex (the shape kinds emit their
    /// triangles in the same order for a scene away from the antimeridian).
    fn assert_world_space_matches_clip_space(
        camera: &MapCamera,
        viewport: &ViewportPixels,
        overlays: &[MapOverlay],
    ) {
        let geometry = WorldSpaceOverlayGeometry::tessellate(overlays);
        let clip_space_vertices =
            overlay_triangle_vertices(camera, viewport, DEFAULT_FIELD_OF_VIEW_DEGREES, overlays);
        assert_eq!(
            geometry.vertex_count(),
            clip_space_vertices.len(),
            "the world-space layer must emit the same vertex count"
        );
        let uniforms = WorldSpaceOverlayFrameUniforms::for_frame(
            camera,
            viewport,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
            geometry.origin_normalized_world(),
        );
        for (vertex_index, (world_vertex, clip_space_vertex)) in geometry
            .vertices()
            .iter()
            .zip(clip_space_vertices.iter())
            .enumerate()
        {
            let expected_screen = vertex_screen_pixels(clip_space_vertex, viewport);
            let world_screen = WORLD_SPACE_OVERLAY_WORLD_COPY_OFFSETS
                .iter()
                .find_map(|copy_offset| {
                    world_space_vertex_screen(&uniforms, world_vertex, f64::from(*copy_offset))
                })
                .unwrap_or_else(|| {
                    panic!("world-space vertex {vertex_index} has no drawable world copy")
                });
            let drift =
                (world_screen[0] - expected_screen[0]).hypot(world_screen[1] - expected_screen[1]);
            assert!(
                drift <= WORLD_SPACE_SCREEN_TOLERANCE_PIXELS,
                "world-space vertex {vertex_index} drifts {drift} px from the clip-space \
                 assembly: {world_screen:?} vs {expected_screen:?}"
            );
        }
    }

    fn sacramento_camera() -> MapCamera {
        MapCamera::new(coordinate(38.5766, -121.4944), 14.0, 0.0, 0.0)
    }

    #[test]
    fn world_space_route_matches_the_clip_space_assembly() {
        let route = RoutePolylineOverlay::new(vec![
            coordinate(38.573483, -121.5023),
            coordinate(38.5766, -121.4944),
            coordinate(38.580987, -121.4858),
        ]);
        assert_world_space_matches_clip_space(
            &sacramento_camera(),
            &square_viewport(),
            &[MapOverlay::RoutePolyline(route)],
        );
    }

    #[test]
    fn world_space_markers_match_the_clip_space_assembly() {
        let markers = PointMarkersOverlay::new(vec![
            coordinate(38.574984, -121.4971),
            coordinate(38.5766, -121.4944),
            coordinate(38.578216, -121.4917),
        ]);
        assert_world_space_matches_clip_space(
            &sacramento_camera(),
            &square_viewport(),
            &[MapOverlay::PointMarkers(markers)],
        );
    }

    #[test]
    fn world_space_filled_polygon_matches_the_clip_space_assembly() {
        let polygon = FilledPolygonOverlay::new(vec![
            coordinate(38.573483, -121.4987),
            coordinate(38.573483, -121.4901),
            coordinate(38.579717, -121.4901),
            coordinate(38.579717, -121.4987),
        ]);
        assert_world_space_matches_clip_space(
            &sacramento_camera(),
            &square_viewport(),
            &[MapOverlay::FilledPolygon(polygon)],
        );
    }

    #[test]
    fn world_space_geometry_is_independent_of_the_camera_pose() {
        // The cached layer takes no camera and stores content-defined offsets;
        // two tessellations of the same list are identical, and the origin is
        // the first coordinate's normalized world position.
        let route = RoutePolylineOverlay::new(vec![
            coordinate(38.573483, -121.5023),
            coordinate(38.5766, -121.4944),
        ]);
        let overlays = [MapOverlay::RoutePolyline(route)];
        let first = WorldSpaceOverlayGeometry::tessellate(&overlays);
        let second = WorldSpaceOverlayGeometry::tessellate(&overlays);
        assert_eq!(first, second);
        let expected_origin =
            crate::projection::coordinate_to_normalized_world(&coordinate(38.573483, -121.5023));
        assert_eq!(first.origin_normalized_world(), expected_origin);
        assert_eq!(first.triangle_count(), 2);
    }

    #[test]
    fn world_space_antimeridian_route_draws_both_world_copies() {
        // The seam-crossing route unwrapped from longitude 100 east past 180
        // to -160 must appear at both screen edges, exactly as the CPU
        // assembly's window split places it. The cached layer stores the
        // continuous chain once; the frame's world-copy instances and the
        // shader's window discard supply the two recentered pieces.
        let camera = top_down_camera([0.5, 0.5], 0.0);
        let viewport = square_viewport();
        let route = RoutePolylineOverlay::new(vec![
            coordinate(20.0, 100.0),
            coordinate(20.0, 170.0),
            coordinate(20.0, -160.0),
        ]);
        let overlays = [MapOverlay::RoutePolyline(route)];
        let geometry = WorldSpaceOverlayGeometry::tessellate(&overlays);
        let clip_space_vertices =
            overlay_triangle_vertices(&camera, &viewport, DEFAULT_FIELD_OF_VIEW_DEGREES, &overlays);
        assert_eq!(
            geometry.triangle_count(),
            6,
            "the cached layer holds two segment quads and one square join"
        );
        assert!(
            clip_space_vertices.len() > geometry.vertex_count(),
            "the CPU assembly additionally split the seam-crossing segment"
        );
        let uniforms = WorldSpaceOverlayFrameUniforms::for_frame(
            &camera,
            &viewport,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
            geometry.origin_normalized_world(),
        );
        let mut left_edge_drawn = false;
        let mut right_edge_drawn = false;
        let mut selected_copies = Vec::new();
        for copy_offset in WORLD_SPACE_OVERLAY_WORLD_COPY_OFFSETS {
            let mut copy_drew = false;
            for world_vertex in geometry.vertices() {
                if let Some(screen) =
                    world_space_vertex_screen(&uniforms, world_vertex, f64::from(copy_offset))
                {
                    copy_drew = true;
                    left_edge_drawn |= screen[0] < viewport.width_pixels * 0.1;
                    right_edge_drawn |= screen[0] > viewport.width_pixels * 0.9;
                }
            }
            if copy_drew {
                selected_copies.push(copy_offset);
            }
        }
        assert_eq!(
            selected_copies,
            vec![0.0, 1.0],
            "the route's nearest world copy spans both sides of the seam"
        );
        assert!(
            left_edge_drawn && right_edge_drawn,
            "the antimeridian route must reach both screen edges"
        );
    }

    #[test]
    fn an_empty_world_space_layer_draws_nothing() {
        let geometry = WorldSpaceOverlayGeometry::tessellate(&[]);
        assert!(geometry.is_empty());
        assert_eq!(geometry.triangle_count(), 0);
        assert_eq!(geometry.origin_normalized_world(), [0.0, 0.0]);
    }

    #[test]
    fn scaling_to_a_device_pixel_ratio_multiplies_the_screen_space_sizes() {
        let device_pixel_ratio = 3.0;
        let route = MapOverlay::route_polyline(vec![coordinate(0.0, -0.01), coordinate(0.0, 0.01)]);
        let markers = MapOverlay::point_markers(vec![coordinate(0.0, 0.0)]);
        let pins = MapOverlay::point_pins(vec![coordinate(0.0, 0.0)]);
        let traffic = MapOverlay::traffic_flow(vec![TrafficFlowData::from_congestion_level(
            vec![coordinate(0.0, -0.01), coordinate(0.0, 0.01)],
            TrafficFlowCongestionLevel::FreeFlow,
        )]);
        let polygon = MapOverlay::filled_polygon(vec![
            coordinate(0.0, 0.0),
            coordinate(0.0, 0.01),
            coordinate(0.01, 0.0),
        ]);

        let scaled_route = route.scaled_to_device_pixel_ratio(device_pixel_ratio);
        let scaled_markers = markers.scaled_to_device_pixel_ratio(device_pixel_ratio);
        let scaled_pins = pins.scaled_to_device_pixel_ratio(device_pixel_ratio);
        let scaled_traffic = traffic.scaled_to_device_pixel_ratio(device_pixel_ratio);
        let scaled_polygon = polygon.scaled_to_device_pixel_ratio(device_pixel_ratio);

        match scaled_route {
            MapOverlay::RoutePolyline(route_polyline) => assert_eq!(
                route_polyline.width_pixels,
                DEFAULT_ROUTE_LINE_WIDTH_PIXELS * device_pixel_ratio
            ),
            other => panic!("the scaled route kept its kind, was {other:?}"),
        }
        match scaled_markers {
            MapOverlay::PointMarkers(markers) => assert_eq!(
                markers.radius_pixels,
                DEFAULT_POINT_MARKER_RADIUS_PIXELS * device_pixel_ratio
            ),
            other => panic!("the scaled markers kept their kind, was {other:?}"),
        }
        match scaled_pins {
            MapOverlay::PointPins(pins) => assert_eq!(
                pins.radius_pixels,
                DEFAULT_POINT_PIN_RADIUS_PIXELS * device_pixel_ratio
            ),
            other => panic!("the scaled pins kept their kind, was {other:?}"),
        }
        match scaled_traffic {
            MapOverlay::TrafficFlow(traffic_flow) => assert_eq!(
                traffic_flow.width_pixels,
                DEFAULT_TRAFFIC_LINE_WIDTH_PIXELS * device_pixel_ratio
            ),
            other => panic!("the scaled traffic layer kept its kind, was {other:?}"),
        }
        assert_eq!(
            scaled_polygon, polygon,
            "a filled polygon has no screen-space size to scale"
        );
        assert_eq!(
            route.scaled_to_device_pixel_ratio(1.0),
            route,
            "the identity ratio leaves the overlay unchanged"
        );
    }

    #[test]
    fn the_opacity_multiplier_scales_only_alpha() {
        let route = MapOverlay::route_polyline(vec![
            GeographicCoordinate::from_degrees(38.0, -121.0).unwrap(),
            GeographicCoordinate::from_degrees(38.1, -121.1).unwrap(),
        ]);
        let polygon = MapOverlay::FilledPolygon(FilledPolygonOverlay {
            rgba_color: [0.2, 0.4, 0.6, 0.8],
            rgba_outline_color: Some([0.1, 0.2, 0.3, 0.5]),
            ..FilledPolygonOverlay::new(vec![
                GeographicCoordinate::from_degrees(38.0, -121.0).unwrap(),
                GeographicCoordinate::from_degrees(38.1, -121.0).unwrap(),
                GeographicCoordinate::from_degrees(38.0, -121.1).unwrap(),
            ])
        });
        let markers = MapOverlay::PointMarkers(PointMarkersOverlay {
            rgba_color: [0.3, 0.3, 0.3, 0.9],
            rgba_stroke_color: Some([0.5, 0.5, 0.5, 1.0]),
            ..PointMarkersOverlay::new(vec![
                GeographicCoordinate::from_degrees(38.0, -121.0).unwrap(),
            ])
        });
        let pins = MapOverlay::point_pins(vec![
            GeographicCoordinate::from_degrees(38.0, -121.0).unwrap(),
        ]);
        for overlay in [&route, &polygon, &markers, &pins] {
            assert_eq!(
                overlay.with_opacity_multiplier(1.0),
                *overlay,
                "full opacity leaves the overlay unchanged"
            );
        }
        assert_eq!(
            route.with_opacity_multiplier(2.0),
            route,
            "an out-of-range opacity clamps to full"
        );
        match polygon.with_opacity_multiplier(0.5) {
            MapOverlay::FilledPolygon(faded_polygon) => {
                assert_eq!(faded_polygon.rgba_color, [0.2, 0.4, 0.6, 0.4]);
                assert_eq!(
                    faded_polygon.rgba_outline_color,
                    Some([0.1, 0.2, 0.3, 0.25])
                );
            }
            other => panic!("the faded polygon kept its kind, was {other:?}"),
        }
        match markers.with_opacity_multiplier(0.5) {
            MapOverlay::PointMarkers(faded_markers) => {
                assert_eq!(faded_markers.rgba_color, [0.3, 0.3, 0.3, 0.45]);
                assert_eq!(faded_markers.rgba_stroke_color, Some([0.5, 0.5, 0.5, 0.5]));
            }
            other => panic!("the faded markers kept their kind, was {other:?}"),
        }
        match route.with_opacity_multiplier(0.25) {
            MapOverlay::RoutePolyline(faded_route) => assert_eq!(
                faded_route.rgba_color,
                [
                    DEFAULT_ROUTE_LINE_RGBA[0],
                    DEFAULT_ROUTE_LINE_RGBA[1],
                    DEFAULT_ROUTE_LINE_RGBA[2],
                    DEFAULT_ROUTE_LINE_RGBA[3] * 0.25
                ]
            ),
            other => panic!("the faded route kept its kind, was {other:?}"),
        }
    }

    #[test]
    fn emission_scales_screen_sizes_without_a_pre_scaled_copy() {
        // Scaling during assembly must match scaling a copy first, so a frame
        // with a device-pixel-ratio surface never needs a pre-scaled list. The
        // styled route exercises the stroke's offset, casing gap and blur too.
        let camera = top_down_camera([0.5, 0.5], 12.0);
        let viewport = square_viewport();
        let mut styled_route =
            RoutePolylineOverlay::new(vec![coordinate(0.0, -0.01), coordinate(0.0, 0.01)]);
        styled_route.stroke = Some(PolylineStroke {
            cap: LineCap::Round,
            join: LineJoin::Miter,
            dash_array: vec![2.0, 1.0],
            offset_pixels: 3.0,
            gap_width_pixels: 4.0,
            blur_pixels: 1.5,
        });
        let overlays = vec![
            MapOverlay::route_polyline(vec![coordinate(0.0, -0.01), coordinate(0.0, 0.01)]),
            MapOverlay::RoutePolyline(styled_route),
            MapOverlay::point_markers(vec![coordinate(0.0, 0.0)]),
            MapOverlay::point_pins(vec![coordinate(0.0, 0.0)]),
            MapOverlay::traffic_flow(vec![TrafficFlowData::from_congestion_level(
                vec![coordinate(0.0, -0.01), coordinate(0.0, 0.01)],
                TrafficFlowCongestionLevel::Slow,
            )]),
        ];
        let device_pixel_ratio = 2.0;
        let emitted = overlay_triangle_vertices_for_layers(
            &camera,
            &viewport,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
            &[&overlays],
            device_pixel_ratio,
        );
        let pre_scaled: Vec<MapOverlay> = overlays
            .iter()
            .map(|overlay| overlay.scaled_to_device_pixel_ratio(device_pixel_ratio))
            .collect();
        let expected = overlay_triangle_vertices(
            &camera,
            &viewport,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
            &pre_scaled,
        );
        assert!(!emitted.is_empty(), "the scene draws triangles");
        assert_eq!(
            emitted.len(),
            expected.len(),
            "emission scaling emits the pre-scaled list's vertices"
        );
        for (emitted_vertex, expected_vertex) in emitted.iter().zip(&expected) {
            assert_eq!(emitted_vertex.clip_position, expected_vertex.clip_position);
            assert_eq!(emitted_vertex.rgba_color, expected_vertex.rgba_color);
        }
    }

    /// A dense circle ring around `center` in normalized-world units, closed
    /// by a repeated first point.
    fn dense_world_ring(
        center: [f64; 2],
        radius: f64,
        point_count: usize,
    ) -> Vec<GeographicCoordinate> {
        let mut coordinates = Vec::with_capacity(point_count + 1);
        for point_index in 0..=point_count {
            let angle = std::f64::consts::TAU * point_index as f64 / point_count as f64;
            coordinates.push(normalized_world_to_coordinate([
                center[0] + radius * angle.cos(),
                center[1] + radius * angle.sin(),
            ]));
        }
        coordinates
    }

    #[test]
    fn low_zoom_simplification_reduces_a_dense_ring_and_leaves_city_zoom_alone() {
        let coordinates = dense_world_ring([0.32, 0.42], 0.002, 256);
        let polygon = FilledPolygonOverlay::new(coordinates.clone());
        let mut low_zoom_layers = vec![vec![MapOverlay::FilledPolygon(polygon.clone())]];
        simplify_overlay_layers_for_style_zoom(&mut low_zoom_layers, 7.0);
        let MapOverlay::FilledPolygon(simplified_polygon) = &low_zoom_layers[0][0] else {
            panic!("the fill survives low-zoom simplification");
        };
        assert!(
            simplified_polygon.ring_coordinates.len() < coordinates.len() / 2,
            "a dense ring simplifies to a small fraction of its vertices: {} of {}",
            simplified_polygon.ring_coordinates.len(),
            coordinates.len()
        );
        assert!(
            simplified_polygon.ring_coordinates.len() >= 3,
            "the ring stays an area"
        );

        let mut city_zoom_layers = vec![vec![MapOverlay::FilledPolygon(polygon)]];
        simplify_overlay_layers_for_style_zoom(&mut city_zoom_layers, 9.0);
        let MapOverlay::FilledPolygon(untouched_polygon) = &city_zoom_layers[0][0] else {
            panic!("the fill survives at city zoom");
        };
        assert_eq!(
            untouched_polygon.ring_coordinates, coordinates,
            "the committed city-zoom goldens paint the exact style geometry"
        );
    }

    #[test]
    fn a_sub_pixel_feature_is_culled_at_low_zoom() {
        // At z7 one world unit spans 32 768 px, so this ring is about a third
        // of a pixel across: invisible, and culled rather than tessellated.
        let tiny_polygon = FilledPolygonOverlay::new(dense_world_ring([0.32, 0.42], 0.000_005, 16));
        let mut low_zoom_layers = vec![vec![MapOverlay::FilledPolygon(tiny_polygon.clone())]];
        simplify_overlay_layers_for_style_zoom(&mut low_zoom_layers, 7.0);
        assert!(
            low_zoom_layers[0].is_empty(),
            "a sub-pixel fill is dropped at low zoom"
        );

        let mut city_zoom_layers = vec![vec![MapOverlay::FilledPolygon(tiny_polygon)]];
        simplify_overlay_layers_for_style_zoom(&mut city_zoom_layers, 9.0);
        assert_eq!(
            city_zoom_layers[0].len(),
            1,
            "city zoom keeps the same feature"
        );
    }

    #[test]
    fn a_line_translate_displaces_both_stroke_paths_in_screen_space() {
        let camera = top_down_camera([0.5, 0.5], 10.0);
        let viewport = square_viewport();
        let translate_pixels = [12.0, -7.0];
        let styled_stroke = PolylineStroke {
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            dash_array: Vec::new(),
            offset_pixels: 0.0,
            gap_width_pixels: 0.0,
            blur_pixels: 0.0,
        };
        for stroke in [None, Some(styled_stroke)] {
            let mut untranslated = centered_horizontal_route(0.02);
            untranslated.stroke = stroke.clone();
            let mut translated = untranslated.clone();
            translated.translate_pixels = translate_pixels;
            translated.translate_anchor = TranslateAnchor::Viewport;
            let untranslated_vertices = overlay_triangle_vertices(
                &camera,
                &viewport,
                DEFAULT_FIELD_OF_VIEW_DEGREES,
                &[MapOverlay::RoutePolyline(untranslated)],
            );
            let translated_vertices = overlay_triangle_vertices(
                &camera,
                &viewport,
                DEFAULT_FIELD_OF_VIEW_DEGREES,
                &[MapOverlay::RoutePolyline(translated)],
            );
            assert!(!untranslated_vertices.is_empty(), "the route draws");
            assert_eq!(untranslated_vertices.len(), translated_vertices.len());
            for (untranslated_vertex, translated_vertex) in
                untranslated_vertices.iter().zip(&translated_vertices)
            {
                let untranslated_screen = vertex_screen_pixels(untranslated_vertex, &viewport);
                let translated_screen = vertex_screen_pixels(translated_vertex, &viewport);
                assert!(
                    (translated_screen[0] - untranslated_screen[0] - translate_pixels[0]).abs()
                        < 1e-6,
                    "line-translate displaces x: {untranslated_screen:?} -> {translated_screen:?}"
                );
                assert!(
                    (translated_screen[1] - untranslated_screen[1] - translate_pixels[1]).abs()
                        < 1e-6,
                    "line-translate displaces y: {untranslated_screen:?} -> {translated_screen:?}"
                );
            }
        }
    }
}
