/**
 * The keyless plan flow's shared-artifact helpers (§4.1): the search filters
 * the artifact's `demo_places`, and the plan resolves the artifact's
 * `demo_routes` pair — the same data the web demo serves, never a provider
 * call and never invented geometry.
 *
 * These are the exact helpers the shell consumes when the running session has
 * no provider credential applied, so the assertions cover the shell's keyless
 * decisions without a native side (jest has no SDK).
 */

import {
  demoPlaceNamed,
  demoPlaces,
  demoRoutes,
  recordedDemoRoute,
  searchDemoPlaces,
} from '../src/referenceScenes';

test('the keyless search returns the shared artifact demo places', () => {
  expect(demoPlaces.length).toBeGreaterThan(0);
  const firstDemoPlace = demoPlaces[0];
  const queryText = firstDemoPlace.name.slice(0, 5);
  const matchingRows = searchDemoPlaces(queryText);
  expect(matchingRows.length).toBeGreaterThan(0);
  expect(
    matchingRows.some(row => row.structuredAddressText === firstDemoPlace.name),
  ).toBe(true);
  expect(matchingRows.every(row => row.isDemoPlace === true)).toBe(true);
  expect(matchingRows.every(row => row.noteText === firstDemoPlace.note)).toBe(true);
  // No match is an honest empty answer, never a guessed row.
  expect(searchDemoPlaces('no such demo place')).toEqual([]);
});

test('the keyless plan of a recorded pair produces the recorded route', () => {
  const recordedPair = demoRoutes[0];
  expect(recordedPair).toBeDefined();
  const origin = demoPlaceNamed(recordedPair.origin_name);
  const destination = demoPlaceNamed(recordedPair.destination_name);
  expect(origin).not.toBeNull();
  expect(destination).not.toBeNull();
  const resolvedRoute = recordedDemoRoute(origin!.name, destination!.name);
  expect(resolvedRoute).not.toBeNull();
  expect(resolvedRoute!.route.distance_meters).toBeGreaterThan(0);
  expect(resolvedRoute!.route.duration_seconds).toBeGreaterThan(0);
  expect(resolvedRoute!.route.legs.length).toBeGreaterThan(0);
  expect(resolvedRoute!.route.legs[0].steps.length).toBeGreaterThan(0);
  // An unrecorded pair answers null, so the shell can say so honestly.
  expect(recordedDemoRoute(origin!.name, origin!.name)).toBeNull();
});
