//! The Mapbox glyph PBF reader and its typed model.
//!
//! One fetched glyph range is the `glyphs` message of the Mapbox glyph
//! protocol: a list of font stacks, each carrying its name, its range label
//! (`"0-255"`, …) and its glyphs. Each glyph carries a Unicode codepoint,
//! its ink-box metrics and an optional signed-distance-field bitmap whose
//! border is
//! [`GLYPH_SIGNED_DISTANCE_FIELD_BORDER_PIXELS`] pixels wide on every side. The reader reuses
//! the protobuf wire reader of `rustic-maps-vector-tiles`; every malformed path is a
//! typed [`GlyphError`], never a panic.
//!
//! The parser accepts whichever codepoints a producer serves. Text shaping —
//! bidirectional ordering, ligatures and complex-script cluster layout — is
//! explicitly out of scope; the label pipeline handles Latin text first and
//! nothing here guesses at a shaping algorithm.

use crate::error::GlyphError;
use rustic_maps_vector_tiles::VectorTileError;
use rustic_maps_vector_tiles::wire::{
    WIRE_TYPE_LENGTH_DELIMITED, WIRE_TYPE_VARINT, WireReader, zigzag_decode,
};

/// The size of the signed-distance-field border the glyph protocol puts
/// around every glyph's ink box, in pixels.
///
/// The bitmap of a glyph with a `width` × `height` ink box is therefore
/// `(width + 2 × GLYPH_SIGNED_DISTANCE_FIELD_BORDER_PIXELS)` ×
/// `(height + 2 × GLYPH_SIGNED_DISTANCE_FIELD_BORDER_PIXELS)` pixels, one `R8Unorm` byte each.
pub const GLYPH_SIGNED_DISTANCE_FIELD_BORDER_PIXELS: u32 = 3;

/// The number of codepoints one glyph range covers. Range labels name a
/// 256-codepoint block (`"0-255"`, `"256-511"`, …).
pub const GLYPH_RANGE_CODEPOINT_COUNT: u32 = 256;

/// The bit mask selecting the first codepoint of the 256-codepoint block a
/// codepoint belongs to.
const GLYPH_RANGE_FIRST_CODEPOINT_MASK: u32 = GLYPH_RANGE_CODEPOINT_COUNT - 1;

/// One 256-codepoint block of glyphs, the unit a glyph URL requests with its
/// `{range}` placeholder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphRange {
    /// The range's first codepoint (inclusive).
    pub first_codepoint: u32,
    /// The range's last codepoint (inclusive).
    pub last_codepoint: u32,
}

impl GlyphRange {
    /// Basic Latin plus Latin-1 Supplement (`U+0000`–`U+00FF`).
    pub const BASIC_LATIN: Self = Self::new(0x0000, 0x00FF);
    /// The protocol's second Latin block (`U+0100`–`U+01FF`): Latin
    /// Extended-A with the first half of Latin Extended-B, the unit a glyph
    /// URL names as `256-511`.
    pub const LATIN_EXTENDED: Self = Self::new(0x0100, 0x01FF);

    /// Builds an ordered codepoint span. Callers supply a first codepoint at
    /// or below the last; [`GlyphRange::containing`] is the checked builder
    /// for an arbitrary codepoint.
    pub const fn new(first_codepoint: u32, last_codepoint: u32) -> Self {
        Self {
            first_codepoint,
            last_codepoint,
        }
    }

    /// The 256-codepoint block that contains `codepoint`.
    pub const fn containing(codepoint: u32) -> Self {
        let first_codepoint = codepoint & !GLYPH_RANGE_FIRST_CODEPOINT_MASK;
        Self::new(
            first_codepoint,
            first_codepoint | GLYPH_RANGE_FIRST_CODEPOINT_MASK,
        )
    }

    /// Whether `codepoint` falls inside this range.
    pub const fn contains(&self, codepoint: u32) -> bool {
        codepoint >= self.first_codepoint && codepoint <= self.last_codepoint
    }

