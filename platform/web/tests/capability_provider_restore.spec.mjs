// The per-capability provider proof (M40, docs/design/
// ProviderSelectionPerCapability.md): the settings surface switches routing to
// the registry's keyless routing provider, plans the demo pair through the
// committed routing fixture served same-origin, reloads, and finds the same
// provider still serving with the plan resolving from the fixture. Every
// provider fact (key, display name, demo address, fixture path) comes from
// `platform/providers.json`, so this suite names no vendor, and the network
// guard aborts anything that leaves the harness origin.
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

const TEST_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const REPOSITORY_ROOT = path.resolve(TEST_DIRECTORY, '..', '..', '..');

/** The shared provider registry the page renders. */
const providers_registry = JSON.parse(
  readFileSync(path.join(REPOSITORY_ROOT, 'platform', 'providers.json'), 'utf8'),
);

/** The keyless routing descriptor the settings picker offers. */
const keyless_routing_descriptor = providers_registry.providers.find(
  (provider_descriptor) =>
    provider_descriptor.registration_available?.wasm === true
    && Array.isArray(provider_descriptor.served_contracts)
    && provider_descriptor.served_contracts.includes('routing')
    && (typeof provider_descriptor.credential_label !== 'string'
      || provider_descriptor.credential_label.trim() === ''),
);
const PROVIDER_KEY = keyless_routing_descriptor.provider_key;
const PROVIDER_DISPLAY_NAME = keyless_routing_descriptor.display_name;
const READY_ROW_TEXT = `${PROVIDER_DISPLAY_NAME} · Ready`;
const PROVIDER_FIXTURE_BASE_PATH = providers_registry.fixture_base_path;

/** The routing fixture route that serves one provider-relative request path. */
function routing_fixture_route_for(request_path) {
  const routing_fixture_route = keyless_routing_descriptor.fixture_routes.find(
    (candidate_route) => candidate_route.purpose === 'routing'
      && new RegExp(candidate_route.path_pattern).test(request_path),
  );
  if (routing_fixture_route === undefined) {
    throw new Error(
      `no committed routing fixture route matches the request ${request_path}`);
  }
  return routing_fixture_route;
}

/** The route record the fixture route serving one request returns. */
function recorded_route_for(request_path) {
  const routing_fixture_route = routing_fixture_route_for(request_path);
  const recorded_route_payload = JSON.parse(
    readFileSync(path.join(REPOSITORY_ROOT, routing_fixture_route.fixture), 'utf8'),
  );
  return recorded_route_payload.routes[0];
}

/** Records the provider-relative fixture request paths the page sends. */
function record_provider_fixture_requests(page) {
  const request_paths = [];
  const fixture_prefix = `${PROVIDER_FIXTURE_BASE_PATH}/${PROVIDER_KEY}/`;
  const harness_origin = new URL(page.url()).origin;
  page.on('request', (request) => {
    const request_url = new URL(request.url());
    if (request_url.origin !== harness_origin) return;
    if (!request_url.pathname.startsWith(fixture_prefix)) return;
    request_paths.push(
      decodeURIComponent(request_url.pathname.slice(fixture_prefix.length)));
  });
  return request_paths;
}

/** The one distance phrase the route card renders (metric, one decimal). */
function distance_phrase(distance_meters) {
  return distance_meters < 1000
    ? `${Math.round(distance_meters)} m`
    : `${(distance_meters / 1000).toFixed(1)} km`;
}

/** The one duration phrase the route card renders (minutes and seconds). */
function duration_phrase(duration_seconds) {
  const whole_minutes = Math.floor(duration_seconds / 60);
  const remainder_seconds = duration_seconds - whole_minutes * 60;
  return whole_minutes > 0
    ? `${whole_minutes} min ${Math.round(remainder_seconds)} s`
    : `${Math.round(duration_seconds)} s`;
}

/** The route card's facts for the recorded route that answered a request. */
function recorded_route_facts(request_path) {
  const recorded_route = recorded_route_for(request_path);
  return `${distance_phrase(recorded_route.distance)} · `
    + `${duration_phrase(recorded_route.duration)}`;
}

/** Opens the settings sheet when it is closed. */
async function open_settings(page) {
  const settings_dialog = page.locator('#settings-dialog');
  if (!(await settings_dialog.evaluate((element) => element.open))) {
    await page.locator('#settings-control').click();
  }
  await expect(settings_dialog).toBeVisible();
}

/** Closes the settings sheet; the trip card's controls live behind it. */
async function close_settings(page) {
  const settings_dialog = page.locator('#settings-dialog');
  if (await settings_dialog.evaluate((element) => element.open)) {
    await page.keyboard.press('Escape');
  }
  await expect(settings_dialog).toBeHidden();
}

/** Fills the demo pair and plans through the serving routing provider. */
async function plan_demo_route(page, request_paths) {
  const previous_request_count = request_paths.length;
  await page.locator('#trip-origin-input').fill('Capitol Park');
  await page.locator('#trip-destination-input').fill('Tower Bridge');
  await page.locator('#trip-plan-route-control').click();
  await expect(page.locator('#trip-route-result')).toBeVisible();
  // The request the fixture route answered names the committed payload whose
  // facts the card must carry; a different recorded route would change them.
  await expect
    .poll(() => request_paths.length)
    .toBeGreaterThan(previous_request_count);
  const request_path = request_paths[request_paths.length - 1];
  await expect(page.locator('#trip-route-facts')).toHaveText(
    recorded_route_facts(request_path));
  // A live provider plan carries no recorded-route note; a fallback would.
  await expect(page.locator('#trip-route-note')).toBeHidden();
}

test('a keyless routing choice survives a reload and plans from the committed fixture', async ({
  page,
}) => {
  await page.goto('/?useProviderFixtures');
  await waitForMapTestReady(page);
  const request_paths = record_provider_fixture_requests(page);

  // The registry's keyless routing descriptor offers the row and its picker
  // entry; nothing serves routing before the visitor chooses.
  await open_settings(page);
  const routing_picker = page.locator('#capability-provider-select-routing');
  const routing_serving = page.locator('#capability-provider-serving-routing');
  // The picker's option text is applied by the readiness refresh after the
  // provider readback lands; under a loaded host that can trail the default
  // assertion window, so the row waits with the suite's readiness budget.
  await expect(
    routing_picker.locator(`option[data-provider-key="${PROVIDER_KEY}"]`),
  ).toHaveText(READY_ROW_TEXT, { timeout: 15_000 });
  await routing_picker.selectOption(PROVIDER_KEY);
  await expect(routing_serving).toHaveText(READY_ROW_TEXT, { timeout: 15_000 });
  await expect(routing_picker).toHaveValue(PROVIDER_KEY);
  await close_settings(page);

  // The plan asks the selected provider; the same-origin fixture route
  // answers with the committed payload, so the card's facts are its facts.
  await plan_demo_route(page, request_paths);

  // The explicit choice is the versioned preferences record's entry.
  const stored_preferences = await page.evaluate(() =>
    JSON.parse(window.localStorage.getItem('rustic-maps-demo-preferences')));
  expect(stored_preferences.capabilityProviders).toEqual({
    routing: PROVIDER_KEY,
  });

  // Reload: the remembered choice re-applies before the first interaction,
  // the row names the same provider, and the plan resolves again from the
  // same committed fixture.
  await page.reload();
  await waitForMapTestReady(page);
  await open_settings(page);
  await expect(routing_serving).toHaveText(READY_ROW_TEXT);
  await expect(routing_picker).toHaveValue(PROVIDER_KEY);
  await close_settings(page);
  await plan_demo_route(page, request_paths);
});
