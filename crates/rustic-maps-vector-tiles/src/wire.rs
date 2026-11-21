//! The protobuf wire-format reader the tile decoder is built on.
//!
//! The reader is public because the workspace's glyph PBF reader
//! (`rustic-maps-glyphs`) reuses it instead of carrying a second protobuf
//! implementation; it is still not an FFI type and never appears in the
//! frozen UDL contract.
//!
//! A single pass, zero-copy, borrow-only reader: it hands out borrowed
//! slices for length-delimited fields and never allocates in the
//! hot path of the decode. Only the wire primitives the MVT and glyph
//! schemas use are implemented — varints, fixed-width scalars and
//! length-delimited bytes — with every malformed input surfacing a typed
//! [`VectorTileError`] instead of a panic.

use crate::error::VectorTileError;

/// Wire type 0: the base-128 varint.
pub const WIRE_TYPE_VARINT: u64 = 0;
/// Wire type 1: a 64-bit little-endian value.
pub const WIRE_TYPE_FIXED64: u64 = 1;
/// Wire type 2: a length-prefixed byte string.
pub const WIRE_TYPE_LENGTH_DELIMITED: u64 = 2;
/// Wire type 5: a 32-bit little-endian value.
pub const WIRE_TYPE_FIXED32: u64 = 5;

/// The most bytes a 64-bit varint occupies (7 bits per byte).
pub(crate) const MAXIMUM_VARINT_LENGTH_BYTES: u32 = 10;

/// Number of payload bits each varint byte carries.
const VARINT_BITS_PER_BYTE: u32 = 7;

/// A decoded protobuf field key: the field number and its wire type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WireFieldKey {
    /// The field's number in the message.
    pub field_number: u32,
    /// The wire type the field's value travels with.
    pub wire_type: u64,
}

/// The borrow-only protobuf wire-format cursor.
pub struct WireReader<'bytes> {
    bytes: &'bytes [u8],
    position: usize,
}

impl<'bytes> WireReader<'bytes> {
    /// Starts a read pass over a complete encoded message's bytes.
    pub fn new(bytes: &'bytes [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    /// Whether every byte of the message has been consumed.
    pub fn at_end(&self) -> bool {
        self.position >= self.bytes.len()
    }

    /// Reads the next byte, or reports truncation.
    fn next_byte(&mut self, context: &'static str) -> Result<u8, VectorTileError> {
        let byte = self
            .bytes
            .get(self.position)
            .copied()
            .ok_or(VectorTileError::TruncatedBuffer { context })?;
        self.position += 1;
        Ok(byte)
    }

    /// Reads a base-128 varint, rejecting byte counts past
    /// [`MAXIMUM_VARINT_LENGTH_BYTES`] and payloads that overflow 64 bits.
    pub fn read_varint(&mut self, context: &'static str) -> Result<u64, VectorTileError> {
        let mut value: u64 = 0;
        for byte_index in 0..MAXIMUM_VARINT_LENGTH_BYTES {
            let byte = self.next_byte(context)?;
            let payload = u64::from(byte & 0b0111_1111);
            let shift = byte_index * VARINT_BITS_PER_BYTE;
            let is_terminal_byte = byte & 0b1000_0000 == 0;
            if shift >= 64 || (shift == 63 && payload > 1) {
                return Err(VectorTileError::VarintValueExceeds64Bits { context });
            }
            value |= payload << shift;
            if is_terminal_byte {
                return Ok(value);
            }
        }
        Err(VectorTileError::VarintByteCountExceeded {
            maximum_byte_count: MAXIMUM_VARINT_LENGTH_BYTES,
            context,
        })
    }

    /// Reads a field key, splitting it into field number and wire type and
    /// refusing zero field numbers and unknown wire types.
    pub fn read_field_key(
        &mut self,
        context: &'static str,
    ) -> Result<WireFieldKey, VectorTileError> {
        let key = self.read_varint(context)?;
        let wire_type = key & 0b0111;
        let field_number = key >> 3;
        if field_number == 0 {
            return Err(VectorTileError::InvalidWireField {
                field_number: 0,
                wire_type,
                context,
            });
        }
        if field_number > u32::MAX as u64 {
            return Err(VectorTileError::InvalidWireField {
                field_number: field_number as u32,
                wire_type,
                context,
            });
        }
        if wire_type != WIRE_TYPE_VARINT
            && wire_type != WIRE_TYPE_FIXED64
            && wire_type != WIRE_TYPE_LENGTH_DELIMITED
            && wire_type != WIRE_TYPE_FIXED32
        {
            return Err(VectorTileError::InvalidWireField {
                field_number: field_number as u32,
                wire_type,
                context,
            });
        }
        Ok(WireFieldKey {
            field_number: field_number as u32,
            wire_type,
        })
    }

    /// Reads a length-prefixed byte string, returning the borrowed slice.
    pub fn read_length_delimited(
        &mut self,
        context: &'static str,
    ) -> Result<&'bytes [u8], VectorTileError> {
        let claimed_length = self.read_varint(context)?;
        let remaining = &self.bytes[self.position..];
        let length = claimed_length as usize;
        if length > remaining.len() {
            return Err(VectorTileError::LengthDelimitedOverrun {
                claimed_length,
                context,
            });
        }
        let bytes = &remaining[..length];
        self.position += length;
        Ok(bytes)
    }

