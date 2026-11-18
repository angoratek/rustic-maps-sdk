/**
 * The on-map layer panel's pure decisions (docs/ReferenceAppUX.md §4.2):
 * the base-map rows, the traffic switch's availability, the demo-scene rows,
 * the device-location row and the camera-bounds answer the traffic layer asks
 * the capability for. The screen owns the effectful half (the SDK calls, the
 * overlay installs and the preferences record); this module stays free of
 * React and native imports so jest can pin every decision without a native
 * side.
 *
 * No provider and no vendor URL shape is named here. Which base rows, which
 * vector styles and which traffic capability exist comes from the shared
 * provider registry the caller passes in; the helpers only read the declared
 * fields. Consumer copy comes from the shared strings in `theme.ts`.
 */

import type { BoundingBox } from 'maps-sdk-rn/MapView';

import {
  vectorBaseChoiceKey,
  vectorBaseChoiceLabel,
  type BaseMapChoice,
  type VectorBaseMapChoice,
} from './baseMap';
import type { ProviderCredentialDescriptor } from './providerRegistry';
import { appStrings } from './theme';

// --- Shared identifiers and copy (the parity contract) ---

/** The collapsed control's test and accessibility identifier. */
export const mapLayersControlIdentifier = 'map-layers-control';

/** The expanded panel's test identifier. */
export const mapLayersPanelIdentifier = 'map-layers-panel';

/** The panel's Close action. */
export const mapLayersCloseIdentifier = 'map-layers-close';

/** The panel's own status line for a failed or refused layer action. */
export const mapLayersStatusIdentifier = 'map-layers-status';

/** One raster base row: `map-layer-base-<provider_key>`. */
export const mapLayerBaseRowIdentifierPrefix = 'map-layer-base-';

/** One committed vector style row: `map-layer-style-<style_id>`. */
export const mapLayerStyleRowIdentifierPrefix = 'map-layer-style-';

/** One demo-scene switch: `map-layer-scene-<scene_key>`. */
export const mapLayerSceneRowIdentifierPrefix = 'map-layer-scene-';

/** The traffic layer's switch. */
export const mapLayerTrafficIdentifier = 'map-layer-traffic';

/** The overlays section's shared clear action. */
export const mapLayerClearOverlaysIdentifier = 'map-layer-clear-overlays';

/** The panel's device-location status row. */
export const deviceLocationStatusIdentifier = 'device-location-status';

/** The panel's Stop action for the device-location source. */
export const deviceLocationStopIdentifier = 'device-location-stop';

/** The collapsed control's accessible name. */
export const mapLayersControlLabel = 'Map layers';

/** The control's non-default-layer dot. */
export const mapLayersActiveDotIdentifier = 'map-layers-active-dot';

/** The control's accessible value while a non-default layer is active. */
export const mapLayersActiveDotAccessibilityValue = 'Non-default layer active';

/** The control's layers glyph (the Compose/SwiftUI control's own mark). */
export const mapLayersControlGlyph = '▤';

/**
 * The panel's section titles in the order the specification fixes them. The
 * device-location row is the fourth section (the status-and-stop surface the
 * round my-location control opens).
 */
export const mapLayersPanelSectionTitles = Object.freeze([
  'Base map',
  'Traffic',
  'Overlays',
  'Device location',
]);

/** The vector-style group title inside the Base map section. */
export const mapLayersVectorStylesGroupTitle = appStrings.baseMapVectorGroupLabelText;

/**
 * How long the camera must stay still before the traffic layer asks the
 * capability for the settled camera's bounds. Every camera change restarts
 * it, so one settle yields one request.
 */
export const trafficRefreshDebounceMilliseconds = 400;

/** The drawn width of one traffic-flow line, in screen pixels. */
export const trafficFlowOverlayWidthPixels = 4;

/** The overlay identifier the traffic layer installs and removes. */
export const trafficFlowOverlayIdentifier = 'map-layer-traffic-overlay';

/** The capability name a registry descriptor declares to serve traffic flow. */
export const trafficFlowCapabilityName = 'traffic_flow';

/** The disabled reason a credentialed provider's row renders. */
export function needsCredentialText(credentialLabel: string): string {
  return appStrings.baseMapNeedsCredentialPrefixText + credentialLabel;
}

// --- Base map ---

