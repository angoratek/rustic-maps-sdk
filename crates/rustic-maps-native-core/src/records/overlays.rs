//! Map overlay descriptors, the surface handle and the location indicator.

#[cfg(not(target_arch = "wasm32"))]
use crate::error::SdkError;

use super::{GeographicCoordinate, TrafficFlowData};

/// Lowest valid overlay color channel (a channel is also refused when NaN,
/// outside the closed range by construction). Native-only with the surface
/// view's overlay converter.
#[cfg(not(target_arch = "wasm32"))]
const OVERLAY_MINIMUM_COLOR_CHANNEL: f64 = 0.0;
/// Highest valid overlay color channel. Native-only with the surface view's
/// overlay converter.
#[cfg(not(target_arch = "wasm32"))]
const OVERLAY_MAXIMUM_COLOR_CHANNEL: f64 = 1.0;

/// Solid RGBA overlay style, every channel in `[0, 1]`; the rustic-maps-rendering
/// pipelines consume `f32` channels, converted in the constructor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverlayColor {
    /// Red channel in `[0, 1]`.
    pub red: f64,
    /// Green channel in `[0, 1]`.
    pub green: f64,
    /// Blue channel in `[0, 1]`.
    pub blue: f64,
    /// Alpha channel in `[0, 1]` (the overlay pipeline alpha-blends).
    pub alpha: f64,
}

impl OverlayColor {
    /// Builds the native `f32` RGBA style array from the mirror record,
    /// refusing any channel outside the documented range. Native-only with
    /// the surface view's overlay converter.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn to_native_rgba(self, style_description: &str) -> Result<[f32; 4], SdkError> {
        for (channel_name, channel_value) in [
            ("red", self.red),
            ("green", self.green),
            ("blue", self.blue),
            ("alpha", self.alpha),
        ] {
            if !((OVERLAY_MINIMUM_COLOR_CHANNEL..=OVERLAY_MAXIMUM_COLOR_CHANNEL)
                .contains(&channel_value))
            {
                return Err(SdkError::InvalidRenderInput {
                    detail: format!(
                        "{style_description} color channel {channel_name} \
                         {channel_value} outside [0, 1]"
                    ),
                });
            }
        }
        Ok([
            self.red as f32,
            self.green as f32,
            self.blue as f32,
            self.alpha as f32,
        ])
    }
}

/// Which overlay shape one descriptor configures — the kind tag of the
/// wasm canvas' plain-object overlay names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapOverlayKind {
    /// A routed path drawn as a thick line.
    RoutePolyline,
    /// A filled polygon: one outer ring, ear-clip triangulated.
    FilledPolygon,
    /// Screen-facing point markers (octagon fans).
    PointMarkers,
    /// Congestion-colored traffic-flow lines.
    TrafficFlow,
    /// Screen-facing teardrop pins anchored at their tip.
    PointPins,
}

/// One overlay shape drawn over the map surface's tiles in every frame,
/// selected by `kind`. Each kind reads exactly its own payload fields and
/// ignores the others (the wasm canvas carries the same tolerance); the
/// kind-required payload must be present.
#[derive(Debug, Clone, PartialEq)]
pub struct MapOverlay {
    /// Which overlay shape this descriptor configures and which payload
    /// fields carry it.
    pub kind: MapOverlayKind,
    /// Route polyline: the traversal-order polyline. Required for the kind.
    pub coordinates: Option<Vec<GeographicCoordinate>>,
    /// Route polyline, filled polygon and point markers: solid color
    /// override, or `None` for the kind's documented default.
    pub rgba_color: Option<OverlayColor>,
    /// Route polyline and traffic flow: line width in screen pixels
    /// (positive and finite), or `None` for the documented default.
    pub width_pixels: Option<f64>,
    /// Filled polygon: the outer ring without a repeated closing vertex.
    /// Required for the kind.
    pub ring_coordinates: Option<Vec<GeographicCoordinate>>,
    /// Point markers and point pins: the geographic anchor of every marker
    /// (a pin's anchor is its tip). Required for both kinds.
    pub point_marker_positions: Option<Vec<GeographicCoordinate>>,
    /// Point markers and point pins: marker or head radius in screen pixels
    /// (positive and finite), or `None` for the documented default.
    pub point_marker_radius_pixels: Option<f64>,
    /// Traffic flow: the flow observations to paint. Required for the kind.
    pub traffic_flow_segments: Option<Vec<TrafficFlowData>>,
}

