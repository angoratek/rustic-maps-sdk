//! End-to-end error mapping through each credentialled vendor provider and the
//! SDK bridge, against the committed error payloads. The loopback server
//! answers every request with one payload and status, so no test reaches the
//! network.
//!
//! Each case proves the seam the SDK owns: the vendor provider constructs with
//! a base URL, the provider layer reports the payload as its typed error, and
//! the bridge converts that error into the SDK's unified `SdkError`. Every
//! case also asserts that the credential the request carried is absent from
//! the unified message; the server records the request so the redaction is
//! proven against a request that really sent the credential.
//!
//! The `geocode_unauthorized.json` and `geocode_rate_limited.json` payloads are
//! the shapes each provider crate's `http_errors`/`error_cases` tests record.
//! Google's `route_no_route.json` is the shape its routing contract test
//! records; the other vendors' routing contract tests record no no-route body,
//! so their no-route payload empties the success envelope their routing
//! response type and committed route fixture already declare.

use everymap_core::auth::header::HeaderAuthProvider;
use everymap_core::auth::{ApiKeyProvider, AuthProvider};
use geospatial_traits::{GeocodingProvider, RoutingProvider};
use geospatial_types::GeographicCoordinate;
use geospatial_types::route::{RouteOptions, TransportationMode};
use rustic_maps::SdkError;
use rustic_maps_core::everymap_bridge::{EveryMapGeocoder, EveryMapRouter};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

/// The credential every case registers. It reaches the vendor request and
/// must never reach the unified message.
const REGISTERED_CREDENTIAL: &str = "placeholder-credential";

/// HTTP status of a successfully parsed payload, including a no-route body.
const HTTP_OK_STATUS: u16 = 200;
/// HTTP status of the committed unauthorized payload.
const HTTP_UNAUTHORIZED_STATUS: u16 = 401;
/// HTTP status of the committed rate-limited payload.
const HTTP_RATE_LIMITED_STATUS: u16 = 429;

/// The retry hint the recorded rate-limit payloads carry. It exceeds the
/// client's retry ceiling, so the failing request returns at once instead of
/// sleeping.
const RETRY_AFTER_HEADER: (&str, &str) = ("Retry-After", "60");
/// The media type every committed payload is served as.
const CONTENT_TYPE_HEADER: (&str, &str) = ("Content-Type", "application/json");

/// The unified detail a 401 produces: the shared client's status error, whose
/// display is the only part the bridge carries into the SDK error.
const UNAUTHORIZED_UNIFIED_DETAIL: &str = "HTTP error 401: HTTP error: Unauthorized";
/// The unified detail the bridge reports when a parsed response carries no
/// route.
const NO_ROUTE_UNIFIED_DETAIL: &str = "the provider returned no route for the request";
/// The unified message the rate-limit variant displays.
const RATE_LIMIT_UNIFIED_MESSAGE: &str = "rate limit exhausted";

/// A loopback server answering every request with one status, one set of
/// headers and one committed payload, while recording the requests it served.
struct FixtureServer {
    base_url: String,
    request_count: Arc<Mutex<usize>>,
    received_requests: Arc<Mutex<Vec<String>>>,
}

impl FixtureServer {
    fn start(status: u16, headers: &[(&'static str, &'static str)], payload: Vec<u8>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("the loopback port binds");
        let port = listener.local_addr().expect("the port is readable").port();
        let request_count = Arc::new(Mutex::new(0));
        let received_requests = Arc::new(Mutex::new(Vec::new()));
        let served_count = Arc::clone(&request_count);
        let recorded_requests = Arc::clone(&received_requests);
        let response_headers: Vec<(String, String)> = headers
            .iter()
            .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
            .collect();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                serve_one(
                    stream,
                    status,
                    &response_headers,
                    &payload,
                    &served_count,
                    &recorded_requests,
                );
            }
        });
        Self {
            base_url: format!("http://127.0.0.1:{port}"),
            request_count,
            received_requests,
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

    /// Every request head the server received, joined for the redaction
    /// assertion.
    fn received_requests(&self) -> String {
        self.received_requests
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .join("\n")
    }
}

