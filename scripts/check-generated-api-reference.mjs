#!/usr/bin/env node
// Gates the five generated API reference pages against the source surface
// they were rendered from.
//
// `scripts/build-docs-site.sh` generates the pages with
// `scripts/generate-api-reference.mjs`, stamps each page with a machine-
// readable provenance marker holding the source surface revision, and then
// verifies the staged set. The revision is the HEAD commit plus a SHA-256
// fingerprint of every input the generator reads: the UniFFI contract, the
// generated Kotlin/Swift/TypeScript/wasm declaration surfaces, the wasm export
// sources, the Rust crate sources the Rust page indexes and the renderer
// itself. A page that is missing, carries no marker, or carries a marker from
// a different revision is refused, so a stale page can never be staged at all
// (the verification also runs on a non-publish build) or published.
//
// USAGE
//   node scripts/check-generated-api-reference.mjs --print-revision
//   node scripts/check-generated-api-reference.mjs --mark <directory> [--revision <revision>]
//   node scripts/check-generated-api-reference.mjs --verify <directory> [--revision <revision>]
//   node scripts/check-generated-api-reference.mjs --self-test
//
// `--mark` refuses a page that is already marked: stamping a page the build
// did not just render would mask staleness. Without `--revision`, `--mark`
// and `--verify` compute the revision from the current tree.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  API_REFERENCE_SOURCE_DIRECTORY_PATHS,
  API_REFERENCE_SOURCE_FILE_PATHS,
  OUTPUT_FILE_NAMES,
} from "./generate-api-reference.mjs";

const REPOSITORY_ROOT = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);

/**
 * The renderer is an input too: a change to the generator can change every
 * page without touching the contract or a binding surface.
 */
const GENERATOR_SCRIPT_RELATIVE_PATH = "scripts/generate-api-reference.mjs";

/** Every file whose contents feed the fingerprint, in a fixed order. */
const PROVENANCE_FILE_PATHS = [
  GENERATOR_SCRIPT_RELATIVE_PATH,
  ...API_REFERENCE_SOURCE_FILE_PATHS,
];

/** The opening of the provenance marker every generated page carries. */
const REVISION_MARKER_PREFIX =
  "<!-- generated-api-reference-provenance: revision=";
/** The closing of the provenance marker. */
const REVISION_MARKER_SUFFIX = " -->";
/** Matches the whole first line of a marked page and captures its revision. */
const REVISION_MARKER_PATTERN =
  /^<!-- generated-api-reference-provenance: revision=(\S+) -->$/u;
/** A revision must stay a single whitespace-free token inside the marker. */
const REVISION_PATTERN = /^\S+$/u;
/** Fingerprint entry for an input file that is absent from the tree. */
const MISSING_FILE_FINGERPRINT = "<missing-file>";
/** Fingerprint entry for an input directory that is absent from the tree. */
const MISSING_DIRECTORY_FINGERPRINT = "<missing-directory>";
/** Directory entries whose contents the fingerprint includes. */
const RUST_SOURCE_FILE_SUFFIX = ".rs";

/** Answers the marker line for a revision. */
function revisionMarkerLine(revision) {
  return `${REVISION_MARKER_PREFIX}${revision}${REVISION_MARKER_SUFFIX}`;
}

/** Answers the revision a page carries, or null when it carries no marker. */
function readMarkerRevision(pageContent) {
  const firstLine = pageContent.replace(/\r\n?/gu, "\n").split("\n", 1)[0];
  const markerMatch = REVISION_MARKER_PATTERN.exec(firstLine);
  return markerMatch === null ? null : markerMatch[1];
}

/** True when the first line begins a provenance marker, valid or not. */
function startsWithMarker(pageContent) {
  const firstLine = pageContent.replace(/\r\n?/gu, "\n").split("\n", 1)[0];
  return firstLine.startsWith(REVISION_MARKER_PREFIX);
}

/** Lists the `.rs` files under a directory, recursively, in sorted order. */
function listRustSourceFiles(directory) {
  const collected = [];
  const walk = (currentDirectory) => {
    for (const entry of readdirSync(currentDirectory, { withFileTypes: true })) {
      const absolutePath = path.join(currentDirectory, entry.name);
      if (entry.isDirectory()) {
        walk(absolutePath);
      } else if (entry.isFile() && entry.name.endsWith(RUST_SOURCE_FILE_SUFFIX)) {
        collected.push(absolutePath);
      }
    }
  };
  walk(directory);
  return collected.sort();
}

