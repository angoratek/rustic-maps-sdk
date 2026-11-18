// A simulated trip keeps the follow camera covered (PLAN.md M71).
//
// The trip is the hardest tile case: the camera follows a moving fix stream at
// the recorded follow zoom, so every second asks the scheduler for addresses
// the previous second did not, while guidance and the puck ride the same pump.
// The spec plans the recorded demo route, starts the follow demo, samples the
// page's own frame statistics, and fails when any sampled frame after the
// grace period reports a tile its own or an ancestor's pooled imagery could
// not cover, or when the guidance stops advancing. The measuring harness is
// `perf/run-follow-trip-budget.mjs`, which records the readable numbers.
import { expect, test, waitForMapTestReady } from './network_guard.mjs';
import {
  FOLLOW_TRIP_FIXTURE_GRACE_MILLISECONDS,
  FOLLOW_TRIP_SAMPLE_INTERVAL_MILLISECONDS,
  planAndStartFollowTrip,
  sampleFollowTrip,
  summarizeFollowTrip,
  waitForFollowTripReady,
} from '../perf/follow_trip_budget_harness.mjs';

/** How long the trip is sampled; long enough to cross one maneuver. */
const FOLLOW_TRIP_SAMPLE_WINDOW_MILLISECONDS = 8_000;

test('a simulated trip keeps every requested tile covered', async ({ page }) => {
  await page.goto('/?advanced');
  await waitForMapTestReady(page);
  await waitForFollowTripReady(page);

  await planAndStartFollowTrip(page);
  const samples = await sampleFollowTrip(
    page,
    FOLLOW_TRIP_SAMPLE_WINDOW_MILLISECONDS,
    FOLLOW_TRIP_SAMPLE_INTERVAL_MILLISECONDS,
  );
  const summary = summarizeFollowTrip(samples);

  // The trip is moving: a still camera would make the tile assertions weak.
  expect(summary.sampleCount).toBeGreaterThan(0);
  expect(summary.progressStartMeters).not.toBeNull();
  expect(summary.progressEndMeters).toBeLessThan(summary.progressStartMeters);

  // The frames are real GPU frames, not a page that stopped drawing.
  const last_bucket = summary.seconds[summary.seconds.length - 1];
  expect(last_bucket.framesDrawn).toBeGreaterThan(0);

  // After the grace period every sampled frame drew every requested tile,
  // own-level or from its nearest pooled ancestor.
  const grace_seconds = FOLLOW_TRIP_FIXTURE_GRACE_MILLISECONDS / 1_000;
  const measured_seconds = summary.seconds.filter(
    (bucket) => bucket.second >= grace_seconds,
  );
  expect(measured_seconds.length).toBeGreaterThan(0);
  for (const bucket of measured_seconds) {
    expect(
      bucket.missingFrameCount,
      `second ${bucket.second} reported ${bucket.missingTileCount} missing`
        + ` tile(s): ${bucket.missingTileAddresses.join(', ')}`,
    ).toBe(0);
  }
});
