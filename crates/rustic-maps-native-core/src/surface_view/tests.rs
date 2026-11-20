use super::{MapSdk, MapView, SurfaceFrameRenderer, SurfaceSession, SurfaceSlot, VectorBaseSlot};
use crate::camera::MapCamera;
use crate::error::SdkError;
use crate::records::{
    BoundingBox, EdgeInsets, GeographicCoordinate, LocationIndicatorState, LocationIndicatorStyle,
    LocationIndicatorVisualKind, MapOverlay, MapOverlayKind, OverlayColor,
    ProviderVectorTileSource, TrafficFlowCongestionLevel, TrafficFlowData,
    UrlTemplateVectorTileSource, ViewportSize,
};
use async_trait::async_trait;
use geospatial_traits::{RasterTileProvider, VectorTileProvider, VectorTileSourceDescriptor};
use geospatial_types::{
    GeographicCoordinate as NativeGeographicCoordinate, RasterImageFormat, RasterTileData,
    RasterTileResolution, SDKError as NativeSdkError, VectorTileData,
};
use rustic_maps_rendering::{
    DEFAULT_FIELD_OF_VIEW_DEGREES, LabelLayer, LocationIndicatorLayer,
    MapCamera as RenderingMapCamera, MapOverlay as RenderingMapOverlay, PreparedTile,
    SurfaceFrameStatistics, SurfaceRenderError, TileKey, TileScheduler, TileTexturePool,
    VectorBaseScheduler, ViewportPixels, vector_style::VECTOR_STYLE_ID_BRIGHT_LIGHT,
};
use rustic_maps_test_support::{
    SACRAMENTO_LATITUDE_DEGREES as TEST_LATITUDE_DEGREES,
    SACRAMENTO_LONGITUDE_DEGREES as TEST_LONGITUDE_DEGREES,
};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::task::{Context, Poll, Waker};

/// Latitude outside the canonical range the coordinate validation
/// refuses.
const INVALID_LATITUDE_DEGREES: f64 = 91.0;
/// Point-marker style the override test asserts, in float channels.
const OVERRIDDEN_POINT_MARKER_RGBA: [f32; 4] = [1.0, 0.5, 0.0, 0.75];
/// Point-marker radius the override test asserts, in screen pixels.
const OVERRIDDEN_POINT_MARKER_RADIUS_PIXELS: f64 = 12.0;
/// Horizontal accuracy, in meters, the indicator tests' fix reports.
const TEST_INDICATOR_ACCURACY_METERS: f64 = 40.0;
/// The committed OpenFreeMap z14 vector tile over Sacramento: the demo
/// tile whose place layer names Sacramento.
const DEMO_VECTOR_TILE_BYTES: &[u8] =
    include_bytes!("../../../../fixtures/openfreemap/openmaptiles_tile_z14_x2661_y6286.pbf");
/// The demo tile's address.
const DEMO_VECTOR_TILE_KEY: TileKey = TileKey {
    tile_x: 2661,
    tile_y: 6286,
    zoom_level: 14,
};
/// The zoom level the demo frame resolves at.
const DEMO_FRAME_ZOOM_LEVEL: f64 = 14.0;
/// Edge of the square viewport the label assertions project into, in
/// screen pixels.
const LABEL_TEST_VIEWPORT_EDGE_PIXELS: f64 = 512.0;

/// A provider serving the one committed demo tile and refusing every other
/// address; the surface tests never touch the network.
struct CommittedVectorTileProvider {
    tile_bytes: Vec<u8>,
}

#[async_trait]
impl VectorTileProvider for CommittedVectorTileProvider {
    fn source_descriptor(&self) -> VectorTileSourceDescriptor {
        VectorTileSourceDescriptor {
            source_name: "committed-demo-tile".to_string(),
            attribution_text: "Map data © OpenStreetMap contributors".to_string(),
            minimum_zoom_level: 0,
            maximum_zoom_level: 14,
            style_hints: Vec::new(),
        }
    }

    async fn fetch_vector_tile(
        &self,
        tile_x: u32,
        tile_y: u32,
        zoom_level: u8,
    ) -> Result<VectorTileData, NativeSdkError> {
        let requested_tile_key = TileKey::new(tile_x, tile_y, zoom_level)?;
        if requested_tile_key != DEMO_VECTOR_TILE_KEY {
            return Err(NativeSdkError::ResponseParsingFailure(format!(
                "the committed provider serves no {zoom_level}/{tile_x}/{tile_y}"
            )));
        }
        VectorTileData::from_wire_bytes(tile_x, tile_y, zoom_level, self.tile_bytes.clone())
    }
}

fn coordinate(latitude: f64, longitude: f64) -> GeographicCoordinate {
    GeographicCoordinate {
        latitude,
        longitude,
    }
}

fn route_polyline_descriptor() -> MapOverlay {
    MapOverlay {
        kind: MapOverlayKind::RoutePolyline,
        coordinates: Some(vec![coordinate(
            TEST_LATITUDE_DEGREES,
            TEST_LONGITUDE_DEGREES,
        )]),
        rgba_color: None,
        width_pixels: None,
        ring_coordinates: None,
        point_marker_positions: None,
        point_marker_radius_pixels: None,
        traffic_flow_segments: None,
    }
}

fn overlay_view() -> MapView {
    MapView::new(Arc::new(MapSdk::new()))
}

/// Pixel edge of the test surface's configured extent.
const TEST_SURFACE_EXTENT_PIXELS: u32 = 256;

/// Byte budget of the test session's tile payload pool.
const TEST_TILE_PAYLOAD_POOL_MAXIMUM_BYTES: u64 = 1 << 20;

/// Missing tiles one test frame may fetch.
const TEST_MAXIMUM_TILE_FETCHES_PER_FRAME: usize = 4;

/// Completion-wait ticks for the spawned frame work to restore the slot.
const FRAME_RESTORE_WAIT_TICKS: usize = 200;

/// Duration of one completion-wait tick; well above the test frame's work.
const FRAME_RESTORE_WAIT_TICK: std::time::Duration = std::time::Duration::from_millis(10);

/// A raster provider serving nothing: the test frames never touch the
/// network and draw the tiles-only clear.
struct RefusingRasterTileProvider;

#[async_trait]
impl RasterTileProvider for RefusingRasterTileProvider {
    async fn fetch_raster_tile(
        &self,
        tile_x: u32,
        tile_y: u32,
        zoom_level: u8,
        _image_format: RasterImageFormat,
        _tile_resolution: RasterTileResolution,
    ) -> Result<RasterTileData, NativeSdkError> {
        Err(NativeSdkError::ResponseParsingFailure(format!(
            "the test provider serves no {zoom_level}/{tile_x}/{tile_y}"
        )))
    }
}

/// The tile the reload tests seed into the attached scheduler's pool.
const RELOAD_TEST_POOLED_TILE_KEY: TileKey = TileKey {
    tile_x: 2662,
    tile_y: 6286,
    zoom_level: 14,
};

/// A raster provider that serves one payload and counts its fetches, so
/// a test can tell which provider a frame's warm pass used.
struct CountingRasterTileProvider {
    /// The encoded payload every fetch answers with.
    encoded_image_bytes: Vec<u8>,
    /// How many fetches reached the provider.
    fetch_count: Arc<AtomicUsize>,
}

#[async_trait]
impl RasterTileProvider for CountingRasterTileProvider {
    async fn fetch_raster_tile(
        &self,
        tile_x: u32,
        tile_y: u32,
        zoom_level: u8,
        image_format: RasterImageFormat,
        tile_resolution: RasterTileResolution,
    ) -> Result<RasterTileData, NativeSdkError> {
        self.fetch_count.fetch_add(1, Ordering::SeqCst);
        Ok(RasterTileData {
            tile_x,
            tile_y,
            zoom_level,
            image_format,
            tile_resolution,
            image_bytes: self.encoded_image_bytes.clone(),
        })
    }
}

/// What the test frame renderer does when a frame renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestFrameBehavior {
    /// Answers a zeroed frame.
    Draw,
    /// Unwinds, exercising the ownership guard's panic path.
    Panic,
}

/// A one-shot gate a gated test frame waits on, so a test can drop the
/// caller's draw future while the spawned work is known to be in flight.
struct TestFrameGate {
    started: Mutex<bool>,
    started_condition: Condvar,
    released: Mutex<bool>,
    released_condition: Condvar,
}

impl TestFrameGate {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            started: Mutex::new(false),
            started_condition: Condvar::new(),
            released: Mutex::new(false),
            released_condition: Condvar::new(),
        })
    }

    /// Marks the frame started and blocks until the test releases it.
    fn enter_and_wait_for_release(&self) {
        {
            let mut started = self.started.lock().unwrap();
            *started = true;
            self.started_condition.notify_all();
        }
        let mut released = self.released.lock().unwrap();
        while !*released {
            released = self.released_condition.wait(released).unwrap();
        }
    }

    /// Blocks until the frame work has started rendering.
    fn wait_until_started(&self) {
        let mut started = self.started.lock().unwrap();
        while !*started {
            started = self.started_condition.wait(started).unwrap();
        }
    }

    /// Lets the frame work finish.
    fn release(&self) {
        let mut released = self.released.lock().unwrap();
        *released = true;
        self.released_condition.notify_all();
    }
}

/// The GPU-free [`SurfaceFrameRenderer`] double the cancellation tests
/// install into a session.
struct TestFrameRenderer {
    /// What one rendered frame does.
    behavior: TestFrameBehavior,
    /// The one-shot gate the first rendered frame waits on, if any.
    gate: Mutex<Option<Arc<TestFrameGate>>>,
    /// How many frames reached the render seam.
    rendered_frame_count: Arc<AtomicUsize>,
}

impl SurfaceFrameRenderer for TestFrameRenderer {
    fn configure_extent(
        &mut self,
        _width_pixels: u32,
        _height_pixels: u32,
    ) -> Result<(), SurfaceRenderError> {
        Ok(())
    }

    fn set_clear_color(&mut self, _clear_color: [f32; 4]) {}

    fn render_frame_layers_and_labels(
        &mut self,
        _camera: &RenderingMapCamera,
        _viewport: &ViewportPixels,
        _prepared_tiles: &[PreparedTile],
        _pool: &mut TileTexturePool,
        _overlay_layers: &[&[RenderingMapOverlay]],
        _overlay_device_pixel_ratio: f64,
        _location_indicator_layer: Option<&LocationIndicatorLayer>,
        _label_layer: Option<&LabelLayer>,
    ) -> Result<SurfaceFrameStatistics, SurfaceRenderError> {
        let gate = self.gate.lock().unwrap().take();
        if let Some(gate) = gate {
            gate.enter_and_wait_for_release();
        }
        self.rendered_frame_count.fetch_add(1, Ordering::SeqCst);
        match self.behavior {
            TestFrameBehavior::Draw => Ok(SurfaceFrameStatistics {
                requested_tile_count: 0,
                present_tile_count: 0,
                fallback_tile_count: 0,
                deferred_tile_count: 0,
                unavailable_tile_count: 0,
                overlay_triangle_count: 0,
            }),
            TestFrameBehavior::Panic => panic!("the test frame renderer panics"),
        }
    }
}

/// Installs a GPU-free session into the view's surface slot, standing in
/// for `attach` (which needs a real platform surface).
fn install_test_session(
    surface_view: &MapView,
    behavior: TestFrameBehavior,
    gate: Option<Arc<TestFrameGate>>,
    rendered_frame_count: Arc<AtomicUsize>,
) {
    let raster_tile_provider: Arc<dyn RasterTileProvider> = Arc::new(RefusingRasterTileProvider);
    let surface_session = SurfaceSession {
        surface_renderer: Box::new(TestFrameRenderer {
            behavior,
            gate: Mutex::new(gate),
            rendered_frame_count,
        }),
        tile_scheduler: TileScheduler::new(
            raster_tile_provider,
            TileTexturePool::new(TEST_TILE_PAYLOAD_POOL_MAXIMUM_BYTES),
            RasterImageFormat::Png,
            RasterTileResolution::Standard,
            TEST_MAXIMUM_TILE_FETCHES_PER_FRAME,
        ),
        width_pixels: TEST_SURFACE_EXTENT_PIXELS,
        height_pixels: TEST_SURFACE_EXTENT_PIXELS,
        previous_label_placements: Vec::new(),
        zoom_transition: rustic_maps_rendering::ZoomTransition::new(),
        zoom_transition_clock_origin: std::time::Instant::now(),
    };
    *surface_view.surface_slot_lock() = SurfaceSlot::Idle(Box::new(surface_session));
    surface_view.camera_state_lock().viewport = Some(ViewportSize {
        width_pixels: f64::from(TEST_SURFACE_EXTENT_PIXELS),
        height_pixels: f64::from(TEST_SURFACE_EXTENT_PIXELS),
    });
}

/// The camera the cancellation tests draw.
fn test_draw_camera() -> Arc<MapCamera> {
    Arc::new(
        MapCamera::new(
            coordinate(TEST_LATITUDE_DEGREES, TEST_LONGITUDE_DEGREES),
            12.0,
            0.0,
            0.0,
        )
        .expect("a canonical camera validates"),
    )
}

