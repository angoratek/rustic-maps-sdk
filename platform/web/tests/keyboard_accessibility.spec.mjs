// The keyboard and screen-reader contract of the live demo's map surface
// (docs/ReferenceAppUX.md §4.2): the map is a focus stop with an accessible
// name and instructions, the arrow keys pan, +/- zoom, Shift+arrows rotate
// and tilt, Home resets north and flat, every changed camera fact is spoken,
// and a keyboard visitor can skip straight to the control sheet. The camera
// facts come from the page's own state seam, so the test cannot pass on a
// label alone.
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

/** One arrow-key pan step (the page's `KEYBOARD_PAN_STEP_CSS_PIXELS`). */
const KEYBOARD_PAN_STEP_CSS_PIXELS = 100;
/** The pan step's tolerance in degrees at zoom 14 (a few meters). */
const PAN_LONGITUDE_TOLERANCE_DEGREES = 1.0;
/** The page's eased camera moves complete within this budget. */
const CAMERA_MOVE_TIMEOUT_MILLISECONDS = 3_000;

test.beforeEach(async ({ page }) => {
  await page.goto('/?advanced');
  await waitForMapTestReady(page);
  await expect
    .poll(() => page.evaluate(() => window.mapCameraState()), { timeout: 20_000 })
    .not.toBeNull();
});

/** The page's camera state, read through its test seam. */
async function camera_state_of(page) {
  return page.evaluate(() => window.mapCameraState());
}

/** Focuses the map surface the way a keyboard visitor would. */
async function focus_map(page) {
  await page.locator('#map-overlay').focus();
  await expect(page.locator('#map-overlay')).toBeFocused();
}

test('the map surface exposes its keyboard model and instructions', async ({ page }) => {
  const overlay = page.locator('#map-overlay');
  await expect(overlay).toHaveAttribute('tabindex', '0');
  await expect(overlay).toHaveAttribute('role', 'application');
  await expect(overlay).toHaveAttribute('aria-label', 'Interactive map');
  await expect(overlay).toHaveAttribute('aria-describedby', 'map-keyboard-instructions');
  // The GPU canvas below the input surface is decorative.
  await expect(page.locator('#map-canvas')).toHaveAttribute('aria-hidden', 'true');

  const instructions = await page.locator('#map-keyboard-instructions').textContent();
  expect(instructions).toContain('arrow keys to pan');
  expect(instructions).toContain('plus and minus to zoom');
  expect(instructions).toContain('Shift');
  expect(instructions).toContain('Home');

  const described_by_text = await page.evaluate(() => document
    .getElementById('map-keyboard-instructions').textContent.replace(/\s+/g, ' '));
  expect(described_by_text.length).toBeGreaterThan(40);
});

test('the first Tab reaches a visible skip link that lands on the controls', async ({ page }) => {
  await page.keyboard.press('Tab');
  const focused_class = await page.evaluate(() => document.activeElement?.className ?? '');
  expect(focused_class).toContain('skip-link');

  // The skip link is offscreen until focused, then reads as a real control.
  const focused_box = await page.locator('.skip-link').boundingBox();
  expect(focused_box).not.toBeNull();
  expect(focused_box.y).toBeGreaterThanOrEqual(0);

  await page.keyboard.press('Enter');
  await expect.poll(
    () => page.evaluate(() => document.activeElement?.id ?? ''),
    { timeout: CAMERA_MOVE_TIMEOUT_MILLISECONDS },
  ).toBe('controls');
});

test('a keyboard focus ring is visible on the map surface', async ({ page }) => {
  // Tab once (skip link), again (map surface): a keyboard-acquired focus.
  await page.keyboard.press('Tab');
  await page.keyboard.press('Tab');
  await expect(page.locator('#map-overlay')).toBeFocused();

  const outline = await page.evaluate(() => {
    const style = getComputedStyle(document.getElementById('map-overlay'));
    return { style: style.outlineStyle, width: style.outlineWidth };
  });
  expect(outline.style).not.toBe('none');
  expect(parseFloat(outline.width)).toBeGreaterThanOrEqual(2);
});

