//! The browser vector base: the installed source summary and the status
//! helpers the readback answers.

use super::*;

/// Additive diagnostics fields present while a source is configured: the
/// payload cache's cumulative hits and misses, the fetches in flight at the
/// last resolved frame, the missing tiles a cached parent covered, and the
/// time from installing the source to the first painted tile. They are not
/// part of the UDL record (a later contract addition), so the
/// not-configured status keeps the contract's exact shape.
pub(super) const VECTOR_STATUS_FIELD_PAYLOAD_CACHE_HIT_COUNT: &str = "payloadCacheHitCount";
pub(super) const VECTOR_STATUS_FIELD_PAYLOAD_CACHE_MISS_COUNT: &str = "payloadCacheMissCount";
pub(super) const VECTOR_STATUS_FIELD_IN_FLIGHT_FETCH_COUNT: &str = "inFlightTileFetchCount";
pub(super) const VECTOR_STATUS_FIELD_FALLBACK_TILE_COUNT: &str = "fallbackTileCount";
pub(super) const VECTOR_STATUS_FIELD_FIRST_DRAWN_TILE_MILLISECONDS: &str =
    "timeToFirstDrawnTileMilliseconds";
/// Additive status field carrying the last resolved frame's vector tiles that
/// were neither drawn nor failed. Present while a source is configured.
pub(super) const VECTOR_STATUS_FIELD_PENDING_TILE_COUNT: &str = "pendingTileCount";
/// Additive status field reporting whether a document install is still
/// resolving. Present in every status shape, so a page can tell a download
/// from a settled (or cancelled) base.
pub(super) const VECTOR_STATUS_FIELD_PENDING_VECTOR_INSTALL: &str = "pendingVectorInstall";

/// Builds the JavaScript array of a bounded string-name list (unsupported
/// property names or degraded asset names).
pub(super) fn string_values_array(values: &[String]) -> JsValue {
    let array = Array::new();
    for value in values {
        array.push(&JsValue::from_str(value));
    }
    JsValue::from(array)
}

/// Builds the JavaScript array of the last frame's per-source layer counts.
pub(super) fn source_layer_counts_array(
    counts: &[VectorSourceLayerCount],
) -> Result<Array, JsValue> {
    let array = Array::new();
    for count in counts {
        let count_object = object_from_fields(&[
            (
                VECTOR_SOURCE_LAYER_COUNT_FIELD_SOURCE_ID,
                JsValue::from_str(&count.source_id),
            ),
            (
                VECTOR_SOURCE_LAYER_COUNT_FIELD_MATCHED_LAYER_COUNT,
                JsValue::from(count.matched_layer_count),
            ),
            (
                VECTOR_SOURCE_LAYER_COUNT_FIELD_DRAWN_LAYER_COUNT,
                JsValue::from(count.drawn_layer_count),
            ),
        ])?;
        array.push(&JsValue::from(count_object));
    }
    Ok(array)
}

/// The absolute address a style-document reference names.
///
/// A page may name the document relative to its own origin; the document's
/// own glyph and sprite references then resolve against that same origin, so
/// the address is made absolute once, before anything is cached or fetched.
pub(super) fn absolute_style_document_url(style_document_url: &str) -> String {
    let trimmed_address = style_document_url.trim();
    if url::Url::parse(trimmed_address).is_ok() {
        return trimmed_address.to_string();
    }
    let Some(window) = web_sys::window() else {
        return trimmed_address.to_string();
    };
    let page_origin = window.location().origin().unwrap_or_default();
    let Ok(base) = url::Url::parse(&page_origin) else {
        return trimmed_address.to_string();
    };
    base.join(trimmed_address)
        .map(|absolute_address| absolute_address.to_string())
        .unwrap_or_else(|_| trimmed_address.to_string())
}

/// An install requested while a frame owns the vector-base scheduler, waiting
/// for the frame's restore to apply it.
pub(super) struct PendingVectorBaseSource {
    /// The scheduler the requested source built.
    pub(super) scheduler: VectorBaseScheduler,
    /// The status summary that describes it.
    pub(super) summary: VectorBaseSourceSummary,
    /// The install generation that built it. A clear or a later install bumps
    /// the view's generation, and a queued source whose value no longer
    /// matches is dropped instead of applied.
    pub(super) install_generation: u64,
}

/// One installed vector base source's identity, mirroring the native
/// `VectorBaseSourceSummary`: the fields [`BrowserMapView::vector_tile_source_status`]
/// reports back without borrowing the scheduler.
#[derive(Debug, Clone)]
pub(super) struct VectorBaseSourceSummary {
    pub(super) style_id: String,
    pub(super) source_name: String,
    pub(super) attribution_text: String,
    pub(super) maximum_zoom_level: u8,
    pub(super) unsupported_property_count: u32,
    pub(super) unsupported_property_names: Vec<String>,
    pub(super) degraded_asset_names: Vec<String>,
}

impl VectorBaseSourceSummary {
    /// The honest not-configured status the readback answers with no source
    /// installed (empty style, name and attribution, zoom ceiling zero).
    pub(super) fn not_configured() -> Self {
        Self {
            style_id: String::new(),
            source_name: String::new(),
            attribution_text: String::new(),
            maximum_zoom_level: 0,
            unsupported_property_count: 0,
            unsupported_property_names: Vec::new(),
            degraded_asset_names: Vec::new(),
        }
    }

    /// Reads the summary of the scheduler a source descriptor built.
    pub(super) fn from_scheduler(vector_base_scheduler: &VectorBaseScheduler) -> Self {
        let source_descriptor = vector_base_scheduler.source_descriptor();
        let fidelity = vector_base_scheduler.style_fidelity();
        Self {
            style_id: vector_base_scheduler.style_id().to_string(),
            source_name: source_descriptor.source_name,
            // The style's own source declaration wins over the tileset's
            // descriptor, exactly as it does on the native surface.
            attribution_text: vector_base_scheduler.attribution_text(),
            maximum_zoom_level: vector_base_scheduler.served_maximum_zoom_level(),
            unsupported_property_count: fidelity.unsupported_property_count,
            unsupported_property_names: fidelity.unsupported_property_names,
            degraded_asset_names: fidelity.degraded_asset_names,
        }
    }
}
