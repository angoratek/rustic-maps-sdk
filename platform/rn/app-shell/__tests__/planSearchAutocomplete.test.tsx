/**
 * The plan fields' type-ahead (docs/design/Autocomplete.md): the three-
 * character threshold, the quiet timer, the five-row cap, the cancellation of
 * a superseded request, selection filling the field, and the keyless/keyed
 * provider choice. The provider rows come from the shared registry fixture
 * (`platform/providers.json`); the SDK seam is a fake gateway, so no test
 * touches the network and no provider name is hard-coded here.
 */

import React, { useState } from 'react';
import { act, fireEvent, render, screen } from '@testing-library/react-native';

jest.mock('maps-sdk-rn', () => ({
  getSdkCompiledVersion: () => 'jest-stub-version',
}));

jest.mock('maps-sdk-rn/MapView', () => {
  const React = require('react');
  const { View } = require('react-native');
  const MockMapView = React.forwardRef(
    (props: Record<string, unknown>, ref: unknown) =>
      React.createElement(View, { ...props, ref }),
  );
  return { MapView: MockMapView, default: MockMapView };
});

jest.mock('maps-sdk-rn/MapsSdk', () => ({
  MapsSdk: { create: jest.fn(), isAvailable: () => false },
  mapsSdk: { create: jest.fn(), isAvailable: () => false },
}));

jest.mock('react-native-safe-area-context', () => {
  const React = require('react');
  return {
    SafeAreaProvider: (props: { children?: React.ReactNode }) =>
      React.createElement(React.Fragment, null, props.children ?? null),
    useSafeAreaInsets: () => ({ top: 0, bottom: 0, left: 0, right: 0 }),
  };
});

import { PlanSearchView } from '../App';
import {
  selectAutocompleteProvider,
  type AutocompleteProviderSelection,
  type PlanAutocompleteGateway,
} from '../src/planSearchAutocomplete';
import type { PlaceRow } from '../src/RefappNative';
import { providerDescriptors } from '../src/providerRegistry';
import { buildScreenStyles } from '../src/screenStyles';
import { themeColorsByThemeName, themeFramingTokens } from '../src/theme';
import {
  ProviderContract,
  RouteStrategy,
  TransportationMode,
} from 'maps-sdk-rn/src/generated/rustic_maps.ts';
import type { ProviderRegistration } from 'maps-sdk-rn/MapsSdk';

function suggestionRow(name: string): PlaceRow {
  return {
    structuredAddressText: name,
    latitudeDegrees: 38.58,
    longitudeDegrees: -121.49,
    qualityText: 'street',
  };
}

const styles = buildScreenStyles(themeColorsByThemeName.dark);

const enabledChoice = { isEnabled: true, reasonText: '' };
const disabledChoice = { isEnabled: false, reasonText: '' };

const directionsControl = {
  selection: {
    transportationMode: TransportationMode.Automobile,
    routeStrategy: RouteStrategy.Fastest,
  },
  car: enabledChoice,
  walk: enabledChoice,
  fastest: enabledChoice,
  shortest: disabledChoice,
  isStrategyRowVisible: true,
  unavailableReasonText: '',
};