/** What one base row's act selects. */
export type MapLayerBaseSelection =
  | { kind: 'raster'; providerKey: string }
  | { kind: 'vector'; choiceKey: string };

/** One base-map row of the layers panel. */
export interface MapLayerBaseRow {
  /** The shared test handle (`map-layer-base-…` or `map-layer-style-…`). */
  identifier: string;
  /** The consumer-facing row name. */
  displayName: string;
  /** The selection the row performs. */
  selection: MapLayerBaseSelection;
  /** Whether the row's base currently serves the map. */
  isServing: boolean;
  /** The disabled-with-reason text, or null while the row is usable. */
  disabledReasonText: string | null;
}

/**
 * Builds the base rows in the specification's order: the raster descriptors
 * first, then every offered vector style. A credentialed raster descriptor
 * whose key is not applied is disabled with `Needs <credential label>`, and a
 * styled vector base whose credential is missing is disabled the same way.
 */
export function buildMapLayerBaseRows(
  rasterChoices: readonly BaseMapChoice[],
  vectorChoices: readonly VectorBaseMapChoice[],
  activeRasterProviderKey: string | null,
  activeVectorChoiceKey: string | null,
): MapLayerBaseRow[] {
  const rows: MapLayerBaseRow[] = rasterChoices.map(choice => ({
    identifier:
      mapLayerBaseRowIdentifierPrefix + choice.descriptor.provider_key,
    displayName: choice.descriptor.display_name,
    selection: {
      kind: 'raster',
      providerKey: choice.descriptor.provider_key,
    },
    isServing:
      activeVectorChoiceKey === null &&
      choice.descriptor.provider_key === activeRasterProviderKey,
    disabledReasonText: choice.isServable
      ? null
      : needsCredentialText(choice.descriptor.credential_label ?? ''),
  }));
  const vectorRows: MapLayerBaseRow[] = vectorChoices.map(choice => ({
    identifier: mapLayerStyleRowIdentifierPrefix + choice.style.style_id,
    displayName: vectorBaseChoiceLabel(choice),
    selection: {
      kind: 'vector',
      choiceKey: vectorBaseChoiceKey(choice),
    },
    isServing: vectorBaseChoiceKey(choice) === activeVectorChoiceKey,
    disabledReasonText: choice.isServable
      ? null
      : needsCredentialText(choice.descriptor.credential_label ?? ''),
  }));
  rows.push(...vectorRows);
  return rows;
}

/** The trailing status one base row renders. */
export function baseRowStatusText(row: MapLayerBaseRow): string {
  if (row.disabledReasonText !== null) {
    return row.disabledReasonText;
  }
  return row.isServing
    ? appStrings.baseMapActiveStatusText
    : appStrings.baseMapInactiveStatusText;
}

/** The accessible name of one base row (its name plus its status). */
export function baseRowAccessibilityLabel(row: MapLayerBaseRow): string {
  return `${row.displayName} — ${baseRowStatusText(row)}`;
}

// --- Traffic ---

/** The traffic switch's availability, resolved from the shared registry. */
export type TrafficLayerAvailability =
  | {
      state: 'available';
      providerKey: string;
      attributionText: string;
    }
  | {
      state: 'needsCredential';
      providerKey: string;
      reasonText: string;
    }
  | {
      state: 'notConfigured';
      providerKey: null;
      reasonText: string;
    };

/** Whether a descriptor requires a runtime credential before it can serve. */
function descriptorRequiresCredential(
  descriptor: ProviderCredentialDescriptor,
): boolean {
  return (
    typeof descriptor.credential_label === 'string' &&
    descriptor.credential_label.trim() !== '' &&
    descriptor.registration_available?.react_native === true
  );
}

/**
 * Resolves the traffic switch from the shared registry: the first descriptor
 * declaring the capability and registering on React Native serves it; a
 * credential it does not yet have renders the descriptor's own label, while
 * no declaration at all states `Not configured`. Both reason strings are the
 * shared consumer copy, never a shell literal.
 */
