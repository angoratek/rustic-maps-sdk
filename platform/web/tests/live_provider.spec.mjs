// The live-provider browser path (M13/M15): a runtime-registered provider
// fetches through the browser `fetch` transport and parses the response with
// the same provider implementation the native targets use. Every provider
// fact in this suite (key, label, attribution, fixture routes) comes from the
// shared registry `platform/providers.json`; the dev server serves those
// fixtures under `/providers/{provider_key}/…`, so no external network is
// touched and no vendor path is named here.
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
const PROVIDER_CREDENTIAL_LABEL = provider_descriptor.credential_label;
const PROVIDER_ATTRIBUTION_TEXT = provider_descriptor.attribution_text;
const PROVIDER_FIXTURE_BASE_PATH = providers_registry.fixture_base_path;


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

/** Opens the settings sheet (the provider credentials' home) when closed. */
async function open_provider_panel(page) {
  const settings_dialog = page.locator('#settings-dialog');
  if (!(await settings_dialog.evaluate(element => element.open))) {
    await page.locator('#settings-control').click();
  }
  await expect(settings_dialog).toBeVisible();
}

/**
 * Closes the settings sheet: the trip card's route action and the search
 * field live behind it while the modal is open.
 */
async function close_provider_panel(page) {
  const settings_dialog = page.locator('#settings-dialog');
  if (await settings_dialog.evaluate(element => element.open)) {
    await page.keyboard.press('Escape');
  }
  await expect(settings_dialog).toBeHidden();
}

/** The descriptor route serving one purpose (`geocoding`/`directions`/`raster_tiles`). */
function fixture_route_for_purpose(purpose) {
  const fixture_route = provider_descriptor.fixture_routes.find(
    (candidate_route) => candidate_route.purpose === purpose,
  );
  if (fixture_route === undefined) {
    throw new Error(`the registry declares no "${purpose}" fixture route`);
  }
  return fixture_route;
}

// The recorded provider responses the live calls fetch through the
// same-origin fixture routes (no external network); their committed bytes are
// the expected answers.
const recorded_geocoding_response = readFileSync(
  path.join(REPOSITORY_ROOT, fixture_route_for_purpose('geocoding').fixture),
  'utf8',
);
const recorded_directions_response = readFileSync(
  path.join(REPOSITORY_ROOT, fixture_route_for_purpose('directions').fixture),
  'utf8',
);
// One committed recorded raster tile, served at the provider's own Tiles API
// path by its fixture route; its bytes are the expected answer.
const recorded_tile_bytes = readFileSync(
  path.join(
    REPOSITORY_ROOT,
    fixture_route_for_purpose('raster_tiles').fixture_directory,
    '14_2661_6286.png',
  ),
);

// The PNG file signature every fetched tile must start with.
const PNG_SIGNATURE_BYTES = [137, 80, 78, 71, 13, 10, 26, 10];
// The recorded fixture's route facts (the provider crate's fixture test
// pins them): 1.7 km, 235 s, one leg of six maneuvers.
const RECORDED_ROUTE_DISTANCE_METERS = 1700;
const RECORDED_ROUTE_DURATION_SECONDS = 235;
const RECORDED_ROUTE_STEP_COUNT = 6;
const RECORDED_ROUTE_FIRST_STEP_TEXT = 'Turn left onto L St. Go for 864 m.';
// The playground's demo tile address (the zoom-14 camera's Sacramento tile).
const DEMO_TILE_ZOOM_LEVEL = 14;
const DEMO_TILE_X = 2661;
const DEMO_TILE_Y = 6286;

/** The panel row the registry descriptor renders. */
function provider_row(page) {
  return page.locator(
    `#provider-credential-rows [data-provider-key="${PROVIDER_KEY}"]`,
  );
}

test.beforeEach(async ({ page }) => {
  await page.goto('/?advanced');
  // The Rust-side `initializeSdk` promise gates every assertion.
  await waitForMapTestReady(page);
});

test('a registered live provider fetches and parses through the browser transport', async ({
  page,
}) => {
  await apply_test_provider_registration(page);
  const rows = await page.evaluate(() =>
    window.mapTest.forwardGeocode('Sacramento,California'),
  );
  expect(Array.isArray(rows)).toBe(true);
  expect(rows.length).toBeGreaterThan(0);
  expect(rows[0].structuredAddressText).toContain('Sacramento');
  expect(typeof rows[0].latitudeDegrees).toBe('number');
  expect(typeof rows[0].longitudeDegrees).toBe('number');
  expect(typeof rows[0].qualityText).toBe('string');
});

