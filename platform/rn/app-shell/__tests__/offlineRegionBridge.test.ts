/**
 * The offline-region bridge on the package's typed JavaScript surface.
 *
 * The native halves call the generated `OfflineManager` and cross records as
 * camel-case JSON; progress crosses the package's region event channel as the
 * generated phase enum's zero-based ordinal. These tests pin the event-to-
 * callback mapping, the cancellation terminal state, the no-cache `TileCache`
 * refusal and the JSON round trips the screen depends on.
 */

import { NativeModules } from 'react-native';

import { MapsSdk, type RegionDefinition } from '@angoratek/rustic-maps-sdk/MapsSdk';

jest.mock('react-native', () => {
  const offlineRegionListeners = new Set<any>();
  const addListener = (eventName: string, handler: any) => {
    if (eventName === 'MapsSdkOfflineRegionEvent') {
      offlineRegionListeners.add(handler);
    }
    return {
      remove: () => {
        offlineRegionListeners.delete(handler);
      },
    };
  };
  return {
    __offlineRegionListeners: offlineRegionListeners,
    NativeModules: {
      MapsSdkNative: {
        initialize: jest.fn(async () => undefined),
        regionTileCount: jest.fn(),
        downloadRegion: jest.fn(),
        cancelRegionDownload: jest.fn(async () => undefined),
        regionRecords: jest.fn(),
        regionRecord: jest.fn(),
        deleteRegion: jest.fn(),
        addListener: jest.fn(),
        removeListeners: jest.fn(),
      },
    },
    DeviceEventEmitter: { addListener },
    NativeEventEmitter: class {
      addListener(eventName: string, handler: any) {
        return addListener(eventName, handler);
      }
    },
    Platform: { OS: 'android' },
  };
});

interface FakeNativeModule {
  initialize: jest.Mock;
  regionTileCount: jest.Mock;
  downloadRegion: jest.Mock;
  cancelRegionDownload: jest.Mock;
  regionRecords: jest.Mock;
  regionRecord: jest.Mock;
  deleteRegion: jest.Mock;
}

const nativeModule = (
  NativeModules as unknown as { MapsSdkNative: FakeNativeModule }
).MapsSdkNative;

const reactNativeMock = jest.requireMock('react-native') as {
  __offlineRegionListeners: Set<(payload: any) => void>;
};

/** Emits one native progress event on the package's region channel. */
function emitRegionProgress(payload: {
  regionId: string;
  totalTileCount: number;
  completedTileCount: number;
  networkFetchedTileCount: number;
  diskServedTileCount: number;
  phase: number;
}): void {
  reactNativeMock.__offlineRegionListeners.forEach(handler => handler(payload));
}

/** The demo region the reference shell downloads. */
const demoRegion: RegionDefinition = {
  regionId: 'downtown-sacramento',
  displayName: 'Downtown Sacramento',
  boundingBox: {
    southWest: { latitude: 38.566, longitude: -121.508 },
    northEast: { latitude: 38.582, longitude: -121.488 },
  },
  minimumZoomLevel: 12,
  maximumZoomLevel: 16,
  imageFormat: 0,
  tileResolution: 0,
  timeToLiveSeconds: 2_592_000,
};

/** One stored-region record in the bridge's camel-case JSON shape. */
const storedRegionRecord = {
  regionId: 'downtown-sacramento',
  displayName: 'Downtown Sacramento',
  boundingBox: demoRegion.boundingBox,
  minimumZoomLevel: 12,
  maximumZoomLevel: 16,
  imageFormat: 0,
  tileResolution: 0,
  effectiveTimeToLiveSeconds: 2_592_000,
  memberTileCount: 23,
  presentTileCount: 23,
  presentPayloadBytes: 48_000,
};

beforeEach(() => {
  jest.clearAllMocks();
  reactNativeMock.__offlineRegionListeners.clear();
  nativeModule.cancelRegionDownload.mockResolvedValue(undefined);
});

