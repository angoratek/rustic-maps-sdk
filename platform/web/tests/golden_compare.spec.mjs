// Browser canvas-vs-golden compare: the wasm WebGPU canvas renders the two
// committed golden scenes and the four committed Sacramento overlay scenes
// (`crates/rustic-maps-rendering/fixtures/render/golden/`) on a dedicated 512x512
// fixture page and the test asserts the same tolerances the native golden
// harnesses assert — an in-page pixel loop over `drawImage`-probed surface
// bytes, never a screenshot. The overlay scenes are the `overlay_golden.rs`
// scene vocabulary (route line, polygon fill, point markers, traffic flow),
// applied to the wasm canvas through the shared `overlay_scenes.mjs` data.
//
// Mirrored constants, flagged with `golden_image.rs` / `overlay_golden.rs`,
// MUST stay in sync with `crates/rustic-maps-rendering/tests/golden_image.rs` and
// `crates/rustic-maps-rendering/tests/overlay_golden.rs`: the native renderer is the
// reference and the browser probe is the check, so widening either side
// alone breaks the parity contract.
import { expect, test } from './network_guard.mjs';

// Frame edge in pixels for both golden scenes (`golden_image.rs`
// GOLDEN_VIEWPORT_EDGE_PIXELS) — also the page's fixed canvas extent.
const GOLDEN_VIEWPORT_EDGE_PIXELS = 512;
// The mean absolute per-channel error (0..255 scale) tolerated between the
// rendered frame and its golden fixture (`golden_image.rs`
// MEAN_ABSOLUTE_ERROR_TOLERANCE).
const MEAN_ABSOLUTE_ERROR_TOLERANCE = 2.0;
// Per-pixel absolute channel error (0..255 scale) above which a pixel
// counts as an outlier (`golden_image.rs` OUTLIER_CHANNEL_ERROR).
const OUTLIER_CHANNEL_ERROR = 24.0;
// Fraction of outlier pixels tolerated (`golden_image.rs`
// OUTLIER_PIXEL_FRACTION_LIMIT).
const OUTLIER_PIXEL_FRACTION_LIMIT = 0.02;
// Pitch of the two golden cameras (`golden_image.rs`
// TOP_DOWN_PITCH_DEGREES / PITCHED_PITCH_DEGREES).
const TOP_DOWN_PITCH_DEGREES = 0;
const PITCHED_PITCH_DEGREES = 45;
// The golden fixture files, named exactly as the `/golden` route
// allow-lists them (`golden_image.rs` TOP_DOWN_GOLDEN_FILE /
// PITCHED_GOLDEN_FILE).
const TOP_DOWN_GOLDEN_FILE = 'sacramento_zoom14_top_down.png';
const PITCHED_GOLDEN_FILE = 'sacramento_zoom14_pitched45.png';
// Frame statistics each settled golden scene must reach. Both cameras list
// a trailing tile (14/2663/6286) that the fixture page deliberately resolves
// through its zoom-13 parent via `mapGoldenStageTileParentFallback`
// (`golden_image.rs` `fill_pool_and_prepare`), so it still draws; the pitched
// camera additionally lists 14/2661/6286, whose projected span lies wholly
// outside the viewport at pitch 45. The quad assembler culls that one
// requested tile, but it has pooled imagery, so it is a draw-list property
// and not a missing tile: `missingTiles` reports tiles the scheduler could
// not resolve, never tiles the frame culled.
const SCENE_EXPECTED_TILES_BY_PITCH_DEGREES = new Map([
  [TOP_DOWN_PITCH_DEGREES, { drawnTiles: 4, missingTiles: 0 }],
  [PITCHED_PITCH_DEGREES, { drawnTiles: 8, missingTiles: 0 }],
]);

