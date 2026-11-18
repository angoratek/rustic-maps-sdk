// Browser label pass (M9 slice S3): the live demo on the Bright vector base
// draws at least one place name at the demo's zoom. The vector tile requests
// are intercepted and answered with the registry's committed vector fixture
// (the same tile the native place-label golden reads), and the glyph range is
// served same-origin by the dev server from the committed fixture, so the
// suite never touches an external network. Every provider fact (key,
// template, fixture) comes from the shared registry.
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test, waitForMapTestReady } from './network_guard.mjs';
import { HOST, PORT } from '../serve.mjs';

const TEST_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const REPOSITORY_ROOT = path.resolve(TEST_DIRECTORY, '..', '..', '..');

/** The shared provider registry the dev server also serves to the page. */
const PROVIDERS_REGISTRY = JSON.parse(
  readFileSync(path.join(REPOSITORY_ROOT, 'platform', 'providers.json'), 'utf8'),
);

/** The vector descriptor the suite drives: the one offering Bright. */
const VECTOR_DESCRIPTOR = PROVIDERS_REGISTRY.providers.find(
  (descriptor) => Array.isArray(descriptor.vector_styles)
    && descriptor.vector_styles.some((style) => style.style_id === 'bright-light'),
);
if (VECTOR_DESCRIPTOR === undefined) {
  throw new Error('the registry declares no vector provider with named styles');
}

/** The descriptor's committed vector tile fixture route. */
const VECTOR_TILE_FIXTURE_ROUTE = VECTOR_DESCRIPTOR.fixture_routes.find(
  (fixture_route) => fixture_route.purpose === 'vector_tiles',
);
if (VECTOR_TILE_FIXTURE_ROUTE === undefined) {
  throw new Error(`${VECTOR_DESCRIPTOR.provider_key} declares no vector_tiles fixture route`);
}
const RECORDED_VECTOR_TILE_BYTES = readFileSync(
  path.join(REPOSITORY_ROOT, VECTOR_TILE_FIXTURE_ROUTE.fixture),
);

/** The external tile host the page's registry template addresses. */
const VECTOR_TILE_HOST = new URL(
  VECTOR_DESCRIPTOR.url_template
    .replace('{z}', '0')
    .replace('{x}', '0')
    .replace('{y}', '0'),
).host;
/** The dev server the page under test is served from. */
const DEV_SERVER_HOST = `${HOST}:${PORT}`;

/** The Bright style id the descriptor offers and the suite selects. */
const BRIGHT_STYLE_ID = 'bright-light';
/** The demo camera's zoom (the recorded tile fixture's zoom). */
const DEMO_ZOOM_LEVEL = 14;
/** GPU initialization and the vector-base load budget. */
const RENDERER_READINESS_TIMEOUT_MILLISECONDS = 30_000;
/** The vector base's fetch/settle budget after the mode switch. */
const VECTOR_BASE_SETTLE_TIMEOUT_MILLISECONDS = 20_000;
/** The frame's label budget; mirrors `rustic_maps_rendering::labels::MAXIMUM_LABELS_PER_FRAME`. */
const MAXIMUM_LABELS_PER_FRAME = 32;

