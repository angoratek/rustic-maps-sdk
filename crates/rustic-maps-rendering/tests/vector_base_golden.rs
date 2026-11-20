//! Golden-image harness for the styled vector base the native surfaces draw.
//!
//! One scene renders the committed OpenMapTiles z14 fixture through the
//! shared `VectorBaseScheduler` with the Bright style, composes the frame's
//! overlays and labels and captures the headless frame. Android, iOS and
//! React Native all draw this same Rust pipeline, so this golden is the
//! native family's styled-base reference; the browser has one golden per
//! named style of its own.
//!
//! Comparison is tolerant in the same way as the raster goldens: GPUs differ
//! in sampler behavior. A missing golden is written deliberately with
//! `UPDATE_GOLDENS=1`; the test skips when no graphics adapter exists.

mod common;

use async_trait::async_trait;
use common::golden::{
    GoldenTolerance, compare_or_create_golden, golden_path, golden_regeneration_is_requested,
};
use geospatial_traits::{VectorTileProvider, VectorTileSourceDescriptor};
use geospatial_types::{GeographicCoordinate, SDKError, VectorTileData};
use rustic_maps_rendering::labels::LabelLayer;
use rustic_maps_rendering::overlay::ClipSpaceOverlayVertex;
use rustic_maps_rendering::vector_base::VectorBaseScheduler;
use rustic_maps_rendering::vector_style::{
    VECTOR_STYLE_ID_BRIGHT_LIGHT, VECTOR_STYLE_ID_CARTO_LIGHT, VECTOR_STYLE_ID_OPENMAPTILES_LIGHT,
};
use rustic_maps_rendering::{
    CapturedFrame, DEFAULT_FIELD_OF_VIEW_DEGREES, GraphicsContextSource, HeadlessTileRenderer,
    MapCamera, TileKey, TileTexturePool, ViewportPixels, WgpuGraphicsContextSource,
};
use rustic_maps_test_support::square_viewport;
use std::sync::Arc;
/// The committed OpenMapTiles z14 fixture's center: the camera keeps exactly
/// the served tile in view.
const TILE_CENTER_LATITUDE_DEGREES: f64 = 38.573_937_515_575_91;
const TILE_CENTER_LONGITUDE_DEGREES: f64 = -121.519_775_390_625;
/// The fixture tile's address.
const FIXTURE_TILE_KEY: TileKey = TileKey {
    tile_x: 2661,
    tile_y: 6286,
    zoom_level: 14,
};
/// The committed fixture the served tile answers with.
const FIXTURE_TILE_BYTES: &[u8] =
    include_bytes!("../../../fixtures/openfreemap/openmaptiles_tile_z14_x2661_y6286.pbf");
/// The golden file the Bright scene pins.
const STYLED_BASE_GOLDEN_FILE: &str = "sacramento_zoom14_bright_vector_base.png";
/// The golden file the OpenMapTiles-schema scene pins: the same fixture tile
/// painted by the schema style, whose symbol layers must draw text.
const OPENMAPTILES_STYLED_BASE_GOLDEN_FILE: &str =
    "sacramento_zoom14_openmaptiles_light_vector_base.png";
/// The synthetic OpenStreetMap US-schema z9 tile over the demo's region: the
/// regional frame, pinned after the road hierarchy work.
const REGIONAL_TILE_KEY: TileKey = TileKey {
    tile_x: 83,
    tile_y: 196,
    zoom_level: 9,
};
const REGIONAL_TILE_BYTES: &[u8] =
    include_bytes!("../../../fixtures/openstreetmap-us/osmus_tile_z9_x83_y196.mvt");
/// The regional tile's center, so the camera keeps exactly it in view.
const REGIONAL_CENTER_LATITUDE_DEGREES: f64 = 38.554_8;
const REGIONAL_CENTER_LONGITUDE_DEGREES: f64 = -121.289_062_5;
/// The golden file the Carto regional scene pins.
const REGIONAL_CARTO_GOLDEN_FILE: &str = "sacramento_region_zoom9_carto_light.png";
/// A provider serving the one committed tile; any other address fails, so
/// only the fixture contributes geometry.
struct FixtureTileProvider;

#[async_trait]
impl VectorTileProvider for FixtureTileProvider {
    fn source_descriptor(&self) -> VectorTileSourceDescriptor {
        VectorTileSourceDescriptor {
            source_name: "golden-vector".to_string(),
            attribution_text: "Committed golden vector tile".to_string(),
            minimum_zoom_level: 0,
            maximum_zoom_level: 14,
            style_hints: Vec::new(),
        }
    }

