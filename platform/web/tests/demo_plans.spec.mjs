// The browser recorded-plan replay (M31 web v2 flows): the shared artifact's
// recorded plans stay available to launch automation, selecting one draws and
// frames its route, and the simulated trip replays the trace's own fixes
// through the wasm `Navigator` — the native `SimulatedTripDriver`'s
// recorded-trace path. Every fixture is same-origin: the dev server derives
// its trace allowlist from the artifact, so the tests reach no external
// network.
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

// The three committed scenarios the artifact declares.
const CLEAN_WALK_SCENARIO = 'sacramento_clean_walk';
const RECOVERY_SCENARIO = 'sacramento_deviation_recovery';
const REROUTE_SCENARIO = 'sacramento_deviation_reroute';

// The replay settles quickly once the 8x speed choice is selected: 143 clean
// samples at 5 s / (20 x 8) = 31.25 ms each.
// A software-rendered CI runner advances the replay's timer-driven playback
// several times slower than the host does; the arrival budget covers that.
const REPLAY_SETTLE_TIMEOUT_MILLISECONDS = 90_000;
// The recorded traces are long at the default pace; the off-route excursion
// occurs within the first third of the recovery trace.
const OFF_ROUTE_SETTLE_TIMEOUT_MILLISECONDS = 20_000;

test.beforeEach(async ({ page }) => {
  // `?advanced` opens the panel the way a visitor would; the map's wasm
  // readiness promise gates every assertion.
  await page.goto('/?advanced');
  await waitForMapTestReady(page);
});

test('selecting the clean walk frames its route and the replay arrives', async ({
  page,
}) => {
  // The replay walks the recorded trace in real time; a software-rendered CI
  // runner needs more than the suite's default budget to arrive.
  test.setTimeout(120_000);
  await page.evaluate(
    (scenario) => window.selectDemoPlanScenario(scenario), CLEAN_WALK_SCENARIO);

  await expect
    .poll(() => page.evaluate(() => window.mapDemoPlanState()?.selectedScenario), {
      timeout: REPLAY_SETTLE_TIMEOUT_MILLISECONDS,
    })
    .toBe(CLEAN_WALK_SCENARIO);

  // The recorded route is drawn (polyline plus the two endpoint pins) and the
  // camera is fitted inside the trace's own bounding box. The trace read is
  // the same allow-listed route the page fetched.
  const selection = await page.evaluate(async (scenario) => {
    const trace = await fetch(`navigation-traces/${scenario}.json`)
      .then((response) => response.json());
    return {
      plan_state: window.mapDemoPlanState(),
      bounding_box: trace.route.bounding_box,
      route_distance_meters: trace.route.distance_meters,
    };
  }, CLEAN_WALK_SCENARIO);

  expect(selection.plan_state.overlayKinds).toEqual([
    'routePolyline',
    'pointPins',
    'pointPins',
  ]);
  expect(selection.plan_state.routeDistanceMeters)
    .toBeCloseTo(selection.route_distance_meters, 6);
  // Framing centers the route in the inset viewport, so the control sheet can
  // shift the camera center slightly off the box's geographic center; the
  // camera is nonetheless on the route's neighborhood and at a fitting zoom.
  const { south_west, north_east } = selection.bounding_box;
  const bbox_center_latitude = (south_west.latitude + north_east.latitude) / 2;
  const bbox_center_longitude = (south_west.longitude + north_east.longitude) / 2;
  const FRAMING_NEIGHBORHOOD_TOLERANCE_DEGREES = 0.01;
  expect(Math.abs(selection.plan_state.camera.latitude - bbox_center_latitude))
    .toBeLessThanOrEqual(FRAMING_NEIGHBORHOOD_TOLERANCE_DEGREES);
  expect(Math.abs(selection.plan_state.camera.longitude - bbox_center_longitude))
    .toBeLessThanOrEqual(FRAMING_NEIGHBORHOOD_TOLERANCE_DEGREES);
  expect(selection.plan_state.camera.zoom).toBeGreaterThan(8);
  expect(selection.plan_state.camera.zoom).toBeLessThanOrEqual(17);

  // Starting the trip replays the trace: guidance enters navigating, then the
  // 8x pace reaches the recorded arrival.
  await page.locator('#follow-demo-start-control').click();
  await expect
    .poll(() => page.evaluate(() => window.mapNavigationSnapshot()?.phase), {
      timeout: REPLAY_SETTLE_TIMEOUT_MILLISECONDS,
    })
    .toBe('navigating');
  await expect(page.locator('#banner-instruction-text')).not.toHaveText('');
  await page
    .locator('#follow-demo-speed-controls button', { hasText: '8×' })
    .click();
  await expect
    .poll(() => page.evaluate(() => window.mapNavigationSnapshot()?.phase), {
      timeout: REPLAY_SETTLE_TIMEOUT_MILLISECONDS,
    })
    .toBe('arrived');
  await expect(page.locator('#navigation-phase-value')).toHaveText('Arrived');
  expect(await page.evaluate(
    () => window.mapNavigationSnapshot()?.instruction.progress.distanceRemainingMeters))
    .toBe(0);
  // A recorded trace has no route position to seek to, so the seek row is
  // hidden for its replay.
  await expect(page.locator('#follow-demo-seek-row')).toBeHidden();

  // Stop ends the trip and clears the card.
  await page.locator('#follow-demo-start-control').click();
  await expect(page.locator('#navigation-phase-value')).toHaveText('Not started');
  expect(await page.evaluate(() => window.mapNavigationSnapshot())).toBeNull();
});

