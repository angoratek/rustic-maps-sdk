// The styled vector base's zoom cross-fade contract: crossing a tile level
// keeps the retired level's geometry on screen over the new level and
// dissolves it once the new level is present, so a zoom-out never clears to
// the background. The proof reads the canvas frame statistics
// (`vectorZoomTransitionRunning`, `vectorRetainedTiles` and
// `overlayTriangles`) through a test-owned wasm canvas whose vector source is
// the committed golden tile served same-origin, so it measures the same state
// machine the page drives rather than inferring it from pixels.
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

const FIELD_OF_VIEW_DEGREES = 60;
/**
 * The committed golden vector tile's center (z14/2661/6286): at z14.4 the
 * camera requests that tile's own level and the tile is cached, and at z13.4
 * it requests the level above, whose address the fixture route does not
 * serve.
 */
const CAMERA_POSE = {
  latitude: 38.57393751557591,
  longitude: -121.519775390625,
  bearing: 0,
  pitch: 0,
};
/** The zoom whose rounded tile level (14) the fixture serves. */
const SERVED_CAMERA_ZOOM = 14.4;
/** The zoom whose rounded tile level (13) the fixture does not serve. */
const RETIRED_CAMERA_ZOOM = 13.4;
/** The same-origin route `serve.mjs` resolves through the provider registry. */
const GOLDEN_VECTOR_TILE_TEMPLATE_PATH = '/golden-vector/{z}/{x}/{y}.pbf';
/** Pause between settle frames, so fetched payloads can deliver. */
const SETTLE_FRAME_DELAY_MILLISECONDS = 30;
/** Upper bound on settle frames in one check. */
const SETTLE_FRAME_LIMIT = 80;
/** How long the fade drain waits before its last frame. */
const FADE_DRAIN_MILLISECONDS = 1400;
/** Upper bound on frames drawn while the fade drains. */
const FADE_FRAME_LIMIT = 60;

test.beforeEach(async ({ page }) => {
  await page.goto('/?advanced');
  await waitForMapTestReady(page);
});

/**
 * Builds a test-owned wasm canvas over the committed golden vector tile and
 * its frame helpers on `window`, or reports that WebGPU could not initialize.
 */