/** Answers the repository-relative path with forward slashes. */
function relativePath(repositoryRoot, absolutePath) {
  return path.relative(repositoryRoot, absolutePath).split(path.sep).join("/");
}

/** Reads HEAD; a non-git tree cannot name the revision it rendered from. */
function readHeadCommit(repositoryRoot) {
  try {
    return execFileSync("git", [
      "-C",
      repositoryRoot,
      "rev-parse",
      "HEAD",
    ], { encoding: "utf8" }).trim();
  } catch {
    throw new Error(
      `${repositoryRoot} is not a git checkout; the generated API reference`
        + " revision needs the HEAD commit",
    );
  }
}

/**
 * Answers the source surface revision: the HEAD commit plus a SHA-256
 * fingerprint of every input the generator reads. The fingerprint records a
 * sentinel for an absent input, so removing or adding a generated binding
 * surface changes the revision too.
 */
export function computeSourceSurfaceRevision(repositoryRoot = REPOSITORY_ROOT) {
  const sourceCommit = readHeadCommit(repositoryRoot);
  const entries = [];
  for (const filePath of PROVENANCE_FILE_PATHS) {
    entries.push({
      relativePath: filePath,
      absolutePath: path.join(repositoryRoot, filePath),
      kind: "file",
    });
  }
  for (const directoryPath of API_REFERENCE_SOURCE_DIRECTORY_PATHS) {
    const absoluteDirectory = path.join(repositoryRoot, directoryPath);
    if (!existsSync(absoluteDirectory) || !statSync(absoluteDirectory).isDirectory()) {
      entries.push({
        relativePath: directoryPath,
        absolutePath: absoluteDirectory,
        kind: "missing-directory",
      });
      continue;
    }
    for (const rustSourcePath of listRustSourceFiles(absoluteDirectory)) {
      entries.push({
        relativePath: relativePath(repositoryRoot, rustSourcePath),
        absolutePath: rustSourcePath,
        kind: "file",
      });
    }
  }
  // A byte-order sort, not a locale-aware one: the revision must be identical
  // on every machine that reads the same tree.
  entries.sort((left, right) => (
    left.relativePath < right.relativePath
      ? -1
      : (left.relativePath > right.relativePath ? 1 : 0)
  ));

  const fingerprint = createHash("sha256");
  for (const entry of entries) {
    fingerprint.update(`${entry.relativePath}\u0000`);
    if (entry.kind === "missing-directory") {
      fingerprint.update(MISSING_DIRECTORY_FINGERPRINT);
    } else if (!existsSync(entry.absolutePath)) {
      fingerprint.update(MISSING_FILE_FINGERPRINT);
    } else {
      fingerprint.update(readFileSync(entry.absolutePath));
    }
  }
  return `${sourceCommit}:sha256:${fingerprint.digest("hex")}`;
}

/**
 * Prepends the provenance marker to every generated page. Answers the number
 * of pages marked. Refuses a missing, empty or already-marked page: the
 * marker states what the build just rendered, so re-stamping an older page
 * would hide exactly the staleness this gate exists to catch.
 */
export function markGeneratedPages(directory, revision) {
  if (!REVISION_PATTERN.test(revision)) {
    throw new Error(`the revision ${JSON.stringify(revision)} is not a single token`);
  }
  let markedPageCount = 0;
  for (const fileName of OUTPUT_FILE_NAMES) {
    const pagePath = path.join(directory, fileName);
    if (!existsSync(pagePath)) {
      throw new Error(
        `the generated API reference page ${fileName} is missing at ${pagePath};`
          + ` write it first with node scripts/generate-api-reference.mjs --output-dir ${directory}`,
      );
    }
    const pageContent = readFileSync(pagePath, "utf8");
    if (pageContent.trim() === "") {
      throw new Error(
        `the generated API reference page ${fileName} is empty at ${pagePath};`
          + ` regenerate it with scripts/build-docs-site.sh`,
      );
    }
    if (startsWithMarker(pageContent)) {
      throw new Error(
        `the generated API reference page ${fileName} already carries a provenance`
          + " marker; regenerate it from the current tree instead of stamping an older page",
      );
    }
    writeFileSync(pagePath, `${revisionMarkerLine(revision)}\n${pageContent}`);
    markedPageCount += 1;
  }
  return markedPageCount;
}

