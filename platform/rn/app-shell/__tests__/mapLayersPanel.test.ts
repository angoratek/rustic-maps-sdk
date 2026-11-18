/**
 * The on-map layer panel's pure state machine
 * (docs/ReferenceAppUX.md §4.2): the section order, the base rows'
 * serving/refusal marks, the traffic switch's availability and remembered
 * restore, the scene switches, the device-location row and the camera-bounds
 * answer the traffic layer queries. No provider name is hardcoded: every
 * fixture below is a hand-built descriptor, standing in for the shared
 * registry artifact.
 */

import type { BaseMapChoice, VectorBaseMapChoice } from '../src/baseMap';
import type { ProviderCredentialDescriptor } from '../src/providerRegistry';
import {
  baseRowAccessibilityLabel,
  baseRowStatusText,
  buildMapLayerBaseRows,
  buildMapLayerSceneRows,
  deviceLocationCannotStartReasonText,
  deviceLocationPanelStatusText,
  geographicBoundsForCamera,
  initialMapLayersPanelState,
  isDeviceLocationSourceRunning,
  isNonDefaultLayerActive,
  isTrafficLayerAvailable,
  mapLayerBaseRowIdentifierPrefix,
  mapLayersPanelReducer,
  mapLayersPanelSectionTitles,
  mapLayerStyleRowIdentifierPrefix,
  shouldRestoreTrafficLayer,
  trafficLayerAttributionText,
  trafficLayerAvailability,
  trafficLayerReasonText,
} from '../src/mapLayersPanelState';
import { appStrings } from '../src/theme';

/** A registry descriptor with only the fields the panel reads. */
function descriptor(
  overrides: Partial<ProviderCredentialDescriptor> &
    Pick<ProviderCredentialDescriptor, 'provider_key' | 'display_name'>,
): ProviderCredentialDescriptor {
  return {
    attribution_text: overrides.display_name,
    settings_hint: '',
    registration_available: {
      android: true,
      ios: true,
      react_native: true,
      wasm: true,
    },
    served_contracts: [],
    ...overrides,
  };
}

const credentiallessRasterDescriptor = descriptor({
  provider_key: 'open-raster',
  display_name: 'Open Raster',
  served_contracts: ['raster_tiles'],
  raster_url_template: 'https://tiles.invalid/{z}/{x}/{y}.png',
});

const keyedRasterDescriptor = descriptor({
  provider_key: 'keyed-base',
  display_name: 'Keyed Base',
  credential_label: 'API key',
  served_contracts: ['raster_tiles'],
});

const vectorDescriptor = descriptor({
  provider_key: 'vector-base',
  display_name: 'Vector Base',
  served_contracts: ['vector_tiles'],
  url_template: 'https://vector.invalid/{z}/{x}/{y}',
  vector_styles: [
    {
      style_id: 'bright-light',
      display_name: 'Bright Light',
      theme: 'light',
      attribution_text: 'Vector Base',
    },
  ],
});

const trafficDescriptor = descriptor({
  provider_key: 'traffic-source',
  display_name: 'Traffic Source',
  credential_label: 'API key',
  served_contracts: ['traffic_flow'],
  attribution_text: 'Traffic data by Traffic Source',
});

const rasterChoice = (choiceDescriptor: ProviderCredentialDescriptor, isServable: boolean): BaseMapChoice => ({
  descriptor: choiceDescriptor,
  isServable,
});

const vectorChoice = (
  choiceDescriptor: ProviderCredentialDescriptor,
  isServable = true,
): VectorBaseMapChoice => ({
  descriptor: choiceDescriptor,
  style: choiceDescriptor.vector_styles![0],
  isServable,
});

test('the panel sections keep the specification order and wording', () => {
  expect([...mapLayersPanelSectionTitles]).toEqual([
    'Base map',
    'Traffic',
    'Overlays',
    'Device location',
  ]);
});

