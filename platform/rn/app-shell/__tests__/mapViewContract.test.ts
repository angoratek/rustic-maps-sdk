/**
 * Typed-API parity for the package's `<MapView>`.
 *
 * The native `MapView` (Android AAR / iOS package) exposes one listener
 * (ready, camera changed, click, failure, frame statistics, deferred,
 * settled), the camera verbs, the contract's pan/zoom gesture members, the
 * id-addressed overlay calls, the location layer and the readbacks. This test
 * builds the JavaScript handle and props object against those exact
 * signatures, with the contract's own record types, so a drift in either the
 * component or the contract fails `tsc`/jest rather than an app at runtime.
 */

import type {
  BoundingBox,
  EdgeInsets,
  FrameStatistics,
  GeographicCoordinate,
  LocationIndicatorState,
  LocationIndicatorStyle,
  MapCamera,
  MapOverlay,
  MapOverlayKind,
  MapViewFailure,
  MapViewHandle,
  MapViewProps,
  LocationIndicatorVisualKind,
  ProviderVectorTileSource,
  ViewportSize,
} from '@angoratek/rustic-maps-sdk/MapView';
import type {
  UrlTemplateVectorTileSource,
  VectorTileSourceStatus,
} from '@angoratek/rustic-maps-sdk/MapsSdk';

// The component module is not imported at runtime: this is a type-level
// assertion test, and loading the native component is covered by the app
// suite's mocked render.

test('the MapView handle mirrors the native camera, overlay and location calls', async () => {
  const camera: MapCamera = {
    latitudeDegrees: 38.58,
    longitudeDegrees: -121.49,
    zoomLevel: 14,
    bearingDegrees: 0,
    pitchDegrees: 0,
  };
  const boundingBox: BoundingBox = {
    southWest: { latitude: 38.5, longitude: -121.6 },
    northEast: { latitude: 38.7, longitude: -121.4 },
  };
  const edgeInsets: EdgeInsets = { left: 1, top: 2, right: 3, bottom: 4 };
  const overlay: MapOverlay = {
    kind: 0 as MapOverlayKind,
    coordinates: [{ latitude: 38.58, longitude: -121.49 }],
  };
  const locationState: LocationIndicatorState = {
    coordinate: { latitude: 38.58, longitude: -121.49 },
    courseHeadingDegrees: 90,
    horizontalAccuracyMeters: 5,
  };
  const vectorSource: UrlTemplateVectorTileSource = {
    urlTemplate: 'https://tiles.example.org/{z}/{x}/{y}.pbf',
    sourceName: 'vector-provider',
    attributionText: 'Style Bright, design CC BY 4.0',
    styleId: 'bright-light',
    minimumZoomLevel: 0,
    maximumZoomLevel: 14,
  };
  const locationStyle: LocationIndicatorStyle = {
    visualKind: 0 as LocationIndicatorVisualKind,
    radiusPixels: undefined,
    rgbaColor: undefined,
    showAccuracyRing: true,
  };

  const handle: MapViewHandle = {
    setCamera: (nextCamera: MapCamera) => { void nextCamera; },
    animateCamera: (nextCamera: MapCamera, durationMilliseconds?: number) => {
      void nextCamera;
      void durationMilliseconds;
    },
    fitBounds: (
      nextBounds: BoundingBox,
      nextEdgeInsets?: EdgeInsets,
      maximumZoomLevel?: number,
    ) => {
      void nextBounds;
      void nextEdgeInsets;
      void maximumZoomLevel;
    },
    panByScreenPixels: (
      deltaXPixels: number,
      deltaYPixels: number,
      isGestureDriven?: boolean,
    ) => {
      void deltaXPixels;
      void deltaYPixels;
      void isGestureDriven;
    },
    zoomByLevels: (deltaZoomLevels: number) => { void deltaZoomLevels; },
    rotateByDegrees: (clockwiseDegrees: number) => { void clockwiseDegrees; },
    hasDrawnFirstFrame: async (): Promise<boolean> => false,
    viewportWidthPixels: async (): Promise<number> => 0,
    viewportHeightPixels: async (): Promise<number> => 0,
    viewportPixelExtent: async (): Promise<ViewportSize | null> => null,
    displayScale: async (): Promise<number> => 2,
    addOverlay: async (
      overlayIdentifier: string,
      nextOverlay: MapOverlay,
    ): Promise<boolean> => {
      void overlayIdentifier;
      void nextOverlay;
      return true;
    },
    updateOverlay: async (
      overlayIdentifier: string,
      nextOverlay: MapOverlay,
    ): Promise<boolean> => {
      void overlayIdentifier;
      void nextOverlay;
      return true;
    },
    removeOverlay: async (overlayIdentifier: string): Promise<boolean> => {
      void overlayIdentifier;
      return true;
    },
    setOverlays: async (
      overlays: ReadonlyArray<MapOverlay>,
    ): Promise<boolean> => {
      void overlays;
      return true;
    },
    clearOverlays: async (): Promise<boolean> => true,
    setLocationIndicator: (
      state: LocationIndicatorState,
      style: LocationIndicatorStyle,
    ) => {
      void state;
      void style;
    },
    clearLocationIndicator: () => {},
    setVectorTileSource: async (source: UrlTemplateVectorTileSource) => {
      void source;
      return true;
    },
    setProviderVectorTileSourceFromDocument: async (
      source: ProviderVectorTileSource,
      styleDocumentUrl: string,
    ) => {
      void source;
      void styleDocumentUrl;
      return true;
    },
    clearVectorTileSource: async (): Promise<boolean> => true,
    getVectorTileSourceStatus: async (): Promise<VectorTileSourceStatus | null> =>
      null,
    retryAttach: () => {},
    reloadTileSource: () => {},
    setRenderingPaused: (paused: boolean) => { void paused; },
    getCamera: (): MapCamera | null => camera,
    groundMetersPerScreenPixel: async (): Promise<number | null> => 1.5,
  };

  handle.setCamera(camera);
  handle.animateCamera(camera);
  handle.fitBounds(boundingBox, edgeInsets, 17);
  handle.panByScreenPixels(10, -10, true);
  handle.zoomByLevels(0.5);
  handle.rotateByDegrees(15);
  await expect(handle.hasDrawnFirstFrame()).resolves.toBe(false);
  await expect(handle.viewportWidthPixels()).resolves.toBe(0);
  await expect(handle.viewportHeightPixels()).resolves.toBe(0);
  await expect(handle.viewportPixelExtent()).resolves.toBeNull();
  await expect(handle.displayScale()).resolves.toBe(2);
  await expect(handle.addOverlay('route', overlay)).resolves.toBe(true);
  await expect(handle.updateOverlay('route', overlay)).resolves.toBe(true);
  await expect(handle.removeOverlay('route')).resolves.toBe(true);
  await expect(handle.setOverlays([overlay])).resolves.toBe(true);
  await expect(handle.clearOverlays()).resolves.toBe(true);
  await expect(
    handle.setVectorTileSource(vectorSource),
  ).resolves.toBe(true);
  await expect(
    handle.setProviderVectorTileSourceFromDocument(
      { providerKey: 'vector-provider', styleId: 'streets-light' },
      'https://styles.example.org/v1/streets',
    ),
  ).resolves.toBe(true);
  await expect(handle.clearVectorTileSource()).resolves.toBe(true);
  await expect(handle.getVectorTileSourceStatus()).resolves.toBeNull();
  handle.setLocationIndicator(locationState, locationStyle);
  handle.clearLocationIndicator();
  handle.retryAttach();
  handle.reloadTileSource();
  handle.setRenderingPaused(false);
  expect(handle.getCamera()).toEqual(camera);
  await expect(handle.groundMetersPerScreenPixel()).resolves.toBe(1.5);
});

