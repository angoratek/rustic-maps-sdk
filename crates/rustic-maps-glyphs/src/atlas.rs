//! An `R8Unorm` glyph atlas packer.
//!
//! The packer copies decoded glyph bitmaps into one page of single-channel
//! 8-bit alpha pixels (the `R8Unorm` texture format the text pipeline uploads)
//! and answers each glyph's page rectangle, normalized texture
//! coordinates and advance. Packing is a deterministic shelf/row layout:
//! glyphs are ordered by codepoint with the Latin ranges first, then every
//! glyph is placed left to right on the current shelf, and a new shelf starts
//! whenever the next glyph does not fit horizontally. The page background is
//! zero (fully transparent), and the configured padding separates adjacent
//! glyph bitmaps so bilinear sampling cannot bleed between them.
//!
//! A page has a documented maximum edge of
//! [`MAXIMUM_GLYPH_ATLAS_PAGE_EDGE_PIXELS`] pixels; an input that cannot fit
//! reports a typed [`GlyphAtlasError`] rather than panicking or silently
//! dropping a glyph. Multi-page atlases and complex-script shaping are out of
//! scope: the packer covers the Latin ranges, and complex scripts need
//! shaping support outside this crate.

use crate::glyph::{Glyph, GlyphRange};
use thiserror::Error;

/// The default atlas page width in pixels.
pub const DEFAULT_GLYPH_ATLAS_PAGE_WIDTH_PIXELS: u32 = 1024;

/// The default atlas page height in pixels.
pub const DEFAULT_GLYPH_ATLAS_PAGE_HEIGHT_PIXELS: u32 = 1024;

/// The documented maximum atlas page edge in pixels. Both page dimensions are
/// bounded by this value, keeping one `R8Unorm` page within
/// [`MAXIMUM_GLYPH_ATLAS_PAGE_BYTE_COUNT`] bytes.
pub const MAXIMUM_GLYPH_ATLAS_PAGE_EDGE_PIXELS: u32 = 4096;

/// The byte size of a maximum-sized `R8Unorm` page (16 MiB).
pub const MAXIMUM_GLYPH_ATLAS_PAGE_BYTE_COUNT: usize =
    MAXIMUM_GLYPH_ATLAS_PAGE_EDGE_PIXELS as usize * MAXIMUM_GLYPH_ATLAS_PAGE_EDGE_PIXELS as usize;

/// The default number of pixels reserved between adjacent glyph bitmaps.
pub const DEFAULT_GLYPH_ATLAS_PADDING_PIXELS: u32 = 1;

/// The bytes one `R8Unorm` pixel occupies.
pub const GLYPH_ATLAS_BYTES_PER_PIXEL: usize = 1;

/// The packing priority of a glyph in the Latin ranges; lower packs first.
const LATIN_GLYPH_PACKING_PRIORITY: u8 = 0;

/// The packing priority of every other glyph; they follow the Latin ranges.
const NON_LATIN_GLYPH_PACKING_PRIORITY: u8 = 1;

/// The page dimensions and inter-glyph padding a packer uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlyphAtlasConfiguration {
    /// The page width in pixels.
    pub page_width_pixels: u32,
    /// The page height in pixels.
    pub page_height_pixels: u32,
    /// The pixels reserved to the right of and below each glyph bitmap.
    pub padding_pixels: u32,
}

impl Default for GlyphAtlasConfiguration {
    fn default() -> Self {
        Self {
            page_width_pixels: DEFAULT_GLYPH_ATLAS_PAGE_WIDTH_PIXELS,
            page_height_pixels: DEFAULT_GLYPH_ATLAS_PAGE_HEIGHT_PIXELS,
            padding_pixels: DEFAULT_GLYPH_ATLAS_PADDING_PIXELS,
        }
    }
}

