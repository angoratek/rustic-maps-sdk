// The canvas' recorded-fixture capability: a local page serves the
// same-origin `/tiles` fixtures and probes them before a registered live
// source, while a page that stages none asks its provider directly and issues
// no fixture request. A deployed page is simulated from the local origin with
// `?noRecordedTileFixtures`; every provider fact comes from the shared
// registry and every payload is a committed fixture served same-origin.
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test, waitForMapTestReady } from './network_guard.mjs';
import { MAP_LAYER_BASE_ROW_IDENTIFIER_PREFIX } from '../map_layers.mjs';

const TEST_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const REPOSITORY_ROOT = path.resolve(TEST_DIRECTORY, '..', '..', '..');
const RENDERER_READINESS_TIMEOUT_MILLISECONDS = 30_000;
const PROVIDER_REQUEST_TIMEOUT_MILLISECONDS = 30_000;
/** The same-origin route the recorded raster fixtures are served from. */
const RECORDED_FIXTURE_ROUTE_PREFIX = '/tiles/';

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
const PROVIDER_FIXTURE_ROOT =
  `${providers_registry.fixture_base_path}/${credentialed_raster_descriptor.provider_key}`;

/**
 * Records every raster-relevant request the page issues, in order. An entry is
 * a recorded-fixture probe or a provider request; anything else is ignored.
 */
function record_raster_requests(page) {
  const request_log = [];
  page.on('request', (request) => {
    const request_url = new URL(request.url());
    if (request_url.pathname.startsWith(RECORDED_FIXTURE_ROUTE_PREFIX)) {
      request_log.push({ kind: 'fixture', pathname: request_url.pathname });
    } else if (request_url.pathname.startsWith(`${PROVIDER_FIXTURE_ROOT}/`)) {
      request_log.push({ kind: 'provider', pathname: request_url.pathname });
    }
  });
  return request_log;
}

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

/** The panel row one raster descriptor is selected from. */
function raster_row(page, provider_descriptor) {
  return page.locator(
    `#${MAP_LAYER_BASE_ROW_IDENTIFIER_PREFIX}${provider_descriptor.provider_key}`);
}

/** Waits for the renderer's mode to answer and returns it. */
async function wait_for_renderer_mode(page) {
  await expect
    .poll(() => page.evaluate(() => (
      typeof window.mapRendererMode === 'function' ? window.mapRendererMode() : null)), {
      timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS,
    })
    .not.toBeNull();
  return page.evaluate(() => window.mapRendererMode());
}

/** Applies the credential and selects the credentialed raster base. */
async function select_credentialed_raster_base(page) {
  await apply_provider_credential(page, credentialed_raster_descriptor);
  await open_map_layers_panel(page);
  await raster_row(page, credentialed_raster_descriptor).click();
  await expect
    .poll(() => page.evaluate(() => window.mapRasterProviderKey()), {
      timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS,
    })
    .toBe(credentialed_raster_descriptor.provider_key);
}

test('a page that serves no recorded fixtures asks the raster provider directly', async ({
  page,
}) => {
  test.setTimeout(120_000);
  const request_log = record_raster_requests(page);
  // `?noRecordedTileFixtures` drives the deployed page's capability-off path
  // from the local origin: no `/tiles` route exists to answer a probe.
  await page.goto('/?advanced&useProviderFixtures&noRecordedTileFixtures');
  await waitForMapTestReady(page);
  const renderer_mode = await wait_for_renderer_mode(page);
  if (renderer_mode !== 'gpu') {
    test.skip(true, 'headless WebGPU unavailable; the raster fetch needs the GPU renderer');
    return;
  }

  await select_credentialed_raster_base(page);

  await expect
    .poll(
      () => request_log.filter((entry) => entry.kind === 'provider').length,
      { timeout: PROVIDER_REQUEST_TIMEOUT_MILLISECONDS },
    )
    .toBeGreaterThan(0);
  expect(
    request_log.filter((entry) => entry.kind === 'fixture'),
    'a page that serves no fixtures must not attempt a fixture request',
  ).toEqual([]);
  expect(request_log[0].kind).toBe('provider');
});

test('a local page still probes the recorded fixtures before its provider', async ({
  page,
}) => {
  test.setTimeout(120_000);
  const request_log = record_raster_requests(page);
  await page.goto('/?advanced&useProviderFixtures');
  await waitForMapTestReady(page);
  const renderer_mode = await wait_for_renderer_mode(page);
  if (renderer_mode !== 'gpu') {
    test.skip(true, 'headless WebGPU unavailable; the raster fetch needs the GPU renderer');
    return;
  }

  await select_credentialed_raster_base(page);

  await expect
    .poll(
      () => request_log.filter((entry) => entry.kind === 'fixture').length,
      { timeout: PROVIDER_REQUEST_TIMEOUT_MILLISECONDS },
    )
    .toBeGreaterThan(0);
  expect(request_log[0].kind).toBe('fixture');
});
