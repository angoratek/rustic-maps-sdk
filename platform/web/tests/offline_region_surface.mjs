// Offline-region wasm-surface assertion: the WebAssembly build has no disk
// pack store, so neither the curated TypeScript mirror nor the generated
// wasm-bindgen surface may declare or claim a region-download member.
//
// `contract_sync.mjs` is deliberately addition-tolerant: it accepts a UDL
// that grows members the wasm seam does not serve. This test is the explicit
// negative companion the offline-region contract needs — the region members
// may exist on the native contract, but the browser surface must stay
// without them until a disk pack store exists.
//
// The check reads the generated record mirror
// (`platform/web/types/rustic_maps.d.ts`), the generated package declarations
// and runtime when a wasm build is present, and the hand-reviewed wasm export
// sources. A file that is absent on a fresh checkout is reported, never
// silently treated as passing; the generated mirror must always exist.
//
// Run directly:
//   node --test platform/web/tests/offline_region_surface.mjs

import { strict as assert } from 'node:assert';
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const TEST_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const REPOSITORY_ROOT = path.resolve(TEST_DIRECTORY, '..', '..', '..');

/** The generated TypeScript mirror of the contract records. */
const CURATED_MIRROR_PATH = path.join(
  REPOSITORY_ROOT, 'platform', 'web', 'types', 'rustic_maps.d.ts',
);

/**
 * The generated wasm-bindgen package files, when a wasm build exists. They
 * are build artifacts (`platform/web/pkg/` is gitignored), so their absence
 * is expected on a fresh checkout and is reported by the test name.
 */
const GENERATED_WASM_PATHS = [
  path.join(REPOSITORY_ROOT, 'platform', 'web', 'pkg', 'rustic_maps_web_bindings.d.ts'),
  path.join(REPOSITORY_ROOT, 'platform', 'web', 'pkg', 'rustic_maps_web_bindings.js'),
];

/** The hand-reviewed `wasm_bindgen` export sources. */
const WASM_EXPORT_SOURCES = [
  path.join(REPOSITORY_ROOT, 'crates', 'rustic-maps-web-bindings', 'src', 'exports.rs'),
  path.join(REPOSITORY_ROOT, 'crates', 'rustic-maps-web-bindings', 'src', 'navigation.rs'),
  path.join(REPOSITORY_ROOT, 'crates', 'rustic-maps-web-bindings', 'src', 'canvas.rs'),
  path.join(REPOSITORY_ROOT, 'crates', 'rustic-maps-web-bindings', 'src', 'live_providers.rs'),
  path.join(REPOSITORY_ROOT, 'crates', 'rustic-maps-web-bindings', 'src', 'diagnostics.rs'),
];

/**
 * Every spelling of a region-download member or record. The snake_case names
 * are the contract identifiers; the lower-camel forms are the JavaScript
 * surface's convention; the PascalCase names are the record types.
 */
const FORBIDDEN_REGION_SPELLINGS = [
  'region_tile_count',
  'regionTileCount',
  'download_region',
  'downloadRegion',
  'cancel_region_download',
  'cancelRegionDownload',
  'region_records',
  'regionRecords',
  'region_record',
  'regionRecord',
  'delete_region',
  'deleteRegion',
  'RegionDefinition',
  'RegionRecord',
  'RegionDownloadProgress',
  'RegionDownloadPhase',
  'RegionRemovalReport',
  'RegionDownloadProgressListener',
  'on_region_download_progress',
  'onRegionDownloadProgress',
];

/**
 * Builds a whole-word matcher so `RegionRecord` does not match a longer
 * identifier and `download_region` does not match `download_region_state`.
 * @param {string} spelling
 * @returns {RegExp}
 */
function whole_word_pattern(spelling) {
  return new RegExp(`(?<![A-Za-z0-9_])${spelling}(?![A-Za-z0-9_])`);
}

/**
 * Returns every forbidden spelling present in a source text.
 * @param {string} source_text
 * @returns {string[]}
 */
function forbidden_spellings_in(source_text) {
  return FORBIDDEN_REGION_SPELLINGS.filter((spelling) =>
    whole_word_pattern(spelling).test(source_text),
  );
}

test('the generated TypeScript mirror declares no offline-region member', () => {
  assert.ok(
    existsSync(CURATED_MIRROR_PATH),
    `the generated mirror is missing: ${CURATED_MIRROR_PATH}`,
  );
  const mirror_text = readFileSync(CURATED_MIRROR_PATH, 'utf8');
  // Positive control: the file must be the real mirror, so an empty or
  // renamed file cannot make the negative assertion pass vacuously.
  assert.match(mirror_text, /offlineRouteFromArchiveBytes|BoundingBox/);
  assert.deepEqual(forbidden_spellings_in(mirror_text), []);
});

test('a generated wasm package declares no offline-region member', (test_context) => {
  const present_paths = GENERATED_WASM_PATHS.filter((candidate) => existsSync(candidate));
  if (present_paths.length === 0) {
    test_context.diagnostic(
      'no generated wasm package is present; the generated-mirror and export-source assertions still apply',
    );
    return;
  }
  for (const source_path of present_paths) {
    const source_text = readFileSync(source_path, 'utf8');
    assert.deepEqual(
      forbidden_spellings_in(source_text),
      [],
      `${path.relative(REPOSITORY_ROOT, source_path)} must not declare an offline-region member`,
    );
  }
});

test('the wasm export sources serve no offline-region member', () => {
  const present_sources = WASM_EXPORT_SOURCES.filter((candidate) => existsSync(candidate));
  assert.ok(present_sources.length > 0, 'no wasm export source was found');
  for (const source_path of present_sources) {
    const source_text = readFileSync(source_path, 'utf8');
    // Positive control: every file in the list is a real `wasm_bindgen`
    // export source, so the negative assertion cannot pass on a stray file.
    assert.match(source_text, /wasm_bindgen/);
    assert.deepEqual(
      forbidden_spellings_in(source_text),
      [],
      `${path.relative(REPOSITORY_ROOT, source_path)} must not export an offline-region member`,
    );
  }
});
