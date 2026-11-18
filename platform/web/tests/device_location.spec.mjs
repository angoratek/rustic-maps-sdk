// Device positioning on the browser seam: the page's Geolocation source feeds
// the Rust `PositionController`, which validates and smooths the fix; the page
// draws the renderer-owned tracking dot, centres the camera on the first fix
// and reports the source state in the map layers panel's Device location row.
// The controller is the same `rustic-maps-positioning` type the native targets
// bind through the contract.
import { expect, test, waitForMapTestReady } from './network_guard.mjs';
import { HOST, PORT } from '../serve.mjs';

/** The origin the harness serves and the browser permission grants name. */
const HARNESS_ORIGIN = `http://${HOST}:${PORT}`;

/** The demo's anchor, used as the stubbed device position. */
const DEVICE_LATITUDE_DEGREES = 38.5766;
const DEVICE_LONGITUDE_DEGREES = -121.4944;
/** The stubbed horizontal accuracy, in meters. */
const DEVICE_ACCURACY_METERS = 5;
/** The settle budget for the first fix to reach the controller. */
const DEVICE_FIX_TIMEOUT_MILLISECONDS = 10_000;
/** The recorded Capitol Park -> Tower Bridge route's origin, used as the
 * device position for a device-driven trip. */
const DEVICE_ROUTE_ORIGIN_LATITUDE_DEGREES = 38.57693;
const DEVICE_ROUTE_ORIGIN_LONGITUDE_DEGREES = -121.49492;

/** Opens the map layers panel, the device-location status row's home. */
async function open_map_layers_panel(page) {
  const panel = page.locator('#map-layers-panel');
  if (!(await panel.isVisible())) {
    await page.locator('#map-layers-control').click();
  }
  await expect(panel).toBeVisible();
}

