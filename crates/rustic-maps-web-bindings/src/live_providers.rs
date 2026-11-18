//! Live provider registration on the wasm seam.
//!
//! The browser surface serves the contract's provider calls through the same
//! per-capability registry the native targets use: `rustic-maps-core`'s
//! [`ProviderRegistry`] keeps one slot per contract, so a page may serve
//! geocoding from one provider and raster tiles from another. A page names a
//! capability and a provider key from the shared provider registry
//! ([`configure_provider`]); the dispatch table in [`install_provider`] is the
//! only place a vendor is named, and adding a provider adds its crate
//! dependency and one match arm.
//!
//! The registry is module-local and single-threaded; no credential is
//! persisted or logged. The exports below answer the contract's provider
//! calls — forward and reverse geocoding, search, routing and its capability
//! readback, distance matrices, traffic incidents and flow, elevation, raster
//! tiles, the registration readback and the URL-template raster source — with
//! the same records and refusal vocabulary the native bindings carry.
//!
//! Two seams exist only on the browser. [`set_provider_base_url`] points a
//! registered provider's requests at a same-origin route, which is how the
//! test harness serves recorded provider payloads; a deployed page never
//! needs it. `clear_provider_registrations` empties every slot, the browser
//! counterpart of the settings surface's Clear act, because the contract has
//! no clearing member.

use crate::exports::{
    ERROR_CODE_INVALID_ARGUMENT, ERROR_CODE_INVALID_COORDINATE_INPUT,
    ERROR_CODE_PROVIDER_NOT_CONFIGURED, ERROR_CODE_PROVIDER_REQUEST_REJECTED,
    ERROR_CODE_UNSUPPORTED_CAPABILITY, failure_with_code, object_from_fields, route_data_object,
    sdk_error_failure,
};
use crate::shared::validated_coordinate;
use everymap_core::domains::traffic as everymap_traffic;
use everymap_providers_mapquest::{
    MapQuestAutocompleteProvider, MapQuestClient, MapQuestDistanceMatrix, MapQuestGeocoder,
    MapQuestPlaceSearchProvider, MapQuestProviderConfiguration, MapQuestRasterTileProvider,
    MapQuestRouter, MapQuestTrafficProvider,
};
use geospatial_traits::{RasterTileProvider, VectorTileProvider};
use geospatial_types::route::{
    AvoidOptions, RouteCapabilities, RouteOptions, RouteStrategy, TransportationMode, UnitSystem,
};
use geospatial_types::{
    BoundingBox, DistanceMatrix, ElevationPoint, PlaceResult, RasterImageFormat,
    RasterTileResolution, SDKError, TrafficFlowData, TrafficIncident,
};
use js_sys::{Array, Object, Uint8Array};
use rustic_maps_core::everymap_bridge::{
    EveryMapDistanceMatrixProvider, EveryMapGeocoder, EveryMapPlaceSearchProvider,
    EveryMapRasterTileProvider, EveryMapRouter, EveryMapTrafficProvider,
    EveryMapVectorTileProvider, map_provider_error, tile_source_budget_rates,
};
use rustic_maps_core::provider_observation::{
    DEFAULT_REFILL_TOKENS_PER_SECOND, DEFAULT_TOKEN_BUCKET_CAPACITY_TOKENS, EveryMapRequestObserver,
};
use rustic_maps_core::{
    GridCachedTrafficFlowProvider, ProviderContract, ProviderRegistry, StyleResourceClient,
    TrafficFlowCacheConfiguration, TrafficFlowCurrentTimeSeconds, TrafficFlowGridCache,
};
use std::cell::{Cell, RefCell};
use std::sync::Arc;
use wasm_bindgen::prelude::*;

/// Raster format the live tile call requests: the Tiles API's PNG form, the
/// encoding of the committed tile fixtures the browser canvas renders.
const LIVE_RASTER_TILE_IMAGE_FORMAT: RasterImageFormat = RasterImageFormat::Png;
/// Raster resolution the live tile call requests: the standard 256-pixel
/// edge (`256/png` in the provider path), matching the recorded tile samples.
const LIVE_RASTER_TILE_RESOLUTION: RasterTileResolution = RasterTileResolution::Standard;
/// The dispatch key of the MapQuest provider crate, equal to the shared
/// registry's `wasm_registration` value.
const MAPQUEST_PROVIDER_KEY: &str = "mapquest";
/// The catalog's display name for [`MAPQUEST_PROVIDER_KEY`].
const MAPQUEST_DISPLAY_NAME: &str = "MapQuest";
/// The dispatch key of the OSRM provider crate, equal to the shared registry's
/// `wasm_registration` value.
const OSRM_PROVIDER_KEY: &str = "osrm";
/// The registry display name for [`OSRM_PROVIDER_KEY`].
const OSRM_DISPLAY_NAME: &str = "OSRM";
/// The dispatch key of the Photon provider crate, equal to the shared
/// registry's provider key.
const PHOTON_PROVIDER_KEY: &str = "photon";
/// The registry display name for [`PHOTON_PROVIDER_KEY`].
const PHOTON_DISPLAY_NAME: &str = "Photon";
/// The dispatch key of the Valhalla provider crate, equal to the shared
/// registry's `wasm_registration` value.
const VALHALLA_PROVIDER_KEY: &str = "valhalla";
/// The registry display name for [`VALHALLA_PROVIDER_KEY`].
const VALHALLA_DISPLAY_NAME: &str = "Valhalla";
/// The largest departure timestamp JavaScript can carry exactly (the safe
/// integer limit); a larger value would silently lose seconds in the f64
/// round trip the seam accepts.
const MAXIMUM_DEPARTURE_TIMESTAMP_SECONDS: f64 = 9_007_199_254_740_991.0;
/// Lowest zoom a raster source serves when its declaration omits the field:
/// the whole world in one tile.
const DEFAULT_RASTER_SOURCE_MINIMUM_ZOOM_LEVEL: u8 = 0;
/// Highest zoom a raster source serves when its declaration omits the field:
/// the deepest zoom the shipped provider serves.
const DEFAULT_RASTER_SOURCE_MAXIMUM_ZOOM_LEVEL: u8 = 22;

/// What one provider selection records: which key serves which capability,
/// and the display name a settings surface shows for it.
struct ProviderSelection {
    /// The capability the selection serves.
    contract: ProviderContract,
    /// The provider's stable registry key.
    provider_key: String,
    /// The provider's display name.
    display_name: String,
}

/// A browser-only request-redirection seam: the base URL a provider key
/// addresses instead of its public endpoint. The test harness points a
/// registered provider at the same-origin fixture routes with it.
struct ProviderBaseUrlOverride {
    /// The provider key the override applies to.
    provider_key: String,
    /// The base URL the provider's client is built with.
    base_url: String,
}

/// The module's provider state: the registry every live call resolves
/// through, the selections the readback reports, and the browser-only base
/// URL overrides.
struct ProviderState {
    /// One slot per provider contract.
    registry: ProviderRegistry,
    /// Which provider serves each configured capability.
    selections: Vec<ProviderSelection>,
    /// Base URL overrides, by provider key.
    base_url_overrides: Vec<ProviderBaseUrlOverride>,
}

thread_local! {
    /// The module's provider state (a registry with every slot empty until a
    /// page configures one).
    static PROVIDER_STATE: RefCell<ProviderState> = RefCell::new(ProviderState {
        registry: ProviderRegistry::new(),
        selections: Vec::new(),
        base_url_overrides: Vec::new(),
    });
    /// The raster-tile provider the generated contract registry installed.
    /// When the canvas ships inside the generated module's crate graph, a
    /// source registered through the generated `MapSdk` and the canvas' live
    /// fallback are the same tile source: the binding crate mirrors every
    /// raster-tile registration here, and the canvas consults this slot
    /// before its own registration. Absent when the canvas runs on its own.
    static SHARED_RASTER_TILE_PROVIDER: RefCell<Option<Arc<dyn RasterTileProvider>>> =
        const { RefCell::new(None) };
    /// The vector-tile provider the generated contract registry installed:
    /// the mirror that lets a provider-backed base installed through the
    /// generated `MapSdk` and one configured through the page's own surface
    /// resolve to the same source. Absent when the canvas runs on its own.
    static SHARED_VECTOR_TILE_PROVIDER: RefCell<Option<Arc<dyn VectorTileProvider>>> =
        const { RefCell::new(None) };
    /// The authenticated style-resource fetch the generated contract registry
    /// installed beside the vector-tile provider: the mirror that lets a
    /// provider-backed base installed through the generated `MapSdk` load the
    /// provider's own style document. Absent when the canvas runs on its own.
    static SHARED_VECTOR_STYLE_RESOURCES: RefCell<Option<Arc<dyn StyleResourceClient>>> =
        const { RefCell::new(None) };
    /// The hosts the provider declared for the style resources it publishes,
    /// mirrored beside the shared fetch so a base installed through the
    /// generated `MapSdk` applies the same credential scope the canvas does.
    static SHARED_VECTOR_STYLE_RESOURCE_HOSTS: RefCell<Vec<String>> =
        const { RefCell::new(Vec::new()) };
    /// The page-registered URL-template raster source: the keyless fallback.
    /// It serves only while no provider is configured for the raster-tile
    /// contract, so a credential and the fallback can never fight over one
    /// slot whatever order their asynchronous registrations run in.
    static LIVE_RASTER_FALLBACK_PROVIDER: RefCell<Option<Arc<dyn RasterTileProvider>>> =
        const { RefCell::new(None) };
    /// The zoom ceiling the canvas' live raster fallback fetches at, as the
    /// fallback source declares it. Absent until a fallback is registered, so
    /// the canvas keeps its recorded-fixture ceiling.
    static LIVE_RASTER_MAXIMUM_ZOOM_LEVEL: RefCell<Option<u8>> = const { RefCell::new(None) };
    /// The traffic-flow cache the serving traffic provider answers from, kept
    /// so a page can read its hit and miss counters.
    static TRAFFIC_FLOW_CACHE: RefCell<Option<Arc<TrafficFlowGridCache>>> =
        const { RefCell::new(None) };
    /// Monotonic revision of the provider a raster tile request resolves
    /// through. Every registration, configuration or clear that can change
    /// the serving source bumps it; the canvas stamps its in-flight fetches
    /// with the revision they started under and discards a result that
    /// resolves after a later revision, so no payload from a previous source
    /// stages into the pool.
    static RASTER_TILE_PROVIDER_REGISTRATION_REVISION: Cell<u64> = const { Cell::new(0) };
}

/// Forgets the serving traffic-flow cache.
///
/// A provider that cannot be wrapped serves uncached; before it takes the
/// slot, any cache a previous provider installed is cleared so a statistics
/// readback never reports counters for a provider the page no longer serves.
fn forget_traffic_flow_cache() {
    TRAFFIC_FLOW_CACHE.with(|cache_slot| {
        *cache_slot.borrow_mut() = None;
    });
}

