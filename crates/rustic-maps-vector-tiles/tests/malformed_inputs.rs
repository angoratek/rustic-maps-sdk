//! The decoder's total-decode property: every malformed, truncated or
//! mutated buffer produces a typed [`rustic_maps_vector_tiles::Error`]-style
//! result and never a panic, because this decoder runs on untrusted
//! network bytes.
//!
//! Three surfaces, all network-free:
//! 1. arbitrary byte strings (proptest);
//! 2. deterministic seeded mutations of the committed synthetic Sourdough
//!    tile (bounded set of byte flips, truncations, byte-count inflations);
//! 3. deterministic seeded mutations of a decoded reference tile built by
//!    the test-suite encoder (structure-targeted corruptions: tag
//!    indices, dictionary truncations, geometry command streams).

/// The shared test-suite encoder; this binary uses only part of it.
#[allow(dead_code)]
mod common;

use proptest::prelude::*;

use rustic_maps_vector_tiles::{
    VectorTile, VectorTileGeometry, VectorTileGeometryType, decode_vector_tile,
};

/// The committed gzip-decoded Sourdough tile, the mutation base.
const SOURDOUGH_TILE_BYTES: &[u8] =
    include_bytes!("../../../fixtures/openstreetmap-us/sourdough_tile_z6_x18_y27.mvt");

/// The reference tile the structural corruptions are built over: one
/// layer, one tagged polygon feature, one tagged point feature, one
/// dictionary key, one dictionary value.
fn reference_tile_bytes() -> Vec<u8> {
    let point_command_integers = [
        common::pack_command_integer(1, 1),
        common::zigzag_encode(25),
        common::zigzag_encode(17),
    ];
    common::encode_tile(&[common::encode_layer(
        "reference",
        Some(2),
        &[
            common::encode_feature(
                Some(1),
                Some(3),
                Some(&[0, 0]),
                Some(&[
                    common::pack_command_integer(1, 1),
                    common::zigzag_encode(3),
                    common::zigzag_encode(6),
                    common::pack_command_integer(2, 2),
                    common::zigzag_encode(5),
                    common::zigzag_encode(6),
                    common::zigzag_encode(12),
                    common::zigzag_encode(22),
                    common::pack_command_integer(1, 7),
                ]),
            ),
            common::encode_feature(None, Some(1), Some(&[0, 0]), Some(&point_command_integers)),
        ],
        &["name"],
        &[common::encode_text_value("Rust Harbor")],
        None,
    )])
}

/// Mutates the base bytes deterministically from a seed: byte flips at
/// seeded positions, seeded truncations and seeded byte repetitions.
fn mutate_buffer_deterministically(base_bytes: &[u8], seed: u64) -> Vec<u8> {
    let mut mutated = base_bytes.to_vec();
    // Three mixed rounds of seeded mutations per seed: flip a byte, then
    // optionally truncate, then optionally append garbage.
    let flip_position = (seed % 512) as usize;
    let second_position = ((seed * 31 + 7) % 512) as usize;
    let flip_value = ((seed >> 3) & 0xff) as u8;
    if flip_position < mutated.len() {
        mutated[flip_position] ^= if flip_value == 0 { 0xff } else { flip_value };
    }
    match (seed % 3, seed % 5) {
        // Truncate at a seeded position.
        (0, _) if !mutated.is_empty() => {
            let truncate_at = second_position.min(mutated.len()).max(1);
            mutated.truncate(truncate_at);
        }
        // Repeat a prefix twice (overlong claims elsewhere in the buffer).
        (1, _) => {
            let split_at = (second_position % mutated.len().max(1)).max(1);
            let prefix = mutated[..split_at].to_vec();
            mutated.extend_from_slice(&prefix);
        }
        // Append garbage bytes.
        _ => {
            mutated.extend_from_slice(&[flip_value, flip_value.wrapping_add(1)]);
        }
    }
    mutated
}

#[test]
fn bounded_seeded_mutations_of_the_sourdough_fixture_never_panic() {
    // A bounded, deterministic sweep: 1024 seeds over the committed
    // synthetic tile — each must end in Ok or Err, never a panic.
    for seed in 0..1024u64 {
        let mutated = mutate_buffer_deterministically(SOURDOUGH_TILE_BYTES, seed);
        if mutated.is_empty() {
            continue;
        }
        // The result is only observed so a panic aborts the test.
        let _ = decode_vector_tile(&mutated);
    }
}

#[test]
fn every_prefix_of_the_sourdough_fixture_decodes_without_panicking() {
    // All prefixes, including zero-byte ones.
    for length in 0..=SOURDOUGH_TILE_BYTES.len() {
        let _ = decode_vector_tile(&SOURDOUGH_TILE_BYTES[..length]);
    }
}

#[test]
fn bounded_seeded_mutations_of_the_reference_tile_never_panic() {
    let reference_tile = reference_tile_bytes();
    for seed in 0..1024u64 {
        let mutated = mutate_buffer_deterministically(&reference_tile, seed);
        if mutated.is_empty() {
            continue;
        }
        let _ = decode_vector_tile(&mutated);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// The only contract: decode is total — Ok or Err, never a panic.
    #[test]
    fn arbitrary_buffers_decode_or_error_never_panic(
        buffer in prop::collection::vec(any::<u8>(), 0..256),
    ) {
        let _ = decode_vector_tile(&buffer);
    }

    #[test]
    fn arbitrary_truncations_of_the_real_fixture_decode_never_panicking(
        drop_tail_count in 0u64..512,
    ) {
        let truncate_at = SOURDOUGH_TILE_BYTES
            .len()
            .saturating_sub(drop_tail_count as usize);
        let _ = decode_vector_tile(&SOURDOUGH_TILE_BYTES[..truncate_at]);
    }

    #[test]
    fn structured_tiles_with_arbitrary_geometry_and_tag_integers_never_panic(
        geometry_type in 0u64..4,
        geometry_integers in prop::collection::vec(0u64..1024, 0..32),
        tags in prop::collection::vec(0u64..64, 0..8),
    ) {
        let feature_bytes = common::encode_feature(Some(9), Some(geometry_type), Some(&tags), Some(&geometry_integers));
        let tile_bytes = common::encode_tile(&[common::encode_layer(
            "fuzzed",
            Some(2),
            &[feature_bytes],
            &["name"],
            &[common::encode_text_value("Rust Harbor")],
            None,
        )]);
        let _ = decode_vector_tile(&tile_bytes);
    }
}

#[test]
fn the_untouched_reference_tile_still_decodes_to_its_documented_shape() {
    // Guard that the corruption harness's base tile itself stays well
    // formed (mutators only ever see copies).
    let tile: VectorTile = decode_vector_tile(&reference_tile_bytes()).unwrap();
    assert_eq!(tile.layers.len(), 1);
    assert_eq!(tile.layers[0].features.len(), 2);
    match &tile.layers[0].features[0].geometry {
        VectorTileGeometry::Polygons(rings) => {
            assert_eq!(rings.len(), 1);
            assert_eq!(rings[0].vertices.len(), 3);
        }
        other => panic!("expected polygon geometry, got {other:?}"),
    }
    assert_eq!(
        tile.layers[0].features[0].geometry_type,
        VectorTileGeometryType::Polygon
    );
    assert!(tile.layers[0].features[1].geometry_type == VectorTileGeometryType::Point);
}
