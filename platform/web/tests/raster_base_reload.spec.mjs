// Raster provider changes reload the canvas' pooled raster state: selecting a
// base row, applying a credential and clearing one all call the page's reload
// seam, so a warm pool from the previous source cannot draw again. The bytes
// the switched-to provider serves are the committed fixture under its own
// registry route; every request stays same-origin.
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test, waitForMapTestReady } from './network_guard.mjs';
import { MAP_LAYER_BASE_ROW_IDENTIFIER_PREFIX } from '../map_layers.mjs';

const TEST_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const REPOSITORY_ROOT = path.resolve(TEST_DIRECTORY, '..', '..', '..');
const RENDERER_READINESS_TIMEOUT_MILLISECONDS = 30_000;

const providers_registry = JSON.parse(
  readFileSync(path.join(REPOSITORY_ROOT, 'platform', 'providers.json'), 'utf8'),
);

/** The first credentialed raster descriptor with one recorded provider tile. */
const credentialed_raster_descriptor = providers_registry.providers.find(
  (provider_descriptor) =>
    (provider_descriptor.served_contracts ?? []).includes('raster_tiles')
    && provider_descriptor.registration_available?.wasm === true
    && typeof provider_descriptor.credential_label === 'string'
    && provider_descriptor.credential_label.trim() !== ''
    && !(provider_descriptor.fixture_routes ?? []).some(
      (fixture_route) => fixture_route.purpose === 'raster_tiles'
        && fixture_route.fixture_kind === 'tile_directory'));
if (credentialed_raster_descriptor === undefined) {
  throw new Error('the registry declares no credentialed raster provider');
}
const CREDENTIALED_PROVIDER_ROOT =
  `${providers_registry.fixture_base_path}/${credentialed_raster_descriptor.provider_key}`;
const CREDENTIALED_RASTER_ROUTE_PATTERNS =
  credentialed_raster_descriptor.fixture_routes
    .filter((fixture_route) => fixture_route.purpose === 'raster_tiles')
    .map((fixture_route) => new RegExp(fixture_route.path_pattern));

/** Opens the on-map layers panel. */
async function open_map_layers_panel(page) {
  await page.locator('#map-layers-control').click();
  await expect(page.locator('#map-layers-panel')).toBeVisible();
}

/** Applies one provider's credential through the settings surface. */
async function apply_provider_credential(page, provider_descriptor) {
  await page.locator('#settings-control').click();
  const credential_row = page.locator(
    `[data-provider-key="${provider_descriptor.provider_key}"]`);
  await credential_row.locator('[data-provider-credential-input]').fill('test-key');
  await credential_row.locator('[data-provider-credential-apply]').click();
  await expect(
    credential_row.locator('.provider-applied-chip')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.locator('#settings-dialog')).toBeHidden();
}

/** Waits for the renderer's mode to answer. */
async function wait_for_renderer_mode(page) {
  await expect
    .poll(() => page.evaluate(() => (
      typeof window.mapRendererMode === 'function' ? window.mapRendererMode() : null)), {
      timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS,
    })
    .not.toBeNull();
  return page.evaluate(() => window.mapRendererMode());
}