/// Marks a change to the provider raster tile requests resolve through.
fn bump_raster_tile_provider_registration_revision() {
    RASTER_TILE_PROVIDER_REGISTRATION_REVISION.with(|revision_slot| {
        revision_slot.set(revision_slot.get().wrapping_add(1));
    });
}

/// The current revision of the raster provider registration. A consumer that
/// pools decoded payloads or retry schedules compares the revision it built
/// them under against this value and discards state from an older source.
pub(crate) fn raster_tile_provider_registration_revision() -> u64 {
    RASTER_TILE_PROVIDER_REGISTRATION_REVISION.with(|revision_slot| revision_slot.get())
}

/// Runs `operation` against the module state under one borrow.
fn with_provider_state<T>(operation: impl FnOnce(&mut ProviderState) -> T) -> T {
    PROVIDER_STATE.with(|state_slot| operation(&mut state_slot.borrow_mut()))
}

/// Builds the MapQuest provider client from a runtime credential; a base URL
/// override points it at a same-origin route instead of the public platform
/// endpoint.
fn mapquest_provider_client(
    credential: String,
    base_url: Option<String>,
    contract: ProviderContract,
) -> Arc<MapQuestClient> {
    let mut configuration = MapQuestProviderConfiguration {
        api_key: credential,
        ..MapQuestProviderConfiguration::default()
    };
    if let Some(override_base_url) = base_url
        && !override_base_url.trim().is_empty()
    {
        configuration.base_url = override_base_url;
    }
    let mut client = MapQuestClient::new(configuration);
    // The provider layer reports each attempt to its own observer; the
    // adapter renders that into the SDK's diagnostics vocabulary with the
    // contract this client serves.
    client.set_observer(Arc::new(EveryMapRequestObserver::new(
        contract.contract_name(),
        MAPQUEST_PROVIDER_KEY,
    )));
    Arc::new(client)
}

/// Whether a shared-table contract names this binding's contract.
fn catalog_contract_matches(
    catalog_contract: rustic_maps_providers::Contract,
    binding_contract: ProviderContract,
) -> bool {
    use rustic_maps_providers::Contract as CatalogContract;
    matches!(
        (catalog_contract, binding_contract),
        (CatalogContract::Geocoding, ProviderContract::Geocoding)
            | (CatalogContract::Routing, ProviderContract::Routing)
            | (
                CatalogContract::DistanceMatrix,
                ProviderContract::DistanceMatrix
            )
            | (CatalogContract::PlaceSearch, ProviderContract::PlaceSearch)
            | (
                CatalogContract::TrafficIncidents,
                ProviderContract::TrafficIncidents
            )
            | (CatalogContract::TrafficFlow, ProviderContract::TrafficFlow)
            | (CatalogContract::RasterTiles, ProviderContract::RasterTiles)
            | (CatalogContract::VectorTiles, ProviderContract::VectorTiles)
    )
}

/// Every credentialed vendor the browser dispatch can construct. The
/// display name, credential shape and served contracts come from the shared
/// provider crate, so a provider cannot be advertised on one target and
/// refused on another.
const BROWSER_VENDOR_KEYS: &[&str] = &["here", "google", "tomtom", "mapbox", "radar"];

/// Registers one capability of a vendor's implementation set in the browser
/// registry.
fn install_vendor_contract(
    state: &mut ProviderState,
    contract: ProviderContract,
    implementations: &rustic_maps_providers::VendorImplementations,
) -> bool {
    let is_served = match contract {
        ProviderContract::Geocoding => implementations.geocoder.is_some(),
        ProviderContract::Routing => implementations.router.is_some(),
        ProviderContract::TrafficIncidents | ProviderContract::TrafficFlow => {
            implementations.traffic.is_some()
        }
        ProviderContract::RasterTiles => implementations.raster_tiles.is_some(),
        ProviderContract::VectorTiles => implementations.vector_tiles.is_some(),
        ProviderContract::DistanceMatrix
        | ProviderContract::PlaceSearch
        | ProviderContract::Elevation
        | ProviderContract::OfflineRouting => false,
    };
    if !is_served {
        return false;
    }
    match contract {
        ProviderContract::Geocoding => {
            if let Some(geocoder) = implementations.geocoder.as_ref() {
                state
                    .registry
                    .register_geocoding_provider(Arc::new(EveryMapGeocoder::new(Arc::clone(
                        geocoder,
                    ))));
            }
        }
        ProviderContract::Routing => {
            if let Some(router) = implementations.router.as_ref() {
                state
                    .registry
                    .register_routing_provider(Arc::new(EveryMapRouter::new(Arc::clone(router))));
            }
        }
        ProviderContract::TrafficIncidents => {
            if let Some(traffic) = implementations.traffic.as_ref() {
                state.registry.register_traffic_incident_provider(Arc::new(
                    EveryMapTrafficProvider::new(Arc::clone(traffic)),
                ));
            }
        }
        ProviderContract::TrafficFlow => {
            if let Some(traffic) = implementations.traffic.as_ref() {
                register_traffic_flow_with_cache(state, Arc::clone(traffic));
            }
        }
        ProviderContract::RasterTiles => {
            if let Some(raster_tiles) = implementations.raster_tiles.as_ref() {
                state.registry.register_raster_tile_provider(Arc::new(
                    EveryMapRasterTileProvider::new(Arc::clone(raster_tiles)),
                ));
                // The canvas' live fallback fetches at the declared ceiling;
                // without it a vendor base stops at the recorded-fixture zoom.
                set_live_raster_maximum_zoom_level(Some(DEFAULT_RASTER_SOURCE_MAXIMUM_ZOOM_LEVEL));
            }
        }
        ProviderContract::VectorTiles => {
            if let Some(vector_tiles) = implementations.vector_tiles.as_ref() {
                state.registry.register_vector_tile_provider(Arc::new(
                    EveryMapVectorTileProvider::new(Arc::clone(vector_tiles)),
                ));
                if let Some(style_resources) = implementations.style_resources.as_ref() {
                    state.registry.register_vector_tile_style_resources(
                        Arc::clone(style_resources),
                        implementations
                            .style_resource_hosts
                            .iter()
                            .map(|host| host.to_string())
                            .collect(),
                    );
                }
            }
        }
        ProviderContract::DistanceMatrix
        | ProviderContract::PlaceSearch
        | ProviderContract::Elevation
        | ProviderContract::OfflineRouting => {}
    }
    true
}

/// Wraps a traffic-flow provider in the grid-keyed cache the browser registry
/// answers settled-camera repeats from.
fn register_traffic_flow_with_cache(
    state: &mut ProviderState,
    traffic: Arc<dyn everymap_traffic::TrafficProvider>,
) {
    let traffic_flow_provider: Arc<dyn geospatial_traits::TrafficFlowProvider> =
        Arc::new(EveryMapTrafficProvider::new(traffic));
    let traffic_flow_current_time_seconds: TrafficFlowCurrentTimeSeconds =
        Arc::new(browser_current_time_seconds);
    match GridCachedTrafficFlowProvider::new(
        Arc::clone(&traffic_flow_provider),
        TrafficFlowCacheConfiguration::default(),
        traffic_flow_current_time_seconds,
    ) {
        Ok(cached_traffic_flow_provider) => {
            // The statistics readback answers from this handle, so a vendor
            // traffic provider reports the same counters MapQuest does.
            TRAFFIC_FLOW_CACHE.with(|cache_slot| {
                *cache_slot.borrow_mut() = Some(cached_traffic_flow_provider.cache());
            });
            state
                .registry
                .register_traffic_flow_provider(Arc::new(cached_traffic_flow_provider));
        }
        // The default configuration is valid by construction, so this arm is
        // unreachable in practice; serving uncached is the honest fallback.
        // A previous provider's cache slot is cleared with it, so the
        // statistics readback never reports counters for the uncached one.
        Err(_) => {
            forget_traffic_flow_cache();
            state
                .registry
                .register_traffic_flow_provider(traffic_flow_provider)
        }
    }
}

/// The base URL override a provider key was given, if any.
fn base_url_override_for(state: &ProviderState, provider_key: &str) -> Option<String> {
    state
        .base_url_overrides
        .iter()
        .find(|override_entry| override_entry.provider_key == provider_key)
        .map(|override_entry| override_entry.base_url.clone())
}

/// The base URL override a provider key was given, if any.
///
/// This is the read side of [`set_provider_base_url`], carried across the
/// crate boundary so the generated contract registry in
/// `rustic-maps-native-core` installs the same fixture-backed provider the
/// browser canvas' own registration builds. It stays browser-only and
/// exists for a page that serves a provider's routes itself, such as the
/// test harness; a deployed page never sets an override.
#[cfg(target_arch = "wasm32")]
pub fn base_url_override(provider_key: &str) -> Option<String> {
    PROVIDER_STATE.with(|state_slot| base_url_override_for(&state_slot.borrow(), provider_key))
}

/// Records which provider serves a capability, replacing any earlier
/// selection for it.
fn record_selection(
    state: &mut ProviderState,
    contract: ProviderContract,
    provider_key: &str,
    display_name: &str,
) {
    state
        .selections
        .retain(|selection| selection.contract != contract);
    state.selections.push(ProviderSelection {
        contract,
        provider_key: provider_key.to_string(),
        display_name: display_name.to_string(),
    });
}

/// Records the provider the generated contract registry chose for a
/// capability, so the canvas' own provider-key checks (a provider-backed
/// vector base's descriptor, a raster selection) read the same choice the
/// page made through `MapSdk`.
///
/// `rustic-maps-native-core` mirrors every successful registration here on
/// wasm, beside the provider handle it publishes to the canvas' shared
/// slots. A native build has no canvas to inform.
pub fn record_shared_provider_selection(
    contract: ProviderContract,
    provider_key: &str,
    display_name: &str,
) {
    with_provider_state(|state| {
        record_selection(state, contract, provider_key, display_name);
    });
}