test('a progress event maps onto the typed callback and the final record', async () => {
  nativeModule.downloadRegion.mockImplementation(async () => {
    emitRegionProgress({
      regionId: 'downtown-sacramento',
      totalTileCount: 23,
      completedTileCount: 1,
      networkFetchedTileCount: 1,
      diskServedTileCount: 0,
      phase: 0,
    });
    emitRegionProgress({
      regionId: 'downtown-sacramento',
      totalTileCount: 23,
      completedTileCount: 23,
      networkFetchedTileCount: 23,
      diskServedTileCount: 0,
      phase: 1,
    });
    return JSON.stringify(storedRegionRecord);
  });
  const sdk = await MapsSdk.create();
  const progressReports: Array<{
    completedTileCount: number;
    phase: string;
  }> = [];

  const record = await sdk.downloadRegion(demoRegion, progress => {
    progressReports.push({
      completedTileCount: progress.completedTileCount,
      phase: progress.phase,
    });
  });

  expect(progressReports).toEqual([
    { completedTileCount: 1, phase: 'downloading' },
    { completedTileCount: 23, phase: 'completed' },
  ]);
  expect(record).toEqual(storedRegionRecord);
  expect(nativeModule.downloadRegion).toHaveBeenCalledTimes(1);
  expect(JSON.parse(nativeModule.downloadRegion.mock.calls[0][0])).toEqual({
    regionId: 'downtown-sacramento',
    displayName: 'Downtown Sacramento',
    boundingBox: demoRegion.boundingBox,
    minimumZoomLevel: 12,
    maximumZoomLevel: 16,
    imageFormat: 0,
    tileResolution: 0,
    timeToLiveSeconds: 2_592_000,
  });
  // The terminal report detaches the listener.
  expect(reactNativeMock.__offlineRegionListeners.size).toBe(0);
});

test('cancellation resolves with the partial record and a cancelled terminal phase', async () => {
  const sdk = await MapsSdk.create();
  nativeModule.downloadRegion.mockImplementation(async () => {
    await sdk.cancelRegionDownload('downtown-sacramento');
    emitRegionProgress({
      regionId: 'downtown-sacramento',
      totalTileCount: 23,
      completedTileCount: 7,
      networkFetchedTileCount: 7,
      diskServedTileCount: 0,
      phase: 2,
    });
    return JSON.stringify({
      ...storedRegionRecord,
      presentTileCount: 7,
      presentPayloadBytes: 14_000,
    });
  });
  const phases: string[] = [];

  const record = await sdk.downloadRegion(demoRegion, progress => {
    phases.push(progress.phase);
  });

  expect(nativeModule.cancelRegionDownload).toHaveBeenCalledWith(
    'downtown-sacramento',
  );
  expect(phases).toEqual(['cancelled']);
  expect(record.presentTileCount).toBe(7);
  expect(reactNativeMock.__offlineRegionListeners.size).toBe(0);
});

test('a keyless session surfaces the SDK TileCache refusal verbatim', async () => {
  nativeModule.downloadRegion.mockRejectedValue(
    new Error('ProviderNotConfigured: TileCache'),
  );
  const sdk = await MapsSdk.create();

  await expect(sdk.downloadRegion(demoRegion)).rejects.toThrow(
    'ProviderNotConfigured: TileCache',
  );
  expect(reactNativeMock.__offlineRegionListeners.size).toBe(0);
});

test('region listing, single read, deletion and sizing round-trip JSON', async () => {
  nativeModule.regionRecords.mockResolvedValue(
    JSON.stringify([storedRegionRecord]),
  );
  nativeModule.regionRecord.mockResolvedValue(
    JSON.stringify(storedRegionRecord),
  );
  nativeModule.deleteRegion.mockResolvedValue(
    JSON.stringify({ removedEntryCount: 23, removedPayloadBytes: 48_000 }),
  );
  nativeModule.regionTileCount.mockResolvedValue(23);
  const sdk = await MapsSdk.create();

  await expect(sdk.regionRecords()).resolves.toEqual([storedRegionRecord]);
  await expect(sdk.regionRecord('downtown-sacramento')).resolves.toEqual(
    storedRegionRecord,
  );
  await expect(sdk.deleteRegion('downtown-sacramento')).resolves.toEqual({
    removedEntryCount: 23,
    removedPayloadBytes: 48_000,
  });
  await expect(sdk.regionTileCount(demoRegion)).resolves.toBe(23);
  expect(JSON.parse(nativeModule.regionTileCount.mock.calls[0][0])).toEqual(
    expect.objectContaining({
      regionId: 'downtown-sacramento',
      minimumZoomLevel: 12,
      maximumZoomLevel: 16,
      timeToLiveSeconds: 2_592_000,
    }),
  );
});

test('an absent stored region reads back as null', async () => {
  nativeModule.regionRecord.mockResolvedValue(null);
  const sdk = await MapsSdk.create();

  await expect(sdk.regionRecord('unknown-region')).resolves.toBeNull();
});
