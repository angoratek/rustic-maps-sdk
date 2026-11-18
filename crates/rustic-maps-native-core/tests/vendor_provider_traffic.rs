//! End-to-end traffic through each credentialled vendor provider and the SDK
//! bridge, against the committed payloads the provider layer's own contract
//! tests recorded. The loopback server answers each request by path with one
//! payload, so no test reaches the network.
//!
//! Each case proves the seam the SDK owns: the vendor provider constructs
//! with a base URL, the provider layer parses its flow and incident payloads,
//! and the bridge converts both into the SDK's records. The vendor request
//! shapes stay covered by the provider layer's own suites.

use everymap_core::auth::ApiKeyProvider;
use geospatial_traits::{TrafficFlowProvider, TrafficIncidentProvider};
use geospatial_types::{BoundingBox, GeographicCoordinate};
use rustic_maps_core::everymap_bridge::EveryMapTrafficProvider;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

/// HTTP status the scripted server returns for a payload.
const HTTP_OK_STATUS: u16 = 200;

/// HTTP status the scripted server returns for a request no route answers,
/// so a mismatched path fails the case instead of stalling it.
const HTTP_NOT_FOUND_STATUS: u16 = 404;

/// One request-path fragment and the committed payload it answers with.
struct ScriptedRoute {
    path_fragment: &'static str,
    payload: Vec<u8>,
}

/// A loopback server answering each request with the payload whose path
/// fragment the request line carries, counting the requests it served and
/// recording any request no scripted route answers.
struct FixtureServer {
    base_url: String,
    request_count: Arc<Mutex<usize>>,
    unmatched_requests: Arc<Mutex<Vec<String>>>,
}

