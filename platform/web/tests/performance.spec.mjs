// The demo's frame-budget monitor (owner ask, round 26): the page reports its
// own GPU frame cost and names the cause when a frame is too slow. The spec
// drives a few settled frames and asserts the readout shape; the warning
// verdict itself needs real load, so it is asserted only structurally.
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

test('the performance readout reports the frame budget after settled frames', async ({
  page,
}) => {
  await page.goto('/?advanced');
  await waitForMapTestReady(page);

  // The demo renders on demand; a couple of zoom steps settle new frames.
  await page.locator('#zoom-in-control').click();
  await page.locator('#zoom-in-control').click();
  await expect
    .poll(async () => page.evaluate(() => window.mapPerformance().status), {
      timeout: 15000,
    })
    .toMatch(/warming|ok|slow/);

  const summary = await page.evaluate(() => window.mapPerformance());
  expect(summary.sampleCount).toBeGreaterThanOrEqual(0);
  expect(summary.isSlow).toBe(summary.status === 'slow');
  if (summary.status === 'slow') {
    expect(typeof summary.causeText).toBe('string');
    expect(summary.causeText.length).toBeGreaterThan(0);
  }

  // The frame reports its own stage spans (M23), so the harness and the
  // diagnostics readout can attribute a slow frame instead of guessing. They
  // are real clocks, not placeholders: every stage is a finite, non-negative
  // number of milliseconds.
  // The readout's own verdict is only meaningful past its warm-up, so the
  // stage spans are read from the first frame that reported them.
  await expect
    .poll(
      () => page.evaluate(() => window.mapLastFrameStats()?.stageMilliseconds ?? null),
      { timeout: 15000 },
    )
    .not.toBeNull();
  const stage_milliseconds = await page.evaluate(
    () => window.mapLastFrameStats().stageMilliseconds,
  );
  for (const stage_name of ['tilePipeline', 'overlayGeometry', 'encode', 'submit']) {
    expect(typeof stage_milliseconds[stage_name]).toBe('number');
    expect(Number.isFinite(stage_milliseconds[stage_name])).toBe(true);
    expect(stage_milliseconds[stage_name]).toBeGreaterThanOrEqual(0);
  }

  const facts = await page.locator('#diagnostics-facts-lines').textContent();
  expect(facts).toContain('performance ');
  expect(facts).toContain('frames drawn');
});

test('the demo keeps the docs back-link out of the local harness root', async ({
  page,
}) => {
  // Under the documentation site's /demo/ path the same page reveals the
  // link (verified against the published site); the local dev-server root
  // must never show a dead "up" link.
  await page.goto('/?advanced');
  await expect(page.locator('#docs-back-link')).toBeHidden();
  await expect(page.locator('#docs-back-link')).toHaveAttribute('href', '../');
});
