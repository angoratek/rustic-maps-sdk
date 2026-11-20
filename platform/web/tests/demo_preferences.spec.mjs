// The browser demo's remembered choices (M37): explicit theme, base-map,
// demo-plan, scene, camera-preset, speed and search-field selections survive a
// reload through one versioned localStorage record. Transient state (a route
// result, an open dialog, the live camera pose) is never restored, and the
// provider-key opt-in store is a separate record the reset row never touches.
// Every provider fact comes from the shared registry and every trace/tile
// payload is a committed fixture, so the suite touches no external network.
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test, waitForMapTestReady } from './network_guard.mjs';
import { HOST, PORT } from '../serve.mjs';

/** The origin the harness serves and the browser permission grants name. */
const HARNESS_ORIGIN = `http://${HOST}:${PORT}`;
/** The stubbed device position; distinct from the launch camera center, so a
 * restored camera proves the fix centered it. */
const DEVICE_LATITUDE_DEGREES = 38.6012;
const DEVICE_LONGITUDE_DEGREES = -121.4512;
const DEVICE_ACCURACY_METERS = 5;
/** The settle budget for a fix or a traffic refresh. */
const DEVICE_FIX_TIMEOUT_MILLISECONDS = 20_000;
const TRAFFIC_SETTLE_TIMEOUT_MILLISECONDS = 20_000;

const TEST_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const REPOSITORY_ROOT = path.resolve(TEST_DIRECTORY, '..', '..', '..');
const PREFERENCES_STORAGE_KEY = 'rustic-maps-demo-preferences';
const PREFERENCES_STORAGE_KEYS = [
  'baseMap',
  'cameraPresetId',
  'capabilityProviders',
  'demoPlanScenario',
  'destinationText',
  'deviceLocationEnabled',
  'directionsRouteStrategy',
  'directionsTransportationMode',
  'lastSearchFieldKey',
  'lastSearchQueryText',
  'originText',
  'overlaySceneKeys',
  'speedMultiplier',
  'themeChoice',
  'trafficLayerEnabled',
  'version',
];

const providers_registry = JSON.parse(
  readFileSync(path.join(REPOSITORY_ROOT, 'platform', 'providers.json'), 'utf8'),
);
const reference_scenes = JSON.parse(
  readFileSync(path.join(REPOSITORY_ROOT, 'platform', 'reference-scenes.json'), 'utf8'),
);

/** The first registry vector base offering more than one named style. */
const vector_provider_descriptor = providers_registry.providers.find(
  (provider_descriptor) =>
    typeof provider_descriptor.url_template === 'string'
    && (provider_descriptor.vector_styles ?? []).length > 1,
);
const VECTOR_PROVIDER_KEY = vector_provider_descriptor.provider_key;
const VECTOR_STYLE_ID = vector_provider_descriptor.vector_styles[1].style_id;
const VECTOR_TILE_BYTES = readFileSync(path.join(
  REPOSITORY_ROOT,
  vector_provider_descriptor.fixture_routes.find(
    (fixture_route) => fixture_route.purpose === 'vector_tiles',
  ).fixture,
));
const DEMO_PLAN_SCENARIO = reference_scenes.demo_plans[0].scenario;
const CAMERA_PRESET = reference_scenes.camera_presets[1];

/** The descriptor that declares the traffic-flow capability and its fixture. */
const traffic_descriptor = providers_registry.providers.find(
  (provider_descriptor) =>
    (provider_descriptor.served_contracts ?? []).includes('traffic_flow'));
const TRAFFIC_REQUEST_PATH =
  `${providers_registry.fixture_base_path}/${traffic_descriptor.provider_key}`;

/** Answers every vector-tile request from the committed fixture. */
async function serve_vector_tiles(page) {
  const content_type = vector_provider_descriptor.url_template.endsWith('.pbf')
    ? 'application/x-protobuf'
    : 'application/vnd.mapbox-vector-tile';
  const route_pattern = vector_provider_descriptor.url_template.endsWith('.pbf')
    ? '**/*.pbf'
    : '**/*.mvt';
  await page.route(route_pattern, (route) => route.fulfill({
    status: 200,
    contentType: content_type,
    body: VECTOR_TILE_BYTES,
  }));
}

