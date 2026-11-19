//! JavaScript field names of the contract records the hand-written canvas
//! surface exchanges.
//!
//! The frozen contract generates the browser API; these constants name the
//! lower-camel fields the canvas, the vector layer and their plain-object
//! builders read. They are hand-maintained beside those readers.

// The constants sit in a skipped block so their values stay byte stable under
// `cargo fmt`; the values are the formatted contract field names, not
// hand-wrapped text. Every constant is imported by the canvas or the vector
// layer, so on the wasm32 build an unused one fails instead of drifting. The
// host build compiles this file only for the shared plumbing and its tests,
// where those wasm-only consumers do not exist.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
#[rustfmt::skip]
mod field_names {
    /// JavaScript field name of the contract's `TileKey.tile_x`.
    pub(crate) const TILE_FIELD_TILE_X: &str = "tileX";

    /// JavaScript field name of the contract's `TileKey.tile_y`.
    pub(crate) const TILE_FIELD_TILE_Y: &str = "tileY";

    /// JavaScript field name of the contract's `TileKey.zoom_level`.
    pub(crate) const TILE_FIELD_ZOOM_LEVEL: &str = "zoomLevel";

    /// JavaScript field name of the contract's `OverlayColor.alpha`.
    pub(crate) const COLOR_FIELD_ALPHA: &str = "alpha";

    /// JavaScript field name of the contract's `OverlayColor.blue`.
    pub(crate) const COLOR_FIELD_BLUE: &str = "blue";

    /// JavaScript field name of the contract's `OverlayColor.green`.
    pub(crate) const COLOR_FIELD_GREEN: &str = "green";

    /// JavaScript field name of the contract's `OverlayColor.red`.
    pub(crate) const COLOR_FIELD_RED: &str = "red";

    /// JavaScript field name of the contract's `GeographicCoordinate.latitude`.
    pub(crate) const COORDINATE_FIELD_LATITUDE: &str = "latitude";

    /// JavaScript field name of the contract's `GeographicCoordinate.longitude`.
    pub(crate) const COORDINATE_FIELD_LONGITUDE: &str = "longitude";

    /// JavaScript field name of the contract's `LocationIndicatorState.course_heading_degrees`.
    pub(crate) const INDICATOR_FIELD_COURSE_HEADING_DEGREES: &str = "courseHeadingDegrees";

    /// JavaScript field name of the contract's `LocationIndicatorStyle.halo_rgba_color`.
    pub(crate) const INDICATOR_FIELD_HALO_RGBA_COLOR: &str = "haloRgbaColor";

    /// JavaScript field name of the contract's `LocationIndicatorState.horizontal_accuracy_meters`.
    pub(crate) const INDICATOR_FIELD_HORIZONTAL_ACCURACY_METERS: &str = "horizontalAccuracyMeters";

    /// JavaScript field name of the contract's `LocationIndicatorStyle.radius_pixels`.
    pub(crate) const INDICATOR_FIELD_RADIUS_PIXELS: &str = "radiusPixels";

    /// JavaScript field name of the contract's `LocationIndicatorStyle.rgba_color`.
    pub(crate) const INDICATOR_FIELD_RGBA_COLOR: &str = "rgbaColor";

    /// JavaScript field name of the contract's `LocationIndicatorStyle.ring_rgba_color`.
    pub(crate) const INDICATOR_FIELD_RING_RGBA_COLOR: &str = "ringRgbaColor";

    /// JavaScript field name of the contract's `LocationIndicatorStyle.show_accuracy_ring`.
    pub(crate) const INDICATOR_FIELD_SHOW_ACCURACY_RING: &str = "showAccuracyRing";

    /// JavaScript field name of the contract's `LocationIndicatorStyle.visual_kind`.
    pub(crate) const INDICATOR_FIELD_VISUAL_KIND: &str = "visualKind";

    /// JavaScript field name of the contract's `LocationIndicatorState.coordinate`.
    pub(crate) const INDICATOR_STATE_FIELD_COORDINATE: &str = "coordinate";

    /// JavaScript field name of the contract's `TrafficFlowData.congestion_level`.
    pub(crate) const OVERLAY_FIELD_CONGESTION_LEVEL: &str = "congestionLevel";

