#!/usr/bin/env node
// The browser frame-budget harness (PLAN.md, M23).
//
// WHAT IT MEASURES. A frozen scenario — the base map over the recorded tile
// fixtures (vector by default, `--layer raster` for the tile path), a
// 1920 × 1080 canvas at device scale 1, a settle, a warm-up, then a four-second
// constant-velocity pan out and back so the camera never leaves the recorded
// ground — driven through the page's own frame path, reporting the p50/p95/p99
// of:
//
//   * the `renderFrame` call (the wasm frame: geometry, encoding, submit),
//   * the animation-frame interval (the cadence the visitor sees),
//   * the wasm's own per-stage spans, which ride the frame statistics the page
//     already reads,
//   * the render pass's GPU time from timestamp queries, reported one frame
//     after the pass and skipped when the adapter does not support them.
//
// WebGPU timestamp values are quantized by the implementation (Chromium's
// step is commonly 100 µs and is platform-dependent), so the GPU pass row is
// a distribution of quantized samples, not a cycle-exact figure. Read it next
// to `renderFrame`, never instead of it.
//
// WHY IT IS NOT A PLAYWRIGHT TEST. The standing test machine renders through
// SwiftShader, whose absolute frame times say nothing about a GPU
// (the measurement trap `docs/reference/BrowserRenderPerformance.md` records:
// MapLibre's benchmark under-reported by ~390× there). This runner therefore
// launches the host's own Chromium with hardware WebGPU and is a
// host-machine measurement, deliberately outside `npm test` so CI stays
// deterministic.
//
// THE RECIPE (`docs/reference/BrowserRenderPerformance.md`) states the scenario, the
// machine, and the numbers the performance chapter quotes; this runner is
// the committed way to reproduce them.
//
// Usage:
//   node perf/run-frame-budget.mjs                    # vector base, readable report
//   node perf/run-frame-budget.mjs --layer raster     # the tile path instead
//   node perf/run-frame-budget.mjs --json out.json    # also write the report
//   node perf/run-frame-budget.mjs --headed           # watch the pan

import { writeFile } from 'node:fs/promises';
import { chromium } from '@playwright/test';
import { HOST, PORT, startStaticServer, stopStaticServer } from '../serve.mjs';

/** The scenario's fixed canvas: 1920 × 1080 at device scale 1. */
const VIEWPORT_SIZE = { width: 1920, height: 1080 };
const DEVICE_SCALE_FACTOR = 1;

/** The constant pan velocity and duration the milestones quote against. */
const PAN_PIXELS_PER_SECOND = 600;
const PAN_DURATION_MILLISECONDS = 4000;
const WARMUP_DURATION_MILLISECONDS = 1500;

/** The 60 Hz frame budget and its tolerated tail (one dropped frame). */
const FRAME_BUDGET_MILLISECONDS = 16.7;
const P99_FRAME_BUDGET_MILLISECONDS = 33;

/** How long the page may take to settle its first complete vector frame. */
const SETTLE_TIMEOUT_MILLISECONDS = 60_000;
const SETTLE_POLL_INTERVAL_MILLISECONDS = 100;

/** WebGPU needs Chromium's own switch; the SwiftShader flags the test
 *  config forces are deliberately absent so the host's GPU is used. */
const BROWSER_LAUNCH_ARGUMENTS = [
  '--enable-unsafe-webgpu',
  '--enable-gpu',
  '--no-sandbox',
];

/** Reports one stage of the run on stderr, so a stalled run says where. */
function reportStage(stage_text) {
  process.stderr.write(`  … ${stage_text}\n`);
}

/** The base layers the scenario can measure: the vector base draws the
 *  overlay path, the raster base the tile path. */
const BASE_LAYERS = new Set(['vector', 'raster']);
const DEFAULT_BASE_LAYER = 'vector';

/** Reads the command line: `--json <path>`, `--layer <vector|raster>` and
 *  `--headed`. */
function readOptions(argv) {
  const options = { json_path: null, headed: false, base_layer: DEFAULT_BASE_LAYER };
  for (let index = 2; index < argv.length; index += 1) {
    if (argv[index] === '--json') {
      options.json_path = argv[index + 1] ?? null;
      index += 1;
    } else if (argv[index] === '--layer') {
      const requested_layer = argv[index + 1];
      if (!BASE_LAYERS.has(requested_layer)) {
        throw new Error(`--layer must be one of ${[...BASE_LAYERS].join(', ')}`);
      }
      options.base_layer = requested_layer;
      index += 1;
    } else if (argv[index] === '--headed') {
      options.headed = true;
    }
  }
  return options;
}

