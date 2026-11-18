//! Deterministic synthetic graphs for measuring the offline routing engine.
//!
//! The generators write archive bytes with the same deterministic builder the
//! graph compiler uses, read them back through the same validating reader and
//! index them with [`LoadedRoutingGraph::new`]. Measurements therefore run
//! over the production graph structures, on reproducible inputs, with no
//! network access.
//!
//! Cost columns are chosen so both strategies' heuristics stay admissible:
//! every distance is at least the endpoints' straight-line distance, and every
//! duration is `distance / speed` at a speed below the packed column's
//! saturation limit.
//!
//! These functions support performance measurement only. Production routing
//! always goes through [`crate::offline_routing::a_star_search`].

use rustic_maps_offline_graph_archive::archive::{
    EdgeProperties, EdgeRecord, INSTRUCTION_ABSENT_INDEX, NodeRecord, OfflineRoutingNetworkGraph,
    OfflineRoutingNetworkGraphBuilder, RoadClass,
};

use geospatial_types::{GeographicCoordinate, SDKError};

use crate::offline_routing::LoadedRoutingGraph;

// ---------------------------------------------------------------------------
// Benchmark shape constants (the recorded benchmark suite's sweep)
// ---------------------------------------------------------------------------

/// Grid graph side lengths the grid benchmark sweeps, as squares of
/// `side * side` nodes.
///
/// The sizes are small enough for a quick run and large enough that the
/// settled-set and wall-clock growth of Dijkstra against A*'s corridor is
/// visible at the top size.
pub const BENCHMARK_GRID_SIDE_LENGTHS: [u32; 3] = [16, 32, 64];

/// Random-spanning graph node counts the random-spanning benchmark sweeps.
///
/// The spanning topology, branching corridors plus shortcut arcs instead of a
/// lattice, gives Dijkstra a second, differently shaped case.
pub const BENCHMARK_RANDOM_SPANNING_NODE_COUNTS: [usize; 3] = [512, 2048, 8192];

/// Fixed seed of the random-spanning generator.
///
/// The generated graphs are part of a recorded baseline, so every run
/// regenerates byte-identical archives. `BenchmarkRng` is a plain SplitMix64
/// generator.
pub const BENCHMARK_RANDOM_SPANNING_SEED: u64 = 0x5EED_00A5_0F7E_BE1A;

/// Node count of the synthetic graph used to time loading: the validating read
/// and the routing-index build.
///
/// The graph carries more nodes and edges than the small reference graph, so
/// load regressions are visible.
pub const BENCHMARK_SPANNING_LOAD_BENCH_NODE_COUNT: usize = 2048;

// ---------------------------------------------------------------------------
// Synthetic graph geometry and cost constants
// ---------------------------------------------------------------------------

/// Grid and lattice spacing in degrees: ~55.6 m along a meridian, i.e. an
/// urban block length the node lattice is placed on.
const BENCHMARK_BLOCK_DEGREES: f64 = 0.0005;

/// The street classes and speeds of the synthetic non-arterial arcs, as
/// `(road class, speed km/h, relative weight)`: a realistic street mix where
/// most travel happens on slow local streets (the shape contrast the
/// `Fastest` heuristic prunes on). Speeds are whole archive speed cells and
/// stay below the packed column's saturation limit, keeping the heuristic
/// admissible.
const BENCHMARK_ARC_CLASSES: [(RoadClass, u16, u64); 7] = [
    (RoadClass::Motorway, 130, 2),
    (RoadClass::Trunk, 110, 3),
    (RoadClass::Primary, 85, 6),
    (RoadClass::Secondary, 60, 12),
    (RoadClass::Tertiary, 50, 12),
    (RoadClass::Unclassified, 40, 25),
    (RoadClass::Residential, 30, 40),
];

/// The arterial overlay of the grid generator: every stride-th street (each
/// direction the street runs) carries this fast class/speed — the metro's
/// major-road spine, the corridor shape A*'s heuristic is for.
const BENCHMARK_ARTERIAL_STRIDE: u32 = 8;
/// The arterial arc's class/speed pair (see the street-mix table above).
const BENCHMARK_ARTERIAL_CLASS: (RoadClass, u16) = (RoadClass::Trunk, 110);

