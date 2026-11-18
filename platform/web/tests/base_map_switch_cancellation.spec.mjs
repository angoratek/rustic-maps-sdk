// A raster selection made while a provider style document is still loading
// must win: the later document resolution may not overwrite the raster base.
// The style document and every glyph/sprite response are held behind a manual
// gate, the hosted style row is selected, the raster row is selected, and only
// then is the gate released. The canvas must report no installed vector source
// and settle on drawn raster tiles. Provider facts come from the shared
// registry; every byte is a committed fixture, so no external network is used.
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

const TEST_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const REPOSITORY_ROOT = path.resolve(TEST_DIRECTORY, '..', '..', '..');
const PROVIDERS_REGISTRY = JSON.parse(
  readFileSync(path.join(REPOSITORY_ROOT, 'platform', 'providers.json'), 'utf8'),
);

/** The first provider this build can register that publishes a hosted style. */
const HOSTED_VECTOR_DESCRIPTOR = PROVIDERS_REGISTRY.providers.find(
  (provider_descriptor) =>
    provider_descriptor.registration_available?.wasm === true
    && (provider_descriptor.vector_styles ?? []).some(
      (style_choice) => typeof style_choice.document_url === 'string'
        && style_choice.document_url.trim() !== '',
    ),
);
if (HOSTED_VECTOR_DESCRIPTOR === undefined) {
  throw new Error('the registry declares no registrable provider with a hosted style document');
}
/** The hosted style row this case selects. */
const HOSTED_STYLE_ID = HOSTED_VECTOR_DESCRIPTOR.vector_styles.find(
  (style_choice) => typeof style_choice.document_url === 'string'
    && style_choice.document_url.trim() !== '',
).style_id;
/** The first raster base descriptor, whose row switches the canvas back. */
const RASTER_BASE_DESCRIPTOR = PROVIDERS_REGISTRY.providers.find(
  (provider_descriptor) =>
    (provider_descriptor.fixture_routes ?? []).some(
      (fixture_route) => fixture_route.purpose === 'raster_tiles',
    ),
);
if (RASTER_BASE_DESCRIPTOR === undefined) {
  throw new Error('the registry declares no provider with a raster tile fixture route');
}

/** The page's own identifiers for the two base rows. */
const HOSTED_STYLE_ROW_IDENTIFIER = `map-layer-style-${HOSTED_STYLE_ID}`;
const RASTER_ROW_IDENTIFIER = `map-layer-base-${RASTER_BASE_DESCRIPTOR.provider_key}`;
/** The same-origin fixture prefix every held style resource arrives under. */
const PROVIDER_FIXTURE_PREFIX =
  `${PROVIDERS_REGISTRY.fixture_base_path}/${HOSTED_VECTOR_DESCRIPTOR.provider_key}`;

/** Renderer and settle budgets shared with the other GPU page suites. */
const RENDERER_READINESS_TIMEOUT_MILLISECONDS = 30_000;
const FRAME_SETTLE_TIMEOUT_MILLISECONDS = 30_000;
/** The placeholder credential the page's own settings seam accepts. */
const PLACEHOLDER_CREDENTIAL = 'test-placeholder-vector-key';

/** Content type a committed style-resource fixture is served with. */
function content_type_for_fixture(fixture_path) {
  if (fixture_path.endsWith('.json')) return 'application/json';
  if (fixture_path.endsWith('.png')) return 'image/png';
  return 'application/x-protobuf';
}

/** The committed fixture route matching one relative style-resource path. */
function fixture_route_for_relative_path(relative_path) {
  return HOSTED_VECTOR_DESCRIPTOR.fixture_routes.find(
    (fixture_route) =>
      (fixture_route.purpose === 'style_document'
        || fixture_route.purpose === 'glyphs'
        || fixture_route.purpose === 'sprite_sheet')
      && new RegExp(fixture_route.path_pattern).test(relative_path),
  );
}

