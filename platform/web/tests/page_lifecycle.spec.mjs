// Page lifecycle: the canvases follow their layout box (a ResizeObserver, not
// only a window resize event), the display-density watcher catches a ratio
// change without a resize, and leaving the page frees the WebGPU canvas so its
// device, surface and cached payloads are released. The hidden-tab rule is
// asserted through the page's own seam: while the tab reports `hidden`, a
// scheduled frame stays queued instead of drawing.
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

test('the canvases follow their layout box and the page releases its renderer', async ({
  page,
}) => {
  await page.goto('/?advanced');
  await waitForMapTestReady(page);
  await page.waitForFunction(
    () =>
      window.mapRendererMode?.() === 'gpu' ||
      window.mapRendererMode?.() === 'overlay',
    null,
    { timeout: 30_000 },
  );

  const smaller_viewport = { width: 720, height: 540 };
  await page.setViewportSize(smaller_viewport);
  const expected_extent = await page.evaluate((viewport) => {
    const ratio = Math.max(1, window.devicePixelRatio || 1);
    return {
      width: Math.round(viewport.width * ratio),
      height: Math.round(viewport.height * ratio),
    };
  }, smaller_viewport);
  await expect
    .poll(
      () =>
        page.evaluate(() => {
          const canvas = document.getElementById('map-canvas');
          return { width: canvas.width, height: canvas.height };
        }),
      { timeout: 10_000 },
    )
    .toEqual(expected_extent);

  const had_gpu_canvas = await page.evaluate(
    () => window.mapGpuCanvas?.() !== null && window.mapGpuCanvas?.() !== undefined,
  );
  if (had_gpu_canvas) {
    await page.evaluate(() => window.dispatchEvent(new Event('pagehide')));
    await expect
      .poll(() => page.evaluate(() => window.mapGpuCanvas?.() ?? null), {
        timeout: 10_000,
      })
      .toBeNull();
  }
});

test('a hidden tab queues its frame instead of drawing it', async ({ page }) => {
  await page.goto('/?advanced');
  await waitForMapTestReady(page);
  await page.waitForFunction(
    () => typeof window.mapRendererMode?.() === 'string',
    null,
    { timeout: 30_000 },
  );
  test.skip(
    (await page.evaluate(() => window.mapRendererMode())) !== 'gpu',
    'the completed-frame counter only advances on the WebGPU renderer',
  );

  // Report the tab as hidden to the page's lifecycle listener, then ask for a
  // render through the same seam a control uses. The page must not draw while
  // hidden; becoming visible again draws the queued frame.
  const frames_before = await page.evaluate(() => {
    Object.defineProperty(document, 'visibilityState', {
      configurable: true,
      get: () => 'hidden',
    });
    document.dispatchEvent(new Event('visibilitychange'));
    return window.mapFramesDrawn();
  });

  // Wait for any frame already in flight to finish before sampling the count.
  await page.waitForFunction(
    (baseline) => window.mapFramesDrawn() >= baseline,
    frames_before,
    { timeout: 10_000 },
  );
  await page.evaluate(() => {
    window.mapRequestRenderForTest();
  });
  const frames_while_hidden = await page.evaluate(
    () => window.mapFramesDrawn(),
  );
  expect(frames_while_hidden).toBe(frames_before);

  await page.evaluate(() => {
    Object.defineProperty(document, 'visibilityState', {
      configurable: true,
      get: () => 'visible',
    });
    document.dispatchEvent(new Event('visibilitychange'));
  });
  await expect
    .poll(() => page.evaluate(() => window.mapFramesDrawn()), {
      timeout: 10_000,
    })
    .toBeGreaterThan(frames_while_hidden);
});
