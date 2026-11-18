// The double click / double tap zoom contract of the demo's gesture surface
// (docs/ReferenceAppUX.md §5.2): one double click zooms exactly one level
// around the pointer, and the ground point under the pointer stays under it
// (the contract's focal zoom, the same member the wheel and the cluster use).
// A second double click on the new level proves the gesture repeats rather
// than latching. The gesture rides real input (Playwright's mouse dispatches
// pointer events), and the camera facts come from the page's live state and
// the wasm projection, so the page and the SDK cannot disagree about the
// result.
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

const FIELD_OF_VIEW_DEGREES = 60;
/** One double click's zoom step (the page's `DOUBLE_TAP_ZOOM_STEP`). */
const DOUBLE_TAP_ZOOM_STEP = 1.0;
/** Ground-hold tolerance at the focal point, in screen pixels. */
const GROUND_HOLD_TOLERANCE_PIXELS = 0.25;
/** The gesture point on the map, clear of the panels and the bottom dock. */
const GESTURE_POINT_PIXELS = [430, 200];

/** The page's camera state, read through its test seam. */
async function camera_state_of(page) {
  return page.evaluate(() => window.mapCameraState());
}

/** The ground coordinate under one page point, unprojected by the camera. */
async function ground_under_page_point(page, page_point_pixels) {
  return page.evaluate(
    ([page_point, field_of_view_degrees]) => {
      const state = window.mapCameraState();
      const live_camera = new window.mapTest.MapCamera(
        state.latitude,
        state.longitude,
        state.zoom,
        state.bearing,
        state.pitch,
      );
      return live_camera.screenToGroundCoordinate(
        page_point[0],
        page_point[1],
        window.innerWidth,
        window.innerHeight,
        field_of_view_degrees,
      );
    },
    [page_point_pixels, FIELD_OF_VIEW_DEGREES],
  );
}

/** Where one ground coordinate lands on screen under the live camera. */
async function screen_point_of(page, ground_coordinate) {
  return page.evaluate(
    ([ground, field_of_view_degrees]) => {
      const state = window.mapCameraState();
      const live_camera = new window.mapTest.MapCamera(
        state.latitude,
        state.longitude,
        state.zoom,
        state.bearing,
        state.pitch,
      );
      return live_camera.projectToScreen(
        ground.latitude,
        ground.longitude,
        window.innerWidth,
        window.innerHeight,
        field_of_view_degrees,
      );
    },
    [ground_coordinate, FIELD_OF_VIEW_DEGREES],
  );
}

test.beforeEach(async ({ page }) => {
  await page.goto('/?advanced');
  await waitForMapTestReady(page);
  await expect
    .poll(() => page.evaluate(() => window.mapCameraState()), { timeout: 20_000 })
    .not.toBeNull();
  // The gesture must land on the map canvas, not on a floating panel.
  const gesture_target_id = await page.evaluate(
    ([gesture_point]) =>
      document.elementFromPoint(gesture_point[0], gesture_point[1])?.id,
    [GESTURE_POINT_PIXELS],
  );
  expect(gesture_target_id).toBe('map-overlay');
});

test('a double click zooms one level around the pointer and holds its ground point', async ({
  page,
}) => {
  const camera_before = await camera_state_of(page);
  const ground_before = await ground_under_page_point(page, GESTURE_POINT_PIXELS);

  await page.mouse.dblclick(GESTURE_POINT_PIXELS[0], GESTURE_POINT_PIXELS[1]);

  await expect
    .poll(async () => (await camera_state_of(page)).zoom, { timeout: 10_000 })
    .toBeCloseTo(camera_before.zoom + DOUBLE_TAP_ZOOM_STEP, 5);

  // The focal contract: the world point that was under the pointer is still
  // under it after the zoom.
  const projected_after = await screen_point_of(page, ground_before);
  expect(Math.abs(projected_after.x - GESTURE_POINT_PIXELS[0]))
    .toBeLessThan(GROUND_HOLD_TOLERANCE_PIXELS);
  expect(Math.abs(projected_after.y - GESTURE_POINT_PIXELS[1]))
    .toBeLessThan(GROUND_HOLD_TOLERANCE_PIXELS);
});

test('a second double click zooms again from the new level', async ({ page }) => {
  const camera_before = await camera_state_of(page);
  await page.mouse.dblclick(GESTURE_POINT_PIXELS[0], GESTURE_POINT_PIXELS[1]);
  await expect
    .poll(async () => (await camera_state_of(page)).zoom, { timeout: 10_000 })
    .toBeCloseTo(camera_before.zoom + DOUBLE_TAP_ZOOM_STEP, 5);

  await page.mouse.dblclick(GESTURE_POINT_PIXELS[0], GESTURE_POINT_PIXELS[1]);
  await expect
    .poll(async () => (await camera_state_of(page)).zoom, { timeout: 10_000 })
    .toBeCloseTo(camera_before.zoom + 2 * DOUBLE_TAP_ZOOM_STEP, 5);
});
