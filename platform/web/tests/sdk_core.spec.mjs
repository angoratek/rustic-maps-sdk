// Visual end-to-end checks of the wasm SDK core in a real Chromium tab:
// module load, projection, project/unproject round trips, visible tile
// listing across the antimeridian, and the WebGPU canvas rendering the
// recorded raster tiles. Tolerances mirror the native camera tests in
// `crates/rustic-maps-rendering/src/camera.rs`.
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

const CENTER_PIXEL_TOLERANCE = 1e-2;
const SCREEN_PIXEL_TOLERANCE = 2e-3;
const ROUND_TRIP_COORDINATE_TOLERANCE_DEGREES = 1e-6;
const FIELD_OF_VIEW_DEGREES = 60;
const PERSPECTIVE_FIELD_OF_VIEW_DEGREES = 45;

const VIEWPORT_WIDTH_PIXELS = 1024;
const VIEWPORT_HEIGHT_PIXELS = 768;

const TEST_CENTER_LATITUDE = 38.5766;
const TEST_CENTER_LONGITUDE = -121.4944;
const TEST_CAMERA_ZOOM = 12;
const TEST_CAMERA_BEARING_DEGREES = 33;
const TEST_CAMERA_PITCH_DEGREES = 40;

const ROUND_TRIP_ZOOM = 14;
const ROUND_TRIP_BEARING_DEGREES = 25;
const ROUND_TRIP_PITCH_DEGREES = 30;

const ANTIMERIDIAN_LATITUDE = 0.0;
const ANTIMERIDIAN_LONGITUDE = 179.95;
const ANTIMERIDIAN_ZOOM = 8;
const ANTIMERIDIAN_BEARING_DEGREES = 0;
const ANTIMERIDIAN_WEST_COPY_COLUMN = 255;
const ANTIMERIDIAN_EAST_COPY_COLUMN = 0;
const ANTIMERIDIAN_ROW_LOW = 127;
const ANTIMERIDIAN_ROW_HIGH = 128;

const SAMPLED_SCREEN_POINTS = [
  [512, 384],
  [256, 192],
  [768, 576],
  [100, 100],
  [900, 700],
];

test.beforeEach(async ({ page }) => {
  // Observe the readiness ladder from before any page script runs: the
  // `loading`→`ready` flip can outpace a test's first sample after
  // `mapTestReady` (the 2D fallback path flips immediately), so attribute
  // mutations are recorded here; `attributeOldValue` captures the previous
  // value at mutation time, immune to the sampling race.
  await page.addInitScript(() => {
    window.__readiness_ladder = [];
    const ladder_observer = new MutationObserver((records) => {
      for (const record of records) {
        if (
          record.target?.id === 'status-line' &&
          record.attributeName === 'data-readiness'
        ) {
          window.__readiness_ladder.push({
            from: record.oldValue,
            to: record.target.dataset.readiness,
          });
        }
      }
    });
    // The document element does not exist yet at init-script time (the
    // script runs on an empty document): observe `document` itself.
    ladder_observer.observe(document, {
      subtree: true,
      attributes: true,
      attributeFilter: ['data-readiness'],
      attributeOldValue: true,
    });
  });
  await page.goto('/?advanced');
  // The Rust-side `initializeSdk` promise gates every assertion.
  await waitForMapTestReady(page);
});

test('sdk loads and reports its compiled version', async ({ page }) => {
  const compiled_version = await page.evaluate(() =>
    window.mapTest.getSdkCompiledVersion(),
  );
  expect(typeof compiled_version).toBe('string');
  expect(compiled_version.length).toBeGreaterThan(0);
  expect(await page.evaluate(() => window.__sdkReady)).toBe(true);
});

test('center projects to the viewport center', async ({ page }) => {
  const screen_point = await page.evaluate(
    ({
      latitude,
      longitude,
      zoom_level,
      bearing_degrees,
      pitch_degrees,
      width_pixels,
      height_pixels,
      field_of_view_degrees,
    }) => {
      const camera = new window.mapTest.MapCamera(
        latitude,
        longitude,
        zoom_level,
        bearing_degrees,
        pitch_degrees,
      );
      return camera.projectToScreen(
        latitude,
        longitude,
        width_pixels,
        height_pixels,
        field_of_view_degrees,
      );
    },
    {
      latitude: TEST_CENTER_LATITUDE,
      longitude: TEST_CENTER_LONGITUDE,
      zoom_level: TEST_CAMERA_ZOOM,
      bearing_degrees: TEST_CAMERA_BEARING_DEGREES,
      pitch_degrees: TEST_CAMERA_PITCH_DEGREES,
      width_pixels: VIEWPORT_WIDTH_PIXELS,
      height_pixels: VIEWPORT_HEIGHT_PIXELS,
      field_of_view_degrees: FIELD_OF_VIEW_DEGREES,
    },
  );
  expect(
    Math.abs(screen_point.x - VIEWPORT_WIDTH_PIXELS / 2),
  ).toBeLessThan(CENTER_PIXEL_TOLERANCE);
  expect(
    Math.abs(screen_point.y - VIEWPORT_HEIGHT_PIXELS / 2),
  ).toBeLessThan(CENTER_PIXEL_TOLERANCE);
});

