/**
 * The shared reference-application flows on React Native
 * (docs/ReferenceAppUX.md): the labeled plan fields with
 * their inline Search actions and per-field My location chips, the swap, the
 * typed-text resolution on Plan route, and the diagnostics pane's opt-in
 * subscription. The provider-row composition and the unreachable demo server
 * are pinned by `capabilityProviderState.test.ts`.
 */

import React from 'react';
import ReactTestRenderer from 'react-test-renderer';
import { DeviceEventEmitter, Text } from 'react-native';

jest.mock('maps-sdk-rn', () => ({
  getSdkCompiledVersion: () => 'jest-stub-version',
}));

jest.mock('maps-sdk-rn/MapView', () => {
  const React = require('react');
  const { View } = require('react-native');
  const MockMapView = React.forwardRef(
    (props: Record<string, unknown>, ref: unknown) => {
      React.useImperativeHandle(ref, () => ({
        clearVectorTileSource: jest.fn(),
        reloadTileSource: jest.fn(),
        clearLocationIndicator: jest.fn(),
        setLocationIndicator: jest.fn(),
        setCamera: jest.fn(),
        getCamera: jest.fn(() => null),
        addOverlay: jest.fn(async () => true),
        removeOverlay: jest.fn(async () => true),
        setVectorTileSource: jest.fn(async () => true),
        getVectorTileSourceStatus: jest.fn(async () => ({
          requestedTileCount: 0,
          presentTileCount: 0,
          failedTileCount: 0,
        })),
      }));
      return React.createElement(View, props);
    },
  );
  return { MapView: MockMapView, default: MockMapView };
});

/** The one shared SDK stub every test drives. */
const mockSdk = {
  initialize: jest.fn(async () => undefined),
  getCompiledVersion: jest.fn(async () => 'jest-stub-version'),
  defaultProviderKey: jest.fn(async () => 'mapquest'),
  setProviderCredential: jest.fn(async () => ({ isApplied: false, detailText: '' })),
  setProviderCredentialWithTileCache: jest.fn(async () => ({
    isApplied: false,
    detailText: '',
  })),
  configureProvider: jest.fn(async () => undefined),
  providerRegistrations: jest.fn(async () => []),
  routeCapabilities: jest.fn(async () => ({
    supportedTransportationModes: [0, 2],
    supportsShortestRouteStrategy: true,
  })),
  registerUrlTemplateRasterTileSource: jest.fn(async () => ({
    isApplied: false,
    detailText: '',
  })),
  restoreCredentialedBaseMap: jest.fn(async () => ({
    isApplied: false,
    detailText: '',
  })),
  search: jest.fn(async () => []),
  suggest: jest.fn(async () => []),
  fetchTrafficFlow: jest.fn(async () => []),
  startDeviceLocation: jest.fn(async () => true),
  stopDeviceLocation: jest.fn(async () => true),
  latestDeviceFix: jest.fn(async () => null),
  deviceLocationSmoothedCourseDegrees: jest.fn(async () => null),
  deviceLocationSourceStatus: jest.fn(async () => null),
  forwardGeocode: jest.fn(async () => []),
  reverseGeocode: jest.fn(async () => []),
  tileCacheTerms: jest.fn(async () => null),
  setTileCacheTimeToLive: jest.fn(async () => undefined),
  latestProviderFailureDiagnostics: jest.fn(async () => []),
  diagnosticsLines: jest.fn(async () => []),
};

jest.mock('maps-sdk-rn/MapsSdk', () => ({
  MapsSdk: { create: async () => mockSdk, isAvailable: () => false },
  mapsSdk: { create: async () => mockSdk, isAvailable: () => false },
}));

/** The demo bridge module is absent unless a test installs one. */
const mockRefappMapModuleHolder: {
  module: Record<string, unknown> | null;
} = { module: null };

jest.mock('../src/RefappNative', () => {
  const actualModule = jest.requireActual(
    '../src/RefappNative',
  ) as typeof import('../src/RefappNative');
  return {
    ...actualModule,
    get refappMapModule() {
      return mockRefappMapModuleHolder.module;
    },
    hasNativeController: () => mockRefappMapModuleHolder.module !== null,
  };
});