test('the MapView props mirror the native listener callbacks', () => {
  const statistics: FrameStatistics = {
    requestedTileCount: 1,
    presentTileCount: 1,
    fallbackTileCount: 0,
    deferredTileCount: 0,
    unavailableTileCount: 0,
    overlayTriangleCount: 0,
  };
  const coordinate: GeographicCoordinate = { latitude: 0, longitude: 0 };
  const failure: MapViewFailure = { phase: 'draw', message: 'busy' };

  const props: MapViewProps = {
    style: undefined,
    testID: 'map',
    topBarInsetPixels: 0,
    bottomBarInsetPixels: 0,
    leftBarInsetPixels: 0,
    rightBarInsetPixels: 0,
    isGestureEnabled: true,
    isRenderingPaused: false,
    onMapReady: () => {},
    onCameraChanged: (camera: MapCamera, isGestureDriven: boolean) => {
      void camera;
      void isGestureDriven;
    },
    onMapClick: (clickCoordinate: GeographicCoordinate) => {
      void clickCoordinate;
    },
    onError: (mapFailure: MapViewFailure) => { void mapFailure; },
    onFrameStatistics: (frameStatistics: FrameStatistics) => {
      void frameStatistics;
    },
    onFrameDeferred: () => {},
    onFrameSettled: () => {},
  };

  props.onMapReady?.();
  props.onCameraChanged?.(
    {
      latitudeDegrees: 0,
      longitudeDegrees: 0,
      zoomLevel: 2,
      bearingDegrees: 0,
      pitchDegrees: 0,
    },
    true,
  );
  props.onMapClick?.(coordinate);
  props.onError?.(failure);
  props.onFrameStatistics?.(statistics);
  props.onFrameDeferred?.();
  props.onFrameSettled?.();
  expect(props.testID).toBe('map');
});