/// Why a glyph set could not be packed into one atlas page.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GlyphAtlasError {
    /// A page dimension is zero.
    #[error("glyph atlas page {axis} must not be zero pixels")]
    ZeroPageDimension {
        /// Which page dimension is zero (`width` or `height`).
        axis: &'static str,
    },
    /// A page dimension exceeds the documented maximum.
    #[error(
        "glyph atlas page {axis} is {value} pixels, above the documented maximum of {maximum} pixels"
    )]
    PageDimensionExceedsMaximum {
        /// Which page dimension is too large (`width` or `height`).
        axis: &'static str,
        /// The supplied dimension.
        value: u32,
        /// The documented maximum.
        maximum: u32,
    },
    /// The padding does not leave room for a glyph on the page.
    #[error(
        "glyph atlas padding {padding_pixels} pixels must be smaller than the page {axis} of {page_dimension} pixels"
    )]
    PaddingExceedsPageDimension {
        /// The supplied padding in pixels.
        padding_pixels: u32,
        /// Which page dimension the padding was checked against.
        axis: &'static str,
        /// The supplied page dimension.
        page_dimension: u32,
    },
    /// The page's pixel count overflows addressable memory.
    #[error("glyph atlas page pixel count overflows addressable memory")]
    PagePixelCountOverflow,
    /// Two input glyphs declare the same codepoint.
    #[error("glyph {codepoint} appears more than once in the atlas input")]
    DuplicateGlyphCodepoint {
        /// The duplicated codepoint.
        codepoint: u32,
    },
    /// A glyph's bitmap length disagrees with its declared metrics.
    #[error(
        "glyph {codepoint} bitmap holds {actual_byte_count} bytes but its {bitmap_width_pixels}×{bitmap_height_pixels} signed-distance-field bitmap requires {expected_byte_count}"
    )]
    GlyphBitmapLengthMismatch {
        /// The glyph's Unicode codepoint.
        codepoint: u32,
        /// The padded signed-distance-field bitmap width in pixels.
        bitmap_width_pixels: u64,
        /// The padded signed-distance-field bitmap height in pixels.
        bitmap_height_pixels: u64,
        /// The byte length the bitmap must have.
        expected_byte_count: u64,
        /// The byte length the bitmap actually carries.
        actual_byte_count: u64,
    },
    /// A glyph's padded bitmap dimensions overflow addressable memory.
    #[error("glyph {codepoint} bitmap dimensions overflow addressable memory")]
    GlyphBitmapDimensionsOverflow {
        /// The glyph's Unicode codepoint.
        codepoint: u32,
    },
    /// One glyph's bitmap is larger than the whole page.
    #[error(
        "glyph {codepoint}'s {bitmap_width_pixels}×{bitmap_height_pixels} signed-distance-field bitmap does not fit a {page_width_pixels}×{page_height_pixels} atlas page"
    )]
    GlyphTooLargeForPage {
        /// The glyph's Unicode codepoint.
        codepoint: u32,
        /// The padded signed-distance-field bitmap width in pixels.
        bitmap_width_pixels: u32,
        /// The padded signed-distance-field bitmap height in pixels.
        bitmap_height_pixels: u32,
        /// The page width in pixels.
        page_width_pixels: u32,
        /// The page height in pixels.
        page_height_pixels: u32,
    },
    /// The page has no remaining shelf room for the next glyph.
    #[error(
        "the {page_width_pixels}×{page_height_pixels} atlas page cannot hold glyph {codepoint}; packing stopped after {packed_glyph_count} glyphs"
    )]
    PageExhausted {
        /// The glyph that did not fit.
        codepoint: u32,
        /// The page width in pixels.
        page_width_pixels: u32,
        /// The page height in pixels.
        page_height_pixels: u32,
        /// How many glyphs were placed before the failure.
        packed_glyph_count: usize,
    },
}

/// A glyph bitmap's pixel rectangle on the atlas page, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlyphPixelRectangle {
    /// The rectangle's left edge in pixels.
    pub x_pixels: u32,
    /// The rectangle's top edge in pixels.
    pub y_pixels: u32,
    /// The rectangle's width in pixels.
    pub width_pixels: u32,
    /// The rectangle's height in pixels.
    pub height_pixels: u32,
}

