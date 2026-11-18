//! The Mapbox Vector Tile v2.1 message decoder.
//!
//! Walks the specification's protobuf schema (`Tile` → layers → features
//! with their dictionary-encoded attributes) one field key at a time on
//! the [`crate::wire`] reader. Field numbers outside the schema are
//! skipped on the protobuf forward-compatibility path (the schema reserves
//! extensions 16–8191); a schema field that arrives with a wire type it
//! cannot carry is refused as malformed rather than silently skipped. The
//! decode is total: every malformed input yields a typed
//! [`VectorTileError`], never a panic, because these bytes arrive from
//! untrusted network fetches.

use crate::error::VectorTileError;
use crate::geometry::{
    GEOMETRY_TYPE_LINESTRING, GEOMETRY_TYPE_POINT, GEOMETRY_TYPE_POLYGON, GEOMETRY_TYPE_UNKNOWN,
    VectorTileGeometry, VectorTileGeometryType, decode_geometry_command_stream,
};
use crate::wire::{
    WIRE_TYPE_FIXED32, WIRE_TYPE_FIXED64, WIRE_TYPE_LENGTH_DELIMITED, WIRE_TYPE_VARINT,
    WireFieldKey, WireReader,
};

/// The Tile message field a layer travels in.
const TILE_LAYER_FIELD_NUMBER: u32 = 3;
/// The Layer message's fields.
const LAYER_NAME_FIELD_NUMBER: u32 = 1;
const LAYER_FEATURES_FIELD_NUMBER: u32 = 2;
const LAYER_KEYS_FIELD_NUMBER: u32 = 3;
const LAYER_VALUES_FIELD_NUMBER: u32 = 4;
const LAYER_EXTENT_FIELD_NUMBER: u32 = 5;
const LAYER_VERSION_FIELD_NUMBER: u32 = 15;
/// The Feature message's fields.
const FEATURE_ID_FIELD_NUMBER: u32 = 1;
const FEATURE_TAGS_FIELD_NUMBER: u32 = 2;
const FEATURE_TYPE_FIELD_NUMBER: u32 = 3;
const FEATURE_GEOMETRY_FIELD_NUMBER: u32 = 4;
/// The Value message's seven variant fields, exactly one of which is
/// present in a valid value.
const VALUE_TEXT_FIELD_NUMBER: u32 = 1;
const VALUE_FLOAT_FIELD_NUMBER: u32 = 2;
const VALUE_DOUBLE_FIELD_NUMBER: u32 = 3;
const VALUE_SIGNED_FIELD_NUMBER: u32 = 4;
const VALUE_UNSIGNED_FIELD_NUMBER: u32 = 5;
const VALUE_ZIGZAG_SIGNED_FIELD_NUMBER: u32 = 6;
const VALUE_BOOLEAN_FIELD_NUMBER: u32 = 7;

/// The layer version the specification requires.
const SPECIFICATION_LAYER_VERSION: u64 = 2;
/// The extent a layer without the field carries — the schema's default.
pub const DEFAULT_TILE_EXTENT: u64 = 4096;

/// One decoded Mapbox Vector Tile: an ordered set of named layers.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorTile {
    /// The tile's layers, in wire order.
    pub layers: Vec<VectorTileLayer>,
}

/// One decoded layer: its name, extent, specification version, the
/// attribute dictionaries and its decoded features.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorTileLayer {
    /// The layer's `name` field; unique within its tile.
    pub name: String,
    /// The layer's `extent` field: the size of this layer's tile-local
    /// coordinate grid (layers without the field carry
    /// [`DEFAULT_TILE_EXTENT`]).
    pub extent: u64,
    /// The layer's `version` field — the decoder requires the
    /// specification's value of 2.
    pub version: u32,
    /// The layer's decoded features, in wire order.
    pub features: Vec<VectorTileFeature>,
}

/// One decoded feature: optional identifier, declared geometry kind,
/// decoded geometry and dictionary-resolved attributes.
///
/// Attribute pairs are kept as a sequence rather than a map because the
/// wire format does not forbid a producer from emitting one key twice; a
/// consumer wanting map semantics reduces the pair list itself.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorTileFeature {
    /// The feature's `id` field, when the producer set it.
    pub identifier: Option<u64>,
    /// The feature's declared geometry kind.
    pub geometry_type: VectorTileGeometryType,
    /// The feature's decoded geometry.
    pub geometry: VectorTileGeometry,
    /// The feature's attributes as `(key, value)` pairs, in tag order.
    pub attributes: Vec<(String, VectorTileAttributeValue)>,
}

