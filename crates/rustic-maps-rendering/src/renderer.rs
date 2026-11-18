//! GPU-free frame assembly: prepared tiles mapped to drawable quads.
//!
//! The pure half of the renderer. A [`Renderer`] consumes the camera pose, the
//! viewport, and the tiles the [`crate::tile_scheduler::TileScheduler`]
//! prepared for the frame, and produces the list of textured quads to draw in
//! normalized world coordinates, including parent-fallback quads for tiles
//! that the texture pool does not hold.
//!
//! Quads are projected into per-fragment clip-space vertex arrays: a world
//! span is wrapped around the camera as one unit (never corner by corner) and
//! split into at most one vertex array per world copy it reaches, so spans
//! crossing the antimeridian render contiguously instead of tearing. See
//! [`FrameProjection`].
//!
//! Everything in this module is free of `wgpu` types, so the quad assembly,
//! the fallback texture window math, and the clip-space projection of the quad
//! corners are all unit-testable without a GPU.

use crate::camera::MapCamera;
use crate::camera::ViewportPixels;
use crate::projection::{coordinate_to_normalized_world, zoom_scale_pixels};
use crate::tile_key::TileKey;
use crate::tile_scheduler::{PreparedTile, TileReadiness};
use crate::tile_texture_pool::TileTexturePool;
use glam::{DMat4, DVec4};

/// Backdrop color behind the map. Opaque black: a tile that never arrives
/// blends into the void instead of flashing white.
pub const CLEAR_COLOR_RGBA: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// Normalized-device-coordinate bounds of the viewport in both axes. Quads
/// whose entire projected bounding box lies outside these bounds are culled
/// during assembly.
pub const VIEWPORT_NORMALIZED_DEVICE_BOUNDS: [f32; 2] = [1.0, 1.0];

/// How many world copies one tile span can reach after wrapping. A span is at
/// most one world wide (the zoom-0 root tile), so wrapping it next to the
/// camera-centered window can cross at most one copy boundary.
const WORLD_COPY_FRAGMENTS_MAXIMUM: usize = 2;

/// Half-width of the camera-centered world window in normalized-world units:
/// wrapped span positions beyond it belong to the adjacent world copy, where
/// coordinates continue from the window edge. Shared with the overlay
/// assembly (`crate::overlay`), which splits its shapes against the same
/// world-copy windows.
pub(crate) const CAMERA_WORLD_WINDOW_HALF_WIDTH: f64 = 0.5;

/// Normalized-world rectangle one tile occupies: x grows eastward and y grows
/// southward across `[0, 1]` (web-mercator convention), so the tile grid
/// subdivides the unit square from the north.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TileWorldSpan {
    /// Western edge (inclusive).
    pub minimum_x: f64,
    /// Northern edge (inclusive).
    pub minimum_y: f64,
    /// Eastern edge (exclusive).
    pub maximum_x: f64,
    /// Southern edge (exclusive).
    pub maximum_y: f64,
}

impl TileWorldSpan {
    /// The normalized-world span of a single tile at its own zoom level:
    /// tile `(tx, ty, z)` covers `x ∈ [tx / 2^z, (tx + 1) / 2^z]` and the
    /// matching y range.
    pub fn covering_tile(tile_key: TileKey) -> Self {
        let tiles_per_axis = 2f64.powi(i32::from(tile_key.zoom_level));
        let tile_span = 1.0 / tiles_per_axis;
        Self {
            minimum_x: tile_key.tile_x as f64 * tile_span,
            minimum_y: tile_key.tile_y as f64 * tile_span,
            maximum_x: (tile_key.tile_x as f64 + 1.0) * tile_span,
            maximum_y: (tile_key.tile_y as f64 + 1.0) * tile_span,
        }
    }

    /// Horizontal extent of the span.
    pub fn width(&self) -> f64 {
        self.maximum_x - self.minimum_x
    }

    /// Vertical extent of the span.
    pub fn height(&self) -> f64 {
        self.maximum_y - self.minimum_y
    }

    /// Whether another span sits fully inside this span (boundaries
    /// inclusive). Tile spans of an ancestor chain meet at shared edges.
    pub fn contains(&self, other: &TileWorldSpan) -> bool {
        self.minimum_x <= other.minimum_x
            && self.minimum_y <= other.minimum_y
            && other.maximum_x <= self.maximum_x
            && other.maximum_y <= self.maximum_y
    }
}

/// The sub-window of a texture that a quad samples: u grows rightward with
/// the texture columns and v grows downward with the texture rows, so at v =
/// 0 the image's top row — the northern edge of a tile — is sampled. All
/// components lie in `[0, 1]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextureWindow {
    /// Leftmost sampled column.
    pub minimum_u: f64,
    /// Topmost (northernmost) sampled row.
    pub minimum_v: f64,
    /// Rightmost sampled column.
    pub maximum_u: f64,
    /// Bottommost (southernmost) sampled row.
    pub maximum_v: f64,
}

/// A retired tile zoom level drawn over the newly requested one while a zoom
/// crosses a tile level.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RetainedTileLevel {
    /// The retired level whose pooled tiles stay on screen.
    pub tile_zoom_level: u8,
    /// The retired level's alpha: one while it is held, ramping to zero as
    /// the new level takes over.
    pub opacity: f32,
}

/// One textured quad to draw for a frame, in normalized world coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TileQuad {
    /// Tile whose pooled payload supplies the pixels. This is the requested
    /// tile itself, or its nearest pooled ancestor for a fallback draw.
    pub texture_tile_key: TileKey,
    /// The normalized-world rectangle the quad covers — always the requested
    /// tile's own span, so a fallback draws ancestor pixels at the child's
    /// geographic position.
    pub world_span: TileWorldSpan,
    /// Which part of the texture the quad samples (the full texture for a
    /// present tile; the ancestor's covering window for a fallback).
    pub texture_window: TextureWindow,
    /// The quad's alpha. Pooled tiles draw opaque; a retired zoom level
    /// fading out into the new one ramps it from one to zero.
    pub opacity: f32,
}

