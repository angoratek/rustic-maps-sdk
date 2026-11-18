//! Records the real MapQuest platform raster tiles the golden-image tests
//! render with into `fixtures/render/tiles/`, so the tests never touch the
//! network: they load these files. Run manually; CI never depends on live
//! credentials. Reads `MAPQUEST_CONSUMER_KEY` from the workspace `.env` (see
//! `.env.example`); the key is never printed or hardcoded:
//!
//! ```sh
//! set -a; source .env; set +a
//! cargo run -p rustic-maps-rendering --example record_golden_tiles
//! ```
//!
//! The tile set matches what `tests/golden_image.rs` renders: every visible
//! tile of both golden scenes (the Sacramento anchor at the scene tile zoom, one
//! camera per scene pitch) plus each scene's fallback parent tile. Recording
//! is idempotent — tiles whose fixture file already exists are skipped.

use everymap_providers_mapquest::{
    MapQuestClient, MapQuestProviderConfiguration, MapQuestRasterTileProvider,
};
use geospatial_traits::RasterTileProvider;
use geospatial_types::{RasterImageFormat, RasterTileResolution};
use rustic_maps_core::everymap_bridge::EveryMapRasterTileProvider;
use rustic_maps_rendering::{DEFAULT_FIELD_OF_VIEW_DEGREES, MapCamera, TileKey, ViewportPixels};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

/// The two scene pitches of the golden tests, top-down and pitched.
const SCENE_PITCHES_DEGREES: [f64; 2] = [0.0, 45.0];
/// Bearing of both golden cameras: north up.
const SCENE_BEARING_DEGREES: f64 = 0.0;
/// Sacramento center; must match the golden scenes' anchor: the California
/// State Capitol / Capitol Park in downtown Sacramento.
const SACRAMENTO_LATITUDE_DEGREES: f64 = 38.5766;
/// Sacramento center longitude.
const SACRAMENTO_LONGITUDE_DEGREES: f64 = -121.4944;
/// Tile zoom the golden scenes schedule at; must match
/// `SCENE_TILE_ZOOM_LEVEL` in `tests/golden_image.rs`.
const SCENE_TILE_ZOOM_LEVEL: u8 = 14;
/// Frame edge of the golden scenes in pixels.
const GOLDEN_VIEWPORT_EDGE_PIXELS: f64 = 512.0;
/// Directory the recorded tile fixtures land in, relative to the crate
/// manifest; `tests/golden_image.rs` loads from the same directory.
const TILE_FIXTURE_DIRECTORY: &str = "fixtures/render/tiles";
/// The workspace environment file carrying the consumer key.
const ENVIRONMENT_FILE: &str = ".env";
/// Environment variable holding the MapQuest consumer key.
const CONSUMER_KEY_ENVIRONMENT_VARIABLE: &str = "MAPQUEST_CONSUMER_KEY";

/// Returns the file a tile is recorded to, under the crate manifest in the
/// fixture directory (the same roots `tests/golden_image.rs` reads from).
fn fixture_tile_path(tile_key: TileKey) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(TILE_FIXTURE_DIRECTORY)
        .join(tile_file_name(tile_key))
}

/// File name a tile is recorded under: its `zoom_tile_x_tile_y.png`
/// web-mercator XYZ address.
fn tile_file_name(tile_key: TileKey) -> String {
    format!(
        "{}_{}_{}.png",
        tile_key.zoom_level, tile_key.tile_x, tile_key.tile_y
    )
}

