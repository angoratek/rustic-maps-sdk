// Contract-sync test: the browser surface is generated from the frozen UniFFI
// contract, so the gate asserts the two facts a generation drift would break.
//
//   1. The committed WebAssembly contract (`src/rustic_maps_wasm.udl`) is
//      exactly the frozen contract (`src/rustic_maps.udl`) minus the native
//      surface `MapView` interface and its doc block. Both the wasm
//      scaffolding and the bindgen consume that one file.
//   2. The generated package (`platform/web/generated`) is the published
//      browser module: it carries the contract entrypoint and namespace, the
//      wasm-bindgen canvas glue with the distinct `BrowserMapView`, the
//      stripped contract (no `MapView` in the signature table), and the
//      workspace version.
//
// Run directly (no server or browser involved); the package must have been
// built by `scripts/build-wasm2-package.sh`:
//   node --test platform/web/tests/contract_sync.mjs
// Playwright also runs it through its `globalSetup`, which builds the package
// first.

import { strict as assert } from 'node:assert';
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath, pathToFileURL } from 'node:url';

const TEST_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const REPOSITORY_ROOT = path.resolve(TEST_DIRECTORY, '..', '..', '..');

const CONTRACT_DIRECTORY = path.join(
  REPOSITORY_ROOT,
  'crates',
  'rustic-maps-native-core',
  'src',
);
const FROZEN_CONTRACT_PATH = path.join(CONTRACT_DIRECTORY, 'rustic_maps.udl');
const WASM_CONTRACT_PATH = path.join(CONTRACT_DIRECTORY, 'rustic_maps_wasm.udl');
const PACKAGE_DIRECTORY = path.join(REPOSITORY_ROOT, 'platform', 'web', 'generated');
const NPM_MANIFEST_PATH = path.join(REPOSITORY_ROOT, 'platform', 'web', 'npm', 'package.json');
const WORKSPACE_MANIFEST_PATH = path.join(REPOSITORY_ROOT, 'Cargo.toml');

/** The native surface interface the WebAssembly module does not compile. */
const SURFACE_INTERFACE_HEADER = 'interface MapView {';
/** The name the generated contract `MapView` must not reach the browser under. */
const CONTRACT_SURFACE_INTERFACE_NAME = 'MapView';
/** The distinct wasm-bindgen canvas class the page owns. */
const BROWSER_CANVAS_CLASS_NAME = 'BrowserMapView';

/**
 * Returns the frozen contract with the native surface interface, and the
 * contiguous `///` documentation directly above it, removed: the split the
 * committed WebAssembly contract records. Mirrors the Rust split test.
 * @param {string} frozen_contract The frozen contract source.
 * @returns {string}
 */
function frozen_contract_without_surface_interface(frozen_contract) {
  // `str::lines()` in the Rust split test drops the final trailing newline, so
  // the reconstruction appends exactly one.
  const lines = frozen_contract.split('\n');
  if (lines.length > 0 && lines[lines.length - 1] === '') {
    lines.pop();
  }
  const header_indexes = lines
    .map((line, index) => (line.trim() === SURFACE_INTERFACE_HEADER ? index : -1))
    .filter((index) => index >= 0);
  assert.equal(
    header_indexes.length,
    1,
    `the frozen contract must carry exactly one \`${SURFACE_INTERFACE_HEADER}\` interface`,
  );
  const header_index = header_indexes[0];

  let start_index = header_index;
  while (start_index > 0 && lines[start_index - 1].trimStart().startsWith('///')) {
    start_index -= 1;
  }

  let depth = 0;
  let end_index = -1;
  for (let line_index = header_index; line_index < lines.length; line_index += 1) {
    const line = lines[line_index];
    depth += (line.match(/{/g) ?? []).length;
    depth -= (line.match(/}/g) ?? []).length;
    if (depth === 0) {
      end_index = line_index;
      break;
    }
  }
  assert.ok(end_index >= 0, `the \`${SURFACE_INTERFACE_HEADER}\` interface must close`);

  const stripped = [];
  for (let line_index = 0; line_index < lines.length; line_index += 1) {
    if (line_index >= start_index && line_index <= end_index) {
      continue;
    }
    stripped.push(lines[line_index]);
  }
  return `${stripped.join('\n')}\n`;
}

/** The workspace version from the root manifest. */
function workspace_version() {
  const version_match = readFileSync(WORKSPACE_MANIFEST_PATH, 'utf8')
    .match(/^version = "([^"]+)"$/m);
  assert.ok(version_match !== null, 'the workspace manifest declares a version');
  return version_match[1];
}

