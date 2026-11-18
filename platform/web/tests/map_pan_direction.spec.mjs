// The pan/zoom direction contract of the playground's gesture surface
// (docs/ReferenceAppUX.md §5.1/§5.2): map content follows the pointer — a
// pointer's ground point at gesture start must land under the pointer after
// the drag, so a rightward drag moves the content right (the camera center
// moves west) and a downward drag moves the content down (the camera center
// moves north). The wheel is the same convention's zoom face: a wheel-up
// (negative `deltaY`) zooms in, and a center-focal wheel never pans. These
// are real-input probes (mouse = pointer events) against the page's live
// camera state; the ground-point holds ride the wasm camera's own math.
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

const FIELD_OF_VIEW_DEGREES = 60;
// The page's wheel shaping (`ZOOM_WHEEL_SCALE` in index.html): a drift
// between the two sides fails the exact zoom-level assertion.
const WHEEL_ZOOM_SCALE = 0.0025;
// The drag distance in screen pixels: a hundred pixels at the launch
// camera is far above the longitude/latitude float noise in degrees.
const DRAG_PIXELS = 100;
const WHEEL_TICKS = 120;
// Content-follows-pointer hold tolerance: the wasm camera math permits
// sub-hundredth-pixel drift (the seam's own test tolerance), and the
// pose round trip through the readback state adds none at a drag this
// short.
const GROUND_HOLD_TOLERANCE_PIXELS = 1e-1;
// The drag's screen point (right of the controls panel's 280 px width,
// above the bottom dock) and the drag's step cadence.
const DRAG_START_PIXELS = [430, 200];
const DRAG_STEP_PIXELS = 4;
// The pan-move assert margin: at least this many degrees of longitude or
// latitude move per 100-pixel drag at the launch camera (zoom 14 spans a
// few hundred ground meters per 100 px).
const MINIMUM_PAN_MOVE_DEGREES = 1e-3;

test.beforeEach(async ({ page }) => {
  await page.goto('/?advanced');
  // The Rust-side `initializeSdk` promise gates every assertion.
  await waitForMapTestReady(page);
  await expect
    .poll(() => page.evaluate(() => window.mapCameraState()), {
      timeout: 20_000,
    })
    .not.toBeNull();
  // The drag target must be the map itself: the panels float above the
  // canvas and would swallow a misaimed drag.
  const drag_target_id = await page.evaluate(
    ([drag_start_pixels]) =>
      document.elementFromPoint(drag_start_pixels[0], drag_start_pixels[1])?.id,
    [DRAG_START_PIXELS],
  );
  expect(drag_target_id).toBe('map-overlay');
});

test('a rightward drag moves the content right (the camera center wests)', async ({
  page,
}) => {
  const pointer_start = DRAG_START_PIXELS;
  const pointer_end = [pointer_start[0] + DRAG_PIXELS, pointer_start[1]];
  const camera_before = await camera_state_of(page);
  const ground_under_pointer_start = await ground_under_page_point(
    page,
    pointer_start,
  );

  await drag_screen_segment(page, pointer_start, pointer_end);

  // The camera center moved west: the content moved east — right, with the
  // pointer — revealed on the now-open west side.
  const camera_after = await camera_state_of(page);
  expect(camera_after.longitude).toBeLessThan(
    camera_before.longitude - MINIMUM_PAN_MOVE_DEGREES,
  );
  // The exact convention: the pointer start's ground point landed under
  // the pointer end.
  await assert_ground_point_lands_under(page, ground_under_pointer_start, pointer_end);
  // Zoom, bearing and pitch ride through unchanged.
  expect(camera_after.zoom).toBeCloseTo(camera_before.zoom, 6);
  expect(camera_after.bearing).toBeCloseTo(camera_before.bearing, 6);
  expect(camera_after.pitch).toBeCloseTo(camera_before.pitch, 6);
});

test('a downward drag moves the content down (the camera center norths)', async ({
  page,
}) => {
  const pointer_start = DRAG_START_PIXELS;
  const pointer_end = [pointer_start[0], pointer_start[1] + DRAG_PIXELS];
  const camera_before = await camera_state_of(page);
  const ground_under_pointer_start = await ground_under_page_point(
    page,
    pointer_start,
  );

  await drag_screen_segment(page, pointer_start, pointer_end);

  // The camera center moved north: the content moved south — down, with
  // the pointer.
  const camera_after = await camera_state_of(page);
  expect(camera_after.latitude).toBeGreaterThan(
    camera_before.latitude + MINIMUM_PAN_MOVE_DEGREES,
  );
  // The exact convention, on the vertical axis this time.
  await assert_ground_point_lands_under(page, ground_under_pointer_start, pointer_end);
});

