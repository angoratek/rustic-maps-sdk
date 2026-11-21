/**
 * Jest-level unit tests of the shared search-and-routing walkthrough: the
 * camera is set before anything else, both queries are resolved through the
 * package SDK, the plan receives the two resolved coordinates in order, and
 * the planned geometry reaches the map view as one route-polyline overlay. A
 * query with no candidate stops the walkthrough before it plans anything.
 *
 * The SDK and the map view are recording stand-ins (the same seam shape the
 * package exposes); the generated enum module is mocked so the test does not
 * load the native bindings.
 */

import { runWalkthrough } from '../src/walkthroughScenario';
import type { MapsSdk, Route, RouteOptions } from '@angoratek/rustic-maps-sdk/MapsSdk';
import type {
  GeographicCoordinate,
  MapCamera,
  MapOverlay,
  MapViewHandle,
} from '@angoratek/rustic-maps-sdk/MapView';

/** What the generated `Navigator` stand-in observed. */
const mockNavigationRecording: {
  startCalls: Array<{
    route: Route;
    unitSystem: number;
    stepAdvancePolicy: unknown;
    deviationPolicy: unknown;
  }>;
  fixes: Array<{ coordinate: GeographicCoordinate }>;
  offlineRequests: Array<{
    archiveBytes: ArrayBuffer;
    start: GeographicCoordinate;
    end: GeographicCoordinate;
  }>;
} = { startCalls: [], fixes: [], offlineRequests: [] };

jest.mock('@angoratek/rustic-maps-sdk/src/generated/rustic_maps.ts', () => ({
  MapOverlayKind: {
    RoutePolyline: 0,
    FilledPolygon: 1,
    PointMarkers: 2,
    TrafficFlow: 3,
    PointPins: 4,
  },
  RouteStrategy: { Fastest: 0, Shortest: 1 },
  TransportationMode: { Automobile: 0, Bicycle: 1, Pedestrian: 2 },
  UnitSystem: { Metric: 0, Imperial: 1 },
  Navigator: class {
    startNavigation(
      route: Route,
      unitSystem: number,
      stepAdvancePolicy: unknown,
      deviationPolicy: unknown,
    ) {
      mockNavigationRecording.startCalls.push({
        route,
        unitSystem,
        stepAdvancePolicy,
        deviationPolicy,
      });
    }
    updateLocation(locationUpdate: { coordinate: GeographicCoordinate }) {
      mockNavigationRecording.fixes.push(locationUpdate);
    }
    currentTripState() {
      return { phase: 'navigating' };
    }
  },
  defaultStepAdvancePolicy: () => 'step-advance-policy',
  defaultDeviationPolicy: () => 'deviation-policy',
  offlineRouteFromArchiveBytes: async (
    archiveBytes: ArrayBuffer,
    start: GeographicCoordinate,
    end: GeographicCoordinate,
  ) => {
    mockNavigationRecording.offlineRequests.push({ archiveBytes, start, end });
    return {
      distanceMeters: 222,
      durationSeconds: 160,
      boundingBox: { southWest: start, northEast: end },
      legs: [
        { distanceMeters: 222, durationSeconds: 160, steps: [] },
      ],
    };
  },
}));

beforeEach(() => {
  mockNavigationRecording.startCalls.length = 0;
  mockNavigationRecording.fixes.length = 0;
  mockNavigationRecording.offlineRequests.length = 0;
});

const originSearchQuery = 'Capitol Park';
const destinationSearchQuery = 'Tower Bridge';

const originCoordinate: GeographicCoordinate = {
  latitude: 38.5766,
  longitude: -121.4944,
};
const destinationCoordinate: GeographicCoordinate = {
  latitude: 38.5816,
  longitude: -121.4946,
};

/** The subset of a resolved place the walkthrough reads. */
interface ResolvedPlace {
  structuredAddressText: string;
  coordinate: GeographicCoordinate;
  geocodeQuality: number;
}

