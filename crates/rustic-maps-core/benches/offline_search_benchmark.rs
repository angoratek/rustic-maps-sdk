//! Criterion benchmark suite for the offline routing engine: the
//! benchmark gate for the recorded baseline behind `PRD.md` §4.6's
//! "Dijkstra upgraded to A*".
//!
//! Two groups:
//!
//! 1. `offline_search_astar_vs_dijkstra` — A* head-to-head against
//!    [`rustic_maps_core::offline_routing::dijkstra_reference_search`] over the
//!    real loaded-graph structures of deterministic synthetic graphs (the
//!    `offline_routing_benchmark` generators: a street grid and a random
//!    spanning topology), corner-to-corner, `Fastest` weighting.
//! 2. `offline_archive_compile_and_load` — the fixture-scale PBF→archive
//!    compile + validating load + routing-index build, plus the load and
//!    index-build halves at a synthetic scale where the column counts
//!    dominate.
//!
//! These numbers are the **recorded baseline for local trend tracking**
//! (uploaded as artifacts by the path-filtered `benchmark` workflow), NOT a
//! CI pass/fail: wall clock on shared runners is noisy, and the enforced
//! alarm is the coarse budget test in `offline_routing_benchmark` instead.
//!
//! Native-only (criterion + the wasm-free fixture compiler; benches are
//! never part of the wasm gate, which is lib-only).

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use rustic_maps_core::offline_routing::{
    LoadedRoutingGraph, TraversalCost, a_star_search, dijkstra_reference_search,
};
use rustic_maps_core::offline_routing_benchmark::{
    BENCHMARK_GRID_SIDE_LENGTHS, BENCHMARK_RANDOM_SPANNING_NODE_COUNTS,
    BENCHMARK_RANDOM_SPANNING_SEED, BENCHMARK_SPANNING_LOAD_BENCH_NODE_COUNT,
    benchmark_corner_query_pair, benchmark_grid_loaded_graph,
    benchmark_random_spanning_archive_bytes, benchmark_random_spanning_loaded_graph,
};
use rustic_maps_offline_graph_archive::archive::OfflineRoutingNetworkGraph;
use rustic_maps_offline_graph_compiler::compile_osm_extract_bytes;

/// The fixture's committed PBF extract, compiled in-process (network-free;
/// see the module docs' baseline contract).
const COMMITTED_FIXTURE_PBF_BYTES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../rustic-maps-offline-graph-compiler/fixtures/osm/routing_grid.osm.pbf"
);

/// The `Fastest` weighting (durations) is the benchmark's recorded strategy:
/// it is the default routing strategy and the one the haversine heuristic
/// most aggressively prunes. The equality tests cover `Shortest` too.
const BENCHMARKED_STRATEGY: TraversalCost = TraversalCost::TimeSeconds;

/// Searches both engines across the graph once (the bencher body shared by
/// the A* and the Dijkstra-reference benches of one graph).
fn search_corner_query(
    loaded_graph: &LoadedRoutingGraph,
    (start_node_index, end_node_index): (u32, u32),
    dijkstra_reference: bool,
) {
    if dijkstra_reference {
        dijkstra_reference_search(
            black_box(loaded_graph),
            start_node_index,
            end_node_index,
            BENCHMARKED_STRATEGY,
        )
        .expect("the benchmark graph connects its corner query");
    } else {
        a_star_search(
            black_box(loaded_graph),
            start_node_index,
            end_node_index,
            BENCHMARKED_STRATEGY,
        )
        .expect("the benchmark graph connects its corner query");
    }
}

/// Group 1: A* vs the Dijkstra reference on the deterministic synthetic
/// graphs, corner-to-corner.
fn benchmark_search_group(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("offline_search_astar_vs_dijkstra");
    for &side_length in &BENCHMARK_GRID_SIDE_LENGTHS {
        let loaded_graph =
            benchmark_grid_loaded_graph(side_length).expect("the grid benchmark graph loads");
        let corner_query = benchmark_corner_query_pair(side_length * side_length);
        for (engine_name, dijkstra_reference) in [("astar", false), ("dijkstra_reference", true)] {
            group.bench_with_input(
                BenchmarkId::new(engine_name, format!("{side_length}x{side_length}")),
                &(dijkstra_reference, corner_query),
                |bencher, &(dijkstra_reference, corner_query)| {
                    bencher.iter(|| {
                        search_corner_query(&loaded_graph, corner_query, dijkstra_reference);
                    });
                },
            );
        }
    }
    for &node_count in &BENCHMARK_RANDOM_SPANNING_NODE_COUNTS {
        let loaded_graph =
            benchmark_random_spanning_loaded_graph(node_count, BENCHMARK_RANDOM_SPANNING_SEED)
                .expect("the spanning benchmark graph loads");
        let corner_query = benchmark_corner_query_pair(node_count as u32);
        for (engine_name, dijkstra_reference) in [("astar", false), ("dijkstra_reference", true)] {
            group.bench_with_input(
                BenchmarkId::new(engine_name, node_count),
                &(dijkstra_reference, corner_query),
                |bencher, &(dijkstra_reference, corner_query)| {
                    bencher.iter(|| {
                        search_corner_query(&loaded_graph, corner_query, dijkstra_reference);
                    });
                },
            );
        }
    }
    group.finish();
}

/// Group 2: the compile-and-load path, fixture-scale and synthetic-scale.
fn benchmark_archive_compile_and_load_group(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("offline_archive_compile_and_load");

    // The committed fixture compiled in-process, then loaded and indexed:
    // the full production load path at fixture scale.
    let fixture_pbf_bytes = std::fs::read(COMMITTED_FIXTURE_PBF_BYTES)
        .expect("the committed routing-grid fixture exists beside the compiler crate");
    group.bench_function("compile_and_load_routing_grid_fixture", |bencher| {
        bencher.iter(|| {
            let archive_bytes = compile_osm_extract_bytes(black_box(&fixture_pbf_bytes))
                .expect("the committed fixture compiles");
            let network_graph = OfflineRoutingNetworkGraph::from_bytes(&archive_bytes)
                .expect("the compiled fixture validates");
            LoadedRoutingGraph::new(network_graph)
        });
    });

    // The load halves at a synthetic scale, so regression alarms distinguish
    // validation from index building when the columns grow.
    let synthetic_archive_bytes = benchmark_random_spanning_archive_bytes(
        BENCHMARK_SPANNING_LOAD_BENCH_NODE_COUNT,
        BENCHMARK_RANDOM_SPANNING_SEED,
    )
    .expect("the synthetic load benchmark graph encodes");
    group.throughput(criterion::Throughput::Bytes(
        synthetic_archive_bytes.len() as u64
    ));
    group.bench_function("validate_synthetic_archive_bytes", |bencher| {
        bencher.iter(|| {
            OfflineRoutingNetworkGraph::from_bytes(black_box(&synthetic_archive_bytes))
                .expect("the synthetic benchmark archive validates")
        });
    });
    group.bench_function("load_and_index_synthetic_archive", |bencher| {
        bencher.iter(|| {
            let network_graph =
                OfflineRoutingNetworkGraph::from_bytes(black_box(&synthetic_archive_bytes))
                    .expect("the synthetic benchmark archive validates");
            LoadedRoutingGraph::new(network_graph)
        });
    });

    group.finish();
}

criterion_group!(
    name = offline_benchmarks;
    config = Criterion::default();
    targets = benchmark_search_group, benchmark_archive_compile_and_load_group
);
criterion_main!(offline_benchmarks);