/** The stored preferences record, read through the page's own origin. */
async function stored_preferences(page) {
  return page.evaluate((storage_key) => {
    const stored_text = window.localStorage.getItem(storage_key);
    return stored_text === null ? null : JSON.parse(stored_text);
  }, PREFERENCES_STORAGE_KEY);
}

test('explicit choices survive a reload', async ({ page }) => {
  // Two renderer initializations and a recorded-trace load: a software
  // renderer under a parallel suite needs more than the default budget.
  test.setTimeout(120_000);
  await serve_vector_tiles(page);
  await page.goto('/?advanced');
  await waitForMapTestReady(page);
  await expect
    .poll(() => page.evaluate(() => window.mapCameraState()), { timeout: 20_000 })
    .not.toBeNull();

  // Theme: Auto cycles to Light, an explicit choice.
  await page.click('#settings-control');
  await page.click('#theme-toggle-control');
  const remembered_theme_choice = await page.evaluate(
    () => document.documentElement.dataset.themeChoice);
  expect(remembered_theme_choice).toBe('light');
  await page.keyboard.press('Escape');

  // Base map: the registry's second vector source and its second style,
  // chosen from the layers panel's single home.
  await page.click('#map-layers-control');
  await page.locator(`#map-layer-style-${VECTOR_STYLE_ID}`).click();
  await expect
    .poll(() => page.evaluate(() => window.mapTileMode()), { timeout: 20_000 })
    .toBe('vector');
  await expect
    .poll(() => page.evaluate(() => window.mapVectorBaseStatus()), { timeout: 20_000 })
    .toContain(VECTOR_STYLE_ID);
  await page.keyboard.press('Escape');

  // Demo plan: the launch-automation seam loads a committed recorded trace.
  await page.evaluate(
    (scenario) => window.selectDemoPlanScenario(scenario), DEMO_PLAN_SCENARIO);
  await expect
    .poll(
      () => page.evaluate(() => window.mapDemoPlanState()?.selectedScenario),
      { timeout: 20_000 },
    )
    .toBe(DEMO_PLAN_SCENARIO);

  // Camera preset: the second artifact pose, eased by the live control.
  await page
    .locator(`#camera-preset-controls [data-camera-preset-id="${CAMERA_PRESET.id}"]`)
    .click();
  await expect
    .poll(async () => {
      const state = await page.evaluate(() => window.mapCameraState());
      return Math.abs(state.latitude - CAMERA_PRESET.latitude_degrees) < 1e-3
        && Math.abs(state.longitude - CAMERA_PRESET.longitude_degrees) < 1e-3
        && Math.abs(state.zoom - CAMERA_PRESET.zoom_level) < 1e-3
        && Math.abs(state.bearing - CAMERA_PRESET.bearing_degrees) < 1e-3
        && Math.abs(state.pitch - CAMERA_PRESET.pitch_degrees) < 1e-3;
    }, { timeout: 5_000 })
    .toBe(true);

  // Search fields: a submitted query and the final endpoint texts.
  await page.locator('#trip-origin-input').fill('Capitol');
  await page.locator('#trip-origin-search-control').click();
  await page.locator('#trip-origin-input').fill('Capitol Park');
  await page.locator('#trip-destination-input').fill('Tower Bridge');

  // The record carries every explicit choice under its documented keys.
  const written_record = await stored_preferences(page);
  expect(written_record.version).toBe(1);
  expect(Object.keys(written_record).sort()).toEqual(PREFERENCES_STORAGE_KEYS);
  expect(written_record.themeChoice).toBe('light');
  expect(written_record.baseMap).toEqual({
    mode: 'vector',
    vectorProviderKey: VECTOR_PROVIDER_KEY,
    vectorStyleId: VECTOR_STYLE_ID,
  });
  expect(written_record.demoPlanScenario).toBe(DEMO_PLAN_SCENARIO);
  expect(written_record.cameraPresetId).toBe(CAMERA_PRESET.id);
  expect(written_record.originText).toBe('Capitol Park');
  expect(written_record.destinationText).toBe('Tower Bridge');
  expect(written_record.lastSearchFieldKey).toBe('origin');
  expect(written_record.lastSearchQueryText).toBe('Capitol');

  // Reload: each value is restored before the page is interactive.
  await page.reload();
  await waitForMapTestReady(page);
  await expect
    .poll(
      () => page.evaluate(() => document.documentElement.dataset.themeChoice),
      { timeout: 20_000 },
    )
    .toBe('light');
  await expect
    .poll(() => page.evaluate(() => window.mapTileMode()), { timeout: 20_000 })
    .toBe('vector');
  await expect
    .poll(() => page.evaluate(() => window.mapVectorProviderKey()))
    .toBe(VECTOR_PROVIDER_KEY);
  await expect
    .poll(
      () => page.evaluate(
        () => window.mapGpuCanvas()?.vectorTileSourceStatus().styleId ?? null),
      { timeout: 20_000 },
    )
    .toBe(VECTOR_STYLE_ID);
  await expect
    .poll(
      () => page.evaluate(() => window.mapDemoPlanState()?.selectedScenario),
      { timeout: 20_000 },
    )
    .toBe(DEMO_PLAN_SCENARIO);
  await expect(
    page.locator(
      `#camera-preset-controls [data-camera-preset-id="${CAMERA_PRESET.id}"]`,
    ),
  ).toHaveClass(/selected/);
  await expect
    .poll(async () => {
      const state = await page.evaluate(() => window.mapCameraState());
      return Math.abs(state.latitude - CAMERA_PRESET.latitude_degrees) < 1e-3
        && Math.abs(state.longitude - CAMERA_PRESET.longitude_degrees) < 1e-3
        && Math.abs(state.pitch - CAMERA_PRESET.pitch_degrees) < 1e-3;
    }, { timeout: 20_000 })
    .toBe(true);
  await expect(page.locator('#trip-origin-input')).toHaveValue('Capitol Park');
  await expect(page.locator('#trip-destination-input')).toHaveValue('Tower Bridge');
});

