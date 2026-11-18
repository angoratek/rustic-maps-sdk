// The follow-trip tile budget: drive the demo through a simulated trip on the
// page's own user path and sample what every rendered frame actually drew.
//
// A simulated trip is the hardest case for tile delivery: the camera follows a
// moving fix stream at the recorded follow zoom, so every second asks the
// scheduler for addresses the previous second did not, while guidance and the
// location puck ride the same pump. This harness plans the recorded demo route,
// starts the follow demo, and samples `mapLastFrameStats` and the guidance
// snapshot once per interval. It reports per-second requested, drawn and
// missing counts, the frame cadence, the wasm stage spans and the provider
// request cadence the diagnostics ring recorded.
//
// Two modes:
//
//   * `fixtures` (default) serves the committed same-origin tile fixtures and
//     never touches the network. Once the grace period after the trip starts
//     has passed, a settled viewport must draw every tile it requested; a
//     missing tile there is a scheduler or fixture-coverage defect.
//   * `live` opens the demo with `?liveTiles` so uncovered addresses go to the
//     registry's live raster source. A session credential may be applied for
//     one run, so the keyed provider path measures the same trip. Live numbers
//     are measurements, not a gate: the verdict reports the missing share and
//     fails only above the documented budget.

export const FOLLOW_TRIP_SAMPLE_INTERVAL_MILLISECONDS = 250;
export const FOLLOW_TRIP_DEFAULT_SECONDS = 15;
export const FOLLOW_TRIP_FIXTURE_GRACE_MILLISECONDS = 2_000;
/** How long the arrival frame waits for the pump to draw the settled view. */
export const FOLLOW_TRIP_ARRIVAL_SETTLE_MILLISECONDS = 400;
export const FOLLOW_TRIP_FIELD_OF_VIEW_DEGREES = 60;
export const FOLLOW_TRIP_FIXTURE_VIEWPORT = { width: 640, height: 480 };
export const FOLLOW_TRIP_LIVE_VIEWPORT = { width: 1280, height: 900 };
export const FOLLOW_TRIP_DEMO_ORIGIN = 'Capitol Park';
export const FOLLOW_TRIP_DEMO_DESTINATION = 'Tower Bridge';
export const FOLLOW_TRIP_ROUTE_RESULT_MARKER = 'recorded demo route';
export const FOLLOW_TRIP_READINESS_TIMEOUT_MILLISECONDS = 60_000;
export const FOLLOW_TRIP_PLAN_TIMEOUT_MILLISECONDS = 20_000;
/** Share of sampled frames that may report a missing tile in `live` mode. */
export const FOLLOW_TRIP_LIVE_MISSING_FRAME_SHARE_BUDGET = 0.2;
/** Diagnostics events read per flush; a bounded read keeps the report small. */
export const FOLLOW_TRIP_DIAGNOSTIC_EVENT_LIMIT = 200;

/** Waits until the wasm surface answered readiness and built the GPU view. */
export async function waitForFollowTripReady(page) {
  await page.waitForFunction(
    () => Boolean(window.mapTest),
    null,
    { timeout: FOLLOW_TRIP_READINESS_TIMEOUT_MILLISECONDS },
  );
  await page.waitForFunction(
    () => window.mapCameraState?.().rendererMode === 'gpu',
    null,
    { timeout: FOLLOW_TRIP_READINESS_TIMEOUT_MILLISECONDS },
  );
}

/**
 * Fills the two plan fields, plans the recorded demo route, waits for the
 * route card and starts the follow demo. `speedMultiplier` selects one of the
 * demo's playback chips before the trip starts; omit it for the remembered
 * choice. Returns once the trip is engaged.
 */
export async function planAndStartFollowTrip(page, speedMultiplier = null) {
  await page.locator('#trip-origin-input').fill(FOLLOW_TRIP_DEMO_ORIGIN);
  await page.locator('#trip-destination-input')
    .fill(FOLLOW_TRIP_DEMO_DESTINATION);
  await page.locator('#trip-destination-search-control').click();
  const first_result = page.locator('#trip-search-results button').first();
  await first_result.waitFor({
    state: 'visible',
    timeout: FOLLOW_TRIP_PLAN_TIMEOUT_MILLISECONDS,
  });
  await first_result.click();
  await page.locator('#trip-plan-route-control').click();
  await page.waitForFunction(
    (marker) => (document.getElementById('trip-route-result').textContent ?? '')
      .toLowerCase()
      .includes(marker),
    FOLLOW_TRIP_ROUTE_RESULT_MARKER,
    { timeout: FOLLOW_TRIP_PLAN_TIMEOUT_MILLISECONDS },
  );
  // Let the plan's framing settle before the trip starts, so the first
  // samples measure the follow camera rather than the route fit.
  await page.waitForTimeout(FOLLOW_TRIP_FIXTURE_GRACE_MILLISECONDS);
  await page.locator('#follow-demo-start-control').click();
  // The playback chips live in the running trip's panel; selecting one
  // re-arms the fix timer at the new cadence.
  if (speedMultiplier !== null) {
    await page
      .locator('#follow-demo-speed-controls button', {
        hasText: `${speedMultiplier}×`,
      })
      .click();
  }
}

