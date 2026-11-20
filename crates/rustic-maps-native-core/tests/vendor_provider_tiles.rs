//! End-to-end tile fetches through each tile-serving vendor provider and the
//! SDK bridge, against the committed tile bytes the browser harness and the
//! rendering fixtures already serve. The loopback server answers every
//! request with one payload, so no test reaches the network.
//!
//! Each case proves the seam the SDK owns: the vendor provider constructs with
//! a base URL, the provider layer fetches its tile, and the bridge hands the
//! bytes and address back unchanged.

mod common;

use common::{LoopbackFixtureServer, ScriptedResponse};
use everymap_core::auth::{ApiKeyProvider, AuthProvider};
use geospatial_traits::{RasterTileProvider, VectorTileProvider};
use geospatial_types::{RasterImageFormat, RasterTileResolution};
use rustic_maps_core::everymap_bridge::{EveryMapRasterTileProvider, EveryMapVectorTileProvider};
use std::sync::Arc;

/// HTTP status the scripted server returns for a payload.
const HTTP_OK_STATUS: u16 = 200;
/// The media type a tile response carries; the providers read the bytes.
const TILE_CONTENT_TYPE: &str = "application/octet-stream";

/// The recorded PNG the TomTom raster request serves.
const TOMTOM_RASTER_FIXTURE: &str =
    "crates/rustic-maps-rendering/fixtures/render/tiles/13_1331_3143.png";
/// The tile address the recorded PNG carries.
const TOMTOM_RASTER_TILE_X: u32 = 1331;
/// The tile row the recorded PNG carries.
const TOMTOM_RASTER_TILE_Y: u32 = 3143;
/// The zoom the recorded PNG was rendered at.
const TOMTOM_RASTER_ZOOM_LEVEL: u8 = 13;

/// The recorded vector tile HERE's request serves, shared with the other
/// vector sources rather than duplicated.
const HERE_VECTOR_FIXTURE: &str = "fixtures/openstreetmap-us/sourdough_tile_z6_x18_y27.mvt";
/// The tile address the shared vector payload carries.
const HERE_VECTOR_TILE_X: u32 = 18;
/// The tile row the shared vector payload carries.
const HERE_VECTOR_TILE_Y: u32 = 27;
/// The zoom the shared vector payload carries.
const HERE_VECTOR_ZOOM_LEVEL: u8 = 6;

/// The recorded vector tile Mapbox's request serves, shared with the other
/// vector sources rather than duplicated.
const MAPBOX_VECTOR_FIXTURE: &str = "fixtures/openfreemap/openmaptiles_tile_z14_x2661_y6286.pbf";
/// The tile address the Mapbox payload carries.
const MAPBOX_VECTOR_TILE_X: u32 = 2661;
/// The tile row the Mapbox payload carries.
const MAPBOX_VECTOR_TILE_Y: u32 = 6286;
/// The zoom the Mapbox payload carries.
const MAPBOX_VECTOR_ZOOM_LEVEL: u8 = 14;

/// A server answering every request with one committed tile payload.
fn fixture_server(payload: Vec<u8>) -> LoopbackFixtureServer {
    LoopbackFixtureServer::start(vec![ScriptedResponse::with_content_type(
        HTTP_OK_STATUS,
        TILE_CONTENT_TYPE,
        payload,
    )])
}

fn fixture_bytes(relative_path: &str) -> Vec<u8> {
    common::fixture_bytes(relative_path)
}

fn api_key_auth(credential: &str, parameter: &str) -> Arc<dyn AuthProvider> {
    Arc::new(ApiKeyProvider::new(
        credential.to_string(),
        parameter.to_string(),
    ))
}

