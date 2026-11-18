// The reference-application behavior model on the browser target
// (docs/design/ReferenceApplicationBehavior.md): the four named flows run
// through `window.runReferenceFlow`, the plan fields carry visible labels, a
// swap action and their My location chips, the route card reports distance,
// duration, mode, strategy and ETA, the provider-capability section composes
// its rows from the registry descriptors, and diagnostics stay off until the
// Advanced pane is opened.
//
// Every provider fact (key, display name, credential label, demo address,
// fixture path) is read from `platform/providers.json`, and the dev server
// serves its fixture routes same-origin, so this suite never names a vendor or
// touches the network.
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test, waitForMapTestReady } from './network_guard.mjs';
import { HOST, PORT } from '../serve.mjs';

/** The origin the harness serves and the browser permission grants name. */
const HARNESS_ORIGIN = `http://${HOST}:${PORT}`;

const TEST_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const REPOSITORY_ROOT = path.resolve(TEST_DIRECTORY, '..', '..', '..');
const providers_registry = JSON.parse(
  readFileSync(path.join(REPOSITORY_ROOT, 'platform', 'providers.json'), 'utf8'),
);
const PROVIDER_FIXTURE_BASE_PATH = providers_registry.fixture_base_path;
const TEST_PROVIDER_CREDENTIAL = 'test-key';
const DEVICE_LATITUDE_DEGREES = 38.5766;
const DEVICE_LONGITUDE_DEGREES = -121.4944;
const DEVICE_ACCURACY_METERS = 5;
const DEVICE_FIX_TIMEOUT_MILLISECONDS = 10_000;

/** The wasm-registerable provider the browser can apply a credential to. */
const credentialed_provider_descriptor = providers_registry.providers.find(
  (descriptor) => descriptor.registration_available?.wasm === true
    && typeof descriptor.credential_label === 'string'
    && descriptor.credential_label.trim() !== '',
);
const CREDENTIALED_PROVIDER_KEY = credentialed_provider_descriptor.provider_key;
const CREDENTIALED_PROVIDER_DISPLAY_NAME =
  credentialed_provider_descriptor.display_name;

/** The keyless routing descriptors the capability picker offers. */
const keyless_routing_descriptors = providers_registry.providers.filter(
  (descriptor) => descriptor.registration_available?.wasm === true
    && Array.isArray(descriptor.served_contracts)
    && descriptor.served_contracts.includes('routing')
    && (typeof descriptor.credential_label !== 'string'
      || descriptor.credential_label.trim() === ''),
);

/**
 * The routing descriptor that owns a committed fixture containing a ferry
 * leg. The fixture names its own endpoints, so this case plans the recorded
 * crossing without spelling a provider name or a URL shape.
 */
const ferry_fixture_descriptor = providers_registry.providers.find(
  (descriptor) => descriptor.registration_available?.wasm === true
    && Array.isArray(descriptor.fixture_routes)
    && descriptor.fixture_routes.some(
      (fixture_route) => typeof fixture_route.fixture === 'string'
        && fixture_route.fixture.includes('ferry')),
);
const ferry_fixture_route = ferry_fixture_descriptor.fixture_routes.find(
  (fixture_route) => typeof fixture_route.fixture === 'string'
    && fixture_route.fixture.includes('ferry'),
);
const ferry_fixture = JSON.parse(
  readFileSync(path.join(REPOSITORY_ROOT, ferry_fixture_route.fixture), 'utf8'),
);
const ferry_fixture_pattern = new RegExp(ferry_fixture_route.path_pattern);
/** Route-service waypoint positions are `[longitude, latitude]`. */
const ferry_origin_location = ferry_fixture.waypoints[0].location;
const ferry_destination_location = ferry_fixture.waypoints[1].location;

/** The provider-relative fixture request paths the page has requested. */
function record_provider_fixture_requests(page, provider_key) {
  const request_paths = [];
  const fixture_prefix = `${PROVIDER_FIXTURE_BASE_PATH}/${provider_key}/`;
  page.on('request', (request) => {
    const request_url = new URL(request.url());
    if (request_url.origin !== HARNESS_ORIGIN) return;
    if (!request_url.pathname.startsWith(fixture_prefix)) return;
    request_paths.push(
      decodeURIComponent(request_url.pathname.slice(fixture_prefix.length)));
  });
  return request_paths;
}

