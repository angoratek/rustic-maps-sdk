//! Contract tests for the SDK-owned constants the platform shells read
//! through the namespace: the renderer's default field of view, the camera's
//! maximum zoom level, and the mean-sphere meters in one degree of latitude.
//!
//! Each getter must return the renderer's own constant, so a shell that
//! reads the contract can never drift from the value the renderer uses.

use rustic_maps::{default_field_of_view_degrees, maximum_zoom_level, meters_per_degree_latitude};
use rustic_maps_rendering::camera::METERS_PER_DEGREE_LATITUDE;
use rustic_maps_rendering::{DEFAULT_FIELD_OF_VIEW_DEGREES, MAXIMUM_ZOOM_LEVEL};

#[test]
fn default_field_of_view_degrees_returns_the_renderer_default() {
    assert_eq!(
        default_field_of_view_degrees(),
        DEFAULT_FIELD_OF_VIEW_DEGREES
    );
}

#[test]
fn maximum_zoom_level_returns_the_renderer_maximum() {
    assert_eq!(maximum_zoom_level(), MAXIMUM_ZOOM_LEVEL);
}

#[test]
fn meters_per_degree_latitude_returns_the_renderer_constant() {
    assert_eq!(meters_per_degree_latitude(), METERS_PER_DEGREE_LATITUDE);
}