/** Asserts the stripped WebAssembly contract is the frozen contract's split. */
function assert_wasm_contract_is_the_split() {
  const frozen_contract = readFileSync(FROZEN_CONTRACT_PATH, 'utf8');
  const wasm_contract = readFileSync(WASM_CONTRACT_PATH, 'utf8');
  assert.equal(
    wasm_contract,
    frozen_contract_without_surface_interface(frozen_contract),
    'src/rustic_maps_wasm.udl must be regenerated from src/rustic_maps.udl: it is '
      + 'the frozen contract with the `MapView` interface and its doc block removed',
  );
  assert.ok(
    !wasm_contract.includes(SURFACE_INTERFACE_HEADER),
    'the WebAssembly contract must not carry the native surface interface',
  );
  for (const declaration of ['interface MapSdk', 'interface MapCamera', 'interface Navigator']) {
    assert.ok(
      wasm_contract.includes(declaration),
      `the WebAssembly contract must keep ${declaration}`,
    );
  }
}

/** Asserts the generated package carries the published browser surface. */
function assert_generated_package_identity() {
  assert.ok(
    existsSync(PACKAGE_DIRECTORY),
    'the generated browser package is missing; run scripts/build-wasm2-package.sh first',
  );
  for (const file_name of [
    'index.js',
    'rustic_maps.js',
    'rustic_maps-ffi.js',
    'rustic_maps_bg.js',
    'rustic_maps.wasm',
  ]) {
    assert.ok(
      existsSync(path.join(PACKAGE_DIRECTORY, file_name)),
      `the generated browser package is missing ${file_name}`,
    );
  }

  const index_text = readFileSync(path.join(PACKAGE_DIRECTORY, 'index.js'), 'utf8');
  assert.ok(
    index_text.includes('uniffiInitAsync'),
    'the generated entrypoint exports uniffiInitAsync',
  );

  const contract_text = readFileSync(path.join(PACKAGE_DIRECTORY, 'rustic_maps.js'), 'utf8');
  for (const surface_fragment of [
    'class MapSdk',
    'class MapCamera',
    'class Navigator',
    'getSdkCompiledVersion',
  ]) {
    assert.ok(
      contract_text.includes(surface_fragment),
      `the generated contract module carries ${surface_fragment}`,
    );
  }

  const canvas_text = readFileSync(path.join(PACKAGE_DIRECTORY, 'rustic_maps_bg.js'), 'utf8');
  assert.ok(
    canvas_text.includes(BROWSER_CANVAS_CLASS_NAME),
    `the wasm-bindgen glue exports the ${BROWSER_CANVAS_CLASS_NAME} canvas class`,
  );

  // The signature table names every contract member the module must export; a
  // `MapView` entry means the generation read the frozen contract instead of
  // the stripped WebAssembly one.
  const signature_table_text = readFileSync(
    path.join(PACKAGE_DIRECTORY, 'rustic_maps-ffi.js'),
    'utf8',
  );
  assert.ok(
    !new RegExp(`\\b${CONTRACT_SURFACE_INTERFACE_NAME}\\b`).test(signature_table_text),
    'the generated signature table must not name the stripped contract MapView',
  );

  const manifest = JSON.parse(readFileSync(NPM_MANIFEST_PATH, 'utf8'));
  assert.equal(manifest.name, '@angoratek/rustic-maps-sdk-web', 'the npm package identity');
  assert.equal(
    manifest.version,
    workspace_version(),
    'the npm package version matches the workspace version',
  );
}

/**
 * Runs every contract-sync comparison; throws on the first drift. Shared by
 * the `node --test` registration and the Playwright `globalSetup`.
 */
export function run_contract_sync_checks() {
  assert_wasm_contract_is_the_split();
  assert_generated_package_identity();
}

/** The `node --test` suite name of the parity assertions. */
const CONTRACT_SYNC_TEST_NAME =
  'the browser surface is generated from the frozen contract';

// Run the suite only when this file is the entry point; the Playwright
// `globalSetup` imports the exported runner instead.
const RUNNING_AS_ENTRY = process.argv[1] !== undefined
  && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url;

if (RUNNING_AS_ENTRY) {
  test(CONTRACT_SYNC_TEST_NAME, () => {
    run_contract_sync_checks();
  });
}
