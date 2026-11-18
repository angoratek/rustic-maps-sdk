//! The provider catalog table shared by the SDK's native and browser bindings.
//!
//! The native catalog and the browser dispatch each carry a row per provider:
//! its key, display name, credential requirement and the contracts it serves.
//! That table used to exist three times — once per binding and once in the
//! shared registry — so a provider could be advertised on one target and
//! refused on another. It lives here once; both bindings read it and their
//! tests compare their own rows against it, and the registry test compares the
//! descriptors in `platform/providers.json` against it.
//!
//! The vendor implementations stay in their own crates. This crate carries no
//! vendor URL, credential value or transport; adding a provider adds one row
//! here and one construction function in the binding's shared seam.

/// One capability a provider row can serve, in the order a settings surface
/// lists capabilities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Contract {
    Geocoding,
    Routing,
    DistanceMatrix,
    PlaceSearch,
    TrafficIncidents,
    TrafficFlow,
    RasterTiles,
    VectorTiles,
}

impl Contract {
    /// The capability's name in the shared registry's `served_contracts` list.
    pub fn registry_name(self) -> &'static str {
        match self {
            Self::Geocoding => "geocoding",
            Self::Routing => "routing",
            Self::DistanceMatrix => "distance_matrix",
            Self::PlaceSearch => "place_search",
            Self::TrafficIncidents => "traffic_incidents",
            Self::TrafficFlow => "traffic_flow",
            Self::RasterTiles => "raster_tiles",
            Self::VectorTiles => "vector_tiles",
        }
    }
}

/// One provider this build can construct.
pub struct ProviderEntry {
    /// The provider's stable registry key.
    pub provider_key: &'static str,
    /// The display name a settings surface shows.
    pub display_name: &'static str,
    /// Whether the provider refuses to serve without a credential, or takes a
    /// deployment address in its credential slot.
    pub credential_required: bool,
    /// The capabilities the provider serves through the SDK's contracts.
    pub served_contracts: &'static [Contract],
}

/// Every provider the catalog can construct, in settings-surface order: the
/// full-service provider first, then the single-capability providers.
pub const PROVIDER_ENTRIES: &[ProviderEntry] = &[
    ProviderEntry {
        provider_key: "mapquest",
        display_name: "MapQuest",
        credential_required: true,
        served_contracts: &[
            Contract::Geocoding,
            Contract::Routing,
            Contract::DistanceMatrix,
            Contract::PlaceSearch,
            Contract::TrafficIncidents,
            Contract::TrafficFlow,
            Contract::RasterTiles,
        ],
    },
    ProviderEntry {
        provider_key: "here",
        display_name: "HERE",
        credential_required: true,
        served_contracts: &[
            Contract::Geocoding,
            Contract::Routing,
            Contract::TrafficIncidents,
            Contract::TrafficFlow,
            Contract::VectorTiles,
        ],
    },
    ProviderEntry {
        provider_key: "google",
        display_name: "Google Maps",
        credential_required: true,
        served_contracts: &[Contract::Geocoding, Contract::Routing],
    },
    ProviderEntry {
        provider_key: "tomtom",
        display_name: "TomTom",
        credential_required: true,
        served_contracts: &[
            Contract::Geocoding,
            Contract::Routing,
            Contract::TrafficIncidents,
            Contract::TrafficFlow,
            Contract::RasterTiles,
        ],
    },
    ProviderEntry {
        provider_key: "mapbox",
        display_name: "Mapbox",
        credential_required: true,
        served_contracts: &[
            Contract::Geocoding,
            Contract::Routing,
            Contract::VectorTiles,
        ],
    },
    ProviderEntry {
        provider_key: "radar",
        display_name: "Radar",
        credential_required: true,
        served_contracts: &[Contract::Geocoding, Contract::Routing],
    },
    ProviderEntry {
        provider_key: "osrm",
        display_name: "OSRM",
        credential_required: false,
        served_contracts: &[Contract::Routing],
    },
    ProviderEntry {
        provider_key: "photon",
        display_name: "Photon",
        credential_required: false,
        served_contracts: &[Contract::Geocoding, Contract::PlaceSearch],
    },
    ProviderEntry {
        provider_key: "valhalla",
        display_name: "Valhalla",
        credential_required: false,
        served_contracts: &[Contract::Routing],
    },
];

