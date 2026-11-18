// The wasm canvas' vector-source contract members (M31 round 76): the status
// readback is honest before an install, after a refused install, after a
// refused render, and after a clear, and the install path refuses malformed
// descriptors without throwing across the boundary as a panic. The status
// object is the UDL `VectorTileSourceStatus` mirror; the page's base-map line
// reads it every frame.
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

/** The renderer-readiness budget (a slow first WebGPU init). */
const RENDERER_READINESS_TIMEOUT_MILLISECONDS = 30_000;

test.beforeEach(async ({ page }) => {
  await page.goto('/');
  await waitForMapTestReady(page);
  await expect
    .poll(() => page.evaluate(() => (typeof window.mapRendererMode === 'function' ? window.mapRendererMode() : null)), {
      timeout: RENDERER_READINESS_TIMEOUT_MILLISECONDS,
    })
    .toBe('gpu');
});

/** Builds an off-screen canvas bound to the page's SDK root. */
async function create_probe_view(page) {
  return page.evaluate(async () => {
    const canvas = document.createElement('canvas');
    canvas.width = 256;
    canvas.height = 256;
    canvas.style.position = 'fixed';
    canvas.style.left = '-10000px';
    document.body.appendChild(canvas);
    const map_view = new window.mapTest.MapView(canvas);
    await map_view.initialize();
    window.vectorSourceProbeView = map_view;
    return true;
  });
}

test('the status answers not-configured, configured and cleared in the contract shape', async ({
  page,
}) => {
  await create_probe_view(page);

  const status_before = await page.evaluate(() => {
    const status = window.vectorSourceProbeView.vectorTileSourceStatus();
    return {
      keys: Object.keys(status).sort(),
      isConfigured: status.isConfigured,
      styleId: status.styleId,
      sourceName: status.sourceName,
      attributionText: status.attributionText,
      maximumZoomLevel: status.maximumZoomLevel,
      drawnTileCount: status.drawnTileCount,
      failedTileCount: status.failedTileCount,
      lastFailureDetail: status.lastFailureDetail,
      unsupportedPropertyCount: status.unsupportedPropertyCount,
      unsupportedPropertyNames: status.unsupportedPropertyNames,
      degradedAssetNames: status.degradedAssetNames,
      sourceLayerCounts: status.sourceLayerCounts,
    };
  });
  // The UDL record's field set, in the seam's lower-camel spelling.
  expect(status_before.keys).toEqual([
    'attributionText',
    'degradedAssetNames',
    'drawnTileCount',
    'failedTileCount',
    'isConfigured',
    'lastFailureDetail',
    'maximumZoomLevel',
    'pendingVectorInstall',
    'sourceLayerCounts',
    'sourceName',
    'styleId',
    'unsupportedPropertyCount',
    'unsupportedPropertyNames',
  ]);
  expect(status_before.isConfigured).toBe(false);
  expect(status_before.styleId).toBe('');
  expect(status_before.sourceName).toBe('');
  expect(status_before.attributionText).toBe('');
  expect(status_before.maximumZoomLevel).toBe(0);
  expect(status_before.drawnTileCount).toBe(0);
  expect(status_before.failedTileCount).toBe(0);
  expect(status_before.lastFailureDetail).toBeNull();
  expect(status_before.unsupportedPropertyCount).toBe(0);
  expect(status_before.unsupportedPropertyNames).toEqual([]);
  expect(status_before.degradedAssetNames).toEqual([]);
  expect(status_before.sourceLayerCounts).toEqual([]);

  const install_outcome = await page.evaluate(() => {
    const map_view = window.vectorSourceProbeView;
    try {
      map_view.setVectorTileSource({
        urlTemplate: `${window.location.origin}/vector-source-probe/{z}/{x}/{y}.pbf`,
        sourceName: 'probe-tileset',
        attributionText: 'Probe attribution',
        styleId: 'bright-light',
        minimumZoomLevel: 0,
        maximumZoomLevel: 14,
      });
      return { accepted: true };
    } catch (install_failure) {
      return { accepted: false, detail: String(install_failure) };
    }
  });
  expect(install_outcome).toEqual({ accepted: true });

  const status_after = await page.evaluate(() => {
    const status = window.vectorSourceProbeView.vectorTileSourceStatus();
    return {
      isConfigured: status.isConfigured,
      styleId: status.styleId,
      sourceName: status.sourceName,
      attributionText: status.attributionText,
      maximumZoomLevel: status.maximumZoomLevel,
      unsupportedPropertyCount: status.unsupportedPropertyCount,
      unsupportedPropertyNames: status.unsupportedPropertyNames,
      degradedAssetNames: status.degradedAssetNames,
      sourceLayerCounts: status.sourceLayerCounts,
    };
  });
  expect(status_after.isConfigured).toBe(true);
  expect(status_after.styleId).toBe('bright-light');
  expect(status_after.sourceName).toBe('probe-tileset');
  expect(status_after.attributionText).toBe('Probe attribution');
  expect(status_after.maximumZoomLevel).toBe(14);
  expect(typeof status_after.unsupportedPropertyCount).toBe('number');
  expect(Array.isArray(status_after.unsupportedPropertyNames)).toBe(true);
  expect(status_after.unsupportedPropertyNames.length).toBeLessThanOrEqual(32);
  expect(Array.isArray(status_after.degradedAssetNames)).toBe(true);
  expect(Array.isArray(status_after.sourceLayerCounts)).toBe(true);

  // A clear leaves the honest not-configured status behind.
  await page.evaluate(() => window.vectorSourceProbeView.clearVectorTileSource());
  const status_cleared = await page.evaluate(() => {
    const status = window.vectorSourceProbeView.vectorTileSourceStatus();
    return {
      isConfigured: status.isConfigured,
      styleId: status.styleId,
      drawnTileCount: status.drawnTileCount,
      lastFailureDetail: status.lastFailureDetail,
    };
  });
  expect(status_cleared).toEqual({
    isConfigured: false,
    styleId: '',
    drawnTileCount: 0,
    lastFailureDetail: null,
  });
});