const originPlace: ResolvedPlace = {
  structuredAddressText: 'Capitol Park, Sacramento, CA',
  coordinate: originCoordinate,
  geocodeQuality: 0,
};
const destinationPlace: ResolvedPlace = {
  structuredAddressText: 'Tower Bridge, Sacramento, CA',
  coordinate: destinationCoordinate,
  geocodeQuality: 0,
};

const plannedRoute: Route = {
  distanceMeters: 1200,
  durationSeconds: 900,
  geometry: [],
  boundingBox: {
    southWest: { latitude: 38.5766, longitude: -121.4946 },
    northEast: { latitude: 38.5816, longitude: -121.4944 },
  },
  legs: [
    {
      distanceMeters: 1200,
      durationSeconds: 900,
      steps: [
        {
          distanceMeters: 600,
          durationSeconds: 450,
          maneuverType: 'depart',
          instructionDisplayText: 'Head north',
          instructionSpeechText: 'Head north',
          shape: [
            originCoordinate,
            { latitude: 38.5791, longitude: -121.4945 },
          ],
        },
        {
          distanceMeters: 600,
          durationSeconds: 450,
          maneuverType: 'arrive',
          instructionDisplayText: 'Arrive at the destination',
          instructionSpeechText: 'Arrive at the destination',
          shape: [
            { latitude: 38.5791, longitude: -121.4945 },
            destinationCoordinate,
          ],
        },
      ],
    },
  ],
};

/** What the recording stand-ins observed on the SDK and map-view seams. */
interface WalkthroughRecording {
  callOrder: string[];
  camera: MapCamera | null;
  waypoints: ReadonlyArray<GeographicCoordinate> | null;
  routeOptions: RouteOptions | null;
  overlays: ReadonlyArray<MapOverlay> | null;
}

/** Builds the recording SDK and map view a walkthrough run drives. */
function createRecordingSeams(results: {
  originResults: ReadonlyArray<ResolvedPlace>;
  destinationResults: ReadonlyArray<ResolvedPlace>;
}): {
  mapSdk: MapsSdk;
  mapView: MapViewHandle;
  recording: WalkthroughRecording;
} {
  const recording: WalkthroughRecording = {
    callOrder: [],
    camera: null,
    waypoints: null,
    routeOptions: null,
    overlays: null,
  };

  const mapSdk = {
    forwardGeocode: async (addressText: string) => {
      recording.callOrder.push(`forwardGeocode:${addressText}`);
      return addressText === originSearchQuery
        ? [...results.originResults]
        : [...results.destinationResults];
    },
    calculateRoute: async (
      waypoints: ReadonlyArray<GeographicCoordinate>,
      routeOptions: RouteOptions,
    ) => {
      recording.callOrder.push('calculateRoute');
      recording.waypoints = waypoints;
      recording.routeOptions = routeOptions;
      return plannedRoute;
    },
  } as unknown as MapsSdk;

  const mapView = {
    setCamera: (camera: MapCamera) => {
      recording.callOrder.push('setCamera');
      recording.camera = camera;
    },
    setOverlays: async (overlays: ReadonlyArray<MapOverlay>) => {
      recording.callOrder.push('setOverlays');
      recording.overlays = overlays;
      return true;
    },
  } as unknown as MapViewHandle;

  return { mapSdk, mapView, recording };
}