/// The table row of a provider key, when this build carries it.
pub fn entry_for_key(provider_key: &str) -> Option<&'static ProviderEntry> {
    PROVIDER_ENTRIES
        .iter()
        .find(|entry| entry.provider_key == provider_key)
}

use everymap_core::auth::header::HeaderAuthProvider;
use everymap_core::auth::{ApiKeyProvider, AuthProvider};
use everymap_core::domains::routing as everymap_routing;
use everymap_core::domains::search as everymap_search;
use everymap_core::domains::tiling as everymap_tiling;
use everymap_core::domains::traffic as everymap_traffic;
use everymap_core::http::Method;
use geospatial_types::SDKError;
use rustic_maps_core::provider_observation::EveryMapRequestObserver;
use rustic_maps_core::{StyleResourceClient, everymap_bridge::map_provider_error};
use std::sync::Arc;

/// The everymap-rs implementations one credential builds for a vendor. A
/// vendor that does not serve a contract leaves its field empty.
pub struct VendorImplementations {
    pub geocoder: Option<Arc<dyn everymap_search::Geocoder>>,
    pub router: Option<Arc<dyn everymap_routing::Router>>,
    pub traffic: Option<Arc<dyn everymap_traffic::TrafficProvider>>,
    pub raster_tiles: Option<Arc<dyn everymap_tiling::RasterTileProvider>>,
    pub vector_tiles: Option<Arc<dyn everymap_tiling::VectorTileProvider>>,
    /// Fetches the style document, glyph ranges and sprites the vendor
    /// publishes for its vector tiles, through the same authenticated client
    /// the contract calls use. Empty when the vendor serves no vector tiles or
    /// publishes no hosted style resources.
    pub style_resources: Option<Arc<dyn StyleResourceClient>>,
    /// The hosts the vendor publishes style resources on, besides the style
    /// document's own origin. The renderer refuses a document that names any
    /// other host before a request is made, so the credential only travels to
    /// hosts the vendor declared. Empty when the vendor publishes no style
    /// resources.
    pub style_resource_hosts: &'static [&'static str],
}

/// The host HERE publishes a style document's glyph ranges and sprite sheets
/// on; the document itself is served by the same asset host.
const HERE_STYLE_RESOURCE_HOSTS: &[&str] = &["assets.vector.hereapi.com"];

/// Fetches published rendering style resources through HERE's authenticated
/// client, so the credential, transport, pacing and retry policy stay the
/// provider layer's.
struct HereStyleResources {
    client: Arc<everymap_providers_here::HereClient>,
}

#[async_trait::async_trait]
impl StyleResourceClient for HereStyleResources {
    async fn fetch_style_resource(&self, url: &str) -> Result<Vec<u8>, SDKError> {
        let request = self.client.build_request(Method::Get, url);
        let response = self
            .client
            .request(request)
            .await
            .map_err(map_provider_error)?;
        Ok(response.body().to_vec())
    }
}

/// The host Mapbox serves style documents, glyph ranges and sprite sheets
/// from.
const MAPBOX_STYLE_RESOURCE_HOSTS: &[&str] = &["api.mapbox.com"];

/// The vendor URI scheme a Mapbox style names its sprite base with, as in
/// `mapbox://sprites/{username}/{style_id}`.
pub const MAPBOX_SPRITES_URI_PREFIX: &str = "mapbox://sprites/";

/// The vendor URI scheme a Mapbox style names its glyph template with, as in
/// `mapbox://fonts/{username}/{fontstack}/{range}`.
pub const MAPBOX_FONTS_URI_PREFIX: &str = "mapbox://fonts/";

/// The Styles API base a `mapbox://sprites/…` base maps to.
const MAPBOX_SPRITES_API_BASE_URL: &str = "https://api.mapbox.com/styles/v1";

