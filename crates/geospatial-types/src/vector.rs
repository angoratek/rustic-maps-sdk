//! Vector tile imagery records.
//!
//! A vector tile is carried as its wire bytes: the Mapbox Vector Tile (MVT)
//! protobuf, optionally wrapped in gzip by the serving CDN. The record keeps
//! no parsed state, so a cached payload stays byte-identical to what the
//! provider delivered. Parsing into layers and geometries happens in the
//! renderer, not here.

use serde::{Deserialize, Serialize};

use crate::error::SDKError;

/// First byte of the two-byte gzip magic number (`1f 8b`).
const GZIP_MAGIC_FIRST_BYTE: u8 = 0x1f;
/// Second byte of the two-byte gzip magic number (`1f 8b`).
const GZIP_MAGIC_SECOND_BYTE: u8 = 0x8b;

/// Wire encoding of a vector tile's byte payload.
///
/// Tile services commonly serve MVT protobuf with `Content-Encoding: gzip`.
/// The record preserves what arrived on the wire; a decoder applies gzip
/// decompression only for [`VectorTileEncoding::GzippedMapboxVectorTile`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VectorTileEncoding {
    /// Plain MVT protobuf bytes.
    MapboxVectorTile,
    /// MVT protobuf bytes wrapped in gzip (the CDN's `Content-Encoding`).
    GzippedMapboxVectorTile,
}

/// One fetched web-mercator XYZ vector tile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VectorTileData {
    /// Tile column coordinate at the given zoom level.
    pub tile_x: u32,
    /// Tile row coordinate at the given zoom level.
    pub tile_y: u32,
    /// Zoom level this tile belongs to.
    pub zoom_level: u8,
    /// Wire encoding of the byte payload.
    pub tile_encoding: VectorTileEncoding,
    /// Encoded tile bytes (MVT protobuf, optionally gzip-wrapped).
    pub tile_bytes: Vec<u8>,
}

impl VectorTileData {
    /// Builds the record from wire bytes.
    ///
    /// # Parameters
    /// - `tile_x`, `tile_y` — the tile's column and row at `zoom_level`.
    /// - `zoom_level` — the web-mercator zoom level of the tile.
    /// - `wire_bytes` — the payload exactly as delivered by the provider.
    ///
    /// # Returns
    /// A tile record whose encoding is identified from the leading two-byte
    /// gzip magic number, so the record names the encoding it actually
    /// carries.
    ///
    /// # Errors
    /// Returns [`SDKError::ResponseParsingFailure`] when the payload is empty;
    /// an empty payload is never treated as an empty tile.
    pub fn from_wire_bytes(
        tile_x: u32,
        tile_y: u32,
        zoom_level: u8,
        wire_bytes: Vec<u8>,
    ) -> Result<Self, SDKError> {
        let tile_encoding = Self::sniff_tile_encoding(&wire_bytes)?;
        Ok(Self {
            tile_x,
            tile_y,
            zoom_level,
            tile_encoding,
            tile_bytes: wire_bytes,
        })
    }

    /// Classifies the wire payload by its leading gzip magic number; an
    /// empty payload is a parsing failure, not a silent empty tile.
    fn sniff_tile_encoding(wire_bytes: &[u8]) -> Result<VectorTileEncoding, SDKError> {
        if wire_bytes.is_empty() {
            return Err(SDKError::ResponseParsingFailure(
                "vector tile payload is empty".to_string(),
            ));
        }
        let carries_gzip_magic = wire_bytes[0] == GZIP_MAGIC_FIRST_BYTE
            && wire_bytes.len() > 1
            && wire_bytes[1] == GZIP_MAGIC_SECOND_BYTE;
        if carries_gzip_magic {
            Ok(VectorTileEncoding::GzippedMapboxVectorTile)
        } else {
            Ok(VectorTileEncoding::MapboxVectorTile)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{VectorTileData, VectorTileEncoding};
    use crate::SDKError;

    /// 0x1f 0x8b — the two-byte gzip magic the encoding sniff keys on.
    const GZIP_MAGIC_BYTES: [u8; 2] = [0x1f, 0x8b];
    /// Protobuf key for MVT's repeated `layers` field (field 3, wire type 2).
    const LAYER_FIELD_PROTOBUF_KEY: u8 = 0x1a;

    #[test]
    fn plain_protobuf_bytes_classify_as_mapbox_vector_tiles() {
        let tile =
            VectorTileData::from_wire_bytes(1, 2, 3, vec![LAYER_FIELD_PROTOBUF_KEY, 0]).unwrap();
        assert_eq!(tile.tile_encoding, VectorTileEncoding::MapboxVectorTile);
        assert_eq!(tile.tile_bytes, vec![LAYER_FIELD_PROTOBUF_KEY, 0]);
    }

    #[test]
    fn gzip_magic_bytes_classify_as_gzipped_vector_tiles() {
        let wire_bytes = [GZIP_MAGIC_BYTES.as_slice(), &[0x08]].concat();
        let tile = VectorTileData::from_wire_bytes(4, 5, 6, wire_bytes).unwrap();
        assert_eq!(
            tile.tile_encoding,
            VectorTileEncoding::GzippedMapboxVectorTile
        );
    }

    #[test]
    fn empty_wire_payload_is_a_parsing_failure() {
        assert!(matches!(
            VectorTileData::from_wire_bytes(0, 0, 0, Vec::new()),
            Err(SDKError::ResponseParsingFailure(_))
        ));
    }
}
