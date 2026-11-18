//! The `wasm_bindgen` JavaScript surface: thin wrappers around the
//! rustic-maps-rendering core, returning plain JavaScript objects and arrays. The
//! naming mirrors the native API (crate `rustic-maps-rendering`, module `camera`).

use crate::SDK_COMPILED_VERSION;
use crate::generated_contract_field_names::*;
use crate::shared::{validated_bounding_box, validated_coordinate, validated_viewport_pixels};
use geospatial_types::route::{RouteData, RouteLeg, RouteStep};
use geospatial_types::{BoundingBox, GeographicCoordinate, ManeuverType, SDKError};
use js_sys::{Array, Object, Reflect, Uint8Array};
use rustic_maps_core::offline_routing::calculate_offline_route_from_archive_bytes;
use rustic_maps_navigation::{
    RouteWalk, SimulatedTripFix as NativeSimulatedTripFix,
    SimulatedTripPace as NativeSimulatedTripPace,
};
use rustic_maps_rendering::CameraInteraction;
use rustic_maps_rendering::EdgeInsetsPixels as NativeEdgeInsetsPixels;
use rustic_maps_rendering::MapCamera as NativeMapCamera;
use rustic_maps_rendering::TileKey;
use rustic_maps_rendering::fit_bounds;
use wasm_bindgen::prelude::*;

/// Name of the window flag the browser test suite polls to confirm the SDK
/// started.
const SDK_READY_WINDOW_FLAG: &str = "__sdkReady";
/// Field name of a camera pose's zoom level exposed to JavaScript.
const CAMERA_POSE_FIELD_ZOOM_LEVEL: &str = "zoomLevel";
/// Field name of a camera pose's bearing exposed to JavaScript.
const CAMERA_POSE_FIELD_BEARING_DEGREES: &str = "bearingDegrees";
/// Field name of a camera pose's pitch exposed to JavaScript.
const CAMERA_POSE_FIELD_PITCH_DEGREES: &str = "pitchDegrees";

/// JavaScript property carrying a thrown failure's stable error code.
pub(crate) const FAILURE_FIELD_CODE: &str = "code";
/// JavaScript property a thrown failure's string conversion reads. An empty
/// name keeps `Error.prototype.toString` at the message alone, so callers that
/// compare or display `String(error)` keep seeing the verbatim detail.
pub(crate) const FAILURE_FIELD_NAME: &str = "name";

/// A coordinate, viewport extent or bounding-box corner failed validation.
/// Mirrors the contract's `InvalidCoordinateInput` variant.
pub(crate) const ERROR_CODE_INVALID_COORDINATE_INPUT: &str = "invalidCoordinateInput";
/// The transport failed before the provider answered. Mirrors the contract's
/// `NetworkRequestFailed` variant.
pub(crate) const ERROR_CODE_NETWORK_REQUEST_FAILED: &str = "networkRequestFailed";
/// A payload could not be parsed. Mirrors the contract's
/// `ResponseParsingFailure` variant.
pub(crate) const ERROR_CODE_RESPONSE_PARSING_FAILURE: &str = "responseParsingFailure";
/// The provider returned an application-level refusal. Mirrors the contract's
/// `ProviderRequestRejected` variant.
pub(crate) const ERROR_CODE_PROVIDER_REQUEST_REJECTED: &str = "providerRequestRejected";
/// The provider's rate limiter refused the request. Mirrors the contract's
/// `RateLimitExhausted` variant.
pub(crate) const ERROR_CODE_RATE_LIMIT_EXHAUSTED: &str = "rateLimitExhausted";
/// No provider is registered for the requested contract or provider key.
/// Mirrors the contract's `ProviderNotConfigured` variant.
pub(crate) const ERROR_CODE_PROVIDER_NOT_CONFIGURED: &str = "providerNotConfigured";
/// A navigation input (a route record, a navigation policy or a location
/// update) failed validation. Mirrors the contract's `InvalidNavigationInput`
/// variant.
pub(crate) const ERROR_CODE_INVALID_NAVIGATION_INPUT: &str = "invalidNavigationInput";
/// A render descriptor (an overlay, label, indicator or surface input) is not
/// usable. Mirrors the contract's `InvalidRenderInput` variant.
pub(crate) const ERROR_CODE_INVALID_RENDER_INPUT: &str = "invalidRenderInput";
/// Live rendering could not proceed on the canvas surface. Mirrors the
/// contract's `RenderingSurfaceUnavailable` variant.
pub(crate) const ERROR_CODE_RENDERING_SURFACE_UNAVAILABLE: &str = "renderingSurfaceUnavailable";
/// A cache operation failed. Mirrors the contract's `CacheOperationFailure`
/// variant.
pub(crate) const ERROR_CODE_CACHE_OPERATION_FAILURE: &str = "cacheOperationFailure";
/// An offline routing graph failed validation or could not be read. Mirrors
/// the contract's `OfflineRoutingFailure` variant.
pub(crate) const ERROR_CODE_OFFLINE_ROUTING_FAILURE: &str = "offlineRoutingFailure";

/// A seam-only argument (an options object, a lifecycle state or an address)
/// is not usable and has no matching contract error variant.
pub(crate) const ERROR_CODE_INVALID_ARGUMENT: &str = "invalidArgument";
/// A requested capability (a style, a source kind or a graphics feature) is
/// not available in this build and has no matching contract error variant.
pub(crate) const ERROR_CODE_UNSUPPORTED_CAPABILITY: &str = "unsupportedCapability";
/// A screen-to-ground or ground-to-screen transform has no solution (a point
/// behind the camera or a ray below the horizon) and has no matching contract
/// error variant.
pub(crate) const ERROR_CODE_GEOMETRY_UNAVAILABLE: &str = "geometryUnavailable";
/// The browser runtime a call needs is not available and has no matching
/// contract error variant.
pub(crate) const ERROR_CODE_RUNTIME_UNAVAILABLE: &str = "runtimeUnavailable";
/// A failure with no more specific classification.
pub(crate) const ERROR_CODE_INTERNAL: &str = "internal";

/// Builds the failure value the JavaScript surface throws for an
/// unclassified failure.
pub(crate) fn failure(message: &str) -> JsValue {
    failure_with_code(ERROR_CODE_INTERNAL, message)
}

