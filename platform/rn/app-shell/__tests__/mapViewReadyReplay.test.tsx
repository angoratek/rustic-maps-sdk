/**
 * The late-subscriber readiness rule.
 *
 * The native view reports `mapReady` once, on its first presented frame. A
 * JavaScript subscription can attach after that frame, and the event channel
 * does not replay it. The component therefore reads the same first-frame
 * state back from the native module; the readback and the event share one
 * delivery guard, so the consumer callback fires exactly once whichever path
 * observes readiness first.
 *
 * The package's native module is a local fake here: the real Swift and Kotlin
 * implementations are covered by their own platform builds, and this pins the
 * JavaScript half without a native runtime or a network call.
 */

import React from 'react';
import ReactTestRenderer from 'react-test-renderer';
import { DeviceEventEmitter, NativeModules } from 'react-native';

import type { MapViewProps } from 'maps-sdk-rn/MapView';

interface MapReadyNativeModuleMock {
  isMapReady: jest.Mock<Promise<boolean>, [string]>;
}

let nativeModuleMock: MapReadyNativeModuleMock;

beforeAll(() => {
  nativeModuleMock = {
    isMapReady: jest.fn(async (_viewIdentifier: string) => true),
  };
  (NativeModules as unknown as Record<string, unknown>).MapsSdkNative = {
    ...nativeModuleMock,
    setRenderingPaused: jest.fn(),
    addListener: jest.fn(),
    removeListeners: jest.fn(),
  };
});

beforeEach(() => {
  nativeModuleMock.isMapReady.mockReset();
  nativeModuleMock.isMapReady.mockResolvedValue(true);
});

/**
 * Loads the component after the native module fake is installed: the
 * component reads `NativeModules.MapsSdkNative` once at module load, so a
 * static import would capture the absent native half.
 */
function loadMapView(): React.ComponentType<MapViewProps> {
  const mapViewModule = jest.requireActual<typeof import('maps-sdk-rn/MapView')>(
    'maps-sdk-rn/MapView',
  );
  return mapViewModule.MapView;
}

/** The identifier the rendered host view passes to the native side. */
function renderedViewIdentifier(
  testRenderer: ReactTestRenderer.ReactTestRenderer,
): string {
  const nativeView = testRenderer.root
    .findAllByProps({ testID: 'map' })
    .find(node => typeof node.props.viewIdentifier === 'string');
  expect(nativeView).toBeDefined();
  return nativeView!.props.viewIdentifier as string;
}

/** Flushes the readiness readback promise the mount effect starts. */
async function flushReadinessReadback(): Promise<void> {
  await ReactTestRenderer.act(async () => {
    await Promise.resolve();
  });
}

test('a subscriber that missed the native mapReady event still learns readiness once', async () => {
  const MapView = loadMapView();
  const onMapReady = jest.fn();
  let testRenderer: ReactTestRenderer.ReactTestRenderer;
  await ReactTestRenderer.act(() => {
    testRenderer = ReactTestRenderer.create(
      <MapView testID="map" onMapReady={onMapReady} />,
    );
  });
  await flushReadinessReadback();

  expect(nativeModuleMock.isMapReady).toHaveBeenCalledTimes(1);
  expect(onMapReady).toHaveBeenCalledTimes(1);

  // The event for the same first frame arrives afterwards; the shared guard
  // keeps it from reporting readiness a second time.
  const viewIdentifier = renderedViewIdentifier(testRenderer!);
  await ReactTestRenderer.act(() => {
    DeviceEventEmitter.emit('MapsSdkMapViewEvent', {
      viewIdentifier,
      event: 'mapReady',
    });
  });
  expect(onMapReady).toHaveBeenCalledTimes(1);

  await ReactTestRenderer.act(() => {
    testRenderer!.unmount();
  });
});

test('a map that has not drawn reports readiness only when the native event arrives', async () => {
  nativeModuleMock.isMapReady.mockResolvedValueOnce(false);
  const MapView = loadMapView();
  const onMapReady = jest.fn();
  let testRenderer: ReactTestRenderer.ReactTestRenderer;
  await ReactTestRenderer.act(() => {
    testRenderer = ReactTestRenderer.create(
      <MapView testID="map" onMapReady={onMapReady} />,
    );
  });
  await flushReadinessReadback();

  expect(nativeModuleMock.isMapReady).toHaveBeenCalledTimes(1);
  expect(onMapReady).not.toHaveBeenCalled();

  const viewIdentifier = renderedViewIdentifier(testRenderer!);
  await ReactTestRenderer.act(() => {
    DeviceEventEmitter.emit('MapsSdkMapViewEvent', {
      viewIdentifier,
      event: 'mapReady',
    });
  });
  expect(onMapReady).toHaveBeenCalledTimes(1);

  await ReactTestRenderer.act(() => {
    testRenderer!.unmount();
  });
});
