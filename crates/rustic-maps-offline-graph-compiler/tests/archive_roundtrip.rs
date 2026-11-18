//! Milestone M7 slice 1 verify gates for `rustic-maps-offline-graph-compiler`:
//! fixture generation (byte-equality guarded), the compiler round trip over
//! the committed fixture (node coordinates, edge topology, oneway direction,
//! speeds/classes, turn restriction, instruction strings), compiler output
//! determinism, and the typed-corruption contract of the archive reader.
//!
//! The committed fixture (`fixtures/osm/routing_grid.osm.pbf`, hand-sized)
//! is described in `tests/common/mod.rs`. Regenerate it with
//! `MAP_OFFLINE_GRAPH_COMPILER_REGENERATE_FIXTURE=1 cargo test -p rustic-maps-offline-graph-compiler`.

use geospatial_types::{OfflineGraphInvalidReason, SDKError};
use rustic_maps_offline_graph_compiler::archive::{
    EdgeRecord, OfflineRoutingNetworkGraph, RestrictionKind, RoadClass,
};
use rustic_maps_offline_graph_compiler::compiler::compile_osm_extract_bytes;

use common::{
    OsmPbfFixture, REGENERATE_ARCHIVE_FIXTURE_ENVIRONMENT_VARIABLE,
    REGENERATE_FIXTURE_ENVIRONMENT_VARIABLE, archive_fixture_bytes, archive_fixture_path,
    canonical_routing_grid_fixture, fixture_bytes, fixture_path,
};

mod common;

/// Rounding tolerance between the fixture's degree coordinates and the
/// archive's quantized coordinates.
const COORDINATE_ASSERTION_TOLERANCE_DEGREES: f64 = 1e-7;

/// Hand-computed distance of one axis-aligned grid segment (0.001 degrees
/// at the equator on the mean-Earth haversine).
const GRID_SEGMENT_DISTANCE_METERS: u32 = 111;

/// `GRID_SEGMENT_DISTANCE_METERS` at the quantized 30 km/h residential
/// speed, in milliseconds.
const RESIDENTIAL_SEGMENT_DURATION_MILLISECONDS: u32 = 13_320;

/// `GRID_SEGMENT_DISTANCE_METERS` at the quantized 20 km/h service speed
/// (the `maxspeed=20` override), in milliseconds.
const SERVICE_SEGMENT_DURATION_MILLISECONDS: u32 = 19_980;

/// Byte offset of the packed encoded value inside one edge record row.
const ENCODED_VALUE_ROW_OFFSET: usize = 16;

/// Byte offset of the target node index inside one edge record row.
const TARGET_NODE_ROW_OFFSET: usize = 4;

/// Guards the always-on regeneration seam: the committed fixture must equal
/// the canonical in-repo generator output, byte for byte.
#[test]
fn committed_osm_fixture_matches_canonical_generation() {
    let canonical_bytes = canonical_routing_grid_fixture();
    if std::env::var_os(REGENERATE_FIXTURE_ENVIRONMENT_VARIABLE).is_some() {
        std::fs::create_dir_all(
            fixture_path()
                .parent()
                .expect("fixture path always has a parent"),
        )
        .unwrap();
        std::fs::write(fixture_path(), canonical_bytes).unwrap();
        return;
    }
    assert_eq!(
        fixture_bytes(),
        canonical_bytes,
        "committed fixture drifted from the canonical generator; regenerate with \
         {REGENERATE_FIXTURE_ENVIRONMENT_VARIABLE}=1 cargo test -p rustic-maps-offline-graph-compiler"
    );
}