/**
 * Answers one violation per page that is missing, empty, unmarked or marked
 * with a revision other than the expected one. An empty answer means the
 * staged set was rendered from the current source surface.
 */
export function findGeneratedPageViolations(directory, revision) {
  const violations = [];
  for (const fileName of OUTPUT_FILE_NAMES) {
    const pagePath = path.join(directory, fileName);
    if (!existsSync(pagePath)) {
      violations.push(
        `the generated API reference page ${fileName} is missing at ${pagePath};`
          + " regenerate the pages with `node scripts/generate-api-reference.mjs"
          + ` --output-dir ${directory}` + "`",
      );
      continue;
    }
    const pageContent = readFileSync(pagePath, "utf8");
    if (pageContent.trim() === "") {
      violations.push(
        `the generated API reference page ${fileName} is empty at ${pagePath};`
          + " regenerate the pages with scripts/build-docs-site.sh",
      );
      continue;
    }
    const markerRevision = readMarkerRevision(pageContent);
    if (markerRevision === null) {
      violations.push(
        `the generated API reference page ${fileName} carries no provenance`
          + " marker; regenerate the pages with scripts/build-docs-site.sh"
          + " instead of staging them by hand",
      );
      continue;
    }
    if (markerRevision !== revision) {
      violations.push(
        `the generated API reference page ${fileName} was rendered from revision`
          + ` ${markerRevision}, but the current source surface revision is`
          + ` ${revision}; regenerate the pages with scripts/build-docs-site.sh`,
      );
    }
  }
  return violations;
}

/** Prints each violation and answers the exit code for the verification. */
export function runVerification(directory, revision) {
  const violations = findGeneratedPageViolations(directory, revision);
  if (violations.length > 0) {
    for (const violation of violations) {
      process.stderr.write(`generated API reference gate: ${violation}\n`);
    }
    process.stderr.write(
      `generated API reference gate: ${violations.length} of`
        + ` ${OUTPUT_FILE_NAMES.length} page(s) refused; refusing to stage the site\n`,
    );
    return 1;
  }
  process.stdout.write(
    `generated API reference gate: ${OUTPUT_FILE_NAMES.length} page(s) match`
      + ` revision ${revision}\n`,
  );
  return 0;
}

/** Writes the generated page names into a scratch directory with a marker. */
function writeScratchPages(directory, revision) {
  for (const fileName of OUTPUT_FILE_NAMES) {
    const pageContent = `# ${fileName}\n\nGenerated test page.\n`;
    const markedContent = revision === null
      ? pageContent
      : `${revisionMarkerLine(revision)}\n${pageContent}`;
    writeFileSync(path.join(directory, fileName), markedContent);
  }
}

/**
 * Asserts the fail-closed behavior with synthetic directory cases: the
 * matching set passes, and a missing page, an absent marker, an empty page
 * and a mismatched marker each answer a non-zero exit code.
 */
