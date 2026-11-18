/**
 * The package's one missing-native policy: `create` still answers a handle so
 * a JavaScript-only environment can import the surface, but every operation
 * that needs the native SDK throws the same typed `MapsSdkError` with code
 * `MapsSdkNativeUnavailable` — never a silent no-op or an unbounded
 * per-method fallback value.
 */

jest.mock('react-native', () => ({
  NativeModules: {},
  DeviceEventEmitter: { addListener: jest.fn(() => ({ remove: jest.fn() })) },
  NativeEventEmitter: class {
    addListener() {
      return { remove: jest.fn() };
    }
  },
  Platform: { OS: 'ios' },
}));

import {
  MapsSdk,
  MapsSdkError,
  MapsSdkErrorCode,
} from 'maps-sdk-rn/MapsSdk';

test('create still answers a handle without the native half', async () => {
  const sdk = await MapsSdk.create();

  expect(MapsSdk.isAvailable()).toBe(false);
  await expect(sdk.getCompiledVersion()).rejects.toBeInstanceOf(MapsSdkError);
});

test('every native operation refuses with the one typed missing-native code', async () => {
  const sdk = await MapsSdk.create();

  const expectedRefusal = {
    code: MapsSdkErrorCode.NativeUnavailable,
  };
  await expect(sdk.getCompiledVersion()).rejects.toMatchObject(expectedRefusal);
  await expect(
    sdk.setProviderCredential('provider', 'credential'),
  ).rejects.toMatchObject(expectedRefusal);
  await expect(sdk.providerRegistrations()).rejects.toMatchObject(
    expectedRefusal,
  );
  await expect(sdk.diagnosticsLines()).rejects.toMatchObject(expectedRefusal);
});
