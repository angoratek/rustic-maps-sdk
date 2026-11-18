// Event-driven navigation on the wasm seam (M31): the contract's
// `NavigationListener` topics reach JavaScript as one callback per event —
// instructions, phase transitions, deviation updates, speech and the
// wasm-only stop event — instead of only being readable by polling
// `currentTripState`. The route comes from the committed provider fixtures
// through the same registered provider the other live cases use (no external
// network); the fixes are derived from the route's own step shapes.
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

const TEST_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const REPOSITORY_ROOT = path.resolve(TEST_DIRECTORY, '..', '..', '..');
const providers_registry = JSON.parse(
  readFileSync(path.join(REPOSITORY_ROOT, 'platform', 'providers.json'), 'utf8'),
);
const provider_descriptor = providers_registry.providers[0];
const PROVIDER_KEY = provider_descriptor.provider_key;
const PROVIDER_WASM_REGISTRATION = provider_descriptor.wasm_registration;
const PROVIDER_FIXTURE_BASE_PATH = providers_registry.fixture_base_path;
// The recorded fixture route's endpoints (the provider crate's fixture test
// pins them): Capitol Park to Tower Bridge.
const ORIGIN = { latitude: 38.57693, longitude: -121.49492 };
const DESTINATION = { latitude: 38.57874, longitude: -121.50745 };
// A fix far enough north to exceed the default deviation allowance.
const OFF_ROUTE_NORTH_METERS = 200;
const METERS_PER_DEGREE_LATITUDE = 111_320;

test.beforeEach(async ({ page }) => {
  await page.goto('/?advanced');
  await waitForMapTestReady(page);
  await page.evaluate(
    ([registration_key, provider_key, fixture_base_path]) => {
      window.mapTest.setProviderBaseUrl(
        registration_key,
        `${window.location.origin}${fixture_base_path}/${provider_key}`,
      );
      window.mapTest.configureProviderSet(provider_key, 'test-key');
    },
    [PROVIDER_WASM_REGISTRATION, PROVIDER_KEY, PROVIDER_FIXTURE_BASE_PATH],
  );
});

test('the navigator delivers instructions, phase, deviation, speech and stop events', async ({
  page,
}) => {
  const observation = await page.evaluate(
    async ([origin, destination, off_route_north_meters, meters_per_degree]) => {
      const route = await window.mapTest.calculateRoute(
        origin.latitude,
        origin.longitude,
        destination.latitude,
        destination.longitude,
      );
      const navigator = new window.mapTest.Navigator();
      const events = [];
      const listener_id = navigator.addNavigationListener((event) => {
        events.push(event);
      });
      navigator.startNavigation(route, 'metric');

      const first_step = route.legs[0].steps[0];
      const on_route_coordinate = first_step.shape[0];
      const off_route_coordinate = {
        latitude:
          on_route_coordinate.latitude
          + off_route_north_meters / meters_per_degree,
        longitude: on_route_coordinate.longitude,
      };
      const on_route_snapshot = navigator.updateLocation({
        coordinate: on_route_coordinate,
        horizontalAccuracyMeters: 2,
        courseHeadingDegrees: null,
        timestampSeconds: null,
      });
      const off_route_snapshot = navigator.updateLocation({
        coordinate: off_route_coordinate,
        horizontalAccuracyMeters: 2,
        courseHeadingDegrees: null,
        timestampSeconds: null,
      });
      navigator.stopNavigation();
      const removed = navigator.removeNavigationListener(listener_id);
      return {
        events,
        removed,
        on_route_phase: on_route_snapshot?.phase ?? null,
        off_route_phase: off_route_snapshot?.phase ?? null,
      };
    },
    [ORIGIN, DESTINATION, OFF_ROUTE_NORTH_METERS, METERS_PER_DEGREE_LATITUDE],
  );

  const topics = observation.events.map((event) => event.topic);
  expect(topics[0]).toBe('instruction');
  expect(topics).toContain('phase');
  expect(topics).toContain('speech');
  expect(topics).toContain('deviation');
  expect(topics[topics.length - 1]).toBe('stop');
  expect(observation.removed).toBe(true);
  expect(observation.on_route_phase).toBe('navigating');
  expect(observation.off_route_phase).toBe('offRoute');

  // The phase topic names the transition the native listener reports, and
  // the deviation topic carries the off-route snapshot.
  const off_route_phase_event = observation.events.find(
    (event) => event.topic === 'phase' && event.phaseChange === 'offRouteEntered',
  );
  expect(off_route_phase_event).toBeTruthy();
  expect(off_route_phase_event.snapshot.phase).toBe('offRoute');
  const deviation_event = observation.events.find(
    (event) => event.topic === 'deviation',
  );
  expect(deviation_event.snapshot.deviation).not.toBeNull();
  expect(deviation_event.snapshot.deviation.distanceFromRouteMeters)
    .toBeGreaterThan(0);

  // The speech topic carries the composition's own speech text, never the
  // banner sentence.
  const speech_event = observation.events.find(
    (event) => event.topic === 'speech',
  );
  expect(typeof speech_event.speechText).toBe('string');
  expect(speech_event.speechText.length).toBeGreaterThan(0);
  expect(speech_event.snapshot.instruction.activeManeuver.speechText)
    .toBe(speech_event.speechText);

  // Every instruction event carries a snapshot whose phase is one of the
  // seam's spellings.
  for (const event of observation.events.filter(
    (candidate) => candidate.topic === 'instruction',
  )) {
    expect(['navigating', 'offRoute', 'arrived']).toContain(event.snapshot.phase);
  }
});
