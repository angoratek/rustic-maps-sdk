// Round-11 reference-app parity checks of the web playground's control
// cluster, location indicator, eased camera moves, follow demo and
// diagnostics readout (docs/ReferenceAppUX.md §4.2/§5.3/§4.1). The expected
// values import the same platform-glue module the page drives, so page and
// tests cannot drift on a token.
import { expect, test, waitForMapTestReady } from './network_guard.mjs';
import { HOST, PORT } from '../serve.mjs';

/** The origin the harness serves and the browser permission grants name. */
const HARNESS_ORIGIN = `http://${HOST}:${PORT}`;

import {
  EASED_CAMERA_MOVE_DURATION_MILLISECONDS,
  FOLLOW_BEARING_CHASE_TIME_CONSTANT_MILLISECONDS,
  NAVIGATION_FOLLOW_PITCH_3D_DEGREES,
  CourseMedianSmoother,
  ExponentialValueChase,
  SmoothstepTimedEase,
  follow_zoom_toward_trip_end,
  polyline_fix_at_travelled_meters,
  polyline_length_meters,
  scale_bar_distance_label_text,
  select_scale_bar_scale,
} from '../map_controls.mjs';

const FIELD_OF_VIEW_DEGREES = 60;
const INITIAL_ZOOM_LEVEL = 14;
const ZOOM_STEP_LEVELS = 1;

// Timeout of the frame-pump awaits: the eased moves run EASED_CAMERA_MOVE…
// plus margin; the follow demo's fix interval is 250 ms.
const PUMP_IDLE_TIMEOUT_MILLISECONDS = 3000;
const FOLLOW_DEMO_AWAIT_MILLISECONDS = 3000;
const RENDERER_READINESS_TIMEOUT_MILLISECONDS = 20000;
// How far the guided indicator may lie from the demo route's line: one
// playback step's worth of ground, which keeps the assertion independent of
// which fix the page happened to report when it was read.
const GUIDED_INDICATOR_ROUTE_TOLERANCE_METERS = 60;

/** Perpendicular distance from a point to a polyline, in meters, using the
 * flat-earth approximation the demo's few-kilometer route makes exact
 * enough for a tolerance. */
function distance_to_route_meters(point, polyline) {
  const meters_per_degree_latitude = 111_320;
  const meters_per_degree_longitude =
    meters_per_degree_latitude * Math.cos((point.latitude * Math.PI) / 180);
  let nearest = Infinity;
  for (let index = 1; index < polyline.length; index += 1) {
    const start = polyline[index - 1];
    const end = polyline[index];
    const start_x = (start.longitude - point.longitude) * meters_per_degree_longitude;
    const start_y = (start.latitude - point.latitude) * meters_per_degree_latitude;
    const end_x = (end.longitude - point.longitude) * meters_per_degree_longitude;
    const end_y = (end.latitude - point.latitude) * meters_per_degree_latitude;
    const segment_x = end_x - start_x;
    const segment_y = end_y - start_y;
    const segment_length_squared = segment_x * segment_x + segment_y * segment_y;
    const projected =
      segment_length_squared === 0
        ? 0
        : Math.max(
          0,
          Math.min(1, -(start_x * segment_x + start_y * segment_y) / segment_length_squared),
        );
    const closest_x = start_x + projected * segment_x;
    const closest_y = start_y + projected * segment_y;
    nearest = Math.min(nearest, Math.hypot(closest_x, closest_y));
  }
  return nearest;
}
// Tile fixtures a corridor frame still awaits: the settled pump's own bound
// (mirrored by the page's constant) times the drain by 100 ms retries.
const SETTLED_PUMP_DRAIN_TIMEOUT_MILLISECONDS = 5000;

test.beforeEach(async ({ page }) => {
  await page.goto('/?advanced');
  // The Rust-side `initializeSdk` promise gates every assertion.
  await waitForMapTestReady(page);
});

test('module glue constants mirror the §2.4 token rows', async () => {
  // The shared module is the page's own import; the token rows must be the
  // design language's values (a drift would silently change both sides).
  expect(select_scale_bar_scale(0.0)).toBeNull();
  expect(select_scale_bar_scale(Number.POSITIVE_INFINITY)).toBeNull();
  expect(select_scale_bar_scale(-0.5)).toBeNull();
  expect(EASED_CAMERA_MOVE_DURATION_MILLISECONDS).toBe(300);
  expect(NAVIGATION_FOLLOW_PITCH_3D_DEGREES).toBe(45);
  expect(FOLLOW_BEARING_CHASE_TIME_CONSTANT_MILLISECONDS).toBe(150);
});

