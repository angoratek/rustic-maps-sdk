// Reloading the raster tile source: the wasm canvas serves the contract's
// `reloadTileSource()` member. The first case proves the façade clears the
// pooled raster state so the next frame re-plans its fetches. The second
// serves a changed tile source through an intercepted same-origin route and
// proves the frame after the reload draws the new bytes, never the previous
// source's pooled payload. No request leaves the harness origin.
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

const TEST_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const REPOSITORY_ROOT = path.resolve(TEST_DIRECTORY, '..', '..', '..');
const TILE_FIXTURE_DIRECTORY = path.join(
  REPOSITORY_ROOT,
  'crates',
  'rustic-maps-rendering',
  'fixtures',
  'render',
  'tiles',
);

/** The first tile source's committed payload. */
const FIRST_SOURCE_TILE_BYTES = readFileSync(
  path.join(TILE_FIXTURE_DIRECTORY, '14_2662_6286.png'),
);
/** The second tile source's committed payload (a different recorded tile). */
const SECOND_SOURCE_TILE_BYTES = readFileSync(
  path.join(TILE_FIXTURE_DIRECTORY, '14_2660_6284.png'),
);
/** Names the intercepted route records beside each served payload. */
const FIRST_SOURCE_NAME = 'first';
const SECOND_SOURCE_NAME = 'second';

/** Edge of the test canvas, in pixels. */
const TEST_CANVAS_EDGE_PIXELS = 256;
/** The playground's field of view (`FIELD_OF_VIEW_DEGREES` in index.html). */
const FIELD_OF_VIEW_DEGREES = 60;
/** A camera over the recorded Sacramento fixtures. */
const SACRAMENTO_CAMERA = {
  latitude: 38.5766,
  longitude: -121.4944,
  zoom: 14,
  bearing: 0,
  pitch: 0,
};
/** A zoom whose tile addresses differ from the page's own render loop. */
const INTERCEPTED_TILE_ZOOM_LEVEL = 12;
/** The demo camera at the intercepted zoom. */
const INTERCEPTED_CAMERA = { ...SACRAMENTO_CAMERA, zoom: INTERCEPTED_TILE_ZOOM_LEVEL };
/** Frame limit and settle delay for a fresh set of tiles. */
const SETTLE_FRAME_LIMIT = 40;
const SETTLE_FRAME_DELAY_MILLISECONDS = 30;

test.beforeEach(async ({ page }) => {
  await page.goto('/?advanced');
  await waitForMapTestReady(page);
});

/**
 * Builds a test-owned wasm canvas and its frame helpers on `window`. Skips the
 * test when the browser cannot initialize WebGPU (the documented degradation
 * path the other GPU suites assert).
 */
async function require_test_canvas(page) {
  const initialization = await page.evaluate(
    async ({ canvas_edge_pixels, settle_frame_delay_milliseconds }) => {
      const canvas_element = document.createElement('canvas');
      canvas_element.width = canvas_edge_pixels;
      canvas_element.height = canvas_edge_pixels;
      canvas_element.id = 'tile-source-reload-canvas';
      // Keep the probe surface in the viewport: a presented WebGPU frame is
      // what `drawImage` reads, and a display-hidden canvas never presents.
      canvas_element.style.position = 'fixed';
      canvas_element.style.left = '0';
      canvas_element.style.top = '0';
      document.body.append(canvas_element);
      try {
        const canvas = new window.mapTest.MapView(
          canvas_element,
          window.mapTest.maps,
          window.mapTest.TileResolution.Standard,
        );
        const adapter = await canvas.initialize();
        const renderFrame = async (camera_pose, field_of_view_degrees) => {
          // `renderFrame` consumes the camera by value, so the frame builds
          // its own and never frees it.
          const camera = new window.mapTest.MapCamera(
            camera_pose.latitude,
            camera_pose.longitude,
            camera_pose.zoom,
            camera_pose.bearing,
            camera_pose.pitch,
          );
          return canvas.renderFrame(camera, field_of_view_degrees);
        };
        window.tileSourceReloadTest = {
          canvas,
          canvasElement: canvas_element,
          renderFrame,
          renderSettledFrame: async (
            camera_pose,
            field_of_view_degrees,
            maximum_frames,
          ) => {
            let frame_statistics = null;
            for (let settle_frame = 0; settle_frame < maximum_frames; settle_frame += 1) {
              frame_statistics = await renderFrame(camera_pose, field_of_view_degrees);
              if (frame_statistics.drawnTiles > 0 && frame_statistics.missingTiles === 0) {
                break;
              }
              await new Promise((resolve) =>
                setTimeout(resolve, settle_frame_delay_milliseconds));
            }
            return frame_statistics;
          },
        };
        return { ready: true, adapter };
      } catch (initialization_failure) {
        return {
          ready: false,
          error: String(initialization_failure?.message ?? initialization_failure),
        };
      }
    },
    {
      canvas_edge_pixels: TEST_CANVAS_EDGE_PIXELS,
      settle_frame_delay_milliseconds: SETTLE_FRAME_DELAY_MILLISECONDS,
    },
  );
  if (!initialization.ready) {
    test.skip(
      true,
      `headless WebGPU unavailable in this browser; degradation asserted: ${initialization.error}`,
    );
    return false;
  }
  return true;
}

