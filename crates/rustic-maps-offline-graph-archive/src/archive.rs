//! `OfflineRoutingNetworkGraph` archive format: records, deterministic
//! writer and validating in-memory reader.
//!
//! The container story lives in the crate root ([`crate`]). This module
//! holds the format constants, the packed record types, the builder that
//! emits bytes deterministically, and the reader that validates every load
//! against the typed corruption model (`SDKError::OfflineRoutingFailure`).
//! The memory-mapped zero-copy reader is the native-only sibling module
//! ([`crate::mmap_reader`]): it answers the same byte slices into this
//! in-memory reader, so both load paths share one validation
//! implementation.
//!
//! # Archive layout (format version 1)
//!
//! All integers little-endian:
//!
//! ```text
//!   0  magic "RMOG"                                       4 bytes
//!   4  format version (u32)                               4 bytes
//!   8  table-of-contents entry count (u32)                4 bytes
//!  12  table-of-contents entries (48 bytes each):
//!        resource name  : 32 ASCII bytes, NUL-padded
//!        resource offset: u64 (multiple of 8)
//!        resource length: u64
//! then the resource payloads, in canonical order, optionally separated by
//! up to 7 unnamed alignment-padding bytes (written as zero).
//! ```
//!
//! Canonical resources and record widths:
//!
//! | resource                     | record width | payload            |
//! |------------------------------|--------------|--------------------|
//! | `nodes`                      | 8            | coordinate records |
//! | `edges`                      | 20           | directed edges     |
//! | `lane_guidances`             | 8            | lane guidance      |
//! | `instruction_strings`        | blob         | UTF-8 text bytes   |
//! | `instruction_lookup_indices` | 4            | `instruction_strings` byte offsets, one per turn restriction (`INSTRUCTION_ABSENT_INDEX` = no text) |
//! | `turn_restrictions`          | 16           | restricted turns   |
//!
//! The edge `encoded_value` column packs the GraphHopper-style encoded
//! values: bits 0..3 [`RoadClass`], bits 3..8 speed cells
//! ([`SPEED_QUANTUM_KILOMETERS_PER_HOUR`] km/h per cell), bit 8 forward
//! access, bit 9 backward access, bits 10..32 reserved (must be zero).

use geospatial_types::{OfflineGraphInvalidReason, SDKError};

// ---------------------------------------------------------------------------
// Format constants
// ---------------------------------------------------------------------------

/// Archive magic bytes: "RMOG" (Rustic-Maps-Offline-Graph).
pub const ARCHIVE_MAGIC: [u8; 4] = *b"RMOG";

/// The only archive format version this crate writes and reads.
pub const ARCHIVE_FORMAT_VERSION: u32 = 1;

/// Byte width of a table-of-contents resource name field (ASCII, NUL-padded).
pub const RESOURCE_NAME_BYTES: usize = 32;

/// Every resource payload starts at a byte offset that is a multiple of this
/// alignment; unnamed zero padding of up to `ARCHIVE_RESOURCE_ALIGNMENT - 1`
/// bytes may separate resources.
pub const ARCHIVE_RESOURCE_ALIGNMENT: usize = 8;

/// Byte width of one node record.
pub const NODE_RECORD_BYTES: usize = 2 * U32_BYTES;

/// Byte width of one directed edge record.
pub const EDGE_RECORD_BYTES: usize = 5 * U32_BYTES;

/// Byte width of one lane guidance record.
pub const LANE_GUIDANCE_RECORD_BYTES: usize = 2 * U32_BYTES;

/// Byte width of one turn restriction record.
pub const TURN_RESTRICTION_RECORD_BYTES: usize = 4 * U32_BYTES;

/// Byte width of one instruction lookup index (an `instruction_strings`
/// byte offset).
pub const INSTRUCTION_LOOKUP_ENTRY_BYTES: usize = U32_BYTES;

/// Byte width of the file header (magic + format version + entry count).
pub const FILE_HEADER_BYTES: usize = ARCHIVE_MAGIC.len() + 2 * U32_BYTES;

/// Byte width of one table-of-contents entry.
pub const TABLE_OF_CONTENTS_ENTRY_BYTES: usize = RESOURCE_NAME_BYTES + 2 * U64_BYTES;

/// Lookup-index sentinel meaning "no precomputed instruction text exists
/// for this turn restriction".
pub const INSTRUCTION_ABSENT_INDEX: u32 = u32::MAX;

const U32_BYTES: usize = 4;
const U64_BYTES: usize = 8;

/// Canonical resource count; a different entry count fails as malformed —
/// new resources join only through a format version bump, keeping readers
/// strict.
const RESOURCE_COUNT: usize = 6;

/// Canonical resource list, in payload order: the graph members first, with
/// turn restrictions and instruction tables appended addition-style.
const CANONICAL_RESOURCE_NAMES: [&str; RESOURCE_COUNT] = [
    "nodes",
    "edges",
    "lane_guidances",
    "instruction_strings",
    "instruction_lookup_indices",
    "turn_restrictions",
];

// ---------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------

/// Quantized node position. Coordinates are stored as fixed-point
/// whole-`COORDINATE_QUANTUM_PER_DEGREE` offsets so every record stays a
/// plain integral column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeRecord {
    latitude_quantized: u32,
    longitude_quantized: u32,
}

impl NodeRecord {
    /// Builds a record from geographic degrees, rounding to the coordinate
    /// quantum.
    #[must_use]
    pub fn from_geographic_degrees(latitude_degrees: f64, longitude_degrees: f64) -> Self {
        let latitude_quantized = ((latitude_degrees + LATITUDE_OFFSET_DEGREES)
            * COORDINATE_QUANTUM_PER_DEGREE)
            .round() as u32;
        let longitude_quantized = ((longitude_degrees + LONGITUDE_OFFSET_DEGREES)
            * COORDINATE_QUANTUM_PER_DEGREE)
            .round() as u32;
        Self {
            latitude_quantized,
            longitude_quantized,
        }
    }

    /// Latitude in degrees (quantization-rounded).
    #[must_use]
    pub fn latitude_degrees(&self) -> f64 {
        f64::from(self.latitude_quantized) / COORDINATE_QUANTUM_PER_DEGREE - LATITUDE_OFFSET_DEGREES
    }