test('zoom controls step the camera one zoom level and the slider follows', async ({
  page,
}) => {
  await page.locator('#zoom-in-control').click();
  await expect
    .poll(() => page.evaluate(() => window.mapCameraState().zoom), {
      timeout: PUMP_IDLE_TIMEOUT_MILLISECONDS,
    })
    .toBe(INITIAL_ZOOM_LEVEL + ZOOM_STEP_LEVELS);
  const slider_value_after_zoom_in = await page
    .locator('#zoom-control')
    .inputValue();
  expect(Number(slider_value_after_zoom_in)).toBeCloseTo(
    INITIAL_ZOOM_LEVEL + ZOOM_STEP_LEVELS,
    5,
  );

  await page.locator('#zoom-out-control').click();
  await expect
    .poll(() => page.evaluate(() => window.mapCameraState().zoom), {
      timeout: PUMP_IDLE_TIMEOUT_MILLISECONDS,
    })
    .toBe(INITIAL_ZOOM_LEVEL);

  // The step clamps at the slider range's ends like the slider does.
  const slider_minimum = Number(
    await page.locator('#zoom-control').getAttribute('min'),
  );
  const zoom_before_clamping = await page.evaluate(
    () => window.mapCameraState().zoom,
  );
  const clamp_step_count = Math.ceil(zoom_before_clamping - slider_minimum);
  for (let step_index = 0; step_index < clamp_step_count; step_index += 1) {
    await page.locator('#zoom-out-control').click();
  }
  await expect
    .poll(() => page.evaluate(() => window.mapCameraState().zoom), {
      timeout: PUMP_IDLE_TIMEOUT_MILLISECONDS,
    })
    .toBeCloseTo(slider_minimum, 5);
});

test('a held zoom control repeats its step at the §2.4 cadence', async ({ page }) => {
  const zoom_control = page.locator('#zoom-in-control');
  const zoom_before = await page.evaluate(() => window.mapCameraState().zoom);
  await zoom_control.dispatchEvent('pointerdown');
  await expect
    .poll(
      () => page.evaluate(() => window.mapCameraState().zoom),
      { timeout: PUMP_IDLE_TIMEOUT_MILLISECONDS },
    )
    // A 250 ms repeat cadence must land at least two further steps while a
    // test pointer holds the control for a second-and-a-half.
    .toBeGreaterThanOrEqual(zoom_before + 2);
  // Reset: wait long enough for a 3rd step first, then release.
  await page.waitForTimeout(300);
  await zoom_control.dispatchEvent('pointerup');
  const zoom_after_hold = await page.evaluate(() => window.mapCameraState().zoom);
  expect(zoom_after_hold).toBeGreaterThan(zoom_before + 1);
  // Release must not double-step: the value settles (the pump idles) with
  // no further repeat after the release.
  await page.waitForTimeout(400);
  const zoom_settled = await page.evaluate(() => window.mapCameraState().zoom);
  expect(zoom_settled).toBe(zoom_after_hold);
  // Return the camera to the default zoom for the later tests' sanity.
  const zoom_out_control = page.locator('#zoom-out-control');
  const steps_from_zoom_after_hold_to_default = Math.ceil(
    zoom_settled - INITIAL_ZOOM_LEVEL,
  );
  for (
    let step_index = 0;
    step_index < steps_from_zoom_after_hold_to_default;
    step_index += 1
  ) {
    await zoom_out_control.click();
  }
  await expect
    .poll(() => page.evaluate(() => window.mapCameraState().zoom), {
      timeout: PUMP_IDLE_TIMEOUT_MILLISECONDS,
    })
    .toBeLessThanOrEqual(INITIAL_ZOOM_LEVEL);
});

test('the scale bar selects a §2.4 ladder distance and width', async ({ page }) => {
  // The selection is refreshed per render step; await the first one.
  await expect
    .poll(() => page.evaluate(() => window.mapScaleBar?.() ?? null), {
      timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS,
    })
    .not.toBeNull();
  const scale_bar_from_page = await page.evaluate(() => window.mapScaleBar?.());
  expect(scale_bar_from_page).not.toBeNull();
  const ground_answer = await page.evaluate((field_of_view_degrees) => {
    const camera_state = window.mapCameraState();
    const camera = new window.mapTest.MapCamera(
      camera_state.latitude,
      camera_state.longitude,
      camera_state.zoom,
      camera_state.bearing,
      camera_state.pitch,
    );
    return camera.groundMetersPerScreenPixel(
      document.getElementById('map-canvas').width,
      document.getElementById('map-canvas').height,
      field_of_view_degrees,
    );
  }, FIELD_OF_VIEW_DEGREES);
  const expected_selection = select_scale_bar_scale(ground_answer);
  expect(expected_selection).not.toBeNull();
  // The page computed exactly the shared module's selection (the two sides
  // consume one ladder implementation).
  expect(scale_bar_from_page.distanceMeters).toBeCloseTo(
    expected_selection.distanceMeters,
    5,
  );
  expect(scale_bar_from_page.widthPixels).toBeCloseTo(expected_selection.widthPixels, 5);
  expect(scale_bar_from_page.widthPixels).toBeGreaterThanOrEqual(50);
  expect(scale_bar_from_page.widthPixels).toBeLessThanOrEqual(120);
  expect(scale_bar_from_page.labelText).toBe(
    scale_bar_distance_label_text(expected_selection.distanceMeters),
  );
  // The bar's drawn width matches the selection's pixels.
  const bar_width = await page
    .locator('#scale-bar-line')
    .evaluate((scale_bar_line) => scale_bar_line.getBoundingClientRect().width);
  expect(Math.round(expected_selection.widthPixels)).toBe(Math.round(bar_width));
});

