// Tests for scripts/mirror-docs-site-tree.mjs.
//
// Run with: node --test scripts/mirror-docs-site-tree.test.mjs
//
// The cases pin the rule the docs publisher needs: a staged tree without a
// complete artifact set never deletes the pages checkout's downloads, a
// complete set replaces them, and every deleted path is reported.

import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';

import {
  inspectDownloadsSet,
  listRelativeFiles,
  mirrorSiteTree,
  planDownloadsPreservation,
} from './mirror-docs-site-tree.mjs';

/** A SHA-256 of the empty string, 64 hex digits, for a checksum fixture. */
const EMPTY_SHA256 = 'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855';

/**
 * Creates a scratch directory and removes it when the test ends.
 *
 * @param {import('node:test').TestContext} context the running test.
 * @returns {string} the directory path.
 */
function scratchDirectory(context) {
  const directory = mkdtempSync(join(tmpdir(), 'docs-mirror-test-'));
  context.after(() => rmSync(directory, { recursive: true, force: true }));
  return directory;
}

/**
 * Creates a pages checkout with one published version and a page.
 *
 * @param {string} root the checkout directory.
 * @returns {void}
 */
function createPagesCheckout(root) {
  mkdirSync(join(root, '.git'), { recursive: true });
  writeFileSync(join(root, '.git', 'HEAD'), 'ref: refs/heads/gh-pages\n');
  writeFileSync(join(root, 'index.html'), '<p>published</p>\n');
  mkdirSync(join(root, 'downloads', 'v0.1.0'), { recursive: true });
  writeFileSync(join(root, 'downloads', 'v0.1.0', 'map-sdk-release.aar'), 'aar');
  writeFileSync(
    join(root, 'downloads', 'v0.1.0', 'SHA256SUMS.txt'),
    `${EMPTY_SHA256}  map-sdk-release.aar\n`,
  );
}

/**
 * Creates a staged site with a page and, when asked, a downloads set.
 *
 * @param {string} root the staged site directory.
 * @param {{ downloads?: { version: string, artifacts: string[], checksums?: boolean } }} [options]
 *   the downloads set to stage.
 * @returns {void}
 */
function createStagedSite(root, options = {}) {
  mkdirSync(root, { recursive: true });
  writeFileSync(join(root, 'index.html'), '<p>staged</p>\n');
  if (options.downloads === undefined) {
    return;
  }
  const versionDirectory = join(root, 'downloads', options.downloads.version);
  mkdirSync(versionDirectory, { recursive: true });
  for (const artifact of options.downloads.artifacts) {
    writeFileSync(join(versionDirectory, artifact), `payload of ${artifact}`);
  }
  if (options.downloads.checksums !== false) {
    const checksums = options.downloads.artifacts
      .map((artifact) => `${EMPTY_SHA256}  ${artifact}`)
      .join('\n');
    writeFileSync(join(versionDirectory, 'SHA256SUMS.txt'), `${checksums}\n`);
  }
}

test('a staged tree without downloads preserves the published artifacts', (context) => {
  const root = scratchDirectory(context);
  const staged = join(root, 'staged');
  const pages = join(root, 'pages');
  createStagedSite(staged);
  createPagesCheckout(pages);

  const result = mirrorSiteTree({ stagedSiteDirectory: staged, pagesRepositoryDirectory: pages });

  assert.equal(result.downloads.preserve, true);
  assert.match(result.downloads.reason, /carries no downloads directory/);
  assert.equal(
    readFileSync(join(pages, 'downloads', 'v0.1.0', 'map-sdk-release.aar'), 'utf8'),
    'aar',
  );
  assert.deepEqual(result.removedFiles, []);
  assert.equal(readFileSync(join(pages, 'index.html'), 'utf8'), '<p>staged</p>\n');
  assert.ok(existsSync(join(pages, '.git', 'HEAD')), 'the checkout keeps its history');
});

test('a complete staged downloads set replaces the published artifacts', (context) => {
  const root = scratchDirectory(context);
  const staged = join(root, 'staged');
  const pages = join(root, 'pages');
  createStagedSite(staged, {
    downloads: { version: 'v0.2.0', artifacts: ['map-sdk-release.aar', 'SHA256SUMS.txt.sig'] },
  });
  createPagesCheckout(pages);

  const result = mirrorSiteTree({ stagedSiteDirectory: staged, pagesRepositoryDirectory: pages });

  assert.equal(result.downloads.preserve, false);
  assert.ok(result.removedFiles.includes('downloads/v0.1.0/map-sdk-release.aar'));
  assert.ok(result.removedFiles.includes('downloads/v0.1.0/SHA256SUMS.txt'));
  assert.ok(!existsSync(join(pages, 'downloads', 'v0.1.0')));
  assert.equal(
    readFileSync(join(pages, 'downloads', 'v0.2.0', 'map-sdk-release.aar'), 'utf8'),
    'payload of map-sdk-release.aar',
  );
});

test('an incomplete staged downloads set keeps the published artifacts', (context) => {
  const root = scratchDirectory(context);
  const staged = join(root, 'staged');
  const pages = join(root, 'pages');
  createStagedSite(staged, {
    downloads: { version: 'v0.2.0', artifacts: ['map-sdk-release.aar'], checksums: false },
  });
  createPagesCheckout(pages);

  const plan = planDownloadsPreservation({
    stagedSiteDirectory: staged,
    pagesRepositoryDirectory: pages,
  });
  assert.equal(plan.preserve, true);
  assert.match(plan.reason, /incomplete/);
  assert.match(plan.reason, /SHA256SUMS\.txt is missing/);

  const result = mirrorSiteTree({ stagedSiteDirectory: staged, pagesRepositoryDirectory: pages });
  assert.ok(existsSync(join(pages, 'downloads', 'v0.1.0', 'map-sdk-release.aar')));
  assert.deepEqual(
    result.removedFiles.filter((path) => path.startsWith('downloads/')),
    [],
  );
});

test('a checksum entry with no file makes the set incomplete', (context) => {
  const root = scratchDirectory(context);
  const downloads = join(root, 'downloads');
  mkdirSync(join(downloads, 'v0.3.0'), { recursive: true });
  writeFileSync(
    join(downloads, 'v0.3.0', 'SHA256SUMS.txt'),
    `${EMPTY_SHA256}  absent.aar\n`,
  );

  const inspection = inspectDownloadsSet(downloads);
  assert.equal(inspection.complete, false);
  assert.deepEqual(inspection.problems, ['v0.3.0/absent.aar is listed but absent']);
});

test('the mirror reports every path it deletes', (context) => {
  const root = scratchDirectory(context);
  const staged = join(root, 'staged');
  const pages = join(root, 'pages');
  createStagedSite(staged);
  createPagesCheckout(pages);
  mkdirSync(join(pages, 'old-section'));
  writeFileSync(join(pages, 'old-section', 'stale.html'), 'stale');

  const result = mirrorSiteTree({ stagedSiteDirectory: staged, pagesRepositoryDirectory: pages });
  assert.deepEqual(result.removedFiles, ['old-section/stale.html']);
  assert.ok(!existsSync(join(pages, 'old-section')));
});

test('the file listing skips the checkout history', (context) => {
  const root = scratchDirectory(context);
  createPagesCheckout(root);
  assert.deepEqual(listRelativeFiles(root, { skipTopLevelGit: true }), [
    'downloads/v0.1.0/SHA256SUMS.txt',
    'downloads/v0.1.0/map-sdk-release.aar',
    'index.html',
  ]);
  assert.ok(listRelativeFiles(root).includes('.git/HEAD'));
});
