//! The UniFFI-facing async provider surface: one [`MapSdk`] object resolving
//! the implemented `geospatial-traits` contracts through the `rustic-maps-core`
//! [`ProviderRegistry`], one exported `async` method per contract call
//! mirroring the trait surface one-to-one.
//!
//! Provider registration has two paths. The native integrator path stays
//! foreign-free: the `register_*` methods here wrap the registry for
//! providers built in Rust, including MapQuest providers constructed from a
//! consumer key. The foreign integrator path injects implementations of the
//! exported callback contracts through the `register_*_from_foreign`
//! methods, which wrap them in the [`crate::foreign_providers`] adapters
//! before the same registry call. Calls into an unregistered contract throw
//! [`SdkError::ProviderNotConfigured`] with the contract name.

use std::sync::{Arc, Mutex};

#[cfg(not(target_arch = "wasm32"))]
use std::collections::HashMap;

use geospatial_traits::{
    DistanceMatrixProvider, ElevationProvider, GeocodingProvider, RasterTileProvider,
    RoutingProvider, SearchPlaceProvider, TrafficFlowProvider, TrafficIncidentProvider,
    VectorTileProvider,
};
use rustic_maps_core::{ProviderRegistry, StyleResourceClient};

use crate::error::SdkError;
use crate::foreign_providers::{
    ForeignDistanceMatrixProvider, ForeignElevationProvider, ForeignGeocodingProvider,
    ForeignRasterTileProvider, ForeignRoutingProvider, ForeignSearchPlaceProvider,
    ForeignTrafficFlowProvider, ForeignTrafficIncidentProvider, ForeignVectorTileProvider,
};
use crate::projection::{validated_bounding_box, validated_coordinate};
use crate::records::{
    BoundingBox, DistanceMatrix, ElevationPoint, GeographicCoordinate, Place, ProviderContract,
    RasterImageFormat, RasterTileData, RasterTileResolution, Route, RouteCapabilities,
    RouteOptions, SdkLogSeverity, TileCacheTerms, TrafficFlowData, TrafficIncident,
    TransportationMode, UrlTemplateRasterTileSource,
};
use crate::runtime::drive;

/// Sentinel standing in for "the installed provider declares no caching-terms
/// ceiling" when clamping a requested tile-cache time-to-live: an
/// `Option<u64>` cannot feed the unclamped `min` arithmetic directly, so a
/// missing ceiling becomes this value no requested time-to-live ever
/// exceeds (the same convention `option.unwrap_or(u64::MAX)` spells
/// inline). Native-only with the clamping helper it feeds.
#[cfg(not(target_arch = "wasm32"))]
const NO_PROVIDER_TERMS_CEILING_SECONDS: u64 = u64::MAX;

/// Registry-registry contract name the tile-cache settings surface reports
/// under `ProviderNotConfigured` when no tile cache is installed: the
/// raster-tile contract itself may be served by an uncached provider, while
/// the settings surface addresses the cache backing store alone.
pub(crate) const TILE_CACHE_NOT_CONFIGURED_CONTRACT_NAME: &str = "TileCache";

/// The provider key a host-platform implementation registers under: the
/// application itself supplies the implementation, so no registry descriptor
/// names it and the settings surface reads the pair back as the serving
/// provider for the capability.
pub(crate) const APPLICATION_PROVIDER_KEY: &str = "application";

/// The display name that accompanies [`APPLICATION_PROVIDER_KEY`].
pub(crate) const APPLICATION_PROVIDER_DISPLAY_NAME: &str = "Application-provided";

/// Async provider entry point of the SDK: resolves every call of the
/// implemented provider contracts through the [`ProviderRegistry`].
///
/// The exported methods mirror the provider contracts one-to-one; each
/// validates its inputs (binding coordinates must be canonical degrees) and
/// converts the native result into the binding records in [`crate::records`].
/// Provider registration stays native (this crate's `register_*` methods).
///
/// The registry lock is taken only to clone a provider slot out of the
/// registry; the guard never spans an await, so every exported future stays
/// `Send`. Each exported method resolves its provider synchronously and
/// then hands the call's work to the crate-owned tokio runtime through
/// [`drive`], whose returned future only awaits a completion channel — the
/// foreign executor polls a runtime-independent handback, because the
/// provider futures need a live reactor no foreign polling thread runs (see
/// the `runtime` module).
#[derive(Debug)]
pub struct MapSdk {
    provider_registry: Mutex<ProviderRegistry>,
    /// The tile cache currently installed behind the raster-tile contract
    /// slot with the terms its settings surface reads and tunes, or `None`
    /// while the raster-tile contract serves uncached (never registered
    /// with a tile cache, or re-registered without one — re-registration
    /// replaces the slots and the tuning with them). The wasm build has no
    /// disk cache store, so it carries no installation slot at all.
    #[cfg(not(target_arch = "wasm32"))]
    tile_cache_installation: Mutex<Option<TileCacheInstallation>>,
    /// The provider-failure diagnostic seam's ring: the bounded record of
    /// every failed foreign-callback provider call registered on
    /// this object, read back through
    /// [`MapSdk::latest_provider_failure_diagnostics`]. The foreign-callback
    /// adapters registered here hold a handle to it.
    provider_failure_diagnostics: Arc<crate::provider_diagnostics::ProviderFailureDiagnosticsRing>,
    /// Which provider key an application chose for each capability, kept
    /// beside the registry because the registry answers only whether a slot
    /// is filled. [`Self::provider_registrations`] reads both, so a settings
    /// surface names the serving provider rather than guessing.
    provider_selections: Mutex<Vec<crate::provider_catalog::ProviderSelection>>,
    /// The in-flight offline region downloads, keyed by their region
    /// identifier. Keeping the registry on the SDK (not on the installation)
    /// means every `OfflineManager` built over the same `MapSdk` sees the same
    /// downloads; replacing the installation signals every entry to cancel so
    /// no download keeps writing into a store the SDK stopped serving. The
    /// wasm build has no disk store and no region downloads.
    #[cfg(not(target_arch = "wasm32"))]
    region_downloads: Mutex<HashMap<String, Arc<RegionDownloadCancellation>>>,
}

/// The cancellation signal of one in-flight offline region download. A
/// download races its prepare future against the receiver of this channel and
/// drops the future when the signal arrives; dropping is cancellation-safe by
/// the tile-pack manager's contract (each tile fetch is atomic, the store
/// writes are transactional, and the membership names the missing tiles).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct RegionDownloadCancellation {
    /// `true` once the download must stop. The receiver lives inside the
    /// download's `tokio::select!`; the sender lives in the SDK registry.
    cancellation_signal: tokio::sync::watch::Sender<bool>,
}

#[cfg(not(target_arch = "wasm32"))]
impl std::fmt::Debug for RegionDownloadCancellation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RegionDownloadCancellation")
            .finish_non_exhaustive()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl RegionDownloadCancellation {
    /// Builds a fresh, unsignalled cancellation channel for one download.
    fn new() -> Self {
        let (cancellation_signal, _receiver) = tokio::sync::watch::channel(false);
        Self {
            cancellation_signal,
        }
    }

    /// Subscribes a receiver the download races against its prepare future.
    fn subscribe(&self) -> tokio::sync::watch::Receiver<bool> {
        self.cancellation_signal.subscribe()
    }

    /// Signals the download to stop. Idempotent: a second signal is a no-op.
    fn signal_cancellation(&self) {
        // The receiver existing for the whole download is the normal case;
        // a receiver that already went away means the download finished and
        // there is nothing left to signal.
        let _ = self.cancellation_signal.send(true);
    }
}

/// The tile cache currently installed behind the raster-tile contract slot,
/// holding what the settings surface needs to read the effective terms and
/// to rewrap the underlying provider under a new time-to-live
/// (`rustic_maps_cache::CachedRasterTileProvider` re-wraps per override; the store's
/// quota stays fixed at open). Native-only: the wasm build has no SQLite
/// response store to install.
#[cfg(not(target_arch = "wasm32"))]
struct TileCacheInstallation {
    /// The underlying, uncached raster-tile provider the cached wrapper
    /// re-wraps on every time-to-live override.
    raster_tile_provider: Arc<dyn RasterTileProvider>,
    /// The SQLite response store the cached wrapper consults. Kept as the
    /// concrete store so the vector-tile payload cache can share it under a
    /// second typed view; the store serves any serializable record.
    response_store: Arc<rustic_maps_cache::SqliteResponseCache>,
    /// The tile payload cache counters every view of `response_store` records
    /// into: the cached raster wrapper and the vector base's payload cache.
    /// A newly installed cache starts a new pair, so the counters reset with
    /// the cache they describe.
    statistics_counters: Arc<rustic_maps_cache::TileCacheStatisticsCounters>,
    /// The install's tile-cache namespace: the catalog provider key, so
    /// two providers in one store never read each other's tiles.
    tile_source_namespace: String,
    /// Time-to-live currently in force on the installed cached wrapper, in
    /// seconds.
    time_to_live_seconds: u64,
    /// Payload-byte budget reserved for tile packs beside the general LRU
    /// cache. A [`TilePackManager`](rustic_maps_cache::TilePackManager)
    /// rebuilt from this installation enforces it; see
    /// [`rustic_maps_cache::DEFAULT_TILE_PACK_BYTE_BUDGET_BYTES`].
    tile_pack_byte_budget_bytes: u64,
    /// The installed provider's declared caching-terms ceiling in seconds;
    /// `None` when the provider declares no ceiling (an override then
    /// applies verbatim).
    provider_terms_ceiling_seconds: Option<u64>,
    /// The installed provider's declared maximum raster-tile zoom level;
    /// `None` when the provider declares none, which leaves the tile-pack
    /// manager's structural bound in force.
    raster_tile_maximum_zoom_level: Option<u8>,
}

/// Debug is implemented by hand (and only over the settings values) because
/// the installed providers and the response store do not implement `Debug`,
/// and because the `MapSdk` derive must keep printing the registry without
/// pulling the provider graph into a panic-prone `Debug` walk.
#[cfg(not(target_arch = "wasm32"))]
impl std::fmt::Debug for TileCacheInstallation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TileCacheInstallation")
            .field("time_to_live_seconds", &self.time_to_live_seconds)
            .field(
                "tile_pack_byte_budget_bytes",
                &self.tile_pack_byte_budget_bytes,
            )
            .field(
                "provider_terms_ceiling_seconds",
                &self.provider_terms_ceiling_seconds,
            )
            .field(
                "raster_tile_maximum_zoom_level",
                &self.raster_tile_maximum_zoom_level,
            )
            .finish_non_exhaustive()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl TileCacheInstallation {
    /// The installed store seen through the shared tile payload counters, as
    /// the raster-tile store view the cached wrapper and the tile-pack manager
    /// fetch through.
    fn raster_tile_response_store(
        &self,
    ) -> Arc<dyn rustic_maps_core::CachedResponseStore<geospatial_types::RasterTileData>> {
        Arc::new(rustic_maps_cache::CountingCachedResponseStore::new(
            self.response_store.clone(),
            Arc::clone(&self.statistics_counters),
        ))
    }

    /// The installed store seen through the same shared counters, as the
    /// vector-tile payload store view the vector base fetches through.
    fn vector_tile_response_store(
        &self,
    ) -> Arc<dyn rustic_maps_core::CachedResponseStore<geospatial_types::VectorTileData>> {
        Arc::new(rustic_maps_cache::CountingCachedResponseStore::new(
            self.response_store.clone(),
            Arc::clone(&self.statistics_counters),
        ))
    }

    /// Builds a tile-pack manager over this installation's current snapshot:
    /// the uncached provider wrapped in the cached pipeline under the current
    /// time-to-live, the same store, the provider's terms ceiling and its
    /// declared maximum zoom level.
    ///
    /// The manager is rebuilt per exported call rather than stored: an
    /// override of the time-to-live rewraps the cached provider, and a stored
    /// manager would keep holding the previous wrapper and its time-to-live.
    fn tile_pack_manager(&self) -> rustic_maps_cache::TilePackManager {
        let cached_raster_tile_provider = rustic_maps_cache::CachedRasterTileProvider::new(
            Arc::clone(&self.raster_tile_provider),
            self.raster_tile_response_store(),
            self.tile_source_namespace.clone(),
        )
        .with_time_to_live_seconds(self.time_to_live_seconds)
        .with_blocking_work_executor(crate::runtime::blocking_work_executor());
        let tile_pack_manager = rustic_maps_cache::TilePackManager::new(
            Arc::new(cached_raster_tile_provider),
            Arc::clone(&self.response_store),
            self.provider_terms_ceiling_seconds,
        )
        .with_pack_byte_budget_bytes(self.tile_pack_byte_budget_bytes)
        .with_blocking_work_executor(crate::runtime::blocking_work_executor());
        match self.raster_tile_maximum_zoom_level {
            Some(raster_tile_maximum_zoom_level) => {
                tile_pack_manager.with_maximum_zoom_level(raster_tile_maximum_zoom_level)
            }
            None => tile_pack_manager,
        }
    }
}

impl MapSdk {
    /// Builds the SDK with an unconfigured provider registry; the native
    /// integrator registers providers through the `register_*` methods
    /// before the first await call. The first construction also installs the
    /// process-wide diagnostics subscriber; later constructions are
    /// idempotent — see [`crate::diagnostics`].
    ///
    /// A process is constructed once on every supported platform seam; the
    /// diagnostics state is process-global by construction.
    pub fn new() -> Self {
        crate::diagnostics::DiagnosticsHandle::shared();
        // Every provider transport reports one redacted observation per
        // attempt; the bridge publishes each as a diagnostics event under the
        // transport's own target, so the existing subscription and flush
        // seams carry the provider request stream.
        rustic_maps_core::provider_observation::install_tracing_observation_bridge();
        Self {
            provider_registry: Mutex::new(ProviderRegistry::new()),
            #[cfg(not(target_arch = "wasm32"))]
            tile_cache_installation: Mutex::new(None),
            provider_failure_diagnostics: Arc::new(
                crate::provider_diagnostics::ProviderFailureDiagnosticsRing::new(),
            ),
            provider_selections: Mutex::new(Vec::new()),
            #[cfg(not(target_arch = "wasm32"))]
            region_downloads: Mutex::new(HashMap::new()),
        }
    }