test('my-location shows the tracking-dot indicator centered on the camera', async ({
  page,
  context,
}) => {
  // The round control now drives the device location source; stub a fix at
  // the camera's own center so the tracking dot lands where this test has
  // always probed for it.
  await context.grantPermissions(['geolocation'], {
    origin: HARNESS_ORIGIN,
  });
  await context.setGeolocation({
    latitude: 38.5766,
    longitude: -121.4944,
    accuracy: 5,
  });
  // The renderer mode only becomes known after the first frame; a pre-render
  // read would spuriously skip on a healthy GPU run.
  await page.waitForFunction(() => window.mapRendererMode() !== null, null, {
    timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS,
  });
  const renderer_mode = await page.evaluate(() => (typeof window.mapRendererMode === 'function' ? window.mapRendererMode() : null));
  if (renderer_mode !== 'gpu') {
    test.skip(
      true,
      'headless WebGPU unavailable; the 2D path draws no location indicator',
    );
    return;
  }
  await page.waitForFunction(
    () => (window.mapLastFrameStats?.()?.requestedTiles ?? 0) > 0,
    null,
    { timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS },
  );

  await page.locator('#my-location-control').click();
  await expect
    .poll(() => page.evaluate(() => window.mapLocationIndicatorState()), {
      timeout: PUMP_IDLE_TIMEOUT_MILLISECONDS,
    })
    .not.toBeNull();
  const indicator_state = await page.evaluate(() => window.mapLocationIndicatorState());
  expect(indicator_state.visualKind).toBe('trackingDot');
  const camera_state = await page.evaluate(() => window.mapCameraState());
  expect(indicator_state.latitude).toBeCloseTo(camera_state.latitude, 5);
  expect(indicator_state.longitude).toBeCloseTo(camera_state.longitude, 5);
  expect(indicator_state.courseHeadingDegrees).toBeNull();
  // The renderer's accuracy-ring draw threshold is above the demo fix's
  // reported accuracy (the native demo's rule): the ring stays undrawn.
  expect(indicator_state.horizontalAccuracyMeters).toBeLessThan(10);

  // Pixel probe: the disc paints screen-blue (the wasm disc default) at the
  // camera-centered anchor. Chromium stops serving a presented WebGPU frame
  // to `drawImage` once the compositor has consumed it, so — like the
  // smoke test — the probe nudges the render queue (a zero-delta wheel
  // event re-schedules a GPU frame without moving the camera) and samples
  // across the following animation frames, keeping the best count: on a
  // slow runner the pre-nudge presented frame may predate the indicator's
  // first drawn frame entirely.
  const blue_texel_count = await page.evaluate(() => {
    const PROBE_ANIMATION_FRAMES = 12;
    const PROBE_TIMEOUT_MILLISECONDS = 2_000;
    const probe_canvas = document.createElement('canvas');
    const gpu_canvas = document.getElementById('map-canvas');
    const probe_height = 60;
    const probe_width = 60;
    const count_blue_texels = () => {
      probe_canvas.width = probe_width;
      probe_canvas.height = probe_height;
      const probe_context = probe_canvas.getContext('2d');
      probe_context.drawImage(
        gpu_canvas,
        gpu_canvas.width / 2 - probe_width / 2,
        gpu_canvas.height / 2 - probe_height / 2,
        probe_width,
        probe_height,
        0,
        0,
        probe_width,
        probe_height,
      );
      const { data } = probe_context.getImageData(0, 0, probe_width, probe_height);
      let blue_texels = 0;
      for (let texel_index = 0; texel_index < data.length; texel_index += 4) {
        const red = data[texel_index];
        const green = data[texel_index + 1];
        const blue = data[texel_index + 2];
        if (blue - red > 60 && blue - green > 40 && blue > 120) {
          blue_texels += 1;
        }
      }
      return blue_texels;
    };
    document.getElementById('map-overlay').dispatchEvent(
      new WheelEvent('wheel', { deltaY: 0, cancelable: true }),
    );
    return new Promise((resolve_probe) => {
      let best_blue_texel_count = 0;
      for (let attempt = 0; attempt < PROBE_ANIMATION_FRAMES; ++attempt) {
        requestAnimationFrame(() => {
          best_blue_texel_count = Math.max(best_blue_texel_count, count_blue_texels());
        });
      }
      setTimeout(() => resolve_probe(best_blue_texel_count), PROBE_TIMEOUT_MILLISECONDS);
    });
  });
  expect(blue_texel_count).toBeGreaterThan(20);
});

