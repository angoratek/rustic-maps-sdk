//! The engine facades: typed capability objects constructed over an existing
//! [`MapSdk`].
//!
//! Each engine holds the very same [`MapSdk`] the application already owns —
//! one provider registry, one tile-cache installation, one diagnostics ring
//! set — and delegates every member to the matching `MapSdk` method
//! verbatim. The engines add no validation, no mapping and no error
//! rewriting of their own: a typed failure is the SDK's typed failure with
//! the same variant and the same detail message, so an integrator moving
//! from `map_sdk.search(...)` to `search_engine.search(...)` observes no
//! behavioural change. Their point is vocabulary: the same engine names
//! reach Kotlin, Swift, React Native and the wasm mirror through one UDL
//! contract, and an application holds one `MapSdk` plus one engine per
//! capability instead of a single object with every call on it.
//!
//! The async members await the SDK's runtime-independent futures directly,
//! so the exported futures stay `Send` (the provider reactor work already
//! hands back through [`crate::runtime::drive`]).

use std::sync::Arc;

use crate::diagnostics::SdkDiagnosticEventListener;
use crate::error::SdkError;
use crate::provider::MapSdk;
use crate::records::{
    BoundingBox, DistanceMatrix, GeographicCoordinate, Place, RegionDefinition,
    RegionDownloadProgress, RegionRecord, RegionRemovalReport, Route, RouteCapabilities,
    RouteOptions, SdkLogSeverity, TileCacheTerms, TransportationMode,
};
// The download progress phases are produced by the native download driver only:
// the browser build has no region downloads and refuses every pack member.
#[cfg(not(target_arch = "wasm32"))]
use crate::records::RegionDownloadPhase;

/// Offline region download progress listener of the exported contract
/// (`rustic_maps.udl`'s `RegionDownloadProgressListener`): the SDK delivers
/// one `Downloading` report with zero completions before the first tile, one
/// `Downloading` report per completed tile, and exactly one terminal report.
///
/// Callbacks run on an SDK worker thread, never on the caller's thread. An
/// implementation must not block and must not call back into the same
/// `OfflineManager`; a platform surface marshals to its UI thread.
pub trait RegionDownloadProgressListener: Send + Sync {
    /// Receives one progress report of a region download.
    fn on_region_download_progress(&self, progress: RegionDownloadProgress);

    /// Internally required by the uniffi foreign-handle machinery (the
    /// foreign-handle probe of the uniffi 0.31 trait-object lowering);
    /// Rust-side implementations keep the default, which reports a
    /// Rust-implemented object.
    #[doc(hidden)]
    fn uniffi_foreign_handle(&self) -> Option<::uniffi::Handle> {
        Option::None
    }
}

/// Search and geocoding facade over a [`MapSdk`]'s registered provider
/// contracts.
pub struct SearchEngine {
    /// The SDK whose provider registry serves this engine's calls.
    map_sdk: Arc<MapSdk>,
}

impl SearchEngine {
    /// Builds the engine over the SDK whose provider registry serves its
    /// calls.
    pub fn new(map_sdk: Arc<MapSdk>) -> Self {
        Self { map_sdk }
    }

    /// Returns full search results for a settled query; the boundary
    /// restricts the search when given. Delegates to [`MapSdk::search`].
    pub async fn search(
        &self,
        query_text: String,
        search_boundary: Option<BoundingBox>,
    ) -> Result<Vec<Place>, SdkError> {
        self.map_sdk.search(query_text, search_boundary).await
    }

    /// Returns fast completion candidates while the user types; the
    /// boundary restricts the search when given. Delegates to
    /// [`MapSdk::suggest`].
    pub async fn suggest(
        &self,
        query_text: String,
        search_boundary: Option<BoundingBox>,
    ) -> Result<Vec<Place>, SdkError> {
        self.map_sdk.suggest(query_text, search_boundary).await
    }

    /// Resolves a coordinate into candidate places describing that
    /// position. Delegates to [`MapSdk::reverse_geocode`].
    pub async fn reverse_geocode(
        &self,
        coordinate: GeographicCoordinate,
    ) -> Result<Vec<Place>, SdkError> {
        self.map_sdk.reverse_geocode(coordinate).await
    }
}

/// Routing facade over a [`MapSdk`]'s registered provider contracts.
pub struct RoutingEngine {
    /// The SDK whose provider registry serves this engine's calls.
    map_sdk: Arc<MapSdk>,
}

impl RoutingEngine {
    /// Builds the engine over the SDK whose provider registry serves its
    /// calls.
    pub fn new(map_sdk: Arc<MapSdk>) -> Self {
        Self { map_sdk }
    }

    /// Calculates a route through the ordered waypoints with the given
    /// options. Delegates to [`MapSdk::calculate_route`].
    pub async fn calculate_route(
        &self,
        waypoints: Vec<GeographicCoordinate>,
        route_options: RouteOptions,
    ) -> Result<Route, SdkError> {
        self.map_sdk.calculate_route(waypoints, route_options).await
    }

    /// Reports which transport modes and which strategy the serving routing
    /// provider can honour. Delegates to [`MapSdk::route_capabilities`].
    pub fn route_capabilities(&self) -> Result<RouteCapabilities, SdkError> {
        self.map_sdk.route_capabilities()
    }

    /// Calculates durations and distances for every origin-destination
    /// pair. Delegates to [`MapSdk::calculate_distance_matrix`].
    pub async fn calculate_distance_matrix(
        &self,
        origins: Vec<GeographicCoordinate>,
        destinations: Vec<GeographicCoordinate>,
        transportation_mode: TransportationMode,
    ) -> Result<DistanceMatrix, SdkError> {
        self.map_sdk
            .calculate_distance_matrix(origins, destinations, transportation_mode)
            .await
    }
}

/// Offline tile-cache settings facade over a [`MapSdk`]'s installed tile
/// cache.
pub struct OfflineManager {
    /// The SDK whose installed tile cache this manager reads and tunes.
    map_sdk: Arc<MapSdk>,
}

impl OfflineManager {
    /// Builds the manager over the SDK whose installed tile cache it reads
    /// and tunes.
    pub fn new(map_sdk: Arc<MapSdk>) -> Self {
        Self { map_sdk }
    }

    /// Reads the effective raster-tile caching terms of the installed
    /// cache. Delegates to [`MapSdk::raster_tile_cache_terms`].
    pub fn tile_cache_terms(&self) -> Result<TileCacheTerms, SdkError> {
        self.map_sdk.raster_tile_cache_terms()
    }

    /// Overrides the installed raster-tile cache's time-to-live in seconds.
    /// Delegates to [`MapSdk::set_raster_tile_cache_time_to_live`].
    pub fn set_tile_time_to_live(&self, time_to_live_seconds: u64) -> Result<(), SdkError> {
        self.map_sdk
            .set_raster_tile_cache_time_to_live(time_to_live_seconds)
    }

    /// Reserves a payload-byte budget for tile packs beside the general LRU
    /// cache. Delegates to [`MapSdk::set_tile_pack_byte_budget`].
    pub fn set_tile_pack_byte_budget(&self, byte_budget: u64) -> Result<(), SdkError> {
        self.map_sdk.set_tile_pack_byte_budget(byte_budget)
    }

    /// Reads the effective payload-byte budget reserved for tile packs.
    /// Delegates to [`MapSdk::tile_pack_byte_budget`].
    pub fn tile_pack_byte_budget(&self) -> Result<u64, SdkError> {
        self.map_sdk.tile_pack_byte_budget()
    }

