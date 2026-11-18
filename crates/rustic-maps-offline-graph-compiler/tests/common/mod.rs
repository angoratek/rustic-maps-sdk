//! Hand-rolled minimal OpenStreetMap PBF writer for committed fixtures.
//!
//! Writes a valid PBF extract with **uncompressed** `Blob.raw` payloads
//! only, so the fixture needs no compression dependency at all. Field
//! framing follows `osmformat.proto`/`fileformat.proto` (the copies inside
//! the `osmpbf` crate, which is the fixture's only consumer).
//!
//! One deliberate divergence from the upstream OSM PBF spec: relation
//! `roles_sid` values are written as raw string-table indices — the spec
//! describes them as delta-coded, but `osmpbf`'s reader treats them raw
//! (`RelMemberIter` applies no delta), and this fixture must round-trip
//! through `osmpbf`. Deltas and raw values coincide whenever the roles of a
//! relation reference ascending string indices, which the builder below
//! arranges, so both readings of a committed fixture agree.
//!
//! Used only under `tests/`; the compiler binary consumes real, external
//! PBF extracts.

use std::collections::HashMap;

/// Wire types of protobuf (structural framing values).
const WIRE_TYPE_VARINT: u32 = 0;
const WIRE_TYPE_LENGTH_DELIMITED: u32 = 2;

/// A node of a fixture extract.
pub struct FixtureNode {
    osm_id: i64,
    latitude_degrees: f64,
    longitude_degrees: f64,
}

/// A way of a fixture extract.
pub struct FixtureWay {
    osm_id: i64,
    node_osm_ids: Vec<i64>,
    tags: Vec<(&'static str, &'static str)>,
}

/// Element type of a fixture relation member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixtureMemberType {
    /// References a node.
    Node,
    /// References a way.
    Way,
}

/// A member of a fixture relation.
pub struct FixtureRelationMember {
    member_type: FixtureMemberType,
    role: &'static str,
    member_osm_id: i64,
}

/// A relation of a fixture extract.
pub struct FixtureRelation {
    osm_id: i64,
    tags: Vec<(&'static str, &'static str)>,
    members: Vec<FixtureRelationMember>,
}

/// A fixture extract: an ordered set of nodes, ways and relations that
/// encodes deterministically into one PBF byte vector.
#[derive(Default)]
pub struct OsmPbfFixture {
    nodes: Vec<FixtureNode>,
    ways: Vec<FixtureWay>,
    relations: Vec<FixtureRelation>,
}

impl OsmPbfFixture {
    /// An empty extract.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a node.
    pub fn push_node(&mut self, osm_id: i64, latitude_degrees: f64, longitude_degrees: f64) {
        self.nodes.push(FixtureNode {
            osm_id,
            latitude_degrees,
            longitude_degrees,
        });
    }

    /// Appends a way with its ordered node references and tags.
    pub fn push_way(
        &mut self,
        osm_id: i64,
        node_osm_ids: Vec<i64>,
        tags: Vec<(&'static str, &'static str)>,
    ) {
        self.ways.push(FixtureWay {
            osm_id,
            node_osm_ids,
            tags,
        });
    }

    /// Appends a relation with its tags and members.
    pub fn push_relation(
        &mut self,
        osm_id: i64,
        tags: Vec<(&'static str, &'static str)>,
        members: Vec<FixtureRelationMember>,
    ) {
        self.relations.push(FixtureRelation {
            osm_id,
            tags,
            members,
        });
    }

    /// Encodes the fixture into deterministic PBF bytes.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut string_table = StringTableBuilder::new();
        let header_block = Vec::<u8>::new();
        let primitive_block = self.encode_primitive_block(&mut string_table);
        let mut file_bytes = Vec::new();
        frame_blob(&mut file_bytes, "OSMHeader", &header_block);
        frame_blob(&mut file_bytes, "OSMData", &primitive_block);
        file_bytes
    }

    fn encode_primitive_block(&self, string_table: &mut StringTableBuilder) -> Vec<u8> {
        let mut primitive_group = Vec::new();
        for fixture_node in &self.nodes {
            write_len_delimited_field(&mut primitive_group, 1, &encode_node(fixture_node));
        }
        for fixture_way in &self.ways {
            let way_message = encode_way(fixture_way, string_table);
            write_len_delimited_field(&mut primitive_group, 3, &way_message);
        }
        for fixture_relation in &self.relations {
            let relation_message = encode_relation(fixture_relation, string_table);
            write_len_delimited_field(&mut primitive_group, 4, &relation_message);
        }
        let mut primitive_block = Vec::new();
        write_len_delimited_field(&mut primitive_block, 1, &encode_string_table(string_table));
        write_len_delimited_field(&mut primitive_block, 2, &primitive_group);
        primitive_block
    }
}

