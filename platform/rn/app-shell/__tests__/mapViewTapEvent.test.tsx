/**
 * The JS surface of the confirmed-tap rule (§5.2), at the package boundary.
 *
 * The native hosts own the gesture recognition: a double tap zooms and their
 * single-tap recognizers wait for it to fail, so one double tap reports no
 * `onMapClick`. This test covers the JavaScript half of that contract — one
 * forwarded native `mapClick` event must reach the consumer callback exactly
 * once, with the native view's coordinate. It renders the real package
 * `MapView` component (the native view manager is the jest preset's host
 * stand-in) and emits on the package's own event channel.
 */

import React from 'react';
import ReactTestRenderer from 'react-test-renderer';
import { DeviceEventEmitter, NativeModules } from 'react-native';

import type { MapViewProps } from 'maps-sdk-rn/MapView';

/**
 * Installs the native module fake the component expects at load time. The
 * component reads `NativeModules.MapsSdkNative` once, so the fake must exist
 * before it is required; the render path also pauses rendering through the
 * module on mount.
 */
beforeAll(() => {
  (NativeModules as unknown as Record<string, unknown>).MapsSdkNative = {
    isMapReady: jest.fn(async () => false),
    setRenderingPaused: jest.fn(),
    addListener: jest.fn(),
    removeListeners: jest.fn(),
  };
});

/** Loads the component after the native module fake is installed. */
function loadMapView(): React.ComponentType<MapViewProps> {
  return jest.requireActual<typeof import('maps-sdk-rn/MapView')>(
    'maps-sdk-rn/MapView',
  ).MapView;
}

test('one forwarded native mapClick event reaches the consumer callback exactly once', async () => {
  const MapView = loadMapView();
  const onMapClick = jest.fn();
  let testRenderer: ReactTestRenderer.ReactTestRenderer;
  await ReactTestRenderer.act(() => {
    testRenderer = ReactTestRenderer.create(
      <MapView testID="map" onMapClick={onMapClick} />,
    );
  });

  // The component allocates the identifier the native events carry; read it
  // back from the rendered host view rather than guessing it.
  const nativeView = testRenderer!.root
    .findAllByProps({ testID: 'map' })
    .find(node => typeof node.props.viewIdentifier === 'string');
  expect(nativeView).toBeDefined();
  const viewIdentifier = nativeView!.props.viewIdentifier as string;

  const clickedCoordinate = { latitude: 38.58, longitude: -121.49 };
  await ReactTestRenderer.act(async () => {
    DeviceEventEmitter.emit('MapsSdkMapViewEvent', {
      viewIdentifier,
      event: 'mapClick',
      coordinate: clickedCoordinate,
    });
  });

  expect(onMapClick).toHaveBeenCalledTimes(1);
  expect(onMapClick).toHaveBeenCalledWith(clickedCoordinate);
});
