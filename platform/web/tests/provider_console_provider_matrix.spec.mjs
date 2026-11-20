// The provider matrix: every contract a registrable provider serves is called
// through the browser's generated contract registry, answered from that
// route's committed fixture, and shown in the console as a row prefixed with
// the provider's display name. Every provider fact — key, display name,
// credential requirement, served contracts, fixture route — comes from the
// shared registry, so the suite names no vendor and touches no network.
//
// The expected contract set is the descriptor's own `served_contracts`; each
// fixture route names the contract it answers in the registry's `contract`
// field. A served contract the browser cannot exercise is declared here as a
// skipped case whose title names the contract and states the reason, so a gap
// is reported rather than silently dropped.
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
 * The tile a raster fixture route is requested with when its pattern declares
 * no literal address: the demo camera's Sacramento tile at zoom 14.
 */
const DEFAULT_RASTER_TILE = { zoomLevel: 14, tileX: 2661, tileY: 6286 };
/** The two origins and one destination the committed matrix fixture records. */
const DISTANCE_MATRIX_ORIGINS = [
  38.57693, -121.49492, 38.578739, -121.507447,
];
const DISTANCE_MATRIX_DESTINATIONS = [38.5816, -121.4944];
/** The traffic boxes the committed incident and flow fixtures record. */
const TRAFFIC_INCIDENTS_BOX = [39.7, -105.1, 39.9, -104.8];
const TRAFFIC_FLOW_BOX = [40.6, -74.05, 40.65, -74.0];
/** The flush cap the provider console's own ring reads; larger than its bound. */
const DIAGNOSTIC_FLUSH_LIMIT = 4096;

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

/**
 * The tile address a raster fixture route is requested with: the literal
 * `{z}/{x}/{y}.png` address its pattern pins, or the demo tile when the
 * pattern captures the segments or declares none.
 */
function raster_tile_arguments_for(path_pattern) {
  if (path_pattern.includes('(?<tile_zoom_level>')) {
    return { ...DEFAULT_RASTER_TILE };
  }
  const literal_address = /(\d+)\/(\d+)\/(\d+)\\\.png\$/.exec(path_pattern);
  if (literal_address === null) {
    return { ...DEFAULT_RASTER_TILE };
  }
  return {
    zoomLevel: Number(literal_address[1]),
    tileX: Number(literal_address[2]),
    tileY: Number(literal_address[3]),
  };
}

/** The parsed result every geocoding and place-search call must answer. */
function assert_place_rows(rows) {
  expect(Array.isArray(rows)).toBe(true);
  expect(rows.length).toBeGreaterThan(0);
  expect(typeof rows[0].structuredAddressText).toBe('string');
  expect(typeof rows[0].latitudeDegrees).toBe('number');
  expect(typeof rows[0].longitudeDegrees).toBe('number');
}

/** The parsed result an everymap route answer must carry. */
function assert_route(route) {
  expect(route).not.toBeNull();
  expect(Array.isArray(route.legs)).toBe(true);
  expect(route.legs.length).toBeGreaterThan(0);
  expect(typeof route.distanceMeters).toBe('number');
  expect(typeof route.durationSeconds).toBe('number');
}

/** The parsed result a distance-matrix answer must carry. */
function assert_distance_matrix(entries) {
  expect(Array.isArray(entries)).toBe(true);
  expect(entries.length).toBeGreaterThan(0);
  expect(typeof entries[0].originIndex).toBe('number');
  expect(typeof entries[0].destinationIndex).toBe('number');
  expect(typeof entries[0].distanceMeters).toBe('number');
  expect(typeof entries[0].durationSeconds).toBe('number');
}

/** Every string `id` the committed payload carries, at any depth. */
function fixture_identifiers(fixture_value) {
  const identifiers = new Set();
  const visit = (value) => {
    if (Array.isArray(value)) {
      for (const entry of value) {
        visit(entry);
      }
      return;
    }
    if (value === null || typeof value !== 'object') {
      return;
    }
    if (typeof value.id === 'string') {
      identifiers.add(value.id);
    }
    for (const child of Object.values(value)) {
      visit(child);
    }
  };
  visit(fixture_value);
  return identifiers;
}

/**
 * The parsed incidents must be a subset of the committed payload's own
 * records. The contract refuses an incident without a usable position, so a
 * fixture whose entries carry none parses to an empty list; every identifier
 * the browser does answer must still come from that fixture.
 */