test('project and unproject round trip within tolerance', async ({ page }) => {
  const drifts = await page.evaluate(
    ({
      latitude,
      longitude,
      zoom_level,
      bearing_degrees,
      pitch_degrees,
      screen_points,
      width_pixels,
      height_pixels,
      field_of_view_degrees,
    }) => {
      const camera = new window.mapTest.MapCamera(
        latitude,
        longitude,
        zoom_level,
        bearing_degrees,
        pitch_degrees,
      );
      return screen_points.map(([screen_x, screen_y]) => {
        const ground = camera.screenToGroundCoordinate(
          screen_x,
          screen_y,
          width_pixels,
          height_pixels,
          field_of_view_degrees,
        );
        if (!ground) {
          return null;
        }
        const rescreened = camera.projectToScreen(
          ground.latitude,
          ground.longitude,
          width_pixels,
          height_pixels,
          field_of_view_degrees,
        );
        if (!rescreened) {
          return null;
        }
        // A true coordinate round trip: unprojecting the rescreened pixel
        // must return the same ground point the pixel first resolved to.
        const unprojection_repeat = camera.screenToGroundCoordinate(
          rescreened.x,
          rescreened.y,
          width_pixels,
          height_pixels,
          field_of_view_degrees,
        );
        if (!unprojection_repeat) {
          return null;
        }
        return {
          pixel_drift: Math.max(
            Math.abs(rescreened.x - screen_x),
            Math.abs(rescreened.y - screen_y),
          ),
          coordinate_drift: Math.max(
            Math.abs(unprojection_repeat.latitude - ground.latitude),
            Math.abs(unprojection_repeat.longitude - ground.longitude),
          ),
        };
      });
    },
    {
      latitude: TEST_CENTER_LATITUDE,
      longitude: TEST_CENTER_LONGITUDE,
      zoom_level: ROUND_TRIP_ZOOM,
      bearing_degrees: ROUND_TRIP_BEARING_DEGREES,
      pitch_degrees: ROUND_TRIP_PITCH_DEGREES,
      screen_points: SAMPLED_SCREEN_POINTS,
      width_pixels: VIEWPORT_WIDTH_PIXELS,
      height_pixels: VIEWPORT_HEIGHT_PIXELS,
      field_of_view_degrees: PERSPECTIVE_FIELD_OF_VIEW_DEGREES,
    },
  );
  for (const drift of drifts) {
    expect(drift, 'every sampled pixel must resolve to ground and back').not.toBeNull();
    expect(drift.pixel_drift).toBeLessThan(SCREEN_PIXEL_TOLERANCE);
    expect(drift.coordinate_drift).toBeLessThan(
      ROUND_TRIP_COORDINATE_TOLERANCE_DEGREES,
    );
  }
});

test('visible tile keys contain the center tile and wrap across the antimeridian', async ({
  page,
}) => {
  const listing = await page.evaluate(
    ({
      latitude,
      longitude,
      zoom_level,
      bearing_degrees,
      width_pixels,
      height_pixels,
      field_of_view_degrees,
    }) => {
      const camera = new window.mapTest.MapCamera(
        latitude,
        longitude,
        zoom_level,
        bearing_degrees,
        0.0,
      );
      const tileZoomLevel = camera.tileZoomLevel();
      return {
        tileZoomLevel,
        visible_tiles: camera.visibleTileKeys(
          width_pixels,
          height_pixels,
          field_of_view_degrees,
          tileZoomLevel,
        ),
        center_tile: window.mapTest.tileKeyForCoordinate(
          latitude,
          longitude,
          tileZoomLevel,
        ),
      };
    },
    {
      latitude: ANTIMERIDIAN_LATITUDE,
      longitude: ANTIMERIDIAN_LONGITUDE,
      zoom_level: ANTIMERIDIAN_ZOOM,
      bearing_degrees: ANTIMERIDIAN_BEARING_DEGREES,
      width_pixels: VIEWPORT_WIDTH_PIXELS,
      height_pixels: VIEWPORT_HEIGHT_PIXELS,
      field_of_view_degrees: FIELD_OF_VIEW_DEGREES,
    },
  );
  expect(listing.tileZoomLevel).toBe(ANTIMERIDIAN_ZOOM);

  const listed_columns = new Set(listing.visible_tiles.map((tile) => tile.tileX));
  expect(
    listed_columns.has(ANTIMERIDIAN_WEST_COPY_COLUMN) &&
      listed_columns.has(ANTIMERIDIAN_EAST_COPY_COLUMN),
    `footprint at the seam must list both world copies: ${JSON.stringify(
      listing.visible_tiles,
    )}`,
  ).toBe(true);
  for (const tile of listing.visible_tiles) {
    expect(tile.zoomLevel).toBe(ANTIMERIDIAN_ZOOM);
    expect(tile.tileY).toBeGreaterThanOrEqual(ANTIMERIDIAN_ROW_LOW);
    expect(tile.tileY).toBeLessThanOrEqual(ANTIMERIDIAN_ROW_HIGH);
  }
  // The center tile itself must be listed.
  expect(
    listing.visible_tiles.some(
      (tile) =>
        tile.tileX === listing.center_tile.tileX &&
        tile.tileY === listing.center_tile.tileY,
    ),
    `center tile ${JSON.stringify(listing.center_tile)} missing from the listing`,
  ).toBe(true);
});
// --- WebGPU canvas rendering (wasm GPU render path) ---

