// The wasm offline routing seam (milestone M7: the wasm target loads the
// offline routing graph from in-memory archive bytes — no `mmap` in the
// browser): `window.mapTest.offlineRouteFromArchiveBytes` fetches the
// committed compiled `RMOG` fixture same-origin (every fixture the harness
// touches is same-origin; the browser reaches no external network), loads
// the bytes into the in-repo
// `rustic-maps-core::offline_routing::ArchiveOfflineRoutingProvider`, and answers
// the route's maneuver texts from the graph's derived geometry. The
// expected texts and distances are the ones the rustic-maps-core fixture tests pin
// (the committed routing-grid fixture, compiled by the same deterministic
// compiler the byte-equality guard runs).
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

// The committed compiled archive fixture `serve.mjs` serves same-origin.
const OFFLINE_GRAPH_FIXTURE_URL = '/offline-graph/routing_grid.rmog';

// The committed fixture's grid facts (the compiler crate's round-trip test
// pins them): axis-aligned residential segments, 0.001 degrees each, at the
// quantized 30 km/h residential speed.
const GRID_SEGMENT_METERS = 111;
const RESIDENTIAL_SEGMENT_DURATION_SECONDS = 13.32;

// Fixture node coordinates (osm ids as pinned in the compiler's round-trip
// test), as `[latitude, longitude]` degree pairs: the L-shaped two-way
// avenue, the residential oneway street.
const FIXTURE_NODE_1001 = [0.0, 0.0];
const FIXTURE_NODE_1004 = [0.001, 0.0];
const FIXTURE_NODE_1005 = [0.001, 0.001];
const FIXTURE_NODE_1006 = [0.001, 0.002];

// The route answers' numeric grid facts.
const TWO_SEGMENT_ROUTE_METERS = 2.0 * GRID_SEGMENT_METERS;
const TWO_SEGMENT_ROUTE_SECONDS = 2.0 * RESIDENTIAL_SEGMENT_DURATION_SECONDS;

test.beforeEach(async ({ page }) => {
  await page.goto('/?advanced');
  // The Rust-side `initializeSdk` promise gates every assertion.
  await waitForMapTestReady(page);
});

test('the in-memory offline graph answers the grid’s derived turn maneuvers', async ({
  page,
}) => {
  // 1005 → 1001: south along the L's spine, then the ninety-degree junction
  // at 1002 turns the route west — a derived right turn, whose banner texts
  // ride end-anchored on the departure step (the rustic-maps-core fixture test's
  // expected texts).
  const route = await route_from_fixture_bytes(page, FIXTURE_NODE_1005, FIXTURE_NODE_1001);
  expect(route.legs).toHaveLength(1);
  expect(route.distanceMeters).toBe(TWO_SEGMENT_ROUTE_METERS);
  expect(route.durationSeconds).toBeCloseTo(TWO_SEGMENT_ROUTE_SECONDS, 9);
  const steps = route.legs[0].steps;
  expect(steps).toHaveLength(3);
  expect(
    steps.map((step) => step.maneuverType),
  ).toEqual(['departure', 'turn-right', 'arrival']);
  expect(
    steps.map((step) => step.instructionDisplayText),
  ).toEqual(['Turn right', 'Turn right', 'Arrival']);
  expect(
    steps.map((step) => step.instructionSpeechText),
  ).toEqual(['Turn right', 'Turn right', 'Arrival']);
  expect(
    steps.map((step) => step.shape.length),
  ).toEqual([2, 2, 1]);
  // Quantized archive coordinates round-trip with float noise; the same
  // resolution-derived tolerance the rustic-maps-core fixture tests assert against.
  const bounding_box_tolerance_degrees = 1e-6;
  const asserted_corners = [
    [route.boundingBox.southWest, FIXTURE_NODE_1001],
    [route.boundingBox.northEast, FIXTURE_NODE_1005],
  ];
  for (const [corner, fixture_node] of asserted_corners) {
    expect(Math.abs(corner.latitude - fixture_node[0])).toBeLessThan(
      bounding_box_tolerance_degrees,
    );
    expect(Math.abs(corner.longitude - fixture_node[1])).toBeLessThan(
      bounding_box_tolerance_degrees,
    );
  }
});

test('the oneway grid street routes straight through, departure to arrival', async ({
  page,
}) => {
  // 1004 → 1006 is the oneway street's stored direction (east): two
  // straight runs inside the derived-maneuver straight band, so the route
  // carries the plain departure/arrival texts and nothing is invented.
  const route = await route_from_fixture_bytes(page, FIXTURE_NODE_1004, FIXTURE_NODE_1006);
  expect(route.distanceMeters).toBe(TWO_SEGMENT_ROUTE_METERS);
  expect(route.durationSeconds).toBeCloseTo(TWO_SEGMENT_ROUTE_SECONDS, 9);
  const steps = route.legs[0].steps;
  expect(steps).toHaveLength(2);
  expect(
    steps.map((step) => step.maneuverType),
  ).toEqual(['departure', 'arrival']);
  expect(
    steps.map((step) => step.instructionDisplayText),
  ).toEqual(['Departure', 'Arrival']);
  expect(
    steps.map((step) => step.instructionSpeechText),
  ).toEqual(['Departure', 'Arrival']);
});