/// Builds a thrown failure carrying a stable error code and the verbatim
/// detail message.
///
/// The value is a JavaScript `Error` whose `message` is the detail. The
/// `code` property is the stable identifier a catch handler branches on, and
/// `String(error)` yields the message alone.
pub(crate) fn failure_with_code(code: &str, message: &str) -> JsValue {
    let error = js_sys::Error::new(message);
    let error_value: &JsValue = error.as_ref();
    let _ = Reflect::set(
        error_value,
        &JsValue::from_str(FAILURE_FIELD_NAME),
        &JsValue::from_str(""),
    );
    let _ = Reflect::set(
        error_value,
        &JsValue::from_str(FAILURE_FIELD_CODE),
        &JsValue::from_str(code),
    );
    error_value.clone()
}

/// Maps a core `SDKError` onto the thrown failure value, keeping its verbatim
/// display message and its contract variant as the stable error code.
pub(crate) fn sdk_error_failure(sdk_error: &SDKError) -> JsValue {
    let code = match sdk_error {
        SDKError::InvalidCoordinateInput(_) => ERROR_CODE_INVALID_COORDINATE_INPUT,
        SDKError::NetworkRequestFailed(_) => ERROR_CODE_NETWORK_REQUEST_FAILED,
        SDKError::ResponseParsingFailure(_) => ERROR_CODE_RESPONSE_PARSING_FAILURE,
        SDKError::ProviderRequestRejected(_) => ERROR_CODE_PROVIDER_REQUEST_REJECTED,
        SDKError::RateLimitExhausted => ERROR_CODE_RATE_LIMIT_EXHAUSTED,
        SDKError::ProviderNotConfigured(_) => ERROR_CODE_PROVIDER_NOT_CONFIGURED,
        SDKError::CacheOperationFailure(_) => ERROR_CODE_CACHE_OPERATION_FAILURE,
        SDKError::OfflineRoutingFailure(_) => ERROR_CODE_OFFLINE_ROUTING_FAILURE,
    };
    failure_with_code(code, &sdk_error.to_string())
}

/// Builds a plain JavaScript object from `(name, value)` pairs.
pub(crate) fn object_from_fields(fields: &[(&str, JsValue)]) -> Result<Object, JsValue> {
    let object = Object::new();
    for (field_name, field_value) in fields {
        Reflect::set(&object, &JsValue::from_str(field_name), field_value)?;
    }
    Ok(object)
}

/// Builds the plain `{latitude, longitude, zoomLevel, bearingDegrees,
/// pitchDegrees}` pose object of a native camera.
fn camera_pose_object(native_camera: &NativeMapCamera) -> Result<Object, JsValue> {
    object_from_fields(&[
        (
            COORDINATE_FIELD_LATITUDE,
            JsValue::from(native_camera.center.latitude),
        ),
        (
            COORDINATE_FIELD_LONGITUDE,
            JsValue::from(native_camera.center.longitude),
        ),
        (
            CAMERA_POSE_FIELD_ZOOM_LEVEL,
            JsValue::from(native_camera.zoom_level),
        ),
        (
            CAMERA_POSE_FIELD_BEARING_DEGREES,
            JsValue::from(native_camera.bearing_degrees),
        ),
        (
            CAMERA_POSE_FIELD_PITCH_DEGREES,
            JsValue::from(native_camera.pitch_degrees),
        ),
    ])
}

/// Anchored map camera (geographic center, zoom, bearing, pitch) mirroring
/// the native [`rustic_maps_rendering::MapCamera`]. The ground point under the
/// center always sits at the viewport center.
#[wasm_bindgen]
pub struct MapCamera {
    native_camera: NativeMapCamera,
}

#[wasm_bindgen]
impl MapCamera {
    /// Builds a camera, clamping zoom and pitch and wrapping the bearing
    /// into `[0, 360)`, exactly like the native constructor.
    #[wasm_bindgen(constructor)]
    pub fn new(
        center_latitude: f64,
        center_longitude: f64,
        zoom_level: f64,
        bearing_degrees: f64,
        pitch_degrees: f64,
    ) -> Result<MapCamera, JsValue> {
        let center = validated_coordinate(center_latitude, center_longitude)
            .map_err(|message| failure_with_code(ERROR_CODE_INVALID_COORDINATE_INPUT, &message))?;
        Ok(Self {
            native_camera: NativeMapCamera::new(center, zoom_level, bearing_degrees, pitch_degrees),
        })
    }

    /// Builds the camera that frames a bounding box inside the viewport's
    /// inset region, mirroring the contract's `fit_bounds` constructor:
    /// top-down and north-up, zoomed to the largest level that keeps the
    /// whole box inside the inset area (clamped to `maximum_zoom_level`), and
    /// centered on the inset region's center. Fails with a message when the
    /// corners, viewport, insets or maximum zoom are unusable.
    // The argument count is the JavaScript API's shape (a bounding box plus a
    // viewport plus insets); bundling it into an object would change the seam
    // the page and its tests already call.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = fittingBounds)]
    pub fn fitting_bounds(
        south_west_latitude: f64,
        south_west_longitude: f64,
        north_east_latitude: f64,
        north_east_longitude: f64,
        width_pixels: f64,
        height_pixels: f64,
        inset_top_pixels: f64,
        inset_right_pixels: f64,
        inset_bottom_pixels: f64,
        inset_left_pixels: f64,
        maximum_zoom_level: f64,
    ) -> Result<MapCamera, JsValue> {
        let bounding_box = validated_bounding_box(
            south_west_latitude,
            south_west_longitude,
            north_east_latitude,
            north_east_longitude,
        )
        .map_err(|message| failure_with_code(ERROR_CODE_INVALID_COORDINATE_INPUT, &message))?;
        let viewport = validated_viewport_pixels(width_pixels, height_pixels)
            .map_err(|message| failure_with_code(ERROR_CODE_INVALID_COORDINATE_INPUT, &message))?;
        let edge_insets = NativeEdgeInsetsPixels {
            left: inset_left_pixels,
            top: inset_top_pixels,
            right: inset_right_pixels,
            bottom: inset_bottom_pixels,
        };
        let fitted_camera = fit_bounds(&bounding_box, &viewport, edge_insets, maximum_zoom_level)
            .map_err(|message| {
            failure_with_code(ERROR_CODE_INVALID_COORDINATE_INPUT, &message)
        })?;
        Ok(Self {
            native_camera: fitted_camera,
        })
    }

