//! The sprite atlas: a style's sprite index document and its sheet decoded
//! into one texture the icon pass samples.
//!
//! A MapLibre style names a sprite base URL. The host resolves that URL through
//! an injected [`SpriteSource`] and hands back a [`SpriteAtlas`]; the renderer
//! never fetches, so a test binds the URL to committed fixtures and a product
//! keeps the fetch in its own transport. The index follows the published
//! sprite-index format: an object keyed by icon name whose members are the
//! icon's rectangle in the sheet (`x`, `y`, `width`, `height`) and the pixel
//! ratio it was authored at.
//!
//! A malformed index, a missing icon and an icon rectangle outside the sheet
//! all degrade to "no icon" rather than failing a frame: an icon that cannot
//! be sampled is not painted, and the rest of the map (and the layer's text)
//! still draws.

use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use serde_json::Value as JsonValue;

/// The pixel ratio a sprite-index entry uses when it omits one.
pub const DEFAULT_SPRITE_PIXEL_RATIO: f64 = 1.0;

/// Bytes in one RGBA8 sprite-sheet texel.
pub const SPRITE_ATLAS_PIXEL_BYTES: u32 = 4;

/// One icon's rectangle inside the sprite sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct SpriteIcon {
    /// The rectangle's left edge, in sheet texels.
    pub x_pixels: u32,
    /// The rectangle's top edge, in sheet texels.
    pub y_pixels: u32,
    /// The rectangle's width, in sheet texels.
    pub width_pixels: u32,
    /// The rectangle's height, in sheet texels.
    pub height_pixels: u32,
    /// The device-pixel ratio the icon was authored at; the display size is
    /// the sheet rectangle divided by this ratio.
    pub pixel_ratio: f64,
}

impl SpriteIcon {
    /// The icon's display size at `scale`, in CSS pixels.
    pub fn display_size_pixels(&self, scale: f64) -> [f64; 2] {
        let pixel_ratio = if self.pixel_ratio.is_finite() && self.pixel_ratio > 0.0 {
            self.pixel_ratio
        } else {
            DEFAULT_SPRITE_PIXEL_RATIO
        };
        [
            f64::from(self.width_pixels) / pixel_ratio * scale,
            f64::from(self.height_pixels) / pixel_ratio * scale,
        ]
    }
}

/// A parsed sprite index: every icon name and its rectangle.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SpriteIndex {
    icons: BTreeMap<String, SpriteIcon>,
}

impl SpriteIndex {
    /// Parses a sprite index document.
    ///
    /// A top-level entry that is not an object, lacks a numeric rectangle or
    /// declares a zero-sized rectangle is skipped rather than failing the
    /// index, so one malformed icon cannot blank the whole sheet.
    ///
    /// # Errors
    /// Invalid JSON and a non-object root report [`SpriteAtlasError`].
    pub fn parse_json(index_json: &str) -> Result<Self, SpriteAtlasError> {
        let root: JsonValue =
            serde_json::from_str(index_json).map_err(|json_error| SpriteAtlasError {
                detail: format!("the sprite index is not valid JSON: {json_error}"),
            })?;
        let object = root.as_object().ok_or_else(|| SpriteAtlasError {
            detail: "the sprite index must be an object keyed by icon name".to_string(),
        })?;
        let mut icons = BTreeMap::new();
        for (icon_name, icon_value) in object {
            let Some(icon_object) = icon_value.as_object() else {
                continue;
            };
            let Some(x_pixels) = json_unsigned(icon_object.get("x")) else {
                continue;
            };
            let Some(y_pixels) = json_unsigned(icon_object.get("y")) else {
                continue;
            };
            let Some(width_pixels) = json_unsigned(icon_object.get("width")) else {
                continue;
            };
            let Some(height_pixels) = json_unsigned(icon_object.get("height")) else {
                continue;
            };
            if width_pixels == 0 || height_pixels == 0 {
                continue;
            }
            let pixel_ratio = icon_object
                .get("pixelRatio")
                .and_then(JsonValue::as_f64)
                .filter(|ratio| ratio.is_finite() && *ratio > 0.0)
                .unwrap_or(DEFAULT_SPRITE_PIXEL_RATIO);
            icons.insert(
                icon_name.clone(),
                SpriteIcon {
                    x_pixels,
                    y_pixels,
                    width_pixels,
                    height_pixels,
                    pixel_ratio,
                },
            );
        }
        Ok(Self { icons })
    }