// The playground resolves its renderer mode (GPU or degraded overlay) after
// `mapTestReady`; the fixtures themselves are local, so only a slow first
// browser GPU init can stretch this wait.
const RENDERER_READINESS_TIMEOUT_MILLISECONDS = 30_000;
// The default playground camera sits on downtown Sacramento at the
// fixtures' highest recorded zoom: `record_golden_tiles` recorded the
// zoom-14 neighborhood 2661..=2663 x 6284..=6286 there.
const DEFAULT_RENDER_TILE_ZOOM_LEVEL = 14;
const READOUT_CENTER_PATTERN = /center (-?\d+\.\d+), (-?\d+\.\d+)/;
// The probe samples the GPU canvas at this reduced square size; beyond the
// painted-pixel minimum real imagery has clearly reached the surface.
const PROBE_CANVAS_EDGE_PIXELS = 64;
// The GPU clear color is opaque black, so every non-black sampled texel is
// map imagery or center-mark paint.
const MINIMUM_PROBED_PAINTED_PIXELS = 100;
// Components in one RGBA sample of `getImageData` data.
const PIXELS_PER_RGBA_SAMPLE = 4;
// Animation frames the drawImage probe samples across after its nudge.
const PROBE_ANIMATION_FRAMES = 12;

// Waits for the playground's documented renderer-mode resolution and
// returns it.
async function resolve_renderer_mode(page) {
  await page.waitForFunction(
    () =>
      window.mapRendererMode?.() === 'gpu' ||
      window.mapRendererMode?.() === 'overlay',
    null,
    { timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS },
  );
  return page.evaluate(() => (typeof window.mapRendererMode === 'function' ? window.mapRendererMode() : null));
}

function readout_center(text_content) {
  const center_match = READOUT_CENTER_PATTERN.exec(text_content);
  if (!center_match) {
    return null;
  }
  return { latitude: Number(center_match[1]), longitude: Number(center_match[2]) };
}

test('wgpu canvas draws recorded tiles when browser WebGPU is available', async ({
  page,
}) => {
  const renderer_mode = await resolve_renderer_mode(page);
  if (renderer_mode !== 'gpu') {
    // Documented degradation: no browser WebGPU means the overlay renderer
    // with its reason recorded in the status line and readout.
    expect(renderer_mode).toBe('overlay');
    const degradation = await page.evaluate(() => ({
      status_text: document.getElementById('status-line').textContent,
      diagnostics_text: document.getElementById('diagnostics-facts-lines').textContent,
    }));
    // The first-run line stays human; the renderer detail is diagnostics-only.
    expect(degradation.status_text).toBe('Degraded: 2D grid');
    expect(degradation.diagnostics_text).toContain('mode overlay');
    test.skip(
      true,
      `headless WebGPU unavailable in this browser; degradation asserted: ${degradation.status_text}`,
    );
    return;
  }

  // The settled frame: the first frame that requested tiles may still be
  // waiting on the detached prefetch, and the settled pump re-renders until
  // none are missing.
  const frame_statistics = await page.waitForFunction(() => {
    const statistics = window.mapLastFrameStats?.();
    return statistics && statistics.requestedTiles > 0 && statistics.missingTiles === 0
      ? statistics
      : null;
  }, null, { timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS }).then((statistics_handle) =>
    statistics_handle.jsonValue());

  // The default camera sits on the recorded Sacramento neighborhood at its
  // native zoom, so every requested tile must be drawable and none missing.
  expect(frame_statistics.tileZoomLevel).toBe(DEFAULT_RENDER_TILE_ZOOM_LEVEL);
  expect(frame_statistics.requestedTiles).toBeGreaterThan(0);
  expect(frame_statistics.drawnTiles).toBeGreaterThan(0);
  expect(frame_statistics.missingTiles).toBe(0);
  expect(frame_statistics.drawnFragments).toBeGreaterThanOrEqual(
    frame_statistics.drawnTiles,
  );

  // Pixel probe: composite the WebGPU canvas into a 2D probe and count
  // non-black texels — the clear color is opaque black, so painted pixels
  // are map imagery. Chromium stops serving a presented WebGPU frame to
  // `drawImage` once the compositor has consumed it, so the probe nudges
  // the render queue (a zero-delta wheel event re-schedules a GPU frame
  // without moving the camera) and then samples across animation frames.
  const probe_painted_pixels = await page.evaluate(
    ({
      probe_edge_pixels,
      pixels_per_rgba_sample,
      minimum_probed_painted_pixels,
      probe_animation_frames,
    }) => {
      const paint_probe_canvas = () => {
        const gpu_canvas = document.getElementById('map-canvas');
        const probe_canvas = document.createElement('canvas');
        probe_canvas.width = probe_edge_pixels;
        probe_canvas.height = probe_edge_pixels;
        const probe_context = probe_canvas.getContext('2d');
        probe_context.drawImage(gpu_canvas, 0, 0, probe_edge_pixels, probe_edge_pixels);
        const probe_pixels = probe_context.getImageData(
          0,
          0,
          probe_edge_pixels,
          probe_edge_pixels,
        ).data;
        let painted_pixels = 0;
        for (
          let pixel_index = 0;
          pixel_index < probe_pixels.length;
          pixel_index += pixels_per_rgba_sample
        ) {
          if (
            probe_pixels[pixel_index] > 0 ||
            probe_pixels[pixel_index + 1] > 0 ||
            probe_pixels[pixel_index + 2] > 0
          ) {
            ++painted_pixels;
          }
        }
        return painted_pixels;
      };
      document.getElementById('map-overlay').dispatchEvent(
        new WheelEvent('wheel', { deltaY: 0, cancelable: true }),
      );
      // The freshly presented frame is readable immediately after the
      // compositor picks it up; sample across the following frames.
      return new Promise((resolve_probe) => {
        for (let attempt = 0; attempt < probe_animation_frames; ++attempt) {
          requestAnimationFrame(() => {
            if (paint_probe_canvas() >= minimum_probed_painted_pixels) {
              resolve_probe(true);
            }
          });
        }
        setTimeout(() => resolve_probe(false), 2_000);
      });
    },
    {
      probe_edge_pixels: PROBE_CANVAS_EDGE_PIXELS,
      pixels_per_rgba_sample: PIXELS_PER_RGBA_SAMPLE,
      minimum_probed_painted_pixels: MINIMUM_PROBED_PAINTED_PIXELS,
      probe_animation_frames: PROBE_ANIMATION_FRAMES,
    },
  );
  expect(probe_painted_pixels).toBe(true);
});

