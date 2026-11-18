/**
 * The reference application's v2 screen (docs/ReferenceAppUX.md): the map
 * full-bleed, the readiness chip at top-start (§6.1) opening the diagnostics
 * sheet, the turn banner stack above the map during a demo (§7.1), and the
 * sheet-and-drawer dock (§4.2's peek/half/full sheet, the navigation drawer
 * during a trip) carrying the plan flow (§4.1's search, route summary, demo
 * plans, demo scenes, offline card) and the theme row (§3's explicit-owned
 * theme).
 * Tokens come from `theme.ts` (§2's token tables, dark default); every
 * consumer-facing string names product concepts, never seam names.
 */

import React, { useCallback, useEffect, useMemo, useReducer, useRef, useState } from 'react';
import {
  AccessibilityInfo,
  Modal,
  Animated,
  BackHandler,
  PanResponder,
  PermissionsAndroid,
  PixelRatio,
  Platform,
  Pressable,
  ScrollView,
  Switch,
  TextInput,
  Text,
  View,
  findNodeHandle,
  useColorScheme,
  useWindowDimensions,
} from 'react-native';

import {
  addControllerListener,
  hasNativeController,
  refappMapModule,
  type CacheStateEvent,
  type CameraReadbackEvent,
  type ErrorReportEvent,
  type FrameStatisticsEvent,
  type PlaceRow,
  type PlanStateEvent,
  type PlaybackStateEvent,
  type ReadinessEvent,
  type SpeechEngineStateEvent,
  type SpeechInstructionEvent,
  type TraceUpdateEvent,
} from './src/RefappNative';
import { MapView, type MapCamera, type MapViewHandle } from 'maps-sdk-rn/MapView';
import { FrameClock } from 'maps-sdk-rn/src/frameClock.ts';
import { FollowBearingMode } from 'maps-sdk-rn/src/followBearingMode.ts';
import { LocationPoseAnimator } from 'maps-sdk-rn/src/locationPoseAnimator.ts';
import {
  GeocodeQuality,
  LocationIndicatorVisualKind,
  MapOverlayKind,
  ProviderContract,
  RouteStrategy,
  TransportationMode,
} from 'maps-sdk-rn/src/generated/rustic_maps.ts';
import { MapsSdk } from 'maps-sdk-rn/MapsSdk';
import type {
  ProviderRegistration,
  RegionDownloadProgress,
  RegionRecord,
  RouteCapabilities,
  TileCacheStatistics,
  TrafficFlowCacheStatistics,
  TrafficFlowData,
} from 'maps-sdk-rn/MapsSdk';
import { SafeAreaProvider, useSafeAreaInsets } from 'react-native-safe-area-context';
import {
  appStrings,
  defaultThemeChoice,
  nextThemeChoice,
  resolvedThemeName,
  themeChoiceFromStoredValue,
  themeChoiceSwitchLabel,
  themeColorsByThemeName,
  themeSpacing,
  themeTypography,
  type ThemeChoice,
  type ThemeColorTokens,
} from './src/theme';
import { buildScreenStyles } from './src/screenStyles';
import {
  bearingOffNorthDegrees,
  compassVisibilityBearingThresholdDegrees,
  distanceLabelText,
  mapControlLayout,
  mapControlZoomRepeatIntervalMilliseconds,
  selectScaleBarScale,
} from './src/mapControls';
import {
  peekDockHeight,
  phoneWidthBreakpointPixels,
  sheetFullHeightFraction,
  sheetHalfHeightFraction,
  sheetHalfPhoneHeightFraction,
} from './src/sheetLayout';
import {
  readinessChipPresentation,
  type ReadinessTone,
} from './src/readinessPresentation';
import {
  cameraPresets,
  demoPlaceNamed,
  demoPlans,
  offlineRegionDefinitions,
  overlayScenes,
  recordedDemoRoute,
  searchDemoPlaces,
  type CameraPreset,
} from './src/referenceScenes';
import {
  providerAttributionTermsNotes,
  providerAttributionText,
  providerDescriptors,
  providerReferenceAppUserAgent,
  providerRegistry,
  type ProviderCredentialDescriptor,
} from './src/providerRegistry';
import {
  activeBaseMap,
  baseMapChoices,
  baseMapRegistrationPlan,
  hostedVectorBaseInstall,
  keylessVectorBaseChoice,
  vectorBaseAttributionText,
  vectorBaseChoiceKey,
  vectorBaseMapChoices,
  vectorBaseSource,
  type BaseMapChoice,
  type VectorBaseMapChoice,
} from './src/baseMap';
import {
  canApplyProviderCredential,
  createInitialProviderCredentialsState,
  isProviderCredentialApplied,
  isProviderCredentialRegistrationAvailable,
  planProviderCredentialApply,
  providerCredentialReducer,
  usableProviderCredentialDescriptors,
  type ProviderCredentialRowState,
} from './src/providerCredentials';
import {
  createSpeechConsumer,
  initialSpeechConsumerState,
  speechDeliveryKindLabels,
  type SpeechConsumerState,
} from './src/speechConsumer';
import {
  defaultReferenceAppPreferences,
  referenceAppPreferencesFromStoredValue,
  simulatedTripSpeedMultiplierChoices,
  type ReferenceAppPreferences,
} from './src/preferences';
import {
  applyCapabilityProviderSelection,
  buildCapabilityProviderRows,
  capabilityContractForRegistryName,
  capabilityRegistryName,
  capabilityServingStatusText,
  type CapabilityProviderRow,
} from './src/capabilityProviderState';
import {
  selectAutocompleteProvider,
  usePlanAutocomplete,
  type AutocompleteProviderSelection,
  type PlanAutocompleteGateway,
} from './src/planSearchAutocomplete';
import {
  DiagnosticsStreamController,
  diagnosticsBacklogFlushLimit,
  type DiagnosticEvent,
} from './src/diagnosticsStream';
import {
  hasProviderConsoleAlertAfter,
  type ProviderRequestTrace,
} from './src/providerConsole';
import { DeveloperConsole } from './src/DeveloperConsole';
import {
  directionsControlState,
  directionsSelectionFromNames,
  type DirectionsControlState,
  type DirectionsRouteStrategyName,
  type DirectionsTransportationModeName,
} from './src/directionsSelection';
import {
  MapLayersControl,
  MapLayersPanel,
  type MapLayersControlRef,
} from './src/MapLayersPanel';
import {
  buildMapLayerBaseRows,
  buildMapLayerSceneRows,
  deviceLocationCannotStartReasonText,
  geographicBoundsForCamera,
  initialMapLayersPanelState,
  isNonDefaultLayerActive,
  isTrafficLayerAvailable,
  mapLayersPanelReducer,
  shouldRestoreTrafficLayer,
  trafficFlowOverlayIdentifier,
  trafficFlowOverlayWidthPixels,
  trafficLayerAvailability,
  trafficLayerAttributionText,
  trafficRefreshDebounceMilliseconds,
  type MapCameraPose,
  type MapLayerBaseRow,
} from './src/mapLayersPanelState';

/** The sheet's settled state (§4.2's peek/half/full, tap-cycle and drag). */
type SheetState = 'peek' | 'half' | 'full';
/** The sheet's content form: the home rows, or the search view (§4.1). */
type SheetContentMode = 'home' | 'search';
/** The plan field the result list currently serves (§4.1's field pair). */
type PlanSearchField = 'origin' | 'destination';

/** The consumer field word a no-match line names. */
function planSearchFieldLabel(planSearchField: PlanSearchField): string {
  return planSearchField === 'origin' ? 'origin' : 'destination';
}

/** The A/B markers the two plan fields carry beside their visible labels. */
const planOriginFieldMarker = 'A';
const planDestinationFieldMarker = 'B';

/** The decimal places the My location fill gives a device fix (metres). */
const planFieldDeviceFixDecimalPlaces = 5;

/** The mark a My location chip carries once it filled its own field. */
const myLocationChipFilledMark = '·';

/** The diagnostics event's fields, decoded from the native payload. */
function diagnosticEventFromPayload(
  payload: Record<string, unknown>,
): DiagnosticEvent {
  return {
    timestampMillisecondsSinceEpoch: Number(
      payload.timestampMillisecondsSinceEpoch ?? 0,
    ),
    severity:
      typeof payload.severity === 'string' ? payload.severity : 'info',
    subsystemTarget:
      typeof payload.subsystemTarget === 'string'
        ? payload.subsystemTarget
        : '',
    messageLine:
      typeof payload.messageLine === 'string' ? payload.messageLine : '',
    providerRequestTrace: providerRequestTraceFromPayload(
      payload.providerRequestTrace,
    ),
  };
}

/** The typed provider request trace, or null when the event carries none. */
function providerRequestTraceFromPayload(
  payload: unknown,
): ProviderRequestTrace | null {
  if (payload === null || typeof payload !== 'object') {
    return null;
  }
  const trace = payload as Record<string, unknown>;
  const responseExcerpt =
    typeof trace.responseExcerpt === 'string' ? trace.responseExcerpt : null;
  return {
    providerKey:
      typeof trace.providerKey === 'string' ? trace.providerKey : '',
    providerContract:
      typeof trace.providerContract === 'string'
        ? trace.providerContract
        : '',
    httpMethod:
      typeof trace.httpMethod === 'string' ? trace.httpMethod : '',
    requestPath:
      typeof trace.requestPath === 'string' ? trace.requestPath : '',
    statusClass:
      typeof trace.statusClass === 'string' ? trace.statusClass : '',
    outcome: typeof trace.outcome === 'string' ? trace.outcome : '',
    durationMilliseconds: Number(trace.durationMilliseconds ?? 0),
    responseBytes: Number(trace.responseBytes ?? 0),
    responseExcerpt,
  };
}

/** The selected scene keys in artifact order (the remembered record's order). */
function rememberedSceneKeys(
  sceneSelections: Record<string, boolean>,
): string[] {
  return overlayScenes
    .filter(({ sceneKey }) => sceneSelections[sceneKey])
    .map(({ sceneKey }) => sceneKey);
}

/**
 * The dock's viewport fractions and the content-driven peek height come from
 * `sheetLayout.ts` (§2.4's home-sheet rows, §1's full-state cap); only the
 * drag and spring behaviors stay local.
 */
/** Drag thresholds: movement recognition and the 30 px state-snap rule. */
const sheetDragRecognizePixels = 8;
const sheetDragSnapPixels = 30;
/** The dock settle's spring feel (§4.2's quick settle, a single small overshoot). */
const settleSpringSpeed = 20;
const settleSpringBounciness = 4;

/**
 * The offline card's demo region (the shared artifact's first
 * `offline_regions` row), or null when the artifact declares none.
 */
const demoOfflineRegion = offlineRegionDefinitions[0] ?? null;

/** The error strip's own top gap below the status bar (§6.3's stacking). */
const errorStripGap = 70;
/** The banner stack's own top gap below the status bar (§7.1's stacking). */
const bannerStackGap = 56;

/**
 * The readiness chip's color role → the owned theme's color row. The chip
 * never borrows a success/failure color for an unknown state (D4).
 */
const readinessToneColorKeys: Record<ReadinessTone, keyof ThemeColorTokens> = {
  neutral: 'textSecondary',
  positive: 'positive',
  warning: 'warning',
  danger: 'danger',
};

/** No readiness event has arrived yet: the state is not known (D4). */
const initialReadiness: ReadinessEvent = {
  state: 'unknown',
  text: '',
};

const initialPlanState: PlanStateEvent = {
  isPlanned: false,
  isInFlight: false,
  titleText: '',
};

/**
 * The Location group's second control (the shared copy of every reference
 * application): device-driven navigation follows the planned route from the
 * same location source's fixes, so the banner, the ETA line and the follow
 * camera read real fixes instead of the recorded trace.
 */
const deviceNavigationStartText = 'Navigate from my location';
const deviceNavigationStopText = 'Stop device navigation';
const deviceNavigationPlanFirstText =
  'Plan a route first; device navigation follows the route you planned.';
const deviceNavigationWaitingText = 'Waiting for your position to start guidance.';
const deviceNavigationGuidanceUnavailableText =
  'Guidance could not start on the planned route.';
/**
 * The accuracy a device fix is delivered at when its source carries none: the
 * navigation reducer needs a graded fix to advance steps and judge
 * deviations, so the accepted fix is reported at the recorded trace's own
 * accuracy value.
 */
const deviceNavigationFallbackAccuracyMeters = 8.0;

/** No playback state reported yet: the trip is not active at the default pace. */
const initialPlaybackState: PlaybackStateEvent = {
  isActive: false,
  isPaused: false,
  speedMultiplier: simulatedTripSpeedMultiplierChoices[0],
  hasArrived: false,
  isSeekable: false,
  totalMeters: 0,
};

/**
 * The provider-credential rows the settings sheet renders, driven by the
 * shared provider registry's descriptors (platform/providers.json): a
 * malformed entry is dropped, and each valid descriptor gets one row. Module
 * scope keeps the list identity stable across renders.
 */
const credentialDescriptors: ProviderCredentialDescriptor[] =
  usableProviderCredentialDescriptors(providerDescriptors);

/** A thrown error's own text, without an `Error:` prefix (the SDK's verbatim words). */
function verbatimErrorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * Whether a thrown error is the contract's missing-provider variant. The
 * generated binding's message carries the variant name, so the traffic layer
 * can turn its switch off with the capability's own reason.
 */
function isProviderNotConfiguredFailure(error: unknown): boolean {
  return /ProviderNotConfigured/.test(verbatimErrorText(error));
}

/**
 * The consumer sentence for a capability read that failed because no
 * provider serves it. The generated binding's message names the contract
 * (`ProviderNotConfigured(contractName=RoutingProvider)`); every surface
 * renders the shared `no '{provider}' provider is configured` sentence
 * instead of the binding's representation.
 */
function capabilityUnavailableText(error: unknown): string {
  const message = verbatimErrorText(error);
  // The binding's message for this variant can be either the full
  // `ProviderNotConfigured(contractName=…)` representation or just the
  // `contractName=…` field list, so the field is the stable signal.
  const contractName = /contractName=([^,)]+)/.exec(message)?.[1]?.trim();
  if (contractName === undefined) {
    return message;
  }
  return appStrings.providerNotConfiguredDetailTemplateText.replace(
    '{provider}',
    contractName,
  );
}

/**
 * The row's consumer-language status line (never echoes the credential). The
 * canonical status templates carry a `{provider}` slot, filled with the
 * registry descriptor's display name here at the call site (§2.5).
 */
function providerCredentialStatusText(
  providerDisplayName: string,
  rowState: ProviderCredentialRowState,
): string {
  if (rowState.statusKind === 'refused') {
    return (
      appStrings.providerCredentialRefusedLeadInText + rowState.refusalDetailText
    );
  }
  if (rowState.statusKind === 'applied') {
    const appliedStatusText = appStrings.providerCredentialAppliedStatusText.replace(
      '{provider}',
      providerDisplayName,
    );
    if (rowState.storageFailureText !== '') {
      return (
        appliedStatusText +
        appStrings.providerCredentialStorageFailureLeadInText +
        rowState.storageFailureText
      );
    }
    return (
      appliedStatusText +
      (rowState.isRememberEnabled
        ? appStrings.providerCredentialRememberedSuffixText
        : appStrings.providerCredentialSessionOnlySuffixText)
    );
  }
  return appStrings.providerCredentialKeylessStatusText.replace(
    '{provider}',
    providerDisplayName,
  );
}

export default function App(): React.JSX.Element {
  return (
    <SafeAreaProvider>
      <ReferenceAppScreen />
    </SafeAreaProvider>
  );
}