    /// Whether `codepoint` belongs to the Latin ranges the atlas packs first:
    /// Basic Latin with Latin-1 Supplement, or the second Latin block.
    pub const fn is_latin_codepoint(codepoint: u32) -> bool {
        Self::BASIC_LATIN.contains(codepoint) || Self::LATIN_EXTENDED.contains(codepoint)
    }

    /// The range's URL segment (`"0-255"`, `"256-511"`, …), the value of the
    /// glyph URL template's `{range}` placeholder.
    pub fn url_segment(&self) -> String {
        format!("{}-{}", self.first_codepoint, self.last_codepoint)
    }
}

/// One decoded glyph: its codepoint, ink-box metrics and optional
/// signed-distance-field bitmap.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Glyph {
    /// The glyph's Unicode codepoint (the protocol's `id` field).
    pub codepoint: u32,
    /// The signed-distance-field bitmap, one byte per pixel, row-major. Empty
    /// when the producer omits the bitmap (as it does for a space).
    pub bitmap: Vec<u8>,
    /// The ink-box width in pixels, excluding the signed-distance-field border.
    pub width: u32,
    /// The ink-box height in pixels, excluding the signed-distance-field border.
    pub height: u32,
    /// The horizontal bearing from the pen origin to the ink box, in pixels.
    pub left: i32,
    /// The vertical bearing from the baseline to the ink box, in pixels.
    pub top: i32,
    /// The horizontal advance to the next pen position, in pixels.
    pub advance: u32,
}

impl Glyph {
    /// The padded signed-distance-field bitmap width in pixels: the ink-box width plus the
    /// border on both sides.
    pub fn bitmap_width_pixels(&self) -> u64 {
        u64::from(self.width) + 2 * u64::from(GLYPH_SIGNED_DISTANCE_FIELD_BORDER_PIXELS)
    }

    /// The padded signed-distance-field bitmap height in pixels: the ink-box height plus the
    /// border on both sides.
    pub fn bitmap_height_pixels(&self) -> u64 {
        u64::from(self.height) + 2 * u64::from(GLYPH_SIGNED_DISTANCE_FIELD_BORDER_PIXELS)
    }

    /// The byte length a present signed-distance-field bitmap must have, or `None` when the
    /// padded dimensions overflow a 64-bit byte count.
    pub fn expected_bitmap_byte_count(&self) -> Option<u64> {
        self.bitmap_width_pixels()
            .checked_mul(self.bitmap_height_pixels())
    }

    /// The bytes this glyph retains: its bitmap bytes plus the inline glyph
    /// record. Allocator slack is deliberately not counted.
    pub fn retained_byte_count(&self) -> usize {
        self.bitmap.len() + std::mem::size_of::<Self>()
    }
}

/// One decoded font stack: the font family list the glyphs were rendered
/// from, the range label the payload covers and the decoded glyphs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GlyphFontStack {
    /// The font stack's name (a comma-separated family list).
    pub name: String,
    /// The range label the payload declares (`"0-255"`, …).
    pub range: String,
    /// The decoded glyphs, in payload order.
    pub glyphs: Vec<Glyph>,
}

impl GlyphFontStack {
    /// Finds a glyph by codepoint.
    pub fn glyph(&self, codepoint: u32) -> Option<&Glyph> {
        self.glyphs
            .iter()
            .find(|glyph| glyph.codepoint == codepoint)
    }

    /// The bytes this font stack retains: its name and range text, its glyph
    /// records and their bitmaps, plus the inline stack record. Allocator
    /// slack is deliberately not counted.
    pub fn retained_byte_count(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.name.len()
            + self.range.len()
            + self.glyphs.len() * std::mem::size_of::<Glyph>()
            + self
                .glyphs
                .iter()
                .map(|glyph| glyph.bitmap.len())
                .sum::<usize>()
    }
}

