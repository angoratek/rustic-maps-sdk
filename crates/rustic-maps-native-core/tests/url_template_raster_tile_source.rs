//! End-to-end tests for the provider-neutral base-map registration
//! (`MapSdk::register_url_template_raster_tile_source`) against the shared
//! loopback fixture server, so no test reaches the network.
//!
//! They prove what the contract promises: a registered source serves the
//! SDK's raster-tile slot with its own declared format and resolution, a
//! refused registration leaves the previously serving source in place, and
//! registering drops an installed tile-cache installation.

mod common;

use common::{LoopbackFixtureServer, ScriptedResponse};
use rustic_maps::{
    MapSdk, RasterImageFormat, RasterTileResolution, SdkError, UrlTemplateRasterTileSource,
};

/// Smallest plausible PNG envelope (the 8-byte signature).
const SAMPLE_TILE_BYTES: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// HTTP status the scripted server returns for a successful tile.
const HTTP_OK_STATUS: u16 = 200;

/// The media type the sample tile is served as.
const PNG_CONTENT_TYPE: &str = "image/png";

/// The consumer key the tile-cache installation test registers MapQuest
/// with: constructing the providers touches no network, and the test never
/// fetches through them.
const MAPQUEST_TEST_CONSUMER_KEY: &str = "test-consumer-key";

/// Byte budget of the tile-cache installation test's temp database.
const TEST_TILE_CACHE_BUDGET_BYTES: u64 = 16 * 1024 * 1024;

/// A server answering every request with the sample tile.
fn fixture_server() -> LoopbackFixtureServer {
    LoopbackFixtureServer::start(vec![ScriptedResponse::with_content_type(
        HTTP_OK_STATUS,
        PNG_CONTENT_TYPE,
        SAMPLE_TILE_BYTES.to_vec(),
    )])
}

/// A source declaration pointing at the server.
fn raster_tile_source(
    server: &LoopbackFixtureServer,
    source_name: &str,
) -> UrlTemplateRasterTileSource {
    UrlTemplateRasterTileSource {
        url_template: format!("{}/tiles/{{z}}/{{x}}/{{y}}.png", server.base_url()),
        source_name: source_name.to_string(),
        minimum_zoom_level: 0,
        maximum_zoom_level: 19,
        image_format: RasterImageFormat::Png,
        tile_resolution: RasterTileResolution::Standard,
        user_agent: "RusticMapsTest/1.0 (+https://example.test)".to_string(),
        token_bucket_capacity_tokens: None,
        refill_tokens_per_second: None,
    }
}

