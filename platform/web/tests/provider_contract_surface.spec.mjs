// The contract-shaped provider surface on the WebAssembly target: one call
// per capability, the per-capability registration readback, the routing
// capability answer, and the URL-template raster source. Every provider fact
// comes from the shared registry `platform/providers.json`, and every provider
// response is a committed fixture served same-origin by the dev server, so no
// external network is touched and no vendor path is named here.
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

const TEST_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const REPOSITORY_ROOT = path.resolve(TEST_DIRECTORY, '..', '..', '..');

/** The shared provider registry the dev server also serves to the page. */
const providers_registry = JSON.parse(
  readFileSync(path.join(REPOSITORY_ROOT, 'platform', 'providers.json'), 'utf8'),
);

/** The descriptor under test (the registry is seeded with one provider). */
const provider_descriptor = providers_registry.providers[0];
const PROVIDER_KEY = provider_descriptor.provider_key;
const PROVIDER_WASM_REGISTRATION = provider_descriptor.wasm_registration;
const PROVIDER_DISPLAY_NAME = provider_descriptor.display_name;
const PROVIDER_FIXTURE_BASE_PATH = providers_registry.fixture_base_path;

/** The capabilities the descriptor declares, in the contract's spelling. */
const PROVIDER_SERVED_CONTRACT_NAMES = provider_descriptor.served_contracts.map(
  (contract_name) => contract_name
    .split('_')
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join(''),
);

/** Every capability the contract defines, in the readback's stable order. */
const ALL_CONTRACT_NAMES = [
  'Geocoding',
  'Routing',
  'DistanceMatrix',
  'PlaceSearch',
  'TrafficIncidents',
  'TrafficFlow',
  'Elevation',
  'RasterTiles',
  'VectorTiles',
  'OfflineRouting',
];

/** The demo coordinate the recorded reverse-geocoding fixture answers for. */
const REVERSE_GEOCODE_LATITUDE_DEGREES = 38.5766;
const REVERSE_GEOCODE_LONGITUDE_DEGREES = -121.4944;

/**
 * Registers the descriptor's provider for the whole capability set, with its
 * requests aimed at the same-origin fixture routes: the browser-only base URL
 * override, then the contract-shaped credential call.
 */
async function apply_test_provider_registration(page) {
  await page.evaluate(
    ([registration_key, provider_key, fixture_base_path]) => {
      window.mapTest.setProviderBaseUrl(
        registration_key,
        `${window.location.origin}${fixture_base_path}/${provider_key}`,
      );
      window.mapTest.configureProviderSet(provider_key, 'test-key');
    },
    [PROVIDER_WASM_REGISTRATION, PROVIDER_KEY, PROVIDER_FIXTURE_BASE_PATH],
  );
}

/** The registration rows, as `{contract, providerKey, displayName, registered}`. */
async function read_provider_registrations(page) {
  return page.evaluate(() => window.mapTest.providerRegistrations());
}

test.beforeEach(async ({ page }) => {
  await page.goto('/?advanced');
  await waitForMapTestReady(page);
});

test('the registration readback names no provider until one is configured', async ({
  page,
}) => {
  const registrations = await read_provider_registrations(page);
  expect(registrations.map((registration) => registration.contract))
    .toEqual(ALL_CONTRACT_NAMES);
  for (const registration of registrations) {
    expect(registration.registered).toBe(false);
    expect(registration.providerKey).toBe('');
    expect(registration.displayName).toBe('');
  }
});

test('one credential registers every capability the provider serves', async ({
  page,
}) => {
  await apply_test_provider_registration(page);
  const registrations = await read_provider_registrations(page);
  for (const registration of registrations) {
    const is_served = PROVIDER_SERVED_CONTRACT_NAMES.includes(
      registration.contract,
    );
    expect(registration.registered).toBe(is_served);
    expect(registration.providerKey).toBe(is_served ? PROVIDER_KEY : '');
    expect(registration.displayName)
      .toBe(is_served ? PROVIDER_DISPLAY_NAME : '');
  }
  // Clearing every slot returns the surface to its keyless answer.
  await page.evaluate(() => window.mapTest.clearProviderRegistrations());
  const cleared_registrations = await read_provider_registrations(page);
  for (const registration of cleared_registrations) {
    expect(registration.registered).toBe(false);
  }
});

