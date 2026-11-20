//! GPU map rendering: camera model, raster tile scheduling with parent
//! fallback and a texture pool, vector-tile styling (fill, line, circle and
//! symbol layers), overlays, and location indicators.
//!
//! The crate separates scene composition from GPU execution. Projection,
//! camera math, tile scheduling, overlay and label assembly, and the quad
//! assembly in [`renderer`] are free of `wgpu` types, so they build and run on
//! every target, including WebAssembly. The native [`headless`] and
//! [`surface`] renderers consume that composition and submit it to a `wgpu`
//! device. On WebAssembly the host page drives WebGPU and consumes the same
//! vertex and style contracts through the wasm bindings.

pub mod camera;
pub mod camera_interaction;
pub mod geojson_source;
#[cfg(not(target_arch = "wasm32"))]
pub mod gpu_texture_cache;
#[cfg(not(target_arch = "wasm32"))]
pub mod headless;
pub mod hosted_glyphs;
pub mod hosted_sprites;
pub mod image_limits;
pub mod labels;
#[cfg(test)]
#[allow(dead_code)]
mod legacy_style_parity;
/// The pre-compiler style parser, retained only for the migration parity test.
/// Nothing in a non-test build compiles this module.
#[cfg(test)]
#[allow(dead_code)]
mod legacy_vector_style;
pub mod location_indicator;
pub mod overlay;
pub mod prefetch;
pub mod projection;
pub mod renderer;
pub mod source_binding;
pub mod sprite;
mod style_resource_address;
#[cfg(not(target_arch = "wasm32"))]
pub mod surface;
pub mod tile_key;
pub mod tile_scheduler;
pub mod tile_texture_pool;
pub mod vector_base;
pub mod vector_style;
pub mod zoom_transition;

