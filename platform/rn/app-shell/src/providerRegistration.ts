/**
 * The runtime provider-credential registration path shared by the settings
 * surface's explicit Apply and the launch-time restore. A provider that
 * declares a raster-tile caching ceiling installs its raster slot through the
 * native module's cache-aware member; every other provider registers one
 * capability per call through the SDK catalog, so the descriptor's own key
 * and served contracts decide what is installed.
 *
 * The module holds the decision only: callers pass the native module and the
 * SDK handle they already hold. It reads no component state and no storage,
 * and the credential is handed through untouched; jest pins every outcome
 * with fake handles.
 */

import { ProviderContract } from 'maps-sdk-rn/src/generated/rustic_maps.ts';

import { capabilityContractForRegistryName } from './capabilityProviderState';
import type { ProviderCredentialDescriptor } from './providerRegistry';

/** One runtime provider registration's honest outcome (§3's explicit Apply). */
export interface ProviderCredentialRegistrationOutcome {
  /** Whether the SDK accepted the credential. */
  isApplied: boolean;
  /** The verbatim refusal detail (empty when the credential applied). */
  detailText: string;
}

/** The native module member that installs a provider's cache-aware credential. */
export interface ProviderCredentialNativeRegistration {
  /**
   * Registers one credential through the platform runtime's cache-installing
   * path. A refusal resolves with its own detail text rather than rejecting.
   */
  setProviderCredential(
    providerKey: string,
    credential: string,
  ): Promise<ProviderCredentialRegistrationOutcome>;
}

/** The SDK handle member that configures one capability's provider. */
export interface ProviderCredentialSdkRegistration {
  /**
   * Configures one capability's serving provider. A refusal rejects with the
   * SDK's own message.
   */
  configureProvider(
    contract: ProviderContract,
    providerKey: string,
    credential: string,
  ): Promise<void>;
}

/** One registration call's inputs. */
export interface ProviderCredentialRegistrationOptions {
  /** The registry descriptor whose credential is being registered. */
  descriptor: ProviderCredentialDescriptor;
  /** The credential to apply; never read from or written to storage here. */
  credential: string;
  /** The native module, or null when the native side did not load. */
  nativeModule: ProviderCredentialNativeRegistration | null;
  /** The running SDK handle, or null before the SDK is created. */
  sdk: ProviderCredentialSdkRegistration | null;
}

/**
 * Registers one provider credential.
 *
 * A descriptor that declares a raster-tile caching ceiling installs its
 * raster slot through the native module. Every other descriptor registers one
 * capability per served contract through the SDK. A missing native module, a
 * missing SDK, or a descriptor that serves no capability this build can
 * register answers an honest refusal instead of calling a seam.
 */
export async function registerProviderCredential(
  options: ProviderCredentialRegistrationOptions,
): Promise<ProviderCredentialRegistrationOutcome> {
  const { descriptor, credential, nativeModule, sdk } = options;
  if (descriptor.raster_tile_cache_ceiling_days !== undefined) {
    if (nativeModule === null) {
      return {
        isApplied: false,
        detailText: 'the native module is unavailable',
      };
    }
    return nativeModule.setProviderCredential(
      descriptor.provider_key,
      credential,
    );
  }
  if (sdk === null) {
    return { isApplied: false, detailText: 'the SDK is not running' };
  }
  const servedContracts = (descriptor.served_contracts ?? [])
    .map((registryName) => capabilityContractForRegistryName(registryName))
    .filter((contract): contract is ProviderContract => contract !== null);
  if (servedContracts.length === 0) {
    return {
      isApplied: false,
      detailText:
        `${descriptor.display_name} declares no capability this build can register.`,
    };
  }
  try {
    for (const contract of servedContracts) {
      await sdk.configureProvider(
        contract,
        descriptor.provider_key,
        credential,
      );
    }
    return { isApplied: true, detailText: '' };
  } catch (applyError) {
    return { isApplied: false, detailText: String(applyError) };
  }
}
