//! The provider catalog: which provider keys this build carries, the
//! capabilities each serves, and the construction call for each.
//!
//! The registry in `rustic-maps-core` resolves calls per capability, so an
//! application can serve one capability from one provider and the next from
//! another. This module is the seam that makes that choice reachable from an
//! application: a key names a provider the binary was built with, and
//! [`ProviderCatalogEntry::install`] registers exactly the requested
//! capability, leaving every other capability's slot untouched.
//!
//! A provider's name appears here, in its own crate, in its sample-data
//! directory and in the shared registry descriptor — nowhere else. Adding a
//! provider is a crate, a descriptor, and one entry in [`PROVIDER_CATALOG`].

use crate::MapSdk;
use crate::error::SdkError;
use crate::records::{ProviderContract, ProviderRegistration};
use everymap_providers_mapquest::{
    MapQuestAutocompleteProvider, MapQuestClient, MapQuestDistanceMatrix, MapQuestGeocoder,
    MapQuestPlaceSearchProvider, MapQuestProviderConfiguration, MapQuestRasterTileProvider,
    MapQuestRouter, MapQuestTrafficProvider,
};
use rustic_maps_core::everymap_bridge::{
    EveryMapDistanceMatrixProvider, EveryMapGeocoder, EveryMapPlaceSearchProvider,
    EveryMapRasterTileProvider, EveryMapRouter, EveryMapTrafficProvider,
    EveryMapVectorTileProvider,
};
use rustic_maps_core::provider_observation::EveryMapRequestObserver;
use rustic_maps_providers::{Contract, PROVIDER_ENTRIES, ProviderEntry};
use std::sync::{Arc, Mutex};

/// The MapQuest terms' imagery-caching ceiling: raster-tile responses may be
/// stored for up to 30 calendar days.
pub(crate) const MAPQUEST_RASTER_TILE_CACHE_CEILING_SECONDS: u64 = 30 * 24 * 60 * 60;

/// The deepest zoom level the MapQuest Tiles API serves.
pub(crate) const MAPQUEST_RASTER_TILE_MAXIMUM_ZOOM_LEVEL: u8 = 20;

/// One provider this build can construct, keyed by its stable registry key.
pub(crate) struct ProviderCatalogEntry {
    /// The provider's stable key, matching its shared-registry descriptor.
    pub(crate) provider_key: &'static str,
    /// The provider's display name for a settings surface.
    pub(crate) display_name: &'static str,
    /// Whether the provider refuses to serve without a credential.
    pub(crate) credential_required: bool,
    /// The capabilities [`Self::install`] can register.
    pub(crate) served_contracts: &'static [ProviderContract],
    /// Registers the provider implementation for one capability on the SDK.
    pub(crate) install: fn(&MapSdk, ProviderContract, &str) -> Result<(), SdkError>,
    /// The provider's declared raster-tile caching-terms ceiling in seconds,
    /// or `None` when the provider declares no ceiling or serves no raster
    /// tiles. The tile-cache settings surface clamps every requested
    /// time-to-live through it. The wasm build installs no disk tile cache,
    /// so it never reads the value.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) raster_tile_cache_ceiling_seconds: Option<u64>,
    /// The deepest raster-tile zoom level the provider declares it serves, or
    /// `None` when the provider declares no maximum. An installed tile cache
    /// carries it into the tile-pack manager, which refuses a region deeper
    /// than the ceiling before downloading anything. The wasm build installs
    /// no disk tile cache, so it never reads the value.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) raster_tile_maximum_zoom_level: Option<u8>,
}

/// Compares two provider keys in a constant context, where `==` on strings is
/// not available.
const fn provider_keys_equal(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    if left.len() != right.len() {
        return false;
    }
    let mut index = 0;
    while index < left.len() {
        if left[index] != right[index] {
            return false;
        }
        index += 1;
    }
    true
}

/// The shared table's row for a provider key.
///
/// Every provider-facing string and flag the native catalog carries is read
/// from this row, so one declaration in `PROVIDER_ENTRIES` describes a
/// provider on every target. The lookup runs at compile time; a key with no
/// shared row fails the build rather than advertising a provider this build
/// cannot describe.
const fn shared_provider_entry(provider_key: &str) -> &'static ProviderEntry {
    let mut index = 0;
    while index < PROVIDER_ENTRIES.len() {
        let entry = &PROVIDER_ENTRIES[index];
        if provider_keys_equal(entry.provider_key, provider_key) {
            return entry;
        }
        index += 1;
    }
    panic!("the provider catalog names a key the shared provider table does not declare")
}

/// The native mirror of one shared capability contract.
const fn native_contract(contract: Contract) -> ProviderContract {
    match contract {
        Contract::Geocoding => ProviderContract::Geocoding,
        Contract::Routing => ProviderContract::Routing,
        Contract::DistanceMatrix => ProviderContract::DistanceMatrix,
        Contract::PlaceSearch => ProviderContract::PlaceSearch,
        Contract::TrafficIncidents => ProviderContract::TrafficIncidents,
        Contract::TrafficFlow => ProviderContract::TrafficFlow,
        Contract::RasterTiles => ProviderContract::RasterTiles,
        Contract::VectorTiles => ProviderContract::VectorTiles,
    }
}