/// The Fonts API base a `mapbox://fonts/…` template maps to.
const MAPBOX_FONTS_API_BASE_URL: &str = "https://api.mapbox.com/fonts/v1";

/// The segment the Styles API names a style's sprite by.
const MAPBOX_SPRITES_API_SEGMENT: &str = "sprite";

/// The suffix the Fonts API serves one glyph range with.
const MAPBOX_FONTS_API_SUFFIX: &str = ".pbf";

/// Maps a Mapbox style document's vendor URI schemes to the Styles and Fonts
/// API addresses that serve the same resources.
///
/// The official documents name their resources this way:
/// `mapbox://sprites/{username}/{style_id}` serves the sprite index and sheet
/// from `…/styles/v1/{username}/{style_id}/sprite`, and
/// `mapbox://fonts/{username}/{fontstack}/{range}` serves one glyph range from
/// `…/fonts/v1/{username}/{fontstack}/{range}.pbf`. The glyph template's
/// `{fontstack}` and `{range}` placeholders are preserved so the renderer
/// substitutes them after this mapping.
///
/// # Parameters
/// - `declared_address` — the document's `sprite` or `glyphs` value.
///
/// # Returns
/// The absolute API address, or `None` when the value is not one of these two
/// schemes (a relative or absolute web address keeps its own rule).
pub fn resolve_mapbox_style_resource_address(declared_address: &str) -> Option<String> {
    let declared_address = declared_address.trim();
    if let Some(sprite_path) = declared_address.strip_prefix(MAPBOX_SPRITES_URI_PREFIX) {
        let mut segments = sprite_path.split('/');
        let username = segments.next().filter(|segment| !segment.is_empty())?;
        let style_id = segments.next().filter(|segment| !segment.is_empty())?;
        // A trailing version or cache segment names the same sprite base; the
        // Styles API path does not take it.
        return Some(format!(
            "{MAPBOX_SPRITES_API_BASE_URL}/{username}/{style_id}/{MAPBOX_SPRITES_API_SEGMENT}"
        ));
    }
    if let Some(font_path) = declared_address.strip_prefix(MAPBOX_FONTS_URI_PREFIX) {
        let segments: Vec<&str> = font_path.split('/').collect();
        if segments.len() != 3 || segments.iter().any(|segment| segment.is_empty()) {
            return None;
        }
        let [username, fontstack, range] = segments.as_slice() else {
            return None;
        };
        return Some(format!(
            "{MAPBOX_FONTS_API_BASE_URL}/{username}/{fontstack}/{range}{MAPBOX_FONTS_API_SUFFIX}"
        ));
    }
    None
}

/// Fetches published rendering style resources through Mapbox's authenticated
/// client, so the credential, transport, pacing and retry policy stay the
/// provider layer's.
struct MapBoxStyleResources {
    client: Arc<everymap_providers_mapbox::MapBoxClient>,
}

#[async_trait::async_trait]
impl StyleResourceClient for MapBoxStyleResources {
    async fn fetch_style_resource(&self, url: &str) -> Result<Vec<u8>, SDKError> {
        let request = self.client.build_request(Method::Get, url);
        let response = self
            .client
            .request(request)
            .await
            .map_err(map_provider_error)?;
        Ok(response.body().to_vec())
    }

    fn resolve_declared_style_resource_address(&self, declared_address: &str) -> Option<String> {
        resolve_mapbox_style_resource_address(declared_address)
    }
}

/// The query-parameter credential a credentialed vendor's client applies.
fn api_key_auth(credential: &str, parameter: &str) -> Arc<dyn AuthProvider> {
    Arc::new(ApiKeyProvider::new(
        credential.to_string(),
        parameter.to_string(),
    ))
}

/// The observer every request of one registration reports through.
fn contract_observer(contract_name: &str, provider_key: &str) -> Arc<EveryMapRequestObserver> {
    Arc::new(EveryMapRequestObserver::new(contract_name, provider_key))
}

/// The override a registration was given, when it is not blank.
fn base_url_override(override_base_url: Option<&str>) -> Option<&str> {
    override_base_url.filter(|url| !url.trim().is_empty())
}