impl MapOverlay {
    /// Validates a screen-pixel size field (a line width or marker radius):
    /// it must be positive and finite, like the wasm canvas' validation of
    /// the same fields. Native-only with the surface view's overlay
    /// converter.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn validated_size_pixels(
        size_pixels: f64,
        size_description: &str,
    ) -> Result<f64, SdkError> {
        if !size_pixels.is_finite() || size_pixels.is_sign_negative() || size_pixels == 0.0 {
            return Err(SdkError::InvalidRenderInput {
                detail: format!("{size_description} must be positive and finite"),
            });
        }
        Ok(size_pixels)
    }

    /// Builds the native overlay from the descriptor, refusing an
    /// incompatible kind/payload pair or an unusable style field. A `None`
    /// style field keeps the documented rustic-maps-rendering default of the kind.
    /// Native-only with the surface view that draws it.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn into_native_overlay(self) -> Result<rustic_maps_rendering::MapOverlay, SdkError> {
        match self.kind {
            MapOverlayKind::RoutePolyline => {
                let mut route_polyline = rustic_maps_rendering::RoutePolylineOverlay::new(
                    validated_coordinates(self.coordinates, "route polyline")?,
                );
                if let Some(rgba_color) = self.rgba_color {
                    route_polyline.rgba_color = rgba_color.to_native_rgba("route polyline")?;
                }
                if let Some(width_pixels) = self.width_pixels {
                    route_polyline.width_pixels =
                        Self::validated_size_pixels(width_pixels, "route polyline width")?;
                }
                Ok(rustic_maps_rendering::MapOverlay::RoutePolyline(
                    route_polyline,
                ))
            }
            MapOverlayKind::FilledPolygon => {
                let mut filled_polygon = rustic_maps_rendering::FilledPolygonOverlay::new(
                    validated_coordinates(self.ring_coordinates, "filled polygon ring")?,
                );
                if let Some(rgba_color) = self.rgba_color {
                    filled_polygon.rgba_color = rgba_color.to_native_rgba("filled polygon")?;
                }
                Ok(rustic_maps_rendering::MapOverlay::FilledPolygon(
                    filled_polygon,
                ))
            }
            MapOverlayKind::PointMarkers => {
                let mut point_markers = rustic_maps_rendering::PointMarkersOverlay::new(
                    validated_coordinates(self.point_marker_positions, "point markers")?,
                );
                if let Some(rgba_color) = self.rgba_color {
                    point_markers.rgba_color = rgba_color.to_native_rgba("point markers")?;
                }
                if let Some(radius_pixels) = self.point_marker_radius_pixels {
                    point_markers.radius_pixels =
                        Self::validated_size_pixels(radius_pixels, "point marker radius")?;
                }
                Ok(rustic_maps_rendering::MapOverlay::PointMarkers(
                    point_markers,
                ))
            }
            MapOverlayKind::PointPins => {
                let mut point_pins = rustic_maps_rendering::PointPinsOverlay::new(
                    validated_coordinates(self.point_marker_positions, "point pins")?,
                );
                if let Some(rgba_color) = self.rgba_color {
                    point_pins.rgba_color = rgba_color.to_native_rgba("point pins")?;
                }
                if let Some(radius_pixels) = self.point_marker_radius_pixels {
                    point_pins.radius_pixels =
                        Self::validated_size_pixels(radius_pixels, "point pin radius")?;
                }
                Ok(rustic_maps_rendering::MapOverlay::PointPins(point_pins))
            }
            MapOverlayKind::TrafficFlow => {
                let mut traffic_flow = rustic_maps_rendering::TrafficFlowOverlay::new(
                    self.traffic_flow_segments
                        .ok_or(SdkError::InvalidRenderInput {
                            detail: "traffic flow descriptor needs traffic_flow_segments"
                                .to_string(),
                        })?
                        .into_iter()
                        .map(|traffic_flow_segment| {
                            traffic_flow_segment.into_native_traffic_flow_data()
                        })
                        .collect::<Result<Vec<_>, SdkError>>()?,
                );
                if let Some(width_pixels) = self.width_pixels {
                    traffic_flow.width_pixels =
                        Self::validated_size_pixels(width_pixels, "traffic flow width")?;
                }
                Ok(rustic_maps_rendering::MapOverlay::TrafficFlow(traffic_flow))
            }
        }
    }
}

/// Converts the kind-required coordinate payload: missing payload is
/// refused, and present payload validates every coordinate the overlay
/// seam validates — the same canonical-degree boundary the projection
/// surface applies (`SdkError::InvalidCoordinateInput` for an out-of-range
/// latitude or longitude, matching the wasm canvas' overlay validation).
/// Native-only with the surface view's overlay converter.
#[cfg(not(target_arch = "wasm32"))]
fn validated_coordinates(
    coordinates: Option<Vec<GeographicCoordinate>>,
    payload_description: &str,
) -> Result<Vec<geospatial_types::GeographicCoordinate>, SdkError> {
    let mirror_coordinates = coordinates.ok_or(SdkError::InvalidRenderInput {
        detail: format!("{payload_description} descriptor needs its coordinates"),
    })?;
    mirror_coordinates
        .into_iter()
        .map(crate::projection::validated_coordinate)
        .collect()
}