fn serve_one(
    mut stream: TcpStream,
    status: u16,
    response_headers: &[(String, String)],
    payload: &[u8],
    served_count: &Arc<Mutex<usize>>,
    received_requests: &Arc<Mutex<Vec<String>>>,
) {
    let mut reader = BufReader::new(stream.try_clone().expect("the stream clones"));
    let mut request_head = String::new();
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        let Ok(bytes_read) = reader.read_line(&mut line) else {
            break;
        };
        if bytes_read == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            content_length = value.trim().parse().unwrap_or(0);
        }
        request_head.push_str(&line);
    }
    if content_length > 0 {
        let mut body = vec![0u8; content_length];
        let _ = reader.read_exact(&mut body);
        request_head.push_str(&String::from_utf8_lossy(&body));
    }
    *served_count
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) += 1;
    received_requests
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(request_head);

    let mut response = format!("HTTP/1.1 {status} {}\r\n", reason_phrase(status));
    for (name, value) in response_headers {
        response.push_str(name);
        response.push_str(": ");
        response.push_str(value);
        response.push_str("\r\n");
    }
    response.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    ));
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.write_all(payload);
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        HTTP_OK_STATUS => "OK",
        HTTP_UNAUTHORIZED_STATUS => "Unauthorized",
        HTTP_RATE_LIMITED_STATUS => "Too Many Requests",
        _ => "Unknown",
    }
}

fn fixture_bytes(provider_key: &str, file_name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("fixtures")
        .join(provider_key)
        .join(file_name);
    std::fs::read(&path).unwrap_or_else(|error| panic!("{} is checked in: {error}", path.display()))
}

fn api_key_auth(parameter: &str) -> Arc<dyn AuthProvider> {
    Arc::new(ApiKeyProvider::new(
        REGISTERED_CREDENTIAL.to_string(),
        parameter.to_string(),
    ))
}

fn header_auth() -> Arc<dyn AuthProvider> {
    Arc::new(HeaderAuthProvider::new(REGISTERED_CREDENTIAL.to_string()))
}

fn here_geocoder(server: &FixtureServer) -> Arc<dyn GeocodingProvider> {
    let client = Arc::new(everymap_providers_here::HereClient::new(api_key_auth(
        "apiKey",
    )));
    Arc::new(EveryMapGeocoder::new(Arc::new(
        everymap_providers_here::HereGeocoder::with_base_url(client, server.base_url().to_string()),
    )))
}

fn here_router(server: &FixtureServer) -> Arc<dyn RoutingProvider> {
    let client = Arc::new(everymap_providers_here::HereClient::new(api_key_auth(
        "apiKey",
    )));
    Arc::new(EveryMapRouter::new(Arc::new(
        everymap_providers_here::domain::routing::HereRouter::with_base_url(
            client,
            server.base_url().to_string(),
        ),
    )))
}

fn google_geocoder(server: &FixtureServer) -> Arc<dyn GeocodingProvider> {
    let client = Arc::new(everymap_providers_google::GoogleClient::new(api_key_auth(
        "key",
    )));
    Arc::new(EveryMapGeocoder::new(Arc::new(
        everymap_providers_google::GoogleGeocoder::with_base_url(
            client,
            server.base_url().to_string(),
        ),
    )))
}

fn google_router(server: &FixtureServer) -> Arc<dyn RoutingProvider> {
    let client = Arc::new(everymap_providers_google::GoogleClient::new(api_key_auth(
        "key",
    )));
    Arc::new(EveryMapRouter::new(Arc::new(
        everymap_providers_google::GoogleRouter::with_base_url(
            client,
            server.base_url().to_string(),
        ),
    )))
}

fn tomtom_geocoder(server: &FixtureServer) -> Arc<dyn GeocodingProvider> {
    let client = Arc::new(everymap_providers_tomtom::TomTomClient::new(api_key_auth(
        "key",
    )));
    Arc::new(EveryMapGeocoder::new(Arc::new(
        everymap_providers_tomtom::TomTomGeocoder::with_base_url(
            client,
            server.base_url().to_string(),
        ),
    )))
}

