//! Tile-pack management: region selection, download into the
//! same SQLite store the cache uses, per-region on-disk reporting and
//! eviction.
//!
//! # The coexistence model (pack tiles versus cache tiles)
//!
//! A pack's tiles live in the very same store — and under the very same
//! fingerprints — the [`CachedRasterTileProvider`] writes ordinary cache
//! entries under. The pack is bookkeeping over those entries:
//!
//! * At [`TilePackManager::prepare_tile_pack`] the pack record plus one
//!   membership row per planned tile fingerprint is written first, before
//!   any download; a membership row without an entry is inert and merely
//!   names a tile still to download.
//! * The download itself runs through the cached provider pipeline, so a
//!   present tile is served from the store (never re-downloaded) and a
//!   missing tile is fetched by the wrapped provider and stored by the
//!   store's ordinary write path.
//! * While the pack record exists, its member fingerprints are excluded
//!   from the store's least-recently-used eviction (`sqlite_store`):
//!   ordinary budget pressure cannot remove a packed tile, only the pack's
//!   explicit [`TilePackManager::evict_tile_pack`] (or a member entry's own
//!   time-to-live expiry) can.
//! * Pack entries are subject to the same per-provider imagery ceiling as
//!   ordinary cache tiles: the pack's effective retention is the requested
//!   value — defaulting to the tiles category's thirty days — clamped to the
//!   installed provider's declared ceiling. A request above the ceiling is
//!   accepted and stores at the ceiling: the compliance bound limits what
//!   the cache retains, never how the call returns.
//! * Packs carry their own reserved payload-byte budget beside the general
//!   LRU cache ([`TilePackManager::with_pack_byte_budget_bytes`]). A download
//!   that would cross it withdraws the one tile that crossed, keeps the
//!   partial pack and its membership resumable, and reports the cache's
//!   write refusal. Ordinary eviction still never frees a pack-pinned tile,
//!   and the structural [`MAXIMUM_TILE_PACK_TILE_COUNT`] cap remains the only
//!   tile-count limit.
//! * A pack's zoom range is bounded by the provider-declared maximum zoom
//!   level the manager is configured with; the cache layer carries no
//!   provider ceiling of its own. A pack deeper than that maximum is
//!   refused before any tile is downloaded.
//!
//! The scheduler and renderer stay untouched: packed tiles flow to a
//! `TileScheduler`-like caller through the same registered cached
//! provider slot as network-fetched tiles, and are indistinguishable there.
//! The integration test under `rustic-maps-rendering` pins that equivalence.

use crate::cached_providers::CachedRasterTileProvider;
use crate::cached_providers::tile_request_descriptor;
use crate::response_category::ResponseCategory;
use crate::sqlite_store::{
    SqliteResponseCache, map_execution_failure, map_pool_failure, unix_seconds_now,
};
use geospatial_types::{
    BoundingBox, CacheOperationFailureReason, GeographicCoordinate, RasterImageFormat,
    RasterTileResolution, SDKError,
};
use r2d2_sqlite::rusqlite;
use rustic_maps_core::{
    BlockingWorkExecutor, CacheThenNetworkOutcome, InlineBlockingWorkExecutor, run_blocking,
};
use std::sync::Arc;

/// The highest tile zoom level the web-mercator plane math represents: tile
/// indices are `u32` and the plane is `1 << zoom` tiles wide, so zoom 31 is
/// the last representable level. This is a **structural** bound, not a
/// provider ceiling: each provider declares its own usable maximum, and a
/// [`TilePackManager`] takes that declared ceiling as configuration.
const MAXIMUM_REPRESENTABLE_TILE_ZOOM_LEVEL: u8 = 31;

/// Guard against an integrator bounding box plus zoom range enumerating an
/// unbounded tile count (a whole-continent pack at street zoom); a larger
/// selection is rejected, never attempted.
pub const MAXIMUM_TILE_PACK_TILE_COUNT: usize = 65_536;

/// The byte budget tile packs start with before the application configures
/// one: no separate pack limit, so the structural
/// [`MAXIMUM_TILE_PACK_TILE_COUNT`] cap stays the only size bound until
/// [`TilePackManager::with_pack_byte_budget_bytes`] or the application's
/// settings surface reserves one. A configured budget caps the payload bytes
/// a pack may hold beside the general LRU cache; it never changes the
/// structural cap.
pub const DEFAULT_TILE_PACK_BYTE_BUDGET_BYTES: u64 = u64::MAX;

/// Web-mercator row chunk of the membership batch inserts; three bound
/// parameters per row keep a batch far below SQLite's variable-count limit.
const MEMBERSHIP_INSERT_ROWS_PER_STATEMENT: usize = 256;

/// Fingerprint chunk of the batched pack-freshness updates: one bound
/// parameter per fingerprint plus the expiry and pack identifier stay far
/// below SQLite's variable-count limit.
const FRESHNESS_UPDATE_FINGERPRINTS_PER_STATEMENT: usize = 256;

/// Floating-point slack of a region-edge tile-index conversion: an edge
/// longitude or latitude exactly on a tile boundary may evaluate one ulp
/// past the mathematical value, and the enumeration must not let that widen
/// the pack by an extra tile column or row.
const TILE_INDEX_BOUNDARY_EPSILON: f64 = 1e-9;

/// Integrator-selected pack shape and metadata: the region plus the tile
/// form the pack holds.
#[derive(Debug, Clone, PartialEq)]
pub struct TilePackDescription {
    /// Caller-chosen stable identifier; re-prepare of the same identifier
    /// is idempotent, a redefined shape replaces the pack's membership.
    pub pack_id: String,
    /// Human-readable name for pack-listing surfaces.
    pub display_name: String,
    /// The region's bounding box; boxes never span the antimeridian.
    pub region_bounding_box: BoundingBox,
    /// Lowest zoom level downloaded; ranges download coarse-first so the
    /// renderer's parent fallback covers the region while detail arrives.
    pub minimum_zoom_level: u8,
    /// Highest zoom level downloaded.
    pub maximum_zoom_level: u8,
    /// Encoded format the store's tile payloads carry.
    pub image_format: RasterImageFormat,
    /// Pixel resolution the store's tile payloads carry.
    pub tile_resolution: RasterTileResolution,
    /// Requested retention in seconds; `None` takes the tiles category's
    /// default (thirty days). Clamped to the provider's declared ceiling
    /// (see the module documentation); a zero is refused.
    pub requested_time_to_live_seconds: Option<u64>,
}

/// One web-mercator tile of a pack's region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TilePackTileAddress {
    /// Tile column at the pack zoom level.
    pub tile_x: u32,
    /// Tile row at the pack zoom level.
    pub tile_y: u32,
    /// The pack zoom level this tile belongs to.
    pub zoom_level: u8,
}

/// What a prepare call reports as it goes: honest per-tile counts of the
/// download's source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TilePackProgress {
    /// Tiles the region enumerates (the prepare's total).
    pub total_tile_count: u64,
    /// Tiles fetched and refreshed so far.
    pub completed_tile_count: u64,
    /// Completed tiles the wrapped provider had to fetch.
    pub network_fetched_tile_count: u64,
    /// Completed tiles the store already held.
    pub disk_served_tile_count: u64,
}

/// What a pack reports as on disk: shape, effective retention, and the
/// live tiles among its members.
#[derive(Debug, Clone, PartialEq)]
pub struct TilePackRecord {
    /// The namespace of the provider or source that produced the pack's
    /// tiles. A pack is reported and removed as its provider's own: the same
    /// pack identifier under another namespace is a different pack.
    pub tile_source_namespace: String,
    /// The pack's identifier.
    pub pack_id: String,
    /// The pack's display name.
    pub display_name: String,
    /// The recorded region the pack covers.
    pub region_bounding_box: BoundingBox,
    /// The recorded lowest zoom level.
    pub minimum_zoom_level: u8,
    /// The recorded highest zoom level.
    pub maximum_zoom_level: u8,
    /// The recorded encoded format.
    pub image_format: RasterImageFormat,
    /// The recorded pixel resolution.
    pub tile_resolution: RasterTileResolution,
    /// The retention the pack's entries carry (requested, ceiling-clamped).
    pub effective_time_to_live_seconds: u64,
    /// Tile fingerprints the pack claims membership for.
    pub member_tile_count: u64,
    /// Members whose entries are live on disk right now.
    pub present_tile_count: u64,
    /// Payload bytes of the live members.
    pub present_payload_bytes: u64,
}

/// What a pack's explicit eviction removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TilePackRemovalReport {
    /// Entries deleted (never a tile another live pack still claims).
    pub removed_entry_count: u64,
    /// Payload bytes those entries carried.
    pub removed_payload_bytes: u64,
}

/// The pack's tiles, coarse zoom first (the fallback-friendly order), then
/// by tile row and column for a stable download order.
pub fn enumerate_region_tiles(
    region_bounding_box: &BoundingBox,
    minimum_zoom_level: u8,
    maximum_zoom_level: u8,
) -> Result<Vec<TilePackTileAddress>, SDKError> {
    if minimum_zoom_level > maximum_zoom_level {
        return Err(SDKError::InvalidCoordinateInput(format!(
            "the tile pack zoom range {minimum_zoom_level}..={maximum_zoom_level} is inverted"
        )));
    }
    if maximum_zoom_level > MAXIMUM_REPRESENTABLE_TILE_ZOOM_LEVEL {
        return Err(SDKError::InvalidCoordinateInput(format!(
            "zoom level {maximum_zoom_level} exceeds the representable tile-pack maximum of \
             {MAXIMUM_REPRESENTABLE_TILE_ZOOM_LEVEL}"
        )));
    }
    let mut tile_addresses = Vec::new();
    for zoom_level in minimum_zoom_level..=maximum_zoom_level {
        let plane_tile_count = 1u32 << zoom_level;
        let first_tile_x = first_tile_index(
            longitude_to_tile_x_fraction(
                region_bounding_box.south_west.longitude,
                plane_tile_count,
            ),
            plane_tile_count,
        );
        let last_tile_x = last_tile_index(
            longitude_to_tile_x_fraction(
                region_bounding_box.north_east.longitude,
                plane_tile_count,
            ),
            first_tile_x,
            plane_tile_count,
        );
        let first_tile_y = first_tile_index(
            latitude_to_tile_y_fraction(region_bounding_box.north_east.latitude, plane_tile_count),
            plane_tile_count,
        );
        let last_tile_y = last_tile_index(
            latitude_to_tile_y_fraction(region_bounding_box.south_west.latitude, plane_tile_count),
            first_tile_y,
            plane_tile_count,
        );
        let zoom_tile_count =
            (last_tile_x - first_tile_x + 1) as usize * (last_tile_y - first_tile_y + 1) as usize;
        if tile_addresses.len() + zoom_tile_count > MAXIMUM_TILE_PACK_TILE_COUNT {
            return Err(SDKError::InvalidCoordinateInput(format!(
                "the region enumerates more than the {MAXIMUM_TILE_PACK_TILE_COUNT} tile \
                 maximum; shrink the bounding box or the zoom range"
            )));
        }
        for tile_y in first_tile_y..=last_tile_y {
            for tile_x in first_tile_x..=last_tile_x {
                tile_addresses.push(TilePackTileAddress {
                    tile_x,
                    tile_y,
                    zoom_level,
                });
            }
        }
    }
    Ok(tile_addresses)
}

/// West-of-tile-index fraction of a longitude at the stated plane width.
fn longitude_to_tile_x_fraction(longitude_degrees: f64, plane_tile_count: u32) -> f64 {
    ((longitude_degrees + 180.0) / 360.0) * f64::from(plane_tile_count)
}

/// North-of-tile-index fraction of a latitude at the stated plane width,
/// the web-mercator `y = (1 - ln(tan + sec) / pi) / 2` projection; values
/// past the projection's usable latitude saturate through the tile-index
/// clamps.
fn latitude_to_tile_y_fraction(latitude_degrees: f64, plane_tile_count: u32) -> f64 {
    let latitude_radians = latitude_degrees.to_radians();
    let mercator_y_fraction = (1.0
        - (latitude_radians.tan() + latitude_radians.cos().recip()).ln() / std::f64::consts::PI)
        / 2.0;
    mercator_y_fraction * f64::from(plane_tile_count)
}

/// The westward-or-northward tile of a raw fractional index, clamped into
/// the plane (polar latitudes saturate at the plane's edge).
fn first_tile_index(raw_tile_index: f64, plane_tile_count: u32) -> u32 {
    raw_tile_index
        .floor()
        .max(0.0)
        .min(f64::from(plane_tile_count - 1)) as u32
}

