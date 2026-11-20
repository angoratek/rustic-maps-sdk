//! Fixtures the workspace's test suites share.
//!
//! Test targets across the workspace build the same handful of fixtures: a
//! geographic coordinate at a pose, a viewport, the downtown Sacramento
//! anchor and a demo camera. This crate holds each one once, so a change to a
//! fixture is a single edit and every suite keeps agreeing on the values.
//!
//! The crate is development-only: it declares `publish = false`, every
//! dependent lists it under `[dev-dependencies]`, and it never enters a
//! published crate's dependency graph.
//!
//! Two coordinate constructors exist because the suites depend on both
//! behaviours. [`coordinate`] builds the record directly, so a test can hand
//! an out-of-range value to the validator under test; [`validated_coordinate`]
//! rejects out-of-range input the way the production constructor does. The
//! binding-surface record has no constructor here on purpose: this crate must
//! not depend on the native binding crate, whose exported symbols cannot be
//! linked twice into one test target. Its contract tests build that record
//! through their own shared test module.

use geospatial_types::GeographicCoordinate;
use rustic_maps_rendering::{MapCamera, ViewportPixels};

/// Sacramento's latitude in decimal degrees: the California State Capitol and
/// Capitol Park in downtown Sacramento, the anchor every demo scene uses.
///
/// The value is fixed because committed raster and vector tiles, golden
/// images and recorded routes all address that corner of the world.
pub const SACRAMENTO_LATITUDE_DEGREES: f64 = 38.5766;

/// Sacramento's longitude in decimal degrees, the other half of the demo
/// anchor.
pub const SACRAMENTO_LONGITUDE_DEGREES: f64 = -121.4944;

/// Tile column of the Sacramento anchor at zoom 14.
///
/// The committed z14 tile fixtures are addressed by this column, so a test
/// that changes it must also regenerate the fixtures.
pub const SACRAMENTO_TILE_X: u32 = 2662;

/// Tile row of the Sacramento anchor at zoom 14.
pub const SACRAMENTO_TILE_Y: u32 = 6286;

/// Zoom level of the Sacramento tile address: the level the demo scenes
/// schedule their committed tiles at.
pub const SACRAMENTO_TILE_ZOOM_LEVEL: u8 = 14;

/// Zoom level of the Sacramento demo camera, as a floating-point camera
/// level; the tile address uses the same level as [`SACRAMENTO_TILE_ZOOM_LEVEL`].
pub const SACRAMENTO_CAMERA_ZOOM_LEVEL: f64 = 14.0;

/// Width of the default test viewport in pixels.
const VIEWPORT_WIDTH_PIXELS: f64 = 1024.0;

/// Height of the default test viewport in pixels.
const VIEWPORT_HEIGHT_PIXELS: f64 = 768.0;

/// Width of the compact test viewport in pixels.
const COMPACT_VIEWPORT_WIDTH_PIXELS: f64 = 800.0;

/// Height of the compact test viewport in pixels.
const COMPACT_VIEWPORT_HEIGHT_PIXELS: f64 = 600.0;

/// Edge length of the square test viewport in pixels: a power of two so tile
/// spans map cleanly onto it.
const SQUARE_VIEWPORT_EDGE_PIXELS: f64 = 512.0;

/// Bearing of a north-up camera, in degrees.
const NORTH_UP_BEARING_DEGREES: f64 = 0.0;

/// Pitch of a top-down camera, in degrees.
const TOP_DOWN_PITCH_DEGREES: f64 = 0.0;

/// Panic detail for a fixture coordinate outside the canonical ranges.
const INVALID_FIXTURE_COORDINATE_DETAIL: &str =
    "fixture coordinates must be within the canonical ranges";

/// Builds a geographic coordinate without validating it.
///
/// Use this fixture when a test hands the coordinate to a validator under
/// test, so an out-of-range value reaches the validator instead of panicking
/// in the fixture.
///
/// # Parameters
/// - `latitude` — degrees north of the equator.
/// - `longitude` — degrees east of the prime meridian.
///
/// # Returns
/// The record carrying both values verbatim.
pub fn coordinate(latitude: f64, longitude: f64) -> GeographicCoordinate {
    GeographicCoordinate {
        latitude,
        longitude,
    }
}

/// Builds a geographic coordinate through the validating constructor.
///
/// Use this fixture when a test needs a coordinate it knows is canonical.
///
/// # Parameters
/// - `latitude` — degrees north of the equator, within `[-90.0, 90.0]`.
/// - `longitude` — degrees east of the prime meridian, within
///   `[-180.0, 180.0]`.
///
/// # Returns
/// The validated coordinate.
///
/// # Panics
/// Panics when either component is outside its canonical range, naming the
/// fixture requirement in the panic detail.
pub fn validated_coordinate(latitude: f64, longitude: f64) -> GeographicCoordinate {
    GeographicCoordinate::from_degrees(latitude, longitude)
        .expect(INVALID_FIXTURE_COORDINATE_DETAIL)
}