    async fn fetch_vector_tile(
        &self,
        tile_x: u32,
        tile_y: u32,
        zoom_level: u8,
    ) -> Result<VectorTileData, SDKError> {
        let tile_key = TileKey::new(tile_x, tile_y, zoom_level)?;
        let tile_bytes: &[u8] = if tile_key == FIXTURE_TILE_KEY {
            FIXTURE_TILE_BYTES
        } else if tile_key == REGIONAL_TILE_KEY {
            REGIONAL_TILE_BYTES
        } else {
            return Err(SDKError::ResponseParsingFailure(format!(
                "the golden provider serves no {zoom_level}/{tile_x}/{tile_y}"
            )));
        };
        VectorTileData::from_wire_bytes(tile_x, tile_y, zoom_level, tile_bytes.to_vec())
    }
}

/// Builds the fixture scene's camera (top-down, north up).
fn build_camera() -> MapCamera {
    MapCamera::new(
        GeographicCoordinate {
            latitude: TILE_CENTER_LATITUDE_DEGREES,
            longitude: TILE_CENTER_LONGITUDE_DEGREES,
        },
        14.0,
        0.0,
        0.0,
    )
}

/// Renders the fixture tile through the named style, asserting that the frame
/// draws and that its symbol layers place at least one label: a style that
/// silently lost its text would otherwise still compare as a valid image.
async fn render_styled_base(style_id: &str, style_description: &str) -> Option<CapturedFrame> {
    render_styled_base_scene(
        style_id,
        style_description,
        build_camera(),
        FIXTURE_TILE_KEY.zoom_level,
    )
    .await
}

/// Renders the scene at `camera`, resolving the visible tiles at the camera's
/// zoom, and asserts the frame draws a label.
async fn render_styled_base_scene(
    style_id: &str,
    style_description: &str,
    camera: MapCamera,
    served_zoom_level: u8,
) -> Option<CapturedFrame> {
    let mut renderer = request_renderer_or_skip()?;
    let viewport = square_viewport();
    let mut scheduler = VectorBaseScheduler::from_style_id(Arc::new(FixtureTileProvider), style_id)
        .expect("the committed style loads");
    let visible_tile_keys =
        camera.visible_tile_keys(&viewport, DEFAULT_FIELD_OF_VIEW_DEGREES, served_zoom_level);
    let vector_base_frame = scheduler
        .resolve_frame(&visible_tile_keys, camera.zoom_level)
        .await;
    assert!(
        vector_base_frame.drawn_tile_count > 0,
        "{style_description}: the fixture tile must draw"
    );
    let label_layer =
        vector_base_frame.label_layer(&camera, &viewport, DEFAULT_FIELD_OF_VIEW_DEGREES);
    assert!(
        label_layer.as_ref().is_some_and(|layer| !layer.is_empty()),
        "{style_description}: the style must place at least one label"
    );
    if let Some(background) = vector_base_frame.background {
        renderer.set_clear_color_rgba(background);
    }
    let mut pool = TileTexturePool::new(0);
    Some(
        renderer
            .render_captured_frame_layers_and_labels(
                &camera,
                &viewport,
                &[],
                &mut pool,
                &vector_base_frame.overlays,
                None,
                label_layer.as_ref(),
            )
            .expect("the styled base frame renders"),
    )
}

#[tokio::test]
async fn the_bright_styled_base_matches_its_golden() {
    let Some(captured_frame) =
        render_styled_base(VECTOR_STYLE_ID_BRIGHT_LIGHT, "Bright light").await
    else {
        return;
    };
    compare_or_create_golden(
        &captured_frame,
        STYLED_BASE_GOLDEN_FILE,
        "the Bright vector base scene",
        GoldenTolerance::STANDARD,
    );
}

/// Builds the regional scene's camera (top-down, north up at zoom 9).
fn build_regional_camera() -> MapCamera {
    MapCamera::new(
        GeographicCoordinate {
            latitude: REGIONAL_CENTER_LATITUDE_DEGREES,
            longitude: REGIONAL_CENTER_LONGITUDE_DEGREES,
        },
        f64::from(REGIONAL_TILE_KEY.zoom_level),
        0.0,
        0.0,
    )
}

