//! Hand-written browser canvas surface for the Rustic Maps SDK.
//!
//! The frozen UniFFI contract is generated for the browser by
//! `uniffi-bindgen-react-native`'s `wasm2` target; this crate supplies only the
//! pieces that contract cannot express: the GPU canvas (`BrowserMapView`,
//! `MapCamera`
//! and its pose algebra), the recorded-tile and vector-layer helpers, and the
//! small provider slots and fixture-endpoint bridge the canvas reads. It is
//! compiled into the generated module as `browser_canvas`.
//!
//! The exported JavaScript surface lives in `exports` and `canvas`, which
//! only compile under `wasm32`; the shared validation and tile plumbing stays
//! here so host tests exercise it without a browser.

// The shared plumbing is consumed by the wasm exports and the host tests, so
// it only compiles on those configurations and a plain host library build
// stays empty of unused code.
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) mod shared;

// The reference-application behavior math: the value shaping is host-tested,
// and the `wasm_bindgen` attributes over it compile for the browser target.
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) mod refapp_math;

#[cfg(target_arch = "wasm32")]
pub mod canvas;
#[cfg(target_arch = "wasm32")]
pub mod exports;
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) mod generated_contract_field_names;
#[cfg(target_arch = "wasm32")]
pub mod live_providers;
#[cfg(target_arch = "wasm32")]
pub mod refapp;
#[cfg(target_arch = "wasm32")]
pub mod vector_layer;
// The optional declaration fields of a browser URL-template vector source:
// the canvas and its host-runnable tests share one validation and declaration
// builder.
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) mod vector_source_fields;
// The browser camera state store and contract-member math: host-tested so
// `cargo test` exercises the same fit, interpolation and bounds rules the
// wasm `MapCamera` and `BrowserMapView` wrappers carry.
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) mod web_camera;

/// Semver of the compiled SDK, equal to the workspace package version.
pub const SDK_COMPILED_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::{SDK_COMPILED_VERSION, shared};

    #[test]
    fn sdk_compiled_version_is_non_empty() {
        assert!(!SDK_COMPILED_VERSION.trim().is_empty());
    }

    #[test]
    fn validated_coordinate_accepts_canonical_degrees() {
        let coordinate =
            shared::validated_coordinate(38.5766, -121.4944).expect("canonical degrees are valid");
        assert!((coordinate.latitude - 38.5766).abs() < f64::EPSILON);
        assert!((coordinate.longitude - -121.4944).abs() < f64::EPSILON);
    }

    #[test]
    fn validated_coordinate_rejects_out_of_range_input() {
        assert!(shared::validated_coordinate(95.0, 0.0).is_err());
        assert!(shared::validated_coordinate(0.0, -200.0).is_err());
    }

    #[test]
    fn validated_viewport_rejects_non_positive_extents() {
        assert!(shared::validated_viewport_pixels(0.0, 600.0).is_err());
        assert!(shared::validated_viewport_pixels(800.0, -1.0).is_err());
    }

    #[test]
    fn validated_viewport_accepts_positive_extents() {
        let viewport =
            shared::validated_viewport_pixels(800.0, 600.0).expect("positive extents are valid");
        assert_eq!(viewport.width_pixels, 800.0);
        assert_eq!(viewport.height_pixels, 600.0);
    }

    #[test]
    fn validated_bounding_box_accepts_ordered_corners() {
        let bounding_box = shared::validated_bounding_box(45.0, 5.0, 49.0, 11.0)
            .expect("ordered corners are valid");
        assert_eq!(bounding_box.south_west.latitude, 45.0);
        assert_eq!(bounding_box.north_east.longitude, 11.0);
    }

    #[test]
    fn validated_bounding_box_rejects_inverted_corners() {
        assert!(shared::validated_bounding_box(49.0, 11.0, 45.0, 5.0).is_err());
    }
}