pub use camera::{
    CameraAnimation, DEFAULT_FIELD_OF_VIEW_DEGREES, EdgeInsetsPixels, FlyToOptions,
    MAXIMUM_PITCH_DEGREES, MAXIMUM_ZOOM_LEVEL, MINIMUM_ZOOM_LEVEL, MapCamera, ViewportPixels,
    camera_geographic_bounds, fit_bounds, fly_to_animation, interpolate_camera,
};
pub use camera_interaction::CameraInteraction;
pub use geojson_source::{
    DEFAULT_GEOJSON_BUFFER_PIXELS, DEFAULT_GEOJSON_SIMPLIFICATION_TOLERANCE,
    DEFAULT_GEOJSON_TILING_EXTENT, GEOJSON_MAXIMUM_MERCATOR_LATITUDE_DEGREES,
    GEOJSON_TILE_LAYER_NAME, GeoJsonTilingError, GeoJsonTilingOptions, tile_geojson_for_tile,
    tile_geojson_for_tiles,
};
#[cfg(not(target_arch = "wasm32"))]
pub use gpu_texture_cache::{GPU_TILE_TEXTURE_CACHE_DEFAULT_MAXIMUM_BYTES, GpuTileTextureCache};
#[cfg(not(target_arch = "wasm32"))]
pub use headless::{
    CapturedFrame, GraphicsContext, GraphicsContextError, GraphicsContextSource,
    HeadlessRenderError, HeadlessTileRenderer, WgpuGraphicsContextSource, capture_frame_bytes,
};
pub use hosted_glyphs::{
    GLYPH_URL_FONT_STACK_PLACEHOLDER, GLYPH_URL_RANGE_PLACEHOLDER, HostedGlyphRanges,
};
pub use hosted_sprites::{
    SPRITE_INDEX_URL_SUFFIX, SPRITE_SHEET_URL_SUFFIX, fetch_hosted_sprite_atlas,
};
pub use labels::{
    ClipSpaceGlyphVertex, ClipSpaceIconVertex, DEFAULT_LABEL_RANK,
    DEFAULT_LATIN_GLYPH_RANGE_PBF_BYTES, GLYPH_EDGE_GAMMA_DEVICE_PIXELS,
    GLYPH_FONT_ASCENDER_PIXELS, GLYPH_FONT_EM_PIXELS,
    GLYPH_SIGNED_DISTANCE_FIELD_MAXIMUM_OUTWARD_PIXELS,
    GLYPH_SIGNED_DISTANCE_FIELD_PIXELS_PER_UNIT, GLYPH_TEXT_SIGNED_DISTANCE_THRESHOLD,
    ICON_OFFSET_EM_PIXELS, IconQuadVertices, LABEL_BASELINE_OFFSET_FRACTION,
    LABEL_COLLISION_PADDING_PIXELS, LABEL_RANK_ATTRIBUTE_NAME, LINE_LABEL_REPEAT_SPACING_PIXELS,
    LINE_LABEL_STRAIGHT_RUN_ANGLE_TOLERANCE_RADIANS, LabelFont, LabelLayer, LabelQuadVertices,
    LabelTextMetrics, MINIMUM_ICON_SCALE, MapIconCandidate, MapLabelCandidate, PlacedIcon,
    PlacedLabel, default_label_font, glyph_edge_gamma, glyph_halo_signed_distance_threshold,
    label_candidates_for_compiled_layer_at_zoom, label_candidates_for_vector_tile,
};
pub use location_indicator::{
    DEFAULT_ACCURACY_RING_RGBA, DEFAULT_LOCATION_DISC_RADIUS_PIXELS, DEFAULT_LOCATION_DISC_RGBA,
    DEFAULT_LOCATION_HALO_RGBA, DEFAULT_LOCATION_RING_RGBA, LOCATION_ACCURACY_RING_FAN_SIDES,
    LOCATION_DISC_FAN_SIDES, LOCATION_INDICATOR_TOP_LAYER_DRAW_ORDER,
    LOCATION_NAVIGATION_DART_BASE_HALF_WIDTH_RATIO, LOCATION_NAVIGATION_DART_BASE_OFFSET_RATIO,
    LOCATION_NAVIGATION_DART_NOTCH_OFFSET_RATIO, LOCATION_NAVIGATION_DART_OUTLINE_WIDTH_PIXELS,
    LOCATION_NAVIGATION_DART_TIP_RADIUS_RATIO, LOCATION_PUCK_HALO_WIDTH_PIXELS,
    LOCATION_PUCK_RING_WIDTH_PIXELS, LocationIndicatorLayer, LocationIndicatorState,
    LocationIndicatorStyle, LocationIndicatorVisualKind, location_indicator_vertices,
};
pub use overlay::{
    ClipSpaceOverlayVertex, DEFAULT_FILLED_POLYGON_RGBA, DEFAULT_POINT_MARKER_RADIUS_PIXELS,
    DEFAULT_POINT_MARKER_RGBA, DEFAULT_POINT_PIN_RADIUS_PIXELS, DEFAULT_POINT_PIN_RGBA,
    DEFAULT_ROUTE_LINE_RGBA, DEFAULT_ROUTE_LINE_WIDTH_PIXELS, DEFAULT_TRAFFIC_LINE_WIDTH_PIXELS,
    FilledPolygonOverlay, MapOverlay, OVERLAY_TRIANGLE_VERTEX_COUNT, PointMarkersOverlay,
    PointPinsOverlay, PolylineStroke, RoutePolylineOverlay, TRAFFIC_BLOCKED_RGBA,
    TRAFFIC_CONGESTED_RGBA, TRAFFIC_FREE_FLOW_RGBA, TRAFFIC_SLOW_RGBA, TrafficFlowOverlay,
    WORLD_SPACE_OVERLAY_WORLD_COPY_INSTANCES, WorldSpaceOverlayFrameUniforms,
    WorldSpaceOverlayGeometry, WorldSpaceOverlayVertex, overlay_triangle_vertices,
    overlays_require_camera_space_strokes, traffic_congestion_rgba,
};
pub use prefetch::{
    PREFETCH_FALLBACK_ZOOM_BELOW, PREFETCH_MAXIMUM_CONCURRENT_FETCHES, PREFETCH_RING_TILES,
    TilePrefetchPlanner,
};
pub use projection::{
    MAXIMUM_MERCATOR_LATITUDE_DEGREES, MAXIMUM_TILE_ZOOM_LEVEL, MERCATOR_TILE_SIZE_PIXELS,
    coordinate_to_normalized_world, normalized_world_to_coordinate, tile_key_for_coordinate,
    tiles_covering, zoom_scale_pixels,
};
pub use renderer::{
    CLEAR_COLOR_RGBA, ClipSpaceTileVertex, Renderer, RetainedTileLevel, TextureWindow, TileQuad,
    TileWorldSpan,
};
pub use source_binding::{
    BoundGeoJsonSource, BoundRasterSource, BoundStyleSources, BoundVectorSource,
    GeoJsonSourceBinding, MultiSourceVectorFrame, RasterLayerRequest, RasterSourceBinding,
    SOURCE_ATTRIBUTION_SEPARATOR, SourceBindingDiagnostic, SourceFetchPolicy, StyleSourceBindings,
    VectorSourceBinding, flipped_tile_row, raster_layer_requests,
    resolve_multi_source_vector_frame, tile_geographic_bounds,
};
pub use sprite::{
    DEFAULT_SPRITE_PIXEL_RATIO, SPRITE_ATLAS_PIXEL_BYTES, SpriteAtlas, SpriteAtlasError,
    SpriteIcon, SpriteIndex, SpriteSource, SpriteTextureRectangle,
};
#[cfg(not(target_arch = "wasm32"))]
pub use surface::{
    SurfaceFrameStatistics, SurfaceGraphicsContext, SurfaceGraphicsContextSource,
    SurfaceRenderError, SurfaceRenderTarget, SurfaceTileRenderer, WgpuSurfaceGraphicsContextSource,
};
pub use tile_key::TileKey;
pub use tile_scheduler::{PreparedTile, TileReadiness, TileScheduler, prepared_frame_is_idle};
pub use tile_texture_pool::TileTexturePool;
pub use vector_base::{
    DEFAULT_VECTOR_BASE_MAXIMUM_CACHED_TILE_COUNT, DEFAULT_VECTOR_BASE_PREFETCH_RING_TILE_BUDGET,
    DEFAULT_VECTOR_TILE_MEMORY_CACHE_MAXIMUM_TILE_COUNT,
    DEFAULT_VECTOR_TILE_PAYLOAD_TIME_TO_LIVE_SECONDS,
    VECTOR_BASE_STYLE_ZOOM_QUANTIZATION_STEPS_PER_LEVEL, VectorBaseFrame,
    VectorBaseFrameDiagnostics, VectorBaseScheduler, VectorTileMemoryCache,
};
pub use zoom_transition::{
    ZOOM_TRANSITION_FADE_MILLISECONDS, ZOOM_TRANSITION_MAXIMUM_HOLD_MILLISECONDS,
    ZOOM_TRANSITION_READY_TILE_FRACTION, ZoomTransition, ZoomTransitionFrame,
    requested_level_ready_tile_fraction,
};