test('the 2D/3D toggle swings the pitch over its eased duration', async ({ page }) => {
  await page.locator('#plane-toggle-control').click();
  await expect
    .poll(() => page.evaluate(() => window.mapCameraState().pitch), {
      timeout: PUMP_IDLE_TIMEOUT_MILLISECONDS,
    })
    .toBeCloseTo(NAVIGATION_FOLLOW_PITCH_3D_DEGREES, 1);
  expect(await page.locator('#plane-toggle-control').textContent()).toBe('3D');
  expect(
    await page
      .locator('#plane-toggle-control')
      .evaluate((plane_button) => plane_button.classList.contains('primary')),
  ).toBe(true);

  await page.locator('#plane-toggle-control').click();
  await expect
    .poll(() => page.evaluate(() => window.mapCameraState().pitch), {
      timeout: PUMP_IDLE_TIMEOUT_MILLISECONDS,
    })
    .toBeCloseTo(0, 5);
  expect(await page.locator('#plane-toggle-control').textContent()).toBe('2D');
});

test('the compass appears off north and eases the camera north up on tap', async ({
  page,
}) => {
  // Off north: the compass shows, rotated opposite the bearing.
  await expect
    .poll(() => page.locator('#compass-control').isHidden())
    .toBe(true);
  await page.locator('#bearing-control').fill('33');
  await expect
    .poll(() => page.locator('#compass-control').isHidden())
    .toBe(false);
  const compass_letter_rotation = await page
    .locator('#compass-north-letter')
    .evaluate((north_letter) => north_letter.style.transform);
  expect(compass_letter_rotation).toBe('rotate(-33deg)');

  // Tap-to-re-north: the eased move returns the bearing to north in the
  // token duration, and the compass hides again.
  await page.locator('#compass-control').click();
  await expect
    .poll(() => page.evaluate(() => window.mapCameraState().bearing), {
      timeout: EASED_CAMERA_MOVE_DURATION_MILLISECONDS + PUMP_IDLE_TIMEOUT_MILLISECONDS,
    })
    .toBeCloseTo(0, 1);
  await expect
    .poll(() => page.locator('#compass-control').isHidden(), {
      timeout: PUMP_IDLE_TIMEOUT_MILLISECONDS,
    })
    .toBe(true);
});

