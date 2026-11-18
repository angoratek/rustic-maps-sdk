//! Surface rendering of raster tiles plus overlays onto a live native
//! window: the same scene composition as the headless renderer, presented
//! through a real `wgpu::Surface`.
//!
//! The renderer draws one textured quad per pooled tile for a single frame
//! and an optional overlay layer ([`crate::overlay::MapOverlay`]) after the
//! quads in the same render pass — byte-for-byte the headless pipeline's
//! geometry and shaders (the private shader sources and encode helpers mirror
//! [`crate::headless`], and the WebAssembly canvas follows the same pattern) —
//! but the frame renders into the next acquired surface texture and is
//! presented, not captured. The pipelines therefore target the *surface's
//! negotiated format* (for example
//! Bgra8Unorm under Metal), unlike the headless capture's fixed
//! `FRAME_TEXTURE_FORMAT`; the negotiation happens once at construction from
//! the documented preference list and every later configuration keeps it.
//!
//! # Lifecycle
//!
//! Per-platform raw window handles lower into a [`wgpu::SurfaceTargetUnsafe`]
//! *outside* this crate, which takes over from there — this module is
//! platform-agnostic and never names a windowing backend:
//!
//! 1. Wrap the unsafe target with
//!    [`SurfaceRenderTarget::from_unsafe_target`], which creates the
//!    `wgpu::Instance` and the `'static` [`wgpu::Surface`] against it. The
//!    raw handle must stay valid for the surface's whole lifetime;
//!    `wgpu`'s own validity rules apply (see
//!    [`wgpu::SurfaceTargetUnsafe`]).
//! 2. Build the renderer with [`SurfaceTileRenderer::new`], acquiring the
//!    adapter/device/queue through the injected
//!    [`SurfaceGraphicsContextSource`] (production:
//!    [`WgpuSurfaceGraphicsContextSource`], which requests an adapter
//!    *compatible with the surface*, unlike the headless source). The
//!    surface format is negotiated here.
//! 3. Size the render area explicitly with
//!    [`SurfaceTileRenderer::configure_extent`] — resize is a call, not an
//!    observation: only a configure changes the swap-chain extent. A zero
//!    extent is refused; an already-configured extent reconfigures only when
//!    the extent changes.
//! 4. Draw every frame with [`SurfaceTileRenderer::render_frame`]; the
//!    frame's viewport must match the configured extent exactly. A stale
//!    surface (window resized or torn down behind the renderer's back)
//!    reconfigures once and retries the acquisition before failing.
//! 5. Detach by dropping the renderer: the `wgpu` surface's own release
//!    semantics are its drop behavior (wgpu 30 has no separate
//!    un-configure call), so the host's surface view drops the renderer — and
//!    every [`SurfaceRenderTarget`] it wraps — when its window handle goes
//!    away.
//!
//! Attaching and detaching the underlying platform view belong to the
//! caller. Dropping the [`SurfaceRenderTarget`] while its raw handle is
//! reused elsewhere is a caller error to avoid, so the host code that owns
//! the view must outlive the renderer.
//!
//! # Native only
//!
//! Like [`crate::headless`], this module blocks on the asynchronous `wgpu`
//! initialization and configuration calls (`pollster`) and is not valid on
//! WebAssembly, where the browser runtime drives WebGPU.

use crate::camera::{MapCamera, ViewportPixels};
use crate::gpu_texture_cache::GpuTileTextureCache;
use crate::headless::{
    GLYPH_ATLAS_PIXEL_BYTES, GLYPH_ATLAS_SHADER_SOURCE, GLYPH_ATLAS_TEXTURE_FILTER,
    GLYPH_ATLAS_TEXTURE_FORMAT, GraphicsContextError, MAXIMUM_QUADS_PER_FRAME,
    TILE_TEXTURE_ADDRESS_MODE, TILE_TEXTURE_FILTER, TILE_TEXTURE_MIPMAP_FILTER,
};
use crate::labels::{ClipSpaceGlyphVertex, LabelFont, LabelLayer};
use crate::overlay::{
    ClipSpaceOverlayVertex, MapOverlay, OVERLAY_TRIANGLE_VERTEX_COUNT, UNSCALED_DEVICE_PIXEL_RATIO,
    overlay_triangle_vertices_for_layers,
};
use crate::renderer::{CLEAR_COLOR_RGBA, ClipSpaceTileVertex, Renderer};
use crate::tile_scheduler::{PreparedTile, TileReadiness};
use crate::tile_texture_pool::TileTexturePool;
use image::DynamicImage;
use wgpu::{
    AdapterInfo, Buffer, Device, Extent3d, Queue, TexelCopyBufferLayout, TexelCopyTextureInfo,
    Texture, TextureAspect, TextureUsages,
};

/// GPU format of the uploaded tile textures. The provider tile payloads
/// decode to plain RGBA8, so no sRGB conversion enters the pipeline and
/// texel values pass through unchanged onto the non-sRGB surface formats.
const TILE_TEXTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Bytes in one RGBA8 texel, of both tile pixels and surface pixels.
/// Verbatim mirror of the headless renderer's [`crate::headless`] private
/// constant of the same name.
const PIXEL_BYTES: u32 = 4;

/// Indices consumed by one drawn quad: the two triangles of the corner
/// pattern `0, 1, 2, 2, 1, 3`. Verbatim mirror of the headless renderer's
/// private constant of the same name.
const INDICES_PER_QUAD: usize = 6;

/// Corners of one drawn quad: the four vertices one world-copy fragment
/// contributes. Verbatim mirror of the headless renderer's private constant
/// of the same name.
const CORNERS_PER_QUAD: usize = 4;

/// WGSL pipeline: a single textured-quad shader. Verbatim mirror of the
/// headless renderer's private shader source; the vertex positions arrive
/// already in clip space (the camera transform runs on the CPU in `f64`), so
/// the vertex stage is a pass-through and the fragment stage samples the
/// tile texture.
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
/// blending. Verbatim mirror of the headless renderer's private shader
/// source (and of the documented wasm canvas mirror's shader text).
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

/// Surface formats the renderer accepts, most preferred first. Both are
/// non-sRGB, so the tile texel values pass through unchanged to the screen.
/// Mirror of the wasm canvas' surface-format preference list, ordered for the
/// same reason: Metal's native format first.
const SURFACE_FORMAT_PREFERENCE: [wgpu::TextureFormat; 2] = [
    wgpu::TextureFormat::Bgra8Unorm,
    wgpu::TextureFormat::Rgba8Unorm,
];

/// The structured events target the surface frame path emits through (the
/// per-frame accounting line); the diagnostics sink's severity gate holds
/// the GPU-hot path's cost.
const SURFACE_TRACING_TARGET: &str = "renderer";

/// Presentation pacing: frames queue in FIFO order, trading latency for the
/// absence of tearing, and preferred alpha compositing — mirrors of the wasm
/// canvas' surface configuration. The alpha mode is a *preference*: a
/// platform that does not support opaque compositing (the Android
/// emulator's ANGLE stack reports only `Inherit`, for example) negotiates
/// the first mode it does support instead of failing the device request,
/// and every mode the renderer handles treats the tile texels' pass-through
/// alpha the same way across its three consumers.
const PRESENT_MODE: wgpu::PresentMode = wgpu::PresentMode::Fifo;
const PREFERRED_ALPHA_MODE: wgpu::CompositeAlphaMode = wgpu::CompositeAlphaMode::Opaque;

