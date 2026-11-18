//! End-to-end routing through each credentialled vendor provider and the SDK
//! bridge, against the committed payloads the provider layer's own contract
//! tests recorded. The loopback server answers every request with one payload,
//! so no test reaches the network.
//!
//! Each case proves the seam the SDK owns: the vendor provider constructs with
//! a base URL, the provider layer parses its payload, and the bridge converts
//! the result into the SDK's `Route` with its geometry and legs.

use everymap_core::auth::header::HeaderAuthProvider;
use everymap_core::auth::{ApiKeyProvider, AuthProvider};
use geospatial_traits::RoutingProvider;
use geospatial_types::GeographicCoordinate;
use geospatial_types::route::{RouteOptions, TransportationMode};
use rustic_maps_core::everymap_bridge::EveryMapRouter;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

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

#[tokio::test]
async fn here_routing_crosses_the_bridge_with_geometry_and_legs() {
    let server = FixtureServer::start(fixture_bytes("here", "route_two_sections.json"));
    let client = Arc::new(everymap_providers_here::HereClient::new(Arc::new(
        ApiKeyProvider::new("placeholder-key".to_string(), "apiKey".to_string()),
    )));
    let router = EveryMapRouter::new(Arc::new(
        everymap_providers_here::domain::routing::HereRouter::with_base_url(
            client,
            server.base_url().to_string(),
        ),
    ));
    let start = GeographicCoordinate::from_degrees(52.5163, 13.3777)
        .expect("the fixture start is a valid coordinate");
    let end = GeographicCoordinate::from_degrees(52.52, 13.39)
        .expect("the fixture destination is a valid coordinate");
    let options = RouteOptions {
        transportation_mode: TransportationMode::Automobile,
        ..RouteOptions::default()
    };

    let route = router
        .calculate_route(&[start, end], &options)
        .await
        .expect("the vendor provider answers the committed payload");
    assert!(
        route.distance_meters > 0.0,
        "the route reports its distance"
    );
    assert!(
        !route.geometry.is_empty(),
        "the route's encoded shape decoded into points"
    );
    assert!(
        !route.legs.is_empty(),
        "every fixture section maps into a leg"
    );
    assert_eq!(server.request_count(), 1);
}

// ---------------------------------------------------------------------------
// One case per remaining credentialled vendor: the same bridge seam, each
// against the payload the provider layer's own routing contract test recorded.
// ---------------------------------------------------------------------------

/// The route the fixture encodes; each vendor answers for these endpoints.
async fn route_between(
    router: &EveryMapRouter,
    start: (f64, f64),
    end: (f64, f64),
) -> geospatial_types::route::RouteData {
    let start = GeographicCoordinate::from_degrees(start.0, start.1)
        .expect("the fixture start is a valid coordinate");
    let end = GeographicCoordinate::from_degrees(end.0, end.1)
        .expect("the fixture destination is a valid coordinate");
    let options = RouteOptions {
        transportation_mode: TransportationMode::Automobile,
        ..RouteOptions::default()
    };
    router
        .calculate_route(&[start, end], &options)
        .await
        .expect("the vendor provider answers the committed payload")
}

/// The Berlin-to-Paris endpoints the Google, Mapbox and Radar payloads carry.
const BERLIN_TO_PARIS: ((f64, f64), (f64, f64)) = ((52.5163, 13.3777), (48.8566, 2.3522));

