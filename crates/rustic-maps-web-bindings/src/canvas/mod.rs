//! WebGPU map canvas: renders recorded raster tiles into an
//! `HTMLCanvasElement` through `wgpu` on the browser.
//!
//! The page owns a [`BrowserMapView`] built over its own canvas element. Every
//! [`BrowserMapView::render_frame`] call drives one complete frame: the bound
//! camera lists its visible tiles, the [`rustic_maps_rendering::TileScheduler`]
//! resolves missing ones from the same-origin `/tiles` route (the local
//! static server exposing the recorded platform tile fixtures — the browser
//! never fetches an external network), the GPU-free
//! [`rustic_maps_rendering::Renderer`] assembles viewport-culled quads and projects
//! them to clip space in `f64`, and a `wgpu` surface pass draws them.
//!
//! The `wgpu` half compiles only on wasm32 and mirrors the native
//! `rustic-maps-rendering` headless pipeline (which this crate cannot reuse because
//! that module is `cfg`-gated out on wasm): same WGSL text, same vertex
//! layout, same two-triangle index pattern, and the same degradation rules —
//! an undecodable pooled payload skips its quads for the frame and is
//! evicted from the pool so the scheduler refetches it, while only
//! structural failures (adapter/device acquisition, an overflowing draw
//! list, a failed surface acquisition) surface as errors.
//!
//! Frames carry the overlay layer through a cached world-space tessellation:
//! the overlay vocabulary ([`rustic_maps_rendering::MapOverlay`]) is tessellated once
//! per content revision into origin-relative normalized-world geometry
//! ([`rustic_maps_rendering::WorldSpaceOverlayGeometry`]), uploaded once, and drawn
//! every frame with a small uniform carrying the view-projection, viewport
//! and world-copy placement. The vertex shader expands line widths, joins,
//! markers and pins in screen pixels — the same math the clip-space assembly
//! used — so panning, rotation and zoom neither re-tessellate nor re-upload
//! the layer. The overlays draw after the tile quads in the same render pass
//! through their own solid-color pipeline (same alpha blending). The page
//! supplies the overlay list as
//! plain JavaScript descriptor objects through
//! [`BrowserMapView::set_overlays`] (the bulk call) or one at a time through
//! [`BrowserMapView::add_overlay`], which answers an identifier the page can
//! [`BrowserMapView::update_overlay`] and [`BrowserMapView::remove_overlay`] by; it clears
//! everything through [`BrowserMapView::clear_overlays`]. Every invalid descriptor
//! is a coded rejection, never a panic.
//!
//! The location indicator rides the exact same seam (the native surface's
//! top layer): the page supplies one fix-and-style descriptor pair through
//! [`BrowserMapView::set_location_indicator`] and clears it through
//! [`BrowserMapView::clear_location_indicator`]; [`BrowserMapView::render_frame`]
//! assembles its clip-space triangles with
//! [`rustic_maps_rendering::location_indicator_vertices`] at the same camera pose and
//! draws them through the same solid-color pipeline, after the overlays. The
//! indicator's vertices contribute nothing to the frame statistics, mirroring
//! the native statistics' overlay-only count.
//!
//! Nothing on the JavaScript-facing paths may panic: every fallible step
//! maps to a coded `JsValue` error. The interior draw state sits in
//! a `RefCell` because `wasm-bindgen` async export methods borrow `self`
//! across their futures; the page must never overlap two `render_frame`
//! futures, so it serializes its render queue behind a busy flag.

use crate::contract_field_names::*;
use crate::exports::{
    ERROR_CODE_INVALID_ARGUMENT, ERROR_CODE_INVALID_COORDINATE_INPUT,
    ERROR_CODE_INVALID_RENDER_INPUT, ERROR_CODE_NETWORK_REQUEST_FAILED,
    ERROR_CODE_PROVIDER_NOT_CONFIGURED, ERROR_CODE_RENDERING_SURFACE_UNAVAILABLE,
    ERROR_CODE_RESPONSE_PARSING_FAILURE, ERROR_CODE_UNSUPPORTED_CAPABILITY, MapCamera,
    failure_with_code, object_from_fields, sdk_error_failure,
};
use crate::live_providers::{
    registered_raster_tile_provider, registered_vector_style_resource_hosts,
    registered_vector_style_resources, registered_vector_tile_provider, selected_provider_key,
};
use crate::shared::{
    MAXIMUM_RECORDED_TILE_ZOOM_LEVEL, clamped_surface_extent, tile_fetch_zoom_level,
    tile_fixture_url, validated_bounding_box, validated_coordinate, validated_viewport_pixels,
};
use crate::vector_source_fields::{
    SourceFieldValue, VectorSourceDeclarationFields, url_template_vector_source,
    vector_source_extras,
};
use crate::web_camera::{self, CameraStore, rotate_camera_clockwise};
use everymap_providers_url_template_vector::UrlTemplateVectorTileProvider;
use geospatial_traits::{RasterTileProvider, VectorTileSourceDescriptor};
use geospatial_types::{
    GeographicCoordinate, RasterImageFormat, RasterTileData, RasterTileResolution, SDKError,
    TrafficFlowCongestionLevel, TrafficFlowData,
};
use image::DynamicImage;
use js_sys::{Array, Object, Reflect, Uint8Array};
use rustic_maps_core::ProviderContract;
use rustic_maps_core::everymap_bridge::{
    EveryMapVectorTileProvider, map_provider_error, tile_source_budget_rates,
};
use rustic_maps_rendering::renderer::ClipSpaceTileVertex;
use rustic_maps_rendering::vector_base::{
    DEFAULT_VECTOR_BASE_VISIBLE_PREFETCH_BURST_TILE_COUNT,
    DEFAULT_VECTOR_TILE_MEMORY_CACHE_MAXIMUM_TILE_COUNT,
    DEFAULT_VECTOR_TILE_PAYLOAD_TIME_TO_LIVE_SECONDS, VectorBaseFrame, VectorBaseScheduler,
    VectorSourceLayerCount, VectorTileMemoryCache,
};
use rustic_maps_rendering::{
    CLEAR_COLOR_RGBA, ClipSpaceGlyphVertex, ClipSpaceOverlayVertex,
    DEFAULT_LOCATION_DISC_RADIUS_PIXELS, EdgeInsetsPixels, FilledPolygonOverlay, HostedGlyphRanges,
    LabelFont, LocationIndicatorLayer, LocationIndicatorState, LocationIndicatorStyle,
    LocationIndicatorVisualKind, MapOverlay, OVERLAY_TRIANGLE_VERTEX_COUNT, PlacedLabel,
    PointMarkersOverlay, PointPinsOverlay, Renderer, RoutePolylineOverlay, SpriteAtlas, TileKey,
    TileReadiness, TileScheduler, TileTexturePool, TrafficFlowOverlay,
    WORLD_SPACE_OVERLAY_WORLD_COPY_INSTANCES, WorldSpaceOverlayFrameUniforms,
    WorldSpaceOverlayGeometry, WorldSpaceOverlayVertex, ZoomTransition, fetch_hosted_sprite_atlas,
    location_indicator_vertices, overlay_triangle_vertices, overlays_require_camera_space_strokes,
    requested_level_ready_tile_fraction,
};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::sync::{Arc, Mutex, PoisonError};
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{HtmlCanvasElement, Response};
use wgpu::{
    BindGroup, Buffer, CurrentSurfaceTexture, Extent3d, Surface, SurfaceTexture,
    TexelCopyBufferInfo, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture, TextureAspect,
    TextureFormat, TextureUsages,
};

mod draw;
mod fixtures;
mod javascript;
mod shader;
mod statistics;
mod tile_fetch;
mod vector_base;

use self::draw::*;
use self::fixtures::*;
use self::javascript::*;
use self::shader::*;
use self::statistics::*;
use self::tile_fetch::*;
use self::vector_base::*;

/// GPU format of the uploaded tile textures. The recorded tile PNGs are
/// decoded to plain RGBA8, so no sRGB conversion enters the pipeline and
/// colors round-trip byte-for-byte onto the non-sRGB surface formats.
const TILE_TEXTURE_FORMAT: TextureFormat = TextureFormat::Rgba8Unorm;

/// Canvas formats the renderer accepts, most preferred first. Both are
/// non-sRGB, so the tile texel values pass through unchanged to the screen.
const SURFACE_FORMAT_PREFERENCE: [TextureFormat; 2] =
    [TextureFormat::Bgra8Unorm, TextureFormat::Rgba8Unorm];

/// Bytes in one RGBA8 texel, of both tile pixels and surface pixels.
const PIXEL_BYTES: u32 = 4;

/// The vertical field of view the programmatic canvas camera verbs use: the
/// same perspective the native wrappers hand the contract's camera members.
const DEFAULT_FIELD_OF_VIEW_DEGREES: f64 = 60.0;

/// The `widthPixels` field of the viewport extent readback.
const VIEWPORT_SIZE_FIELD_WIDTH_PIXELS: &str = "widthPixels";

/// The `heightPixels` field of the viewport extent readback.
const VIEWPORT_SIZE_FIELD_HEIGHT_PIXELS: &str = "heightPixels";

/// Filter mode of the tile sampler: linear, giving smooth minification and
/// smoothly-upscaled recorded texels, mirroring the native headless renderer.
const TILE_TEXTURE_FILTER: wgpu::FilterMode = wgpu::FilterMode::Linear;

/// Interpolation between mip levels. The uploaded textures carry no mip
/// chain, so the mode never applies; linear keeps the sampler consistent
/// if mip chains are added.
const TILE_TEXTURE_MIPMAP_FILTER: wgpu::MipmapFilterMode = wgpu::MipmapFilterMode::Linear;

/// Clamp sampler addressing: quads sample exactly their declared sub-window
/// of the texture, so edge behavior never triggers.
const TILE_TEXTURE_ADDRESS_MODE: wgpu::AddressMode = wgpu::AddressMode::ClampToEdge;

/// GPU format of the glyph atlas: one signed-distance byte per pixel, which
/// the label shader thresholds into text coverage. Mirror of the native
/// `rustic-maps-rendering` glyph renderer's format.
const GLYPH_ATLAS_TEXTURE_FORMAT: TextureFormat = TextureFormat::R8Unorm;

/// Bytes in one `R8Unorm` atlas pixel.
const GLYPH_ATLAS_PIXEL_BYTES: u32 = 1;

/// Filter mode of the glyph atlas sampler: linear interpolation keeps the
/// signed-distance contour smooth when the style's text size differs from the
/// atlas' 24-pixel em.
const GLYPH_ATLAS_TEXTURE_FILTER: wgpu::FilterMode = wgpu::FilterMode::Linear;

/// Draw quads a frame may carry, where a world-copy fragment counts as one
/// draw quad: the bound keeps the fragment vertices (four corners each) and
/// the draw list inside a 16-bit index buffer, mirroring the native
/// headless renderer.
const MAXIMUM_QUADS_PER_FRAME: usize = u16::MAX as usize / CORNERS_PER_QUAD;

/// Indices consumed by one drawn quad: the two triangles of the corner
/// pattern `0, 1, 2, 2, 1, 3`.
const INDICES_PER_QUAD: usize = 6;

/// Corners of one drawn quad: the four vertices one world-copy fragment
/// contributes.
const CORNERS_PER_QUAD: usize = 4;

/// Byte budget of the browser tile payload pool. One recorded 256x256 PNG
/// tile is tens of kilobytes encoded, so the pool comfortably holds the
/// recorded fixtures of a far wider neighborhood than the default view.
const TILE_PAYLOAD_POOL_MAXIMUM_BYTES: u64 = 64 << 20;

/// Missing tiles the render frame fetches from the same-origin `/tiles`
/// route per frame; the rest report `Unavailable` and resolve on a later
/// frame.
const MAXIMUM_TILE_FETCHES_PER_FRAME: usize = 32;

/// The DOM event the canvas dispatches — on the canvas element itself — when
/// a prefetched tile payload lands. A page that renders on demand listens for
/// it and draws the frame that includes the new tile; the canvas owns no
/// render loop of its own.
const MAP_TILE_ARRIVED_EVENT_NAME: &str = "map-tile-arrived";

/// The tile-arrival coalescing window, in milliseconds. A visible prefetch
/// delivers one callback per tile; without coalescing a pan that fills fifteen
/// tiles schedules fifteen repaints. Every delivery inside this window folds
/// into one pending timeout, so a page repaints at most once per window while
/// still seeing the first arrival promptly.
const MAP_TILE_ARRIVAL_COALESCE_MILLISECONDS: i32 = 100;

/// How many tile fetches may run at once. A frame starts them without
/// waiting, so this is the whole network budget: enough to fill a new view in
/// one round of round trips, small enough that a pan does not open a hundred
/// sockets.
const MAXIMUM_CONCURRENT_TILE_FETCHES: usize = 32;

/// How many decoded tile payloads the staging cache keeps. A decoded
/// 512-pixel RGBA8 tile costs about a megabyte, so the cache is bounded well
/// above the visible tile count and well below the texture budget; it exists
/// to carry a payload from its staging decode to its upload, and clearing it
/// wholesale on overflow costs one decode, not correctness.
const MAXIMUM_CACHED_DECODED_TILE_PAYLOADS: usize = 64;

/// How many tiles a canvas remembers as having no fetchable source. At the
/// bound the memory clears wholesale: remembering every address of a long
/// wander must not grow without limit, and a forgotten address costs one
/// request, never correctness.
const MAXIMUM_UNAVAILABLE_TILE_KEYS: usize = 4096;

/// How many staged tile payloads the preload pass holds before clearing. A
/// staged payload is consumed by the next frame's fetch pass; the bound sits
/// far above the per-frame fetch budget, and an overflow costs a refetch.
const MAXIMUM_STAGED_TILE_PAYLOADS: usize = 256;

/// How long the first retry after a transient tile failure waits, doubled per
/// consecutive failure up to the cap: a struggling tile source is backed off,
/// not hammered every frame.
const TILE_FETCH_RETRY_BASE_DELAY_MILLISECONDS: f64 = 2_000.0;
/// The longest a transient tile failure defers its retry.
const TILE_FETCH_RETRY_MAXIMUM_DELAY_MILLISECONDS: f64 = 30_000.0;
/// How many tiles' retry schedules the canvas keeps before dropping them all;
/// the schedules are an optimization, never a correctness record.
const MAXIMUM_TRACKED_TILE_RETRY_SCHEDULES: usize = 1024;

/// The statuses a same-origin tile route answers when it has no recorded tile
/// for an address: "not found" and "gone" are definitive, so the tile is not
/// retried.
const HTTP_NOT_FOUND_STATUS_CODE: u16 = 404;
/// See [`HTTP_NOT_FOUND_STATUS_CODE`].
const HTTP_GONE_STATUS_CODE: u16 = 410;

/// FNV-1a's 64-bit offset basis and prime, used to fingerprint encoded tile
/// payloads (see [`fingerprint_encoded_bytes`]).
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Highest zoom level of the web-mercator tile grid itself (a guard on the
/// live source setting).
const MAXIMUM_TILE_GRID_ZOOM_LEVEL: u8 = 24;

/// Cached GPU tile textures kept while a camera moves. One cached 256x256
/// RGBA8 texture costs 256 KiB of GPU memory, so the cap bounds the browser
/// canvas' texture cache at the same order of magnitude as the native
/// headless one.
const MAXIMUM_CACHED_TILE_TEXTURES: usize = 256;

/// Desired frames in flight of the surface configuration: the pacing of a
/// small render-on-demand map canvas.
const DESIRED_MAXIMUM_FRAME_LATENCY: u32 = 2;

/// Whether the raster tile base layer starts visible: the canvas draws the
/// provider's raster tiles as the default base map until the page switches
/// its base layer through [`BrowserMapView::set_tile_layer_visible`].
const TILE_LAYER_VISIBLE_BY_DEFAULT: bool = true;

/// Whether the page serves the recorded PNG tile fixtures at the same-origin
/// `/tiles` route, which decides whether a raster fetch may probe that route
/// before falling through to a registered live source. The local dev server
/// and the test harness serve the fixtures; a deployed page stages none and
/// constructs the canvas with the capability off, so a raster request goes
/// straight to its provider instead of issuing one 404 per tile.
const RECORDED_TILE_FIXTURES_ARE_SERVED_BY_DEFAULT: bool = true;

/// How many times one render frame tries to acquire the surface texture,
/// reconfiguring between attempts; more attempts would fight a surface the
/// browser keeps tearing down.
const SURFACE_ACQUISITION_ATTEMPTS: usize = 2;

/// Name of the JavaScript field carrying point markers' anchor positions.
pub(crate) const OVERLAY_FIELD_POSITIONS: &str = "positions";
/// Name of the JavaScript field carrying a traffic-flow overlay's segments.
const OVERLAY_FIELD_SEGMENTS: &str = "segments";
/// Name of the JavaScript field carrying an overlay's color
/// override (`None` keeps the rustic-maps-rendering default).
pub(crate) const OVERLAY_FIELD_COLOR: &str = "color";
/// Name of the JavaScript field carrying a point marker's radius override in
/// screen pixels (`None` keeps the rustic-maps-rendering default).
pub(crate) const OVERLAY_FIELD_RADIUS_PIXELS: &str = "radiusPixels";
/// Name of the JavaScript `kind` value selecting a route polyline.
pub(crate) const OVERLAY_KIND_ROUTE_POLYLINE: &str = "routePolyline";
/// Name of the JavaScript `kind` value selecting a filled polygon.
pub(crate) const OVERLAY_KIND_FILLED_POLYGON: &str = "filledPolygon";
/// Name of the JavaScript `kind` value selecting point markers.
pub(crate) const OVERLAY_KIND_POINT_MARKERS: &str = "pointMarkers";
/// Name of the JavaScript `kind` value selecting teardrop point pins.
pub(crate) const OVERLAY_KIND_POINT_PINS: &str = "pointPins";
/// Name of the JavaScript `kind` value selecting a traffic-flow layer.
const OVERLAY_KIND_TRAFFIC_FLOW: &str = "trafficFlow";
/// Visual-kind value selecting the plain tracking dot.
const INDICATOR_KIND_TRACKING_DOT: &str = "trackingDot";
/// Visual-kind value selecting the guided-course arrow.
const INDICATOR_KIND_GUIDED_COURSE_ARROW: &str = "guidedCourseArrow";

/// Device-pixel ratio from which the page picks the retina tile
/// resolution: a denser backing store sharpens the sampled tile textures, and
/// the Tiles API serves its `@2x` double-texel form for the same logical tile
/// span. A 1.5× display — or a 2× screen at reduced browser zoom, where the
/// page reports about 1.6 — still gains visibly from the double-texel form,
/// while a 1× display keeps the standard tiles. Exposed through
/// [`retina_device_pixel_ratio_threshold`] so the page and the canvas never
/// disagree on the seam.
const RETINA_DEVICE_PIXEL_RATIO_THRESHOLD: f64 = 1.5;

/// Tile pixel resolution the GPU canvas fetches and samples, mirroring
/// [`geospatial_types::RasterTileResolution`] at the JavaScript seam: the
/// shared record type cannot itself carry `wasm-bindgen` attributes, so the
/// page passes this enum and the canvas converts it.
#[wasm_bindgen(js_name = TileResolution)]
pub enum CanvasTileResolution {
    /// One texel per logical tile pixel.
    Standard,
    /// Twice the texels per logical tile pixel (the recorded `@2x` fixtures).
    Retina,
}

impl From<CanvasTileResolution> for RasterTileResolution {
    fn from(canvas_tile_resolution: CanvasTileResolution) -> RasterTileResolution {
        match canvas_tile_resolution {
            CanvasTileResolution::Standard => RasterTileResolution::Standard,
            CanvasTileResolution::Retina => RasterTileResolution::Retina,
        }
    }
}

/// Congestion classification a traffic-flow segment paints with, mirroring
/// [`geospatial_types::TrafficFlowCongestionLevel`] at the JavaScript seam:
/// the shared record type cannot itself carry `wasm-bindgen` attributes, so
/// the page passes this enum and the canvas converts it (the same pattern as
/// [`CanvasTileResolution`]).
#[wasm_bindgen(js_name = TrafficFlowCongestionLevel)]
pub enum CanvasTrafficFlowCongestionLevel {
    /// At or above free-flow speed.
    FreeFlow,
    /// Moderately below free-flow speed.
    Slow,
    /// Stop-and-go traffic.
    Congested,
    /// Roadway effectively closed.
    Blocked,
}

impl From<CanvasTrafficFlowCongestionLevel> for TrafficFlowCongestionLevel {
    fn from(
        canvas_congestion_level: CanvasTrafficFlowCongestionLevel,
    ) -> TrafficFlowCongestionLevel {
        match canvas_congestion_level {
            CanvasTrafficFlowCongestionLevel::FreeFlow => TrafficFlowCongestionLevel::FreeFlow,
            CanvasTrafficFlowCongestionLevel::Slow => TrafficFlowCongestionLevel::Slow,
            CanvasTrafficFlowCongestionLevel::Congested => TrafficFlowCongestionLevel::Congested,
            CanvasTrafficFlowCongestionLevel::Blocked => TrafficFlowCongestionLevel::Blocked,
        }
    }
}

/// Wasm-boundary discriminant of [`CanvasTrafficFlowCongestionLevel::FreeFlow`]:
/// the level travels through a plain JavaScript descriptor object
/// (unlike a `wasm-bindgen`-typed parameter) as its plain numeric enum value,
/// so reading it back matches against the enum's own values, never literal
/// integers.
const CONGESTION_LEVEL_FREE_FLOW_DISCRIMINANT: u32 =
    CanvasTrafficFlowCongestionLevel::FreeFlow as u32;
/// Wasm-boundary discriminant of [`CanvasTrafficFlowCongestionLevel::Slow`].
const CONGESTION_LEVEL_SLOW_DISCRIMINANT: u32 = CanvasTrafficFlowCongestionLevel::Slow as u32;
/// Wasm-boundary discriminant of [`CanvasTrafficFlowCongestionLevel::Congested`].
const CONGESTION_LEVEL_CONGESTED_DISCRIMINANT: u32 =
    CanvasTrafficFlowCongestionLevel::Congested as u32;
/// Wasm-boundary discriminant of [`CanvasTrafficFlowCongestionLevel::Blocked`].
const CONGESTION_LEVEL_BLOCKED_DISCRIMINANT: u32 = CanvasTrafficFlowCongestionLevel::Blocked as u32;

/// Reports the device-pixel ratio from which the page's JavaScript
/// chooses between [`CanvasTileResolution::Retina`] and
/// [`CanvasTileResolution::Standard`]: at or above the threshold the page
/// passes retina. The threshold lives in Rust so the seam stays one source
/// of truth.
#[wasm_bindgen(js_name = devicePixelRatioThreshold)]
pub fn retina_device_pixel_ratio_threshold() -> f64 {
    RETINA_DEVICE_PIXEL_RATIO_THRESHOLD
}

/// Device-pixel ratio assumed when the page has no window (a worker or a
/// detached document): one device pixel per CSS pixel.
const FALLBACK_DEVICE_PIXEL_RATIO: f64 = 1.0;

/// The tile resolution [`BrowserMapView::new`] uses when the caller omits it: the
/// same rule the page applies with [`RETINA_DEVICE_PIXEL_RATIO_THRESHOLD`],
/// so the two-argument constructor and the page's own choice cannot disagree.
fn device_tile_resolution() -> CanvasTileResolution {
    let device_pixel_ratio = web_sys::window()
        .map(|window| window.device_pixel_ratio())
        .unwrap_or(FALLBACK_DEVICE_PIXEL_RATIO);
    if device_pixel_ratio >= RETINA_DEVICE_PIXEL_RATIO_THRESHOLD {
        CanvasTileResolution::Retina
    } else {
        CanvasTileResolution::Standard
    }
}

/// Statistics text a frame reports for the tile resolution it sampled.
fn tile_resolution_status_text(tile_resolution: RasterTileResolution) -> &'static str {
    match tile_resolution {
        RasterTileResolution::Standard => STATUS_TILE_RESOLUTION_STANDARD_TEXT,
        RasterTileResolution::Retina => STATUS_TILE_RESOLUTION_RETINA_TEXT,
    }
}