impl GlyphPixelRectangle {
    /// The rectangle a glyph without a bitmap occupies: no pixels at the
    /// page origin, keeping its advance without consuming atlas space.
    pub const EMPTY: Self = Self {
        x_pixels: 0,
        y_pixels: 0,
        width_pixels: 0,
        height_pixels: 0,
    };
}

/// A glyph bitmap's normalized texture rectangle.
///
/// Coordinates use the texture-space origin the renderer's `wgpu` pipeline
/// uses: `u` grows right and `v` grows down.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphUvRectangle {
    /// The rectangle's left edge, in `[0, 1]`.
    pub minimum_u: f32,
    /// The rectangle's top edge, in `[0, 1]`.
    pub minimum_v: f32,
    /// The rectangle's right edge, in `[0, 1]`.
    pub maximum_u: f32,
    /// The rectangle's bottom edge, in `[0, 1]`.
    pub maximum_v: f32,
}

impl GlyphUvRectangle {
    /// The rectangle a glyph without a bitmap occupies.
    pub const EMPTY: Self = Self {
        minimum_u: 0.0,
        minimum_v: 0.0,
        maximum_u: 0.0,
        maximum_v: 0.0,
    };
}

/// One glyph's placement and layout metrics after packing.
#[derive(Debug, Clone, PartialEq)]
pub struct PackedGlyph {
    /// The glyph's Unicode codepoint.
    pub codepoint: u32,
    /// The ink-box width in pixels, excluding the signed-distance-field border.
    pub width_pixels: u32,
    /// The ink-box height in pixels, excluding the signed-distance-field border.
    pub height_pixels: u32,
    /// The horizontal bearing from the pen origin to the ink box.
    pub left: i32,
    /// The vertical bearing from the baseline to the ink box.
    pub top: i32,
    /// The horizontal advance to the next pen position.
    pub advance: u32,
    /// The signed-distance-field bitmap's pixel rectangle; [`GlyphPixelRectangle::EMPTY`] when
    /// the glyph carried no bitmap.
    pub bitmap_rectangle: GlyphPixelRectangle,
    /// The signed-distance-field bitmap's normalized texture rectangle;
    /// [`GlyphUvRectangle::EMPTY`] when the glyph carried no bitmap.
    pub uv_rectangle: GlyphUvRectangle,
}

/// One packed `R8Unorm` atlas page and the placement of every input glyph.
#[derive(Debug, Clone, PartialEq)]
pub struct PackedGlyphAtlas {
    /// The page width in pixels.
    pub width_pixels: u32,
    /// The page height in pixels.
    pub height_pixels: u32,
    /// The `R8Unorm` page: one alpha byte per pixel, row-major, with a
    /// zero-filled background.
    pub pixels: Vec<u8>,
    /// The packed glyphs in packing order: Latin codepoints first, then the
    /// rest, each group ascending by codepoint.
    pub glyphs: Vec<PackedGlyph>,
}

impl PackedGlyphAtlas {
    /// Finds a packed glyph by codepoint.
    pub fn glyph(&self, codepoint: u32) -> Option<&PackedGlyph> {
        self.glyphs
            .iter()
            .find(|glyph| glyph.codepoint == codepoint)
    }
}

