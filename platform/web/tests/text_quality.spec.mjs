// High-density text: the page lays the map out in CSS pixels, but a Retina
// browser composites a 1× canvas by upscaling it, which is what made the
// demo's SDF labels look foggy (M30). These cases pin the other half of the
// fix: the canvas backing store is sized in device pixels while its laid-out
// size stays in CSS pixels, and the wasm canvas reads the ratio back.
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

/** The device-pixel ratio this case renders at (a Retina 2× display). */
const RETINA_DEVICE_PIXEL_RATIO = 2;
/** The CSS viewport the browser lays the page out in. */
const CSS_VIEWPORT = { width: 800, height: 600 };

test.describe('a 2× display', () => {
  test.use({
    viewport: CSS_VIEWPORT,
    deviceScaleFactor: RETINA_DEVICE_PIXEL_RATIO,
  });

  test('backs the canvases with device pixels while their layout stays in CSS pixels', async ({
    page,
  }) => {
    await page.goto('/?advanced');
    await waitForMapTestReady(page);

    const canvas_facts = await page.evaluate(() => {
      const canvases = ['map-canvas', 'map-overlay'].map(canvas_identifier => {
        const canvas = document.getElementById(canvas_identifier);
        return {
          identifier: canvas_identifier,
          backing_width: canvas.width,
          backing_height: canvas.height,
          css_width: canvas.clientWidth,
          css_height: canvas.clientHeight,
        };
      });
      return { device_pixel_ratio: window.devicePixelRatio, canvases };
    });

    expect(canvas_facts.device_pixel_ratio).toBe(RETINA_DEVICE_PIXEL_RATIO);
    for (const canvas of canvas_facts.canvases) {
      // The compositor draws the backing store 1:1: no browser upscale of the
      // glyph atlas or of any line work.
      expect(canvas.backing_width).toBe(
        CSS_VIEWPORT.width * RETINA_DEVICE_PIXEL_RATIO,
      );
      expect(canvas.backing_height).toBe(
        CSS_VIEWPORT.height * RETINA_DEVICE_PIXEL_RATIO,
      );
      // The stylesheet keeps the laid-out size in CSS pixels, so the page's
      // controls and hit targets are unchanged.
      expect(canvas.css_width).toBe(CSS_VIEWPORT.width);
      expect(canvas.css_height).toBe(CSS_VIEWPORT.height);
    }
  });
});