/// Installs one provider into the registry slot for one contract. This match
/// is the whole compiled-provider dispatch table: adding a provider adds its
/// crate dependency and its arms here, and nothing else in this module names
/// a vendor.
fn install_provider(
    state: &mut ProviderState,
    contract: ProviderContract,
    provider_key: &str,
    credential: &str,
) -> Result<(), JsValue> {
    // A key entered with surrounding whitespace is the same key on every
    // target; the native catalog trims before installing, so this seam does
    // too rather than failing a provider's own validation.
    let credential = credential.trim();
    if provider_key == OSRM_PROVIDER_KEY || provider_key == VALHALLA_PROVIDER_KEY {
        if contract != ProviderContract::Routing {
            return Err(failure_with_code(
                ERROR_CODE_UNSUPPORTED_CAPABILITY,
                &format!(
                    "the {provider_key} provider serves no {} capability",
                    contract_name(contract)
                ),
            ));
        }
        let base_url = base_url_override_for(state, provider_key)
            .unwrap_or_else(|| credential.trim().to_string());
        if base_url.is_empty() {
            return Err(failure_with_code(
                ERROR_CODE_PROVIDER_REQUEST_REJECTED,
                &format!("the {provider_key} provider requires a deployment address"),
            ));
        }
        // A keyless routing provider's construction does not validate its
        // address, so the registration refuses one that is not an http(s) URL
        // with a host instead of failing on the first route request.
        let has_deployment_scheme =
            base_url.starts_with("http://") || base_url.starts_with("https://");
        let host_and_path = base_url
            .split_once("://")
            .map(|(_scheme, remainder)| remainder)
            .unwrap_or_default();
        if !has_deployment_scheme || host_and_path.is_empty() || host_and_path.starts_with('/') {
            return Err(failure_with_code(
                ERROR_CODE_PROVIDER_REQUEST_REJECTED,
                &format!(
                    "the {provider_key} deployment address {base_url} must be an http or https \
                     URL with a host"
                ),
            ));
        }
        if provider_key == OSRM_PROVIDER_KEY {
            let provider = everymap_providers_osrm::OsrmRouter::with_observer(
                everymap_providers_osrm::OsrmProviderConfiguration {
                    base_url,
                    ..Default::default()
                },
                Arc::new(EveryMapRequestObserver::new(
                    contract.contract_name(),
                    OSRM_PROVIDER_KEY,
                )),
            );
            state
                .registry
                .register_routing_provider(Arc::new(EveryMapRouter::new(Arc::new(provider))));
            record_selection(state, contract, provider_key, OSRM_DISPLAY_NAME);
        } else {
            let provider = everymap_providers_valhalla::ValhallaRouter::with_observer(
                everymap_providers_valhalla::ValhallaProviderConfiguration {
                    base_url,
                    ..Default::default()
                },
                Arc::new(EveryMapRequestObserver::new(
                    contract.contract_name(),
                    VALHALLA_PROVIDER_KEY,
                )),
            );
            state
                .registry
                .register_routing_provider(Arc::new(EveryMapRouter::new(Arc::new(provider))));
            record_selection(state, contract, provider_key, VALHALLA_DISPLAY_NAME);
        }
        return Ok(());
    }
    if provider_key == PHOTON_PROVIDER_KEY {
        if !matches!(
            contract,
            ProviderContract::Geocoding | ProviderContract::PlaceSearch
        ) {
            return Err(failure_with_code(
                ERROR_CODE_UNSUPPORTED_CAPABILITY,
                &format!(
                    "the {provider_key} provider serves no {} capability",
                    contract_name(contract)
                ),
            ));
        }
        let base_url = base_url_override_for(state, provider_key).unwrap_or_else(|| {
            let trimmed_credential = credential.trim();
            if trimmed_credential.is_empty() {
                everymap_providers_photon::DEFAULT_PHOTON_BASE_URL.to_string()
            } else {
                trimmed_credential.to_string()
            }
        });
        let mut photon_client = everymap_providers_photon::PhotonClient::new(
            everymap_providers_photon::PhotonProviderConfiguration {
                base_url,
                ..Default::default()
            },
        );
        photon_client.set_observer(Arc::new(EveryMapRequestObserver::new(
            contract.contract_name(),
            PHOTON_PROVIDER_KEY,
        )));
        let client = Arc::new(photon_client);
        match contract {
            ProviderContract::Geocoding => {
                state
                    .registry
                    .register_geocoding_provider(Arc::new(EveryMapGeocoder::new(Arc::new(
                        everymap_providers_photon::PhotonGeocoder::new(Arc::clone(&client)),
                    ))));
            }
            ProviderContract::PlaceSearch => {
                state.registry.register_search_place_provider(Arc::new(
                    EveryMapPlaceSearchProvider::new(
                        Arc::new(everymap_providers_photon::PhotonPlaceSearchProvider::new(
                            Arc::clone(&client),
                        )),
                        Arc::new(everymap_providers_photon::PhotonAutocompleteProvider::new(
                            client,
                        )),
                    ),
                ));
            }
            ProviderContract::Routing
            | ProviderContract::DistanceMatrix
            | ProviderContract::TrafficIncidents
            | ProviderContract::TrafficFlow
            | ProviderContract::Elevation
            | ProviderContract::RasterTiles
            | ProviderContract::VectorTiles
            | ProviderContract::OfflineRouting => {
                return Err(failure_with_code(
                    ERROR_CODE_UNSUPPORTED_CAPABILITY,
                    &format!(
                        "the {provider_key} provider serves no {} capability",
                        contract_name(contract)
                    ),
                ));
            }
        }
        record_selection(state, contract, provider_key, PHOTON_DISPLAY_NAME);
        return Ok(());
    }
    if BROWSER_VENDOR_KEYS.contains(&provider_key) {
        let shared_entry = rustic_maps_providers::entry_for_key(provider_key)
            .expect("every browser vendor is tabled in the shared provider crate");
        if !shared_entry
            .served_contracts
            .iter()
            .any(|catalog_contract| catalog_contract_matches(*catalog_contract, contract))
        {
            return Err(failure_with_code(
                ERROR_CODE_UNSUPPORTED_CAPABILITY,
                &format!(
                    "the {provider_key} provider serves no {} capability",
                    contract_name(contract)
                ),
            ));
        }
        if credential.trim().is_empty() {
            return Err(failure_with_code(
                ERROR_CODE_PROVIDER_REQUEST_REJECTED,
                &format!(
                    "the {} provider requires a credential",
                    shared_entry.display_name
                ),
            ));
        }
        let base_url = base_url_override_for(state, provider_key);
        let implementations = rustic_maps_providers::build_vendor(
            provider_key,
            credential,
            base_url.as_deref(),
            contract.contract_name(),
        )
        .expect("every browser vendor key has a shared builder");
        if !install_vendor_contract(state, contract, &implementations) {
            return Err(failure_with_code(
                ERROR_CODE_PROVIDER_NOT_CONFIGURED,
                &format!(
                    "the {provider_key} provider carries no {} implementation",
                    contract_name(contract)
                ),
            ));
        }
        record_selection(state, contract, provider_key, shared_entry.display_name);
        return Ok(());
    }
    if provider_key != MAPQUEST_PROVIDER_KEY {
        return Err(failure_with_code(
            ERROR_CODE_PROVIDER_NOT_CONFIGURED,
            &format!(
                "no provider implementation is compiled for the provider key \"{provider_key}\""
            ),
        ));
    }
    if credential.trim().is_empty() {
        return Err(failure_with_code(
            ERROR_CODE_PROVIDER_REQUEST_REJECTED,
            &format!("the {MAPQUEST_DISPLAY_NAME} provider requires a credential"),
        ));
    }
    let base_url = base_url_override_for(state, provider_key);
    let provider_client = mapquest_provider_client(credential.to_string(), base_url, contract);
    match contract {
        ProviderContract::Geocoding => {
            state
                .registry
                .register_geocoding_provider(Arc::new(EveryMapGeocoder::new(Arc::new(
                    MapQuestGeocoder::new(Arc::clone(&provider_client)),
                ))))
        }
        ProviderContract::Routing => {
            state
                .registry
                .register_routing_provider(Arc::new(EveryMapRouter::new(Arc::new(
                    MapQuestRouter::new(Arc::clone(&provider_client)),
                ))))
        }
        ProviderContract::DistanceMatrix => state.registry.register_distance_matrix_provider(
            Arc::new(EveryMapDistanceMatrixProvider::new(Arc::new(
                MapQuestDistanceMatrix::new(Arc::clone(&provider_client)),
            ))),
        ),
        ProviderContract::PlaceSearch => state.registry.register_search_place_provider(Arc::new(
            EveryMapPlaceSearchProvider::new(
                Arc::new(MapQuestPlaceSearchProvider::new(Arc::clone(
                    &provider_client,
                ))),
                Arc::new(MapQuestAutocompleteProvider::new(provider_client)),
            ),
        )),
        ProviderContract::TrafficIncidents => state.registry.register_traffic_incident_provider(
            Arc::new(EveryMapTrafficProvider::new(Arc::new(
                MapQuestTrafficProvider::new(Arc::clone(&provider_client)),
            ))),
        ),
        ProviderContract::TrafficFlow => {
            // The grid-keyed, memory-bounded traffic cache wraps every
            // traffic-flow provider, exactly as the native registration does:
            // a settled camera asks for the same pane again and is answered
            // from memory.
            let traffic_flow_provider = EveryMapTrafficProvider::new(Arc::new(
                MapQuestTrafficProvider::new(Arc::clone(&provider_client)),
            ));
            let traffic_flow_current_time_seconds: TrafficFlowCurrentTimeSeconds =
                Arc::new(browser_current_time_seconds);
            match GridCachedTrafficFlowProvider::new(
                Arc::new(traffic_flow_provider),
                TrafficFlowCacheConfiguration::default(),
                traffic_flow_current_time_seconds,
            ) {
                Ok(cached_traffic_flow_provider) => {
                    TRAFFIC_FLOW_CACHE.with(|cache_slot| {
                        *cache_slot.borrow_mut() = Some(cached_traffic_flow_provider.cache());
                    });
                    state
                        .registry
                        .register_traffic_flow_provider(Arc::new(cached_traffic_flow_provider))
                }
                // The default configuration is valid by construction; a
                // provider that cannot be wrapped serves uncached rather than
                // not at all. Clearing the slot keeps stale counters from a
                // provider the page no longer serves out of the readback.
                Err(_cache_configuration_error) => {
                    forget_traffic_flow_cache();
                    state.registry.register_traffic_flow_provider(Arc::new(
                        EveryMapTrafficProvider::new(Arc::new(MapQuestTrafficProvider::new(
                            provider_client,
                        ))),
                    ))
                }
            }
        }
        ProviderContract::RasterTiles => {
            state.registry.register_raster_tile_provider(Arc::new(
                EveryMapRasterTileProvider::new(Arc::new(MapQuestRasterTileProvider::new(
                    provider_client,
                ))),
            ));
            set_live_raster_maximum_zoom_level(Some(DEFAULT_RASTER_SOURCE_MAXIMUM_ZOOM_LEVEL));
        }
        ProviderContract::Elevation
        | ProviderContract::VectorTiles
        | ProviderContract::OfflineRouting => {
            return Err(failure_with_code(
                ERROR_CODE_UNSUPPORTED_CAPABILITY,
                &format!(
                    "the {MAPQUEST_DISPLAY_NAME} provider serves no {} capability",
                    contract_name(contract)
                ),
            ));
        }
    }
    record_selection(state, contract, provider_key, MAPQUEST_DISPLAY_NAME);
    Ok(())
}

