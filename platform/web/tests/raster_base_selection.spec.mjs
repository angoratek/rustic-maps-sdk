// Raster base selection in the layers panel: the fixture-backed keyless row,
// the credentialed rows that wait for their key, the provider a row installs,
// the serving mark, attribution and remembered choice that follow it, and the
// page-side selection epoch that keeps a superseded vector install from
// repainting a raster base. Every provider fact comes from the shared
// registry and every payload is a committed fixture served same-origin.
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test, waitForMapTestReady } from './network_guard.mjs';
import {
  MAP_LAYER_BASE_ROW_IDENTIFIER_PREFIX,
  MAP_LAYER_STYLE_ROW_IDENTIFIER_PREFIX,
} from '../map_layers.mjs';

const TEST_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const REPOSITORY_ROOT = path.resolve(TEST_DIRECTORY, '..', '..', '..');
const PREFERENCES_STORAGE_KEY = 'rustic-maps-demo-preferences';
const RENDERER_READINESS_TIMEOUT_MILLISECONDS = 30_000;
const SUPERSEDED_INSTALL_SETTLE_MILLISECONDS = 2_500;
const STYLE_DOCUMENT_DELAY_MILLISECONDS = 1_500;

const providers_registry = JSON.parse(
  readFileSync(path.join(REPOSITORY_ROOT, 'platform', 'providers.json'), 'utf8'),
);

/** The first descriptor with a complete committed demo tile set. */
const demo_raster_descriptor = providers_registry.providers.find(
  (provider_descriptor) => (provider_descriptor.fixture_routes ?? []).some(
    (fixture_route) => fixture_route.purpose === 'raster_tiles'
      && fixture_route.fixture_kind === 'tile_directory'));
/** The first credentialed raster descriptor without a complete demo tile set. */
const credentialed_raster_descriptor = providers_registry.providers.find(
  (provider_descriptor) =>
    (provider_descriptor.served_contracts ?? []).includes('raster_tiles')
    && provider_descriptor.registration_available?.wasm === true
    && typeof provider_descriptor.credential_label === 'string'
    && provider_descriptor.credential_label.trim() !== ''
    && !(provider_descriptor.fixture_routes ?? []).some(
      (fixture_route) => fixture_route.purpose === 'raster_tiles'
        && fixture_route.fixture_kind === 'tile_directory'));
/** A hosted vector family publishing a style document the page fetches. */
const hosted_vector_case = providers_registry.providers
  .flatMap((provider_descriptor) => {
    const style = (provider_descriptor.vector_styles ?? []).find(
      (style_choice) => typeof style_choice.document_url === 'string');
    return style === undefined ? [] : [{ provider_descriptor, style }];
  })[0];
/** The keyless live raster fallback the registry's URL template declares. */
const live_raster_descriptor = providers_registry.providers.find(
  (provider_descriptor) =>
    typeof provider_descriptor.raster_url_template === 'string'
    && provider_descriptor.raster_url_template.trim() !== '');

for (const required_descriptor of [
  demo_raster_descriptor,
  credentialed_raster_descriptor,
  hosted_vector_case,
  live_raster_descriptor,
]) {
  if (required_descriptor === undefined) {
    throw new Error(
      'the registry no longer declares the descriptors this case reads');
  }
}

/** The panel row one raster descriptor is selected from. */
function raster_row(page, provider_descriptor) {
  return page.locator(
    `#${MAP_LAYER_BASE_ROW_IDENTIFIER_PREFIX}${provider_descriptor.provider_key}`);
}

/** Opens the on-map layers panel. */
async function open_map_layers_panel(page) {
  await page.locator('#map-layers-control').click();
  await expect(page.locator('#map-layers-panel')).toBeVisible();
}

