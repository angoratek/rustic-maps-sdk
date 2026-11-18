// The provider console panel: opening it lists one row per provider request,
// each prefixed with the provider's display name from the shared registry,
// filtering narrows the rows, and neither the rows nor the exported JSON carry
// the credential. The dev server serves the recorded fixture same-origin, so no
// test touches the network.
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

/** The capability contract the settings surface configures. */
const GEOCODING_CONTRACT = 'geocoding';
/** A unique credential, so the redaction assertion cannot pass by accident. */
const CONSOLE_CREDENTIAL = 'console-probe-secret-value';
/** The contract name the console row shows, the provider layer's own. */
const GEOCODING_PROVIDER_CONTRACT_NAME = 'GeocodingProvider';

/** The request path the registry declares for one fixture purpose. */
function request_path_for_purpose(purpose) {
  const fixture_route = GEOCODING_DESCRIPTOR.fixture_routes.find(
    (candidate) => candidate.purpose === purpose,
  );
  if (fixture_route === undefined) {
    throw new Error(
      `the registry declares no ${purpose} fixture route for the console provider`,
    );
  }
  // `^geocoding/v1/address$` → `/geocoding/v1/address`.
  return `/${fixture_route.path_pattern.replace(/^\^|\$$/g, '')}`;
}

/**
 * One capability the console lists: the contract to configure, the request the
 * demo issues, the contract name the row shows, and the registry fixture route
 * the row's path comes from (`null` for a tile route, whose path carries
 * segments the registry serves as a directory).
 */
const CAPABILITY_CASES = [
  {
    contract: 'routing',
    reportedContract: 'RoutingProvider',
    fixturePurpose: 'directions',
    method: 'GET',
    invoke: (page) => page.evaluate(() =>
      window.mapTest.calculateRoute(
        38.57693,
        -121.49492,
        38.578739,
        -121.507447,
      )),
  },
  {
    contract: 'place_search',
    reportedContract: 'SearchPlaceProvider',
    fixturePurpose: 'search',
    method: 'GET',
    invoke: (page) => page.evaluate(() => window.mapTest.searchPlaces('Denver')),
  },
  {
    contract: 'geocoding',
    reportedContract: GEOCODING_PROVIDER_CONTRACT_NAME,
    fixturePurpose: 'reverse_geocoding',
    method: 'GET',
    invoke: (page) => page.evaluate(() =>
      window.mapTest.reverseGeocode(38.57693, -121.49492)),
  },
  {
    contract: 'distance_matrix',
    reportedContract: 'DistanceMatrixProvider',
    fixturePurpose: 'distance_matrix',
    method: 'POST',
    invoke: (page) => page.evaluate(() =>
      window.mapTest.calculateDistanceMatrix(
        [38.57693, -121.49492, 38.578739, -121.507447],
        [38.5816, -121.4944],
        'car',
      )),
  },
  {
    contract: 'traffic_incidents',
    reportedContract: 'TrafficIncidentProvider',
    fixturePurpose: 'traffic_incidents',
    method: 'GET',
    invoke: (page) => page.evaluate(() =>
      window.mapTest.fetchTrafficIncidents(39.7, -105.1, 39.9, -104.8)),
  },
  {
    contract: 'traffic_flow',
    reportedContract: 'TrafficFlowProvider',
    fixturePurpose: 'traffic_flow',
    method: 'GET',
    invoke: (page) => page.evaluate(() =>
      window.mapTest.fetchTrafficFlow(40.7, -74.05, 40.8, -73.95)),
  },
  {
    contract: 'raster_tiles',
    reportedContract: 'RasterTileProvider',
    fixturePurpose: null,
    method: 'GET',
    invoke: (page) => page.evaluate(() =>
      window.mapTest.fetchRasterTile(15, 5323, 12574)),
  },
];

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
      GEOCODING_CONTRACT,
      GEOCODING_DESCRIPTOR.provider_key,
      PROVIDERS_REGISTRY.fixture_base_path,
      CONSOLE_CREDENTIAL,
    ],
  );
});

