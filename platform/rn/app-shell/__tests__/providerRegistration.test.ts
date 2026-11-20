/**
 * The shared provider-credential registration path: a caching-ceiling
 * descriptor goes through the native module's cache-aware member, every
 * other descriptor configures each served capability through the SDK, and a
 * missing seam answers an honest refusal instead of calling one.
 */

import { ProviderContract } from 'maps-sdk-rn/src/generated/rustic_maps.ts';

import {
  registerProviderCredential,
  type ProviderCredentialNativeRegistration,
  type ProviderCredentialSdkRegistration,
} from '../src/providerRegistration';
import type { ProviderCredentialDescriptor } from '../src/providerRegistry';

/** One registry descriptor with the fields the registration path reads. */
function descriptor(
  overrides: Partial<ProviderCredentialDescriptor>,
): ProviderCredentialDescriptor {
  return {
    provider_key: 'test-provider',
    display_name: 'Test Provider',
    credential_label: 'API key',
    served_contracts: ['geocoding'],
    registration_available: {
      android: true,
      ios: true,
      react_native: true,
      wasm: true,
    },
    attribution_text: '',
    settings_hint: '',
    ...overrides,
  };
}

test('a caching-ceiling descriptor registers through the native module', async () => {
  const setProviderCredential = jest.fn(async () => ({
    isApplied: true,
    detailText: '',
  }));
  const nativeModule: ProviderCredentialNativeRegistration = {
    setProviderCredential,
  };
  const cachingDescriptor = descriptor({
    provider_key: 'cached-provider',
    raster_tile_cache_ceiling_days: 30,
  });

  await expect(
    registerProviderCredential({
      descriptor: cachingDescriptor,
      credential: 'secret-key',
      nativeModule,
      sdk: null,
    }),
  ).resolves.toEqual({ isApplied: true, detailText: '' });
  expect(setProviderCredential).toHaveBeenCalledWith(
    'cached-provider',
    'secret-key',
  );
});

test('a caching-ceiling descriptor without a native module is refused honestly', async () => {
  await expect(
    registerProviderCredential({
      descriptor: descriptor({ raster_tile_cache_ceiling_days: 30 }),
      credential: 'secret-key',
      nativeModule: null,
      sdk: null,
    }),
  ).resolves.toEqual({
    isApplied: false,
    detailText: 'the native module is unavailable',
  });
});

test('a credential descriptor without a running SDK is refused honestly', async () => {
  await expect(
    registerProviderCredential({
      descriptor: descriptor({}),
      credential: 'secret-key',
      nativeModule: null,
      sdk: null,
    }),
  ).resolves.toEqual({
    isApplied: false,
    detailText: 'the SDK is not running',
  });
});

test('a descriptor serving no registrable capability is refused by name', async () => {
  const configureProvider = jest.fn(async () => undefined);
  const sdk: ProviderCredentialSdkRegistration = { configureProvider };

  await expect(
    registerProviderCredential({
      descriptor: descriptor({
        display_name: 'Future Provider',
        served_contracts: ['not_a_contract'],
      }),
      credential: 'secret-key',
      nativeModule: null,
      sdk,
    }),
  ).resolves.toEqual({
    isApplied: false,
    detailText:
      'Future Provider declares no capability this build can register.',
  });
  expect(configureProvider).not.toHaveBeenCalled();
});

test('a credential descriptor registers every served capability', async () => {
  const configureProvider = jest.fn(async () => undefined);
  const sdk: ProviderCredentialSdkRegistration = { configureProvider };

  await expect(
    registerProviderCredential({
      descriptor: descriptor({
        served_contracts: ['geocoding', 'routing'],
      }),
      credential: 'secret-key',
      nativeModule: null,
      sdk,
    }),
  ).resolves.toEqual({ isApplied: true, detailText: '' });
  expect(configureProvider).toHaveBeenNthCalledWith(
    1,
    ProviderContract.Geocoding,
    'test-provider',
    'secret-key',
  );
  expect(configureProvider).toHaveBeenNthCalledWith(
    2,
    ProviderContract.Routing,
    'test-provider',
    'secret-key',
  );
});

test('an SDK refusal keeps its message verbatim', async () => {
  const sdk: ProviderCredentialSdkRegistration = {
    configureProvider: async () => {
      throw new Error('the provider refused the credential');
    },
  };

  await expect(
    registerProviderCredential({
      descriptor: descriptor({}),
      credential: 'secret-key',
      nativeModule: null,
      sdk,
    }),
  ).resolves.toEqual({
    isApplied: false,
    detailText: 'Error: the provider refused the credential',
  });
});