/// Desired frames in flight of the surface configuration: the pacing of a
/// small render-on-demand map view, matching the wasm canvas.
const DESIRED_MAXIMUM_FRAME_LATENCY: u32 = 2;

/// How many times one render frame tries to acquire the surface texture,
/// reconfiguring between attempts; more attempts would fight a surface the
/// platform keeps tearing down. Mirror of the wasm canvas.
const SURFACE_ACQUISITION_ATTEMPTS: usize = 2;

/// A live presentation target: the `wgpu` instance and the surface created
/// against it from a platform-provided [`wgpu::SurfaceTargetUnsafe`]. The
/// surface is `'static` — `wgpu` relies on the raw handle staying valid
/// while the surface exists, which is the platform glue's responsibility
/// (see the module documentation on the lifecycle).
#[derive(Debug)]
pub struct SurfaceRenderTarget {
    /// Instance the surface was created against; the surface's backend
    /// objects borrow from it, so it outlives the surface by construction.
    pub instance: wgpu::Instance,
    /// The live surface of a platform window or layer.
    pub surface: wgpu::Surface<'static>,
}

impl SurfaceRenderTarget {
    /// Creates the `wgpu` instance and takes the surface for an
    /// already-assembled unsafe target (a raw window handle, a Metal layer
    /// pointer, or one of the typed variants).
    ///
    /// The instance honours the standard `WGPU_BACKEND` selection, so a host
    /// whose compositor exposes only one usable backend — an Android emulator
    /// with a software GLES stack, for example — can pin it before the first
    /// surface call. With the variable unset every compiled backend stays
    /// enabled.
    ///
    /// # Safety
    ///
    /// The target's raw handle must point to a live platform object and stay
    /// valid for the surface's whole lifetime — the guarantee the platform
    /// glue owning that object makes when it assembles the
    /// [`wgpu::SurfaceTargetUnsafe`]. A handle already returned to the
    /// platform cannot be validated here; this constructor trusts it (see
    /// the module documentation on where the guarantee lives).
    ///
    /// # Errors
    /// Returns [`SurfaceRenderError`] when `wgpu` cannot create a surface for
    /// the target.
    pub fn from_unsafe_target(
        surface_target: wgpu::SurfaceTargetUnsafe,
    ) -> Result<Self, SurfaceRenderError> {
        let mut instance_descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        instance_descriptor.backends = wgpu::Backends::from_env().unwrap_or_default();
        let instance = wgpu::Instance::new(instance_descriptor);
        // SAFETY: the platform glue guarantees the raw handle's validity for
        // the surface's whole lifetime (see the safety note above).
        let surface = unsafe { instance.create_surface_unsafe(surface_target) }
            .map_err(|error| SurfaceRenderError::SurfaceTargetInvalid(error.to_string()))?;
        Ok(Self { instance, surface })
    }
}

/// A surface-compatible adapter/device/queue triple for surface rendering.
/// Carries the [`wgpu::Adapter`] itself — unlike the headless context —
/// because surface configuration and format negotiation consult the
/// adapter's capabilities on every extent change.
#[derive(Debug)]
pub struct SurfaceGraphicsContext {
    /// Adapter to negotiate the surface against and pass to
    /// `get_default_config`.
    pub adapter: wgpu::Adapter,
    /// Logical device the renderer creates resources on.
    pub device: Device,
    /// Queue the renderer submits command buffers through.
    pub queue: Queue,
    /// Adapter description, naming the backend that renders the frames.
    pub adapter_information: AdapterInfo,
}

/// Source of the graphics device used for surface rendering.
///
/// Implement this trait to supply a surface-compatible device acquired
/// elsewhere instead of relying on environment adapter discovery. The surface
/// target is passed in so the production adapter search can require
/// compatibility with it.
pub trait SurfaceGraphicsContextSource: Send + Sync {
    /// Acquires a surface-compatible device/queue pair, or explains why none
    /// was available.
    fn request_surface_graphics_context(
        &self,
        surface_target: &SurfaceRenderTarget,
    ) -> Result<SurfaceGraphicsContext, GraphicsContextError>;
}

/// The production source: requests the first available adapter compatible
/// with the wrapping surface, blocking on it with `pollster` (native only;
/// see the module documentation). The device/queue request itself is the
/// headless source's call sequence verbatim.
pub struct WgpuSurfaceGraphicsContextSource;

impl SurfaceGraphicsContextSource for WgpuSurfaceGraphicsContextSource {
    fn request_surface_graphics_context(
        &self,
        surface_target: &SurfaceRenderTarget,
    ) -> Result<SurfaceGraphicsContext, GraphicsContextError> {
        let adapter = pollster::block_on(surface_target.instance.request_adapter(
            &wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface_target.surface),
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            },
        ))
        .map_err(|error| GraphicsContextError::AdapterUnavailable(error.to_string()))?;

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("rustic-maps-rendering-surface"),
            required_features: wgpu::Features::empty(),
            // WebGL2 downlevel defaults keep the request within what every
            // backend reports: some Metal adapters (iOS simulator runtimes,
            // for example) cap `max_inter_stage_shader_variables` at 15, one
            // below the `Limits::default()` request, and the Android
            // emulator's ANGLE/SwiftShader device reports a compute-
            // workgroup limit of zero, which the plain downlevel defaults
            // still exceed. The renderer draws no compute work and uses only
            // a handful of varyings, so the WebGL2 limits are sufficient.
            required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        }))
        .map_err(|error| GraphicsContextError::DeviceRequestFailed(error.to_string()))?;

        Ok(SurfaceGraphicsContext {
            adapter_information: adapter.get_info(),
            adapter,
            device,
            queue,
        })
    }
}

/// Why rendering to a live surface could not proceed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SurfaceRenderError {
    /// The raw window handle could not become a `wgpu` surface (the handle
    /// was already dead, or the platform backend does not recognize it).
    SurfaceTargetInvalid(String),
    /// The graphics context could not be acquired at all: either no adapter
    /// matched (for example on a machine without a usable GPU backend) or
    /// the device request failed. The detail is the headless context
    /// source's own verbatim failure.
    GraphicsContextUnavailable(GraphicsContextError),
    /// The surface refused configuration: no preferred format was
    /// supported, the extent was unusable, or configuration itself failed.
    SurfaceConfigurationFailed(String),
    /// The surface texture could not be acquired even after the
    /// documented reconfigure-and-retry attempt: the surface stayed stale,
    /// lost or invalid.
    SurfaceTextureAcquisitionFailed(String),
    /// The frame's viewport did not match the configured surface extent:
    /// the projection would not map onto the presented pixels, so nothing
    /// was drawn.
    ViewportExtentMismatch {
        /// Viewport extent the frame asked to draw through.
        viewport_extent: (u32, u32),
        /// Extent the surface is currently configured to.
        configured_extent: (u32, u32),
    },
    /// The draw list would overflow the 16-bit index buffer.
    DrawListTooLarge {
        /// Draw quads (world-copy fragments) assembled for the frame.
        quad_count: usize,
        /// Draw quads the index buffer supports.
        maximum_quad_count: usize,
    },
}

