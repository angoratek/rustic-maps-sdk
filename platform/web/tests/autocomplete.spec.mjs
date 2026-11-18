// The shared type-ahead rule (docs/ReferenceAppUX.md §5.5) in the browser:
// a query needs three characters, waits 250 ms of quiet before asking the
// provider, restarts the timer on every keystroke (so a superseded query
// never fires), shows at most five rows, and filling a field from a row
// closes the list. The provider is the registry's keyless place provider
// pointed at the same-origin fixtures, so no test touches the network.
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
const KEYLESS_PLACE_DESCRIPTOR = PROVIDERS_REGISTRY.providers.find(
  (descriptor) =>
    descriptor.served_contracts.includes('place_search')
    && descriptor.credential_label === '',
);
if (KEYLESS_PLACE_DESCRIPTOR === undefined) {
  throw new Error('the registry declares no keyless place-search provider');
}
const PLACE_SEARCH_CONTRACT = 'place_search';
/** The rule's values, read from the shared tokens. */
const TOKENS = JSON.parse(
  readFileSync(
    path.join(REPOSITORY_ROOT, 'platform', 'reference-tokens.json'),
    'utf8',
  ),
).behavior_tokens;
function tokenValue(tokenName) {
  const token = TOKENS.find((candidate) => candidate.token === tokenName);
  if (token === undefined) {
    throw new Error(`the token "${tokenName}" is missing`);
  }
  return token.value;
}
const MINIMUM_QUERY_CHARACTERS = tokenValue(
  'autocomplete-minimum-query-characters',
);
const DEBOUNCE_MILLISECONDS = tokenValue('autocomplete-debounce-milliseconds');
const MAXIMUM_RESULTS = tokenValue('autocomplete-maximum-results');
/** One keystroke's spacing, short of the debounce. */
const KEYSTROKE_INTERVAL_MILLISECONDS = 80;

test.beforeEach(async ({ page }) => {
  await page.goto('/?advanced');
  await waitForMapTestReady(page);
  await page.evaluate(
    ([contract, providerKey, fixtureBasePath]) => {
      window.mapTest.setProviderBaseUrl(
        providerKey,
        `${window.location.origin}${fixtureBasePath}/${providerKey}`,
      );
      window.mapTest.configureProvider(contract, providerKey, '');
    },
    [
      PLACE_SEARCH_CONTRACT,
      KEYLESS_PLACE_DESCRIPTOR.provider_key,
      PROVIDERS_REGISTRY.fixture_base_path,
    ],
  );
});

/** Counts the provider requests the page issues through the fixture route. */
async function count_provider_requests(page) {
  const request_log = [];
  await page.route(
    `**${PROVIDERS_REGISTRY.fixture_base_path}/${KEYLESS_PLACE_DESCRIPTOR.provider_key}/**`,
    (route) => {
      request_log.push(route.request().url());
      return route.continue();
    },
  );
  return request_log;
}

test('a short query never asks the provider and shows no rows', async ({ page }) => {
  const request_log = await count_provider_requests(page);
  const origin_input = page.locator('#trip-origin-input');
  await origin_input.click();
  await origin_input.fill('Ca'.padEnd(MINIMUM_QUERY_CHARACTERS - 1, 'a'));
  await page.waitForTimeout(DEBOUNCE_MILLISECONDS + 200);
  expect(request_log).toHaveLength(0);
  await expect(page.locator('#trip-search-results')).toBeHidden();
});

test('the quiet timer restarts on every keystroke and only the final query asks', async ({
  page,
}) => {
  const request_log = await count_provider_requests(page);
  const origin_input = page.locator('#trip-origin-input');
  await origin_input.click();
  await origin_input.type('Cap', { delay: KEYSTROKE_INTERVAL_MILLISECONDS });
  await page.waitForTimeout(DEBOUNCE_MILLISECONDS + 250);
  expect(request_log).toHaveLength(1);
  await expect(page.locator('#trip-search-results')).toBeVisible();
  const result_rows = page.locator('#trip-search-results button');
  expect(await result_rows.count()).toBeLessThanOrEqual(MAXIMUM_RESULTS);
  await expect(result_rows.first()).toContainText('Capitol Mall');
});

test('choosing a row fills the field and closes the list', async ({ page }) => {
  await count_provider_requests(page);
  const origin_input = page.locator('#trip-origin-input');
  await origin_input.click();
  await origin_input.type('Capitol', { delay: KEYSTROKE_INTERVAL_MILLISECONDS });
  await page.waitForTimeout(DEBOUNCE_MILLISECONDS + 250);
  const first_row = page.locator('#trip-search-results button').first();
  await expect(first_row).toContainText('Capitol Mall');
  await first_row.click();
  await expect(origin_input).toHaveValue('Capitol Mall');
  await expect(page.locator('#trip-search-results')).toBeHidden();
});