function assert_traffic_incidents(incidents, fixture_bytes) {
  expect(Array.isArray(incidents)).toBe(true);
  const fixture_identifiers_set = fixture_identifiers(
    JSON.parse(fixture_bytes.toString('utf8')),
  );
  expect(fixture_identifiers_set.size).toBeGreaterThan(0);
  for (const incident of incidents) {
    expect(typeof incident.identifier).toBe('string');
    expect(fixture_identifiers_set.has(incident.identifier)).toBe(true);
    expect(typeof incident.incidentType).toBe('string');
    expect(typeof incident.severity).toBe('string');
    expect(typeof incident.startCoordinate).toBe('object');
  }
}

/** The parsed result a traffic-flow answer must carry. */
function assert_traffic_flow(flow_rows) {
  expect(Array.isArray(flow_rows)).toBe(true);
  expect(flow_rows.length).toBeGreaterThan(0);
  expect(typeof flow_rows[0].congestionLevel).toBe('string');
  expect(Array.isArray(flow_rows[0].shapeCoordinates)).toBe(true);
}

/** The fetched raster tile must be the committed fixture's exact bytes. */
function assert_raster_tile(tile_bytes, fixture_bytes) {
  expect(tile_bytes instanceof Uint8Array).toBe(true);
  expect(tile_bytes.byteLength).toBe(fixture_bytes.length);
  expect(Buffer.from(tile_bytes).equals(fixture_bytes)).toBe(true);
}

/**
 * The browser request each route purpose stands for. The route's registry
 * `contract` decides which served contract it covers; the purpose only picks
 * the browser call that reaches it, and `REQUEST_CONTRACTS` pins the two so a
 * registry drift is refused rather than silently mis-grouped.
 */
const ROUTE_REQUESTS = {
  geocoding: {
    reportedContract: 'GeocodingProvider',
    invoke: (page) => page.evaluate(() =>
      window.mapTest.forwardGeocode('Denver')),
    assertParsedResult: assert_place_rows,
  },
  reverse_geocoding: {
    reportedContract: 'GeocodingProvider',
    invoke: (page) => page.evaluate(() =>
      window.mapTest.reverseGeocode(38.57693, -121.49492)),
    assertParsedResult: assert_place_rows,
  },
  search: {
    reportedContract: 'SearchPlaceProvider',
    invoke: (page) => page.evaluate(() => window.mapTest.searchPlaces('Denver')),
    assertParsedResult: assert_place_rows,
  },
  routing: {
    reportedContract: 'RoutingProvider',
    argumentsFor: (path_pattern) => route_coordinates_for(path_pattern),
    invoke: (page, coordinates) => page.evaluate((route) =>
      window.mapTest.calculateRoute(
        route.originLatitude,
        route.originLongitude,
        route.destinationLatitude,
        route.destinationLongitude,
      ), coordinates),
    assertParsedResult: assert_route,
  },
  directions: {
    reportedContract: 'RoutingProvider',
    argumentsFor: (path_pattern) => route_coordinates_for(path_pattern),
    invoke: (page, coordinates) => page.evaluate((route) =>
      window.mapTest.calculateRoute(
        route.originLatitude,
        route.originLongitude,
        route.destinationLatitude,
        route.destinationLongitude,
      ), coordinates),
    assertParsedResult: assert_route,
  },
  distance_matrix: {
    reportedContract: 'DistanceMatrixProvider',
    argumentsFor: () => ({
      origins: DISTANCE_MATRIX_ORIGINS,
      destinations: DISTANCE_MATRIX_DESTINATIONS,
    }),
    invoke: (page, arguments_value) => page.evaluate((matrix) =>
      window.mapTest.calculateDistanceMatrix(
        matrix.origins,
        matrix.destinations,
        'Automobile',
      ), arguments_value),
    assertParsedResult: assert_distance_matrix,
  },
  traffic_incidents: {
    reportedContract: 'TrafficIncidentProvider',
    argumentsFor: () => TRAFFIC_INCIDENTS_BOX,
    invoke: (page, box) => page.evaluate((corners) =>
      window.mapTest.fetchTrafficIncidents(
        corners[0], corners[1], corners[2], corners[3],
      ), box),
    assertParsedResult: assert_traffic_incidents,
  },
  traffic_flow: {
    reportedContract: 'TrafficFlowProvider',
    argumentsFor: () => TRAFFIC_FLOW_BOX,
    invoke: (page, box) => page.evaluate((corners) =>
      window.mapTest.fetchTrafficFlow(
        corners[0], corners[1], corners[2], corners[3],
      ), box),
    assertParsedResult: assert_traffic_flow,
  },
  raster_tiles: {
    reportedContract: 'RasterTileProvider',
    argumentsFor: (path_pattern) => raster_tile_arguments_for(path_pattern),
    invoke: (page, tile) => page.evaluate((address) =>
      window.mapTest.fetchRasterTile(
        address.zoomLevel,
        address.tileX,
        address.tileY,
      ), tile),
    assertParsedResult: assert_raster_tile,
  },
};

