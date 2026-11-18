//! The Mapbox Vector Tile Specification v2.1's worked geometry examples,
//! decoded and verified against the specification's documented decodings.
//!
//! # Provenance (honest route)
//!
//! The specification's repository (github.com/mapbox/vector-tile-spec,
//! version 2.1) publishes its examples as *documented command-integer
//! sequences in §4.3/Appendix A*, not as byte-exact binary `.mvt`
//! fixtures — the repository carries only `README.md` and
//! `vector_tile.proto`, so fetching example tile bytes is infeasible.
//! This suite therefore hand-encodes the **exact** integer sequences the
//! specification documents (embedded below as literals, quoted from the
//! spec text fetched on 2026-10-29) into minimal tiles with the
//! test-suite encoder in `common`, and asserts the decoder's output
//! equals the specification's documented decoding for each example. For
//! the single-segment examples the suite also re-encodes the decoded
//! coordinates with the same helper and reproduces the specification's
//! exact integer arrays; the multi-feature examples' later features are
//! cursor-relative to the previous feature's last `LineTo`, so they are
//! verified from the documented arrays' decode only. No production
//! re-encoder exists — this helper is tests-only.

mod common;

use rustic_maps_vector_tiles::{
    VectorTile, VectorTileAttributeValue, VectorTileGeometry, VectorTileGeometryType,
    decode_vector_tile,
};

/// §4.3's point example: `Encoded as: [ 9 50 34 ]`, decoded as
/// `MoveTo(+25, +17)` → point `(25, 17)`.
const POINT_EXAMPLE: [u64; 3] = [9, 50, 34];

/// §4.3's multipoint example: `Encoded as: [ 17 10 14 3 9 ]`, decoded as
/// `MoveTo(+5, +7)`, `MoveTo(-2, -5)` → points `(5, 7)` and `(3, 2)` (the
/// second delta is cursor-relative).
const MULTIPOINT_EXAMPLE: [u64; 5] = [17, 10, 14, 3, 9];

/// §4.3's linestring example: `Encoded as: [ 9 4 4 18 0 16 16 0 ]`,
/// decoded as `MoveTo(+2, +2)`, `LineTo(+0, +8)`, `LineTo(+8, +0)` →
/// `(2, 2)`, `(2, 10)`, `(10, 10)`.
const LINESTRING_EXAMPLE: [u64; 8] = [9, 4, 4, 18, 0, 16, 16, 0];

/// §4.3's multilinestring example: `Encoded as: [ 9 4 4 18 0 16 16 0 9 17
/// 17 10 4 8 ]` — the second linestring's `MoveTo(-9, -9)` is relative to
/// the *previous `LineTo`'s cursor*, per the specification's note.
const MULTILINESTRING_EXAMPLE: [u64; 14] = [9, 4, 4, 18, 0, 16, 16, 0, 9, 17, 17, 10, 4, 8];

/// §4.3's polygon example: `Encoded as: [ 9 6 12 18 10 12 24 44 15 ]`,
/// decoded as `MoveTo(3, 6)`, `LineTo(5, 6)`, `LineTo(12, 22)`,
/// `ClosePath` — the closing segment back to `(3, 6)` is implied.
const POLYGON_EXAMPLE: [u64; 9] = [9, 6, 12, 18, 10, 12, 24, 44, 15];

/// §4.3's multipolygon example: `Encoded as: [ 9 0 0 26 20 0 0 20 19 0 15
/// 9 22 2 26 18 0 0 18 17 0 15 9 4 13 26 0 8 8 0 0 7 15 ]` — polygon 1's
/// exterior, polygon 2's exterior (positive shoelace area = a new
/// polygon) and polygon 2's interior ring (negative area = a hole).
const MULTIPOLYGON_EXAMPLE: [u64; 33] = [
    9, 0, 0, 26, 20, 0, 0, 20, 19, 0, 15, 9, 22, 2, 26, 18, 0, 0, 18, 17, 0, 15, 9, 4, 13, 26, 0,
    8, 8, 0, 0, 7, 15,
];

/// Decodes a single-feature example tile and returns its sole geometry
/// with the declared kind.
fn decoded_example_geometry(
    geometry_type_wire_value: u64,
    geometry_command_integers: &[u64],
) -> (VectorTileGeometry, VectorTileGeometryType) {
    let tile_bytes = common::encode_example_tile(
        "example",
        geometry_type_wire_value,
        geometry_command_integers,
    );
    let tile = decode_vector_tile(&tile_bytes).unwrap();
    assert_eq!(tile.layers.len(), 1);
    let feature = &tile.layers[0].features[0];
    (feature.geometry.clone(), feature.geometry_type)
}