impl TileQuad {
    /// The quad covering a pooled tile's full own texture.
    fn present_tile(tile_key: TileKey) -> Self {
        Self {
            texture_tile_key: tile_key,
            world_span: TileWorldSpan::covering_tile(tile_key),
            texture_window: TextureWindow {
                minimum_u: 0.0,
                minimum_v: 0.0,
                maximum_u: 1.0,
                maximum_v: 1.0,
            },
            opacity: 1.0,
        }
    }

    /// The quad drawing a fallback ancestor's pixels over the requested
    /// child's own span. Only the covering sub-window of the ancestor texture
    /// is sampled, so the child's geographic footprint shows exactly the
    /// ancestor content that covers it, and nothing outside it.
    fn fallback_tile(requested_tile_key: TileKey, fallback_tile_key: TileKey) -> Self {
        let requested_span = TileWorldSpan::covering_tile(requested_tile_key);
        let fallback_span = TileWorldSpan::covering_tile(fallback_tile_key);
        Self {
            texture_tile_key: fallback_tile_key,
            world_span: requested_span,
            texture_window: TextureWindow {
                minimum_u: (requested_span.minimum_x - fallback_span.minimum_x)
                    / fallback_span.width(),
                minimum_v: (requested_span.minimum_y - fallback_span.minimum_y)
                    / fallback_span.height(),
                maximum_u: (requested_span.maximum_x - fallback_span.minimum_x)
                    / fallback_span.width(),
                maximum_v: (requested_span.maximum_y - fallback_span.minimum_y)
                    / fallback_span.height(),
            },
            opacity: 1.0,
        }
    }
}

/// One quad corner as consumed by the GPU pipeline: a position in clip space
/// (homogeneous coordinates; the perspective divide happens on the GPU) and
/// the texture coordinates to sample there. `repr(C)` keeps the field order
/// the vertex buffer layout expects.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ClipSpaceTileVertex {
    /// Position in clip space, `[x, y, z, w]`.
    pub clip_position: [f32; 4],
    /// Texture-coordinate sample point.
    pub texture_coordinates: [f32; 2],
    /// The quad's alpha, multiplied into the sampled texel's alpha by the
    /// fragment stage.
    pub opacity: f32,
}

/// Per-pass projection context: the view-projection matrix, the world pixel
/// scale, and the camera's normalized-world x, built once per frame and
/// threaded through fragment projection and culling. Rebuilding the matrix
/// per quad was an order of magnitude of redundant `f64` matrix math.
#[derive(Debug, Clone, Copy)]
struct FrameProjection {
    view_projection_matrix: DMat4,
    zoom_scale_pixels: f64,
    center_normalized_x: f64,
}

impl FrameProjection {
    /// Builds the projection context for one frame.
    fn for_frame(
        camera: &MapCamera,
        viewport: &ViewportPixels,
        field_of_view_degrees: f64,
    ) -> Self {
        Self {
            view_projection_matrix: camera.view_projection_matrix(viewport, field_of_view_degrees),
            zoom_scale_pixels: zoom_scale_pixels(camera.zoom_level),
            center_normalized_x: coordinate_to_normalized_world(&camera.center)[0],
        }
    }

    /// Projects a quad into zero, one, or two clip-space vertex arrays — one
    /// per world-copy fragment the span reaches around the camera's copy.
    ///
    /// The span wraps as one unit: the minimum corner's wrap delta (shared
    /// with the camera, in `(-0.5, 0.5]`) fixes the span's world copy, and
    /// every remaining x position advances continuously from it. Wrapping
    /// corners independently tears any span crossing the world-copy boundary,
    /// because the far corner flicks to the opposite edge and inverts or
    /// collapses
    /// the quad — a zoom-0 root tile spanned the whole world and collapsed to
    /// a single point.
    ///
    /// Each fragment is the span's intersection with one camera-centered
    /// world copy window and carries the proportionally split texture window,
    /// so each fragment draws its copy's portion of the texture at the right
    /// screen place. A fragment whose every corner lies behind the camera is
    /// dropped; that is the only case losing coverage, since anything with a
    /// corner in front still reaches viewable ground.
    fn quad_fragments(&self, quad: &TileQuad) -> Vec<[ClipSpaceTileVertex; 4]> {
        let span = &quad.world_span;
        let window = &quad.texture_window;
        let span_width = span.width();
        let minimum_wrapped_x = self.center_normalized_x
            + crate::camera::normalized_shortest_delta(self.center_normalized_x, span.minimum_x);
        let wrapped_maximum_x = minimum_wrapped_x + span_width;

        let mut fragments = Vec::with_capacity(WORLD_COPY_FRAGMENTS_MAXIMUM);
        for copy_index in 0..WORLD_COPY_FRAGMENTS_MAXIMUM {
            let copy_offset = copy_index as f64;
            let copy_window_start =
                self.center_normalized_x - CAMERA_WORLD_WINDOW_HALF_WIDTH + copy_offset;
            let copy_window_end = copy_window_start + 2.0 * CAMERA_WORLD_WINDOW_HALF_WIDTH;
            let fragment_start_x = minimum_wrapped_x.max(copy_window_start);
            let fragment_end_x = wrapped_maximum_x.min(copy_window_end);
            if fragment_end_x <= fragment_start_x {
                continue;
            }
            // The fragment's sub-window of the texture, split linearly by its
            // fraction of the span so the pixels keep their geographic place.
            let start_span_fraction = (fragment_start_x - minimum_wrapped_x) / span_width;
            let end_span_fraction = (fragment_end_x - minimum_wrapped_x) / span_width;
            let u_range = window.maximum_u - window.minimum_u;
            let fragment_window = TextureWindow {
                minimum_u: window.minimum_u + u_range * start_span_fraction,
                minimum_v: window.minimum_v,
                maximum_u: window.minimum_u + u_range * end_span_fraction,
                maximum_v: window.maximum_v,
            };
            let fragment = [
                (
                    fragment_start_x,
                    span.minimum_y,
                    fragment_window.minimum_u,
                    fragment_window.minimum_v,
                ),
                (
                    fragment_end_x,
                    span.minimum_y,
                    fragment_window.maximum_u,
                    fragment_window.minimum_v,
                ),
                (
                    fragment_start_x,
                    span.maximum_y,
                    fragment_window.minimum_u,
                    fragment_window.maximum_v,
                ),
                (
                    fragment_end_x,
                    span.maximum_y,
                    fragment_window.maximum_u,
                    fragment_window.maximum_v,
                ),
            ]
            .map(|(wrapped_x, world_y, texture_u, texture_v)| {
                // Camera world space is y-north pixel space (see
                // `MapCamera::ground_position_vector`); copy `copy_index`
                // continues coordinates past the camera window by subtracting
                // the copy offset, placing the span's wrapped remainder at the
                // screen edge the wrap points to.
                let x_pixels = (wrapped_x - copy_offset) * self.zoom_scale_pixels;
                let y_pixels = -world_y * self.zoom_scale_pixels;
                let clip_position =
                    self.view_projection_matrix * DVec4::new(x_pixels, y_pixels, 0.0, 1.0);
                ClipSpaceTileVertex {
                    clip_position: [
                        clip_position.x as f32,
                        clip_position.y as f32,
                        clip_position.z as f32,
                        clip_position.w as f32,
                    ],
                    texture_coordinates: [texture_u as f32, texture_v as f32],
                    opacity: quad.opacity,
                }
            });
            if fragment.iter().all(|vertex| vertex.clip_position[3] <= 0.0) {
                continue;
            }
            fragments.push(fragment);
        }
        fragments
    }