/** Selects one routing descriptor through the settings capability picker. */
async function select_routing_descriptor(page, descriptor) {
  await page.locator('#settings-control').click();
  await expect(page.locator('#settings-dialog')).toBeVisible();
  if (typeof descriptor.credential_label === 'string'
    && descriptor.credential_label.trim() !== '') {
    const provider_row = page.locator(
      `#provider-credential-rows [data-provider-key="${descriptor.provider_key}"]`);
    await provider_row.locator('[data-provider-credential-input]').fill(
      TEST_PROVIDER_CREDENTIAL);
    await provider_row.locator('[data-provider-credential-apply]').click();
  }
  await page.locator('#capability-provider-select-routing').selectOption(
    descriptor.provider_key);
  await expect(page.locator('#capability-provider-serving-routing')).toHaveText(
    `${descriptor.display_name} · Ready`);
  await page.keyboard.press('Escape');
  await expect(page.locator('#settings-dialog')).toBeHidden();
}

/** Plans the ferry fixture's own endpoints with the given route options. */
async function plan_ferry_route(page, route_options) {
  return page.evaluate((arguments_object) => window.mapTest
    .calculateRouteWithOptions(
      arguments_object.originLatitude,
      arguments_object.originLongitude,
      arguments_object.destinationLatitude,
      arguments_object.destinationLongitude,
      arguments_object.routeOptions,
    ), {
    originLatitude: ferry_origin_location[1],
    originLongitude: ferry_origin_location[0],
    destinationLatitude: ferry_destination_location[1],
    destinationLongitude: ferry_destination_location[0],
    routeOptions: route_options,
  });
}

/** The ferry step of a planned route, or undefined when none is present. */
function ferry_step_of(route) {
  return route.legs
    .flatMap((leg) => leg.steps)
    .find((step) => step.maneuverType === 'ferry');
}

/** Applies the credentialed provider's test key through the settings UI. */
async function apply_credentialed_provider(page) {
  await page.locator('#settings-control').click();
  await expect(page.locator('#settings-dialog')).toBeVisible();
  const provider_row = page.locator(
    `#provider-credential-rows [data-provider-key="${CREDENTIALED_PROVIDER_KEY}"]`);
  await provider_row.locator('[data-provider-credential-input]').fill(
    TEST_PROVIDER_CREDENTIAL);
  await provider_row.locator('[data-provider-credential-apply]').click();
  await expect(page.locator('#provider-credential-status')).toHaveText(
    `${CREDENTIALED_PROVIDER_DISPLAY_NAME} key applied`);
}

test('the plan-and-navigate flow drives the labelled fields, the route card and the trip', async ({
  page,
}) => {
  await page.goto('/');
  await waitForMapTestReady(page);

  // Diagnostics are off before the visitor opens the Advanced pane.
  expect(await page.evaluate(() => window.mapDiagnosticsState().subscribed))
    .toBe(false);

  // The two fields carry visible A/B labels and the swap action; each field
  // keeps its own Search action and My location chip. The marker is
  // decorative and excluded from the accessible name.
  await expect(page.locator('label[for="trip-origin-input"]')).toHaveText(
    'A Origin');
  await expect(page.locator('label[for="trip-destination-input"]')).toHaveText(
    'B Destination');
  await expect(page.locator('#trip-swap-control')).toHaveAttribute(
    'aria-label', 'Swap origin and destination');
  await expect(page.locator('#trip-origin-search-control')).toHaveText('Search');
  await expect(page.locator('#trip-destination-search-control')).toHaveText('Search');
  await expect(page.locator('#origin-my-location')).toHaveText('My location');
  await expect(page.locator('#destination-my-location')).toHaveText('My location');

  // The served flow schema (when present) or the built-in fallback runs the
  // same public user paths; the report names which one.
  const flow_report = await page.evaluate(() =>
    window.runReferenceFlow('plan_and_navigate'));
  expect(flow_report.flowId).toBe('plan_and_navigate');
  expect(['schema', 'fallback']).toContain(flow_report.source);

  // The route card reports distance, duration, mode, strategy and ETA from the
  // committed artifact.
  await expect(page.locator('#trip-route-result')).toBeVisible();
  await expect(page.locator('#trip-route-title')).toHaveText(
    'Capitol Park → Tower Bridge');
  await expect(page.locator('#trip-route-facts')).toHaveText(/\d+(\.\d+)? (m|km) · /);
  await expect(page.locator('#trip-route-directions')).toHaveText(
    'Pedestrian · Fastest');
  await expect(page.locator('#trip-route-eta')).toHaveText(/^ETA \d{2}:\d{2}$/);
  await expect(page.locator('#trip-route-maneuver')).not.toBeEmpty();

  // The trip started on the planned route.
  await expect
    .poll(() => page.evaluate(() => window.mapFollowDemoState().phase), {
      timeout: 15_000,
    })
    .toBe('engaged');
});

