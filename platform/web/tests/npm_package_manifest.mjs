// Packaging assertion (`docs/PackagingAndRelease.md` §2.4): the published
// `@angoratek/rustic-maps-sdk-web` tarball must carry the generated `wasm2`
// ESM set with its `.d.ts` declarations, the hand-written canvas declarations
// published beside the glue, and the licenses and notices. The manifest's
// `files` and `exports` are the contract; this check stages the package the
// way `scripts/package-web-npm.sh` stages it, asks `npm pack --dry-run` for
// the real member list, and fails when a named file is not in it.
//
// The check needs the generated package (`scripts/build-wasm2-package.sh`);
// the Playwright `globalSetup` runs it after the freshness gate.
//
// Run directly (no server or browser involved):
//   node --test platform/web/tests/npm_package_manifest.mjs

import { strict as assert } from 'node:assert';
import { execFileSync } from 'node:child_process';
import {
  cpSync,
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
} from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath, pathToFileURL } from 'node:url';

const TEST_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const REPOSITORY_ROOT = path.resolve(TEST_DIRECTORY, '..', '..', '..');
const WEB_ROOT = path.join(REPOSITORY_ROOT, 'platform', 'web');
const GENERATED_PACKAGE_DIRECTORY = path.join(WEB_ROOT, 'generated');
const NPM_PACKAGE_DIRECTORY = path.join(WEB_ROOT, 'npm');
const CANVAS_TYPES_PATH = path.join(WEB_ROOT, 'types', 'browser_canvas.d.ts');
const PACKAGE_MANIFEST_PATH = path.join(NPM_PACKAGE_DIRECTORY, 'package.json');
const README_PATH = path.join(NPM_PACKAGE_DIRECTORY, 'README.md');
const MIT_LICENSE_PATH = path.join(REPOSITORY_ROOT, 'LICENSE-MIT');
const APACHE_LICENSE_PATH = path.join(REPOSITORY_ROOT, 'LICENSE-APACHE');
const NOTICES_PATH = path.join(REPOSITORY_ROOT, 'docs', 'THIRD_PARTY_NOTICES.md');

/** The generated ESM set and declarations the packaging step emits. */
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

/** The hand-written canvas declarations publish under the glue's own name. */
const CANVAS_TYPES_FILE_NAME = 'rustic_maps_bg.d.ts';

/** The declarations a missing file in the tarball would break. */
const REQUIRED_DECLARATION_FILE_NAMES = [
  'index.d.ts',
  'rustic_maps.d.ts',
  CANVAS_TYPES_FILE_NAME,
];

/** Every file the manifest's `files` list commits to publishing. */
const REQUIRED_PACKAGE_FILE_NAMES = [
  ...EMITTED_PACKAGE_FILE_NAMES,
  CANVAS_TYPES_FILE_NAME,
  'README.md',
  'LICENSE-MIT',
  'LICENSE-APACHE',
  'THIRD_PARTY_NOTICES.md',
];

/**
 * Asserts the npm manifest names the declarations and points its type
 * subpaths at them, so `npm pack` can never silently omit either group.
 * @param {object} manifest The parsed `platform/web/npm/package.json`.
 */
function assert_manifest_names_published_files(manifest) {
  const listed_files = new Set(manifest.files ?? []);
  for (const required_file_name of REQUIRED_PACKAGE_FILE_NAMES) {
    assert(
      listed_files.has(required_file_name),
      `the npm manifest's "files" list must carry "${required_file_name}",`
      + ' or the published tarball drops it.',
    );
  }
  assert.strictEqual(
    manifest.exports?.['.']?.types,
    './index.d.ts',
    'the npm manifest must type the package root with the generated entrypoint',
  );
  assert.strictEqual(
    manifest.exports?.['./rustic_maps']?.types,
    './rustic_maps.d.ts',
    'the npm manifest must expose the contract declarations through'
    + ' exports["./rustic_maps"].types.',
  );
  assert.strictEqual(
    manifest.exports?.['./rustic_maps_bg.js']?.types,
    `./${CANVAS_TYPES_FILE_NAME}`,
    'the npm manifest must type the wasm-bindgen glue with the canvas'
    + ' declarations.',
  );
  assert.strictEqual(
    manifest.types,
    './index.d.ts',
    'the npm manifest "types" entry must name the generated declarations.',
  );
}

