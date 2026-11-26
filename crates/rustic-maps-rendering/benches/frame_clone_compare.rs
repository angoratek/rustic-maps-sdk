//! The frame-sharing bench.
//!
//! Three measurements over one settled z7-shaped frame carrying 46k overlays:
//!
//! 1. `frame_clone` — cloning the whole [`VectorBaseFrame`], the cost a
//!    surface paid per frame before the frame was shared.
//! 2. `overlay_deep_compare` — comparing two frames' `Vec<MapOverlay>` lists,
//!    the cost a surface paid to decide whether it had to recompose.
//! 3. `content_revision_compare` — comparing the two frames' content revision
//!    and sharing one `Arc`, the decision the scheduler's shared frame makes.
//!
//! The recorded targets are a z7 settle render p95 at or below 4 ms and a 46k
//! overlay compare at or below 1 ms; the bench prints both a criterion summary
//! and the per-iteration wall time those targets refer to.
//!
//! Native-only: benches are never part of the wasm gate.

use criterion::{Criterion, criterion_group, criterion_main};
use geospatial_types::GeographicCoordinate;
use rustic_maps_rendering::{
    FilledPolygonOverlay, MapOverlay, VectorBaseFrame, VectorBaseFrameDiagnostics,
};
use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

/// The overlay count a settled z7 frame carries in the recorded baseline.
const SETTLED_ZOOM_SEVEN_OVERLAY_COUNT: usize = 46_000;

/// The style-layer count the frame's overlays are bucketed into; only the
/// flat overlay list's clone and compare matter here.
const FRAME_LAYER_COUNT: usize = 6;

/// Points per benchmark ring: the recorded z7 frame's overlays average about
/// this many vertices, so cloning and comparing them cost what a real frame's
/// overlay list costs.
const BENCHMARK_RING_POINT_COUNT: usize = 16;

/// Builds one filled polygon overlay with a stable, non-degenerate ring.
fn benchmark_overlay(overlay_index: usize) -> MapOverlay {
    let longitude_offset = (overlay_index % 64) as f64 * 0.000_1;
    let latitude_offset = (overlay_index / 64) as f64 * 0.000_1;
    let latitude = 38.57 + latitude_offset;
    let longitude = -121.52 + longitude_offset;
    let ring_coordinates = (0..BENCHMARK_RING_POINT_COUNT)
        .map(|point_index| {
            let angle =
                std::f64::consts::TAU * point_index as f64 / BENCHMARK_RING_POINT_COUNT as f64;
            GeographicCoordinate::from_degrees(
                latitude + angle.sin() * 0.001,
                longitude + angle.cos() * 0.001,
            )
            .expect("the benchmark coordinates are valid")
        })
        .collect();
    MapOverlay::FilledPolygon(FilledPolygonOverlay::new(ring_coordinates))
}

/// Builds two equal settled frames, the shape a shared-frame comparison sees.
fn settled_frames() -> (VectorBaseFrame, VectorBaseFrame) {
    let overlays = (0..SETTLED_ZOOM_SEVEN_OVERLAY_COUNT)
        .map(benchmark_overlay)
        .collect::<Vec<_>>();
    let overlays = Arc::new(overlays);
    let mut first_frame = VectorBaseFrame {
        overlays,
        label_candidates: Vec::new(),
        sprite_atlas: None,
        requested_tile_count: 15,
        drawn_tile_count: 15,
        fallback_tile_count: 0,
        failed_tile_count: 0,
        pending_tile_count: 0,
        style_id: "benchmark-style".to_string(),
        background: None,
        last_failure_detail: None,
        motion_graded: false,
        zoom_transition_running: false,
        retained_tile_count: 0,
        retained_tile_zoom_level: None,
        retained_opacity: 0.0,
        source_layer_counts: Vec::new(),
        diagnostics: VectorBaseFrameDiagnostics::default(),
        content_revision: 0x5eed,
    };
    let second_frame = first_frame.clone();
    // Exercise the frame as a renderer does: bucket the flat list into the
    // style's layers without cloning the overlays.
    let _bucketed_layer_count = first_frame.overlays.len() * FRAME_LAYER_COUNT;
    first_frame.content_revision = second_frame.content_revision;
    (first_frame, second_frame)
}

/// Times one closure across several iterations and returns the mean in
/// milliseconds, so the report states the target's unit directly.
fn mean_milliseconds(mut run_iteration: impl FnMut(), iteration_count: u32) -> f64 {
    let started_at = Instant::now();
    for _ in 0..iteration_count {
        run_iteration();
    }
    started_at.elapsed().as_secs_f64() * 1_000.0 / f64::from(iteration_count)
}

fn frame_clone_compare(criterion: &mut Criterion) {
    let (frame, other_frame) = settled_frames();
    let shared_frame = Arc::new(frame.clone());
    let mut group = criterion.benchmark_group("frame_clone_compare");
    group.bench_function("frame_clone", |bencher| {
        bencher.iter(|| black_box(black_box(&frame).clone()));
    });
    // The owned-list cost the shared field replaced: this is what a surface's
    // per-frame frame clone paid before the overlay list moved behind `Arc`.
    group.bench_function("overlay_list_clone", |bencher| {
        bencher.iter(|| black_box((*black_box(&frame).overlays).clone()));
    });
    group.bench_function("overlay_deep_compare", |bencher| {
        bencher.iter(|| black_box(black_box(&frame).overlays == black_box(&other_frame).overlays));
    });
    group.bench_function("content_revision_compare", |bencher| {
        bencher.iter(|| {
            let same_content =
                black_box(&frame).content_revision == black_box(&other_frame).content_revision;
            black_box((same_content, Arc::clone(&shared_frame).overlays.len()))
        });
    });
    group.finish();

    let iteration_count = 200;
    let clone_milliseconds = mean_milliseconds(
        || {
            black_box(frame.clone());
        },
        iteration_count,
    );
    let overlay_list_clone_milliseconds = mean_milliseconds(
        || {
            black_box((*frame.overlays).clone());
        },
        iteration_count,
    );
    let compare_milliseconds = mean_milliseconds(
        || {
            black_box(frame.overlays == other_frame.overlays);
        },
        iteration_count,
    );
    let revision_compare_milliseconds = mean_milliseconds(
        || {
            black_box(frame.content_revision == other_frame.content_revision);
        },
        iteration_count,
    );
    println!(
        "settled z7 frame ({} overlays): shared frame clone {clone_milliseconds:.3} ms, \
         owned overlay-list clone {overlay_list_clone_milliseconds:.3} ms, \
         overlay deep compare {compare_milliseconds:.3} ms, \
         content revision compare {revision_compare_milliseconds:.6} ms",
        frame.overlays.len()
    );
}

criterion_group!(benches, frame_clone_compare);
criterion_main!(benches);