function ReferenceAppScreen(): React.JSX.Element {
  // The Compose application's `systemBarsPadding()` equivalent: every docked
  // overlay sits inside the system bars (the map surface itself is inset by
  // the package MapView's safe-area props).
  const safeAreaInsets = useSafeAreaInsets();
  // The safe-area library reports insets in density-independent units; the
  // map host's paddings are physical pixels (the surface extent the renderer
  // validates), so the values are scaled by the screen's pixel ratio.
  const densityScale = PixelRatio.get();
  const screenDimensions = useWindowDimensions();
  /** The phone-width rule (§1): the half state takes its phone cap. */
  const isPhoneWidth = screenDimensions.width <= phoneWidthBreakpointPixels;
  const sheetFullHeight = Math.round(screenDimensions.height * sheetFullHeightFraction);
  const halfHeightFraction = isPhoneWidth
    ? sheetHalfPhoneHeightFraction
    : sheetHalfHeightFraction;
  const sheetHalfHeight = Math.round(screenDimensions.height * halfHeightFraction);
  // --- The recorded demo plan (§4.1): the artifact's own walk is the default
  // selection; the peek's start action needs a selected plan (D8). ---
  const artifactDemoPlan =
    demoPlans.find(demoPlan => demoPlan.isArtifactTrace) ?? null;
  const [selectedPlanAssetName, setSelectedPlanAssetName] = useState<string | null>(
    artifactDemoPlan?.traceAssetName ?? null,
  );
  const selectedPlanAssetNameRef = useRef(artifactDemoPlan?.traceAssetName ?? null);
  /** §4.1/D8: the guided walk needs the native replay seam and a selected plan. */
  const canStartGuidedWalk =
    hasNativeController() && selectedPlanAssetName !== null;
  const guidedWalkUnavailableReason = !hasNativeController()
    ? appStrings.guidedWalkUnavailableNoNativeText
    : selectedPlanAssetName === null
      ? appStrings.guidedWalkUnavailableNoPlanText
      : null;
  /**
   * The plan the peek's Start would replay before a route is planned: the
   * selected demo plan, named by the peek's trip-source line so a start
   * without a plan is never mistaken for a live route
   * (`plan.trip_source_demo_plan`).
   */
  const selectedDemoPlan =
    demoPlans.find(demoPlan => demoPlan.traceAssetName === selectedPlanAssetName) ??
    null;
  /**
   * The peek's content-driven height (§2.4's exactly two rows), never a
   * fraction and never a literal.
   */
  const peekHeight = peekDockHeight();
  // The translateY offsets of the settled states over the constant-height
  // panel: the visible window is always the panel's top, so the content
  // swaps by settled state and never animates a height.
  const peekOffset = sheetFullHeight - peekHeight;
  const halfOffset = sheetFullHeight - sheetHalfHeight;
  const offsetOfState = useCallback(
    (state: SheetState): number => (state === 'peek' ? peekOffset : state === 'half' ? halfOffset : 0),
    [peekOffset, halfOffset],
  );

  // --- The theme model (§3: Light, Dark or Auto; Auto default) ---
  // Auto follows the operating system's scheme live; an explicit Light or
  // Dark choice ignores it from then on and is remembered on the device.
  const systemThemeName = useColorScheme();
  const [themeChoice, setThemeChoice] = useState<ThemeChoice>(defaultThemeChoice);
  const themeName = resolvedThemeName(themeChoice, systemThemeName);
  const themeColors = themeColorsByThemeName[themeName];
  const styles = useMemo(() => buildScreenStyles(themeColors), [themeColors]);

  // --- Remembered choices (one versioned record; docs/ReferenceAppUX.md §3) ---
  // The record is read once at mount and written on every explicit choice.
  // Transient state and provider credentials never enter it.
  const [preferences, setPreferences] = useState<ReferenceAppPreferences>(
    defaultReferenceAppPreferences,
  );
  const preferencesRef = useRef<ReferenceAppPreferences>(preferences);
  /** Whether the mount-time restore settled (a missing store counts). */
  const [arePreferencesHydrated, setArePreferencesHydrated] = useState(false);
  const rememberPreferences = useCallback(
    (update: (current: ReferenceAppPreferences) => ReferenceAppPreferences) => {
      const nextPreferences = update(preferencesRef.current);
      preferencesRef.current = nextPreferences;
      setPreferences(nextPreferences);
      const module = refappMapModule;
      if (module && typeof module.storePreferences === 'function') {
        module.storePreferences(JSON.stringify(nextPreferences));
      }
    },
    [],
  );

  const cycleThemeChoice = useCallback(() => {
    const nextChoice = nextThemeChoice(themeChoice);
    setThemeChoice(nextChoice);
    rememberPreferences(currentPreferences => ({
      ...currentPreferences,
      themeChoice: nextChoice,
    }));
  }, [themeChoice, rememberPreferences]);

  // --- The readiness ladder (§6.1) ---
  const [readiness, setReadiness] = useState<ReadinessEvent>(initialReadiness);
  // --- The verbatim error report (§6.3) ---
  const [errorReport, setErrorReport] = useState<ErrorReportEvent | null>(null);
  // --- Diagnostics sheet contents ---
  const [diagnosticsVisible, setDiagnosticsVisible] = useState(false);
  const [diagnosticsLines, setDiagnosticsLines] = useState<string[]>([]);
  /**
   * The opt-in stream's coalesced window (ReferenceApplicationBehavior.md,
   * diagnostics policy). It stays empty and unsubscribed until the pane
   * opens; the controller owns the subscription, cadence and window.
   */
  const [diagnosticEvents, setDiagnosticEvents] = useState<DiagnosticEvent[]>([]);
  const [diagnosticDebugEvents, setDiagnosticDebugEvents] = useState(false);
  // --- The developer console (M58) ---
  // It reads the same opt-in window as the diagnostics sheet. The control's
  // dot counts a Warning or Error newer than the watermark the console last
  // showed; the sheet keeps only its level filter and selected row.
  const [isDeveloperConsoleOpen, setIsDeveloperConsoleOpen] = useState(false);
  const [developerConsoleAlertWatermark, setDeveloperConsoleAlertWatermark] =
    useState(0);
  const [trafficCacheStatistics, setTrafficCacheStatistics] =
    useState<TrafficFlowCacheStatistics | null>(null);
  const [tileCacheStatistics, setTileCacheStatistics] =
    useState<TileCacheStatistics | null>(null);
  /**
   * The confirmed map taps the native view reported this session (§5.2: a
   * double tap zooms and reports no click of its own). The diagnostics sheet
   * renders the count beside the native camera lines.
   */
  const [mapTapCount, setMapTapCount] = useState(0);
  // --- The settings surface (§3's runtime provider credentials) ---
  const [settingsVisible, setSettingsVisible] = useState(false);
  const [providerCredentialsState, dispatchProviderCredential] = useReducer(
    providerCredentialReducer,
    credentialDescriptors,
    createInitialProviderCredentialsState,
  );
  // The settings sheet's honest keyless line shows only while no row applied
  // a credential (no key is a first-class state, §3). The keyless-start effect
  // reads it too, so it is derived here rather than near the sheet.
  const hasAppliedProviderCredential = credentialDescriptors.some(descriptor => {
    const rowState = providerCredentialsState.rows[descriptor.provider_key];
    return rowState !== undefined && isProviderCredentialApplied(rowState);
  });
  /**
   * The credential the running session holds for a provider key, read from
   * the settings row that applied (or restored) it. It never leaves this
   * process and is never written to the preferences record.
   */
  const appliedCapabilityProviderCredential = useCallback(
    (providerKey: string): string | null => {
      const rowState = providerCredentialsState.rows[providerKey];
      if (rowState === undefined || !isProviderCredentialApplied(rowState)) {
        return null;
      }
      const credential = rowState.enteredCredential.trim();
      return credential === '' ? null : credential;
    },
    [providerCredentialsState.rows],
  );
  // --- The per-capability provider rows (docs/design/ProviderCoverage.md §Per-capability selection) ---
  const [capabilityProviderRows, setCapabilityProviderRows] = useState<
    CapabilityProviderRow[]
  >([]);
  /**
   * The SDK's raw registration readback, kept beside the presentation rows so
   * the type-ahead's provider choice reads the same serving state the
   * settings surface shows.
   */
  const [providerRegistrations, setProviderRegistrations] = useState<
    ProviderRegistration[]
  >([]);
  const [capabilityProviderDetailTexts, setCapabilityProviderDetailTexts] =
    useState<ReadonlyMap<ProviderContract, string>>(new Map());
  /** Whether the remembered per-capability choices have been re-applied. */
  const hasRestoredCapabilityProvidersRef = useRef(false);
  // --- The plan sheet's Directions control (PRD.md §6 item 36) ---
  const [routeCapabilities, setRouteCapabilities] =
    useState<RouteCapabilities | null>(null);
  const [routeCapabilitiesUnavailableText, setRouteCapabilitiesUnavailableText] =
    useState('');
  const directionsSelection = useMemo(
    () =>
      directionsSelectionFromNames(
        preferences.directionsTransportationMode,
        preferences.directionsRouteStrategy,
      ),
    [
      preferences.directionsTransportationMode,
      preferences.directionsRouteStrategy,
    ],
  );
  const directionsControl = useMemo(
    () =>
      directionsControlState(
        directionsSelection,
        routeCapabilities,
        routeCapabilitiesUnavailableText,
      ),
    [directionsSelection, routeCapabilities, routeCapabilitiesUnavailableText],
  );
  /** The package MapView's imperative handle (the base-map switch reloads it). */
  const mapViewRef = useRef<MapViewHandle | null>(null);
  // Device positioning: the host source feeds the Rust controller; the
  // shell polls the latest fix and presents it through the renderer's layer.
  const [isDeviceLocationActive, setIsDeviceLocationActive] = useState(false);
  const [deviceLocationStatusText, setDeviceLocationStatusText] = useState<string | null>(null);
  const deviceLocationIntervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
  /**
   * The puck's interpolation and the app shell's frame clock (M72): each
   * accepted fix starts an eased motion, and the frame clock draws the
   * interpolated pose until it settles. The Rust crate remains the reference
   * model; this shell mirrors it because the Turbo Module exposes no seam.
   */
  const locationPoseAnimatorRef = useRef(new LocationPoseAnimator());
  const locationFrameClockRef = useRef(new FrameClock());
  /**
   * Whether guidance currently follows the device source's own fixes on the
   * planned route (the Location group's second control). A ref mirrors the
   * state for the polling interval, which is created once per source start.
   */
  const [isDeviceNavigationActive, setIsDeviceNavigationActive] = useState(false);
  const isDeviceNavigationActiveRef = useRef(false);
  /**
   * Whether starting device navigation also started the location source, so
   * stopping the mode does not turn off a source the visitor chose.
   */
  const deviceNavigationStartedLocationSourceRef = useRef(false);
  /** The one package SDK handle, created once at mount (shared process-wide). */
  const sdkInstanceRef = useRef<MapsSdk | null>(null);
  /** Whether the package SDK has been created (the keyless start waits on it). */
  const [isSdkCreated, setIsSdkCreated] = useState(false);
  /**
   * Whether the stored-credential restore has settled (found nothing, restored
   * a value, or the secure store is absent). The keyless start waits on it so
   * a remembered key is never overridden by the default base.
   */
  const [hasCredentialRestoreSettled, setHasCredentialRestoreSettled] =
    useState(false);
  /** The keyless base map installs once per session. */
  const hasAppliedKeylessStartRef = useRef(false);
  /** Whether the launch-argument credential check has settled. */
  const [isLaunchCredentialChecked, setIsLaunchCredentialChecked] =
    useState(false);
  /**
   * The registry's keyless styled base while the package MapView has not
   * installed it yet (the base registers after the credential checks settle;
   * the map's own ready callback retries if the view was not ready then).
   */
  const pendingKeylessVectorBaseChoiceRef = useRef<VectorBaseMapChoice | null>(
    null,
  );
  // --- The base-map switch (M9): the serving source's selection ---
  const [selectedBaseMapProviderKey, setSelectedBaseMapProviderKey] =
    useState<string | null>(null);
  // --- The styled vector base: the active choice's key, never the
  // registry object, so a registry refresh cannot keep a dropped style alive.
  const [selectedVectorBaseChoiceKey, setSelectedVectorBaseChoiceKey] =
    useState<string | null>(null);
  /** The last registered cache line, restored when a keyed base map returns. */
  const registeredCacheStateRef = useRef<CacheStateEvent | null>(null);
  // --- Guided-walk demo state (§7) ---
  const [traceActive, setTraceActive] = useState(false);
  const [traceUpdate, setTraceUpdate] = useState<TraceUpdateEvent | null>(null);
  const [playbackState, setPlaybackState] =
    useState<PlaybackStateEvent>(initialPlaybackState);
  const [deviationText, setDeviationText] = useState<string | null>(null);
  // --- The plan flow's state (§4.1) ---
  const [planState, setPlanState] = useState<PlanStateEvent>(initialPlanState);
  // Whether the offline card's route control is waiting on its plan (the
  // control's disabled state; the native plan-state event carries the rest).
  const [offlineRouteInFlight, setOfflineRouteInFlight] = useState(false);
  // --- The offline region download ---
  const [storedOfflineRegions, setStoredOfflineRegions] = useState<RegionRecord[]>([]);
  const [offlineRegionTileCount, setOfflineRegionTileCount] = useState<number | null>(null);
  const [offlineRegionProgress, setOfflineRegionProgress] =
    useState<RegionDownloadProgress | null>(null);
  const [offlineRegionStatus, setOfflineRegionStatus] = useState<
    'idle' | 'downloading' | 'completed' | 'cancelled' | 'failed'
  >('idle');
  const [offlineRegionErrorText, setOfflineRegionErrorText] = useState<string | null>(null);
  const [offlineRegionInFlight, setOfflineRegionInFlight] = useState(false);
  const [offlineRegionDeleted, setOfflineRegionDeleted] = useState(false);
  // --- Spoken guidance (§4.2's speech consumer; the contract) ---
  const speechConsumer = useMemo(
    () =>
      createSpeechConsumer({
        speak: (speechText, engineUtteranceId) => refappMapModule?.speakUtterance(speechText, engineUtteranceId),
        stopSpeech: () => refappMapModule?.stopSpeech(),
      }),
    [],
  );
  const [speechConsumerState, setSpeechConsumerState] =
    useState<SpeechConsumerState>(initialSpeechConsumerState);
  const [speechEngineState, setSpeechEngineState] = useState<SpeechEngineStateEvent>({
    isEngineReady: false,
  });
  const [cacheState, setCacheState] = useState<CacheStateEvent | null>(null);
  const [tileStatistics, setTileStatistics] = useState<FrameStatisticsEvent | null>(null);
  // --- The map control cluster's read-only camera state (§4.2) ---
  const [cameraReadback, setCameraReadback] = useState<CameraReadbackEvent | null>(null);
  const [isAttributionPanelOpen, setIsAttributionPanelOpen] = useState(false);
  // --- The on-map layer control (docs/ReferenceAppUX.md §4.2) ---
  const [mapLayersState, dispatchMapLayers] = useReducer(
    mapLayersPanelReducer,
    undefined,
    initialMapLayersPanelState,
  );
  /** The collapsed control, the focus target when the panel closes. */
  const mapLayersControlRef = useRef<MapLayersControlRef | null>(null);
  /** The last camera the map view adopted (the traffic layer's query box). */
  const mapCameraPoseRef = useRef<MapCameraPose | null>(null);
  /** The last requested pose key; a camera change restarts the debounce. */
  const mapCameraPoseKeyRef = useRef('');
  const [mapCameraPoseKey, setMapCameraPoseKey] = useState('');
  /** Re-runs the debounced refresh after a coalesced request completes. */
  const [trafficRefreshNonce, setTrafficRefreshNonce] = useState(0);
  const trafficRefreshInFlightRef = useRef(false);
  const trafficRefreshPendingRef = useRef(false);
  /** Invalidates answers that belong to a layer state the user already left. */
  const trafficRequestGenerationRef = useRef(0);
  /** The remembered traffic choice restores at most once per session. */
  const hasRestoredTrafficLayerRef = useRef(false);
  const [sheetState, setSheetState] = useState<SheetState>('peek');
  /**
   * The navigation drawer's measured height: the controls and the attribution
   * dock above whatever bottom surface is present, so the trip dock's height
   * is read from its rendered rows rather than guessed (D10/D6).
   */
  const [navigationDrawerHeight, setNavigationDrawerHeight] = useState(0);
  const [contentMode, setContentMode] = useState<SheetContentMode>('home');
  const [activePlanSearchField, setActivePlanSearchField] =
    useState<PlanSearchField>('destination');
  const [originSearchText, setOriginSearchText] = useState('');
  const [destinationSearchText, setDestinationSearchText] = useState('');
  const [searchRows, setSearchRows] = useState<PlaceRow[]>([]);
  const [searchInFlight, setSearchInFlight] = useState(false);
  // The keyless search's honest no-match line (§4.1): set when the artifact
  // filter answered no demo place, cleared by the next search.
  const [demoSearchNoMatchText, setDemoSearchNoMatchText] = useState<string | null>(null);
  // Whether the running SDK session has a provider credential applied. The
  // bridge answers at mount so a launch-environment injected key is honored;
  // a keyless session offers the artifact's demo places and recorded routes.
  const [hasSessionProviderCredential, setHasSessionProviderCredential] =
    useState(false);
  const [assignedOriginText, setAssignedOriginText] = useState<string | null>(null);
  const [assignedDestinationText, setAssignedDestinationText] = useState<string | null>(null);
  // The resolved rows behind each field's assignment, so a swap can move the
  // two resolved places with the two texts.
  const [assignedOriginPlace, setAssignedOriginPlace] = useState<PlaceRow | null>(null);
  const [assignedDestinationPlace, setAssignedDestinationPlace] =
    useState<PlaceRow | null>(null);
  /**
   * Which plan fields a My location chip filled this session, so the chip
   * marks the field it filled and a swap moves the mark with its text.
   */
  const [myLocationFilledFields, setMyLocationFilledFields] = useState<{
    origin: boolean;
    destination: boolean;
  }>({ origin: false, destination: false });
  /**
   * The most recent device fix's display text, or null before the first fix.
   * It is what a plan field's My location chip fills (never the map camera).
   */
  const [latestFixDisplayText, setLatestFixDisplayText] = useState<string | null>(null);
  /**
   * The plan field a My location chip asked to fill while the source was
   * still waiting for its first fix. The poll loop fills exactly that field
   * when the fix lands, mirroring the iOS chip's pending-fill behavior.
   */
  const pendingMyLocationFieldRef = useRef<PlanSearchField | null>(null);
  const [sceneInfoSceneKey, setSceneInfoSceneKey] = useState<string | null>(null);
  // --- Camera presets ---
  const [activePresetId, setActivePresetId] = useState<string | null>(null);
  // The compiled-version probe (the cdylib through the module),
  // taken once on mount and surfaced in the diagnostics sheet.
  const [compiledVersion, setCompiledVersion] = useState<string | null>(null);

  // The controller's one event channel.
  useEffect(() => {
    const subscriptions = [
      addControllerListener('RefappReadiness', (payload: Record<string, unknown>) =>
        setReadiness(payload as unknown as ReadinessEvent),
      ),
      addControllerListener('RefappErrorReport', (payload: Record<string, unknown>) => {
        const report = payload as unknown as ErrorReportEvent;
        setErrorReport(report.variantLabel === '' ? null : report);
      }),
      addControllerListener('RefappTraceState', (payload: Record<string, unknown>) => {
        setTraceActive(Boolean(payload.isActive));
        // A replay owns its guidance: starting or ending one drops the last
        // replay's update, whose arrival line and "Arrived" phase would
        // otherwise sit beside the new trip's counting ETA until the first
        // update lands.
        setTraceUpdate(null);
      }),
      addControllerListener('RefappTraceUpdate', (payload: Record<string, unknown>) => {
        setTraceUpdate(payload as unknown as TraceUpdateEvent);
      }),
      addControllerListener('RefappPlaybackState', (payload: Record<string, unknown>) => {
        setPlaybackState(payload as unknown as PlaybackStateEvent);
      }),
      addControllerListener('RefappDeviationAlert', (payload: Record<string, unknown>) => {
        setDeviationText(String(payload.text ?? ''));
      }),
      addControllerListener('RefappPlanState', (payload: Record<string, unknown>) => {
        setPlanState(payload as unknown as PlanStateEvent);
      }),
      addControllerListener('RefappCacheState', (payload: Record<string, unknown>) => {
        const nextCacheState = payload as unknown as CacheStateEvent;
        // Remember the last registered line: a keyed switch-back re-registers
        // the same cache (same file and budget), so the offline card can show
        // the genuine line again instead of a guess.
        if (nextCacheState.isRegistered) {
          registeredCacheStateRef.current = nextCacheState;
        }
        setCacheState(nextCacheState);
      }),
      addControllerListener('RefappFrameStatistics', (payload: Record<string, unknown>) => {
        setTileStatistics(payload as unknown as FrameStatisticsEvent);
      }),
      addControllerListener('RefappSpeechInstruction', (payload: Record<string, unknown>) => {
        // The contract's delivery crossing verbatim; the consumer
        // owns the dedup, pacing and toggle decisions (the jest-tested half).
        const delivery = payload as unknown as SpeechInstructionEvent;
        speechConsumer.consumeDelivery({
          speechText: delivery.speechText ?? null,
          speechInstructionId: delivery.speechInstructionId ?? null,
          instructionKind:
            delivery.instructionKind ?? speechDeliveryKindLabels.maneuverApproach,
        });
      }),
      addControllerListener('RefappSpeechState', (payload: Record<string, unknown>) => {
        setSpeechEngineState(payload as unknown as SpeechEngineStateEvent);
      }),
      addControllerListener('RefappCameraReadback', (payload: Record<string, unknown>) => {
        setCameraReadback(payload as unknown as CameraReadbackEvent);
      }),
    ];
    return () => subscriptions.forEach(removeSubscription => removeSubscription());
  }, [speechConsumer]);

  // The consumer's state rides the drawer's speech row (§4.2's toggle).
  useEffect(() => speechConsumer.onStateChange(setSpeechConsumerState), [speechConsumer]);
  // The ready line carries the compiled version the package SDK answers; the
  // version promise can settle after the first frame's ready callback.
  useEffect(() => {
    if (compiledVersion === null) {
      return;
    }
    setReadiness(previousReadiness =>
      previousReadiness.state === 'ready'
        ? {
            ...previousReadiness,
            text:
              appStrings.readinessReadyPrefixText + compiledVersion + ')',
          }
        : previousReadiness,
    );
  }, [compiledVersion]);


  // SDK start (the facade + surface + tile cache), the demo's initial framing
  // (the artifact's first preset, the Compose application's composition start
  // framing), and the probe run once.
  useEffect(() => {
    // The package owns the one native MapSdk; creating it also applies the
    // launch-argument automation credential through the public registration
    // path. The demo session then loads its shared scene artifact.
    MapsSdk.create()
      .then(sdk => {
        sdkInstanceRef.current = sdk;
        setIsSdkCreated(true);
        sdk.getCompiledVersion().then(version => setCompiledVersion(version));
        refappMapModule?.createSdk();
        // The session's credential state decides the keyless demo flow: a
        // launch-environment injected key makes the provider path stand,
        // exactly as an explicitly applied key does.
        refappMapModule
          ?.isProviderCredentialApplied?.()
          .then(setHasSessionProviderCredential)
          .catch(() => undefined)
          .finally(() => setIsLaunchCredentialChecked(true));
      })
      .catch(startupError => {
        // No console mirror: the failure reaches the visitor's error strip
        // verbatim and the diagnostics pane through the SDK's own stream.
        setErrorReport({
          variantLabel: 'MapsSdkStart',
          detailText: String(startupError),
        });
      });
    // The speech engine's state is read back once at mount: its
    // initialization event can fire before these listeners subscribe (§6.2).
    refappMapModule?.getSpeechEngineState?.().then(engineState => {
      setSpeechEngineState(engineState);
    });
    // The initial framing waits for the remembered choices (the startup
    // restore effect below), so a remembered preset wins over the default.
  }, []);

  /**
   * Reads the SDK's per-capability provider readback and rebuilds every
   * settings row. A readback that cannot answer leaves no rows rather than
   * inventing a serving provider.
   */
  const loadCapabilityProviderRows = useCallback(
    async (
      detailTexts: ReadonlyMap<ProviderContract, string>,
    ): Promise<void> => {
      const sdk = sdkInstanceRef.current;
      if (sdk === null) {
        setCapabilityProviderRows([]);
        setProviderRegistrations([]);
        return;
      }
      try {
        const registrations = await sdk.providerRegistrations();
        setProviderRegistrations(registrations);
        setCapabilityProviderRows(
          buildCapabilityProviderRows(
            registrations,
            providerDescriptors,
            appliedCapabilityProviderCredential,
            detailTexts,
          ),
        );
      } catch (readbackError) {
        setErrorReport({
          variantLabel: 'ProviderReadback',
          detailText: String(readbackError),
        });
        setCapabilityProviderRows([]);
      }
    },
    [appliedCapabilityProviderCredential],
  );

  // The rows follow the SDK's readback: once the session exists, and again
  // whenever the applied credential changes (a choice's credential reason
  // retires) or a selection records a new detail.
  useEffect(() => {
    if (!isSdkCreated) {
      return;
    }
    void loadCapabilityProviderRows(capabilityProviderDetailTexts);
  }, [
    isSdkCreated,
    loadCapabilityProviderRows,
    capabilityProviderDetailTexts,
  ]);

  // The Directions control reads the serving routing provider's capabilities
  // whenever the plan sheet opens and after any provider change (the rows
  // refresh), so an unsupported choice is disabled with its reason.
  useEffect(() => {
    if (!isSdkCreated) {
      return;
    }
    const sdk = sdkInstanceRef.current;
    if (sdk === null) {
      return;
    }
    let isCancelled = false;
    sdk
      .routeCapabilities()
      .then(capabilities => {
        if (!isCancelled) {
          setRouteCapabilities(capabilities);
          setRouteCapabilitiesUnavailableText('');
        }
      })
      .catch(capabilitiesError => {
        if (!isCancelled) {
          setRouteCapabilities(null);
          setRouteCapabilitiesUnavailableText(
            capabilityUnavailableText(capabilitiesError),
          );
        }
      });
    return () => {
      isCancelled = true;
    };
  }, [isSdkCreated, contentMode, capabilityProviderRows]);

  /**
   * Registers one provider credential. A provider that declares a raster-tile
   * caching ceiling installs its raster slot through the disk cache; every
   * other provider registers one capability per call through the catalog, so
   * the row's own key and served contracts decide what is installed.
   */
  const registerProviderCredential = useCallback(
    async (
      descriptor: ProviderCredentialDescriptor,
      credential: string,
    ): Promise<{ isApplied: boolean; detailText: string }> => {
      if (descriptor.raster_tile_cache_ceiling_days !== undefined) {
        if (refappMapModule === null) {
          return {
            isApplied: false,
            detailText: 'the native module is unavailable',
          };
        }
        return refappMapModule.setProviderCredential(
          descriptor.provider_key,
          credential,
        );
      }
      const sdk = sdkInstanceRef.current;
      if (sdk === null) {
        return { isApplied: false, detailText: 'the SDK is not running' };
      }
      const servedContracts = (descriptor.served_contracts ?? [])
        .map((registryName) =>
          capabilityContractForRegistryName(registryName),
        )
        .filter(
          (contract): contract is ProviderContract => contract !== null,
        );
      if (servedContracts.length === 0) {
        return {
          isApplied: false,
          detailText:
            `${descriptor.display_name} declares no capability this build can register.`,
        };
      }
      try {
        for (const contract of servedContracts) {
          await sdk.configureProvider(
            contract,
            descriptor.provider_key,
            credential,
          );
        }
        return { isApplied: true, detailText: '' };
      } catch (applyError) {
        return { isApplied: false, detailText: String(applyError) };
      }
    },
    [],
  );

  // The opt-in restored credential (§3's mount rule): a value can only be in
  // the secure store behind a previous "Remember on this device" opt-in, so
  // its presence is the opt-in and the row is re-applied automatically. An
  // absent store or value leaves the honest keyless state untouched.
  useEffect(() => {
    if (!refappMapModule) {
      setHasCredentialRestoreSettled(true);
      return;
    }
    // Each descriptor's restore settles independently, but the keyless start
    // waits for the whole pass: a remembered key that applies a moment later
    // must not be overridden by the default base map.
    let isCancelled = false;
    const restoreStoredCredential = async (
      descriptor: ProviderCredentialDescriptor,
    ): Promise<void> => {
      let storedCredential: string | null | undefined;
      try {
        storedCredential = await refappMapModule?.getStoredProviderCredential(
          descriptor.provider_key,
        );
      } catch {
        // The secure store is absent or refused: the app still starts
        // keyless and the settings row reports it honestly.
        return;
      }
      if (
        storedCredential === null ||
        storedCredential === undefined ||
        storedCredential.trim() === ''
      ) {
        return;
      }
      dispatchProviderCredential({
        type: 'storedCredentialRestored',
        providerKey: descriptor.provider_key,
        credential: storedCredential,
      });
      try {
        const applyResult = await registerProviderCredential(
          descriptor,
          storedCredential,
        );
        if (applyResult?.isApplied) {
          dispatchProviderCredential({
            type: 'applySucceeded',
            providerKey: descriptor.provider_key,
          });
          setHasSessionProviderCredential(true);
        } else if (applyResult) {
          dispatchProviderCredential({
            type: 'applyRefused',
            providerKey: descriptor.provider_key,
            refusalDetailText: applyResult.detailText,
          });
        }
      } catch (applyError) {
        dispatchProviderCredential({
          type: 'applyRefused',
          providerKey: descriptor.provider_key,
          refusalDetailText: String(applyError),
        });
      }
    };
    void Promise.all(
      credentialDescriptors
        .filter(isProviderCredentialRegistrationAvailable)
        .map(restoreStoredCredential),
    ).finally(() => {
      if (!isCancelled) {
        setHasCredentialRestoreSettled(true);
      }
    });
    return () => {
      isCancelled = true;
    };
  }, [registerProviderCredential]);

  /**
   * Installs one styled-vector-base choice on the map view: the provider's
   * published style document through the asynchronous provider-document
   * member when the style names one, else the committed-style member.
   * Resolves whether the SDK accepted the install.
   */
  const installVectorBaseChoice = useCallback(
    async (
      mapView: MapViewHandle,
      choice: VectorBaseMapChoice,
    ): Promise<boolean> => {
      const hostedInstall = hostedVectorBaseInstall(choice);
      if (hostedInstall !== null) {
        return mapView.setProviderVectorTileSourceFromDocument(
          hostedInstall.source,
          hostedInstall.styleDocumentUrl,
        );
      }
      return mapView.setVectorTileSource(vectorBaseSource(choice));
    },
    [],
  );

  /**
   * Installs a staged keyless styled base once the package MapView handle
   * exists (its own ready callback retries after a re-attach). A refusal
   * leaves no selection recorded, so the layers panel row stays truthful.
   */
  const installPendingKeylessVectorBase = useCallback(() => {
    const choice = pendingKeylessVectorBaseChoiceRef.current;
    const mapView = mapViewRef.current;
    if (choice === null || mapView === null) {
      return;
    }
    pendingKeylessVectorBaseChoiceRef.current = null;
    installVectorBaseChoice(mapView, choice)
      .then(accepted => {
        if (!accepted) {
          return;
        }
        setSelectedVectorBaseChoiceKey(vectorBaseChoiceKey(choice));
        setErrorReport(null);
      })
      .catch(() => undefined);
  }, [installVectorBaseChoice]);

  /**
   * The remembered styled base staged until the package MapView handle
   * exists (the same ready callback retries after a re-attach). A refusal
   * leaves no selection recorded, so the layers panel row stays truthful.
   */
  const pendingRememberedVectorBaseChoiceRef = useRef<VectorBaseMapChoice | null>(
    null,
  );
  const installPendingRememberedVectorBase = useCallback(() => {
    const choice = pendingRememberedVectorBaseChoiceRef.current;
    const mapView = mapViewRef.current;
    if (choice === null || mapView === null) {
      return;
    }
    pendingRememberedVectorBaseChoiceRef.current = null;
    installVectorBaseChoice(mapView, choice)
      .then(accepted => {
        if (!accepted) {
          return;
        }
        setSelectedVectorBaseChoiceKey(vectorBaseChoiceKey(choice));
        setErrorReport(null);
      })
      .catch(() => undefined);
  }, [installVectorBaseChoice]);

  // The keyless start: once the SDK
  // exists, the remembered choices are known and the credential checks have
  // settled, a session with no credential registers the registry's
  // credential-less raster source (the package view attaches through the
  // raster provider contract) and then installs the registry's declared
  // styled vector base over it. A launch or stored credential wins, and a
  // remembered base map restores below over the raster fallback instead of
  // the keyless vector default.
  useEffect(() => {
    if (hasAppliedKeylessStartRef.current) {
      return;
    }
    if (
      !arePreferencesHydrated ||
      !isSdkCreated ||
      !hasCredentialRestoreSettled ||
      !isLaunchCredentialChecked
    ) {
      return;
    }
    if (hasSessionProviderCredential || hasAppliedProviderCredential) {
      return;
    }
    const sdk = sdkInstanceRef.current;
    if (sdk === null) {
      return;
    }
    // Whether the remembered base map restores in this keyless session: a
    // vector style by its choice key, or a credential-less raster provider.
    // A credential provider's remembered raster cannot serve without a key,
    // so the keyless default stands until a key is applied.
    const rememberedBaseMap = preferences.baseMap;
    const hasRestorableRememberedBaseMap =
      rememberedBaseMap !== null &&
      (rememberedBaseMap.styleId !== null
        ? vectorBaseMapChoices(providerDescriptors).some(
            choice =>
              vectorBaseChoiceKey(choice) ===
              `${rememberedBaseMap.providerKey}/${rememberedBaseMap.styleId}`,
          )
        : baseMapChoices(providerDescriptors, new Set<string>()).some(
            choice =>
              choice.isServable &&
              choice.descriptor.provider_key === rememberedBaseMap.providerKey,
          ));
    const keylessRasterChoice = baseMapChoices(
      providerDescriptors,
      new Set<string>(),
    ).find(choice => choice.isServable);
    if (keylessRasterChoice === undefined) {
      return;
    }
    hasAppliedKeylessStartRef.current = true;
    const registrationPlan = baseMapRegistrationPlan(
      keylessRasterChoice.descriptor,
      providerReferenceAppUserAgent(compiledVersion ?? ''),
    );
    if (registrationPlan.kind !== 'urlTemplate') {
      return;
    }
    sdk
      .registerUrlTemplateRasterTileSource(registrationPlan.source)
      .then(registrationResult => {
        if (!registrationResult.isApplied) {
          setErrorReport({
            variantLabel: appStrings.baseMapSectionHeaderText,
            detailText: registrationResult.detailText,
          });
          return;
        }
        // A credential-less source serves uncached: the offline card states
        // it rather than keeping a previous promise (mirrors a manual pick).
        setCacheState({
          isRegistered: false,
          stateText: appStrings.baseMapUncachedNoteText,
        });
        if (hasRestorableRememberedBaseMap) {
          // The remembered explicit selection restores below and wins; the
          // raster registration stays as its fallback. Staging the keyless
          // vector default here would overwrite that selection.
          return;
        }
        setSelectedBaseMapProviderKey(keylessRasterChoice.descriptor.provider_key);
        mapViewRef.current?.clearVectorTileSource();
        setSelectedVectorBaseChoiceKey(null);
        mapViewRef.current?.reloadTileSource();
        pendingKeylessVectorBaseChoiceRef.current = keylessVectorBaseChoice(
          providerDescriptors,
          providerRegistry.keyless_base_map,
        );
        installPendingKeylessVectorBase();
      })
      .catch(registrationError => {
        setErrorReport({
          variantLabel: appStrings.baseMapSectionHeaderText,
          detailText: String(registrationError),
        });
      });
  }, [
    arePreferencesHydrated,
    preferences.baseMap,
    isSdkCreated,
    hasCredentialRestoreSettled,
    isLaunchCredentialChecked,
    hasSessionProviderCredential,
    hasAppliedProviderCredential,
    compiledVersion,
    installPendingKeylessVectorBase,
  ]);

  // --- The sheet settle control (the dock's state transitions; declared
  // before the back handler, whose grown-state back closes a state through the
  // same settle path) ---
  const translateYValue = useRef(new Animated.Value(peekOffset)).current;
  const settledSheetStateRef = useRef<SheetState>('peek');

  const settleToSheetState = useCallback(
    (nextState: SheetState) => {
      // Every sheet-state change takes this single settle path: the transform
      // springs to the state's offset while the state write moves whatever
      // rides the settled height (the attribution line, the back handler's
      // grown-state check) in lockstep.
      settledSheetStateRef.current = nextState;
      setSheetState(nextState);
      Animated.spring(translateYValue, {
        toValue: offsetOfState(nextState),
        useNativeDriver: true,
        speed: settleSpringSpeed,
        bounciness: settleSpringBounciness,
      } as never).start();
    },
    [offsetOfState, translateYValue],
  );

  // --- The hardware BackHandler (§6.2: back closes the sheet's grown states
  // before it can exit the app; a menu or modal over the sheet consumes back
  // first) ---
  useEffect(() => {
    const backSubscription = BackHandler.addEventListener('hardwareBackPress', () => {
      if (settingsVisible) {
        setSettingsVisible(false);
        return true;
      }
      if (diagnosticsVisible) {
        setDiagnosticsVisible(false);
        return true;
      }
      if (contentMode === 'search') {
        setContentMode('home');
        return true;
      }
      if (sheetState !== 'peek' && !traceActive) {
        // The dock springs to the peek dock (the same settle path every other
        // state change takes: a bare state write would leave the dock's
        // translate at the grown position behind the new state).
        settleToSheetState('peek');
        return true;
      }
      // Nothing of the sheet is grown: the app itself may exit.
      return false;
    });
    return () => backSubscription.remove();
  }, [diagnosticsVisible, settingsVisible, contentMode, sheetState, traceActive, settleToSheetState]);

  // Keeps the animation's resting point honest across a rotation (the dock's
  // viewport-fraction heights move); a settled-state change re-springs on its
  // own and must not be jumped here.
  const appliedDockHeightRef = useRef(0);
  useEffect(() => {
    if (appliedDockHeightRef.current !== sheetFullHeight) {
      appliedDockHeightRef.current = sheetFullHeight;
      translateYValue.setValue(offsetOfState(settledSheetStateRef.current));
    }
  }, [offsetOfState, sheetFullHeight, translateYValue]);

  const sheetPanResponder = useMemo(
    () =>
      PanResponder.create({
        onStartShouldSetPanResponder: () => false,
        onMoveShouldSetPanResponder: (gestureEvent, gestureState) =>
          Math.abs(gestureState.dy) > sheetDragRecognizePixels &&
          Math.abs(gestureState.dy) > Math.abs(gestureState.dx),
        onPanResponderMove: (gestureEvent, gestureState) => {
          // The drag maps the finger's travel onto the panel's travel through
          // the current state's offset; the release snaps one state per rule.
          const draggedOffset = offsetOfState(settledSheetStateRef.current) + gestureState.dy;
          translateYValue.setValue(Math.max(0, Math.min(peekOffset, draggedOffset)));
        },
        onPanResponderRelease: (gestureEvent, gestureState) => {
          const currentState = settledSheetStateRef.current;
          if (gestureState.dy < -sheetDragSnapPixels) {
            settleToSheetState(
              currentState === 'peek' ? 'half' : currentState === 'half' ? 'full' : 'full',
            );
          } else if (gestureState.dy > sheetDragSnapPixels) {
            settleToSheetState(
              currentState === 'full' ? 'half' : currentState === 'half' ? 'peek' : 'peek',
            );
          } else {
            settleToSheetState(currentState);
          }
        },
      }),
    [offsetOfState, peekOffset, settleToSheetState, translateYValue],
  );

  // --- The plan flow's controls (§4.1) ---
  /**
   * Opens the search view for one field from an explicit action. The fresh
   * open drops the previous field's result list; focusing a field that is
   * already visible keeps it (`focusPlanField`).
   */
  const openSearchView = useCallback((planSearchField: PlanSearchField) => {
    setActivePlanSearchField(planSearchField);
    setSearchRows([]);
    setDemoSearchNoMatchText(null);
    setContentMode('search');
    settleToSheetState('full');
  }, [settleToSheetState]);

  /**
   * Focuses one field: the assignment target follows focus and the search
   * view opens full, but the shared result list stays, exactly as focusing a
   * field on iOS keeps the last list.
   */
  const focusPlanField = useCallback((planSearchField: PlanSearchField) => {
    setActivePlanSearchField(planSearchField);
    setContentMode('search');
    settleToSheetState('full');
  }, [settleToSheetState]);

  /** Drops one field's My location fill mark when the visitor edits it. */
  const clearMyLocationFill = useCallback((planSearchField: PlanSearchField) => {
    setMyLocationFilledFields(currentFields =>
      currentFields[planSearchField]
        ? { ...currentFields, [planSearchField]: false }
        : currentFields,
    );
  }, []);

  const changeOriginSearchText = useCallback(
    (searchText: string) => {
      setOriginSearchText(searchText);
      rememberPreferences(currentPreferences => ({
        ...currentPreferences,
        search: { ...currentPreferences.search, origin: searchText },
      }));
    },
    [rememberPreferences],
  );

  const changeDestinationSearchText = useCallback(
    (searchText: string) => {
      setDestinationSearchText(searchText);
      rememberPreferences(currentPreferences => ({
        ...currentPreferences,
        search: { ...currentPreferences.search, destination: searchText },
      }));
    },
    [rememberPreferences],
  );

  /**
   * One field's own edit: the text changes, its My location fill mark drops,
   * and an assignment the text edited away from is cleared so the assigned
   * row stops reading as assigned and Plan route re-resolves from the text.
   * Programmatic fills (an assignment, a swap) use the text setters above.
   */
  const editOriginSearchText = useCallback(
    (searchText: string) => {
      changeOriginSearchText(searchText);
      clearMyLocationFill('origin');
      if (searchText !== assignedOriginText) {
        setAssignedOriginText(null);
        setAssignedOriginPlace(null);
      }
    },
    [assignedOriginText, changeOriginSearchText, clearMyLocationFill],
  );

  const editDestinationSearchText = useCallback(
    (searchText: string) => {
      changeDestinationSearchText(searchText);
      clearMyLocationFill('destination');
      if (searchText !== assignedDestinationText) {
        setAssignedDestinationText(null);
        setAssignedDestinationPlace(null);
      }
    },
    [
      assignedDestinationText,
      changeDestinationSearchText,
      clearMyLocationFill,
    ],
  );

  /** Stores one field's resolved row and its display text together. */
  const assignPlanFieldPlace = useCallback(
    (planSearchField: PlanSearchField, placeRow: PlaceRow) => {
      if (planSearchField === 'origin') {
        setAssignedOriginText(placeRow.structuredAddressText);
        setAssignedOriginPlace(placeRow);
        changeOriginSearchText(placeRow.structuredAddressText);
      } else {
        setAssignedDestinationText(placeRow.structuredAddressText);
        setAssignedDestinationPlace(placeRow);
        changeDestinationSearchText(placeRow.structuredAddressText);
      }
    },
    [changeDestinationSearchText, changeOriginSearchText],
  );

  /**
   * The type-ahead's provider choice (docs/ReferenceAppUX.md §5.5): the
   * credentialed provider when the settings surface configured a key, the
   * keyless search provider otherwise. Both come from the shared registry and
   * the SDK's readback, never a provider name in this shell.
   */
  const autocompleteProviderSelection =
    useMemo<AutocompleteProviderSelection | null>(
      () =>
        selectAutocompleteProvider({
          descriptors: providerDescriptors,
          registrations: providerRegistrations,
          credentialForProvider: appliedCapabilityProviderCredential,
          hasSessionProviderCredential,
        }),
      [
        appliedCapabilityProviderCredential,
        hasSessionProviderCredential,
        providerRegistrations,
      ],
    );

  /**
   * The SDK seam behind the fields' type-ahead: ensure the selected provider
   * serves place search, then ask the contract's suggest call. A selection
   * the readback already reports is not reconfigured.
   */
  const configuredAutocompleteProviderRef = useRef<string | null>(null);
  const autocompleteGateway = useMemo<PlanAutocompleteGateway>(
    () => ({
      suggest: async (queryText, providerSelection, signal) => {
        const sdk = sdkInstanceRef.current;
        if (sdk === null) {
          return [];
        }
        const selectionKey =
          providerSelection.providerKey + '\n' + providerSelection.credentialText;
        if (
          !providerSelection.isConfigured &&
          providerSelection.credentialText !== '' &&
          configuredAutocompleteProviderRef.current !== selectionKey
        ) {
          await sdk.configureProvider(
            ProviderContract.PlaceSearch,
            providerSelection.providerKey,
            providerSelection.credentialText,
          );
          configuredAutocompleteProviderRef.current = selectionKey;
        }
        if (signal.aborted) {
          return [];
        }
        const places = await sdk.suggest(queryText);
        if (signal.aborted) {
          return [];
        }
        return places.map(place => ({
          structuredAddressText: place.structuredAddressText,
          latitudeDegrees: place.coordinate.latitude,
          longitudeDegrees: place.coordinate.longitude,
          qualityText: GeocodeQuality[place.geocodeQuality].toLowerCase(),
        }));
      },
    }),
    [],
  );

  /**
   * Assigns one resolved row to the named field: a keyless demo row fills the
   * field directly, and a live row reaches the native session's assignment
   * first, exactly as a clicked search result does. Both a chosen suggestion
   * and a clicked result row land here, so Plan route resolves either as the
   * typed text does.
   */
  const assignPlanFieldRow = useCallback(
    (planSearchField: PlanSearchField, placeRow: PlaceRow) => {
      if (!hasSessionProviderCredential) {
        assignPlanFieldPlace(planSearchField, placeRow);
        return;
      }
      if (!refappMapModule) {
        return;
      }
      refappMapModule
        .assignSearchResult(
          planSearchField === 'origin',
          placeRow.structuredAddressText,
          placeRow.latitudeDegrees,
          placeRow.longitudeDegrees,
        )
        .then(assigned => {
          if (!assigned) {
            return;
          }
          assignPlanFieldPlace(planSearchField, placeRow);
        })
        .catch(assignmentError => {
          setErrorReport({
            variantLabel: 'PlanAssignment',
            detailText: String(assignmentError),
          });
        });
    },
    [assignPlanFieldPlace, hasSessionProviderCredential],
  );

  const runPlanSearch = useCallback((planSearchField: PlanSearchField, queryText: string) => {
    if (queryText.trim() === '') {
      return;
    }
    setActivePlanSearchField(planSearchField);
    // The last query is a remembered choice too; the field it was typed in
    // already persisted through its change handler.
    rememberPreferences(currentPreferences => ({
      ...currentPreferences,
      search: { ...currentPreferences.search, lastQuery: queryText },
    }));
    if (!hasSessionProviderCredential) {
      // The keyless search filters the shared artifact's demo places — never
      // a provider call, never an invented row. An empty answer says so
      // honestly instead of a silent list.
      const demoRows = searchDemoPlaces(queryText);
      setSearchRows(demoRows);
      setSearchInFlight(false);
      setDemoSearchNoMatchText(
        demoRows.length === 0
          ? appStrings.demoNoMatchHintFormat.replace(
              '{field}',
              planSearchFieldLabel(planSearchField),
            )
          : null,
      );
      return;
    }
    if (!refappMapModule) {
      return;
    }
    setDemoSearchNoMatchText(null);
    setSearchInFlight(true);
    refappMapModule
      .searchPlaces(queryText)
      .then(rows => {
        setSearchRows(rows);
        setSearchInFlight(false);
      })
      .catch(searchError => {
        setSearchRows([]);
        setSearchInFlight(false);
        // The verbatim refusal also rides the error strip (the native event);
        // the search view shows its own honest empty state.
        setErrorReport({
          variantLabel: 'PlanSearch',
          detailText: String(searchError),
        });
      });
  }, [hasSessionProviderCredential, rememberPreferences]);

  /** A clicked result row assigns to the field the search served. */
  const assignSearchRow = useCallback(
    (placeRow: PlaceRow) => {
      assignPlanFieldRow(activePlanSearchField, placeRow);
    },
    [activePlanSearchField, assignPlanFieldRow],
  );

  // The explicit Plan route action (§4.1): disabled-with-reason until both
  // fields hold text; planning resolves each field and lands the route
  // summary at half.
  const canPlanRoute =
    originSearchText.trim() !== '' && destinationSearchText.trim() !== '';
  // The keyless filter's rows carry the demo marker: the list says they are
  // demo places, never live search results.
  const searchRowsAreDemoPlaces = searchRows.some(row => row.isDemoPlace === true);
  /**
   * Whether the session has a routing provider registered: a credentialed
   * provider, or a keyless one the visitor selected. Either way the plan
   * action asks that provider instead of replaying the artifact's recorded
   * route.
   */
  const hasRegisteredRoutingProvider = useMemo(
    () =>
      providerRegistrations.some(
        registration =>
          registration.contract === ProviderContract.Routing &&
          registration.registered,
      ),
    [providerRegistrations],
  );
  const planRouteReasonText =
    originSearchText.trim() === '' && destinationSearchText.trim() === ''
      ? appStrings.planMissingBothText
      : originSearchText.trim() === ''
        ? appStrings.planMissingOriginText
        : destinationSearchText.trim() === ''
          ? appStrings.planMissingDestinationText
          : '';
  // The reason a My location chip cannot start the source, exactly the
  // device-location row's reason (nil while the chip works).
  const myLocationDisabledReason = deviceLocationCannotStartReasonText(
    mapLayersState.locationStatus,
  );

  /**
   * Resolves one plan field in the shared order: the assigned place when the
   * field still holds its text, the exact demo-place name, then the
   * provider's first result. Answers the resolved name, or null when nothing
   * resolved (the caller names the field in the error strip).
   */
  const resolvePlanField = useCallback(
    async (planSearchField: PlanSearchField): Promise<string | null> => {
      const queryText = (
        planSearchField === 'origin' ? originSearchText : destinationSearchText
      ).trim();
      if (queryText === '') {
        return null;
      }
      const assignedText =
        planSearchField === 'origin' ? assignedOriginText : assignedDestinationText;
      const assignedPlace =
        planSearchField === 'origin' ? assignedOriginPlace : assignedDestinationPlace;
      if (assignedText === queryText && assignedPlace !== null) {
        return assignedPlace.structuredAddressText;
      }
      const demoPlace = demoPlaceNamed(queryText);
      if (demoPlace !== null) {
        const demoRow: PlaceRow = {
          structuredAddressText: demoPlace.name,
          latitudeDegrees: demoPlace.latitude_degrees,
          longitudeDegrees: demoPlace.longitude_degrees,
          qualityText: '',
          noteText: demoPlace.note,
          isDemoPlace: true,
        };
        if (hasSessionProviderCredential && refappMapModule) {
          const assigned = await refappMapModule.assignSearchResult(
            planSearchField === 'origin',
            demoRow.structuredAddressText,
            demoRow.latitudeDegrees,
            demoRow.longitudeDegrees,
          );
          if (!assigned) {
            return null;
          }
        }
        assignPlanFieldPlace(planSearchField, demoRow);
        return demoRow.structuredAddressText;
      }
      if (!hasSessionProviderCredential || !refappMapModule) {
        return null;
      }
      const providerRows = await refappMapModule.searchPlaces(queryText);
      const firstProviderRow = providerRows[0];
      if (firstProviderRow === undefined) {
        return null;
      }
      const assigned = await refappMapModule.assignSearchResult(
        planSearchField === 'origin',
        firstProviderRow.structuredAddressText,
        firstProviderRow.latitudeDegrees,
        firstProviderRow.longitudeDegrees,
      );
      if (!assigned) {
        return null;
      }
      assignPlanFieldPlace(planSearchField, firstProviderRow);
      return firstProviderRow.structuredAddressText;
    },
    [
      assignedDestinationPlace,
      assignedDestinationText,
      assignedOriginPlace,
      assignedOriginText,
      assignPlanFieldPlace,
      destinationSearchText,
      hasSessionProviderCredential,
      originSearchText,
    ],
  );

  /** Names the field that resolved to no place in the error strip. */
  const reportUnresolvedPlanField = useCallback(
    (planSearchField: PlanSearchField) => {
      setErrorReport({
        variantLabel: 'PlanFieldUnresolved',
        detailText: hasSessionProviderCredential
          ? `the provider found no place for the ${planSearchFieldLabel(planSearchField)}`
          : appStrings.demoNoMatchHintFormat.replace(
              '{field}',
              planSearchFieldLabel(planSearchField),
            ),
      });
    },
    [hasSessionProviderCredential],
  );

  const planRouteFromFields = useCallback(async () => {
    if (!canPlanRoute) {
      return;
    }
    let resolvedOriginName: string | null;
    let resolvedDestinationName: string | null;
    try {
      resolvedOriginName = await resolvePlanField('origin');
      if (resolvedOriginName === null) {
        reportUnresolvedPlanField('origin');
        return;
      }
      resolvedDestinationName = await resolvePlanField('destination');
      if (resolvedDestinationName === null) {
        reportUnresolvedPlanField('destination');
        return;
      }
    } catch (resolutionError) {
      setErrorReport({
        variantLabel: 'PlanFieldResolution',
        detailText: String(resolutionError),
      });
      return;
    }
    if (!hasSessionProviderCredential && !hasRegisteredRoutingProvider) {
      // The keyless plan with no routing provider applies the artifact's
      // recorded route for the resolved pair — never a provider call, never
      // invented geometry. An unrecorded pair says so honestly instead of
      // failing silently. A selected keyless routing provider registers one,
      // so the same action then plans through that provider below.
      if (recordedDemoRoute(resolvedOriginName, resolvedDestinationName) === null) {
        setErrorReport({
          variantLabel: 'MissingRecordedRoute',
          detailText: appStrings.noRecordedDemoRouteHintFormat
            .replace('{origin}', resolvedOriginName)
            .replace('{destination}', resolvedDestinationName),
        });
        return;
      }
      if (!refappMapModule) {
        return;
      }
      refappMapModule
        .planRecordedDemoRoute(resolvedOriginName, resolvedDestinationName)
        .then(planned => {
          if (planned) {
            setContentMode('home');
            settleToSheetState('half');
          }
        })
        .catch(planError => {
          setErrorReport({
            variantLabel: 'RecordedRoute',
            detailText: String(planError),
          });
        });
      return;
    }
    if (!refappMapModule) {
      return;
    }
    refappMapModule
      .planRoute(
        directionsControl.selection.transportationMode,
        directionsControl.selection.routeStrategy,
      )
      .then(planned => {
        if (planned) {
          setContentMode('home');
          settleToSheetState('half');
        }
      })
      .catch(planError => {
        setErrorReport({
          variantLabel: 'PlanRoute',
          detailText: String(planError),
        });
      });
  }, [
    canPlanRoute,
    directionsControl.selection.transportationMode,
    directionsControl.selection.routeStrategy,
    hasRegisteredRoutingProvider,
    hasSessionProviderCredential,
    reportUnresolvedPlanField,
    resolvePlanField,
    settleToSheetState,
  ]);

  /**
   * The field-pair swap: the two texts and their resolved places exchange,
   * and the live native assignment follows when a keyed session holds both.
   */
  const swapPlanFields = useCallback(() => {
    const previousOriginText = originSearchText;
    const previousDestinationText = destinationSearchText;
    const previousOriginPlace = assignedOriginPlace;
    const previousDestinationPlace = assignedDestinationPlace;
    changeOriginSearchText(previousDestinationText);
    changeDestinationSearchText(previousOriginText);
    setAssignedOriginText(assignedDestinationText);
    setAssignedDestinationText(assignedOriginText);
    setAssignedOriginPlace(previousDestinationPlace);
    setAssignedDestinationPlace(previousOriginPlace);
    setMyLocationFilledFields(currentFields => ({
      origin: currentFields.destination,
      destination: currentFields.origin,
    }));
    setActivePlanSearchField(previous =>
      previous === 'origin' ? 'destination' : 'origin',
    );
    if (hasSessionProviderCredential && refappMapModule) {
      if (previousDestinationPlace !== null) {
        void refappMapModule.assignSearchResult(
          true,
          previousDestinationPlace.structuredAddressText,
          previousDestinationPlace.latitudeDegrees,
          previousDestinationPlace.longitudeDegrees,
        );
      }
      if (previousOriginPlace !== null) {
        void refappMapModule.assignSearchResult(
          false,
          previousOriginPlace.structuredAddressText,
          previousOriginPlace.latitudeDegrees,
          previousOriginPlace.longitudeDegrees,
        );
      }
    }
  }, [
    assignedDestinationPlace,
    assignedDestinationText,
    assignedOriginPlace,
    assignedOriginText,
    changeDestinationSearchText,
    changeOriginSearchText,
    destinationSearchText,
    hasSessionProviderCredential,
    originSearchText,
  ]);

  /** Fills exactly one plan field from a device fix, never the other. */
  const fillPlanFieldFromDeviceFix = useCallback(
    (planSearchField: PlanSearchField, latitude: number, longitude: number) => {
      const fixText =
        latitude.toFixed(planFieldDeviceFixDecimalPlaces) +
        ', ' +
        longitude.toFixed(planFieldDeviceFixDecimalPlaces);
      const fixRow: PlaceRow = {
        structuredAddressText: fixText,
        latitudeDegrees: latitude,
        longitudeDegrees: longitude,
        qualityText: '',
      };
      if (hasSessionProviderCredential && refappMapModule) {
        void refappMapModule.assignSearchResult(
          planSearchField === 'origin',
          fixText,
          latitude,
          longitude,
        );
      }
      assignPlanFieldPlace(planSearchField, fixRow);
      setMyLocationFilledFields(currentFields => ({
        ...currentFields,
        [planSearchField]: true,
      }));
    },
    [assignPlanFieldPlace, hasSessionProviderCredential],
  );

  /**
   * The offline card's route control (§2.5 `offline.plan_route_action`): the
   * native host plans over the bundled recorded test graph and emits the
   * same plan-state event a live plan does; the route summary then says it is
   * recorded test data, not live data. A refusal arrives verbatim on the
   * error channel, which the strip above the sheet already renders.
   */
  const planOfflineRouteFromArchive = useCallback(() => {
    if (!refappMapModule || offlineRouteInFlight) {
      return;
    }
    setOfflineRouteInFlight(true);
    refappMapModule
      .planOfflineRouteFromArchive()
      .then(planned => {
        if (planned) {
          setContentMode('home');
          settleToSheetState('half');
        }
      })
      .catch(offlineError => {
        setErrorReport({
          variantLabel: 'OfflineRoute',
          detailText: String(offlineError),
        });
      })
      .finally(() => setOfflineRouteInFlight(false));
  }, [offlineRouteInFlight, settleToSheetState]);

  /**
   * Reloads the regions stored on disk. A not-configured cache leaves the
   * list empty rather than inventing a stored region; the card's cache line
   * already states the genuine cause.
   */
  const refreshStoredOfflineRegions = useCallback(async () => {
    const sdk = sdkInstanceRef.current;
    if (sdk === null) {
      setStoredOfflineRegions([]);
      return;
    }
    try {
      setStoredOfflineRegions(await sdk.regionRecords());
    } catch {
      setStoredOfflineRegions([]);
    }
  }, []);

  /** Reads the demo region's enumerated tile count for the card's warning. */
  const refreshOfflineRegionTileCount = useCallback(async () => {
    const sdk = sdkInstanceRef.current;
    if (sdk === null || demoOfflineRegion === null) {
      setOfflineRegionTileCount(null);
      return;
    }
    try {
      setOfflineRegionTileCount(await sdk.regionTileCount(demoOfflineRegion));
    } catch {
      setOfflineRegionTileCount(null);
    }
  }, []);

  const isOfflineCacheRegistered = cacheState?.isRegistered === true;

  // The region list and the demo region's tile count follow the registered
  // cache: both require the installed store the credential registration
  // creates.
  useEffect(() => {
    if (!isSdkCreated || !isOfflineCacheRegistered) {
      setStoredOfflineRegions([]);
      setOfflineRegionTileCount(null);
      return;
    }
    void refreshStoredOfflineRegions();
    void refreshOfflineRegionTileCount();
  }, [
    isSdkCreated,
    isOfflineCacheRegistered,
    refreshStoredOfflineRegions,
    refreshOfflineRegionTileCount,
  ]);

  /**
   * Starts (or resumes) the demo region download. Progress reports update
   * the card live; the promise answers the record as it stands on disk, so a
   * cancelled download resolves with its partial record and a terminal
   * `cancelled` report rather than an error.
   */
  const downloadOfflineRegion = useCallback(async () => {
    const sdk = sdkInstanceRef.current;
    if (sdk === null || demoOfflineRegion === null || offlineRegionInFlight) {
      return;
    }
    setOfflineRegionInFlight(true);
    setOfflineRegionStatus('downloading');
    setOfflineRegionErrorText(null);
    setOfflineRegionDeleted(false);
    setOfflineRegionProgress(null);
    try {
      await sdk.downloadRegion(demoOfflineRegion, progress => {
        setOfflineRegionProgress(progress);
        if (progress.phase !== 'downloading') {
          setOfflineRegionStatus(progress.phase);
        }
      });
      setOfflineRegionStatus(currentStatus =>
        currentStatus === 'cancelled' ? 'cancelled' : 'completed',
      );
      await refreshStoredOfflineRegions();
    } catch (downloadError) {
      setOfflineRegionStatus('failed');
      setOfflineRegionErrorText(verbatimErrorText(downloadError));
      // A failed download keeps its partial region, so the list still shows
      // the resumable tiles.
      await refreshStoredOfflineRegions();
    } finally {
      setOfflineRegionInFlight(false);
    }
  }, [offlineRegionInFlight, refreshStoredOfflineRegions]);

  /** Stops the in-flight download; the partial region stays resumable. */
  const cancelOfflineRegionDownload = useCallback(() => {
    const sdk = sdkInstanceRef.current;
    if (sdk === null || demoOfflineRegion === null) {
      return;
    }
    sdk.cancelRegionDownload(demoOfflineRegion.regionId).catch(cancelError => {
      setOfflineRegionStatus('failed');
      setOfflineRegionErrorText(verbatimErrorText(cancelError));
    });
  }, []);

  /** Removes one stored region and returns the card to the store-empty state. */
  const deleteOfflineRegion = useCallback(
    async (regionId: string) => {
      const sdk = sdkInstanceRef.current;
      if (sdk === null) {
        return;
      }
      try {
        await sdk.deleteRegion(regionId);
        setOfflineRegionDeleted(true);
        setOfflineRegionProgress(null);
        setOfflineRegionStatus('idle');
        setOfflineRegionErrorText(null);
        await refreshStoredOfflineRegions();
      } catch (deleteError) {
        setOfflineRegionStatus('failed');
        setOfflineRegionErrorText(verbatimErrorText(deleteError));
      }
    },
    [refreshStoredOfflineRegions],
  );

  // --- Camera presets ---
  const applyPreset = useCallback(
    (preset: CameraPreset) => {
      setActivePresetId(preset.id);
      refappMapModule?.applyCameraPreset(
        preset.latitude_degrees,
        preset.longitude_degrees,
        preset.zoom_level,
        preset.bearing_degrees,
        preset.pitch_degrees,
      );
      rememberPreferences(currentPreferences => ({
        ...currentPreferences,
        cameraPresetId: preset.id,
      }));
    },
    [rememberPreferences],
  );

  // --- The map control cluster (§4.2): native-owned camera edits ---
  const zoomInByControl = useCallback(() => {
    refappMapModule?.zoomInControl();
  }, []);

  const zoomOutByControl = useCallback(() => {
    refappMapModule?.zoomOutControl();
  }, []);

  const centerOnMyLocation = useCallback(() => {
    refappMapModule?.myLocationControl();
  }, []);

  const toggleCameraPlane = useCallback(() => {
    refappMapModule?.cameraPlaneControl();
  }, []);

  /**
   * Draws the puck's interpolated pose for this frame (M72). The callback
   * answers whether the motion still runs, so the frame clock schedules the
   * next frame only while the puck is moving.
   */
  const drawLatestLocationPose = useCallback((): boolean => {
    const pose = locationPoseAnimatorRef.current.poseAt(Date.now());
    if (pose === null) {
      return false;
    }
    mapViewRef.current?.setLocationIndicator(
      {
        coordinate: {
          latitude: pose.latitudeDegrees,
          longitude: pose.longitudeDegrees,
        },
        courseHeadingDegrees: pose.courseDegrees ?? undefined,
        horizontalAccuracyMeters: pose.horizontalAccuracyMeters ?? undefined,
      },
      {
        visualKind: LocationIndicatorVisualKind.TrackingDot,
        showAccuracyRing: true,
      },
    );
    return pose.isAnimating;
  }, []);

  // The compass is a mode toggle during a trip (M73): choosing north-up
  // pins the native follow camera's bearing at 0 and the control stays
  // visible so the visitor can return to course-up. The native side owns the
  // camera; this state mirrors the chosen mode for the control's visibility.
  const [followBearingMode, setFollowBearingMode] = useState(
    FollowBearingMode.CourseUp,
  );
  const isHoldingNorthUp = followBearingMode === FollowBearingMode.NorthUp;
  const toggleCompassMode = useCallback(() => {
    if (traceActive) {
      setFollowBearingMode(currentMode =>
        currentMode === FollowBearingMode.NorthUp
          ? FollowBearingMode.CourseUp
          : FollowBearingMode.NorthUp,
      );
    }
    refappMapModule?.compassControl();
  }, [traceActive]);

  // A trip starts course-up and its end restores the default (M73).
  useEffect(() => {
    setFollowBearingMode(FollowBearingMode.CourseUp);
  }, [traceActive]);

  // --- The simulated navigation controls (§7) ---
  const stopDeviceLocation = useCallback(async () => {
    if (deviceLocationIntervalRef.current !== null) {
      clearInterval(deviceLocationIntervalRef.current);
      deviceLocationIntervalRef.current = null;
    }
    locationFrameClockRef.current.stop();
    locationPoseAnimatorRef.current.reset();
    const sdk = sdkInstanceRef.current;
    if (sdk !== null) {
      await sdk.stopDeviceLocation().catch(() => false);
    }
    mapViewRef.current?.clearLocationIndicator();
    setIsDeviceLocationActive(false);
    setDeviceLocationStatusText('Location off.');
    dispatchMapLayers({ type: 'deviceLocationStatusChanged', status: 'off' });
  }, [dispatchMapLayers]);

  /**
   * The device-navigation event lets the shell reflect a trip the native side
   * ended by itself (a refused fix): the control then stops claiming guidance,
   * and a location source this mode started is stopped with it.
   */
  useEffect(() => {
    return addControllerListener('RefappDeviceNavigationState', payload => {
      const isNowActive = Boolean(payload.isActive);
      isDeviceNavigationActiveRef.current = isNowActive;
      setIsDeviceNavigationActive(isNowActive);
      if (!isNowActive && deviceNavigationStartedLocationSourceRef.current) {
        deviceNavigationStartedLocationSourceRef.current = false;
        void stopDeviceLocation();
      }
    });
  }, [stopDeviceLocation]);

  /**
   * Starts the device location source through the permission and start path
   * the Show control uses, then polls its accepted fixes. While a
   * device-driven trip runs, each fix goes to the navigator and the native
   * follow camera and indicator present it; outside a trip the shell shows the
   * fix as a tracking dot and centres the map. A false answer means the start
   * path already reported its permission or availability state.
   */
  const startDeviceLocationSource = useCallback(async (): Promise<boolean> => {
    const sdk = sdkInstanceRef.current;
    if (sdk === null) {
      return false;
    }
    if (Platform.OS === 'android') {
      const permission = await PermissionsAndroid.request(
        PermissionsAndroid.PERMISSIONS.ACCESS_FINE_LOCATION,
      );
      if (permission !== PermissionsAndroid.RESULTS.GRANTED) {
        setDeviceLocationStatusText(
          'Location permission was denied. Allow it in Settings to show your position.',
        );
        dispatchMapLayers({
          type: 'deviceLocationStatusChanged',
          status: 'permissionDenied',
        });
        return false;
      }
    }
    const started = await sdk.startDeviceLocation().catch(() => false);
    if (!started) {
      const sourceStatus = await sdk.deviceLocationSourceStatus().catch(() => null);
      setDeviceLocationStatusText(
        sourceStatus === 'permissionDenied'
          ? 'Location permission was denied. Allow it in Settings to show your position.'
          : 'Location is unavailable on this device right now.',
      );
      dispatchMapLayers({
        type: 'deviceLocationStatusChanged',
        status:
          sourceStatus === 'permissionDenied' ? 'permissionDenied' : 'unavailable',
      });
      return false;
    }
    setIsDeviceLocationActive(true);
    setDeviceLocationStatusText(null);
    dispatchMapLayers({ type: 'deviceLocationStatusChanged', status: 'waiting' });
    deviceLocationIntervalRef.current = setInterval(async () => {
      const fix = await sdk.latestDeviceFix().catch(() => null);
      if (fix === null) {
        return;
      }
      const smoothedCourse = await sdk
        .deviceLocationSmoothedCourseDegrees()
        .catch(() => null);
      setLatestFixDisplayText(
        fix.coordinate.latitude.toFixed(planFieldDeviceFixDecimalPlaces) +
          ', ' +
          fix.coordinate.longitude.toFixed(planFieldDeviceFixDecimalPlaces),
      );
      setDeviceLocationStatusText(
        `Position ${fix.coordinate.latitude.toFixed(planFieldDeviceFixDecimalPlaces)}, ` +
          `${fix.coordinate.longitude.toFixed(planFieldDeviceFixDecimalPlaces)}.`,
      );
      // A My location chip pressed before the first fix filled its field now:
      // exactly that field, never the other one.
      const pendingMyLocationField = pendingMyLocationFieldRef.current;
      if (pendingMyLocationField !== null) {
        pendingMyLocationFieldRef.current = null;
        fillPlanFieldFromDeviceFix(
          pendingMyLocationField,
          fix.coordinate.latitude,
          fix.coordinate.longitude,
        );
      }
      dispatchMapLayers({ type: 'deviceLocationStatusChanged', status: 'active' });
      if (isDeviceNavigationActiveRef.current) {
        // Every accepted fix of a device-driven trip reaches the route's
        // navigator through the controller's smoothed course; the native
        // presentation owns the banner, ETA, indicator and follow camera.
        refappMapModule?.deliverDeviceNavigationFix(
          fix.coordinate.latitude,
          fix.coordinate.longitude,
          smoothedCourse,
          fix.horizontalAccuracyMeters ?? deviceNavigationFallbackAccuracyMeters,
        );
        return;
      }
      // The fix enters the animator and the frame clock draws the eased pose;
      // the camera still takes the fix's own centre (the follow camera owns
      // its chase and must not be dragged by an interpolation artefact).
      locationPoseAnimatorRef.current.pushFix({
        latitudeDegrees: fix.coordinate.latitude,
        longitudeDegrees: fix.coordinate.longitude,
        courseDegrees:
          smoothedCourse ?? fix.courseHeadingDegrees ?? null,
        horizontalAccuracyMeters: fix.horizontalAccuracyMeters ?? null,
        receivedAtMilliseconds: Date.now(),
      });
      locationFrameClockRef.current.start(drawLatestLocationPose);
      mapViewRef.current?.setCamera({
        latitudeDegrees: fix.coordinate.latitude,
        longitudeDegrees: fix.coordinate.longitude,
        zoomLevel: 15,
        bearingDegrees: 0,
        pitchDegrees: 0,
      });
    }, 1000);
    return true;
  }, [dispatchMapLayers, drawLatestLocationPose, fillPlanFieldFromDeviceFix]);

  const toggleDeviceLocation = useCallback(async () => {
    if (isDeviceLocationActive) {
      // An explicit Hide also ends device navigation: the source this mode
      // may have started is the one being hidden.
      if (isDeviceNavigationActiveRef.current) {
        isDeviceNavigationActiveRef.current = false;
        deviceNavigationStartedLocationSourceRef.current = false;
        setIsDeviceNavigationActive(false);
        refappMapModule?.stopDeviceNavigation();
      }
      await stopDeviceLocation();
      return;
    }
    await startDeviceLocationSource();
  }, [isDeviceLocationActive, startDeviceLocationSource, stopDeviceLocation]);

  /**
   * Ends device-driven guidance. The location source stops only when this
   * mode started it; a source the visitor started by hand keeps running.
   */
  const stopDeviceNavigation = useCallback(async () => {
    if (!isDeviceNavigationActiveRef.current) {
      return;
    }
    const hasStartedLocationSource =
      deviceNavigationStartedLocationSourceRef.current;
    isDeviceNavigationActiveRef.current = false;
    deviceNavigationStartedLocationSourceRef.current = false;
    setIsDeviceNavigationActive(false);
    refappMapModule?.stopDeviceNavigation();
    if (hasStartedLocationSource) {
      await stopDeviceLocation();
    } else {
      setDeviceLocationStatusText(null);
    }
  }, [stopDeviceLocation]);

  /**
   * One My location chip: starts the device source when it is off, then fills
   * only that field from the latest fix. With no fix yet the field waits for
   * the next accepted one; a source that cannot start reports the shared
   * unavailable sentence.
   */
  const fillPlanFieldWithMyLocation = useCallback(
    async (planSearchField: PlanSearchField) => {
      // The chip is disabled while the panel reports a reason; refuse here
      // too, so no call path starts a source the visitor cannot use.
      if (myLocationDisabledReason !== null) {
        return;
      }
      setActivePlanSearchField(planSearchField);
      const sdk = sdkInstanceRef.current;
      let latestFix = (await sdk?.latestDeviceFix().catch(() => null)) ?? null;
      if (latestFix === null) {
        const sourceStarted = await startDeviceLocationSource();
        if (!sourceStarted) {
          setErrorReport({
            variantLabel: 'MyLocationUnavailable',
            detailText: appStrings.planMyLocationUnavailableFormat.replace(
              '{reason}',
              deviceLocationStatusText ?? 'the location source could not start',
            ),
          });
          return;
        }
        latestFix = (await sdk?.latestDeviceFix().catch(() => null)) ?? null;
      }
      if (latestFix === null) {
        pendingMyLocationFieldRef.current = planSearchField;
        return;
      }
      fillPlanFieldFromDeviceFix(
        planSearchField,
        latestFix.coordinate.latitude,
        latestFix.coordinate.longitude,
      );
    },
    [
      deviceLocationStatusText,
      fillPlanFieldFromDeviceFix,
      myLocationDisabledReason,
      startDeviceLocationSource,
    ],
  );

  /**
   * Starts guidance on the planned route from this device's fixes. Without a
   * planned route the control states what it needs and starts nothing. The
   * location source starts when it is off, through the same path the location
   * control uses, and a simulated trip ends first because one trip runs at a
   * time.
   */
  const startDeviceNavigation = useCallback(async () => {
    if (isDeviceNavigationActiveRef.current) {
      return;
    }
    if (!planState.isPlanned) {
      setDeviceLocationStatusText(deviceNavigationPlanFirstText);
      return;
    }
    // One trip at a time: the simulated presentation ends before the
    // device-driven one engages.
    if (traceActive) {
      refappMapModule?.exitTraceReplay();
    }
    const wasLocationSourceActive = isDeviceLocationActive;
    if (!wasLocationSourceActive) {
      const didStartLocationSource = await startDeviceLocationSource();
      if (!didStartLocationSource) {
        return;
      }
    }
    deviceNavigationStartedLocationSourceRef.current = !wasLocationSourceActive;
    let didStartGuidance = false;
    const startGuidanceRequest = refappMapModule?.startDeviceNavigation?.();
    if (startGuidanceRequest !== undefined) {
      didStartGuidance = await startGuidanceRequest.catch(() => false);
    }
    if (!didStartGuidance) {
      setDeviceLocationStatusText(deviceNavigationGuidanceUnavailableText);
      if (deviceNavigationStartedLocationSourceRef.current) {
        deviceNavigationStartedLocationSourceRef.current = false;
        await stopDeviceLocation();
      }
      return;
    }
    isDeviceNavigationActiveRef.current = true;
    setIsDeviceNavigationActive(true);
    setDeviceLocationStatusText(deviceNavigationWaitingText);
  }, [
    isDeviceLocationActive,
    planState.isPlanned,
    startDeviceLocationSource,
    stopDeviceLocation,
    traceActive,
  ]);

  /** The Location group's second control: start guidance, or stop it. */
  const toggleDeviceNavigation = useCallback(() => {
    if (isDeviceNavigationActiveRef.current) {
      void stopDeviceNavigation();
    } else {
      void startDeviceNavigation();
    }
  }, [startDeviceNavigation, stopDeviceNavigation]);

  useEffect(() => () => {
    if (deviceLocationIntervalRef.current !== null) {
      clearInterval(deviceLocationIntervalRef.current);
      deviceLocationIntervalRef.current = null;
    }
    locationFrameClockRef.current.stop();
  }, []);

  const startSimulatedNavigation = useCallback(async () => {
    // One trip at a time: a device-driven trip ends before the replay starts.
    if (isDeviceNavigationActiveRef.current) {
      await stopDeviceNavigation();
    }
    refappMapModule?.startTraceReplay();
  }, [stopDeviceNavigation]);

  const exitSimulatedNavigation = useCallback(async () => {
    // The drawer's exit control ends whichever trip is presented.
    if (isDeviceNavigationActiveRef.current) {
      await stopDeviceNavigation();
      return;
    }
    refappMapModule?.exitTraceReplay();
  }, [stopDeviceNavigation]);

  /** The drawer's one Pause/Resume control (§7.2): the act the tap performs. */
  const toggleSimulatedNavigationPause = useCallback(() => {
    if (playbackState.isPaused) {
      refappMapModule?.resumeTraceReplay();
    } else {
      refappMapModule?.pauseTraceReplay();
    }
  }, [playbackState.isPaused]);

  /** One speed chip's act: the walker re-anchors at the travelled distance. */
  const selectSimulatedNavigationSpeed = useCallback(
    (speedMultiplier: number) => {
      refappMapModule?.selectTraceReplaySpeedMultiplier(speedMultiplier);
      rememberPreferences(currentPreferences => ({
        ...currentPreferences,
        speedMultiplier,
      }));
    },
    [rememberPreferences],
  );

  /** The seek control's act: the walker supplies the fix at the distance. */
  const seekSimulatedNavigation = useCallback((travelledMeters: number) => {
    refappMapModule?.seekTraceReplay(travelledMeters);
  }, []);

  // --- The drawer's speech toggle (§4.2; the consumer owns the off stop) ---
  const toggleSpeech = useCallback(
    (nextSpeechEnabled: boolean) => {
      speechConsumer.setSpeechEnabled(nextSpeechEnabled);
    },
    [speechConsumer],
  );

  // --- The demo-scene switches (the layers panel's overlay section) ---
  const [sceneSelections, setSceneSelections] = useState<Record<string, boolean>>({});
  const sceneSelectionsRef = useRef<Record<string, boolean>>({});

  const toggleSceneChip = useCallback(
    (sceneKey: string, isSelected: boolean) => {
      const nextSelections = {
        ...sceneSelectionsRef.current,
        [sceneKey]: isSelected,
      };
      sceneSelectionsRef.current = nextSelections;
      setSceneSelections(nextSelections);
      refappMapModule?.setSceneOverlayToggle(sceneKey, isSelected);
      rememberPreferences(currentPreferences => ({
        ...currentPreferences,
        selectedSceneKeys: rememberedSceneKeys(nextSelections),
      }));
    },
    [rememberPreferences],
  );

  /** The overlays section's shared clear action: every scene off in one act,
   * writing the same remembered record each switch writes. */
  const clearSceneSelections = useCallback(() => {
    for (const sceneKey of rememberedSceneKeys(sceneSelectionsRef.current)) {
      refappMapModule?.setSceneOverlayToggle(sceneKey, false);
    }
    sceneSelectionsRef.current = {};
    setSceneSelections({});
    rememberPreferences(currentPreferences => ({
      ...currentPreferences,
      selectedSceneKeys: [],
    }));
  }, [rememberPreferences]);

  // --- The remembered choices' mount-time restore ---
  // The record is read once; a device that never stored one starts from the
  // defaults. A theme an earlier build stored in its own key is carried into
  // the record so that upgrade keeps the choice. Remembered plan and scene
  // ids are resolved against the artifact's offered rows, so a dropped row
  // falls back to the default instead of an unusable selection.
  useEffect(() => {
    const module = refappMapModule;
    let isMounted = true;
    const hydratePreferences = async (): Promise<void> => {
      let storedPreferences: string | null = null;
      if (module && typeof module.loadPreferences === 'function') {
        try {
          storedPreferences = await module.loadPreferences();
        } catch {
          storedPreferences = null;
        }
      }
      const storedRecordIsAbsent =
        storedPreferences === null || storedPreferences.trim() === '';
      let restoredPreferences =
        referenceAppPreferencesFromStoredValue(storedPreferences);
      if (restoredPreferences === null) {
        restoredPreferences = defaultReferenceAppPreferences();
        if (
          storedRecordIsAbsent &&
          module &&
          typeof module.loadThemeChoice === 'function'
        ) {
          try {
            restoredPreferences = {
              ...restoredPreferences,
              themeChoice: themeChoiceFromStoredValue(
                await module.loadThemeChoice(),
              ),
            };
          } catch {
            // A missing legacy store is the same state as no earlier choice.
          }
        }
      }
      const rememberedPlan =
        demoPlans.find(
          demoPlan =>
            demoPlan.traceAssetName === restoredPreferences?.demoPlanAssetName,
        ) ?? null;
      const rememberedPreset =
        cameraPresets.find(
          preset => preset.id === restoredPreferences?.cameraPresetId,
        ) ?? null;
      const knownSceneKeys = new Set(
        overlayScenes.map(({ sceneKey }) => sceneKey),
      );
      const normalizedPreferences: ReferenceAppPreferences = {
        ...restoredPreferences,
        demoPlanAssetName: rememberedPlan?.traceAssetName ?? null,
        cameraPresetId: rememberedPreset?.id ?? null,
        selectedSceneKeys: restoredPreferences.selectedSceneKeys.filter(
          sceneKey => knownSceneKeys.has(sceneKey),
        ),
      };
      if (!isMounted) {
        return;
      }
      preferencesRef.current = normalizedPreferences;
      setPreferences(normalizedPreferences);
      setThemeChoice(normalizedPreferences.themeChoice);
      setOriginSearchText(normalizedPreferences.search.origin);
      setDestinationSearchText(normalizedPreferences.search.destination);
      const restoredPlanAssetName =
        normalizedPreferences.demoPlanAssetName ??
        artifactDemoPlan?.traceAssetName ??
        null;
      selectedPlanAssetNameRef.current = restoredPlanAssetName;
      setSelectedPlanAssetName(restoredPlanAssetName);
      const restoredSelections: Record<string, boolean> = {};
      for (const sceneKey of normalizedPreferences.selectedSceneKeys) {
        restoredSelections[sceneKey] = true;
      }
      sceneSelectionsRef.current = restoredSelections;
      setSceneSelections(restoredSelections);
      setArePreferencesHydrated(true);
    };
    void hydratePreferences();
    return () => {
      isMounted = false;
    };
  }, [artifactDemoPlan?.traceAssetName]);

  // The remembered plan and camera preset apply once the SDK session exists.
  // The plan's route framing runs first so the remembered camera preset, the
  // last explicit framing choice, is what the first frame rests on. The
  // remembered search fields only fill their inputs: the explicit Plan route
  // action stays the only path that plans a route.
  const hasAppliedStartupSelectionRef = useRef(false);
  useEffect(() => {
    if (
      !arePreferencesHydrated ||
      !isSdkCreated ||
      hasAppliedStartupSelectionRef.current
    ) {
      return;
    }
    hasAppliedStartupSelectionRef.current = true;
    const applyRememberedFraming = () => {
      const rememberedPreset =
        cameraPresets.find(
          preset => preset.id === preferencesRef.current.cameraPresetId,
        ) ?? cameraPresets[0];
      if (!rememberedPreset) {
        return;
      }
      setActivePresetId(rememberedPreset.id);
      refappMapModule?.applyCameraPreset(
        rememberedPreset.latitude_degrees,
        rememberedPreset.longitude_degrees,
        rememberedPreset.zoom_level,
        rememberedPreset.bearing_degrees,
        rememberedPreset.pitch_degrees,
      );
    };
    const rememberedPlanAssetName = preferencesRef.current.demoPlanAssetName;
    const defaultPlanAssetName = artifactDemoPlan?.traceAssetName ?? null;
    if (
      rememberedPlanAssetName === null ||
      rememberedPlanAssetName === defaultPlanAssetName
    ) {
      applyRememberedFraming();
      return;
    }
    const planRequest = refappMapModule?.selectTraceReplayPlan?.(
      rememberedPlanAssetName,
    );
    if (planRequest === undefined) {
      applyRememberedFraming();
      return;
    }
    planRequest
      .then(() => applyRememberedFraming())
      .catch(startupPlanError => {
        setErrorReport({
          variantLabel: 'DemoPlan',
          detailText: String(startupPlanError),
        });
        applyRememberedFraming();
      });
  }, [arePreferencesHydrated, isSdkCreated, artifactDemoPlan?.traceAssetName]);

  // The remembered playback pace reaches the native walker before a trip
  // starts; the drawer then opens on the chosen chip.
  useEffect(() => {
    if (!arePreferencesHydrated) {
      return;
    }
    const rememberedSpeedMultiplier = preferencesRef.current.speedMultiplier;
    if (rememberedSpeedMultiplier !== simulatedTripSpeedMultiplierChoices[0]) {
      refappMapModule?.selectTraceReplaySpeedMultiplier(
        rememberedSpeedMultiplier,
      );
    }
  }, [arePreferencesHydrated]);

  // Restored scene chips need the live surface; once it reports ready their
  // overlays are applied through the same native call a chip uses.
  const hasAppliedRestoredScenesRef = useRef(false);
  useEffect(() => {
    if (
      !arePreferencesHydrated ||
      readiness.state !== 'ready' ||
      hasAppliedRestoredScenesRef.current
    ) {
      return;
    }
    hasAppliedRestoredScenesRef.current = true;
    for (const { sceneKey } of overlayScenes) {
      if (sceneSelectionsRef.current[sceneKey]) {
        refappMapModule?.setSceneOverlayToggle(sceneKey, true);
      }
    }
  }, [arePreferencesHydrated, readiness.state]);

  // The settings surface's reset row: the record is cleared and the choices
  // the current screen shows return to their defaults. The provider
  // credential keeps its own separate opt-in store.
  const resetRememberedChoices = useCallback(() => {
    const defaultPreferences = defaultReferenceAppPreferences();
    preferencesRef.current = defaultPreferences;
    setPreferences(defaultPreferences);
    const module = refappMapModule;
    if (module && typeof module.clearPreferences === 'function') {
      module.clearPreferences();
    }
    setThemeChoice(defaultPreferences.themeChoice);
    setOriginSearchText('');
    setDestinationSearchText('');
    setAssignedOriginText(null);
    setAssignedDestinationText(null);
    setSearchRows([]);
    setDemoSearchNoMatchText(null);
    const defaultPlanAssetName = artifactDemoPlan?.traceAssetName ?? null;
    selectedPlanAssetNameRef.current = defaultPlanAssetName;
    setSelectedPlanAssetName(defaultPlanAssetName);
    sceneSelectionsRef.current = {};
    setSceneSelections({});
    module?.clearSceneOverlays();
    dispatchMapLayers({ type: 'trafficToggled', isEnabled: false });
    module?.selectTraceReplaySpeedMultiplier(defaultPreferences.speedMultiplier);
    setPlaybackState(initialPlaybackState);
    const applyDefaultFraming = () => {
      const defaultPreset = cameraPresets[0];
      if (!defaultPreset) {
        return;
      }
      setActivePresetId(defaultPreset.id);
      module?.applyCameraPreset(
        defaultPreset.latitude_degrees,
        defaultPreset.longitude_degrees,
        defaultPreset.zoom_level,
        defaultPreset.bearing_degrees,
        defaultPreset.pitch_degrees,
      );
    };
    const defaultPlanRequest =
      defaultPlanAssetName !== null
        ? module?.selectTraceReplayPlan?.(defaultPlanAssetName)
        : undefined;
    if (defaultPlanRequest === undefined) {
      applyDefaultFraming();
      return;
    }
    defaultPlanRequest
      .then(() => applyDefaultFraming())
      .catch(() => applyDefaultFraming());
  }, [artifactDemoPlan?.traceAssetName, dispatchMapLayers]);

  // --- The provider credential settings surface (§3): masked edit, explicit
  // Apply, opt-in persistence, and the honest Clear (the restart limitation
  // is stated in the sheet, never hidden) ---
  const openSettings = useCallback(() => setSettingsVisible(true), []);

  const editProviderCredential = useCallback(
    (descriptor: ProviderCredentialDescriptor, credential: string) => {
      dispatchProviderCredential({
        type: 'credentialEdited',
        providerKey: descriptor.provider_key,
        credential,
      });
    },
    [],
  );

  const toggleProviderCredentialRemember = useCallback(
    (descriptor: ProviderCredentialDescriptor, isRememberEnabled: boolean) => {
      dispatchProviderCredential({
        type: 'rememberToggled',
        providerKey: descriptor.provider_key,
        isRememberEnabled,
      });
      if (!isRememberEnabled) {
        // Opting out removes any earlier stored copy immediately: persistence
        // only ever exists behind the toggle.
        refappMapModule
          ?.clearStoredProviderCredential(descriptor.provider_key)
          .catch(() => undefined);
      }
    },
    [],
  );

  const applyProviderCredential = useCallback(
    (descriptor: ProviderCredentialDescriptor) => {
      const rowState = providerCredentialsState.rows[descriptor.provider_key];
      if (rowState === undefined || !refappMapModule) {
        return;
      }
      const applyPlan = planProviderCredentialApply(rowState);
      if (applyPlan === null) {
        return;
      }
      const providerKey = descriptor.provider_key;
      registerProviderCredential(descriptor, applyPlan.credential)
        .then(applyResult => {
          if (!applyResult.isApplied) {
            dispatchProviderCredential({
              type: 'applyRefused',
              providerKey,
              refusalDetailText: applyResult.detailText,
            });
            return;
          }
          dispatchProviderCredential({ type: 'applySucceeded', providerKey });
          setHasSessionProviderCredential(true);
          if (applyPlan.shouldPersist) {
            refappMapModule
              ?.storeProviderCredential(providerKey, applyPlan.credential)
              .then(isStored => {
                if (!isStored) {
                  dispatchProviderCredential({
                    type: 'storageFailed',
                    providerKey,
                    failureText: appStrings.providerCredentialUnavailableReasonText,
                  });
                }
              })
              .catch(storeError => {
                dispatchProviderCredential({
                  type: 'storageFailed',
                  providerKey,
                  failureText: String(storeError),
                });
              });
          } else {
            // Session-only: a previously remembered value must not survive.
            refappMapModule
              ?.clearStoredProviderCredential(providerKey)
              .catch(() => undefined);
          }
        })
        .catch(applyError => {
          dispatchProviderCredential({
            type: 'applyRefused',
            providerKey,
            refusalDetailText: String(applyError),
          });
        });
    },
    [providerCredentialsState.rows, registerProviderCredential],
  );

  const clearProviderCredential = useCallback(
    (descriptor: ProviderCredentialDescriptor) => {
      const providerKey = descriptor.provider_key;
      dispatchProviderCredential({ type: 'cleared', providerKey });
      refappMapModule
        ?.clearStoredProviderCredential(providerKey)
        .catch(() => undefined);
    },
    [],
  );

  /**
   * One capability row's pick: `configureProvider` with the credential the
   * session holds for a credentialed provider and an empty string for a
   * keyless one. An accepted choice is remembered; a refusal shows the SDK's
   * own message verbatim and leaves the previous provider serving.
   */
  const selectCapabilityProvider = useCallback(
    (row: CapabilityProviderRow, providerKey: string) => {
      const sdk = sdkInstanceRef.current;
      if (sdk === null || row.isReadOnly) {
        return;
      }
      void applyCapabilityProviderSelection({
        gateway: sdk,
        row,
        providerKey,
        appliedCredential: appliedCapabilityProviderCredential,
      }).then(outcome => {
        if (outcome.isApplied) {
          setCapabilityProviderDetailTexts(previousDetailTexts => {
            const nextDetailTexts = new Map(previousDetailTexts);
            nextDetailTexts.delete(row.contract);
            return nextDetailTexts;
          });
          rememberPreferences(currentPreferences => ({
            ...currentPreferences,
            capabilityProviderKeys: {
              ...currentPreferences.capabilityProviderKeys,
              [capabilityRegistryName(row.contract)]: providerKey,
            },
          }));
          return;
        }
        if (outcome.detailText !== '') {
          const rowDetailText =
            outcome.nextStepText === ''
              ? outcome.detailText
              : `${outcome.detailText} ${outcome.nextStepText}`;
          setCapabilityProviderDetailTexts(previousDetailTexts =>
            new Map(previousDetailTexts).set(row.contract, rowDetailText),
          );
        }
      });
    },
    [appliedCapabilityProviderCredential, rememberPreferences],
  );

  // The remembered per-capability choices re-apply once the credential
  // restore settled, so a credentialed provider finds the key the session
  // already holds. A restore never rewrites the record.
  useEffect(() => {
    if (
      !arePreferencesHydrated ||
      !isSdkCreated ||
      !hasCredentialRestoreSettled ||
      hasRestoredCapabilityProvidersRef.current
    ) {
      return;
    }
    hasRestoredCapabilityProvidersRef.current = true;
    const sdk = sdkInstanceRef.current;
    if (sdk === null) {
      return;
    }
    const rememberedProviderKeys = preferencesRef.current.capabilityProviderKeys;
    if (Object.keys(rememberedProviderKeys).length === 0) {
      return;
    }
    void (async () => {
      let rows: CapabilityProviderRow[];
      try {
        rows = buildCapabilityProviderRows(
          await sdk.providerRegistrations(),
          providerDescriptors,
          appliedCapabilityProviderCredential,
          capabilityProviderDetailTexts,
        );
      } catch {
        return;
      }
      for (const row of rows) {
        if (row.isReadOnly) {
          continue;
        }
        const providerKey =
          rememberedProviderKeys[capabilityRegistryName(row.contract)];
        if (providerKey === undefined) {
          continue;
        }
        const outcome = await applyCapabilityProviderSelection({
          gateway: sdk,
          row,
          providerKey,
          appliedCredential: appliedCapabilityProviderCredential,
        });
        if (!outcome.isApplied && outcome.detailText !== '') {
          const rowDetailText =
            outcome.nextStepText === ''
              ? outcome.detailText
              : `${outcome.detailText} ${outcome.nextStepText}`;
          setCapabilityProviderDetailTexts(previousDetailTexts =>
            new Map(previousDetailTexts).set(row.contract, rowDetailText),
          );
        }
      }
      await loadCapabilityProviderRows(capabilityProviderDetailTexts);
    })();
  }, [
    arePreferencesHydrated,
    isSdkCreated,
    hasCredentialRestoreSettled,
    appliedCapabilityProviderCredential,
    capabilityProviderDetailTexts,
    loadCapabilityProviderRows,
  ]);

  /** One Directions mode choice: remembered for the next plan and relaunch. */
  const selectDirectionsTransportationMode = useCallback(
    (transportationModeName: DirectionsTransportationModeName) => {
      rememberPreferences(currentPreferences => ({
        ...currentPreferences,
        directionsTransportationMode: transportationModeName,
      }));
    },
    [rememberPreferences],
  );

  /** One Directions strategy choice: remembered for the next car plan. */
  const selectDirectionsRouteStrategy = useCallback(
    (routeStrategyName: DirectionsRouteStrategyName) => {
      rememberPreferences(currentPreferences => ({
        ...currentPreferences,
        directionsRouteStrategy: routeStrategyName,
      }));
    },
    [rememberPreferences],
  );

  /**
   * One base-map row's act (M9's native base-map switch): registers the
   * chosen source onto the SDK's raster-tile slot — a credential-less source
   * through the provider-neutral URL-template registration, a credentialed
   * one by re-registering its already-applied key — then asks the map view to
   * re-resolve its provider so the switch shows on the next frame. A refusal
   * reports verbatim and leaves the previously serving base map in place,
   * because the SDK validates the whole declaration before the slot moves.
   */
  const selectBaseMap = useCallback(
    (choice: BaseMapChoice) => {
      const descriptor = choice.descriptor;
      const sdk = sdkInstanceRef.current;
      if (sdk === null) {
        setErrorReport({
          variantLabel: appStrings.baseMapSectionHeaderText,
          detailText: appStrings.nativeSdkUnavailableText,
        });
        return;
      }
      const registrationPlan = baseMapRegistrationPlan(
        descriptor,
        providerReferenceAppUserAgent(compiledVersion ?? ''),
      );
      // The attached view holds the provider it resolved when it attached and
      // keeps its pooled tiles by coordinate: without this the previous
      // imagery would stay until the camera reached new tiles.
      const recordServedBaseMap = () => {
        setSelectedBaseMapProviderKey(descriptor.provider_key);
        // An explicit choice wins over the keyless styled base.
        pendingKeylessVectorBaseChoiceRef.current = null;
        // A raster selection replaces a styled vector base: the vector base
        // draws over the raster tiles, so leaving it installed would keep
        // painting over the newly selected imagery.
        mapViewRef.current?.clearVectorTileSource();
        setSelectedVectorBaseChoiceKey(null);
        setErrorReport(null);
        mapViewRef.current?.reloadTileSource();
        rememberPreferences(currentPreferences => ({
          ...currentPreferences,
          baseMap: { providerKey: descriptor.provider_key, styleId: null },
        }));
      };
      const reportRefusal = (detailText: string) => {
        setErrorReport({
          variantLabel: appStrings.baseMapSectionHeaderText,
          detailText,
        });
      };
      if (registrationPlan.kind === 'credentialedRestore') {
        sdk
          .restoreCredentialedBaseMap(registrationPlan.providerKey)
          .then(restorationResult => {
            if (!restorationResult.isApplied) {
              reportRefusal(restorationResult.detailText);
              return;
            }
            // The re-registration repeats the credential and installs the
            // tile cache again, so the offline card shows its genuine line.
            setCacheState(registeredCacheStateRef.current);
            recordServedBaseMap();
          })
          .catch(restorationError => reportRefusal(String(restorationError)));
        return;
      }
      sdk
        .registerUrlTemplateRasterTileSource(registrationPlan.source)
        .then(registrationResult => {
          if (!registrationResult.isApplied) {
            reportRefusal(registrationResult.detailText);
            return;
          }
          // A credential-less source dropped the tile-cache installation,
          // whose terms belonged to the previous provider: the offline card
          // must say so rather than keep the old promise.
          setCacheState({
            isRegistered: false,
            stateText: appStrings.baseMapUncachedNoteText,
          });
          recordServedBaseMap();
        })
        .catch(registrationError => reportRefusal(String(registrationError)));
    },

    [compiledVersion, rememberPreferences],
  );

  /**
   * One styled-vector-base row's act: installs the registry
   * descriptor's vector tileset with the chosen committed style on the
   * package MapView, so every following frame draws the styled base over the
   * raster tiles and beneath the app's own overlays. A refusal reports and
   * leaves the previously serving base in place; the selection records only a
   * base the SDK accepted.
   */
  const selectVectorBase = useCallback((choice: VectorBaseMapChoice) => {
    const mapView = mapViewRef.current;
    if (mapView === null) {
      setErrorReport({
        variantLabel: appStrings.baseMapSectionHeaderText,
        detailText: appStrings.baseMapViewNotReadyText,
      });
      return;
    }
    installVectorBaseChoice(mapView, choice)
      .then(accepted => {
        if (!accepted) {
          // The view reported the refusal through its failure event; the
          // selection keeps the previous base, so nothing else changes.
          return;
        }
        pendingKeylessVectorBaseChoiceRef.current = null;
        setSelectedVectorBaseChoiceKey(vectorBaseChoiceKey(choice));
        setErrorReport(null);
        rememberPreferences(currentPreferences => ({
          ...currentPreferences,
          baseMap: {
            providerKey: choice.descriptor.provider_key,
            styleId: choice.style.style_id,
          },
        }));
      })
      .catch(selectionError =>
        setErrorReport({
          variantLabel: appStrings.baseMapSectionHeaderText,
          detailText: String(selectionError),
        }),
      );
  }, [installVectorBaseChoice, rememberPreferences]);

  /**
   * The diagnostics controller: it subscribes to nothing until the pane
   * opens, then follows the reference-application policy (Info on open, an
   * explicit Debug switch, a 100-event window coalesced every 500 ms).
   */
  const diagnosticsController = useMemo(
    () =>
      new DiagnosticsStreamController(
        {
          subscribe: (minimumSeverityOrdinal, onEvent) => {
            const removeListener = addControllerListener(
              'RefappDiagnosticEvent',
              payload => onEvent(diagnosticEventFromPayload(payload)),
            );
            refappMapModule?.startDiagnosticEvents?.(
              minimumSeverityOrdinal,
              diagnosticsBacklogFlushLimit,
            );
            return () => {
              removeListener();
              refappMapModule?.stopDiagnosticEvents?.();
            };
          },
          flushBacklog: () => {
            // `startDiagnosticEvents` registers the listener and flushes the
            // bounded backlog in the same native call.
          },
        },
        snapshot => {
          setDiagnosticEvents(snapshot.events);
          setDiagnosticDebugEvents(snapshot.isDebugEventsEnabled);
        },
      ),
    [],
  );

  // The pane's lifecycle drives the stream: opening either diagnostics
  // surface (the diagnostics sheet or the developer console) subscribes at
  // Info, and closing both stops the refresh cadence and removes the
  // listener. The lowered Debug gate stays for the session.
  useEffect(() => {
    if (diagnosticsVisible || isDeveloperConsoleOpen) {
      diagnosticsController.open();
    } else {
      diagnosticsController.close();
    }
  }, [diagnosticsController, diagnosticsVisible, isDeveloperConsoleOpen]);
  // A screen that unmounts while the pane is open removes the listener too.
  useEffect(
    () => () => diagnosticsController.close(),
    [diagnosticsController],
  );

  const openDiagnostics = useCallback(() => {
    // The subscription starts only when the pane's state commits; no console
    // line and no event listener exists before the visitor asks for diagnostics.
    setDiagnosticsVisible(true);
    if (!refappMapModule) {
      // No native seam: the pane still opens with the honest unavailable
      // line instead of silently doing nothing (§6.3's no-silent-failure rule).
      setDiagnosticsLines([appStrings.diagnosticsLinesUnavailableText]);
      return;
    }
    refappMapModule
      .getDiagnosticsLines()
      .then(lines => {
        setDiagnosticsLines([
          ...lines,
          'Compiled SDK version: ' +
            (compiledVersion ?? 'unavailable — the native module did not load'),
        ]);
      })
      .catch(loadingError => {
        setDiagnosticsLines(['Diagnostics query failed: ' + String(loadingError)]);
      });
  }, [compiledVersion, diagnosticsController]);

  const closeDiagnostics = useCallback(() => {
    diagnosticsController.close();
    setDiagnosticsVisible(false);
  }, [diagnosticsController]);

  const toggleDiagnosticDebugEvents = useCallback(() => {
    diagnosticsController.setDebugEvents(!diagnosticDebugEvents);
  }, [diagnosticDebugEvents, diagnosticsController]);

  const clearDiagnosticEvents = useCallback(() => {
    diagnosticsController.clear();
  }, [diagnosticsController]);

  /** The registry display name the console rows and cache line compose with. */
  const providerDisplayNameFor = useCallback(
    (providerKey: string) =>
      providerDescriptors.find(
        descriptor => descriptor.provider_key === providerKey,
      )?.display_name ?? providerKey,
    [],
  );

  const openDeveloperConsole = useCallback(() => {
    setDeveloperConsoleAlertWatermark(Date.now());
    setIsDeveloperConsoleOpen(true);
    // The cache summary reads the installed tile payload cache and the
    // serving traffic cache's counters; a shell without the readback (or with
    // no installed cache) keeps the honest line.
    sdkInstanceRef.current
      ?.trafficFlowCacheStatistics()
      .then(statistics => setTrafficCacheStatistics(statistics))
      .catch(() => setTrafficCacheStatistics(null));
    sdkInstanceRef.current
      ?.tileCacheStatistics()
      .then(statistics => setTileCacheStatistics(statistics))
      .catch(() => setTileCacheStatistics(null));
  }, []);

  const closeDeveloperConsole = useCallback(() => {
    // Everything the sheet showed is read; a later Warning or Error counts
    // again toward the control's dot.
    setDeveloperConsoleAlertWatermark(Date.now());
    setIsDeveloperConsoleOpen(false);
  }, []);

  // While the console is open, every coalesced event is already visible, so
  // the watermark keeps moving; when it closes, the last shown event stays
  // read and only a newer one lights the dot.
  useEffect(() => {
    if (!isDeveloperConsoleOpen || diagnosticEvents.length === 0) {
      return;
    }
    const latestTimestamp = Math.max(
      ...diagnosticEvents.map(event => event.timestampMillisecondsSinceEpoch),
    );
    setDeveloperConsoleAlertWatermark(currentWatermark =>
      latestTimestamp > currentWatermark ? latestTimestamp : currentWatermark,
    );
  }, [diagnosticEvents, isDeveloperConsoleOpen]);

  const hasUnreadDeveloperConsoleAlert = hasProviderConsoleAlertAfter(
    diagnosticEvents,
    developerConsoleAlertWatermark,
  );

  // The attribution line rides whichever bottom surface is present: the
  // sheet's visible height, or the trip drawer's measured height (§4.2's
  // static notice sits above the present dock; D6 keeps it visible in trips).
  const sheetVisibleHeight =
    sheetState === 'peek' ? peekHeight : sheetState === 'half' ? sheetHalfHeight : sheetFullHeight;
  const presentDockHeight = traceActive ? navigationDrawerHeight : sheetVisibleHeight;
  const attributionBottomOffset =
    presentDockHeight + themeSpacing.stackTight + safeAreaInsets.bottom;
  // The base-map switch (M9): the credential-less registry sources plus any
  // credential provider applied this session (its registration installs its
  // raster slot), in registry order.
  const appliedProviderKeys = useMemo(
    () =>
      new Set(
        credentialDescriptors
          .filter(descriptor => {
            const rowState =
              providerCredentialsState.rows[descriptor.provider_key];
            return rowState !== undefined && isProviderCredentialApplied(rowState);
          })
          .map(descriptor => descriptor.provider_key),
      ),
    [providerCredentialsState.rows],
  );
  const settingsBaseMapChoices = useMemo(
    () => baseMapChoices(providerDescriptors, appliedProviderKeys),
    [appliedProviderKeys],
  );
  const activeBaseMapDescriptor = activeBaseMap(
    settingsBaseMapChoices,
    selectedBaseMapProviderKey,
  );
  const settingsVectorBaseChoices = useMemo(
    () => vectorBaseMapChoices(providerDescriptors, appliedProviderKeys),
    [appliedProviderKeys],
  );
  const activeVectorBaseChoice = useMemo(
    () =>
      settingsVectorBaseChoices.find(
        choice => vectorBaseChoiceKey(choice) === selectedVectorBaseChoiceKey,
      ) ?? null,
    [settingsVectorBaseChoices, selectedVectorBaseChoiceKey],
  );
  const rememberedBaseMap = preferences.baseMap;
  const rememberedVectorBaseChoice = useMemo(() => {
    if (rememberedBaseMap === null || rememberedBaseMap.styleId === null) {
      return null;
    }
    const rememberedChoiceKey = `${rememberedBaseMap.providerKey}/${rememberedBaseMap.styleId}`;
    return (
      settingsVectorBaseChoices.find(
        choice => vectorBaseChoiceKey(choice) === rememberedChoiceKey,
      ) ?? null
    );
  }, [rememberedBaseMap, settingsVectorBaseChoices]);
  const rememberedRasterBaseChoice = useMemo(() => {
    if (rememberedBaseMap === null || rememberedBaseMap.styleId !== null) {
      return null;
    }
    return (
      settingsBaseMapChoices.find(
        choice =>
          choice.isServable &&
          choice.descriptor.provider_key === rememberedBaseMap.providerKey,
      ) ?? null
    );
  }, [rememberedBaseMap, settingsBaseMapChoices]);

  // The remembered base map restores once the session and the credential
  // checks have settled; an explicit remembered selection wins over the
  // keyless default. A selection that cannot serve in this session stays
  // unapplied and the keyless default stands until its key is applied.
  const hasRestoredRememberedBaseMapRef = useRef(false);
  useEffect(() => {
    if (
      !arePreferencesHydrated ||
      !isSdkCreated ||
      !hasCredentialRestoreSettled ||
      !isLaunchCredentialChecked ||
      hasRestoredRememberedBaseMapRef.current
    ) {
      return;
    }
    if (rememberedBaseMap === null) {
      return;
    }
    if (rememberedVectorBaseChoice !== null) {
      hasRestoredRememberedBaseMapRef.current = true;
      pendingRememberedVectorBaseChoiceRef.current = rememberedVectorBaseChoice;
      installPendingRememberedVectorBase();
      return;
    }
    if (rememberedRasterBaseChoice !== null) {
      hasRestoredRememberedBaseMapRef.current = true;
      selectBaseMap(rememberedRasterBaseChoice);
    }
  }, [
    arePreferencesHydrated,
    hasCredentialRestoreSettled,
    installPendingRememberedVectorBase,
    isLaunchCredentialChecked,
    isSdkCreated,
    rememberedBaseMap,
    rememberedRasterBaseChoice,
    rememberedVectorBaseChoice,
    selectBaseMap,
  ]);
  // --- The on-map layer control (docs/ReferenceAppUX.md §4.2) ---
  const trafficAvailability = useMemo(
    () => trafficLayerAvailability(providerDescriptors, appliedProviderKeys),
    [appliedProviderKeys],
  );
  const mapLayerBaseRows = useMemo(
    () =>
      buildMapLayerBaseRows(
        settingsBaseMapChoices,
        settingsVectorBaseChoices,
        activeBaseMapDescriptor?.provider_key ?? null,
        selectedVectorBaseChoiceKey,
      ),
    [
      activeBaseMapDescriptor,
      selectedVectorBaseChoiceKey,
      settingsBaseMapChoices,
      settingsVectorBaseChoices,
    ],
  );
  const enabledSceneKeys = rememberedSceneKeys(sceneSelections);
  const mapLayerSceneRows = useMemo(
    () =>
      buildMapLayerSceneRows(
        overlayScenes.map(({ sceneKey, scene }) => ({
          sceneKey,
          label: scene.label,
        })),
        rememberedSceneKeys(sceneSelections),
      ),
    [sceneSelections],
  );
  const keylessStyledBaseChoice = useMemo(
    () =>
      keylessVectorBaseChoice(
        providerDescriptors,
        providerRegistry.keyless_base_map,
      ),
    [],
  );
  const defaultRasterProviderKey =
    settingsBaseMapChoices.find(choice => choice.isServable)?.descriptor
      .provider_key ?? null;
  // The dot shows whenever the serving base is not the session's default:
  // the registry's keyless styled base (once installed) or its first servable
  // raster source.
  const isDefaultBaseServing =
    activeVectorBaseChoice !== null
      ? keylessStyledBaseChoice !== null &&
        vectorBaseChoiceKey(activeVectorBaseChoice) ===
          vectorBaseChoiceKey(keylessStyledBaseChoice)
      : activeBaseMapDescriptor?.provider_key === defaultRasterProviderKey;
  const isNonDefaultLayerSelected = isNonDefaultLayerActive({
    isDefaultBaseServing,
    isTrafficEnabled: mapLayersState.isTrafficEnabled,
    enabledSceneCount: enabledSceneKeys.length,
  });

  const openMapLayersPanel = useCallback(
    () => dispatchMapLayers({ type: 'panelOpened' }),
    [],
  );
  const closeMapLayersPanel = useCallback(
    () => dispatchMapLayers({ type: 'panelClosed' }),
    [],
  );
  /** The panel's close path returns accessibility focus to the control. */
  const returnFocusToMapLayersControl = useCallback(() => {
    // The modal unmounts on the close state write; focus moves once it has,
    // so the modal's own focus trap cannot take it back.
    setTimeout(() => {
      const controlNode = findNodeHandle(mapLayersControlRef.current);
      if (controlNode !== null && controlNode !== undefined) {
        AccessibilityInfo.setAccessibilityFocus(controlNode);
      }
    }, 0);
  }, []);
  /** One base row's act: the layers panel's source switch. */
  const selectMapLayerBaseRow = useCallback(
    (row: MapLayerBaseRow) => {
      const selection = row.selection;
      if (selection.kind === 'raster') {
        const choice = settingsBaseMapChoices.find(
          candidate =>
            candidate.descriptor.provider_key === selection.providerKey,
        );
        if (choice !== undefined) {
          selectBaseMap(choice);
        }
        return;
      }
      const choice = settingsVectorBaseChoices.find(
        candidate => vectorBaseChoiceKey(candidate) === selection.choiceKey,
      );
      if (choice !== undefined) {
        selectVectorBase(choice);
      }
    },
    [selectBaseMap, selectVectorBase, settingsBaseMapChoices, settingsVectorBaseChoices],
  );

  // The registry answer drives the switch; a configuration change that drops
  // the serving descriptor turns the layer off in the same transition.
  useEffect(() => {
    dispatchMapLayers({
      type: 'trafficConfigurationUpdated',
      availability: trafficAvailability,
    });
  }, [trafficAvailability]);

  /**
   * One traffic switch act: the choice is remembered only when the registry
   * can serve it now, so a keyless session keeps the previous choice instead
   * of recording a layer it never drew.
   */
  const setTrafficLayerEnabled = useCallback(
    (isEnabled: boolean) => {
      dispatchMapLayers({ type: 'trafficToggled', isEnabled });
      if (!isEnabled || isTrafficLayerAvailable(trafficAvailability)) {
        rememberPreferences(currentPreferences => ({
          ...currentPreferences,
          trafficLayerEnabled: isEnabled,
        }));
      }
    },
    [rememberPreferences, trafficAvailability],
  );

  // The remembered traffic choice restores once a serving provider can honour
  // it; a keyless session keeps the choice for a later launch instead of
  // pretending the layer is on.
  useEffect(() => {
    if (hasRestoredTrafficLayerRef.current || !arePreferencesHydrated) {
      return;
    }
    if (
      shouldRestoreTrafficLayer(
        preferences.trafficLayerEnabled,
        trafficAvailability,
      )
    ) {
      hasRestoredTrafficLayerRef.current = true;
      dispatchMapLayers({ type: 'trafficRestored' });
    } else if (trafficAvailability.state !== 'needsCredential') {
      // Nothing can restore this session; a later credential application goes
      // through the user's own switch instead.
      hasRestoredTrafficLayerRef.current = true;
    }
  }, [
    arePreferencesHydrated,
    preferences.trafficLayerEnabled,
    trafficAvailability,
  ]);

  /**
   * One traffic capability call for the settled camera's bounds. A request
   * that arrives while another runs is coalesced into one follow-up; an
   * answer whose generation the user already left is dropped.
   */
  const performTrafficRefresh = useCallback(async () => {
    if (!mapLayersState.isTrafficEnabled) {
      return;
    }
    if (trafficRefreshInFlightRef.current) {
      trafficRefreshPendingRef.current = true;
      return;
    }
    const sdk = sdkInstanceRef.current;
    const mapView = mapViewRef.current;
    const cameraPose = mapCameraPoseRef.current;
    if (sdk === null || mapView === null || cameraPose === null) {
      return;
    }
    const bounds = geographicBoundsForCamera(
      cameraPose,
      screenDimensions.width * densityScale,
      screenDimensions.height * densityScale,
    );
    if (bounds === null) {
      return;
    }
    trafficRefreshInFlightRef.current = true;
    const requestGeneration = trafficRequestGenerationRef.current;
    dispatchMapLayers({ type: 'trafficRefreshStarted' });
    try {
      const trafficFlowSegments: TrafficFlowData[] =
        await sdk.fetchTrafficFlow(bounds);
      if (requestGeneration !== trafficRequestGenerationRef.current) {
        return;
      }
      await mapView.addOverlay(trafficFlowOverlayIdentifier, {
        kind: MapOverlayKind.TrafficFlow,
        widthPixels: trafficFlowOverlayWidthPixels,
        trafficFlowSegments,
      });
      dispatchMapLayers({ type: 'trafficRefreshSucceeded' });
    } catch (refreshError) {
      if (requestGeneration !== trafficRequestGenerationRef.current) {
        return;
      }
      dispatchMapLayers({
        type: 'trafficRefreshFailed',
        detailText: verbatimErrorText(refreshError),
        isProviderNotConfigured:
          isProviderNotConfiguredFailure(refreshError),
      });
    } finally {
      trafficRefreshInFlightRef.current = false;
      if (trafficRefreshPendingRef.current) {
        trafficRefreshPendingRef.current = false;
        setTrafficRefreshNonce(nonce => nonce + 1);
      }
    }
  }, [
    densityScale,
    mapLayersState.isTrafficEnabled,
    screenDimensions.height,
    screenDimensions.width,
  ]);

  // Every camera change restarts the debounce; turning the layer off cancels
  // the pending refresh, invalidates an answer in flight and removes the
  // overlay, so the layer is cleanly off.
  useEffect(() => {
    if (!mapLayersState.isTrafficEnabled) {
      trafficRequestGenerationRef.current += 1;
      trafficRefreshPendingRef.current = false;
      void mapViewRef.current?.removeOverlay?.(trafficFlowOverlayIdentifier);
      return;
    }
    const debounceTimer = setTimeout(() => {
      void performTrafficRefresh();
    }, trafficRefreshDebounceMilliseconds);
    return () => clearTimeout(debounceTimer);
  }, [
    mapCameraPoseKey,
    mapLayersState.isTrafficEnabled,
    performTrafficRefresh,
    trafficRefreshNonce,
  ]);

  /** Records the camera pose and restarts the traffic debounce on a change. */
  const noteMapCameraChanged = useCallback((camera: MapCamera) => {
    mapCameraPoseRef.current = camera;
    const poseKey =
      `${camera.latitudeDegrees},${camera.longitudeDegrees},` +
      `${camera.zoomLevel},${camera.bearingDegrees},${camera.pitchDegrees}`;
    if (poseKey !== mapCameraPoseKeyRef.current) {
      mapCameraPoseKeyRef.current = poseKey;
      setMapCameraPoseKey(poseKey);
    }
  }, []);

  // The attribution follows the serving base map, never the union of every
  // registrable provider: a base-map switch repaints the notice too. A styled
  // vector base contributes its own style's design line, and the traffic
  // layer appends the serving descriptor's line while it draws.
  const trafficAttributionText = mapLayersState.isTrafficEnabled
    ? trafficLayerAttributionText(trafficAvailability)
    : null;
  const baseAttributionText =
    activeVectorBaseChoice !== null
      ? vectorBaseAttributionText(activeVectorBaseChoice)
      : providerAttributionText(activeBaseMapDescriptor);
  const attributionText =
    trafficAttributionText === null
      ? baseAttributionText
      : `${baseAttributionText} · ${trafficAttributionText}`;
  const attributionTermsNotes =
    providerAttributionTermsNotes(activeBaseMapDescriptor);
  // §6.1/§6.5: the chip keeps a neutral tone whenever it does not know a
  // state; keyless renders the neutral prompt instead of a provider failure.
  const readinessChip = readinessChipPresentation(
    readiness,
    !hasAppliedProviderCredential,
  );
  // §4.1/§6.2: the overlay scenes need the live map surface, so the group is
  // disabled-with-reason until the readiness ladder reports ready (D7).
  const areSceneOverlaysLive = readiness.state === 'ready';

  return (
    <View style={styles.screen} testID="refapp-screen">
      <MapView
        ref={mapViewRef}
        style={styles.mapFill}
        testID="refapp-map"
        topBarInsetPixels={densityScale * safeAreaInsets.top}
        bottomBarInsetPixels={densityScale * safeAreaInsets.bottom}
        leftBarInsetPixels={densityScale * safeAreaInsets.left}
        rightBarInsetPixels={densityScale * safeAreaInsets.right}
        onMapReady={() => {
          setReadiness({
            state: 'ready',
            text:
              appStrings.readinessReadyPrefixText +
              (compiledVersion ?? 'unknown') +
              ')',
          });
          installPendingRememberedVectorBase();
          installPendingKeylessVectorBase();
          // Seed the traffic layer's query box with the first adopted camera;
          // later changes restart its debounce through the same note path.
          const firstCamera = mapViewRef.current?.getCamera();
          if (firstCamera !== null && firstCamera !== undefined) {
            noteMapCameraChanged(firstCamera);
          }
        }}
        onMapClick={() => setMapTapCount(tapCount => tapCount + 1)}
        onCameraChanged={noteMapCameraChanged}
        onError={failure => {
          // A map failure that is the contract's missing-provider variant
          // renders the shared consumer sentence; every other failure keeps
          // the SDK's own words.
          const detailText = capabilityUnavailableText(failure.message);
          setReadiness({
            state: failure.phase === 'draw' ? 'degraded' : 'failed',
            text:
              failure.phase === 'draw'
                ? appStrings.readinessDegradedPrefixText + detailText
                : appStrings.readinessFailedPrefixText + detailText,
            detail: detailText,
          });
          setErrorReport({ variantLabel: failure.phase, detailText });
        }}
        onFrameStatistics={statistics => {
          setTileStatistics({
            live: true,
            requestedTileCount: statistics.requestedTileCount,
            presentTileCount: statistics.presentTileCount,
            fallbackTileCount: statistics.fallbackTileCount,
            deferredTileCount: statistics.deferredTileCount,
            unavailableTileCount: statistics.unavailableTileCount,
            overlayTriangleCount: statistics.overlayTriangleCount,
            framesDrawnCount: 0,
            framesDeferredCount: 0,
            framesFailedCount: 0,
          });
        }}
      />

      <TurnBannerStack
        traceActive={traceActive}
        traceUpdate={traceUpdate}
        deviationText={deviationText}
        safeAreaTop={safeAreaInsets.top}
        styles={styles}
      />

      <View
        style={[styles.readinessBar, { top: safeAreaInsets.top + themeSpacing.stackGroup }]}>
        <Pressable
          style={styles.readinessChip}
          onPress={openDiagnostics}
          testID="readiness-chip">
          <Text
            style={[
              styles.readinessChipText,
              { color: themeColors[readinessToneColorKeys[readinessChip.tone]] },
            ]}
            numberOfLines={1}
            testID="readiness-chip-text">
            {readinessChip.text}
          </Text>
        </Pressable>
        <Pressable
          style={styles.settingsGearButton}
          onPress={openSettings}
          accessibilityRole="button"
          accessibilityLabel={appStrings.settingsGearAccessibilityText}
          testID="open-settings">
          <Text style={styles.settingsGearGlyph}>{appStrings.settingsGearGlyph}</Text>
        </Pressable>
      </View>

      {errorReport ? (
        <View style={[styles.errorStrip, { top: safeAreaInsets.top + errorStripGap }]} testID="error-strip">
          <Text style={styles.errorStripVariant}>{errorReport.variantLabel}</Text>
          <Text style={styles.errorStripDetail}>{errorReport.detailText}</Text>
        </View>
      ) : null}

      {/* The floating bottom stack: the scale bar, the disabled-with-reason
          strip and the attribution notice, in the Compose dock's order, all
          riding the present dock's measured height (§4.2, D6). One column
          instead of three absolute offsets keeps the lines from colliding. */}
      <View
        style={[styles.floatingBottomStack, { bottom: attributionBottomOffset }]}
        pointerEvents="box-none">
        <ScaleBarOverlay
          groundMetersPerScreenPixel={cameraReadback?.groundMetersPerScreenPixel}
          styles={styles}
        />

        {/* §6.2/D8: the guided walk's disabled-with-reason strip, docked above
            the attribution so the peek's action explains itself. */}
        {guidedWalkUnavailableReason ? (
          <View
            style={styles.floatingReasonStrip}
            testID="guided-walk-unavailable-reason">
            <Text style={styles.reasonStripText}>{guidedWalkUnavailableReason}</Text>
          </View>
        ) : null}

      </View>

      {/* The attribution info control (§1/§4.2): the compact
          circular "ⓘ" at the map's bottom-end corner; its panel carries the
          registry's attribution lines and terms notes. It rides whichever
          dock is present, like the line it replaced (D6). */}
      <View
        style={[styles.bottomEndDock, { bottom: attributionBottomOffset }]}
        pointerEvents="box-none">
        {/* The on-map layer control docks directly above the attribution
            notice (docs/ReferenceAppUX.md §4.2). */}
        <MapLayersControl
          ref={mapLayersControlRef}
          isNonDefaultLayerActive={isNonDefaultLayerSelected}
          onPress={openMapLayersPanel}
          styles={styles}
        />
        <AttributionInfoControl
          isPanelOpen={isAttributionPanelOpen}
          onTogglePanel={() => setIsAttributionPanelOpen(open => !open)}
          onClosePanel={() => setIsAttributionPanelOpen(false)}
          attributionText={attributionText}
          attributionTermsNotes={attributionTermsNotes}
          styles={styles}
        />
      </View>

      {mapLayersState.isOpen ? (
        <MapLayersPanel
          baseRows={mapLayerBaseRows}
          onSelectBaseRow={selectMapLayerBaseRow}
          trafficAvailability={trafficAvailability}
          isTrafficEnabled={mapLayersState.isTrafficEnabled}
          onToggleTraffic={setTrafficLayerEnabled}
          sceneRows={mapLayerSceneRows}
          areSceneOverlaysAvailable={areSceneOverlaysLive}
          sceneOverlaysUnavailableReasonText={
            appStrings.demoScenesUnavailableReasonText + readinessChip.text
          }
          onToggleScene={toggleSceneChip}
          onClearScenes={clearSceneSelections}
          locationStatus={mapLayersState.locationStatus}
          onStopDeviceLocation={() => {
            void stopDeviceLocation();
          }}
          statusLineText={mapLayersState.statusLineText}
          bottomInset={safeAreaInsets.bottom}
          onClose={closeMapLayersPanel}
          onReturnFocus={returnFocusToMapLayersControl}
          styles={styles}
        />
      ) : null}

      {/* The map control cluster (§4.2): the compass at the top-end, the
          zoom/my-location/2D-3D stack at the end edge — all above the
          settled dock. */}
      <CompassControl
        bearingDegrees={cameraReadback?.bearingDegrees ?? 0}
        traceActive={traceActive}
        isHoldingNorthUp={isHoldingNorthUp}
        safeAreaTop={safeAreaInsets.top}
        onPress={toggleCompassMode}
        styles={styles}
      />
      <MapControlCluster
        isThreeDCameraEnabled={cameraReadback?.isThreeDCameraEnabled ?? false}
        isRecentering={traceActive && cameraReadback?.isFollowing === false}
        onZoomIn={zoomInByControl}
        onZoomOut={zoomOutByControl}
        onMyLocation={centerOnMyLocation}
        onTogglePlane={toggleCameraPlane}
        styles={styles}
      />
      <DeveloperConsole
        events={diagnosticEvents}
        isOpen={isDeveloperConsoleOpen}
        hasUnreadAlert={hasUnreadDeveloperConsoleAlert}
        onOpen={openDeveloperConsole}
        onClose={closeDeveloperConsole}
        displayNameForProvider={providerDisplayNameFor}
        tileStatistics={tileStatistics}
        tileCacheStatistics={tileCacheStatistics}
        trafficAvailability={trafficAvailability}
        trafficCacheStatistics={trafficCacheStatistics}
        controlBottomOffset={attributionBottomOffset}
        styles={styles}
      />

      {traceActive ? (
        <NavigationDrawer
          traceUpdate={traceUpdate}
          playbackState={playbackState}
          isDeviceNavigationActive={isDeviceNavigationActive}
          onStopDeviceNavigation={stopDeviceNavigation}
          safeAreaBottom={safeAreaInsets.bottom}
          onExit={exitSimulatedNavigation}
          onTogglePause={toggleSimulatedNavigationPause}
          onSelectSpeed={selectSimulatedNavigationSpeed}
          onSeek={seekSimulatedNavigation}
          speechConsumerState={speechConsumerState}
          speechEngineState={speechEngineState}
          onToggleSpeech={toggleSpeech}
          onMeasuredHeight={setNavigationDrawerHeight}
          styles={styles}
        />
      ) : (
        <View style={styles.dockHitSlop} pointerEvents="box-none">
          <Animated.View
            style={[
              styles.sheetDock,
              {
                height: sheetFullHeight,
                bottom: safeAreaInsets.bottom,
                transform: [{ translateY: translateYValue }],
              },
            ]}
            {...sheetPanResponder.panHandlers}>
            <View style={styles.sheetGrabber} />
            <ScrollView
              style={styles.sheetScroll}
              contentContainerStyle={styles.sheetContent}
              nestedScrollEnabled>
              {/* The peek rows (§4.2's two-row peek) */}
              <Pressable
                style={styles.peekSearchField}
                onPress={() => openSearchView('destination')}
                testID="peek-search-field">
                <Text style={styles.peekSearchFieldText}>{appStrings.whereToText}</Text>
              </Pressable>
              <View style={styles.peekActionRow}>
                <Pressable
                  style={[
                    styles.actionControl,
                    styles.actionControlPrimary,
                    canStartGuidedWalk ? null : styles.actionControlDisabled,
                  ]}
                  onPress={startSimulatedNavigation}
                  disabled={!canStartGuidedWalk}
                  testID="start-simulated-navigation">
                  <Text style={[styles.actionControlText, styles.actionControlTextPrimary]}>
                    {appStrings.peekPrimaryActionText}
                  </Text>
                </Pressable>
                <Pressable
                  style={styles.actionControl}
                  onPress={() => openSearchView('origin')}
                  testID="set-origin-button">
                  <Text style={styles.actionControlText}>{appStrings.originFieldHintText}</Text>
                </Pressable>
              </View>

              {/* The trip source Start will replay (§1): a planned route when
                  one stands, otherwise the selected demo plan. */}
              {!planState.isPlanned && selectedDemoPlan !== null ? (
                <Text style={styles.peekTripSource} testID="peek-trip-source">
                  {appStrings.planTripSourceDemoPlanFormat.replace(
                    '{plan}',
                    selectedDemoPlan.label,
                  )}
                </Text>
              ) : null}

              {contentMode === 'search' ? (
                <PlanSearchView
                  originSearchText={originSearchText}
                  destinationSearchText={destinationSearchText}
                  searchRows={searchRows}
                  searchRowsAreDemoPlaces={searchRowsAreDemoPlaces}
                  demoSearchNoMatchText={demoSearchNoMatchText}
                  searchInFlight={searchInFlight}
                  assignedOriginText={assignedOriginText}
                  assignedDestinationText={assignedDestinationText}
                  planRouteReasonText={planRouteReasonText}
                  canPlanRoute={canPlanRoute}
                  directionsControl={directionsControl}
                  styles={styles}
                  myLocationDisabledReason={myLocationDisabledReason}
                  myLocationFilledFields={myLocationFilledFields}
                  autocompleteProviderSelection={autocompleteProviderSelection}
                  autocompleteGateway={autocompleteGateway}
                  onOriginSearchTextChange={editOriginSearchText}
                  onDestinationSearchTextChange={editDestinationSearchText}
                  onSearch={runPlanSearch}
                  onSelectAutocompleteSuggestion={assignPlanFieldRow}
                  onAssign={assignSearchRow}
                  onPlanRoute={planRouteFromFields}
                  onSelectDirectionsTransportationMode={
                    selectDirectionsTransportationMode
                  }
                  onSelectDirectionsRouteStrategy={selectDirectionsRouteStrategy}
                  onFieldFocused={focusPlanField}
                  onFillMyLocation={fillPlanFieldWithMyLocation}
                  onSwapFields={swapPlanFields}
                  onDone={() => {
                    setContentMode('home');
                    settleToSheetState('half');
                  }}
                />
              ) : (
                <>
                  {planState.isPlanned ? (
                    <RouteSummaryCard
                      planState={planState}
                      styles={styles}
                      onStart={startSimulatedNavigation}
                      onEdit={() => openSearchView('destination')}
                    />
                  ) : null}
                  <Text style={styles.sheetHeader}>Location</Text>
                  <Pressable
                    style={[styles.actionControl, isDeviceLocationActive && styles.chipActive]}
                    onPress={toggleDeviceLocation}
                    testID="device-location-control">
                    <Text style={styles.actionControlText}>
                      {isDeviceLocationActive ? 'Hide my location' : 'Show my location'}
                    </Text>
                  </Pressable>
                  <Pressable
                    style={[
                      styles.actionControl,
                      isDeviceNavigationActive && styles.chipActive,
                    ]}
                    onPress={toggleDeviceNavigation}
                    testID="device-navigation-control">
                    <Text style={styles.actionControlText}>
                      {isDeviceNavigationActive
                        ? deviceNavigationStopText
                        : deviceNavigationStartText}
                    </Text>
                  </Pressable>
                  {deviceLocationStatusText !== null ? (
                    <Text style={styles.sheetHeader} testID="device-location-status">
                      {deviceLocationStatusText}
                    </Text>
                  ) : null}
                  <OfflineMapsCard
                    cacheState={cacheState}
                    tileStatistics={tileStatistics}
                    isOfflineRouteInFlight={offlineRouteInFlight}
                    onPlanOfflineRoute={planOfflineRouteFromArchive}
                    demoRegionNameText={demoOfflineRegion?.displayName ?? null}
                    demoRegionTileCount={offlineRegionTileCount}
                    isOfflineCacheRegistered={isOfflineCacheRegistered}
                    regionProgress={offlineRegionProgress}
                    regionStatus={offlineRegionStatus}
                    regionErrorText={offlineRegionErrorText}
                    isRegionDownloadInFlight={offlineRegionInFlight}
                    storedRegions={storedOfflineRegions}
                    isRegionDeleted={offlineRegionDeleted}
                    onDownloadRegion={downloadOfflineRegion}
                    onCancelRegionDownload={cancelOfflineRegionDownload}
                    onDeleteRegion={deleteOfflineRegion}
                    styles={styles}
                  />
                  <Text style={styles.sheetHeader}>{appStrings.themeRowTitleText}</Text>
                  <View style={styles.chipRow} testID="theme-row">
                    <Pressable
                      style={styles.chip}
                      onPress={cycleThemeChoice}
                      accessibilityRole="button"
                      testID="theme-toggle">
                      <Text style={styles.chipText}>
                        {themeChoiceSwitchLabel(themeChoice)}
                      </Text>
                    </Pressable>
                  </View>
                  <Text style={styles.sheetHeader}>{appStrings.cameraSectionHeader}</Text>
                  <View style={styles.chipRow} testID="preset-chips">
                    {cameraPresets.map(preset => (
                      <Pressable
                        key={preset.id}
                        style={[styles.chip, activePresetId === preset.id && styles.chipActive]}
                        onPress={() => applyPreset(preset)}
                        testID={'preset-' + preset.id}>
                        <Text
                          style={[styles.chipText, activePresetId === preset.id && styles.chipTextActive]}>
                          {preset.label}
                        </Text>
                      </Pressable>
                    ))}
                  </View>
                </>
              )}
            </ScrollView>
          </Animated.View>
        </View>
      )}

      {diagnosticsVisible ? (
        <View style={styles.diagnosticsScrim} testID="diagnostics-sheet">
          <Pressable style={styles.diagnosticsBackdrop} onPress={closeDiagnostics} />
          <View style={styles.diagnosticsSheet}>
            <View style={styles.diagnosticsHeaderRow}>
              <Text style={styles.diagnosticsTitle}>{appStrings.diagnosticsTitle}</Text>
              <Pressable
                style={styles.retryControl}
                onPress={closeDiagnostics}
                testID="diagnostics-close">
                <Text style={styles.retryControlText}>
                  {appStrings.settingsDoneText}
                </Text>
              </Pressable>
            </View>
            {diagnosticsLines.map((line, lineIndex) => (
              <Text key={lineIndex} style={styles.diagnosticsLine}>
                {line}
              </Text>
            ))}
            {/* The confirmed-tap count beside the native camera lines
                (§5.2's one-click-per-single-tap rule). */}
            <Text style={styles.diagnosticsLine} testID="diagnostics-map-tap-count">
              {`map taps ${mapTapCount}`}
            </Text>
            <Text style={styles.diagnosticsSectionHeader}>
              {appStrings.diagnosticsRecentEventsTitleText}
            </Text>
            <View style={styles.diagnosticsEventList} testID="diagnostics-event-list">
              {diagnosticEvents.length === 0 ? (
                <Text style={styles.diagnosticsLine} testID="diagnostics-no-events">
                  {appStrings.diagnosticsNoEventsText}
                </Text>
              ) : (
                diagnosticEvents.map((event, eventIndex) => (
                  <Text
                    key={`${event.timestampMillisecondsSinceEpoch}-${eventIndex}`}
                    style={styles.diagnosticsLine}
                    testID={'diagnostics-event-' + eventIndex}>
                    {`${event.severity} · ${event.subsystemTarget} · ${event.messageLine}`}
                  </Text>
                ))
              )}
            </View>
            <View style={styles.diagnosticsControlRow}>
              <Pressable
                style={styles.retryControl}
                onPress={toggleDiagnosticDebugEvents}
                disabled={diagnosticDebugEvents}
                testID="diagnostics-debug-toggle">
                <Text style={styles.retryControlText}>
                  {diagnosticDebugEvents
                    ? appStrings.diagnosticsHideDebugEventsText
                    : appStrings.diagnosticsShowDebugEventsText}
                </Text>
              </Pressable>
              <Pressable
                style={styles.retryControl}
                onPress={clearDiagnosticEvents}
                testID="diagnostics-clear">
                <Text style={styles.retryControlText}>
                  {appStrings.diagnosticsClearActionText}
                </Text>
              </Pressable>
            </View>
            <Text
              style={styles.diagnosticsLine}
              testID="diagnostics-stream-scope-note">
              {appStrings.diagnosticsStreamScopeNoteText}
            </Text>
            <Pressable
              style={styles.retryControl}
              onPress={() => refappMapModule?.retrySurfaceAttach()}
              testID="retry-surface">
              <Text style={styles.retryControlText}>{appStrings.retrySurfaceText}</Text>
            </Pressable>
          </View>
        </View>
      ) : null}

      {settingsVisible ? (
        <View style={styles.settingsScrim} testID="settings-sheet">
          <Pressable
            style={styles.settingsBackdrop}
            onPress={() => setSettingsVisible(false)}
          />
          <View
            style={[
              styles.settingsSheet,
              {
                maxHeight: sheetFullHeight,
                marginBottom:
                  themeSpacing.gutterInline + safeAreaInsets.bottom,
              },
            ]}
            testID="settings-panel">
            {/* The title and Close action stay pinned above the scrolling
                content, so a settings list taller than the panel cap never
                puts the way out out of reach. */}
            <View style={styles.settingsHeaderRow}>
              <Text style={styles.settingsTitle}>
                {appStrings.settingsTitleText}
              </Text>
              <Pressable
                style={styles.retryControl}
                onPress={() => setSettingsVisible(false)}
                accessibilityRole="button"
                testID="settings-done">
                <Text style={styles.retryControlText}>
                  {appStrings.settingsDoneText}
                </Text>
              </Pressable>
            </View>
            <ScrollView
              style={styles.settingsScroll}
              contentContainerStyle={styles.settingsContent}
              nestedScrollEnabled
              testID="settings-scroll">
            <Text
              style={styles.settingsSectionHeader}
              testID="capability-provider-section">
              {appStrings.capabilityProviderSectionHeaderText}
            </Text>
            {capabilityProviderRows.map(capabilityProviderRow => (
              <CapabilityProviderRowView
                key={capabilityProviderRow.contract}
                row={capabilityProviderRow}
                styles={styles}
                onSelect={providerKey =>
                  selectCapabilityProvider(capabilityProviderRow, providerKey)
                }
              />
            ))}
            <Text style={styles.settingsSectionHeader}>
              {appStrings.providerCredentialsSectionHeaderText}
            </Text>
            {!hasAppliedProviderCredential ? (
              <Text style={styles.settingsKeylessLine} testID="provider-keyless-line">
                {appStrings.providerCredentialKeylessNeutralText}
              </Text>
            ) : null}
            {credentialDescriptors.map(descriptor => {
              const rowState =
                providerCredentialsState.rows[descriptor.provider_key];
              if (rowState === undefined) {
                return null;
              }
              return (
                <ProviderCredentialRow
                  key={descriptor.provider_key}
                  descriptor={descriptor}
                  rowState={rowState}
                  hintText={descriptor.settings_hint}
                  styles={styles}
                  onChangeCredential={credential =>
                    editProviderCredential(descriptor, credential)
                  }
                  onToggleRemember={isRememberEnabled =>
                    toggleProviderCredentialRemember(descriptor, isRememberEnabled)
                  }
                  onApply={() => applyProviderCredential(descriptor)}
                  onClear={() => clearProviderCredential(descriptor)}
                />
              );
            })}
            <Text style={styles.settingsRestartNote} testID="provider-restart-note">
              {appStrings.providerCredentialRestartNoteText}
            </Text>
            <Text style={styles.settingsSectionHeader}>
              {appStrings.settingsResetPreferencesText}
            </Text>
            <Pressable
              style={styles.actionControl}
              onPress={resetRememberedChoices}
              accessibilityRole="button"
              testID="settings-reset-preferences">
              <Text style={styles.actionControlText}>
                {appStrings.settingsResetPreferencesText}
              </Text>
            </Pressable>
            <Text
              style={styles.settingsRestartNote}
              testID="settings-reset-preferences-note">
              {appStrings.settingsResetPreferencesNoteText}
            </Text>
            </ScrollView>
          </View>
        </View>
      ) : null}
    </View>
  );
}

