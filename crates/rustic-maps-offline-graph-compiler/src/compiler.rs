//! The compiler pipeline: OpenStreetMap PBF extract → deterministic
//! [`OfflineRoutingNetworkGraph`] archive bytes.
//!
//! # Determinism contract
//!
//! Two runs over the same PBF produce byte-identical archives. The ordering
//! rules that make this true:
//!
//! - nodes keep their surviving graph nodes in ascending OSM node id order;
//! - edges sort by (`source_node_index`, `target_node_index`, originating
//!   OSM way id);
//! - lane guidance records (none emitted — see [`LaneGuidanceRecord`]'s
//!   documentation for the schema refusal) would sort by edge index;
//! - turn restrictions sort by (`from_edge_index`, `via_node_index`,
//!   `to_edge_index`), and instruction strings are emitted in that same
//!   restriction order;
//! - the writer's canonical resource order and zero-filled alignment gaps
//!   are fixed in [`crate::archive`].
//!
//! # Pipeline
//!
//! The pipeline collects nodes, drivable ways and restriction relations in
//! one sequential pass ([`profile`] applies the routing rules), then
//! resolves graph topology and restrictions in memory. The compiler is built
//! for metro-scale extracts: everything is held in memory, and unresolved
//! restrictions are skipped rather than failing the compile.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{self, Cursor};

use osmpbf::{Element, ElementReader};

use crate::archive::{
    EdgeRecord, LaneGuidanceRecord, NodeRecord, OfflineRoutingNetworkGraphBuilder, RestrictionKind,
    TurnRestrictionRecord, speed_cells_from_kilometers_per_hour,
};
use crate::profile::{
    TravelDirection, edge_properties_for_direction, instruction_phrase_for_restriction_kind,
    is_blocked_by_access, posted_speed_override_kilometers_per_hour,
    restriction_kind_for_openstreetmap_value, road_class_and_default_speed_for_highway,
    travel_direction_for_oneway_tag,
};

/// Haversine computation uses this mean Earth radius; segment distances use
/// the quantized archive coordinates so they are exactly re-derivable from
/// the archive itself.
const EARTH_RADIUS_METERS: f64 = 6_371_000.0;
const METERS_PER_KILOMETER: f64 = 1_000.0;
const SECONDS_PER_HOUR: f64 = 3_600.0;
const MILLISECONDS_PER_SECOND: f64 = 1_000.0;

/// Compiler-side failure: parsing, I/O, or graph overflow.
#[derive(Debug)]
pub enum CompilerError {
    /// Reading the input PBF failed.
    InputReadFailure(io::Error),
    /// The PBF parser rejected the extract.
    OsmParsingFailure(String),
    /// The extract exceeded this minimal compiler's addressable size.
    GraphCapacityExceeded(String),
}

impl std::fmt::Display for CompilerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InputReadFailure(error) => {
                write!(formatter, "input read failed: {error}")
            }
            Self::OsmParsingFailure(detail) => {
                write!(formatter, "OSM PBF parsing failed: {detail}")
            }
            Self::GraphCapacityExceeded(detail) => {
                write!(
                    formatter,
                    "graph exceeds this compiler's capacity: {detail}"
                )
            }
        }
    }
}

impl std::error::Error for CompilerError {}

/// Compiles an OpenStreetMap PBF extract into deterministic archive bytes.
///
/// # Errors
/// See [`CompilerError`]; note archive-side validation failures surface as
/// `geospatial_types::SDKError` only on the read path ([`crate::archive`]).
pub fn compile_osm_extract_bytes(osm_pbf_bytes: &[u8]) -> Result<Vec<u8>, CompilerError> {
    let element_reader = ElementReader::new(Cursor::new(osm_pbf_bytes.to_vec()));
    compile_from_element_reader(element_reader)
}

/// Reads a PBF file from disk and compiles it.
///
/// # Errors
/// See [`CompilerError`].
pub fn compile_osm_extract_file(input_path: &std::path::Path) -> Result<Vec<u8>, CompilerError> {
    let osm_pbf_bytes = std::fs::read(input_path).map_err(CompilerError::InputReadFailure)?;
    compile_osm_extract_bytes(&osm_pbf_bytes)
}

/// Collected OSM facts from the single sequential pass.
#[derive(Debug, Default)]
struct CollectedElements {
    node_positions: BTreeMap<i64, (f64, f64)>,
    drivable_ways: Vec<CollectedWay>,
    turn_restriction_candidates: Vec<TurnRestrictionCandidate>,
}

