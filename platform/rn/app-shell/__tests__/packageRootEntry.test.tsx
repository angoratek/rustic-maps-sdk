/**
 * The package's advertised root entry.
 *
 * `package.json` points `main` and `exports["."]` at `src/index.ts`, which
 * ubrn regenerates. The generation step appends the package's curated
 * surface, so a consumer can import `MapView` and `MapsSdk` from the root
 * beside the generated bindings. This smoke imports that root entry with the
 * native installer and the generated bindings stubbed (their native half is
 * absent in a JavaScript runner) and pins the exported surface.
 */

import * as packageRoot from 'maps-sdk-rn';
import { MapView as CuratedMapView } from 'maps-sdk-rn/MapView';
import { MapsSdk as CuratedMapsSdk } from 'maps-sdk-rn/MapsSdk';

// The generated installer talks to the native TurboModule; a JavaScript
// runner has no bridge runtime, so it is stubbed to a no-op installer.
jest.mock('maps-sdk-rn/src/NativeMapsSdkRn', () => ({
  __esModule: true,
  default: {
    installRustCrate: jest.fn(() => true),
    cleanupRustCrate: jest.fn(() => true),
  },
}));

// The generated bindings' `initialize()` runs the native checksum gate.
// Stub the generated module with one namespace member and an initializer so
// the root entry's own wiring (namespace export and initialization) runs.
jest.mock('maps-sdk-rn/src/generated/rustic_maps', () => ({
  __esModule: true,
  default: { initialize: jest.fn() },
  MapView: class GeneratedMapView {},
  MapSdk: class GeneratedMapSdk {},
}));

describe('package root entry', () => {
  test('exports the curated component and SDK classes', () => {
    expect(packageRoot.MapView).toBe(CuratedMapView);
    expect(packageRoot.MapsSdk).toBe(CuratedMapsSdk);
    expect(packageRoot.mapsSdk).toBeDefined();
    expect(typeof packageRoot.MapView).toBe('object');
  });

  test('exports the generated contract namespace beside the curated surface', () => {
    expect(packageRoot.rustic_maps).toBeDefined();
    // The generated member is re-exported at the root even though the
    // curated component shadows the generated `MapView` name.
    expect(typeof packageRoot.MapSdk).toBe('function');
    // The generated namespace keeps the contract's own `MapView` object even
    // though the curated component shadows that name at the root.
    expect(
      (packageRoot.rustic_maps as unknown as { MapView: unknown }).MapView,
    ).toBeDefined();
    expect(packageRoot.default).toEqual({
      rustic_maps: packageRoot.rustic_maps,
    });
  });

  test('exports the typed time-to-live refusal from the root', () => {
    expect(packageRoot.InvalidTileCacheTimeToLiveError).toBeDefined();
  });
});
