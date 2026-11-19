/**
 * The typed React Native `MapView` component shipped with the package.
 *
 * The native half (Android: the AAR's lifecycle-aware `MapView` widget, iOS:
 * the `MapNativeCoreUI` view compiled into the pod) owns the surface, the
 * frame pump, gestures, the camera value store and animation, the overlay and
 * location-layer bookkeeping and the listener. This component exposes exactly
 * that native shape to JavaScript:
 *
 * - props: the four safe-area inset props (physical pixels), gesture and
 *   render-pause switches, and one callback per native listener method;
 * - imperative calls on the forwarded ref: the camera verbs, the pan/zoom
 *   gesture members, the id-addressed overlay calls, the location layer, the
 *   attach retry and the readbacks;
 * - the callbacks the native listener delivers: map ready, camera changed,
 *   map click, failure, frame statistics, deferred and settled frames.
 *
 * The SDK object is owned by the package's native module (see `MapsSdk.ts`);
 * it is deliberately not a prop, because the generated JSI bindings cannot
 * import a foreign handle and a second `MapSdk` would open a second tile
 * cache. Initialize the package SDK once (`await MapsSdk.create()`) before
 * mounting the view; the view attaches as soon as the SDK is available.
 */

import React, {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
} from 'react';
import {
  DeviceEventEmitter,
  NativeEventEmitter,
  NativeModules,
  Platform,
  requireNativeComponent,
} from 'react-native';
import type { StyleProp, ViewStyle } from 'react-native';

import type {
  BoundingBox,
  EdgeInsets,
  FrameStatistics,
  GeographicCoordinate,
  LocationIndicatorState,
  LocationIndicatorStyle,
  MapOverlay,
  ProviderVectorTileSource,
  ViewportSize,
} from './generated/rustic_maps';
import type {
  UrlTemplateVectorTileSource,
  VectorTileSourceStatus,
} from './MapsSdk';
import { MapsSdkError, wrapNativeFailures } from './MapsSdkError';

// The record types the component's calls speak are re-exported beside it, so
// a consumer imports one module for the component and its payload shapes.
export type {
  BoundingBox,
  EdgeInsets,
  FrameStatistics,
  GeographicCoordinate,
  LocationIndicatorState,
  LocationIndicatorStyle,
  MapOverlay,
  MapOverlayKind,
  LocationIndicatorVisualKind,
  OverlayColor,
  ProviderVectorTileSource,
  ViewportSize,
} from './generated/rustic_maps';

/** The camera value the native `MapView` adopts and reads back. */
export interface MapCamera {
  /** Camera center latitude in decimal degrees. */
  latitudeDegrees: number;
  /** Camera center longitude in decimal degrees. */
  longitudeDegrees: number;
  /** Zoom level. */
  zoomLevel: number;
  /** Bearing in degrees clockwise from true north. */
  bearingDegrees: number;
  /** Pitch in degrees; zero is top-down. */
  pitchDegrees: number;
}

/** Which native `MapView` operation produced a `MapViewFailure`. */
export type MapViewFailurePhase =
  | 'surface'
  | 'resize'
  | 'draw'
  | 'camera'
  | 'overlay'
  | 'locationIndicator'
  | 'baseMap'
  | 'tap'
  | 'listener';

/** One refused native `MapView` operation, carrying its verbatim failure. */
export interface MapViewFailure {
  /** The wrapper operation that failed. */
  phase: MapViewFailurePhase;
  /** The verbatim SDK or platform failure message. */
  message: string;
}

/**
 * The callbacks the native `MapView` listener delivers on the main thread;
 * every one is optional, exactly like the native listener's empty defaults.
 */