/// Assigns stable string-table indices in first-use order.
struct StringTableBuilder {
    index_by_string: HashMap<String, i32>,
    entries: Vec<String>,
}

impl StringTableBuilder {
    fn new() -> Self {
        Self {
            index_by_string: HashMap::new(),
            entries: Vec::new(),
        }
    }

    fn index_of(&mut self, string: &str) -> i32 {
        let entries = &mut self.entries;
        *self
            .index_by_string
            .entry(string.to_string())
            .or_insert_with(|| {
                entries.push(string.to_string());
                (entries.len() - 1) as i32
            })
    }

    fn entries(&self) -> &[String] {
        &self.entries
    }
}

/// Encodes one plain (non-dense) node; lat/lon ride the 100-nanodegree
/// granularity default.
fn encode_node(fixture_node: &FixtureNode) -> Vec<u8> {
    let mut node_message = Vec::new();
    write_sint64_field(&mut node_message, 1, fixture_node.osm_id);
    write_sint64_field(
        &mut node_message,
        8,
        granularity_units(fixture_node.latitude_degrees),
    );
    write_sint64_field(
        &mut node_message,
        9,
        granularity_units(fixture_node.longitude_degrees),
    );
    node_message
}

/// Converts degrees into PBF position units under the default 100-nanodegree
/// granularity (`lat = 1e-7 * stored_value`).
fn granularity_units(degrees: f64) -> i64 {
    (degrees * 10_000_000.0).round() as i64
}

/// Encodes one way: tags as parallel packed `keys`/`vals` indices, node
/// references as packed delta-coded `sint64` refs.
fn encode_way(fixture_way: &FixtureWay, string_table: &mut StringTableBuilder) -> Vec<u8> {
    let mut way_message = Vec::new();
    write_varint_field(&mut way_message, 1, fixture_way.osm_id as u64);
    write_tag_fields(
        &mut way_message,
        2,
        &fixture_way.tags,
        string_table,
        |tag_entry| tag_entry.0,
    );
    write_tag_fields(
        &mut way_message,
        3,
        &fixture_way.tags,
        string_table,
        |tag_entry| tag_entry.1,
    );
    let delta_coded_refs = delta_encode(&fixture_way.node_osm_ids);
    write_packed_varints(&mut way_message, 8, &zigzag_encode_all(&delta_coded_refs));
    way_message
}

/// Encodes one relation: tags, raw-coded `roles_sid` (see module docs),
/// delta-coded `memids`, packed member types (`NODE = 0`, `WAY = 1`).
fn encode_relation(
    fixture_relation: &FixtureRelation,
    string_table: &mut StringTableBuilder,
) -> Vec<u8> {
    let mut relation_message = Vec::new();
    write_varint_field(&mut relation_message, 1, fixture_relation.osm_id as u64);
    write_tag_fields(
        &mut relation_message,
        2,
        &fixture_relation.tags,
        string_table,
        |tag_entry| tag_entry.0,
    );
    write_tag_fields(
        &mut relation_message,
        3,
        &fixture_relation.tags,
        string_table,
        |tag_entry| tag_entry.1,
    );
    let role_indices: Vec<i32> = fixture_relation
        .members
        .iter()
        .map(|member| string_table.index_of(member.role))
        .collect();
    write_packed_varints(
        &mut relation_message,
        8,
        &role_indices
            .iter()
            .map(|&role_index| role_index as i64 as u64)
            .collect::<Vec<u64>>(),
    );
    let member_osm_ids: Vec<i64> = fixture_relation
        .members
        .iter()
        .map(|member| member.member_osm_id)
        .collect();
    write_packed_varints(
        &mut relation_message,
        9,
        &zigzag_encode_all(&delta_encode(&member_osm_ids)),
    );
    write_packed_varints(
        &mut relation_message,
        10,
        &fixture_relation
            .members
            .iter()
            .map(|member| match member.member_type {
                FixtureMemberType::Node => 0_u64,
                FixtureMemberType::Way => 1_u64,
            })
            .collect::<Vec<u64>>(),
    );
    relation_message
}

