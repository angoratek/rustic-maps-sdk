//! Frame and cache counters, log events and provider failure reports.

#[cfg(not(target_arch = "wasm32"))]
use rustic_maps_rendering::SurfaceFrameStatistics;

/// One presented frame's tile and overlay accounting (`MapView::draw`'s
/// return), mirroring `rustic-maps-rendering`'s per-frame statistics: every requested
/// tile lands in exactly one readiness count, and drawn pixels come from the
/// present and fallback tiles while the deferred and unavailable ones are
/// where later frames may still change pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameStatistics {
    /// Tile requests the frame resolved (per request; duplicates in a
    /// request listing cost one fetch but report per request).
    pub requested_tile_count: u32,
    /// Tiles drawn at their own zoom level.
    pub present_tile_count: u32,
    /// Tiles drawn from their nearest pooled ancestor.
    pub fallback_tile_count: u32,
    /// Tiles whose fetch was deferred by the per-frame budget.
    pub deferred_tile_count: u32,
    /// Tiles the provider rejected this frame.
    pub unavailable_tile_count: u32,
    /// Clip-space triangles the overlay layer contributed to the frame.
    pub overlay_triangle_count: u32,
}

impl FrameStatistics {
    /// Builds the mirror record from the native frame statistics.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn from_native_frame_statistics(
        native_frame_statistics: &SurfaceFrameStatistics,
    ) -> Self {
        Self {
            requested_tile_count: native_frame_statistics.requested_tile_count,
            present_tile_count: native_frame_statistics.present_tile_count,
            fallback_tile_count: native_frame_statistics.fallback_tile_count,
            deferred_tile_count: native_frame_statistics.deferred_tile_count,
            unavailable_tile_count: native_frame_statistics.unavailable_tile_count,
            overlay_triangle_count: native_frame_statistics.overlay_triangle_count,
        }
    }
}

/// The serving traffic-flow cache's counters beside the budget they obey
/// (`MapSdk::traffic_flow_cache_statistics`'s return; see
/// `rustic_maps.udl`'s `TrafficFlowCacheStatistics` for the field contract).
/// The hits and misses prove a repeated pane was answered without asking the
/// provider, and `maximum_entry_count` bounds how many cells the cache may
/// hold at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrafficFlowCacheStatistics {
    /// Requests answered entirely from fresh cells.
    pub hit_count: u64,
    /// Requests that had to ask the provider for at least one cell.
    pub miss_count: u64,
    /// Cells currently held.
    pub entry_count: u64,
    /// Cells dropped to stay inside the entry budget.
    pub eviction_count: u64,
    /// Flow segments currently held across every cell.
    pub segment_count: u64,
    /// Zoom level the cells are keyed at.
    pub grid_zoom_level: u32,
    /// Cells the cache may hold before the least recently used is evicted.
    pub maximum_entry_count: u64,
    /// Seconds a stored cell stays fresh.
    pub time_to_live_seconds: u64,
}

impl From<rustic_maps_core::TrafficFlowCacheReadback> for TrafficFlowCacheStatistics {
    fn from(readback: rustic_maps_core::TrafficFlowCacheReadback) -> Self {
        Self {
            hit_count: readback.statistics.hit_count,
            miss_count: readback.statistics.miss_count,
            entry_count: u64::try_from(readback.statistics.entry_count).unwrap_or(u64::MAX),
            eviction_count: readback.statistics.eviction_count,
            segment_count: u64::try_from(readback.statistics.segment_count).unwrap_or(u64::MAX),
            grid_zoom_level: u32::from(readback.configuration.grid_zoom_level),
            maximum_entry_count: u64::try_from(readback.configuration.maximum_entry_count)
                .unwrap_or(u64::MAX),
            time_to_live_seconds: readback.configuration.time_to_live_seconds,
        }
    }
}

/// The installed tile payload cache's exact counters and occupancy
/// (`MapSdk::tile_cache_statistics`'s return; see `rustic_maps.udl`'s
/// `TileCacheStatistics` for the field contract).
///
/// Both the raster base and the vector base fetch through the same installed
/// store, so one counter pair covers every payload lookup: a hit was answered
/// without a provider request, and a miss went to the provider (or degraded to
/// one after a store read failure).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TileCacheStatistics {
    /// Payload lookups the store answered without a provider request.
    pub hit_count: u64,
    /// Payload lookups the store could not answer.
    pub miss_count: u64,
    /// Bytes currently stored, or `None` when the store's occupancy could not
    /// be read.
    pub stored_byte_count: Option<u64>,
}

#[cfg(not(target_arch = "wasm32"))]
impl From<rustic_maps_cache::TileCacheStatistics> for TileCacheStatistics {
    fn from(statistics: rustic_maps_cache::TileCacheStatistics) -> Self {
        Self {
            hit_count: statistics.hit_count,
            miss_count: statistics.miss_count,
            stored_byte_count: None,
        }
    }
}