/// Everything a frame needs once initialization succeeded.
struct CanvasDrawState {
    device: wgpu::Device,
    queue: wgpu::Queue,
    adapter: wgpu::Adapter,
    surface_format: TextureFormat,
    tile_pipeline: wgpu::RenderPipeline,
    tile_sampler: wgpu::Sampler,
    /// Solid-color pipeline of the clip-space overlay layer, still used by the
    /// per-frame location-indicator triangles.
    overlay_pipeline: wgpu::RenderPipeline,
    /// World-space overlay pipeline and its per-frame uniform, drawn from the
    /// cached overlay tessellation.
    world_space_overlay_pipeline: WorldSpaceOverlayPipeline,
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
    /// The cached world-space overlay geometry (re-tessellated only when the
    /// overlay content revision changes).
    world_space_overlay_cache: Option<WorldSpaceOverlayCache>,
    /// The overlay layer's vertex buffer, reused across revisions so a
    /// re-tessellation only reallocates when the new geometry outgrows it.
    world_space_overlay_vertices: ReusableUploadBuffer,
    /// The raster tile layer's per-frame vertex and index buffers, reused so
    /// a steady frame never allocates.
    tile_vertex_buffer: ReusableUploadBuffer,
    tile_index_buffer: ReusableUploadBuffer,
    /// The label layer's halo and text vertex buffers, reused per frame.
    label_halo_vertex_buffer: ReusableUploadBuffer,
    label_text_vertex_buffer: ReusableUploadBuffer,
    /// The label layer's route-reference badge vertex buffer, reused per frame.
    label_badge_vertex_buffer: ReusableUploadBuffer,
    /// The icon pass's vertex and index buffers, reused per frame.
    icon_vertex_buffer: ReusableUploadBuffer,
    icon_index_buffer: ReusableUploadBuffer,
    /// The location indicator's vertex buffer, reused per frame.
    indicator_vertex_buffer: ReusableUploadBuffer,
    /// The camera-space overlay vertex buffer, used instead of the cached
    /// world-space geometry when a composed overlay carries a styled stroke.
    camera_space_overlay_vertices: ReusableUploadBuffer,
    /// Vertices the camera-space overlay buffer holds this frame; zero draws
    /// nothing.
    camera_space_overlay_vertex_count: u32,
    /// The overlay revision the composed list below was built at.
    composed_overlay_revision: Option<u64>,
    /// The composed overlay list when it carries a styled stroke, which the
    /// camera-space path re-assembles at every frame's camera pose. Empty
    /// while the world-space cache serves the list.
    camera_space_composed_overlays: Vec<MapOverlay>,
    /// The recorded-tile provider, also held directly (the scheduler keeps it
    /// behind a type-erased trait object) so the frame loop can preload
    /// fetched payloads into it for the scheduler to hand out.
    tile_provider: std::sync::Arc<RecordedTilePayloadProvider>,
    tile_scheduler: TileScheduler,
    /// Cached tile textures keyed by their (tile, revision) stamps; see
    /// [`TileTextureCacheEntry`].
    tile_textures: HashMap<TileKey, TileTextureCacheEntry>,
    /// Access order of the texture cache entries (least recent first), so
    /// pruning drops the oldest entries when the cache exceeds its cap.
    texture_cache_order: VecDeque<TileKey>,
    /// Canvas extent in pixels the surface was last configured at.
    configured_extent: (u32, u32),
    /// Timestamp-query ring that reports the render pass's GPU time, absent
    /// when the adapter does not support it.
    gpu_pass_timestamps: Option<GpuPassTimestamps>,
    /// The tile-level cross-fade state: which retired zoom level stays on
    /// screen while the new level's tiles arrive, and at what opacity.
    zoom_transition: ZoomTransition,
    /// The browser clock reading the cross-fade's timestamps start from.
    zoom_transition_clock_origin_milliseconds: f64,
    /// The draw list of the most recently presented frame, replayed by
    /// [`BrowserMapView::snapshot`] into an offscreen texture.
    last_frame_draw: Option<FrameDrawRecord>,
}

impl CanvasDrawState {
    /// Every GPU vertex and index buffer allocation this draw state has made
    /// across all of its reusable upload slots, reported in the frame
    /// statistics as [`STATUS_FIELD_GPU_BUFFER_ALLOCATIONS`].
    fn upload_buffer_allocation_count(&self) -> usize {
        [
            &self.world_space_overlay_vertices,
            &self.tile_vertex_buffer,
            &self.tile_index_buffer,
            &self.label_halo_vertex_buffer,
            &self.label_text_vertex_buffer,
            &self.indicator_vertex_buffer,
            &self.camera_space_overlay_vertices,
        ]
        .into_iter()
        .map(|slot| slot.allocations)
        .sum()
    }

    /// Uploads the label layer's packed glyph atlas as an `R8Unorm` texture
    /// when the font's content revision changed since the last upload, and
    /// reuses the cached texture and bind group otherwise — the atlas is
    /// uploaded once per distinct font, never per frame.
    fn ensure_glyph_atlas_texture(&mut self, font: &LabelFont) -> wgpu::BindGroup {
        if let Some(cache_entry) = &self.glyph_atlas_texture
            && cache_entry.revision == font.revision()
        {
            return cache_entry.bind_group.clone();
        }
        let atlas = font.atlas();
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("rustic-maps-web-bindings-glyph-atlas-texture"),
            size: wgpu::Extent3d {
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
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &atlas.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(GLYPH_ATLAS_PIXEL_BYTES * atlas.width_pixels),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: atlas.width_pixels,
                height: atlas.height_pixels,
                depth_or_array_layers: 1,
            },
        );
        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rustic-maps-web-bindings-glyph-atlas-bind-group"),
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
    /// atlas bound in place of a tile.
    fn ensure_sprite_atlas_texture(
        &mut self,
        sprite_atlas: &rustic_maps_rendering::SpriteAtlas,
    ) -> wgpu::BindGroup {
        if let Some(cache_entry) = &self.sprite_atlas_texture
            && cache_entry.revision == sprite_atlas.revision()
        {
            return cache_entry.bind_group.clone();
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("rustic-maps-web-bindings-sprite-atlas-texture"),
            size: wgpu::Extent3d {
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
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            sprite_atlas.rgba_pixels(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(
                    rustic_maps_rendering::SPRITE_ATLAS_PIXEL_BYTES * sprite_atlas.width_pixels(),
                ),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: sprite_atlas.width_pixels(),
                height: sprite_atlas.height_pixels(),
                depth_or_array_layers: 1,
            },
        );
        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rustic-maps-web-bindings-sprite-atlas-bind-group"),
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

    /// Uploads a pooled payload as a renderable texture, touching the pool's
    /// recency for the tile. Mirrors the native headless upload path:
    /// `Ok(None)` — no error — when the payload left the pool between
    /// assembly and upload, or when it fails to decode (whose corrupt bytes
    /// are then evicted from the pool so the scheduler refetches them and
    /// every later frame recovers). Returns the texture and bind group,
    /// freshly uploaded on a payload-revision miss or reused from the cache
    /// on a hit.
    fn tile_texture_and_bind_group(
        &mut self,
        tile_key: &TileKey,
    ) -> Result<Option<(Texture, wgpu::BindGroup)>, String> {
        let pool = self.tile_scheduler.texture_pool_mut();
        let Some(payload_revision) = pool.payload_revision(tile_key) else {
            return Ok(None);
        };
        if let Some(cache_entry) = self.tile_textures.get(tile_key) {
            if cache_entry.payload_revision == payload_revision {
                let (texture, bind_group) =
                    (cache_entry.texture.clone(), cache_entry.bind_group.clone());
                // Drawing from the cache is still a use of the pooled bytes:
                // keep the payload's recency fresh or LRU evicts exactly the
                // steady-camera tiles the cache exists to avoid refetching.
                self.tile_scheduler
                    .texture_pool_mut()
                    .refresh_recency(tile_key);
                self.texture_cache_order
                    .retain(|cached_tile_key| cached_tile_key != tile_key);
                self.texture_cache_order.push_back(*tile_key);
                return Ok(Some((texture, bind_group)));
            }
            // The pooled bytes were replaced since the upload: drop the stale
            // texture before reading the fresh payload.
            self.tile_textures.remove(tile_key);
            self.texture_cache_order
                .retain(|cached_tile_key| cached_tile_key != tile_key);
        }
        let Some(encoded_image_bytes) = pool.lookup(tile_key).map(<[u8]>::to_vec) else {
            return Ok(None);
        };
        // The staging decode already produced these texels (M23): reuse it
        // rather than decoding the same payload a second time. A miss means
        // the payload outlived its decoded form (a texture evicted and then
        // re-uploaded), so it decodes here; a corrupt payload evicts and
        // reports exactly as before.
        let decoded_payload = match self
            .tile_provider
            .decoded_tile_payload(tile_key, &encoded_image_bytes)
        {
            Some(decoded_payload) => decoded_payload,
            None => std::sync::Arc::new(decode_tile_payload(&encoded_image_bytes).inspect_err(
                |_failure_message| {
                    self.tile_scheduler.texture_pool_mut().evict(tile_key);
                },
            )?),
        };
        let (width_pixels, height_pixels) =
            (decoded_payload.width_pixels, decoded_payload.height_pixels);
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("rustic-maps-web-bindings-tile-texture"),
            size: wgpu::Extent3d {
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
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &decoded_payload.rgba_bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(PIXEL_BYTES * width_pixels),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: width_pixels,
                height: height_pixels,
                depth_or_array_layers: 1,
            },
        );
        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rustic-maps-web-bindings-tile-bind-group"),
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
        self.prune_and_insert_tile_texture_cache_entry(
            *tile_key,
            TileTextureCacheEntry {
                payload_revision,
                texture: texture.clone(),
                bind_group: bind_group.clone(),
            },
        );
        Ok(Some((texture, bind_group)))
    }

    /// Drops cache entries whose pooled bytes were replaced or evicted since
    /// their upload (a pool-revision miss invalidates the texture), inserts
    /// the fresh entry, and prunes the cache down to its bound by least
    /// recent use.
    fn prune_and_insert_tile_texture_cache_entry(
        &mut self,
        tile_key: TileKey,
        cache_entry: TileTextureCacheEntry,
    ) {
        let stale_tile_keys: Vec<TileKey> = self
            .tile_textures
            .iter()
            .filter_map(|(cached_tile_key, cached_entry)| {
                let pool_revision = self
                    .tile_scheduler
                    .texture_pool()
                    .payload_revision(cached_tile_key);
                (pool_revision != Some(cached_entry.payload_revision)).then_some(*cached_tile_key)
            })
            .collect();
        for stale_tile_key in stale_tile_keys {
            self.texture_cache_order
                .retain(|key| key != &stale_tile_key);
            self.tile_textures.remove(&stale_tile_key);
        }
        while self.tile_textures.len() >= MAXIMUM_CACHED_TILE_TEXTURES {
            let Some(least_recent_tile_key) = self.texture_cache_order.pop_front() else {
                break;
            };
            self.tile_textures.remove(&least_recent_tile_key);
        }
        self.texture_cache_order.push_back(tile_key);
        self.tile_textures.insert(tile_key, cache_entry);
    }
}
/// JavaScript field name of the flattened geographic-bounds readback's
/// south-west latitude: the page's own traffic and framing seams read this
/// shape rather than the contract record's nested corners.
const GEOGRAPHIC_BOUNDS_FIELD_SOUTH_WEST_LATITUDE: &str = "southWestLatitude";
/// JavaScript field name of the flattened geographic-bounds readback's
/// south-west longitude.
const GEOGRAPHIC_BOUNDS_FIELD_SOUTH_WEST_LONGITUDE: &str = "southWestLongitude";
/// JavaScript field name of the flattened geographic-bounds readback's
/// north-east latitude.
const GEOGRAPHIC_BOUNDS_FIELD_NORTH_EAST_LATITUDE: &str = "northEastLatitude";
/// JavaScript field name of the flattened geographic-bounds readback's
/// north-east longitude.
const GEOGRAPHIC_BOUNDS_FIELD_NORTH_EAST_LONGITUDE: &str = "northEastLongitude";

/// The browser GPU map canvas the page owns. Construct it over an
/// `HTMLCanvasElement`, [`BrowserMapView::initialize`] it once (acquiring the
/// adapter and device and building the tile pipeline), then call
/// [`BrowserMapView::render_frame`] per wanted frame; each call re-schedules
/// the visible tiles and redraws.
#[wasm_bindgen]
pub struct BrowserMapView {
    /// The canvas element the page renders onto; also the surface target.
    canvas: HtmlCanvasElement,
    /// This canvas' slot in the thread-local tile-arrival target registry,
    /// which lets the `Send + Sync` delivery callback name the canvas without
    /// holding it.
    map_tile_arrival_target_index: usize,
    instance: wgpu::Instance,
    surface: Surface<'static>,
    /// Tile pixel resolution the canvas fetches and samples its recorded
    /// fixtures at, fixed for the canvas' lifetime (the page picks it from
    /// its device-pixel ratio at construction).
    tile_resolution: RasterTileResolution,
    /// Whether a raster fetch may probe the same-origin recorded tile
    /// fixtures before a registered live source, fixed for the canvas'
    /// lifetime by the page's construction choice (see
    /// [`RECORDED_TILE_FIXTURES_ARE_SERVED_BY_DEFAULT`]).
    recorded_tile_fixtures_are_served: bool,
    /// Guards against (and reports) a second, racing `initialize`.
    initialization_claimed: Cell<bool>,
    /// Whether the raster tile base layer draws in every following
    /// [`BrowserMapView::render_frame`] (see
    /// [`BrowserMapView::set_tile_layer_visible`]); the overlay and
    /// location-indicator layers always draw, so the page can paint a vector
    /// base through them while this raster layer is hidden.
    tile_layer_visible: Cell<bool>,
    /// The frame pass's clear color: the shared dark clear by default, which
    /// the page overrides around a light vector base so the gaps between
    /// vector fills do not read as a black map. Values are 0..=1.
    clear_color: Cell<[f64; 4]>,
    /// Tiles whose fetch is already running. A frame starts a fetch and
    /// returns, because no fetch runs inside a presenting frame, so this set
    /// is what keeps a 60 Hz repaint from starting sixty fetches for one tile.
    tile_fetches_in_flight: Rc<RefCell<HashSet<TileKey>>>,
    /// A `reloadTileSource` request that landed while a frame owned the draw
    /// state: the frame's next run applies it before resolving its tiles, the
    /// same deferred-reload rule the native surface applies when a draw holds
    /// its scheduler.
    raster_reload_requested: Cell<bool>,
    /// The live-provider registration revision the pooled raster state was
    /// built under. A different revision means every staged payload, decoded
    /// texture, retry schedule and unavailable-tile mark belongs to a source
    /// the page no longer serves, so the next frame replaces them.
    raster_registration_revision: Cell<u64>,
    draw_state: RefCell<Option<CanvasDrawState>>,
    /// Overlay list drawn over the tiles in every following `render_frame`
    /// (empty — the tiles-only frame — until `set_overlays` fills it).
    overlays: RefCell<Vec<MapOverlay>>,
    /// Monotonic revision of [`Self::overlays`]; the cached world-space
    /// tessellation re-runs only when this changes, so panning, rotation and
    /// zoom reuse the uploaded geometry.
    overlay_revision: Cell<u64>,
    /// The caller-facing identifier addressing each drawn overlay, parallel
    /// to `overlays`: `addOverlay` entries carry their generated identifier,
    /// bulk `setOverlays` entries carry the empty string.
    overlay_identifiers: RefCell<Vec<String>>,
    /// The next identifier `addOverlay` answers (`overlay-1`, `overlay-2`,
    /// …); monotonic per view and never reset, even by `clearOverlays` or
    /// `setOverlays`.
    next_overlay_identifier: Cell<u64>,
    /// The labels the installed vector base's previous frame placed, carried
    /// into the next frame's collision pass so the names already on screen
    /// keep their spots while the camera moves. The list is cleared whenever
    /// the vector base is installed or dropped.
    previous_label_placements: RefCell<Vec<PlacedLabel>>,
    /// The installed vector base source: the shared
    /// [`VectorBaseScheduler`] fetches, decodes and styles the tileset, and
    /// every following [`BrowserMapView::render_frame`] draws its overlays and label
    /// candidates beneath the page's own layers, exactly like the native
    /// surface. `None` keeps the raster base in force.
    vector_base_scheduler: RefCell<Option<VectorBaseScheduler>>,
    /// The browser's in-memory vector payload cache, shared by every source
    /// installed on this view: a style switch or a reinstall of the same
    /// tileset then serves the region from memory instead of refetching it.
    /// The browser has no SQLite store, so this is the equivalent of the
    /// native disk cache under the same TTL and bounded-capacity policy.
    vector_tile_payload_cache: Arc<VectorTileMemoryCache>,
    /// The provider-published style documents fetched so far, keyed by the
    /// provider key and the address they came from. A document is kept only
    /// after it compiled, so a repeated install of the same family costs no
    /// request.
    provider_style_documents: RefCell<HashMap<(String, String), String>>,
    /// The label fonts a provider document's glyph endpoint served, keyed by
    /// the provider key, the document address and the font stack. A repeated
    /// install of the same family draws its labels from the cached font.
    provider_label_fonts: RefCell<HashMap<(String, String, String), Arc<LabelFont>>>,
    /// The sprite atlases a provider document's sprite base served, keyed by
    /// the provider key, the document address and the sprite base.
    provider_sprite_atlases: RefCell<HashMap<(String, String, String), Arc<SpriteAtlas>>>,
    /// The installed source's identity for the status readback, kept beside
    /// the scheduler so a status query never borrows the drawing seam.
    vector_base_source_summary: RefCell<Option<VectorBaseSourceSummary>>,
    /// The last resolved vector base frame: its overlays compose into every
    /// following frame and its label candidates feed the label layer.
    vector_base_last_frame: RefCell<Option<Arc<VectorBaseFrame>>>,
    /// Taken while a render resolves the scheduler; installing a source
    /// refuses while it is set (the native surface's Busy refusal). A clear
    /// requested during a frame is deferred to the frame's restore, which
    /// drops the scheduler instead of writing it back.
    vector_base_draw_in_flight: Cell<bool>,
    /// Marked by [`BrowserMapView::clear_vector_tile_source`] while a frame owns the
    /// scheduler; the frame's restore honours it by dropping the source, so a
    /// clear requested mid-frame still stops every following frame.
    vector_base_clear_requested: Cell<bool>,
    /// Whether the page reports a camera gesture in progress (a pointer pan,
    /// a wheel zoom, a double-tap zoom or a keyboard camera step). A frame
    /// resolved while it is set draws the vector base in motion-graded mode,
    /// from cached geometry at the last settled style zoom and without label
    /// assembly. The page clears it when the gesture ends and schedules the
    /// settle frame.
    camera_gesture_active: Cell<bool>,
    /// An install requested while a frame owns the scheduler: the frame's
    /// restore applies it, so a style switch requested mid-frame succeeds
    /// instead of being refused, and the last request wins.
    pending_vector_base_source: RefCell<Option<PendingVectorBaseSource>>,
    /// Monotonic generation of the vector-base install path. Every install
    /// captures the value it began with and bumps the counter; a clear bumps
    /// it too. A document install that resumes after an await with an older
    /// value is superseded and returns without touching the scheduler, the
    /// status, the error path or the provider resource caches.
    vector_base_install_generation: Cell<u64>,
    /// Document installs still resolving. The status readback reports
    /// `pendingVectorInstall` while this is nonzero.
    vector_base_document_installs_in_flight: Cell<u32>,
    /// Location-indicator layer drawn over the overlays in every following
    /// `render_frame` (cleared — no indicator — until
    /// `set_location_indicator` supplies one).
    location_indicator: RefCell<Option<LocationIndicatorLayer>>,
    /// The one camera pose this view holds, mirroring the native surface's
    /// `camera`/`set_camera` pair. A rendered frame is authoritative and
    /// refreshes it from the frame's own camera argument.
    camera_store: CameraStore,
    /// Whether a frame has drawn and presented successfully since this view
    /// was built. The first `renderFrame` resolution is the readiness
    /// signal; a page that asks after it reads the same state here.
    has_drawn_first_frame: Cell<bool>,
}

impl Drop for BrowserMapView {
    fn drop(&mut self) {
        unregister_map_tile_arrival_target(self.map_tile_arrival_target_index);
    }
}

#[wasm_bindgen]
impl BrowserMapView {
    /// Builds the canvas renderer over the page's canvas element
    /// (`new BrowserMapView(canvas, tileResolution?, recordedTileFixturesAreServed?)`).
    /// The tile pixel resolution is optional: omitted, it follows the same
    /// device-pixel-ratio rule as `retinaDevicePixelRatioThreshold()`;
    /// supplied, it is honoured verbatim.
    ///
    /// The recorded-fixture capability is optional and defaults to enabled.
    /// Enabled, a raster fetch probes the same-origin `/tiles` fixtures before
    /// a registered live source, which is what the local dev server and the
    /// test harness serve. Disabled, the fetch goes straight to the registered
    /// provider and records no fixture attempt, so a page that stages no
    /// fixtures issues no 404 for them. Fails when the element cannot host a
    /// GPU surface (for example after another context already claimed the
    /// element). Call [`BrowserMapView::initialize`] before the first
    /// [`BrowserMapView::render_frame`].
    #[wasm_bindgen(constructor)]
    pub fn new(
        canvas: HtmlCanvasElement,
        tile_resolution: Option<CanvasTileResolution>,
        recorded_tile_fixtures_are_served: Option<bool>,
    ) -> Result<BrowserMapView, JsValue> {
        let tile_resolution = tile_resolution.unwrap_or_else(device_tile_resolution);
        let recorded_tile_fixtures_are_served = recorded_tile_fixtures_are_served
            .unwrap_or(RECORDED_TILE_FIXTURES_ARE_SERVED_BY_DEFAULT);
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
            .map_err(|error| {
                failure_with_code(
                    ERROR_CODE_RENDERING_SURFACE_UNAVAILABLE,
                    &format!("the canvas cannot host a GPU surface: {error}"),
                )
            })?;
        let map_tile_arrival_target_index = register_map_tile_arrival_target(&canvas);
        Ok(Self {
            canvas,
            map_tile_arrival_target_index,
            instance,
            surface,
            tile_resolution: tile_resolution.into(),
            recorded_tile_fixtures_are_served,
            initialization_claimed: Cell::new(false),
            tile_layer_visible: Cell::new(TILE_LAYER_VISIBLE_BY_DEFAULT),
            clear_color: Cell::new([
                f64::from(CLEAR_COLOR_RGBA[0]),
                f64::from(CLEAR_COLOR_RGBA[1]),
                f64::from(CLEAR_COLOR_RGBA[2]),
                f64::from(CLEAR_COLOR_RGBA[3]),
            ]),
            tile_fetches_in_flight: Rc::new(RefCell::new(HashSet::new())),
            raster_reload_requested: Cell::new(false),
            raster_registration_revision: Cell::new(
                crate::live_providers::raster_tile_provider_registration_revision(),
            ),
            draw_state: RefCell::new(None),
            overlays: RefCell::new(Vec::new()),
            overlay_revision: Cell::new(0),
            overlay_identifiers: RefCell::new(Vec::new()),
            next_overlay_identifier: Cell::new(0),
            previous_label_placements: RefCell::new(Vec::new()),
            vector_base_scheduler: RefCell::new(None),
            vector_tile_payload_cache: Arc::new(VectorTileMemoryCache::new(
                DEFAULT_VECTOR_TILE_MEMORY_CACHE_MAXIMUM_TILE_COUNT,
            )),
            provider_style_documents: RefCell::new(HashMap::new()),
            provider_label_fonts: RefCell::new(HashMap::new()),
            provider_sprite_atlases: RefCell::new(HashMap::new()),
            vector_base_source_summary: RefCell::new(None),
            vector_base_last_frame: RefCell::new(None),
            vector_base_draw_in_flight: Cell::new(false),
            vector_base_clear_requested: Cell::new(false),
            camera_gesture_active: Cell::new(false),
            pending_vector_base_source: RefCell::new(None),
            vector_base_install_generation: Cell::new(0),
            vector_base_document_installs_in_flight: Cell::new(0),
            location_indicator: RefCell::new(None),
            camera_store: CameraStore::new(),
            has_drawn_first_frame: Cell::new(false),
        })
    }

