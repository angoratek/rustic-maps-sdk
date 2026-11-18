/**
 * Jest-level sanity of the reference application shell: the v2 screen renders
 * from the shared demo artifact without a native side — the native seam is
 * mocked (no TurboModule, no bridge module), so the honest no-native states
 * render instead of a crash.
 */

import React from 'react';
import ReactTestRenderer from 'react-test-renderer';
import { DeviceEventEmitter, Dimensions, StyleSheet } from 'react-native';

// The native seams are absent in the jest environment: bridge module null
// (RefappNative guards it), TurboModule require mocked to a stub.
jest.mock('maps-sdk-rn', () => ({
  getSdkCompiledVersion: () => 'jest-stub-version',
}));

// The package's typed MapView and SDK proxy are mocked at their deep entry
// points: the jest environment has no native view manager or module.
jest.mock('maps-sdk-rn/MapView', () => {
  const React = require('react');
  const { View } = require('react-native');
  // The host view's imperative surface: the shell calls these from the
  // device-position poll and the base-map switch, so the mock carries them
  // instead of a bare View ref.
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

jest.mock('maps-sdk-rn/MapsSdk', () => {
  const mockSdk = {
    initialize: async () => undefined,
    getCompiledVersion: async () => 'jest-stub-version',
    defaultProviderKey: async () => 'mapquest',
    setProviderCredential: async () => ({ isApplied: false, detailText: '' }),
    setProviderCredentialWithTileCache: async () => ({
      isApplied: false,
      detailText: '',
    }),
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
    restoreCredentialedBaseMap: async () => ({
      isApplied: false,
      detailText: '',
    }),
    search: async () => [],
    suggest: async () => [],
    fetchTrafficFlow: jest.fn(async () => []),
    startDeviceLocation: jest.fn(async () => true),
    stopDeviceLocation: jest.fn(async () => true),
    latestDeviceFix: jest.fn(async () => null),
    deviceLocationSmoothedCourseDegrees: jest.fn(async () => null),
    deviceLocationSourceStatus: jest.fn(async () => null),
    forwardGeocode: async () => [],
    reverseGeocode: async () => [],
    tileCacheTerms: async () => null,
    setTileCacheTimeToLive: async () => undefined,
    latestProviderFailureDiagnostics: async () => [],
    diagnosticsLines: async () => [],
  };
  return {
    MapsSdk: { create: async () => mockSdk, isAvailable: () => false },
    mapsSdk: { create: async () => mockSdk, isAvailable: () => false },
  };
});

/**
 * The demo bridge module is absent by default (the suite's honest no-native
 * states). A test that needs the device-navigation seam installs one here;
 * the accessors read the holder lazily so every other test keeps the null
 * module. The `mock` prefix is what the jest hoisting contract requires.
 */
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
    searchPlaces: jest.fn(async () => []),
    assignSearchResult: jest.fn(async () => true),
    ...overrides,
  };
}

// SafeAreaProvider without native insets delivers no children in the jest
// environment; a zero-inset stub keeps the whole screen tree renderable.
jest.mock('react-native-safe-area-context', () => {
  const React = require('react');
  return {
    SafeAreaProvider: (props: { children?: React.ReactNode }) =>
      React.createElement(React.Fragment, null, props.children ?? null),
    useSafeAreaInsets: () => ({ top: 0, bottom: 0, left: 0, right: 0 }),
  };
});

import { MapsSdk } from 'maps-sdk-rn/MapsSdk';
import {
  ProviderContract,
  RouteStrategy,
  TransportationMode,
} from 'maps-sdk-rn/src/generated/rustic_maps.ts';
import App from '../App';
import {
  cameraPresets,
  demoPlaces,
  demoPlans,
  overlayScenes,
} from '../src/referenceScenes';
import {
  appStrings,
  defaultThemeChoice,
  resolvedThemeName,
  themeChoiceSwitchLabel,
  themeColorsByThemeName,
} from '../src/theme';
import { sheetFullHeightFraction } from '../src/sheetLayout';
import { providerDescriptors } from '../src/providerRegistry';

afterEach(async () => {
  mockRefappMapModuleHolder.module = null;
  const sdk = await MapsSdk.create();
  (sdk.providerRegistrations as jest.Mock).mockResolvedValue([]);
  (sdk.routeCapabilities as jest.Mock).mockResolvedValue({
    supportedTransportationModes: [
      TransportationMode.Automobile,
      TransportationMode.Pedestrian,
    ],
    supportsShortestRouteStrategy: true,
  });
  (sdk.configureProvider as jest.Mock).mockClear();
});

/** Renders the screen and lets the mount effects settle. */
async function renderApp(): Promise<ReactTestRenderer.ReactTestRenderer> {
  let testRenderer: ReactTestRenderer.ReactTestRenderer;
  await ReactTestRenderer.act(() => {
    testRenderer = ReactTestRenderer.create(<App />);
  });
  return testRenderer!;
}

test('renders the reference screen from the shared demo artifact', async () => {
  let testRenderer: ReactTestRenderer.ReactTestRenderer;
  await ReactTestRenderer.act(() => {
    testRenderer = ReactTestRenderer.create(<App />);
  });
  const json = testRenderer!.toJSON();
  expect(json).toBeTruthy();
});

test('the v2 peek rows and the offline card render without a native side', async () => {
  let testRenderer: ReactTestRenderer.ReactTestRenderer;
  await ReactTestRenderer.act(() => {
    testRenderer = ReactTestRenderer.create(<App />);
  });
  const json = JSON.stringify(testRenderer!.toJSON());
  // The two-row peek (§4.2): the plan flow's entry field and the primary act.
  expect(json).toContain(appStrings.whereToText);
  expect(json).toContain(appStrings.peekPrimaryActionText);
  // The offline maps card renders its honest unregistered line — no native
  // side means no cache registration (§4.1's two honest lines).
  expect(json).toContain(appStrings.offlineMapsCardTitleText);
  expect(json).toContain('Cache not registered');
  // The theme row renders the three-way control, whose label names the next
  // choice; Auto is the default of a device that never chose (§3).
  expect(json).toContain(appStrings.themeRowTitleText);
  expect(json).toContain(themeChoiceSwitchLabel(defaultThemeChoice));
});

