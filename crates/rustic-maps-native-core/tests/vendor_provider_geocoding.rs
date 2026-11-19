//! End-to-end geocoding through each credentialled vendor provider and the
//! SDK bridge, against the committed payloads the provider layer's own
//! contract tests recorded. The loopback server answers every request with
//! one payload, so no test reaches the network.
//!
//! Each case proves the seam the SDK owns: the vendor provider constructs
//! with a base URL, the provider layer parses its payload, and the bridge
//! converts the result into the SDK's `PlaceResult`.

use everymap_core::auth::header::HeaderAuthProvider;
use everymap_core::auth::{ApiKeyProvider, AuthProvider};
use geospatial_traits::GeocodingProvider;
use rustic_maps_core::everymap_bridge::EveryMapGeocoder;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

/// The latitude every vendor fixture answers for the Brandenburg Gate query,
/// with the tolerance a provider's own coordinate rounding may introduce.
const BRANDENBURG_GATE_LATITUDE: f64 = 52.5163;
const COORDINATE_TOLERANCE_DEGREES: f64 = 0.05;

/// The coordinate every vendor's recorded reverse payload was recorded for.
const REVERSE_GEOCODE_LATITUDE: f64 = 52.52;
const REVERSE_GEOCODE_LONGITUDE: f64 = 13.405;

/// HTTP status the scripted server returns for a payload.
const HTTP_OK_STATUS: u16 = 200;

/// A loopback server answering every request with one committed payload and
/// counting the requests it served.
struct FixtureServer {
    base_url: String,
    request_count: Arc<Mutex<usize>>,
}