test('the round control starts the source, centres on the first fix and reports Active', async ({
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

  // The round control's off state requests permission, starts the source and
  // centres on the first fix.
  await page.locator('#my-location-control').click();
  await expect
    .poll(
      () => page.evaluate(() => window.mapDeviceLocationState()?.fix?.latitude ?? null),
      { timeout: DEVICE_FIX_TIMEOUT_MILLISECONDS },
    )
    .not.toBeNull();
  const state = await page.evaluate(() => window.mapDeviceLocationState());
  expect(state.isActive).toBe(true);
  expect(state.fix.latitude).toBeCloseTo(DEVICE_LATITUDE_DEGREES, 4);
  expect(state.fix.longitude).toBeCloseTo(DEVICE_LONGITUDE_DEGREES, 4);
  expect(state.fix.horizontalAccuracyMeters).toBeCloseTo(DEVICE_ACCURACY_METERS, 3);
  expect(state.sourceStatus).toBeNull();
  expect(state.isStale).toBe(false);
  await expect(page.locator('#my-location-control')).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  // The panel's row names the state; the settings sheet keeps the detail.
  await open_map_layers_panel(page);
  await expect(page.locator('#device-location-status')).toHaveText('Active');
  await expect(page.locator('#device-location-detail')).toContainText('Position');

  // The camera followed the first fix.
  const camera = await page.evaluate(() => window.mapCameraState());
  expect(camera.latitude).toBeCloseTo(DEVICE_LATITUDE_DEGREES, 3);
  expect(camera.longitude).toBeCloseTo(DEVICE_LONGITUDE_DEGREES, 3);

  // The panel's Stop action is the one stop surface.
  await page.locator('#device-location-stop').click();
  await expect(page.locator('#my-location-control')).toHaveAttribute(
    'aria-pressed',
    'false',
  );
  await expect(page.locator('#device-location-status')).toHaveText('Off');
  const stopped = await page.evaluate(() => window.mapDeviceLocationState());
  expect(stopped.isActive).toBe(false);
  expect(stopped.fix).toBeNull();
});

test('a denied permission is reported honestly to the controller, the panel and the chips', async ({
  page,
}) => {
  await page.addInitScript(() => {
    Object.defineProperty(navigator, 'geolocation', {
      configurable: true,
      value: {
        watchPosition: (_success, error) => {
          setTimeout(() => error({ code: 1 }), 0);
          return 1;
        },
        clearWatch: () => {},
        getCurrentPosition: () => {},
      },
    });
  });
  await page.goto('/');
  await waitForMapTestReady(page);
  await page.locator('#my-location-control').click();

  await expect
    .poll(
      () => page.evaluate(() => window.mapDeviceLocationState()?.sourceStatus ?? null),
      { timeout: DEVICE_FIX_TIMEOUT_MILLISECONDS },
    )
    .toBe('permissionDenied');
  await open_map_layers_panel(page);
  await expect(page.locator('#device-location-status')).toHaveText('Permission denied');
  await expect(page.locator('#device-location-detail')).toContainText(
    'Location permission was denied',
  );
  expect(await page.evaluate(() => window.mapDeviceLocationState().fix)).toBeNull();
  // The route chips carry the same refusal and stay disabled.
  await expect(page.locator('#origin-my-location')).toBeDisabled();
  await expect(page.locator('#destination-my-location')).toBeDisabled();
});

test('a planned route navigates from the device fixes and stops again', async ({
  page,
  context,
}) => {
  // The recorded Capitol Park -> Tower Bridge route starts here.
  await context.grantPermissions(['geolocation'], {
    origin: HARNESS_ORIGIN,
  });
  await context.setGeolocation({
    latitude: DEVICE_ROUTE_ORIGIN_LATITUDE_DEGREES,
    longitude: DEVICE_ROUTE_ORIGIN_LONGITUDE_DEGREES,
    accuracy: DEVICE_ACCURACY_METERS,
  });
  await page.goto('/');
  await waitForMapTestReady(page);

  // Without a planned route the control states what it needs instead of
  // starting a trip nobody planned.
  await page.locator('#settings-control').click();
  await page.locator('#device-navigation-control').click();
  await expect(page.locator('#device-location-detail')).toHaveText(
    'Plan a route first; device navigation follows the route you planned.');
  await expect(page.locator('#device-navigation-control')).toHaveAttribute(
    'aria-pressed',
    'false',
  );
  await page.locator('#settings-close-control').click();

  // Plan the recorded route through the search-backed fields.
  await page.locator('#trip-origin-input').fill('Capitol Park');
  await page.locator('#trip-destination-input').fill('Tower');
  await page.locator('#trip-destination-search-control').click();
  await page.locator('#trip-search-results button').first().click();
  await page.locator('#trip-plan-route-control').click();
  await expect(page.locator('#trip-route-result')).toContainText(
    'Capitol Park → Tower Bridge',
  );

  // Device navigation starts the location source it needs, starts guidance
  // on the planned route and shows the banner from the first device fix.
  await page.locator('#settings-control').click();
  await page.locator('#device-navigation-control').click();
  await expect(page.locator('#device-navigation-control')).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  await expect(page.locator('#device-navigation-control')).toHaveText(
    'Stop device navigation',
  );
  await expect(page.locator('#my-location-control')).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  await expect
    .poll(
      () => page.evaluate(
        () => document.getElementById('navigation-phase-value').textContent,
      ),
      { timeout: DEVICE_FIX_TIMEOUT_MILLISECONDS },
    )
    .not.toBe('');
  expect(await page.evaluate(
    () => document.getElementById('navigation-banner-overlay').hidden,
  )).toBe(false);
  // The first device fix reaches the controller and drives guidance.
  await expect
    .poll(
      () => page.evaluate(() => window.mapDeviceLocationState()?.fix?.latitude ?? null),
      { timeout: DEVICE_FIX_TIMEOUT_MILLISECONDS },
    )
    .not.toBeNull();
  const navigating_state = await page.evaluate(() => window.mapDeviceLocationState());
  expect(navigating_state.isNavigating).toBe(true);
  expect(navigating_state.fix.latitude).toBeCloseTo(
    DEVICE_ROUTE_ORIGIN_LATITUDE_DEGREES,
    4,
  );

  // Stopping ends the trip and the source the control started.
  await page.locator('#device-navigation-control').click();
  await expect(page.locator('#device-navigation-control')).toHaveAttribute(
    'aria-pressed',
    'false',
  );
  await expect(page.locator('#my-location-control')).toHaveAttribute(
    'aria-pressed',
    'false',
  );
  const stopped_state = await page.evaluate(() => window.mapDeviceLocationState());
  expect(stopped_state.isNavigating).toBe(false);
  expect(stopped_state.isActive).toBe(false);
});