/// Decodes one fetched glyph range (the `glyphs` protobuf message).
///
/// A range response carries exactly one font stack; a payload with zero or
/// several reports [`GlyphError::UnexpectedFontStackCount`]. Unknown fields
/// are skipped for forward compatibility, and every known field is checked
/// for its wire type before it is read.
pub fn decode_glyph_font_stack(bytes: &[u8]) -> Result<GlyphFontStack, GlyphError> {
    let mut reader = WireReader::new(bytes);
    let mut font_stacks = Vec::new();
    while !reader.at_end() {
        let field_key = reader.read_field_key("glyph response")?;
        match (field_key.field_number, field_key.wire_type) {
            (1, WIRE_TYPE_LENGTH_DELIMITED) => {
                let font_stack_bytes = reader.read_length_delimited("glyph response font stack")?;
                font_stacks.push(decode_font_stack(font_stack_bytes)?);
            }
            (1, wire_type) => {
                return Err(unsupported_wire_type(1, wire_type, "glyph response"));
            }
            _ => reader.skip_field(field_key, "glyph response")?,
        }
    }
    if font_stacks.len() != 1 {
        return Err(GlyphError::UnexpectedFontStackCount {
            found: font_stacks.len(),
        });
    }
    Ok(font_stacks.remove(0))
}

/// Decodes one `fontstack` message.
fn decode_font_stack(bytes: &[u8]) -> Result<GlyphFontStack, GlyphError> {
    let mut reader = WireReader::new(bytes);
    let mut name = None;
    let mut range = None;
    let mut glyphs = Vec::new();
    while !reader.at_end() {
        let field_key = reader.read_field_key("font stack")?;
        match (field_key.field_number, field_key.wire_type) {
            (1, WIRE_TYPE_LENGTH_DELIMITED) => {
                let name_bytes = reader.read_length_delimited("font stack name")?;
                name = Some(decode_utf8(name_bytes, "font stack name")?);
            }
            (2, WIRE_TYPE_LENGTH_DELIMITED) => {
                let range_bytes = reader.read_length_delimited("font stack range")?;
                range = Some(decode_utf8(range_bytes, "font stack range")?);
            }
            (3, WIRE_TYPE_LENGTH_DELIMITED) => {
                let glyph_bytes = reader.read_length_delimited("font stack glyph")?;
                glyphs.push(decode_glyph(glyph_bytes)?);
            }
            (1..=3, wire_type) => {
                return Err(unsupported_wire_type(
                    field_key.field_number,
                    wire_type,
                    "font stack",
                ));
            }
            _ => reader.skip_field(field_key, "font stack")?,
        }
    }
    Ok(GlyphFontStack {
        name: name.ok_or(GlyphError::MissingFontStackName)?,
        range: range.ok_or(GlyphError::MissingFontStackRange)?,
        glyphs,
    })
}