/// Zero edge insets, the natural default of `fit_bounds`.
fn zero_edge_insets() -> EdgeInsets {
    EdgeInsets {
        left: 0.0,
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
    }
}

#[test]
fn camera_readback_reports_the_neutral_default_pose() {
    let surface_view = overlay_view();
    let default_camera = surface_view.camera();
    assert_eq!(default_camera.center().latitude, 0.0);
    assert_eq!(default_camera.center().longitude, 0.0);
    assert_eq!(default_camera.zoom_level(), 2.0);
    assert_eq!(default_camera.bearing_degrees(), 0.0);
    assert_eq!(default_camera.pitch_degrees(), 0.0);
}

#[test]
fn set_camera_updates_the_readback_pose() {
    let surface_view = overlay_view();
    let adopted_camera = MapCamera::new(
        coordinate(TEST_LATITUDE_DEGREES, TEST_LONGITUDE_DEGREES),
        14.0,
        40.0,
        25.0,
    )
    .expect("a canonical camera validates");
    surface_view.set_camera(Arc::new(adopted_camera));
    let readback = surface_view.camera();
    assert_eq!(readback.center().latitude, TEST_LATITUDE_DEGREES);
    assert_eq!(readback.center().longitude, TEST_LONGITUDE_DEGREES);
    assert_eq!(readback.zoom_level(), 14.0);
    assert_eq!(readback.bearing_degrees(), 40.0);
    assert_eq!(readback.pitch_degrees(), 25.0);
}

#[test]
fn fit_bounds_adopts_the_fitted_camera_for_the_attached_viewport() {
    let surface_view = overlay_view();
    install_test_session(
        &surface_view,
        TestFrameBehavior::Draw,
        None,
        Arc::new(AtomicUsize::new(0)),
    );
    let california_box = BoundingBox {
        south_west: coordinate(32.5, -124.4),
        north_east: coordinate(42.0, -114.1),
    };
    let fitted_camera = surface_view
        .fit_bounds(california_box, zero_edge_insets(), 12.0)
        .expect("the fitted camera accepts a canonical box and usable insets")
        .expect("the test session attaches a viewport");
    // Fitted cameras are top-down and north-up by construction.
    assert_eq!(fitted_camera.pitch_degrees(), 0.0);
    assert_eq!(fitted_camera.bearing_degrees(), 0.0);
    assert!(fitted_camera.zoom_level() > 0.0 && fitted_camera.zoom_level() <= 12.0);
    let fitted_center = fitted_camera.center();
    assert!(fitted_center.latitude >= california_box.south_west.latitude);
    assert!(fitted_center.latitude <= california_box.north_east.latitude);
    assert!(fitted_center.longitude >= california_box.south_west.longitude);
    assert!(fitted_center.longitude <= california_box.north_east.longitude);
    // The readback reports exactly the adopted pose.
    let readback = surface_view.camera();
    assert_eq!(readback.center().latitude, fitted_center.latitude);
    assert_eq!(readback.center().longitude, fitted_center.longitude);
    assert_eq!(readback.zoom_level(), fitted_camera.zoom_level());
}

#[test]
fn fit_bounds_answers_null_without_an_attached_viewport() {
    let surface_view = overlay_view();
    let previous_camera = surface_view.camera();
    let fitted_camera = surface_view
        .fit_bounds(
            BoundingBox {
                south_west: coordinate(32.5, -124.4),
                north_east: coordinate(42.0, -114.1),
            },
            zero_edge_insets(),
            12.0,
        )
        .expect("the fit itself is valid");
    assert!(fitted_camera.is_none(), "a detached view fits nothing");
    assert_eq!(
        surface_view.camera().zoom_level(),
        previous_camera.zoom_level(),
        "a detached fit leaves the held pose untouched"
    );
}

#[test]
fn fit_bounds_rejects_invalid_input_and_keeps_the_previous_camera() {
    let surface_view = overlay_view();
    install_test_session(
        &surface_view,
        TestFrameBehavior::Draw,
        None,
        Arc::new(AtomicUsize::new(0)),
    );
    let previous_camera = MapCamera::new(
        coordinate(TEST_LATITUDE_DEGREES, TEST_LONGITUDE_DEGREES),
        9.0,
        15.0,
        12.0,
    )
    .expect("a canonical camera validates");
    surface_view.set_camera(Arc::new(previous_camera));
    let inverted_box = BoundingBox {
        south_west: coordinate(42.0, -114.1),
        north_east: coordinate(32.5, -124.4),
    };
    let refused_fit = surface_view.fit_bounds(inverted_box, zero_edge_insets(), 12.0);
    assert!(matches!(
        refused_fit,
        Err(SdkError::InvalidCoordinateInput { .. })
    ));
    let readback = surface_view.camera();
    assert_eq!(readback.center().latitude, TEST_LATITUDE_DEGREES);
    assert_eq!(readback.zoom_level(), 9.0);
    assert_eq!(readback.bearing_degrees(), 15.0);
}

#[test]
fn camera_geographic_bounds_frames_the_attached_viewport_around_the_center() {
    let surface_view = overlay_view();
    install_test_session(
        &surface_view,
        TestFrameBehavior::Draw,
        None,
        Arc::new(AtomicUsize::new(0)),
    );
    let viewport_bounds = surface_view
        .camera_geographic_bounds()
        .expect("a top-down viewport at the neutral pose projects its corners");
    // The neutral pose frames the origin, so the bounds straddle it and
    // the projection is symmetric about the center.
    assert!(viewport_bounds.south_west.latitude < 0.0);
    assert!(viewport_bounds.north_east.latitude > 0.0);
    assert!(viewport_bounds.south_west.longitude < 0.0);
    assert!(viewport_bounds.north_east.longitude > 0.0);
    assert!(
        (viewport_bounds.north_east.longitude + viewport_bounds.south_west.longitude).abs() < 1e-6,
        "the longitude span is centered on the origin: {viewport_bounds:?}"
    );
    assert!(viewport_bounds.north_east.longitude - viewport_bounds.south_west.longitude > 0.0);
}

#[test]
fn camera_geographic_bounds_answers_null_when_detached() {
    let surface_view = overlay_view();
    assert!(surface_view.camera_geographic_bounds().is_none());
    install_test_session(
        &surface_view,
        TestFrameBehavior::Draw,
        None,
        Arc::new(AtomicUsize::new(0)),
    );
    assert!(surface_view.camera_geographic_bounds().is_some());
    surface_view.detach();
    assert!(
        surface_view.camera_geographic_bounds().is_none(),
        "detach clears the viewport the bounds are read at"
    );
}

/// Polls a future once with a no-op waker.
fn poll_once<FutureType: Future>(future: Pin<&mut FutureType>) -> Poll<FutureType::Output> {
    future.poll(&mut Context::from_waker(Waker::noop()))
}

/// Waits for the spawned frame work to restore the view's surface slot.
async fn wait_until_surface_restored(surface_view: &MapView) {
    for _ in 0..FRAME_RESTORE_WAIT_TICKS {
        if matches!(*surface_view.surface_slot_lock(), SurfaceSlot::Idle(_)) {
            return;
        }
        tokio::time::sleep(FRAME_RESTORE_WAIT_TICK).await;
    }
    panic!("the spawned frame work did not restore the surface slot");
}

/// Experiments read the stored list; the production surface keeps it
/// private, so the test module reaches the same field.
fn stored_overlays(surface_view: &MapView) -> Vec<RenderingMapOverlay> {
    surface_view
        .overlays
        .lock()
        .unwrap()
        .drawn_overlays()
        .as_ref()
        .clone()
}

/// Asserts the failure names the refusing reason fragment verbatim.
fn assert_refusal_contains(failure: SdkError, detail_fragment: &str) {
    let failure_text = failure.to_string();
    assert!(
        failure_text.contains(detail_fragment),
        "the refusal must carry the reason: {failure_text}"
    );
}

#[test]
fn a_route_polyline_descriptor_converts_with_the_documented_defaults() {
    let surface_view = overlay_view();
    surface_view
        .set_overlays(vec![route_polyline_descriptor()])
        .expect("a complete route polyline descriptor validates");
    let overlays = stored_overlays(&surface_view);
    assert_eq!(overlays.len(), 1);
    let (converted_rgba_color, converted_width_pixels) = match &overlays[0] {
        RenderingMapOverlay::RoutePolyline(route_polyline) => {
            (route_polyline.rgba_color, route_polyline.width_pixels)
        }
        other => panic!("the wrong overlay kind converted: {other:?}"),
    };
    // The default style is the rustic-maps-rendering crate's own constants.
    assert_eq!(
        converted_rgba_color,
        rustic_maps_rendering::DEFAULT_ROUTE_LINE_RGBA
    );
    assert_eq!(
        converted_width_pixels,
        rustic_maps_rendering::DEFAULT_ROUTE_LINE_WIDTH_PIXELS
    );
}

#[test]
fn a_point_marker_descriptor_overrides_its_style_fields() {
    let surface_view = overlay_view();
    surface_view
        .set_overlays(vec![MapOverlay {
            kind: MapOverlayKind::PointMarkers,
            coordinates: None,
            rgba_color: Some(OverlayColor {
                red: f64::from(OVERRIDDEN_POINT_MARKER_RGBA[0]),
                green: f64::from(OVERRIDDEN_POINT_MARKER_RGBA[1]),
                blue: f64::from(OVERRIDDEN_POINT_MARKER_RGBA[2]),
                alpha: f64::from(OVERRIDDEN_POINT_MARKER_RGBA[3]),
            }),
            width_pixels: None,
            ring_coordinates: None,
            point_marker_positions: Some(vec![coordinate(
                TEST_LATITUDE_DEGREES,
                TEST_LONGITUDE_DEGREES,
            )]),
            point_marker_radius_pixels: Some(OVERRIDDEN_POINT_MARKER_RADIUS_PIXELS),
            traffic_flow_segments: None,
        }])
        .expect("a complete point marker descriptor validates");
    let (converted_rgba_color, converted_radius_pixels) = match &stored_overlays(&surface_view)[0] {
        RenderingMapOverlay::PointMarkers(point_markers) => {
            (point_markers.rgba_color, point_markers.radius_pixels)
        }
        other => panic!("the wrong overlay kind converted: {other:?}"),
    };
    assert_eq!(converted_rgba_color, OVERRIDDEN_POINT_MARKER_RGBA);
    assert_eq!(
        converted_radius_pixels,
        OVERRIDDEN_POINT_MARKER_RADIUS_PIXELS
    );
}

#[test]
fn a_kind_payload_missing_refuses_the_descriptor() {
    let mut missing_payload_descriptor = route_polyline_descriptor();
    missing_payload_descriptor.coordinates = None;
    let failure = overlay_view()
        .set_overlays(vec![missing_payload_descriptor])
        .expect_err("a route polyline without coordinates must refuse");
    assert_refusal_contains(
        failure,
        "overlay 0 is invalid: route polyline descriptor needs its coordinates",
    );
}

#[test]
fn an_out_of_range_color_channel_refuses_the_descriptor() {
    let mut colored_descriptor = route_polyline_descriptor();
    colored_descriptor.rgba_color = Some(OverlayColor {
        red: 1.0,
        green: 1.5,
        blue: 0.0,
        alpha: 1.0,
    });
    let failure = overlay_view()
        .set_overlays(vec![colored_descriptor])
        .expect_err("a color channel outside [0, 1] must refuse");
    assert_refusal_contains(failure, "channel green 1.5 outside [0, 1]");
}

#[test]
fn a_non_positive_width_refuses_the_descriptor() {
    let mut zero_width_descriptor = route_polyline_descriptor();
    zero_width_descriptor.width_pixels = Some(0.0);
    let failure = overlay_view()
        .set_overlays(vec![zero_width_descriptor])
        .expect_err("a non-positive line width must refuse");
    assert_refusal_contains(
        failure,
        "overlay 0 is invalid: route polyline width must be positive and finite",
    );
}

#[test]
fn an_out_of_range_overlay_coordinate_refuses_with_the_coordinate_boundary() {
    let mut out_of_range_descriptor = route_polyline_descriptor();
    out_of_range_descriptor.coordinates = Some(vec![coordinate(
        INVALID_LATITUDE_DEGREES,
        TEST_LONGITUDE_DEGREES,
    )]);
    let failure = overlay_view()
        .set_overlays(vec![out_of_range_descriptor])
        .expect_err("an overlay coordinate outside its range must refuse");
    assert!(matches!(failure, SdkError::InvalidCoordinateInput { .. }));
}

