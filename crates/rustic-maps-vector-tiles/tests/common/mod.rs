//! The test-suite's only encoder: a hand-rolled protobuf writer that
//! mirrors the decoder field-by-field (the specification's own packing
//! formulas spelled out), used to build the spec's documented example
//! tiles and adversarial variants in tests. No production code re-encodes
//! MVT; this module is compiled for tests only.

/// Encodes the base-128 varint representation of `value`.
pub fn append_varint(value: u64, bytes: &mut Vec<u8>) {
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

/// Encodes a field key from its field number and wire type per
/// `(field_number << 3) | wire_type`.
pub fn append_field_key(field_number: u32, wire_type: u64, bytes: &mut Vec<u8>) {
    append_varint(((field_number as u64) << 3) | wire_type, bytes);
}

/// Encodes a length-delimited field (wire type 2).
pub fn append_length_delimited_field(field_number: u32, payload: &[u8], bytes: &mut Vec<u8>) {
    append_field_key(field_number, 2, bytes);
    append_varint(payload.len() as u64, bytes);
    bytes.extend_from_slice(payload);
}

/// Encodes a varint field (wire type 0).
pub fn append_varint_field(field_number: u32, value: u64, bytes: &mut Vec<u8>) {
    append_field_key(field_number, 0, bytes);
    append_varint(value, bytes);
}

/// Encodes a packed varint field (wire type 2, values varint-encoded in
/// the payload), the shape the specification pins for `tags` and
/// `geometry`.
pub fn append_packed_varint_field(field_number: u32, values: &[u64], bytes: &mut Vec<u8>) {
    let mut payload = Vec::new();
    for value in values {
        append_varint(*value, &mut payload);
    }
    append_length_delimited_field(field_number, &payload, bytes);
}

/// Packs a command into its command integer per
/// `CommandInteger = (id & 0x7) | (count << 3)`.
pub fn pack_command_integer(command_count: u64, command_id: u64) -> u64 {
    (command_id & 0x7) | (command_count << 3)
}

/// Encodes the ZigZag transform per `ParameterInteger = (value << 1) ^
/// (value >> 63)`.
pub fn zigzag_encode(value: i64) -> u64 {
    ((value << 1) ^ (value >> 63)) as u64
}

/// Encodes one encoded `Feature` message.
///
/// A `None` vector skips the corresponding field, exercising
/// absent-field decodes.
pub fn encode_feature(
    identifier: Option<u64>,
    geometry_type: Option<u64>,
    tags: Option<&[u64]>,
    geometry_command_integers: Option<&[u64]>,
) -> Vec<u8> {
    let mut feature_bytes = Vec::new();
    if let Some(identifier) = identifier {
        // Field 1: id (optional uint64).
        append_varint_field(1, identifier, &mut feature_bytes);
    }
    if let Some(tags) = tags {
        // Field 2: tags (repeated uint32, packed).
        append_packed_varint_field(2, tags, &mut feature_bytes);
    }
    if let Some(geometry_type) = geometry_type {
        // Field 3: type (optional GeomType enum).
        append_varint_field(3, geometry_type, &mut feature_bytes);
    }
    if let Some(geometry) = geometry_command_integers {
        // Field 4: geometry (repeated uint32, packed).
        append_packed_varint_field(4, geometry, &mut feature_bytes);
    }
    feature_bytes
}

/// Encodes one value message carrying a single `float` (fixed 32-bit)
/// variant.
pub fn encode_float_value(value: f32) -> Vec<u8> {
    let mut value_bytes = Vec::new();
    append_field_key(2, 5, &mut value_bytes);
    value_bytes.extend_from_slice(&value.to_le_bytes());
    value_bytes
}

/// Encodes one value message carrying a single `double` (fixed 64-bit)
/// variant.
pub fn encode_double_value(value: f64) -> Vec<u8> {
    let mut value_bytes = Vec::new();
    append_field_key(3, 1, &mut value_bytes);
    value_bytes.extend_from_slice(&value.to_le_bytes());
    value_bytes
}

/// Encodes one value message carrying a single `bool` variant.
pub fn encode_boolean_value(value: bool) -> Vec<u8> {
    let mut value_bytes = Vec::new();
    append_varint_field(7, u64::from(value), &mut value_bytes);
    value_bytes
}

/// Encodes one encoded `Layer` message.
pub fn encode_layer(
    name: &str,
    version: Option<u64>,
    features: &[Vec<u8>],
    keys: &[&str],
    values: &[Vec<u8>],
    extent: Option<u64>,
) -> Vec<u8> {
    let mut layer_bytes = Vec::new();
    // Field 1: name (required string).
    append_length_delimited_field(1, name.as_bytes(), &mut layer_bytes);
    // Field 2: features (repeated Feature).
    for feature_bytes in features {
        append_length_delimited_field(2, feature_bytes, &mut layer_bytes);
    }
    // Field 3: keys (repeated string).
    for key in keys {
        append_length_delimited_field(3, key.as_bytes(), &mut layer_bytes);
    }
    // Field 4: values (repeated Value).
    for value_bytes in values {
        append_length_delimited_field(4, value_bytes, &mut layer_bytes);
    }
    // Field 5: extent (optional uint32).
    if let Some(extent) = extent {
        append_varint_field(5, extent, &mut layer_bytes);
    }
    // Field 15: version (required uint32).
    if let Some(version) = version {
        append_varint_field(15, version, &mut layer_bytes);
    }
    layer_bytes
}

/// Encodes one value message carrying a single `string` variant.
pub fn encode_text_value(text: &str) -> Vec<u8> {
    let mut value_bytes = Vec::new();
    append_length_delimited_field(1, text.as_bytes(), &mut value_bytes);
    value_bytes
}

/// Encodes one value message carrying a single `sint` variant.
pub fn encode_zigzag_value(value: i64) -> Vec<u8> {
    let mut value_bytes = Vec::new();
    append_varint_field(6, zigzag_encode(value), &mut value_bytes);
    value_bytes
}

/// Encodes one value message carrying a single `int` (varint, raw
/// two's-complement) variant.
pub fn encode_signed_value(value: i64) -> Vec<u8> {
    let mut value_bytes = Vec::new();
    append_varint_field(4, value as u64, &mut value_bytes);
    value_bytes
}

/// Encodes one value message carrying a single `uint` variant.
pub fn encode_unsigned_value(value: u64) -> Vec<u8> {
    let mut value_bytes = Vec::new();
    append_varint_field(5, value, &mut value_bytes);
    value_bytes
}

/// Encodes one `Tile` message carrying the given encoded layers.
pub fn encode_tile(layers: &[Vec<u8>]) -> Vec<u8> {
    let mut tile_bytes = Vec::new();
    // Field 3: layers (repeated Layer).
    for layer_bytes in layers {
        append_length_delimited_field(3, layer_bytes, &mut tile_bytes);
    }
    tile_bytes
}

/// Command id 0b001: `MoveTo`.
pub const COMMAND_ID_MOVE_TO: u64 = 1;
/// Command id 0b010: `LineTo`.
pub const COMMAND_ID_LINE_TO: u64 = 2;
/// Command id 0b111: `ClosePath`.
pub const COMMAND_ID_CLOSE_PATH: u64 = 7;

/// Encodes the command integer stream of one `MoveTo` command carrying
/// every given multipoint vertex (the shape the specification's point
/// examples use).
pub fn encode_move_to_command_stream(points: &[(i64, i64)]) -> Vec<u64> {
    let mut stream = vec![pack_command_integer(
        points.len() as u64,
        COMMAND_ID_MOVE_TO,
    )];
    let mut cursor = (0i64, 0i64);
    for (x, y) in points {
        stream.push(zigzag_encode(x - cursor.0));
        stream.push(zigzag_encode(y - cursor.1));
        cursor = (*x, *y);
    }
    stream
}

/// Encodes the command integer stream of one linear path: a count-1
/// `MoveTo`, the remaining deltas as one `LineTo`, and (for polygon rings)
/// a terminating `ClosePath`.
pub fn encode_linear_path_command_stream(vertices: &[(i64, i64)], close_ring: bool) -> Vec<u64> {
    let mut deltas = Vec::new();
    let mut cursor = (0i64, 0i64);
    for (x, y) in vertices {
        deltas.push(zigzag_encode(x - cursor.0));
        deltas.push(zigzag_encode(y - cursor.1));
        cursor = (*x, *y);
    }
    let mut stream = vec![
        pack_command_integer(1, COMMAND_ID_MOVE_TO),
        deltas[0],
        deltas[1],
    ];
    if deltas.len() > 2 {
        stream.push(pack_command_integer(
            ((deltas.len() - 2) / 2) as u64,
            COMMAND_ID_LINE_TO,
        ));
        stream.extend(&deltas[2..]);
    }
    if close_ring {
        stream.push(pack_command_integer(1, COMMAND_ID_CLOSE_PATH));
    }
    stream
}

/// Assembles a minimal one-layer tile around one geometry command stream,
/// the shape the specification's geometry examples decode within.
pub fn encode_example_tile(
    layer_name: &str,
    geometry_type: u64,
    geometry_command_integers: &[u64],
) -> Vec<u8> {
    encode_tile(&[encode_layer(
        layer_name,
        Some(2),
        &[encode_feature(
            None,
            Some(geometry_type),
            None,
            Some(geometry_command_integers),
        )],
        &[],
        &[],
        None,
    )])
}