impl From<GraphicsContextError> for SurfaceRenderError {
    fn from(error: GraphicsContextError) -> Self {
        SurfaceRenderError::GraphicsContextUnavailable(error)
    }
}

/// One presented surface frame's tile and overlay accounting, straight out
/// of the scheduler's readiness verdicts and the overlay assembly — every
/// requested tile lands in exactly one of the four readiness counts, so
/// `present + fallback + deferred + unavailable` always equals `requested`.
/// Drawn pixels this frame come from the `present` and `fallback` tiles;
/// the `deferred` and `unavailable` tiles are where later frames may still
/// change pixels (the pending half of the fetch-state taxonomy).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceFrameStatistics {
    /// Tile requests the frame resolved (one entry per request — a request
    /// listing duplicates costs one fetch but reports every request).
    pub requested_tile_count: u32,
    /// Tiles drawn at their own zoom level.
    pub present_tile_count: u32,
    /// Tiles drawn from their nearest pooled ancestor.
    pub fallback_tile_count: u32,
    /// Tiles whose fetch stayed inside the per-frame budget deferral.
    pub deferred_tile_count: u32,
    /// Tiles the provider rejected this frame.
    pub unavailable_tile_count: u32,
    /// Clip-space triangles the overlay layer contributed to the frame.
    pub overlay_triangle_count: u32,
}

impl SurfaceFrameStatistics {
    /// Builds the statistics from a frame's readiness verdicts and its
    /// overlay vertex stream (a flat, non-indexed triangle list).
    pub(crate) fn from_frame_readiness_and_overlay_vertices(
        prepared_tiles: &[PreparedTile],
        overlay_vertex_count: usize,
    ) -> Self {
        let mut statistics = Self {
            requested_tile_count: prepared_tiles.len() as u32,
            present_tile_count: 0,
            fallback_tile_count: 0,
            deferred_tile_count: 0,
            unavailable_tile_count: 0,
            overlay_triangle_count: (overlay_vertex_count / OVERLAY_TRIANGLE_VERTEX_COUNT) as u32,
        };
        for prepared_tile in prepared_tiles {
            match prepared_tile.readiness {
                TileReadiness::Present => statistics.present_tile_count += 1,
                TileReadiness::FallsBackTo { .. } => statistics.fallback_tile_count += 1,
                TileReadiness::Deferred => statistics.deferred_tile_count += 1,
                TileReadiness::Unavailable => statistics.unavailable_tile_count += 1,
            }
        }
        // The per-frame accounting event (the "renderer" target): one debug
        // line per presented frame, held behind the subscribers' severity
        // gate like the "tile_fetch" target's own events.
        tracing::debug!(
            target: SURFACE_TRACING_TARGET,
            requested_tile_count = statistics.requested_tile_count,
            present_tile_count = statistics.present_tile_count,
            fallback_tile_count = statistics.fallback_tile_count,
            deferred_tile_count = statistics.deferred_tile_count,
            unavailable_tile_count = statistics.unavailable_tile_count,
            "surface frame resolved"
        );
        statistics
    }
}

impl std::fmt::Display for SurfaceRenderError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SurfaceRenderError::SurfaceTargetInvalid(detail) => {
                write!(formatter, "surface target invalid: {detail}")
            }
            SurfaceRenderError::GraphicsContextUnavailable(error) => {
                write!(formatter, "graphics context unavailable: {error}")
            }
            SurfaceRenderError::SurfaceConfigurationFailed(detail) => {
                write!(formatter, "surface configuration failed: {detail}")
            }
            SurfaceRenderError::SurfaceTextureAcquisitionFailed(detail) => {
                write!(formatter, "surface texture acquisition failed: {detail}")
            }
            SurfaceRenderError::ViewportExtentMismatch {
                viewport_extent,
                configured_extent,
            } => write!(
                formatter,
                "viewport extent {viewport_extent:?} does not match the \
                 configured surface extent {configured_extent:?}"
            ),
            SurfaceRenderError::DrawListTooLarge {
                quad_count,
                maximum_quad_count,
            } => write!(
                formatter,
                "draw list of {quad_count} quads exceeds the supported {maximum_quad_count}"
            ),
        }
    }
}

impl std::error::Error for SurfaceRenderError {}

/// One uploaded glyph atlas texture, stamped with the font's content
/// revision so the atlas is uploaded once per distinct font rather than per
/// frame. The bind group owns the texture view, which keeps the atlas alive.
struct GlyphAtlasTextureCacheEntry {
    revision: u64,
    bind_group: wgpu::BindGroup,
}

/// One uploaded sprite atlas texture, stamped with the atlas' content
/// revision so the sheet is uploaded once per distinct sprite. The bind group
/// owns the texture view, which keeps the atlas alive.
struct SpriteAtlasTextureCacheEntry {
    revision: u64,
    bind_group: wgpu::BindGroup,
}

/// Renders raster tiles to a live `wgpu` surface. Owns the pipelines,
/// sampler, uploaded-texture cache and the negotiated surface format; the
/// graphics context is injected, so production code and tests can supply any
/// surface-backed device through [`SurfaceGraphicsContextSource`]. The
/// pipeline pair, upload path and encode helpers mirror the headless
/// renderer exactly (see the module documentation).
pub struct SurfaceTileRenderer {
    surface_target: SurfaceRenderTarget,
    graphics_context: SurfaceGraphicsContext,
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
    /// The negotiated, non-sRGB swap-chain format the pipelines were built
    /// against; every later configuration keeps it.
    surface_format: wgpu::TextureFormat,
    /// The frame pass's clear color, behind every tile, overlay and label.
    /// Starts at the shared dark clear; a styled vector base replaces it with
    /// the style's own canvas background.
    clear_color: [f32; 4],
}