test('wheel up zooms in, wheel down zooms out, and the center-focal wheel pans nowhere', async ({
  page,
}) => {
  const camera_before = await camera_state_of(page);
  const zoom_before = camera_before.zoom;
  await page.mouse.move(DRAG_START_PIXELS[0], DRAG_START_PIXELS[1]);

  await page.mouse.wheel(0, -WHEEL_TICKS);
  // A wheel-up's negative `deltaY` must raise the zoom (content grows —
  // zoom in), shaped by the page's wheel scale.
  await expect
    .poll(() => page.evaluate(() => window.mapCameraState().zoom), {
      timeout: 3_000,
    })
    .toBeCloseTo(zoom_before + WHEEL_TICKS * WHEEL_ZOOM_SCALE, 2);

  await page.mouse.wheel(0, WHEEL_TICKS);
  await expect
    .poll(() => page.evaluate(() => window.mapCameraState().zoom), {
      timeout: 3_000,
    })
    .toBeCloseTo(zoom_before, 2);
  // The wheel's focal point is the viewport center: reversing the tick
  // must undo the zoom and pan nowhere — a drifted center would betray a
  // focal mismatch the direction contract forbids.
  const camera_after = await camera_state_of(page);
  expect(camera_after.latitude).toBeCloseTo(
    camera_before.latitude,
    4,
  );
  expect(camera_after.longitude).toBeCloseTo(
    camera_before.longitude,
    4,
  );
});

// --- The suite's helpers ---

/** The page's live camera pose (the readback the UX tests consume). */
async function camera_state_of(page) {
  return page.evaluate(() => window.mapCameraState());
}

/**
 * Builds the live pose's exact camera in wasm and unprojects a screen
 * point through it (the page's own viewport and field of view).
 */
async function ground_under_page_point(page, screen_point_pixels) {
  return page.evaluate(
    ([screen_point, field_of_view_degrees]) => {
      const overlay_canvas = document.getElementById('map-overlay');
      const camera_state = window.mapCameraState();
      const camera = new window.mapTest.MapCamera(
        camera_state.latitude,
        camera_state.longitude,
        camera_state.zoom,
        camera_state.bearing,
        camera_state.pitch,
      );
      try {
        return camera.screenToGroundCoordinate(
          screen_point[0],
          screen_point[1],
          overlay_canvas.width,
          overlay_canvas.height,
          field_of_view_degrees,
        );
      } finally {
        camera.free();
      }
    },
    [screen_point_pixels, FIELD_OF_VIEW_DEGREES],
  );
}

/**
 * Projects a ground point through the current pose's camera and asserts it
 * sits under the given screen point — the content-follows-pointer hold.
 */
async function assert_ground_point_lands_under(page, ground, screen_point) {
  const screen_offset = await page.evaluate(
    ([ground, pointer_end, field_of_view_degrees]) => {
      const overlay_canvas = document.getElementById('map-overlay');
      const camera_state = window.mapCameraState();
      const camera = new window.mapTest.MapCamera(
        camera_state.latitude,
        camera_state.longitude,
        camera_state.zoom,
        camera_state.bearing,
        camera_state.pitch,
      );
      try {
        const projected = camera.projectToScreen(
          ground.latitude,
          ground.longitude,
          overlay_canvas.width,
          overlay_canvas.height,
          field_of_view_degrees,
        );
        return {
          dx: projected.x - pointer_end[0],
          dy: projected.y - pointer_end[1],
        };
      } finally {
        camera.free();
      }
    },
    [ground, screen_point, FIELD_OF_VIEW_DEGREES],
  );
  expect(Math.hypot(screen_offset.dx, screen_offset.dy)).toBeLessThan(
    GROUND_HOLD_TOLERANCE_PIXELS,
  );
}

/**
 * Drags the mouse along a screen segment in steps (the page computes each
 * incremental `clientX` delta on its own pointer bookkeeping).
 */
async function drag_screen_segment(page, pointer_start, pointer_end) {
  await page.mouse.move(pointer_start[0], pointer_start[1]);
  await page.mouse.down();
  const steps = Math.ceil(
    Math.max(
      Math.abs(pointer_end[0] - pointer_start[0]),
      Math.abs(pointer_end[1] - pointer_start[1]),
    ) / DRAG_STEP_PIXELS,
  );
  for (let step_index = 1; step_index <= steps; step_index += 1) {
    const interpolation = step_index / steps;
    await page.mouse.move(
      pointer_start[0] + (pointer_end[0] - pointer_start[0]) * interpolation,
      pointer_start[1] + (pointer_end[1] - pointer_start[1]) * interpolation,
    );
  }
  await page.mouse.up();
}