test('configuring one capability leaves the others empty', async ({ page }) => {
  await page.evaluate(
    ([registration_key]) =>
      window.mapTest.configureProvider('Routing', registration_key, 'test-key'),
    [PROVIDER_WASM_REGISTRATION],
  );
  const registrations = await read_provider_registrations(page);
  const routing_registration = registrations.find(
    (registration) => registration.contract === 'Routing',
  );
  expect(routing_registration.registered).toBe(true);
  expect(routing_registration.providerKey).toBe(PROVIDER_KEY);
  const other_registrations = registrations.filter(
    (registration) => registration.contract !== 'Routing',
  );
  for (const registration of other_registrations) {
    expect(registration.registered).toBe(false);
  }
});

test('a refused configuration leaves the serving provider in place', async ({
  page,
}) => {
  await page.evaluate(
    ([registration_key]) =>
      window.mapTest.configureProvider('Routing', registration_key, 'test-key'),
    [PROVIDER_WASM_REGISTRATION],
  );
  // An unknown key is refused with the key in the message.
  await expect(
    page.evaluate(() =>
      window.mapTest.configureProvider('Routing', 'unknown-provider', 'test-key'),
    ),
  ).rejects.toThrow(/unknown-provider/);
  // A provider that requires a credential refuses a blank one.
  await expect(
    page.evaluate(
      ([registration_key]) =>
        window.mapTest.configureProvider('Routing', registration_key, '   '),
      [PROVIDER_WASM_REGISTRATION],
    ),
  ).rejects.toThrow(/requires a credential/);
  // The refusal changed nothing: the routing slot still answers.
  const registrations = await read_provider_registrations(page);
  const routing_registration = registrations.find(
    (registration) => registration.contract === 'Routing',
  );
  expect(routing_registration.registered).toBe(true);
  expect(routing_registration.providerKey).toBe(PROVIDER_KEY);
});

test('the routing capability readback reports what the provider honours', async ({
  page,
}) => {
  // Without a routing provider the readback is the contract's refusal.
  await expect(
    page.evaluate(() => window.mapTest.routeCapabilities()),
  ).rejects.toThrow(/RoutingProvider/);

  await page.evaluate(
    ([registration_key]) =>
      window.mapTest.configureProvider('Routing', registration_key, 'test-key'),
    [PROVIDER_WASM_REGISTRATION],
  );
  const capabilities = await page.evaluate(
    () => window.mapTest.routeCapabilities(),
  );
  expect(capabilities.supportedTransportationModes).toEqual([
    'Automobile',
    'Bicycle',
    'Pedestrian',
  ]);
  expect(capabilities.supportsShortestRouteStrategy).toBe(true);
});

test('reverse geocoding answers the recorded place through the browser transport', async ({
  page,
}) => {
  await apply_test_provider_registration(page);
  const rows = await page.evaluate(
    ([latitude_degrees, longitude_degrees]) =>
      window.mapTest.reverseGeocode(latitude_degrees, longitude_degrees),
    [REVERSE_GEOCODE_LATITUDE_DEGREES, REVERSE_GEOCODE_LONGITUDE_DEGREES],
  );
  expect(rows.length).toBe(1);
  expect(rows[0].structuredAddressText).toContain('Sacramento');
  expect(rows[0].latitudeDegrees).toBe(REVERSE_GEOCODE_LATITUDE_DEGREES);
  expect(rows[0].longitudeDegrees).toBe(REVERSE_GEOCODE_LONGITUDE_DEGREES);
});

test('a stalled provider fetch is aborted at the request deadline', async ({
  page,
}) => {
  // The browser transport's deadline is 30 seconds and a transient network
  // failure carries the provider's two-retry budget, so the request settles
  // only after three deadlines and their back-offs (about 92 seconds).
  test.setTimeout(120_000);
  await apply_test_provider_registration(page);
  // Never fulfil the provider's request: the deadline, not the server, must
  // settle the call, so a stalled connection cannot pin an in-flight slot.
  await page.route(
    `**${PROVIDER_FIXTURE_BASE_PATH}/${PROVIDER_KEY}/**`,
    () => {},
  );
  // Traffic flow is a single attempt (no retry budget), so one deadline
  // settles the call; an idempotent GET would exhaust its retries first.
  const failure_text = await page.evaluate(
    ([south_west_latitude, south_west_longitude, north_east_latitude, north_east_longitude]) =>
      window.mapTest
        .fetchTrafficFlow(
          south_west_latitude,
          south_west_longitude,
          north_east_latitude,
          north_east_longitude,
        )
        .then(
          () => null,
          (error) => String(error?.message ?? error),
        ),
    [40.6, -74.05, 40.65, -74.0],
  );
  expect(failure_text).toMatch(/request timed out/);
});

