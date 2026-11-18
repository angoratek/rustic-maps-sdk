// The wasm MapView is canvas-owning and bound to the SDK root it was
// constructed with (M17's web slice): a page creates one `MapsSdk` root and
// hands it to every `MapView` — `new MapView(canvas, maps)` — mirroring the
// native construction shape. The optional third argument pins the tile
// resolution for harnesses (the golden capture); the two-argument form
// derives it from the device-pixel ratio through the same exported
// threshold the page uses.

import { expect, test, waitForMapTestReady } from './network_guard.mjs';

test('the wasm MapView is constructed with the SDK root', async ({ page }) => {
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

    // The documented shape: one SDK root, a view over it.
    const view = new window.mapTest.MapView(
      buildCanvas(),
      window.mapTest.maps,
    );
    // The harness shape: the same root, with the tile resolution pinned.
    const pinnedView = new window.mapTest.MapView(
      buildCanvas(),
      window.mapTest.maps,
      window.mapTest.TileResolution.Retina,
    );

    return {
      root_version: window.mapTest.maps.compiledVersion,
      module_version: window.mapTest.getSdkCompiledVersion(),
      view_version: view.sdkCompiledVersion,
      pinned_view_version: pinnedView.sdkCompiledVersion,
    };
  });

  expect(observations.root_version).toMatch(/^\d+\.\d+\.\d+/);
  expect(observations.root_version).toBe(observations.module_version);
  expect(observations.view_version).toBe(observations.root_version);
  expect(observations.pinned_view_version).toBe(observations.root_version);
});