test('the location control starts the device source and offers to stop it', async () => {
  const testRenderer = await renderApp();
  const locationControl = testRenderer.root.findByProps({
    testID: 'device-location-control',
  });
  await ReactTestRenderer.act(async () => {
    await locationControl.props.onPress();
  });
  const json = JSON.stringify(testRenderer.toJSON());
  expect(json).toContain('Hide my location');
  const sdk = await MapsSdk.create();
  expect(sdk.startDeviceLocation).toHaveBeenCalled();
  await ReactTestRenderer.act(async () => {
    testRenderer.unmount();
  });
});

test('the device-navigation control states the plan-first sentence without a planned route', async () => {
  const testRenderer = await renderApp();
  const sdk = await MapsSdk.create();
  const startDeviceLocationMock = sdk.startDeviceLocation as jest.Mock;
  startDeviceLocationMock.mockClear();
  const navigationControl = testRenderer.root.findByProps({
    testID: 'device-navigation-control',
  });
  await ReactTestRenderer.act(async () => {
    await navigationControl.props.onPress();
  });
  expect(
    testRenderer.root.findByProps({ testID: 'device-location-status' }).props
      .children,
  ).toBe('Plan a route first; device navigation follows the route you planned.');
  // Nothing started: no location source, no guidance, and the control still
  // offers to start.
  expect(startDeviceLocationMock).not.toHaveBeenCalled();
  expect(JSON.stringify(testRenderer.toJSON())).toContain(
    'Navigate from my location',
  );
  await ReactTestRenderer.act(async () => {
    testRenderer.unmount();
  });
});

test('the device-navigation control starts guidance on the planned route and stops it', async () => {
  const startDeviceNavigation = jest.fn(async () => true);
  const stopDeviceNavigation = jest.fn();
  const deliverDeviceNavigationFix = jest.fn();
  mockRefappMapModuleHolder.module = createMockRefappMapModule({
    startDeviceNavigation,
    stopDeviceNavigation,
    deliverDeviceNavigationFix,
  });
  const testRenderer = await renderApp();
  // The keyless recorded plan installs the route the control follows; the
  // plan state rides the same native event the route summary card reads.
  await ReactTestRenderer.act(async () => {
    DeviceEventEmitter.emit('RefappPlanState', {
      isPlanned: true,
      isInFlight: false,
      titleText: 'Capitol Park to Tower Bridge',
    });
  });
  const sdk = await MapsSdk.create();
  const startDeviceLocationMock = sdk.startDeviceLocation as jest.Mock;
  const stopDeviceLocationMock = sdk.stopDeviceLocation as jest.Mock;
  startDeviceLocationMock.mockClear();
  stopDeviceLocationMock.mockClear();

  await ReactTestRenderer.act(async () => {
    await testRenderer.root
      .findByProps({ testID: 'device-navigation-control' })
      .props.onPress();
  });
  // The mode starts the location source when it is off and then starts
  // guidance on the planned route.
  expect(startDeviceLocationMock).toHaveBeenCalledTimes(1);
  expect(startDeviceNavigation).toHaveBeenCalledTimes(1);
  expect(JSON.stringify(testRenderer.toJSON())).toContain(
    'Stop device navigation',
  );
  expect(
    testRenderer.root.findByProps({ testID: 'device-location-status' }).props
      .children,
  ).toBe('Waiting for your position to start guidance.');

  await ReactTestRenderer.act(async () => {
    await testRenderer.root
      .findByProps({ testID: 'device-navigation-control' })
      .props.onPress();
  });
  // Stopping ends the trip and the source this mode started.
  expect(stopDeviceNavigation).toHaveBeenCalledTimes(1);
  expect(stopDeviceLocationMock).toHaveBeenCalledTimes(1);
  expect(JSON.stringify(testRenderer.toJSON())).toContain(
    'Navigate from my location',
  );
  await ReactTestRenderer.act(async () => {
    testRenderer.unmount();
  });
});

test('the plan flow gates its route action until both fields hold text', async () => {
  const testRenderer = await renderApp();
  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'set-origin-button' }).props.onPress();
  });
  expect(testRenderer.root.findByProps({ testID: 'search-heading' })).toBeTruthy();
  const planRouteButton = () => testRenderer.root.findByProps({ testID: 'plan-route' });
  expect(planRouteButton().props.disabled).toBe(true);
  expect(
    testRenderer.root.findByProps({ testID: 'plan-route-reason' }).props.children,
  ).toBe(appStrings.planRouteMissingBothText);

  await ReactTestRenderer.act(async () => {
    testRenderer.root
      .findByProps({ testID: 'origin-search-field' })
      .props.onChangeText('Capitol Park');
  });
  expect(planRouteButton().props.disabled).toBe(true);
  expect(
    testRenderer.root.findByProps({ testID: 'plan-route-reason' }).props.children,
  ).toBe(appStrings.planRouteMissingDestinationText);

  await ReactTestRenderer.act(async () => {
    testRenderer.root
      .findByProps({ testID: 'destination-search-field' })
      .props.onChangeText('Tower Bridge');
  });
  expect(planRouteButton().props.disabled).toBe(false);
  expect(
    testRenderer.root.findAllByProps({ testID: 'plan-route-reason' }),
  ).toHaveLength(0);

  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'search-done' }).props.onPress();
  });
  expect(testRenderer.root.findAllByProps({ testID: 'search-heading' })).toHaveLength(0);
});

test('typing both demo places and pressing Plan route applies the recorded route', async () => {
  // The keyless plan reads the fields' own text at the moment of the press:
  // a stale callback closure used to report an unresolved destination even
  // though the field held a demo place name.
  const planRecordedDemoRoute = jest.fn(async () => true);
  mockRefappMapModuleHolder.module = createMockRefappMapModule({
    planRecordedDemoRoute,
  });
  const testRenderer = await renderApp();
  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'set-origin-button' }).props.onPress();
  });
  await ReactTestRenderer.act(async () => {
    testRenderer.root
      .findByProps({ testID: 'origin-search-field' })
      .props.onChangeText('Capitol Park');
  });
  await ReactTestRenderer.act(async () => {
    testRenderer.root
      .findByProps({ testID: 'destination-search-field' })
      .props.onChangeText('Tower Bridge');
  });
  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'plan-route' }).props.onPress();
  });
  expect(planRecordedDemoRoute).toHaveBeenCalledWith('Capitol Park', 'Tower Bridge');
  expect(testRenderer.root.findAllByProps({ testID: 'plan-route-reason' })).toHaveLength(0);
  await ReactTestRenderer.act(async () => {
    testRenderer.unmount();
  });
});