#[tokio::test]
async fn tomtom_raster_tiles_carry_the_recorded_png() {
    let recorded_png = fixture_bytes(TOMTOM_RASTER_FIXTURE);
    let server = fixture_server(recorded_png.clone());
    let client = Arc::new(everymap_providers_tomtom::TomTomClient::new(api_key_auth(
        "placeholder-key",
        "key",
    )));
    let provider = EveryMapRasterTileProvider::new(Arc::new(
        everymap_providers_tomtom::TomTomTileProvider::with_base_url(
            client,
            server.base_url().to_string(),
        ),
    ));

    let tile = provider
        .fetch_raster_tile(
            TOMTOM_RASTER_TILE_X,
            TOMTOM_RASTER_TILE_Y,
            TOMTOM_RASTER_ZOOM_LEVEL,
            RasterImageFormat::Png,
            RasterTileResolution::Standard,
        )
        .await
        .expect("the vendor provider answers the committed tile");
    assert_eq!(
        tile.image_bytes, recorded_png,
        "the recorded tile's bytes survive the bridge"
    );
    assert_eq!(tile.tile_x, TOMTOM_RASTER_TILE_X);
    assert_eq!(tile.tile_y, TOMTOM_RASTER_TILE_Y);
    assert_eq!(tile.zoom_level, TOMTOM_RASTER_ZOOM_LEVEL);
    assert_eq!(tile.image_format, RasterImageFormat::Png);
    assert_eq!(tile.tile_resolution, RasterTileResolution::Standard);
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn here_vector_tiles_carry_the_recorded_tile() {
    let recorded_tile = fixture_bytes(HERE_VECTOR_FIXTURE);
    let server = fixture_server(recorded_tile.clone());
    let client = Arc::new(everymap_providers_here::HereClient::new(api_key_auth(
        "placeholder-key",
        "apiKey",
    )));
    let provider = EveryMapVectorTileProvider::new(Arc::new(
        everymap_providers_here::HereTileProvider::with_base_url(
            client,
            server.base_url().to_string(),
        ),
    ));

    let descriptor = provider.source_descriptor();
    assert_eq!(descriptor.source_name, "here/vector");
    assert!(
        !descriptor.attribution_text.is_empty(),
        "the vendor provider declares its attribution"
    );
    assert!(descriptor.maximum_zoom_level >= HERE_VECTOR_ZOOM_LEVEL);

    let tile = provider
        .fetch_vector_tile(
            HERE_VECTOR_TILE_X,
            HERE_VECTOR_TILE_Y,
            HERE_VECTOR_ZOOM_LEVEL,
        )
        .await
        .expect("the vendor provider answers the committed tile");
    assert_eq!(
        tile.tile_bytes, recorded_tile,
        "the recorded tile's bytes survive the bridge"
    );
    assert_eq!(tile.tile_x, HERE_VECTOR_TILE_X);
    assert_eq!(tile.tile_y, HERE_VECTOR_TILE_Y);
    assert_eq!(tile.zoom_level, HERE_VECTOR_ZOOM_LEVEL);
    assert_eq!(server.request_count(), 1);
}

#[tokio::test]
async fn mapbox_vector_tiles_carry_the_recorded_tile() {
    let recorded_tile = fixture_bytes(MAPBOX_VECTOR_FIXTURE);
    let server = fixture_server(recorded_tile.clone());
    let client = Arc::new(everymap_providers_mapbox::MapBoxClient::new(api_key_auth(
        "placeholder-key",
        "access_token",
    )));
    let provider = EveryMapVectorTileProvider::new(Arc::new(
        everymap_providers_mapbox::MapBoxTileProvider::with_base_url(
            client,
            server.base_url().to_string(),
        ),
    ));

    let descriptor = provider.source_descriptor();
    assert_eq!(descriptor.source_name, "mapbox/mapbox.mapbox-streets-v8");
    assert!(
        !descriptor.attribution_text.is_empty(),
        "the vendor provider declares its attribution"
    );
    assert!(descriptor.maximum_zoom_level >= MAPBOX_VECTOR_ZOOM_LEVEL);

    let tile = provider
        .fetch_vector_tile(
            MAPBOX_VECTOR_TILE_X,
            MAPBOX_VECTOR_TILE_Y,
            MAPBOX_VECTOR_ZOOM_LEVEL,
        )
        .await
        .expect("the vendor provider answers the committed tile");
    assert_eq!(
        tile.tile_bytes, recorded_tile,
        "the recorded tile's bytes survive the bridge"
    );
    assert_eq!(tile.tile_x, MAPBOX_VECTOR_TILE_X);
    assert_eq!(tile.tile_y, MAPBOX_VECTOR_TILE_Y);
    assert_eq!(tile.zoom_level, MAPBOX_VECTOR_ZOOM_LEVEL);
    assert_eq!(server.request_count(), 1);
}