/// Maps a shared row's served-contract list to the native contract mirror.
///
/// The array length is the shared list's own length, supplied as the constant
/// generic argument, so the catalog never restates how many capabilities a
/// provider serves. The assertion turns a supplied length that disagrees with
/// the shared row into a compile-time failure.
const fn native_contracts<const CONTRACT_COUNT: usize>(
    shared_contracts: &[Contract],
) -> [ProviderContract; CONTRACT_COUNT] {
    assert!(
        shared_contracts.len() == CONTRACT_COUNT,
        "the native contract list matches the shared row's length"
    );
    let mut contracts = [ProviderContract::Geocoding; CONTRACT_COUNT];
    let mut index = 0;
    while index < shared_contracts.len() {
        contracts[index] = native_contract(shared_contracts[index]);
        index += 1;
    }
    contracts
}

/// Every provider this build can construct.
///
/// Each row reads its key, display name, credential requirement and served
/// contracts from [`PROVIDER_ENTRIES`] at compile time; only the native
/// construction dispatch and the raster-tile caching terms, which the shared
/// table does not carry, are stated here. The rows follow the shared table's
/// order, which is the order a settings surface lists keys in when it offers a
/// choice: the full-service provider first, then the single-capability
/// providers.
pub(crate) const PROVIDER_CATALOG: &[ProviderCatalogEntry] = &[
    ProviderCatalogEntry {
        provider_key: shared_provider_entry("mapquest").provider_key,
        display_name: shared_provider_entry("mapquest").display_name,
        credential_required: shared_provider_entry("mapquest").credential_required,
        served_contracts: &native_contracts::<
            { shared_provider_entry("mapquest").served_contracts.len() },
        >(shared_provider_entry("mapquest").served_contracts),
        install: install_mapquest_contract,
        raster_tile_cache_ceiling_seconds: Some(MAPQUEST_RASTER_TILE_CACHE_CEILING_SECONDS),
        raster_tile_maximum_zoom_level: Some(MAPQUEST_RASTER_TILE_MAXIMUM_ZOOM_LEVEL),
    },
    ProviderCatalogEntry {
        provider_key: shared_provider_entry("here").provider_key,
        display_name: shared_provider_entry("here").display_name,
        credential_required: shared_provider_entry("here").credential_required,
        served_contracts: &native_contracts::<
            { shared_provider_entry("here").served_contracts.len() },
        >(shared_provider_entry("here").served_contracts),
        install: install_here_contract,
        raster_tile_cache_ceiling_seconds: None,
        raster_tile_maximum_zoom_level: None,
    },
    ProviderCatalogEntry {
        provider_key: shared_provider_entry("google").provider_key,
        display_name: shared_provider_entry("google").display_name,
        credential_required: shared_provider_entry("google").credential_required,
        served_contracts: &native_contracts::<
            { shared_provider_entry("google").served_contracts.len() },
        >(shared_provider_entry("google").served_contracts),
        install: install_google_contract,
        raster_tile_cache_ceiling_seconds: None,
        raster_tile_maximum_zoom_level: None,
    },
    ProviderCatalogEntry {
        provider_key: shared_provider_entry("tomtom").provider_key,
        display_name: shared_provider_entry("tomtom").display_name,
        credential_required: shared_provider_entry("tomtom").credential_required,
        served_contracts: &native_contracts::<
            { shared_provider_entry("tomtom").served_contracts.len() },
        >(shared_provider_entry("tomtom").served_contracts),
        install: install_tomtom_contract,
        raster_tile_cache_ceiling_seconds: None,
        raster_tile_maximum_zoom_level: None,
    },
    ProviderCatalogEntry {
        provider_key: shared_provider_entry("mapbox").provider_key,
        display_name: shared_provider_entry("mapbox").display_name,
        credential_required: shared_provider_entry("mapbox").credential_required,
        served_contracts: &native_contracts::<
            { shared_provider_entry("mapbox").served_contracts.len() },
        >(shared_provider_entry("mapbox").served_contracts),
        install: install_mapbox_contract,
        raster_tile_cache_ceiling_seconds: None,
        raster_tile_maximum_zoom_level: None,
    },
    ProviderCatalogEntry {
        provider_key: shared_provider_entry("radar").provider_key,
        display_name: shared_provider_entry("radar").display_name,
        credential_required: shared_provider_entry("radar").credential_required,
        served_contracts: &native_contracts::<
            { shared_provider_entry("radar").served_contracts.len() },
        >(shared_provider_entry("radar").served_contracts),
        install: install_radar_contract,
        raster_tile_cache_ceiling_seconds: None,
        raster_tile_maximum_zoom_level: None,
    },
    ProviderCatalogEntry {
        provider_key: shared_provider_entry("osrm").provider_key,
        display_name: shared_provider_entry("osrm").display_name,
        credential_required: shared_provider_entry("osrm").credential_required,
        served_contracts: &native_contracts::<
            { shared_provider_entry("osrm").served_contracts.len() },
        >(shared_provider_entry("osrm").served_contracts),
        install: install_osrm_contract,
        raster_tile_cache_ceiling_seconds: None,
        raster_tile_maximum_zoom_level: None,
    },
    ProviderCatalogEntry {
        provider_key: shared_provider_entry("photon").provider_key,
        display_name: shared_provider_entry("photon").display_name,
        credential_required: shared_provider_entry("photon").credential_required,
        served_contracts: &native_contracts::<
            { shared_provider_entry("photon").served_contracts.len() },
        >(shared_provider_entry("photon").served_contracts),
        install: install_photon_contract,
        raster_tile_cache_ceiling_seconds: None,
        raster_tile_maximum_zoom_level: None,
    },
    ProviderCatalogEntry {
        provider_key: shared_provider_entry("valhalla").provider_key,
        display_name: shared_provider_entry("valhalla").display_name,
        credential_required: shared_provider_entry("valhalla").credential_required,
        served_contracts: &native_contracts::<
            { shared_provider_entry("valhalla").served_contracts.len() },
        >(shared_provider_entry("valhalla").served_contracts),
        install: install_valhalla_contract,
        raster_tile_cache_ceiling_seconds: None,
        raster_tile_maximum_zoom_level: None,
    },
];