/// Reads the serving traffic-flow cache's counters: `{hitCount, missCount,
/// entryCount, evictionCount, segmentCount, gridZoomLevel,
/// maximumEntryCount, timeToLiveSeconds}`, or null while no traffic provider
/// is registered. A page's developer console shows these; nothing mutates
/// through this call.
#[wasm_bindgen(js_name = trafficFlowCacheStatistics)]
pub fn traffic_flow_cache_statistics() -> Result<JsValue, JsValue> {
    let Some(traffic_flow_cache) =
        TRAFFIC_FLOW_CACHE.with(|cache_slot| cache_slot.borrow().clone())
    else {
        // `null`, not `undefined`: the documented shape is a nullable record.
        return Ok(JsValue::NULL);
    };
    let statistics = traffic_flow_cache.statistics();
    let configuration = traffic_flow_cache.configuration();
    // The counters arrive as JavaScript numbers, not BigInt: `JsValue::from`
    // maps a 64-bit integer to a `bigint`, and a page comparing a count
    // against a number would then never match.
    Ok(object_from_fields(&[
        ("hitCount", JsValue::from_f64(statistics.hit_count as f64)),
        ("missCount", JsValue::from_f64(statistics.miss_count as f64)),
        (
            "entryCount",
            JsValue::from_f64(statistics.entry_count as f64),
        ),
        (
            "evictionCount",
            JsValue::from_f64(statistics.eviction_count as f64),
        ),
        (
            "segmentCount",
            JsValue::from_f64(statistics.segment_count as f64),
        ),
        (
            "gridZoomLevel",
            JsValue::from(configuration.grid_zoom_level),
        ),
        (
            "maximumEntryCount",
            JsValue::from_f64(configuration.maximum_entry_count as f64),
        ),
        (
            "timeToLiveSeconds",
            JsValue::from_f64(configuration.time_to_live_seconds as f64),
        ),
    ])?
    .into())
}

/// The browser's clock in whole seconds: `Date.now()` is milliseconds since
/// the epoch, and the standard library's clock panics on this target.
fn browser_current_time_seconds() -> u64 {
    (js_sys::Date::now() / 1000.0) as u64
}

/// The contract's name for diagnostics and readbacks, matching the UDL enum.
fn contract_name(contract: ProviderContract) -> &'static str {
    match contract {
        ProviderContract::Geocoding => "Geocoding",
        ProviderContract::Routing => "Routing",
        ProviderContract::DistanceMatrix => "DistanceMatrix",
        ProviderContract::PlaceSearch => "PlaceSearch",
        ProviderContract::TrafficIncidents => "TrafficIncidents",
        ProviderContract::TrafficFlow => "TrafficFlow",
        ProviderContract::Elevation => "Elevation",
        ProviderContract::RasterTiles => "RasterTiles",
        ProviderContract::VectorTiles => "VectorTiles",
        ProviderContract::OfflineRouting => "OfflineRouting",
    }
}

/// Reads a contract name the way a page spells it: the UDL enum's name, the
/// registry's `…Provider` name, or the plain lowercase capability name.
fn provider_contract_from_name(contract_name_text: &str) -> Result<ProviderContract, JsValue> {
    let normalized: String = contract_name_text
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    let normalized = normalized
        .strip_suffix("provider")
        .unwrap_or(&normalized)
        .to_string();
    match normalized.as_str() {
        "geocoding" => Ok(ProviderContract::Geocoding),
        "routing" => Ok(ProviderContract::Routing),
        "distancematrix" => Ok(ProviderContract::DistanceMatrix),
        "placesearch" | "searchplace" => Ok(ProviderContract::PlaceSearch),
        "trafficincidents" | "trafficincident" => Ok(ProviderContract::TrafficIncidents),
        "trafficflow" => Ok(ProviderContract::TrafficFlow),
        "elevation" => Ok(ProviderContract::Elevation),
        "rastertiles" | "rastertile" => Ok(ProviderContract::RasterTiles),
        "vectortiles" | "vectortile" => Ok(ProviderContract::VectorTiles),
        "offlinerouting" => Ok(ProviderContract::OfflineRouting),
        _ => Err(failure_with_code(
            ERROR_CODE_INVALID_ARGUMENT,
            &format!("\"{contract_name_text}\" is not a provider contract"),
        )),
    }
}

/// Chooses the provider that serves one capability, leaving every other
/// capability untouched — the contract's `MapSdk::configureProvider`.
///
/// `contract` is the capability's name (`Geocoding`, `Routing`,
/// `DistanceMatrix`, `PlaceSearch`, `TrafficIncidents`, `TrafficFlow`,
/// `Elevation`, `RasterTiles`, `VectorTiles` or `OfflineRouting`; the
/// registry's `…Provider` spelling is accepted too). `providerKey` is a
/// provider this build carries, and `credential` its key — an empty string is
/// accepted only for a provider that needs none.
///
/// Fails with `ProviderNotConfigured` when the key is unknown to this build,
/// `ProviderRequestRejected` when the provider needs a credential and none
/// was supplied, and `UnsupportedCapability` when the key serves no such
/// capability. A refused call leaves the previously serving provider in
/// place.
#[wasm_bindgen(js_name = configureProvider)]
pub fn configure_provider(
    contract: String,
    provider_key: String,
    credential: String,
) -> Result<(), JsValue> {
    let provider_contract = provider_contract_from_name(&contract)?;
    let registration_outcome = with_provider_state(|state| {
        install_provider(state, provider_contract, &provider_key, &credential)
    });
    if registration_outcome.is_ok() && provider_contract == ProviderContract::RasterTiles {
        bump_raster_tile_provider_registration_revision();
    }
    registration_outcome
}

/// Registers every capability the provider named by `provider_key` serves
/// from one credential — the contract's `MapSdk.configure_provider_set`.
///
/// The served list comes from the shared provider table, so a capability a
/// provider does not serve is never installed into a slot it cannot fill. A
/// provider that requires a credential refuses a blank one; the credential
/// lives in module memory only and is never logged. Configure one capability at
/// a time with [`configure_provider`] to mix providers across capabilities.
#[wasm_bindgen(js_name = configureProviderSet)]
pub fn configure_provider_set(provider_key: String, credential: String) -> Result<(), JsValue> {
    let Some(provider_entry) = rustic_maps_providers::entry_for_key(&provider_key) else {
        return Err(failure_with_code(
            ERROR_CODE_UNSUPPORTED_CAPABILITY,
            &format!("provider key {provider_key} is not built into this SDK"),
        ));
    };
    if provider_entry.credential_required && credential.trim().is_empty() {
        return Err(failure_with_code(
            ERROR_CODE_PROVIDER_REQUEST_REJECTED,
            &format!(
                "the {} provider requires a credential before it can serve requests",
                provider_entry.display_name
            ),
        ));
    }
    let registration_outcome = with_provider_state(|state| {
        for contract in ProviderContract::ALL {
            if provider_entry
                .served_contracts
                .iter()
                .any(|catalog_contract| catalog_contract_matches(*catalog_contract, contract))
            {
                install_provider(state, contract, &provider_key, &credential)?;
            }
        }
        Ok(())
    });
    let installs_raster_tiles = provider_entry
        .served_contracts
        .iter()
        .any(|catalog_contract| {
            catalog_contract_matches(*catalog_contract, ProviderContract::RasterTiles)
        });
    if registration_outcome.is_ok() && installs_raster_tiles {
        bump_raster_tile_provider_registration_revision();
    }
    registration_outcome
}

/// Points a provider key's requests at a base URL instead of its public
/// endpoint — a browser-only seam for a page that serves the provider's
/// routes itself, such as the test harness. Register the provider after
/// calling this; the override applies to every later registration of the key.
#[wasm_bindgen(js_name = setProviderBaseUrl)]
pub fn set_provider_base_url(provider_key: String, base_url: String) -> Result<(), JsValue> {
    if base_url.trim().is_empty() {
        return Err(failure_with_code(
            ERROR_CODE_INVALID_ARGUMENT,
            "a provider base URL must not be empty",
        ));
    }
    with_provider_state(|state| {
        state
            .base_url_overrides
            .retain(|override_entry| override_entry.provider_key != provider_key);
        state.base_url_overrides.push(ProviderBaseUrlOverride {
            provider_key,
            base_url: base_url.trim().to_string(),
        });
        Ok(())
    })
}

/// Empties every provider slot and forgets the selections — the browser
/// counterpart of the settings surface's Clear act. The contract carries no
/// clearing member, and a page that clears keeps nothing afterwards.
#[wasm_bindgen(js_name = clearProviderRegistrations)]
pub fn clear_provider_registrations() {
    with_provider_state(|state| {
        *state = ProviderState {
            registry: ProviderRegistry::new(),
            selections: Vec::new(),
            base_url_overrides: std::mem::take(&mut state.base_url_overrides),
        };
    });
    // The traffic-flow cache belonged to the provider that just went away;
    // keeping it would answer later traffic reads from a provider the page
    // no longer serves.
    TRAFFIC_FLOW_CACHE.with(|cache_slot| {
        *cache_slot.borrow_mut() = None;
    });
    // The page's URL-template fallback survives a clear, and so does its
    // fetch ceiling: a cleared page keeps drawing live tiles.
    bump_raster_tile_provider_registration_revision();
}

/// Reports which provider serves each capability, in the contract's stable
/// order, as `{contract, providerKey, displayName, registered}` rows. Every
/// capability appears, including the ones with no provider installed; those
/// carry empty texts and `registered` false.
#[wasm_bindgen(js_name = providerRegistrations)]
pub fn provider_registrations() -> Result<Array, JsValue> {
    with_provider_state(|state| {
        let registration_array = Array::new();
        for contract in ProviderContract::ALL {
            let selection = state
                .selections
                .iter()
                .find(|selection| selection.contract == contract);
            let registered = state.registry.is_configured(contract);
            let registration_object = object_from_fields(&[
                ("contract", JsValue::from_str(contract_name(contract))),
                (
                    "providerKey",
                    JsValue::from_str(
                        selection
                            .filter(|_| registered)
                            .map_or("", |selection| selection.provider_key.as_str()),
                    ),
                ),
                (
                    "displayName",
                    JsValue::from_str(
                        selection
                            .filter(|_| registered)
                            .map_or("", |selection| selection.display_name.as_str()),
                    ),
                ),
                ("registered", JsValue::from_bool(registered)),
            ])?;
            registration_array.push(&registration_object.into());
        }
        Ok(registration_array)
    })
}

