// The bare vector-base surface the fit-budget scenario measures.
//
// It loads the SDK exactly as the reference page does (`main.mjs`), creates
// one MapView on a 1920 × 1080 canvas, installs the committed performance
// fixture source with a same-origin template and exposes
// `window.runFitBudgetScenario`, which the host-machine runner and the
// Playwright spec both call.
//
// The scenario is a cold fit: one frame at the start zoom (so the scheduler
// sees a camera before the fit), then a z14 → z7 camera change, paced at one
// render per animation frame — the page's default frame cadence. It records,
// per frame, the wall-clock render time, the wasm stage spans, the overlay
// triangle count and the source's drawn/failed tile counts, and resolves with
// the time to the first base tile, the time to a complete visible set, and
// those samples.

/** The camera pose the fit uses: top-down, north up, over the fixture. */
const FIT_CENTER_LATITUDE_DEGREES = 38.57393751557591;
const FIT_CENTER_LONGITUDE_DEGREES = -121.519775390625;

/** The vertical field of view the renderer's default projection uses. */
const FIELD_OF_VIEW_DEGREES = 60;

/** The same-origin template the committed performance fixtures answer. */
const FIXTURE_URL_TEMPLATE = '/perf/fixtures/vector/{z}/{x}/{y}.mvt';

/** The fixture source's neutral identity; no provider name or vendor URL. */
const FIXTURE_SOURCE_NAME = 'perf-fixture';
const FIXTURE_ATTRIBUTION_TEXT = 'Committed performance fixture';
const FIXTURE_STYLE_ID = 'carto-light';

/** The burst the scenario declares, matching the default scheduler burst. */
const FIXTURE_TOKEN_BUCKET_CAPACITY_TOKENS = 8;
const FIXTURE_REFILL_TOKENS_PER_SECOND = 8;

/** How long a fit may take before the scenario gives up (a stalled run). */
const FIT_TIMEOUT_MILLISECONDS = 20_000;

/** How many settled frames the scenario records after the fit completes. */
const SETTLE_FRAME_COUNT = 30;

/** The zoom whose unchanged settle frames the label-cache measurement uses. */
const Z8_SETTLE_ZOOM_LEVEL = 8;
/** How many unchanged z8 settle frames the scenario records. */
const Z8_SETTLE_FRAME_COUNT = 30;
/** Upper bound on frames drawn while the z7→z8 cross-fade drains. */
const Z8_TRANSITION_DRAIN_FRAME_LIMIT = 120;

/** Resolves on the next animation frame. */
function nextAnimationFrame() {
  return new Promise((resolve) => requestAnimationFrame(resolve));
}

let view = null;

window.perfReady = (async () => {
  await window.mapTestReady;
  const canvas = document.getElementById('fit-budget-canvas');
  const MapView = window.mapTest.MapView;
  view = new MapView(canvas, window.mapTest.maps, window.mapTest.TileResolution.Standard);
  const adapter_detail = await view.initialize();
  // The harness reads the surface's status between frames; keep the one view
  // reachable without guessing a binding.
  window.fitBudgetView = view;
  view.setVectorTileSource({
    urlTemplate: `${window.location.origin}${FIXTURE_URL_TEMPLATE}`,
    sourceName: FIXTURE_SOURCE_NAME,
    attributionText: FIXTURE_ATTRIBUTION_TEXT,
    styleId: FIXTURE_STYLE_ID,
    minimumZoomLevel: 0,
    maximumZoomLevel: 8,
    tokenBucketCapacityTokens: FIXTURE_TOKEN_BUCKET_CAPACITY_TOKENS,
    refillTokensPerSecond: FIXTURE_REFILL_TOKENS_PER_SECOND,
  });
  return {
    adapterDetail: adapter_detail,
    rendererMode: window.mapTest.maps ? 'gpu' : 'unknown',
    width: canvas.width,
    height: canvas.height,
  };
})();