test('a disconnected pair is refused honestly instead of guessed', async ({
  page,
}) => {
  // 1006 → 1001: the oneway street only carries 1004 → 1006, so no drivable
  // path exists between the snapped nodes — the provider refuses typed
  // (surfacing here as the harness's plain verbatim error string).
  const refusal_message = await route_refusal_message(
    page,
    FIXTURE_NODE_1006,
    FIXTURE_NODE_1001,
  );
  expect(refusal_message).toContain(
    'no drivable path exists between the snapped nodes',
  );
});

test('archive bytes that fail validation are refused at the load', async ({
  page,
}) => {
  // The same corruption the rustic-maps-core registry test applies: flipping the
  // format version's first byte makes the archive unloadable, and a failed
  // load is refused typed instead of routing over a graph never loaded.
  const refusal_message = await corrupted_bytes_refusal_message(page);
  expect(refusal_message).toContain('offline graph invalid');
  expect(refusal_message).toContain('VersionUnsupported');
});

// --- The suite's helpers ---
//
// The browser-side bodies are self-contained functions: Playwright
// serializes them to the page, so the same-origin fixture fetch is written
// out inside each body (the URL rides the arguments).

/**
 * Builds the seam's route answer over the committed fixture archive.
 */
async function route_from_fixture_bytes(page, start, end) {
  return page.evaluate(
    async ([fixture_url, start, end]) => {
      const archive_bytes = await fetch_fixtured_archive_bytes(fixture_url);
      return window.mapTest.offlineRouteFromArchiveBytes(
        archive_bytes,
        start[0],
        start[1],
        end[0],
        end[1],
      );
      /**
       * Fetches the committed compiled archive fixture same-origin as
       * bytes (the harness's only network reach is this localhost origin).
       */
      async function fetch_fixtured_archive_bytes(fetch_url) {
        const fixture_response = await fetch(fetch_url);
        if (!fixture_response.ok) {
          throw new Error(
            `the offline graph fixture is not served: ${fixture_response.status}`,
          );
        }
        return new Uint8Array(await fixture_response.arrayBuffer());
      }
    },
    [OFFLINE_GRAPH_FIXTURE_URL, start, end],
  );
}

/**
 * Runs the seam and returns the refusal's message string (caught in-page,
 * so the harness's plain-string throw shape arrives verbatim), or null
 * when the route unexpectedly succeeded.
 */
async function route_refusal_message(page, start, end) {
  return page.evaluate(
    async ([fixture_url, start, end]) => {
      try {
        const archive_bytes = await fetch_fixtured_archive_bytes(fixture_url);
        await window.mapTest.offlineRouteFromArchiveBytes(
          archive_bytes,
          start[0],
          start[1],
          end[0],
          end[1],
        );
        return null;
      } catch (refusal) {
        return String(refusal);
      }
      async function fetch_fixtured_archive_bytes(fetch_url) {
        const fixture_response = await fetch(fetch_url);
        if (!fixture_response.ok) {
          throw new Error(
            `the offline graph fixture is not served: ${fixture_response.status}`,
          );
        }
        return new Uint8Array(await fixture_response.arrayBuffer());
      }
    },
    [OFFLINE_GRAPH_FIXTURE_URL, start, end],
  );
}

/**
 * Flips the format version's first byte of a fetched copy (the corrupt
 * reload the rustic-maps-core tests apply) and reports the load's refusal — the
 * seam's typed load validation surfaced verbatim.
 */
async function corrupted_bytes_refusal_message(page) {
  return page.evaluate(async (fixture_url) => {
    try {
      const corrupted_bytes = await fetch_fixtured_archive_bytes(fixture_url);
      // The archive's wire layout: 4 magic bytes, then the little-endian
      // format version — byte 4 is the version's low byte.
      corrupted_bytes[4] = 0x2a;
      await window.mapTest.offlineRouteFromArchiveBytes(
        corrupted_bytes,
        0.001,
        0.0,
        0.001,
        0.002,
      );
      return null;
    } catch (refusal) {
      return String(refusal);
    }
    async function fetch_fixtured_archive_bytes(fetch_url) {
      const fixture_response = await fetch(fetch_url);
      if (!fixture_response.ok) {
        throw new Error(
          `the offline graph fixture is not served: ${fixture_response.status}`,
        );
      }
      return new Uint8Array(await fixture_response.arrayBuffer());
    }
  }, OFFLINE_GRAPH_FIXTURE_URL);
}