    /// Whether any part of a quad's fragments reaches the viewport.
    fn quad_reaches_viewport(&self, quad: &TileQuad) -> bool {
        self.quad_fragments(quad)
            .iter()
            .any(fragment_reaches_viewport)
    }
}

/// Whether any part of one fragment's projected area reaches the viewport.
///
/// Any corner with `w <= 0` disables culling for the fragment. Corners behind
/// the near plane divide (per-corner, before the GPU's homogeneous clip) into
/// coordinates far outside the bounds, and the fragment's genuinely visible
/// ground strip — the part the GPU's exact frustum clip cuts into view — can
/// sit inside the viewport while every surviving corner's divided bounding
/// box does not. The measured failure band at a 75-degree field of view is a
/// few degrees wide near 32 degrees of pitch; culling such a strip left a
/// visible map hole. A conservative `true` costs an extra draw the GPU clips.
fn fragment_reaches_viewport(fragment: &[ClipSpaceTileVertex; 4]) -> bool {
    if fragment.iter().any(|vertex| vertex.clip_position[3] <= 0.0) {
        return true;
    }
    let (mut minimum_screen_x, mut maximum_screen_x) = (f32::INFINITY, f32::NEG_INFINITY);
    let (mut minimum_screen_y, mut maximum_screen_y) = (f32::INFINITY, f32::NEG_INFINITY);
    for vertex in fragment {
        let clip_position = vertex.clip_position;
        // Every corner is in front (checked above), so the divide is sound.
        let screen_x = clip_position[0] / clip_position[3];
        let screen_y = clip_position[1] / clip_position[3];
        minimum_screen_x = minimum_screen_x.min(screen_x);
        maximum_screen_x = maximum_screen_x.max(screen_x);
        minimum_screen_y = minimum_screen_y.min(screen_y);
        maximum_screen_y = maximum_screen_y.max(screen_y);
    }
    let [bound_x, bound_y] = VIEWPORT_NORMALIZED_DEVICE_BOUNDS;
    minimum_screen_x <= bound_x
        && maximum_screen_x >= -bound_x
        && minimum_screen_y <= bound_y
        && maximum_screen_y >= -bound_y
}

/// Quad assembler for one frame: turns prepared tiles into draw items and
/// projects their corners into clip space. Construct it once and reuse it
/// across frames; it holds only the field of view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Renderer {
    field_of_view_degrees: f64,
}

impl Default for Renderer {
    fn default() -> Self {
        Self {
            field_of_view_degrees: crate::camera::DEFAULT_FIELD_OF_VIEW_DEGREES,
        }
    }
}

impl Renderer {
    /// Builds a quad assembler with a non-default field of view.
    pub fn new(field_of_view_degrees: f64) -> Self {
        Self {
            field_of_view_degrees,
        }
    }

    /// The field of view every frame assembly of this renderer assumes; the
    /// overlay assembly of the same frame needs the same projection pose.
    pub fn field_of_view_degrees(&self) -> f64 {
        self.field_of_view_degrees
    }

    /// Assembles the frame's textured quads, culled to the viewport.
    ///
    /// `Present` tiles contribute a quad with their full texture; tiles
    /// resolved as [`TileReadiness::FallsBackTo`] contribute a quad over the
    /// requested tile's own span sampling the nearest pooled ancestor's
    /// texture. `Unavailable` and `Deferred` tiles draw nothing this frame —
    /// the scheduler will resolve them in a later frame. A pooled payload
    /// that left the pool between scheduling and assembly — or one the GPU
    /// stage later finds undecodable, which it skips and evicts so the
    /// scheduler refetches (see the headless renderer) — draws nothing rather
    /// than guessing.
    ///
    /// Culling is per fragment and conservative around the near plane; see
    /// [`FrameProjection::quad_fragments`]. The view-projection matrix is
    /// built once per frame and threaded through projection and culling.
    ///
    /// # Parameters
    /// - `camera` — the frame's camera pose.
    /// - `viewport` — the viewport the frame targets, in physical pixels.
    /// - `prepared_tiles` — the scheduler's readiness verdicts for this frame.
    /// - `pool` — the pooled payloads the quads draw from.
    ///
    /// # Returns
    /// The drawable quads in prepared-tile order.
    pub fn frame_quads(
        &self,
        camera: &MapCamera,
        viewport: &ViewportPixels,
        prepared_tiles: &[PreparedTile],
        pool: &TileTexturePool,
    ) -> Vec<TileQuad> {
        let frame_projection =
            FrameProjection::for_frame(camera, viewport, self.field_of_view_degrees);
        prepared_tiles
            .iter()
            .filter_map(|prepared_tile| {
                match prepared_tile.readiness {
                    TileReadiness::Present if pool.contains(&prepared_tile.requested_tile_key) => {
                        Some(TileQuad::present_tile(prepared_tile.requested_tile_key))
                    }
                    TileReadiness::FallsBackTo {
                        fallback_tile_key,
                        zoom_level_difference,
                    } if pool.contains(&fallback_tile_key) => {
                        debug_assert_eq!(
                            u32::from(fallback_tile_key.zoom_level)
                                + u32::from(zoom_level_difference),
                            u32::from(prepared_tile.requested_tile_key.zoom_level)
                        );
                        Some(TileQuad::fallback_tile(
                            prepared_tile.requested_tile_key,
                            fallback_tile_key,
                        ))
                    }
                    TileReadiness::Present
                    | TileReadiness::FallsBackTo { .. }
                    | TileReadiness::Unavailable
                    | TileReadiness::Deferred => None,
                }
                .filter(|quad| frame_projection.quad_reaches_viewport(quad))
            })
            .collect()
    }

