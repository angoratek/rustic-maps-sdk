//! Records the wider raster tile footprint the browser playground
//! (`platform/web`) renders around the same golden Sacramento scene, so the 640
//! x 480 playground viewport fills with tiles and keeps filling while the
//! viewer drag-pans. Run manually; CI never depends on live credentials.
//! Reads `MAPQUEST_CONSUMER_KEY` from the workspace `.env` (see
//! `.env.example`); the key is never printed or hardcoded:
//!
//! ```sh
//! set -a; source .env; set +a
//! cargo run -p rustic-maps-rendering --example record_playground_tiles
//! ```
//!
//! Fixture naming (recorded next to the golden tiles in
//! `fixtures/render/tiles/`):
//!
//! * standard resolution — `zoom_tile_x_tile_y.png` (the existing scheme;
//!   files already recorded by `record_golden_tiles` are kept byte-identical
//!   because recording is idempotent);
//! * retina resolution — `zoom_tile_x_tile_y@2x.png`, the Tiles API's
//!   `@2x` form: twice the texels per edge for the same logical tile span.
//!
//! The standard footprint is the playground's visible tile span padded to at
//! least [`STANDARD_FOOTPRINT_TILES_X`] x [`STANDARD_FOOTPRINT_TILES_Y`]; the
//! retina footprint is the same span padded to at least
//! [`RETINA_FOOTPRINT_TILES_X`] x [`RETINA_FOOTPRINT_TILES_Y`] (smaller, so
//! the committed retina bytes stay within budget). Camera parameters match
//! the golden scenes and the playground's initial pose.

use everymap_providers_mapquest::{
    MapQuestClient, MapQuestProviderConfiguration, MapQuestRasterTileProvider,
};
use geospatial_traits::RasterTileProvider;
use geospatial_types::{RasterImageFormat, RasterTileResolution};
use rustic_maps_core::everymap_bridge::EveryMapRasterTileProvider;
use rustic_maps_rendering::{DEFAULT_FIELD_OF_VIEW_DEGREES, MapCamera, TileKey, ViewportPixels};
use std::path::PathBuf;
use std::sync::Arc;

/// Tile zoom the playground schedules at; the golden scenes' zoom.
const PLAYGROUND_TILE_ZOOM_LEVEL: u8 = 14;
/// Camera pitch of the playground's initial pose: top-down.
const PLAYGROUND_PITCH_DEGREES: f64 = 0.0;
/// Camera bearing of the playground's initial pose: north up.
const PLAYGROUND_BEARING_DEGREES: f64 = 0.0;
/// Playground viewport width in CSS pixels (the Playwright view is 640x480;
/// margin for drag-panning comes from the footprint padding).
const PLAYGROUND_VIEWPORT_WIDTH_PIXELS: f64 = 640.0;
/// Playground viewport height in CSS pixels.
const PLAYGROUND_VIEWPORT_HEIGHT_PIXELS: f64 = 480.0;
/// Minimum standard-resolution footprint width in tiles: the visible span
/// plus margin for drag-panning.
const STANDARD_FOOTPRINT_TILES_X: u32 = 6;
/// Minimum standard-resolution footprint height in tiles.
const STANDARD_FOOTPRINT_TILES_Y: u32 = 5;
/// Minimum retina-resolution footprint width in tiles (the center block, so
/// the committed retina bytes stay small).
const RETINA_FOOTPRINT_TILES_X: u32 = 4;
/// Minimum retina-resolution footprint height in tiles.
const RETINA_FOOTPRINT_TILES_Y: u32 = 3;
/// Filename suffix marking a retina-resolution tile fixture.
const RETINA_TILE_FILE_SUFFIX: &str = "@2x";
/// Sacramento center; must match the golden scenes' anchor and the
/// playground's initial camera: the California State Capitol / Capitol Park in
/// downtown Sacramento.
const SACRAMENTO_LATITUDE_DEGREES: f64 = 38.5766;
/// Sacramento center longitude.
const SACRAMENTO_LONGITUDE_DEGREES: f64 = -121.4944;
/// Directory the recorded tile fixtures land in, relative to the crate
/// manifest; `record_golden_tiles` records into the same directory.
const TILE_FIXTURE_DIRECTORY: &str = "fixtures/render/tiles";
/// The workspace environment file carrying the consumer key.
const ENVIRONMENT_FILE: &str = ".env";
/// Environment variable holding the MapQuest consumer key.
const CONSUMER_KEY_ENVIRONMENT_VARIABLE: &str = "MAPQUEST_CONSUMER_KEY";

/// Inclusive tile-address span along one axis.
struct TileAxisSpan {
    lowest_tile_index: u32,
    highest_tile_index: u32,
}

impl TileAxisSpan {
    /// Pads the span symmetrically to at least the requested tile count; an
    /// odd remainder extends the high end.
    fn padded_to_at_least(&self, minimum_tile_count: u32) -> TileAxisSpan {
        let tile_count = self.highest_tile_index - self.lowest_tile_index + 1;
        assert!(
            minimum_tile_count >= tile_count,
            "the requested footprint is smaller than the visible span"
        );
        let symmetric_padding_tiles = (minimum_tile_count - tile_count) / 2;
        TileAxisSpan {
            lowest_tile_index: self.lowest_tile_index - symmetric_padding_tiles,
            highest_tile_index: self.highest_tile_index
                + symmetric_padding_tiles
                + (minimum_tile_count - tile_count) % 2,
        }
    }
}