#[test]
fn a_traffic_flow_descriptor_converts_its_congestion_classification() {
    let surface_view = overlay_view();
    surface_view
        .set_overlays(vec![MapOverlay {
            kind: MapOverlayKind::TrafficFlow,
            coordinates: None,
            rgba_color: None,
            width_pixels: None,
            ring_coordinates: None,
            point_marker_positions: None,
            point_marker_radius_pixels: None,
            traffic_flow_segments: Some(vec![TrafficFlowData {
                shape_coordinates: vec![
                    coordinate(TEST_LATITUDE_DEGREES, TEST_LONGITUDE_DEGREES),
                    coordinate(TEST_LATITUDE_DEGREES + 0.01, TEST_LONGITUDE_DEGREES),
                ],
                current_speed_kilometers_per_hour: Some(35.0),
                free_flow_speed_kilometers_per_hour: Some(80.0),
                congestion_level: TrafficFlowCongestionLevel::Slow,
            }]),
        }])
        .expect("a complete traffic flow descriptor validates");
    let stored_overlays = stored_overlays(&surface_view);
    let (converted_flows, converted_width_pixels) = match &stored_overlays[0] {
        RenderingMapOverlay::TrafficFlow(traffic_flow) => {
            (&traffic_flow.flows, traffic_flow.width_pixels)
        }
        other => panic!("the wrong overlay kind converted: {other:?}"),
    };
    assert_eq!(converted_flows.len(), 1);
    // The classification copy keeps the level, whose color lookup
    // paints the documented slow-traffic amber.
    let converted_congestion_level = converted_flows[0].congestion_level;
    assert!(
        matches!(
            converted_congestion_level,
            geospatial_types::TrafficFlowCongestionLevel::Slow
        ),
        "the traffic segment classification must copy verbatim"
    );
    assert_eq!(
        rustic_maps_rendering::traffic_congestion_rgba(converted_congestion_level),
        rustic_maps_rendering::TRAFFIC_SLOW_RGBA
    );
    assert_eq!(
        converted_width_pixels,
        rustic_maps_rendering::DEFAULT_TRAFFIC_LINE_WIDTH_PIXELS
    );
}

#[test]
fn a_filled_polygon_descriptor_converts_with_the_documented_default() {
    let surface_view = overlay_view();
    surface_view
        .set_overlays(vec![MapOverlay {
            kind: MapOverlayKind::FilledPolygon,
            coordinates: None,
            rgba_color: None,
            width_pixels: None,
            ring_coordinates: Some(vec![
                coordinate(TEST_LATITUDE_DEGREES, TEST_LONGITUDE_DEGREES),
                coordinate(TEST_LATITUDE_DEGREES + 0.01, TEST_LONGITUDE_DEGREES),
                coordinate(TEST_LATITUDE_DEGREES, TEST_LONGITUDE_DEGREES + 0.01),
            ]),
            point_marker_positions: None,
            point_marker_radius_pixels: None,
            traffic_flow_segments: None,
        }])
        .expect("a complete filled polygon descriptor validates");
    let converted_rgba_color = match &stored_overlays(&surface_view)[0] {
        RenderingMapOverlay::FilledPolygon(filled_polygon) => filled_polygon.rgba_color,
        other => panic!("the wrong overlay kind converted: {other:?}"),
    };
    assert_eq!(
        converted_rgba_color,
        rustic_maps_rendering::DEFAULT_FILLED_POLYGON_RGBA
    );
}

#[test]
fn an_invalid_partial_list_keeps_the_previous_overlay_list_in_force() {
    let surface_view = overlay_view();
    surface_view
        .set_overlays(vec![route_polyline_descriptor()])
        .expect("the first list validates");
    // The second call carries an invalid second entry: the whole call
    // refuses and the first list draws on.
    let mut invalid_second_descriptor = route_polyline_descriptor();
    invalid_second_descriptor.width_pixels = Some(0.0);
    let failure = surface_view
        .set_overlays(vec![route_polyline_descriptor(), invalid_second_descriptor])
        .expect_err("the all-or-nothing rule refuses the whole call");
    let failure_text = failure.to_string();
    assert!(
        failure_text.contains("overlay 1 is invalid"),
        "the refusal names the failed entry: {failure_text}"
    );
    assert_eq!(stored_overlays(&surface_view).len(), 1);
}

#[test]
fn clear_overlays_empties_the_list() {
    let surface_view = overlay_view();
    surface_view
        .set_overlays(vec![route_polyline_descriptor()])
        .expect("the list validates");
    surface_view.clear_overlays();
    assert!(stored_overlays(&surface_view).is_empty());
}

#[test]
fn an_unchanged_overlay_list_is_shared_instead_of_deep_cloned_per_frame() {
    let surface_view = overlay_view();
    surface_view
        .set_overlays(vec![route_polyline_descriptor()])
        .expect("the list validates");
    let first_snapshot = surface_view.overlays_lock().drawn_overlays();
    let second_snapshot = surface_view.overlays_lock().drawn_overlays();
    assert!(
        Arc::ptr_eq(&first_snapshot, &second_snapshot),
        "a frame snapshot clones the shared payload, never the overlay list"
    );
    let revision_before_mutation = surface_view.overlays_lock().revision();

    // A mutation after the snapshot was taken copies the list once for
    // the store, leaving the in-flight frame drawing its own snapshot.
    surface_view
        .add_overlay(route_polyline_descriptor())
        .expect("the added overlay validates");
    assert_eq!(
        first_snapshot.len(),
        1,
        "the in-flight snapshot keeps the list it captured"
    );
    let snapshot_after_mutation = surface_view.overlays_lock().drawn_overlays();
    assert_eq!(snapshot_after_mutation.len(), 2);
    assert!(
        !Arc::ptr_eq(&first_snapshot, &snapshot_after_mutation),
        "the mutation gave the store its own payload"
    );
    assert!(
        surface_view.overlays_lock().revision() > revision_before_mutation,
        "every overlay mutation advances the settle revision"
    );
}

#[test]
fn every_overlay_mutation_advances_the_settle_revision() {
    let surface_view = overlay_view();
    let initial_revision = surface_view.overlays_lock().revision();
    surface_view
        .set_overlays(vec![route_polyline_descriptor()])
        .expect("the list validates");
    let after_set = surface_view.overlays_lock().revision();
    assert!(after_set > initial_revision);
    let overlay_identifier = surface_view
        .add_overlay(route_polyline_descriptor())
        .expect("the added overlay validates");
    let after_add = surface_view.overlays_lock().revision();
    assert!(after_add > after_set);
    surface_view
        .update_overlay(overlay_identifier.clone(), route_polyline_descriptor())
        .expect("a known identifier updates");
    let after_update = surface_view.overlays_lock().revision();
    assert!(after_update > after_add);
    assert!(surface_view.remove_overlay(overlay_identifier));
    let after_remove = surface_view.overlays_lock().revision();
    assert!(after_remove > after_update);
    surface_view.clear_overlays();
    assert!(surface_view.overlays_lock().revision() > after_remove);
}

/// Route-line width the addressed-overlay tests replace a descriptor
/// with, in screen pixels (distinct from the documented default).
const UPDATED_ROUTE_LINE_WIDTH_PIXELS: f64 = 7.0;

/// A route polyline descriptor carrying an explicit width, so an
/// addressed replacement is observable in the stored native overlay.
fn route_polyline_descriptor_with_width(width_pixels: f64) -> MapOverlay {
    MapOverlay {
        width_pixels: Some(width_pixels),
        ..route_polyline_descriptor()
    }
}

#[test]
fn add_overlay_assigns_monotonic_identifiers_in_draw_order() {
    let surface_view = overlay_view();
    let first_identifier = surface_view
        .add_overlay(route_polyline_descriptor())
        .expect("the first descriptor validates");
    let second_identifier = surface_view
        .add_overlay(route_polyline_descriptor())
        .expect("the second descriptor validates");
    assert_eq!(first_identifier, "overlay-1");
    assert_eq!(second_identifier, "overlay-2");
    assert_eq!(
        stored_overlays(&surface_view).len(),
        2,
        "every added overlay joins the drawn list"
    );
}

#[test]
fn add_overlay_refuses_an_invalid_descriptor_and_adds_nothing() {
    let surface_view = overlay_view();
    let failure = surface_view
        .add_overlay(route_polyline_descriptor_with_width(0.0))
        .expect_err("a non-positive line width must refuse");
    assert_refusal_contains(
        failure,
        "the overlay is invalid: route polyline width must be positive and finite",
    );
    assert!(stored_overlays(&surface_view).is_empty());
    // A refused descriptor consumes no identifier: the next add still
    // answers the first identifier.
    assert_eq!(
        surface_view
            .add_overlay(route_polyline_descriptor())
            .expect("the valid descriptor validates"),
        "overlay-1"
    );
}

#[test]
fn update_overlay_replaces_the_addressed_overlay_in_place() {
    let surface_view = overlay_view();
    let first_identifier = surface_view
        .add_overlay(route_polyline_descriptor())
        .expect("the first descriptor validates");
    surface_view
        .add_overlay(route_polyline_descriptor())
        .expect("the second descriptor validates");
    surface_view
        .update_overlay(
            first_identifier,
            route_polyline_descriptor_with_width(UPDATED_ROUTE_LINE_WIDTH_PIXELS),
        )
        .expect("a known identifier with a valid descriptor updates");
    let stored_overlays = stored_overlays(&surface_view);
    assert_eq!(stored_overlays.len(), 2, "update never changes the length");
    match &stored_overlays[0] {
        RenderingMapOverlay::RoutePolyline(route_polyline) => {
            assert_eq!(
                route_polyline.width_pixels, UPDATED_ROUTE_LINE_WIDTH_PIXELS,
                "the addressed entry was replaced in its own position"
            );
        }
        other => panic!("the wrong overlay kind stored: {other:?}"),
    }
    match &stored_overlays[1] {
        RenderingMapOverlay::RoutePolyline(route_polyline) => assert_eq!(
            route_polyline.width_pixels,
            rustic_maps_rendering::DEFAULT_ROUTE_LINE_WIDTH_PIXELS,
            "the other addressed overlay stays untouched"
        ),
        other => panic!("the wrong overlay kind stored: {other:?}"),
    }
}

#[test]
fn update_overlay_refuses_an_unknown_identifier_with_a_precise_detail() {
    let surface_view = overlay_view();
    let failure = surface_view
        .update_overlay("overlay-404".to_string(), route_polyline_descriptor())
        .expect_err("an unknown identifier must refuse");
    assert!(matches!(failure, SdkError::InvalidRenderInput { .. }));
    assert_refusal_contains(
        failure,
        "no overlay is addressed by identifier `overlay-404`",
    );
}

#[test]
fn update_overlay_with_an_invalid_descriptor_keeps_the_previous_overlay() {
    let surface_view = overlay_view();
    let overlay_identifier = surface_view
        .add_overlay(route_polyline_descriptor())
        .expect("the descriptor validates");
    let overlays_before_update = stored_overlays(&surface_view);
    let failure = surface_view
        .update_overlay(
            overlay_identifier,
            route_polyline_descriptor_with_width(0.0),
        )
        .expect_err("a non-positive line width must refuse");
    assert_refusal_contains(failure, "the overlay is invalid");
    assert_eq!(
        stored_overlays(&surface_view),
        overlays_before_update,
        "a refused replacement keeps the previous overlay"
    );
}

#[test]
fn remove_overlay_answers_presence_and_removes_only_the_addressed_overlay() {
    let surface_view = overlay_view();
    let first_identifier = surface_view
        .add_overlay(route_polyline_descriptor())
        .expect("the first descriptor validates");
    let second_identifier = surface_view
        .add_overlay(route_polyline_descriptor_with_width(
            UPDATED_ROUTE_LINE_WIDTH_PIXELS,
        ))
        .expect("the second descriptor validates");
    assert!(surface_view.remove_overlay(first_identifier.clone()));
    let remaining_overlays = stored_overlays(&surface_view);
    assert_eq!(remaining_overlays.len(), 1);
    match &remaining_overlays[0] {
        RenderingMapOverlay::RoutePolyline(route_polyline) => assert_eq!(
            route_polyline.width_pixels, UPDATED_ROUTE_LINE_WIDTH_PIXELS,
            "the unaddressed overlay the caller did not remove survives"
        ),
        other => panic!("the wrong overlay kind stored: {other:?}"),
    }
    assert!(
        !surface_view.remove_overlay(first_identifier),
        "a removed identifier is no longer present"
    );
    assert!(surface_view.remove_overlay(second_identifier));
    assert!(stored_overlays(&surface_view).is_empty());
}

#[test]
fn set_overlays_drops_the_addressed_bookkeeping_and_replaces_the_list() {
    let surface_view = overlay_view();
    let overlay_identifier = surface_view
        .add_overlay(route_polyline_descriptor())
        .expect("the descriptor validates");
    surface_view
        .set_overlays(vec![
            route_polyline_descriptor(),
            route_polyline_descriptor(),
        ])
        .expect("the bulk list validates");
    assert_eq!(
        stored_overlays(&surface_view).len(),
        2,
        "the bulk call replaces the whole drawn list"
    );
    assert!(
        matches!(
            surface_view.update_overlay(overlay_identifier.clone(), route_polyline_descriptor()),
            Err(SdkError::InvalidRenderInput { .. })
        ),
        "a bulk replacement invalidates the earlier identifier"
    );
    assert!(!surface_view.remove_overlay(overlay_identifier));
    assert_eq!(
        surface_view
            .add_overlay(route_polyline_descriptor())
            .expect("the next add validates"),
        "overlay-2",
        "the identifier counter survives the bulk replacement"
    );
    assert_eq!(stored_overlays(&surface_view).len(), 3);
}