/// One drivable way after profile evaluation.
#[derive(Debug)]
struct CollectedWay {
    way_osm_id: i64,
    node_osm_ids: Vec<i64>,
    road_class: crate::archive::RoadClass,
    speed_kilometers_per_hour: u16,
    travel_direction: TravelDirection,
}

/// One well-formed `type=restriction` relation that maps to a known
/// [`RestrictionKind`].
#[derive(Debug)]
struct TurnRestrictionCandidate {
    relation_osm_id: i64,
    from_way_osm_id: i64,
    via_node_osm_id: i64,
    to_way_osm_id: i64,
    restriction_kind: RestrictionKind,
}

fn compile_from_element_reader(
    element_reader: ElementReader<Cursor<Vec<u8>>>,
) -> Result<Vec<u8>, CompilerError> {
    let mut collected_elements = CollectedElements::default();
    element_reader
        .for_each(|element| collect_element(element, &mut collected_elements))
        .map_err(|pbf_error| CompilerError::OsmParsingFailure(pbf_error.to_string()))?;
    build_archive(collected_elements)
}

/// Applies the [`crate::profile`] rules to one OSM element and collects it.
fn collect_element(element: Element<'_>, collected_elements: &mut CollectedElements) {
    match element {
        Element::Node(node) => {
            collected_elements
                .node_positions
                .insert(node.id(), (node.lat(), node.lon()));
        }
        Element::DenseNode(node) => {
            collected_elements
                .node_positions
                .insert(node.id(), (node.lat(), node.lon()));
        }
        Element::Way(way) => {
            if let Some(collected_way) = collect_way(way.id(), way.refs().collect(), way.tags()) {
                collected_elements.drivable_ways.push(collected_way);
            }
        }
        Element::Relation(relation) => {
            if let Some(candidate) =
                collect_restriction_candidate(relation.id(), relation.tags(), relation.members())
            {
                collected_elements
                    .turn_restriction_candidates
                    .push(candidate);
            }
        }
    }
}

/// Way tag snapshot the profile rules evaluate against.
struct WayTagSummary {
    highway: Option<String>,
    oneway: Option<String>,
    access: Option<String>,
    maxspeed: Option<String>,
}

fn collect_way_summary<'a>(tags: impl Iterator<Item = (&'a str, &'a str)>) -> WayTagSummary {
    let mut summary = WayTagSummary {
        highway: None,
        oneway: None,
        access: None,
        maxspeed: None,
    };
    for (tag_key, tag_value) in tags {
        match tag_key {
            "highway" if summary.highway.is_none() => summary.highway = Some(tag_value.to_string()),
            "oneway" if summary.oneway.is_none() => summary.oneway = Some(tag_value.to_string()),
            "access" if summary.access.is_none() => summary.access = Some(tag_value.to_string()),
            "maxspeed" if summary.maxspeed.is_none() => {
                summary.maxspeed = Some(tag_value.to_string())
            }
            _ => {}
        }
    }
    summary
}

fn collect_way<'a>(
    way_osm_id: i64,
    node_osm_ids: Vec<i64>,
    tags: impl Iterator<Item = (&'a str, &'a str)>,
) -> Option<CollectedWay> {
    let way_tag_summary = collect_way_summary(tags);
    if is_blocked_by_access(way_tag_summary.access.as_deref()) {
        return None;
    }
    let (road_class, default_speed_kilometers_per_hour) =
        road_class_and_default_speed_for_highway(way_tag_summary.highway.as_deref()?)?;
    let speed_kilometers_per_hour =
        posted_speed_override_kilometers_per_hour(way_tag_summary.maxspeed.as_deref())
            .unwrap_or(default_speed_kilometers_per_hour);
    Some(CollectedWay {
        way_osm_id,
        node_osm_ids,
        road_class,
        speed_kilometers_per_hour,
        travel_direction: travel_direction_for_oneway_tag(way_tag_summary.oneway.as_deref()),
    })
}

/// Relation member snapshot: role name and referenced element id.
struct RelationMember {
    role: String,
    member_type: osmpbf::RelMemberType,
    member_osm_id: i64,
}