    /// Records which provider key serves a capability, replacing any earlier
    /// selection for the same capability.
    pub(crate) fn record_provider_selection(
        &self,
        contract: ProviderContract,
        provider_key: &str,
        display_name: &str,
    ) {
        let mut selections = crate::panic_boundary::state_lock_guard(
            &self.provider_selections,
            "the provider selection record",
        );
        selections.retain(|selection| selection.contract != contract);
        selections.push(crate::provider_catalog::ProviderSelection {
            contract,
            provider_key: provider_key.to_string(),
            display_name: display_name.to_string(),
        });
    }

    /// Drops the recorded selection for a capability, for a call that removes
    /// the provider the selection named.
    ///
    /// The wasm surface's map view refuses every vector-base installation, so
    /// nothing records a vector selection there and the method has no caller
    /// on that target.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) fn clear_provider_selection(&self, contract: ProviderContract) {
        let mut selections = crate::panic_boundary::state_lock_guard(
            &self.provider_selections,
            "the provider selection record",
        );
        selections.retain(|selection| selection.contract != contract);
    }

    /// Chooses the provider that serves one capability.
    ///
    /// The key names a provider this build carries; the call registers that
    /// provider's implementation for the requested capability and leaves every
    /// other capability untouched, so an application can serve geocoding from
    /// one provider and routing from another. Nothing is registered until the
    /// provider constructs successfully, so a refused call leaves the
    /// previously serving provider in place.
    ///
    /// # Parameters
    /// - `contract` — the capability whose provider is being chosen.
    /// - `provider_key` — the stable key of a provider built into this SDK.
    /// - `credential` — the provider's credential; empty is accepted only for
    ///   a provider that needs none.
    ///
    /// # Errors
    /// Throws `ProviderNotConfigured` when the key is unknown to this build or
    /// serves no implementation of `contract`, and `ProviderRequestRejected`
    /// when the provider declares a credential and the supplied one is blank.
    /// A provider's own construction failure is passed through verbatim.
    pub fn configure_provider(
        &self,
        contract: ProviderContract,
        provider_key: String,
        credential: String,
    ) -> Result<(), SdkError> {
        let Some(catalog_entry) = crate::provider_catalog::catalog_entry_for_key(&provider_key)
        else {
            return Err(SdkError::ProviderNotConfigured {
                contract_name: format!("provider key {provider_key} is not built into this SDK"),
            });
        };
        if !catalog_entry.served_contracts.contains(&contract) {
            return Err(SdkError::ProviderNotConfigured {
                contract_name: crate::provider_catalog::contract_name_of(contract),
            });
        }
        let trimmed_credential = credential.trim();
        if catalog_entry.credential_required && trimmed_credential.is_empty() {
            return Err(SdkError::ProviderRequestRejected {
                detail: format!(
                    "the provider {} requires a credential before it can serve requests",
                    catalog_entry.display_name
                ),
            });
        }
        (catalog_entry.install)(self, contract, trimmed_credential)?;
        self.record_provider_selection(
            contract,
            catalog_entry.provider_key,
            catalog_entry.display_name,
        );
        Ok(())
    }

    /// Reports which provider serves each capability, in the order a settings
    /// surface lists them.
    ///
    /// Every capability appears, including the ones with no provider
    /// installed; those carry an empty key and name with `registered` false.
    /// The readback never mutates the registry.
    pub fn provider_registrations(&self) -> Vec<crate::records::ProviderRegistration> {
        let selections = crate::panic_boundary::state_lock_guard(
            &self.provider_selections,
            "the provider selection record",
        )
        .clone();
        let provider_registry = crate::panic_boundary::state_lock_guard(
            &self.provider_registry,
            "the provider registry",
        );
        rustic_maps_core::ProviderContract::ALL
            .into_iter()
            .map(|core_contract| {
                let contract = ProviderContract::from_core_contract(core_contract);
                let selection = selections
                    .iter()
                    .find(|selection| selection.contract == contract);
                crate::provider_catalog::registration_entry(
                    contract,
                    selection,
                    provider_registry.is_configured(core_contract),
                )
            })
            .collect()
    }

    /// Subscribes a listener to the diagnostics live stream: the listener
    /// receives every SDK event that meets its minimum severity as
    /// it is recorded, fire-and-forget. A subscription at
    /// [`SdkLogSeverity::Debug`] or [`SdkLogSeverity::Trace`] also lowers the
    /// sink's severity gate, which starts the GPU-hot render path's
    /// fine-grained instrumentation (the gate never raises back). Duplicate
    /// registrations deliver duplicate events (the caller's subscription
    /// lifecycle owns the listener).
    pub fn subscribe_to_diagnostic_events(
        &self,
        listener: Arc<dyn crate::diagnostics::SdkDiagnosticEventListener>,
        minimum_severity: SdkLogSeverity,
    ) {
        crate::diagnostics::DiagnosticsHandle::shared().subscribe(listener, minimum_severity);
    }

    /// Replays the most recent events the diagnostics rings recorded: at
    /// most `maximum_count` events oldest first, every
    /// subsystem's ring interleaved in recording order, filtered by
    /// `minimum_severity`. The flush is non-destructive — the rings are
    /// never emptied, so a later flush can redeliver the same events.
    /// Delivered events are fired-and-forget, never awaited.
    pub fn flush_recent_diagnostic_events(
        &self,
        listener: Arc<dyn crate::diagnostics::SdkDiagnosticEventListener>,
        minimum_severity: SdkLogSeverity,
        maximum_count: u32,
    ) -> u32 {
        let recent_events = crate::diagnostics::DiagnosticsHandle::shared()
            .recent_events(minimum_severity, maximum_count);
        let flush_count = recent_events.len() as u32;
        for event in recent_events {
            listener.on_diagnostic_event(event);
        }
        flush_count
    }

    /// Enables or disables the bounded, redacted response prefix carried by
    /// provider request observations.
    ///
    /// Off by default. While it is on, each provider request observation
    /// carries the first 512 bytes of the response body in
    /// `ProviderRequestTrace.response_excerpt`, with credential-bearing JSON
    /// members and assignments redacted. The switch is process-wide.
    pub fn set_response_excerpt_capture(&self, enabled: bool) {
        rustic_maps_core::provider_observation::set_response_excerpt_capture(enabled);
    }

    /// Reads the retained diagnostics of every failed foreign-callback
    /// provider call this object has served, oldest first — the failures
    /// registered foreign
    /// implementations of the `[Trait, WithForeign]` provider contracts
    /// threw, both the typed ones (their own `SdkError` throws) and the
    /// unclassified ones (failures without a mapped error, reported at the
    /// throwing call under `ProviderRequestRejected`). The readback is
    /// non-destructive and never mutates the ring; the ring is bounded
    /// (retaining the newest `ProviderFailureDiagnostic::
    /// PROVIDER_FAILURE_DIAGNOSTIC_RING_CAPACITY` capacity).
    /// Native-registered providers do not surface here: the seam observes
    /// the foreign callback surface alone.
    pub fn latest_provider_failure_diagnostics(
        &self,
    ) -> Vec<crate::records::ProviderFailureDiagnostic> {
        self.provider_failure_diagnostics.latest()
    }

    /// Registers the geocoding implementation on the native integrator path
    /// (the UDL surface carries no provider parameters on this path).
    pub fn register_geocoding_provider(&self, geocoding_provider: Arc<dyn GeocodingProvider>) {
        crate::panic_boundary::state_lock_guard(
            &self.provider_registry,
            "the provider registry's contract slots",
        )
        .register_geocoding_provider(geocoding_provider);
    }

    /// Registers the geocoding implementation coming from the foreign code
    /// implementing the exported [`crate::foreign_providers::GeocodingProvider`]
    /// callback contract: the foreign implementation is wrapped in the
    /// adapter bridging it to the native contract, then registered on the
    /// same registry slot as the native path. The adapter records every
    /// failed contract call on this object's provider-failure diagnostic
    /// ring (read back through
    /// [`MapSdk::latest_provider_failure_diagnostics`]).
    pub fn register_geocoding_provider_from_foreign(
        &self,
        foreign_geocoding_provider: Arc<dyn crate::foreign_providers::GeocodingProvider>,
    ) {
        self.record_provider_selection(
            ProviderContract::Geocoding,
            APPLICATION_PROVIDER_KEY,
            APPLICATION_PROVIDER_DISPLAY_NAME,
        );
        self.register_geocoding_provider(Arc::new(ForeignGeocodingProvider::new(
            foreign_geocoding_provider,
            Arc::clone(&self.provider_failure_diagnostics),
        )));
    }

    /// Registers the routing implementation on the native integrator path
    /// (the UDL surface carries no provider parameters).
    pub fn register_routing_provider(&self, routing_provider: Arc<dyn RoutingProvider>) {
        crate::panic_boundary::state_lock_guard(
            &self.provider_registry,
            "the provider registry's contract slots",
        )
        .register_routing_provider(routing_provider);
    }

    /// Registers the distance-matrix implementation on the native integrator
    /// path (the UDL surface carries no provider parameters).
    pub fn register_distance_matrix_provider(
        &self,
        distance_matrix_provider: Arc<dyn DistanceMatrixProvider>,
    ) {
        crate::panic_boundary::state_lock_guard(
            &self.provider_registry,
            "the provider registry's contract slots",
        )
        .register_distance_matrix_provider(distance_matrix_provider);
    }

    /// Registers the place-search implementation on the native integrator
    /// path (the UDL surface carries no provider parameters).
    pub fn register_search_place_provider(
        &self,
        search_place_provider: Arc<dyn SearchPlaceProvider>,
    ) {
        crate::panic_boundary::state_lock_guard(
            &self.provider_registry,
            "the provider registry's contract slots",
        )
        .register_search_place_provider(search_place_provider);
    }

    /// Registers the traffic-incident implementation on the native
    /// integrator path (the UDL surface carries no provider
    /// parameters).
    pub fn register_traffic_incident_provider(
        &self,
        traffic_incident_provider: Arc<dyn TrafficIncidentProvider>,
    ) {
        crate::panic_boundary::state_lock_guard(
            &self.provider_registry,
            "the provider registry's contract slots",
        )
        .register_traffic_incident_provider(traffic_incident_provider);
    }

    /// Registers the traffic-flow implementation on the native integrator
    /// path (the UDL surface carries no provider parameters).
    ///
    /// The registered provider is wrapped in the grid-keyed, memory-bounded
    /// traffic cache: a traffic layer asks for the segments under its camera
    /// on every settle, and the cache answers a pane it already holds instead
    /// of asking the provider again. The wrapper is transparent — the same
    /// segments, the same failures — and a re-registration replaces both the
    /// provider and its cache.
    pub fn register_traffic_flow_provider(
        &self,
        traffic_flow_provider: Arc<dyn TrafficFlowProvider>,
    ) {
        let traffic_flow_current_time_seconds: rustic_maps_core::TrafficFlowCurrentTimeSeconds =
            Arc::new(rustic_maps_core::system_clock_time_seconds);
        let provider_for_cache = Arc::clone(&traffic_flow_provider);
        let mut provider_registry = crate::panic_boundary::state_lock_guard(
            &self.provider_registry,
            "the provider registry's contract slots",
        );
        match rustic_maps_core::GridCachedTrafficFlowProvider::new(
            provider_for_cache,
            rustic_maps_core::TrafficFlowCacheConfiguration::default(),
            traffic_flow_current_time_seconds,
        ) {
            Ok(cached_traffic_flow_provider) => {
                // The registry keeps the cache handle beside the wrapped
                // provider, so the statistics readback can report the
                // counters the provider is actually answered from.
                let traffic_flow_cache = cached_traffic_flow_provider.cache();
                provider_registry.register_traffic_flow_provider_with_cache(
                    Arc::new(cached_traffic_flow_provider),
                    traffic_flow_cache,
                );
            }
            // The default configuration is valid by construction, so this
            // arm is unreachable in practice; serving uncached is the
            // honest fallback if it ever is not.
            Err(cache_configuration_error) => {
                tracing::warn!(
                    target: "rustic_maps::providers",
                    error = %cache_configuration_error,
                    "the traffic flow cache configuration was refused; the provider serves uncached"
                );
                provider_registry.register_traffic_flow_provider(traffic_flow_provider);
            }
        }
    }

    /// Reads the serving traffic-flow cache's counters and budget: the hits,
    /// misses, held cells and evictions, beside the grid zoom, entry budget
    /// and time-to-live the cache enforces. `None` while no traffic-flow
    /// provider is registered or the registered provider owns its own
    /// freshness policy and answers uncached.
    pub fn traffic_flow_cache_statistics(
        &self,
    ) -> Option<crate::records::TrafficFlowCacheStatistics> {
        crate::panic_boundary::state_lock_guard(
            &self.provider_registry,
            "the provider registry's contract slots",
        )
        .traffic_flow_cache_statistics()
        .map(crate::records::TrafficFlowCacheStatistics::from)
    }

    /// Registers the elevation implementation on the native integrator path
    /// (the UDL surface carries no provider parameters on this path).
    pub fn register_elevation_provider(&self, elevation_provider: Arc<dyn ElevationProvider>) {
        crate::panic_boundary::state_lock_guard(
            &self.provider_registry,
            "the provider registry's contract slots",
        )
        .register_elevation_provider(elevation_provider);
    }

    /// Registers the raster-tile implementation on the native integrator
    /// path (the UDL surface carries no provider parameters on this
    /// path).
    ///
    /// On wasm the hand-written canvas ships in this crate's module, so the
    /// registration is mirrored into the canvas' shared slot: a source
    /// registered here (a URL template through the contract, a credential, a
    /// foreign callback) is also the canvas' live tile fallback. The native
    /// registry stays the single owner of the slot; the mirror is a clone of
    /// the same provider handle.
    pub fn register_raster_tile_provider(&self, raster_tile_provider: Arc<dyn RasterTileProvider>) {
        #[cfg(target_arch = "wasm32")]
        rustic_maps_web_bindings::live_providers::set_shared_raster_tile_provider(Some(
            Arc::clone(&raster_tile_provider),
        ));
        crate::panic_boundary::state_lock_guard(
            &self.provider_registry,
            "the provider registry's contract slots",
        )
        .register_raster_tile_provider(raster_tile_provider);
    }

    /// Registers a vector-tile implementation on the registry's vector-tile
    /// slot, replacing whichever source served it before.
    ///
    /// The view's provider-backed base resolves the registered implementation
    /// through this slot; a provider-backed vector family joins a build by
    /// being registered here exactly as a raster provider joins by
    /// [`Self::register_raster_tile_provider`].
    pub fn register_vector_tile_provider(&self, vector_tile_provider: Arc<dyn VectorTileProvider>) {
        // The browser canvas keeps its own registry; mirroring the handle lets
        // a provider-backed base installed through this surface resolve to the
        // same source the canvas draws, exactly as the raster registration
        // does.
        #[cfg(target_arch = "wasm32")]
        {
            rustic_maps_web_bindings::live_providers::set_shared_vector_tile_provider(Some(
                Arc::clone(&vector_tile_provider),
            ));
            // The new provider's fetch is registered right after this call;
            // mirroring the clear keeps the canvas from serving the previous
            // provider's credential in between.
            rustic_maps_web_bindings::live_providers::set_shared_vector_style_resources(
                None,
                Vec::new(),
            );
        }
        crate::panic_boundary::state_lock_guard(
            &self.provider_registry,
            "the provider registry's contract slots",
        )
        .register_vector_tile_provider(vector_tile_provider);
    }

    /// Registers the authenticated fetch of the vector-tile provider's
    /// published style resources along with the provider itself, so a
    /// provider-backed base can paint the provider's own style document
    /// instead of a committed style.
    ///
    /// The fetch holds no credential of its own: the provider client carries
    /// the credential and applies it to whatever address the base names.
    ///
    /// # Parameters
    /// - `style_resources` — the authenticated fetch of the style document,
    ///   glyph ranges and sprite sheets the provider publishes.
    /// - `declared_hosts` — the hosts the provider publishes those resources
    ///   on, besides the style document's own origin. The renderer refuses a
    ///   document that names any other host before a request is made.
    pub fn register_vector_tile_style_resources(
        &self,
        style_resources: Arc<dyn StyleResourceClient>,
        declared_hosts: Vec<String>,
    ) {
        // The browser canvas keeps its own registry; mirroring the handle lets
        // a provider-backed base installed through this surface resolve the
        // same style-resource fetch the canvas draws through.
        #[cfg(target_arch = "wasm32")]
        rustic_maps_web_bindings::live_providers::set_shared_vector_style_resources(
            Some(Arc::clone(&style_resources)),
            declared_hosts.clone(),
        );
        crate::panic_boundary::state_lock_guard(
            &self.provider_registry,
            "the provider registry's contract slots",
        )
        .register_vector_tile_style_resources(style_resources, declared_hosts);
    }

    /// Registers the raster-tile implementation coming from the foreign code
    /// implementing the exported
    /// [`crate::foreign_providers::RasterTileProvider`] callback contract:
    /// the foreign implementation is wrapped in the adapter bridging it to
    /// the native contract, then registered on the same registry slot as the
    /// native path.
    pub fn register_raster_tile_provider_from_foreign(
        &self,
        foreign_raster_tile_provider: Arc<dyn crate::foreign_providers::RasterTileProvider>,
    ) {
        self.record_provider_selection(
            ProviderContract::RasterTiles,
            APPLICATION_PROVIDER_KEY,
            APPLICATION_PROVIDER_DISPLAY_NAME,
        );
        self.register_raster_tile_provider(Arc::new(ForeignRasterTileProvider::new(
            foreign_raster_tile_provider,
            Arc::clone(&self.provider_failure_diagnostics),
        )));
    }

    /// Registers the vector-tile implementation coming from the foreign code
    /// implementing the exported
    /// [`crate::foreign_providers::VectorTileProvider`] callback contract:
    /// the foreign implementation is wrapped in the adapter bridging it to
    /// the native contract, then registered on the same registry slot as the
    /// native path. The selection is recorded under the application key, so
    /// `MapView::set_provider_vector_tile_source` resolving that key installs
    /// a base from this provider.
    pub fn register_vector_tile_provider_from_foreign(
        &self,
        foreign_vector_tile_provider: Arc<dyn crate::foreign_providers::VectorTileProvider>,
    ) {
        self.record_provider_selection(
            ProviderContract::VectorTiles,
            APPLICATION_PROVIDER_KEY,
            APPLICATION_PROVIDER_DISPLAY_NAME,
        );
        self.register_vector_tile_provider(Arc::new(ForeignVectorTileProvider::new(
            foreign_vector_tile_provider,
            Arc::clone(&self.provider_failure_diagnostics),
        )));
    }

    /// Reports whether a vector-tile provider is registered. A foreign
    /// provider registered through
    /// [`Self::register_vector_tile_provider_from_foreign`] fills the slot
    /// before that call returns, so the readback answers whether a base can
    /// resolve one.
    pub fn vector_tile_provider_is_registered(&self) -> bool {
        crate::panic_boundary::state_lock_guard(
            &self.provider_registry,
            "the provider registry's contract slots",
        )
        .require_vector_tile_provider()
        .is_ok()
    }

    /// Registers a routing implementation written in application code: the
    /// adapter bridges it to the native routing contract and registers it on
    /// the routing slot, replacing whichever provider served routing before.
    /// Every failed call is recorded on this object's provider-failure
    /// diagnostic ring.
    pub fn register_routing_provider_from_foreign(
        &self,
        foreign_routing_provider: Arc<dyn crate::foreign_providers::RoutingProvider>,
    ) {
        self.record_provider_selection(
            ProviderContract::Routing,
            APPLICATION_PROVIDER_KEY,
            APPLICATION_PROVIDER_DISPLAY_NAME,
        );
        self.register_routing_provider(Arc::new(ForeignRoutingProvider::new(
            foreign_routing_provider,
            Arc::clone(&self.provider_failure_diagnostics),
        )));
    }

    /// Registers a distance-matrix implementation written in application
    /// code on the distance-matrix slot, replacing the previous provider.
    pub fn register_distance_matrix_provider_from_foreign(
        &self,
        foreign_distance_matrix_provider: Arc<dyn crate::foreign_providers::DistanceMatrixProvider>,
    ) {
        self.record_provider_selection(
            ProviderContract::DistanceMatrix,
            APPLICATION_PROVIDER_KEY,
            APPLICATION_PROVIDER_DISPLAY_NAME,
        );
        self.register_distance_matrix_provider(Arc::new(ForeignDistanceMatrixProvider::new(
            foreign_distance_matrix_provider,
            Arc::clone(&self.provider_failure_diagnostics),
        )));
    }

    /// Registers a place-search implementation written in application code on
    /// the place-search slot, replacing the previous provider.
    pub fn register_search_place_provider_from_foreign(
        &self,
        foreign_search_place_provider: Arc<dyn crate::foreign_providers::SearchPlaceProvider>,
    ) {
        self.record_provider_selection(
            ProviderContract::PlaceSearch,
            APPLICATION_PROVIDER_KEY,
            APPLICATION_PROVIDER_DISPLAY_NAME,
        );
        self.register_search_place_provider(Arc::new(ForeignSearchPlaceProvider::new(
            foreign_search_place_provider,
            Arc::clone(&self.provider_failure_diagnostics),
        )));
    }

    /// Registers a traffic-incident implementation written in application
    /// code on the traffic-incident slot, replacing the previous provider.
    pub fn register_traffic_incident_provider_from_foreign(
        &self,
        foreign_traffic_incident_provider: Arc<
            dyn crate::foreign_providers::TrafficIncidentProvider,
        >,
    ) {
        self.record_provider_selection(
            ProviderContract::TrafficIncidents,
            APPLICATION_PROVIDER_KEY,
            APPLICATION_PROVIDER_DISPLAY_NAME,
        );
        self.register_traffic_incident_provider(Arc::new(ForeignTrafficIncidentProvider::new(
            foreign_traffic_incident_provider,
            Arc::clone(&self.provider_failure_diagnostics),
        )));
    }

    /// Registers a traffic-flow implementation written in application code on
    /// the traffic-flow slot, replacing the previous provider.
    ///
    /// A host-supplied implementation is served as-is, without the SDK's
    /// grid-keyed traffic cache. That cache is the SDK's own freshness policy
    /// for the providers it installs; a host implementation owns its
    /// freshness and failure reporting, so wrapping it would make a
    /// host-visible refresh indistinguishable from a provider answer. The
    /// catalog-installed provider path keeps the cache because it is the SDK's
    /// own implementation.
    pub fn register_traffic_flow_provider_from_foreign(
        &self,
        foreign_traffic_flow_provider: Arc<dyn crate::foreign_providers::TrafficFlowProvider>,
    ) {
        self.record_provider_selection(
            ProviderContract::TrafficFlow,
            APPLICATION_PROVIDER_KEY,
            APPLICATION_PROVIDER_DISPLAY_NAME,
        );
        crate::panic_boundary::state_lock_guard(
            &self.provider_registry,
            "the provider registry's contract slots",
        )
        .register_traffic_flow_provider(Arc::new(ForeignTrafficFlowProvider::new(
            foreign_traffic_flow_provider,
            Arc::clone(&self.provider_failure_diagnostics),
        )));
    }

    /// Registers an elevation implementation written in application code on
    /// the elevation slot, replacing the previous provider.
    pub fn register_elevation_provider_from_foreign(
        &self,
        foreign_elevation_provider: Arc<dyn crate::foreign_providers::ElevationProvider>,
    ) {
        self.record_provider_selection(
            ProviderContract::Elevation,
            APPLICATION_PROVIDER_KEY,
            APPLICATION_PROVIDER_DISPLAY_NAME,
        );
        self.register_elevation_provider(Arc::new(ForeignElevationProvider::new(
            foreign_elevation_provider,
            Arc::clone(&self.provider_failure_diagnostics),
        )));
    }

    /// Registers every provider contract the catalog entry named by
    /// `provider_key` serves from one credential — the whole-set form of
    /// [`Self::configure_provider`], so one call installs every capability of
    /// the chosen provider instead of one capability at a time.
    ///
    /// The key must name a provider this build carries. Every construction
    /// failure throws with its verbatim detail; re-registration replaces the
    /// slots. A provider that requires a credential refuses a blank one before
    /// anything is registered, and the call leaves every previously registered
    /// provider in place on failure. Elevation stays unregistered for a
    /// provider that serves no elevation contract, so elevation calls keep
    /// throwing `ProviderNotConfigured`.
    ///
    /// The raster-tile contract is installed uncached, so any earlier tile-cache
    /// installation and its terms are dropped; use
    /// [`Self::set_provider_credential_with_tile_cache`] to keep one.
    pub fn configure_provider_set(
        &self,
        provider_key: String,
        credential: String,
    ) -> Result<(), SdkError> {
        let Some(catalog_entry) = crate::provider_catalog::catalog_entry_for_key(&provider_key)
        else {
            return Err(SdkError::ProviderNotConfigured {
                contract_name: format!("provider key {provider_key} is not built into this SDK"),
            });
        };
        let trimmed_credential = credential.trim();
        if catalog_entry.credential_required && trimmed_credential.is_empty() {
            return Err(SdkError::ProviderRequestRejected {
                detail: format!(
                    "the provider {} requires a credential before it can serve requests",
                    catalog_entry.display_name
                ),
            });
        }
        for contract in catalog_entry.served_contracts {
            (catalog_entry.install)(self, *contract, trimmed_credential)?;
        }
        for contract in catalog_entry.served_contracts {
            self.record_provider_selection(
                *contract,
                catalog_entry.provider_key,
                catalog_entry.display_name,
            );
        }
        // The set registration serves the raster-tile contract uncached, so
        // any earlier cache installation and its terms are dropped.
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.cancel_all_region_downloads();
            *crate::panic_boundary::state_lock_guard(
                &self.tile_cache_installation,
                "the tile-cache installation state",
            ) = None;
        }
        Ok(())
    }

    /// Registers every provider contract the catalog entry named by
    /// `provider_key` serves from one credential, exactly as
    /// [`Self::configure_provider_set`] does for a whole provider set,
    /// wrapping the raster-tile contract in the disk response cache opened
    /// (or created) as a SQLite database at `tile_cache_database_path` under
    /// an absolute byte budget of `tile_cache_absolute_maximum_bytes`.
    /// Cached tile responses follow the tiles category's time-to-live (thirty
    /// days by default, clamped to the catalog entry's declared caching-terms
    /// ceiling, applied as
    /// [`rustic_maps_cache::ResponseCategory::RasterTiles`]'s default); a cache
    /// miss goes to the network and back-fills the cache, so pre-warmed
    /// tiles keep rendering whenever the network is unavailable.
    ///
    /// The key must name a provider built into this build that serves the
    /// raster-tile contract and declares caching terms, the path must be
    /// non-empty and the byte budget positive, and a provider that requires a
    /// credential must receive a non-blank one; each is refused with its typed
    /// error. A cache open or quota failure throws with its verbatim detail.
    /// Re-registration replaces the slots (including the cached raster-tile
    /// slot).
    ///
    /// The disk store is native-only: a wasm build has no SQLite response
    /// store, so the call validates its inputs and then throws
    /// [`SdkError::CacheOperationFailure`] with that target limitation.
    pub fn set_provider_credential_with_tile_cache(
        &self,
        provider_key: String,
        credential: String,
        tile_cache_database_path: String,
        tile_cache_absolute_maximum_bytes: u64,
    ) -> Result<(), SdkError> {
        if tile_cache_database_path.is_empty() {
            return Err(SdkError::InvalidCoordinateInput {
                detail: "the tile cache database path must be non-empty".to_string(),
            });
        }
        if tile_cache_absolute_maximum_bytes == 0 {
            return Err(SdkError::InvalidCoordinateInput {
                detail: "the tile cache byte budget must be positive".to_string(),
            });
        }
        let Some(catalog_entry) = crate::provider_catalog::catalog_entry_for_key(&provider_key)
        else {
            return Err(SdkError::ProviderNotConfigured {
                contract_name: format!("provider key {provider_key} is not built into this SDK"),
            });
        };
        if !catalog_entry
            .served_contracts
            .contains(&ProviderContract::RasterTiles)
        {
            return Err(SdkError::ProviderRequestRejected {
                detail: format!(
                    "the provider {} serves no raster tiles, so it cannot install a tile cache",
                    catalog_entry.display_name
                ),
            });
        }
        let Some(provider_terms_ceiling_seconds) = catalog_entry.raster_tile_cache_ceiling_seconds
        else {
            return Err(SdkError::ProviderRequestRejected {
                detail: format!(
                    "the provider {} declares no raster-tile caching terms, so it cannot \
                     install a tile cache",
                    catalog_entry.display_name
                ),
            });
        };
        let trimmed_credential = credential.trim();
        if catalog_entry.credential_required && trimmed_credential.is_empty() {
            return Err(SdkError::ProviderRequestRejected {
                detail: format!(
                    "the provider {} requires a credential before it can serve requests",
                    catalog_entry.display_name
                ),
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let tile_cache_response_store = rustic_maps_cache::SqliteResponseCache::open(
                rustic_maps_cache::SqliteResponseCacheConfiguration {
                    database_path: std::path::PathBuf::from(tile_cache_database_path),
                    quota: rustic_maps_cache::CacheQuota {
                        absolute_maximum_bytes: Some(tile_cache_absolute_maximum_bytes),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .map_err(SdkError::from)?;
            for contract in catalog_entry.served_contracts {
                (catalog_entry.install)(self, *contract, trimmed_credential)?;
            }
            let uncached_raster_tile_provider = self.required_raster_tile_provider()?;
            let default_time_to_live_seconds =
                rustic_maps_cache::ResponseCategory::RasterTiles.default_time_to_live_seconds();
            let clamped_time_to_live_seconds = clamped_to_provider_terms_ceiling(
                default_time_to_live_seconds,
                Some(provider_terms_ceiling_seconds),
            );
            // One counter pair serves every view of this cache: the cached
            // raster wrapper and the vector base's payload store both count
            // their lookups into it.
            let statistics_counters = rustic_maps_cache::TileCacheStatisticsCounters::shared();
            let counting_raster_tile_response_store: Arc<
                dyn rustic_maps_core::CachedResponseStore<geospatial_types::RasterTileData>,
            > = Arc::new(rustic_maps_cache::CountingCachedResponseStore::new(
                tile_cache_response_store.clone(),
                Arc::clone(&statistics_counters),
            ));
            self.register_raster_tile_provider(Arc::new(
                rustic_maps_cache::CachedRasterTileProvider::new(
                    Arc::clone(&uncached_raster_tile_provider),
                    counting_raster_tile_response_store,
                    catalog_entry.provider_key,
                )
                .with_time_to_live_seconds(clamped_time_to_live_seconds)
                .with_blocking_work_executor(crate::runtime::blocking_work_executor()),
            ));
            for contract in catalog_entry.served_contracts {
                self.record_provider_selection(
                    *contract,
                    catalog_entry.provider_key,
                    catalog_entry.display_name,
                );
            }
            // A replaced installation never keeps serving an in-flight pack
            // download: signal every download before the swap so none keeps
            // writing into the store the SDK is about to stop serving.
            self.cancel_all_region_downloads();
            *crate::panic_boundary::state_lock_guard(
                &self.tile_cache_installation,
                "the tile-cache installation state",
            ) = Some(TileCacheInstallation {
                raster_tile_provider: uncached_raster_tile_provider,
                response_store: tile_cache_response_store,
                statistics_counters,
                tile_source_namespace: catalog_entry.provider_key.to_string(),
                time_to_live_seconds: clamped_time_to_live_seconds,
                tile_pack_byte_budget_bytes: rustic_maps_cache::DEFAULT_TILE_PACK_BYTE_BUDGET_BYTES,
                provider_terms_ceiling_seconds: Some(provider_terms_ceiling_seconds),
                raster_tile_maximum_zoom_level: catalog_entry.raster_tile_maximum_zoom_level,
            });
            Ok(())
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (
                trimmed_credential,
                catalog_entry,
                provider_terms_ceiling_seconds,
            );
            Err(SdkError::CacheOperationFailure {
                detail: "the disk tile cache is unavailable on this target; \
                         a wasm build has no SQLite response store"
                    .to_string(),
            })
        }
    }

    /// Registers a provider-neutral base map: any web-mercator XYZ raster
    /// tileset that publishes a
    /// `{z}/{x}/{y}` URL template, served by the SDK itself with no provider
    /// crate involved — the foreign mirror of the native integrator's
    /// `register_raster_tile_provider` call, so an application offers a
    /// second base map without a new binding.
    ///
    /// The registration replaces the raster-tile slot only — every other
    /// provider slot keeps serving — and drops any installed tile cache
    /// (the cache's terms belong to the provider that installed it, so the
    /// settings surface afterwards reports the honest `TileCache`
    /// not-configured error instead of another provider's ceiling). Every
    /// declaration is validated before the slot moves, so a refused call
    /// leaves the previously served provider in place.
    pub fn register_url_template_raster_tile_source(
        &self,
        source: UrlTemplateRasterTileSource,
    ) -> Result<(), SdkError> {
        let source_name = source.source_name.clone();
        let provider_source = source.into_provider_source()?;
        let url_template_raster_tile_provider =
            everymap_providers_url_template_raster::UrlTemplateRasterTileProvider::new(
                provider_source,
            )
            .map_err(|error| {
                SdkError::from(rustic_maps_core::everymap_bridge::map_provider_error(error))
            })?;
        self.record_provider_selection(ProviderContract::RasterTiles, &source_name, &source_name);
        self.register_raster_tile_provider(Arc::new(
            rustic_maps_core::everymap_bridge::EveryMapRasterTileProvider::new(Arc::new(
                url_template_raster_tile_provider,
            )),
        ));
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.cancel_all_region_downloads();
            *crate::panic_boundary::state_lock_guard(
                &self.tile_cache_installation,
                "the tile-cache installation state",
            ) = None;
        }
        Ok(())
    }

    /// Overrides the installed raster-tile cache's time-to-live, in seconds,
    /// re-wrapping the cached provider under the requested value through
    /// [`rustic_maps_cache::CachedRasterTileProvider::with_time_to_live_seconds`]. A
    /// value above the installed provider's declared terms ceiling is
    /// accepted with the ceiling clamping what the cache stores (the ceiling
    /// bounds what the cache may retain, never a preference);
    /// [`Self::raster_tile_cache_terms`] reports the effective value. A zero
    /// value is refused: a zero time-to-live expires every entry on capture,
    /// a cache that stores nothing. Requires a tile cache installed through
    /// [`Self::set_provider_credential_with_tile_cache`] — the
    /// byte budget is registration-time (the store's quota is fixed at open)
    /// and retunes only through re-registration.
    pub fn set_raster_tile_cache_time_to_live(
        &self,
        time_to_live_seconds: u64,
    ) -> Result<(), SdkError> {
        if time_to_live_seconds == 0 {
            return Err(SdkError::InvalidCoordinateInput {
                detail: "the raster-tile cache time-to-live must be positive (seconds)".to_string(),
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (rebuilt_raster_tile_provider, clamped_time_to_live_seconds) = {
                let installation = crate::panic_boundary::state_lock_guard(
                    &self.tile_cache_installation,
                    "the tile-cache installation state",
                );
                let tile_cache_installation =
                    installation
                        .as_ref()
                        .ok_or(SdkError::ProviderNotConfigured {
                            contract_name: TILE_CACHE_NOT_CONFIGURED_CONTRACT_NAME.to_string(),
                        })?;
                let clamped_time_to_live_seconds = clamped_to_provider_terms_ceiling(
                    time_to_live_seconds,
                    tile_cache_installation.provider_terms_ceiling_seconds,
                );
                (
                    Arc::new(
                        rustic_maps_cache::CachedRasterTileProvider::new(
                            tile_cache_installation.raster_tile_provider.clone(),
                            tile_cache_installation.raster_tile_response_store(),
                            tile_cache_installation.tile_source_namespace.clone(),
                        )
                        .with_time_to_live_seconds(clamped_time_to_live_seconds)
                        .with_blocking_work_executor(crate::runtime::blocking_work_executor()),
                    ),
                    clamped_time_to_live_seconds,
                )
            };
            // The rebuild and the registry write stay outside the
            // installation lock: the registry's guard is never taken while
            // the installation's is held, and vice versa, so the two locks
            // cannot dead lock.
            self.register_raster_tile_provider(rebuilt_raster_tile_provider);
            if let Some(tile_cache_installation) = crate::panic_boundary::state_lock_guard(
                &self.tile_cache_installation,
                "the tile-cache installation state",
            )
            .as_mut()
            {
                tile_cache_installation.time_to_live_seconds = clamped_time_to_live_seconds;
            }
            Ok(())
        }
        // No installable disk cache exists on wasm, so the native
        // not-configured refusal is the only outcome there.
        #[cfg(target_arch = "wasm32")]
        {
            Err(SdkError::ProviderNotConfigured {
                contract_name: TILE_CACHE_NOT_CONFIGURED_CONTRACT_NAME.to_string(),
            })
        }
    }

    /// Reads the effective raster-tile caching terms so an application can
    /// display them — the value currently
    /// in force (the category default, or the last accepted override,
    /// clamped when it exceeded the provider's terms ceiling), the
    /// provider's declared ceiling (`None` when the provider declares no
    /// ceiling; the requested value then applies verbatim), and the
    /// category default a freshly registered cache starts at. Requires a
    /// tile cache installed through
    /// [`Self::set_provider_credential_with_tile_cache`]; the readback
    /// never mutates the cache.
    pub fn raster_tile_cache_terms(&self) -> Result<TileCacheTerms, SdkError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let tile_cache_installation_lock = crate::panic_boundary::state_lock_guard(
                &self.tile_cache_installation,
                "the tile-cache installation state",
            );
            let tile_cache_installation =
                tile_cache_installation_lock
                    .as_ref()
                    .ok_or(SdkError::ProviderNotConfigured {
                        contract_name: TILE_CACHE_NOT_CONFIGURED_CONTRACT_NAME.to_string(),
                    })?;
            Ok(TileCacheTerms {
                effective_time_to_live_seconds: tile_cache_installation.time_to_live_seconds,
                provider_terms_ceiling_seconds: tile_cache_installation
                    .provider_terms_ceiling_seconds,
                default_time_to_live_seconds: rustic_maps_cache::ResponseCategory::RasterTiles
                    .default_time_to_live_seconds(),
            })
        }
        // No installable disk cache exists on wasm, so the native
        // not-configured refusal is the only outcome there.
        #[cfg(target_arch = "wasm32")]
        {
            Err(SdkError::ProviderNotConfigured {
                contract_name: TILE_CACHE_NOT_CONFIGURED_CONTRACT_NAME.to_string(),
            })
        }
    }

    /// Reads the installed tile payload cache's exact counters and occupancy,
    /// for a diagnostics panel or a developer console. `None` while no tile
    /// cache is installed, which is every wasm build and a keyless native
    /// session.
    ///
    /// Both the raster base and the vector base fetch through the same
    /// installed store, so one counter pair covers every payload lookup: a
    /// hit was answered without a provider request and a miss went to the
    /// provider. The byte occupancy is the store's current payload size, or
    /// `None` when the store could not be read; the counters stand either
    /// way. Read-only: the readback never mutates the cache.
    pub fn tile_cache_statistics(&self) -> Option<crate::records::TileCacheStatistics> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let tile_cache_installation_lock = crate::panic_boundary::state_lock_guard(
                &self.tile_cache_installation,
                "the tile-cache installation state",
            );
            tile_cache_installation_lock
                .as_ref()
                .map(
                    |tile_cache_installation| crate::records::TileCacheStatistics {
                        stored_byte_count: tile_cache_installation
                            .response_store
                            .total_stored_bytes()
                            .ok(),
                        ..crate::records::TileCacheStatistics::from(
                            tile_cache_installation.statistics_counters.statistics(),
                        )
                    },
                )
        }
        // No installable disk cache exists on wasm, so there is nothing to
        // report there.
        #[cfg(target_arch = "wasm32")]
        {
            None
        }
    }

    /// Reserves a payload-byte budget for tile packs beside the general LRU
    /// cache. Every tile-pack manager rebuilt from the installed cache
    /// enforces the value, so a download past the budget fails with the
    /// cache's `WriteRejected` failure, keeps the partial pack and resumes
    /// under a raised budget. The structural
    /// [`rustic_maps_cache::MAXIMUM_TILE_PACK_TILE_COUNT`] cap remains the
    /// only tile-count limit. A zero budget is refused: it would refuse every
    /// pack download.
    ///
    /// Requires a tile cache installed through
    /// [`Self::set_provider_credential_with_tile_cache`].
    pub(crate) fn set_tile_pack_byte_budget(&self, byte_budget: u64) -> Result<(), SdkError> {
        if byte_budget == 0 {
            return Err(SdkError::InvalidCoordinateInput {
                detail: "the tile-pack byte budget must be positive (bytes)".to_string(),
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut tile_cache_installation_lock = crate::panic_boundary::state_lock_guard(
                &self.tile_cache_installation,
                "the tile-cache installation state",
            );
            let tile_cache_installation = tile_cache_installation_lock
                .as_mut()
                .ok_or_else(tile_cache_not_configured_error)?;
            tile_cache_installation.tile_pack_byte_budget_bytes = byte_budget;
            Ok(())
        }
        // No installable disk cache exists on wasm, so the native
        // not-configured refusal is the only outcome there.
        #[cfg(target_arch = "wasm32")]
        {
            let _ = byte_budget;
            Err(tile_cache_not_configured_error())
        }
    }

    /// Reads the effective payload-byte budget reserved for tile packs; the
    /// readback never mutates the cache. Requires a tile cache installed
    /// through [`Self::set_provider_credential_with_tile_cache`].
    pub(crate) fn tile_pack_byte_budget(&self) -> Result<u64, SdkError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let tile_cache_installation_lock = crate::panic_boundary::state_lock_guard(
                &self.tile_cache_installation,
                "the tile-cache installation state",
            );
            tile_cache_installation_lock
                .as_ref()
                .map(|tile_cache_installation| tile_cache_installation.tile_pack_byte_budget_bytes)
                .ok_or_else(tile_cache_not_configured_error)
        }
        // No installable disk cache exists on wasm, so the native
        // not-configured refusal is the only outcome there.
        #[cfg(target_arch = "wasm32")]
        {
            Err(tile_cache_not_configured_error())
        }
    }

    /// The installed tile cache's response store, seen as the vector-tile
    /// payload store the base map consults, or `None` while no tile cache is
    /// installed. The store serves every serializable response category, so
    /// vector and raster payloads share one database, quota and eviction
    /// policy; a build with no installed cache fetches vector tiles uncached.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn vector_tile_payload_cache_store(
        &self,
    ) -> Option<Arc<dyn rustic_maps_core::CachedResponseStore<geospatial_types::VectorTileData>>>
    {
        let tile_cache_installation_lock = crate::panic_boundary::state_lock_guard(
            &self.tile_cache_installation,
            "the tile-cache installation state",
        );
        tile_cache_installation_lock
            .as_ref()
            .map(TileCacheInstallation::vector_tile_response_store)
    }

    /// The tile cache's response store under a byte view, so a provider's
    /// published style document, glyph ranges and sprite sheet land in the
    /// same namespaced cache file as its tiles. `None` while no tile cache is
    /// installed, which leaves those fetches on the network.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn style_resource_cache_store(
        &self,
    ) -> Option<Arc<dyn rustic_maps_core::CachedResponseStore<Vec<u8>>>> {
        let tile_cache_installation_lock = crate::panic_boundary::state_lock_guard(
            &self.tile_cache_installation,
            "the tile-cache installation state",
        );
        tile_cache_installation_lock
            .as_ref()
            .map(|tile_cache_installation| {
                Arc::clone(&tile_cache_installation.response_store)
                    as Arc<dyn rustic_maps_core::CachedResponseStore<Vec<u8>>>
            })
    }

    /// Builds a tile-pack manager over the current installed-cache snapshot,
    /// or throws the honest `TileCache` not-configured failure when no cache
    /// is installed. The snapshot is rebuilt per call so a time-to-live
    /// override is observed by the next call with no rebuild hook.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn tile_pack_manager(&self) -> Result<rustic_maps_cache::TilePackManager, SdkError> {
        let tile_cache_installation_lock = crate::panic_boundary::state_lock_guard(
            &self.tile_cache_installation,
            "the tile-cache installation state",
        );
        tile_cache_installation_lock
            .as_ref()
            .map(TileCacheInstallation::tile_pack_manager)
            .ok_or_else(tile_cache_not_configured_error)
    }

    /// The installed provider's declared maximum raster-tile zoom level, or
    /// `None` when it declares none; throws the `TileCache` not-configured
    /// failure when no cache is installed.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn tile_pack_maximum_zoom_level(&self) -> Result<Option<u8>, SdkError> {
        let tile_cache_installation_lock = crate::panic_boundary::state_lock_guard(
            &self.tile_cache_installation,
            "the tile-cache installation state",
        );
        tile_cache_installation_lock
            .as_ref()
            .map(|tile_cache_installation| tile_cache_installation.raster_tile_maximum_zoom_level)
            .ok_or_else(tile_cache_not_configured_error)
    }

    /// Registers one in-flight region download and answers the cancellation
    /// receiver the download races against its prepare future. A second
    /// download of the same region while one is in flight is refused with a
    /// typed cache failure naming the region; different regions may run
    /// concurrently.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn begin_region_download(
        &self,
        region_id: &str,
    ) -> Result<
        (
            tokio::sync::watch::Receiver<bool>,
            Arc<RegionDownloadCancellation>,
        ),
        SdkError,
    > {
        let mut region_downloads = crate::panic_boundary::state_lock_guard(
            &self.region_downloads,
            "the in-flight region download registry",
        );
        if region_downloads.contains_key(region_id) {
            return Err(SdkError::CacheOperationFailure {
                detail: format!(
                    "a download for the region {region_id:?} is already in flight; \
                     cancel it or wait for it to finish"
                ),
            });
        }
        let region_download_cancellation = Arc::new(RegionDownloadCancellation::new());
        let cancellation_receiver = region_download_cancellation.subscribe();
        region_downloads.insert(
            region_id.to_string(),
            Arc::clone(&region_download_cancellation),
        );
        Ok((cancellation_receiver, region_download_cancellation))
    }

    /// Drops one region's in-flight registration once its download resolved,
    /// whether it completed, failed or was cancelled. Removes only this
    /// download's own entry: replacing the installation clears the registry,
    /// so the identifier may already name a newer download.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn finish_region_download(
        &self,
        region_id: &str,
        region_download_cancellation: &Arc<RegionDownloadCancellation>,
    ) {
        let mut region_downloads = crate::panic_boundary::state_lock_guard(
            &self.region_downloads,
            "the in-flight region download registry",
        );
        let is_this_download = region_downloads
            .get(region_id)
            .is_some_and(|registered| Arc::ptr_eq(registered, region_download_cancellation));
        if is_this_download {
            region_downloads.remove(region_id);
        }
    }

    /// Signals one region's in-flight download to cancel. A no-op for an
    /// unknown, finished or already cancelled region: cancellation is
    /// idempotent.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn cancel_region_download(&self, region_id: &str) {
        let region_downloads = crate::panic_boundary::state_lock_guard(
            &self.region_downloads,
            "the in-flight region download registry",
        );
        if let Some(region_download_cancellation) = region_downloads.get(region_id) {
            region_download_cancellation.signal_cancellation();
        }
    }

    /// Signals every in-flight region download to cancel and empties the
    /// registry. Called whenever the installation is replaced or dropped, so
    /// a download never keeps writing into a store the SDK stopped serving.
    #[cfg(not(target_arch = "wasm32"))]
    fn cancel_all_region_downloads(&self) {
        let mut region_downloads = crate::panic_boundary::state_lock_guard(
            &self.region_downloads,
            "the in-flight region download registry",
        );
        for region_download_cancellation in region_downloads.values() {
            region_download_cancellation.signal_cancellation();
        }
        region_downloads.clear();
    }

    /// Resolves a free-form address text into candidate places, mirroring
    /// the `GeocodingProvider::forward_geocode` contract.
    pub async fn forward_geocode(&self, address_text: String) -> Result<Vec<Place>, SdkError> {
        let geocoding_provider = self.required_geocoding_provider()?;
        drive(async move {
            let native_places = geocoding_provider
                .forward_geocode(&address_text)
                .await
                .map_err(SdkError::from)?;
            Ok(native_places
                .iter()
                .map(Place::from_native_place_result)
                .collect())
        })
        .await
    }

    /// Resolves a coordinate into candidate places describing that position,
    /// mirroring the `GeocodingProvider::reverse_geocode` contract; the
    /// coordinate must be canonical degrees.
    pub async fn reverse_geocode(
        &self,
        coordinate: GeographicCoordinate,
    ) -> Result<Vec<Place>, SdkError> {
        let geocoding_provider = self.required_geocoding_provider()?;
        drive(async move {
            let native_coordinate = validated_coordinate(coordinate)?;
            let native_places = geocoding_provider
                .reverse_geocode(&native_coordinate)
                .await
                .map_err(SdkError::from)?;
            Ok(native_places
                .iter()
                .map(Place::from_native_place_result)
                .collect())
        })
        .await
    }

    /// Calculates a route through the ordered waypoints with the given
    /// options, mirroring the `RoutingProvider::calculate_route` contract;
    /// every waypoint must be a canonical coordinate.
    pub async fn calculate_route(
        &self,
        waypoints: Vec<GeographicCoordinate>,
        route_options: RouteOptions,
    ) -> Result<Route, SdkError> {
        let routing_provider = self.required_routing_provider()?;
        drive(async move {
            let native_waypoints = waypoints
                .iter()
                .copied()
                .map(validated_coordinate)
                .collect::<Result<Vec<_>, SdkError>>()?;
            let native_route = routing_provider
                .calculate_route(&native_waypoints, &route_options.to_native_route_options())
                .await
                .map_err(SdkError::from)?;
            Ok(Route::from_native_route_data(&native_route))
        })
        .await
    }

    /// Calculates durations and distances for every origin-destination pair,
    /// mirroring the `DistanceMatrixProvider::calculate_distance_matrix`
    /// contract; every origin and destination must be a canonical
    /// coordinate.
    pub async fn calculate_distance_matrix(
        &self,
        origins: Vec<GeographicCoordinate>,
        destinations: Vec<GeographicCoordinate>,
        transportation_mode: TransportationMode,
    ) -> Result<DistanceMatrix, SdkError> {
        let distance_matrix_provider = self.required_distance_matrix_provider()?;
        drive(async move {
            let native_origins = validated_degrees(&origins)?;
            let native_destinations = validated_degrees(&destinations)?;
            let native_distance_matrix = distance_matrix_provider
                .calculate_distance_matrix(
                    &native_origins,
                    &native_destinations,
                    transportation_mode.to_native_transportation_mode(),
                )
                .await
                .map_err(SdkError::from)?;
            Ok(DistanceMatrix::from_native_distance_matrix(
                &native_distance_matrix,
            ))
        })
        .await
    }

    /// Reports which transport modes and which strategy the serving routing
    /// provider can honour, mirroring the `RoutingProvider::route_capabilities`
    /// contract.
    pub fn route_capabilities(&self) -> Result<RouteCapabilities, SdkError> {
        let routing_provider = self.required_routing_provider()?;
        Ok(RouteCapabilities::from_native_route_capabilities(
            &routing_provider.route_capabilities(),
        ))
    }

    /// Returns fast completion candidates while the user types, mirroring
    /// the `SearchPlaceProvider::search_autocomplete` contract; the boundary
    /// restricts the search when given and must span canonical corners.
    pub async fn suggest(
        &self,
        query_text: String,
        search_boundary: Option<BoundingBox>,
    ) -> Result<Vec<Place>, SdkError> {
        let search_place_provider = self.required_search_place_provider()?;
        drive(async move {
            let native_search_boundary = validated_search_boundary(search_boundary)?;
            let native_places = search_place_provider
                .search_autocomplete(&query_text, native_search_boundary)
                .await
                .map_err(SdkError::from)?;
            Ok(native_places
                .iter()
                .map(Place::from_native_place_result)
                .collect())
        })
        .await
    }

    /// Returns full search results for a settled query, mirroring the
    /// `SearchPlaceProvider::search_places` contract; the boundary restricts
    /// the search when given and must span canonical corners.
    pub async fn search(
        &self,
        query_text: String,
        search_boundary: Option<BoundingBox>,
    ) -> Result<Vec<Place>, SdkError> {
        let search_place_provider = self.required_search_place_provider()?;
        drive(async move {
            let native_search_boundary = validated_search_boundary(search_boundary)?;
            let native_places = search_place_provider
                .search_places(&query_text, native_search_boundary)
                .await
                .map_err(SdkError::from)?;
            Ok(native_places
                .iter()
                .map(Place::from_native_place_result)
                .collect())
        })
        .await
    }

    /// Fetches incidents intersecting the bounding box, mirroring the
    /// `TrafficIncidentProvider::fetch_traffic_incidents` contract; the box
    /// corners must be canonical and south-west to north-east ordered.
    pub async fn fetch_traffic_incidents(
        &self,
        bounding_box: BoundingBox,
    ) -> Result<Vec<TrafficIncident>, SdkError> {
        let traffic_incident_provider = self.required_traffic_incident_provider()?;
        drive(async move {
            let native_bounding_box = validated_bounding_box(bounding_box)?;
            let native_traffic_incidents = traffic_incident_provider
                .fetch_traffic_incidents(&native_bounding_box)
                .await
                .map_err(SdkError::from)?;
            Ok(native_traffic_incidents
                .iter()
                .map(TrafficIncident::from_native_traffic_incident)
                .collect())
        })
        .await
    }

    /// Fetches per-segment flow observations inside the bounding box,
    /// mirroring the `TrafficFlowProvider::fetch_traffic_flow` contract; the
    /// box corners must be canonical and south-west to north-east ordered.
    pub async fn fetch_traffic_flow(
        &self,
        bounding_box: BoundingBox,
    ) -> Result<Vec<TrafficFlowData>, SdkError> {
        let traffic_flow_provider = self.required_traffic_flow_provider()?;
        drive(async move {
            let native_bounding_box = validated_bounding_box(bounding_box)?;
            let native_traffic_flow = traffic_flow_provider
                .fetch_traffic_flow(&native_bounding_box)
                .await
                .map_err(SdkError::from)?;
            Ok(native_traffic_flow
                .iter()
                .map(TrafficFlowData::from_native_traffic_flow_data)
                .collect())
        })
        .await
    }

    /// Fetches elevation samples for the ordered points, mirroring the
    /// `ElevationProvider::fetch_elevation_points` contract; every point must
    /// be a canonical coordinate.
    pub async fn fetch_elevation_points(
        &self,
        points: Vec<GeographicCoordinate>,
    ) -> Result<Vec<ElevationPoint>, SdkError> {
        let elevation_provider = self.required_elevation_provider()?;
        drive(async move {
            let native_points = points
                .into_iter()
                .map(validated_coordinate)
                .collect::<Result<Vec<_>, SdkError>>()?;
            let native_elevation_points = elevation_provider
                .fetch_elevation_points(&native_points)
                .await
                .map_err(SdkError::from)?;
            Ok(native_elevation_points
                .iter()
                .map(ElevationPoint::from_native_elevation_point)
                .collect())
        })
        .await
    }

    /// Fetches one web-mercator XYZ raster tile at the requested resolution,
    /// mirroring the `RasterTileProvider::fetch_raster_tile` contract.
    pub async fn fetch_raster_tile(
        &self,
        tile_x: u32,
        tile_y: u32,
        zoom_level: u8,
        image_format: RasterImageFormat,
        tile_resolution: RasterTileResolution,
    ) -> Result<RasterTileData, SdkError> {
        let raster_tile_provider = self.required_raster_tile_provider()?;
        drive(async move {
            let native_tile = raster_tile_provider
                .fetch_raster_tile(
                    tile_x,
                    tile_y,
                    zoom_level,
                    image_format.to_native_image_format(),
                    tile_resolution.to_native_tile_resolution(),
                )
                .await
                .map_err(SdkError::from)?;
            Ok(RasterTileData::from_native_raster_tile_data(&native_tile))
        })
        .await
    }

    /// Clones the registered geocoding provider out of the registry;
    /// otherwise throws
    /// [`SdkError::ProviderNotConfigured`] with the contract name. The lock
    /// guard is dropped when this returns, before any await.
    fn required_geocoding_provider(&self) -> Result<Arc<dyn GeocodingProvider>, SdkError> {
        crate::panic_boundary::state_lock_guard(
            &self.provider_registry,
            "the provider registry's contract slots",
        )
        .require_geocoding_provider()
        .map_err(SdkError::from)
    }

    /// Clones the registered routing provider out of the registry; otherwise
    /// throws the contract's not-configured failure. The lock guard is
    /// dropped when this returns, before any await.
    fn required_routing_provider(&self) -> Result<Arc<dyn RoutingProvider>, SdkError> {
        crate::panic_boundary::state_lock_guard(
            &self.provider_registry,
            "the provider registry's contract slots",
        )
        .require_routing_provider()
        .map_err(SdkError::from)
    }

    /// Clones the registered distance-matrix provider out of the registry;
    /// otherwise throws the contract's not-configured failure. The lock
    /// guard is dropped when this returns, before any await.
    fn required_distance_matrix_provider(
        &self,
    ) -> Result<Arc<dyn DistanceMatrixProvider>, SdkError> {
        crate::panic_boundary::state_lock_guard(
            &self.provider_registry,
            "the provider registry's contract slots",
        )
        .require_distance_matrix_provider()
        .map_err(SdkError::from)
    }

    /// Clones the registered place-search provider out of the registry;
    /// otherwise throws the contract's not-configured failure. The lock
    /// guard is dropped when this returns, before any await.
    fn required_search_place_provider(&self) -> Result<Arc<dyn SearchPlaceProvider>, SdkError> {
        crate::panic_boundary::state_lock_guard(
            &self.provider_registry,
            "the provider registry's contract slots",
        )
        .require_search_place_provider()
        .map_err(SdkError::from)
    }

    /// Clones the registered traffic-incident provider out of the registry;
    /// otherwise throws the contract's not-configured failure. The lock
    /// guard is dropped when this returns, before any await.
    fn required_traffic_incident_provider(
        &self,
    ) -> Result<Arc<dyn TrafficIncidentProvider>, SdkError> {
        crate::panic_boundary::state_lock_guard(
            &self.provider_registry,
            "the provider registry's contract slots",
        )
        .require_traffic_incident_provider()
        .map_err(SdkError::from)
    }

    /// Clones the registered traffic-flow provider out of the registry;
    /// otherwise throws the contract's not-configured failure. The lock
    /// guard is dropped when this returns, before any await.
    fn required_traffic_flow_provider(&self) -> Result<Arc<dyn TrafficFlowProvider>, SdkError> {
        crate::panic_boundary::state_lock_guard(
            &self.provider_registry,
            "the provider registry's contract slots",
        )
        .require_traffic_flow_provider()
        .map_err(SdkError::from)
    }

    /// Clones the registered raster-tile provider out of the registry;
    /// otherwise throws the contract's not-configured failure. The lock
    /// guard is dropped when this returns, before any await. The surface
    /// seam pulls the same provider through this accessor at attach time.
    pub(crate) fn required_raster_tile_provider(
        &self,
    ) -> Result<Arc<dyn RasterTileProvider>, SdkError> {
        crate::panic_boundary::state_lock_guard(
            &self.provider_registry,
            "the provider registry's contract slots",
        )
        .require_raster_tile_provider()
        .map_err(SdkError::from)
    }

    /// Clones the registered vector-tile provider out of the registry;
    /// otherwise throws the contract's not-configured failure. The native
    /// surface view resolves a provider-backed vector base through this
    /// accessor.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) fn required_vector_tile_provider(
        &self,
    ) -> Result<Arc<dyn VectorTileProvider>, SdkError> {
        crate::panic_boundary::state_lock_guard(
            &self.provider_registry,
            "the provider registry's contract slots",
        )
        .require_vector_tile_provider()
        .map_err(SdkError::from)
    }

    /// Returns the authenticated fetch of the vector-tile provider's
    /// published style resources, or a not-configured failure naming it when
    /// the provider publishes no hosted style.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn required_vector_tile_style_resources(
        &self,
    ) -> Result<Arc<dyn StyleResourceClient>, SdkError> {
        crate::panic_boundary::state_lock_guard(
            &self.provider_registry,
            "the provider registry's contract slots",
        )
        .require_vector_tile_style_resources()
        .map_err(SdkError::from)
    }

    /// The hosts the serving provider declared for its published style
    /// resources, empty while nothing is registered.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn vector_tile_style_resource_hosts(&self) -> Vec<String> {
        crate::panic_boundary::state_lock_guard(
            &self.provider_registry,
            "the provider registry's contract slots",
        )
        .vector_tile_style_resource_hosts()
    }

    /// Reports the provider key and display name recorded for a capability,
    /// or `None` while no provider has been configured for it. The native
    /// surface view uses the key to verify that a provider-backed base names
    /// the provider that actually serves vector tiles.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) fn selected_provider(&self, contract: ProviderContract) -> Option<(String, String)> {
        let selections = crate::panic_boundary::state_lock_guard(
            &self.provider_selections,
            "the provider selection record",
        );
        selections
            .iter()
            .find(|selection| selection.contract == contract)
            .map(|selection| {
                (
                    selection.provider_key.clone(),
                    selection.display_name.clone(),
                )
            })
    }

    /// Clones the registered elevation provider out of the registry;
    /// otherwise throws the contract's not-configured failure. The lock guard
    /// is dropped when this returns, before any await.
    fn required_elevation_provider(&self) -> Result<Arc<dyn ElevationProvider>, SdkError> {
        crate::panic_boundary::state_lock_guard(
            &self.provider_registry,
            "the provider registry's contract slots",
        )
        .require_elevation_provider()
        .map_err(SdkError::from)
    }
}