test('elevation refuses while no elevation provider is configured', async ({
  page,
}) => {
  await apply_test_provider_registration(page);
  // The provider set this build carries serves no elevation: the contract's
  // own refusal names the capability rather than inventing samples.
  await expect(
    page.evaluate(() =>
      window.mapTest.fetchElevationPoints([38.5766, -121.4944]),
    ),
  ).rejects.toThrow(/ElevationProvider/);
});

test('a repeated traffic request over the same ground answers from the cache', async ({
  page,
}) => {
  await apply_test_provider_registration(page);

  const traffic_descriptor = providers_registry.providers.find(
    (provider) => (provider.served_contracts ?? []).includes('traffic_flow'),
  );
  expect(traffic_descriptor).toBeDefined();
  const traffic_fixture_route = traffic_descriptor.fixture_routes.find(
    (fixture_route) => fixture_route.purpose === 'traffic_flow',
  );
  expect(traffic_fixture_route).toBeDefined();

  // The recorded flow fixture's own box: the same one the live-provider
  // suite uses, so the answer holds the fixture's four segments.
  const flow_box = [40.6, -74.05, 40.65, -74.0];
  const first_answer = await page.evaluate((box) =>
    window.mapTest.fetchTrafficFlow(box[0], box[1], box[2], box[3]), flow_box);
  expect(Array.isArray(first_answer)).toBe(true);
  expect(first_answer.length).toBeGreaterThan(0);

  const statistics_after_first_request = await page.evaluate(
    () => window.mapTest.trafficFlowCacheStatistics(),
  );
  expect(statistics_after_first_request).not.toBeNull();
  expect(statistics_after_first_request.missCount).toBe(1);
  expect(statistics_after_first_request.hitCount).toBe(0);
  expect(statistics_after_first_request.entryCount).toBeGreaterThan(0);

  // A nudge of the same camera: a different box, the same grid cell. The
  // answer is what the cell holds (the segment whose first point falls in
  // it), not a second provider call.
  const nudged_box = [40.601, -74.049, 40.651, -73.999];
  const second_answer = await page.evaluate((box) =>
    window.mapTest.fetchTrafficFlow(box[0], box[1], box[2], box[3]), nudged_box);
  expect(second_answer.length).toBeGreaterThan(0);
  expect(second_answer[0].congestionLevel).toBe(first_answer[0].congestionLevel);

  const statistics_after_second_request = await page.evaluate(
    () => window.mapTest.trafficFlowCacheStatistics(),
  );
  expect(statistics_after_second_request.hitCount).toBe(1);
  expect(statistics_after_second_request.missCount).toBe(1);
});

test('a URL-template raster source registers through the contract call', async ({
  page,
}) => {
  const raster_descriptor = providers_registry.providers.find(
    (provider) => typeof provider.raster_url_template === 'string'
      && provider.raster_url_template.trim() !== '',
  );
  expect(raster_descriptor).toBeDefined();

  // A template without {z}/{x}/{y} is refused before anything changes.
  await expect(
    page.evaluate(() =>
      window.mapTest.registerUrlTemplateRasterTileSource({
        urlTemplate: 'https://tiles.example.invalid/static.png',
        sourceName: 'Broken source',
        minimumZoomLevel: 0,
        maximumZoomLevel: 19,
        imageFormat: 'png',
        tileResolution: 'standard',
        userAgent: '',
      }),
    ),
  ).rejects.toThrow(/placeholder/);

  await page.evaluate(
    ([url_template, source_name, maximum_zoom_level]) =>
      window.mapTest.registerUrlTemplateRasterTileSource({
        urlTemplate: url_template,
        sourceName: source_name,
        minimumZoomLevel: 0,
        maximumZoomLevel: maximum_zoom_level,
        imageFormat: 'png',
        tileResolution: 'standard',
      }),
    [
      raster_descriptor.raster_url_template,
      raster_descriptor.display_name,
      raster_descriptor.maximum_zoom_level,
    ],
  );
  // The source is a fallback, not a configured provider: the readback keeps
  // reporting the raster contract as unconfigured until a credential
  // configures one.
  const registrations = await read_provider_registrations(page);
  const raster_registration = registrations.find(
    (registration) => registration.contract === 'RasterTiles',
  );
  expect(raster_registration.registered).toBe(false);
  expect(raster_registration.displayName).toBe('');
  await page.evaluate(() => window.mapTest.clearUrlTemplateRasterTileSource());
});

