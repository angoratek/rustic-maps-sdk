#!/usr/bin/env node
// Mirrors a staged documentation site onto a Pages checkout, deleting only the
// paths the new tree replaces and never dropping the release downloads by
// accident.
//
// The publisher used to wipe the checkout and copy the staged tree over it.
// A staged tree without `downloads/` (a source checkout that never built the
// release artifacts) then deleted every published download, because the
// publisher cannot tell "this build ships no downloads" from "this build has
// not built them yet". This module makes that distinction explicit: an
// existing `downloads/` tree survives unless the staged tree carries a
// complete set for it, and every path the mirror deletes is printed.
//
// Run directly as `node scripts/mirror-docs-site-tree.mjs <staged-site> <pages-checkout>`.
// The exported functions are what the test suite drives.

import {
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

/** The checkout's own history; the mirror never touches it. */
const PAGES_GIT_DIRECTORY = '.git';

/** The directory the release artifacts live in, inside the published site. */
const DOWNLOADS_DIRECTORY = 'downloads';

/** The per-version checksum file that proves an artifact set is complete. */
const CHECKSUMS_FILE_NAME = 'SHA256SUMS.txt';

/**
 * Lists every file and symlink below `root`, as sorted POSIX-style relative
 * paths. `.git` at the top level is excluded: the mirror never rewrites the
 * pages checkout's history.
 *
 * @param {string} root absolute directory to list.
 * @param {{ skipTopLevelGit?: boolean }} [options] whether to omit `.git`.
 * @returns {string[]} the relative paths.
 */
export function listRelativeFiles(root, options = {}) {
  const { skipTopLevelGit = false } = options;
  const files = [];
  const walk = (directory, prefix) => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      if (skipTopLevelGit && prefix === '' && entry.name === PAGES_GIT_DIRECTORY) {
        continue;
      }
      const path = prefix === '' ? entry.name : `${prefix}/${entry.name}`;
      const absolute = join(directory, entry.name);
      if (entry.isDirectory()) {
        walk(absolute, path);
      } else {
        files.push(path);
      }
    }
  };
  walk(root, '');
  return files.sort();
}

/**
 * Reads the file names a `SHA256SUMS.txt` lists.
 *
 * Both checksum spellings are accepted: `shasum -a 256` writes
 * `hash  name`, and GNU `sha256sum -b` writes `hash *name`.
 *
 * @param {string} checksumsPath the checksum file.
 * @returns {string[]} the listed file names.
 */
export function parseChecksumFileNames(checksumsPath) {
  const names = [];
  const contents = readFileText(checksumsPath);
  for (const line of contents.split('\n')) {
    const match = /^[0-9a-fA-F]{64}\s+\*?(.+?)\s*$/.exec(line);
    if (match) {
      names.push(match[1]);
    }
  }
  return names;
}

/**
 * Decides whether a staged `downloads/` directory is a complete artifact set:
 * every version directory carries a checksum file, every file that checksum
 * file lists is present, and at least one artifact is listed.
 *
 * @param {string} downloadsDirectory the staged downloads directory.
 * @returns {{ complete: boolean, versionCount: number, artifactCount: number, problems: string[] }}
 *   the verdict and the reasons it is incomplete.
 */
export function inspectDownloadsSet(downloadsDirectory) {
  const problems = [];
  if (!existsSync(downloadsDirectory) || !statSync(downloadsDirectory).isDirectory()) {
    return { complete: false, versionCount: 0, artifactCount: 0, problems: ['the directory is absent'] };
  }
  const versionDirectories = readdirSync(downloadsDirectory, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)
    .sort();
  if (versionDirectories.length === 0) {
    problems.push('it holds no version directory');
  }
  let artifactCount = 0;
  for (const version of versionDirectories) {
    const versionDirectory = join(downloadsDirectory, version);
    const checksumsPath = join(versionDirectory, CHECKSUMS_FILE_NAME);
    if (!existsSync(checksumsPath)) {
      problems.push(`${version}/${CHECKSUMS_FILE_NAME} is missing`);
      continue;
    }
    const names = parseChecksumFileNames(checksumsPath);
    if (names.length === 0) {
      problems.push(`${version}/${CHECKSUMS_FILE_NAME} lists no artifact`);
    }
    for (const name of names) {
      artifactCount += 1;
      if (!existsSync(join(versionDirectory, name))) {
        problems.push(`${version}/${name} is listed but absent`);
      }
    }
  }
  return {
    complete: problems.length === 0,
    versionCount: versionDirectories.length,
    artifactCount,
    problems,
  };
}

/**
 * Plans what the mirror does to an existing `downloads/` tree.
 *
 * @param {{ stagedSiteDirectory: string, pagesRepositoryDirectory: string }} input
 *   the staged site and the pages checkout.
 * @returns {{ preserve: boolean, reason: string, fileCount: number }}
 *   whether the checkout's downloads survive, and why.
 */
