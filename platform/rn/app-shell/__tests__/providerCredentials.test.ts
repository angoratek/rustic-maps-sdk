/**
 * Jest-level unit tests of the reference application's provider-credential
 * decisions (docs/ProviderCredentialsAndBrowserProviders.md §3–§6):
 * descriptor validation, the apply/remember state machine, the explicit
 * apply plan, and the clear semantics — pure decisions, no native side.
 */

import {
  canApplyProviderCredential,
  createInitialProviderCredentialsState,
  isProviderCredentialApplied,
  isProviderCredentialDescriptorValid,
  isProviderCredentialRegistrationAvailable,
  planProviderCredentialApply,
  providerCredentialReducer,
  usableProviderCredentialDescriptors,
  type ProviderCredentialAction,
  type ProviderCredentialsState,
} from '../src/providerCredentials';
import type { ProviderCredentialDescriptor } from '../src/providerRegistry';

const mapquestDescriptor: ProviderCredentialDescriptor = {
  provider_key: 'mapquest',
  display_name: 'MapQuest',
  credential_label: 'MapQuest API key',
  registration_available: {
    android: true,
    ios: true,
    react_native: true,
    wasm: false,
  },
  attribution_text: '© MapQuest · Map data © OpenStreetMap contributors (ODbL)',
  settings_hint: 'Create a key in the MapQuest developer console.',
};

const unsupportedDescriptor: ProviderCredentialDescriptor = {
  ...mapquestDescriptor,
  provider_key: 'future-provider',
  display_name: 'Future provider',
  credential_label: 'Future provider API key',
  registration_available: {
    android: false,
    ios: false,
    react_native: false,
    wasm: false,
  },
};

function reduceRow(
  state: ProviderCredentialsState,
  ...actions: ProviderCredentialAction[]
): ProviderCredentialsState {
  return actions.reduce(providerCredentialReducer, state);
}

test('descriptor validation keeps complete rows and drops malformed entries', () => {
  expect(isProviderCredentialDescriptorValid(mapquestDescriptor)).toBe(true);

  expect(
    isProviderCredentialDescriptorValid({ ...mapquestDescriptor, provider_key: '' }),
  ).toBe(false);
  expect(
    isProviderCredentialDescriptorValid({ ...mapquestDescriptor, provider_key: '   ' }),
  ).toBe(false);
  expect(
    isProviderCredentialDescriptorValid({ ...mapquestDescriptor, display_name: '' }),
  ).toBe(false);
  expect(
    isProviderCredentialDescriptorValid({ ...mapquestDescriptor, credential_label: ' ' }),
  ).toBe(false);

  const usable = usableProviderCredentialDescriptors([
    mapquestDescriptor,
    { ...mapquestDescriptor, provider_key: '', display_name: 'Broken' },
    unsupportedDescriptor,
  ]);
  expect(usable.map(descriptor => descriptor.provider_key)).toEqual([
    'mapquest',
    'future-provider',
  ]);
});

test('registration availability follows the descriptor react_native flag', () => {
  expect(isProviderCredentialRegistrationAvailable(mapquestDescriptor)).toBe(true);
  expect(isProviderCredentialRegistrationAvailable(unsupportedDescriptor)).toBe(false);
});

test('a descriptor starts keyless with the remember toggle off', () => {
  const state = createInitialProviderCredentialsState([
    mapquestDescriptor,
    unsupportedDescriptor,
  ]);
  const row = state.rows.mapquest;
  expect(row.statusKind).toBe('keyless');
  expect(row.enteredCredential).toBe('');
  expect(row.isRememberEnabled).toBe(false);
  expect(row.refusalDetailText).toBe('');
  expect(row.storageFailureText).toBe('');
  expect(canApplyProviderCredential(row)).toBe(false);
  expect(isProviderCredentialApplied(row)).toBe(false);
  expect(Object.keys(state.rows)).toHaveLength(2);
});

test('editing and applying move a row from keyless to applied', () => {
  const initial = createInitialProviderCredentialsState([mapquestDescriptor]);
  const edited = reduceRow(initial, {
    type: 'credentialEdited',
    providerKey: 'mapquest',
    credential: 'abc123',
  });
  expect(edited.rows.mapquest.enteredCredential).toBe('abc123');
  expect(canApplyProviderCredential(edited.rows.mapquest)).toBe(true);

  const applied = reduceRow(edited, {
    type: 'applySucceeded',
    providerKey: 'mapquest',
  });
  expect(applied.rows.mapquest.statusKind).toBe('applied');
  expect(isProviderCredentialApplied(applied.rows.mapquest)).toBe(true);
});

