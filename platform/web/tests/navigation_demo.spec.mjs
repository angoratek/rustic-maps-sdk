// Live guidance on the browser seam (M29): the demo's navigation card is
// driven by the wasm `Navigator` — the same navigation reducer the native
// controllers use — rather than the retired layout preview. The trip is the
// keyless recorded route; every provider fact comes from the shared artifact.
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

/** The demo's fix cadence and the guidance settle budget. */
const GUIDANCE_SETTLE_TIMEOUT_MILLISECONDS = 20_000;

test('the guidance card follows the simulated trip instead of a preview', async ({
  page,
}) => {
  await page.goto('/');
  await waitForMapTestReady(page);

  // Before a route is planned the card is idle and says so; the old preview
  // notice and its fixed samples are gone.
  await expect(page.locator('#navigation-demo-reason')).toHaveText(
    'Plan a route, then start the simulated trip to see live guidance.');
  await expect(page.locator('#navigation-phase-value')).toHaveText('Not started');
  await expect(page.locator('#navigation-demo-preview-notice')).toBeHidden();
  // The ETA line is present but holds no remaining values until guidance runs.
  await expect(page.locator('#navigation-eta-line')).toContainText('ETA');
  await expect(page.locator('#navigation-eta-distance-value')).toHaveText('—');
  await expect(page.locator('#navigation-eta-duration-value')).toHaveText('—');
  // The turn banner sits over the map only while guidance runs.
  await expect(page.locator('#navigation-banner-overlay')).toBeHidden();
  expect(await page.evaluate(() => window.mapNavigationSnapshot())).toBeNull();

  // Plan the recorded Capitol Park → Tower Bridge route, keyless.
  await page.locator('#trip-origin-input').fill('Capitol Park');
  await page.locator('#trip-destination-input').fill('Tower Bridge');
  await page.locator('#trip-destination-search-control').click();
  await page.locator('#trip-search-results button').first().click();
  await page.locator('#trip-plan-route-control').click();
  await expect(page.locator('#trip-route-result')).toContainText(
    'Recorded demo route');

  // Starting the trip starts live guidance on that route: the banner already
  // names the first maneuver when the first snapshot arrives.
  await page.locator('#follow-demo-start-control').click();
  await expect
    .poll(() => page.evaluate(() => window.mapNavigationSnapshot()?.phase), {
      timeout: GUIDANCE_SETTLE_TIMEOUT_MILLISECONDS,
    })
    .toBe('navigating');
  await expect(page.locator('#navigation-demo-reason')).toBeHidden();
  await expect(page.locator('#navigation-phase-value')).toHaveText('Navigating');
  await expect(page.locator('#banner-instruction-text')).not.toHaveText('');
  await expect(page.locator('#upcoming-instruction-text')).toContainText('Then:');
  // The ETA line states the same two labelled remaining values the native
  // drawers show, and the turn banner is placed over the map.
  await expect(page.locator('#navigation-eta-distance-value')).not.toHaveText('—');
  await expect(page.locator('#navigation-eta-duration-value')).not.toHaveText('—');
  await expect(page.locator('#navigation-banner-overlay')).toBeVisible();

  const first_snapshot = await page.evaluate(() => window.mapNavigationSnapshot());
  expect(first_snapshot.instruction.activeManeuver.providerDisplayText)
    .not.toBeNull();
  expect(first_snapshot.instruction.progress.distanceRemainingMeters)
    .toBeGreaterThan(0);
  expect(first_snapshot.deviation).toBeNull();

  // The banner shows the provider's own sentence (the phrase line carries the
  // distance), so the distance is not printed twice inside one banner. The
  // two reads happen in one synchronous evaluation, so a fix cannot land
  // between them and make the assertion race the live trip.
  const rendered_banner = await page.evaluate(() => ({
    banner_text: document.getElementById('banner-instruction-text').textContent,
    distance_phrase_text:
      document.getElementById('banner-distance-phrase-text').textContent,
    snapshot: window.mapNavigationSnapshot(),
  }));
  expect(rendered_banner.banner_text).toBe(
    rendered_banner.snapshot.instruction.activeManeuver.providerDisplayText);
  expect(rendered_banner.distance_phrase_text).not.toBe('');

  // The walk advances: distance to the next maneuver and remaining distance
  // both fall, and the step index moves off the first step.
  await expect
    .poll(async () => page.evaluate(
      () => window.mapNavigationSnapshot()?.instruction.progress
        .distanceRemainingMeters ?? Infinity), {
      timeout: GUIDANCE_SETTLE_TIMEOUT_MILLISECONDS,
    })
    .toBeLessThan(first_snapshot.instruction.progress.distanceRemainingMeters);
  const later_snapshot = await page.evaluate(() => window.mapNavigationSnapshot());
  expect(later_snapshot.instruction.progress.distanceToNextManeuverMeters)
    .toBeLessThanOrEqual(first_snapshot.instruction.progress.distanceToNextManeuverMeters);
  expect(later_snapshot.currentStepIndex).toBeGreaterThanOrEqual(
    first_snapshot.currentStepIndex);

  // Seeking to the end replays the route into the navigator, so the card
  // arrives with the trip instead of freezing on the first step (a single
  // teleported fix cannot advance a forward-only reducer).
  const seek_maximum_meters = await page
    .locator('#follow-demo-seek-control')
    .getAttribute('max');
  await page.evaluate((value) => {
    const seek_control = document.getElementById('follow-demo-seek-control');
    seek_control.value = value;
    seek_control.dispatchEvent(new Event('input', { bubbles: true }));
    seek_control.dispatchEvent(new Event('change', { bubbles: true }));
  }, seek_maximum_meters);
  await expect(page.locator('#navigation-phase-value')).toHaveText('Arrived');
  expect(await page.evaluate(() => window.mapNavigationSnapshot()?.isArrived))
    .toBe(true);
  expect(await page.evaluate(
    () => window.mapNavigationSnapshot()?.instruction.progress.distanceRemainingMeters))
    .toBe(0);

  // Stop ends the trip and clears the card.
  await page.locator('#follow-demo-start-control').click();
  await expect(page.locator('#navigation-phase-value')).toHaveText('Not started');
  expect(await page.evaluate(() => window.mapNavigationSnapshot())).toBeNull();
});