test('the follow demo drives the guided indicator and the §5.3 zoom ramp', async ({
  page,
}) => {
  // The renderer mode only becomes known after the first frame; a pre-render
  // read would spuriously skip on a healthy GPU run.
  await page.waitForFunction(() => window.mapRendererMode() !== null, null, {
    timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS,
  });
  const renderer_mode = await page.evaluate(() => (typeof window.mapRendererMode === 'function' ? window.mapRendererMode() : null));
  if (renderer_mode !== 'gpu') {
    test.skip(
      true,
      'headless WebGPU unavailable; the 2D path runs no follow demo',
    );
    return;
  }
  const pre_trip_camera = await page.evaluate(() => window.mapCameraState());
  await page.locator('#follow-demo-start-control').click();
  await expect
    .poll(() => page.evaluate(() => window.mapFollowDemoState().phase), {
      timeout: FOLLOW_DEMO_AWAIT_MILLISECONDS,
    })
    .toBe('engaged');
  const follow_state_travelled = await page.evaluate(() =>
    window.mapFollowDemoState());
  // The start applied the fixed deep zoom (§5.3: the base token, ramped
  // from the travelled fraction — zero at the start).
  expect(follow_state_travelled.cameraZoom).toBeCloseTo(
    follow_zoom_toward_trip_end(1.0),
    1,
  );
  expect(follow_state_travelled.totalMeters).toBeGreaterThan(0);

  // The guided indicator rides the route: the guided-course arrow with a
  // course, at a fix position on the route polyline.
  await expect
    .poll(() => page.evaluate(() => window.mapLocationIndicatorState()?.visualKind), {
      timeout: FOLLOW_DEMO_AWAIT_MILLISECONDS,
    })
    .toBe('guidedCourseArrow');
  const guided_indicator = await page.evaluate(() => window.mapLocationIndicatorState());
  expect(guided_indicator.courseHeadingDegrees).not.toBeNull();
  // The demo advances a fix every few hundred milliseconds, so the indicator
  // is asserted against the route itself rather than against one sampled fix:
  // the arrow rides the polyline, within the demo's own step size.
  const route_polyline = [
    // The artifact's route scene coordinates, mirrored numerically (the
    // demo drives exactly the shared artifact; the round trip is checked
    // by the artifact's own fixture hash — see overlay_scenes.mjs).
    { latitude: 38.573483, longitude: -121.5023 },
    { latitude: 38.5766, longitude: -121.4944 },
    { latitude: 38.580987, longitude: -121.4858 },
  ];
  expect(
    distance_to_route_meters(guided_indicator, route_polyline),
  ).toBeLessThan(GUIDED_INDICATOR_ROUTE_TOLERANCE_METERS);

  // The §5.3 follow camera points the map along the smoothed course: the
  // route scene runs roughly eastward, so the bearing leaves north.
  await expect
    .poll(() => page.evaluate(() => window.mapCameraState().bearing), {
      timeout: FOLLOW_DEMO_AWAIT_MILLISECONDS,
    })
    .toBeGreaterThan(20);

  // A pan gesture breaks follow immediately, with the my-location control's
  // re-center affordance; a deliberate gesture keeps follow broken until the
  // control re-engages it (a later fix must not snap the panned view back).
  await page.locator('#map-overlay').dispatchEvent('pointerdown');
  const broken_phase = await page.evaluate(() => window.mapFollowDemoState().phase);
  expect(broken_phase).toBe('brokenByGesture');
  expect(
    await page
      .locator('#my-location-control')
      .evaluate(control => control.classList.contains('recentering')),
  ).toBe(true);
  await page.waitForTimeout(400);
  await expect
    .poll(() => page.evaluate(() => window.mapFollowDemoState().phase), {
      timeout: FOLLOW_DEMO_AWAIT_MILLISECONDS,
    })
    .toBe('brokenByGesture');

  // The my-location control re-centers and re-engages follow.
  await page.locator('#my-location-control').click();
  await expect
    .poll(() => page.evaluate(() => window.mapFollowDemoState().phase), {
      timeout: FOLLOW_DEMO_AWAIT_MILLISECONDS,
    })
    .toBe('engaged');

  // Stopping the trip restores the pre-trip camera and clears the
  // indicator (trip end runs the same restore path).
  await page.locator('#follow-demo-start-control').click();
  await expect
    .poll(() => page.evaluate(() => window.mapFollowDemoState().phase), {
      timeout: FOLLOW_DEMO_AWAIT_MILLISECONDS,
    })
    .toBe('disengaged');
  expect(await page.evaluate(() => window.mapLocationIndicatorState())).toBeNull();
  const restored_camera = await page.evaluate(() => window.mapCameraState());
  expect(restored_camera.latitude).toBeCloseTo(pre_trip_camera.latitude, 5);
  expect(restored_camera.longitude).toBeCloseTo(pre_trip_camera.longitude, 5);
  expect(restored_camera.zoom).toBeCloseTo(pre_trip_camera.zoom, 5);
  expect(restored_camera.pitch).toBeCloseTo(pre_trip_camera.pitch, 5);
});

test('the settled pump drains the default scene to zero missing tiles', async ({
  page,
}) => {
  await page.waitForFunction(
    () => (window.mapLastFrameStats?.()?.requestedTiles ?? 0) > 0,
    null,
    { timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS },
  );
  await page.waitForFunction(
    () => {
      const frame_statistics = window.mapLastFrameStats?.();
      return frame_statistics && frame_statistics.missingTiles === 0;
    },
    null,
    { timeout: SETTLED_PUMP_DRAIN_TIMEOUT_MILLISECONDS },
  );
  const frame_facts = await page.evaluate(() => window.mapFrameFacts());
  expect(frame_facts.framesDrawn).toBeGreaterThan(0);
  expect(frame_facts.settledPumpRerenders).toBeGreaterThanOrEqual(0);
});

test('the diagnostics readout reports frame facts and a bounded debug stream', async ({
  page,
}) => {
  await page.waitForFunction(
    () => (window.mapLastFrameStats?.()?.requestedTiles ?? 0) > 0,
    null,
    { timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS },
  );
  // `?advanced` is the explicit opt-in: the pane opens its one stream at Info,
  // and the explicit Show debug events switch lowers the display gate to Debug
  // for the rest of the session.
  await expect
    .poll(() => page.evaluate(() => window.mapDiagnosticsState().subscribed), {
      timeout: 10_000,
    })
    .toBe(true);
  expect(await page.evaluate(() => window.mapDiagnosticsState().displaySeverity))
    .toBe('Info');
  await page.locator('#diagnostics-debug-toggle').check();
  await expect
    .poll(() => page.evaluate(() => window.mapDiagnosticsState().displaySeverity))
    .toBe('Debug');
  await expect(page.locator('#diagnostics-debug-events-label')).toHaveText(
    'Hide debug events');
  // The pane is bounded to the shared event window.
  expect(await page.evaluate(() => window.mapDiagnosticsState().eventWindow))
    .toBe(100);

  // Clear resets the pane's displayed list.
  await page.locator('#diagnostics-clear-control').click();
  await expect(page.locator('#diagnostics-event-lines')).toHaveText(
    'No SDK events recorded yet.');

  const facts_text = await page.locator('#diagnostics-facts-lines').textContent();
  expect(facts_text).toContain('mode gpu');
  expect(facts_text).toMatch(/frames drawn [1-9]/);
  expect(facts_text).toContain('tiles drawn ');
});