    /// The icon the index declares under `icon_name`.
    pub fn icon(&self, icon_name: &str) -> Option<&SpriteIcon> {
        self.icons.get(icon_name)
    }

    /// How many icons the index declares.
    pub fn len(&self) -> usize {
        self.icons.len()
    }

    /// Whether the index declares no icons.
    pub fn is_empty(&self) -> bool {
        self.icons.is_empty()
    }
}

/// One icon's normalized texture window in the atlas, `[0, 1]` on each axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpriteTextureRectangle {
    /// The window's left edge.
    pub minimum_u: f32,
    /// The window's top edge.
    pub minimum_v: f32,
    /// The window's right edge.
    pub maximum_u: f32,
    /// The window's bottom edge.
    pub maximum_v: f32,
}

/// A sprite index paired with its decoded sheet: the RGBA8 pixels the icon
/// pass samples and the atlas' deterministic content revision.
#[derive(Debug, Clone, PartialEq)]
pub struct SpriteAtlas {
    index: SpriteIndex,
    width_pixels: u32,
    height_pixels: u32,
    rgba_pixels: Vec<u8>,
    revision: u64,
}

impl SpriteAtlas {
    /// Builds an atlas from an already-decoded RGBA8 sheet.
    ///
    /// # Errors
    /// A pixel buffer whose length is not `width × height × 4` reports
    /// [`SpriteAtlasError`]; the atlas never samples past its pixels.
    pub fn from_index_and_rgba(
        index: SpriteIndex,
        width_pixels: u32,
        height_pixels: u32,
        rgba_pixels: Vec<u8>,
    ) -> Result<Self, SpriteAtlasError> {
        let expected_byte_count = u64::from(width_pixels)
            * u64::from(height_pixels)
            * u64::from(SPRITE_ATLAS_PIXEL_BYTES);
        if rgba_pixels.len() as u64 != expected_byte_count {
            return Err(SpriteAtlasError {
                detail: format!(
                    "the sprite sheet pixels hold {} bytes, but {} by {} RGBA needs {}",
                    rgba_pixels.len(),
                    width_pixels,
                    height_pixels,
                    expected_byte_count
                ),
            });
        }
        let revision = atlas_revision(&index, width_pixels, height_pixels, &rgba_pixels);
        Ok(Self {
            index,
            width_pixels,
            height_pixels,
            rgba_pixels,
            revision,
        })
    }

    /// Builds an atlas from a sprite index document and an encoded PNG sheet.
    ///
    /// # Errors
    /// A malformed index, an undecodable image or one outside the SDK decode
    /// bounds, or a pixel buffer of the wrong size reports [`SpriteAtlasError`].
    pub fn from_index_and_sheet_png(
        index_json: &[u8],
        sheet_png: &[u8],
    ) -> Result<Self, SpriteAtlasError> {
        let index_text =
            std::str::from_utf8(index_json).map_err(|utf8_error| SpriteAtlasError {
                detail: format!("the sprite index is not UTF-8: {utf8_error}"),
            })?;
        let index = SpriteIndex::parse_json(index_text)?;
        let sheet =
            crate::image_limits::decode_image_with_limits(sheet_png).map_err(|image_error| {
                SpriteAtlasError {
                    detail: format!("the sprite sheet is not a decodable image: {image_error}"),
                }
            })?;
        let rgba_sheet = sheet.to_rgba8();
        let (width_pixels, height_pixels) = rgba_sheet.dimensions();
        Self::from_index_and_rgba(index, width_pixels, height_pixels, rgba_sheet.into_raw())
    }