/** The two fields with the shell's own controlled-text and assignment path. */
function PlanSearchHarness(props: {
  gateway: PlanAutocompleteGateway;
  providerSelection: AutocompleteProviderSelection | null;
}) {
  const [originSearchText, setOriginSearchText] = useState('');
  const [destinationSearchText, setDestinationSearchText] = useState('');
  const [assignedOrigin, setAssignedOrigin] = useState<PlaceRow | null>(null);
  const [assignedDestination, setAssignedDestination] =
    useState<PlaceRow | null>(null);
  return (
    <PlanSearchView
      originSearchText={originSearchText}
      destinationSearchText={destinationSearchText}
      searchRows={[]}
      searchRowsAreDemoPlaces={false}
      demoSearchNoMatchText={null}
      searchInFlight={false}
      assignedOriginText={assignedOrigin?.structuredAddressText ?? null}
      assignedDestinationText={
        assignedDestination?.structuredAddressText ?? null
      }
      planRouteReasonText=""
      canPlanRoute={false}
      directionsControl={directionsControl}
      styles={styles}
      myLocationDisabledReason={null}
      myLocationFilledFields={{ origin: false, destination: false }}
      autocompleteProviderSelection={props.providerSelection}
      autocompleteGateway={props.gateway}
      onOriginSearchTextChange={setOriginSearchText}
      onDestinationSearchTextChange={setDestinationSearchText}
      onSearch={() => undefined}
      onSelectAutocompleteSuggestion={(planSearchField, placeRow) => {
        if (planSearchField === 'origin') {
          setAssignedOrigin(placeRow);
          setOriginSearchText(placeRow.structuredAddressText);
        } else {
          setAssignedDestination(placeRow);
          setDestinationSearchText(placeRow.structuredAddressText);
        }
      }}
      onAssign={() => undefined}
      onPlanRoute={() => undefined}
      onSelectDirectionsTransportationMode={() => undefined}
      onSelectDirectionsRouteStrategy={() => undefined}
      onFieldFocused={() => undefined}
      onFillMyLocation={() => undefined}
      onSwapFields={() => undefined}
      onDone={() => undefined}
    />
  );
}

/** Types into the origin field and lets the quiet period elapse. */
async function typeOriginAndSettle(queryText: string): Promise<void> {
  await fireEvent.changeText(
    screen.getByTestId('origin-search-field'),
    queryText,
  );
  await act(async () => {
    jest.advanceTimersByTime(
      themeFramingTokens.autocompleteDebounceMilliseconds,
    );
  });
}

beforeEach(() => {
  jest.useFakeTimers();
});

afterEach(() => {
  jest.useRealTimers();
});

test('a query asks the provider only once the trimmed text reaches the minimum', async () => {
  const minimumCharacters =
    themeFramingTokens.autocompleteMinimumQueryCharacters;
  const belowThresholdQuery = 'x'.repeat(minimumCharacters - 1);
  const thresholdQuery = 'x'.repeat(minimumCharacters);
  const gateway: PlanAutocompleteGateway = {
    suggest: jest.fn(async () => [suggestionRow('Sacramento')]),
  };
  const providerSelection = selectAutocompleteProvider({
    descriptors: providerDescriptors,
    registrations: [],
    credentialForProvider: () => null,
    hasSessionProviderCredential: false,
  });
  await render(
    <PlanSearchHarness
      gateway={gateway}
      providerSelection={providerSelection}
    />,
  );

  await fireEvent.changeText(
    screen.getByTestId('origin-search-field'),
    ` ${belowThresholdQuery} `,
  );
  await act(async () => {
    jest.advanceTimersByTime(
      themeFramingTokens.autocompleteDebounceMilliseconds,
    );
  });
  expect(gateway.suggest).not.toHaveBeenCalled();
  expect(screen.queryByTestId('origin-autocomplete-list')).toBeNull();

  await fireEvent.changeText(
    screen.getByTestId('origin-search-field'),
    thresholdQuery,
  );
  await act(async () => {
    jest.advanceTimersByTime(
      themeFramingTokens.autocompleteDebounceMilliseconds - 1,
    );
  });
  expect(gateway.suggest).not.toHaveBeenCalled();

  await act(async () => {
    jest.advanceTimersByTime(1);
  });
  expect(gateway.suggest).toHaveBeenCalledTimes(1);
  expect(gateway.suggest).toHaveBeenCalledWith(
    thresholdQuery,
    providerSelection,
    expect.any(AbortSignal),
  );
  expect(screen.getByTestId('origin-autocomplete-list')).toBeTruthy();

  // Dropping back below the threshold clears the list without asking again.
  await typeOriginAndSettle(belowThresholdQuery);
  expect(screen.queryByTestId('origin-autocomplete-list')).toBeNull();
  expect(gateway.suggest).toHaveBeenCalledTimes(1);
});