/// The base URL a provider key was pointed at with
/// `MapSdk::set_provider_base_url`, if any.
///
/// The store is process-wide, like the diagnostics state: an application
/// constructs one SDK per process, and the browser surface replaces its
/// generated SDK object when it clears every provider, which must not forget
/// an override a page set for its fixture routes. The credential-only catalog
/// installs carry no endpoint of their own, so without an override they reach
/// the provider's public default; the override redirects every later
/// configuration of the key to another origin without changing that default.
static PROVIDER_BASE_URL_OVERRIDES: Mutex<Vec<ProviderBaseUrlOverride>> = Mutex::new(Vec::new());

/// One provider key's configured base URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProviderBaseUrlOverride {
    /// The provider key the override applies to.
    pub(crate) provider_key: String,
    /// The base URL every later configuration of the key addresses.
    pub(crate) base_url: String,
}

/// The base URL override in force for a provider key, if any.
fn provider_base_url_override(provider_key: &str) -> Option<String> {
    let overrides = crate::panic_boundary::state_lock_guard(
        &PROVIDER_BASE_URL_OVERRIDES,
        "the provider base URL overrides",
    );
    overrides
        .iter()
        .find(|override_entry| override_entry.provider_key == provider_key)
        .map(|override_entry| override_entry.base_url.clone())
}

/// Records or clears the base URL a provider key is configured with.
///
/// A non-empty URL is validated before it replaces any earlier override, so a
/// refused call leaves the previous value in force. An empty or absent URL
/// clears the override and restores the provider's public default.
pub(crate) fn record_provider_base_url(
    provider_key: &str,
    base_url: Option<&str>,
) -> Result<(), SdkError> {
    let trimmed_base_url = base_url.map(str::trim).filter(|url| !url.is_empty());
    let validated_base_url = match trimmed_base_url {
        Some(base_url) => Some(validated_provider_base_url(base_url)?),
        None => None,
    };
    let mut overrides = crate::panic_boundary::state_lock_guard(
        &PROVIDER_BASE_URL_OVERRIDES,
        "the provider base URL overrides",
    );
    overrides.retain(|override_entry| override_entry.provider_key != provider_key);
    if let Some(validated_base_url) = validated_base_url {
        overrides.push(ProviderBaseUrlOverride {
            provider_key: provider_key.to_string(),
            base_url: validated_base_url,
        });
    }
    Ok(())
}

/// Validates a base URL a provider key is configured with.
///
/// The credential travels in its own parameter, never in the address: the URL
/// must be an absolute http or https URL with a host and no userinfo, query
/// or fragment, each of which could smuggle a key into the endpoint. The
/// detail names the reason and never echoes the value.
fn validated_provider_base_url(base_url: &str) -> Result<String, SdkError> {
    let parsed = url::Url::parse(base_url).map_err(|error| SdkError::ProviderRequestRejected {
        detail: format!("the provider base URL is not a usable URL: {error}"),
    })?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(SdkError::ProviderRequestRejected {
            detail: "the provider base URL must be an http or https URL with a host".to_string(),
        });
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(SdkError::ProviderRequestRejected {
            detail: "the provider base URL must not carry credentials in its userinfo".to_string(),
        });
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(SdkError::ProviderRequestRejected {
            detail: "the provider base URL must not carry a query or a fragment".to_string(),
        });
    }
    Ok(base_url.to_string())
}

/// The credential a keyless provider reads as an optional deployment address.
///
/// A self-hosted routing engine has no key: the application supplies the base
/// URL of the deployment it uses, or leaves it empty to reach the crate's
/// documented local default.
fn deployment_base_url_or_default(credential: &str, default_base_url: &str) -> String {
    let trimmed_credential = credential.trim();
    if trimmed_credential.is_empty() {
        default_base_url.to_string()
    } else {
        trimmed_credential.to_string()
    }
}

/// Refuses a deployment address the provider layer would only reject at
/// request time.
///
/// A keyless routing provider's constructor is infallible, so the
/// registration validates the address itself: an address that is not an
/// http(s) URL with a host never reaches the registry.
fn validated_deployment_base_url(
    credential: &str,
    default_base_url: &str,
) -> Result<String, SdkError> {
    let base_url = deployment_base_url_or_default(credential, default_base_url);
    let parsed = url::Url::parse(&base_url).map_err(|error| SdkError::ProviderRequestRejected {
        detail: format!("the deployment address {base_url} is not a usable URL: {error}"),
    })?;
    if parsed.host_str().is_none() || !matches!(parsed.scheme(), "http" | "https") {
        return Err(SdkError::ProviderRequestRejected {
            detail: format!(
                "the deployment address {base_url} must be an http or https URL with a host"
            ),
        });
    }
    Ok(base_url)
}

