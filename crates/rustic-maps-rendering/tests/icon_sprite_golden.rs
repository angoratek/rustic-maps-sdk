//! The icon sprite pass rendered end to end: a committed style with an
//! `icon-image` symbol layer over one recorded OpenMapTiles z14 vector tile,
//! composited over the recorded raster tiles of the same Sacramento scene and
//! captured against a committed PNG.
//!
//! The style's `sprite` URL is resolved through an injected, in-memory
//! [`SpriteSource`] that reads the committed index and sheet; nothing here
//! touches the network. The scene proves the whole path: the style document's
//! `icon-*` layout resolves, the sprite index and sheet load, the collision
//! pass reserves both the icon and its text boxes, and the renderer draws the
//! sprite quads through its textured pipeline.
//!
//! Comparison is tolerant like the other goldens: GPUs differ in sampler
//! behavior. A missing golden is written deliberately with `UPDATE_GOLDENS=1`;
//! the test skips when no graphics adapter exists.

mod common;

use async_trait::async_trait;
use common::golden::{GoldenTolerance, compare_or_create_golden};
use geospatial_traits::{VectorTileProvider, VectorTileSourceDescriptor};
use geospatial_types::{GeographicCoordinate, SDKError, VectorTileData};
use image::GenericImageView as _;
use rustic_maps_rendering::sprite::{SpriteAtlas, SpriteAtlasError, SpriteSource};
use rustic_maps_rendering::vector_base::VectorBaseScheduler;
use rustic_maps_rendering::vector_style::vector_tile_style_from_json;
use rustic_maps_rendering::{
    DEFAULT_FIELD_OF_VIEW_DEGREES, GraphicsContextSource, HeadlessTileRenderer, MapCamera,
    PreparedTile, TileKey, TileReadiness, TileTexturePool, ViewportPixels,
    WgpuGraphicsContextSource,
};
use std::path::PathBuf;
use std::sync::Arc;

/// Frame edge in pixels, the other goldens' extent.
const GOLDEN_VIEWPORT_EDGE_PIXELS: f64 = 512.0;
/// The fixture vector tile's center: the camera keeps that tile in view so
/// the scheduler requests the address the fixture provider serves.
const SCENE_CENTER_LATITUDE_DEGREES: f64 = 38.573_937_515_575_92;
/// See [`SCENE_CENTER_LATITUDE_DEGREES`].
const SCENE_CENTER_LONGITUDE_DEGREES: f64 = -121.519_775_390_625;
/// The scene camera's zoom and the served tile zoom.
const SCENE_ZOOM_LEVEL: f64 = 14.0;
/// The scene camera is top-down and north up.
const SCENE_BEARING_DEGREES: f64 = 0.0;
const SCENE_PITCH_DEGREES: f64 = 0.0;
/// Fixture of the icon scene.
const ICON_GOLDEN_FILE: &str = "sacramento_zoom14_icon_sprites.png";
/// The style id the scheduler reports for the scene.
const ICON_STYLE_ID: &str = "icon-sprite-fixture";
/// The committed style document with the icon layer.
const ICON_STYLE_JSON: &str =
    include_str!("../../rustic-maps-style/fixtures/styles/icon-sprite-style.json");
/// The committed sprite index and sheet the style's URL resolves to.
const SPRITE_INDEX_JSON: &[u8] =
    include_bytes!("../../rustic-maps-style/fixtures/sprites/icon-sprite.json");
const SPRITE_SHEET_PNG: &[u8] =
    include_bytes!("../../rustic-maps-style/fixtures/sprites/icon-sprite.png");
/// The sprite URL the fixture source serves; any other URL is refused.
const FIXTURE_SPRITE_URL: &str = "fixture://icon-sprite";
/// The committed OpenFreeMap z14 tile whose `poi` layer names the bus stops.
const VECTOR_TILE_BYTES: &[u8] =
    include_bytes!("../../../fixtures/openfreemap/openmaptiles_tile_z14_x2661_y6286.pbf");