export interface MapViewProps {
  /** Layout style; the native view fills its parent by default. */
  style?: StyleProp<ViewStyle>;
  /** Test identifier forwarded to the native view. */
  testID?: string;
  /** Safe-area inset on the top edge, in physical pixels. */
  topBarInsetPixels?: number;
  /** Safe-area inset on the bottom edge, in physical pixels. */
  bottomBarInsetPixels?: number;
  /** Safe-area inset on the left edge, in physical pixels. */
  leftBarInsetPixels?: number;
  /** Safe-area inset on the right edge, in physical pixels. */
  rightBarInsetPixels?: number;
  /** Whether touch gestures pan, pinch and rotate the camera (default true). */
  isGestureEnabled?: boolean;
  /** Pauses or resumes drawing explicitly (default false). */
  isRenderingPaused?: boolean;
  /**
   * The first frame drew and presented successfully. A subscriber that
   * attaches after the native event still receives it once through the
   * component's readiness readback.
   */
  onMapReady?: () => void;
  /** The adopted camera changed: a set, an animation step, a gesture or a fit. */
  onCameraChanged?: (camera: MapCamera, isGestureDriven: boolean) => void;
  /** A tap unprojected onto the ground plane; a miss reports nothing. */
  onMapClick?: (coordinate: GeographicCoordinate) => void;
  /** An SDK or platform operation failed; the failure is never swallowed. */
  onError?: (failure: MapViewFailure) => void;
  /** One presented frame's tile and overlay accounting. */
  onFrameStatistics?: (statistics: FrameStatistics) => void;
  /** A frame was skipped by the serialized-draw backpressure; the next retries. */
  onFrameDeferred?: () => void;
  /** The pump paused because the presented frame is settled. */
  onFrameSettled?: () => void;
}

/** The imperative calls the component forwards to the native view. */
export interface MapViewHandle {
  /** Sets the camera immediately; a refused pose reports through `onError`. */
  setCamera(camera: MapCamera): void;
  /** Eases the camera to the pose over `durationMilliseconds`. */
  animateCamera(camera: MapCamera, durationMilliseconds?: number): void;
  /** Frames the bounds inside the viewport's inset region. */
  fitBounds(
    boundingBox: BoundingBox,
    edgeInsets?: EdgeInsets,
    maximumZoomLevel?: number,
  ): void;
  /** Pans by a screen-pixel delta through the contract camera member. */
  panByScreenPixels(
    deltaXPixels: number,
    deltaYPixels: number,
    isGestureDriven?: boolean,
  ): void;
  /** Zooms by levels around the viewport center through the contract member. */
  zoomByLevels(deltaZoomLevels: number): void;
  /**
   * Rotates the content clockwise by `clockwiseDegrees` around the viewport
   * center (the content-follows-fingers convention, the same sign every
   * native view uses).
   */
  rotateByDegrees(clockwiseDegrees: number): void;
  /**
   * Whether the native view has drawn and presented its first frame. It
   * forwards the same readiness the `mapReady` event reports.
   */
  hasDrawnFirstFrame(): Promise<boolean>;
  /** The render viewport width in pixels (zero before attach). */
  viewportWidthPixels(): Promise<number>;
  /** The render viewport height in pixels (zero before attach). */
  viewportHeightPixels(): Promise<number>;
  /**
   * The capped physical-pixel extent the surface is configured to, or null
   * before the first frame.
   */
  viewportPixelExtent(): Promise<ViewportSize | null>;
  /** The display's device pixels per dp (the SDK's drawing-buffer scale). */
  displayScale(): Promise<number>;
  /** Adds or replaces the overlay held under `overlayIdentifier`. */
  addOverlay(overlayIdentifier: string, overlay: MapOverlay): Promise<boolean>;
  /** Replaces the overlay held under `overlayIdentifier`. */
  updateOverlay(overlayIdentifier: string, overlay: MapOverlay): Promise<boolean>;
  /** Removes the overlay held under `overlayIdentifier`. */
  removeOverlay(overlayIdentifier: string): Promise<boolean>;
  /** Replaces the whole overlay list (resets the identifier bookkeeping). */
  setOverlays(overlays: ReadonlyArray<MapOverlay>): Promise<boolean>;
  /** Clears every overlay and the identifier bookkeeping. */
  clearOverlays(): Promise<boolean>;
  /** Sets the renderer-owned location layer. */
  setLocationIndicator(
    state: LocationIndicatorState,
    style: LocationIndicatorStyle,
  ): void;
  /** Clears the location layer. */
  clearLocationIndicator(): void;
  /**
   * Installs a styled vector base map: a provider-neutral vector
   * tileset painted with one of the SDK's committed styles, drawn over the
   * raster tiles and beneath every overlay. Resolves `true` once the SDK
   * accepted it; a refusal reports through the view's failure event.
   */
  setVectorTileSource(source: UrlTemplateVectorTileSource): Promise<boolean>;
  /**
   * Installs a provider-backed styled vector base painted with the style
   * document the provider publishes. The document is fetched with the
   * registered credential and kept for the session, so the application ships no
   * vendor style file. Resolves `true` once the SDK accepted it; a refusal
   * reports through the view's failure event and leaves the installed base in
   * force.
   */
  setProviderVectorTileSourceFromDocument(
    source: ProviderVectorTileSource,
    styleDocumentUrl: string,
  ): Promise<boolean>;
  /** Removes the vector base map; resolves `true` once the SDK accepted. */
  clearVectorTileSource(): Promise<boolean>;
  /** The vector base source's status readback, or null before the view exists. */
  getVectorTileSourceStatus(): Promise<VectorTileSourceStatus | null>;
  /** Retries a failed surface attach. */
  retryAttach(): void;
  /**
   * Re-resolves the SDK's raster-tile provider and drops every pooled tile,
   * so a provider, credential or source change reaches the next frame
   * without an attach/detach cycle. The camera, overlays, location indicator
   * and listeners keep their state.
   */
  reloadTileSource(): void;
  /** Pauses or resumes drawing explicitly. */
  setRenderingPaused(paused: boolean): void;
  /** The last camera the view adopted, or null before the first change. */
  getCamera(): MapCamera | null;
  /** Ground meters represented by one screen pixel, or null before attach. */
  groundMetersPerScreenPixel(): Promise<number | null>;
}