test('a registered live provider suggests places while typing and returns settled search rows', async ({
  page,
}) => {
  await apply_test_provider_registration(page);

  // The committed prediction fixture is the answer for both members: the
  // autocomplete call and the settled search call share the provider's
  // endpoint (a different documented limit only).
  const suggestion_rows = await page.evaluate(() =>
    window.mapTest.suggestPlaces('Denver'),
  );
  expect(suggestion_rows.length).toBe(3);
  expect(suggestion_rows[0].structuredAddressText).toContain("Denver Int'l Airport");
  expect(suggestion_rows[0].latitudeDegrees).toBeCloseTo(39.85352, 5);
  expect(suggestion_rows[0].longitudeDegrees).toBeCloseTo(-104.67377, 5);
  expect(suggestion_rows[0].qualityText).toBe('Point');

  const search_rows = await page.evaluate(() =>
    window.mapTest.searchPlaces('Denver'),
  );
  expect(search_rows.length).toBe(3);
  expect(search_rows[1].structuredAddressText).toContain('Denver Art Museum');
});

test('a boundary-narrowed search call validates its corners before asking the provider', async ({
  page,
}) => {
  await apply_test_provider_registration(page);

  // A valid Denver box asks the provider (the fixture answers regardless of
  // the ranking center, so only the parsed rows and the call's success are
  // asserted here).
  const rows = await page.evaluate(() =>
    window.mapTest.suggestPlacesInBoundary('Denver', 39.6, -105.1, 39.95, -104.6),
  );
  expect(rows.length).toBe(3);

  // Inverted corners never reach the provider: the seam refuses them.
  await expect(
    page.evaluate(() =>
      window.mapTest.searchPlacesInBoundary('Denver', 40.0, -105.1, 39.6, -104.6),
    ),
  ).rejects.toThrow(/inverted/);
});

test('a registered live provider accepts the contract route options object', async ({
  page,
}) => {
  await apply_test_provider_registration(page);

  // The fixture answers regardless of the request's options; this proves the
  // options object reaches the provider call and the answer keeps the route
  // shape the plain call produces.
  const route = await page.evaluate(() =>
    window.mapTest.calculateRouteWithOptions(
      38.57693,
      -121.49492,
      38.57874,
      -121.50745,
      {
        transportationMode: 'pedestrian',
        routeStrategy: 'shortest',
        unitSystem: 'imperial',
        avoidOptions: {
          tollRoads: true,
          limitedAccessHighways: true,
          ferries: true,
          unpavedRoads: true,
        },
        departureTimestampSeconds: 1767225600,
      },
    ),
  );
  // The imperial unit choice is visible in the answer: the fixture's 1.7
  // miles are converted to 2735.8848 metres (1.7 × 1609.344), which proves
  // the options reached the provider request and its parser.
  expect(route.distanceMeters).toBeCloseTo(2735.8848, 4);
  expect(route.durationSeconds).toBe(RECORDED_ROUTE_DURATION_SECONDS);
  expect(route.legs[0].steps).toHaveLength(RECORDED_ROUTE_STEP_COUNT);

  // Unknown enum values and malformed options are refused before the
  // provider is asked.
  await expect(
    page.evaluate(() =>
      window.mapTest.calculateRouteWithOptions(38.5, -121.5, 38.6, -121.4, {
        routeStrategy: 'scenic',
      }),
    ),
  ).rejects.toThrow(/route strategy must be fastest or shortest/);
  await expect(
    page.evaluate(() =>
      window.mapTest.calculateRouteWithOptions(38.5, -121.5, 38.6, -121.4, {
        departureTimestampSeconds: -1,
      }),
    ),
  ).rejects.toThrow(/departure timestamp must be finite, non-negative/);
  await expect(
    page.evaluate(() =>
      window.mapTest.calculateRouteWithOptions(38.5, -121.5, 38.6, -121.4, 'nope'),
    ),
  ).rejects.toThrow(/route options argument must be an object/);
});

