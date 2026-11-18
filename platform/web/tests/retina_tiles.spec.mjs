// High-density displays: the demo's tile resolution follows the device pixel
// ratio, and a moderately dense backing store (a 1.5x display, or a 2x screen
// at reduced browser zoom) still asks for the double-texel fixtures. The
// threshold is the one the wasm surface exports, so the page and the canvas
// cannot disagree.
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

test.use({ deviceScaleFactor: 1.6 });

test('a moderately dense display asks for the double-resolution tiles', async ({
  page,
}) => {
  const requested_tile_urls = [];
  page.on('request', (request) => {
    const request_url = new URL(request.url());
    if (request_url.pathname.startsWith('/tiles/')) {
      requested_tile_urls.push(request_url.pathname);
    }
  });
  await page.goto('/?advanced');
  await waitForMapTestReady(page);
  await page.waitForFunction(
    () =>
      window.mapRendererMode?.() === 'gpu' ||
      window.mapRendererMode?.() === 'overlay',
    null,
    { timeout: 30_000 },
  );
  const renderer_mode = await page.evaluate(() => window.mapRendererMode());
  if (renderer_mode !== 'gpu') {
    test.skip(true, 'headless WebGPU unavailable; the fixture prefetch needs the GPU renderer');
    return;
  }
  await expect
    .poll(
      () => requested_tile_urls.some((path_name) => path_name.endsWith('@2x.png')),
      { timeout: 20_000 },
    )
    .toBe(true);
});