window.runFitBudgetScenario = async ({
  startZoomLevel,
  fitZoomLevel,
  maximumZoomLevel,
  measureZ8Settle = false,
}) => {
  const canvas = document.getElementById('fit-budget-canvas');
  const MapCamera = window.mapTest.MapCamera;
  // The first frame gives the scheduler a camera so the measured change is a
  // fit, not the surface's very first resolve.
  await view.renderFrame(
    new MapCamera(
      FIT_CENTER_LATITUDE_DEGREES,
      FIT_CENTER_LONGITUDE_DEGREES,
      startZoomLevel,
      0,
      0,
    ),
    FIELD_OF_VIEW_DEGREES,
  );
  await nextAnimationFrame();
  // The served zoom clamps to the fixture ceiling; the expected visible count
  // is the camera's own listing at that zoom.
  const servedZoomLevel = Math.min(fitZoomLevel, maximumZoomLevel);
  const probeCamera = new MapCamera(
    FIT_CENTER_LATITUDE_DEGREES,
    FIT_CENTER_LONGITUDE_DEGREES,
    fitZoomLevel,
    0,
    0,
  );
  const expectedTileCount = probeCamera.visibleTileKeys(
    canvas.width,
    canvas.height,
    FIELD_OF_VIEW_DEGREES,
    servedZoomLevel,
  ).length;

  const fitStartedMilliseconds = performance.now();
  let firstDrawnBaseTileMilliseconds = null;
  let firstFullFrameMilliseconds = null;
  const samples = [];
  let sourceStatus = view.vectorTileSourceStatus();
  while (performance.now() - fitStartedMilliseconds <= FIT_TIMEOUT_MILLISECONDS) {
    const renderStartedMilliseconds = performance.now();
    const statistics = await view.renderFrame(
      new MapCamera(
        FIT_CENTER_LATITUDE_DEGREES,
        FIT_CENTER_LONGITUDE_DEGREES,
        fitZoomLevel,
        0,
        0,
      ),
      FIELD_OF_VIEW_DEGREES,
    );
    const renderFinishedMilliseconds = performance.now();
    sourceStatus = view.vectorTileSourceStatus();
    const sample = {
      atMilliseconds: renderStartedMilliseconds - fitStartedMilliseconds,
      renderMilliseconds: renderFinishedMilliseconds - renderStartedMilliseconds,
      overlayTriangles: statistics.overlayTriangles,
      stageMilliseconds: statistics.stageMilliseconds,
      drawnTileCount: sourceStatus.drawnTileCount,
      failedTileCount: sourceStatus.failedTileCount,
      requestedTileCount: expectedTileCount,
    };
    samples.push(sample);
    if (firstDrawnBaseTileMilliseconds === null && statistics.overlayTriangles > 0) {
      firstDrawnBaseTileMilliseconds = renderFinishedMilliseconds - fitStartedMilliseconds;
    }
    if (
      firstFullFrameMilliseconds === null
      && sourceStatus.drawnTileCount >= expectedTileCount
      && sourceStatus.failedTileCount === 0
    ) {
      firstFullFrameMilliseconds = renderFinishedMilliseconds - fitStartedMilliseconds;
      break;
    }
    await nextAnimationFrame();
  }
  // A settled frame is the shared-frame case: every tile is cached and the
  // camera is unchanged. Record a short run so the report can state the
  // steady-state render cost next to the fit's arrival frames.
  const settleSamples = [];
  for (
    let settleFrameIndex = 0;
    settleFrameIndex < SETTLE_FRAME_COUNT && firstFullFrameMilliseconds !== null;
    settleFrameIndex += 1
  ) {
    await nextAnimationFrame();
    const renderStartedMilliseconds = performance.now();
    const statistics = await view.renderFrame(
      new MapCamera(
        FIT_CENTER_LATITUDE_DEGREES,
        FIT_CENTER_LONGITUDE_DEGREES,
        fitZoomLevel,
        0,
        0,
      ),
      FIELD_OF_VIEW_DEGREES,
    );
    settleSamples.push({
      renderMilliseconds: performance.now() - renderStartedMilliseconds,
      overlayTriangles: statistics.overlayTriangles,
      stageMilliseconds: statistics.stageMilliseconds,
    });
  }
  // The z8 phase measures unchanged settle frames where the label candidate
  // cache is warm and the tile-level cross-fade has drained. The host
  // harness asks for it; the deterministic spec keeps its shorter run.
  const z8SettleSamples = [];
  if (measureZ8Settle && firstFullFrameMilliseconds !== null) {
    const z8Camera = () =>
      new MapCamera(
        FIT_CENTER_LATITUDE_DEGREES,
        FIT_CENTER_LONGITUDE_DEGREES,
        Z8_SETTLE_ZOOM_LEVEL,
        0,
        0,
      );
    // Drain the z7 → z8 cross-fade; the first settled frame also warms the
    // z8 bucket's label candidates.
    for (
      let drainFrame = 0;
      drainFrame < Z8_TRANSITION_DRAIN_FRAME_LIMIT;
      drainFrame += 1
    ) {
      await nextAnimationFrame();
      const statistics = await view.renderFrame(z8Camera(), FIELD_OF_VIEW_DEGREES);
      if (
        !statistics.vectorZoomTransitionRunning
        && statistics.overlayTriangles > 0
      ) {
        // One more frame after the transition lands, so the samples below
        // are the shared-frame steady state with the label cache warm.
        await view.renderFrame(z8Camera(), FIELD_OF_VIEW_DEGREES);
        break;
      }
    }
    for (
      let settleFrameIndex = 0;
      settleFrameIndex < Z8_SETTLE_FRAME_COUNT;
      settleFrameIndex += 1
    ) {
      await nextAnimationFrame();
      const renderStartedMilliseconds = performance.now();
      const statistics = await view.renderFrame(z8Camera(), FIELD_OF_VIEW_DEGREES);
      z8SettleSamples.push({
        renderMilliseconds: performance.now() - renderStartedMilliseconds,
        overlayTriangles: statistics.overlayTriangles,
        labelCandidateExtractionCount:
          statistics.vectorBaseLabelCandidateExtractionCount ?? 0,
        labelCandidateCacheHitCount:
          statistics.vectorBaseLabelCandidateCacheHitCount ?? 0,
      });
    }
  }
  return {
    servedZoomLevel,
    expectedTileCount,
    firstDrawnBaseTileMilliseconds,
    firstFullFrameMilliseconds,
    timedOut: firstFullFrameMilliseconds === null,
    samples,
    settleSamples,
    z8SettleSamples,
    lastFailureDetail: sourceStatus.lastFailureDetail,
  };
};
