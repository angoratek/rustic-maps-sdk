// The consumer trip flow (round 26): a visitor searches the shared demo
// places, selects a destination, plans the recorded route and starts the
// simulated trip without a provider key, while the camera/scene/debug controls
// stay folded under Advanced.
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

test('a keyless visitor plans any demo pair and starts the recorded trip', async ({
  page,
}) => {
  await page.goto('/');
  await waitForMapTestReady(page);

  // The consumer surface is the trip planner; the expert controls start
  // folded away.
  expect(await page.evaluate(
    () => document.getElementById('advanced-panel').open)).toBe(false);
  await expect(page.locator('#trip-planner')).toBeVisible();
  await expect(page.locator('#trip-origin-input')).toBeVisible();
  await expect(page.locator('#trip-destination-input')).toBeVisible();

  // Two searches, as every route planner: Plan route stays disabled with a
  // named reason until both fields hold text.
  await expect(page.locator('#trip-plan-route-control')).toBeDisabled();
  await expect(page.locator('#trip-plan-route-reason')).toHaveText(
    'Enter an origin and a destination to plan the route.');
  await page.locator('#trip-origin-input').fill('Capitol Park');
  await expect(page.locator('#trip-plan-route-control')).toBeDisabled();
  await expect(page.locator('#trip-plan-route-reason')).toHaveText(
    'Enter a destination to plan the route.');

  // A keyless search filters the shared demo places into the focused field.
  await page.locator('#trip-destination-search-control').click();
  await expect(page.locator('#provider-credential-status')).toHaveText(
    'Enter a destination first.');
  await page.locator('#trip-destination-input').fill('Tower');
  await page.locator('#trip-destination-search-control').click();
  const result_buttons = page.locator('#trip-search-results button');
  await expect(result_buttons).toHaveCount(1);
  await expect(result_buttons.first()).toContainText('Tower Bridge');
  await result_buttons.first().click();
  expect(await page.locator('#trip-destination-input').inputValue())
    .toBe('Tower Bridge');
  await expect(page.locator('#trip-search-results')).toBeHidden();

  // Both fields populated: the action enables and plans the recorded route
  // between them, draws the polyline and the A/B pins, and frames the route.
  await expect(page.locator('#trip-plan-route-control')).toBeEnabled();
  await expect(page.locator('#trip-plan-route-reason')).toBeHidden();
  await page.locator('#trip-plan-route-control').click();
  await expect(page.locator('#trip-route-result')).toContainText(
    'Capitol Park \u2192 Tower Bridge');
  await expect(page.locator('#trip-route-result')).toContainText(
    'Recorded demo route, pedestrian, fastest');
  await expect(page.locator('#provider-credential-status')).toContainText(
    'Recorded demo route ready');
  await expect
    .poll(async () => page.evaluate(
      () => window.mapLastFrameStats()?.overlayTriangles ?? 0), { timeout: 20_000 })
    .toBeGreaterThan(64);

  // The camera frames the route's box: both endpoints project inside the
  // canvas, and the route sits inside it with margin.
  const framing = await page.evaluate(() => {
    const state = window.mapCameraState();
    const canvas = document.getElementById('map-canvas');
    const camera = new window.mapTest.MapCamera(
      state.latitude, state.longitude, state.zoom, state.bearing, state.pitch);
    const project = (latitude, longitude) => camera.projectToScreen(
      latitude, longitude, canvas.clientWidth, canvas.clientHeight, 60);
    return {
      zoom: state.zoom,
      origin: project(38.57693, -121.49492),
      destination: project(38.578739, -121.507447),
      width: canvas.clientWidth,
      height: canvas.clientHeight,
    };
  });
  for (const point of [framing.origin, framing.destination]) {
    expect(point.x).toBeGreaterThanOrEqual(0);
    expect(point.x).toBeLessThanOrEqual(framing.width);
    expect(point.y).toBeGreaterThanOrEqual(0);
    expect(point.y).toBeLessThanOrEqual(framing.height);
  }

  // Starting the trip engages the follow camera over the recorded trace.
  await page.locator('#follow-demo-start-control').click();
  await expect
    .poll(async () => page.evaluate(
      () => window.mapFollowDemoState().phase), { timeout: 15000 })
    .toBe('engaged');
});

