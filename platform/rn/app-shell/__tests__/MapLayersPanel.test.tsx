/**
 * The layer panel's render contract (docs/ReferenceAppUX.md §4.2):
 * the collapsed control's accessible name and dot, the panel's section order,
 * the serving row mark, the traffic reason copy, the scene and device-location
 * switches, and the close path (button, close button, Escape/back through the
 * modal's request-close) returning focus to the control.
 */

import React from 'react';
import ReactTestRenderer from 'react-test-renderer';
import { Modal, Text } from 'react-native';

import {
  MapLayersControl,
  MapLayersPanel,
  type MapLayersPanelProps,
} from '../src/MapLayersPanel';
import {
  buildMapLayerBaseRows,
  buildMapLayerSceneRows,
  mapLayersActiveDotIdentifier,
  mapLayerTrafficIdentifier,
  mapLayersCloseIdentifier,
  mapLayersControlIdentifier,
  mapLayersPanelSectionTitles,
  trafficLayerAvailability,
} from '../src/mapLayersPanelState';
import { baseMapChoices, vectorBaseMapChoices } from '../src/baseMap';
import type { ProviderCredentialDescriptor } from '../src/providerRegistry';
import { buildScreenStyles } from '../src/screenStyles';
import { themeColorsByThemeName } from '../src/theme';

const styles = buildScreenStyles(themeColorsByThemeName.dark);

const rasterDescriptor: ProviderCredentialDescriptor = {
  provider_key: 'open-raster',
  display_name: 'Open Raster',
  served_contracts: ['raster_tiles'],
  registration_available: {
    android: true,
    ios: true,
    react_native: true,
    wasm: true,
  },
  attribution_text: 'Open Raster',
  settings_hint: '',
  raster_url_template: 'https://tiles.invalid/{z}/{x}/{y}.png',
};

const vectorDescriptor: ProviderCredentialDescriptor = {
  provider_key: 'vector-base',
  display_name: 'Vector Base',
  served_contracts: ['vector_tiles'],
  registration_available: {
    android: true,
    ios: true,
    react_native: true,
    wasm: true,
  },
  attribution_text: 'Vector Base',
  settings_hint: '',
  url_template: 'https://vector.invalid/{z}/{x}/{y}',
  vector_styles: [
    {
      style_id: 'bright-light',
      display_name: 'Bright Light',
      theme: 'light',
      attribution_text: 'Vector Base',
    },
  ],
};

const trafficDescriptor: ProviderCredentialDescriptor = {
  provider_key: 'traffic-source',
  display_name: 'Traffic Source',
  credential_label: 'API key',
  served_contracts: ['traffic_flow'],
  registration_available: {
    android: true,
    ios: true,
    react_native: true,
    wasm: true,
  },
  attribution_text: 'Traffic data by Traffic Source',
  settings_hint: '',
};

const baseRows = buildMapLayerBaseRows(
  baseMapChoices([rasterDescriptor], new Set<string>()),
  vectorBaseMapChoices([vectorDescriptor]),
  rasterDescriptor.provider_key,
  null,
);

const sceneRows = buildMapLayerSceneRows(
  [
    { sceneKey: 'route', label: 'Route' },
    { sceneKey: 'polygon', label: 'Area' },
  ],
  ['route'],
);

function panelProps(
  overrides: Partial<MapLayersPanelProps> = {},
): MapLayersPanelProps {
  return {
    baseRows,
    onSelectBaseRow: jest.fn(),
    trafficAvailability: trafficLayerAvailability([trafficDescriptor], new Set<string>()),
    isTrafficEnabled: false,
    onToggleTraffic: jest.fn(),
    sceneRows,
    areSceneOverlaysAvailable: true,
    sceneOverlaysUnavailableReasonText: null,
    onToggleScene: jest.fn(),
    onClearScenes: jest.fn(),
    locationStatus: 'active',
    onStopDeviceLocation: jest.fn(),
    statusLineText: null,
    bottomInset: 0,
    onClose: jest.fn(),
    onReturnFocus: jest.fn(),
    styles,
    ...overrides,
  };
}

test('the collapsed control carries the accessible name and its dot', async () => {
  let testRenderer: ReactTestRenderer.ReactTestRenderer;
  await ReactTestRenderer.act(() => {
    testRenderer = ReactTestRenderer.create(
      <MapLayersControl
        isNonDefaultLayerActive
        onPress={jest.fn()}
        styles={styles}
      />,
    );
  });

  const control = testRenderer!.root.findByProps({
    testID: mapLayersControlIdentifier,
  });
  expect(control.props.accessibilityLabel).toBe('Map layers');
  expect(control.props.accessibilityValue).toEqual({
    text: 'Non-default layer active',
  });
  expect(
    testRenderer!.root.findAllByProps({
      testID: mapLayersActiveDotIdentifier,
    }).length,
  ).toBeGreaterThan(0);

  let inactiveRenderer: ReactTestRenderer.ReactTestRenderer;
  await ReactTestRenderer.act(() => {
    inactiveRenderer = ReactTestRenderer.create(
      <MapLayersControl
        isNonDefaultLayerActive={false}
        onPress={jest.fn()}
        styles={styles}
      />,
    );
  });
  expect(
    inactiveRenderer!.root.findAllByProps({
      testID: mapLayersActiveDotIdentifier,
    }),
  ).toHaveLength(0);
});