/** The mount-path methods the shell calls on a present demo bridge module. */
function createMockRefappMapModule(
  overrides: Record<string, unknown>,
): Record<string, unknown> {
  return {
    createSdk: jest.fn(),
    applyCameraPreset: jest.fn(),
    getSpeechEngineState: jest.fn(async () => ({ isEngineReady: false })),
    isProviderCredentialApplied: jest.fn(async () => false),
    getStoredProviderCredential: jest.fn(async () => null),
    loadPreferences: jest.fn(async () => null),
    storePreferences: jest.fn(),
    clearPreferences: jest.fn(),
    searchPlaces: jest.fn(async () => []),
    assignSearchResult: jest.fn(async () => true),
    ...overrides,
  };
}

jest.mock('react-native-safe-area-context', () => {
  const React = require('react');
  return {
    SafeAreaProvider: (props: { children?: React.ReactNode }) =>
      React.createElement(React.Fragment, null, props.children ?? null),
    useSafeAreaInsets: () => ({ top: 0, bottom: 0, left: 0, right: 0 }),
  };
});

import App from '../App';
import {
  ProviderContract,
  TransportationMode,
} from 'maps-sdk-rn/src/generated/rustic_maps.ts';
import { demoPlaces, demoPlans, demoRoutes } from '../src/referenceScenes';
import { providerDescriptors } from '../src/providerRegistry';
import { appStrings } from '../src/theme';

afterEach(() => {
  jest.useRealTimers();
  mockRefappMapModuleHolder.module = null;
  jest.clearAllMocks();
  (mockSdk.latestDeviceFix as jest.Mock).mockResolvedValue(null);
  (mockSdk.startDeviceLocation as jest.Mock).mockResolvedValue(true);
  (mockSdk.deviceLocationSourceStatus as jest.Mock).mockResolvedValue(null);
  (mockSdk.providerRegistrations as jest.Mock).mockResolvedValue([]);
});

/** Renders the screen and lets the mount effects settle. */
async function renderApp(): Promise<ReactTestRenderer.ReactTestRenderer> {
  let testRenderer: ReactTestRenderer.ReactTestRenderer;
  await ReactTestRenderer.act(() => {
    testRenderer = ReactTestRenderer.create(<App />);
  });
  return testRenderer!;
}

async function settleMountEffects(): Promise<void> {
  await ReactTestRenderer.act(async () => {
    await new Promise<void>(resolve => setTimeout(() => resolve(), 0));
  });
}

async function openSearchView(
  testRenderer: ReactTestRenderer.ReactTestRenderer,
): Promise<void> {
  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'set-origin-button' }).props.onPress();
  });
}

test('the plan fields carry visible labels, inline Search actions and per-field My location chips', async () => {
  const testRenderer = await renderApp();
  await openSearchView(testRenderer);

  // The duplicate switch-field row is gone; the visible labels carry markers.
  expect(
    testRenderer.root.findAllByProps({ testID: 'switch-origin-field' }),
  ).toHaveLength(0);
  expect(
    testRenderer.root.findAllByProps({ testID: 'switch-destination-field' }),
  ).toHaveLength(0);
  expect(
    testRenderer.root.findByProps({ testID: 'origin-field-marker' }).props.children,
  ).toBe('A');
  expect(
    testRenderer.root.findByProps({ testID: 'destination-field-marker' }).props.children,
  ).toBe('B');

  const renderedJson = JSON.stringify(testRenderer.toJSON());
  expect(renderedJson).toContain(appStrings.planOriginLabelText);
  expect(renderedJson).toContain(appStrings.planDestinationLabelText);
  expect(renderedJson).toContain(appStrings.planSearchActionText);
  expect(renderedJson).toContain(appStrings.planMyLocationActionText);
  expect(renderedJson).toContain(appStrings.planSwapActionText);

  for (const testID of [
    'origin-search-action',
    'destination-search-action',
    'origin-my-location',
    'destination-my-location',
    'swap-plan-fields',
    'search-results-list',
  ]) {
    expect(testRenderer.root.findByProps({ testID })).toBeDefined();
  }

  await ReactTestRenderer.act(async () => {
    testRenderer.unmount();
  });
});