test('the theme toggle cycles the screen from Auto to the light table', async () => {
  const testRenderer = await renderApp();
  const themeToggle = testRenderer.root.findByProps({ testID: 'theme-toggle' });
  await ReactTestRenderer.act(async () => {
    themeToggle.props.onPress();
  });
  // The light table's ground color carries the screen backdrop (§2 light).
  const json = JSON.stringify(testRenderer.toJSON());
  expect(json).toContain('#e8ebf0');
  expect(json).toContain('rgba(255, 255, 255, 0.92)');
  // The control now names the next choice after Light: Dark (§3).
  expect(json).toContain(themeChoiceSwitchLabel('light'));
  expect(json).not.toContain(themeChoiceSwitchLabel(defaultThemeChoice));
});

test('the artifact camera presets reach the dock as chips', async () => {
  expect(cameraPresets.length).toBeGreaterThan(0);
  let testRenderer: ReactTestRenderer.ReactTestRenderer;
  await ReactTestRenderer.act(() => {
    testRenderer = ReactTestRenderer.create(<App />);
  });
  const json = JSON.stringify(testRenderer!.toJSON());
  for (const preset of cameraPresets) {
    expect(json).toContain(preset.label);
  }
});

test('the map control cluster renders the 2D plane state with honest no-data hides', async () => {
  let testRenderer: ReactTestRenderer.ReactTestRenderer;
  await ReactTestRenderer.act(() => {
    testRenderer = ReactTestRenderer.create(<App />);
  });
  const root = testRenderer!.root;
  // The cluster's four controls (§4.2) are present without a native side.
  expect(root.findByProps({ testID: 'map-control-cluster' })).toBeDefined();
  expect(root.findByProps({ testID: 'map-control-zoom-in' })).toBeDefined();
  expect(root.findByProps({ testID: 'map-control-zoom-out' })).toBeDefined();
  expect(root.findByProps({ testID: 'map-control-my-location' })).toBeDefined();
  expect(root.findByProps({ testID: 'map-control-plane' })).toBeDefined();
  // The plane label reads the honest default (no camera event yet).
  expect(JSON.stringify(testRenderer!.toJSON())).toContain(
    appStrings.mapControlTwoDLabelText,
  );
  // No bearing and no ground answer: the compass and the scale bar render
  // nothing rather than an invented value (§4.2).
  expect(root.findAllByProps({ testID: 'compass-control' })).toHaveLength(0);
  expect(root.findAllByProps({ testID: 'scale-bar' })).toHaveLength(0);
});

test('a missing-provider map failure renders the shared consumer sentence', async () => {
  const testRenderer = await renderApp();
  const mapView = testRenderer!.root.findByProps({ testID: 'refapp-map' });
  await ReactTestRenderer.act(async () => {
    mapView.props.onError({
      phase: 'draw',
      message: 'ProviderNotConfigured(contractName=VectorTiles)',
    });
  });
  const readinessText = JSON.stringify(testRenderer!.toJSON());
  expect(readinessText).toContain(
    appStrings.providerNotConfiguredDetailTemplateText.replace(
      '{provider}',
      'VectorTiles',
    ),
  );
  expect(readinessText).not.toContain('ProviderNotConfigured(contractName=');
});

test('the readiness chip renders the neutral keyless prompt, never an implied state (D4)', async () => {
  const testRenderer = await renderApp();
  const chipText = testRenderer.root.findByProps({ testID: 'readiness-chip-text' });
  // No credential applied and no readiness event yet: the neutral keyless
  // prompt, not a loading/ready/failed claim.
  expect(chipText.props.children).toBe(appStrings.providerCredentialKeylessNeutralText);
  const chipStyleEntries = ([] as ReadonlyArray<object | boolean>).concat(
    chipText.props.style as ReadonlyArray<object | boolean>,
  );
  // The test renderer reports no system scheme, so Auto resolves to the light
  // table's secondary text colour (docs/ReferenceAppUX.md §3).
  const defaultResolvedThemeColors =
    themeColorsByThemeName[resolvedThemeName(defaultThemeChoice, null)];
  const carriesNeutralColor = chipStyleEntries.some(
    styleEntry =>
      typeof styleEntry === 'object' &&
      styleEntry !== null &&
      'color' in styleEntry &&
      (styleEntry as { color: string }).color === defaultResolvedThemeColors.textSecondary,
  );
  expect(carriesNeutralColor).toBe(true);
});

test('the diagnostics pane opens with an empty, opt-in stream state', async () => {
  const testRenderer = await renderApp();
  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'readiness-chip' }).props.onPress();
  });
  const json = JSON.stringify(testRenderer.toJSON());
  expect(json).toContain(appStrings.diagnosticsLinesUnavailableText);
  expect(json).toContain(appStrings.diagnosticsRecentEventsTitleText);
  expect(
    testRenderer.root.findByProps({ testID: 'diagnostics-no-events' }).props.children,
  ).toBe(appStrings.diagnosticsNoEventsText);
  expect(json).toContain(appStrings.diagnosticsStreamScopeNoteText);
  // Closing the pane stops the stream's refresh cadence, so no timer outlives
  // the test.
  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'diagnostics-close' }).props.onPress();
  });
  await ReactTestRenderer.act(async () => {
    testRenderer.unmount();
  });
});

test('the attribution text stays available behind the info control during a simulated trip (D6)', async () => {
  const testRenderer = await renderApp();
  await ReactTestRenderer.act(async () => {
    DeviceEventEmitter.emit('RefappTraceState', { isActive: true });
  });
  expect(testRenderer.root.findByProps({ testID: 'navigation-drawer' })).toBeDefined();
  // The compact control replaced the always-visible line: the compliance
  // text lives in its panel and is still one tap away during a trip.
  await ReactTestRenderer.act(async () => {
    testRenderer.root
      .findByProps({ testID: 'attribution-info-control' })
      .props.onPress();
  });
  const attributionNotice = testRenderer.root.findByProps({ testID: 'attribution' });
  expect(JSON.stringify(attributionNotice.props.children)).toContain('OpenStreetMap');
});