#[tokio::test]
async fn a_registered_source_serves_the_raster_tile_slot() {
    let server = fixture_server();
    let map_sdk = MapSdk::new();
    map_sdk
        .register_url_template_raster_tile_source(raster_tile_source(&server, "test/base-map"))
        .expect("a valid declaration registers");

    let tile = map_sdk
        .fetch_raster_tile(
            2,
            1,
            3,
            RasterImageFormat::Png,
            RasterTileResolution::Standard,
        )
        .await
        .expect("the registered source serves the slot");

    assert_eq!(tile.tile_x, 2);
    assert_eq!(tile.tile_y, 1);
    assert_eq!(tile.zoom_level, 3);
    assert_eq!(tile.image_bytes, SAMPLE_TILE_BYTES.to_vec());
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn a_declared_pacing_reaches_the_registered_source() {
    let server = fixture_server();
    let map_sdk = MapSdk::new();
    map_sdk
        .register_url_template_raster_tile_source(UrlTemplateRasterTileSource {
            token_bucket_capacity_tokens: Some(1.0),
            refill_tokens_per_second: Some(0.001),
            ..raster_tile_source(&server, "test/strict-pacing")
        })
        .expect("a strict but usable declaration registers");

    map_sdk
        .fetch_raster_tile(
            2,
            1,
            3,
            RasterImageFormat::Png,
            RasterTileResolution::Standard,
        )
        .await
        .expect("the declared one-token burst admits the first tile");
    // The default eight-token bucket would admit this second tile at once;
    // the declaration's one token, refilling at a thousandth of a token per
    // second, cannot fund it inside the SDK's ten-second ceiling, so the
    // provider refuses as soon as the budget is known to be out of reach
    // instead of holding the call. That refusal is the proof the declared
    // fields reached the provider.
    let second_tile = tokio::time::timeout(
        std::time::Duration::from_millis(200),
        map_sdk.fetch_raster_tile(
            2,
            1,
            3,
            RasterImageFormat::Png,
            RasterTileResolution::Standard,
        ),
    )
    .await
    .expect("the declared budget answers within the ceiling instead of waiting");
    assert!(
        matches!(second_tile, Err(SdkError::RateLimitExhausted)),
        "the declared one-token budget refuses the second tile: {second_tile:?}"
    );
    assert_eq!(server.request_count(), 1);
}

#[test]
fn an_unusable_declared_pacing_is_refused_at_registration() {
    let server = fixture_server();
    let map_sdk = MapSdk::new();
    match map_sdk.register_url_template_raster_tile_source(UrlTemplateRasterTileSource {
        token_bucket_capacity_tokens: Some(0.0),
        ..raster_tile_source(&server, "test/strict-pacing")
    }) {
        Err(SdkError::InvalidCoordinateInput { detail }) => {
            assert!(
                detail.contains("test/strict-pacing tile source"),
                "{detail}"
            );
        }
        other => panic!("a zero burst capacity must be refused, got {other:?}"),
    }
    assert_eq!(server.request_count(), 0);
}

#[tokio::test]
async fn a_refused_registration_leaves_the_previous_source_serving() {
    let first_server = fixture_server();
    let second_server = fixture_server();
    let map_sdk = MapSdk::new();
    map_sdk
        .register_url_template_raster_tile_source(raster_tile_source(&first_server, "test/first"))
        .expect("the first declaration registers");

    let refused = map_sdk.register_url_template_raster_tile_source(UrlTemplateRasterTileSource {
        url_template: format!("{}/tiles/{{x}}/{{y}}.png", second_server.base_url()),
        ..raster_tile_source(&second_server, "test/second")
    });
    match refused {
        Err(SdkError::InvalidCoordinateInput { detail }) => {
            assert!(detail.contains("lacks the {z} placeholder"), "{detail}");
        }
        other => panic!("a template without {{z}} must be refused, got {other:?}"),
    }

    map_sdk
        .fetch_raster_tile(
            2,
            1,
            3,
            RasterImageFormat::Png,
            RasterTileResolution::Standard,
        )
        .await
        .expect("the previously registered source still serves");
    assert_eq!(first_server.request_count(), 1);
    assert_eq!(second_server.request_count(), 0);
}

#[test]
fn a_blank_source_name_is_refused() {
    let server = fixture_server();
    let map_sdk = MapSdk::new();
    assert!(matches!(
        map_sdk.register_url_template_raster_tile_source(raster_tile_source(&server, " ")),
        Err(SdkError::InvalidCoordinateInput { .. })
    ));
}

#[test]
fn an_inverted_zoom_range_is_refused() {
    let server = fixture_server();
    let map_sdk = MapSdk::new();
    assert!(matches!(
        map_sdk.register_url_template_raster_tile_source(UrlTemplateRasterTileSource {
            minimum_zoom_level: 12,
            maximum_zoom_level: 8,
            ..raster_tile_source(&server, "test/inverted")
        }),
        Err(SdkError::InvalidCoordinateInput { .. })
    ));
}

#[test]
fn registering_drops_the_installed_tile_cache() {
    let server = fixture_server();
    let cache_directory = std::env::temp_dir().join(format!(
        "rustic-maps-native-core-url-template-cache-{}",
        std::process::id()
    ));
    let map_sdk = MapSdk::new();
    map_sdk
        .set_provider_credential_with_tile_cache(
            "mapquest".to_string(),
            MAPQUEST_TEST_CONSUMER_KEY.to_string(),
            cache_directory
                .join("tiles.sqlite")
                .to_string_lossy()
                .to_string(),
            TEST_TILE_CACHE_BUDGET_BYTES,
        )
        .expect("an empty path in an existing directory opens the cache");
    assert!(map_sdk.raster_tile_cache_terms().is_ok());

    map_sdk
        .register_url_template_raster_tile_source(raster_tile_source(&server, "test/base-map"))
        .expect("a valid declaration registers");

    // The cache belonged to the provider that installed it: the settings
    // surface must say so rather than report another provider's ceiling.
    assert!(matches!(
        map_sdk.raster_tile_cache_terms(),
        Err(SdkError::ProviderNotConfigured { .. })
    ));
    std::fs::remove_dir_all(&cache_directory).expect("the test cache directory cleans up");
}