/// Packs the glyphs into one `R8Unorm` page under the supplied configuration.
///
/// The output is deterministic: the same glyph set always produces the same
/// rectangles and the same page bytes, regardless of input order. A glyph
/// without a bitmap (a space, for example) keeps its metrics and advance but
/// occupies no page space. Any glyph that does not fit reports
/// [`GlyphAtlasError::GlyphTooLargeForPage`] or
/// [`GlyphAtlasError::PageExhausted`].
pub fn pack_glyph_atlas(
    glyphs: &[Glyph],
    configuration: GlyphAtlasConfiguration,
) -> Result<PackedGlyphAtlas, GlyphAtlasError> {
    validate_configuration(configuration)?;
    let bitmap_dimensions = validate_glyphs(glyphs, configuration)?;

    let page_width_pixels = configuration.page_width_pixels;
    let page_height_pixels = configuration.page_height_pixels;
    let page_pixel_count =
        usize::try_from(u64::from(page_width_pixels) * u64::from(page_height_pixels))
            .map_err(|_| GlyphAtlasError::PagePixelCountOverflow)?;
    let mut pixels = vec![0u8; page_pixel_count * GLYPH_ATLAS_BYTES_PER_PIXEL];

    let mut glyph_indices: Vec<usize> = (0..glyphs.len()).collect();
    glyph_indices.sort_by_key(|&glyph_index| {
        let glyph = &glyphs[glyph_index];
        let packing_priority = if GlyphRange::is_latin_codepoint(glyph.codepoint) {
            LATIN_GLYPH_PACKING_PRIORITY
        } else {
            NON_LATIN_GLYPH_PACKING_PRIORITY
        };
        (packing_priority, glyph.codepoint)
    });

    let mut cursor_x_pixels = 0u32;
    let mut cursor_y_pixels = 0u32;
    let mut current_shelf_height_pixels = 0u32;
    let mut packed_glyphs = Vec::with_capacity(glyphs.len());
    for glyph_index in glyph_indices {
        let glyph = &glyphs[glyph_index];
        let (bitmap_width_pixels, bitmap_height_pixels) = bitmap_dimensions[glyph_index];
        let (bitmap_rectangle, uv_rectangle) =
            if bitmap_width_pixels == 0 || bitmap_height_pixels == 0 {
                (GlyphPixelRectangle::EMPTY, GlyphUvRectangle::EMPTY)
            } else {
                if cursor_x_pixels + bitmap_width_pixels > page_width_pixels {
                    cursor_y_pixels += current_shelf_height_pixels;
                    cursor_x_pixels = 0;
                    current_shelf_height_pixels = 0;
                }
                if cursor_y_pixels + bitmap_height_pixels > page_height_pixels {
                    return Err(GlyphAtlasError::PageExhausted {
                        codepoint: glyph.codepoint,
                        page_width_pixels,
                        page_height_pixels,
                        packed_glyph_count: packed_glyphs.len(),
                    });
                }
                let bitmap_rectangle = GlyphPixelRectangle {
                    x_pixels: cursor_x_pixels,
                    y_pixels: cursor_y_pixels,
                    width_pixels: bitmap_width_pixels,
                    height_pixels: bitmap_height_pixels,
                };
                copy_bitmap_into_page(
                    glyph,
                    bitmap_rectangle,
                    page_width_pixels as usize,
                    &mut pixels,
                );
                cursor_x_pixels += bitmap_width_pixels + configuration.padding_pixels;
                current_shelf_height_pixels = current_shelf_height_pixels
                    .max(bitmap_height_pixels + configuration.padding_pixels);
                (
                    bitmap_rectangle,
                    normalized_rectangle(bitmap_rectangle, page_width_pixels, page_height_pixels),
                )
            };
        packed_glyphs.push(PackedGlyph {
            codepoint: glyph.codepoint,
            width_pixels: glyph.width,
            height_pixels: glyph.height,
            left: glyph.left,
            top: glyph.top,
            advance: glyph.advance,
            bitmap_rectangle,
            uv_rectangle,
        });
    }

    Ok(PackedGlyphAtlas {
        width_pixels: page_width_pixels,
        height_pixels: page_height_pixels,
        pixels,
        glyphs: packed_glyphs,
    })
}

