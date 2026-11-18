//! Headless `wgpu` rendering of raster tiles plus deterministic frame
//! capture.
//!
//! The renderer draws one textured quad per pooled tile for a single frame
//! into an off-screen render-attachment texture and reads the pixels back
//! into a [`CapturedFrame`]; an optional overlay layer
//! ([`crate::overlay::MapOverlay`]) draws after the quads in the same render
//! pass through a separate solid-color pipeline. Pooled payloads that fail to
//! decode are skipped for the frame and evicted from the pool so the
//! scheduler refetches them: one corrupt tile never poisons the frames after
//! it. Only structural failures (an overflowing draw list, a failed context
//! acquisition or capture) are errors. No window surface is requested, so
//! this works on any native backend (Metal, Vulkan, DirectX). Device
//! acquisition lives behind the object-safe [`GraphicsContextSource`] trait,
//! which keeps `wgpu` types out of the scheduling core and lets a caller
//! substitute its own context source or skip rendering when no adapter
//! exists.
//!
//! This module is native-only: it blocks on the asynchronous `wgpu`
//! initialization calls and polls the device, which is not valid on
//! WebAssembly, where the browser's own async runtime drives WebGPU.
//!
//! # Capturing snapshots
//!
//! A capture composes the scene into an off-screen render attachment that is
//! never presented to a window, and [`capture_frame_bytes`] reads the result
//! back. The readback is inherently blocking ([`capture_frame_bytes`]
//! escalates to a blocking device poll).
//!
//! For deterministic captures, [`HeadlessTileRenderer::snapshot_frame_when_idle`]
//! refuses to render while pending tile work remains, returning
//! [`HeadlessRenderError::PendingTileWork`] immediately without touching the
//! GPU; otherwise it behaves exactly like the ordinary capture. The refusal
//! decision costs no device work and never waits for pending work to finish.
//! Completing that work is the caller's next prepare frame.
//! [`HeadlessTileRenderer::render_captured_frame_with_overlays`] is the
//! ungated form: it always renders and captures, however much tile work is
//! still pending.
//!
//! The idle gate covers tile fetch state only; camera animations are
//! composed outside this renderer entirely (see [`crate::tile_scheduler`] on
//! the full idle semantics).

use crate::camera::{MapCamera, ViewportPixels};
use crate::gpu_texture_cache::{GPU_TILE_TEXTURE_CACHE_DEFAULT_MAXIMUM_BYTES, GpuTileTextureCache};
use crate::labels::{ClipSpaceGlyphVertex, LabelFont, LabelLayer};
use crate::overlay::{ClipSpaceOverlayVertex, MapOverlay, overlay_triangle_vertices};
use crate::renderer::{CLEAR_COLOR_RGBA, ClipSpaceTileVertex, Renderer};
use crate::tile_key::TileKey;
use crate::tile_scheduler::PreparedTile;
use crate::tile_texture_pool::TileTexturePool;
use image::DynamicImage;
use std::sync::mpsc::{self, TryRecvError};
use wgpu::{
    AdapterInfo, Buffer, CommandEncoder, Device, Extent3d, MapMode, PollType, Queue,
    TexelCopyBufferInfo, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture, TextureAspect,
    TextureUsages,
};

/// GPU format of both the tile textures and the captured frame. The `image`
/// crate decodes tile payloads to plain RGBA8, so the pipeline applies no
/// sRGB conversion and colors round-trip byte-for-byte.
pub const FRAME_TEXTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Bytes in one decoded RGBA8 tile pixel.
const PIXEL_BYTES: u32 = 4;

/// Filter mode of the tile sampler. Linear interpolation gives smooth
/// minification for both the pitched camera and fallback draws scaled up.
pub const TILE_TEXTURE_FILTER: wgpu::FilterMode = wgpu::FilterMode::Linear;

/// Interpolation between mip levels. The uploaded textures carry no mip
/// chain, so the mode never applies; linear keeps the sampler consistent
/// should mip chains be added.
pub const TILE_TEXTURE_MIPMAP_FILTER: wgpu::MipmapFilterMode = wgpu::MipmapFilterMode::Linear;

/// Clamp sampler addressing: quads sample exactly their declared sub-window
/// of the texture, so edge behavior never triggers.
pub const TILE_TEXTURE_ADDRESS_MODE: wgpu::AddressMode = wgpu::AddressMode::ClampToEdge;

/// Draw quads a frame may carry, where a world-copy fragment of a tile
/// counts as one draw quad: the bound keeps both the fragment vertices (four
/// corners each) and the draw list inside a 16-bit index buffer.
pub const MAXIMUM_QUADS_PER_FRAME: usize = u16::MAX as usize / CORNERS_PER_QUAD;

/// Indices consumed by one drawn quad: the two triangles of the corner
/// pattern `0, 1, 2, 2, 1, 3`.
const INDICES_PER_QUAD: usize = 6;

/// Corners of one drawn quad: the four vertices one world-copy fragment
/// contributes.
const CORNERS_PER_QUAD: usize = 4;

/// WGSL pipeline: a single textured-quad shader. Vertex positions arrive
/// already in clip space (the camera transform runs on the CPU in `f64`; see
/// [`crate::renderer::Renderer::tile_vertices_in_clip_space`]), so the vertex
/// stage is a pass-through and the fragment stage samples the tile texture.
const TILE_QUAD_SHADER_SOURCE: &str = "
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) texture_coordinates: vec2<f32>,
    @location(1) opacity: f32,
};

@group(0)
@binding(0)
var tile_texture: texture_2d<f32>;

@group(0)
@binding(1)
var tile_sampler: sampler;

@vertex
fn vertex_main(@location(0) clip_position: vec4<f32>, @location(1) texture_coordinates: vec2<f32>, @location(2) opacity: f32) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = clip_position;
    output.texture_coordinates = texture_coordinates;
    output.opacity = opacity;
    return output;
}

@fragment
fn fragment_main(vertex: VertexOutput) -> @location(0) vec4<f32> {
    let sampled = textureSample(tile_texture, tile_sampler, vertex.texture_coordinates);
    return vec4<f32>(sampled.rgb, sampled.a * vertex.opacity);
}
";

/// WGSL pipeline for the overlay layer: solid-color triangles in clip space,
/// drawn after the tile quads in the same render pass through standard alpha
/// blending (translucent overlay colors composite over the base tiles).
/// Vertex positions arrive already in clip space — the camera transform runs
/// on the CPU in `f64` (see [`crate::overlay::overlay_triangle_vertices`] and
/// the documented wasm canvas mirror: same vertex layout and shader text
/// there) — so the vertex stage is a pass-through and the fragment stage
/// writes the interpolated vertex color.
const OVERLAY_SHADER_SOURCE: &str = "
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) rgba_color: vec4<f32>,
};

@vertex
fn vertex_main(@location(0) clip_position: vec4<f32>, @location(1) rgba_color: vec4<f32>) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = clip_position;
    output.rgba_color = rgba_color;
    return output;
}

@fragment
fn fragment_main(vertex: VertexOutput) -> @location(0) vec4<f32> {
    return vertex.rgba_color;
}
";

/// GPU format of the glyph atlas: one signed-distance byte per pixel, which
/// the label shader thresholds into text coverage.
pub const GLYPH_ATLAS_TEXTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

/// Bytes in one `R8Unorm` atlas pixel.
pub(crate) const GLYPH_ATLAS_PIXEL_BYTES: u32 = 1;

/// Filter mode of the glyph atlas sampler: linear interpolation keeps the
/// signed-distance contour smooth when the style's text size differs from the
/// atlas' 24-pixel em.
pub const GLYPH_ATLAS_TEXTURE_FILTER: wgpu::FilterMode = wgpu::FilterMode::Linear;

/// WGSL pipeline of the textured glyph quads, shared by the halo and text
/// passes: vertex positions arrive in clip space, and the fragment stage
/// thresholds the atlas' signed distance into an anti-aliased coverage value
/// tinted by the vertex color.
pub(crate) const GLYPH_ATLAS_SHADER_SOURCE: &str = "
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) texture_coordinates: vec2<f32>,
    @location(1) rgba_color: vec4<f32>,
    @location(2) signed_distance_threshold: f32,
    @location(3) signed_distance_gamma: f32,
};

@group(0)
@binding(0)
var glyph_atlas: texture_2d<f32>;

