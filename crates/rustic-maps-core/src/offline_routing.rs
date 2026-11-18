//! Offline route calculation over compiled routing graph archives.
//!
//! [`ArchiveOfflineRoutingProvider`] implements [`OfflineRoutingProvider`]
//! over a compiled routing graph archive. It loads the archive from a
//! memory-mapped file on native targets or from in-memory bytes on every
//! target, validates it with the archive reader, and builds the adjacency and
//! turn-restriction indexes once. Route requests are then answered with A*
//! under a haversine heuristic.
//!
//! # Targets
//!
//! The whole module compiles on every target; only graph loading differs.
//!
//! - [`OfflineGraphSource::Memory`] is available everywhere and is the only
//!   graph source on WebAssembly.
//! - [`OfflineGraphSource::FilePath`] memory-maps the archive on native
//!   targets. On WebAssembly it returns
//!   [`SDKError::ProviderRequestRejected`] with a message that directs the
//!   caller to [`OfflineGraphSource::Memory`], because the browser has no
//!   memory mapping.
//!
//! # Routing behavior
//!
//! - Only automobile routing is supported. The archive carries an automobile
//!   profile, so a request for another transportation mode is rejected with
//!   [`SDKError::ProviderRequestRejected`] rather than answered.
//! - Costs follow [`RouteOptions::route_strategy`]: `Fastest` minimizes edge
//!   durations and `Shortest` minimizes edge distances. Edge speeds and road
//!   classes come from each edge record.
//! - Steps are derived from graph metadata. Besides road-class and speed
//!   runs, a step boundary occurs at every junction whose signed bearing
//!   change leaves the straight band; the step then carries the matching
//!   [`TurnDirection`]. A road-class or speed change without a turn produces
//!   a `Continue` step. The archive carries no street names, so instruction
//!   text stays name-free and faithful to the geometry, for example
//!   `"Turn right"` or `"U-turn"`. A strongly curved road yields a step at
//!   each junction.
//! - Display and speech text on a step names the maneuver at the step's end
//!   junction, the point a countdown to the next maneuver refers to.
//!   [`ManeuverType`] stays anchored to the maneuver that opens the step. See
//!   `end_anchored_instruction_text`.
//! - Turn restrictions are evaluated against the incoming edge of the
//!   settled path. The approximation is exact unless a node is reachable
//!   through several best-cost paths with differently restricted incoming
//!   edges; in that case it can over-restrict, never under-restrict.
//!
//! # Threading and async
//!
//! [`OfflineRoutingProvider::load_offline_graph`] validates the archive and
//! builds the graph indexes, and
//! [`OfflineRoutingProvider::calculate_offline_route`] snaps the endpoints and
//! runs A*. Both are CPU-bound and run through the blocking-work seam
//! ([`crate::blocking_work::BlockingWorkExecutor`]): a target with a blocking
//! pool runs them on a worker thread, and the browser runs them inline on its
//! single task queue. No lock is held across an await; the search clones the
//! loaded graph handle and releases the read lock before running.
//!
//! A load validates the new archive completely before replacing the current
//! graph; a failed load leaves the previous graph loaded. The provider is safe
//! to share across threads.
//!
//! # Search engine and performance measurement
//!
//! [`a_star_search`] and [`dijkstra_reference_search`] expose the search
//! engine directly so performance can be measured over the same loaded graph.
//! Dijkstra is a reference baseline only: production routing always goes
//! through A*. Both searches expose [`ComputedRoute::settled_node_count`], a
//! deterministic expansion metric for comparing them. These items are hidden
//! from generated documentation and are not part of the supported API.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use geospatial_traits::{OfflineGraphSource, OfflineRoutingProvider};

use crate::blocking_work::{BlockingWorkExecutor, InlineBlockingWorkExecutor, run_blocking};
use geospatial_types::route::{
    ManeuverType, RouteData, RouteLeg, RouteOptions, RouteStep, RouteStrategy, TransportationMode,
    TurnDirection,
};
use geospatial_types::{BoundingBox, GeographicCoordinate, SDKError};
use rustic_maps_offline_graph_archive::archive::{
    OfflineRoutingNetworkGraph, RestrictionKind, RoadClass, SPEED_CELL_LIMIT,
    SPEED_QUANTUM_KILOMETERS_PER_HOUR,
};
#[cfg(not(target_arch = "wasm32"))]
use rustic_maps_offline_graph_archive::offline_routing_network_graph_from_mapped_file;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

/// Maximum distance a request coordinate may sit from the nearest graph
/// node and still be snapped to it for routing.
const NODE_SNAP_TOLERANCE_METERS: f64 = 50.0;

/// Fastest speed any edge of the packed speed column can carry; the
/// admissibility bound for A*'s duration heuristic (the `Fastest` strategy).
/// Written with primitive casts so it stays a plain constant.
const MAX_GRAPH_SPEED_KILOMETERS_PER_HOUR: f64 =
    SPEED_CELL_LIMIT as f64 * SPEED_QUANTUM_KILOMETERS_PER_HOUR as f64;

/// Unit conversion for the speed column.
const METERS_PER_KILOMETER: f64 = 1_000.0;
/// Unit conversion for the duration column (milliseconds → seconds).
const MILLISECONDS_PER_SECOND: f64 = 1_000.0;
/// Unit conversion for the speed column.
const SECONDS_PER_HOUR: f64 = 3_600.0;
/// The admissible heuristic scale for the duration cost function: at the
/// fastest storable edge speed, the straight-line distance is a lower bound
/// on every remaining traversal time.
const MAX_GRAPH_SPEED_METERS_PER_SECOND: f64 =
    MAX_GRAPH_SPEED_KILOMETERS_PER_HOUR * METERS_PER_KILOMETER / SECONDS_PER_HOUR;

/// Instruction text of the departure step (the archive has no street names;
/// see the module docs).
const DEPARTURE_INSTRUCTION_TEXT: &str = "Departure";
/// Instruction text of a road-class/speed-run change step (the archive has
/// no street names; see the module docs).
const CONTINUE_INSTRUCTION_TEXT: &str = "Continue";
/// Instruction text of the terminal step (the archive has no street names;
/// see the module docs).
const ARRIVAL_INSTRUCTION_TEXT: &str = "Arrival";

// Junction-geometry constants (`offline_routing`'s maneuver derivation).
//
// Bearings are measured on the local equirectangular plane around a
// junction's midpoint, mirroring rustic-maps-navigation's projection geometry and
// the compiler's mean-Earth radius, so derived turns match the geometry the
// archive itself encodes.
/// Mean-Earth radius for the local-plane bearing geometry, meters.
const EARTH_RADIUS_METERS: f64 = 6_371_000.0;
/// Segments shorter than this magnitude carry no representable direction;
/// their junction derives no maneuver rather than a fabricated one.
const MINIMUM_DIRECTIONAL_SEGMENT_LENGTH_METERS: f64 = 0.01;
/// Angular domain conversions for bearing arithmetic, in degrees.
const FULL_CIRCLE_DEGREES: f64 = 360.0;
/// Half-revolution magnitude, in degrees, of the signed bearing-change range.
const HALF_TURN_DEGREES: f64 = 180.0;

// Turn-direction bands over the junction's signed bearing change (degrees).
//
// Magnitude bands, chosen so the axis-aligned street grids' ninety-degree
// corners classify as standard turns and gentle curve junctions stay inside
// the straight band; each band edge is inclusive on its turn side:
/// Bearing changes within this absolute magnitude keep the run: the geometry
/// reads as straight on, so no turn maneuver is derived (a road-class/speed
/// change in that band still opens a `Continue` step).
const STRAIGHT_ON_BAND_DEGREES: f64 = 20.0;
/// Junction turns up to this magnitude are *slight* left/right turns.
const SLIGHT_TURN_BAND_DEGREES: f64 = 60.0;
/// Junction turns up to this magnitude are standard left/right turns.
const STANDARD_TURN_BAND_DEGREES: f64 = 120.0;
/// Junction turns up to this magnitude are *sharp* left/right turns; beyond
/// it the geometry reads as a full reversal of direction (a U-turn).
const SHARP_TURN_BAND_DEGREES: f64 = 160.0;

/// One traversable direction of an edge record. A record with both access
/// bits set contributes both of its directions.
///
/// Hidden from generated documentation: it is public so performance
/// measurement code in separate compilation units can reach the routing
/// engine. It is not part of the supported API.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HalfEdge {
    /// Graph node the traversal reaches.
    to_node_index: u32,
    /// Index of the edge record this direction belongs to, for run grouping
    /// and turn-restriction matching.
    edge_record_index: u32,
    distance_meters: f64,
    duration_seconds: f64,
    road_class: RoadClass,
    speed_kilometers_per_hour: u16,
}

/// One turn-restriction rule of a loaded graph, keyed by the edge record the
/// turn is made from and the node it turns at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TurnRule {
    restriction_kind: RestrictionKind,
    to_edge_index: u32,
}

/// A validated archive plus the routing indexes built from it at load time.
///
/// Routing consumes the archived geometry, the packed edge columns and the
/// turn-restriction rules, so the graph is decomposed into those indexes
/// here. The archive itself is not retained.
///
/// Hidden from generated documentation: it is public so performance
/// measurement code in separate compilation units can build and search it.
/// It is not part of the supported API.
#[doc(hidden)]
#[derive(Debug)]
pub struct LoadedRoutingGraph {
    node_coordinates: Vec<GeographicCoordinate>,
    outgoing_half_edges: Vec<Vec<HalfEdge>>,
    /// Keyed by `(from edge record index, via node index)`. Multiple rules
    /// for one key combine: a `no` rule blocks its recorded edge pair, and
    /// any `only` rule restricts that from/via position to the union of its
    /// allowed edge records.
    turn_rules: HashMap<(u32, u32), Vec<TurnRule>>,
}

/// Builds a coordinate out of archive node degrees. The archive quantizes
/// into canonical ranges, so validation can only fail on a compiler bug.
fn archive_node_coordinate(latitude_degrees: f64, longitude_degrees: f64) -> GeographicCoordinate {
    GeographicCoordinate::from_degrees(latitude_degrees, longitude_degrees)
        .expect("archive node coordinates are quantized within the canonical ranges")
}