    /// Reads the camera pose this view holds — the browser shape of the
    /// contract's `MapView.camera()`.
    ///
    /// The pose starts at zero latitude and longitude, zoom 2, north-up and
    /// top-down, and every accepted `setCamera`, `fitBounds` or rendered
    /// frame updates it. The view stores its own pose so the page reports the
    /// same value it hands to a frame; drawing itself still receives the pose
    /// as an argument.
    ///
    /// # Returns
    /// The current camera pose.
    #[wasm_bindgen(js_name = camera)]
    pub fn camera(&self) -> MapCamera {
        MapCamera::from_native_camera(self.camera_store.camera())
    }

    /// Adopts `camera` as the pose following frames and readbacks report —
    /// the browser shape of the contract's `MapView.set_camera`.
    ///
    /// The pose is already validated (its constructor clamps zoom and pitch
    /// and wraps the bearing), so adopting it cannot fail and never changes
    /// an overlay. The binding borrows `camera`, so the page keeps using its
    /// own instance after the call.
    ///
    /// # Parameters
    /// - `camera` — the pose following frames and readbacks report.
    #[wasm_bindgen(js_name = setCamera)]
    pub fn set_camera(&self, camera: &MapCamera) {
        self.camera_store.set_camera(*camera.native_camera());
    }

    /// Rotates the content clockwise by `clockwise_degrees` around the
    /// viewport center, the browser shape of the wrapper's
    /// `rotateByDegrees`.
    ///
    /// The content-follows-fingers convention makes a clockwise content turn
    /// lower the camera bearing, so the contract's clockwise-positive bearing
    /// delta receives the negated value — the same sign the native wrappers
    /// apply. Before the first frame has configured the surface there is no
    /// viewport to rotate around, so the call changes nothing.
    ///
    /// # Parameters
    /// - `clockwise_degrees` — content rotation in degrees.
    #[wasm_bindgen(js_name = rotateByDegrees)]
    pub fn rotate_by_degrees(&self, clockwise_degrees: f64) -> Result<(), JsValue> {
        let Some((width_pixels, height_pixels)) = self.configured_viewport_extent() else {
            return Ok(());
        };
        let viewport = validated_viewport_pixels(f64::from(width_pixels), f64::from(height_pixels))
            .map_err(|failure_message| {
                failure_with_code(ERROR_CODE_INVALID_COORDINATE_INPUT, &failure_message)
            })?;
        let rotated = rotate_camera_clockwise(
            &self.camera_store.camera(),
            clockwise_degrees,
            &viewport,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
        );
        self.camera_store.set_camera(rotated);
        Ok(())
    }

    /// Reports whether a frame has drawn and presented successfully since
    /// this view was built — the browser shape of the wrapper's
    /// `hasDrawnFirstFrame`.
    ///
    /// # Returns
    /// True after the first successful `renderFrame` presentation.
    #[wasm_bindgen(js_name = hasDrawnFirstFrame)]
    pub fn has_drawn_first_frame(&self) -> bool {
        self.has_drawn_first_frame.get()
    }

    /// The render viewport width in pixels, the browser shape of the
    /// wrapper's `viewportWidthPixels` readback.
    ///
    /// # Returns
    /// The surface extent the last frame configured, or zero before the
    /// first frame.
    #[wasm_bindgen(js_name = viewportWidthPixels)]
    pub fn viewport_width_pixels(&self) -> u32 {
        self.configured_viewport_extent()
            .map(|(width_pixels, _height_pixels)| width_pixels)
            .unwrap_or(0)
    }

    /// The render viewport height in pixels, the browser shape of the
    /// wrapper's `viewportHeightPixels` readback.
    ///
    /// # Returns
    /// The surface extent the last frame configured, or zero before the
    /// first frame.
    #[wasm_bindgen(js_name = viewportHeightPixels)]
    pub fn viewport_height_pixels(&self) -> u32 {
        self.configured_viewport_extent()
            .map(|(_width_pixels, height_pixels)| height_pixels)
            .unwrap_or(0)
    }

    /// The capped physical-pixel extent the surface is configured to, the
    /// browser shape of the wrapper's `viewportPixelExtent` readback.
    ///
    /// # Returns
    /// `{widthPixels, heightPixels}`, or null before the first frame.
    #[wasm_bindgen(js_name = viewportPixelExtent)]
    pub fn viewport_pixel_extent(&self) -> Result<JsValue, JsValue> {
        let Some((width_pixels, height_pixels)) = self.configured_viewport_extent() else {
            return Ok(JsValue::NULL);
        };
        Ok(object_from_fields(&[
            (
                VIEWPORT_SIZE_FIELD_WIDTH_PIXELS,
                JsValue::from(width_pixels),
            ),
            (
                VIEWPORT_SIZE_FIELD_HEIGHT_PIXELS,
                JsValue::from(height_pixels),
            ),
        ])?
        .into())
    }

    /// The device pixels one CSS pixel covers on this canvas — the browser
    /// shape of the wrapper's `displayScale` readback.
    ///
    /// # Returns
    /// The drawing-buffer-to-layout ratio, or one before the page lays the
    /// canvas out.
    #[wasm_bindgen(js_name = displayScale)]
    pub fn display_scale(&self) -> f64 {
        screen_pixel_ratio(&self.canvas)
    }

    /// The surface extent the last frame configured, or null before a frame
    /// has sized the surface.
    fn configured_viewport_extent(&self) -> Option<(u32, u32)> {
        let extent = self
            .draw_state
            .borrow()
            .as_ref()
            .map(|draw_state| draw_state.configured_extent);
        extent.filter(|(width_pixels, height_pixels)| *width_pixels > 0 && *height_pixels > 0)
    }

    /// Frames a bounding box in the caller's viewport and adopts the fitted
    /// camera — the browser shape of the contract's `MapView.fit_bounds`.
    ///
    /// The viewport and insets are explicit arguments, exactly like
    /// `MapCamera.fittingBounds` and `groundMetersPerScreenPixel`, because a
    /// page frames its CSS-pixel viewport rather than the canvas' drawing
    /// buffer. The fit is top-down and north-up and keeps the whole box
    /// inside the inset area, clamped to `maximum_zoom_level`.
    ///
    /// # Parameters
    /// - `south_west_latitude` / `south_west_longitude` — the box's
    ///   south-west corner.
    /// - `north_east_latitude` / `north_east_longitude` — the box's
    ///   north-east corner.
    /// - `width_pixels` / `height_pixels` — the viewport extent.
    /// - `inset_top_pixels` / `inset_right_pixels` / `inset_bottom_pixels` /
    ///   `inset_left_pixels` — pixels kept clear on each viewport edge.
    /// - `maximum_zoom_level` — upper zoom bound for the fitted camera.
    ///
    /// # Returns
    /// The adopted camera, or null before `initialize` (no surface yet);
    /// nothing changes in that case.
    ///
    /// # Errors
    /// Throws `InvalidCoordinateInput` when the bounding box is not
    /// canonical, the viewport is not positive, the insets are negative or
    /// unusable, the maximum zoom is not finite, or the box spans no positive
    /// world area. A refused fit keeps the previous camera.
    // The argument count is the JavaScript API's shape (a bounding box plus a
    // viewport plus insets), matching `MapCamera.fittingBounds`.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = fitBounds)]
    pub fn fit_bounds(
        &self,
        south_west_latitude: f64,
        south_west_longitude: f64,
        north_east_latitude: f64,
        north_east_longitude: f64,
        width_pixels: f64,
        height_pixels: f64,
        inset_top_pixels: f64,
        inset_right_pixels: f64,
        inset_bottom_pixels: f64,
        inset_left_pixels: f64,
        maximum_zoom_level: f64,
    ) -> Result<JsValue, JsValue> {
        // The contract answers null while no surface viewport is attached;
        // the browser's attachment point is `initialize`.
        if !self.initialization_claimed.get() {
            return Ok(JsValue::NULL);
        }
        let bounding_box = validated_bounding_box(
            south_west_latitude,
            south_west_longitude,
            north_east_latitude,
            north_east_longitude,
        )
        .map_err(|message| failure_with_code(ERROR_CODE_INVALID_COORDINATE_INPUT, &message))?;
        let viewport = validated_viewport_pixels(width_pixels, height_pixels)
            .map_err(|message| failure_with_code(ERROR_CODE_INVALID_COORDINATE_INPUT, &message))?;
        let edge_insets = EdgeInsetsPixels {
            left: inset_left_pixels,
            top: inset_top_pixels,
            right: inset_right_pixels,
            bottom: inset_bottom_pixels,
        };
        let fitted_camera =
            web_camera::fit_camera(&bounding_box, &viewport, edge_insets, maximum_zoom_level)
                .map_err(|message| {
                    failure_with_code(ERROR_CODE_INVALID_COORDINATE_INPUT, &message)
                })?;
        self.camera_store.set_camera(fitted_camera);
        Ok(JsValue::from(MapCamera::from_native_camera(fitted_camera)))
    }

    /// Reads the geographic bounds of the viewport at the stored camera pose
    /// — the browser shape of the contract's
    /// `MapView.camera_geographic_bounds`.
    ///
    /// The viewport and vertical field of view are explicit arguments,
    /// exactly like the projection methods, because the browser has no
    /// attached platform surface to read them from. The four viewport corners
    /// are unprojected first; a steep pitch can put the far corner below the
    /// horizon, so when fewer than three corners reach the ground the
    /// camera's own ground scale bounds a box around its center instead.
    ///
    /// # Parameters
    /// - `width_pixels` / `height_pixels` — the viewport extent.
    /// - `field_of_view_degrees` — vertical field of view of the projection.
    ///
    /// # Returns
    /// `{southWestLatitude, southWestLongitude, northEastLatitude,
    /// northEastLongitude}`, or null when the projection cannot answer.
    ///
    /// # Errors
    /// Throws `InvalidCoordinateInput` for an invalid viewport.
    #[wasm_bindgen(js_name = cameraGeographicBounds)]
    pub fn camera_geographic_bounds(
        &self,
        width_pixels: f64,
        height_pixels: f64,
        field_of_view_degrees: f64,
    ) -> Result<JsValue, JsValue> {
        let viewport = validated_viewport_pixels(width_pixels, height_pixels)
            .map_err(|message| failure_with_code(ERROR_CODE_INVALID_COORDINATE_INPUT, &message))?;
        let geographic_bounds = web_camera::camera_geographic_bounds(
            &self.camera_store.camera(),
            &viewport,
            field_of_view_degrees,
        );
        let Some(geographic_bounds) = geographic_bounds else {
            return Ok(JsValue::NULL);
        };
        Ok(object_from_fields(&[
            (
                GEOGRAPHIC_BOUNDS_FIELD_SOUTH_WEST_LATITUDE,
                JsValue::from(geographic_bounds.south_west.latitude),
            ),
            (
                GEOGRAPHIC_BOUNDS_FIELD_SOUTH_WEST_LONGITUDE,
                JsValue::from(geographic_bounds.south_west.longitude),
            ),
            (
                GEOGRAPHIC_BOUNDS_FIELD_NORTH_EAST_LATITUDE,
                JsValue::from(geographic_bounds.north_east.latitude),
            ),
            (
                GEOGRAPHIC_BOUNDS_FIELD_NORTH_EAST_LONGITUDE,
                JsValue::from(geographic_bounds.north_east.longitude),
            ),
        ])?
        .into())
    }

    /// Acquires the adapter and device, picks the pass-through surface
    /// format, and builds the tile pipeline. Resolves to the adapter
    /// description (graphics backend and adapter name) for the page's status
    /// line, or fails when no WebGPU adapter supports this canvas — the page
    /// then degrades to the 2D tile grid overlay. Never callable twice on
    /// the same canvas.
    pub async fn initialize(&self) -> Result<String, JsValue> {
        if self.initialization_claimed.replace(true) {
            return Err(failure_with_code(
                ERROR_CODE_INVALID_ARGUMENT,
                "the GPU canvas is already initialized or initializing",
            ));
        }
        let result = self.initialize_device().await;
        if result.is_err() {
            // A transient adapter or device failure must not latch the view
            // into a permanently degraded state: release the claim so the
            // next call may retry (or recover after a device loss).
            self.initialization_claimed.set(false);
        }
        result
    }

    /// Performs the device, pipeline and surface setup for
    /// [`Self::initialize`], which owns the initialization claim.
    async fn initialize_device(&self) -> Result<String, JsValue> {
        let adapter = self
            .instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&self.surface),
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .await
            .map_err(|error| {
                failure_with_code(
                    ERROR_CODE_RENDERING_SURFACE_UNAVAILABLE,
                    &format!("no WebGPU adapter supports this canvas: {error}"),
                )
            })?;

        // GPU pass timestamps are optional: an adapter that does not
        // advertise them, or that is a software rasterizer, is accepted
        // exactly as before and simply reports no GPU time.
        let gpu_pass_timestamps_supported = adapter_supports_gpu_pass_timestamps(&adapter);
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("rustic-maps-web-bindings-canvas"),
                required_features: if gpu_pass_timestamps_supported {
                    wgpu::Features::TIMESTAMP_QUERY
                } else {
                    wgpu::Features::empty()
                },
                // The adapter's own limits are guaranteed satisfiable; the
                // WebGPU defaults exceed a WebGL2 fallback adapter's
                // `max_inter_stage_shader_variables`, which would refuse the
                // device and silently kill the documented fallback.
                required_limits: adapter.limits(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|error| {
                failure_with_code(
                    ERROR_CODE_RENDERING_SURFACE_UNAVAILABLE,
                    &format!("the WebGPU device request failed: {error}"),
                )
            })?;

        // Device loss and uncaptured errors are recorded where a developer
        // can see them instead of dying in the browser console: wgpu's
        // default for an uncaptured error is a panic, and an asynchronous
        // device loss would otherwise surface only as a frozen map. The next
        // frame then fails with the coded surface error below, and the page
        // degrades to its documented 2D path.
        device.set_device_lost_callback(|device_lost_reason, device_lost_message| {
            tracing::error!(
                target: "rustic_maps_web_bindings::canvas",
                "the WebGPU device was lost ({device_lost_reason:?}): {device_lost_message}"
            );
        });
        device.on_uncaptured_error(std::sync::Arc::new(|device_error| {
            tracing::error!(
                target: "rustic_maps_web_bindings::canvas",
                "an uncaptured WebGPU error occurred: {device_error}"
            );
        }));

        let surface_capabilities = self.surface.get_capabilities(&adapter);
        let surface_format = SURFACE_FORMAT_PREFERENCE
            .into_iter()
            .find(|preferred_format| surface_capabilities.formats.contains(preferred_format))
            .ok_or_else(|| {
                failure_with_code(
                    ERROR_CODE_RENDERING_SURFACE_UNAVAILABLE,
                    &format!(
                        "the canvas supports none of the pass-through surface formats \
                         {SURFACE_FORMAT_PREFERENCE:?}; it offers {:?}",
                        surface_capabilities.formats
                    ),
                )
            })?;

        let (tile_pipeline, tile_sampler) =
            build_tile_pipeline(&device, surface_format).map_err(|failure_message| {
                failure_with_code(ERROR_CODE_RENDERING_SURFACE_UNAVAILABLE, &failure_message)
            })?;
        let overlay_pipeline =
            build_overlay_pipeline(&device, surface_format).map_err(|failure_message| {
                failure_with_code(ERROR_CODE_RENDERING_SURFACE_UNAVAILABLE, &failure_message)
            })?;
        let world_space_overlay_pipeline =
            build_world_space_overlay_pipeline(&device, surface_format).map_err(
                |failure_message| {
                    failure_with_code(ERROR_CODE_RENDERING_SURFACE_UNAVAILABLE, &failure_message)
                },
            )?;
        let glyph_pipeline =
            build_glyph_pipeline(&device, surface_format).map_err(|failure_message| {
                failure_with_code(ERROR_CODE_RENDERING_SURFACE_UNAVAILABLE, &failure_message)
            })?;
        let glyph_sampler = build_glyph_atlas_sampler(&device);
        let tile_provider =
            std::sync::Arc::new(RecordedTilePayloadProvider::new(self.tile_resolution));
        let tile_scheduler = TileScheduler::new(
            tile_provider.clone(),
            TileTexturePool::new(TILE_PAYLOAD_POOL_MAXIMUM_BYTES),
            RasterImageFormat::Png,
            self.tile_resolution,
            MAXIMUM_TILE_FETCHES_PER_FRAME,
        );