@group(0)
@binding(1)
var glyph_sampler: sampler;

@vertex
fn vertex_main(
    @location(0) clip_position: vec4<f32>,
    @location(1) texture_coordinates: vec2<f32>,
    @location(2) rgba_color: vec4<f32>,
    @location(3) signed_distance_threshold: f32,
    @location(4) signed_distance_gamma: f32,
) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = clip_position;
    output.texture_coordinates = texture_coordinates;
    output.rgba_color = rgba_color;
    output.signed_distance_threshold = signed_distance_threshold;
    output.signed_distance_gamma = signed_distance_gamma;
    return output;
}

@fragment
fn fragment_main(vertex: VertexOutput) -> @location(0) vec4<f32> {
    let signed_distance = textureSample(glyph_atlas, glyph_sampler, vertex.texture_coordinates).r;
    let coverage = smoothstep(
        vertex.signed_distance_threshold - vertex.signed_distance_gamma,
        vertex.signed_distance_threshold + vertex.signed_distance_gamma,
        signed_distance);
    return vec4<f32>(vertex.rgba_color.rgb, vertex.rgba_color.a * coverage);
}
";

/// One uploaded glyph atlas texture, stamped with the font's content
/// revision so the atlas is uploaded once per distinct font rather than per
/// frame.
struct GlyphAtlasTextureCacheEntry {
    revision: u64,
    /// The bind group owns the texture view, so keeping it is what keeps the
    /// uploaded atlas alive.
    bind_group: wgpu::BindGroup,
}

/// One uploaded sprite atlas texture, stamped with the atlas' content
/// revision so the sheet is uploaded once per distinct sprite rather than per
/// frame.
struct SpriteAtlasTextureCacheEntry {
    revision: u64,
    /// The bind group owns the texture view, so keeping it is what keeps the
    /// uploaded sheet alive.
    bind_group: wgpu::BindGroup,
}

/// An adapter/device/queue triple acquired for headless rendering.
#[derive(Debug)]
pub struct GraphicsContext {
    /// Logical device the renderer creates resources on.
    pub device: Device,
    /// Queue the renderer submits command buffers through.
    pub queue: Queue,
    /// Adapter description, naming the backend that renders the frames.
    pub adapter_information: AdapterInfo,
}

/// Why no graphics context could be acquired.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphicsContextError {
    /// No rendering adapter matched the request (for example on a machine
    /// without a usable GPU backend).
    AdapterUnavailable(String),
    /// An adapter was found but the device request failed.
    DeviceRequestFailed(String),
}

impl std::fmt::Display for GraphicsContextError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GraphicsContextError::AdapterUnavailable(detail) => {
                write!(formatter, "graphics adapter unavailable: {detail}")
            }
            GraphicsContextError::DeviceRequestFailed(detail) => {
                write!(formatter, "graphics device request failed: {detail}")
            }
        }
    }
}

/// Source of the graphics device used for headless rendering.
///
/// Implement this trait to supply a device acquired elsewhere (a pinned
/// backend, a scripted renderer, or a context shared with another renderer)
/// instead of relying on environment adapter discovery. Tests use it to
/// substitute their own device source.
pub trait GraphicsContextSource: Send + Sync {
    /// Acquires a device/queue pair for headless rendering, or explains why
    /// none was available.
    fn request_graphics_context(&self) -> Result<GraphicsContext, GraphicsContextError>;
}

/// The production source: requests the first available adapter with no
/// compatible surface, blocking on it with `pollster` (native only; see the
/// module documentation).
pub struct WgpuGraphicsContextSource;

impl GraphicsContextSource for WgpuGraphicsContextSource {
    fn request_graphics_context(&self) -> Result<GraphicsContext, GraphicsContextError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: None,
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .map_err(|error| GraphicsContextError::AdapterUnavailable(error.to_string()))?;

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("rustic-maps-rendering-headless"),
            required_features: wgpu::Features::empty(),
            // See `surface.rs` for why the WebGL2 downlevel defaults are
            // requested rather than `Limits::default()`: adapter caps differ
            // (Metal's inter-stage varyings, SwiftShader's zero compute
            // workgroup limit) and the renderer needs neither compute nor
            // more varyings than these limits allow.
            required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        }))
        .map_err(|error| GraphicsContextError::DeviceRequestFailed(error.to_string()))?;

        Ok(GraphicsContext {
            device,
            queue,
            adapter_information: adapter.get_info(),
        })
    }
}

/// Why one frame could not be rendered or captured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeadlessRenderError {
    /// The graphics context could not be acquired at all: either no adapter
    /// matched ([`GraphicsContextError::AdapterUnavailable`]) or the device
    /// request failed. Kept distinguishable from a per-frame capture failure
    /// so callers can tell "no GPU here" from "this frame broke".
    GraphicsContextUnavailable(GraphicsContextError),
    /// The draw list would overflow the 16-bit index buffer.
    DrawListTooLarge {
        /// Draw quads (world-copy fragments) assembled for the frame.
        quad_count: usize,
        /// Draw quads the index buffer supports.
        maximum_quad_count: usize,
    },
    /// The rendered frame could not be captured back to the CPU.
    FrameCapture(String),
    /// The idle-gated snapshot refused to render: pending tile work remains,
    /// so the frame's pixels could still change through scheduled work.
    /// Reports how many requested tiles are not pooled at their own zoom
    /// level — verdicts other than `Present`, or `Present` verdicts whose
    /// payload left the pool since. Nothing was rendered; retrying after the
    /// scheduler's next prepare frame resolves the pending tiles.
    PendingTileWork {
        /// Requested tiles still lacking their own pooled payload.
        pending_tile_count: usize,
    },
}

impl From<GraphicsContextError> for HeadlessRenderError {
    fn from(error: GraphicsContextError) -> Self {
        HeadlessRenderError::GraphicsContextUnavailable(error)
    }
}

/// One rendered frame copied back from the GPU: tightly packed RGBA8,
/// row-major, origin top-left, no row padding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedFrame {
    /// Frame width in pixels.
    pub width_pixels: u32,
    /// Frame height in pixels.
    pub height_pixels: u32,
    /// `width * height * 4` RGBA bytes, rows tightly packed.
    pub rgba_bytes: Vec<u8>,
}

/// Outcome of the staging buffer's map callback, handed to the frame reader
/// over a channel to bridge the `wgpu` callback into synchronous code.
type BufferMapOutcome = Result<(), String>;

/// The row stride granularity texture-to-buffer copies require: every row of
/// a copied texture starts on a 256-byte boundary
/// ([`wgpu::COPY_BYTES_PER_ROW_ALIGNMENT`]), so partial rows round up to it.
const ROW_ALIGNMENT_BYTES: usize = 256;

/// Bytes per buffer row after the mandatory copy alignment above: each row of
/// a texture-to-buffer copy starts on a 256-byte boundary, so partial rows
/// round up.
fn aligned_bytes_per_row(width_pixels: u32) -> usize {
    let unpadded_row_bytes = width_pixels as usize * PIXEL_BYTES as usize;
    unpadded_row_bytes.div_ceil(ROW_ALIGNMENT_BYTES) * ROW_ALIGNMENT_BYTES
}

/// Renders raster tiles headlessly. Owns the pipeline, sampler, and the
/// uploaded-texture cache; the graphics context is injected, so production
/// code and tests can supply any backed device through
/// [`GraphicsContextSource`].
pub struct HeadlessTileRenderer {
    graphics_context: GraphicsContext,
    tile_pipeline: wgpu::RenderPipeline,
    /// Solid-color pipeline of the overlay layer, drawn after the tile
    /// pipeline's quads in the same render pass.
    overlay_pipeline: wgpu::RenderPipeline,
    /// Textured glyph pipeline of the label layer's halo and text passes,
    /// drawn after the overlays and before the location indicator.
    glyph_pipeline: wgpu::RenderPipeline,
    glyph_sampler: wgpu::Sampler,
    /// The uploaded `R8Unorm` glyph atlas, reused across frames until the
    /// font's content revision changes.
    glyph_atlas_texture: Option<GlyphAtlasTextureCacheEntry>,
    /// The uploaded RGBA sprite atlas the icon pass samples, reused across
    /// frames until the atlas' content revision changes.
    sprite_atlas_texture: Option<SpriteAtlasTextureCacheEntry>,
    tile_sampler: wgpu::Sampler,
    quad_composer: Renderer,
    /// Uploads still valid for reuse across frames; see
    /// [`GpuTileTextureCache`] for the reuse and staleness model.
    gpu_texture_cache: GpuTileTextureCache,
    /// The colour the captured frame clears to. Defaults to the renderer's
    /// neutral clear; a styled base sets it to the style's canvas colour so a
    /// headless capture matches what a surface presents.
    clear_color_rgba: [f32; 4],
}