    /// The quads of a tile zoom level that is being faded out: every pooled
    /// tile of `retained_tile_zoom_level` whose area the viewport reaches,
    /// drawn at `opacity`. They cover the same ground as the requested
    /// level's quads and are projected with the current camera, so the tile
    /// spans scale the retired level into place as the zoom changes. Tiles
    /// the pool no longer holds contribute nothing; the frame simply falls
    /// back to the new level for that area.
    pub fn retained_level_quads(
        &self,
        camera: &MapCamera,
        viewport: &ViewportPixels,
        retained_tile_zoom_level: u8,
        pool: &TileTexturePool,
        opacity: f32,
    ) -> Vec<TileQuad> {
        let frame_projection =
            FrameProjection::for_frame(camera, viewport, self.field_of_view_degrees);
        camera
            .visible_tile_keys(
                viewport,
                self.field_of_view_degrees,
                retained_tile_zoom_level,
            )
            .into_iter()
            .filter(|tile_key| pool.contains(tile_key))
            .map(|tile_key| {
                let mut quad = TileQuad::present_tile(tile_key);
                quad.opacity = opacity;
                quad
            })
            .filter(|quad| frame_projection.quad_reaches_viewport(quad))
            .collect()
    }

    /// Projects every quad of a frame into per-fragment vertex arrays,
    /// preserving quad order: entry `i` holds the 0, 1, or 2 world-copy
    /// fragments of quad `i`. Pairs with [`Self::frame_quads`], whose quads
    /// are already viewport-culled. The view-projection matrix is built once.
    pub fn quad_fragment_vertices(
        &self,
        camera: &MapCamera,
        viewport: &ViewportPixels,
        quads: &[TileQuad],
    ) -> Vec<Vec<[ClipSpaceTileVertex; 4]>> {
        let frame_projection =
            FrameProjection::for_frame(camera, viewport, self.field_of_view_degrees);
        quads
            .iter()
            .map(|quad| frame_projection.quad_fragments(quad))
            .collect()
    }

    /// Flattens [`Self::quad_fragment_vertices`] into one vertex array per
    /// fragment, dropping quads that lie entirely behind the camera.
    pub fn quad_vertices(
        &self,
        camera: &MapCamera,
        viewport: &ViewportPixels,
        quads: &[TileQuad],
    ) -> Vec<[ClipSpaceTileVertex; 4]> {
        self.quad_fragment_vertices(camera, viewport, quads)
            .into_iter()
            .flatten()
            .collect()
    }

    /// Convenience variant of [`Self::quad_fragment_vertices`] for a single
    /// quad; it rebuilds the projection per call, so frame-scale callers pass
    /// through the batched methods instead.
    pub fn tile_vertices_in_clip_space(
        &self,
        camera: &MapCamera,
        viewport: &ViewportPixels,
        quad: &TileQuad,
    ) -> Vec<[ClipSpaceTileVertex; 4]> {
        self.quad_fragment_vertices(camera, viewport, std::slice::from_ref(quad))
            .into_iter()
            .next()
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Renderer, TextureWindow, TileQuad, TileWorldSpan, VIEWPORT_NORMALIZED_DEVICE_BOUNDS,
    };
    use crate::camera::{MapCamera, ViewportPixels};
    use crate::projection::{normalized_world_to_coordinate, tile_key_for_coordinate};
    use crate::tile_key::TileKey;
    use crate::tile_scheduler::{PreparedTile, TileReadiness};
    use crate::tile_texture_pool::TileTexturePool;

    /// How far a known clip-space projection may drift from its analytic
    /// value through matrix round-off.
    const CLIP_POSITION_TOLERANCE: f64 = 1e-4;

    // Parameters of the near-plane cull regression sweep: the defect measured
    // at a 75-degree field of view across a band of pitches, at any zoom (the
    // tile footprint is 512 pixels at every tile zoom, so the pixel geometry
    // is zoom-invariant).
    /// Field of view where the culling defect reproduces.
    const CULL_BAND_FIELD_OF_VIEW_DEGREES: f64 = 75.0;
    /// Lowest pitch of the regression sweep.
    const CULL_BAND_MINIMUM_PITCH_DEGREES: f64 = 30.0;
    /// Pitch step of the regression sweep.
    const CULL_BAND_PITCH_STEP_DEGREES: f64 = 0.5;
    /// Pitch steps in the regression sweep.
    const CULL_BAND_PITCH_STEPS: u32 = 25;
    /// Camera tile zoom of the regression sweep.
    const CULL_BAND_ZOOM_LEVEL: u8 = 10;
    /// Tiles examined around the anchor tile in each axis of the sweep.
    const CULL_BAND_TILE_NEIGHBORHOOD: i64 = 4;
    /// Interior sample points per tile edge when probing where a tile's
    /// ground area projects into the viewport.
    const CULL_BAND_SAMPLES_PER_TILE_EDGE: usize = 24;