/// The fixture tile's address.
const VECTOR_TILE_KEY: TileKey = TileKey {
    tile_x: 2661,
    tile_y: 6286,
    zoom_level: 14,
};
/// Directory of the recorded raster tile fixtures, relative to the manifest.
const TILE_FIXTURE_DIRECTORY: &str = "fixtures/render/tiles";
/// Edge length in texels the recorded raster tiles carry.
const RECORDED_TILE_EDGE_PIXELS: u32 = 256;
/// Encoded-pixel budget of the tile pool.
const SCENE_POOL_BUDGET_BYTES: u64 = 64 << 20;
/// Serves the committed vector tile and refuses every other address.
struct FixtureTileProvider;

#[async_trait]
impl VectorTileProvider for FixtureTileProvider {
    fn source_descriptor(&self) -> VectorTileSourceDescriptor {
        VectorTileSourceDescriptor {
            source_name: "icon-golden-vector".to_string(),
            attribution_text: "Committed icon golden vector tile".to_string(),
            minimum_zoom_level: VECTOR_TILE_KEY.zoom_level,
            maximum_zoom_level: VECTOR_TILE_KEY.zoom_level,
            style_hints: Vec::new(),
        }
    }

    async fn fetch_vector_tile(
        &self,
        tile_x: u32,
        tile_y: u32,
        zoom_level: u8,
    ) -> Result<VectorTileData, SDKError> {
        if (tile_x, tile_y, zoom_level)
            == (
                VECTOR_TILE_KEY.tile_x,
                VECTOR_TILE_KEY.tile_y,
                VECTOR_TILE_KEY.zoom_level,
            )
        {
            return VectorTileData::from_wire_bytes(
                tile_x,
                tile_y,
                zoom_level,
                VECTOR_TILE_BYTES.to_vec(),
            );
        }
        Err(SDKError::ResponseParsingFailure(format!(
            "the icon fixture serves no {zoom_level}/{tile_x}/{tile_y}"
        )))
    }
}

/// Serves the committed sprite index and sheet to the style's fixture URL.
struct FixtureSpriteSource;

impl SpriteSource for FixtureSpriteSource {
    fn load_sprite(&self, sprite_url: &str) -> Result<SpriteAtlas, SpriteAtlasError> {
        if sprite_url != FIXTURE_SPRITE_URL {
            return Err(SpriteAtlasError {
                detail: format!("the fixture sprite source serves no {sprite_url:?}"),
            });
        }
        SpriteAtlas::from_index_and_sheet_png(SPRITE_INDEX_JSON, SPRITE_SHEET_PNG)
    }
}

fn build_camera() -> MapCamera {
    MapCamera::new(
        GeographicCoordinate {
            latitude: SCENE_CENTER_LATITUDE_DEGREES,
            longitude: SCENE_CENTER_LONGITUDE_DEGREES,
        },
        SCENE_ZOOM_LEVEL,
        SCENE_BEARING_DEGREES,
        SCENE_PITCH_DEGREES,
    )
}

fn golden_viewport() -> ViewportPixels {
    ViewportPixels::new(GOLDEN_VIEWPORT_EDGE_PIXELS, GOLDEN_VIEWPORT_EDGE_PIXELS)
        .expect("the viewport extent is valid")
}

fn recorded_tile_path(tile_key: TileKey) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(TILE_FIXTURE_DIRECTORY)
        .join(format!(
            "{}_{}_{}.png",
            tile_key.zoom_level, tile_key.tile_x, tile_key.tile_y
        ))
}

