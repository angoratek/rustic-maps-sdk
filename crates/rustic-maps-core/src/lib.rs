//! Orchestration core of the Maps SDK: the provider registry, the
//! cache-then-network policy and the typed value boundary between providers
//! and the rest of the SDK.
//!
//! Rendering, navigation and provider crates build on this crate. No concrete
//! provider is wired in directly; applications register providers at startup
//! through [`ProviderRegistry`].

pub mod blocking_work;
pub mod cache_policy;
// Adapters that present an everymap-rs provider through this SDK's contracts.
pub mod everymap_bridge;
pub mod offline_routing;
pub mod provider_observation;
// Benchmark support for the offline routing engine (M7 slice 6): the
// synthetic graphs, the Dijkstra baseline's harness and the coarse CI
// budget alarm. Test/bench seam, never production routing surface.
pub mod offline_routing_benchmark;
pub mod registry;
pub mod routing_fallback;
// Fetches of rendering style resources (style documents, glyph ranges and
// sprites) through a provider's own authenticated client.
pub mod style_resources;
// The namespace-aware cache keys tile payloads are stored under, shared by
// the storage crate and the renderer.
pub mod tile_cache_keys;
// Grid-keyed, memory-bounded caching of traffic-flow answers (the traffic
// layer's reuse across pans), portable to the browser.
pub mod traffic_flow_cache;

pub use blocking_work::{BlockingWorkExecutor, InlineBlockingWorkExecutor, run_blocking};
pub use cache_policy::{CacheThenNetworkOutcome, CachedResponseStore, execute_cache_then_network};
pub use offline_routing::{
    ArchiveOfflineRoutingProvider, calculate_offline_route_from_archive_bytes,
};
pub use registry::{ProviderContract, ProviderRegistry};
pub use routing_fallback::OfflineFallbackRoutingProvider;
pub use style_resources::StyleResourceClient;
pub use tile_cache_keys::{VECTOR_TILES_FINGERPRINT_PREFIX, vector_tile_payload_fingerprint};
pub use traffic_flow_cache::{
    DEFAULT_TRAFFIC_FLOW_GRID_ZOOM_LEVEL, DEFAULT_TRAFFIC_FLOW_MAXIMUM_ENTRY_COUNT,
    DEFAULT_TRAFFIC_FLOW_TIME_TO_LIVE_SECONDS, GridCachedTrafficFlowProvider,
    TrafficFlowCacheConfiguration, TrafficFlowCacheReadback, TrafficFlowCacheStatistics,
    TrafficFlowCurrentTimeSeconds, TrafficFlowGridCache, system_clock_time_seconds,
};
