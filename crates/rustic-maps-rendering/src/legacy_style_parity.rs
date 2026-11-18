//! Migration parity for the compiled style.
//!
//! This test is the proof that the compiler and the converted documents paint
//! what the pre-compiler parser painted. For every committed style it compares:
//!
//! - the layer set, order and zoom spans;
//! - the background colour and every resolved paint value the old model kept,
//!   at five camera zoom levels;
//! - every overlay primitive the committed vector-tile fixtures produce at
//!   those zoom levels, structurally and exactly;
//! - every label candidate the fixtures produce, field by field.
//!
//! The legacy parser and label walk live in a test-only module; no non-test
//! build can import them.

use crate::labels::{MapLabelCandidate, label_candidates_for_vector_tile};
use crate::legacy_vector_style::{
    VectorStyleGeometryKind, VectorSymbolPlacement, VectorTileStyle,
    legacy_label_candidates_for_vector_tile,
    overlay_primitives_for_vector_tile_at_zoom as legacy_overlays_at_zoom,
    vector_tile_style_from_id as legacy_style_from_id,
};
use crate::vector_style::{
    VECTOR_STYLE_ID_BRIGHT_LIGHT, VECTOR_STYLE_ID_CARTO_DARK, VECTOR_STYLE_ID_CARTO_LIGHT,
    VECTOR_STYLE_ID_FIORD_DARK, VECTOR_STYLE_ID_OPENMAPTILES_DARK,
    VECTOR_STYLE_ID_OPENMAPTILES_LIGHT, VECTOR_STYLE_ID_POSITRON_LIGHT, VectorTileAddress,
    overlay_primitives_for_vector_tile_at_zoom, vector_tile_style_from_id,
};
use rustic_maps_style::{
    CompiledLayerKind, CompiledStyle, EvaluationContext, ResolvedPaint, SymbolPlacement,
};
use rustic_maps_vector_tiles::{VectorTile, decode_vector_tile};

/// Every committed style id the renderer offers.
const COMMITTED_STYLE_IDS: [&str; 7] = [
    VECTOR_STYLE_ID_CARTO_LIGHT,
    VECTOR_STYLE_ID_CARTO_DARK,
    VECTOR_STYLE_ID_OPENMAPTILES_LIGHT,
    VECTOR_STYLE_ID_OPENMAPTILES_DARK,
    VECTOR_STYLE_ID_BRIGHT_LIGHT,
    VECTOR_STYLE_ID_FIORD_DARK,
    VECTOR_STYLE_ID_POSITRON_LIGHT,
];

/// The five camera zoom levels the migration is verified at.
const PARITY_ZOOM_LEVELS: [f64; 5] = [9.0, 12.0, 14.0, 16.0, 18.0];

/// One committed vector-tile fixture.
struct FixtureTile {
    /// A name for failure messages.
    name: &'static str,
    /// The decoded tile.
    tile: VectorTile,
    /// The address the tile belongs to.
    address: VectorTileAddress,
}

/// The same address in the legacy parser's own type.
fn legacy_address(address: VectorTileAddress) -> crate::legacy_vector_style::VectorTileAddress {
    crate::legacy_vector_style::VectorTileAddress {
        zoom_level: address.zoom_level,
        tile_x: address.tile_x,
        tile_y: address.tile_y,
    }
}

/// Decodes the three committed MVT fixtures.
fn fixture_tiles() -> Vec<FixtureTile> {
    vec![
        FixtureTile {
            name: "sourdough z6",
            tile: decode_vector_tile(include_bytes!(
                "../../../fixtures/openstreetmap-us/sourdough_tile_z6_x18_y27.mvt"
            ))
            .expect("the sourdough fixture decodes"),
            address: VectorTileAddress {
                zoom_level: 6,
                tile_x: 18,
                tile_y: 27,
            },
        },
        FixtureTile {
            name: "openstreetmap-us z9",
            tile: decode_vector_tile(include_bytes!(
                "../../../fixtures/openstreetmap-us/osmus_tile_z9_x83_y196.mvt"
            ))
            .expect("the regional fixture decodes"),
            address: VectorTileAddress {
                zoom_level: 9,
                tile_x: 83,
                tile_y: 196,
            },
        },
        FixtureTile {
            name: "openmaptiles z14",
            tile: decode_vector_tile(include_bytes!(
                "../../../fixtures/openfreemap/openmaptiles_tile_z14_x2661_y6286.pbf"
            ))
            .expect("the OpenMapTiles fixture decodes"),
            address: VectorTileAddress {
                zoom_level: 14,
                tile_x: 2661,
                tile_y: 6286,
            },
        },
    ]
}