impl FixtureServer {
    fn start(routes: Vec<ScriptedRoute>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("the loopback port binds");
        let port = listener.local_addr().expect("the port is readable").port();
        let request_count = Arc::new(Mutex::new(0));
        let unmatched_requests = Arc::new(Mutex::new(Vec::new()));
        let served_count = Arc::clone(&request_count);
        let unmatched = Arc::clone(&unmatched_requests);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                serve_one(stream, &routes, &served_count, &unmatched);
            }
        });
        Self {
            base_url: format!("http://127.0.0.1:{port}"),
            request_count,
            unmatched_requests,
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

    fn unmatched_requests(&self) -> Vec<String> {
        self.unmatched_requests
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

fn serve_one(
    mut stream: TcpStream,
    routes: &[ScriptedRoute],
    served_count: &Arc<Mutex<usize>>,
    unmatched_requests: &Arc<Mutex<Vec<String>>>,
) {
    let mut reader = BufReader::new(stream.try_clone().expect("the stream clones"));
    let mut request_line = String::new();
    let _ = reader.read_line(&mut request_line);
    for line in reader.lines() {
        let Ok(line) = line else { break };
        if line.is_empty() {
            break;
        }
    }
    *served_count
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) += 1;
    let scripted_payload = routes
        .iter()
        .find(|route| request_line.contains(route.path_fragment))
        .map(|route| route.payload.as_slice());
    let (status, payload) = match scripted_payload {
        Some(payload) => (HTTP_OK_STATUS, payload),
        None => {
            unmatched_requests
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(request_line.trim_end().to_string());
            (HTTP_NOT_FOUND_STATUS, b"{}".as_slice())
        }
    };
    let reason = if status == HTTP_OK_STATUS {
        "OK"
    } else {
        "Not Found"
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
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

fn api_key_auth(credential: &str, parameter: &str) -> Arc<ApiKeyProvider> {
    Arc::new(ApiKeyProvider::new(
        credential.to_string(),
        parameter.to_string(),
    ))
}

/// The Berlin box the recorded payloads were captured for.
fn berlin_box() -> BoundingBox {
    BoundingBox::from_corners(
        GeographicCoordinate::from_degrees(52.4, 13.3)
            .expect("the south-west corner is a valid coordinate"),
        GeographicCoordinate::from_degrees(52.6, 13.5)
            .expect("the north-east corner is a valid coordinate"),
    )
    .expect("the corners describe a valid box")
}

#[tokio::test]
async fn here_traffic_crosses_the_bridge_with_flow_and_incidents() {
    let server = FixtureServer::start(vec![
        ScriptedRoute {
            path_fragment: "/flow",
            payload: fixture_bytes("here", "traffic_flow_berlin.json"),
        },
        ScriptedRoute {
            path_fragment: "/incidents",
            payload: fixture_bytes("here", "traffic_incidents_berlin.json"),
        },
    ]);
    let client = Arc::new(everymap_providers_here::HereClient::new(api_key_auth(
        "placeholder-key",
        "apiKey",
    )));
    let traffic =
        everymap_providers_here::HereTraffic::with_base_url(client, server.base_url().to_string());
    let provider = EveryMapTrafficProvider::new(Arc::new(traffic));

    // The provider layer parses HERE's incident payload but reports no
    // incident coordinate, and the SDK's incident record places every
    // incident on the map, so the bridge has nothing to convert. The
    // provider-layer fix is recorded in PLAN.md (M76); this assertion is the
    // tripwire that will fail once HERE reports coordinates.
    let incidents = provider
        .fetch_traffic_incidents(&berlin_box())
        .await
        .expect("the vendor provider answers the committed payloads");
    assert!(
        incidents.is_empty(),
        "HERE reports no incident coordinate through the provider layer today"
    );

    let flows = provider
        .fetch_traffic_flow(&berlin_box())
        .await
        .expect("the vendor provider answers the committed payload");
    assert_eq!(flows.len(), 1, "the fixture reports one flow segment");
    assert_eq!(flows[0].current_speed_kilometers_per_hour, Some(40.0));
    assert_eq!(
        server.request_count(),
        3,
        "the incident call is flow plus incidents; the flow call is one request"
    );
    assert!(
        server.unmatched_requests().is_empty(),
        "every request answers a scripted route: {:?}",
        server.unmatched_requests()
    );
}

#[tokio::test]
async fn tomtom_traffic_crosses_the_bridge_with_flow_and_incidents() {
    let server = FixtureServer::start(vec![
        ScriptedRoute {
            path_fragment: "flowSegmentData",
            payload: fixture_bytes("tomtom", "traffic_flow_berlin.json"),
        },
        ScriptedRoute {
            path_fragment: "incidentDetails",
            payload: fixture_bytes("tomtom", "traffic_incidents_berlin.json"),
        },
    ]);
    let client = Arc::new(everymap_providers_tomtom::TomTomClient::new(api_key_auth(
        "placeholder-key",
        "key",
    )));
    let traffic = everymap_providers_tomtom::TomTomTraffic::with_base_url(
        client,
        server.base_url().to_string(),
    );
    let provider = EveryMapTrafficProvider::new(Arc::new(traffic));

    // TomTom's provider layer reports no incident coordinate and passes the
    // incident type through capitalised ("Accident"), which is outside the
    // contract's vocabulary, so the bridge keeps nothing here either. Both
    // causes are recorded in PLAN.md (M76); this assertion fails once the
    // provider reports a typed, placed incident.
    let incidents = provider
        .fetch_traffic_incidents(&berlin_box())
        .await
        .expect("the vendor provider answers the committed payloads");
    assert!(
        incidents.is_empty(),
        "TomTom reports no placed, contracted incident through the provider layer today"
    );

    let flows = provider
        .fetch_traffic_flow(&berlin_box())
        .await
        .expect("the vendor provider answers the committed payload");
    assert_eq!(flows.len(), 1, "the fixture reports one flow segment");
    assert_eq!(flows[0].current_speed_kilometers_per_hour, Some(65.0));
    assert_eq!(
        server.request_count(),
        3,
        "the incident call is flow plus incidents; the flow call is one request"
    );
    assert!(
        server.unmatched_requests().is_empty(),
        "every request answers a scripted route: {:?}",
        server.unmatched_requests()
    );
}

#[tokio::test]
async fn mapquest_traffic_crosses_the_bridge_with_flow_and_incidents() {
    let server = FixtureServer::start(vec![
        ScriptedRoute {
            path_fragment: "/traffic/v2/flow",
            payload: fixture_bytes("mapquest", "traffic_flow_new_york.json"),
        },
        ScriptedRoute {
            path_fragment: "/traffic/v2/incidents",
            payload: fixture_bytes("mapquest", "traffic_incidents_denver.json"),
        },
    ]);
    let configuration = everymap_providers_mapquest::MapQuestProviderConfiguration {
        api_key: "placeholder-key".to_string(),
        base_url: server.base_url().to_string(),
        ..everymap_providers_mapquest::MapQuestProviderConfiguration::default()
    };
    let client = Arc::new(everymap_providers_mapquest::MapQuestClient::new(
        configuration,
    ));
    let provider = EveryMapTrafficProvider::new(Arc::new(
        everymap_providers_mapquest::MapQuestTrafficProvider::new(client),
    ));

    let incidents = provider
        .fetch_traffic_incidents(&berlin_box())
        .await
        .expect("the vendor provider answers the committed payloads");
    assert_eq!(
        incidents.len(),
        5,
        "the fixture reports five placed incidents"
    );
    assert_eq!(incidents[0].identifier, "1396961518491253");

    let flows = provider
        .fetch_traffic_flow(&berlin_box())
        .await
        .expect("the vendor provider answers the committed payload");
    assert!(!flows.is_empty(), "the fixture reports flow features");
    assert_eq!(
        server.request_count(),
        3,
        "the incident call is flow plus incidents; the flow call is one request"
    );
    assert!(
        server.unmatched_requests().is_empty(),
        "every request answers a scripted route: {:?}",
        server.unmatched_requests()
    );
}