test('the style, traffic layer and centered device fix survive a reload', async ({
  page,
  context,
}) => {
  test.setTimeout(180_000);
  await serve_vector_tiles(page);
  await context.grantPermissions(['geolocation'], { origin: HARNESS_ORIGIN });
  await context.setGeolocation({
    latitude: DEVICE_LATITUDE_DEGREES,
    longitude: DEVICE_LONGITUDE_DEGREES,
    accuracy: DEVICE_ACCURACY_METERS,
  });
  await page.goto('/?advanced&useProviderFixtures');
  await waitForMapTestReady(page);
  await expect
    .poll(() => page.evaluate(() => window.mapCameraState()), { timeout: 20_000 })
    .not.toBeNull();

  // Every traffic request must stay on the harness's committed fixture route.
  const traffic_request_urls = [];
  page.on('request', (request) => {
    if (request.url().includes(TRAFFIC_REQUEST_PATH)) {
      traffic_request_urls.push(request.url());
    }
  });

  // The traffic capability needs the credential the opt-in store keeps; the
  // credential is not part of the preferences record under test.
  await page.click('#settings-control');
  await page.locator('#remember-provider-credential').check();
  const credential_row = page.locator(
    `[data-provider-key="${traffic_descriptor.provider_key}"]`);
  await credential_row.locator('[data-provider-credential-input]').fill('test-key');
  await credential_row.locator('[data-provider-credential-apply]').click();
  await expect(credential_row.locator('.provider-applied-chip')).toBeVisible();
  await page.keyboard.press('Escape');

  // A style switch the launch default does not already serve.
  await page.click('#map-layers-control');
  await page.locator(`#map-layer-style-${VECTOR_STYLE_ID}`).click();
  await expect
    .poll(() => page.evaluate(() => window.mapTileMode()), { timeout: 20_000 })
    .toBe('vector');
  await expect
    .poll(
      () => page.evaluate(() => window.mapGpuCanvas()?.vectorTileSourceStatus().styleId ?? null),
      { timeout: 20_000 },
    )
    .toBe(VECTOR_STYLE_ID);

  // The traffic layer, fed from the committed flow fixture.
  const traffic_switch = page.locator('#map-layer-traffic');
  await traffic_switch.click();
  await expect(traffic_switch).toHaveAttribute('aria-checked', 'true');
  await expect
    .poll(
      () => page.evaluate(() => window.mapTrafficLayerState().overlayInstallCount),
      { timeout: TRAFFIC_SETTLE_TIMEOUT_MILLISECONDS },
    )
    .toBeGreaterThan(0);
  const traffic_before_reload = await page.evaluate(() => window.mapTrafficLayerState());
  expect(traffic_before_reload.segmentCount).toBeGreaterThan(0);
  expect(traffic_request_urls.length).toBeGreaterThan(0);
  expect(traffic_request_urls.every((url) => url.startsWith(HARNESS_ORIGIN))).toBe(true);
  await page.keyboard.press('Escape');

  // The round control starts the source and centers on the first fix.
  await page.locator('#my-location-control').click();
  await expect
    .poll(
      () => page.evaluate(
        () => window.mapDeviceLocationState()?.fix?.latitude ?? null),
      { timeout: DEVICE_FIX_TIMEOUT_MILLISECONDS },
    )
    .not.toBeNull();
  await expect
    .poll(async () => {
      const camera = await page.evaluate(() => window.mapCameraState());
      return Math.abs(camera.latitude - DEVICE_LATITUDE_DEGREES) < 1e-3
        && Math.abs(camera.longitude - DEVICE_LONGITUDE_DEGREES) < 1e-3;
    }, { timeout: DEVICE_FIX_TIMEOUT_MILLISECONDS })
    .toBe(true);

  const written_record = await stored_preferences(page);
  expect(written_record.baseMap.vectorStyleId).toBe(VECTOR_STYLE_ID);
  expect(written_record.trafficLayerEnabled).toBe(true);
  expect(written_record.deviceLocationEnabled).toBe(true);

  // Reload: all three return from the record, not from a demo default.
  await page.reload();
  await waitForMapTestReady(page);
  await expect
    .poll(() => page.evaluate(() => window.mapTileMode()), { timeout: 20_000 })
    .toBe('vector');
  await expect
    .poll(
      () => page.evaluate(() => window.mapGpuCanvas()?.vectorTileSourceStatus().styleId ?? null),
      { timeout: 20_000 },
    )
    .toBe(VECTOR_STYLE_ID);
  await expect
    .poll(
      () => page.evaluate(() => window.mapTrafficLayerState().enabled),
      { timeout: TRAFFIC_SETTLE_TIMEOUT_MILLISECONDS },
    )
    .toBe(true);
  await expect
    .poll(
      () => page.evaluate(() => window.mapTrafficLayerState().overlayInstallCount),
      { timeout: TRAFFIC_SETTLE_TIMEOUT_MILLISECONDS },
    )
    .toBeGreaterThan(0);
  const traffic_after_reload = await page.evaluate(() => window.mapTrafficLayerState());
  expect(traffic_after_reload.segmentCount).toBe(traffic_before_reload.segmentCount);
  expect(traffic_request_urls.every((url) => url.startsWith(HARNESS_ORIGIN))).toBe(true);
  await expect
    .poll(
      () => page.evaluate(
        () => window.mapDeviceLocationState()?.fix?.latitude ?? null),
      { timeout: DEVICE_FIX_TIMEOUT_MILLISECONDS },
    )
    .not.toBeNull();
  await expect
    .poll(async () => {
      const camera = await page.evaluate(() => window.mapCameraState());
      return Math.abs(camera.latitude - DEVICE_LATITUDE_DEGREES) < 1e-3
        && Math.abs(camera.longitude - DEVICE_LONGITUDE_DEGREES) < 1e-3;
    }, { timeout: DEVICE_FIX_TIMEOUT_MILLISECONDS })
    .toBe(true);
  expect(await page.evaluate(() => window.mapDeviceLocationState().isActive)).toBe(true);
});

