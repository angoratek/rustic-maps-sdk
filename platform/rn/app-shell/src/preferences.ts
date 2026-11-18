/**
 * The reference application's remembered choices: one versioned record of the
 * explicit selections the shell restores on the next launch. Transient state
 * (the camera pose, an open sheet, the current route result) and the provider
 * credential are never part of this record; the credential keeps its own
 * opt-in store.
 *
 * The record crosses to the native side as a single JSON string through the
 * demo bridge module. A record written by an older version is migrated so its
 * recognizable choices survive a schema change, a record written by a newer
 * version is ignored as a whole, an unknown key is ignored, and an invalid
 * value falls back to that field's default rather than failing the restore.
 */

import {
  defaultDirectionsRouteStrategyName,
  defaultDirectionsTransportationModeName,
  directionsRouteStrategyNames,
  directionsTransportationModeNames,
  type DirectionsRouteStrategyName,
  type DirectionsTransportationModeName,
} from './directionsSelection';
import {
  defaultThemeChoice,
  themeChoiceFromStoredValue,
  type ThemeChoice,
} from './theme';

/** The record shape this build writes and reads. */
export const referenceAppPreferencesVersion = 2;

/**
 * The oldest stored record version this build migrates. Every version from
 * here to the one below the current has a migration step in
 * `referenceAppPreferencesMigrations`.
 */
const oldestMigratableReferenceAppPreferencesVersion = 1;

/**
 * The playback speed multipliers the drawer offers and the record may carry.
 * The native hosts validate the same set before rebuilding a walker.
 */
export const simulatedTripSpeedMultiplierChoices: readonly number[] = [1, 2, 4, 8];

/**
 * One remembered base map: a registry provider key plus the committed vector
 * style id, or a null style id for the provider's raster source.
 */
export interface RememberedBaseMap {
  providerKey: string;
  styleId: string | null;
}

/** The last texts the plan flow held: both fields and the last search query. */
export interface RememberedSearchTexts {
  origin: string;
  destination: string;
  lastQuery: string;
}

/**
 * The explicit choices the shell remembers across launches. Every field has a
 * default so a partial or older record restores what it can.
 */
export interface ReferenceAppPreferences {
  version: typeof referenceAppPreferencesVersion;
  themeChoice: ThemeChoice;
  baseMap: RememberedBaseMap | null;
  /** The selected demo plan's bundled trace name, or null for the default plan. */
  demoPlanAssetName: string | null;
  /** The selected overlay scene keys, in artifact order. */
  selectedSceneKeys: string[];
  /**
   * Whether the traffic layer was on, restored only when the serving provider
   * can honour it in the next session (docs/design/MapControlsAndTrafficLayer.md).
   */
  trafficLayerEnabled: boolean;
  cameraPresetId: string | null;
  speedMultiplier: number;
  search: RememberedSearchTexts;
  /** The plan sheet's mode choice (Car default, docs/design/DirectionsModesAndStrategy.md). */
  directionsTransportationMode: DirectionsTransportationModeName;
  /** The plan sheet's strategy choice (Fastest default, shown for car plans). */
  directionsRouteStrategy: DirectionsRouteStrategyName;
  /**
   * The provider chosen for each capability, keyed by the capability's
   * registry name (`routing`, `geocoding`, …). An unknown capability key is
   * ignored on restore; a choice the SDK refuses keeps the previous provider.
   */
  capabilityProviderKeys: Record<string, string>;
}

/** The record of a device that never chose or of a reset. */
export function defaultReferenceAppPreferences(): ReferenceAppPreferences {
  return {
    version: referenceAppPreferencesVersion,
    themeChoice: defaultThemeChoice,
    baseMap: null,
    demoPlanAssetName: null,
    selectedSceneKeys: [],
    trafficLayerEnabled: false,
    cameraPresetId: null,
    speedMultiplier: simulatedTripSpeedMultiplierChoices[0],
    search: { origin: '', destination: '', lastQuery: '' },
    directionsTransportationMode: defaultDirectionsTransportationModeName,
    directionsRouteStrategy: defaultDirectionsRouteStrategyName,
    capabilityProviderKeys: {},
  };
}

/** A parsed stored record's named fields. */
type StoredReferenceAppPreferences = Record<string, unknown>;

/**
 * One forward migration: lifts a record written by `fromVersion` to the next
 * version. A step maps the choices the newer shape still recognizes; a value
 * it cannot map is left to that field's default when the record is read.
 */
interface ReferenceAppPreferencesMigration {
  fromVersion: number;
  migrate: (
    storedRecord: StoredReferenceAppPreferences,
  ) => StoredReferenceAppPreferences;
}

/**
 * The migration ladder from every older stored shape to the current one, in
 * ascending order. A missing step means that version cannot be recognized and
 * its record is ignored as a whole.
 */
const referenceAppPreferencesMigrations: readonly ReferenceAppPreferencesMigration[] =
  [
    {
      fromVersion: oldestMigratableReferenceAppPreferencesVersion,
      migrate: storedRecord => ({
        ...storedRecord,
        // The second shape carries no renamed or removed field: every
        // first-shape choice is recognized unchanged. The step stays explicit
        // so a future shape change maps its fields here instead of discarding
        // the record.
        version: oldestMigratableReferenceAppPreferencesVersion + 1,
      }),
    },
  ];

/** Whether a parsed JSON value is an object with named fields. */
function isJsonObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/** A trimmed non-empty string, or null for anything else. */
function nonEmptyString(value: unknown): string | null {
  if (typeof value !== 'string') {
    return null;
  }
  const trimmedText = value.trim();
  return trimmedText === '' ? null : trimmedText;
}

/** A remembered text field, preserving the entered text exactly. */
function textFromValue(value: unknown): string {
  return typeof value === 'string' ? value : '';
}