/// The eastward-or-southward tile of a raw fractional index, inclusive: an
/// edge exactly on a tile boundary does not include the tile east of it,
/// and a region smaller than one tile still includes its containing tile.
fn last_tile_index(raw_tile_index: f64, first_tile_index: u32, plane_tile_count: u32) -> u32 {
    let upper_exclusive_tile_index = (raw_tile_index - TILE_INDEX_BOUNDARY_EPSILON)
        .ceil()
        .max(f64::from(first_tile_index) + 1.0)
        .min(f64::from(plane_tile_count));
    (upper_exclusive_tile_index - 1.0) as u32
}

/// The fingerprint one pack tile's member row stores: exactly the cached
/// wrapper's scheme, so membership and cache entries key identically.
fn pack_member_fingerprint(
    tile_source_namespace: &str,
    tile_address: &TilePackTileAddress,
    image_format: RasterImageFormat,
    tile_resolution: RasterTileResolution,
) -> String {
    tile_request_descriptor(
        tile_source_namespace,
        tile_address.tile_x,
        tile_address.tile_y,
        tile_address.zoom_level,
        image_format,
        tile_resolution,
    )
    .to_fingerprint()
}

/// The persisted, non-numeric pack columns between the SQL row read and the
/// validated [`TilePackRecord`]; `tile_source_namespace` and `pack_id` travel
/// as the lookup parameters.
struct UnpackedReaderRow {
    display_name: String,
    south_west_latitude_degrees: f64,
    south_west_longitude_degrees: f64,
    north_east_latitude_degrees: f64,
    north_east_longitude_degrees: f64,
    minimum_zoom_level: Result<u8, ()>,
    maximum_zoom_level: Result<u8, ()>,
    image_format: Result<RasterImageFormat, ()>,
    tile_resolution: Result<RasterTileResolution, ()>,
    effective_time_to_live_seconds: Result<u64, ()>,
}

/// The pack row's non-key columns, in the order every read selects them.
const PACK_TABLE_VALUE_COLUMNS: &str = "display_name,
                   south_west_latitude_degrees, south_west_longitude_degrees,
                   north_east_latitude_degrees, north_east_longitude_degrees,
                   minimum_zoom_level, maximum_zoom_level,
                   image_format_name, tile_resolution_name,
                   effective_time_to_live_seconds";

fn read_pack_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<UnpackedReaderRow> {
    let minimum_zoom_level: i64 = row.get(5)?;
    let maximum_zoom_level: i64 = row.get(6)?;
    let effective_time_to_live_seconds: i64 = row.get(9)?;
    Ok(UnpackedReaderRow {
        display_name: row.get(0)?,
        south_west_latitude_degrees: row.get(1)?,
        south_west_longitude_degrees: row.get(2)?,
        north_east_latitude_degrees: row.get(3)?,
        north_east_longitude_degrees: row.get(4)?,
        minimum_zoom_level: u8::try_from(minimum_zoom_level).map_err(|_| ()),
        maximum_zoom_level: u8::try_from(maximum_zoom_level).map_err(|_| ()),
        image_format: read_image_format_storage_name(&row.get::<_, String>(7)?).map_err(|_| ()),
        tile_resolution: read_tile_resolution_storage_name(&row.get::<_, String>(8)?)
            .map_err(|_| ()),
        effective_time_to_live_seconds: u64::try_from(effective_time_to_live_seconds)
            .map_err(|_| ()),
    })
}

/// Corrupted persisted shapes surface to the caller as
/// [`CacheOperationFailureReason::RecordCorrupted`]: a pack row that no
/// longer validates is a store-integrity matter, not an input matter.
fn corrupted_record_error() -> SDKError {
    SDKError::CacheOperationFailure(CacheOperationFailureReason::RecordCorrupted)
}

fn validated_pack_record(
    tile_source_namespace: &str,
    pack_id: &str,
    read_row: UnpackedReaderRow,
) -> Result<TilePackRecord, SDKError> {
    let image_format = read_row
        .image_format
        .map_err(|_| corrupted_record_error())?;
    let tile_resolution = read_row
        .tile_resolution
        .map_err(|_| corrupted_record_error())?;
    let minimum_zoom_level = read_row
        .minimum_zoom_level
        .map_err(|_| corrupted_record_error())?;
    let maximum_zoom_level = read_row
        .maximum_zoom_level
        .map_err(|_| corrupted_record_error())?;
    if minimum_zoom_level > maximum_zoom_level
        || maximum_zoom_level > MAXIMUM_REPRESENTABLE_TILE_ZOOM_LEVEL
    {
        return Err(corrupted_record_error());
    }
    let effective_time_to_live_seconds = read_row
        .effective_time_to_live_seconds
        .map_err(|_| corrupted_record_error())?;
    let south_west_coordinate = GeographicCoordinate::from_degrees(
        read_row.south_west_latitude_degrees,
        read_row.south_west_longitude_degrees,
    )
    .map_err(|_| corrupted_record_error())?;
    let north_east_coordinate = GeographicCoordinate::from_degrees(
        read_row.north_east_latitude_degrees,
        read_row.north_east_longitude_degrees,
    )
    .map_err(|_| corrupted_record_error())?;
    let region_bounding_box =
        BoundingBox::from_corners(south_west_coordinate, north_east_coordinate)
            .map_err(|_| corrupted_record_error())?;
    Ok(TilePackRecord {
        tile_source_namespace: tile_source_namespace.to_string(),
        pack_id: pack_id.to_string(),
        display_name: read_row.display_name,
        region_bounding_box,
        minimum_zoom_level,
        maximum_zoom_level,
        image_format,
        tile_resolution,
        effective_time_to_live_seconds,
        member_tile_count: 0,
        present_tile_count: 0,
        present_payload_bytes: 0,
    })
}