fn decoded_linestring_coordinates(
    geometry: &VectorTileGeometry,
    line_string_index: usize,
) -> Vec<(i64, i64)> {
    match geometry {
        VectorTileGeometry::LineStrings(line_strings) => line_strings[line_string_index]
            .iter()
            .map(|point| (point.x_coordinate, point.y_coordinate))
            .collect::<Vec<_>>(),
        other => panic!("expected linestring geometry, got {other:?}"),
    }
}

#[test]
fn the_spec_point_example_decodes_to_point_25_17() {
    let (geometry, geometry_type) = decoded_example_geometry(1, &POINT_EXAMPLE);
    assert_eq!(geometry_type, VectorTileGeometryType::Point);
    match geometry {
        VectorTileGeometry::Points(points) => {
            assert_eq!(points.len(), 1);
            assert_eq!(points[0].x_coordinate, 25);
            assert_eq!(points[0].y_coordinate, 17);
        }
        other => panic!("expected points geometry, got {other:?}"),
    }
}

#[test]
fn the_spec_multipoint_example_decodes_to_both_points() {
    let (geometry, _) = decoded_example_geometry(1, &MULTIPOINT_EXAMPLE);
    match geometry {
        VectorTileGeometry::Points(points) => {
            assert_eq!(points.len(), 2);
            assert_eq!(points[0].x_coordinate, 5);
            assert_eq!(points[0].y_coordinate, 7);
            // The second delta is cursor-relative: (5, 7) + (-2, -5).
            assert_eq!(points[1].x_coordinate, 3);
            assert_eq!(points[1].y_coordinate, 2);
        }
        other => panic!("expected points geometry, got {other:?}"),
    }
}

#[test]
fn the_spec_linestring_example_decodes_to_its_three_vertices() {
    let (geometry, geometry_type) = decoded_example_geometry(2, &LINESTRING_EXAMPLE);
    assert_eq!(geometry_type, VectorTileGeometryType::LineString);
    assert_eq!(
        decoded_linestring_coordinates(&geometry, 0),
        vec![(2, 2), (2, 10), (10, 10)]
    );
}

#[test]
fn the_spec_multilinestring_example_decodes_to_both_linestrings() {
    let (geometry, _) = decoded_example_geometry(2, &MULTILINESTRING_EXAMPLE);
    match geometry {
        VectorTileGeometry::LineStrings(line_strings) => {
            assert_eq!(line_strings.len(), 2);
            assert_eq!(
                line_strings
                    .iter()
                    .map(|line_string| line_string
                        .iter()
                        .map(|point| (point.x_coordinate, point.y_coordinate))
                        .collect::<Vec<_>>())
                    .collect::<Vec<_>>(),
                vec![vec![(2, 2), (2, 10), (10, 10)], vec![(1, 1), (3, 5)]]
            );
        }
        other => panic!("expected linestring geometry, got {other:?}"),
    }
}

#[test]
fn the_spec_polygon_example_decodes_to_its_closed_ring() {
    let (geometry, geometry_type) = decoded_example_geometry(3, &POLYGON_EXAMPLE);
    assert_eq!(geometry_type, VectorTileGeometryType::Polygon);
    match geometry {
        VectorTileGeometry::Polygons(rings) => {
            assert_eq!(rings.len(), 1);
            // The closing vertex is implied by the ClosePath command, not
            // duplicated into the vertex list.
            assert_eq!(
                rings[0]
                    .vertices
                    .iter()
                    .map(|point| (point.x_coordinate, point.y_coordinate))
                    .collect::<Vec<_>>(),
                vec![(3, 6), (8, 12), (20, 34)]
            );
            // The implied closing edge spells the ring back to (3, 6).
            assert_eq!(
                rings[0]
                    .closed_vertices()
                    .iter()
                    .map(|point| (point.x_coordinate, point.y_coordinate))
                    .collect::<Vec<_>>(),
                vec![(3, 6), (8, 12), (20, 34), (3, 6)]
            );
        }
        other => panic!("expected polygon geometry, got {other:?}"),
    }
}