/// Builds the implementations of one credentialed vendor for a contract.
///
/// # Parameters
/// - `provider_key` — the registry key (`here`, `google`, `tomtom`, `mapbox`,
///   `radar`).
/// - `credential` — the issued key, or the deployment address of a keyless
///   vendor.
/// - `override_base_url` — a test-only base URL every request is sent to.
/// - `contract_name` — the capability name the request observation reports.
///
/// # Returns
/// The vendor's implementations, or `None` when no browser/native builder
/// carries that key.
pub fn build_vendor(
    provider_key: &str,
    credential: &str,
    override_base_url: Option<&str>,
    contract_name: &str,
) -> Option<VendorImplementations> {
    match provider_key {
        "here" => Some(here_implementations(
            credential,
            override_base_url,
            contract_name,
        )),
        "google" => Some(google_implementations(
            credential,
            override_base_url,
            contract_name,
        )),
        "tomtom" => Some(tomtom_implementations(
            credential,
            override_base_url,
            contract_name,
        )),
        "mapbox" => Some(mapbox_implementations(
            credential,
            override_base_url,
            contract_name,
        )),
        "radar" => Some(radar_implementations(
            credential,
            override_base_url,
            contract_name,
        )),
        _ => None,
    }
}

fn here_implementations(
    credential: &str,
    override_base_url: Option<&str>,
    contract_name: &str,
) -> VendorImplementations {
    let mut client = everymap_providers_here::HereClient::new(api_key_auth(credential, "apiKey"));
    client.set_observer(contract_observer(contract_name, "here"));
    let client = Arc::new(client);
    let base_url = base_url_override(override_base_url);
    let geocoder = match base_url {
        Some(url) => everymap_providers_here::HereGeocoder::with_base_url(
            Arc::clone(&client),
            url.to_string(),
        ),
        None => everymap_providers_here::HereGeocoder::new(Arc::clone(&client)),
    };
    let router = match base_url {
        Some(url) => {
            everymap_providers_here::HereRouter::with_base_url(Arc::clone(&client), url.to_string())
        }
        None => everymap_providers_here::HereRouter::new(Arc::clone(&client)),
    };
    let traffic = match base_url {
        Some(url) => everymap_providers_here::HereTraffic::with_base_url(
            Arc::clone(&client),
            url.to_string(),
        ),
        None => everymap_providers_here::HereTraffic::new(Arc::clone(&client)),
    };
    let vector_tiles = match base_url {
        Some(url) => everymap_providers_here::HereTileProvider::with_base_url(
            Arc::clone(&client),
            url.to_string(),
        ),
        None => everymap_providers_here::HereTileProvider::new(Arc::clone(&client)),
    };
    let style_resources = Arc::new(HereStyleResources { client });
    VendorImplementations {
        geocoder: Some(Arc::new(geocoder)),
        router: Some(Arc::new(router)),
        traffic: Some(Arc::new(traffic)),
        raster_tiles: None,
        vector_tiles: Some(Arc::new(vector_tiles)),
        style_resources: Some(style_resources),
        style_resource_hosts: HERE_STYLE_RESOURCE_HOSTS,
    }
}

fn google_implementations(
    credential: &str,
    override_base_url: Option<&str>,
    contract_name: &str,
) -> VendorImplementations {
    let mut client = everymap_providers_google::GoogleClient::new(api_key_auth(credential, "key"));
    client.set_observer(contract_observer(contract_name, "google"));
    let client = Arc::new(client);
    let base_url = base_url_override(override_base_url);
    let geocoder = match base_url {
        Some(url) => everymap_providers_google::GoogleGeocoder::with_base_url(
            Arc::clone(&client),
            url.to_string(),
        ),
        None => everymap_providers_google::GoogleGeocoder::new(Arc::clone(&client)),
    };
    let router = match base_url {
        Some(url) => everymap_providers_google::GoogleRouter::with_base_url(
            Arc::clone(&client),
            url.to_string(),
        ),
        None => everymap_providers_google::GoogleRouter::new(client),
    };
    VendorImplementations {
        geocoder: Some(Arc::new(geocoder)),
        router: Some(Arc::new(router)),
        traffic: None,
        raster_tiles: None,
        vector_tiles: None,
        style_resources: None,
        style_resource_hosts: &[],
    }
}