test('the peek names the demo plan Start will replay until a route is planned', async () => {
  const artifactPlan = demoPlans.find(demoPlan => demoPlan.isArtifactTrace);
  expect(artifactPlan).toBeDefined();
  const testRenderer = await renderApp();

  // Before a route stands, the peek says which recorded plan Start replays.
  expect(
    testRenderer.root.findByProps({ testID: 'peek-trip-source' }).props.children,
  ).toBe(
    appStrings.planTripSourceDemoPlanFormat.replace('{plan}', artifactPlan!.label),
  );

  // A planned route replaces the demo-plan fallback, so the line goes away.
  await ReactTestRenderer.act(async () => {
    DeviceEventEmitter.emit('RefappPlanState', {
      isPlanned: true,
      isInFlight: false,
      titleText: 'Tower Bridge',
      distanceText: '1.2 km',
      durationText: '12 min 0 s',
    });
  });
  expect(
    testRenderer.root.findAllByProps({ testID: 'peek-trip-source' }),
  ).toHaveLength(0);

  await ReactTestRenderer.act(async () => {
    testRenderer.unmount();
  });
});

test('the route card renders the native distance and duration facts', async () => {
  const testRenderer = await renderApp();
  await ReactTestRenderer.act(async () => {
    DeviceEventEmitter.emit('RefappPlanState', {
      isPlanned: true,
      isInFlight: false,
      titleText: 'Tower Bridge',
      distanceText: '1.2 km',
      durationText: '12 min 0 s',
      isRecordedDemoRoute: true,
    });
  });

  const renderedJson = JSON.stringify(testRenderer.toJSON());
  expect(renderedJson).toContain('1.2 km · 12 min 0 s');
  expect(renderedJson).toContain(appStrings.recordedDemoRouteNoteText);
  expect(renderedJson).toContain(appStrings.routeSummaryStartButtonText);
  expect(renderedJson).toContain(appStrings.routeSummaryEditButtonText);
  // The card carries the route's facts, not a destination title (the iOS
  // card's shape). The offline archive's own title is the one exception.
  expect(
    testRenderer.root.findAllByProps({ testID: 'route-summary-title' }),
  ).toHaveLength(0);
  expect(renderedJson).not.toContain('Tower Bridge');

  await ReactTestRenderer.act(async () => {
    testRenderer.unmount();
  });
});

test('an unresolved keyless field names the field in the error strip', async () => {
  const testRenderer = await renderApp();
  await openSearchView(testRenderer);
  await ReactTestRenderer.act(async () => {
    testRenderer.root
      .findByProps({ testID: 'origin-search-field' })
      .props.onChangeText('Nowhere near the demo data');
    testRenderer.root
      .findByProps({ testID: 'destination-search-field' })
      .props.onChangeText('Also nowhere');
  });
  await ReactTestRenderer.act(async () => {
    await testRenderer.root.findByProps({ testID: 'plan-route' }).props.onPress();
  });

  expect(testRenderer.root.findByProps({ testID: 'error-strip' })).toBeDefined();
  const renderedJson = JSON.stringify(testRenderer.toJSON());
  expect(renderedJson).toContain('PlanFieldUnresolved');
  expect(renderedJson).toContain('the origin');

  await ReactTestRenderer.act(async () => {
    testRenderer.unmount();
  });
});