#[test]
fn the_spec_multipolygon_example_decodes_to_three_rings() {
    let (geometry, _) = decoded_example_geometry(3, &MULTIPOLYGON_EXAMPLE);
    match geometry {
        VectorTileGeometry::Polygons(rings) => {
            assert_eq!(rings.len(), 3);
            // Polygon 1's exterior: (0,0) → (10,0) → (10,10) → (0,10),
            // with the spec's "Cursor at 0,10 after this command" note.
            assert_eq!(
                ring_coordinates(&rings[0]),
                vec![(0, 0), (10, 0), (10, 10), (0, 10)]
            );
            // Polygon 2's exterior — the spec's note: "THIS IS RELATIVE
            // TO LAST LINETO".
            assert_eq!(
                ring_coordinates(&rings[1]),
                vec![(11, 11), (20, 11), (20, 20), (11, 20)]
            );
            // Polygon 2's interior ring (the hole), whose negative area
            // is the specification's hole discriminator.
            assert_eq!(
                ring_coordinates(&rings[2]),
                vec![(13, 13), (13, 17), (17, 17), (17, 13)]
            );
        }
        other => panic!("expected polygon geometry, got {other:?}"),
    }
}

fn ring_coordinates(ring: &rustic_maps_vector_tiles::VectorTileLinearRing) -> Vec<(i64, i64)> {
    ring.vertices
        .iter()
        .map(|point| (point.x_coordinate, point.y_coordinate))
        .collect::<Vec<_>>()
}

#[test]
fn re_encoding_the_single_feature_examples_reproduces_the_spec_arrays() {
    // The helper's one-MoveTo and one-path encoders reproduce the
    // specification's exact packed arrays from the documented decoded
    // coordinates:
    assert_eq!(
        common::encode_move_to_command_stream(&[(25, 17)]),
        POINT_EXAMPLE.to_vec()
    );
    assert_eq!(
        common::encode_move_to_command_stream(&[(5, 7), (3, 2)]),
        MULTIPOINT_EXAMPLE.to_vec()
    );
    assert_eq!(
        common::encode_linear_path_command_stream(&[(2, 2), (2, 10), (10, 10)], false),
        LINESTRING_EXAMPLE.to_vec()
    );
    assert_eq!(
        common::encode_linear_path_command_stream(&[(3, 6), (8, 12), (20, 34)], true),
        POLYGON_EXAMPLE.to_vec()
    );
}

#[test]
fn the_command_integer_packing_formula_matches_the_spec_examples() {
    // CommandInteger = (id & 0x7) | (count << 3), reproduced against the
    // worked examples' first integers.
    assert_eq!(common::pack_command_integer(1, 1), 9);
    assert_eq!(common::pack_command_integer(2, 1), 17);
    assert_eq!(common::pack_command_integer(2, 2), 18);
    assert_eq!(common::pack_command_integer(1, 7), 15);
}

#[test]
fn feature_attributes_resolve_through_the_layer_dictionaries() {
    // Keys (layer field 3) and the seven Value variants (fields 1–7),
    // resolved by a feature's packed tag index pairs (field 2): key i
    // pairs with value i.
    let keys = [
        "name",
        "ratio",
        "visible",
        "population",
        "count",
        "sint",
        "level",
    ];
    let values = vec![
        common::encode_text_value("Rust Harbor"),
        common::encode_float_value(1.5),
        common::encode_double_value(2.25),
        common::encode_signed_value(-7),
        common::encode_unsigned_value(42),
        common::encode_zigzag_value(-100),
        common::encode_boolean_value(true),
    ];
    // Tag pairs: (key index, value index) covering all seven variants.
    let tags: [u64; 14] = [0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6];
    let feature_bytes = common::encode_feature(Some(3), Some(1), Some(&tags), Some(&POINT_EXAMPLE));
    let tile_bytes = common::encode_tile(&[common::encode_layer(
        "attributes",
        Some(2),
        &[feature_bytes],
        &keys,
        &values,
        None,
    )]);
    let tile = decode_vector_tile(&tile_bytes).unwrap();
    let feature = &tile.layers[0].features[0];
    assert_eq!(feature.identifier, Some(3));
    assert_eq!(feature.geometry_type, VectorTileGeometryType::Point);
    assert_eq!(feature.attributes.len(), 7);
    assert_eq!(
        feature.attributes[0],
        (
            "name".to_string(),
            VectorTileAttributeValue::Text("Rust Harbor".to_string())
        )
    );
    assert_eq!(
        feature.attributes[1],
        ("ratio".to_string(), VectorTileAttributeValue::Float(1.5))
    );
    assert_eq!(
        feature.attributes[2],
        (
            "visible".to_string(),
            VectorTileAttributeValue::Double(2.25)
        )
    );
    assert_eq!(
        feature.attributes[3],
        (
            "population".to_string(),
            VectorTileAttributeValue::Signed(-7)
        )
    );
    assert_eq!(
        feature.attributes[4],
        ("count".to_string(), VectorTileAttributeValue::Unsigned(42))
    );
    assert_eq!(
        feature.attributes[5],
        (
            "sint".to_string(),
            VectorTileAttributeValue::ZigZagSigned(-100)
        )
    );
    assert_eq!(
        feature.attributes[6],
        ("level".to_string(), VectorTileAttributeValue::Boolean(true))
    );
    assert_eq!(feature.attributes[1].1.numeric_value(), Some(1.5));
    assert_eq!(feature.attributes[0].1.numeric_value(), None);
}