test('the Bright vector base draws a place name at the demo zoom', async ({ page }) => {
  const intercepted_tile_requests = [];
  const external_requests = [];
  await page.route(`https://${VECTOR_TILE_HOST}/**`, (route) => {
    intercepted_tile_requests.push(route.request().url());
    return route.fulfill({
      status: 200,
      contentType: 'application/vnd.mapbox-vector-tile',
      body: RECORDED_VECTOR_TILE_BYTES,
    });
  });
  // Any other external request is a fixture leak: the demo must read its
  // glyphs and tiles same-origin or through the intercepted provider host.
  await page.route('**/*', (route) => {
    const request_url = new URL(route.request().url());
    if (request_url.host === VECTOR_TILE_HOST) {
      return route.fallback();
    }
    if (request_url.host !== DEV_SERVER_HOST) {
      external_requests.push(route.request().url());
      return route.abort();
    }
    return route.fallback();
  });

  await page.goto('/?advanced');
  await waitForMapTestReady(page);
  await expect
    .poll(() => page.evaluate(() => (typeof window.mapRendererMode === 'function' ? window.mapRendererMode() : null)), {
      timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS,
    })
    .toBe('gpu');

  // The glyphs are the SDK's embedded default font, drawn inside the canvas
  // (M31's renderer-internal label ruling): the page fetches no glyph range.

  // The base-map flow lives in the floating layers panel, the single home on
  // every target.
  await page.click('#map-layers-control');
  await expect(page.locator('#map-layers-panel')).toBeVisible();

  // Switch the base layer to the descriptor's Bright style.
  await page.click(`#map-layer-style-${BRIGHT_STYLE_ID}`);
  await expect
    .poll(() => page.evaluate(() => window.mapTileMode()), {
      timeout: VECTOR_BASE_SETTLE_TIMEOUT_MILLISECONDS,
    })
    .toBe('vector');

  // The demo's zoom is the fixture's zoom; the camera never moves here.
  expect(await page.evaluate(() => window.mapLastFrameStats()?.tileZoomLevel))
    .toBeLessThanOrEqual(DEMO_ZOOM_LEVEL);

  // The frame's own statistics prove the label pass placed and drew text: at
  // least one label survived collision rejection and produced glyph triangles.
  await expect
    .poll(
      () => page.evaluate(() => window.mapLastFrameStats()?.labelCount ?? 0),
      { timeout: VECTOR_BASE_SETTLE_TIMEOUT_MILLISECONDS },
    )
    .toBeGreaterThan(0);
  await expect
    .poll(
      () => page.evaluate(() => window.mapLastFrameStats()?.labelGlyphTriangles ?? 0),
      { timeout: VECTOR_BASE_SETTLE_TIMEOUT_MILLISECONDS },
    )
    .toBeGreaterThan(0);
  const label_frame_stats = await page.evaluate(() => window.mapLastFrameStats());
  expect(
    label_frame_stats.labelCount,
    'the frame stops at the density ceiling no matter how many tiles carry labels',
  ).toBeLessThanOrEqual(MAXIMUM_LABELS_PER_FRAME);

  // The frame names the labels it painted: the tiles' place names project
  // inside the camera, so the drawn set carries the city and its districts
  // (the renderer's collision rule decides which of them survives).
  const label_texts = await page.evaluate(
    () => window.mapLastFrameStats()?.labelTexts ?? [],
  );
  expect(label_texts.length).toBeGreaterThan(0);
  expect(
    label_texts.some((text) => text.endsWith('Sacramento')),
    `a painted place name reads as the city or a district, got ${JSON.stringify(label_texts)}`,
  ).toBe(true);

  // The line-placed road names paint too, but the dense demo zoom is
  // budget-bound: its ranked place and POI names fill all 32 slots under the
  // class-before-rank order, so the unranked road names (last within their
  // class) cannot place there. One zoom band out the POI layers drop away and
  // the road names place deterministically; assert the line-label path and
  // its separate statistics there.
  await page.evaluate(() => {
    const zoom_control = document.getElementById('zoom-control');
    zoom_control.value = '13';
    zoom_control.dispatchEvent(new Event('input', { bubbles: true }));
    zoom_control.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await expect
    .poll(
      () => page.evaluate(() => window.mapLastFrameStats()?.tileZoomLevel ?? 0),
      { timeout: VECTOR_BASE_SETTLE_TIMEOUT_MILLISECONDS },
    )
    .toBe(13);
  await expect
    .poll(
      () => page.evaluate(() => window.mapLastFrameStats()?.lineLabelCount ?? 0),
      { timeout: VECTOR_BASE_SETTLE_TIMEOUT_MILLISECONDS },
    )
    .toBeGreaterThan(0);
  const line_label_texts = await page.evaluate(
    () => window.mapLastFrameStats()?.lineLabelTexts ?? [],
  );
  expect(line_label_texts.length).toBeGreaterThan(0);
  expect(
    line_label_texts.some((text) =>
      /(Street|Avenue|Boulevard|Road|Lane|Court|Way|Drive|Parkway|Circle|Freeway|Highway|Bridge)$/.test(
        text,
      ),
    ),
    `a painted road name reads as a street name, got ${JSON.stringify(line_label_texts)}`,
  ).toBe(true);

  // The base-map status reports the canvas-owned source's resolved tiles.
  await expect
    .poll(() => page.evaluate(() => window.mapVectorBaseStatus()), {
      timeout: VECTOR_BASE_SETTLE_TIMEOUT_MILLISECONDS,
    })
    .toContain('tiles drawn');

  expect(intercepted_tile_requests.length).toBeGreaterThan(0);
  expect(external_requests).toEqual([]);
});