    /// Answers how many tiles the region enumerates, so a settings surface can
    /// warn about a large download before starting it. Nothing is written.
    ///
    /// # Errors
    /// Throws `ProviderNotConfigured` naming `TileCache` when no tile cache is
    /// installed, and `InvalidCoordinateInput` for an invalid definition or a
    /// region above the provider's declared maximum zoom level.
    #[cfg_attr(target_arch = "wasm32", allow(unused_variables))]
    pub fn region_tile_count(&self, region: RegionDefinition) -> Result<u64, SdkError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let region_bounding_box = region_bounding_box(&region)?;
            if region.time_to_live_seconds == Some(0) {
                return Err(invalid_zero_region_retention_error());
            }
            let tile_count = rustic_maps_cache::enumerate_region_tiles(
                &region_bounding_box,
                region.minimum_zoom_level,
                region.maximum_zoom_level,
            )?
            .len() as u64;
            refuse_region_above_provider_zoom_ceiling(
                region.maximum_zoom_level,
                self.map_sdk.tile_pack_maximum_zoom_level()?,
            )?;
            Ok(tile_count)
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = region_bounding_box(&region)?;
            Err(crate::provider::tile_cache_not_configured_error())
        }
    }

    /// Downloads the region's tiles into the installed cache and answers the
    /// region's record as it stands on disk afterwards. Progress goes to the
    /// listener as described by [`RegionDownloadProgressListener`].
    ///
    /// Cancellation through [`Self::cancel_region_download`] is a successful
    /// partial result: the returned record carries fewer present tiles than
    /// members, the terminal report is `Cancelled`, and the next call resumes.
    ///
    /// # Errors
    /// Throws `ProviderNotConfigured` naming `TileCache` when no tile cache is
    /// installed; `InvalidCoordinateInput` for an invalid definition or a
    /// region above the provider's declared maximum zoom level; the provider's
    /// typed failure when a tile cannot be fetched; and `CacheOperationFailure`
    /// when the region is already downloading or the store refuses a write.
    #[cfg_attr(target_arch = "wasm32", allow(unused_variables))]
    pub async fn download_region(
        &self,
        region: RegionDefinition,
        progress_listener: Arc<dyn RegionDownloadProgressListener>,
    ) -> Result<RegionRecord, SdkError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let tile_pack_description = tile_pack_description_from_region(&region)?;
            let map_sdk = Arc::clone(&self.map_sdk);
            let tile_pack_manager = map_sdk.tile_pack_manager()?;
            let (cancellation_receiver, region_download_cancellation) =
                map_sdk.begin_region_download(&tile_pack_description.pack_id)?;
            let region_download_registration = RegionDownloadRegistration {
                map_sdk: &map_sdk,
                region_id: tile_pack_description.pack_id.clone(),
                region_download_cancellation,
            };
            let region_record = crate::runtime::drive(run_region_download(
                tile_pack_manager,
                tile_pack_description,
                cancellation_receiver,
                progress_listener,
            ))
            .await;
            drop(region_download_registration);
            region_record
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = region_bounding_box(&region)?;
            Err(crate::provider::tile_cache_not_configured_error())
        }
    }

    /// Cancels the region's in-flight download if one exists. Cancellation is
    /// idempotent and a successful partial result; a no-op for an unknown,
    /// finished or already cancelled region.
    pub fn cancel_region_download(&self, region_id: String) {
        #[cfg(not(target_arch = "wasm32"))]
        self.map_sdk.cancel_region_download(&region_id);
        #[cfg(target_arch = "wasm32")]
        let _ = region_id;
    }

    /// Lists every region stored on disk, in the store's listing order.
    ///
    /// # Errors
    /// Throws `ProviderNotConfigured` naming `TileCache` when no tile cache is
    /// installed and `CacheOperationFailure` when a stored record is
    /// corrupted.
    pub async fn region_records(&self) -> Result<Vec<RegionRecord>, SdkError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let tile_pack_manager = self.map_sdk.tile_pack_manager()?;
            let blocking_work_executor = crate::runtime::blocking_work_executor();
            let tile_pack_records = crate::runtime::drive(async move {
                rustic_maps_core::blocking_work::run_blocking(
                    blocking_work_executor.as_ref(),
                    move || tile_pack_manager.tile_pack_states(),
                )
                .await
            })
            .await?;
            Ok(tile_pack_records
                .iter()
                .map(region_record_from_tile_pack_record)
                .collect())
        }
        #[cfg(target_arch = "wasm32")]
        {
            Err(crate::provider::tile_cache_not_configured_error())
        }
    }

    /// Reads one stored region's record, or `None` when no region is stored
    /// under the identifier.
    ///
    /// # Errors
    /// Throws `ProviderNotConfigured` naming `TileCache` when no tile cache is
    /// installed and `CacheOperationFailure` when the stored record is
    /// corrupted.
    pub async fn region_record(&self, region_id: String) -> Result<Option<RegionRecord>, SdkError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let tile_pack_manager = self.map_sdk.tile_pack_manager()?;
            let blocking_work_executor = crate::runtime::blocking_work_executor();
            let request_region_id = region_id;
            let tile_pack_record = crate::runtime::drive(async move {
                rustic_maps_core::blocking_work::run_blocking(
                    blocking_work_executor.as_ref(),
                    move || tile_pack_manager.tile_pack_state(&request_region_id),
                )
                .await
            })
            .await?;
            Ok(tile_pack_record
                .as_ref()
                .map(region_record_from_tile_pack_record))
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = region_id;
            Err(crate::provider::tile_cache_not_configured_error())
        }
    }

    /// Deletes the region's stored tiles and its membership and answers what
    /// was removed; a tile another live region still claims is kept.
    ///
    /// # Errors
    /// Throws `ProviderNotConfigured` naming `TileCache` when no tile cache is
    /// installed and `InvalidCoordinateInput` naming the identifier when no
    /// region is stored under it.
    pub async fn delete_region(&self, region_id: String) -> Result<RegionRemovalReport, SdkError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let tile_pack_manager = self.map_sdk.tile_pack_manager()?;
            let blocking_work_executor = crate::runtime::blocking_work_executor();
            let request_region_id = region_id;
            let tile_pack_removal_report = crate::runtime::drive(async move {
                rustic_maps_core::blocking_work::run_blocking(
                    blocking_work_executor.as_ref(),
                    move || tile_pack_manager.evict_tile_pack(&request_region_id),
                )
                .await
            })
            .await?;
            Ok(RegionRemovalReport {
                removed_entry_count: tile_pack_removal_report.removed_entry_count,
                removed_payload_bytes: tile_pack_removal_report.removed_payload_bytes,
            })
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = region_id;
            Err(crate::provider::tile_cache_not_configured_error())
        }
    }
}

/// The bounding box of a region definition, validated into the native type.
fn region_bounding_box(
    region: &RegionDefinition,
) -> Result<geospatial_types::BoundingBox, SdkError> {
    crate::projection::validated_bounding_box(region.bounding_box)
}

/// The typed refusal of a zero requested region retention: the stored entries
/// would expire on capture.
#[cfg(not(target_arch = "wasm32"))]
fn invalid_zero_region_retention_error() -> SdkError {
    SdkError::InvalidCoordinateInput {
        detail: "a region time-to-live must be positive; zero expires every tile on capture"
            .to_string(),
    }
}

/// Refuses a region deeper than the installed provider's declared maximum zoom
/// level, before anything is defined or downloaded.
#[cfg(not(target_arch = "wasm32"))]
fn refuse_region_above_provider_zoom_ceiling(
    region_maximum_zoom_level: u8,
    provider_maximum_zoom_level: Option<u8>,
) -> Result<(), SdkError> {
    match provider_maximum_zoom_level {
        Some(provider_maximum_zoom_level)
            if region_maximum_zoom_level > provider_maximum_zoom_level =>
        {
            Err(SdkError::InvalidCoordinateInput {
                detail: format!(
                    "zoom level {region_maximum_zoom_level} exceeds the installed provider's \
                     declared maximum of {provider_maximum_zoom_level}"
                ),
            })
        }
        _ => Ok(()),
    }
}

/// Builds the tile-pack description of a region definition, validating its
/// bounding box into the native corner type.
#[cfg(not(target_arch = "wasm32"))]
fn tile_pack_description_from_region(
    region: &RegionDefinition,
) -> Result<rustic_maps_cache::TilePackDescription, SdkError> {
    Ok(rustic_maps_cache::TilePackDescription {
        pack_id: region.region_id.clone(),
        display_name: region.display_name.clone(),
        region_bounding_box: region_bounding_box(region)?,
        minimum_zoom_level: region.minimum_zoom_level,
        maximum_zoom_level: region.maximum_zoom_level,
        image_format: region.image_format.to_native_image_format(),
        tile_resolution: region.tile_resolution.to_native_tile_resolution(),
        requested_time_to_live_seconds: region.time_to_live_seconds,
    })
}