/** One native view event on the package's event channel. */
interface NativeMapViewEvent {
  viewIdentifier: string;
  event: string;
  camera?: MapCamera;
  isGestureDriven?: boolean;
  coordinate?: GeographicCoordinate;
  failurePhase?: MapViewFailurePhase;
  failureMessage?: string;
  statistics?: FrameStatistics;
}

/** The package's native module surface this component uses. */
interface MapsSdkNativeModule {
  setCamera(viewIdentifier: string, cameraJson: string): void;
  animateCamera(
    viewIdentifier: string,
    cameraJson: string,
    durationMilliseconds: number,
  ): void;
  fitBounds(
    viewIdentifier: string,
    boundingBoxJson: string,
    edgeInsetsJson: string,
    maximumZoomLevel: number,
  ): void;
  panByScreenPixels(
    viewIdentifier: string,
    deltaXPixels: number,
    deltaYPixels: number,
    isGestureDriven: boolean,
  ): void;
  zoomByLevels(viewIdentifier: string, deltaZoomLevels: number): void;
  rotateByDegrees(viewIdentifier: string, clockwiseDegrees: number): void;
  viewportWidthPixels(viewIdentifier: string): Promise<number>;
  viewportHeightPixels(viewIdentifier: string): Promise<number>;
  viewportPixelExtent(
    viewIdentifier: string,
  ): Promise<ViewportSize | null>;
  displayScale(viewIdentifier: string): Promise<number>;
  addOverlay(
    viewIdentifier: string,
    overlayIdentifier: string,
    overlayJson: string,
  ): Promise<boolean>;
  updateOverlay(
    viewIdentifier: string,
    overlayIdentifier: string,
    overlayJson: string,
  ): Promise<boolean>;
  removeOverlay(
    viewIdentifier: string,
    overlayIdentifier: string,
  ): Promise<boolean>;
  setOverlays(
    viewIdentifier: string,
    overlaysJson: string,
  ): Promise<boolean>;
  clearOverlays(viewIdentifier: string): Promise<boolean>;
  setLocationIndicator(
    viewIdentifier: string,
    stateJson: string,
    styleJson: string,
  ): void;
  clearLocationIndicator(viewIdentifier: string): void;
  setVectorTileSource(
    viewIdentifier: string,
    sourceJson: string,
  ): Promise<boolean>;
  setProviderVectorTileSourceFromDocument(
    viewIdentifier: string,
    sourceJson: string,
    styleDocumentUrl: string,
  ): Promise<boolean>;
  clearVectorTileSource(viewIdentifier: string): Promise<boolean>;
  getVectorTileSourceStatus(viewIdentifier: string): Promise<string>;
  retryAttach(viewIdentifier: string): void;
  reloadTileSource(viewIdentifier: string): void;
  setRenderingPaused(viewIdentifier: string, paused: boolean): void;
  getCamera(viewIdentifier: string): Promise<string | null>;
  /** The view's first-frame state (the `mapReady` event's condition). */
  isMapReady(viewIdentifier: string): Promise<boolean>;
  groundMetersPerScreenPixel(viewIdentifier: string): Promise<number | null>;
}