/// Reports which transport modes and which route strategy the serving routing
/// provider can honour — the contract's `MapSdk::routeCapabilities`.
///
/// A surface reads this answer so it can disable a choice the provider cannot
/// serve with a reason, instead of sending the request and receiving a route
/// from a path the reader did not ask for. Fails with
/// `ProviderNotConfigured` while no routing provider is installed.
#[wasm_bindgen(js_name = routeCapabilities)]
pub fn route_capabilities() -> Result<Object, JsValue> {
    let routing_provider = with_provider_state(|state| state.registry.require_routing_provider())
        .map_err(|error| sdk_error_failure(&error))?;
    route_capabilities_object(&routing_provider.route_capabilities())
}

/// Maps the capability record to its plain JavaScript shape: the UDL enum
/// names as text.
fn route_capabilities_object(capabilities: &RouteCapabilities) -> Result<Object, JsValue> {
    let modes_array = Array::new();
    for transportation_mode in &capabilities.supported_transportation_modes {
        modes_array.push(&JsValue::from_str(&format!("{transportation_mode:?}")));
    }
    object_from_fields(&[
        ("supportedTransportationModes", modes_array.into()),
        (
            "supportsShortestRouteStrategy",
            JsValue::from_bool(capabilities.supports_shortest_route_strategy),
        ),
    ])
}

/// Resolves a coordinate into candidate places describing that position
/// through the installed geocoding provider — the contract's
/// `MapSdk::reverseGeocode` in the same row shape [`crate::live_providers::forward_geocode_live`]
/// answers.
#[wasm_bindgen(js_name = reverseGeocode)]
pub async fn reverse_geocode(
    latitude_degrees: f64,
    longitude_degrees: f64,
) -> Result<Array, JsValue> {
    let geocoding_provider =
        with_provider_state(|state| state.registry.require_geocoding_provider())
            .map_err(|error| sdk_error_failure(&error))?;
    let coordinate = validated_coordinate(latitude_degrees, longitude_degrees)
        .map_err(|message| failure_with_code(ERROR_CODE_INVALID_COORDINATE_INPUT, &message))?;
    let place_rows = geocoding_provider
        .reverse_geocode(&coordinate)
        .await
        .map_err(|error| sdk_error_failure(&error))?;
    place_rows_array(place_rows)
}

/// Installs the raster-tile provider the generated contract registry holds,
/// so the canvas' live fallback fetches through the same source the contract
/// surface serves. `None` clears the shared slot. The binding crate mirrors
/// every registration here; the canvas consults it before its own
/// registration, so one provider serves both surfaces.
pub fn set_shared_raster_tile_provider(provider: Option<Arc<dyn RasterTileProvider>>) {
    SHARED_RASTER_TILE_PROVIDER.with(|provider_slot| {
        *provider_slot.borrow_mut() = provider;
    });
    bump_raster_tile_provider_registration_revision();
}

/// Records the zoom ceiling the canvas' live raster fallback should fetch at.
fn set_live_raster_maximum_zoom_level(maximum_zoom_level: Option<u8>) {
    LIVE_RASTER_MAXIMUM_ZOOM_LEVEL.with(|zoom_slot| {
        *zoom_slot.borrow_mut() = maximum_zoom_level;
    });
}

/// The zoom ceiling the canvas' live raster fallback fetches at, when a
/// raster source is installed, and `None` while the canvas serves only its
/// recorded fixtures.
pub(crate) fn live_raster_maximum_zoom_level() -> Option<u8> {
    LIVE_RASTER_MAXIMUM_ZOOM_LEVEL.with(|zoom_slot| *zoom_slot.borrow())
}

/// The raster-tile provider the canvas' live fallback fetches through: the
/// shared slot the contract registry mirrors first, then the configured
/// provider for the raster-tile contract, then the page-registered
/// URL-template fallback. A configured provider always outranks the
/// fallback, so a credential is never displaced by the keyless source.
/// Absent while nothing serves, in which case the canvas draws only its
/// recorded fixtures.
pub(crate) fn registered_raster_tile_provider() -> Option<Arc<dyn RasterTileProvider>> {
    SHARED_RASTER_TILE_PROVIDER
        .with(|provider_slot| provider_slot.borrow().clone())
        .or_else(|| with_provider_state(|state| state.registry.require_raster_tile_provider().ok()))
        .or_else(|| {
            LIVE_RASTER_FALLBACK_PROVIDER.with(|provider_slot| provider_slot.borrow().clone())
        })
}

/// The page-registered URL-template raster source, if any.
///
/// The generated contract registry in `rustic-maps-native-core` reads this on
/// wasm when its own raster-tile slot is empty, so the keyless fallback the
/// canvas draws also answers a generated `fetch_raster_tile`. It is the
/// browser-only counterpart of the shared slot mirror, and a page that
/// configured a provider never reaches it.
pub fn browser_raster_tile_fallback() -> Option<Arc<dyn RasterTileProvider>> {
    LIVE_RASTER_FALLBACK_PROVIDER.with(|provider_slot| provider_slot.borrow().clone())
}

/// Clears every provider slot the canvas consults on the generated contract
/// registry's behalf — the browser-only companion of the page's Clear act.
///
/// A page that replaces its `MapSdk` drops the native registry, but the
/// mirrors it published to the canvas would otherwise keep serving. The
/// page-registered URL-template fallback survives, exactly as it survives
/// [`clear_provider_registrations`].
#[wasm_bindgen(js_name = clearSharedProviderSlots)]
pub fn clear_shared_provider_slots() {
    SHARED_RASTER_TILE_PROVIDER.with(|provider_slot| {
        *provider_slot.borrow_mut() = None;
    });
    SHARED_VECTOR_TILE_PROVIDER.with(|provider_slot| {
        *provider_slot.borrow_mut() = None;
    });
    SHARED_VECTOR_STYLE_RESOURCES.with(|style_slot| {
        *style_slot.borrow_mut() = None;
    });
    SHARED_VECTOR_STYLE_RESOURCE_HOSTS.with(|hosts_slot| hosts_slot.borrow_mut().clear());
    // The selections the generated registry mirrored are its choices too: a
    // cleared page names no serving provider.
    with_provider_state(|state| state.selections.clear());
    bump_raster_tile_provider_registration_revision();
}

/// Installs the vector-tile provider the generated contract registry holds,
/// so a provider-backed base installed through that surface resolves to the
/// same source the canvas draws. `None` clears the shared slot.
pub fn set_shared_vector_tile_provider(provider: Option<Arc<dyn VectorTileProvider>>) {
    SHARED_VECTOR_TILE_PROVIDER.with(|provider_slot| {
        *provider_slot.borrow_mut() = provider;
    });
}

/// The vector-tile provider the module serves: the shared slot the contract
/// registry mirrors first, then the provider configured through the page's
/// own registration. `None` while nothing serves the capability.
pub(crate) fn registered_vector_tile_provider() -> Option<Arc<dyn VectorTileProvider>> {
    SHARED_VECTOR_TILE_PROVIDER
        .with(|provider_slot| provider_slot.borrow().clone())
        .or_else(|| with_provider_state(|state| state.registry.require_vector_tile_provider().ok()))
}

/// Installs the authenticated style-resource fetch and the hosts the
/// provider declared for it, so a base installed through the generated
/// contract registry loads the provider's own style document and resources.
/// `None` clears both shared slots.
pub fn set_shared_vector_style_resources(
    style_resources: Option<Arc<dyn StyleResourceClient>>,
    declared_hosts: Vec<String>,
) {
    SHARED_VECTOR_STYLE_RESOURCES.with(|style_slot| {
        *style_slot.borrow_mut() = style_resources;
    });
    SHARED_VECTOR_STYLE_RESOURCE_HOSTS.with(|hosts_slot| {
        *hosts_slot.borrow_mut() = declared_hosts;
    });
}

/// The style-resource fetch the module serves: the shared slot the contract
/// registry mirrors first, then the one registered through the page's own
/// surface. `None` while the vector-tile provider publishes no hosted style.
pub(crate) fn registered_vector_style_resources() -> Option<Arc<dyn StyleResourceClient>> {
    SHARED_VECTOR_STYLE_RESOURCES
        .with(|style_slot| style_slot.borrow().clone())
        .or_else(|| {
            with_provider_state(|state| state.registry.require_vector_tile_style_resources().ok())
        })
}

/// The hosts the serving provider declared for its published style
/// resources: the shared slot the contract registry mirrors first, then the
/// one registered through the page's own surface.
pub(crate) fn registered_vector_style_resource_hosts() -> Vec<String> {
    let shared_hosts =
        SHARED_VECTOR_STYLE_RESOURCE_HOSTS.with(|hosts_slot| hosts_slot.borrow().clone());
    if shared_hosts.is_empty() {
        with_provider_state(|state| state.registry.vector_tile_style_resource_hosts())
    } else {
        shared_hosts
    }
}

/// The provider key the module recorded for one capability, or `None` while
/// nothing is configured for it. The canvas uses it to refuse a
/// provider-backed source that names a different provider.
pub(crate) fn selected_provider_key(contract: ProviderContract) -> Option<String> {
    with_provider_state(|state| {
        state
            .selections
            .iter()
            .find(|selection| selection.contract == contract)
            .map(|selection| selection.provider_key.clone())
    })
}

/// Forward-geocodes through the installed geocoding provider, answering the
/// same row shape the native reference applications consume. Fails with a
/// message when no provider is registered or the provider refuses the query.
#[wasm_bindgen(js_name = forwardGeocode)]
pub async fn forward_geocode_live(query_text: String) -> Result<Array, JsValue> {
    let geocoding_provider =
        with_provider_state(|state| state.registry.require_geocoding_provider())
            .map_err(|error| sdk_error_failure(&error))?;
    let place_rows = geocoding_provider
        .forward_geocode(&query_text)
        .await
        .map_err(|error| sdk_error_failure(&error))?;
    place_rows_array(place_rows)
}

/// Returns fast completion candidates while the visitor types, through the
/// installed search-place provider — the same rows the native reference apps
/// consume from `MapSdk::suggest`. Fails with a verbatim message when no
/// provider is registered or the provider refuses the query.
#[wasm_bindgen(js_name = suggestPlaces)]
pub async fn suggest_places(query_text: String) -> Result<Array, JsValue> {
    search_place_rows(query_text, None, true).await
}

/// [`suggest_places`] narrowed to a ranking boundary (the contract's
/// `MapSdk::suggest` boundary argument).
#[wasm_bindgen(js_name = suggestPlacesInBoundary)]
pub async fn suggest_places_in_boundary(
    query_text: String,
    south_west_latitude: f64,
    south_west_longitude: f64,
    north_east_latitude: f64,
    north_east_longitude: f64,
) -> Result<Array, JsValue> {
    let search_boundary = validated_search_boundary(
        south_west_latitude,
        south_west_longitude,
        north_east_latitude,
        north_east_longitude,
    )?;
    search_place_rows(query_text, Some(search_boundary), true).await
}