/**
 * Builds the package tree `scripts/package-web-npm.sh` packs: the generated
 * ESM set and declarations, the committed manifest and README, the canvas
 * declarations, and the license and notice files.
 * @param {string} staging_directory The empty staging directory.
 */
function stage_package(staging_directory) {
  for (const emitted_file_name of EMITTED_PACKAGE_FILE_NAMES) {
    const emitted_path = path.join(GENERATED_PACKAGE_DIRECTORY, emitted_file_name);
    assert(
      existsSync(emitted_path),
      `the generated package is missing "${emitted_file_name}"; run`
      + ' scripts/build-wasm2-package.sh before packaging.',
    );
    cpSync(emitted_path, path.join(staging_directory, emitted_file_name));
  }
  cpSync(PACKAGE_MANIFEST_PATH, path.join(staging_directory, 'package.json'));
  cpSync(README_PATH, path.join(staging_directory, 'README.md'));
  cpSync(CANVAS_TYPES_PATH, path.join(staging_directory, CANVAS_TYPES_FILE_NAME));
  cpSync(MIT_LICENSE_PATH, path.join(staging_directory, 'LICENSE-MIT'));
  cpSync(APACHE_LICENSE_PATH, path.join(staging_directory, 'LICENSE-APACHE'));
  cpSync(NOTICES_PATH, path.join(staging_directory, 'THIRD_PARTY_NOTICES.md'));
}

/**
 * Asks npm for the packed member list of the staged tree.
 * @param {string} staging_directory The staged package tree.
 * @returns {Set<string>} The member paths npm would publish.
 */
function packed_member_paths(staging_directory) {
  const pack_output = execFileSync(
    'npm',
    ['pack', '--dry-run', '--json'],
    { cwd: staging_directory, encoding: 'utf8' },
  );
  const [packed_package] = JSON.parse(pack_output);
  return new Set((packed_package.files ?? []).map((file) => file.path));
}

/**
 * Runs the packaging assertions; throws on the first missing file.
 * Shared by the `node --test` registration and the Playwright `globalSetup`.
 */
function run_npm_package_manifest_checks() {
  const manifest = JSON.parse(readFileSync(PACKAGE_MANIFEST_PATH, 'utf8'));
  assert_manifest_names_published_files(manifest);

  const staging_directory = mkdtempSync(
    path.join(os.tmpdir(), 'rustic-maps-npm-package-'),
  );
  try {
    stage_package(staging_directory);
    const member_paths = packed_member_paths(staging_directory);
    for (const required_declaration_file_name of REQUIRED_DECLARATION_FILE_NAMES) {
      assert(
        member_paths.has(required_declaration_file_name),
        `the packed tarball misses "${required_declaration_file_name}";`
        + ' a type consumer would lose the declarations.',
      );
    }
    for (const listed_file_name of manifest.files) {
      assert(
        member_paths.has(listed_file_name),
        `the npm manifest lists "${listed_file_name}", but npm pack omits it`
        + ' from the tarball; stage it or drop the entry.',
      );
    }
    assert(
      member_paths.has('package.json'),
      'the packed tarball must carry package.json.',
    );
  } finally {
    rmSync(staging_directory, { recursive: true, force: true });
  }
}

export { run_npm_package_manifest_checks };

/** The `node --test` suite name of the packaging assertions. */
const NPM_PACKAGE_MANIFEST_TEST_NAME =
  'the packed npm tarball carries the generated and wasm-bindgen declarations';

// Run the suite only when this file is the entry point; the Playwright
// `globalSetup` imports the exported runner instead.
const RUNNING_AS_ENTRY = process.argv[1] !== undefined
  && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url;

if (RUNNING_AS_ENTRY) {
  test(NPM_PACKAGE_MANIFEST_TEST_NAME, () => {
    run_npm_package_manifest_checks();
  });
}