test('both plan fields own their own type-ahead list', async () => {
  const gateway: PlanAutocompleteGateway = {
    suggest: jest.fn(async () => [suggestionRow('Sacramento')]),
  };
  const providerSelection = selectAutocompleteProvider({
    descriptors: providerDescriptors,
    registrations: [],
    credentialForProvider: () => null,
    hasSessionProviderCredential: false,
  });
  await render(
    <PlanSearchHarness
      gateway={gateway}
      providerSelection={providerSelection}
    />,
  );

  await fireEvent.changeText(
    screen.getByTestId('destination-search-field'),
    'sac',
  );
  await act(async () => {
    jest.advanceTimersByTime(
      themeFramingTokens.autocompleteDebounceMilliseconds,
    );
  });

  expect(gateway.suggest).toHaveBeenCalledWith(
    'sac',
    providerSelection,
    expect.any(AbortSignal),
  );
  expect(screen.getByTestId('destination-autocomplete-list')).toBeTruthy();
  expect(screen.queryByTestId('origin-autocomplete-list')).toBeNull();
});

test('the list shows at most the maximum rows even when the provider answers more', async () => {
  const maximumResults = themeFramingTokens.autocompleteMaximumResults;
  const gateway: PlanAutocompleteGateway = {
    suggest: jest.fn(async () =>
      Array.from({ length: maximumResults + 3 }, (_unused, index) =>
        suggestionRow(`Place ${index}`),
      ),
    ),
  };
  const providerSelection = selectAutocompleteProvider({
    descriptors: providerDescriptors,
    registrations: [],
    credentialForProvider: () => null,
    hasSessionProviderCredential: false,
  });
  await render(
    <PlanSearchHarness
      gateway={gateway}
      providerSelection={providerSelection}
    />,
  );

  await typeOriginAndSettle('place');

  expect(screen.getByTestId('origin-autocomplete-row-0')).toBeTruthy();
  expect(
    screen.getByTestId(`origin-autocomplete-row-${maximumResults - 1}`),
  ).toBeTruthy();
  expect(
    screen.queryByTestId(`origin-autocomplete-row-${maximumResults}`),
  ).toBeNull();
});

test('a superseded request is cancelled and its late answer is dropped', async () => {
  let resolveFirstRequest: (rows: PlaceRow[]) => void = () => undefined;
  const firstAnswer = new Promise<PlaceRow[]>(resolve => {
    resolveFirstRequest = resolve;
  });
  let resolveSecondRequest: (rows: PlaceRow[]) => void = () => undefined;
  const secondAnswer = new Promise<PlaceRow[]>(resolve => {
    resolveSecondRequest = resolve;
  });
  const firstSignalHolder: { signal: AbortSignal | null } = { signal: null };
  const gateway: PlanAutocompleteGateway = {
    suggest: jest
      .fn()
      .mockImplementationOnce(
        async (
          _queryText: string,
          _providerSelection: AutocompleteProviderSelection,
          signal: AbortSignal,
        ) => {
          firstSignalHolder.signal = signal;
          return firstAnswer;
        },
      )
      .mockImplementationOnce(async () => secondAnswer),
  };
  const providerSelection = selectAutocompleteProvider({
    descriptors: providerDescriptors,
    registrations: [],
    credentialForProvider: () => null,
    hasSessionProviderCredential: false,
  });
  await render(
    <PlanSearchHarness
      gateway={gateway}
      providerSelection={providerSelection}
    />,
  );

  await typeOriginAndSettle('abc');
  expect(gateway.suggest).toHaveBeenCalledTimes(1);
  expect(firstSignalHolder.signal?.aborted).toBe(false);

  // The newer query supersedes the first request before its answer arrives.
  await fireEvent.changeText(screen.getByTestId('origin-search-field'), 'abcd');
  expect(firstSignalHolder.signal?.aborted).toBe(true);

  await act(async () => {
    resolveFirstRequest([suggestionRow('Stale answer')]);
  });
  expect(screen.queryByTestId('origin-autocomplete-list')).toBeNull();

  await act(async () => {
    jest.advanceTimersByTime(
      themeFramingTokens.autocompleteDebounceMilliseconds,
    );
  });
  expect(gateway.suggest).toHaveBeenCalledTimes(2);
  await act(async () => {
    resolveSecondRequest([suggestionRow('Current answer')]);
  });
  expect(screen.getByTestId('origin-autocomplete-row-0')).toBeTruthy();
  expect(screen.getByText('Current answer')).toBeTruthy();
  expect(screen.queryByText('Stale answer')).toBeNull();
});

