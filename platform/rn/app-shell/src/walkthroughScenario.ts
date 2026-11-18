/**
 * The shared worked example every platform's tutorial chapter runs: open the
 * map on the home city, resolve an origin and a destination by name, plan a
 * pedestrian route between them, draw that route over the surface, start
 * guidance on it, and plan the offline demo request over the committed
 * routing graph.
 *
 * The same scenario is the `search-and-routing` tutorial's source: the
 * `docs-snippet` markers below delimit the blocks the chapter generator
 * extracts, one block per chapter section. Keep every marked run a contiguous
 * group of statements at one indentation depth, so the extracted snippet reads
 * as an ordinary excerpt of this function.
 *
 * The demo request mirrors the other platforms' route options: pedestrian
 * routing, fastest strategy, metric display units and no avoided road
 * features. The generated `RouteOptions` carries the departure instant as an
 * optional `bigint`; leaving it `undefined` is the contract's "depart now"
 * (and the only spelling the generated TypeScript surface accepts).
 */

import {
  MapOverlayKind,
  Navigator,
  NavigationLocationUpdate,
  RouteStrategy,
  TransportationMode,
  UnitSystem,
  defaultDeviationPolicy,
  defaultStepAdvancePolicy,
  offlineRouteFromArchiveBytes,
} from 'maps-sdk-rn/src/generated/rustic_maps.ts';
import type { MapsSdk, Route, RouteOptions } from 'maps-sdk-rn/MapsSdk';
import type { MapOverlay, MapViewHandle } from 'maps-sdk-rn/MapView';
import type { GeographicCoordinate } from 'maps-sdk-rn/MapView';

/** The fixed-accuracy envelope the worked example's single fix reports. */
const WALKTHROUGH_FIX_HORIZONTAL_ACCURACY_METERS = 5.0;
/** The routing-grid fixture's demo request (its straight oneway street). */
const WALKTHROUGH_OFFLINE_START_LATITUDE_DEGREES = 0.001;
const WALKTHROUGH_OFFLINE_START_LONGITUDE_DEGREES = 0.0;
const WALKTHROUGH_OFFLINE_END_LATITUDE_DEGREES = 0.001;
const WALKTHROUGH_OFFLINE_END_LONGITUDE_DEGREES = 0.002;

/**
 * Runs the shared search-and-routing walkthrough against one SDK handle and
 * one map view. The camera is set first, both queries are resolved by name,
 * the route is planned between the two resolved coordinates and its geometry
 * is pushed as a single route-polyline overlay.
 *
 * A query that resolves to no place returns `null` before any routing call:
 * the walkthrough never invents a coordinate and never plans a partial route.
 *
 * @param mapSdk The package SDK handle the queries and the plan run through.
 * @param mapView The mounted map view the camera and overlay changes target.
 * @param originSearchQuery The free-form origin text to geocode.
 * @param destinationSearchQuery The free-form destination text to geocode.
 * @param offlineArchiveBytes The committed routing-grid archive, when the
 *   caller has it: the offline step plans the demo request over it. Omitted,
 *   the step is skipped rather than inventing a route.
 * @returns The planned route, or `null` when either query resolved to no place.
 */
export async function runWalkthrough(
  mapSdk: MapsSdk,
  mapView: MapViewHandle,
  originSearchQuery: string,
  destinationSearchQuery: string,
  offlineArchiveBytes?: ArrayBuffer,
): Promise<Route | null> {
  // docs-snippet: begin first-map/open-the-map
  const mapCamera = {
    latitudeDegrees: 38.5766,
    longitudeDegrees: -121.4944,
    zoomLevel: 12.0,
    bearingDegrees: 0.0,
    pitchDegrees: 0.0,
  };
  mapView.setCamera(mapCamera);
  // docs-snippet: end first-map/open-the-map

  // docs-snippet: begin search-and-routing/search-for-places
  const originPlace = (await mapSdk.forwardGeocode(originSearchQuery))[0] ?? null;
  const destinationPlace =
    (await mapSdk.forwardGeocode(destinationSearchQuery))[0] ?? null;
  // docs-snippet: end search-and-routing/search-for-places

  if (originPlace === null || destinationPlace === null) {
    return null;
  }

  // docs-snippet: begin search-and-routing/plan-the-route
  const routeOptions: RouteOptions = {
    transportationMode: TransportationMode.Pedestrian,
    routeStrategy: RouteStrategy.Fastest,
    unitSystem: UnitSystem.Metric,
    avoidOptions: {
      tollRoads: false,
      limitedAccessHighways: false,
      ferries: false,
      unpavedRoads: false,
    },
    departureTimestampSeconds: undefined,
  };
  const plannedRoute = await mapSdk.calculateRoute(
    [originPlace.coordinate, destinationPlace.coordinate],
    routeOptions,
  );
  // docs-snippet: end search-and-routing/plan-the-route

  // docs-snippet: begin search-and-routing/draw-the-route
  const routeOverlay: MapOverlay = {
    kind: MapOverlayKind.RoutePolyline,
    coordinates: plannedRoute.legs.flatMap(leg =>
      leg.steps.flatMap(step => step.shape),
    ),
  };
  await mapView.setOverlays([routeOverlay]);
  // docs-snippet: end search-and-routing/draw-the-route

  const navigator = new Navigator();
  navigator.startNavigation(
    plannedRoute,
    UnitSystem.Metric,
    defaultStepAdvancePolicy(),
    defaultDeviationPolicy(),
  );
  // The first fix is the route's own first vertex: the walker's first
  // reported position. Real consumers feed their location source here.
  const firstRouteCoordinate: GeographicCoordinate | undefined =
    plannedRoute.legs[0]?.steps[0]?.shape[0];
  if (firstRouteCoordinate === undefined) {
    return null;
  }
  const navigationFix: NavigationLocationUpdate = {
    coordinate: firstRouteCoordinate,
    horizontalAccuracyMeters: WALKTHROUGH_FIX_HORIZONTAL_ACCURACY_METERS,
  };
  navigator.updateLocation(navigationFix);
  const tripState = navigator.currentTripState();

  if (offlineArchiveBytes !== undefined) {
    // The committed routing-grid archive's straight oneway street, the same
    // request the tutorial and the browser worked example use.
    const offlineRoute = await offlineRouteFromArchiveBytes(
      offlineArchiveBytes,
      {
        latitude: WALKTHROUGH_OFFLINE_START_LATITUDE_DEGREES,
        longitude: WALKTHROUGH_OFFLINE_START_LONGITUDE_DEGREES,
      },
      {
        latitude: WALKTHROUGH_OFFLINE_END_LATITUDE_DEGREES,
        longitude: WALKTHROUGH_OFFLINE_END_LONGITUDE_DEGREES,
      },
    );
    if (offlineRoute.legs.length === 0) {
      return null;
    }
  }

  if (tripState === undefined) {
    return null;
  }
  return plannedRoute;
}