    fn tile_key(tile_x: u32, tile_y: u32, zoom_level: u8) -> TileKey {
        TileKey::new(tile_x, tile_y, zoom_level).unwrap()
    }

    fn square_viewport() -> ViewportPixels {
        ViewportPixels::new(512.0, 512.0).unwrap()
    }

    fn camera_above(normalized_world_center: [f64; 2], zoom_level: f64) -> MapCamera {
        MapCamera::new(
            normalized_world_to_coordinate(normalized_world_center),
            zoom_level,
            0.0,
            0.0,
        )
    }

    /// Camera hovering over tile `(0, 0, 1)`'s center at its own zoom, so the
    /// tile spans exactly the visible ground square of the test viewport.
    fn quadrant_camera() -> MapCamera {
        camera_above([0.25, 0.25], 1.0)
    }

    fn prepared(requested_tile_key: TileKey, readiness: TileReadiness) -> Vec<PreparedTile> {
        vec![PreparedTile {
            requested_tile_key,
            readiness,
        }]
    }

    /// Divides a vertex's clip coordinates to its normalized-device x/y.
    fn divided_position(vertex: &super::ClipSpaceTileVertex) -> [f64; 2] {
        [
            f64::from(vertex.clip_position[0]) / f64::from(vertex.clip_position[3]),
            f64::from(vertex.clip_position[1]) / f64::from(vertex.clip_position[3]),
        ]
    }

    /// Asserts a fragment's corners sit at the expected normalized-device
    /// positions and sample the expected texture coordinates.
    fn assert_fragment(
        fragment: &[super::ClipSpaceTileVertex; 4],
        expected_positions: [[f64; 2]; 4],
        expected_uvs: [[f64; 2]; 4],
        context: &str,
    ) {
        for (corner_index, vertex) in fragment.iter().enumerate() {
            let divided = divided_position(vertex);
            assert!(
                (divided[0] - expected_positions[corner_index][0]).abs() < CLIP_POSITION_TOLERANCE
                    && (divided[1] - expected_positions[corner_index][1]).abs()
                        < CLIP_POSITION_TOLERANCE,
                "{context}: corner {corner_index} divided {divided:?} vs {expected:?}",
                expected = expected_positions[corner_index]
            );
            let texture = [
                f64::from(vertex.texture_coordinates[0]),
                f64::from(vertex.texture_coordinates[1]),
            ];
            assert!(
                (texture[0] - expected_uvs[corner_index][0]).abs() < CLIP_POSITION_TOLERANCE
                    && (texture[1] - expected_uvs[corner_index][1]).abs() < CLIP_POSITION_TOLERANCE,
                "{context}: corner {corner_index} texture {texture:?} vs {expected:?}",
                expected = expected_uvs[corner_index]
            );
        }
    }

    #[test]
    fn tile_spans_follow_the_grid() {
        let root_span = TileWorldSpan::covering_tile(tile_key(0, 0, 0));
        assert_eq!(
            root_span,
            TileWorldSpan {
                minimum_x: 0.0,
                minimum_y: 0.0,
                maximum_x: 1.0,
                maximum_y: 1.0,
            }
        );
        let quadrants = 2f64.powi(1);
        let north_east_quadrant = TileWorldSpan::covering_tile(tile_key(1, 0, 1));
        assert_eq!(north_east_quadrant.minimum_x, 1.0 / quadrants);
        assert_eq!(north_east_quadrant.minimum_y, 0.0);
        assert!(root_span.contains(&north_east_quadrant));
    }

    #[test]
    fn a_present_tile_samples_its_full_texture() {
        let tile = tile_key(0, 0, 1);
        let mut payload_pool = TileTexturePool::new(1 << 20);
        payload_pool.insert(tile, vec![1, 2, 3]);
        let renderer = Renderer::default();
        let quads = renderer.frame_quads(
            &quadrant_camera(),
            &square_viewport(),
            &prepared(tile, TileReadiness::Present),
            &payload_pool,
        );
        assert_eq!(
            quads,
            vec![TileQuad {
                texture_tile_key: tile,
                world_span: TileWorldSpan {
                    minimum_x: 0.0,
                    minimum_y: 0.0,
                    maximum_x: 0.5,
                    maximum_y: 0.5,
                },
                texture_window: TextureWindow {
                    minimum_u: 0.0,
                    minimum_v: 0.0,
                    maximum_u: 1.0,
                    maximum_v: 1.0,
                },
                opacity: 1.0,
            }]
        );
    }

    #[test]
    fn a_fallback_draws_the_ancestor_texture_over_the_childs_own_span() {
        // Child (5, 5, 4) is the north-east quadrant of parent (2, 2, 3):
        // its span is the parent's span subdivided exactly at the child's
        // grid slot.
        let child = tile_key(5, 5, 4);
        let parent = child.parent_tile_key().unwrap();
        assert_eq!(parent, tile_key(2, 2, 3));
        let child_span = TileWorldSpan::covering_tile(child);
        let parent_span = TileWorldSpan::covering_tile(parent);
        assert!(parent_span.contains(&child_span));

        let mut payload_pool = TileTexturePool::new(1 << 20);
        payload_pool.insert(parent, vec![7]);
        let child_center_world = [
            (child_span.minimum_x + child_span.maximum_x) / 2.0,
            (child_span.minimum_y + child_span.maximum_y) / 2.0,
        ];
        let renderer = Renderer::default();
        let quads = renderer.frame_quads(
            &camera_above(child_center_world, 4.0),
            &square_viewport(),
            &prepared(
                child,
                TileReadiness::FallsBackTo {
                    fallback_tile_key: parent,
                    zoom_level_difference: 1,
                },
            ),
            &payload_pool,
        );
        assert_eq!(
            quads,
            vec![TileQuad {
                texture_tile_key: parent,
                world_span: child_span,
                texture_window: TextureWindow {
                    minimum_u: 0.5,
                    minimum_v: 0.5,
                    maximum_u: 1.0,
                    maximum_v: 1.0,
                },
                opacity: 1.0,
            }]
        );
    }