/// Returns the file a tile is recorded to, under the crate manifest in the
/// fixture directory.
fn fixture_tile_path(tile_file_name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(TILE_FIXTURE_DIRECTORY)
        .join(tile_file_name)
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

/// Returns the visible tile span of the initial playground camera, which the
/// footprints pad out from.
fn playground_visible_axis_spans(camera: &MapCamera) -> (TileAxisSpan, TileAxisSpan) {
    let viewport = ViewportPixels::new(
        PLAYGROUND_VIEWPORT_WIDTH_PIXELS,
        PLAYGROUND_VIEWPORT_HEIGHT_PIXELS,
    )
    .expect("the playground viewport is a valid viewport");
    let visible_tile_keys = camera.visible_tile_keys(
        &viewport,
        DEFAULT_FIELD_OF_VIEW_DEGREES,
        PLAYGROUND_TILE_ZOOM_LEVEL,
    );
    assert!(
        !visible_tile_keys.is_empty(),
        "the playground camera always shows at least one tile"
    );
    let mut tile_x_span = TileAxisSpan {
        lowest_tile_index: visible_tile_keys[0].tile_x,
        highest_tile_index: visible_tile_keys[0].tile_x,
    };
    let mut tile_y_span = TileAxisSpan {
        lowest_tile_index: visible_tile_keys[0].tile_y,
        highest_tile_index: visible_tile_keys[0].tile_y,
    };
    for tile_key in &visible_tile_keys[..] {
        tile_x_span.lowest_tile_index = tile_x_span.lowest_tile_index.min(tile_key.tile_x);
        tile_x_span.highest_tile_index = tile_x_span.highest_tile_index.max(tile_key.tile_x);
        tile_y_span.lowest_tile_index = tile_y_span.lowest_tile_index.min(tile_key.tile_y);
        tile_y_span.highest_tile_index = tile_y_span.highest_tile_index.max(tile_key.tile_y);
    }
    (tile_x_span, tile_y_span)
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
    let playground_camera = MapCamera::new(
        geospatial_types::GeographicCoordinate::from_degrees(
            SACRAMENTO_LATITUDE_DEGREES,
            SACRAMENTO_LONGITUDE_DEGREES,
        )
        .expect("the playground scene anchor is a valid coordinate"),
        f64::from(PLAYGROUND_TILE_ZOOM_LEVEL),
        PLAYGROUND_BEARING_DEGREES,
        PLAYGROUND_PITCH_DEGREES,
    );
    let (visible_tile_x_span, visible_tile_y_span) =
        playground_visible_axis_spans(&playground_camera);

    // Standard: the visible span padded to the wide drag-panning margin.
    let standard_tile_x_span = visible_tile_x_span.padded_to_at_least(STANDARD_FOOTPRINT_TILES_X);
    let standard_tile_y_span = visible_tile_y_span.padded_to_at_least(STANDARD_FOOTPRINT_TILES_Y);
    // Retina: the same visible span padded to the smaller center block, so
    // the committed retina bytes stay within budget.
    let retina_tile_x_span = visible_tile_x_span.padded_to_at_least(RETINA_FOOTPRINT_TILES_X);
    let retina_tile_y_span = visible_tile_y_span.padded_to_at_least(RETINA_FOOTPRINT_TILES_Y);

    let mut standard_tile_keys = Vec::new();
    for tile_x in standard_tile_x_span.lowest_tile_index..=standard_tile_x_span.highest_tile_index {
        for tile_y in
            standard_tile_y_span.lowest_tile_index..=standard_tile_y_span.highest_tile_index
        {
            standard_tile_keys.push(
                TileKey::new(tile_x, tile_y, PLAYGROUND_TILE_ZOOM_LEVEL)
                    .expect("the recorded address is a valid Sacramento tile"),
            );
        }
    }
    let mut retina_tile_keys = Vec::new();
    for tile_x in retina_tile_x_span.lowest_tile_index..=retina_tile_x_span.highest_tile_index {
        for tile_y in retina_tile_y_span.lowest_tile_index..=retina_tile_y_span.highest_tile_index {
            retina_tile_keys.push(
                TileKey::new(tile_x, tile_y, PLAYGROUND_TILE_ZOOM_LEVEL)
                    .expect("the recorded address is a valid Sacramento tile"),
            );
        }
    }

    for (tile_resolution, tile_keys) in [
        (RasterTileResolution::Standard, standard_tile_keys),
        (RasterTileResolution::Retina, retina_tile_keys),
    ] {
        let mut recorded_tile_count = 0usize;
        for tile_key in &tile_keys {
            let tile_fixture_path =
                fixture_tile_path(&tile_fixture_file_name(*tile_key, tile_resolution));
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
                    tile_resolution,
                )
                .await
                .unwrap_or_else(|error| {
                    panic!(
                        "tile {tile_key:?} at resolution {tile_resolution:?} failed to fetch \
                         for {:?}: {error}",
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
        println!(
            "{tile_resolution:?}: {recorded_tile_count} new tile fixture(s) of {} requested",
            tile_keys.len()
        );
    }
}

/// File name a tile is recorded under: its `zoom_tile_x_tile_y` web-mercator
/// XYZ address, plus the `@2x` marker for retina fixtures.
fn tile_fixture_file_name(tile_key: TileKey, tile_resolution: RasterTileResolution) -> String {
    let retina_suffix = match tile_resolution {
        RasterTileResolution::Standard => "",
        RasterTileResolution::Retina => RETINA_TILE_FILE_SUFFIX,
    };
    format!(
        "{}_{}_{}{}.png",
        tile_key.zoom_level, tile_key.tile_x, tile_key.tile_y, retina_suffix
    )
}