/// Registers the OSRM routing implementation for its one capability.
///
/// The credential, when non-empty, is the deployment's base URL; an empty one
/// targets the crate's documented local default. A malformed URL is refused
/// by the provider crate before anything is registered.
fn install_osrm_contract(
    map_sdk: &MapSdk,
    contract: ProviderContract,
    credential: &str,
) -> Result<(), SdkError> {
    if contract != ProviderContract::Routing {
        return Err(SdkError::ProviderNotConfigured {
            contract_name: contract_name_of(contract),
        });
    }
    let configuration = everymap_providers_osrm::OsrmProviderConfiguration {
        base_url: validated_deployment_base_url(
            &provider_base_url_override("osrm").unwrap_or_else(|| credential.to_string()),
            everymap_providers_osrm::DEFAULT_OSRM_BASE_URL,
        )?,
        ..Default::default()
    };
    let provider = everymap_providers_osrm::OsrmRouter::with_observer(
        configuration,
        Arc::new(EveryMapRequestObserver::new(
            &contract_name_of(contract),
            "osrm",
        )),
    );
    map_sdk.register_routing_provider(Arc::new(EveryMapRouter::new(Arc::new(provider))));
    Ok(())
}

/// Registers the Photon geocoding and place-search implementations for one
/// capability each.
///
/// The credential, when non-empty, is the deployment's base URL; an empty one
/// targets the public Photon instance the crate documents. A malformed URL is
/// refused by the provider crate before anything is registered.
fn install_photon_contract(
    map_sdk: &MapSdk,
    contract: ProviderContract,
    credential: &str,
) -> Result<(), SdkError> {
    let serves_contract = matches!(
        contract,
        ProviderContract::Geocoding | ProviderContract::PlaceSearch
    );
    if !serves_contract {
        return Err(SdkError::ProviderNotConfigured {
            contract_name: contract_name_of(contract),
        });
    }
    let configuration = everymap_providers_photon::PhotonProviderConfiguration {
        base_url: deployment_base_url_or_default(
            &provider_base_url_override("photon").unwrap_or_else(|| credential.to_string()),
            everymap_providers_photon::DEFAULT_PHOTON_BASE_URL,
        ),
        ..Default::default()
    };
    let mut photon_client = everymap_providers_photon::PhotonClient::new(configuration);
    photon_client.set_observer(Arc::new(EveryMapRequestObserver::new(
        &contract_name_of(contract),
        "photon",
    )));
    let client = Arc::new(photon_client);
    match contract {
        ProviderContract::Geocoding => {
            map_sdk.register_geocoding_provider(Arc::new(EveryMapGeocoder::new(Arc::new(
                everymap_providers_photon::PhotonGeocoder::new(Arc::clone(&client)),
            ))));
        }
        ProviderContract::PlaceSearch => {
            map_sdk.register_search_place_provider(Arc::new(EveryMapPlaceSearchProvider::new(
                Arc::new(everymap_providers_photon::PhotonPlaceSearchProvider::new(
                    Arc::clone(&client),
                )),
                Arc::new(everymap_providers_photon::PhotonAutocompleteProvider::new(
                    client,
                )),
            )));
        }
        _ => {
            return Err(SdkError::ProviderNotConfigured {
                contract_name: contract_name_of(contract),
            });
        }
    }
    Ok(())
}

/// Registers the Valhalla routing implementation for its one capability.
///
/// The credential, when non-empty, is the deployment's base URL; an empty one
/// targets the crate's documented local default.
fn install_valhalla_contract(
    map_sdk: &MapSdk,
    contract: ProviderContract,
    credential: &str,
) -> Result<(), SdkError> {
    if contract != ProviderContract::Routing {
        return Err(SdkError::ProviderNotConfigured {
            contract_name: contract_name_of(contract),
        });
    }
    let configuration = everymap_providers_valhalla::ValhallaProviderConfiguration {
        base_url: validated_deployment_base_url(
            &provider_base_url_override("valhalla").unwrap_or_else(|| credential.to_string()),
            everymap_providers_valhalla::DEFAULT_VALHALLA_BASE_URL,
        )?,
        ..Default::default()
    };
    let provider = everymap_providers_valhalla::ValhallaRouter::with_observer(
        configuration,
        Arc::new(EveryMapRequestObserver::new(
            &contract_name_of(contract),
            "valhalla",
        )),
    );
    map_sdk.register_routing_provider(Arc::new(EveryMapRouter::new(Arc::new(provider))));
    Ok(())
}

/// Finds the catalog entry for a provider key.
pub(crate) fn catalog_entry_for_key(provider_key: &str) -> Option<&'static ProviderCatalogEntry> {
    PROVIDER_CATALOG
        .iter()
        .find(|entry| entry.provider_key == provider_key)
}

