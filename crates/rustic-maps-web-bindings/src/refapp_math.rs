//! Host-testable value shaping for the reference-application math.
//!
//! [`map_refapp_core`] owns the behavior; this module only spells its answers
//! in the field and label shapes the JavaScript seam exchanges, so the wasm
//! attributes in [`crate::refapp`] stay thin and the shaping itself is covered
//! by host tests. Every value here delegates to the crate; nothing is
//! recomputed.

use map_refapp_core::{
    FollowPhase, PreTripCamera, SelectedScaleBarScale,
    trace_dispatch_interval_milliseconds as native_trace_dispatch_interval_milliseconds,
};

/// One chosen scale-bar scale in the seam's lower-camel-case field spelling.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScaleBarSelectionValue {
    /// The bar's labelled distance in metres.
    pub distance_meters: f64,
    /// The bar's physical width for that distance, in viewport pixels.
    pub width_pixels: f64,
}

impl From<SelectedScaleBarScale> for ScaleBarSelectionValue {
    fn from(selection: SelectedScaleBarScale) -> Self {
        Self {
            distance_meters: selection.distance_meters,
            width_pixels: selection.width_pixels,
        }
    }
}

/// The camera values captured at trip start, in the seam's field spelling.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreTripCameraValue {
    /// Camera centre latitude in degrees.
    pub center_latitude_degrees: f64,
    /// Camera centre longitude in degrees.
    pub center_longitude_degrees: f64,
    /// Camera zoom level.
    pub zoom_level: f64,
    /// Camera bearing in degrees clockwise from north.
    pub bearing_degrees: f64,
    /// Camera pitch in degrees.
    pub pitch_degrees: f64,
}

impl From<PreTripCamera> for PreTripCameraValue {
    fn from(camera: PreTripCamera) -> Self {
        Self {
            center_latitude_degrees: camera.center_latitude_degrees,
            center_longitude_degrees: camera.center_longitude_degrees,
            zoom_level: camera.zoom_level,
            bearing_degrees: camera.bearing_degrees,
            pitch_degrees: camera.pitch_degrees,
        }
    }
}

/// Selects the scale-bar ladder distance using the design language's token
/// width range.
pub fn select_scale_bar_scale(
    ground_meters_per_screen_pixel: f64,
) -> Option<ScaleBarSelectionValue> {
    map_refapp_core::select_scale_bar_scale_with_token_width_range(ground_meters_per_screen_pixel)
        .map(ScaleBarSelectionValue::from)
}

/// Selects the scale-bar ladder distance that fits an explicit width range.
pub fn select_scale_bar_scale_with_width_range(
    ground_meters_per_screen_pixel: f64,
    minimum_width_pixels: f64,
    maximum_width_pixels: f64,
) -> Option<ScaleBarSelectionValue> {
    map_refapp_core::select_scale_bar_scale(
        ground_meters_per_screen_pixel,
        minimum_width_pixels,
        maximum_width_pixels,
    )
    .map(ScaleBarSelectionValue::from)
}

/// The selected ladder distance's metric label.
pub fn scale_bar_distance_label_text(distance_meters: f64) -> String {
    map_refapp_core::scale_bar_distance_label_text(distance_meters)
}

/// The replay dispatch interval as a JavaScript number.
///
/// The crate answers whole milliseconds as `u64`; `wasm-bindgen` maps that to
/// a JavaScript `BigInt`, while the page's timer, tests and arithmetic all use
/// `number`. The value is far below the precise-integer bound, so the
/// conversion is exact.
pub fn trace_dispatch_interval_milliseconds(
    recorded_sample_interval_seconds: f64,
    playback_acceleration_factor: f64,
) -> f64 {
    native_trace_dispatch_interval_milliseconds(
        recorded_sample_interval_seconds,
        playback_acceleration_factor,
    ) as f64
}

/// The page-facing label of a follow-camera phase.
pub fn follow_phase_label(phase: FollowPhase) -> &'static str {
    match phase {
        FollowPhase::Disengaged => "disengaged",
        FollowPhase::Engaged => "engaged",
        FollowPhase::BrokenByGesture => "brokenByGesture",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PreTripCameraValue, ScaleBarSelectionValue, follow_phase_label,
        scale_bar_distance_label_text, select_scale_bar_scale,
        select_scale_bar_scale_with_width_range, trace_dispatch_interval_milliseconds,
    };
    use map_refapp_core::{
        FollowPhase, PreTripCamera,
        trace_dispatch_interval_milliseconds as native_trace_dispatch_interval_milliseconds,
    };

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() <= 1.0e-9,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn scale_bar_wrappers_match_the_crate() {
        for ground in [0.5, 1.0, 3.0, 10.0, 250.0] {
            let exported = select_scale_bar_scale(ground).expect("a usable ground answer selects");
            let native = map_refapp_core::select_scale_bar_scale_with_token_width_range(ground)
                .expect("the crate selects the same answer");
            assert_close(exported.distance_meters, native.distance_meters);
            assert_close(exported.width_pixels, native.width_pixels);
            assert_close(
                ScaleBarSelectionValue::from(native).distance_meters,
                native.distance_meters,
            );
        }
        assert_eq!(select_scale_bar_scale(0.0), None);
        assert_eq!(select_scale_bar_scale(f64::NAN), None);
    }

    #[test]
    fn explicit_width_range_wrapper_matches_the_crate() {
        let exported = select_scale_bar_scale_with_width_range(1.0, 10.0, 20.0)
            .expect("a fitting ladder distance exists");
        let native = map_refapp_core::select_scale_bar_scale(1.0, 10.0, 20.0)
            .expect("the crate selects the same answer");
        assert_close(exported.distance_meters, native.distance_meters);
        assert_close(exported.width_pixels, native.width_pixels);
        assert_eq!(
            select_scale_bar_scale_with_width_range(1.0, 120.0, 50.0),
            None
        );
    }

    #[test]
    fn scale_bar_label_matches_the_crate() {
        for distance in [0.0, 999.9, 1000.0, 2500.0] {
            assert_eq!(
                scale_bar_distance_label_text(distance),
                map_refapp_core::scale_bar_distance_label_text(distance)
            );
        }
    }

    #[test]
    fn trace_dispatch_interval_matches_the_crate_as_a_number() {
        for (recorded_seconds, acceleration) in [(5.0, 20.0), (1.0, 1.0), (0.001, 1.0), (5.0, 0.0)]
        {
            assert_close(
                trace_dispatch_interval_milliseconds(recorded_seconds, acceleration),
                native_trace_dispatch_interval_milliseconds(recorded_seconds, acceleration) as f64,
            );
        }
    }

    #[test]
    fn follow_phase_labels_are_the_page_names() {
        assert_eq!(follow_phase_label(FollowPhase::Disengaged), "disengaged");
        assert_eq!(follow_phase_label(FollowPhase::Engaged), "engaged");
        assert_eq!(
            follow_phase_label(FollowPhase::BrokenByGesture),
            "brokenByGesture"
        );
    }

    #[test]
    fn pre_trip_camera_value_matches_the_crate() {
        let native = PreTripCamera {
            center_latitude_degrees: 38.0,
            center_longitude_degrees: -121.0,
            zoom_level: 12.0,
            bearing_degrees: 15.0,
            pitch_degrees: 0.0,
        };
        assert_eq!(
            PreTripCameraValue::from(native),
            PreTripCameraValue {
                center_latitude_degrees: 38.0,
                center_longitude_degrees: -121.0,
                zoom_level: 12.0,
                bearing_degrees: 15.0,
                pitch_degrees: 0.0,
            }
        );
    }
}
