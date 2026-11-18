/**
 * The package `MapView` delegates its camera path to the native view.
 *
 * The native halves own the contract camera value: the JavaScript handle
 * forwards set, animate and fit to the native module and reads the last
 * adopted pose from the native `cameraChanged` event. This test renders the
 * real component against a fake native module and asserts that forwarding,
 * so a second camera state in JavaScript cannot reappear unnoticed.
 */

import React from 'react';
import ReactTestRenderer from 'react-test-renderer';
import { DeviceEventEmitter, NativeModules } from 'react-native';

import type { MapCamera, MapViewHandle } from 'maps-sdk-rn/MapView';

const boundingBox = {
  southWest: { latitude: 38.4, longitude: -121.7 },
  northEast: { latitude: 38.8, longitude: -121.2 },
};

const camera: MapCamera = {
  latitudeDegrees: 38.58,
  longitudeDegrees: -121.49,
  zoomLevel: 14,
  bearingDegrees: 45,
  pitchDegrees: 0,
};

let setCamera: jest.Mock;
let animateCamera: jest.Mock;
let fitBounds: jest.Mock;

beforeAll(() => {
  setCamera = jest.fn();
  animateCamera = jest.fn();
  fitBounds = jest.fn();
  (NativeModules as unknown as Record<string, unknown>).MapsSdkNative = {
    setCamera,
    animateCamera,
    fitBounds,
    isMapReady: jest.fn(async () => false),
    setRenderingPaused: jest.fn(),
    addListener: jest.fn(),
    removeListeners: jest.fn(),
  };
});

function loadMapView() {
  return jest.requireActual<typeof import('maps-sdk-rn/MapView')>(
    'maps-sdk-rn/MapView',
  ).MapView;
}

test('the handle forwards set, animate and fit to the native view', async () => {
  const MapView = loadMapView();
  const handleRef = React.createRef<MapViewHandle>();
  let testRenderer: ReactTestRenderer.ReactTestRenderer;
  await ReactTestRenderer.act(() => {
    testRenderer = ReactTestRenderer.create(<MapView ref={handleRef} testID="map" />);
  });

  const viewIdentifier = testRenderer!.root
    .findAllByProps({ testID: 'map' })
    .find(node => typeof node.props.viewIdentifier === 'string')!
    .props.viewIdentifier as string;

  await ReactTestRenderer.act(async () => {
    handleRef.current!.setCamera(camera);
    handleRef.current!.animateCamera(camera);
    handleRef.current!.animateCamera(camera, 500);
    handleRef.current!.fitBounds(boundingBox);
    handleRef.current!.fitBounds(boundingBox, { left: 1, top: 2, right: 3, bottom: 4 }, 17);
  });

  expect(setCamera).toHaveBeenCalledWith(viewIdentifier, JSON.stringify(camera));
  expect(animateCamera).toHaveBeenNthCalledWith(
    1,
    viewIdentifier,
    JSON.stringify(camera),
    300,
  );
  expect(animateCamera).toHaveBeenNthCalledWith(
    2,
    viewIdentifier,
    JSON.stringify(camera),
    500,
  );
  expect(fitBounds).toHaveBeenNthCalledWith(
    1,
    viewIdentifier,
    JSON.stringify(boundingBox),
    JSON.stringify({ left: 0, top: 0, right: 0, bottom: 0 }),
    24,
  );
  expect(fitBounds).toHaveBeenNthCalledWith(
    2,
    viewIdentifier,
    JSON.stringify(boundingBox),
    JSON.stringify({ left: 1, top: 2, right: 3, bottom: 4 }),
    17,
  );
});

test('getCamera projects the last native camera change, not a local state', async () => {
  const MapView = loadMapView();
  const handleRef = React.createRef<MapViewHandle>();
  let testRenderer: ReactTestRenderer.ReactTestRenderer;
  await ReactTestRenderer.act(() => {
    testRenderer = ReactTestRenderer.create(<MapView ref={handleRef} testID="map" />);
  });

  const viewIdentifier = testRenderer!.root
    .findAllByProps({ testID: 'map' })
    .find(node => typeof node.props.viewIdentifier === 'string')!
    .props.viewIdentifier as string;

  expect(handleRef.current!.getCamera()).toBeNull();

  await ReactTestRenderer.act(async () => {
    DeviceEventEmitter.emit('MapsSdkMapViewEvent', {
      viewIdentifier,
      event: 'cameraChanged',
      camera,
      isGestureDriven: false,
    });
  });

  expect(handleRef.current!.getCamera()).toEqual(camera);
});
