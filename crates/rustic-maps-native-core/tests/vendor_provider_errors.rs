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

mod common;

use common::{LoopbackFixtureServer, ScriptedResponse};
use everymap_core::auth::header::HeaderAuthProvider;
use everymap_core::auth::{ApiKeyProvider, AuthProvider};
use geospatial_traits::{GeocodingProvider, RoutingProvider};
use geospatial_types::GeographicCoordinate;
use geospatial_types::route::{RouteOptions, TransportationMode};
use rustic_maps::SdkError;
use rustic_maps_core::everymap_bridge::{EveryMapGeocoder, EveryMapRouter};
use std::sync::Arc;

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

/// The unified detail a 401 produces: the shared client's status error, whose
/// display is the only part the bridge carries into the SDK error.
const UNAUTHORIZED_UNIFIED_DETAIL: &str = "HTTP error 401: HTTP error: Unauthorized";
/// The unified detail the bridge reports when a parsed response carries no
/// route.
const NO_ROUTE_UNIFIED_DETAIL: &str = "the provider returned no route for the request";
/// The unified message the rate-limit variant displays.
const RATE_LIMIT_UNIFIED_MESSAGE: &str = "rate limit exhausted";

/// A server answering every request with one committed JSON payload at
/// `status`.
fn fixture_server(status: u16, payload: Vec<u8>) -> LoopbackFixtureServer {
    LoopbackFixtureServer::start(vec![ScriptedResponse::json(status, payload)])
}

/// A server answering every request with the committed rate-limit payload
/// and its retry hint.
fn rate_limited_server(payload: Vec<u8>) -> LoopbackFixtureServer {
    LoopbackFixtureServer::start(vec![
        ScriptedResponse::json(HTTP_RATE_LIMITED_STATUS, payload)
            .with_header(RETRY_AFTER_HEADER.0, RETRY_AFTER_HEADER.1),
    ])
}