/// Fills the pool with the scene's recorded raster tiles and marks every tile
/// present. A missing fixture is a hard error naming the path; the test never
/// synthesizes or fetches imagery.
fn fill_raster_pool_and_prepare(camera: &MapCamera) -> (TileTexturePool, Vec<PreparedTile>) {
    let scene_tile_keys = camera.visible_tile_keys(
        &golden_viewport(),
        DEFAULT_FIELD_OF_VIEW_DEGREES,
        VECTOR_TILE_KEY.zoom_level,
    );
    let mut pool = TileTexturePool::new(SCENE_POOL_BUDGET_BYTES);
    for tile_key in &scene_tile_keys {
        let tile_path = recorded_tile_path(*tile_key);
        let tile_bytes = std::fs::read(&tile_path).unwrap_or_else(|error| {
            panic!("the recorded raster tile {tile_path:?} is unreadable: {error}")
        });
        let decoded = image::load_from_memory(&tile_bytes).unwrap_or_else(|error| {
            panic!("the recorded raster tile {tile_path:?} is undecodable: {error}")
        });
        assert_eq!(
            decoded.dimensions(),
            (RECORDED_TILE_EDGE_PIXELS, RECORDED_TILE_EDGE_PIXELS),
            "the recorded raster tile {tile_path:?} has an unexpected edge"
        );
        pool.insert(*tile_key, tile_bytes);
    }
    let prepared_tiles = scene_tile_keys
        .into_iter()
        .map(|requested_tile_key| PreparedTile {
            requested_tile_key,
            readiness: TileReadiness::Present,
        })
        .collect();
    (pool, prepared_tiles)
}

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

#[tokio::test]
async fn the_icon_sprite_style_renders_icons_and_text_over_recorded_tiles() {
    let Some(mut renderer) = request_renderer_or_skip() else {
        return;
    };
    let camera = build_camera();
    let viewport = golden_viewport();
    let compiled_style =
        vector_tile_style_from_json(ICON_STYLE_JSON).expect("the icon style compiles");
    assert!(
        compiled_style.sprite.is_some(),
        "the fixture style declares a sprite"
    );
    assert!(
        compiled_style
            .layers
            .iter()
            .any(|layer| matches!(layer.kind, rustic_maps_style::CompiledLayerKind::Symbol)),
        "the fixture style declares a symbol layer"
    );
    let visible_tile_keys = camera.visible_tile_keys(
        &viewport,
        DEFAULT_FIELD_OF_VIEW_DEGREES,
        VECTOR_TILE_KEY.zoom_level,
    );
    let mut scheduler =
        VectorBaseScheduler::new(Arc::new(FixtureTileProvider), ICON_STYLE_ID, compiled_style)
            .with_sprite_source(Arc::new(FixtureSpriteSource));
    let vector_base_frame = scheduler
        .resolve_frame(&visible_tile_keys, camera.zoom_level)
        .await;
    assert!(
        vector_base_frame.drawn_tile_count > 0,
        "the committed vector tile must draw"
    );
    assert!(
        vector_base_frame.sprite_atlas.is_some(),
        "the injected source must resolve the style's sprite URL"
    );
    let label_layer =
        vector_base_frame.label_layer(&camera, &viewport, DEFAULT_FIELD_OF_VIEW_DEGREES);
    let label_layer = label_layer.expect("the icon layer must place candidates");
    assert!(
        label_layer
            .labels()
            .iter()
            .any(|label| label.icon.is_some()),
        "at least one bus stop must place its sprite icon"
    );
    assert!(
        !label_layer.icon_quad_vertices(&viewport).is_empty(),
        "the placed icons must assemble sprite quads"
    );
    if let Some(background) = vector_base_frame.background {
        renderer.set_clear_color_rgba(background);
    }
    let (mut pool, prepared_tiles) = fill_raster_pool_and_prepare(&camera);
    let captured_frame = renderer
        .render_captured_frame_layers_and_labels(
            &camera,
            &viewport,
            &prepared_tiles,
            &mut pool,
            &vector_base_frame.overlays,
            None,
            Some(&label_layer),
        )
        .expect("the icon frame renders");
    compare_or_create_golden(
        &captured_frame,
        ICON_GOLDEN_FILE,
        "the icon sprite scene",
        GoldenTolerance::STANDARD,
    );
}