impl SurfaceTileRenderer {
    /// Acquires a surface-compatible graphics context through
    /// `context_source`, negotiates the surface format against the target's
    /// capabilities, and builds the tile and overlay pipelines on the acquired
    /// device.
    ///
    /// # Parameters
    /// - `surface_target` — the surface and instance to render into.
    /// - `context_source` — the source of a surface-compatible device.
    ///
    /// # Errors
    /// Returns [`SurfaceRenderError`] when no device is available or when the
    /// surface supports none of the preferred formats.
    pub fn new(
        surface_target: SurfaceRenderTarget,
        context_source: &dyn SurfaceGraphicsContextSource,
    ) -> Result<Self, SurfaceRenderError> {
        let graphics_context = context_source.request_surface_graphics_context(&surface_target)?;
        let surface_capabilities = surface_target
            .surface
            .get_capabilities(&graphics_context.adapter);
        let surface_format =
            preferred_surface_format(&surface_capabilities.formats).ok_or_else(|| {
                SurfaceRenderError::SurfaceConfigurationFailed(format!(
                    "the surface supports none of the formats \
                     {SURFACE_FORMAT_PREFERENCE:?}"
                ))
            })?;
        let tile_pipeline = build_tile_pipeline(&graphics_context, surface_format);
        let overlay_pipeline = build_overlay_pipeline(&graphics_context, surface_format);
        let glyph_pipeline = build_glyph_pipeline(&graphics_context, surface_format);
        let tile_sampler = build_tile_sampler(&graphics_context);
        let glyph_sampler = build_glyph_sampler(&graphics_context);
        Ok(Self {
            surface_target,
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
                crate::gpu_texture_cache::GPU_TILE_TEXTURE_CACHE_DEFAULT_MAXIMUM_BYTES,
            ),
            surface_format,
            clear_color: CLEAR_COLOR_RGBA,
        })
    }

    /// Borrows the wrapped surface target, for diagnostics and for the
    /// platform glue that wants to observe the instance it came from.
    pub fn surface_target(&self) -> &SurfaceRenderTarget {
        &self.surface_target
    }

    /// Description of the adapter this renderer draws with.
    pub fn adapter_information(&self) -> &AdapterInfo {
        &self.graphics_context.adapter_information
    }

    /// The negotiated, non-sRGB swap-chain format the pipelines render in.
    pub fn surface_format(&self) -> wgpu::TextureFormat {
        self.surface_format
    }

    /// Replaces the frame pass's clear color — the surface behind any tile,
    /// overlay or label that does not cover it. The default is the shared
    /// dark clear ([`CLEAR_COLOR_RGBA`]); a styled vector base sets its own
    /// canvas background here so the gaps between its translucent fills read
    /// as the designed map, exactly as the browser canvas'
    /// `setBackgroundColor` does.
    pub fn set_clear_color(&mut self, clear_color: [f32; 4]) {
        self.clear_color = clear_color;
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

    /// The extent the surface is currently configured to, read from the
    /// surface's own configuration state, if attached to a usable
    /// configuration.
    fn configured_surface_extent(&self) -> Option<(u32, u32)> {
        self.surface_target
            .surface
            .get_configuration()
            .map(|configuration| (configuration.width, configuration.height))
    }

    /// Sizes the render area explicitly: only this call changes the
    /// swap-chain extent. A zero extent is refused (a surface with no pixels
    /// is not configurable);
    /// reconfiguring happens only when the extent actually changed, so a
    /// repeated call on extent change is idempotent. The first configure
    /// adopts the negotiated surface format and the documented present and
    /// alpha modes; the format is never renegotiated, so a platform surprise
    /// that drops support for it surfaces here as a configuration failure.
    ///
    /// # Parameters
    /// - `width_pixels` — the new render-area width, in physical pixels.
    /// - `height_pixels` — the new render-area height, in physical pixels.
    ///
    /// # Errors
    /// Returns [`SurfaceRenderError::SurfaceConfigurationFailed`] when the
    /// extent is zero or when the surface rejects the configuration.
    pub fn configure_extent(
        &mut self,
        width_pixels: u32,
        height_pixels: u32,
    ) -> Result<(), SurfaceRenderError> {
        self.configure_surface_for_extent(width_pixels, height_pixels, false)
    }

    /// Configures the surface for an extent, overriding the `wgpu`
    /// defaults for format, present and alpha mode exactly like the wasm
    /// canvas does; `force_reconfiguration` is the stale-surface reset of
    /// the acquisition retry.
    fn configure_surface_for_extent(
        &mut self,
        width_pixels: u32,
        height_pixels: u32,
        force_reconfiguration: bool,
    ) -> Result<(), SurfaceRenderError> {
        if width_pixels == 0 || height_pixels == 0 {
            return Err(SurfaceRenderError::SurfaceConfigurationFailed(
                "the surface extent must be non-zero: a surface with no \
                 pixels cannot be configured"
                    .to_string(),
            ));
        }
        if !force_reconfiguration
            && self.configured_surface_extent() == Some((width_pixels, height_pixels))
        {
            return Ok(());
        }
        let mut configuration = self
            .surface_target
            .surface
            .get_default_config(&self.graphics_context.adapter, width_pixels, height_pixels)
            .ok_or_else(|| {
                SurfaceRenderError::SurfaceConfigurationFailed(format!(
                    "the surface supports none of the surface formats \
                     {SURFACE_FORMAT_PREFERENCE:?}"
                ))
            })?;
        configuration.format = self.surface_format;
        configuration.present_mode = PRESENT_MODE;
        configuration.alpha_mode =
            negotiated_alpha_mode(&self.surface_target.surface, &self.graphics_context.adapter)?;
        configuration.desired_maximum_frame_latency = DESIRED_MAXIMUM_FRAME_LATENCY;
        self.surface_target
            .surface
            .configure(&self.graphics_context.device, &configuration);
        Ok(())
    }

    /// Reads the surface texture at the currently configured extent,
    /// reconfiguring the surface once on a stale or lost surface between
    /// attempts (the documented stale-surface retry, mirroring the wasm
    /// canvas).
    fn acquire_surface_texture(&mut self) -> Result<wgpu::SurfaceTexture, SurfaceRenderError> {
        for attempt_index in 0..SURFACE_ACQUISITION_ATTEMPTS {
            match self.surface_target.surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(surface_texture)
                | wgpu::CurrentSurfaceTexture::Suboptimal(surface_texture) => {
                    return Ok(surface_texture);
                }
                // Outdated, Lost and Validation mean a stale or dead surface
                // configuration is at fault, so the next attempt reconfigures
                // (see the `wgpu` guidance on each) and tries again.
                wgpu::CurrentSurfaceTexture::Outdated
                | wgpu::CurrentSurfaceTexture::Lost
                | wgpu::CurrentSurfaceTexture::Validation
                    if attempt_index + 1 < SURFACE_ACQUISITION_ATTEMPTS =>
                {
                    // The configuration can be gone exactly here, on the
                    // recovery path the branch exists for; report it rather
                    // than panicking at the FFI boundary.
                    let Some((width_pixels, height_pixels)) = self.configured_surface_extent()
                    else {
                        return Err(SurfaceRenderError::SurfaceConfigurationFailed(
                            "the surface configuration was lost and could not be read back"
                                .to_string(),
                        ));
                    };
                    self.configure_surface_for_extent(width_pixels, height_pixels, true)?;
                }
                status_variant => {
                    return Err(SurfaceRenderError::SurfaceTextureAcquisitionFailed(
                        format!(
                            "the map surface could not provide a frame texture: \
                             {status_variant:?}"
                        ),
                    ));
                }
            }
        }
        Err(SurfaceRenderError::SurfaceTextureAcquisitionFailed(
            "the map surface was never reconfigured into a usable state".to_string(),
        ))
    }

    /// Renders one frame of tiles plus overlays to the live surface and
    /// presents it. The three-layer variant adds the location-indicator
    /// layer; this signature keeps the two-layer contract byte-stable by
    /// delegating with no indicator.
    ///
    /// # Parameters
    /// - `camera` — the frame's camera pose.
    /// - `viewport` — the render area; it must match the configured extent.
    /// - `prepared_tiles` — the scheduler's readiness verdicts for the frame.
    /// - `pool` — the pooled tile payloads the frame draws from.
    /// - `overlays` — the overlay shapes drawn over the tiles.
    ///
    /// # Returns
    /// The frame's readiness and overlay statistics.
    ///
    /// # Errors
    /// Returns [`SurfaceRenderError`] when the surface cannot be acquired or
    /// configured for the requested extent.
    pub fn render_frame(
        &mut self,
        camera: &MapCamera,
        viewport: &ViewportPixels,
        prepared_tiles: &[PreparedTile],
        pool: &mut TileTexturePool,
        overlays: &[MapOverlay],
    ) -> Result<SurfaceFrameStatistics, SurfaceRenderError> {
        self.render_frame_layers(camera, viewport, prepared_tiles, pool, overlays, None)
    }

    /// The frame's explicit three-layer variant: the pooled tiles' textured
    /// quads, then the overlay layer, then the location-indicator layer
    /// (the indicator draws last, the frame's explicit top layer — see
    /// [`crate::location_indicator::LOCATION_INDICATOR_TOP_LAYER_DRAW_ORDER`]).
    ///
    /// The frame composition — quad assembly, texture uploads with
    /// revision-stamped cache reuse, corrupted-payload eviction, the
    /// tiles-then-overlays-then-indicator pass order — is the headless
    /// renderer's behavior exactly (see [`HeadlessTileRenderer`'s frame
    /// documentation][`crate::headless::HeadlessTileRenderer::render_captured_frame_layers`]);
    /// differences are only the attachment and its outcome: the frame draws
    /// into the next acquired surface texture, presented instead of
    /// captured.
    ///
    /// Returns the frame's [`SurfaceFrameStatistics`] — the scheduler
    /// readiness verdicts and the overlay triangles the presented frame
    /// carried.
    ///
    /// Requires a prior [`Self::configure_extent`]; the frame's viewport
    /// must match the configured extent exactly (pixels are physical). An
    /// empty frame request set, an empty overlay list, and a cleared
    /// indicator naturally render the clear color alone.
    pub fn render_frame_layers(
        &mut self,
        camera: &MapCamera,
        viewport: &ViewportPixels,
        prepared_tiles: &[PreparedTile],
        pool: &mut TileTexturePool,
        overlays: &[MapOverlay],
        location_indicator_layer: Option<&crate::location_indicator::LocationIndicatorLayer>,
    ) -> Result<SurfaceFrameStatistics, SurfaceRenderError> {
        self.render_frame_layers_and_labels(
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
    /// indicator — and presents it. A `None` or empty label layer contributes
    /// no buffer or draw, leaving the frame identical to the unlabeled
    /// pipeline's.
    pub fn render_frame_with_labels(
        &mut self,
        camera: &MapCamera,
        viewport: &ViewportPixels,
        prepared_tiles: &[PreparedTile],
        pool: &mut TileTexturePool,
        label_layer: Option<&LabelLayer>,
    ) -> Result<SurfaceFrameStatistics, SurfaceRenderError> {
        self.render_frame_layers_and_labels(
            camera,
            viewport,
            prepared_tiles,
            pool,
            &[],
            None,
            label_layer,
        )
    }

    /// The frame's full four-layer variant with overlays supplied as a single
    /// list: a delegating wrapper over
    /// [`Self::render_frame_layers_and_labels_with_overlay_layers`] with no
    /// frame-level screen scale.
    // One argument per frame layer keeps the draw order explicit at the call
    // site; the earlier three-layer variant stays as a delegating wrapper.
    #[allow(clippy::too_many_arguments)]
    pub fn render_frame_layers_and_labels(
        &mut self,
        camera: &MapCamera,
        viewport: &ViewportPixels,
        prepared_tiles: &[PreparedTile],
        pool: &mut TileTexturePool,
        overlays: &[MapOverlay],
        location_indicator_layer: Option<&crate::location_indicator::LocationIndicatorLayer>,
        label_layer: Option<&LabelLayer>,
    ) -> Result<SurfaceFrameStatistics, SurfaceRenderError> {
        self.render_frame_layers_and_labels_with_overlay_layers(
            camera,
            viewport,
            prepared_tiles,
            pool,
            &[overlays],
            UNSCALED_DEVICE_PIXEL_RATIO,
            location_indicator_layer,
            label_layer,
        )
    }

    /// The frame's full four-layer variant, composing several ordered overlay
    /// layers and scaling their declared point/CSS screen sizes by
    /// `overlay_device_pixel_ratio` at emission. Pooled tile quads come first,
    /// then the composed overlays, then the label layer's halo and text
    /// passes (drawn over the overlays through the textured glyph pipeline),
    /// then the location indicator. An absent or empty label layer creates no
    /// buffer, no atlas upload and no draw, so frames without labels stay
    /// identical to the earlier pipeline's.
    ///
    /// Passing the layers and the ratio separately lets a caller with a
    /// device-pixel-ratio surface draw its overlay list unchanged: the frame
    /// scales the screen sizes while assembling instead of cloning a
    /// pre-scaled copy of the list.
    // One argument per frame layer keeps the draw order explicit at the call
    // site; the earlier three-layer variant stays as a delegating wrapper.
    #[allow(clippy::too_many_arguments)]
    pub fn render_frame_layers_and_labels_with_overlay_layers(
        &mut self,
        camera: &MapCamera,
        viewport: &ViewportPixels,
        prepared_tiles: &[PreparedTile],
        pool: &mut TileTexturePool,
        overlay_layers: &[&[MapOverlay]],
        overlay_device_pixel_ratio: f64,
        location_indicator_layer: Option<&crate::location_indicator::LocationIndicatorLayer>,
        label_layer: Option<&LabelLayer>,
    ) -> Result<SurfaceFrameStatistics, SurfaceRenderError> {
        self.render_frame_layers_and_labels_with_retained_level(
            camera,
            viewport,
            prepared_tiles,
            pool,
            overlay_layers,
            overlay_device_pixel_ratio,
            location_indicator_layer,
            label_layer,
            None,
        )
    }

    /// The frame's full variant with a retired tile zoom level drawn over the
    /// requested one. A zoom that crosses a tile level keeps the previous
    /// level's pooled tiles on screen — projected with the current camera, so
    /// they scale into place — and dissolves them into the new level as its
    /// tiles arrive. The retained quads draw after the requested level's, so
    /// a full-opacity hold covers the new level exactly and the fade is an
    /// alpha dissolve rather than a flash of background.
    #[allow(clippy::too_many_arguments)]
    pub fn render_frame_layers_and_labels_with_retained_level(
        &mut self,
        camera: &MapCamera,
        viewport: &ViewportPixels,
        prepared_tiles: &[PreparedTile],
        pool: &mut TileTexturePool,
        overlay_layers: &[&[MapOverlay]],
        overlay_device_pixel_ratio: f64,
        location_indicator_layer: Option<&crate::location_indicator::LocationIndicatorLayer>,
        label_layer: Option<&LabelLayer>,
        retained_tile_level: Option<crate::renderer::RetainedTileLevel>,
    ) -> Result<SurfaceFrameStatistics, SurfaceRenderError> {
        let Some((configured_width_pixels, configured_height_pixels)) =
            self.configured_surface_extent()
        else {
            return Err(SurfaceRenderError::SurfaceConfigurationFailed(
                "the surface is not configured; call configure_extent \
                 before rendering frames"
                    .to_string(),
            ));
        };
        let viewport_extent = (viewport.width_pixels as u32, viewport.height_pixels as u32);
        if viewport_extent != (configured_width_pixels, configured_height_pixels) {
            return Err(SurfaceRenderError::ViewportExtentMismatch {
                viewport_extent,
                configured_extent: (configured_width_pixels, configured_height_pixels),
            });
        }

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
            return Err(SurfaceRenderError::DrawListTooLarge {
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
                Some("rustic-maps-rendering-surface-tile-vertices"),
            )
        });
        let index_buffer = (!fragment_vertices.is_empty()).then(|| {
            create_filled_buffer(
                &device,
                &queue,
                &index_bytes,
                wgpu::BufferUsages::INDEX,
                Some("rustic-maps-rendering-surface-tile-indices"),
            )
        });

        // The overlay layer assembles through the same camera pose as the
        // tile quads and draws after them; the composed layers' screen sizes
        // scale at emission, and no layer creates no buffer and no draw.
        let overlay_vertices = overlay_triangle_vertices_for_layers(
            camera,
            viewport,
            self.quad_composer.field_of_view_degrees(),
            overlay_layers,
            overlay_device_pixel_ratio,
        );
        let overlay_vertex_buffer = (!overlay_vertices.is_empty()).then(|| {
            create_filled_buffer(
                &device,
                &queue,
                &encoded_overlay_vertices(&overlay_vertices),
                wgpu::BufferUsages::VERTEX,
                Some("rustic-maps-rendering-surface-overlay-vertices"),
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
                Some("rustic-maps-rendering-surface-indicator-vertices"),
            )
        });

        // The label layer assembles its halo and text quads GPU-free through
        // the same camera pose; an absent or empty layer creates no buffers
        // and no glyph atlas upload.
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
                Some("rustic-maps-rendering-surface-label-halo-vertices"),
            )
        });
        let text_vertex_buffer = (!label_quad_vertices.text.is_empty()).then(|| {
            create_filled_buffer(
                &device,
                &queue,
                &encoded_glyph_vertices(&label_quad_vertices.text),
                wgpu::BufferUsages::VERTEX,
                Some("rustic-maps-rendering-surface-label-text-vertices"),
            )
        });
        // A route-reference badge draws as solid triangles beneath its own
        // text; an unbadged frame creates no buffer and draws nothing.
        let badge_vertex_buffer = (!label_quad_vertices.badge.is_empty()).then(|| {
            create_filled_buffer(
                &device,
                &queue,
                &encoded_overlay_vertices(&label_quad_vertices.badge),
                wgpu::BufferUsages::VERTEX,
                Some("rustic-maps-rendering-surface-label-badge-vertices"),
            )
        });

        // The icon pass draws the placed icons' sprite quads through the tile
        // pipeline with the sprite atlas bound; no atlas or no icon draws
        // nothing and leaves a text-only frame byte-identical.
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
                Some("rustic-maps-rendering-surface-icon-vertices"),
            )
        });
        let icon_index_buffer = (!icon_quad_vertices.is_empty()).then(|| {
            create_filled_buffer(
                &device,
                &queue,
                &icon_index_bytes,
                wgpu::BufferUsages::INDEX,
                Some("rustic-maps-rendering-surface-icon-indices"),
            )
        });

        let surface_texture = self.acquire_surface_texture()?;
        let frame_texture_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut command_encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("rustic-maps-rendering-surface-frame-encoder"),
        });
        {
            let mut render_pass = command_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("rustic-maps-rendering-surface-frame-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &frame_texture_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(self.clear_color[0]),
                            g: f64::from(self.clear_color[1]),
                            b: f64::from(self.clear_color[2]),
                            a: f64::from(self.clear_color[3]),
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
        // The frame's commands are encoded against the acquired texture
        // already owned exclusively by the present queue; submitting and
        // presenting them is the composition's only observable outcome.
        queue.submit([command_encoder.finish()]);
        self.graphics_context.queue.present(surface_texture);
        Ok(
            SurfaceFrameStatistics::from_frame_readiness_and_overlay_vertices(
                prepared_tiles,
                overlay_vertices.len(),
            ),
        )
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
    ///
    /// Verbatim mirror of the headless renderer's private upload path; only
    /// the uploaded texture format differs ([`TILE_TEXTURE_FORMAT`], plain
    /// RGBA8, instead of the headless capture's frame format — the two are
    /// the same format there only because headless captures in RGBA8 too).
    fn upload_tile_texture(
        &mut self,
        device: &Device,
        tile_key: crate::tile_key::TileKey,
        pool: &mut TileTexturePool,
    ) -> Result<Option<(Texture, wgpu::BindGroup)>, SurfaceRenderError> {
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
    /// count feeds the cache budget as the texture's GPU cost. Verbatim
    /// mirror of the headless renderer's private helper, against
    /// [`TILE_TEXTURE_FORMAT`].
    fn upload_decoded_texture(
        &self,
        device: &Device,
        decoded_image: DynamicImage,
    ) -> (Texture, wgpu::BindGroup, u64) {
        let rgba_image = to_rgba8(decoded_image);
        let rgba_bytes = rgba_image.as_raw();
        let (width_pixels, height_pixels) = rgba_image.dimensions();

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("rustic-maps-rendering-surface-tile-texture"),
            size: Extent3d {
                width: width_pixels,
                height: height_pixels,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TILE_TEXTURE_FORMAT,
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
            label: Some("rustic-maps-rendering-surface-glyph-atlas-texture"),
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
            label: Some("rustic-maps-rendering-surface-glyph-atlas-bind-group"),
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

    /// Uploads the icon pass's sprite sheet as an RGBA texture when the
    /// atlas' content revision changed, and reuses the cached bind group
    /// otherwise. The sheet draws through the tile pipeline with the sprite
    /// atlas bound instead of a tile texture.
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
            label: Some("rustic-maps-rendering-surface-sprite-atlas-texture"),
            size: Extent3d {
                width: sprite_atlas.width_pixels(),
                height: sprite_atlas.height_pixels(),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TILE_TEXTURE_FORMAT,
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
            label: Some("rustic-maps-rendering-surface-sprite-atlas-bind-group"),
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
            label: Some("rustic-maps-rendering-surface-tile-bind-group"),
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
/// the most common PNG color model. Verbatim mirror of the headless
/// renderer's private helper.
fn to_rgba8(decoded_image: DynamicImage) -> image::RgbaImage {
    match decoded_image {
        DynamicImage::ImageRgba8(rgba_image) => rgba_image,
        other => other.to_rgba8(),
    }
}

/// Serializes fragment vertices into the vertex and index byte streams the GPU
/// buffers expect: little-endian floats, [`CORNERS_PER_QUAD`] corners per
/// world-copy fragment, and the two-triangle index pattern `0, 1, 2, 2, 1, 3`
/// ([`INDICES_PER_QUAD`] per fragment). Verbatim mirror of the headless
/// renderer's private helper.
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

/// Serializes the overlay triangle-list vertices into the vertex byte stream
/// the GPU buffer expects: little-endian floats. The overlay draws
/// non-indexed (a plain vertex draw over the flat triangle list), so no index
/// stream exists for this layer. Verbatim mirror of the headless renderer's
/// private helper.
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

fn encoded_overlay_vertices(overlay_vertices: &[ClipSpaceOverlayVertex]) -> Vec<u8> {
    let mut vertex_bytes = Vec::with_capacity(std::mem::size_of_val(overlay_vertices));
    for vertex in overlay_vertices {
        for component in vertex.clip_position.iter().chain(vertex.rgba_color.iter()) {
            vertex_bytes.extend_from_slice(&component.to_le_bytes());
        }
    }
    vertex_bytes
}

/// Creates a device buffer pre-filled from host bytes. Verbatim mirror of the
/// headless renderer's private helper.
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

/// Picks the surface format the pipelines render in: the first entry of the
/// documented [`SURFACE_FORMAT_PREFERENCE`] the adapter's surface actually
/// supports, or `None` when none is (the caller turns that into a
/// configuration failure). Kept GPU-free so the preference order is
/// unit-testable without an adapter.
fn preferred_surface_format(
    supported_formats: &[wgpu::TextureFormat],
) -> Option<wgpu::TextureFormat> {
    SURFACE_FORMAT_PREFERENCE
        .iter()
        .find(|preferred_format| supported_formats.contains(preferred_format))
        .copied()
}

/// Negotiates the configured alpha mode: the preferred opaque compositing
/// when the surface supports it, otherwise the first mode the platform
/// reports. Mirrors `preferred_surface_format`'s prefer-then-negotiate
/// pattern — a hardware-backed surface, unlike a web canvas, is free to
/// restrict the compositing modes it accepts, and refusing such a surface
/// (`Android emulator`) would render the SDK on none of that backend's own
/// listed surfaces.
fn negotiated_alpha_mode(
    surface: &wgpu::Surface<'static>,
    adapter: &wgpu::Adapter,
) -> Result<wgpu::CompositeAlphaMode, SurfaceRenderError> {
    let surface_capabilities = surface.get_capabilities(adapter);
    if surface_capabilities
        .alpha_modes
        .contains(&PREFERRED_ALPHA_MODE)
    {
        return Ok(PREFERRED_ALPHA_MODE);
    }
    surface_capabilities
        .alpha_modes
        .first()
        .copied()
        .ok_or_else(|| {
            SurfaceRenderError::SurfaceConfigurationFailed(
                "the surface supports no alpha mode at all, which no wgpu \
             backend reports; refusing to configure"
                    .to_string(),
            )
        })
}

/// Builds the textured-quad pipeline of the tile layer against a target
/// surface format. Verbatim mirror of the headless pipeline builder; only
/// the target format and the labels differ.
fn build_tile_pipeline(
    graphics_context: &SurfaceGraphicsContext,
    surface_format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let shader_module =
        graphics_context
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("rustic-maps-rendering-surface-tile-quad-shader"),
                source: wgpu::ShaderSource::Wgsl(TILE_QUAD_SHADER_SOURCE.into()),
            });

    let bind_group_layout =
        graphics_context
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("rustic-maps-rendering-surface-tile-bind-group-layout"),
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
                offset: std::mem::offset_of!(ClipSpaceTileVertex, opacity) as wgpu::BufferAddress,
                shader_location: 2,
            },
        ],
    };

    let pipeline_layout =
        graphics_context
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("rustic-maps-rendering-surface-tile-pipeline-layout"),
                bind_group_layouts: &[Some(&bind_group_layout)],
                immediate_size: 0,
            });

    graphics_context
        .device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rustic-maps-rendering-surface-tile-pipeline"),
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
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        })
}