    /// JavaScript field name of the contract's `MapOverlay.coordinates`.
    pub(crate) const OVERLAY_FIELD_COORDINATES: &str = "coordinates";

    /// JavaScript field name of the contract's `MapOverlay.kind`.
    pub(crate) const OVERLAY_FIELD_KIND: &str = "kind";

    /// JavaScript field name of the contract's `GeographicCoordinate.latitude`.
    pub(crate) const OVERLAY_FIELD_LATITUDE: &str = "latitude";

    /// JavaScript field name of the contract's `GeographicCoordinate.longitude`.
    pub(crate) const OVERLAY_FIELD_LONGITUDE: &str = "longitude";

    /// JavaScript field name of the contract's `MapOverlay.ring_coordinates`.
    pub(crate) const OVERLAY_FIELD_RING_COORDINATES: &str = "ringCoordinates";

    /// JavaScript field name of the contract's `TrafficFlowData.shape_coordinates`.
    pub(crate) const OVERLAY_FIELD_SHAPE_COORDINATES: &str = "shapeCoordinates";

    /// JavaScript field name of the contract's `MapOverlay.width_pixels`.
    pub(crate) const OVERLAY_FIELD_WIDTH_PIXELS: &str = "widthPixels";

    /// JavaScript field name of the contract's `ProviderVectorTileSource.provider_key`.
    pub(crate) const PROVIDER_VECTOR_SOURCE_FIELD_PROVIDER_KEY: &str = "providerKey";

    /// JavaScript field name of the contract's `ProviderVectorTileSource.style_id`.
    pub(crate) const PROVIDER_VECTOR_SOURCE_FIELD_STYLE_ID: &str = "styleId";

    /// JavaScript field name of the contract's `ScreenPoint.x`.
    pub(crate) const SCREEN_POINT_FIELD_X: &str = "x";

    /// JavaScript field name of the contract's `ScreenPoint.y`.
    pub(crate) const SCREEN_POINT_FIELD_Y: &str = "y";

    /// JavaScript field name of the contract's `SourceHeader.name`.
    pub(crate) const SOURCE_HEADER_FIELD_NAME: &str = "name";

    /// JavaScript field name of the contract's `SourceHeader.value`.
    pub(crate) const SOURCE_HEADER_FIELD_VALUE: &str = "value";

    /// JavaScript field name of the contract's `UrlTemplateVectorTileSource.attribution_text`.
    pub(crate) const VECTOR_SOURCE_FIELD_ATTRIBUTION_TEXT: &str = "attributionText";

    /// JavaScript field name of the contract's `UrlTemplateVectorTileSource.headers`.
    pub(crate) const VECTOR_SOURCE_FIELD_HEADERS: &str = "headers";

    /// JavaScript field name of the contract's `UrlTemplateVectorTileSource.maximum_zoom_level`.
    pub(crate) const VECTOR_SOURCE_FIELD_MAXIMUM_ZOOM_LEVEL: &str = "maximumZoomLevel";

    /// JavaScript field name of the contract's `UrlTemplateVectorTileSource.minimum_zoom_level`.
    pub(crate) const VECTOR_SOURCE_FIELD_MINIMUM_ZOOM_LEVEL: &str = "minimumZoomLevel";

    /// JavaScript field name of the contract's `UrlTemplateVectorTileSource.refill_tokens_per_second`.
    pub(crate) const VECTOR_SOURCE_FIELD_REFILL_TOKENS_PER_SECOND: &str = "refillTokensPerSecond";

    /// JavaScript field name of the contract's `UrlTemplateVectorTileSource.source_name`.
    pub(crate) const VECTOR_SOURCE_FIELD_SOURCE_NAME: &str = "sourceName";

    /// JavaScript field name of the contract's `UrlTemplateVectorTileSource.style_hints`.
    pub(crate) const VECTOR_SOURCE_FIELD_STYLE_HINTS: &str = "styleHints";

    /// JavaScript field name of the contract's `UrlTemplateVectorTileSource.style_id`.
    pub(crate) const VECTOR_SOURCE_FIELD_STYLE_ID: &str = "styleId";

    /// JavaScript field name of the contract's `UrlTemplateVectorTileSource.subdomains`.
    pub(crate) const VECTOR_SOURCE_FIELD_SUBDOMAINS: &str = "subdomains";