/// Checks the page dimensions and padding.
fn validate_configuration(configuration: GlyphAtlasConfiguration) -> Result<(), GlyphAtlasError> {
    if configuration.page_width_pixels == 0 {
        return Err(GlyphAtlasError::ZeroPageDimension { axis: "width" });
    }
    if configuration.page_height_pixels == 0 {
        return Err(GlyphAtlasError::ZeroPageDimension { axis: "height" });
    }
    if configuration.page_width_pixels > MAXIMUM_GLYPH_ATLAS_PAGE_EDGE_PIXELS {
        return Err(GlyphAtlasError::PageDimensionExceedsMaximum {
            axis: "width",
            value: configuration.page_width_pixels,
            maximum: MAXIMUM_GLYPH_ATLAS_PAGE_EDGE_PIXELS,
        });
    }
    if configuration.page_height_pixels > MAXIMUM_GLYPH_ATLAS_PAGE_EDGE_PIXELS {
        return Err(GlyphAtlasError::PageDimensionExceedsMaximum {
            axis: "height",
            value: configuration.page_height_pixels,
            maximum: MAXIMUM_GLYPH_ATLAS_PAGE_EDGE_PIXELS,
        });
    }
    if configuration.padding_pixels >= configuration.page_width_pixels {
        return Err(GlyphAtlasError::PaddingExceedsPageDimension {
            padding_pixels: configuration.padding_pixels,
            axis: "width",
            page_dimension: configuration.page_width_pixels,
        });
    }
    if configuration.padding_pixels >= configuration.page_height_pixels {
        return Err(GlyphAtlasError::PaddingExceedsPageDimension {
            padding_pixels: configuration.padding_pixels,
            axis: "height",
            page_dimension: configuration.page_height_pixels,
        });
    }
    Ok(())
}

/// Validates every glyph and returns its padded bitmap dimensions (`(0, 0)`
/// for a glyph without a bitmap).
fn validate_glyphs(
    glyphs: &[Glyph],
    configuration: GlyphAtlasConfiguration,
) -> Result<Vec<(u32, u32)>, GlyphAtlasError> {
    let mut seen_codepoints = std::collections::HashSet::with_capacity(glyphs.len());
    let mut bitmap_dimensions = Vec::with_capacity(glyphs.len());
    for glyph in glyphs {
        if !seen_codepoints.insert(glyph.codepoint) {
            return Err(GlyphAtlasError::DuplicateGlyphCodepoint {
                codepoint: glyph.codepoint,
            });
        }
        if glyph.bitmap.is_empty() {
            bitmap_dimensions.push((0, 0));
            continue;
        }
        let expected_byte_count = glyph.expected_bitmap_byte_count().ok_or(
            GlyphAtlasError::GlyphBitmapDimensionsOverflow {
                codepoint: glyph.codepoint,
            },
        )?;
        let actual_byte_count = glyph.bitmap.len() as u64;
        let bitmap_width_pixels = u32::try_from(glyph.bitmap_width_pixels()).map_err(|_| {
            GlyphAtlasError::GlyphBitmapDimensionsOverflow {
                codepoint: glyph.codepoint,
            }
        })?;
        let bitmap_height_pixels = u32::try_from(glyph.bitmap_height_pixels()).map_err(|_| {
            GlyphAtlasError::GlyphBitmapDimensionsOverflow {
                codepoint: glyph.codepoint,
            }
        })?;
        if expected_byte_count != actual_byte_count {
            return Err(GlyphAtlasError::GlyphBitmapLengthMismatch {
                codepoint: glyph.codepoint,
                bitmap_width_pixels: u64::from(bitmap_width_pixels),
                bitmap_height_pixels: u64::from(bitmap_height_pixels),
                expected_byte_count,
                actual_byte_count,
            });
        }
        if bitmap_width_pixels > configuration.page_width_pixels
            || bitmap_height_pixels > configuration.page_height_pixels
        {
            return Err(GlyphAtlasError::GlyphTooLargeForPage {
                codepoint: glyph.codepoint,
                bitmap_width_pixels,
                bitmap_height_pixels,
                page_width_pixels: configuration.page_width_pixels,
                page_height_pixels: configuration.page_height_pixels,
            });
        }
        bitmap_dimensions.push((bitmap_width_pixels, bitmap_height_pixels));
    }
    Ok(bitmap_dimensions)
}