impl LoadedRoutingGraph {
    /// Turns a validated archive into a router-ready graph, deriving the
    /// half-edge adjacency and the restriction rules once at load time.
    ///
    /// Hidden from generated documentation; public for performance
    /// measurement code (see the struct documentation).
    #[doc(hidden)]
    #[must_use]
    pub fn new(network_graph: OfflineRoutingNetworkGraph) -> Self {
        let node_coordinates = network_graph
            .nodes()
            .iter()
            .map(|node_record| {
                archive_node_coordinate(
                    node_record.latitude_degrees(),
                    node_record.longitude_degrees(),
                )
            })
            .collect();

        let mut outgoing_half_edges: Vec<Vec<HalfEdge>> =
            vec![Vec::new(); network_graph.nodes().len()];
        let mut turn_rules: HashMap<(u32, u32), Vec<TurnRule>> = HashMap::new();

        for (edge_record_index, edge_record) in network_graph.edges().iter().enumerate() {
            let road_class = edge_record
                .road_class()
                .expect("validated at load: the reader rejects unknown road classes");
            let half_edge_template = HalfEdge {
                to_node_index: 0,
                edge_record_index: edge_record_index as u32,
                distance_meters: f64::from(edge_record.distance_meters()),
                duration_seconds: f64::from(edge_record.duration_milliseconds())
                    / MILLISECONDS_PER_SECOND,
                road_class,
                speed_kilometers_per_hour: edge_record.speed_kilometers_per_hour(),
            };
            // Emitted record orientation: the compiler stores every
            // traversable direction as its own (source, target) pair — the
            // round-trip contract of the committed fixture pins the
            // against-node-order direction as the stored (later, earlier)
            // pair — so the traversable direction is exactly the stored one
            // and the access bits annotate which OSM way node-order
            // direction the record covers (provenance, not orientation).
            // (Reading the backward bit as "traverse target→source" instead
            // would fold every two-way way's two directions onto one arc.)
            if edge_record.forward_allowed() || edge_record.backward_allowed() {
                outgoing_half_edges[edge_record.source_node_index() as usize].push(HalfEdge {
                    to_node_index: edge_record.target_node_index(),
                    ..half_edge_template
                });
            }
        }

        for turn_restriction in network_graph.turn_restrictions() {
            turn_rules
                .entry((
                    turn_restriction.from_edge_index(),
                    turn_restriction.via_node_index(),
                ))
                .or_default()
                .push(TurnRule {
                    restriction_kind: turn_restriction
                        .restriction_kind()
                        .expect("validated at load: the reader rejects unknown restriction kinds"),
                    to_edge_index: turn_restriction.to_edge_index(),
                });
        }

        Self {
            node_coordinates,
            outgoing_half_edges,
            turn_rules,
        }
    }

    /// Nearest node to the request coordinate within
    /// [`NODE_SNAP_TOLERANCE_METERS`].
    fn snap_node_index(&self, request_coordinate: &GeographicCoordinate) -> Result<u32, SDKError> {
        let (best_node_index, best_distance_meters) = self
            .node_coordinates
            .iter()
            .enumerate()
            .map(|(node_index, node_coordinate)| {
                (
                    node_index,
                    node_coordinate.haversine_distance_meters(request_coordinate),
                )
            })
            .min_by(|left, right| left.1.total_cmp(&right.1))
            .expect("the archive always carries at least one node to snap to");
        if best_distance_meters > NODE_SNAP_TOLERANCE_METERS {
            return Err(SDKError::InvalidCoordinateInput(format!(
                "no offline routing graph node within \
                 {NODE_SNAP_TOLERANCE_METERS} m of the request coordinate"
            )));
        }
        Ok(best_node_index as u32)
    }
}

/// An offline routing provider over a compiled routing graph archive.
///
/// Load an archive with [`OfflineRoutingProvider::load_offline_graph`], then
/// answer automobile route requests with
/// [`OfflineRoutingProvider::calculate_offline_route`]. See the module
/// documentation for supported routing behavior, target rules and thread
/// safety.
pub struct ArchiveOfflineRoutingProvider {
    loaded_graph: RwLock<Option<Arc<LoadedRoutingGraph>>>,
    blocking_work_executor: Arc<dyn BlockingWorkExecutor>,
}

impl std::fmt::Debug for ArchiveOfflineRoutingProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ArchiveOfflineRoutingProvider")
            .finish_non_exhaustive()
    }
}

impl Default for ArchiveOfflineRoutingProvider {
    fn default() -> Self {
        Self {
            loaded_graph: RwLock::new(None),
            blocking_work_executor: Arc::new(InlineBlockingWorkExecutor),
        }
    }
}

impl ArchiveOfflineRoutingProvider {
    /// A provider with no graph loaded.
    ///
    /// Archive loading and route searches run inline on the polling thread,
    /// so the provider is usable on a target with no worker threads (the
    /// browser). A target with a blocking pool supplies it through
    /// [`Self::with_blocking_work_executor`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Runs the archive load and the route search through `executor`, keeping
    /// CPU-bound graph work off the async reactor.
    #[must_use]
    pub fn with_blocking_work_executor(
        mut self,
        blocking_work_executor: Arc<dyn BlockingWorkExecutor>,
    ) -> Self {
        self.blocking_work_executor = blocking_work_executor;
        self
    }
}

/// Loads a compiled routing graph from in-memory archive bytes and calculates
/// one route between two coordinates with the default request options
/// ([`RouteOptions::default`]: automobile, fastest, metric).
///
/// The bytes are validated by the same reader as
/// [`OfflineGraphSource::Memory`]. This function is the single implementation
/// behind the corresponding free helper and WebAssembly export, so the native
/// and browser surfaces cannot diverge.
///
/// # Parameters
/// - `archive_bytes` — a complete compiled routing graph archive.
/// - `start`, `end` — the route endpoints.
///
/// # Returns
/// A route between the two points, including geometry and steps.
///
/// # Errors
/// Returns [`SDKError::OfflineRoutingFailure`] when the archive is truncated,
/// malformed or of an unsupported version. Returns
/// [`SDKError::InvalidCoordinateInput`] when an endpoint is beyond the graph's
/// snap tolerance, and [`SDKError::ProviderRequestRejected`] when the graph
/// cannot connect the two points. No coordinate or connection is guessed.
///
/// # Async
/// The function performs no network access and does not block on I/O beyond
/// validating the supplied bytes.
pub async fn calculate_offline_route_from_archive_bytes(
    archive_bytes: &[u8],
    start: &GeographicCoordinate,
    end: &GeographicCoordinate,
) -> Result<RouteData, SDKError> {
    let offline_provider = ArchiveOfflineRoutingProvider::new();
    offline_provider
        .load_offline_graph(OfflineGraphSource::Memory(archive_bytes.to_vec()))
        .await?;
    offline_provider
        .calculate_offline_route(start, end, &RouteOptions::default())
        .await
}

/// The `FilePath`-rejection message for WebAssembly (see the module
/// documentation): the browser loads the offline routing graph from in-memory
/// bytes, so a file-path load names the [`OfflineGraphSource::Memory`] source
/// it should have used instead of failing opaquely.
#[cfg(target_arch = "wasm32")]
fn wasm_file_path_rejection_message() -> &'static str {
    "offline routing graph archives cannot be loaded from a file path on the \
     wasm target (no mmap in the browser); load the compiled archive from \
     in-memory bytes via OfflineGraphSource::Memory"
}

/// The cost weight A* minimizes, derived from
/// [`RouteOptions::route_strategy`].
///
/// Hidden from generated documentation: it is public so performance
/// measurement code can pass it into both searches (see the struct
/// documentation of [`LoadedRoutingGraph`]). It is not part of the supported
/// API.
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub enum TraversalCost {
    /// Minimize edge durations (the `Fastest` strategy).
    TimeSeconds,
    /// Minimize edge distances (the `Shortest` strategy).
    DistanceMeters,
}

impl TraversalCost {
    fn cost_of(&self, half_edge: &HalfEdge) -> f64 {
        match self {
            Self::TimeSeconds => half_edge.duration_seconds,
            Self::DistanceMeters => half_edge.distance_meters,
        }
    }
}

/// Binary-heap entry ordered smallest-`priority`-first. Equality and
/// ordering follow `total_cmp` (total order), so `Eq` is a deliberate, safe
/// empty implementation.
#[derive(Debug, PartialEq)]
struct FrontierEntry {
    priority: f64,
    node_index: u32,
}

impl Eq for FrontierEntry {}

impl Ord for FrontierEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // The heap pops what this ordering calls "greatest", so the priority
        // comparison is inverted here to pop the smallest priority first.
        other
            .priority
            .total_cmp(&self.priority)
            .then_with(|| self.node_index.cmp(&other.node_index))
    }
}

impl PartialOrd for FrontierEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[async_trait]
impl OfflineRoutingProvider for ArchiveOfflineRoutingProvider {
    async fn load_offline_graph(&self, graph_source: OfflineGraphSource) -> Result<(), SDKError> {
        // Validation, memory mapping and index construction are CPU-bound and
        // run through the blocking-work seam; the wasm inline executor runs
        // them on the browser task queue, the native executor on a worker.
        let network_graph = run_blocking(self.blocking_work_executor.as_ref(), move || {
            match graph_source {
                // Blocking mmap + read-only validation, off the async reactor.
                #[cfg(not(target_arch = "wasm32"))]
                OfflineGraphSource::FilePath(archive_path) => {
                    offline_routing_network_graph_from_mapped_file(Path::new(&archive_path))
                }
                // The wasm arm of the compile-time target switch: no `mmap`
                // exists in the browser, so a file-path load is rejected typed
                // instead of being attempted (see the module docs).
                #[cfg(target_arch = "wasm32")]
                OfflineGraphSource::FilePath(_) => Err(SDKError::ProviderRequestRejected(
                    wasm_file_path_rejection_message().to_string(),
                )),
                OfflineGraphSource::Memory(archive_bytes) => {
                    OfflineRoutingNetworkGraph::from_bytes(&archive_bytes)
                }
            }
        })
        .await?;

        // Validate-completely-then-replace: a failed load above leaves the
        // previously loaded graph untouched. Locks are held for the copy,
        // never across an await.
        let mut loaded_graph_guard = self.loaded_graph.write().unwrap_or_else(|poisoned| {
            // A poisoned lock only means a thread panicked mid-replacement;
            // the stored graph itself is a plain value and stays usable.
            poisoned.into_inner()
        });
        *loaded_graph_guard = Some(Arc::new(LoadedRoutingGraph::new(network_graph)));
        Ok(())
    }

