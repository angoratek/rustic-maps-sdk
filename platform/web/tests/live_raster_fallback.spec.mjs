// The worldwide raster fallback: the canvas asks the same-origin recorded
// fixtures first and, only when an address has none, the registry's live
// raster template. A page served from a local host keeps its tile traffic
// same-origin unless it opts in with `?liveTiles`, so the automated suite
// never contacts a tile service; both halves are asserted here through an
// intercepted route, and every provider fact comes from the shared registry.
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

const TEST_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const REPOSITORY_ROOT = path.resolve(TEST_DIRECTORY, '..', '..', '..');

/** The shared provider registry that declares the raster tile template. */
const PROVIDERS_REGISTRY = JSON.parse(
  readFileSync(path.join(REPOSITORY_ROOT, 'platform', 'providers.json'), 'utf8'),
);
/** The first registry descriptor declaring a raster tile URL template. */
const RASTER_TEMPLATE_DESCRIPTOR = PROVIDERS_REGISTRY.providers.find(
  (descriptor) => typeof descriptor.raster_url_template === 'string',
);
if (RASTER_TEMPLATE_DESCRIPTOR === undefined) {
  throw new Error('the registry declares no provider with a raster_url_template');
}
/** The tile host the descriptor's template addresses. */
const RASTER_TILE_HOST = new URL(
  RASTER_TEMPLATE_DESCRIPTOR.raster_url_template
    .replace('{z}', '0')
    .replace('{x}', '0')
    .replace('{y}', '0'),
).host;
/** A recorded tile the intercepted live route answers with. */
const RECORDED_TILE_BYTES = readFileSync(path.join(
  REPOSITORY_ROOT,
  'crates',
  'rustic-maps-rendering',
  'fixtures',
  'render',
  'tiles',
  '14_2662_6286.png',
));
// A zoom level whose whole world fits in a handful of tiles, so the demo's
// launch camera is outside the recorded fixture area at every address.
const WORLD_ZOOM_LEVEL = 3;
/** GPU initialization and first-frame budget, as in the other page suites. */
const RENDERER_READINESS_TIMEOUT_MILLISECONDS = 30_000;
/** Frames and fetches the fallback may need after the camera moves. */
const FALLBACK_SETTLE_TIMEOUT_MILLISECONDS = 20_000;

/**
 * Moves the page's camera to `zoomLevel` over the demo center by driving the
 * zoom slider, the same seam a reader uses.
 */
async function zoom_to(page, zoomLevel) {
  await page.evaluate((targetZoomLevel) => {
    const zoom_slider = document.getElementById('zoom-control');
    zoom_slider.value = String(targetZoomLevel);
    zoom_slider.dispatchEvent(new Event('input', { bubbles: true }));
  }, zoomLevel);
}

test('a local page keeps tile traffic same-origin and reports the uncovered tiles', async ({
  page,
}) => {
  const external_requests = [];
  await page.route(`https://${RASTER_TILE_HOST}/**`, (route) => {
    external_requests.push(route.request().url());
    return route.fulfill({ status: 404, body: '' });
  });
  await page.goto('/?advanced');
  await waitForMapTestReady(page);
  await expect
    .poll(() => page.evaluate(() => (typeof window.mapRendererMode === 'function' ? window.mapRendererMode() : null)), {
      timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS,
    })
    .toBe('gpu');
  await zoom_to(page, WORLD_ZOOM_LEVEL);
  // A settled world camera has no recorded fixture, and the page is local,
  // so the frame must report the missing tiles rather than phone home.
  await expect
    .poll(
      () => page.evaluate(() => window.mapLastFrameStats()?.missingTiles ?? 0),
      { timeout: FALLBACK_SETTLE_TIMEOUT_MILLISECONDS },
    )
    .toBeGreaterThan(0);
  expect(external_requests).toEqual([]);
});

test('the live fallback draws tiles anywhere when a local page opts in', async ({ page }) => {
  const served_tile_urls = [];
  await page.route(`https://${RASTER_TILE_HOST}/**`, (route) => {
    served_tile_urls.push(route.request().url());
    return route.fulfill({
      status: 200,
      contentType: 'image/png',
      body: RECORDED_TILE_BYTES,
    });
  });
  await page.goto('/?advanced&liveTiles');
  await waitForMapTestReady(page);
  await expect
    .poll(() => page.evaluate(() => (typeof window.mapRendererMode === 'function' ? window.mapRendererMode() : null)), {
      timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS,
    })
    .toBe('gpu');
  await zoom_to(page, WORLD_ZOOM_LEVEL);
  await expect
    .poll(() => served_tile_urls.length, { timeout: FALLBACK_SETTLE_TIMEOUT_MILLISECONDS })
    .toBeGreaterThan(0);
  await expect
    .poll(
      () => page.evaluate(() => window.mapLastFrameStats()?.drawnTiles ?? 0),
      { timeout: FALLBACK_SETTLE_TIMEOUT_MILLISECONDS },
    )
    .toBeGreaterThan(0);
  // The fallback addresses the descriptor's own template at the test zoom.
  expect(served_tile_urls[0]).toContain(`/${WORLD_ZOOM_LEVEL}/`);
});