/**
 * One provider-credential row (§3): the descriptor's labels, the masked
 * secure entry, Apply/Clear, the honest status line, and the opt-in
 * "Remember on this device" toggle (default off). A provider the SDK does not
 * register on React Native renders disabled-with-reason instead of pretending
 * to work.
 */
function ProviderCredentialRow(props: {
  descriptor: ProviderCredentialDescriptor;
  rowState: ProviderCredentialRowState;
  /** The hint line: the descriptor's own, or the base map's "enter a key" pointer. */
  hintText: string;
  styles: ReturnType<typeof buildScreenStyles>;
  onChangeCredential: (credential: string) => void;
  onToggleRemember: (isRememberEnabled: boolean) => void;
  onApply: () => void;
  onClear: () => void;
}) {
  const isRegistrationAvailable = isProviderCredentialRegistrationAvailable(
    props.descriptor,
  );
  const isApplyEnabled =
    isRegistrationAvailable && canApplyProviderCredential(props.rowState);
  return (
    <View
      style={props.styles.settingsProviderRow}
      testID={'provider-credential-row-' + props.descriptor.provider_key}>
      <Text style={props.styles.settingsProviderName}>
        {props.descriptor.display_name}
      </Text>
      <Text style={props.styles.settingsCredentialLabel}>
        {props.descriptor.credential_label}
      </Text>
      <TextInput
        style={props.styles.settingsCredentialInput}
        value={props.rowState.enteredCredential}
        onChangeText={props.onChangeCredential}
        secureTextEntry
        autoCapitalize="none"
        autoCorrect={false}
        placeholder={props.descriptor.credential_label}
        editable={isRegistrationAvailable}
        testID={'provider-credential-input-' + props.descriptor.provider_key}
      />
      <Text
        style={props.styles.settingsCredentialStatus}
        testID={'provider-credential-status-' + props.descriptor.provider_key}>
        {providerCredentialStatusText(props.descriptor.display_name, props.rowState)}
      </Text>
      <View style={props.styles.settingsRememberRow}>
        <Text style={props.styles.settingsRememberLabel}>
          {appStrings.providerCredentialRememberLabelText}
        </Text>
        <Switch
          value={props.rowState.isRememberEnabled}
          onValueChange={props.onToggleRemember}
          disabled={!isRegistrationAvailable}
          testID={'provider-credential-remember-' + props.descriptor.provider_key}
        />
      </View>
      <View style={props.styles.peekActionRow}>
        <Pressable
          style={[
            props.styles.actionControl,
            props.styles.actionControlPrimary,
            isApplyEnabled ? null : props.styles.actionControlDisabled,
          ]}
          onPress={props.onApply}
          disabled={!isApplyEnabled}
          testID={'provider-credential-apply-' + props.descriptor.provider_key}>
          <Text style={props.styles.actionControlText}>
            {appStrings.providerCredentialApplyButtonText}
          </Text>
        </Pressable>
        <Pressable
          style={props.styles.actionControl}
          onPress={props.onClear}
          disabled={!isRegistrationAvailable}
          testID={'provider-credential-clear-' + props.descriptor.provider_key}>
          <Text style={props.styles.actionControlText}>
            {appStrings.providerCredentialClearButtonText}
          </Text>
        </Pressable>
      </View>
      {isRegistrationAvailable ? (
        <>
          <Text style={props.styles.settingsCredentialHint}>
            {props.hintText}
          </Text>
          <Text style={props.styles.settingsCredentialAttribution}>
            {props.descriptor.attribution_text}
          </Text>
        </>
      ) : (
        <Text
          style={props.styles.settingsUnavailableText}
          testID={'provider-credential-unavailable-' + props.descriptor.provider_key}>
          {appStrings.providerCredentialUnavailableReasonText}
        </Text>
      )}
    </View>
  );
}

