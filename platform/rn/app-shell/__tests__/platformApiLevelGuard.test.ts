/**
 * Android API-floor guard for the tile-cache path.
 *
 * The React Native package supports the AAR's API 24 floor. `java.nio.file`
 * arrived in API 26, so `Paths` on the tile-cache path class-loads only on
 * API 26+ and throws `NoClassDefFoundError` on API 24/25. The runtime builds
 * the cache path with `java.io.File`, which exists on every API level and
 * needs no core-library desugaring. This pins the source so the regression
 * cannot return unnoticed.
 */

import fs from 'node:fs';
import path from 'node:path';

/** `platform/rn`, the package root the app-shell tests live under. */
const packageRoot = path.resolve(__dirname, '..', '..');
const runtimeSourcePath = path.join(
  packageRoot,
  'android',
  'src',
  'main',
  'java',
  'com',
  'mapssdkrn',
  'MapsSdkNativeRuntime.kt',
);

describe('Android API 24/25 tile-cache path', () => {
  test('the runtime builds the cache path with java.io.File, not java.nio.file.Paths', () => {
    const runtimeSource = fs.readFileSync(runtimeSourcePath, 'utf8');

    expect(runtimeSource).not.toContain('java.nio.file.Paths');
    expect(runtimeSource).not.toContain('java.nio.file');
    expect(runtimeSource).toContain('import java.io.File');
    expect(runtimeSource).toContain('TILE_CACHE_DATABASE_FILE_NAME');
    // The cache database path is the only File construction on this path.
    expect(runtimeSource).toMatch(
      /File\(\s*cacheDirectory,\s*TILE_CACHE_DATABASE_FILE_NAME,\s*\)\.absolutePath/,
    );
  });
});