impl HeadlessTileRenderer {
    /// Sets the colour the captured frame clears to.
    pub fn set_clear_color_rgba(&mut self, rgba_color: [f32; 4]) {
        self.clear_color_rgba = rgba_color;
    }

    /// Acquires a graphics context through `source` and builds the tile
    /// pipeline on it.
    ///
    /// # Errors
    /// Returns [`HeadlessRenderError`] when no graphics device is available or
    /// when the pipeline objects cannot be created.
    pub fn new(source: &dyn GraphicsContextSource) -> Result<Self, HeadlessRenderError> {
        let graphics_context = source.request_graphics_context()?;
        Self::with_graphics_context(graphics_context)
    }

    /// Builds the tile pipeline on an already-acquired graphics context, so a
    /// caller can inject a device created elsewhere.
    pub fn with_graphics_context(
        graphics_context: GraphicsContext,
    ) -> Result<Self, HeadlessRenderError> {
        let shader_module =
            graphics_context
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("rustic-maps-rendering-tile-quad-shader"),
                    source: wgpu::ShaderSource::Wgsl(TILE_QUAD_SHADER_SOURCE.into()),
                });

        let bind_group_layout =
            graphics_context
                .device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("rustic-maps-rendering-tile-bind-group-layout"),
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Texture {
                                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                                view_dimension: wgpu::TextureViewDimension::D2,
                                multisampled: false,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                            count: None,
                        },
                    ],
                });

        let vertex_buffer_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ClipSpaceTileVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x4,
                    offset: 0,
                    shader_location: 0,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: std::mem::offset_of!(ClipSpaceTileVertex, texture_coordinates)
                        as wgpu::BufferAddress,
                    shader_location: 1,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32,
                    offset: std::mem::offset_of!(ClipSpaceTileVertex, opacity)
                        as wgpu::BufferAddress,
                    shader_location: 2,
                },
            ],
        };

        let pipeline_layout =
            graphics_context
                .device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("rustic-maps-rendering-tile-pipeline-layout"),
                    bind_group_layouts: &[Some(&bind_group_layout)],
                    immediate_size: 0,
                });

        let tile_pipeline =
            graphics_context
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("rustic-maps-rendering-tile-pipeline"),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &shader_module,
                        entry_point: Some("vertex_main"),
                        compilation_options: Default::default(),
                        buffers: &[Some(vertex_buffer_layout)],
                    },
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleList,
                        cull_mode: None,
                        ..Default::default()
                    },
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &shader_module,
                        entry_point: Some("fragment_main"),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: FRAME_TEXTURE_FORMAT,
                            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    multiview_mask: None,
                    cache: None,
                });

        let tile_sampler = graphics_context
            .device
            .create_sampler(&wgpu::SamplerDescriptor {
                label: Some("rustic-maps-rendering-tile-sampler"),
                address_mode_u: TILE_TEXTURE_ADDRESS_MODE,
                address_mode_v: TILE_TEXTURE_ADDRESS_MODE,
                address_mode_w: TILE_TEXTURE_ADDRESS_MODE,
                mag_filter: TILE_TEXTURE_FILTER,
                min_filter: TILE_TEXTURE_FILTER,
                mipmap_filter: TILE_TEXTURE_MIPMAP_FILTER,
                ..Default::default()
            });

        let overlay_shader_module =
            graphics_context
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("rustic-maps-rendering-overlay-shader"),
                    source: wgpu::ShaderSource::Wgsl(OVERLAY_SHADER_SOURCE.into()),
                });

        let overlay_vertex_buffer_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ClipSpaceOverlayVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x4,
                    offset: 0,
                    shader_location: 0,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x4,
                    offset: std::mem::offset_of!(ClipSpaceOverlayVertex, rgba_color)
                        as wgpu::BufferAddress,
                    shader_location: 1,
                },
            ],
        };

        let overlay_pipeline_layout =
            graphics_context
                .device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("rustic-maps-rendering-overlay-pipeline-layout"),
                    bind_group_layouts: &[],
                    immediate_size: 0,
                });

        let overlay_pipeline =
            graphics_context
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("rustic-maps-rendering-overlay-pipeline"),
                    layout: Some(&overlay_pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &overlay_shader_module,
                        entry_point: Some("vertex_main"),
                        compilation_options: Default::default(),
                        buffers: &[Some(overlay_vertex_buffer_layout)],
                    },
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleList,
                        cull_mode: None,
                        ..Default::default()
                    },
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &overlay_shader_module,
                        entry_point: Some("fragment_main"),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: FRAME_TEXTURE_FORMAT,
                            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    multiview_mask: None,
                    cache: None,
                });

        let glyph_shader_module =
            graphics_context
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("rustic-maps-rendering-glyph-atlas-shader"),
                    source: wgpu::ShaderSource::Wgsl(GLYPH_ATLAS_SHADER_SOURCE.into()),
                });

        let glyph_bind_group_layout =
            graphics_context
                .device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("rustic-maps-rendering-glyph-atlas-bind-group-layout"),
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Texture {
                                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                                view_dimension: wgpu::TextureViewDimension::D2,
                                multisampled: false,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                            count: None,
                        },
                    ],
                });

        let glyph_vertex_buffer_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ClipSpaceGlyphVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x4,
                    offset: 0,
                    shader_location: 0,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: std::mem::offset_of!(ClipSpaceGlyphVertex, texture_coordinates)
                        as wgpu::BufferAddress,
                    shader_location: 1,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x4,
                    offset: std::mem::offset_of!(ClipSpaceGlyphVertex, rgba_color)
                        as wgpu::BufferAddress,
                    shader_location: 2,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32,
                    offset: std::mem::offset_of!(ClipSpaceGlyphVertex, signed_distance_threshold)
                        as wgpu::BufferAddress,
                    shader_location: 3,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32,
                    offset: std::mem::offset_of!(ClipSpaceGlyphVertex, signed_distance_gamma)
                        as wgpu::BufferAddress,
                    shader_location: 4,
                },
            ],
        };

        let glyph_pipeline_layout =
            graphics_context
                .device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("rustic-maps-rendering-glyph-atlas-pipeline-layout"),
                    bind_group_layouts: &[Some(&glyph_bind_group_layout)],
                    immediate_size: 0,
                });

        let glyph_pipeline =
            graphics_context
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("rustic-maps-rendering-glyph-atlas-pipeline"),
                    layout: Some(&glyph_pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &glyph_shader_module,
                        entry_point: Some("vertex_main"),
                        compilation_options: Default::default(),
                        buffers: &[Some(glyph_vertex_buffer_layout)],
                    },
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleList,
                        cull_mode: None,
                        ..Default::default()
                    },
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &glyph_shader_module,
                        entry_point: Some("fragment_main"),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: FRAME_TEXTURE_FORMAT,
                            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    multiview_mask: None,
                    cache: None,
                });

        let glyph_sampler = graphics_context
            .device
            .create_sampler(&wgpu::SamplerDescriptor {
                label: Some("rustic-maps-rendering-glyph-atlas-sampler"),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: GLYPH_ATLAS_TEXTURE_FILTER,
                min_filter: GLYPH_ATLAS_TEXTURE_FILTER,
                mipmap_filter: wgpu::MipmapFilterMode::Nearest,
                ..Default::default()
            });

        Ok(Self {
            graphics_context,
            tile_pipeline,
            overlay_pipeline,
            glyph_pipeline,
            glyph_sampler,
            glyph_atlas_texture: None,
            sprite_atlas_texture: None,
            tile_sampler,
            quad_composer: Renderer::default(),
            gpu_texture_cache: GpuTileTextureCache::new(
                GPU_TILE_TEXTURE_CACHE_DEFAULT_MAXIMUM_BYTES,
            ),
            clear_color_rgba: CLEAR_COLOR_RGBA,
        })
    }

    /// Description of the adapter this renderer draws with.
    pub fn adapter_information(&self) -> &AdapterInfo {
        &self.graphics_context.adapter_information
    }

    /// The cache of uploaded tile textures, for diagnostics (uploads vs
    /// cache-hit counts, GPU bytes held).
    pub fn gpu_texture_cache(&self) -> &GpuTileTextureCache {
        &self.gpu_texture_cache
    }

    /// Mutable access to the cache, mostly to tune its byte budget for
    /// constrained devices before frames render.
    pub fn gpu_texture_cache_mut(&mut self) -> &mut GpuTileTextureCache {
        &mut self.gpu_texture_cache
    }

    /// Renders one frame of tiles and captures its pixels, with no overlays —
    /// byte-comparable to the pre-overlay pipeline by construction, since
    /// [`Self::render_captured_frame_with_overlays`] draws nothing for an
    /// empty overlay list.
    pub fn render_captured_frame(
        &mut self,
        camera: &MapCamera,
        viewport: &ViewportPixels,
        prepared_tiles: &[PreparedTile],
        pool: &mut TileTexturePool,
    ) -> Result<CapturedFrame, HeadlessRenderError> {
        self.render_captured_frame_with_overlays(camera, viewport, prepared_tiles, pool, &[])
    }

    /// The idle-gated snapshot: refuses to render while pending tile work
    /// remains, and otherwise captures exactly the ordinary frame (see the
    /// module documentation for the idle gate).
    ///
    /// The gate is the idle render-state signal applied to this frame's own
    /// request set at capture time, so it needs no scheduler around: a
    /// requested tile is pending when its prepared verdict is anything but
    /// [`crate::tile_scheduler::TileReadiness::Present`] — a parent fallback
    /// still owes the child's
    /// detail, an `Unavailable` fetch is retried later, a `Deferred` tile is
    /// fetched later — and also when a `Present` verdict's payload left the
    /// pool between the prepare frame and this snapshot, whose area would
    /// otherwise capture as a hole pixels the next fetch could fill. When
    /// the verdict is [`crate::prepared_frame_is_idle`] and every pooled
    /// payload is still in the pool, the frame's pixels can no longer change
    /// through scheduled work and the frame renders and captures — byte-for-
    /// byte the ordinary capture's behavior, so the two are intercomparable.
    ///
    /// The refusal is decided before any GPU work: an unresolved frame
    /// returns [`HeadlessRenderError::PendingTileWork`] immediately and
    /// never waits for the pending work to complete. An empty request set is
    /// idle by vacuity, so an empty-frame snapshot renders the clear color
    /// (as does the ordinary capture).
    pub fn snapshot_frame_when_idle(
        &mut self,
        camera: &MapCamera,
        viewport: &ViewportPixels,
        prepared_tiles: &[PreparedTile],
        pool: &mut TileTexturePool,
        overlays: &[MapOverlay],
    ) -> Result<CapturedFrame, HeadlessRenderError> {
        let pending_tile_count = idle_gate::pending_tile_count(prepared_tiles, pool);
        if pending_tile_count > 0 {
            return Err(HeadlessRenderError::PendingTileWork { pending_tile_count });
        }
        self.render_captured_frame_with_overlays(camera, viewport, prepared_tiles, pool, overlays)
    }

    /// Renders one frame — the pooled tiles' textured quads plus the overlay
    /// layer on top — and captures its pixels. The three-layer variant adds
    /// the location-indicator layer; this signature keeps the two-layer
    /// contract byte-stable by delegating with no indicator.
    pub fn render_captured_frame_with_overlays(
        &mut self,
        camera: &MapCamera,
        viewport: &ViewportPixels,
        prepared_tiles: &[PreparedTile],
        pool: &mut TileTexturePool,
        overlays: &[MapOverlay],
    ) -> Result<CapturedFrame, HeadlessRenderError> {
        self.render_captured_frame_layers(camera, viewport, prepared_tiles, pool, overlays, None)
    }

    /// The frame's explicit three-layer variant: the pooled tiles' textured
    /// quads, then the overlay layer, then the location-indicator layer
    /// (the indicator draws last, the frame's explicit top layer — see
    /// [`crate::location_indicator::LOCATION_INDICATOR_TOP_LAYER_DRAW_ORDER`]).
    ///
    /// The frame's quads are assembled GPU-free by
    /// [`crate::renderer::Renderer::frame_quads`]; each pooled payload is
    /// uploaded once — either freshly decoded, or reused from
    /// [`GpuTileTextureCache`] when the pool's payload revision still matches
    /// the upload's stamp — in prepared order, so later detailed tiles paint
    /// over any earlier fallback content they overlap. Uploads whose bytes
    /// the pool replaced or dropped since are pruned at frame start. Each
    /// quad may project into up to two world-copy fragments, which are
    /// separate draw quads sharing the quad's texture. A payload that fails
    /// to decode — or one that left the pool between assembly and upload —
    /// skips its quads and, when corrupt, is evicted from the pool so the
    /// scheduler refetches it; the rest of the frame renders normally.
    ///
    /// After the tile draws, the overlay layer — the clip-space solid-color
    /// triangles assembled GPU-free by [`crate::overlay::overlay_triangle_vertices`]
    /// through the same camera pose — draws through a second pipeline into
    /// the same render attachment, alpha-blended over the tiles. An empty
    /// overlay list contributes no draw at all, keeping such frames identical
    /// to the tiles-only pipeline's.
    ///
    /// The location-indicator layer draws third — the clip-space solid-color
    /// triangles assembled GPU-free by
    /// [`crate::location_indicator::location_indicator_vertices`] through the
    /// same camera pose, drawn through the same overlay pipeline
    /// (shader/layout-identical) and alpha-blended over what the first two
    /// layers drew. A `None` indicator creates no buffer and contributes no
    /// draw, keeping indicator-free frames identical to the two-layer
    /// pipeline's.
    pub fn render_captured_frame_layers(
        &mut self,
        camera: &MapCamera,
        viewport: &ViewportPixels,
        prepared_tiles: &[PreparedTile],
        pool: &mut TileTexturePool,
        overlays: &[MapOverlay],
        location_indicator_layer: Option<&crate::location_indicator::LocationIndicatorLayer>,
    ) -> Result<CapturedFrame, HeadlessRenderError> {
        self.render_captured_frame_layers_and_labels(
            camera,
            viewport,
            prepared_tiles,
            pool,
            overlays,
            location_indicator_layer,
            None,
        )
    }

    /// Renders one frame of tiles, overlays and the label layer — no location
    /// indicator — and captures its pixels: the label golden's entry point.
    /// A `None` or empty label layer contributes no buffer or draw, leaving
    /// the frame byte-identical to the unlabeled pipeline's.
    pub fn render_captured_frame_with_labels(
        &mut self,
        camera: &MapCamera,
        viewport: &ViewportPixels,
        prepared_tiles: &[PreparedTile],
        pool: &mut TileTexturePool,
        label_layer: Option<&LabelLayer>,
    ) -> Result<CapturedFrame, HeadlessRenderError> {
        self.render_captured_frame_layers_and_labels(
            camera,
            viewport,
            prepared_tiles,
            pool,
            &[],
            None,
            label_layer,
        )
    }

    /// The frame's full four-layer variant: pooled tile quads, overlays, the
    /// label layer's halo and text passes (drawn over the overlays through the
    /// textured glyph pipeline), and the location indicator on top. A
    /// cleared indicator and an absent or empty label layer both create no
    /// draw, so indicator-free and label-free frames stay byte-identical to
    /// the earlier pipelines' captures.
    ///
    /// The glyph atlas uploads once per distinct font: the uploaded page is
    /// stamped with the font's content revision and reused on every later
    /// frame that draws the same font.
    // One argument per frame layer keeps the draw order explicit at the call
    // site; the earlier three-layer variant stays as a delegating wrapper.
    #[allow(clippy::too_many_arguments)]
    pub fn render_captured_frame_layers_and_labels(
        &mut self,
        camera: &MapCamera,
        viewport: &ViewportPixels,
        prepared_tiles: &[PreparedTile],
        pool: &mut TileTexturePool,
        overlays: &[MapOverlay],
        location_indicator_layer: Option<&crate::location_indicator::LocationIndicatorLayer>,
        label_layer: Option<&LabelLayer>,
    ) -> Result<CapturedFrame, HeadlessRenderError> {
        self.render_captured_frame_layers_and_labels_with_retained_level(
            camera,
            viewport,
            prepared_tiles,
            pool,
            overlays,
            location_indicator_layer,
            label_layer,
            None,
        )
    }

    /// The frame's full variant with a retired tile zoom level drawn over the
    /// requested one: the zoom cross-fade's capture path, so a golden can
    /// show a mid-fade frame. The retained quads draw after the requested
    /// level's and carry their own opacity.
    #[allow(clippy::too_many_arguments)]
    pub fn render_captured_frame_layers_and_labels_with_retained_level(
        &mut self,
        camera: &MapCamera,
        viewport: &ViewportPixels,
        prepared_tiles: &[PreparedTile],
        pool: &mut TileTexturePool,
        overlays: &[MapOverlay],
        location_indicator_layer: Option<&crate::location_indicator::LocationIndicatorLayer>,
        label_layer: Option<&LabelLayer>,
        retained_tile_level: Option<crate::renderer::RetainedTileLevel>,
    ) -> Result<CapturedFrame, HeadlessRenderError> {
        let mut quads = self
            .quad_composer
            .frame_quads(camera, viewport, prepared_tiles, pool);
        if let Some(retained_tile_level) = retained_tile_level {
            quads.extend(self.quad_composer.retained_level_quads(
                camera,
                viewport,
                retained_tile_level.tile_zoom_level,
                pool,
                retained_tile_level.opacity,
            ));
        }

        // Drop uploads the pool invalidated — bytes replaced or evicted since
        // their texture was uploaded — before deciding what needs uploading.
        self.gpu_texture_cache
            .retain_matching(|tile_key, pool_revision| {
                pool.payload_revision(tile_key) == Some(pool_revision)
            });
        // Touch the recency of everything this frame draws before any upload,
        // so an upload's eviction pass below cannot remove an entry a later
        // quad of this same frame reuses.
        for quad in &quads {
            self.gpu_texture_cache.touch(&quad.texture_tile_key);
        }

        // The device and queue handles are cheap reference-counted clones;
        // borrowing them out of the context instead would pin the shared
        // borrow of `self` across the mutable upload path below.
        let device = self.graphics_context.device.clone();
        let queue = self.graphics_context.queue.clone();

        // One vertex array per world-copy fragment, each tagged with the quad
        // whose texture it draws.
        let quad_fragment_vertices = self
            .quad_composer
            .quad_fragment_vertices(camera, viewport, &quads);
        let mut fragment_vertices = Vec::new();
        let mut fragment_texture_positions = Vec::with_capacity(quad_fragment_vertices.len());
        for (quad_index, quad_fragments) in quad_fragment_vertices.iter().enumerate() {
            for quad_fragment in quad_fragments {
                fragment_texture_positions.push(quad_index);
                fragment_vertices.push(*quad_fragment);
            }
        }
        if fragment_vertices.len() > MAXIMUM_QUADS_PER_FRAME {
            return Err(HeadlessRenderError::DrawListTooLarge {
                quad_count: fragment_vertices.len(),
                maximum_quad_count: MAXIMUM_QUADS_PER_FRAME,
            });
        }

        // Upload once per quad; every world-copy fragment of the quad draws
        // with the same texture bind group. A quad whose payload is missing
        // or undecodable contributes no draw (see `upload_tile_texture`).
        let mut tile_textures_and_bind_groups = Vec::with_capacity(quads.len());
        for quad in &quads {
            tile_textures_and_bind_groups.push(self.upload_tile_texture(
                &device,
                quad.texture_tile_key,
                pool,
            )?);
        }

        let (vertex_bytes, index_bytes) = encoded_fragment_buffers(&fragment_vertices);

        // An all-missing frame has no draw list; its buffers would be empty,
        // which `wgpu` buffers cannot represent, so only buffers of a frame
        // that draws are created and the all-missing frame renders as the
        // clear color alone.
        let vertex_buffer = (!fragment_vertices.is_empty()).then(|| {
            create_filled_buffer(
                &device,
                &queue,
                &vertex_bytes,
                wgpu::BufferUsages::VERTEX,
                Some("rustic-maps-rendering-tile-vertices"),
            )
        });
        let index_buffer = (!fragment_vertices.is_empty()).then(|| {
            create_filled_buffer(
                &device,
                &queue,
                &index_bytes,
                wgpu::BufferUsages::INDEX,
                Some("rustic-maps-rendering-tile-indices"),
            )
        });

        // The overlay layer assembles through the same camera pose as the
        // tile quads and draws after them; an empty overlay list creates no
        // buffer and contributes no draw.
        let overlay_vertices = overlay_triangle_vertices(
            camera,
            viewport,
            self.quad_composer.field_of_view_degrees(),
            overlays,
        );
        let overlay_vertex_buffer = (!overlay_vertices.is_empty()).then(|| {
            create_filled_buffer(
                &device,
                &queue,
                &encoded_overlay_vertices(&overlay_vertices),
                wgpu::BufferUsages::VERTEX,
                Some("rustic-maps-rendering-overlay-vertices"),
            )
        });

        // The location-indicator layer assembles through the same camera pose
        // and draws through the same overlay pipeline after the overlays; a
        // cleared (None) indicator creates no buffer and draws nothing.
        let indicator_vertices = crate::location_indicator::location_indicator_vertices(
            camera,
            viewport,
            self.quad_composer.field_of_view_degrees(),
            location_indicator_layer,
        );
        let indicator_vertex_buffer = (!indicator_vertices.is_empty()).then(|| {
            create_filled_buffer(
                &device,
                &queue,
                &encoded_overlay_vertices(&indicator_vertices),
                wgpu::BufferUsages::VERTEX,
                Some("rustic-maps-rendering-indicator-vertices"),
            )
        });

        // The label layer assembles its halo and text quads GPU-free through
        // the same camera pose; an absent or empty layer creates no buffers
        // and no glyph atlas texture, so unlabeled frames stay byte-identical.
        let label_quad_vertices = label_layer
            .filter(|layer| !layer.is_empty())
            .map(|layer| layer.quad_vertices(viewport))
            .unwrap_or_default();
        let glyph_bind_group = match label_layer {
            Some(layer) if !label_quad_vertices.is_empty() => {
                Some(self.ensure_glyph_atlas_texture(&device, &queue, layer.font()))
            }
            _ => None,
        };
        let halo_vertex_buffer = (!label_quad_vertices.halo.is_empty()).then(|| {
            create_filled_buffer(
                &device,
                &queue,
                &encoded_glyph_vertices(&label_quad_vertices.halo),
                wgpu::BufferUsages::VERTEX,
                Some("rustic-maps-rendering-label-halo-vertices"),
            )
        });
        let text_vertex_buffer = (!label_quad_vertices.text.is_empty()).then(|| {
            create_filled_buffer(
                &device,
                &queue,
                &encoded_glyph_vertices(&label_quad_vertices.text),
                wgpu::BufferUsages::VERTEX,
                Some("rustic-maps-rendering-label-text-vertices"),
            )
        });
        // A route-reference badge draws as solid triangles beneath its own
        // text, through the same overlay pipeline the styled polygons use. An
        // absent badge creates no buffer and no draw, so unbadged frames stay
        // byte-identical.
        let badge_vertex_buffer = (!label_quad_vertices.badge.is_empty()).then(|| {
            create_filled_buffer(
                &device,
                &queue,
                &encoded_overlay_vertices(&label_quad_vertices.badge),
                wgpu::BufferUsages::VERTEX,
                Some("rustic-maps-rendering-label-badge-vertices"),
            )
        });

        // The icon pass assembles the placed icons' sprite quads GPU-free and
        // draws them through the tile textured pipeline with the sprite atlas
        // bound; an absent atlas or no placed icon creates no buffers and no
        // draw, so a text-only frame stays byte-identical.
        let icon_quad_vertices = label_layer
            .filter(|layer| !layer.is_empty())
            .map(|layer| layer.icon_quad_vertices(viewport))
            .unwrap_or_default();
        let sprite_atlas_bind_group = match label_layer {
            Some(layer) if !icon_quad_vertices.is_empty() => layer
                .sprite()
                .map(|atlas| self.ensure_sprite_atlas_texture(&device, &queue, atlas)),
            _ => None,
        };
        let (icon_vertex_bytes, icon_index_bytes) = icon_quad_vertices.encoded_buffers();
        let icon_vertex_buffer = (!icon_quad_vertices.is_empty()).then(|| {
            create_filled_buffer(
                &device,
                &queue,
                &icon_vertex_bytes,
                wgpu::BufferUsages::VERTEX,
                Some("rustic-maps-rendering-icon-vertices"),
            )
        });
        let icon_index_buffer = (!icon_quad_vertices.is_empty()).then(|| {
            create_filled_buffer(
                &device,
                &queue,
                &icon_index_bytes,
                wgpu::BufferUsages::INDEX,
                Some("rustic-maps-rendering-icon-indices"),
            )
        });

        let width_pixels = viewport.width_pixels as u32;
        let height_pixels = viewport.height_pixels as u32;
        let frame_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("rustic-maps-rendering-frame"),
            size: Extent3d {
                width: width_pixels,
                height: height_pixels,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FRAME_TEXTURE_FORMAT,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let frame_texture_view = frame_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut command_encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("rustic-maps-rendering-frame-encoder"),
        });
        {
            let mut render_pass = command_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("rustic-maps-rendering-frame-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &frame_texture_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(self.clear_color_rgba[0]),
                            g: f64::from(self.clear_color_rgba[1]),
                            b: f64::from(self.clear_color_rgba[2]),
                            a: f64::from(self.clear_color_rgba[3]),
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if !fragment_vertices.is_empty() {
                render_pass.set_pipeline(&self.tile_pipeline);
                render_pass.set_vertex_buffer(0, vertex_buffer.expect("frame draws").slice(..));
                render_pass.set_index_buffer(
                    index_buffer.expect("frame draws").slice(..),
                    wgpu::IndexFormat::Uint16,
                );
                for (fragment_index, texture_position) in
                    fragment_texture_positions.iter().copied().enumerate()
                {
                    let Some((_, bind_group)) = &tile_textures_and_bind_groups[texture_position]
                    else {
                        continue;
                    };
                    render_pass.set_bind_group(0, bind_group, &[]);
                    let first_index = u32::try_from(fragment_index * INDICES_PER_QUAD)
                        .expect("the draw list was bounded above");
                    render_pass.draw_indexed(
                        first_index..first_index + INDICES_PER_QUAD as u32,
                        0,
                        0..1,
                    );
                }
            }
            if let Some(overlay_vertex_buffer) = &overlay_vertex_buffer {
                render_pass.set_pipeline(&self.overlay_pipeline);
                render_pass.set_vertex_buffer(0, overlay_vertex_buffer.slice(..));
                let overlay_vertex_count = u32::try_from(overlay_vertices.len())
                    .expect("the overlay triangle list stays inside u32");
                render_pass.draw(0..overlay_vertex_count, 0..1);
            }
            if let Some(badge_vertex_buffer) = &badge_vertex_buffer {
                render_pass.set_pipeline(&self.overlay_pipeline);
                render_pass.set_vertex_buffer(0, badge_vertex_buffer.slice(..));
                let badge_vertex_count = u32::try_from(label_quad_vertices.badge.len())
                    .expect("the badge triangle list stays inside u32");
                render_pass.draw(0..badge_vertex_count, 0..1);
            }
            if let Some(sprite_atlas_bind_group) = &sprite_atlas_bind_group {
                render_pass.set_pipeline(&self.tile_pipeline);
                render_pass.set_bind_group(0, sprite_atlas_bind_group, &[]);
                if let (Some(icon_vertex_buffer), Some(icon_index_buffer)) =
                    (&icon_vertex_buffer, &icon_index_buffer)
                {
                    render_pass.set_vertex_buffer(0, icon_vertex_buffer.slice(..));
                    render_pass
                        .set_index_buffer(icon_index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                    let icon_index_count = u32::try_from(
                        icon_quad_vertices.quads.len() * crate::labels::ICON_INDICES_PER_QUAD,
                    )
                    .expect("the icon quad list stays inside the u32 draw range");
                    render_pass.draw_indexed(0..icon_index_count, 0, 0..1);
                }
            }
            if let Some(glyph_bind_group) = &glyph_bind_group {
                render_pass.set_pipeline(&self.glyph_pipeline);
                render_pass.set_bind_group(0, glyph_bind_group, &[]);
                if let Some(halo_vertex_buffer) = &halo_vertex_buffer {
                    render_pass.set_vertex_buffer(0, halo_vertex_buffer.slice(..));
                    let halo_vertex_count = u32::try_from(label_quad_vertices.halo.len())
                        .expect("the halo triangle list stays inside u32");
                    render_pass.draw(0..halo_vertex_count, 0..1);
                }
                if let Some(text_vertex_buffer) = &text_vertex_buffer {
                    render_pass.set_vertex_buffer(0, text_vertex_buffer.slice(..));
                    let text_vertex_count = u32::try_from(label_quad_vertices.text.len())
                        .expect("the label triangle list stays inside u32");
                    render_pass.draw(0..text_vertex_count, 0..1);
                }
            }
            if let Some(indicator_vertex_buffer) = &indicator_vertex_buffer {
                render_pass.set_pipeline(&self.overlay_pipeline);
                render_pass.set_vertex_buffer(0, indicator_vertex_buffer.slice(..));
                let indicator_vertex_count = u32::try_from(indicator_vertices.len())
                    .expect("the indicator triangle list stays inside u32");
                render_pass.draw(0..indicator_vertex_count, 0..1);
            }
        }

        capture_frame_bytes(&device, &queue, command_encoder, &frame_texture)
            .map_err(HeadlessRenderError::FrameCapture)
    }

    /// Uploads a pooled payload as a renderable texture, touching the pool's
    /// recency for the tile on both the fresh-load and cached-reuse paths.
    /// Returns `Ok(None)` — no error — for two skip
    /// cases: the payload left the pool between assembly and upload (an
    /// eviction raced the frame), or the payload fails to decode, in which
    /// case the corrupt bytes are evicted so the scheduler's next fetch
    /// replaces them and every later frame recovers. Otherwise returns the
    /// texture and bind group, freshly uploaded on a payload-revision miss or
    /// reused from the GPU texture cache on a hit.
    fn upload_tile_texture(
        &mut self,
        device: &Device,
        tile_key: TileKey,
        pool: &mut TileTexturePool,
    ) -> Result<Option<(Texture, wgpu::BindGroup)>, HeadlessRenderError> {
        // The revision stamps the cache entry, so a pool miss also misses the
        // cache: an upload whose source bytes are gone must never draw.
        let Some(payload_revision) = pool.payload_revision(&tile_key) else {
            return Ok(None);
        };
        if let Some((texture, bind_group)) =
            self.gpu_texture_cache.lookup(&tile_key, payload_revision)
        {
            // Drawing from the cache is still a use of the pooled bytes: keep
            // the payload's recency fresh or LRU would evict exactly the
            // steady-camera tiles the cache exists to avoid refetching.
            pool.refresh_recency(&tile_key);
            return Ok(Some((texture.clone(), bind_group.clone())));
        }
        let Some(encoded_image_bytes) = pool.lookup(&tile_key).map(<[u8]>::to_vec) else {
            return Ok(None);
        };
        match crate::image_limits::decode_image_with_limits(&encoded_image_bytes) {
            Ok(decoded_image) => {
                let (texture, bind_group, texture_bytes) =
                    self.upload_decoded_texture(device, decoded_image);
                self.gpu_texture_cache.insert(
                    tile_key,
                    payload_revision,
                    texture.clone(),
                    bind_group.clone(),
                    texture_bytes,
                );
                Ok(Some((texture, bind_group)))
            }
            Err(_) => {
                pool.evict(&tile_key);
                Ok(None)
            }
        }
    }

    /// Creates a renderable texture from a decoded image, queues its RGBA8
    /// pixels onto the device, and builds the tile bind group; the byte
    /// count feeds the cache budget as the texture's GPU cost.
    fn upload_decoded_texture(
        &mut self,
        device: &Device,
        decoded_image: DynamicImage,
    ) -> (Texture, wgpu::BindGroup, u64) {
        let rgba_image = to_rgba8(decoded_image);
        let rgba_bytes = rgba_image.as_raw();
        let (width_pixels, height_pixels) = rgba_image.dimensions();

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("rustic-maps-rendering-tile-texture"),
            size: Extent3d {
                width: width_pixels,
                height: height_pixels,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FRAME_TEXTURE_FORMAT,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.graphics_context.queue.write_texture(
            TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            rgba_bytes,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(PIXEL_BYTES * width_pixels),
                rows_per_image: None,
            },
            Extent3d {
                width: width_pixels,
                height: height_pixels,
                depth_or_array_layers: 1,
            },
        );
        let bind_group = self.create_tile_bind_group(device, &texture);
        let texture_bytes = u64::from(width_pixels) * u64::from(height_pixels) * PIXEL_BYTES as u64;
        (texture, bind_group, texture_bytes)
    }

    /// Uploads the label layer's packed glyph atlas as an `R8Unorm` texture
    /// when the font's content revision changed since the last upload, and
    /// reuses the cached texture and bind group otherwise — the atlas is
    /// uploaded once per distinct font, never per frame.
    fn ensure_glyph_atlas_texture(
        &mut self,
        device: &Device,
        queue: &Queue,
        font: &LabelFont,
    ) -> wgpu::BindGroup {
        if let Some(cache_entry) = &self.glyph_atlas_texture
            && cache_entry.revision == font.revision()
        {
            return cache_entry.bind_group.clone();
        }
        let atlas = font.atlas();
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("rustic-maps-rendering-glyph-atlas-texture"),
            size: Extent3d {
                width: atlas.width_pixels,
                height: atlas.height_pixels,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: GLYPH_ATLAS_TEXTURE_FORMAT,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            &atlas.pixels,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(GLYPH_ATLAS_PIXEL_BYTES * atlas.width_pixels),
                rows_per_image: None,
            },
            Extent3d {
                width: atlas.width_pixels,
                height: atlas.height_pixels,
                depth_or_array_layers: 1,
            },
        );
        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rustic-maps-rendering-glyph-atlas-bind-group"),
            layout: &self.glyph_pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.glyph_sampler),
                },
            ],
        });
        self.glyph_atlas_texture = Some(GlyphAtlasTextureCacheEntry {
            revision: font.revision(),
            bind_group: bind_group.clone(),
        });
        bind_group
    }

    /// Uploads the placed icons' sprite sheet as an `RGBA8` texture when the
    /// atlas' content revision changed since the last upload, and reuses the
    /// cached texture and bind group otherwise. The sheet draws through the
    /// same textured-quad pipeline (and therefore the same vertex layout) as
    /// the raster tiles, with the sprite atlas bound in place of a tile.
    fn ensure_sprite_atlas_texture(
        &mut self,
        device: &Device,
        queue: &Queue,
        sprite_atlas: &crate::sprite::SpriteAtlas,
    ) -> wgpu::BindGroup {
        if let Some(cache_entry) = &self.sprite_atlas_texture
            && cache_entry.revision == sprite_atlas.revision()
        {
            return cache_entry.bind_group.clone();
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("rustic-maps-rendering-sprite-atlas-texture"),
            size: Extent3d {
                width: sprite_atlas.width_pixels(),
                height: sprite_atlas.height_pixels(),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FRAME_TEXTURE_FORMAT,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            sprite_atlas.rgba_pixels(),
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(PIXEL_BYTES * sprite_atlas.width_pixels()),
                rows_per_image: None,
            },
            Extent3d {
                width: sprite_atlas.width_pixels(),
                height: sprite_atlas.height_pixels(),
                depth_or_array_layers: 1,
            },
        );
        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rustic-maps-rendering-sprite-atlas-bind-group"),
            layout: &self.tile_pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.tile_sampler),
                },
            ],
        });
        self.sprite_atlas_texture = Some(SpriteAtlasTextureCacheEntry {
            revision: sprite_atlas.revision(),
            bind_group: bind_group.clone(),
        });
        bind_group
    }

    fn create_tile_bind_group(&self, device: &Device, texture: &Texture) -> wgpu::BindGroup {
        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rustic-maps-rendering-tile-bind-group"),
            layout: &self.tile_pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.tile_sampler),
                },
            ],
        })
    }
}