test('the reset row clears the record and a route result is never restored', async ({
  page,
}) => {
  await page.goto('/?useProviderFixtures');
  await waitForMapTestReady(page);

  // Plan a recorded demo route: its result is transient page state.
  await page.locator('#trip-origin-input').fill('Capitol Park');
  await page.locator('#trip-destination-input').fill('Tower Bridge');
  await page.locator('#trip-plan-route-control').click();
  await expect(page.locator('#trip-route-result')).toContainText(
    'Capitol Park → Tower Bridge');

  // Reload: the search texts return, the route result does not.
  await page.reload();
  await waitForMapTestReady(page);
  await expect(page.locator('#trip-origin-input')).toHaveValue('Capitol Park');
  await expect(page.locator('#trip-destination-input')).toHaveValue('Tower Bridge');
  await expect(page.locator('#trip-route-result')).toBeHidden();

  // Park a credential-store entry so the reset row's scope is provable.
  const provider_key = providers_registry.providers[0].provider_key;
  await page.evaluate((credential_provider_key) => {
    window.localStorage.setItem('rusticMapsSdk.rememberProviderCredentials', 'true');
    window.localStorage.setItem(
      `rusticMapsSdk.providerCredential.${credential_provider_key}`,
      'test-key',
    );
    // The older single-key theme store must never outlive the reset either.
    window.localStorage.setItem('rusticMapsSdk.themeChoice', 'dark');
  }, provider_key);

  // Reset clears the demo record and says so, leaving the credential store.
  await page.click('#settings-control');
  await page.click('#reset-remembered-choices-control');
  await expect(page.locator('#remembered-choices-status')).toHaveText(
    'Remembered choices cleared. Provider keys are unchanged.');
  expect(await stored_preferences(page)).toBeNull();
  expect(await page.evaluate(() => window.localStorage.getItem(
    'rusticMapsSdk.rememberProviderCredentials'))).toBe('true');
  expect(await page.evaluate((credential_provider_key) =>
    window.localStorage.getItem(
      `rusticMapsSdk.providerCredential.${credential_provider_key}`,
    ), provider_key)).toBe('test-key');
  expect(await page.evaluate(
    () => window.localStorage.getItem('rusticMapsSdk.themeChoice'))).toBeNull();

  // The next visit starts from the defaults, not the forgotten choices.
  await page.reload();
  await waitForMapTestReady(page);
  await expect(page.locator('#trip-origin-input')).toHaveValue('');
  await expect(page.locator('#trip-destination-input')).toHaveValue('');
  expect(await page.evaluate(
    () => document.documentElement.dataset.themeChoice)).toBe('auto');
  expect(await page.evaluate(() => window.mapTileMode())).toBe('raster');
  await expect(page.locator('#trip-route-result')).toBeHidden();
});