test('a transient tile failure is retried after its back-off, not blacklisted', async ({
  page,
}) => {
  const renderer_mode = await resolve_renderer_mode(page);
  if (renderer_mode !== 'gpu') {
    test.skip(true, 'headless WebGPU unavailable; the tile fetch path needs the GPU renderer');
    return;
  }
  await page.waitForFunction(
    () => window.mapLastFrameStats?.()?.missingTiles === 0,
    null,
    { timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS },
  );

  const requested_tile_urls = [];
  let injected_failure = false;
  await page.route('**/tiles/**', (route) => {
    requested_tile_urls.push(route.request().url());
    if (!injected_failure) {
      injected_failure = true;
      return route.fulfill({
        status: 500,
        contentType: 'text/plain',
        body: 'injected transient failure',
      });
    }
    return route.continue();
  });

  // A zoom step asks for addresses the texture pool does not hold, so the
  // canvas starts fresh fetches; the first one fails transiently.
  await page.evaluate(() => {
    const zoom_slider = document.getElementById('zoom-control');
    zoom_slider.value = '13';
    zoom_slider.dispatchEvent(new Event('input', { bubbles: true }));
  });
  await expect
    .poll(() => requested_tile_urls.length, { timeout: 15_000 })
    .toBeGreaterThan(0);
  const transiently_failed_url = requested_tile_urls[0];

  // The base back-off is two seconds; a camera nudge after it must retry the
  // same address, proving the 500 did not blacklist the tile.
  await page.waitForTimeout(2_500);
  await page.evaluate(() =>
    document
      .getElementById('map-overlay')
      .dispatchEvent(new WheelEvent('wheel', { deltaY: 0, cancelable: true })),
  );
  await expect
    .poll(
      () =>
        requested_tile_urls.filter((url) => url === transiently_failed_url)
          .length,
      { timeout: 15_000 },
    )
    .toBeGreaterThanOrEqual(2);
});