        let gpu_pass_timestamps =
            gpu_pass_timestamps_supported.then(|| GpuPassTimestamps::new(&device, &queue));
        let adapter_information = adapter.get_info();
        *self.draw_state.borrow_mut() = Some(CanvasDrawState {
            device,
            queue,
            adapter,
            surface_format,
            tile_pipeline,
            tile_sampler,
            overlay_pipeline,
            world_space_overlay_pipeline,
            world_space_overlay_cache: None,
            world_space_overlay_vertices: ReusableUploadBuffer::default(),
            tile_vertex_buffer: ReusableUploadBuffer::default(),
            tile_index_buffer: ReusableUploadBuffer::default(),
            label_halo_vertex_buffer: ReusableUploadBuffer::default(),
            label_text_vertex_buffer: ReusableUploadBuffer::default(),
            label_badge_vertex_buffer: ReusableUploadBuffer::default(),
            icon_vertex_buffer: ReusableUploadBuffer::default(),
            icon_index_buffer: ReusableUploadBuffer::default(),
            indicator_vertex_buffer: ReusableUploadBuffer::default(),
            camera_space_overlay_vertices: ReusableUploadBuffer::default(),
            camera_space_overlay_vertex_count: 0,
            composed_overlay_revision: None,
            camera_space_composed_overlays: Vec::new(),
            glyph_pipeline,
            glyph_sampler,
            glyph_atlas_texture: None,
            sprite_atlas_texture: None,
            tile_provider,
            tile_scheduler,
            tile_textures: HashMap::new(),
            texture_cache_order: VecDeque::new(),
            configured_extent: (0, 0),
            gpu_pass_timestamps,
            zoom_transition: ZoomTransition::new(),
            zoom_transition_clock_origin_milliseconds: browser_now_milliseconds(),
            last_frame_draw: None,
        });
        Ok(format!(
            "{} ({:?})",
            adapter_information.name, adapter_information.backend
        ))
    }

    /// Reports a camera gesture in progress on the page's own gesture surface:
    /// true from the pointer/wheel/key gesture's start until it ends.
    ///
    /// The page owns gesture recognition, so it is the only side that knows a
    /// gesture is active. While the flag is set, every frame resolves the
    /// vector base in motion-graded mode — cached geometry at the last settled
    /// style zoom and no label assembly — and reports `motionGraded` in its
    /// statistics. Calling it with false and scheduling one more frame draws
    /// the settle: the resolved style and labels at the final pose.
    ///
    /// This is a browser surface seam, not a contract member: the native
    /// surface reads the same signal from the camera pose each frame, where
    /// the platform's gesture recognizers already move it.
    #[wasm_bindgen(js_name = setCameraGestureActive)]
    pub fn set_camera_gesture_active(&self, active: bool) {
        self.camera_gesture_active.set(active);
    }