    async fn calculate_offline_route(
        &self,
        start: &GeographicCoordinate,
        end: &GeographicCoordinate,
        route_options: &RouteOptions,
    ) -> Result<RouteData, SDKError> {
        match route_options.transportation_mode {
            TransportationMode::Automobile => {}
            other_mode => {
                return Err(SDKError::ProviderRequestRejected(format!(
                    "the offline routing graph is an automobile profile; \
                     requested {other_mode:?}"
                )));
            }
        }

        // Clone the graph handle out of the lock, then release it: the
        // CPU-bound snap and A* search run through the blocking-work seam
        // without holding a lock across an await.
        let loaded_graph = {
            let loaded_graph_guard = self
                .loaded_graph
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            loaded_graph_guard.as_ref().cloned().ok_or_else(|| {
                SDKError::ProviderRequestRejected(
                    "no offline routing graph is loaded; call load_offline_graph first".to_string(),
                )
            })?
        };

        let traversal_cost = match route_options.route_strategy {
            RouteStrategy::Fastest => TraversalCost::TimeSeconds,
            RouteStrategy::Shortest => TraversalCost::DistanceMeters,
        };
        let start = *start;
        let end = *end;
        run_blocking(self.blocking_work_executor.as_ref(), move || {
            let start_node_index = loaded_graph.snap_node_index(&start)?;
            let end_node_index = loaded_graph.snap_node_index(&end)?;
            compute_route(
                loaded_graph.as_ref(),
                start_node_index,
                end_node_index,
                traversal_cost,
            )
        })
        .await
    }
}

/// The node chain and traversal cost of a successful search.
///
/// Hidden from generated documentation: it is public so performance
/// measurement code can compare the two searches' chains, costs and
/// settled-set sizes through these fields (see the struct documentation of
/// [`LoadedRoutingGraph`]). It is not part of the supported API.
#[doc(hidden)]
pub struct ComputedRoute {
    /// Node chain positions along the route, start (inclusive) to end
    /// (inclusive).
    pub node_chain_indices: Vec<u32>,
    /// Traversed half-edges in route order; empty for a start-equals-end
    /// route.
    pub route_half_edges: Vec<HalfEdge>,
    /// How many nodes the search settled (popped and expanded) before the
    /// terminal settle: a deterministic expansion metric for comparing search
    /// strategies (see the module documentation).
    pub settled_node_count: usize,
    /// The accumulated start→end traversal cost in the requested weight
    /// (seconds for [`TraversalCost::TimeSeconds`], meters for
    /// [`TraversalCost::DistanceMeters`]).
    pub total_cost: f64,
}

/// Runs A* between the snapped nodes and materializes the result as
/// [`RouteData`].
///
/// # Errors
/// Returns [`SDKError::ProviderRequestRejected`] when no drivable path exists
/// between the two nodes.
fn compute_route(
    loaded_graph: &LoadedRoutingGraph,
    start_node_index: u32,
    end_node_index: u32,
    traversal_cost: TraversalCost,
) -> Result<RouteData, SDKError> {
    let Some(route_chain) = a_star_search(
        loaded_graph,
        start_node_index,
        end_node_index,
        traversal_cost,
    ) else {
        return Err(SDKError::ProviderRequestRejected(
            "the offline routing graph cannot connect the request points; \
             no drivable path exists between the snapped nodes"
                .to_string(),
        ));
    };
    Ok(materialize_route_data(loaded_graph, &route_chain))
}

/// The maneuver content of one materialized route step (see the docs of
/// [`RouteStepSpan`]).
#[derive(Debug, Clone, Copy, PartialEq)]
enum StepManeuver {
    /// The route's opening step.
    Departure,
    /// A road-class/speed run change without a derived turn.
    Continue,
    /// A junction turn derived from the signed bearing change.
    Turn(TurnDirection),
}

/// One maximal step of the materialized route: a graph-edge span carried as
/// one `RouteStep`, opened by a maneuver ([`StepManeuver`]) and kept open
/// while the junctions along it stay inside the straight band on unchanged
/// road class and speed.
struct RouteStepSpan {
    /// Inclusive node-chain index span of the step (the step's shape runs
    /// from its first node's predecessor edge to the junction node closing
    /// it — the next step's maneuver point).
    node_chain_start: usize,
    node_chain_end: usize,
    distance_meters: f64,
    duration_seconds: f64,
    step_maneuver: StepManeuver,
}

/// The junction verdict between two consecutive route edges: the maneuver
/// [`StepManeuver`] the next step opens with, or `None` when the run
/// continues (geometry within the straight band on unchanged road class and
/// speed).
fn junction_step_entry(
    node_chain: &[GeographicCoordinate],
    route_half_edges: &[HalfEdge],
    edge_position: usize,
) -> Option<StepManeuver> {
    let incoming_bearing =
        segment_bearing_degrees(&node_chain[edge_position - 1], &node_chain[edge_position]);
    let outgoing_bearing =
        segment_bearing_degrees(&node_chain[edge_position], &node_chain[edge_position + 1]);
    let (Some(incoming_bearing_degrees), Some(outgoing_bearing_degrees)) =
        (incoming_bearing, outgoing_bearing)
    else {
        // Unresolvable junction geometry derives no turn maneuver; the
        // road-class/speed run comparison below still applies.
        return run_key_change_maneuver(
            &route_half_edges[edge_position - 1],
            &route_half_edges[edge_position],
        );
    };
    let signed_bearing_change_degrees =
        signed_bearing_change_degrees(incoming_bearing_degrees, outgoing_bearing_degrees);
    if let Some(derived_turn_direction) = derived_turn_direction(signed_bearing_change_degrees) {
        return Some(StepManeuver::Turn(derived_turn_direction));
    }
    run_key_change_maneuver(
        &route_half_edges[edge_position - 1],
        &route_half_edges[edge_position],
    )
}

/// The `Continue` maneuver of a road-class or speed change, or `None` when
/// the two edges share both columns.
fn run_key_change_maneuver(
    previous_half_edge: &HalfEdge,
    half_edge: &HalfEdge,
) -> Option<StepManeuver> {
    let same_run_key = previous_half_edge.road_class == half_edge.road_class
        && previous_half_edge.speed_kilometers_per_hour == half_edge.speed_kilometers_per_hour;
    if same_run_key {
        return None;
    }
    Some(StepManeuver::Continue)
}

/// North-referenced bearing of the segment start→end, in degrees over
/// `[0, 360)`, measured on the local equirectangular plane around the segment
/// midpoint.
///
/// # Returns
/// The bearing, or `None` for a segment too short to carry a representable
/// direction.
fn segment_bearing_degrees(
    segment_start: &GeographicCoordinate,
    segment_end: &GeographicCoordinate,
) -> Option<f64> {
    let reference_latitude = (segment_start.latitude + segment_end.latitude) / 2.0;
    let east_meters = (segment_end.longitude - segment_start.longitude).to_radians()
        * EARTH_RADIUS_METERS
        * reference_latitude.to_radians().cos();
    let north_meters =
        (segment_end.latitude - segment_start.latitude).to_radians() * EARTH_RADIUS_METERS;
    let segment_length_meters = east_meters.hypot(north_meters);
    if segment_length_meters < MINIMUM_DIRECTIONAL_SEGMENT_LENGTH_METERS {
        return None;
    }
    let north_referenced_bearing_degrees = east_meters.atan2(north_meters).to_degrees();
    Some((north_referenced_bearing_degrees + FULL_CIRCLE_DEGREES).rem_euclid(FULL_CIRCLE_DEGREES))
}

/// The signed bearing change between two north-referenced bearings, in
/// degrees over `(-180, 180]`, measuring clockwise (toward east from a
/// northbound road) as positive: the right-turn side.
fn signed_bearing_change_degrees(
    incoming_bearing_degrees: f64,
    outgoing_bearing_degrees: f64,
) -> f64 {
    (outgoing_bearing_degrees - incoming_bearing_degrees + HALF_TURN_DEGREES)
        .rem_euclid(FULL_CIRCLE_DEGREES)
        - HALF_TURN_DEGREES
}

/// The turn a junction's signed bearing change derives, or `None` while the
/// change stays inside the straight band (see the band constants above).
///
/// [`TurnDirection::Straight`] and [`TurnDirection::LeavingRoad`] are never
/// derived: the straight band closes on `None`, because the run's `Continue`
/// semantics cover the straight case, and a road-name departure is not a
/// bearing fact the archive carries.
fn derived_turn_direction(signed_bearing_change_degrees: f64) -> Option<TurnDirection> {
    let turn_magnitude_degrees = signed_bearing_change_degrees.abs();
    if turn_magnitude_degrees <= STRAIGHT_ON_BAND_DEGREES {
        return None;
    }
    let (right_turn_direction, left_turn_direction) =
        if turn_magnitude_degrees <= SLIGHT_TURN_BAND_DEGREES {
            (TurnDirection::SlightRight, TurnDirection::SlightLeft)
        } else if turn_magnitude_degrees <= STANDARD_TURN_BAND_DEGREES {
            (TurnDirection::Right, TurnDirection::Left)
        } else if turn_magnitude_degrees <= SHARP_TURN_BAND_DEGREES {
            (TurnDirection::SharpRight, TurnDirection::SharpLeft)
        } else {
            (TurnDirection::UTurn, TurnDirection::UTurn)
        };
    Some(if signed_bearing_change_degrees.is_sign_positive() {
        right_turn_direction
    } else {
        left_turn_direction
    })
}