fn collect_restriction_candidate<'a>(
    relation_osm_id: i64,
    tags: impl Iterator<Item = (&'a str, &'a str)>,
    members: impl Iterator<Item = osmpbf::RelMember<'a>>,
) -> Option<TurnRestrictionCandidate> {
    let mut relation_is_restriction = false;
    let mut restriction_value: Option<&str> = None;
    for (tag_key, tag_value) in tags {
        if tag_key == "type" && tag_value == "restriction" {
            relation_is_restriction = true;
        }
        if tag_key == "restriction" && restriction_value.is_none() {
            restriction_value = Some(tag_value);
        }
    }
    if !relation_is_restriction {
        return None;
    }
    let restriction_kind = restriction_kind_for_openstreetmap_value(restriction_value?)?;
    let collected_members: Vec<RelationMember> = members
        .filter_map(|member| {
            let role = member.role().ok()?.to_string();
            Some(RelationMember {
                role,
                member_type: member.member_type,
                member_osm_id: member.member_id,
            })
        })
        .collect();
    if collected_members.len() != 3 {
        return None;
    }
    let from_way_osm_id =
        single_member_osm_id(&collected_members, "from", osmpbf::RelMemberType::Way)?;
    let via_node_osm_id =
        single_member_osm_id(&collected_members, "via", osmpbf::RelMemberType::Node)?;
    let to_way_osm_id = single_member_osm_id(&collected_members, "to", osmpbf::RelMemberType::Way)?;
    Some(TurnRestrictionCandidate {
        relation_osm_id,
        from_way_osm_id,
        via_node_osm_id,
        to_way_osm_id,
        restriction_kind,
    })
}

/// The single member id for one required role and element type, or `None`
/// for malformed relations (missing role, duplicated role, wrong type).
fn single_member_osm_id(
    collected_members: &[RelationMember],
    required_role: &str,
    required_member_type: osmpbf::RelMemberType,
) -> Option<i64> {
    matching_members(collected_members, required_role, required_member_type)
        .next()
        .map(|member| member.member_osm_id)
}

fn matching_members<'a>(
    collected_members: &'a [RelationMember],
    required_role: &str,
    required_member_type: osmpbf::RelMemberType,
) -> impl Iterator<Item = &'a RelationMember> {
    collected_members.iter().filter(move |member| {
        member.role == required_role && member.member_type == required_member_type
    })
}

// ---------------------------------------------------------------------------
// Topology build
// ---------------------------------------------------------------------------

/// One edge before final ordering, with its originating way and OSM node
/// ids for tie-breaks and restriction resolution.
struct EdgeCandidate {
    edge_record: EdgeRecord,
    way_osm_id: i64,
    source_node_osm_id: i64,
    target_node_osm_id: i64,
}

/// Per-way record of emitted edges for restriction resolution.
struct WayEdgeReference {
    way_osm_id: i64,
    edge_index: u32,
    source_node_osm_id: i64,
    target_node_osm_id: i64,
}

