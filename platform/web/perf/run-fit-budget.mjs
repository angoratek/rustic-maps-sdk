#!/usr/bin/env node
// The z14→z7 vector fit-budget harness.
//
// WHAT IT MEASURES. A cold fit: one frame at z14 on the bare perf surface,
// then the camera changes to z7 and the page renders at its default cadence
// (one frame per animation frame). The runner reports:
//
//   * the time from the fit to the first frame that drew a base tile,
//   * the time from the fit to the first frame that drew every visible tile,
//   * per-frame `renderFrame` wall time and the wasm stage spans
//     (tile pipeline, overlay geometry, encode, submit),
//   * the requested and drawn tile counts and the overlay triangle count.
//
// It fails when the first full frame exceeds 1.5 s or the frame draws more
// than 400k overlay triangles at z7. Both are device measurements: this
// runner, like `run-frame-budget.mjs`, launches the host's own Chromium with
// hardware WebGPU and deliberately stays outside `npm test`, whose SwiftShader
// renderer says nothing about a GPU. The committed fixtures it serves are
// provider-neutral performance input, installed directly with a same-origin
// template rather than through the provider registry.
//
// Usage:
//   node perf/run-fit-budget.mjs                 # readable report
//   node perf/run-fit-budget.mjs --json out.json # also write the report
//   node perf/run-fit-budget.mjs --headed        # watch the fit

import { writeFile } from 'node:fs/promises';
import { chromium } from '@playwright/test';
import { HOST, PORT, startStaticServer, stopStaticServer } from '../serve.mjs';
import {
  FIT_BUDGET_DEVICE_SCALE_FACTOR,
  FIT_BUDGET_FIRST_FULL_FRAME_MILLISECONDS,
  FIT_BUDGET_OVERLAY_TRIANGLE_COUNT,
  FIT_BUDGET_PAGE_PATH,
  FIT_BUDGET_SETTLE_RENDER_P95_MILLISECONDS,
  FIT_BUDGET_Z8_SETTLE_RENDER_P95_MILLISECONDS,
  FIT_BUDGET_VIEWPORT,
  installFitBudgetFixtureRoute,
  runFitBudgetScenario,
  summarizeMetrics,
} from './fit_budget_harness.mjs';

/** WebGPU needs Chromium's own switch; the test configuration's SwiftShader
 *  flags are deliberately absent so the host's GPU is used. */
const BROWSER_LAUNCH_ARGUMENTS = [
  '--enable-unsafe-webgpu',
  '--enable-gpu',
  '--no-sandbox',
];

/** Reports one stage of the run on stderr, so a stalled run says where. */
function reportStage(stage_text) {
  process.stderr.write(`  … ${stage_text}\n`);
}

/** Reads the command line: `--json <path>` and `--headed`. */
function readOptions(argv) {
  const options = { json_path: null, headed: false };
  for (let index = 2; index < argv.length; index += 1) {
    if (argv[index] === '--json') {
      options.json_path = argv[index + 1] ?? null;
      index += 1;
    } else if (argv[index] === '--headed') {
      options.headed = true;
    }
  }
  return options;
}

/** Starts the static server, or reuses one a previous run left on the port. */
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

