//! SQLite-backed implementation of [`rustic_maps_core::CachedResponseStore`].
//!
//! One shared cache database serves every response category; lookups,
//! entries, and eviction are keyed by request fingerprints prepared by the
//! callers. Writes run through a pool of WAL-mode connections, guarded by
//! `BEGIN IMMEDIATE` + `SAVEPOINT` so a failed upsert leaves the database
//! untouched. After every write the store evicts least-recently-used entries
//! until the total payload size fits the resolved [`CacheQuota`] budget.

pub use crate::disk_quota::{CacheQuota, DiskSpaceProbe, FixedDiskSpaceProbe, RealDiskSpaceProbe};
pub use r2d2_sqlite::rusqlite;

use crate::ResponseCategory;
use geospatial_types::{CacheOperationFailureReason, SDKError};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rustic_maps_core::CachedResponseStore;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// Current schema and migration scripts, applied newest-last.
const SCHEMA_MIGRATIONS: &[&str] = &[
    // Version 1: entry table + recency index.
    "
    CREATE TABLE entries (
        fingerprint TEXT PRIMARY KEY,
        category TEXT NOT NULL,
        payload BLOB NOT NULL,
        payload_size_bytes INTEGER NOT NULL,
        expires_at_unix_seconds INTEGER NOT NULL,
        created_at_unix_seconds INTEGER NOT NULL,
        last_accessed_at_unix_seconds INTEGER NOT NULL,
        recency_sequence INTEGER NOT NULL
    );
    CREATE INDEX entries_last_accessed_idx ON entries (recency_sequence);
    ",
    // Version 2: the tile-pack tables — one record per defined
    // pack, and a membership table pinning every planned tile fingerprint.
    // A membership row whose entry is absent is inert (the tile is still to
    // download); the LRU eviction excludes member fingerprints entirely, so
    // pack tiles are removed only by their pack's explicit eviction.
    "
    CREATE TABLE tile_packs (
        pack_id TEXT PRIMARY KEY,
        display_name TEXT NOT NULL,
        south_west_latitude_degrees REAL NOT NULL,
        south_west_longitude_degrees REAL NOT NULL,
        north_east_latitude_degrees REAL NOT NULL,
        north_east_longitude_degrees REAL NOT NULL,
        minimum_zoom_level INTEGER NOT NULL,
        maximum_zoom_level INTEGER NOT NULL,
        image_format_name TEXT NOT NULL,
        tile_resolution_name TEXT NOT NULL,
        effective_time_to_live_seconds INTEGER NOT NULL,
        created_at_unix_seconds INTEGER NOT NULL
    );
    CREATE TABLE tile_pack_members (
        pack_id TEXT NOT NULL,
        fingerprint TEXT NOT NULL,
        PRIMARY KEY (pack_id, fingerprint)
    );
    ",
    // Version 3: one namespaced cache file. Raster and vector fingerprints
    // carry a `tile_source_namespace`, and a tile pack is identified by the
    // namespace that produced its tiles beside its caller-chosen identifier.
    // Entries stored before the namespace existed can never be reached by a
    // namespaced key, and pack rows name no producer, so both are purged once
    // and the pack tables are recreated with the namespace in their key.
    "
    DELETE FROM entries WHERE category IN ('raster_tiles', 'vector_tiles');
    DROP TABLE tile_pack_members;
    DROP TABLE tile_packs;
    CREATE TABLE tile_packs (
        tile_source_namespace TEXT NOT NULL,
        pack_id TEXT NOT NULL,
        display_name TEXT NOT NULL,
        south_west_latitude_degrees REAL NOT NULL,
        south_west_longitude_degrees REAL NOT NULL,
        north_east_latitude_degrees REAL NOT NULL,
        north_east_longitude_degrees REAL NOT NULL,
        minimum_zoom_level INTEGER NOT NULL,
        maximum_zoom_level INTEGER NOT NULL,
        image_format_name TEXT NOT NULL,
        tile_resolution_name TEXT NOT NULL,
        effective_time_to_live_seconds INTEGER NOT NULL,
        created_at_unix_seconds INTEGER NOT NULL,
        PRIMARY KEY (tile_source_namespace, pack_id)
    );
    CREATE TABLE tile_pack_members (
        tile_source_namespace TEXT NOT NULL,
        pack_id TEXT NOT NULL,
        fingerprint TEXT NOT NULL,
        PRIMARY KEY (tile_source_namespace, pack_id, fingerprint)
    );
    ",
];
const SCHEMA_TABLE_BATCH: &str = "
    CREATE TABLE IF NOT EXISTS schema_migrations (
        version INTEGER PRIMARY KEY,
        applied_at_unix_seconds INTEGER NOT NULL
    );
";
const WRITE_SAVEPOINT_NAME: &str = "rustic_maps_cache_write";

/// Settings for one SQLite cache database on disk.
#[derive(Debug, Clone)]
pub struct SqliteResponseCacheConfiguration {
    /// Path of the SQLite database file; the parent directory is created.
    pub database_path: PathBuf,
    /// Size constraints resolved into a byte budget before each eviction.
    pub quota: CacheQuota,
    /// Connection-pool size; WAL writes serialize on SQLite, so a small
    /// number of readers suffices.
    pub maximum_pool_connections: u32,
}