/// The default test viewport: 1024 by 768 pixels.
///
/// # Returns
/// A 1024 by 768 pixel viewport.
pub fn viewport() -> ViewportPixels {
    ViewportPixels::new(VIEWPORT_WIDTH_PIXELS, VIEWPORT_HEIGHT_PIXELS)
        .expect("the default test viewport is canonical")
}

/// The compact test viewport: 800 by 600 pixels, the browser camera suite's
/// shape.
///
/// # Returns
/// An 800 by 600 pixel viewport.
pub fn compact_viewport() -> ViewportPixels {
    ViewportPixels::new(
        COMPACT_VIEWPORT_WIDTH_PIXELS,
        COMPACT_VIEWPORT_HEIGHT_PIXELS,
    )
    .expect("the compact test viewport is canonical")
}

/// The square test viewport: 512 by 512 pixels, the label and overlay
/// placement suites' shape.
///
/// # Returns
/// A 512 by 512 pixel viewport.
pub fn square_viewport() -> ViewportPixels {
    ViewportPixels::new(SQUARE_VIEWPORT_EDGE_PIXELS, SQUARE_VIEWPORT_EDGE_PIXELS)
        .expect("the square test viewport is canonical")
}

/// Builds a rendering camera at an explicit pose.
///
/// The camera constructor clamps zoom, bearing and pitch into range, so a
/// fixture pose is the same value the shells would apply.
///
/// # Parameters
/// - `center` — the ground point held at the viewport center.
/// - `zoom_level` — the zoom level in tile levels.
/// - `bearing_degrees` — the clockwise rotation from north.
/// - `pitch_degrees` — the tilt from top-down.
///
/// # Returns
/// The camera at that pose after the constructor's clamping.
pub fn camera(
    center: GeographicCoordinate,
    zoom_level: f64,
    bearing_degrees: f64,
    pitch_degrees: f64,
) -> MapCamera {
    MapCamera::new(center, zoom_level, bearing_degrees, pitch_degrees)
}

/// The Sacramento demo camera: north up, top down, at the demo zoom level.
///
/// # Returns
/// The rendering camera centered on the Sacramento anchor.
///
/// # Panics
/// Panics when the shared Sacramento anchor is outside the canonical ranges,
/// which would mean the shared constants themselves are wrong.
pub fn sacramento_camera() -> MapCamera {
    camera(
        validated_coordinate(SACRAMENTO_LATITUDE_DEGREES, SACRAMENTO_LONGITUDE_DEGREES),
        SACRAMENTO_CAMERA_ZOOM_LEVEL,
        NORTH_UP_BEARING_DEGREES,
        TOP_DOWN_PITCH_DEGREES,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        SACRAMENTO_CAMERA_ZOOM_LEVEL, SACRAMENTO_LATITUDE_DEGREES, SACRAMENTO_LONGITUDE_DEGREES,
        camera, compact_viewport, coordinate, sacramento_camera, square_viewport,
        validated_coordinate, viewport,
    };

    #[test]
    fn coordinate_carries_the_values_verbatim() {
        let unvalidated = coordinate(-200.0, 400.0);
        assert_eq!(unvalidated.latitude, -200.0);
        assert_eq!(unvalidated.longitude, 400.0);
    }

    #[test]
    fn validated_coordinate_carries_canonical_values() {
        let validated =
            validated_coordinate(SACRAMENTO_LATITUDE_DEGREES, SACRAMENTO_LONGITUDE_DEGREES);
        assert_eq!(validated.latitude, SACRAMENTO_LATITUDE_DEGREES);
        assert_eq!(validated.longitude, SACRAMENTO_LONGITUDE_DEGREES);
    }

    #[test]
    #[should_panic(expected = "fixture coordinates must be within the canonical ranges")]
    fn validated_coordinate_rejects_out_of_range_input() {
        let _ = validated_coordinate(91.0, 0.0);
    }

    #[test]
    fn viewports_carry_their_documented_extents() {
        let default = viewport();
        assert_eq!(
            (default.width_pixels, default.height_pixels),
            (1024.0, 768.0)
        );

        let compact = compact_viewport();
        assert_eq!(
            (compact.width_pixels, compact.height_pixels),
            (800.0, 600.0)
        );

        let square = square_viewport();
        assert_eq!((square.width_pixels, square.height_pixels), (512.0, 512.0));
    }

    #[test]
    fn camera_keeps_the_requested_pose() {
        let pose = camera(coordinate(47.0, 8.5), 12.0, 30.0, 20.0);
        assert_eq!(pose.center, coordinate(47.0, 8.5));
        assert_eq!(pose.zoom_level, 12.0);
        assert_eq!(pose.bearing_degrees, 30.0);
        assert_eq!(pose.pitch_degrees, 20.0);
    }

    #[test]
    fn sacramento_camera_is_the_demo_pose() {
        let demo = sacramento_camera();
        assert_eq!(
            (demo.center.latitude, demo.center.longitude),
            (SACRAMENTO_LATITUDE_DEGREES, SACRAMENTO_LONGITUDE_DEGREES)
        );
        assert_eq!(demo.zoom_level, SACRAMENTO_CAMERA_ZOOM_LEVEL);
        assert_eq!(demo.bearing_degrees, 0.0);
        assert_eq!(demo.pitch_degrees, 0.0);
    }
}