    /// Longitude in degrees (quantization-rounded).
    #[must_use]
    pub fn longitude_degrees(&self) -> f64 {
        f64::from(self.longitude_quantized) / COORDINATE_QUANTUM_PER_DEGREE
            - LONGITUDE_OFFSET_DEGREES
    }
}

const COORDINATE_QUANTUM_PER_DEGREE: f64 = 10_000_000.0;
const LATITUDE_OFFSET_DEGREES: f64 = 90.0;
const LONGITUDE_OFFSET_DEGREES: f64 = 180.0;

/// Road class of a directed edge, packed into the edge's encoded value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoadClass {
    /// Motorway.
    Motorway,
    /// Trunk road.
    Trunk,
    /// Primary road.
    Primary,
    /// Secondary road.
    Secondary,
    /// Tertiary road.
    Tertiary,
    /// Unclassified public road.
    Unclassified,
    /// Residential street.
    Residential,
    /// Service road.
    Service,
}

impl RoadClass {
    #[must_use]
    pub fn discriminant(self) -> u32 {
        match self {
            Self::Motorway => 0,
            Self::Trunk => 1,
            Self::Primary => 2,
            Self::Secondary => 3,
            Self::Tertiary => 4,
            Self::Unclassified => 5,
            Self::Residential => 6,
            Self::Service => 7,
        }
    }

    #[must_use]
    pub fn from_discriminant(discriminant: u32) -> Option<Self> {
        match discriminant {
            0 => Some(Self::Motorway),
            1 => Some(Self::Trunk),
            2 => Some(Self::Primary),
            3 => Some(Self::Secondary),
            4 => Some(Self::Tertiary),
            5 => Some(Self::Unclassified),
            6 => Some(Self::Residential),
            7 => Some(Self::Service),
            _ => None,
        }
    }
}

/// Turn restriction kind, packed into the restriction record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestrictionKind {
    /// Left turns are forbidden.
    NoLeftTurn,
    /// Right turns are forbidden.
    NoRightTurn,
    /// U-turns are forbidden.
    NoUTurn,
    /// Going straight on is forbidden.
    NoStraightOn,
    /// Only going straight on is allowed.
    OnlyStraightOn,
}

impl RestrictionKind {
    #[must_use]
    pub fn discriminant(self) -> u32 {
        match self {
            Self::NoLeftTurn => 0,
            Self::NoRightTurn => 1,
            Self::NoUTurn => 2,
            Self::NoStraightOn => 3,
            Self::OnlyStraightOn => 4,
        }
    }

    #[must_use]
    pub fn from_discriminant(discriminant: u32) -> Option<Self> {
        match discriminant {
            0 => Some(Self::NoLeftTurn),
            1 => Some(Self::NoRightTurn),
            2 => Some(Self::NoUTurn),
            3 => Some(Self::NoStraightOn),
            4 => Some(Self::OnlyStraightOn),
            _ => None,
        }
    }
}

const ROAD_CLASS_BIT_COUNT: u32 = 3;
const ROAD_CLASS_BIT_OFFSET: u32 = 0;
const SPEED_CELL_BIT_COUNT: u32 = 5;
const SPEED_CELL_BIT_OFFSET: u32 = ROAD_CLASS_BIT_COUNT;
const ACCESS_FORWARD_BIT_OFFSET: u32 = SPEED_CELL_BIT_OFFSET + SPEED_CELL_BIT_COUNT;
const ACCESS_BACKWARD_BIT_OFFSET: u32 = ACCESS_FORWARD_BIT_OFFSET + 1;
/// Bits above the access columns must be zero: writers emit them zero and
/// readers reject non-zero reserved bits as corruption (a newer writer must
/// bump [`ARCHIVE_FORMAT_VERSION`] instead).
const RESERVED_BIT_OFFSET: u32 = ACCESS_BACKWARD_BIT_OFFSET + 1;

/// Speed column quantum in km/h: the packed edge column stores whole
/// quantums only.
pub const SPEED_QUANTUM_KILOMETERS_PER_HOUR: u16 = 5;

/// Highest storable speed quantum count (`SPEED_CELL_BIT_COUNT` wide).
pub const SPEED_CELL_LIMIT: u16 = (1 << SPEED_CELL_BIT_COUNT) - 1;

/// Speed, road class and direction-access columns of a directed edge,
/// before packing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdgeProperties {
    /// Road classification.
    pub road_class: RoadClass,
    /// Speed along the edge in km/h (stored quantum-truncated and clamped).
    pub speed_kilometers_per_hour: u16,
    /// Whether travel along the OSM way node order exists and is allowed.
    pub forward_allowed: bool,
    /// Whether travel against the OSM way node order exists and is allowed.
    pub backward_allowed: bool,
}

impl EdgeProperties {
    fn encoded_value(self) -> u32 {
        let speed_cells = u32::from(speed_cells_from_kilometers_per_hour(
            self.speed_kilometers_per_hour,
        ));
        speed_cells << SPEED_CELL_BIT_OFFSET
            | self.road_class.discriminant() << ROAD_CLASS_BIT_OFFSET
            | u32::from(self.forward_allowed) << ACCESS_FORWARD_BIT_OFFSET
            | u32::from(self.backward_allowed) << ACCESS_BACKWARD_BIT_OFFSET
    }
}

/// Quantizes a km/h speed into storable speed cells (5 km/h each, clamped to
/// [`SPEED_CELL_LIMIT`]).
#[must_use]
pub fn speed_cells_from_kilometers_per_hour(speed_kilometers_per_hour: u16) -> u16 {
    (speed_kilometers_per_hour / SPEED_QUANTUM_KILOMETERS_PER_HOUR).min(SPEED_CELL_LIMIT)
}

/// A directed edge. Distance and duration are plain full-width columns;
/// class, speed and access ride the packed `encoded_value` column (the
/// GraphHopper encoded-values pattern, keeping the routing hot path
/// branch-free).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdgeRecord {
    source_node_index: u32,
    target_node_index: u32,
    distance_meters: u32,
    duration_milliseconds: u32,
    encoded_value: u32,
}

