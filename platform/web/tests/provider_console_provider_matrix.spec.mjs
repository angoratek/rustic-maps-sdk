// The provider matrix: every provider this build can register in the browser
// answers one capability against its recorded fixture, and the console shows a
// row prefixed with that provider's display name. Every provider fact — key,
// display name, credential requirement, fixture route — comes from the shared
// registry, so the suite names no vendor and touches no network.
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

/** A unique credential, so the redaction assertion cannot pass by accident. */
const MATRIX_CREDENTIAL = 'matrix-probe-secret-value';
/** The route the recorded demo pairs use when a pattern names no coordinates. */
const DEFAULT_ROUTE_COORDINATES = {
  originLatitude: 38.57693,
  originLongitude: -121.49492,
  destinationLatitude: 38.578739,
  destinationLongitude: -121.507447,
};

/**
 * The route inputs a fixture route needs: its pattern may pin the exact
 * `longitude,latitude;longitude,latitude` pair the fixture was recorded for.
 */
function route_coordinates_for(path_pattern) {
  const coordinate_pattern =
    /(-?\d+(?:\\?\.\d+)?),(-?\d+(?:\\?\.\d+)?);(-?\d+(?:\\?\.\d+)?),(-?\d+(?:\\?\.\d+)?)/;
  const match = path_pattern.match(coordinate_pattern);
  if (match === null) {
    return DEFAULT_ROUTE_COORDINATES;
  }
  const coordinates = match
    .slice(1)
    .map((value) => Number(value.replaceAll('\\.', '.')));
  return {
    originLatitude: coordinates[1],
    originLongitude: coordinates[0],
    destinationLatitude: coordinates[3],
    destinationLongitude: coordinates[2],
  };
}

/** The literal request path a route pattern names, or `null` when it has no literal form. */
function literal_request_path(path_pattern) {
  const body = path_pattern.replace(/^\^|\$$/g, '').replaceAll('\\.', '.');
  return /^[A-Za-z0-9\-._~/;,]+$/.test(body) ? `/${body}` : null;
}

/** The request each fixture purpose stands for. */
const PURPOSE_REQUESTS = {
  geocoding: {
    contract: 'geocoding',
    reportedContract: 'GeocodingProvider',
    invoke: (page) => page.evaluate(() =>
      window.mapTest.forwardGeocode('Denver')),
  },
  reverse_geocoding: {
    contract: 'geocoding',
    reportedContract: 'GeocodingProvider',
    invoke: (page) => page.evaluate(() =>
      window.mapTest.reverseGeocode(38.57693, -121.49492)),
  },
  search: {
    contract: 'place_search',
    reportedContract: 'SearchPlaceProvider',
    invoke: (page) => page.evaluate(() => window.mapTest.searchPlaces('Denver')),
  },
  routing: {
    contract: 'routing',
    reportedContract: 'RoutingProvider',
    invoke: (page, coordinates) => page.evaluate((route) =>
      window.mapTest.calculateRoute(
        route.originLatitude,
        route.originLongitude,
        route.destinationLatitude,
        route.destinationLongitude,
      ), coordinates),
  },
  directions: {
    contract: 'routing',
    reportedContract: 'RoutingProvider',
    invoke: (page, coordinates) => page.evaluate((route) =>
      window.mapTest.calculateRoute(
        route.originLatitude,
        route.originLongitude,
        route.destinationLatitude,
        route.destinationLongitude,
      ), coordinates),
  },
};

/**
 * One row per registrable provider: the provider's first fixture route whose
 * purpose the suite knows how to request, the literal path that route declares
 * (when it has one), and the route inputs its pattern needs.
 *
 * Keyless providers join the matrix too: every client the SDK builds,
 * credentialed or keyless, reports its attempts to the same observer, so a
 * keyless request reaches a console row like any other.
 */
const PROVIDER_CASES = PROVIDERS_REGISTRY.providers
  .filter((descriptor) => descriptor.registration_available?.wasm === true)
  .map((descriptor) => {
    const fixture_route = descriptor.fixture_routes.find(
      (candidate) => PURPOSE_REQUESTS[candidate.purpose] !== undefined,
    );
    if (fixture_route === undefined) {
      throw new Error(
        `the registry declares no supported fixture route for ${descriptor.provider_key}`,
      );
    }
    return {
      descriptor,
      request: PURPOSE_REQUESTS[fixture_route.purpose],
      requestPath: literal_request_path(fixture_route.path_pattern),
      coordinates: route_coordinates_for(fixture_route.path_pattern),
    };
  });

if (PROVIDER_CASES.length < 2) {
  throw new Error(
    'the registry declares fewer than two registrable providers to exercise',
  );
}

test.beforeEach(async ({ page }) => {
  await page.goto('/?advanced');
  await waitForMapTestReady(page);
  await page.locator('#provider-console-control').click();
});

test('every registrable provider answers a capability and names itself in a console row', async ({
  page,
}) => {
  for (const provider_case of PROVIDER_CASES) {
    const { descriptor, request } = provider_case;
    await page.evaluate(
      ([provider_key, fixture_base_path, contract, credential]) => {
        window.mapTest.setProviderBaseUrl(
          provider_key,
          `${window.location.origin}${fixture_base_path}/${provider_key}`,
        );
        window.mapTest.configureProvider(contract, provider_key, credential);
      },
      [
        descriptor.provider_key,
        PROVIDERS_REGISTRY.fixture_base_path,
        request.contract,
        descriptor.credential_label === '' ? '' : MATRIX_CREDENTIAL,
      ],
    );
    try {
      await request.invoke(page, provider_case.coordinates);
    } catch (invocation_error) {
      throw new Error(
        `${descriptor.provider_key} (${request.contract}) failed: ${invocation_error.message}`,
      );
    }

    const provider_rows = page
      .locator('#provider-console-rows li')
      .filter({ hasText: descriptor.display_name });
    await expect(provider_rows.first()).toContainText(
      request.reportedContract,
    );
    if (provider_case.requestPath !== null) {
      await expect(provider_rows.first()).toContainText(
        provider_case.requestPath,
      );
    }
    await expect(provider_rows.first()).toContainText('2xx');
    await expect(provider_rows.first()).not.toContainText(MATRIX_CREDENTIAL);
  }
});