#[tokio::test]
async fn the_regional_carto_base_draws_its_shields_and_matches_its_golden() {
    let Some(captured_frame) = render_styled_base_scene(
        VECTOR_STYLE_ID_CARTO_LIGHT,
        "Carto light at the regional zoom",
        build_regional_camera(),
        REGIONAL_TILE_KEY.zoom_level,
    )
    .await
    else {
        return;
    };
    compare_or_create_golden(
        &captured_frame,
        REGIONAL_CARTO_GOLDEN_FILE,
        "the regional Carto vector base scene",
        GoldenTolerance::STANDARD,
    );
}

/// The regional Carto scene's placed label layer and its viewport, assembled
/// through the same scheduler path the rendered golden uses.
async fn regional_label_layer() -> (LabelLayer, ViewportPixels) {
    let viewport = square_viewport();
    let camera = build_regional_camera();
    let mut scheduler = VectorBaseScheduler::from_style_id(
        Arc::new(FixtureTileProvider),
        VECTOR_STYLE_ID_CARTO_LIGHT,
    )
    .expect("the committed style loads");
    let visible_tile_keys = camera.visible_tile_keys(
        &viewport,
        DEFAULT_FIELD_OF_VIEW_DEGREES,
        REGIONAL_TILE_KEY.zoom_level,
    );
    let vector_base_frame = scheduler
        .resolve_frame(&visible_tile_keys, camera.zoom_level)
        .await;
    let label_layer = vector_base_frame
        .label_layer(&camera, &viewport, DEFAULT_FIELD_OF_VIEW_DEGREES)
        .expect("the regional scene places labels");
    (label_layer, viewport)
}

/// The regional tile's `routes` layer carries both road and transit refs; the
/// shield path must place both as badges, and neither may paint as a road name.
#[tokio::test]
async fn the_regional_carto_labels_place_a_road_ref_and_a_transit_ref_as_badges() {
    let (label_layer, _viewport) = regional_label_layer().await;
    let labels = label_layer.labels();
    let shield_texts: Vec<&str> = labels
        .iter()
        .filter(|label| label.shield.is_some())
        .map(|label| label.text.as_str())
        .collect();
    // The fixture's `routes` layer carries these road refs.
    let road_reference = shield_texts
        .iter()
        .find(|text| matches!(**text, "5" | "49" | "50" | "65" | "80" | "99"));
    assert!(
        road_reference.is_some(),
        "the regional frame must place a road ref badge; placed shield texts: {shield_texts:?}"
    );
    // ...and these transit refs.
    let transit_reference = shield_texts.iter().find(|text| {
        matches!(
            **text,
            "Blue"
                | "Green"
                | "Gold"
                | "Coast Starlight"
                | "CZ"
                | "Gold Runner"
                | "Capitol Corridor"
        )
    });
    assert!(
        transit_reference.is_some(),
        "the regional frame must place a transit ref badge; placed shield texts: {shield_texts:?}"
    );
    // A route reference never paints as a road name: every badge's text is a
    // reference from the routes layer, and the old ref-only text layer is gone.
    for label in labels {
        if label.shield.is_none() {
            assert!(
                !matches!(
                    label.text.as_str(),
                    "5" | "49"
                        | "50"
                        | "65"
                        | "80"
                        | "99"
                        | "Blue"
                        | "Green"
                        | "Gold"
                        | "Coast Starlight"
                        | "CZ"
                        | "Gold Runner"
                        | "Capitol Corridor"
                ),
                "a route reference painted as plain text: {:?}",
                label.text
            );
        }
    }
}

/// The committed badge vertex stream of the regional Carto scene. The bytes
/// are the encoded solid vertices the native and browser renderers upload, in
/// draw order, so a one-float change in the badge tessellation fails this test
/// even when the tolerant image compare absorbs it.
const BADGE_VERTEX_GOLDEN_FILE: &str = "sacramento_region_zoom9_carto_light_badges.bin";
/// The exact number of badge vertices the regional scene emits.
const EXPECTED_BADGE_VERTEX_COUNT: usize = 576;
/// The exact clip-space extent of the badge stream, `[min x, min y, max x,
/// max y]`.
const EXPECTED_BADGE_CLIP_EXTENT: [f32; 4] =
    [-0.760_031_2, -0.373_952_5, 0.761_984_35, 0.154_146_97];