/** The nearest-rank percentile of an ascending numeric list. */
function percentile(sorted_values, fraction) {
  if (sorted_values.length === 0) return null;
  const rank = Math.ceil(fraction * sorted_values.length);
  const index = Math.min(sorted_values.length - 1, Math.max(0, rank - 1));
  return sorted_values[index];
}

/** Summarizes one metric list as count/min/p50/p95/p99/max. */
function summarize(values) {
  const sorted_values = [...values].sort((first, second) => first - second);
  if (sorted_values.length === 0) {
    return { count: 0, minimum: null, p50: null, p95: null, p99: null, maximum: null };
  }
  const round = (value) => (value === null ? null : Number(value.toFixed(2)));
  return {
    count: sorted_values.length,
    minimum: round(sorted_values[0]),
    p50: round(percentile(sorted_values, 0.5)),
    p95: round(percentile(sorted_values, 0.95)),
    p99: round(percentile(sorted_values, 0.99)),
    maximum: round(sorted_values[sorted_values.length - 1]),
  };
}

/** Waits until the page reports a complete vector-base frame. */
async function awaitSettledBaseFrame(page, base_layer) {
  const deadline = Date.now() + SETTLE_TIMEOUT_MILLISECONDS;
  while (Date.now() < deadline) {
    // A vector frame hides the raster tile layer, so its tile counters stay
    // at zero by design; the base is ready when its geometry is drawn.
    const settled = await page.evaluate(([expectedLayer]) => {
      if (window.mapTileMode?.() !== expectedLayer) return false;
      const statistics = window.mapLastFrameStats?.();
      if (!statistics) return false;
      // The vector base hides the raster tiles, so a vector frame reports no
      // tile counters at all; the raster base's frame is settled when every
      // requested tile drew.
      return expectedLayer === 'vector'
        ? statistics.overlayTriangles > 0
        : statistics.drawnTiles > 0 && statistics.missingTiles === 0;
    }, [base_layer]);
    if (settled) return;
    await page.waitForTimeout(SETTLE_POLL_INTERVAL_MILLISECONDS);
  }
  const final_state = await page.evaluate(() => ({
    mode: window.mapTileMode?.() ?? null,
    rendererMode: window.mapRendererMode?.() ?? null,
    vectorStatus: window.mapVectorBaseStatus?.() ?? null,
    statistics: window.mapLastFrameStats?.() ?? null,
  }));
  throw new Error(
    'the page never settled a complete base frame; is WebGPU available '
      + `to this Chromium build (renderer mode must be "gpu")? last state: ${
        JSON.stringify(final_state)}`,
  );
}

/** Runs one scenario pass and returns the page's samples plus frame intervals. */
function runScenario(page, duration_milliseconds) {
  return page.evaluate(
    ([panPixelsPerSecond, durationMilliseconds]) =>
      window.runFrameBudgetScenario({ panPixelsPerSecond, durationMilliseconds }),
    [PAN_PIXELS_PER_SECOND, duration_milliseconds],
  );
}