/// Returns full search results for a settled query through the installed
/// search-place provider — the same rows the native reference apps consume
/// from `MapSdk::search`.
#[wasm_bindgen(js_name = searchPlaces)]
pub async fn search_places(query_text: String) -> Result<Array, JsValue> {
    search_place_rows(query_text, None, false).await
}

/// [`search_places`] narrowed to a ranking boundary (the contract's
/// `MapSdk::search` boundary argument).
#[wasm_bindgen(js_name = searchPlacesInBoundary)]
pub async fn search_places_in_boundary(
    query_text: String,
    south_west_latitude: f64,
    south_west_longitude: f64,
    north_east_latitude: f64,
    north_east_longitude: f64,
) -> Result<Array, JsValue> {
    let search_boundary = validated_search_boundary(
        south_west_latitude,
        south_west_longitude,
        north_east_latitude,
        north_east_longitude,
    )?;
    search_place_rows(query_text, Some(search_boundary), false).await
}

/// Answers one search-place call from the installed provider: autocomplete
/// for a typing visitor, the full result set for a settled query.
async fn search_place_rows(
    query_text: String,
    search_boundary: Option<BoundingBox>,
    is_autocomplete: bool,
) -> Result<Array, JsValue> {
    let search_place_provider =
        with_provider_state(|state| state.registry.require_search_place_provider())
            .map_err(|error| sdk_error_failure(&error))?;
    let place_rows = if is_autocomplete {
        search_place_provider
            .search_autocomplete(&query_text, search_boundary)
            .await
    } else {
        search_place_provider
            .search_places(&query_text, search_boundary)
            .await
    }
    .map_err(|error| sdk_error_failure(&error))?;
    place_rows_array(place_rows)
}

/// Validates the four corner scalars a boundary-taking search call accepts
/// and builds the vendor-neutral box (inverted or out-of-range corners are
/// refused verbatim).
fn validated_search_boundary(
    south_west_latitude: f64,
    south_west_longitude: f64,
    north_east_latitude: f64,
    north_east_longitude: f64,
) -> Result<BoundingBox, JsValue> {
    let south_west = validated_coordinate(south_west_latitude, south_west_longitude)
        .map_err(|message| failure_with_code(ERROR_CODE_INVALID_COORDINATE_INPUT, &message))?;
    let north_east = validated_coordinate(north_east_latitude, north_east_longitude)
        .map_err(|message| failure_with_code(ERROR_CODE_INVALID_COORDINATE_INPUT, &message))?;
    BoundingBox::from_corners(south_west, north_east).map_err(|error| sdk_error_failure(&error))
}

/// Maps the vendor-neutral place rows to the plain JavaScript row shape every
/// wasm geocoding/search call answers (the native contract's `Place` field
/// names, lower-camel-cased).
fn place_rows_array(place_rows: Vec<PlaceResult>) -> Result<Array, JsValue> {
    let row_array = Array::new();
    for place_row in place_rows {
        let row_object = object_from_fields(&[
            (
                "structuredAddressText",
                JsValue::from_str(&place_row.structured_address_text),
            ),
            (
                "latitudeDegrees",
                JsValue::from(place_row.coordinate.latitude),
            ),
            (
                "longitudeDegrees",
                JsValue::from(place_row.coordinate.longitude),
            ),
            (
                "qualityText",
                JsValue::from_str(&format!("{:?}", place_row.geocode_quality)),
            ),
        ])?;
        row_array.push(&row_object.into());
    }
    Ok(row_array)
}

/// Plans a two-waypoint route through the installed routing provider,
/// answering the same plain JavaScript shape
/// [`crate::exports::offline_route_from_archive_bytes`] does: `distanceMeters`,
/// `durationSeconds`, `boundingBox` and `legs` of `steps` carrying the stable
/// snake-case `maneuverType` label and the provider's end-anchored instruction
/// texts.
///
/// The request uses the SDK defaults: automobile, fastest, metric —
/// `RouteOptions::default()`, the same mode the recorded route sample was
/// produced with.
/// Failures throw an error carrying the verbatim detail and a stable code:
/// no registration, an invalid coordinate, or the provider's own refusal.
#[wasm_bindgen(js_name = calculateRoute)]
pub async fn calculate_route_live(
    origin_latitude: f64,
    origin_longitude: f64,
    destination_latitude: f64,
    destination_longitude: f64,
) -> Result<Object, JsValue> {
    route_answer(
        origin_latitude,
        origin_longitude,
        destination_latitude,
        destination_longitude,
        &RouteOptions::default(),
    )
    .await
}

/// Plans a two-waypoint route with the contract's full `RouteOptions`
/// (`MapSdk::calculate_route`'s options argument): transportation mode,
/// strategy, unit system, avoids and an optional departure instant. The
/// options arrive as one plain object; omitted fields take the contract
/// defaults, and an unknown enum name is refused before the provider is
/// asked. The answer is the same route shape [`calculate_route_live`]
/// produces.
#[wasm_bindgen(js_name = calculateRouteWithOptions)]
pub async fn calculate_route_with_options(
    origin_latitude: f64,
    origin_longitude: f64,
    destination_latitude: f64,
    destination_longitude: f64,
    options: JsValue,
) -> Result<Object, JsValue> {
    let route_options = route_options_from_object(&options)?;
    route_answer(
        origin_latitude,
        origin_longitude,
        destination_latitude,
        destination_longitude,
        &route_options,
    )
    .await
}

/// The shared route call behind both route exports.
async fn route_answer(
    origin_latitude: f64,
    origin_longitude: f64,
    destination_latitude: f64,
    destination_longitude: f64,
    route_options: &RouteOptions,
) -> Result<Object, JsValue> {
    let routing_provider = with_provider_state(|state| state.registry.require_routing_provider())
        .map_err(|error| sdk_error_failure(&error))?;
    let origin = validated_coordinate(origin_latitude, origin_longitude)
        .map_err(|message| failure_with_code(ERROR_CODE_INVALID_COORDINATE_INPUT, &message))?;
    let destination = validated_coordinate(destination_latitude, destination_longitude)
        .map_err(|message| failure_with_code(ERROR_CODE_INVALID_COORDINATE_INPUT, &message))?;
    let route_data = routing_provider
        .calculate_route(&[origin, destination], route_options)
        .await
        .map_err(|error| sdk_error_failure(&error))?;
    route_data_object(&route_data)
}

/// Reads one optional field from a JavaScript options object.
fn options_field(options: &JsValue, field_name: &str) -> Result<JsValue, JsValue> {
    js_sys::Reflect::get(options, &JsValue::from_str(field_name)).map_err(|_| {
        failure_with_code(
            ERROR_CODE_INVALID_ARGUMENT,
            &format!("the route options object refuses {field_name}"),
        )
    })
}

/// Reads an optional string field, falling back to `default_value` when the
/// field is absent or null.
fn options_text_field(
    options: &JsValue,
    field_name: &str,
    default_value: &str,
) -> Result<String, JsValue> {
    let field_value = options_field(options, field_name)?;
    if field_value.is_undefined() || field_value.is_null() {
        return Ok(default_value.to_string());
    }
    field_value.as_string().ok_or_else(|| {
        failure_with_code(
            ERROR_CODE_INVALID_ARGUMENT,
            &format!("the route option {field_name} must be a string"),
        )
    })
}

/// Reads an optional boolean field, defaulting to false.
fn options_boolean_field(options: &JsValue, field_name: &str) -> Result<bool, JsValue> {
    let field_value = options_field(options, field_name)?;
    if field_value.is_undefined() || field_value.is_null() {
        return Ok(false);
    }
    field_value.as_bool().ok_or_else(|| {
        failure_with_code(
            ERROR_CODE_INVALID_ARGUMENT,
            &format!("the route option {field_name} must be a boolean"),
        )
    })
}

/// Parses the plain `RouteOptions` object into the vendor-neutral record.
fn route_options_from_object(options: &JsValue) -> Result<RouteOptions, JsValue> {
    if !options.is_object() {
        return Err(failure_with_code(
            ERROR_CODE_INVALID_ARGUMENT,
            "the route options argument must be an object",
        ));
    }
    let transportation_mode = transportation_mode_from_name(&options_text_field(
        options,
        "transportationMode",
        "automobile",
    )?)?;
    let route_strategy = match options_text_field(options, "routeStrategy", "fastest")?
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "fastest" => RouteStrategy::Fastest,
        "shortest" => RouteStrategy::Shortest,
        _ => {
            return Err(failure_with_code(
                ERROR_CODE_INVALID_ARGUMENT,
                "the route strategy must be fastest or shortest",
            ));
        }
    };
    let unit_system = match options_text_field(options, "unitSystem", "metric")?
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "metric" => UnitSystem::Metric,
        "imperial" => UnitSystem::Imperial,
        _ => {
            return Err(failure_with_code(
                ERROR_CODE_INVALID_ARGUMENT,
                "the unit system must be metric or imperial",
            ));
        }
    };
    let avoid_field = options_field(options, "avoidOptions")?;
    let avoid_options = if avoid_field.is_undefined() || avoid_field.is_null() {
        AvoidOptions::default()
    } else if !avoid_field.is_object() {
        return Err(failure_with_code(
            ERROR_CODE_INVALID_ARGUMENT,
            "the avoidOptions field must be an object",
        ));
    } else {
        AvoidOptions {
            toll_roads: options_boolean_field(&avoid_field, "tollRoads")?,
            limited_access_highways: options_boolean_field(&avoid_field, "limitedAccessHighways")?,
            ferries: options_boolean_field(&avoid_field, "ferries")?,
            unpaved_roads: options_boolean_field(&avoid_field, "unpavedRoads")?,
        }
    };
    let departure_field = options_field(options, "departureTimestampSeconds")?;
    let departure_timestamp_seconds = if departure_field.is_undefined() || departure_field.is_null()
    {
        None
    } else {
        let departure_seconds = departure_field.as_f64().ok_or_else(|| {
            failure_with_code(
                ERROR_CODE_INVALID_ARGUMENT,
                "the departure timestamp must be a number of seconds",
            )
        })?;
        if !departure_seconds.is_finite()
            || departure_seconds < 0.0
            || departure_seconds > MAXIMUM_DEPARTURE_TIMESTAMP_SECONDS
        {
            return Err(failure_with_code(
                ERROR_CODE_INVALID_ARGUMENT,
                "the departure timestamp must be finite, non-negative and \
                 within the u64 second range",
            ));
        }
        Some(departure_seconds as u64)
    };
    Ok(RouteOptions {
        transportation_mode,
        route_strategy,
        unit_system,
        avoid_options,
        departure_timestamp_seconds,
    })
}