#[test]
fn clear_overlays_clears_the_addressed_set_and_keeps_identifiers_unique() {
    let surface_view = overlay_view();
    let overlay_identifier = surface_view
        .add_overlay(route_polyline_descriptor())
        .expect("the descriptor validates");
    surface_view.clear_overlays();
    assert!(stored_overlays(&surface_view).is_empty());
    assert!(!surface_view.remove_overlay(overlay_identifier));
    assert_eq!(
        surface_view
            .add_overlay(route_polyline_descriptor())
            .expect("the next add validates"),
        "overlay-2",
        "the identifier counter survives the clear"
    );
}

#[test]
fn addressed_overlay_state_outlives_detach() {
    let surface_view = overlay_view();
    let overlay_identifier = surface_view
        .add_overlay(route_polyline_descriptor())
        .expect("the descriptor validates");
    // The view is detached (no attach happened); detaching again is a
    // no-op and must not touch the overlay state.
    surface_view.detach();
    surface_view
        .update_overlay(
            overlay_identifier.clone(),
            route_polyline_descriptor_with_width(UPDATED_ROUTE_LINE_WIDTH_PIXELS),
        )
        .expect("an addressed overlay stays updatable detached");
    assert_eq!(stored_overlays(&surface_view).len(), 1);
    assert!(surface_view.remove_overlay(overlay_identifier));
}

#[tokio::test]
async fn a_detached_view_refuses_its_draw() {
    let surface_view = overlay_view();
    let camera = MapCamera::new(
        coordinate(TEST_LATITUDE_DEGREES, TEST_LONGITUDE_DEGREES),
        12.0,
        0.0,
        0.0,
    )
    .expect("a canonical camera validates");
    let failure = surface_view
        .draw(Arc::new(camera))
        .await
        .expect_err("a detached view must refuse its draw");
    assert_refusal_contains(failure, "the map surface view is detached");
}

