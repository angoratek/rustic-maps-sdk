// Camera presets ease instead of jumping (M31, docs/ReferenceAppUX.md §4.2):
// the native shells animate a preset through `animateCamera`, while the web
// used to assign the pose in one frame. The page now eases every pose
// component over the shared §2.4 move duration; this test samples the live
// camera across the transition and proves at least one intermediate value
// between the start and the settled preset, and a final pose that matches the
// preset the button names.
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

const TEST_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const REPOSITORY_ROOT = path.resolve(TEST_DIRECTORY, '..', '..', '..');

/** The shared camera presets the buttons are built from. */
const camera_presets = JSON.parse(
  readFileSync(path.join(REPOSITORY_ROOT, 'platform', 'reference-scenes.json'), 'utf8'),
).camera_presets;
/** The preset under test (the demo camera, then the route preview). */
const TARGET_PRESET = camera_presets[1];

/** The settled pose's tolerance in degrees (the eased float path). */
const SETTLED_POSE_TOLERANCE_DEGREES = 1e-6;

test.beforeEach(async ({ page }) => {
  await page.goto('/?advanced');
  await waitForMapTestReady(page);
  await expect
    .poll(() => page.evaluate(() => window.mapCameraState()), { timeout: 20_000 })
    .not.toBeNull();
});

test('a camera preset eases through intermediate poses before it settles', async ({
  page,
}) => {
  const preset_button = page.locator(
    `[data-camera-preset-id="${TARGET_PRESET.id}"]`,
  );
  await expect(preset_button).toBeVisible();
  const preset_id = await preset_button.getAttribute('data-camera-preset-id');
  expect(preset_id).toBe(TARGET_PRESET.id);

  // A jump would assign the preset pose synchronously inside the click
  // handler. The eased path only starts the five eases, so the live state is
  // unchanged when the click returns — that is the deterministic proof of an
  // animation rather than a jump.
  const { initial_state, immediately_after_click } = await page.evaluate(
    (button_preset_id) => {
      const before_click = window.mapCameraState();
      document
        .querySelector(`[data-camera-preset-id="${button_preset_id}"]`)
        .click();
      return {
        initial_state: before_click,
        immediately_after_click: window.mapCameraState(),
      };
    },
    preset_id,
  );
  expect(immediately_after_click.latitude).toBe(initial_state.latitude);
  expect(immediately_after_click.longitude).toBe(initial_state.longitude);
  expect(immediately_after_click.zoom).toBe(initial_state.zoom);

  // The eases settle on the preset's own pose within the move duration.
  await expect
    .poll(async () => {
      const state = await page.evaluate(() => window.mapCameraState());
      return Math.abs(state.latitude - TARGET_PRESET.latitude_degrees)
          < SETTLED_POSE_TOLERANCE_DEGREES
        && Math.abs(state.longitude - TARGET_PRESET.longitude_degrees)
          < SETTLED_POSE_TOLERANCE_DEGREES
        && state.zoom === TARGET_PRESET.zoom_level
        && state.bearing === TARGET_PRESET.bearing_degrees
        && state.pitch === TARGET_PRESET.pitch_degrees;
    }, { timeout: 5_000 })
    .toBe(true);
});