/// Registers one capability from the provider set the shared client serves.
///
/// The client is constructed per call, so a credential change takes effect
/// immediately and the previously serving provider stays in place.
fn install_mapquest_contract(
    map_sdk: &MapSdk,
    contract: ProviderContract,
    credential: &str,
) -> Result<(), SdkError> {
    register_mapquest_contract(
        map_sdk,
        contract,
        mapquest_provider_client(credential, contract),
    )
}

/// Builds the full-service provider's client configuration.
///
/// The key travels in `api_key`; `base_url` stays the provider's public
/// default unless an application set an override for the key, so the
/// credential is never carried in the address.
fn mapquest_provider_configuration(
    credential: &str,
    base_url_override: Option<&str>,
) -> MapQuestProviderConfiguration {
    let mut configuration = MapQuestProviderConfiguration {
        api_key: credential.to_string(),
        ..MapQuestProviderConfiguration::default()
    };
    if let Some(base_url) = base_url_override {
        configuration.base_url = base_url.to_string();
    }
    configuration
}

/// Builds the provider set's client for one credential and one capability.
///
/// Each capability gets its own client so its request observations carry the
/// contract they served; the provider layer has no shared pacing state to
/// lose by not sharing one.
fn mapquest_provider_client(credential: &str, contract: ProviderContract) -> Arc<MapQuestClient> {
    let configuration = mapquest_provider_configuration(
        credential,
        provider_base_url_override("mapquest").as_deref(),
    );
    let mut client = MapQuestClient::new(configuration);
    client.set_observer(Arc::new(EveryMapRequestObserver::new(
        &contract_name_of(contract),
        "mapquest",
    )));
    Arc::new(client)
}

/// Registers one capability of the provider set onto the SDK.
fn register_mapquest_contract(
    map_sdk: &MapSdk,
    contract: ProviderContract,
    provider_client: Arc<MapQuestClient>,
) -> Result<(), SdkError> {
    match contract {
        ProviderContract::Geocoding => {
            map_sdk.register_geocoding_provider(Arc::new(EveryMapGeocoder::new(Arc::new(
                MapQuestGeocoder::new(provider_client),
            ))));
            Ok(())
        }
        ProviderContract::Routing => {
            map_sdk.register_routing_provider(Arc::new(EveryMapRouter::new(Arc::new(
                MapQuestRouter::new(provider_client),
            ))));
            Ok(())
        }
        ProviderContract::DistanceMatrix => {
            map_sdk.register_distance_matrix_provider(Arc::new(
                EveryMapDistanceMatrixProvider::new(Arc::new(MapQuestDistanceMatrix::new(
                    provider_client,
                ))),
            ));
            Ok(())
        }
        ProviderContract::PlaceSearch => {
            map_sdk.register_search_place_provider(Arc::new(EveryMapPlaceSearchProvider::new(
                Arc::new(MapQuestPlaceSearchProvider::new(Arc::clone(
                    &provider_client,
                ))),
                Arc::new(MapQuestAutocompleteProvider::new(provider_client)),
            )));
            Ok(())
        }
        ProviderContract::TrafficIncidents => {
            map_sdk.register_traffic_incident_provider(Arc::new(EveryMapTrafficProvider::new(
                Arc::new(MapQuestTrafficProvider::new(provider_client)),
            )));
            Ok(())
        }
        ProviderContract::TrafficFlow => {
            map_sdk.register_traffic_flow_provider(Arc::new(EveryMapTrafficProvider::new(
                Arc::new(MapQuestTrafficProvider::new(provider_client)),
            )));
            Ok(())
        }
        ProviderContract::RasterTiles => {
            map_sdk.register_raster_tile_provider(Arc::new(EveryMapRasterTileProvider::new(
                Arc::new(MapQuestRasterTileProvider::new(provider_client)),
            )));
            Ok(())
        }
        // The catalog's served-contract list gates every other capability
        // before this call, so this arm is the compiler asking for totality
        // rather than a reachable path.
        ProviderContract::Elevation
        | ProviderContract::VectorTiles
        | ProviderContract::OfflineRouting => Err(SdkError::ProviderNotConfigured {
            contract_name: contract_name_of(contract),
        }),
    }
}

/// The refusal a vendor answers for a contract it does not serve.
fn contract_not_served(contract: ProviderContract) -> SdkError {
    SdkError::ProviderNotConfigured {
        contract_name: contract_name_of(contract),
    }
}

/// Builds one vendor's implementations through the shared provider crate and
/// registers the requested capability through the bridge.
fn install_shared_vendor(
    map_sdk: &MapSdk,
    contract: ProviderContract,
    provider_key: &str,
    credential: &str,
) -> Result<(), SdkError> {
    let implementations = rustic_maps_providers::build_vendor(
        provider_key,
        credential,
        provider_base_url_override(provider_key).as_deref(),
        &contract_name_of(contract),
    )
    .ok_or_else(|| contract_not_served(contract))?;
    install_vendor_contract(map_sdk, contract, &implementations)
}

