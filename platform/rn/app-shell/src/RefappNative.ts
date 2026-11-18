/**
 * The reference application's native seam from JavaScript:
 *
 * - `NativeModules.RefappMap` (the app shell's demo bridge module) drives the
 *   demo-only surface: camera presets, the guided-walk replay, the secure
 *   credential store, the speech engine and the diagnostics lines. The map
 *   surface, camera gestures and the one native `MapSdk` belong to the
 *   package (`MapsSdk`, `MapView`).
 * - One event channel (`DeviceEventEmitter`) carries the controller's
 *   readiness, error, statistics, camera, and guided-walk events.
 *
 * Every access is guarded: the shell renders its honest failure states rather
 * than crashing when a piece of the seam is unavailable (a plain-test run, a
 * missing TurboModule, a denied event emitter).
 */

import {
  NativeModules,
  DeviceEventEmitter,
  NativeEventEmitter,
  Platform,
} from 'react-native';

/** One provider search result row (the promise-resolved plan-search shape). */
export interface PlaceRow {
  structuredAddressText: string;
  latitudeDegrees: number;
  longitudeDegrees: number;
  qualityText: string;
  /** The artifact demo-place note (keyless demo rows only). */
  noteText?: string;
  /** Whether the row is the shared artifact's demo place, never a live result. */
  isDemoPlace?: boolean;
}