test('the panel renders its sections in the specification order', async () => {
  let testRenderer: ReactTestRenderer.ReactTestRenderer;
  await ReactTestRenderer.act(() => {
    testRenderer = ReactTestRenderer.create(<MapLayersPanel {...panelProps()} />);
  });

  const sectionTitles = testRenderer!.root
    .findAllByType(Text)
    .map(textNode => textNode.props.children)
    .filter(
      (child): child is string =>
        typeof child === 'string' &&
        (mapLayersPanelSectionTitles as readonly string[]).includes(child),
    );
  const firstOccurrences = mapLayersPanelSectionTitles.map(title =>
    sectionTitles.indexOf(title),
  );
  expect(firstOccurrences).toEqual([...firstOccurrences].sort((a, b) => a - b));
  expect(firstOccurrences.every(index => index >= 0)).toBe(true);
});

test('the serving row is marked and the unapplied traffic switch names its key', async () => {
  let testRenderer: ReactTestRenderer.ReactTestRenderer;
  await ReactTestRenderer.act(() => {
    testRenderer = ReactTestRenderer.create(<MapLayersPanel {...panelProps()} />);
  });

  const servingRow = testRenderer!.root.findByProps({
    testID: 'map-layer-base-open-raster',
  });
  expect(servingRow.props.accessibilityLabel).toBe('Open Raster — Serving');
  expect(servingRow.props.accessibilityState).toEqual({
    disabled: false,
    selected: true,
  });

  const trafficSwitch = testRenderer!.root.findByProps({
    testID: mapLayerTrafficIdentifier,
  });
  expect(trafficSwitch.props.disabled).toBe(true);
  expect(trafficSwitch.props.accessibilityState).toEqual({
    checked: false,
    disabled: true,
  });
  expect(
    testRenderer!.root.findByProps({ testID: 'map-layer-traffic-reason' }).props
      .children,
  ).toBe('Needs API key');
});

test('the overlays section offers the shared clear action', async () => {
  const onClearScenes = jest.fn();
  let testRenderer: ReactTestRenderer.ReactTestRenderer;
  await ReactTestRenderer.act(() => {
    testRenderer = ReactTestRenderer.create(
      <MapLayersPanel {...panelProps({ onClearScenes })} />,
    );
  });

  const clearAction = testRenderer!.root.findByProps({
    testID: 'map-layer-clear-overlays',
  });
  expect(clearAction.props.accessibilityRole).toBe('button');
  await ReactTestRenderer.act(() => {
    clearAction.props.onPress();
  });
  expect(onClearScenes).toHaveBeenCalledTimes(1);
});

test('the device-location row shows its state and Stop while the source runs', async () => {
  const onStopDeviceLocation = jest.fn();
  let testRenderer: ReactTestRenderer.ReactTestRenderer;
  await ReactTestRenderer.act(() => {
    testRenderer = ReactTestRenderer.create(
      <MapLayersPanel {...panelProps({ onStopDeviceLocation })} />,
    );
  });

  expect(
    testRenderer!.root.findByProps({ testID: 'device-location-status' }).props
      .children,
  ).toBe('Active');
  await ReactTestRenderer.act(() => {
    testRenderer!.root.findByProps({ testID: 'device-location-stop' }).props
      .onPress();
  });
  expect(onStopDeviceLocation).toHaveBeenCalledTimes(1);
});

test('the close button and the modal request-close both close and return focus', async () => {
  const onClose = jest.fn();
  const onReturnFocus = jest.fn();
  let testRenderer: ReactTestRenderer.ReactTestRenderer;
  await ReactTestRenderer.act(() => {
    testRenderer = ReactTestRenderer.create(
      <MapLayersPanel
        {...panelProps({ onClose, onReturnFocus })}
      />,
    );
  });

  await ReactTestRenderer.act(() => {
    testRenderer!.root.findByProps({ testID: mapLayersCloseIdentifier }).props
      .onPress();
  });
  expect(onClose).toHaveBeenCalledTimes(1);
  expect(onReturnFocus).toHaveBeenCalledTimes(1);

  await ReactTestRenderer.act(() => {
    testRenderer!.root.findByType(Modal).props.onRequestClose();
  });
  expect(onClose).toHaveBeenCalledTimes(2);
  expect(onReturnFocus).toHaveBeenCalledTimes(2);
});

test('the scene switches mirror the shared scene rows', async () => {
  let testRenderer: ReactTestRenderer.ReactTestRenderer;
  await ReactTestRenderer.act(() => {
    testRenderer = ReactTestRenderer.create(<MapLayersPanel {...panelProps()} />);
  });

  const routeSwitch = testRenderer!.root.findByProps({
    testID: 'map-layer-scene-route',
  });
  expect(routeSwitch.props.accessibilityState).toEqual({
    checked: true,
    disabled: false,
  });
  expect(
    testRenderer!.root.findByProps({ testID: 'map-layer-scene-polygon' }).props
      .accessibilityState,
  ).toEqual({ checked: false, disabled: false });
});