    /// Eases from `start_camera` toward `target_camera` over `fraction` —
    /// the contract's `MapCamera.interpolate` member.
    ///
    /// The SDK's one camera-interpolation curve: the fraction is clamped into
    /// `[0, 1]` and eased with smoothstep, the center travels the shortest
    /// longitude path in Mercator space, zoom and pitch interpolate linearly,
    /// and the bearing takes the shortest arc. Fractions at or below zero
    /// answer `start_camera` verbatim and fractions at or above one answer
    /// `target_camera` verbatim, so an animation's endpoints are exact.
    ///
    /// # Parameters
    /// - `start_camera` — the pose the interpolation starts from.
    /// - `target_camera` — the pose the interpolation heads toward.
    /// - `fraction` — animation progress, clamped into `[0, 1]`.
    ///
    /// # Returns
    /// The camera pose at the eased progress between the two poses.
    ///
    /// # Errors
    /// Throws `InvalidCoordinateInput` when `fraction` is not finite.
    #[wasm_bindgen(js_name = interpolate)]
    pub fn interpolate(
        start_camera: &MapCamera,
        target_camera: &MapCamera,
        fraction: f64,
    ) -> Result<MapCamera, JsValue> {
        let interpolated_camera = crate::web_camera::interpolate_cameras(
            start_camera.native_camera(),
            target_camera.native_camera(),
            fraction,
        )
        .map_err(|message| failure_with_code(ERROR_CODE_INVALID_COORDINATE_INPUT, &message))?;
        Ok(MapCamera::from_native_camera(interpolated_camera))
    }

    /// Projects a coordinate to viewport pixels (origin top-left, y down),
    /// as `{x, y}`. Fails with a message when the point lies behind the
    /// camera.
    #[wasm_bindgen(js_name = projectToScreen)]
    pub fn project_to_screen(
        &self,
        latitude: f64,
        longitude: f64,
        width_pixels: f64,
        height_pixels: f64,
        field_of_view_degrees: f64,
    ) -> Result<Object, JsValue> {
        let coordinate = validated_coordinate(latitude, longitude)
            .map_err(|message| failure_with_code(ERROR_CODE_INVALID_COORDINATE_INPUT, &message))?;
        let viewport = validated_viewport_pixels(width_pixels, height_pixels)
            .map_err(|message| failure_with_code(ERROR_CODE_INVALID_COORDINATE_INPUT, &message))?;
        let screen_point = self
            .native_camera
            .project_to_screen(&coordinate, &viewport, field_of_view_degrees)
            .ok_or_else(|| {
                failure_with_code(
                    ERROR_CODE_GEOMETRY_UNAVAILABLE,
                    "coordinate lies behind the camera",
                )
            })?;
        object_from_fields(&[
            (SCREEN_POINT_FIELD_X, JsValue::from(screen_point[0])),
            (SCREEN_POINT_FIELD_Y, JsValue::from(screen_point[1])),
        ])
    }

    /// Unprojects a viewport pixel onto the ground plane, as
    /// `{latitude, longitude}`. Fails with a message when the ray misses the
    /// ground (typically below the horizon at steep pitch).
    #[wasm_bindgen(js_name = screenToGroundCoordinate)]
    pub fn screen_to_ground_coordinate(
        &self,
        screen_x: f64,
        screen_y: f64,
        width_pixels: f64,
        height_pixels: f64,
        field_of_view_degrees: f64,
    ) -> Result<Object, JsValue> {
        let viewport = validated_viewport_pixels(width_pixels, height_pixels)
            .map_err(|message| failure_with_code(ERROR_CODE_INVALID_COORDINATE_INPUT, &message))?;
        let ground_coordinate = self
            .native_camera
            .screen_to_ground_coordinate([screen_x, screen_y], &viewport, field_of_view_degrees)
            .ok_or_else(|| {
                failure_with_code(
                    ERROR_CODE_GEOMETRY_UNAVAILABLE,
                    "pixel ray misses the ground plane",
                )
            })?;
        object_from_fields(&[
            (
                COORDINATE_FIELD_LATITUDE,
                JsValue::from(ground_coordinate.latitude),
            ),
            (
                COORDINATE_FIELD_LONGITUDE,
                JsValue::from(ground_coordinate.longitude),
            ),
        ])
    }

    /// Every tile at `tile_zoom_level` whose area intersects the visible
    /// ground, as an array of `{tileX, tileY, zoomLevel}` sorted row-major
    /// from the north, mirroring the native `visible_tile_keys`.
    #[wasm_bindgen(js_name = visibleTileKeys)]
    pub fn visible_tile_keys(
        &self,
        width_pixels: f64,
        height_pixels: f64,
        field_of_view_degrees: f64,
        tile_zoom_level: u8,
    ) -> Result<Array, JsValue> {
        let viewport = validated_viewport_pixels(width_pixels, height_pixels)
            .map_err(|message| failure_with_code(ERROR_CODE_INVALID_COORDINATE_INPUT, &message))?;
        let visible_tiles =
            self.native_camera
                .visible_tile_keys(&viewport, field_of_view_degrees, tile_zoom_level);
        let tile_entries = Array::new();
        for tile_key in &visible_tiles {
            tile_entries.push(&tile_address_object(tile_key)?.into());
        }
        Ok(tile_entries)
    }

    /// Ground meters represented by one screen pixel at the camera's center,
    /// the value a scale bar needs. The viewport and field-of-view arguments
    /// mirror the projection methods' shape and the viewport is validated
    /// like everywhere else; the flat-centered approximation is itself
    /// independent of both.
    #[wasm_bindgen(js_name = groundMetersPerScreenPixel)]
    pub fn ground_meters_per_screen_pixel(
        &self,
        width_pixels: f64,
        height_pixels: f64,
        field_of_view_degrees: f64,
    ) -> Result<f64, JsValue> {
        let _viewport = validated_viewport_pixels(width_pixels, height_pixels)
            .map_err(|message| failure_with_code(ERROR_CODE_INVALID_COORDINATE_INPUT, &message))?;
        let _ = field_of_view_degrees;
        Ok(self.native_camera.ground_meters_per_screen_pixel())
    }

    /// The camera's center, as `{latitude, longitude}` — the contract's
    /// `center()` getter. The getters make a fitted camera's chosen framing
    /// readable.
    #[wasm_bindgen(js_name = center)]
    pub fn center(&self) -> Result<Object, JsValue> {
        object_from_fields(&[
            (
                COORDINATE_FIELD_LATITUDE,
                JsValue::from(self.native_camera.center.latitude),
            ),
            (
                COORDINATE_FIELD_LONGITUDE,
                JsValue::from(self.native_camera.center.longitude),
            ),
        ])
    }