/** The `RefappMap` bridge module's JavaScript-side API. */
export interface RefappMapModule {
  createSdk(): void;
  applyCameraPreset(
    latitudeDegrees: number,
    longitudeDegrees: number,
    zoomLevel: number,
    bearingDegrees: number,
    pitchDegrees: number,
  ): void;
  /** One zoom control step (§4.2's cluster; the screen repeats on hold). */
  zoomInControl(): void;
  zoomOutControl(): void;
  /** The my-location control (§4.2): centers on the latest fix. */
  myLocationControl(): void;
  /** The 2D/3D plane toggle (§4.2). */
  cameraPlaneControl(): void;
  /** The compass tap (§2.4): eases the camera north up. */
  compassControl(): void;
  /** The plan search (§4.1): resolves the provider result rows. */
  searchPlaces(queryText: string): Promise<PlaceRow[]>;
  /** One result row's plan-field assignment; resolves once assigned. The
   * assignment never plans by itself (§4.1). */
  assignSearchResult(
    assignToOrigin: boolean,
    placeNameText: string,
    latitudeDegrees: number,
    longitudeDegrees: number,
  ): Promise<boolean>;
  /** The explicit Plan route action (§4.1): plans between the two assigned
   * plan fields and resolves `true` once the plan settled; a missing field
   * rejects with a reason naming it. The two generated enum ordinals are the
   * Directions choice every live plan carries
   * (docs/design/DirectionsModesAndStrategy.md). */
  planRoute(
    transportationMode: number,
    routeStrategy: number,
  ): Promise<boolean>;
  /**
   * The keyless plan (§4.1): applies the shared artifact's recorded route for
   * the ordered demo-place pair and resolves `true` once the plan settled.
   * No provider call is involved; a pair the artifact does not record
   * rejects with a reason naming both places.
   */
  planRecordedDemoRoute(
    originName: string,
    destinationName: string,
  ): Promise<boolean>;
  /**
   * The offline card's route control (§2.5 `offline.plan_route_action`):
   * plans over the committed compiled routing archive the app bundle carries
   * and resolves `true` once the plan settled. No provider, credential or
   * network round trip is involved; a refused archive rejects with its
   * verbatim detail and publishes the same report on the error channel.
   */
  planOfflineRouteFromArchive(): Promise<boolean>;
  /**
   * Whether the running SDK session has a provider credential applied. The
   * shell reads it once at mount so a launch-environment injected key is
   * honored: a keyless session then offers the artifact's demo places and
   * the keyed session keeps using the provider.
   */
  isProviderCredentialApplied(): Promise<boolean>;
  /** One demo-scene chip's selection over the SDK overlay API (§4.1). */
  setSceneOverlayToggle(sceneKey: string, isSelected: boolean): void;
  /** The scene chips' Clear control. */
  clearSceneOverlays(): void;
  /**
   * One demo-plan pick (§4.1): the recorded walk the guided-walk demo replays;
   * resolves when the plan applied (the planned route dropped, the framing set).
   */
  selectTraceReplayPlan(traceAssetName: string): Promise<boolean>;
  startTraceReplay(): void;
  /**
   * Starts guidance on the planned route from the device's own fixes and
   * resolves whether a planned route let it start. A false answer is the
   * plan-first case: the shell states what the control needs and starts
   * nothing. The location source itself belongs to the shell, which starts it
   * through the same permission and source path the location control uses.
   */
  startDeviceNavigation(): Promise<boolean>;
  /** Ends device-driven guidance; the shell stops the source it started. */
  stopDeviceNavigation(): void;
  /**
   * One accepted device fix into the device-driven trip's navigator: the fix
   * coordinate, its horizontal accuracy (null when the source reports none)
   * and the controller's smoothed course (null when no fix carried one).
   */
  deliverDeviceNavigationFix(
    latitudeDegrees: number,
    longitudeDegrees: number,
    courseHeadingDegrees: number | null,
    horizontalAccuracyMeters: number | null,
  ): void;
  exitTraceReplay(): void;
  /** The drawer's pause act (§7.2): the playback holds its current fix. */
  pauseTraceReplay(): void;
  /** The drawer's resume act: the playback continues from its current fix. */
  resumeTraceReplay(): void;
  /**
   * The drawer's seek act: moves the trip to a travelled distance along
   * the route through the walker, so the camera, the indicator, the ETA line
   * and the arrival follow exactly as on a normal fix.
   */
  seekTraceReplay(travelledMeters: number): void;
  /**
   * One playback-speed choice. A route-following trip rebuilds its walker at
   * the new pace without moving the travelled position; a recorded trace's
   * replay cadence follows the choice.
   */
  selectTraceReplaySpeedMultiplier(speedMultiplier: number): void;
  /** One verbatim guidance line into the app-owned speech engine (§4.1/§6.3). */
  speakUtterance(speechText: string, engineUtteranceId: string): void;
  /** The speech consumer's toggle-off path: whatever speaks, stops. */
  stopSpeech(): void;
  /** The speech engine's initialization state, read back at the mount (§6.2). */
  getSpeechEngineState(): Promise<SpeechEngineStateEvent>;
  /**
   * Registers one runtime provider credential through the SDK's existing
   * registration call (docs/ProviderCredentialsAndBrowserProviders.md §3):
   * a refusal's detail text is verbatim, and the controller re-emits its
   * readiness and cache state. The credential never travels back to JS.
   */
  setProviderCredential(
    providerKey: string,
    credential: string,
  ): Promise<ProviderCredentialApplyResult>;
  /** The opt-in secure store's value, or null when nothing is stored. */
  getStoredProviderCredential(providerKey: string): Promise<string | null>;
  /** Persists the credential behind the platform secure store; false on failure. */
  storeProviderCredential(providerKey: string, credential: string): Promise<boolean>;
  /** Removes the stored credential; false when the secure store is unavailable. */
  clearStoredProviderCredential(providerKey: string): Promise<boolean>;
  retrySurfaceAttach(): void;
  getDiagnosticsLines(): Promise<string[]>;
  /**
   * Registers one diagnostics listener at a minimum severity and flushes the
   * bounded backlog. It is called only when the diagnostics pane opens (and
   * again when the explicit debug switch lowers the gate); no listener exists
   * before that.
   */
  startDiagnosticEvents(
    minimumSeverityOrdinal: number,
    maximumBacklogCount: number,
  ): void;
  /** Removes the diagnostics listener when the pane closes. */
  stopDiagnosticEvents(): void;
  /**
   * The theme choice this device remembered (§3), or null when it never
   * chose. A platform without a store answers null.
   */
  loadThemeChoice(): Promise<string | null>;
  /** Remembers one theme choice on this device (§3). */
  storeThemeChoice(themeChoice: string): void;
  /**
   * The device's versioned preferences record as a JSON string, or null when
   * this device never chose. The record carries only explicit choices; a
   * provider credential is never part of it.
   */
  loadPreferences(): Promise<string | null>;
  /** Remembers the versioned preferences record on this device. */
  storePreferences(preferencesJson: string): void;
  /** Clears the preferences record and any theme choice an earlier build stored. */
  clearPreferences(): void;
}

/** The runtime provider registration's honest outcome (§3's explicit Apply). */
export interface ProviderCredentialApplyResult {
  isApplied: boolean;
  /** The verbatim refusal detail (empty when the credential applied). */
  detailText: string;
}

/** The bridge module (null when the native side is absent, e.g. in jest). */
export const refappMapModule: RefappMapModule | null =
  (NativeModules as Record<string, RefappMapModule | undefined>).RefappMap ?? null;

/** The native module's event-listener export shape the iOS emitter needs. */
interface NativeEventEmitterModule {
  addListener(eventName: string): void;
  removeListeners(count: number): void;
}