/// Copies one glyph's bitmap rows into the page at its rectangle.
fn copy_bitmap_into_page(
    glyph: &Glyph,
    rectangle: GlyphPixelRectangle,
    page_width_pixels: usize,
    page: &mut [u8],
) {
    let bitmap_width_pixels = rectangle.width_pixels as usize;
    for row_index in 0..rectangle.height_pixels as usize {
        let source_start = row_index * bitmap_width_pixels;
        let source_end = source_start + bitmap_width_pixels;
        let destination_start = (rectangle.y_pixels as usize + row_index) * page_width_pixels
            + rectangle.x_pixels as usize;
        let destination_end = destination_start + bitmap_width_pixels;
        page[destination_start..destination_end]
            .copy_from_slice(&glyph.bitmap[source_start..source_end]);
    }
}

/// Converts a pixel rectangle to normalized texture coordinates for the page.
fn normalized_rectangle(
    rectangle: GlyphPixelRectangle,
    page_width_pixels: u32,
    page_height_pixels: u32,
) -> GlyphUvRectangle {
    let page_width = page_width_pixels as f32;
    let page_height = page_height_pixels as f32;
    GlyphUvRectangle {
        minimum_u: rectangle.x_pixels as f32 / page_width,
        minimum_v: rectangle.y_pixels as f32 / page_height,
        maximum_u: (rectangle.x_pixels + rectangle.width_pixels) as f32 / page_width,
        maximum_v: (rectangle.y_pixels + rectangle.height_pixels) as f32 / page_height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glyph::GLYPH_SIGNED_DISTANCE_FIELD_BORDER_PIXELS;

    /// A glyph whose bitmap matches its declared ink box.
    fn glyph_with_bitmap(
        codepoint: u32,
        width: u32,
        height: u32,
        advance: u32,
        bitmap_fill: u8,
    ) -> Glyph {
        let bitmap_width_pixels =
            width as usize + 2 * GLYPH_SIGNED_DISTANCE_FIELD_BORDER_PIXELS as usize;
        let bitmap_height_pixels =
            height as usize + 2 * GLYPH_SIGNED_DISTANCE_FIELD_BORDER_PIXELS as usize;
        Glyph {
            codepoint,
            bitmap: vec![bitmap_fill; bitmap_width_pixels * bitmap_height_pixels],
            width,
            height,
            left: 1,
            top: 2,
            advance,
        }
    }

    /// A glyph without a bitmap (a space).
    fn blank_glyph(codepoint: u32) -> Glyph {
        Glyph {
            codepoint,
            bitmap: Vec::new(),
            width: 0,
            height: 0,
            left: 0,
            top: 0,
            advance: 4,
        }
    }

    /// The small page the golden and determinism tests use.
    fn small_page_configuration() -> GlyphAtlasConfiguration {
        GlyphAtlasConfiguration {
            page_width_pixels: 32,
            page_height_pixels: 16,
            padding_pixels: 1,
        }
    }

    /// The fixed golden glyph set: three Latin glyphs (one blank) and one
    /// Cyrillic glyph.
    fn golden_glyphs() -> Vec<Glyph> {
        vec![
            glyph_with_bitmap(0x41, 3, 5, 7, 0xa5),
            glyph_with_bitmap(0x42, 2, 4, 6, 0x5a),
            blank_glyph(0x20),
            glyph_with_bitmap(0x0410, 3, 5, 8, 0x33),
        ]
    }

    #[test]
    fn packs_the_golden_glyph_set_into_known_rectangles() {
        let atlas = pack_glyph_atlas(&golden_glyphs(), small_page_configuration())
            .expect("the golden set fits the page");

        assert_eq!(atlas.width_pixels, 32);
        assert_eq!(atlas.height_pixels, 16);
        assert_eq!(atlas.pixels.len(), 32 * 16);
        assert_eq!(
            atlas
                .glyphs
                .iter()
                .map(|glyph| glyph.codepoint)
                .collect::<Vec<_>>(),
            vec![0x20, 0x41, 0x42, 0x0410]
        );

        let blank = atlas.glyph(0x20).expect("the blank glyph is packed");
        assert_eq!(blank.bitmap_rectangle, GlyphPixelRectangle::EMPTY);
        assert_eq!(blank.uv_rectangle, GlyphUvRectangle::EMPTY);
        assert_eq!(blank.advance, 4);

        let first = atlas.glyph(0x41).expect("the first glyph is packed");
        assert_eq!(
            first.bitmap_rectangle,
            GlyphPixelRectangle {
                x_pixels: 0,
                y_pixels: 0,
                width_pixels: 9,
                height_pixels: 11,
            }
        );
        assert_eq!(
            first.uv_rectangle,
            GlyphUvRectangle {
                minimum_u: 0.0,
                minimum_v: 0.0,
                maximum_u: 9.0 / 32.0,
                maximum_v: 11.0 / 16.0,
            }
        );

        let second = atlas.glyph(0x42).expect("the second glyph is packed");
        assert_eq!(
            second.bitmap_rectangle,
            GlyphPixelRectangle {
                x_pixels: 10,
                y_pixels: 0,
                width_pixels: 8,
                height_pixels: 10,
            }
        );
        assert_eq!(
            second.uv_rectangle,
            GlyphUvRectangle {
                minimum_u: 10.0 / 32.0,
                minimum_v: 0.0,
                maximum_u: 18.0 / 32.0,
                maximum_v: 10.0 / 16.0,
            }
        );

        let cyrillic = atlas.glyph(0x0410).expect("the Cyrillic glyph is packed");
        assert_eq!(
            cyrillic.bitmap_rectangle,
            GlyphPixelRectangle {
                x_pixels: 19,
                y_pixels: 0,
                width_pixels: 9,
                height_pixels: 11,
            }
        );
        assert_eq!(
            cyrillic.uv_rectangle,
            GlyphUvRectangle {
                minimum_u: 19.0 / 32.0,
                minimum_v: 0.0,
                maximum_u: 28.0 / 32.0,
                maximum_v: 11.0 / 16.0,
            }
        );

        // The bitmap bytes are copied row-major and the background stays zero.
        assert_eq!(atlas.pixels[0], 0xa5);
        assert_eq!(atlas.pixels[31 * 16 - 1], 0);
    }

    #[test]
    fn packing_is_deterministic_regardless_of_input_order() {
        let configuration = small_page_configuration();
        let forward = pack_glyph_atlas(&golden_glyphs(), configuration).expect("fits");
        let mut reversed_glyphs = golden_glyphs();
        reversed_glyphs.reverse();
        let reversed = pack_glyph_atlas(&reversed_glyphs, configuration).expect("fits");

        assert_eq!(forward, reversed);
    }

    #[test]
    fn latin_glyphs_pack_before_non_latin_glyphs() {
        let glyphs = vec![
            glyph_with_bitmap(0x0410, 3, 5, 8, 0x33),
            glyph_with_bitmap(0x41, 3, 5, 7, 0xa5),
        ];
        let atlas = pack_glyph_atlas(&glyphs, small_page_configuration()).expect("fits");

        assert_eq!(
            atlas
                .glyphs
                .iter()
                .map(|glyph| glyph.codepoint)
                .collect::<Vec<_>>(),
            vec![0x41, 0x0410]
        );
    }

    #[test]
    fn a_glyph_larger_than_the_page_is_reported() {
        let glyphs = vec![glyph_with_bitmap(0x41, 3, 5, 7, 0xa5)];
        let configuration = GlyphAtlasConfiguration {
            page_width_pixels: 8,
            page_height_pixels: 64,
            padding_pixels: 1,
        };
        let failure = pack_glyph_atlas(&glyphs, configuration).expect_err("the bitmap is 9 wide");

        assert_eq!(
            failure,
            GlyphAtlasError::GlyphTooLargeForPage {
                codepoint: 0x41,
                bitmap_width_pixels: 9,
                bitmap_height_pixels: 11,
                page_width_pixels: 8,
                page_height_pixels: 64,
            }
        );
    }

    #[test]
    fn a_full_page_reports_rather_than_panicking() {
        let glyphs = vec![
            glyph_with_bitmap(0x41, 3, 5, 7, 0xa5),
            glyph_with_bitmap(0x42, 3, 5, 7, 0x5a),
        ];
        let configuration = GlyphAtlasConfiguration {
            page_width_pixels: 16,
            page_height_pixels: 14,
            padding_pixels: 1,
        };
        let failure =
            pack_glyph_atlas(&glyphs, configuration).expect_err("the second shelf overflows");

        assert_eq!(
            failure,
            GlyphAtlasError::PageExhausted {
                codepoint: 0x42,
                page_width_pixels: 16,
                page_height_pixels: 14,
                packed_glyph_count: 1,
            }
        );
    }

    #[test]
    fn duplicate_codepoints_are_refused() {
        let glyphs = vec![
            glyph_with_bitmap(0x41, 3, 5, 7, 0xa5),
            glyph_with_bitmap(0x41, 3, 5, 7, 0x5a),
        ];
        let failure = pack_glyph_atlas(&glyphs, small_page_configuration())
            .expect_err("two glyphs share a codepoint");

        assert_eq!(
            failure,
            GlyphAtlasError::DuplicateGlyphCodepoint { codepoint: 0x41 }
        );
    }

    #[test]
    fn a_bitmap_length_mismatch_is_reported() {
        let mut glyph = glyph_with_bitmap(0x41, 3, 5, 7, 0xa5);
        glyph.bitmap.pop();
        let failure = pack_glyph_atlas(&[glyph], small_page_configuration())
            .expect_err("the bitmap is one byte short");

        assert!(matches!(
            failure,
            GlyphAtlasError::GlyphBitmapLengthMismatch { .. }
        ));
    }

    #[test]
    fn invalid_page_configurations_are_refused() {
        let glyphs = [glyph_with_bitmap(0x41, 3, 5, 7, 0xa5)];

        let zero_width = GlyphAtlasConfiguration {
            page_width_pixels: 0,
            page_height_pixels: 16,
            padding_pixels: 0,
        };
        assert_eq!(
            pack_glyph_atlas(&glyphs, zero_width).expect_err("zero width"),
            GlyphAtlasError::ZeroPageDimension { axis: "width" }
        );

        let oversized = GlyphAtlasConfiguration {
            page_width_pixels: MAXIMUM_GLYPH_ATLAS_PAGE_EDGE_PIXELS + 1,
            page_height_pixels: 16,
            padding_pixels: 0,
        };
        assert_eq!(
            pack_glyph_atlas(&glyphs, oversized).expect_err("too wide"),
            GlyphAtlasError::PageDimensionExceedsMaximum {
                axis: "width",
                value: MAXIMUM_GLYPH_ATLAS_PAGE_EDGE_PIXELS + 1,
                maximum: MAXIMUM_GLYPH_ATLAS_PAGE_EDGE_PIXELS,
            }
        );

        let excessive_padding = GlyphAtlasConfiguration {
            page_width_pixels: 16,
            page_height_pixels: 16,
            padding_pixels: 16,
        };
        assert_eq!(
            pack_glyph_atlas(&glyphs, excessive_padding).expect_err("padding fills the page"),
            GlyphAtlasError::PaddingExceedsPageDimension {
                padding_pixels: 16,
                axis: "width",
                page_dimension: 16,
            }
        );
    }

    #[test]
    fn an_empty_glyph_set_produces_a_zeroed_page() {
        let atlas = pack_glyph_atlas(
            &[],
            GlyphAtlasConfiguration {
                page_width_pixels: 4,
                page_height_pixels: 4,
                padding_pixels: 1,
            },
        )
        .expect("an empty set fits");

        assert_eq!(atlas.glyphs, Vec::new());
        assert_eq!(atlas.pixels, vec![0u8; 16]);
    }
}