/**
 * One per-capability provider row (docs/design/ProviderCoverage.md §Per-capability selection):
 * the capability, the serving provider's name or the honest "Not configured"
 * line, and the registry's eligible providers as the picker. The two tile
 * rows are read-only because the map's layer control owns them.
 */
function CapabilityProviderRowView(props: {
  row: CapabilityProviderRow;
  styles: ReturnType<typeof buildScreenStyles>;
  onSelect: (providerKey: string) => void;
}) {
  const capabilityName = capabilityRegistryName(props.row.contract);
  const servingChoice = props.row.choices.find(
    choice => choice.providerKey === props.row.servingProviderKey,
  );
  return (
    <View
      style={props.styles.settingsProviderRow}
      testID={'capability-provider-' + capabilityName}>
      <Text style={props.styles.settingsProviderName}>
        {props.row.capabilityDisplayName}
      </Text>
      <Text
        style={props.styles.settingsCapabilityServing}
        testID={'capability-provider-serving-' + capabilityName}>
        {capabilityServingStatusText(props.row)}
      </Text>
      {servingChoice !== undefined && servingChoice.demoBaseUrl !== null ? (
        <>
          <Text
            style={props.styles.settingsKeylessLine}
            testID={'capability-provider-endpoint-' + capabilityName}>
            {appStrings.settingsRoutingDemoEndpointFormat.replace(
              '{url}',
              servingChoice.demoBaseUrl,
            )}
          </Text>
          {servingChoice.demoBaseUrlNote !== null ? (
            <Text
              style={props.styles.settingsUnavailableText}
              testID={'capability-provider-endpoint-note-' + capabilityName}>
              {servingChoice.demoBaseUrlNote}
            </Text>
          ) : null}
        </>
      ) : null}
      {props.row.isReadOnly ? (
        <Text
          style={props.styles.settingsKeylessLine}
          testID={'capability-provider-readonly-' + capabilityName}>
          {props.row.readOnlyNoteText}
        </Text>
      ) : (
        <View style={props.styles.chipRow}>
          {props.row.choices.map(choice => {
            const isServing = choice.providerKey === props.row.servingProviderKey;
            return (
              <Pressable
                key={choice.providerKey}
                style={[props.styles.chip, isServing ? props.styles.chipActive : null]}
                onPress={() => props.onSelect(choice.providerKey)}
                testID={
                  'capability-provider-choice-' +
                  capabilityName +
                  '-' +
                  choice.providerKey
                }>
                <Text
                  style={[
                    props.styles.chipText,
                    isServing ? props.styles.chipTextActive : null,
                  ]}>
                  {choice.rowLabel}
                </Text>
              </Pressable>
            );
          })}
        </View>
      )}
      {props.row.detailText !== null ? (
        <Text
          style={props.styles.settingsUnavailableText}
          testID={'capability-provider-detail-' + capabilityName}>
          {props.row.detailText}
        </Text>
      ) : null}
    </View>
  );
}

