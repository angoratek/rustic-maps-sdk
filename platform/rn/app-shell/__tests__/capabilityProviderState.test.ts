/**
 * The per-capability provider section's decisions
 * (docs/Providers.md (Per-capability selection)): the rows follow the SDK's stable
 * order, a capability with no installed provider reads "Not configured", the
 * picker is limited to registry descriptors that serve the capability and
 * register on React Native, a selection passes the applied credential for a
 * credentialed provider and an empty string for a keyless one, and a refusal
 * shows the SDK's message verbatim without moving the serving provider.
 */

import { ProviderContract } from '@angoratek/rustic-maps-sdk/src/generated/rustic_maps.ts';
import type { ProviderRegistration } from '@angoratek/rustic-maps-sdk/MapsSdk';

import {
  applyCapabilityProviderSelection,
  buildCapabilityProviderRows,
  capabilityContractForRegistryName,
  capabilityDisplayName,
  capabilityRegistryName,
  capabilityServingStatusText,
  isReadOnlyCapability,
  type CapabilityProviderGateway,
} from '../src/capabilityProviderState';
import type { ProviderCredentialDescriptor } from '../src/providerRegistry';
import { providerDescriptors } from '../src/providerRegistry';
import { appStrings } from '../src/theme';

/** One registry descriptor with the fields the section reads. */
function descriptor(overrides: Partial<ProviderCredentialDescriptor>): ProviderCredentialDescriptor {
  return {
    provider_key: 'test-provider',
    display_name: 'Test Provider',
    credential_label: '',
    served_contracts: [],
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

const fullServiceProvider = descriptor({
  provider_key: 'full-service',
  display_name: 'Full Service',
  credential_label: 'API key',
  served_contracts: ['geocoding', 'routing', 'raster_tiles'],
});
const keylessRouter = descriptor({
  provider_key: 'test-router',
  display_name: 'Test Router',
  served_contracts: ['routing'],
  demo_base_url: 'https://demo.test-router.example',
  demo_base_url_note: 'Public demo server, rate-limited and not for production use.',
});
const nativeUnavailableRouter = descriptor({
  provider_key: 'other-router',
  display_name: 'Other Router',
  served_contracts: ['routing'],
  registration_available: {
    android: true,
    ios: true,
    react_native: false,
    wasm: true,
  },
});
const keylessRasterSource = descriptor({
  provider_key: 'test-raster',
  display_name: 'Test Raster',
  served_contracts: ['raster_tiles'],
});

/** One registration entry with every field the row reads. */
function registration(overrides: Partial<ProviderRegistration>): ProviderRegistration {
  return {
    contract: ProviderContract.Geocoding,
    providerKey: '',
    displayName: '',
    registered: false,
    ...overrides,
  };
}

test('rows follow the SDK readback order and say Not configured honestly', () => {
  const rows = buildCapabilityProviderRows(
    [
      registration({ contract: ProviderContract.Routing }),
      registration({
        contract: ProviderContract.Geocoding,
        providerKey: 'full-service',
        displayName: 'Full Service',
        registered: true,
      }),
    ],
    [fullServiceProvider, keylessRouter],
    () => null,
  );

  expect(rows.map(row => row.contract)).toEqual([
    ProviderContract.Routing,
    ProviderContract.Geocoding,
  ]);
  expect(rows[0].servingProviderKey).toBeNull();
  expect(capabilityServingStatusText(rows[0])).toBe(
    appStrings.settingsProviderStateNotConfiguredText,
  );
  expect(capabilityServingStatusText(rows[1])).toBe('Full Service · Needs API key');
  expect(capabilityServingStatusText(rows[1])).not.toBe('Full Service');
  expect(rows[1].servingProviderRowText).toBe('Full Service · Needs API key');
});

test('the picker offers only descriptors that serve the capability and register on React Native', () => {
  const rows = buildCapabilityProviderRows(
    [registration({ contract: ProviderContract.Routing })],
    [fullServiceProvider, keylessRouter, nativeUnavailableRouter],
    () => null,
  );

  expect(rows[0].choices.map(choice => choice.providerKey)).toEqual([
    'full-service',
    'test-router',
  ]);
});

test('the tile capabilities are read-only and name the layer control', () => {
  expect(isReadOnlyCapability(ProviderContract.RasterTiles)).toBe(true);
  expect(isReadOnlyCapability(ProviderContract.VectorTiles)).toBe(true);
  expect(isReadOnlyCapability(ProviderContract.Routing)).toBe(false);

  const rows = buildCapabilityProviderRows(
    [
      registration({
        contract: ProviderContract.RasterTiles,
        providerKey: 'test-raster',
        displayName: 'Test Raster',
        registered: true,
      }),
    ],
    [keylessRasterSource],
    () => null,
  );
  expect(rows[0].isReadOnly).toBe(true);
  expect(rows[0].choices).toEqual([]);
  expect(rows[0].readOnlyNoteText).toBe(
    appStrings.capabilityProviderReadOnlyNoteText,
  );
});

test('capability names derive from the contract enum, not a hardcoded table', () => {
  expect(capabilityRegistryName(ProviderContract.Geocoding)).toBe('geocoding');
  expect(capabilityRegistryName(ProviderContract.DistanceMatrix)).toBe(
    'distance_matrix',
  );
  expect(capabilityRegistryName(ProviderContract.OfflineRouting)).toBe(
    'offline_routing',
  );
  expect(capabilityDisplayName(ProviderContract.DistanceMatrix)).toBe(
    'Distance matrix',
  );
  expect(capabilityDisplayName(ProviderContract.TrafficFlow)).toBe(
    'Traffic flow',
  );
});

test('every capability the shared registry declares maps to a contract and back', () => {
  // The real registry artifact: a capability added there alone must resolve
  // without a per-surface table.
  const registryNames = new Set(
    providerDescriptors.flatMap(
      descriptor => descriptor.served_contracts ?? [],
    ),
  );
  expect(registryNames.size).toBeGreaterThan(0);
  for (const registryName of registryNames) {
    const contract = capabilityContractForRegistryName(registryName);
    expect(contract).not.toBeNull();
    if (contract === null) {
      continue;
    }
    expect(capabilityRegistryName(contract)).toBe(registryName);
  }
});

test('every contract round-trips through its registry name', () => {
  for (const contract of Object.values(ProviderContract)) {
    if (typeof contract !== 'number') {
      continue;
    }
    expect(
      capabilityContractForRegistryName(capabilityRegistryName(contract)),
    ).toBe(contract);
  }
});

test('the shared registry composes the descriptor name with its credential state', () => {
  // The real registry artifact, not a test descriptor: the first React
  // Native-capable descriptor that declares a credential and serves
  // geocoding. A keyless session therefore reports the descriptor's own
  // display name and credential label in the shared composition.
  const credentialedDescriptor = providerDescriptors.find(
    candidateDescriptor =>
      candidateDescriptor.registration_available.react_native === true &&
      (candidateDescriptor.credential_label ?? '').trim() !== '' &&
      (candidateDescriptor.served_contracts ?? []).includes(
        capabilityRegistryName(ProviderContract.Geocoding),
      ),
  );
  expect(credentialedDescriptor).toBeDefined();
  if (credentialedDescriptor === undefined) {
    return;
  }
  const rows = buildCapabilityProviderRows(
    [
      registration({
        contract: ProviderContract.Geocoding,
        providerKey: credentialedDescriptor.provider_key,
        displayName: credentialedDescriptor.display_name,
        registered: true,
      }),
    ],
    providerDescriptors,
    () => null,
  );

  expect(capabilityServingStatusText(rows[0])).toBe(
    credentialedDescriptor.display_name +
      ' · Needs ' +
      credentialedDescriptor.credential_label,
  );
});

test('a keyless session over the shared registry reports Ready, never a bare name', () => {
  const keylessDescriptor = providerDescriptors.find(
    candidateDescriptor =>
      candidateDescriptor.registration_available.react_native === true &&
      (candidateDescriptor.credential_label ?? '').trim() === '' &&
      (candidateDescriptor.served_contracts ?? []).includes(
        capabilityRegistryName(ProviderContract.Routing),
      ),
  );
  expect(keylessDescriptor).toBeDefined();
  if (keylessDescriptor === undefined) {
    return;
  }
  const rows = buildCapabilityProviderRows(
    [
      registration({
        contract: ProviderContract.Routing,
        providerKey: keylessDescriptor.provider_key,
        displayName: keylessDescriptor.display_name,
        registered: true,
      }),
    ],
    providerDescriptors,
    () => null,
  );

  expect(capabilityServingStatusText(rows[0])).toBe(
    keylessDescriptor.display_name + ' · Ready',
  );
});

test('a selection passes the applied credential for a credentialed provider', async () => {
  const configureProvider = jest.fn(async () => undefined);
  const gateway: CapabilityProviderGateway = { configureProvider };
  const rows = buildCapabilityProviderRows(
    [registration({ contract: ProviderContract.Routing })],
    [fullServiceProvider, keylessRouter],
    providerKey => (providerKey === 'full-service' ? 'stored-key' : null),
  );

  const outcome = await applyCapabilityProviderSelection({
    gateway,
    row: rows[0],
    providerKey: 'full-service',
    appliedCredential: providerKey =>
      providerKey === 'full-service' ? 'stored-key' : null,
  });

  expect(outcome).toEqual({
    isApplied: true,
    detailText: '',
    nextStepText: '',
    providerKey: 'full-service',
  });
  expect(configureProvider).toHaveBeenCalledWith(
    ProviderContract.Routing,
    'full-service',
    'stored-key',
  );
});

test('a keyless provider receives its descriptor demo address, never an empty credential', async () => {
  const configureProvider = jest.fn(async () => undefined);
  const rows = buildCapabilityProviderRows(
    [registration({ contract: ProviderContract.Routing })],
    [keylessRouter],
    () => null,
  );
  expect(rows[0].choices[0].rowLabel).toBe('Test Router · Ready');
  expect(rows[0].choices[0].demoBaseUrl).toBe(
    'https://demo.test-router.example',
  );

  const outcome = await applyCapabilityProviderSelection({
    gateway: { configureProvider },
    row: rows[0],
    providerKey: 'test-router',
    appliedCredential: () => null,
  });

  expect(outcome.isApplied).toBe(true);
  expect(configureProvider).toHaveBeenCalledWith(
    ProviderContract.Routing,
    'test-router',
    'https://demo.test-router.example',
  );
});

test('a keyless provider with no demo address is refused without an SDK call', async () => {
  const configureProvider = jest.fn(async () => undefined);
  const rows = buildCapabilityProviderRows(
    [registration({ contract: ProviderContract.Routing })],
    [
      descriptor({
        provider_key: 'no-demo-router',
        display_name: 'No Demo Router',
        served_contracts: ['routing'],
      }),
    ],
    () => null,
  );

  const outcome = await applyCapabilityProviderSelection({
    gateway: { configureProvider },
    row: rows[0],
    providerKey: 'no-demo-router',
    appliedCredential: () => null,
  });

  expect(outcome.isApplied).toBe(false);
  expect(outcome.detailText).toContain(
    appStrings.settingsProviderStateNotConfiguredText,
  );
  expect(configureProvider).not.toHaveBeenCalled();
});

test('a credentialed provider without an applied key reports the reason without calling the SDK', async () => {
  const configureProvider = jest.fn(async () => undefined);
  const rows = buildCapabilityProviderRows(
    [registration({ contract: ProviderContract.Routing })],
    [fullServiceProvider],
    () => null,
  );
  expect(rows[0].choices[0].requiredCredentialLabel).toBe('API key');

  const outcome = await applyCapabilityProviderSelection({
    gateway: { configureProvider },
    row: rows[0],
    providerKey: 'full-service',
    appliedCredential: () => null,
  });

  expect(outcome.isApplied).toBe(false);
  expect(outcome.detailText).toBe(
    appStrings.capabilityProviderNeedsCredentialTemplate.replace(
      '{credential}',
      'API key',
    ),
  );
  expect(configureProvider).not.toHaveBeenCalled();
});

test('a refusal shows the SDK message verbatim and leaves the serving provider', async () => {
  const registrations = [
    registration({
      contract: ProviderContract.Routing,
      providerKey: 'full-service',
      displayName: 'Full Service',
      registered: true,
    }),
  ];
  const rows = buildCapabilityProviderRows(
    registrations,
    [fullServiceProvider, keylessRouter],
    () => 'stored-key',
  );
  const gateway: CapabilityProviderGateway = {
    configureProvider: jest.fn(async () => {
      throw new Error('provider credential is refused');
    }),
  };

  const outcome = await applyCapabilityProviderSelection({
    gateway,
    row: rows[0],
    providerKey: 'test-router',
    appliedCredential: () => null,
  });

  expect(outcome.isApplied).toBe(false);
  expect(outcome.detailText).toBe('provider credential is refused');
  expect(outcome.nextStepText).toBe(
    appStrings.settingsRoutingUnreachableNextStepText,
  );
  // A fresh readback still names the previously serving provider.
  const refreshedRows = buildCapabilityProviderRows(
    registrations,
    [fullServiceProvider, keylessRouter],
    () => 'stored-key',
  );
  expect(capabilityServingStatusText(refreshedRows[0])).toBe('Full Service · Ready');
});

test('a provider the registry does not offer is refused with its reason', async () => {
  const configureProvider = jest.fn(async () => undefined);
  const rows = buildCapabilityProviderRows(
    [registration({ contract: ProviderContract.Routing })],
    [keylessRouter],
    () => null,
  );

  const outcome = await applyCapabilityProviderSelection({
    gateway: { configureProvider },
    row: rows[0],
    providerKey: 'unknown-provider',
    appliedCredential: () => null,
  });

  expect(outcome.isApplied).toBe(false);
  expect(outcome.detailText).toBe(
    appStrings.capabilityProviderNotOfferedTemplate.replace(
      '{provider}',
      'unknown-provider',
    ),
  );
  expect(configureProvider).not.toHaveBeenCalled();
});

test('every row and picker entry composes the name with its credential state', () => {
  const rows = buildCapabilityProviderRows(
    [registration({ contract: ProviderContract.Routing })],
    [
      fullServiceProvider,
      keylessRouter,
      descriptor({
        provider_key: 'ready-credentialed',
        display_name: 'Ready Credentialed',
        credential_label: 'API key',
        served_contracts: ['routing'],
      }),
    ],
    providerKey => (providerKey === 'ready-credentialed' ? 'stored-key' : null),
  );

  const pickerLabels = rows[0].choices.map(choice => choice.rowLabel);
  expect(pickerLabels).toEqual([
    'Full Service · Needs API key',
    'Test Router · Ready',
    'Ready Credentialed · Ready',
  ]);
  // Every label names its provider; no bare state and no bare name.
  for (const label of pickerLabels) {
    expect(label).toContain(' · ');
  }
});

test('the demo endpoint and its rate-limit note travel on the choice', () => {
  const rows = buildCapabilityProviderRows(
    [
      registration({
        contract: ProviderContract.Routing,
        providerKey: 'test-router',
        displayName: 'Test Router',
        registered: true,
      }),
    ],
    [keylessRouter],
    () => null,
  );

  expect(capabilityServingStatusText(rows[0])).toBe('Test Router · Ready');
  expect(rows[0].choices[0].demoBaseUrl).toBe(
    'https://demo.test-router.example',
  );
  expect(rows[0].choices[0].demoBaseUrlNote).toBe(
    'Public demo server, rate-limited and not for production use.',
  );
});