test('the deviation recovery plan raises the off-route alert', async ({ page }) => {
  await page.evaluate(
    (scenario) => window.selectDemoPlanScenario(scenario), RECOVERY_SCENARIO);
  await expect
    .poll(() => page.evaluate(() => window.mapDemoPlanState()?.selectedScenario), {
      timeout: REPLAY_SETTLE_TIMEOUT_MILLISECONDS,
    })
    .toBe(RECOVERY_SCENARIO);

  // Open the navigation card so the alert's visibility is observable, the way
  // a visitor watching guidance would.
  await page.locator('#navigation-demo-panel summary').click();
  await page.locator('#follow-demo-start-control').click();

  // The trace's excursion is judged off route by the same reducer the native
  // apps drive: the phase freezes and the card's canonical alert appears.
  // The replay records the alert copy at the judgment, so the assertion does
  // not depend on the frozen phase's duration.
  await expect
    .poll(() => page.evaluate(() => window.mapDemoPlanState()?.offRouteObserved), {
      timeout: OFF_ROUTE_SETTLE_TIMEOUT_MILLISECONDS,
    })
    .toBe(true);
  const off_route_report = await page.evaluate(() => ({
    snapshot: window.mapNavigationSnapshot(),
    alert_text: window.mapDemoPlanState().offRouteAlertText,
  }));
  // The card's own alert copy at the judgment is the evidence: the trace's
  // excursion froze guidance and raised the canonical off-route sentence.
  expect(off_route_report.alert_text).toContain('Off route');
  // The frozen phase is still on screen within the excursion's window; the
  // guidance payload carries the reroute-required marker the native card
  // shows.
  if (off_route_report.snapshot?.phase === 'offRoute') {
    expect(off_route_report.snapshot.deviation.statusChangeKind).toBe('OffRoute');
    expect(off_route_report.snapshot.instruction.isReroutingRequired).toBe(true);
    await expect(page.locator('#deviation-alert-preview')).toBeVisible();
    await expect(page.locator('#navigation-phase-value')).toHaveText('Off route');
  }

  // Stop ends the replay honestly.
  await page.locator('#follow-demo-start-control').click();
  await expect(page.locator('#navigation-phase-value')).toHaveText('Not started');
});

test('the deviation reroute plan applies its recorded replacement route', async ({
  page,
}) => {
  await page.evaluate(
    (scenario) => window.selectDemoPlanScenario(scenario), REROUTE_SCENARIO);
  await expect
    .poll(() => page.evaluate(() => window.mapDemoPlanState()?.selectedScenario), {
      timeout: REPLAY_SETTLE_TIMEOUT_MILLISECONDS,
    })
    .toBe(REROUTE_SCENARIO);

  await page.locator('#follow-demo-start-control').click();
  await page
    .locator('#follow-demo-speed-controls button', { hasText: '8×' })
    .click();

  // The replay applies the fixture's replacement route at the first off-route
  // judgment (the native driver's reroute policy); afterwards the deviation
  // is lifted and the guidance navigates the replacement route.
  await expect
    .poll(() => page.evaluate(() => window.mapDemoPlanState()?.rerouteApplied), {
      timeout: REPLAY_SETTLE_TIMEOUT_MILLISECONDS,
    })
    .toBe(true);
  await expect
    .poll(
      () => page.evaluate(
        () => window.mapNavigationSnapshot()?.instruction.isReroutingRequired),
      { timeout: REPLAY_SETTLE_TIMEOUT_MILLISECONDS },
    )
    .toBe(false);
  await expect
    .poll(() => page.evaluate(() => window.mapNavigationSnapshot()?.deviation), {
      timeout: REPLAY_SETTLE_TIMEOUT_MILLISECONDS,
    })
    .toBeNull();

  await page.locator('#follow-demo-start-control').click();
  await expect(page.locator('#navigation-phase-value')).toHaveText('Not started');
});