/// The platform window surface handle of `MapView::attach`: exactly
/// one platform pointer is set. Pointers lower as `u64` (Kotlin `ULong`,
/// Swift `UInt64`), where a plain `u64 0` stands in for the foreign
/// languages' null pointer without ever dereferencing one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SurfaceHandle {
    /// macOS: pointer to the `NSView` the surface renders into.
    pub appkit_view_pointer: Option<u64>,
    /// iOS: pointer to the `UIView` the surface renders into.
    pub ios_view_pointer: Option<u64>,
    /// Android: pointer to the `ANativeWindow` obtained from the platform
    /// `Surface`.
    pub android_native_window_pointer: Option<u64>,
}

/// Which visual form the renderer-owned location layer draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocationIndicatorVisualKind {
    /// The soft tracking dot (no course known or no route): accent core,
    /// white ring and dark halo.
    TrackingDot,
    /// The course-directed navigation dart inside the same ring and halo
    /// (navigation follow mode); degrades to the tracking dot without a
    /// course.
    GuidedCourseArrow,
}

/// Style of the location layer; `None` fields keep the documented
/// `rustic-maps-rendering` defaults (the overlay-style pattern).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocationIndicatorStyle {
    /// Which visual form this layer draws.
    pub visual_kind: LocationIndicatorVisualKind,
    /// Accent-core radius in screen pixels; `None` keeps the default.
    pub radius_pixels: Option<f64>,
    /// Accent fill color of the core disc and navigation dart; `None` keeps
    /// the default.
    pub rgba_color: Option<OverlayColor>,
    /// Ring color: the ring around the puck and the navigation dart's
    /// outline; `None` keeps the default white.
    pub ring_rgba_color: Option<OverlayColor>,
    /// Halo color drawn under the puck; `None` keeps the default.
    pub halo_rgba_color: Option<OverlayColor>,
    /// Accuracy ring drawn around the puck; `None` keeps the default.
    pub show_accuracy_ring: Option<bool>,
}

/// One frame's location-layer state (the presentable subset of a position
/// fix): position, optional course over ground, optional horizontal accuracy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocationIndicatorState {
    /// Fix position.
    pub coordinate: GeographicCoordinate,
    /// Course over ground in degrees clockwise from true north; `None` when
    /// the source does not report one.
    pub course_heading_degrees: Option<f64>,
    /// Horizontal accuracy radius in meters; the accuracy ring renders when
    /// set and the style shows rings.
    pub horizontal_accuracy_meters: Option<f64>,
}

impl LocationIndicatorVisualKind {
    /// Builds the native visual kind tag from the mirror record.
    /// Native-only with the surface view that draws it.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn to_native_visual_kind(
        self,
    ) -> rustic_maps_rendering::LocationIndicatorVisualKind {
        match self {
            Self::TrackingDot => rustic_maps_rendering::LocationIndicatorVisualKind::TrackingDot,
            Self::GuidedCourseArrow => {
                rustic_maps_rendering::LocationIndicatorVisualKind::GuidedCourseArrow
            }
        }
    }
}

impl LocationIndicatorState {
    /// Validates the fix and builds the native location-layer state: the
    /// coordinate must be canonical degrees (the same boundary the overlay
    /// seam applies), while the optional scalar fields pass through
    /// verbatim (the renderer's clamps own their ranges). Native-only with
    /// the surface view that draws it.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn into_native_location_indicator_state(
        self,
    ) -> Result<rustic_maps_rendering::LocationIndicatorState, SdkError> {
        Ok(rustic_maps_rendering::LocationIndicatorState {
            coordinate: crate::projection::validated_coordinate(self.coordinate)?,
            course_heading_degrees: self.course_heading_degrees,
            horizontal_accuracy_meters: self.horizontal_accuracy_meters,
        })
    }
}

impl LocationIndicatorStyle {
    /// Validates the numeric style fields and builds the native style; the
    /// `None` fields keep the documented rustic-maps-rendering defaults (mirroring
    /// the overlay descriptor's style validation: a radius must be positive
    /// and finite, a color channel inside `[0, 1]`). Native-only with the
    /// surface view that draws it.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn into_native_location_indicator_style(
        self,
    ) -> Result<rustic_maps_rendering::LocationIndicatorStyle, SdkError> {
        if let Some(radius_pixels) = self.radius_pixels {
            MapOverlay::validated_size_pixels(radius_pixels, "location indicator disc radius")?;
        }
        let rgba_color = match self.rgba_color {
            Some(rgba_color) => Some(rgba_color.to_native_rgba("location indicator")?),
            None => None,
        };
        let ring_rgba_color = match self.ring_rgba_color {
            Some(ring_rgba_color) => {
                Some(ring_rgba_color.to_native_rgba("location indicator ring")?)
            }
            None => None,
        };
        let halo_rgba_color = match self.halo_rgba_color {
            Some(halo_rgba_color) => {
                Some(halo_rgba_color.to_native_rgba("location indicator halo")?)
            }
            None => None,
        };
        Ok(rustic_maps_rendering::LocationIndicatorStyle {
            visual_kind: self.visual_kind.to_native_visual_kind(),
            radius_pixels: self.radius_pixels,
            rgba_color,
            ring_rgba_color,
            halo_rgba_color,
            show_accuracy_ring: self.show_accuracy_ring,
        })
    }
}
