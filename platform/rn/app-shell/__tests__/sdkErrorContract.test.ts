/**
 * The package's one typed failure contract.
 *
 * A native bridge rejection carries a stable code on both halves
 * (`MapsSdkNativeFailure`, `MapsSdkInvalidTimeToLive`); the JavaScript surface
 * must turn it into one `MapsSdkError` shape instead of leaving a bare bridge
 * `Error`, and the time-to-live refusal must share that base.
 */

jest.mock('react-native', () => ({
  NativeModules: {
    MapsSdkNative: {
      initialize: async () => undefined,
      getCompiledVersion: async () => {
        const refusal = new Error('provider not configured: refused');
        (refusal as Error & { code?: string }).code = 'MapsSdkNativeFailure';
        throw refusal;
      },
    },
  },
  DeviceEventEmitter: { addListener: jest.fn(() => ({ remove: jest.fn() })) },
  NativeEventEmitter: class {
    addListener() {
      return { remove: jest.fn() };
    }
  },
  Platform: { OS: 'ios' },
}));

import {
  InvalidTileCacheTimeToLiveError,
  MapsSdk,
  MapsSdkError,
  MapsSdkErrorCode,
} from 'maps-sdk-rn/MapsSdk';

test('a native rejection surfaces as one typed MapsSdkError with its own code', async () => {
  const sdk = await MapsSdk.create();

  await expect(sdk.getCompiledVersion()).rejects.toBeInstanceOf(MapsSdkError);
  await expect(sdk.getCompiledVersion()).rejects.toMatchObject({
    code: MapsSdkErrorCode.NativeFailure,
    message: 'provider not configured: refused',
  });
});

test('fromNative preserves a bridge code and wraps a bare error', () => {
  const bridgeRefusal = Object.assign(new Error('the SDK refused'), {
    code: 'MapsSdkNativeFailure',
  });
  const wrappedBridgeRefusal = MapsSdkError.fromNative(bridgeRefusal);
  expect(wrappedBridgeRefusal.code).toBe(MapsSdkErrorCode.NativeFailure);
  expect(wrappedBridgeRefusal.message).toBe('the SDK refused');

  const wrappedBareError = MapsSdkError.fromNative(new Error('plain'));
  expect(wrappedBareError.code).toBe(MapsSdkErrorCode.NativeFailure);
  expect(wrappedBareError.message).toBe('plain');

  const alreadyTyped = MapsSdkError.nativeUnavailable();
  expect(MapsSdkError.fromNative(alreadyTyped)).toBe(alreadyTyped);
});

test('the time-to-live refusal shares the typed error base and code', () => {
  const refusal = new InvalidTileCacheTimeToLiveError();

  expect(refusal).toBeInstanceOf(MapsSdkError);
  expect(refusal.code).toBe(MapsSdkErrorCode.InvalidTimeToLive);
});
