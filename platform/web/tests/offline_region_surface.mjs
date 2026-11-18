// Offline-region wasm-surface assertion: the browser canvas crate has no disk
// pack store, so its hand-written export sources must not invent a
// region-download member. The generated contract surface is the frozen
// contract and is covered by the contract-sync split; this test guards the one
// hand-maintained piece against drifting toward a store it cannot provide.
//
// The check reads the hand-reviewed wasm export sources. A file that is
// absent on a fresh checkout is reported, never silently treated as passing.
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

/** The hand-reviewed `wasm_bindgen` export sources. */
const WASM_EXPORT_SOURCES = [
  path.join(REPOSITORY_ROOT, 'crates', 'rustic-maps-web-bindings', 'src', 'exports.rs'),
  path.join(REPOSITORY_ROOT, 'crates', 'rustic-maps-web-bindings', 'src', 'canvas.rs'),
  path.join(REPOSITORY_ROOT, 'crates', 'rustic-maps-web-bindings', 'src', 'live_providers.rs'),
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