test('the attribution info control expands the compliance text and closes again', async ({
  page,
}) => {
  const info_control = page.locator('#attribution-info-control');
  const panel = page.locator('#attribution-panel');
  await expect(panel).toBeHidden();
  await expect(info_control).toHaveAttribute('aria-label', 'Map attribution and terms');
  await expect(info_control).toHaveAttribute('aria-expanded', 'false');

  // The compact control replaces the always-visible line; one tap expands
  // the descriptor attribution and terms text.
  await info_control.click();
  await expect(panel).toBeVisible();
  await expect(info_control).toHaveAttribute('aria-expanded', 'true');
  // The license phrase every registry descriptor carries, so the assertion
  // names no provider.
  await expect(page.locator('#attribution-notice')).toContainText('ODbL');
  // The descriptor's terms note joins the panel (the shared registry text).
  await expect(panel.locator('.attribution-terms-note').first()).toContainText(
    'rate limits',
  );

  // Escape closes the panel; a tap outside closes it too.
  await page.keyboard.press('Escape');
  await expect(panel).toBeHidden();
  await info_control.click();
  await expect(panel).toBeVisible();
  await page.locator('#controls h1').click();
  await expect(panel).toBeHidden();
});

test('the §5.3 smoothing primitives behave like the native mirror', async () => {
  // The median smoother: five fixes' window, circular median.
  const smoother = new CourseMedianSmoother(5);
  expect(smoother.median()).toBeNull();
  smoother.add(350);
  smoother.add(10);
  smoother.add(355);
  // Distance ranking around 10 puts 355 (5 degrees) in the middle of
  // {10, 355, 350}: the circular median is 355 (a linear median would pick
  // 350).
  expect(smoother.median()).toBeCloseTo(355, 5);

  // The exponential chase: a 150 ms time constant advances roughly 63% of
  // the remaining distance each constant.
  const chase = new ExponentialValueChase(FOLLOW_BEARING_CHASE_TIME_CONSTANT_MILLISECONDS);
  // The chase's first frame folds one display frame's catch-up (~11%).
  const first_frame_value = chase.advance(0, 100, 0);
  expect(first_frame_value).toBeGreaterThan(0);
  expect(first_frame_value).toBeLessThan(12);
  // One full time constant later: ~63% of the remaining distance closes.
  const one_constant_value = chase.advance(first_frame_value, 100, 150);
  expect(one_constant_value).toBeGreaterThan(60);
  expect(one_constant_value).toBeLessThan(70);
  // The snap: under the snap distance the chase lands on the target.
  expect(new ExponentialValueChase(150).advance(0, 0.04, 0)).toBeCloseTo(0.04, 5);

  // The timed ease: smoothstep in and out over the token duration.
  const timed_ease = new SmoothstepTimedEase();
  timed_ease.begin(0, 0, 45, 300);
  expect(timed_ease.advance(150)).toEqual([22.5, false]);
  expect(timed_ease.advance(300)).toEqual([45, true]);
  expect(timed_ease.advance(400)).toEqual([45, true]);
  expect(timed_ease.is_running).toBe(false);
});