    #[test]
    fn a_two_level_fallback_samples_the_ancestors_covering_window() {
        // (10, 7, 4) descends through (5, 3, 3) to (2, 1, 2), so the child
        // span covers the ancestor's north-east quadrant window of 0.25.
        let child = tile_key(10, 7, 4);
        let ancestor = tile_key(2, 1, 2);
        let mut payload_pool = TileTexturePool::new(1 << 20);
        payload_pool.insert(ancestor, vec![7]);
        let renderer = Renderer::default();
        let quads = renderer.frame_quads(
            &camera_above([0.65625, 0.46875], 4.0),
            &square_viewport(),
            &prepared(
                child,
                TileReadiness::FallsBackTo {
                    fallback_tile_key: ancestor,
                    zoom_level_difference: 2,
                },
            ),
            &payload_pool,
        );
        let window = quads[0].texture_window;
        let child_span = TileWorldSpan::covering_tile(child);
        let ancestor_span = TileWorldSpan::covering_tile(ancestor);
        let expected_window = TextureWindow {
            minimum_u: (child_span.minimum_x - ancestor_span.minimum_x) / ancestor_span.width(),
            minimum_v: (child_span.minimum_y - ancestor_span.minimum_y) / ancestor_span.height(),
            maximum_u: (child_span.maximum_x - ancestor_span.minimum_x) / ancestor_span.width(),
            maximum_v: (child_span.maximum_y - ancestor_span.minimum_y) / ancestor_span.height(),
        };
        assert_eq!(window, expected_window);
        assert!(
            (window.maximum_u - window.minimum_u - 0.25).abs() < CLIP_POSITION_TOLERANCE,
            "the texture window must cover one 2^zoom_level_difference subdivision"
        );
    }

    #[test]
    fn unresolved_tiles_draw_nothing() {
        let payload_pool = TileTexturePool::new(1 << 20);
        let renderer = Renderer::default();
        for readiness in [
            TileReadiness::Unavailable,
            TileReadiness::Deferred,
            TileReadiness::Present,
            TileReadiness::FallsBackTo {
                fallback_tile_key: tile_key(0, 0, 0),
                zoom_level_difference: 3,
            },
        ] {
            let quads = renderer.frame_quads(
                &quadrant_camera(),
                &square_viewport(),
                &prepared(tile_key(3, 2, 2), readiness),
                &payload_pool,
            );
            assert!(quads.is_empty(), "readiness {readiness:?} drew a quad");
        }
    }

    #[test]
    fn a_fully_offscreen_tile_is_dropped() {
        // At zoom 2 hovered from x = 0.3, the north-east edge tile (3, 0, 2)
        // spans x [0.75, 1.0]: it wraps wholly into the adjacent world copy,
        // whose remainder projects to device x [-3.2, -1.2] — wholly beyond
        // the left screen edge with every corner in front, so the bounding
        // box cull drops the tile without ever reaching the GPU.
        let renderer = Renderer::default();
        let mut payload_pool = TileTexturePool::new(1 << 20);
        payload_pool.insert(tile_key(3, 0, 2), vec![1]);
        let quads = renderer.frame_quads(
            &camera_above([0.3, 0.25], 2.0),
            &square_viewport(),
            &prepared(tile_key(3, 0, 2), TileReadiness::Present),
            &payload_pool,
        );
        assert!(quads.is_empty());
        assert_eq!(VIEWPORT_NORMALIZED_DEVICE_BOUNDS, [1.0, 1.0]);
    }

    #[test]
    fn the_visible_tile_square_fills_the_viewport_on_screen() {
        // Camera altitude, viewport extent, and field of view line up over a
        // square 512-pixel viewport at zoom 1: the tile under the camera
        // spans exactly the visible ground square, so its quad reaches both
        // viewport edges, and north stays at the screen top (world y grows
        // southward, and the projection keeps it there).
        let renderer = Renderer::default();
        let camera = quadrant_camera();
        let quad = TileQuad::present_tile(tile_key(0, 0, 1));
        let viewport = square_viewport();
        let fragments = renderer.tile_vertices_in_clip_space(&camera, &viewport, &quad);
        assert_eq!(
            fragments.len(),
            1,
            "a span inside the camera's world copy must not fragment"
        );
        // Corners in quad order (northwest, northeast, southwest,
        // southeast). The homogeneous denominator is the camera-space depth,
        // which is positive for every ground corner, so only the divided
        // x/y parts are asserted.
        let expected_xy: [[f64; 2]; 4] = [[-1.0, 1.0], [1.0, 1.0], [-1.0, -1.0], [1.0, -1.0]];
        for (vertex, expected) in fragments[0].iter().zip(expected_xy) {
            assert!(vertex.clip_position[3] > 0.0, "corner behind the camera");
            let divided = divided_position(vertex);
            assert!(
                (divided[0] - expected[0]).abs() < CLIP_POSITION_TOLERANCE,
                "x component {divided:?} vs {expected:?}"
            );
            assert!(
                (divided[1] - expected[1]).abs() < CLIP_POSITION_TOLERANCE,
                "y component {divided:?} vs {expected:?}"
            );
        }
    }