impl Default for MapSdk {
    /// Mirrors [`MapSdk::new`] (the clippy `new_without_default` convention).
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
impl MapSdk {
    /// The namespace a test installation's cache entries carry.
    const TEST_TILE_CACHE_NAMESPACE: &'static str = "test-provider";

    /// Installs a tile-cache installation built from a test provider double
    /// and store, so the offline pack members are exercised without a live
    /// provider or the network. Production code never installs an
    /// installation this way.
    pub(crate) fn install_tile_cache_for_testing(
        &self,
        raster_tile_provider: Arc<dyn RasterTileProvider>,
        response_store: Arc<rustic_maps_cache::SqliteResponseCache>,
        time_to_live_seconds: u64,
        provider_terms_ceiling_seconds: Option<u64>,
        raster_tile_maximum_zoom_level: Option<u8>,
    ) {
        *crate::panic_boundary::state_lock_guard(
            &self.tile_cache_installation,
            "the tile-cache installation state",
        ) = Some(TileCacheInstallation {
            raster_tile_provider,
            response_store,
            statistics_counters: rustic_maps_cache::TileCacheStatisticsCounters::shared(),
            tile_source_namespace: Self::TEST_TILE_CACHE_NAMESPACE.to_string(),
            time_to_live_seconds,
            tile_pack_byte_budget_bytes: rustic_maps_cache::DEFAULT_TILE_PACK_BYTE_BUDGET_BYTES,
            provider_terms_ceiling_seconds,
            raster_tile_maximum_zoom_level,
        });
    }
}

/// The honest not-configured failure of a surface that requires an installed
/// disk tile cache: the browser build has no such store, so every pack member
/// answers this same failure there, and a native call without an installation
/// answers it too.
pub(crate) fn tile_cache_not_configured_error() -> SdkError {
    SdkError::ProviderNotConfigured {
        contract_name: TILE_CACHE_NOT_CONFIGURED_CONTRACT_NAME.to_string(),
    }
}

/// Clamps a requested tile-cache time-to-live into what the installed
/// provider's terms allow: the ceiling bounds what the cache may
/// retain, so a request above it stores at the ceiling and the terms
/// readback reports the effective value — the request is never refused,
/// because the compliance bound is enforced by what the cache stores, not by
/// how the call returns. Without a declared ceiling the requested value
/// applies verbatim. Native-only with the disk-cache installation it serves.
#[cfg(not(target_arch = "wasm32"))]
fn clamped_to_provider_terms_ceiling(
    requested_time_to_live_seconds: u64,
    provider_terms_ceiling_seconds: Option<u64>,
) -> u64 {
    requested_time_to_live_seconds
        .min(provider_terms_ceiling_seconds.unwrap_or(NO_PROVIDER_TERMS_CEILING_SECONDS))
}

/// Validates every waypoint degree into the native coordinate type.
fn validated_degrees(
    coordinates: &[GeographicCoordinate],
) -> Result<Vec<geospatial_types::GeographicCoordinate>, SdkError> {
    coordinates
        .iter()
        .copied()
        .map(validated_coordinate)
        .collect()
}

/// Validates the optional search boundary into its native form.
fn validated_search_boundary(
    search_boundary: Option<BoundingBox>,
) -> Result<Option<geospatial_types::BoundingBox>, SdkError> {
    search_boundary.map(validated_bounding_box).transpose()
}

#[cfg(test)]
mod tests {
    use super::MapSdk;
    use crate::error::SdkError;
    use crate::records::ProviderContract;