    /// Reads a 32-bit little-endian value, as `[u8; 4]` ready for integer
    /// or floating point interpretation.
    pub fn read_fixed32(&mut self, context: &'static str) -> Result<[u8; 4], VectorTileError> {
        let remaining = &self.bytes[self.position..];
        let bytes: [u8; 4] = remaining
            .get(0..4)
            .and_then(|slice| slice.try_into().ok())
            .ok_or(VectorTileError::TruncatedBuffer { context })?;
        self.position += 4;
        Ok(bytes)
    }

    /// Reads a 64-bit little-endian value, as `[u8; 8]` ready for integer
    /// or floating point interpretation.
    pub fn read_fixed64(&mut self, context: &'static str) -> Result<[u8; 8], VectorTileError> {
        let remaining = &self.bytes[self.position..];
        let bytes: [u8; 8] = remaining
            .get(0..8)
            .and_then(|slice| slice.try_into().ok())
            .ok_or(VectorTileError::TruncatedBuffer { context })?;
        self.position += 8;
        Ok(bytes)
    }

    /// Skips an unknown field's value on the forward-compatibility path
    /// (proto extensions): varints and fixed scalars are consumed, byte
    /// strings are jumped over, and proto group markers are refused as
    /// invalid keys.
    pub fn skip_field(
        &mut self,
        field_key: WireFieldKey,
        context: &'static str,
    ) -> Result<(), VectorTileError> {
        match field_key.wire_type {
            WIRE_TYPE_VARINT => {
                self.read_varint(context)?;
            }
            WIRE_TYPE_FIXED64 => {
                self.read_fixed64(context)?;
            }
            WIRE_TYPE_LENGTH_DELIMITED => {
                self.read_length_delimited(context)?;
            }
            WIRE_TYPE_FIXED32 => {
                self.read_fixed32(context)?;
            }
            _ => {
                return Err(VectorTileError::InvalidWireField {
                    field_number: field_key.field_number,
                    wire_type: field_key.wire_type,
                    context,
                });
            }
        }
        Ok(())
    }
}

/// Decodes the ZigZag transform the specification applies to `ParameterInteger`
/// deltas and `sint` attribute values: `value = (ParameterInteger >> 1) ^
/// -(ParameterInteger & 1)`.
pub fn zigzag_decode(parameter_integer: u64) -> i64 {
    let magnitude = (parameter_integer >> 1) as i64;
    let sign_bit = (parameter_integer & 1) as i64;
    magnitude ^ -sign_bit
}