test('a fix beyond the deviation threshold freezes guidance and raises the alert', async ({
  page,
}) => {
  await page.goto('/');
  await waitForMapTestReady(page);

  // One trip on the artifact's own recorded route, driven directly through
  // the seam so the test can place the fixes: on the route first, then far
  // off it. No panel or camera is involved in this case.
  const deviation_report = await page.evaluate(async () => {
    const artifact = await fetch('reference-scenes.json').then((response) =>
      response.json());
    // The artifact stores the recorded route in its snake-case JSON spelling;
    // the seam reads the contract's camel-case record.
    const recorded_route = artifact.demo_routes[0].route;
    const route = {
      distanceMeters: recorded_route.distance_meters,
      durationSeconds: recorded_route.duration_seconds,
      boundingBox: {
        southWest: recorded_route.bounding_box.south_west,
        northEast: recorded_route.bounding_box.north_east,
      },
      legs: recorded_route.legs.map((recorded_leg) => ({
        distanceMeters: recorded_leg.distance_meters,
        durationSeconds: recorded_leg.duration_seconds,
        steps: recorded_leg.steps.map((recorded_step) => ({
          distanceMeters: recorded_step.distance_meters,
          durationSeconds: recorded_step.duration_seconds,
          maneuverType: recorded_step.maneuver_type,
          instructionDisplayText: recorded_step.instruction_display_text,
          instructionSpeechText: recorded_step.instruction_speech_text,
          shape: recorded_step.shape,
        })),
      })),
    };
    const first_coordinate = route.legs[0].steps[0].shape[0];
    const navigator = new window.mapTest.Navigator();
    navigator.startNavigation(route, 'metric');
    const on_route = navigator.updateLocation({
      coordinate: first_coordinate,
      horizontalAccuracyMeters: 8.0,
      courseHeadingDegrees: null,
      timestampSeconds: null,
    });
    const off_route = navigator.updateLocation({
      coordinate: {
        latitude: first_coordinate.latitude + 0.002,
        longitude: first_coordinate.longitude,
      },
      horizontalAccuracyMeters: 8.0,
      courseHeadingDegrees: null,
      timestampSeconds: null,
    });
    navigator.stopNavigation();
    return { on_route, off_route, has_active_trip_after_stop: navigator.hasActiveTrip() };
  });

  expect(deviation_report.on_route.phase).toBe('navigating');
  expect(deviation_report.on_route.deviation).toBeNull();
  expect(deviation_report.off_route.phase).toBe('offRoute');
  expect(deviation_report.off_route.deviation).not.toBeNull();
  expect(deviation_report.off_route.deviation.statusChangeKind).toBe('OffRoute');
  expect(deviation_report.off_route.deviation.distanceFromRouteMeters)
    .toBeGreaterThan(100);
  expect(deviation_report.off_route.instruction.isReroutingRequired).toBe(true);
  expect(deviation_report.has_active_trip_after_stop).toBe(false);
});
