//! The presented frame draw record and the vertex and snapshot encoders
//! shared by the live pass and the offscreen snapshot.

use super::*;

/// One tile fragment's draw call: its bound tile texture and the index range
/// it covers in the frame's shared vertex and index buffers.
pub(super) struct TileFragmentDraw {
    pub(super) bind_group: BindGroup,
    pub(super) first_index: u32,
}

/// The label layer's two glyph passes, kept together so the snapshot replay
/// draws the same halo-then-text order the presented frame used.
pub(super) struct GlyphLabelDraw {
    pub(super) bind_group: BindGroup,
    pub(super) halo: Option<(Buffer, u32)>,
    pub(super) text: Option<(Buffer, u32)>,
}

/// The complete draw list of one presented frame.
///
/// The buffers and bind groups are handle clones, so the record outlives the
/// frame's local variables while the GPU objects they name stay alive. The
/// presenting pass and [`BrowserMapView::snapshot`]'s offscreen replay both encode
/// this same record through [`encode_frame_draw_calls`], which keeps the
/// captured pixels and the presented pixels on one code path.
pub(super) struct FrameDrawRecord {
    /// The pass's clear color, each channel on the 0..=1 scale.
    pub(super) clear_color: [f64; 4],
    /// The raster tile layer's shared vertex and index buffers.
    pub(super) tile_vertex_buffer: Option<Buffer>,
    pub(super) tile_index_buffer: Option<Buffer>,
    /// One entry per drawn tile fragment, in submission order.
    pub(super) tile_fragments: Vec<TileFragmentDraw>,
    /// The cached world-space overlay layer: its vertex buffer and count.
    pub(super) world_space_overlay: Option<(Buffer, u32)>,
    /// The camera-space overlay layer: its vertex buffer and count.
    pub(super) camera_space_overlay: Option<(Buffer, u32)>,
    /// The sprite icon pass: its atlas bind group, vertex and index buffers,
    /// and index count.
    pub(super) sprite_icons: Option<(BindGroup, Buffer, Buffer, u32)>,
    /// The route-reference badge pass: its vertex buffer and count, drawn as
    /// solid triangles beneath the labels' glyphs.
    pub(super) label_badge: Option<(Buffer, u32)>,
    /// The label layer's halo and text passes.
    pub(super) glyph_labels: Option<GlyphLabelDraw>,
    /// The location-indicator layer's vertex buffer and count.
    pub(super) location_indicator: Option<(Buffer, u32)>,
}

