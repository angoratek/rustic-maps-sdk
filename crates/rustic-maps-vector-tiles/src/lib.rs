//! The Mapbox Vector Tile (MVT) v2.1 decoder: the protobuf wire-format
//! reader, the layer/feature/attribute model and the geometry
//! command-stream decoder that turns fetched vector tiles into
//! renderer-consumable structures.
//!
//! The crate is pure byte parsing with zero runtime dependencies beyond
//! [`thiserror`], so it builds on native targets and on
//! `wasm32-unknown-unknown` alike. These are internal processing types, not
//! FFI records: nothing here appears in the frozen UDL contract, and no
//! `VectorTile` bytes are trusted — every malformed input surfaces as a
//! typed [`VectorTileError`], never a panic or a guessed decode.
//!
//! Entry point: [`decode_vector_tile`], taking the unwrapped MVT protobuf
//! payload (the provider seam unwraps any gzip envelope before handing the
//! bytes over). The decoded tile carries [`VectorTileLayer`]s of
//! [`VectorTileFeature`]s whose geometries are the specification's
//! MoveTo/LineTo/ClosePath command stream resolved to tile-local extent
//! coordinates.

#![cfg_attr(not(test), deny(clippy::expect_used, clippy::unwrap_used))]

mod decoder;
mod error;
mod geometry;
pub mod wire;

pub use decoder::{
    DEFAULT_TILE_EXTENT, VectorTile, VectorTileAttributeValue, VectorTileFeature, VectorTileLayer,
    decode_vector_tile,
};
pub use error::VectorTileError;
pub use geometry::{
    VectorTileGeometry, VectorTileGeometryPoint, VectorTileGeometryType, VectorTileLinearRing,
};

/// The URL of the Mapbox Vector Tile specification this crate decodes;
/// the workspace documentation links it.
pub const SPECIFICATION_DOCUMENTATION_URL: &str =
    "https://github.com/mapbox/vector-tile-spec/tree/2.1";

/// The largest plain vector-tile payload a decoder accepts after any gzip
/// decompression. A tile payload is untrusted network input, so a decompression
/// bomb must fail as a typed refusal rather than exhaust the process; sixteen
/// megabytes is far above any real tile (a dense z14 city tile is tens of
/// kilobytes) while staying a bounded allocation.
pub const MAXIMUM_DECOMPRESSED_VECTOR_TILE_BYTES: usize = 16 * 1024 * 1024;