fn tomtom_implementations(
    credential: &str,
    override_base_url: Option<&str>,
    contract_name: &str,
) -> VendorImplementations {
    let mut client = everymap_providers_tomtom::TomTomClient::new(api_key_auth(credential, "key"));
    client.set_observer(contract_observer(contract_name, "tomtom"));
    let client = Arc::new(client);
    let base_url = base_url_override(override_base_url);
    let geocoder = match base_url {
        Some(url) => everymap_providers_tomtom::TomTomGeocoder::with_base_url(
            Arc::clone(&client),
            url.to_string(),
        ),
        None => everymap_providers_tomtom::TomTomGeocoder::new(Arc::clone(&client)),
    };
    let router = match base_url {
        Some(url) => everymap_providers_tomtom::TomTomRouter::with_base_url(
            Arc::clone(&client),
            url.to_string(),
        ),
        None => everymap_providers_tomtom::TomTomRouter::new(Arc::clone(&client)),
    };
    let traffic = match base_url {
        Some(url) => everymap_providers_tomtom::TomTomTraffic::with_base_url(
            Arc::clone(&client),
            url.to_string(),
        ),
        None => everymap_providers_tomtom::TomTomTraffic::new(Arc::clone(&client)),
    };
    let raster_tiles = match base_url {
        Some(url) => everymap_providers_tomtom::TomTomTileProvider::with_base_url(
            Arc::clone(&client),
            url.to_string(),
        ),
        None => everymap_providers_tomtom::TomTomTileProvider::new(client),
    };
    VendorImplementations {
        geocoder: Some(Arc::new(geocoder)),
        router: Some(Arc::new(router)),
        traffic: Some(Arc::new(traffic)),
        raster_tiles: Some(Arc::new(raster_tiles)),
        vector_tiles: None,
        style_resources: None,
        style_resource_hosts: &[],
    }
}

fn mapbox_implementations(
    credential: &str,
    override_base_url: Option<&str>,
    contract_name: &str,
) -> VendorImplementations {
    let mut client =
        everymap_providers_mapbox::MapBoxClient::new(api_key_auth(credential, "access_token"));
    client.set_observer(contract_observer(contract_name, "mapbox"));
    let client = Arc::new(client);
    let base_url = base_url_override(override_base_url);
    let geocoder = match base_url {
        Some(url) => everymap_providers_mapbox::MapBoxGeocoder::with_base_url(
            Arc::clone(&client),
            url.to_string(),
        ),
        None => everymap_providers_mapbox::MapBoxGeocoder::new(Arc::clone(&client)),
    };
    let router = match base_url {
        Some(url) => everymap_providers_mapbox::MapBoxRouter::with_base_url(
            Arc::clone(&client),
            url.to_string(),
        ),
        None => everymap_providers_mapbox::MapBoxRouter::new(Arc::clone(&client)),
    };
    let vector_tiles = match base_url {
        Some(url) => everymap_providers_mapbox::MapBoxTileProvider::with_base_url(
            Arc::clone(&client),
            url.to_string(),
        ),
        None => everymap_providers_mapbox::MapBoxTileProvider::new(Arc::clone(&client)),
    };
    let style_resources = Arc::new(MapBoxStyleResources { client });
    VendorImplementations {
        geocoder: Some(Arc::new(geocoder)),
        router: Some(Arc::new(router)),
        traffic: None,
        raster_tiles: None,
        vector_tiles: Some(Arc::new(vector_tiles)),
        style_resources: Some(style_resources),
        style_resource_hosts: MAPBOX_STYLE_RESOURCE_HOSTS,
    }
}