impl SqliteResponseCache {
    /// Writes (or replaces) the pack record and re-syncs its membership to
    /// the stated fingerprints. Idempotent for an unchanged shape; a changed
    /// shape drops the stale memberships, unpinned tiles fall back under
    /// ordinary LRU eviction. The pack is identified by its namespace and
    /// identifier together, so two providers' packs never overwrite each
    /// other.
    pub(crate) fn define_tile_pack(
        &self,
        tile_source_namespace: &str,
        pack_description: &TilePackDescription,
        member_fingerprints: &[String],
        effective_time_to_live_seconds: u64,
    ) -> Result<(), SDKError> {
        if pack_description.pack_id.is_empty() {
            return Err(SDKError::InvalidCoordinateInput(
                "the tile pack identifier must be non-empty".to_string(),
            ));
        }
        if effective_time_to_live_seconds == 0 {
            return Err(SDKError::InvalidCoordinateInput(
                "the tile pack time-to-live must be positive; zero expires every tile on capture"
                    .to_string(),
            ));
        }
        let region_bounding_box = pack_description.region_bounding_box;
        let effective_time_to_live = validated_pack_retention(effective_time_to_live_seconds)?;
        let minimum_zoom_level = i64::from(pack_description.minimum_zoom_level);
        let maximum_zoom_level = i64::from(pack_description.maximum_zoom_level);
        let image_format_name = image_format_storage_name(pack_description.image_format);
        let tile_resolution_name = tile_resolution_storage_name(pack_description.tile_resolution);
        let tile_source_namespace = tile_source_namespace.to_string();
        let pack_id = pack_description.pack_id.clone();
        let display_name = pack_description.display_name.clone();
        self.perform_write_transaction(move |connection| {
            connection
                .execute(
                    "
                    INSERT OR REPLACE INTO tile_packs (
                        tile_source_namespace, pack_id, display_name,
                        south_west_latitude_degrees, south_west_longitude_degrees,
                        north_east_latitude_degrees, north_east_longitude_degrees,
                        minimum_zoom_level, maximum_zoom_level,
                        image_format_name, tile_resolution_name,
                        effective_time_to_live_seconds, created_at_unix_seconds
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                    ",
                    rusqlite::params![
                        tile_source_namespace,
                        pack_id,
                        display_name,
                        region_bounding_box.south_west.latitude,
                        region_bounding_box.south_west.longitude,
                        region_bounding_box.north_east.latitude,
                        region_bounding_box.north_east.longitude,
                        minimum_zoom_level,
                        maximum_zoom_level,
                        image_format_name,
                        tile_resolution_name,
                        effective_time_to_live,
                        unix_seconds_now(),
                    ],
                )
                .map_err(map_execution_failure)?;
            connection
                .execute(
                    "DELETE FROM tile_pack_members WHERE tile_source_namespace = ?1 AND pack_id = ?2",
                    rusqlite::params![tile_source_namespace, pack_id],
                )
                .map_err(map_execution_failure)?;
            for membership_chunk in member_fingerprints.chunks(MEMBERSHIP_INSERT_ROWS_PER_STATEMENT)
            {
                let insert_value_placeholders: Vec<String> = (1..=membership_chunk.len())
                    .map(|row_position| {
                        format!("(?{}, ?{}, ?{})", row_position * 3 - 2, row_position * 3 - 1, row_position * 3)
                    })
                    .collect();
                let insert_sql = format!(
                    "INSERT OR IGNORE INTO tile_pack_members (tile_source_namespace, pack_id, fingerprint) VALUES {}",
                    insert_value_placeholders.join(", ")
                );
                let bound_values: Vec<&str> = membership_chunk
                    .iter()
                    .flat_map(|fingerprint| {
                        [
                            tile_source_namespace.as_str(),
                            pack_id.as_str(),
                            fingerprint.as_str(),
                        ]
                    })
                    .collect();
                connection
                    .execute(
                        &insert_sql,
                        rusqlite::params_from_iter(bound_values.iter().copied()),
                    )
                    .map_err(map_execution_failure)?;
            }
            Ok(())
        })
    }

    /// Moves every stated member entry's expiry to the pack's effective
    /// retention in one guarded write transaction, batching the updates.
    /// Members without a live entry (still to download) are no-ops; the
    /// update only reaches an entry the pipeline has actually stored.
    pub(crate) fn refresh_member_tile_freshnesses(
        &self,
        tile_source_namespace: &str,
        pack_id: &str,
        tile_fingerprints: &[String],
        effective_time_to_live_seconds: u64,
    ) -> Result<(), SDKError> {
        if tile_fingerprints.is_empty() {
            return Ok(());
        }
        let effective_time_to_live = validated_pack_retention(effective_time_to_live_seconds)?;
        let expires_at_unix_seconds = unix_seconds_now().saturating_add(effective_time_to_live);
        let tile_source_namespace = tile_source_namespace.to_string();
        let pack_id = pack_id.to_string();
        self.perform_write_transaction(move |connection| {
            for fingerprint_chunk in
                tile_fingerprints.chunks(FRESHNESS_UPDATE_FINGERPRINTS_PER_STATEMENT)
            {
                let fingerprint_placeholders = (0..fingerprint_chunk.len())
                    .map(|position| format!("?{}", position + 2))
                    .collect::<Vec<_>>()
                    .join(", ");
                let namespace_placeholder = fingerprint_chunk.len() + 2;
                let pack_id_placeholder = fingerprint_chunk.len() + 3;
                let update_sql = format!(
                    "
                    UPDATE entries SET expires_at_unix_seconds = ?1
                    WHERE fingerprint IN ({fingerprint_placeholders})
                    AND EXISTS (
                        SELECT 1 FROM tile_pack_members
                        WHERE tile_source_namespace = ?{namespace_placeholder}
                        AND pack_id = ?{pack_id_placeholder}
                        AND fingerprint = entries.fingerprint
                    )
                    "
                );
                let mut bound_values: Vec<rusqlite::types::Value> =
                    Vec::with_capacity(fingerprint_chunk.len() + 3);
                bound_values.push(rusqlite::types::Value::Integer(expires_at_unix_seconds));
                bound_values.extend(
                    fingerprint_chunk
                        .iter()
                        .map(|fingerprint| rusqlite::types::Value::Text(fingerprint.clone())),
                );
                bound_values.push(rusqlite::types::Value::Text(tile_source_namespace.clone()));
                bound_values.push(rusqlite::types::Value::Text(pack_id.clone()));
                connection
                    .execute(&update_sql, rusqlite::params_from_iter(bound_values))
                    .map_err(map_execution_failure)?;
            }
            Ok(())
        })
    }

    /// The stated pack's record, or `None` when no such pack is defined under
    /// the stated namespace.
    pub(crate) fn tile_pack_record(
        &self,
        tile_source_namespace: &str,
        pack_id: &str,
    ) -> Result<Option<TilePackRecord>, SDKError> {
        let connection = self.connection_pool.get().map_err(map_pool_failure)?;
        let pack_row = connection.query_row(
            &format!(
                "
                SELECT {PACK_TABLE_VALUE_COLUMNS}
                FROM tile_packs WHERE tile_source_namespace = ?1 AND pack_id = ?2
                "
            ),
            rusqlite::params![tile_source_namespace, pack_id],
            read_pack_row,
        );
        match pack_row {
            Ok(read_row) => {
                let mut record = validated_pack_record(tile_source_namespace, pack_id, read_row)?;
                let (member_tile_count, present_tile_count, present_payload_bytes) =
                    read_on_disk_counts(&connection, tile_source_namespace, pack_id)
                        .map_err(map_execution_failure)?;
                record.member_tile_count = member_tile_count;
                record.present_tile_count = present_tile_count;
                record.present_payload_bytes = present_payload_bytes;
                Ok(Some(record))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(rusqlite::Error::InvalidColumnType(..)) => Err(corrupted_record_error()),
            Err(sqlite_error) => Err(map_execution_failure(sqlite_error)),
        }
    }

    /// Every pack defined under the stated namespace, in definition order.
    pub(crate) fn tile_pack_records(
        &self,
        tile_source_namespace: &str,
    ) -> Result<Vec<TilePackRecord>, SDKError> {
        let connection = self.connection_pool.get().map_err(map_pool_failure)?;
        let pack_identifiers: Vec<String> = connection
            .prepare("SELECT pack_id FROM tile_packs WHERE tile_source_namespace = ?1")
            .map_err(map_execution_failure)?
            .query_map([tile_source_namespace], |row| row.get(0))
            .map_err(map_execution_failure)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_execution_failure)?;
        drop(connection);
        let mut pack_records = Vec::with_capacity(pack_identifiers.len());
        for pack_id in &pack_identifiers {
            match self.tile_pack_record(tile_source_namespace, pack_id)? {
                Some(pack_record) => pack_records.push(pack_record),
                None => return Err(corrupted_record_error()),
            }
        }
        Ok(pack_records)
    }

    /// Removes the pack record, its membership rows, and every member entry
    /// no other live pack still claims. Removing an unstated (unknown)
    /// identifier is a typed input error; removing an already-drained pack
    /// reports a zero removal.
    pub(crate) fn remove_tile_pack(
        &self,
        tile_source_namespace: &str,
        pack_id: &str,
    ) -> Result<TilePackRemovalReport, SDKError> {
        let pack_record_connection = self.connection_pool.get().map_err(map_pool_failure)?;
        let pack_record_count: i64 = pack_record_connection
            .query_row(
                "SELECT COUNT(*) FROM tile_packs WHERE tile_source_namespace = ?1 AND pack_id = ?2",
                rusqlite::params![tile_source_namespace, pack_id],
                |row| row.get(0),
            )
            .map_err(map_execution_failure)?;
        drop(pack_record_connection);
        if pack_record_count == 0 {
            return Err(SDKError::InvalidCoordinateInput(format!(
                "no tile pack is registered with the identifier {pack_id:?}"
            )));
        }
        let tile_source_namespace = tile_source_namespace.to_string();
        let pack_id = pack_id.to_string();
        self.perform_write_transaction(move |connection| {
            let (removed_entry_count, removed_payload_bytes): (i64, i64) = connection
                .query_row(
                    "
                    SELECT COUNT(*), COALESCE(SUM(payload_size_bytes), 0) FROM entries
                    WHERE fingerprint IN (
                        SELECT fingerprint FROM tile_pack_members
                        WHERE tile_source_namespace = ?1 AND pack_id = ?2
                    )
                    AND fingerprint NOT IN (
                        SELECT fingerprint FROM tile_pack_members
                        WHERE NOT (tile_source_namespace = ?1 AND pack_id = ?2)
                    )
                    ",
                    rusqlite::params![tile_source_namespace, pack_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(map_execution_failure)?;
            connection
                .execute(
                    "
                    DELETE FROM entries
                    WHERE fingerprint IN (
                        SELECT fingerprint FROM tile_pack_members
                        WHERE tile_source_namespace = ?1 AND pack_id = ?2
                    )
                    AND fingerprint NOT IN (
                        SELECT fingerprint FROM tile_pack_members
                        WHERE NOT (tile_source_namespace = ?1 AND pack_id = ?2)
                    )
                    ",
                    rusqlite::params![tile_source_namespace, pack_id],
                )
                .map_err(map_execution_failure)?;
            connection
                .execute(
                    "DELETE FROM tile_pack_members WHERE tile_source_namespace = ?1 AND pack_id = ?2",
                    rusqlite::params![tile_source_namespace, pack_id],
                )
                .map_err(map_execution_failure)?;
            connection
                .execute(
                    "DELETE FROM tile_packs WHERE tile_source_namespace = ?1 AND pack_id = ?2",
                    rusqlite::params![tile_source_namespace, pack_id],
                )
                .map_err(map_execution_failure)?;
            Ok(TilePackRemovalReport {
                removed_entry_count: removed_entry_count.max(0) as u64,
                removed_payload_bytes: removed_payload_bytes.max(0) as u64,
            })
        })
    }
}

/// A pack retention into the store's clock domain; a non-positive value is
/// an input error, an overflow is refused.
fn validated_pack_retention(effective_time_to_live_seconds: u64) -> Result<i64, SDKError> {
    i64::try_from(effective_time_to_live_seconds)
        .ok()
        .filter(|retention_seconds| *retention_seconds > 0)
        .ok_or_else(|| {
            SDKError::InvalidCoordinateInput(
                "the tile pack time-to-live must be positive and fit the store's clock".to_string(),
            )
        })
}

fn read_on_disk_counts(
    connection: &rusqlite::Connection,
    tile_source_namespace: &str,
    pack_id: &str,
) -> rusqlite::Result<(u64, u64, u64)> {
    let member_tile_count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM tile_pack_members WHERE tile_source_namespace = ?1 AND pack_id = ?2",
        rusqlite::params![tile_source_namespace, pack_id],
        |row| row.get(0),
    )?;
    let (present_tile_count, present_payload_bytes): (i64, i64) = connection.query_row(
        "
        SELECT COUNT(*), COALESCE(SUM(entry.payload_size_bytes), 0)
        FROM tile_pack_members AS member
        JOIN entries AS entry ON entry.fingerprint = member.fingerprint
        WHERE member.tile_source_namespace = ?1 AND member.pack_id = ?2
          AND entry.expires_at_unix_seconds > ?3
        ",
        rusqlite::params![tile_source_namespace, pack_id, unix_seconds_now()],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok((
        member_tile_count.max(0) as u64,
        present_tile_count.max(0) as u64,
        present_payload_bytes.max(0) as u64,
    ))
}

fn image_format_storage_name(image_format: RasterImageFormat) -> &'static str {
    match image_format {
        RasterImageFormat::Png => "png",
        RasterImageFormat::Jpeg => "jpeg",
    }
}

fn read_image_format_storage_name(storage_name: &str) -> Result<RasterImageFormat, SDKError> {
    match storage_name {
        "png" => Ok(RasterImageFormat::Png),
        "jpeg" => Ok(RasterImageFormat::Jpeg),
        _ => Err(corrupted_record_error()),
    }
}

fn tile_resolution_storage_name(tile_resolution: RasterTileResolution) -> &'static str {
    match tile_resolution {
        RasterTileResolution::Standard => "standard",
        RasterTileResolution::Retina => "retina",
    }
}

fn read_tile_resolution_storage_name(storage_name: &str) -> Result<RasterTileResolution, SDKError> {
    match storage_name {
        "standard" => Ok(RasterTileResolution::Standard),
        "retina" => Ok(RasterTileResolution::Retina),
        _ => Err(corrupted_record_error()),
    }
}

/// The pack manager over the tile cache's registered seam: it prepares
/// regions for download through the installed (cached-wrapped) raster-tile
/// provider, reports what is on disk per region, and evicts per region.
///
/// The manager holds the same [`SqliteResponseCache`] the cached wrapper
/// writes through, so a prepared tile and a network-fetched-and-cached tile
/// are the same store entry — a scheduler configured over the cached
/// provider answers pack tiles with no integration at all.
pub struct TilePackManager {
    raster_tile_provider: Arc<CachedRasterTileProvider>,
    response_cache: Arc<SqliteResponseCache>,
    provider_terms_ceiling_seconds: Option<u64>,
    maximum_zoom_level: u8,
    /// The payload-byte budget the packs this manager defines may hold,
    /// reserved beside the general LRU cache. See
    /// [`DEFAULT_TILE_PACK_BYTE_BUDGET_BYTES`].
    pack_byte_budget_bytes: u64,
    blocking_work_executor: Arc<dyn BlockingWorkExecutor>,
}

impl TilePackManager {
    /// Builds a manager over the installed cached raster-tile provider and
    /// its store. `provider_terms_ceiling_seconds` is the installed
    /// provider's declared caching-terms ceiling (`None` when the provider
    /// declares no ceiling): a pack retention above it is accepted and
    /// clamped to the ceiling, matching the tile-cache settings surface.
    ///
    /// The zoom ceiling starts at the structural plane-math maximum; a
    /// provider that serves a shallower range supplies its declared maximum
    /// through [`Self::with_maximum_zoom_level`]. Synchronous store work runs
    /// inline until [`Self::with_blocking_work_executor`] supplies a target
    /// with worker threads.
    pub fn new(
        raster_tile_provider: Arc<CachedRasterTileProvider>,
        response_cache: Arc<SqliteResponseCache>,
        provider_terms_ceiling_seconds: Option<u64>,
    ) -> Self {
        Self {
            raster_tile_provider,
            response_cache,
            provider_terms_ceiling_seconds,
            maximum_zoom_level: MAXIMUM_REPRESENTABLE_TILE_ZOOM_LEVEL,
            pack_byte_budget_bytes: DEFAULT_TILE_PACK_BYTE_BUDGET_BYTES,
            blocking_work_executor: Arc::new(InlineBlockingWorkExecutor),
        }
    }

    /// Caps a pack's zoom levels at the installed provider's declared
    /// maximum zoom level, so a pack never requests a tile the provider does
    /// not serve. A pack above the ceiling is refused with
    /// [`SDKError::InvalidCoordinateInput`].
    #[must_use]
    pub fn with_maximum_zoom_level(mut self, maximum_zoom_level: u8) -> Self {
        self.maximum_zoom_level = maximum_zoom_level;
        self
    }

    /// Reserves `pack_byte_budget_bytes` for the packs this manager defines,
    /// beside the general LRU cache. A download stops before storing a tile
    /// that would take its pack past the budget, fails with
    /// [`SDKError::CacheOperationFailure`] and
    /// [`CacheOperationFailureReason::WriteRejected`], and keeps everything
    /// already stored and its membership, so a later call under a raised
    /// budget resumes the remaining tiles. The budget is its own reserve:
    /// ordinary cache eviction still never removes a present pack row.
    #[must_use]
    pub fn with_pack_byte_budget_bytes(mut self, pack_byte_budget_bytes: u64) -> Self {
        self.pack_byte_budget_bytes = pack_byte_budget_bytes;
        self
    }

    /// The effective payload-byte budget of the packs this manager defines;
    /// [`DEFAULT_TILE_PACK_BYTE_BUDGET_BYTES`] until the application reserves
    /// one.
    pub fn pack_byte_budget_bytes(&self) -> u64 {
        self.pack_byte_budget_bytes
    }

    /// Runs the manager's synchronous store work — defining the pack, the
    /// batched freshness writes and the final record read — through
    /// `blocking_work_executor`, so a target with worker threads keeps SQLite
    /// off the async reactor that drives the provider futures.
    ///
    /// The default is [`InlineBlockingWorkExecutor`], which runs the work
    /// where it is awaited; on a single-threaded target no off-reactor
    /// placement exists.
    #[must_use]
    pub fn with_blocking_work_executor(
        mut self,
        blocking_work_executor: Arc<dyn BlockingWorkExecutor>,
    ) -> Self {
        self.blocking_work_executor = blocking_work_executor;
        self
    }

    /// The retention a pack's entries carry: the requested value (or the
    /// tiles category's default) clamped to the provider's ceiling.
    pub fn effective_time_to_live_seconds(&self, pack_description: &TilePackDescription) -> u64 {
        let requested_time_to_live_seconds = pack_description
            .requested_time_to_live_seconds
            .unwrap_or_else(|| ResponseCategory::RasterTiles.default_time_to_live_seconds());
        match self.provider_terms_ceiling_seconds {
            Some(provider_terms_ceiling_seconds) => {
                requested_time_to_live_seconds.min(provider_terms_ceiling_seconds)
            }
            None => requested_time_to_live_seconds,
        }
    }