test('base rows list raster descriptors first, then every committed style', () => {
  const rows = buildMapLayerBaseRows(
    [
      rasterChoice(credentiallessRasterDescriptor, true),
      rasterChoice(keyedRasterDescriptor, false),
    ],
    [vectorChoice(vectorDescriptor)],
    'open-raster',
    null,
  );

  expect(rows.map(row => row.identifier)).toEqual([
    mapLayerBaseRowIdentifierPrefix + 'open-raster',
    mapLayerBaseRowIdentifierPrefix + 'keyed-base',
    mapLayerStyleRowIdentifierPrefix + 'bright-light',
  ]);
  expect(rows.map(row => row.displayName)).toEqual([
    'Open Raster',
    'Keyed Base',
    'Vector Base · Bright Light',
  ]);
  expect(rows[0].isServing).toBe(true);
  expect(baseRowStatusText(rows[0])).toBe(appStrings.baseMapActiveStatusText);
  expect(baseRowAccessibilityLabel(rows[0])).toBe('Open Raster — Serving');
  expect(rows[1].isServing).toBe(false);
  expect(rows[1].disabledReasonText).toBe('Needs API key');
  expect(baseRowStatusText(rows[1])).toBe('Needs API key');
  expect(rows[2].disabledReasonText).toBeNull();
});

test('a vector row selects its provider/style and becomes the serving row', () => {
  const rows = buildMapLayerBaseRows(
    [rasterChoice(credentiallessRasterDescriptor, true)],
    [vectorChoice(vectorDescriptor)],
    'open-raster',
    'vector-base/bright-light',
  );

  expect(rows[1].selection).toEqual({
    kind: 'vector',
    choiceKey: 'vector-base/bright-light',
  });
  expect(rows[1].isServing).toBe(true);
  expect(rows[0].isServing).toBe(false);
  expect(baseRowStatusText(rows[1])).toBe(appStrings.baseMapActiveStatusText);
});

test('a provider-published vector family renders one selectable row', () => {
  // With its credential applied the row installs; a family still waiting for
  // its key states what it needs instead (the next test).
  const hostedDescriptor = descriptor({
    provider_key: 'hosted-vector-base',
    display_name: 'Hosted Vector Base',
    credential_label: 'Access token',
    served_contracts: ['vector_tiles'],
    vector_styles: [
      {
        style_id: 'streets-light',
        display_name: 'Streets',
        theme: 'light',
        attribution_text: 'Hosted Vector Base',
        document_url: 'https://styles.invalid/v1/streets',
      },
    ],
  });
  const rows = buildMapLayerBaseRows(
    [],
    [vectorChoice(hostedDescriptor)],
    null,
    null,
  );
  expect(rows).toHaveLength(1);
  expect(rows[0].identifier).toBe(
    mapLayerStyleRowIdentifierPrefix + 'streets-light',
  );
  expect(rows[0].displayName).toBe('Hosted Vector Base · Streets');
  expect(rows[0].disabledReasonText).toBeNull();
});

test('a provider-published vector family without its key states what it needs', () => {
  const hostedDescriptor = descriptor({
    provider_key: 'hosted-vector-base',
    display_name: 'Hosted Vector Base',
    credential_label: 'Access token',
    served_contracts: ['vector_tiles'],
    vector_styles: [
      {
        style_id: 'streets-light',
        display_name: 'Streets',
        theme: 'light',
        attribution_text: 'Hosted Vector Base',
        document_url: 'https://styles.invalid/v1/streets',
      },
    ],
  });
  const rows = buildMapLayerBaseRows(
    [],
    [vectorChoice(hostedDescriptor, false)],
    null,
    null,
  );
  expect(rows).toHaveLength(1);
  expect(rows[0].disabledReasonText).toBe('Needs Access token');
  expect(baseRowStatusText(rows[0])).toBe('Needs Access token');
});