export function planDownloadsPreservation(input) {
  const existingDownloads = join(input.pagesRepositoryDirectory, DOWNLOADS_DIRECTORY);
  if (!existsSync(existingDownloads) || !statSync(existingDownloads).isDirectory()) {
    return { preserve: false, reason: 'the pages checkout holds no downloads', fileCount: 0 };
  }
  const existingFileCount = countFiles(existingDownloads);
  const stagedDownloads = join(input.stagedSiteDirectory, DOWNLOADS_DIRECTORY);
  if (!existsSync(stagedDownloads)) {
    return {
      preserve: true,
      reason: 'the staged tree carries no downloads directory',
      fileCount: existingFileCount,
    };
  }
  const inspection = inspectDownloadsSet(stagedDownloads);
  if (!inspection.complete) {
    return {
      preserve: true,
      reason: `the staged downloads set is incomplete: ${inspection.problems.join('; ')}`,
      fileCount: existingFileCount,
    };
  }
  return {
    preserve: false,
    reason: `the staged tree carries a complete set of ${inspection.artifactCount} artifacts`,
    fileCount: existingFileCount,
  };
}

/**
 * Mirrors the staged site onto the pages checkout.
 *
 * Every top-level entry except `.git` is removed, the staged tree is copied
 * in, and a preserved `downloads/` tree is restored afterwards. The returned
 * `removedFiles` is exactly what the caller reports as deleted, and it never
 * names a path the mirror preserves.
 *
 * @param {{ stagedSiteDirectory: string, pagesRepositoryDirectory: string }} input
 *   the staged site and the pages checkout.
 * @returns {{ removedFiles: string[], writtenFiles: string[], downloads: { preserve: boolean, reason: string, fileCount: number } }}
 *   what changed.
 */
export function mirrorSiteTree(input) {
  const stagedSiteDirectory = resolve(input.stagedSiteDirectory);
  const pagesRepositoryDirectory = resolve(input.pagesRepositoryDirectory);
  for (const [label, directory] of [
    ['staged site', stagedSiteDirectory],
    ['pages checkout', pagesRepositoryDirectory],
  ]) {
    if (!existsSync(directory) || !statSync(directory).isDirectory()) {
      throw new Error(`the ${label} is not a directory: ${directory}`);
    }
  }

  const downloads = planDownloadsPreservation({
    stagedSiteDirectory,
    pagesRepositoryDirectory,
  });
  const existingFiles = listRelativeFiles(pagesRepositoryDirectory, { skipTopLevelGit: true });
  const stagedFiles = listRelativeFiles(stagedSiteDirectory);
  const stagedSet = new Set(stagedFiles);
  const preservedPrefix = `${DOWNLOADS_DIRECTORY}/`;
  const removedFiles = existingFiles.filter((path) => {
    if (stagedSet.has(path)) {
      return false;
    }
    return !(downloads.preserve && path.startsWith(preservedPrefix));
  });

  let savedDownloadsDirectory = null;
  if (downloads.preserve) {
    savedDownloadsDirectory = mkdtempSync(join(tmpdir(), 'docs-downloads-'));
    cpSync(join(pagesRepositoryDirectory, DOWNLOADS_DIRECTORY), savedDownloadsDirectory, {
      recursive: true,
    });
  }

  for (const entry of readdirSync(pagesRepositoryDirectory, { withFileTypes: true })) {
    if (entry.name === PAGES_GIT_DIRECTORY) {
      continue;
    }
    rmSync(join(pagesRepositoryDirectory, entry.name), { recursive: true, force: true });
  }
  cpSync(stagedSiteDirectory, pagesRepositoryDirectory, { recursive: true });

  if (savedDownloadsDirectory !== null) {
    const restoredDownloads = join(pagesRepositoryDirectory, DOWNLOADS_DIRECTORY);
    rmSync(restoredDownloads, { recursive: true, force: true });
    mkdirSync(restoredDownloads, { recursive: true });
    cpSync(savedDownloadsDirectory, restoredDownloads, { recursive: true });
    rmSync(savedDownloadsDirectory, { recursive: true, force: true });
  }

  return { removedFiles, writtenFiles: stagedFiles, downloads };
}

/** Reads a UTF-8 text file. */
function readFileText(path) {
  return readFileSync(path, 'utf8');
}

/** A path counted as a file. */
function countFiles(directory) {
  return listRelativeFiles(directory).length;
}

/** Runs the mirror when this file is the program, not an import. */
function main() {
  const [stagedSiteDirectory, pagesRepositoryDirectory] = process.argv.slice(2);
  if (!stagedSiteDirectory || !pagesRepositoryDirectory) {
    console.error('usage: mirror-docs-site-tree.mjs <staged-site-directory> <pages-checkout>');
    process.exit(2);
  }
  const result = mirrorSiteTree({ stagedSiteDirectory, pagesRepositoryDirectory });
  for (const path of result.removedFiles) {
    console.log(`removing: ${path}`);
  }
  console.log(`mirrored: ${result.writtenFiles.length} files from ${stagedSiteDirectory}`);
  if (result.downloads.preserve) {
    console.log(
      `preserving: ${result.downloads.fileCount} downloads files (${result.downloads.reason})`,
    );
  } else if (result.downloads.reason.startsWith('the staged tree carries a complete')) {
    console.log(`replacing: ${result.downloads.fileCount} downloads files (${result.downloads.reason})`);
  }
}

const invokedDirectly =
  process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (invokedDirectly) {
  main();
}