impl FixtureServer {
    fn start(payload: Vec<u8>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("the loopback port binds");
        let port = listener.local_addr().expect("the port is readable").port();
        let request_count = Arc::new(Mutex::new(0));
        let served_count = Arc::clone(&request_count);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                serve_one(stream, &payload, &served_count);
            }
        });
        Self {
            base_url: format!("http://127.0.0.1:{port}"),
            request_count,
        }
    }

    fn base_url(&self) -> &str {
        &self.base_url
    }

    fn request_count(&self) -> usize {
        *self
            .request_count
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn serve_one(mut stream: TcpStream, payload: &[u8], served_count: &Arc<Mutex<usize>>) {
    let reader = BufReader::new(stream.try_clone().expect("the stream clones"));
    for line in reader.lines() {
        let Ok(line) = line else { break };
        if line.is_empty() {
            break;
        }
    }
    *served_count
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) += 1;
    let header = format!(
        "HTTP/1.1 {HTTP_OK_STATUS} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(payload);
}

fn fixture_bytes(provider_key: &str, file_name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("fixtures")
        .join(provider_key)
        .join(file_name);
    std::fs::read(&path).unwrap_or_else(|error| panic!("{} is checked in: {error}", path.display()))
}

fn api_key_auth(credential: &str, parameter: &str) -> Arc<dyn AuthProvider> {
    Arc::new(ApiKeyProvider::new(
        credential.to_string(),
        parameter.to_string(),
    ))
}

async fn forward_geocode(
    provider: Arc<dyn GeocodingProvider>,
    query: &str,
) -> Vec<geospatial_types::PlaceResult> {
    provider
        .forward_geocode(query)
        .await
        .unwrap_or_else(|error| panic!("the vendor provider answers: {error:?}"))
}

fn assert_brandenburg_gate(places: &[geospatial_types::PlaceResult], provider_key: &str) {
    let first = places
        .first()
        .unwrap_or_else(|| panic!("{provider_key} answers one place"));
    assert!(
        (first.coordinate.latitude - BRANDENBURG_GATE_LATITUDE).abs()
            < COORDINATE_TOLERANCE_DEGREES,
        "{provider_key} reports the fixture coordinate, got {}",
        first.coordinate.latitude
    );
}

async fn reverse_geocode(
    provider: Arc<dyn GeocodingProvider>,
    latitude: f64,
    longitude: f64,
) -> Vec<geospatial_types::PlaceResult> {
    let coordinate = geospatial_types::GeographicCoordinate::from_degrees(latitude, longitude)
        .expect("the reverse-geocode coordinate is valid");
    provider
        .reverse_geocode(&coordinate)
        .await
        .unwrap_or_else(|error| panic!("the vendor provider answers: {error:?}"))
}

fn assert_reverse_place(
    places: &[geospatial_types::PlaceResult],
    provider_key: &str,
    expected_latitude: f64,
    expected_longitude: f64,
    expected_address_fragment: &str,
) {
    let first = places
        .first()
        .unwrap_or_else(|| panic!("{provider_key} answers one place"));
    assert!(
        (first.coordinate.latitude - expected_latitude).abs() < COORDINATE_TOLERANCE_DEGREES
            && (first.coordinate.longitude - expected_longitude).abs()
                < COORDINATE_TOLERANCE_DEGREES,
        "{provider_key} reports the reverse fixture coordinate, got {}, {}",
        first.coordinate.latitude,
        first.coordinate.longitude
    );
    assert!(
        first
            .structured_address_text
            .contains(expected_address_fragment),
        "{provider_key} reports the reverse fixture address {:?}, which does not contain {:?}",
        first.structured_address_text,
        expected_address_fragment
    );
}

#[tokio::test]
async fn here_geocoding_crosses_the_bridge() {
    let server = FixtureServer::start(fixture_bytes("here", "geocode_brandenburg_gate.json"));
    let client = Arc::new(everymap_providers_here::HereClient::new(api_key_auth(
        "placeholder-key",
        "apiKey",
    )));
    let geocoder =
        everymap_providers_here::HereGeocoder::with_base_url(client, server.base_url().to_string());
    let provider = Arc::new(EveryMapGeocoder::new(Arc::new(geocoder)));
    let places = forward_geocode(provider, "Brandenburg Gate, Berlin").await;
    assert_brandenburg_gate(&places, "HERE");
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn google_geocoding_crosses_the_bridge() {
    let server = FixtureServer::start(fixture_bytes("google", "geocode_brandenburg_gate.json"));
    let client = Arc::new(everymap_providers_google::GoogleClient::new(api_key_auth(
        "placeholder-key",
        "key",
    )));
    let geocoder = everymap_providers_google::GoogleGeocoder::with_base_url(
        client,
        server.base_url().to_string(),
    );
    let provider = Arc::new(EveryMapGeocoder::new(Arc::new(geocoder)));
    let places = forward_geocode(provider, "Brandenburg Gate, Berlin").await;
    assert_brandenburg_gate(&places, "Google Maps");
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn tomtom_geocoding_crosses_the_bridge() {
    let server = FixtureServer::start(fixture_bytes("tomtom", "geocode_brandenburg_gate.json"));
    let client = Arc::new(everymap_providers_tomtom::TomTomClient::new(api_key_auth(
        "placeholder-key",
        "key",
    )));
    let geocoder = everymap_providers_tomtom::TomTomGeocoder::with_base_url(
        client,
        server.base_url().to_string(),
    );
    let provider = Arc::new(EveryMapGeocoder::new(Arc::new(geocoder)));
    let places = forward_geocode(provider, "Brandenburg Gate, Berlin").await;
    assert_brandenburg_gate(&places, "TomTom");
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn mapbox_geocoding_crosses_the_bridge() {
    let server = FixtureServer::start(fixture_bytes("mapbox", "geocode_berlin.json"));
    let client = Arc::new(everymap_providers_mapbox::MapBoxClient::new(api_key_auth(
        "placeholder-key",
        "access_token",
    )));
    let geocoder = everymap_providers_mapbox::MapBoxGeocoder::with_base_url(
        client,
        server.base_url().to_string(),
    );
    let provider = Arc::new(EveryMapGeocoder::new(Arc::new(geocoder)));
    let places = forward_geocode(provider, "Berlin").await;
    assert_brandenburg_gate(&places, "MapBox");
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn radar_geocoding_crosses_the_bridge() {
    let server = FixtureServer::start(fixture_bytes("radar", "geocode_brandenburg_gate.json"));
    let auth: Arc<dyn AuthProvider> =
        Arc::new(HeaderAuthProvider::new("placeholder-key".to_string()));
    let client = Arc::new(everymap_providers_radar::RadarClient::new(auth));
    let geocoder = everymap_providers_radar::RadarGeocoder::with_base_url(
        client,
        server.base_url().to_string(),
    );
    let provider = Arc::new(EveryMapGeocoder::new(Arc::new(geocoder)));
    let places = forward_geocode(provider, "Brandenburg Gate, Berlin").await;
    assert_brandenburg_gate(&places, "Radar");
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn here_reverse_geocoding_crosses_the_bridge() {
    let server = FixtureServer::start(fixture_bytes("here", "geocode_reverse_berlin.json"));
    let client = Arc::new(everymap_providers_here::HereClient::new(api_key_auth(
        "placeholder-key",
        "apiKey",
    )));
    let geocoder =
        everymap_providers_here::HereGeocoder::with_base_url(client, server.base_url().to_string());
    let provider = Arc::new(EveryMapGeocoder::new(Arc::new(geocoder)));
    let places = reverse_geocode(
        provider,
        REVERSE_GEOCODE_LATITUDE,
        REVERSE_GEOCODE_LONGITUDE,
    )
    .await;
    assert_reverse_place(&places, "HERE", 52.52, 13.405, "Berlin, Germany");
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn google_reverse_geocoding_crosses_the_bridge() {
    let server = FixtureServer::start(fixture_bytes("google", "geocode_reverse_berlin.json"));
    let client = Arc::new(everymap_providers_google::GoogleClient::new(api_key_auth(
        "placeholder-key",
        "key",
    )));
    let geocoder = everymap_providers_google::GoogleGeocoder::with_base_url(
        client,
        server.base_url().to_string(),
    );
    let provider = Arc::new(EveryMapGeocoder::new(Arc::new(geocoder)));
    let places = reverse_geocode(
        provider,
        REVERSE_GEOCODE_LATITUDE,
        REVERSE_GEOCODE_LONGITUDE,
    )
    .await;
    assert_reverse_place(&places, "Google Maps", 52.5163, 13.3777, "Pariser Platz");
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn tomtom_reverse_geocoding_crosses_the_bridge() {
    let server = FixtureServer::start(fixture_bytes("tomtom", "geocode_reverse_berlin.json"));
    let client = Arc::new(everymap_providers_tomtom::TomTomClient::new(api_key_auth(
        "placeholder-key",
        "key",
    )));
    let geocoder = everymap_providers_tomtom::TomTomGeocoder::with_base_url(
        client,
        server.base_url().to_string(),
    );
    let provider = Arc::new(EveryMapGeocoder::new(Arc::new(geocoder)));
    let places = reverse_geocode(
        provider,
        REVERSE_GEOCODE_LATITUDE,
        REVERSE_GEOCODE_LONGITUDE,
    )
    .await;
    assert_reverse_place(&places, "TomTom", 52.520264, 13.399690, "Bodestraße");
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn mapbox_reverse_geocoding_crosses_the_bridge() {
    let server = FixtureServer::start(fixture_bytes("mapbox", "geocode_reverse_berlin.json"));
    let client = Arc::new(everymap_providers_mapbox::MapBoxClient::new(api_key_auth(
        "placeholder-key",
        "access_token",
    )));
    let geocoder = everymap_providers_mapbox::MapBoxGeocoder::with_base_url(
        client,
        server.base_url().to_string(),
    );
    let provider = Arc::new(EveryMapGeocoder::new(Arc::new(geocoder)));
    let places = reverse_geocode(
        provider,
        REVERSE_GEOCODE_LATITUDE,
        REVERSE_GEOCODE_LONGITUDE,
    )
    .await;
    assert_reverse_place(&places, "MapBox", 52.520264, 13.399690, "Bodestraße 1");
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn radar_reverse_geocoding_crosses_the_bridge() {
    let server = FixtureServer::start(fixture_bytes("radar", "geocode_reverse_berlin.json"));
    let auth: Arc<dyn AuthProvider> =
        Arc::new(HeaderAuthProvider::new("placeholder-key".to_string()));
    let client = Arc::new(everymap_providers_radar::RadarClient::new(auth));
    let geocoder = everymap_providers_radar::RadarGeocoder::with_base_url(
        client,
        server.base_url().to_string(),
    );
    let provider = Arc::new(EveryMapGeocoder::new(Arc::new(geocoder)));
    let places = reverse_geocode(
        provider,
        REVERSE_GEOCODE_LATITUDE,
        REVERSE_GEOCODE_LONGITUDE,
    )
    .await;
    assert_reverse_place(&places, "Radar", 52.5163, 13.3777, "Pariser Platz");
    assert_eq!(server.request_count(), 1);
}