/** Renders the human-readable report. */
function formatReport(report) {
  const lines = [];
  const metric_row = (label, summary) => {
    const cell = (value) => (value === null ? '—' : String(value).padStart(8));
    lines.push(
      `  ${label.padEnd(30)}${String(summary.count).padStart(6)}`
        + `${cell(summary.p50)}${cell(summary.p95)}${cell(summary.p99)}`
        + `${cell(summary.maximum)}`,
    );
  };
  lines.push('');
  lines.push('  scenario: cold z14 → z7 fit, one frame per animation frame');
  lines.push(`  renderer: ${report.renderer.mode} (${report.renderer.detail})`);
  if (report.renderer.adapter) {
    const adapter = report.renderer.adapter;
    lines.push(`  adapter:  ${[adapter.vendor, adapter.architecture, adapter.device, adapter.description]
      .filter(Boolean).join(' · ') || 'unreported'}`);
  }
  lines.push(`  canvas:   ${report.scenario.canvasWidth} × ${report.scenario.canvasHeight}`
    + ` at device scale ${report.scenario.deviceScaleFactor}`);
  lines.push(`  fit:      ${report.scenario.fitZoomLevel} → served zoom ${report.fit.servedZoomLevel}`
    + `, ${report.fit.expectedTileCount} visible tiles requested`);
  lines.push('');
  lines.push(`  first drawn base tile: ${report.fit.firstDrawnBaseTileMilliseconds ?? '—'} ms`);
  lines.push(
    `  first full frame:      ${report.fit.firstFullFrameMilliseconds ?? '—'} ms`
    + ` (budget ${FIT_BUDGET_FIRST_FULL_FRAME_MILLISECONDS} ms)`,
  );
  const last_sample = report.fit.samples[report.fit.samples.length - 1];
  lines.push(
    `  last frame:            ${last_sample.drawnTileCount} of`
    + ` ${last_sample.requestedTileCount} tiles drawn,`
    + ` ${report.fit.maximumOverlayTriangles} overlay triangles`
    + ` (budget ${FIT_BUDGET_OVERLAY_TRIANGLE_COUNT})`,
  );
  lines.push('');
  lines.push('  metric                         count      p50     p95     p99     max');
  metric_row('renderFrame (ms)', report.renderFrameMilliseconds);
  metric_row('settled renderFrame (ms)', report.settleRenderMilliseconds);
  if (report.z8SettleMilliseconds.count > 0) {
    metric_row('z8 settle renderFrame (ms)', report.z8SettleMilliseconds);
  }
  for (const [stage, summary] of Object.entries(report.stageMilliseconds)) {
    metric_row(`${stage} (ms)`, summary);
  }
  lines.push('');
  lines.push(
    `  verdict: ${report.verdict.withinFirstFullFrameBudget ? 'within' : 'OVER'}`
    + ` the ${FIT_BUDGET_FIRST_FULL_FRAME_MILLISECONDS} ms first-full-frame budget, `
    + `${report.verdict.withinTriangleBudget ? 'within' : 'OVER'}`
    + ` the ${FIT_BUDGET_OVERLAY_TRIANGLE_COUNT.toLocaleString('en-US')}-triangle budget, `
    + `${report.verdict.withinSettleRenderBudget ? 'within' : 'OVER'}`
    + ` the ${FIT_BUDGET_SETTLE_RENDER_P95_MILLISECONDS} ms settled p95 budget, `
    + `${report.verdict.withinZ8SettleRenderBudget ? 'within' : 'OVER'}`
    + ` the ${FIT_BUDGET_Z8_SETTLE_RENDER_P95_MILLISECONDS} ms z8 p95 budget, `
    + `${report.verdict.z8ExtractsNoLabels ? 'zero' : 'NONZERO'}`
    + ' z8 label extractions',
  );
  lines.push('');
  return lines.join('\n');
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
      viewport: FIT_BUDGET_VIEWPORT,
      deviceScaleFactor: FIT_BUDGET_DEVICE_SCALE_FACTOR,
    });
    const page = await context.newPage();
    await installFitBudgetFixtureRoute(page);
    reportStage('opening the bare fit-budget surface');
    await page.goto(`${server_url}${FIT_BUDGET_PAGE_PATH}`);
    const surface = await page.evaluate(() => window.perfReady);
    reportStage('the GPU surface initialized');
    const fit = await runFitBudgetScenario(page, { measureZ8Settle: true });
    reportStage('the fit completed');
    const renderer = await page.evaluate(async () => {
      const adapter = await navigator.gpu?.requestAdapter?.();
      return {
        mode: 'gpu',
        detail: 'hardware WebGPU',
        browser: navigator.userAgent,
        adapter: adapter
          ? {
            vendor: adapter.info?.vendor ?? null,
            architecture: adapter.info?.architecture ?? null,
            device: adapter.info?.device ?? null,
            description: adapter.info?.description ?? null,
          }
          : null,
      };
    });
    const render_frame_milliseconds = summarizeMetrics(
      fit.samples.map((sample) => sample.renderMilliseconds),
    );
    const stage_names = fit.samples.length > 0
      ? Object.keys(fit.samples[fit.samples.length - 1].stageMilliseconds ?? {})
      : [];
    const stage_milliseconds = Object.fromEntries(
      stage_names.map((stage) => [
        stage,
        summarizeMetrics(
          fit.samples.map((sample) => sample.stageMilliseconds?.[stage] ?? 0),
        ),
      ]),
    );
    const maximum_overlay_triangles = Math.max(
      ...fit.samples.map((sample) => sample.overlayTriangles),
    );
    const settle_render_milliseconds = summarizeMetrics(
      (fit.settleSamples ?? []).map((sample) => sample.renderMilliseconds),
    );
    const z8_settle_samples = fit.z8SettleSamples ?? [];
    const z8_settle_milliseconds = summarizeMetrics(
      z8_settle_samples.map((sample) => sample.renderMilliseconds),
    );
    const z8_last_sample = z8_settle_samples[z8_settle_samples.length - 1] ?? null;
    const z8_label_candidate_extraction_count =
      z8_last_sample?.labelCandidateExtractionCount ?? null;
    const z8_label_candidate_cache_hit_count =
      z8_last_sample?.labelCandidateCacheHitCount ?? null;
    const report = {
      generatedAt: new Date().toISOString(),
      scenario: {
        description:
          'cold z14 → z7 fit over the committed performance fixtures, '
          + 'one frame per animation frame',
        canvasWidth: surface.width,
        canvasHeight: surface.height,
        deviceScaleFactor: FIT_BUDGET_DEVICE_SCALE_FACTOR,
        startZoomLevel: 14,
        fitZoomLevel: 7,
      },
      renderer,
      fit: {
        ...fit,
        maximumOverlayTriangles: maximum_overlay_triangles,
      },
      renderFrameMilliseconds: render_frame_milliseconds,
      settleRenderMilliseconds: settle_render_milliseconds,
      z8SettleMilliseconds: z8_settle_milliseconds,
      z8Settle: {
        sampleCount: z8_settle_samples.length,
        overlayTriangles: z8_last_sample?.overlayTriangles ?? null,
        labelCandidateExtractionCount: z8_label_candidate_extraction_count,
        labelCandidateCacheHitCount: z8_label_candidate_cache_hit_count,
      },
      stageMilliseconds: stage_milliseconds,
      verdict: {
        withinFirstFullFrameBudget:
          fit.firstFullFrameMilliseconds !== null
          && fit.firstFullFrameMilliseconds <= FIT_BUDGET_FIRST_FULL_FRAME_MILLISECONDS,
        withinTriangleBudget:
          maximum_overlay_triangles <= FIT_BUDGET_OVERLAY_TRIANGLE_COUNT,
        withinSettleRenderBudget:
          settle_render_milliseconds.p95 !== null
          && settle_render_milliseconds.p95 <= FIT_BUDGET_SETTLE_RENDER_P95_MILLISECONDS,
        withinZ8SettleRenderBudget:
          z8_settle_milliseconds.p95 !== null
          && z8_settle_milliseconds.p95
            <= FIT_BUDGET_Z8_SETTLE_RENDER_P95_MILLISECONDS,
        z8ExtractsNoLabels:
          z8_settle_samples.length > 0
          && z8_label_candidate_extraction_count === 0,
      },
    };
    process.stdout.write(formatReport(report));
    if (options.json_path) {
      await writeFile(options.json_path, `${JSON.stringify(report, null, 2)}\n`, 'utf8');
      process.stdout.write(`  report written to ${options.json_path}\n\n`);
    }
    if (
      !report.verdict.withinFirstFullFrameBudget
      || !report.verdict.withinTriangleBudget
      || !report.verdict.withinSettleRenderBudget
      || !report.verdict.withinZ8SettleRenderBudget
      || !report.verdict.z8ExtractsNoLabels
    ) {
      process.exitCode = 1;
    }
  } finally {
    await browser.close();
    if (owns_server) await stopStaticServer();
  }
}

main().catch((error) => {
  process.stderr.write(`fit-budget harness failed: ${error?.stack ?? error}\n`);
  process.exitCode = 1;
  // The static server keeps the loop alive; the measurement is over either way.
  process.exit(1);
});
