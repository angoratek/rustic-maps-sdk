// The keyless place-search provider end to end in the browser: the registry
// descriptor declares the capability and its same-origin fixture routes, the
// page registers the provider with a deployment address (no credential), and
// the SDK's suggest/search calls fetch, parse and map the recorded provider
// responses. Nothing here touches the network: the fixture server answers the
// descriptor's routes from the committed bytes the provider crate's own tests
// pin. The harness names no vendor: everything comes from the registry.
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
/**
 * The registry descriptor this suite exercises: the keyless provider that
 * serves place search. The harness names no vendor, so the registry alone
 * decides which provider is under test.
 */
const KEYLESS_PLACE_DESCRIPTOR = PROVIDERS_REGISTRY.providers.find(
  (descriptor) =>
    descriptor.served_contracts.includes('place_search')
    && descriptor.credential_label === '',
);
if (KEYLESS_PLACE_DESCRIPTOR === undefined) {
  throw new Error('the registry declares no keyless place-search provider');
}
/** The capability contract the settings surface configures. */
const PLACE_SEARCH_CONTRACT = 'place_search';
/** The fixture route serving a search query. */
const SEARCH_FIXTURE_ROUTE = KEYLESS_PLACE_DESCRIPTOR.fixture_routes.find(
  (fixture_route) => fixture_route.purpose === 'search',
);
if (SEARCH_FIXTURE_ROUTE === undefined) {
  throw new Error('the keyless place provider declares no search fixture route');
}
/** The recorded search answer, the expected rows' source. */
const RECORDED_SEARCH_RESPONSE = JSON.parse(
  readFileSync(path.join(REPOSITORY_ROOT, SEARCH_FIXTURE_ROUTE.fixture), 'utf8'),
);

test.beforeEach(async ({ page }) => {
  await page.goto('/?advanced');
  await waitForMapTestReady(page);
});

/** Registers the keyless place provider with requests aimed at the fixtures. */
async function configure_keyless_place_search(page) {
  await page.evaluate(
    ([contract, provider_key, fixture_base_path]) => {
      window.mapTest.setProviderBaseUrl(
        provider_key,
        `${window.location.origin}${fixture_base_path}/${provider_key}`,
      );
      // A keyless provider takes the deployment address in the credential
      // slot; the base URL override keeps the request same-origin.
      window.mapTest.configureProvider(contract, provider_key, '');
    },
    [
      PLACE_SEARCH_CONTRACT,
      KEYLESS_PLACE_DESCRIPTOR.provider_key,
      PROVIDERS_REGISTRY.fixture_base_path,
    ],
  );
}

test('the keyless place provider suggests and searches over the recorded responses', async ({
  page,
}) => {
  await configure_keyless_place_search(page);

  const suggestion_rows = await page.evaluate(() =>
    window.mapTest.suggestPlaces('Capitol'),
  );
  expect(suggestion_rows.length).toBe(RECORDED_SEARCH_RESPONSE.features.length);
  expect(suggestion_rows[0].structuredAddressText).toBe('Capitol Mall');
  expect(suggestion_rows[0].latitudeDegrees).toBeCloseTo(38.579, 5);
  expect(suggestion_rows[0].longitudeDegrees).toBeCloseTo(-121.493, 5);
  expect(suggestion_rows[0].qualityText).toBe('Street');

  const search_rows = await page.evaluate(() =>
    window.mapTest.searchPlaces('Capitol'),
  );
  expect(search_rows.length).toBe(RECORDED_SEARCH_RESPONSE.features.length);
  // A feature without a name composes its display line from the address
  // components the provider published.
  expect(search_rows[1].structuredAddressText).toBe(
    '1315 10th Street, 95814, Sacramento, California, United States',
  );
  expect(search_rows[1].qualityText).toBe('StreetAddress');
});

test('an empty query is refused before any request', async ({ page }) => {
  await configure_keyless_place_search(page);
  await expect(
    page.evaluate(() => window.mapTest.suggestPlaces('   ')),
  ).rejects.toThrow(/must not be empty/);
});
