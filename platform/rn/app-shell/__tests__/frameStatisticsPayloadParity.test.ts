/**
 * Frame-statistics payload parity between the two native hosts.
 *
 * `MapView.tsx` reads the frame accounting from `nativeEvent.statistics`, so
 * both emitters must deliver the same six fields nested under that key. The
 * Android emitter once wrote the fields at the payload's top level, which
 * made `onFrameStatistics` silently never fire for React Native consumers.
 * This fixture pins the two source emitters to one shape.
 */

import fs from 'node:fs';
import path from 'node:path';

/** The six accounting fields the `FrameStatistics` contract carries. */
const FRAME_STATISTICS_FIELD_NAMES = [
  'requestedTileCount',
  'presentTileCount',
  'fallbackTileCount',
  'deferredTileCount',
  'unavailableTileCount',
  'overlayTriangleCount',
];

/** `platform/rn`, the package root the app-shell tests live under. */
const packageRoot = path.resolve(__dirname, '..', '..');

function readPackageSource(relativePath: string): string {
  return fs.readFileSync(path.join(packageRoot, relativePath), 'utf8');
}

/** The fields the Android host writes into its nested statistics map. */
function androidFrameStatisticsFieldNames(source: string): string[] {
  const emitterStart = source.indexOf('override fun onFrameStatistics(');
  const emitterEnd = source.indexOf('override fun onFrameDeferred()');
  expect(emitterStart).toBeGreaterThanOrEqual(0);
  expect(emitterEnd).toBeGreaterThan(emitterStart);
  const emitterSource = source.slice(emitterStart, emitterEnd);
  expect(emitterSource).toContain('putMap("statistics"');
  return [...emitterSource.matchAll(/\.putInt\("([A-Za-z]+)"/g)].map(
    match => match[1],
  );
}

/** The fields the iOS host writes into its nested statistics dictionary. */
function iosFrameStatisticsFieldNames(source: string): string[] {
  const emitterStart = source.indexOf('didDrawFrame statistics:');
  const emitterEnd = source.indexOf('func mapViewDidDeferFrame(');
  expect(emitterStart).toBeGreaterThanOrEqual(0);
  expect(emitterEnd).toBeGreaterThan(emitterStart);
  const emitterSource = source.slice(emitterStart, emitterEnd);
  expect(emitterSource).toContain('"statistics": [');
  return [
    ...emitterSource.matchAll(/"([A-Za-z]+)": Int\(statistics\./g),
  ].map(match => match[1]);
}

describe('native onFrameStatistics payload parity', () => {
  test('both emitters carry the same six fields nested under statistics', () => {
    const androidFields = androidFrameStatisticsFieldNames(
      readPackageSource(
        'android/src/main/java/com/mapssdkrn/MapsSdkMapHostView.kt',
      ),
    );
    const iosFields = iosFrameStatisticsFieldNames(
      readPackageSource('ios/MapsSdkMapHostView.swift'),
    );

    expect([...androidFields].sort()).toEqual(
      [...FRAME_STATISTICS_FIELD_NAMES].sort(),
    );
    expect([...iosFields].sort()).toEqual(
      [...FRAME_STATISTICS_FIELD_NAMES].sort(),
    );
  });
});
