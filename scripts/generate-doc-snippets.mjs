#!/usr/bin/env node
// Tutorial snippet generator (PLAN.md, M25).
//
// Every fenced code block in the tutorial chapters is regenerated from a
// marked region of a reference application, so a guide cannot drift from the
// code that ships. `docs/Release.md` is the contract; this file
// implements it.
//
// A source region is delimited by two comments carrying the snippet
// identifier:
//
//   // docs-snippet: begin search-and-routing/plan-route
//   ...
//   // docs-snippet: end search-and-routing/plan-route
//
// A chapter carries the identifier and the platform in HTML comments, and the
// generator rewrites everything between them:
//
//   <!-- docs-snippet: begin search-and-routing/plan-route android -->
//   ```kotlin
//   ...
//   ```
//   <!-- docs-snippet: end search-and-routing/plan-route android -->
//
// Prose stays authored; only the fenced body is generated.
//
// USAGE
//   node scripts/generate-doc-snippets.mjs
//   node scripts/generate-doc-snippets.mjs --check
//   node scripts/generate-doc-snippets.mjs --self-test
//
// OPTIONS
//   --check       Report drift and exit non-zero without writing.
//   --self-test   Prove the generator detects drift and orphaned regions.
//   --manifest <path>  Snippet manifest (default `docs/doc-snippets.json`).
//   --verbose     Print every region written and every file touched.

import { existsSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const SCRIPT_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(SCRIPT_DIRECTORY, '..');

/** The identifier grammar shared by source markers and chapter markers. */
const SNIPPET_IDENTIFIER_PATTERN = /^[a-z0-9]+(?:-[a-z0-9]+)*\/[a-z0-9]+(?:-[a-z0-9]+)*$/;

/** A source marker comment, e.g. `// docs-snippet: begin first-map/create-sdk`. */
const SOURCE_MARKER_PATTERN = /^\s*\/\/\s*docs-snippet:\s*(begin|end)\s+(\S+)\s*$/;

/** A chapter begin marker, e.g. `<!-- docs-snippet: begin first-map/create-sdk android -->`. */
const CHAPTER_BEGIN_PATTERN = /^\s*<!--\s*docs-snippet:\s*begin\s+(\S+)\s+(\S+)\s*-->\s*$/;

/** A chapter end marker, matching its begin marker's identifier and platform. */
const CHAPTER_END_PATTERN = /^\s*<!--\s*docs-snippet:\s*end\s+(\S+)\s+(\S+)\s*-->\s*$/;

/** The fence language each source extension is published under. */
const LANGUAGE_BY_EXTENSION = new Map([
  ['.kt', 'kotlin'],
  ['.swift', 'swift'],
  ['.ts', 'ts'],
  ['.tsx', 'ts'],
  ['.js', 'js'],
  ['.mjs', 'js'],
  ['.jsx', 'js'],
]);

/** Directories never scanned for source markers. */
const SCAN_EXCLUDED_DIRECTORY_NAMES = new Set([
  '.git',
  'build',
  'DerivedData',
  'node_modules',
  'Pods',
  'target',
]);

/** The default manifest path, relative to the repository root. */
const DEFAULT_MANIFEST_PATH = 'docs/doc-snippets.json';

function usage() {
  process.stdout.write(
    'usage: node scripts/generate-doc-snippets.mjs [--check] [--self-test] ' +
      '[--manifest <path>] [--verbose]\n',
  );
}

function fail(message) {
  process.stderr.write(`error: ${message}\n`);
  process.exit(2);
}

// ---------------------------------------------------------------------------
// Manifest
// ---------------------------------------------------------------------------

/**
 * Reads the snippet manifest: the platform roots to scan and the chapters to
 * rewrite. Throws with a precise message so a malformed manifest cannot be
 * mistaken for "nothing to do".
 */
function readManifest(manifestPath) {
  const absolutePath = path.resolve(REPO_ROOT, manifestPath);
  if (!existsSync(absolutePath)) {
    fail(`snippet manifest not found: ${absolutePath}`);
  }
  let manifest;
  try {
    manifest = JSON.parse(readFileSync(absolutePath, 'utf8'));
  } catch (parseError) {
    fail(`${absolutePath}: ${parseError.message}`);
  }
  if (!Array.isArray(manifest.platforms) || manifest.platforms.length === 0) {
    fail(`${absolutePath}: "platforms" must be a non-empty array`);
  }
  for (const platform of manifest.platforms) {
    if (typeof platform.platform !== 'string' || !Array.isArray(platform.roots)) {
      fail(`${absolutePath}: every platform needs a "platform" name and a "roots" array`);
    }
  }
  if (!Array.isArray(manifest.chapters) || manifest.chapters.length === 0) {
    fail(`${absolutePath}: "chapters" must be a non-empty array`);
  }
  return manifest;
}

// ---------------------------------------------------------------------------
// Source regions
// ---------------------------------------------------------------------------

function listSourceFiles(rootDirectory) {
  const files = [];
  const pending = [rootDirectory];
  while (pending.length > 0) {
    const directory = pending.pop();
    for (const entry of readdirSync(directory)) {
      if (SCAN_EXCLUDED_DIRECTORY_NAMES.has(entry)) {
        continue;
      }
      const absoluteEntry = path.join(directory, entry);
      const statistics = statSync(absoluteEntry);
      if (statistics.isDirectory()) {
        pending.push(absoluteEntry);
      } else if (LANGUAGE_BY_EXTENSION.has(path.extname(entry))) {
        files.push(absoluteEntry);
      }
    }
  }
  return files.sort();
}

/**
 * Extracts every marked region from one source file.
 *
 * A region's lines keep their relative indentation; the generator removes the
 * common leading whitespace when it publishes the block, so a region nested in
 * a class body reads at column zero in the chapter.
 */
function extractRegions(absolutePath, displayPath, language) {
  const lines = readFileSync(absolutePath, 'utf8').split('\n');
  const regions = [];
  let openIdentifier = null;
  let openLine = 0;
  let openLines = [];
  for (let index = 0; index < lines.length; index += 1) {
    const markerMatch = lines[index].match(SOURCE_MARKER_PATTERN);
    if (!markerMatch) {
      if (openIdentifier !== null) {
        openLines.push(lines[index]);
      }
      continue;
    }
    const [, direction, identifier] = markerMatch;
    if (direction === 'begin') {
      if (openIdentifier !== null) {
        throw new Error(
          `${displayPath}:${index + 1}: "${openIdentifier}" is still open ` +
            `(begun at line ${openLine}); a marked region cannot nest`,
        );
      }
      if (!SNIPPET_IDENTIFIER_PATTERN.test(identifier)) {
        throw new Error(
          `${displayPath}:${index + 1}: "${identifier}" is not a <chapter>/<step> identifier`,
        );
      }
      openIdentifier = identifier;
      openLine = index + 1;
      openLines = [];
    } else {
      if (openIdentifier === null) {
        throw new Error(`${displayPath}:${index + 1}: "end ${identifier}" has no begin`);
      }
      if (identifier !== openIdentifier) {
        throw new Error(
          `${displayPath}:${index + 1}: "end ${identifier}" closes "${openIdentifier}"`,
        );
      }
      regions.push({
        identifier,
        language,
        sourcePath: displayPath,
        sourceLine: openLine,
        code: trimBlankEdges(openLines).join('\n'),
      });
      openIdentifier = null;
      openLines = [];
    }
  }
  if (openIdentifier !== null) {
    throw new Error(`${displayPath}:${openLine}: "${openIdentifier}" is never closed`);
  }
  return regions;
}

function trimBlankEdges(lines) {
  let start = 0;
  let end = lines.length;
  while (start < end && lines[start].trim() === '') {
    start += 1;
  }
  while (end > start && lines[end - 1].trim() === '') {
    end -= 1;
  }
  return lines.slice(start, end);
}

function dedent(lines) {
  const indents = lines
    .filter((line) => line.trim() !== '')
    .map((line) => line.length - line.trimStart().length);
  if (indents.length === 0) {
    return lines;
  }
  const commonIndent = Math.min(...indents);
  return lines.map((line) => (line.trim() === '' ? '' : line.slice(commonIndent)));
}

/**
 * Collects every region from every platform root, keyed by
 * `<platform>\0<identifier>`. Two regions claiming the same key is an error:
 * a chapter could otherwise publish either one depending on scan order.
 */
function collectRegions(manifest) {
  const regions = new Map();
  for (const platform of manifest.platforms) {
    for (const root of platform.roots) {
      const absoluteRoot = path.resolve(REPO_ROOT, root);
      if (!existsSync(absoluteRoot)) {
        fail(`platform "${platform.platform}" root not found: ${absoluteRoot}`);
      }
      for (const absolutePath of listSourceFiles(absoluteRoot)) {
        const displayPath = path.relative(REPO_ROOT, absolutePath);
        const language = LANGUAGE_BY_EXTENSION.get(path.extname(absolutePath));
        for (const region of extractRegions(absolutePath, displayPath, language)) {
          const key = regionKey(platform.platform, region.identifier);
          const existing = regions.get(key);
          if (existing !== undefined) {
            throw new Error(
              `"${region.identifier}" is marked twice for ${platform.platform}: ` +
                `${existing.sourcePath}:${existing.sourceLine} and ` +
                `${region.sourcePath}:${region.sourceLine}`,
            );
          }
          regions.set(key, { ...region, platform: platform.platform });
        }
      }
    }
  }
  return regions;
}

function regionKey(platform, identifier) {
  return `${platform}\u0000${identifier}`;
}

// ---------------------------------------------------------------------------
// Chapter rewriting
// ---------------------------------------------------------------------------

function renderBlock(region) {
  const body = dedent(region.code.split('\n')).join('\n');
  return ['```' + region.language, body, '```'].join('\n');
}

/**
 * Rewrites one chapter's generated regions and returns the new text plus the
 * list of changes. Regions the sources do not provide are reported, not
 * silently emptied: a chapter that names a region nobody marks is drift.
 */
function rewriteChapter(displayPath, sourceText, regions) {
  const lines = sourceText.split('\n');
  const output = [];
  const changes = [];
  const published = new Set();
  let index = 0;
  while (index < lines.length) {
    const beginMatch = lines[index].match(CHAPTER_BEGIN_PATTERN);
    if (!beginMatch) {
      const strayEnd = lines[index].match(CHAPTER_END_PATTERN);
      if (strayEnd) {
        throw new Error(
          `${displayPath}:${index + 1}: "end ${strayEnd[1]} ${strayEnd[2]}" has no begin`,
        );
      }
      output.push(lines[index]);
      index += 1;
      continue;
    }
    const [, identifier, platform] = beginMatch;
    const beginLine = index + 1;
    let endIndex = index + 1;
    while (endIndex < lines.length) {
      const endMatch = lines[endIndex].match(CHAPTER_END_PATTERN);
      if (endMatch) {
        if (endMatch[1] !== identifier || endMatch[2] !== platform) {
          throw new Error(
            `${displayPath}:${endIndex + 1}: "end ${endMatch[1]} ${endMatch[2]}" ` +
              `closes "${identifier} ${platform}" opened at line ${beginLine}`,
          );
        }
        break;
      }
      endIndex += 1;
    }
    if (endIndex >= lines.length) {
      throw new Error(
        `${displayPath}:${beginLine}: "${identifier} ${platform}" is never closed`,
      );
    }
    const region = regions.get(regionKey(platform, identifier));
    if (region === undefined) {
      throw new Error(
        `${displayPath}:${beginLine}: no ${platform} source marks "${identifier}" ` +
          '(add the region to a reference application, or delete the chapter block)',
      );
    }
    const previous = lines.slice(index + 1, endIndex).join('\n');
    const rendered = renderBlock(region);
    if (previous !== rendered) {
      changes.push({
        identifier,
        platform,
        line: beginLine,
        source: `${region.sourcePath}:${region.sourceLine}`,
      });
    }
    output.push(lines[index]);
    output.push(...rendered.split('\n'));
    output.push(lines[endIndex]);
    published.add(regionKey(platform, identifier));
    index = endIndex + 1;
  }
  return { text: output.join('\n'), changes, published };
}

function chapterFiles(manifest) {
  const files = [];
  for (const chapter of manifest.chapters) {
    const absoluteChapter = path.resolve(REPO_ROOT, chapter);
    if (!existsSync(absoluteChapter)) {
      fail(`chapter path not found: ${absoluteChapter}`);
    }
    const statistics = statSync(absoluteChapter);
    if (statistics.isDirectory()) {
      for (const entry of readdirSync(absoluteChapter).sort()) {
        if (entry.endsWith('.md')) {
          files.push(path.join(absoluteChapter, entry));
        }
      }
    } else {
      files.push(absoluteChapter);
    }
  }
  return files;
}

/**
 * Runs the whole pipeline. `write` decides whether chapters are rewritten;
 * the returned report is what `--check` and the default mode both print.
 */
function run({ manifestPath = DEFAULT_MANIFEST_PATH, write = true, verbose = false } = {}) {
  const manifest = readManifest(manifestPath);
  const regions = collectRegions(manifest);
  const report = { drifted: [], written: [], orphans: [], checkedChapters: 0 };
  const published = new Set();
  for (const absoluteChapter of chapterFiles(manifest)) {
    const displayPath = path.relative(REPO_ROOT, absoluteChapter);
    const sourceText = readFileSync(absoluteChapter, 'utf8');
    const result = rewriteChapter(displayPath, sourceText, regions);
    report.checkedChapters += 1;
    for (const change of result.changes) {
      report.drifted.push({ chapter: displayPath, ...change });
    }
    for (const key of result.published) {
      published.add(key);
    }
    if (write && result.text !== sourceText) {
      writeFileSync(absoluteChapter, result.text);
      report.written.push(displayPath);
    }
  }
  for (const [key, region] of regions) {
    if (!published.has(key)) {
      report.orphans.push({
        platform: region.platform,
        identifier: region.identifier,
        source: `${region.sourcePath}:${region.sourceLine}`,
      });
    }
  }
  if (verbose) {
    for (const change of report.drifted) {
      process.stdout.write(
        `${change.chapter}:${change.line}: regenerated ${change.identifier} ` +
          `(${change.platform}) from ${change.source}\n`,
      );
    }
    for (const orphan of report.orphans) {
      process.stdout.write(
        `${orphan.source}: ${orphan.platform} region "${orphan.identifier}" ` +
          'is not published by any chapter\n',
      );
    }
  }
  return report;
}

function printReport(report) {
  process.stdout.write(
    `doc snippets: ${report.checkedChapters} chapters checked, ` +
      `${report.written.length} rewritten, ${report.drifted.length} drifted, ` +
      `${report.orphans.length} unpublished\n`,
  );
  for (const change of report.drifted) {
    process.stdout.write(
      `  drift: ${change.chapter}:${change.line} — ${change.identifier} ` +
        `(${change.platform}) differs from ${change.source}\n`,
    );
  }
  for (const orphan of report.orphans) {
    process.stdout.write(
      `  unpublished: ${orphan.source} — ${orphan.platform} "${orphan.identifier}"\n`,
    );
  }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/**
 * Proves the generator can fail. The self-test builds a temporary tree with
 * one source region and one chapter, then asserts that: a matching pair
 * rewrites to the source's text; a changed source is reported as drift; and a
 * chapter naming an unmarked region throws instead of publishing an empty
 * block.
 */
function runSelfTest() {
  const failures = [];
  const assert = (condition, description) => {
    if (!condition) {
      failures.push(description);
    }
  };

  const regions = new Map();
  regions.set(regionKey('android', 'first-map/create-sdk'), {
    identifier: 'first-map/create-sdk',
    platform: 'android',
    language: 'kotlin',
    sourcePath: 'Example.kt',
    sourceLine: 3,
    code: '        val mapSdk = MapSdk()\n        mapSdk.setProviderCredential(apiKey)',
  });

  const chapter = [
    '# Chapter',
    '',
    '<!-- docs-snippet: begin first-map/create-sdk android -->',
    '```kotlin',
    '        val mapSdk = MapSdk()',
    '```',
    '<!-- docs-snippet: end first-map/create-sdk android -->',
    '',
  ].join('\n');

  const rewritten = rewriteChapter('chapter.md', chapter, regions);
  assert(rewritten.changes.length === 1, 'a stale block is reported as drift');
  assert(
    rewritten.text.includes(
      '```kotlin\nval mapSdk = MapSdk()\nmapSdk.setProviderCredential(apiKey)\n```',
    ),
    'the generated block is dedented and carries every region line',
  );

  const current = rewriteChapter('chapter.md', rewritten.text, regions);
  assert(current.changes.length === 0, 'a regenerated chapter has no drift');

  let orphanDetected = false;
  try {
    rewriteChapter(
      'chapter.md',
      chapter.replace('create-sdk android', 'create-sdk ios').replace(
        'end first-map/create-sdk android',
        'end first-map/create-sdk ios',
      ),
      regions,
    );
  } catch {
    orphanDetected = true;
  }
  assert(orphanDetected, 'a chapter naming an unmarked region fails loudly');

  if (failures.length > 0) {
    for (const failure of failures) {
      process.stderr.write(`self-test failure: ${failure}\n`);
    }
    process.exit(1);
  }
  process.stdout.write('doc snippets self-test: 4 assertions passed\n');
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

function parseArguments(argv) {
  const options = { check: false, selfTest: false, verbose: false, manifestPath: DEFAULT_MANIFEST_PATH };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === '--check') {
      options.check = true;
    } else if (argument === '--self-test') {
      options.selfTest = true;
    } else if (argument === '--verbose') {
      options.verbose = true;
    } else if (argument === '--manifest') {
      index += 1;
      if (index >= argv.length) {
        fail('--manifest needs a path');
      }
      options.manifestPath = argv[index];
    } else if (argument === '--help' || argument === '-h') {
      usage();
      process.exit(0);
    } else {
      fail(`unknown option: ${argument}`);
    }
  }
  return options;
}

function main() {
  const options = parseArguments(process.argv.slice(2));
  if (options.selfTest) {
    runSelfTest();
    return;
  }
  const report = run({
    manifestPath: options.manifestPath,
    write: !options.check,
    verbose: options.verbose,
  });
  printReport(report);
  if (options.check && (report.drifted.length > 0 || report.orphans.length > 0)) {
    process.exit(1);
  }
  if (report.orphans.length > 0 && !options.check) {
    // Writing cannot fix an unpublished region: the author has to decide where
    // it belongs, so the run fails rather than leaving the marker unnoticed.
    process.exit(1);
  }
}

main();