fn encode_string_table(string_table: &StringTableBuilder) -> Vec<u8> {
    let mut string_table_message = Vec::new();
    for entry in string_table.entries() {
        write_len_delimited_field(&mut string_table_message, 1, entry.as_bytes());
    }
    string_table_message
}

/// Writes `keys` or `vals` (packed string-table indices of the tag keys or
/// values).
fn write_tag_fields(
    message: &mut Vec<u8>,
    field_number: u32,
    tags: &[(&'static str, &'static str)],
    string_table: &mut StringTableBuilder,
    pick_tag_side: impl Fn(&(&'static str, &'static str)) -> &'static str,
) {
    let tag_indices: Vec<u64> = tags
        .iter()
        .map(|tag_entry| string_table.index_of(pick_tag_side(tag_entry)) as u64)
        .collect();
    write_packed_varints(message, field_number, &tag_indices);
}

fn delta_encode(values: &[i64]) -> Vec<i64> {
    let mut delta_coded = Vec::with_capacity(values.len());
    let mut previous_value = 0_i64;
    for &value in values {
        delta_coded.push(value - previous_value);
        previous_value = value;
    }
    delta_coded
}

fn zigzag_encode_all(delta_coded: &[i64]) -> Vec<u64> {
    delta_coded
        .iter()
        .map(|&value| ((value << 1) ^ (value >> 63)) as u64)
        .collect()
}

fn write_varint(buffer: &mut Vec<u8>, mut value: u64) {
    loop {
        let mut byte = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        buffer.push(byte);
        if value == 0 {
            break;
        }
    }
}

/// Writes an `sint64` value in zigzag encoding.
fn write_sint64_field(buffer: &mut Vec<u8>, field_number: u32, value: i64) {
    write_varint_field(buffer, field_number, ((value << 1) ^ (value >> 63)) as u64);
}

/// Writes a plain (`uint32`/`int64` style) varint field.
fn write_varint_field(buffer: &mut Vec<u8>, field_number: u32, value: u64) {
    write_field_key(buffer, field_number, WIRE_TYPE_VARINT);
    write_varint(buffer, value);
}

fn write_field_key(buffer: &mut Vec<u8>, field_number: u32, wire_type: u32) {
    write_varint(
        buffer,
        (u64::from(field_number) << 3) | u64::from(wire_type),
    );
}

fn write_len_delimited_field(buffer: &mut Vec<u8>, field_number: u32, payload: &[u8]) {
    write_field_key(buffer, field_number, WIRE_TYPE_LENGTH_DELIMITED);
    write_varint(buffer, payload.len() as u64);
    buffer.extend_from_slice(payload);
}

/// Writes a packed repeated varint field (keys, vals, refs, memids, types).
fn write_packed_varints(buffer: &mut Vec<u8>, field_number: u32, values: &[u64]) {
    let mut payload = Vec::new();
    for &value in values {
        write_varint(&mut payload, value);
    }
    write_len_delimited_field(buffer, field_number, &payload);
}

/// Frames one blob into the PBF file: big-endian header length prefix,
/// `BlobHeader` (`type`, `datasize`), then the `Blob` holding the
/// uncompressed raw payload. `datasize` counts the serialized `Blob`
/// message (tag + length prefix + payload), as the file format requires,
/// so readers seeking past it stay aligned.
fn frame_blob(file_bytes: &mut Vec<u8>, blob_type: &str, payload: &[u8]) {
    let mut blob_header = Vec::new();
    write_len_delimited_field(&mut blob_header, 1, blob_type.as_bytes());
    let mut blob = Vec::new();
    write_len_delimited_field(&mut blob, 1, payload);
    write_varint_field(&mut blob_header, 3, blob.len() as u64);
    file_bytes.extend_from_slice(&u32::try_from(blob_header.len()).unwrap_or(0).to_be_bytes());
    file_bytes.extend_from_slice(&blob_header);
    file_bytes.extend_from_slice(&blob);
}

// ---------------------------------------------------------------------------
// The canonical committed fixture
// ---------------------------------------------------------------------------

/// Directory and name of the committed fixture extract.
pub const FIXTURE_PATH: &str = "fixtures/osm/routing_grid.osm.pbf";