test('a My location chip fills only its own field and never plans', async () => {
  (mockSdk.latestDeviceFix as jest.Mock).mockResolvedValue({
    coordinate: { latitude: 37.12345, longitude: -122.54321 },
    horizontalAccuracyMeters: 5,
    courseHeadingDegrees: null,
    timestampSeconds: 1,
  });
  const planRecordedDemoRoute = jest.fn(async () => true);
  const planRoute = jest.fn(async () => true);
  mockRefappMapModuleHolder.module = createMockRefappMapModule({
    planRecordedDemoRoute,
    planRoute,
  });
  const testRenderer = await renderApp();
  await openSearchView(testRenderer);

  await ReactTestRenderer.act(async () => {
    await testRenderer.root
      .findByProps({ testID: 'origin-my-location' })
      .props.onPress();
  });
  expect(
    testRenderer.root.findByProps({ testID: 'origin-search-field' }).props.value,
  ).toBe('37.12345, -122.54321');
  expect(
    testRenderer.root.findByProps({ testID: 'destination-search-field' }).props.value,
  ).toBe('');

  await ReactTestRenderer.act(async () => {
    await testRenderer.root
      .findByProps({ testID: 'destination-my-location' })
      .props.onPress();
  });
  expect(
    testRenderer.root.findByProps({ testID: 'destination-search-field' }).props.value,
  ).toBe('37.12345, -122.54321');
  // The origin field kept its own fill: each chip touches one field only.
  expect(
    testRenderer.root.findByProps({ testID: 'origin-search-field' }).props.value,
  ).toBe('37.12345, -122.54321');
  // Filling a field never plans.
  expect(planRecordedDemoRoute).not.toHaveBeenCalled();
  expect(planRoute).not.toHaveBeenCalled();

  await ReactTestRenderer.act(async () => {
    testRenderer.unmount();
  });
});

test('a My location chip marks the field it filled and the mark follows a swap and an edit', async () => {
  (mockSdk.latestDeviceFix as jest.Mock).mockResolvedValue({
    coordinate: { latitude: 37.12345, longitude: -122.54321 },
    horizontalAccuracyMeters: 5,
    courseHeadingDegrees: null,
    timestampSeconds: 1,
  });
  const testRenderer = await renderApp();
  await settleMountEffects();
  await openSearchView(testRenderer);

  const chipLabel = (testID: string): string =>
    String(
      testRenderer.root
        .findByProps({ testID })
        .findByType(Text).props.children,
    );

  await ReactTestRenderer.act(async () => {
    await testRenderer.root
      .findByProps({ testID: 'origin-my-location' })
      .props.onPress();
  });
  expect(chipLabel('origin-my-location')).toBe(
    `${appStrings.planMyLocationActionText} ·`,
  );
  expect(chipLabel('destination-my-location')).toBe(
    appStrings.planMyLocationActionText,
  );
  expect(
    testRenderer.root.findByProps({ testID: 'origin-my-location' }).props
      .accessibilityState,
  ).toEqual({ disabled: false, selected: true });

  // A swap moves the fill mark with its text, exactly as it moves the texts.
  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'swap-plan-fields' }).props.onPress();
  });
  expect(chipLabel('origin-my-location')).toBe(
    appStrings.planMyLocationActionText,
  );
  expect(chipLabel('destination-my-location')).toBe(
    `${appStrings.planMyLocationActionText} ·`,
  );

  // Editing the filled field drops the mark: it no longer holds the fix.
  await ReactTestRenderer.act(async () => {
    testRenderer.root
      .findByProps({ testID: 'destination-search-field' })
      .props.onChangeText('Tower Bridge');
  });
  expect(chipLabel('destination-my-location')).toBe(
    appStrings.planMyLocationActionText,
  );

  await ReactTestRenderer.act(async () => {
    testRenderer.unmount();
  });
});

test('a My location chip is disabled with the denied reason the location panel reports', async () => {
  (mockSdk.startDeviceLocation as jest.Mock).mockResolvedValue(false);
  (mockSdk.deviceLocationSourceStatus as jest.Mock).mockResolvedValue(
    'permissionDenied',
  );
  const testRenderer = await renderApp();
  await settleMountEffects();
  await openSearchView(testRenderer);

  await ReactTestRenderer.act(async () => {
    await testRenderer.root
      .findByProps({ testID: 'origin-my-location' })
      .props.onPress();
  });

  expect(
    testRenderer.root.findByProps({ testID: 'origin-my-location' }).props.disabled,
  ).toBe(true);
  expect(
    testRenderer.root.findByProps({ testID: 'destination-my-location' }).props
      .disabled,
  ).toBe(true);
  expect(
    testRenderer.root.findByProps({ testID: 'origin-my-location-reason' }).props
      .children,
  ).toBe('Permission denied');

  // A press on the disabled chip starts nothing again.
  const startCallsAfterFirstPress = (mockSdk.startDeviceLocation as jest.Mock).mock
    .calls.length;
  await ReactTestRenderer.act(async () => {
    await testRenderer.root
      .findByProps({ testID: 'origin-my-location' })
      .props.onPress();
  });
  expect(
    (mockSdk.startDeviceLocation as jest.Mock).mock.calls.length,
  ).toBe(startCallsAfterFirstPress);

  await ReactTestRenderer.act(async () => {
    testRenderer.unmount();
  });
});