test('selecting and clearing a raster provider reloads the raster tile source', async ({
  page,
}) => {
  test.setTimeout(120_000);
  await page.goto('/?advanced&useProviderFixtures');
  await waitForMapTestReady(page);
  const renderer_mode = await wait_for_renderer_mode(page);
  if (renderer_mode !== 'gpu') {
    test.skip(true, 'headless WebGPU unavailable; there is no GPU canvas to reload');
    return;
  }
  // Count the page's calls to the canvas' documented reload seam.
  const seam_was_wrapped = await page.evaluate(() => {
    const canvas = window.mapGpuCanvas();
    if (canvas === null) return false;
    const original_reload = canvas.reloadTileSource.bind(canvas);
    window.__rasterReloadCount = 0;
    canvas.reloadTileSource = () => {
      window.__rasterReloadCount += 1;
      return original_reload();
    };
    return true;
  });
  expect(seam_was_wrapped).toBe(true);

  await apply_provider_credential(page, credentialed_raster_descriptor);
  const reload_count_after_apply =
    await page.evaluate(() => window.__rasterReloadCount);
  await open_map_layers_panel(page);
  const credentialed_row = page.locator(
    `#${MAP_LAYER_BASE_ROW_IDENTIFIER_PREFIX}`
    + `${credentialed_raster_descriptor.provider_key}`);
  await credentialed_row.click();
  await expect
    .poll(() => page.evaluate(() => window.__rasterReloadCount), {
      timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS,
    })
    .toBeGreaterThan(reload_count_after_apply);
  const reload_count_after_selection =
    await page.evaluate(() => window.__rasterReloadCount);

  // Clearing the credential is a provider change too: the pooled payloads
  // from the provider that just went away must not survive it.
  await page.keyboard.press('Escape');
  await page.locator('#settings-control').click();
  await page.locator(
    `[data-provider-key="${credentialed_raster_descriptor.provider_key}"] `
    + '[data-provider-credential-clear]').click();
  await expect
    .poll(() => page.evaluate(() => window.__rasterReloadCount), {
      timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS,
    })
    .toBeGreaterThan(reload_count_after_selection);
});

test('a provider switch draws the switched-to provider from a cold pool', async ({
  page,
}) => {
  test.setTimeout(120_000);
  // The provider's committed sample is served for every address its route
  // matches, so the demo camera's neighbours draw too.
  await page.goto('/?advanced&useProviderFixtures');
  await waitForMapTestReady(page);
  const renderer_mode = await wait_for_renderer_mode(page);
  if (renderer_mode !== 'gpu') {
    test.skip(true, 'headless WebGPU unavailable; the frame statistics need the GPU path');
    return;
  }
  // Warm the pool from the demo's recorded fixture imagery.
  await expect
    .poll(() => page.evaluate(() => window.mapLastFrameStats()?.drawnTiles ?? 0), {
      timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS,
    })
    .toBeGreaterThan(0);

  const provider_request_paths = [];
  page.on('request', (request) => {
    const request_url = new URL(request.url());
    if (request_url.pathname.startsWith(`${CREDENTIALED_PROVIDER_ROOT}/`)) {
      provider_request_paths.push(request_url.pathname);
    }
  });
  // Deny the recorded fixture route so the switched-to provider must serve
  // the tiles; the dev server answers its committed sample same-origin.
  await page.route('**/tiles/**', (route) => route.fulfill({
    status: 404,
    contentType: 'text/plain',
    body: '',
  }));

  await apply_provider_credential(page, credentialed_raster_descriptor);
  await open_map_layers_panel(page);
  await page.locator(
    `#${MAP_LAYER_BASE_ROW_IDENTIFIER_PREFIX}`
    + `${credentialed_raster_descriptor.provider_key}`).click();

  // The reload drops the warm pool: the next frames fetch from the provider.
  await expect
    .poll(() => provider_request_paths.length, {
      timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS,
    })
    .toBeGreaterThan(0);
  const relative_path_prefix = `${CREDENTIALED_PROVIDER_ROOT}/`;
  const requested_relative_paths = provider_request_paths.map(
    (request_path) => request_path.slice(relative_path_prefix.length));
  expect(requested_relative_paths.some((relative_path) =>
    CREDENTIALED_RASTER_ROUTE_PATTERNS.some(
      (route_pattern) => route_pattern.test(relative_path)))).toBe(true);
  // The provider's bytes are drawn, not merely requested.
  await expect
    .poll(() => page.evaluate(() => window.mapLastFrameStats()?.drawnTiles ?? 0), {
      timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS,
    })
    .toBeGreaterThan(0);
});