/// Normalizes a decoded tile payload to plain RGBA8 without an extra copy for
/// the most common PNG color model.
fn to_rgba8(decoded_image: DynamicImage) -> image::RgbaImage {
    match decoded_image {
        DynamicImage::ImageRgba8(rgba_image) => rgba_image,
        other => other.to_rgba8(),
    }
}

/// The GPU-free gating state of the idle-gated snapshot: how many of a
/// frame's requested tiles are not pooled at their own zoom level. Kept
/// free of `wgpu` types (and of the renderer itself) so the refusal logic is
/// unit-testable and re-derivable without a device; see
/// [`HeadlessTileRenderer::snapshot_frame_when_idle`].
mod idle_gate {
    use crate::tile_scheduler::{PreparedTile, TileReadiness};
    use crate::tile_texture_pool::TileTexturePool;

    /// Counts the frame's requested tiles that are still pending: a verdict
    /// other than [`TileReadiness::Present`] — a fallback still owes the
    /// child's detail, an unavailable fetch is retried, a deferred tile is
    /// fetched — or a `Present` verdict whose payload has left the pool
    /// since the frame was prepared.
    pub(super) fn pending_tile_count(
        prepared_tiles: &[PreparedTile],
        pool: &TileTexturePool,
    ) -> usize {
        prepared_tiles
            .iter()
            .filter(|prepared_tile| {
                prepared_tile.readiness != TileReadiness::Present
                    || !pool.contains(&prepared_tile.requested_tile_key)
            })
            .count()
    }
}