    /// The icon the atlas serves, when the index declares it and its rectangle
    /// lies inside the sheet.
    pub fn icon(&self, icon_name: &str) -> Option<&SpriteIcon> {
        let icon = self.index.icon(icon_name)?;
        let right_edge = icon.x_pixels.checked_add(icon.width_pixels)?;
        let bottom_edge = icon.y_pixels.checked_add(icon.height_pixels)?;
        (right_edge <= self.width_pixels && bottom_edge <= self.height_pixels).then_some(icon)
    }

    /// The icon's normalized texture window.
    pub fn texture_rectangle(&self, icon: &SpriteIcon) -> SpriteTextureRectangle {
        let width_pixels = self.width_pixels.max(1) as f32;
        let height_pixels = self.height_pixels.max(1) as f32;
        let minimum_u = icon.x_pixels as f32 / width_pixels;
        let minimum_v = icon.y_pixels as f32 / height_pixels;
        SpriteTextureRectangle {
            minimum_u,
            minimum_v,
            maximum_u: (icon.x_pixels + icon.width_pixels) as f32 / width_pixels,
            maximum_v: (icon.y_pixels + icon.height_pixels) as f32 / height_pixels,
        }
    }

    /// The sheet's width in texels.
    pub fn width_pixels(&self) -> u32 {
        self.width_pixels
    }

    /// The sheet's height in texels.
    pub fn height_pixels(&self) -> u32 {
        self.height_pixels
    }

    /// The sheet's RGBA8 pixels, row-major from the top-left corner.
    pub fn rgba_pixels(&self) -> &[u8] {
        &self.rgba_pixels
    }

    /// The atlas' deterministic content revision; a backend uploads the
    /// texture when this changes and reuses it otherwise.
    pub fn revision(&self) -> u64 {
        self.revision
    }
}

/// A host-supplied sprite loader: the style's `sprite` URL in, the decoded
/// atlas out.
///
/// Injection keeps the renderer free of URLs and credentials: a test binds the
/// URL to committed fixtures, an application to its own transport and cache.
pub trait SpriteSource: Send + Sync {
    /// Loads the sprite index and sheet a style's `sprite` URL names.
    ///
    /// # Errors
    /// A URL the source does not serve and a malformed index or sheet report
    /// [`SpriteAtlasError`]; the frame then draws no icons.
    fn load_sprite(&self, sprite_url: &str) -> Result<SpriteAtlas, SpriteAtlasError>;
}

/// Why a sprite index or sheet could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpriteAtlasError {
    /// The underlying parse or decode detail.
    pub detail: String,
}

impl std::fmt::Display for SpriteAtlasError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "sprite atlas invalid: {}", self.detail)
    }
}

impl std::error::Error for SpriteAtlasError {}

/// A deterministic fingerprint of the atlas' index and sheet pixels.
fn atlas_revision(
    index: &SpriteIndex,
    width_pixels: u32,
    height_pixels: u32,
    rgba_pixels: &[u8],
) -> u64 {
    let mut hasher = DefaultHasher::new();
    width_pixels.hash(&mut hasher);
    height_pixels.hash(&mut hasher);
    for (icon_name, icon) in &index.icons {
        icon_name.hash(&mut hasher);
        icon.x_pixels.hash(&mut hasher);
        icon.y_pixels.hash(&mut hasher);
        icon.width_pixels.hash(&mut hasher);
        icon.height_pixels.hash(&mut hasher);
        icon.pixel_ratio.to_bits().hash(&mut hasher);
    }
    rgba_pixels.hash(&mut hasher);
    hasher.finish()
}

