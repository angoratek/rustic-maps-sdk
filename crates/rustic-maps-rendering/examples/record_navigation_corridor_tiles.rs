//! Records the deep-navigation raster tile corridor the reference demos'
//! follow camera renders with once the demo imagery reaches the navigation
//! follow zoom (`docs/ReferenceAppUX.md` §2.4's `navigation-follow-zoom`):
//! zooms 15–17 along the demo route polyline from
//! `platform/reference-scenes.json` (`overlay_scenes.route`), whose tile span
//! at zoom 13–14 is exactly the neighborhood the existing
//! `record_golden_tiles` / `record_playground_tiles` fixtures cover. Run
//! manually; CI never depends on live credentials. Reads
//! `MAPQUEST_CONSUMER_KEY` from the workspace `.env` (see `.env.example`);
//! the key is never printed or hardcoded:
//!
//! ```sh
//! set -a; source .env; set +a
//! cargo run -p rustic-maps-rendering --example record_navigation_corridor_tiles
//! ```
//!
//! The coverage mirrors `record_playground_tiles`' padding policy applied to
//! the demo corridor instead of the playground camera: at every corridor zoom
//! the standard-resolution set is the union of the corridor poses' visible
//! tiles (samples along the route at the §2.4 follow pitch and a cardinal
//! bearing sweep, plus the §5.3 2D pose), so any follow-camera rendering on
//! the corridor schedules recorded tiles; the retina set is the route's own
//! tile span padded to the same smaller center block the playground's retina
//! footprint uses and limited to the standard set (the follow camera centers
//! always lie on the route, so the retina bytes stay within budget by
//! covering only the corridor core).
//!
//! Recording is idempotent — tiles whose fixture file already exists are
//! skipped. Pass `--plan` to print the per-zoom tile sets without fetching.

