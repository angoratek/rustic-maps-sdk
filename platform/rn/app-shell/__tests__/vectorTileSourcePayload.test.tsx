/**
 * The styled vector base source's full contract payload at the package
 * boundary. The native decoders turn this JSON into the generated
 * `UrlTemplateVectorTileSource` record, so every field the contract carries
 * must cross the bridge: the tileset's terms, its `{s}` subdomains, the headers
 * each request carries and the style hints its provider declares.
 *
 * A field the caller omits stays absent from the payload (the decoders read an
 * absent member as null); a field the caller sets to null crosses as null.
 */

import React from 'react';
import ReactTestRenderer from 'react-test-renderer';
import { NativeModules } from 'react-native';

import type { MapViewHandle } from '@angoratek/rustic-maps-sdk/MapView';
import type { UrlTemplateVectorTileSource } from '@angoratek/rustic-maps-sdk/MapsSdk';

const setVectorTileSource = jest.fn(
  async (_viewIdentifier: string, _sourceJson: string) => true,
);

beforeEach(() => {
  setVectorTileSource.mockClear();
});

beforeAll(() => {
  (NativeModules as unknown as Record<string, unknown>).MapsSdkNative = {
    isMapReady: jest.fn(async () => false),
    setRenderingPaused: jest.fn(),
    addListener: jest.fn(),
    removeListeners: jest.fn(),
    setVectorTileSource,
  };
});

/** Loads the component after the native module fake is installed. */
function loadMapView() {
  return jest.requireActual<typeof import('@angoratek/rustic-maps-sdk/MapView')>(
    '@angoratek/rustic-maps-sdk/MapView',
  ).MapView;
}

/**
 * Mounts the real package component and returns its imperative handle plus
 * the native view identifier the payload addresses.
 */
async function mountMapView(): Promise<{
  handle: MapViewHandle;
  viewIdentifier: string;
}> {
  const MapView = loadMapView();
  const handleRef = React.createRef<MapViewHandle>();
  let testRenderer: ReactTestRenderer.ReactTestRenderer;
  await ReactTestRenderer.act(() => {
    testRenderer = ReactTestRenderer.create(
      <MapView ref={handleRef} testID="map" />,
    );
  });
  const nativeView = testRenderer!.root
    .findAllByProps({ testID: 'map' })
    .find(node => typeof node.props.viewIdentifier === 'string');
  if (handleRef.current === null || nativeView === undefined) {
    throw new Error('the package MapView handle and host view must exist');
  }
  return {
    handle: handleRef.current,
    viewIdentifier: nativeView.props.viewIdentifier as string,
  };
}

/** The vector source's fields the curated TypeScript surface declares. */
const fullSource: UrlTemplateVectorTileSource = {
  urlTemplate: 'https://tiles.example.org/{s}/{z}/{x}/{y}.pbf',
  sourceName: 'vector-provider',
  attributionText: 'Style Bright, design CC BY 4.0',
  styleId: 'bright-light',
  minimumZoomLevel: 0,
  maximumZoomLevel: 14,
  tokenBucketCapacityTokens: 4,
  refillTokensPerSecond: 2,
  termsText: 'Tiles served under the provider terms.',
  subdomains: ['a', 'b'],
  headers: [
    { name: 'X-Tile-Client', value: 'rustic-maps' },
    { name: 'X-Trace', value: 'vector-base' },
  ],
  styleHints: ['bright', 'light'],
};

test('every source field reaches the native module payload', async () => {
  const { handle, viewIdentifier } = await mountMapView();

  await expect(handle.setVectorTileSource(fullSource)).resolves.toBe(true);

  expect(setVectorTileSource).toHaveBeenCalledTimes(1);
  const [payloadViewIdentifier, sourceJson] = setVectorTileSource.mock.calls[0];
  expect(payloadViewIdentifier).toBe(viewIdentifier);
  expect(JSON.parse(sourceJson)).toEqual(fullSource);
});

test('omitted source fields stay absent while explicit nulls cross as null', async () => {
  const { handle } = await mountMapView();
  const minimalSource: UrlTemplateVectorTileSource = {
    urlTemplate: 'https://tiles.example.org/{z}/{x}/{y}.pbf',
    sourceName: 'vector-provider',
    attributionText: 'Example tiles',
    styleId: 'streets-light',
    minimumZoomLevel: 3,
    maximumZoomLevel: 12,
  };

  await handle.setVectorTileSource(minimalSource);
  const omittedPayload = JSON.parse(setVectorTileSource.mock.calls[0][1]);
  expect(omittedPayload).not.toHaveProperty('termsText');
  expect(omittedPayload).not.toHaveProperty('subdomains');
  expect(omittedPayload).not.toHaveProperty('headers');
  expect(omittedPayload).not.toHaveProperty('styleHints');

  const nullSource: UrlTemplateVectorTileSource = {
    ...minimalSource,
    termsText: null,
    subdomains: null,
    headers: null,
    styleHints: null,
  };
  await handle.setVectorTileSource(nullSource);
  const nullPayload = JSON.parse(setVectorTileSource.mock.calls[1][1]);
  expect(nullPayload.termsText).toBeNull();
  expect(nullPayload.subdomains).toBeNull();
  expect(nullPayload.headers).toBeNull();
  expect(nullPayload.styleHints).toBeNull();
});