fn radar_implementations(
    credential: &str,
    override_base_url: Option<&str>,
    contract_name: &str,
) -> VendorImplementations {
    let auth: Arc<dyn AuthProvider> = Arc::new(HeaderAuthProvider::new(credential.to_string()));
    let mut client = everymap_providers_radar::RadarClient::new(auth);
    client.set_observer(contract_observer(contract_name, "radar"));
    let client = Arc::new(client);
    let base_url = base_url_override(override_base_url);
    let geocoder = match base_url {
        Some(url) => everymap_providers_radar::RadarGeocoder::with_base_url(
            Arc::clone(&client),
            url.to_string(),
        ),
        None => everymap_providers_radar::RadarGeocoder::new(Arc::clone(&client)),
    };
    let router = match base_url {
        Some(url) => everymap_providers_radar::RadarRouter::with_base_url(
            Arc::clone(&client),
            url.to_string(),
        ),
        None => everymap_providers_radar::RadarRouter::new(client),
    };
    VendorImplementations {
        geocoder: Some(Arc::new(geocoder)),
        router: Some(Arc::new(router)),
        traffic: None,
        raster_tiles: None,
        vector_tiles: None,
        style_resources: None,
        style_resource_hosts: &[],
    }
}

#[cfg(test)]
mod tests {
    use super::{Contract, PROVIDER_ENTRIES, entry_for_key};

    #[test]
    fn every_row_is_addressable_by_its_key() {
        for entry in PROVIDER_ENTRIES {
            let resolved = entry_for_key(entry.provider_key).expect("the row resolves");
            assert_eq!(resolved.display_name, entry.display_name);
        }
    }

    #[test]
    fn the_vector_capable_vendors_declare_vector_tiles() {
        for provider_key in ["here", "mapbox"] {
            let entry = entry_for_key(provider_key).expect("the vendor is catalogued");
            assert!(
                entry.served_contracts.contains(&Contract::VectorTiles),
                "{provider_key} serves vector tiles"
            );
        }
        for provider_key in ["mapquest", "google", "tomtom", "radar"] {
            let entry = entry_for_key(provider_key).expect("the vendor is catalogued");
            assert!(
                !entry.served_contracts.contains(&Contract::VectorTiles),
                "{provider_key} does not serve vector tiles"
            );
        }
    }

    #[test]
    fn the_contract_names_match_the_registry_spelling() {
        assert_eq!(Contract::Geocoding.registry_name(), "geocoding");
        assert_eq!(Contract::TrafficFlow.registry_name(), "traffic_flow");
        assert_eq!(Contract::RasterTiles.registry_name(), "raster_tiles");
        assert_eq!(Contract::VectorTiles.registry_name(), "vector_tiles");
    }

    #[test]
    fn the_mapbox_sprite_uri_maps_to_the_styles_api_sprite_endpoint() {
        assert_eq!(
            super::resolve_mapbox_style_resource_address("mapbox://sprites/mapbox/streets-v12")
                .as_deref(),
            Some("https://api.mapbox.com/styles/v1/mapbox/streets-v12/sprite")
        );
    }

    #[test]
    fn the_mapbox_glyph_uri_maps_to_the_fonts_api_template_and_keeps_placeholders() {
        assert_eq!(
            super::resolve_mapbox_style_resource_address(
                "mapbox://fonts/mapbox/{fontstack}/{range}"
            )
            .as_deref(),
            Some("https://api.mapbox.com/fonts/v1/mapbox/{fontstack}/{range}.pbf")
        );
    }

    #[test]
    fn web_and_relative_addresses_are_not_rewritten() {
        for address in [
            "glyphs/{fontstack}/{range}.pbf",
            "https://styles.example.test/published/sprite",
            "vendorscheme://sprites/vendor/neutral",
        ] {
            assert_eq!(
                super::resolve_mapbox_style_resource_address(address),
                None,
                "{address} keeps its own resolution rule"
            );
        }
    }

    #[test]
    fn a_mapbox_uri_without_the_user_and_style_segments_is_not_guessed() {
        assert_eq!(
            super::resolve_mapbox_style_resource_address("mapbox://sprites/mapbox"),
            None
        );
        assert_eq!(
            super::resolve_mapbox_style_resource_address("mapbox://fonts/mapbox/{fontstack}"),
            None
        );
    }
}