/// Guards the browser harness's committed compiled archive fixture (the
/// file `platform/web/serve.mjs` serves same-origin and the wasm target
/// loads from in-memory bytes): it must equal the canonical compiler output
/// byte for byte — mirroring the fixture extract's guard above — and read
/// back validated, which is what the wasm loader does with it.
#[test]
fn committed_compiled_archive_fixture_matches_canonical_compilation() {
    let canonical_archive_bytes = compile_osm_extract_bytes(&fixture_bytes()).unwrap();
    if std::env::var_os(REGENERATE_ARCHIVE_FIXTURE_ENVIRONMENT_VARIABLE).is_some() {
        std::fs::create_dir_all(
            archive_fixture_path()
                .parent()
                .expect("fixture path always has a parent"),
        )
        .unwrap();
        std::fs::write(archive_fixture_path(), &canonical_archive_bytes).unwrap();
        return;
    }
    assert_eq!(
        archive_fixture_bytes(),
        canonical_archive_bytes,
        "committed compiled archive fixture drifted from the canonical compiler output; \
         regenerate with {REGENERATE_ARCHIVE_FIXTURE_ENVIRONMENT_VARIABLE}=1 \
         cargo test -p rustic-maps-offline-graph-compiler"
    );
    OfflineRoutingNetworkGraph::from_bytes(&canonical_archive_bytes)
        .expect("the committed archive fixture validates");
}

/// The milestone verify gate: compile the fixture extract and assert every
/// archive field the round trip must preserve.
#[test]
fn compile_fixture_and_round_trip_all_archive_fields() {
    let archive_bytes = compile_osm_extract_bytes(&fixture_bytes()).unwrap();
    let graph = OfflineRoutingNetworkGraph::from_bytes(&archive_bytes).unwrap();

    // Nodes: ascending OSM id order. Only nodes referenced by drivable ways
    // survive: the unreferenced node 1003 and the private way's node 1007
    // are dropped, so the fixture's 8 nodes become 6 archive nodes.
    let expected_nodes: [(i64, f64, f64); 6] = [
        (1001, 0.000, 0.000),
        (1002, 0.000, 0.001),
        (1004, 0.001, 0.000),
        (1005, 0.001, 0.001),
        (1006, 0.001, 0.002),
        (1008, 0.002, 0.002),
    ];
    assert_eq!(graph.nodes().len(), expected_nodes.len());
    for ((osm_node_id, latitude_degrees, longitude_degrees), node_record) in
        expected_nodes.iter().zip(graph.nodes())
    {
        assert!(
            (node_record.latitude_degrees() - latitude_degrees).abs()
                < COORDINATE_ASSERTION_TOLERANCE_DEGREES,
            "node {osm_node_id} latitude drifted"
        );
        assert!(
            (node_record.longitude_degrees() - longitude_degrees).abs()
                < COORDINATE_ASSERTION_TOLERANCE_DEGREES,
            "node {osm_node_id} longitude drifted"
        );
    }

    // Edges in canonical order (see the compiler's determinism contract):
    // way 2001 both directions, way 2002 oneway forward, way 2004 service
    // both directions.
    let expected_edge_endpoints: [(u32, u32); 8] = [
        (0, 1),
        (1, 0),
        (1, 3),
        (2, 3),
        (3, 1),
        (3, 4),
        (4, 5),
        (5, 4),
    ];
    assert_eq!(graph.edges().len(), expected_edge_endpoints.len());
    for ((source_node_index, target_node_index), edge_record) in
        expected_edge_endpoints.iter().zip(graph.edges())
    {
        assert_eq!(edge_record.source_node_index(), *source_node_index);
        assert_eq!(edge_record.target_node_index(), *target_node_index);
        assert_eq!(edge_record.distance_meters(), GRID_SEGMENT_DISTANCE_METERS);
    }

    // Oneway direction: the oneway street only exists along its node order,
    // and two-way streets carry both directed records.
    let oneway_forward_edge = edge_by_node_pair(&graph, 2, 3).unwrap();
    assert!(oneway_forward_edge.forward_allowed());
    assert!(!oneway_forward_edge.backward_allowed());
    assert_eq!(edge_by_node_pair(&graph, 3, 2), None);
    assert_eq!(edge_by_node_pair(&graph, 4, 3), None);
    let two_way_forward_edge = edge_by_node_pair(&graph, 0, 1).unwrap();
    assert!(two_way_forward_edge.forward_allowed());
    assert!(!two_way_forward_edge.backward_allowed());
    let two_way_backward_edge = edge_by_node_pair(&graph, 1, 0).unwrap();
    assert!(!two_way_backward_edge.forward_allowed());
    assert!(two_way_backward_edge.backward_allowed());

    // Road classes, speeds and durations (the packed encoded-value columns
    // plus the distance/duration columns). Edges 0..=5 are the residential
    // ways; edges 6..=7 the `maxspeed=20` service way.
    for edge_record in &graph.edges()[..6] {
        assert_eq!(edge_record.road_class(), Some(RoadClass::Residential));
        assert_eq!(edge_record.speed_kilometers_per_hour(), 30);
        assert_eq!(
            edge_record.duration_milliseconds(),
            RESIDENTIAL_SEGMENT_DURATION_MILLISECONDS
        );
    }
    for edge_record in &graph.edges()[6..] {
        assert_eq!(edge_record.road_class(), Some(RoadClass::Service));
        assert_eq!(edge_record.speed_kilometers_per_hour(), 20);
        assert_eq!(
            edge_record.duration_milliseconds(),
            SERVICE_SEGMENT_DURATION_MILLISECONDS
        );
    }

    // Turn restriction: from way 2001's edge ending at node 1005 (edge
    // index 2), across node 1005 (index 3), onto way 2002's edge leaving it
    // (edge index 5).
    assert_eq!(graph.turn_restrictions().len(), 1);
    let restriction = &graph.turn_restrictions()[0];
    assert_eq!(restriction.from_edge_index(), 2);
    assert_eq!(restriction.via_node_index(), 3);
    assert_eq!(restriction.to_edge_index(), 5);
    assert_eq!(
        restriction.restriction_kind(),
        Some(RestrictionKind::NoLeftTurn)
    );

    // Instruction strings + lookup indices: one phrase per restriction,
    // indexed by byte offset into the blob.
    assert_eq!(graph.instruction_lookup_indices(), &[0]);
    assert_eq!(graph.instruction_text(0), Some("No left turn"));

    // Lane guidance: resource present, no records in this slice.
    assert!(graph.lane_guidances().is_empty());
}

