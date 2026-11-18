// North-up holds during a guided trip (M73): the compass is a mode toggle, so
// a tap during navigation keeps the camera at bearing 0 across every
// following fix instead of letting the course chase take it back, and a
// second tap resumes the chase. The route comes from the committed provider
// fixtures, so the case never reaches a network.
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
// The fixture route reports a course at every fix, so the chase moves the
// bearing away from north within a couple of fixes.
const SAMPLE_INTERVAL_MILLISECONDS = 250;
const SAMPLE_COUNT = 12;

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

/** Samples the drawn camera bearing for a few seconds. */
async function sample_bearings(page) {
  const bearings = [];
  for (let sample_index = 0; sample_index < SAMPLE_COUNT; sample_index += 1) {
    bearings.push(
      await page.evaluate(() => window.mapCameraState().bearing),
    );
    await page.waitForTimeout(SAMPLE_INTERVAL_MILLISECONDS);
  }
  return bearings;
}

test('north-up holds across fixes and the compass resumes course-up', async ({
  page,
}) => {
  test.setTimeout(180_000);
  await page.goto('/?useProviderFixtures');
  await waitForMapTestReady(page);
  await apply_test_credential(page);
  await page.locator('#trip-origin-input').fill('Capitol Park');
  await page.locator('#trip-destination-input').fill('Tower Bridge');
  await page.locator('#trip-plan-route-control').click();
  await expect(page.locator('#trip-route-result')).toBeVisible();
  await page.locator('#follow-demo-start-control').click();

  // The chase follows the route's course first.
  const course_up_bearings = await sample_bearings(page);
  expect(
    course_up_bearings.some((bearing) => Math.abs(bearing) > 0.5),
    'the chase must turn the camera with the course before north-up is chosen',
  ).toBe(true);

  // A compass tap during the trip holds north-up. The first samples are the
  // documented eased re-north move (one eased-camera-move duration); every
  // sample after it must stay at 0 across the following fixes.
  await page.locator('#compass-control').click();
  const north_up_bearings = await sample_bearings(page);
  const settled_north_up_bearings = north_up_bearings.slice(2);
  expect(
    settled_north_up_bearings.every((bearing) => Math.abs(bearing) < 0.5),
    `every fix must keep bearing 0 in north-up, got ${JSON.stringify(north_up_bearings)}`,
  ).toBe(true);

  // A second tap resumes the chase.
  await page.locator('#compass-control').click();
  const resumed_bearings = await sample_bearings(page);
  expect(
    resumed_bearings.some((bearing) => Math.abs(bearing) > 0.5),
    'course-up must resume the chase after the second tap',
  ).toBe(true);
});