use everymap_providers_mapquest::{
    MapQuestClient, MapQuestProviderConfiguration, MapQuestRasterTileProvider,
};
use geospatial_traits::RasterTileProvider;
use geospatial_types::{RasterImageFormat, RasterTileResolution};
use rustic_maps_core::everymap_bridge::EveryMapRasterTileProvider;
use rustic_maps_rendering::{
    DEFAULT_FIELD_OF_VIEW_DEGREES, MapCamera, TileKey, ViewportPixels, tile_key_for_coordinate,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

/// The deepest zoom the recorded corridor reaches: the reference demos'
/// navigation follow zoom (`docs/ReferenceAppUX.md` §2.4).
const CORRIDOR_DEEPEST_ZOOM_LEVEL: u8 = 17;
/// The shallowest zoom the recorded corridor reaches: one level below the
/// already recorded golden zoom 14, so each corridor zoom always has a
/// fallback parent recorded one level shallower.
const CORRIDOR_SHALLOWEST_ZOOM_LEVEL: u8 = 15;
/// Corridor camera poses carry the §2.4 navigation follow pitch in 3D.
const FOLLOW_PITCH_DEGREES: f64 = 45.0;
/// Corridor camera poses carry the §5.3 2D follow pitch.
const TWO_DIMENSIONAL_PITCH_DEGREES: f64 = 0.0;
/// The cardinal bearing sweep each corridor pose is rendered from: a
/// course-follow camera can face any bearing along the route, and the
/// visible-tile answer is the frustum's axis-aligned bounding box, so the
/// four cardinal faces cover a rotated pose's view conservatively.
const BEARING_SWEEP_DEGREES: [f64; 4] = [0.0, 90.0, 180.0, 270.0];
/// North-up bearing of the 2D corridor pose.
const TWO_DIMENSIONAL_BEARING_DEGREES: f64 = 0.0;
/// Route samples are placed on each polyline segment at most this far apart,
/// so consecutive poses' views overlap along the whole corridor.
const CORRIDOR_SAMPLE_SPACING_METERS: f64 = 150.0;
/// Viewport the corridor poses render: the playground's Playwright viewport,
/// the same one `record_playground_tiles` records its drag-panning margin for.
const CORRIDOR_VIEWPORT_WIDTH_PIXELS: f64 = 640.0;
const CORRIDOR_VIEWPORT_HEIGHT_PIXELS: f64 = 480.0;
/// Minimum retina-resolution footprint per axis (the same smaller center-block
/// budget `record_playground_tiles` keeps its retina bytes within).
const RETINA_FOOTPRINT_TILES: u32 = 4;
/// Directory the recorded tile fixtures land in (the recorded demo imagery
/// the golden tests load and `platform/web` serves same-origin).
const TILE_FIXTURE_DIRECTORY: &str = "fixtures/render/tiles";
/// The shared demo artifact carrying the demo route polyline, relative to the
/// crate manifest.
const REFERENCE_SCENES_FILE: &str = "../../platform/reference-scenes.json";
/// The workspace environment file carrying the consumer key.
const ENVIRONMENT_FILE: &str = ".env";
/// Environment variable holding the MapQuest consumer key.
const CONSUMER_KEY_ENVIRONMENT_VARIABLE: &str = "MAPQUEST_CONSUMER_KEY";
/// Filename suffix marking a retina-resolution tile fixture.
const RETINA_TILE_FILE_SUFFIX: &str = "@2x";

/// Inclusive tile-address span along one axis.
struct TileAxisSpan {
    lowest_tile_index: u32,
    highest_tile_index: u32,
}

impl TileAxisSpan {
    /// Pads the span symmetrically to at least the requested tile count; an
    /// odd remainder extends the high end (mirroring
    /// `record_playground_tiles`' padding policy).
    fn padded_to_at_least(&self, minimum_tile_count: u32) -> TileAxisSpan {
        let tile_count = self.highest_tile_index - self.lowest_tile_index + 1;
        if minimum_tile_count <= tile_count {
            return TileAxisSpan {
                lowest_tile_index: self.lowest_tile_index,
                highest_tile_index: self.highest_tile_index,
            };
        }
        let symmetric_padding_tiles = (minimum_tile_count - tile_count) / 2;
        TileAxisSpan {
            lowest_tile_index: self.lowest_tile_index - symmetric_padding_tiles,
            highest_tile_index: self.highest_tile_index
                + symmetric_padding_tiles
                + (minimum_tile_count - tile_count) % 2,
        }
    }
}

/// Returns the demo route polyline's coordinates in degrees from the shared
/// reference-scene artifact, so the corridor follows one source of truth.
fn demo_route_coordinates_degrees() -> Vec<(f64, f64)> {
    let reference_scenes_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(REFERENCE_SCENES_FILE);
    let reference_scenes_text =
        std::fs::read_to_string(&reference_scenes_path).unwrap_or_else(|error| {
            panic!(
                "the reference scene artifact is unreadable at {reference_scenes_path:?}: {error}"
            )
        });
    let reference_scenes: serde_json::Value = serde_json::from_str(&reference_scenes_text)
        .expect("the reference scene artifact is valid JSON");
    let route_coordinates = reference_scenes["overlay_scenes"]["route"]["coordinates"]
        .as_array()
        .expect("the demo route carries a coordinate list")
        .iter()
        .map(|coordinate| {
            (
                coordinate["latitude_degrees"]
                    .as_f64()
                    .expect("the demo route latitude is a number"),
                coordinate["longitude_degrees"]
                    .as_f64()
                    .expect("the demo route longitude is a number"),
            )
        })
        .collect::<Vec<_>>();
    assert!(
        route_coordinates.len() >= 2,
        "the demo route polyline spans at least one segment"
    );
    route_coordinates
}

/// Returns the corridor's sample centers: the route's segments sampled at the
/// corridor spacing (vertices included), so consecutive poses' views overlap
/// along the whole corridor.
fn corridor_sample_coordinates_degrees() -> Vec<(f64, f64)> {
    let route_coordinates_degrees = demo_route_coordinates_degrees();
    let mut sample_coordinates_degrees = Vec::new();
    for segment_index in 0..route_coordinates_degrees.len() - 1 {
        let segment_start_degrees = route_coordinates_degrees[segment_index];
        let segment_end_degrees = route_coordinates_degrees[segment_index + 1];
        let segment_start_coordinate = geospatial_types::GeographicCoordinate::from_degrees(
            segment_start_degrees.0,
            segment_start_degrees.1,
        )
        .expect("the demo route segment start is a valid coordinate");
        let segment_end_coordinate = geospatial_types::GeographicCoordinate::from_degrees(
            segment_end_degrees.0,
            segment_end_degrees.1,
        )
        .expect("the demo route segment end is a valid coordinate");
        let segment_length_meters =
            segment_start_coordinate.haversine_distance_meters(&segment_end_coordinate);
        let segment_sample_count =
            (segment_length_meters / CORRIDOR_SAMPLE_SPACING_METERS).ceil() as usize;
        for sample_index in 0..=segment_sample_count {
            let segment_ratio = sample_index as f64 / segment_sample_count as f64;
            sample_coordinates_degrees.push((
                segment_start_degrees.0
                    + (segment_end_degrees.0 - segment_start_degrees.0) * segment_ratio,
                segment_start_degrees.1
                    + (segment_end_degrees.1 - segment_start_degrees.1) * segment_ratio,
            ));
        }
    }
    sample_coordinates_degrees
}

/// Returns the corridor's camera poses: each sample rendered at the 3D follow
/// pitch from the cardinal bearing sweep and at the 2D pose (the two follow
/// modes the demo exposes), per corridor zoom.
fn corridor_camera_poses() -> Vec<(MapCamera, u8)> {
    let mut poses = Vec::new();
    for zoom_level in CORRIDOR_SHALLOWEST_ZOOM_LEVEL..=CORRIDOR_DEEPEST_ZOOM_LEVEL {
        for (sample_latitude_degrees, sample_longitude_degrees) in
            corridor_sample_coordinates_degrees()
        {
            let sample_coordinate = geospatial_types::GeographicCoordinate::from_degrees(
                sample_latitude_degrees,
                sample_longitude_degrees,
            )
            .expect("the corridor sample is a valid coordinate");
            for bearing_degrees in BEARING_SWEEP_DEGREES {
                poses.push((
                    MapCamera::new(
                        sample_coordinate,
                        f64::from(zoom_level),
                        bearing_degrees,
                        FOLLOW_PITCH_DEGREES,
                    ),
                    zoom_level,
                ));
            }
            poses.push((
                MapCamera::new(
                    sample_coordinate,
                    f64::from(zoom_level),
                    TWO_DIMENSIONAL_BEARING_DEGREES,
                    TWO_DIMENSIONAL_PITCH_DEGREES,
                ),
                zoom_level,
            ));
        }
    }
    poses
}

/// Returns the file a tile is recorded to (the recorded demo imagery's
/// `zoom_tile_x_tile_y[@2x].png` address).
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
    if std::env::args().any(|argument| argument == "--plan") {
        let (standard_tile_keys_by_zoom, retina_tile_keys_by_zoom) = corridor_tile_sets();
        let mut total_tile_count = 0usize;
        for (zoom_level, standard_tile_keys) in &standard_tile_keys_by_zoom {
            println!(
                "zoom {zoom_level}: {} standard + {} retina tile fixture(s)",
                standard_tile_keys.len(),
                retina_tile_keys_by_zoom[zoom_level].len()
            );
            total_tile_count +=
                standard_tile_keys.len() + retina_tile_keys_by_zoom[zoom_level].len();
        }
        println!("{total_tile_count} corridor tile fixture(s) in total");
        return;
    }

    let (standard_tile_keys_by_zoom, retina_tile_keys_by_zoom) = corridor_tile_sets();
    let consumer_key = load_consumer_key();
    let provider_client = Arc::new(MapQuestClient::new(MapQuestProviderConfiguration {
        api_key: consumer_key,
        ..MapQuestProviderConfiguration::default()
    }));
    let tile_provider =
        EveryMapRasterTileProvider::new(Arc::new(MapQuestRasterTileProvider::new(provider_client)));

    for (zoom_level, standard_tile_keys) in &standard_tile_keys_by_zoom {
        for (tile_resolution, tile_keys) in [
            (RasterTileResolution::Standard, standard_tile_keys),
            (
                RasterTileResolution::Retina,
                &retina_tile_keys_by_zoom[zoom_level],
            ),
        ] {
            let mut recorded_tile_count = 0usize;
            for tile_key in tile_keys {
                let tile_fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join(TILE_FIXTURE_DIRECTORY)
                    .join(tile_fixture_file_name(*tile_key, tile_resolution));
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
                "{tile_resolution:?} at zoom {zoom_level}: {recorded_tile_count} new tile \
                 fixture(s) of {} requested",
                tile_keys.len()
            );
        }
    }
}