test('a verbatim refusal is kept until a new edit or a successful apply', () => {
  const initial = createInitialProviderCredentialsState([mapquestDescriptor]);
  const refused = reduceRow(
    initial,
    { type: 'credentialEdited', providerKey: 'mapquest', credential: 'bad-key' },
    {
      type: 'applyRefused',
      providerKey: 'mapquest',
      refusalDetailText: 'the provider rejected the key (401)',
    },
  );
  expect(refused.rows.mapquest.statusKind).toBe('refused');
  expect(refused.rows.mapquest.refusalDetailText).toBe(
    'the provider rejected the key (401)',
  );

  const edited = reduceRow(refused, {
    type: 'credentialEdited',
    providerKey: 'mapquest',
    credential: 'another-key',
  });
  expect(edited.rows.mapquest.statusKind).toBe('keyless');
  expect(edited.rows.mapquest.refusalDetailText).toBe('');
});

test('an applied credential is not un-applied by a later edit', () => {
  const initial = createInitialProviderCredentialsState([mapquestDescriptor]);
  const applied = reduceRow(
    initial,
    { type: 'credentialEdited', providerKey: 'mapquest', credential: 'first-key' },
    { type: 'applySucceeded', providerKey: 'mapquest' },
    { type: 'credentialEdited', providerKey: 'mapquest', credential: 'second-key' },
  );
  expect(applied.rows.mapquest.statusKind).toBe('applied');
  expect(applied.rows.mapquest.enteredCredential).toBe('second-key');
});

test('the apply plan trims the value and persists only on an opt-in', () => {
  const initial = createInitialProviderCredentialsState([mapquestDescriptor]);
  const emptyPlan = planProviderCredentialApply(initial.rows.mapquest);
  expect(emptyPlan).toBeNull();

  const whitespace = reduceRow(initial, {
    type: 'credentialEdited',
    providerKey: 'mapquest',
    credential: '   ',
  });
  expect(planProviderCredentialApply(whitespace.rows.mapquest)).toBeNull();

  const sessionOnly = reduceRow(initial, {
    type: 'credentialEdited',
    providerKey: 'mapquest',
    credential: '  session-key  ',
  });
  expect(planProviderCredentialApply(sessionOnly.rows.mapquest)).toEqual({
    credential: 'session-key',
    shouldPersist: false,
  });

  const remembered = reduceRow(
    sessionOnly,
    { type: 'rememberToggled', providerKey: 'mapquest', isRememberEnabled: true },
  );
  expect(planProviderCredentialApply(remembered.rows.mapquest)).toEqual({
    credential: 'session-key',
    shouldPersist: true,
  });
});

test('a restored stored credential re-applies with the opt-in on', () => {
  const initial = createInitialProviderCredentialsState([mapquestDescriptor]);
  const restored = reduceRow(initial, {
    type: 'storedCredentialRestored',
    providerKey: 'mapquest',
    credential: 'remembered-key',
  });
  expect(restored.rows.mapquest.statusKind).toBe('applied');
  expect(restored.rows.mapquest.isRememberEnabled).toBe(true);
  expect(restored.rows.mapquest.enteredCredential).toBe('remembered-key');
});

test('a secure-store failure is reported beside an applied credential', () => {
  const initial = createInitialProviderCredentialsState([mapquestDescriptor]);
  const applied = reduceRow(
    initial,
    { type: 'credentialEdited', providerKey: 'mapquest', credential: 'abc123' },
    {
      type: 'rememberToggled',
      providerKey: 'mapquest',
      isRememberEnabled: true,
    },
    { type: 'applySucceeded', providerKey: 'mapquest' },
    {
      type: 'storageFailed',
      providerKey: 'mapquest',
      failureText: 'the secure store is unavailable',
    },
  );
  expect(applied.rows.mapquest.statusKind).toBe('applied');
  expect(applied.rows.mapquest.storageFailureText).toBe(
    'the secure store is unavailable',
  );
});

test('clear resets the value, the opt-in, the status and every notice', () => {
  const initial = createInitialProviderCredentialsState([mapquestDescriptor]);
  const applied = reduceRow(
    initial,
    { type: 'credentialEdited', providerKey: 'mapquest', credential: 'abc123' },
    { type: 'rememberToggled', providerKey: 'mapquest', isRememberEnabled: true },
    { type: 'applySucceeded', providerKey: 'mapquest' },
    {
      type: 'storageFailed',
      providerKey: 'mapquest',
      failureText: 'the secure store is unavailable',
    },
    { type: 'cleared', providerKey: 'mapquest' },
  );
  expect(applied.rows.mapquest).toEqual(initial.rows.mapquest);
  expect(canApplyProviderCredential(applied.rows.mapquest)).toBe(false);
});

test('an unknown provider key leaves the state untouched', () => {
  const initial = createInitialProviderCredentialsState([mapquestDescriptor]);
  const unchanged = reduceRow(initial, {
    type: 'applySucceeded',
    providerKey: 'missing-provider',
  });
  expect(unchanged).toBe(initial);
});
