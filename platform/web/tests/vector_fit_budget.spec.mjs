// The z14→z7 vector fit smoke gate (PLAN.md M65).
//
// This suite runs under the Playwright harness's SwiftShader renderer, so it
// does not gate wall-clock frame time: `platform/web/perf/run-fit-budget.mjs`
// measures the 1.5 s first-full-frame budget on the host's own GPU and stays
// outside `npm test`. What this spec does gate is hardware-independent:
//
//   * the committed z7/z8 performance fixtures install through a same-origin
//     template and the fit completes its visible set,
//   * the first drawn base tile arrives no later than the first full frame,
//   * the z7 frame's overlay geometry stays inside the 400k-triangle budget
//     (the low-zoom LOD is deterministic on every renderer).
//
// Run the same scenario with `MAP_FIT_BUDGET_STRICT=1` on a GPU host to also
// enforce the first-full-frame budget.
import { expect, test } from './network_guard.mjs';
import {
  FIT_BUDGET_FIRST_FULL_FRAME_MILLISECONDS,
  FIT_BUDGET_OVERLAY_TRIANGLE_COUNT,
  FIT_BUDGET_PAGE_PATH,
  FIT_BUDGET_VIEWPORT,
  installFitBudgetFixtureRoute,
  runFitBudgetScenario,
} from '../perf/fit_budget_harness.mjs';

/** The fit may take longer under SwiftShader, but never stall forever. */
const FIT_COMPLETION_TIMEOUT_MILLISECONDS = 60_000;

test('the z7 fit paints the visible set inside the overlay triangle budget', async ({
  page,
}) => {
  await page.setViewportSize(FIT_BUDGET_VIEWPORT);
  await installFitBudgetFixtureRoute(page);
  await page.goto(FIT_BUDGET_PAGE_PATH);
  await page.waitForFunction(() => window.perfReady !== undefined, null, {
    timeout: FIT_COMPLETION_TIMEOUT_MILLISECONDS,
  });
  await page.evaluate(() => window.perfReady);

  const fit = await runFitBudgetScenario(page);
  expect(fit.timedOut, `the fit never completed: ${fit.lastFailureDetail ?? 'no failure detail'}`)
    .toBe(false);
  expect(fit.expectedTileCount).toBeGreaterThan(0);
  expect(fit.firstDrawnBaseTileMilliseconds).not.toBeNull();
  expect(fit.firstFullFrameMilliseconds)
    .toBeGreaterThanOrEqual(fit.firstDrawnBaseTileMilliseconds);
  const final_sample = fit.samples[fit.samples.length - 1];
  expect(final_sample.drawnTileCount).toBeGreaterThanOrEqual(fit.expectedTileCount);
  expect(final_sample.failedTileCount).toBe(0);
  const maximum_overlay_triangles = Math.max(
    ...fit.samples.map((sample) => sample.overlayTriangles),
  );
  expect(maximum_overlay_triangles)
    .toBeLessThanOrEqual(FIT_BUDGET_OVERLAY_TRIANGLE_COUNT);
  if (process.env.MAP_FIT_BUDGET_STRICT === '1') {
    expect(fit.firstFullFrameMilliseconds)
      .toBeLessThanOrEqual(FIT_BUDGET_FIRST_FULL_FRAME_MILLISECONDS);
  }
});