// The default playground camera sits at the fixtures' highest recorded
// zoom, as in the smoke test.
const GOLDEN_SCENE_TILE_ZOOM_LEVEL = 14;
// The overlay scenes, keyed exactly as `golden.mjs`'s
// `mapGoldenSetOverlayScene` accepts them, with the expected frame statistics
// each scene reaches (the same camera as the top-down tile scene, so the tile
// counts equal its 4-drawn/0-missing entry) and the committed fixture each
// scene diffs against. The expected overlay-triangle counts mirror the native
// `overlay_golden.rs` scene assembly, computed from its same shapes: the
// route's two segments and the polygon's two ear-clip triangles each fill one
// world-copy fragment, every marker is an eight-triangle octagon fan, and
// each of the four traffic flows thicken into one two-triangle quad.
const OVERLAY_SCENE_EXPECTATIONS = [
  {
    scene_key: 'route',
    golden_file: 'sacramento_zoom14_route_line.png',
    // Two segment quads (two triangles each) plus one square join at the
    // scene's single interior vertex.
    expected_overlay_triangles: 6,
  },
  {
    scene_key: 'polygon',
    golden_file: 'sacramento_zoom14_polygon_fill.png',
    expected_overlay_triangles: 2,
  },
  {
    scene_key: 'markers',
    golden_file: 'sacramento_zoom14_markers.png',
    expected_overlay_triangles: 24,
  },
  {
    scene_key: 'traffic',
    golden_file: 'sacramento_zoom14_traffic_flow.png',
    expected_overlay_triangles: 8,
  },
];
// The rendered probe scenes settle through real GPU initialization; only a
// slow first browser init can stretch these waits, as in the smoke test.
const RENDERER_READINESS_TIMEOUT_MILLISECONDS = 30_000;
// Precondition budget for the golden fixture decodes on page readiness.
const GOLDEN_DECODE_TIMEOUT_MILLISECONDS = 10_000;

// The readiness payload also carries the decoded golden sizes, so the
// 512x512 precondition cannot pass while the fixtures changed shape.
test.beforeEach(async ({ page }) => {
  await page.goto('/golden.html');
  await page.evaluate(() => window.mapGoldenReady);
});

// Waits for the fixture page's documented renderer-mode resolution and
// ensures the scenes will render: no browser WebGPU degrades to the overlay
// path, which the test asserts and skips on.
async function require_gpu_renderer(page) {
  await page.waitForFunction(
    () =>
      window.mapGoldenStatus().rendererMode === 'gpu' ||
      window.mapGoldenStatus().rendererMode === 'overlay',
    null,
    { timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS },
  );
  const status = await page.evaluate(() => window.mapGoldenStatus());
  if (status.rendererMode !== 'gpu') {
    test.skip(
      true,
      `headless WebGPU unavailable in this browser; degradation asserted: ${JSON.stringify(status)}`,
    );
    return null;
  }
  return status;
}

// The scene's settle budget: the wasm surface fetches at most 32 recorded
// tiles per render (its `MAXIMUM_TILE_FETCHES_PER_FRAME` in
// `crates/rustic-maps-web-bindings/src/canvas.rs`), so the first render may leave
// requested tiles unfetched; re-render until the scheduler catches up
// before asserting.
const MAXIMUM_SETTLE_RENDER_FRAMES = 8;

// Preconditions shared by the two scenes: the committed golden decodes at
// the golden viewport edge (a shape mismatch would read as drift before the
// compare ever runs).
async function assert_scene_preconditions(page, scene_description) {
  const readiness = await page.waitForFunction(() => window.mapGoldenReady, null, {
    timeout: GOLDEN_DECODE_TIMEOUT_MILLISECONDS,
  }).then((handle) => handle.jsonValue());
  for (const golden_sizes of Object.values(readiness.goldenSizes)) {
    expect(golden_sizes.width_pixels, `${scene_description} golden width`).toBe(
      GOLDEN_VIEWPORT_EDGE_PIXELS,
    );
    expect(golden_sizes.height_pixels, `${scene_description} golden height`).toBe(
      GOLDEN_VIEWPORT_EDGE_PIXELS,
    );
  }
  return readiness;
}