    /// The camera's zoom level — the contract's `zoom_level()` getter.
    #[wasm_bindgen(js_name = zoomLevel)]
    pub fn zoom_level(&self) -> f64 {
        self.native_camera.zoom_level
    }

    /// The camera's bearing in degrees — the contract's
    /// `bearing_degrees()` getter.
    #[wasm_bindgen(js_name = bearingDegrees)]
    pub fn bearing_degrees(&self) -> f64 {
        self.native_camera.bearing_degrees
    }

    /// The camera's pitch in degrees — the contract's `pitch_degrees()`
    /// getter.
    #[wasm_bindgen(js_name = pitchDegrees)]
    pub fn pitch_degrees(&self) -> f64 {
        self.native_camera.pitch_degrees
    }

    /// Nearest whole zoom level a tile fetch should use for this camera,
    /// mirroring the native `tile_zoom_level`.
    #[wasm_bindgen(js_name = tileZoomLevel)]
    pub fn tile_zoom_level(&self) -> u8 {
        self.native_camera.tile_zoom_level()
    }

    /// Pans the camera by a pointer travel of `delta_x_pixels` x
    /// `delta_y_pixels` (screen pixels): the ground point the pointer just
    /// moved over becomes the new camera center, so map content tracks the
    /// pointer exactly. Answers the input camera pose unchanged when the
    /// delta's ray misses the ground plane, as the contract member
    /// documents. The free `panCameraByScreenPixels` helper is an
    /// addition-only alias for existing call sites.
    #[wasm_bindgen(js_name = panByScreenPixels)]
    pub fn pan_by_screen_pixels(
        &self,
        delta_x_pixels: f64,
        delta_y_pixels: f64,
        width_pixels: f64,
        height_pixels: f64,
        field_of_view_degrees: f64,
    ) -> Result<Object, JsValue> {
        let viewport = validated_viewport_pixels(width_pixels, height_pixels)
            .map_err(|message| failure_with_code(ERROR_CODE_INVALID_COORDINATE_INPUT, &message))?;
        camera_pose_object(&self.native_camera.pan_by_screen_pixels(
            [delta_x_pixels, delta_y_pixels],
            &viewport,
            field_of_view_degrees,
        ))
    }

    /// Zooms the camera by `delta_zoom_levels` keeping the world point under
    /// `focal_x_pixels`/`focal_y_pixels` fixed under that pixel; the
    /// camera's own zoom range clamps the answer, and the uncorrected zoom
    /// answers when the focal ray misses the ground plane.
    #[wasm_bindgen(js_name = zoomByLevelsAroundScreenPoint)]
    pub fn zoom_by_levels_around_screen_point(
        &self,
        delta_zoom_levels: f64,
        focal_x_pixels: f64,
        focal_y_pixels: f64,
        width_pixels: f64,
        height_pixels: f64,
        field_of_view_degrees: f64,
    ) -> Result<Object, JsValue> {
        let viewport = validated_viewport_pixels(width_pixels, height_pixels)
            .map_err(|message| failure_with_code(ERROR_CODE_INVALID_COORDINATE_INPUT, &message))?;
        camera_pose_object(&self.native_camera.zoom_by_levels_around_screen_point(
            delta_zoom_levels,
            [focal_x_pixels, focal_y_pixels],
            &viewport,
            field_of_view_degrees,
        ))
    }

    /// Rotates the camera bearing by `delta_bearing_degrees` (degrees
    /// clockwise, wrapping into `[0, 360)`) keeping the world point under
    /// `focal_x_pixels`/`focal_y_pixels` fixed under that pixel; the
    /// uncorrected rotation answers when the focal ray misses the ground
    /// plane.
    #[wasm_bindgen(js_name = rotateByDegreesAroundScreenPoint)]
    pub fn rotate_by_degrees_around_screen_point(
        &self,
        delta_bearing_degrees: f64,
        focal_x_pixels: f64,
        focal_y_pixels: f64,
        width_pixels: f64,
        height_pixels: f64,
        field_of_view_degrees: f64,
    ) -> Result<Object, JsValue> {
        let viewport = validated_viewport_pixels(width_pixels, height_pixels)
            .map_err(|message| failure_with_code(ERROR_CODE_INVALID_COORDINATE_INPUT, &message))?;
        camera_pose_object(&self.native_camera.rotate_by_degrees_around_screen_point(
            delta_bearing_degrees,
            [focal_x_pixels, focal_y_pixels],
            &viewport,
            field_of_view_degrees,
        ))
    }
}

impl MapCamera {
    /// Wraps a native camera pose for a browser readback.
    pub(crate) fn from_native_camera(native_camera: NativeMapCamera) -> Self {
        Self { native_camera }
    }

    /// The native camera pose behind this binding, shared with the wasm GPU
    /// canvas renderer (see [`crate::canvas`]).
    pub(crate) fn native_camera(&self) -> &NativeMapCamera {
        &self.native_camera
    }
}

fn tile_address_object(tile_key: &TileKey) -> Result<Object, JsValue> {
    object_from_fields(&[
        (TILE_FIELD_TILE_X, JsValue::from(tile_key.tile_x)),
        (TILE_FIELD_TILE_Y, JsValue::from(tile_key.tile_y)),
        (TILE_FIELD_ZOOM_LEVEL, JsValue::from(tile_key.zoom_level)),
    ])
}

/// Maps a coordinate to normalized world coordinates in `[0, 1]²`, as
/// `{x, y}`; latitude is clamped to the Mercator cutoffs, mirroring the
/// native `coordinate_to_normalized_world`.
#[wasm_bindgen(js_name = coordinateToNormalizedWorld)]
pub fn coordinate_to_normalized_world(latitude: f64, longitude: f64) -> Result<Object, JsValue> {
    let coordinate = validated_coordinate(latitude, longitude)
        .map_err(|message| failure_with_code(ERROR_CODE_INVALID_COORDINATE_INPUT, &message))?;
    let normalized_world = rustic_maps_rendering::coordinate_to_normalized_world(&coordinate);
    object_from_fields(&[
        (NORMALIZED_WORLD_FIELD_X, JsValue::from(normalized_world[0])),
        (NORMALIZED_WORLD_FIELD_Y, JsValue::from(normalized_world[1])),
    ])
}