test('the settings sheet fits a small viewport and scrolls to its last row', async ({
  page,
}) => {
  const viewport = { width: 360, height: 640 };
  await page.setViewportSize(viewport);
  await page.goto('/?settings');
  await waitForMapTestReady(page);

  const dialog = page.locator('#settings-dialog');
  await expect(dialog).toBeVisible();
  const dialog_box = await dialog.boundingBox();
  expect(dialog_box.x).toBeGreaterThanOrEqual(0);
  expect(dialog_box.y).toBeGreaterThanOrEqual(0);
  expect(dialog_box.x + dialog_box.width).toBeLessThanOrEqual(viewport.width);
  expect(dialog_box.y + dialog_box.height).toBeLessThanOrEqual(viewport.height);

  // The content column scrolls inside the capped sheet.
  const body = page.locator('.settings-body');
  const scroll_metrics = await body.evaluate((element) => ({
    scroll_height: element.scrollHeight,
    client_height: element.clientHeight,
  }));
  expect(scroll_metrics.scroll_height).toBeGreaterThan(scroll_metrics.client_height);

  await body.evaluate((element) => {
    element.scrollTop = element.scrollHeight;
  });
  const reset_button_box = await page
    .locator('#reset-remembered-choices-control')
    .boundingBox();
  expect(reset_button_box.y + reset_button_box.height)
    .toBeLessThanOrEqual(viewport.height);
  expect(reset_button_box.y).toBeGreaterThanOrEqual(dialog_box.y);
  // The title and Close action stay pinned while the content scrolls.
  const title_box = await page.locator('#settings-title').boundingBox();
  expect(title_box.y).toBeGreaterThanOrEqual(dialog_box.y);
  expect(title_box.y + title_box.height)
    .toBeLessThanOrEqual(dialog_box.y + dialog_box.height);
  await expect(page.locator('#settings-close-control')).toBeVisible();
});