// The URL-template source is the keyless fallback: it serves only while no
// provider is configured for the raster contract. These two cases pin the
// precedence in both directions with an intercepted tile response, so no test
// leaves the origin.
const FALLBACK_TILE_FIXTURE_PATH = path.join(
  REPOSITORY_ROOT,
  'crates',
  'rustic-maps-rendering',
  'fixtures',
  'render',
  'tiles',
  '14_2661_6286.png',
);

/** The fallback template's origin, derived from the registry (never typed). */
function fallback_tile_origin() {
  const raster_descriptor = providers_registry.providers.find(
    (provider) => typeof provider.raster_url_template === 'string'
      && provider.raster_url_template.trim() !== '',
  );
  const sample_tile_url = raster_descriptor.raster_url_template
    .replace('{z}', '14')
    .replace('{x}', '2661')
    .replace('{y}', '6286');
  return new URL(sample_tile_url).origin;
}

/** Registers the registry's URL-template raster source as the fallback. */
async function register_live_raster_fallback(page) {
  const raster_descriptor = providers_registry.providers.find(
    (provider) => typeof provider.raster_url_template === 'string'
      && provider.raster_url_template.trim() !== '',
  );
  await page.evaluate(
    ([url_template, source_name, maximum_zoom_level]) =>
      window.mapTest.registerUrlTemplateRasterTileSource({
        urlTemplate: url_template,
        sourceName: source_name,
        minimumZoomLevel: 0,
        maximumZoomLevel: maximum_zoom_level,
        imageFormat: 'png',
        tileResolution: 'standard',
      }),
    [
      raster_descriptor.raster_url_template,
      raster_descriptor.display_name,
      raster_descriptor.maximum_zoom_level,
    ],
  );
}

test('a keyless page fetches tiles through the URL-template fallback', async ({
  page,
}) => {
  const tile_bytes = readFileSync(FALLBACK_TILE_FIXTURE_PATH);
  const fallback_origin = fallback_tile_origin();
  const fallback_urls = [];
  page.on('request', (request) => {
    if (request.url().startsWith(`${fallback_origin}/`)) {
      fallback_urls.push(request.url());
    }
  });
  await page.route(`${fallback_origin}/**`, (route) => route.fulfill({
    status: 200,
    contentType: 'image/png',
    body: tile_bytes,
  }));
  await register_live_raster_fallback(page);
  const tile_length = await page.evaluate(
    () => window.mapTest.fetchRasterTile(14, 2661, 6286).then((bytes) => bytes.length),
  );
  expect(tile_length).toBe(tile_bytes.length);
  expect(fallback_urls.length).toBe(1);
  await page.evaluate(() => window.mapTest.clearUrlTemplateRasterTileSource());
});