    /// Consumer key the registration tests use; the client construction and
    /// the registry slots need no live endpoint.
    const MAPQUEST_TEST_CONSUMER_KEY: &str = "test-consumer-key";

    /// Tile-cache byte budget the registration tests install.
    const TEST_TILE_CACHE_BUDGET_BYTES: u64 = 16 * 1024 * 1024;

    #[test]
    fn mapquest_registration_installs_every_documented_provider_slot() {
        let map_sdk = MapSdk::new();
        map_sdk
            .configure_provider_set(
                "mapquest".to_string(),
                MAPQUEST_TEST_CONSUMER_KEY.to_string(),
            )
            .expect("the MapQuest providers construct from a usable key");
        // Every slot the one-call registration documents resolves.
        assert!(map_sdk.required_geocoding_provider().is_ok());
        assert!(map_sdk.required_routing_provider().is_ok());
        assert!(map_sdk.required_distance_matrix_provider().is_ok());
        assert!(map_sdk.required_search_place_provider().is_ok());
        assert!(map_sdk.required_traffic_incident_provider().is_ok());
        assert!(map_sdk.required_traffic_flow_provider().is_ok());
        assert!(map_sdk.required_raster_tile_provider().is_ok());
        // Elevation stays unregistered: the MapQuest provider set carries no
        // elevation provider, so the slot keeps its not-configured failure.
        match map_sdk
            .provider_registry
            .lock()
            .unwrap()
            .require_elevation_provider()
        {
            Err(elevation_failure) => assert!(matches!(
                elevation_failure,
                geospatial_types::SDKError::ProviderNotConfigured(_)
            )),
            Ok(_) => panic!("elevation stays unregistered"),
        }
    }