/// Encodes badge vertices exactly as the renderers upload them: clip position
/// then colour, each component little-endian.
fn badge_vertex_bytes(vertices: &[ClipSpaceOverlayVertex]) -> Vec<u8> {
    const BYTES_PER_VERTEX: usize = std::mem::size_of::<ClipSpaceOverlayVertex>();
    let mut bytes = Vec::with_capacity(vertices.len() * BYTES_PER_VERTEX);
    for vertex in vertices {
        for component in vertex.clip_position {
            bytes.extend_from_slice(&component.to_le_bytes());
        }
        for component in vertex.rgba_color {
            bytes.extend_from_slice(&component.to_le_bytes());
        }
    }
    bytes
}

/// The badge stream's clip-space extent, `[min x, min y, max x, max y]`.
fn badge_clip_extent(vertices: &[ClipSpaceOverlayVertex]) -> [f32; 4] {
    let mut extent = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for vertex in vertices {
        extent[0] = extent[0].min(vertex.clip_position[0]);
        extent[1] = extent[1].min(vertex.clip_position[1]);
        extent[2] = extent[2].max(vertex.clip_position[0]);
        extent[3] = extent[3].max(vertex.clip_position[1]);
    }
    extent
}

/// Pins the regional scene's badge vertex stream byte-for-byte, plus its exact
/// vertex count and clip-space extent. The committed bytes carry both a road
/// ref and a transit ref badge.
///
/// The committed bytes were generated on macOS. The geometry is CPU-only, but
/// a different platform's libm can differ in the last unit in the last place
/// of the projection and rotation math, so a Linux run may produce a different
/// stream. The remedy is a deliberate `UPDATE_GOLDENS=1` regeneration on that
/// platform, reviewed and committed; the assertion stays byte-exact.
#[tokio::test]
async fn the_regional_badge_vertex_stream_is_byte_exact() {
    let (label_layer, viewport) = regional_label_layer().await;
    let vertices = label_layer.quad_vertices(&viewport).badge;
    eprintln!(
        "badge vertex count {} clip extent {:?}",
        vertices.len(),
        badge_clip_extent(&vertices)
    );
    assert_eq!(
        vertices.len(),
        EXPECTED_BADGE_VERTEX_COUNT,
        "the regional badge vertex count changed"
    );
    assert_eq!(
        badge_clip_extent(&vertices),
        EXPECTED_BADGE_CLIP_EXTENT,
        "the regional badge clip extent changed"
    );
    let bytes = badge_vertex_bytes(&vertices);
    let badge_golden_path = golden_path(BADGE_VERTEX_GOLDEN_FILE);
    if !badge_golden_path.exists() {
        if !golden_regeneration_is_requested() {
            panic!(
                "badge vertex golden is missing at {badge_golden_path:?}; regenerate deliberately \
                 with UPDATE_GOLDENS=1 and commit the result"
            );
        }
        std::fs::write(&badge_golden_path, &bytes).expect("the badge vertex golden is written");
        println!("wrote {badge_golden_path:?}; review and commit it");
        return;
    }
    let golden_bytes =
        std::fs::read(&badge_golden_path).expect("the badge vertex golden is readable");
    if bytes != golden_bytes {
        let first_difference = bytes
            .iter()
            .zip(&golden_bytes)
            .position(|(captured, golden)| captured != golden)
            .unwrap_or(bytes.len().min(golden_bytes.len()));
        panic!(
            "the regional badge vertex stream drifted from {badge_golden_path:?}: {} captured \
             bytes, {} golden bytes, first difference at byte {first_difference}",
            bytes.len(),
            golden_bytes.len()
        );
    }
}

#[tokio::test]
async fn the_openmaptiles_styled_base_draws_its_labels_and_matches_its_golden() {
    let Some(captured_frame) =
        render_styled_base(VECTOR_STYLE_ID_OPENMAPTILES_LIGHT, "OpenMapTiles light").await
    else {
        return;
    };
    compare_or_create_golden(
        &captured_frame,
        OPENMAPTILES_STYLED_BASE_GOLDEN_FILE,
        "the OpenMapTiles vector base scene",
        GoldenTolerance::STANDARD,
    );
}

/// Acquires the renderer or prints why the CPU-only environment skips.
fn request_renderer_or_skip() -> Option<HeadlessTileRenderer> {
    match WgpuGraphicsContextSource.request_graphics_context() {
        Ok(graphics_context) => Some(
            HeadlessTileRenderer::with_graphics_context(graphics_context)
                .expect("the pipeline builds on the acquired context"),
        ),
        Err(error) => {
            println!("skipping GPU golden test: no graphics adapter available ({error})");
            None
        }
    }
}
