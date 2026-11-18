#!/usr/bin/env node
// Guards the browser-loadable WebAssembly package (`platform/web/generated`)
// against staleness. `scripts/build-wasm2-package.sh` writes a fingerprint of
// the inputs that determine the package's contract surface; this check
// recomputes it and refuses a package built from different sources. It also
// asserts the emitted module set and the module's size budget, so a missing
// file or an unoptimized emit fails before a browser spec loads it.
//
// The hashed inputs are the ones a contract change can travel through: the
// frozen UDL, the committed stripped WebAssembly UDL, the binding crate's
// build script that selects the scaffolding, the generation script that
// produces the TypeScript and stages the module, the packaging script, and the
// relative-specifier rewrite the package depends on. A change to any of them
// means the package must be rebuilt.
//
// Usage:
//   node scripts/check-wasm2-package-freshness.mjs           # verify
//   node scripts/check-wasm2-package-freshness.mjs --write   # record (build)
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPOSITORY_ROOT = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);
/** The browser package the build produces; gitignored build output. */
export const PACKAGE_DIRECTORY = path.join(
  REPOSITORY_ROOT,
  'platform',
  'web',
  'generated',
);
/** Where the build records the fingerprint of the inputs it was made from. */
export const SOURCE_FINGERPRINT_FILE_NAME = 'SOURCE_FINGERPRINT';
/** The files the packaging step emits, in the order it writes them. */
const EMITTED_PACKAGE_FILE_NAMES = [
  'index.js',
  'index.d.ts',
  'rustic_maps.js',
  'rustic_maps.d.ts',
  'rustic_maps-ffi.js',
  'rustic_maps-ffi.d.ts',
  'rustic_maps_bg.js',
  'rustic_maps.wasm',
];
/**
 * The emitted module's size budget. The optimized wasm2 module sits near
 * 5.9 MB; the budget leaves room for contract growth while still failing on an
 * accidental unoptimized emit or an unexpected dependency payload.
 */
const MAXIMUM_WASM_MODULE_BYTES = 8 * 1024 * 1024;
/**
 * The inputs whose content determines the emitted package. Paths are
 * repository-relative so the hash never depends on a checkout's location.
 */
const FINGERPRINT_INPUT_PATHS = [
  'crates/rustic-maps-native-core/src/rustic_maps.udl',
  'crates/rustic-maps-native-core/src/rustic_maps_wasm.udl',
  'crates/rustic-maps-native-core/build.rs',
  'scripts/generate-wasm-bindings.sh',
  'scripts/build-wasm2-package.sh',
  'scripts/add-generated-esm-extensions.mjs',
];

/**
 * Hashes every input by path and content, so an edit, rename or deletion each
 * changes the result.
 *
 * @returns {string} the lower-case hexadecimal SHA-256 digest.
 */
export function computeWasm2SourceFingerprint() {
  const digest = createHash('sha256');
  for (const relativePath of FINGERPRINT_INPUT_PATHS) {
    const filePath = path.join(REPOSITORY_ROOT, relativePath);
    if (!existsSync(filePath)) {
      throw new Error(`the fingerprint input ${relativePath} is missing`);
    }
    const fileBytes = readFileSync(filePath);
    digest.update(relativePath);
    digest.update('\0');
    digest.update(String(fileBytes.length));
    digest.update('\0');
    digest.update(fileBytes);
  }
  return digest.digest('hex');
}

/**
 * Verifies that `platform/web/generated` was packaged from the current
 * sources.
 *
 * @throws {Error} when a file is missing, the fingerprint is absent or does
 *   not match, or the module is empty or above its size budget.
 */
export function verifyWasm2PackageFreshness() {
  for (const fileName of EMITTED_PACKAGE_FILE_NAMES) {
    const filePath = path.join(PACKAGE_DIRECTORY, fileName);
    if (!existsSync(filePath)) {
      throw new Error(
        `the generated browser package is incomplete: ${filePath} is missing; ` +
          'run scripts/build-wasm2-package.sh',
      );
    }
  }
  const modulePath = path.join(PACKAGE_DIRECTORY, 'rustic_maps.wasm');
  const moduleSizeBytes = statSync(modulePath).size;
  if (moduleSizeBytes === 0) {
    throw new Error(
      `the emitted WebAssembly module ${modulePath} is empty; ` +
        'run scripts/build-wasm2-package.sh',
    );
  }
  if (moduleSizeBytes > MAXIMUM_WASM_MODULE_BYTES) {
    throw new Error(
      `the emitted WebAssembly module is ${moduleSizeBytes} bytes, above the ` +
        `${MAXIMUM_WASM_MODULE_BYTES} byte budget: run ` +
        'scripts/build-wasm2-package.sh with wasm-opt available',
    );
  }
  const fingerprintPath = path.join(
    PACKAGE_DIRECTORY,
    SOURCE_FINGERPRINT_FILE_NAME,
  );
  if (!existsSync(fingerprintPath)) {
    throw new Error(
      'the generated browser package records no source fingerprint; ' +
        'run scripts/build-wasm2-package.sh',
    );
  }
  const recordedFingerprint = readFileSync(fingerprintPath, 'utf8').trim();
  const currentFingerprint = computeWasm2SourceFingerprint();
  if (recordedFingerprint !== currentFingerprint) {
    throw new Error(
      'the generated browser package is stale: its generation inputs changed ' +
        `after it was packaged (${recordedFingerprint.slice(0, 12)} -> ` +
        `${currentFingerprint.slice(0, 12)}); run scripts/build-wasm2-package.sh`,
    );
  }
}

/** Records the current fingerprint beside the emitted package. */
export function recordWasm2SourceFingerprint() {
  writeFileSync(
    path.join(PACKAGE_DIRECTORY, SOURCE_FINGERPRINT_FILE_NAME),
    `${computeWasm2SourceFingerprint()}\n`,
  );
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  if (process.argv.includes('--write')) {
    recordWasm2SourceFingerprint();
    console.log('recorded the generated browser package source fingerprint');
  } else {
    verifyWasm2PackageFreshness();
    console.log('the generated browser package matches the current sources');
  }
}