    /// Defines (or idempotently re-defines) the pack, downloads its region's
    /// tiles coarse-first through the cached pipeline, and reports the
    /// record as it stands on disk afterwards. The progress callback reports
    /// the running totals (called once with zero completions up front, once
    /// per completed tile).
    ///
    /// The pack and its membership are written before the first tile, so
    /// dropping the returned future at any point is cancellation-safe: every
    /// tile already fetched stays in the store, the membership still names
    /// the missing tiles, and a later call resumes from exactly what is
    /// present instead of redefining the pack from scratch.
    ///
    /// # Errors
    ///
    /// A provider failure keeps its typed error and leaves the partial pack
    /// defined and resumable. A prepare that finishes the download but finds
    /// `present_tile_count < member_tile_count` — the store's write path is
    /// best-effort, so a refused tile write does not fail the fetch — is
    /// reported as [`SDKError::CacheOperationFailure`] with
    /// [`CacheOperationFailureReason::WriteRejected`] rather than success.
    /// The same failure reports a tile that would cross the pack's byte
    /// budget ([`Self::with_pack_byte_budget_bytes`]): that tile is withdrawn
    /// again, the pack stays at or below the budget, and its membership still
    /// names the missing tile so a later call under a raised budget resumes
    /// it.
    pub async fn prepare_tile_pack(
        &self,
        pack_description: &TilePackDescription,
        progress_callback: &mut impl FnMut(TilePackProgress),
    ) -> Result<TilePackRecord, SDKError> {
        if pack_description.requested_time_to_live_seconds == Some(0) {
            return Err(SDKError::InvalidCoordinateInput(
                "a tile pack time-to-live must be positive; zero expires every tile on capture"
                    .to_string(),
            ));
        }
        let effective_time_to_live_seconds = self.effective_time_to_live_seconds(pack_description);
        let enumerated_tiles = enumerate_region_tiles(
            &pack_description.region_bounding_box,
            pack_description.minimum_zoom_level,
            pack_description.maximum_zoom_level,
        )?;
        if pack_description.maximum_zoom_level > self.maximum_zoom_level {
            return Err(SDKError::InvalidCoordinateInput(format!(
                "zoom level {} exceeds the installed provider's declared maximum of {}",
                pack_description.maximum_zoom_level, self.maximum_zoom_level
            )));
        }
        let (tile_addresses, member_fingerprints): (Vec<_>, Vec<_>) = enumerated_tiles
            .into_iter()
            .map(|tile_address| {
                let member_fingerprint = pack_member_fingerprint(
                    self.raster_tile_provider.tile_source_namespace(),
                    &tile_address,
                    pack_description.image_format,
                    pack_description.tile_resolution,
                );
                (tile_address, member_fingerprint)
            })
            .unzip();
        let member_fingerprints = Arc::new(member_fingerprints);
        self.define_tile_pack_through_executor(
            pack_description,
            Arc::clone(&member_fingerprints),
            effective_time_to_live_seconds,
        )
        .await?;
        // The pack's byte budget is enforced as the download goes. With the
        // default unbounded budget there is nothing to read or check, so the
        // download path costs exactly what it did before; a configured budget
        // starts from what an earlier partial download already stored and
        // measures each tile this download stores through its own primary-key
        // lookup. A pack already over budget (the application lowered it below
        // an existing pack) refuses immediately and changes nothing, so
        // raising the budget later resumes it.
        let mut pack_present_payload_bytes =
            if self.pack_byte_budget_bytes == DEFAULT_TILE_PACK_BYTE_BUDGET_BYTES {
                0
            } else {
                let pack_present_payload_bytes = self
                    .tile_pack_record_through_executor(&pack_description.pack_id)
                    .await?
                    .ok_or_else(corrupted_record_error)?
                    .present_payload_bytes;
                if pack_present_payload_bytes > self.pack_byte_budget_bytes {
                    return Err(SDKError::CacheOperationFailure(
                        CacheOperationFailureReason::WriteRejected,
                    ));
                }
                pack_present_payload_bytes
            };
        let total_tile_count = tile_addresses.len() as u64;
        let mut progress = TilePackProgress {
            total_tile_count,
            completed_tile_count: 0,
            network_fetched_tile_count: 0,
            disk_served_tile_count: 0,
        };
        progress_callback(progress);
        // Freshness updates are batched: one guarded write covers a chunk of
        // downloaded tiles instead of one write per tile. Tiles fetched
        // before a failure are still refreshed, matching the per-tile
        // behavior the batching replaced.
        let mut pending_refresh_fingerprints: Vec<String> =
            Vec::with_capacity(FRESHNESS_UPDATE_FINGERPRINTS_PER_STATEMENT);
        let mut fetch_result: Result<(), SDKError> = Ok(());
        for (tile_address, tile_fingerprint) in
            tile_addresses.iter().zip(member_fingerprints.iter())
        {
            let raster_tile_provider = self.raster_tile_provider.clone();
            let fetch_outcome = match raster_tile_provider
                .fetch_raster_tile_with_cache_outcome(
                    tile_address.tile_x,
                    tile_address.tile_y,
                    tile_address.zoom_level,
                    pack_description.image_format,
                    pack_description.tile_resolution,
                )
                .await
            {
                Ok((_fetched_tile, fetch_outcome)) => fetch_outcome,
                Err(fetch_error) => {
                    fetch_result = Err(fetch_error);
                    break;
                }
            };
            if self.pack_byte_budget_bytes != DEFAULT_TILE_PACK_BYTE_BUDGET_BYTES
                && fetch_outcome == CacheThenNetworkOutcome::NetworkFetch
                && let Some(just_stored_tile_bytes) = self
                    .response_cache
                    .entry_payload_size_bytes(tile_fingerprint)?
            {
                if pack_present_payload_bytes.saturating_add(just_stored_tile_bytes)
                    > self.pack_byte_budget_bytes
                {
                    // Withdraw the tile that crossed the budget, so the
                    // stored pack stays within it; the membership row
                    // remains, so a later raised-budget call retries this
                    // tile. Only a tile this download just stored reaches
                    // this path (a cache hit was already counted).
                    self.response_cache.remove_entry(tile_fingerprint)?;
                    fetch_result = Err(SDKError::CacheOperationFailure(
                        CacheOperationFailureReason::WriteRejected,
                    ));
                    break;
                }
                pack_present_payload_bytes += just_stored_tile_bytes;
            }
            pending_refresh_fingerprints.push(tile_fingerprint.clone());
            match fetch_outcome {
                CacheThenNetworkOutcome::NetworkFetch => progress.network_fetched_tile_count += 1,
                CacheThenNetworkOutcome::CacheHit => progress.disk_served_tile_count += 1,
            }
            progress.completed_tile_count += 1;
            progress_callback(progress);
            if pending_refresh_fingerprints.len() >= FRESHNESS_UPDATE_FINGERPRINTS_PER_STATEMENT {
                let batch_fingerprints = std::mem::replace(
                    &mut pending_refresh_fingerprints,
                    Vec::with_capacity(FRESHNESS_UPDATE_FINGERPRINTS_PER_STATEMENT),
                );
                self.refresh_member_tile_freshnesses_through_executor(
                    &pack_description.pack_id,
                    batch_fingerprints,
                    effective_time_to_live_seconds,
                )
                .await?;
            }
        }
        let refresh_result = self
            .refresh_member_tile_freshnesses_through_executor(
                &pack_description.pack_id,
                std::mem::take(&mut pending_refresh_fingerprints),
                effective_time_to_live_seconds,
            )
            .await;
        // A fetch failure keeps its precedence; the flush of the tiles that
        // did download is best-effort in that case.
        if fetch_result.is_ok() {
            refresh_result?;
        }
        fetch_result?;
        let pack_record = self
            .tile_pack_record_through_executor(&pack_description.pack_id)
            .await?
            .ok_or_else(corrupted_record_error)?;
        if pack_record.present_tile_count < pack_record.member_tile_count {
            return Err(SDKError::CacheOperationFailure(
                CacheOperationFailureReason::WriteRejected,
            ));
        }
        Ok(pack_record)
    }

    /// Defines the pack and its membership through the blocking executor;
    /// membership travels as a shared vector so the download loop keeps its
    /// fingerprints without copying them per tile.
    async fn define_tile_pack_through_executor(
        &self,
        pack_description: &TilePackDescription,
        member_fingerprints: Arc<Vec<String>>,
        effective_time_to_live_seconds: u64,
    ) -> Result<(), SDKError> {
        let response_cache = Arc::clone(&self.response_cache);
        let tile_source_namespace = self.tile_source_namespace().to_string();
        let pack_description = pack_description.clone();
        run_blocking(self.blocking_work_executor.as_ref(), move || {
            response_cache.define_tile_pack(
                &tile_source_namespace,
                &pack_description,
                member_fingerprints.as_slice(),
                effective_time_to_live_seconds,
            )
        })
        .await
    }

    /// Persists one batch of downloaded members' freshness through the
    /// blocking executor.
    async fn refresh_member_tile_freshnesses_through_executor(
        &self,
        pack_id: &str,
        tile_fingerprints: Vec<String>,
        effective_time_to_live_seconds: u64,
    ) -> Result<(), SDKError> {
        let response_cache = Arc::clone(&self.response_cache);
        let tile_source_namespace = self.tile_source_namespace().to_string();
        let pack_id = pack_id.to_string();
        run_blocking(self.blocking_work_executor.as_ref(), move || {
            response_cache.refresh_member_tile_freshnesses(
                &tile_source_namespace,
                &pack_id,
                &tile_fingerprints,
                effective_time_to_live_seconds,
            )
        })
        .await
    }

    /// Reads the final pack record through the blocking executor.
    async fn tile_pack_record_through_executor(
        &self,
        pack_id: &str,
    ) -> Result<Option<TilePackRecord>, SDKError> {
        let response_cache = Arc::clone(&self.response_cache);
        let tile_source_namespace = self.tile_source_namespace().to_string();
        let pack_id = pack_id.to_string();
        run_blocking(self.blocking_work_executor.as_ref(), move || {
            response_cache.tile_pack_record(&tile_source_namespace, &pack_id)
        })
        .await
    }

    /// The namespace every pack this manager defines belongs to: the
    /// installed cached raster provider's own.
    fn tile_source_namespace(&self) -> &str {
        self.raster_tile_provider.tile_source_namespace()
    }

    /// The stated pack's on-disk record, or `None` when not defined.
    pub fn tile_pack_state(&self, pack_id: &str) -> Result<Option<TilePackRecord>, SDKError> {
        self.response_cache
            .tile_pack_record(self.tile_source_namespace(), pack_id)
    }

    /// Every defined pack's on-disk record.
    pub fn tile_pack_states(&self) -> Result<Vec<TilePackRecord>, SDKError> {
        self.response_cache
            .tile_pack_records(self.tile_source_namespace())
    }

    /// Explicitly evicts the pack's tiles and its record. Ordinary cache
    /// entries and other live packs' tiles are left in place (a tile two
    /// packs share stays until both evict).
    pub fn evict_tile_pack(&self, pack_id: &str) -> Result<TilePackRemovalReport, SDKError> {
        self.response_cache
            .remove_tile_pack(self.tile_source_namespace(), pack_id)
    }
}

#[cfg(test)]
mod tests {
    use super::{TilePackDescription, TilePackManager, TilePackProgress, enumerate_region_tiles};

    /// The namespace the tile-pack tests fingerprint under.
    const TEST_TILE_SOURCE_NAMESPACE: &str = "test-provider";
    use crate::FixedDiskSpaceProbe;
    use crate::disk_quota::{CacheQuota, DiskSpaceProbe};
    use crate::sqlite_store::{SqliteResponseCache, SqliteResponseCacheConfiguration};
    use async_trait::async_trait;
    use geospatial_traits::RasterTileProvider;
    use geospatial_types::{
        BoundingBox, CacheOperationFailureReason, GeographicCoordinate, RasterImageFormat,
        RasterTileData, RasterTileResolution, SDKError,
    };
    use rustic_maps_core::{BlockingWorkExecutor, CachedResponseStore};
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    fn coordinate(latitude: f64, longitude: f64) -> GeographicCoordinate {
        GeographicCoordinate::from_degrees(latitude, longitude).unwrap()
    }

    fn sacramento_region() -> BoundingBox {
        BoundingBox::from_corners(coordinate(38.57, -121.5), coordinate(38.58, -121.49)).unwrap()
    }

    fn pack_description(pack_id: &str, region: BoundingBox, zoom_level: u8) -> TilePackDescription {
        TilePackDescription {
            pack_id: pack_id.to_string(),
            display_name: format!("{pack_id} display"),
            region_bounding_box: region,
            minimum_zoom_level: zoom_level,
            maximum_zoom_level: zoom_level,
            image_format: RasterImageFormat::Png,
            tile_resolution: RasterTileResolution::Standard,
            requested_time_to_live_seconds: None,
        }
    }

    /// The scripted double's address-keyed outcome table.
    type ScriptedTileOutcomes = HashMap<(u32, u32, u8), Result<Vec<u8>, String>>;

    /// Network-free provider double: canned tile payloads keyed by the
    /// XYZ address, every request recorded, failing tiles scripted too.
    struct ScriptedTileProvider {
        scripted_tiles: Mutex<ScriptedTileOutcomes>,
        requested_tiles: Mutex<Vec<(u32, u32, u8)>>,
    }