/** Renders the human-readable report. */
function formatReport(report) {
  const lines = [];
  const metric_row = (label, summary) => {
    const cell = (value) => (value === null ? '—' : String(value).padStart(8));
    lines.push(
      `  ${label.padEnd(26)}${String(summary.count).padStart(6)}`
        + `${cell(summary.p50)}${cell(summary.p95)}${cell(summary.p99)}`
        + `${cell(summary.maximum)}`,
    );
  };
  lines.push('');
  lines.push(`  scenario: ${report.scenario.description}`);
  lines.push(`  renderer: ${report.renderer.mode} (${report.renderer.detail})`);
  if (report.renderer.adapter) {
    const adapter = report.renderer.adapter;
    lines.push(`  adapter:  ${[adapter.vendor, adapter.architecture, adapter.device, adapter.description]
      .filter(Boolean).join(' · ') || 'unreported'}`);
  }
  lines.push(`  canvas:   ${report.scenario.canvasWidth} × ${report.scenario.canvasHeight}`
    + ` at device scale ${report.scenario.deviceScaleFactor}`);
  lines.push(`  frames:   last frame drew ${report.lastFrame.drawnTiles} tiles`
    + ` (${report.lastFrame.drawnFragments} fragments),`
    + ` ${report.lastFrame.overlayTriangles} overlay triangles`);
  lines.push(`  motion:   ${report.motionGradedFrameCount} of `
    + `${report.verdict.frameCount} frames drawn motion-graded (vector base);`
    + ' the pan ends with one settled frame');
  lines.push('');
  lines.push('  metric                     count      p50     p95     p99     max');
  metric_row('renderFrame (ms)', report.renderFrameMilliseconds);
  metric_row('animation frame (ms)', report.animationFrameIntervalMilliseconds);
  if (report.wasmStageMilliseconds) {
    for (const [stage, summary] of Object.entries(report.wasmStageMilliseconds)) {
      metric_row(`wasm ${stage} (ms)`, summary);
    }
  }
  if (report.gpuPassMilliseconds) {
    metric_row('gpu pass (ms)', report.gpuPassMilliseconds);
    lines.push(
      '  gpu pass: WebGPU timestamps are implementation-quantized (commonly '
        + '100 µs steps); the value describes the previous frame\'s pass.',
    );
  } else {
    lines.push(
      '  gpu pass: skipped — this adapter does not support timestamp queries.',
    );
  }
  lines.push('');
  lines.push(
    `  60 Hz budget ${FRAME_BUDGET_MILLISECONDS} ms — `
      + `p95 ${report.verdict.withinFrameBudget ? 'within' : 'OVER'} budget, `
      + `p99 ${report.verdict.withinP99Budget ? 'within' : 'OVER'} ${P99_FRAME_BUDGET_MILLISECONDS} ms`,
  );
  lines.push(`  frames over ${FRAME_BUDGET_MILLISECONDS} ms: `
    + `${report.verdict.framesOverBudget} of ${report.verdict.frameCount}`);
  lines.push('');
  return lines.join('\n');
}

/** Starts the static server, or reuses one a previous run already left on
 *  the harness port (the page's root is the same either way). */
async function startOrReuseStaticServer() {
  const server_url = `http://${HOST}:${PORT}`;
  try {
    const probe_response = await fetch(`${server_url}/package.json`);
    if (probe_response.ok) return { server_url, owns_server: false };
  } catch {
    // Nothing is listening: fall through and start the harness's own server.
  }
  await startStaticServer();
  return { server_url, owns_server: true };
}

