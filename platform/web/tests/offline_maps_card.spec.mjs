// The offline-maps card of the browser reference app (M31, docs/ReferenceAppUX.md
// §4.1/§2.5): the card plans the recorded routing-grid demo request over the
// committed archive bytes the dev server answers same-origin, through the
// contract's `offlineRouteFromArchiveBytes` free function — no provider, no
// external network. It states its recorded-test-graph provenance, draws and
// frames the route, and reports a refused archive verbatim rather than
// pretending a plan happened.
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

// The routing-grid fixture facts (the compiler's round-trip test pins them):
// two 111 m residential segments at 30 km/h.
const GRID_ROUTE_DISTANCE_METERS = 222;
const GRID_ROUTE_DURATION_SECONDS = 26.64;
// The fitted camera keeps the grid route centered near its midpoint.
const GRID_ROUTE_MIDPOINT_LATITUDE_DEGREES = 0.001;
const GRID_ROUTE_MIDPOINT_LONGITUDE_DEGREES = 0.001;
const FIT_TOLERANCE_DEGREES = 0.01;

test.beforeEach(async ({ page }) => {
  await page.goto('/?advanced');
  await waitForMapTestReady(page);
  await expect
    .poll(() => page.evaluate(() => window.mapCameraState()), { timeout: 20_000 })
    .not.toBeNull();
});

test('the offline card plans the recorded graph route and states its provenance', async ({
  page,
}) => {
  await expect(page.locator('#offline-maps-controls h2')).toHaveText('Offline maps');
  const route_control = page.getByRole('button', { name: 'Plan offline route' });
  await expect(route_control).toBeEnabled();
  await route_control.click();

  const result = page.locator('#offline-route-result');
  await expect(result).toBeVisible();
  await expect(result).toContainText('Offline demo route');
  await expect(result).toContainText('Planned offline from a recorded test graph');
  await expect(page.locator('#offline-route-facts')).toHaveText(
    `${GRID_ROUTE_DISTANCE_METERS} m · ${Math.round(GRID_ROUTE_DURATION_SECONDS)} s`,
  );
  await expect(page.locator('#offline-maps-status')).toBeHidden();

  // The route is framed through the same fitted camera the live planner
  // uses, so the visitor sees the geometry the card describes.
  await expect
    .poll(async () => {
      const camera_state = await page.evaluate(() => window.mapCameraState());
      return Math.abs(
        camera_state.latitude - GRID_ROUTE_MIDPOINT_LATITUDE_DEGREES,
      ) < FIT_TOLERANCE_DEGREES
        && Math.abs(
          camera_state.longitude - GRID_ROUTE_MIDPOINT_LONGITUDE_DEGREES,
        ) < FIT_TOLERANCE_DEGREES;
    }, { timeout: 10_000 })
    .toBe(true);
});

test('a missing archive is reported verbatim and claims no route', async ({ page }) => {
  await page.route('**/offline-graph/routing_grid.rmog', (route) =>
    route.fulfill({ status: 404, body: 'not found' }));
  await page.getByRole('button', { name: 'Plan offline route' }).click();

  await expect(page.locator('#offline-maps-status')).toBeVisible();
  await expect(page.locator('#offline-maps-status')).toContainText(
    'the offline graph fixture is not served (404)',
  );
  await expect(page.locator('#offline-route-result')).toBeHidden();
  // The card is usable again after the refusal.
  await expect(page.getByRole('button', { name: 'Plan offline route' })).toBeEnabled();
});
