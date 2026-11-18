#!/usr/bin/env node
// Guards the generated WebAssembly package against staleness and version
// drift. `scripts/build-wasm.sh` writes `platform/web/pkg/SOURCE_FINGERPRINT`
// after every emit; this check recomputes the same hash over the Rust inputs
// tracked in git and refuses a package built from different sources. It also
// asserts the emitted module set and the npm manifest's version against the
// workspace version, so a published tarball can never claim a version the
// workspace does not carry.
//
// Usage:
//   node scripts/check-wasm-package-freshness.mjs           # verify
//   node scripts/check-wasm-package-freshness.mjs --write   # record (build)
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPOSITORY_ROOT = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);
const PACKAGE_DIRECTORY = path.join(REPOSITORY_ROOT, 'platform', 'web', 'pkg');
/** Where the build records the fingerprint of the sources it was made from. */
export const SOURCE_FINGERPRINT_FILE_NAME = 'SOURCE_FINGERPRINT';
/** The files wasm-bindgen emits for the `web` target, in build order. */
const EMITTED_PACKAGE_FILE_NAMES = [
  'rustic_maps_web_bindings.js',
  'rustic_maps_web_bindings.d.ts',
  'rustic_maps_web_bindings_bg.wasm',
  'rustic_maps_web_bindings_bg.wasm.d.ts',
];
/**
 * The emitted module's size budget. The optimized module sits near 4.2 MB;
 * the budget leaves room for growth while still failing on an accidental
 * unoptimized emit or a dependency that drags in an unexpected payload.
 */
const MAXIMUM_WASM_MODULE_BYTES = 8 * 1024 * 1024;

/** The npm manifest whose version must equal the workspace version. */
const NPM_MANIFEST_PATH = path.join(
  REPOSITORY_ROOT,
  'platform',
  'web',
  'npm',
  'package.json',
);

/**
 * Lists the git-tracked Rust inputs that can change the emitted module: the
 * workspace manifests, every crate manifest and source file, and build
 * scripts. Sorted so the hash never depends on filesystem order.
 */
function trackedRustInputPaths() {
  const tracked_paths = execFileSync(
    'git',
    [
      '-C',
      REPOSITORY_ROOT,
      'ls-files',
      '-z',
      '--',
      'Cargo.lock',
      'Cargo.toml',
      'crates',
    ],
    { encoding: 'utf8' },
  ).split('\0');
  return tracked_paths
    .filter((tracked_path) => tracked_path.length > 0)
    .filter(
      (tracked_path) =>
        tracked_path.endsWith('.rs') ||
        tracked_path.endsWith('/Cargo.toml') ||
        tracked_path.endsWith('/build.rs') ||
        tracked_path === 'Cargo.toml' ||
        tracked_path === 'Cargo.lock',
    )
    .sort();
}

/**
 * Hashes every tracked Rust input by path and content, so a rename, a
 * deletion and an edit each change the result.
 *
 * @returns {string} the lower-case hexadecimal SHA-256 digest.
 */
export function computeWasmSourceFingerprint() {
  const digest = createHash('sha256');
  for (const relative_path of trackedRustInputPaths()) {
    const file_bytes = readFileSync(path.join(REPOSITORY_ROOT, relative_path));
    digest.update(relative_path);
    digest.update('\0');
    digest.update(String(file_bytes.length));
    digest.update('\0');
    digest.update(file_bytes);
  }
  return digest.digest('hex');
}

/** Reads the workspace version from the root `Cargo.toml`. */
function workspaceVersion() {
  const cargo_manifest = readFileSync(
    path.join(REPOSITORY_ROOT, 'Cargo.toml'),
    'utf8',
  );
  const version_line = cargo_manifest.match(/^version = "([^"]+)"$/m);
  if (version_line === null) {
    throw new Error('the root Cargo.toml declares no workspace version');
  }
  return version_line[1];
}

/** Throws when the npm manifest's version differs from the workspace's. */
function assertPackageVersionMatchesWorkspace() {
  const npm_manifest = JSON.parse(readFileSync(NPM_MANIFEST_PATH, 'utf8'));
  const expected_version = workspaceVersion();
  if (npm_manifest.version !== expected_version) {
    throw new Error(
      `the WebAssembly npm manifest declares version ${npm_manifest.version}, ` +
        `the workspace ${expected_version}: align platform/web/npm/package.json`,
    );
  }
}

/**
 * Verifies that `platform/web/pkg` was emitted from the current sources.
 *
 * @throws {Error} when a file is missing, the fingerprint is absent or does
 *   not match, or the emitted module is empty.
 */
export function verifyWasmPackageFreshness() {
  for (const package_file_name of EMITTED_PACKAGE_FILE_NAMES) {
    const package_file_path = path.join(PACKAGE_DIRECTORY, package_file_name);
    if (!existsSync(package_file_path)) {
      throw new Error(
        `the WebAssembly package is incomplete: ${package_file_path} is missing; ` +
          'run scripts/build-wasm.sh',
      );
    }
  }
  const module_path = path.join(
    PACKAGE_DIRECTORY,
    'rustic_maps_web_bindings_bg.wasm',
  );
  const module_size_bytes = statSync(module_path).size;
  if (module_size_bytes === 0) {
    throw new Error(
      `the emitted WebAssembly module ${module_path} is empty; ` +
        'run scripts/build-wasm.sh',
    );
  }
  if (module_size_bytes > MAXIMUM_WASM_MODULE_BYTES) {
    throw new Error(
      `the emitted WebAssembly module is ${module_size_bytes} bytes, above the ` +
        `${MAXIMUM_WASM_MODULE_BYTES} byte budget: run scripts/build-wasm.sh ` +
        'with wasm-opt available',
    );
  }
  const fingerprint_path = path.join(
    PACKAGE_DIRECTORY,
    SOURCE_FINGERPRINT_FILE_NAME,
  );
  if (!existsSync(fingerprint_path)) {
    throw new Error(
      `the WebAssembly package records no source fingerprint; ` +
        'run scripts/build-wasm.sh',
    );
  }
  const recorded_fingerprint = readFileSync(fingerprint_path, 'utf8').trim();
  const current_fingerprint = computeWasmSourceFingerprint();
  if (recorded_fingerprint !== current_fingerprint) {
    throw new Error(
      'the WebAssembly package is stale: the Rust sources changed after ' +
        `platform/web/pkg was emitted (${recorded_fingerprint.slice(0, 12)} -> ` +
        `${current_fingerprint.slice(0, 12)}); run scripts/build-wasm.sh`,
    );
  }
  assertPackageVersionMatchesWorkspace();
}

/** Records the current fingerprint beside the emitted module. */
export function recordWasmSourceFingerprint() {
  writeFileSync(
    path.join(PACKAGE_DIRECTORY, SOURCE_FINGERPRINT_FILE_NAME),
    `${computeWasmSourceFingerprint()}\n`,
  );
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  if (process.argv.includes('--write')) {
    recordWasmSourceFingerprint();
    console.log('recorded the WebAssembly source fingerprint');
  } else {
    verifyWasmPackageFreshness();
    console.log('the WebAssembly package matches the current sources');
  }
}