impl EdgeRecord {
    /// Builds a directed edge record, quantizing the properties into the
    /// encoded value.
    #[must_use]
    pub fn new_directed(
        source_node_index: u32,
        target_node_index: u32,
        distance_meters: u32,
        duration_milliseconds: u32,
        edge_properties: EdgeProperties,
    ) -> Self {
        Self {
            source_node_index,
            target_node_index,
            distance_meters,
            duration_milliseconds,
            encoded_value: edge_properties.encoded_value(),
        }
    }

    /// Index of the edge's source node.
    #[must_use]
    pub fn source_node_index(&self) -> u32 {
        self.source_node_index
    }

    /// Index of the edge's target node.
    #[must_use]
    pub fn target_node_index(&self) -> u32 {
        self.target_node_index
    }

    /// Distance along the edge in meters.
    #[must_use]
    pub fn distance_meters(&self) -> u32 {
        self.distance_meters
    }

    /// Traversal duration in milliseconds.
    #[must_use]
    pub fn duration_milliseconds(&self) -> u32 {
        self.duration_milliseconds
    }

    /// Raw packed encoded value (speed cells, road class, access, reserved).
    #[must_use]
    pub fn encoded_value(&self) -> u32 {
        self.encoded_value
    }

    /// Speed along the edge in km/h, as stored (5 km/h granularity).
    #[must_use]
    pub fn speed_kilometers_per_hour(&self) -> u16 {
        let speed_cells =
            (self.encoded_value >> SPEED_CELL_BIT_OFFSET) & ((1 << SPEED_CELL_BIT_COUNT) - 1);
        speed_cells as u16 * SPEED_QUANTUM_KILOMETERS_PER_HOUR
    }

    /// Road class, `None` on records that skipped load-time validation.
    #[must_use]
    pub fn road_class(&self) -> Option<RoadClass> {
        let discriminant =
            (self.encoded_value >> ROAD_CLASS_BIT_OFFSET) & ((1 << ROAD_CLASS_BIT_COUNT) - 1);
        RoadClass::from_discriminant(discriminant)
    }

    /// Whether the record covers the OSM way node-order direction.
    #[must_use]
    pub fn forward_allowed(&self) -> bool {
        self.encoded_value & (1 << ACCESS_FORWARD_BIT_OFFSET) != 0
    }

    /// Whether the record covers travel against the OSM way node order.
    #[must_use]
    pub fn backward_allowed(&self) -> bool {
        self.encoded_value & (1 << ACCESS_BACKWARD_BIT_OFFSET) != 0
    }
}

/// Precomputed lane guidance for an edge. The record carries a per-edge lane
/// count only — and that is exactly why the OSMPBF compiler emits none (see
/// `compiler.rs`): OSM `turn:lanes` expresses *which maneuvers each lane
/// permits* (e.g. `through|through|right`), while the SDK's step-level
/// `LaneGuidance` carries `permitted_directions` per lane. A counts-only
/// record can represent neither, so any production from today's tags would
/// either drop the turn information or fabricate a "recommended" lane. The
/// resource stays empty (and the format byte-stable) until lane geometry
/// enters the layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaneGuidanceRecord {
    edge_index: u32,
    lane_count: u32,
}

impl LaneGuidanceRecord {
    /// Builds a lane guidance record.
    #[must_use]
    pub fn new(edge_index: u32, lane_count: u32) -> Self {
        Self {
            edge_index,
            lane_count,
        }
    }

    /// Index of the guided edge.
    #[must_use]
    pub fn edge_index(&self) -> u32 {
        self.edge_index
    }

    /// Lane count along the guided edge.
    #[must_use]
    pub fn lane_count(&self) -> u32 {
        self.lane_count
    }
}

/// A restricted turn: from one directed edge, through a node, onto another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnRestrictionRecord {
    from_edge_index: u32,
    via_node_index: u32,
    to_edge_index: u32,
    restriction_kind_discriminant: u32,
}

impl TurnRestrictionRecord {
    /// Builds a turn restriction record.
    #[must_use]
    pub fn new(
        from_edge_index: u32,
        via_node_index: u32,
        to_edge_index: u32,
        restriction_kind: RestrictionKind,
    ) -> Self {
        Self {
            from_edge_index,
            via_node_index,
            to_edge_index,
            restriction_kind_discriminant: restriction_kind.discriminant(),
        }
    }

    /// Index of the edge leading into the restricted turn.
    #[must_use]
    pub fn from_edge_index(&self) -> u32 {
        self.from_edge_index
    }

    /// Index of the node the turn passes through.
    #[must_use]
    pub fn via_node_index(&self) -> u32 {
        self.via_node_index
    }

    /// Index of the edge the restricted turn would enter.
    #[must_use]
    pub fn to_edge_index(&self) -> u32 {
        self.to_edge_index
    }

    /// Restriction kind, `None` on records that skipped load-time
    /// validation.
    #[must_use]
    pub fn restriction_kind(&self) -> Option<RestrictionKind> {
        RestrictionKind::from_discriminant(self.restriction_kind_discriminant)
    }
}

// ---------------------------------------------------------------------------
// Builder / writer
// ---------------------------------------------------------------------------

/// Assembles all archive resources and encodes them deterministically.
///
/// Byte-for-byte determinism requires every vector to already be in final
/// stable order; the compiler documents and guarantees its ordering rules
/// (the `rustic-maps-offline-graph-compiler` crate). All alignment gaps are written
/// as zero bytes.
#[derive(Debug, Clone, Default)]
pub struct OfflineRoutingNetworkGraphBuilder {
    nodes: Vec<NodeRecord>,
    edges: Vec<EdgeRecord>,
    lane_guidances: Vec<LaneGuidanceRecord>,
    instruction_strings: Vec<u8>,
    instruction_lookup_indices: Vec<u32>,
    turn_restrictions: Vec<TurnRestrictionRecord>,
}

impl OfflineRoutingNetworkGraphBuilder {
    /// An empty archive; resources come in through the `with_*` builders.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the node records (must already be in final order).
    #[must_use]
    pub fn with_nodes(mut self, nodes: Vec<NodeRecord>) -> Self {
        self.nodes = nodes;
        self
    }