test('suggestion chips fill the focused field and Enter searches', async ({
  page,
}) => {
  await page.goto('/');
  await waitForMapTestReady(page);

  // The demo places are offered as one-tap suggestions, so a visitor never
  // has to guess a query to start; a chip fills the field that has focus.
  const suggestion_chips = page.locator('#trip-suggestions button');
  await expect(suggestion_chips.first()).toHaveText('Capitol Park');
  await page.locator('#trip-origin-input').focus();
  await suggestion_chips.first().click();
  await expect(page.locator('#provider-credential-status')).toContainText(
    'Selected Capitol Park as the origin');
  expect(await page.locator('#trip-origin-input').inputValue()).toBe('Capitol Park');
  await page.locator('#trip-destination-input').focus();
  await suggestion_chips.nth(1).click();
  expect(await page.locator('#trip-destination-input').inputValue())
    .toBe('Tower Bridge');
  await expect(page.locator('#trip-search-results')).toBeHidden();
  await expect(page.locator('#trip-plan-route-control')).toBeEnabled();

  // Enter in a field runs that field's search without hunting for a button.
  await page.locator('#trip-origin-input').fill('Old');
  await page.locator('#trip-origin-input').press('Enter');
  const result_buttons = page.locator('#trip-search-results button');
  await expect(result_buttons.first()).toContainText('Old Sacramento');
  await expect(page.locator('#trip-search-results')).toBeVisible();

  // A pair the recorded artifact covers plans without a key; the status
  // names the route honestly rather than pretending a live route was planned.
  await result_buttons.first().click();
  await page.locator('#trip-plan-route-control').click();
  await expect(page.locator('#trip-route-result')).toContainText(
    'Old Sacramento \u2192 Tower Bridge');
  await expect(page.locator('#trip-route-result')).toContainText(
    'Recorded demo route');

  // The settings sheet is the credential surface: appearance sits beside the
  // provider rows, and Escape returns to the map.
  await page.click('#settings-control');
  await expect(page.locator('#provider-credentials-panel')).toBeVisible();
  await expect(page.locator('#provider-credential-rows')).not.toBeEmpty();
  await page.keyboard.press('Escape');
  expect(await page.evaluate(
    () => document.getElementById('settings-dialog').open)).toBe(false);
});

test('the keyless trip card opens settings with the credential field focused', async ({
  page,
}) => {
  await page.goto('/');
  await waitForMapTestReady(page);

  // The honest keyless state carries the primary onboarding action.
  await expect(page.locator('#provider-credential-status')).toHaveText(
    'Set a provider key to load live map data');
  const credential_action = page.locator('#trip-credential-action');
  await expect(credential_action).toContainText('Add a provider key');
  await expect(credential_action).toHaveAttribute('data-state', 'keyless');

  // One click opens the native dialog and lands in the credential field,
  // not on the close control.
  await credential_action.click();
  await expect(page.locator('#settings-dialog')).toBeVisible();
  await expect(
    page.locator('#provider-credential-rows input[data-provider-credential-input]').first(),
  ).toBeFocused();

  // Credentials are the first section; capabilities and appearance follow,
  // the same order the native settings sheets use. The base map lives in the
  // floating layers control, not here.
  await expect(page.locator('.settings-section-title')).toHaveText([
    'Provider credentials',
    'Provider capabilities',
    'Location',
    'Theme',
  ]);

  // Escape closes the modal and returns focus to the action that opened it.
  await page.keyboard.press('Escape');
  await expect(page.locator('#settings-dialog')).toBeHidden();
  await expect(credential_action).toBeFocused();
});