/// Serializes fragment vertices into the vertex and index byte streams the GPU
/// buffers expect: little-endian floats, [`CORNERS_PER_QUAD`] corners per
/// world-copy fragment, and the two-triangle index pattern `0, 1, 2, 2, 1, 3`
/// ([`INDICES_PER_QUAD`] per fragment).
fn encoded_fragment_buffers(fragment_vertices: &[[ClipSpaceTileVertex; 4]]) -> (Vec<u8>, Vec<u8>) {
    let vertex_stride = std::mem::size_of::<ClipSpaceTileVertex>();
    let mut vertex_bytes =
        Vec::with_capacity(fragment_vertices.len() * vertex_stride * CORNERS_PER_QUAD);
    let mut index_bytes =
        Vec::with_capacity(fragment_vertices.len() * INDICES_PER_QUAD * std::mem::size_of::<u16>());
    for (fragment_index, vertices) in fragment_vertices.iter().enumerate() {
        let base_corner_index = u16::try_from(fragment_index * CORNERS_PER_QUAD)
            .expect("the draw list was bounded above");
        for component in vertices.iter().flat_map(|vertex| {
            vertex
                .clip_position
                .iter()
                .chain(vertex.texture_coordinates.iter())
                .chain(std::iter::once(&vertex.opacity))
        }) {
            vertex_bytes.extend_from_slice(&component.to_le_bytes());
        }
        for index in [
            base_corner_index,
            base_corner_index + 1,
            base_corner_index + 2,
            base_corner_index + 2,
            base_corner_index + 1,
            base_corner_index + 3,
        ] {
            index_bytes.extend_from_slice(&index.to_le_bytes());
        }
    }
    (vertex_bytes, index_bytes)
}