impl Default for SqliteResponseCacheConfiguration {
    fn default() -> Self {
        Self {
            database_path: std::env::temp_dir().join("map-sdk-sdk-cache.sqlite"),
            quota: CacheQuota::default(),
            maximum_pool_connections: 4,
        }
    }
}

/// SQLite-backed response cache shared by every provider contract.
pub struct SqliteResponseCache {
    pub(crate) connection_pool: Pool<SqliteConnectionManager>,
    quota: CacheQuota,
    disk_space_probe: Arc<dyn DiskSpaceProbe>,
    database_path: PathBuf,
}

impl SqliteResponseCache {
    /// Opens (creating or migrating as needed) the cache database.
    pub fn open(
        configuration: SqliteResponseCacheConfiguration,
    ) -> Result<Arc<SqliteResponseCache>, SDKError> {
        Self::with_disk_space_probe(configuration, Arc::new(RealDiskSpaceProbe))
    }

    /// Opens the cache with an injected [`DiskSpaceProbe`] (tests).
    pub fn with_disk_space_probe(
        configuration: SqliteResponseCacheConfiguration,
        disk_space_probe: Arc<dyn DiskSpaceProbe>,
    ) -> Result<Arc<SqliteResponseCache>, SDKError> {
        configuration.quota.validate()?;
        if let Some(parent_directory) = configuration.database_path.parent() {
            std::fs::create_dir_all(parent_directory).map_err(|_| {
                SDKError::CacheOperationFailure(CacheOperationFailureReason::StoreUnavailable)
            })?;
        }
        // WAL is a database-level property: enable it once on a dedicated
        // connection before the pool spins up, so concurrent inits cannot
        // race on the pragma ("database is locked").
        {
            let setup_connection = rusqlite::Connection::open(&configuration.database_path)
                .map_err(map_execution_failure)?;
            setup_connection
                .pragma_update(None, "journal_mode", "WAL")
                .map_err(map_execution_failure)?;
        }
        let connection_manager = SqliteConnectionManager::file(&configuration.database_path)
            .with_init(|connection| {
                connection.pragma_update(None, "synchronous", "NORMAL")?;
                connection.busy_timeout(Duration::from_millis(5_000))
            });
        let connection_pool = Pool::builder()
            .max_size(configuration.maximum_pool_connections)
            .build(connection_manager)
            .map_err(|_| {
                SDKError::CacheOperationFailure(CacheOperationFailureReason::StoreUnavailable)
            })?;
        let cache = SqliteResponseCache {
            connection_pool,
            quota: configuration.quota,
            disk_space_probe,
            database_path: configuration.database_path,
        };
        cache.apply_migrations()?;
        Ok(Arc::new(cache))
    }

    /// Total payload bytes currently stored; diagnostics and eviction tests.
    pub fn total_stored_bytes(&self) -> Result<u64, SDKError> {
        let connection = self.connection_pool.get().map_err(map_pool_failure)?;
        let total_bytes: i64 = connection
            .query_row(
                "SELECT COALESCE(SUM(payload_size_bytes), 0) FROM entries",
                [],
                |row| row.get(0),
            )
            .map_err(map_execution_failure)?;
        Ok(total_bytes.max(0) as u64)
    }