#[test]
fn int_and_sint_variants_decode_the_minus_one_hundred_value_alike() {
    // `int` (field 4) reads the raw two's-complement varint; `sint`
    // (field 6) applies the ZigZag transform; both yield -100.
    let mut signed_value = Vec::new();
    common::append_varint_field(4, (-100i64) as u64, &mut signed_value);
    let mut sint_value = Vec::new();
    common::append_varint_field(6, common::zigzag_encode(-100), &mut sint_value);
    let feature_bytes = common::encode_feature(None, Some(1), Some(&[0, 0, 1, 1]), None);
    let tile_bytes = common::encode_tile(&[common::encode_layer(
        "variants",
        Some(2),
        &[feature_bytes],
        &["raw_int", "sint"],
        &[signed_value, sint_value],
        None,
    )]);
    let tile = decode_vector_tile(&tile_bytes).unwrap();
    assert_eq!(
        tile.layers[0].features[0].attributes[0],
        (
            "raw_int".to_string(),
            VectorTileAttributeValue::Signed(-100)
        )
    );
    assert_eq!(
        tile.layers[0].features[0].attributes[1],
        (
            "sint".to_string(),
            VectorTileAttributeValue::ZigZagSigned(-100)
        )
    );
}

#[test]
fn a_layer_without_extent_carries_the_default_extent() {
    let tile_bytes = common::encode_tile(&[common::encode_layer(
        "extentless",
        Some(2),
        &[],
        &[],
        &[],
        None,
    )]);
    let tile = decode_vector_tile(&tile_bytes).unwrap();
    assert_eq!(tile.layers[0].extent, 4096);
}

#[test]
fn a_layer_with_an_explicit_extent_preserves_it() {
    let tile_bytes = common::encode_tile(&[common::encode_layer(
        "explicit",
        Some(2),
        &[],
        &[],
        &[],
        Some(2048),
    )]);
    let tile = decode_vector_tile(&tile_bytes).unwrap();
    assert_eq!(tile.layers[0].extent, 2048);
}

#[test]
fn multi_layer_tiles_are_decoded_in_wire_order() {
    let tile_bytes = common::encode_tile(&[
        common::encode_layer("first", Some(2), &[], &[], &[], None),
        common::encode_layer("second", Some(2), &[], &[], &[], None),
    ]);
    let tile = decode_vector_tile(&tile_bytes).unwrap();
    assert_eq!(tile.layers.len(), 2);
    assert_eq!(tile.layers[0].name, "first");
    assert_eq!(tile.layers[1].name, "second");
}

#[test]
fn unknown_field_numbers_at_the_tile_level_are_skipped() {
    // Field 99 (a varint) around the tile's layer field: the schema
    // reserves extensions 16-8191 for forward-compatible producers.
    let mut tile_bytes = Vec::new();
    common::append_varint_field(99, 7, &mut tile_bytes);
    let layer_bytes = common::encode_layer("wrapped", Some(2), &[], &[], &[], None);
    common::append_length_delimited_field(3, &layer_bytes, &mut tile_bytes);
    common::append_length_delimited_field(100, &[0xde, 0xad], &mut tile_bytes);
    let tile = decode_vector_tile(&tile_bytes).unwrap();
    assert_eq!(tile.layers.len(), 1);
    assert_eq!(tile.layers[0].name, "wrapped");
}

#[test]
fn unknown_field_numbers_inside_a_feature_are_skipped_too() {
    let mut feature_bytes = common::encode_feature(Some(11), Some(1), None, Some(&POINT_EXAMPLE));
    common::append_varint_field(99, 3, &mut feature_bytes);
    let tile_bytes = common::encode_tile(&[common::encode_layer(
        "extended",
        Some(2),
        &[feature_bytes],
        &[],
        &[],
        None,
    )]);
    let tile: VectorTile = decode_vector_tile(&tile_bytes).unwrap();
    assert_eq!(tile.layers[0].features[0].identifier, Some(11));
    assert!(matches!(
        tile.layers[0].features[0].geometry,
        VectorTileGeometry::Points(_)
    ));
}