/**
 * Renders one frame over the settled camera and immediately probes the
 * canvas: `drawImage` reads a WebGPU canvas only while its presented frame is
 * fresh, so the probe follows the presenting frame in the same task.
 */
async function rendered_pixel_hash(page, camera_pose) {
  return page.evaluate(
    async ({ camera_pose, field_of_view_degrees }) => {
      const { canvasElement, renderFrame } = window.tileSourceReloadTest;
      const frame_statistics = await renderFrame(camera_pose, field_of_view_degrees);
      const probe_canvas = document.createElement('canvas');
      probe_canvas.width = canvasElement.width;
      probe_canvas.height = canvasElement.height;
      const probe_context = probe_canvas.getContext('2d');
      probe_context.drawImage(canvasElement, 0, 0);
      const { data } = probe_context.getImageData(
        0,
        0,
        probe_canvas.width,
        probe_canvas.height,
      );
      let signature = 0;
      for (let texel_index = 0; texel_index < data.length; texel_index += 4) {
        signature = (
          Math.imul(signature, 31)
          + data[texel_index]
          + Math.imul(data[texel_index + 1], 3)
          + Math.imul(data[texel_index + 2], 7)
        ) >>> 0;
      }
      return { signature, frame_statistics };
    },
    { camera_pose, field_of_view_degrees: FIELD_OF_VIEW_DEGREES },
  );
}

test('the wasm façade exposes reloadTileSource and the next frame re-plans its tiles', async ({
  page,
}) => {
  if (!(await require_test_canvas(page))) {
    return;
  }
  const member_kind = await page.evaluate(
    () => typeof window.mapTest.MapView.prototype.reloadTileSource,
  );
  expect(member_kind).toBe('function');

  const first_settle = await page.evaluate(
    async ({ camera_pose, field_of_view_degrees, settle_frame_limit }) =>
      window.tileSourceReloadTest.renderSettledFrame(
        camera_pose,
        field_of_view_degrees,
        settle_frame_limit,
      ),
    {
      camera_pose: SACRAMENTO_CAMERA,
      field_of_view_degrees: FIELD_OF_VIEW_DEGREES,
      settle_frame_limit: SETTLE_FRAME_LIMIT,
    },
  );
  expect(first_settle.requestedTiles).toBeGreaterThan(0);
  expect(first_settle.drawnTiles).toBe(first_settle.requestedTiles);
  expect(first_settle.missingTiles).toBe(0);

  // Without a reload the settled camera keeps drawing from the pool: the
  // same frame reports nothing missing.
  const repeated_frame = await page.evaluate(
    ({ camera_pose, field_of_view_degrees }) =>
      window.tileSourceReloadTest.renderFrame(camera_pose, field_of_view_degrees),
    { camera_pose: SACRAMENTO_CAMERA, field_of_view_degrees: FIELD_OF_VIEW_DEGREES },
  );
  expect(repeated_frame.missingTiles).toBe(0);

  await page.evaluate(() => window.tileSourceReloadTest.canvas.reloadTileSource());
  const frame_after_reload = await page.evaluate(
    ({ camera_pose, field_of_view_degrees }) =>
      window.tileSourceReloadTest.renderFrame(camera_pose, field_of_view_degrees),
    { camera_pose: SACRAMENTO_CAMERA, field_of_view_degrees: FIELD_OF_VIEW_DEGREES },
  );
  expect(frame_after_reload.requestedTiles).toBeGreaterThan(0);
  expect(
    frame_after_reload.missingTiles,
    'the reload drops the pooled tiles, so the next frame re-plans every fetch',
  ).toBe(frame_after_reload.requestedTiles);

  // The re-planned tiles arrive again from the same sources.
  const second_settle = await page.evaluate(
    async ({ camera_pose, field_of_view_degrees, settle_frame_limit }) =>
      window.tileSourceReloadTest.renderSettledFrame(
        camera_pose,
        field_of_view_degrees,
        settle_frame_limit,
      ),
    {
      camera_pose: SACRAMENTO_CAMERA,
      field_of_view_degrees: FIELD_OF_VIEW_DEGREES,
      settle_frame_limit: SETTLE_FRAME_LIMIT,
    },
  );
  expect(second_settle.drawnTiles).toBe(second_settle.requestedTiles);
});