/// Two compiles of the same extract must agree byte for byte.
#[test]
fn compiler_output_is_deterministic_across_runs() {
    let first_archive = compile_osm_extract_bytes(&fixture_bytes()).unwrap();
    let second_archive = compile_osm_extract_bytes(&fixture_bytes()).unwrap();
    assert_eq!(first_archive, second_archive);
}

/// OSM `turn:lanes` carries *per-lane permitted maneuvers*
/// (`through|through|right`), while the counts-only `LaneGuidanceRecord`
/// cannot represent which maneuvers each lane permits. The compiler must
/// therefore refuse to fabricate lane guidance from lane tags: the way still
/// compiles to edges (the tag carries no routing weight), but the lane
/// guidance resource stays empty (see `LaneGuidanceRecord`'s documentation).
#[test]
fn turn_lane_tags_do_not_fabricate_lane_guidance_records() {
    let mut fixture = OsmPbfFixture::new();
    fixture.push_node(4001, 0.000, 0.000);
    fixture.push_node(4002, 0.000, 0.002);
    fixture.push_way(
        4003,
        vec![4001, 4002],
        vec![
            ("highway", "primary"),
            ("lanes", "3"),
            ("turn:lanes", "through|through|right"),
        ],
    );
    let archive_bytes = compile_osm_extract_bytes(&fixture.encode()).unwrap();
    let graph = OfflineRoutingNetworkGraph::from_bytes(&archive_bytes).unwrap();
    // Both node-order directions exist (no oneway tag), so the lane-tagged
    // way produces its two directed edges — proving the compile happened.
    assert_eq!(graph.edges().len(), 2);
    assert_eq!(graph.edges()[0].road_class(), Some(RoadClass::Primary));
    assert!(graph.lane_guidances().is_empty());
}

/// Corrupting the format version must fail typed and non-panicking.
#[test]
fn compiled_archive_fails_typed_when_version_is_corrupted() {
    let mut archive_bytes = compile_osm_extract_bytes(&fixture_bytes()).unwrap();
    archive_bytes[4] = 0x2A;
    assert_eq!(
        OfflineRoutingNetworkGraph::from_bytes(&archive_bytes).unwrap_err(),
        SDKError::OfflineRoutingFailure(OfflineGraphInvalidReason::VersionUnsupported)
    );
}