/// Returns the corridor's tile sets per zoom: standard, the union of the
/// corridor poses' visible tiles; retina, the route's own tile span padded to
/// the smaller center block and limited to the standard set.
fn corridor_tile_sets() -> (
    BTreeMap<u8, BTreeSet<TileKey>>,
    BTreeMap<u8, BTreeSet<TileKey>>,
) {
    let viewport = ViewportPixels::new(
        CORRIDOR_VIEWPORT_WIDTH_PIXELS,
        CORRIDOR_VIEWPORT_HEIGHT_PIXELS,
    )
    .expect("the corridor viewport is a valid viewport");
    let route_coordinates_degrees = demo_route_coordinates_degrees();

    let mut standard_tile_keys_by_zoom: BTreeMap<u8, BTreeSet<TileKey>> = BTreeMap::new();
    for (pose_camera, zoom_level) in corridor_camera_poses() {
        for tile_key in
            pose_camera.visible_tile_keys(&viewport, DEFAULT_FIELD_OF_VIEW_DEGREES, zoom_level)
        {
            standard_tile_keys_by_zoom
                .entry(zoom_level)
                .or_default()
                .insert(tile_key);
        }
    }

    let mut retina_tile_keys_by_zoom: BTreeMap<u8, BTreeSet<TileKey>> = BTreeMap::new();
    for (zoom_level, standard_tile_keys) in &standard_tile_keys_by_zoom {
        let route_tile_keys = route_coordinates_degrees
            .iter()
            .map(|(latitude_degrees, longitude_degrees)| {
                tile_key_for_coordinate(
                    &geospatial_types::GeographicCoordinate::from_degrees(
                        *latitude_degrees,
                        *longitude_degrees,
                    )
                    .expect("the demo route vertex is a valid coordinate"),
                    *zoom_level,
                )
            })
            .collect::<Vec<_>>();
        let route_x_span = TileAxisSpan {
            lowest_tile_index: route_tile_keys.iter().map(|k| k.tile_x).min().unwrap(),
            highest_tile_index: route_tile_keys.iter().map(|k| k.tile_x).max().unwrap(),
        }
        .padded_to_at_least(RETINA_FOOTPRINT_TILES);
        let route_y_span = TileAxisSpan {
            lowest_tile_index: route_tile_keys.iter().map(|k| k.tile_y).min().unwrap(),
            highest_tile_index: route_tile_keys.iter().map(|k| k.tile_y).max().unwrap(),
        }
        .padded_to_at_least(RETINA_FOOTPRINT_TILES);
        let retina_tile_keys = retina_tile_keys_by_zoom.entry(*zoom_level).or_default();
        for tile_x in route_x_span.lowest_tile_index..=route_x_span.highest_tile_index {
            for tile_y in route_y_span.lowest_tile_index..=route_y_span.highest_tile_index {
                let tile_key = TileKey::new(tile_x, tile_y, *zoom_level)
                    .expect("the retina address is a valid corridor tile");
                if standard_tile_keys.contains(&tile_key) {
                    retina_tile_keys.insert(tile_key);
                }
            }
        }
    }
    (standard_tile_keys_by_zoom, retina_tile_keys_by_zoom)
}