/**
 * One named event of the controller's single event channel.
 *
 * The iOS RCTEventEmitter (React Native 0.88, bridgeless) emits an event only
 * when at least one JS listener was registered on the *module itself* — the
 * base class's `addListener` RCT export counts the listeners, so the
 * subscription must go through `NativeEventEmitter` on this platform. The
 * Android device-event path delivers to `DeviceEventEmitter` unconditionally
 * (its `DeviceEventManagerModule.emitDeviceEvent` has no listener gate), so
 * the same subscription stays there (the module carries no `addListener`
 * export on Android — `NativeEventEmitter` would throw).
 */
export function addControllerListener(
  eventName: string,
  handler: (arguments_5: Record<string, unknown>) => void,
): () => void {
  if (Platform.OS === 'ios' && refappMapModule !== null) {
    const moduleEmitter = new NativeEventEmitter(
      refappMapModule as unknown as NativeEventEmitterModule,
    );
    const subscription = moduleEmitter.addListener(
      eventName,
      handler as (event: unknown) => void,
    );
    return () => subscription.remove();
  }
  const subscription = DeviceEventEmitter.addListener(eventName, handler);
  // Remove only this subscription: `removeAllListeners` would detach parallel
  // listeners on the same event (the audit's D11 trap).
  return () => subscription.remove();
}

/** Whether the native side of the shell is reachable (drives honest states). */
export function hasNativeController(): boolean {
  return refappMapModule !== null;
}

export interface ReadinessEvent {
  state: string;
  text: string;
  detail?: string | null;
}

export interface ErrorReportEvent {
  variantLabel: string;
  detailText: string;
}

/** The control cluster's read-only camera state (§4.2's cluster and scale bar). */
export interface CameraReadbackEvent {
  /** The diagnostics readback line (the native-formatted camera summary). */
  text: string;
  zoomLevel: number;
  bearingDegrees: number;
  pitchDegrees: number;
  isThreeDCameraEnabled: boolean;
  hasLocationFix: boolean;
  /** Whether the trip follow camera is engaged (false outside a trip). */
  isFollowing?: boolean;
  /** Present only when the camera answered a valid ground scale (§4.2). */
  groundMetersPerScreenPixel?: number;
}

export interface FrameStatisticsEvent {
  live: boolean;
  requestedTileCount?: number;
  presentTileCount?: number;
  fallbackTileCount?: number;
  deferredTileCount?: number;
  unavailableTileCount?: number;
  overlayTriangleCount?: number;
  framesDrawnCount: number;
  framesDeferredCount: number;
  framesFailedCount: number;
}

export interface TraceUpdateEvent {
  phase: string;
  isArrival: boolean;
  isReroutingRequired: boolean;
  bannerHeadlineText: string;
  bannerDisplayText: string;
  upcomingDisplayText: string | null;
  progressLineText: string | null;
  /** The trip's distance along the route, in meters (the seek control's value). */
  travelledMeters: number;
}

export interface TraceStateEvent {
  isActive: boolean;
  traceName: string;
}

/**
 * Whether guidance follows the device's own fixes on the planned route. The
 * event lets the shell reflect a trip the native side ended by itself (a
 * refused fix), so the control never claims a guidance session that stopped.
 */
export interface DeviceNavigationStateEvent {
  isActive: boolean;
}

/**
 * The drawer's playback state (§7.2): whether a presented trip is paused, its
 * selected speed multiplier and whether it has arrived. Delivered separately
 * from `TraceStateEvent` so a pause or a pace change never drops the last
 * guidance update (the trace-state listener clears it).
 */
export interface PlaybackStateEvent {
  isActive: boolean;
  isPaused: boolean;
  speedMultiplier: number;
  hasArrived: boolean;
  /** Whether the presented trip walks the route's geometry (the seek control). */
  isSeekable: boolean;
  /** The route's total length, in meters (the seek control's range). */
  totalMeters: number;
}

/** The plan flow's state event (§4.1's route summary card). */
export interface PlanStateEvent {
  isPlanned: boolean;
  isInFlight: boolean;
  titleText: string;
  distanceText?: string;
  durationText?: string;
  /** Whether the planned route is the shared artifact's recorded demo route. */
  isRecordedDemoRoute?: boolean;
  /** Whether the planned route came from the bundled offline test graph. */
  isOfflineArchiveRoute?: boolean;
}

/** The offline card's cache line (§4.1's two honest lines, first line). */
export interface CacheStateEvent {
  isRegistered: boolean;
  stateText: string;
}

/** One speech topic delivery crossing to the consumer verbatim. */
export interface SpeechInstructionEvent {
  speechText: string;
  /** The contract's monotonic `utterance_id`, as its decimal string. */
  speechInstructionId: string;
  /** What triggered the line (the lowercase enum label). */
  instructionKind: string;
}

/** The speech engine's honest ready/unavailable state (§6.2). */
export interface SpeechEngineStateEvent {
  isEngineReady: boolean;
  unavailableReasonText?: string;
}