#[tokio::test]
async fn google_routing_crosses_the_bridge_with_geometry_and_legs() {
    let server = FixtureServer::start(fixture_bytes("google", "route_berlin_paris.json"));
    let client = Arc::new(everymap_providers_google::GoogleClient::new(api_key_auth(
        "placeholder-key",
        "key",
    )));
    let router = EveryMapRouter::new(Arc::new(
        everymap_providers_google::GoogleRouter::with_base_url(
            client,
            server.base_url().to_string(),
        ),
    ));

    let route = route_between(&router, BERLIN_TO_PARIS.0, BERLIN_TO_PARIS.1).await;
    assert!(
        (route.distance_meters - 1_052_000.0).abs() < 1.0,
        "the route reports the fixture distance"
    );
    assert!(
        !route.geometry.is_empty(),
        "the encoded polyline decoded into points"
    );
    assert!(!route.legs.is_empty(), "the fixture leg maps into a leg");
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn tomtom_routing_crosses_the_bridge_with_geometry_and_legs() {
    let server = FixtureServer::start(fixture_bytes("tomtom", "route_berlin_london.json"));
    let client = Arc::new(everymap_providers_tomtom::TomTomClient::new(api_key_auth(
        "placeholder-key",
        "key",
    )));
    let router = EveryMapRouter::new(Arc::new(
        everymap_providers_tomtom::TomTomRouter::with_base_url(
            client,
            server.base_url().to_string(),
        ),
    ));

    let route = route_between(&router, (52.52, 13.405), (51.5, -0.12)).await;
    assert!(
        (route.distance_meters - 947_000.0).abs() < 1.0,
        "the route reports the fixture distance"
    );
    assert!(
        !route.geometry.is_empty(),
        "the fixture points decoded into geometry"
    );
    assert!(!route.legs.is_empty(), "the fixture leg maps into a leg");
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn mapbox_routing_crosses_the_bridge_with_geometry_and_legs() {
    let server = FixtureServer::start(fixture_bytes("mapbox", "route_berlin_paris.json"));
    let client = Arc::new(everymap_providers_mapbox::MapBoxClient::new(api_key_auth(
        "placeholder-key",
        "access_token",
    )));
    let router = EveryMapRouter::new(Arc::new(
        everymap_providers_mapbox::MapBoxRouter::with_base_url(
            client,
            server.base_url().to_string(),
        ),
    ));

    let route = route_between(&router, BERLIN_TO_PARIS.0, BERLIN_TO_PARIS.1).await;
    assert!(
        (route.distance_meters - 947_000.0).abs() < 1.0,
        "the route reports the fixture distance"
    );
    assert!(
        !route.geometry.is_empty(),
        "the Mapbox polyline decoded into points"
    );
    assert!(!route.legs.is_empty(), "the fixture leg maps into a leg");
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn radar_routing_crosses_the_bridge_with_geometry_and_legs() {
    let server = FixtureServer::start(fixture_bytes("radar", "route_berlin_paris.json"));
    let auth: Arc<dyn AuthProvider> =
        Arc::new(HeaderAuthProvider::new("placeholder-key".to_string()));
    let client = Arc::new(everymap_providers_radar::RadarClient::new(auth));
    let router = EveryMapRouter::new(Arc::new(
        everymap_providers_radar::RadarRouter::with_base_url(client, server.base_url().to_string()),
    ));

    let route = route_between(&router, BERLIN_TO_PARIS.0, BERLIN_TO_PARIS.1).await;
    assert!(
        (route.distance_meters - 250_000.0).abs() < 1.0,
        "the route reports the fixture distance"
    );
    assert!(
        !route.geometry.is_empty(),
        "the polyline6 shape decoded into points"
    );
    assert!(!route.legs.is_empty(), "the fixture leg maps into a leg");
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn mapquest_routing_crosses_the_bridge_with_geometry_and_legs() {
    let server = FixtureServer::start(fixture_bytes(
        "mapquest",
        "directions_route_two_waypoints.json",
    ));
    let configuration = everymap_providers_mapquest::MapQuestProviderConfiguration {
        api_key: "placeholder-key".to_string(),
        base_url: server.base_url().to_string(),
        ..everymap_providers_mapquest::MapQuestProviderConfiguration::default()
    };
    let client = Arc::new(everymap_providers_mapquest::MapQuestClient::new(
        configuration,
    ));
    let router = EveryMapRouter::new(Arc::new(everymap_providers_mapquest::MapQuestRouter::new(
        client,
    )));

    let route = route_between(&router, BERLIN_TO_PARIS.0, BERLIN_TO_PARIS.1).await;
    assert!(
        (route.distance_meters - 1_700.0).abs() < 50.0,
        "the route reports the fixture distance converted to metres"
    );
    assert!(
        !route.geometry.is_empty(),
        "the fixture shape points decoded into geometry"
    );
    assert!(!route.legs.is_empty(), "the fixture leg maps into a leg");
    assert_eq!(server.request_count(), 1);
}