/** The contract each route purpose must answer, checked against the registry. */
const REQUEST_CONTRACTS = {
  geocoding: 'geocoding',
  reverse_geocoding: 'geocoding',
  search: 'place_search',
  routing: 'routing',
  directions: 'routing',
  distance_matrix: 'distance_matrix',
  traffic_incidents: 'traffic_incidents',
  traffic_flow: 'traffic_flow',
  raster_tiles: 'raster_tiles',
};

/** The registry path of the committed payload a route's request serves. */
function committed_fixture_path(fixture_route, arguments_value) {
  if (typeof fixture_route.fixture === 'string') {
    return fixture_route.fixture;
  }
  if (
    fixture_route.fixture_kind === 'tile_directory'
    && typeof fixture_route.fixture_directory === 'string'
    && typeof fixture_route.image_format === 'string'
  ) {
    const tile = arguments_value;
    return path.join(
      fixture_route.fixture_directory,
      `${tile.zoomLevel}_${tile.tileX}_${tile.tileY}.${fixture_route.image_format}`,
    );
  }
  return null;
}

/** Whether the browser can register the descriptor's provider. */
function is_registrable_in_browser(descriptor) {
  return descriptor.registration_available?.wasm === true
    && typeof descriptor.wasm_registration === 'string';
}

const REGISTRABLE_DESCRIPTORS = PROVIDERS_REGISTRY.providers.filter(
  is_registrable_in_browser,
);

/**
 * One case per supported fixture route, grouped by the route's own registry
 * `contract`. A served contract with a route the browser cannot request, or
 * with no route at all, becomes one explicit skipped case naming the
 * contract.
 */
const PROVIDER_ROUTE_CASES = [];
const PROVIDER_CONTRACT_GAPS = [];
for (const descriptor of REGISTRABLE_DESCRIPTORS) {
  for (const contract of descriptor.served_contracts) {
    const contract_routes = (descriptor.fixture_routes ?? []).filter(
      (fixture_route) => fixture_route.contract === contract,
    );
    if (contract_routes.length === 0) {
      const fixtureless_record = (descriptor.fixtureless_contracts ?? []).find(
        (fixtureless_contract) => fixtureless_contract.contract === contract,
      );
      PROVIDER_CONTRACT_GAPS.push({
        descriptor,
        contract,
        reason: fixtureless_record === undefined
          ? `the registry declares no fixture route for contract ${contract}`
          : `contract ${contract} is fixtureless: ${fixtureless_record.reason}`,
      });
      continue;
    }

    const supported_routes = [];
    for (const [route_ordinal, fixture_route] of contract_routes.entries()) {
      const request = ROUTE_REQUESTS[fixture_route.purpose];
      if (request === undefined) {
        continue;
      }
      if (REQUEST_CONTRACTS[fixture_route.purpose] !== fixture_route.contract) {
        throw new Error(
          `the registry route "${fixture_route.purpose}" on `
            + `"${descriptor.provider_key}" names contract `
            + `"${fixture_route.contract}", but its purpose answers `
            + `"${REQUEST_CONTRACTS[fixture_route.purpose]}"`,
        );
      }
      const arguments_value = request.argumentsFor === undefined
        ? undefined
        : request.argumentsFor(fixture_route.path_pattern);
      const fixture_path = committed_fixture_path(fixture_route, arguments_value);
      if (fixture_path === null) {
        throw new Error(
          `the registry route "${fixture_route.purpose}" on `
            + `"${descriptor.provider_key}" names no committed fixture`,
        );
      }
      const literal_path = literal_request_path(fixture_route.path_pattern);
      supported_routes.push({
        descriptor,
        contract,
        fixture_route,
        request,
        arguments: arguments_value,
        fixturePath: fixture_path,
        // Two routes can share a purpose (a literal example and a captured
        // pattern), and two purposes can share a literal path (forward and
        // reverse geocoding), so the title names both.
        routeLabel: `${fixture_route.purpose} `
          + (literal_path ?? `route ${route_ordinal + 1}`),
      });
    }

    if (supported_routes.length === 0) {
      const purposes = [
        ...new Set(contract_routes.map((fixture_route) => fixture_route.purpose)),
      ];
      PROVIDER_CONTRACT_GAPS.push({
        descriptor,
        contract,
        reason: `the browser matrix has no request for contract ${contract}'s `
          + `route purposes (${purposes.join(', ')})`,
      });
      continue;
    }
    PROVIDER_ROUTE_CASES.push(...supported_routes);
  }
}