/// Decodes one `glyph` message and validates its bitmap length.
fn decode_glyph(bytes: &[u8]) -> Result<Glyph, GlyphError> {
    let mut reader = WireReader::new(bytes);
    let mut codepoint = None;
    let mut bitmap = Vec::new();
    let mut bitmap_was_present = false;
    let mut width = None;
    let mut height = None;
    let mut left = None;
    let mut top = None;
    let mut advance = None;
    while !reader.at_end() {
        let field_key = reader.read_field_key("glyph")?;
        match (field_key.field_number, field_key.wire_type) {
            (1, WIRE_TYPE_VARINT) => {
                codepoint = Some(read_unsigned_field(&mut reader, "id")?);
            }
            (2, WIRE_TYPE_LENGTH_DELIMITED) => {
                let bitmap_bytes = reader.read_length_delimited("glyph bitmap")?;
                bitmap = bitmap_bytes.to_vec();
                bitmap_was_present = true;
            }
            (3, WIRE_TYPE_VARINT) => {
                width = Some(read_unsigned_field(&mut reader, "width")?);
            }
            (4, WIRE_TYPE_VARINT) => {
                height = Some(read_unsigned_field(&mut reader, "height")?);
            }
            (5, WIRE_TYPE_VARINT) => {
                left = Some(read_signed_field(&mut reader, "left")?);
            }
            (6, WIRE_TYPE_VARINT) => {
                top = Some(read_signed_field(&mut reader, "top")?);
            }
            (7, WIRE_TYPE_VARINT) => {
                advance = Some(read_unsigned_field(&mut reader, "advance")?);
            }
            (1..=7, wire_type) => {
                return Err(unsupported_wire_type(
                    field_key.field_number,
                    wire_type,
                    "glyph",
                ));
            }
            _ => reader.skip_field(field_key, "glyph")?,
        }
    }
    let glyph = Glyph {
        codepoint: codepoint.ok_or(GlyphError::MissingGlyphField { field: "id" })?,
        bitmap,
        width: width.ok_or(GlyphError::MissingGlyphField { field: "width" })?,
        height: height.ok_or(GlyphError::MissingGlyphField { field: "height" })?,
        left: left.ok_or(GlyphError::MissingGlyphField { field: "left" })?,
        top: top.ok_or(GlyphError::MissingGlyphField { field: "top" })?,
        advance: advance.ok_or(GlyphError::MissingGlyphField { field: "advance" })?,
    };
    if bitmap_was_present {
        let expected_byte_count = glyph.expected_bitmap_byte_count().ok_or(
            GlyphError::GlyphBitmapDimensionsOverflow {
                codepoint: glyph.codepoint,
                width: glyph.width,
                height: glyph.height,
            },
        )?;
        let actual_byte_count = glyph.bitmap.len() as u64;
        if expected_byte_count != actual_byte_count {
            return Err(GlyphError::GlyphBitmapLengthMismatch {
                codepoint: glyph.codepoint,
                width: glyph.width,
                height: glyph.height,
                expected_byte_count,
                actual_byte_count,
            });
        }
    }
    Ok(glyph)
}

/// Reads a varint field that must fit into a `u32`.
fn read_unsigned_field(
    reader: &mut WireReader<'_>,
    field: &'static str,
) -> Result<u32, GlyphError> {
    let value = reader.read_varint(field)?;
    u32::try_from(value).map_err(|_| GlyphError::UnsignedValueOutOfRange { field, value })
}

/// Reads a ZigZag-encoded `sint32` field.
fn read_signed_field(reader: &mut WireReader<'_>, field: &'static str) -> Result<i32, GlyphError> {
    let value = reader.read_varint(field)?;
    let signed_value = zigzag_decode(value);
    i32::try_from(signed_value).map_err(|_| GlyphError::SignedValueOutOfRange {
        field,
        value: signed_value,
    })
}

/// Decodes a UTF-8 string field.
fn decode_utf8(bytes: &[u8], context: &'static str) -> Result<String, GlyphError> {
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| GlyphError::MalformedProtobuf {
            source: VectorTileError::InvalidUtf8 { context },
        })
}