/// Builds the binding mirror of one on-disk tile-pack record.
#[cfg(not(target_arch = "wasm32"))]
fn region_record_from_tile_pack_record(
    tile_pack_record: &rustic_maps_cache::TilePackRecord,
) -> RegionRecord {
    RegionRecord {
        region_id: tile_pack_record.pack_id.clone(),
        display_name: tile_pack_record.display_name.clone(),
        bounding_box: BoundingBox::from_native_bounding_box(&tile_pack_record.region_bounding_box),
        minimum_zoom_level: tile_pack_record.minimum_zoom_level,
        maximum_zoom_level: tile_pack_record.maximum_zoom_level,
        image_format: crate::records::RasterImageFormat::from_native_image_format(
            tile_pack_record.image_format,
        ),
        tile_resolution: crate::records::RasterTileResolution::from_native_tile_resolution(
            tile_pack_record.tile_resolution,
        ),
        effective_time_to_live_seconds: tile_pack_record.effective_time_to_live_seconds,
        member_tile_count: tile_pack_record.member_tile_count,
        present_tile_count: tile_pack_record.present_tile_count,
        present_payload_bytes: tile_pack_record.present_payload_bytes,
    }
}

/// One region download's in-flight registration: dropping it releases the
/// region identifier however the download resolved, including an unwinding
/// panic on the driver task.
#[cfg(not(target_arch = "wasm32"))]
struct RegionDownloadRegistration<'sdk> {
    map_sdk: &'sdk MapSdk,
    region_id: String,
    /// This download's own registry entry: the guard removes only this entry,
    /// never a newer download registered under the same identifier after an
    /// installation replacement cleared the registry.
    region_download_cancellation: Arc<crate::provider::RegionDownloadCancellation>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for RegionDownloadRegistration<'_> {
    fn drop(&mut self) {
        self.map_sdk
            .finish_region_download(&self.region_id, &self.region_download_cancellation);
    }
}

/// Reports progress to the foreign listener and keeps the latest counts so a
/// terminal report carries the download's final partial totals.
#[cfg(not(target_arch = "wasm32"))]
struct RegionDownloadReporter {
    region_id: String,
    progress_listener: Arc<dyn RegionDownloadProgressListener>,
    latest_progress: std::sync::Mutex<rustic_maps_cache::TilePackProgress>,
}

#[cfg(not(target_arch = "wasm32"))]
impl RegionDownloadReporter {
    /// Builds a reporter for one region download, starting from zero counts.
    fn new(region_id: String, progress_listener: Arc<dyn RegionDownloadProgressListener>) -> Self {
        Self {
            region_id,
            progress_listener,
            latest_progress: std::sync::Mutex::new(rustic_maps_cache::TilePackProgress {
                total_tile_count: 0,
                completed_tile_count: 0,
                network_fetched_tile_count: 0,
                disk_served_tile_count: 0,
            }),
        }
    }

    /// Records and delivers one non-terminal progress report.
    fn report(&self, progress: rustic_maps_cache::TilePackProgress) {
        {
            *crate::panic_boundary::state_lock_guard(
                &self.latest_progress,
                "the region download progress snapshot",
            ) = progress;
        }
        self.progress_listener
            .on_region_download_progress(RegionDownloadProgress {
                region_id: self.region_id.clone(),
                total_tile_count: progress.total_tile_count,
                completed_tile_count: progress.completed_tile_count,
                network_fetched_tile_count: progress.network_fetched_tile_count,
                disk_served_tile_count: progress.disk_served_tile_count,
                phase: RegionDownloadPhase::Downloading,
            });
    }

    /// Delivers the download's single terminal report, carrying the counts the
    /// last non-terminal report recorded.
    fn report_terminal(&self, phase: RegionDownloadPhase) {
        let latest_progress = *crate::panic_boundary::state_lock_guard(
            &self.latest_progress,
            "the region download progress snapshot",
        );
        self.progress_listener
            .on_region_download_progress(RegionDownloadProgress {
                region_id: self.region_id.clone(),
                total_tile_count: latest_progress.total_tile_count,
                completed_tile_count: latest_progress.completed_tile_count,
                network_fetched_tile_count: latest_progress.network_fetched_tile_count,
                disk_served_tile_count: latest_progress.disk_served_tile_count,
                phase,
            });
    }
}

/// Awaits the region download's cancellation signal. A closed channel means the
/// registry was cleared (the installation was replaced), which is a
/// cancellation too.
#[cfg(not(target_arch = "wasm32"))]
async fn wait_for_cancellation(mut cancellation_receiver: tokio::sync::watch::Receiver<bool>) {
    if *cancellation_receiver.borrow() {
        return;
    }
    while cancellation_receiver.changed().await.is_ok() {
        if *cancellation_receiver.borrow() {
            return;
        }
    }
}

/// Runs one region download on the crate-owned runtime: races the prepare
/// future against the cancellation signal and answers the final record.
#[cfg(not(target_arch = "wasm32"))]
async fn run_region_download(
    tile_pack_manager: rustic_maps_cache::TilePackManager,
    tile_pack_description: rustic_maps_cache::TilePackDescription,
    cancellation_receiver: tokio::sync::watch::Receiver<bool>,
    progress_listener: Arc<dyn RegionDownloadProgressListener>,
) -> Result<RegionRecord, SdkError> {
    let reporter =
        RegionDownloadReporter::new(tile_pack_description.pack_id.clone(), progress_listener);
    // The prepare future is confined to this block, so a cancellation that
    // wins the race drops it here (cancellation-safe by the manager's
    // contract) before the partial record is read back.
    let preparation_outcome = {
        let mut progress_callback = |progress| reporter.report(progress);
        let preparation =
            tile_pack_manager.prepare_tile_pack(&tile_pack_description, &mut progress_callback);
        tokio::pin!(preparation);
        // `biased` polls the prepare first: the manager defines the pack's
        // membership synchronously before its first await, so a cancellation
        // that wins afterwards always finds the partial region defined and
        // resumable.
        tokio::select! {
            biased;
            preparation_result = &mut preparation => Some(preparation_result),
            _ = wait_for_cancellation(cancellation_receiver) => None,
        }
    };
    match preparation_outcome {
        Some(Ok(tile_pack_record)) => {
            reporter.report_terminal(RegionDownloadPhase::Completed);
            Ok(region_record_from_tile_pack_record(&tile_pack_record))
        }
        Some(Err(download_error)) => {
            reporter.report_terminal(RegionDownloadPhase::Failed);
            Err(download_error.into())
        }
        None => {
            match define_and_read_cancelled_region(&tile_pack_manager, &tile_pack_description).await
            {
                Ok(tile_pack_record) => {
                    reporter.report_terminal(RegionDownloadPhase::Cancelled);
                    Ok(region_record_from_tile_pack_record(&tile_pack_record))
                }
                Err(store_error) => {
                    reporter.report_terminal(RegionDownloadPhase::Failed);
                    Err(store_error)
                }
            }
        }
    }
}

