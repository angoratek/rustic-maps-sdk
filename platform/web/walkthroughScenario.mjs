// The tutorial's one worked example on the WebAssembly target (M25): search →
// route → navigate → offline with the same identifiers the Android, iOS and
// React Native scenarios use (mapSdk, mapView, mapCamera, originSearchQuery,
// destinationSearchQuery, originPlace, destinationPlace, routeOptions,
// plannedRoute, routeOverlay). The browser demo is keyless, so the "search"
// resolves the shared artifact's recorded demo places and the route is the
// artifact's recorded demo route for that pair — the same content the native
// apps show, never an invented result. A keyed integration registers a provider
// and calls `forwardGeocode`/`calculateRouteWithOptions` instead; the tabs in
// the book state that difference. Navigation and offline are real calls here,
// not illustrations.
//
// The module is importable from the served page (Playwright imports it) and is
// pure of page-specific state: the caller hands it the wasm surface, a canvas
// and the shared artifact.

/** The walkthrough's two demo queries (the shared artifact's place names). */
export const WALKTHROUGH_ORIGIN_QUERY = 'Capitol Park';
/** The walkthrough's destination query. */
export const WALKTHROUGH_DESTINATION_QUERY = 'Tower Bridge';
/** The camera the example frames the route with. */
const WALKTHROUGH_ZOOM_LEVEL = 14;
const WALKTHROUGH_FIELD_OF_VIEW_DEGREES = 60;
/** The keyless walkthrough's recorded routing-graph demo request. */
const OFFLINE_ROUTE_START_LATITUDE_DEGREES = 0.001;
const OFFLINE_ROUTE_START_LONGITUDE_DEGREES = 0.0;
const OFFLINE_ROUTE_END_LATITUDE_DEGREES = 0.001;
const OFFLINE_ROUTE_END_LONGITUDE_DEGREES = 0.002;

/** Resolves one recorded demo place by its artifact name. */
function find_demo_place(referenceScenes, place_name) {
  const demo_place = referenceScenes.demo_places.find(
    (candidate) => candidate.name === place_name,
  );
  if (demo_place === undefined) {
    throw new Error(`the artifact records no demo place named "${place_name}"`);
  }
  return demo_place;
}

/** Resolves the recorded route for one ordered demo-place pair. */
function find_demo_route(referenceScenes, origin_name, destination_name) {
  const demo_route = referenceScenes.demo_routes.find(
    (candidate) => candidate.origin_name === origin_name
      && candidate.destination_name === destination_name,
  );
  if (demo_route === undefined) {
    throw new Error(
      `the artifact records no route from "${origin_name}" to "${destination_name}"`,
    );
  }
  return demo_route;
}

/** Converts a recorded route to the Navigator's camel-case contract shape. */
function navigator_route(recorded_route) {
  const coordinate = (raw_coordinate) => ({
    latitude: raw_coordinate.latitude,
    longitude: raw_coordinate.longitude,
  });
  const route = recorded_route.route;
  return {
    distanceMeters: route.distance_meters,
    durationSeconds: route.duration_seconds,
    boundingBox: {
      southWest: coordinate(route.bounding_box.south_west),
      northEast: coordinate(route.bounding_box.north_east),
    },
    legs: route.legs.map((leg) => ({
      distanceMeters: leg.distance_meters,
      durationSeconds: leg.duration_seconds,
      steps: leg.steps.map((step) => ({
        distanceMeters: step.distance_meters,
        durationSeconds: step.duration_seconds,
        maneuverType: step.maneuver_type,
        instructionDisplayText: step.instruction_display_text,
        instructionSpeechText: step.instruction_speech_text,
        shape: step.shape.map(coordinate),
      })),
    })),
  };
}

/** The planned route as one traversal-order polyline. */
function route_polyline(route) {
  return route.legs.flatMap((leg) => leg.steps.flatMap((step) => step.shape));
}

/**
 * Runs the worked example end to end and answers every shared identifier plus
 * the navigation snapshot and the offline route, so a test can assert the
 * results instead of reading illustrations.
 */
export async function runWalkthroughScenario({
  mapTest,
  canvas,
  referenceScenes,
  offlineGraphUrl,
}) {
  const mapSdk = mapTest.maps;
  const mapView = new mapTest.MapView(canvas, mapSdk);
  await mapView.initialize();

  const originSearchQuery = WALKTHROUGH_ORIGIN_QUERY;
  const destinationSearchQuery = WALKTHROUGH_DESTINATION_QUERY;
  const originPlace = find_demo_place(referenceScenes, originSearchQuery);
  const destinationPlace = find_demo_place(referenceScenes, destinationSearchQuery);
  const mapCamera = new mapTest.MapCamera(
    (originPlace.latitude_degrees + destinationPlace.latitude_degrees) / 2,
    (originPlace.longitude_degrees + destinationPlace.longitude_degrees) / 2,
    WALKTHROUGH_ZOOM_LEVEL,
    0,
    0,
  );

  // The options a live plan would send; the keyless recorded route already
  // used the same pedestrian/fastest/metric profile.
  const routeOptions = {
    transportationMode: 'pedestrian',
    routeStrategy: 'fastest',
    unitSystem: 'metric',
    avoidOptions: {
      tollRoads: false,
      limitedAccessHighways: false,
      ferries: false,
      unpavedRoads: false,
    },
  };
  const plannedRoute = navigator_route(
    find_demo_route(referenceScenes, originPlace.name, destinationPlace.name),
  );
  const routeOverlay = {
    kind: 'routePolyline',
    coordinates: route_polyline(plannedRoute),
  };

  mapView.setBaseMapVisible(false);
  mapView.setBackgroundColor(0.09, 0.10, 0.12, 1);
  mapView.setOverlays([routeOverlay]);
  await mapView.renderFrame(mapCamera, WALKTHROUGH_FIELD_OF_VIEW_DEGREES);

  const navigator = new mapTest.Navigator();
  navigator.startNavigation(plannedRoute, 'metric');
  const navigationSnapshot = navigator.updateLocation({
    coordinate: plannedRoute.legs[0].steps[0].shape[0],
    horizontalAccuracyMeters: 2,
    courseHeadingDegrees: null,
    timestampSeconds: null,
  });

  const offlineGraphResponse = await fetch(offlineGraphUrl);
  if (!offlineGraphResponse.ok) {
    throw new Error(
      `the offline graph fixture is not served (${offlineGraphResponse.status})`,
    );
  }
  const offlineRoute = await mapTest.offlineRouteFromArchiveBytes(
    new Uint8Array(await offlineGraphResponse.arrayBuffer()),
    OFFLINE_ROUTE_START_LATITUDE_DEGREES,
    OFFLINE_ROUTE_START_LONGITUDE_DEGREES,
    OFFLINE_ROUTE_END_LATITUDE_DEGREES,
    OFFLINE_ROUTE_END_LONGITUDE_DEGREES,
  );

  return {
    mapSdk,
    mapView,
    mapCamera,
    originSearchQuery,
    destinationSearchQuery,
    originPlace,
    destinationPlace,
    routeOptions,
    plannedRoute,
    routeOverlay,
    navigationSnapshot,
    offlineRoute,
  };
}