/// Registers one capability of a vendor's implementation set on the SDK.
fn install_vendor_contract(
    map_sdk: &MapSdk,
    contract: ProviderContract,
    implementations: &rustic_maps_providers::VendorImplementations,
) -> Result<(), SdkError> {
    match contract {
        ProviderContract::Geocoding => {
            let Some(geocoder) = implementations.geocoder.as_ref() else {
                return Err(contract_not_served(contract));
            };
            map_sdk
                .register_geocoding_provider(Arc::new(EveryMapGeocoder::new(Arc::clone(geocoder))));
            Ok(())
        }
        ProviderContract::Routing => {
            let Some(router) = implementations.router.as_ref() else {
                return Err(contract_not_served(contract));
            };
            map_sdk.register_routing_provider(Arc::new(EveryMapRouter::new(Arc::clone(router))));
            Ok(())
        }
        ProviderContract::TrafficIncidents => {
            let Some(traffic) = implementations.traffic.as_ref() else {
                return Err(contract_not_served(contract));
            };
            map_sdk.register_traffic_incident_provider(Arc::new(EveryMapTrafficProvider::new(
                Arc::clone(traffic),
            )));
            Ok(())
        }
        ProviderContract::TrafficFlow => {
            let Some(traffic) = implementations.traffic.as_ref() else {
                return Err(contract_not_served(contract));
            };
            map_sdk.register_traffic_flow_provider(Arc::new(EveryMapTrafficProvider::new(
                Arc::clone(traffic),
            )));
            Ok(())
        }
        ProviderContract::RasterTiles => {
            let Some(raster_tiles) = implementations.raster_tiles.as_ref() else {
                return Err(contract_not_served(contract));
            };
            map_sdk.register_raster_tile_provider(Arc::new(EveryMapRasterTileProvider::new(
                Arc::clone(raster_tiles),
            )));
            Ok(())
        }
        ProviderContract::VectorTiles => {
            let Some(vector_tiles) = implementations.vector_tiles.as_ref() else {
                return Err(contract_not_served(contract));
            };
            map_sdk.register_vector_tile_provider(Arc::new(EveryMapVectorTileProvider::new(
                Arc::clone(vector_tiles),
            )));
            if let Some(style_resources) = implementations.style_resources.as_ref() {
                map_sdk.register_vector_tile_style_resources(
                    Arc::clone(style_resources),
                    implementations
                        .style_resource_hosts
                        .iter()
                        .map(|host| host.to_string())
                        .collect(),
                );
            }
            Ok(())
        }
        ProviderContract::DistanceMatrix
        | ProviderContract::PlaceSearch
        | ProviderContract::Elevation
        | ProviderContract::OfflineRouting => Err(contract_not_served(contract)),
    }
}

fn install_here_contract(
    map_sdk: &MapSdk,
    contract: ProviderContract,
    credential: &str,
) -> Result<(), SdkError> {
    install_shared_vendor(map_sdk, contract, "here", credential)
}

fn install_google_contract(
    map_sdk: &MapSdk,
    contract: ProviderContract,
    credential: &str,
) -> Result<(), SdkError> {
    install_shared_vendor(map_sdk, contract, "google", credential)
}

fn install_tomtom_contract(
    map_sdk: &MapSdk,
    contract: ProviderContract,
    credential: &str,
) -> Result<(), SdkError> {
    install_shared_vendor(map_sdk, contract, "tomtom", credential)
}

fn install_mapbox_contract(
    map_sdk: &MapSdk,
    contract: ProviderContract,
    credential: &str,
) -> Result<(), SdkError> {
    install_shared_vendor(map_sdk, contract, "mapbox", credential)
}

fn install_radar_contract(
    map_sdk: &MapSdk,
    contract: ProviderContract,
    credential: &str,
) -> Result<(), SdkError> {
    install_shared_vendor(map_sdk, contract, "radar", credential)
}

/// The name a capability reports in a not-configured detail.
pub(crate) fn contract_name_of(contract: ProviderContract) -> String {
    contract.to_core_contract().contract_name().to_string()
}

/// Builds one readback entry from the registry state and the recorded
/// selection.
pub(crate) fn registration_entry(
    contract: ProviderContract,
    selection: Option<&ProviderSelection>,
    registry_configured: bool,
) -> ProviderRegistration {
    let registered = registry_configured || selection.is_some();
    ProviderRegistration {
        contract,
        provider_key: selection
            .map(|selection| selection.provider_key.clone())
            .unwrap_or_default(),
        display_name: selection
            .map(|selection| selection.display_name.clone())
            .unwrap_or_default(),
        registered,
    }
}

/// The provider an application chose for one capability, kept so the readback
/// can name it: the registry itself answers only whether its slot is filled.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ProviderSelection {
    /// The capability the selection applies to.
    pub(crate) contract: ProviderContract,
    /// The provider's stable key.
    pub(crate) provider_key: String,
    /// The provider's display name.
    pub(crate) display_name: String,
}

#[cfg(test)]
mod tests {
    use super::{MAPQUEST_RASTER_TILE_MAXIMUM_ZOOM_LEVEL, PROVIDER_CATALOG, catalog_entry_for_key};

    #[test]
    fn the_catalog_declares_the_mapquest_raster_zoom_ceiling() {
        let mapquest_entry =
            catalog_entry_for_key("mapquest").expect("the full-service provider is catalogued");
        assert_eq!(
            mapquest_entry.raster_tile_maximum_zoom_level,
            Some(MAPQUEST_RASTER_TILE_MAXIMUM_ZOOM_LEVEL),
            "the credentialed provider's declared ceiling reaches the tile-pack manager"
        );
        assert_eq!(
            mapquest_entry.raster_tile_maximum_zoom_level,
            Some(20),
            "the documented MapQuest Tiles API maximum is twenty"
        );
    }

