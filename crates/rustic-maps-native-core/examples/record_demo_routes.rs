//! Records the keyless demo routes: one provider route for every ordered pair
//! of the shared artifact's `demo_places`, merged into
//! `platform/reference-scenes.json` as `demo_routes`.
//!
//! Requires `MAPQUEST_CONSUMER_KEY` in the environment (see `.env.example`).
//! Run manually; CI never depends on live credentials:
//!
//! ```sh
//! set -a; source .env; set +a
//! cargo run -p rustic-maps-native-core --example record_demo_routes
//! ```
//!
//! The recorder answers the same two-waypoint directions call the shipped
//! provider serves, with the demo's own shared route options (pedestrian,
//! fastest), and stores the contract `RouteData` records verbatim. The route
//! goes through the same everymap provider and adapter the SDK registers at
//! runtime, so a recorded route is the route an application gets. It never
//! invents geometry: a pair the provider refuses aborts the recording instead
//! of writing a partial artifact. The API key appears nowhere in the output.

use everymap_providers_mapquest::{MapQuestClient, MapQuestProviderConfiguration, MapQuestRouter};
use geospatial_traits::RoutingProvider;
use geospatial_types::GeographicCoordinate;
use geospatial_types::route::{RouteData, RouteOptions, RouteStrategy, TransportationMode};
use rustic_maps_core::everymap_bridge::EveryMapRouter;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;

/// Default artifact path, relative to the workspace root.
const REFERENCE_SCENES_PATH: &str = "platform/reference-scenes.json";

/// The artifact member this recorder writes.
const DEMO_ROUTES_MEMBER_NAME: &str = "demo_routes";

/// One recorded demo route: the ordered pair of demo places and the provider's
/// route between them.
#[derive(Debug, Serialize)]
struct DemoRouteRecord {
    origin_name: String,
    destination_name: String,
    origin: GeographicCoordinate,
    destination: GeographicCoordinate,
    route: RouteData,
}

/// The demo's shared route options, identical on every platform shell: a
/// pedestrian fastest route, matching the walk the simulated trip follows.
fn demo_route_options() -> RouteOptions {
    RouteOptions {
        transportation_mode: TransportationMode::Pedestrian,
        route_strategy: RouteStrategy::Fastest,
        ..RouteOptions::default()
    }
}

#[tokio::main]
async fn main() {
    let api_key = std::env::var("MAPQUEST_CONSUMER_KEY").unwrap_or_else(|_| {
        panic!("MAPQUEST_CONSUMER_KEY missing; copy .env.example to .env and fill it in")
    });
    let client = Arc::new(MapQuestClient::new(MapQuestProviderConfiguration {
        api_key,
        ..MapQuestProviderConfiguration::default()
    }));
    let router = EveryMapRouter::new(Arc::new(MapQuestRouter::new(client)));

    let artifact_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(REFERENCE_SCENES_PATH));
    let artifact_text = std::fs::read_to_string(&artifact_path)
        .unwrap_or_else(|error| panic!("cannot read {:?}: {error}", artifact_path));
    let artifact: serde_json::Value = serde_json::from_str(&artifact_text)
        .unwrap_or_else(|error| panic!("the artifact is not valid JSON: {error}"));
    let demo_places = artifact["demo_places"]
        .as_array()
        .expect("the artifact carries its demo_places array");

    let places: Vec<(String, GeographicCoordinate)> = demo_places
        .iter()
        .map(|demo_place| {
            let name = demo_place["name"]
                .as_str()
                .expect("a place name")
                .to_string();
            let latitude_degrees = demo_place["latitude_degrees"]
                .as_f64()
                .expect("a place latitude");
            let longitude_degrees = demo_place["longitude_degrees"]
                .as_f64()
                .expect("a place longitude");
            let coordinate =
                GeographicCoordinate::from_degrees(latitude_degrees, longitude_degrees)
                    .expect("the artifact's demo places are valid coordinates");
            (name, coordinate)
        })
        .collect();

    let mut demo_routes = Vec::new();
    for origin_place in &places {
        for destination_place in &places {
            if origin_place.0 == destination_place.0 {
                continue;
            }
            println!("routing {} -> {}…", origin_place.0, destination_place.0);
            let route = router
                .calculate_route(
                    &[origin_place.1, destination_place.1],
                    &demo_route_options(),
                )
                .await
                .unwrap_or_else(|error| {
                    panic!(
                        "the provider refused the {} -> {} demo route: {error}",
                        origin_place.0, destination_place.0
                    )
                });
            demo_routes.push(DemoRouteRecord {
                origin_name: origin_place.0.clone(),
                destination_name: destination_place.0.clone(),
                origin: origin_place.1,
                destination: destination_place.1,
                route,
            });
        }
    }
    println!("recorded {} ordered demo routes", demo_routes.len());

    let rendered_member = serde_json::to_string_pretty(&demo_routes).expect("routes serialize");
    let merged_text = merge_json_member(&artifact_text, DEMO_ROUTES_MEMBER_NAME, &rendered_member);
    std::fs::write(&artifact_path, merged_text)
        .unwrap_or_else(|error| panic!("cannot write {:?}: {error}", artifact_path));
    println!("wrote {:?}", artifact_path);
}