test('a URL-template raster source applies its declared pacing', async ({
  page,
}) => {
  const raster_descriptor = providers_registry.providers.find(
    (provider) => typeof provider.raster_url_template === 'string'
      && provider.raster_url_template.trim() !== '',
  );
  const tile_bytes = readFileSync(FALLBACK_TILE_FIXTURE_PATH);
  const fallback_origin = fallback_tile_origin();
  const fallback_urls = [];
  page.on('request', (request) => {
    if (request.url().startsWith(`${fallback_origin}/`)) {
      fallback_urls.push(request.url());
    }
  });
  await page.route(`${fallback_origin}/**`, (route) => route.fulfill({
    status: 200,
    contentType: 'image/png',
    body: tile_bytes,
  }));
  await page.evaluate(
    ([url_template, source_name, maximum_zoom_level]) =>
      window.mapTest.registerUrlTemplateRasterTileSource({
        urlTemplate: url_template,
        sourceName: source_name,
        minimumZoomLevel: 0,
        maximumZoomLevel: maximum_zoom_level,
        imageFormat: 'png',
        tileResolution: 'standard',
        // One immediate request, then a refill far past the limiter's
        // maximum wait: the declared rate reaches the provider instead of
        // the default eight-request burst.
        tokenBucketCapacityTokens: 1,
        refillTokensPerSecond: 0.001,
      }),
    [
      raster_descriptor.raster_url_template,
      raster_descriptor.display_name,
      raster_descriptor.maximum_zoom_level,
    ],
  );
  const first_tile_length = await page.evaluate(
    () => window.mapTest.fetchRasterTile(14, 2661, 6286).then((bytes) => bytes.length),
  );
  expect(first_tile_length).toBe(tile_bytes.length);
  await expect(
    page.evaluate(() => window.mapTest.fetchRasterTile(14, 2661, 6286)),
  ).rejects.toThrow(/rate limit/i);
  expect(fallback_urls.length).toBe(1);
  await page.evaluate(() => window.mapTest.clearUrlTemplateRasterTileSource());
});

test('a configured raster provider outranks the URL-template fallback', async ({
  page,
}) => {
  const fallback_origin = fallback_tile_origin();
  const fallback_urls = [];
  page.on('request', (request) => {
    if (request.url().startsWith(`${fallback_origin}/`)) {
      fallback_urls.push(request.url());
    }
  });
  await page.route(`${fallback_origin}/**`, (route) => route.abort());
  await register_live_raster_fallback(page);
  await apply_test_provider_registration(page);
  const tile_length = await page.evaluate(
    () => window.mapTest.fetchRasterTile(14, 2661, 6286).then((bytes) => bytes.length),
  );
  expect(tile_length).toBeGreaterThan(0);
  expect(fallback_urls.length).toBe(0);
});

test('the browser transport drops a forbidden user-agent header', async ({
  page,
}) => {
  // The page records the header names the SDK hands to `fetch`. A declared
  // user agent must not appear: the browser transport filters forbidden
  // request headers before the request is built, so no cross-origin preflight
  // advertises one.
  const observed_header_names = await page.evaluate(async () => {
    const original_fetch = window.fetch;
    const seen_header_names = [];
    window.fetch = async (input, init) => {
      const request = input instanceof Request ? input : new Request(input, init);
      request.headers.forEach((_value, name) => seen_header_names.push(name.toLowerCase()));
      return new Response(new Uint8Array([137, 80, 78, 71]), {
        status: 200,
        headers: { 'content-type': 'image/png' },
      });
    };
    try {
      window.mapTest.registerUrlTemplateRasterTileSource({
        urlTemplate: 'https://tiles.example.invalid/{z}/{x}/{y}.png',
        sourceName: 'Header probe',
        minimumZoomLevel: 0,
        maximumZoomLevel: 19,
        imageFormat: 'png',
        tileResolution: 'standard',
        userAgent: 'RusticMapsTest/1.0',
      });
      await window.mapTest.fetchRasterTile(14, 2661, 6286);
    } finally {
      window.fetch = original_fetch;
      window.mapTest.clearUrlTemplateRasterTileSource();
    }
    return seen_header_names;
  });
  expect(observed_header_names).not.toContain('user-agent');
});

test('clearing registrations releases the traffic cache it owned', async ({
  page,
}) => {
  await apply_test_provider_registration(page);
  const flow_box = [40.6, -74.05, 40.65, -74.0];
  await page.evaluate((box) =>
    window.mapTest.fetchTrafficFlow(box[0], box[1], box[2], box[3]), flow_box);
  const statistics_before_clear = await page.evaluate(
    () => window.mapTest.trafficFlowCacheStatistics(),
  );
  expect(statistics_before_clear).not.toBeNull();

  await page.evaluate(() => window.mapTest.clearProviderRegistrations());
  const statistics_after_clear = await page.evaluate(
    () => window.mapTest.trafficFlowCacheStatistics(),
  );
  expect(statistics_after_clear).toBeNull();
});