/// Builds the solid-color pipeline of the overlay layer against a target
/// surface format — the headless overlay pipeline with alpha blending,
/// labeled for the surface path.
fn build_overlay_pipeline(
    graphics_context: &SurfaceGraphicsContext,
    surface_format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let overlay_shader_module =
        graphics_context
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("rustic-maps-rendering-surface-overlay-shader"),
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
                label: Some("rustic-maps-rendering-surface-overlay-pipeline-layout"),
                bind_group_layouts: &[],
                immediate_size: 0,
            });

    graphics_context
        .device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rustic-maps-rendering-surface-overlay-pipeline"),
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
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        })
}

/// Builds the textured-glyph pipeline of the label layer against a target
/// surface format: the headless glyph pipeline's shader and vertex layout
/// verbatim, with standard alpha blending so halos and text composite over
/// the base.
fn build_glyph_pipeline(
    graphics_context: &SurfaceGraphicsContext,
    surface_format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let glyph_shader_module =
        graphics_context
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("rustic-maps-rendering-surface-glyph-atlas-shader"),
                source: wgpu::ShaderSource::Wgsl(GLYPH_ATLAS_SHADER_SOURCE.into()),
            });

    let glyph_bind_group_layout =
        graphics_context
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("rustic-maps-rendering-surface-glyph-atlas-bind-group-layout"),
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
                label: Some("rustic-maps-rendering-surface-glyph-atlas-pipeline-layout"),
                bind_group_layouts: &[Some(&glyph_bind_group_layout)],
                immediate_size: 0,
            });

    graphics_context
        .device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rustic-maps-rendering-surface-glyph-atlas-pipeline"),
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
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        })
}