    /// Sets the edge records (must already be in final order).
    #[must_use]
    pub fn with_edges(mut self, edges: Vec<EdgeRecord>) -> Self {
        self.edges = edges;
        self
    }

    /// Sets the lane guidance records (must already be in final order).
    #[must_use]
    pub fn with_lane_guidances(mut self, lane_guidances: Vec<LaneGuidanceRecord>) -> Self {
        self.lane_guidances = lane_guidances;
        self
    }

    /// Sets the concatenated UTF-8 instruction string blob.
    #[must_use]
    pub fn with_instruction_strings(mut self, instruction_strings: Vec<u8>) -> Self {
        self.instruction_strings = instruction_strings;
        self
    }

    /// Sets the lookup indices (one per turn restriction, or
    /// [`INSTRUCTION_ABSENT_INDEX`]).
    #[must_use]
    pub fn with_instruction_lookup_indices(mut self, instruction_lookup_indices: Vec<u32>) -> Self {
        self.instruction_lookup_indices = instruction_lookup_indices;
        self
    }

    /// Sets the turn restriction records (must already be in final order).
    #[must_use]
    pub fn with_turn_restrictions(mut self, turn_restrictions: Vec<TurnRestrictionRecord>) -> Self {
        self.turn_restrictions = turn_restrictions;
        self
    }

    /// Validates every cross-reference and encodes the archive bytes.
    ///
    /// # Errors
    /// Returns `SDKError::OfflineRoutingFailure` for any out-of-range index
    /// the caller supplied — the same typed corruption contract the reader
    /// enforces on load, so writers fail symmetrically with readers.
    pub fn encode_deterministic(self) -> Result<Vec<u8>, SDKError> {
        self.validate_references()?;
        Ok(encode_bytes(
            &self.nodes,
            &self.edges,
            &self.lane_guidances,
            &self.instruction_strings,
            &self.instruction_lookup_indices,
            &self.turn_restrictions,
        ))
    }

    fn validate_references(&self) -> Result<(), SDKError> {
        let node_count = self.nodes.len();
        let edge_count = self.edges.len();
        for edge in &self.edges {
            if edge.source_node_index as usize >= node_count
                || edge.target_node_index as usize >= node_count
            {
                return Err(index_out_of_range_error());
            }
        }
        for restriction in &self.turn_restrictions {
            if restriction.from_edge_index() as usize >= edge_count
                || restriction.to_edge_index() as usize >= edge_count
                || restriction.via_node_index() as usize >= node_count
                || restriction.restriction_kind().is_none()
            {
                return Err(index_out_of_range_error());
            }
        }
        for lane_guidance in &self.lane_guidances {
            if lane_guidance.edge_index() as usize >= edge_count {
                return Err(index_out_of_range_error());
            }
        }
        if self.instruction_lookup_indices.len() != self.turn_restrictions.len() {
            return Err(index_out_of_range_error());
        }
        let instruction_text = std::str::from_utf8(&self.instruction_strings).map_err(|_| {
            SDKError::OfflineRoutingFailure(OfflineGraphInvalidReason::ArchiveMalformed)
        })?;
        for &lookup_index in &self.instruction_lookup_indices {
            if lookup_index != INSTRUCTION_ABSENT_INDEX {
                let offset = lookup_index as usize;
                if offset > instruction_text.len() || !instruction_text.is_char_boundary(offset) {
                    return Err(index_out_of_range_error());
                }
            }
        }
        Ok(())
    }
}

fn index_out_of_range_error() -> SDKError {
    SDKError::OfflineRoutingFailure(OfflineGraphInvalidReason::IndexOutOfRange)
}

/// Serializes the archive in canonical resource order (see module docs).
fn encode_bytes(
    nodes: &[NodeRecord],
    edges: &[EdgeRecord],
    lane_guidances: &[LaneGuidanceRecord],
    instruction_strings: &[u8],
    instruction_lookup_indices: &[u32],
    turn_restrictions: &[TurnRestrictionRecord],
) -> Vec<u8> {
    let resource_lengths: [usize; RESOURCE_COUNT] = [
        nodes.len() * NODE_RECORD_BYTES,
        edges.len() * EDGE_RECORD_BYTES,
        lane_guidances.len() * LANE_GUIDANCE_RECORD_BYTES,
        instruction_strings.len(),
        instruction_lookup_indices.len() * INSTRUCTION_LOOKUP_ENTRY_BYTES,
        turn_restrictions.len() * TURN_RESTRICTION_RECORD_BYTES,
    ];

    let resources_start = FILE_HEADER_BYTES + RESOURCE_COUNT * TABLE_OF_CONTENTS_ENTRY_BYTES;
    let mut resource_offsets = [0usize; RESOURCE_COUNT];
    let mut cursor = resources_start;
    for (resource_index, &resource_length) in resource_lengths.iter().enumerate() {
        let aligned_offset = align_offset(cursor);
        resource_offsets[resource_index] = aligned_offset;
        cursor = aligned_offset + resource_length;
    }
    let archive_length = align_offset(cursor);

    let mut archive_bytes = vec![0_u8; archive_length];
    archive_bytes[..ARCHIVE_MAGIC.len()].copy_from_slice(&ARCHIVE_MAGIC);
    write_u32(&mut archive_bytes, 4, ARCHIVE_FORMAT_VERSION);
    write_u32(&mut archive_bytes, 8, RESOURCE_COUNT as u32);

    for (resource_index, resource_name) in CANONICAL_RESOURCE_NAMES.iter().enumerate() {
        write_table_of_contents_entry(
            &mut archive_bytes,
            FILE_HEADER_BYTES + resource_index * TABLE_OF_CONTENTS_ENTRY_BYTES,
            resource_name,
            resource_offsets[resource_index],
            resource_lengths[resource_index],
        );
    }

    write_records(
        &mut archive_bytes,
        resource_offsets[0],
        nodes,
        NODE_RECORD_BYTES,
        |node, row| {
            write_u32(row, 0, node.latitude_quantized);
            write_u32(row, 4, node.longitude_quantized);
        },
    );
    write_records(
        &mut archive_bytes,
        resource_offsets[1],
        edges,
        EDGE_RECORD_BYTES,
        |edge, row| {
            write_u32(row, 0, edge.source_node_index);
            write_u32(row, 4, edge.target_node_index);
            write_u32(row, 8, edge.distance_meters);
            write_u32(row, 12, edge.duration_milliseconds);
            write_u32(row, 16, edge.encoded_value);
        },
    );
    write_records(
        &mut archive_bytes,
        resource_offsets[2],
        lane_guidances,
        LANE_GUIDANCE_RECORD_BYTES,
        |lane_guidance, row| {
            write_u32(row, 0, lane_guidance.edge_index);
            write_u32(row, 4, lane_guidance.lane_count);
        },
    );
    let instruction_offset = resource_offsets[3];
    archive_bytes[instruction_offset..instruction_offset + instruction_strings.len()]
        .copy_from_slice(instruction_strings);
    write_records(
        &mut archive_bytes,
        resource_offsets[4],
        instruction_lookup_indices,
        INSTRUCTION_LOOKUP_ENTRY_BYTES,
        |lookup_index, row| write_u32(row, 0, *lookup_index),
    );
    write_records(
        &mut archive_bytes,
        resource_offsets[5],
        turn_restrictions,
        TURN_RESTRICTION_RECORD_BYTES,
        |restriction, row| {
            write_u32(row, 0, restriction.from_edge_index);
            write_u32(row, 4, restriction.via_node_index);
            write_u32(row, 8, restriction.to_edge_index);
            write_u32(row, 12, restriction.restriction_kind_discriminant);
        },
    );
    archive_bytes
}