test('the my-location flow fills one field per chip without planning', async ({
  page,
  context,
}) => {
  await context.grantPermissions(['geolocation'], {
    origin: HARNESS_ORIGIN,
  });
  await context.setGeolocation({
    latitude: DEVICE_LATITUDE_DEGREES,
    longitude: DEVICE_LONGITUDE_DEGREES,
    accuracy: DEVICE_ACCURACY_METERS,
  });
  await page.goto('/');
  await waitForMapTestReady(page);

  const flow_report = await page.evaluate(() =>
    window.runReferenceFlow('my_location_field'));
  expect(flow_report.flowId).toBe('my_location_field');

  // Both fields hold the device fix's display text; no route was planned.
  await expect
    .poll(() => page.locator('#trip-origin-input').inputValue(), {
      timeout: DEVICE_FIX_TIMEOUT_MILLISECONDS,
    })
    .toBe(`${DEVICE_LATITUDE_DEGREES.toFixed(5)}, ${DEVICE_LONGITUDE_DEGREES.toFixed(5)}`);
  await expect(page.locator('#trip-destination-input')).toHaveValue(
    `${DEVICE_LATITUDE_DEGREES.toFixed(5)}, ${DEVICE_LONGITUDE_DEGREES.toFixed(5)}`);
  await expect(page.locator('#trip-route-result')).toBeHidden();
  await expect(page.locator('#trip-plan-route-control')).toBeEnabled();
});

test('the swap action exchanges both endpoint fields', async ({ page }) => {
  await page.goto('/');
  await waitForMapTestReady(page);

  await page.locator('#trip-origin-input').fill('Capitol Park');
  await page.locator('#trip-destination-input').fill('Tower Bridge');
  await page.locator('#trip-swap-control').click();
  await expect(page.locator('#trip-origin-input')).toHaveValue('Tower Bridge');
  await expect(page.locator('#trip-destination-input')).toHaveValue('Capitol Park');
  await expect(page.locator('#trip-plan-route-control')).toBeEnabled();
});

test('the routing capability picker composes descriptor rows and plans through the committed fixture', async ({
  page,
}) => {
  await page.goto('/?useProviderFixtures');
  await waitForMapTestReady(page);
  await apply_credentialed_provider(page);

  // The routing row names the serving provider with its credential state, and
  // the picker offers every registered routing descriptor in the same shape.
  const routing_serving = page.locator('#capability-provider-serving-routing');
  await expect(routing_serving).toHaveText(
    `${CREDENTIALED_PROVIDER_DISPLAY_NAME} · Ready`);
  await expect(page.locator('#capability-provider-select-routing')).toHaveValue(
    CREDENTIALED_PROVIDER_KEY);
  for (const keyless_descriptor of keyless_routing_descriptors) {
    await expect(
      page.locator(
        `#capability-provider-select-routing option[data-provider-key="${keyless_descriptor.provider_key}"]`),
    ).toHaveText(`${keyless_descriptor.display_name} · Ready`);
  }

  // A typed plan resolves both fields and plans through the committed
  // provider fixture.
  await page.keyboard.press('Escape');
  await page.locator('#trip-origin-input').fill('Capitol Park');
  await page.locator('#trip-destination-input').fill('Tower Bridge');
  await page.locator('#trip-plan-route-control').click();
  await expect(page.locator('#trip-route-result')).toBeVisible();
  await expect(page.locator('#trip-route-facts')).toHaveText(
    '1.7 km · 3 min 55 s');
  await expect(page.locator('#trip-route-directions')).toHaveText(
    'Automobile · Fastest');
  await expect(page.locator('#trip-route-eta')).toHaveText(/^ETA \d{2}:\d{2}$/);
});