fn fixture_bytes(provider_key: &str, file_name: &str) -> Vec<u8> {
    common::fixture_bytes(&format!("fixtures/{provider_key}/{file_name}"))
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

fn here_geocoder(server: &LoopbackFixtureServer) -> Arc<dyn GeocodingProvider> {
    let client = Arc::new(everymap_providers_here::HereClient::new(api_key_auth(
        "apiKey",
    )));
    Arc::new(EveryMapGeocoder::new(Arc::new(
        everymap_providers_here::HereGeocoder::with_base_url(client, server.base_url().to_string()),
    )))
}

fn here_router(server: &LoopbackFixtureServer) -> Arc<dyn RoutingProvider> {
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

fn google_geocoder(server: &LoopbackFixtureServer) -> Arc<dyn GeocodingProvider> {
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

fn google_router(server: &LoopbackFixtureServer) -> Arc<dyn RoutingProvider> {
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

fn tomtom_geocoder(server: &LoopbackFixtureServer) -> Arc<dyn GeocodingProvider> {
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

fn tomtom_router(server: &LoopbackFixtureServer) -> Arc<dyn RoutingProvider> {
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

fn mapbox_geocoder(server: &LoopbackFixtureServer) -> Arc<dyn GeocodingProvider> {
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

fn mapbox_router(server: &LoopbackFixtureServer) -> Arc<dyn RoutingProvider> {
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

fn radar_geocoder(server: &LoopbackFixtureServer) -> Arc<dyn GeocodingProvider> {
    let client = Arc::new(everymap_providers_radar::RadarClient::new(header_auth()));
    Arc::new(EveryMapGeocoder::new(Arc::new(
        everymap_providers_radar::RadarGeocoder::with_base_url(
            client,
            server.base_url().to_string(),
        ),
    )))
}

fn radar_router(server: &LoopbackFixtureServer) -> Arc<dyn RoutingProvider> {
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
fn assert_credential_reached_the_vendor(server: &LoopbackFixtureServer) {
    assert!(
        server
            .received_requests_text()
            .contains(REGISTERED_CREDENTIAL),
        "the vendor request carried the registered credential"
    );
}

// ---------------------------------------------------------------------------
// HERE
// ---------------------------------------------------------------------------

#[tokio::test]
async fn here_unauthorized_payload_maps_to_provider_request_rejected() {
    let server = fixture_server(
        HTTP_UNAUTHORIZED_STATUS,
        fixture_bytes("here", "geocode_unauthorized.json"),
    );
    let error = geocoding_failure(here_geocoder(&server)).await;
    assert_provider_request_rejected(&error, UNAUTHORIZED_UNIFIED_DETAIL);
    assert_credential_reached_the_vendor(&server);
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn here_rate_limited_payload_maps_to_rate_limit_exhausted() {
    let server = rate_limited_server(fixture_bytes("here", "geocode_rate_limited.json"));
    let error = geocoding_failure(here_geocoder(&server)).await;
    assert_rate_limit_exhausted(&error);
    assert_credential_reached_the_vendor(&server);
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn here_no_route_payload_maps_to_provider_request_rejected() {
    let server = fixture_server(HTTP_OK_STATUS, fixture_bytes("here", "route_no_route.json"));
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
    let server = fixture_server(
        HTTP_UNAUTHORIZED_STATUS,
        fixture_bytes("google", "geocode_unauthorized.json"),
    );
    let error = geocoding_failure(google_geocoder(&server)).await;
    assert_provider_request_rejected(&error, UNAUTHORIZED_UNIFIED_DETAIL);
    assert_credential_reached_the_vendor(&server);
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn google_rate_limited_payload_maps_to_rate_limit_exhausted() {
    let server = rate_limited_server(fixture_bytes("google", "geocode_rate_limited.json"));
    let error = geocoding_failure(google_geocoder(&server)).await;
    assert_rate_limit_exhausted(&error);
    assert_credential_reached_the_vendor(&server);
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn google_no_route_payload_maps_to_provider_request_rejected() {
    let server = fixture_server(
        HTTP_OK_STATUS,
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
    let server = fixture_server(
        HTTP_UNAUTHORIZED_STATUS,
        fixture_bytes("tomtom", "geocode_unauthorized.json"),
    );
    let error = geocoding_failure(tomtom_geocoder(&server)).await;
    assert_provider_request_rejected(&error, UNAUTHORIZED_UNIFIED_DETAIL);
    assert_credential_reached_the_vendor(&server);
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn tomtom_rate_limited_payload_maps_to_rate_limit_exhausted() {
    let server = rate_limited_server(fixture_bytes("tomtom", "geocode_rate_limited.json"));
    let error = geocoding_failure(tomtom_geocoder(&server)).await;
    assert_rate_limit_exhausted(&error);
    assert_credential_reached_the_vendor(&server);
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn tomtom_no_route_payload_maps_to_provider_request_rejected() {
    let server = fixture_server(
        HTTP_OK_STATUS,
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
    let server = fixture_server(
        HTTP_UNAUTHORIZED_STATUS,
        fixture_bytes("mapbox", "geocode_unauthorized.json"),
    );
    let error = geocoding_failure(mapbox_geocoder(&server)).await;
    assert_provider_request_rejected(&error, UNAUTHORIZED_UNIFIED_DETAIL);
    assert_credential_reached_the_vendor(&server);
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn mapbox_rate_limited_payload_maps_to_rate_limit_exhausted() {
    let server = rate_limited_server(fixture_bytes("mapbox", "geocode_rate_limited.json"));
    let error = geocoding_failure(mapbox_geocoder(&server)).await;
    assert_rate_limit_exhausted(&error);
    assert_credential_reached_the_vendor(&server);
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn mapbox_no_route_payload_maps_to_provider_request_rejected() {
    let server = fixture_server(
        HTTP_OK_STATUS,
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
    let server = fixture_server(
        HTTP_UNAUTHORIZED_STATUS,
        fixture_bytes("radar", "geocode_unauthorized.json"),
    );
    let error = geocoding_failure(radar_geocoder(&server)).await;
    assert_provider_request_rejected(&error, UNAUTHORIZED_UNIFIED_DETAIL);
    assert_credential_reached_the_vendor(&server);
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn radar_rate_limited_payload_maps_to_rate_limit_exhausted() {
    let server = rate_limited_server(fixture_bytes("radar", "geocode_rate_limited.json"));
    let error = geocoding_failure(radar_geocoder(&server)).await;
    assert_rate_limit_exhausted(&error);
    assert_credential_reached_the_vendor(&server);
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn radar_no_route_payload_maps_to_provider_request_rejected() {
    let server = fixture_server(
        HTTP_OK_STATUS,
        fixture_bytes("radar", "route_no_route.json"),
    );
    let error = routing_failure(radar_router(&server)).await;
    assert_provider_request_rejected(&error, NO_ROUTE_UNIFIED_DETAIL);
    assert_credential_reached_the_vendor(&server);
    assert_eq!(server.request_count(), 1);
}
