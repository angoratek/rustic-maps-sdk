// Public-comment scrub gate: the shipped wrapper sources are a product
// surface, and their documentation comments must describe the feature rather
// than the project's internal history. This gate scans line-initial comments
// and trailing `//` comments in the published Android, iOS and React Native
// wrapper sources for milestone identifiers, round numbers, internal review
// vocabulary and repository paths.
//
// Scope: the public wrapper source trees only. Generated bindings and the
// reference applications are not scanned (the bindings are build artifacts,
// and the reference applications are demo code).
//
// Run directly:
//   node scripts/check-public-comment-scrub.mjs
// It is also run by `scripts/check_ux_parity.mjs`, so a comment that leaks the
// vocabulary fails the standing pre-commit gate.
// Self-test (mutates a temporary tree and asserts both the clean and the
// offending cases behave as required, so the gate cannot pass vacuously):
//   node scripts/check-public-comment-scrub.mjs --self-test
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

/** The published wrapper source trees, relative to the repository root. */
export const SCANNED_SOURCE_DIRECTORIES = [
  'platform/ios/Sources',
  'platform/android/maps-sdk/src/main/kotlin',
  'platform/rn/src',
];

/**
 * Returns the comment text on one line, or `null` when the line carries no
 * comment. A line-initial `//`, `/*` or `*` comment counts whole; otherwise the
 * text from the first `//` onward counts. A `//` that is part of a URL scheme
 * (`https://`) is not a comment start. The scan does not parse string
 * literals, so a bare `//` inside a literal is still treated as a comment
 * start; URLs are stripped before the vocabulary check, so a literal carrying
 * a path such as `/docs/` or `/rounds/10` cannot trip it.
 */
function commentTextOf(line) {
  const trimmed = line.trimStart();
  if (
    trimmed.startsWith('//') ||
    trimmed.startsWith('/*') ||
    trimmed.startsWith('*')
  ) {
    return line;
  }
  let lineCommentIndex = line.indexOf('//');
  while (
    lineCommentIndex > 0 &&
    line[lineCommentIndex - 1] === ':'
  ) {
    lineCommentIndex = line.indexOf('//', lineCommentIndex + 2);
  }
  return lineCommentIndex === -1 ? null : line.slice(lineCommentIndex);
}

/** One banned internal-vocabulary category and what a comment must say instead. */
const BANNED_VOCABULARY = [
  {
    description: 'milestone identifier',
    pattern: /\bM[0-9]{2,3}\b/,
    replacement: 'describe the feature instead',
  },
  {
    description: 'round number',
    pattern: /\brounds?[\s-]+[0-9]+\b/i,
    replacement: 'describe the feature instead',
  },
  {
    description: 'owner-question identifier',
    pattern: /\bOQ\s?-?[0-9]+\b/,
    replacement: 'describe the feature instead',
  },
  {
    description: 'slice vocabulary',
    pattern: /\bslices?\b/i,
    replacement: 'describe the feature instead',
  },
  {
    description: 'milestone vocabulary',
    pattern: /\bmilestones?\b/i,
    replacement: 'describe the feature instead',
  },
  {
    description: 'internal review vocabulary',
    pattern: /\baudits?\b|\bowner(?:'s)?\s+ruling\b|\bPRD\b/i,
    replacement: 'delete the internal aside',
  },
  {
    description: 'internal product vocabulary',
    pattern: /\brefapps?\b|\bharnesses\b/i,
    replacement: 'reference application, test suite',
  },
  {
    description: 'repository path',
    pattern: /(?:docs|platform|crates|scripts|tmp)\//,
    replacement: 'delete the path and say what the reader gets',
  },
];

/**
 * Scans one source tree for comment lines that carry banned vocabulary.
 *
 * @param {string} repositoryRoot the repository root.
 * @returns {{filePath: string, lineNumber: number, description: string, text: string}[]}
 *   one finding per offending comment line, in file and line order.
 */
export function scrubPublicCommentSources(repositoryRoot) {
  const findings = [];
  for (const relativeDirectory of SCANNED_SOURCE_DIRECTORIES) {
    const absoluteDirectory = path.join(repositoryRoot, relativeDirectory);
    for (const filePath of sourceFilesUnder(absoluteDirectory)) {
      const relativeFilePath = path.relative(repositoryRoot, filePath);
      const lines = readFileSync(filePath, 'utf8').split('\n');
      lines.forEach((line, index) => {
        const commentText = commentTextOf(line);
        if (commentText === null) {
          return;
        }
        // A URL in a public comment is reader-facing documentation, not an
        // internal path or identifier, so check the prose around it.
        const scannedText = commentText.replace(/https?:\/\/\S+/g, ' ');
        for (const banned of BANNED_VOCABULARY) {
          if (banned.pattern.test(scannedText)) {
            findings.push({
              filePath: relativeFilePath,
              lineNumber: index + 1,
              description: banned.description,
              text: line.trim(),
            });
            return;
          }
        }
      });
    }
  }
  return findings;
}

/** Every `.swift`, `.kt` and `.ts`/`.tsx` file under one directory, recursively. */
function sourceFilesUnder(directory) {
  const files = [];
  if (!existsSync(directory)) {
    return files;
  }
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const entryPath = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      files.push(...sourceFilesUnder(entryPath));
    } else if (/\.(swift|kt|ts|tsx)$/.test(entry.name)) {
      files.push(entryPath);
    }
  }
  return files;
}