/** The event channel both native halves emit on (one named event). */
const nativeMapViewEventName = 'MapsSdkMapViewEvent';

/** The native view manager's registered component name. */
const nativeMapViewComponentName = 'MapsSdkMapView';

/** The default camera animation duration (the design language's §5.3). */
const defaultCameraAnimationDurationMilliseconds = 300;

/** The SDK's maximum zoom level, the native fit-bounds ceiling. */
const defaultFitBoundsMaximumZoomLevel = 24;

/** No insets, the natural default of `fitBounds`. */
const zeroEdgeInsets: EdgeInsets = { left: 0, top: 0, right: 0, bottom: 0 };

/** The package's native module, or null when the native half is absent. */
const nativeModule: MapsSdkNativeModule | null =
  (NativeModules as Record<string, MapsSdkNativeModule | undefined>)
    .MapsSdkNative ?? null;

/** The same module with every bridge rejection surfaced as `MapsSdkError`. */
const failureWrappedNativeModule: MapsSdkNativeModule | null =
  nativeModule === null ? null : wrapNativeFailures(nativeModule);

/**
 * The native module, or the package's typed missing-native refusal. Every
 * imperative handle call needs the native view; a JavaScript-only environment
 * gets one catchable `MapsSdkError` (`MapsSdkNativeUnavailable`) instead of a
 * silent no-op or a per-method fallback value.
 */
function requireNativeModule(): MapsSdkNativeModule {
  if (failureWrappedNativeModule === null) {
    throw MapsSdkError.nativeUnavailable();
  }
  return failureWrappedNativeModule;
}

/** A stable, process-unique identifier the native view registry keys on. */
let nextViewIdentifier = 0;

function allocateViewIdentifier(): string {
  nextViewIdentifier += 1;
  return `maps-sdk-map-${nextViewIdentifier}`;
}

/** The native component (the view manager creates the native host view). */
const NativeMapView = requireNativeComponent<{
  style?: StyleProp<ViewStyle>;
  testID?: string;
  viewIdentifier: string;
  topBarInsetPixels: number;
  bottomBarInsetPixels: number;
  leftBarInsetPixels: number;
  rightBarInsetPixels: number;
  isGestureEnabled: boolean;
  isRenderingPaused: boolean;
}>(nativeMapViewComponentName);

/**
 * Subscribes to the package's single native view event channel.
 *
 * Android's device-event emitter delivers unconditionally; the iOS
 * `RCTEventEmitter` only emits when a JS listener is registered on the module
 * itself, so the subscription goes through `NativeEventEmitter` there (the
 * same platform split the reference shell already uses).
 */
function subscribeToNativeMapViewEvents(
  handler: (event: NativeMapViewEvent) => void,
): () => void {
  if (Platform.OS === 'ios' && nativeModule !== null) {
    const moduleEmitter = new NativeEventEmitter(
      nativeModule as unknown as {
        addListener(eventName: string): void;
        removeListeners(count: number): void;
      },
    );
    const subscription = moduleEmitter.addListener(
      nativeMapViewEventName,
      handler as (event: unknown) => void,
    );
    return () => subscription.remove();
  }
  const subscription = DeviceEventEmitter.addListener(
    nativeMapViewEventName,
    handler,
  );
  return () => subscription.remove();
}

/**
 * The typed map component. Hold its ref to drive the native view:
 *
 * ```tsx
 * const mapView = useRef<MapViewHandle>(null);
 * await MapsSdk.create();
 * <MapView ref={mapView} onMapReady={() => mapView.current?.setCamera(...)} />
 * ```
 */