/** Applies one provider's credential through the settings surface. */
async function apply_provider_credential(page, provider_descriptor, {
  remember = false,
} = {}) {
  await page.locator('#settings-control').click();
  if (remember) {
    await page.locator('#remember-provider-credential').check();
  }
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

test('a credentialed raster row gates on its key while the fixture row serves', async ({
  page,
}) => {
  await page.goto('/?advanced&useProviderFixtures');
  await waitForMapTestReady(page);
  await open_map_layers_panel(page);

  const fixture_row = raster_row(page, demo_raster_descriptor);
  await expect(fixture_row).toBeEnabled();
  await expect(fixture_row).toHaveAttribute('aria-pressed', 'true');
  await expect(fixture_row).toContainText('Serving');
  await expect(fixture_row).not.toContainText('Needs');

  // A credentialed provider with one recorded sample is not demo-fixture
  // backed: its row waits for the key the registry declares.
  const credentialed_row = raster_row(page, credentialed_raster_descriptor);
  await expect(credentialed_row).toBeDisabled();
  await expect(credentialed_row).toContainText(
    `Needs ${credentialed_raster_descriptor.credential_label}`);
  expect(await page.evaluate(() => window.mapRasterProviderKey()))
    .toBe(demo_raster_descriptor.provider_key);

  // Applying the key enables the row but never changes what serves.
  await page.keyboard.press('Escape');
  await apply_provider_credential(page, credentialed_raster_descriptor);
  await open_map_layers_panel(page);
  await expect(credentialed_row).toBeEnabled();
  await expect(credentialed_row).not.toContainText('Needs');
  await expect(credentialed_row).toHaveAttribute('aria-pressed', 'false');
  await expect(fixture_row).toHaveAttribute('aria-pressed', 'true');
  await expect(fixture_row).toContainText('Serving');
  expect(await page.evaluate(() => window.mapRasterProviderKey()))
    .toBe(demo_raster_descriptor.provider_key);
  const raster_registration = await page.evaluate(() =>
    window.mapTest.providerRegistrations().find(
      (registration) => registration.contract === 'RasterTiles'));
  expect(raster_registration.registered).toBe(false);
});

test('selecting a raster row installs its provider and the panel, attribution and registration agree', async ({
  page,
}) => {
  await page.goto('/?advanced&useProviderFixtures');
  await waitForMapTestReady(page);
  await apply_provider_credential(page, credentialed_raster_descriptor);
  await open_map_layers_panel(page);

  const fixture_row = raster_row(page, demo_raster_descriptor);
  const credentialed_row = raster_row(page, credentialed_raster_descriptor);
  await credentialed_row.click();
  await expect
    .poll(() => page.evaluate(() => window.mapRasterProviderKey()), {
      timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS,
    })
    .toBe(credentialed_raster_descriptor.provider_key);
  await expect(credentialed_row).toHaveAttribute('aria-pressed', 'true');
  await expect(credentialed_row).toContainText('Serving');
  await expect(fixture_row).toHaveAttribute('aria-pressed', 'false');
  await expect(fixture_row).not.toContainText('Serving');
  const installed_registration = await page.evaluate(() =>
    window.mapTest.providerRegistrations().find(
      (registration) => registration.contract === 'RasterTiles'));
  expect(installed_registration.registered).toBe(true);
  expect(installed_registration.providerKey)
    .toBe(credentialed_raster_descriptor.provider_key);
  await expect(page.locator('#attribution-notice')).toContainText(
    credentialed_raster_descriptor.attribution_text);

  // Selecting the keyless fixture row clears the credentialed raster slot;
  // the other capabilities the key configured keep serving.
  await fixture_row.click();
  await expect
    .poll(() => page.evaluate(() => window.mapRasterProviderKey()), {
      timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS,
    })
    .toBe(demo_raster_descriptor.provider_key);
  await expect(fixture_row).toHaveAttribute('aria-pressed', 'true');
  await expect(fixture_row).toContainText('Serving');
  await expect(credentialed_row).toHaveAttribute('aria-pressed', 'false');
  const cleared_registration = await page.evaluate(() =>
    window.mapTest.providerRegistrations().find(
      (registration) => registration.contract === 'RasterTiles'));
  expect(cleared_registration.registered).toBe(false);
  const geocoding_registration = await page.evaluate(() =>
    window.mapTest.providerRegistrations().find(
      (registration) => registration.contract === 'Geocoding'));
  expect(geocoding_registration.registered).toBe(true);
  expect(geocoding_registration.providerKey)
    .toBe(credentialed_raster_descriptor.provider_key);
  await expect(page.locator('#attribution-notice')).toContainText(
    demo_raster_descriptor.attribution_text);
  await expect(page.locator('#attribution-notice')).not.toContainText(
    credentialed_raster_descriptor.attribution_text);
});

test('the remembered raster choice restores a non-default provider on reload', async ({
  page,
}) => {
  await page.goto('/?advanced&useProviderFixtures');
  await waitForMapTestReady(page);
  await apply_provider_credential(page, credentialed_raster_descriptor, {
    remember: true,
  });
  await open_map_layers_panel(page);
  await raster_row(page, credentialed_raster_descriptor).click();
  await expect
    .poll(() => page.evaluate(() => window.mapRasterProviderKey()), {
      timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS,
    })
    .toBe(credentialed_raster_descriptor.provider_key);

  const stored_record = await page.evaluate((storage_key) =>
    JSON.parse(window.localStorage.getItem(storage_key)), PREFERENCES_STORAGE_KEY);
  expect(stored_record.baseMap).toEqual({
    mode: 'raster',
    rasterProviderKey: credentialed_raster_descriptor.provider_key,
  });

  // A reload re-applies the remembered key and restores the same row.
  await page.reload();
  await waitForMapTestReady(page);
  await expect
    .poll(() => page.evaluate(() => window.mapRasterProviderKey()), {
      timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS,
    })
    .toBe(credentialed_raster_descriptor.provider_key);
  await open_map_layers_panel(page);
  const restored_row = raster_row(page, credentialed_raster_descriptor);
  await expect(restored_row).toHaveAttribute('aria-pressed', 'true');
  await expect(restored_row).toContainText('Serving');
  const restored_registration = await page.evaluate(() =>
    window.mapTest.providerRegistrations().find(
      (registration) => registration.contract === 'RasterTiles'));
  expect(restored_registration.registered).toBe(true);
  expect(restored_registration.providerKey)
    .toBe(credentialed_raster_descriptor.provider_key);
});

test('a superseded vector install cannot repaint the raster base it lost to', async ({
  page,
}) => {
  test.setTimeout(120_000);
  const { provider_descriptor, style } = hosted_vector_case;
  const style_document_fixture_path =
    `${providers_registry.fixture_base_path}/${provider_descriptor.provider_key}`
    + new URL(style.document_url).pathname;
  let release_style_document;
  const style_document_started = new Promise((resolve) => {
    release_style_document = resolve;
  });
  await page.route(
    (candidate) => candidate.pathname === style_document_fixture_path,
    async (route) => {
      release_style_document();
      await new Promise((resolve) =>
        setTimeout(resolve, STYLE_DOCUMENT_DELAY_MILLISECONDS));
      // The page may have superseded the install and cancelled the request;
      // the test only needs the delayed rejection to reach the page's own
      // callback, so a closed request is not a failure here.
      try {
        await route.fulfill({
          status: 500,
          contentType: 'text/plain',
          body: 'style document unavailable',
        });
      } catch {
        // The request was already gone.
      }
    },
  );

  await page.goto('/?advanced&useProviderFixtures');
  await waitForMapTestReady(page);
  const renderer_mode = await wait_for_renderer_mode(page);
  if (renderer_mode !== 'gpu') {
    test.skip(true, 'headless WebGPU unavailable; the vector base needs the GPU renderer');
    return;
  }
  await apply_provider_credential(page, provider_descriptor);
  await open_map_layers_panel(page);

  // Start the provider-document install, then pick the raster base while the
  // document is still in flight.
  const style_row = page.locator(
    `#${MAP_LAYER_STYLE_ROW_IDENTIFIER_PREFIX}${style.style_id}`);
  await style_row.click();
  await style_document_started;
  await raster_row(page, demo_raster_descriptor).click();
  await expect
    .poll(() => page.evaluate(() => window.mapTileMode()), {
      timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS,
    })
    .toBe('raster');

  // The superseded install's rejection must not reach the page's status line.
  await page.waitForTimeout(SUPERSEDED_INSTALL_SETTLE_MILLISECONDS);
  expect(await page.evaluate(() => window.mapVectorBaseStatus())).toBeNull();
  expect(await page.evaluate(() => window.mapTileMode())).toBe('raster');
  await expect(raster_row(page, demo_raster_descriptor))
    .toHaveAttribute('aria-pressed', 'true');
});

test('the keyless live fallback is a named row and its attribution names it', async ({
  page,
}) => {
  // `?liveTiles` is the local page's explicit opt-in to register the live
  // fallback, the same registration a deployed host performs at startup.
  await page.goto('/?advanced&useProviderFixtures&liveTiles');
  await waitForMapTestReady(page);
  await open_map_layers_panel(page);

  const live_row = raster_row(page, live_raster_descriptor);
  await expect(live_row).toBeEnabled();
  await expect(live_row).toHaveText(new RegExp(
    `${live_raster_descriptor.display_name} · Raster`));
  // The fixture-backed row serves the recorded camera, and the deployment's
  // live source covers the rest, so both attribution lines are present.
  await expect(page.locator('#attribution-notice')).toContainText(
    demo_raster_descriptor.attribution_text);
  await expect(page.locator('#attribution-notice')).toContainText(
    live_raster_descriptor.attribution_text);

  // Selecting the named fallback row makes it the serving base, and its
  // attribution names only the source that now serves.
  await live_row.click();
  await expect
    .poll(() => page.evaluate(() => window.mapRasterProviderKey()), {
      timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS,
    })
    .toBe(live_raster_descriptor.provider_key);
  await expect(live_row).toHaveAttribute('aria-pressed', 'true');
  await expect(live_row).toContainText('Serving');
  await expect(raster_row(page, demo_raster_descriptor))
    .toHaveAttribute('aria-pressed', 'false');
  await expect(page.locator('#attribution-notice')).toContainText(
    live_raster_descriptor.attribution_text);
});