/// One layer attribute value, over the specification's seven variants.
///
/// The specification's `int`/`uint`/`sint` variants decode to the same two
/// numerics, but they stay distinct variants so a consumer can observe
/// which signedness the wire carried.
#[derive(Debug, Clone, PartialEq)]
pub enum VectorTileAttributeValue {
    /// The `string` variant.
    Text(String),
    /// The `float` (32-bit IEEE 754) variant.
    Float(f32),
    /// The `double` (64-bit IEEE 754) variant.
    Double(f64),
    /// The `int` (varint int64, two's-complement interpreted) variant.
    Signed(i64),
    /// The `uint` (varint uint64) variant.
    Unsigned(u64),
    /// The `sint` (ZigZag-encoded int64) variant, decoded.
    ZigZagSigned(i64),
    /// The `bool` variant.
    Boolean(bool),
}

impl VectorTileAttributeValue {
    /// The variant's numeric reading as a 64-bit float, when it carries a
    /// number; text and boolean variants report none.
    pub fn numeric_value(&self) -> Option<f64> {
        match self {
            Self::Text(_) | Self::Boolean(_) => None,
            Self::Float(value) => Some(f64::from(*value)),
            Self::Double(value) => Some(*value),
            Self::Signed(value) => Some(*value as f64),
            Self::Unsigned(value) => Some(*value as f64),
            Self::ZigZagSigned(value) => Some(*value as f64),
        }
    }
}

/// Decodes a complete Mapbox Vector Tile from its protobuf bytes.
///
/// The bytes are the unwrapped MVT payload — a `VectorTileEncoding::MapboxVectorTile`
/// wire body, or the gzip-decoded payload the provider seam unwraps for
/// `VectorTileEncoding::GzippedMapboxVectorTile`; unwrapping gzip is not
/// this decoder's job.
pub fn decode_vector_tile(tile_bytes: &[u8]) -> Result<VectorTile, VectorTileError> {
    let mut reader = WireReader::new(tile_bytes);
    let mut layers = Vec::new();
    while !reader.at_end() {
        let field_key = reader.read_field_key("tile body")?;
        if field_key.field_number == TILE_LAYER_FIELD_NUMBER
            && field_key.wire_type == WIRE_TYPE_LENGTH_DELIMITED
        {
            let layer_bytes = reader.read_length_delimited("layer bytes")?;
            let layer = parse_layer(layer_bytes)?;
            if layers
                .iter()
                .any(|decoded_layer: &VectorTileLayer| decoded_layer.name == layer.name)
            {
                return Err(VectorTileError::DuplicateLayerName { name: layer.name });
            }
            layers.push(layer);
        } else if field_key.field_number == TILE_LAYER_FIELD_NUMBER {
            return Err(VectorTileError::UnsupportedWireType {
                field_number: field_key.field_number,
                wire_type: field_key.wire_type,
                context: "tile layer",
            });
        } else {
            reader.skip_field(field_key, "tile body")?;
        }
    }
    Ok(VectorTile { layers })
}

/// The wire-order draft of a feature, decoded before the layer's key and
/// value dictionaries are known (protobuf field order inside a message
/// need not match the schema's field order, so a feature's tags can appear
/// before its dictionaries are complete).
struct FeatureDraft {
    identifier: Option<u64>,
    geometry_type: Option<u64>,
    geometry_integers: Option<Vec<u64>>,
    tags: Option<Vec<u64>>,
}

