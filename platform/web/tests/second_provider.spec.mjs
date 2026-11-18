// The second-provider proof (M15 phase 5): the
// shared registry declares a second, keyless provider whose single recorded
// vector-tile fixture is served by the generic `/providers/{key}/…` router.
// The descriptor claims no credential label, no wasm registration and no
// platform availability, so the proof is the data path only — no UI
// capability is invented. Every provider fact is read from the registry, so
// this harness never spells a provider name (the provider-name guard scans
// this directory).
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test } from './network_guard.mjs';

const TEST_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const REPOSITORY_ROOT = path.resolve(TEST_DIRECTORY, '..', '..', '..');

/** The shared provider registry the dev server also serves to the page. */
const providers_registry = JSON.parse(
  readFileSync(path.join(REPOSITORY_ROOT, 'platform', 'providers.json'), 'utf8'),
);
const PROVIDER_FIXTURE_BASE_PATH = providers_registry.fixture_base_path;

/**
 * The registry's keyless proof provider that also declares a recorded
 * vector-tile fixture (the router proof's subject). The registry may grow
 * more keyless descriptors; the first with a vector fixture is the one the
 * generic router test exercises.
 */
const keyless_provider_descriptor = providers_registry.providers.find(
  (descriptor) => (descriptor.wasm_registration === null
    || descriptor.wasm_registration === undefined)
    && descriptor.fixture_routes.some(
      (fixture_route) => fixture_route.purpose === 'vector_tiles'),
);
const PROVIDER_KEY = keyless_provider_descriptor.provider_key;
const VECTOR_TILE_ROUTE = keyless_provider_descriptor.fixture_routes.find(
  (fixture_route) => fixture_route.purpose === 'vector_tiles',
);
/** The committed recorded fixture bytes the route must serve unchanged. */
const RECORDED_VECTOR_TILE_BYTES = readFileSync(
  path.join(REPOSITORY_ROOT, VECTOR_TILE_ROUTE.fixture),
);
/** The served route's provider-relative path (the descriptor's exact pattern). */
const SERVED_VECTOR_TILE_PATH = '/vector/sourdough/6/18/27.mvt';
/** A neighbouring address the exact-path pattern must not serve. */
const UNSERVED_VECTOR_TILE_PATH = '/vector/sourdough/6/18/28.mvt';
/** The MVT media type the dev server maps the extension onto. */
const VECTOR_TILE_CONTENT_TYPE = 'application/vnd.mapbox-vector-tile';

test('the registry declares a keyless, unavailable descriptor per data source', () => {
  expect(providers_registry.providers.length).toBeGreaterThanOrEqual(2);
  expect(typeof PROVIDER_KEY).toBe('string');
  // No credential label: the settings surface renders no row for it.
  expect(
    typeof keyless_provider_descriptor.credential_label === 'undefined'
      || keyless_provider_descriptor.credential_label.trim() === '',
  ).toBe(true);
  // No wasm registration and no platform availability: the descriptor proves
  // the data path only, never a capability the build does not have.
  expect(keyless_provider_descriptor.wasm_registration).toBeNull();
  expect(keyless_provider_descriptor.registration_available).toEqual({
    android: false,
    ios: false,
    react_native: false,
    wasm: false,
  });
});

test('the generic router serves the committed vector tile with its media type', async ({
  page,
}) => {
  const response = await page.request.get(
    `${PROVIDER_FIXTURE_BASE_PATH}/${PROVIDER_KEY}${SERVED_VECTOR_TILE_PATH}`,
  );
  expect(response.status()).toBe(200);
  expect(response.headers()['content-type']).toBe(VECTOR_TILE_CONTENT_TYPE);
  expect((await response.body()).equals(RECORDED_VECTOR_TILE_BYTES)).toBe(true);
});

test('addresses outside the descriptor route stay unserved', async ({ page }) => {
  const unserved_route_response = await page.request.get(
    `${PROVIDER_FIXTURE_BASE_PATH}/${PROVIDER_KEY}${UNSERVED_VECTOR_TILE_PATH}`,
  );
  expect(unserved_route_response.status()).toBe(404);

  const unknown_provider_response = await page.request.get(
    `${PROVIDER_FIXTURE_BASE_PATH}/unknown-provider${SERVED_VECTOR_TILE_PATH}`,
  );
  expect(unknown_provider_response.status()).toBe(404);
});

test('the registry is served and carries the second descriptor', async ({ page }) => {
  const response = await page.request.get('/providers.json');
  expect(response.status()).toBe(200);
  const served_registry = await response.json();
  expect(
    served_registry.providers.some(
      (descriptor) => descriptor.provider_key === PROVIDER_KEY,
    ),
  ).toBe(true);
});