test('a My location chip is disabled with the unavailable reason the location panel reports', async () => {
  (mockSdk.startDeviceLocation as jest.Mock).mockResolvedValue(false);
  (mockSdk.deviceLocationSourceStatus as jest.Mock).mockResolvedValue(
    'unavailable',
  );
  const testRenderer = await renderApp();
  await settleMountEffects();
  await openSearchView(testRenderer);

  await ReactTestRenderer.act(async () => {
    await testRenderer.root
      .findByProps({ testID: 'destination-my-location' })
      .props.onPress();
  });

  expect(
    testRenderer.root.findByProps({ testID: 'destination-my-location' }).props
      .disabled,
  ).toBe(true);
  expect(
    testRenderer.root.findByProps({ testID: 'destination-my-location-reason' })
      .props.children,
  ).toBe('Unavailable');

  await ReactTestRenderer.act(async () => {
    testRenderer.unmount();
  });
});

test('focusing the other field keeps the shared result list and moves the assignment target', async () => {
  const demoPlace = demoPlaces[0];
  const testRenderer = await renderApp();
  await settleMountEffects();
  await openSearchView(testRenderer);

  // A keyless search fills the shared result list with the demo matches.
  await ReactTestRenderer.act(async () => {
    testRenderer.root
      .findByProps({ testID: 'origin-search-field' })
      .props.onChangeText('capitol');
  });
  await ReactTestRenderer.act(async () => {
    testRenderer.root
      .findByProps({ testID: 'origin-search-action' })
      .props.onPress();
  });
  const resultRow = () =>
    testRenderer.root.findByProps({
      testID: 'search-result-' + demoPlace.name,
    });
  expect(resultRow()).toBeDefined();

  // Focusing the other field keeps the list (the iOS focus rule)...
  await ReactTestRenderer.act(async () => {
    testRenderer.root
      .findByProps({ testID: 'destination-search-field' })
      .props.onFocus();
  });
  expect(resultRow()).toBeDefined();

  // ...and the assignment target follows focus: the row fills destination.
  await ReactTestRenderer.act(async () => {
    resultRow().props.onPress();
  });
  expect(
    testRenderer.root.findByProps({ testID: 'destination-search-field' }).props
      .value,
  ).toBe(demoPlace.name);
  expect(
    testRenderer.root.findByProps({ testID: 'origin-search-field' }).props.value,
  ).toBe('capitol');

  await ReactTestRenderer.act(async () => {
    testRenderer.unmount();
  });
});

test('editing a field away from its assignment clears the assigned-row marker', async () => {
  const demoPlace = demoPlaces[0];
  const testRenderer = await renderApp();
  await settleMountEffects();
  await openSearchView(testRenderer);

  await ReactTestRenderer.act(async () => {
    testRenderer.root
      .findByProps({ testID: 'origin-search-field' })
      .props.onChangeText('capitol');
  });
  await ReactTestRenderer.act(async () => {
    testRenderer.root
      .findByProps({ testID: 'origin-search-action' })
      .props.onPress();
  });
  await ReactTestRenderer.act(async () => {
    testRenderer.root
      .findByProps({ testID: 'search-result-' + demoPlace.name })
      .props.onPress();
  });
  const assignedMarker = appStrings.planOriginLabelText + ' · ';
  expect(JSON.stringify(testRenderer.toJSON())).toContain(assignedMarker);

  // The edit leaves the assigned text, so the row stops reading as assigned.
  await ReactTestRenderer.act(async () => {
    testRenderer.root
      .findByProps({ testID: 'origin-search-field' })
      .props.onChangeText('Tower Bridge');
  });
  expect(JSON.stringify(testRenderer.toJSON())).not.toContain(assignedMarker);

  await ReactTestRenderer.act(async () => {
    testRenderer.unmount();
  });
});