fn tomtom_router(server: &FixtureServer) -> Arc<dyn RoutingProvider> {
    let client = Arc::new(everymap_providers_tomtom::TomTomClient::new(api_key_auth(
        "key",
    )));
    Arc::new(EveryMapRouter::new(Arc::new(
        everymap_providers_tomtom::TomTomRouter::with_base_url(
            client,
            server.base_url().to_string(),
        ),
    )))
}

fn mapbox_geocoder(server: &FixtureServer) -> Arc<dyn GeocodingProvider> {
    let client = Arc::new(everymap_providers_mapbox::MapBoxClient::new(api_key_auth(
        "access_token",
    )));
    Arc::new(EveryMapGeocoder::new(Arc::new(
        everymap_providers_mapbox::MapBoxGeocoder::with_base_url(
            client,
            server.base_url().to_string(),
        ),
    )))
}

fn mapbox_router(server: &FixtureServer) -> Arc<dyn RoutingProvider> {
    let client = Arc::new(everymap_providers_mapbox::MapBoxClient::new(api_key_auth(
        "access_token",
    )));
    Arc::new(EveryMapRouter::new(Arc::new(
        everymap_providers_mapbox::MapBoxRouter::with_base_url(
            client,
            server.base_url().to_string(),
        ),
    )))
}

fn radar_geocoder(server: &FixtureServer) -> Arc<dyn GeocodingProvider> {
    let client = Arc::new(everymap_providers_radar::RadarClient::new(header_auth()));
    Arc::new(EveryMapGeocoder::new(Arc::new(
        everymap_providers_radar::RadarGeocoder::with_base_url(
            client,
            server.base_url().to_string(),
        ),
    )))
}

fn radar_router(server: &FixtureServer) -> Arc<dyn RoutingProvider> {
    let client = Arc::new(everymap_providers_radar::RadarClient::new(header_auth()));
    Arc::new(EveryMapRouter::new(Arc::new(
        everymap_providers_radar::RadarRouter::with_base_url(client, server.base_url().to_string()),
    )))
}

async fn geocoding_failure(provider: Arc<dyn GeocodingProvider>) -> SdkError {
    let error = provider
        .forward_geocode("Brandenburg Gate, Berlin")
        .await
        .expect_err("the committed error payload is not a place result");
    SdkError::from(error)
}

async fn routing_failure(router: Arc<dyn RoutingProvider>) -> SdkError {
    let start = GeographicCoordinate::from_degrees(52.5163, 13.3777)
        .expect("the fixture start is a valid coordinate");
    let end = GeographicCoordinate::from_degrees(48.8566, 2.3522)
        .expect("the fixture destination is a valid coordinate");
    let options = RouteOptions {
        transportation_mode: TransportationMode::Automobile,
        ..RouteOptions::default()
    };
    let error = router
        .calculate_route(&[start, end], &options)
        .await
        .expect_err("the committed no-route payload is not a route");
    SdkError::from(error)
}

/// Asserts the unified provider-rejection variant and its exact message, then
/// that the credential is redacted from that message.
fn assert_provider_request_rejected(error: &SdkError, expected_detail: &str) {
    match error {
        SdkError::ProviderRequestRejected { detail } => {
            assert_eq!(
                detail, expected_detail,
                "the bridge maps the vendor failure onto the unified rejection message"
            );
        }
        other => panic!("expected SdkError::ProviderRequestRejected, got {other:?}"),
    }
    assert_credential_redacted(error);
}

/// Asserts the unified rate-limit variant and message, then that the
/// credential is redacted from that message.
fn assert_rate_limit_exhausted(error: &SdkError) {
    assert_eq!(
        error,
        &SdkError::RateLimitExhausted,
        "a 429 maps onto the unified rate-limit variant"
    );
    assert_eq!(error.to_string(), RATE_LIMIT_UNIFIED_MESSAGE);
    assert_credential_redacted(error);
}

fn assert_credential_redacted(error: &SdkError) {
    let unified_message = error.to_string();
    assert!(
        !unified_message.contains(REGISTERED_CREDENTIAL),
        "the unified message must redact the credential: {unified_message}"
    );
}