/// The name-free instruction text of a step-opening maneuver.
///
/// The archive carries no street names, so the text states only what the
/// derived geometry justifies. The text is head-anchored: it names the
/// maneuver performed at the span's opening junction.
/// [`materialize_route_data`] shifts authored texts onto the step they belong
/// to for the banner (see [`end_anchored_instruction_text`]).
fn step_maneuver_instruction_text(step_maneuver: StepManeuver) -> &'static str {
    match step_maneuver {
        StepManeuver::Departure => DEPARTURE_INSTRUCTION_TEXT,
        StepManeuver::Continue => CONTINUE_INSTRUCTION_TEXT,
        StepManeuver::Turn(turn_direction) => turn_instruction_text(turn_direction),
    }
}

/// The name-free instruction text of a turn maneuver.
///
/// The archive carries no street names, so the text states only what the
/// derived geometry justifies.
fn turn_instruction_text(turn_direction: TurnDirection) -> &'static str {
    match turn_direction {
        TurnDirection::Straight | TurnDirection::LeavingRoad => {
            // Never derived from a bearing change (see
            // `derived_turn_direction`); a straight-band junction carries
            // the Continue content instead.
            CONTINUE_INSTRUCTION_TEXT
        }
        TurnDirection::SlightLeft => "Turn slight left",
        TurnDirection::Left => "Turn left",
        TurnDirection::SharpLeft => "Turn sharp left",
        TurnDirection::SlightRight => "Turn slight right",
        TurnDirection::Right => "Turn right",
        TurnDirection::SharpRight => "Turn sharp right",
        TurnDirection::UTurn => "U-turn",
    }
}

/// The instruction text surfaced on the step covering the span at
/// `route_span_index`, given each span's authored text in span order.
///
/// # Step text is end-anchored
///
/// A span's own opening maneuver is anchored at the junction entering the
/// span, which matches the head-anchored [`ManeuverType`] and the archive
/// geometry. A banner, however, counts down to a step's end: the junction
/// closing the span. While a span is traversed, head-anchoring would keep
/// narrating the turn already performed and never name the maneuver being
/// approached. The step's text therefore carries the authored text of the
/// next span's opening maneuver, the maneuver at the step's end junction.
///
/// - The final traversal span has no following junction maneuver, because its
///   end is the destination itself. It keeps its own authored text, so no
///   authored text is lost.
/// - The departure text is never surfaced as a banner while a turn is still
///   ahead. Only a route with no junction maneuver at all keeps it.
/// - The arrival is not a span-authored text. It is an appended, zero-cost
///   terminal step, and its text stays the arrival's own; the final traversal
///   span does not adopt it.
fn end_anchored_instruction_text(
    authored_texts: &[&'static str],
    route_span_index: usize,
) -> &'static str {
    // The `&&'static str` of `get` coerces to the `&'static str` return.
    authored_texts
        .get(route_span_index + 1)
        .unwrap_or(&authored_texts[route_span_index])
}

/// A* over the half-edge adjacency with a haversine heuristic.
///
/// The heuristic is admissible for both weights: straight-line distance is a
/// lower bound on distance costs, and, under the packed speed column's
/// saturation limit, on duration costs. Turn restrictions are evaluated
/// against the settled path's incoming half-edge (see the module
/// documentation).
///
/// # Parameters
/// - `loaded_graph` — the indexed graph to search.
/// - `start_node_index`, `end_node_index` — the endpoint node indexes.
/// - `traversal_cost` — the cost weight to minimize.
///
/// # Returns
/// The optimal route chain, or `None` when the end node is unreachable.
///
/// Hidden from generated documentation: public so performance measurement
/// code can compare it with [`dijkstra_reference_search`]. It is not part of
/// the supported API.
#[doc(hidden)]
pub fn a_star_search(
    loaded_graph: &LoadedRoutingGraph,
    start_node_index: u32,
    end_node_index: u32,
    traversal_cost: TraversalCost,
) -> Option<ComputedRoute> {
    let end_coordinate = loaded_graph.node_coordinates[end_node_index as usize];
    frontier_search(
        loaded_graph,
        start_node_index,
        end_node_index,
        traversal_cost,
        |node_index| {
            let straight_line_meters = loaded_graph.node_coordinates[node_index as usize]
                .haversine_distance_meters(&end_coordinate);
            match traversal_cost {
                TraversalCost::DistanceMeters => straight_line_meters,
                TraversalCost::TimeSeconds => {
                    straight_line_meters / MAX_GRAPH_SPEED_METERS_PER_SECOND
                }
            }
        },
    )
}

/// The Dijkstra reference search: A*'s twin with a zero heuristic.
///
/// It runs the same relaxation, settled heap, restriction evaluation and
/// chain reconstruction as [`a_star_search`], so comparing the two isolates
/// the heuristic's pruning.
///
/// This is a measurement baseline only and is not a production route path.
/// Routing always uses [`a_star_search`]. The function is hidden from
/// generated documentation and public only so measurement code in separate
/// compilation units can call it; it is not part of the supported API.
#[doc(hidden)]
pub fn dijkstra_reference_search(
    loaded_graph: &LoadedRoutingGraph,
    start_node_index: u32,
    end_node_index: u32,
    traversal_cost: TraversalCost,
) -> Option<ComputedRoute> {
    frontier_search(
        loaded_graph,
        start_node_index,
        end_node_index,
        traversal_cost,
        |_| ZERO_HEURISTIC,
    )
}

/// The zero heuristic of the Dijkstra reference search.
const ZERO_HEURISTIC: f64 = 0.0;

/// The shared frontier search behind A* and its Dijkstra reference: one
/// relaxation/settlement/restriction engine, with the heuristic
/// monomorphized in (zero cost for the reference), so the two algorithms'
/// comparison isolates exactly the heuristic's pruning.
fn frontier_search(
    loaded_graph: &LoadedRoutingGraph,
    start_node_index: u32,
    end_node_index: u32,
    traversal_cost: TraversalCost,
    heuristic: impl Fn(u32) -> f64,
) -> Option<ComputedRoute> {
    let node_count = loaded_graph.node_coordinates.len();
    let mut distance_from_start = vec![f64::INFINITY; node_count];
    // Predecessor half-edge as `(source node, outgoing-list position)` —
    // enough to reconstruct the traversed half-edges backwards.
    let mut previous_half_edge: Vec<Option<(usize, usize)>> = vec![None; node_count];
    let mut settled = vec![false; node_count];
    let mut frontier = std::collections::BinaryHeap::new();
    // Settled-set size before the terminal settle (the deterministic
    // expansion metric the benchmark suite compares A* against the Dijkstra
    // reference with).
    let mut settled_node_count = 0usize;

    distance_from_start[start_node_index as usize] = 0.0;
    frontier.push(FrontierEntry {
        priority: heuristic(start_node_index),
        node_index: start_node_index,
    });

    while let Some(FrontierEntry { node_index, .. }) = frontier.pop() {
        let node_index_as_usize = node_index as usize;
        if settled[node_index_as_usize] {
            continue;
        }
        settled[node_index_as_usize] = true;
        settled_node_count += 1;
        if node_index == end_node_index {
            break;
        }
        for (half_edge_position, half_edge) in loaded_graph.outgoing_half_edges[node_index_as_usize]
            .iter()
            .enumerate()
        {
            let to_node_index_as_usize = half_edge.to_node_index as usize;
            if settled[to_node_index_as_usize] {
                continue;
            }
            if turn_is_restricted(
                loaded_graph,
                previous_half_edge[node_index_as_usize],
                node_index_as_usize,
                half_edge,
            ) {
                continue;
            }
            let candidate_distance =
                distance_from_start[node_index_as_usize] + traversal_cost.cost_of(half_edge);
            if candidate_distance < distance_from_start[to_node_index_as_usize] {
                distance_from_start[to_node_index_as_usize] = candidate_distance;
                previous_half_edge[to_node_index_as_usize] =
                    Some((node_index_as_usize, half_edge_position));
                frontier.push(FrontierEntry {
                    priority: candidate_distance + heuristic(half_edge.to_node_index),
                    node_index: half_edge.to_node_index,
                });
            }
        }
    }

    if !settled[end_node_index as usize] {
        return None;
    }

    let total_cost = distance_from_start[end_node_index as usize];
    let mut route_half_edges = Vec::new();
    let mut walk_node_index = end_node_index as usize;
    while walk_node_index != start_node_index as usize {
        let (previous_node_index, half_edge_position) = previous_half_edge[walk_node_index]?;
        let half_edge = loaded_graph.outgoing_half_edges[previous_node_index][half_edge_position];
        route_half_edges.push(half_edge);
        walk_node_index = previous_node_index;
    }
    route_half_edges.reverse();

    let node_chain_indices = std::iter::once(start_node_index)
        .chain(
            route_half_edges
                .iter()
                .map(|half_edge| half_edge.to_node_index),
        )
        .collect();
    Some(ComputedRoute {
        node_chain_indices,
        route_half_edges,
        settled_node_count,
        total_cost,
    })
}

/// Whether the move from `current_node_index` (arrived via the settled
/// incoming half-edge, or fresh from the start) onto `candidate_half_edge`
/// is blocked by a turn-rule restriction.
fn turn_is_restricted(
    loaded_graph: &LoadedRoutingGraph,
    incoming_half_edge: Option<(usize, usize)>,
    current_node_index: usize,
    candidate_half_edge: &HalfEdge,
) -> bool {
    let Some((previous_node_index, previous_half_edge_position)) = incoming_half_edge else {
        return false;
    };
    let incoming_edge_record_index = loaded_graph.outgoing_half_edges[previous_node_index]
        [previous_half_edge_position]
        .edge_record_index;
    let Some(turn_rules) = loaded_graph.turn_rules.get(&(
        incoming_edge_record_index,
        u32::try_from(current_node_index).unwrap_or(u32::MAX),
    )) else {
        return false;
    };

    let mut forbidden = false;
    let mut only_allowed_to_edge_indices: Option<Vec<u32>> = None;
    for turn_rule in turn_rules {
        match turn_rule.restriction_kind {
            RestrictionKind::OnlyStraightOn => only_allowed_to_edge_indices
                .get_or_insert_with(Vec::new)
                .push(turn_rule.to_edge_index),
            _ => forbidden |= turn_rule.to_edge_index == candidate_half_edge.edge_record_index,
        }
    }
    forbidden
        || only_allowed_to_edge_indices
            .is_some_and(|allowed| !allowed.contains(&candidate_half_edge.edge_record_index))
}