    /// The stored payload size of the entry under `request_fingerprint`, or
    /// `None` while no entry carries it. One primary-key lookup: the tile-pack
    /// byte budget asks after each stored tile instead of re-summing the
    /// pack's membership on every step.
    pub(crate) fn entry_payload_size_bytes(
        &self,
        request_fingerprint: &str,
    ) -> Result<Option<u64>, SDKError> {
        let connection = self.connection_pool.get().map_err(map_pool_failure)?;
        let payload_size = connection.query_row(
            "SELECT payload_size_bytes FROM entries WHERE fingerprint = ?1",
            [request_fingerprint],
            |row| row.get::<_, i64>(0),
        );
        match payload_size {
            Ok(payload_size_bytes) => Ok(Some(payload_size_bytes.max(0) as u64)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(sqlite_error) => Err(map_execution_failure(sqlite_error)),
        }
    }

    /// Deletes the entry under `request_fingerprint`, leaving its tile-pack
    /// membership row in place. The tile-pack manager calls this for the one
    /// tile a download fetched past its byte budget, so the pack stays within
    /// the budget and a later resume retries the tile; callers only reach it
    /// for a tile this download itself stored.
    pub(crate) fn remove_entry(&self, request_fingerprint: &str) -> Result<(), SDKError> {
        let request_fingerprint = request_fingerprint.to_string();
        self.perform_write_transaction(move |connection| {
            connection
                .execute(
                    "DELETE FROM entries WHERE fingerprint = ?1",
                    [request_fingerprint],
                )
                .map_err(map_execution_failure)?;
            Ok(())
        })
    }

    /// Runs `write_operation` inside `BEGIN IMMEDIATE` + `SAVEPOINT`; on
    /// error the savepoint is rolled back and the failure surfaces, leaving
    /// any earlier writes in the transaction intact. Crate-internal so the
    /// tile-pack store methods (`tile_packs`) share the same guarded write
    /// discipline over this module's pool.
    pub(crate) fn perform_write_transaction<T>(
        &self,
        write_operation: impl FnOnce(&rusqlite::Connection) -> Result<T, SDKError>,
    ) -> Result<T, SDKError> {
        let connection = self.connection_pool.get().map_err(map_pool_failure)?;
        connection
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(map_execution_failure)?;
        connection
            .execute_batch(&format!("SAVEPOINT {WRITE_SAVEPOINT_NAME}"))
            .map_err(map_execution_failure)?;
        match write_operation(&connection) {
            Ok(value) => {
                connection
                    .execute_batch(&format!("RELEASE SAVEPOINT {WRITE_SAVEPOINT_NAME}; COMMIT"))
                    .map_err(map_execution_failure)?;
                Ok(value)
            }
            Err(operation_error) => {
                let _ = connection.execute_batch(&format!(
                    "ROLLBACK TO SAVEPOINT {WRITE_SAVEPOINT_NAME}; ROLLBACK"
                ));
                Err(operation_error)
            }
        }
    }

    fn apply_migrations(&self) -> Result<(), SDKError> {
        self.perform_write_transaction(|connection| {
            connection
                .execute_batch(SCHEMA_TABLE_BATCH)
                .map_err(map_execution_failure)?;
            for (zero_based_version, migration_batch) in SCHEMA_MIGRATIONS.iter().enumerate() {
                let version_number = (zero_based_version + 1) as i64;
                let applied_version_count: i64 = connection
                    .query_row(
                        "SELECT COUNT(*) FROM schema_migrations WHERE version = ?1",
                        [version_number],
                        |row| row.get(0),
                    )
                    .map_err(map_execution_failure)?;
                if applied_version_count > 0 {
                    continue;
                }
                connection
                    .execute_batch(migration_batch)
                    .map_err(map_execution_failure)?;
                connection
                    .execute(
                        "INSERT INTO schema_migrations (version, applied_at_unix_seconds) VALUES (?1, ?2)",
                        [version_number, unix_seconds_now()],
                    )
                    .map_err(map_execution_failure)?;
            }
            Ok(())
        })
    }

    fn store_entry(
        &self,
        request_fingerprint: &str,
        response_category: ResponseCategory,
        payload_bytes: Vec<u8>,
        time_to_live_seconds: u64,
    ) -> Result<(), SDKError> {
        let payload_size_bytes = payload_bytes.len() as u64;
        let budget_bytes = self
            .quota
            .resolve_budget_bytes(self.disk_space_probe.as_ref(), &self.database_path);
        if payload_size_bytes > budget_bytes {
            return Err(SDKError::CacheOperationFailure(
                CacheOperationFailureReason::WriteRejected,
            ));
        }
        let expires_at = unix_seconds_now()
            .saturating_add(i64::try_from(time_to_live_seconds).unwrap_or(i64::MAX));
        self.perform_write_transaction(move |connection| {
            connection
                .execute(
                    "
                    INSERT OR REPLACE INTO entries (
                        fingerprint, category, payload, payload_size_bytes,
                        expires_at_unix_seconds, created_at_unix_seconds, last_accessed_at_unix_seconds, recency_sequence
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, (SELECT COALESCE(MAX(recency_sequence), 0) + 1 FROM entries))
                    ",
                    rusqlite::params![
                        request_fingerprint,
                        response_category.storage_name(),
                        payload_bytes,
                        payload_size_bytes as i64,
                        expires_at,
                        unix_seconds_now(),
                    ],
                )
                .map_err(map_execution_failure)?;
            Ok(())
        })?;
        self.evict_to_budget(budget_bytes)
    }

    fn lookup_entry(
        &self,
        request_fingerprint: &str,
    ) -> Result<Option<(Vec<u8>, ResponseCategory)>, SDKError> {
        let connection = self.connection_pool.get().map_err(map_pool_failure)?;
        let lookup = connection.query_row(
            "
            SELECT expires_at_unix_seconds, payload, category
            FROM entries WHERE fingerprint = ?1
            ",
            [request_fingerprint],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        );
        let (expires_at, payload, category_name) = match lookup {
            Ok(row) => row,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(error) => return Err(map_execution_failure(error)),
        };
        if expires_at <= unix_seconds_now() {
            connection
                .execute(
                    "DELETE FROM entries WHERE fingerprint = ?1",
                    [request_fingerprint],
                )
                .map_err(map_execution_failure)?;
            return Ok(None);
        }
        let response_category =
            ResponseCategory::from_storage_name(&category_name).map_err(|_| {
                SDKError::CacheOperationFailure(CacheOperationFailureReason::RecordCorrupted)
            })?;
        connection
            .execute(
                "UPDATE entries SET last_accessed_at_unix_seconds = ?1, \
                 recency_sequence = (SELECT COALESCE(MAX(recency_sequence), 0) + 1 FROM entries) \
                 WHERE fingerprint = ?2",
                rusqlite::params![unix_seconds_now(), request_fingerprint],
            )
            .map_err(map_execution_failure)?;
        Ok(Some((payload, response_category)))
    }

    /// Deletes the least-recently-used entries in one pass until the total
    /// payload fits `budget_bytes`.
    ///
    /// The deficit is computed once, the least-recently-used non-pinned
    /// entries are walked once, and the prefix that covers the deficit is
    /// removed with a single `DELETE`. Entries already under budget skip the
    /// pass entirely. A single entry larger than the budget is tolerated
    /// as-is when no further candidate exists to remove.
    ///
    /// Tile-pack members are never LRU candidates: while their
    /// pack record exists, a pack's tiles are pinned against ordinary budget
    /// pressure and leave the store only through that pack's explicit
    /// eviction or the entry's time-to-live. The budget still counts pack
    /// bytes in its total, so a packed store near its budget evicts only the
    /// ordinary entries — the integrator sizes packs against the budget.
    fn evict_to_budget(&self, budget_bytes: u64) -> Result<(), SDKError> {
        if budget_bytes == u64::MAX {
            return Ok(());
        }
        self.perform_write_transaction(move |connection| {
            let total_bytes: i64 = connection
                .query_row(
                    "SELECT COALESCE(SUM(payload_size_bytes), 0) FROM entries",
                    [],
                    |row| row.get(0),
                )
                .map_err(map_execution_failure)?;
            let total_bytes = total_bytes.max(0) as u64;
            if total_bytes <= budget_bytes {
                return Ok(());
            }
            let deficit_bytes = total_bytes - budget_bytes;

            // Walk the LRU order once, stopping at the first ordinary entry
            // whose removal brings the running total past the deficit. That
            // entry's recency sequence is the prefix cutoff.
            let deletion_cutoff_recency_sequence = {
                let mut candidate_statement = connection
                    .prepare(
                        "
                        SELECT recency_sequence, payload_size_bytes FROM entries
                        WHERE fingerprint NOT IN (SELECT fingerprint FROM tile_pack_members)
                        ORDER BY recency_sequence ASC
                        ",
                    )
                    .map_err(map_execution_failure)?;
                let mut candidate_rows = candidate_statement
                    .query([])
                    .map_err(map_execution_failure)?;
                let mut deleted_payload_bytes: u64 = 0;
                let mut cutoff: Option<i64> = None;
                while let Some(candidate_row) =
                    candidate_rows.next().map_err(map_execution_failure)?
                {
                    if deleted_payload_bytes >= deficit_bytes {
                        break;
                    }
                    let recency_sequence: i64 =
                        candidate_row.get(0).map_err(map_execution_failure)?;
                    let payload_size_bytes: i64 =
                        candidate_row.get(1).map_err(map_execution_failure)?;
                    deleted_payload_bytes =
                        deleted_payload_bytes.saturating_add(payload_size_bytes.max(0) as u64);
                    cutoff = Some(recency_sequence);
                }
                cutoff
            };
            let Some(deletion_cutoff_recency_sequence) = deletion_cutoff_recency_sequence else {
                // Only pack-pinned entries remain: they are tolerated by
                // design until their pack's explicit eviction or expiry.
                return Ok(());
            };
            connection
                .execute(
                    "
                    DELETE FROM entries
                    WHERE fingerprint NOT IN (SELECT fingerprint FROM tile_pack_members)
                      AND recency_sequence <= ?1
                    ",
                    [deletion_cutoff_recency_sequence],
                )
                .map_err(map_execution_failure)?;
            Ok(())
        })
    }
}

/// Any deserializable typed record can travel through the single cache.
impl<D> CachedResponseStore<D> for SqliteResponseCache
where
    D: serde::Serialize + serde::de::DeserializeOwned + Send + Sync,
{
    fn lookup_response(&self, request_fingerprint: &str) -> Result<Option<Arc<D>>, SDKError> {
        match self.lookup_entry(request_fingerprint)? {
            None => Ok(None),
            Some((payload, _response_category)) => {
                let response_value: D = serde_json::from_slice(&payload).map_err(|_| {
                    SDKError::CacheOperationFailure(CacheOperationFailureReason::RecordCorrupted)
                })?;
                Ok(Some(Arc::new(response_value)))
            }
        }
    }

    fn store_response(
        &self,
        request_fingerprint: &str,
        response_value: Arc<D>,
        time_to_live_seconds: u64,
    ) -> Result<(), SDKError> {
        let payload_bytes = serde_json::to_vec(response_value.as_ref()).map_err(|_| {
            SDKError::CacheOperationFailure(CacheOperationFailureReason::WriteRejected)
        })?;
        // Fingerprints prepared by `RequestDescriptor` name their category
        // as a `"<category>:<hash>"` prefix; unknown or bare fingerprints
        // fall back to the geocoding default (TTL stays caller-provided).
        let response_category = response_category_of_fingerprint(request_fingerprint);
        self.store_entry(
            request_fingerprint,
            response_category,
            payload_bytes,
            time_to_live_seconds,
        )
    }
}

fn response_category_of_fingerprint(request_fingerprint: &str) -> ResponseCategory {
    request_fingerprint
        .split_once(':')
        .and_then(|(prefix, _)| ResponseCategory::from_storage_name(prefix).ok())
        .unwrap_or(ResponseCategory::Geocoding)
}

pub(crate) fn unix_seconds_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_secs()).ok())
        .unwrap_or(0)
}

