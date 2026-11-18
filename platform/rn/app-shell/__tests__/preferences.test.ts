/**
 * The remembered-choices record: defaults, a complete versioned record,
 * unknown keys, incompatible versions and invalid field values. The record
 * carries only explicit choices; transient state and credentials have no
 * field here.
 */

import {
  defaultReferenceAppPreferences,
  referenceAppPreferencesFromStoredValue,
  referenceAppPreferencesVersion,
  simulatedTripSpeedMultiplierChoices,
} from '../src/preferences';

test('a device that never chose starts from the record defaults', () => {
  expect(defaultReferenceAppPreferences()).toEqual({
    version: referenceAppPreferencesVersion,
    themeChoice: 'auto',
    baseMap: null,
    demoPlanAssetName: null,
    selectedSceneKeys: [],
    trafficLayerEnabled: false,
    cameraPresetId: null,
    speedMultiplier: simulatedTripSpeedMultiplierChoices[0],
    search: { origin: '', destination: '', lastQuery: '' },
    directionsTransportationMode: 'automobile',
    directionsRouteStrategy: 'fastest',
    capabilityProviderKeys: {},
  });
  expect(referenceAppPreferencesFromStoredValue(null)).toBeNull();
  expect(referenceAppPreferencesFromStoredValue(undefined)).toBeNull();
  expect(referenceAppPreferencesFromStoredValue('')).toBeNull();
});

test('a complete versioned record restores every remembered choice', () => {
  const restoredPreferences = referenceAppPreferencesFromStoredValue(
    JSON.stringify({
      version: referenceAppPreferencesVersion,
      themeChoice: 'dark',
      baseMap: { providerKey: 'openfreemap', styleId: 'bright-light' },
      demoPlanAssetName: 'sacramento_clean_walk.json',
      selectedSceneKeys: ['route', 'traffic'],
      trafficLayerEnabled: true,
      cameraPresetId: 'pitched-view',
      speedMultiplier: 4,
      search: {
        origin: 'Capitol Park',
        destination: 'Tower Bridge',
        lastQuery: 'Tower',
      },
      directionsTransportationMode: 'pedestrian',
      directionsRouteStrategy: 'shortest',
      capabilityProviderKeys: { routing: 'osrm' },
    }),
  );
  expect(restoredPreferences).toEqual({
    version: referenceAppPreferencesVersion,
    themeChoice: 'dark',
    baseMap: { providerKey: 'openfreemap', styleId: 'bright-light' },
    demoPlanAssetName: 'sacramento_clean_walk.json',
    selectedSceneKeys: ['route', 'traffic'],
    trafficLayerEnabled: true,
    cameraPresetId: 'pitched-view',
    speedMultiplier: 4,
    search: {
      origin: 'Capitol Park',
      destination: 'Tower Bridge',
      lastQuery: 'Tower',
    },
    directionsTransportationMode: 'pedestrian',
    directionsRouteStrategy: 'shortest',
    capabilityProviderKeys: { routing: 'osrm' },
  });
});

test('a record from before the Directions and capability fields restores their defaults', () => {
  const restoredPreferences = referenceAppPreferencesFromStoredValue(
    JSON.stringify({
      version: referenceAppPreferencesVersion,
      themeChoice: 'dark',
      search: { origin: 'Capitol Park' },
    }),
  );
  expect(restoredPreferences?.directionsTransportationMode).toBe('automobile');
  expect(restoredPreferences?.directionsRouteStrategy).toBe('fastest');
  expect(restoredPreferences?.capabilityProviderKeys).toEqual({});
  expect(restoredPreferences?.trafficLayerEnabled).toBe(false);
});

test('an unknown Directions name falls back and a malformed capability map is dropped', () => {
  const restoredPreferences = referenceAppPreferencesFromStoredValue(
    JSON.stringify({
      version: referenceAppPreferencesVersion,
      directionsTransportationMode: 'hovercraft',
      directionsRouteStrategy: 'scenic',
      capabilityProviderKeys: {
        routing: 'osrm',
        geocoding: '',
        '  ': 'mapquest',
        traffic_flow: 7,
      },
    }),
  );
  expect(restoredPreferences?.directionsTransportationMode).toBe('automobile');
  expect(restoredPreferences?.directionsRouteStrategy).toBe('fastest');
  expect(restoredPreferences?.capabilityProviderKeys).toEqual({ routing: 'osrm' });
});

test('a record from the previous schema version upgrades with its choices intact', () => {
  const restoredPreferences = referenceAppPreferencesFromStoredValue(
    JSON.stringify({
      version: 1,
      themeChoice: 'dark',
      baseMap: { providerKey: 'openfreemap', styleId: 'bright-light' },
      demoPlanAssetName: 'sacramento_clean_walk.json',
      selectedSceneKeys: ['route', 'traffic'],
      trafficLayerEnabled: true,
      cameraPresetId: 'pitched-view',
      speedMultiplier: 4,
      search: {
        origin: 'Capitol Park',
        destination: 'Tower Bridge',
        lastQuery: 'Tower',
      },
      directionsTransportationMode: 'pedestrian',
      directionsRouteStrategy: 'shortest',
      capabilityProviderKeys: { routing: 'osrm' },
    }),
  );
  expect(restoredPreferences).toEqual({
    version: referenceAppPreferencesVersion,
    themeChoice: 'dark',
    baseMap: { providerKey: 'openfreemap', styleId: 'bright-light' },
    demoPlanAssetName: 'sacramento_clean_walk.json',
    selectedSceneKeys: ['route', 'traffic'],
    trafficLayerEnabled: true,
    cameraPresetId: 'pitched-view',
    speedMultiplier: 4,
    search: {
      origin: 'Capitol Park',
      destination: 'Tower Bridge',
      lastQuery: 'Tower',
    },
    directionsTransportationMode: 'pedestrian',
    directionsRouteStrategy: 'shortest',
    capabilityProviderKeys: { routing: 'osrm' },
  });
});