/// Parses one encoded layer into its decoded form, resolving each
/// feature's attribute tags against the layer's dictionaries.
fn parse_layer(layer_bytes: &[u8]) -> Result<VectorTileLayer, VectorTileError> {
    let mut reader = WireReader::new(layer_bytes);
    let mut drafts = Vec::new();
    let mut keys: Vec<String> = Vec::new();
    let mut values: Vec<VectorTileAttributeValue> = Vec::new();
    let mut name: Option<String> = None;
    let mut version: Option<u64> = None;
    let mut extent: Option<u64> = None;
    while !reader.at_end() {
        let field_key = reader.read_field_key("layer body")?;
        match (field_key.field_number, field_key.wire_type) {
            (LAYER_NAME_FIELD_NUMBER, WIRE_TYPE_LENGTH_DELIMITED) => {
                let name_bytes = reader.read_length_delimited("layer name bytes")?;
                let decoded_name = utf8_from_bytes(name_bytes, "layer name")?;
                name = Some(decoded_name);
            }
            (LAYER_FEATURES_FIELD_NUMBER, WIRE_TYPE_LENGTH_DELIMITED) => {
                let feature_bytes = reader.read_length_delimited("feature bytes")?;
                drafts.push(parse_feature_draft(feature_bytes)?);
            }
            (LAYER_KEYS_FIELD_NUMBER, WIRE_TYPE_LENGTH_DELIMITED) => {
                let key_bytes = reader.read_length_delimited("layer key bytes")?;
                keys.push(utf8_from_bytes(key_bytes, "layer key")?);
            }
            (LAYER_VALUES_FIELD_NUMBER, WIRE_TYPE_LENGTH_DELIMITED) => {
                let value_bytes = reader.read_length_delimited("layer value bytes")?;
                values.push(parse_value_message(value_bytes)?);
            }
            (LAYER_EXTENT_FIELD_NUMBER, WIRE_TYPE_VARINT) => {
                extent = Some(reader.read_varint("layer extent")?);
            }
            (LAYER_VERSION_FIELD_NUMBER, WIRE_TYPE_VARINT) => {
                version = Some(reader.read_varint("layer version")?);
            }
            (LAYER_NAME_FIELD_NUMBER, _)
            | (LAYER_FEATURES_FIELD_NUMBER, _)
            | (LAYER_KEYS_FIELD_NUMBER, _)
            | (LAYER_VALUES_FIELD_NUMBER, _)
            | (LAYER_EXTENT_FIELD_NUMBER, _)
            | (LAYER_VERSION_FIELD_NUMBER, _) => {
                return Err(layer_field_wire_type_error(&field_key));
            }
            _ => reader.skip_field(field_key, "layer body")?,
        }
    }
    let layer_name = name.ok_or(VectorTileError::MissingLayerName)?;
    // An absent version field decodes to the protobuf default of 1, which
    // the specification refuses just like any other non-2 version.
    let version_value = version.unwrap_or(1);
    if version_value != SPECIFICATION_LAYER_VERSION {
        return Err(VectorTileError::UnsupportedLayerVersion {
            found: version_value as u32,
        });
    }
    let mut features = Vec::with_capacity(drafts.len());
    for draft in &drafts {
        features.push(assemble_feature(draft, &keys, &values)?);
    }
    Ok(VectorTileLayer {
        name: layer_name,
        extent: extent.unwrap_or(DEFAULT_TILE_EXTENT),
        version: version_value as u32,
        features,
    })
}

/// The malformed-wire-type error for a known Layer field whose value
/// arrived on a wire type it cannot carry.
fn layer_field_wire_type_error(field_key: &WireFieldKey) -> VectorTileError {
    let context = match field_key.field_number {
        LAYER_NAME_FIELD_NUMBER => "layer name",
        LAYER_FEATURES_FIELD_NUMBER => "layer features",
        LAYER_KEYS_FIELD_NUMBER => "layer keys",
        LAYER_VALUES_FIELD_NUMBER => "layer values",
        LAYER_EXTENT_FIELD_NUMBER => "layer extent",
        _ => "layer version",
    };
    VectorTileError::UnsupportedWireType {
        field_number: field_key.field_number,
        wire_type: field_key.wire_type,
        context,
    }
}

/// Parses one encoded feature into its wire-order draft (dictionary
/// resolution is deferred until the layer's dictionaries are complete).
fn parse_feature_draft(feature_bytes: &[u8]) -> Result<FeatureDraft, VectorTileError> {
    let mut reader = WireReader::new(feature_bytes);
    let mut draft = FeatureDraft {
        identifier: None,
        geometry_type: None,
        geometry_integers: None,
        tags: None,
    };
    while !reader.at_end() {
        let field_key = reader.read_field_key("feature body")?;
        match (field_key.field_number, field_key.wire_type) {
            (FEATURE_ID_FIELD_NUMBER, WIRE_TYPE_VARINT) => {
                draft.identifier = Some(reader.read_varint("feature id")?);
            }
            (FEATURE_TAGS_FIELD_NUMBER, WIRE_TYPE_LENGTH_DELIMITED) => {
                let tag_bytes = reader.read_length_delimited("feature tags")?;
                draft.tags = Some(parse_packed_varint_array(tag_bytes, "feature tags")?);
            }
            (FEATURE_TYPE_FIELD_NUMBER, WIRE_TYPE_VARINT) => {
                draft.geometry_type = Some(reader.read_varint("feature type")?);
            }
            (FEATURE_GEOMETRY_FIELD_NUMBER, WIRE_TYPE_LENGTH_DELIMITED) => {
                let geometry_bytes = reader.read_length_delimited("feature geometry")?;
                draft.geometry_integers = Some(parse_packed_varint_array(
                    geometry_bytes,
                    "feature geometry",
                )?);
            }
            (FEATURE_ID_FIELD_NUMBER, _) | (FEATURE_TYPE_FIELD_NUMBER, _) => {
                return Err(feature_field_wire_type_error(&field_key));
            }
            (FEATURE_TAGS_FIELD_NUMBER, _) | (FEATURE_GEOMETRY_FIELD_NUMBER, _) => {
                return Err(VectorTileError::PackedFieldUnpacked {
                    field_number: field_key.field_number,
                    wire_type: field_key.wire_type,
                });
            }
            _ => reader.skip_field(field_key, "feature body")?,
        }
    }
    Ok(draft)
}