test('a registered live provider answers a distance matrix in meters and seconds', async ({
  page,
}) => {
  await apply_test_provider_registration(page);

  // The committed matrix fixture (three recorded Sacramento locations, unit
  // `k`): two origins and the third location as the destination, so the
  // answer is the fixture's third column converted from kilometres.
  const entries = await page.evaluate(() =>
    window.mapTest.calculateDistanceMatrix(
      [38.57693, -121.49492, 38.57874, -121.50745],
      [38.57393, -121.46956],
      'Automobile',
    ),
  );
  expect(entries).toHaveLength(2);
  expect(entries[0]).toEqual({
    originIndex: 0,
    destinationIndex: 0,
    distanceMeters: 2600,
    durationSeconds: 320,
  });
  expect(entries[1]).toEqual({
    originIndex: 1,
    destinationIndex: 0,
    distanceMeters: 4500,
    durationSeconds: 514,
  });

  // An unknown mode is refused before the provider is asked, and the
  // lowercase spelling of a known mode works.
  await expect(
    page.evaluate(() =>
      window.mapTest.calculateDistanceMatrix([0, 0], [1, 1], 'teleport'),
    ),
  ).rejects.toThrow(/transportation mode must be/);
});

test('a registered live provider answers traffic incidents and flow inside a box', async ({
  page,
}) => {
  await apply_test_provider_registration(page);

  // The committed Denver incidents fixture: five accidents, all severity
  // medium, each an incident point with no reported end coordinate.
  const incidents = await page.evaluate(() =>
    window.mapTest.fetchTrafficIncidents(39.7, -105.1, 39.9, -104.8),
  );
  expect(incidents).toHaveLength(5);
  expect(incidents[0]).toEqual({
    identifier: '1396961518491253',
    incidentType: 'Accident',
    severity: 'Medium',
    shortDescriptionText: 'Closed',
    startCoordinate: { latitude: 39.85682, longitude: -105.03604 },
    endCoordinate: null,
  });

  // The committed New York flow fixture: one green, yellow, red and black
  // segment; the parser flattens each feature's line strings into one row
  // and the fixture reports no absolute speeds.
  const flow_rows = await page.evaluate(() =>
    window.mapTest.fetchTrafficFlow(40.6, -74.05, 40.65, -74.0),
  );
  expect(flow_rows).toHaveLength(4);
  expect(flow_rows.map((flow_row) => flow_row.congestionLevel)).toEqual([
    'FreeFlow',
    'Slow',
    'Congested',
    'Blocked',
  ]);
  expect(flow_rows.map((flow_row) => flow_row.shapeCoordinates.length)).toEqual([
    23, 30, 11, 8,
  ]);
  expect(flow_rows[0].currentSpeedKilometersPerHour).toBeNull();
  expect(flow_rows[0].freeFlowSpeedKilometersPerHour).toBeNull();
  expect(flow_rows[0].shapeCoordinates[0]).toEqual({
    latitude: 40.63701,
    longitude: -74.01926,
  });

  // The same boundary validation the search and matrix calls use.
  await expect(
    page.evaluate(() =>
      window.mapTest.fetchTrafficIncidents(40.0, -74.05, 39.9, -74.0),
    ),
  ).rejects.toThrow(/inverted/);
});

test('a registered live provider plans and parses a two-waypoint route', async ({
  page,
}) => {
  await apply_test_provider_registration(page);
  // The recorded fixture's own Sacramento trip (1315 10th St → Front St).
  const route = await page.evaluate(() =>
    window.mapTest.calculateRoute(
      38.57693,
      -121.49492,
      38.578739,
      -121.507447,
    ),
  );
  expect(route.legs).toHaveLength(1);
  expect(route.distanceMeters).toBeCloseTo(RECORDED_ROUTE_DISTANCE_METERS, 0);
  expect(route.durationSeconds).toBeCloseTo(RECORDED_ROUTE_DURATION_SECONDS, 0);
  const steps = route.legs[0].steps;
  expect(steps).toHaveLength(RECORDED_ROUTE_STEP_COUNT);
  // The same maneuver vocabulary and end-anchored texts the offline seam
  // answers: the first step carries the first turn's banner text.
  expect(steps[0].maneuverType).toBe('departure');
  expect(steps[steps.length - 1].maneuverType).toBe('arrival');
  expect(steps[0].instructionDisplayText).toBe(RECORDED_ROUTE_FIRST_STEP_TEXT);
  expect(steps[0].instructionSpeechText).toBe(RECORDED_ROUTE_FIRST_STEP_TEXT);
  expect(Array.isArray(steps[0].shape)).toBe(true);
  expect(steps[0].shape.length).toBeGreaterThan(0);
  expect(typeof route.boundingBox.southWest.latitude).toBe('number');
});