/// Calculates a travel-time matrix through the installed distance-matrix
/// provider — the contract's `MapSdk::calculateDistanceMatrix` on the wasm
/// seam. Coordinates arrive as flat `[latitude, longitude, …]` arrays; the
/// mode is the contract enum's name (or the common lowercase spelling).
/// Answers `{originIndex, destinationIndex, distanceMeters,
/// durationSeconds}` rows.
#[wasm_bindgen(js_name = calculateDistanceMatrix)]
pub async fn calculate_distance_matrix(
    origin_coordinates: Vec<f64>,
    destination_coordinates: Vec<f64>,
    transportation_mode: String,
) -> Result<Array, JsValue> {
    let distance_matrix_provider =
        with_provider_state(|state| state.registry.require_distance_matrix_provider())
            .map_err(|error| sdk_error_failure(&error))?;
    let origins = coordinate_pairs_array(&origin_coordinates, "origin")?;
    let destinations = coordinate_pairs_array(&destination_coordinates, "destination")?;
    let mode = transportation_mode_from_name(&transportation_mode)?;
    let matrix = distance_matrix_provider
        .calculate_distance_matrix(&origins, &destinations, mode)
        .await
        .map_err(|error| sdk_error_failure(&error))?;
    distance_matrix_entries_array(&matrix)
}

/// Reads a flat `[latitude, longitude, …]` array into validated coordinates.
fn coordinate_pairs_array(
    flat_values: &[f64],
    label: &str,
) -> Result<Vec<geospatial_types::GeographicCoordinate>, JsValue> {
    if flat_values.is_empty() || !flat_values.len().is_multiple_of(2) {
        return Err(failure_with_code(
            ERROR_CODE_INVALID_ARGUMENT,
            &format!(
                "{label} coordinates must be a non-empty, even-length \
                 [latitude, longitude, …] array"
            ),
        ));
    }
    flat_values
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            validated_coordinate(pair[0], pair[1])
                .map_err(|message| failure_with_code(ERROR_CODE_INVALID_COORDINATE_INPUT, &message))
        })
        .collect()
}

/// The contract's transportation-mode name, accepting the common lowercase
/// spellings the JavaScript side uses.
fn transportation_mode_from_name(name: &str) -> Result<TransportationMode, JsValue> {
    match name.trim().to_ascii_lowercase().as_str() {
        "automobile" | "car" => Ok(TransportationMode::Automobile),
        "bicycle" => Ok(TransportationMode::Bicycle),
        "pedestrian" | "walking" => Ok(TransportationMode::Pedestrian),
        _ => Err(failure_with_code(
            ERROR_CODE_INVALID_ARGUMENT,
            "the transportation mode must be automobile, bicycle or pedestrian",
        )),
    }
}

/// Maps the matrix record to its plain JavaScript rows.
fn distance_matrix_entries_array(matrix: &DistanceMatrix) -> Result<Array, JsValue> {
    let entry_array = Array::new();
    for entry in &matrix.entries {
        let entry_object = object_from_fields(&[
            ("originIndex", JsValue::from(entry.origin_index)),
            ("destinationIndex", JsValue::from(entry.destination_index)),
            ("distanceMeters", JsValue::from(entry.distance_meters)),
            ("durationSeconds", JsValue::from(entry.duration_seconds)),
        ])?;
        entry_array.push(&entry_object.into());
    }
    Ok(entry_array)
}

/// Fetches traffic disruptions intersecting the bounding box — the contract's
/// `MapSdk::fetchTrafficIncidents` on the wasm seam. The row shape is the
/// contract's `TrafficIncident` with lower-camel field names and enum names
/// as text (`incidentType`, `severity`).
#[wasm_bindgen(js_name = fetchTrafficIncidents)]
pub async fn fetch_traffic_incidents(
    south_west_latitude: f64,
    south_west_longitude: f64,
    north_east_latitude: f64,
    north_east_longitude: f64,
) -> Result<Array, JsValue> {
    let traffic_incident_provider =
        with_provider_state(|state| state.registry.require_traffic_incident_provider())
            .map_err(|error| sdk_error_failure(&error))?;
    let bounding_box = validated_search_boundary(
        south_west_latitude,
        south_west_longitude,
        north_east_latitude,
        north_east_longitude,
    )?;
    let incidents = traffic_incident_provider
        .fetch_traffic_incidents(&bounding_box)
        .await
        .map_err(|error| sdk_error_failure(&error))?;
    traffic_incidents_array(&incidents)
}

/// Fetches per-segment traffic flow inside the bounding box — the contract's
/// `MapSdk::fetchTrafficFlow` on the wasm seam. The row shape is the
/// contract's `TrafficFlowData` with lower-camel field names, speed values or
/// null, and the congestion level's enum name as text.
#[wasm_bindgen(js_name = fetchTrafficFlow)]
pub async fn fetch_traffic_flow(
    south_west_latitude: f64,
    south_west_longitude: f64,
    north_east_latitude: f64,
    north_east_longitude: f64,
) -> Result<Array, JsValue> {
    let traffic_flow_provider =
        with_provider_state(|state| state.registry.require_traffic_flow_provider())
            .map_err(|error| sdk_error_failure(&error))?;
    let bounding_box = validated_search_boundary(
        south_west_latitude,
        south_west_longitude,
        north_east_latitude,
        north_east_longitude,
    )?;
    let flow_rows = traffic_flow_provider
        .fetch_traffic_flow(&bounding_box)
        .await
        .map_err(|error| sdk_error_failure(&error))?;
    traffic_flow_array(&flow_rows)
}

/// Fetches elevation samples through the installed elevation provider — the
/// contract's `MapSdk::fetchElevationPoints` on the wasm seam. Coordinates
/// arrive as a flat `[latitude, longitude, …]` array; the answer rows carry
/// `latitude`, `longitude` and `elevationMeters`.
///
/// The provider set this build carries serves no elevation, so the call
/// answers `ProviderNotConfigured` until a page configures one; the refusal
/// names the contract, exactly as the native bindings' does.
#[wasm_bindgen(js_name = fetchElevationPoints)]
pub async fn fetch_elevation_points(point_coordinates: Vec<f64>) -> Result<Array, JsValue> {
    let elevation_provider =
        with_provider_state(|state| state.registry.require_elevation_provider())
            .map_err(|error| sdk_error_failure(&error))?;
    let points = coordinate_pairs_array(&point_coordinates, "elevation point")?;
    let elevation_points = elevation_provider
        .fetch_elevation_points(&points)
        .await
        .map_err(|error| sdk_error_failure(&error))?;
    elevation_points_array(&elevation_points)
}

/// Maps the elevation samples to their plain JavaScript rows.
fn elevation_points_array(elevation_points: &[ElevationPoint]) -> Result<Array, JsValue> {
    let point_array = Array::new();
    for elevation_point in elevation_points {
        let point_object = object_from_fields(&[
            (
                "latitude",
                JsValue::from(elevation_point.coordinate.latitude),
            ),
            (
                "longitude",
                JsValue::from(elevation_point.coordinate.longitude),
            ),
            (
                "elevationMeters",
                JsValue::from(elevation_point.elevation_meters),
            ),
        ])?;
        point_array.push(&point_object.into());
    }
    Ok(point_array)
}

/// One `{latitude, longitude}` object for a traffic row.
fn coordinate_object(
    coordinate: &geospatial_types::GeographicCoordinate,
) -> Result<Object, JsValue> {
    object_from_fields(&[
        ("latitude", JsValue::from(coordinate.latitude)),
        ("longitude", JsValue::from(coordinate.longitude)),
    ])
}

/// Maps the incident records to their plain JavaScript rows.
fn traffic_incidents_array(incidents: &[TrafficIncident]) -> Result<Array, JsValue> {
    let incident_array = Array::new();
    for incident in incidents {
        let start_coordinate = coordinate_object(&incident.start_coordinate)?;
        let end_coordinate = match &incident.end_coordinate {
            Some(coordinate) => coordinate_object(coordinate)?.into(),
            None => JsValue::NULL,
        };
        let incident_object = object_from_fields(&[
            ("identifier", JsValue::from_str(&incident.identifier)),
            (
                "incidentType",
                JsValue::from_str(&format!("{:?}", incident.incident_type)),
            ),
            (
                "severity",
                JsValue::from_str(&format!("{:?}", incident.severity)),
            ),
            (
                "shortDescriptionText",
                JsValue::from_str(&incident.short_description_text),
            ),
            ("startCoordinate", start_coordinate.into()),
            ("endCoordinate", end_coordinate),
        ])?;
        incident_array.push(&incident_object.into());
    }
    Ok(incident_array)
}

/// Maps the flow records to their plain JavaScript rows.
fn traffic_flow_array(flow_rows: &[TrafficFlowData]) -> Result<Array, JsValue> {
    let flow_array = Array::new();
    for flow_row in flow_rows {
        let shape_coordinates = Array::new();
        for coordinate in &flow_row.shape_coordinates {
            shape_coordinates.push(&coordinate_object(coordinate)?.into());
        }
        let current_speed = flow_row
            .current_speed_kilometers_per_hour
            .map_or(JsValue::NULL, JsValue::from);
        let free_flow_speed = flow_row
            .free_flow_speed_kilometers_per_hour
            .map_or(JsValue::NULL, JsValue::from);
        let flow_object = object_from_fields(&[
            ("shapeCoordinates", shape_coordinates.into()),
            ("currentSpeedKilometersPerHour", current_speed),
            ("freeFlowSpeedKilometersPerHour", free_flow_speed),
            (
                "congestionLevel",
                JsValue::from_str(&format!("{:?}", flow_row.congestion_level)),
            ),
        ])?;
        flow_array.push(&flow_object.into());
    }
    Ok(flow_array)
}