test('a rejected frame reports the failure and degrades to the 2D grid', async ({
  page,
}) => {
  const renderer_mode = await resolve_renderer_mode(page);
  if (renderer_mode !== 'gpu') {
    test.skip(true, 'headless WebGPU unavailable; the frame-failure path needs the GPU renderer');
    return;
  }
  // Force the next frame to reject: the page must name the failing surface,
  // degrade to the documented 2D path and keep its render loop alive instead
  // of freezing on an unhandled rejection.
  await page.evaluate(() => {
    window.mapGpuCanvas().renderFrame = async () => {
      throw new Error('forced surface failure');
    };
    window.dispatchEvent(new Event('resize'));
  });
  await page.waitForFunction(() => window.mapRendererMode() === 'overlay', null, {
    timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS,
  });
  const degraded_state = await page.evaluate(() => ({
    detail: window.mapRendererDetail(),
    error_hidden: document.getElementById('error-report').hidden,
    error_text: document.getElementById('error-report').textContent,
    status_text: document.getElementById('status-line').textContent,
  }));
  expect(degraded_state.detail).toContain('forced surface failure');
  expect(degraded_state.error_text).toBe('Render frame: forced surface failure');
  expect(degraded_state.error_hidden).toBe(false);
  expect(degraded_state.status_text).toBe('Degraded: 2D grid');

  // The loop still paints: a camera nudge reaches the 2D readout.
  await page.evaluate(() =>
    document.getElementById('map-overlay').dispatchEvent(
      new WheelEvent('wheel', { deltaY: 0, cancelable: true }),
    ),
  );
  await page.waitForFunction(
    () => document.getElementById('diagnostics-facts-lines').textContent.includes('mode overlay'),
    null,
    { timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS },
  );
});

test('readout center tracks a scripted drag on the map', async ({ page }) => {
  // The readout is developer chrome, opt-in under Advanced → Debug.
  await page.locator('#renderer-readout-control').check();
  await expect(page.locator('#readout')).toBeVisible();
  const renderer_mode = await resolve_renderer_mode(page);
  expect(renderer_mode === 'gpu' || renderer_mode === 'overlay').toBe(true);
  if (renderer_mode === 'gpu') {
    await page.waitForFunction(
      () => window.mapLastFrameStats?.()?.requestedTiles > 0,
      null,
      { timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS },
    );
  }

  const center_before = await page.evaluate(() => {
    // The regex runs inside the page: closure-level helpers do not cross
    // the `page.evaluate` boundary.
    const center_match = /center (-?\d+\.\d+), (-?\d+\.\d+)/.exec(
      document.getElementById('readout').textContent,
    );
    return center_match
      ? { latitude: Number(center_match[1]), longitude: Number(center_match[2]) }
      : null;
  });
  expect(center_before).not.toBeNull();

  // A scripted drag across the viewport center pans the camera: the drag
  // delta moves the ground point under the pointer to the camera center.
  // The gesture starts on open map, clear of the 320 px primary panel and the
  // map-control cluster at the end edge.
  await page.mouse.move(430, 240);
  await page.mouse.down();
  await page.mouse.move(510, 280, { steps: 8 });
  await page.mouse.up();

  await page.waitForFunction(
    (center_before) => {
      const center_after = /center (-?\d+\.\d+), (-?\d+\.\d+)/.exec(
        document.getElementById('readout').textContent,
      );
      if (!center_after) return false;
      // Same regex-in-page constraint as the first read.
      const parsed_center_after = {
        latitude: Number(center_after[1]),
        longitude: Number(center_after[2]),
      };
      return (
        Math.abs(parsed_center_after.latitude - center_before.latitude) > 1e-4 ||
        Math.abs(parsed_center_after.longitude - center_before.longitude) > 1e-4
      );
    },
    center_before,
    { timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS },
  );
});

// --- Playground scene overlays (wasm overlay surface) ---

// The scene switches live in the floating layers panel, the single home for
// the shared overlay scenes.
const MAP_LAYERS_CONTROL_ID = 'map-layers-control';
const SCENE_SWITCH_SELECTOR = '#map-layer-scene-rows [role="switch"]';
const SCENE_REASON_SELECTOR = '#map-layers-scenes-reason';

test('playground scene overlays toggle on and clear on the GPU canvas', async ({
  page,
}) => {
  const page_errors = [];
  page.on('pageerror', (page_error) => page_errors.push(String(page_error)));
  const renderer_mode = await resolve_renderer_mode(page);
  if (renderer_mode !== 'gpu') {
    // Documented degradation: the 2D path draws no scene overlays, so the
    // layers panel must disable its scene switches with a visible reason.
    await page.click(`#${MAP_LAYERS_CONTROL_ID}`);
    await expect(page.locator(SCENE_SWITCH_SELECTOR).first()).toBeDisabled();
    await expect(page.locator(SCENE_REASON_SELECTOR)).toContainText('WebGPU');
    test.skip(true, 'headless WebGPU unavailable; the 2D path draws no scene overlays');
    return;
  }

  await page.waitForFunction(
    () => (window.mapLastFrameStats?.()?.requestedTiles ?? 0) > 0,
    null,
    { timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS },
  );
  // No overlay toggle is checked by default: the first settled frame draws
  // the tiles alone.
  expect(
    await page.evaluate(() => window.mapLastFrameStats().overlayTriangles),
  ).toBe(0);

  // Enabling a scene switch applies its scene and the next rendered frame
  // carries its triangles.
  await page.click(`#${MAP_LAYERS_CONTROL_ID}`);
  const first_scene_switch = page.locator(SCENE_SWITCH_SELECTOR).first();
  await first_scene_switch.click();
  await page.waitForFunction(
    () => (window.mapLastFrameStats?.()?.overlayTriangles ?? 0) > 0,
    null,
    { timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS },
  );

  // Switching it off again empties the canvas' overlay list.
  await first_scene_switch.click();
  await page.waitForFunction(() => {
    const statistics = window.mapLastFrameStats?.();
    return statistics && statistics.overlayTriangles === 0 ? statistics : null;
  }, null, { timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS });

  expect(page_errors, 'overlay toggling must raise no page errors').toEqual([]);
});