/**
 * The plan sheet's Directions control (PRD.md §6 item 36):
 * the Car/Walk mode choice, then the Fastest/Shortest strategy for car plans
 * only. A choice the serving routing provider cannot honour is disabled and
 * states its reason.
 */
function DirectionsControls(props: {
  control: DirectionsControlState;
  styles: ReturnType<typeof buildScreenStyles>;
  onSelectTransportationMode: (
    transportationModeName: DirectionsTransportationModeName,
  ) => void;
  onSelectRouteStrategy: (
    routeStrategyName: DirectionsRouteStrategyName,
  ) => void;
}) {
  const isCarSelected =
    props.control.selection.transportationMode === TransportationMode.Automobile;
  const isWalkSelected =
    props.control.selection.transportationMode === TransportationMode.Pedestrian;
  const isFastestSelected =
    props.control.selection.routeStrategy === RouteStrategy.Fastest;
  const isShortestSelected =
    props.control.selection.routeStrategy === RouteStrategy.Shortest;
  const modeReasonText = !props.control.car.isEnabled
    ? props.control.car.reasonText
    : !props.control.walk.isEnabled
      ? props.control.walk.reasonText
      : '';
  return (
    <View testID="directions-controls">
      <Text style={props.styles.sheetHeader}>
        {appStrings.directionsTitleText}
      </Text>
      <Text style={props.styles.searchResultsHeader}>
        {appStrings.directionsModeRowTitleText}
      </Text>
      <View style={props.styles.chipRow} testID="directions-mode-row">
        <Pressable
          style={[
            props.styles.chip,
            isCarSelected ? props.styles.chipActive : null,
            !props.control.car.isEnabled ? props.styles.actionControlDisabled : null,
          ]}
          disabled={!props.control.car.isEnabled}
          accessibilityState={{ selected: isCarSelected }}
          onPress={() => props.onSelectTransportationMode('automobile')}
          testID="directions-mode-car">
          <Text
            style={[
              props.styles.chipText,
              isCarSelected ? props.styles.chipTextActive : null,
            ]}>
            {appStrings.directionsModeCarText}
          </Text>
        </Pressable>
        <Pressable
          style={[
            props.styles.chip,
            isWalkSelected ? props.styles.chipActive : null,
            !props.control.walk.isEnabled ? props.styles.actionControlDisabled : null,
          ]}
          disabled={!props.control.walk.isEnabled}
          accessibilityState={{ selected: isWalkSelected }}
          onPress={() => props.onSelectTransportationMode('pedestrian')}
          testID="directions-mode-walk">
          <Text
            style={[
              props.styles.chipText,
              isWalkSelected ? props.styles.chipTextActive : null,
            ]}>
            {appStrings.directionsModeWalkText}
          </Text>
        </Pressable>
      </View>
      {modeReasonText !== '' ? (
        <Text
          style={props.styles.reasonStripText}
          testID="directions-mode-reason">
          {modeReasonText}
        </Text>
      ) : null}
      {props.control.isStrategyRowVisible ? (
        <>
          <Text style={props.styles.searchResultsHeader}>
            {appStrings.directionsStrategyRowTitleText}
          </Text>
          <View style={props.styles.chipRow} testID="directions-strategy-row">
            <Pressable
              style={[
                props.styles.chip,
                isFastestSelected ? props.styles.chipActive : null,
              ]}
              accessibilityState={{ selected: isFastestSelected }}
              onPress={() => props.onSelectRouteStrategy('fastest')}
              testID="directions-strategy-fastest">
              <Text
                style={[
                  props.styles.chipText,
                  isFastestSelected ? props.styles.chipTextActive : null,
                ]}>
                {appStrings.directionsStrategyFastestText}
              </Text>
            </Pressable>
            <Pressable
              style={[
                props.styles.chip,
                isShortestSelected ? props.styles.chipActive : null,
                !props.control.shortest.isEnabled
                  ? props.styles.actionControlDisabled
                  : null,
              ]}
              disabled={!props.control.shortest.isEnabled}
              accessibilityState={{ selected: isShortestSelected }}
              onPress={() => props.onSelectRouteStrategy('shortest')}
              testID="directions-strategy-shortest">
              <Text
                style={[
                  props.styles.chipText,
                  isShortestSelected ? props.styles.chipTextActive : null,
                ]}>
                {appStrings.directionsStrategyShortestText}
              </Text>
            </Pressable>
          </View>
          {!props.control.shortest.isEnabled &&
          props.control.shortest.reasonText !== modeReasonText ? (
            <Text
              style={props.styles.reasonStripText}
              testID="directions-strategy-reason">
              {props.control.shortest.reasonText}
            </Text>
          ) : null}
        </>
      ) : null}
    </View>
  );
}