/// Rounds a byte length up to the next [`ARCHIVE_RESOURCE_ALIGNMENT`]
/// boundary (identity when already aligned).
fn align_offset(offset: usize) -> usize {
    offset.div_ceil(ARCHIVE_RESOURCE_ALIGNMENT) * ARCHIVE_RESOURCE_ALIGNMENT
}

fn write_records<T>(
    archive_bytes: &mut [u8],
    resource_offset: usize,
    records: &[T],
    record_bytes: usize,
    write_record: impl Fn(&T, &mut [u8]),
) {
    for (record_index, record) in records.iter().enumerate() {
        let row_start = resource_offset + record_index * record_bytes;
        let row = &mut archive_bytes[row_start..row_start + record_bytes];
        write_record(record, row);
    }
}

fn write_u32(destination: &mut [u8], byte_offset: usize, value: u32) {
    destination[byte_offset..byte_offset + U32_BYTES].copy_from_slice(&value.to_le_bytes());
}

fn write_u64(destination: &mut [u8], byte_offset: usize, value: u64) {
    destination[byte_offset..byte_offset + U64_BYTES].copy_from_slice(&value.to_le_bytes());
}

fn write_table_of_contents_entry(
    archive_bytes: &mut [u8],
    entry_offset: usize,
    resource_name: &str,
    resource_offset: usize,
    resource_length: usize,
) {
    let name_bytes = resource_name.as_bytes();
    archive_bytes[entry_offset..entry_offset + name_bytes.len()].copy_from_slice(name_bytes);
    write_u64(
        archive_bytes,
        entry_offset + RESOURCE_NAME_BYTES,
        resource_offset as u64,
    );
    write_u64(
        archive_bytes,
        entry_offset + RESOURCE_NAME_BYTES + U64_BYTES,
        resource_length as u64,
    );
}

// ---------------------------------------------------------------------------
// Reader
// ---------------------------------------------------------------------------

/// A validated in-memory `OfflineRoutingNetworkGraph` archive.
///
/// Reading validates the container (header, table of contents, alignment,
/// resource bounds, packed-value enums, UTF-8 instruction strings) and every
/// cross-reference, failing typed with the three
/// `OfflineGraphInvalidReason`s, never UB on corrupt input. The
/// native memory-mapped reader ([`crate::mmap_reader`]) answers the same
/// byte slices into this implementation, sharing this validation logic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfflineRoutingNetworkGraph {
    nodes: Vec<NodeRecord>,
    edges: Vec<EdgeRecord>,
    lane_guidances: Vec<LaneGuidanceRecord>,
    instruction_strings: Vec<u8>,
    instruction_lookup_indices: Vec<u32>,
    turn_restrictions: Vec<TurnRestrictionRecord>,
}

