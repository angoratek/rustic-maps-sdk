// The zoom cross-fade contract: a camera crossing a tile level keeps the
// retired level's pooled tiles on screen over the new level and dissolves
// them once the new level is present, instead of clearing the viewport and
// filling it in tile by tile. The proof reads the canvas frame statistics
// (`zoomTransitionRunning` and `retainedTiles`) through a test-owned wasm
// canvas, so it measures the same state machine the page's settled pump
// drives rather than inferring it from pixels.
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

const FIELD_OF_VIEW_DEGREES = 60;
/**
 * A pose at the corner shared by recorded z16 tiles, framed at the recorded
 * z15 level: one tile spans 512 screen pixels at either level, so the
 * 512-pixel viewport covers a 2x2 block at both ends of the crossing.
 */
const START_CAMERA_POSE = {
  latitude: 38.57823196583314,
  longitude: -121.48681640625,
  zoom: 15,
  bearing: 0,
  pitch: 0,
};
/** Pause between settle frames, so fetched payloads can deliver. */
const SETTLE_FRAME_DELAY_MILLISECONDS = 30;
/** Upper bound on settle frames in one check. */
const SETTLE_FRAME_LIMIT = 60;
/** How long the fade drain waits before its last frame. */
const FADE_DRAIN_MILLISECONDS = 350;
/** Upper bound on frames drawn while the fade drains. */
const FADE_FRAME_LIMIT = 20;

test.beforeEach(async ({ page }) => {
  await page.goto('/?advanced');
  await waitForMapTestReady(page);
});

/**
 * Builds a test-owned wasm canvas and its frame helpers on `window`, or
 * reports the browser could not initialize WebGPU.
 */
async function require_test_canvas(page) {
  return page.evaluate(async () => {
    const canvas_edge_pixels = 512;
    const canvas_element = document.createElement('canvas');
    canvas_element.width = canvas_edge_pixels;
    canvas_element.height = canvas_edge_pixels;
    canvas_element.id = 'zoom-cross-fade-test-canvas';
    document.body.append(canvas_element);
    try {
      const canvas = new window.mapTest.MapView(
        canvas_element,
        window.mapTest.maps,
        window.mapTest.TileResolution.Standard,
      );
      const adapter = await canvas.initialize();
      const build_camera = (camera_pose) =>
        new window.mapTest.MapCamera(
          camera_pose.latitude,
          camera_pose.longitude,
          camera_pose.zoom,
          camera_pose.bearing,
          camera_pose.pitch,
        );
      window.zoomCrossFadeTest = {
        renderFrame: (camera_pose, field_of_view_degrees) =>
          canvas.renderFrame(build_camera(camera_pose), field_of_view_degrees),
        renderUntilSettled: async (
          camera_pose,
          field_of_view_degrees,
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
              field_of_view_degrees,
            );
            await new Promise((resolve) =>
              setTimeout(resolve, settle_frame_delay_milliseconds));
            if (
              frame_statistics.drawnTiles > 0 &&
              frame_statistics.missingTiles === 0 &&
              !frame_statistics.zoomTransitionRunning
            ) {
              break;
            }
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
  });
}

/** Draws one frame at a pose and drains the fade that follows it. */
async function draw_crossing_frame(page, camera_pose) {
  return page.evaluate(
    async ({ pose, field_of_view_degrees, drain_milliseconds, frame_limit }) => {
      const test_canvas = window.zoomCrossFadeTest;
      const first_frame = await test_canvas.renderFrame(pose, field_of_view_degrees);
      await new Promise((resolve) => setTimeout(resolve, drain_milliseconds));
      let last_frame = first_frame;
      for (let frame = 0; frame < frame_limit; frame += 1) {
        last_frame = await test_canvas.renderFrame(pose, field_of_view_degrees);
        if (!last_frame.zoomTransitionRunning) {
          break;
        }
        await new Promise((resolve) => setTimeout(resolve, 30));
      }
      return { first_frame, last_frame };
    },
    {
      pose: camera_pose,
      field_of_view_degrees: FIELD_OF_VIEW_DEGREES,
      drain_milliseconds: FADE_DRAIN_MILLISECONDS,
      frame_limit: FADE_FRAME_LIMIT,
    },
  );
}

test('a zoom in that crosses a tile level holds the retired level and dissolves it', async ({
  page,
}) => {
  const initialization = await require_test_canvas(page);
  test.skip(!initialization.ready, `WebGPU unavailable: ${initialization.error}`);
  const settled = await page.evaluate(
    async ({ pose, field_of_view_degrees, delay, limit }) =>
      window.zoomCrossFadeTest.renderUntilSettled(
        pose,
        field_of_view_degrees,
        delay,
        limit,
      ),
    {
      pose: START_CAMERA_POSE,
      field_of_view_degrees: FIELD_OF_VIEW_DEGREES,
      delay: SETTLE_FRAME_DELAY_MILLISECONDS,
      limit: SETTLE_FRAME_LIMIT,
    },
  );
  expect(settled.drawnTiles).toBeGreaterThan(0);

  const crossing = await draw_crossing_frame(page, {
    ...START_CAMERA_POSE,
    zoom: START_CAMERA_POSE.zoom + 1,
  });
  // The first frame after the level change keeps the retired level: its
  // pooled tiles draw over the new level, and the transition still owes
  // frames.
  expect(crossing.first_frame.zoomTransitionRunning).toBe(true);
  expect(crossing.first_frame.retainedTiles).toBeGreaterThan(0);
  // Once the fade drains, the new level owns the frame alone.
  expect(crossing.last_frame.zoomTransitionRunning).toBe(false);
  expect(crossing.last_frame.retainedTiles).toBe(0);
  expect(crossing.last_frame.drawnTiles).toBeGreaterThan(0);
  expect(crossing.last_frame.missingTiles).toBe(0);
});

test('a zoom out that crosses a tile level never clears to the background', async ({
  page,
}) => {
  const initialization = await require_test_canvas(page);
  test.skip(!initialization.ready, `WebGPU unavailable: ${initialization.error}`);
  const zoomed_in_pose = { ...START_CAMERA_POSE, zoom: START_CAMERA_POSE.zoom + 1 };
  await page.evaluate(
    async ({ pose, field_of_view_degrees, delay, limit }) =>
      window.zoomCrossFadeTest.renderUntilSettled(
        pose,
        field_of_view_degrees,
        delay,
        limit,
      ),
    {
      pose: zoomed_in_pose,
      field_of_view_degrees: FIELD_OF_VIEW_DEGREES,
      delay: SETTLE_FRAME_DELAY_MILLISECONDS,
      limit: SETTLE_FRAME_LIMIT,
    },
  );

  const crossing = await draw_crossing_frame(page, START_CAMERA_POSE);
  // Zooming out has no ancestor fallback to cover the new level's gaps, so
  // without the retained level the first frame would show the background.
  // The retired level draws over the whole viewport instead.
  expect(crossing.first_frame.retainedTiles).toBeGreaterThan(0);
  expect(crossing.first_frame.zoomTransitionRunning).toBe(true);
  expect(crossing.last_frame.zoomTransitionRunning).toBe(false);
  expect(crossing.last_frame.retainedTiles).toBe(0);
});
