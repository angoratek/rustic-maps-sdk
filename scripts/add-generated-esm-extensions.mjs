#!/usr/bin/env node
// Adds the `.js` extension to relative import and export specifiers in the
// compiled WebAssembly package.
//
// TypeScript emits the generated bindings' relative specifiers exactly as the
// generator wrote them (`./rustic_maps`, `./rustic_maps-ffi`) because the
// generator targets a bundler, which resolves an extensionless path. A browser
// loading the package as native ES modules does not: it resolves every
// specifier as a URL and needs the file's extension. This rewrite is the one
// transformation the no-bundler packaging path performs, so it only touches
// statements that start a line — a specifier inside a documentation comment is
// left alone.
//
// Usage:
//   node scripts/add-generated-esm-extensions.mjs <package directory>
import { readdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';

const relativeSpecifierPattern =
  /^(\s*(?:import|export)\b[^\n]*?\bfrom\s*)(['"])(\.[^'"]*)\2/gm;
const sideEffectImportPattern = /^(\s*import\s*)(['"])(\.[^'"]*)\2/gm;

/**
 * Rewrites one match, appending `.js` when the relative specifier carries no
 * extension of its own.
 */
function rewriteRelativeSpecifier(match, prefix, quote, specifier) {
  return path.extname(specifier) === ''
    ? `${prefix}${quote}${specifier}.js${quote}`
    : match;
}

const packageDirectory = process.argv[2];
if (packageDirectory === undefined) {
  console.error(
    'add-generated-esm-extensions: pass the package directory to rewrite',
  );
  process.exit(1);
}

for (const fileName of readdirSync(packageDirectory)) {
  if (!fileName.endsWith('.js')) continue;
  const filePath = path.join(packageDirectory, fileName);
  const source = readFileSync(filePath, 'utf8');
  const rewritten = source
    .replace(relativeSpecifierPattern, rewriteRelativeSpecifier)
    .replace(sideEffectImportPattern, rewriteRelativeSpecifier);
  if (rewritten !== source) writeFileSync(filePath, rewritten);
}