test('a keyless routing provider receives its descriptor demo address', async ({
  page,
}) => {
  // The registry owns the public demo address; this suite reads it from the
  // descriptor and never spells a vendor URL itself.
  await page.goto('/?useProviderFixtures');
  await waitForMapTestReady(page);

  // Record every capability selection with its credential argument and whether
  // the wasm binding accepted it.
  await page.evaluate(() => {
    window.__providerSelectionCalls = [];
    const original_configure = window.mapTest.configureProvider;
    window.mapTest.configureProvider = (...selection_arguments) => {
      window.__providerSelectionCalls.push(selection_arguments);
      try {
        const result = original_configure(...selection_arguments);
        window.__providerSelectionSucceeded = true;
        return result;
      } catch (error) {
        window.__providerSelectionSucceeded = false;
        window.__providerSelectionError = String(error?.message ?? error);
        throw error;
      }
    };
  });

  // The provider-switch flow reads the routing row, selects the keyless
  // provider, returns to the map and plans the already-entered pair (the
  // schema's flow assumes the plan fields are filled).
  await page.locator('#trip-origin-input').fill('Capitol Park');
  await page.locator('#trip-destination-input').fill('Tower Bridge');
  const flow_report = await page.evaluate(() =>
    window.runReferenceFlow('routing_provider_switch'));
  expect(flow_report.flowId).toBe('routing_provider_switch');
  const selection_calls = await page.evaluate(
    () => window.__providerSelectionCalls);
  expect(selection_calls.length).toBeGreaterThanOrEqual(1);
  const selected_key = selection_calls[0][1];
  const keyless_descriptor = keyless_routing_descriptors.find(
    (descriptor) => descriptor.provider_key === selected_key);
  expect(keyless_descriptor).toBeDefined();
  expect(typeof keyless_descriptor.demo_base_url).toBe('string');
  expect(keyless_descriptor.demo_base_url.startsWith('https://')).toBe(true);
  // The descriptor's demo address, never an empty or local deployment.
  expect(selection_calls[0]).toEqual([
    'Routing',
    keyless_descriptor.provider_key,
    keyless_descriptor.demo_base_url,
  ]);

  const selection_succeeded = await page.evaluate(
    () => window.__providerSelectionSucceeded);
  if (selection_succeeded) {
    await expect(page.locator('#capability-provider-serving-routing')).toHaveText(
      `${keyless_descriptor.display_name} · Ready`);
    await expect(page.locator('#capability-provider-endpoint-routing')).toHaveText(
      `Demo server: ${keyless_descriptor.demo_base_url}`);
  } else {
    // This build's wasm dispatch does not carry the keyless provider yet: the
    // refusal crosses verbatim, the previous provider stays, and the row never
    // invents a serving name.
    const selection_error = await page.evaluate(
      () => window.__providerSelectionError);
    await expect(page.locator('#capability-provider-status')).toHaveText(
      selection_error);
    await expect(page.locator('#capability-provider-serving-routing')).toHaveText(
      'Not configured');
  }
});

test('the reference routing flow carries a ferry instruction through display and speech', async ({
  page,
}) => {
  test.setTimeout(120_000);
  await page.goto('/?useProviderFixtures');
  await waitForMapTestReady(page);
  await select_routing_descriptor(page, ferry_fixture_descriptor);

  const request_paths = record_provider_fixture_requests(
    page, ferry_fixture_descriptor.provider_key);

  // Car and Fastest is the first plan a reference application sends.
  const fastest_route = await plan_ferry_route(page, {
    transportationMode: 'automobile',
    routeStrategy: 'fastest',
  });
  // The same mode with Shortest: the provider has no strategy switch, so the
  // request and the crossing instruction must be unchanged.
  const shortest_route = await plan_ferry_route(page, {
    transportationMode: 'automobile',
    routeStrategy: 'shortest',
  });

  for (const [plan_label, planned_route] of [
    ['fastest', fastest_route],
    ['shortest', shortest_route],
  ]) {
    const ferry_step = ferry_step_of(planned_route);
    expect(ferry_step, `the ${plan_label} plan carries a ferry step`).toBeDefined();
    expect(ferry_step.instructionDisplayText).toBe('Take the ferry');
    expect(ferry_step.instructionSpeechText).toBe('Take the ferry');
  }

  // Both requests arrived, and both matched the descriptor's driving-profile
  // fixture route: the automobile choice reached the provider, and the
  // strategy choice did not change the request.
  expect(request_paths).toHaveLength(2);
  expect(request_paths[0]).toBe(request_paths[1]);
  for (const request_path of request_paths) {
    expect(
      ferry_fixture_pattern.test(request_path),
      `the request ${request_path} matched the ferry fixture route`,
    ).toBe(true);
  }
});