/// Builds the glyph atlas sampler of the surface renderer: clamp addressing
/// and linear filtering over the `R8Unorm` page.
fn build_glyph_sampler(graphics_context: &SurfaceGraphicsContext) -> wgpu::Sampler {
    graphics_context
        .device
        .create_sampler(&wgpu::SamplerDescriptor {
            label: Some("rustic-maps-rendering-surface-glyph-atlas-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: GLYPH_ATLAS_TEXTURE_FILTER,
            min_filter: GLYPH_ATLAS_TEXTURE_FILTER,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        })
}

/// Builds the tile sampler of the surface renderer: clamp addressing and
/// linear filtering, the headless sampler's modes verbatim.
fn build_tile_sampler(graphics_context: &SurfaceGraphicsContext) -> wgpu::Sampler {
    graphics_context
        .device
        .create_sampler(&wgpu::SamplerDescriptor {
            label: Some("rustic-maps-rendering-surface-tile-sampler"),
            address_mode_u: TILE_TEXTURE_ADDRESS_MODE,
            address_mode_v: TILE_TEXTURE_ADDRESS_MODE,
            address_mode_w: TILE_TEXTURE_ADDRESS_MODE,
            mag_filter: TILE_TEXTURE_FILTER,
            min_filter: TILE_TEXTURE_FILTER,
            mipmap_filter: TILE_TEXTURE_MIPMAP_FILTER,
            ..Default::default()
        })
}
#[cfg(test)]
mod tests {
    use super::{
        SurfaceFrameStatistics, encoded_fragment_buffers, encoded_overlay_vertices,
        preferred_surface_format, to_rgba8,
    };
    use crate::overlay::{ClipSpaceOverlayVertex, DEFAULT_FILLED_POLYGON_RGBA};
    use crate::renderer::ClipSpaceTileVertex;
    use crate::tile_key::TileKey;
    use crate::tile_scheduler::{PreparedTile, TileReadiness};
    use wgpu::TextureFormat;

    #[test]
    fn frame_statistics_classify_every_readiness_and_count_triangles() {
        let requested_tiles = [
            PreparedTile {
                requested_tile_key: TileKey::new(0, 0, 14).expect("a valid tile key"),
                readiness: TileReadiness::Present,
            },
            PreparedTile {
                requested_tile_key: TileKey::new(1, 0, 14).expect("a valid tile key"),
                readiness: TileReadiness::FallsBackTo {
                    fallback_tile_key: TileKey::new(0, 0, 13).expect("a valid tile key"),
                    zoom_level_difference: 1,
                },
            },
            PreparedTile {
                requested_tile_key: TileKey::new(2, 0, 14).expect("a valid tile key"),
                readiness: TileReadiness::Deferred,
            },
            PreparedTile {
                requested_tile_key: TileKey::new(3, 0, 14).expect("a valid tile key"),
                readiness: TileReadiness::Unavailable,
            },
        ];
        // Two overlay triangles arrive as a flat six-vertex stream.
        let statistics = SurfaceFrameStatistics::from_frame_readiness_and_overlay_vertices(
            &requested_tiles,
            crate::overlay::OVERLAY_TRIANGLE_VERTEX_COUNT * 2,
        );
        assert_eq!(statistics.requested_tile_count, 4);
        assert_eq!(statistics.present_tile_count, 1);
        assert_eq!(statistics.fallback_tile_count, 1);
        assert_eq!(statistics.deferred_tile_count, 1);
        assert_eq!(statistics.unavailable_tile_count, 1);
        assert_eq!(statistics.overlay_triangle_count, 2);
        // A partial stream (a straddling fragment mid-assembly) is never
        // rounded up: only whole triangles of the presented frame count.
        let partial = SurfaceFrameStatistics::from_frame_readiness_and_overlay_vertices(
            &requested_tiles,
            crate::overlay::OVERLAY_TRIANGLE_VERTEX_COUNT + 1,
        );
        assert_eq!(partial.overlay_triangle_count, 1);
        let empty = SurfaceFrameStatistics::from_frame_readiness_and_overlay_vertices(&[], 0);
        assert_eq!(
            empty,
            SurfaceFrameStatistics {
                requested_tile_count: 0,
                present_tile_count: 0,
                fallback_tile_count: 0,
                deferred_tile_count: 0,
                unavailable_tile_count: 0,
                overlay_triangle_count: 0,
            }
        );
    }

    /// Formats an offscreen Metal back-end advertises, plus the two
    /// preferred ones somewhere in the middle: the preference list must pick
    /// its first supported entry, regardless of position.
    fn metal_like_supported_formats() -> Vec<TextureFormat> {
        vec![
            TextureFormat::Rgba16Float,
            TextureFormat::Bgra8Unorm,
            TextureFormat::Rgba8Unorm,
        ]
    }

    #[test]
    fn the_first_preferred_supported_format_wins() {
        assert_eq!(
            preferred_surface_format(&metal_like_supported_formats()),
            Some(TextureFormat::Bgra8Unorm)
        );
        assert_eq!(
            preferred_surface_format(&[TextureFormat::Bgra8Unorm]),
            Some(TextureFormat::Bgra8Unorm)
        );
    }

    #[test]
    fn an_unsupported_surface_reports_no_formats_left() {
        assert_eq!(
            preferred_surface_format(&[TextureFormat::Rgba16Float]),
            None,
            "a surface offering only non-preferred formats cannot render \
             this pipeline's non-sRGB color contract"
        );
    }

    #[test]
    fn the_decode_normalization_keeps_plain_rgba8_without_a_copy() {
        // The normalized decode of an already-RGBA8 image returns the same
        // pixel data unchanged.
        let decoded_image = image::DynamicImage::ImageRgba8(
            image::RgbaImage::from_raw(2, 1, vec![1, 2, 3, 4, 5, 6, 7, 8])
                .expect("the buffer matches the dimensions"),
        );
        let normalized_image = to_rgba8(decoded_image);
        assert_eq!(normalized_image.as_raw(), &vec![1, 2, 3, 4, 5, 6, 7, 8]);
        // A non-RGBA8 decode normalizes into RGBA8 instead of failing.
        let grayscale_normalized = to_rgba8(image::DynamicImage::ImageLuma8(
            image::GrayImage::from_raw(1, 1, vec![7]).expect("the buffer matches the dimensions"),
        ));
        assert_eq!(grayscale_normalized.as_raw(), &vec![7, 7, 7, 255]);
    }

    #[test]
    fn encoded_fragments_serialize_corners_and_their_index_pattern() {
        let fragment_vertices = vec![[
            ClipSpaceTileVertex {
                clip_position: [-1.0, -1.0, 0.0, 1.0],
                texture_coordinates: [0.0, 1.0],
                opacity: 1.0,
            },
            ClipSpaceTileVertex {
                clip_position: [1.0, -1.0, 0.0, 1.0],
                texture_coordinates: [1.0, 1.0],
                opacity: 1.0,
            },
            ClipSpaceTileVertex {
                clip_position: [-1.0, 1.0, 0.0, 1.0],
                texture_coordinates: [0.0, 0.0],
                opacity: 1.0,
            },
            ClipSpaceTileVertex {
                clip_position: [1.0, 1.0, 0.0, 1.0],
                texture_coordinates: [1.0, 0.0],
                opacity: 1.0,
            },
        ]];
        let (vertex_bytes, index_bytes) = encoded_fragment_buffers(&fragment_vertices);
        // One fragment, four corners at 28 bytes (7 floats) each.
        assert_eq!(vertex_bytes.len(), 112);
        assert_eq!(index_bytes, vec![0, 0, 1, 0, 2, 0, 2, 0, 1, 0, 3, 0]);
    }

    #[test]
    fn encoded_fragments_bound_the_index_range_to_the_corner_count() {
        let corner = ClipSpaceTileVertex {
            clip_position: [0.0; 4],
            texture_coordinates: [0.0; 2],
            opacity: 1.0,
        };
        let fragment_vertices = [[corner; 4], [corner; 4]];
        let (_, index_bytes) = encoded_fragment_buffers(&fragment_vertices);
        // The second fragment's first index continues the buffer-wide
        // corner numbering at 4.
        assert_eq!(&index_bytes[12..24], &[4, 0, 5, 0, 6, 0, 6, 0, 5, 0, 7, 0]);
    }

    #[test]
    fn encoded_overlay_vertices_serialize_position_then_color_flat() {
        let overlay_vertices = vec![ClipSpaceOverlayVertex {
            clip_position: [1.0, 2.0, 3.0, 4.0],
            rgba_color: DEFAULT_FILLED_POLYGON_RGBA,
        }];
        let vertex_bytes = encoded_overlay_vertices(&overlay_vertices);
        assert_eq!(vertex_bytes.len(), 32);
    }
}
