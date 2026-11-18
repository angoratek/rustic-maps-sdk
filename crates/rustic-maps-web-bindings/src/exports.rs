//! The `wasm_bindgen` canvas-surface helpers: thin wrappers around the
//! rustic-maps-rendering core, returning plain JavaScript objects and arrays. The
//! naming mirrors the native API (crate `rustic-maps-rendering`, module `camera`).
//!
//! The generated contract surface lives in the `wasm2` package; this module
//! carries only the camera class and the plain-object builders the canvas and
//! the recorded-tile vector layer still read.

use crate::generated_contract_field_names::*;
use crate::shared::{validated_bounding_box, validated_coordinate, validated_viewport_pixels};
use geospatial_types::{GeographicCoordinate, SDKError};
use js_sys::{Array, Object, Reflect};
use rustic_maps_rendering::CameraInteraction;
use rustic_maps_rendering::EdgeInsetsPixels as NativeEdgeInsetsPixels;
use rustic_maps_rendering::MapCamera as NativeMapCamera;
use rustic_maps_rendering::TileKey;
use rustic_maps_rendering::fit_bounds;
use wasm_bindgen::prelude::*;

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
    /// documents.
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

/// Builds the plain `{tileX, tileY, zoomLevel}` address of a visible tile.
fn tile_address_object(tile_key: &TileKey) -> Result<Object, JsValue> {
    object_from_fields(&[
        (TILE_FIELD_TILE_X, JsValue::from(tile_key.tile_x)),
        (TILE_FIELD_TILE_Y, JsValue::from(tile_key.tile_y)),
        (TILE_FIELD_ZOOM_LEVEL, JsValue::from(tile_key.zoom_level)),
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