fn build_archive(collected_elements: CollectedElements) -> Result<Vec<u8>, CompilerError> {
    let CollectedElements {
        node_positions,
        mut drivable_ways,
        mut turn_restriction_candidates,
    } = collected_elements;

    drivable_ways.sort_by_key(|way| way.way_osm_id);
    turn_restriction_candidates.sort_by_key(|candidate| candidate.relation_osm_id);

    let referenced_node_osm_ids: HashSet<i64> = drivable_ways
        .iter()
        .flat_map(|way| way.node_osm_ids.iter().copied())
        .collect();
    let node_index_by_osm_id: HashMap<i64, u32> = node_positions
        .iter()
        .filter(|(node_osm_id, _)| referenced_node_osm_ids.contains(node_osm_id))
        .enumerate()
        .map(|(node_index, (node_osm_id, _))| (*node_osm_id, node_index as u32))
        .collect();
    let node_records: Vec<NodeRecord> = node_positions
        .iter()
        .filter(|(node_osm_id, _)| node_index_by_osm_id.contains_key(*node_osm_id))
        .map(|(_, &(latitude_degrees, longitude_degrees))| {
            NodeRecord::from_geographic_degrees(latitude_degrees, longitude_degrees)
        })
        .collect();

    let mut edge_candidates: Vec<EdgeCandidate> = Vec::new();
    let mut way_edge_references: Vec<WayEdgeReference> = Vec::new();
    for way in &drivable_ways {
        let resolved_node_indices: Option<Vec<(i64, u32)>> = way
            .node_osm_ids
            .iter()
            .map(|node_osm_id| {
                node_index_by_osm_id
                    .get(node_osm_id)
                    .map(|&index| (*node_osm_id, index))
            })
            .collect();
        let Some(resolved_node_indices) = resolved_node_indices else {
            // A drivable way referencing a node missing from the extract:
            // skip the way rather than emit dangling edges.
            continue;
        };
        for segment in resolved_node_indices.windows(2) {
            let (from_node_osm_id, from_node_index) = segment[0];
            let (to_node_osm_id, to_node_index) = segment[1];
            if from_node_osm_id == to_node_osm_id {
                // Zero-length self loop: not a routing edge in this slice.
                continue;
            }
            let from_node_record = &node_records[from_node_index as usize];
            let to_node_record = &node_records[to_node_index as usize];
            let distance_meters = haversine_distance_meters(from_node_record, to_node_record);
            for (emits_forward_direction, (source_node_index, target_node_index)) in
                emit_directions(way.travel_direction, from_node_index, to_node_index)
            {
                let edge_record = EdgeRecord::new_directed(
                    source_node_index,
                    target_node_index,
                    distance_meters,
                    duration_milliseconds_for(distance_meters, way.speed_kilometers_per_hour),
                    edge_properties_for_direction(
                        way.road_class,
                        way.speed_kilometers_per_hour,
                        emits_forward_direction,
                    ),
                );
                edge_candidates.push(EdgeCandidate {
                    edge_record,
                    way_osm_id: way.way_osm_id,
                    source_node_osm_id: from_node_osm_id,
                    target_node_osm_id: to_node_osm_id,
                });
            }
        }
    }

    // Canonical edge order (see module docs).
    edge_candidates.sort_by_key(|edge_candidate| {
        (
            edge_candidate.edge_record.source_node_index(),
            edge_candidate.edge_record.target_node_index(),
            edge_candidate.way_osm_id,
        )
    });
    let edge_records: Vec<EdgeRecord> = edge_candidates
        .iter()
        .map(|edge_candidate| edge_candidate.edge_record)
        .collect();
    for (edge_index, edge_candidate) in edge_candidates.iter().enumerate() {
        way_edge_references.push(WayEdgeReference {
            way_osm_id: edge_candidate.way_osm_id,
            edge_index: edge_index as u32,
            source_node_osm_id: edge_candidate.source_node_osm_id,
            target_node_osm_id: edge_candidate.target_node_osm_id,
        });
    }

    // Resolve turn restrictions onto directed edges; unresolved candidates
    // (referencing undrivable or missing ways) are skipped, never fail.
    let mut resolved_restrictions: Vec<(TurnRestrictionRecord, &'static str)> = Vec::new();
    for candidate in &turn_restriction_candidates {
        let Some(via_node_index) = node_index_by_osm_id
            .get(&candidate.via_node_osm_id)
            .copied()
        else {
            continue;
        };
        let Some(from_edge_index) = first_way_edge_where(
            &way_edge_references,
            candidate.from_way_osm_id,
            |reference| reference.target_node_osm_id == candidate.via_node_osm_id,
        ) else {
            continue;
        };
        let Some(to_edge_index) =
            first_way_edge_where(&way_edge_references, candidate.to_way_osm_id, |reference| {
                reference.source_node_osm_id == candidate.via_node_osm_id
            })
        else {
            continue;
        };
        resolved_restrictions.push((
            TurnRestrictionRecord::new(
                from_edge_index,
                via_node_index,
                to_edge_index,
                candidate.restriction_kind,
            ),
            instruction_phrase_for_restriction_kind(candidate.restriction_kind),
        ));
    }
    resolved_restrictions.sort_by_key(|(restriction_record, _)| {
        (
            restriction_record.from_edge_index(),
            restriction_record.via_node_index(),
            restriction_record.to_edge_index(),
        )
    });
    let turn_restrictions: Vec<TurnRestrictionRecord> = resolved_restrictions
        .iter()
        .map(|(restriction_record, _)| *restriction_record)
        .collect();
    let mut instruction_strings = Vec::<u8>::new();
    let mut instruction_lookup_indices = Vec::<u32>::new();
    for (_, instruction_phrase) in &resolved_restrictions {
        instruction_lookup_indices.push(instruction_strings.len() as u32);
        instruction_strings.extend_from_slice(instruction_phrase.as_bytes());
    }

    check_index_capacity(node_records.len(), edge_records.len())?;

    // Lane guidance stays empty by refusal, not by omission: OSM `turn:lanes`
    // carries per-lane *permitted maneuvers* ("through|through|right"), while
    // `LaneGuidanceRecord` can hold only an edge lane count — emitting counts
    // would either silently drop the turn information the tags express or
    // fabricate a recommended lane. See `LaneGuidanceRecord`' documentation;
    // the resource keeps its slot in the canonical resource order so the
    // archive layout stays byte-stable for the future lane-geometry slice.
    let archive_builder = OfflineRoutingNetworkGraphBuilder::new()
        .with_nodes(node_records)
        .with_edges(edge_records)
        .with_lane_guidances(Vec::<LaneGuidanceRecord>::new())
        .with_instruction_strings(instruction_strings)
        .with_instruction_lookup_indices(instruction_lookup_indices)
        .with_turn_restrictions(turn_restrictions);
    archive_builder
        .encode_deterministic()
        .map_err(|archive_error| CompilerError::OsmParsingFailure(archive_error.to_string()))
}

/// Emits the directed edges for one way segment in the way's allowed
/// directions: `(covers forward direction?, (source index, target index))`.
fn emit_directions(
    travel_direction: TravelDirection,
    from_node_index: u32,
    to_node_index: u32,
) -> Vec<(bool, (u32, u32))> {
    match travel_direction {
        TravelDirection::ForwardOnly => vec![(true, (from_node_index, to_node_index))],
        TravelDirection::BackwardOnly => vec![(false, (to_node_index, from_node_index))],
        TravelDirection::Both => vec![
            (true, (from_node_index, to_node_index)),
            (false, (to_node_index, from_node_index)),
        ],
    }
}

fn first_way_edge_where(
    way_edge_references: &[WayEdgeReference],
    way_osm_id: i64,
    matches: impl Fn(&WayEdgeReference) -> bool,
) -> Option<u32> {
    way_edge_references
        .iter()
        .find(|reference| reference.way_osm_id == way_osm_id && matches(reference))
        .map(|reference| reference.edge_index)
}

fn check_index_capacity(node_count: usize, edge_count: usize) -> Result<(), CompilerError> {
    if node_count > u32::MAX as usize || edge_count > u32::MAX as usize {
        return Err(CompilerError::GraphCapacityExceeded(format!(
            "{node_count} nodes / {edge_count} edges exceed the u32 index space"
        )));
    }
    Ok(())
}

/// Haversine distance between two quantized node records, rounded to whole
/// meters.
fn haversine_distance_meters(from: &NodeRecord, to: &NodeRecord) -> u32 {
    let from_latitude = from.latitude_degrees().to_radians();
    let to_latitude = to.latitude_degrees().to_radians();
    let delta_latitude = (to.latitude_degrees() - from.latitude_degrees()).to_radians();
    let delta_longitude = (to.longitude_degrees() - from.longitude_degrees()).to_radians();
    let haversine_argument = (delta_latitude / 2.0).sin().powi(2)
        + from_latitude.cos() * to_latitude.cos() * (delta_longitude / 2.0).sin().powi(2);
    let distance_meters = 2.0 * EARTH_RADIUS_METERS * haversine_argument.sqrt().asin();
    distance_meters.round() as u32
}

/// Traversal duration from distance and stored speed, rounded to whole
/// milliseconds. The stored (quantized) speed keeps the archive
/// self-consistent: distances and durations re-derive from it exactly.
fn duration_milliseconds_for(distance_meters: u32, speed_kilometers_per_hour: u16) -> u32 {
    let speed_meters_per_second = f64::from(quantized_speed(speed_kilometers_per_hour))
        * METERS_PER_KILOMETER
        / SECONDS_PER_HOUR;
    let duration_seconds = f64::from(distance_meters) / speed_meters_per_second;
    (duration_seconds * MILLISECONDS_PER_SECOND).round() as u32
}

/// The speed value the archive stores for the given profile speed.
fn quantized_speed(speed_kilometers_per_hour: u16) -> u16 {
    speed_cells_from_kilometers_per_hour(speed_kilometers_per_hour)
        * crate::archive::SPEED_QUANTUM_KILOMETERS_PER_HOUR
}
