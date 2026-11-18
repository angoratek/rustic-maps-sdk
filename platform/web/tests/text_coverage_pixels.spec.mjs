// The browser half of the SDF edge proof (M30 Slice D): render the committed
// OpenMapTiles fixture through the canvas-owned vector source with the base
// map hidden and read the freshly presented frame back through a 2D canvas.
// The style's own `place` symbol layer supplies the text (the renderer-internal
// label ruling), so no page-side label list or glyph range reaches the canvas.
// The frame is measured with the same coverage profile the headless golden
// test measures: a 1× and a 2× display must both keep the stroke interior at
// ≥ 0.98 coverage and the transition beside the stroke at no more than two
// device pixels. The style's point size is the same at both ratios; only its
// device size doubles.
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

/** The probe frame's CSS edge in pixels. */
const PROBE_CSS_EDGE_PIXELS = 256;
/** Coverage a stroke interior must reach. */
const COVERAGE_INTERIOR_MINIMUM = 0.98;
/** Coverage below this is background. */
const COVERAGE_EDGE_MINIMUM = 0.05;
/** Coverage above this is plateau, not transition. */
const COVERAGE_EDGE_MAXIMUM = 0.95;
/** The widest transition a stroke edge may span, in device pixels. */
const MAXIMUM_EDGE_RUN_PIXELS = 2;
/**
 * The place feature the probe centers on: the OpenMapTiles fixture's
 * `place` feature for West Sacramento. The probe style renders it as a
 * literal `I`, so the camera centers exactly one glyph.
 */
const PROBE_LATITUDE_DEGREES = 38.580462965604134;
const PROBE_LONGITUDE_DEGREES = -121.53023600578308;
const PROBE_ZOOM_LEVEL = 18;
/** The committed fixture route the dev server serves same-origin. */
const VECTOR_TILE_TEMPLATE_PATH = '/golden-vector/{z}/{x}/{y}.pbf';
/**
 * The committed measurement style the probe paints: one literal white `I` at
 * 48 point pixels on black, styled over the fixture's place layer.
 */
const VECTOR_STYLE_ID = 'label-coverage-probe';
/** The fixture's own address zoom; the camera zoom is independent of it. */
const VECTOR_MAXIMUM_ZOOM_LEVEL = 14;
/** Frames the vector base may need to fetch, decode and place its labels. */
const VECTOR_SETTLE_ATTEMPTS = 60;
/** The literal glyph text the probe style paints. */
const PROBE_LABEL_TEXT = 'I';

/** Renders the vector-base label frame in-page and returns its pixels. */
async function probe_frame_pixels(page, device_pixel_ratio) {
  return page.evaluate(
    async ([
      css_edge,
      device_pixel_ratio,
      latitude,
      longitude,
      zoom,
      template_path,
      style_id,
      maximum_zoom_level,
      settle_attempts,
      expected_label_text,
    ]) => {
      const canvas = document.createElement('canvas');
      canvas.style.width = `${css_edge}px`;
      canvas.style.height = `${css_edge}px`;
      canvas.width = css_edge * device_pixel_ratio;
      canvas.height = css_edge * device_pixel_ratio;
      canvas.style.position = 'fixed';
      canvas.style.left = '-10000px';
      document.body.appendChild(canvas);

      const map_view = new window.mapTest.MapView(canvas, window.mapTest.maps);
      await map_view.initialize();
      // The installed vector source hides the raster base and paints the
      // style's own background; the page supplies no tiles, labels or glyphs.
      map_view.setVectorTileSource({
        urlTemplate: `${window.location.origin}${template_path}`,
        sourceName: 'coverage-probe',
        attributionText: 'Committed OpenMapTiles fixture',
        styleId: style_id,
        minimumZoomLevel: 0,
        maximumZoomLevel: maximum_zoom_level,
      });

      let label_glyph_triangles = 0;
      let label_texts = [];
      for (let attempt = 0; attempt < settle_attempts; attempt += 1) {
        // `renderFrame` consumes the camera by value, so every frame builds
        // its own (freeing a moved binding would be a double free).
        const camera = new window.mapTest.MapCamera(
          latitude, longitude, zoom, 0, 0);
        const frame_statistics = await map_view.renderFrame(camera, 60);
        label_glyph_triangles = frame_statistics.labelGlyphTriangles ?? 0;
        label_texts = frame_statistics.labelTexts ?? [];
        if (label_glyph_triangles > 0) {
          break;
        }
        // A presenting frame starts its tile fetches and returns; yield so
        // the prefetch can deliver the fixture before the next frame.
        await new Promise((resolve) => requestAnimationFrame(resolve));
      }
      if (label_glyph_triangles === 0) {
        throw new Error(
          'the vector base drew no label glyphs within the settle budget',
        );
      }
      if (!label_texts.includes(expected_label_text)) {
        throw new Error(
          `the vector base did not paint the probe glyph ` +
            `${expected_label_text}: ${JSON.stringify(label_texts)}`,
        );
      }
      // drawImage on a WebGPU canvas reads the freshly presented frame only;
      // sample the next animation frame's snapshot (the golden harness's
      // same rule).
      await new Promise((resolve) => requestAnimationFrame(resolve));
      const probe_canvas = document.createElement('canvas');
      probe_canvas.width = canvas.width;
      probe_canvas.height = canvas.height;
      const probe_context = probe_canvas.getContext('2d');
      probe_context.drawImage(canvas, 0, 0);
      const image_data = probe_context.getImageData(
        0, 0, probe_canvas.width, probe_canvas.height);
      canvas.remove();
      return {
        width: probe_canvas.width,
        height: probe_canvas.height,
        bytes: Array.from(image_data.data),
        labelGlyphTriangles: label_glyph_triangles,
        labelTexts: label_texts,
      };
    },
    [
      PROBE_CSS_EDGE_PIXELS,
      device_pixel_ratio,
      PROBE_LATITUDE_DEGREES,
      PROBE_LONGITUDE_DEGREES,
      PROBE_ZOOM_LEVEL,
      VECTOR_TILE_TEMPLATE_PATH,
      VECTOR_STYLE_ID,
      VECTOR_MAXIMUM_ZOOM_LEVEL,
      VECTOR_SETTLE_ATTEMPTS,
      PROBE_LABEL_TEXT,
    ],
  );
}