    impl ScriptedTileProvider {
        fn serving_tiles(
            served_tiles: impl IntoIterator<Item = ((u32, u32, u8), Vec<u8>)>,
        ) -> Arc<Self> {
            Arc::new(Self {
                scripted_tiles: Mutex::new(
                    served_tiles
                        .into_iter()
                        .map(|(tile_address, image_bytes)| (tile_address, Ok(image_bytes)))
                        .collect(),
                ),
                requested_tiles: Mutex::new(Vec::new()),
            })
        }

        fn failing_tiles(
            served_tiles: impl IntoIterator<Item = ((u32, u32, u8), Vec<u8>)>,
            failed_tiles: impl IntoIterator<Item = (u32, u32, u8)>,
        ) -> Arc<Self> {
            Arc::new(Self {
                scripted_tiles: Mutex::new(
                    served_tiles
                        .into_iter()
                        .map(|(tile_address, image_bytes)| (tile_address, Ok(image_bytes)))
                        .chain(
                            failed_tiles
                                .into_iter()
                                .map(|tile_address| (tile_address, Err("unroutable".to_string()))),
                        )
                        .collect(),
                ),
                requested_tiles: Mutex::new(Vec::new()),
            })
        }

        fn recorded_requests(&self) -> Vec<(u32, u32, u8)> {
            self.requested_tiles.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl RasterTileProvider for ScriptedTileProvider {
        async fn fetch_raster_tile(
            &self,
            tile_x: u32,
            tile_y: u32,
            zoom_level: u8,
            _image_format: RasterImageFormat,
            _tile_resolution: RasterTileResolution,
        ) -> Result<RasterTileData, SDKError> {
            self.requested_tiles
                .lock()
                .unwrap()
                .push((tile_x, tile_y, zoom_level));
            let scripted_payload = self
                .scripted_tiles
                .lock()
                .unwrap()
                .get(&(tile_x, tile_y, zoom_level))
                .cloned();
            match scripted_payload {
                Some(Ok(image_bytes)) => Ok(RasterTileData {
                    tile_x,
                    tile_y,
                    zoom_level,
                    image_format: RasterImageFormat::Png,
                    tile_resolution: RasterTileResolution::Standard,
                    image_bytes,
                }),
                Some(Err(detail)) => Err(SDKError::NetworkRequestFailed(detail)),
                None => Err(SDKError::ResponseParsingFailure(
                    "tile not scripted".to_string(),
                )),
            }
        }
    }

    /// Network-free provider double that serves scripted tiles and then
    /// stops on a chosen request count, signalling the test so it can drop
    /// the in-flight prepare future at that exact point. The stop happens
    /// once: after the test takes the signal sender, later requests serve
    /// normally, which is what a resumed prepare needs.
    struct PausingTileProvider {
        served_tiles: Mutex<HashMap<(u32, u32, u8), Vec<u8>>>,
        requested_tiles: Mutex<Vec<(u32, u32, u8)>>,
        pause_after_requested_tile_count: usize,
        pause_signal_sender: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    }

    impl PausingTileProvider {
        fn new(
            served_tiles: impl IntoIterator<Item = ((u32, u32, u8), Vec<u8>)>,
            pause_after_requested_tile_count: usize,
            pause_signal_sender: tokio::sync::oneshot::Sender<()>,
        ) -> Arc<Self> {
            Arc::new(Self {
                served_tiles: Mutex::new(served_tiles.into_iter().collect()),
                requested_tiles: Mutex::new(Vec::new()),
                pause_after_requested_tile_count,
                pause_signal_sender: Mutex::new(Some(pause_signal_sender)),
            })
        }

        fn recorded_requests(&self) -> Vec<(u32, u32, u8)> {
            self.requested_tiles.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl RasterTileProvider for PausingTileProvider {
        async fn fetch_raster_tile(
            &self,
            tile_x: u32,
            tile_y: u32,
            zoom_level: u8,
            _image_format: RasterImageFormat,
            _tile_resolution: RasterTileResolution,
        ) -> Result<RasterTileData, SDKError> {
            let requested_tile_count = {
                let mut requested_tiles = self.requested_tiles.lock().unwrap();
                requested_tiles.push((tile_x, tile_y, zoom_level));
                requested_tiles.len()
            };
            if requested_tile_count > self.pause_after_requested_tile_count {
                let pause_signal_sender = self.pause_signal_sender.lock().unwrap().take();
                if let Some(pause_signal_sender) = pause_signal_sender {
                    let _ = pause_signal_sender.send(());
                    std::future::pending::<()>().await;
                }
            }
            let image_bytes = self
                .served_tiles
                .lock()
                .unwrap()
                .get(&(tile_x, tile_y, zoom_level))
                .cloned()
                .ok_or_else(|| SDKError::ResponseParsingFailure("tile not scripted".to_string()))?;
            Ok(RasterTileData {
                tile_x,
                tile_y,
                zoom_level,
                image_format: RasterImageFormat::Png,
                tile_resolution: RasterTileResolution::Standard,
                image_bytes,
            })
        }
    }

    /// Executor double proving the manager routes its synchronous store work
    /// through the injected seam rather than calling it inline itself.
    #[derive(Default)]
    struct CountingBlockingWorkExecutor {
        run_count: AtomicUsize,
    }

    impl BlockingWorkExecutor for CountingBlockingWorkExecutor {
        fn run_blocking(
            &self,
            work: Box<dyn FnOnce() -> Box<dyn std::any::Any + Send> + Send + 'static>,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Box<dyn std::any::Any + Send>> + Send + 'static>,
        > {
            self.run_count.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move { work() })
        }
    }

    fn unique_database_path(test_name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "rustic-maps-cache-tile-pack-{}-{test_name}.sqlite",
            std::process::id()
        ))
    }

    fn open_pack_store_with_probe(
        test_name: &str,
        quota: CacheQuota,
        available_disk_bytes: Arc<Mutex<u64>>,
    ) -> Arc<SqliteResponseCache> {
        struct MutableDiskSpaceProbe(Arc<Mutex<u64>>);
        impl DiskSpaceProbe for MutableDiskSpaceProbe {
            fn available_disk_bytes(&self, _cache_file_path: &Path) -> Result<u64, SDKError> {
                Ok(*self.0.lock().unwrap())
            }
        }
        let database_path = unique_database_path(test_name);
        std::fs::remove_file(database_path.with_extension("sqlite-wal")).ok();
        std::fs::remove_file(database_path.with_extension("sqlite-shm")).ok();
        std::fs::remove_file(&database_path).ok();
        SqliteResponseCache::with_disk_space_probe(
            SqliteResponseCacheConfiguration {
                database_path,
                quota,
                ..SqliteResponseCacheConfiguration::default()
            },
            Arc::new(MutableDiskSpaceProbe(available_disk_bytes)),
        )
        .unwrap()
    }

    fn open_pack_store(test_name: &str) -> Arc<SqliteResponseCache> {
        SqliteResponseCache::with_disk_space_probe(
            SqliteResponseCacheConfiguration {
                database_path: {
                    let database_path = unique_database_path(test_name);
                    std::fs::remove_file(database_path.with_extension("sqlite-wal")).ok();
                    std::fs::remove_file(database_path.with_extension("sqlite-shm")).ok();
                    std::fs::remove_file(&database_path).ok();
                    database_path
                },
                ..SqliteResponseCacheConfiguration::default()
            },
            Arc::new(FixedDiskSpaceProbe::reporting(u64::MAX)),
        )
        .unwrap()
    }

    fn prepared_manager<RasterProvider>(
        test_name: &str,
        provider_terms_ceiling_seconds: Option<u64>,
        scripted_tile_provider: &Arc<RasterProvider>,
    ) -> (Arc<SqliteResponseCache>, TilePackManager)
    where
        RasterProvider: RasterTileProvider + 'static,
    {
        let response_cache = open_pack_store(test_name);
        let cached_raster_tile_provider = Arc::new(crate::CachedRasterTileProvider::new(
            scripted_tile_provider.clone(),
            response_cache.clone(),
            TEST_TILE_SOURCE_NAMESPACE,
        ));
        let tile_pack_manager = TilePackManager::new(
            cached_raster_tile_provider,
            response_cache.clone(),
            provider_terms_ceiling_seconds,
        );
        (response_cache, tile_pack_manager)
    }

    /// A progress recorder: the tests pass
    /// `progress_recorder_callback(&mut recorder)` into a prepare and
    /// assert over the drained reports afterwards.
    struct ProgressRecorder {
        reports: Mutex<Vec<TilePackProgress>>,
    }

    impl ProgressRecorder {
        fn new() -> Self {
            Self {
                reports: Mutex::new(Vec::new()),
            }
        }

        fn drained(&self) -> Vec<TilePackProgress> {
            self.reports.lock().unwrap().clone()
        }
    }

    fn progress_recorder_callback(
        progress_recorder: &mut ProgressRecorder,
    ) -> impl FnMut(TilePackProgress) + '_ {
        move |tile_pack_progress| {
            progress_recorder
                .reports
                .lock()
                .unwrap()
                .push(tile_pack_progress)
        }
    }

    #[tokio::test]
    async fn region_enumeration_lists_exactly_the_tiles_overlapping_the_region() {
        // Sacramento sits inside the single zoom-3 tile (1, 3); zoom 4 keeps it
        // in tile (2, 6).
        let tile_addresses = enumerate_region_tiles(&sacramento_region(), 3, 3).unwrap();
        assert_eq!(
            tile_addresses,
            vec![super::TilePackTileAddress {
                tile_x: 1,
                tile_y: 3,
                zoom_level: 3
            }]
        );
        let two_level_addresses = enumerate_region_tiles(&sacramento_region(), 3, 4).unwrap();
        assert_eq!(
            two_level_addresses,
            vec![
                super::TilePackTileAddress {
                    tile_x: 1,
                    tile_y: 3,
                    zoom_level: 3
                },
                super::TilePackTileAddress {
                    tile_x: 2,
                    tile_y: 6,
                    zoom_level: 4
                },
            ],
            "coarse zooms list first so the fallback covers the region early"
        );
    }

    #[test]
    fn boundary_aligned_region_edges_neither_widen_nor_shrink_the_region() {
        // The box's east edge lands exactly on the zoom-1 tile boundary at
        // tile x = 1 (longitude 0): tile 1 must not join the pack.
        let west_box =
            BoundingBox::from_corners(coordinate(0.0, -90.0), coordinate(30.0, 0.0)).unwrap();
        assert_eq!(
            enumerate_region_tiles(&west_box, 1, 1)
                .unwrap()
                .iter()
                .map(|tile_address| (tile_address.tile_x, tile_address.tile_y))
                .collect::<Vec<_>>(),
            vec![(0, 0)],
            "a tile-edge-aligned east boundary excludes the tile east of it"
        );
        let east_box =
            BoundingBox::from_corners(coordinate(0.0, 0.0), coordinate(30.0, 90.0)).unwrap();
        assert_eq!(
            enumerate_region_tiles(&east_box, 1, 1)
                .unwrap()
                .iter()
                .map(|tile_address| (tile_address.tile_x, tile_address.tile_y))
                .collect::<Vec<_>>(),
            vec![(1, 0)],
            "a tile-edge-aligned west boundary includes the tile starting on it"
        );
    }

    #[test]
    fn invalid_zoom_ranges_and_oversized_regions_are_typed_rejections() {
        let region = sacramento_region();
        let inverted = enumerate_region_tiles(&region, 5, 4);
        assert!(matches!(inverted, Err(SDKError::InvalidCoordinateInput(_))));
        let beyond_the_representable_plane = enumerate_region_tiles(&region, 31, 32);
        assert!(matches!(
            beyond_the_representable_plane,
            Err(SDKError::InvalidCoordinateInput(_))
        ));
        let world_at_street_zoom = enumerate_region_tiles(
            &BoundingBox::from_corners(coordinate(-85.0, -179.0), coordinate(85.0, 179.0)).unwrap(),
            17,
            17,
        );
        assert!(matches!(
            world_at_street_zoom,
            Err(SDKError::InvalidCoordinateInput(_))
        ));
    }

    #[tokio::test]
    async fn a_pack_deeper_than_the_provider_declared_zoom_ceiling_is_refused() {
        let scripted_tile_provider =
            ScriptedTileProvider::serving_tiles([((3, 3, 21), vec![7; 64])]);
        let (_response_cache, tile_pack_manager) =
            prepared_manager("provider-zoom-ceiling", None, &scripted_tile_provider);
        let tile_pack_manager = tile_pack_manager.with_maximum_zoom_level(20);
        let too_deep_pack = pack_description("too-deep", sacramento_region(), 21);
        let failure = tile_pack_manager
            .prepare_tile_pack(&too_deep_pack, &mut |_| {})
            .await;
        assert!(
            matches!(failure, Err(SDKError::InvalidCoordinateInput(_))),
            "a pack above the provider's declared maximum is refused, got {failure:?}"
        );
        assert!(
            tile_pack_manager
                .tile_pack_state("too-deep")
                .unwrap()
                .is_none(),
            "a refused pack defines nothing and downloads nothing"
        );
        assert!(
            scripted_tile_provider.recorded_requests().is_empty(),
            "the provider ceiling is enforced before any tile is requested"
        );
    }