/// The fast motorway-override arc of the benchmark generators (the sparse
/// fast overlay of the spanning family): at the packed speed column's
/// saturation limit, so the `Fastest` heuristic is exactly tight on these
/// arcs.
const BENCHMARK_MOTORWAY_ARC: (RoadClass, u16) = (RoadClass::Motorway, 155);

/// How many of the spanning family's shortcut arcs per node are the fast
/// motorway override, in whole percent of `node_count` (the sparse fast
/// overlay; the rest of the shortcut arcs draw the street mix).
const BENCHMARK_MOTORWAY_SHORTCUT_PERCENT: u64 = 6;

/// Width (in lattice-neighbor steps) of the random-spanning generator's
/// edge window: a node's spanning edge reaches a random node among the few
/// nearest earlier cells, so edges stay geometrically local like a road
/// network's do.
const BENCHMARK_SPANNING_PARENT_WINDOW: usize = 8;

/// One-way shortcut arcs per node added on top of the random spanning edges
/// (the occasional long detour arc that breaks the pure corridor).
const BENCHMARK_SHORTCUT_ARCS_PER_NODE: usize = 2;

/// Sideways jitter of spanning-lattice node positions, as a fraction of the
/// block spacing (keeps node coordinates distinguishable without overlap).
const BENCHMARK_SPANNING_JITTER_FRACTION: f64 = 0.4;

/// Jitter resolution: the generator's jitter draw is `next_below` of this,
/// centered to a signed fraction of [`BENCHMARK_SPANNING_JITTER_FRACTION`].
const BENCHMARK_SPANNING_JITTER_STEPS: u64 = 1_001;

/// Whole-meter inflation the generator may add over an arc's straight-line
/// distance: the distance column's own granularity, so the draw is fully
/// visible in the archive while the column never drops below the
/// admissible straight-line bound. The range is deliberately wide (winding
/// streets travel far more than their straight-line length) so the recorded
/// graphs' optimal paths are *unique* per endpoint pair — the premise of
/// the A*/reference chain-equality assertion (an equal-total alternative
/// optimum would let the two engines' settlement orders diverge on a tie).
/// The recorded (seed, size) set is verified tie-free by the equality test
/// itself; growing either means re-verifying.
const BENCHMARK_DISTANCE_INFLATION_METERS: u64 = 128;

/// Unit conversions for the duration column (`distance / speed`), matching
/// `offline_routing`'s columns (milliseconds, seconds, km/h).
const BENCHMARK_MILLISECONDS_PER_SECOND: f64 = 1_000.0;
/// Unit conversion: seconds per hour.
const BENCHMARK_SECONDS_PER_HOUR: f64 = 3_600.0;
/// Unit conversion: meters per kilometer.
const BENCHMARK_METERS_PER_KILOMETER: f64 = 1_000.0;

// ---------------------------------------------------------------------------
// Deterministic PRNG (SplitMix64, no external dependency)
// ---------------------------------------------------------------------------

/// SplitMix64's golden-gamma increment; the standard `0x9E3779B97F4A7C15`.
const SPLITMIX64_GOLDEN_GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

/// A plain SplitMix64 PRNG: the synthetic benchmark graphs must be
/// byte-identical on every machine and every run, so the generator is
/// seeded in-source instead of pulling in a random-number dependency.
#[derive(Debug)]
struct BenchmarkRng {
    state: u64,
}

impl BenchmarkRng {
    /// A PRNG seeded from the (fixed) generator seed.
    const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// The next SplitMix64 output.
    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(SPLITMIX64_GOLDEN_GAMMA);
        let mut mixed = self.state;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        mixed ^ (mixed >> 31)
    }

    /// The next output reduced below `bound` (the tiny modulo bias of a
    /// benchmark generator is irrelevant against a fixed recorded graph).
    fn next_below(&mut self, bound: u64) -> u64 {
        self.next_u64() % bound
    }
}

// ---------------------------------------------------------------------------
// Loaded-graph builders (the full production path: encode → validate → index)
// ---------------------------------------------------------------------------

