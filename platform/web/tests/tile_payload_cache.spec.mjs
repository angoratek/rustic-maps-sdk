// The wasm tile payload cache seam (docs/ReferenceAppUX.md §11): the page's
// IndexedDB-backed implementation persists the fetched encoded payloads, and
// the fetch path serves a cached payload without asking the fixture route
// again. The default seam stores nothing, so every other suite keeps the
// plain fetch behaviour; this file opts in with `?tileCache`.
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

/** Time the first frame and the cache fill may take under software rendering. */
const CACHE_SETTLE_TIMEOUT_MILLISECONDS = 30_000;

/**
 * The same-origin tile fixture paths requested since the last reset.
 * @param {import('@playwright/test').Page} page
 * @returns {{ paths: string[], reset: () => void }}
 */
function record_tile_requests(page) {
  const paths = [];
  page.on('request', (request) => {
    const path = new URL(request.url()).pathname;
    if (path.startsWith('/tiles/')) {
      paths.push(path);
    }
  });
  return { paths, reset: () => { paths.length = 0; } };
}

test('a persisted tile payload is served from IndexedDB without a refetch', async ({
  page,
}) => {
  const tile_requests = record_tile_requests(page);

  await page.goto('/?tileCache');
  await waitForMapTestReady(page);
  await expect
    .poll(() => page.evaluate(() => window.mapFramesDrawn()), {
      timeout: CACHE_SETTLE_TIMEOUT_MILLISECONDS,
    })
    .toBeGreaterThan(0);
  await expect
    .poll(() => page.evaluate(() => window.tilePayloadCache.count()), {
      timeout: CACHE_SETTLE_TIMEOUT_MILLISECONDS,
    })
    .toBeGreaterThan(0);

  const first_load_tile_paths = [...new Set(tile_requests.paths)];
  expect(first_load_tile_paths.length).toBeGreaterThan(0);

  // Reopen the same origin with every fixture route refused: the persisted
  // payloads must draw the map without requesting those addresses again.
  await page.route('**/tiles/**', (route) =>
    route.fulfill({ status: 404, body: '' }),
  );
  tile_requests.reset();
  await page.goto('/?tileCache');
  await waitForMapTestReady(page);
  await expect
    .poll(
      () => page.evaluate(() => window.mapLastFrameStats()?.drawnTiles ?? 0),
      { timeout: CACHE_SETTLE_TIMEOUT_MILLISECONDS },
    )
    .toBeGreaterThan(0);

  const refetched_paths = [...new Set(tile_requests.paths)].filter((path) =>
    first_load_tile_paths.includes(path),
  );
  expect(
    refetched_paths,
    'a persisted address must come from the cache, not the fixture route',
  ).toEqual([]);
});

test('clearing the cache drops the persisted payloads', async ({ page }) => {
  await page.goto('/?tileCache');
  await waitForMapTestReady(page);
  await expect
    .poll(() => page.evaluate(() => window.tilePayloadCache.count()), {
      timeout: CACHE_SETTLE_TIMEOUT_MILLISECONDS,
    })
    .toBeGreaterThan(0);

  await page.evaluate(async () => {
    await window.tilePayloadCache.clear();
    window.mapTest.clearTilePayloadCache();
  });

  expect(await page.evaluate(() => window.tilePayloadCache.count())).toBe(0);
});