/**
 * Runs the self-test: a clean temporary tree must pass, and a line-initial
 * round identifier, a trailing `//` owner-question identifier and a
 * line-initial milestone identifier must each be reported. Throws nothing;
 * prints the outcome and sets the exit code on failure.
 */
function runSelfTest() {
  const temporaryRoot = mkdtempSync(path.join(tmpdir(), 'public-comment-scrub-'));
  try {
    const cleanDirectory = path.join(
      temporaryRoot,
      SCANNED_SOURCE_DIRECTORIES[0],
      'Clean',
    );
    mkdirSync(cleanDirectory, { recursive: true });
    writeFileSync(
      path.join(cleanDirectory, 'Clean.swift'),
      [
        '// Draws the map view.',
        'let tileURL = "https://example.test/tiles/1/2/3"',
        '// See https://example.test/docs/rounds/10 for the request contract.',
        '',
      ].join('\n'),
    );

    const violatingDirectory = path.join(
      temporaryRoot,
      SCANNED_SOURCE_DIRECTORIES[1],
      'Violating',
    );
    mkdirSync(violatingDirectory, { recursive: true });
    writeFileSync(
      path.join(violatingDirectory, 'Violating.kt'),
      [
        '// The round-10 speech topic.',
        'val state = 1 // OQ5 tracks this',
        '// M22 behaviour',
      ].join('\n'),
    );

    const cleanFindings = scrubPublicCommentSources(
      path.join(temporaryRoot),
    );
    const cleanFileFindings = cleanFindings.filter(
      (finding) => !finding.filePath.includes('Violating.kt'),
    );
    const violatingFindings = cleanFindings.filter((finding) =>
      finding.filePath.includes('Violating.kt'),
    );
    const descriptions = violatingFindings.map(
      (finding) => finding.description,
    );
    const requiredDescriptions = [
      'round number',
      'owner-question identifier',
      'milestone identifier',
    ];
    const ok =
      cleanFileFindings.length === 0 &&
      violatingFindings.length === requiredDescriptions.length &&
      requiredDescriptions.every((description) =>
        descriptions.includes(description),
      );
    if (ok) {
      console.log(
        `Public-comment scrub self-test: PASS (${requiredDescriptions.length}/${requiredDescriptions.length} cases, clean tree clean).`,
      );
    } else {
      console.log(
        `Public-comment scrub self-test: FAIL (clean findings: ${cleanFileFindings.length}, offending findings: ${descriptions.join(', ') || 'none'}).`,
      );
      process.exitCode = 1;
    }
  } finally {
    rmSync(temporaryRoot, { recursive: true, force: true });
  }
}

const isMainModule =
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);

if (isMainModule && process.argv.includes('--self-test')) {
  runSelfTest();
} else if (isMainModule) {
  const repositoryRoot = path.resolve(
    path.dirname(fileURLToPath(import.meta.url)),
    '..',
  );
  const findings = scrubPublicCommentSources(repositoryRoot);
  if (findings.length === 0) {
    console.log(
      `Public-comment scrub: PASS (${SCANNED_SOURCE_DIRECTORIES.length} source trees).`,
    );
  } else {
    for (const finding of findings) {
      console.log(
        `${finding.filePath}:${finding.lineNumber}: ${finding.description}: ${finding.text}`,
      );
    }
    console.log(
      `Public-comment scrub: FAIL (${findings.length} comment line(s) leak internal vocabulary).`,
    );
    process.exitCode = 1;
  }
}