impl OfflineRoutingNetworkGraph {
    /// Parses and fully validates archive bytes.
    ///
    /// # Errors
    /// - `OfflineGraphInvalidReason::ArchiveMalformed` — bad magic, bad
    ///   table of contents, misaligned/unordered/out-of-bounds resources,
    ///   unknown enum values or set reserved bits, non-UTF-8 instruction
    ///   strings.
    /// - `OfflineGraphInvalidReason::VersionUnsupported` — magic recognized
    ///   but the format version is not this crate's.
    /// - `OfflineGraphInvalidReason::IndexOutOfRange` — any record
    ///   references an out-of-range node, edge or instruction offset.
    pub fn from_bytes(archive_bytes: &[u8]) -> Result<Self, SDKError> {
        if archive_bytes.len() < FILE_HEADER_BYTES
            || archive_bytes.get(..ARCHIVE_MAGIC.len()) != Some(ARCHIVE_MAGIC.as_slice())
        {
            return Err(SDKError::OfflineRoutingFailure(
                OfflineGraphInvalidReason::ArchiveMalformed,
            ));
        }
        if read_u32(archive_bytes, 4) != ARCHIVE_FORMAT_VERSION {
            return Err(SDKError::OfflineRoutingFailure(
                OfflineGraphInvalidReason::VersionUnsupported,
            ));
        }
        if read_u32(archive_bytes, 8) != RESOURCE_COUNT as u32 {
            return Err(SDKError::OfflineRoutingFailure(
                OfflineGraphInvalidReason::ArchiveMalformed,
            ));
        }

        let resources_start =
            FILE_HEADER_BYTES + CANONICAL_RESOURCE_NAMES.len() * TABLE_OF_CONTENTS_ENTRY_BYTES;
        if archive_bytes.len() < resources_start {
            return Err(SDKError::OfflineRoutingFailure(
                OfflineGraphInvalidReason::ArchiveMalformed,
            ));
        }

        let mut resource_spans = [[0usize; 2]; CANONICAL_RESOURCE_NAMES.len()];
        let mut previous_resource_end = resources_start;
        for (resource_index, &resource_name) in CANONICAL_RESOURCE_NAMES.iter().enumerate() {
            let entry_offset = FILE_HEADER_BYTES + resource_index * TABLE_OF_CONTENTS_ENTRY_BYTES;
            let stored_name = read_resource_name(archive_bytes, entry_offset)?;
            if stored_name != resource_name {
                return Err(SDKError::OfflineRoutingFailure(
                    OfflineGraphInvalidReason::ArchiveMalformed,
                ));
            }
            let resource_offset =
                read_u64(archive_bytes, entry_offset + RESOURCE_NAME_BYTES) as usize;
            let resource_length = read_u64(
                archive_bytes,
                entry_offset + RESOURCE_NAME_BYTES + U64_BYTES,
            ) as usize;
            if resource_offset < previous_resource_end
                || !resource_offset.is_multiple_of(ARCHIVE_RESOURCE_ALIGNMENT)
                || resource_offset
                    .checked_add(resource_length)
                    .is_none_or(|resource_end| resource_end > archive_bytes.len())
            {
                return Err(SDKError::OfflineRoutingFailure(
                    OfflineGraphInvalidReason::ArchiveMalformed,
                ));
            }
            resource_spans[resource_index] = [resource_offset, resource_length];
            previous_resource_end = resource_offset + resource_length;
        }

        let node_bytes = resource_payload(archive_bytes, resource_spans[0]);
        let edge_bytes = resource_payload(archive_bytes, resource_spans[1]);
        let lane_guidance_bytes = resource_payload(archive_bytes, resource_spans[2]);
        let instruction_blob = resource_payload(archive_bytes, resource_spans[3]);
        let lookup_bytes = resource_payload(archive_bytes, resource_spans[4]);
        let restriction_bytes = resource_payload(archive_bytes, resource_spans[5]);

        expect_record_multiple(node_bytes, NODE_RECORD_BYTES)?;
        expect_record_multiple(edge_bytes, EDGE_RECORD_BYTES)?;
        expect_record_multiple(lane_guidance_bytes, LANE_GUIDANCE_RECORD_BYTES)?;
        expect_record_multiple(lookup_bytes, INSTRUCTION_LOOKUP_ENTRY_BYTES)?;
        expect_record_multiple(restriction_bytes, TURN_RESTRICTION_RECORD_BYTES)?;

        let nodes: Vec<NodeRecord> = node_bytes
            .as_chunks::<NODE_RECORD_BYTES>()
            .0
            .iter()
            .map(|record_row| NodeRecord {
                latitude_quantized: row_u32(record_row, 0),
                longitude_quantized: row_u32(record_row, 4),
            })
            .collect();
        let edges: Vec<EdgeRecord> = edge_bytes
            .as_chunks::<EDGE_RECORD_BYTES>()
            .0
            .iter()
            .map(|record_row| EdgeRecord {
                source_node_index: row_u32(record_row, 0),
                target_node_index: row_u32(record_row, 4),
                distance_meters: row_u32(record_row, 8),
                duration_milliseconds: row_u32(record_row, 12),
                encoded_value: row_u32(record_row, 16),
            })
            .collect();
        let lane_guidances: Vec<LaneGuidanceRecord> = lane_guidance_bytes
            .as_chunks::<LANE_GUIDANCE_RECORD_BYTES>()
            .0
            .iter()
            .map(|record_row| LaneGuidanceRecord {
                edge_index: row_u32(record_row, 0),
                lane_count: row_u32(record_row, 4),
            })
            .collect();
        let instruction_lookup_indices: Vec<u32> = lookup_bytes
            .as_chunks::<INSTRUCTION_LOOKUP_ENTRY_BYTES>()
            .0
            .iter()
            .map(|record_row| row_u32(record_row, 0))
            .collect();
        let turn_restrictions: Vec<TurnRestrictionRecord> = restriction_bytes
            .as_chunks::<TURN_RESTRICTION_RECORD_BYTES>()
            .0
            .iter()
            .map(|record_row| TurnRestrictionRecord {
                from_edge_index: row_u32(record_row, 0),
                via_node_index: row_u32(record_row, 4),
                to_edge_index: row_u32(record_row, 8),
                restriction_kind_discriminant: row_u32(record_row, 12),
            })
            .collect();

        // Packed-value enums and reserved bits: corruption checks keeping
        // the accessors honest afterwards.
        for edge in &edges {
            if edge.encoded_value >> RESERVED_BIT_OFFSET != 0 || edge.road_class().is_none() {
                return Err(SDKError::OfflineRoutingFailure(
                    OfflineGraphInvalidReason::ArchiveMalformed,
                ));
            }
        }
        for restriction in &turn_restrictions {
            if restriction.restriction_kind().is_none() {
                return Err(SDKError::OfflineRoutingFailure(
                    OfflineGraphInvalidReason::ArchiveMalformed,
                ));
            }
        }

        // Cross-references.
        for edge in &edges {
            if edge.source_node_index as usize >= nodes.len()
                || edge.target_node_index as usize >= nodes.len()
            {
                return Err(index_out_of_range_error());
            }
        }
        for restriction in &turn_restrictions {
            if restriction.from_edge_index() as usize >= edges.len()
                || restriction.to_edge_index() as usize >= edges.len()
                || restriction.via_node_index() as usize >= nodes.len()
            {
                return Err(index_out_of_range_error());
            }
        }
        for lane_guidance in &lane_guidances {
            if lane_guidance.edge_index() as usize >= edges.len() {
                return Err(index_out_of_range_error());
            }
        }
        if instruction_lookup_indices.len() != turn_restrictions.len() {
            return Err(index_out_of_range_error());
        }
        let instruction_text = std::str::from_utf8(instruction_blob).map_err(|_| {
            SDKError::OfflineRoutingFailure(OfflineGraphInvalidReason::ArchiveMalformed)
        })?;
        for &lookup_index in &instruction_lookup_indices {
            if lookup_index != INSTRUCTION_ABSENT_INDEX {
                let offset = lookup_index as usize;
                if offset > instruction_text.len() || !instruction_text.is_char_boundary(offset) {
                    return Err(index_out_of_range_error());
                }
            }
        }

        Ok(Self {
            nodes,
            edges,
            lane_guidances,
            instruction_strings: instruction_blob.to_vec(),
            instruction_lookup_indices,
            turn_restrictions,
        })
    }