#[test]
fn every_committed_style_matches_the_legacy_parser() {
    let fixtures = fixture_tiles();
    let mut compared_overlay_count = 0;
    let mut compared_label_count = 0;
    for style_id in COMMITTED_STYLE_IDS {
        let legacy: VectorTileStyle = legacy_style_from_id(style_id)
            .unwrap_or_else(|style_error| panic!("{style_id}: legacy parse failed: {style_error}"));
        let compiled: CompiledStyle = vector_tile_style_from_id(style_id)
            .unwrap_or_else(|style_error| panic!("{style_id}: compile failed: {style_error}"));
        assert!(
            compiled.unsupported.is_empty(),
            "{style_id}: the converted document asks for properties the renderer cannot apply: {:?}",
            compiled.unsupported
        );
        assert_eq!(
            legacy.layers.len(),
            compiled.layers.len(),
            "{style_id}: the compiled layer set changed size"
        );
        for (layer_index, (legacy_layer, compiled_layer)) in
            legacy.layers.iter().zip(&compiled.layers).enumerate()
        {
            assert_eq!(
                compiled_layer.order_index, layer_index,
                "{style_id}: layer {layer_index} lost its stable order key"
            );
            assert_eq!(
                legacy_layer.source_layer,
                compiled_layer.source_layer.clone().unwrap_or_default(),
                "{style_id}: layer {layer_index} reads a different source layer"
            );
            assert_eq!(
                legacy_layer.min_zoom_level, compiled_layer.minimum_zoom,
                "{style_id}: layer {layer_index} minimum zoom"
            );
            assert_eq!(
                legacy_layer.max_zoom_level, compiled_layer.maximum_zoom,
                "{style_id}: layer {layer_index} maximum zoom"
            );
            for zoom_level in PARITY_ZOOM_LEVELS {
                compare_layer_paint(
                    style_id,
                    layer_index,
                    legacy_layer,
                    compiled_layer,
                    zoom_level,
                );
            }
        }
        for zoom_level in PARITY_ZOOM_LEVELS {
            assert_eq!(
                legacy.background,
                compiled.background_at(zoom_level),
                "{style_id}: background at zoom {zoom_level}"
            );
        }
        for fixture in &fixtures {
            for zoom_level in PARITY_ZOOM_LEVELS {
                compared_overlay_count +=
                    compare_overlays(style_id, fixture, &legacy, &compiled, zoom_level);
                compared_label_count +=
                    compare_labels(style_id, fixture, &legacy, &compiled, zoom_level);
            }
        }
    }
    assert!(
        compared_overlay_count > 0,
        "the parity fixtures painted no overlays, so the comparison proves nothing"
    );
    assert!(
        compared_label_count > 0,
        "the parity fixtures produced no labels, so the comparison proves nothing"
    );
}