/**
 * Applies a session credential through the demo's own settings surface, the
 * same path a visitor takes. The value is never written to a file or a log.
 */
export async function applySessionCredential(page, credential) {
  const credential_row = page
    .locator('#provider-credential-rows .provider-credential-row')
    .first();
  await credential_row.locator('[data-provider-credential-input]')
    .fill(credential);
  await credential_row.locator('[data-provider-credential-apply]').click();
  await page.waitForFunction(
    () => (document.getElementById('provider-credential-status')?.textContent
      ?? '').toLowerCase()
      .includes('applied'),
    null,
    { timeout: FOLLOW_TRIP_PLAN_TIMEOUT_MILLISECONDS },
  );
  // The field is cleared after applying; the session keeps the key only.
  await credential_row.locator('[data-provider-credential-input]').fill('');
}

/** Reads one sample: frame statistics, camera, guidance and follow state. */
async function readSample(page) {
  return page.evaluate((event_limit) => ({
    atMilliseconds: performance.now(),
    framesDrawn: window.mapFramesDrawn?.(),
    statistics: window.mapLastFrameStats?.(),
    camera: window.mapCameraState?.(),
    progress: window.mapNavigationSnapshot?.()?.instruction?.progress ?? null,
    follow: window.mapFollowDemoState?.(),
    diagnostic_events: window.mapTest?.flushRecentDiagnosticEvents
      ? window.mapTest.flushRecentDiagnosticEvents(1, event_limit)
      : [],
  }), FOLLOW_TRIP_DIAGNOSTIC_EVENT_LIMIT);
}

/** Samples the trip for `windowMilliseconds` at the sample interval. */
export async function sampleFollowTrip(
  page,
  windowMilliseconds,
  sampleIntervalMilliseconds = FOLLOW_TRIP_SAMPLE_INTERVAL_MILLISECONDS,
) {
  const started_at = Date.now();
  const samples = [];
  while (Date.now() - started_at < windowMilliseconds) {
    const sample = await readSample(page);
    samples.push(sample);
    // A faster playback can finish the route inside the window. The page's
    // pump stops when the trip settles, so the last rendered frame's counts
    // would otherwise be read as a lasting gap; ask for one settling frame
    // and take it as the trip's final sample.
    if (sample.follow?.hasArrived === true) {
      await page.evaluate(() => window.mapRequestRenderForTest?.());
      await page.waitForTimeout(FOLLOW_TRIP_ARRIVAL_SETTLE_MILLISECONDS);
      samples.push(await readSample(page));
      break;
    }
    await page.waitForTimeout(sampleIntervalMilliseconds);
  }
  return samples;
}

/** The per-second aggregates of one trip's samples. */
export function summarizeFollowTrip(samples) {
  const first_sample = samples[0];
  const start_seconds = first_sample?.atMilliseconds ?? 0;
  const buckets = new Map();
  for (const sample of samples) {
    const bucket_second = Math.floor(
      (sample.atMilliseconds - start_seconds) / 1_000,
    );
    if (!buckets.has(bucket_second)) {
      buckets.set(bucket_second, {
        second: bucket_second,
        sampleCount: 0,
        requestedTileCount: 0,
        drawnTileCount: 0,
        missingTileCount: 0,
        maximumMissingTileCount: 0,
        missingFrameCount: 0,
        missingTileAddresses: new Set(),
        framesDrawn: 0,
      });
    }
    const bucket = buckets.get(bucket_second);
    const statistics = sample.statistics;
    bucket.sampleCount += 1;
    if (statistics === null || statistics === undefined) continue;
    bucket.requestedTileCount += statistics.requestedTiles ?? 0;
    bucket.drawnTileCount += statistics.drawnTiles ?? 0;
    bucket.missingTileCount += statistics.missingTiles ?? 0;
    bucket.maximumMissingTileCount = Math.max(
      bucket.maximumMissingTileCount,
      statistics.missingTiles ?? 0,
    );
    if ((statistics.missingTiles ?? 0) > 0) bucket.missingFrameCount += 1;
    for (const address of statistics.missingTileAddresses ?? []) {
      bucket.missingTileAddresses.add(address);
    }
    bucket.framesDrawn = Math.max(
      bucket.framesDrawn,
      sample.framesDrawn ?? 0,
    );
  }
  const last_sample = samples[samples.length - 1];
  const provider_events = collectProviderRequestEvents(samples);
  return {
    sampleCount: samples.length,
    speedMultiplier: first_sample?.follow?.speedMultiplier ?? null,
    elapsedMilliseconds:
      (last_sample?.atMilliseconds ?? 0) - start_seconds,
    progressStartMeters:
      first_sample?.progress?.distanceRemainingMeters ?? null,
    progressEndMeters:
      last_sample?.progress?.distanceRemainingMeters ?? null,
    cameraStartZoom: first_sample?.camera?.zoom ?? null,
    cameraEndZoom: last_sample?.camera?.zoom ?? null,
    seconds: [...buckets.values()]
      .sort((left, right) => left.second - right.second)
      .map((bucket) => ({
        ...bucket,
        missingTileAddresses: [...bucket.missingTileAddresses],
      })),
    providerRequestCount: provider_events.length,
    providerRequestSeconds: summarizeProviderRequests(
      provider_events,
      start_seconds,
    ),
  };
}