test('a selected keyless routing provider plans through it instead of the recorded route', async () => {
  const routingDescriptor = providerDescriptors.find(
    descriptor =>
      (descriptor.served_contracts ?? []).includes('routing') &&
      (descriptor.credential_label ?? '').trim() === '' &&
      descriptor.registration_available?.react_native === true,
  );
  expect(routingDescriptor).toBeDefined();
  (mockSdk.providerRegistrations as jest.Mock).mockResolvedValue([
    {
      contract: ProviderContract.Routing,
      providerKey: routingDescriptor!.provider_key,
      displayName: routingDescriptor!.display_name,
      registered: true,
    },
  ]);
  const planRoute = jest.fn(async () => true);
  const planRecordedDemoRoute = jest.fn(async () => true);
  mockRefappMapModuleHolder.module = createMockRefappMapModule({
    isProviderCredentialApplied: jest.fn(async () => false),
    planRoute,
    planRecordedDemoRoute,
  });
  const recordedPair = demoRoutes[0];
  const testRenderer = await renderApp();
  await settleMountEffects();
  await settleMountEffects();
  await openSearchView(testRenderer);

  await ReactTestRenderer.act(async () => {
    testRenderer.root
      .findByProps({ testID: 'origin-search-field' })
      .props.onChangeText(recordedPair.origin_name);
    testRenderer.root
      .findByProps({ testID: 'destination-search-field' })
      .props.onChangeText(recordedPair.destination_name);
  });
  await ReactTestRenderer.act(async () => {
    await testRenderer.root.findByProps({ testID: 'plan-route' }).props.onPress();
  });
  await settleMountEffects();

  expect(planRoute).toHaveBeenCalledTimes(1);
  expect(planRecordedDemoRoute).not.toHaveBeenCalled();

  await ReactTestRenderer.act(async () => {
    testRenderer.unmount();
  });
});

test('swap exchanges the two fields and their resolved places', async () => {
  const testRenderer = await renderApp();
  await openSearchView(testRenderer);
  await ReactTestRenderer.act(async () => {
    testRenderer.root
      .findByProps({ testID: 'origin-search-field' })
      .props.onChangeText('Capitol Park');
    testRenderer.root
      .findByProps({ testID: 'destination-search-field' })
      .props.onChangeText('Tower Bridge');
  });

  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'swap-plan-fields' }).props.onPress();
  });

  expect(
    testRenderer.root.findByProps({ testID: 'origin-search-field' }).props.value,
  ).toBe('Tower Bridge');
  expect(
    testRenderer.root.findByProps({ testID: 'destination-search-field' }).props.value,
  ).toBe('Capitol Park');

  await ReactTestRenderer.act(async () => {
    testRenderer.unmount();
  });
});