test('the drawer carries the speed chips and one Pause/Resume control (§7.2)', async () => {
  const testRenderer = await renderApp();
  await ReactTestRenderer.act(async () => {
    DeviceEventEmitter.emit('RefappTraceState', { isActive: true, traceName: 'walk' });
  });
  const drawerText = JSON.stringify(testRenderer.toJSON());
  expect(drawerText).toContain(appStrings.navigationSpeedLabelText);
  [1, 2, 4, 8].forEach(speedMultiplier => {
    expect(
      testRenderer.root.findByProps({
        testID: 'navigation-speed-chip-' + speedMultiplier,
      }),
    ).toBeDefined();
  });
  expect(
    testRenderer.root.findByProps({ testID: 'drawer-playback-pause' }),
  ).toBeDefined();
  // A paused trip's one control reads Resume; the chips keep the last pace.
  await ReactTestRenderer.act(async () => {
    DeviceEventEmitter.emit('RefappPlaybackState', {
      isActive: true,
      isPaused: true,
      speedMultiplier: 4,
      hasArrived: false,
      isSeekable: true,
      totalMeters: 1436,
    });
  });
  expect(JSON.stringify(testRenderer.toJSON())).toContain(
    appStrings.navigationResumeActionText,
  );
  // The arrival hides the playback controls; the exit stays.
  await ReactTestRenderer.act(async () => {
    DeviceEventEmitter.emit('RefappTraceUpdate', {
      phase: 'Arrived',
      isArrival: true,
      isReroutingRequired: false,
      bannerHeadlineText: '',
      bannerDisplayText: '',
      upcomingDisplayText: null,
      progressLineText: null,
      travelledMeters: 1436,
    });
  });
  expect(
    testRenderer.root.findAllByProps({ testID: 'navigation-speed-row' }),
  ).toHaveLength(0);
  expect(
    testRenderer.root.findAllByProps({ testID: 'drawer-playback-pause' }),
  ).toHaveLength(0);
  expect(testRenderer.root.findByProps({ testID: 'drawer-exit' })).toBeDefined();
});

test('a new simulated trip drops the previous trip\'s arrival state', async () => {
  const testRenderer = await renderApp();
  const arrivedUpdate = {
    phase: 'Arrived',
    isArrival: true,
    isReroutingRequired: false,
    bannerHeadlineText: 'Arrive',
    bannerDisplayText: 'Arrive at Capitol Park',
    upcomingDisplayText: null,
    progressLineText: '0 m',
    travelledMeters: 1436,
  };
  await ReactTestRenderer.act(async () => {
    DeviceEventEmitter.emit('RefappTraceState', { isActive: true, traceName: 'first' });
    DeviceEventEmitter.emit('RefappTraceUpdate', arrivedUpdate);
  });
  expect(
    testRenderer.root.findByProps({ testID: 'drawer-phase-line' }).props.children,
  ).toBe('Arrived');
  // Exiting and starting another replay must not leave the finished trip's
  // phase or ETA beside the new trip's counting countdown.
  await ReactTestRenderer.act(async () => {
    DeviceEventEmitter.emit('RefappTraceState', { isActive: false, traceName: 'first' });
    DeviceEventEmitter.emit('RefappTraceState', { isActive: true, traceName: 'second' });
  });
  expect(
    testRenderer.root.findByProps({ testID: 'drawer-phase-line' }).props.children,
  ).toBe(appStrings.phaseNotStartedText);
  expect(
    testRenderer.root
      .findByProps({ testID: 'drawer-eta-line' })
      .props.children.join(''),
  ).toBe(appStrings.navigationDrawerEtaLabelText + ': ');
});

test('the layers panel scene switches are disabled-with-reason until the map is ready (D7)', async () => {
  const testRenderer = await renderApp();
  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'map-layers-control' }).props.onPress();
  });
  const firstSceneKey = overlayScenes[0].sceneKey;
  expect(
    testRenderer.root.findByProps({
      testID: 'map-layer-scene-' + firstSceneKey,
    }).props.disabled,
  ).toBe(true);
  expect(
    testRenderer.root.findByProps({ testID: 'map-layer-scenes-reason' }).props.children,
  ).toContain(appStrings.demoScenesUnavailableReasonText);
});

test('a keyless search offers the artifact demo places and says so', async () => {
  const testRenderer = await renderApp();
  const peekSearch = testRenderer.root.findByProps({ testID: 'peek-search-field' });
  await ReactTestRenderer.act(async () => {
    peekSearch.props.onPress();
  });
  const demoPlace = demoPlaces[0];
  const queryText = demoPlace.name.slice(0, 5);
  let originField = testRenderer.root.findByProps({ testID: 'origin-search-field' });
  await ReactTestRenderer.act(async () => {
    originField.props.onChangeText(queryText);
  });
  originField = testRenderer.root.findByProps({ testID: 'origin-search-field' });
  await ReactTestRenderer.act(async () => {
    originField.props.onSubmitEditing();
  });
  const json = JSON.stringify(testRenderer.toJSON());
  // The rows are the artifact's demo places; the list says so and carries
  // the artifact's own note (never a provider result).
  expect(json).toContain(appStrings.demoPlacesHeadingText);
  expect(json).toContain(demoPlace.name);
  expect(json).toContain(demoPlace.note);
});

test('the guided walk action is disabled with its reason when nothing can run (D8)', async () => {
  const testRenderer = await renderApp();
  expect(
    testRenderer.root.findByProps({ testID: 'start-simulated-navigation' }).props
      .disabled,
  ).toBe(true);
  const json = JSON.stringify(testRenderer.toJSON());
  expect(json).toContain(appStrings.guidedWalkUnavailableNoNativeText);
});

/** Lets the mount-time preference and SDK promises settle. */
async function settleMountEffects(): Promise<void> {
  await ReactTestRenderer.act(async () => {
    await new Promise<void>(resolve => setTimeout(() => resolve(), 0));
  });
}

