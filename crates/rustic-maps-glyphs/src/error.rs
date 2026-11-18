//! The typed failure vocabulary of the glyph PBF reader.
//!
//! Glyph ranges arrive from provider fetches on untrusted paths, so every
//! malformed payload — truncated buffers, impossible wire keys, missing
//! required fields, out-of-range metrics, a bitmap whose length disagrees
//! with its declared dimensions — surfaces as a [`GlyphError`]. The reader
//! never panics and never guesses a partial decode.

use rustic_maps_vector_tiles::VectorTileError;
use thiserror::Error;

/// A failure the Mapbox glyph PBF reader encountered while turning wire
/// bytes into [`crate::GlyphFontStack`] structures.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GlyphError {
    /// The payload violates the protobuf wire format; the wrapped
    /// [`VectorTileError`] names the exact malformation.
    #[error("malformed glyph protobuf: {source}")]
    MalformedProtobuf {
        /// The underlying wire-format failure.
        #[from]
        source: VectorTileError,
    },
    /// The payload carries no font stack, or more than one. One fetched
    /// range response carries exactly one font stack.
    #[error("glyph response carries {found} font stacks; one range response carries exactly one")]
    UnexpectedFontStackCount {
        /// How many font stacks the payload carried.
        found: usize,
    },
    /// A font stack carries no `name` field; the glyph schema requires it.
    #[error("font stack is missing its required name field")]
    MissingFontStackName,
    /// A font stack carries no `range` field; the glyph schema requires it.
    #[error("font stack is missing its required range field")]
    MissingFontStackRange,
    /// A glyph carries none of the required metric fields.
    #[error("glyph is missing its required {field} field")]
    MissingGlyphField {
        /// The protobuf field's documented name (`id`, `width`, `height`,
        /// `left`, `top` or `advance`).
        field: &'static str,
    },
    /// A glyph's bitmap length disagrees with the signed-distance-field
    /// dimensions its metrics imply (a 3-pixel border around the ink box).
    #[error(
        "glyph {codepoint} bitmap holds {actual_byte_count} bytes but its {width}×{height} ink box requires {expected_byte_count} signed-distance-field bytes"
    )]
    GlyphBitmapLengthMismatch {
        /// The glyph's Unicode codepoint.
        codepoint: u32,
        /// The declared ink-box width in pixels.
        width: u32,
        /// The declared ink-box height in pixels.
        height: u32,
        /// The byte length the signed-distance-field bitmap must have.
        expected_byte_count: u64,
        /// The byte length the payload actually carried.
        actual_byte_count: u64,
    },
    /// A glyph's padded bitmap dimensions overflow addressable memory.
    #[error(
        "glyph {codepoint} bitmap dimensions overflow addressable memory ({width}×{height} ink box)"
    )]
    GlyphBitmapDimensionsOverflow {
        /// The glyph's Unicode codepoint.
        codepoint: u32,
        /// The declared ink-box width in pixels.
        width: u32,
        /// The declared ink-box height in pixels.
        height: u32,
    },
    /// An unsigned glyph field does not fit into its 32-bit protobuf type.
    #[error("glyph {field} value {value} exceeds the 32-bit range")]
    UnsignedValueOutOfRange {
        /// The protobuf field's documented name.
        field: &'static str,
        /// The decoded wire value.
        value: u64,
    },
    /// A signed glyph field does not fit into its 32-bit protobuf type.
    #[error("glyph {field} value {value} exceeds the signed 32-bit range")]
    SignedValueOutOfRange {
        /// The protobuf field's documented name.
        field: &'static str,
        /// The decoded signed value.
        value: i64,
    },
}
