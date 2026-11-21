/**
 * The curated `MapsSdk` proxy's provider-surface parity: distance
 * matrix and traffic members reach the package's native module with the
 * contract's JSON representation and decode the typed answers. The native
 * module is a local fake here — the real Kotlin/Swift implementations are
 * covered by their own builds — so this pins the JavaScript half of the
 * rule that RN consumers reach the same capabilities as the other targets.
 *
 * The device-positioning members are pinned the same way: every call reaches
 * the native module, and its result maps onto the typed surface, including
 * an absent fix, an absent course and an absent source status.
 */

import { NativeModules } from 'react-native';

import type {
  DevicePositionFix,
  PositionSourceFailureReason,
  ProviderRegistration,
  RouteCapabilities,
  TransportationMode,
} from '@angoratek/rustic-maps-sdk/MapsSdk';
import { ProviderContract } from '@angoratek/rustic-maps-sdk/src/generated/rustic_maps.ts';

const mockNativeModuleCalls: { [methodName: string]: unknown[] } = {};

jest.mock('react-native', () => {
  const recordCall = (methodName: string, argumentsList: unknown[]) => {
    mockNativeModuleCalls[methodName] = argumentsList;
  };
  return {
    NativeModules: {
      MapsSdkNative: {
        initialize: async () => {
          recordCall('initialize', []);
        },
        defaultProviderKey: async () => 'test-provider',
        configureProvider: async (
          contract: number,
          providerKey: string,
          credential: string,
        ) => {
          recordCall('configureProvider', [contract, providerKey, credential]);
        },
        providerRegistrations: async () => {
          recordCall('providerRegistrations', []);
          return JSON.stringify([
            {
              contract: 0,
              providerKey: 'test-geocoder',
              displayName: 'Test Geocoder',
              registered: true,
            },
            {
              contract: 1,
              providerKey: '',
              displayName: '',
              registered: false,
            },
          ]);
        },
        routeCapabilities: async () => {
          recordCall('routeCapabilities', []);
          return JSON.stringify({
            supportedTransportationModes: [0, 2],
            supportsShortestRouteStrategy: true,
          });
        },
        calculateDistanceMatrix: async (
          originsJson: string,
          destinationsJson: string,
          transportationMode: number,
        ) => {
          recordCall('calculateDistanceMatrix', [
            originsJson,
            destinationsJson,
            transportationMode,
          ]);
          return JSON.stringify({
            entries: [
              {
                originIndex: 0,
                destinationIndex: 0,
                distanceMeters: 2600,
                durationSeconds: 320,
              },
            ],
          });
        },
        fetchTrafficIncidents: async (boundingBoxJson: string) => {
          recordCall('fetchTrafficIncidents', [boundingBoxJson]);
          return JSON.stringify([
            {
              identifier: '1396961518491253',
              incidentType: 2,
              severity: 1,
              shortDescriptionText: 'Closed',
              startCoordinate: { latitude: 39.85682, longitude: -105.03604 },
              endCoordinate: null,
            },
          ]);
        },
        fetchTrafficFlow: async (boundingBoxJson: string) => {
          recordCall('fetchTrafficFlow', [boundingBoxJson]);
          return JSON.stringify([
            {
              shapeCoordinates: [
                { latitude: 40.63701, longitude: -74.01926 },
                { latitude: 40.63723, longitude: -74.01964 },
              ],
              currentSpeedKilometersPerHour: null,
              freeFlowSpeedKilometersPerHour: null,
              congestionLevel: 0,
            },
          ]);
        },
        startDeviceLocation: async () => {
          recordCall('startDeviceLocation', []);
          return true;
        },
        stopDeviceLocation: async () => {
          recordCall('stopDeviceLocation', []);
          return true;
        },
        latestDeviceFix: async () => {
          recordCall('latestDeviceFix', []);
          return JSON.stringify({
            coordinate: { latitude: 38.57693, longitude: -121.49492 },
            horizontalAccuracyMeters: 8.5,
            courseHeadingDegrees: null,
            timestampSeconds: 1730000000,
          });
        },
        deviceLocationSmoothedCourseDegrees: async () => {
          recordCall('deviceLocationSmoothedCourseDegrees', []);
          return 87.5;
        },
        deviceLocationSourceStatus: async () => {
          recordCall('deviceLocationSourceStatus', []);
          return 'permissionDenied';
        },
      },
    },
  };
});

