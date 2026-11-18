//! The typed failure vocabulary of the Mapbox Vector Tile decoder.
//!
//! Every malformed input — truncated buffers, impossible wire keys,
//! out-of-range dictionary indices, invalid geometry command streams — is
//! surfaced as a [`VectorTileError`]; the decoder never panics and never
//! partially guesses, because these bytes arrive from network fetches on
//! untrusted paths.

use thiserror::Error;

/// A failure the Mapbox Vector Tile v2.1 decoder encountered while turning
/// wire bytes into [`crate::decoder::VectorTile`] structures.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum VectorTileError {
    /// The byte stream ended before a value this position requires.
    #[error("tile is truncated: {context}")]
    TruncatedBuffer {
        /// Where in the decode the stream ended early.
        context: &'static str,
    },
    /// A varint uses more than the 10 bytes a 64-bit value can occupy.
    #[error("varint uses more than {maximum_byte_count} bytes: {context}")]
    VarintByteCountExceeded {
        /// The maximum varint byte count (10 for 64-bit values).
        maximum_byte_count: u32,
        /// Where in the decode the varint was read.
        context: &'static str,
    },
    /// A varint's value does not fit into 64 bits (the 10th byte's
    /// continuation payload exceeds the remaining addressable bits).
    #[error("varint value exceeds 64 bits: {context}")]
    VarintValueExceeds64Bits {
        /// Where in the decode the varint was read.
        context: &'static str,
    },
    /// A protobuf field key has field number 0 or a wire type the
    /// decoder does not recognize (0 varint, 1 64-bit, 2 length-delimited
    /// and 5 32-bit are the only valid ones).
    #[error("invalid protobuf field key (field {field_number}, wire type {wire_type}): {context}")]
    InvalidWireField {
        /// The decoded field number.
        field_number: u32,
        /// The decoded wire type.
        wire_type: u64,
        /// Where in the decode the key was read.
        context: &'static str,
    },
    /// A field the decoder knows about arrived with a wire type it cannot
    /// carry; such payloads would be silently misread, so they are refused.
    #[error(
        "field {field_number} arrived with wire type {wire_type}, which cannot carry it: {context}"
    )]
    UnsupportedWireType {
        /// The field number whose wire type is inapplicable.
        field_number: u32,
        /// The inapplicable wire type.
        wire_type: u64,
        /// Where in the decode the field was read.
        context: &'static str,
    },
    /// A length-delimited field claims more bytes than the buffer holds.
    #[error("length-delimited field claims {claimed_length} bytes but {context} holds fewer")]
    LengthDelimitedOverrun {
        /// The claimed byte length.
        claimed_length: u64,
        /// Where in the decode the field was read.
        context: &'static str,
    },
    /// A string field the decoder reads as UTF-8 is not valid UTF-8.
    #[error("bytes are not valid UTF-8: {context}")]
    InvalidUtf8 {
        /// Where in the decode the string was read.
        context: &'static str,
    },
    /// A layer's version field is missing or names an unsupported MVT
    /// major version; the v2.1 specification requires exactly `2`.
    #[error(
        "layer version {found} is unsupported; the Mapbox Vector Tile specification requires version 2"
    )]
    UnsupportedLayerVersion {
        /// The version value the layer carried (an absent field decodes
        /// to the protobuf default of 1).
        found: u32,
    },
    /// A layer carries no `name` field; the specification makes it required.
    #[error("layer is missing its required name field")]
    MissingLayerName,
    /// Two layers in one tile share a name; the specification makes layer
    /// names unique within a tile.
    #[error("two layers share the name \"{name}\"")]
    DuplicateLayerName {
        /// The duplicated name.
        name: String,
    },
    /// A feature's `tags` field holds an odd number of integers; tags are
    /// key/value index pairs and must come in pairs.
    #[error("feature tags hold {found} integers, which is not a multiple of 2")]
    TagElementCountNotEven {
        /// The odd element count.
        found: u64,
    },
    /// A tag's key or value index points past its layer's dictionary.
    #[error(
        "attribute dictionary index {index} is out of range for {context} holding {dictionary_size} entries"
    )]
    AttributeIndexOutOfRange {
        /// The out-of-range dictionary index.
        index: u64,
        /// Whether the failing index selects a key or a value.
        context: &'static str,
        /// The actual dictionary size the index was checked against.
        dictionary_size: u64,
    },
    /// A layer attribute value carries none of the seven value variants;
    /// the specification requires exactly one.
    #[error("attribute value carries none of its seven value fields")]
    AttributeValueVariantMissing,
    /// A layer attribute value carries more than one of the seven value
    /// variants; the specification requires exactly one.
    #[error("attribute value carries conflicting value fields (field repeat)")]
    AttributeValueVariantConflicting,
    /// A feature declares geometry type `UNKNOWN` (0) or a type value
    /// beyond the specification's enum; no compliant producer emits these.
    #[error(
        "feature declares geometry type {found}, which the specification's GeomType enum does not use"
    )]
    UnknownGeometryType {
        /// The type value the feature declared.
        found: u32,
    },
    /// A geometry command integer names no command (`MoveTo`, `LineTo`,
    /// `ClosePath`).
    #[error("geometry command id {command_id} is unknown")]
    UnknownGeometryCommand {
        /// The unknown command id.
        command_id: u64,
    },
    /// A geometry command integer carries a count of 0; commands always
    /// process at least one occurrence.
    #[error("geometry command {command_id} carries a count of 0")]
    GeometryCommandCountOfZero {
        /// The command id carrying the zero count.
        command_id: u64,
    },
    /// A geometry command carries a count beyond the 29-bit range the
    /// specification's packing admits.
    #[error(
        "geometry command carries a count of {found}, beyond the 29-bit range the specification admits"
    )]
    GeometryCommandCountOutOfRange {
        /// The out-of-range count.
        found: u64,
    },
    /// A `ClosePath` command's count is not 1; the specification requires
    /// exactly one occurrence per ring closure.
    #[error("ClosePath carries count {found}; the specification requires exactly 1")]
    ClosePathCommandCountNotOne {
        /// The count the command integer declared.
        found: u64,
    },
    /// The geometry parameter stream ended before the pending command's
    /// declared parameter count was fulfilled.
    #[error("geometry parameter integers ran out for command {context}")]
    GeometryParameterStreamTruncated {
        /// Which command was waiting for parameters.
        context: &'static str,
    },
    /// A `MoveTo` with a command count greater than 1 appeared inside a
    /// geometry kind that requires exactly one vertex per `MoveTo`.
    #[error(
        "MoveTo with count {count} inside a {geometry_kind}; only the POINT geometry kind accepts multi-vertex MoveTo commands"
    )]
    MultiVertexMoveToOutsidePoints {
        /// The declared count.
        count: u64,
        /// The geometry kind the command appeared in.
        geometry_kind: &'static str,
    },
    /// A geometry command appeared whose parameter/semantic shape its
    /// geometry kind does not allow (a `LineTo` or `ClosePath` in a POINT
    /// feature, a `ClosePath` in a LINESTRING feature).
    #[error("geometry command {command_id} is not allowed inside a {geometry_kind}")]
    GeometryCommandNotAllowed {
        /// The disallowed command id.
        command_id: u64,
        /// The geometry kind the command appeared in.
        geometry_kind: &'static str,
    },
    /// A `LineTo` appeared before the stream's first `MoveTo`; there is no
    /// previous cursor vertex to continue from.
    #[error("LineTo appears before the first MoveTo of its linear path")]
    LineToBeforeMoveTo,
    /// A `LineTo` carried the `(0, 0)` delta, which the specification
    /// forbids (zero-length segments are not representable).
    #[error("LineTo carries the (0, 0) delta; zero-length segments are forbidden")]
    ZeroDeltaLineTo,
    /// A polygon's command stream ended with a ring still open (no
    /// `ClosePath`); polygon rings are always explicitly closed.
    #[error("polygon command stream ends with an unclosed ring (no ClosePath)")]
    PolygonRingUnclosed,
    /// Accumulating a geometry coordinate's delta overflowed the signed
    /// 64-bit cursor.
    #[error("geometry coordinate delta overflowed the 64-bit cursor")]
    GeometryCoordinateOverflow,
    /// A geometry or tags field arrived unpacked; the specification pins
    /// both to the packed encoding, and an unpacked stream would otherwise
    /// decode to an empty geometry or attribute set silently.
    #[error(
        "field {field_number} arrived unpacked (wire type {wire_type}); the specification pins packed encoding"
    )]
    PackedFieldUnpacked {
        /// The field number that must travel packed.
        field_number: u32,
        /// The wire type it arrived with.
        wire_type: u64,
    },
}