/// Splits the route's half-edges into maximal steps: a span opens at a
/// maneuver ([`StepManeuver`]) — the route's `Departure`, a derived junction
/// turn, or a road-class/speed run change — and continues across junctions
/// that stay inside the straight band on unchanged road class and speed.
fn group_steps(
    node_chain: &[GeographicCoordinate],
    route_half_edges: &[HalfEdge],
) -> Vec<RouteStepSpan> {
    let mut step_spans: Vec<RouteStepSpan> = Vec::new();
    for (edge_position, half_edge) in route_half_edges.iter().enumerate() {
        let step_maneuver = if edge_position == 0 {
            StepManeuver::Departure
        } else {
            match junction_step_entry(node_chain, route_half_edges, edge_position) {
                Some(new_step_maneuver) => new_step_maneuver,
                None => {
                    // The run continues: extend the step already in
                    // progress (the first edge always opened one).
                    let continuing_step = step_spans
                        .last_mut()
                        .expect("a continuing step always follows its opened first edge");
                    continuing_step.node_chain_end = edge_position + 1;
                    continuing_step.distance_meters += half_edge.distance_meters;
                    continuing_step.duration_seconds += half_edge.duration_seconds;
                    continue;
                }
            }
        };
        step_spans.push(RouteStepSpan {
            node_chain_start: edge_position,
            node_chain_end: edge_position + 1,
            distance_meters: half_edge.distance_meters,
            duration_seconds: half_edge.duration_seconds,
            step_maneuver,
        });
    }
    step_spans
}

/// Builds the [`RouteData`] for a route chain.
///
/// The result has one step per junction-justified maneuver or road-class or
/// speed run change; it opens with a `Departure` step and ends with a
/// terminal, zero-cost `Arrival` step. Distances and durations come strictly
/// from the edge records. Step texts are end-anchored (see
/// [`end_anchored_instruction_text`]), while [`ManeuverType`] stays
/// head-anchored on each span's own opening maneuver.
fn materialize_route_data(
    loaded_graph: &LoadedRoutingGraph,
    route_chain: &ComputedRoute,
) -> RouteData {
    let node_chain: Vec<GeographicCoordinate> = route_chain
        .node_chain_indices
        .iter()
        .map(|&node_index| loaded_graph.node_coordinates[node_index as usize])
        .collect();

    let mut steps: Vec<RouteStep> = Vec::new();
    if route_chain.route_half_edges.is_empty() {
        // A start-equals-end route: both maneuvers at the same point, zero
        // cost.
        steps.push(RouteStep {
            distance_meters: 0.0,
            duration_seconds: 0.0,
            maneuver_type: ManeuverType::Departure,
            instruction_display_text: DEPARTURE_INSTRUCTION_TEXT.to_string(),
            instruction_speech_text: DEPARTURE_INSTRUCTION_TEXT.to_string(),
            shape: node_chain.clone(),
            lane_guidances: Vec::new(),
        });
    }
    let route_step_spans = group_steps(&node_chain, &route_chain.route_half_edges);
    // Each span's authored (head-anchored maneuver) text, in span order —
    // the pool the end-anchored shift below draws from.
    let authored_instruction_texts: Vec<&'static str> = route_step_spans
        .iter()
        .map(|route_step_span| step_maneuver_instruction_text(route_step_span.step_maneuver))
        .collect();
    for (route_span_index, route_step_span) in route_step_spans.iter().enumerate() {
        let maneuver_type = match route_step_span.step_maneuver {
            StepManeuver::Departure => ManeuverType::Departure,
            StepManeuver::Continue => ManeuverType::Continue,
            StepManeuver::Turn(turn_direction) => ManeuverType::Turn(turn_direction),
        };
        let instruction_text =
            end_anchored_instruction_text(&authored_instruction_texts, route_span_index);
        steps.push(RouteStep {
            distance_meters: route_step_span.distance_meters,
            duration_seconds: route_step_span.duration_seconds,
            maneuver_type,
            instruction_display_text: instruction_text.to_string(),
            instruction_speech_text: instruction_text.to_string(),
            shape: node_chain[route_step_span.node_chain_start..=route_step_span.node_chain_end]
                .to_vec(),
            // The archive's lane-guidance resource is present-but-minimal
            // (the compiler emits no records in this slice); no lane
            // guidance is fabricated.
            lane_guidances: Vec::new(),
        });
    }
    let end_coordinate = *node_chain
        .last()
        .expect("the route node chain always carries at least the snapped start node");
    steps.push(RouteStep {
        distance_meters: 0.0,
        duration_seconds: 0.0,
        maneuver_type: ManeuverType::Arrival,
        instruction_display_text: ARRIVAL_INSTRUCTION_TEXT.to_string(),
        instruction_speech_text: ARRIVAL_INSTRUCTION_TEXT.to_string(),
        shape: vec![end_coordinate],
        lane_guidances: Vec::new(),
    });

    let total_distance_meters: f64 = route_chain
        .route_half_edges
        .iter()
        .map(|half_edge| half_edge.distance_meters)
        .sum();
    let total_duration_seconds: f64 = route_chain
        .route_half_edges
        .iter()
        .map(|half_edge| half_edge.duration_seconds)
        .sum();

    RouteData {
        distance_meters: total_distance_meters,
        duration_seconds: total_duration_seconds,
        bounding_box: bounding_box_over(&node_chain),
        geometry: Vec::new(),
        legs: vec![RouteLeg {
            distance_meters: total_distance_meters,
            duration_seconds: total_duration_seconds,
            steps,
        }],
    }
}

/// The geographic bounds of the route's node chain. Coordinates are exact
/// (quantized) degrees, so the corner construction cannot fail.
fn bounding_box_over(node_chain: &[GeographicCoordinate]) -> BoundingBox {
    BoundingBox::from_coordinates(node_chain.iter().copied())
        .expect("a route has at least one node")
}

#[cfg(test)]
mod tests {
    use super::*;
    use geospatial_types::OfflineGraphInvalidReason;
    use geospatial_types::route::{AvoidOptions, UnitSystem};
    use rustic_maps_offline_graph_archive::archive::{
        EdgeProperties, EdgeRecord, INSTRUCTION_ABSENT_INDEX, NodeRecord,
        OfflineRoutingNetworkGraph, OfflineRoutingNetworkGraphBuilder, TurnRestrictionRecord,
    };
    use rustic_maps_offline_graph_compiler::compile_osm_extract_bytes;

    /// Grid spacing of the hand-built test graphs: 0.001 degrees latitude,
    /// about 111 m along a meridian (the fixture's segment distance).
    const GRID_SEGMENT_DEGREES: f64 = 0.001;
    /// The same spacing as the archive's truncated distance column stores it.
    const GRID_SEGMENT_METERS: u32 = 111;
    /// The travel duration the test segments' duration column stores, in
    /// seconds (13,320 ms at the 30 km/h residential default).
    const GRID_SEGMENT_DURATION_SECONDS: f64 = 13.32;

    fn coordinate(latitude_degrees: f64, longitude_degrees: f64) -> GeographicCoordinate {
        GeographicCoordinate::from_degrees(latitude_degrees, longitude_degrees)
            .expect("test coordinates are within the canonical ranges")
    }

    fn automobile_options(route_strategy: RouteStrategy) -> RouteOptions {
        RouteOptions {
            transportation_mode: TransportationMode::Automobile,
            route_strategy,
            unit_system: UnitSystem::Metric,
            avoid_options: AvoidOptions::default(),
            departure_timestamp_seconds: None,
        }
    }

    /// Builds a test archive by encoding it and reading it back, so tests
    /// exercise the same validated shapes production loads produce.
    fn loaded_graph_from(
        nodes: Vec<NodeRecord>,
        edges: Vec<EdgeRecord>,
        turn_restrictions: Vec<TurnRestrictionRecord>,
    ) -> LoadedRoutingGraph {
        let archive_bytes = OfflineRoutingNetworkGraphBuilder::new()
            .with_nodes(nodes)
            .with_edges(edges)
            .with_lane_guidances(Vec::new())
            .with_instruction_strings(Vec::new())
            .with_instruction_lookup_indices(vec![
                INSTRUCTION_ABSENT_INDEX;
                turn_restrictions.len()
            ])
            .with_turn_restrictions(turn_restrictions)
            .encode_deterministic()
            .expect("hand-built test archives are self-consistent");
        LoadedRoutingGraph::new(
            OfflineRoutingNetworkGraph::from_bytes(&archive_bytes)
                .expect("hand-built test archives validate"),
        )
    }

    fn residental_forward(source: u32, target: u32) -> EdgeRecord {
        EdgeRecord::new_directed(
            source,
            target,
            GRID_SEGMENT_METERS,
            13_320,
            EdgeProperties {
                road_class: RoadClass::Residential,
                speed_kilometers_per_hour: 30,
                forward_allowed: true,
                backward_allowed: false,
            },
        )
    }

    #[test]
    fn astar_follows_the_strategy_weight_between_parallel_links() {
        // Two parallel links between the same pair of nodes with
        // deliberately conflicting distance/duration columns.
        let loaded_graph = loaded_graph_from(
            vec![
                NodeRecord::from_geographic_degrees(0.0, 0.0),
                NodeRecord::from_geographic_degrees(0.0, GRID_SEGMENT_DEGREES),
            ],
            vec![
                EdgeRecord::new_directed(
                    0,
                    1,
                    200,
                    60_000,
                    EdgeProperties {
                        road_class: RoadClass::Residential,
                        speed_kilometers_per_hour: 30,
                        forward_allowed: true,
                        backward_allowed: false,
                    },
                ),
                EdgeRecord::new_directed(
                    0,
                    1,
                    500,
                    20_000,
                    EdgeProperties {
                        road_class: RoadClass::Motorway,
                        speed_kilometers_per_hour: 150,
                        forward_allowed: true,
                        backward_allowed: false,
                    },
                ),
            ],
            Vec::new(),
        );

        let fastest_route = compute_route(&loaded_graph, 0, 1, TraversalCost::TimeSeconds)
            .expect("the parallel links connect the nodes");
        assert_eq!(fastest_route.distance_meters, 500.0);
        assert_eq!(fastest_route.duration_seconds, 20.0);

        let shortest_route = compute_route(&loaded_graph, 0, 1, TraversalCost::DistanceMeters)
            .expect("the parallel links connect the nodes");
        assert_eq!(shortest_route.distance_meters, 200.0);
        assert_eq!(shortest_route.duration_seconds, 60.0);
    }