/// Builds the grid benchmark graph of `side_length * side_length` street
/// nodes: two-way blocks with an arterial street every
/// `BENCHMARK_ARTERIAL_STRIDE` rows and columns.
///
/// The graph is built through the full production path: deterministic archive
/// bytes, validating reader, then routing index.
///
/// # Errors
/// Returns [`SDKError::OfflineRoutingFailure`] only on a generator bug; a
/// correctly generated graph always validates.
pub fn benchmark_grid_loaded_graph(side_length: u32) -> Result<LoadedRoutingGraph, SDKError> {
    loaded_graph_from_archive_bytes(&benchmark_grid_archive_bytes(side_length)?)
}

/// Builds the random-spanning benchmark graph of `node_count` nodes:
/// geometrically local spanning edges plus one-way shortcut arcs.
///
/// The graph is built through the full production path (see
/// [`benchmark_grid_loaded_graph`]).
///
/// # Errors
/// Returns [`SDKError::OfflineRoutingFailure`] only on a generator bug; a
/// correctly generated graph always validates.
pub fn benchmark_random_spanning_loaded_graph(
    node_count: usize,
    seed: u64,
) -> Result<LoadedRoutingGraph, SDKError> {
    loaded_graph_from_archive_bytes(&benchmark_random_spanning_archive_bytes(node_count, seed)?)
}

/// The canonical query pair of a `node_count`-node benchmark graph: the two
/// opposing index corners.
///
/// This is the worst case for a settled-set comparison: Dijkstra settles
/// nearly the whole graph, while the haversine corridor does not.
#[must_use]
pub fn benchmark_corner_query_pair(node_count: u32) -> (u32, u32) {
    (0, node_count - 1)
}

/// Loads a benchmark graph exactly the way the routing provider loads a
/// compiled archive: validating in-memory reader, then the routing index.
fn loaded_graph_from_archive_bytes(archive_bytes: &[u8]) -> Result<LoadedRoutingGraph, SDKError> {
    let network_graph = OfflineRoutingNetworkGraph::from_bytes(archive_bytes)?;
    Ok(LoadedRoutingGraph::new(network_graph))
}

// ---------------------------------------------------------------------------
// Archive-byte generators
// ---------------------------------------------------------------------------

/// Writes the grid graph's deterministic archive bytes.
///
/// # Errors
/// Returns [`SDKError::OfflineRoutingFailure`] only on a generator bug; indices
/// are in range by construction.
pub fn benchmark_grid_archive_bytes(side_length: u32) -> Result<Vec<u8>, SDKError> {
    let side_as_usize = side_length as usize;
    let node_count = side_as_usize * side_as_usize;
    // The seed folds the shape in, so every size is a fixed recorded graph.
    let mut benchmark_rng =
        BenchmarkRng::new(BENCHMARK_RANDOM_SPANNING_SEED ^ u64::from(side_length));

    let mut node_records = Vec::with_capacity(node_count);
    for row in 0..side_as_usize {
        for column in 0..side_as_usize {
            node_records.push(NodeRecord::from_geographic_degrees(
                f64::from(row as u32) * BENCHMARK_BLOCK_DEGREES,
                f64::from(column as u32) * BENCHMARK_BLOCK_DEGREES,
            ));
        }
    }

    let mut edge_records = Vec::new();
    for row in 0..side_as_usize {
        for column in 0..side_as_usize {
            let node_index = row * side_as_usize + column;
            if column + 1 < side_as_usize {
                // An eastbound block: arterial when its street row is one
                // of the stride-spaced artery rows.
                let forced_class_and_speed = ((row as u32)
                    .is_multiple_of(BENCHMARK_ARTERIAL_STRIDE))
                .then_some(BENCHMARK_ARTERIAL_CLASS);
                push_two_way_arc(
                    &mut edge_records,
                    node_index,
                    node_index + 1,
                    &node_records,
                    &mut benchmark_rng,
                    forced_class_and_speed,
                );
            }
            if row + 1 < side_as_usize {
                // A southbound block: arterial when its street column is
                // one of the stride-spaced artery columns.
                let forced_class_and_speed = ((column as u32)
                    .is_multiple_of(BENCHMARK_ARTERIAL_STRIDE))
                .then_some(BENCHMARK_ARTERIAL_CLASS);
                push_two_way_arc(
                    &mut edge_records,
                    node_index,
                    node_index + side_as_usize,
                    &node_records,
                    &mut benchmark_rng,
                    forced_class_and_speed,
                );
            }
        }
    }

    encoded_archive_bytes(node_records, edge_records)
}

