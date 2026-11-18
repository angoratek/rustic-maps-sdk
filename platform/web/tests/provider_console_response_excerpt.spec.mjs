// The provider console's response-excerpt switch: rows carry no response
// prefix until the switch is on, the captured prefix is bounded and redacted,
// and the export carries it as typed data. The dev server serves the recorded
// fixture same-origin, so no test touches the network.
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

const TEST_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const REPOSITORY_ROOT = path.resolve(TEST_DIRECTORY, '..', '..', '..');
const PROVIDERS_REGISTRY = JSON.parse(
  readFileSync(
    path.join(REPOSITORY_ROOT, 'platform', 'providers.json'),
    'utf8',
  ),
);

/** The credentialed geocoding provider whose fixture route this suite uses. */
const GEOCODING_DESCRIPTOR = PROVIDERS_REGISTRY.providers.find(
  (descriptor) =>
    descriptor.credential_label !== ''
    && descriptor.served_contracts.includes('geocoding')
    && descriptor.fixture_routes.some(
      (fixture_route) => fixture_route.purpose === 'geocoding',
    ),
);
if (GEOCODING_DESCRIPTOR === undefined) {
  throw new Error(
    'the registry declares no credentialed geocoding provider with a fixture route',
  );
}

/** A unique credential, so the redaction assertion cannot pass by accident. */
const EXCERPT_CREDENTIAL = 'excerpt-probe-secret-value';
/** The excerpt cap the observation documents, in bytes. */
const RESPONSE_EXCERPT_MAXIMUM_BYTES = 512;

test.beforeEach(async ({ page }) => {
  await page.goto('/?advanced');
  await waitForMapTestReady(page);
  await page.evaluate(
    ([contract, provider_key, fixture_base_path, credential]) => {
      window.mapTest.setProviderBaseUrl(
        provider_key,
        `${window.location.origin}${fixture_base_path}/${provider_key}`,
      );
      window.mapTest.configureProvider(contract, provider_key, credential);
    },
    [
      'geocoding',
      GEOCODING_DESCRIPTOR.provider_key,
      PROVIDERS_REGISTRY.fixture_base_path,
      EXCERPT_CREDENTIAL,
    ],
  );
});

test('the console carries a response excerpt only after the switch is on', async ({
  page,
}) => {
  await page.evaluate(() => window.mapTest.forwardGeocode('Sacramento,California'));
  await page.locator('#provider-console-control').click();
  const console_rows = page.locator('#provider-console-rows li');
  await expect(console_rows.first()).toBeVisible();

  // Off by default: the request is listed, its body is not.
  await expect(page.locator('.provider-console-excerpt')).toHaveCount(0);

  const capture_switch = page.locator('#provider-console-capture');
  await expect(capture_switch).not.toBeChecked();
  await capture_switch.check();

  await page.evaluate(() => window.mapTest.forwardGeocode('Sacramento,California'));
  const excerpt = page.locator('.provider-console-excerpt').first();
  await expect(excerpt).toBeVisible();
  const excerpt_text = await excerpt.textContent();
  expect(excerpt_text.length).toBeGreaterThan(0);
  expect(excerpt_text.length).toBeLessThanOrEqual(RESPONSE_EXCERPT_MAXIMUM_BYTES);
  expect(excerpt_text).not.toContain(EXCERPT_CREDENTIAL);
  // The captured prefix is the provider's answer, not an empty placeholder.
  expect(excerpt_text.trim().startsWith('{')).toBe(true);

  // The typed export carries the excerpt beside the request facts.
  const [download] = await Promise.all([
    page.waitForEvent('download'),
    page.locator('#provider-console-export').click(),
  ]);
  const exported_rows = JSON.parse(readFileSync(await download.path(), 'utf8'));
  const row_with_excerpt = exported_rows.find(
    (row) => typeof row.responseExcerpt === 'string' && row.responseExcerpt.length > 0,
  );
  expect(row_with_excerpt).toBeDefined();
  expect(row_with_excerpt.responseExcerpt).not.toContain(EXCERPT_CREDENTIAL);
});
