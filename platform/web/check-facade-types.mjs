#!/usr/bin/env node
// Typechecks the browser compatibility facade against the generated package.
//
// `main.mjs` is plain JavaScript with `// @ts-check`; this check runs the
// repository's pinned TypeScript over it, resolving the generated contract
// declarations (`generated/rustic_maps.d.ts`, `rustic_maps-ffi.d.ts` and
// `index.d.ts`) and the committed canvas declarations staged beside the glue
// as `generated/rustic_maps_bg.d.ts`. A generated field that is renamed or
// removed therefore fails here instead of only in a browser test.
//
// The pragma is what checks the entry: `--checkJs` would also check the
// generated JavaScript modules the entry imports, which carry `@ts-nocheck`
// source but no pragma in the emitted output.
//
// Usage:
//   node platform/web/check-facade-types.mjs            # check main.mjs
//   node platform/web/check-facade-types.mjs <entry>    # check one variant
//
// The generated package must already exist:
//   scripts/build-wasm2-package.sh
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const WEB_ROOT = path.dirname(fileURLToPath(import.meta.url));
const TYPESCRIPT_BINARY = path.join(WEB_ROOT, 'node_modules', '.bin', 'tsc');
/** The generated package the facade imports; gitignored build output. */
const GENERATED_PACKAGE_DIRECTORY = path.join(WEB_ROOT, 'generated');
/** The facade checked when no entry is named. */
const DEFAULT_ENTRY_FILE = path.join(WEB_ROOT, 'main.mjs');
/** The pragma that makes TypeScript check this file without `--checkJs`. */
const FACADE_TYPECHECK_PRAGMA = '// @ts-check';
/** The declarations the entry resolves against; every one must be present. */
const REQUIRED_DECLARATION_FILE_NAMES = [
  'index.d.ts',
  'rustic_maps.d.ts',
  'rustic_maps-ffi.d.ts',
  'rustic_maps_bg.d.ts',
];

/**
 * Fails with an actionable message when the package or the pinned compiler is
 * missing, so a checkout that skipped the packaging step learns the remedy
 * instead of reading a compiler resolution error.
 */
function assert_prerequisites() {
  if (!existsSync(TYPESCRIPT_BINARY)) {
    throw new Error(
      `the pinned TypeScript compiler is missing: ${TYPESCRIPT_BINARY}; ` +
        'run npm ci in platform/web first',
    );
  }
  for (const fileName of REQUIRED_DECLARATION_FILE_NAMES) {
    const declarationPath = path.join(GENERATED_PACKAGE_DIRECTORY, fileName);
    if (!existsSync(declarationPath)) {
      throw new Error(
        `the generated browser package is missing ${declarationPath}; ` +
          'run scripts/build-wasm2-package.sh',
      );
    }
  }
}

/**
 * Typechecks one facade entry file against the generated package.
 *
 * @param {string} [entryFilePath] The file to check; defaults to `main.mjs`.
 * @throws {Error} when a prerequisite is missing or TypeScript reports errors.
 */
export function run_facade_typecheck(entryFilePath = DEFAULT_ENTRY_FILE) {
  assert_prerequisites();
  // `--allowJs` alone only infers the entry; the pragma is what checks it, so
  // a file that lost it would pass this gate without being checked.
  const entry_source = readFileSync(entryFilePath, 'utf8');
  if (!entry_source.trimStart().startsWith(FACADE_TYPECHECK_PRAGMA)) {
    throw new Error(
      `${entryFilePath} does not start with ${FACADE_TYPECHECK_PRAGMA}, so ` +
        'TypeScript would not check it',
    );
  }
  execFileSync(
    TYPESCRIPT_BINARY,
    [
      '--noEmit',
      '--allowJs',
      '--target',
      'es2022',
      '--module',
      'esnext',
      '--moduleResolution',
      'bundler',
      entryFilePath,
    ],
    { cwd: WEB_ROOT, stdio: 'inherit' },
  );
  console.log(
    `facade typecheck passed: ${path.relative(WEB_ROOT, entryFilePath)}`,
  );
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    run_facade_typecheck(
      process.argv[2] === undefined
        ? DEFAULT_ENTRY_FILE
        : path.resolve(process.argv[2]),
    );
  } catch (error) {
    // TypeScript prints its own diagnostics; this names the failed run.
    console.error(
      error instanceof Error ? error.message.split('\n')[0] : String(error),
    );
    process.exitCode = 1;
  }
}