/// Reads the partial region a cancelled download left behind, defining its
/// membership first if the cancellation raced the prepare's very first poll.
#[cfg(not(target_arch = "wasm32"))]
async fn define_and_read_cancelled_region(
    tile_pack_manager: &rustic_maps_cache::TilePackManager,
    tile_pack_description: &rustic_maps_cache::TilePackDescription,
) -> Result<rustic_maps_cache::TilePackRecord, SdkError> {
    if let Some(tile_pack_record) =
        tile_pack_manager.tile_pack_state(&tile_pack_description.pack_id)?
    {
        return Ok(tile_pack_record);
    }
    // The membership is defined synchronously before the prepare's first
    // await, so polling it once defines it; the future is then dropped
    // without downloading anything.
    let mut discard_progress = |_progress: rustic_maps_cache::TilePackProgress| {};
    {
        let preparation =
            tile_pack_manager.prepare_tile_pack(tile_pack_description, &mut discard_progress);
        tokio::pin!(preparation);
        let _first_poll = std::future::poll_fn(|poll_context| {
            std::task::Poll::Ready(std::future::Future::poll(
                preparation.as_mut(),
                poll_context,
            ))
        })
        .await;
    }
    tile_pack_manager
        .tile_pack_state(&tile_pack_description.pack_id)?
        .ok_or_else(|| SdkError::CacheOperationFailure {
            detail: format!(
                "the cancelled download of region {:?} left no membership record to resume from",
                tile_pack_description.pack_id
            ),
        })
}

/// Diagnostics facade over a [`MapSdk`]'s structured log-event stream and
/// bounded ring buffers.
pub struct Diagnostics {
    /// The SDK whose diagnostics this facade observes.
    map_sdk: Arc<MapSdk>,
}

impl Diagnostics {
    /// Builds the facade over the SDK whose diagnostics it observes.
    pub fn new(map_sdk: Arc<MapSdk>) -> Self {
        Self { map_sdk }
    }

    /// Subscribes the foreign listener to the SDK's structured internal log
    /// events at `minimum_severity` or above. Delegates to
    /// [`MapSdk::subscribe_to_diagnostic_events`].
    pub fn add_listener(
        &self,
        listener: Arc<dyn SdkDiagnosticEventListener>,
        minimum_severity: SdkLogSeverity,
    ) {
        self.map_sdk
            .subscribe_to_diagnostic_events(listener, minimum_severity);
    }