/// Replaces (or appends) one top-level JSON member in a JSON document's text,
/// keeping every other member's spelling and order untouched — the artifact is
/// hand-maintained and reviewed, so a whole-file re-serialization would churn
/// it for no reader's benefit. The replacement text must be a self-contained
/// JSON value.
fn merge_json_member(document_text: &str, member_name: &str, member_json: &str) -> String {
    let member_key = format!("\"{member_name}\"");
    if let Some(key_start) = document_text.find(&member_key) {
        let value_start = document_text[key_start..]
            .find(':')
            .map(|colon_offset| key_start + colon_offset + 1)
            .expect("the artifact member carries a value");
        let value_end = json_value_end(document_text, value_start);
        let indented_member = format!("\n  {member_json}").replace('\n', "\n  ");
        return format!(
            "{}{}{}",
            &document_text[..value_start],
            indented_member,
            &document_text[value_end..],
        );
    }
    let closing_brace = document_text
        .rfind('}')
        .expect("the artifact is a JSON object");
    let member_text = member_json.replace('\n', "\n  ");
    format!(
        "{},\n  {member_key}: {member_text}{}",
        document_text[..closing_brace].trim_end(),
        &document_text[closing_brace..],
    )
}

/// Finds the exclusive end offset of the JSON value starting at `value_start`,
/// skipping whitespace and tracking strings so brackets inside string values
/// never count.
fn json_value_end(document_text: &str, value_start: usize) -> usize {
    let bytes = document_text.as_bytes();
    let mut index = value_start;
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    let opening = bytes[index];
    if opening == b'"' {
        index += 1;
        while index < bytes.len() {
            match bytes[index] {
                b'\\' => index += 2,
                b'"' => return index + 1,
                _ => index += 1,
            }
        }
        return bytes.len();
    }
    let (open_bracket, close_bracket) = match opening {
        b'[' => (b'[', b']'),
        b'{' => (b'{', b'}'),
        _ => {
            // A scalar value ends at the next comma or closing brace.
            while index < bytes.len() && bytes[index] != b',' && bytes[index] != b'}' {
                index += 1;
            }
            return index;
        }
    };
    let mut depth = 0usize;
    let mut in_string = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            match byte {
                b'\\' => index += 1,
                b'"' => in_string = false,
                _ => {}
            }
        } else {
            match byte {
                b'"' => in_string = true,
                bracket if bracket == open_bracket => depth += 1,
                bracket if bracket == close_bracket => {
                    depth -= 1;
                    if depth == 0 {
                        return index + 1;
                    }
                }
                _ => {}
            }
        }
        index += 1;
    }
    bytes.len()
}

#[cfg(test)]
mod tests {
    use super::merge_json_member;

    #[test]
    fn replacing_an_existing_member_keeps_the_rest_verbatim() {
        let document = "{\n  \"a\": 1,\n  \"demo_routes\": [1, 2],\n  \"b\": \"x}\"\n}\n";
        let merged = merge_json_member(document, "demo_routes", "[3]");
        assert!(merged.contains("\"a\": 1"), "{merged}");
        assert!(merged.contains("\"demo_routes\": [3]"), "{merged}");
        assert!(merged.contains("\"b\": \"x}\""), "{merged}");
    }

    #[test]
    fn appending_a_missing_member_keeps_the_document_valid() {
        let document = "{\n  \"a\": 1\n}\n";
        let merged = merge_json_member(document, "demo_routes", "[]");
        let parsed: serde_json::Value = serde_json::from_str(&merged).expect("still JSON");
        assert_eq!(parsed["a"], 1);
        assert_eq!(parsed["demo_routes"], serde_json::json!([]));
    }
}
