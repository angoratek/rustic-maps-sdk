/**
 * The pure decisions of the reference application's provider-credential
 * settings surface (docs/Providers.md (Credentials through the coverage matrix)):
 * descriptor validation, the per-provider apply/remember state machine, and
 * the clear semantics. The React screen owns the effectful half (the bridge
 * call, the secure store and the event channel); this module stays free of
 * React and native imports so jest can pin every decision without a native
 * side.
 *
 * Security rules encoded here: a credential is only ever persisted when the
 * "Remember on this device" toggle is on (the default is off), and clearing a
 * row always drops the entered value and the opt-in together.
 */

import type { ProviderCredentialDescriptor } from './providerRegistry';

/** The row's status: keyless (the honest first-class state), applied, refused. */
export type ProviderCredentialStatusKind = 'keyless' | 'applied' | 'refused';

/** One provider row's consumer-visible state. */
export interface ProviderCredentialRowState {
  providerKey: string;
  /** The masked field's current value (never rendered back in full). */
  enteredCredential: string;
  /** The opt-in toggle's value; off by default (§2's recorded decision). */
  isRememberEnabled: boolean;
  statusKind: ProviderCredentialStatusKind;
  /** The verbatim refusal detail (non-empty only while `refused`). */
  refusalDetailText: string;
  /** An honest secure-store failure message (empty when persistence worked). */
  storageFailureText: string;
}

export interface ProviderCredentialsState {
  rows: Record<string, ProviderCredentialRowState>;
}

/** One state transition of the settings surface. */
export type ProviderCredentialAction =
  | { type: 'credentialEdited'; providerKey: string; credential: string }
  | { type: 'rememberToggled'; providerKey: string; isRememberEnabled: boolean }
  | { type: 'applySucceeded'; providerKey: string }
  | { type: 'applyRefused'; providerKey: string; refusalDetailText: string }
  | { type: 'storageFailed'; providerKey: string; failureText: string }
  | { type: 'cleared'; providerKey: string }
  | { type: 'storedCredentialRestored'; providerKey: string; credential: string };

/** One explicit apply: the trimmed value and whether it should be persisted. */
export interface ProviderCredentialApplyPlan {
  credential: string;
  shouldPersist: boolean;
}

/**
 * Whether a descriptor carries everything the settings row needs to render.
 * A malformed artifact entry is skipped rather than rendering an empty row.
 */
export function isProviderCredentialDescriptorValid(
  descriptor: ProviderCredentialDescriptor,
): boolean {
  return (
    typeof descriptor?.provider_key === 'string' &&
    descriptor.provider_key.trim() !== '' &&
    typeof descriptor.display_name === 'string' &&
    descriptor.display_name.trim() !== '' &&
    typeof descriptor.credential_label === 'string' &&
    descriptor.credential_label.trim() !== ''
  );
}

/** The artifact descriptors a settings row can render (invalid entries dropped). */
export function usableProviderCredentialDescriptors(
  descriptors: readonly ProviderCredentialDescriptor[],
): ProviderCredentialDescriptor[] {
  return descriptors.filter(isProviderCredentialDescriptorValid);
}

/**
 * Whether the SDK registers this provider on React Native today; a false row
 * renders disabled-with-reason instead of pretending to work (§3).
 */
export function isProviderCredentialRegistrationAvailable(
  descriptor: ProviderCredentialDescriptor,
): boolean {
  return descriptor.registration_available?.react_native === true;
}

/** One keyless, opt-out row (the state a descriptor starts in). */
export function createInitialProviderCredentialRowState(
  providerKey: string,
): ProviderCredentialRowState {
  return {
    providerKey,
    enteredCredential: '',
    isRememberEnabled: false,
    statusKind: 'keyless',
    refusalDetailText: '',
    storageFailureText: '',
  };
}

/** Builds the settings state for a descriptor list (one keyless row each). */
export function createInitialProviderCredentialsState(
  descriptors: readonly ProviderCredentialDescriptor[],
): ProviderCredentialsState {
  const rows: Record<string, ProviderCredentialRowState> = {};
  for (const descriptor of descriptors) {
    rows[descriptor.provider_key] = createInitialProviderCredentialRowState(
      descriptor.provider_key,
    );
  }
  return { rows };
}

/**
 * The row state machine. A missing row is left untouched; edits reset a stale
 * refusal (the new value is a new attempt) but never un-apply a credential the
 * SDK already registered — the registry has no unregister operation (§3).
 */
export function providerCredentialReducer(
  state: ProviderCredentialsState,
  action: ProviderCredentialAction,
): ProviderCredentialsState {
  const row = state.rows[action.providerKey];
  if (row === undefined) {
    return state;
  }
  let updatedRow: ProviderCredentialRowState;
  switch (action.type) {
    case 'credentialEdited':
      updatedRow = {
        ...row,
        enteredCredential: action.credential,
        refusalDetailText: row.statusKind === 'refused' ? '' : row.refusalDetailText,
        statusKind: row.statusKind === 'refused' ? 'keyless' : row.statusKind,
      };
      break;
    case 'rememberToggled':
      updatedRow = { ...row, isRememberEnabled: action.isRememberEnabled };
      break;
    case 'applySucceeded':
      updatedRow = {
        ...row,
        statusKind: 'applied',
        refusalDetailText: '',
        storageFailureText: '',
      };
      break;
    case 'applyRefused':
      updatedRow = {
        ...row,
        statusKind: 'refused',
        refusalDetailText: action.refusalDetailText,
        storageFailureText: '',
      };
      break;
    case 'storageFailed':
      updatedRow = { ...row, storageFailureText: action.failureText };
      break;
    case 'cleared':
      updatedRow = createInitialProviderCredentialRowState(action.providerKey);
      break;
    case 'storedCredentialRestored':
      // A stored value can only exist behind a previous opt-in, so restoring
      // it re-applies and turns the toggle on (§3's mount rule).
      updatedRow = {
        ...row,
        enteredCredential: action.credential,
        isRememberEnabled: true,
        statusKind: 'applied',
        refusalDetailText: '',
        storageFailureText: '',
      };
      break;
  }
  return { ...state, rows: { ...state.rows, [action.providerKey]: updatedRow } };
}

/** Whether Apply is meaningful: the trimmed field holds a value. */
export function canApplyProviderCredential(row: ProviderCredentialRowState): boolean {
  return row.enteredCredential.trim() !== '';
}

/** Whether the SDK accepted this provider's credential. */
export function isProviderCredentialApplied(row: ProviderCredentialRowState): boolean {
  return row.statusKind === 'applied';
}

/**
 * The explicit apply's plan: `null` refuses an empty (or whitespace-only)
 * field; otherwise the trimmed credential and the persistence decision that
 * follows the toggle. Persistence only ever happens on an explicit opt-in.
 */
export function planProviderCredentialApply(
  row: ProviderCredentialRowState,
): ProviderCredentialApplyPlan | null {
  const credential = row.enteredCredential.trim();
  if (credential === '') {
    return null;
  }
  return { credential, shouldPersist: row.isRememberEnabled };
}
