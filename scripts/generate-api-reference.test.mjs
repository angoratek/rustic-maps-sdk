// Tests for scripts/generate-api-reference.mjs.
//
// Run with: node --test scripts/generate-api-reference.test.mjs

import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

import {
  OUTPUT_FILE_NAMES,
  WASM_EXPOSED_UDL_INTERFACES,
  checkApiReferenceDirectory,
  generateApiReferencePages,
  main,
  parseUdl,
  writeApiReferencePages,
} from './generate-api-reference.mjs';

const REPOSITORY_ROOT = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);
const CONTRACT_PATH = path.join(
  REPOSITORY_ROOT,
  'crates/rustic-maps-native-core/src/rustic_maps.udl',
);

const NATIVE_PAGE_NAMES = [
  'api-reference-android.md',
  'api-reference-ios.md',
  'api-reference-react-native.md',
];

/** True when a page has a level-two or level-three heading for a name. */
function hasHeading(pageContent, name) {
  return (
    pageContent.includes(`\n## ${name}\n`)
    || pageContent.includes(`\n### ${name}\n`)
  );
}

/** Makes a scratch directory the caller must remove. */
function makeScratchDirectory(testContext, prefix) {
  const directory = mkdtempSync(path.join(os.tmpdir(), prefix));
  testContext.after(() => {
    rmSync(directory, { recursive: true, force: true });
  });
  return directory;
}

test('generation is deterministic and writes every page', () => {
  const first = generateApiReferencePages();
  const second = generateApiReferencePages();

  assert.deepEqual([...first.keys()], OUTPUT_FILE_NAMES);
  for (const fileName of OUTPUT_FILE_NAMES) {
    assert.equal(typeof first.get(fileName), 'string');
    assert.equal(first.get(fileName), second.get(fileName), `${fileName} must be stable`);
    assert.ok(first.get(fileName).endsWith('\n'), `${fileName} must end with a newline`);
  }
});

test('every generated page carries the generated-file note and no host path', () => {
  const pages = generateApiReferencePages();
  for (const fileName of OUTPUT_FILE_NAMES) {
    const content = pages.get(fileName);
    assert.match(content, /Generated from the SDK contract/u, `${fileName} note`);
    assert.match(content, /Do not edit this page by hand/u, `${fileName} edit warning`);
    assert.equal(content.includes(REPOSITORY_ROOT), false, `${fileName} host path`);
    assert.equal(content.includes('/Users/'), false, `${fileName} absolute path`);
  }
});

test('every contract interface, record, enum and namespace function reaches the native pages', () => {
  const model = parseUdl(readFileSync(CONTRACT_PATH, 'utf8'));
  const pages = generateApiReferencePages();

  for (const pageName of NATIVE_PAGE_NAMES) {
    const pageContent = pages.get(pageName);
    for (const interfaceDefinition of model.interfaces) {
      assert.ok(
        hasHeading(pageContent, interfaceDefinition.name),
        `${pageName} documents interface ${interfaceDefinition.name}`,
      );
    }
    for (const dictionary of model.dictionaries) {
      assert.ok(
        hasHeading(pageContent, dictionary.name),
        `${pageName} documents record ${dictionary.name}`,
      );
    }
    for (const enumDefinition of model.enums) {
      assert.ok(
        hasHeading(pageContent, enumDefinition.name),
        `${pageName} documents enum ${enumDefinition.name}`,
      );
    }
    for (const callable of model.functions) {
      const camelName = callable.name.replace(
        /_([a-z0-9])/gu,
        (_, character) => character.toUpperCase(),
      );
      assert.ok(
        pageContent.includes(`\`${camelName}\``),
        `${pageName} documents namespace function ${callable.name}`,
      );
    }
  }
});

test('the WebAssembly page exposes every contract interface the wasm surface serves', () => {
  const pages = generateApiReferencePages();
  const wasmPage = pages.get('api-reference-wasm.md');
  for (const interfaceName of WASM_EXPOSED_UDL_INTERFACES) {
    assert.ok(
      hasHeading(wasmPage, interfaceName),
      `the WebAssembly page documents ${interfaceName}`,
    );
  }
  // The canvas surface and the generated contract classes the package serves.
  for (const className of [
    'BrowserMapView',
    'MapCamera',
    'MapSdk',
    'Navigator',
    'RouteSimulator',
  ]) {
    assert.ok(hasHeading(wasmPage, className), `the WebAssembly page documents ${className}`);
  }
});

test('--check accepts a freshly generated directory and rejects a mutated page', (testContext) => {
  const pages = generateApiReferencePages();
  const directory = makeScratchDirectory(testContext, 'api-reference-check-');

  writeApiReferencePages(pages, directory);
  assert.equal(checkApiReferenceDirectory(pages, directory).isStale, false);
  assert.equal(main(['--check', directory]), 0);

  writeFileSync(
    path.join(directory, 'api-reference-android.md'),
    `${readFileSync(path.join(directory, 'api-reference-android.md'), 'utf8')}\nstale line\n`,
  );
  const result = checkApiReferenceDirectory(pages, directory);
  assert.equal(result.isStale, true);
  assert.deepEqual(
    result.staleFiles.map((staleFile) => staleFile.fileName),
    ['api-reference-android.md'],
  );
  assert.equal(main(['--check', directory]), 1);
});

test('--check rejects a missing page', (testContext) => {
  const pages = generateApiReferencePages();
  const directory = makeScratchDirectory(testContext, 'api-reference-missing-');
  writeApiReferencePages(pages, directory);
  rmSync(path.join(directory, 'api-reference-wasm.md'));

  const result = checkApiReferenceDirectory(pages, directory);
  assert.equal(result.isStale, true);
  assert.equal(result.staleFiles[0].fileName, 'api-reference-wasm.md');
  assert.equal(result.staleFiles[0].reason, 'missing');
});