    #[test]
    fn no_turn_restriction_blocks_its_edge_pair() {
        let nodes = vec![
            NodeRecord::from_geographic_degrees(0.0, 0.0),
            NodeRecord::from_geographic_degrees(0.0, GRID_SEGMENT_DEGREES),
            NodeRecord::from_geographic_degrees(0.0, 2.0 * GRID_SEGMENT_DEGREES),
        ];
        // Edges 0 and 1 form the direct two-segment path; edge 2 goes
        // 0 -> 2 the long way around.
        let edge_records = vec![
            residental_forward(0, 1),
            residental_forward(1, 2),
            EdgeRecord::new_directed(
                0,
                2,
                500,
                40_000,
                EdgeProperties {
                    road_class: RoadClass::Residential,
                    speed_kilometers_per_hour: 30,
                    forward_allowed: true,
                    backward_allowed: false,
                },
            ),
        ];

        let unrestricted = loaded_graph_from(nodes.clone(), edge_records.clone(), Vec::new());
        let fastest_short_route = compute_route(&unrestricted, 0, 2, TraversalCost::TimeSeconds)
            .expect("the direct path connects the nodes");
        assert_eq!(
            fastest_short_route.distance_meters,
            2.0 * f64::from(GRID_SEGMENT_METERS)
        );

        let restricted = loaded_graph_from(
            nodes,
            edge_records,
            vec![TurnRestrictionRecord::new(
                0,
                1,
                1,
                RestrictionKind::NoLeftTurn,
            )],
        );
        let forced_around_route = compute_route(&restricted, 0, 2, TraversalCost::TimeSeconds)
            .expect("the long way around remains drivable");
        assert_eq!(forced_around_route.distance_meters, 500.0);
    }

    #[test]
    fn only_straight_on_restriction_blocks_every_other_to_edge() {
        let nodes = vec![
            NodeRecord::from_geographic_degrees(0.0, 0.0),
            NodeRecord::from_geographic_degrees(0.0, GRID_SEGMENT_DEGREES),
            NodeRecord::from_geographic_degrees(0.0, 2.0 * GRID_SEGMENT_DEGREES),
            NodeRecord::from_geographic_degrees(GRID_SEGMENT_DEGREES, GRID_SEGMENT_DEGREES),
        ];
        // From edge 0 at node 1, edge 1 goes on straight and edge 2 turns
        // toward node 3.
        let edge_records = vec![
            residental_forward(0, 1),
            residental_forward(1, 2),
            residental_forward(1, 3),
        ];
        let restricted = loaded_graph_from(
            nodes,
            edge_records,
            vec![TurnRestrictionRecord::new(
                0,
                1,
                1,
                RestrictionKind::OnlyStraightOn,
            )],
        );

        let straight_on_route = compute_route(&restricted, 0, 2, TraversalCost::TimeSeconds)
            .expect("the only-allowed straight-on leg reaches node 2");
        assert_eq!(
            straight_on_route.distance_meters,
            2.0 * f64::from(GRID_SEGMENT_METERS)
        );
        let turn_route_failure = compute_route(&restricted, 0, 3, TraversalCost::TimeSeconds)
            .expect_err("the restricted turn is the only approach to node 3");
        assert_eq!(
            turn_route_failure,
            SDKError::ProviderRequestRejected(
                "the offline routing graph cannot connect the request points; \
                 no drivable path exists between the snapped nodes"
                    .to_string()
            )
        );
    }

    #[test]
    fn snapping_covers_the_named_tolerance_and_rejects_beyond_it() {
        let loaded_graph = loaded_graph_from(
            vec![
                NodeRecord::from_geographic_degrees(0.0, 0.0),
                NodeRecord::from_geographic_degrees(0.0, GRID_SEGMENT_DEGREES),
            ],
            vec![residental_forward(0, 1)],
            Vec::new(),
        );

        // A third of the tolerance away from node 0 still snaps there.
        let offset_within_tolerance = coordinate(0.0003, 0.0);
        assert_eq!(
            loaded_graph
                .snap_node_index(&offset_within_tolerance)
                .expect("the offset stays within the snap tolerance"),
            0
        );
        // A full segment away fails the tolerance.
        let far_coordinate = coordinate(0.0, 10.0 * GRID_SEGMENT_DEGREES);
        assert!(loaded_graph.snap_node_index(&far_coordinate).is_err());
    }

    #[tokio::test]
    async fn non_automobile_modes_are_rejected_instead_of_answered() {
        let loaded_graph = loaded_graph_from(
            vec![
                NodeRecord::from_geographic_degrees(0.0, 0.0),
                NodeRecord::from_geographic_degrees(0.0, GRID_SEGMENT_DEGREES),
            ],
            vec![residental_forward(0, 1)],
            Vec::new(),
        );
        let pedestrian_options = RouteOptions {
            transportation_mode: TransportationMode::Pedestrian,
            route_strategy: RouteStrategy::Fastest,
            unit_system: UnitSystem::Metric,
            avoid_options: AvoidOptions::default(),
            departure_timestamp_seconds: None,
        };
        let provider = ArchiveOfflineRoutingProvider {
            loaded_graph: RwLock::new(Some(Arc::new(loaded_graph))),
            blocking_work_executor: Arc::new(InlineBlockingWorkExecutor),
        };
        let start = coordinate(0.0, 0.0);
        let end = coordinate(0.0, GRID_SEGMENT_DEGREES);
        match provider
            .calculate_offline_route(&start, &end, &pedestrian_options)
            .await
        {
            Err(SDKError::ProviderRequestRejected(_)) => {}
            other => panic!("expected a rejected pedestrian request, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn calculate_without_a_loaded_graph_reports_a_rejected_request() {
        let provider = ArchiveOfflineRoutingProvider::new();
        let start = coordinate(0.0, 0.0);
        let end = coordinate(0.0, GRID_SEGMENT_DEGREES);
        match provider
            .calculate_offline_route(&start, &end, &automobile_options(RouteStrategy::Fastest))
            .await
        {
            Err(SDKError::ProviderRequestRejected(_)) => {}
            other => panic!("expected a not-loaded-graph rejection, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn failed_load_keeps_the_previously_loaded_graph() {
        let provider = ArchiveOfflineRoutingProvider::new();
        let archive_bytes = OfflineRoutingNetworkGraphBuilder::new()
            .with_nodes(vec![
                NodeRecord::from_geographic_degrees(0.0, 0.0),
                NodeRecord::from_geographic_degrees(0.0, GRID_SEGMENT_DEGREES),
            ])
            .with_edges(vec![residental_forward(0, 1)])
            .with_lane_guidances(Vec::new())
            .with_instruction_strings(Vec::new())
            .with_instruction_lookup_indices(Vec::new())
            .with_turn_restrictions(Vec::new())
            .encode_deterministic()
            .expect("the hand-built archive is self-consistent");

        provider
            .load_offline_graph(OfflineGraphSource::Memory(archive_bytes.clone()))
            .await
            .expect("the test archive loads");

        let mut corrupted_bytes = archive_bytes.clone();
        corrupted_bytes[0] = b'X';
        let failed_load = provider
            .load_offline_graph(OfflineGraphSource::Memory(corrupted_bytes))
            .await
            .expect_err("the corrupted archive fails validation");
        assert_eq!(
            failed_load,
            SDKError::OfflineRoutingFailure(OfflineGraphInvalidReason::ArchiveMalformed)
        );

        // The previously loaded graph is still the one answering routes.
        let start = coordinate(0.0, 0.0);
        let end = coordinate(0.0, GRID_SEGMENT_DEGREES);
        let route = provider
            .calculate_offline_route(&start, &end, &automobile_options(RouteStrategy::Fastest))
            .await
            .expect("the first graph still answers routes after the failed load");
        assert_eq!(route.distance_meters, GRID_SEGMENT_METERS as f64);
    }

    #[test]
    fn collinear_run_materializes_a_single_departure_step() {
        // Three colinear nodes on one road: no junction turn, no class
        // change — one Departure step spanning both edges, then Arrival.
        let loaded_graph = loaded_graph_from(
            vec![
                NodeRecord::from_geographic_degrees(0.0, 0.0),
                NodeRecord::from_geographic_degrees(0.0, GRID_SEGMENT_DEGREES),
                NodeRecord::from_geographic_degrees(0.0, 2.0 * GRID_SEGMENT_DEGREES),
            ],
            vec![residental_forward(0, 1), residental_forward(1, 2)],
            Vec::new(),
        );
        let route = compute_route(&loaded_graph, 0, 2, TraversalCost::TimeSeconds)
            .expect("the two colinear segments connect the node chain");
        let steps = &route.legs[0].steps;
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].maneuver_type, ManeuverType::Departure);
        // The fallback of the end-anchored rule: with no junction maneuver
        // anywhere on the route, the departure span keeps its own authored
        // text — the departure text is only ever hidden when a following
        // maneuver's text replaces it.
        assert_eq!(
            steps[0].instruction_display_text,
            DEPARTURE_INSTRUCTION_TEXT
        );
        assert_eq!(
            steps[0].distance_meters,
            2.0 * f64::from(GRID_SEGMENT_METERS)
        );
        assert_eq!(steps[0].shape.len(), 3);
        assert_eq!(steps[1].maneuver_type, ManeuverType::Arrival);
        assert_eq!(steps[1].distance_meters, 0.0);
        assert_eq!(steps[1].shape.len(), 1);
        // Quantized archive coordinates round-trip with float noise; assert
        // against a resolution-derived tolerance.
        assert!(
            (route.bounding_box.south_west.latitude - 0.0).abs() < 1e-6
                && (route.bounding_box.south_west.longitude - 0.0).abs() < 1e-6
                && (route.bounding_box.north_east.latitude - 0.0).abs() < 1e-6
                && (route.bounding_box.north_east.longitude - 2.0 * GRID_SEGMENT_DEGREES).abs()
                    < 1e-6
        );
    }

    #[test]
    fn l_shaped_route_materializes_a_derived_left_turn_step() {
        // The ninety-degree junction of the L shape derives a standard left
        // turn (eastbound street onto a northbound street), splitting the
        // departure run even though the road class and speed match.
        let loaded_graph = loaded_graph_from(
            vec![
                NodeRecord::from_geographic_degrees(0.0, 0.0),
                NodeRecord::from_geographic_degrees(0.0, GRID_SEGMENT_DEGREES),
                NodeRecord::from_geographic_degrees(GRID_SEGMENT_DEGREES, GRID_SEGMENT_DEGREES),
            ],
            vec![residental_forward(0, 1), residental_forward(1, 2)],
            Vec::new(),
        );
        let route = compute_route(&loaded_graph, 0, 2, TraversalCost::TimeSeconds)
            .expect("the two segments connect the node chain");
        let steps = &route.legs[0].steps;
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].maneuver_type, ManeuverType::Departure);
        // End-anchored text: the departure step's banner names the left turn
        // at its *end* junction (the junction the countdown points at), never
        // the departure maneuver it opens with.
        assert_eq!(steps[0].instruction_display_text, "Turn left");
        assert_eq!(steps[0].instruction_speech_text, "Turn left");
        assert_eq!(steps[0].distance_meters, f64::from(GRID_SEGMENT_METERS));
        assert_eq!(steps[0].shape.len(), 2);
        assert_eq!(
            steps[1].maneuver_type,
            ManeuverType::Turn(TurnDirection::Left)
        );
        // The route's final traversal span has no following junction
        // maneuver, so it keeps its own authored (head-anchored) turn text.
        assert_eq!(steps[1].instruction_display_text, "Turn left");
        assert_eq!(steps[1].instruction_speech_text, "Turn left");
        assert_eq!(steps[1].distance_meters, f64::from(GRID_SEGMENT_METERS));
        assert_eq!(steps[1].shape.len(), 2);
        assert_eq!(steps[2].maneuver_type, ManeuverType::Arrival);
        assert_eq!(steps[2].instruction_display_text, ARRIVAL_INSTRUCTION_TEXT);
        assert_eq!(steps[2].distance_meters, 0.0);
    }

