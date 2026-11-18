// The frame loop must never provoke a WebGPU validation error. The timestamp
// readback ring maps small buffers asynchronously; a cancelled or repeated map
// makes browsers with strict validation (Firefox) reject the buffer
// ("still mapped", "already mapped", "map is pending") and the map stops
// presenting. This drives the page through a live trip and pans the camera
// while capturing the console, and asserts no such error was reported.
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

/** Console patterns that indicate the GPU buffer lifecycle broke. */
const GPU_VALIDATION_ERROR_PATTERN =
  /webgpu error|already mapped|still mapped|map is pending|validation error/i;
/** The settle budget for the first frame and the readiness ladder. */
const FRAME_SETTLE_TIMEOUT_MILLISECONDS = 20_000;
/** How long the trip and pan run while the console is watched, in ms. */
const OBSERVATION_WINDOW_MILLISECONDS = 6000;

test('the frame loop emits no WebGPU validation errors during a trip and a pan', async ({
  page,
}) => {
  const validation_errors = [];
  page.on('console', (message) => {
    if (message.type() === 'error' || message.type() === 'warning') {
      const text = message.text();
      if (GPU_VALIDATION_ERROR_PATTERN.test(text)) {
        validation_errors.push(text);
      }
    }
  });
  page.on('pageerror', (error) => {
    if (GPU_VALIDATION_ERROR_PATTERN.test(String(error))) {
      validation_errors.push(String(error));
    }
  });

  await page.goto('/?advanced');
  await waitForMapTestReady(page);
  await expect
    .poll(
      () => page.evaluate(
        () => window.mapLastFrameStats()?.stageMilliseconds !== undefined,
      ),
      { timeout: FRAME_SETTLE_TIMEOUT_MILLISECONDS },
    )
    .toBe(true);

  // A live trip keeps rendering frame after frame (the timestamp ring maps a
  // buffer on every one), and a drag interrupts the follow without stopping
  // the loop.
  await page.locator('#follow-demo-start-control').click();
  await page.locator('#map-overlay').dispatchEvent('pointerdown');
  await page.mouse.move(320, 240);
  await page.mouse.down();
  await page.mouse.move(420, 260, { steps: 10 });
  await page.mouse.up();
  await page.waitForTimeout(OBSERVATION_WINDOW_MILLISECONDS);

  const frame_statistics = await page.evaluate(() => window.mapLastFrameStats());
  expect(frame_statistics?.drawnFragments ?? 0).toBeGreaterThan(0);
  expect(validation_errors).toEqual([]);
});