test('an unreachable routing deployment is reported verbatim with no fallback', async ({
  page,
}) => {
  // No fixture override: the credentialed provider registers against its real
  // endpoint, then the harness re-points it at a same-origin address the dev
  // server does not serve, so the request fails without leaving the origin.
  await page.goto('/');
  await waitForMapTestReady(page);
  await apply_credentialed_provider(page);

  // Reinstall the routing provider against an address the dev server does not
  // serve, through the capability picker (the browser-only base-URL seam is
  // the harness override; production selection uses the descriptor address).
  await page.evaluate((provider_key) => {
    window.mapTest.setProviderBaseUrl(
      provider_key,
      `${window.location.origin}/providers/${provider_key}-unreachable`,
    );
  }, CREDENTIALED_PROVIDER_KEY);
  // The provider is already serving, so dispatch the picker's change event
  // directly to reinstall it against the overridden base URL.
  await page.evaluate((provider_key) => {
    const provider_picker = document.getElementById(
      'capability-provider-select-routing');
    provider_picker.value = provider_key;
    provider_picker.dispatchEvent(new Event('change', { bubbles: true }));
  }, CREDENTIALED_PROVIDER_KEY);
  await page.keyboard.press('Escape');

  await page.locator('#trip-origin-input').fill('Capitol Park');
  await page.locator('#trip-destination-input').fill('Tower Bridge');
  await page.locator('#trip-plan-route-control').click();

  // The failure is verbatim, the route card stays absent, the recorded demo
  // route is never substituted, and Plan route re-enables.
  await expect(page.locator('#provider-credential-status')).toContainText('Refused:');
  await expect(page.locator('#trip-route-result')).toBeHidden();
  await expect(page.locator('#trip-plan-route-control')).toBeEnabled();
  expect(await page.evaluate(
    () => document.getElementById('trip-route-result').textContent))
    .not.toContain('Recorded demo route');

  // The routing row names the retry-or-own-deployment next step.
  await page.locator('#settings-control').click();
  await expect(page.locator('#capability-provider-next-step-routing')).toBeVisible();
  await expect(page.locator('#capability-provider-next-step-routing')).toContainText(
    'The demo server did not answer. Retry, or enter your own deployment address.');
});

test('diagnostics stay off until the Advanced pane opens, then stream a bounded window', async ({
  page,
}) => {
  await page.goto('/');
  await waitForMapTestReady(page);
  // Renders happen while the pane stays closed; no subscription is made.
  await page.locator('#zoom-in-control').click();
  await page.waitForTimeout(300);
  expect(await page.evaluate(() => window.mapDiagnosticsState().subscribed))
    .toBe(false);

  // Opening the pane is the opt-in: one stream at Info, the shared 100-event
  // window and the 500 ms refresh cadence.
  await page.locator('#advanced-panel > summary').click();
  await expect
    .poll(() => page.evaluate(() => window.mapDiagnosticsState().subscribed))
    .toBe(true);
  const opened_state = await page.evaluate(() => window.mapDiagnosticsState());
  expect(opened_state.debugEnabled).toBe(false);
  expect(opened_state.eventWindow).toBe(100);
  expect(opened_state.refreshCadenceMilliseconds).toBe(500);
  await expect(page.locator('#diagnostics-debug-events-label')).toHaveText(
    'Show debug events');

  // The explicit switch lowers the gate and relabels itself.
  await page.locator('#diagnostics-debug-toggle').check();
  await expect(page.locator('#diagnostics-debug-events-label')).toHaveText(
    'Hide debug events');
  expect(await page.evaluate(() => window.mapDiagnosticsState().debugEnabled))
    .toBe(true);

  // The named diagnostics flow reads the same public controls; Clear resets
  // the pane's displayed list.
  const flow_report = await page.evaluate(() =>
    window.runReferenceFlow('diagnostics_opt_in'));
  expect(flow_report.flowId).toBe('diagnostics_opt_in');
  expect(await page.evaluate(
    () => window.mapDiagnosticsState().clearedAtMilliseconds)).not.toBeNull();
  await expect(page.locator('#diagnostics-event-lines')).toHaveText(
    'No SDK events recorded yet.');

  // The opt-in is per session: a relaunch starts with diagnostics off.
  await page.reload();
  await waitForMapTestReady(page);
  expect(await page.evaluate(() => window.mapDiagnosticsState().subscribed))
    .toBe(false);
});