test('the traffic switch states needs-credential, available and not-configured', () => {
  // No descriptor declares the capability: the shared "Not configured" copy.
  expect(trafficLayerAvailability([], new Set<string>())).toEqual({
    state: 'notConfigured',
    providerKey: null,
    reasonText: appStrings.capabilityProviderNotConfiguredText,
  });
  expect(appStrings.capabilityProviderNotConfiguredText).toBe('Not configured');

  // A declaration whose credential is not applied names what it needs.
  const needsCredential = trafficLayerAvailability(
    [trafficDescriptor],
    new Set<string>(),
  );
  expect(needsCredential).toEqual({
    state: 'needsCredential',
    providerKey: 'traffic-source',
    reasonText: 'Needs API key',
  });
  expect(isTrafficLayerAvailable(needsCredential)).toBe(false);
  expect(trafficLayerReasonText(needsCredential)).toBe('Needs API key');
  expect(trafficLayerAttributionText(needsCredential)).toBeNull();

  // Once the key is applied the serving descriptor (and its attribution) answer.
  const available = trafficLayerAvailability(
    [trafficDescriptor],
    new Set(['traffic-source']),
  );
  expect(available).toEqual({
    state: 'available',
    providerKey: 'traffic-source',
    attributionText: 'Traffic data by Traffic Source',
  });
  expect(isTrafficLayerAvailable(available)).toBe(true);
  expect(trafficLayerReasonText(available)).toBeNull();
  expect(trafficLayerAttributionText(available)).toBe(
    'Traffic data by Traffic Source',
  );
});

test('a descriptor with no React Native registration does not claim traffic', () => {
  const unavailableDescriptor = descriptor({
    provider_key: 'traffic-source',
    display_name: 'Traffic Source',
    served_contracts: ['traffic_flow'],
    registration_available: {
      android: true,
      ios: true,
      react_native: false,
      wasm: false,
    },
  });
  expect(
    trafficLayerAvailability([unavailableDescriptor], new Set<string>()).state,
  ).toBe('notConfigured');
});

test('the remembered traffic choice restores only when the provider can honour it', () => {
  const available = trafficLayerAvailability(
    [trafficDescriptor],
    new Set(['traffic-source']),
  );
  const needsCredential = trafficLayerAvailability(
    [trafficDescriptor],
    new Set<string>(),
  );

  expect(shouldRestoreTrafficLayer(true, available)).toBe(true);
  expect(shouldRestoreTrafficLayer(false, available)).toBe(false);
  expect(shouldRestoreTrafficLayer(true, needsCredential)).toBe(false);
  expect(shouldRestoreTrafficLayer(true, trafficLayerAvailability([], new Set<string>()))).toBe(
    false,
  );
});

test('scenes build one switch per shared scene with stable identifiers', () => {
  const rows = buildMapLayerSceneRows(
    [
      { sceneKey: 'route', label: 'Route' },
      { sceneKey: 'polygon', label: 'Area' },
    ],
    ['polygon'],
  );
  expect(rows).toEqual([
    {
      sceneKey: 'route',
      label: 'Route',
      isEnabled: false,
      identifier: 'map-layer-scene-route',
    },
    {
      sceneKey: 'polygon',
      label: 'Area',
      isEnabled: true,
      identifier: 'map-layer-scene-polygon',
    },
  ]);
});

test('the active dot follows the default base, traffic and scenes', () => {
  expect(
    isNonDefaultLayerActive({
      isDefaultBaseServing: true,
      isTrafficEnabled: false,
      enabledSceneCount: 0,
    }),
  ).toBe(false);
  expect(
    isNonDefaultLayerActive({
      isDefaultBaseServing: false,
      isTrafficEnabled: false,
      enabledSceneCount: 0,
    }),
  ).toBe(true);
  expect(
    isNonDefaultLayerActive({
      isDefaultBaseServing: true,
      isTrafficEnabled: true,
      enabledSceneCount: 0,
    }),
  ).toBe(true);
  expect(
    isNonDefaultLayerActive({
      isDefaultBaseServing: true,
      isTrafficEnabled: false,
      enabledSceneCount: 1,
    }),
  ).toBe(true);
});

test('the device-location row renders its state and stop states', () => {
  expect(deviceLocationPanelStatusText('off')).toBe('Off');
  expect(deviceLocationPanelStatusText('waiting')).toBe('Waiting for a fix');
  expect(deviceLocationPanelStatusText('active')).toBe('Active');
  expect(deviceLocationPanelStatusText('permissionDenied')).toBe(
    'Permission denied',
  );
  expect(deviceLocationPanelStatusText('unavailable')).toBe('Unavailable');

  expect(isDeviceLocationSourceRunning('waiting')).toBe(true);
  expect(isDeviceLocationSourceRunning('active')).toBe(true);
  expect(isDeviceLocationSourceRunning('off')).toBe(false);
  expect(deviceLocationCannotStartReasonText('permissionDenied')).toBe(
    'Permission denied',
  );
  expect(deviceLocationCannotStartReasonText('active')).toBeNull();
});