    #[test]
    fn configuring_one_capability_leaves_every_other_slot_untouched() {
        let map_sdk = MapSdk::new();
        map_sdk
            .configure_provider(
                ProviderContract::Routing,
                "mapquest".to_string(),
                MAPQUEST_TEST_CONSUMER_KEY.to_string(),
            )
            .expect("the routing provider constructs from a usable key");
        assert!(map_sdk.required_routing_provider().is_ok());
        // Only routing was chosen: every other capability keeps its
        // not-configured answer, which is what lets an application mix
        // providers.
        assert!(map_sdk.required_geocoding_provider().is_err());
        assert!(map_sdk.required_raster_tile_provider().is_err());
        let registrations = map_sdk.provider_registrations();
        let routing_registration = registrations
            .iter()
            .find(|registration| registration.contract == ProviderContract::Routing)
            .expect("every capability appears in the readback");
        assert!(routing_registration.registered);
        assert_eq!(routing_registration.provider_key, "mapquest");
        assert_eq!(routing_registration.display_name, "MapQuest");
        let geocoding_registration = registrations
            .iter()
            .find(|registration| registration.contract == ProviderContract::Geocoding)
            .expect("every capability appears in the readback");
        assert!(!geocoding_registration.registered);
        assert!(geocoding_registration.provider_key.is_empty());
    }