// --- Retina tile resolution (device-pixel-ratio seam) ---

// The standard-resolution sibling of the retina fixtures, so the test can
// prove the URL difference and not just a differing byte size.
const STANDARD_FIXTURE_URL_PATTERN = /\/tiles\/\d+_\d+_\d+\.png$/;
const RETINA_FIXTURE_URL_PATTERN = /\/tiles\/\d+_\d+_\d+@2x\.png$/;
// Same viewport as the rest of the suite; the retina run differs only in
// device scale factor.
const RETINA_VIEWPORT = { width: 640, height: 480 };
const RETINA_DEVICE_SCALE_FACTOR = 2;

test('retina displays fetch the double-resolution tile fixtures', async ({
  browser,
}) => {
  // The suite's default context runs at device scale one; retina starts a
  // dedicated doubled context (a `test.use` override would apply too late:
  // the shared `beforeEach` already consumed the page fixture).
  const retina_context = await browser.newContext({
    channel: 'chromium',
    viewport: RETINA_VIEWPORT,
    deviceScaleFactor: RETINA_DEVICE_SCALE_FACTOR,
  });
  const page = await retina_context.newPage();
  // Collect the tile fixture URLs the canvas fetches across its frames.
  const requested_tile_urls = [];
  page.on('request', (request) => {
    const request_url = request.url();
    if (request_url.includes('/tiles/')) {
      requested_tile_urls.push(request_url);
    }
  });

  await page.goto('/?advanced');
  await waitForMapTestReady(page);
  const renderer_mode = await resolve_renderer_mode(page);
  if (renderer_mode !== 'gpu') {
    await retina_context.close();
    test.skip(true, 'headless WebGPU unavailable; the overlay path has no tile fetches');
    return;
  }

  const frame_statistics = await page
    .waitForFunction(
      () => {
        const statistics = window.mapLastFrameStats?.();
        return statistics && statistics.requestedTiles > 0 && statistics.drawnTiles > 0
          ? statistics
          : null;
      },
      null,
      { timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS },
    )
    .then((statistics_handle) => statistics_handle.jsonValue());

  // The doubled pixel ratio must have selected the retina resolution seam.
  expect(await page.evaluate(() => window.devicePixelRatio)).toBe(
    RETINA_DEVICE_SCALE_FACTOR,
  );
  expect(frame_statistics.tileResolution).toBe('retina');
  expect(frame_statistics.drawnTiles).toBeGreaterThan(0);

  // Give the serialized render queue a settled frame, then close the
  // context before asserting so the URL list stops growing.
  await page.waitForTimeout(250);
  await retina_context.close();

  expect(
    requested_tile_urls.some((request_url) =>
      RETINA_FIXTURE_URL_PATTERN.test(request_url),
    ),
    `the retina context must fetch a @2x fixture: ${JSON.stringify(
      requested_tile_urls,
    )}`,
  ).toBe(true);
  // Sanity over the same observation: only recorded fixture addresses.
  expect(requested_tile_urls.length).toBeGreaterThan(0);
  for (const request_url of requested_tile_urls) {
    expect(
      RETINA_FIXTURE_URL_PATTERN.test(request_url) ||
        STANDARD_FIXTURE_URL_PATTERN.test(request_url),
    ).toBe(true);
  }
});

// --- UX language (docs/ReferenceAppUX.md): the refapp states worth locking ---

// Semantic hooks of the UX-language contract (§10.1); ids may be renamed
// only with this spec updated in the same change.
const NAVIGATION_DEMO_PANEL_ID = 'navigation-demo-panel';
const NAVIGATION_DEMO_REASON_ID = 'navigation-demo-reason';
const NAVIGATION_DEMO_PREVIEW_NOTICE_ID = 'navigation-demo-preview-notice';
const CAMERA_PRESET_SELECTOR_BY_ID = (preset_id) =>
  `#camera-preset-controls [data-camera-preset-id='${preset_id}']`;
// The shared artifact's route-preview preset (platform/reference-scenes.json),
// restated here so the test fails on an artifact drift instead of silently
// passing.
const ROUTE_PREVIEW_PRESET_ID = 'route-preview';
const ROUTE_PREVIEW_CAMERA = {
  latitude_degrees: 38.577235,
  longitude_degrees: -121.49405,
  zoom_level: 14,
  bearing_degrees: 30,
  pitch_degrees: 30,
};
const PRESET_CAMERA_TOLERANCE_DEGREES = 1e-7;
// The phone-width pass's viewport (the narrowest phone-class width).
const PHONE_VIEWPORT = { width: 360, height: 740 };