/// A JSON number as an unsigned sheet coordinate. A negative or fractional
/// value is not a texel coordinate and yields `None`.
fn json_unsigned(json_value: Option<&JsonValue>) -> Option<u32> {
    let number = json_value?.as_f64()?;
    if !number.is_finite() || number < 0.0 || number.fract() != 0.0 || number > f64::from(u32::MAX)
    {
        return None;
    }
    Some(number as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    const INDEX_JSON: &str = r#"{
        "railway": { "x": 0, "y": 0, "width": 16, "height": 16, "pixelRatio": 1 },
        "retina": { "x": 16, "y": 0, "width": 32, "height": 32, "pixelRatio": 2 },
        "broken": { "x": "left", "y": 0, "width": 4, "height": 4 }
    }"#;

    #[test]
    fn the_index_parses_rectangles_pixel_ratios_and_skips_malformed_entries() {
        let index = SpriteIndex::parse_json(INDEX_JSON).expect("the fixture index parses");
        let railway = index.icon("railway").expect("railway is declared");
        assert_eq!((railway.x_pixels, railway.y_pixels), (0, 0));
        assert_eq!((railway.width_pixels, railway.height_pixels), (16, 16));
        assert_eq!(railway.pixel_ratio, 1.0);
        assert_eq!(
            index
                .icon("retina")
                .expect("retina is declared")
                .pixel_ratio,
            2.0
        );
        assert!(
            index.icon("broken").is_none(),
            "a malformed rectangle is skipped"
        );
        assert_eq!(index.len(), 2);
        assert!(!index.is_empty());
    }

    #[test]
    fn an_icon_display_size_divides_the_sheet_rectangle_by_its_pixel_ratio() {
        let index = SpriteIndex::parse_json(INDEX_JSON).expect("the fixture index parses");
        let retina = index.icon("retina").expect("retina is declared");
        assert_eq!(retina.display_size_pixels(1.0), [16.0, 16.0]);
        assert_eq!(retina.display_size_pixels(1.5), [24.0, 24.0]);
    }

    #[test]
    fn a_sheet_builds_an_atlas_and_reports_normalized_texture_windows() {
        let index = SpriteIndex::parse_json(INDEX_JSON).expect("the fixture index parses");
        let atlas = SpriteAtlas::from_index_and_rgba(index, 48, 16, vec![0; 48 * 16 * 4])
            .expect("the sheet size matches");
        let railway = atlas
            .icon("railway")
            .expect("railway lies inside the sheet");
        let texture = atlas.texture_rectangle(railway);
        assert_eq!(
            texture,
            SpriteTextureRectangle {
                minimum_u: 0.0,
                minimum_v: 0.0,
                maximum_u: 16.0 / 48.0,
                maximum_v: 1.0,
            }
        );
        assert_eq!(atlas.width_pixels(), 48);
        assert_eq!(atlas.height_pixels(), 16);
        assert_eq!(atlas.rgba_pixels().len(), 48 * 16 * 4);
    }

    #[test]
    fn an_icon_outside_the_sheet_is_not_served_and_a_bad_pixel_buffer_is_an_error() {
        let index = SpriteIndex::parse_json(
            r#"{ "outside": { "x": 40, "y": 0, "width": 16, "height": 16 } }"#,
        )
        .expect("the fixture index parses");
        let atlas = SpriteAtlas::from_index_and_rgba(index, 48, 16, vec![0; 48 * 16 * 4])
            .expect("the sheet size matches");
        assert!(
            atlas.icon("outside").is_none(),
            "an icon rectangle that runs past the sheet must not be sampled"
        );
        let index = SpriteIndex::parse_json(INDEX_JSON).expect("the fixture index parses");
        assert!(
            SpriteAtlas::from_index_and_rgba(index, 48, 16, vec![0; 3]).is_err(),
            "a pixel buffer of the wrong size is refused rather than sampled out of bounds"
        );
    }

    #[test]
    fn the_revision_changes_with_the_sheet_pixels_and_the_index() {
        let index = SpriteIndex::parse_json(INDEX_JSON).expect("the fixture index parses");
        let first = SpriteAtlas::from_index_and_rgba(index.clone(), 48, 16, vec![0; 48 * 16 * 4])
            .expect("the sheet size matches");
        let second = SpriteAtlas::from_index_and_rgba(index.clone(), 48, 16, vec![0; 48 * 16 * 4])
            .expect("the sheet size matches");
        let mut changed_pixels = vec![0; 48 * 16 * 4];
        changed_pixels[0] = 255;
        let changed = SpriteAtlas::from_index_and_rgba(index, 48, 16, changed_pixels)
            .expect("the sheet size matches");
        assert_eq!(first.revision(), second.revision());
        assert_ne!(first.revision(), changed.revision());
    }
}