/// Writes the random-spanning graph's deterministic archive bytes.
///
/// # Errors
/// Returns [`SDKError::OfflineRoutingFailure`] only on a generator bug; indices
/// are in range by construction.
pub fn benchmark_random_spanning_archive_bytes(
    node_count: usize,
    seed: u64,
) -> Result<Vec<u8>, SDKError> {
    let mut benchmark_rng = BenchmarkRng::new(seed);
    let lattice_side = f64::from(node_count as u32).sqrt().ceil() as usize;

    // Jittered lattice positions: cell placement with a per-node sideways
    // offset, so straight-line distances vary but stay ordered.
    let mut node_records = Vec::with_capacity(node_count);
    for node_index in 0..node_count {
        let (row, column) = (node_index / lattice_side, node_index % lattice_side);
        node_records.push(NodeRecord::from_geographic_degrees(
            f64::from(row as u32) * BENCHMARK_BLOCK_DEGREES + jitter_degrees(&mut benchmark_rng),
            f64::from(column as u32) * BENCHMARK_BLOCK_DEGREES + jitter_degrees(&mut benchmark_rng),
        ));
    }

    let mut edge_records = Vec::new();
    // The spanning edges, both ways: directed reachability of the whole
    // graph, on geometrically local random edges (branching corridors
    // rather than a lattice).
    for node_index in 1..node_count {
        let parent_window = BENCHMARK_SPANNING_PARENT_WINDOW.min(node_index);
        let parent_index = node_index - 1 - benchmark_rng.next_below(parent_window as u64) as usize;
        push_two_way_arc(
            &mut edge_records,
            parent_index,
            node_index,
            &node_records,
            &mut benchmark_rng,
            None,
        );
    }
    // One-way shortcut arcs detouring across the lattice: mostly the street
    // mix, plus the sparse fast motorway overlay (the metro's few fast
    // corridors across a slow local network — the cost anisotropy A*'s
    // heuristic prunes on).
    let shortcut_arc_count = node_count * BENCHMARK_SHORTCUT_ARCS_PER_NODE;
    let motorway_arc_count = node_count * (BENCHMARK_MOTORWAY_SHORTCUT_PERCENT as usize) / 100;
    for arc_position in 0..shortcut_arc_count {
        let from_index = benchmark_rng.next_below(node_count as u64) as usize;
        let to_index = benchmark_rng.next_below(node_count as u64) as usize;
        if from_index == to_index {
            continue;
        }
        let motorway_arc = arc_position < motorway_arc_count;
        push_single_arc(
            &mut edge_records,
            from_index,
            to_index,
            &node_records,
            &mut benchmark_rng,
            motorway_arc.then_some(BENCHMARK_MOTORWAY_ARC),
        );
    }

    encoded_archive_bytes(node_records, edge_records)
}

/// The sideways jitter of a spanning-lattice node, in signed degrees.
fn jitter_degrees(benchmark_rng: &mut BenchmarkRng) -> f64 {
    let centered_steps = benchmark_rng.next_below(BENCHMARK_SPANNING_JITTER_STEPS) as f64
        - f64::from(BENCHMARK_SPANNING_JITTER_STEPS as u32) / 2.0;
    centered_steps / f64::from(BENCHMARK_SPANNING_JITTER_STEPS as u32)
        * BENCHMARK_SPANNING_JITTER_FRACTION
        * BENCHMARK_BLOCK_DEGREES
}

/// Appends both directions of one two-way arc as their own edge records
/// (the archive's stored-pair contract: every traversable direction is its
/// own `(source, target)` record).
fn push_two_way_arc(
    edge_records: &mut Vec<EdgeRecord>,
    first_node_index: usize,
    second_node_index: usize,
    node_records: &[NodeRecord],
    benchmark_rng: &mut BenchmarkRng,
    forced_class_and_speed: Option<(RoadClass, u16)>,
) {
    push_single_arc(
        edge_records,
        first_node_index,
        second_node_index,
        node_records,
        benchmark_rng,
        forced_class_and_speed,
    );
    push_single_arc(
        edge_records,
        second_node_index,
        first_node_index,
        node_records,
        benchmark_rng,
        forced_class_and_speed,
    );
}