test('a malformed descriptor is refused verbatim and leaves the previous source in force', async ({
  page,
}) => {
  await create_probe_view(page);
  const refusals = await page.evaluate(() => {
    const map_view = window.vectorSourceProbeView;
    const valid_source = {
      urlTemplate: `${window.location.origin}/vector-source-probe/{z}/{x}/{y}.pbf`,
      sourceName: 'probe-tileset',
      attributionText: 'Probe attribution',
      styleId: 'bright-light',
      minimumZoomLevel: 0,
      maximumZoomLevel: 14,
    };
    map_view.setVectorTileSource(valid_source);
    const attempts = [
      { ...valid_source, styleId: 'no-such-style' },
      { ...valid_source, sourceName: '' },
      { ...valid_source, minimumZoomLevel: 15, maximumZoomLevel: 14 },
      { ...valid_source, urlTemplate: 'not-a-tile-template' },
      { ...valid_source, maximumZoomLevel: 'fourteen' },
      { ...valid_source, tokenBucketCapacityTokens: 0 },
      { ...valid_source, refillTokensPerSecond: -1 },
    ];
    const details = attempts.map(attempt => {
      try {
        map_view.setVectorTileSource(attempt);
        return 'unexpectedly accepted';
      } catch (refusal) {
        return String(refusal);
      }
    });
    return {
      details,
      stillConfigured: map_view.vectorTileSourceStatus().isConfigured,
      stillStyled: map_view.vectorTileSourceStatus().styleId,
    };
  });
  expect(refusals.details[0]).toContain('style the SDK does not carry');
  expect(refusals.details[1]).toContain('source name must be non-empty');
  expect(refusals.details[2]).toContain('inverted zoom range');
  expect(refusals.details[3]).toContain('lacks the');
  expect(refusals.details[4]).toContain('maximumZoomLevel');
  // The optional pacing fields reach the provider's validation, which names
  // the declaring source in its refusal.
  expect(refusals.details[5]).toContain('probe-tileset tile source');
  expect(refusals.details[5]).toContain('token-bucket capacity');
  expect(refusals.details[6]).toContain('probe-tileset tile source');
  expect(refusals.details[6]).toContain('refill rate');
  expect(refusals.stillConfigured).toBe(true);
  expect(refusals.stillStyled).toBe('bright-light');
});