#[cfg(test)]
mod tests {
    use crate::error::VectorTileError;
    use crate::wire::{
        WIRE_TYPE_FIXED32, WIRE_TYPE_FIXED64, WIRE_TYPE_LENGTH_DELIMITED, WIRE_TYPE_VARINT,
        WireReader, zigzag_decode,
    };

    /// Appends the base-128 varint encoding of `value` to `bytes`, the
    /// mirror of the decoder's reader.
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

    /// Builds a one-field-key buffer for key-reading tests.
    fn field_key_bytes(key: u64) -> [u8; 16] {
        let mut bytes = Vec::new();
        append_varint(key, &mut bytes);
        bytes.extend_from_slice(&[0x00; 15]);
        bytes.try_into().unwrap()
    }

    #[test]
    fn single_byte_varints_decode_to_themselves() {
        let mut reader = WireReader::new(&[0x00, 0x01, 0x7f]);
        assert_eq!(reader.read_varint("test").unwrap(), 0);
        assert_eq!(reader.read_varint("test").unwrap(), 1);
        assert_eq!(reader.read_varint("test").unwrap(), 127);
        assert!(reader.at_end());
    }

    #[test]
    fn multi_byte_varints_decode_low_bits_first() {
        // 300 encodes as two bytes: low 7 bits then high bits.
        let mut reader = WireReader::new(&[0xac, 0x02]);
        assert_eq!(reader.read_varint("test").unwrap(), 300);
    }

    #[test]
    fn sixty_four_bit_varints_use_ten_bytes() {
        // u64::MAX encodes as ten continuation-prefixed bytes (0xff x 9,
        // then 0x01).
        let encoded = [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01];
        let mut reader = WireReader::new(&encoded);
        assert_eq!(reader.read_varint("test").unwrap(), u64::MAX);
    }

    #[test]
    fn an_eleventh_varint_byte_is_rejected_as_an_excessive_byte_count() {
        // 10 bytes can encode any u64 (the 10th byte carrying bit 63);
        // an 11-byte varint is one byte too many.
        let encoded = [
            0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x81, 0x01,
        ];
        let mut reader = WireReader::new(&encoded);
        assert!(matches!(
            reader.read_varint("test"),
            Err(VectorTileError::VarintByteCountExceeded { .. })
        ));
    }

    #[test]
    fn varints_exceeding_sixty_four_bits_are_rejected() {
        // The 10th byte's payload may only be 0 or 1 (bit 63); carrying a
        // higher bit would overflow u64.
        let encoded = [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f];
        let mut reader = WireReader::new(&encoded);
        assert!(matches!(
            reader.read_varint("test"),
            Err(VectorTileError::VarintValueExceeds64Bits { .. })
        ));
    }

    #[test]
    fn truncated_varints_report_truncation() {
        let mut reader = WireReader::new(&[0x80]);
        assert!(matches!(
            reader.read_varint("test"),
            Err(VectorTileError::TruncatedBuffer { .. })
        ));
    }

    #[test]
    fn field_keys_split_into_field_number_and_wire_type() {
        let key_bytes = field_key_bytes((3 << 3) | WIRE_TYPE_LENGTH_DELIMITED);
        let mut reader = WireReader::new(&key_bytes);
        let key = reader.read_field_key("test").unwrap();
        assert_eq!(key.field_number, 3);
        assert_eq!(key.wire_type, WIRE_TYPE_LENGTH_DELIMITED);
    }

    #[test]
    fn zero_field_numbers_are_rejected() {
        let mut reader = WireReader::new(&[0x00]);
        assert!(matches!(
            reader.read_field_key("test"),
            Err(VectorTileError::InvalidWireField { .. })
        ));
    }