/// Appends one directed arc record with per-arc varied, column-honest costs.
fn push_single_arc(
    edge_records: &mut Vec<EdgeRecord>,
    source_node_index: usize,
    target_node_index: usize,
    node_records: &[NodeRecord],
    benchmark_rng: &mut BenchmarkRng,
    forced_class_and_speed: Option<(RoadClass, u16)>,
) {
    let straight_line_meters = straight_line_meters(
        &node_records[source_node_index],
        &node_records[target_node_index],
    );
    let (road_class, speed_kilometers_per_hour) =
        forced_class_and_speed.unwrap_or_else(|| benchmark_street_class_and_speed(benchmark_rng));
    let (distance_meters, duration_milliseconds) = benchmark_cost_columns(
        straight_line_meters,
        speed_kilometers_per_hour,
        benchmark_rng,
    );
    edge_records.push(EdgeRecord::new_directed(
        source_node_index as u32,
        target_node_index as u32,
        distance_meters,
        duration_milliseconds,
        EdgeProperties {
            road_class,
            speed_kilometers_per_hour,
            forward_allowed: true,
            backward_allowed: false,
        },
    ));
}

/// The class/speed pair of a non-arterial synthetic arc, drawn by the
/// street-mix table's relative weights (arbitrary but fixed per arc; the
/// class column feeds maneuver runs, not the search costs).
fn benchmark_street_class_and_speed(benchmark_rng: &mut BenchmarkRng) -> (RoadClass, u16) {
    let mut class_draw = benchmark_rng.next_below(100);
    for &(road_class, speed_kilometers_per_hour, weight) in &BENCHMARK_ARC_CLASSES {
        if class_draw < weight {
            return (road_class, speed_kilometers_per_hour);
        }
        class_draw -= weight;
    }
    let (road_class, speed_kilometers_per_hour, _) =
        BENCHMARK_ARC_CLASSES[BENCHMARK_ARC_CLASSES.len() - 1];
    (road_class, speed_kilometers_per_hour)
}

/// The cost columns of one synthetic arc: distance inflated over the
/// straight line by a whole-percentage draw (the straight-line heuristic
/// stays admissible for the `Shortest` strategy), duration = distance over
/// speed (below the packed column's saturation limit, so the `Fastest`
/// heuristic stays admissible too).
fn benchmark_cost_columns(
    straight_line_meters: f64,
    speed_kilometers_per_hour: u16,
    benchmark_rng: &mut BenchmarkRng,
) -> (u32, u32) {
    let variation_meters = benchmark_rng.next_below(BENCHMARK_DISTANCE_INFLATION_METERS) as u32;
    let distance_meters = straight_line_meters.ceil() as u32 + variation_meters;
    let speed_meters_per_second = f64::from(speed_kilometers_per_hour)
        * BENCHMARK_METERS_PER_KILOMETER
        / BENCHMARK_SECONDS_PER_HOUR;
    let duration_milliseconds = (f64::from(distance_meters) / speed_meters_per_second
        * BENCHMARK_MILLISECONDS_PER_SECOND)
        .round() as u32;
    (distance_meters, duration_milliseconds)
}

/// Straight-line (great-circle) distance between two nodes' quantized
/// coordinates: the admissibility floor the distance column inflates over.
fn straight_line_meters(from_node: &NodeRecord, to_node: &NodeRecord) -> f64 {
    geographic_coordinate_of(from_node)
        .haversine_distance_meters(&geographic_coordinate_of(to_node))
}

/// A quantized node record as a `GeographicCoordinate` (always in range).
fn geographic_coordinate_of(node_record: &NodeRecord) -> GeographicCoordinate {
    GeographicCoordinate::from_degrees(
        node_record.latitude_degrees(),
        node_record.longitude_degrees(),
    )
    .expect("benchmark node coordinates are within the canonical ranges")
}