/** The coverage profile of one frame, mirroring the headless test's math. */
function coverage_profile(frame) {
  const pixel_luminance = (offset) =>
    (frame.bytes[offset] + frame.bytes[offset + 1] + frame.bytes[offset + 2]) / 3;
  const pixel_count = frame.width * frame.height;
  const luminances = [];
  for (let pixel = 0; pixel < pixel_count; pixel += 1) {
    luminances.push(pixel_luminance(pixel * 4));
  }
  const corner_luminances = [
    luminances[0],
    luminances[frame.width - 1],
    luminances[(frame.height - 1) * frame.width],
    luminances[frame.height * frame.width - 1],
  ];
  const background =
    corner_luminances.reduce((sum, value) => sum + value, 0)
    / corner_luminances.length;
  const text = luminances.reduce(
    (maximum, value) => Math.max(maximum, value), background);
  expect(text).toBeGreaterThan(background + 1);
  const coverage = luminances.map(
    (value) => Math.min(1, Math.max(0, (value - background) / (text - background))));
  const coverage_at = (x, y) => coverage[y * frame.width + x];

  let best = null;
  for (let y = 0; y < frame.height; y += 1) {
    let x = 0;
    while (x < frame.width) {
      if (coverage_at(x, y) < COVERAGE_INTERIOR_MINIMUM) {
        x += 1;
        continue;
      }
      const run_start = x;
      while (x < frame.width && coverage_at(x, y) >= COVERAGE_INTERIOR_MINIMUM) {
        x += 1;
      }
      const run_length = x - run_start;
      if (best === null || run_length > best.run_length) {
        best = { run_length, run_start, row: y };
      }
    }
  }
  expect(best).not.toBeNull();

  let left_edge = 0;
  let left_x = best.run_start;
  while (
    left_x > 0
    && coverage_at(left_x - 1, best.row) > COVERAGE_EDGE_MINIMUM
    && coverage_at(left_x - 1, best.row) < COVERAGE_EDGE_MAXIMUM
  ) {
    left_edge += 1;
    left_x -= 1;
  }
  let right_x = best.run_start + best.run_length;
  let right_edge = 0;
  while (
    right_x < frame.width
    && coverage_at(right_x, best.row) > COVERAGE_EDGE_MINIMUM
    && coverage_at(right_x, best.row) < COVERAGE_EDGE_MAXIMUM
  ) {
    right_edge += 1;
    right_x += 1;
  }

  const interior_pixel_count = coverage.filter(
    (value) => value >= COVERAGE_INTERIOR_MINIMUM).length;
  return { interior_pixel_count, left_edge, right_edge };
}

/** Asserts the profile facts for one rendered probe frame. */
function expect_crisp_profile(frame, device_pixel_ratio) {
  expect(frame.width).toBe(PROBE_CSS_EDGE_PIXELS * device_pixel_ratio);
  expect(frame.labelGlyphTriangles).toBeGreaterThan(0);
  expect(frame.labelTexts).toContain(PROBE_LABEL_TEXT);
  const profile = coverage_profile(frame);
  expect(
    profile.interior_pixel_count,
    `the stroke must reach full coverage at ${device_pixel_ratio}×`,
  ).toBeGreaterThan(0);
  expect(
    profile.left_edge,
    `the left stroke edge at ${device_pixel_ratio}×`,
  ).toBeLessThanOrEqual(MAXIMUM_EDGE_RUN_PIXELS);
  expect(
    profile.right_edge,
    `the right stroke edge at ${device_pixel_ratio}×`,
  ).toBeLessThanOrEqual(MAXIMUM_EDGE_RUN_PIXELS);
}

test.describe('at a 1× display', () => {
  test.use({
    viewport: { width: 800, height: 600 },
    deviceScaleFactor: 1,
  });

  test('the vector base draws a full-coverage stroke with a two-pixel edge', async ({
    page,
  }) => {
    await page.goto('/?advanced');
    await waitForMapTestReady(page);
    expect_crisp_profile(await probe_frame_pixels(page, 1), 1);
  });
});

test.describe('at a 2× display', () => {
  test.use({
    viewport: { width: 800, height: 600 },
    deviceScaleFactor: 2,
  });

  test('the same point size keeps its full interior and a device-pixel edge', async ({
    page,
  }) => {
    await page.goto('/?advanced');
    await waitForMapTestReady(page);
    expect_crisp_profile(await probe_frame_pixels(page, 2), 2);
  });
});