/** The record the last `storePreferences` call wrote. */
function latestStoredPreferences(storePreferences: jest.Mock): Record<string, unknown> {
  expect(storePreferences).toHaveBeenCalled();
  const latestCall = storePreferences.mock.calls[storePreferences.mock.calls.length - 1];
  return JSON.parse(latestCall[0] as string) as Record<string, unknown>;
}

test('a remembered preferences record restores choices without planning a route', async () => {
  const rememberedPlan = demoPlans[1];
  const rememberedPreset = cameraPresets[1];
  const rememberedSceneKey = overlayScenes[1].sceneKey;
  const planRoute = jest.fn(async () => true);
  const planRecordedDemoRoute = jest.fn(async () => true);
  const selectTraceReplayPlan = jest.fn(async () => true);
  const applyCameraPreset = jest.fn();
  const selectTraceReplaySpeedMultiplier = jest.fn();
  const setSceneOverlayToggle = jest.fn();
  const storePreferences = jest.fn();
  mockRefappMapModuleHolder.module = createMockRefappMapModule({
    loadPreferences: jest.fn(async () =>
      JSON.stringify({
        version: 1,
        themeChoice: 'dark',
        baseMap: null,
        demoPlanAssetName: rememberedPlan.traceAssetName,
        selectedSceneKeys: [rememberedSceneKey],
        cameraPresetId: rememberedPreset.id,
        speedMultiplier: 4,
        search: {
          origin: demoPlaces[0].name,
          destination: demoPlaces[1].name,
          lastQuery: demoPlaces[1].name,
        },
      }),
    ),
    storePreferences,
    clearPreferences: jest.fn(),
    planRoute,
    planRecordedDemoRoute,
    selectTraceReplayPlan,
    applyCameraPreset,
    selectTraceReplaySpeedMultiplier,
    setSceneOverlayToggle,
  });
  const testRenderer = await renderApp();
  await settleMountEffects();

  // The theme table and its control follow the remembered choice.
  const json = JSON.stringify(testRenderer.toJSON());
  expect(json).toContain(themeColorsByThemeName.dark.screenBackdrop);
  expect(json).toContain(themeColorsByThemeName.dark.surface);
  expect(json).toContain(themeChoiceSwitchLabel('dark'));

  // The plan, camera and pace restore through the native seam.
  expect(selectTraceReplayPlan).toHaveBeenCalledWith(rememberedPlan.traceAssetName);
  expect(applyCameraPreset).toHaveBeenCalledWith(
    rememberedPreset.latitude_degrees,
    rememberedPreset.longitude_degrees,
    rememberedPreset.zoom_level,
    rememberedPreset.bearing_degrees,
    rememberedPreset.pitch_degrees,
  );
  expect(selectTraceReplaySpeedMultiplier).toHaveBeenCalledWith(4);

  // The remembered search pair fills the fields but never plans a route.
  expect(planRoute).not.toHaveBeenCalled();
  expect(planRecordedDemoRoute).not.toHaveBeenCalled();
  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'set-origin-button' }).props.onPress();
  });
  expect(
    testRenderer.root.findByProps({ testID: 'origin-search-field' }).props.value,
  ).toBe(demoPlaces[0].name);
  expect(
    testRenderer.root.findByProps({ testID: 'destination-search-field' }).props.value,
  ).toBe(demoPlaces[1].name);

  // A restored scene chip reaches the live surface once it is ready.
  await ReactTestRenderer.act(async () => {
    DeviceEventEmitter.emit('RefappReadiness', {
      state: 'ready',
      text: 'Maps SDK gpu renderer active (version jest)',
    });
  });
  expect(setSceneOverlayToggle).toHaveBeenCalledWith(rememberedSceneKey, true);

  // Hydration writes nothing back.
  expect(storePreferences).not.toHaveBeenCalled();
});

test('each explicit choice writes the versioned record', async () => {
  const storePreferences = jest.fn();
  const module = createMockRefappMapModule({
    loadPreferences: jest.fn(async () => null),
    storePreferences,
    clearPreferences: jest.fn(),
    clearSceneOverlays: jest.fn(),
    setSceneOverlayToggle: jest.fn(),
    selectTraceReplayPlan: jest.fn(async () => true),
    selectTraceReplaySpeedMultiplier: jest.fn(),
  });
  mockRefappMapModuleHolder.module = module;
  const testRenderer = await renderApp();
  await settleMountEffects();
  expect(storePreferences).not.toHaveBeenCalled();

  // Theme.
  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'theme-toggle' }).props.onPress();
  });
  expect(latestStoredPreferences(storePreferences).themeChoice).toBe('light');

  // Camera preset.
  const nextPreset = cameraPresets[1];
  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'preset-' + nextPreset.id }).props.onPress();
  });
  expect(latestStoredPreferences(storePreferences).cameraPresetId).toBe(nextPreset.id);

  // The search fields and the last query.
  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'set-origin-button' }).props.onPress();
  });
  await ReactTestRenderer.act(async () => {
    testRenderer.root
      .findByProps({ testID: 'origin-search-field' })
      .props.onChangeText('Capitol Park');
  });
  expect(latestStoredPreferences(storePreferences).search).toMatchObject({
    origin: 'Capitol Park',
  });
  await ReactTestRenderer.act(async () => {
    testRenderer.root
      .findByProps({ testID: 'destination-search-field' })
      .props.onChangeText('Tower Bridge');
  });
  expect(latestStoredPreferences(storePreferences).search).toMatchObject({
    origin: 'Capitol Park',
    destination: 'Tower Bridge',
  });
  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'origin-search-field' }).props.onSubmitEditing();
  });
  expect(latestStoredPreferences(storePreferences).search).toMatchObject({
    lastQuery: 'Capitol Park',
  });
  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'search-done' }).props.onPress();
  });

  // A scene switch in the layers panel.
  await ReactTestRenderer.act(async () => {
    DeviceEventEmitter.emit('RefappReadiness', {
      state: 'ready',
      text: 'Maps SDK gpu renderer active (version jest)',
    });
  });
  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'map-layers-control' }).props.onPress();
  });
  await ReactTestRenderer.act(async () => {
    testRenderer.root
      .findByProps({ testID: 'map-layer-scene-' + overlayScenes[0].sceneKey })
      .props.onValueChange(true);
  });
  expect(latestStoredPreferences(storePreferences).selectedSceneKeys).toEqual([
    overlayScenes[0].sceneKey,
  ]);

  // A styled base map row.
  await ReactTestRenderer.act(async () => {
    testRenderer.root
      .findByProps({ testID: 'map-layer-style-bright-light' })
      .props.onPress();
  });
  await settleMountEffects();
  expect(latestStoredPreferences(storePreferences).baseMap).toEqual({
    providerKey: 'openfreemap',
    styleId: 'bright-light',
  });
  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'map-layers-close' }).props.onPress();
  });

  // A playback speed chip.
  await ReactTestRenderer.act(async () => {
    DeviceEventEmitter.emit('RefappTraceState', { isActive: true, traceName: 'walk' });
  });
  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'navigation-speed-chip-2' }).props.onPress();
  });
  expect(latestStoredPreferences(storePreferences).speedMultiplier).toBe(2);
});