/**
 * One labeled plan field (§4.1): its A/B marker and label, the input with its
 * inline Search action, the type-ahead list the field owns, and its My
 * location chip.
 */
function PlanFieldSearchControl(props: {
  planSearchField: PlanSearchField;
  marker: string;
  labelText: string;
  hintText: string;
  fieldText: string;
  searchFieldTestID: string;
  searchActionTestID: string;
  myLocationTestID: string;
  /** The reason the My location chip cannot start, or null while it works. */
  myLocationDisabledReason: string | null;
  /** Whether this field is the one the My location chip filled. */
  isMyLocationFilled: boolean;
  styles: ReturnType<typeof buildScreenStyles>;
  autocompleteProviderSelection: AutocompleteProviderSelection | null;
  autocompleteGateway: PlanAutocompleteGateway | null;
  onChangeText: (text: string) => void;
  onSearch: (planSearchField: PlanSearchField, queryText: string) => void;
  onFieldFocused: (planSearchField: PlanSearchField) => void;
  onFillMyLocation: (planSearchField: PlanSearchField) => void;
  onSelectSuggestion: (
    planSearchField: PlanSearchField,
    placeRow: PlaceRow,
  ) => void;
}) {
  // Only text the visitor typed asks a provider: a programmatic fill (a
  // result assignment, the My location chip, a swap) reaches the field
  // through the value prop and never through this handler.
  const userEditedQueryRef = useRef<string | null>(null);
  const autocomplete = usePlanAutocomplete({
    queryText: props.fieldText,
    userEditedQueryText: userEditedQueryRef.current,
    providerSelection: props.autocompleteProviderSelection,
    gateway: props.autocompleteGateway,
    onSelectSuggestion: placeRow =>
      props.onSelectSuggestion(props.planSearchField, placeRow),
  });
  return (
    <View style={props.styles.searchFieldGroup}>
      <View style={props.styles.searchFieldLabelRow}>
        <Text
          style={props.styles.searchFieldMarker}
          testID={`${props.planSearchField}-field-marker`}>
          {props.marker}
        </Text>
        <Text style={props.styles.searchFieldLabel}>{props.labelText}</Text>
      </View>
      <View style={props.styles.searchFieldControlRow}>
        <TextInput
          style={[
            props.styles.searchFieldInput,
            props.styles.searchFieldInputFlexible,
          ]}
          value={props.fieldText}
          onChangeText={text => {
            userEditedQueryRef.current = text;
            props.onChangeText(text);
          }}
          onFocus={() => props.onFieldFocused(props.planSearchField)}
          onSubmitEditing={() =>
            props.onSearch(props.planSearchField, props.fieldText)
          }
          placeholder={props.hintText}
          returnKeyType="search"
          testID={props.searchFieldTestID}
        />
        <Pressable
          style={props.styles.searchFieldAction}
          onPress={() => props.onSearch(props.planSearchField, props.fieldText)}
          testID={props.searchActionTestID}>
          <Text style={props.styles.searchFieldActionText}>
            {appStrings.planSearchActionText}
          </Text>
        </Pressable>
      </View>
      {autocomplete.status === 'failed' ? (
        <Text
          style={props.styles.searchResultsHeader}
          testID={`${props.planSearchField}-autocomplete-error`}>
          {autocomplete.errorText}
        </Text>
      ) : null}
      {autocomplete.status === 'noMatches' ? (
        <View
          style={props.styles.resultList}
          testID={`${props.planSearchField}-autocomplete-list`}>
          <View
            style={props.styles.resultRow}
            testID={`${props.planSearchField}-autocomplete-no-matches`}>
            <Text style={props.styles.resultRowSecondary}>
              {appStrings.planAutocompleteNoMatchesText}
            </Text>
          </View>
        </View>
      ) : null}
      {autocomplete.suggestions.length > 0 ? (
        <View
          style={props.styles.resultList}
          testID={`${props.planSearchField}-autocomplete-list`}>
          {autocomplete.suggestions.map((suggestion, suggestionIndex) => (
            <Pressable
              key={suggestion.structuredAddressText}
              style={props.styles.resultRow}
              accessibilityLabel={suggestion.structuredAddressText}
              onPress={() => autocomplete.selectSuggestion(suggestion)}
              testID={`${props.planSearchField}-autocomplete-row-${suggestionIndex}`}>
              <Text style={props.styles.resultRowName}>
                {suggestion.structuredAddressText}
              </Text>
            </Pressable>
          ))}
        </View>
      ) : null}
      <Pressable
        style={[
          props.styles.myLocationChip,
          props.isMyLocationFilled ? props.styles.myLocationChipFilled : null,
          props.myLocationDisabledReason !== null
            ? props.styles.myLocationChipDisabled
            : null,
        ]}
        disabled={props.myLocationDisabledReason !== null}
        accessibilityState={{
          disabled: props.myLocationDisabledReason !== null,
          selected: props.isMyLocationFilled,
        }}
        onPress={() => props.onFillMyLocation(props.planSearchField)}
        accessibilityLabel={`Use my location as the ${props.planSearchField}`}
        testID={props.myLocationTestID}>
        <Text style={props.styles.myLocationChipText}>
          {props.isMyLocationFilled
            ? `${appStrings.planMyLocationActionText} ${myLocationChipFilledMark}`
            : appStrings.planMyLocationActionText}
        </Text>
      </Pressable>
      {props.myLocationDisabledReason !== null ? (
        <Text
          style={props.styles.searchResultsHeader}
          testID={`${props.myLocationTestID}-reason`}>
          {props.myLocationDisabledReason}
        </Text>
      ) : null}
    </View>
  );
}