test('a previous-version record drops only the choices it cannot map', () => {
  const restoredPreferences = referenceAppPreferencesFromStoredValue(
    JSON.stringify({
      version: 1,
      themeChoice: 'sepia',
      baseMap: { providerKey: '   ' },
      selectedSceneKeys: ['route', '', 4, 'route'],
      speedMultiplier: 3,
      search: { origin: 'Capitol Park', destination: 7 },
      directionsTransportationMode: 'hovercraft',
      directionsRouteStrategy: 'scenic',
      capabilityProviderKeys: { routing: 'osrm', geocoding: '' },
    }),
  );
  expect(restoredPreferences?.version).toBe(referenceAppPreferencesVersion);
  expect(restoredPreferences?.themeChoice).toBe('auto');
  expect(restoredPreferences?.baseMap).toBeNull();
  expect(restoredPreferences?.selectedSceneKeys).toEqual(['route']);
  expect(restoredPreferences?.speedMultiplier).toBe(
    simulatedTripSpeedMultiplierChoices[0],
  );
  expect(restoredPreferences?.search).toEqual({
    origin: 'Capitol Park',
    destination: '',
    lastQuery: '',
  });
  expect(restoredPreferences?.directionsTransportationMode).toBe('automobile');
  expect(restoredPreferences?.directionsRouteStrategy).toBe('fastest');
  expect(restoredPreferences?.capabilityProviderKeys).toEqual({ routing: 'osrm' });
});

test('a record older than the migration ladder is dropped as a whole', () => {
  expect(
    referenceAppPreferencesFromStoredValue(
      JSON.stringify({ version: 0, themeChoice: 'dark' }),
    ),
  ).toBeNull();
});

test('unknown keys are ignored and a raster base map carries no style id', () => {
  const restoredPreferences = referenceAppPreferencesFromStoredValue(
    JSON.stringify({
      version: referenceAppPreferencesVersion,
      themeChoice: 'light',
      baseMap: { providerKey: 'openstreetmap', styleId: '' },
      futureChoice: 'ignored',
      search: { origin: 'A', futureText: 'ignored' },
    }),
  );
  expect(restoredPreferences).not.toBeNull();
  expect(restoredPreferences?.themeChoice).toBe('light');
  expect(restoredPreferences?.baseMap).toEqual({
    providerKey: 'openstreetmap',
    styleId: null,
  });
  expect(restoredPreferences?.search.origin).toBe('A');
});

test('a record from an incompatible version is dropped as a whole', () => {
  expect(
    referenceAppPreferencesFromStoredValue(
      JSON.stringify({
        version: referenceAppPreferencesVersion + 1,
        themeChoice: 'dark',
      }),
    ),
  ).toBeNull();
  expect(referenceAppPreferencesFromStoredValue('not json')).toBeNull();
  expect(
    referenceAppPreferencesFromStoredValue(JSON.stringify({ themeChoice: 'dark' })),
  ).toBeNull();
  expect(
    referenceAppPreferencesFromStoredValue(
      JSON.stringify({ version: referenceAppPreferencesVersion, themeChoice: 'sepia' }),
    )?.themeChoice,
  ).toBe('auto');
});

test('invalid fields fall back to their defaults without failing the restore', () => {
  const restoredPreferences = referenceAppPreferencesFromStoredValue(
    JSON.stringify({
      version: referenceAppPreferencesVersion,
      themeChoice: 7,
      baseMap: { providerKey: '   ', styleId: 'bright-light' },
      demoPlanAssetName: 12,
      selectedSceneKeys: ['route', '', 4, 'route'],
      cameraPresetId: null,
      speedMultiplier: 3,
      search: { origin: 42, destination: 'Tower Bridge' },
    }),
  );
  expect(restoredPreferences).not.toBeNull();
  expect(restoredPreferences?.themeChoice).toBe('auto');
  expect(restoredPreferences?.baseMap).toBeNull();
  expect(restoredPreferences?.demoPlanAssetName).toBeNull();
  expect(restoredPreferences?.selectedSceneKeys).toEqual(['route']);
  expect(restoredPreferences?.cameraPresetId).toBeNull();
  expect(restoredPreferences?.speedMultiplier).toBe(
    simulatedTripSpeedMultiplierChoices[0],
  );
  expect(restoredPreferences?.search).toEqual({
    origin: '',
    destination: 'Tower Bridge',
    lastQuery: '',
  });
});