/// The XYZ address of the tile containing the coordinate at the zoom level,
/// as `{tileX, tileY, zoomLevel}`, mirroring the native
/// `tile_key_for_coordinate`.
#[wasm_bindgen(js_name = tileKeyForCoordinate)]
pub fn tile_key_for_coordinate(
    latitude: f64,
    longitude: f64,
    zoom_level: u8,
) -> Result<Object, JsValue> {
    let coordinate = validated_coordinate(latitude, longitude)
        .map_err(|message| failure_with_code(ERROR_CODE_INVALID_COORDINATE_INPUT, &message))?;
    tile_address_object(&rustic_maps_rendering::tile_key_for_coordinate(
        &coordinate,
        zoom_level,
    ))
}

/// Every XYZ tile whose area intersects the spanning box, as an array of
/// `{tileX, tileY, zoomLevel}` ordered row-major from the north, mirroring
/// the native `tiles_covering`.
#[wasm_bindgen(js_name = tilesCovering)]
pub fn tiles_covering(
    south_west_latitude: f64,
    south_west_longitude: f64,
    north_east_latitude: f64,
    north_east_longitude: f64,
    zoom_level: u8,
) -> Result<Array, JsValue> {
    let bounding_box = validated_bounding_box(
        south_west_latitude,
        south_west_longitude,
        north_east_latitude,
        north_east_longitude,
    )
    .map_err(|message| failure_with_code(ERROR_CODE_INVALID_COORDINATE_INPUT, &message))?;
    let covering_tiles = rustic_maps_rendering::tiles_covering(&bounding_box, zoom_level);
    let tile_entries = Array::new();
    for tile_key in &covering_tiles {
        tile_entries.push(&tile_address_object(tile_key)?.into());
    }
    Ok(tile_entries)
}

/// Semver of the compiled SDK, equal to the workspace package version.
#[wasm_bindgen(js_name = getSdkCompiledVersion)]
pub fn get_sdk_compiled_version() -> String {
    SDK_COMPILED_VERSION.to_string()
}

/// Installs the browser SDK runtime: the panic hook, the diagnostics
/// readout ring, which installs the bounded ring subscriber when no global
/// subscriber is live yet (events then record at the idle gate or above
/// until `subscribe_to_diagnostic_events` lowers it), and the
/// `window.__sdkReady` flag a page can poll for readiness.
fn install_browser_runtime() -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    crate::diagnostics::ensure_ring_subscriber();
    // Every provider transport reports one redacted observation per attempt;
    // the bridge publishes each as a diagnostics event under the transport's
    // target, so the page's diagnostics readout carries the request stream.
    rustic_maps_core::provider_observation::install_tracing_observation_bridge();
    let browser_window = web_sys::window().ok_or_else(|| {
        failure_with_code(ERROR_CODE_RUNTIME_UNAVAILABLE, "window is unavailable")
    })?;
    Reflect::set(
        &browser_window,
        &JsValue::from_str(SDK_READY_WINDOW_FLAG),
        &JsValue::TRUE,
    )?;
    Ok(())
}

/// The browser SDK root (`MapsSdk.create()`): the object a page creates once
/// and hands to its [`crate::canvas::MapView`], mirroring the native
/// `MapSdk` construction shape. The wasm runtime is module-global, so this
/// root carries the readiness result and the compiled version rather than a
/// second runtime.
#[wasm_bindgen(js_name = MapsSdk)]
pub struct MapsSdk {
    /// The compiled SDK version, captured when the runtime became ready.
    compiled_version: String,
}

#[wasm_bindgen]
impl MapsSdk {
    /// Creates the SDK root and resolves once the browser runtime is ready
    /// (the same work [`start_sdk`] performs). The returned root is what
    /// `new MapView(canvas, maps)` expects.
    #[wasm_bindgen(js_name = create)]
    pub async fn create() -> Result<MapsSdk, JsValue> {
        install_browser_runtime()?;
        Ok(Self {
            compiled_version: get_sdk_compiled_version(),
        })
    }

    /// The compiled SDK version (the workspace package version).
    #[wasm_bindgen(getter, js_name = compiledVersion)]
    pub fn compiled_version(&self) -> String {
        self.compiled_version.clone()
    }
}

/// Readiness starter: installs the browser runtime, sets the
/// `window.__sdkReady` flag and resolves to the compiled version. Prefer
/// `await MapsSdk.create()`; this alias stays for existing call sites.
#[wasm_bindgen(js_name = initializeSdk)]
pub fn start_sdk() -> js_sys::Promise {
    wasm_bindgen_futures::future_to_promise(async move {
        install_browser_runtime()?;
        Ok(JsValue::from_str(&get_sdk_compiled_version()))
    })
}

// ---------------------------------------------------------------------------
// The offline routing graph seam (milestone M7, wasm in-memory graph path):
// the compiled `RMOG` archive loads from in-memory bytes in the browser —
// the wasm target has no `mmap` — and answers routes over the committed
// routing-grid fixture, which `serve.mjs` serves same-origin.
// ---------------------------------------------------------------------------

/// Routes between two coordinates over an offline routing graph loaded from
/// in-memory archive bytes. The browser has no `mmap`, so the compiled archive
/// arrives as a `Uint8Array`, for example a same-origin downloaded archive,
/// and is validated in memory before routing, exactly like the native
/// `Memory` source seam.
///
/// The request routes through the in-repo
/// [`rustic_maps_core::offline_routing::ArchiveOfflineRoutingProvider`] (its A*
/// engine, snap, restricts and derived maneuver steps) under the
/// automobile / fastest / metric request the offline graph profile serves
/// (`RouteOptions` defaults). The answer mirrors the frozen contract's
/// `RouteData`/`RouteLeg`/`RouteStep` dictionaries field for field (lower
/// camel case; `maneuverType` the stable snake-case label).
///
/// The browser half of the contract's `offline_route_from_archive_bytes`
/// free helper: both delegate to
/// [`rustic_maps_core::offline_routing::calculate_offline_route_from_archive_bytes`],
/// so the native and wasm answers cannot drift. Failures throw an error
/// carrying the verbatim detail and a stable code: bytes that fail archive
/// validation, a coordinate within no graph node's snap tolerance, or a
/// request pair the graph cannot connect — refused, never guessed.
#[wasm_bindgen(js_name = offlineRouteFromArchiveBytes)]
pub fn offline_route_from_archive_bytes(
    archive_bytes: Uint8Array,
    start_latitude: f64,
    start_longitude: f64,
    end_latitude: f64,
    end_longitude: f64,
) -> Result<js_sys::Promise, JsValue> {
    let archive_bytes = archive_bytes.to_vec();
    let offline_route_answer = async move {
        offline_route_answer_from_bytes(
            &archive_bytes,
            start_latitude,
            start_longitude,
            end_latitude,
            end_longitude,
        )
        .await
        .map(Into::into)
    };
    Ok(wasm_bindgen_futures::future_to_promise(
        offline_route_answer,
    ))
}