test('a raster selection cancels an in-flight hosted style document install', async ({
  page,
}) => {
  test.setTimeout(120_000);

  let release_style_resources;
  const style_resources_gate = new Promise((resolve) => {
    release_style_resources = resolve;
  });
  let held_request_count = 0;
  // Every document, glyph and sprite response waits for the gate, so the
  // raster selection lands strictly between the install's first await and its
  // commit.
  await page.route(`**${PROVIDER_FIXTURE_PREFIX}/styles/**`, async (route) => {
    held_request_count += 1;
    await style_resources_gate;
    const request_pathname = new URL(route.request().url()).pathname;
    const relative_path = request_pathname.slice(PROVIDER_FIXTURE_PREFIX.length + 1);
    const fixture_route = fixture_route_for_relative_path(relative_path);
    if (fixture_route === undefined) {
      await route.fulfill({ status: 404, body: '' });
      return;
    }
    const fixture_path = path.join(REPOSITORY_ROOT, fixture_route.fixture);
    await route.fulfill({
      status: 200,
      contentType: content_type_for_fixture(fixture_route.fixture),
      body: readFileSync(fixture_path),
    });
  });

  await page.goto('/?advanced&useProviderFixtures');
  await waitForMapTestReady(page);
  await expect
    .poll(
      () => page.evaluate(
        () => (typeof window.mapRendererMode === 'function'
          ? window.mapRendererMode()
          : null)),
      { timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS },
    )
    .toBe('gpu');

  // Apply the hosted provider's credential through the page's own settings
  // seam, so its style rows become selectable and the fixture base URL is set.
  await page.locator('#settings-control').click();
  const credential_row = page.locator(
    '#provider-credential-rows '
    + `[data-provider-key="${HOSTED_VECTOR_DESCRIPTOR.provider_key}"]`);
  await credential_row.locator('[data-provider-credential-input]')
    .fill(PLACEHOLDER_CREDENTIAL);
  await credential_row.locator('[data-provider-credential-apply]').click();
  await expect(credential_row.locator('.provider-applied-chip')).toBeVisible();
  await page.keyboard.press('Escape');

  // Capture the install's own promise, so the assertions below run after the
  // superseded install has had every chance to overwrite the raster base.
  await page.evaluate(() => {
    const canvas = window.mapGpuCanvas();
    const original_install =
      canvas.setProviderVectorTileSourceFromDocument.bind(canvas);
    canvas.setProviderVectorTileSourceFromDocument = (...install_arguments) => {
      const install_promise = original_install(...install_arguments);
      window.hostedStyleInstallPromise = install_promise;
      return install_promise;
    };
  });

  await page.locator('#map-layers-control').click();
  const hosted_style_row = page.locator(`#${HOSTED_STYLE_ROW_IDENTIFIER}`);
  await expect(hosted_style_row).toBeEnabled({ timeout: 60_000 });
  await hosted_style_row.click();
  // The install is suspended on its style document.
  await expect.poll(() => held_request_count, {
    timeout: FRAME_SETTLE_TIMEOUT_MILLISECONDS,
  }).toBeGreaterThan(0);

  // The raster selection lands while the document install is suspended. Its
  // clear bumps the install generation, so the resolution below must be
  // discarded.
  await page.locator(`#${RASTER_ROW_IDENTIFIER}`).click();
  release_style_resources();
  // Wait for the superseded install to finish resolving before asking what
  // the canvas reports: the defect this case guards against is its late write.
  await page.evaluate(() => window.hostedStyleInstallPromise);

  await expect
    .poll(
      () => page.evaluate(
        () => window.mapGpuCanvas().vectorTileSourceStatus().isConfigured),
      { timeout: FRAME_SETTLE_TIMEOUT_MILLISECONDS },
    )
    .toBe(false);
  await expect
    .poll(
      () => page.evaluate(() => {
        window.mapRequestRenderForTest?.();
        const frame_statistics = window.mapLastFrameStats();
        return frame_statistics !== null
          && frame_statistics.requestedTiles > 0
          && frame_statistics.drawnTiles > 0
          && frame_statistics.pendingVectorTiles === 0
          && window.mapGpuCanvas().vectorTileSourceStatus().pendingVectorInstall === false;
      }),
      { timeout: FRAME_SETTLE_TIMEOUT_MILLISECONDS },
    )
    .toBe(true);

  const report = await page.evaluate(() => ({
    source_status: window.mapGpuCanvas().vectorTileSourceStatus(),
    frame_statistics: window.mapLastFrameStats(),
  }));
  expect(report.source_status.isConfigured).toBe(false);
  expect(report.source_status.pendingVectorInstall).toBe(false);
  expect(report.frame_statistics.pendingVectorTiles).toBe(0);
  expect(report.frame_statistics.drawnVectorTiles).toBe(0);
  expect(report.frame_statistics.drawnTiles).toBeGreaterThan(0);
  // The superseded install wrote no failure into the page's base-map status.
  await expect(page.locator('#map-layers-base-status')).toBeHidden();
});
