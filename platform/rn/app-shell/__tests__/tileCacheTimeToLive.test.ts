/**
 * `setTileCacheTimeToLive` input validation on the JavaScript side.
 *
 * The contract takes a `u64`; the iOS `UInt64(Double)` initializer traps on
 * NaN, negatives and out-of-range values, and the Android conversion wraps.
 * The proxy refuses every invalid value before it crosses the bridge and
 * refuses a value above the installed provider's declared terms ceiling.
 */

import { NativeModules } from 'react-native';

import {
  InvalidTileCacheTimeToLiveError,
  MapsSdk,
  TILE_CACHE_TIME_TO_LIVE_ERROR_CODE,
  TILE_CACHE_TIME_TO_LIVE_INVALID_MESSAGE,
} from '@angoratek/rustic-maps-sdk/MapsSdk';

jest.mock('react-native', () => ({
  NativeModules: {
    MapsSdkNative: {
      initialize: jest.fn(async () => undefined),
      setTileCacheTimeToLive: jest.fn(async () => undefined),
      tileCacheTerms: jest.fn(async () =>
        JSON.stringify({
          effectiveTimeToLiveSeconds: 3600,
          providerTermsCeilingSeconds: 2_592_000,
          defaultTimeToLiveSeconds: 2_592_000,
        }),
      ),
    },
  },
}));

interface FakeNativeModule {
  initialize: jest.Mock;
  setTileCacheTimeToLive: jest.Mock;
  tileCacheTerms: jest.Mock;
}

const nativeModule = (
  NativeModules as unknown as { MapsSdkNative: FakeNativeModule }
).MapsSdkNative;

/** The terms the fake native module answers. */
function answerTileCacheTerms(terms: {
  effectiveTimeToLiveSeconds: number;
  providerTermsCeilingSeconds?: number;
  defaultTimeToLiveSeconds: number;
}): void {
  nativeModule.tileCacheTerms.mockResolvedValue(JSON.stringify(terms));
}

beforeEach(() => {
  jest.clearAllMocks();
  answerTileCacheTerms({
    effectiveTimeToLiveSeconds: 3600,
    providerTermsCeilingSeconds: 2_592_000,
    defaultTimeToLiveSeconds: 2_592_000,
  });
});

test.each([
  ['NaN', Number.NaN],
  ['positive infinity', Number.POSITIVE_INFINITY],
  ['negative infinity', Number.NEGATIVE_INFINITY],
  ['zero', 0],
  ['negative', -60],
  ['fractional', 1.5],
  ['beyond the safe integer range', Number.MAX_SAFE_INTEGER + 1],
])('a %s time-to-live is refused before the bridge', async (_label, value) => {
  const sdk = await MapsSdk.create();

  await expect(sdk.setTileCacheTimeToLive(value)).rejects.toBeInstanceOf(
    InvalidTileCacheTimeToLiveError,
  );
  await expect(sdk.setTileCacheTimeToLive(value)).rejects.toMatchObject({
    code: TILE_CACHE_TIME_TO_LIVE_ERROR_CODE,
    message: TILE_CACHE_TIME_TO_LIVE_INVALID_MESSAGE,
  });
  expect(nativeModule.setTileCacheTimeToLive).not.toHaveBeenCalled();
  expect(nativeModule.tileCacheTerms).not.toHaveBeenCalled();
});

test('a value above the provider terms ceiling is refused with the ceiling', async () => {
  const sdk = await MapsSdk.create();

  await expect(sdk.setTileCacheTimeToLive(2_592_001)).rejects.toMatchObject({
    code: TILE_CACHE_TIME_TO_LIVE_ERROR_CODE,
    message:
      "the tile-cache time-to-live exceeds the provider's terms ceiling of 2592000 seconds",
  });
  expect(nativeModule.setTileCacheTimeToLive).not.toHaveBeenCalled();
});

test('a positive whole value within the ceiling reaches the native module', async () => {
  const sdk = await MapsSdk.create();

  await sdk.setTileCacheTimeToLive(86_400);

  expect(nativeModule.tileCacheTerms).toHaveBeenCalledTimes(1);
  expect(nativeModule.setTileCacheTimeToLive).toHaveBeenCalledTimes(1);
  expect(nativeModule.setTileCacheTimeToLive).toHaveBeenCalledWith(86_400);
});

test('a provider without a declared ceiling accepts a large whole value', async () => {
  answerTileCacheTerms({
    effectiveTimeToLiveSeconds: 3600,
    defaultTimeToLiveSeconds: 2_592_000,
  });
  const sdk = await MapsSdk.create();

  await sdk.setTileCacheTimeToLive(9_999_999);

  expect(nativeModule.setTileCacheTimeToLive).toHaveBeenCalledWith(9_999_999);
});

test('an installed-cache failure from the terms readback surfaces verbatim', async () => {
  nativeModule.tileCacheTerms.mockRejectedValue(
    new Error('TileCache is not configured'),
  );
  const sdk = await MapsSdk.create();

  await expect(sdk.setTileCacheTimeToLive(3600)).rejects.toThrow(
    'TileCache is not configured',
  );
  expect(nativeModule.setTileCacheTimeToLive).not.toHaveBeenCalled();
});