// Renders the scene until it reaches its expected tile statistics (the wasm
// fetch budget spreads the decodes over up to `MAXIMUM_TILE_FETCHES_PER_FRAME`
// frames, mirrored above), stages the scene's parent-fallback tile first —
// the deliberate fallback the committed goldens were rendered with — then
// asserts the frame statistics and probes the surface, asserting the
// mirrored golden tolerances on the best sample. `expected_overlay_triangles`
// pins the overlay layer's drawn triangle count for the scene (zero for the
// tiles-only scenes, the same empty-list rule the native `overlay_golden.rs`
// degradation test covers).
async function assert_scene_matches_golden(
  page,
  pitch_degrees,
  golden_file_name,
  scene_description,
  expected_overlay_triangles,
) {
  await page.evaluate(
    (pitch) => window.mapGoldenStageTileParentFallback(pitch),
    pitch_degrees,
  );
  const expected_tiles = SCENE_EXPECTED_TILES_BY_PITCH_DEGREES.get(pitch_degrees);
  let frame_statistics = null;
  for (let settle_frame = 0; settle_frame < MAXIMUM_SETTLE_RENDER_FRAMES; ++settle_frame) {
    frame_statistics = await page.evaluate(
      (pitch) => window.mapGoldenRenderFrame(pitch),
      pitch_degrees,
    );
    if (
      frame_statistics.drawnTiles === expected_tiles.drawnTiles &&
      frame_statistics.missingTiles === expected_tiles.missingTiles
    ) {
      break;
    }
  }
  expect(
    frame_statistics.tileZoomLevel,
    `${scene_description} fetch zoom`,
  ).toBe(GOLDEN_SCENE_TILE_ZOOM_LEVEL);
  expect(
    frame_statistics.drawnTiles,
    `${scene_description} drawn tile count`,
  ).toBe(expected_tiles.drawnTiles);
  expect(
    frame_statistics.missingTiles,
    `${scene_description} missing tile count`,
  ).toBe(expected_tiles.missingTiles);
  expect(
    frame_statistics.overlayTriangles,
    `${scene_description} overlay triangle count`,
  ).toBe(expected_overlay_triangles);

  const probe_metrics = await page.evaluate(
    ({ pitch, golden_file, outlier_channel_error }) =>
      window.mapGoldenProbe(pitch, golden_file, outlier_channel_error),
    {
      pitch: pitch_degrees,
      golden_file: golden_file_name,
      outlier_channel_error: OUTLIER_CHANNEL_ERROR,
    },
  );
  const error_description = `${scene_description}: mean absolute channel error ` +
    `${probe_metrics.meanAbsoluteError.toFixed(4)} (limit ` +
    `${MEAN_ABSOLUTE_ERROR_TOLERANCE}), outlier fraction ` +
    `${probe_metrics.outlierPixelFraction.toFixed(5)} (limit ` +
    `${OUTLIER_PIXEL_FRACTION_LIMIT}) over ` +
    `${probe_metrics.sampleAttempts} sampled frames (best sample ` +
    `${probe_metrics.bestSampleAttempt})`;
  expect(
    probe_metrics.meanAbsoluteError,
    `${scene_description} frame drifted from its golden fixture: ${error_description}`,
  ).toBeLessThanOrEqual(MEAN_ABSOLUTE_ERROR_TOLERANCE);
  expect(
    probe_metrics.outlierPixelFraction,
    `${scene_description} frame drifted from its golden fixture: ${error_description}`,
  ).toBeLessThanOrEqual(OUTLIER_PIXEL_FRACTION_LIMIT);
  // Every sampled frame must have been readable: a probe that never found
  // a plausible sample is a probe mechanics failure, not a tolerance miss.
  expect(
    probe_metrics.sampleAttempts,
    `${scene_description} probe must sample its full frame budget: ${error_description}`,
  ).toBeGreaterThan(0);
}

test('browser WebGPU canvas matches the committed top-down golden fixture', async ({ page }) => {
  await assert_scene_preconditions(page, 'the top-down scene');
  const renderer_status = await require_gpu_renderer(page);
  if (renderer_status === null) {
    return;
  }
  await assert_scene_matches_golden(
    page,
    TOP_DOWN_PITCH_DEGREES,
    TOP_DOWN_GOLDEN_FILE,
    'the top-down scene',
    0,
  );
});