export function trafficLayerAvailability(
  providerDescriptors: readonly ProviderCredentialDescriptor[],
  appliedProviderKeys: ReadonlySet<string>,
): TrafficLayerAvailability {
  const descriptor = providerDescriptors.find(
    candidate =>
      candidate.registration_available?.react_native === true &&
      Array.isArray(candidate.served_contracts) &&
      candidate.served_contracts.includes(trafficFlowCapabilityName),
  );
  if (descriptor === undefined) {
    return {
      state: 'notConfigured',
      providerKey: null,
      reasonText: appStrings.capabilityProviderNotConfiguredText,
    };
  }
  if (
    descriptorRequiresCredential(descriptor) &&
    !appliedProviderKeys.has(descriptor.provider_key)
  ) {
    return {
      state: 'needsCredential',
      providerKey: descriptor.provider_key,
      reasonText: needsCredentialText(descriptor.credential_label ?? ''),
    };
  }
  return {
    state: 'available',
    providerKey: descriptor.provider_key,
    attributionText: descriptor.attribution_text,
  };
}

/** Whether the traffic switch can be turned on. */
export function isTrafficLayerAvailable(
  availability: TrafficLayerAvailability,
): boolean {
  return availability.state === 'available';
}

/** The disabled reason the traffic switch renders, or null while available. */
export function trafficLayerReasonText(
  availability: TrafficLayerAvailability,
): string | null {
  return availability.state === 'available' ? null : availability.reasonText;
}

/** The serving descriptor's attribution line while the layer is on. */
export function trafficLayerAttributionText(
  availability: TrafficLayerAvailability,
): string | null {
  return availability.state === 'available'
    ? availability.attributionText
    : null;
}

/**
 * Whether the remembered traffic choice restores in this session. A choice
 * restores only when the serving provider can honour it now; a keyless
 * session keeps the choice for a later launch instead of pretending the layer
 * is on.
 */
export function shouldRestoreTrafficLayer(
  rememberedTrafficLayerEnabled: boolean,
  availability: TrafficLayerAvailability,
): boolean {
  return rememberedTrafficLayerEnabled && isTrafficLayerAvailable(availability);
}

// --- Overlays ---

/** One demo-scene switch of the Overlays section. */
export interface MapLayerSceneRow {
  sceneKey: string;
  label: string;
  isEnabled: boolean;
  identifier: string;
}

/** Builds one switch per shared demo scene, in the artifact's order. */
export function buildMapLayerSceneRows(
  scenes: readonly { sceneKey: string; label: string }[],
  enabledSceneKeys: readonly string[],
): MapLayerSceneRow[] {
  const enabledSceneKeySet = new Set(enabledSceneKeys);
  return scenes.map(scene => ({
    sceneKey: scene.sceneKey,
    label: scene.label,
    isEnabled: enabledSceneKeySet.has(scene.sceneKey),
    identifier: mapLayerSceneRowIdentifierPrefix + scene.sceneKey,
  }));
}

/**
 * Whether the collapsed control shows its non-default-layer dot: any base
 * other than the session's default, traffic on, or any demo scene enabled.
 */
export function isNonDefaultLayerActive({
  isDefaultBaseServing,
  isTrafficEnabled,
  enabledSceneCount,
}: {
  isDefaultBaseServing: boolean;
  isTrafficEnabled: boolean;
  enabledSceneCount: number;
}): boolean {
  return !isDefaultBaseServing || isTrafficEnabled || enabledSceneCount > 0;
}

// --- Device location ---

/** The panel's device-location row states, exactly the panel table's wording. */
export type DeviceLocationPanelStatus =
  | 'off'
  | 'waiting'
  | 'active'
  | 'permissionDenied'
  | 'unavailable';

/** The row's presentation text for each state. */
export const deviceLocationPanelStatusTexts = {
  off: 'Off',
  waiting: 'Waiting for a fix',
  active: 'Active',
  permissionDenied: 'Permission denied',
  unavailable: 'Unavailable',
} as const;

/** The row's presentation text for one state. */
export function deviceLocationPanelStatusText(
  status: DeviceLocationPanelStatus,
): string {
  return deviceLocationPanelStatusTexts[status];
}

/** Whether the source is updating (the row shows its Stop action). */
export function isDeviceLocationSourceRunning(
  status: DeviceLocationPanelStatus,
): boolean {
  return status === 'waiting' || status === 'active';
}

/** The reason the source cannot start, or null while it can. */
export function deviceLocationCannotStartReasonText(
  status: DeviceLocationPanelStatus,
): string | null {
  return status === 'permissionDenied' || status === 'unavailable'
    ? deviceLocationPanelStatusTexts[status]
    : null;
}