    #[test]
    fn the_photon_entry_serves_geocoding_and_place_search_keylessly() {
        let photon_entry =
            catalog_entry_for_key("photon").expect("the Photon provider is catalogued");
        assert!(!photon_entry.credential_required);
        assert!(
            photon_entry
                .served_contracts
                .contains(&crate::records::ProviderContract::Geocoding)
        );
        assert!(
            photon_entry
                .served_contracts
                .contains(&crate::records::ProviderContract::PlaceSearch)
        );
        assert_eq!(photon_entry.raster_tile_maximum_zoom_level, None);
    }

    #[test]
    fn catalog_entries_without_raster_tiles_declare_no_zoom_ceiling() {
        for provider_key in ["osrm", "valhalla"] {
            let catalog_entry = catalog_entry_for_key(provider_key)
                .expect("the single-capability routing providers are catalogued");
            assert_eq!(
                catalog_entry.raster_tile_maximum_zoom_level, None,
                "{provider_key} serves no raster tiles and declares no zoom ceiling"
            );
        }
        assert_eq!(
            PROVIDER_CATALOG
                .iter()
                .filter(|entry| entry.raster_tile_maximum_zoom_level.is_some())
                .count(),
            1,
            "only the raster-serving provider declares a zoom ceiling"
        );
    }

    #[test]
    fn the_catalog_matches_the_shared_provider_table() {
        for entry in PROVIDER_CATALOG {
            let shared =
                rustic_maps_providers::entry_for_key(entry.provider_key).unwrap_or_else(|| {
                    panic!(
                        "{} is missing from the shared provider table",
                        entry.provider_key
                    )
                });
            assert_eq!(entry.display_name, shared.display_name);
            assert_eq!(entry.credential_required, shared.credential_required);
            let mut catalog_contracts: Vec<&str> = entry
                .served_contracts
                .iter()
                .map(|contract| contract.registry_name())
                .collect();
            let mut shared_contracts: Vec<&str> = shared
                .served_contracts
                .iter()
                .map(|contract| contract.registry_name())
                .collect();
            catalog_contracts.sort_unstable();
            shared_contracts.sort_unstable();
            assert_eq!(
                catalog_contracts, shared_contracts,
                "{}",
                entry.provider_key
            );
        }
        for shared in rustic_maps_providers::PROVIDER_ENTRIES {
            assert!(
                catalog_entry_for_key(shared.provider_key).is_some(),
                "{} is not catalogued by this build",
                shared.provider_key
            );
        }
    }

    #[test]
    fn the_catalog_derives_every_row_from_the_shared_provider_table() {
        let shared_entries = rustic_maps_providers::PROVIDER_ENTRIES;
        assert_eq!(
            PROVIDER_CATALOG.len(),
            shared_entries.len(),
            "the catalog declares one row per shared provider entry"
        );
        for (catalog_entry, shared_entry) in PROVIDER_CATALOG.iter().zip(shared_entries) {
            assert_eq!(catalog_entry.provider_key, shared_entry.provider_key);
            assert_eq!(catalog_entry.display_name, shared_entry.display_name);
            assert_eq!(
                catalog_entry.credential_required,
                shared_entry.credential_required
            );
            let catalog_contracts: Vec<&str> = catalog_entry
                .served_contracts
                .iter()
                .map(|contract| contract.registry_name())
                .collect();
            let shared_contracts: Vec<&str> = shared_entry
                .served_contracts
                .iter()
                .map(|contract| contract.registry_name())
                .collect();
            assert_eq!(
                catalog_contracts, shared_contracts,
                "{} derives exactly the shared row's served contracts",
                catalog_entry.provider_key
            );
        }
    }

    #[test]
    fn every_credentialed_vendor_serves_geocoding_and_routing() {
        for provider_key in ["mapquest", "here", "google", "tomtom", "mapbox", "radar"] {
            let catalog_entry = catalog_entry_for_key(provider_key)
                .unwrap_or_else(|| panic!("{provider_key} is catalogued"));
            assert!(catalog_entry.credential_required);
            for contract in [
                crate::records::ProviderContract::Geocoding,
                crate::records::ProviderContract::Routing,
            ] {
                assert!(
                    catalog_entry.served_contracts.contains(&contract),
                    "{provider_key} declares {contract:?}"
                );
            }
        }
        let tomtom = catalog_entry_for_key("tomtom").expect("TomTom is catalogued");
        assert!(
            tomtom
                .served_contracts
                .contains(&crate::records::ProviderContract::RasterTiles),
            "TomTom serves raster tiles through the registry's tile slot"
        );
    }