test('a changed tile source serves new bytes after the reload', async ({ page }) => {
  // Every intercepted request serves the payload the test's current source
  // names, and records which source it came from. The route is same-origin:
  // the harness never contacts a tile service.
  const served_requests = [];
  let current_source = {
    name: FIRST_SOURCE_NAME,
    bytes: FIRST_SOURCE_TILE_BYTES,
  };
  await page.route('**/tiles/*.png', (route) => {
    served_requests.push({
      url: route.request().url(),
      source_name: current_source.name,
    });
    return route.fulfill({
      status: 200,
      contentType: 'image/png',
      // The reload's re-fetch must reach this route, never the HTTP cache:
      // the test asserts the changed payload was actually served again.
      headers: { 'cache-control': 'no-store' },
      body: current_source.bytes,
    });
  });

  if (!(await require_test_canvas(page))) {
    return;
  }
  const first_settle = await page.evaluate(
    async ({ camera_pose, field_of_view_degrees, settle_frame_limit }) =>
      window.tileSourceReloadTest.renderSettledFrame(
        camera_pose,
        field_of_view_degrees,
        settle_frame_limit,
      ),
    {
      camera_pose: INTERCEPTED_CAMERA,
      field_of_view_degrees: FIELD_OF_VIEW_DEGREES,
      settle_frame_limit: SETTLE_FRAME_LIMIT,
    },
  );
  expect(first_settle.drawnTiles).toBeGreaterThan(0);
  expect(first_settle.missingTiles).toBe(0);
  const first_pixels = await rendered_pixel_hash(page, INTERCEPTED_CAMERA);
  expect(first_pixels.signature).not.toBe(0);

  const intercepted_address_fragment = `/tiles/${INTERCEPTED_TILE_ZOOM_LEVEL}_`;
  const request_count_before_reload = served_requests.filter((served_request) =>
    served_request.url.includes(intercepted_address_fragment)).length;
  expect(request_count_before_reload).toBeGreaterThan(0);
  expect(
    served_requests
      .filter((served_request) => served_request.url.includes(intercepted_address_fragment))
      .every((served_request) => served_request.source_name === FIRST_SOURCE_NAME),
  ).toBe(true);

  // The source changes, then the page reloads the raster state: the same tile
  // addresses are requested again and answer with the second payload.
  current_source = {
    name: SECOND_SOURCE_NAME,
    bytes: SECOND_SOURCE_TILE_BYTES,
  };
  await page.evaluate(() => window.tileSourceReloadTest.canvas.reloadTileSource());
  const second_settle = await page.evaluate(
    async ({ camera_pose, field_of_view_degrees, settle_frame_limit }) =>
      window.tileSourceReloadTest.renderSettledFrame(
        camera_pose,
        field_of_view_degrees,
        settle_frame_limit,
      ),
    {
      camera_pose: INTERCEPTED_CAMERA,
      field_of_view_degrees: FIELD_OF_VIEW_DEGREES,
      settle_frame_limit: SETTLE_FRAME_LIMIT,
    },
  );
  expect(second_settle.drawnTiles).toBeGreaterThan(0);
  expect(second_settle.missingTiles).toBe(0);

  const requests_after_reload = served_requests.filter(
    (served_request) =>
      served_request.url.includes(intercepted_address_fragment)
      && served_request.source_name === SECOND_SOURCE_NAME,
  );
  expect(
    requests_after_reload.length,
    'the reload re-requests the pooled tile addresses from the changed source',
  ).toBeGreaterThan(0);

  const second_pixels = await rendered_pixel_hash(page, INTERCEPTED_CAMERA);
  expect(
    second_pixels.signature,
    'the frame after the reload draws the changed source bytes, not the pooled payload',
  ).not.toBe(first_pixels.signature);
});
