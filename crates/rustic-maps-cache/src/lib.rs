//! SQLite-backed request/response cache for the Maps SDK:
//!
//! - [`RequestDescriptor`] fingerprints: SHA-256 over order-independent
//!   normalized request parameters, category-tagged (`geocoding:<hex>`).
//! - [`ResponseCategory`]: per-category default TTLs with per-call override.
//! - [`SqliteResponseCache`]: pooled WAL-mode connections, `SAVEPOINT`-guarded
//!   writes, lazy TTL expiry, and least-recently-used eviction under a
//!   [`CacheQuota`] of absolute bytes and/or a share of free disk space.
//! - [`cached_providers`]: cache-then-network wrappers around every
//!   [`geospatial_traits`] provider contract.
//! - [`tile_packs`]: the tile-pack manager over the same store —
//!   region selection, download, honest per-region on-disk reporting and
//!   explicit per-pack eviction, with pack tiles pinned against ordinary
//!   LRU eviction while their pack record exists.

pub mod cached_providers;
pub mod cached_style_resources;
pub mod disk_quota;
pub mod request_fingerprint;
pub mod response_category;
pub mod sqlite_store;
pub mod tile_cache_statistics;
pub mod tile_packs;

pub use cached_providers::{
    CachedDistanceMatrixProvider, CachedGeocodingProvider, CachedRasterTileProvider,
    CachedRoutingProvider, CachedSearchPlaceProvider, CachedTrafficFlowProvider,
    CachedTrafficIncidentProvider, CachedVectorTileProvider, TileResponseCacheHandle,
    tile_request_descriptor,
};
pub use cached_style_resources::CachedStyleResourceClient;
pub use disk_quota::{CacheQuota, DiskSpaceProbe, FixedDiskSpaceProbe, RealDiskSpaceProbe};
pub use request_fingerprint::{RequestDescriptor, coordinate_to_fingerprint_text};
pub use response_category::ResponseCategory;
pub use sqlite_store::{SqliteResponseCache, SqliteResponseCacheConfiguration};
pub use tile_cache_statistics::{
    CountingCachedResponseStore, TileCacheStatistics, TileCacheStatisticsCounters,
};
pub use tile_packs::{
    DEFAULT_TILE_PACK_BYTE_BUDGET_BYTES, MAXIMUM_TILE_PACK_TILE_COUNT, TilePackDescription,
    TilePackManager, TilePackProgress, TilePackRecord, TilePackRemovalReport, TilePackTileAddress,
    enumerate_region_tiles,
};