    #[test]
    fn step_texts_carry_the_maneuver_at_the_step_end() {
        // The end-anchored banner convention of the offline fallback, named
        // after the provider test of the same rule (the MapQuest provider): a
        // step's display/speech text is the authored maneuver text at the
        // step's *end* — the opening maneuver of the span following it, the
        // junction the countdown points at — while the head-anchored
        // `ManeuverType` stays untouched, the final traversal span keeps its
        // own authored text, and the terminal arrival step keeps its arrival
        // text. Route shape: east, a ninety-degree junction deriving a left
        // turn, then a second ninety-degree junction deriving a right turn.
        let loaded_graph = loaded_graph_from(
            vec![
                NodeRecord::from_geographic_degrees(0.0, 0.0),
                NodeRecord::from_geographic_degrees(0.0, GRID_SEGMENT_DEGREES),
                NodeRecord::from_geographic_degrees(GRID_SEGMENT_DEGREES, GRID_SEGMENT_DEGREES),
                NodeRecord::from_geographic_degrees(
                    GRID_SEGMENT_DEGREES,
                    2.0 * GRID_SEGMENT_DEGREES,
                ),
            ],
            vec![
                residental_forward(0, 1),
                residental_forward(1, 2),
                residental_forward(2, 3),
            ],
            Vec::new(),
        );
        let route = compute_route(&loaded_graph, 0, 3, TraversalCost::TimeSeconds)
            .expect("the three segments connect the node chain");
        let steps = &route.legs[0].steps;
        assert_eq!(steps.len(), 4);
        // Each span's text is the turn derived at the junction closing it;
        // each span's maneuver type is the maneuver opening it.
        assert_eq!(steps[0].maneuver_type, ManeuverType::Departure);
        assert_eq!(steps[0].instruction_display_text, "Turn left");
        assert_eq!(steps[0].instruction_speech_text, "Turn left");
        assert_eq!(
            steps[1].maneuver_type,
            ManeuverType::Turn(TurnDirection::Left)
        );
        assert_eq!(steps[1].instruction_display_text, "Turn right");
        assert_eq!(steps[1].instruction_speech_text, "Turn right");
        assert_eq!(
            steps[2].maneuver_type,
            ManeuverType::Turn(TurnDirection::Right)
        );
        assert_eq!(steps[2].instruction_display_text, "Turn right");
        assert_eq!(steps[2].instruction_speech_text, "Turn right");
        assert_eq!(steps[3].maneuver_type, ManeuverType::Arrival);
        // The appended arrival step is the only carrier of the arrival text
        // (the deliberate offline divergence from the provider rule,
        // documented on `end_anchored_instruction_text`: the arrival is not
        // a span-authored text).
        assert_eq!(steps[3].instruction_display_text, ARRIVAL_INSTRUCTION_TEXT);
        assert_eq!(steps[3].instruction_speech_text, ARRIVAL_INSTRUCTION_TEXT);

        // The fallback's first-step corollary: with no junction maneuver
        // anywhere on the route, the departure text is the one text there
        // is — the single departure span keeps its own authored text exactly
        // as the start-equals-end route does, and the arrival keeps its own.
        let collinear_graph = loaded_graph_from(
            vec![
                NodeRecord::from_geographic_degrees(0.0, 0.0),
                NodeRecord::from_geographic_degrees(0.0, GRID_SEGMENT_DEGREES),
                NodeRecord::from_geographic_degrees(0.0, 2.0 * GRID_SEGMENT_DEGREES),
            ],
            vec![residental_forward(0, 1), residental_forward(1, 2)],
            Vec::new(),
        );
        let collinear_route = compute_route(&collinear_graph, 0, 2, TraversalCost::TimeSeconds)
            .expect("the colinear segments connect the node chain");
        let collinear_steps = &collinear_route.legs[0].steps;
        assert_eq!(collinear_steps.len(), 2);
        assert_eq!(
            collinear_steps[0].instruction_display_text,
            DEPARTURE_INSTRUCTION_TEXT
        );
        assert_eq!(
            collinear_steps[0].instruction_speech_text,
            DEPARTURE_INSTRUCTION_TEXT
        );
        assert_eq!(
            collinear_steps[1].instruction_display_text,
            ARRIVAL_INSTRUCTION_TEXT
        );
        assert_eq!(
            collinear_steps[1].instruction_speech_text,
            ARRIVAL_INSTRUCTION_TEXT
        );
    }

