// The wasm `MapView`'s id-addressed overlay members (M17's contract slice)
// mirror the native contract: `addOverlay` answers a monotonic identifier
// that is never reused, `updateOverlay` replaces the addressed overlay in
// its draw position, `removeOverlay` answers whether the identifier was
// present, and a refused descriptor adds nothing. The bookkeeping needs no
// rendered frame, so the test drives the canvas directly on its own
// element instead of disturbing the demo's surface.

import { expect, test, waitForMapTestReady } from './network_guard.mjs';

test('the wasm MapView addresses overlays by identifier', async ({ page }) => {
  await page.goto('/');

  await waitForMapTestReady(page);

  const observations = await page.evaluate(async () => {

    const canvas = document.createElement('canvas');
    canvas.width = 64;
    canvas.height = 64;
    document.body.appendChild(canvas);
    const view = new window.mapTest.MapView(
      canvas,
      window.mapTest.TileResolution.Standard,
    );

    const polyline = {
      kind: 'routePolyline',
      coordinates: [
        { latitude: 38.5766, longitude: -121.4944 },
        { latitude: 38.5802, longitude: -121.5083 },
      ],
    };

    const first_identifier = view.addOverlay(polyline);
    const second_identifier = view.addOverlay(polyline);

    let unknown_update_message = null;
    let unknown_update_code = null;
    try {
      view.updateOverlay('overlay-999', polyline);
    } catch (error) {
      unknown_update_message = String(error);
      unknown_update_code = error?.code;
    }

    let invalid_add_message = null;
    let invalid_add_code = null;
    try {
      view.addOverlay({ kind: 'notAKind' });
    } catch (error) {
      invalid_add_message = String(error);
      invalid_add_code = error?.code;
    }

    const removed_known = view.removeOverlay(second_identifier);
    const removed_again = view.removeOverlay(second_identifier);

    // Clearing drops every overlay but never rewinds the identifier
    // sequence: identifiers are never reused.
    view.clearOverlays();
    const identifier_after_clear = view.addOverlay(polyline);

    return {
      first_identifier,
      second_identifier,
      identifier_after_clear,
      unknown_update_message,
      unknown_update_code,
      invalid_add_message,
      invalid_add_code,
      removed_known,
      removed_again,
    };
  });

  expect(observations.first_identifier).toBe('overlay-1');
  expect(observations.second_identifier).toBe('overlay-2');
  expect(observations.identifier_after_clear).toBe('overlay-3');
  expect(observations.unknown_update_message).toContain('overlay-999');
  expect(observations.unknown_update_code).toBe('invalidArgument');
  expect(observations.invalid_add_message).toContain('invalid');
  expect(observations.invalid_add_code).toBe('invalidRenderInput');
  expect(observations.removed_known).toBe(true);
  expect(observations.removed_again).toBe(false);
});