export function runSelfTest() {
  const expectedRevision =
    `${"f".repeat(40)}:sha256:${"a".repeat(64)}`;
  const otherRevision =
    `${"e".repeat(40)}:sha256:${"b".repeat(64)}`;
  const firstPageName = OUTPUT_FILE_NAMES[0];
  const scratchRoot = mkdtempSync(
    path.join(os.tmpdir(), "generated-api-reference-gate-"),
  );
  const cases = [
    {
      name: "a matching marked set passes",
      prepare(directory) {
        writeScratchPages(directory, expectedRevision);
      },
      expectedExitCode: 0,
    },
    {
      name: "a missing page is refused",
      prepare(directory) {
        writeScratchPages(directory, expectedRevision);
        unlinkSync(path.join(directory, firstPageName));
      },
      expectedExitCode: 1,
    },
    {
      name: "an absent marker is refused",
      prepare(directory) {
        writeScratchPages(directory, null);
      },
      expectedExitCode: 1,
    },
    {
      name: "an empty page is refused",
      prepare(directory) {
        writeScratchPages(directory, expectedRevision);
        writeFileSync(path.join(directory, firstPageName), "");
      },
      expectedExitCode: 1,
    },
    {
      name: "a mismatched marker is refused",
      prepare(directory) {
        writeScratchPages(directory, otherRevision);
      },
      expectedExitCode: 1,
    },
  ];
  try {
    for (const [caseIndex, selfTestCase] of cases.entries()) {
      const caseDirectory = path.join(scratchRoot, `case-${caseIndex}`);
      mkdirSync(caseDirectory);
      selfTestCase.prepare(caseDirectory);
      const observedExitCode = findGeneratedPageViolations(
        caseDirectory,
        expectedRevision,
      ).length === 0 ? 0 : 1;
      if (observedExitCode !== selfTestCase.expectedExitCode) {
        process.stderr.write(
          "generated API reference gate self-check FAILED: case"
            + ` '${selfTestCase.name}' expected exit ${selfTestCase.expectedExitCode},`
            + ` observed ${observedExitCode}\n`,
        );
        return 1;
      }
    }
    process.stdout.write(
      `generated API reference gate self-test: ${cases.length}/${cases.length}`
        + " cases behave as ruled\n",
    );
    return 0;
  } finally {
    rmSync(scratchRoot, { recursive: true, force: true });
  }
}

/** Prints the CLI usage text. */
function printUsage() {
  process.stdout.write(
    "usage: node scripts/check-generated-api-reference.mjs"
      + " [--print-revision | --mark DIR | --verify DIR | --self-test]"
      + " [--revision REVISION]\n",
  );
}

/** Parses the CLI arguments. */
function parseArguments(argv) {
  const options = {
    mode: null,
    directory: null,
    revision: null,
    selfTest: false,
    help: false,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === "--print-revision") {
      options.mode = "print-revision";
    } else if (argument === "--mark" || argument === "--verify") {
      options.mode = argument.slice(2);
      options.directory = argv[index + 1];
      if (options.directory === undefined) {
        throw new Error(`${argument} needs a directory`);
      }
      index += 1;
    } else if (argument === "--revision") {
      options.revision = argv[index + 1];
      if (options.revision === undefined) {
        throw new Error("--revision needs a value");
      }
      index += 1;
    } else if (argument === "--self-test") {
      options.selfTest = true;
    } else if (argument === "--help" || argument === "-h") {
      options.help = true;
    } else {
      throw new Error(`unknown option: ${argument}`);
    }
  }
  const selectedModes = [
    options.mode !== null ? 1 : 0,
    options.selfTest ? 1 : 0,
  ].reduce((total, selected) => total + selected, 0);
  if (!options.help && selectedModes !== 1) {
    throw new Error("choose exactly one of --print-revision, --mark, --verify or --self-test");
  }
  return options;
}

/** Runs the selected mode and answers an exit code. */
export function main(argv) {
  let options;
  try {
    options = parseArguments(argv);
  } catch (error) {
    process.stderr.write(`error: ${error.message}\n`);
    return 2;
  }
  if (options.help) {
    printUsage();
    return 0;
  }
  if (options.selfTest) {
    return runSelfTest();
  }
  let revision = options.revision;
  if (revision === null) {
    try {
      revision = computeSourceSurfaceRevision(REPOSITORY_ROOT);
    } catch (error) {
      process.stderr.write(`error: ${error.message}\n`);
      return 1;
    }
  }
  if (options.mode === "print-revision") {
    process.stdout.write(`${revision}\n`);
    return 0;
  }
  try {
    if (options.mode === "mark") {
      const markedPageCount = markGeneratedPages(options.directory, revision);
      process.stdout.write(
        `generated API reference revision: ${markedPageCount} page(s) marked`
          + ` with ${revision}\n`,
      );
      return 0;
    }
    return runVerification(options.directory, revision);
  } catch (error) {
    process.stderr.write(`generated API reference gate: ${error.message}\n`);
    return 1;
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main(process.argv.slice(2));
}