// --- Camera bounds (the traffic layer's query box) ---

/** One camera pose the bounds answer reads (the package `MapCamera` shape). */
export interface MapCameraPose {
  latitudeDegrees: number;
  longitudeDegrees: number;
  zoomLevel: number;
}

/** The web-mercator world tile's pixel edge at zoom 0 (the tile scheme's own). */
const webMercatorWorldTileSizePixels = 256;

/** The latitude where web mercator's projected world ends. */
const maximumWebMercatorLatitudeDegrees = 85.05112878;

/** The longitude/latitude limits the inverse projection clamps into. */
const maximumLongitudeDegrees = 180;
const minimumLatitudeDegrees = -90;
const maximumLatitudeDegrees = 90;

/** Degrees per radian (structural math). */
const degreesPerRadian = 180 / Math.PI;
/** Radians per degree (structural math). */
const radiansPerDegree = Math.PI / 180;

/**
 * The geographic bounds the viewport covers for one camera pose, in web
 * mercator. The answer is the axis-aligned box of the viewport rectangle; a
 * non-finite pose or a zero viewport answers null (the panel then states the
 * bounds are unavailable rather than asking the capability for a guess).
 */
export function geographicBoundsForCamera(
  camera: MapCameraPose,
  viewportWidthPixels: number,
  viewportHeightPixels: number,
): BoundingBox | null {
  if (
    !Number.isFinite(camera.latitudeDegrees) ||
    !Number.isFinite(camera.longitudeDegrees) ||
    !Number.isFinite(camera.zoomLevel) ||
    !Number.isFinite(viewportWidthPixels) ||
    !Number.isFinite(viewportHeightPixels) ||
    viewportWidthPixels <= 0 ||
    viewportHeightPixels <= 0
  ) {
    return null;
  }
  const clampedLatitudeDegrees = Math.max(
    -maximumWebMercatorLatitudeDegrees,
    Math.min(maximumWebMercatorLatitudeDegrees, camera.latitudeDegrees),
  );
  const worldSizePixels =
    webMercatorWorldTileSizePixels * 2 ** camera.zoomLevel;
  const centerX =
    ((camera.longitudeDegrees + maximumLongitudeDegrees) /
      (2 * maximumLongitudeDegrees)) *
    worldSizePixels;
  const sineLatitude = Math.sin(clampedLatitudeDegrees * radiansPerDegree);
  const centerY =
    (0.5 -
      Math.log((1 + sineLatitude) / (1 - sineLatitude)) / (4 * Math.PI)) *
    worldSizePixels;
  const halfWidthPixels = viewportWidthPixels / 2;
  const halfHeightPixels = viewportHeightPixels / 2;
  return {
    southWest: {
      latitude: mercatorYToLatitude(
        centerY + halfHeightPixels,
        worldSizePixels,
      ),
      longitude: mercatorXToLongitude(
        centerX - halfWidthPixels,
        worldSizePixels,
      ),
    },
    northEast: {
      latitude: mercatorYToLatitude(
        centerY - halfHeightPixels,
        worldSizePixels,
      ),
      longitude: mercatorXToLongitude(
        centerX + halfWidthPixels,
        worldSizePixels,
      ),
    },
  };
}

/** The longitude a web-mercator world x coordinate names. */
function mercatorXToLongitude(
  worldXPixels: number,
  worldSizePixels: number,
): number {
  const longitudeDegrees =
    (worldXPixels / worldSizePixels) * (2 * maximumLongitudeDegrees) -
    maximumLongitudeDegrees;
  return Math.max(
    -maximumLongitudeDegrees,
    Math.min(maximumLongitudeDegrees, longitudeDegrees),
  );
}

/** The latitude a web-mercator world y coordinate names. */
function mercatorYToLatitude(
  worldYPixels: number,
  worldSizePixels: number,
): number {
  const projectedY = Math.PI - (2 * Math.PI * worldYPixels) / worldSizePixels;
  const latitudeDegrees = Math.atan(Math.sinh(projectedY)) * degreesPerRadian;
  return Math.max(
    minimumLatitudeDegrees,
    Math.min(maximumLatitudeDegrees, latitudeDegrees),
  );
}

