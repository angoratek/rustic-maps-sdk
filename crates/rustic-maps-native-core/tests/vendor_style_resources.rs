//! Fetches of a vendor's published style resources through the vendor's own
//! authenticated client, against committed synthetic documents. The loopback
//! server answers every request with one payload and records the request
//! target, so the test proves which credential the request carried without
//! reaching the network.
//!
//! Each case proves the seam the SDK owns: the SDK names only the resource
//! address, and the client that serves the provider's contract calls applies
//! the credential, its transport and its request policy. The fetched bytes are
//! a style document the renderer compiles, which is what the vector base
//! consumes.

mod common;

use common::{LoopbackFixtureServer, ScriptedResponse};
use rustic_maps_providers::build_vendor;

/// HTTP status the scripted server returns for a payload.
const HTTP_OK_STATUS: u16 = 200;
/// The media type a style document response carries.
const STYLE_DOCUMENT_CONTENT_TYPE: &str = "application/json";
/// The credential each case registers; the server sees it in the request.
const REGISTERED_CREDENTIAL: &str = "placeholder-credential";
/// The contract the style resources are read for.
const VECTOR_TILES_CONTRACT_NAME: &str = "vector_tiles";

/// The synthetic style document HERE's resource path serves.
const HERE_STYLE_DOCUMENT_FIXTURE: &str = "fixtures/here/vendor_style_document.json";
/// The resource path HERE's client is asked for.
const HERE_STYLE_DOCUMENT_PATH: &str = "/styles/berlin/base/mapbox/tilezen";
/// The query parameter HERE's client appends its credential as.
const HERE_CREDENTIAL_PARAMETER: &str = "apiKey";

/// The synthetic style document Mapbox's resource path serves.
const MAPBOX_STYLE_DOCUMENT_FIXTURE: &str = "fixtures/mapbox/vendor_style_document.json";
/// The resource path Mapbox's client is asked for.
const MAPBOX_STYLE_DOCUMENT_PATH: &str = "/styles/v1/mapbox/streets-v11";
/// The query parameter Mapbox's client appends its credential as.
const MAPBOX_CREDENTIAL_PARAMETER: &str = "access_token";

/// A server answering every request with one committed style document.
fn fixture_server(payload: Vec<u8>) -> LoopbackFixtureServer {
    LoopbackFixtureServer::start(vec![ScriptedResponse::with_content_type(
        HTTP_OK_STATUS,
        STYLE_DOCUMENT_CONTENT_TYPE,
        payload,
    )])
}

fn fixture_bytes(relative_path: &str) -> Vec<u8> {
    common::fixture_bytes(relative_path)
}

/// Asserts the fetched bytes are the committed document and that it compiles
/// into the style the vector base paints with.
fn assert_compiles_to_a_style_document(fetched: Vec<u8>, recorded_document: &[u8]) {
    assert_eq!(
        fetched, recorded_document,
        "the committed document's bytes survive the fetch"
    );
    let document = String::from_utf8(fetched).expect("a style document is UTF-8");
    rustic_maps_rendering::vector_style::vector_tile_style_from_json(&document)
        .expect("the fetched document compiles into a vector style");
}

#[tokio::test]
async fn here_style_resources_carry_the_registered_credential() {
    let recorded_document = fixture_bytes(HERE_STYLE_DOCUMENT_FIXTURE);
    let server = fixture_server(recorded_document.clone());
    let implementations = build_vendor(
        "here",
        REGISTERED_CREDENTIAL,
        None,
        VECTOR_TILES_CONTRACT_NAME,
    )
    .expect("the catalog builds the vendor");
    let style_resources = implementations
        .style_resources
        .expect("HERE publishes style resources for its vector tiles");

    let fetched = style_resources
        .fetch_style_resource(&format!("{}{HERE_STYLE_DOCUMENT_PATH}", server.base_url()))
        .await
        .expect("the vendor client answers the committed style document");
    assert_compiles_to_a_style_document(fetched, &recorded_document);

    let request_targets = server.request_targets();
    assert_eq!(request_targets.len(), 1);
    assert!(
        request_targets[0].contains(&format!(
            "{HERE_CREDENTIAL_PARAMETER}={REGISTERED_CREDENTIAL}"
        )),
        "the registered credential reached the request through the vendor client: {:?}",
        request_targets
    );
}

#[tokio::test]
async fn mapbox_style_resources_carry_the_registered_credential() {
    let recorded_document = fixture_bytes(MAPBOX_STYLE_DOCUMENT_FIXTURE);
    let server = fixture_server(recorded_document.clone());
    let implementations = build_vendor(
        "mapbox",
        REGISTERED_CREDENTIAL,
        None,
        VECTOR_TILES_CONTRACT_NAME,
    )
    .expect("the catalog builds the vendor");
    let style_resources = implementations
        .style_resources
        .expect("Mapbox publishes style resources for its vector tiles");

    let fetched = style_resources
        .fetch_style_resource(&format!(
            "{}{MAPBOX_STYLE_DOCUMENT_PATH}",
            server.base_url()
        ))
        .await
        .expect("the vendor client answers the committed style document");
    assert_compiles_to_a_style_document(fetched, &recorded_document);

    let request_targets = server.request_targets();
    assert_eq!(request_targets.len(), 1);
    assert!(
        request_targets[0].contains(&format!(
            "{MAPBOX_CREDENTIAL_PARAMETER}={REGISTERED_CREDENTIAL}"
        )),
        "the registered credential reached the request through the vendor client: {:?}",
        request_targets
    );
}

#[test]
fn vendors_without_hosted_vector_styles_declare_no_style_resources() {
    for provider_key in ["google", "tomtom", "radar"] {
        let implementations = build_vendor(provider_key, REGISTERED_CREDENTIAL, None, "geocoding")
            .expect("the catalog builds the vendor");
        assert!(
            implementations.style_resources.is_none(),
            "{provider_key} publishes no hosted vector style, so it declares no style resources"
        );
    }
}