test('the layers panel base-map rows run raster, two vector styles and back', async () => {
  // The cross-platform sequence on the React Native shell, driven through
  // the layers panel rows the app renders: a raster base, a committed vector
  // style, a second style of the same descriptor, and the raster again.
  const sdk = await MapsSdk.create();
  (sdk.registerUrlTemplateRasterTileSource as jest.Mock).mockResolvedValue({
    isApplied: true,
    detailText: '',
  });
  const storePreferences = jest.fn();
  mockRefappMapModuleHolder.module = createMockRefappMapModule({
    loadPreferences: jest.fn(async () => null),
    storePreferences,
    clearPreferences: jest.fn(),
  });
  const testRenderer = await renderApp();
  await settleMountEffects();
  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'map-layers-control' }).props.onPress();
  });
  await settleMountEffects();

  const pressRow = async (testID: string) => {
    await ReactTestRenderer.act(async () => {
      testRenderer.root.findByProps({ testID }).props.onPress();
    });
    await settleMountEffects();
  };
  const storedBaseMap = () => latestStoredPreferences(storePreferences).baseMap;
  const rowIsServing = (testID: string) =>
    testRenderer.root.findByProps({ testID }).props.accessibilityState.selected;

  // Raster: the credential-less base registers and serves.
  await pressRow('map-layer-base-openstreetmap');
  expect(storedBaseMap()).toEqual({
    providerKey: 'openstreetmap',
    styleId: null,
  });
  expect(rowIsServing('map-layer-base-openstreetmap')).toBe(true);

  // Raster → vector: the descriptor's committed style takes the base.
  await pressRow('map-layer-style-bright-light');
  expect(storedBaseMap()).toEqual({
    providerKey: 'openfreemap',
    styleId: 'bright-light',
  });
  expect(rowIsServing('map-layer-style-bright-light')).toBe(true);

  // Vector → vector: the second style takes it and the Serving mark moves.
  await pressRow('map-layer-style-fiord-dark');
  expect(storedBaseMap()).toEqual({
    providerKey: 'openfreemap',
    styleId: 'fiord-dark',
  });
  expect(rowIsServing('map-layer-style-bright-light')).toBe(false);
  expect(rowIsServing('map-layer-style-fiord-dark')).toBe(true);

  // Vector → raster: the raster row serves again and no style claims it.
  await pressRow('map-layer-base-openstreetmap');
  expect(storedBaseMap()).toEqual({
    providerKey: 'openstreetmap',
    styleId: null,
  });
  expect(rowIsServing('map-layer-style-fiord-dark')).toBe(false);
  await ReactTestRenderer.act(async () => {
    testRenderer.unmount();
  });
});

test('the settings panel caps at the full-state height and scrolls internally', async () => {
  const testRenderer = await renderApp();
  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'open-settings' }).props.onPress();
  });
  const panelStyle = StyleSheet.flatten(
    testRenderer.root.findByProps({ testID: 'settings-panel' }).props.style,
  );
  expect(panelStyle.maxHeight).toBe(
    Math.round(Dimensions.get('window').height * sheetFullHeightFraction),
  );
  // The content column scrolls inside the cap, and the pinned header keeps
  // the title and its Close action outside that scroller.
  expect(
    testRenderer.root.findByProps({ testID: 'settings-scroll' }),
  ).toBeDefined();
  expect(testRenderer.root.findByProps({ testID: 'settings-done' })).toBeDefined();
});

test('the settings sheet renders one capability row per SDK readback entry', async () => {
  const sdk = await MapsSdk.create();
  (sdk.providerRegistrations as jest.Mock).mockResolvedValue([
    {
      contract: ProviderContract.Geocoding,
      providerKey: '',
      displayName: '',
      registered: false,
    },
    {
      contract: ProviderContract.Routing,
      providerKey: 'test-router',
      displayName: 'Test Router',
      registered: true,
    },
    {
      contract: ProviderContract.RasterTiles,
      providerKey: 'test-raster',
      displayName: 'Test Raster',
      registered: true,
    },
  ]);
  const testRenderer = await renderApp();
  await settleMountEffects();
  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'open-settings' }).props.onPress();
  });
  await settleMountEffects();

  expect(
    testRenderer.root.findByProps({ testID: 'capability-provider-geocoding' }),
  ).toBeDefined();
  expect(
    testRenderer.root.findByProps({
      testID: 'capability-provider-serving-geocoding',
    }).props.children,
  ).toBe(appStrings.capabilityProviderNotConfiguredText);
  expect(
    testRenderer.root.findByProps({
      testID: 'capability-provider-serving-routing',
    }).props.children,
  ).toBe('Test Router · Ready');
  // The two tile rows are read-only and name the layer control.
  expect(
    testRenderer.root.findByProps({
      testID: 'capability-provider-readonly-raster_tiles',
    }).props.children,
  ).toBe(appStrings.capabilityProviderReadOnlyNoteText);
  // The row order follows the SDK's own readback order.
  const orderedCapabilityNames = [
    'capability-provider-geocoding',
    'capability-provider-routing',
    'capability-provider-raster_tiles',
  ];
  const renderedJson = JSON.stringify(testRenderer.toJSON());
  const renderedPositions = orderedCapabilityNames.map(capabilityTestId =>
    renderedJson.indexOf(capabilityTestId),
  );
  expect(renderedPositions).toEqual(
    [...renderedPositions].sort((leftPosition, rightPosition) => leftPosition - rightPosition),
  );
  await ReactTestRenderer.act(async () => {
    testRenderer.unmount();
  });
});

