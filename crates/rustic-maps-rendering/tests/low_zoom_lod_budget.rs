//! The low-zoom level-of-detail budget.
//!
//! One committed performance tile in the Carto light style is styled at
//! the cold-fit zoom, tessellated with and without the screen-space
//! simplification, and counted against the frame budget the z7 fit gate
//! enforces: 400 000 overlay triangles over the fifteen visible tiles. The
//! performance fixture repeats one tile's geometry, so `reduced_triangles * 15`
//! is the frame's triangle count.
//!
//! This is a geometry gate, not a timing gate: it runs on every renderer.

use geospatial_types::GeographicCoordinate;
use rustic_maps_rendering::overlay::simplify_overlay_layers_for_style_zoom;
use rustic_maps_rendering::vector_style::{
    VECTOR_STYLE_ID_CARTO_LIGHT, VectorTileAddress, vector_tile_overlay_layers_at_zoom,
    vector_tile_style_from_id,
};
use rustic_maps_rendering::{
    DEFAULT_FIELD_OF_VIEW_DEGREES, MapCamera, MapOverlay, ViewportPixels,
    normalized_world_to_coordinate, overlay_triangle_vertices,
};
use rustic_maps_vector_tiles::decode_vector_tile;

/// The committed performance tile the performance fixture serves.
const FIXTURE_TILE_BYTES: &[u8] =
    include_bytes!("../../../fixtures/openstreetmap-us/osmus_tile_z9_x83_y196.mvt");

/// The tile's address in the z9 fixture set.
const FIXTURE_TILE_ADDRESS: VectorTileAddress = VectorTileAddress {
    zoom_level: 9,
    tile_x: 83,
    tile_y: 196,
};

/// The cold-fit zoom the simplification evaluates at.
const FIT_STYLE_ZOOM_LEVEL: f64 = 7.0;

/// The visible tile count the z7 fit scenario requests.
const FIT_VISIBLE_TILE_COUNT: usize = 15;

/// The frame's overlay triangle budget.
const FIT_OVERLAY_TRIANGLE_BUDGET: usize = 400_000;

/// The measured canvas the fit scenario renders into.
const FIT_VIEWPORT_WIDTH_PIXELS: f64 = 1920.0;
const FIT_VIEWPORT_HEIGHT_PIXELS: f64 = 1080.0;

/// The camera at the tile's center, top-down, at the fixture's zoom.
fn fixture_camera() -> MapCamera {
    let tile_count = 1u32 << FIXTURE_TILE_ADDRESS.zoom_level;
    let center = normalized_world_to_coordinate([
        (f64::from(FIXTURE_TILE_ADDRESS.tile_x) + 0.5) / f64::from(tile_count),
        (f64::from(FIXTURE_TILE_ADDRESS.tile_y) + 0.5) / f64::from(tile_count),
    ]);
    MapCamera::new(center, FIT_STYLE_ZOOM_LEVEL, 0.0, 0.0)
}

/// The triangle count the frame's overlay assembly emits for the layers.
fn overlay_triangle_count(
    camera: &MapCamera,
    viewport: &ViewportPixels,
    layer_overlays: &[Vec<MapOverlay>],
) -> usize {
    let overlays = layer_overlays.iter().flatten().cloned().collect::<Vec<_>>();
    overlay_triangle_vertices(camera, viewport, DEFAULT_FIELD_OF_VIEW_DEGREES, &overlays).len() / 3
}

#[test]
fn low_zoom_simplification_keeps_the_z7_frame_inside_its_triangle_budget() {
    let tile = decode_vector_tile(FIXTURE_TILE_BYTES).expect("the committed tile decodes");
    let style = vector_tile_style_from_id(VECTOR_STYLE_ID_CARTO_LIGHT)
        .expect("the committed Carto light style loads");
    let camera = fixture_camera();
    let viewport =
        ViewportPixels::new(FIT_VIEWPORT_WIDTH_PIXELS, FIT_VIEWPORT_HEIGHT_PIXELS).unwrap();

    let exact_layers = vector_tile_overlay_layers_at_zoom(
        &tile,
        FIXTURE_TILE_ADDRESS,
        &style,
        FIT_STYLE_ZOOM_LEVEL,
    );
    let exact_triangles = overlay_triangle_count(&camera, &viewport, &exact_layers);

    let mut reduced_layers = exact_layers;
    simplify_overlay_layers_for_style_zoom(&mut reduced_layers, FIT_STYLE_ZOOM_LEVEL);
    let reduced_triangles = overlay_triangle_count(&camera, &viewport, &reduced_layers);

    println!(
        "z7 Carto light tile: {exact_triangles} exact triangles, \
         {reduced_triangles} simplified; {FIT_VISIBLE_TILE_COUNT} tiles = {}",
        reduced_triangles * FIT_VISIBLE_TILE_COUNT
    );
    assert!(
        reduced_triangles < exact_triangles,
        "low-zoom simplification must reduce the tile's triangle count"
    );
    assert!(
        reduced_triangles * FIT_VISIBLE_TILE_COUNT <= FIT_OVERLAY_TRIANGLE_BUDGET,
        "the z7 fit frame draws {} triangles over {FIT_VISIBLE_TILE_COUNT} tiles, \
         over the {FIT_OVERLAY_TRIANGLE_BUDGET} budget",
        reduced_triangles * FIT_VISIBLE_TILE_COUNT
    );
}

/// The tile's coordinates stay inside the geographic span its address names:
/// a guard that the style conversion still produces usable geometry after the
/// simplification pass.
#[test]
fn the_simplified_tile_stays_inside_its_geographic_span() {
    let tile = decode_vector_tile(FIXTURE_TILE_BYTES).expect("the committed tile decodes");
    let style = vector_tile_style_from_id(VECTOR_STYLE_ID_CARTO_LIGHT)
        .expect("the committed Carto light style loads");
    let mut layers = vector_tile_overlay_layers_at_zoom(
        &tile,
        FIXTURE_TILE_ADDRESS,
        &style,
        FIT_STYLE_ZOOM_LEVEL,
    );
    simplify_overlay_layers_for_style_zoom(&mut layers, FIT_STYLE_ZOOM_LEVEL);
    for overlay in layers.iter().flatten() {
        let coordinates: &[GeographicCoordinate] = match overlay {
            MapOverlay::RoutePolyline(route_polyline) => &route_polyline.coordinates,
            MapOverlay::FilledPolygon(filled_polygon) => &filled_polygon.ring_coordinates,
            _ => continue,
        };
        assert!(
            coordinates
                .iter()
                .all(|coordinate| coordinate.latitude.is_finite()
                    && coordinate.longitude.is_finite()),
            "every simplified coordinate remains finite"
        );
    }
}