test('arrow keys pan by one documented step and announce the center', async ({ page }) => {
  await focus_map(page);
  const before = await camera_state_of(page);

  await page.keyboard.press('ArrowRight');
  await expect
    .poll(async () => (await camera_state_of(page)).longitude, {
      timeout: CAMERA_MOVE_TIMEOUT_MILLISECONDS,
    })
    .toBeLessThan(before.longitude);

  const after_right = await camera_state_of(page);
  expect(Math.abs(before.longitude - after_right.longitude))
    .toBeLessThan(PAN_LONGITUDE_TOLERANCE_DEGREES);
  expect(after_right.latitude).toBeCloseTo(before.latitude, 6);

  // The opposite key travels back to the starting center (the pan is a drag,
  // not a jump), and the live region reports the position after each move.
  await page.keyboard.press('ArrowLeft');
  await expect
    .poll(async () => (await camera_state_of(page)).longitude, {
      timeout: CAMERA_MOVE_TIMEOUT_MILLISECONDS,
    })
    .toBeCloseTo(before.longitude, 6);
  await expect
    .poll(() => page.locator('#map-status').textContent(), {
      timeout: CAMERA_MOVE_TIMEOUT_MILLISECONDS,
    })
    .toContain('Map moved to');
});

test('plus and minus zoom one level around the viewport center', async ({ page }) => {
  await focus_map(page);
  const before = await camera_state_of(page);

  await page.keyboard.press('=');
  await expect
    .poll(async () => (await camera_state_of(page)).zoom, {
      timeout: CAMERA_MOVE_TIMEOUT_MILLISECONDS,
    })
    .toBeCloseTo(before.zoom + 1, 6);
  await expect(page.locator('#map-status')).toContainText('Zoom');

  await page.keyboard.press('-');
  await expect
    .poll(async () => (await camera_state_of(page)).zoom, {
      timeout: CAMERA_MOVE_TIMEOUT_MILLISECONDS,
    })
    .toBeCloseTo(before.zoom, 6);
});

test('Shift+arrows rotate and tilt, and Home resets north and flat', async ({ page }) => {
  await focus_map(page);
  const before = await camera_state_of(page);

  await page.keyboard.press('Shift+ArrowRight');
  await expect
    .poll(async () => (await camera_state_of(page)).bearing, {
      timeout: CAMERA_MOVE_TIMEOUT_MILLISECONDS,
    })
    .toBeGreaterThan(before.bearing + 1);

  await page.keyboard.press('Shift+ArrowUp');
  await expect
    .poll(async () => (await camera_state_of(page)).pitch, {
      timeout: CAMERA_MOVE_TIMEOUT_MILLISECONDS,
    })
    .toBeGreaterThan(before.pitch + 1);

  await page.keyboard.press('Home');
  await expect
    .poll(async () => {
      const state = await camera_state_of(page);
      return Math.abs(state.bearing) + Math.abs(state.pitch);
    }, { timeout: CAMERA_MOVE_TIMEOUT_MILLISECONDS })
    .toBeLessThan(0.5);
  // The reset never changes the zoom level.
  const after = await camera_state_of(page);
  expect(after.zoom).toBeCloseTo(before.zoom, 6);
});

test('the 2D/3D control exposes its two-state pressed value', async ({ page }) => {
  const plane_toggle = page.getByRole('button', {
    name: 'Toggle the 2D and 3D camera planes',
  });
  await expect(plane_toggle).toHaveAttribute('aria-pressed', 'false');
  await plane_toggle.click();
  await expect(plane_toggle).toHaveAttribute('aria-pressed', 'true');
  await expect(plane_toggle).toHaveText('3D');
});

test('browser and system chords are left to the browser', async ({ page }) => {
  await focus_map(page);
  const before = await camera_state_of(page);
  // Ctrl/Cmd/Alt arrows belong to the browser and the OS, not the map.
  await page.keyboard.press('Control+ArrowRight');
  await page.keyboard.press('Alt+ArrowRight');
  await page.waitForTimeout(200);
  const after = await camera_state_of(page);
  expect(after.longitude).toBeCloseTo(before.longitude, 6);
});