/// The effective raster-tile caching terms of the installed cache
/// (`MapSdk::raster_tile_cache_terms`'s return), shaped so a developer can
/// display the caching posture: the category default, the provider's terms
/// ceiling and the value currently in force (see
/// `rustic_maps.udl`'s `TileCacheTerms` for the field contract).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileCacheTerms {
    /// Time-to-live currently in force on the installed cache, in seconds:
    /// the category default, or the last accepted override (clamped to the
    /// ceiling when the override exceeded it).
    pub effective_time_to_live_seconds: u64,
    /// The installed provider's declared caching-terms ceiling in seconds,
    /// or `None` when the provider declares no ceiling — the requested
    /// time-to-live then applies verbatim. For the MapQuest raster tiles
    /// this is the documented terms ceiling (thirty calendar days).
    pub provider_terms_ceiling_seconds: Option<u64>,
    /// The cache category's default time-to-live for raster tiles, in
    /// seconds (thirty days, the imagery provider's declared ceiling); the
    /// value a freshly registered cache starts at.
    pub default_time_to_live_seconds: u64,
}

/// Severity of an SDK diagnostic event, mirroring the `tracing` levels the
/// crates log under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SdkLogSeverity {
    /// Very fine-grained frame-path instrumentation.
    Trace,
    /// Fine-grained internal state transitions.
    Debug,
    /// Coarse-grained progress.
    Info,
    /// A problem this event explains.
    Warn,
    /// A failure this event explains.
    Error,
}

/// One completed provider request, as a diagnostics event reports it.
///
/// The record is already redacted: the request path keeps its structure with
/// query values removed, and no credential or response body is carried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRequestTrace {
    /// The registry key of the provider that served the call.
    pub provider_key: String,
    /// The contract the call served, for example `RoutingProvider`.
    pub provider_contract: String,
    /// The HTTP method as an uppercase word (`GET`, `POST`).
    pub http_method: String,
    /// The redacted request path, with query names and no values.
    pub request_path: String,
    /// `2xx`, `3xx`, `4xx`, `5xx` or the transport-failure marker.
    pub status_class: String,
    /// The derived outcome word (`success`, `client-error`, …).
    pub outcome: String,
    /// Wall-clock duration of the attempt in milliseconds.
    pub duration_milliseconds: u64,
    /// Response payload size in bytes; zero when no response arrived.
    pub response_bytes: u64,
    /// The redacted response prefix captured behind the excerpt switch,
    /// capped at 512 bytes.
    pub response_excerpt: Option<String>,
}

/// One structured SDK internal log event, as delivered to a foreign
/// diagnostics listener or read back by a flush. Coordinates in event messages
/// are rounded at the sink; the API key never appears in a message.
#[derive(Debug, Clone, PartialEq)]
pub struct LogEvent {
    /// Wall-clock milliseconds since the Unix epoch, as the ring recorded
    /// the event.
    pub timestamp_milliseconds_since_epoch: u64,
    /// The event's severity.
    pub severity: SdkLogSeverity,
    /// The named per-subsystem target that logged it.
    pub subsystem_target: String,
    /// The one-line already-scrubbed message.
    pub message_line: String,
    /// The typed request facts when this event reports a completed provider
    /// call; `None` for every other event.
    pub provider_request_trace: Option<ProviderRequestTrace>,
}

/// Which shape of foreign provider failure a diagnostic captured. The two
/// shapes follow the two ways a foreign implementation of a
/// `[Trait, WithForeign]` provider contract can fail: typed (it threw one of
/// the contract's `SdkError` variants) or unclassified (it failed without a
/// mapped error, the round-11 conversion's subject).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderFailureClassification {
    /// The foreign implementation failed without a mapped error (an
    /// exception the contract does not spell): the failure text is the
    /// uniffi callback machinery's verbatim reason for it.
    UnclassifiedCallbackFailure,
    /// The foreign implementation threw one of the contract's `SdkError`
    /// variants, which round-trips typed through the seam: the failure text
    /// is the thrown binding error's verbatim display form.
    MappedContractError,
}

/// One failed foreign-callback provider call, as retained in
/// `MapSdk::latest_provider_failure_diagnostics`'s bounded per-object ring
/// (the provider-failure diagnostic seam). Every
/// field is knowable at the adapter boundary; no data is invented beyond it
/// (see the UDL documentation for the classification limitation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderFailureDiagnostic {
    /// Wall-clock milliseconds since the Unix epoch, as the seam recorded
    /// the failure.
    pub timestamp_milliseconds_since_epoch: u64,
    /// Which failure shape this entry captured.
    pub failure_classification: ProviderFailureClassification,
    /// The provider contract the foreign code implements, e.g.
    /// `GeocodingProvider`.
    pub provider_contract_name: String,
    /// The contract method whose call failed, e.g. `forward_geocode`.
    pub provider_method_name: String,
    /// The verbatim failure text: the uniffi callback machinery's reason for
    /// the unclassified shape, or the thrown binding error's display form
    /// for the mapped shape.
    pub failure_detail: String,
}