/// Registers a `{z}/{x}/{y}` raster tileset as the serving raster-tile
/// provider — the contract's `MapSdk::registerUrlTemplateRasterTileSource`.
///
/// The declaration arrives as one plain object with the contract's
/// `UrlTemplateRasterTileSource` fields, lower-camel-cased: `urlTemplate`,
/// `sourceName`, `minimumZoomLevel`, `maximumZoomLevel`, `imageFormat`
/// (`Png` or `Jpeg`), `tileResolution` (`Standard` or `Retina`), `userAgent`,
/// and the optional `tokenBucketCapacityTokens` / `refillTokensPerSecond`
/// pacing rates. Zoom levels and texts fall back to the documented defaults;
/// the format defaults to `Png`, the resolution to `Standard`, and absent
/// pacing fields to the SDK's documented bucket.
///
/// A malformed template, a blank source name, an inverted zoom range or an
/// unusable pacing rate throws `InvalidCoordinateInput` with a precise detail,
/// and the previous source keeps serving. The canvas' live raster fallback
/// fetches through the registered source at the zoom ceiling it declares.
#[wasm_bindgen(js_name = registerUrlTemplateRasterTileSource)]
pub fn register_url_template_raster_tile_source(source: JsValue) -> Result<(), JsValue> {
    if !source.is_object() {
        return Err(failure_with_code(
            ERROR_CODE_INVALID_ARGUMENT,
            "the raster tile source argument must be an object",
        ));
    }
    let url_template = required_source_text(&source, "urlTemplate")?;
    let source_name = required_source_text(&source, "sourceName")?;
    let minimum_zoom_level = source_zoom_level(
        &source,
        "minimumZoomLevel",
        DEFAULT_RASTER_SOURCE_MINIMUM_ZOOM_LEVEL,
    )?;
    let maximum_zoom_level = source_zoom_level(
        &source,
        "maximumZoomLevel",
        DEFAULT_RASTER_SOURCE_MAXIMUM_ZOOM_LEVEL,
    )?;
    let image_format = match source_text_field(&source, "imageFormat")?
        .unwrap_or_else(|| "png".to_string())
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => RasterImageFormat::Png,
        "jpeg" | "jpg" => RasterImageFormat::Jpeg,
        _ => {
            return Err(failure_with_code(
                ERROR_CODE_INVALID_ARGUMENT,
                "the raster tile image format must be png or jpeg",
            ));
        }
    };
    let tile_resolution = match source_text_field(&source, "tileResolution")?
        .unwrap_or_else(|| "standard".to_string())
        .to_ascii_lowercase()
        .as_str()
    {
        "standard" => RasterTileResolution::Standard,
        "retina" => RasterTileResolution::Retina,
        _ => {
            return Err(failure_with_code(
                ERROR_CODE_INVALID_ARGUMENT,
                "the raster tile resolution must be standard or retina",
            ));
        }
    };
    let user_agent = source_text_field(&source, "userAgent")?.unwrap_or_default();
    let (capacity_tokens, refill_tokens_per_second) = tile_source_budget_rates(
        &source_name,
        source_number_field(&source, "tokenBucketCapacityTokens")?
            .unwrap_or(DEFAULT_TOKEN_BUCKET_CAPACITY_TOKENS),
        source_number_field(&source, "refillTokensPerSecond")?
            .unwrap_or(DEFAULT_REFILL_TOKENS_PER_SECOND),
    )
    .map_err(|error| sdk_error_failure(&error))?;
    let pacing = everymap_providers_url_template_raster::TileSourcePacing::new(
        capacity_tokens,
        refill_tokens_per_second,
    )
    .with_maximum_wait_seconds(rustic_maps_core::everymap_bridge::TILE_SOURCE_MAXIMUM_WAIT_SECONDS);
    let headers = if user_agent.is_empty() {
        Vec::new()
    } else {
        vec![("User-Agent".to_string(), user_agent)]
    };
    let raster_tile_provider =
        everymap_providers_url_template_raster::UrlTemplateRasterTileProvider::new(
            everymap_providers_url_template_raster::UrlTemplateRasterTileSource {
                url_template,
                source_name: source_name.clone(),
                attribution_text: String::new(),
                terms_text: String::new(),
                minimum_zoom_level,
                maximum_zoom_level,
                image_format: match image_format {
                    RasterImageFormat::Png => {
                        everymap_core::domains::tiling::RasterImageFormat::Png
                    }
                    RasterImageFormat::Jpeg => {
                        everymap_core::domains::tiling::RasterImageFormat::Jpeg
                    }
                },
                tile_resolution: match tile_resolution {
                    RasterTileResolution::Standard => {
                        everymap_core::domains::tiling::RasterTileResolution::Standard
                    }
                    RasterTileResolution::Retina => {
                        everymap_core::domains::tiling::RasterTileResolution::Retina
                    }
                },
                subdomains: Vec::new(),
                headers,
                pacing: Some(pacing),
            },
        )
        .map_err(|error| sdk_error_failure(&map_provider_error(error)))?;
    LIVE_RASTER_FALLBACK_PROVIDER.with(|provider_slot| {
        *provider_slot.borrow_mut() = Some(Arc::new(EveryMapRasterTileProvider::new(Arc::new(
            raster_tile_provider,
        ))));
    });
    set_live_raster_maximum_zoom_level(Some(maximum_zoom_level));
    bump_raster_tile_provider_registration_revision();
    Ok(())
}

/// Removes the page-registered URL-template raster source. A configured
/// raster provider is untouched; with none, the canvas falls back to its
/// recorded fixtures.
#[wasm_bindgen(js_name = clearUrlTemplateRasterTileSource)]
pub fn clear_url_template_raster_tile_source() {
    LIVE_RASTER_FALLBACK_PROVIDER.with(|provider_slot| {
        *provider_slot.borrow_mut() = None;
    });
    set_live_raster_maximum_zoom_level(None);
    bump_raster_tile_provider_registration_revision();
}

/// Reads a required text field from a source declaration object.
fn required_source_text(source: &JsValue, field_name: &str) -> Result<String, JsValue> {
    let field_text = source_text_field(source, field_name)?;
    match field_text {
        Some(text) if !text.trim().is_empty() => Ok(text),
        _ => Err(failure_with_code(
            ERROR_CODE_INVALID_ARGUMENT,
            &format!("the raster tile source {field_name} must be a non-empty string"),
        )),
    }
}

/// Reads an optional text field from a source declaration object.
fn source_text_field(source: &JsValue, field_name: &str) -> Result<Option<String>, JsValue> {
    let field_value =
        js_sys::Reflect::get(source, &JsValue::from_str(field_name)).map_err(|_| {
            failure_with_code(
                ERROR_CODE_INVALID_ARGUMENT,
                &format!("the raster tile source object refuses {field_name}"),
            )
        })?;
    if field_value.is_undefined() || field_value.is_null() {
        return Ok(None);
    }
    field_value.as_string().map(Some).ok_or_else(|| {
        failure_with_code(
            ERROR_CODE_INVALID_ARGUMENT,
            &format!("the raster tile source {field_name} must be a string"),
        )
    })
}

/// Reads an optional numeric field from a source declaration object. An
/// absent (`undefined` or `null`) field is `None`; a present non-number is
/// refused. The provider validates the numeric range, because its refusal
/// names the source the value belongs to.
fn source_number_field(source: &JsValue, field_name: &str) -> Result<Option<f64>, JsValue> {
    let field_value =
        js_sys::Reflect::get(source, &JsValue::from_str(field_name)).map_err(|_| {
            failure_with_code(
                ERROR_CODE_INVALID_ARGUMENT,
                &format!("the raster tile source object refuses {field_name}"),
            )
        })?;
    if field_value.is_undefined() || field_value.is_null() {
        return Ok(None);
    }
    field_value.as_f64().map(Some).ok_or_else(|| {
        failure_with_code(
            ERROR_CODE_INVALID_ARGUMENT,
            &format!("the raster tile source {field_name} must be a number"),
        )
    })
}

/// Reads an optional zoom-level field, defaulting when absent and refusing a
/// value outside the `u8` zoom range.
fn source_zoom_level(
    source: &JsValue,
    field_name: &str,
    default_zoom_level: u8,
) -> Result<u8, JsValue> {
    let field_value =
        js_sys::Reflect::get(source, &JsValue::from_str(field_name)).map_err(|_| {
            failure_with_code(
                ERROR_CODE_INVALID_ARGUMENT,
                &format!("the raster tile source object refuses {field_name}"),
            )
        })?;
    if field_value.is_undefined() || field_value.is_null() {
        return Ok(default_zoom_level);
    }
    let zoom_level = field_value.as_f64().ok_or_else(|| {
        failure_with_code(
            ERROR_CODE_INVALID_ARGUMENT,
            &format!("the raster tile source {field_name} must be a number"),
        )
    })?;
    if !zoom_level.is_finite()
        || zoom_level.fract() != 0.0
        || !(0.0..=f64::from(u8::MAX)).contains(&zoom_level)
    {
        return Err(failure_with_code(
            ERROR_CODE_INVALID_ARGUMENT,
            &format!("the raster tile source {field_name} must be a whole zoom level"),
        ));
    }
    Ok(zoom_level as u8)
}

/// Fetches one raster tile through the installed raster-tile provider,
/// answering the PNG bytes as a `Uint8Array`.
///
/// The request is the standard-resolution PNG form of the provider's
/// documented tile path. Failures throw an error carrying the verbatim
/// detail and a stable code: no registration, a zoom or tile address outside
/// the provider's documented bounds, or the provider's own refusal.
#[wasm_bindgen(js_name = fetchRasterTile)]
pub async fn fetch_raster_tile_live(
    zoom_level: u8,
    tile_x: u32,
    tile_y: u32,
) -> Result<Uint8Array, JsValue> {
    // The same resolution the canvas uses: a configured provider first, the
    // page's URL-template fallback second.
    let raster_tile_provider = registered_raster_tile_provider().ok_or_else(|| {
        sdk_error_failure(&SDKError::ProviderNotConfigured(
            "RasterTileProvider".to_string(),
        ))
    })?;
    let tile_data = raster_tile_provider
        .fetch_raster_tile(
            tile_x,
            tile_y,
            zoom_level,
            LIVE_RASTER_TILE_IMAGE_FORMAT,
            LIVE_RASTER_TILE_RESOLUTION,
        )
        .await
        .map_err(|error| sdk_error_failure(&error))?;
    Ok(Uint8Array::from(tile_data.image_bytes.as_slice()))
}

#[cfg(test)]
mod tests {
    use super::{TRAFFIC_FLOW_CACHE, forget_traffic_flow_cache};
    use rustic_maps_core::{TrafficFlowCacheConfiguration, TrafficFlowGridCache};
    use std::sync::Arc;

    #[test]
    fn an_uncached_fallback_clears_the_previous_cache_readback() {
        let cache =
            TrafficFlowGridCache::new(TrafficFlowCacheConfiguration::default(), Arc::new(|| 0))
                .expect("the default cache configuration is valid");
        TRAFFIC_FLOW_CACHE.with(|cache_slot| {
            *cache_slot.borrow_mut() = Some(Arc::new(cache));
        });
        assert!(
            TRAFFIC_FLOW_CACHE.with(|cache_slot| cache_slot.borrow().is_some()),
            "the previous provider's cache is readable"
        );
        forget_traffic_flow_cache();
        assert!(
            TRAFFIC_FLOW_CACHE.with(|cache_slot| cache_slot.borrow().is_none()),
            "an uncached provider must not leave a previous provider's counters readable"
        );
    }
}
