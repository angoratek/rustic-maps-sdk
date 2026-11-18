#!/usr/bin/env node
// Checks that the internal `docs/` folder stays a navigable set:
//
//   1. every Markdown file under `docs/` (except the index and the generated
//      notices) is linked from `docs/README.md`, so nothing is orphaned;
//   2. every `docs/…` path referenced from an editable file — the root
//      documents, the scripts, the platform sources and the docs themselves —
//      exists, so a move can never leave a dangling reference behind.
//
// History is not exempt: a dated record still has to be where the index says
// it is. `CHANGELOG.md` is exempt from the reference check because its older
// round entries describe the tree as it was.
//
// Usage:
//   node scripts/check-docs-references.mjs
//
// Exit 0 with a summary, or 1 with one line per problem.

import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const REPOSITORY_ROOT = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);
const DOCS_DIRECTORY = path.join(REPOSITORY_ROOT, "docs");
const DOCS_INDEX_PATH = path.join(DOCS_DIRECTORY, "README.md");

/** Files whose `docs/…` references must resolve. */
const REFERENCE_SOURCES = [
  "README.md",
  "CLAUDE.md",
  "PLAN.md",
  "PRD.md",
  "site/book-src",
  "scripts",
  "platform",
  "docs",
];
/** Files that may mention a path that no longer exists (dated narrative). */
const REFERENCE_CHECK_EXEMPTIONS = new Set(["CHANGELOG.md"]);
/** A `docs/…` path with a Markdown extension, anywhere in a line. */
const DOCS_PATH_PATTERN = /docs\/[A-Za-z0-9_./-]+\.(?:md|udl)/g;

/** Every Markdown file under `docs/`, relative to the repository root. */
function list_docs_markdown_files() {
  const collected = [];
  const walk = (directory) => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const absolute_path = path.join(directory, entry.name);
      if (entry.isDirectory()) {
        walk(absolute_path);
      } else if (entry.name.endsWith(".md")) {
        collected.push(
          path.relative(REPOSITORY_ROOT, absolute_path).split(path.sep).join("/"),
        );
      }
    }
  };
  walk(DOCS_DIRECTORY);
  return collected.sort();
}

/** Every regular file under a directory, recursively. */
function list_files(directory) {
  const collected = [];
  const walk = (current) => {
    for (const entry of readdirSync(current, { withFileTypes: true })) {
      const absolute_path = path.join(current, entry.name);
      if (entry.isDirectory()) {
        walk(absolute_path);
      } else if (entry.isFile()) {
        collected.push(absolute_path);
      }
    }
  };
  walk(directory);
  return collected;
}

const problems = [];

// 1. The index links every file.
const docs_index_text = readFileSync(DOCS_INDEX_PATH, "utf8");
const docs_markdown_files = list_docs_markdown_files();
// The index writes a file's path relative to `docs/` in a table cell (and
// occasionally with the `docs/` prefix in prose), so both spellings count.
const INDEXED_PATH_PATTERN = /`([A-Za-z0-9_./-]+\.md)`/g;
const indexed_files = new Set(
  [...docs_index_text.matchAll(INDEXED_PATH_PATTERN)]
    .map((match) => match[1].replace(/^docs\//, ""))
    .filter((relative_path) => !relative_path.startsWith("../")),
);
for (const docs_file of docs_markdown_files) {
  const relative_to_docs = docs_file.replace(/^docs\//, "");
  if (relative_to_docs === "README.md") {
    continue;
  }
  if (relative_to_docs === "THIRD_PARTY_NOTICES.md") {
    // Generated notices, linked from the policy table's prose rather than a
    // row of their own.
    continue;
  }
  if (!indexed_files.has(relative_to_docs)) {
    problems.push(`${docs_file} is not linked from docs/README.md`);
  }
}

// 2. Every reference resolves.
const reference_files = [];
for (const source of REFERENCE_SOURCES) {
  const absolute_path = path.join(REPOSITORY_ROOT, source);
  if (!existsSync(absolute_path)) {
    continue;
  }
  if (statSync(absolute_path).isDirectory()) {
    reference_files.push(...list_files(absolute_path));
  } else {
    reference_files.push(absolute_path);
  }
}
for (const file_path of reference_files) {
  const relative_path = path
    .relative(REPOSITORY_ROOT, file_path)
    .split(path.sep)
    .join("/");
  if (REFERENCE_CHECK_EXEMPTIONS.has(relative_path)) {
    continue;
  }
  if (!/\.(?:md|mjs|js|ts|tsx|rs|kt|swift|sh|yml|yaml|json|toml)$/.test(relative_path)) {
    continue;
  }
  // Installed dependencies and build outputs are not this repository's text.
  if (
    relative_path.includes("node_modules/")
    || relative_path.includes("/build/")
    || relative_path.includes("/target/")
    || relative_path.includes("platform/web/pkg/")
  ) {
    continue;
  }
  let file_text;
  try {
    file_text = readFileSync(file_path, "utf8");
  } catch {
    continue;
  }
  for (const match of file_text.matchAll(DOCS_PATH_PATTERN)) {
    const referenced_path = match[0];
    // A path inside a URL (a GitHub blob link, say) is not this repository's
    // file; require a token boundary before it.
    const preceding_character = match.index > 0
      ? file_text[match.index - 1]
      : " ";
    if (/[A-Za-z0-9/]/.test(preceding_character)) {
      continue;
    }
    if (referenced_path.startsWith("docs/history/")) {
      // A dated record may name a path that has since moved; the history
      // folder's own files are checked by their own references.
      if (!existsSync(path.join(REPOSITORY_ROOT, referenced_path))) {
        problems.push(
          `${relative_path}: references ${referenced_path}, which does not exist`,
        );
      }
      continue;
    }
    if (!existsSync(path.join(REPOSITORY_ROOT, referenced_path))) {
      problems.push(
        `${relative_path}: references ${referenced_path}, which does not exist`,
      );
    }
  }
}

const unique_problems = [...new Set(problems)];
if (unique_problems.length > 0) {
  for (const problem of unique_problems) {
    console.error(`docs-references: ${problem}`);
  }
  process.exit(1);
}
console.log(
  `docs-references: ${docs_markdown_files.length} files under docs/, all indexed; every reference resolves`,
);
