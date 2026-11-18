// The generated contract and the canvas share one wasm module. The contract
// `MapSdk` is the page's SDK root; the canvas class is the generated glue's
// distinct `BrowserMapView`, so the stripped contract `MapView` never reaches
// the page and the two cannot collide. The page keeps no second SDK root: the
// canvas constructor takes only its element and an optional tile resolution.

import { expect, test, waitForMapTestReady } from './network_guard.mjs';

test('the page exposes the generated SDK root and the distinct browser canvas', async ({
  page,
}) => {
  await page.goto('/');

  await waitForMapTestReady(page);

  const observations = await page.evaluate(async () => {
    const buildCanvas = () => {
      const canvas = document.createElement('canvas');
      canvas.width = 64;
      canvas.height = 64;
      document.body.appendChild(canvas);
      return canvas;
    };

    // The documented canvas shape: one element, an optional tile resolution.
    const view = new window.mapTest.MapView(buildCanvas());
    const pinnedView = new window.mapTest.MapView(
      buildCanvas(),
      window.mapTest.TileResolution.Retina,
    );

    return {
      canvas_class_name: window.mapTest.MapView.name,
      has_legacy_root: 'maps' in window.mapTest,
      has_legacy_root_class: 'MapsSdk' in window.mapTest,
      module_version: window.mapTest.getSdkCompiledVersion(),
      has_view_version: 'sdkCompiledVersion' in view,
      has_pinned_view_version: 'sdkCompiledVersion' in pinnedView,
    };
  });

  expect(observations.canvas_class_name).toBe('BrowserMapView');
  expect(observations.has_legacy_root).toBe(false);
  expect(observations.has_legacy_root_class).toBe(false);
  expect(observations.module_version).toMatch(/^\d+\.\d+\.\d+/);
  expect(observations.has_view_version).toBe(false);
  expect(observations.has_pinned_view_version).toBe(false);
});