/// Compares one layer's resolved paint at one zoom level.
fn compare_layer_paint(
    style_id: &str,
    layer_index: usize,
    legacy_layer: &crate::legacy_vector_style::VectorLayerStyle,
    compiled_layer: &rustic_maps_style::CompiledLayer,
    zoom_level: f64,
) {
    let context = EvaluationContext::at_zoom(zoom_level);
    let resolved_paint = compiled_layer.resolved_paint(&context);
    match legacy_layer.geometry_kind {
        VectorStyleGeometryKind::Fill => {
            assert_eq!(
                compiled_layer.kind,
                CompiledLayerKind::Fill,
                "{style_id}: layer {layer_index} kind"
            );
            let ResolvedPaint::Fill(paint) = resolved_paint else {
                panic!("{style_id}: layer {layer_index} resolved no fill paint");
            };
            assert_eq!(
                legacy_layer.fill_rgba,
                [
                    paint.color.red as f32,
                    paint.color.green as f32,
                    paint.color.blue as f32,
                    (paint.color.alpha * paint.opacity) as f32,
                ],
                "{style_id}: layer {layer_index} fill at zoom {zoom_level}"
            );
        }
        VectorStyleGeometryKind::Line => {
            assert_eq!(
                compiled_layer.kind,
                CompiledLayerKind::Line,
                "{style_id}: layer {layer_index} kind"
            );
            let ResolvedPaint::Line(paint) = resolved_paint else {
                panic!("{style_id}: layer {layer_index} resolved no line paint");
            };
            assert_eq!(
                legacy_layer.line_rgba,
                [
                    paint.color.red as f32,
                    paint.color.green as f32,
                    paint.color.blue as f32,
                    (paint.color.alpha * paint.opacity) as f32,
                ],
                "{style_id}: layer {layer_index} line colour at zoom {zoom_level}"
            );
            assert_eq!(
                legacy_layer.width_pixels_at_zoom(zoom_level),
                paint.width,
                "{style_id}: layer {layer_index} line width at zoom {zoom_level}"
            );
        }
        VectorStyleGeometryKind::Circle => {
            assert_eq!(
                compiled_layer.kind,
                CompiledLayerKind::Circle,
                "{style_id}: layer {layer_index} kind"
            );
            let ResolvedPaint::Circle(paint) = resolved_paint else {
                panic!("{style_id}: layer {layer_index} resolved no circle paint");
            };
            assert_eq!(
                legacy_layer.circle_rgba,
                [
                    paint.color.red as f32,
                    paint.color.green as f32,
                    paint.color.blue as f32,
                    (paint.color.alpha * paint.opacity) as f32,
                ],
                "{style_id}: layer {layer_index} circle colour at zoom {zoom_level}"
            );
            assert_eq!(
                legacy_layer.circle_radius_pixels, paint.radius,
                "{style_id}: layer {layer_index} circle radius at zoom {zoom_level}"
            );
        }
        VectorStyleGeometryKind::Symbol => {
            assert_eq!(
                compiled_layer.kind,
                CompiledLayerKind::Symbol,
                "{style_id}: layer {layer_index} kind"
            );
            let ResolvedPaint::Symbol(paint) = resolved_paint else {
                panic!("{style_id}: layer {layer_index} resolved no symbol paint");
            };
            assert_eq!(
                legacy_layer.text_rgba,
                [
                    paint.text_color.red as f32,
                    paint.text_color.green as f32,
                    paint.text_color.blue as f32,
                    (paint.text_color.alpha * paint.text_opacity) as f32,
                ],
                "{style_id}: layer {layer_index} text colour at zoom {zoom_level}"
            );
            assert_eq!(
                legacy_layer.halo_rgba,
                [
                    paint.halo_color.red as f32,
                    paint.halo_color.green as f32,
                    paint.halo_color.blue as f32,
                    paint.halo_color.alpha as f32,
                ],
                "{style_id}: layer {layer_index} halo colour at zoom {zoom_level}"
            );
            assert_eq!(
                legacy_layer.halo_width_pixels, paint.halo_width,
                "{style_id}: layer {layer_index} halo width at zoom {zoom_level}"
            );
            let layout = compiled_layer.resolved_symbol_layout(&context);
            assert_eq!(
                legacy_layer.text_size_pixels_at_zoom(zoom_level),
                layout.text_size,
                "{style_id}: layer {layer_index} text size at zoom {zoom_level}"
            );
            assert_eq!(
                legacy_layer.font_stack, layout.font_stack,
                "{style_id}: layer {layer_index} font stack"
            );
            assert_eq!(
                legacy_layer.allow_overlap, layout.allow_overlap,
                "{style_id}: layer {layer_index} overlap rule"
            );
            assert_eq!(
                legacy_layer.priority, layout.sort_key as u32,
                "{style_id}: layer {layer_index} placement key"
            );
            let expected_placement = match legacy_layer.placement {
                VectorSymbolPlacement::Point => SymbolPlacement::Point,
                VectorSymbolPlacement::Line => SymbolPlacement::Line,
            };
            assert_eq!(
                expected_placement, layout.placement,
                "{style_id}: layer {layer_index} placement"
            );
        }
    }
}