    /// All node records.
    #[must_use]
    pub fn nodes(&self) -> &[NodeRecord] {
        &self.nodes
    }

    /// All directed edge records, in canonical order (the compiler's
    /// determinism contract in the `rustic-maps-offline-graph-compiler` crate).
    #[must_use]
    pub fn edges(&self) -> &[EdgeRecord] {
        &self.edges
    }

    /// All lane guidance records.
    #[must_use]
    pub fn lane_guidances(&self) -> &[LaneGuidanceRecord] {
        &self.lane_guidances
    }

    /// All turn restriction records.
    #[must_use]
    pub fn turn_restrictions(&self) -> &[TurnRestrictionRecord] {
        &self.turn_restrictions
    }

    /// All instruction lookup indices (parallel to [`Self::turn_restrictions`]).
    #[must_use]
    pub fn instruction_lookup_indices(&self) -> &[u32] {
        &self.instruction_lookup_indices
    }

    /// Precomputed instruction text for one lookup entry, `None` only for
    /// the [`INSTRUCTION_ABSENT_INDEX`] sentinel.
    ///
    /// Text extent: from this lookup's offset to the next non-absent lookup
    /// offset, or the end of the blob. Load-time validation guarantees every
    /// non-absent offset is inside the blob on a UTF-8 character boundary,
    /// so the extent stays a valid UTF-8 sub-slice.
    #[must_use]
    pub fn instruction_text(&self, lookup_entry_index: usize) -> Option<&str> {
        let lookup_index = self
            .instruction_lookup_indices
            .get(lookup_entry_index)
            .copied()?;
        if lookup_index == INSTRUCTION_ABSENT_INDEX {
            return None;
        }
        let text_start = lookup_index as usize;
        let text_end = self
            .instruction_lookup_indices
            .get(lookup_entry_index + 1..)
            .and_then(|rest| {
                rest.iter()
                    .copied()
                    .find(|&candidate| candidate != INSTRUCTION_ABSENT_INDEX)
            })
            .map_or(self.instruction_strings.len(), |candidate| {
                candidate as usize
            });
        std::str::from_utf8(&self.instruction_strings[text_start..text_end]).ok()
    }
}

/// Slices a validated resource payload out of the archive.
fn resource_payload(archive_bytes: &[u8], [resource_offset, resource_length]: [usize; 2]) -> &[u8] {
    &archive_bytes[resource_offset..resource_offset + resource_length]
}

fn read_resource_name(archive_bytes: &[u8], entry_offset: usize) -> Result<String, SDKError> {
    let name_field = &archive_bytes[entry_offset..entry_offset + RESOURCE_NAME_BYTES];
    let name_end = name_field.iter().take_while(|&byte| byte != &b'\0').count();
    if !name_field[..name_end].is_ascii() {
        return Err(SDKError::OfflineRoutingFailure(
            OfflineGraphInvalidReason::ArchiveMalformed,
        ));
    }
    Ok(String::from_utf8_lossy(&name_field[..name_end]).into_owned())
}

fn expect_record_multiple(resource_bytes: &[u8], record_bytes: usize) -> Result<(), SDKError> {
    if !resource_bytes.len().is_multiple_of(record_bytes) {
        return Err(SDKError::OfflineRoutingFailure(
            OfflineGraphInvalidReason::ArchiveMalformed,
        ));
    }
    Ok(())
}

fn read_u32(archive_bytes: &[u8], byte_offset: usize) -> u32 {
    let mut value_bytes = [0_u8; U32_BYTES];
    value_bytes.copy_from_slice(&archive_bytes[byte_offset..byte_offset + U32_BYTES]);
    u32::from_le_bytes(value_bytes)
}

fn read_u64(archive_bytes: &[u8], byte_offset: usize) -> u64 {
    let mut value_bytes = [0_u8; U64_BYTES];
    value_bytes.copy_from_slice(&archive_bytes[byte_offset..byte_offset + U64_BYTES]);
    u64::from_le_bytes(value_bytes)
}