    #[test]
    fn the_vector_tile_arm_registers_the_supplied_provider() {
        use crate::records::ProviderContract;
        use std::sync::Arc;

        // A keyless URL-template provider stands in for any vector-capable
        // vendor: its construction validates a committed source declaration
        // and reaches no network.
        let vector_provider =
            everymap_providers_url_template_vector::UrlTemplateVectorTileProvider::new(
                everymap_providers_url_template_vector::openfreemap_vector_source(),
            )
            .expect("the committed source declaration builds");
        let implementations = rustic_maps_providers::VendorImplementations {
            geocoder: None,
            router: None,
            traffic: None,
            raster_tiles: None,
            vector_tiles: Some(Arc::new(vector_provider)),
            style_resources: None,
            style_resource_hosts: &[],
        };
        let map_sdk = crate::MapSdk::new();
        super::install_vendor_contract(&map_sdk, ProviderContract::VectorTiles, &implementations)
            .expect("the vector-tile arm registers a supplied provider");
        let vector_tiles_registration = map_sdk
            .provider_registrations()
            .into_iter()
            .find(|registration| registration.contract == ProviderContract::VectorTiles)
            .expect("every capability is reported");
        assert!(
            vector_tiles_registration.registered,
            "the registry's vector-tile slot is filled"
        );
        // A vendor set without a vector implementation is refused, so a
        // provider that does not serve vector tiles cannot claim the slot.
        let implementations_without_vector_tiles = rustic_maps_providers::VendorImplementations {
            geocoder: None,
            router: None,
            traffic: None,
            raster_tiles: None,
            vector_tiles: None,
            style_resources: None,
            style_resource_hosts: &[],
        };
        assert!(
            super::install_vendor_contract(
                &map_sdk,
                ProviderContract::VectorTiles,
                &implementations_without_vector_tiles,
            )
            .is_err()
        );
    }

    #[test]
    fn every_catalog_entry_registers_exactly_its_declared_contracts() {
        for catalog_entry in PROVIDER_CATALOG {
            let map_sdk = crate::MapSdk::new();
            let credential = if catalog_entry.credential_required {
                "placeholder-credential"
            } else {
                ""
            };
            for contract in catalog_entry.served_contracts {
                map_sdk
                    .configure_provider(
                        *contract,
                        catalog_entry.provider_key.to_string(),
                        credential.to_string(),
                    )
                    .unwrap_or_else(|error| {
                        panic!(
                            "{} refused its declared {contract:?} capability: {error:?}",
                            catalog_entry.provider_key
                        )
                    });
            }
            for registration in map_sdk.provider_registrations() {
                let is_served = catalog_entry
                    .served_contracts
                    .contains(&registration.contract);
                assert_eq!(
                    registration.registered, is_served,
                    "{} reports {:?} as registered={}",
                    catalog_entry.provider_key, registration.contract, registration.registered
                );
                if is_served {
                    assert_eq!(registration.provider_key, catalog_entry.provider_key);
                    assert_eq!(registration.display_name, catalog_entry.display_name);
                }
            }
            // A contract the entry does not declare is refused before it can
            // fill a slot the provider cannot serve.
            if let Some(unserved_contract) = crate::records::ProviderContract::ALL
                .into_iter()
                .find(|contract| !catalog_entry.served_contracts.contains(contract))
            {
                assert!(
                    map_sdk
                        .configure_provider(
                            unserved_contract,
                            catalog_entry.provider_key.to_string(),
                            credential.to_string(),
                        )
                        .is_err(),
                    "{} accepted an undeclared {unserved_contract:?} capability",
                    catalog_entry.provider_key
                );
            }
        }
    }

    /// Serializes the cases that configure the process-wide base URL store,
    /// so one case's override cannot reach another's assertion.
    static BASE_URL_OVERRIDE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn clearing_a_base_url_override_restores_the_public_default() {
        let _test_lock = BASE_URL_OVERRIDE_TEST_LOCK
            .lock()
            .expect("the base URL store test lock is healthy");
        const LOOPBACK_ORIGIN: &str = "http://127.0.0.1:9";
        let map_sdk = crate::MapSdk::new();
        map_sdk
            .set_provider_base_url("mapquest".to_string(), Some(LOOPBACK_ORIGIN.to_string()))
            .expect("the loopback origin is a valid base URL");
        let overridden_configuration = super::mapquest_provider_configuration(
            "placeholder-key",
            super::provider_base_url_override("mapquest").as_deref(),
        );
        assert_eq!(
            overridden_configuration.base_url, LOOPBACK_ORIGIN,
            "an override redirects the client the catalog constructs"
        );

        map_sdk
            .set_provider_base_url("mapquest".to_string(), None)
            .expect("clearing the override succeeds");
        let restored_configuration = super::mapquest_provider_configuration(
            "placeholder-key",
            super::provider_base_url_override("mapquest").as_deref(),
        );
        assert_eq!(
            restored_configuration.base_url,
            everymap_providers_mapquest::DEFAULT_MAPQUEST_BASE_URL,
            "clearing the override restores the provider's public default"
        );
    }

    #[test]
    fn an_empty_base_url_clears_the_override() {
        let _test_lock = BASE_URL_OVERRIDE_TEST_LOCK
            .lock()
            .expect("the base URL store test lock is healthy");
        const LOOPBACK_ORIGIN: &str = "http://127.0.0.1:9";
        let map_sdk = crate::MapSdk::new();
        map_sdk
            .set_provider_base_url("mapquest".to_string(), Some(LOOPBACK_ORIGIN.to_string()))
            .expect("the loopback origin is a valid base URL");
        map_sdk
            .set_provider_base_url("mapquest".to_string(), Some(String::new()))
            .expect("an empty base URL clears the override");
        assert_eq!(
            super::provider_base_url_override("mapquest"),
            None,
            "no override remains after an empty base URL"
        );
    }
}
