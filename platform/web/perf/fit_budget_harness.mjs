// Shared browser-harness pieces for the z14→z7 fit-budget scenario.
//
// The scenario installs a vector source directly on the SDK canvas with a
// same-origin template rather than going through the provider registry: the
// committed performance fixtures are the same recorded vector tile bytes at
// z7 and z8, and every tile address of a zoom resolves to that zoom's file.
// The dev server serves them statically under `/perf/fixtures/vector/…`; the
// route below is what makes one file answer every address, and it keeps the
// harness's addresses provider-neutral.
//
// This module is imported by both `run-fit-budget.mjs` (the host-machine
// measurement, outside `npm test`) and `tests/vector_fit_budget.spec.mjs` (the
// deterministic geometry gate), so the two drive the same page and the same
// fixture bytes.

import { readFile } from 'node:fs/promises';

/** The page that hosts one bare SDK canvas, with no playground frame pump. */
export const FIT_BUDGET_PAGE_PATH = '/perf/fit-budget.html';

/** The canvas the scenario measures: 1920 × 1080 at device scale 1. */
export const FIT_BUDGET_VIEWPORT = { width: 1920, height: 1080 };
export const FIT_BUDGET_DEVICE_SCALE_FACTOR = 1;

/** The camera the scenario fits: top-down, north up, over the fixture region. */
export const FIT_BUDGET_CENTER_LATITUDE_DEGREES = 38.57393751557591;
export const FIT_BUDGET_CENTER_LONGITUDE_DEGREES = -121.519775390625;

/** The zooms the scenario measures: a z14 start fitted down to z7. */
export const FIT_BUDGET_START_ZOOM_LEVEL = 14;
export const FIT_BUDGET_FIT_ZOOM_LEVEL = 7;

/** The highest zoom the committed fixtures serve; the source clamps to it. */
export const FIT_BUDGET_MAXIMUM_ZOOM_LEVEL = 8;

/** The budgets the scenario enforces (PLAN.md's M65 criteria). */
export const FIT_BUDGET_FIRST_FULL_FRAME_MILLISECONDS = 1500;
export const FIT_BUDGET_OVERLAY_TRIANGLE_COUNT = 400_000;
export const FIT_BUDGET_SETTLE_RENDER_P95_MILLISECONDS = 4;
/** The z8 unchanged settle-frame budget the label cache must hold. */
export const FIT_BUDGET_Z8_SETTLE_RENDER_P95_MILLISECONDS = 6;

/** The fixture file a zoom's tile address is answered with. */
const FIXTURE_FILE_PATH_PATTERN = /^\/perf\/fixtures\/vector\/(\d+)\/\d+\/\d+\.mvt$/;
const FIXTURE_ZOOM_LEVELS = new Set(['7', '8']);

/**
 * Routes every `/perf/fixtures/vector/{z}/{x}/{y}.mvt` request to the
 * committed `z{z}.mvt` bytes, so one file per zoom stands in for the whole
 * zoom's tile grid while the address keeps the provider-neutral XYZ shape.
 *
 * @param {import('@playwright/test').Page} page the page to install the route on.
 * @returns {Promise<void>} resolves once the route is installed.
 */
export async function installFitBudgetFixtureRoute(page) {
  await page.route(
    (url) => FIXTURE_FILE_PATH_PATTERN.test(url.pathname),
    async (route) => {
      const request_url = new URL(route.request().url());
      const fixture_match = request_url.pathname.match(FIXTURE_FILE_PATH_PATTERN);
      const zoom_segment = fixture_match?.[1];
      if (zoom_segment === undefined || !FIXTURE_ZOOM_LEVELS.has(zoom_segment)) {
        await route.continue();
        return;
      }
      const fixture_bytes = await readFile(
        new URL(`./fixtures/vector/z${zoom_segment}.mvt`, import.meta.url),
      );
      await route.fulfill({
        status: 200,
        contentType: 'application/x-protobuf',
        body: fixture_bytes,
      });
    },
  );
}

/**
 * Runs the fit scenario in the page and returns its measurement.
 *
 * @param {import('@playwright/test').Page} page a page on the fit-budget page.
 * @returns {Promise<object>} the scenario's samples and timings.
 */
export function runFitBudgetScenario(page, { measureZ8Settle = false } = {}) {
  return page.evaluate(
    ([startZoomLevel, fitZoomLevel, maximumZoomLevel, measureZ8Settle]) =>
      window.runFitBudgetScenario({
        startZoomLevel,
        fitZoomLevel,
        maximumZoomLevel,
        measureZ8Settle,
      }),
    [
      FIT_BUDGET_START_ZOOM_LEVEL,
      FIT_BUDGET_FIT_ZOOM_LEVEL,
      FIT_BUDGET_MAXIMUM_ZOOM_LEVEL,
      measureZ8Settle,
    ],
  );
}

/** The nearest-rank percentile of an ascending numeric list. */
export function percentile(sorted_values, fraction) {
  if (sorted_values.length === 0) return null;
  const rank = Math.ceil(fraction * sorted_values.length);
  return sorted_values[Math.min(sorted_values.length - 1, Math.max(0, rank - 1))];
}

/** Summarizes one metric list as count/min/p50/p95/p99/max. */
export function summarizeMetrics(values) {
  const sorted_values = [...values].sort((first, second) => first - second);
  if (sorted_values.length === 0) {
    return { count: 0, minimum: null, p50: null, p95: null, p99: null, maximum: null };
  }
  const round = (value) => Number(value.toFixed(2));
  return {
    count: sorted_values.length,
    minimum: round(sorted_values[0]),
    p50: round(percentile(sorted_values, 0.5)),
    p95: round(percentile(sorted_values, 0.95)),
    p99: round(percentile(sorted_values, 0.99)),
    maximum: round(sorted_values[sorted_values.length - 1]),
  };
}