/// Compares every overlay primitive the fixture produces at one zoom.
fn compare_overlays(
    style_id: &str,
    fixture: &FixtureTile,
    legacy: &VectorTileStyle,
    compiled: &CompiledStyle,
    zoom_level: f64,
) -> usize {
    let legacy_batches = legacy_overlays_at_zoom(
        &fixture.tile,
        legacy_address(fixture.address),
        legacy,
        zoom_level,
    );
    let compiled_batches = overlay_primitives_for_vector_tile_at_zoom(
        &fixture.tile,
        fixture.address,
        compiled,
        zoom_level,
    );
    assert_eq!(
        legacy_batches.len(),
        compiled_batches.len(),
        "{style_id}: {}: batch count at zoom {zoom_level}",
        fixture.name
    );
    for (legacy_batch, compiled_batch) in legacy_batches.iter().zip(&compiled_batches) {
        let compiled_layer = compiled
            .layers
            .get(compiled_batch.layer_index)
            .unwrap_or_else(|| {
                panic!(
                    "{style_id}: {}: batch names layer index {} outside the style",
                    fixture.name, compiled_batch.layer_index
                )
            });
        assert_eq!(
            legacy_batch.layer_style.source_layer,
            compiled_layer.source_layer.clone().unwrap_or_default(),
            "{style_id}: {}: batch order at zoom {zoom_level}",
            fixture.name
        );
        assert_eq!(
            compiled_batch.layer_id, compiled_layer.id,
            "{style_id}: {}: batch id at zoom {zoom_level}",
            fixture.name
        );
        assert_eq!(
            legacy_batch.overlays, compiled_batch.overlays,
            "{style_id}: {}: overlay primitives at zoom {zoom_level}",
            fixture.name
        );
    }
    legacy_batches
        .iter()
        .map(|batch| batch.overlays.len())
        .sum()
}

/// Compares every label candidate the fixture produces at one zoom.
fn compare_labels(
    style_id: &str,
    fixture: &FixtureTile,
    legacy: &VectorTileStyle,
    compiled: &CompiledStyle,
    zoom_level: f64,
) -> usize {
    let legacy_candidates = legacy_label_candidates_for_vector_tile(
        &fixture.tile,
        legacy_address(fixture.address),
        legacy,
        zoom_level,
    );
    let compiled_candidates =
        label_candidates_for_vector_tile(&fixture.tile, fixture.address, compiled, zoom_level);
    assert_eq!(
        legacy_candidates.len(),
        compiled_candidates.len(),
        "{style_id}: {}: label candidate count at zoom {zoom_level}",
        fixture.name
    );
    for (legacy_candidate, compiled_candidate) in legacy_candidates.iter().zip(&compiled_candidates)
    {
        assert_label_candidate_eq(
            style_id,
            fixture.name,
            zoom_level,
            legacy_candidate,
            compiled_candidate,
            compiled,
        );
    }
    legacy_candidates.len()
}

/// Compares one label candidate field by field.
///
/// The legacy label model predates route-reference badges: it collects the
/// same candidates and places the same text, but every legacy candidate
/// carries `shield: None`. The compiled style resolves the badge the layer
/// declares, so a compiled candidate carries `Some` exactly where its source
/// layer declares a shield block. That divergence is deliberate and asserted
/// here rather than skipped, so a badge silently appearing or disappearing
/// fails the parity run.
fn assert_label_candidate_eq(
    style_id: &str,
    fixture_name: &str,
    zoom_level: f64,
    legacy: &MapLabelCandidate,
    compiled: &MapLabelCandidate,
    compiled_style: &CompiledStyle,
) {
    let context = format!("{style_id}: {fixture_name}: label at zoom {zoom_level}");
    assert_eq!(legacy.text, compiled.text, "{context}: text");
    assert_eq!(legacy.anchor, compiled.anchor, "{context}: anchor");
    assert_eq!(
        legacy.line_coordinates, compiled.line_coordinates,
        "{context}: line coordinates"
    );
    assert_eq!(
        legacy.text_rgba, compiled.text_rgba,
        "{context}: text colour"
    );
    assert_eq!(
        legacy.text_size_pixels, compiled.text_size_pixels,
        "{context}: text size"
    );
    assert_eq!(
        legacy.halo_rgba, compiled.halo_rgba,
        "{context}: halo colour"
    );
    assert_eq!(
        legacy.halo_width_pixels, compiled.halo_width_pixels,
        "{context}: halo width"
    );
    assert_eq!(legacy.priority, compiled.priority, "{context}: priority");
    assert_eq!(legacy.rank, compiled.rank, "{context}: rank");
    assert_eq!(
        legacy.allow_overlap, compiled.allow_overlap,
        "{context}: overlap rule"
    );
    assert_eq!(
        legacy.style_order, compiled.style_order,
        "{context}: style order"
    );
    assert!(
        legacy.shield.is_none(),
        "{context}: the legacy model predates badges and never carries one"
    );
    if compiled.shield.is_some() {
        let source_layer = compiled_style
            .layers
            .get(compiled.style_order)
            .unwrap_or_else(|| {
                panic!(
                    "{context}: the badge names layer {} outside the style",
                    compiled.style_order
                )
            });
        assert!(
            source_layer.shield.is_some(),
            "{context}: a compiled badge comes only from a declared shield block"
        );
    }
}