/// The malformed-wire-type error for a known Feature field whose value
/// arrived on a wire type it cannot carry.
fn feature_field_wire_type_error(field_key: &WireFieldKey) -> VectorTileError {
    let context = if field_key.field_number == FEATURE_ID_FIELD_NUMBER {
        "feature id"
    } else {
        "feature type"
    };
    VectorTileError::UnsupportedWireType {
        field_number: field_key.field_number,
        wire_type: field_key.wire_type,
        context,
    }
}

/// Turns a dictionary-resolved feature draft into a decoded
/// [`VectorTileFeature`], checking the tag pairs and the geometry command
/// stream per the specification.
fn assemble_feature(
    draft: &FeatureDraft,
    keys: &[String],
    values: &[VectorTileAttributeValue],
) -> Result<VectorTileFeature, VectorTileError> {
    // A feature without the type field decodes to the protobuf default of
    // UNKNOWN (0), which no compliant producer emits.
    let geometry_type_value = draft.geometry_type.unwrap_or(GEOMETRY_TYPE_UNKNOWN);
    let geometry_type = match geometry_type_value {
        GEOMETRY_TYPE_UNKNOWN => {
            return Err(VectorTileError::UnknownGeometryType { found: 0 });
        }
        GEOMETRY_TYPE_POINT => VectorTileGeometryType::Point,
        GEOMETRY_TYPE_LINESTRING => VectorTileGeometryType::LineString,
        GEOMETRY_TYPE_POLYGON => VectorTileGeometryType::Polygon,
        _ => {
            return Err(VectorTileError::UnknownGeometryType {
                found: geometry_type_value as u32,
            });
        }
    };
    let attributes = resolve_feature_attributes(draft, keys, values)?;
    let geometry = match &draft.geometry_integers {
        Some(command_integers) => decode_geometry_command_stream(geometry_type, command_integers)?,
        None => empty_geometry(geometry_type),
    };
    Ok(VectorTileFeature {
        identifier: draft.identifier,
        geometry_type,
        geometry,
        attributes,
    })
}

/// Resolves a feature's tag index pairs against the layer's dictionaries.
fn resolve_feature_attributes(
    draft: &FeatureDraft,
    keys: &[String],
    values: &[VectorTileAttributeValue],
) -> Result<Vec<(String, VectorTileAttributeValue)>, VectorTileError> {
    let tags = match &draft.tags {
        Some(tags) => tags,
        None => return Ok(Vec::new()),
    };
    if tags.len() % 2 != 0 {
        return Err(VectorTileError::TagElementCountNotEven {
            found: tags.len() as u64,
        });
    }
    let mut attributes = Vec::with_capacity(tags.len() / 2);
    for pair in tags.chunks(2) {
        let key_index = pair[0];
        let value_index = pair[1];
        if key_index >= keys.len() as u64 {
            return Err(VectorTileError::AttributeIndexOutOfRange {
                index: key_index,
                context: "attribute keys",
                dictionary_size: keys.len() as u64,
            });
        }
        if value_index >= values.len() as u64 {
            return Err(VectorTileError::AttributeIndexOutOfRange {
                index: value_index,
                context: "attribute values",
                dictionary_size: values.len() as u64,
            });
        }
        attributes.push((
            keys[key_index as usize].clone(),
            values[value_index as usize].clone(),
        ));
    }
    Ok(attributes)
}

