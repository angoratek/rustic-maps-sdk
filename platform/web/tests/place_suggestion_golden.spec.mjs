// The golden suggestion line and class for every recorded place-search
// payload: each provider that serves type-ahead pins both values beside its
// fixture route, so the bridge's mapping is proven for every provider rather
// than for one example. A suggestion's text must be exactly the line its
// provider published (never the line with the city appended), and its quality
// must come from the provider's own class (never the point default when the
// payload classifies the row). Choosing a row puts that same line in the
// field. Every provider fact — key, credential requirement, base path, the
// fixture route and its pinned values — comes from the shared registry, and
// the dev server answers the recorded bytes same-origin, so no test touches
// the network.
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
const PLACE_SEARCH_CONTRACT = 'place_search';
/** The pinned values describe the first row of the recorded response. */
const FIRST_RESULT_INDEX = 0;
/** A unique credential, so a credential leak cannot pass by accident. */
const GOLDEN_CREDENTIAL = 'golden-probe-secret-value';
/** The query the field-level case types; the fixture answers by route. */
const FIELD_SUGGESTION_QUERY = 'Capitol';
/** One keystroke's spacing, short of the debounce. */
const KEYSTROKE_INTERVAL_MILLISECONDS = 80;

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
const DEBOUNCE_MILLISECONDS = tokenValue('autocomplete-debounce-milliseconds');

/**
 * One case per place-search fixture route that pins its expected suggestion:
 * the descriptor, the route serving the recorded answer, and the values the
 * SDK must show for its first row.
 */
const SUGGESTION_CASES = PROVIDERS_REGISTRY.providers
  .filter((descriptor) => descriptor.served_contracts.includes(PLACE_SEARCH_CONTRACT))
  .flatMap((descriptor) =>
    (descriptor.fixture_routes ?? [])
      .filter((fixture_route) =>
        fixture_route.purpose === 'search'
        && fixture_route.expectations !== undefined)
      .map((fixture_route) => ({
        descriptor,
        fixture_route,
        expectation: fixture_route.expectations,
      })),
  );

if (SUGGESTION_CASES.length < 2) {
  throw new Error(
    'the registry pins the suggestion line and class for fewer than two '
      + 'place-search providers, so the mapping is proven for one example only',
  );
}

for (const suggestion_case of SUGGESTION_CASES) {
  const { descriptor, expectation } = suggestion_case;
  test(`${descriptor.provider_key} shows the recorded suggestion line and class`, async ({
    page,
  }) => {
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
        PLACE_SEARCH_CONTRACT,
        descriptor.provider_key,
        PROVIDERS_REGISTRY.fixture_base_path,
        descriptor.credential_label === '' ? '' : GOLDEN_CREDENTIAL,
      ],
    );

    const suggestion_rows = await page.evaluate((query) =>
      window.mapTest.suggestPlaces(query), FIELD_SUGGESTION_QUERY);
    expect(suggestion_rows.length).toBeGreaterThan(FIRST_RESULT_INDEX);
    expect(suggestion_rows[FIRST_RESULT_INDEX].structuredAddressText).toBe(
      expectation.suggestion_text,
    );
    expect(suggestion_rows[FIRST_RESULT_INDEX].qualityText).toBe(
      expectation.suggestion_quality,
    );

    const origin_input = page.locator('#trip-origin-input');
    await origin_input.click();
    await origin_input.type(FIELD_SUGGESTION_QUERY, {
      delay: KEYSTROKE_INTERVAL_MILLISECONDS,
    });
    await page.waitForTimeout(DEBOUNCE_MILLISECONDS + 250);
    const first_row = page.locator('#trip-search-results button').first();
    await expect(first_row).toContainText(expectation.suggestion_text);
    await first_row.click();
    await expect(origin_input).toHaveValue(expectation.suggestion_text);
    await expect(page.locator('#trip-search-results')).toBeHidden();
  });
}
