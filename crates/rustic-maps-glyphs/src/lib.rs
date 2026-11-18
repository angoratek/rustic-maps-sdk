//! Glyph support for the symbol/text pipeline: the Mapbox glyph
//! protobuf reader, a byte-bounded range cache and an `R8Unorm` atlas packer.
//!
//! The crate is pure byte parsing and CPU-side packing. Its only runtime
//! dependencies are [`thiserror`] and the workspace's dependency-free
//! protobuf wire reader (`rustic-maps-vector-tiles`), so it builds on native targets
//! and on `wasm32-unknown-unknown` alike. Nothing here draws, touches a GPU
//! pipeline or places labels; the renderer's text pass consumes the packed atlas.
//!
//! # Scope: Latin only
//!
//! The reader parses whichever codepoints a producer serves, and the atlas
//! packs the Latin ranges first, but there is no shaping, bidirectional
//! ordering or ligature handling. Complex-script text (Arabic, Indic, …) is
//! explicitly out of scope until a shaping dependency is justified.
//!
//! Entry points: [`decode_glyph_font_stack`] turns a fetched glyph range into
//! a [`GlyphFontStack`], [`GlyphRangeCache`] retains decoded ranges under a
//! byte and entry ceiling, and [`pack_glyph_atlas`] lays glyphs out on one
//! `R8Unorm` page with per-glyph texture rectangles.

#![cfg_attr(not(test), deny(clippy::expect_used, clippy::unwrap_used))]

mod atlas;
mod error;
mod glyph;
mod range_cache;

pub use atlas::{
    DEFAULT_GLYPH_ATLAS_PADDING_PIXELS, DEFAULT_GLYPH_ATLAS_PAGE_HEIGHT_PIXELS,
    DEFAULT_GLYPH_ATLAS_PAGE_WIDTH_PIXELS, GLYPH_ATLAS_BYTES_PER_PIXEL, GlyphAtlasConfiguration,
    GlyphAtlasError, GlyphPixelRectangle, GlyphUvRectangle, MAXIMUM_GLYPH_ATLAS_PAGE_BYTE_COUNT,
    MAXIMUM_GLYPH_ATLAS_PAGE_EDGE_PIXELS, PackedGlyph, PackedGlyphAtlas, pack_glyph_atlas,
};
pub use error::GlyphError;
pub use glyph::{
    GLYPH_RANGE_CODEPOINT_COUNT, GLYPH_SIGNED_DISTANCE_FIELD_BORDER_PIXELS, Glyph, GlyphFontStack,
    GlyphRange, decode_glyph_font_stack,
};
pub use range_cache::{
    DEFAULT_MAXIMUM_CACHED_BYTE_COUNT, DEFAULT_MAXIMUM_CACHED_RANGE_COUNT, GlyphRangeCache,
    GlyphRangeCacheConfiguration, GlyphRangeKey,
};

/// The URL of the Mapbox glyph protocol this crate reads: a `glyphs` protobuf
/// message of font stacks, glyph metrics and 3-pixel-bordered signed-distance
/// fields.
pub const GLYPH_PROTOCOL_DOCUMENTATION_URL: &str =
    "https://github.com/mapbox/node-fontnik/blob/master/proto/glyphs.proto";