/// Validates the coordinates and answers the route over the archive bytes
/// through the shared `rustic-maps-core` implementation the contract's native member
/// also uses. On wasm the whole call stays pure computation — no await
/// points, no runtime — so the promise settles after one microtask chain.
async fn offline_route_answer_from_bytes(
    archive_bytes: &[u8],
    start_latitude: f64,
    start_longitude: f64,
    end_latitude: f64,
    end_longitude: f64,
) -> Result<Object, JsValue> {
    let start = validated_coordinate(start_latitude, start_longitude)
        .map_err(|message| failure_with_code(ERROR_CODE_INVALID_COORDINATE_INPUT, &message))?;
    let end = validated_coordinate(end_latitude, end_longitude)
        .map_err(|message| failure_with_code(ERROR_CODE_INVALID_COORDINATE_INPUT, &message))?;
    let route_data = calculate_offline_route_from_archive_bytes(archive_bytes, &start, &end)
        .await
        .map_err(|sdk_error| sdk_error_failure(&sdk_error))?;
    route_data_object(&route_data)
        .map_err(|js_failure| failure(&js_conversion_failure_message(js_failure)))
}

/// Maps the object builders' `JsValue` failure (field-set reflection) onto
/// the detail text of the seam's internal failure.
fn js_conversion_failure_message(js_failure: JsValue) -> String {
    js_failure
        .as_string()
        .unwrap_or_else(|| "route answer object construction failed".to_string())
}

/// Builds the plain JavaScript route answer of a [`RouteData`], mirroring
/// the frozen contract's `RouteData` dictionary field for field (see the
/// export docs). Shared with the live route seam (`crate::live_providers`),
/// so the offline and live answers cannot drift apart.
pub(crate) fn route_data_object(route_data: &RouteData) -> Result<Object, JsValue> {
    let route_legs = Array::new();
    for route_leg in &route_data.legs {
        route_legs.push(&route_leg_object(route_leg)?.into());
    }
    let route_geometry = Array::new();
    for coordinate in &route_data.geometry {
        route_geometry.push(&geographic_coordinate_object(coordinate)?.into());
    }
    object_from_fields(&[
        (
            ROUTE_FIELD_DISTANCE_METERS,
            JsValue::from(route_data.distance_meters),
        ),
        (
            ROUTE_FIELD_DURATION_SECONDS,
            JsValue::from(route_data.duration_seconds),
        ),
        (
            ROUTE_FIELD_BOUNDING_BOX,
            bounding_box_object(&route_data.bounding_box)?.into(),
        ),
        (ROUTE_FIELD_GEOMETRY, route_geometry.into()),
        (ROUTE_FIELD_LEGS, route_legs.into()),
    ])
}

/// Builds the plain JavaScript leg object of a [`RouteLeg`], mirroring the
/// frozen contract's `RouteLeg` dictionary (see the export docs).
fn route_leg_object(route_leg: &RouteLeg) -> Result<Object, JsValue> {
    let route_steps = Array::new();
    for route_step in &route_leg.steps {
        route_steps.push(&route_step_object(route_step)?.into());
    }
    object_from_fields(&[
        (
            ROUTE_FIELD_DISTANCE_METERS,
            JsValue::from(route_leg.distance_meters),
        ),
        (
            ROUTE_FIELD_DURATION_SECONDS,
            JsValue::from(route_leg.duration_seconds),
        ),
        (ROUTE_FIELD_STEPS, route_steps.into()),
    ])
}

/// Builds the plain JavaScript step object of a [`RouteStep`], mirroring
/// the frozen contract's `RouteStep` dictionary; the maneuver classification
/// serializes as its stable snake-case label (`departure`, `turn-right`, …),
/// the contract's established single spelling.
fn route_step_object(route_step: &RouteStep) -> Result<Object, JsValue> {
    let step_shape = Array::new();
    for shape_coordinate in &route_step.shape {
        step_shape.push(&geographic_coordinate_object(shape_coordinate)?.into());
    }
    object_from_fields(&[
        (
            ROUTE_FIELD_DISTANCE_METERS,
            JsValue::from(route_step.distance_meters),
        ),
        (
            ROUTE_FIELD_DURATION_SECONDS,
            JsValue::from(route_step.duration_seconds),
        ),
        (
            ROUTE_FIELD_MANEUVER_TYPE,
            JsValue::from_str(&route_step.maneuver_type.stable_label()),
        ),
        (
            ROUTE_FIELD_INSTRUCTION_DISPLAY_TEXT,
            JsValue::from_str(&route_step.instruction_display_text),
        ),
        (
            ROUTE_FIELD_INSTRUCTION_SPEECH_TEXT,
            JsValue::from_str(&route_step.instruction_speech_text),
        ),
        (ROUTE_FIELD_SHAPE, step_shape.into()),
    ])
}

/// Builds the plain `{southWest, northEast}` bounds object of a
/// [`BoundingBox`], mirroring the frozen contract's `BoundingBox`
/// dictionary (see the export docs).
fn bounding_box_object(bounding_box: &BoundingBox) -> Result<Object, JsValue> {
    object_from_fields(&[
        (
            BOUNDING_BOX_FIELD_SOUTH_WEST,
            geographic_coordinate_object(&bounding_box.south_west)?.into(),
        ),
        (
            BOUNDING_BOX_FIELD_NORTH_EAST,
            geographic_coordinate_object(&bounding_box.north_east)?.into(),
        ),
    ])
}