/// Records every draw call in `draw_record` into an already-open render pass.
///
/// [`BrowserMapView::render_frame`] and [`BrowserMapView::snapshot`] share this function so
/// the offscreen snapshot can never drift from what the surface presents.
pub(super) fn encode_frame_draw_calls(
    render_pass: &mut wgpu::RenderPass<'_>,
    draw_state: &CanvasDrawState,
    draw_record: &FrameDrawRecord,
) {
    if !draw_record.tile_fragments.is_empty() {
        render_pass.set_pipeline(&draw_state.tile_pipeline);
        render_pass.set_vertex_buffer(
            0,
            draw_record
                .tile_vertex_buffer
                .as_ref()
                .expect("a fragment list was built from the tile vertex buffer")
                .slice(..),
        );
        render_pass.set_index_buffer(
            draw_record
                .tile_index_buffer
                .as_ref()
                .expect("a fragment list was built from the tile index buffer")
                .slice(..),
            wgpu::IndexFormat::Uint16,
        );
        for fragment in &draw_record.tile_fragments {
            render_pass.set_bind_group(0, &fragment.bind_group, &[]);
            render_pass.draw_indexed(
                fragment.first_index..(fragment.first_index + INDICES_PER_QUAD as u32),
                0,
                0..1,
            );
        }
    }
    if let Some((overlay_vertex_buffer, vertex_count)) = &draw_record.world_space_overlay {
        render_pass.set_pipeline(&draw_state.world_space_overlay_pipeline.pipeline);
        render_pass.set_bind_group(
            0,
            &draw_state
                .world_space_overlay_pipeline
                .frame_uniform_bind_group,
            &[],
        );
        render_pass.set_vertex_buffer(0, overlay_vertex_buffer.slice(..));
        // One instanced draw, one instance per world copy the shader's
        // fragment window can select.
        render_pass.draw(
            0..*vertex_count,
            0..WORLD_SPACE_OVERLAY_WORLD_COPY_INSTANCES,
        );
    }
    if let Some((overlay_vertex_buffer, vertex_count)) = &draw_record.camera_space_overlay {
        render_pass.set_pipeline(&draw_state.overlay_pipeline);
        render_pass.set_vertex_buffer(0, overlay_vertex_buffer.slice(..));
        render_pass.draw(0..*vertex_count, 0..1);
    }
    if let Some((badge_vertex_buffer, vertex_count)) = &draw_record.label_badge {
        render_pass.set_pipeline(&draw_state.overlay_pipeline);
        render_pass.set_vertex_buffer(0, badge_vertex_buffer.slice(..));
        render_pass.draw(0..*vertex_count, 0..1);
    }
    if let Some((bind_group, vertex_buffer, index_buffer, index_count)) = &draw_record.sprite_icons
    {
        render_pass.set_pipeline(&draw_state.tile_pipeline);
        render_pass.set_bind_group(0, bind_group, &[]);
        render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        render_pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint16);
        render_pass.draw_indexed(0..*index_count, 0, 0..1);
    }
    if let Some(glyph_labels) = &draw_record.glyph_labels {
        render_pass.set_pipeline(&draw_state.glyph_pipeline);
        render_pass.set_bind_group(0, &glyph_labels.bind_group, &[]);
        if let Some((halo_vertex_buffer, vertex_count)) = &glyph_labels.halo {
            render_pass.set_vertex_buffer(0, halo_vertex_buffer.slice(..));
            render_pass.draw(0..*vertex_count, 0..1);
        }
        if let Some((text_vertex_buffer, vertex_count)) = &glyph_labels.text {
            render_pass.set_vertex_buffer(0, text_vertex_buffer.slice(..));
            render_pass.draw(0..*vertex_count, 0..1);
        }
    }
    if let Some((indicator_vertex_buffer, vertex_count)) = &draw_record.location_indicator {
        render_pass.set_pipeline(&draw_state.overlay_pipeline);
        render_pass.set_vertex_buffer(0, indicator_vertex_buffer.slice(..));
        render_pass.draw(0..*vertex_count, 0..1);
    }
}

/// The row stride a texture-to-buffer copy must use for one pixel width: the
/// unpadded RGBA row rounded up to WebGPU's [`wgpu::COPY_BYTES_PER_ROW_ALIGNMENT`].
pub(super) fn aligned_copy_bytes_per_row(width_pixels: u32) -> u32 {
    (width_pixels * PIXEL_BYTES).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
        * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT
}

/// Rewrites a snapshot readback into the RGBA channel order the browser
/// surface exposes: the pass-through surface format is BGRA on most GPUs, so
/// its red and blue channels swap here. The readback buffer of an RGBA
/// surface is already in the answered order.
pub(super) fn normalized_snapshot_rgba_bytes(
    source_format: TextureFormat,
    mut bytes: Vec<u8>,
) -> Vec<u8> {
    if source_format == TextureFormat::Bgra8Unorm {
        for pixel in bytes.as_chunks_mut::<{ PIXEL_BYTES as usize }>().0 {
            pixel.swap(0, 2);
        }
    }
    bytes
}

/// The browser's monotonic clock in milliseconds, used for the frame's stage
/// spans. `performance.now()` is high-resolution and monotonic (unlike
/// `Date.now()`), which is what a frame budget needs; a page without a
/// `performance` object reports `NaN` rather than a fabricated zero, so a
/// missing measurement is visibly missing.
pub(super) fn browser_now_milliseconds() -> f64 {
    web_sys::window()
        .and_then(|browser_window| browser_window.performance())
        .map(|performance| performance.now())
        .unwrap_or(f64::NAN)
}