/** Collects the distinct provider request traces the diagnostics ring holds. */
function collectProviderRequestEvents(samples) {
  const events_by_key = new Map();
  for (const sample of samples) {
    for (const event of sample.diagnostic_events ?? []) {
      const trace = event?.providerRequestTrace;
      if (!trace) continue;
      const key = [
        event.timestampMillisecondsSinceEpoch,
        trace.providerKey,
        trace.contractName,
        trace.requestPath,
      ].join('|');
      events_by_key.set(key, {
        timestampMillisecondsSinceEpoch:
          event.timestampMillisecondsSinceEpoch,
        trace,
      });
    }
  }
  return [...events_by_key.values()];
}

/** Buckets provider requests by the trip second the request completed in. */
function summarizeProviderRequests(events, startMilliseconds) {
  const buckets = new Map();
  for (const event of events) {
    const second = Math.max(
      0,
      Math.floor(
        (event.timestampMillisecondsSinceEpoch - startMilliseconds) / 1_000,
      ),
    );
    buckets.set(second, (buckets.get(second) ?? 0) + 1);
  }
  return [...buckets.entries()]
    .sort(([left], [right]) => left - right)
    .map(([second, count]) => ({ second, count }));
}

/**
 * The harness verdict. In `fixtures` mode a sampled frame after the grace
 * period that reports a missing tile fails; in `live` mode the missing-frame
 * share is compared to the documented budget and reported either way.
 */
export function evaluateFollowTrip(summary, mode) {
  const grace_seconds = FOLLOW_TRIP_FIXTURE_GRACE_MILLISECONDS / 1_000;
  const measured_seconds = summary.seconds.filter(
    (bucket) => bucket.second >= grace_seconds,
  );
  const sampled_second_count = measured_seconds.length;
  const missing_second_count = measured_seconds.filter(
    (bucket) => bucket.missingFrameCount > 0,
  ).length;
  const missing_frame_share = summary.seconds.length === 0
    ? 0
    : summary.seconds.reduce(
      (sum, bucket) => sum + bucket.missingFrameCount,
      0,
    ) / Math.max(
      1,
      summary.seconds.reduce((sum, bucket) => sum + bucket.sampleCount, 0),
    );
  if (summary.sampleCount === 0) {
    return {
      outcome: 'failed',
      reason: 'the trip produced no samples',
      missingFrameShare: 0,
      sampledSecondCount: 0,
    };
  }
  if (mode === 'fixtures') {
    const missing_frames = measured_seconds.reduce(
      (sum, bucket) => sum + bucket.missingFrameCount,
      0,
    );
    return {
      outcome: missing_frames === 0 ? 'passed' : 'failed',
      reason: missing_frames === 0
        ? 'every sampled frame after the grace period drew every requested tile'
        : `${missing_frames} sampled frame(s) after the grace period reported`
          + ' a missing tile',
      missingFrameShare: missing_frame_share,
      sampledSecondCount: sampled_second_count,
      missingSecondCount: missing_second_count,
    };
  }
  return {
    outcome:
      missing_frame_share <= FOLLOW_TRIP_LIVE_MISSING_FRAME_SHARE_BUDGET
        ? 'passed'
        : 'failed',
    reason: `missing-frame share ${(missing_frame_share * 100).toFixed(1)}%`
      + ` against the ${(FOLLOW_TRIP_LIVE_MISSING_FRAME_SHARE_BUDGET * 100)
        .toFixed(0)}% live budget`,
    missingFrameShare: missing_frame_share,
    sampledSecondCount: sampled_second_count,
    missingSecondCount: missing_second_count,
  };
}