/**
 * The plan search view (§4.1 and docs/ReferenceAppUX.md):
 * two visibly labeled fields (A/B markers), each with its inline Search
 * action and My location chip, one swap action, the Directions choices, the
 * shared result rows with the assigned-row state, and Done back to the half
 * sheet.
 */
export function PlanSearchView(props: {
  originSearchText: string;
  destinationSearchText: string;
  searchRows: PlaceRow[];
  /** Whether the rows are the shared artifact's demo places (§4.1's keyless search). */
  searchRowsAreDemoPlaces: boolean;
  /** The keyless search's honest no-match line, null while a match stands. */
  demoSearchNoMatchText: string | null;
  searchInFlight: boolean;
  assignedOriginText: string | null;
  assignedDestinationText: string | null;
  planRouteReasonText: string;
  canPlanRoute: boolean;
  directionsControl: DirectionsControlState;
  styles: ReturnType<typeof buildScreenStyles>;
  /** The reason each My location chip cannot start, or null while it works. */
  myLocationDisabledReason: string | null;
  /** Which fields a My location chip filled this session. */
  myLocationFilledFields: { origin: boolean; destination: boolean };
  /** The credential state's type-ahead provider, or null when none is usable. */
  autocompleteProviderSelection: AutocompleteProviderSelection | null;
  /** The SDK seam the fields' type-ahead asks. */
  autocompleteGateway: PlanAutocompleteGateway | null;
  onOriginSearchTextChange: (text: string) => void;
  onDestinationSearchTextChange: (text: string) => void;
  onSearch: (planSearchField: PlanSearchField, queryText: string) => void;
  onSelectAutocompleteSuggestion: (
    planSearchField: PlanSearchField,
    placeRow: PlaceRow,
  ) => void;
  onAssign: (placeRow: PlaceRow) => void;
  onPlanRoute: () => void;
  onSelectDirectionsTransportationMode: (
    transportationModeName: DirectionsTransportationModeName,
  ) => void;
  onSelectDirectionsRouteStrategy: (
    routeStrategyName: DirectionsRouteStrategyName,
  ) => void;
  onFieldFocused: (planSearchField: PlanSearchField) => void;
  onFillMyLocation: (planSearchField: PlanSearchField) => void;
  onSwapFields: () => void;
  onDone: () => void;
}) {
  return (
    <>
      <Text style={props.styles.searchViewHeading} testID="search-heading">
        {appStrings.searchHeadingText}
      </Text>
      <PlanFieldSearchControl
        planSearchField="origin"
        marker={planOriginFieldMarker}
        labelText={appStrings.planOriginLabelText}
        hintText={appStrings.originFieldHintText}
        fieldText={props.originSearchText}
        onChangeText={props.onOriginSearchTextChange}
        searchFieldTestID="origin-search-field"
        searchActionTestID="origin-search-action"
        myLocationTestID="origin-my-location"
        myLocationDisabledReason={props.myLocationDisabledReason}
        isMyLocationFilled={props.myLocationFilledFields.origin}
        styles={props.styles}
        autocompleteProviderSelection={props.autocompleteProviderSelection}
        autocompleteGateway={props.autocompleteGateway}
        onSearch={props.onSearch}
        onFieldFocused={props.onFieldFocused}
        onFillMyLocation={props.onFillMyLocation}
        onSelectSuggestion={props.onSelectAutocompleteSuggestion}
      />
      <PlanFieldSearchControl
        planSearchField="destination"
        marker={planDestinationFieldMarker}
        labelText={appStrings.planDestinationLabelText}
        hintText={appStrings.destinationFieldHintText}
        fieldText={props.destinationSearchText}
        onChangeText={props.onDestinationSearchTextChange}
        searchFieldTestID="destination-search-field"
        searchActionTestID="destination-search-action"
        myLocationTestID="destination-my-location"
        myLocationDisabledReason={props.myLocationDisabledReason}
        isMyLocationFilled={props.myLocationFilledFields.destination}
        styles={props.styles}
        autocompleteProviderSelection={props.autocompleteProviderSelection}
        autocompleteGateway={props.autocompleteGateway}
        onSearch={props.onSearch}
        onFieldFocused={props.onFieldFocused}
        onFillMyLocation={props.onFillMyLocation}
        onSelectSuggestion={props.onSelectAutocompleteSuggestion}
      />
      <View style={props.styles.peekActionRow}>
        <Pressable
          style={props.styles.actionControl}
          onPress={props.onSwapFields}
          testID="swap-plan-fields">
          <Text style={props.styles.actionControlText}>
            {appStrings.planSwapActionText}
          </Text>
        </Pressable>
        <Pressable style={props.styles.actionControl} onPress={props.onDone} testID="search-done">
          <Text style={props.styles.actionControlText}>{appStrings.searchDoneText}</Text>
        </Pressable>
      </View>
      {props.planRouteReasonText !== '' ? (
        <Text style={props.styles.searchResultsHeader} testID="plan-route-reason">
          {props.planRouteReasonText}
        </Text>
      ) : null}
      <DirectionsControls
        control={props.directionsControl}
        styles={props.styles}
        onSelectTransportationMode={props.onSelectDirectionsTransportationMode}
        onSelectRouteStrategy={props.onSelectDirectionsRouteStrategy}
      />
      <Pressable
        style={[
          props.styles.actionControl,
          props.styles.actionControlPrimary,
          !props.canPlanRoute ? props.styles.actionControlDisabled : null,
        ]}
        disabled={!props.canPlanRoute}
        onPress={props.onPlanRoute}
        testID="plan-route">
        <Text style={[props.styles.actionControlText, props.styles.actionControlTextPrimary]}>
          {appStrings.planRouteButtonText}
        </Text>
      </Pressable>
      <Text style={props.styles.searchResultsHeader} testID="search-results-header">
        {props.searchInFlight
          ? appStrings.searchInFlightText
          : props.demoSearchNoMatchText !== null
            ? props.demoSearchNoMatchText
            : props.searchRows.length > 0
              ? props.searchRowsAreDemoPlaces
                ? appStrings.demoPlacesHeadingText
                : appStrings.searchResultsLabelText
              : props.destinationSearchText === '' && props.originSearchText === ''
                ? appStrings.searchHintText
                : ''}
      </Text>
      <View style={props.styles.resultList} testID="search-results-list">
        {props.searchRows.map(placeRow => {
          const assignedToOrigin = props.assignedOriginText === placeRow.structuredAddressText;
          const assignedToDestination =
            props.assignedDestinationText === placeRow.structuredAddressText;
          return (
            <Pressable
              key={placeRow.structuredAddressText}
              style={[
                props.styles.resultRow,
                assignedToOrigin || assignedToDestination ? props.styles.resultRowAssigned : null,
              ]}
              onPress={() => props.onAssign(placeRow)}
              testID={'search-result-' + placeRow.structuredAddressText}>
              <Text style={props.styles.resultRowName}>{placeRow.structuredAddressText}</Text>
              <Text style={props.styles.resultRowSecondary}>
                {(assignedToOrigin ? appStrings.planOriginLabelText + ' · ' : '') +
                  (assignedToDestination ? appStrings.planDestinationLabelText + ' · ' : '') +
                  (placeRow.noteText ?? placeRow.qualityText)}
              </Text>
            </Pressable>
          );
        })}
      </View>
    </>
  );
}

