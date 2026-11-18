/**
 * The plan fields' type-ahead (docs/design/Autocomplete.md): the quiet timer,
 * the row cap, the one-in-flight-request rule and the provider the session's
 * credential state selects.
 *
 * The rule's numbers come from `themeFramingTokens`, which mirrors
 * `platform/reference-tokens.json`; nothing here hard-codes them. Providers
 * come from the shared registry descriptors and the SDK's registration
 * readback, so no provider name appears in the shell.
 */

import { useCallback, useEffect, useRef, useState } from 'react';

import { ProviderContract } from 'maps-sdk-rn/src/generated/rustic_maps.ts';
import type { ProviderRegistration } from 'maps-sdk-rn/MapsSdk';

import { capabilityRegistryName } from './capabilityProviderState';
import type { PlaceRow } from './RefappNative';
import type { ProviderCredentialDescriptor } from './providerRegistry';
import { appStrings, themeFramingTokens } from './theme';

/**
 * The registry's contract name for the capability that answers a type-ahead
 * query. `MapSdk::suggest` resolves through the registered place-search
 * provider, so the selection below names that capability.
 */
const PLACE_SEARCH_REGISTRY_CONTRACT = capabilityRegistryName(
  ProviderContract.PlaceSearch,
);

/**
 * One provider choice for the type-ahead: the provider the credential state
 * selects and the value that registers it.
 */
export interface AutocompleteProviderSelection {
  /** The provider's stable registry key. */
  providerKey: string;
  /**
   * The API key when the provider is credentialed, the declared demo
   * deployment address when it is keyless. Empty for a launch-injected
   * credential the SDK already registered.
   */
  credentialText: string;
  /** Whether the selected provider needs a credential. */
  isCredentialed: boolean;
  /** Whether the SDK readback already reports this provider serving place search. */
  isConfigured: boolean;
}

/** The inputs the provider choice reads. */
export interface SelectAutocompleteProviderOptions {
  /** The shared registry's descriptors (never a hard-coded provider). */
  descriptors: ProviderCredentialDescriptor[];
  /** The SDK's registration readback for the current session. */
  registrations: ProviderRegistration[];
  /** The credential the settings surface holds for a provider key, or null. */
  credentialForProvider: (providerKey: string) => string | null;
  /** Whether a launch-injected credential is applied to the session. */
  hasSessionProviderCredential: boolean;
}

function providerServesPlaceSearch(
  descriptor: ProviderCredentialDescriptor,
): boolean {
  return (
    descriptor.registration_available.react_native &&
    (descriptor.served_contracts ?? []).includes(PLACE_SEARCH_REGISTRY_CONTRACT)
  );
}

function registeredPlaceSearchProviderKeys(
  registrations: ProviderRegistration[],
): Set<string> {
  return new Set(
    registrations
      .filter(
        registration =>
          registration.contract === ProviderContract.PlaceSearch &&
          registration.registered,
      )
      .map(registration => registration.providerKey),
  );
}

/**
 * Selects the provider that answers type-ahead from the session's credential
 * state: the credentialed provider when the settings surface (or a launch
 * injection) configured a key, the keyless search provider otherwise.
 *
 * Answers null when the shared registry declares no usable provider; the
 * fields then ask no provider rather than guessing one.
 */
export function selectAutocompleteProvider(
  options: SelectAutocompleteProviderOptions,
): AutocompleteProviderSelection | null {
  const candidates = options.descriptors.filter(providerServesPlaceSearch);
  const registeredProviderKeys = registeredPlaceSearchProviderKeys(
    options.registrations,
  );
  const credentialedCandidates = candidates.filter(
    descriptor => (descriptor.credential_label ?? '').trim() !== '',
  );

  for (const descriptor of credentialedCandidates) {
    const credentialText =
      options.credentialForProvider(descriptor.provider_key)?.trim() ?? '';
    if (credentialText !== '') {
      return {
        providerKey: descriptor.provider_key,
        credentialText,
        isCredentialed: true,
        isConfigured: registeredProviderKeys.has(descriptor.provider_key),
      };
    }
  }

  // A credential injected at launch never travels back to JavaScript. The
  // registry's first credentialed provider is the one the credential-only
  // registration installs; the empty credential tells the gateway not to
  // reconfigure it (and not to override the launch registration with the
  // keyless provider while the readback is still loading).
  if (options.hasSessionProviderCredential) {
    const sessionProvider = credentialedCandidates[0];
    if (sessionProvider !== undefined) {
      return {
        providerKey: sessionProvider.provider_key,
        credentialText: '',
        isCredentialed: true,
        isConfigured: registeredProviderKeys.has(sessionProvider.provider_key),
      };
    }
  }

  const keylessProvider = candidates.find(
    descriptor =>
      (descriptor.credential_label ?? '').trim() === '' &&
      (descriptor.demo_base_url ?? '').trim() !== '',
  );
  if (keylessProvider === undefined) {
    return null;
  }
  return {
    providerKey: keylessProvider.provider_key,
    credentialText: (keylessProvider.demo_base_url ?? '').trim(),
    isCredentialed: false,
    isConfigured: registeredProviderKeys.has(keylessProvider.provider_key),
  };
}

/** The state one field's type-ahead is in. */
export type PlanAutocompleteStatus =
  | 'idle'
  | 'loading'
  | 'ready'
  | 'noMatches'
  | 'failed';