fn row_u32(record_row: &[u8], byte_offset: usize) -> u32 {
    let mut value_bytes = [0_u8; U32_BYTES];
    value_bytes.copy_from_slice(&record_row[byte_offset..byte_offset + U32_BYTES]);
    u32::from_le_bytes(value_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn residential_edge_properties() -> EdgeProperties {
        EdgeProperties {
            road_class: RoadClass::Residential,
            speed_kilometers_per_hour: 30,
            forward_allowed: true,
            backward_allowed: true,
        }
    }

    fn minimal_three_node_builder() -> OfflineRoutingNetworkGraphBuilder {
        let nodes = vec![
            NodeRecord::from_geographic_degrees(0.0, 0.0),
            NodeRecord::from_geographic_degrees(0.0, 0.001),
            NodeRecord::from_geographic_degrees(0.001, 0.0),
        ];
        let edges = vec![
            EdgeRecord::new_directed(0, 1, 111, 13_320, residential_edge_properties()),
            EdgeRecord::new_directed(1, 2, 111, 13_320, residential_edge_properties()),
        ];
        OfflineRoutingNetworkGraphBuilder::new()
            .with_nodes(nodes)
            .with_edges(edges)
            .with_lane_guidances(Vec::new())
            .with_instruction_strings(Vec::new())
            .with_instruction_lookup_indices(Vec::new())
            .with_turn_restrictions(Vec::new())
    }

    #[test]
    fn encoded_value_round_trips_through_packing() {
        let edge = EdgeRecord::new_directed(1, 2, 50, 6_000, residential_edge_properties());
        assert_eq!(edge.source_node_index(), 1);
        assert_eq!(edge.target_node_index(), 2);
        assert_eq!(edge.distance_meters(), 50);
        assert_eq!(edge.duration_milliseconds(), 6_000);
        assert_eq!(edge.road_class(), Some(RoadClass::Residential));
        assert_eq!(edge.speed_kilometers_per_hour(), 30);
        assert!(edge.forward_allowed());
        assert!(edge.backward_allowed());
        assert_eq!(edge.encoded_value >> RESERVED_BIT_OFFSET, 0);
    }

    #[test]
    fn speed_quantization_clamps_and_truncates() {
        assert_eq!(speed_cells_from_kilometers_per_hour(0), 0);
        assert_eq!(speed_cells_from_kilometers_per_hour(44), 8);
        assert_eq!(speed_cells_from_kilometers_per_hour(155), 31);
        assert_eq!(speed_cells_from_kilometers_per_hour(400), 31);
    }

    #[test]
    fn encode_then_read_preserves_all_resources() {
        let builder = minimal_three_node_builder().with_lane_guidances(vec![
            LaneGuidanceRecord::new(0, 2),
            LaneGuidanceRecord::new(1, 1),
        ]);
        let archive_bytes = builder.encode_deterministic().unwrap();
        let graph = OfflineRoutingNetworkGraph::from_bytes(&archive_bytes).unwrap();

        assert_eq!(graph.nodes().len(), 3);
        assert_eq!(graph.nodes()[0].latitude_degrees(), 0.0);
        assert_eq!(graph.nodes()[0].longitude_degrees(), 0.0);
        assert_eq!(graph.edges().len(), 2);
        assert_eq!(graph.edges()[0].road_class(), Some(RoadClass::Residential));
        assert_eq!(graph.edges()[1].target_node_index(), 2);
        assert_eq!(graph.lane_guidances().len(), 2);
        assert_eq!(graph.lane_guidances()[0].lane_count(), 2);
        assert!(graph.turn_restrictions().is_empty());
        assert!(graph.instruction_lookup_indices().is_empty());
    }

    #[test]
    fn reader_rejects_truncated_table_of_contents() {
        let archive_bytes = minimal_three_node_builder().encode_deterministic().unwrap();
        let truncated = &archive_bytes[..FILE_HEADER_BYTES + TABLE_OF_CONTENTS_ENTRY_BYTES];
        assert_eq!(
            OfflineRoutingNetworkGraph::from_bytes(truncated).unwrap_err(),
            SDKError::OfflineRoutingFailure(OfflineGraphInvalidReason::ArchiveMalformed),
        );
    }

    #[test]
    fn reader_rejects_bad_magic_as_archive_malformed() {
        let mut archive_bytes = minimal_three_node_builder().encode_deterministic().unwrap();
        archive_bytes[0] = b'X';
        assert_eq!(
            OfflineRoutingNetworkGraph::from_bytes(&archive_bytes).unwrap_err(),
            SDKError::OfflineRoutingFailure(OfflineGraphInvalidReason::ArchiveMalformed),
        );
    }

    #[test]
    fn reader_rejects_unknown_format_version() {
        let mut archive_bytes = minimal_three_node_builder().encode_deterministic().unwrap();
        archive_bytes[4] = 0x2A;
        assert_eq!(
            OfflineRoutingNetworkGraph::from_bytes(&archive_bytes).unwrap_err(),
            SDKError::OfflineRoutingFailure(OfflineGraphInvalidReason::VersionUnsupported),
        );
    }

    #[test]
    fn reader_rejects_out_of_range_edge_reference() {
        // The builder refuses out-of-range references by construction (the
        // next test), so corrupt the encoded bytes instead: point the first
        // edge's source node index past the node set.
        let mut archive_bytes = minimal_three_node_builder().encode_deterministic().unwrap();
        let edges_resource_index = CANONICAL_RESOURCE_NAMES
            .iter()
            .position(|resource_name| *resource_name == "edges")
            .unwrap();
        let entry_offset = FILE_HEADER_BYTES + edges_resource_index * TABLE_OF_CONTENTS_ENTRY_BYTES;
        let edges_offset = read_u64(&archive_bytes, entry_offset + RESOURCE_NAME_BYTES) as usize;
        let corrupted_source_index = u32::MAX;
        archive_bytes[edges_offset..edges_offset + U32_BYTES]
            .copy_from_slice(&corrupted_source_index.to_le_bytes());
        assert_eq!(
            OfflineRoutingNetworkGraph::from_bytes(&archive_bytes).unwrap_err(),
            SDKError::OfflineRoutingFailure(OfflineGraphInvalidReason::IndexOutOfRange),
        );
    }

    #[test]
    fn writer_rejects_out_of_range_references_symmetrically() {
        let builder = minimal_three_node_builder().with_edges(vec![
            EdgeRecord::new_directed(0, 1, 111, 13_320, residential_edge_properties()),
            EdgeRecord::new_directed(1, 99, 111, 13_320, residential_edge_properties()),
        ]);
        assert_eq!(
            builder.encode_deterministic().unwrap_err(),
            SDKError::OfflineRoutingFailure(OfflineGraphInvalidReason::IndexOutOfRange),
        );
    }

    #[test]
    fn reader_rejects_reserved_bits_in_encoded_value() {
        let builder = minimal_three_node_builder().with_edges(vec![
            EdgeRecord::new_directed(0, 1, 111, 13_320, residential_edge_properties()),
            EdgeRecord {
                source_node_index: 1,
                target_node_index: 2,
                distance_meters: 111,
                duration_milliseconds: 13_320,
                encoded_value: residential_edge_properties().encoded_value()
                    | (1 << RESERVED_BIT_OFFSET),
            },
        ]);
        let archive_bytes = builder.encode_deterministic().unwrap();
        assert_eq!(
            OfflineRoutingNetworkGraph::from_bytes(&archive_bytes).unwrap_err(),
            SDKError::OfflineRoutingFailure(OfflineGraphInvalidReason::ArchiveMalformed),
        );
    }
}
