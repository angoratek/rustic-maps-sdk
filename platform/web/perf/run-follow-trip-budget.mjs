#!/usr/bin/env node
// The follow-trip tile budget runner.
//
// Plans the recorded demo route, starts the simulated trip and samples the
// page's own frame statistics once per interval. The readable report states,
// per second: requested and drawn tiles, missing tiles, the frames that
// reported a miss, and the provider requests the diagnostics ring recorded.
// It fails when a settled fixture-backed frame reports a missing tile, or when
// a live run spends more than the documented share of frames missing tiles.
//
// This harness runs outside `npm test` on purpose: it starts the demo's own
// page, drives the same user path a visitor takes, and in `live` mode reaches
// the network. The fixture mode serves committed tiles only.
//
// Usage:
//   node perf/run-follow-trip-budget.mjs                  # readable report
//   node perf/run-follow-trip-budget.mjs --json out.json  # also write the report
//   node perf/run-follow-trip-budget.mjs --live           # live raster fallback
//   PROVIDER_API_KEY=… node perf/run-follow-trip-budget.mjs --live
//   node perf/run-follow-trip-budget.mjs --seconds 20 --headed

import { writeFile } from 'node:fs/promises';
import { chromium } from '@playwright/test';
import { BROWSER_LAUNCH_OPTIONS } from '../browser_launch_options.mjs';
import { HOST, PORT, startStaticServer, stopStaticServer } from '../serve.mjs';
import {
  FOLLOW_TRIP_DEFAULT_SECONDS,
  FOLLOW_TRIP_FIXTURE_VIEWPORT,
  FOLLOW_TRIP_LIVE_VIEWPORT,
  FOLLOW_TRIP_SAMPLE_INTERVAL_MILLISECONDS,
  applySessionCredential,
  evaluateFollowTrip,
  planAndStartFollowTrip,
  sampleFollowTrip,
  summarizeFollowTrip,
  waitForFollowTripReady,
} from './follow_trip_budget_harness.mjs';

/** Reports one stage on stderr, so a stalled run says where. */
function reportStage(stage_text) {
  process.stderr.write(`  … ${stage_text}\n`);
}

/** Reads the command line. */
function readOptions(argv) {
  const options = {
    json_path: null,
    headed: false,
    live: false,
    seconds: FOLLOW_TRIP_DEFAULT_SECONDS,
    speedMultiplier: null,
  };
  for (let index = 2; index < argv.length; index += 1) {
    if (argv[index] === '--json') {
      options.json_path = argv[index + 1] ?? null;
      index += 1;
    } else if (argv[index] === '--headed') {
      options.headed = true;
    } else if (argv[index] === '--live') {
      options.live = true;
    } else if (argv[index] === '--speed') {
      const parsed_speed = Number(argv[index + 1] ?? '');
      if (Number.isFinite(parsed_speed) && parsed_speed > 0) {
        options.speedMultiplier = parsed_speed;
      }
      index += 1;
    } else if (argv[index] === '--seconds') {
      const parsed_seconds = Number(argv[index + 1] ?? '');
      if (Number.isFinite(parsed_seconds) && parsed_seconds > 0) {
        options.seconds = parsed_seconds;
      }
      index += 1;
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
  lines.push('');
  lines.push(`  scenario: follow trip, ${report.mode} tiles,`
    + ` ${report.secondsRequested} seconds requested`);
  lines.push(`  viewport: ${report.viewport.width} × ${report.viewport.height}`);
  lines.push(`  trip:     ${report.summary.sampleCount} samples over`
    + ` ${(report.summary.elapsedMilliseconds / 1_000).toFixed(1)} s at`
    + ` ${report.summary.speedMultiplier ?? '—'}×;`
    + ` remaining ${report.summary.progressStartMeters ?? '—'} m →`
    + ` ${report.summary.progressEndMeters ?? '—'} m;`
    + ` zoom ${report.summary.cameraStartZoom ?? '—'} →`
    + ` ${report.summary.cameraEndZoom ?? '—'}`);
  lines.push(`  provider requests recorded: ${report.summary.providerRequestCount}`);
  lines.push('');
  lines.push('  second  frames   requested   drawn   missing   miss frames'
    + '   provider requests');
  for (const bucket of report.summary.seconds) {
    const provider_requests = report.summary.providerRequestSeconds.find(
      (entry) => entry.second === bucket.second,
    )?.count ?? 0;
    lines.push(
      `  ${String(bucket.second).padStart(6)}`
      + `  ${String(bucket.framesDrawn).padStart(6)}`
      + `  ${String(bucket.requestedTileCount).padStart(11)}`
      + `  ${String(bucket.drawnTileCount).padStart(7)}`
      + `  ${String(bucket.missingTileCount).padStart(9)}`
      + `  ${String(bucket.missingFrameCount).padStart(13)}`
      + `  ${String(provider_requests).padStart(17)}`,
    );
    if (bucket.missingTileAddresses.length > 0) {
      lines.push(`          missing: ${bucket.missingTileAddresses.join(', ')}`);
    }
  }
  lines.push('');
  lines.push(`  verdict: ${report.verdict.outcome.toUpperCase()}`);
  lines.push(`  reason:  ${report.verdict.reason}`);
  lines.push('');
  return lines.join('\n');
}

const options = readOptions(process.argv);
const credential = process.env.PROVIDER_API_KEY?.trim() ?? '';
if (options.live && credential === '') {
  reportStage('no PROVIDER_API_KEY in the environment: measuring the keyless'
    + ' live raster source');
}

const { server_url, owns_server } = await startOrReuseStaticServer();
const viewport = options.live
  ? FOLLOW_TRIP_LIVE_VIEWPORT
  : FOLLOW_TRIP_FIXTURE_VIEWPORT;
const browser = await chromium.launch({
  channel: 'chromium',
  headless: !options.headed,
  ...BROWSER_LAUNCH_OPTIONS,
});
let exit_code = 0;
try {
  const page = await browser.newPage({ viewport });
  page.on('pageerror', (page_failure) => {
    process.stderr.write(`PAGE ERROR ${page_failure.message}\n`);
  });
  reportStage(`opening ${options.live ? 'the live-tiles' : 'the fixture'} demo`);
  await page.goto(`${server_url}/?advanced${options.live ? '&liveTiles' : ''}`);
  reportStage('awaiting the GPU renderer');
  await waitForFollowTripReady(page);
  if (options.live && credential !== '') {
    reportStage('applying the session credential through the settings surface');
    await applySessionCredential(page, credential);
  }
  reportStage('planning and starting the recorded demo trip');
  await planAndStartFollowTrip(page, options.speedMultiplier);
  reportStage(`sampling ${options.seconds} seconds`);
  const samples = await sampleFollowTrip(
    page,
    options.seconds * 1_000,
    FOLLOW_TRIP_SAMPLE_INTERVAL_MILLISECONDS,
  );
  const summary = summarizeFollowTrip(samples);
  const verdict = evaluateFollowTrip(summary, options.live ? 'live' : 'fixtures');
  const report = {
    mode: options.live ? 'live' : 'fixtures',
    viewport,
    secondsRequested: options.seconds,
    renderer: await page.evaluate(() => window.mapCameraState?.().rendererMode),
    summary,
    verdict,
  };
  console.log(formatReport(report));
  if (options.json_path !== null) {
    await writeFile(options.json_path, `${JSON.stringify(report, null, 1)}\n`);
    console.log(`  report written to ${options.json_path}`);
  }
  if (verdict.outcome !== 'passed') exit_code = 1;
} finally {
  await browser.close();
  if (owns_server) await stopStaticServer();
}
process.exitCode = exit_code;