/// Builds the typed wire-format refusal for a known field with an unexpected
/// wire type.
fn unsupported_wire_type(field_number: u32, wire_type: u64, context: &'static str) -> GlyphError {
    GlyphError::MalformedProtobuf {
        source: VectorTileError::UnsupportedWireType {
            field_number,
            wire_type,
            context,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Appends the base-128 varint encoding of `value`.
    fn append_varint(value: u64, bytes: &mut Vec<u8>) {
        let mut rest = value;
        loop {
            let byte = (rest & 0x7f) as u8;
            rest >>= 7;
            if rest == 0 {
                bytes.push(byte);
                break;
            }
            bytes.push(byte | 0x80);
        }
    }

    /// Appends a field key.
    fn append_field_key(field_number: u32, wire_type: u64, bytes: &mut Vec<u8>) {
        append_varint((u64::from(field_number) << 3) | wire_type, bytes);
    }

    /// Appends a varint field.
    fn append_varint_field(field_number: u32, value: u64, bytes: &mut Vec<u8>) {
        append_field_key(field_number, WIRE_TYPE_VARINT, bytes);
        append_varint(value, bytes);
    }

    /// Appends a length-delimited field.
    fn append_length_delimited_field(field_number: u32, payload: &[u8], bytes: &mut Vec<u8>) {
        append_field_key(field_number, WIRE_TYPE_LENGTH_DELIMITED, bytes);
        append_varint(payload.len() as u64, bytes);
        bytes.extend_from_slice(payload);
    }

    /// The ZigZag encoding of a signed value, the mirror of the decoder.
    fn zigzag_encode(value: i64) -> u64 {
        ((value as u64) << 1) ^ ((value >> 63) as u64)
    }

    /// The signed-distance-field byte count for an ink box.
    fn signed_distance_field_byte_count(width: u32, height: u32) -> usize {
        ((width as usize) + 2 * GLYPH_SIGNED_DISTANCE_FIELD_BORDER_PIXELS as usize)
            * ((height as usize) + 2 * GLYPH_SIGNED_DISTANCE_FIELD_BORDER_PIXELS as usize)
    }

    /// Encodes one glyph, omitting any field whose closure returns `None`.
    fn encode_glyph(
        codepoint: u32,
        width: u32,
        height: u32,
        left: i32,
        top: i32,
        advance: u32,
        bitmap: Option<&[u8]>,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        append_varint_field(1, u64::from(codepoint), &mut bytes);
        if let Some(bitmap_bytes) = bitmap {
            append_length_delimited_field(2, bitmap_bytes, &mut bytes);
        }
        append_varint_field(3, u64::from(width), &mut bytes);
        append_varint_field(4, u64::from(height), &mut bytes);
        append_varint_field(5, zigzag_encode(i64::from(left)), &mut bytes);
        append_varint_field(6, zigzag_encode(i64::from(top)), &mut bytes);
        append_varint_field(7, u64::from(advance), &mut bytes);
        bytes
    }

    /// Encodes one font stack.
    fn encode_font_stack(name: &str, range: &str, glyphs: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = Vec::new();
        append_length_delimited_field(1, name.as_bytes(), &mut bytes);
        append_length_delimited_field(2, range.as_bytes(), &mut bytes);
        for glyph in glyphs {
            append_length_delimited_field(3, glyph, &mut bytes);
        }
        bytes
    }

    /// Encodes one glyph response around the supplied font stacks.
    fn encode_glyph_response(font_stacks: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for font_stack in font_stacks {
            append_length_delimited_field(1, font_stack, &mut bytes);
        }
        bytes
    }

    /// A glyph whose bitmap matches its declared ink box.
    fn valid_glyph(codepoint: u32) -> Vec<u8> {
        let width = 3;
        let height = 5;
        let bitmap = vec![0x7f; signed_distance_field_byte_count(width, height)];
        encode_glyph(codepoint, width, height, 1, 2, 6, Some(&bitmap))
    }

    #[test]
    fn decodes_one_font_stack_with_glyph_metrics_and_bitmap() {
        let bitmap = vec![0x40; signed_distance_field_byte_count(3, 5)];
        let glyph = encode_glyph(0x41, 3, 5, -1, 2, 7, Some(&bitmap));
        let font_stack = encode_font_stack("Noto Sans Regular", "0-255", &[glyph]);
        let response = encode_glyph_response(&[font_stack]);

        let decoded = decode_glyph_font_stack(&response).expect("the payload is well formed");
        assert_eq!(decoded.name, "Noto Sans Regular");
        assert_eq!(decoded.range, "0-255");
        assert_eq!(decoded.glyphs.len(), 1);
        let decoded_glyph = &decoded.glyphs[0];
        assert_eq!(decoded_glyph.codepoint, 0x41);
        assert_eq!(decoded_glyph.width, 3);
        assert_eq!(decoded_glyph.height, 5);
        assert_eq!(decoded_glyph.left, -1);
        assert_eq!(decoded_glyph.top, 2);
        assert_eq!(decoded_glyph.advance, 7);
        assert_eq!(decoded_glyph.bitmap, bitmap);
        assert_eq!(decoded_glyph.bitmap_width_pixels(), 9);
        assert_eq!(decoded_glyph.bitmap_height_pixels(), 11);
        assert_eq!(
            decoded_glyph.expected_bitmap_byte_count(),
            Some(bitmap.len() as u64)
        );
    }

    #[test]
    fn a_zero_size_glyph_may_omit_its_bitmap() {
        let glyph = encode_glyph(0x20, 0, 0, 0, 0, 4, None);
        let font_stack = encode_font_stack("Noto Sans Regular", "0-255", &[glyph]);
        let decoded =
            decode_glyph_font_stack(&encode_glyph_response(&[font_stack])).expect("valid");

        assert_eq!(decoded.glyphs[0].bitmap, Vec::<u8>::new());
        assert_eq!(decoded.glyphs[0].advance, 4);
    }

    #[test]
    fn a_bitmap_must_match_the_padded_signed_distance_field_dimensions() {
        let glyph = encode_glyph(0x41, 3, 5, 0, 0, 6, Some(&[0u8; 3]));
        let font_stack = encode_font_stack("Noto Sans Regular", "0-255", &[glyph]);
        let failure = decode_glyph_font_stack(&encode_glyph_response(&[font_stack]))
            .expect_err("the bitmap length is wrong");

        assert_eq!(
            failure,
            GlyphError::GlyphBitmapLengthMismatch {
                codepoint: 0x41,
                width: 3,
                height: 5,
                expected_byte_count: 99,
                actual_byte_count: 3,
            }
        );
    }

    #[test]
    fn missing_required_fields_report_typed_errors() {
        // A glyph with the bitmap and width but no height.
        let mut glyph = Vec::new();
        append_varint_field(1, 0x41, &mut glyph);
        append_varint_field(3, 3, &mut glyph);
        let font_stack = encode_font_stack("Noto Sans Regular", "0-255", &[glyph]);
        let failure = decode_glyph_font_stack(&encode_glyph_response(&[font_stack]))
            .expect_err("height is missing");

        assert_eq!(failure, GlyphError::MissingGlyphField { field: "height" });

        // A font stack without a range.
        let mut range_less = Vec::new();
        append_length_delimited_field(1, b"Noto Sans Regular", &mut range_less);
        let failure = decode_glyph_font_stack(&encode_glyph_response(&[range_less]))
            .expect_err("range is missing");
        assert_eq!(failure, GlyphError::MissingFontStackRange);
    }

    #[test]
    fn a_payload_with_zero_or_several_font_stacks_is_refused() {
        let failure = decode_glyph_font_stack(&[]).expect_err("no font stack");
        assert_eq!(failure, GlyphError::UnexpectedFontStackCount { found: 0 });

        let first = encode_font_stack("First", "0-255", &[]);
        let second = encode_font_stack("Second", "0-255", &[]);
        let failure = decode_glyph_font_stack(&encode_glyph_response(&[first, second]))
            .expect_err("two font stacks");
        assert_eq!(failure, GlyphError::UnexpectedFontStackCount { found: 2 });
    }

    #[test]
    fn a_known_field_with_the_wrong_wire_type_is_refused() {
        let mut glyph = Vec::new();
        append_varint_field(1, 0x41, &mut glyph);
        append_length_delimited_field(3, b"not-a-varint", &mut glyph);
        let font_stack = encode_font_stack("Noto Sans Regular", "0-255", &[glyph]);
        let failure = decode_glyph_font_stack(&encode_glyph_response(&[font_stack]))
            .expect_err("width arrived length-delimited");

        assert!(matches!(
            failure,
            GlyphError::MalformedProtobuf {
                source: VectorTileError::UnsupportedWireType {
                    field_number: 3,
                    ..
                }
            }
        ));
    }

    #[test]
    fn utf8_errors_are_typed() {
        let mut font_stack = Vec::new();
        append_length_delimited_field(1, &[0xff, 0xfe], &mut font_stack);
        append_length_delimited_field(2, b"0-255", &mut font_stack);
        let failure = decode_glyph_font_stack(&encode_glyph_response(&[font_stack]))
            .expect_err("the name is not UTF-8");

        assert!(matches!(
            failure,
            GlyphError::MalformedProtobuf {
                source: VectorTileError::InvalidUtf8 { .. }
            }
        ));
    }

    #[test]
    fn unknown_fields_are_skipped_for_forward_compatibility() {
        let mut glyph = valid_glyph(0x41);
        append_varint_field(9, 42, &mut glyph);
        append_length_delimited_field(10, b"extension", &mut glyph);
        let mut font_stack = encode_font_stack("Noto Sans Regular", "0-255", &[glyph]);
        append_varint_field(9, 7, &mut font_stack);
        let mut response = encode_glyph_response(&[font_stack]);
        append_length_delimited_field(16, b"extension", &mut response);

        let decoded = decode_glyph_font_stack(&response).expect("unknown fields are skipped");
        assert_eq!(decoded.glyphs.len(), 1);
        assert_eq!(decoded.glyphs[0].codepoint, 0x41);
    }

    #[test]
    fn out_of_range_unsigned_and_signed_values_are_refused() {
        let mut glyph = valid_glyph(0x41);
        // Append a second `advance` field carrying a value above u32::MAX;
        // the last value wins and must be refused.
        append_varint_field(7, u64::from(u32::MAX) + 1, &mut glyph);
        let font_stack = encode_font_stack("Noto Sans Regular", "0-255", &[glyph]);
        let failure = decode_glyph_font_stack(&encode_glyph_response(&[font_stack]))
            .expect_err("advance exceeds u32");
        assert_eq!(
            failure,
            GlyphError::UnsignedValueOutOfRange {
                field: "advance",
                value: u64::from(u32::MAX) + 1,
            }
        );
    }

    #[test]
    fn ranges_name_their_256_codepoint_block() {
        assert_eq!(GlyphRange::containing(0x41), GlyphRange::BASIC_LATIN);
        assert_eq!(GlyphRange::containing(0xFF), GlyphRange::BASIC_LATIN);
        assert_eq!(GlyphRange::containing(0x100).first_codepoint, 0x100);
        assert_eq!(GlyphRange::containing(0x2FF).first_codepoint, 0x200);
        assert_eq!(GlyphRange::BASIC_LATIN.url_segment(), "0-255");
        assert_eq!(GlyphRange::LATIN_EXTENDED.url_segment(), "256-511");
        assert_eq!(GlyphRange::LATIN_EXTENDED, GlyphRange::containing(0x100));
        assert!(GlyphRange::is_latin_codepoint(0x41));
        assert!(GlyphRange::is_latin_codepoint(0x17F));
        assert!(GlyphRange::is_latin_codepoint(0x1FF));
        assert!(!GlyphRange::is_latin_codepoint(0x410));
    }

    #[test]
    fn retained_byte_count_includes_the_bitmap() {
        let bitmap = vec![0u8; signed_distance_field_byte_count(3, 5)];
        let glyph = encode_glyph(0x41, 3, 5, 0, 0, 6, Some(&bitmap));
        let font_stack = encode_font_stack("Noto Sans Regular", "0-255", &[glyph]);
        let decoded = decode_glyph_font_stack(&encode_glyph_response(&[font_stack])).unwrap();

        let expected = std::mem::size_of::<GlyphFontStack>()
            + decoded.name.len()
            + decoded.range.len()
            + std::mem::size_of::<Glyph>()
            + bitmap.len();
        assert_eq!(decoded.retained_byte_count(), expected);
    }

    use proptest::prelude::*;

    proptest! {
        #[test]
        fn arbitrary_bytes_decode_without_panicking(
            bytes in prop::collection::vec(any::<u8>(), 0..2048),
        ) {
            if let Ok(font_stack) = decode_glyph_font_stack(&bytes) {
                for glyph in &font_stack.glyphs {
                    if !glyph.bitmap.is_empty() {
                        prop_assert_eq!(
                            glyph.bitmap.len() as u64,
                            glyph.expected_bitmap_byte_count().unwrap(),
                        );
                    }
                }
            }
        }
    }
}