/** The fake native module's device-positioning members, as the proxy calls them. */
const nativeModule = (
  NativeModules as unknown as {
    MapsSdkNative: {
      configureProvider(
        contract: number,
        providerKey: string,
        credential: string,
      ): Promise<void>;
      startDeviceLocation(): Promise<boolean>;
      stopDeviceLocation(): Promise<boolean>;
      latestDeviceFix(): Promise<string | null>;
      deviceLocationSmoothedCourseDegrees(): Promise<number | null>;
      deviceLocationSourceStatus(): Promise<PositionSourceFailureReason | null>;
    };
  }
).MapsSdkNative;

afterEach(() => {
  jest.restoreAllMocks();
});

// Imported after the react-native mock is registered.
import { MapsSdk } from '@angoratek/rustic-maps-sdk/MapsSdk';

const ORIGIN = { latitude: 38.57693, longitude: -121.49492 };
const DESTINATION = { latitude: 38.57393, longitude: -121.46956 };
const DENVER_BOX = {
  southWest: { latitude: 39.7, longitude: -105.1 },
  northEast: { latitude: 39.9, longitude: -104.8 },
};

test('the distance matrix reaches the native module and decodes its records', async () => {
  const sdk = await MapsSdk.create();
  const distanceMatrix = await sdk.calculateDistanceMatrix(
    [ORIGIN],
    [DESTINATION],
    2 as TransportationMode,
  );

  expect(mockNativeModuleCalls.calculateDistanceMatrix).toEqual([
    JSON.stringify([ORIGIN]),
    JSON.stringify([DESTINATION]),
    2,
  ]);
  expect(distanceMatrix.entries).toEqual([
    {
      originIndex: 0,
      destinationIndex: 0,
      distanceMeters: 2600,
      durationSeconds: 320,
    },
  ]);
});

test('traffic incidents and flow reach the native module and decode their records', async () => {
  const sdk = await MapsSdk.create();
  const incidents = await sdk.fetchTrafficIncidents(DENVER_BOX);
  expect(mockNativeModuleCalls.fetchTrafficIncidents).toEqual([
    JSON.stringify(DENVER_BOX),
  ]);
  expect(incidents).toEqual([
    {
      identifier: '1396961518491253',
      incidentType: 2,
      severity: 1,
      shortDescriptionText: 'Closed',
      startCoordinate: { latitude: 39.85682, longitude: -105.03604 },
      endCoordinate: null,
    },
  ]);

  const flowObservations = await sdk.fetchTrafficFlow(DENVER_BOX);
  expect(mockNativeModuleCalls.fetchTrafficFlow).toEqual([
    JSON.stringify(DENVER_BOX),
  ]);
  expect(flowObservations).toEqual([
    {
      shapeCoordinates: [
        { latitude: 40.63701, longitude: -74.01926 },
        { latitude: 40.63723, longitude: -74.01964 },
      ],
      currentSpeedKilometersPerHour: null,
      freeFlowSpeedKilometersPerHour: null,
      congestionLevel: 0,
    },
  ]);
});

test('the device-location start and stop calls reach the native module', async () => {
  const sdk = await MapsSdk.create();

  await expect(sdk.startDeviceLocation()).resolves.toBe(true);
  expect(mockNativeModuleCalls.startDeviceLocation).toEqual([]);

  await expect(sdk.stopDeviceLocation()).resolves.toBe(true);
  expect(mockNativeModuleCalls.stopDeviceLocation).toEqual([]);
});