// A provider the browser cannot register is an explicit gap per served
// contract, not a silent omission from the matrix.
for (const descriptor of PROVIDERS_REGISTRY.providers) {
  if (is_registrable_in_browser(descriptor)) {
    continue;
  }
  for (const contract of descriptor.served_contracts) {
    PROVIDER_CONTRACT_GAPS.push({
      descriptor,
      contract,
      reason: `provider ${descriptor.provider_key} is not registrable in the `
        + 'browser (registration_available.wasm is false)',
    });
  }
}

// Every served contract of every provider is either exercised or reported.
for (const descriptor of PROVIDERS_REGISTRY.providers) {
  for (const contract of descriptor.served_contracts) {
    const has_case = PROVIDER_ROUTE_CASES.some(
      (provider_case) =>
        provider_case.descriptor === descriptor
        && provider_case.contract === contract,
    );
    const has_gap = PROVIDER_CONTRACT_GAPS.some(
      (provider_gap) =>
        provider_gap.descriptor === descriptor
        && provider_gap.contract === contract,
    );
    if (!has_case && !has_gap) {
      throw new Error(
        `the matrix silently drops contract "${contract}" of provider `
          + `"${descriptor.provider_key}"`,
      );
    }
  }
}

if (PROVIDER_ROUTE_CASES.length < 2) {
  throw new Error(
    'the registry declares fewer than two supported provider routes to exercise',
  );
}

/**
 * The provider-relative path an observed request went to, with the fixture
 * server's base path, provider key and redacted query removed.
 */
function provider_relative_request_path(request_path, fixture_base_path, provider_key) {
  const prefix = `${fixture_base_path}/${provider_key}/`;
  if (!request_path.startsWith(prefix)) {
    throw new Error(
      `the observed request path ${request_path} is not under ${prefix}`,
    );
  }
  return request_path.split('?')[0].slice(prefix.length);
}

/**
 * The first registry route the fixture server would serve for one request:
 * the same method-then-pattern order `platform/web/serve.mjs` applies.
 */
function serving_fixture_route(descriptor, http_method, relative_path) {
  return (descriptor.fixture_routes ?? []).find(
    (fixture_route) =>
      (fixture_route.method ?? 'GET') === http_method
      && new RegExp(fixture_route.path_pattern).test(relative_path),
  ) ?? null;
}

/** The latest provider-request observation for one provider and contract. */
async function latest_provider_request(page, provider_key, provider_contract) {
  return page.evaluate(
    ([target_provider_key, target_provider_contract, flush_limit]) => {
      const events = Array.from(window.mapTest.flushRecentDiagnosticEvents(
        window.mapTest.SdkLogSeverity.Info,
        flush_limit,
      ));
      for (let index = events.length - 1; index >= 0; index -= 1) {
        const request = events[index].providerRequest;
        if (
          request?.providerKey === target_provider_key
          && request?.providerContract === target_provider_contract
        ) {
          return request;
        }
      }
      return null;
    },
    [provider_key, provider_contract, DIAGNOSTIC_FLUSH_LIMIT],
  );
}

test.beforeEach(async ({ page }) => {
  await page.goto('/?advanced');
  await waitForMapTestReady(page);
  await page.locator('#provider-console-control').click();
});