    /// JavaScript field name of the contract's `UrlTemplateVectorTileSource.terms_text`.
    pub(crate) const VECTOR_SOURCE_FIELD_TERMS_TEXT: &str = "termsText";

    /// JavaScript field name of the contract's `UrlTemplateVectorTileSource.token_bucket_capacity_tokens`.
    pub(crate) const VECTOR_SOURCE_FIELD_TOKEN_BUCKET_CAPACITY_TOKENS: &str = "tokenBucketCapacityTokens";

    /// JavaScript field name of the contract's `UrlTemplateVectorTileSource.url_template`.
    pub(crate) const VECTOR_SOURCE_FIELD_URL_TEMPLATE: &str = "urlTemplate";

    /// JavaScript field name of the contract's `VectorTileSourceLayerCount.drawn_layer_count`.
    pub(crate) const VECTOR_SOURCE_LAYER_COUNT_FIELD_DRAWN_LAYER_COUNT: &str = "drawnLayerCount";

    /// JavaScript field name of the contract's `VectorTileSourceLayerCount.matched_layer_count`.
    pub(crate) const VECTOR_SOURCE_LAYER_COUNT_FIELD_MATCHED_LAYER_COUNT: &str = "matchedLayerCount";

    /// JavaScript field name of the contract's `VectorTileSourceLayerCount.source_id`.
    pub(crate) const VECTOR_SOURCE_LAYER_COUNT_FIELD_SOURCE_ID: &str = "sourceId";

    /// JavaScript field name of the contract's `VectorTileSourceStatus.attribution_text`.
    pub(crate) const VECTOR_STATUS_FIELD_ATTRIBUTION_TEXT: &str = "attributionText";

    /// JavaScript field name of the contract's `VectorTileSourceStatus.degraded_asset_names`.
    pub(crate) const VECTOR_STATUS_FIELD_DEGRADED_ASSET_NAMES: &str = "degradedAssetNames";

    /// JavaScript field name of the contract's `VectorTileSourceStatus.drawn_tile_count`.
    pub(crate) const VECTOR_STATUS_FIELD_DRAWN_TILE_COUNT: &str = "drawnTileCount";

    /// JavaScript field name of the contract's `VectorTileSourceStatus.failed_tile_count`.
    pub(crate) const VECTOR_STATUS_FIELD_FAILED_TILE_COUNT: &str = "failedTileCount";

    /// JavaScript field name of the contract's `VectorTileSourceStatus.is_configured`.
    pub(crate) const VECTOR_STATUS_FIELD_IS_CONFIGURED: &str = "isConfigured";

    /// JavaScript field name of the contract's `VectorTileSourceStatus.last_failure_detail`.
    pub(crate) const VECTOR_STATUS_FIELD_LAST_FAILURE_DETAIL: &str = "lastFailureDetail";

    /// JavaScript field name of the contract's `VectorTileSourceStatus.maximum_zoom_level`.
    pub(crate) const VECTOR_STATUS_FIELD_MAXIMUM_ZOOM_LEVEL: &str = "maximumZoomLevel";

    /// JavaScript field name of the contract's `VectorTileSourceStatus.source_layer_counts`.
    pub(crate) const VECTOR_STATUS_FIELD_SOURCE_LAYER_COUNTS: &str = "sourceLayerCounts";

    /// JavaScript field name of the contract's `VectorTileSourceStatus.source_name`.
    pub(crate) const VECTOR_STATUS_FIELD_SOURCE_NAME: &str = "sourceName";

    /// JavaScript field name of the contract's `VectorTileSourceStatus.style_id`.
    pub(crate) const VECTOR_STATUS_FIELD_STYLE_ID: &str = "styleId";

    /// JavaScript field name of the contract's `VectorTileSourceStatus.unsupported_property_count`.
    pub(crate) const VECTOR_STATUS_FIELD_UNSUPPORTED_PROPERTY_COUNT: &str = "unsupportedPropertyCount";

    /// JavaScript field name of the contract's `VectorTileSourceStatus.unsupported_property_names`.
    pub(crate) const VECTOR_STATUS_FIELD_UNSUPPORTED_PROPERTY_NAMES: &str = "unsupportedPropertyNames";
}

pub(crate) use field_names::*;