    #[test]
    fn the_registration_readback_lists_every_capability_in_a_stable_order() {
        let map_sdk = MapSdk::new();
        let registrations = map_sdk.provider_registrations();
        assert_eq!(registrations.len(), ProviderContract::ALL.len());
        assert_eq!(
            registrations
                .iter()
                .map(|registration| registration.contract)
                .collect::<Vec<_>>(),
            ProviderContract::ALL.to_vec(),
            "the readback follows the documented settings order"
        );
        assert!(
            registrations
                .iter()
                .all(|registration| !registration.registered),
            "a fresh SDK serves no capability"
        );
    }

    #[test]
    fn an_unknown_key_or_an_unserved_capability_is_refused_by_name() {
        let map_sdk = MapSdk::new();
        match map_sdk.configure_provider(
            ProviderContract::Routing,
            "no-such-provider".to_string(),
            String::new(),
        ) {
            Err(SdkError::ProviderNotConfigured { contract_name }) => {
                assert!(
                    contract_name.contains("no-such-provider"),
                    "{contract_name}"
                );
            }
            other => panic!("an unknown provider key must throw, got {other:?}"),
        }
        match map_sdk.configure_provider(
            ProviderContract::Elevation,
            "mapquest".to_string(),
            MAPQUEST_TEST_CONSUMER_KEY.to_string(),
        ) {
            Err(SdkError::ProviderNotConfigured { contract_name }) => {
                assert_eq!(contract_name, "ElevationProvider");
            }
            other => panic!("an unserved capability must throw, got {other:?}"),
        }
    }

    #[test]
    fn a_blank_credential_is_refused_and_leaves_the_previous_provider_serving() {
        let map_sdk = MapSdk::new();
        map_sdk
            .configure_provider(
                ProviderContract::Routing,
                "mapquest".to_string(),
                MAPQUEST_TEST_CONSUMER_KEY.to_string(),
            )
            .expect("the first configuration succeeds");
        match map_sdk.configure_provider(
            ProviderContract::Routing,
            "mapquest".to_string(),
            "   ".to_string(),
        ) {
            Err(SdkError::ProviderRequestRejected { detail }) => {
                assert!(detail.contains("requires a credential"), "{detail}");
            }
            other => panic!("a blank credential must throw, got {other:?}"),
        }
        assert!(
            map_sdk.required_routing_provider().is_ok(),
            "a refused configuration leaves the previous provider serving"
        );
    }