async function main() {
  const options = readOptions(process.argv);
  const { server_url, owns_server } = await startOrReuseStaticServer();
  const browser = await chromium.launch({
    channel: 'chromium',
    headless: !options.headed,
    args: BROWSER_LAUNCH_ARGUMENTS,
  });
  try {
    const context = await browser.newContext({
      viewport: VIEWPORT_SIZE,
      deviceScaleFactor: DEVICE_SCALE_FACTOR,
    });
    const page = await context.newPage();
    reportStage('opening the playground');
    await page.goto(`${server_url}/?advanced`);
    await page.evaluate(() => window.mapTestReady);
    reportStage('the SDK initialized');
    // The layers panel is the single home for base selection. Its rows render
    // before the renderer answers and stay disabled until it does (a click on
    // a disabled row is a silent no-op), so the scenario waits for the chosen
    // row to become selectable.
    await page.evaluate(() => {
      document.getElementById('map-layers-control').click();
    });
    await page.waitForFunction(
      ([baseLayer]) => {
        const base_map_button = document.querySelector(
          `#map-layer-base-rows button[data-tile-mode="${baseLayer}"]`,
        );
        return base_map_button !== null && !base_map_button.disabled;
      },
      [options.base_layer],
      { timeout: SETTLE_TIMEOUT_MILLISECONDS },
    );
    // The vector base draws the overlay path over the recorded tile fixtures;
    // the raster base draws the tile path itself. Both are measured, because
    // the two paths own different stages of the frame.
    await page.evaluate(([baseLayer]) => {
      document.querySelector(
        `#map-layer-base-rows button[data-tile-mode="${baseLayer}"]`).click();
      // Close the panel so the measured frames carry only the map surface.
      document.getElementById('map-layers-control').click();
    }, [options.base_layer]);
    reportStage(`${options.base_layer} base chosen`);
    await awaitSettledBaseFrame(page, options.base_layer);
    reportStage('first complete base frame');

    // Warm-up: shader compilation, the first uploads and the initial vector
    // reload are one-time costs that say nothing about steady state.
    await runScenario(page, WARMUP_DURATION_MILLISECONDS);
    reportStage('warm-up pan done');
    await awaitSettledBaseFrame(page, options.base_layer);
    reportStage('settled after warm-up');

    const scenario_result = await runScenario(page, PAN_DURATION_MILLISECONDS);
    reportStage('measured pan done');
    const last_frame = await page.evaluate(() => window.mapLastFrameStats());
    // The report must be able to say what produced the numbers: the renderer
    // mode, the browser build and the adapter behind it (a software rasterizer
    // answers in milliseconds where a GPU answers in microseconds, and a
    // quoted figure without that context is not a measurement).
    const renderer = await page.evaluate(async () => {
      const adapter = await navigator.gpu?.requestAdapter?.();
      const adapter_info = adapter?.info ?? null;
      return {
        mode: window.mapRendererMode(),
        detail: window.mapCameraState()?.rendererMode ?? 'unknown',
        browser: navigator.userAgent,
        adapter: adapter_info
          ? {
            vendor: adapter_info.vendor ?? null,
            architecture: adapter_info.architecture ?? null,
            device: adapter_info.device ?? null,
            description: adapter_info.description ?? null,
          }
          : null,
      };
    });
    const canvas_extent = await page.evaluate(() => ({
      width: document.getElementById('map-canvas').width,
      height: document.getElementById('map-canvas').height,
    }));

    const samples = scenario_result.samples;
    const render_frame_milliseconds = summarize(
      samples.map((sample) => sample.renderMilliseconds),
    );
    const animation_frame_interval_milliseconds = summarize(
      scenario_result.animationFrameIntervalsMilliseconds,
    );
    const wasm_stage_milliseconds = samples.some((sample) => sample.stageMilliseconds)
      ? Object.fromEntries(
        Object.keys(samples[samples.length - 1].stageMilliseconds).map((stage) => [
          stage,
          summarize(samples.map((sample) => sample.stageMilliseconds[stage] ?? 0)),
        ]),
      )
      : null;
    const gpu_pass_milliseconds = samples.some(
      (sample) => typeof sample.gpuPassMilliseconds === 'number',
    )
      ? summarize(
        samples
          .map((sample) => sample.gpuPassMilliseconds)
          .filter((value) => typeof value === 'number'),
      )
      : null;

    const report = {
      generatedAt: new Date().toISOString(),
      scenario: {
        description:
          `${PAN_PIXELS_PER_SECOND} device px/s pan for ${PAN_DURATION_MILLISECONDS} ms`
          + ` over the ${options.base_layer} base, after a settle and a warm-up pass`,
        baseLayer: options.base_layer,
        panPixelsPerSecond: PAN_PIXELS_PER_SECOND,
        panDurationMilliseconds: PAN_DURATION_MILLISECONDS,
        canvasWidth: canvas_extent.width,
        canvasHeight: canvas_extent.height,
        deviceScaleFactor: DEVICE_SCALE_FACTOR,
      },
      renderer,
      lastFrame: last_frame,
      renderFrameMilliseconds: render_frame_milliseconds,
      animationFrameIntervalMilliseconds: animation_frame_interval_milliseconds,
      wasmStageMilliseconds: wasm_stage_milliseconds,
      gpuPassMilliseconds: gpu_pass_milliseconds,
      motionGradedFrameCount: samples.filter((sample) => sample.motionGraded).length,
      verdict: {
        frameCount: samples.length,
        framesOverBudget: samples.filter(
          (sample) => sample.renderMilliseconds > FRAME_BUDGET_MILLISECONDS,
        ).length,
        withinFrameBudget:
          render_frame_milliseconds.p95 !== null
          && render_frame_milliseconds.p95 <= FRAME_BUDGET_MILLISECONDS,
        withinP99Budget:
          render_frame_milliseconds.p99 !== null
          && render_frame_milliseconds.p99 <= P99_FRAME_BUDGET_MILLISECONDS,
      },
    };
    process.stdout.write(formatReport(report));
    if (options.json_path) {
      await writeFile(options.json_path, `${JSON.stringify(report, null, 2)}\n`, 'utf8');
      process.stdout.write(`  report written to ${options.json_path}\n\n`);
    }
  } finally {
    await browser.close();
    if (owns_server) await stopStaticServer();
  }
}

main().catch((error) => {
  process.stderr.write(`frame-budget harness failed: ${error?.stack ?? error}\n`);
  process.exitCode = 1;
  // The static server keeps the loop alive; the measurement is over either way.
  process.exit(1);
});
