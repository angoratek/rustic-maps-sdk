//! The scale bar's clean-distance ladder.
//!
//! From the camera's ground-meters-per-screen-pixel answer the bar selects the
//! largest ladder distance whose physical width does not exceed the token
//! maximum, provided that width also reaches the token minimum. The ladder is
//! the conventional 1-2-3-5-10 sequence per decade.

/// The clean-distance ladder's multipliers, per decade.
pub const SCALE_BAR_LADDER_MULTIPLIERS: [f64; 5] = [1.0, 2.0, 3.0, 5.0, 10.0];

/// The ladder's lower decade scan bound around one metre.
///
/// `10⁻² … 10⁹` metres spans every usable camera zoom, from deep-zoom
/// centimetres to a zoom-zero hemisphere bar.
pub const SCALE_BAR_LADDER_LOWER_DECADE_SCAN_RANGE: i32 = 2;

/// The ladder's upper decade scan bound around one metre.
pub const SCALE_BAR_LADDER_DECADE_SCAN_RANGE: i32 = 9;

/// The scale bar's width floor in pixels.
pub const SCALE_BAR_WIDTH_MINIMUM_PIXELS: f64 = 50.0;

/// The scale bar's width ceiling in pixels.
pub const SCALE_BAR_WIDTH_MAXIMUM_PIXELS: f64 = 120.0;

/// One metre in kilometres, the boundary between the bar's two metric labels.
pub const METERS_PER_KILOMETER: f64 = 1000.0;

/// One chosen scale-bar scale: the labelled distance and the bar's physical
/// width.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelectedScaleBarScale {
    /// The bar's labelled distance in metres.
    pub distance_meters: f64,
    /// The bar's physical width for that distance, in viewport pixels.
    pub width_pixels: f64,
}

/// Selects the ladder distance that fits an explicit width range.
///
/// Returns `None` when the ground answer or the width range is not usable; the
/// bar then renders no distance rather than inventing one.
pub fn select_scale_bar_scale(
    ground_meters_per_screen_pixel: f64,
    minimum_width_pixels: f64,
    maximum_width_pixels: f64,
) -> Option<SelectedScaleBarScale> {
    if !ground_meters_per_screen_pixel.is_finite() || ground_meters_per_screen_pixel <= 0.0 {
        return None;
    }
    if !minimum_width_pixels.is_finite()
        || !maximum_width_pixels.is_finite()
        || minimum_width_pixels < 0.0
        || maximum_width_pixels <= 0.0
        || minimum_width_pixels > maximum_width_pixels
    {
        return None;
    }

    let decade_count = (SCALE_BAR_LADDER_DECADE_SCAN_RANGE
        + SCALE_BAR_LADDER_LOWER_DECADE_SCAN_RANGE
        + 1) as usize;
    let mut candidates = Vec::with_capacity(decade_count * SCALE_BAR_LADDER_MULTIPLIERS.len());
    for decade_exponent in
        -SCALE_BAR_LADDER_LOWER_DECADE_SCAN_RANGE..=SCALE_BAR_LADDER_DECADE_SCAN_RANGE
    {
        let decade_scale = 10.0_f64.powi(decade_exponent);
        for multiplier in SCALE_BAR_LADDER_MULTIPLIERS {
            candidates.push(multiplier * decade_scale);
        }
    }
    candidates.sort_by(f64::total_cmp);

    let mut selected = None;
    for candidate_distance_meters in candidates {
        let candidate_width_pixels = candidate_distance_meters / ground_meters_per_screen_pixel;
        if candidate_width_pixels > maximum_width_pixels {
            break;
        }
        selected = Some(SelectedScaleBarScale {
            distance_meters: candidate_distance_meters,
            width_pixels: candidate_width_pixels,
        });
    }
    selected.filter(|scale| scale.width_pixels >= minimum_width_pixels)
}

/// Selects the ladder distance using the design language's token width range.
pub fn select_scale_bar_scale_with_token_width_range(
    ground_meters_per_screen_pixel: f64,
) -> Option<SelectedScaleBarScale> {
    select_scale_bar_scale(
        ground_meters_per_screen_pixel,
        SCALE_BAR_WIDTH_MINIMUM_PIXELS,
        SCALE_BAR_WIDTH_MAXIMUM_PIXELS,
    )
}

/// The ladder distance's metric label: whole metres under a kilometre,
/// rounded kilometres otherwise.
pub fn scale_bar_distance_label_text(distance_meters: f64) -> String {
    if distance_meters < METERS_PER_KILOMETER {
        format!("{} m", distance_meters.trunc() as i64)
    } else {
        format!(
            "{} km",
            (distance_meters / METERS_PER_KILOMETER).round() as i64
        )
    }
}
