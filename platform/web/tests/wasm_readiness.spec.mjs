// Page readiness vs. module evaluation: the page's scripts are two module
// scripts, and the wasm binary is fetched by one of them. A browser may run
// the other's module body while that fetch is still in flight, so no module
// body may construct a wasm object before `mapTestReady` resolves. Holding the
// binary back makes that ordering deterministic, which is what these cases do.
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

/** How long the wasm response is held back, in milliseconds. */
const WASM_RESPONSE_DELAY_MILLISECONDS = 3_000;

// The generated package's module (the one `uniffiInitAsync` opens), served
// beside the package's entrypoint.
const WASM_BINARY_ROUTE = /\/generated\/rustic_maps\.wasm$/;

async function load_with_delayed_wasm(page) {
  const page_errors = [];
  page.on('pageerror', (error) => page_errors.push(error.message));
  await page.route(WASM_BINARY_ROUTE, async (route) => {
    await new Promise((resolve) => {
      setTimeout(resolve, WASM_RESPONSE_DELAY_MILLISECONDS);
    });
    await route.continue();
  });
  await page.goto('/');
  return page_errors;
}

test('the page installs fully when the wasm binary arrives after its scripts', async ({
  page,
}) => {
  const page_errors = await load_with_delayed_wasm(page);
  await waitForMapTestReady(page);
  // The last installs of the page's module body sit after every wasm-backed
  // seam, so a body that aborted mid-evaluation leaves this hook undefined.
  await expect
    .poll(() => page.evaluate(() => typeof window.mapDiagnosticsState), {
      timeout: 15_000,
    })
    .toBe('function');
  expect(page_errors).toEqual([]);
});

test('a delayed wasm binary leaves the location puck ready to glide', async ({
  page,
}) => {
  const page_errors = await load_with_delayed_wasm(page);
  await waitForMapTestReady(page);
  const pose = await page.evaluate(() => window.mapLocationPose?.() ?? null);
  expect(page_errors).toEqual([]);
  expect(pose === null || typeof pose === 'object').toBe(true);
});