test('the shared reference math runs through the wasm exports in the page', async ({
  page,
}) => {
  // The page and its Playwright specs import one module; in the browser that
  // module re-exports `map-refapp-core`'s wasm surface, so these values are
  // the Rust answers the controls, the follow camera and the scale bar read.
  const shared_values = await page.evaluate(async () => {
    const math = await import('./map_controls.mjs');
    const smoother = new math.CourseMedianSmoother(5);
    smoother.add(350);
    smoother.add(10);
    smoother.add(355);

    const chase = new math.ExponentialValueChase(150);
    const chase_first_frame = chase.advance(0, 100, 0);

    const timed_ease = new math.SmoothstepTimedEase();
    timed_ease.begin(0, 0, 45, 300);
    const ease_mid = timed_ease.advance(150);
    const ease_end = timed_ease.advance(300);

    const follow_camera = new math.FollowCamera();
    follow_camera.engageTrip(38, -121, 12, 15, 0);
    const engaged_phase = follow_camera.phase;
    follow_camera.onFix(38.1, -121.1, 90, 0.8);
    const target_zoom = follow_camera.targetZoomLevel;
    const target_bearing = follow_camera.targetBearingDegrees;
    follow_camera.breakOnGesture();
    follow_camera.onFix(38.2, -121.2, 92, 0.7);
    const broken_phase = follow_camera.phase;
    const broken_target_bearing = follow_camera.targetBearingDegrees;
    follow_camera.reengage();
    const reengaged_phase = follow_camera.phase;
    const restored_camera = follow_camera.restorePreTripCamera();
    const final_phase = follow_camera.phase;

    const cursor = new math.TraceCursor(3);
    const cursor_indices = [
      cursor.advance(),
      cursor.advance(),
      cursor.advance(),
      cursor.advance(),
    ];

    return {
      median: smoother.median(),
      chase_first_frame,
      ease_mid,
      ease_end,
      engaged_phase,
      target_zoom,
      target_bearing,
      broken_phase,
      broken_target_bearing,
      reengaged_phase,
      restored_zoom: restored_camera?.zoomLevel ?? null,
      final_phase,
      cursor_indices,
      scale_selection: math.select_scale_bar_scale(1.0),
      empty_selection: math.select_scale_bar_scale(0),
      dispatch_interval: math.trace_dispatch_interval_milliseconds(5, 20),
      fast_dispatch_interval: math.trace_dispatch_interval_milliseconds(5, 160),
      smoothstep_quarter: math.smoothstep_eased_progress(0.25),
      bearing_off_north: math.bearing_off_north_degrees(350),
      signed_difference: math.signed_angular_difference_degrees(10, 350),
      follow_zoom_at_start: math.follow_zoom_toward_trip_end(1.0),
    };
  });

  // The wasm crate answers exactly the documented values the page uses.
  expect(shared_values.median).toBeCloseTo(355, 5);
  expect(shared_values.chase_first_frame).toBeGreaterThan(0);
  expect(shared_values.chase_first_frame).toBeLessThan(12);
  expect(shared_values.ease_mid).toEqual([22.5, false]);
  expect(shared_values.ease_end).toEqual([45, true]);
  // The follow-camera state machine: engage, retarget, gesture-break, explicit
  // re-engage and pre-trip restore all come from the crate.
  expect(shared_values.engaged_phase).toBe('engaged');
  expect(shared_values.target_zoom).toBeCloseTo(17.1, 5);
  expect(shared_values.target_bearing).toBeCloseTo(90, 5);
  expect(shared_values.broken_phase).toBe('brokenByGesture');
  expect(shared_values.broken_target_bearing).toBeCloseTo(90, 5);
  expect(shared_values.reengaged_phase).toBe('engaged');
  expect(shared_values.restored_zoom).toBe(12);
  expect(shared_values.final_phase).toBe('disengaged');
  // The trace cursor saturates after the last sample.
  expect(shared_values.cursor_indices).toEqual([0, 1, 2, null]);
  // The scale-bar ladder uses the token width range, and the dispatch cadence
  // folds the acceleration factor.
  expect(shared_values.scale_selection).toEqual({
    distanceMeters: 100,
    widthPixels: 100,
  });
  expect(shared_values.empty_selection).toBeNull();
  expect(shared_values.dispatch_interval).toBe(250);
  expect(shared_values.fast_dispatch_interval).toBe(31);
  // The easing and bearing helpers agree with the documented formulas.
  expect(shared_values.smoothstep_quarter).toBeCloseTo(0.15625, 9);
  expect(shared_values.bearing_off_north).toBeCloseTo(10, 9);
  expect(shared_values.signed_difference).toBeCloseTo(20, 9);
  expect(shared_values.follow_zoom_at_start).toBeCloseTo(17, 9);
});
test('the simulated trip offers its paces and pauses where it stands', async ({
  page,
}) => {
  await page.waitForFunction(() => window.mapRendererMode() !== null, null, {
    timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS,
  });
  const renderer_mode = await page.evaluate(() => (typeof window.mapRendererMode === 'function' ? window.mapRendererMode() : null));
  if (renderer_mode !== 'gpu') {
    test.skip(
      true,
      'headless WebGPU unavailable; the 2D path runs no follow demo',
    );
    return;
  }
  const playback_row = page.locator('#follow-demo-playback');
  await expect(playback_row).toBeHidden();
  await page.locator('#follow-demo-start-control').click();
  await expect(playback_row).toBeVisible();

  // The paces are canonical copy: four chips, the first selected.
  const speed_chips = page.locator('#follow-demo-speed-controls button');
  await expect(speed_chips).toHaveCount(4);
  await expect(speed_chips.nth(0)).toHaveAttribute('aria-pressed', 'true');
  await expect(speed_chips.nth(3)).toHaveText('8×');
  await speed_chips.nth(3).click();
  await expect(speed_chips.nth(3)).toHaveAttribute('aria-pressed', 'true');

  // A pause freezes the travelled distance and names its own resume.
  const pause_control = page.locator('#follow-demo-pause-control');
  await pause_control.click();
  await expect(pause_control).toHaveText('Resume');
  const paused_state = await page.evaluate(() => window.mapFollowDemoState());
  await page.waitForTimeout(400);
  const still_paused_state = await page.evaluate(() => window.mapFollowDemoState());
  expect(still_paused_state.travelledMeters).toBe(paused_state.travelledMeters);

  await pause_control.click();
  await expect(pause_control).toHaveText('Pause');
  await expect
    .poll(() => page.evaluate(() => window.mapFollowDemoState().travelledMeters), {
      timeout: FOLLOW_DEMO_AWAIT_MILLISECONDS,
    })
    .toBeGreaterThan(paused_state.travelledMeters);
});

