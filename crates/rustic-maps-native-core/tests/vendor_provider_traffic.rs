//! End-to-end traffic through each credentialled vendor provider and the SDK
//! bridge, against the committed payloads the provider layer's own contract
//! tests recorded. The loopback server answers each request by path with one
//! payload, so no test reaches the network.
//!
//! Each case proves the seam the SDK owns: the vendor provider constructs
//! with a base URL, the provider layer parses its flow and incident payloads,
//! and the bridge converts both into the SDK's records. The vendor request
//! shapes stay covered by the provider layer's own suites.

mod common;

use common::{LoopbackFixtureServer, ScriptedResponse};
use everymap_core::auth::ApiKeyProvider;
use geospatial_traits::{TrafficFlowProvider, TrafficIncidentProvider};
use geospatial_types::{BoundingBox, GeographicCoordinate};
use rustic_maps_core::everymap_bridge::EveryMapTrafficProvider;
use std::sync::Arc;

/// HTTP status the scripted server returns for a payload.
const HTTP_OK_STATUS: u16 = 200;

/// A server answering each request path fragment with its committed JSON
/// payload.
fn fixture_server(routes: Vec<(&str, Vec<u8>)>) -> LoopbackFixtureServer {
    LoopbackFixtureServer::start(
        routes
            .into_iter()
            .map(|(path_fragment, payload)| {
                ScriptedResponse::json(HTTP_OK_STATUS, payload).for_path_fragment(path_fragment)
            })
            .collect(),
    )
}

fn fixture_bytes(provider_key: &str, file_name: &str) -> Vec<u8> {
    common::fixture_bytes(&format!("fixtures/{provider_key}/{file_name}"))
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
    let server = fixture_server(vec![
        ("/flow", fixture_bytes("here", "traffic_flow_berlin.json")),
        (
            "/incidents",
            fixture_bytes("here", "traffic_incidents_berlin.json"),
        ),
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
    let server = fixture_server(vec![
        (
            "flowSegmentData",
            fixture_bytes("tomtom", "traffic_flow_berlin.json"),
        ),
        (
            "incidentDetails",
            fixture_bytes("tomtom", "traffic_incidents_berlin.json"),
        ),
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
    let server = fixture_server(vec![
        (
            "/traffic/v2/flow",
            fixture_bytes("mapquest", "traffic_flow_new_york.json"),
        ),
        (
            "/traffic/v2/incidents",
            fixture_bytes("mapquest", "traffic_incidents_denver.json"),
        ),
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