pub(crate) fn map_pool_failure(_pool_error: r2d2::Error) -> SDKError {
    SDKError::CacheOperationFailure(CacheOperationFailureReason::StoreUnavailable)
}

pub(crate) fn map_execution_failure(_sqlite_error: rusqlite::Error) -> SDKError {
    SDKError::CacheOperationFailure(CacheOperationFailureReason::StoreUnavailable)
}

impl std::fmt::Debug for SqliteResponseCache {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SqliteResponseCache")
            .field("database_path", &self.database_path)
            .field("quota", &self.quota)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CacheOperationFailureReason, SDKError, SqliteResponseCache,
        SqliteResponseCacheConfiguration,
    };
    use crate::disk_quota::{CacheQuota, DiskSpaceProbe, FixedDiskSpaceProbe};
    use crate::tile_packs::TilePackDescription;
    use geospatial_types::{
        BoundingBox, GeographicCoordinate, RasterImageFormat, RasterTileResolution,
    };
    use rustic_maps_core::CachedResponseStore;
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    /// Disk-space probe whose reported free space a test can change between
    /// writes, so the resolved budget can shrink below what is already
    /// stored and trigger eviction.
    struct MutableDiskSpaceProbe {
        available_bytes: Mutex<u64>,
    }

    impl MutableDiskSpaceProbe {
        fn reporting(available_bytes: u64) -> Arc<Self> {
            Arc::new(Self {
                available_bytes: Mutex::new(available_bytes),
            })
        }

        fn set_available_bytes(&self, available_bytes: u64) {
            *self.available_bytes.lock().unwrap() = available_bytes;
        }
    }

    impl DiskSpaceProbe for MutableDiskSpaceProbe {
        fn available_disk_bytes(&self, _cache_file_path: &Path) -> Result<u64, SDKError> {
            Ok(*self.available_bytes.lock().unwrap())
        }
    }

    fn unique_database_path(test_name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "rustic-maps-cache-unit-{}-{test_name}.sqlite",
            std::process::id(),
        ))
    }

    fn open_cache(test_name: &str, quota: CacheQuota) -> Arc<SqliteResponseCache> {
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
            Arc::new(FixedDiskSpaceProbe::reporting(u64::MAX)),
        )
        .unwrap()
    }

    fn repeated_text_entry(base_text: &str, target_bytes: usize) -> String {
        base_text.repeat(target_bytes.div_ceil(base_text.len()))
    }

    /// A string whose serialized JSON payload is exactly `payload_bytes`
    /// bytes: serde_json frames a string with two quote characters.
    fn exact_payload_entry(payload_bytes: usize) -> String {
        "a".repeat(payload_bytes - 2)
    }

    /// Opens a cache whose resolved budget is the mutable probe's reported
    /// free space (a 100% share of it), so a test can shrink it between
    /// writes.
    fn open_cache_with_mutable_budget(
        test_name: &str,
    ) -> (Arc<SqliteResponseCache>, Arc<MutableDiskSpaceProbe>) {
        let database_path = unique_database_path(test_name);
        std::fs::remove_file(database_path.with_extension("sqlite-wal")).ok();
        std::fs::remove_file(database_path.with_extension("sqlite-shm")).ok();
        std::fs::remove_file(&database_path).ok();
        let disk_space_probe = MutableDiskSpaceProbe::reporting(u64::MAX);
        let cache = SqliteResponseCache::with_disk_space_probe(
            SqliteResponseCacheConfiguration {
                database_path,
                quota: CacheQuota {
                    free_disk_share_percentage: Some(100.0),
                    ..CacheQuota::default()
                },
                ..SqliteResponseCacheConfiguration::default()
            },
            disk_space_probe.clone(),
        )
        .unwrap();
        (cache, disk_space_probe)
    }

    fn store_exact_payload(cache: &SqliteResponseCache, fingerprint: &str, payload_bytes: usize) {
        CachedResponseStore::<String>::store_response(
            cache,
            fingerprint,
            Arc::new(exact_payload_entry(payload_bytes)),
            3_600,
        )
        .unwrap();
    }

    fn lookup_present(cache: &SqliteResponseCache, fingerprint: &str) -> bool {
        CachedResponseStore::<String>::lookup_response(cache, fingerprint)
            .unwrap()
            .is_some()
    }

    #[test]
    fn a_pre_namespace_store_is_purged_once_on_open() {
        // The database a store written before namespaced tile keys opens:
        // schema versions 1 and 2 only, with a raster entry and a pack row
        // that no namespaced key can reach. Opening runs migration 3, which
        // purges both and recreates the pack tables with the namespace in
        // their key.
        let database_path = unique_database_path("pre-namespace");
        std::fs::remove_file(database_path.with_extension("sqlite-wal")).ok();
        std::fs::remove_file(database_path.with_extension("sqlite-shm")).ok();
        std::fs::remove_file(&database_path).ok();
        {
            let legacy_connection = super::rusqlite::Connection::open(&database_path).unwrap();
            legacy_connection
                .execute_batch(
                    "
                    CREATE TABLE schema_migrations (
                        version INTEGER PRIMARY KEY,
                        applied_at_unix_seconds INTEGER NOT NULL
                    );
                    INSERT INTO schema_migrations VALUES (1, 0), (2, 0);
                    CREATE TABLE entries (
                        fingerprint TEXT PRIMARY KEY,
                        category TEXT NOT NULL,
                        payload BLOB NOT NULL,
                        payload_size_bytes INTEGER NOT NULL,
                        expires_at_unix_seconds INTEGER NOT NULL,
                        created_at_unix_seconds INTEGER NOT NULL,
                        last_accessed_at_unix_seconds INTEGER NOT NULL,
                        recency_sequence INTEGER NOT NULL
                    );
                    CREATE INDEX entries_last_accessed_idx ON entries (recency_sequence);
                    CREATE TABLE tile_packs (
                        pack_id TEXT PRIMARY KEY,
                        display_name TEXT NOT NULL,
                        south_west_latitude_degrees REAL NOT NULL,
                        south_west_longitude_degrees REAL NOT NULL,
                        north_east_latitude_degrees REAL NOT NULL,
                        north_east_longitude_degrees REAL NOT NULL,
                        minimum_zoom_level INTEGER NOT NULL,
                        maximum_zoom_level INTEGER NOT NULL,
                        image_format_name TEXT NOT NULL,
                        tile_resolution_name TEXT NOT NULL,
                        effective_time_to_live_seconds INTEGER NOT NULL,
                        created_at_unix_seconds INTEGER NOT NULL
                    );
                    CREATE TABLE tile_pack_members (
                        pack_id TEXT NOT NULL,
                        fingerprint TEXT NOT NULL,
                        PRIMARY KEY (pack_id, fingerprint)
                    );
                    INSERT INTO entries VALUES (
                        'raster_tiles:legacy', 'raster_tiles', X'00', 1, 9999999999, 0, 0, 0
                    );
                    INSERT INTO entries VALUES (
                        'vector_tiles:legacy', 'vector_tiles', X'00', 1, 9999999999, 0, 0, 0
                    );
                    INSERT INTO entries VALUES (
                        'geocoding:kept', 'geocoding', X'226b65707422', 1, 9999999999, 0, 0, 0
                    );
                    INSERT INTO tile_packs VALUES (
                        'legacy-pack', 'Legacy pack', 0.0, 0.0, 1.0, 1.0, 3, 3,
                        'png', 'standard', 3600, 0
                    );
                    INSERT INTO tile_pack_members VALUES ('legacy-pack', 'raster_tiles:legacy');
                    ",
                )
                .unwrap();
        }
        let cache = SqliteResponseCache::with_disk_space_probe(
            SqliteResponseCacheConfiguration {
                database_path,
                ..SqliteResponseCacheConfiguration::default()
            },
            Arc::new(FixedDiskSpaceProbe::reporting(u64::MAX)),
        )
        .unwrap();
        // The tile entries and the pack are gone; the unrelated geocoding
        // entry survives, because only tile categories changed their keys.
        assert!(
            CachedResponseStore::<String>::lookup_response(cache.as_ref(), "raster_tiles:legacy")
                .unwrap()
                .is_none()
        );
        assert!(
            CachedResponseStore::<String>::lookup_response(cache.as_ref(), "vector_tiles:legacy")
                .unwrap()
                .is_none()
        );
        assert!(
            CachedResponseStore::<String>::lookup_response(cache.as_ref(), "geocoding:kept")
                .unwrap()
                .is_some()
        );
        assert_eq!(cache.tile_pack_records("test-provider").unwrap(), vec![]);
        // A fresh namespaced tile stores and reads through the same file.
        let fresh_fingerprint =
            rustic_maps_core::vector_tile_payload_fingerprint("test-provider", 3, 1, 2);
        CachedResponseStore::<String>::store_response(
            cache.as_ref(),
            &fresh_fingerprint,
            Arc::new("fresh tile".to_string()),
            3_600,
        )
        .unwrap();
        assert!(
            CachedResponseStore::<String>::lookup_response(cache.as_ref(), &fresh_fingerprint)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn two_namespaces_hold_two_packs_with_one_identifier() {
        let cache = open_cache("namespaced-packs", CacheQuota::default());
        let region = BoundingBox::from_corners(
            GeographicCoordinate::from_degrees(38.57, -121.5).unwrap(),
            GeographicCoordinate::from_degrees(38.58, -121.49).unwrap(),
        )
        .unwrap();
        let define = |tile_source_namespace: &str, member_fingerprint: &str| {
            cache
                .define_tile_pack(
                    tile_source_namespace,
                    &TilePackDescription {
                        pack_id: "shared-identifier".to_string(),
                        display_name: format!("{tile_source_namespace} pack"),
                        region_bounding_box: region,
                        minimum_zoom_level: 3,
                        maximum_zoom_level: 3,
                        image_format: RasterImageFormat::Png,
                        tile_resolution: RasterTileResolution::Standard,
                        requested_time_to_live_seconds: None,
                    },
                    &[member_fingerprint.to_string()],
                    3_600,
                )
                .unwrap();
        };
        define("alpha-provider", "alpha-fingerprint");
        define("beta-provider", "beta-fingerprint");
        let alpha_record = cache
            .tile_pack_record("alpha-provider", "shared-identifier")
            .unwrap()
            .unwrap();
        let beta_record = cache
            .tile_pack_record("beta-provider", "shared-identifier")
            .unwrap()
            .unwrap();
        assert_eq!(alpha_record.tile_source_namespace, "alpha-provider");
        assert_eq!(alpha_record.display_name, "alpha-provider pack");
        assert_eq!(alpha_record.member_tile_count, 1);
        assert_eq!(beta_record.tile_source_namespace, "beta-provider");
        assert_eq!(beta_record.display_name, "beta-provider pack");
        assert_eq!(beta_record.member_tile_count, 1);
        // Removing one provider's pack leaves the other's intact.
        cache
            .remove_tile_pack("alpha-provider", "shared-identifier")
            .unwrap();
        assert!(
            cache
                .tile_pack_record("alpha-provider", "shared-identifier")
                .unwrap()
                .is_none()
        );
        assert_eq!(
            cache
                .tile_pack_record("beta-provider", "shared-identifier")
                .unwrap()
                .unwrap()
                .member_tile_count,
            1
        );
    }

    #[test]
    fn store_and_lookup_round_trip() {
        let cache = open_cache("roundtrip", CacheQuota::default());
        CachedResponseStore::<String>::store_response(
            cache.as_ref(),
            "fp-roundtrip",
            Arc::new("value".repeat(40)),
            3_600,
        )
        .unwrap();
        let looked_up =
            CachedResponseStore::<String>::lookup_response(cache.as_ref(), "fp-roundtrip")
                .unwrap()
                .unwrap();
        assert_eq!(looked_up.as_str(), "value".repeat(40));
        assert_eq!(
            CachedResponseStore::<String>::lookup_response(cache.as_ref(), "fp-missing").unwrap(),
            None
        );
    }

    #[test]
    fn zero_ttl_entries_expire_immediately() {
        let cache = open_cache("ttl-zero", CacheQuota::default());
        CachedResponseStore::<String>::store_response(
            cache.as_ref(),
            "fp-expired",
            Arc::new("stale".to_string()),
            0,
        )
        .unwrap();
        assert_eq!(
            CachedResponseStore::<String>::lookup_response(cache.as_ref(), "fp-expired").unwrap(),
            None
        );
    }

    #[test]
    fn least_recently_used_entry_is_evicted_first() {
        // Three ~1 KB entries against a 2.5 KB budget: only two survive.
        let cache = open_cache(
            "lru",
            CacheQuota {
                absolute_maximum_bytes: Some(2_500),
                ..CacheQuota::default()
            },
        );
        let store = |fingerprint: &str, value: String| {
            CachedResponseStore::<String>::store_response(
                cache.as_ref(),
                fingerprint,
                Arc::new(value),
                3_600,
            )
            .unwrap();
        };
        store("fp-a", repeated_text_entry("alpha", 1_000));
        store("fp-b", repeated_text_entry("beta", 1_000));
        // Touch A so B is the least recently used when C arrives.
        let touched = CachedResponseStore::<String>::lookup_response(cache.as_ref(), "fp-a")
            .unwrap()
            .unwrap();
        let _ = touched;
        store("fp-c", repeated_text_entry("gamma", 1_000));
        assert!(
            CachedResponseStore::<String>::lookup_response(cache.as_ref(), "fp-b")
                .unwrap()
                .is_none()
        );
        assert!(
            CachedResponseStore::<String>::lookup_response(cache.as_ref(), "fp-a")
                .unwrap()
                .is_some()
        );
        assert!(
            CachedResponseStore::<String>::lookup_response(cache.as_ref(), "fp-c")
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn payloads_larger_than_the_budget_are_rejected_outright() {
        let cache = open_cache(
            "oversized",
            CacheQuota {
                absolute_maximum_bytes: Some(2_500),
                ..CacheQuota::default()
            },
        );
        let result = CachedResponseStore::<String>::store_response(
            cache.as_ref(),
            "fp-huge",
            Arc::new(repeated_text_entry("huge", 10_000)),
            3_600,
        );
        assert!(matches!(
            result,
            Err(SDKError::CacheOperationFailure(
                CacheOperationFailureReason::WriteRejected
            ))
        ));
        assert!(cache.database_path.exists());
    }

    #[test]
    fn reopening_reuses_the_existing_schema() {
        let database_path = unique_database_path("reopen");
        std::fs::remove_file(&database_path).ok();
        let configuration = SqliteResponseCacheConfiguration {
            database_path: database_path.clone(),
            ..SqliteResponseCacheConfiguration::default()
        };
        let first_cache = SqliteResponseCache::open(configuration.clone()).unwrap();
        CachedResponseStore::<String>::store_response(
            first_cache.as_ref(),
            "fp-persisted",
            Arc::new("kept".to_string()),
            3_600,
        )
        .unwrap();
        drop(first_cache);
        let second_cache = SqliteResponseCache::open(configuration).unwrap();
        let carried =
            CachedResponseStore::<String>::lookup_response(second_cache.as_ref(), "fp-persisted")
                .unwrap()
                .unwrap();
        assert_eq!(carried.as_str(), "kept");
        std::fs::remove_file(&database_path).ok();
    }

    #[test]
    fn eviction_deletes_exactly_the_oldest_prefix_in_one_pass() {
        let (cache, disk_space_probe) = open_cache_with_mutable_budget("evict-prefix");
        disk_space_probe.set_available_bytes(10_000);
        for fingerprint in ["fp-a", "fp-b", "fp-c", "fp-d", "fp-e"] {
            store_exact_payload(cache.as_ref(), fingerprint, 1_000);
        }
        assert_eq!(cache.total_stored_bytes().unwrap(), 5_000);
        // Shrinking the budget below what is stored makes the next write
        // evict exactly the three oldest entries: 6,000 stored against a
        // 3,000 budget is a 3,000 deficit, which three 1,000-byte entries
        // cover precisely.
        disk_space_probe.set_available_bytes(3_000);
        store_exact_payload(cache.as_ref(), "fp-f", 1_000);
        assert_eq!(cache.total_stored_bytes().unwrap(), 3_000);
        for evicted_fingerprint in ["fp-a", "fp-b", "fp-c"] {
            assert!(!lookup_present(cache.as_ref(), evicted_fingerprint));
        }
        for kept_fingerprint in ["fp-d", "fp-e", "fp-f"] {
            assert!(lookup_present(cache.as_ref(), kept_fingerprint));
        }
    }

    #[test]
    fn eviction_follows_recency_not_fingerprint_order() {
        let (cache, disk_space_probe) = open_cache_with_mutable_budget("evict-order");
        disk_space_probe.set_available_bytes(10_000);
        for fingerprint in ["fp-a", "fp-b", "fp-c", "fp-d", "fp-e"] {
            store_exact_payload(cache.as_ref(), fingerprint, 1_000);
        }
        // Touch A and C: the recency order becomes B, D, E, A, C, so the
        // evicted prefix is the lexicographically non-first B and D.
        assert!(lookup_present(cache.as_ref(), "fp-a"));
        assert!(lookup_present(cache.as_ref(), "fp-c"));
        disk_space_probe.set_available_bytes(3_000);
        store_exact_payload(cache.as_ref(), "fp-f", 1_000);
        for evicted_fingerprint in ["fp-b", "fp-d", "fp-e"] {
            assert!(!lookup_present(cache.as_ref(), evicted_fingerprint));
        }
        for kept_fingerprint in ["fp-a", "fp-c", "fp-f"] {
            assert!(lookup_present(cache.as_ref(), kept_fingerprint));
        }
    }

    #[test]
    fn a_store_already_under_budget_deletes_nothing() {
        let (cache, disk_space_probe) = open_cache_with_mutable_budget("evict-under-budget");
        disk_space_probe.set_available_bytes(10_000);
        store_exact_payload(cache.as_ref(), "fp-a", 1_000);
        store_exact_payload(cache.as_ref(), "fp-b", 1_000);
        assert_eq!(cache.total_stored_bytes().unwrap(), 2_000);
        assert!(lookup_present(cache.as_ref(), "fp-a"));
        assert!(lookup_present(cache.as_ref(), "fp-b"));
    }

    #[test]
    fn pack_pinned_entries_survive_ordinary_eviction() {
        let (cache, disk_space_probe) = open_cache_with_mutable_budget("evict-pinned");
        disk_space_probe.set_available_bytes(10_000);
        let region = BoundingBox::from_corners(
            GeographicCoordinate::from_degrees(38.57, -121.5).unwrap(),
            GeographicCoordinate::from_degrees(38.58, -121.49).unwrap(),
        )
        .unwrap();
        cache
            .define_tile_pack(
                "test-provider",
                &TilePackDescription {
                    pack_id: "pinned-pack".to_string(),
                    display_name: "Pinned pack".to_string(),
                    region_bounding_box: region,
                    minimum_zoom_level: 3,
                    maximum_zoom_level: 3,
                    image_format: RasterImageFormat::Png,
                    tile_resolution: RasterTileResolution::Standard,
                    requested_time_to_live_seconds: None,
                },
                &["fp-pinned".to_string()],
                3_600,
            )
            .unwrap();
        // The pinned entry is the oldest by recency, so ordinary LRU order
        // would evict it first; the membership row must keep it.
        store_exact_payload(cache.as_ref(), "fp-pinned", 1_000);
        for fingerprint in ["fp-b", "fp-c", "fp-d"] {
            store_exact_payload(cache.as_ref(), fingerprint, 1_000);
        }
        // Total 4,000 against a 2,000 budget, plus a new 1,000-byte write:
        // the ordinary entries B, C, D cover the deficit and leave the
        // pinned entry and the new write.
        disk_space_probe.set_available_bytes(2_000);
        store_exact_payload(cache.as_ref(), "fp-e", 1_000);
        assert!(lookup_present(cache.as_ref(), "fp-pinned"));
        assert!(lookup_present(cache.as_ref(), "fp-e"));
        for evicted_fingerprint in ["fp-b", "fp-c", "fp-d"] {
            assert!(!lookup_present(cache.as_ref(), evicted_fingerprint));
        }
    }
}