/// Environment switch that (re)writes the committed fixture from the
/// canonical generator; always-on byte-equality tests guard against drift.
pub const REGENERATE_FIXTURE_ENVIRONMENT_VARIABLE: &str =
    "MAP_OFFLINE_GRAPH_COMPILER_REGENERATE_FIXTURE";

/// Directory and name of the committed *compiled archive* fixture: the
/// canonical extract compiled once by the native compiler binary and
/// checked in for the browser harness (the wasm target loads the offline
/// routing graph from in-memory bytes, and `platform/web/serve.mjs` serves
/// exactly this file same-origin). Always-on byte-equality tests guard
/// against drift, mirroring the fixture extract's guard.
pub const ARCHIVE_FIXTURE_PATH: &str = "fixtures/rmog/routing_grid.rmog";

/// Environment switch that (re)writes the committed compiled archive fixture
/// from the canonical compiler; an always-on byte-equality test guards it.
pub const REGENERATE_ARCHIVE_FIXTURE_ENVIRONMENT_VARIABLE: &str =
    "MAP_OFFLINE_GRAPH_COMPILER_REGENERATE_ARCHIVE_FIXTURE";

/// Path of the committed fixture extract.
#[must_use]
pub fn fixture_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(FIXTURE_PATH)
}

/// The committed fixture extract's bytes.
#[must_use]
pub fn fixture_bytes() -> Vec<u8> {
    std::fs::read(fixture_path()).unwrap()
}

/// Path of the committed compiled archive fixture.
#[must_use]
pub fn archive_fixture_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(ARCHIVE_FIXTURE_PATH)
}

/// The committed compiled archive fixture's bytes.
#[must_use]
pub fn archive_fixture_bytes() -> Vec<u8> {
    std::fs::read(archive_fixture_path()).unwrap()
}

/// Builds the canonical routing-grid fixture extract by hand:
///
/// - node 1003 (0.003, -0.003) belongs to no way and must be dropped by the
///   compiler;
/// - way 2001 "North South Avenue": residential, both directions, L-shaped
///   [1001 → 1002 → 1005];
/// - way 2002 "East West Street": residential, `oneway=yes`, [1004 → 1005 →
///   1006];
/// - way 2003 "Secluded Yard": service, `access=private` — dropped;
/// - way 2004 "Southside Link": service, `maxspeed=20`, [1006 → 1008],
///   both directions;
/// - relation 3001: `no_left_turn` from way 2001 via node 1005 onto way
///   2002.
#[must_use]
pub fn canonical_routing_grid_fixture() -> Vec<u8> {
    let mut fixture = OsmPbfFixture::new();
    fixture.push_node(1001, 0.000, 0.000);
    fixture.push_node(1002, 0.000, 0.001);
    fixture.push_node(1003, 0.003, -0.003);
    fixture.push_node(1004, 0.001, 0.000);
    fixture.push_node(1005, 0.001, 0.001);
    fixture.push_node(1006, 0.001, 0.002);
    fixture.push_node(1007, 0.002, 0.001);
    fixture.push_node(1008, 0.002, 0.002);
    fixture.push_way(
        2001,
        vec![1001, 1002, 1005],
        vec![("highway", "residential"), ("name", "North South Avenue")],
    );
    fixture.push_way(
        2002,
        vec![1004, 1005, 1006],
        vec![
            ("highway", "residential"),
            ("name", "East West Street"),
            ("oneway", "yes"),
        ],
    );
    fixture.push_way(
        2003,
        vec![1007, 1005],
        vec![
            ("highway", "service"),
            ("name", "Secluded Yard"),
            ("access", "private"),
        ],
    );
    fixture.push_way(
        2004,
        vec![1006, 1008],
        vec![
            ("highway", "service"),
            ("name", "Southside Link"),
            ("maxspeed", "20"),
        ],
    );
    fixture.push_relation(
        3001,
        vec![("type", "restriction"), ("restriction", "no_left_turn")],
        vec![
            FixtureRelationMember {
                member_type: FixtureMemberType::Way,
                role: "from",
                member_osm_id: 2001,
            },
            FixtureRelationMember {
                member_type: FixtureMemberType::Node,
                role: "via",
                member_osm_id: 1005,
            },
            FixtureRelationMember {
                member_type: FixtureMemberType::Way,
                role: "to",
                member_osm_id: 2002,
            },
        ],
    );
    fixture.encode()
}