/** The route summary card (§4.2's distance·duration, Start and Edit). */
function RouteSummaryCard(props: {
  planState: PlanStateEvent;
  styles: ReturnType<typeof buildScreenStyles>;
  onStart: () => void;
  onEdit: () => void;
}) {
  return (
    <View style={props.styles.routeSummaryCard} testID="route-summary-card">
      {/* The live route card carries its facts, not a destination title
          (the iOS card shows no destination title); only the offline
          archive's own title names the test graph it came from. */}
      {props.planState.isOfflineArchiveRoute ? (
        <Text style={props.styles.routeSummaryTitle} testID="route-summary-title">
          {appStrings.offlineRouteTitleText}
        </Text>
      ) : null}
      <Text style={props.styles.routeSummaryMeta}>
        {props.planState.isInFlight
          ? appStrings.routeSummaryPlanningText
          : `${props.planState.distanceText ?? ''} · ${props.planState.durationText ?? ''}`}
      </Text>
      {props.planState.isRecordedDemoRoute ? (
        <Text style={props.styles.routeSummaryMeta} testID="route-summary-recorded-note">
          {appStrings.recordedDemoRouteNoteText}
        </Text>
      ) : null}
      {props.planState.isOfflineArchiveRoute ? (
        <Text style={props.styles.routeSummaryMeta} testID="route-summary-offline-note">
          {appStrings.offlineRouteNoteText}
        </Text>
      ) : null}
      <View style={props.styles.peekActionRow}>
        <Pressable
          style={[props.styles.actionControl, props.styles.actionControlPrimary]}
          onPress={props.onStart}
          testID="route-summary-start">
          <Text style={[props.styles.actionControlText, props.styles.actionControlTextPrimary]}>
            {appStrings.routeSummaryStartButtonText}
          </Text>
        </Pressable>
        <Pressable style={props.styles.actionControl} onPress={props.onEdit} testID="route-summary-edit">
          <Text style={props.styles.actionControlText}>{appStrings.routeSummaryEditButtonText}</Text>
        </Pressable>
      </View>
    </View>
  );
}

/** The in-flight region download's honest count line. */
function offlineRegionProgressText(progress: RegionDownloadProgress): string {
  return (
    `${progress.completedTileCount}/${progress.totalTileCount} · ` +
    `${progress.networkFetchedTileCount} network · ` +
    `${progress.diskServedTileCount} disk`
  );
}

/**
 * The offline maps card: the registered cache line, the live drawn-tile line,
 * the demo region download (progress, cancel and resume) and the stored
 * regions with their tile and byte counts and Delete controls.
 */
function OfflineMapsCard(props: {
  cacheState: CacheStateEvent | null;
  tileStatistics: FrameStatisticsEvent | null;
  isOfflineRouteInFlight: boolean;
  onPlanOfflineRoute: () => void;
  demoRegionNameText: string | null;
  demoRegionTileCount: number | null;
  isOfflineCacheRegistered: boolean;
  regionProgress: RegionDownloadProgress | null;
  regionStatus: 'idle' | 'downloading' | 'completed' | 'cancelled' | 'failed';
  regionErrorText: string | null;
  isRegionDownloadInFlight: boolean;
  storedRegions: RegionRecord[];
  isRegionDeleted: boolean;
  onDownloadRegion: () => void;
  onCancelRegionDownload: () => void;
  onDeleteRegion: (regionId: string) => void;
  styles: ReturnType<typeof buildScreenStyles>;
}) {
  const cacheLine = props.cacheState?.stateText ?? 'Cache not registered';
  const tileLine = props.tileStatistics?.live
    ? `Tiles requested ${props.tileStatistics.requestedTileCount ?? 0} · ` +
      `drawn ${props.tileStatistics.presentTileCount ?? 0} · ` +
      `fallback ${props.tileStatistics.fallbackTileCount ?? 0}`
    : appStrings.offlineTilesUnavailableText;
  return (
    <View style={props.styles.offlineCard} testID="offline-card">
      <Text style={props.styles.offlineCardTitle}>{appStrings.offlineMapsCardTitleText}</Text>
      <Text style={props.styles.offlineLine} testID="offline-cache-line">
        {cacheLine}
      </Text>
      <Text style={props.styles.offlineLine} testID="offline-tile-line">
        {tileLine}
      </Text>
      {props.demoRegionNameText !== null ? (
        <View style={props.styles.offlineStoredRegion} testID="offline-region">
          <Text style={props.styles.offlineRegionName}>
            {props.demoRegionNameText}
          </Text>
          {props.demoRegionTileCount !== null ? (
            <Text style={props.styles.offlineLine} testID="offline-region-tile-count">
              {`${props.demoRegionTileCount} ${appStrings.offlineRegionTileUnitText}`}
            </Text>
          ) : null}
          {props.regionStatus === 'downloading' ? (
            <Text style={props.styles.offlineLine} testID="offline-region-progress">
              {appStrings.offlineDownloadingStatusText}
              {props.regionProgress !== null
                ? ` ${offlineRegionProgressText(props.regionProgress)}`
                : ''}
            </Text>
          ) : null}
          {props.regionStatus === 'cancelled' &&
          props.regionProgress !== null ? (
            <Text style={props.styles.offlineLine} testID="offline-region-cancelled">
              {offlineRegionProgressText(props.regionProgress)}
            </Text>
          ) : null}
          {props.regionStatus === 'completed' ? (
            <Text style={props.styles.offlineLine} testID="offline-region-completed">
              {appStrings.offlineCompletedStatusText}
            </Text>
          ) : null}
          {props.regionStatus === 'failed' ? (
            <Text style={props.styles.offlineLine} testID="offline-region-failed">
              {appStrings.offlineFailedStatusText}
              {props.regionErrorText !== null ? `: ${props.regionErrorText}` : ''}
            </Text>
          ) : null}
          {props.isRegionDownloadInFlight ? (
            <Pressable
              style={props.styles.actionControl}
              onPress={props.onCancelRegionDownload}
              testID="offline-region-cancel">
              <Text style={props.styles.actionControlText}>
                {appStrings.offlineCancelActionText}
              </Text>
            </Pressable>
          ) : (
            <Pressable
              style={[
                props.styles.actionControl,
                props.isOfflineCacheRegistered
                  ? null
                  : props.styles.actionControlDisabled,
              ]}
              disabled={!props.isOfflineCacheRegistered}
              onPress={props.onDownloadRegion}
              testID="offline-region-download">
              <Text style={props.styles.actionControlText}>
                {appStrings.offlineDownloadActionText}
              </Text>
            </Pressable>
          )}
        </View>
      ) : null}
      {props.storedRegions.map(storedRegion => (
        <View
          key={storedRegion.regionId}
          style={props.styles.offlineStoredRegion}
          testID={`offline-stored-${storedRegion.regionId}`}>
          <Text style={props.styles.offlineRegionName}>
            {storedRegion.displayName}
          </Text>
          <Text style={props.styles.offlineLine}>
            {`${appStrings.offlineStoredTilesLabelText}: ` +
              `${storedRegion.presentTileCount}/${storedRegion.memberTileCount}`}
          </Text>
          <Text style={props.styles.offlineLine}>
            {`${appStrings.offlineStoredBytesLabelText}: ` +
              `${storedRegion.presentPayloadBytes} B`}
          </Text>
          <Pressable
            style={props.styles.actionControl}
            onPress={() => props.onDeleteRegion(storedRegion.regionId)}
            testID={`offline-delete-${storedRegion.regionId}`}>
            <Text style={props.styles.actionControlText}>
              {appStrings.offlineDeleteActionText}
            </Text>
          </Pressable>
        </View>
      ))}
      {props.isRegionDeleted ? (
        <Text style={props.styles.offlineLine} testID="offline-region-deleted">
          {appStrings.offlineDeletedStatusText}
        </Text>
      ) : null}
      <Pressable
        style={[
          props.styles.actionControl,
          props.isOfflineRouteInFlight ? props.styles.actionControlDisabled : null,
        ]}
        disabled={props.isOfflineRouteInFlight}
        onPress={props.onPlanOfflineRoute}
        testID="offline-route-button">
        <Text style={props.styles.actionControlText}>
          {appStrings.offlineRouteActionText}
        </Text>
      </Pressable>
    </View>
  );
}

/**
 * The drawer's seek control: one scrubber over the route's own length.
 * A touch or drag reports the fraction of the track it names, and the native
 * walker supplies the fix at that distance, so the camera, the indicator, the
 * ETA line and the arrival follow exactly as on a normal fix. The row stays
 * visible in the arrival state so a backwards seek can leave it.
 */
function NavigationSeekControl(props: {
  labelText: string;
  travelledMeters: number;
  totalMeters: number;
  onSeek: (travelledMeters: number) => void;
  styles: ReturnType<typeof buildScreenStyles>;
}) {
  const { onSeek, totalMeters } = props;
  const [trackWidth, setTrackWidth] = useState(0);
  const seekFromTrackPosition = useCallback(
    (trackPosition: number) => {
      if (trackWidth <= 0 || totalMeters <= 0) {
        return;
      }
      const travelledFraction = Math.min(
        Math.max(trackPosition / trackWidth, 0),
        1,
      );
      onSeek(travelledFraction * totalMeters);
    },
    [onSeek, totalMeters, trackWidth],
  );
  const panResponder = useMemo(
    () =>
      PanResponder.create({
        onStartShouldSetPanResponder: () => true,
        onMoveShouldSetPanResponder: () => true,
        onPanResponderGrant: event =>
          seekFromTrackPosition(event.nativeEvent.locationX),
        onPanResponderMove: event =>
          seekFromTrackPosition(event.nativeEvent.locationX),
      }),
    [seekFromTrackPosition],
  );
  const travelledFraction =
    props.totalMeters > 0
      ? Math.min(Math.max(props.travelledMeters / props.totalMeters, 0), 1)
      : 0;
  return (
    <View style={props.styles.seekRow} testID="navigation-seek-row">
      <Text style={props.styles.drawerSpeedLabel}>{props.labelText}</Text>
      <View
        style={props.styles.seekTrack}
        onLayout={layoutEvent =>
          setTrackWidth(layoutEvent.nativeEvent.layout.width)
        }
        accessibilityRole="adjustable"
        accessibilityLabel={props.labelText}
        accessibilityValue={{
          min: 0,
          max: props.totalMeters,
          now: props.travelledMeters,
        }}
        {...panResponder.panHandlers}
        testID="navigation-seek-track">
        <View
          style={[
            props.styles.seekFill,
            { width: `${travelledFraction * 100}%` },
          ]}
        />
      </View>
    </View>
  );
}

/**
 * The navigation drawer (§7.2): the ETA-distance line, phase, the
 * playback-speed row and one Pause/Resume control while the trip runs, the
 * speech toggle (§4.2's row — disabled-with-reason while the engine is
 * starting or failed), the honest last-spoken line the consumer carries, and
 * the exit. An arrival replaces the playback controls with the arrived line:
 * the destination stays presented until Exit, as on the other platforms.
 */
function NavigationDrawer(props: {
  traceUpdate: TraceUpdateEvent | null;
  playbackState: PlaybackStateEvent;
  /** A device-driven trip has no playback to steer (speed, pause, seek). */
  isDeviceNavigationActive: boolean;
  /** Ends a device-driven trip from the drawer the trip replaces the sheet
   * with, at the same control and label the half sheet carries. */
  onStopDeviceNavigation: () => void;
  safeAreaBottom: number;
  onExit: () => void;
  onTogglePause: () => void;
  onSelectSpeed: (speedMultiplier: number) => void;
  onSeek: (travelledMeters: number) => void;
  speechConsumerState: SpeechConsumerState;
  speechEngineState: SpeechEngineStateEvent;
  onToggleSpeech: (nextSpeechEnabled: boolean) => void;
  onMeasuredHeight: (height: number) => void;
  styles: ReturnType<typeof buildScreenStyles>;
}) {
  // The walker and the controller each declare the arrival; either one hides
  // the playback controls (the presentation outlives the playback).
  const hasArrived =
    props.traceUpdate?.isArrival === true || props.playbackState.hasArrived;
  const playbackControlText = props.playbackState.isPaused
    ? appStrings.navigationResumeActionText
    : appStrings.navigationPauseActionText;
  return (
    <View
      style={[props.styles.navigationDrawer, { bottom: props.safeAreaBottom }]}
      onLayout={layoutEvent =>
        props.onMeasuredHeight(layoutEvent.nativeEvent.layout.height)
      }
      testID="navigation-drawer">
      <Text style={props.styles.drawerPhaseLabel} testID="drawer-phase-line">
        {props.traceUpdate?.phase ?? appStrings.phaseNotStartedText}
      </Text>
      <Text style={props.styles.drawerEtaLabel} testID="drawer-eta-line">
        {appStrings.navigationDrawerEtaLabelText}: {props.traceUpdate?.progressLineText ?? ''}
      </Text>
      {hasArrived ? (
        <Text style={props.styles.arrivedText}>{appStrings.arrivedText}</Text>
      ) : props.isDeviceNavigationActive ? null : (
        <View style={props.styles.drawerControlRow} testID="navigation-speed-row">
          <Text style={props.styles.drawerSpeedLabel}>
            {appStrings.navigationSpeedLabelText}
          </Text>
          {simulatedTripSpeedMultiplierChoices.map(speedMultiplier => {
            const isSelected =
              props.playbackState.speedMultiplier === speedMultiplier;
            return (
              <Pressable
                key={speedMultiplier}
                style={[props.styles.chip, isSelected && props.styles.chipActive]}
                onPress={() => props.onSelectSpeed(speedMultiplier)}
                testID={'navigation-speed-chip-' + speedMultiplier}>
                <Text
                  style={[
                    props.styles.chipText,
                    isSelected && props.styles.chipTextActive,
                  ]}>
                  {appStrings.navigationSpeedMultiplierFormat.replace(
                    '{multiplier}',
                    String(speedMultiplier),
                  )}
                </Text>
              </Pressable>
            );
          })}
        </View>
      )}
      {props.isDeviceNavigationActive ? (
        <View style={props.styles.drawerControlRow} testID="drawer-device-navigation-row">
          <Pressable
            style={[props.styles.drawerExitControl, props.styles.chipActive]}
            onPress={props.onStopDeviceNavigation}
            testID="device-navigation-control">
            <Text style={props.styles.drawerExitControlText}>
              {deviceNavigationStopText}
            </Text>
          </Pressable>
        </View>
      ) : null}
      {props.playbackState.isSeekable ? (
        <NavigationSeekControl
          labelText={appStrings.navigationSeekLabelText}
          travelledMeters={props.traceUpdate?.travelledMeters ?? 0}
          totalMeters={props.playbackState.totalMeters}
          onSeek={props.onSeek}
          styles={props.styles}
        />
      ) : null}
      {props.speechEngineState.isEngineReady ? (
        <>
          <View style={props.styles.drawerControlRow} testID="speech-row">
            <Pressable
              style={[
                props.styles.chip,
                props.speechConsumerState.isSpeechEnabled && props.styles.chipActive,
              ]}
              onPress={() => props.onToggleSpeech(!props.speechConsumerState.isSpeechEnabled)}
              testID="speech-toggle">
              <Text
                style={[
                  props.styles.chipText,
                  props.speechConsumerState.isSpeechEnabled && props.styles.chipTextActive,
                ]}>
                {appStrings.speechToggleLabelText}
              </Text>
            </Pressable>
            {hasArrived || props.isDeviceNavigationActive ? null : (
              <Pressable
                style={props.styles.drawerExitControl}
                onPress={props.onTogglePause}
                testID="drawer-playback-pause">
                <Text style={props.styles.drawerExitControlText}>
                  {playbackControlText}
                </Text>
              </Pressable>
            )}
            <Pressable
              style={props.styles.drawerExitControl}
              onPress={props.onExit}
              testID="drawer-exit">
              <Text style={props.styles.drawerExitControlText}>
                {appStrings.navigationDrawerExitButtonText}
              </Text>
            </Pressable>
          </View>
          <Text style={props.styles.drawerSpeechLine} testID="drawer-speech-line">
            {appStrings.speechToggleLabelText}: {props.speechConsumerState.lastSpokenText ?? ''}
          </Text>
        </>
      ) : (
        <View style={props.styles.drawerControlRow} testID="speech-row">
          <Text style={props.styles.drawerSpeechReason} testID="drawer-speech-reason">
            {appStrings.speechToggleLabelText} {appStrings.speechToggleUnavailableLeadInText}{' '}
            {props.speechEngineState.unavailableReasonText ?? appStrings.speechEngineStartingText}
          </Text>
          {hasArrived ? null : (
            <Pressable
              style={props.styles.drawerExitControl}
              onPress={props.onTogglePause}
              testID="drawer-playback-pause">
              <Text style={props.styles.drawerExitControlText}>
                {playbackControlText}
              </Text>
            </Pressable>
          )}
          <Pressable
            style={props.styles.drawerExitControl}
            onPress={props.onExit}
            testID="drawer-exit">
            <Text style={props.styles.drawerExitControlText}>
              {appStrings.navigationDrawerExitButtonText}
            </Text>
          </Pressable>
        </View>
      )}
    </View>
  );
}

function TurnBannerStack(props: {
  traceActive: boolean;
  traceUpdate: TraceUpdateEvent | null;
  deviationText: string | null;
  safeAreaTop: number;
  styles: ReturnType<typeof buildScreenStyles>;
}) {
  if (!props.traceActive) {
    return null;
  }
  return (
    <View
      style={[props.styles.bannerStack, { top: props.safeAreaTop + bannerStackGap }]}
      pointerEvents="none"
      testID="turn-banner">
      {props.deviationText ? (
        <View style={[props.styles.bannerCard, props.styles.deviationCard]}>
          <Text style={props.styles.deviationText}>{props.deviationText}</Text>
        </View>
      ) : null}
      {props.traceUpdate?.isArrival ? (
        <View style={props.styles.bannerCard}>
          <Text style={props.styles.arrivedText}>{appStrings.arrivedText}</Text>
        </View>
      ) : (
        <>
          <View style={props.styles.bannerCard}>
            <Text style={props.styles.bannerHeadline}>
              {props.traceUpdate?.bannerHeadlineText ?? ''}
            </Text>
            <Text style={props.styles.bannerBody}>
              {props.traceUpdate?.bannerDisplayText ?? appStrings.awaitingGuidanceText}
            </Text>
          </View>
          {props.traceUpdate?.upcomingDisplayText ? (
            <View style={props.styles.bannerCard}>
              <Text style={props.styles.bannerUpcoming}>{props.traceUpdate.upcomingDisplayText}</Text>
            </View>
          ) : null}
        </>
      )}
    </View>
  );
}

/**
 * The map control cluster (§4.2): zoom −/+, my-location and the 2D/3D plane
 * toggle floating at the map's end edge, vertically centered like the Compose
 * reference application's `Alignment.CenterEnd` cluster. The zoom controls
 * repeat their step while held at the §2.4 cadence; a plain tap steps once.
 */
function MapControlCluster(props: {
  isThreeDCameraEnabled: boolean;
  isRecentering: boolean;
  onZoomIn: () => void;
  onZoomOut: () => void;
  onMyLocation: () => void;
  onTogglePlane: () => void;
  styles: ReturnType<typeof buildScreenStyles>;
}) {
  return (
    <View
      style={props.styles.mapControlCluster}
      pointerEvents="box-none"
      testID="map-control-cluster">
      <HoldRepeatControl
        label={appStrings.mapControlZoomInSymbol}
        accessibilityLabel={appStrings.mapControlZoomInAccessibilityText}
        onActivated={props.onZoomIn}
        styles={props.styles}
        testID="map-control-zoom-in"
      />
      <HoldRepeatControl
        label={appStrings.mapControlZoomOutSymbol}
        accessibilityLabel={appStrings.mapControlZoomOutAccessibilityText}
        onActivated={props.onZoomOut}
        styles={props.styles}
        testID="map-control-zoom-out"
      />
      <Pressable
        style={[
          props.styles.mapControlButton,
          props.isRecentering && props.styles.mapControlButtonActive,
        ]}
        onPress={props.onMyLocation}
        accessibilityRole="button"
        accessibilityLabel={
          props.isRecentering
            ? appStrings.mapControlRecenterAccessibilityText
            : appStrings.mapControlMyLocationAccessibilityText
        }
        testID="map-control-my-location">
        <View
          style={[
            props.styles.myLocationGlyphRing,
            props.isRecentering && props.styles.myLocationGlyphRingActive,
          ]}>
          <View
            style={[
              props.styles.myLocationGlyphDot,
              props.isRecentering && props.styles.myLocationGlyphDotActive,
            ]}
          />
        </View>
      </Pressable>
      <Pressable
        style={props.styles.mapControlButton}
        onPress={props.onTogglePlane}
        accessibilityRole="button"
        accessibilityLabel={appStrings.mapControlPlaneAccessibilityText}
        testID="map-control-plane">
        <Text
          style={[
            props.styles.mapControlButtonText,
            props.isThreeDCameraEnabled && props.styles.mapControlButtonTextActive,
          ]}>
          {props.isThreeDCameraEnabled
            ? appStrings.mapControlThreeDLabelText
            : appStrings.mapControlTwoDLabelText}
        </Text>
      </Pressable>
    </View>
  );
}

/**
 * One hold-to-repeat control (the zoom buttons): the activation fires on the
 * press and repeats at the §2.4 cadence until release, so a hold never
 * double-steps on the release itself.
 */
function HoldRepeatControl(props: {
  label: string;
  accessibilityLabel: string;
  onActivated: () => void;
  styles: ReturnType<typeof buildScreenStyles>;
  testID: string;
}) {
  const repeatTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const stopRepeating = useCallback(() => {
    if (repeatTimerRef.current !== null) {
      clearInterval(repeatTimerRef.current);
      repeatTimerRef.current = null;
    }
  }, []);
  useEffect(() => stopRepeating, [stopRepeating]);
  const startRepeating = useCallback(() => {
    props.onActivated();
    stopRepeating();
    repeatTimerRef.current = setInterval(
      props.onActivated,
      mapControlZoomRepeatIntervalMilliseconds,
    );
  }, [props.onActivated, stopRepeating]);
  return (
    <Pressable
      style={props.styles.mapControlButton}
      onPressIn={startRepeating}
      onPressOut={stopRepeating}
      accessibilityRole="button"
      accessibilityLabel={props.accessibilityLabel}
      testID={props.testID}>
      <Text style={props.styles.mapControlButtonText}>{props.label}</Text>
    </Pressable>
  );
}

/**
 * The compass control (§4.2): top-end. During a trip it is a mode toggle
 * (M73): a tap chooses north-up and the control stays visible while that mode
 * holds, so a second tap can resume course-up. Outside a trip it appears only
 * while the camera is off north by the §2.4 threshold.
 */
function CompassControl(props: {
  bearingDegrees: number;
  traceActive: boolean;
  isHoldingNorthUp: boolean;
  safeAreaTop: number;
  onPress: () => void;
  styles: ReturnType<typeof buildScreenStyles>;
}) {
  if (
    !props.isHoldingNorthUp &&
    bearingOffNorthDegrees(props.bearingDegrees) <
      compassVisibilityBearingThresholdDegrees
  ) {
    return null;
  }
  const topClearance = props.traceActive
    ? mapControlLayout.compassNavigationTopClearance
    : themeSpacing.stackGroup;
  return (
    <Pressable
      style={[props.styles.compassButton, { top: props.safeAreaTop + topClearance }]}
      onPress={props.onPress}
      accessibilityRole="button"
      accessibilityLabel={appStrings.mapControlCompassAccessibilityText}
      testID="compass-control">
      <Text
        style={[
          props.styles.compassLetter,
          { transform: [{ rotate: `${-props.bearingDegrees}deg` }] },
        ]}>
        {appStrings.mapControlCompassNorthLetter}
      </Text>
    </Pressable>
  );
}

/**
 * The scale bar (§4.2): bottom-start, built on the camera's
 * ground-meters-per-screen-pixel answer with the §2.4 width range and the
 * 1-2-3-5-10 distance ladder; no answer renders no bar.
 */
function ScaleBarOverlay(props: {
  groundMetersPerScreenPixel: number | undefined;
  styles: ReturnType<typeof buildScreenStyles>;
}) {
  if (props.groundMetersPerScreenPixel === undefined) {
    return null;
  }
  const selectedScale = selectScaleBarScale(props.groundMetersPerScreenPixel);
  if (selectedScale === null) {
    return null;
  }
  // The selected width is in physical viewport pixels; the drawing unit is
  // the density-independent point (the Compose scale bar's same conversion).
  const barWidthPoints = selectedScale.widthPixels / PixelRatio.get();
  return (
    <View style={props.styles.scaleBar} testID="scale-bar">
      <Text style={props.styles.scaleBarText}>
        {distanceLabelText(selectedScale.distanceMeters)}
      </Text>
      <View style={[props.styles.scaleBarLine, { width: barWidthPoints }]} />
    </View>
  );
}

function AttributionInfoControl(props: {
  isPanelOpen: boolean;
  onTogglePanel: () => void;
  onClosePanel: () => void;
  /** The serving base map's attribution line (empty claims nothing). */
  attributionText: string;
  /** The serving base map's terms note (empty renders no second block). */
  attributionTermsNotes: string[];
  styles: ReturnType<typeof buildScreenStyles>;
}) {
  return (
    <>
      <Pressable
        style={[
          props.styles.attributionInfoControl,
          props.isPanelOpen && props.styles.attributionInfoControlActive,
        ]}
        onPress={props.onTogglePanel}
        accessibilityRole="button"
        accessibilityLabel={appStrings.attributionInfoAccessibilityText}
        accessibilityState={{ expanded: props.isPanelOpen }}
        testID="attribution-info-control">
        <Text style={props.styles.attributionInfoGlyph}>
          {appStrings.attributionInfoSymbol}
        </Text>
      </Pressable>
      <Modal
        visible={props.isPanelOpen}
        transparent
        animationType="fade"
        onRequestClose={props.onClosePanel}>
        <Pressable
          style={props.styles.attributionPanelBackdrop}
          onPress={props.onClosePanel}
          testID="attribution-panel-backdrop">
          <Pressable
            style={props.styles.attributionPanel}
            onPress={() => {}}
            testID="attribution-panel">
            <Text style={props.styles.attributionPanelTitle}>
              {appStrings.attributionPanelTitleText}
            </Text>
            <Text style={props.styles.attribution} selectable testID="attribution">
              {props.attributionText}
            </Text>
            {props.attributionTermsNotes.length > 0 ? (
              <Text style={props.styles.attributionTermsNote} selectable>
                {props.attributionTermsNotes.join('\n\n')}
              </Text>
            ) : null}
          </Pressable>
        </Pressable>
      </Modal>
    </>
  );
}