    #[tokio::test]
    async fn prepare_downloads_the_region_and_reports_it_on_disk() {
        let scripted_tile_provider = ScriptedTileProvider::serving_tiles(
            [(1, 3, 3), (2, 6, 4)].map(|tile_address| (tile_address, vec![7; 64])),
        );
        let (_response_cache, tile_pack_manager) =
            prepared_manager("prepare-reports", None, &scripted_tile_provider);
        let mut pack_description = pack_description("sacramento", sacramento_region(), 3);
        pack_description.maximum_zoom_level = 4;
        let mut progress_recorder = ProgressRecorder::new();
        let pack_record = tile_pack_manager
            .prepare_tile_pack(
                &pack_description,
                &mut progress_recorder_callback(&mut progress_recorder),
            )
            .await
            .unwrap();
        assert_eq!(pack_record.pack_id, "sacramento");
        assert_eq!(pack_record.display_name, "sacramento display");
        assert_eq!(pack_record.minimum_zoom_level, 3);
        assert_eq!(pack_record.maximum_zoom_level, 4);
        assert_eq!(pack_record.image_format, RasterImageFormat::Png);
        assert_eq!(pack_record.tile_resolution, RasterTileResolution::Standard);
        assert_eq!(pack_record.member_tile_count, 2);
        assert_eq!(pack_record.present_tile_count, 2);
        assert!(pack_record.present_payload_bytes > 0);
        assert_eq!(
            pack_record.effective_time_to_live_seconds,
            30 * 24 * 60 * 60
        );
        // The honest progress shape: one zero report up front, one per tile.
        let progress_reports = progress_recorder.drained();
        assert_eq!(progress_reports.len(), 3);
        assert_eq!(
            progress_reports[0],
            TilePackProgress {
                total_tile_count: 2,
                completed_tile_count: 0,
                network_fetched_tile_count: 0,
                disk_served_tile_count: 0,
            }
        );
        let final_progress = progress_reports[2];
        assert_eq!(final_progress.network_fetched_tile_count, 2);
        assert_eq!(final_progress.disk_served_tile_count, 0);
        assert_eq!(final_progress.completed_tile_count, 2);
    }

    #[tokio::test]
    async fn prepare_routes_its_store_work_through_the_blocking_executor() {
        let scripted_tile_provider = ScriptedTileProvider::serving_tiles(
            [(1, 3, 3), (2, 6, 4)].map(|tile_address| (tile_address, vec![7; 64])),
        );
        let (_response_cache, tile_pack_manager) =
            prepared_manager("blocking-executor", None, &scripted_tile_provider);
        let blocking_work_executor = Arc::new(CountingBlockingWorkExecutor::default());
        let tile_pack_manager =
            tile_pack_manager.with_blocking_work_executor(blocking_work_executor.clone());
        let mut pack_description = pack_description("sacramento", sacramento_region(), 3);
        pack_description.maximum_zoom_level = 4;
        let pack_record = tile_pack_manager
            .prepare_tile_pack(&pack_description, &mut |_| {})
            .await
            .unwrap();
        assert_eq!(pack_record.present_tile_count, 2);
        assert_eq!(
            blocking_work_executor.run_count.load(Ordering::SeqCst),
            3,
            "the pack definition, the final freshness flush and the record read each run through the executor"
        );
    }

    #[tokio::test]
    async fn dropping_the_prepare_future_leaves_the_partial_pack_resumable() {
        let (pause_signal_sender, pause_signal_receiver) = tokio::sync::oneshot::channel();
        let pausing_tile_provider = PausingTileProvider::new(
            [(1, 3, 3), (2, 6, 4)].map(|tile_address| (tile_address, vec![7; 64])),
            1,
            pause_signal_sender,
        );
        let (_response_cache, tile_pack_manager) =
            prepared_manager("cancelled", None, &pausing_tile_provider);
        let mut pack_description = pack_description("sacramento", sacramento_region(), 3);
        pack_description.maximum_zoom_level = 4;
        let mut progress_callback = |_| {};
        {
            let prepare_future =
                tile_pack_manager.prepare_tile_pack(&pack_description, &mut progress_callback);
            tokio::pin!(prepare_future);
            tokio::select! {
                pause_signal = pause_signal_receiver => {
                    pause_signal.expect("the provider pauses on the request after the first tile")
                }
                prepare_result = &mut prepare_future => {
                    panic!("the prepare completed before it paused: {prepare_result:?}")
                }
            }
        }
        // The future is dropped here, mid-download. The first tile's store
        // write completed and the membership was written before it.
        let partial_record = tile_pack_manager
            .tile_pack_state("sacramento")
            .unwrap()
            .expect("the partial pack stays defined when the future is dropped");
        assert_eq!(partial_record.member_tile_count, 2);
        assert_eq!(partial_record.present_tile_count, 1);
        assert!(partial_record.present_tile_count < partial_record.member_tile_count);
        // The resume serves the stored tile from disk and fetches only the
        // tile that was missing when the future was dropped.
        let resumed_record = tile_pack_manager
            .prepare_tile_pack(&pack_description, &mut |_| {})
            .await
            .unwrap();
        assert_eq!(resumed_record.member_tile_count, 2);
        assert_eq!(resumed_record.present_tile_count, 2);
        assert_eq!(
            pausing_tile_provider.recorded_requests(),
            vec![(1, 3, 3), (2, 6, 4), (2, 6, 4)],
            "the resume does not redefine or refetch the tile already stored"
        );
    }

    #[tokio::test]
    async fn an_incomplete_prepare_is_a_typed_failure_and_the_resume_succeeds() {
        // A one-byte budget refuses every tile write through the store's
        // best-effort path, so the download finishes but stores nothing.
        let available_disk_bytes = Arc::new(Mutex::new(1_u64));
        let scripted_tile_provider = ScriptedTileProvider::serving_tiles(
            [(1, 3, 3), (2, 6, 4)].map(|tile_address| (tile_address, vec![7; 64])),
        );
        let response_cache = open_pack_store_with_probe(
            "incomplete",
            CacheQuota {
                free_disk_share_percentage: Some(100.0),
                ..CacheQuota::default()
            },
            available_disk_bytes.clone(),
        );
        let cached_raster_tile_provider = Arc::new(crate::CachedRasterTileProvider::new(
            scripted_tile_provider.clone(),
            response_cache.clone(),
            TEST_TILE_SOURCE_NAMESPACE,
        ));
        let tile_pack_manager =
            TilePackManager::new(cached_raster_tile_provider, response_cache.clone(), None);
        let mut pack_description = pack_description("sacramento", sacramento_region(), 3);
        pack_description.maximum_zoom_level = 4;
        let incomplete_prepare = tile_pack_manager
            .prepare_tile_pack(&pack_description, &mut |_| {})
            .await;
        assert!(
            matches!(
                incomplete_prepare,
                Err(SDKError::CacheOperationFailure(
                    CacheOperationFailureReason::WriteRejected
                ))
            ),
            "an incomplete pack is a typed failure, got {incomplete_prepare:?}"
        );
        let partial_record = tile_pack_manager
            .tile_pack_state("sacramento")
            .unwrap()
            .expect("the partial pack stays defined and resumable");
        assert_eq!(partial_record.member_tile_count, 2);
        assert!(partial_record.present_tile_count < partial_record.member_tile_count);
        // With room in the budget the same prepare completes and succeeds.
        *available_disk_bytes.lock().unwrap() = 10_000_000;
        let completed_record = tile_pack_manager
            .prepare_tile_pack(&pack_description, &mut |_| {})
            .await
            .unwrap();
        assert_eq!(completed_record.member_tile_count, 2);
        assert_eq!(completed_record.present_tile_count, 2);
    }

    /// Builds a tile-pack manager over `response_cache` and `scripted_tile_provider`
    /// with the stated pack byte budget, sharing the store with any manager a
    /// test already holds.
    fn pack_manager_with_byte_budget<RasterProvider>(
        response_cache: &Arc<SqliteResponseCache>,
        scripted_tile_provider: &Arc<RasterProvider>,
        pack_byte_budget_bytes: u64,
    ) -> TilePackManager
    where
        RasterProvider: RasterTileProvider + 'static,
    {
        TilePackManager::new(
            Arc::new(crate::CachedRasterTileProvider::new(
                scripted_tile_provider.clone(),
                response_cache.clone(),
                TEST_TILE_SOURCE_NAMESPACE,
            )),
            response_cache.clone(),
            None,
        )
        .with_pack_byte_budget_bytes(pack_byte_budget_bytes)
    }

    /// Measures one stored tile's payload bytes by preparing a one-tile pack
    /// over the shared store and then evicting it. The scripts the budget
    /// tests run expect the tile's exact stored size, which only the store's
    /// own serialized payload reports.
    async fn measured_single_tile_payload_bytes(
        response_cache: &Arc<SqliteResponseCache>,
        scripted_tile_provider: &Arc<ScriptedTileProvider>,
    ) -> u64 {
        let measuring_manager =
            pack_manager_with_byte_budget(response_cache, scripted_tile_provider, u64::MAX);
        let measured_record = measuring_manager
            .prepare_tile_pack(
                &pack_description("measure", sacramento_region(), 3),
                &mut |_| {},
            )
            .await
            .unwrap();
        let single_tile_payload_bytes = measured_record.present_payload_bytes;
        assert!(single_tile_payload_bytes > 0);
        measuring_manager
            .evict_tile_pack("measure")
            .expect("the measuring pack evicts");
        single_tile_payload_bytes
    }

    #[tokio::test]
    async fn a_download_past_the_pack_byte_budget_keeps_the_partial_pack() {
        let scripted_tile_provider = ScriptedTileProvider::serving_tiles(
            [(1, 3, 3), (2, 6, 4)].map(|tile_address| (tile_address, vec![7; 64])),
        );
        let (response_cache, _unbounded_manager) =
            prepared_manager("pack-budget-partial", None, &scripted_tile_provider);
        let single_tile_payload_bytes =
            measured_single_tile_payload_bytes(&response_cache, &scripted_tile_provider).await;
        scripted_tile_provider
            .requested_tiles
            .lock()
            .unwrap()
            .clear();
        let budgeted_manager = pack_manager_with_byte_budget(
            &response_cache,
            &scripted_tile_provider,
            single_tile_payload_bytes,
        );
        assert_eq!(
            budgeted_manager.pack_byte_budget_bytes(),
            single_tile_payload_bytes,
            "the readback reports the configured budget"
        );
        let mut pack_description = pack_description("sacramento", sacramento_region(), 3);
        pack_description.maximum_zoom_level = 4;
        let refused_prepare = budgeted_manager
            .prepare_tile_pack(&pack_description, &mut |_| {})
            .await;
        assert!(
            matches!(
                refused_prepare,
                Err(SDKError::CacheOperationFailure(
                    CacheOperationFailureReason::WriteRejected
                ))
            ),
            "a download past the pack budget is the existing cache write refusal, \
             got {refused_prepare:?}"
        );
        let partial_record = budgeted_manager
            .tile_pack_state("sacramento")
            .unwrap()
            .expect("the partial pack stays defined and resumable");
        assert_eq!(partial_record.member_tile_count, 2);
        assert_eq!(
            partial_record.present_tile_count, 1,
            "the tile that would cross the budget is withdrawn, not stored past it"
        );
        assert_eq!(
            partial_record.present_payload_bytes, single_tile_payload_bytes,
            "the stored pack stays exactly within its budget"
        );
        assert_eq!(
            scripted_tile_provider.recorded_requests(),
            vec![(1, 3, 3), (2, 6, 4)],
            "the download stops after fetching the tile that crosses the budget"
        );
    }

