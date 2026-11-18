// The browser demo's Directions control (docs/design/DirectionsModesAndStrategy.md):
// every route plan carries a mode (Car or Walk, Car default) and a strategy
// (Fastest or Shortest, Fastest default). Both are pressed-choice button
// groups, both survive a reload through the page's remembered-choices record,
// and a plan actually sends the chosen values to the wasm façade's
// `calculateRouteWithOptions` call. The live path runs against the registry's
// own same-origin fixture routes, so the suite touches no external network.
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

const TEST_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const REPOSITORY_ROOT = path.resolve(TEST_DIRECTORY, '..', '..', '..');
const PREFERENCES_STORAGE_KEY = 'rustic-maps-demo-preferences';
const TEST_PROVIDER_CREDENTIAL = 'test-key';

const providers_registry = JSON.parse(
  readFileSync(path.join(REPOSITORY_ROOT, 'platform', 'providers.json'), 'utf8'),
);
/** The registry provider the wasm build can register. */
const provider_descriptor = providers_registry.providers.find(
  (candidate) => candidate.registration_available?.wasm === true
    && typeof candidate.credential_label === 'string',
);
const PROVIDER_KEY = provider_descriptor.provider_key;
const PROVIDER_DISPLAY_NAME = provider_descriptor.display_name;

/** The remembered-choices record as the page's own origin stores it. */
async function stored_preferences(page) {
  return page.evaluate((storage_key) => {
    const stored_text = window.localStorage.getItem(storage_key);
    return stored_text === null ? null : JSON.parse(stored_text);
  }, PREFERENCES_STORAGE_KEY);
}

/** Applies the registry provider's test credential through the settings UI,
 * so the page's live-plan path is active (not its recorded-route fallback). */
async function apply_test_credential(page) {
  await page.locator('#settings-control').click();
  await expect(page.locator('#settings-dialog')).toBeVisible();
  const provider_row = page.locator(
    `#provider-credential-rows [data-provider-key="${PROVIDER_KEY}"]`);
  await provider_row.locator('[data-provider-credential-input]')
    .fill(TEST_PROVIDER_CREDENTIAL);
  await provider_row.locator('[data-provider-credential-apply]').click();
  await expect(page.locator('#provider-credential-status')).toHaveText(
    `${PROVIDER_DISPLAY_NAME} key applied`);
  await page.keyboard.press('Escape');
  await expect(page.locator('#settings-dialog')).toBeHidden();
}

/** Fills the trip planner's two endpoints with the shared demo places. */
async function fill_demo_endpoints(page) {
  await page.locator('#trip-origin-input').fill('Capitol Park');
  await page.locator('#trip-destination-input').fill('Tower Bridge');
  await expect(page.locator('#trip-plan-route-control')).toBeEnabled();
}

/** Records the options object every live plan hands the wasm façade. */
async function record_route_options(page) {
  await page.evaluate(() => {
    const original_route_call = window.mapTest.calculateRouteWithOptions;
    window.directionsRouteCalls = [];
    window.mapTest.calculateRouteWithOptions = (...route_arguments) => {
      window.directionsRouteCalls.push(route_arguments[4]);
      return original_route_call(...route_arguments);
    };
  });
}

/** The route-option objects the page has sent, newest last. */
async function recorded_route_options(page) {
  return page.evaluate(() => window.directionsRouteCalls);
}

test('Car and Fastest are the defaults and both choices survive a reload', async ({
  page,
}) => {
  await page.goto('/');
  await waitForMapTestReady(page);

  // Defaults: Car and Fastest pressed, Walk and Shortest not.
  await expect(page.locator('#directions-mode-car')).toHaveAttribute(
    'aria-pressed', 'true');
  await expect(page.locator('#directions-mode-walk')).toHaveAttribute(
    'aria-pressed', 'false');
  await expect(page.locator('#directions-strategy-fastest')).toHaveAttribute(
    'aria-pressed', 'true');
  await expect(page.locator('#directions-strategy-shortest')).toHaveAttribute(
    'aria-pressed', 'false');
  await expect(page.locator('#directions-strategy-row')).toBeVisible();

  // A keyboard visitor operates the same buttons: Enter selects Walk, and the
  // strategy row hides because the plan walks.
  await page.locator('#directions-mode-walk').focus();
  await page.keyboard.press('Enter');
  await expect(page.locator('#directions-mode-walk')).toHaveAttribute(
    'aria-pressed', 'true');
  await expect(page.locator('#directions-mode-car')).toHaveAttribute(
    'aria-pressed', 'false');
  await expect(page.locator('#directions-strategy-row')).toBeHidden();

  // Back to Car, then Space selects Shortest; the pressed state is exposed.
  await page.locator('#directions-mode-car').focus();
  await page.keyboard.press('Enter');
  await expect(page.locator('#directions-strategy-row')).toBeVisible();
  await page.locator('#directions-strategy-shortest').focus();
  await page.keyboard.press('Space');
  await expect(page.locator('#directions-strategy-shortest')).toHaveAttribute(
    'aria-pressed', 'true');
  await expect(page.locator('#directions-strategy-fastest')).toHaveAttribute(
    'aria-pressed', 'false');

  // Choose Walk once more so the reload must restore both an explicit mode
  // and an explicit strategy.
  await page.locator('#directions-mode-walk').click();
  const written_record = await stored_preferences(page);
  expect(written_record.directionsTransportationMode).toBe('pedestrian');
  expect(written_record.directionsRouteStrategy).toBe('shortest');

  await page.reload();
  await waitForMapTestReady(page);
  await expect(page.locator('#directions-mode-walk')).toHaveAttribute(
    'aria-pressed', 'true');
  await expect(page.locator('#directions-mode-car')).toHaveAttribute(
    'aria-pressed', 'false');
  await expect(page.locator('#directions-strategy-row')).toBeHidden();

  // The remembered strategy is still there when the mode returns to Car.
  await page.locator('#directions-mode-car').click();
  await expect(page.locator('#directions-strategy-shortest')).toHaveAttribute(
    'aria-pressed', 'true');
});

test('a Walk plan sends pedestrian and a Shortest plan sends shortest', async ({
  page,
}) => {
  test.setTimeout(120_000);
  await page.goto('/?useProviderFixtures');
  await waitForMapTestReady(page);
  await apply_test_credential(page);
  await record_route_options(page);
  await fill_demo_endpoints(page);

  // Walk: the next live plan hands the façade the pedestrian mode.
  await page.locator('#directions-mode-walk').click();
  await page.locator('#trip-plan-route-control').click();
  await expect(page.locator('#trip-route-result')).toContainText(
    'Capitol Park → Tower Bridge');
  await expect.poll(async () => (await recorded_route_options(page)).length)
    .toBe(1);
  const walk_options = (await recorded_route_options(page))[0];
  expect(walk_options.transportationMode).toBe('pedestrian');
  expect(walk_options.routeStrategy).toBe('fastest');

  // A car plan with Shortest: the mode and the strategy both reach the call.
  await page.locator('#directions-mode-car').click();
  await page.locator('#directions-strategy-shortest').click();
  await page.locator('#trip-plan-route-control').click();
  await expect.poll(async () => (await recorded_route_options(page)).length)
    .toBe(2);
  const shortest_options = (await recorded_route_options(page))[1];
  expect(shortest_options.transportationMode).toBe('automobile');
  expect(shortest_options.routeStrategy).toBe('shortest');
});