for (const provider_case of PROVIDER_ROUTE_CASES) {
  const {
    descriptor,
    contract,
    fixture_route,
    request,
    arguments: invocation_arguments,
    fixturePath,
    routeLabel,
  } = provider_case;

  test(`${descriptor.provider_key} answers ${contract} through its ${routeLabel} fixture route`, async ({
    page,
  }) => {
    await page.evaluate(
      ([provider_key, fixture_base_path, request_contract, credential]) => {
        window.mapTest.setProviderBaseUrl(
          provider_key,
          `${window.location.origin}${fixture_base_path}/${provider_key}`,
        );
        window.mapTest.configureProvider(
          request_contract,
          provider_key,
          credential,
        );
      },
      [
        descriptor.provider_key,
        PROVIDERS_REGISTRY.fixture_base_path,
        contract,
        descriptor.credential_label === '' ? '' : MATRIX_CREDENTIAL,
      ],
    );

    // The provider's own answer is the committed fixture: hold the response
    // this route's request makes and compare its body with those exact bytes
    // below. A contract call can issue more than one request (the incident
    // contract also reads flow), so the predicate names this route's pattern.
    const provider_base_path =
      `${PROVIDERS_REGISTRY.fixture_base_path}/${descriptor.provider_key}/`;
    const route_pattern = new RegExp(fixture_route.path_pattern);
    const provider_response_promise = page.waitForResponse((response) => {
      const response_path = new URL(response.url()).pathname;
      if (!response_path.startsWith(provider_base_path)) {
        return false;
      }
      return route_pattern.test(response_path.slice(provider_base_path.length))
        && response.request().method() === (fixture_route.method ?? 'GET')
        && response.status() === 200;
    }).catch(() => null);

    let parsed_result;
    let invocation_error = null;
    try {
      parsed_result = await request.invoke(page, invocation_arguments);
    } catch (error) {
      invocation_error = error;
    }

    const observation = await latest_provider_request(
      page,
      descriptor.provider_key,
      request.reportedContract,
    );
    expect(observation).not.toBeNull();
    const relative_path = provider_relative_request_path(
      observation.requestPath,
      PROVIDERS_REGISTRY.fixture_base_path,
      descriptor.provider_key,
    );
    const served_by = serving_fixture_route(
      descriptor,
      observation.httpMethod,
      relative_path,
    );
    if (served_by !== fixture_route) {
      // The browser reached a different route, or no route at all, so this
      // route's fixture is not the one the server would answer. Report the
      // gap, naming the contract, instead of asserting the wrong payload.
      test.skip(
        true,
        `the fixture server cannot reach contract ${contract} through the `
          + `${fixture_route.purpose} route: `
          + (served_by === null
            ? `no registry route matches `
              + `${observation.httpMethod} ${relative_path}`
            : `${served_by.purpose} serves ${relative_path} first`),
      );
      return;
    }
    if (invocation_error !== null) {
      throw new Error(
        `${descriptor.provider_key} (${contract} through `
          + `${fixture_route.purpose}) failed: ${invocation_error.message}`,
      );
    }

    // The traffic contract answers from flow and incidents in one call, so
    // the row is narrowed to this route's path when its pattern is literal.
    const requestPath = literal_request_path(fixture_route.path_pattern);
    const provider_rows = page
      .locator('#provider-console-rows li')
      .filter({ hasText: descriptor.display_name })
      .filter({ hasText: request.reportedContract });
    const matching_rows = requestPath === null
      ? provider_rows
      : provider_rows.filter({ hasText: requestPath });
    await expect(matching_rows.first()).toContainText(
      request.reportedContract,
    );
    if (requestPath !== null) {
      await expect(matching_rows.first()).toContainText(requestPath);
    }
    await expect(matching_rows.first()).toContainText('2xx');
    await expect(matching_rows.first()).not.toContainText(MATRIX_CREDENTIAL);
    // The observed request reached the route whose fixture it claimed, and
    // the committed payload is the bytes the browser answered from.
    expect(relative_path).toMatch(new RegExp(fixture_route.path_pattern));
    const provider_response = await provider_response_promise;
    expect(provider_response).not.toBeNull();
    const fixture_bytes = readFileSync(
      path.join(REPOSITORY_ROOT, fixturePath),
    );
    expect(Buffer.from(await provider_response.body()).equals(fixture_bytes))
      .toBe(true);
    request.assertParsedResult(parsed_result, fixture_bytes);
  });
}

for (const provider_gap of PROVIDER_CONTRACT_GAPS) {
  const { descriptor, contract, reason } = provider_gap;
  test.skip(
    `${descriptor.provider_key} cannot exercise ${contract} in the browser: ${reason}`,
    () => {},
  );
}