test('seeking to the end arrives and seeking back leaves the arrival', async ({
  page,
}) => {
  await page.waitForFunction(() => window.mapRendererMode() !== null, null, {
    timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS,
  });
  const renderer_mode = await page.evaluate(() => (typeof window.mapRendererMode === 'function' ? window.mapRendererMode() : null));
  if (renderer_mode !== 'gpu') {
    test.skip(
      true,
      'headless WebGPU unavailable; the 2D path runs no follow demo',
    );
    return;
  }
  await page.locator('#follow-demo-start-control').click();
  await expect
    .poll(() => page.evaluate(() => window.mapFollowDemoState().phase), {
      timeout: FOLLOW_DEMO_AWAIT_MILLISECONDS,
    })
    .toBe('engaged');

  // Pause first, so the assertion after each seek is the seek's own result
  // (a running walker would legitimately move the travelled distance on).
  await page.locator('#follow-demo-pause-control').click();
  const seek_control = page.locator('#follow-demo-seek-control');
  await expect(seek_control).toBeVisible();
  const total_meters = await page.evaluate(
    () => window.mapFollowDemoState().totalMeters,
  );
  expect(total_meters).toBeGreaterThan(0);

  // The end of the route is exactly the walker's arrival: the trip presents
  // Arrived and the seek control remains usable to leave it.
  await seek_control.evaluate((control, target) => {
    control.value = String(target);
    control.dispatchEvent(new Event('input', { bubbles: true }));
  }, total_meters);
  await expect
    .poll(() => page.evaluate(() => window.mapFollowDemoState().hasArrived), {
      timeout: FOLLOW_DEMO_AWAIT_MILLISECONDS,
    })
    .toBe(true);
  const arrived_state = await page.evaluate(() => window.mapFollowDemoState());
  expect(arrived_state.travelledMeters).toBeCloseTo(total_meters, 1);
  await expect(page.locator('#follow-demo-live-line')).toContainText('Arrived');
  await expect(page.locator('#follow-demo-pause-control')).toBeHidden();

  // Seeking backwards asks the walker for that fix, so the arrival clears
  // and the paused trip stays paused at the new position.
  const midpoint_meters = total_meters / 2;
  await seek_control.evaluate((control, target) => {
    control.value = String(target);
    control.dispatchEvent(new Event('input', { bubbles: true }));
  }, midpoint_meters);
  await expect
    .poll(() => page.evaluate(() => window.mapFollowDemoState().hasArrived), {
      timeout: FOLLOW_DEMO_AWAIT_MILLISECONDS,
    })
    .toBe(false);
  const seeked_state = await page.evaluate(() => window.mapFollowDemoState());
  expect(seeked_state.travelledMeters).toBeCloseTo(midpoint_meters, 0);
  expect(seeked_state.isPaused).toBe(true);
  await expect(page.locator('#follow-demo-live-line')).not.toContainText('Arrived');
  await page.waitForTimeout(400);
  expect(await page.evaluate(() => window.mapFollowDemoState().travelledMeters))
    .toBe(seeked_state.travelledMeters);
});

test('every map control button shares the 40-unit diameter', async ({ page }) => {
  await page.goto('/');
  await waitForMapTestReady(page);
  const control_selectors = [
    '#zoom-in-control',
    '#zoom-out-control',
    '#my-location-control',
    '#plane-toggle-control',
    '#compass-control',
    '#map-layers-control',
    '#attribution-info-control',
  ];
  const control_sizes = await page.evaluate((selectors) => selectors.map(
    (selector) => {
      const style = getComputedStyle(document.querySelector(selector));
      return { selector, width: style.width, height: style.height };
    },
  ), control_selectors);
  for (const control_size of control_sizes) {
    expect(control_size.width, `${control_size.selector} width`).toBe('40px');
    expect(control_size.height, `${control_size.selector} height`).toBe('40px');
  }
});