#[tokio::test]
async fn reloading_the_tile_source_drops_pooled_tiles_without_detaching_the_surface() {
    let map_sdk = Arc::new(MapSdk::new());
    let fetch_count = Arc::new(AtomicUsize::new(0));
    map_sdk.register_raster_tile_provider(Arc::new(CountingRasterTileProvider {
        encoded_image_bytes: vec![7; 32],
        fetch_count: Arc::clone(&fetch_count),
    }));
    let surface_view = MapView::new(map_sdk);
    install_test_session(
        &surface_view,
        TestFrameBehavior::Draw,
        None,
        Arc::new(AtomicUsize::new(0)),
    );
    {
        let mut slot_state = surface_view.surface_slot_lock();
        match &mut *slot_state {
            SurfaceSlot::Idle(session) => session
                .tile_scheduler
                .texture_pool_mut()
                .insert(RELOAD_TEST_POOLED_TILE_KEY, vec![7; 32]),
            SurfaceSlot::Busy | SurfaceSlot::Detached => {
                panic!("the installed test session is attached")
            }
        }
    }

    let camera = test_draw_camera();
    surface_view
        .draw(Arc::clone(&camera))
        .await
        .expect("the test frame draws");
    assert!(
        surface_view
            .is_frame_settled(Arc::clone(&camera))
            .expect("the attached view answers its settle check"),
        "a frame with no pending pixels is settled"
    );

    surface_view
        .reload_tile_source()
        .expect("a registered provider reloads");

    assert!(
        !surface_view
            .is_frame_settled(Arc::clone(&camera))
            .expect("the reload keeps the surface attached"),
        "a reload leaves the view unsettled so the pump draws the re-planned frame"
    );
    match &*surface_view.surface_slot_lock() {
        SurfaceSlot::Idle(session) => {
            assert!(
                !session
                    .tile_scheduler
                    .texture_pool()
                    .contains(&RELOAD_TEST_POOLED_TILE_KEY),
                "the previous source's pooled payload is gone"
            );
            assert!(
                session.tile_scheduler.warming_tile_keys().is_empty(),
                "the fresh scheduler starts with no warm fetch in flight"
            );
        }
        SurfaceSlot::Busy | SurfaceSlot::Detached => {
            panic!("the reload must not detach the surface")
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_frame_after_a_reload_plans_fetches_through_the_re_resolved_provider() {
    let map_sdk = Arc::new(MapSdk::new());
    let fetch_count = Arc::new(AtomicUsize::new(0));
    map_sdk.register_raster_tile_provider(Arc::new(CountingRasterTileProvider {
        encoded_image_bytes: vec![7; 32],
        fetch_count: Arc::clone(&fetch_count),
    }));
    let surface_view = MapView::new(map_sdk);
    install_test_session(
        &surface_view,
        TestFrameBehavior::Draw,
        None,
        Arc::new(AtomicUsize::new(0)),
    );

    surface_view
        .draw(test_draw_camera())
        .await
        .expect("the frame before the reload draws");
    assert_eq!(
        fetch_count.load(Ordering::SeqCst),
        0,
        "the attached session keeps its own provider until a reload"
    );

    surface_view
        .reload_tile_source()
        .expect("a registered provider reloads");
    surface_view
        .draw(test_draw_camera())
        .await
        .expect("the frame after the reload draws");

    for _ in 0..FRAME_RESTORE_WAIT_TICKS {
        if fetch_count.load(Ordering::SeqCst) > 0 {
            break;
        }
        tokio::time::sleep(FRAME_RESTORE_WAIT_TICK).await;
    }
    assert!(
        fetch_count.load(Ordering::SeqCst) > 0,
        "the frame after the reload plans its fetches through the re-resolved provider"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reload_landing_mid_draw_applies_when_the_draw_restores() {
    let map_sdk = Arc::new(MapSdk::new());
    let fetch_count = Arc::new(AtomicUsize::new(0));
    map_sdk.register_raster_tile_provider(Arc::new(CountingRasterTileProvider {
        encoded_image_bytes: vec![7; 32],
        fetch_count: Arc::clone(&fetch_count),
    }));
    let surface_view = MapView::new(map_sdk);
    let gate = TestFrameGate::new();
    install_test_session(
        &surface_view,
        TestFrameBehavior::Draw,
        Some(Arc::clone(&gate)),
        Arc::new(AtomicUsize::new(0)),
    );

    let mut draw_future = Box::pin(surface_view.draw(test_draw_camera()));
    assert!(
        matches!(poll_once(draw_future.as_mut()), Poll::Pending),
        "the spawned frame work holds the session across the runtime handback"
    );
    gate.wait_until_started();
    surface_view
        .reload_tile_source()
        .expect("a mid-draw reload defers instead of refusing");
    gate.release();
    draw_future
        .await
        .expect("the gated frame completes after the release");
    wait_until_surface_restored(&surface_view).await;

    assert!(
        !surface_view
            .is_frame_settled(test_draw_camera())
            .expect("the restored view answers its settle check"),
        "the applied reload leaves the view unsettled"
    );
    surface_view
        .draw(test_draw_camera())
        .await
        .expect("the following frame draws");
    for _ in 0..FRAME_RESTORE_WAIT_TICKS {
        if fetch_count.load(Ordering::SeqCst) > 0 {
            break;
        }
        tokio::time::sleep(FRAME_RESTORE_WAIT_TICK).await;
    }
    assert!(
        fetch_count.load(Ordering::SeqCst) > 0,
        "the following frame plans its fetches through the mid-draw reload's provider"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_the_draw_future_mid_frame_restores_the_slot_and_the_next_draw_succeeds() {
    let surface_view = overlay_view();
    let gate = TestFrameGate::new();
    let rendered_frame_count = Arc::new(AtomicUsize::new(0));
    install_test_session(
        &surface_view,
        TestFrameBehavior::Draw,
        Some(Arc::clone(&gate)),
        Arc::clone(&rendered_frame_count),
    );

    let mut draw_future = Box::pin(surface_view.draw(test_draw_camera()));
    assert!(
        matches!(poll_once(draw_future.as_mut()), Poll::Pending),
        "the spawned frame work holds the session across the runtime handback"
    );
    // The work is provably inside the render seam (it owns the session and
    // scheduler) before the caller's future disappears.
    gate.wait_until_started();
    drop(draw_future);
    gate.release();

    wait_until_surface_restored(&surface_view).await;
    assert!(
        matches!(*surface_view.surface_slot_lock(), SurfaceSlot::Idle(_)),
        "the ownership guard restored the session after the caller dropped its future"
    );
    // The documented serialized-draw rule answers the next frame: it is
    // answered rather than refused with "a frame draw is in flight".
    let statistics = surface_view
        .draw(test_draw_camera())
        .await
        .expect("the next draw is not refused after the cancelled one");
    assert_eq!(statistics.requested_tile_count, 0);
    assert_eq!(
        rendered_frame_count.load(Ordering::SeqCst),
        2,
        "both the cancelled caller's frame and the following draw rendered"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_panicking_frame_work_leaves_the_slot_recoverable() {
    let surface_view = overlay_view();
    let gate = TestFrameGate::new();
    let rendered_frame_count = Arc::new(AtomicUsize::new(0));
    install_test_session(
        &surface_view,
        TestFrameBehavior::Panic,
        Some(Arc::clone(&gate)),
        Arc::clone(&rendered_frame_count),
    );

    let mut draw_future = Box::pin(surface_view.draw(test_draw_camera()));
    assert!(matches!(poll_once(draw_future.as_mut()), Poll::Pending));
    gate.wait_until_started();
    drop(draw_future);
    gate.release();

    wait_until_surface_restored(&surface_view).await;
    assert!(
        matches!(*surface_view.surface_slot_lock(), SurfaceSlot::Idle(_)),
        "a panicking frame unwinds through the ownership guard, which restores the session"
    );
    let recovered_session = surface_view
        .take_surface_session()
        .expect("the slot is recoverable after the panic instead of staying busy");
    surface_view
        .frame_shared_state()
        .restore_surface_session(recovered_session);
}

#[tokio::test]
async fn a_vector_style_frame_produces_the_demo_place_labels() {
    let provider: Arc<dyn VectorTileProvider> = Arc::new(CommittedVectorTileProvider {
        tile_bytes: DEMO_VECTOR_TILE_BYTES.to_vec(),
    });
    let mut scheduler = VectorBaseScheduler::from_style_id(provider, VECTOR_STYLE_ID_BRIGHT_LIGHT)
        .expect("the committed Bright style loads");
    let vector_base_frame = scheduler
        .resolve_frame(&[DEMO_VECTOR_TILE_KEY], DEMO_FRAME_ZOOM_LEVEL)
        .await;
    assert_eq!(vector_base_frame.failed_tile_count, 0);
    assert!(
        vector_base_frame
            .label_candidates
            .iter()
            .any(|candidate| candidate.text == "Sacramento"),
        "the resolved frame must carry the demo tile's place labels"
    );

    // The same layer the draw seam forwards to the label-aware renderer:
    // the style-derived candidates placed through the embedded font.
    let native_camera = RenderingMapCamera::new(
        NativeGeographicCoordinate::from_degrees(TEST_LATITUDE_DEGREES, TEST_LONGITUDE_DEGREES)
            .expect("the demo camera coordinate is canonical"),
        DEMO_FRAME_ZOOM_LEVEL,
        0.0,
        0.0,
    );
    let viewport = ViewportPixels::new(
        LABEL_TEST_VIEWPORT_EDGE_PIXELS,
        LABEL_TEST_VIEWPORT_EDGE_PIXELS,
    )
    .expect("the label test viewport is non-zero");
    let label_layer = vector_base_frame
        .label_layer(&native_camera, &viewport, DEFAULT_FIELD_OF_VIEW_DEGREES)
        .expect("a vector-style frame with place names yields a label layer");
    assert!(
        label_layer
            .labels()
            .iter()
            .any(|label| label.text == "Sacramento"),
        "the draw seam's label layer places the demo place name"
    );
    assert!(
        !label_layer.quad_vertices(&viewport).text.is_empty(),
        "the default font rasterizes the placed labels into glyph quads"
    );
}

/// Experiments read the stored layer; the production surface keeps it
/// private, so the test module reaches the same field.
fn stored_location_indicator(
    surface_view: &MapView,
) -> Option<rustic_maps_rendering::LocationIndicatorLayer> {
    surface_view.location_indicator.lock().unwrap().clone()
}

#[tokio::test]
async fn setting_the_device_pixel_ratio_scales_the_forwarded_label_layer() {
    let provider: Arc<dyn VectorTileProvider> = Arc::new(CommittedVectorTileProvider {
        tile_bytes: DEMO_VECTOR_TILE_BYTES.to_vec(),
    });
    let mut scheduler = VectorBaseScheduler::from_style_id(provider, VECTOR_STYLE_ID_BRIGHT_LIGHT)
        .expect("the committed Bright style loads");
    let vector_base_frame = scheduler
        .resolve_frame(&[DEMO_VECTOR_TILE_KEY], DEMO_FRAME_ZOOM_LEVEL)
        .await;
    let native_camera = RenderingMapCamera::new(
        NativeGeographicCoordinate::from_degrees(TEST_LATITUDE_DEGREES, TEST_LONGITUDE_DEGREES)
            .expect("the demo camera coordinate is canonical"),
        DEMO_FRAME_ZOOM_LEVEL,
        0.0,
        0.0,
    );
    let viewport = ViewportPixels::new(
        LABEL_TEST_VIEWPORT_EDGE_PIXELS,
        LABEL_TEST_VIEWPORT_EDGE_PIXELS,
    )
    .expect("the label test viewport is non-zero");
    let point_sized_layer = vector_base_frame
        .label_layer_with_device_pixel_ratio(
            &native_camera,
            &viewport,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
            1.0,
        )
        .expect("a vector-style frame with place names yields a label layer");
    let point_sized_label = point_sized_layer
        .labels()
        .iter()
        .find(|label| label.text == "Sacramento")
        .expect("the demo place name is placed at the point size");

    let surface_view = overlay_view();
    let device_pixel_ratio = 3.0;
    surface_view
        .set_device_pixel_ratio(device_pixel_ratio)
        .expect("a finite positive ratio validates");
    // The draw seam reads the same stored ratio to build the layer.
    let device_sized_layer = vector_base_frame
        .label_layer_with_device_pixel_ratio(
            &native_camera,
            &viewport,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
            surface_view.stored_device_pixel_ratio(),
        )
        .expect("the device-scaled frame yields a label layer");
    let device_sized_label = device_sized_layer
        .labels()
        .iter()
        .find(|label| label.text == "Sacramento")
        .expect("the demo place name is placed at the device size");
    assert_eq!(
        device_sized_label.text_size_pixels,
        point_sized_label.text_size_pixels * device_pixel_ratio,
        "the stored ratio scales the placed glyph size"
    );
    assert_eq!(
        device_sized_label.halo_width_pixels,
        point_sized_label.halo_width_pixels * device_pixel_ratio,
        "the stored ratio scales the halo width with the glyphs"
    );
}

#[test]
fn an_invalid_device_pixel_ratio_is_refused_and_keeps_the_previous_one() {
    let surface_view = overlay_view();
    surface_view
        .set_device_pixel_ratio(2.0)
        .expect("a finite positive ratio validates");
    for invalid_ratio in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
        let failure = surface_view
            .set_device_pixel_ratio(invalid_ratio)
            .expect_err("a non-finite or non-positive ratio is refused");
        assert!(
            matches!(failure, SdkError::InvalidRenderInput { .. }),
            "the refusal is the typed invalid-input error, was {failure:?}"
        );
    }
    assert_eq!(
        surface_view.stored_device_pixel_ratio(),
        2.0,
        "a refused ratio leaves the previous one in force"
    );
}

fn tracking_dot_indicator_state() -> LocationIndicatorState {
    LocationIndicatorState {
        coordinate: coordinate(TEST_LATITUDE_DEGREES, TEST_LONGITUDE_DEGREES),
        course_heading_degrees: None,
        horizontal_accuracy_meters: Some(TEST_INDICATOR_ACCURACY_METERS),
    }
}

fn tracking_dot_indicator_style() -> LocationIndicatorStyle {
    LocationIndicatorStyle {
        visual_kind: LocationIndicatorVisualKind::TrackingDot,
        radius_pixels: None,
        rgba_color: None,
        ring_rgba_color: None,
        halo_rgba_color: None,
        show_accuracy_ring: None,
    }
}

#[test]
fn a_valid_location_indicator_state_stores_into_the_layer() {
    let surface_view = overlay_view();
    surface_view
        .set_location_indicator(
            tracking_dot_indicator_state(),
            tracking_dot_indicator_style(),
        )
        .expect("a canonical fix with default style validates");
    let stored_layer =
        stored_location_indicator(&surface_view).expect("the validated layer stores");
    assert_eq!(
        stored_layer.state.coordinate.latitude,
        TEST_LATITUDE_DEGREES
    );
    assert_eq!(
        stored_layer.state.horizontal_accuracy_meters,
        Some(TEST_INDICATOR_ACCURACY_METERS)
    );
    match stored_layer.style.visual_kind {
        rustic_maps_rendering::LocationIndicatorVisualKind::TrackingDot => {}
        other => panic!("the visual kind converted wrong: {other:?}"),
    }
    // The null style fields keep the documented rustic-maps-rendering defaults.
    assert_eq!(stored_layer.style.radius_pixels, None);
    assert_eq!(stored_layer.style.rgba_color, None);
    assert_eq!(stored_layer.style.show_accuracy_ring, None);
}

#[test]
fn an_invalid_indicator_coordinate_refuses_and_keeps_the_earlier_layer() {
    let surface_view = overlay_view();
    surface_view
        .set_location_indicator(
            tracking_dot_indicator_state(),
            tracking_dot_indicator_style(),
        )
        .expect("the first fix validates");
    let mut out_of_range_state = tracking_dot_indicator_state();
    out_of_range_state.coordinate = coordinate(INVALID_LATITUDE_DEGREES, TEST_LONGITUDE_DEGREES);
    let failure = surface_view
        .set_location_indicator(out_of_range_state, tracking_dot_indicator_style())
        .expect_err("an out-of-range fix coordinate must refuse");
    assert!(
        matches!(failure, SdkError::InvalidCoordinateInput { .. }),
        "the coordinate boundary owns the refusal: {failure}"
    );
    assert!(
        stored_location_indicator(&surface_view).is_some(),
        "the earlier indicator stays in force"
    );
}

#[test]
fn an_invalid_indicator_style_refuses_and_keeps_the_earlier_layer() {
    let surface_view = overlay_view();
    surface_view
        .set_location_indicator(
            tracking_dot_indicator_state(),
            tracking_dot_indicator_style(),
        )
        .expect("the first style validates");
    let mut invalid_style = tracking_dot_indicator_style();
    invalid_style.rgba_color = Some(OverlayColor {
        red: 1.0,
        green: 1.5,
        blue: 0.0,
        alpha: 1.0,
    });
    let failure = surface_view
        .set_location_indicator(tracking_dot_indicator_state(), invalid_style)
        .expect_err("a color channel outside [0, 1] must refuse");
    assert_refusal_contains(
        failure,
        "location indicator color channel green 1.5 outside [0, 1]",
    );
    let mut non_finite_radius_style = tracking_dot_indicator_style();
    non_finite_radius_style.radius_pixels = Some(0.0);
    let radius_failure = surface_view
        .set_location_indicator(tracking_dot_indicator_state(), non_finite_radius_style)
        .expect_err("a non-positive disc radius must refuse");
    assert_refusal_contains(
        radius_failure,
        "location indicator disc radius must be positive and finite",
    );
    assert!(
        stored_location_indicator(&surface_view).is_some(),
        "the earlier indicator stays in force"
    );
}

#[test]
fn clear_location_indicator_empties_the_layer() {
    let surface_view = overlay_view();
    surface_view
        .set_location_indicator(
            tracking_dot_indicator_state(),
            tracking_dot_indicator_style(),
        )
        .expect("the layer validates");
    surface_view.clear_location_indicator();
    assert!(stored_location_indicator(&surface_view).is_none());
}

#[test]
fn a_detached_view_refuses_its_settle_check() {
    let surface_view = overlay_view();
    let camera = MapCamera::new(
        coordinate(TEST_LATITUDE_DEGREES, TEST_LONGITUDE_DEGREES),
        12.0,
        0.0,
        0.0,
    )
    .expect("a canonical camera validates");
    let failure = surface_view
        .is_frame_settled(Arc::new(camera))
        .expect_err("a detached view must refuse its settle check");
    assert_refusal_contains(failure, "the map surface view is detached");
}

/// Attribution line the base-map switch tests install.
const SWITCH_TEST_ATTRIBUTION_TEXT: &str = "Switch test vector tiles";

/// A well-formed vector base source pointing at a neutral test tileset;
/// the scheduler built from it never fetches in these tests.
fn switch_test_vector_source(source_name: &str) -> UrlTemplateVectorTileSource {
    UrlTemplateVectorTileSource {
        url_template: "https://tiles.example.com/{z}/{x}/{y}.pbf".to_string(),
        source_name: source_name.to_string(),
        attribution_text: SWITCH_TEST_ATTRIBUTION_TEXT.to_string(),
        style_id: VECTOR_STYLE_ID_BRIGHT_LIGHT.to_string(),
        minimum_zoom_level: 0,
        maximum_zoom_level: 14,
        token_bucket_capacity_tokens: None,
        refill_tokens_per_second: None,
        terms_text: None,
        subdomains: None,
        headers: None,
        style_hints: None,
    }
}

/// A view over an SDK whose vector-tile slot serves the one committed
/// demo tile, registered under `provider_key` the way the catalog's
/// install records a configured provider.
fn provider_backed_view(provider_key: &str) -> MapView {
    let map_sdk = Arc::new(MapSdk::new());
    map_sdk.register_vector_tile_provider(Arc::new(CommittedVectorTileProvider {
        tile_bytes: DEMO_VECTOR_TILE_BYTES.to_vec(),
    }));
    map_sdk.record_provider_selection(
        crate::records::ProviderContract::VectorTiles,
        provider_key,
        "Test Provider",
    );
    MapView::new(map_sdk)
}

#[test]
fn a_provider_backed_base_takes_its_descriptor_from_the_provider() {
    let surface_view = provider_backed_view("committed-test-provider");
    surface_view
        .set_provider_vector_tile_source(ProviderVectorTileSource {
            provider_key: "committed-test-provider".to_string(),
            style_id: VECTOR_STYLE_ID_BRIGHT_LIGHT.to_string(),
        })
        .expect("the registered provider serves the committed style");
    let status = surface_view.vector_tile_source_status();
    assert!(status.is_configured);
    assert_eq!(status.style_id, VECTOR_STYLE_ID_BRIGHT_LIGHT);
    assert_eq!(status.source_name, "committed-demo-tile");
    assert_eq!(
        status.attribution_text, "Map data © OpenStreetMap contributors",
        "the provider's descriptor supplies the attribution line"
    );
    assert_eq!(status.maximum_zoom_level, 14);
}

/// A foreign vector-tile implementation serving the committed demo tile.
struct CommittedForeignVectorTileProvider {
    tile_bytes: Vec<u8>,
}

#[async_trait]
impl crate::foreign_providers::VectorTileProvider for CommittedForeignVectorTileProvider {
    fn source_descriptor(&self) -> crate::records::VectorTileSourceDescriptor {
        crate::records::VectorTileSourceDescriptor {
            source_name: "committed-demo-tile".to_string(),
            attribution_text: "Map data © OpenStreetMap contributors".to_string(),
            minimum_zoom_level: 0,
            maximum_zoom_level: 14,
            tile_encoding: crate::records::VectorTileEncoding::MapboxVectorTile,
            style_hints: vec!["openmaptiles".to_string()],
        }
    }

    async fn fetch_vector_tile(
        &self,
        tile_x: u32,
        tile_y: u32,
        zoom_level: u8,
    ) -> Result<crate::records::VectorTileData, SdkError> {
        Ok(crate::records::VectorTileData {
            tile_x,
            tile_y,
            zoom_level,
            tile_encoding: crate::records::VectorTileEncoding::MapboxVectorTile,
            tile_bytes: self.tile_bytes.clone(),
        })
    }
}

#[test]
fn a_foreign_registered_vector_provider_installs_under_the_application_key() {
    let map_sdk = Arc::new(MapSdk::new());
    map_sdk.register_vector_tile_provider_from_foreign(Arc::new(
        CommittedForeignVectorTileProvider {
            tile_bytes: DEMO_VECTOR_TILE_BYTES.to_vec(),
        },
    ));
    assert!(map_sdk.vector_tile_provider_is_registered());
    let surface_view = MapView::new(map_sdk);
    surface_view
        .set_provider_vector_tile_source(ProviderVectorTileSource {
            provider_key: crate::provider::APPLICATION_PROVIDER_KEY.to_string(),
            style_id: VECTOR_STYLE_ID_BRIGHT_LIGHT.to_string(),
        })
        .expect("the application-registered provider resolves under the application key");
    let status = surface_view.vector_tile_source_status();
    assert!(status.is_configured);
    assert_eq!(status.source_name, "committed-demo-tile");
    assert_eq!(
        status.attribution_text, "Map data © OpenStreetMap contributors",
        "the foreign provider's descriptor supplies the attribution line"
    );
    assert_eq!(status.maximum_zoom_level, 14);
}

#[test]
fn a_provider_backed_base_refuses_an_unknown_style_and_keeps_the_installed_one() {
    let surface_view = provider_backed_view("committed-test-provider");
    surface_view
        .set_provider_vector_tile_source(ProviderVectorTileSource {
            provider_key: "committed-test-provider".to_string(),
            style_id: VECTOR_STYLE_ID_BRIGHT_LIGHT.to_string(),
        })
        .expect("the first source installs");
    let failure = surface_view
        .set_provider_vector_tile_source(ProviderVectorTileSource {
            provider_key: "committed-test-provider".to_string(),
            style_id: "no-such-committed-style".to_string(),
        })
        .expect_err("an unknown style must refuse");
    assert_refusal_contains(failure, "names a style the SDK does not carry");
    assert_eq!(
        surface_view.vector_tile_source_status().style_id,
        VECTOR_STYLE_ID_BRIGHT_LIGHT,
        "the refused install leaves the earlier base in force"
    );
}

#[test]
fn a_provider_backed_base_refuses_a_key_the_installed_provider_does_not_serve() {
    let surface_view = provider_backed_view("installed-vector-provider");
    let failure = surface_view
        .set_provider_vector_tile_source(ProviderVectorTileSource {
            provider_key: "another-vector-provider".to_string(),
            style_id: VECTOR_STYLE_ID_BRIGHT_LIGHT.to_string(),
        })
        .expect_err("a provider key the vector slot does not serve must refuse");
    assert_refusal_contains(
        failure,
        "the installed vector-tile provider is installed-vector-provider",
    );
    assert!(!surface_view.vector_tile_source_status().is_configured);
}

#[test]
fn a_provider_backed_base_refuses_when_no_vector_provider_is_registered() {
    let surface_view = MapView::new(Arc::new(MapSdk::new()));
    let failure = surface_view
        .set_provider_vector_tile_source(ProviderVectorTileSource {
            provider_key: "committed-test-provider".to_string(),
            style_id: VECTOR_STYLE_ID_BRIGHT_LIGHT.to_string(),
        })
        .expect_err("no registered vector provider must refuse");
    assert!(
        matches!(failure, SdkError::ProviderNotConfigured { .. }),
        "the registry answers the not-configured failure: {failure}"
    );
    assert!(!surface_view.vector_tile_source_status().is_configured);
}

/// The provider-published style document the document tests serve: the
/// committed synthetic OMV document, which compiles.
const PROVIDER_STYLE_DOCUMENT_JSON: &str =
    include_str!("../../../../fixtures/here/vendor_style_document.json");

/// The address the document tests ask the provider for.
const PROVIDER_STYLE_DOCUMENT_URL: &str = "https://styles.example.com/explore-day.json";

/// Serves one style document, counting the requests it answers. A request
/// for anything besides the document address — the document's own glyph
/// and sprite resources — fails, so the install keeps its embedded font
/// and draws no icons, exactly as a family whose resources are
/// unreachable does.
struct CommittedStyleResourceClient {
    style_document: String,
    fetch_count: Arc<AtomicUsize>,
}

#[async_trait]
impl rustic_maps_core::StyleResourceClient for CommittedStyleResourceClient {
    async fn fetch_style_resource(&self, url: &str) -> Result<Vec<u8>, NativeSdkError> {
        if url != PROVIDER_STYLE_DOCUMENT_URL {
            return Err(NativeSdkError::NetworkRequestFailed(format!(
                "the style resource {url} is outside what this client serves"
            )));
        }
        self.fetch_count.fetch_add(1, Ordering::SeqCst);
        Ok(self.style_document.clone().into_bytes())
    }
}

/// A view over an SDK whose vector slot serves the committed demo tile and
/// which registers one authenticated style-resource fetch answering
/// `style_document`, counting the requests it answers.
fn document_backed_view(provider_key: &str, style_document: &str) -> (MapView, Arc<AtomicUsize>) {
    let fetch_count = Arc::new(AtomicUsize::new(0));
    let map_sdk = Arc::new(MapSdk::new());
    map_sdk.register_vector_tile_provider(Arc::new(CommittedVectorTileProvider {
        tile_bytes: DEMO_VECTOR_TILE_BYTES.to_vec(),
    }));
    map_sdk.register_vector_tile_style_resources(
        Arc::new(CommittedStyleResourceClient {
            style_document: style_document.to_string(),
            fetch_count: Arc::clone(&fetch_count),
        }),
        Vec::new(),
    );
    map_sdk.record_provider_selection(
        crate::records::ProviderContract::VectorTiles,
        provider_key,
        "Test Provider",
    );
    (MapView::new(map_sdk), fetch_count)
}

#[tokio::test]
async fn a_provider_backed_base_paints_the_providers_own_style_document() {
    let (surface_view, fetch_count) =
        document_backed_view("document-test-provider", PROVIDER_STYLE_DOCUMENT_JSON);
    surface_view
        .set_provider_vector_tile_source_from_document(
            ProviderVectorTileSource {
                provider_key: "document-test-provider".to_string(),
                style_id: "explore-day".to_string(),
            },
            PROVIDER_STYLE_DOCUMENT_URL.to_string(),
        )
        .await
        .expect("the provider's published document installs");
    let status = surface_view.vector_tile_source_status();
    assert!(status.is_configured);
    assert_eq!(status.style_id, "explore-day");
    assert_eq!(status.source_name, "committed-demo-tile");
    assert_eq!(
        status.attribution_text, "Map data © OpenStreetMap contributors",
        "the provider's descriptor supplies the attribution line"
    );
    assert_eq!(status.maximum_zoom_level, 14);
    assert_eq!(
        status.unsupported_property_count, 0,
        "the schema-matched committed document applies every property it declares"
    );
    assert_eq!(status.unsupported_property_names, Vec::<String>::new());
    assert!(
        status
            .degraded_asset_names
            .iter()
            .any(|name| name.starts_with("sprite:")),
        "the declared sprite the client cannot fetch is named: {:?}",
        status.degraded_asset_names
    );
    assert!(
        status
            .degraded_asset_names
            .iter()
            .any(|name| name.starts_with("glyphs:")),
        "the declared glyph endpoint the client cannot serve is named: {:?}",
        status.degraded_asset_names
    );
    assert!(
        status.source_layer_counts.is_empty(),
        "no frame has resolved yet, so no per-source counts are reported"
    );
    assert_eq!(
        fetch_count.load(Ordering::SeqCst),
        1,
        "one install fetches the document once"
    );
}

#[tokio::test]
async fn a_document_naming_an_unserved_tileset_is_refused_by_name() {
    let mismatched_document = r#"{
        "version": 8,
        "sources": {
            "base": { "type": "vector", "url": "mapbox://mapbox.mapbox-terrain-v2" }
        },
        "layers": [
            { "id": "water", "type": "fill", "source": "base", "source-layer": "water" }
        ]
    }"#;
    let (surface_view, _) = document_backed_view("document-test-provider", mismatched_document);
    let failure = match surface_view
        .set_provider_vector_tile_source_from_document(
            ProviderVectorTileSource {
                provider_key: "document-test-provider".to_string(),
                style_id: "explore-day".to_string(),
            },
            PROVIDER_STYLE_DOCUMENT_URL.to_string(),
        )
        .await
    {
        Ok(()) => panic!("a source naming another tileset must refuse"),
        Err(failure) => failure,
    };
    let detail = failure.to_string();
    assert!(
        detail.contains("mapbox.mapbox-terrain-v2"),
        "the refusal names the declared address: {detail}"
    );
    assert!(
        detail.contains("committed-demo-tile"),
        "the refusal names the served tileset: {detail}"
    );
    assert!(
        !surface_view.vector_tile_source_status().is_configured,
        "the refused install leaves no base in force"
    );
}

#[tokio::test]
async fn a_cached_style_document_is_not_fetched_again() {
    let (surface_view, fetch_count) =
        document_backed_view("document-test-provider", PROVIDER_STYLE_DOCUMENT_JSON);
    for _ in 0..2 {
        surface_view
            .set_provider_vector_tile_source_from_document(
                ProviderVectorTileSource {
                    provider_key: "document-test-provider".to_string(),
                    style_id: "explore-day".to_string(),
                },
                PROVIDER_STYLE_DOCUMENT_URL.to_string(),
            )
            .await
            .expect("the provider's published document installs");
    }
    assert_eq!(
        fetch_count.load(Ordering::SeqCst),
        1,
        "a second install of the same provider and address reuses the document"
    );
}

#[tokio::test]
async fn a_malformed_style_document_refuses_and_keeps_the_installed_base() {
    let (surface_view, _) =
        document_backed_view("document-test-provider", "{ not a style document }");
    surface_view
        .set_provider_vector_tile_source(ProviderVectorTileSource {
            provider_key: "document-test-provider".to_string(),
            style_id: VECTOR_STYLE_ID_BRIGHT_LIGHT.to_string(),
        })
        .expect("a committed style installs first");
    let failure = surface_view
        .set_provider_vector_tile_source_from_document(
            ProviderVectorTileSource {
                provider_key: "document-test-provider".to_string(),
                style_id: "explore-day".to_string(),
            },
            PROVIDER_STYLE_DOCUMENT_URL.to_string(),
        )
        .await
        .expect_err("a malformed document must refuse");
    assert!(
        matches!(failure, SdkError::ResponseParsingFailure { .. }),
        "the document parse failure is reported: {failure}"
    );
    assert_eq!(
        surface_view.vector_tile_source_status().style_id,
        VECTOR_STYLE_ID_BRIGHT_LIGHT,
        "the refused install leaves the earlier base in force"
    );
}

#[tokio::test]
async fn installing_a_document_refuses_when_the_provider_registers_no_style_resources() {
    let surface_view = provider_backed_view("committed-test-provider");
    let failure = surface_view
        .set_provider_vector_tile_source_from_document(
            ProviderVectorTileSource {
                provider_key: "committed-test-provider".to_string(),
                style_id: "explore-day".to_string(),
            },
            PROVIDER_STYLE_DOCUMENT_URL.to_string(),
        )
        .await
        .expect_err("a provider without a style-resource fetch must refuse");
    assert_refusal_contains(failure, "vector-tile style resources");
    assert!(!surface_view.vector_tile_source_status().is_configured);
}

/// The document the label-font tests install: one symbol layer naming one
/// font stack, and a glyph endpoint on the document's own origin.
const GLYPH_STYLE_DOCUMENT_JSON: &str = r#"{
    "version": 8,
    "glyphs": "https://styles.example.com/glyphs/{fontstack}/{range}.pbf",
    "sources": { "base": { "type": "vector" } },
    "layers": [
        {
            "id": "place-labels",
            "type": "symbol",
            "source": "base",
            "source-layer": "place",
            "layout": {
                "text-field": ["get", "name"],
                "text-font": ["Noto Sans Regular"]
            }
        }
    ]
}"#;

/// The same document with its glyph endpoint on another origin.
const OFF_ORIGIN_GLYPH_STYLE_DOCUMENT_JSON: &str = r#"{
    "version": 8,
    "glyphs": "https://glyphs.example.net/{fontstack}/{range}.pbf",
    "sources": { "base": { "type": "vector" } },
    "layers": [
        {
            "id": "place-labels",
            "type": "symbol",
            "source": "base",
            "source-layer": "place",
            "layout": {
                "text-field": ["get", "name"],
                "text-font": ["Noto Sans Regular"]
            }
        }
    ]
}"#;

/// The same document naming its glyphs in a scheme of its own, the way a
/// vendor style API writes its resource references.
const VENDOR_SCHEME_GLYPH_STYLE_DOCUMENT_JSON: &str = r#"{
    "version": 8,
    "glyphs": "vendorscheme://fonts/{fontstack}/{range}.pbf",
    "sources": { "base": { "type": "vector" } },
    "layers": [
        {
            "id": "place-labels",
            "type": "symbol",
            "source": "base",
            "source-layer": "place",
            "layout": {
                "text-field": ["get", "name"],
                "text-font": ["Noto Sans Regular"]
            }
        }
    ]
}"#;

/// The same document with its glyph endpoint on a host the provider
/// declares for the style resources it publishes.
const DECLARED_HOST_GLYPH_STYLE_DOCUMENT_JSON: &str = r#"{
    "version": 8,
    "glyphs": "https://assets.example.test/fonts/{fontstack}/{range}.pbf",
    "sources": { "base": { "type": "vector" } },
    "layers": [
        {
            "id": "place-labels",
            "type": "symbol",
            "source": "base",
            "source-layer": "place",
            "layout": {
                "text-field": ["get", "name"],
                "text-font": ["Noto Sans Regular"]
            }
        }
    ]
}"#;

/// The committed Basic Latin range the glyph requests answer with.
const BASIC_LATIN_GLYPH_RANGE_BYTES: &[u8] =
    include_bytes!("../../../../fixtures/glyphs/noto_sans_regular_0-255.pbf");
/// The committed second Latin block the glyph requests answer with.
const LATIN_EXTENDED_GLYPH_RANGE_BYTES: &[u8] =
    include_bytes!("../../../../fixtures/glyphs/noto_sans_regular_256-511.pbf");
/// The URL segment the protocol names the second Latin block by; the
/// address a request for that block ends with.
const LATIN_EXTENDED_URL_SUFFIX: &str = "/256-511.pbf";

/// Serves one style document for the document address and the committed
/// glyph range each other address names, recording every address.
struct GlyphServingStyleResourceClient {
    style_document: String,
    requested_urls: Mutex<Vec<String>>,
    refuse_glyph_requests: bool,
}

impl GlyphServingStyleResourceClient {
    fn requested_urls(&self) -> Vec<String> {
        self.requested_urls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

#[async_trait]
impl rustic_maps_core::StyleResourceClient for GlyphServingStyleResourceClient {
    async fn fetch_style_resource(&self, url: &str) -> Result<Vec<u8>, NativeSdkError> {
        self.requested_urls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(url.to_string());
        if url.ends_with(".json") {
            return Ok(self.style_document.clone().into_bytes());
        }
        if self.refuse_glyph_requests {
            return Err(NativeSdkError::NetworkRequestFailed(format!(
                "the glyph range {url} is unreachable"
            )));
        }
        Ok(if url.ends_with(LATIN_EXTENDED_URL_SUFFIX) {
            LATIN_EXTENDED_GLYPH_RANGE_BYTES.to_vec()
        } else {
            BASIC_LATIN_GLYPH_RANGE_BYTES.to_vec()
        })
    }
}

/// A view whose vector slot serves the committed demo tile, whose
/// style-resource fetch answers `style_document` for the document address
/// and the committed glyph ranges for every other address, and whose
/// registration declares `declared_hosts` for those resources.
fn glyph_backed_view(
    provider_key: &str,
    style_document: &str,
    refuse_glyph_requests: bool,
    declared_hosts: Vec<String>,
) -> (MapView, Arc<GlyphServingStyleResourceClient>) {
    let style_resources = Arc::new(GlyphServingStyleResourceClient {
        style_document: style_document.to_string(),
        requested_urls: Mutex::new(Vec::new()),
        refuse_glyph_requests,
    });
    let map_sdk = Arc::new(MapSdk::new());
    map_sdk.register_vector_tile_provider(Arc::new(CommittedVectorTileProvider {
        tile_bytes: DEMO_VECTOR_TILE_BYTES.to_vec(),
    }));
    map_sdk.register_vector_tile_style_resources(
        Arc::clone(&style_resources) as Arc<dyn rustic_maps_core::StyleResourceClient>,
        declared_hosts,
    );
    map_sdk.record_provider_selection(
        crate::records::ProviderContract::VectorTiles,
        provider_key,
        "Test Provider",
    );
    (MapView::new(map_sdk), style_resources)
}

/// The font stack and both Latin blocks the glyph document's install
/// requests, in the order the install names them.
fn glyph_style_document_requests() -> Vec<String> {
    vec![
        PROVIDER_STYLE_DOCUMENT_URL.to_string(),
        "https://styles.example.com/glyphs/Noto%20Sans%20Regular/0-255.pbf".to_string(),
        "https://styles.example.com/glyphs/Noto%20Sans%20Regular/256-511.pbf".to_string(),
    ]
}

#[tokio::test]
async fn installing_a_document_fetches_its_glyph_ranges_and_hands_the_font_to_the_base() {
    let (surface_view, style_resources) = glyph_backed_view(
        "document-test-provider",
        GLYPH_STYLE_DOCUMENT_JSON,
        false,
        Vec::new(),
    );
    surface_view
        .set_provider_vector_tile_source_from_document(
            ProviderVectorTileSource {
                provider_key: "document-test-provider".to_string(),
                style_id: "explore-day".to_string(),
            },
            PROVIDER_STYLE_DOCUMENT_URL.to_string(),
        )
        .await
        .expect("a document whose glyph endpoint serves installs");
    assert_eq!(
        style_resources.requested_urls(),
        glyph_style_document_requests(),
        "the install names the escaped font stack and both Latin blocks, after the document"
    );
    assert!(surface_view.vector_tile_source_status().is_configured);
    let installed_scheduler = surface_view
        .take_vector_base_scheduler()
        .expect("the install wrote the scheduler");
    assert!(
        installed_scheduler.label_font().is_some(),
        "the style's own glyph ranges reach the label pass"
    );
}

#[tokio::test]
async fn a_second_install_of_the_same_document_costs_no_glyph_request() {
    let (surface_view, style_resources) = glyph_backed_view(
        "document-test-provider",
        GLYPH_STYLE_DOCUMENT_JSON,
        false,
        Vec::new(),
    );
    for _ in 0..2 {
        surface_view
            .set_provider_vector_tile_source_from_document(
                ProviderVectorTileSource {
                    provider_key: "document-test-provider".to_string(),
                    style_id: "explore-day".to_string(),
                },
                PROVIDER_STYLE_DOCUMENT_URL.to_string(),
            )
            .await
            .expect("the document installs");
    }
    assert_eq!(
        style_resources.requested_urls(),
        glyph_style_document_requests(),
        "the document and both ranges are fetched once across two installs"
    );
}

#[tokio::test]
async fn a_glyph_address_outside_the_documents_origin_refuses_the_install() {
    let (surface_view, style_resources) = glyph_backed_view(
        "document-test-provider",
        OFF_ORIGIN_GLYPH_STYLE_DOCUMENT_JSON,
        false,
        // A declared host other than the one the document names: the
        // unlisted endpoint still receives no request at all.
        vec!["assets.example.test".to_string()],
    );
    let failure = surface_view
        .set_provider_vector_tile_source_from_document(
            ProviderVectorTileSource {
                provider_key: "document-test-provider".to_string(),
                style_id: "explore-day".to_string(),
            },
            PROVIDER_STYLE_DOCUMENT_URL.to_string(),
        )
        .await
        .expect_err("an off-origin glyph address must refuse");
    assert!(
        matches!(failure, SdkError::ProviderRequestRejected { .. }),
        "the refusal keeps its own variant: {failure}"
    );
    assert_eq!(
        style_resources.requested_urls(),
        vec![PROVIDER_STYLE_DOCUMENT_URL.to_string()],
        "the refusal happens before any glyph request"
    );
    assert!(!surface_view.vector_tile_source_status().is_configured);
}

#[tokio::test]
async fn a_glyph_address_in_a_vendor_scheme_keeps_the_embedded_font() {
    let (surface_view, style_resources) = glyph_backed_view(
        "document-test-provider",
        VENDOR_SCHEME_GLYPH_STYLE_DOCUMENT_JSON,
        false,
        Vec::new(),
    );
    surface_view
        .set_provider_vector_tile_source_from_document(
            ProviderVectorTileSource {
                provider_key: "document-test-provider".to_string(),
                style_id: "explore-day".to_string(),
            },
            PROVIDER_STYLE_DOCUMENT_URL.to_string(),
        )
        .await
        .expect("a glyph address the SDK cannot fetch does not refuse the base");
    assert_eq!(
        style_resources.requested_urls(),
        vec![PROVIDER_STYLE_DOCUMENT_URL.to_string()],
        "an unfetchable glyph address names no request"
    );
    let installed_scheduler = surface_view
        .take_vector_base_scheduler()
        .expect("the install wrote the scheduler");
    assert!(
        installed_scheduler.label_font().is_none(),
        "the labels keep the SDK's embedded Latin range"
    );
}

/// A document naming no glyph endpoint and no sprite: its labels keep the
/// SDK's embedded Latin range and its base draws without icons.
const RESOURCELESS_STYLE_DOCUMENT_JSON: &str = r##"{
    "version": 8,
    "sources": { "base": { "type": "vector" } },
    "layers": [
        {
            "id": "water",
            "type": "fill",
            "source": "base",
            "source-layer": "water",
            "paint": { "fill-color": "#ff0000" }
        }
    ]
}"##;

#[tokio::test]
async fn a_document_without_a_glyph_endpoint_keeps_the_embedded_font() {
    let (surface_view, style_resources) = glyph_backed_view(
        "document-test-provider",
        RESOURCELESS_STYLE_DOCUMENT_JSON,
        false,
        Vec::new(),
    );
    surface_view
        .set_provider_vector_tile_source_from_document(
            ProviderVectorTileSource {
                provider_key: "document-test-provider".to_string(),
                style_id: "explore-day".to_string(),
            },
            PROVIDER_STYLE_DOCUMENT_URL.to_string(),
        )
        .await
        .expect("a document without a glyph endpoint installs");
    assert_eq!(
        style_resources.requested_urls(),
        vec![PROVIDER_STYLE_DOCUMENT_URL.to_string()],
        "a document without a glyph endpoint requests no range"
    );
    let installed_scheduler = surface_view
        .take_vector_base_scheduler()
        .expect("the install wrote the scheduler");
    assert!(
        installed_scheduler.label_font().is_none(),
        "the labels keep the SDK's embedded Latin range"
    );
}

#[tokio::test]
async fn a_glyph_endpoint_on_a_declared_host_is_fetched() {
    let (surface_view, style_resources) = glyph_backed_view(
        "document-test-provider",
        DECLARED_HOST_GLYPH_STYLE_DOCUMENT_JSON,
        false,
        vec!["assets.example.test".to_string()],
    );
    surface_view
        .set_provider_vector_tile_source_from_document(
            ProviderVectorTileSource {
                provider_key: "document-test-provider".to_string(),
                style_id: "explore-day".to_string(),
            },
            PROVIDER_STYLE_DOCUMENT_URL.to_string(),
        )
        .await
        .expect("a glyph endpoint on a host the provider declared installs");
    assert_eq!(
        style_resources.requested_urls(),
        vec![
            PROVIDER_STYLE_DOCUMENT_URL.to_string(),
            "https://assets.example.test/fonts/Noto%20Sans%20Regular/0-255.pbf".to_string(),
            "https://assets.example.test/fonts/Noto%20Sans%20Regular/256-511.pbf".to_string(),
        ],
        "the declared host receives the escaped font stack and both Latin blocks"
    );
    let installed_scheduler = surface_view
        .take_vector_base_scheduler()
        .expect("the install wrote the scheduler");
    assert!(
        installed_scheduler.label_font().is_some(),
        "the declared host's ranges reach the label pass"
    );
}

#[tokio::test]
async fn a_reopened_view_serves_the_document_and_its_glyphs_from_the_disk_cache() {
    let cache_directory = std::env::temp_dir().join(format!(
        "rustic-maps-native-core-style-disk-cache-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&cache_directory);
    let response_store = rustic_maps_cache::SqliteResponseCache::open(
        rustic_maps_cache::SqliteResponseCacheConfiguration {
            database_path: cache_directory.join("tiles.sqlite"),
            ..Default::default()
        },
    )
    .expect("the test tile cache opens");
    let style_resources = Arc::new(GlyphServingStyleResourceClient {
        style_document: GLYPH_STYLE_DOCUMENT_JSON.to_string(),
        requested_urls: Mutex::new(Vec::new()),
        refuse_glyph_requests: false,
    });
    let map_sdk = Arc::new(MapSdk::new());
    map_sdk.register_vector_tile_provider(Arc::new(CommittedVectorTileProvider {
        tile_bytes: DEMO_VECTOR_TILE_BYTES.to_vec(),
    }));
    map_sdk.register_vector_tile_style_resources(
        Arc::clone(&style_resources) as Arc<dyn rustic_maps_core::StyleResourceClient>,
        Vec::new(),
    );
    map_sdk.install_tile_cache_for_testing(
        Arc::new(RefusingRasterTileProvider),
        response_store,
        rustic_maps_cache::ResponseCategory::RasterTiles.default_time_to_live_seconds(),
        None,
        None,
    );
    map_sdk.record_provider_selection(
        crate::records::ProviderContract::VectorTiles,
        "document-test-provider",
        "Test Provider",
    );

    // Two views over one installation: the second is the reopened
    // application, whose style document, glyph ranges and (here absent)
    // sprite would otherwise be requested again.
    for _ in 0..2 {
        let surface_view = MapView::new(Arc::clone(&map_sdk));
        surface_view
            .set_provider_vector_tile_source_from_document(
                ProviderVectorTileSource {
                    provider_key: "document-test-provider".to_string(),
                    style_id: "explore-day".to_string(),
                },
                PROVIDER_STYLE_DOCUMENT_URL.to_string(),
            )
            .await
            .expect("the document installs on both views");
        assert!(surface_view.vector_tile_source_status().is_configured);
    }
    assert_eq!(
        style_resources.requested_urls(),
        glyph_style_document_requests(),
        "the reopened view serves the document and both ranges from the disk cache"
    );
    std::fs::remove_dir_all(&cache_directory).expect("the test cache directory cleans up");
}

#[tokio::test]
async fn a_failed_glyph_fetch_keeps_the_embedded_font_and_the_install_stands() {
    let (surface_view, style_resources) = glyph_backed_view(
        "document-test-provider",
        GLYPH_STYLE_DOCUMENT_JSON,
        true,
        Vec::new(),
    );
    surface_view
        .set_provider_vector_tile_source_from_document(
            ProviderVectorTileSource {
                provider_key: "document-test-provider".to_string(),
                style_id: "explore-day".to_string(),
            },
            PROVIDER_STYLE_DOCUMENT_URL.to_string(),
        )
        .await
        .expect("an unreachable glyph range does not refuse the base");
    assert_eq!(
        style_resources.requested_urls().len(),
        2,
        "the install stops at the first range the provider cannot serve"
    );
    assert!(
        surface_view.vector_tile_source_status().is_configured,
        "the base installs and draws with the embedded range"
    );
    let installed_scheduler = surface_view
        .take_vector_base_scheduler()
        .expect("the install wrote the scheduler");
    assert!(installed_scheduler.label_font().is_none());
}

/// A document declaring a sprite base on its own origin and no glyph
/// endpoint.
const SPRITE_STYLE_DOCUMENT_JSON: &str = r##"{
    "version": 8,
    "sprite": "https://styles.example.com/sprite",
    "sources": { "base": { "type": "vector" } },
    "layers": [
        {
            "id": "water",
            "type": "fill",
            "source": "base",
            "source-layer": "water",
            "paint": { "fill-color": "#ff0000" }
        }
    ]
}"##;

/// The same document with its sprite base on another origin.
const OFF_ORIGIN_SPRITE_STYLE_DOCUMENT_JSON: &str = r##"{
    "version": 8,
    "sprite": "https://sprites.example.net/sprite",
    "sources": { "base": { "type": "vector" } },
    "layers": [
        {
            "id": "water",
            "type": "fill",
            "source": "base",
            "source-layer": "water",
            "paint": { "fill-color": "#ff0000" }
        }
    ]
}"##;

/// The committed sprite index and sheet the two protocol addresses
/// answer with.
const SPRITE_INDEX_BYTES: &[u8] =
    include_bytes!("../../../rustic-maps-style/fixtures/sprites/icon-sprite.json");
const SPRITE_SHEET_BYTES: &[u8] =
    include_bytes!("../../../rustic-maps-style/fixtures/sprites/icon-sprite.png");

/// Serves one style document for the document address and the committed
/// sprite index and sheet for the protocol's two addresses, recording
/// every address.
struct SpriteServingStyleResourceClient {
    style_document: String,
    requested_urls: Mutex<Vec<String>>,
    refuse_sheet_requests: bool,
}

impl SpriteServingStyleResourceClient {
    fn requested_urls(&self) -> Vec<String> {
        self.requested_urls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

#[async_trait]
impl rustic_maps_core::StyleResourceClient for SpriteServingStyleResourceClient {
    async fn fetch_style_resource(&self, url: &str) -> Result<Vec<u8>, NativeSdkError> {
        self.requested_urls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(url.to_string());
        if url == PROVIDER_STYLE_DOCUMENT_URL {
            return Ok(self.style_document.clone().into_bytes());
        }
        if url.ends_with(rustic_maps_rendering::SPRITE_SHEET_URL_SUFFIX) {
            if self.refuse_sheet_requests {
                return Err(NativeSdkError::NetworkRequestFailed(format!(
                    "the sprite sheet {url} is unreachable"
                )));
            }
            return Ok(SPRITE_SHEET_BYTES.to_vec());
        }
        Ok(SPRITE_INDEX_BYTES.to_vec())
    }
}

/// A view whose vector slot serves the committed demo tile and whose
/// style-resource fetch answers `style_document` for the document address
/// and the committed sprite index and sheet for the protocol's addresses.
fn sprite_backed_view(
    style_document: &str,
    refuse_sheet_requests: bool,
) -> (MapView, Arc<SpriteServingStyleResourceClient>) {
    let style_resources = Arc::new(SpriteServingStyleResourceClient {
        style_document: style_document.to_string(),
        requested_urls: Mutex::new(Vec::new()),
        refuse_sheet_requests,
    });
    let map_sdk = Arc::new(MapSdk::new());
    map_sdk.register_vector_tile_provider(Arc::new(CommittedVectorTileProvider {
        tile_bytes: DEMO_VECTOR_TILE_BYTES.to_vec(),
    }));
    map_sdk.register_vector_tile_style_resources(
        Arc::clone(&style_resources) as Arc<dyn rustic_maps_core::StyleResourceClient>,
        // The sprite tests hold every address to the document's own
        // origin; the declared-host rule is pinned at the fetcher and in
        // the glyph install test.
        Vec::new(),
    );
    map_sdk.record_provider_selection(
        crate::records::ProviderContract::VectorTiles,
        "document-test-provider",
        "Test Provider",
    );
    (MapView::new(map_sdk), style_resources)
}

/// The document and both sprite addresses the sprite document's install
/// requests, in the order the install names them.
fn sprite_style_document_requests() -> Vec<String> {
    vec![
        PROVIDER_STYLE_DOCUMENT_URL.to_string(),
        "https://styles.example.com/sprite.json".to_string(),
        "https://styles.example.com/sprite.png".to_string(),
    ]
}

#[tokio::test]
async fn installing_a_document_fetches_its_sprite_once_and_hands_the_atlas_to_the_base() {
    let (surface_view, style_resources) = sprite_backed_view(SPRITE_STYLE_DOCUMENT_JSON, false);
    for _ in 0..2 {
        surface_view
            .set_provider_vector_tile_source_from_document(
                ProviderVectorTileSource {
                    provider_key: "document-test-provider".to_string(),
                    style_id: "explore-day".to_string(),
                },
                PROVIDER_STYLE_DOCUMENT_URL.to_string(),
            )
            .await
            .expect("a document whose sprite serves installs");
    }
    assert_eq!(
        style_resources.requested_urls(),
        sprite_style_document_requests(),
        "the index and sheet are fetched once, in that order, across two installs"
    );
    let installed_scheduler = surface_view
        .take_vector_base_scheduler()
        .expect("the install wrote the scheduler");
    let sprite_atlas = installed_scheduler
        .sprite_atlas()
        .expect("the style's own sprite reaches the base");
    assert!(
        sprite_atlas.icon("park").is_some(),
        "the committed index's icons are addressable"
    );
}

#[tokio::test]
async fn a_sprite_base_outside_the_documents_origin_refuses_the_install() {
    let (surface_view, style_resources) =
        sprite_backed_view(OFF_ORIGIN_SPRITE_STYLE_DOCUMENT_JSON, false);
    let failure = surface_view
        .set_provider_vector_tile_source_from_document(
            ProviderVectorTileSource {
                provider_key: "document-test-provider".to_string(),
                style_id: "explore-day".to_string(),
            },
            PROVIDER_STYLE_DOCUMENT_URL.to_string(),
        )
        .await
        .expect_err("an off-origin sprite base must refuse");
    assert!(
        matches!(failure, SdkError::ProviderRequestRejected { .. }),
        "the refusal keeps its own variant: {failure}"
    );
    assert_eq!(
        style_resources.requested_urls(),
        vec![PROVIDER_STYLE_DOCUMENT_URL.to_string()],
        "the refusal happens before any sprite request"
    );
    assert!(!surface_view.vector_tile_source_status().is_configured);
}

#[tokio::test]
async fn a_failed_sprite_fetch_keeps_the_install_and_draws_text_without_icons() {
    let (surface_view, style_resources) = sprite_backed_view(SPRITE_STYLE_DOCUMENT_JSON, true);
    surface_view
        .set_provider_vector_tile_source_from_document(
            ProviderVectorTileSource {
                provider_key: "document-test-provider".to_string(),
                style_id: "explore-day".to_string(),
            },
            PROVIDER_STYLE_DOCUMENT_URL.to_string(),
        )
        .await
        .expect("an unreachable sprite sheet does not refuse the base");
    assert_eq!(
        style_resources.requested_urls().len(),
        3,
        "the install fetches the index and stops at the sheet the provider cannot serve"
    );
    assert!(
        surface_view.vector_tile_source_status().is_configured,
        "the base installs and draws its text without icons"
    );
    let installed_scheduler = surface_view
        .take_vector_base_scheduler()
        .expect("the install wrote the scheduler");
    assert!(installed_scheduler.sprite_atlas().is_none());
}

#[test]
fn a_clear_landing_mid_draw_applies_when_the_draw_restores() {
    let surface_view = overlay_view();
    surface_view
        .set_vector_tile_source(switch_test_vector_source("drawn-tileset"))
        .expect("a well-formed source installs");
    // A draw owns the scheduler for its duration.
    let drawn_scheduler = surface_view.take_vector_base_scheduler();
    assert!(drawn_scheduler.is_some());
    surface_view
        .clear_vector_tile_source()
        .expect("a mid-draw clear defers instead of refusing");
    let pending_status = surface_view.vector_tile_source_status();
    assert!(
        pending_status.is_configured,
        "the status keeps reporting the source the drawing frame owns"
    );
    assert_eq!(pending_status.source_name, "drawn-tileset");
    surface_view.restore_vector_base_scheduler(drawn_scheduler);
    assert!(
        matches!(*surface_view.vector_base_lock(), VectorBaseSlot::Absent),
        "the restore applies the pending clear instead of writing the drawn base back"
    );
    assert!(!surface_view.vector_tile_source_status().is_configured);
    // The following frame borrows no vector base at all.
    let next_drawn_scheduler = surface_view.take_vector_base_scheduler();
    assert!(next_drawn_scheduler.is_none());
    surface_view.restore_vector_base_scheduler(next_drawn_scheduler);
}

#[test]
fn an_install_landing_mid_draw_applies_when_the_draw_restores() {
    let surface_view = overlay_view();
    // The raster-base case: the frame in flight borrowed no scheduler.
    let drawn_scheduler = surface_view.take_vector_base_scheduler();
    assert!(drawn_scheduler.is_none());
    surface_view
        .set_vector_tile_source(switch_test_vector_source("scheduled-tileset"))
        .expect("a mid-draw install defers instead of refusing");
    assert!(
        !surface_view.vector_tile_source_status().is_configured,
        "a source that has only been scheduled is not reported"
    );
    surface_view.restore_vector_base_scheduler(drawn_scheduler);
    let status = surface_view.vector_tile_source_status();
    assert!(status.is_configured);
    assert_eq!(status.source_name, "scheduled-tileset");
    assert_eq!(status.style_id, VECTOR_STYLE_ID_BRIGHT_LIGHT);
    assert_eq!(status.attribution_text, SWITCH_TEST_ATTRIBUTION_TEXT);
    // The following frame borrows the scheduler the pending install
    // replaced the drawn one with.
    let next_drawn_scheduler = surface_view
        .take_vector_base_scheduler()
        .expect("the following frame draws the installed base");
    assert_eq!(
        next_drawn_scheduler.style_id(),
        VECTOR_STYLE_ID_BRIGHT_LIGHT
    );
    assert_eq!(
        next_drawn_scheduler.source_descriptor().source_name,
        "scheduled-tileset"
    );
    surface_view.restore_vector_base_scheduler(Some(next_drawn_scheduler));
}

#[test]
fn two_installs_during_one_draw_resolve_to_the_last_one() {
    let surface_view = overlay_view();
    surface_view
        .set_vector_tile_source(switch_test_vector_source("drawn-tileset"))
        .expect("a well-formed source installs");
    let drawn_scheduler = surface_view.take_vector_base_scheduler();
    surface_view
        .set_vector_tile_source(switch_test_vector_source("first-switch-tileset"))
        .expect("the first mid-draw install defers");
    surface_view
        .set_vector_tile_source(switch_test_vector_source("last-switch-tileset"))
        .expect("the second mid-draw install replaces the first");
    surface_view.restore_vector_base_scheduler(drawn_scheduler);
    let status = surface_view.vector_tile_source_status();
    assert!(status.is_configured);
    assert_eq!(status.source_name, "last-switch-tileset");
    let next_drawn_scheduler = surface_view
        .take_vector_base_scheduler()
        .expect("the following frame draws the last requested base");
    assert_eq!(
        next_drawn_scheduler.source_descriptor().source_name,
        "last-switch-tileset"
    );
    surface_view.restore_vector_base_scheduler(Some(next_drawn_scheduler));
}

#[test]
fn a_clear_after_a_mid_draw_install_resolves_to_the_clear() {
    let surface_view = overlay_view();
    surface_view
        .set_vector_tile_source(switch_test_vector_source("drawn-tileset"))
        .expect("a well-formed source installs");
    let drawn_scheduler = surface_view.take_vector_base_scheduler();
    surface_view
        .set_vector_tile_source(switch_test_vector_source("scheduled-tileset"))
        .expect("the mid-draw install defers");
    surface_view
        .clear_vector_tile_source()
        .expect("the later mid-draw clear replaces the pending install");
    surface_view.restore_vector_base_scheduler(drawn_scheduler);
    assert!(matches!(
        *surface_view.vector_base_lock(),
        VectorBaseSlot::Absent
    ));
    assert!(!surface_view.vector_tile_source_status().is_configured);
}

#[test]
fn a_deferred_change_never_reaches_a_later_draw() {
    let surface_view = overlay_view();
    let drawn_scheduler = surface_view.take_vector_base_scheduler();
    surface_view
        .set_vector_tile_source(switch_test_vector_source("scheduled-tileset"))
        .expect("the mid-draw install defers");
    surface_view.restore_vector_base_scheduler(drawn_scheduler);
    // The change applied at the restore and the pending slot is empty, so
    // a draw that borrows and returns the base leaves it unchanged.
    assert!(
        surface_view
            .pending_vector_base_change_lock()
            .take()
            .is_none()
    );
    let borrowed_scheduler = surface_view
        .take_vector_base_scheduler()
        .expect("the applied install is in force");
    surface_view.restore_vector_base_scheduler(Some(borrowed_scheduler));
    let status = surface_view.vector_tile_source_status();
    assert!(status.is_configured);
    assert_eq!(status.source_name, "scheduled-tileset");
}
