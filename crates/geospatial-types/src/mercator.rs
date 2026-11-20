//! The Web Mercator projection's latitude and tile-grid limits.

/// Latitude the Web Mercator projection can represent, in decimal degrees.
///
/// The projected world square is finite in latitude: coordinates beyond this
/// limit are clamped to it by every projection, source and tile calculation
/// in the SDK, so this is the one value those layers share.
pub const MAXIMUM_MERCATOR_LATITUDE_DEGREES: f64 = 85.051_128_779_806_6;

/// Highest zoom level the XYZ tile grid can address, in tile levels.
///
/// A grid at zoom level `z` counts `2^z` tiles along each axis, and the SDK
/// holds that count in a `u64`. Shifting a `u64` left by 64 overflows the
/// type, so zoom level 64 and above cannot be represented; this is the last
/// usable level. Projection, camera tile selection and tile-address
/// validation all bound their zoom input by it.
pub const MAXIMUM_TILE_ZOOM_LEVEL: u8 = 63;

#[cfg(test)]
mod tests {
    use super::MAXIMUM_MERCATOR_LATITUDE_DEGREES;

    /// The exact projection limit is `atan(sinh(pi))` in degrees; the
    /// constant carries the correctly rounded value.
    const PROJECTION_LIMIT_TOLERANCE_DEGREES: f64 = 1e-12;

    #[test]
    fn maximum_latitude_matches_the_projection_limit() {
        let exact_limit = std::f64::consts::PI.sinh().atan().to_degrees();
        assert!(
            (MAXIMUM_MERCATOR_LATITUDE_DEGREES - exact_limit).abs()
                < PROJECTION_LIMIT_TOLERANCE_DEGREES
        );
    }
}