test('the theme control cycles Light, Dark and Auto, and Auto follows the OS', async ({
  page,
}) => {
  // The owner ruling of 2026-10-30: Auto is the default and follows the
  // operating system live; an explicit Light or Dark choice is remembered.
  await page.emulateMedia({ colorScheme: 'dark' });
  await page.goto('/');
  await waitForMapTestReady(page);
  // The appearance switch lives in the settings sheet behind the gear.
  await page.click('#settings-control');
  expect(await page.evaluate(
    () => document.getElementById('settings-dialog').open)).toBe(true);
  // Auto by default: the resolved theme follows the dark OS preference.
  expect(
    await page.evaluate(() => [
      document.documentElement.dataset.theme,
      document.documentElement.dataset.themeChoice,
    ]),
  ).toEqual(['dark', 'auto']);
  // The first click picks Light explicitly.
  await page.click('#theme-toggle-control');
  expect(
    await page.evaluate(() => [
      document.documentElement.dataset.theme,
      document.documentElement.dataset.themeChoice,
    ]),
  ).toEqual(['light', 'light']);
  // The second picks Dark explicitly.
  await page.click('#theme-toggle-control');
  expect(
    await page.evaluate(() => [
      document.documentElement.dataset.theme,
      document.documentElement.dataset.themeChoice,
      document.getElementById('theme-toggle-control').textContent,
    ]),
  ).toEqual(['dark', 'dark', 'Switch to auto theme']);
  // The third returns to Auto, which follows the OS again — live.
  await page.click('#theme-toggle-control');
  expect(await page.evaluate(() => document.documentElement.dataset.themeChoice))
    .toBe('auto');
  await page.emulateMedia({ colorScheme: 'light' });
  await expect
    .poll(() => page.evaluate(() => document.documentElement.dataset.theme))
    .toBe('light');
  // Escape closes the modal and returns focus to the gear.
  await page.keyboard.press('Escape');
  expect(await page.evaluate(
    () => document.getElementById('settings-dialog').open)).toBe(false);
  expect(await page.evaluate(() => document.activeElement?.id))
    .toBe('settings-control');
});

test('status line walks the readiness ladder and reports the renderer mode', async ({
  page,
}) => {
  const renderer_mode = await resolve_renderer_mode(page);
  // Sample only a settled line: the flip can sit a tick behind the
  // renderer-mode resolution (the first sample proved racy on slower
  // runners and slower-fast in the other direction locally).
  await page.waitForFunction(
    () =>
      ['ready', 'degraded', 'failed'].includes(
        document.getElementById('status-line').dataset.readiness,
      ),
    null,
    { timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS },
  );
  const readiness = await page.evaluate(() => ({
    readiness: document.getElementById('status-line').dataset.readiness,
    text: document.getElementById('status-line').textContent,
    harness_text: document.getElementById('harness-status').textContent,
  }));
  expect(readiness.readiness === 'ready' || readiness.readiness === 'degraded')
    .toBe(true, `readiness must be ready or degraded, was: ${readiness.readiness}`);
  // The first-run line is a plain human state; the renderer mode, adapter
  // and version detail live in the diagnostics block instead.
  expect(readiness.text).toBe(
    renderer_mode === 'gpu' ? 'Ready' : 'Degraded: 2D grid');
  expect(readiness.harness_text).toContain('ready (version');
  const diagnostics_text = await page.locator('#diagnostics-facts-lines').textContent();
  expect(diagnostics_text).toContain(`mode ${renderer_mode}`);
  expect(diagnostics_text).toContain('version ');
  // The ladder transition is observed mutation-time: the line left
  // `loading` (the served HTML's initial value) exactly once toward the
  // settled state, no matter whether the flip beat the sampling or not.
  const ladder_records = await page.evaluate(() => window.__readiness_ladder);
  expect(ladder_records.length).toBeGreaterThanOrEqual(1);
  expect(ladder_records[0].from).toBe('loading');
  expect(ladder_records[ladder_records.length - 1].to).toBe(
    readiness.readiness,
  );
});

test('scene switches are enabled or disabled-with-reason only after readiness', async ({
  page,
}) => {
  const renderer_mode = await resolve_renderer_mode(page);
  await page.click(`#${MAP_LAYERS_CONTROL_ID}`);
  const first_scene_switch = page.locator(SCENE_SWITCH_SELECTOR).first();
  if (renderer_mode === 'overlay') {
    // Degraded session: the switches stay disabled with the reason showing
    // (the same honest pattern the degradation test above asserts).
    await expect(
      first_scene_switch,
      'the degraded session must keep the scene switches disabled',
    ).toBeDisabled();
    await expect(page.locator(SCENE_REASON_SELECTOR)).toContainText('WebGPU');
    return;
  }
  // GPU session: enabled, no reason shown.
  await expect(first_scene_switch).toBeEnabled();
  await expect(page.locator(SCENE_REASON_SELECTOR)).toBeHidden();
});