    #[tokio::test]
    async fn a_resumed_download_completes_under_a_raised_pack_byte_budget() {
        let scripted_tile_provider = ScriptedTileProvider::serving_tiles(
            [(1, 3, 3), (2, 6, 4)].map(|tile_address| (tile_address, vec![7; 64])),
        );
        let (response_cache, _unbounded_manager) =
            prepared_manager("pack-budget-resume", None, &scripted_tile_provider);
        let single_tile_payload_bytes =
            measured_single_tile_payload_bytes(&response_cache, &scripted_tile_provider).await;
        scripted_tile_provider
            .requested_tiles
            .lock()
            .unwrap()
            .clear();
        let budgeted_manager = pack_manager_with_byte_budget(
            &response_cache,
            &scripted_tile_provider,
            single_tile_payload_bytes,
        );
        let mut pack_description = pack_description("sacramento", sacramento_region(), 3);
        pack_description.maximum_zoom_level = 4;
        budgeted_manager
            .prepare_tile_pack(&pack_description, &mut |_| {})
            .await
            .expect_err("the download crosses the initial budget");
        // The application raises the reserve; the same store and pack resume
        // the tile that was withdrawn.
        let raised_budget_manager = pack_manager_with_byte_budget(
            &response_cache,
            &scripted_tile_provider,
            single_tile_payload_bytes.saturating_mul(4),
        );
        let completed_record = raised_budget_manager
            .prepare_tile_pack(&pack_description, &mut |_| {})
            .await
            .expect("the resumed download fits the raised budget");
        assert_eq!(completed_record.member_tile_count, 2);
        assert_eq!(completed_record.present_tile_count, 2);
        assert_eq!(
            scripted_tile_provider.recorded_requests(),
            vec![(1, 3, 3), (2, 6, 4), (2, 6, 4)],
            "the resume serves the stored tile and refetches only the withdrawn one"
        );
    }

    #[tokio::test]
    async fn ordinary_eviction_never_frees_a_pack_pinned_row() {
        let scripted_tile_provider =
            ScriptedTileProvider::serving_tiles([((1, 3, 3), vec![7; 64])]);
        let shared_general_budget_bytes = Arc::new(Mutex::new(u64::MAX));
        let response_cache = open_pack_store_with_probe(
            "pack-pinned-row",
            CacheQuota {
                free_disk_share_percentage: Some(100.0),
                ..CacheQuota::default()
            },
            shared_general_budget_bytes.clone(),
        );
        let tile_pack_manager =
            pack_manager_with_byte_budget(&response_cache, &scripted_tile_provider, u64::MAX);
        let pack_record = tile_pack_manager
            .prepare_tile_pack(
                &pack_description("pinned", sacramento_region(), 3),
                &mut |_| {},
            )
            .await
            .unwrap();
        let pack_payload_bytes = pack_record.present_payload_bytes;
        assert!(pack_payload_bytes > 0);
        // The general budget sits just above the pack: the small ordinary
        // value is accepted, its write pushes the total over, and the LRU
        // pass may remove only that ordinary entry.
        *shared_general_budget_bytes.lock().unwrap() = pack_payload_bytes + 10;
        CachedResponseStore::<String>::store_response(
            response_cache.as_ref(),
            "ordinary:pressure",
            Arc::new("x".repeat(100)),
            3_600,
        )
        .unwrap();
        let pinned_tile = CachedResponseStore::<RasterTileData>::lookup_response(
            response_cache.as_ref(),
            &super::pack_member_fingerprint(
                TEST_TILE_SOURCE_NAMESPACE,
                &super::TilePackTileAddress {
                    tile_x: 1,
                    tile_y: 3,
                    zoom_level: 3,
                },
                RasterImageFormat::Png,
                RasterTileResolution::Standard,
            ),
        )
        .unwrap();
        assert!(
            pinned_tile.is_some(),
            "eviction pressure must never free a pack-pinned row"
        );
        assert_eq!(
            tile_pack_manager
                .tile_pack_state("pinned")
                .unwrap()
                .unwrap()
                .present_tile_count,
            1
        );
    }

    #[tokio::test]
    async fn re_prepare_of_the_present_region_refetches_nothing() {
        let scripted_tile_provider = ScriptedTileProvider::serving_tiles(
            [(1, 3, 3), (2, 6, 4)].map(|tile_address| (tile_address, vec![7; 64])),
        );
        let (_response_cache, tile_pack_manager) =
            prepared_manager("re-prepare", None, &scripted_tile_provider);
        let mut pack_description = pack_description("sacramento", sacramento_region(), 3);
        pack_description.maximum_zoom_level = 4;
        let mut progress_recorder = ProgressRecorder::new();
        let first_record = tile_pack_manager
            .prepare_tile_pack(
                &pack_description,
                &mut progress_recorder_callback(&mut progress_recorder),
            )
            .await
            .unwrap();
        let second_record = tile_pack_manager
            .prepare_tile_pack(
                &pack_description,
                &mut progress_recorder_callback(&mut progress_recorder),
            )
            .await
            .unwrap();
        assert_eq!(first_record, second_record);
        assert_eq!(scripted_tile_provider.recorded_requests().len(), 2);
        // The second prepare's own tail of progress reports a fully
        // disk-served download.
        let second_prepare_reports = progress_recorder.drained()[3..].to_vec();
        assert_eq!(second_prepare_reports[0].completed_tile_count, 0);
        let second_final_progress = second_prepare_reports[2];
        assert_eq!(second_final_progress.network_fetched_tile_count, 0);
        assert_eq!(second_final_progress.disk_served_tile_count, 2);
    }

    #[tokio::test]
    async fn an_interrupted_prepare_resumes_from_the_already_present_tiles() {
        let failing_tile_address = (2, 6, 4);
        let scripted_tile_provider = ScriptedTileProvider::failing_tiles(
            [(1, 3, 3)].map(|tile_address| (tile_address, vec![7; 64])),
            [failing_tile_address],
        );
        let (_response_cache, tile_pack_manager) =
            prepared_manager("resume", None, &scripted_tile_provider);
        let mut pack_description = pack_description("sacramento", sacramento_region(), 3);
        pack_description.maximum_zoom_level = 4;
        let interrupted_prepare = tile_pack_manager
            .prepare_tile_pack(&pack_description, &mut |_| {})
            .await;
        assert!(matches!(
            interrupted_prepare,
            Err(SDKError::NetworkRequestFailed(_))
        ));
        // The coarse tile is pinned and present; the failed one stays a
        // member without an entry.
        let interrupted_record = tile_pack_manager
            .tile_pack_state("sacramento")
            .unwrap()
            .unwrap();
        assert_eq!(interrupted_record.member_tile_count, 2);
        assert_eq!(interrupted_record.present_tile_count, 1);
        // The provider stops failing: the resume fetches only the missing
        // tile.
        scripted_tile_provider
            .scripted_tiles
            .lock()
            .unwrap()
            .insert(failing_tile_address, Ok(vec![7; 64]));
        let resumed_record = tile_pack_manager
            .prepare_tile_pack(&pack_description, &mut |_| {})
            .await
            .unwrap();
        assert_eq!(resumed_record.present_tile_count, 2);
        assert_eq!(
            scripted_tile_provider.recorded_requests(),
            vec![(1, 3, 3), failing_tile_address, failing_tile_address],
            "the resume touched only the absent tile"
        );
    }

    #[tokio::test]
    async fn pack_tiles_survive_lru_eviction_until_the_packs_explicit_eviction() {
        let scripted_tile_provider =
            ScriptedTileProvider::serving_tiles([((1, 3, 3), vec![7; 64])]);
        let shared_budget_bytes = Arc::new(Mutex::new(u64::MAX));
        let response_cache = open_pack_store_with_probe(
            "quota",
            CacheQuota {
                free_disk_share_percentage: Some(100.0),
                ..CacheQuota::default()
            },
            shared_budget_bytes.clone(),
        );
        let cached_raster_tile_provider = Arc::new(crate::CachedRasterTileProvider::new(
            scripted_tile_provider.clone(),
            response_cache.clone(),
            TEST_TILE_SOURCE_NAMESPACE,
        ));
        let tile_pack_manager = TilePackManager::new(
            cached_raster_tile_provider.clone(),
            response_cache.clone(),
            None,
        );
        let pack_record = tile_pack_manager
            .prepare_tile_pack(
                &pack_description("sacramento", sacramento_region(), 3),
                &mut |_| {},
            )
            .await
            .unwrap();
        let pack_payload_bytes = pack_record.present_payload_bytes;
        assert!(pack_payload_bytes > 0);
        // Budget covering exactly the pack plus one ordinary ~1 KiB entry:
        // every further ordinary write has to evict, and the pack tile —
        // though the least recently used of them all — must outlive each
        // of those evictions.
        *shared_budget_bytes.lock().unwrap() = pack_payload_bytes + 1_100;
        CachedResponseStore::<String>::store_response(
            response_cache.as_ref(),
            "ordinary:first",
            Arc::new("word".repeat(200)),
            3_600,
        )
        .unwrap();
        let held_pack_tile = CachedResponseStore::<RasterTileData>::lookup_response(
            response_cache.as_ref(),
            &super::pack_member_fingerprint(
                TEST_TILE_SOURCE_NAMESPACE,
                &super::TilePackTileAddress {
                    tile_x: 1,
                    tile_y: 3,
                    zoom_level: 3,
                },
                RasterImageFormat::Png,
                RasterTileResolution::Standard,
            ),
        )
        .unwrap();
        assert!(
            held_pack_tile.is_some(),
            "an ordinary write's eviction pressure must not remove the pinned pack tile"
        );
        // A second ordinary write under an unraised budget must evict an
        // ordinary entry (the pack tile is the least recently used, but
        // pinned) — the LRU budget governs ordinary entries only.
        CachedResponseStore::<String>::store_response(
            response_cache.as_ref(),
            "ordinary:second",
            Arc::new("word".repeat(200)),
            3_600,
        )
        .unwrap();
        let first_ordinary_entry = CachedResponseStore::<String>::lookup_response(
            response_cache.as_ref(),
            "ordinary:first",
        )
        .unwrap();
        assert!(
            first_ordinary_entry.is_none(),
            "budget pressure evicted the oldest ordinary entry"
        );
        let second_ordinary_entry = CachedResponseStore::<String>::lookup_response(
            response_cache.as_ref(),
            "ordinary:second",
        )
        .unwrap();
        assert!(second_ordinary_entry.is_some());
        // Explicit eviction frees exactly the pack's tile.
        let removal_report = tile_pack_manager.evict_tile_pack("sacramento").unwrap();
        assert_eq!(removal_report.removed_entry_count, 1);
        assert_eq!(removal_report.removed_payload_bytes, pack_payload_bytes);
        let evicted_pack_tile = CachedResponseStore::<RasterTileData>::lookup_response(
            response_cache.as_ref(),
            &super::pack_member_fingerprint(
                TEST_TILE_SOURCE_NAMESPACE,
                &super::TilePackTileAddress {
                    tile_x: 1,
                    tile_y: 3,
                    zoom_level: 3,
                },
                RasterImageFormat::Png,
                RasterTileResolution::Standard,
            ),
        )
        .unwrap();
        assert!(
            evicted_pack_tile.is_none(),
            "the explicit eviction removed the pack tile"
        );
    }

    #[tokio::test]
    async fn eviction_removes_exactly_the_region_tiles_across_overlapping_packs() {
        // Pack A covers tile (1, 3, 3) alone; pack B covers (1, 3, 3) and
        // (2, 3, 3); their shared tile leaves only when both packs evict.
        let overlapping_region =
            BoundingBox::from_corners(coordinate(38.57, -120.0), coordinate(38.58, -85.0)).unwrap();
        let scripted_tile_provider = ScriptedTileProvider::serving_tiles(
            [(1, 3, 3), (2, 3, 3)].map(|tile_address| (tile_address, vec![9; 64])),
        );
        let (response_cache, tile_pack_manager) =
            prepared_manager("overlaps", None, &scripted_tile_provider);
        CachedResponseStore::<String>::store_response(
            response_cache.as_ref(),
            "ordinary:witness",
            Arc::new("witness".to_string()),
            3_600,
        )
        .unwrap();
        tile_pack_manager
            .prepare_tile_pack(
                &pack_description("pack-a", sacramento_region(), 3),
                &mut |_| {},
            )
            .await
            .unwrap();
        let pack_b_record = tile_pack_manager
            .prepare_tile_pack(
                &pack_description("pack-b", overlapping_region, 3),
                &mut |_| {},
            )
            .await
            .unwrap();
        assert_eq!(pack_b_record.member_tile_count, 2);
        assert_eq!(
            pack_b_record.present_tile_count, 2,
            "the shared tile was already on disk and is not re-downloaded"
        );
        assert_eq!(
            scripted_tile_provider.requested_tiles.lock().unwrap().len(),
            2,
            "three tiles' worth of membership costs two downloads across the two packs"
        );
        let pack_a_removal = tile_pack_manager.evict_tile_pack("pack-a").unwrap();
        assert_eq!(pack_a_removal.removed_entry_count, 0);
        assert_eq!(
            tile_pack_manager
                .tile_pack_state("pack-b")
                .unwrap()
                .unwrap()
                .present_tile_count,
            2,
            "the shared tile survives pack A's eviction while pack B claims it"
        );
        let pack_b_removal = tile_pack_manager.evict_tile_pack("pack-b").unwrap();
        assert_eq!(pack_b_removal.removed_entry_count, 2);
        let tile_lookup = |tile_x: u32| {
            CachedResponseStore::<RasterTileData>::lookup_response(
                response_cache.as_ref(),
                &super::pack_member_fingerprint(
                    TEST_TILE_SOURCE_NAMESPACE,
                    &super::TilePackTileAddress {
                        tile_x,
                        tile_y: 3,
                        zoom_level: 3,
                    },
                    RasterImageFormat::Png,
                    RasterTileResolution::Standard,
                ),
            )
            .unwrap()
        };
        assert!(tile_lookup(1).is_none());
        assert!(tile_lookup(2).is_none());
        assert!(
            CachedResponseStore::<String>::lookup_response(
                response_cache.as_ref(),
                "ordinary:witness"
            )
            .unwrap()
            .is_some(),
            "an ordinary entry outside both packs is untouched by the evictions"
        );
    }

