//! Golden-image comparison shared by the render harnesses.
//!
//! Eight integration harnesses render a scene into a [`CapturedFrame`] and
//! compare the pixels against a committed PNG under
//! `fixtures/render/golden/`. They share this module so the fixture path, the
//! tolerance model, the deliberate regeneration and the drift message behave
//! identically everywhere.
//!
//! A comparison is deliberately tolerant, never pixel-exact: GPUs differ in
//! sampler behavior. A frame passes when both its mean absolute per-channel
//! error and its fraction of outlier pixels stay within its
//! [`GoldenTolerance`].
//!
//! A missing fixture fails the test unless `UPDATE_GOLDENS=1` is set; only
//! then is the fixture written from the captured frame, for review and commit.
//! An existing fixture is always compared, never silently rewritten.

use rustic_maps_rendering::CapturedFrame;
use std::path::{Path, PathBuf};

/// Directory of the golden fixtures, relative to the crate manifest.
pub const GOLDEN_DIRECTORY: &str = "fixtures/render/golden";

/// Environment variable that requests deliberate fixture regeneration.
pub const UPDATE_GOLDENS_VARIABLE: &str = "UPDATE_GOLDENS";

/// Value of [`UPDATE_GOLDENS_VARIABLE`] that requests regeneration.
pub const UPDATE_GOLDENS_REQUESTED_VALUE: &str = "1";

/// Number of channels in a captured RGBA pixel.
const PIXEL_CHANNELS: usize = 4;

/// The tolerated difference between a rendered frame and its golden fixture.
#[derive(Clone, Copy, Debug)]
pub struct GoldenTolerance {
    /// Mean absolute per-channel error tolerated, on the 0..255 scale.
    pub mean_absolute_error: f64,
    /// Per-pixel maximum channel error above which a pixel is an outlier.
    pub outlier_channel_error: f64,
    /// Fraction of pixels that may be outliers.
    pub outlier_pixel_fraction: f64,
}

impl GoldenTolerance {
    /// The tolerance every render harness uses.
    ///
    /// Sampler differences gather along tile edges and steep gradients, never
    /// across the whole frame, so a small outlier fraction passes while the
    /// mean error stays close to zero.
    pub const STANDARD: Self = Self {
        mean_absolute_error: 2.0,
        outlier_channel_error: 24.0,
        outlier_pixel_fraction: 0.02,
    };
}

/// Whether this run requests deliberate golden regeneration.
pub fn golden_regeneration_is_requested() -> bool {
    std::env::var(UPDATE_GOLDENS_VARIABLE)
        .is_ok_and(|value| value == UPDATE_GOLDENS_REQUESTED_VALUE)
}

/// Absolute path of the named golden fixture.
pub fn golden_path(golden_file_name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(GOLDEN_DIRECTORY)
        .join(golden_file_name)
}

/// Compares a captured frame against its golden fixture.
///
/// A missing fixture fails the test unless `UPDATE_GOLDENS=1` is set; only
/// then is it written from the captured frame and the run returns. An
/// existing fixture is read and compared with `tolerance`. A drift beyond the
/// tolerance fails with one message naming the fixture path, the
/// differing-pixel count and the tolerance that applied.
pub fn compare_or_create_golden(
    captured_frame: &CapturedFrame,
    golden_file_name: &str,
    scene_description: &str,
    tolerance: GoldenTolerance,
) {
    let golden_path = golden_path(golden_file_name);
    if !golden_path.exists() {
        if !golden_regeneration_is_requested() {
            panic!(
                "golden fixture for {scene_description} is missing at {golden_path:?}; regenerate \
                 deliberately with UPDATE_GOLDENS=1 and commit the result"
            );
        }
        write_golden(&golden_path, captured_frame);
        println!(
            "golden fixture for {scene_description} was missing; wrote {golden_path:?} because \
             UPDATE_GOLDENS=1 is set - review and commit it"
        );
        return;
    }

    let golden_image = image::open(&golden_path)
        .unwrap_or_else(|error| panic!("golden fixture {golden_path:?} is unreadable: {error}"))
        .to_rgba8();
    assert_eq!(
        (golden_image.width(), golden_image.height()),
        (captured_frame.width_pixels, captured_frame.height_pixels),
        "golden fixture {golden_path:?} has a different size than the rendered frame"
    );

    let golden_bytes = golden_image.as_raw();
    let pixel_count = captured_frame.width_pixels * captured_frame.height_pixels;
    let mut absolute_error_sum: f64 = 0.0;
    let mut differing_pixel_count: u64 = 0;
    for (captured_pixel, golden_pixel) in captured_frame
        .rgba_bytes
        .as_chunks::<PIXEL_CHANNELS>()
        .0
        .iter()
        .zip(golden_bytes.as_chunks::<PIXEL_CHANNELS>().0)
    {
        let mut pixel_maximum_error: f64 = 0.0;
        for channel in 0..PIXEL_CHANNELS {
            let channel_error = f64::from(captured_pixel[channel].abs_diff(golden_pixel[channel]));
            absolute_error_sum += channel_error;
            pixel_maximum_error = pixel_maximum_error.max(channel_error);
        }
        if pixel_maximum_error > tolerance.outlier_channel_error {
            differing_pixel_count += 1;
        }
    }

    let mean_absolute_error = absolute_error_sum / (f64::from(pixel_count) * PIXEL_CHANNELS as f64);
    let outlier_pixel_fraction = differing_pixel_count as f64 / f64::from(pixel_count);
    println!(
        "{scene_description}: mean absolute channel error {mean_absolute_error:.4} \
         (limit {}), outlier fraction {outlier_pixel_fraction:.5} (limit {})",
        tolerance.mean_absolute_error, tolerance.outlier_pixel_fraction
    );
    assert!(
        mean_absolute_error <= tolerance.mean_absolute_error
            && outlier_pixel_fraction <= tolerance.outlier_pixel_fraction,
        "golden mismatch for {scene_description} at {golden_path:?}: \
         {differing_pixel_count} of {pixel_count} pixels differ by more than \
         {} channel levels (fraction {outlier_pixel_fraction:.5}, limit {}); \
         mean absolute channel error {mean_absolute_error:.4} (limit {})",
        tolerance.outlier_channel_error,
        tolerance.outlier_pixel_fraction,
        tolerance.mean_absolute_error
    );
}

/// Writes the captured frame as its golden fixture, creating its directory.
fn write_golden(golden_path: &Path, captured_frame: &CapturedFrame) {
    std::fs::create_dir_all(
        golden_path
            .parent()
            .expect("the golden fixture path has a parent directory"),
    )
    .expect("the golden fixture directory is writable");
    image::save_buffer(
        golden_path,
        &captured_frame.rgba_bytes,
        captured_frame.width_pixels,
        captured_frame.height_pixels,
        image::ColorType::Rgba8,
    )
    .expect("the golden fixture is written deterministically");
}
