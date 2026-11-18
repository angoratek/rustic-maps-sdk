/**
 * The offline card's route control (§2.5 `offline.plan_route_action`): the
 * shell hands the tap to the native host, whose plan-state event carries the
 * offline archive route back. Jest has no native side, so the native seam is
 * mocked at its module boundary — exactly as `App.test.tsx` mocks the SDK —
 * and the assertion reads the route summary the shell renders: the §2.5
 * title, the distance and duration the plan carried, and the honesty note
 * that the route is recorded test data, not live data.
 */

import React from 'react';
import ReactTestRenderer from 'react-test-renderer';
import { DeviceEventEmitter } from 'react-native';

// The generated bindings' package entry is imported for its compiled-version
// probe; the SDK proxy and the native view are mocked at their deep entry
// points (the jest environment has no TurboModule or native view manager).
jest.mock('maps-sdk-rn', () => ({
  getSdkCompiledVersion: () => 'jest-stub-version',
}));

jest.mock('maps-sdk-rn/MapView', () => {
  const React = require('react');
  const { View } = require('react-native');
  const MockMapView = React.forwardRef((props: Record<string, unknown>, ref: unknown) =>
    React.createElement(View, { ...props, ref }),
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
    configureProvider: async () => undefined,
    providerRegistrations: async () => [],
    routeCapabilities: async () => ({
      supportedTransportationModes: [0, 2],
      supportsShortestRouteStrategy: true,
    }),
    registerUrlTemplateRasterTileSource: async () => ({
      isApplied: false,
      detailText: '',
    }),
    restoreCredentialedBaseMap: async () => ({
      isApplied: false,
      detailText: '',
    }),
    search: async () => [],
    suggest: async () => [],
    forwardGeocode: async () => [],
    reverseGeocode: async () => [],
    trafficFlowCacheStatistics: async () => null,
    tileCacheStatistics: async () => null,
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

jest.mock('react-native-safe-area-context', () => {
  const React = require('react');
  return {
    SafeAreaProvider: (props: { children?: React.ReactNode }) =>
      React.createElement(React.Fragment, null, props.children ?? null),
    useSafeAreaInsets: () => ({ top: 0, bottom: 0, left: 0, right: 0 }),
  };
});

// The native seam: the offline route control resolves, then the host's own
// plan-state event carries the route back to the shell as a live plan does.
jest.mock('../src/RefappNative', () => {
  const actual = jest.requireActual('../src/RefappNative');
  const { DeviceEventEmitter: deviceEmitter } = require('react-native');
  const planOfflineRouteFromArchive = jest.fn(async () => {
    deviceEmitter.emit('RefappPlanState', {
      isPlanned: true,
      isInFlight: false,
      titleText: 'Offline demo route',
      distanceText: '222 m',
      durationText: '27 s',
      isRecordedDemoRoute: false,
      isOfflineArchiveRoute: true,
    });
    return true;
  });
  return {
    ...actual,
    refappMapModule: {
      ...(actual.refappMapModule ?? {}),
      // The mount path's one SDK and speech-engine reads: the route control
      // under test is independent of them, so they answer honestly without a
      // native side.
      createSdk: () => undefined,
      isProviderCredentialApplied: async () => false,
      getStoredProviderCredential: async () => null,
      applyCameraPreset: () => undefined,
      planOfflineRouteFromArchive,
    },
    hasNativeController: () => true,
    addControllerListener: (
      eventName: string,
      handler: (payload: Record<string, unknown>) => void,
    ) => {
      const subscription = deviceEmitter.addListener(eventName, handler);
      return () => subscription.remove();
    },
  };
});

import App from '../App';
import { appStrings } from '../src/theme';

test('the offline control plans over the bundled test graph and renders its route', async () => {
  let testRenderer: ReactTestRenderer.ReactTestRenderer;
  await ReactTestRenderer.act(() => {
    testRenderer = ReactTestRenderer.create(<App />);
  });
  // Let the mount's provider-credential promise settle inside act.
  await ReactTestRenderer.act(async () => {});

  // The offline card's route control is the user path under test.
  const offlineRouteButton = testRenderer!.root.findByProps({
    testID: 'offline-route-button',
  });
  expect(offlineRouteButton).toBeDefined();
  await ReactTestRenderer.act(async () => {
    offlineRouteButton.props.onPress();
  });

  // The route summary renders the plan the archive answered: the §2.5 title,
  // the distance and duration, and the recorded-test-graph honesty note.
  testRenderer!.root.findByProps({ testID: 'route-summary-card' });
  const renderedText = JSON.stringify(testRenderer!.toJSON());
  expect(renderedText).toContain(appStrings.offlineRouteTitleText);
  expect(renderedText).toContain('222 m');
  expect(renderedText).toContain('27 s');
  const offlineNote = testRenderer!.root.findByProps({
    testID: 'route-summary-offline-note',
  });
  expect(offlineNote.props.children).toBe(appStrings.offlineRouteNoteText);
});
