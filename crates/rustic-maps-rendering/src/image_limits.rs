//! Decode bounds shared by every image payload the SDK decodes.
//!
//! Raster tiles, sprite sheets and recorded tile payloads are untrusted
//! input: a small compressed image can declare dimensions that expand to
//! gigabytes when decoded. [`decode_image_with_limits`] therefore applies
//! the named bounds below at every decode site instead of relying on the
//! decoder's own default allocation cap.

use std::io::Cursor;

use image::{DynamicImage, ImageError, ImageReader, Limits};

/// The widest image the SDK decodes, in pixels.
///
/// Raster tiles are 256 or 512 pixels wide and the largest sprite sheets the
/// style specification allows are a few thousand pixels wide; the bound
/// leaves room for those while refusing pathological declarations.
pub const MAXIMUM_IMAGE_WIDTH_PIXELS: u32 = 8192;

/// The tallest image the SDK decodes, in pixels.
pub const MAXIMUM_IMAGE_HEIGHT_PIXELS: u32 = 8192;

/// The most memory a single decode may allocate, in bytes.
///
/// Decoded RGBA is four bytes per pixel, so this bound refuses (for example)
/// a 8192 by 8192 image even though it satisfies the dimension bounds. It
/// also covers any intermediate buffers the decoder allocates.
pub const MAXIMUM_IMAGE_ALLOCATION_BYTES: u64 = 64 * 1024 * 1024;

/// Why an encoded image payload could not be decoded.
#[derive(Debug)]
pub enum ImageDecodeError {
    /// The payload declares dimensions, or needs an allocation, beyond the
    /// bounds named in this module.
    LimitsExceeded(image::error::LimitError),
    /// The bytes are not a supported image or do not decode.
    Malformed(ImageError),
}

impl std::fmt::Display for ImageDecodeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LimitsExceeded(limit_error) => {
                write!(
                    formatter,
                    "the image exceeds the decode limits: {limit_error}"
                )
            }
            Self::Malformed(image_error) => {
                write!(formatter, "the image could not be decoded: {image_error}")
            }
        }
    }
}

impl std::error::Error for ImageDecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::LimitsExceeded(limit_error) => Some(limit_error),
            Self::Malformed(image_error) => Some(image_error),
        }
    }
}

impl From<ImageError> for ImageDecodeError {
    fn from(image_error: ImageError) -> Self {
        match image_error {
            ImageError::Limits(limit_error) => Self::LimitsExceeded(limit_error),
            other_image_error => Self::Malformed(other_image_error),
        }
    }
}

/// The decode bounds this module declares, as the decoder's limit structure.
fn decode_limits() -> Limits {
    // The structure is non-exhaustive, so the bounds are set on a
    // fully-populated value rather than through a struct expression.
    let mut limits = Limits::no_limits();
    limits.max_image_width = Some(MAXIMUM_IMAGE_WIDTH_PIXELS);
    limits.max_image_height = Some(MAXIMUM_IMAGE_HEIGHT_PIXELS);
    limits.max_alloc = Some(MAXIMUM_IMAGE_ALLOCATION_BYTES);
    limits
}

/// Decodes an encoded image payload within the bounds named in this module.
///
/// The format is guessed from the payload's own signature. A payload outside
/// the bounds is refused with [`ImageDecodeError::LimitsExceeded`] before the
/// decoder allocates its pixels; a payload that does not decode is reported
/// as [`ImageDecodeError::Malformed`].
///
/// # Errors
/// Reports [`ImageDecodeError`] when the payload exceeds the decode bounds or
/// is not a supported image.
pub fn decode_image_with_limits(
    encoded_image_bytes: &[u8],
) -> Result<DynamicImage, ImageDecodeError> {
    let reader = ImageReader::new(Cursor::new(encoded_image_bytes))
        .with_guessed_format()
        .map_err(ImageError::from)?;
    let mut limited_reader = reader;
    limited_reader.limits(decode_limits());
    limited_reader.decode().map_err(ImageDecodeError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, Rgba, RgbaImage};

    /// Encodes a solid image of the given size as PNG.
    fn encoded_png(width_pixels: u32, height_pixels: u32) -> Vec<u8> {
        let image = RgbaImage::from_pixel(width_pixels, height_pixels, Rgba([0, 0, 0, 255]));
        let mut encoded = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(image)
            .write_to(&mut encoded, ImageFormat::Png)
            .expect("the test PNG encodes");
        encoded.into_inner()
    }

    #[test]
    fn an_image_within_the_limits_decodes() {
        let decoded = decode_image_with_limits(&encoded_png(2, 2))
            .expect("a 2 by 2 image is inside every decode bound");
        assert_eq!((decoded.width(), decoded.height()), (2, 2));
    }

    #[test]
    fn an_over_wide_image_is_refused_with_the_limits_failure() {
        let over_wide = encoded_png(MAXIMUM_IMAGE_WIDTH_PIXELS + 1, 1);
        match decode_image_with_limits(&over_wide) {
            Err(ImageDecodeError::LimitsExceeded(limit_error)) => assert_eq!(
                limit_error.kind(),
                image::error::LimitErrorKind::DimensionError
            ),
            other => {
                panic!("an over-wide image must be refused by the dimension bound, got {other:?}")
            }
        }
    }

    #[test]
    fn an_image_over_the_allocation_bound_is_refused_rather_than_allocated() {
        // 4097 by 4097 RGBA needs 67 MB, over the 64 MB allocation bound but
        // inside the dimension bounds, so this exercises the second bound.
        let allocation_over = encoded_png(4097, 4097);
        match decode_image_with_limits(&allocation_over) {
            Err(ImageDecodeError::LimitsExceeded(limit_error)) => assert_eq!(
                limit_error.kind(),
                image::error::LimitErrorKind::InsufficientMemory
            ),
            other => panic!("an image over the allocation bound must be refused, got {other:?}"),
        }
    }
}