/// Proves the redaction assertion is not vacuous: the vendor request really
/// carried the credential before the unified message omitted it.
fn assert_credential_reached_the_vendor(server: &FixtureServer) {
    assert!(
        server.received_requests().contains(REGISTERED_CREDENTIAL),
        "the vendor request carried the registered credential"
    );
}

// ---------------------------------------------------------------------------
// HERE
// ---------------------------------------------------------------------------

#[tokio::test]
async fn here_unauthorized_payload_maps_to_provider_request_rejected() {
    let server = FixtureServer::start(
        HTTP_UNAUTHORIZED_STATUS,
        &[CONTENT_TYPE_HEADER],
        fixture_bytes("here", "geocode_unauthorized.json"),
    );
    let error = geocoding_failure(here_geocoder(&server)).await;
    assert_provider_request_rejected(&error, UNAUTHORIZED_UNIFIED_DETAIL);
    assert_credential_reached_the_vendor(&server);
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn here_rate_limited_payload_maps_to_rate_limit_exhausted() {
    let server = FixtureServer::start(
        HTTP_RATE_LIMITED_STATUS,
        &[CONTENT_TYPE_HEADER, RETRY_AFTER_HEADER],
        fixture_bytes("here", "geocode_rate_limited.json"),
    );
    let error = geocoding_failure(here_geocoder(&server)).await;
    assert_rate_limit_exhausted(&error);
    assert_credential_reached_the_vendor(&server);
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn here_no_route_payload_maps_to_provider_request_rejected() {
    let server = FixtureServer::start(
        HTTP_OK_STATUS,
        &[CONTENT_TYPE_HEADER],
        fixture_bytes("here", "route_no_route.json"),
    );
    let error = routing_failure(here_router(&server)).await;
    assert_provider_request_rejected(&error, NO_ROUTE_UNIFIED_DETAIL);
    assert_credential_reached_the_vendor(&server);
    assert_eq!(server.request_count(), 1);
}

// ---------------------------------------------------------------------------
// Google
// ---------------------------------------------------------------------------

#[tokio::test]
async fn google_unauthorized_payload_maps_to_provider_request_rejected() {
    let server = FixtureServer::start(
        HTTP_UNAUTHORIZED_STATUS,
        &[CONTENT_TYPE_HEADER],
        fixture_bytes("google", "geocode_unauthorized.json"),
    );
    let error = geocoding_failure(google_geocoder(&server)).await;
    assert_provider_request_rejected(&error, UNAUTHORIZED_UNIFIED_DETAIL);
    assert_credential_reached_the_vendor(&server);
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn google_rate_limited_payload_maps_to_rate_limit_exhausted() {
    let server = FixtureServer::start(
        HTTP_RATE_LIMITED_STATUS,
        &[CONTENT_TYPE_HEADER, RETRY_AFTER_HEADER],
        fixture_bytes("google", "geocode_rate_limited.json"),
    );
    let error = geocoding_failure(google_geocoder(&server)).await;
    assert_rate_limit_exhausted(&error);
    assert_credential_reached_the_vendor(&server);
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn google_no_route_payload_maps_to_provider_request_rejected() {
    let server = FixtureServer::start(
        HTTP_OK_STATUS,
        &[CONTENT_TYPE_HEADER],
        fixture_bytes("google", "route_no_route.json"),
    );
    let error = routing_failure(google_router(&server)).await;
    assert_provider_request_rejected(&error, NO_ROUTE_UNIFIED_DETAIL);
    assert_credential_reached_the_vendor(&server);
    assert_eq!(server.request_count(), 1);
}

// ---------------------------------------------------------------------------
// TomTom
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tomtom_unauthorized_payload_maps_to_provider_request_rejected() {
    let server = FixtureServer::start(
        HTTP_UNAUTHORIZED_STATUS,
        &[CONTENT_TYPE_HEADER],
        fixture_bytes("tomtom", "geocode_unauthorized.json"),
    );
    let error = geocoding_failure(tomtom_geocoder(&server)).await;
    assert_provider_request_rejected(&error, UNAUTHORIZED_UNIFIED_DETAIL);
    assert_credential_reached_the_vendor(&server);
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn tomtom_rate_limited_payload_maps_to_rate_limit_exhausted() {
    let server = FixtureServer::start(
        HTTP_RATE_LIMITED_STATUS,
        &[CONTENT_TYPE_HEADER, RETRY_AFTER_HEADER],
        fixture_bytes("tomtom", "geocode_rate_limited.json"),
    );
    let error = geocoding_failure(tomtom_geocoder(&server)).await;
    assert_rate_limit_exhausted(&error);
    assert_credential_reached_the_vendor(&server);
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn tomtom_no_route_payload_maps_to_provider_request_rejected() {
    let server = FixtureServer::start(
        HTTP_OK_STATUS,
        &[CONTENT_TYPE_HEADER],
        fixture_bytes("tomtom", "route_no_route.json"),
    );
    let error = routing_failure(tomtom_router(&server)).await;
    assert_provider_request_rejected(&error, NO_ROUTE_UNIFIED_DETAIL);
    assert_credential_reached_the_vendor(&server);
    assert_eq!(server.request_count(), 1);
}

// ---------------------------------------------------------------------------
// Mapbox
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mapbox_unauthorized_payload_maps_to_provider_request_rejected() {
    let server = FixtureServer::start(
        HTTP_UNAUTHORIZED_STATUS,
        &[CONTENT_TYPE_HEADER],
        fixture_bytes("mapbox", "geocode_unauthorized.json"),
    );
    let error = geocoding_failure(mapbox_geocoder(&server)).await;
    assert_provider_request_rejected(&error, UNAUTHORIZED_UNIFIED_DETAIL);
    assert_credential_reached_the_vendor(&server);
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn mapbox_rate_limited_payload_maps_to_rate_limit_exhausted() {
    let server = FixtureServer::start(
        HTTP_RATE_LIMITED_STATUS,
        &[CONTENT_TYPE_HEADER, RETRY_AFTER_HEADER],
        fixture_bytes("mapbox", "geocode_rate_limited.json"),
    );
    let error = geocoding_failure(mapbox_geocoder(&server)).await;
    assert_rate_limit_exhausted(&error);
    assert_credential_reached_the_vendor(&server);
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn mapbox_no_route_payload_maps_to_provider_request_rejected() {
    let server = FixtureServer::start(
        HTTP_OK_STATUS,
        &[CONTENT_TYPE_HEADER],
        fixture_bytes("mapbox", "route_no_route.json"),
    );
    let error = routing_failure(mapbox_router(&server)).await;
    assert_provider_request_rejected(&error, NO_ROUTE_UNIFIED_DETAIL);
    assert_credential_reached_the_vendor(&server);
    assert_eq!(server.request_count(), 1);
}

// ---------------------------------------------------------------------------
// Radar
// ---------------------------------------------------------------------------

#[tokio::test]
async fn radar_unauthorized_payload_maps_to_provider_request_rejected() {
    let server = FixtureServer::start(
        HTTP_UNAUTHORIZED_STATUS,
        &[CONTENT_TYPE_HEADER],
        fixture_bytes("radar", "geocode_unauthorized.json"),
    );
    let error = geocoding_failure(radar_geocoder(&server)).await;
    assert_provider_request_rejected(&error, UNAUTHORIZED_UNIFIED_DETAIL);
    assert_credential_reached_the_vendor(&server);
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn radar_rate_limited_payload_maps_to_rate_limit_exhausted() {
    let server = FixtureServer::start(
        HTTP_RATE_LIMITED_STATUS,
        &[CONTENT_TYPE_HEADER, RETRY_AFTER_HEADER],
        fixture_bytes("radar", "geocode_rate_limited.json"),
    );
    let error = geocoding_failure(radar_geocoder(&server)).await;
    assert_rate_limit_exhausted(&error);
    assert_credential_reached_the_vendor(&server);
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn radar_no_route_payload_maps_to_provider_request_rejected() {
    let server = FixtureServer::start(
        HTTP_OK_STATUS,
        &[CONTENT_TYPE_HEADER],
        fixture_bytes("radar", "route_no_route.json"),
    );
    let error = routing_failure(radar_router(&server)).await;
    assert_provider_request_rejected(&error, NO_ROUTE_UNIFIED_DETAIL);
    assert_credential_reached_the_vendor(&server);
    assert_eq!(server.request_count(), 1);
}