/** The SDK seam the type-ahead asks; the shell adapts the SDK to it. */
export interface PlanAutocompleteGateway {
  /**
   * Asks the selected provider for completion candidates. The abort signal is
   * cancelled when a newer query supersedes the request; an implementation
   * drops the answer when it fires.
   */
  suggest(
    queryText: string,
    providerSelection: AutocompleteProviderSelection,
    signal: AbortSignal,
  ): Promise<PlaceRow[]>;
}

/** Everything one plan field's type-ahead renders. */
export interface PlanAutocomplete {
  /** At most `autocompleteMaximumResults` rows; empty unless ready. */
  suggestions: PlaceRow[];
  status: PlanAutocompleteStatus;
  /** One inline sentence while a provider failure stands; empty otherwise. */
  errorText: string;
  /** Fills the field from one row and closes the list. */
  selectSuggestion: (placeRow: PlaceRow) => void;
}

/** The inputs one plan field feeds the type-ahead. */
export interface PlanAutocompleteOptions {
  /** The field's current text. */
  queryText: string;
  /**
   * The text the user last typed into this field, or null when the field has
   * never received typing. A text the user did not type (a search-result
   * assignment, the My location chip, a swap) never asks a provider.
   */
  userEditedQueryText: string | null;
  /** The credential state's provider choice, or null when none is usable. */
  providerSelection: AutocompleteProviderSelection | null;
  /** The SDK seam, or null when the session has no SDK. */
  gateway: PlanAutocompleteGateway | null;
  /** Receives the chosen row so the caller fills the field. */
  onSelectSuggestion: (placeRow: PlaceRow) => void;
}

/**
 * Drives one field's type-ahead: asks only once the trimmed query reaches
 * `autocompleteMinimumQueryCharacters`, waits
 * `autocompleteDebounceMilliseconds` of quiet time, caps the answer at
 * `autocompleteMaximumResults` and drops any answer a newer query superseded.
 */
export function usePlanAutocomplete(
  options: PlanAutocompleteOptions,
): PlanAutocomplete {
  const {
    queryText,
    userEditedQueryText,
    providerSelection,
    gateway,
    onSelectSuggestion,
  } = options;
  const [suggestions, setSuggestions] = useState<PlaceRow[]>([]);
  const [status, setStatus] = useState<PlanAutocompleteStatus>('idle');
  const [errorText, setErrorText] = useState('');
  const requestSequenceRef = useRef(0);
  const inFlightAbortRef = useRef<AbortController | null>(null);
  const suppressedQueryRef = useRef<string | null>(null);

  const cancelInFlightRequest = useCallback(() => {
    requestSequenceRef.current += 1;
    inFlightAbortRef.current?.abort();
    inFlightAbortRef.current = null;
  }, []);

  useEffect(() => {
    const trimmedQuery = queryText.trim();
    const isSuppressedText =
      suppressedQueryRef.current !== null &&
      trimmedQuery === suppressedQueryRef.current;
    if (!isSuppressedText) {
      suppressedQueryRef.current = null;
    }

    cancelInFlightRequest();
    setSuggestions([]);
    setErrorText('');
    setStatus('idle');

    const isUserEditedText = userEditedQueryText === queryText;
    const isBelowThreshold =
      trimmedQuery.length <
      themeFramingTokens.autocompleteMinimumQueryCharacters;
    if (
      isBelowThreshold ||
      isSuppressedText ||
      !isUserEditedText ||
      gateway === null ||
      providerSelection === null
    ) {
      return;
    }

    const requestSequence = requestSequenceRef.current;
    let debounceTimer: ReturnType<typeof setTimeout> | null = setTimeout(() => {
      debounceTimer = null;
      const abortController = new AbortController();
      inFlightAbortRef.current = abortController;
      setStatus('loading');
      // The SDK call starts on a microtask so the timer callback itself stays
      // inside the fake-timer window the tests drive.
      Promise.resolve()
        .then(() =>
          gateway.suggest(
            trimmedQuery,
            providerSelection,
            abortController.signal,
          ),
        )
        .then(rows => {
          if (
            requestSequenceRef.current !== requestSequence ||
            abortController.signal.aborted
          ) {
            return;
          }
          inFlightAbortRef.current = null;
          const cappedRows = rows.slice(
            0,
            themeFramingTokens.autocompleteMaximumResults,
          );
          setSuggestions(cappedRows);
          setStatus(cappedRows.length === 0 ? 'noMatches' : 'ready');
        })
        .catch(() => {
          if (
            requestSequenceRef.current !== requestSequence ||
            abortController.signal.aborted
          ) {
            return;
          }
          inFlightAbortRef.current = null;
          setSuggestions([]);
          setStatus('failed');
          setErrorText(appStrings.planAutocompleteFailedText);
        });
    }, themeFramingTokens.autocompleteDebounceMilliseconds);

    return () => {
      if (debounceTimer !== null) {
        clearTimeout(debounceTimer);
      }
      cancelInFlightRequest();
    };
  }, [
    cancelInFlightRequest,
    gateway,
    providerSelection,
    queryText,
    userEditedQueryText,
  ]);

  const selectSuggestion = useCallback(
    (placeRow: PlaceRow) => {
      // The field will hold this display line; it must not immediately ask
      // the provider for its own selection.
      suppressedQueryRef.current = placeRow.structuredAddressText;
      cancelInFlightRequest();
      setSuggestions([]);
      setErrorText('');
      setStatus('idle');
      onSelectSuggestion(placeRow);
    },
    [cancelInFlightRequest, onSelectSuggestion],
  );

  return {
    suggestions,
    status,
    errorText,
    selectSuggestion,
  };
}