test('typed text that is not a demo place resolves through the provider first result', async () => {
  const searchPlaces = jest.fn(async (queryText: string) => [
    {
      structuredAddressText: `${queryText} resolved`,
      latitudeDegrees: 10,
      longitudeDegrees: 20,
      qualityText: 'high',
    },
  ]);
  const assignSearchResult = jest.fn(async () => true);
  const planRoute = jest.fn(async () => true);
  mockRefappMapModuleHolder.module = createMockRefappMapModule({
    isProviderCredentialApplied: jest.fn(async () => true),
    searchPlaces,
    assignSearchResult,
    planRoute,
  });
  const testRenderer = await renderApp();
  await settleMountEffects();
  await openSearchView(testRenderer);

  await ReactTestRenderer.act(async () => {
    testRenderer.root
      .findByProps({ testID: 'origin-search-field' })
      .props.onChangeText('123 Main Street');
    testRenderer.root
      .findByProps({ testID: 'destination-search-field' })
      .props.onChangeText('456 Side Avenue');
  });
  await ReactTestRenderer.act(async () => {
    await testRenderer.root.findByProps({ testID: 'plan-route' }).props.onPress();
  });
  await settleMountEffects();

  expect(searchPlaces).toHaveBeenCalledWith('123 Main Street');
  expect(searchPlaces).toHaveBeenCalledWith('456 Side Avenue');
  expect(assignSearchResult).toHaveBeenCalledTimes(2);
  expect(assignSearchResult).toHaveBeenCalledWith(
    true,
    '123 Main Street resolved',
    10,
    20,
  );
  expect(assignSearchResult).toHaveBeenCalledWith(
    false,
    '456 Side Avenue resolved',
    10,
    20,
  );
  expect(planRoute).toHaveBeenCalledWith(
    TransportationMode.Automobile,
    expect.anything(),
  );

  await ReactTestRenderer.act(async () => {
    testRenderer.unmount();
  });
});

test('the diagnostics stream stays off until the pane opens, then follows the opt-in policy', async () => {
  const startDiagnosticEvents = jest.fn();
  const stopDiagnosticEvents = jest.fn();
  const getDiagnosticsLines = jest.fn(async () => ['SDK version: jest']);
  mockRefappMapModuleHolder.module = createMockRefappMapModule({
    startDiagnosticEvents,
    stopDiagnosticEvents,
    getDiagnosticsLines,
  });
  const consoleLogSpy = jest.spyOn(console, 'log').mockImplementation(() => undefined);
  const testRenderer = await renderApp();
  await settleMountEffects();

  // The stream's 500 ms cadence is a wall-clock interval; fake timers make
  // the coalescing assertion deterministic (the mount already settled on the
  // real timers above).
  jest.useFakeTimers();

  // Nothing subscribes and nothing logs before the visitor asks for diagnostics.
  expect(startDiagnosticEvents).not.toHaveBeenCalled();
  expect(consoleLogSpy).not.toHaveBeenCalled();
  expect(
    testRenderer.root.findAllByProps({ testID: 'diagnostics-sheet' }),
  ).toHaveLength(0);

  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'readiness-chip' }).props.onPress();
  });
  expect(startDiagnosticEvents).toHaveBeenCalledWith(2, 100);
  expect(
    testRenderer.root.findByProps({ testID: 'diagnostics-sheet' }),
  ).toBeDefined();

  // One delivered event reaches the bounded window on the 500 ms cadence.
  await ReactTestRenderer.act(async () => {
    DeviceEventEmitter.emit('RefappDiagnosticEvent', {
      timestampMillisecondsSinceEpoch: '1',
      severity: 'info',
      subsystemTarget: 'tile_fetch',
      messageLine: 'tile fetched',
    });
  });
  await ReactTestRenderer.act(async () => {
    jest.advanceTimersByTime(500);
  });
  expect(JSON.stringify(testRenderer.toJSON())).toContain('tile fetched');

  // The explicit switch lowers the gate to Debug exactly once.
  await ReactTestRenderer.act(async () => {
    testRenderer.root
      .findByProps({ testID: 'diagnostics-debug-toggle' })
      .props.onPress();
  });
  expect(startDiagnosticEvents).toHaveBeenLastCalledWith(1, 100);
  expect(JSON.stringify(testRenderer.toJSON())).toContain(
    appStrings.diagnosticsHideDebugEventsText,
  );

  // Clear empties the display list.
  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'diagnostics-clear' }).props.onPress();
  });
  expect(
    testRenderer.root.findByProps({ testID: 'diagnostics-no-events' }).props.children,
  ).toBe(appStrings.diagnosticsNoEventsText);

  // Closing removes the listener; no console line appeared.
  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'diagnostics-close' }).props.onPress();
  });
  expect(stopDiagnosticEvents).toHaveBeenCalled();
  expect(consoleLogSpy).not.toHaveBeenCalled();

  jest.useRealTimers();
  consoleLogSpy.mockRestore();
  await ReactTestRenderer.act(async () => {
    testRenderer.unmount();
  });
});
