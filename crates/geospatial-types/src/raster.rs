//! Raster tile imagery records.

use serde::{Deserialize, Serialize};

/// Encoded image formats a raster tile provider may deliver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RasterImageFormat {
    /// PNG imagery.
    Png,
    /// JPEG imagery.
    Jpeg,
}

/// Pixel resolution a raster tile is rendered at.
///
/// `Standard` is the plain tile form. `Retina` has twice the texels per edge
/// for the same logical tile span, which is the `@2x` request form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RasterTileResolution {
    /// One texel per logical tile pixel.
    #[default]
    Standard,
    /// Twice the texels per logical tile pixel (the `@2x` request form).
    Retina,
}

/// One fetched web-mercator XYZ raster tile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RasterTileData {
    /// Tile column coordinate at the given zoom level.
    pub tile_x: u32,
    /// Tile row coordinate at the given zoom level.
    pub tile_y: u32,
    /// Zoom level this tile belongs to.
    pub zoom_level: u8,
    /// Encoding of the byte payload.
    pub image_format: RasterImageFormat,
    /// Pixel resolution the payload was rendered at.
    ///
    /// A serialized payload that predates this field deserializes as
    /// [`RasterTileResolution::Standard`], the resolution of every tile
    /// recorded before the field existed.
    #[serde(default)]
    pub tile_resolution: RasterTileResolution,
    /// Encoded image bytes.
    pub image_bytes: Vec<u8>,
}