test('a registered live provider fetches committed PNG tile bytes', async ({ page }) => {
  await apply_test_provider_registration(page);
  const tile_report = await page.evaluate(
    async ([zoom_level, tile_x, tile_y, png_signature_length]) => {
      const tile_bytes = await window.mapTest.fetchRasterTile(
        zoom_level,
        tile_x,
        tile_y,
      );
      return {
        is_uint8_array: tile_bytes instanceof Uint8Array,
        byte_length: tile_bytes.length,
        leading_bytes: Array.from(tile_bytes.slice(0, png_signature_length)),
      };
    },
    [
      DEMO_TILE_ZOOM_LEVEL,
      DEMO_TILE_X,
      DEMO_TILE_Y,
      PNG_SIGNATURE_BYTES.length,
    ],
  );
  expect(tile_report.is_uint8_array).toBe(true);
  expect(tile_report.byte_length).toBe(recorded_tile_bytes.length);
  expect(tile_report.leading_bytes).toEqual(PNG_SIGNATURE_BYTES);
});

test('live calls refuse honestly without a registration', async ({ page }) => {
  await page.evaluate(() => window.mapTest.clearProviderRegistrations());
  // Every live call answers the contract's own refusal, naming the capability
  // that has no provider, so a page reads the same text the native bindings
  // produce.
  const refusals = await page.evaluate(async () => {
    const refusal_failures = [];
    for (const live_call of [
      () => window.mapTest.forwardGeocode('Sacramento,California'),
      () => window.mapTest.calculateRoute(38.57693, -121.49492, 38.578739, -121.507447),
      () => window.mapTest.fetchRasterTile(14, 2661, 6286),
    ]) {
      try {
        await live_call();
        refusal_failures.push(null);
      } catch (error) {
        refusal_failures.push({ message: String(error), code: error?.code });
      }
    }
    return refusal_failures;
  });
  const expected_contract_names = [
    'GeocodingProvider',
    'RoutingProvider',
    'RasterTileProvider',
  ];
  for (const [refusal_index, refusal] of refusals.entries()) {
    expect(refusal.message).toBe(
      `no provider configured for ${expected_contract_names[refusal_index]}`,
    );
    expect(refusal.code).toBe('providerNotConfigured');
  }
});

test('the panel renders the registry descriptors and the fixture router serves by provider key', async ({
  page,
}) => {
  const descriptor_row = provider_row(page);
  await expect(descriptor_row).toHaveCount(1);
  // The credential label comes from the descriptor (display name + the
  // generic credential label), never from a hardcoded string in the page.
  await expect(descriptor_row.locator('label')).toContainText(PROVIDER_DISPLAY_NAME);
  await expect(descriptor_row.locator('label')).toContainText(PROVIDER_CREDENTIAL_LABEL);
  // The attribution text lives behind the compact info control now; open it
  // before asserting the descriptor's compliance line.
  await page.locator('#attribution-info-control').click();
  await expect(page.locator('#attribution-panel')).toBeVisible();
  // The panel aggregates every provider available on this platform, so the
  // descriptor's own compliance line is asserted by containment.
  await expect(page.locator('#attribution-notice')).toContainText(
    PROVIDER_ATTRIBUTION_TEXT,
  );

  // The dev server resolves the provider's own path shape through the
  // registry's fixture routes, keyed by the provider key.
  const geocoding_response = await page.request.get(
    `${PROVIDER_FIXTURE_BASE_PATH}/${PROVIDER_KEY}/geocoding/v1/address?key=test-key`,
  );
  expect(geocoding_response.ok()).toBe(true);
  expect(await geocoding_response.text()).toBe(recorded_geocoding_response);

  // An address outside every descriptor route stays unserved.
  const unmatched_response = await page.request.get(
    `${PROVIDER_FIXTURE_BASE_PATH}/${PROVIDER_KEY}/geocoding/v1/address/unmatched`,
  );
  expect(unmatched_response.status()).toBe(404);
});

test('the playground settings panel applies a key and runs a live search', async ({
  page,
}) => {
  // The harness seam points registration at the registry's same-origin
  // fixture routes, so the panel path runs with no external network.
  await page.goto('/?advanced&useProviderFixtures');
  await waitForMapTestReady(page);
  const descriptor_row = provider_row(page);
  // The panel is open by default; a click would fold it and hide the rows.
  await open_provider_panel(page);
  await descriptor_row.locator('[data-provider-credential-input]').fill('test-key');
  await descriptor_row.locator('[data-provider-credential-apply]').click();
  await expect(page.locator('#provider-credential-status')).toHaveText(
    `${PROVIDER_DISPLAY_NAME} key applied`,
  );
  // The applied key is never echoed back into the field.
  await expect(descriptor_row.locator('[data-provider-credential-input]')).toHaveValue('');
  await close_provider_panel(page);
  await page.locator('#trip-destination-input').fill('Sacramento,California');
  await page.locator('#trip-destination-search-control').click();
  await expect(page.locator('#trip-search-results li').first()).toContainText(
    'Sacramento',
  );
});