    #[test]
    fn the_root_tile_fills_the_viewport_across_the_world_copy_seam() {
        // Regression: wrapping each corner independently collapsed the
        // zoom-0 root tile hovered from x = 0.3 — the east corner wrapped
        // back onto the west corner's position, degenerating a whole-world
        // span to one point. The span must wrap as a unit at its minimum
        // corner and split into the two world-copy fragments that together
        // cover the viewport, east content forward (never mirrored).
        let renderer = Renderer::default();
        let camera = camera_above([0.3, 0.5], 0.0);
        let viewport = square_viewport();
        let root_quad = TileQuad::present_tile(tile_key(0, 0, 0));
        let fragments = renderer.tile_vertices_in_clip_space(&camera, &viewport, &root_quad);
        assert_eq!(
            fragments.len(),
            2,
            "the root span reaches two world copies around the camera"
        );
        // Fragment 0 is the camera's own copy: the span's x [0, 0.8) at
        // normalized device x [-0.6, 1.0]; fragment 1 continues the wrapped
        // remainder [0.8, 1.0] at device x [-1.0, -0.6]. Corner order as
        // everywhere else: northwest, northeast, southwest, southeast.
        let expected_positions = [
            [[-0.6, 1.0], [1.0, 1.0], [-0.6, -1.0], [1.0, -1.0]],
            [[-1.0, 1.0], [-0.6, 1.0], [-1.0, -1.0], [-0.6, -1.0]],
        ];
        let expected_uvs = [
            [[0.0, 0.0], [0.8, 0.0], [0.0, 1.0], [0.8, 1.0]],
            [[0.8, 0.0], [1.0, 0.0], [0.8, 1.0], [1.0, 1.0]],
        ];
        for (fragment_index, fragment) in fragments.iter().enumerate() {
            for vertex in fragment {
                assert!(vertex.clip_position[3] > 0.0, "corner behind the camera");
            }
            assert_fragment(
                fragment,
                expected_positions[fragment_index],
                expected_uvs[fragment_index],
                "root tile fragment {fragment_index}",
            );
            // Non-mirrored orientation: texture u and device x must grow
            // together across each fragment's west and east corners.
            let [west, east] = [
                divided_position(&fragment[0]),
                divided_position(&fragment[1]),
            ];
            assert!(
                east[0] > west[0],
                "fragment {fragment_index} drew eastward texture content westward"
            );
        }
        // The two fragments together cover the whole viewport horizontally.
        let mut fragments_minimum_x = f64::INFINITY;
        let mut fragments_maximum_x = f64::NEG_INFINITY;
        for fragment in &fragments {
            for vertex in fragment {
                let divided = divided_position(vertex);
                fragments_minimum_x = fragments_minimum_x.min(divided[0]);
                fragments_maximum_x = fragments_maximum_x.max(divided[0]);
            }
        }
        assert!((fragments_minimum_x + 1.0).abs() < CLIP_POSITION_TOLERANCE);
        assert!((fragments_maximum_x - 1.0).abs() < CLIP_POSITION_TOLERANCE);
        // The quad survives frame assembly, feeding both fragments downstream.
        let mut payload_pool = TileTexturePool::new(1 << 20);
        payload_pool.insert(tile_key(0, 0, 0), vec![1]);
        let quads = renderer.frame_quads(
            &camera,
            &viewport,
            &prepared(tile_key(0, 0, 0), TileReadiness::Present),
            &payload_pool,
        );
        assert_eq!(quads.len(), 1);
        assert_eq!(
            renderer.quad_vertices(&camera, &viewport, &quads).len(),
            2,
            "both fragments reach the vertex stage"
        );
    }

    #[test]
    fn a_seam_straddling_span_splits_with_a_proportional_texture_window() {
        // At zoom 1 hovered from x = 0.3, tile (1, 0, 1) spans x [0.5, 1.0]:
        // the seam at x = 0.5 lands between the camera's world copies, so the
        // span draws its camera-copy part [0.5, 0.8) at device x [0.8, 2.0]
        // and its wrapped remainder [0.8, 1.0] at device x [-2.0, -1.2]. The
        // texture window splits proportionally: 0.6 of the span width lies in
        // the first fragment, so its u-range is the first 0.6 of the window.
        let renderer = Renderer::default();
        let camera = camera_above([0.3, 0.25], 1.0);
        let viewport = square_viewport();
        let quad = TileQuad::present_tile(tile_key(1, 0, 1));
        let fragments = renderer.tile_vertices_in_clip_space(&camera, &viewport, &quad);
        assert_eq!(fragments.len(), 2);
        let expected_positions = [
            [[0.8, 1.0], [2.0, 1.0], [0.8, -1.0], [2.0, -1.0]],
            [[-2.0, 1.0], [-1.2, 1.0], [-2.0, -1.0], [-1.2, -1.0]],
        ];
        let expected_uvs = [
            [[0.0, 0.0], [0.6, 0.0], [0.0, 1.0], [0.6, 1.0]],
            [[0.6, 0.0], [1.0, 0.0], [0.6, 1.0], [1.0, 1.0]],
        ];
        for (fragment_index, fragment) in fragments.iter().enumerate() {
            assert_fragment(
                fragment,
                expected_positions[fragment_index],
                expected_uvs[fragment_index],
                "tile (1,0,1) fragment {fragment_index}",
            );
        }
        // Frame assembly keeps the tile through its visible camera-copy
        // fragment; the wrapped remainder projects wholly off-screen with
        // every corner in front, so the cull drops it there.
        let mut payload_pool = TileTexturePool::new(1 << 20);
        payload_pool.insert(tile_key(1, 0, 1), vec![1]);
        let quads = renderer.frame_quads(
            &camera,
            &viewport,
            &prepared(tile_key(1, 0, 1), TileReadiness::Present),
            &payload_pool,
        );
        assert_eq!(quads.len(), 1);
    }

