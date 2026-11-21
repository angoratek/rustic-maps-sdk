// Tests for scripts/README.md.
//
// Run with: node --test scripts/scripts-index.test.mjs
//
// The index is the reason this directory does not accumulate one-off tools:
// every script names its role and who runs it, and a new file fails these
// tests until it is classified (or deleted).

import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';

/** The directory holding this test and the scripts it indexes. */
const SCRIPTS_DIRECTORY = fileURLToPath(new URL('.', import.meta.url));

/** The index document beside the scripts. */
const INDEX_FILE_NAME = 'README.md';

/**
 * Lists every file below the scripts directory, as POSIX-style relative
 * paths. The index itself is not a script.
 *
 * @param {string} directory the directory to walk.
 * @param {string} [prefix] the relative path walked so far.
 * @returns {string[]} the sorted relative paths.
 */
function listScriptFiles(directory, prefix = '') {
  const files = [];
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = prefix === '' ? entry.name : `${prefix}/${entry.name}`;
    if (entry.isDirectory()) {
      files.push(...listScriptFiles(join(directory, entry.name), path));
    } else if (path !== INDEX_FILE_NAME) {
      files.push(path);
    }
  }
  return files.sort();
}

/**
 * Reads the script names the index's table classifies.
 *
 * A row's first cell holds exactly one backticked path; the role and runner
 * follow.
 *
 * @param {string} indexPath the index document.
 * @returns {string[]} the classified paths, sorted.
 */
function readIndexedScriptNames(indexPath) {
  const names = [];
  for (const line of readFileSync(indexPath, 'utf8').split('\n')) {
    const match = /^\| `([^`]+)` \|/.exec(line);
    if (match) {
      names.push(match[1]);
    }
  }
  return names.sort();
}

test('every script is classified in the index', () => {
  const indexed = new Set(readIndexedScriptNames(join(SCRIPTS_DIRECTORY, INDEX_FILE_NAME)));
  const unclassified = listScriptFiles(SCRIPTS_DIRECTORY).filter((path) => !indexed.has(path));
  assert.deepEqual(
    unclassified,
    [],
    `classify these scripts in scripts/README.md or delete them: ${unclassified.join(', ')}`,
  );
});

test('the index names no script that is gone', () => {
  const present = new Set(listScriptFiles(SCRIPTS_DIRECTORY));
  const stale = readIndexedScriptNames(join(SCRIPTS_DIRECTORY, INDEX_FILE_NAME)).filter(
    (name) => !present.has(name),
  );
  assert.deepEqual(
    stale,
    [],
    `remove these rows from scripts/README.md: ${stale.join(', ')}`,
  );
});

test('the index classifies each script once', () => {
  const names = readIndexedScriptNames(join(SCRIPTS_DIRECTORY, INDEX_FILE_NAME));
  const duplicates = names.filter((name, index) => names.indexOf(name) !== index);
  assert.deepEqual(duplicates, [], `each script appears once in the index`);
  assert.ok(
    names.length > 1,
    `the index must classify more than ${names.length} scripts`,
  );
});