/**
 * The remembered base map a stored value names, or null when it names none.
 * The provider key is required; a missing style id means the raster source.
 */
function rememberedBaseMapFromValue(value: unknown): RememberedBaseMap | null {
  if (!isJsonObject(value)) {
    return null;
  }
  const providerKey = nonEmptyString(value.providerKey);
  if (providerKey === null) {
    return null;
  }
  return { providerKey, styleId: nonEmptyString(value.styleId) };
}

/** The selected scene keys of a stored value: strings only, de-duplicated. */
function sceneKeysFromValue(value: unknown): string[] {
  if (!Array.isArray(value)) {
    return [];
  }
  const sceneKeys: string[] = [];
  for (const candidate of value) {
    const sceneKey = nonEmptyString(candidate);
    if (sceneKey !== null && !sceneKeys.includes(sceneKey)) {
      sceneKeys.push(sceneKey);
    }
  }
  return sceneKeys;
}

/** One of the offered speed multipliers, or the default pace. */
function speedMultiplierFromValue(value: unknown): number {
  return (
    simulatedTripSpeedMultiplierChoices.find(choice => choice === value) ??
    simulatedTripSpeedMultiplierChoices[0]
  );
}

/** One of the offered Directions modes, or Car (the default). */
function directionsTransportationModeFromValue(
  value: unknown,
): DirectionsTransportationModeName {
  return (
    directionsTransportationModeNames.find(
      transportationModeName => transportationModeName === value,
    ) ?? defaultDirectionsTransportationModeName
  );
}

/** One of the offered Directions strategies, or Fastest (the default). */
function directionsRouteStrategyFromValue(
  value: unknown,
): DirectionsRouteStrategyName {
  return (
    directionsRouteStrategyNames.find(
      routeStrategyName => routeStrategyName === value,
    ) ?? defaultDirectionsRouteStrategyName
  );
}

/** The remembered per-capability provider keys (non-empty keys only). */
function capabilityProviderKeysFromValue(
  value: unknown,
): Record<string, string> {
  if (!isJsonObject(value)) {
    return {};
  }
  const capabilityProviderKeys: Record<string, string> = {};
  for (const [capabilityName, providerKey] of Object.entries(value)) {
    const nonEmptyProviderKey = nonEmptyString(providerKey);
    if (capabilityName.trim() !== '' && nonEmptyProviderKey !== null) {
      capabilityProviderKeys[capabilityName] = nonEmptyProviderKey;
    }
  }
  return capabilityProviderKeys;
}

/**
 * Lifts a parsed stored record to the current version, or null when its
 * version is newer than this build or no migration path reaches the current
 * shape.
 */
function migrateStoredReferenceAppPreferences(
  storedRecord: StoredReferenceAppPreferences,
): StoredReferenceAppPreferences | null {
  const storedVersion = storedRecord.version;
  if (typeof storedVersion !== 'number' || !Number.isInteger(storedVersion)) {
    return null;
  }
  let migratedRecord = storedRecord;
  for (
    let version = storedVersion;
    version < referenceAppPreferencesVersion;
    version += 1
  ) {
    const migration = referenceAppPreferencesMigrations.find(
      candidateMigration => candidateMigration.fromVersion === version,
    );
    if (migration === undefined) {
      return null;
    }
    migratedRecord = migration.migrate(migratedRecord);
  }
  return migratedRecord.version === referenceAppPreferencesVersion
    ? migratedRecord
    : null;
}

/**
 * The record a stored JSON string names, or null when nothing was stored, the
 * string is not JSON, or it was written by a newer version. An older version
 * is migrated first so its recognizable choices survive the schema change.
 * Unknown keys are ignored and each invalid field falls back to its default.
 */
export function referenceAppPreferencesFromStoredValue(
  storedValue: string | null | undefined,
): ReferenceAppPreferences | null {
  if (typeof storedValue !== 'string' || storedValue.trim() === '') {
    return null;
  }
  let parsedValue: unknown;
  try {
    parsedValue = JSON.parse(storedValue);
  } catch {
    return null;
  }
  if (!isJsonObject(parsedValue)) {
    return null;
  }
  const migratedValue = migrateStoredReferenceAppPreferences(parsedValue);
  if (migratedValue === null) {
    return null;
  }
  const defaults = defaultReferenceAppPreferences();
  const storedSearch = isJsonObject(migratedValue.search)
    ? migratedValue.search
    : {};
  return {
    version: referenceAppPreferencesVersion,
    themeChoice: themeChoiceFromStoredValue(
      typeof migratedValue.themeChoice === 'string'
        ? migratedValue.themeChoice
        : null,
    ),
    baseMap: rememberedBaseMapFromValue(migratedValue.baseMap),
    demoPlanAssetName: nonEmptyString(migratedValue.demoPlanAssetName),
    selectedSceneKeys: sceneKeysFromValue(migratedValue.selectedSceneKeys),
    trafficLayerEnabled: migratedValue.trafficLayerEnabled === true,
    cameraPresetId: nonEmptyString(migratedValue.cameraPresetId),
    speedMultiplier: speedMultiplierFromValue(migratedValue.speedMultiplier),
    search: {
      origin: textFromValue(storedSearch.origin),
      destination: textFromValue(storedSearch.destination),
      lastQuery: textFromValue(storedSearch.lastQuery),
    },
    directionsTransportationMode: directionsTransportationModeFromValue(
      migratedValue.directionsTransportationMode,
    ),
    directionsRouteStrategy: directionsRouteStrategyFromValue(
      migratedValue.directionsRouteStrategy,
    ),
    capabilityProviderKeys: capabilityProviderKeysFromValue(
      migratedValue.capabilityProviderKeys,
    ),
  };
}