test('choosing a suggestion fills the field, closes the list and does not re-ask', async () => {
  const gateway: PlanAutocompleteGateway = {
    suggest: jest.fn(async () => [suggestionRow('Sacramento Capitol Park')]),
  };
  const providerSelection = selectAutocompleteProvider({
    descriptors: providerDescriptors,
    registrations: [],
    credentialForProvider: () => null,
    hasSessionProviderCredential: false,
  });
  await render(
    <PlanSearchHarness
      gateway={gateway}
      providerSelection={providerSelection}
    />,
  );

  await typeOriginAndSettle('sac');
  expect(screen.getByTestId('origin-autocomplete-list')).toBeTruthy();

  await fireEvent.press(screen.getByTestId('origin-autocomplete-row-0'));

  expect(screen.getByTestId('origin-search-field').props.value).toBe(
    'Sacramento Capitol Park',
  );
  expect(screen.queryByTestId('origin-autocomplete-list')).toBeNull();

  // The selected display line is the field's text, not a new query.
  await act(async () => {
    jest.advanceTimersByTime(
      themeFramingTokens.autocompleteDebounceMilliseconds * 2,
    );
  });
  expect(gateway.suggest).toHaveBeenCalledTimes(1);
});

test('a failed provider request keeps the text, shows one sentence and retries on the next keystroke', async () => {
  const gateway: PlanAutocompleteGateway = {
    suggest: jest
      .fn()
      .mockRejectedValueOnce(new Error('provider refused'))
      .mockResolvedValueOnce([suggestionRow('Sacramento')]),
  };
  const providerSelection = selectAutocompleteProvider({
    descriptors: providerDescriptors,
    registrations: [],
    credentialForProvider: () => null,
    hasSessionProviderCredential: false,
  });
  await render(
    <PlanSearchHarness
      gateway={gateway}
      providerSelection={providerSelection}
    />,
  );

  await typeOriginAndSettle('abc');

  expect(screen.getByTestId('origin-search-field').props.value).toBe('abc');
  expect(screen.getByTestId('origin-autocomplete-error')).toBeTruthy();
  expect(screen.queryByTestId('origin-autocomplete-list')).toBeNull();

  await typeOriginAndSettle('abcd');

  expect(screen.queryByTestId('origin-autocomplete-error')).toBeNull();
  expect(screen.getByTestId('origin-autocomplete-row-0')).toBeTruthy();
});

test('no matches renders one row rather than an empty list', async () => {
  const gateway: PlanAutocompleteGateway = {
    suggest: jest.fn(async () => []),
  };
  const providerSelection = selectAutocompleteProvider({
    descriptors: providerDescriptors,
    registrations: [],
    credentialForProvider: () => null,
    hasSessionProviderCredential: false,
  });
  await render(
    <PlanSearchHarness
      gateway={gateway}
      providerSelection={providerSelection}
    />,
  );

  await typeOriginAndSettle('zzz');

  expect(screen.getByTestId('origin-autocomplete-no-matches')).toBeTruthy();
  expect(screen.queryByTestId('origin-autocomplete-row-0')).toBeNull();
});