test('the routing row composes the serving provider and shows its demo endpoint', async () => {
  const sdk = await MapsSdk.create();
  (sdk.providerRegistrations as jest.Mock).mockResolvedValue([
    {
      contract: ProviderContract.Routing,
      providerKey: 'osrm',
      displayName: 'OSRM',
      registered: true,
    },
  ]);
  const osrmDescriptor = providerDescriptors.find(
    descriptor => descriptor.provider_key === 'osrm',
  );
  expect(osrmDescriptor?.demo_base_url).toBeTruthy();
  const testRenderer = await renderApp();
  await settleMountEffects();
  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'open-settings' }).props.onPress();
  });
  await settleMountEffects();

  // The serving line and every picker entry use the one composition; the row
  // never prints a bare provider name or a bare state.
  expect(
    testRenderer.root.findByProps({
      testID: 'capability-provider-serving-routing',
    }).props.children,
  ).toBe('OSRM · Ready');
  expect(
    testRenderer.root.findByProps({
      testID: 'capability-provider-choice-routing-osrm',
    }).props.children.props.children,
  ).toBe('OSRM · Ready');

  // The keyless provider's public demo address and its rate-limit note are
  // read from the registry descriptor, never spelled in the shell.
  expect(
    testRenderer.root.findByProps({
      testID: 'capability-provider-endpoint-routing',
    }).props.children,
  ).toBe(
    appStrings.settingsRoutingDemoEndpointFormat.replace(
      '{url}',
      osrmDescriptor!.demo_base_url!,
    ),
  );
  expect(
    testRenderer.root.findByProps({
      testID: 'capability-provider-endpoint-note-routing',
    }).props.children,
  ).toBe(osrmDescriptor!.demo_base_url_note);

  await ReactTestRenderer.act(async () => {
    testRenderer.unmount();
  });
});

test('selecting a keyless capability provider calls the SDK and remembers the key', async () => {
  const sdk = await MapsSdk.create();
  (sdk.providerRegistrations as jest.Mock).mockResolvedValue([
    {
      contract: ProviderContract.Routing,
      providerKey: '',
      displayName: '',
      registered: false,
    },
  ]);
  const storePreferences = jest.fn();
  mockRefappMapModuleHolder.module = createMockRefappMapModule({
    loadPreferences: jest.fn(async () => null),
    storePreferences,
    clearPreferences: jest.fn(),
  });
  const testRenderer = await renderApp();
  await settleMountEffects();
  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'open-settings' }).props.onPress();
  });
  await settleMountEffects();

  await ReactTestRenderer.act(async () => {
    testRenderer.root
      .findByProps({ testID: 'capability-provider-choice-routing-osrm' })
      .props.onPress();
  });
  await settleMountEffects();

  // A keyless routing provider is selected with the descriptor's public demo
  // address as its deployment argument, never an invented empty string.
  const osrmDemoBaseUrl =
    providerDescriptors.find(descriptor => descriptor.provider_key === 'osrm')
      ?.demo_base_url ?? null;
  if (osrmDemoBaseUrl === null) {
    // The descriptor publishes no demo address yet: the selection is refused
    // honestly and no SDK call is made.
    expect(sdk.configureProvider).not.toHaveBeenCalled();
    expect(
      testRenderer.root.findByProps({
        testID: 'capability-provider-detail-routing',
      }).props.children,
    ).toContain(appStrings.settingsProviderStateNotConfiguredText);
  } else {
    expect(sdk.configureProvider).toHaveBeenCalledWith(
      ProviderContract.Routing,
      'osrm',
      osrmDemoBaseUrl,
    );
    expect(latestStoredPreferences(storePreferences).capabilityProviderKeys).toEqual({
      routing: 'osrm',
    });
  }
  await ReactTestRenderer.act(async () => {
    testRenderer.unmount();
  });
});

test('a remembered capability provider re-applies at startup', async () => {
  const sdk = await MapsSdk.create();
  (sdk.providerRegistrations as jest.Mock).mockResolvedValue([
    {
      contract: ProviderContract.Routing,
      providerKey: '',
      displayName: '',
      registered: false,
    },
  ]);
  mockRefappMapModuleHolder.module = createMockRefappMapModule({
    loadPreferences: jest.fn(async () =>
      JSON.stringify({
        version: 1,
        themeChoice: 'auto',
        baseMap: null,
        demoPlanAssetName: null,
        selectedSceneKeys: [],
        cameraPresetId: null,
        speedMultiplier: 1,
        search: { origin: '', destination: '', lastQuery: '' },
        directionsTransportationMode: 'automobile',
        directionsRouteStrategy: 'fastest',
        capabilityProviderKeys: { routing: 'osrm' },
      }),
    ),
    storePreferences: jest.fn(),
    clearPreferences: jest.fn(),
  });
  const testRenderer = await renderApp();
  await settleMountEffects();
  await settleMountEffects();

  const osrmDemoBaseUrl =
    providerDescriptors.find(descriptor => descriptor.provider_key === 'osrm')
      ?.demo_base_url ?? null;
  if (osrmDemoBaseUrl === null) {
    expect(sdk.configureProvider).not.toHaveBeenCalled();
  } else {
    expect(sdk.configureProvider).toHaveBeenCalledWith(
      ProviderContract.Routing,
      'osrm',
      osrmDemoBaseUrl,
    );
  }
  await ReactTestRenderer.act(async () => {
    testRenderer.unmount();
  });
});

