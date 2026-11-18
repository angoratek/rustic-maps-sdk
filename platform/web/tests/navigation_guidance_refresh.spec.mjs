// The demo's live guidance follows the fixes (M71): once a planned route is
// walked, the banner's distance to the maneuver, the remaining distance and
// the travelled position all advance on the fix timer, so a visitor never
// watches a frozen instruction while the camera moves. The route comes from
// the committed provider fixtures, so the case never reaches a network.
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
// The fixes dispatch every 250 ms; fifteen seconds is three maneuvers' worth
// of walking through the fixture route at the demo's pace.
const SAMPLE_INTERVAL_MILLISECONDS = 250;
const SAMPLE_COUNT = 60;

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

test('the guidance banner and progress advance with the fixes', async ({ page }) => {
  test.setTimeout(180_000);
  await page.goto('/?useProviderFixtures');
  await waitForMapTestReady(page);
  await apply_test_credential(page);
  await page.locator('#trip-origin-input').fill('Capitol Park');
  await page.locator('#trip-destination-input').fill('Tower Bridge');
  await page.locator('#trip-plan-route-control').click();
  await expect(page.locator('#trip-route-result')).toBeVisible();
  await page.locator('#follow-demo-start-control').click();

  const samples = [];
  for (let sample_index = 0; sample_index < SAMPLE_COUNT; sample_index += 1) {
    samples.push(
      await page.evaluate(() => {
        const snapshot = window.mapNavigationSnapshot?.();
        const seek_control = document.getElementById('follow-demo-seek-control');
        return {
          travelled: seek_control === null ? null : Number(seek_control.value),
          remaining:
            snapshot?.instruction?.progress?.distanceRemainingMeters ?? null,
          distance_phrase:
            snapshot?.instruction?.activeManeuver?.distancePhraseText ?? null,
        };
      }),
    );
    await page.waitForTimeout(SAMPLE_INTERVAL_MILLISECONDS);
  }

  const travelled_values = samples
    .map((sample) => sample.travelled)
    .filter((value) => value !== null);
  expect(travelled_values.length).toBeGreaterThan(1);
  expect(
    travelled_values[travelled_values.length - 1],
    'the walker must move along the planned route',
  ).toBeGreaterThan(travelled_values[0]);

  const remaining_values = samples
    .map((sample) => sample.remaining)
    .filter((value) => value !== null);
  expect(remaining_values.length).toBeGreaterThan(1);
  expect(
    remaining_values[remaining_values.length - 1],
    'the remaining distance must fall as the trip advances',
  ).toBeLessThan(remaining_values[0]);

  const distinct_distance_phrases = new Set(
    samples
      .map((sample) => sample.distance_phrase)
      .filter((phrase) => phrase !== null && phrase !== ''),
  );
  expect(
    distinct_distance_phrases.size,
    'the banner countdown must change while the trip advances',
  ).toBeGreaterThan(1);
});