// --- The panel's state machine ---

/** The panel's own transient state. */
export interface MapLayersPanelState {
  /** Whether the expanded panel is open. */
  isOpen: boolean;
  /** The traffic switch's availability. */
  trafficAvailability: TrafficLayerAvailability;
  /** Whether the traffic layer is on. */
  isTrafficEnabled: boolean;
  /** Whether a traffic request is in flight. */
  isTrafficRefreshing: boolean;
  /** The panel's status line, or null while nothing needs saying. */
  statusLineText: string | null;
  /** The device-location row's state. */
  locationStatus: DeviceLocationPanelStatus;
}

/** One transition of the panel's state machine. */
export type MapLayersPanelAction =
  | { type: 'panelOpened' }
  | { type: 'panelClosed' }
  | { type: 'trafficConfigurationUpdated'; availability: TrafficLayerAvailability }
  | { type: 'trafficToggled'; isEnabled: boolean }
  | { type: 'trafficRestored' }
  | { type: 'trafficRefreshStarted' }
  | { type: 'trafficRefreshSucceeded' }
  | {
      type: 'trafficRefreshFailed';
      detailText: string;
      isProviderNotConfigured: boolean;
    }
  | { type: 'deviceLocationStatusChanged'; status: DeviceLocationPanelStatus }
  | { type: 'statusLineCleared' };

/** The panel's initial state (closed, no traffic, location off). */
export function initialMapLayersPanelState(): MapLayersPanelState {
  return {
    isOpen: false,
    trafficAvailability: trafficLayerAvailability([], new Set<string>()),
    isTrafficEnabled: false,
    isTrafficRefreshing: false,
    statusLineText: null,
    locationStatus: 'off',
  };
}

/**
 * The panel's state machine. A traffic turn-on the registry cannot serve
 * states the reason instead of pretending; a configuration change that makes
 * the serving descriptor unavailable turns the layer off; a failed refresh
 * keeps the previous overlay and states the verbatim detail, and a
 * `ProviderNotConfigured` failure turns the switch off with its reason.
 */
export function mapLayersPanelReducer(
  state: MapLayersPanelState,
  action: MapLayersPanelAction,
): MapLayersPanelState {
  switch (action.type) {
    case 'panelOpened':
      return { ...state, isOpen: true };
    case 'panelClosed':
      return { ...state, isOpen: false };
    case 'trafficConfigurationUpdated': {
      const isNowAvailable = isTrafficLayerAvailable(action.availability);
      return {
        ...state,
        trafficAvailability: action.availability,
        isTrafficEnabled: state.isTrafficEnabled && isNowAvailable,
        isTrafficRefreshing: state.isTrafficRefreshing && isNowAvailable,
      };
    }
    case 'trafficToggled':
      if (action.isEnabled) {
        if (!isTrafficLayerAvailable(state.trafficAvailability)) {
          return {
            ...state,
            isTrafficEnabled: false,
            statusLineText: trafficLayerReasonText(state.trafficAvailability),
          };
        }
        return { ...state, isTrafficEnabled: true, statusLineText: null };
      }
      return {
        ...state,
        isTrafficEnabled: false,
        isTrafficRefreshing: false,
        statusLineText: null,
      };
    case 'trafficRestored':
      return isTrafficLayerAvailable(state.trafficAvailability)
        ? { ...state, isTrafficEnabled: true, statusLineText: null }
        : state;
    case 'trafficRefreshStarted':
      return { ...state, isTrafficRefreshing: true };
    case 'trafficRefreshSucceeded':
      return {
        ...state,
        isTrafficRefreshing: false,
        statusLineText: null,
      };
    case 'trafficRefreshFailed':
      return {
        ...state,
        isTrafficRefreshing: false,
        isTrafficEnabled: action.isProviderNotConfigured
          ? false
          : state.isTrafficEnabled,
        statusLineText: action.detailText,
      };
    case 'deviceLocationStatusChanged': {
      const statusLineText =
        action.status === 'permissionDenied' || action.status === 'unavailable'
          ? deviceLocationPanelStatusTexts[action.status]
          : action.status === 'off'
            ? null
            : state.statusLineText;
      return { ...state, locationStatus: action.status, statusLineText };
    }
    case 'statusLineCleared':
      return { ...state, statusLineText: null };
  }
}