    #[test]
    fn unknown_wire_types_are_rejected() {
        // Wire types 3 (start group) and 4 (end group), plus 6 and 7, are
        // not addressable by this reader.
        for wire_type in [3u64, 4, 6, 7] {
            let key_bytes = field_key_bytes((5 << 3) | wire_type);
            let mut reader = WireReader::new(&key_bytes);
            assert!(matches!(
                reader.read_field_key("test"),
                Err(VectorTileError::InvalidWireField { .. })
            ));
        }
    }

    #[test]
    fn length_delimited_slices_are_borrowed_from_the_buffer() {
        let mut reader = WireReader::new(&[0x03, b'a', b'b', b'c', 0x01]);
        assert_eq!(reader.read_length_delimited("test").unwrap(), b"abc");
        assert_eq!(reader.read_varint("test").unwrap(), 1);
    }

    #[test]
    fn length_delimited_claims_beyond_the_buffer_are_rejected() {
        let mut reader = WireReader::new(&[0x05, b'a', b'b']);
        assert!(matches!(
            reader.read_length_delimited("test"),
            Err(VectorTileError::LengthDelimitedOverrun { .. })
        ));
    }

    #[test]
    fn fixed_width_scalars_read_little_endian() {
        // 1.0f32 and 1.0f64 are exactly 0b0,0111_1111,000… IEEE 754 forms.
        let mut reader = WireReader::new(&[
            0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf0, 0x3f,
        ]);
        assert_eq!(
            f32::from_le_bytes(reader.read_fixed32("test").unwrap()),
            1.0
        );
        assert_eq!(
            f64::from_le_bytes(reader.read_fixed64("test").unwrap()),
            1.0
        );
    }

    #[test]
    fn skip_field_consumes_each_supported_wire_type() {
        let mut bytes = Vec::new();
        // field 3, fixed32: four payload bytes.
        append_varint((3 << 3) | WIRE_TYPE_FIXED32, &mut bytes);
        bytes.extend_from_slice(&[0x01, 0x02, 0x03, 0x04]);
        // field 3, fixed64: eight payload bytes.
        append_varint((3 << 3) | WIRE_TYPE_FIXED64, &mut bytes);
        bytes.extend_from_slice(&[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]);
        // field 3, length-delimited: two payload bytes.
        append_varint((3 << 3) | WIRE_TYPE_LENGTH_DELIMITED, &mut bytes);
        bytes.push(0x02);
        bytes.extend_from_slice(&[0xaa, 0xbb]);
        // field 3, varint: one payload byte.
        append_varint((3 << 3) | WIRE_TYPE_VARINT, &mut bytes);
        bytes.push(0x01);
        let mut reader = WireReader::new(&bytes);
        for wire_type in [
            WIRE_TYPE_FIXED32,
            WIRE_TYPE_FIXED64,
            WIRE_TYPE_LENGTH_DELIMITED,
            WIRE_TYPE_VARINT,
        ] {
            let field_key = reader.read_field_key("test").unwrap();
            assert_eq!(field_key.field_number, 3);
            assert_eq!(field_key.wire_type, wire_type);
            reader.skip_field(field_key, "test").unwrap();
        }
        assert!(reader.at_end());
    }

    #[test]
    fn zigzag_decoding_follows_the_spec_formula() {
        // The specification's worked transform pairs.
        assert_eq!(zigzag_decode(0), 0);
        assert_eq!(zigzag_decode(1), -1);
        assert_eq!(zigzag_decode(2), 1);
        assert_eq!(zigzag_decode(3), -2);
        // The point example's parameters: 50 -> +25, 34 -> +17.
        assert_eq!(zigzag_decode(50), 25);
        assert_eq!(zigzag_decode(34), 17);
        // The extremes decode to the mirrored signed extremes: even
        // parameter integers are magnitudes, odd ones negate them.
        assert_eq!(zigzag_decode(u64::MAX), i64::MIN);
        assert_eq!(zigzag_decode(u64::MAX - 1), i64::MAX);
        assert_eq!(zigzag_decode(u64::MAX - 2), i64::MIN + 1);
    }
}