test('sets the camera first, resolves both queries and plans between the two places', async () => {
  const { mapSdk, mapView, recording } = createRecordingSeams({
    originResults: [originPlace],
    destinationResults: [destinationPlace],
  });

  const result = await runWalkthrough(
    mapSdk,
    mapView,
    originSearchQuery,
    destinationSearchQuery,
  );

  expect(result).toBe(plannedRoute);
  expect(recording.callOrder).toEqual([
    'setCamera',
    `forwardGeocode:${originSearchQuery}`,
    `forwardGeocode:${destinationSearchQuery}`,
    'calculateRoute',
    'setOverlays',
  ]);
  expect(recording.camera).toEqual({
    latitudeDegrees: 38.5766,
    longitudeDegrees: -121.4944,
    zoomLevel: 12.0,
    bearingDegrees: 0.0,
    pitchDegrees: 0.0,
  });
  expect(recording.waypoints).toEqual([
    originPlace.coordinate,
    destinationPlace.coordinate,
  ]);
  expect(recording.routeOptions).toEqual({
    transportationMode: 2,
    routeStrategy: 0,
    unitSystem: 0,
    avoidOptions: {
      tollRoads: false,
      limitedAccessHighways: false,
      ferries: false,
      unpavedRoads: false,
    },
    departureTimestampSeconds: undefined,
  });
});

test('pushes one route-polyline overlay carrying every step shape', async () => {
  const { mapSdk, mapView, recording } = createRecordingSeams({
    originResults: [originPlace],
    destinationResults: [destinationPlace],
  });

  await runWalkthrough(
    mapSdk,
    mapView,
    originSearchQuery,
    destinationSearchQuery,
  );

  const expectedShape = plannedRoute.legs.flatMap(leg =>
    leg.steps.flatMap(step => step.shape),
  );
  expect(recording.overlays).toEqual([
    { kind: 0, coordinates: expectedShape },
  ]);
});

test('a query with no candidate returns null and never plans', async () => {
  for (const missingSide of ['origin', 'destination'] as const) {
    const { mapSdk, mapView, recording } = createRecordingSeams({
      originResults: missingSide === 'origin' ? [] : [originPlace],
      destinationResults:
        missingSide === 'destination' ? [] : [destinationPlace],
    });

    const result = await runWalkthrough(
      mapSdk,
      mapView,
      originSearchQuery,
      destinationSearchQuery,
    );

    expect(result).toBeNull();
    expect(recording.callOrder).toEqual([
      'setCamera',
      `forwardGeocode:${originSearchQuery}`,
      `forwardGeocode:${destinationSearchQuery}`,
    ]);
    expect(recording.waypoints).toBeNull();
    expect(recording.overlays).toBeNull();
  }
});

test('starts guidance on the planned route and feeds its first shape vertex', async () => {
  const { mapSdk, mapView } = createRecordingSeams({
    originResults: [originPlace],
    destinationResults: [destinationPlace],
  });

  await runWalkthrough(
    mapSdk,
    mapView,
    originSearchQuery,
    destinationSearchQuery,
  );

  expect(mockNavigationRecording.startCalls).toHaveLength(1);
  expect(mockNavigationRecording.startCalls[0].route).toBe(plannedRoute);
  expect(mockNavigationRecording.startCalls[0].unitSystem).toBe(0);
  expect(mockNavigationRecording.startCalls[0].stepAdvancePolicy).toBe(
    'step-advance-policy',
  );
  expect(mockNavigationRecording.startCalls[0].deviationPolicy).toBe(
    'deviation-policy',
  );
  expect(mockNavigationRecording.fixes).toEqual([
    {
      coordinate: plannedRoute.legs[0].steps[0].shape[0],
      horizontalAccuracyMeters: 5.0,
    },
  ]);
});

test('plans the offline demo request over the supplied archive bytes', async () => {
  const { mapSdk, mapView } = createRecordingSeams({
    originResults: [originPlace],
    destinationResults: [destinationPlace],
  });
  const offlineArchiveBytes = new ArrayBuffer(8);

  const result = await runWalkthrough(
    mapSdk,
    mapView,
    originSearchQuery,
    destinationSearchQuery,
    offlineArchiveBytes,
  );

  expect(result).toBe(plannedRoute);
  expect(mockNavigationRecording.offlineRequests).toEqual([
    {
      archiveBytes: offlineArchiveBytes,
      start: { latitude: 0.001, longitude: 0.0 },
      end: { latitude: 0.001, longitude: 0.002 },
    },
  ]);
});