    #[tokio::test]
    async fn removal_of_an_unknown_pack_is_a_typed_input_error() {
        let scripted_tile_provider = ScriptedTileProvider::serving_tiles([]);
        let (_response_cache, tile_pack_manager) =
            prepared_manager("unknown-pack", None, &scripted_tile_provider);
        let unknown_removal = tile_pack_manager.evict_tile_pack("nowhere");
        assert!(matches!(
            unknown_removal,
            Err(SDKError::InvalidCoordinateInput(_))
        ));
    }

    #[tokio::test]
    async fn the_requested_retention_clamps_to_the_provider_terms_ceiling() {
        let scripted_tile_provider =
            ScriptedTileProvider::serving_tiles([((1, 3, 3), vec![7; 64])]);
        let (_response_cache, ceilinged_manager) =
            prepared_manager("clamp", Some(60), &scripted_tile_provider);
        // The round-11 ruling's shape: an above-ceiling request is accepted
        // and stores at the ceiling; it is never refused.
        let mut oversized_description = pack_description("sacramento", sacramento_region(), 3);
        oversized_description.requested_time_to_live_seconds = Some(999_999_999);
        let ceilinged_record = ceilinged_manager
            .prepare_tile_pack(&oversized_description, &mut |_| {})
            .await
            .unwrap();
        assert_eq!(ceilinged_record.effective_time_to_live_seconds, 60);
        // Without a declared ceiling the requested value applies verbatim.
        let (_response_cache, unceilinged_manager) =
            prepared_manager("no-clamp", None, &scripted_tile_provider);
        let mut requested_description = pack_description("sacramento", sacramento_region(), 3);
        requested_description.requested_time_to_live_seconds = Some(3_600);
        assert_eq!(
            unceilinged_manager.effective_time_to_live_seconds(&requested_description),
            3_600
        );
        let unrequested_description = pack_description("sacramento", sacramento_region(), 3);
        assert_eq!(
            unceilinged_manager.effective_time_to_live_seconds(&unrequested_description),
            30 * 24 * 60 * 60,
            "no requested retention takes the tiles category's default"
        );
    }

    #[tokio::test]
    async fn a_zero_requested_time_to_live_is_refused() {
        let scripted_tile_provider =
            ScriptedTileProvider::serving_tiles([((1, 3, 3), vec![7; 64])]);
        let (_response_cache, tile_pack_manager) =
            prepared_manager("zero-ttl", None, &scripted_tile_provider);
        let mut zero_description = pack_description("sacramento", sacramento_region(), 3);
        zero_description.requested_time_to_live_seconds = Some(0);
        let refused_prepare = tile_pack_manager
            .prepare_tile_pack(&zero_description, &mut |_| {})
            .await;
        assert!(matches!(
            refused_prepare,
            Err(SDKError::InvalidCoordinateInput(_))
        ));
        assert!(
            tile_pack_manager
                .tile_pack_state("sacramento")
                .unwrap()
                .is_none(),
            "a refused description defines no pack record"
        );
        assert!(scripted_tile_provider.recorded_requests().is_empty());
    }

    #[tokio::test]
    async fn a_pack_definition_survives_a_store_reopen() {
        let scripted_tile_provider =
            ScriptedTileProvider::serving_tiles([((1, 3, 3), vec![7; 64])]);
        let (response_cache, tile_pack_manager) =
            prepared_manager("reopen", None, &scripted_tile_provider);
        let prepared_record = tile_pack_manager
            .prepare_tile_pack(
                &pack_description("sacramento", sacramento_region(), 3),
                &mut |_| {},
            )
            .await
            .unwrap();
        drop(tile_pack_manager);
        drop(response_cache);
        let reopened_cache = SqliteResponseCache::with_disk_space_probe(
            SqliteResponseCacheConfiguration {
                database_path: unique_database_path("reopen"),
                ..SqliteResponseCacheConfiguration::default()
            },
            Arc::new(FixedDiskSpaceProbe::reporting(u64::MAX)),
        )
        .unwrap();
        let reopened_cached_tile_provider = Arc::new(crate::CachedRasterTileProvider::new(
            ScriptedTileProvider::serving_tiles([]),
            reopened_cache.clone(),
            TEST_TILE_SOURCE_NAMESPACE,
        ));
        let reopened_manager =
            TilePackManager::new(reopened_cached_tile_provider, reopened_cache.clone(), None);
        let reopened_record = reopened_manager
            .tile_pack_state("sacramento")
            .unwrap()
            .expect("the pack record and its membership survive the reopen");
        assert_eq!(reopened_record, prepared_record);
        // The reopened membership still holds: a re-prepare over the
        // (failing, unserved) reopened provider fetches nothing because
        // every planned tile is already present.
        reopened_manager
            .prepare_tile_pack(
                &pack_description("sacramento", sacramento_region(), 3),
                &mut |_| {},
            )
            .await
            .unwrap();
        reopened_manager.evict_tile_pack("sacramento").unwrap();
        assert!(
            reopened_manager
                .tile_pack_state("sacramento")
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn redefining_a_pack_syncs_its_membership() {
        let scripted_tile_provider = ScriptedTileProvider::serving_tiles(
            [(1, 3, 3), (2, 3, 3)].map(|tile_address| (tile_address, vec![7; 64])),
        );
        let (_response_cache, tile_pack_manager) =
            prepared_manager("redefine", None, &scripted_tile_provider);
        tile_pack_manager
            .prepare_tile_pack(
                &pack_description("shifting", sacramento_region(), 3),
                &mut |_| {},
            )
            .await
            .unwrap();
        let shifted_region =
            BoundingBox::from_corners(coordinate(38.57, -80.0), coordinate(38.58, -75.0)).unwrap();
        let shifted_record = tile_pack_manager
            .prepare_tile_pack(
                &pack_description("shifting", shifted_region, 3),
                &mut |_| {},
            )
            .await
            .unwrap();
        assert_eq!(shifted_record.member_tile_count, 1);
        assert_eq!(shifted_record.present_tile_count, 1);
        // The dropped membership unpins the region's old tile.
        let dropped_tile_lookup = CachedResponseStore::<RasterTileData>::lookup_response(
            _response_cache.as_ref(),
            &super::pack_member_fingerprint(
                TEST_TILE_SOURCE_NAMESPACE,
                &super::TilePackTileAddress {
                    tile_x: 1,
                    tile_y: 3,
                    zoom_level: 3,
                },
                RasterImageFormat::Png,
                RasterTileResolution::Standard,
            ),
        )
        .unwrap();
        assert!(
            dropped_tile_lookup.is_some(),
            "redefinition never deletes an entry"
        );
    }
}

#[cfg(test)]
mod fingerprint_outcome_tests {
    use super::super::cached_providers::CachedRasterTileProvider;
    use crate::disk_quota::{CacheQuota, FixedDiskSpaceProbe};
    use crate::sqlite_store::{SqliteResponseCache, SqliteResponseCacheConfiguration};
    use async_trait::async_trait;
    use geospatial_traits::RasterTileProvider;
    use geospatial_types::{RasterImageFormat, RasterTileData, RasterTileResolution, SDKError};
    use rustic_maps_core::{CacheThenNetworkOutcome, CachedResponseStore};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// The namespace the tile-fetch outcome tests fingerprint under.
    const TEST_TILE_SOURCE_NAMESPACE: &str = "test-provider";

    struct CountingTileProvider {
        call_count: AtomicUsize,
    }

    #[async_trait]
    impl RasterTileProvider for CountingTileProvider {
        async fn fetch_raster_tile(
            &self,
            tile_x: u32,
            tile_y: u32,
            zoom_level: u8,
            image_format: RasterImageFormat,
            tile_resolution: RasterTileResolution,
        ) -> Result<RasterTileData, SDKError> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(RasterTileData {
                tile_x,
                tile_y,
                zoom_level,
                image_format,
                tile_resolution,
                image_bytes: vec![1; 32],
            })
        }
    }

    #[tokio::test]
    async fn the_tile_fetch_outcome_distinguishes_network_from_cache() {
        let database_path = std::env::temp_dir().join(format!(
            "rustic-maps-cache-outcome-{}.sqlite",
            std::process::id()
        ));
        std::fs::remove_file(&database_path).ok();
        let response_cache: Arc<dyn CachedResponseStore<RasterTileData>> =
            SqliteResponseCache::with_disk_space_probe(
                SqliteResponseCacheConfiguration {
                    database_path,
                    quota: CacheQuota::default(),
                    ..SqliteResponseCacheConfiguration::default()
                },
                Arc::new(FixedDiskSpaceProbe::reporting(u64::MAX)),
            )
            .unwrap();
        let counting_tile_provider = Arc::new(CountingTileProvider {
            call_count: AtomicUsize::new(0),
        });
        let cached_raster_tile_provider = CachedRasterTileProvider::new(
            counting_tile_provider.clone(),
            response_cache,
            TEST_TILE_SOURCE_NAMESPACE,
        );
        let (first_tile, first_outcome) = cached_raster_tile_provider
            .fetch_raster_tile_with_cache_outcome(
                4,
                2,
                3,
                RasterImageFormat::Png,
                RasterTileResolution::Standard,
            )
            .await
            .unwrap();
        assert_eq!(first_outcome, CacheThenNetworkOutcome::NetworkFetch);
        let (_second_tile, second_outcome) = cached_raster_tile_provider
            .fetch_raster_tile_with_cache_outcome(
                4,
                2,
                3,
                RasterImageFormat::Png,
                RasterTileResolution::Standard,
            )
            .await
            .unwrap();
        assert_eq!(second_outcome, CacheThenNetworkOutcome::CacheHit);
        assert_eq!(counting_tile_provider.call_count.load(Ordering::SeqCst), 1);
        // The plain contract method keeps its signature: the outcome stays
        // internal to the pack manager's progress reporting.
        let plain_tile = cached_raster_tile_provider
            .fetch_raster_tile(
                4,
                2,
                3,
                RasterImageFormat::Png,
                RasterTileResolution::Standard,
            )
            .await
            .unwrap();
        assert_eq!(plain_tile, first_tile);
        assert_eq!(counting_tile_provider.call_count.load(Ordering::SeqCst), 1);
    }
}

#[cfg(test)]
mod pack_row_validation_tests {
    use super::{UnpackedReaderRow, validated_pack_record};
    use geospatial_types::{
        CacheOperationFailureReason, RasterImageFormat, RasterTileResolution, SDKError,
    };

    /// A row with every field valid, which each test perturbs.
    fn valid_row() -> UnpackedReaderRow {
        UnpackedReaderRow {
            display_name: "pack".to_string(),
            south_west_latitude_degrees: 38.0,
            south_west_longitude_degrees: -122.0,
            north_east_latitude_degrees: 39.0,
            north_east_longitude_degrees: -121.0,
            minimum_zoom_level: Ok(0),
            maximum_zoom_level: Ok(14),
            image_format: Ok(RasterImageFormat::Png),
            tile_resolution: Ok(RasterTileResolution::Standard),
            effective_time_to_live_seconds: Ok(60),
        }
    }

    #[test]
    fn out_of_range_zoom_and_negative_ttl_are_record_corrupted() {
        let mut out_of_range_zoom = valid_row();
        out_of_range_zoom.maximum_zoom_level = Ok(200);
        assert!(matches!(
            validated_pack_record("test-provider", "pack", out_of_range_zoom),
            Err(SDKError::CacheOperationFailure(
                CacheOperationFailureReason::RecordCorrupted
            ))
        ));

        let mut negative_ttl = valid_row();
        negative_ttl.effective_time_to_live_seconds = Err(());
        assert!(matches!(
            validated_pack_record("test-provider", "pack", negative_ttl),
            Err(SDKError::CacheOperationFailure(
                CacheOperationFailureReason::RecordCorrupted
            ))
        ));

        let mut inverted_zoom_span = valid_row();
        inverted_zoom_span.minimum_zoom_level = Ok(10);
        inverted_zoom_span.maximum_zoom_level = Ok(4);
        assert!(matches!(
            validated_pack_record("test-provider", "pack", inverted_zoom_span),
            Err(SDKError::CacheOperationFailure(
                CacheOperationFailureReason::RecordCorrupted
            ))
        ));
    }
}