// The wasm canvas uploads each layer through a reusable buffer slot that
// grows only when a payload outgrows its capacity, so a settled scene must
// render identical frames without allocating again. The frame statistics
// report the canvas' cumulative allocation count, which makes that property
// observable here instead of resting on inspection.
test('a settled golden scene reuses its GPU buffers instead of allocating per frame', async ({
  page,
}) => {
  const renderer_status = await require_gpu_renderer(page);
  if (renderer_status === null) {
    return;
  }
  await page.evaluate(
    (pitch) => window.mapGoldenStageTileParentFallback(pitch),
    TOP_DOWN_PITCH_DEGREES,
  );
  const expected_tiles = SCENE_EXPECTED_TILES_BY_PITCH_DEGREES.get(TOP_DOWN_PITCH_DEGREES);
  let settled_statistics = null;
  for (let settle_frame = 0; settle_frame < MAXIMUM_SETTLE_RENDER_FRAMES; ++settle_frame) {
    settled_statistics = await page.evaluate(
      (pitch) => window.mapGoldenRenderFrame(pitch),
      TOP_DOWN_PITCH_DEGREES,
    );
    if (
      settled_statistics.drawnTiles === expected_tiles.drawnTiles &&
      settled_statistics.missingTiles === expected_tiles.missingTiles
    ) {
      break;
    }
  }
  expect(
    settled_statistics.gpuBufferAllocations,
    'the settled scene must have uploaded its layers at least once',
  ).toBeGreaterThan(0);
  for (let steady_frame = 1; steady_frame <= 2; ++steady_frame) {
    const steady_statistics = await page.evaluate(
      (pitch) => window.mapGoldenRenderFrame(pitch),
      TOP_DOWN_PITCH_DEGREES,
    );
    expect(
      steady_statistics.gpuBufferAllocations,
      `frame ${steady_frame} after settling allocated a GPU buffer`,
    ).toBe(settled_statistics.gpuBufferAllocations);
  }
});

test('browser WebGPU canvas matches the committed pitched 45-degree golden fixture', async ({ page }) => {
  await assert_scene_preconditions(page, 'the pitched scene');
  const renderer_status = await require_gpu_renderer(page);
  if (renderer_status === null) {
    return;
  }
  await assert_scene_matches_golden(
    page,
    PITCHED_PITCH_DEGREES,
    PITCHED_GOLDEN_FILE,
    'the pitched scene',
    0,
  );
});

// Each committed Sacramento overlay scene: apply the scene's overlay vocabulary
// through the wasm canvas' `setOverlays` (the fixture page keeps the list
// across the probe's re-renders), then compare the full tiles+overlay frame
// against its fixture. The empty-list rule is covered by the two tile
// scenes' zero overlay-triangle assertions above (the wasm mirror of the
// native `overlay_golden.rs` degradation test). Each test renders on a fresh
// page, so no scene leaks into the next.
for (const overlay_expectation of OVERLAY_SCENE_EXPECTATIONS) {
  const scene_description = `the ${overlay_expectation.scene_key} overlay scene`;
  test(
    `browser WebGPU canvas matches the committed ${overlay_expectation.scene_key} overlay golden fixture`,
    async ({ page }) => {
      await assert_scene_preconditions(page, scene_description);
      const renderer_status = await require_gpu_renderer(page);
      if (renderer_status === null) {
        return;
      }
      await page.evaluate(
        (scene_key) => window.mapGoldenSetOverlayScene(scene_key),
        overlay_expectation.scene_key,
      );
      await assert_scene_matches_golden(
        page,
        TOP_DOWN_PITCH_DEGREES,
        overlay_expectation.golden_file,
        scene_description,
        overlay_expectation.expected_overlay_triangles,
      );
      // The applied scene must clear cleanly: a cleared overlay list returns
      // the frame to the tiles-only golden with zero drawn triangles.
      await page.evaluate(() => window.mapGoldenClearOverlays());
      await assert_scene_matches_golden(
        page,
        TOP_DOWN_PITCH_DEGREES,
        TOP_DOWN_GOLDEN_FILE,
        `${scene_description} cleared`,
        0,
      );
    },
  );
}
// One golden per named vector style (M21 residual): each style renders the
// committed golden vector tile at a fixed camera, so an edit to a style
// artifact that changes what the id paints fails the compare.
const VECTOR_STYLE_GOLDEN_EXPECTATIONS = [
  { style_id: 'bright-light', golden_file: 'sacramento_zoom14_bright_light.png' },
  { style_id: 'fiord-dark', golden_file: 'sacramento_zoom14_fiord_dark.png' },
  { style_id: 'positron-light', golden_file: 'sacramento_zoom14_positron_light.png' },
];