test('the plan sheet Directions default to Car and Fastest and a Walk plan sends pedestrian', async () => {
  const planRoute = jest.fn(async () => true);
  mockRefappMapModuleHolder.module = createMockRefappMapModule({
    loadPreferences: jest.fn(async () => null),
    storePreferences: jest.fn(),
    clearPreferences: jest.fn(),
    isProviderCredentialApplied: jest.fn(async () => true),
    planRoute,
  });
  const testRenderer = await renderApp();
  await settleMountEffects();
  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'set-origin-button' }).props.onPress();
  });

  const carChoice = testRenderer.root.findByProps({ testID: 'directions-mode-car' });
  expect(carChoice.props.disabled).toBe(false);
  expect(carChoice.props.accessibilityState).toEqual({ selected: true });
  expect(
    testRenderer.root.findByProps({ testID: 'directions-strategy-fastest' }).props
      .accessibilityState,
  ).toEqual({ selected: true });
  expect(
    testRenderer.root.findByProps({ testID: 'directions-strategy-row' }),
  ).toBeDefined();

  // Choosing Walk hides the strategy row: the contract applies a strategy to
  // automobile routing only.
  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'directions-mode-walk' }).props.onPress();
  });
  expect(
    testRenderer.root.findAllByProps({ testID: 'directions-strategy-row' }),
  ).toHaveLength(0);

  await ReactTestRenderer.act(async () => {
    testRenderer.root
      .findByProps({ testID: 'origin-search-field' })
      .props.onChangeText('Capitol Park');
  });
  await ReactTestRenderer.act(async () => {
    testRenderer.root
      .findByProps({ testID: 'destination-search-field' })
      .props.onChangeText('Tower Bridge');
  });
  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'plan-route' }).props.onPress();
  });
  await settleMountEffects();

  expect(planRoute).toHaveBeenCalledWith(
    TransportationMode.Pedestrian,
    RouteStrategy.Fastest,
  );
  await ReactTestRenderer.act(async () => {
    testRenderer.unmount();
  });
});

test('the Directions choices survive a relaunch through the record', async () => {
  mockRefappMapModuleHolder.module = createMockRefappMapModule({
    loadPreferences: jest.fn(async () =>
      JSON.stringify({
        version: 1,
        themeChoice: 'auto',
        baseMap: null,
        demoPlanAssetName: null,
        selectedSceneKeys: [],
        cameraPresetId: null,
        speedMultiplier: 1,
        search: { origin: '', destination: '', lastQuery: '' },
        directionsTransportationMode: 'pedestrian',
        directionsRouteStrategy: 'shortest',
        capabilityProviderKeys: {},
      }),
    ),
    storePreferences: jest.fn(),
    clearPreferences: jest.fn(),
  });
  const testRenderer = await renderApp();
  await settleMountEffects();
  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'set-origin-button' }).props.onPress();
  });

  expect(
    testRenderer.root.findByProps({ testID: 'directions-mode-walk' }).props
      .accessibilityState,
  ).toEqual({ selected: true });
  expect(
    testRenderer.root.findAllByProps({ testID: 'directions-strategy-row' }),
  ).toHaveLength(0);

  // Back to Car: the remembered Shortest choice is still selected.
  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'directions-mode-car' }).props.onPress();
  });
  expect(
    testRenderer.root.findByProps({ testID: 'directions-strategy-shortest' }).props
      .accessibilityState,
  ).toEqual({ selected: true });
  await ReactTestRenderer.act(async () => {
    testRenderer.unmount();
  });
});

test('an unsupported Directions strategy is disabled with its reason', async () => {
  const sdk = await MapsSdk.create();
  (sdk.routeCapabilities as jest.Mock).mockResolvedValue({
    supportedTransportationModes: [TransportationMode.Automobile],
    supportsShortestRouteStrategy: false,
  });
  const testRenderer = await renderApp();
  await settleMountEffects();
  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'set-origin-button' }).props.onPress();
  });
  await settleMountEffects();

  expect(
    testRenderer.root.findByProps({ testID: 'directions-strategy-shortest' }).props
      .disabled,
  ).toBe(true);
  expect(
    testRenderer.root.findByProps({ testID: 'directions-strategy-reason' }).props
      .children,
  ).toBe(appStrings.directionsShortestUnsupportedText);
  expect(
    testRenderer.root.findByProps({ testID: 'directions-mode-walk' }).props.disabled,
  ).toBe(true);
  expect(
    testRenderer.root.findByProps({ testID: 'directions-mode-reason' }).props.children,
  ).toBe(appStrings.directionsWalkUnsupportedText);
  await ReactTestRenderer.act(async () => {
    testRenderer.unmount();
  });
});

test('the reset row clears the record and returns the shown choices to defaults', async () => {
  const clearPreferences = jest.fn();
  const storePreferences = jest.fn();
  const clearStoredProviderCredential = jest.fn();
  mockRefappMapModuleHolder.module = createMockRefappMapModule({
    loadPreferences: jest.fn(async () =>
      JSON.stringify({
        version: 1,
        themeChoice: 'dark',
        baseMap: null,
        demoPlanAssetName: null,
        selectedSceneKeys: [],
        cameraPresetId: cameraPresets[1].id,
        speedMultiplier: 4,
        search: {
          origin: demoPlaces[0].name,
          destination: demoPlaces[1].name,
          lastQuery: demoPlaces[1].name,
        },
      }),
    ),
    storePreferences,
    clearPreferences,
    clearStoredProviderCredential,
    clearSceneOverlays: jest.fn(),
    selectTraceReplayPlan: jest.fn(async () => true),
    selectTraceReplaySpeedMultiplier: jest.fn(),
  });
  const testRenderer = await renderApp();
  await settleMountEffects();
  expect(storePreferences).not.toHaveBeenCalled();

  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'open-settings' }).props.onPress();
  });
  await ReactTestRenderer.act(async () => {
    testRenderer.root
      .findByProps({ testID: 'settings-reset-preferences' })
      .props.onPress();
  });
  await settleMountEffects();

  // The native record is cleared, and the credential store is not touched.
  expect(clearPreferences).toHaveBeenCalledTimes(1);
  expect(clearStoredProviderCredential).not.toHaveBeenCalled();
  expect(storePreferences).not.toHaveBeenCalled();
  expect(JSON.stringify(testRenderer.toJSON())).toContain(
    themeChoiceSwitchLabel(defaultThemeChoice),
  );

  // The fields the reset showed are empty again.
  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'settings-done' }).props.onPress();
  });
  await ReactTestRenderer.act(async () => {
    testRenderer.root.findByProps({ testID: 'set-origin-button' }).props.onPress();
  });
  expect(
    testRenderer.root.findByProps({ testID: 'origin-search-field' }).props.value,
  ).toBe('');
  expect(
    testRenderer.root.findByProps({ testID: 'destination-search-field' }).props.value,
  ).toBe('');
});