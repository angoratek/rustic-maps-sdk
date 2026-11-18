// The location puck glides between fixes (M72): during a simulated trip the
// pose the canvas draws moves continuously at the frame cadence, never in
// fix-sized hops, and a fix that lands mid-flight retargets without a jump.
// The route comes from the committed provider fixtures, so the case never
// reaches a network.
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

const TEST_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const REPOSITORY_ROOT = path.resolve(TEST_DIRECTORY, '..', '..', '..');
const providers_registry = JSON.parse(
  readFileSync(path.join(REPOSITORY_ROOT, 'platform', 'providers.json'), 'utf8'),
);
const provider_descriptor = providers_registry.providers.find(
  (candidate) => candidate.registration_available?.wasm === true
    && typeof candidate.credential_label === 'string',
);
const PROVIDER_KEY = provider_descriptor.provider_key;
const PROVIDER_DISPLAY_NAME = provider_descriptor.display_name;
// The demo's walking pace at 1x is about 28 m/s. Interpolated motion keeps
// that speed whatever the frame time is; a puck drawn only on fixes hops the
// ~7 m between them inside one frame, which reads as hundreds of meters per
// second. The bound sits far above the pace and far below a hop.
const MAXIMUM_PUCK_SPEED_METERS_PER_SECOND = 120.0;
// A sample interval longer than this is a stalled frame, not a motion test.
const MAXIMUM_MEASURED_INTERVAL_MILLISECONDS = 250;
const SAMPLE_DURATION_MILLISECONDS = 3000;
const METERS_PER_DEGREE_LATITUDE = 111_320;
const METERS_PER_DEGREE_LONGITUDE_EQ = 111_320;

/** Applies the registry provider's test credential through the settings UI. */
async function apply_test_credential(page) {
  await page.locator('#settings-control').click();
  await expect(page.locator('#settings-dialog')).toBeVisible();
  const provider_row = page.locator(
    `#provider-credential-rows [data-provider-key="${PROVIDER_KEY}"]`,
  );
  await provider_row.locator('[data-provider-credential-input]').fill('test-key');
  await provider_row.locator('[data-provider-credential-apply]').click();
  await expect(page.locator('#provider-credential-status')).toHaveText(
    `${PROVIDER_DISPLAY_NAME} key applied`,
  );
  await page.keyboard.press('Escape');
  await expect(page.locator('#settings-dialog')).toBeHidden();
}

test('the location puck moves continuously between fixes', async ({ page }) => {
  test.setTimeout(180_000);
  await page.goto('/?useProviderFixtures');
  await waitForMapTestReady(page);
  await apply_test_credential(page);
  await page.locator('#trip-origin-input').fill('Capitol Park');
  await page.locator('#trip-destination-input').fill('Tower Bridge');
  await page.locator('#trip-plan-route-control').click();
  await expect(page.locator('#trip-route-result')).toBeVisible();
  await page.locator('#follow-demo-start-control').click();

  const samples = await page.evaluate(
    async (sample_duration_milliseconds) => {
      const collected = [];
      const started_at = performance.now();
      await new Promise((resolve) => {
        const collect = () => {
          const pose = window.mapLocationPose();
          if (pose !== null) {
            collected.push({
              sampledAt: performance.now(),
              latitude: pose.latitudeDegrees,
              longitude: pose.longitudeDegrees,
              isAnimating: pose.isAnimating,
            });
          }
          if (performance.now() - started_at < sample_duration_milliseconds) {
            requestAnimationFrame(collect);
          } else {
            resolve();
          }
        };
        requestAnimationFrame(collect);
      });
      return collected;
    },
    SAMPLE_DURATION_MILLISECONDS,
  );

  expect(samples.length).toBeGreaterThan(30);
  const animating_samples = samples.filter((sample) => sample.isAnimating);
  expect(
    animating_samples.length,
    'the puck must be mid-motion for most of a running trip',
  ).toBeGreaterThan(samples.length / 2);

  let largest_speed_meters_per_second = 0;
  let total_moved_meters = 0;
  for (let sample_index = 1; sample_index < samples.length; sample_index += 1) {
    const previous = samples[sample_index - 1];
    const current = samples[sample_index];
    const interval_milliseconds = current.sampledAt - previous.sampledAt;
    const latitude_delta_meters =
      (current.latitude - previous.latitude) * METERS_PER_DEGREE_LATITUDE;
    const longitude_delta_meters =
      (current.longitude - previous.longitude) * METERS_PER_DEGREE_LONGITUDE_EQ;
    const delta_meters = Math.hypot(latitude_delta_meters, longitude_delta_meters);
    total_moved_meters += delta_meters;
    if (
      interval_milliseconds > 0
      && interval_milliseconds <= MAXIMUM_MEASURED_INTERVAL_MILLISECONDS
    ) {
      largest_speed_meters_per_second = Math.max(
        largest_speed_meters_per_second,
        delta_meters / (interval_milliseconds / 1000),
      );
    }
  }
  expect(
    largest_speed_meters_per_second,
    'the drawn pose may never move faster than the trip it follows',
  ).toBeLessThan(MAXIMUM_PUCK_SPEED_METERS_PER_SECOND);
  expect(
    total_moved_meters,
    'the puck must actually travel during the sample',
  ).toBeGreaterThan(10);
});