test('the panel opens and closes through its state machine', () => {
  const closed = initialMapLayersPanelState();
  expect(closed.isOpen).toBe(false);

  const opened = mapLayersPanelReducer(closed, { type: 'panelOpened' });
  expect(opened.isOpen).toBe(true);

  const closedAgain = mapLayersPanelReducer(opened, { type: 'panelClosed' });
  expect(closedAgain.isOpen).toBe(false);
});

test('a traffic turn-on the registry cannot serve states its reason and stays off', () => {
  const state = mapLayersPanelReducer(initialMapLayersPanelState(), {
    type: 'trafficConfigurationUpdated',
    availability: trafficLayerAvailability([trafficDescriptor], new Set<string>()),
  });
  const afterToggle = mapLayersPanelReducer(state, {
    type: 'trafficToggled',
    isEnabled: true,
  });

  expect(afterToggle.isTrafficEnabled).toBe(false);
  expect(afterToggle.statusLineText).toBe('Needs API key');
});

test('a provider-not-configured refresh turns the switch off with its reason', () => {
  const available = trafficLayerAvailability(
    [trafficDescriptor],
    new Set(['traffic-source']),
  );
  const enabled = mapLayersPanelReducer(
    mapLayersPanelReducer(initialMapLayersPanelState(), {
      type: 'trafficConfigurationUpdated',
      availability: available,
    }),
    { type: 'trafficToggled', isEnabled: true },
  );
  expect(enabled.isTrafficEnabled).toBe(true);

  const failed = mapLayersPanelReducer(enabled, {
    type: 'trafficRefreshFailed',
    detailText: 'provider not configured',
    isProviderNotConfigured: true,
  });
  expect(failed.isTrafficEnabled).toBe(false);
  expect(failed.isTrafficRefreshing).toBe(false);
  expect(failed.statusLineText).toBe('provider not configured');
});

test('a configuration change that drops the serving provider turns traffic off', () => {
  const available = trafficLayerAvailability(
    [trafficDescriptor],
    new Set(['traffic-source']),
  );
  const enabled = mapLayersPanelReducer(
    mapLayersPanelReducer(initialMapLayersPanelState(), {
      type: 'trafficConfigurationUpdated',
      availability: available,
    }),
    { type: 'trafficToggled', isEnabled: true },
  );
  const reconfigured = mapLayersPanelReducer(enabled, {
    type: 'trafficConfigurationUpdated',
    availability: trafficLayerAvailability([], new Set<string>()),
  });
  expect(reconfigured.isTrafficEnabled).toBe(false);
});

test('the camera bounds contain the camera center and clamp the world edges', () => {
  const worldBounds = geographicBoundsForCamera(
    { latitudeDegrees: 0, longitudeDegrees: 0, zoomLevel: 0 },
    256,
    256,
  );
  expect(worldBounds).not.toBeNull();
  expect(worldBounds!.southWest.longitude).toBeCloseTo(-180, 6);
  expect(worldBounds!.northEast.longitude).toBeCloseTo(180, 6);
  expect(worldBounds!.southWest.latitude).toBeCloseTo(-85.05112878, 5);
  expect(worldBounds!.northEast.latitude).toBeCloseTo(85.05112878, 5);

  const localBounds = geographicBoundsForCamera(
    { latitudeDegrees: 38.58, longitudeDegrees: -121.49, zoomLevel: 14 },
    1080,
    1920,
  );
  expect(localBounds).not.toBeNull();
  expect(localBounds!.southWest.latitude).toBeLessThan(38.58);
  expect(localBounds!.northEast.latitude).toBeGreaterThan(38.58);
  expect(localBounds!.southWest.longitude).toBeLessThan(-121.49);
  expect(localBounds!.northEast.longitude).toBeGreaterThan(-121.49);

  expect(
    geographicBoundsForCamera(
      { latitudeDegrees: 0, longitudeDegrees: 0, zoomLevel: 0 },
      0,
      0,
    ),
  ).toBeNull();
  expect(
    geographicBoundsForCamera(
      { latitudeDegrees: Number.NaN, longitudeDegrees: 0, zoomLevel: 0 },
      100,
      100,
    ),
  ).toBeNull();
});