/// Serializes the glyph quads into the vertex byte stream the GPU buffer
/// expects: clip position, texture coordinates, paint and the signed-distance
/// threshold, little-endian, in the `repr(C)` field order.
fn encoded_glyph_vertices(glyph_vertices: &[ClipSpaceGlyphVertex]) -> Vec<u8> {
    let mut vertex_bytes = Vec::with_capacity(std::mem::size_of_val(glyph_vertices));
    for vertex in glyph_vertices {
        for component in &vertex.clip_position {
            vertex_bytes.extend_from_slice(&component.to_le_bytes());
        }
        for component in &vertex.texture_coordinates {
            vertex_bytes.extend_from_slice(&component.to_le_bytes());
        }
        for component in &vertex.rgba_color {
            vertex_bytes.extend_from_slice(&component.to_le_bytes());
        }
        vertex_bytes.extend_from_slice(&vertex.signed_distance_threshold.to_le_bytes());
        vertex_bytes.extend_from_slice(&vertex.signed_distance_gamma.to_le_bytes());
    }
    vertex_bytes
}

/// Serializes the overlay triangle-list vertices into the vertex byte stream
/// the GPU buffer expects: little-endian floats. The overlay draws
/// non-indexed (a plain vertex draw over the flat triangle list), so no index
/// stream exists for this layer.
fn encoded_overlay_vertices(overlay_vertices: &[ClipSpaceOverlayVertex]) -> Vec<u8> {
    let mut vertex_bytes = Vec::with_capacity(std::mem::size_of_val(overlay_vertices));
    for vertex in overlay_vertices {
        for component in vertex.clip_position.iter().chain(vertex.rgba_color.iter()) {
            vertex_bytes.extend_from_slice(&component.to_le_bytes());
        }
    }
    vertex_bytes
}