    #[test]
    fn a_keyless_routing_provider_installs_without_a_credential() {
        let map_sdk = MapSdk::new();
        map_sdk
            .configure_provider(ProviderContract::Routing, "osrm".to_string(), String::new())
            .expect("a keyless routing provider installs with an empty credential");
        assert!(map_sdk.required_routing_provider().is_ok());
        let routing_registration = map_sdk
            .provider_registrations()
            .into_iter()
            .find(|registration| registration.contract == ProviderContract::Routing)
            .expect("every capability appears in the readback");
        assert_eq!(routing_registration.provider_key, "osrm");
        assert_eq!(routing_registration.display_name, "OSRM");
        // Replacing the routing provider leaves the other capabilities as
        // they were, so an application can route through OSRM while its
        // geocoding keeps its own provider.
        map_sdk
            .configure_provider(
                ProviderContract::Geocoding,
                "mapquest".to_string(),
                MAPQUEST_TEST_CONSUMER_KEY.to_string(),
            )
            .expect("the geocoding provider constructs from a usable key");
        map_sdk
            .configure_provider(
                ProviderContract::Routing,
                "valhalla".to_string(),
                String::new(),
            )
            .expect("the second keyless routing provider installs");
        let registrations = map_sdk.provider_registrations();
        let provider_key_for = |contract: ProviderContract| {
            registrations
                .iter()
                .find(|registration| registration.contract == contract)
                .map(|registration| registration.provider_key.clone())
                .expect("every capability appears in the readback")
        };
        assert_eq!(provider_key_for(ProviderContract::Routing), "valhalla");
        assert_eq!(provider_key_for(ProviderContract::Geocoding), "mapquest");
    }

    #[test]
    fn a_keyless_routing_provider_refuses_a_malformed_deployment_address() {
        let map_sdk = MapSdk::new();
        match map_sdk.configure_provider(
            ProviderContract::Routing,
            "osrm".to_string(),
            "not a url".to_string(),
        ) {
            Err(_) => {}
            Ok(()) => panic!("a malformed deployment address must be refused"),
        }
        assert!(
            map_sdk.required_routing_provider().is_err(),
            "a refused configuration registers nothing"
        );
    }

    #[test]
    fn a_blank_credential_is_refused_and_registers_nothing() {
        let map_sdk = MapSdk::new();
        for blank_credential in ["", "   ", "\t\n"] {
            match map_sdk
                .configure_provider_set("mapquest".to_string(), blank_credential.to_string())
            {
                Err(SdkError::ProviderRequestRejected { detail }) => {
                    assert!(
                        detail.contains("requires a credential"),
                        "the refusal names the missing credential: {detail}"
                    );
                }
                other => panic!("a blank credential must be refused, got {other:?}"),
            }
        }
        assert!(
            map_sdk
                .provider_registrations()
                .iter()
                .all(|registration| !registration.registered),
            "a refused credential registers no provider contract"
        );
    }

    #[test]
    fn the_all_in_one_registration_records_the_serving_provider_per_capability() {
        let map_sdk = MapSdk::new();
        map_sdk
            .configure_provider_set(
                "mapquest".to_string(),
                MAPQUEST_TEST_CONSUMER_KEY.to_string(),
            )
            .expect("the MapQuest providers construct from a usable key");
        let registrations = map_sdk.provider_registrations();
        for contract in [
            ProviderContract::Geocoding,
            ProviderContract::Routing,
            ProviderContract::DistanceMatrix,
            ProviderContract::PlaceSearch,
            ProviderContract::TrafficIncidents,
            ProviderContract::TrafficFlow,
            ProviderContract::RasterTiles,
        ] {
            let registration = registrations
                .iter()
                .find(|registration| registration.contract == contract)
                .expect("every capability appears in the readback");
            assert!(registration.registered, "{contract:?} is served");
            assert_eq!(registration.provider_key, "mapquest", "{contract:?}");
            assert_eq!(
                registration.display_name, "MapQuest",
                "the catalog entry's display name is recorded with its key"
            );
        }
        let elevation = registrations
            .iter()
            .find(|registration| registration.contract == ProviderContract::Elevation)
            .expect("every capability appears in the readback");
        assert!(!elevation.registered, "the vendor set carries no elevation");
    }

    #[test]
    fn tile_cache_registration_refuses_unusable_inputs() {
        let map_sdk = MapSdk::new();
        let cache_path = std::env::temp_dir()
            .join("rustic-maps-native-core-tile-cache-invalid-test")
            .to_string_lossy()
            .to_string();
        for (path, budget, expected_detail) in [
            ("", 1, "path must be non-empty"),
            (cache_path.as_str(), 0, "budget must be positive"),
        ] {
            match map_sdk.set_provider_credential_with_tile_cache(
                "mapquest".to_string(),
                MAPQUEST_TEST_CONSUMER_KEY.to_string(),
                path.to_string(),
                budget,
            ) {
                Err(SdkError::InvalidCoordinateInput { detail }) => {
                    assert!(detail.contains(expected_detail), "{detail}");
                }
                other => panic!("unusable tile-cache input must throw, got {other:?}"),
            }
        }
    }

    #[test]
    fn tile_cache_registration_refuses_an_unknown_provider_key() {
        let map_sdk = MapSdk::new();
        match map_sdk.set_provider_credential_with_tile_cache(
            "no-such-provider".to_string(),
            MAPQUEST_TEST_CONSUMER_KEY.to_string(),
            std::env::temp_dir()
                .join("rustic-maps-native-core-tile-cache-unknown-provider")
                .to_string_lossy()
                .to_string(),
            TEST_TILE_CACHE_BUDGET_BYTES,
        ) {
            Err(SdkError::ProviderNotConfigured { contract_name }) => {
                assert!(
                    contract_name.contains("no-such-provider"),
                    "the refusal names the unknown key: {contract_name}"
                );
            }
            other => panic!("an unknown provider key must be refused, got {other:?}"),
        }
    }

    #[test]
    fn tile_cache_registration_refuses_a_provider_without_raster_tiles() {
        let map_sdk = MapSdk::new();
        match map_sdk.set_provider_credential_with_tile_cache(
            "osrm".to_string(),
            String::new(),
            std::env::temp_dir()
                .join("rustic-maps-native-core-tile-cache-non-raster")
                .to_string_lossy()
                .to_string(),
            TEST_TILE_CACHE_BUDGET_BYTES,
        ) {
            Err(SdkError::ProviderRequestRejected { detail }) => {
                assert!(
                    detail.contains("serves no raster tiles"),
                    "the refusal names the missing capability: {detail}"
                );
            }
            other => panic!("a provider without raster tiles must be refused, got {other:?}"),
        }
    }

    #[test]
    fn tile_cache_registration_refuses_a_blank_credential() {
        let map_sdk = MapSdk::new();
        for blank_credential in ["", "   ", "\t\n"] {
            match map_sdk.set_provider_credential_with_tile_cache(
                "mapquest".to_string(),
                blank_credential.to_string(),
                std::env::temp_dir()
                    .join("rustic-maps-native-core-tile-cache-blank-credential")
                    .to_string_lossy()
                    .to_string(),
                TEST_TILE_CACHE_BUDGET_BYTES,
            ) {
                Err(SdkError::ProviderRequestRejected { detail }) => {
                    assert!(
                        detail.contains("requires a credential"),
                        "the refusal names the missing credential: {detail}"
                    );
                }
                other => panic!("a blank credential must be refused, got {other:?}"),
            }
        }
        assert!(
            matches!(
                map_sdk.raster_tile_cache_terms(),
                Err(SdkError::ProviderNotConfigured { .. })
            ),
            "a refused registration installs no tile cache"
        );
    }