/// Extracts the consumer key from the workspace `.env` file in the shape of a
/// dotenv assignment, or takes it from the process environment when already
/// set. The key never reaches a message.
fn load_consumer_key() -> String {
    if let Ok(from_environment) = std::env::var(CONSUMER_KEY_ENVIRONMENT_VARIABLE) {
        return from_environment;
    }
    let environment_file_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join(ENVIRONMENT_FILE);
    let environment_text =
        std::fs::read_to_string(&environment_file_path).unwrap_or_else(|error| {
            panic!(
                "the consumer key was neither in the environment nor in {environment_file_path:?}: \
             {error} (copy .env.example to .env and fill it in)"
            )
        });
    for line in environment_text.lines() {
        if let Some(value) = line
            .strip_prefix(CONSUMER_KEY_ENVIRONMENT_VARIABLE)
            .and_then(|remainder| remainder.strip_prefix('='))
        {
            let trimmed_value = value.trim().trim_matches('"');
            if !trimmed_value.is_empty() {
                return trimmed_value.to_string();
            }
        }
    }
    panic!(
        "{CONSUMER_KEY_ENVIRONMENT_VARIABLE} in {environment_file_path:?} carries no value; \
         copy .env.example to .env and fill it in"
    )
}

#[tokio::main]
async fn main() {
    let consumer_key = load_consumer_key();
    let provider_client = Arc::new(MapQuestClient::new(MapQuestProviderConfiguration {
        api_key: consumer_key,
        ..MapQuestProviderConfiguration::default()
    }));
    let tile_provider =
        EveryMapRasterTileProvider::new(Arc::new(MapQuestRasterTileProvider::new(provider_client)));

    let viewport = ViewportPixels::new(GOLDEN_VIEWPORT_EDGE_PIXELS, GOLDEN_VIEWPORT_EDGE_PIXELS)
        .expect("the golden viewport is a valid viewport");
    let mut required_tile_keys = BTreeSet::new();
    for scene_pitch_degrees in SCENE_PITCHES_DEGREES {
        let scene_camera = MapCamera::new(
            geospatial_types::GeographicCoordinate::from_degrees(
                SACRAMENTO_LATITUDE_DEGREES,
                SACRAMENTO_LONGITUDE_DEGREES,
            )
            .expect("the golden scene anchor is a valid coordinate"),
            f64::from(SCENE_TILE_ZOOM_LEVEL),
            SCENE_BEARING_DEGREES,
            scene_pitch_degrees,
        );
        let scene_tile_keys = scene_camera.visible_tile_keys(
            &viewport,
            DEFAULT_FIELD_OF_VIEW_DEGREES,
            SCENE_TILE_ZOOM_LEVEL,
        );
        let fallback_tile_key = scene_tile_keys
            .last()
            .copied()
            .expect("the golden scene always shows at least one tile");
        let fallback_parent_tile_key = fallback_tile_key
            .parent_tile_key()
            .expect("the scene's fallback tile is not the world root");
        required_tile_keys.extend(scene_tile_keys);
        required_tile_keys.insert(fallback_parent_tile_key);
    }

    println!(
        "{} distinct tiles are required by the golden scenes",
        required_tile_keys.len()
    );
    let mut recorded_tile_count = 0usize;
    for tile_key in &required_tile_keys {
        let tile_fixture_path = fixture_tile_path(*tile_key);
        if tile_fixture_path.exists() {
            println!("already recorded: {}", tile_fixture_path.display());
            continue;
        }
        let tile = tile_provider
            .fetch_raster_tile(
                tile_key.tile_x,
                tile_key.tile_y,
                tile_key.zoom_level,
                RasterImageFormat::Png,
                RasterTileResolution::Standard,
            )
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "tile {tile_key:?} failed to fetch for {:?}: {error}",
                    tile_fixture_path
                )
            });
        std::fs::create_dir_all(tile_fixture_path.parent().unwrap())
            .expect("the tile fixture directory is writable");
        std::fs::write(&tile_fixture_path, &tile.image_bytes)
            .expect("the recorded tile fixture is writable");
        println!(
            "recorded {} bytes -> {}",
            tile.image_bytes.len(),
            tile_fixture_path.display()
        );
        recorded_tile_count += 1;
    }
    println!("recorded {recorded_tile_count} new tile fixture(s)");
}