/// Truncating the archive must fail typed as malformed, never panic.
#[test]
fn compiled_archive_fails_typed_when_truncated() {
    let archive_bytes = compile_osm_extract_bytes(&fixture_bytes()).unwrap();
    let truncation_lengths = [0, 8, 100, archive_bytes.len() - 1];
    for &truncation_length in &truncation_lengths {
        assert_eq!(
            OfflineRoutingNetworkGraph::from_bytes(&archive_bytes[..truncation_length])
                .unwrap_err(),
            SDKError::OfflineRoutingFailure(OfflineGraphInvalidReason::ArchiveMalformed),
            "truncation to {truncation_length} bytes was not rejected"
        );
    }
}

/// Garbage in an edge's packed encoded value (reserved bits set) must fail
/// typed as malformed. The corruption flips reserved bit 10 (byte 1 of the
/// encoded value, bit 2 — little-endian layout).
#[test]
fn compiled_archive_fails_typed_when_edge_encoded_value_is_corrupted() {
    let mut archive_bytes = compile_osm_extract_bytes(&fixture_bytes()).unwrap();
    let (edges_offset, _edges_length) = edges_resource_span(&archive_bytes);
    let encoded_value_row = edges_offset + ENCODED_VALUE_ROW_OFFSET;
    archive_bytes[encoded_value_row + 1] |= 0b0000_0100;
    assert_eq!(
        OfflineRoutingNetworkGraph::from_bytes(&archive_bytes).unwrap_err(),
        SDKError::OfflineRoutingFailure(OfflineGraphInvalidReason::ArchiveMalformed)
    );
}

/// An out-of-bounds edge target node index must fail typed as out of
/// range.
#[test]
fn compiled_archive_fails_typed_when_edge_target_is_out_of_range() {
    let mut archive_bytes = compile_osm_extract_bytes(&fixture_bytes()).unwrap();
    let (edges_offset, edges_length) = edges_resource_span(&archive_bytes);
    let last_edge_row = edges_offset + edges_length
        - rustic_maps_offline_graph_compiler::archive::EDGE_RECORD_BYTES;
    let target_field_offset = last_edge_row + TARGET_NODE_ROW_OFFSET;
    archive_bytes[target_field_offset..target_field_offset + 4]
        .copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        OfflineRoutingNetworkGraph::from_bytes(&archive_bytes).unwrap_err(),
        SDKError::OfflineRoutingFailure(OfflineGraphInvalidReason::IndexOutOfRange)
    );
}

/// Reads the `edges` resource payload span out of the archive's
/// table-of-contents entry (offset field rides 32 name bytes into the
/// entry).
fn edges_resource_span(archive_bytes: &[u8]) -> (usize, usize) {
    const EDGES_RESOURCE_INDEX: usize = 1;
    let entry_offset = rustic_maps_offline_graph_compiler::archive::FILE_HEADER_BYTES
        + EDGES_RESOURCE_INDEX
            * rustic_maps_offline_graph_compiler::archive::TABLE_OF_CONTENTS_ENTRY_BYTES;
    let offset_field =
        entry_offset + rustic_maps_offline_graph_compiler::archive::RESOURCE_NAME_BYTES;
    let resource_offset = read_u64_at(archive_bytes, offset_field) as usize;
    let resource_length = read_u64_at(archive_bytes, offset_field + 8) as usize;
    (resource_offset, resource_length)
}

fn read_u64_at(archive_bytes: &[u8], byte_offset: usize) -> u64 {
    u64::from_le_bytes(
        archive_bytes[byte_offset..byte_offset + 8]
            .try_into()
            .unwrap(),
    )
}

/// Finds the unique edge connecting two node indices (as stored).
fn edge_by_node_pair(
    graph: &OfflineRoutingNetworkGraph,
    source_node_index: u32,
    target_node_index: u32,
) -> Option<&EdgeRecord> {
    graph.edges().iter().find(|edge_record| {
        edge_record.source_node_index() == source_node_index
            && edge_record.target_node_index() == target_node_index
    })
}