    /// Renders one map frame for the camera pose into the canvas, resolving
    /// freshly visible tiles from the same-origin `/tiles` route first.
    ///
    /// Resolves to a statistics object the page's readout consumes:
    /// `requestedTiles` (tiles the camera listed at the fetch zoom),
    /// `drawnTiles` (tiles with pooled imagery over their span this frame),
    /// `drawnFragments` (draw quads after world-copy fragment splitting),
    /// `missingTiles` (requested tiles the scheduler could not resolve from
    /// their own or an ancestor's pooled imagery this frame; a tile the frame
    /// culls as off-screen but has imagery for is not missing;
    /// `missingTileAddresses` names up to sixteen of them as `z/x/y` for a
    /// diagnostics readback),
    /// `zoomTransitionRunning` (whether a tile zoom-level cross-fade, a
    /// declared style transition or the styled vector base's own cross-fade
    /// still owes frames, so the page keeps drawing at the animation
    /// cadence),
    /// `motionGraded` (whether a camera gesture was active, so this frame
    /// reused settled geometry and skipped label assembly; the first frame
    /// after the gesture's end resolves both),
    /// `retainedTiles` (retired-level tiles drawn over the new level for the
    /// raster cross-fade),
    /// `vectorZoomTransitionRunning` and `vectorRetainedTiles` (the styled
    /// vector base's equivalents: whether its cross-fade still owes frames,
    /// and how many retired-level tiles it drew this frame),
    /// `vectorBaseLabelCandidateExtractionCount` and
    /// `vectorBaseLabelCandidateCacheHitCount` (tiles whose label candidates
    /// this frame extracted and tiles served from the per-tile cache; an
    /// unchanged settle frame reports zero extractions),
    /// `tileZoomLevel` (the fetch zoom, clamped to the recorded fixtures),
    /// `tileResolution` (the resolution the frame's fixtures were
    /// fetched and sampled at) and `overlayTriangles` (triangles in the
    /// cached world-space overlay layer drawn over the tiles this frame; zero
    /// without overlays; the GPU clips copies and near-plane fragments).
    /// `pendingVectorTiles` and `drawnVectorTiles` report the vector base's
    /// own pending and drawn tile counts (zero while no vector base serves),
    /// and `rasterTilesRetrying` is true when the raster layer listed tiles,
    /// drew none and still holds a retry schedule: the honest "waiting on the
    /// tile source" state rather than a silent black frame.
    /// With the raster tile layer hidden through
    /// [`BrowserMapView::set_tile_layer_visible`] the frame skips the whole
    /// tile pipeline and reports `requestedTiles`, `drawnTiles`,
    /// `drawnFragments` and `missingTiles` as zero while still presenting the
    /// overlay and indicator layers.
    ///
    /// The camera binding is consumed by value (the `wasm-bindgen` class
    /// parameter semantics); the page builds a fresh camera per changed
    /// pose, exactly as it already does. The frame is authoritative for the
    /// stored pose: `camera()` reports the pose this frame drew after it
    /// returns. Two overlapping calls are a construction-time borrow
    /// violation — the page serializes its frames behind a busy flag and
    /// never overlaps.
    #[allow(clippy::await_holding_refcell_ref)]
    // Sound on single-threaded wasm: the draw-state borrow spans the frame's awaits, and no code inside reenters `render_frame` (nothing borrows the same `RefCell` again; page serializes frames). See the module and method documentation.
    #[wasm_bindgen(js_name = renderFrame)]
    pub async fn render_frame(
        &self,
        camera: MapCamera,
        field_of_view_degrees: f64,
    ) -> Result<js_sys::Object, JsValue> {
        // A re-entrant call is refused with a coded error rather than a
        // `RefCell` panic: the contract tells the page to serialize frames,
        // and a second caller from another path must not take the map down.
        let mut draw_state_guard = self.draw_state.try_borrow_mut().map_err(|_| {
            failure_with_code(
                ERROR_CODE_RENDERING_SURFACE_UNAVAILABLE,
                "a frame is already in progress; serialize renderFrame calls",
            )
        })?;
        let draw_state = draw_state_guard.as_mut().ok_or_else(|| {
            failure_with_code(
                ERROR_CODE_RENDERING_SURFACE_UNAVAILABLE,
                "the GPU canvas is not initialized",
            )
        })?;

        // A provider registration change, or a reload requested while the
        // previous frame owned the draw state, applies before this frame
        // resolves its tiles. The registration revision also discards every
        // pooled payload, texture, retry schedule and unavailable mark built
        // for the previous source, so a late fetch cannot stage into the
        // current pool.
        let current_raster_registration_revision =
            crate::live_providers::raster_tile_provider_registration_revision();
        if self.raster_registration_revision.get() != current_raster_registration_revision {
            self.raster_registration_revision
                .set(current_raster_registration_revision);
            self.replace_raster_tile_state(draw_state);
        }
        if self.raster_reload_requested.replace(false) {
            self.replace_raster_tile_state(draw_state);
        }

        // The GPU pass timing ring advances here, before the frame is
        // recorded: the previous frame's completed reading is collected and
        // released, and this frame gets the slot it writes into. A reading
        // that has not landed is dropped rather than awaited, so no frame
        // ever waits on its own timestamps.
        let gpu_pass_write_slot_index = draw_state
            .gpu_pass_timestamps
            .as_mut()
            .and_then(GpuPassTimestamps::begin_frame);

        let (width_pixels, height_pixels) = clamped_surface_extent(
            (self.canvas.width(), self.canvas.height()),
            draw_state.adapter.limits().max_texture_dimension_2d,
        );
        let width_pixels = f64::from(width_pixels);
        let height_pixels = f64::from(height_pixels);
        let viewport =
            validated_viewport_pixels(width_pixels, height_pixels).map_err(|failure_message| {
                failure_with_code(ERROR_CODE_INVALID_COORDINATE_INPUT, &failure_message)
            })?;
        let native_camera = camera.native_camera();
        // A frame is authoritative for the stored pose: a following
        // `camera()` readback reports exactly what this frame drew.
        self.camera_store.set_camera(*native_camera);
        // The fixture set stops at the highest recorded zoom; a live raster
        // source extends the fetch grid to its own declared ceiling, so a deep
        // camera outside the recorded area still fetches servable tiles.
        let tile_source_maximum_zoom_level =
            crate::live_providers::live_raster_maximum_zoom_level()
                .unwrap_or(MAXIMUM_RECORDED_TILE_ZOOM_LEVEL);
        let tile_zoom_level = tile_fetch_zoom_level(
            native_camera.tile_zoom_level(),
            tile_source_maximum_zoom_level,
        );

        // The canvas-owned vector base (M31): the installed source resolves
        // every frame exactly like the native surface — the camera's visible
        // tiles at the served zoom, fetched and decoded once and styled at the
        // camera zoom — and hands its overlays and place names to the layers
        // below. Taking the scheduler out of its slot keeps the borrow off the
        // await; installing or clearing refuses while the frame owns it.
        let mut vector_base_scheduler = self.take_vector_base_scheduler();
        let vector_base_installed = vector_base_scheduler.is_some();
        let mut style_transition_running = false;
        let mut vector_zoom_transition_running = false;
        let vector_base_frame = match vector_base_scheduler.as_mut() {
            Some(vector_base_scheduler) => {
                let vector_base_zoom_level =
                    vector_base_scheduler.served_zoom_level(native_camera.tile_zoom_level());
                let vector_base_tile_keys = native_camera.visible_tile_keys(
                    &viewport,
                    field_of_view_degrees,
                    vector_base_zoom_level,
                );
                // Progressive draw: the frame paints whatever the decoded
                // cache holds and reports the rest pending, while the missing
                // tiles are fetched by detached tasks outside any presenting
                // frame. A delivered tile announces itself on the canvas so
                // the page's on-demand loop draws the next frame with it.
                // A declared style transition blends from the style this one
                // replaced; the monotonic frame clock drives it, so the page
                // never schedules a timer and a missing clock degrades to the
                // settled style.
                let frame_time_seconds = browser_now_milliseconds() / MILLISECONDS_PER_SECOND;
                // The shared frame keeps the tens of thousands of styled
                // overlays behind one reference-counted allocation; an
                // unchanged frame reuses the same allocation.
                let vector_base_frame = vector_base_scheduler
                    .cached_frame_at_time_in_motion_shared(
                        &vector_base_tile_keys,
                        native_camera.zoom_level,
                        frame_time_seconds,
                        self.camera_gesture_active.get(),
                    );
                style_transition_running = vector_base_scheduler.style_transition_running();
                // A vector tile-level cross-fade keeps the page's settled
                // pump drawing on the animation cadence until the fade lands,
                // exactly like the raster cross-fade.
                vector_zoom_transition_running = vector_base_frame.zoom_transition_running;
                // Each visible tile that lands reports itself, so the page can
                // repaint on the first arrival instead of waiting for the whole
                // set; the delivery callback coalesces the burst into at most
                // one repaint per window.
                let map_tile_arrival_target_index = self.map_tile_arrival_target_index;
                if let Some(prefetch) = vector_base_scheduler.prefetch_missing_tiles_with_delivery(
                    &vector_base_tile_keys,
                    move |_tile_key| {
                        schedule_coalesced_map_tile_arrived(map_tile_arrival_target_index);
                    },
                ) {
                    wasm_bindgen_futures::spawn_local(async move {
                        let _ = prefetch.await;
                    });
                }
                // The ring around the viewport (with the coarse parents that
                // back a parent fallback) warms after the presentable frame,
                // so the next pan starts from the cache.
                if let Some(ring_prefetch) =
                    vector_base_scheduler.prefetch_ring_tiles(&vector_base_tile_keys)
                {
                    let canvas = self.canvas.clone();
                    wasm_bindgen_futures::spawn_local(async move {
                        if ring_prefetch.await > 0 {
                            dispatch_map_tile_arrived(&canvas);
                        }
                    });
                }
                Some(vector_base_frame)
            }
            None => None,
        };
        let vector_base_cleared_during_frame = self.vector_base_clear_requested.get();
        self.restore_vector_base_scheduler(vector_base_scheduler);
        if let Some(drawn_vector_base_frame) = vector_base_frame
            .as_ref()
            .filter(|_| !vector_base_cleared_during_frame)
        {
            let mut last_vector_base_frame = self.vector_base_last_frame.borrow_mut();
            // The frame's content revision names the painted content: two
            // frames with the same revision carry the same overlays, so the
            // canvas compares one integer rather than deep-comparing the
            // overlay list.
            let overlays_changed = last_vector_base_frame
                .as_ref()
                .is_none_or(|previous_frame| {
                    previous_frame.content_revision != drawn_vector_base_frame.content_revision
                });
            *last_vector_base_frame = Some(Arc::clone(drawn_vector_base_frame));
            if overlays_changed {
                self.bump_overlay_revision();
            }
        }

        // The raster tile base layer is the provider's imagery. Hiding it (the
        // page's vector-base mode, or an installed vector source, which hides
        // it exactly like the native surface) skips the whole tile pipeline — the camera
        // listing, same-origin fetches, scheduler warm/resolve, texture
        // uploads and tile draws — while the frame still clears the surface
        // and paints the overlay and location-indicator layers. Re-showing it
        // resolves tiles normally on the next frame.
        // The frame's stage clock (M23): one `performance.now()` read at each
        // stage boundary, so a slow frame is attributable from the page
        // itself rather than guessed at from outside. Four monotonic clock
        // reads per frame are far below the frame budget's noise floor.
        let tile_stage_start_milliseconds = browser_now_milliseconds();
        let tile_layer_visible = self.tile_layer_visible.get() && !vector_base_installed;
        let mut requested_tile_count = 0;
        let mut drawn_tile_count = 0;
        let mut retained_tile_count = 0;
        let mut raster_tiles_retrying = false;
        // The addresses of the requested tiles this frame could draw nothing
        // for, bounded; a diagnostics panel names the tiles that lagged.
        let mut missing_tile_addresses: Vec<String> = Vec::new();
        // The requested tiles the scheduler could not resolve from the pool.
        let mut missing_prepared_tiles: Vec<TileKey> = Vec::new();
        // A running style transition keeps the page's settled pump drawing on
        // the animation cadence until the blend lands, exactly like a tile
        // zoom-level cross-fade.
        let mut zoom_transition_running =
            style_transition_running || vector_zoom_transition_running;
        let mut fragment_vertices = Vec::new();
        let mut fragment_quad_positions = Vec::new();
        let mut quad_textures_and_bind_groups = Vec::new();
        if tile_layer_visible {
            let requested_tiles =
                native_camera.visible_tile_keys(&viewport, field_of_view_degrees, tile_zoom_level);
            requested_tile_count = requested_tiles.len();
            // Fetches are started, never awaited (M23): the frame draws what
            // the pool already holds and reports the rest as missing, and the
            // page's settled pump renders again when a payload lands.
            self.start_tile_prefetches(draw_state, &requested_tiles);
            // The split frame path (spec round 10): the same-origin staged
            // payloads move into the pool through the scheduler's awaited warm
            // pass, then the frame resolves from the pool's state alone — the
            // readiness parity of the native surface's resolve/warm split (the
            // wasm page awaits its own warm pass inside the serialized frame,
            // so no warming marks in flight here).
            let _warm_fetch_count = draw_state.tile_scheduler.warm_tiles(&requested_tiles).await;
            let prepared_tiles = draw_state.tile_scheduler.resolve_frame(&requested_tiles);
            // Missing tiles are the ones the scheduler could not resolve from
            // the pool: a tile the frame culled as off-screen but has imagery
            // for is not missing, and a deferred or rejected tile is.
            missing_prepared_tiles = prepared_tiles
                .iter()
                .filter(|prepared_tile| {
                    matches!(
                        prepared_tile.readiness,
                        TileReadiness::Unavailable | TileReadiness::Deferred
                    )
                })
                .map(|prepared_tile| prepared_tile.requested_tile_key)
                .collect();

            // The GPU-free quad assembler: culling, world-copy fragment
            // splitting, and the `f64` camera projection all live here.
            let quad_composer = Renderer::new(field_of_view_degrees);
            let mut quads = quad_composer.frame_quads(
                native_camera,
                &viewport,
                &prepared_tiles,
                draw_state.tile_scheduler.texture_pool(),
            );
            drawn_tile_count = quads.len();
            // A requested tile the scheduler could not resolve from the pool
            // had no pooled imagery, own or ancestor: name it for the
            // diagnostics readback.
            for tile_key in &missing_prepared_tiles {
                if missing_tile_addresses.len() >= MAXIMUM_REPORTED_MISSING_TILE_ADDRESSES {
                    break;
                }
                missing_tile_addresses.push(format!(
                    "{}/{}/{}",
                    tile_key.zoom_level, tile_key.tile_x, tile_key.tile_y,
                ));
            }
            // A frame that listed tiles, drew none and holds a retry schedule
            // is waiting on the tile source. The flag lets the page tell that
            // state from a settled map and from a source that gave up on the
            // address.
            raster_tiles_retrying = requested_tile_count > 0
                && drawn_tile_count == 0
                && draw_state.tile_provider.retry_scheduled_tile_count() > 0;
            // A zoom that crossed a tile level keeps the retired level on
            // screen, resolved from the pool alone, and dissolves it into the
            // new level as its tiles arrive. The page's settled pump reads
            // the transition state from the frame statistics so it keeps
            // drawing the fade out.
            let zoom_transition_frame = draw_state.zoom_transition.update(
                tile_zoom_level,
                browser_now_milliseconds() - draw_state.zoom_transition_clock_origin_milliseconds,
                requested_level_ready_tile_fraction(&prepared_tiles),
            );
            zoom_transition_running = zoom_transition_frame.is_running;
            if let Some(retained_tile_zoom_level) = zoom_transition_frame.retained_tile_zoom_level {
                let retained_quads = quad_composer.retained_level_quads(
                    native_camera,
                    &viewport,
                    retained_tile_zoom_level,
                    draw_state.tile_scheduler.texture_pool(),
                    zoom_transition_frame.retained_opacity,
                );
                retained_tile_count = retained_quads.len();
                quads.extend(retained_quads);
            }
            let quad_fragments =
                quad_composer.quad_fragment_vertices(native_camera, &viewport, &quads);

            // One vertex array per world-copy fragment, each tagged with the
            // quad whose texture it draws — the headless renderer's draw
            // layout.
            fragment_quad_positions = Vec::with_capacity(quad_fragments.len());
            for (quad_index, quad_fragment_vertices) in quad_fragments.iter().enumerate() {
                for quad_fragment_vertices in quad_fragment_vertices {
                    fragment_quad_positions.push(quad_index);
                    fragment_vertices.push(*quad_fragment_vertices);
                }
            }
            if fragment_vertices.len() > MAXIMUM_QUADS_PER_FRAME {
                return Err(failure_with_code(
                    ERROR_CODE_RENDERING_SURFACE_UNAVAILABLE,
                    &format!(
                        "the visible tile fragments {} exceed the draw list bound {}",
                        fragment_vertices.len(),
                        MAXIMUM_QUADS_PER_FRAME
                    ),
                ));
            }

            quad_textures_and_bind_groups = Vec::with_capacity(quads.len());
            for quad in &quads {
                let texture_and_bind_group = draw_state
                    .tile_texture_and_bind_group(&quad.texture_tile_key)
                    .map_err(|failure_message| {
                        failure_with_code(
                            ERROR_CODE_RENDERING_SURFACE_UNAVAILABLE,
                            &failure_message,
                        )
                    })?;
                quad_textures_and_bind_groups.push(texture_and_bind_group);
            }
        }

        let tile_pipeline_milliseconds = browser_now_milliseconds() - tile_stage_start_milliseconds;
        let overlay_stage_start_milliseconds = browser_now_milliseconds();

        // The overlay layer draws from a world-space tessellation cached across
        // frames: the geometry is re-tessellated and uploaded only when the
        // overlay content revision changes, while every frame rewrites the
        // small frame uniform (view projection, viewport and the origin's
        // world-copy placement). Panning, rotation and zoom therefore cost one
        // uniform write and one instanced draw. The borrow spans no await
        // (assembly is synchronous), so a concurrent `set_overlays` from the
        // page only lands in a later frame.
        let overlay_revision = self.overlay_revision.get();
        if draw_state.composed_overlay_revision != Some(overlay_revision) {
            // The base's styled geometry paints first and the page's own
            // overlays paint over it — the native surface's composition. The
            // style declares point/CSS sizes, so the base list scales once
            // here (the page's list arrived scaled through `set_overlays`).
            let composed_overlays =
                {
                    let page_overlays = self.overlays.borrow();
                    let vector_base_frame = self.vector_base_last_frame.borrow();
                    let vector_base_overlay_count = vector_base_frame
                        .as_ref()
                        .map_or(0, |frame| frame.overlays.len());
                    let mut composed_overlays =
                        Vec::with_capacity(vector_base_overlay_count + page_overlays.len());
                    if let Some(frame) = vector_base_frame.as_ref() {
                        let pixel_ratio = screen_pixel_ratio(&self.canvas);
                        composed_overlays.extend(frame.overlays.iter().map(|overlay| {
                            overlay_scaled_to_device_pixel_ratio(overlay, pixel_ratio)
                        }));
                    }
                    composed_overlays.extend(page_overlays.iter().cloned());
                    composed_overlays
                };
            // A styled stroke cannot ride the world-space cache: its dash
            // phase and caps are screen-space, so the whole list assembles at
            // the frame's camera pose instead. Every other overlay keeps the
            // cached path.
            if overlays_require_camera_space_strokes(&composed_overlays) {
                draw_state.camera_space_composed_overlays = composed_overlays;
                draw_state.world_space_overlay_cache = None;
            } else {
                draw_state.camera_space_composed_overlays = Vec::new();
                let overlay_geometry = WorldSpaceOverlayGeometry::tessellate(&composed_overlays);
                if !overlay_geometry.is_empty() {
                    draw_state.world_space_overlay_vertices.upload(
                        &draw_state.device,
                        &draw_state.queue,
                        "rustic-maps-web-bindings-world-space-overlay-vertices",
                        wgpu::BufferUsages::VERTEX,
                        &encoded_world_space_overlay_vertices(overlay_geometry.vertices()),
                    );
                }
                let vertex_count = u32::try_from(overlay_geometry.vertex_count()).expect(
                    "the world-space overlay triangle list stays inside the u32 draw range",
                );
                let triangle_count = u32::try_from(overlay_geometry.triangle_count())
                    .expect("the world-space overlay triangle count stays inside the u32 range");
                draw_state.world_space_overlay_cache = Some(WorldSpaceOverlayCache {
                    vertex_count,
                    triangle_count,
                    origin_normalized_world: overlay_geometry.origin_normalized_world(),
                });
            }
            draw_state.composed_overlay_revision = Some(overlay_revision);
        }
        // The camera-space path re-assembles every frame: panning, rotation
        // and zoom change the screen-space geometry a styled stroke declares.
        let camera_space_overlay_buffer = if draw_state.camera_space_composed_overlays.is_empty() {
            draw_state.camera_space_overlay_vertex_count = 0;
            None
        } else {
            let camera_space_overlay_vertices = overlay_triangle_vertices(
                native_camera,
                &viewport,
                field_of_view_degrees,
                &draw_state.camera_space_composed_overlays,
            );
            let vertex_count = u32::try_from(camera_space_overlay_vertices.len())
                .expect("the camera-space overlay triangle list stays inside the u32 draw range");
            let buffer = (!camera_space_overlay_vertices.is_empty()).then(|| {
                draw_state.camera_space_overlay_vertices.upload(
                    &draw_state.device,
                    &draw_state.queue,
                    "rustic-maps-web-bindings-camera-space-overlay-vertices",
                    wgpu::BufferUsages::VERTEX,
                    &encoded_overlay_vertices(&camera_space_overlay_vertices),
                )
            });
            draw_state.camera_space_overlay_vertex_count = vertex_count;
            buffer
        };
        let (overlay_triangles, overlay_origin_normalized_world) =
            if draw_state.camera_space_composed_overlays.is_empty() {
                let overlay_cache = draw_state
                    .world_space_overlay_cache
                    .as_ref()
                    .expect("the overlay cache was filled above");
                (
                    overlay_cache.triangle_count,
                    overlay_cache.origin_normalized_world,
                )
            } else {
                let vertices_per_triangle = u32::try_from(OVERLAY_TRIANGLE_VERTEX_COUNT)
                    .expect("a triangle always has three vertices");
                (
                    draw_state.camera_space_overlay_vertex_count / vertices_per_triangle,
                    [0.0, 0.0],
                )
            };
        // The style's own canvas background fills the gaps its translucent
        // fills leave once the raster base is hidden; without a background the
        // page's chosen clear stays (the native surface's rule).
        let clear_color = self
            .vector_base_last_frame
            .borrow()
            .as_ref()
            .and_then(|vector_base_frame| vector_base_frame.background)
            .map_or_else(
                || self.clear_color.get(),
                |background| {
                    [
                        f64::from(background[0]),
                        f64::from(background[1]),
                        f64::from(background[2]),
                        f64::from(background[3]),
                    ]
                },
            );
        let overlay_frame_uniforms = WorldSpaceOverlayFrameUniforms::for_frame(
            native_camera,
            &viewport,
            field_of_view_degrees,
            overlay_origin_normalized_world,
        );
        draw_state.queue.write_buffer(
            &draw_state.world_space_overlay_pipeline.frame_uniform_buffer,
            0,
            &encoded_world_space_overlay_frame_uniforms(&overlay_frame_uniforms),
        );

        // The location-indicator layer assembles at the same camera pose and
        // draws after the overlays (the native surface's explicit three-layer
        // order); a cleared indicator contributes no buffer and no draw. The
        // vertices never enter the frame statistics, mirroring the native
        // statistics' overlay-only triangle count.
        let indicator_vertices = {
            let indicator_layer = self.location_indicator.borrow();
            location_indicator_vertices(
                native_camera,
                &viewport,
                field_of_view_degrees,
                indicator_layer.as_ref(),
            )
        };
        let overlay_geometry_milliseconds =
            browser_now_milliseconds() - overlay_stage_start_milliseconds;
        let indicator_vertex_count = u32::try_from(indicator_vertices.len())
            .expect("the indicator triangle list stays inside the u32 draw range");
        let indicator_vertex_buffer = (!indicator_vertices.is_empty()).then(|| {
            draw_state.indicator_vertex_buffer.upload(
                &draw_state.device,
                &draw_state.queue,
                "rustic-maps-web-bindings-indicator-vertices",
                wgpu::BufferUsages::VERTEX,
                &encoded_overlay_vertices(&indicator_vertices),
            )
        });

        // The label layer places its candidates at the frame's camera pose and
        // assembles screen-space glyph quads GPU-free; with no vector base (or
        // no candidates) it contributes no buffers and no draw.
        //
        // An installed vector base owns its place names (M31, the
        // renderer-internal ruling): the frame's own label candidates place
        // through the SDK's embedded default glyph font, the same font the
        // native surfaces draw with.
        let label_layer = {
            let vector_base_frame = self.vector_base_last_frame.borrow();
            vector_base_frame.as_ref().and_then(|frame| {
                let previous_placements = self.previous_label_placements.borrow();
                frame.label_layer_with_device_pixel_ratio_and_history(
                    native_camera,
                    &viewport,
                    field_of_view_degrees,
                    screen_pixel_ratio(&self.canvas),
                    previous_placements.as_slice(),
                )
            })
        };
        // Hand the vector base's placements to the next frame: the names on
        // screen keep their spots through a pan instead of flickering as
        // newcomers compete for the same space. A motion-graded frame placed
        // no labels, so it leaves the last settled placements in hand and the
        // settle places against the pre-gesture history.
        if self
            .vector_base_last_frame
            .borrow()
            .as_ref()
            .is_some_and(|vector_base_frame| !vector_base_frame.motion_graded)
        {
            *self.previous_label_placements.borrow_mut() = label_layer
                .as_ref()
                .map(|layer| layer.labels().to_vec())
                .unwrap_or_default();
        }
        let label_quad_vertices = label_layer
            .as_ref()
            .filter(|layer| !layer.is_empty())
            .map(|layer| layer.quad_vertices(&viewport))
            .unwrap_or_default();
        let label_count = label_layer
            .as_ref()
            .map(|layer| layer.labels().len())
            .unwrap_or(0);
        let line_label_count = label_layer
            .as_ref()
            .map(|layer| {
                layer
                    .labels()
                    .iter()
                    .filter(|label| label.placed_along_line)
                    .count()
            })
            .unwrap_or(0);
        let label_texts = Array::new();
        let line_label_texts = Array::new();
        if let Some(layer) = &label_layer {
            for label in layer.labels() {
                label_texts.push(&JsValue::from_str(&label.text));
                if label.placed_along_line {
                    line_label_texts.push(&JsValue::from_str(&label.text));
                }
            }
        }
        let glyph_bind_group = match &label_layer {
            Some(layer) if !label_quad_vertices.is_empty() => {
                Some(draw_state.ensure_glyph_atlas_texture(layer.font()))
            }
            _ => None,
        };
        let halo_vertex_buffer = (!label_quad_vertices.halo.is_empty()).then(|| {
            draw_state.label_halo_vertex_buffer.upload(
                &draw_state.device,
                &draw_state.queue,
                "rustic-maps-web-bindings-label-halo-vertices",
                wgpu::BufferUsages::VERTEX,
                &encoded_glyph_vertices(&label_quad_vertices.halo),
            )
        });
        let text_vertex_buffer = (!label_quad_vertices.text.is_empty()).then(|| {
            draw_state.label_text_vertex_buffer.upload(
                &draw_state.device,
                &draw_state.queue,
                "rustic-maps-web-bindings-label-text-vertices",
                wgpu::BufferUsages::VERTEX,
                &encoded_glyph_vertices(&label_quad_vertices.text),
            )
        });
        // A route-reference badge draws as solid triangles beneath its own
        // text, through the same overlay pipeline the styled overlays use; an
        // unbadged frame creates no buffer and no draw.
        let badge_vertex_buffer = (!label_quad_vertices.badge.is_empty()).then(|| {
            draw_state.label_badge_vertex_buffer.upload(
                &draw_state.device,
                &draw_state.queue,
                "rustic-maps-web-bindings-label-badge-vertices",
                wgpu::BufferUsages::VERTEX,
                &encoded_overlay_vertices(&label_quad_vertices.badge),
            )
        });

        // The icon pass assembles the placed icons' sprite quads GPU-free and
        // draws them through the tile pipeline with the sprite atlas bound; no
        // atlas or no placed icon contributes no buffers and no draw.
        let icon_quad_vertices = label_layer
            .as_ref()
            .filter(|layer| !layer.is_empty())
            .map(|layer| layer.icon_quad_vertices(&viewport))
            .unwrap_or_default();
        let sprite_atlas_bind_group = match &label_layer {
            Some(layer) if !icon_quad_vertices.is_empty() => layer
                .sprite()
                .map(|atlas| draw_state.ensure_sprite_atlas_texture(atlas)),
            _ => None,
        };
        let (icon_vertex_bytes, icon_index_bytes) = icon_quad_vertices.encoded_buffers();
        let icon_vertex_buffer = (!icon_quad_vertices.is_empty()).then(|| {
            draw_state.icon_vertex_buffer.upload(
                &draw_state.device,
                &draw_state.queue,
                "rustic-maps-web-bindings-icon-vertices",
                wgpu::BufferUsages::VERTEX,
                &icon_vertex_bytes,
            )
        });
        let icon_index_buffer = (!icon_quad_vertices.is_empty()).then(|| {
            draw_state.icon_index_buffer.upload(
                &draw_state.device,
                &draw_state.queue,
                "rustic-maps-web-bindings-icon-indices",
                wgpu::BufferUsages::INDEX,
                &icon_index_bytes,
            )
        });

        let encode_start_milliseconds = browser_now_milliseconds();

        // An all-missing frame has no draw list; its buffers would be empty,
        // which `wgpu` buffers cannot represent, so only a frame that draws
        // fills buffers and the all-missing frame renders as the clear color
        // alone. The buffers themselves are reused across frames. The clear
        // still runs inside the render pass.
        let (vertex_bytes, index_bytes) = encoded_fragment_buffers(&fragment_vertices);
        let vertex_buffer = (!fragment_vertices.is_empty()).then(|| {
            draw_state.tile_vertex_buffer.upload(
                &draw_state.device,
                &draw_state.queue,
                "rustic-maps-web-bindings-tile-vertices",
                wgpu::BufferUsages::VERTEX,
                &vertex_bytes,
            )
        });
        let index_buffer = (!fragment_vertices.is_empty()).then(|| {
            draw_state.tile_index_buffer.upload(
                &draw_state.device,
                &draw_state.queue,
                "rustic-maps-web-bindings-tile-indices",
                wgpu::BufferUsages::INDEX,
                &index_bytes,
            )
        });
        let encode_milliseconds = browser_now_milliseconds() - encode_start_milliseconds;

        // The frame's draw list, kept so `snapshot` can replay exactly what
        // this frame presents into an offscreen texture. The handles are
        // reference-counted clones of the buffers and bind groups encoded
        // below, so recording never duplicates GPU memory.
        let icon_index_count = u32::try_from(
            icon_quad_vertices.quads.len() * rustic_maps_rendering::labels::ICON_INDICES_PER_QUAD,
        )
        .expect("the icon quad list stays inside the u32 draw range");
        let halo_vertex_count = u32::try_from(label_quad_vertices.halo.len())
            .expect("the halo triangle list stays inside the u32 draw range");
        let text_vertex_count = u32::try_from(label_quad_vertices.text.len())
            .expect("the label triangle list stays inside the u32 draw range");
        let badge_vertex_count = u32::try_from(label_quad_vertices.badge.len())
            .expect("the badge triangle list stays inside the u32 draw range");
        let tile_fragments = fragment_quad_positions
            .iter()
            .copied()
            .enumerate()
            .filter_map(|(fragment_index, quad_position)| {
                let (_, bind_group) = quad_textures_and_bind_groups.get(quad_position)?.as_ref()?;
                Some(TileFragmentDraw {
                    bind_group: bind_group.clone(),
                    first_index: u32::try_from(fragment_index * INDICES_PER_QUAD)
                        .expect("the fragment list stays inside the u32 index range"),
                })
            })
            .collect();
        let frame_draw_record = FrameDrawRecord {
            clear_color,
            tile_vertex_buffer: vertex_buffer.clone(),
            tile_index_buffer: index_buffer.clone(),
            tile_fragments,
            world_space_overlay: draw_state
                .world_space_overlay_cache
                .as_ref()
                .filter(|overlay_cache| overlay_cache.vertex_count > 0)
                .map(|overlay_cache| {
                    (
                        draw_state
                            .world_space_overlay_vertices
                            .buffer
                            .as_ref()
                            .expect("a non-empty overlay cache was uploaded")
                            .clone(),
                        overlay_cache.vertex_count,
                    )
                }),
            camera_space_overlay: camera_space_overlay_buffer
                .as_ref()
                .filter(|_| draw_state.camera_space_overlay_vertex_count > 0)
                .map(|buffer| (buffer.clone(), draw_state.camera_space_overlay_vertex_count)),
            sprite_icons: match (
                &sprite_atlas_bind_group,
                icon_vertex_buffer.as_ref(),
                icon_index_buffer.as_ref(),
            ) {
                (Some(bind_group), Some(icon_vertex_buffer), Some(icon_index_buffer)) => Some((
                    bind_group.clone(),
                    icon_vertex_buffer.clone(),
                    icon_index_buffer.clone(),
                    icon_index_count,
                )),
                _ => None,
            },
            label_badge: badge_vertex_buffer
                .as_ref()
                .map(|buffer| (buffer.clone(), badge_vertex_count)),
            glyph_labels: glyph_bind_group.as_ref().map(|bind_group| GlyphLabelDraw {
                bind_group: bind_group.clone(),
                halo: halo_vertex_buffer
                    .as_ref()
                    .map(|buffer| (buffer.clone(), halo_vertex_count)),
                text: text_vertex_buffer
                    .as_ref()
                    .map(|buffer| (buffer.clone(), text_vertex_count)),
            }),
            location_indicator: indicator_vertex_buffer
                .as_ref()
                .map(|buffer| (buffer.clone(), indicator_vertex_count)),
        };

        // Submission stage: the surface texture is acquired last, so a
        // reconfigure the browser forces cannot invalidate buffers that were
        // already built.
        let submit_start_milliseconds = browser_now_milliseconds();
        let surface_texture =
            self.acquire_surface_texture(draw_state)
                .map_err(|failure_message| {
                    failure_with_code(ERROR_CODE_RENDERING_SURFACE_UNAVAILABLE, &failure_message)
                })?;
        let frame_texture_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut command_encoder =
            draw_state
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("rustic-maps-web-bindings-frame-encoder"),
                });
        {
            let mut render_pass = command_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("rustic-maps-web-bindings-frame-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &frame_texture_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: {
                            wgpu::LoadOp::Clear(wgpu::Color {
                                r: clear_color[0],
                                g: clear_color[1],
                                b: clear_color[2],
                                a: clear_color[3],
                            })
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: draw_state.gpu_pass_timestamps.as_ref().map(|timestamps| {
                    wgpu::RenderPassTimestampWrites {
                        query_set: &timestamps.query_set,
                        beginning_of_pass_write_index: Some(GPU_PASS_TIMESTAMP_BEGIN_INDEX),
                        end_of_pass_write_index: Some(GPU_PASS_TIMESTAMP_END_INDEX),
                    }
                }),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            encode_frame_draw_calls(&mut render_pass, draw_state, &frame_draw_record);
        }
        // The timestamp resolve is a buffer operation, so it is recorded
        // outside the render pass and submitted with the frame. The copy
        // targets the frame's readback slot, which the next frame maps.
        if let (Some(timestamps), Some(write_slot_index)) = (
            draw_state.gpu_pass_timestamps.as_ref(),
            gpu_pass_write_slot_index,
        ) {
            timestamps.resolve_and_copy(&mut command_encoder, write_slot_index);
        }
        draw_state.queue.submit([command_encoder.finish()]);
        draw_state.queue.present(surface_texture);
        // The first presented frame is the readiness state the page's
        // `hasDrawnFirstFrame` readback reports.
        self.has_drawn_first_frame.set(true);
        // The presented frame is now the one `snapshot` replays.
        draw_state.last_frame_draw = Some(frame_draw_record);
        // Mapping is a request, never a wait: the callback only records that
        // the reading landed, and the next frame decides whether to use it.
        if let (Some(timestamps), Some(write_slot_index)) = (
            draw_state.gpu_pass_timestamps.as_mut(),
            gpu_pass_write_slot_index,
        ) {
            timestamps.request_readback(write_slot_index);
        }
        let submit_milliseconds = browser_now_milliseconds() - submit_start_milliseconds;

        let stage_milliseconds = object_from_fields(&[
            (
                STAGE_TILE_PIPELINE,
                JsValue::from(tile_pipeline_milliseconds),
            ),
            (
                STAGE_OVERLAY_GEOMETRY,
                JsValue::from(overlay_geometry_milliseconds),
            ),
            (STAGE_ENCODE, JsValue::from(encode_milliseconds)),
            (STAGE_SUBMIT, JsValue::from(submit_milliseconds)),
        ])?;

        let missing_tiles = missing_prepared_tiles.len();
        // The bounded address list travels as one JavaScript array; naming
        // the tiles that lagged is what turns "5 missing" into a diagnosis.
        let missing_tile_address_values = js_sys::Array::new();
        for missing_tile_address in &missing_tile_addresses {
            missing_tile_address_values.push(&JsValue::from(missing_tile_address.as_str()));
        }
        let missing_tile_address_value: JsValue =
            <js_sys::Array as AsRef<JsValue>>::as_ref(&missing_tile_address_values).clone();
        // The vector base's own pending and drawn counts, so one frame report
        // answers both base layers honestly. A raster frame reports zeros for
        // both.
        let (pending_vector_tiles, drawn_vector_tiles) =
            vector_base_frame
                .as_ref()
                .map_or((0, 0), |resolved_vector_base_frame| {
                    (
                        u32::try_from(resolved_vector_base_frame.pending_tile_count)
                            .unwrap_or(u32::MAX),
                        u32::try_from(resolved_vector_base_frame.drawn_tile_count)
                            .unwrap_or(u32::MAX),
                    )
                });
        let mut statistics_fields: Vec<(&str, JsValue)> = vec![
            (
                STATUS_FIELD_REQUESTED_TILES,
                JsValue::from(requested_tile_count),
            ),
            (STATUS_FIELD_DRAWN_TILES, JsValue::from(drawn_tile_count)),
            (
                STATUS_FIELD_DRAWN_FRAGMENTS,
                JsValue::from(fragment_vertices.len()),
            ),
            (STATUS_FIELD_MISSING_TILES, JsValue::from(missing_tiles)),
            (
                STATUS_FIELD_MISSING_TILE_ADDRESSES,
                missing_tile_address_value,
            ),
            (
                STATUS_FIELD_ZOOM_TRANSITION_RUNNING,
                JsValue::from(zoom_transition_running),
            ),
            (
                STATUS_FIELD_RETAINED_TILES,
                JsValue::from(retained_tile_count),
            ),
            (
                STATUS_FIELD_PENDING_VECTOR_TILES,
                JsValue::from(pending_vector_tiles),
            ),
            (
                STATUS_FIELD_DRAWN_VECTOR_TILES,
                JsValue::from(drawn_vector_tiles),
            ),
            (
                STATUS_FIELD_RASTER_TILES_RETRYING,
                JsValue::from(raster_tiles_retrying),
            ),
            (STATUS_FIELD_TILE_ZOOM_LEVEL, JsValue::from(tile_zoom_level)),
            (
                STATUS_FIELD_TILE_RESOLUTION,
                JsValue::from_str(tile_resolution_status_text(
                    draw_state.tile_provider.tile_resolution,
                )),
            ),
            (
                STATUS_FIELD_OVERLAY_TRIANGLES,
                JsValue::from(overlay_triangles),
            ),
            (
                STATUS_FIELD_GPU_BUFFER_ALLOCATIONS,
                JsValue::from(draw_state.upload_buffer_allocation_count()),
            ),
            (STATUS_FIELD_LABEL_COUNT, JsValue::from(label_count)),
            (STATUS_FIELD_LABEL_TEXTS, label_texts.into()),
            (
                STATUS_FIELD_LINE_LABEL_COUNT,
                JsValue::from(line_label_count),
            ),
            (STATUS_FIELD_LINE_LABEL_TEXTS, line_label_texts.into()),
            (
                STATUS_FIELD_LABEL_GLYPH_TRIANGLES,
                JsValue::from(
                    u32::try_from(
                        (label_quad_vertices.halo.len() + label_quad_vertices.text.len()) / 3,
                    )
                    .expect("the label triangle count stays inside the u32 range"),
                ),
            ),
            (
                STATUS_FIELD_LABEL_BADGE_TRIANGLES,
                JsValue::from(
                    u32::try_from(label_quad_vertices.badge.len() / 3)
                        .expect("the badge triangle count stays inside the u32 range"),
                ),
            ),
            (STATUS_FIELD_STAGE_MILLISECONDS, stage_milliseconds.into()),
        ];
        // The GPU pass reading is reported only when one has completed: an
        // adapter without timestamp queries leaves the field out entirely
        // rather than substituting a zero.
        if let Some(gpu_pass_milliseconds) = draw_state
            .gpu_pass_timestamps
            .as_ref()
            .and_then(|timestamps| timestamps.last_completed_milliseconds)
        {
            statistics_fields.push((
                STATUS_FIELD_GPU_PASS_MILLISECONDS,
                JsValue::from(gpu_pass_milliseconds),
            ));
        }
        // A vector-base frame adds its cache and latency accounting, so the
        // page's readout can answer "is the base waiting on the server?"
        // without a second query. Raster frames omit the fields entirely.
        if let Some(vector_base_frame) = vector_base_frame.as_ref() {
            let diagnostics = vector_base_frame.diagnostics;
            statistics_fields.push((
                STATUS_FIELD_VECTOR_BASE_PAYLOAD_CACHE_HIT_COUNT,
                JsValue::from(
                    u32::try_from(diagnostics.payload_cache_hit_count).unwrap_or(u32::MAX),
                ),
            ));
            statistics_fields.push((
                STATUS_FIELD_VECTOR_BASE_PAYLOAD_CACHE_MISS_COUNT,
                JsValue::from(
                    u32::try_from(diagnostics.payload_cache_miss_count).unwrap_or(u32::MAX),
                ),
            ));
            statistics_fields.push((
                STATUS_FIELD_VECTOR_BASE_IN_FLIGHT_FETCH_COUNT,
                JsValue::from(u32::try_from(diagnostics.in_flight_fetch_count).unwrap_or(u32::MAX)),
            ));
            statistics_fields.push((
                STATUS_FIELD_VECTOR_BASE_FALLBACK_TILE_COUNT,
                JsValue::from(
                    u32::try_from(vector_base_frame.fallback_tile_count).unwrap_or(u32::MAX),
                ),
            ));
            statistics_fields.push((
                STATUS_FIELD_VECTOR_BASE_FIRST_DRAWN_TILE_MILLISECONDS,
                diagnostics
                    .time_to_first_drawn_tile_milliseconds
                    .map_or(JsValue::NULL, |milliseconds| {
                        JsValue::from(u32::try_from(milliseconds).unwrap_or(u32::MAX))
                    }),
            ));
            statistics_fields.push((
                STATUS_FIELD_VECTOR_BASE_MOTION_GRADED,
                JsValue::from(vector_base_frame.motion_graded),
            ));
            statistics_fields.push((
                STATUS_FIELD_VECTOR_ZOOM_TRANSITION_RUNNING,
                JsValue::from(vector_base_frame.zoom_transition_running),
            ));
            statistics_fields.push((
                STATUS_FIELD_VECTOR_RETAINED_TILES,
                JsValue::from(
                    u32::try_from(vector_base_frame.retained_tile_count).unwrap_or(u32::MAX),
                ),
            ));
            statistics_fields.push((
                STATUS_FIELD_VECTOR_BASE_LABEL_CANDIDATE_EXTRACTION_COUNT,
                JsValue::from(
                    u32::try_from(diagnostics.label_candidate_extraction_count).unwrap_or(u32::MAX),
                ),
            ));
            statistics_fields.push((
                STATUS_FIELD_VECTOR_BASE_LABEL_CANDIDATE_CACHE_HIT_COUNT,
                JsValue::from(
                    u32::try_from(diagnostics.label_candidate_cache_hit_count).unwrap_or(u32::MAX),
                ),
            ));
        }
        object_from_fields(&statistics_fields)
    }

    /// Answers the last presented frame's pixels as RGBA bytes without
    /// reading the canvas through the compositor.
    ///
    /// The most recent [`BrowserMapView::render_frame`] left a draw list behind;
    /// this method replays that list into an offscreen texture of the same
    /// format, extent and clear color, copies the texture into a CPU-readable
    /// buffer and maps it. The pixels therefore match the frame that was
    /// presented, and captures taken after consecutive frames at the same
    /// camera pose are byte-identical. No `drawImage` re-present cycle is
    /// involved, so the result does not depend on the compositor's
    /// presented-frame lifetime.
    ///
    /// Call it after a `renderFrame` and before the next one: the draw list
    /// names that frame's reusable buffers. Fails before the first frame, or
    /// when the canvas has no pixels.
    #[wasm_bindgen(js_name = snapshot)]
    pub async fn snapshot(&self) -> Result<Vec<u8>, JsValue> {
        let (readback_buffer, bytes_per_row, width_pixels, height_pixels, source_format) = {
            let draw_state_guard = self.draw_state.try_borrow().map_err(|_| {
                failure_with_code(
                    ERROR_CODE_RENDERING_SURFACE_UNAVAILABLE,
                    "a frame is in progress; await it before taking a snapshot",
                )
            })?;
            let draw_state = draw_state_guard.as_ref().ok_or_else(|| {
                failure_with_code(
                    ERROR_CODE_RENDERING_SURFACE_UNAVAILABLE,
                    "the GPU canvas is not initialized",
                )
            })?;
            let frame_draw_record = draw_state.last_frame_draw.as_ref().ok_or_else(|| {
                failure_with_code(
                    ERROR_CODE_RENDERING_SURFACE_UNAVAILABLE,
                    "no frame has been rendered; call renderFrame before snapshot",
                )
            })?;
            let (width_pixels, height_pixels) = draw_state.configured_extent;
            if width_pixels == 0 || height_pixels == 0 {
                return Err(failure_with_code(
                    ERROR_CODE_RENDERING_SURFACE_UNAVAILABLE,
                    "the map canvas has no pixels yet; skip the snapshot",
                ));
            }
            let snapshot_texture = draw_state.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("rustic-maps-web-bindings-snapshot-texture"),
                size: Extent3d {
                    width: width_pixels,
                    height: height_pixels,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: draw_state.surface_format,
                usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let snapshot_view =
                snapshot_texture.create_view(&wgpu::TextureViewDescriptor::default());
            let bytes_per_row = aligned_copy_bytes_per_row(width_pixels);
            let readback_buffer = draw_state.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rustic-maps-web-bindings-snapshot-readback"),
                size: u64::from(bytes_per_row) * u64::from(height_pixels),
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let mut command_encoder =
                draw_state
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("rustic-maps-web-bindings-snapshot-encoder"),
                    });
            {
                let mut render_pass =
                    command_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("rustic-maps-web-bindings-snapshot-pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &snapshot_view,
                            resolve_target: None,
                            depth_slice: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color {
                                    r: frame_draw_record.clear_color[0],
                                    g: frame_draw_record.clear_color[1],
                                    b: frame_draw_record.clear_color[2],
                                    a: frame_draw_record.clear_color[3],
                                }),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                encode_frame_draw_calls(&mut render_pass, draw_state, frame_draw_record);
            }
            command_encoder.copy_texture_to_buffer(
                TexelCopyTextureInfo {
                    texture: &snapshot_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: TextureAspect::All,
                },
                TexelCopyBufferInfo {
                    buffer: &readback_buffer,
                    layout: TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(bytes_per_row),
                        rows_per_image: Some(height_pixels),
                    },
                },
                Extent3d {
                    width: width_pixels,
                    height: height_pixels,
                    depth_or_array_layers: 1,
                },
            );
            draw_state.queue.submit([command_encoder.finish()]);
            (
                readback_buffer,
                bytes_per_row,
                width_pixels,
                height_pixels,
                draw_state.surface_format,
            )
        };

        // The map completes through the browser's own promise scheduling; no
        // further submission is needed to drive it.
        let map_promise = js_sys::Promise::new(&mut |resolve, reject| {
            let resolve = resolve.clone();
            let reject = reject.clone();
            readback_buffer.map_async(wgpu::MapMode::Read, .., move |map_result| {
                let callback_result = match map_result {
                    Ok(()) => resolve.call0(&JsValue::NULL),
                    Err(map_error) => reject.call1(
                        &JsValue::NULL,
                        &JsValue::from_str(&format!("the snapshot readback failed: {map_error:?}")),
                    ),
                };
                let _ = callback_result;
            });
        });
        JsFuture::from(map_promise).await.map_err(|map_error| {
            failure_with_code(
                ERROR_CODE_RENDERING_SURFACE_UNAVAILABLE,
                &format!("the snapshot readback failed: {map_error:?}"),
            )
        })?;

        let rgba_bytes = {
            let mapped_range = readback_buffer.get_mapped_range(..).map_err(|map_error| {
                failure_with_code(
                    ERROR_CODE_RENDERING_SURFACE_UNAVAILABLE,
                    &format!("the snapshot readback could not be read: {map_error:?}"),
                )
            })?;
            let unpadded_bytes_per_row = usize::try_from(width_pixels * PIXEL_BYTES)
                .expect("a viewport row fits the address space");
            let padded_bytes_per_row =
                usize::try_from(bytes_per_row).expect("the copy row stride fits the address space");
            let mut rgba_bytes =
                Vec::with_capacity(unpadded_bytes_per_row * height_pixels as usize);
            for row_index in 0..usize::try_from(height_pixels).expect("the height fits usize") {
                let row_start = row_index * padded_bytes_per_row;
                rgba_bytes.extend_from_slice(
                    &mapped_range[row_start..row_start + unpadded_bytes_per_row],
                );
            }
            rgba_bytes
        };
        readback_buffer.unmap();
        Ok(normalized_snapshot_rgba_bytes(source_format, rgba_bytes))
    }

    /// Stages a deliberate parent-fallback for one requested tile, mirroring
    /// the native golden-image tests: the fallback tile is evicted from the
    /// pool and marked unavailable forever, and its coarser parent's recorded
    /// fixture is pooled instead, so the scheduler resolves the fallback tile
    /// as `FallsBackTo { parent }` every frame and the frame samples the
    /// parent texture. The committed golden frames were produced with the
    /// last visible scene tile resolved exactly this way, so a browser page
    /// that compares against them calls this before rendering a golden scene
    /// to reproduce the reference imagery byte-for-byte.
    ///
    /// Fetches the parent fixture from the same-origin `/tiles` route. A
    /// missing or undecodable parent fixture is an error, not a silent skip:
    /// the caller wants the fallback imagery or nothing. The parent coordinates
    /// must describe the fallback tile's direct parent.
    ///
    /// A page that serves no recorded fixtures has no parent fixture to stage,
    /// so this returns an unsupported-capability failure instead of issuing a
    /// request the page cannot answer.
    ///
    /// The draw state is borrowed only to read the tile resolution and again
    /// after the fetch; the parent fetch itself runs outside any borrow, so a
    /// concurrent frame can draw while the payload is in flight.
    #[wasm_bindgen(js_name = stageParentTile)]
    pub async fn stage_tile_parent_fallback(
        &self,
        fallback_tile_x: u32,
        fallback_tile_y: u32,
        fallback_zoom_level: u8,
        parent_tile_x: u32,
        parent_tile_y: u32,
        parent_zoom_level: u8,
    ) -> Result<(), JsValue> {
        if !self.recorded_tile_fixtures_are_served {
            return Err(failure_with_code(
                ERROR_CODE_UNSUPPORTED_CAPABILITY,
                "this page does not serve the recorded tile fixtures",
            ));
        }
        let fallback_tile_key = TileKey::new(fallback_tile_x, fallback_tile_y, fallback_zoom_level)
            .map_err(|sdk_error| sdk_error_failure(&sdk_error))?;
        let parent_tile_key = TileKey::new(parent_tile_x, parent_tile_y, parent_zoom_level)
            .map_err(|sdk_error| sdk_error_failure(&sdk_error))?;
        if Some(parent_tile_key) != fallback_tile_key.parent_tile_key() {
            return Err(failure_with_code(
                ERROR_CODE_INVALID_ARGUMENT,
                "the parent tile is not the fallback tile's direct parent",
            ));
        }
        let parent_payload_url = {
            let draw_state_guard = self.draw_state.try_borrow().map_err(|_| {
                failure_with_code(
                    ERROR_CODE_RENDERING_SURFACE_UNAVAILABLE,
                    "the GPU canvas state is busy; retry the parent-tile fallback",
                )
            })?;
            let draw_state = draw_state_guard.as_ref().ok_or_else(|| {
                failure_with_code(
                    ERROR_CODE_RENDERING_SURFACE_UNAVAILABLE,
                    "the GPU canvas is not initialized",
                )
            })?;
            tile_fixture_url(&parent_tile_key, draw_state.tile_provider.tile_resolution)
        };
        let parent_payload_bytes =
            fetch_same_origin_bytes(&parent_payload_url)
                .await
                .map_err(|fetch_error| {
                    failure_with_code(ERROR_CODE_NETWORK_REQUEST_FAILED, &fetch_error.to_string())
                })?;
        // Validate the parent payload exactly like the regular preload pass,
        // then move it straight into the pool: the scheduler only ever fetches
        // requested tiles, never ancestors, so the fallback parent must be
        // pooled for `resolve_readiness` to find it.
        let mut draw_state_guard = self.draw_state.try_borrow_mut().map_err(|_| {
            failure_with_code(
                ERROR_CODE_RENDERING_SURFACE_UNAVAILABLE,
                "the GPU canvas state is busy; retry the parent-tile fallback",
            )
        })?;
        let draw_state = draw_state_guard.as_mut().ok_or_else(|| {
            failure_with_code(
                ERROR_CODE_RENDERING_SURFACE_UNAVAILABLE,
                "the GPU canvas is not initialized",
            )
        })?;
        draw_state
            .tile_provider
            .preload_tile_payload(parent_tile_key, &parent_payload_bytes);
        let validated_parent_payload = draw_state
            .tile_provider
            .preloaded_payloads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&parent_tile_key)
            .ok_or_else(|| {
                failure_with_code(
                    ERROR_CODE_RESPONSE_PARSING_FAILURE,
                    "the parent tile fixture failed to decode into RGBA pixels",
                )
            })?;
        draw_state
            .tile_scheduler
            .texture_pool_mut()
            .evict(&fallback_tile_key);
        draw_state
            .tile_provider
            .record_unavailable_tile(fallback_tile_key);
        draw_state
            .tile_scheduler
            .texture_pool_mut()
            .insert(parent_tile_key, validated_parent_payload);
        Ok(())
    }

    /// Sets the overlay list drawn over the tiles in every following
    /// [`BrowserMapView::render_frame`], replacing any earlier list in order
    /// (later list entries draw over earlier ones); an empty array clears.
    ///
    /// Each entry is a plain JavaScript object describing one overlay shape,
    /// selected by its `kind` field:
    ///
    /// * `{kind: "routePolyline", coordinates: [{latitude, longitude}...],
    ///   color?, widthPixels?}` — a routed path drawn as a thick line,
    /// * `{kind: "filledPolygon", ringCoordinates: [{latitude, longitude}...],
    ///   color?}` — an ear-clip triangulated ring fill,
    /// * `{kind: "pointMarkers", positions: [{latitude, longitude}...],
    ///   color?, radiusPixels?}` — screen-facing octagon markers,
    /// * `{kind: "pointPins", positions: [{latitude, longitude}...],
    ///   color?, radiusPixels?}` — teardrop pins anchored at their tip with a
    ///   hollow head center,
    /// * `{kind: "trafficFlow", segments: [{shapeCoordinates:
    ///   [{latitude, longitude}...], congestionLevel:
    ///   CanvasTrafficFlowCongestionLevel}...], widthPixels?}` —
    ///   congestion-colored traffic lines.
    ///
    /// An absent `color`/`widthPixels`/`radiusPixels` override keeps the
    /// documented rustic-maps-rendering default style; the fields exist exactly where
    /// the native overlay structs have them. Colors are `{red, green, blue,
    /// alpha}` in `0.0..=1.0`; widths and radii are positive finite screen
    /// pixel values. Every invalid descriptor (wrong shape, missing or
    /// wrong-typed field, out-of-range coordinate or channel, unknown `kind`
    /// or congestion value) rejects the whole call with a coded
    /// `invalidRenderInput` failure and keeps the previous list in force;
    /// nothing panics.
    #[wasm_bindgen(js_name = setOverlays)]
    pub fn set_overlays(&self, overlays: Vec<JsValue>) -> Result<(), JsValue> {
        let mut validated_overlays = Vec::with_capacity(overlays.len());
        for (overlay_index, overlay_value) in overlays.iter().enumerate() {
            validated_overlays.push(
                validated_map_overlay(overlay_value, screen_pixel_ratio(&self.canvas)).map_err(
                    |failure_message| {
                        failure_with_code(
                            ERROR_CODE_INVALID_RENDER_INPUT,
                            &format!("overlay {overlay_index} is invalid: {failure_message}"),
                        )
                    },
                )?,
            );
        }
        let overlay_count = {
            let mut drawn_overlays = self.overlays.borrow_mut();
            if *drawn_overlays != validated_overlays {
                *drawn_overlays = validated_overlays;
                self.bump_overlay_revision();
            }
            drawn_overlays.len()
        };
        // The bulk call replaces the whole drawn list and drops the
        // addressed bookkeeping: an identifier returned by `add_overlay`
        // before this call no longer addresses anything. The identifier
        // sequence keeps counting (identifiers are never reused).
        *self.overlay_identifiers.borrow_mut() =
            (0..overlay_count).map(|_| String::new()).collect();
        Ok(())
    }

    /// Marks the drawn overlay list changed, so the cached world-space
    /// tessellation re-runs on the next rendered frame.
    fn bump_overlay_revision(&self) {
        self.overlay_revision
            .set(self.overlay_revision.get().wrapping_add(1));
    }

    /// Forgets the vector base's last drawn frame and invalidates the cached
    /// overlay tessellation with it.
    ///
    /// The frame's world-space overlay geometry is re-tessellated only when
    /// the overlay revision changes, and the revision otherwise tracks the
    /// page's own overlay list. Installing, replacing or clearing a vector
    /// base changes the composed list without touching that list, so the
    /// frame is dropped here — and the revision bumped — or the previous
    /// base's fills and lines would keep painting over the raster tiles.
    fn invalidate_vector_base_frame(&self) {
        *self.vector_base_last_frame.borrow_mut() = None;
        // A new or dropped source names its own places: the previous source's
        // placements must not hold spots in the new one's frames.
        self.previous_label_placements.borrow_mut().clear();
        self.bump_overlay_revision();
    }

    /// Adds one overlay descriptor to the drawn list and answers the
    /// generated opaque identifier addressing it (`overlay-1`, `overlay-2`,
    /// …; monotonic per view, never reused). Validation is exactly
    /// `set_overlays`' single-descriptor rule: a refused descriptor throws
    /// and adds nothing. The new overlay draws last, over the entries
    /// already in the list.
    #[wasm_bindgen(js_name = addOverlay)]
    pub fn add_overlay(&self, overlay: JsValue) -> Result<String, JsValue> {
        let validated_overlay = validated_map_overlay(&overlay, screen_pixel_ratio(&self.canvas))
            .map_err(|failure_message| {
            failure_with_code(
                ERROR_CODE_INVALID_RENDER_INPUT,
                &format!("overlay is invalid: {failure_message}"),
            )
        })?;
        let identifier_number = self.next_overlay_identifier.get().saturating_add(1);
        let identifier = format!("overlay-{identifier_number}");
        self.overlays.borrow_mut().push(validated_overlay);
        self.bump_overlay_revision();
        self.overlay_identifiers
            .borrow_mut()
            .push(identifier.clone());
        self.next_overlay_identifier.set(identifier_number);
        Ok(identifier)
    }

    /// Replaces the overlay addressed by `identifier` in its draw position.
    /// An invalid descriptor throws and keeps the previous overlay; an
    /// unknown identifier throws an `invalidArgument` failure naming it.
    #[wasm_bindgen(js_name = updateOverlay)]
    pub fn update_overlay(&self, identifier: &str, overlay: JsValue) -> Result<(), JsValue> {
        let validated_overlay = validated_map_overlay(&overlay, screen_pixel_ratio(&self.canvas))
            .map_err(|failure_message| {
            failure_with_code(
                ERROR_CODE_INVALID_RENDER_INPUT,
                &format!("overlay is invalid: {failure_message}"),
            )
        })?;
        let position = self
            .overlay_identifiers
            .borrow()
            .iter()
            .position(|candidate| candidate == identifier)
            .ok_or_else(|| {
                failure_with_code(
                    ERROR_CODE_INVALID_ARGUMENT,
                    &format!("no overlay is addressed by identifier `{identifier}`"),
                )
            })?;
        let mut drawn_overlays = self.overlays.borrow_mut();
        if drawn_overlays[position] != validated_overlay {
            drawn_overlays[position] = validated_overlay;
            self.bump_overlay_revision();
        }
        Ok(())
    }

    /// Removes the overlay addressed by `identifier`; answers whether the
    /// identifier was present (an unknown identifier is not an error).
    #[wasm_bindgen(js_name = removeOverlay)]
    pub fn remove_overlay(&self, identifier: &str) -> bool {
        let Some(position) = self
            .overlay_identifiers
            .borrow()
            .iter()
            .position(|candidate| candidate == identifier)
        else {
            return false;
        };
        self.overlays.borrow_mut().remove(position);
        self.bump_overlay_revision();
        self.overlay_identifiers.borrow_mut().remove(position);
        true
    }

    /// Clears the overlay list; following frames draw the tiles alone until
    /// [`BrowserMapView::set_overlays`] or [`BrowserMapView::add_overlay`] supplies a new
    /// list. Clears the bulk list and every addressed overlay alike.
    #[wasm_bindgen(js_name = clearOverlays)]
    pub fn clear_overlays(&self) {
        let mut drawn_overlays = self.overlays.borrow_mut();
        if !drawn_overlays.is_empty() {
            drawn_overlays.clear();
            self.bump_overlay_revision();
        }
        self.overlay_identifiers.borrow_mut().clear();
    }

    /// Sets the location-indicator layer drawn over the overlays in every
    /// following [`BrowserMapView::render_frame`] (the native surface's top layer),
    /// replacing any earlier layer.
    ///
    /// The state descriptor is `{coordinate: {latitude, longitude},
    /// courseHeadingDegrees?, horizontalAccuracyMeters?}`; the style
    /// descriptor is `{visualKind: "trackingDot" | "guidedCourseArrow",
    /// radiusPixels?, rgbaColor?, showAccuracyRing?}`, where every omitted
    /// style field keeps the documented rustic-maps-rendering default. Colors are
    /// `{red, green, blue, alpha}` in `0.0..=1.0`; the radius is a positive
    /// finite screen-pixel value. The optional scalars pass through verbatim
    /// (the native indicator state's rule) while the style's own range rules
    /// mirror the native record conversion. Every invalid descriptor rejects
    /// the whole call with a coded `invalidRenderInput` failure and keeps the
    /// previous layer in force; nothing panics.
    #[wasm_bindgen(js_name = setLocationIndicator)]
    pub fn set_location_indicator(
        &self,
        state_value: JsValue,
        style_value: JsValue,
    ) -> Result<(), JsValue> {
        let state_object: &Object = state_value.dyn_ref().ok_or_else(|| {
            failure_with_code(
                ERROR_CODE_INVALID_RENDER_INPUT,
                "the location-indicator state must be a plain JavaScript object",
            )
        })?;
        let style_object: &Object = style_value.dyn_ref().ok_or_else(|| {
            failure_with_code(
                ERROR_CODE_INVALID_RENDER_INPUT,
                "the location-indicator style must be a plain JavaScript object",
            )
        })?;
        let validated_state =
            validated_location_indicator_state(state_object).map_err(|failure_message| {
                failure_with_code(ERROR_CODE_INVALID_RENDER_INPUT, &failure_message)
            });
        let mut validated_style =
            validated_location_indicator_style(style_object).map_err(|failure_message| {
                failure_with_code(ERROR_CODE_INVALID_RENDER_INPUT, &failure_message)
            })?;
        // The page declares indicator sizes in CSS pixels; the drawing buffer
        // is in device pixels (M30's high-density canvas).
        let screen_ratio = screen_pixel_ratio(&self.canvas);
        validated_style.radius_pixels = Some(
            validated_style
                .radius_pixels
                .unwrap_or(DEFAULT_LOCATION_DISC_RADIUS_PIXELS)
                * screen_ratio,
        );
        *self.location_indicator.borrow_mut() = Some(LocationIndicatorLayer::new(
            validated_state?,
            validated_style,
        ));
        Ok(())
    }

    /// Clears the location-indicator layer; following frames draw no
    /// indicator until [`BrowserMapView::set_location_indicator`] supplies a
    /// new one.
    #[wasm_bindgen(js_name = clearLocationIndicator)]
    pub fn clear_location_indicator(&self) {
        *self.location_indicator.borrow_mut() = None;
    }

    /// Installs a styled vector base map (the UDL
    /// `MapView.set_vector_tile_source` contract member).
    ///
    /// The descriptor is one plain JavaScript object carrying the UDL
    /// `UrlTemplateVectorTileSource` fields in lower camel case:
    /// `{urlTemplate, sourceName, attributionText, styleId,
    /// minimumZoomLevel, maximumZoomLevel}`, plus the optional
    /// `tokenBucketCapacityTokens` and `refillTokensPerSecond` pacing rates
    /// (absent fields keep the SDK's documented bucket) and the optional
    /// `termsText`, `subdomains`, `headers` and `styleHints` the tileset's
    /// usage policy and template declare (absent fields mean none, and a
    /// header object carries `{name, value}` strings). The template must be
    /// an absolute `http`/`https` URL carrying `{z}`, `{x}` and `{y}`; the
    /// style id must name one of the committed style artifacts
    /// (`bright-light`, `fiord-dark`, …). The whole source builds before
    /// this view changes, so a refused call (a malformed template, an empty
    /// name, an inverted zoom range, an unusable pacing rate or an unknown
    /// style id) throws a coded failure and leaves the previous base in
    /// force.
    ///
    /// While a source is installed the canvas hides the raster base and paints
    /// the style's own background, exactly like the native surface; the
    /// source's overlays and place names draw beneath every overlay the page
    /// supplies. An install that lands while a frame resolves is deferred to
    /// that frame's end and the last request wins, so a style switch never has
    /// to be retried.
    #[wasm_bindgen(js_name = setVectorTileSource)]
    pub fn set_vector_tile_source(&self, source: JsValue) -> Result<(), JsValue> {
        // A new install supersedes any document install still resolving.
        let install_generation = self.begin_vector_base_install();
        let source_object: &Object = source.dyn_ref().ok_or_else(|| {
            failure_with_code(
                ERROR_CODE_INVALID_ARGUMENT,
                "the vector base source must be a plain JavaScript object",
            )
        })?;
        let url_template =
            required_javascript_string_field(source_object, VECTOR_SOURCE_FIELD_URL_TEMPLATE)
                .map_err(|failure_message| {
                    failure_with_code(ERROR_CODE_INVALID_ARGUMENT, &failure_message)
                })?;
        let source_descriptor = VectorTileSourceDescriptor {
            source_name: required_javascript_string_field(
                source_object,
                VECTOR_SOURCE_FIELD_SOURCE_NAME,
            )
            .map_err(|failure_message| {
                failure_with_code(ERROR_CODE_INVALID_ARGUMENT, &failure_message)
            })?,
            attribution_text: required_javascript_string_field(
                source_object,
                VECTOR_SOURCE_FIELD_ATTRIBUTION_TEXT,
            )
            .map_err(|failure_message| {
                failure_with_code(ERROR_CODE_INVALID_ARGUMENT, &failure_message)
            })?,
            minimum_zoom_level: javascript_zoom_level_field(
                source_object,
                VECTOR_SOURCE_FIELD_MINIMUM_ZOOM_LEVEL,
            )?,
            maximum_zoom_level: javascript_zoom_level_field(
                source_object,
                VECTOR_SOURCE_FIELD_MAXIMUM_ZOOM_LEVEL,
            )?,
            style_hints: Vec::new(),
        };
        let style_id =
            required_javascript_string_field(source_object, VECTOR_SOURCE_FIELD_STYLE_ID).map_err(
                |failure_message| failure_with_code(ERROR_CODE_INVALID_ARGUMENT, &failure_message),
            )?;
        let (capacity_tokens, refill_tokens_per_second) = tile_source_budget_rates(
            &source_descriptor.source_name,
            optional_javascript_pacing_rate_field(
                source_object,
                VECTOR_SOURCE_FIELD_TOKEN_BUCKET_CAPACITY_TOKENS,
            )?
            .unwrap_or(
                rustic_maps_core::provider_observation::DEFAULT_TOKEN_BUCKET_CAPACITY_TOKENS,
            ),
            optional_javascript_pacing_rate_field(
                source_object,
                VECTOR_SOURCE_FIELD_REFILL_TOKENS_PER_SECOND,
            )?
            .unwrap_or(rustic_maps_core::provider_observation::DEFAULT_REFILL_TOKENS_PER_SECOND),
        )
        .map_err(|invalid_rate| sdk_error_failure(&invalid_rate))?;
        let source_name = source_descriptor.source_name.clone();
        let source_fields = source_field_value_from_javascript(&source);
        let extras = vector_source_extras(&source_fields).map_err(|failure_message| {
            failure_with_code(ERROR_CODE_INVALID_ARGUMENT, &failure_message)
        })?;
        let url_template_provider = UrlTemplateVectorTileProvider::new(url_template_vector_source(
            VectorSourceDeclarationFields {
                url_template,
                source_name: source_descriptor.source_name,
                attribution_text: source_descriptor.attribution_text,
                minimum_zoom_level: source_descriptor.minimum_zoom_level,
                maximum_zoom_level: source_descriptor.maximum_zoom_level,
                extras,
                pacing: Some(
                    everymap_providers_url_template_vector::TileSourcePacing::new(
                        capacity_tokens,
                        refill_tokens_per_second,
                    )
                    .with_maximum_wait_seconds(
                        rustic_maps_core::everymap_bridge::TILE_SOURCE_MAXIMUM_WAIT_SECONDS,
                    ),
                ),
            },
        ))
        .map_err(|provider_error| sdk_error_failure(&map_provider_error(provider_error)))?;
        let payload_cache = Arc::clone(&self.vector_tile_payload_cache)
            as Arc<dyn rustic_maps_core::CachedResponseStore<geospatial_types::VectorTileData>>;
        let vector_base_scheduler = VectorBaseScheduler::from_style_id(
            Arc::new(EveryMapVectorTileProvider::new(Arc::new(
                url_template_provider,
            ))),
            &style_id,
        )
        .map_err(|style_error| {
            failure_with_code(
                ERROR_CODE_UNSUPPORTED_CAPABILITY,
                &format!(
                    "the vector base source {source_name} names a style the SDK does \
                             not carry: {style_error}"
                ),
            )
        })?
        .with_visible_prefetch_burst_tile_count(
            DEFAULT_VECTOR_BASE_VISIBLE_PREFETCH_BURST_TILE_COUNT,
        )
        .with_payload_cache(
            payload_cache,
            DEFAULT_VECTOR_TILE_PAYLOAD_TIME_TO_LIVE_SECONDS,
        );
        let vector_base_source_summary =
            VectorBaseSourceSummary::from_scheduler(&vector_base_scheduler);
        if self.vector_base_draw_in_flight.get() {
            // The frame owns the scheduler: remember the install and let the
            // frame's restore apply it (a later request replaces an earlier
            // one, and a clear replaces both).
            self.vector_base_clear_requested.set(false);
            *self.pending_vector_base_source.borrow_mut() = Some(PendingVectorBaseSource {
                scheduler: vector_base_scheduler,
                summary: vector_base_source_summary,
                install_generation,
            });
            return Ok(());
        }
        *self.vector_base_source_summary.borrow_mut() = Some(vector_base_source_summary);
        self.invalidate_vector_base_frame();
        *self.vector_base_scheduler.borrow_mut() = Some(vector_base_scheduler);
        Ok(())
    }

    /// Installs a styled vector base map served by a provider configured
    /// through `configureProvider` for the vector-tiles capability (the UDL
    /// `MapView.set_provider_vector_tile_source` contract member).
    ///
    /// The descriptor is one plain JavaScript object carrying the UDL
    /// `ProviderVectorTileSource` fields in lower camel case:
    /// `{providerKey, styleId}`. The provider supplies its own tileset name,
    /// attribution line and zoom span; the page chooses only the provider and
    /// the committed style. A call naming a provider other than the one
    /// configured for vector tiles, made before any vector-tile provider is
    /// configured, or naming an unknown style id throws a coded failure and
    /// leaves the installed base in force. An install that lands while a frame
    /// resolves is deferred to that frame's end, exactly like
    /// [`Self::set_vector_tile_source`].
    #[wasm_bindgen(js_name = setProviderVectorTileSource)]
    pub fn set_provider_vector_tile_source(&self, source: JsValue) -> Result<(), JsValue> {
        // A new install supersedes any document install still resolving.
        let install_generation = self.begin_vector_base_install();
        let (provider_key, style_id) = provider_vector_source_fields(&source)?;
        if let Some(installed_provider_key) = selected_provider_key(ProviderContract::VectorTiles)
            && installed_provider_key != provider_key
        {
            return Err(failure_with_code(
                ERROR_CODE_PROVIDER_NOT_CONFIGURED,
                &format!(
                    "vector tiles from provider {provider_key}; the installed \
                     vector-tile provider is {installed_provider_key}"
                ),
            ));
        }
        let vector_tile_provider = registered_vector_tile_provider().ok_or_else(|| {
            failure_with_code(
                ERROR_CODE_PROVIDER_NOT_CONFIGURED,
                "no vector-tile provider is configured; configure one before installing \
                 a provider-backed base",
            )
        })?;
        let source_name = vector_tile_provider.source_descriptor().source_name;
        let payload_cache = Arc::clone(&self.vector_tile_payload_cache)
            as Arc<dyn rustic_maps_core::CachedResponseStore<geospatial_types::VectorTileData>>;
        let vector_base_scheduler =
            VectorBaseScheduler::from_style_id(vector_tile_provider, &style_id)
                .map_err(|style_error| {
                    failure_with_code(
                        ERROR_CODE_UNSUPPORTED_CAPABILITY,
                        &format!(
                            "the vector base source {source_name} names a style the SDK does \
                     not carry: {style_error}"
                        ),
                    )
                })?
                .with_visible_prefetch_burst_tile_count(
                    DEFAULT_VECTOR_BASE_VISIBLE_PREFETCH_BURST_TILE_COUNT,
                )
                .with_payload_cache(
                    payload_cache,
                    DEFAULT_VECTOR_TILE_PAYLOAD_TIME_TO_LIVE_SECONDS,
                );
        let vector_base_source_summary =
            VectorBaseSourceSummary::from_scheduler(&vector_base_scheduler);
        if self.vector_base_draw_in_flight.get() {
            // The frame owns the scheduler: remember the install and let the
            // frame's restore apply it (a later request replaces an earlier
            // one, and a clear replaces both).
            self.vector_base_clear_requested.set(false);
            *self.pending_vector_base_source.borrow_mut() = Some(PendingVectorBaseSource {
                scheduler: vector_base_scheduler,
                summary: vector_base_source_summary,
                install_generation,
            });
            return Ok(());
        }
        *self.vector_base_source_summary.borrow_mut() = Some(vector_base_source_summary);
        self.invalidate_vector_base_frame();
        *self.vector_base_scheduler.borrow_mut() = Some(vector_base_scheduler);
        Ok(())
    }

    /// The glyph font a provider's style document names for its symbol
    /// layers.
    ///
    /// The font is built from the document's `glyphs` endpoint over the Latin
    /// blocks the label pass packs, fetched through the provider's
    /// authenticated style-resource client and cached per provider, document
    /// address and font stack. A style that names no endpoint or no symbol
    /// font stack, an address that cannot be fetched as written, and a range
    /// that cannot be fetched, decoded or packed all leave the labels on the
    /// SDK's embedded Latin range: a glyph failure degrades labels, never the
    /// base.
    ///
    /// # Parameters
    /// - `provider_key` — the provider the document came from, which names
    ///   the cached font and the glyph requests' namespace.
    /// - `style_document_url` — the document's address, whose origin and
    ///   directory bound every glyph request.
    /// - `vector_base_scheduler` — the scheduler built from the compiled
    ///   document, whose endpoint and symbol font stacks the font is built
    ///   from.
    /// - `install_generation` — the install this font belongs to. A fetch
    ///   that resolves after a clear or a later install returns without
    ///   inserting the font into the provider cache.
    ///
    /// # Errors
    /// Throws a coded refusal when the document's glyph address resolves to a
    /// host that is neither the document's origin nor one the provider
    /// declared, before any request is made.
    async fn hosted_label_font(
        &self,
        provider_key: &str,
        style_document_url: &str,
        vector_base_scheduler: &VectorBaseScheduler,
        install_generation: u64,
    ) -> Result<Option<Arc<LabelFont>>, JsValue> {
        let style = vector_base_scheduler.style();
        let Some(glyph_url_template) = style.glyphs.as_deref() else {
            return Ok(None);
        };
        let Some(font_stack_name) = style.symbol_font_stacks().into_iter().next() else {
            return Ok(None);
        };
        let cache_key = (
            provider_key.to_string(),
            style_document_url.to_string(),
            font_stack_name.clone(),
        );
        let cached_label_font = self.provider_label_fonts.borrow().get(&cache_key).cloned();
        if let Some(cached_label_font) = cached_label_font {
            return Ok(Some(cached_label_font));
        }
        let Some(style_resources) = registered_vector_style_resources() else {
            return Ok(None);
        };
        let declared_hosts = registered_vector_style_resource_hosts();
        let mut hosted_glyph_ranges = match HostedGlyphRanges::new(
            style_resources,
            provider_key,
            style_document_url,
            glyph_url_template,
            &declared_hosts,
        ) {
            Ok(hosted_glyph_ranges) => hosted_glyph_ranges,
            Err(geospatial_types::SDKError::ResponseParsingFailure(_)) => return Ok(None),
            Err(refusal) => return Err(sdk_error_failure(&refusal)),
        };
        let font_stack_result = hosted_glyph_ranges.latin_font_stack(&font_stack_name).await;
        if !self.vector_base_install_is_current(install_generation) {
            // The install this font belongs to was superseded while the
            // glyph endpoint answered; the cache must not learn about it.
            return Ok(None);
        }
        let Ok(font_stack) = font_stack_result else {
            return Ok(None);
        };
        let Ok(label_font) = LabelFont::from_glyph_font_stack(&font_stack) else {
            return Ok(None);
        };
        let label_font = Arc::new(label_font);
        self.provider_label_fonts
            .borrow_mut()
            .insert(cache_key, Arc::clone(&label_font));
        Ok(Some(label_font))
    }

    /// The decoded sprite atlas a provider's style document names.
    ///
    /// The atlas is resolved and fetched through the same renderer seam the
    /// native surface uses, so both install paths apply one address and
    /// credential rule, and it is cached per provider, document address and
    /// sprite base. A style without a sprite and a sprite the install could
    /// not fetch or decode draw their text without icons.
    ///
    /// # Parameters
    /// - `provider_key` — the provider the document came from.
    /// - `style_document_url` — the document's address, whose origin and
    ///   directory bound both sprite requests.
    /// - `vector_base_scheduler` — the scheduler built from the compiled
    ///   document, whose `sprite` base names the sheet.
    /// - `install_generation` — the install this atlas belongs to. A fetch
    ///   that resolves after a clear or a later install returns without
    ///   inserting the atlas into the provider cache.
    ///
    /// # Returns
    /// The sprite base and its atlas, or `None` when the style declares no
    /// sprite or the sheet could not be resolved, fetched or decoded.
    ///
    /// # Errors
    /// Throws a coded refusal when the sprite base resolves to a host that is
    /// neither the document's origin nor one the provider declared, before any
    /// request is made.
    async fn hosted_sprite_atlas(
        &self,
        provider_key: &str,
        style_document_url: &str,
        vector_base_scheduler: &VectorBaseScheduler,
        install_generation: u64,
    ) -> Result<Option<(String, Arc<SpriteAtlas>)>, JsValue> {
        let Some(sprite) = vector_base_scheduler.style().sprite.as_ref() else {
            return Ok(None);
        };
        let sprite_url = sprite.url.clone();
        let cache_key = (
            provider_key.to_string(),
            style_document_url.to_string(),
            sprite_url.clone(),
        );
        let cached_sprite_atlas = self
            .provider_sprite_atlases
            .borrow()
            .get(&cache_key)
            .cloned();
        if let Some(cached_sprite_atlas) = cached_sprite_atlas {
            return Ok(Some((sprite_url, cached_sprite_atlas)));
        }
        let Some(style_resources) = registered_vector_style_resources() else {
            return Ok(None);
        };
        let declared_hosts = registered_vector_style_resource_hosts();
        let fetched_sprite_atlas = fetch_hosted_sprite_atlas(
            style_resources,
            style_document_url,
            &sprite_url,
            &declared_hosts,
        )
        .await;
        if !self.vector_base_install_is_current(install_generation) {
            // The install this atlas belongs to was superseded while the
            // sprite endpoint answered; a refusal must not surface and the
            // cache must not learn about it.
            return Ok(None);
        }
        let sprite_atlas = match fetched_sprite_atlas {
            Ok(sprite_atlas) => Arc::new(sprite_atlas),
            Err(refusal @ geospatial_types::SDKError::ProviderRequestRejected(_)) => {
                return Err(sdk_error_failure(&refusal));
            }
            Err(_) => return Ok(None),
        };
        self.provider_sprite_atlases
            .borrow_mut()
            .insert(cache_key, Arc::clone(&sprite_atlas));
        Ok(Some((sprite_url, sprite_atlas)))
    }

    /// Installs a provider-backed base painted with the style document the
    /// provider publishes (`MapView.set_provider_vector_tile_source_from_document`'s
    /// contract member). The document is fetched through the configured
    /// provider's own authenticated client, compiled once and kept per
    /// provider and address, so installing the same family again costs no
    /// request. A failed fetch or an uncompilable document throws a coded
    /// failure and leaves the installed base in force.
    ///
    /// # Parameters
    /// - `source` — `{providerKey, styleId}`.
    /// - `style_document_url` — the absolute address of the provider's style
    ///   document, without a credential.
    #[wasm_bindgen(js_name = setProviderVectorTileSourceFromDocument)]
    pub async fn set_provider_vector_tile_source_from_document(
        &self,
        source: JsValue,
        style_document_url: String,
    ) -> Result<(), JsValue> {
        // A document install takes the generation at entry, so a clear or a
        // later install that lands during its awaits supersedes it.
        let install_generation = self.begin_vector_base_install();
        self.vector_base_document_installs_in_flight.set(
            self.vector_base_document_installs_in_flight
                .get()
                .saturating_add(1),
        );
        let install_outcome = self
            .install_provider_vector_tile_source_from_document(
                source,
                style_document_url,
                install_generation,
            )
            .await;
        self.vector_base_document_installs_in_flight.set(
            self.vector_base_document_installs_in_flight
                .get()
                .saturating_sub(1),
        );
        install_outcome
    }

    /// The body of [`Self::set_provider_vector_tile_source_from_document`],
    /// carrying the install generation every resumption checks against.
    async fn install_provider_vector_tile_source_from_document(
        &self,
        source: JsValue,
        style_document_url: String,
        install_generation: u64,
    ) -> Result<(), JsValue> {
        let (provider_key, style_id) = provider_vector_source_fields(&source)?;
        let style_document_url = absolute_style_document_url(&style_document_url);
        if let Some(installed_provider_key) = selected_provider_key(ProviderContract::VectorTiles)
            && installed_provider_key != provider_key
        {
            return Err(failure_with_code(
                ERROR_CODE_PROVIDER_NOT_CONFIGURED,
                &format!(
                    "vector tiles from provider {provider_key}; the installed \
                     vector-tile provider is {installed_provider_key}"
                ),
            ));
        }
        let vector_tile_provider = registered_vector_tile_provider().ok_or_else(|| {
            failure_with_code(
                ERROR_CODE_PROVIDER_NOT_CONFIGURED,
                "no vector-tile provider is configured; configure one before installing \
                 a provider-backed base",
            )
        })?;
        let style_resources = registered_vector_style_resources().ok_or_else(|| {
            failure_with_code(
                ERROR_CODE_PROVIDER_NOT_CONFIGURED,
                "the vector-tile provider registers no style-resource fetch; installing the \
                 provider's own style document needs one",
            )
        })?;
        let source_name = vector_tile_provider.source_descriptor().source_name;
        let document_key = (provider_key.clone(), style_document_url.clone());
        let cached_document = self
            .provider_style_documents
            .borrow()
            .get(&document_key)
            .cloned();
        let style_document = match cached_document {
            Some(style_document) => style_document,
            None => {
                let fetched_style_document = style_resources
                    .fetch_style_resource(&style_document_url)
                    .await;
                if !self.vector_base_install_is_current(install_generation) {
                    // A clear or a later install landed while the document
                    // downloaded: its fetch failure or payload is not this
                    // view's to report or install.
                    return Ok(());
                }
                let document_bytes = fetched_style_document
                    .map_err(|provider_error| sdk_error_failure(&provider_error))?;
                String::from_utf8(document_bytes).map_err(|utf8_error| {
                    failure_with_code(
                        ERROR_CODE_RESPONSE_PARSING_FAILURE,
                        &format!(
                            "the style document the vector base source {source_name} served \
                             is not UTF-8 text: {utf8_error}"
                        ),
                    )
                })?
            }
        };
        if !self.vector_base_install_is_current(install_generation) {
            return Ok(());
        }
        let payload_cache = Arc::clone(&self.vector_tile_payload_cache)
            as Arc<dyn rustic_maps_core::CachedResponseStore<geospatial_types::VectorTileData>>;
        let vector_base_scheduler = VectorBaseScheduler::from_style_document_json(
            vector_tile_provider,
            &style_id,
            &style_document,
        )
        .map_err(|style_error| {
            failure_with_code(
                ERROR_CODE_RESPONSE_PARSING_FAILURE,
                &format!(
                    "the style document the vector base source {source_name} served does \
                     not compile: {style_error}"
                ),
            )
        })?
        .with_visible_prefetch_burst_tile_count(
            DEFAULT_VECTOR_BASE_VISIBLE_PREFETCH_BURST_TILE_COUNT,
        )
        .with_payload_cache(
            payload_cache,
            DEFAULT_VECTOR_TILE_PAYLOAD_TIME_TO_LIVE_SECONDS,
        );
        // The style's own labels draw through the glyph endpoint it declares,
        // fetched with the provider's credential and cached per document.
        let hosted_label_font = self
            .hosted_label_font(
                &provider_key,
                &style_document_url,
                &vector_base_scheduler,
                install_generation,
            )
            .await?;
        if !self.vector_base_install_is_current(install_generation) {
            return Ok(());
        }
        let vector_base_scheduler = match hosted_label_font {
            Some(label_font) => vector_base_scheduler.with_label_font(label_font),
            None => vector_base_scheduler,
        };
        // The style's own icons draw from the sprite it declares, fetched and
        // decoded the same way; a style without a sprite, or one the install
        // could not complete, draws its text without icons.
        let hosted_sprite_atlas = self
            .hosted_sprite_atlas(
                &provider_key,
                &style_document_url,
                &vector_base_scheduler,
                install_generation,
            )
            .await?;
        if !self.vector_base_install_is_current(install_generation) {
            return Ok(());
        }
        let vector_base_scheduler = match hosted_sprite_atlas {
            Some((sprite_url, sprite_atlas)) => {
                vector_base_scheduler.with_prefetched_sprite_atlas(sprite_url, sprite_atlas)
            }
            None => vector_base_scheduler,
        };
        // Remember the document only after it compiled, so a malformed
        // document is fetched again rather than remembered as usable.
        self.provider_style_documents
            .borrow_mut()
            .insert(document_key, style_document);
        let vector_base_source_summary =
            VectorBaseSourceSummary::from_scheduler(&vector_base_scheduler);
        if self.vector_base_draw_in_flight.get() {
            self.vector_base_clear_requested.set(false);
            *self.pending_vector_base_source.borrow_mut() = Some(PendingVectorBaseSource {
                scheduler: vector_base_scheduler,
                summary: vector_base_source_summary,
                install_generation,
            });
            return Ok(());
        }
        *self.vector_base_source_summary.borrow_mut() = Some(vector_base_source_summary);
        self.invalidate_vector_base_frame();
        *self.vector_base_scheduler.borrow_mut() = Some(vector_base_scheduler);
        Ok(())
    }

    /// Drops the canvas' pooled raster state and re-plans the next frame's
    /// fetches from the current raster sources (the UDL
    /// `MapView.reload_tile_source` contract member, served on the canvas).
    ///
    /// The scheduler, its payload pool, the staging provider's staged and
    /// decoded payloads, the uploaded textures and the retained cross-fade
    /// level are replaced or cleared, so a source change reaches the next
    /// frame without constructing a new canvas and no payload fetched from
    /// the previous source draws again. Overlays, labels, the vector base and
    /// the location layer survive the reload; fixtures and the registered
    /// live sources are consulted afresh on the next tile pass.
    ///
    /// A call made while a frame owns the draw state is applied before the
    /// next frame's tile pass. A call before initialization is a no-op.
    #[wasm_bindgen(js_name = reloadTileSource)]
    pub fn reload_tile_source(&self) {
        match self.draw_state.try_borrow_mut() {
            Ok(mut draw_state) => {
                if let Some(draw_state) = draw_state.as_mut() {
                    self.replace_raster_tile_state(draw_state);
                }
                self.raster_reload_requested.set(false);
            }
            Err(_) => {
                // A frame awaits with the draw state borrowed: its next run
                // applies the reload before resolving tiles.
                self.raster_reload_requested.set(true);
            }
        }
    }

    /// Removes the vector base map (the UDL
    /// `MapView.clear_vector_tile_source` contract member): following frames
    /// draw raster tiles and overlays alone. A clear that lands while a frame
    /// resolves is deferred to that frame's end, like installing, and it
    /// cancels an install that was waiting.
    #[wasm_bindgen(js_name = clearVectorTileSource)]
    pub fn clear_vector_tile_source(&self) -> Result<(), JsValue> {
        // A clear supersedes every install in flight: a document that resumes
        // after this call sees an older generation and returns without
        // installing anything.
        let _ = self.begin_vector_base_install();
        // A clear after a pending install wins: the source that would have
        // replaced the current one is dropped, not installed later.
        *self.pending_vector_base_source.borrow_mut() = None;
        *self.vector_base_source_summary.borrow_mut() = None;
        self.invalidate_vector_base_frame();
        if self.vector_base_draw_in_flight.get() {
            // The frame owns the scheduler; its restore drops it instead of
            // writing it back, so the clear lands before the next frame.
            self.vector_base_clear_requested.set(true);
            return Ok(());
        }
        *self.vector_base_scheduler.borrow_mut() = None;
        self.vector_base_clear_requested.set(false);
        Ok(())
    }

    /// Reads the vector base source's status (the UDL
    /// `MapView.vector_tile_source_status` contract member): the configured
    /// source's style, name, attribution and zoom ceiling plus the last
    /// resolved frame's drawn, pending and failed tile counts. Every status
    /// shape also carries `pendingVectorInstall`, which reports whether a
    /// document install is still resolving. While a source is configured, the
    /// object additionally carries the frame diagnostics — payload-cache hits
    /// and misses, in-flight fetches and the first-drawn-tile latency — as
    /// additive fields. A pure query — it never fetches or draws — and it
    /// answers an honest not-configured status with no source installed.
    #[wasm_bindgen(js_name = vectorTileSourceStatus)]
    pub fn vector_tile_source_status(&self) -> Result<Object, JsValue> {
        let source_summary = self
            .vector_base_source_summary
            .borrow()
            .clone()
            .unwrap_or_else(VectorBaseSourceSummary::not_configured);
        let is_configured = self.vector_base_source_summary.borrow().is_some();
        let last_frame_diagnostics = self
            .vector_base_last_frame
            .borrow()
            .as_ref()
            .map(|vector_base_frame| vector_base_frame.diagnostics);
        let (
            drawn_tile_count,
            pending_tile_count,
            fallback_tile_count,
            failed_tile_count,
            last_failure_detail,
        ) = self.vector_base_last_frame.borrow().as_ref().map_or(
            (0, 0, 0, 0, None),
            |vector_base_frame| {
                (
                    u32::try_from(vector_base_frame.drawn_tile_count).unwrap_or(u32::MAX),
                    u32::try_from(vector_base_frame.pending_tile_count).unwrap_or(u32::MAX),
                    u32::try_from(vector_base_frame.fallback_tile_count).unwrap_or(u32::MAX),
                    u32::try_from(vector_base_frame.failed_tile_count).unwrap_or(u32::MAX),
                    vector_base_frame.last_failure_detail.clone(),
                )
            },
        );
        let source_layer_counts = self
            .vector_base_last_frame
            .borrow()
            .as_ref()
            .map(|vector_base_frame| vector_base_frame.source_layer_counts.clone())
            .unwrap_or_default();
        let mut fields: Vec<(&str, JsValue)> = vec![
            (
                VECTOR_STATUS_FIELD_IS_CONFIGURED,
                JsValue::from(is_configured),
            ),
            (
                VECTOR_STATUS_FIELD_PENDING_VECTOR_INSTALL,
                JsValue::from(self.vector_base_document_installs_in_flight.get() > 0),
            ),
            (
                VECTOR_STATUS_FIELD_STYLE_ID,
                JsValue::from_str(&source_summary.style_id),
            ),
            (
                VECTOR_STATUS_FIELD_SOURCE_NAME,
                JsValue::from_str(&source_summary.source_name),
            ),
            (
                VECTOR_STATUS_FIELD_ATTRIBUTION_TEXT,
                JsValue::from_str(&source_summary.attribution_text),
            ),
            (
                VECTOR_STATUS_FIELD_MAXIMUM_ZOOM_LEVEL,
                JsValue::from(source_summary.maximum_zoom_level),
            ),
            (
                VECTOR_STATUS_FIELD_UNSUPPORTED_PROPERTY_COUNT,
                JsValue::from(source_summary.unsupported_property_count),
            ),
            (
                VECTOR_STATUS_FIELD_UNSUPPORTED_PROPERTY_NAMES,
                string_values_array(&source_summary.unsupported_property_names),
            ),
            (
                VECTOR_STATUS_FIELD_DEGRADED_ASSET_NAMES,
                string_values_array(&source_summary.degraded_asset_names),
            ),
            (
                VECTOR_STATUS_FIELD_SOURCE_LAYER_COUNTS,
                JsValue::from(source_layer_counts_array(&source_layer_counts)?),
            ),
            (
                VECTOR_STATUS_FIELD_DRAWN_TILE_COUNT,
                JsValue::from(drawn_tile_count),
            ),
            (
                VECTOR_STATUS_FIELD_FAILED_TILE_COUNT,
                JsValue::from(failed_tile_count),
            ),
            (
                VECTOR_STATUS_FIELD_LAST_FAILURE_DETAIL,
                last_failure_detail
                    .as_deref()
                    .map_or(JsValue::NULL, JsValue::from_str),
            ),
        ];
        if is_configured {
            let diagnostics = last_frame_diagnostics.unwrap_or_default();
            fields.push((
                VECTOR_STATUS_FIELD_PAYLOAD_CACHE_HIT_COUNT,
                JsValue::from(
                    u32::try_from(diagnostics.payload_cache_hit_count).unwrap_or(u32::MAX),
                ),
            ));
            fields.push((
                VECTOR_STATUS_FIELD_PAYLOAD_CACHE_MISS_COUNT,
                JsValue::from(
                    u32::try_from(diagnostics.payload_cache_miss_count).unwrap_or(u32::MAX),
                ),
            ));
            fields.push((
                VECTOR_STATUS_FIELD_IN_FLIGHT_FETCH_COUNT,
                JsValue::from(u32::try_from(diagnostics.in_flight_fetch_count).unwrap_or(u32::MAX)),
            ));
            fields.push((
                VECTOR_STATUS_FIELD_PENDING_TILE_COUNT,
                JsValue::from(pending_tile_count),
            ));
            fields.push((
                VECTOR_STATUS_FIELD_FALLBACK_TILE_COUNT,
                JsValue::from(fallback_tile_count),
            ));
            fields.push((
                VECTOR_STATUS_FIELD_FIRST_DRAWN_TILE_MILLISECONDS,
                diagnostics
                    .time_to_first_drawn_tile_milliseconds
                    .map_or(JsValue::NULL, |milliseconds| {
                        JsValue::from(u32::try_from(milliseconds).unwrap_or(u32::MAX))
                    }),
            ));
        }
        object_from_fields(&fields)
    }

    /// Takes the vector base scheduler out of its slot for one frame's
    /// resolve, marking the draw in flight; `None` when no source is
    /// installed. The restore writes it back only when this frame still owns
    /// the slot (nothing can install during the mark, because installing
    /// refuses while the mark is set).
    fn take_vector_base_scheduler(&self) -> Option<VectorBaseScheduler> {
        let vector_base_scheduler = self.vector_base_scheduler.borrow_mut().take();
        if vector_base_scheduler.is_some() {
            self.vector_base_draw_in_flight.set(true);
        }
        vector_base_scheduler
    }

    /// Restores the scheduler a frame took out and clears the in-flight mark.
    fn restore_vector_base_scheduler(&self, vector_base_scheduler: Option<VectorBaseScheduler>) {
        if self.vector_base_clear_requested.get() {
            // A clear landed mid-frame: drop what the frame borrowed rather
            // than writing it back, so the next frame draws no vector base.
            self.vector_base_clear_requested.set(false);
            self.invalidate_vector_base_frame();
        } else if let Some(vector_base_scheduler) = vector_base_scheduler {
            *self.vector_base_scheduler.borrow_mut() = Some(vector_base_scheduler);
        }
        // An install requested mid-frame is applied now, after the drawn
        // scheduler is back in its slot (or dropped), so the next frame draws
        // the requested source and nothing of the previous one.
        if let Some(pending_vector_base_source) =
            self.pending_vector_base_source.borrow_mut().take()
        {
            // A clear or a later install bumped the generation after this
            // source was queued: it is superseded and must not be applied.
            if pending_vector_base_source.install_generation
                == self.vector_base_install_generation.get()
            {
                *self.vector_base_source_summary.borrow_mut() =
                    Some(pending_vector_base_source.summary);
                self.invalidate_vector_base_frame();
                *self.vector_base_scheduler.borrow_mut() =
                    Some(pending_vector_base_source.scheduler);
            }
        }
        self.vector_base_draw_in_flight.set(false);
    }

    /// Starts a vector-base install: bumps the monotonic install generation
    /// and returns the value this install carries. A clear or a later install
    /// bumps it again, so an await that resumes with an older value is
    /// superseded.
    fn begin_vector_base_install(&self) -> u64 {
        let install_generation = self.vector_base_install_generation.get().wrapping_add(1);
        self.vector_base_install_generation.set(install_generation);
        install_generation
    }

    /// Whether the install that captured `install_generation` is still the
    /// view's current request.
    fn vector_base_install_is_current(&self, install_generation: u64) -> bool {
        self.vector_base_install_generation.get() == install_generation
    }

    /// Shows or hides the raster tile base layer in every following
    /// [`BrowserMapView::render_frame`].
    ///
    /// Hidden frames still clear the surface and draw the overlay and
    /// location-indicator layers, so the page can paint a vector base map
    /// through [`BrowserMapView::set_overlays`] while this raster layer is
    /// hidden. A hidden frame skips the tile listing, same-origin fetches,
    /// scheduler warm/resolve, texture uploads and tile draws, and reports
    /// `requestedTiles: 0`, `drawnTiles: 0` and `missingTiles: 0` while
    /// keeping the current `tileZoomLevel`, `tileResolution` and
    /// `overlayTriangles` statistics. Showing the layer again resolves tiles
    /// normally on the next frame (the texture cache survives hidden frames).
    #[wasm_bindgen(js_name = setBaseMapVisible)]
    pub fn set_tile_layer_visible(&self, visible: bool) {
        self.tile_layer_visible.set(visible);
    }

    /// Sets the frame pass's clear color (each channel 0..=1), the background
    /// behind any tiles or overlays that do not cover the surface. The
    /// default is the shared dark clear; the page paints a light clear around
    /// a vector base so the gaps between its fills do not read as a black
    /// map. A channel outside 0..=1 or a non-finite value is refused.
    #[wasm_bindgen(js_name = setBackgroundColor)]
    pub fn set_clear_color(
        &self,
        red: f64,
        green: f64,
        blue: f64,
        alpha: f64,
    ) -> Result<(), JsValue> {
        let channels = [red, green, blue, alpha];
        if channels
            .iter()
            .any(|channel| !channel.is_finite() || !(0.0..=1.0).contains(channel))
        {
            return Err(failure_with_code(
                ERROR_CODE_INVALID_ARGUMENT,
                "clear color channels must be finite numbers between 0 and 1",
            ));
        }
        self.clear_color.set(channels);
        Ok(())
    }

    /// Reads the surface texture at the canvas' current extent,
    /// reconfiguring the surface first when the element was resized and on a
    /// stale or lost surface between attempts.
    fn acquire_surface_texture(
        &self,
        draw_state: &mut CanvasDrawState,
    ) -> Result<SurfaceTexture, String> {
        let extent = clamped_surface_extent(
            (self.canvas.width(), self.canvas.height()),
            draw_state.adapter.limits().max_texture_dimension_2d,
        );
        self.configure_surface_for_extent(draw_state, extent, false)?;
        for attempt_index in 0..SURFACE_ACQUISITION_ATTEMPTS {
            match self.surface.get_current_texture() {
                CurrentSurfaceTexture::Success(surface_texture)
                | CurrentSurfaceTexture::Suboptimal(surface_texture) => {
                    return Ok(surface_texture);
                }
                // Outdated, Lost and Validation mean a stale or dead surface
                // configuration is at fault, so the last attempt reconfigures
                // (see the `wgpu` guidance on each) and tries again.
                CurrentSurfaceTexture::Outdated
                | CurrentSurfaceTexture::Lost
                | CurrentSurfaceTexture::Validation
                    if attempt_index + 1 < SURFACE_ACQUISITION_ATTEMPTS =>
                {
                    self.configure_surface_for_extent(draw_state, extent, true)?;
                }
                status_variant => {
                    return Err(format!(
                        "the map surface could not provide a frame texture: \
                         {status_variant:?} at extent {extent:?}"
                    ));
                }
            }
        }
        Err(format!(
            "the map surface was never reconfigured into a usable state at \
             extent {extent:?}"
        ))
    }

    /// Configures the surface (overriding the format in particular, which
    /// `get_default_config` leaves at the adapter's first choice) for a
    /// canvas extent, skipping the call when the extent is unchanged unless
    /// `force_reconfiguration` asks for a stale-surface reset. Fails at a
    /// zero extent: configuring an empty surface is invalid, so the caller
    /// gets a clean error instead.
    fn configure_surface_for_extent(
        &self,
        draw_state: &mut CanvasDrawState,
        extent: (u32, u32),
        force_reconfiguration: bool,
    ) -> Result<(), String> {
        if extent == (0, 0) {
            return Err("the map canvas has no pixels yet; skip the frame".to_string());
        }
        if !force_reconfiguration && extent == draw_state.configured_extent {
            return Ok(());
        }
        let mut configuration = self
            .surface
            .get_default_config(&draw_state.adapter, extent.0, extent.1)
            .ok_or_else(|| {
                format!(
                    "the canvas supports none of the surface formats \
                     {SURFACE_FORMAT_PREFERENCE:?}"
                )
            })?;
        configuration.format = draw_state.surface_format;
        configuration.present_mode = wgpu::PresentMode::Fifo;
        configuration.alpha_mode = wgpu::CompositeAlphaMode::Opaque;
        configuration.color_space = wgpu::SurfaceColorSpace::Auto;
        configuration.desired_maximum_frame_latency = DESIRED_MAXIMUM_FRAME_LATENCY;
        self.surface.configure(&draw_state.device, &configuration);
        draw_state.configured_extent = extent;
        Ok(())
    }
}
