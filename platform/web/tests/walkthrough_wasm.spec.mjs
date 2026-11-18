// The tutorial's WebAssembly worked example (M25) runs end to end: search
// (the artifact's recorded demo places, because the demo is keyless) → route
// (the artifact's recorded demo route) → navigate (the wasm Navigator) →
// offline (the committed routing-graph archive), all through the shared
// identifiers the other three platforms use. The module is served beside the
// page and imported there, so the test exercises the real wasm surface.
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

/** The shared artifact's Capitol Park → Tower Bridge recorded route. */
const RECORDED_ROUTE_DISTANCE_METERS = 1400;
/** The committed offline graph's grid demo route. */
const OFFLINE_ROUTE_DISTANCE_METERS = 222;
/** The probe canvas the example owns (CSS pixels). */
const PROBE_CANVAS_EDGE_PIXELS = 320;

test('the WebAssembly worked example runs search, route, navigate and offline', async ({
  page,
}) => {
  await page.goto('/?advanced');
  await waitForMapTestReady(page);

  const result = await page.evaluate(async (canvas_edge) => {
    const canvas = document.createElement('canvas');
    canvas.width = canvas_edge;
    canvas.height = canvas_edge;
    canvas.style.position = 'fixed';
    canvas.style.left = '-10000px';
    document.body.appendChild(canvas);
    const referenceScenes = await (
      await fetch('/reference-scenes.json')
    ).json();
    const { runWalkthroughScenario } = await import('/walkthroughScenario.mjs');
    const walkthrough = await runWalkthroughScenario({
      mapTest: window.mapTest,
      canvas,
      referenceScenes,
      offlineGraphUrl: '/offline-graph/routing_grid.rmog',
    });
    canvas.remove();
    return {
      identifiers: {
        originSearchQuery: walkthrough.originSearchQuery,
        destinationSearchQuery: walkthrough.destinationSearchQuery,
        originPlace: walkthrough.originPlace.name,
        destinationPlace: walkthrough.destinationPlace.name,
      },
      hasMapSdk: typeof walkthrough.mapSdk === 'object' && walkthrough.mapSdk !== null,
      hasMapView: typeof walkthrough.mapView === 'object' && walkthrough.mapView !== null,
      hasMapCamera: typeof walkthrough.mapCamera === 'object' && walkthrough.mapCamera !== null,
      hasRouteOptions: walkthrough.routeOptions.transportationMode === 'pedestrian',
      plannedDistanceMeters: walkthrough.plannedRoute.distanceMeters,
      routeOverlayCoordinateCount: walkthrough.routeOverlay.coordinates.length,
      routeOverlayKind: walkthrough.routeOverlay.kind,
      navigationPhase: walkthrough.navigationSnapshot.phase,
      navigationSpeechText:
        walkthrough.navigationSnapshot.instruction.activeManeuver.speechText,
      offlineDistanceMeters: walkthrough.offlineRoute.distanceMeters,
    };
  }, PROBE_CANVAS_EDGE_PIXELS);

  expect(result.identifiers).toEqual({
    originSearchQuery: 'Capitol Park',
    destinationSearchQuery: 'Tower Bridge',
    originPlace: 'Capitol Park',
    destinationPlace: 'Tower Bridge',
  });
  expect(result.hasMapSdk).toBe(true);
  expect(result.hasMapView).toBe(true);
  expect(result.hasMapCamera).toBe(true);
  expect(result.hasRouteOptions).toBe(true);
  expect(result.plannedDistanceMeters).toBe(RECORDED_ROUTE_DISTANCE_METERS);
  expect(result.routeOverlayKind).toBe('routePolyline');
  expect(result.routeOverlayCoordinateCount).toBeGreaterThan(0);
  expect(result.navigationPhase).toBe('navigating');
  expect(result.navigationSpeechText.length).toBeGreaterThan(0);
  expect(result.offlineDistanceMeters).toBe(OFFLINE_ROUTE_DISTANCE_METERS);
});