    #[test]
    fn tile_cache_registration_installs_the_cached_raster_slot() {
        let map_sdk = MapSdk::new();
        let cache_directory = std::env::temp_dir().join(format!(
            "rustic-maps-native-core-tile-cache-{}",
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
                16 * 1024 * 1024,
            )
            .expect("an empty path in an existing directory opens the cache");
        // The raster slot resolves through the cached wrapper; a plain
        // re-registration replaces the slot with the uncached provider.
        assert!(map_sdk.required_raster_tile_provider().is_ok());
        map_sdk
            .configure_provider_set(
                "mapquest".to_string(),
                MAPQUEST_TEST_CONSUMER_KEY.to_string(),
            )
            .expect("re-registration still replaces every slot");
        assert!(map_sdk.required_raster_tile_provider().is_ok());
        std::fs::remove_dir_all(&cache_directory).expect("the test cache directory cleans up");
    }

    /// Registers the MapQuest providers with a tile cache under a fresh
    /// (label-unique) temporary database path, and cleans the directory up
    /// at the test's end, the same pattern the registration test above uses.
    fn registered_with_labelled_tile_cache(test_label: &str) -> (MapSdk, std::path::PathBuf) {
        let map_sdk = MapSdk::new();
        let cache_directory = std::env::temp_dir().join(format!(
            "rustic-maps-native-core-tile-cache-settings-{test_label}-{}",
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
                16 * 1024 * 1024,
            )
            .expect("an empty path in an existing directory opens the cache");
        (map_sdk, cache_directory)
    }

    #[test]
    fn tile_cache_terms_report_the_installed_default_and_ceiling() {
        let map_sdk = MapSdk::new();
        match map_sdk.raster_tile_cache_terms() {
            Err(SdkError::ProviderNotConfigured { contract_name }) => {
                assert_eq!(
                    contract_name, "TileCache",
                    "without an installation the terms have nothing to read from"
                );
            }
            other => panic!("terms without an installed cache must throw, got {other:?}"),
        }
        let (map_sdk, cache_directory) =
            registered_with_labelled_tile_cache("terms-report-default");
        let tile_cache_terms = map_sdk.raster_tile_cache_terms().unwrap();
        assert_eq!(
            tile_cache_terms.effective_time_to_live_seconds,
            rustic_maps_cache::ResponseCategory::RasterTiles.default_time_to_live_seconds(),
            "a fresh registration starts at the category default (thirty days)"
        );
        assert_eq!(
            tile_cache_terms.provider_terms_ceiling_seconds,
            Some(crate::provider_catalog::MAPQUEST_RASTER_TILE_CACHE_CEILING_SECONDS),
            "the MapQuest registration declares its documented terms ceiling"
        );
        assert_eq!(
            tile_cache_terms.default_time_to_live_seconds,
            tile_cache_terms.provider_terms_ceiling_seconds.unwrap(),
            "the cache default equals the provider ceiling (the lockstep ruling)"
        );
        std::fs::remove_dir_all(&cache_directory).expect("the test cache directory cleans up");
    }

    #[test]
    fn tile_time_to_live_override_clamps_to_the_provider_terms_ceiling() {
        let (map_sdk, cache_directory) = registered_with_labelled_tile_cache("override-clamps");
        // Above the ceiling: accepted, and the effective value stores at the
        // ceiling (PRD §6.7 — the terms bound what the cache retains).
        let above_ceiling_time_to_live =
            crate::provider_catalog::MAPQUEST_RASTER_TILE_CACHE_CEILING_SECONDS * 2;
        map_sdk
            .set_raster_tile_cache_time_to_live(above_ceiling_time_to_live)
            .expect("an above-ceiling request is accepted with the clamped effective value");
        assert_eq!(
            map_sdk
                .raster_tile_cache_terms()
                .unwrap()
                .effective_time_to_live_seconds,
            crate::provider_catalog::MAPQUEST_RASTER_TILE_CACHE_CEILING_SECONDS
        );
        // Below the ceiling: applies verbatim.
        let below_ceiling_time_to_live = 60;
        map_sdk
            .set_raster_tile_cache_time_to_live(below_ceiling_time_to_live)
            .expect("a below-ceiling request applies verbatim");
        assert_eq!(
            map_sdk
                .raster_tile_cache_terms()
                .unwrap()
                .effective_time_to_live_seconds,
            below_ceiling_time_to_live
        );
        std::fs::remove_dir_all(&cache_directory).expect("the test cache directory cleans up");
    }

    #[test]
    fn tile_time_to_live_override_refuses_a_zero_value() {
        let (map_sdk, cache_directory) =
            registered_with_labelled_tile_cache("override-zero-refused");
        match map_sdk.set_raster_tile_cache_time_to_live(0) {
            Err(SdkError::InvalidCoordinateInput { detail }) => {
                assert!(
                    detail.contains("must be positive"),
                    "the refusal carries the precise reasoning: {detail}"
                );
            }
            other => panic!("a zero time-to-live must be refused with a detail, got {other:?}"),
        }
        assert_eq!(
            map_sdk
                .raster_tile_cache_terms()
                .unwrap()
                .effective_time_to_live_seconds,
            rustic_maps_cache::ResponseCategory::RasterTiles.default_time_to_live_seconds(),
            "the refused call left the installed terms unchanged"
        );
        std::fs::remove_dir_all(&cache_directory).expect("the test cache directory cleans up");
    }

    #[test]
    fn tile_cache_settings_surface_requires_an_installed_cache() {
        let map_sdk = MapSdk::new();
        map_sdk
            .configure_provider_set(
                "mapquest".to_string(),
                MAPQUEST_TEST_CONSUMER_KEY.to_string(),
            )
            .expect("the uncached registration still installs the raster slot");
        assert!(map_sdk.required_raster_tile_provider().is_ok());
        for (settings_call, thrown_result) in [
            (
                "raster_tile_cache_terms",
                map_sdk.raster_tile_cache_terms().map(|_| ()),
            ),
            (
                "set_raster_tile_cache_time_to_live",
                map_sdk.set_raster_tile_cache_time_to_live(60),
            ),
        ] {
            match thrown_result {
                Err(SdkError::ProviderNotConfigured { contract_name }) => assert_eq!(
                    contract_name, "TileCache",
                    "{settings_call} without an installed cache names the unconfigured backing store"
                ),
                Err(_) => panic!("{settings_call} threw the wrong variant"),
                Ok(_) => panic!("{settings_call} answered without an installed cache"),
            }
        }
    }

    #[test]
    fn re_registration_resets_the_tile_time_to_live_to_the_default() {
        let (map_sdk, cache_directory) =
            registered_with_labelled_tile_cache("re-registration-resets");
        map_sdk
            .set_raster_tile_cache_time_to_live(60)
            .expect("the override applies to the installed cache");
        // Re-registration through the frozen method replaces the slots and
        // the tuning: the fresh cache starts again at the category default.
        map_sdk
            .set_provider_credential_with_tile_cache(
                "mapquest".to_string(),
                MAPQUEST_TEST_CONSUMER_KEY.to_string(),
                cache_directory
                    .join("tiles.sqlite")
                    .to_string_lossy()
                    .to_string(),
                16 * 1024 * 1024,
            )
            .expect("re-registration reuses the cache path");
        assert_eq!(
            map_sdk
                .raster_tile_cache_terms()
                .unwrap()
                .effective_time_to_live_seconds,
            rustic_maps_cache::ResponseCategory::RasterTiles.default_time_to_live_seconds()
        );
        std::fs::remove_dir_all(&cache_directory).expect("the test cache directory cleans up");
    }

    #[test]
    fn tile_cache_statistics_are_absent_without_an_installed_cache() {
        let map_sdk = MapSdk::new();
        assert!(
            map_sdk.tile_cache_statistics().is_none(),
            "no installed cache means nothing to report"
        );
        map_sdk
            .configure_provider_set(
                "mapquest".to_string(),
                MAPQUEST_TEST_CONSUMER_KEY.to_string(),
            )
            .expect("the uncached registration still installs the raster slot");
        assert!(
            map_sdk.tile_cache_statistics().is_none(),
            "an uncached provider has no tile payload cache to report"
        );
    }

    #[test]
    fn tile_cache_statistics_start_at_zero_with_an_empty_store() {
        let (map_sdk, cache_directory) =
            registered_with_labelled_tile_cache("statistics-zero-state");
        let statistics = map_sdk
            .tile_cache_statistics()
            .expect("the registration installed a tile cache");
        assert_eq!(statistics.hit_count, 0);
        assert_eq!(statistics.miss_count, 0);
        assert_eq!(
            statistics.stored_byte_count,
            Some(0),
            "a fresh store holds no payload bytes"
        );
        std::fs::remove_dir_all(&cache_directory).expect("the test cache directory cleans up");
    }

    /// A vector payload the shared store can round-trip, so the test exercises
    /// the exact store view the vector base fetches through.
    fn vector_tile_payload_for_statistics_test() -> geospatial_types::VectorTileData {
        geospatial_types::VectorTileData {
            tile_x: 1,
            tile_y: 2,
            zoom_level: 3,
            tile_encoding: geospatial_types::VectorTileEncoding::MapboxVectorTile,
            tile_bytes: vec![0_u8; 16],
        }
    }

    #[test]
    fn one_miss_then_one_hit_count_through_the_shared_tile_store() {
        let (map_sdk, cache_directory) =
            registered_with_labelled_tile_cache("statistics-hit-and-miss");
        let payload_cache_store = map_sdk
            .vector_tile_payload_cache_store()
            .expect("the registration installed the shared store");

        assert!(
            payload_cache_store
                .lookup_response("vector-tile-address")
                .expect("an empty store answers no payload")
                .is_none()
        );
        payload_cache_store
            .store_response(
                "vector-tile-address",
                std::sync::Arc::new(vector_tile_payload_for_statistics_test()),
                60,
            )
            .expect("the payload stores in the shared cache");
        assert!(
            payload_cache_store
                .lookup_response("vector-tile-address")
                .expect("the stored payload answers")
                .is_some()
        );

        let statistics = map_sdk
            .tile_cache_statistics()
            .expect("the registration installed a tile cache");
        assert_eq!(statistics.miss_count, 1, "the empty lookup asked the store");
        assert_eq!(
            statistics.hit_count, 1,
            "the stored payload answered from the cache"
        );
        assert!(
            statistics.stored_byte_count.unwrap_or(0) > 0,
            "the stored payload contributes its bytes to the occupancy"
        );
        std::fs::remove_dir_all(&cache_directory).expect("the test cache directory cleans up");
    }

    #[test]
    fn the_counters_survive_a_time_to_live_override() {
        let (map_sdk, cache_directory) =
            registered_with_labelled_tile_cache("statistics-survive-rewrap");
        let payload_cache_store = map_sdk
            .vector_tile_payload_cache_store()
            .expect("the registration installed the shared store");
        payload_cache_store
            .lookup_response("vector-tile-address")
            .expect("the empty store answers no payload");

        // Rewrapping the cached provider under a new time-to-live reuses the
        // installation's counter pair, so the readback keeps counting.
        map_sdk
            .set_raster_tile_cache_time_to_live(60)
            .expect("the override rewraps the cached provider");
        let payload_cache_store_after_rewrap = map_sdk
            .vector_tile_payload_cache_store()
            .expect("the rewrap keeps the shared store");
        payload_cache_store_after_rewrap
            .store_response(
                "vector-tile-address",
                std::sync::Arc::new(vector_tile_payload_for_statistics_test()),
                60,
            )
            .expect("the payload stores in the shared cache");
        payload_cache_store_after_rewrap
            .lookup_response("vector-tile-address")
            .expect("the stored payload answers");

        let statistics = map_sdk
            .tile_cache_statistics()
            .expect("the registration installed a tile cache");
        assert_eq!(
            statistics.miss_count, 1,
            "the pre-override miss stays counted"
        );
        assert_eq!(statistics.hit_count, 1, "the post-override hit is counted");
        std::fs::remove_dir_all(&cache_directory).expect("the test cache directory cleans up");
    }

    #[test]
    fn reinstalling_the_tile_cache_resets_the_counters() {
        let (map_sdk, cache_directory) =
            registered_with_labelled_tile_cache("statistics-reset-on-reinstall");
        map_sdk
            .vector_tile_payload_cache_store()
            .expect("the registration installed the shared store")
            .lookup_response("vector-tile-address")
            .expect("the empty store answers no payload");
        assert_eq!(
            map_sdk
                .tile_cache_statistics()
                .expect("the registration installed a tile cache")
                .miss_count,
            1
        );

        // Re-registration replaces the store and its counter pair; the new
        // cache starts at zero like a freshly installed one.
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
            .expect("re-registration reuses the cache path");
        let reinstalled_statistics = map_sdk
            .tile_cache_statistics()
            .expect("the re-registration installed a tile cache");
        assert_eq!(reinstalled_statistics.hit_count, 0);
        assert_eq!(reinstalled_statistics.miss_count, 0);
        std::fs::remove_dir_all(&cache_directory).expect("the test cache directory cleans up");
    }

    /// The bounding box the traffic-flow caching tests request: a pane small
    /// enough to fall inside a single cache grid cell.
    fn traffic_flow_test_bounding_box() -> crate::records::BoundingBox {
        crate::records::BoundingBox {
            south_west: crate::records::GeographicCoordinate {
                latitude: 38.57,
                longitude: -121.50,
            },
            north_east: crate::records::GeographicCoordinate {
                latitude: 38.58,
                longitude: -121.49,
            },
        }
    }

    /// A foreign traffic-flow implementation that counts its own calls.
    struct CountingForeignTrafficFlowProvider {
        call_count: std::sync::Arc<std::sync::atomic::AtomicU64>,
    }

    #[async_trait::async_trait]
    impl crate::foreign_providers::TrafficFlowProvider for CountingForeignTrafficFlowProvider {
        async fn fetch_traffic_flow(
            &self,
            _bounding_box: crate::records::BoundingBox,
        ) -> Result<Vec<crate::records::TrafficFlowData>, SdkError> {
            self.call_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(vec![crate::records::TrafficFlowData {
                shape_coordinates: vec![crate::records::GeographicCoordinate {
                    latitude: 38.57,
                    longitude: -121.50,
                }],
                current_speed_kilometers_per_hour: Some(40.0),
                free_flow_speed_kilometers_per_hour: Some(60.0),
                congestion_level: crate::records::TrafficFlowCongestionLevel::Slow,
            }])
        }
    }

    /// A native traffic-flow implementation that counts its own calls.
    struct CountingNativeTrafficFlowProvider {
        call_count: std::sync::Arc<std::sync::atomic::AtomicU64>,
    }

    #[async_trait::async_trait]
    impl geospatial_traits::TrafficFlowProvider for CountingNativeTrafficFlowProvider {
        async fn fetch_traffic_flow(
            &self,
            _bounding_box: &geospatial_types::BoundingBox,
        ) -> Result<Vec<geospatial_types::TrafficFlowData>, geospatial_types::SDKError> {
            self.call_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(vec![geospatial_types::TrafficFlowData {
                shape_coordinates: vec![geospatial_types::GeographicCoordinate {
                    latitude: 38.57,
                    longitude: -121.50,
                }],
                current_speed_kilometers_per_hour: Some(40.0),
                free_flow_speed_kilometers_per_hour: Some(60.0),
                congestion_level: geospatial_types::TrafficFlowCongestionLevel::Slow,
            }])
        }
    }

    #[tokio::test]
    async fn a_foreign_traffic_flow_provider_serves_every_request_uncached() {
        let call_count = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let map_sdk = MapSdk::new();
        map_sdk.register_traffic_flow_provider_from_foreign(std::sync::Arc::new(
            CountingForeignTrafficFlowProvider {
                call_count: std::sync::Arc::clone(&call_count),
            },
        ));
        for _ in 0..2 {
            map_sdk
                .fetch_traffic_flow(traffic_flow_test_bounding_box())
                .await
                .expect("the foreign provider serves every request");
        }
        assert_eq!(
            call_count.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "a host-supplied provider owns its freshness, so no SDK cache answers the repeat"
        );
    }

    #[tokio::test]
    async fn the_registry_installed_traffic_flow_provider_still_caches_a_repeated_box() {
        let call_count = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let map_sdk = MapSdk::new();
        map_sdk.register_traffic_flow_provider(std::sync::Arc::new(
            CountingNativeTrafficFlowProvider {
                call_count: std::sync::Arc::clone(&call_count),
            },
        ));
        for _ in 0..2 {
            map_sdk
                .fetch_traffic_flow(traffic_flow_test_bounding_box())
                .await
                .expect("the registry-installed provider serves the box");
        }
        assert_eq!(
            call_count.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "the SDK grid cache answers the repeated box for its own providers"
        );

        // The cache handle the registry keeps makes the same answer readable:
        // one miss asked the provider, one hit did not, and the budget the
        // cache enforces comes back beside the counters.
        let statistics = map_sdk
            .traffic_flow_cache_statistics()
            .expect("a registry-installed traffic-flow provider serves through the grid cache");
        assert_eq!(statistics.miss_count, 1, "the first box asked the provider");
        assert_eq!(
            statistics.hit_count, 1,
            "the repeated box came from the cache"
        );
        assert_eq!(
            statistics.entry_count, 1,
            "one grid cell answered both boxes"
        );
        assert_eq!(
            statistics.eviction_count, 0,
            "one cell stays far inside the entry budget"
        );
        assert!(
            statistics.maximum_entry_count > 0,
            "the readback must carry the entry budget the cache enforces"
        );
        assert!(statistics.time_to_live_seconds > 0);
    }

    #[tokio::test]
    async fn a_foreign_traffic_flow_provider_reports_no_cache_statistics() {
        let call_count = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let map_sdk = MapSdk::new();
        assert!(
            map_sdk.traffic_flow_cache_statistics().is_none(),
            "with nothing registered there is no cache to read"
        );
        map_sdk.register_traffic_flow_provider_from_foreign(std::sync::Arc::new(
            CountingForeignTrafficFlowProvider { call_count },
        ));
        assert!(
            map_sdk.traffic_flow_cache_statistics().is_none(),
            "a host-supplied provider owns its freshness, so no SDK cache reports"
        );
    }

    /// A foreign vector-tile implementation serving one fixed tile and
    /// reporting a descriptor.
    struct FixedForeignVectorTileProvider;

    #[async_trait::async_trait]
    impl crate::foreign_providers::VectorTileProvider for FixedForeignVectorTileProvider {
        fn source_descriptor(&self) -> crate::records::VectorTileSourceDescriptor {
            crate::records::VectorTileSourceDescriptor {
                source_name: "application/test-tiles".to_string(),
                attribution_text: "Test vector tiles".to_string(),
                minimum_zoom_level: 0,
                maximum_zoom_level: 14,
                tile_encoding: crate::records::VectorTileEncoding::MapboxVectorTile,
                style_hints: vec!["openmaptiles".to_string()],
            }
        }

        async fn fetch_vector_tile(
            &self,
            tile_x: u32,
            tile_y: u32,
            zoom_level: u8,
        ) -> Result<crate::records::VectorTileData, SdkError> {
            Ok(crate::records::VectorTileData {
                tile_x,
                tile_y,
                zoom_level,
                tile_encoding: crate::records::VectorTileEncoding::MapboxVectorTile,
                tile_bytes: vec![0x1a, 0x00],
            })
        }
    }

    #[test]
    fn a_foreign_vector_tile_provider_fills_the_slot_and_records_the_application_key() {
        let map_sdk = MapSdk::new();
        assert!(
            !map_sdk.vector_tile_provider_is_registered(),
            "an empty SDK reports no vector-tile provider"
        );
        map_sdk.register_vector_tile_provider_from_foreign(std::sync::Arc::new(
            FixedForeignVectorTileProvider,
        ));
        assert!(map_sdk.vector_tile_provider_is_registered());
        let vector_registration = map_sdk
            .provider_registrations()
            .into_iter()
            .find(|registration| registration.contract == ProviderContract::VectorTiles)
            .expect("every capability is reported");
        assert!(vector_registration.registered);
        assert_eq!(
            vector_registration.provider_key,
            super::APPLICATION_PROVIDER_KEY,
            "the selection names the application key the vector source resolves"
        );
    }

    #[tokio::test]
    async fn a_foreign_vector_tile_provider_serves_its_descriptor_and_tiles() {
        let map_sdk = MapSdk::new();
        map_sdk.register_vector_tile_provider_from_foreign(std::sync::Arc::new(
            FixedForeignVectorTileProvider,
        ));
        let vector_tile_provider = map_sdk
            .required_vector_tile_provider()
            .expect("the registered provider resolves");
        let source_descriptor = vector_tile_provider.source_descriptor();
        assert_eq!(source_descriptor.source_name, "application/test-tiles");
        assert_eq!(source_descriptor.attribution_text, "Test vector tiles");
        assert_eq!(source_descriptor.maximum_zoom_level, 14);
        let tile = vector_tile_provider
            .fetch_vector_tile(3, 4, 5)
            .await
            .expect("the foreign provider serves the tile");
        assert_eq!(tile.tile_x, 3);
        assert_eq!(tile.tile_y, 4);
        assert_eq!(tile.zoom_level, 5);
        assert_eq!(tile.tile_bytes, vec![0x1a, 0x00]);
        assert_eq!(
            tile.tile_encoding,
            geospatial_types::VectorTileEncoding::MapboxVectorTile
        );
    }
}