/// Builds the plain `{latitude, longitude}` object of a route shape vertex
/// or bounds corner, reusing the coordinate field names every export uses.
pub(crate) fn geographic_coordinate_object(
    coordinate: &GeographicCoordinate,
) -> Result<Object, JsValue> {
    object_from_fields(&[
        (
            COORDINATE_FIELD_LATITUDE,
            JsValue::from(coordinate.latitude),
        ),
        (
            COORDINATE_FIELD_LONGITUDE,
            JsValue::from(coordinate.longitude),
        ),
    ])
}

// ---------------------------------------------------------------------------
// The simulated-trip seam (M27): the browser mirror of the contract's
// `RouteSimulator`. It traverses the planned route through the same
// `rustic-maps-navigation` walker the native surfaces use, so the page cannot drift
// from the Kotlin/Swift answer. The constructor takes the lower-camel-case
// plain objects the rest of this seam exchanges: a `Route` (as
// `calculateRoute` answers it) and a `SimulatedTripPace`.
// ---------------------------------------------------------------------------

/// Reads one JavaScript object field, mapping a getter failure (never seen
/// for a plain object) to a detail message.
pub(crate) fn javascript_object_field(
    object: &Object,
    field_name: &str,
) -> Result<JsValue, String> {
    Reflect::get(object, &JsValue::from_str(field_name))
        .map_err(|error| format!("the field {field_name} could not be read: {error:?}"))
}

/// Reads one required plain JavaScript object field.
pub(crate) fn required_javascript_object<'a>(
    value: &'a JsValue,
    description: &str,
) -> Result<&'a Object, String> {
    value
        .dyn_ref::<Object>()
        .ok_or_else(|| format!("{description} must be a plain object"))
}

/// Reads one required finite JavaScript number field.
fn required_javascript_number_field(object: &Object, field_name: &str) -> Result<f64, String> {
    let field_value = javascript_object_field(object, field_name)?;
    let field_number = field_value
        .as_f64()
        .ok_or_else(|| format!("the field {field_name} must be a number"))?;
    if !field_number.is_finite() {
        return Err(format!("the field {field_name} must be finite"));
    }
    Ok(field_number)
}

/// Reads one required JavaScript string field.
fn required_javascript_string_field(object: &Object, field_name: &str) -> Result<String, String> {
    javascript_object_field(object, field_name)?
        .as_string()
        .ok_or_else(|| format!("the field {field_name} must be a string"))
}

/// Validates one plain JavaScript coordinate object into its native
/// geographic coordinate.
pub(crate) fn validated_javascript_coordinate(
    value: &JsValue,
) -> Result<GeographicCoordinate, String> {
    let coordinate_object = required_javascript_object(value, "each coordinate")?;
    validated_coordinate(
        required_javascript_number_field(coordinate_object, COORDINATE_FIELD_LATITUDE)?,
        required_javascript_number_field(coordinate_object, COORDINATE_FIELD_LONGITUDE)?,
    )
    .map_err(|message| format!("invalid coordinate: {message}"))
}

/// Validates a JavaScript array of plain coordinate objects into one native
/// coordinate list.
fn coordinate_list_from_javascript(
    list_value: &JsValue,
    description: &str,
) -> Result<Vec<GeographicCoordinate>, String> {
    let coordinate_array = list_value
        .dyn_ref::<Array>()
        .ok_or_else(|| format!("{description} must be an array of coordinates"))?;
    let mut coordinates = Vec::with_capacity(coordinate_array.length() as usize);
    for coordinate_value in coordinate_array.iter() {
        coordinates.push(validated_javascript_coordinate(&coordinate_value)?);
    }
    Ok(coordinates)
}

/// Parses one plain JavaScript bounding box object into its native record.
fn bounding_box_from_javascript(value: JsValue) -> Result<BoundingBox, String> {
    let bounding_box_object = required_javascript_object(&value, "the route bounding box")?;
    Ok(BoundingBox {
        south_west: validated_javascript_coordinate(&javascript_object_field(
            bounding_box_object,
            BOUNDING_BOX_FIELD_SOUTH_WEST,
        )?)?,
        north_east: validated_javascript_coordinate(&javascript_object_field(
            bounding_box_object,
            BOUNDING_BOX_FIELD_NORTH_EAST,
        )?)?,
    })
}

/// Parses one plain JavaScript route leg object into its native record.
fn route_leg_from_javascript(leg_value: &JsValue) -> Result<RouteLeg, String> {
    let leg_object = required_javascript_object(leg_value, "each route leg")?;
    let steps_array = javascript_object_field(leg_object, ROUTE_FIELD_STEPS)?;
    let steps_values = steps_array
        .dyn_ref::<Array>()
        .ok_or_else(|| "the field steps must be an array of route steps".to_string())?;
    let mut steps = Vec::with_capacity(steps_values.length() as usize);
    for step_value in steps_values.iter() {
        let step_object = required_javascript_object(&step_value, "each route step")?;
        let maneuver_label =
            required_javascript_string_field(step_object, ROUTE_FIELD_MANEUVER_TYPE)?;
        let maneuver_type = ManeuverType::from_stable_label(&maneuver_label)
            .ok_or_else(|| format!("unrecognized step maneuver label `{maneuver_label}`"))?;
        steps.push(RouteStep {
            distance_meters: required_javascript_number_field(
                step_object,
                ROUTE_FIELD_DISTANCE_METERS,
            )?,
            duration_seconds: required_javascript_number_field(
                step_object,
                ROUTE_FIELD_DURATION_SECONDS,
            )?,
            maneuver_type,
            instruction_display_text: required_javascript_string_field(
                step_object,
                ROUTE_FIELD_INSTRUCTION_DISPLAY_TEXT,
            )?,
            instruction_speech_text: required_javascript_string_field(
                step_object,
                ROUTE_FIELD_INSTRUCTION_SPEECH_TEXT,
            )?,
            shape: coordinate_list_from_javascript(
                &javascript_object_field(step_object, ROUTE_FIELD_SHAPE)?,
                "each route step shape",
            )?,
            lane_guidances: Vec::new(),
        });
    }
    Ok(RouteLeg {
        distance_meters: required_javascript_number_field(leg_object, ROUTE_FIELD_DISTANCE_METERS)?,
        duration_seconds: required_javascript_number_field(
            leg_object,
            ROUTE_FIELD_DURATION_SECONDS,
        )?,
        steps,
    })
}