    #[test]
    fn class_change_without_a_turn_materializes_a_continue_step() {
        // A colinear class/speed change (residential way onto a service way)
        // opens a Continue step, not a fabricated turn.
        let loaded_graph = loaded_graph_from(
            vec![
                NodeRecord::from_geographic_degrees(0.0, 0.0),
                NodeRecord::from_geographic_degrees(0.0, GRID_SEGMENT_DEGREES),
                NodeRecord::from_geographic_degrees(0.0, 2.0 * GRID_SEGMENT_DEGREES),
            ],
            vec![
                residental_forward(0, 1),
                EdgeRecord::new_directed(
                    1,
                    2,
                    GRID_SEGMENT_METERS,
                    19_980,
                    EdgeProperties {
                        road_class: RoadClass::Service,
                        speed_kilometers_per_hour: 20,
                        forward_allowed: true,
                        backward_allowed: false,
                    },
                ),
            ],
            Vec::new(),
        );
        let route = compute_route(&loaded_graph, 0, 2, TraversalCost::TimeSeconds)
            .expect("the two segments connect the node chain");
        let steps = &route.legs[0].steps;
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].maneuver_type, ManeuverType::Departure);
        assert_eq!(
            steps[1].maneuver_type,
            ManeuverType::Continue,
            "a straight-on class change is a Continue maneuver"
        );
        // End-anchored text: the departure step adopts the Continue of the
        // straight-on junction closing it; the Continue span itself is the
        // final traversal span and keeps its own authored text.
        assert_eq!(steps[0].instruction_display_text, CONTINUE_INSTRUCTION_TEXT);
        assert_eq!(steps[0].instruction_speech_text, CONTINUE_INSTRUCTION_TEXT);
        assert_eq!(steps[1].instruction_display_text, CONTINUE_INSTRUCTION_TEXT);
        assert_eq!(steps[1].instruction_speech_text, CONTINUE_INSTRUCTION_TEXT);
        assert_eq!(steps[2].maneuver_type, ManeuverType::Arrival);
    }

    #[test]
    fn out_and_back_route_materializes_a_u_turn_step() {
        // Driving onto a node and immediately back along the same edge's
        // reverse direction is the full-reversal geometry: the step splits
        // into a U-turn maneuver.
        let loaded_graph: LoadedRoutingGraph = loaded_graph_from(
            vec![
                NodeRecord::from_geographic_degrees(0.0, 0.0),
                NodeRecord::from_geographic_degrees(0.0, GRID_SEGMENT_DEGREES),
            ],
            vec![residental_forward(0, 1), residental_forward(1, 0)],
            Vec::new(),
        );
        // A hand-built route chain arriving at node 1 on edge 0 and leaving
        // it on edge 1: the out-and-back geometry the derived maneuver reads.
        let node_chain_indices = vec![0, 1, 0];
        let route_half_edges = vec![
            loaded_graph.outgoing_half_edges[0][0],
            loaded_graph.outgoing_half_edges[1][0],
        ];
        let computed_route = ComputedRoute {
            node_chain_indices,
            route_half_edges,
            settled_node_count: 0,
            total_cost: 2.0 * GRID_SEGMENT_DURATION_SECONDS,
        };
        let route = materialize_route_data(&loaded_graph, &computed_route);
        let steps = &route.legs[0].steps;
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].maneuver_type, ManeuverType::Departure);
        assert_eq!(
            steps[1].maneuver_type,
            ManeuverType::Turn(TurnDirection::UTurn),
            "a full reversal of direction derives a U-turn, never a guessed left/right"
        );
        // End-anchored text: the U-turn is derived at the span boundary, so
        // both the departure step (whose end the U-turn junction closes) and
        // the U-turn span itself (last span, own text) carry the U-turn text.
        assert_eq!(steps[0].instruction_display_text, "U-turn");
        assert_eq!(steps[0].instruction_speech_text, "U-turn");
        assert_eq!(steps[1].instruction_display_text, "U-turn");
        assert_eq!(steps[2].maneuver_type, ManeuverType::Arrival);
        assert_eq!(route.duration_seconds, 2.0 * GRID_SEGMENT_DURATION_SECONDS);
    }

    #[test]
    fn derived_turn_direction_covers_the_bearing_change_bands() {
        // Straight band: no maneuver either way, at the band edges too.
        assert_eq!(derived_turn_direction(0.0), None);
        assert_eq!(derived_turn_direction(-20.0), None);
        assert_eq!(derived_turn_direction(20.0), None);
        // Slight band (exclusive at 20, inclusive at 60).
        assert_eq!(
            derived_turn_direction(21.0),
            Some(TurnDirection::SlightRight)
        );
        assert_eq!(
            derived_turn_direction(-45.0),
            Some(TurnDirection::SlightLeft)
        );
        assert_eq!(
            derived_turn_direction(60.0),
            Some(TurnDirection::SlightRight)
        );
        // Standard band.
        assert_eq!(derived_turn_direction(-90.0), Some(TurnDirection::Left));
        assert_eq!(derived_turn_direction(90.0), Some(TurnDirection::Right));
        assert_eq!(derived_turn_direction(120.0), Some(TurnDirection::Right));
        // Sharp band.
        assert_eq!(
            derived_turn_direction(121.0),
            Some(TurnDirection::SharpRight)
        );
        assert_eq!(
            derived_turn_direction(-160.0),
            Some(TurnDirection::SharpLeft)
        );
        // Beyond the sharp band: the full reversal.
        assert_eq!(derived_turn_direction(161.0), Some(TurnDirection::UTurn));
        assert_eq!(derived_turn_direction(-180.0), Some(TurnDirection::UTurn));
        assert_eq!(derived_turn_direction(180.0), Some(TurnDirection::UTurn));
    }

    #[test]
    fn committed_routing_grid_fixture_materializes_derived_turn_maneuvers() {
        // The committed fixture's routing-grid extract, compiled through the
        // production compiler: the L-shaped way and the oneway street meet at
        // right angles, so fixture routes report honest derived maneuvers.
        let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(concat!(
            "..",
            "/rustic-maps-offline-graph-compiler/fixtures/osm/routing_grid.osm.pbf"
        ));
        let fixture_pbf_bytes = std::fs::read(&fixture_path)
            .expect("the committed routing-grid fixture exists beside the compiler crate");
        let archive_bytes =
            compile_osm_extract_bytes(&fixture_pbf_bytes).expect("the committed fixture compiles");
        let network_graph = OfflineRoutingNetworkGraph::from_bytes(&archive_bytes)
            .expect("the compiled fixture reads back validated");
        // Fixture node order (see the compiler's round-trip test): 1001,
        // 1002, 1004, 1005, 1006, 1008 at indices 0..=5.
        let fixture_graph = LoadedRoutingGraph::new(network_graph);
        assert_eq!(fixture_graph.node_coordinates.len(), 6);

        // 1005 -> 1001: south along the L's spine, then the ninety-degree
        // junction at 1002 turns the route west — a derived right turn.
        let turn_route = compute_route(&fixture_graph, 3, 0, TraversalCost::TimeSeconds)
            .expect("the spine-and-junction path connects 1005 to 1001");
        let steps = &turn_route.legs[0].steps;
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].maneuver_type, ManeuverType::Departure);
        // End-anchored text: the departure step's banner names the right
        // turn at its end junction (1002), not its own departure maneuver.
        assert_eq!(steps[0].instruction_display_text, "Turn right");
        assert_eq!(steps[0].instruction_speech_text, "Turn right");
        assert_eq!(steps[0].distance_meters, f64::from(GRID_SEGMENT_METERS));
        assert_eq!(
            steps[1].maneuver_type,
            ManeuverType::Turn(TurnDirection::Right)
        );
        // The final traversal span keeps its own authored turn text.
        assert_eq!(steps[1].instruction_display_text, "Turn right");
        assert_eq!(steps[1].distance_meters, f64::from(GRID_SEGMENT_METERS));
        assert_eq!(steps[2].maneuver_type, ManeuverType::Arrival);
        assert_eq!(steps[2].distance_meters, 0.0);

        // 1001 -> 1005: east along the L's first segment, then the
        // ninety-degree junction at 1002 turns the route north — a derived
        // left turn splitting the departure run.
        let opposite_route = compute_route(&fixture_graph, 0, 3, TraversalCost::TimeSeconds)
            .expect("the spine-and-junction path connects 1001 to 1005");
        let steps = &opposite_route.legs[0].steps;
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].maneuver_type, ManeuverType::Departure);
        assert_eq!(steps[0].distance_meters, f64::from(GRID_SEGMENT_METERS));
        assert_eq!(
            steps[1].maneuver_type,
            ManeuverType::Turn(TurnDirection::Left)
        );
        assert_eq!(steps[2].maneuver_type, ManeuverType::Arrival);
        assert_eq!(steps[2].distance_meters, 0.0);

        // 1004 -> 1001: east, then south, then west — two derived right turns
        // at consecutive junctions (the via-node restriction steers the A*
        // search away from the 1004 -> 1003 -> ... branch once it is priced,
        // and the dead end behind 1006 cannot carry the route either).
        let double_turn_route = compute_route(&fixture_graph, 2, 0, TraversalCost::TimeSeconds)
            .expect("the corner path connects 1004 to 1001");
        let steps = &double_turn_route.legs[0].steps;
        assert_eq!(steps.len(), 4);
        assert_eq!(steps[0].maneuver_type, ManeuverType::Departure);
        assert_eq!(
            steps[1].maneuver_type,
            ManeuverType::Turn(TurnDirection::Right)
        );
        assert_eq!(
            steps[2].maneuver_type,
            ManeuverType::Turn(TurnDirection::Right)
        );
        assert_eq!(steps[3].maneuver_type, ManeuverType::Arrival);
        assert_eq!(
            double_turn_route.distance_meters,
            3.0 * f64::from(GRID_SEGMENT_METERS)
        );
    }

    #[test]
    fn start_equal_to_end_route_reports_a_zero_cost_round_trip() {
        let loaded_graph = loaded_graph_from(
            vec![NodeRecord::from_geographic_degrees(0.0, 0.0)],
            Vec::new(),
            Vec::new(),
        );
        let route = compute_route(&loaded_graph, 0, 0, TraversalCost::TimeSeconds)
            .expect("an identical start and end needs no traversal");
        assert_eq!(route.distance_meters, 0.0);
        assert_eq!(route.duration_seconds, 0.0);
        assert_eq!(route.legs.len(), 1);
        let steps = &route.legs[0].steps;
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].maneuver_type, ManeuverType::Departure);
        assert_eq!(steps[1].maneuver_type, ManeuverType::Arrival);
    }

    /// A scratch archive file that deletes itself when the test ends (the
    /// `FilePath` seam's load input).
    struct TemporaryArchiveFile {
        path: std::path::PathBuf,
    }

    impl TemporaryArchiveFile {
        fn write_with(archive_bytes: &[u8]) -> Self {
            static TEMPORARY_FILE_COUNTER: std::sync::atomic::AtomicUsize =
                std::sync::atomic::AtomicUsize::new(0);
            let unique_counter =
                TEMPORARY_FILE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let path = std::env::temp_dir().join(format!(
                "rustic-maps-memory-equality-test-{}-{unique_counter}.rmog",
                std::process::id()
            ));
            std::fs::write(&path, archive_bytes)
                .expect("the test archive writes to the temp directory");
            Self { path }
        }
    }

    impl Drop for TemporaryArchiveFile {
        fn drop(&mut self) {
            std::fs::remove_file(&self.path).ok();
        }
    }

    /// The wasm-path guarantee, proven on native: the same compiled archive
    /// bytes route identically whether the provider loads them through the
    /// `Memory` seam (the only wasm source) or through the memory-mapped
    /// file seam (the native source) — both feed one validating reader. The
    /// committed fixture is compiled in-process; no network anywhere.
    #[tokio::test]
    async fn fixture_routes_identically_from_memory_and_mapped_file_sources() {
        let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(concat!(
            "..",
            "/rustic-maps-offline-graph-compiler/fixtures/osm/routing_grid.osm.pbf"
        ));
        let archive_bytes = compile_osm_extract_bytes(
            &std::fs::read(&fixture_path).expect("the committed routing-grid fixture exists"),
        )
        .expect("the committed fixture compiles");
        let temporary_file = TemporaryArchiveFile::write_with(&archive_bytes);

        async fn route_from_source(graph_source: OfflineGraphSource) -> RouteData {
            let provider = ArchiveOfflineRoutingProvider::new();
            provider
                .load_offline_graph(graph_source)
                .await
                .expect("both load seams validate the compiled fixture");
            provider
                .calculate_offline_route(
                    &coordinate(0.001, 0.000),
                    &coordinate(0.001, 0.002),
                    &automobile_options(RouteStrategy::Fastest),
                )
                .await
                .expect("the fixture graph connects the request points")
        }

        let route_from_memory_bytes =
            route_from_source(OfflineGraphSource::Memory(archive_bytes.clone())).await;
        let route_from_mapped_file = route_from_source(OfflineGraphSource::FilePath(
            temporary_file.path.to_string_lossy().to_string(),
        ))
        .await;

        assert_eq!(route_from_memory_bytes, route_from_mapped_file);
        // The route content itself: the oneway street's two segments.
        assert_eq!(
            route_from_memory_bytes.distance_meters,
            2.0 * f64::from(GRID_SEGMENT_METERS)
        );
    }
}
