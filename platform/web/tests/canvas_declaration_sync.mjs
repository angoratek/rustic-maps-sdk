// Declaration-sync test: the published browser types
// (`platform/web/types/browser_canvas.d.ts`, shipped as `rustic_maps_bg.d.ts`)
// describe the hand-written canvas surface that wasm-bindgen emits into
// `platform/web/generated/rustic_maps_bg.js`. Nothing else compares the two,
// so a member added to or removed from the Rust canvas would drift silently
// from the declarations a consumer compiles against.
//
// The test parses the classes, functions, constants, methods and accessors of
// both files and fails on any difference in either direction. wasm-bindgen's
// own `__wbg_*` / `__wbindgen_*` helpers and the generated `free` are not part
// of the published surface and are ignored.
//
// Run directly (no server or browser involved); the package must have been
// built by `scripts/build-wasm2-package.sh`:
//   node --test platform/web/tests/canvas_declaration_sync.mjs
// Playwright also runs it through its `globalSetup`, which builds the package
// first.

import { strict as assert } from 'node:assert';
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath, pathToFileURL } from 'node:url';

const TEST_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const REPOSITORY_ROOT = path.resolve(TEST_DIRECTORY, '..', '..', '..');
const GLUE_PATH = path.join(
  REPOSITORY_ROOT,
  'platform',
  'web',
  'generated',
  'rustic_maps_bg.js',
);
const DECLARATION_PATH = path.join(
  REPOSITORY_ROOT,
  'platform',
  'web',
  'types',
  'browser_canvas.d.ts',
);

/** Whether a generated helper name stays out of the published surface. */
function isGeneratedHelper(name) {
  return name.startsWith('__wbg_') || name.startsWith('__wbindgen_');
}

/**
 * Reads one class body from `lines`, starting after its declaration line.
 * The generated glue and the declaration file both close a class with `}` in
 * column zero.
 */
function read_class_body(lines, declaration_index) {
  const body = [];
  for (let index = declaration_index + 1; index < lines.length; index += 1) {
    if (lines[index] === '}') return body;
    body.push(lines[index]);
  }
  return body;
}

/** Splits a source file into its exported names and per-class member sets. */
function parse_surface(source) {
  const lines = source.split('\n');
  const classes = new Map();
  const functions = new Set();
  const constants = new Set();
  lines.forEach((line, index) => {
    const class_declaration = /^export class (\w+) \{/.exec(line);
    if (class_declaration !== null) {
      classes.set(class_declaration[1], read_class_body(lines, index));
      return;
    }
    const function_declaration = /^export function (\w+)\(/.exec(line);
    if (function_declaration !== null) {
      const name = function_declaration[1];
      if (!isGeneratedHelper(name)) functions.add(name);
      return;
    }
    const constant_declaration = /^export const (\w+)[ :=]/.exec(line);
    if (constant_declaration !== null) constants.add(constant_declaration[1]);
  });
  return { classes, functions, constants };
}

/** Collects the published methods and accessors of a generated class body. */
function parse_generated_members(body) {
  const members = new Set();
  for (const line of body) {
    const accessor = /^ {4}(?:static )?get (\w+)\(/.exec(line);
    if (accessor !== null) {
      members.add(accessor[1]);
      continue;
    }
    const method = /^ {4}(?:static )?(\w+)\(/.exec(line);
    if (method !== null) {
      const name = method[1];
      if (name === 'constructor' || name.startsWith('__')) continue;
      members.add(name);
    }
  }
  return members;
}

/** Collects the declared methods and accessors of a declaration class body. */
function parse_declared_members(body) {
  const members = new Set();
  for (const line of body) {
    const accessor = /^ {2}(?:static )?get (\w+)\(/.exec(line);
    if (accessor !== null) {
      members.add(accessor[1]);
      continue;
    }
    const method = /^ {2}(?:static )?(\w+)\(/.exec(line);
    if (method !== null) {
      if (method[1] === 'constructor') continue;
      members.add(method[1]);
    }
  }
  return members;
}

/** Formats a set as a sorted, comma-separated list for a failure message. */
function describe_names(names) {
  return [...names].sort().join(', ');
}

/**
 * Asserts that every class, function, constant, method and accessor the
 * generated glue exports is declared, and that the declarations add nothing
 * the glue does not export.
 *
 * @throws {AssertionError} on any difference in either direction.
 */
export function run_canvas_declaration_sync_checks() {
  assert.ok(
    existsSync(GLUE_PATH),
    `${GLUE_PATH} is missing; build it with scripts/build-wasm2-package.sh`,
  );
  const generated = parse_surface(readFileSync(GLUE_PATH, 'utf8'));
  const declared = parse_surface(readFileSync(DECLARATION_PATH, 'utf8'));

  assert.deepEqual(
    [...generated.classes.keys()].sort(),
    [...declared.classes.keys()].sort(),
    'the classes exported by the glue and the declarations differ',
  );
  assert.deepEqual(
    [...generated.functions].sort(),
    [...declared.functions].sort(),
    'the functions exported by the glue and the declarations differ',
  );
  assert.deepEqual(
    [...generated.constants].sort(),
    [...declared.constants].sort(),
    'the constants exported by the glue and the declarations differ',
  );

  const member_differences = [];
  for (const [class_name, generated_body] of generated.classes) {
    const generated_members = parse_generated_members(generated_body);
    const declared_members = parse_declared_members(declared.classes.get(class_name));
    const undeclared = [...generated_members].filter(
      (name) => !declared_members.has(name),
    );
    if (undeclared.length > 0) {
      member_differences.push(
        `${class_name} exports members the declarations omit: ${describe_names(undeclared)}`,
      );
    }
    const unrealised = [...declared_members].filter(
      (name) => !generated_members.has(name),
    );
    if (unrealised.length > 0) {
      member_differences.push(
        `${class_name} declares members the glue does not export: ${describe_names(unrealised)}`,
      );
    }
  }
  assert.deepEqual(member_differences, [], member_differences.join('\n'));
}

/** The `node --test` suite name of the declaration assertions. */
const CANVAS_DECLARATION_SYNC_TEST_NAME =
  'the published canvas declarations match the generated glue';

// Run the suite only when this file is the entry point; the Playwright
// `globalSetup` imports the exported runner instead.
const RUNNING_AS_ENTRY = process.argv[1] !== undefined
  && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url;

if (RUNNING_AS_ENTRY) {
  test(CANVAS_DECLARATION_SYNC_TEST_NAME, () => {
    run_canvas_declaration_sync_checks();
  });
}
