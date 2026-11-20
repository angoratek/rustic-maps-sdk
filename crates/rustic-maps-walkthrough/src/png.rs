//! Minimal PNG header reading for aspect-ratio validation.
//!
//! Only the eight-byte signature and the `IHDR` width and height are needed, so
//! the crate does not carry an image decoder. A malformed header is reported as
//! a validation error rather than decoded.

/// The eight bytes every PNG document starts with.
pub const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// The chunk type whose first two fields carry the image dimensions.
pub const PNG_IHDR_CHUNK_TYPE: &[u8; 4] = b"IHDR";

const PNG_CHUNK_LENGTH_BYTES: usize = 4;
const PNG_CHUNK_TYPE_BYTES: usize = 4;
const PNG_IHDR_WIDTH_OFFSET: usize =
    PNG_SIGNATURE.len() + PNG_CHUNK_LENGTH_BYTES + PNG_CHUNK_TYPE_BYTES;
const PNG_IHDR_HEIGHT_OFFSET: usize = PNG_IHDR_WIDTH_OFFSET + 4;
const PNG_IHDR_HEADER_BYTES: usize = PNG_IHDR_HEIGHT_OFFSET + 4;

/// Reads the pixel width and height from a PNG document's header.
///
/// # Errors
/// Returns a human-readable reason when the signature or the leading `IHDR`
/// chunk is absent, the document is shorter than its header, or either
/// dimension is zero.
pub fn read_png_dimensions(bytes: &[u8]) -> Result<(u32, u32), String> {
    if bytes.len() < PNG_IHDR_HEADER_BYTES {
        return Err(format!(
            "the file is shorter than the {PNG_IHDR_HEADER_BYTES}-byte PNG header"
        ));
    }
    if bytes[..PNG_SIGNATURE.len()] != PNG_SIGNATURE {
        return Err("the PNG signature is absent".to_owned());
    }
    let chunk_type_start = PNG_SIGNATURE.len() + PNG_CHUNK_LENGTH_BYTES;
    let chunk_type_end = chunk_type_start + PNG_CHUNK_TYPE_BYTES;
    if &bytes[chunk_type_start..chunk_type_end] != PNG_IHDR_CHUNK_TYPE {
        return Err("the first chunk is not IHDR".to_owned());
    }
    let width = u32::from_be_bytes(
        bytes[PNG_IHDR_WIDTH_OFFSET..PNG_IHDR_WIDTH_OFFSET + 4]
            .try_into()
            .map_err(|_| "the width field is truncated")?,
    );
    let height = u32::from_be_bytes(
        bytes[PNG_IHDR_HEIGHT_OFFSET..PNG_IHDR_HEIGHT_OFFSET + 4]
            .try_into()
            .map_err(|_| "the height field is truncated")?,
    );
    if width == 0 || height == 0 {
        return Err("the image has a zero width or height".to_owned());
    }
    Ok((width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_header(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&PNG_SIGNATURE);
        bytes.extend_from_slice(&13_u32.to_be_bytes());
        bytes.extend_from_slice(PNG_IHDR_CHUNK_TYPE);
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        // Bit depth, colour type, compression, filter and interlace, plus the
        // CRC field; the dimension reader stops before the CRC.
        bytes.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
        bytes
    }

    #[test]
    fn reads_declared_dimensions() {
        assert_eq!(
            read_png_dimensions(&png_header(1080, 2200)),
            Ok((1080, 2200))
        );
    }

    #[test]
    fn rejects_a_non_png_document() {
        assert!(read_png_dimensions(b"not a png document at all").is_err());
    }

    #[test]
    fn rejects_a_zero_dimension() {
        assert!(read_png_dimensions(&png_header(0, 2200)).is_err());
    }
}