for (const style_expectation of VECTOR_STYLE_GOLDEN_EXPECTATIONS) {
  test(`the ${style_expectation.style_id} vector style matches its golden`, async ({
    page,
  }) => {
    await require_gpu_renderer(page);
    const metrics = await page.evaluate(
      ([style_id, outlier_channel_error]) =>
        window.mapGoldenProbeVectorStyle(style_id, outlier_channel_error),
      [style_expectation.style_id, OUTLIER_CHANNEL_ERROR],
    );
    expect(
      metrics.drawnTileCount,
      `the ${style_expectation.style_id} base must draw the committed golden tile`,
    ).toBeGreaterThan(0);
    expect(
      metrics.meanAbsoluteError,
      `the ${style_expectation.style_id} style drifted from ` +
        `${style_expectation.golden_file}`,
    ).toBeLessThanOrEqual(MEAN_ABSOLUTE_ERROR_TOLERANCE);
    expect(
      metrics.outlierPixelFraction,
      `the ${style_expectation.style_id} style has too many outlier pixels`,
    ).toBeLessThanOrEqual(OUTLIER_PIXEL_FRACTION_LIMIT);
  });
}

// The capture-free snapshot path: `MapView.snapshot()` replays the last
// presented frame into an offscreen texture and copies it to a mapped buffer,
// so the test reads deterministic pixels without a compositor re-present.
// Two frames at the same pose must snapshot to identical bytes, and the
// first capture must match the committed style golden on the same tolerances
// the `drawImage` probe asserts.
test('the snapshot surface answers byte-identical frames that match the golden', async ({
  page,
}) => {
  await require_gpu_renderer(page);
  const snapshot_probe = await page.evaluate(
    ([style_id, outlier_channel_error]) =>
      window.mapGoldenSnapshotDeterminism(style_id, outlier_channel_error),
    ['bright-light', OUTLIER_CHANNEL_ERROR],
  );
  expect(
    snapshot_probe.byteLength,
    'snapshot() must answer one unpadded RGBA buffer for the 512x512 frame',
  ).toBe(snapshot_probe.expectedByteLength);
  expect(
    snapshot_probe.secondByteLength,
    'both snapshots must describe the same frame extent',
  ).toBe(snapshot_probe.expectedByteLength);
  expect(
    snapshot_probe.differingByteCount,
    'snapshot() must be byte-identical across two renders at the same camera pose',
  ).toBe(0);
  expect(
    snapshot_probe.meanAbsoluteError,
    'the snapshot pixels drifted from sacramento_zoom14_bright_light.png',
  ).toBeLessThanOrEqual(MEAN_ABSOLUTE_ERROR_TOLERANCE);
  expect(
    snapshot_probe.outlierPixelFraction,
    'the snapshot pixels carry too many outliers against the golden',
  ).toBeLessThanOrEqual(OUTLIER_PIXEL_FRACTION_LIMIT);
});

// The regional scene's committed goldens: the browser capture from the same
// z9 camera, style and recorded vector tile the native Carto regional golden
// pins (`crates/rustic-maps-rendering/tests/vector_base_golden.rs`). The
// browser frame is compared against its own golden on the committed
// tolerances, and the two goldens are compared file-to-file, so a
// cross-renderer divergence is measured rather than hidden in the render
// check.
const REGIONAL_CARTO_GOLDEN_FILE = 'sacramento_region_zoom9_carto_light_browser.png';
const NATIVE_REGIONAL_CARTO_GOLDEN_FILE = 'sacramento_region_zoom9_carto_light.png';