export const MapView = forwardRef<MapViewHandle, MapViewProps>(
  function MapView(props, ref): React.JSX.Element {
    const [viewIdentifier] = useState(allocateViewIdentifier);
    const lastCameraRef = useRef<MapCamera | null>(null);
    const hasDeliveredMapReadyRef = useRef(false);
    const propsRef = useRef(props);
    propsRef.current = props;

    /**
     * Reports readiness once. The native event and the late-subscriber
     * readback can both observe the same first frame; the guard keeps the
     * consumer callback from firing twice.
     */
    const deliverMapReady = useCallback(() => {
      if (hasDeliveredMapReadyRef.current) {
        return;
      }
      hasDeliveredMapReadyRef.current = true;
      propsRef.current.onMapReady?.();
    }, []);

    useEffect(() => {
      let isDisposed = false;
      const unsubscribe = subscribeToNativeMapViewEvents(nativeEvent => {
        if (nativeEvent.viewIdentifier !== viewIdentifier) {
          return;
        }
        const currentProps = propsRef.current;
        switch (nativeEvent.event) {
          case 'mapReady':
            deliverMapReady();
            break;
          case 'cameraChanged':
            if (nativeEvent.camera !== undefined) {
              lastCameraRef.current = nativeEvent.camera;
              currentProps.onCameraChanged?.(
                nativeEvent.camera,
                nativeEvent.isGestureDriven === true,
              );
            }
            break;
          case 'mapClick':
            if (nativeEvent.coordinate !== undefined) {
              currentProps.onMapClick?.(nativeEvent.coordinate);
            }
            break;
          case 'error':
            currentProps.onError?.({
              phase: nativeEvent.failurePhase ?? 'draw',
              message: nativeEvent.failureMessage ?? '',
            });
            break;
          case 'frameStatistics':
            if (nativeEvent.statistics !== undefined) {
              currentProps.onFrameStatistics?.(nativeEvent.statistics);
            }
            break;
          case 'frameDeferred':
            currentProps.onFrameDeferred?.();
            break;
          case 'frameSettled':
            currentProps.onFrameSettled?.();
            break;
          default:
            break;
        }
      });
      // The ready event fires once, on the first presented frame. A
      // subscription that attaches after that frame would never hear it, so
      // the native module answers the same first-frame state when asked. A
      // rejection means the native view is not registered yet; the event
      // channel delivers that view's first frame.
      const readyStateRequest = nativeModule?.isMapReady?.(viewIdentifier);
      readyStateRequest
        ?.then(isMapReady => {
          if (!isDisposed && isMapReady) {
            deliverMapReady();
          }
        })
        .catch(() => {
          // No registered view to answer yet (see above).
        });
      return () => {
        isDisposed = true;
        unsubscribe();
      };
    }, [deliverMapReady, viewIdentifier]);

    const applyRenderingPaused = useCallback(
      (paused: boolean) => {
        requireNativeModule().setRenderingPaused(viewIdentifier, paused);
      },
      [viewIdentifier],
    );

    // The declarative pause prop mirrors the native property; an explicit
    // handle call wins until the prop changes again.
    useEffect(() => {
      applyRenderingPaused(props.isRenderingPaused === true);
    }, [applyRenderingPaused, props.isRenderingPaused]);

    useImperativeHandle(
      ref,
      (): MapViewHandle => ({
        setCamera(camera) {
          requireNativeModule().setCamera(viewIdentifier, JSON.stringify(camera));
        },
        animateCamera(camera, durationMilliseconds) {
          requireNativeModule().animateCamera(
            viewIdentifier,
            JSON.stringify(camera),
            durationMilliseconds ?? defaultCameraAnimationDurationMilliseconds,
          );
        },
        fitBounds(boundingBox, edgeInsets, maximumZoomLevel) {
          requireNativeModule().fitBounds(
            viewIdentifier,
            JSON.stringify(boundingBox),
            JSON.stringify(edgeInsets ?? zeroEdgeInsets),
            maximumZoomLevel ?? defaultFitBoundsMaximumZoomLevel,
          );
        },
        panByScreenPixels(deltaXPixels, deltaYPixels, isGestureDriven) {
          requireNativeModule().panByScreenPixels(
            viewIdentifier,
            deltaXPixels,
            deltaYPixels,
            isGestureDriven ?? true,
          );
        },
        zoomByLevels(deltaZoomLevels) {
          requireNativeModule().zoomByLevels(viewIdentifier, deltaZoomLevels);
        },
        rotateByDegrees(clockwiseDegrees) {
          requireNativeModule().rotateByDegrees(
            viewIdentifier,
            clockwiseDegrees,
          );
        },
        hasDrawnFirstFrame() {
          // The same first-frame readback the readiness replay uses.
          return requireNativeModule().isMapReady(viewIdentifier);
        },
        viewportWidthPixels() {
          return requireNativeModule().viewportWidthPixels(viewIdentifier);
        },
        viewportHeightPixels() {
          return requireNativeModule().viewportHeightPixels(viewIdentifier);
        },
        viewportPixelExtent() {
          return requireNativeModule().viewportPixelExtent(viewIdentifier);
        },
        displayScale() {
          return requireNativeModule().displayScale(viewIdentifier);
        },
        addOverlay(overlayIdentifier, overlay) {
          return requireNativeModule().addOverlay(
            viewIdentifier,
            overlayIdentifier,
            JSON.stringify(overlay),
          );
        },
        updateOverlay(overlayIdentifier, overlay) {
          return requireNativeModule().updateOverlay(
            viewIdentifier,
            overlayIdentifier,
            JSON.stringify(overlay),
          );
        },
        removeOverlay(overlayIdentifier) {
          return requireNativeModule().removeOverlay(
            viewIdentifier,
            overlayIdentifier,
          );
        },
        setOverlays(overlays) {
          return requireNativeModule().setOverlays(
            viewIdentifier,
            JSON.stringify(overlays),
          );
        },
        clearOverlays() {
          return requireNativeModule().clearOverlays(viewIdentifier);
        },
        setLocationIndicator(state, style) {
          requireNativeModule().setLocationIndicator(
            viewIdentifier,
            JSON.stringify(state),
            JSON.stringify(style),
          );
        },
        clearLocationIndicator() {
          requireNativeModule().clearLocationIndicator(viewIdentifier);
        },
        setVectorTileSource(source) {
          return requireNativeModule().setVectorTileSource(
            viewIdentifier,
            JSON.stringify(source),
          );
        },
        setProviderVectorTileSourceFromDocument(source, styleDocumentUrl) {
          return requireNativeModule().setProviderVectorTileSourceFromDocument(
            viewIdentifier,
            JSON.stringify(source),
            styleDocumentUrl,
          );
        },
        clearVectorTileSource() {
          return requireNativeModule().clearVectorTileSource(viewIdentifier);
        },
        async getVectorTileSourceStatus() {
          const statusJson =
            await requireNativeModule().getVectorTileSourceStatus(viewIdentifier);
          try {
            const status = JSON.parse(statusJson) as VectorTileSourceStatus;
            return status.isConfigured ? status : null;
          } catch {
            return null;
          }
        },
        retryAttach() {
          requireNativeModule().retryAttach(viewIdentifier);
        },
        reloadTileSource() {
          requireNativeModule().reloadTileSource(viewIdentifier);
        },
        setRenderingPaused(paused) {
          applyRenderingPaused(paused);
        },
        getCamera() {
          return lastCameraRef.current;
        },
        groundMetersPerScreenPixel() {
          return requireNativeModule().groundMetersPerScreenPixel(viewIdentifier);
        },
      }),
      [applyRenderingPaused, viewIdentifier],
    );

    return (
      <NativeMapView
        style={props.style}
        testID={props.testID}
        viewIdentifier={viewIdentifier}
        topBarInsetPixels={props.topBarInsetPixels ?? 0}
        bottomBarInsetPixels={props.bottomBarInsetPixels ?? 0}
        leftBarInsetPixels={props.leftBarInsetPixels ?? 0}
        rightBarInsetPixels={props.rightBarInsetPixels ?? 0}
        isGestureEnabled={props.isGestureEnabled !== false}
        isRenderingPaused={props.isRenderingPaused === true}
      />
    );
  },
);

export default MapView;