async function require_vector_test_canvas(page) {
  return page.evaluate(
    async ([templatePath, fieldOfViewDegrees]) => {
      const canvas_edge_pixels = 512;
      const canvas_element = document.createElement('canvas');
      canvas_element.width = canvas_edge_pixels;
      canvas_element.height = canvas_edge_pixels;
      canvas_element.id = 'vector-zoom-cross-fade-test-canvas';
      document.body.append(canvas_element);
      try {
        const canvas = new window.mapTest.MapView(
          canvas_element,
          window.mapTest.TileResolution.Standard,
        );
        const adapter = await canvas.initialize();
        canvas.setVectorTileSource({
          urlTemplate: `${window.location.origin}${templatePath}`,
          sourceName: 'golden-vector',
          attributionText: 'Committed golden vector tile',
          styleId: 'bright-light',
          minimumZoomLevel: 0,
          maximumZoomLevel: 14,
        });
        const build_camera = (camera_pose) =>
          new window.mapTest.MapCamera(
            camera_pose.latitude,
            camera_pose.longitude,
            camera_pose.zoom,
            camera_pose.bearing,
            camera_pose.pitch,
          );
        window.vectorZoomCrossFadeTest = {
          renderFrame: (camera_pose) =>
            canvas.renderFrame(build_camera(camera_pose), fieldOfViewDegrees),
          renderUntilSettled: async (
            camera_pose,
            settle_frame_delay_milliseconds,
            settle_frame_limit,
          ) => {
            let frame_statistics = null;
            for (
              let settle_frame = 0;
              settle_frame < settle_frame_limit;
              settle_frame += 1
            ) {
              frame_statistics = await canvas.renderFrame(
                build_camera(camera_pose),
                fieldOfViewDegrees,
              );
              await new Promise((resolve) =>
                setTimeout(resolve, settle_frame_delay_milliseconds));
              if (
                frame_statistics.vectorBaseTimeToFirstDrawnTileMilliseconds
                  !== null
                && !frame_statistics.vectorZoomTransitionRunning
                && frame_statistics.overlayTriangles > 0
              ) {
                break;
              }
            }
            return frame_statistics;
          },
          drainFade: async (
            camera_pose,
            drain_milliseconds,
            frame_limit,
          ) => {
            await new Promise((resolve) => setTimeout(resolve, drain_milliseconds));
            let last_frame = null;
            for (let frame = 0; frame < frame_limit; frame += 1) {
              last_frame = await canvas.renderFrame(
                build_camera(camera_pose),
                fieldOfViewDegrees,
              );
              if (!last_frame.vectorZoomTransitionRunning) {
                break;
              }
              await new Promise((resolve) => setTimeout(resolve, 30));
            }
            return last_frame;
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
    [GOLDEN_VECTOR_TILE_TEMPLATE_PATH, FIELD_OF_VIEW_DEGREES],
  );
}

/** Draws one frame at a pose and drains the vector fade that follows it. */
async function draw_crossing_frame(page, camera_pose) {
  return page.evaluate(
    async ({ pose, drain_milliseconds, frame_limit }) => {
      const test_canvas = window.vectorZoomCrossFadeTest;
      const first_frame = await test_canvas.renderFrame(pose);
      const last_frame = await test_canvas.drainFade(
        pose,
        drain_milliseconds,
        frame_limit,
      );
      return { first_frame, last_frame };
    },
    {
      pose: camera_pose,
      drain_milliseconds: FADE_DRAIN_MILLISECONDS,
      frame_limit: FADE_FRAME_LIMIT,
    },
  );
}

test('a vector zoom-out holds the retired level and never clears to the background', async ({
  page,
}) => {
  const initialization = await require_vector_test_canvas(page);
  test.skip(!initialization.ready, `WebGPU unavailable: ${initialization.error}`);
  const settled = await page.evaluate(
    async ({ pose, zoom, delay, limit }) =>
      window.vectorZoomCrossFadeTest.renderUntilSettled(
        { ...pose, zoom },
        delay,
        limit,
      ),
    {
      pose: CAMERA_POSE,
      zoom: SERVED_CAMERA_ZOOM,
      delay: SETTLE_FRAME_DELAY_MILLISECONDS,
      limit: SETTLE_FRAME_LIMIT,
    },
  );
  expect(settled.overlayTriangles).toBeGreaterThan(0);

  const crossing = await draw_crossing_frame(page, {
    ...CAMERA_POSE,
    zoom: RETIRED_CAMERA_ZOOM,
  });
  // The level above has no fixture, so without the retained level the first
  // frame would be the style background alone. The retired z14 geometry is
  // drawn instead, at the settled frame's own triangle count.
  expect(crossing.first_frame.vectorZoomTransitionRunning).toBe(true);
  expect(crossing.first_frame.vectorRetainedTiles).toBeGreaterThan(0);
  expect(crossing.first_frame.overlayTriangles).toBe(settled.overlayTriangles);
  // Once the hold and fade drain, the retired level is gone.
  expect(crossing.last_frame.vectorZoomTransitionRunning).toBe(false);
  expect(crossing.last_frame.vectorRetainedTiles).toBe(0);
});

test('a vector zoom back across the level draws the new level immediately', async ({
  page,
}) => {
  const initialization = await require_vector_test_canvas(page);
  test.skip(!initialization.ready, `WebGPU unavailable: ${initialization.error}`);
  // Settle at the served level first, so its tile is cached when the camera
  // returns to it.
  await page.evaluate(
    async ({ pose, zoom, delay, limit }) =>
      window.vectorZoomCrossFadeTest.renderUntilSettled(
        { ...pose, zoom },
        delay,
        limit,
      ),
    {
      pose: CAMERA_POSE,
      zoom: SERVED_CAMERA_ZOOM,
      delay: SETTLE_FRAME_DELAY_MILLISECONDS,
      limit: SETTLE_FRAME_LIMIT,
    },
  );
  // Cross to the unserved level and draw one frame, so a retained level with
  // no geometry is in hand, then cross back.
  await page.evaluate(
    ({ pose }) => window.vectorZoomCrossFadeTest.renderFrame(pose),
    { pose: { ...CAMERA_POSE, zoom: RETIRED_CAMERA_ZOOM } },
  );
  const returned = await page.evaluate(
    ({ pose }) => window.vectorZoomCrossFadeTest.renderFrame(pose),
    { pose: { ...CAMERA_POSE, zoom: SERVED_CAMERA_ZOOM } },
  );
  // The requested level's tile is cached, so it draws on the first frame; the
  // retired level above holds no geometry to draw over it.
  expect(returned.overlayTriangles).toBeGreaterThan(0);
  expect(returned.vectorRetainedTiles).toBe(0);
  expect(returned.vectorZoomTransitionRunning).toBe(true);
});
