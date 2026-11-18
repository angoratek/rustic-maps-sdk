//! Gesture-input camera helpers on the wasm surface, provided as
//! addition-only aliases of the `MapCamera` members that carry the same
//! operations (`panByScreenPixels`, `zoomByLevelsAroundScreenPoint`,
//! `rotateByDegreesAroundScreenPoint`). The members delegate to
//! [`rustic_maps_rendering`]'s `CameraInteraction` trait — the same math every
//! native shell applies through the frozen UDL contract — so existing call
//! sites that use the free functions keep working unchanged.
//!
//! The gesture loop itself stays on the JavaScript side: the page owns
//! pointer capture, wheel-to-zoom delta shaping, and the zoom range it
//! clamps to before calling. The camera math belongs here.

use crate::exports::MapCamera;
use js_sys::Object;
use wasm_bindgen::prelude::*;

/// Pans the camera by a pointer travel of `delta_x_pixels` x
/// `delta_y_pixels` (screen pixels), answering the resulting
/// `{latitude, longitude, zoomLevel, bearingDegrees, pitchDegrees}` pose.
/// Alias of [`MapCamera::pan_by_screen_pixels`].
#[wasm_bindgen(js_name = panCameraByScreenPixels)]
pub fn camera_pan_by_screen_pixels(
    camera: &MapCamera,
    delta_x_pixels: f64,
    delta_y_pixels: f64,
    width_pixels: f64,
    height_pixels: f64,
    field_of_view_degrees: f64,
) -> Result<Object, JsValue> {
    camera.pan_by_screen_pixels(
        delta_x_pixels,
        delta_y_pixels,
        width_pixels,
        height_pixels,
        field_of_view_degrees,
    )
}

/// Zooms the camera by `delta_zoom_levels` around the focal pixel,
/// answering the resulting pose. Alias of
/// [`MapCamera::zoom_by_levels_around_screen_point`].
#[wasm_bindgen(js_name = zoomCameraByLevelsAroundScreenPoint)]
pub fn camera_zoom_by_levels_around_screen_point(
    camera: &MapCamera,
    delta_zoom_levels: f64,
    focal_x_pixels: f64,
    focal_y_pixels: f64,
    width_pixels: f64,
    height_pixels: f64,
    field_of_view_degrees: f64,
) -> Result<Object, JsValue> {
    camera.zoom_by_levels_around_screen_point(
        delta_zoom_levels,
        focal_x_pixels,
        focal_y_pixels,
        width_pixels,
        height_pixels,
        field_of_view_degrees,
    )
}

/// Rotates the camera bearing by `delta_bearing_degrees` around the focal
/// pixel, answering the resulting pose. Alias of
/// [`MapCamera::rotate_by_degrees_around_screen_point`].
#[wasm_bindgen(js_name = rotateCameraByDegreesAroundScreenPoint)]
pub fn camera_rotate_by_degrees_around_screen_point(
    camera: &MapCamera,
    delta_bearing_degrees: f64,
    focal_x_pixels: f64,
    focal_y_pixels: f64,
    width_pixels: f64,
    height_pixels: f64,
    field_of_view_degrees: f64,
) -> Result<Object, JsValue> {
    camera.rotate_by_degrees_around_screen_point(
        delta_bearing_degrees,
        focal_x_pixels,
        focal_y_pixels,
        width_pixels,
        height_pixels,
        field_of_view_degrees,
    )
}