test('a provider request becomes one console row prefixed with the provider name', async ({
  page,
}) => {
  const rows = await page.evaluate(() =>
    window.mapTest.forwardGeocode('Sacramento,California'),
  );
  expect(rows.length).toBeGreaterThan(0);

  const console_control = page.locator('#provider-console-control');
  const console_panel = page.locator('#provider-console-panel');
  await expect(console_panel).toBeHidden();

  await console_control.click();
  await expect(console_panel).toBeVisible();
  await expect(console_control).toHaveAttribute('aria-expanded', 'true');

  const console_rows = page.locator('#provider-console-rows li');
  await expect(console_rows.first()).toBeVisible();
  const row_text = await console_rows.first().textContent();
  expect(row_text).toContain(GEOCODING_DESCRIPTOR.display_name);
  expect(row_text).toContain(GEOCODING_PROVIDER_CONTRACT_NAME);
  expect(row_text).toContain('GET');
  expect(row_text).toContain('2xx');
  expect(row_text).not.toContain(CONSOLE_CREDENTIAL);

  await console_control.click();
  await expect(console_panel).toBeHidden();
  await expect(console_control).toHaveAttribute('aria-expanded', 'false');
});

test('the console filters by provider and by outcome', async ({ page }) => {
  await page.evaluate(() => window.mapTest.forwardGeocode('Sacramento,California'));
  await page.locator('#provider-console-control').click();
  const console_rows = page.locator('#provider-console-rows li');
  await expect(console_rows.first()).toBeVisible();

  // The provider filter is filled from the registry, never a hardcoded list.
  const provider_filter = page.locator('#provider-console-provider-filter');
  await expect(
    provider_filter.locator(`option[value="${GEOCODING_DESCRIPTOR.provider_key}"]`),
  ).toHaveText(GEOCODING_DESCRIPTOR.display_name);

  await provider_filter.selectOption(GEOCODING_DESCRIPTOR.provider_key);
  await expect(console_rows.first()).toContainText(GEOCODING_DESCRIPTOR.display_name);

  const outcome_filter = page.locator('#provider-console-outcome-filter');
  await outcome_filter.selectOption('client-error');
  await expect(console_rows).toHaveCount(0);
  await expect(page.locator('#provider-console-empty')).toBeVisible();

  await outcome_filter.selectOption('success');
  await expect(console_rows.first()).toContainText('2xx');
});

test('the console export downloads the visible rows without the credential', async ({
  page,
}) => {
  await page.evaluate(() => window.mapTest.forwardGeocode('Sacramento,California'));
  await page.locator('#provider-console-control').click();
  await expect(page.locator('#provider-console-rows li').first()).toBeVisible();

  const [download] = await Promise.all([
    page.waitForEvent('download'),
    page.locator('#provider-console-export').click(),
  ]);
  const download_path = await download.path();
  const exported_rows = JSON.parse(readFileSync(download_path, 'utf8'));
  expect(exported_rows.length).toBeGreaterThan(0);
  expect(exported_rows[0].providerKey).toBe(GEOCODING_DESCRIPTOR.provider_key);
  expect(exported_rows[0].providerContract).toBe(
    GEOCODING_PROVIDER_CONTRACT_NAME,
  );
  expect(JSON.stringify(exported_rows)).not.toContain(CONSOLE_CREDENTIAL);
});


test('every capability request becomes a row that names the capability', async ({
  page,
}) => {
  await page.locator('#provider-console-control').click();

  for (const capability_case of CAPABILITY_CASES) {
    await page.evaluate(
      ([contract, provider_key, credential]) => {
        window.mapTest.configureProvider(contract, provider_key, credential);
      },
      [
        capability_case.contract,
        GEOCODING_DESCRIPTOR.provider_key,
        CONSOLE_CREDENTIAL,
      ],
    );
    await capability_case.invoke(page);

    const capability_rows = page
      .locator('#provider-console-rows li')
      .filter({ hasText: capability_case.reportedContract });
    // Rows render newest first, so the row this request just produced is the
    // first of its contract's rows.
    const newest_row = capability_rows.first();
    await expect(newest_row).toContainText(GEOCODING_DESCRIPTOR.display_name);
    await expect(newest_row).toContainText(`${capability_case.method} `);
    await expect(newest_row).toContainText('2xx');
    if (capability_case.fixturePurpose !== null) {
      await expect(newest_row).toContainText(
        request_path_for_purpose(capability_case.fixturePurpose),
      );
    }
    await expect(newest_row).not.toContainText(CONSOLE_CREDENTIAL);
  }
});