test('the phone sheet clears the bottom docks, wraps chips and reaches Advanced', async ({
  page,
}) => {
  // The sheet must never draw over the status line, scale bar, attribution
  // or a visible error strip, at a tall and a short phone viewport.
  const dock_clearance = async () => page.evaluate(() => {
    const sheet = document.getElementById('controls').getBoundingClientRect();
    const dock_rects = ['status-line', 'scale-bar-overlay', 'attribution-info-control']
      .map((id) => document.getElementById(id).getBoundingClientRect())
      .filter((rect) => rect.height > 0);
    return {
      sheet_bottom: sheet.bottom,
      dock_top: Math.min(...dock_rects.map((rect) => rect.top)),
    };
  });

  for (const viewport of [{ width: 390, height: 844 }, { width: 390, height: 700 }]) {
    await page.setViewportSize(viewport);
    await page.goto('/');
    await waitForMapTestReady(page);
    // The scale bar appears with the first settled frame; its height is part
    // of the dock the sheet reserves.
    await page.waitForFunction(
      () => ['ready', 'degraded', 'failed'].includes(
        document.getElementById('status-line').dataset.readiness),
      null,
      { timeout: 20000 },
    );
    await page.waitForFunction(
      () => !document.getElementById('scale-bar-overlay').hidden,
      null,
      { timeout: 20000 },
    );
    await page.waitForTimeout(200);

    // At rest the sheet sits a gap above the bottom-start dock and below the
    // horizontal map-control row.
    const at_rest = await dock_clearance();
    expect(at_rest.sheet_bottom).toBeLessThanOrEqual(at_rest.dock_top - 7);
    const cluster_sheet = await page.evaluate(() => {
      const cluster = document.getElementById('map-control-cluster').getBoundingClientRect();
      const sheet = document.getElementById('controls').getBoundingClientRect();
      return { cluster_bottom: cluster.bottom, sheet_top: sheet.top };
    });
    expect(cluster_sheet.cluster_bottom).toBeLessThanOrEqual(cluster_sheet.sheet_top - 7);

    // At rest the trip card's primary actions fit at 390x844. The two-search
    // card is taller than the single-search one, so Advanced may sit below
    // the fold; the sheet's own scroll reaches it (asserted below).
    const initial_actions = await page.evaluate(() => {
      const sheet = document.getElementById('controls').getBoundingClientRect();
      const actions = document.querySelector('.trip-actions').getBoundingClientRect();
      return { sheet_bottom: sheet.bottom, actions_bottom: actions.bottom };
    });
    if (viewport.height === 844) {
      expect(initial_actions.actions_bottom)
        .toBeLessThanOrEqual(initial_actions.sheet_bottom + 1);
    }

    // A live error strip grows the dock; the observed height pushes the
    // sheet up instead of letting the error draw over it.
    await page.evaluate(() => {
      const error_report = document.getElementById('error-report');
      error_report.hidden = false;
      error_report.textContent =
        'Live map data: the provider refused the request (rate limit exceeded).';
    });
    await page.waitForTimeout(250);
    const with_error = await dock_clearance();
    expect(with_error.sheet_bottom).toBeLessThanOrEqual(with_error.dock_top - 7);

    // Demo-place chips wrap whole: neither the row nor a chip is clipped.
    const chip_metrics = await page.evaluate(() => {
      const row = document.getElementById('trip-suggestions');
      return {
        row_scroll_width: row.scrollWidth,
        row_client_width: row.clientWidth,
        chips: Array.from(row.querySelectorAll('button')).map((chip) => ({
          label: chip.textContent,
          scroll_width: chip.scrollWidth,
          client_width: chip.clientWidth,
        })),
      };
    });
    expect(chip_metrics.row_scroll_width).toBeLessThanOrEqual(chip_metrics.row_client_width);
    for (const chip of chip_metrics.chips) {
      expect(chip.scroll_width).toBeLessThanOrEqual(chip.client_width);
    }
    expect(chip_metrics.chips.map((chip) => chip.label))
      .toContain('Old Sacramento');

    // On the shorter viewport, or while the error strip is up, the sheet's
    // own scroll still reaches Advanced.
    await page.evaluate(() => {
      const controls = document.getElementById('controls');
      controls.scrollTop = controls.scrollHeight;
    });
    await page.waitForTimeout(100);
    const reached_advanced = await page.evaluate(() => {
      const sheet = document.getElementById('controls').getBoundingClientRect();
      const advanced = document.getElementById('advanced-panel').getBoundingClientRect();
      return {
        sheet_top: sheet.top,
        sheet_bottom: sheet.bottom,
        advanced_top: advanced.top,
        advanced_bottom: advanced.bottom,
      };
    });
    expect(reached_advanced.advanced_top)
      .toBeGreaterThanOrEqual(reached_advanced.sheet_top - 1);
    expect(reached_advanced.advanced_bottom)
      .toBeLessThanOrEqual(reached_advanced.sheet_bottom + 1);

    // The sheet header's gear is not intercepted by the map-control row.
    await page.locator('#settings-control').click();
    await expect(page.locator('#settings-dialog')).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(page.locator('#settings-dialog')).toBeHidden();
  }
});

test('the renderer readout stays hidden until enabled under Advanced debug', async ({
  page,
}) => {
  await page.goto('/?advanced');
  await waitForMapTestReady(page);

  // Developer chrome is off for a first visitor.
  await expect(page.locator('#readout')).toBeHidden();

  await page.locator('#renderer-readout-control').check();
  await expect(page.locator('#readout')).toBeVisible();
  await expect(page.locator('#readout')).toContainText('mode ');

  await page.locator('#renderer-readout-control').uncheck();
  await expect(page.locator('#readout')).toBeHidden();
});