/// The empty decoded shape of a geometry kind (a feature may declare a
/// kind while carrying an empty `geometry` field; the decode stays total
/// and reports the declared kind with no coordinates).
fn empty_geometry(geometry_type: VectorTileGeometryType) -> VectorTileGeometry {
    match geometry_type {
        VectorTileGeometryType::Point => VectorTileGeometry::Points(Vec::new()),
        VectorTileGeometryType::LineString => VectorTileGeometry::LineStrings(Vec::new()),
        VectorTileGeometryType::Polygon => VectorTileGeometry::Polygons(Vec::new()),
        // UNKNOWN features are refused in `assemble_feature`; this arm only
        // keeps the match exhaustive.
        VectorTileGeometryType::Unknown => VectorTileGeometry::Points(Vec::new()),
    }
}

/// Parses a packed repeated `uint32` field (feature `tags` or `geometry`)
/// into its varint values.
fn parse_packed_varint_array(
    packed_bytes: &[u8],
    context: &'static str,
) -> Result<Vec<u64>, VectorTileError> {
    let mut reader = WireReader::new(packed_bytes);
    let mut values = Vec::new();
    while !reader.at_end() {
        values.push(reader.read_varint(context)?);
    }
    Ok(values)
}

/// Reads a length-delimited field as UTF-8 text.
fn utf8_from_bytes(bytes: &[u8], context: &'static str) -> Result<String, VectorTileError> {
    std::str::from_utf8(bytes)
        .map(|text| text.to_string())
        .map_err(|_| VectorTileError::InvalidUtf8 { context })
}

/// Parses one encoded `Value` message: exactly one of the seven variants
/// must be present.
fn parse_value_message(value_bytes: &[u8]) -> Result<VectorTileAttributeValue, VectorTileError> {
    let mut reader = WireReader::new(value_bytes);
    let mut value: Option<VectorTileAttributeValue> = None;
    while !reader.at_end() {
        let field_key = reader.read_field_key("value message")?;
        let decoded = match (field_key.field_number, field_key.wire_type) {
            (VALUE_TEXT_FIELD_NUMBER, WIRE_TYPE_LENGTH_DELIMITED) => {
                let text_bytes = reader.read_length_delimited("value text bytes")?;
                Some(VectorTileAttributeValue::Text(utf8_from_bytes(
                    text_bytes,
                    "attribute value text",
                )?))
            }
            (VALUE_FLOAT_FIELD_NUMBER, WIRE_TYPE_FIXED32) => Some(VectorTileAttributeValue::Float(
                f32::from_le_bytes(reader.read_fixed32("value float bytes")?),
            )),
            (VALUE_DOUBLE_FIELD_NUMBER, WIRE_TYPE_FIXED64) => {
                Some(VectorTileAttributeValue::Double(f64::from_le_bytes(
                    reader.read_fixed64("value double bytes")?,
                )))
            }
            (VALUE_SIGNED_FIELD_NUMBER, WIRE_TYPE_VARINT) => Some(
                VectorTileAttributeValue::Signed(reader.read_varint("value signed")? as i64),
            ),
            (VALUE_UNSIGNED_FIELD_NUMBER, WIRE_TYPE_VARINT) => Some(
                VectorTileAttributeValue::Unsigned(reader.read_varint("value unsigned")?),
            ),
            (VALUE_ZIGZAG_SIGNED_FIELD_NUMBER, WIRE_TYPE_VARINT) => {
                let raw = reader.read_varint("value zigzag signed")?;
                Some(VectorTileAttributeValue::ZigZagSigned(
                    crate::wire::zigzag_decode(raw),
                ))
            }
            (VALUE_BOOLEAN_FIELD_NUMBER, WIRE_TYPE_VARINT) => Some(
                VectorTileAttributeValue::Boolean(reader.read_varint("value boolean")? != 0),
            ),
            (VALUE_TEXT_FIELD_NUMBER, _)
            | (VALUE_FLOAT_FIELD_NUMBER, _)
            | (VALUE_DOUBLE_FIELD_NUMBER, _)
            | (VALUE_SIGNED_FIELD_NUMBER, _)
            | (VALUE_UNSIGNED_FIELD_NUMBER, _)
            | (VALUE_ZIGZAG_SIGNED_FIELD_NUMBER, _)
            | (VALUE_BOOLEAN_FIELD_NUMBER, _) => {
                return Err(VectorTileError::UnsupportedWireType {
                    field_number: field_key.field_number,
                    wire_type: field_key.wire_type,
                    context: "attribute value field",
                });
            }
            _ => {
                reader.skip_field(field_key, "value message")?;
                None
            }
        };
        if decoded.is_some() && value.is_some() {
            return Err(VectorTileError::AttributeValueVariantConflicting);
        }
        if decoded.is_some() {
            value = decoded;
        }
    }
    value.ok_or(VectorTileError::AttributeValueVariantMissing)
}