test('the credential state selects the keyless or the credentialed provider', async () => {
  const keylessDescriptor = providerDescriptors.find(
    descriptor =>
      (descriptor.served_contracts ?? []).includes('place_search') &&
      (descriptor.credential_label ?? '').trim() === '' &&
      (descriptor.demo_base_url ?? '').trim() !== '',
  );
  const credentialedDescriptor = providerDescriptors.find(
    descriptor =>
      (descriptor.served_contracts ?? []).includes('place_search') &&
      (descriptor.credential_label ?? '').trim() !== '',
  );
  expect(keylessDescriptor).toBeDefined();
  expect(credentialedDescriptor).toBeDefined();
  const credentialedProviderKey = credentialedDescriptor!.provider_key;

  const keylessSelection = selectAutocompleteProvider({
    descriptors: providerDescriptors,
    registrations: [],
    credentialForProvider: () => null,
    hasSessionProviderCredential: false,
  });
  expect(keylessSelection?.providerKey).toBe(keylessDescriptor!.provider_key);
  expect(keylessSelection?.isCredentialed).toBe(false);
  expect(keylessSelection?.credentialText).toBe(
    keylessDescriptor!.demo_base_url,
  );

  const keyedSelection = selectAutocompleteProvider({
    descriptors: providerDescriptors,
    registrations: [],
    credentialForProvider: providerKey =>
      providerKey === credentialedProviderKey ? 'entered-key' : null,
    hasSessionProviderCredential: true,
  });
  expect(keyedSelection?.providerKey).toBe(credentialedProviderKey);
  expect(keyedSelection?.isCredentialed).toBe(true);
  expect(keyedSelection?.credentialText).toBe('entered-key');

  // A launch-injected credential never travels to JavaScript; the readback
  // reports the credentialed provider already serving the session.
  const launchKeyedRegistrations: ProviderRegistration[] = [
    {
      contract: ProviderContract.PlaceSearch,
      providerKey: credentialedProviderKey,
      displayName: credentialedDescriptor!.display_name,
      registered: true,
    },
  ];
  const launchSelection = selectAutocompleteProvider({
    descriptors: providerDescriptors,
    registrations: launchKeyedRegistrations,
    credentialForProvider: () => null,
    hasSessionProviderCredential: true,
  });
  expect(launchSelection?.providerKey).toBe(credentialedProviderKey);
  expect(launchSelection?.isConfigured).toBe(true);

  // Before the readback arrives the session stays on the credentialed
  // provider, so the keyless provider never overrides the launch injection.
  const launchBeforeReadback = selectAutocompleteProvider({
    descriptors: providerDescriptors,
    registrations: [],
    credentialForProvider: () => null,
    hasSessionProviderCredential: true,
  });
  expect(launchBeforeReadback?.providerKey).toBe(credentialedProviderKey);
  expect(launchBeforeReadback?.credentialText).toBe('');
  expect(launchBeforeReadback?.isConfigured).toBe(false);

  // Both choices reach the field's SDK seam unchanged.
  const keyedGateway: PlanAutocompleteGateway = {
    suggest: jest.fn(async () => []),
  };
  const keylessGateway: PlanAutocompleteGateway = {
    suggest: jest.fn(async () => []),
  };
  const keyedRender = await render(
    <PlanSearchHarness
      gateway={keyedGateway}
      providerSelection={keyedSelection}
    />,
  );
  await typeOriginAndSettle('abc');
  expect(keyedGateway.suggest).toHaveBeenCalledWith(
    'abc',
    keyedSelection,
    expect.any(AbortSignal),
  );
  await keyedRender.unmount();

  await render(
    <PlanSearchHarness
      gateway={keylessGateway}
      providerSelection={keylessSelection}
    />,
  );
  await typeOriginAndSettle('abc');
  expect(keylessGateway.suggest).toHaveBeenCalledWith(
    'abc',
    keylessSelection,
    expect.any(AbortSignal),
  );
});