    #[test]
    fn a_near_plane_straddling_tile_is_not_culled_out_of_view() {
        // Regression for bounding-box culling over divided coordinates: a
        // tile straddling the near plane keeps genuinely visible ground inside
        // the viewport while every surviving corner may divide far outside one
        // screen edge, so the old per-fragment bounding-box test dropped it.
        // For every swept pitch, any neighborhood tile with a sample-projected
        // ground point inside the viewport must survive assembly.
        //
        // The pixel geometry is zoom-invariant (tiles are 512 pixels and the
        // camera altitude is fixed), so one zoom covers the mechanism; the
        // default 60-degree field of view keeps straddling regions outside the
        // frustum, so the sweep runs at the wider field of view where the
        // defect measured.
        let renderer = Renderer::new(CULL_BAND_FIELD_OF_VIEW_DEGREES);
        let viewport = square_viewport();
        let tiles_per_axis = 2f64.powi(i32::from(CULL_BAND_ZOOM_LEVEL));
        let camera_phase_offset = 0.5 / 8.0 / tiles_per_axis;
        let anchor_world = [0.5 + camera_phase_offset, 0.5 + camera_phase_offset];
        for pitch_step in 0..CULL_BAND_PITCH_STEPS {
            let pitch_degrees = CULL_BAND_MINIMUM_PITCH_DEGREES
                + f64::from(pitch_step) * CULL_BAND_PITCH_STEP_DEGREES;
            let camera = MapCamera::new(
                normalized_world_to_coordinate(anchor_world),
                f64::from(CULL_BAND_ZOOM_LEVEL),
                0.0,
                pitch_degrees,
            );
            let anchor_tile = tile_key_for_coordinate(&camera.center, CULL_BAND_ZOOM_LEVEL);
            let zoom_tile_maximum = (1u32 << CULL_BAND_ZOOM_LEVEL) - 1;
            let mut neighborhood_tiles = Vec::new();
            for delta_x in -CULL_BAND_TILE_NEIGHBORHOOD..=CULL_BAND_TILE_NEIGHBORHOOD {
                for delta_y in -CULL_BAND_TILE_NEIGHBORHOOD..=CULL_BAND_TILE_NEIGHBORHOOD {
                    let tile_x = (i64::from(anchor_tile.tile_x) + delta_x)
                        .clamp(0, i64::from(zoom_tile_maximum));
                    let tile_y = (i64::from(anchor_tile.tile_y) + delta_y)
                        .clamp(0, i64::from(zoom_tile_maximum));
                    neighborhood_tiles.push(
                        TileKey::new(tile_x as u32, tile_y as u32, anchor_tile.zoom_level).unwrap(),
                    );
                }
            }
            let mut payload_pool = TileTexturePool::new(1 << 20);
            for tile_key in &neighborhood_tiles {
                payload_pool.insert(*tile_key, vec![1, 2, 3]);
            }
            let prepared_tiles = neighborhood_tiles
                .iter()
                .map(|tile_key| PreparedTile {
                    requested_tile_key: *tile_key,
                    readiness: TileReadiness::Present,
                })
                .collect::<Vec<PreparedTile>>();
            let quads = renderer.frame_quads(&camera, &viewport, &prepared_tiles, &payload_pool);
            for tile_key in &neighborhood_tiles {
                let span = TileWorldSpan::covering_tile(*tile_key);
                let mut has_inside_viewport_sample = false;
                for sample_column in 0..CULL_BAND_SAMPLES_PER_TILE_EDGE {
                    for sample_row in 0..CULL_BAND_SAMPLES_PER_TILE_EDGE {
                        let sample_x = f64::from(sample_column as u32 + 1)
                            / f64::from(CULL_BAND_SAMPLES_PER_TILE_EDGE as u32 + 1);
                        let sample_y = f64::from(sample_row as u32 + 1)
                            / f64::from(CULL_BAND_SAMPLES_PER_TILE_EDGE as u32 + 1);
                        let sample_world = [
                            span.minimum_x + sample_x * span.width(),
                            span.minimum_y + sample_y * span.height(),
                        ];
                        let sample_ground = normalized_world_to_coordinate(sample_world);
                        if let Some(screen_position) = camera.project_to_screen(
                            &sample_ground,
                            &viewport,
                            CULL_BAND_FIELD_OF_VIEW_DEGREES,
                        ) && screen_position[0] >= 0.0
                            && screen_position[0] <= viewport.width_pixels
                            && screen_position[1] >= 0.0
                            && screen_position[1] <= viewport.height_pixels
                        {
                            has_inside_viewport_sample = true;
                        }
                    }
                }
                if has_inside_viewport_sample {
                    assert!(
                        quads.iter().any(|quad| {
                            quad.texture_tile_key == *tile_key && quad.world_span == span
                        }),
                        "pitch {pitch_degrees}: tile {tile_key:?} projects sample ground \
                         points inside the viewport but assembly culled it"
                    );
                }
            }
        }
    }
    #[test]
    fn a_retained_level_draws_its_pooled_tiles_at_the_requested_opacity() {
        let tile = tile_key(0, 0, 1);
        let mut payload_pool = TileTexturePool::new(1 << 20);
        payload_pool.insert(tile, vec![1, 2, 3]);
        let renderer = Renderer::default();
        let quads = renderer.retained_level_quads(
            &quadrant_camera(),
            &square_viewport(),
            1,
            &payload_pool,
            0.4,
        );
        assert_eq!(quads.len(), 1);
        assert_eq!(quads[0].texture_tile_key, tile);
        assert!((quads[0].opacity - 0.4).abs() < f32::EPSILON);

        let fragments = renderer.quad_vertices(&quadrant_camera(), &square_viewport(), &quads);
        assert!(!fragments.is_empty());
        assert!(
            fragments
                .iter()
                .flatten()
                .all(|vertex| (vertex.opacity - 0.4).abs() < f32::EPSILON),
            "every corner of a retained quad carries its opacity"
        );
    }

    #[test]
    fn a_retained_level_skips_keys_the_pool_no_longer_holds() {
        let pooled_tile = tile_key(0, 0, 1);
        let missing_tile = tile_key(1, 0, 1);
        let mut payload_pool = TileTexturePool::new(1 << 20);
        payload_pool.insert(pooled_tile, vec![1]);
        let renderer = Renderer::default();
        // Centered between the two quadrants, so both are visible and only
        // the pooled one can draw.
        let quads = renderer.retained_level_quads(
            &camera_above([0.5, 0.25], 1.0),
            &square_viewport(),
            1,
            &payload_pool,
            1.0,
        );
        assert!(!quads.is_empty());
        assert!(
            quads.iter().all(|quad| quad.texture_tile_key == pooled_tile
                && quad.texture_tile_key != missing_tile),
            "only the pooled tile draws for a retired level"
        );
    }
}