/// Encodes a generated archive through the same deterministic builder the
/// graph compiler uses, with the empty auxiliary resources a routing-only
/// archive carries.
fn encoded_archive_bytes(
    node_records: Vec<NodeRecord>,
    edge_records: Vec<EdgeRecord>,
) -> Result<Vec<u8>, SDKError> {
    OfflineRoutingNetworkGraphBuilder::new()
        .with_nodes(node_records)
        .with_edges(edge_records)
        .with_lane_guidances(Vec::new())
        .with_instruction_strings(Vec::new())
        .with_instruction_lookup_indices(vec![INSTRUCTION_ABSENT_INDEX; 0])
        .with_turn_restrictions(Vec::new())
        .encode_deterministic()
}

// ---------------------------------------------------------------------------
// Tests: benchmark-record equality + the coarse CI budget alarm
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use crate::offline_routing::{TraversalCost, a_star_search, dijkstra_reference_search};

    /// Test-scale graph shapes (smaller than the sweep constants, same
    /// generators, same seeds' construction rules).
    const TEST_GRID_SIDE_LENGTH: u32 = 12;
    const TEST_SPANNING_NODE_COUNT: usize = 256;
    const TEST_SECOND_SPANNING_NODE_COUNT: usize = 512;
    const TEST_THIRD_GRID_SIDE_LENGTH: u32 = 16;

    /// Both traversal strategies, so the chain/cost agreement covers both
    /// admissibility stories (time and distance).
    const BOTH_STRATEGIES: [TraversalCost; 2] =
        [TraversalCost::TimeSeconds, TraversalCost::DistanceMeters];

    /// The several deterministic query pairs asserted per graph: the
    /// opposing corners and two interior pairs across the index space.
    fn benchmark_query_pairs(node_count: u32) -> Vec<(u32, u32)> {
        vec![
            benchmark_corner_query_pair(node_count),
            (node_count / 4, node_count * 3 / 4),
            (node_count - 1, 0),
        ]
    }

    #[test]
    fn benchmark_graph_generation_is_byte_deterministic() {
        // Same seed, same bytes — the recorded baseline premise: the suite
        // measures the same graphs on every run and every runner.
        assert_eq!(
            benchmark_grid_archive_bytes(TEST_GRID_SIDE_LENGTH).expect("the grid graph encodes"),
            benchmark_grid_archive_bytes(TEST_GRID_SIDE_LENGTH).expect("the grid graph encodes"),
        );
        assert_eq!(
            benchmark_random_spanning_archive_bytes(
                TEST_SPANNING_NODE_COUNT,
                BENCHMARK_RANDOM_SPANNING_SEED
            )
            .expect("the spanning graph encodes"),
            benchmark_random_spanning_archive_bytes(
                TEST_SPANNING_NODE_COUNT,
                BENCHMARK_RANDOM_SPANNING_SEED
            )
            .expect("the spanning graph encodes"),
        );
    }

    #[test]
    fn astar_and_dijkstra_reference_agree_on_the_synthetic_benchmark_graphs() {
        // The result-equality half of the benchmark claim: over the same
        // loaded graphs, A* and the Dijkstra reference return the same
        // optimal chain at the same total cost (per-edge costs vary, so the
        // optimum is unique), and A* settles no more nodes than Dijkstra —
        // the measured-better-or-equal expansion sanity of the "A* beats
        // Dijkstra" claim (all deterministic, noise-free).
        let mut benchmark_graphs = Vec::new();
        for side_length in [TEST_GRID_SIDE_LENGTH, TEST_THIRD_GRID_SIDE_LENGTH] {
            benchmark_graphs.push((
                format!("grid_{side_length}"),
                benchmark_grid_loaded_graph(side_length).expect("the test grid graph loads"),
                side_length * side_length,
            ));
        }
        for node_count in [TEST_SPANNING_NODE_COUNT, TEST_SECOND_SPANNING_NODE_COUNT] {
            benchmark_graphs.push((
                format!("spanning_{node_count}"),
                benchmark_random_spanning_loaded_graph(node_count, BENCHMARK_RANDOM_SPANNING_SEED)
                    .expect("the test spanning graph loads"),
                node_count as u32,
            ));
        }

        for (graph_name, loaded_graph, node_count) in &benchmark_graphs {
            for (start_node_index, end_node_index) in benchmark_query_pairs(*node_count) {
                for traversal_cost in BOTH_STRATEGIES {
                    let astar_route = a_star_search(
                        loaded_graph,
                        start_node_index,
                        end_node_index,
                        traversal_cost,
                    )
                    .unwrap_or_else(|| {
                        panic!("{graph_name} connects {start_node_index} to {end_node_index}")
                    });
                    let reference_route = dijkstra_reference_search(
                        loaded_graph,
                        start_node_index,
                        end_node_index,
                        traversal_cost,
                    )
                    .unwrap_or_else(|| {
                        panic!("{graph_name} connects {start_node_index} to {end_node_index}")
                    });
                    assert_eq!(
                        astar_route.node_chain_indices, reference_route.node_chain_indices,
                        "{graph_name} {traversal_cost:?}: the searches agree on the optimal chain"
                    );
                    assert_eq!(
                        astar_route.total_cost, reference_route.total_cost,
                        "{graph_name} {traversal_cost:?}: the searches agree on the optimal cost"
                    );
                    assert!(
                        astar_route.settled_node_count <= reference_route.settled_node_count,
                        "{graph_name} {traversal_cost:?}: A* ({}) settles no more than \
                         Dijkstra ({})",
                        astar_route.settled_node_count,
                        reference_route.settled_node_count
                    );
                }
            }
        }
    }

    /// The coarse CI budget alarm: the asserted generous upper bound on the
    /// A* solve of the fixed benchmark graphs.
    ///
    /// Shared-runner wall clock is noisy, so this is deliberately NOT a
    /// sharp gate: the bound sits three orders of magnitude above the
    /// locally measured (sub-millisecond) solves and exists to catch
    /// *gross* regressions (a lost heuristic, an O(n²) relaxation) — the
    /// criterion numbers uploaded by the benchmark workflow are the
    /// recorded trend baseline, not a CI pass/fail. If this bound ever
    /// fires while nothing structural changed, it is runner noise; re-run
    /// before bisecting.
    const COARSE_BUDGET_ALARM_MILLISECONDS: u128 = 1_000;

    #[test]
    fn astar_stays_within_the_coarse_solve_budget_alarm_on_the_fixed_graphs() {
        let grid_graph = benchmark_grid_loaded_graph(64).expect("the fixed grid graph loads");
        let spanning_graph =
            benchmark_random_spanning_loaded_graph(8_192, BENCHMARK_RANDOM_SPANNING_SEED)
                .expect("the fixed spanning graph loads");
        let fixed_queries = [
            (
                "grid_64x64",
                &grid_graph,
                benchmark_corner_query_pair(64 * 64),
            ),
            (
                "spanning_8192",
                &spanning_graph,
                benchmark_corner_query_pair(8_192),
            ),
        ];

        for (graph_name, loaded_graph, (start_node_index, end_node_index)) in fixed_queries {
            let solve_started_at = std::time::Instant::now();
            let astar_route = a_star_search(
                loaded_graph,
                start_node_index,
                end_node_index,
                TraversalCost::TimeSeconds,
            )
            .unwrap_or_else(|| panic!("{graph_name} connects the fixed query pair"));
            let solve_elapsed_milliseconds = solve_started_at.elapsed().as_millis();
            assert!(
                solve_elapsed_milliseconds < COARSE_BUDGET_ALARM_MILLISECONDS,
                "{graph_name}: the A* solve took {solve_elapsed_milliseconds} ms, above the \
                 {} ms coarse budget alarm (a gross-regression signal — re-run before \
                 bisecting, shared-runner wall clock is noisy)",
                COARSE_BUDGET_ALARM_MILLISECONDS
            );
            // The alarm graph is also sanity-sized: the solve is fully
            // answered (non-empty chain ending at the requested terminal).
            assert_eq!(
                astar_route
                    .node_chain_indices
                    .last()
                    .expect("non-empty chain"),
                &end_node_index
            );
        }
    }
}