test('camera preset picker applies the shared artifact camera to the live state', async ({
  page,
}) => {
  await resolve_renderer_mode(page);
  await page.click(CAMERA_PRESET_SELECTOR_BY_ID(ROUTE_PREVIEW_PRESET_ID));
  // The preset eases every pose component (the native `animateCamera`
  // behaviour); the settled pose is the assertion target.
  await expect
    .poll(async () => {
      const state = await page.evaluate(() => window.mapCameraState());
      return Math.abs(state.latitude - ROUTE_PREVIEW_CAMERA.latitude_degrees)
          < PRESET_CAMERA_TOLERANCE_DEGREES
        && Math.abs(state.longitude - ROUTE_PREVIEW_CAMERA.longitude_degrees)
          < PRESET_CAMERA_TOLERANCE_DEGREES
        && state.zoom === ROUTE_PREVIEW_CAMERA.zoom_level
        && state.bearing === ROUTE_PREVIEW_CAMERA.bearing_degrees
        && state.pitch === ROUTE_PREVIEW_CAMERA.pitch_degrees;
    }, { timeout: 5_000 })
    .toBe(true);
  const applied = await page.evaluate(() => window.mapCameraState());
  expect(Math.abs(applied.latitude - ROUTE_PREVIEW_CAMERA.latitude_degrees))
    .toBeLessThan(PRESET_CAMERA_TOLERANCE_DEGREES);
  expect(Math.abs(applied.longitude - ROUTE_PREVIEW_CAMERA.longitude_degrees))
    .toBeLessThan(PRESET_CAMERA_TOLERANCE_DEGREES);
  expect(applied.zoom).toBe(ROUTE_PREVIEW_CAMERA.zoom_level);
  expect(applied.bearing).toBe(ROUTE_PREVIEW_CAMERA.bearing_degrees);
  expect(applied.pitch).toBe(ROUTE_PREVIEW_CAMERA.pitch_degrees);
  // The slider and the selected chip follow the applied preset.
  expect(
    await page.evaluate(
      ({ preset_selector }) => ({
        zoom_slider: document.getElementById('zoom-control').value,
        selected: document.querySelector(preset_selector).classList.contains('selected'),
      }),
      { preset_selector: CAMERA_PRESET_SELECTOR_BY_ID(ROUTE_PREVIEW_PRESET_ID) },
    ),
  ).toEqual({ zoom_slider: '14', selected: true });
});

test('simulated navigation panel is idle until a trip starts, with no preview samples', async ({
  page,
}) => {
  // The Navigator is on the wasm seam (round 43), so the panel renders live
  // snapshots only: before a trip it states what to do and shows no sample
  // values, and the retired layout-preview notice is gone
  // (docs/ReferenceAppUX.md §7.4). The live trip is covered by
  // tests/navigation_demo.spec.mjs.
  const panel_presentation = await page.evaluate(
    ({ navigation_panel_id, navigation_reason_id, navigation_preview_notice_id }) => {
      document.getElementById(navigation_panel_id).open = true;
      return {
        reason: document.getElementById(navigation_reason_id).textContent,
        reason_hidden: document.getElementById(navigation_reason_id).hidden,
        preview_notice: document.getElementById(navigation_preview_notice_id).textContent,
        preview_notice_hidden:
          document.getElementById(navigation_preview_notice_id).hidden,
        banner_text: document.getElementById('banner-instruction-text').textContent,
        phase_chip: document.getElementById('navigation-phase-value').textContent,
        snapshot: window.mapNavigationSnapshot(),
      };
    },
    {
      navigation_panel_id: NAVIGATION_DEMO_PANEL_ID,
      navigation_reason_id: NAVIGATION_DEMO_REASON_ID,
      navigation_preview_notice_id: NAVIGATION_DEMO_PREVIEW_NOTICE_ID,
    },
  );
  expect(panel_presentation.reason).toBe(
    'Plan a route, then start the simulated trip to see live guidance.');
  expect(panel_presentation.reason_hidden).toBe(false);
  expect(panel_presentation.preview_notice_hidden).toBe(true);
  expect(panel_presentation.preview_notice).toBe('');
  expect(panel_presentation.banner_text).toBe('');
  expect(panel_presentation.phase_chip).toBe('Not started');
  expect(panel_presentation.snapshot).toBeNull();
});

test('phone-width viewport keeps the layout inside one scroll axis', async ({ page }) => {
  await resolve_renderer_mode(page);
  await page.setViewportSize(PHONE_VIEWPORT);
  const layout_measurement = await page.evaluate(() => ({
    scroll_width: document.documentElement.scrollWidth,
    inner_width: window.innerWidth,
    controls_rectangle_end: document.getElementById('controls').getBoundingClientRect().right,
  }));
  expect(layout_measurement.scroll_width).toBeLessThanOrEqual(layout_measurement.inner_width);
  expect(layout_measurement.controls_rectangle_end).toBeLessThanOrEqual(
    layout_measurement.inner_width,
  );
});