test('the browser regional Carto frame matches its committed golden', async ({ page }) => {
  const renderer_status = await require_gpu_renderer(page);
  if (renderer_status === null) {
    return;
  }
  const metrics = await page.evaluate(
    (outlier_channel_error) =>
      window.mapGoldenProbeRegionalGolden(outlier_channel_error),
    OUTLIER_CHANNEL_ERROR,
  );
  expect(
    metrics.drawnTileCount,
    'the regional Carto base must draw the recorded z9 tile',
  ).toBeGreaterThan(0);
  expect(
    metrics.meanAbsoluteError,
    `the browser regional frame drifted from ${REGIONAL_CARTO_GOLDEN_FILE}`,
  ).toBeLessThanOrEqual(MEAN_ABSOLUTE_ERROR_TOLERANCE);
  expect(
    metrics.outlierPixelFraction,
    'the browser regional frame has too many outlier pixels',
  ).toBeLessThanOrEqual(OUTLIER_PIXEL_FRACTION_LIMIT);
  // The regional frame's `routes` layer carries road and transit refs, so the
  // canvas must draw the label layer's badge pass: solid triangles beneath the
  // reference glyphs, reported by the frame statistics.
  const frame_statistics = await page.evaluate(() => window.mapGoldenFrameStatistics());
  expect(
    frame_statistics?.labelBadgeTriangles ?? 0,
    'the regional Carto frame must draw route-reference badge geometry',
  ).toBeGreaterThan(0);
});

// File-to-file comparison of the two committed goldens: it needs the fixture
// decodes, not a GPU, so it also holds on the documented degradation path.
test('the browser regional golden stays within the native golden tolerance', async ({ page }) => {
  const metrics = await page.evaluate(
    (outlier_channel_error) =>
      window.mapGoldenCompareRegionalGoldens(outlier_channel_error),
    OUTLIER_CHANNEL_ERROR,
  );
  expect(metrics.widthPixels, 'the browser regional golden width').toBe(
    GOLDEN_VIEWPORT_EDGE_PIXELS,
  );
  expect(metrics.heightPixels, 'the browser regional golden height').toBe(
    GOLDEN_VIEWPORT_EDGE_PIXELS,
  );
  expect(
    metrics.meanAbsoluteError,
    `${REGIONAL_CARTO_GOLDEN_FILE} diverged from ${NATIVE_REGIONAL_CARTO_GOLDEN_FILE}`,
  ).toBeLessThanOrEqual(MEAN_ABSOLUTE_ERROR_TOLERANCE);
  expect(
    metrics.outlierPixelFraction,
    `${REGIONAL_CARTO_GOLDEN_FILE} has too many outlier pixels against ` +
      `${NATIVE_REGIONAL_CARTO_GOLDEN_FILE}`,
  ).toBeLessThanOrEqual(OUTLIER_PIXEL_FRACTION_LIMIT);
});

// Motion grading (M38): a pan/zoom gesture draws the vector base from cached
// geometry without label assembly, and the settle frame drawn afterwards must
// be pixel-identical to the same camera's direct, ungraded frame. The two runs
// start from the same installed source and the same label-placement history,
// and a graded frame deliberately leaves that history in hand, so the settle
// resolves the same labels the direct frame did.
test('the settle frame after a pan and zoom gesture is pixel-identical to the direct frame', async ({
  page,
}) => {
  const renderer_status = await require_gpu_renderer(page);
  if (renderer_status === null) {
    return;
  }
  const comparison = await page.evaluate(
    () => window.mapGoldenCompareMotionSettledFrame(),
  );

  // Both the reference and the settle are resolved frames: no gesture was
  // active and the label pass ran.
  expect(comparison.directFrame.motionGraded, 'the direct frame is ungraded').toBe(false);
  expect(
    comparison.directFrame.labelCount,
    'the direct frame places labels',
  ).toBeGreaterThan(0);
  expect(comparison.settleFrame.motionGraded, 'the settle frame is ungraded').toBe(false);
  expect(
    comparison.settleFrame.labelCount,
    'the settle frame places labels',
  ).toBeGreaterThan(0);

  // Every frame drawn mid-gesture was graded and assembled no labels.
  expect(comparison.gradedFrames.length).toBeGreaterThan(1);
  for (const [frame_index, graded_frame] of comparison.gradedFrames.entries()) {
    expect(
      graded_frame.motionGraded,
      `graded frame ${frame_index} must report motionGraded`,
    ).toBe(true);
    expect(
      graded_frame.labelCount,
      `graded frame ${frame_index} must not run label assembly`,
    ).toBe(0);
  }

  expect(
    comparison.differingPixelCount,
    `the settle frame differs from the direct frame at ` +
      `maximum channel ${comparison.maximumChannelDifference}`,
  ).toBe(0);
  expect(comparison.maximumChannelDifference).toBe(0);
});