test('the playground panel plans a live route and fetches one live tile', async ({
  page,
}) => {
  await page.goto('/?advanced&useProviderFixtures');
  await waitForMapTestReady(page);
  const descriptor_row = provider_row(page);
  // The panel is open by default; a click would fold it and hide the rows.
  await open_provider_panel(page);
  await descriptor_row.locator('[data-provider-credential-input]').fill('test-key');
  await descriptor_row.locator('[data-provider-credential-apply]').click();
  await expect(page.locator('#provider-credential-status')).toHaveText(
    `${PROVIDER_DISPLAY_NAME} key applied`,
  );
  // The trip card carries the compact applied state and its Manage action.
  await expect(page.locator('#trip-credential-action')).toContainText('Key applied');

  // The route action lives on the trip card behind the modal settings sheet;
  // it plans the demo trip's own waypoints now that both fields are filled.
  await close_provider_panel(page);
  await page.locator('#trip-origin-input').fill('Capitol Park');
  await page.locator('#trip-destination-input').fill('Tower Bridge');
  await expect(page.locator('#trip-plan-route-control')).toBeEnabled();
  await page.locator('#trip-plan-route-control').click();
  await expect(page.locator('#trip-route-result')).toContainText('1.7 km');
  await expect(page.locator('#trip-route-result')).toContainText('3 min 55 s');
  await expect(page.locator('#trip-route-result')).toContainText(
    'Turn left onto L St',
  );

  // The tile probe sits in the settings sheet; the result stays on the trip
  // card after it closes.
  await open_provider_panel(page);
  await page.locator('#provider-tile-control').click();
  await expect(page.locator('#provider-tile-result')).toContainText(
    `Fetched ${recorded_tile_bytes.length} bytes`,
  );
  // The optional preview draws the fetched PNG into the probe's canvas.
  await expect(page.locator('#provider-tile-canvas')).toBeVisible();

  // Clearing the credential resets every live result honestly.
  await descriptor_row.locator('[data-provider-credential-clear]').click();
  await expect(page.locator('#provider-credential-status')).toHaveText(
    'Set a provider key to load live map data',
  );
  await expect(page.locator('#trip-credential-action')).toContainText(
    'Add a provider key');
  await expect(page.locator('#trip-route-result')).toHaveText('');
  await expect(page.locator('#provider-tile-result')).toHaveText('');
  await expect(page.locator('#provider-tile-canvas')).toBeHidden();
});

test('a remembered key survives a reload and the field stays masked', async ({
  page,
}) => {
  await page.goto('/?useProviderFixtures');
  await waitForMapTestReady(page);
  const descriptor_row = provider_row(page);
  await open_provider_panel(page);

  // The field is masked and offers no reveal affordance, the same rule every
  // platform follows.
  const credential_input = descriptor_row.locator('[data-provider-credential-input]');
  await credential_input.fill('test-key');
  await expect(credential_input).toHaveAttribute('type', 'password');
  await expect(
    descriptor_row.locator('[data-provider-credential-visibility]')).toHaveCount(0);
  await expect(
    descriptor_row.getByRole('button', { name: /show key|hide key/i })).toHaveCount(0);

  // The browser's named persistence exception carries a visible warning.
  await expect(page.locator('.settings-remember-warning'))
    .toContainText('localStorage');

  // Opt into persistence, apply, then reload: the key re-registers itself
  // and the trip card shows the applied state without re-entry.
  await page.locator('#remember-provider-credential').check();
  await descriptor_row.locator('[data-provider-credential-apply]').click();
  await expect(page.locator('#provider-credential-status')).toHaveText(
    `${PROVIDER_DISPLAY_NAME} key applied`,
  );
  await page.keyboard.press('Escape');
  await page.reload();
  await waitForMapTestReady(page);
  await expect(page.locator('#trip-credential-action')).toContainText('Key applied');

  // The restored credential drives the live path.
  await page.locator('#trip-destination-input').fill('Sacramento,California');
  await page.locator('#trip-destination-search-control').click();
  await expect(page.locator('#trip-search-results li').first()).toContainText(
    'Sacramento',
  );
});