test('a source that cannot start answers false without rejecting', async () => {
  const sdk = await MapsSdk.create();
  const startSpy = jest
    .spyOn(nativeModule, 'startDeviceLocation')
    .mockResolvedValue(false);

  await expect(sdk.startDeviceLocation()).resolves.toBe(false);
  expect(startSpy).toHaveBeenCalledTimes(1);
});

test('the latest fix decodes the contract record and an absent fix stays null', async () => {
  const sdk = await MapsSdk.create();
  const expectedFix: DevicePositionFix = {
    coordinate: { latitude: 38.57693, longitude: -121.49492 },
    horizontalAccuracyMeters: 8.5,
    courseHeadingDegrees: null,
    timestampSeconds: 1730000000,
  };

  await expect(sdk.latestDeviceFix()).resolves.toEqual(expectedFix);
  expect(mockNativeModuleCalls.latestDeviceFix).toEqual([]);

  jest.spyOn(nativeModule, 'latestDeviceFix').mockResolvedValue(null);
  await expect(sdk.latestDeviceFix()).resolves.toBeNull();
});

test('configureProvider reaches the native module with the contract and credential', async () => {
  const sdk = await MapsSdk.create();

  await sdk.configureProvider(
    ProviderContract.Routing,
    'test-router',
    'test-credential',
  );

  expect(mockNativeModuleCalls.configureProvider).toEqual([
    ProviderContract.Routing,
    'test-router',
    'test-credential',
  ]);
});

test('provider registrations and route capabilities decode their typed records', async () => {
  const sdk = await MapsSdk.create();

  const registrations: ProviderRegistration[] =
    await sdk.providerRegistrations();
  expect(mockNativeModuleCalls.providerRegistrations).toEqual([]);
  expect(registrations).toEqual([
    {
      contract: ProviderContract.Geocoding,
      providerKey: 'test-geocoder',
      displayName: 'Test Geocoder',
      registered: true,
    },
    {
      contract: ProviderContract.Routing,
      providerKey: '',
      displayName: '',
      registered: false,
    },
  ]);

  const capabilities: RouteCapabilities = await sdk.routeCapabilities();
  expect(mockNativeModuleCalls.routeCapabilities).toEqual([]);
  expect(capabilities).toEqual({
    supportedTransportationModes: [0, 2],
    supportsShortestRouteStrategy: true,
  });
});

test('a refused configureProvider surfaces the native module rejection verbatim', async () => {
  const sdk = await MapsSdk.create();
  jest
    .spyOn(nativeModule, 'configureProvider')
    .mockRejectedValueOnce(new Error('provider not configured: refused'));

  await expect(
    sdk.configureProvider(ProviderContract.Routing, 'unknown', ''),
  ).rejects.toThrow('provider not configured: refused');
});

test('the smoothed course and source status map onto the typed surface', async () => {
  const sdk = await MapsSdk.create();
  const expectedStatus: PositionSourceFailureReason = 'permissionDenied';

  await expect(sdk.deviceLocationSmoothedCourseDegrees()).resolves.toBe(87.5);
  expect(
    mockNativeModuleCalls.deviceLocationSmoothedCourseDegrees,
  ).toEqual([]);
  await expect(sdk.deviceLocationSourceStatus()).resolves.toBe(expectedStatus);
  expect(mockNativeModuleCalls.deviceLocationSourceStatus).toEqual([]);

  jest
    .spyOn(nativeModule, 'deviceLocationSmoothedCourseDegrees')
    .mockResolvedValue(null);
  jest.spyOn(nativeModule, 'deviceLocationSourceStatus').mockResolvedValue(null);
  await expect(sdk.deviceLocationSmoothedCourseDegrees()).resolves.toBeNull();
  await expect(sdk.deviceLocationSourceStatus()).resolves.toBeNull();
});