/// Creates a device buffer pre-filled from host bytes.
fn create_filled_buffer(
    device: &Device,
    queue: &Queue,
    bytes: &[u8],
    usage: wgpu::BufferUsages,
    label: Option<&str>,
) -> Buffer {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label,
        size: bytes.len() as wgpu::BufferAddress,
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&buffer, 0, bytes);
    buffer
}

/// Copies a rendered texture into a staging buffer honoring the 256-byte row
/// alignment ([`wgpu::COPY_BYTES_PER_ROW_ALIGNMENT`]), maps the buffer, and
/// reads it back as a tightly packed [`CapturedFrame`], stripping the padding
/// the alignment inserts between rows.
///
/// # Reusability
///
/// Any render-attachment texture (`RENDER_ATTACHMENT | COPY_SRC`, RGBA8
/// unorm) of any size is captured. The copy commands are encoded onto the
/// provided encoder, which the caller has not submitted yet and which this
/// function then finishes and submits; mapping completes through device
/// polling — native only (see the module docs).
pub fn capture_frame_bytes(
    device: &Device,
    queue: &Queue,
    mut command_encoder: CommandEncoder,
    source_texture: &Texture,
) -> Result<CapturedFrame, String> {
    let texture_size = source_texture.size();
    let width_pixels = texture_size.width;
    let height_pixels = texture_size.height;

    let unpadded_row_bytes = width_pixels as usize * PIXEL_BYTES as usize;
    let bytes_per_row = u32::try_from(aligned_bytes_per_row(width_pixels))
        .expect("aligned row length stays below u32 for viewport sizes");
    let staging_buffer_size = u64::from(bytes_per_row) * u64::from(height_pixels.saturating_sub(1))
        + unpadded_row_bytes as u64;
    let staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("rustic-maps-rendering-frame-staging"),
        size: staging_buffer_size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    command_encoder.copy_texture_to_buffer(
        TexelCopyTextureInfo {
            texture: source_texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: TextureAspect::All,
        },
        TexelCopyBufferInfo {
            buffer: &staging_buffer,
            layout: TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: None,
            },
        },
        Extent3d {
            width: width_pixels,
            height: height_pixels,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([command_encoder.finish()]);

    let (map_result_sender, map_result_receiver) = mpsc::channel::<BufferMapOutcome>();
    staging_buffer
        .slice(..)
        .map_async(MapMode::Read, move |map_result| {
            let _ = map_result_sender.send(map_result.map_err(|error| error.to_string()));
        });

    // The map callback is delivered by polling the device; on native backends
    // only a blocking `Wait` poll guarantees the map completes, so the loop
    // escalates to one on the first empty receive rather than spinning on
    // non-blocking polls.
    loop {
        match map_result_receiver.try_recv() {
            Ok(map_result) => {
                map_result?;
                break;
            }
            Err(TryRecvError::Disconnected) => {
                return Err(
                    "buffer map callback sender dropped before the map completed".to_string(),
                );
            }
            Err(TryRecvError::Empty) => {
                device
                    .poll(PollType::wait_indefinitely())
                    .map_err(|poll_error| poll_error.to_string())?;
            }
        }
    }

    let mapped_bytes = staging_buffer
        .slice(..)
        .get_mapped_range()
        .map_err(|error| error.to_string())?;
    let mut rgba_bytes = Vec::with_capacity(unpadded_row_bytes * height_pixels as usize);
    for row_index in 0..height_pixels as usize {
        let row_start = row_index * bytes_per_row as usize;
        rgba_bytes.extend_from_slice(&mapped_bytes[row_start..row_start + unpadded_row_bytes]);
    }
    drop(mapped_bytes);
    staging_buffer.unmap();

    Ok(CapturedFrame {
        width_pixels,
        height_pixels,
        rgba_bytes,
    })
}
#[cfg(test)]
mod tests {
    use super::idle_gate;
    use super::{ROW_ALIGNMENT_BYTES, aligned_bytes_per_row};
    use crate::tile_key::TileKey;
    use crate::tile_scheduler::{PreparedTile, TileReadiness};
    use crate::tile_texture_pool::TileTexturePool;

    fn tile_key(tile_x: u32, tile_y: u32, zoom_level: u8) -> TileKey {
        TileKey::new(tile_x, tile_y, zoom_level).unwrap()
    }

    fn prepared(requested_tile_key: TileKey, tile_readiness: TileReadiness) -> Vec<PreparedTile> {
        vec![PreparedTile {
            requested_tile_key,
            readiness: tile_readiness,
        }]
    }

    #[test]
    fn rows_pad_to_the_copy_alignment() {
        // 512-pixel rows of RGBA8 are already 2048 bytes, a whole multiple.
        assert_eq!(aligned_bytes_per_row(512), 2048);
        // 510 pixels round up 2040 bytes to the next multiple of 256.
        assert_eq!(aligned_bytes_per_row(510), 2048);
        // 511 pixels, one pixel short of the aligned stride, also round up.
        assert_eq!(aligned_bytes_per_row(511), 2048);
    }

    #[test]
    fn every_supported_row_width_stays_aligned() {
        for width_pixels in [1, 2, 64, 100, 129, 257, 999, 4096] {
            assert_eq!(
                aligned_bytes_per_row(width_pixels) % ROW_ALIGNMENT_BYTES,
                0,
                "width {width_pixels}"
            );
        }
    }

    #[test]
    fn the_idle_gate_counts_only_tiles_without_their_own_pooled_payload() {
        let pooled_tile = tile_key(1, 2, 3);
        let mut payload_pool = TileTexturePool::new(1 << 20);
        payload_pool.insert(pooled_tile, vec![1, 2, 3]);

        // Every unresolved verdict is pending, pooled payload or not.
        assert_eq!(
            idle_gate::pending_tile_count(
                &prepared(
                    pooled_tile,
                    TileReadiness::FallsBackTo {
                        fallback_tile_key: pooled_tile.parent_tile_key().expect("non-root"),
                        zoom_level_difference: 1,
                    },
                ),
                &payload_pool
            ),
            1
        );
        assert_eq!(
            idle_gate::pending_tile_count(
                &prepared(pooled_tile, TileReadiness::Deferred),
                &payload_pool
            ),
            1
        );
        assert_eq!(
            idle_gate::pending_tile_count(
                &prepared(pooled_tile, TileReadiness::Unavailable),
                &payload_pool
            ),
            1
        );
        // A present verdict over a payload still in the pool is idle.
        assert_eq!(
            idle_gate::pending_tile_count(
                &prepared(pooled_tile, TileReadiness::Present),
                &payload_pool
            ),
            0
        );
        // A present verdict whose payload left the pool since the frame was
        // prepared is pending again — its area would capture as a hole.
        assert!(payload_pool.evict(&pooled_tile));
        assert_eq!(
            idle_gate::pending_tile_count(
                &prepared(pooled_tile, TileReadiness::Present),
                &payload_pool
            ),
            1
        );
    }

    #[test]
    fn the_idle_gate_summarizes_the_whole_frame() {
        let present_tile = tile_key(0, 0, 3);
        let deferred_tile = tile_key(1, 0, 3);
        let evicted_present_tile = tile_key(2, 0, 3);
        let mut payload_pool = TileTexturePool::new(1 << 20);
        for pooled_tile in [present_tile, deferred_tile, evicted_present_tile] {
            payload_pool.insert(pooled_tile, vec![1, 2, 3]);
        }
        assert!(payload_pool.evict(&evicted_present_tile));
        let mut frame = prepared(present_tile, TileReadiness::Present);
        frame.extend(prepared(deferred_tile, TileReadiness::Deferred));
        frame.extend(prepared(evicted_present_tile, TileReadiness::Present));
        assert_eq!(
            idle_gate::pending_tile_count(&frame, &payload_pool),
            2,
            "the deferred tile and the evicted payload count as pending"
        );
        // An empty request set is idle by vacuity.
        assert_eq!(idle_gate::pending_tile_count(&[], &payload_pool), 0);
    }
}
