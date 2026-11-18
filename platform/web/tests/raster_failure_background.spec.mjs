// A raster frame whose every requested tile fails used to be indistinguishable
// from a settled black map: the statistics reported zero drawn tiles and
// nothing said whether the canvas was retrying or had given up. This case
// answers every same-origin tile request with a transient failure and asserts
// the frame's own retry state, so the page can explain a black frame instead
// of presenting it as settled.
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

/** Renderer and settle budgets shared with the other GPU page suites. */
const RENDERER_READINESS_TIMEOUT_MILLISECONDS = 30_000;
const RETRY_STATE_TIMEOUT_MILLISECONDS = 30_000;
/** The status a transiently failing tile route answers with. */
const TRANSIENT_FAILURE_STATUS = 500;

test('an all-tiles-fail raster frame reports that retries are in progress', async ({
  page,
}) => {
  test.setTimeout(90_000);
  const failed_tile_request_count = { value: 0 };
  await page.route('**/tiles/**', (route) => {
    failed_tile_request_count.value += 1;
    return route.fulfill({ status: TRANSIENT_FAILURE_STATUS, body: '' });
  });

  await page.goto('/?advanced');
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

  // The first frames list tiles and draw none; the settled pump renders again
  // after the failed fetches record their retry schedules. The retry flag is
  // the honest state that separates "nothing has arrived yet" from "this base
  // is waiting on retries".
  await expect
    .poll(
      () => page.evaluate(() => {
        window.mapRequestRenderForTest?.();
        const frame_statistics = window.mapLastFrameStats();
        return frame_statistics !== null
          && frame_statistics.requestedTiles > 0
          && frame_statistics.drawnTiles === 0
          && frame_statistics.rasterTilesRetrying === true;
      }),
      { timeout: RETRY_STATE_TIMEOUT_MILLISECONDS },
    )
    .toBe(true);

  const frame_statistics = await page.evaluate(() => window.mapLastFrameStats());
  expect(frame_statistics.requestedTiles).toBeGreaterThan(0);
  expect(frame_statistics.drawnTiles).toBe(0);
  expect(frame_statistics.missingTiles).toBe(
    frame_statistics.requestedTiles - frame_statistics.drawnTiles);
  expect(frame_statistics.rasterTilesRetrying).toBe(true);
  expect(failed_tile_request_count.value).toBeGreaterThan(0);
});