    /// Drains up to `maximum_count` of the most recent ring events at
    /// `minimum_severity` or above through the listener, oldest first, and
    /// answers how many were delivered. Delegates to
    /// [`MapSdk::flush_recent_diagnostic_events`].
    pub fn flush_recent(
        &self,
        listener: Arc<dyn SdkDiagnosticEventListener>,
        minimum_severity: SdkLogSeverity,
        maximum_count: u32,
    ) -> u32 {
        self.map_sdk
            .flush_recent_diagnostic_events(listener, minimum_severity, maximum_count)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use async_trait::async_trait;
    use geospatial_traits::RasterTileProvider;
    use geospatial_traits::mock::{
        MockDistanceMatrixProvider, MockGeocodingProvider, MockRoutingProvider,
        MockSearchPlaceProvider,
    };
    use geospatial_types::{
        RasterImageFormat as NativeRasterImageFormat, RasterTileData,
        RasterTileResolution as NativeRasterTileResolution,
    };
    use rustic_maps_test_support::{
        SACRAMENTO_LATITUDE_DEGREES as TEST_LATITUDE_DEGREES,
        SACRAMENTO_LONGITUDE_DEGREES as TEST_LONGITUDE_DEGREES,
    };
    use tokio::sync::Notify;

    use super::{
        Diagnostics, OfflineManager, RegionDownloadProgressListener, RoutingEngine, SearchEngine,
    };
    use crate::diagnostics::SdkDiagnosticEventListener;
    use crate::error::SdkError;
    use crate::provider::MapSdk;
    use crate::records::{
        AvoidOptions, BoundingBox, GeographicCoordinate, LogEvent, RasterImageFormat,
        RasterTileResolution, RegionDefinition, RegionDownloadPhase, RegionDownloadProgress,
        RegionRecord, RouteOptions, RouteStrategy, SdkLogSeverity, TransportationMode, UnitSystem,
    };

    /// Latitude of the second mock coordinate, north-east of the anchor.
    const OTHER_LATITUDE_DEGREES: f64 = 39.5296;
    /// Longitude of the second mock coordinate, north-east of the anchor.
    const OTHER_LONGITUDE_DEGREES: f64 = -119.8138;
    /// Query text the search mocks echo.
    const TEST_QUERY_TEXT: &str = "Capito";
    /// Consumer key the tile-cache tests install; construction needs no live
    /// endpoint.
    const MAPQUEST_TEST_CONSUMER_KEY: &str = "test-consumer-key";
    /// Tile-cache byte budget the settings tests install.
    const TEST_TILE_CACHE_BUDGET_BYTES: u64 = 16 * 1024 * 1024;
    /// Time-to-live override the offline-manager delegation test applies, in
    /// seconds.
    const TEST_TILE_TIME_TO_LIVE_SECONDS: u64 = 60;
    /// Pack byte-budget override the offline-manager delegation test applies,
    /// in bytes.
    const TEST_PACK_BYTE_BUDGET_BYTES: u64 = 8 * 1024 * 1024;

    fn coordinate(latitude: f64, longitude: f64) -> GeographicCoordinate {
        GeographicCoordinate {
            latitude,
            longitude,
        }
    }

    fn sdk_with_mock_providers() -> Arc<MapSdk> {
        let map_sdk = MapSdk::new();
        map_sdk.register_geocoding_provider(Arc::new(MockGeocodingProvider));
        map_sdk.register_routing_provider(Arc::new(MockRoutingProvider));
        map_sdk.register_distance_matrix_provider(Arc::new(MockDistanceMatrixProvider));
        map_sdk.register_search_place_provider(Arc::new(MockSearchPlaceProvider));
        Arc::new(map_sdk)
    }

    fn empty_route_options() -> RouteOptions {
        RouteOptions {
            transportation_mode: TransportationMode::Automobile,
            route_strategy: RouteStrategy::Fastest,
            unit_system: UnitSystem::Metric,
            avoid_options: AvoidOptions {
                toll_roads: false,
                limited_access_highways: false,
                ferries: false,
                unpaved_roads: false,
            },
            departure_timestamp_seconds: None,
        }
    }

    #[tokio::test]
    async fn search_engine_delegates_every_search_verb_to_the_sdk() {
        let search_engine = SearchEngine::new(sdk_with_mock_providers());
        let places = search_engine
            .search(TEST_QUERY_TEXT.to_string(), None)
            .await
            .expect("the registered search provider serves the query");
        assert_eq!(places[0].structured_address_text, TEST_QUERY_TEXT);
        let suggestions = search_engine
            .suggest(TEST_QUERY_TEXT.to_string(), None)
            .await
            .expect("the registered search provider serves the suggestion");
        assert_eq!(suggestions[0].structured_address_text, TEST_QUERY_TEXT);
        let reversed_places = search_engine
            .reverse_geocode(coordinate(OTHER_LATITUDE_DEGREES, OTHER_LONGITUDE_DEGREES))
            .await
            .expect("the registered geocoder serves the coordinate");
        assert_eq!(
            reversed_places[0].coordinate.longitude,
            OTHER_LONGITUDE_DEGREES
        );
    }

    #[tokio::test]
    async fn search_engine_propagates_the_not_configured_failure_verbatim() {
        let search_engine = SearchEngine::new(Arc::new(MapSdk::new()));
        let failure = search_engine
            .search(TEST_QUERY_TEXT.to_string(), None)
            .await
            .expect_err("an unregistered search contract must throw");
        assert_eq!(
            failure,
            SdkError::ProviderNotConfigured {
                contract_name: "SearchPlaceProvider".to_string(),
            }
        );
    }

    #[tokio::test]
    async fn routing_engine_delegates_route_and_matrix_to_the_sdk() {
        let routing_engine = RoutingEngine::new(sdk_with_mock_providers());
        let route = routing_engine
            .calculate_route(
                vec![
                    coordinate(TEST_LATITUDE_DEGREES, TEST_LONGITUDE_DEGREES),
                    coordinate(OTHER_LATITUDE_DEGREES, OTHER_LONGITUDE_DEGREES),
                ],
                empty_route_options(),
            )
            .await
            .expect("the registered router serves the waypoints");
        assert_eq!(route.distance_meters, 1000.0);
        assert_eq!(route.duration_seconds, 90.0);
        let distance_matrix = routing_engine
            .calculate_distance_matrix(
                vec![coordinate(TEST_LATITUDE_DEGREES, TEST_LONGITUDE_DEGREES)],
                vec![coordinate(OTHER_LATITUDE_DEGREES, OTHER_LONGITUDE_DEGREES)],
                TransportationMode::Automobile,
            )
            .await
            .expect("the registered matrix provider serves the pair");
        assert_eq!(distance_matrix.entries.len(), 1);
        assert_eq!(distance_matrix.entries[0].distance_meters, 500.0);
    }

    #[tokio::test]
    async fn routing_engine_propagates_the_not_configured_failure_verbatim() {
        let routing_engine = RoutingEngine::new(Arc::new(MapSdk::new()));
        let route_failure = routing_engine
            .calculate_route(
                vec![coordinate(TEST_LATITUDE_DEGREES, TEST_LONGITUDE_DEGREES)],
                empty_route_options(),
            )
            .await
            .expect_err("an unregistered routing contract must throw");
        assert_eq!(
            route_failure,
            SdkError::ProviderNotConfigured {
                contract_name: "RoutingProvider".to_string(),
            }
        );
        let matrix_failure = routing_engine
            .calculate_distance_matrix(
                vec![coordinate(TEST_LATITUDE_DEGREES, TEST_LONGITUDE_DEGREES)],
                vec![coordinate(OTHER_LATITUDE_DEGREES, OTHER_LONGITUDE_DEGREES)],
                TransportationMode::Automobile,
            )
            .await
            .expect_err("an unregistered distance-matrix contract must throw");
        assert_eq!(
            matrix_failure,
            SdkError::ProviderNotConfigured {
                contract_name: "DistanceMatrixProvider".to_string(),
            }
        );
    }

    #[test]
    fn offline_manager_refuses_the_settings_calls_without_an_installed_cache() {
        let offline_manager = OfflineManager::new(Arc::new(MapSdk::new()));
        match offline_manager.tile_cache_terms() {
            Err(SdkError::ProviderNotConfigured { contract_name }) => {
                assert_eq!(contract_name, "TileCache");
            }
            other => panic!("terms without an installed cache must throw, got {other:?}"),
        }
        match offline_manager.set_tile_time_to_live(TEST_TILE_TIME_TO_LIVE_SECONDS) {
            Err(SdkError::ProviderNotConfigured { contract_name }) => {
                assert_eq!(contract_name, "TileCache");
            }
            other => panic!("the override without an installed cache must throw, got {other:?}"),
        }
        match offline_manager.tile_pack_byte_budget() {
            Err(SdkError::ProviderNotConfigured { contract_name }) => {
                assert_eq!(contract_name, "TileCache");
            }
            other => panic!(
                "the pack budget readback without an installed cache must throw, got {other:?}"
            ),
        }
        match offline_manager.set_tile_pack_byte_budget(TEST_PACK_BYTE_BUDGET_BYTES) {
            Err(SdkError::ProviderNotConfigured { contract_name }) => {
                assert_eq!(contract_name, "TileCache");
            }
            other => {
                panic!("the pack budget without an installed cache must throw, got {other:?}")
            }
        }
        // The SDK's own zero guard still owns the validation.
        let sdk_with_cache = registered_with_labelled_tile_cache("engines-offline-manager");
        let manager_with_cache = OfflineManager::new(Arc::clone(&sdk_with_cache.0));
        match manager_with_cache.set_tile_time_to_live(0) {
            Err(SdkError::InvalidCoordinateInput { detail }) => {
                assert!(detail.contains("must be positive"), "{detail}");
            }
            other => panic!("a zero time-to-live must be refused, got {other:?}"),
        }
        std::fs::remove_dir_all(&sdk_with_cache.1).expect("the test cache directory cleans up");
    }

    #[test]
    fn offline_manager_delegates_the_installed_cache_terms_and_override() {
        let (map_sdk, cache_directory) =
            registered_with_labelled_tile_cache("engines-offline-happy");
        let offline_manager = OfflineManager::new(map_sdk);
        let default_terms = offline_manager
            .tile_cache_terms()
            .expect("an installed cache reports its terms");
        assert_eq!(
            default_terms.effective_time_to_live_seconds,
            default_terms.default_time_to_live_seconds
        );
        offline_manager
            .set_tile_time_to_live(TEST_TILE_TIME_TO_LIVE_SECONDS)
            .expect("the override applies to the installed cache");
        assert_eq!(
            offline_manager
                .tile_cache_terms()
                .expect("the terms read back after the override")
                .effective_time_to_live_seconds,
            TEST_TILE_TIME_TO_LIVE_SECONDS
        );
        std::fs::remove_dir_all(&cache_directory).expect("the test cache directory cleans up");
    }

    #[test]
    fn offline_manager_reports_and_overrides_the_pack_byte_budget() {
        let (map_sdk, cache_directory) = registered_with_labelled_tile_cache("engines-pack-budget");
        let offline_manager = OfflineManager::new(map_sdk);
        assert_eq!(
            offline_manager
                .tile_pack_byte_budget()
                .expect("an installed cache reports its pack budget"),
            rustic_maps_cache::DEFAULT_TILE_PACK_BYTE_BUDGET_BYTES,
            "no reservation yet reports the SDK default"
        );
        offline_manager
            .set_tile_pack_byte_budget(TEST_PACK_BYTE_BUDGET_BYTES)
            .expect("the reservation applies to the installed cache");
        assert_eq!(
            offline_manager
                .tile_pack_byte_budget()
                .expect("the pack budget reads back after the reservation"),
            TEST_PACK_BYTE_BUDGET_BYTES
        );
        match offline_manager.set_tile_pack_byte_budget(0) {
            Err(SdkError::InvalidCoordinateInput { detail }) => {
                assert!(detail.contains("must be positive"), "{detail}");
            }
            other => panic!("a zero pack budget must be refused, got {other:?}"),
        }
        assert_eq!(
            offline_manager
                .tile_pack_byte_budget()
                .expect("a refused budget leaves the previous value in force"),
            TEST_PACK_BYTE_BUDGET_BYTES
        );
        std::fs::remove_dir_all(&cache_directory).expect("the test cache directory cleans up");
    }

    /// Registers the MapQuest providers with a freshly named tile cache and
    /// answers the SDK with the cache directory the test cleans up.
    fn registered_with_labelled_tile_cache(test_label: &str) -> (Arc<MapSdk>, std::path::PathBuf) {
        let map_sdk = MapSdk::new();
        let cache_directory = std::env::temp_dir().join(format!(
            "rustic-maps-native-core-{test_label}-{}",
            std::process::id()
        ));
        map_sdk
            .set_provider_credential_with_tile_cache(
                "mapquest".to_string(),
                MAPQUEST_TEST_CONSUMER_KEY.to_string(),
                cache_directory
                    .join("tiles.sqlite")
                    .to_string_lossy()
                    .to_string(),
                TEST_TILE_CACHE_BUDGET_BYTES,
            )
            .expect("an empty path in an existing directory opens the cache");
        (Arc::new(map_sdk), cache_directory)
    }

    /// A diagnostics listener recording every delivered event and counting
    /// its deliveries.
    #[derive(Default)]
    struct RecordingDiagnosticEventListener {
        delivered_events: Mutex<Vec<LogEvent>>,
        delivery_count: AtomicU32,
    }

    impl SdkDiagnosticEventListener for RecordingDiagnosticEventListener {
        fn on_diagnostic_event(&self, event: LogEvent) {
            self.delivered_events.lock().unwrap().push(event);
            self.delivery_count.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn diagnostics_delegates_the_listener_and_the_bounded_flush() {
        let diagnostics = Diagnostics::new(Arc::new(MapSdk::new()));
        let listener = Arc::new(RecordingDiagnosticEventListener::default());
        diagnostics.add_listener(
            Arc::clone(&listener) as Arc<dyn SdkDiagnosticEventListener>,
            SdkLogSeverity::Warn,
        );
        let delivered_count = diagnostics.flush_recent(
            Arc::clone(&listener) as Arc<dyn SdkDiagnosticEventListener>,
            SdkLogSeverity::Trace,
            u32::MAX,
        );
        assert_eq!(
            listener.delivery_count.load(Ordering::SeqCst),
            delivered_count,
            "the flush answers exactly the events it delivered"
        );
        assert_eq!(
            listener.delivered_events.lock().unwrap().len() as u32,
            delivered_count
        );
    }

    // -----------------------------------------------------------------------
    // Offline region downloads: network-free, over a scripted provider double.
    // -----------------------------------------------------------------------

    /// Identifier the offline-region tests store their region under.
    const TEST_REGION_ID: &str = "demo-region";
    /// Display name the offline-region tests store with their region.
    const TEST_REGION_DISPLAY_NAME: &str = "Demo region";
    /// Zoom level the demo region downloads: a two-column tile row.
    const TEST_REGION_ZOOM_LEVEL: u8 = 2;
    /// South-west latitude of the demo region, in decimal degrees.
    const TEST_REGION_SOUTH_WEST_LATITUDE_DEGREES: f64 = 38.57;
    /// South-west longitude of the demo region, in decimal degrees.
    const TEST_REGION_SOUTH_WEST_LONGITUDE_DEGREES: f64 = -100.0;
    /// North-east latitude of the demo region, in decimal degrees.
    const TEST_REGION_NORTH_EAST_LATITUDE_DEGREES: f64 = 38.58;
    /// North-east longitude of the demo region, in decimal degrees.
    const TEST_REGION_NORTH_EAST_LONGITUDE_DEGREES: f64 = -10.0;
    /// Tile count the demo region enumerates at its one zoom level.
    const TEST_REGION_TILE_COUNT: u64 = 2;
    /// Payload byte length the scripted provider answers every tile with.
    const SCRIPTED_TILE_PAYLOAD_BYTES: usize = 16;
    /// Delay the scripted provider spends per tile, long enough that a
    /// cancellation lands between two tiles.
    const SCRIPTED_TILE_DELAY: Duration = Duration::from_millis(40);
    /// How long a held tile stays in flight: far longer than any test, so the
    /// only way the download ends is the cancellation under test.
    const HELD_TILE_DELAY: Duration = Duration::from_secs(60);
    /// Tile-cache byte budget the offline-region tests install.
    const TEST_REGION_TILE_CACHE_BUDGET_BYTES: u64 = 16 * 1024 * 1024;
    /// Retention the test installation carries, in seconds (thirty days).
    const TEST_REGION_TIME_TO_LIVE_SECONDS: u64 = 30 * 24 * 60 * 60;

    /// Network-free provider double: a deterministic payload per tile, a
    /// configurable per-tile delay and a request counter.
    struct ScriptedRasterTileProvider {
        requested_tile_count: AtomicU32,
        per_tile_delay: Duration,
        hold_tiles_after_the_first: bool,
    }

    impl ScriptedRasterTileProvider {
        fn new(per_tile_delay: Duration) -> Self {
            Self {
                requested_tile_count: AtomicU32::new(0),
                per_tile_delay,
                hold_tiles_after_the_first: false,
            }
        }

        /// Holds every tile after the first until the download is cancelled,
        /// so a cancellation test never races a fast machine: the first tile
        /// completes, the cancellation is signalled, and the tiles behind it
        /// are still in flight by construction.
        fn holding_tiles_after_the_first(per_tile_delay: Duration) -> Self {
            Self {
                hold_tiles_after_the_first: true,
                ..Self::new(per_tile_delay)
            }
        }

        fn requested_tile_count(&self) -> u32 {
            self.requested_tile_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl RasterTileProvider for ScriptedRasterTileProvider {
        async fn fetch_raster_tile(
            &self,
            tile_x: u32,
            tile_y: u32,
            zoom_level: u8,
            image_format: NativeRasterImageFormat,
            tile_resolution: NativeRasterTileResolution,
        ) -> Result<RasterTileData, geospatial_types::SDKError> {
            let request_index = self.requested_tile_count.fetch_add(1, Ordering::SeqCst) + 1;
            if self.hold_tiles_after_the_first && request_index > 1 {
                tokio::time::sleep(HELD_TILE_DELAY).await;
            } else if !self.per_tile_delay.is_zero() {
                tokio::time::sleep(self.per_tile_delay).await;
            }
            Ok(RasterTileData {
                tile_x,
                tile_y,
                zoom_level,
                image_format,
                tile_resolution,
                image_bytes: vec![0x5a; SCRIPTED_TILE_PAYLOAD_BYTES],
            })
        }
    }

    /// A progress listener recording every report and signalling a notification
    /// on the first completed tile, so a cancellation test can cancel while
    /// the download is demonstrably in flight.
    #[derive(Default)]
    struct RecordingRegionDownloadProgressListener {
        reports: Mutex<Vec<RegionDownloadProgress>>,
        first_completed_tile: Mutex<Option<Arc<Notify>>>,
    }

    impl RecordingRegionDownloadProgressListener {
        fn with_first_completed_tile_notification(first_completed_tile: Arc<Notify>) -> Self {
            Self {
                reports: Mutex::new(Vec::new()),
                first_completed_tile: Mutex::new(Some(first_completed_tile)),
            }
        }

        fn reports(&self) -> Vec<RegionDownloadProgress> {
            self.reports.lock().unwrap().clone()
        }
    }

    impl RegionDownloadProgressListener for RecordingRegionDownloadProgressListener {
        fn on_region_download_progress(&self, progress: RegionDownloadProgress) {
            let completes_a_tile = progress.phase == RegionDownloadPhase::Downloading
                && progress.completed_tile_count > 0;
            self.reports.lock().unwrap().push(progress);
            if completes_a_tile
                && let Some(first_completed_tile) = self.first_completed_tile.lock().unwrap().take()
            {
                first_completed_tile.notify_waiters();
            }
        }
    }

    /// The demo region definition the offline-region tests download.
    fn test_region_definition() -> RegionDefinition {
        RegionDefinition {
            region_id: TEST_REGION_ID.to_string(),
            display_name: TEST_REGION_DISPLAY_NAME.to_string(),
            bounding_box: BoundingBox {
                south_west: coordinate(
                    TEST_REGION_SOUTH_WEST_LATITUDE_DEGREES,
                    TEST_REGION_SOUTH_WEST_LONGITUDE_DEGREES,
                ),
                north_east: coordinate(
                    TEST_REGION_NORTH_EAST_LATITUDE_DEGREES,
                    TEST_REGION_NORTH_EAST_LONGITUDE_DEGREES,
                ),
            },
            minimum_zoom_level: TEST_REGION_ZOOM_LEVEL,
            maximum_zoom_level: TEST_REGION_ZOOM_LEVEL,
            image_format: RasterImageFormat::Png,
            tile_resolution: RasterTileResolution::Standard,
            time_to_live_seconds: None,
        }
    }

    /// Installs a test tile cache over the scripted provider and opens a fresh
    /// SQLite store under a label-unique temporary directory. Answers the SDK
    /// the tests build their manager over, the provider counter and the
    /// directory the test cleans up.
    fn sdk_with_scripted_tile_cache(
        test_label: &str,
        per_tile_delay: Duration,
        maximum_zoom_level: Option<u8>,
        hold_tiles_after_the_first: bool,
    ) -> (
        Arc<MapSdk>,
        Arc<ScriptedRasterTileProvider>,
        std::path::PathBuf,
    ) {
        let map_sdk = MapSdk::new();
        let cache_directory = std::env::temp_dir().join(format!(
            "rustic-maps-native-core-offline-{test_label}-{}",
            std::process::id()
        ));
        // A failed run never reaches its own cleanup, so the directory is
        // emptied before the store opens: a leftover database would serve a
        // later run's tiles from disk and pass or fail the download for the
        // wrong reason.
        let _ = std::fs::remove_dir_all(&cache_directory);
        let response_store = rustic_maps_cache::SqliteResponseCache::open(
            rustic_maps_cache::SqliteResponseCacheConfiguration {
                database_path: cache_directory.join("tiles.sqlite"),
                quota: rustic_maps_cache::CacheQuota {
                    absolute_maximum_bytes: Some(TEST_REGION_TILE_CACHE_BUDGET_BYTES),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .expect("the test tile cache opens");
        let scripted_provider = Arc::new(if hold_tiles_after_the_first {
            ScriptedRasterTileProvider::holding_tiles_after_the_first(per_tile_delay)
        } else {
            ScriptedRasterTileProvider::new(per_tile_delay)
        });
        map_sdk.install_tile_cache_for_testing(
            scripted_provider.clone(),
            response_store,
            TEST_REGION_TIME_TO_LIVE_SECONDS,
            Some(TEST_REGION_TIME_TO_LIVE_SECONDS),
            maximum_zoom_level,
        );
        (Arc::new(map_sdk), scripted_provider, cache_directory)
    }

    #[tokio::test]
    async fn download_region_reports_zero_each_tile_and_one_terminal_report() {
        let (map_sdk, _, cache_directory) =
            sdk_with_scripted_tile_cache("progress-sequence", Duration::ZERO, Some(20), false);
        let offline_manager = OfflineManager::new(map_sdk);
        let listener = Arc::new(RecordingRegionDownloadProgressListener::default());
        let region_record = offline_manager
            .download_region(
                test_region_definition(),
                listener.clone() as Arc<dyn RegionDownloadProgressListener>,
            )
            .await
            .expect("the scripted provider serves every tile");
        assert_eq!(region_record.region_id, TEST_REGION_ID);
        assert_eq!(region_record.member_tile_count, TEST_REGION_TILE_COUNT);
        assert_eq!(region_record.present_tile_count, TEST_REGION_TILE_COUNT);
        assert!(region_record.present_payload_bytes > 0);
        assert_eq!(
            offline_manager
                .region_tile_count(test_region_definition())
                .unwrap(),
            TEST_REGION_TILE_COUNT,
            "the sizing call agrees with the download's member count"
        );

        let reports = listener.reports();
        assert_eq!(
            reports
                .first()
                .expect("the download reported upfront")
                .phase,
            RegionDownloadPhase::Downloading
        );
        assert_eq!(reports[0].completed_tile_count, 0);
        assert_eq!(
            reports
                .last()
                .expect("the download reported terminally")
                .phase,
            RegionDownloadPhase::Completed
        );
        let terminal_count = reports
            .iter()
            .filter(|report| report.phase != RegionDownloadPhase::Downloading)
            .count();
        assert_eq!(terminal_count, 1, "exactly one terminal report");
        assert_eq!(
            reports
                .iter()
                .filter(|report| report.phase == RegionDownloadPhase::Downloading)
                .count(),
            TEST_REGION_TILE_COUNT as usize + 1,
            "one upfront report plus one per completed tile"
        );
        let completed_report = reports.last().unwrap();
        assert_eq!(
            completed_report.network_fetched_tile_count,
            TEST_REGION_TILE_COUNT
        );
        assert_eq!(completed_report.disk_served_tile_count, 0);

        // A re-download serves the tiles from disk and fetches nothing.
        let second_listener = Arc::new(RecordingRegionDownloadProgressListener::default());
        let second_record = offline_manager
            .download_region(
                test_region_definition(),
                second_listener.clone() as Arc<dyn RegionDownloadProgressListener>,
            )
            .await
            .expect("the present region re-prepares");
        assert_eq!(second_record.present_tile_count, TEST_REGION_TILE_COUNT);
        let second_completed_report = second_listener
            .reports()
            .pop()
            .expect("the re-prepare reported terminally");
        assert_eq!(
            second_completed_report.phase,
            RegionDownloadPhase::Completed
        );
        assert_eq!(
            second_completed_report.disk_served_tile_count,
            TEST_REGION_TILE_COUNT
        );
        assert_eq!(second_completed_report.network_fetched_tile_count, 0);

        std::fs::remove_dir_all(&cache_directory).expect("the test cache directory cleans up");
    }

    #[tokio::test]
    async fn a_region_above_the_provider_zoom_ceiling_is_refused_before_any_download() {
        let (map_sdk, scripted_provider, cache_directory) = sdk_with_scripted_tile_cache(
            "zoom-ceiling",
            Duration::ZERO,
            Some(TEST_REGION_ZOOM_LEVEL - 1),
            false,
        );
        let offline_manager = OfflineManager::new(map_sdk);
        match offline_manager.region_tile_count(test_region_definition()) {
            Err(SdkError::InvalidCoordinateInput { detail }) => {
                assert!(detail.contains("declared maximum"), "{detail}");
            }
            other => panic!("a region above the ceiling must be refused, got {other:?}"),
        }
        let listener = Arc::new(RecordingRegionDownloadProgressListener::default());
        match offline_manager
            .download_region(
                test_region_definition(),
                listener.clone() as Arc<dyn RegionDownloadProgressListener>,
            )
            .await
        {
            Err(SdkError::InvalidCoordinateInput { detail }) => {
                assert!(detail.contains("declared maximum"), "{detail}");
            }
            other => panic!("a region above the ceiling must be refused, got {other:?}"),
        }
        assert_eq!(
            scripted_provider.requested_tile_count(),
            0,
            "the refusal happens before any tile is fetched"
        );
        assert_eq!(
            offline_manager
                .region_record(TEST_REGION_ID.to_string())
                .await
                .unwrap(),
            None,
            "a refused definition defines nothing"
        );
        assert_eq!(
            listener.reports().last().map(|report| report.phase),
            Some(RegionDownloadPhase::Failed),
            "the refusal reports one terminal Failed phase"
        );
        std::fs::remove_dir_all(&cache_directory).expect("the test cache directory cleans up");
    }

    #[tokio::test]
    async fn a_second_concurrent_download_of_the_same_region_is_refused_and_cancellation_resumes() {
        let (map_sdk, scripted_provider, cache_directory) =
            sdk_with_scripted_tile_cache("cancel-and-resume", SCRIPTED_TILE_DELAY, Some(20), false);
        let offline_manager = Arc::new(OfflineManager::new(map_sdk));
        let first_completed_tile = Arc::new(Notify::new());
        let listener = Arc::new(
            RecordingRegionDownloadProgressListener::with_first_completed_tile_notification(
                first_completed_tile.clone(),
            ),
        );
        let running_manager = Arc::clone(&offline_manager);
        let running_download = tokio::spawn(async move {
            running_manager
                .download_region(
                    test_region_definition(),
                    listener.clone() as Arc<dyn RegionDownloadProgressListener>,
                )
                .await
        });
        first_completed_tile.notified().await;

        // The same region is still in flight: a duplicate is refused with a
        // typed cache failure naming the region.
        let duplicate_listener = Arc::new(RecordingRegionDownloadProgressListener::default());
        match offline_manager
            .download_region(
                test_region_definition(),
                duplicate_listener as Arc<dyn RegionDownloadProgressListener>,
            )
            .await
        {
            Err(SdkError::CacheOperationFailure { detail }) => {
                assert!(detail.contains(TEST_REGION_ID), "{detail}");
            }
            other => panic!("a duplicate region download must be refused, got {other:?}"),
        }

        offline_manager.cancel_region_download(TEST_REGION_ID.to_string());
        let cancelled_record = running_download
            .await
            .expect("the download task joins")
            .expect("cancellation is a successful partial result");
        assert!(
            cancelled_record.present_tile_count >= 1
                && cancelled_record.present_tile_count < TEST_REGION_TILE_COUNT,
            "the cancelled region keeps the tiles that arrived: {cancelled_record:?}"
        );

        // Cancellation is a no-op for a finished region.
        offline_manager.cancel_region_download(TEST_REGION_ID.to_string());

        // The resume serves the kept tile from disk and fetches the missing one.
        let resume_listener = Arc::new(RecordingRegionDownloadProgressListener::default());
        let resumed_record = offline_manager
            .download_region(
                test_region_definition(),
                resume_listener.clone() as Arc<dyn RegionDownloadProgressListener>,
            )
            .await
            .expect("the resume finishes the region");
        assert_eq!(resumed_record.present_tile_count, TEST_REGION_TILE_COUNT);
        let resume_report = resume_listener
            .reports()
            .pop()
            .expect("the resume reported terminally");
        assert_eq!(resume_report.phase, RegionDownloadPhase::Completed);
        assert!(
            resume_report.disk_served_tile_count >= 1,
            "the resume reuses the kept tile: {resume_report:?}"
        );
        assert_eq!(
            resume_report.disk_served_tile_count + resume_report.network_fetched_tile_count,
            TEST_REGION_TILE_COUNT
        );
        assert!(scripted_provider.requested_tile_count() >= 1);
        std::fs::remove_dir_all(&cache_directory).expect("the test cache directory cleans up");
    }

    /// Delay the cancellation test spends per tile. The download runs one
    /// tile at a time, so the second tile only starts after the first
    /// completes; this delay keeps it in flight while the test cancels. A
    /// parallel download would start both tiles and finish them together,
    /// which is the flake this test used to have.
    const CANCELLATION_TILE_DELAY: Duration = Duration::from_millis(250);
    /// How long the cancellation test waits for the download to reach its
    /// first completed tile, and then to join after the installation is
    /// replaced. A download that fails before its first tile would otherwise
    /// leave the test parked forever; the watchdog turns that into a failure.
    const CANCELLATION_WATCHDOG: Duration = Duration::from_secs(30);

    #[tokio::test]
    async fn replacing_the_tile_cache_cancels_an_in_flight_download() {
        let (map_sdk, _, cache_directory) = sdk_with_scripted_tile_cache(
            "replacement-cancels",
            CANCELLATION_TILE_DELAY,
            Some(TEST_REGION_ZOOM_LEVEL),
            true,
        );
        let offline_manager = Arc::new(OfflineManager::new(Arc::clone(&map_sdk)));
        let first_completed_tile = Arc::new(Notify::new());
        let listener = Arc::new(
            RecordingRegionDownloadProgressListener::with_first_completed_tile_notification(
                first_completed_tile.clone(),
            ),
        );
        let running_manager = Arc::clone(&offline_manager);
        let running_download = tokio::spawn(async move {
            running_manager
                .download_region(
                    test_region_definition(),
                    listener.clone() as Arc<dyn RegionDownloadProgressListener>,
                )
                .await
        });
        tokio::time::timeout(CANCELLATION_WATCHDOG, first_completed_tile.notified())
            .await
            .expect("the download reaches its first completed tile");

        // Replacing the installation (a plain re-registration drops the disk
        // cache) signals every in-flight download to stop.
        map_sdk
            .configure_provider_set(
                "mapquest".to_string(),
                MAPQUEST_TEST_CONSUMER_KEY.to_string(),
            )
            .expect("the plain registration installs");
        let cancelled_record = tokio::time::timeout(CANCELLATION_WATCHDOG, running_download)
            .await
            .expect("the replaced installation ends the download")
            .expect("the download task joins")
            .expect("a replaced installation cancels the download successfully");
        assert!(
            cancelled_record.present_tile_count < TEST_REGION_TILE_COUNT,
            "the cancelled region keeps only the tiles that arrived: {cancelled_record:?}"
        );
        // With the installation dropped, the pack surface refuses honestly.
        match offline_manager.region_records().await {
            Err(SdkError::ProviderNotConfigured { contract_name }) => {
                assert_eq!(contract_name, "TileCache");
            }
            other => panic!("the replaced installation leaves no cache, got {other:?}"),
        }
        std::fs::remove_dir_all(&cache_directory).expect("the test cache directory cleans up");
    }

    #[tokio::test]
    async fn region_records_list_read_and_delete_the_stored_regions() {
        let (map_sdk, _, cache_directory) =
            sdk_with_scripted_tile_cache("list-and-delete", Duration::ZERO, Some(20), false);
        let offline_manager = OfflineManager::new(map_sdk);
        offline_manager
            .download_region(
                test_region_definition(),
                Arc::new(RecordingRegionDownloadProgressListener::default()),
            )
            .await
            .expect("the scripted provider serves every tile");

        let region_records = offline_manager
            .region_records()
            .await
            .expect("the store lists its regions");
        assert_eq!(region_records.len(), 1);
        assert_eq!(region_records[0].region_id, TEST_REGION_ID);
        let region_record: RegionRecord = offline_manager
            .region_record(TEST_REGION_ID.to_string())
            .await
            .expect("the stored region reads back")
            .expect("the region is present");
        assert_eq!(region_record.display_name, TEST_REGION_DISPLAY_NAME);
        assert_eq!(region_record.present_tile_count, TEST_REGION_TILE_COUNT);
        assert_eq!(
            region_record.effective_time_to_live_seconds, TEST_REGION_TIME_TO_LIVE_SECONDS,
            "the requested retention applies under the provider ceiling"
        );

        let removal_report = offline_manager
            .delete_region(TEST_REGION_ID.to_string())
            .await
            .expect("the stored region deletes");
        assert_eq!(removal_report.removed_entry_count, TEST_REGION_TILE_COUNT);
        assert!(removal_report.removed_payload_bytes > 0);
        assert_eq!(
            offline_manager
                .region_record(TEST_REGION_ID.to_string())
                .await
                .unwrap(),
            None
        );
        assert!(offline_manager.region_records().await.unwrap().is_empty());

        match offline_manager
            .delete_region(TEST_REGION_ID.to_string())
            .await
        {
            Err(SdkError::InvalidCoordinateInput { detail }) => {
                assert!(detail.contains(TEST_REGION_ID), "{detail}");
            }
            other => panic!("deleting an unknown region must be refused, got {other:?}"),
        }
        std::fs::remove_dir_all(&cache_directory).expect("the test cache directory cleans up");
    }

    #[tokio::test]
    async fn every_pack_member_refuses_without_an_installed_tile_cache() {
        let offline_manager = OfflineManager::new(Arc::new(MapSdk::new()));
        match offline_manager.region_tile_count(test_region_definition()) {
            Err(SdkError::ProviderNotConfigured { contract_name }) => {
                assert_eq!(contract_name, "TileCache");
            }
            other => panic!("sizing without a cache must refuse, got {other:?}"),
        }
        match offline_manager
            .download_region(
                test_region_definition(),
                Arc::new(RecordingRegionDownloadProgressListener::default()),
            )
            .await
        {
            Err(SdkError::ProviderNotConfigured { contract_name }) => {
                assert_eq!(contract_name, "TileCache");
            }
            other => panic!("downloading without a cache must refuse, got {other:?}"),
        }
        match offline_manager.region_records().await {
            Err(SdkError::ProviderNotConfigured { contract_name }) => {
                assert_eq!(contract_name, "TileCache");
            }
            other => panic!("listing without a cache must refuse, got {other:?}"),
        }
        match offline_manager
            .region_record(TEST_REGION_ID.to_string())
            .await
        {
            Err(SdkError::ProviderNotConfigured { contract_name }) => {
                assert_eq!(contract_name, "TileCache");
            }
            other => panic!("reading without a cache must refuse, got {other:?}"),
        }
        match offline_manager
            .delete_region(TEST_REGION_ID.to_string())
            .await
        {
            Err(SdkError::ProviderNotConfigured { contract_name }) => {
                assert_eq!(contract_name, "TileCache");
            }
            other => panic!("deleting without a cache must refuse, got {other:?}"),
        }
        // Cancelling an unknown region is a documented no-op.
        offline_manager.cancel_region_download(TEST_REGION_ID.to_string());
    }

    #[test]
    fn the_browser_pack_refusal_names_the_tile_cache_contract() {
        // The wasm arms of every pack member return exactly this failure: the
        // browser has no SDK-owned disk store. The host test pins the shape
        // the wasm build answers with.
        match crate::provider::tile_cache_not_configured_error() {
            SdkError::ProviderNotConfigured { contract_name } => {
                assert_eq!(contract_name, "TileCache");
            }
            other => panic!("the browser refusal must be the TileCache failure, got {other:?}"),
        }
    }
}