/// Parses one plain JavaScript route object into the native route the walker
/// traverses. The shape mirrors the contract's `Route` dictionary exactly.
pub(crate) fn route_data_from_javascript(route_value: &JsValue) -> Result<RouteData, String> {
    let route_object = required_javascript_object(route_value, "the route")?;
    let legs_array = javascript_object_field(route_object, ROUTE_FIELD_LEGS)?;
    let legs_values = legs_array
        .dyn_ref::<Array>()
        .ok_or_else(|| "the field legs must be an array of route legs".to_string())?;
    let mut legs = Vec::with_capacity(legs_values.length() as usize);
    for leg_value in legs_values.iter() {
        legs.push(route_leg_from_javascript(&leg_value)?);
    }
    // A host may pass a route assembled before the geometry field existed;
    // its steps still carry the geometry, so an absent field is empty.
    let geometry = match javascript_object_field(route_object, ROUTE_FIELD_GEOMETRY) {
        Ok(value) if !value.is_undefined() && !value.is_null() => {
            coordinate_list_from_javascript(&value, "the route geometry")?
        }
        _ => Vec::new(),
    };
    Ok(RouteData {
        distance_meters: required_javascript_number_field(
            route_object,
            ROUTE_FIELD_DISTANCE_METERS,
        )?,
        duration_seconds: required_javascript_number_field(
            route_object,
            ROUTE_FIELD_DURATION_SECONDS,
        )?,
        bounding_box: bounding_box_from_javascript(javascript_object_field(
            route_object,
            ROUTE_FIELD_BOUNDING_BOX,
        )?)?,
        geometry,
        legs,
    })
}

/// Parses one plain JavaScript pace object into the native pace. The walker
/// validates the values; this reader only reads them.
fn pace_from_javascript(pace_value: &JsValue) -> Result<NativeSimulatedTripPace, String> {
    let pace_object = required_javascript_object(pace_value, "the pace")?;
    Ok(NativeSimulatedTripPace {
        speed_meters_per_second: required_javascript_number_field(
            pace_object,
            SIMULATED_TRIP_PACE_FIELD_SPEED_METERS_PER_SECOND,
        )?,
        speed_multiplier: required_javascript_number_field(
            pace_object,
            SIMULATED_TRIP_PACE_FIELD_SPEED_MULTIPLIER,
        )?,
    })
}

/// Builds the plain JavaScript fix object of a native simulated-trip fix,
/// mirroring the frozen contract's `SimulatedTripFix` dictionary field for
/// field (`courseHeadingDegrees` is `null` when the route has no segment).
fn simulated_trip_fix_object(fix: &NativeSimulatedTripFix) -> Result<Object, JsValue> {
    let course_heading_degrees = match fix.course_heading_degrees {
        Some(degrees) => JsValue::from(degrees),
        None => JsValue::NULL,
    };
    object_from_fields(&[
        (
            SIMULATED_TRIP_FIX_FIELD_COORDINATE,
            geographic_coordinate_object(&fix.coordinate)?.into(),
        ),
        (
            SIMULATED_TRIP_FIX_FIELD_COURSE_HEADING_DEGREES,
            course_heading_degrees,
        ),
        (
            SIMULATED_TRIP_FIX_FIELD_TRAVELLED_METERS,
            JsValue::from(fix.travelled_meters),
        ),
        (
            SIMULATED_TRIP_FIX_FIELD_REMAINING_METERS,
            JsValue::from(fix.remaining_meters),
        ),
        (
            SIMULATED_TRIP_FIX_FIELD_ELAPSED_SECONDS,
            JsValue::from(fix.elapsed_seconds),
        ),
        (
            SIMULATED_TRIP_FIX_FIELD_IS_ARRIVED,
            JsValue::from(fix.is_arrived),
        ),
    ])
}

/// An immutable simulated trip along the planned route's own geometry,
/// mirroring the contract's `RouteSimulator`.
///
/// Construct it with a `Route`-shaped plain object (the same shape
/// `calculateRoute` answers) and a `SimulatedTripPace`-shaped plain object;
/// both fix methods then answer `SimulatedTripFix`-shaped plain objects. The
/// SDK carries no pacing policy.
#[wasm_bindgen]
pub struct RouteSimulator {
    route_walk: RouteWalk,
}

#[wasm_bindgen]
impl RouteSimulator {
    /// Builds the walker over `route` at `pace`. Throws with a message when
    /// the route carries no usable geometry or the pace cannot drive a trip.
    #[wasm_bindgen(constructor)]
    pub fn new(route: JsValue, pace: JsValue) -> Result<RouteSimulator, JsValue> {
        let native_route = route_data_from_javascript(&route)
            .map_err(|message| failure_with_code(ERROR_CODE_INVALID_NAVIGATION_INPUT, &message))?;
        let native_pace = pace_from_javascript(&pace)
            .map_err(|message| failure_with_code(ERROR_CODE_INVALID_NAVIGATION_INPUT, &message))?;
        let route_walk =
            RouteWalk::new(&native_route, native_pace).map_err(|route_walk_error| {
                failure_with_code(
                    ERROR_CODE_INVALID_NAVIGATION_INPUT,
                    &route_walk_error.to_string(),
                )
            })?;
        Ok(Self { route_walk })
    }

    /// Total length of the route's flattened polyline, in meters.
    #[wasm_bindgen(js_name = totalDistanceMeters)]
    pub fn total_distance_meters(&self) -> f64 {
        self.route_walk.total_distance_meters()
    }

    /// Total simulated travel time at the chosen pace, in seconds.
    #[wasm_bindgen(js_name = totalDurationSeconds)]
    pub fn total_duration_seconds(&self) -> f64 {
        self.route_walk.total_duration_seconds()
    }

    /// The fix at `travelled_meters` from the route start, clamped into
    /// `[0, totalDistanceMeters()]`.
    #[wasm_bindgen(js_name = fixAtTravelledMeters)]
    pub fn fix_at_travelled_meters(&self, travelled_meters: f64) -> Result<Object, JsValue> {
        simulated_trip_fix_object(&self.route_walk.fix_at_travelled_meters(travelled_meters))
    }

    /// The fix at `elapsed_seconds` of simulated travel, clamped into
    /// `[0, totalDurationSeconds()]`.
    #[wasm_bindgen(js_name = fixAtElapsedSeconds)]
    pub fn fix_at_elapsed_seconds(&self, elapsed_seconds: f64) -> Result<Object, JsValue> {
        simulated_trip_fix_object(&self.route_walk.fix_at_elapsed_seconds(elapsed_seconds))
    }
}
