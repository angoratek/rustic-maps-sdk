// Fixture-provenance gate: every committed test payload names where it came
// from, when it was recorded or lifted, and the SHA-256 of its bytes. The
// manifest (`fixtures/provenance.json`) is the machine-checked record; the
// fixture READMEs carry the human-readable account.
//
// The gate reads only files — no browser, no network, no generated package —
// and fails when:
//
//   1. a recorded file's SHA-256 differs from the manifest;
//   2. a fixture exists under a scanned root without a manifest entry;
//   3. a manifest entry names a file that no longer exists; or
//   4. the provider layer's pinned version (`Cargo.toml` requirement and
//      `Cargo.lock` resolution for the `everymap-*` crates) differs from the
//      version the manifest recorded when the provenance was reviewed.
//
// A dependency bump therefore fails here until the reviewer re-records the
// manifest, which is the point: the provider payloads are lifted from those
// crates, so a new provider-line version forces a provenance review.
//
// Run directly (fast, no server or browser):
//   node --test platform/web/tests/fixture_provenance.mjs
// Playwright also runs it through its `globalSetup`.

import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath, pathToFileURL } from 'node:url';

const TEST_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const REPOSITORY_ROOT = path.resolve(TEST_DIRECTORY, '..', '..', '..');

/** The committed provenance manifest, beside the shared fixture directory. */
const MANIFEST_PATH = path.join(REPOSITORY_ROOT, 'fixtures', 'provenance.json');

/** The workspace manifest carrying the provider-layer dependency pin. */
const WORKSPACE_MANIFEST_PATH = path.join(REPOSITORY_ROOT, 'Cargo.toml');

/** The workspace lockfile carrying the provider-layer resolved versions. */
const WORKSPACE_LOCKFILE_PATH = path.join(REPOSITORY_ROOT, 'Cargo.lock');

/**
 * The committed fixture roots. Every non-documentation file under one of
 * these directories must carry a manifest entry, so a new fixture cannot
 * arrive without provenance.
 */
const FIXTURE_ROOTS = [
  'fixtures',
  'crates/rustic-maps-navigation/fixtures',
  'crates/rustic-maps-offline-graph-compiler/fixtures',
  'crates/rustic-maps-rendering/fixtures',
  'crates/rustic-maps-style/fixtures',
  'platform/web/perf/fixtures',
  'platform/ios/reference-application/Tests/ReferenceApplicationUITests/Fixtures',
];

/**
 * Documentation files inside a fixture directory. They describe the group's
 * provenance rather than being payloads, so they carry no hash of their own.
 */
const DOCUMENTATION_FILE_NAME = 'README.md';

/** The source kinds the manifest may name. */
const SOURCE_KINDS = new Set([
  'url',
  'provider-crate',
  'recorded',
  'generated',
  'authored',
  'copied',
  'unrecorded',
]);

/** An ISO-8601 calendar date (`YYYY-MM-DD`). */
const ISO_DATE_PATTERN = /^\d{4}-\d{2}-\d{2}$/;

/**
 * Build outputs, dependencies and gitignored scratch that are not committed
 * fixtures. A `fixtures` directory under one of these is not scanned.
 */
const SKIPPED_DIRECTORY_NAMES = new Set([
  '.claude', '.git', '.gradle', '.build', 'DerivedData', 'Pods', 'build', 'coverage',
  'generated', 'node_modules', 'playwright-report', 'target', 'test-results', 'tmp',
]);

/**
 * Whether the scan treats a directory name as gitignored scratch it never
 * enters. `.claude` holds local git worktrees, each with its own copy of
 * every fixture directory; scanning them would report the same fixtures
 * twice and fail a clean checkout that happens to have a worktree open.
 * @param {string} directory_name The base name of a directory.
 * @returns {boolean}
 */
export function is_skipped_directory_name(directory_name) {
  return SKIPPED_DIRECTORY_NAMES.has(directory_name);
}

/** A resolved semantic version (`MAJOR.MINOR.PATCH`). */
const SEMANTIC_VERSION_PATTERN = /^\d+\.\d+\.\d+$/;

/** A provider-layer crate name. */
const PROVIDER_CRATE_PATTERN = /^everymap-(core|providers-[a-z0-9-]+)$/;

/**
 * Returns every payload file under `absolute_directory`, recursively.
 * Fixture READMEs and the provenance manifest itself are documentation, not
 * payloads, and are skipped.
 * @param {string} absolute_directory An absolute directory inside a fixture root.
 * @returns {string[]} Absolute file paths.
 */
function payload_files_under(absolute_directory) {
  const files = [];
  for (const entry of readdirSync(absolute_directory, { withFileTypes: true })) {
    const absolute_path = path.join(absolute_directory, entry.name);
    if (entry.isDirectory()) {
      files.push(...payload_files_under(absolute_path));
    } else if (
      entry.isFile()
      && entry.name !== DOCUMENTATION_FILE_NAME
      && absolute_path !== MANIFEST_PATH
    ) {
      files.push(absolute_path);
    }
  }
  return files;
}

/**
 * Returns every directory named `fixtures` in the repository, so a new
 * fixture root cannot escape the scanned set.
 * @param {string} absolute_directory An absolute directory to search.
 * @returns {string[]} Absolute fixture directory paths.
 */
function fixture_directories_under(absolute_directory) {
  const directories = [];
  for (const entry of readdirSync(absolute_directory, { withFileTypes: true })) {
    if (!entry.isDirectory() || is_skipped_directory_name(entry.name)) {
      continue;
    }
    const absolute_path = path.join(absolute_directory, entry.name);
    if (entry.name === 'fixtures') {
      directories.push(absolute_path);
    }
    directories.push(...fixture_directories_under(absolute_path));
  }
  return directories;
}

/**
 * Returns the repository-relative path with forward slashes, the form the
 * manifest records on every platform.
 * @param {string} absolute_path An absolute path inside the repository.
 * @returns {string}
 */
function repository_relative_path(absolute_path) {
  return path.relative(REPOSITORY_ROOT, absolute_path).split(path.sep).join('/');
}

/**
 * Returns the lowercase hexadecimal SHA-256 of the given bytes.
 * @param {Buffer} bytes The file bytes.
 * @returns {string}
 */
function sha256_hexadecimal(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

/**
 * Determines whether a source declares the fields its kind requires.
 * @param {unknown} source The manifest source object.
 * @param {string} context A failure-message prefix.
 * @param {string} provider_version The provider layer's resolved version.
 * @param {string[]} failures Collects every problem found.
 * @returns {void}
 */
function validate_source(source, context, provider_version, failures) {
  if (source === null || typeof source !== 'object' || Array.isArray(source)) {
    failures.push(`${context}: source must be an object`);
    return;
  }
  if (!SOURCE_KINDS.has(source.kind)) {
    failures.push(`${context}: unknown source kind ${JSON.stringify(source.kind)}`);
    return;
  }
  const require_text = (field) => {
    if (typeof source[field] !== 'string' || source[field].trim() === '') {
      failures.push(`${context}: a ${source.kind} source requires a non-empty ${field}`);
    }
  };
  switch (source.kind) {
    case 'url':
      require_text('url');
      if (typeof source.url === 'string' && !/^https?:\/\//.test(source.url)) {
        failures.push(`${context}: a url source must name an http or https address`);
      }
      break;
    case 'provider-crate':
      require_text('crate');
      require_text('version');
      require_text('test');
      if (typeof source.crate === 'string' && !PROVIDER_CRATE_PATTERN.test(source.crate)) {
        failures.push(`${context}: ${source.crate} is not an everymap provider crate`);
      }
      if (
        typeof source.version === 'string'
        && !SEMANTIC_VERSION_PATTERN.test(source.version)
      ) {
        failures.push(`${context}: ${source.version} is not a resolved semantic version`);
      }
      if (source.version !== provider_version) {
        failures.push(
          `${context}: the source records provider version ${JSON.stringify(source.version)} `
          + `but the workspace pins ${provider_version}`,
        );
      }
      break;
    case 'recorded':
      require_text('recorder');
      require_text('note');
      break;
    case 'generated':
      require_text('generator');
      break;
    case 'authored':
    case 'unrecorded':
      require_text('note');
      break;
    case 'copied':
      require_text('from');
      break;
    default:
      break;
  }
}

/**
 * Parses the `everymap-*` dependency requirements from the workspace
 * manifest's `[workspace.dependencies]` table.
 * @param {string} manifest_source The `Cargo.toml` text.
 * @returns {Map<string, string>} Crate name to requirement string.
 */
function provider_layer_requirements(manifest_source) {
  const requirements = new Map();
  for (const line of manifest_source.split('\n')) {
    // Accept both the string form (`everymap-core = "0.4"`) and the inline
    // table form (`everymap-core = { version = "0.4" }`).
    const match = /^\s*(everymap-(?:core|providers-[a-z0-9-]+))\s*=\s*(?:"([^"]+)"|\{[^}]*\bversion\s*=\s*"([^"]+)")/.exec(line);
    const requirement = match?.[2] ?? match?.[3];
    if (requirement !== undefined) {
      requirements.set(match[1], requirement);
    }
  }
  return requirements;
}

/**
 * Parses the resolved `everymap-*` versions from the workspace lockfile.
 * @param {string} lockfile_source The `Cargo.lock` text.
 * @returns {Map<string, string>} Crate name to resolved version.
 */
function provider_layer_versions(lockfile_source) {
  const versions = new Map();
  let current_crate = null;
  for (const line of lockfile_source.split('\n')) {
    const name_match = /^name = "([^"]+)"$/.exec(line);
    if (name_match !== null) {
      current_crate = PROVIDER_CRATE_PATTERN.test(name_match[1]) ? name_match[1] : null;
      continue;
    }
    const version_match = /^version = "([^"]+)"$/.exec(line);
    if (version_match !== null && current_crate !== null) {
      versions.set(current_crate, version_match[1]);
      current_crate = null;
    }
  }
  return versions;
}

/**
 * Returns every payload path discovered under the scanned fixture roots.
 * @returns {string[]} Repository-relative paths with forward slashes.
 */
function discovered_payload_paths() {
  const discovered = [];
  for (const root of FIXTURE_ROOTS) {
    const absolute_root = path.join(REPOSITORY_ROOT, root);
    assert.ok(statSync(absolute_root).isDirectory(), `fixture root ${root} must exist`);
    for (const absolute_path of payload_files_under(absolute_root)) {
      discovered.push(repository_relative_path(absolute_path));
    }
  }
  return discovered.sort();
}

/**
 * Runs every provenance comparison; throws when the manifest drifts from the
 * committed fixtures or the provider layer. Shared by the `node --test`
 * registration and the Playwright `globalSetup`.
 * @returns {void}
 */
export function run_fixture_provenance_checks() {
  const manifest = JSON.parse(readFileSync(MANIFEST_PATH, 'utf8'));
  assert.equal(manifest.schema, 'fixture-provenance/v1', 'the manifest schema');
  assert.ok(Array.isArray(manifest.groups), 'the manifest groups are an array');

  const provider_layer = manifest.providerLayer;
  assert.ok(
    provider_layer !== null && typeof provider_layer === 'object',
    'the manifest records the provider layer',
  );
  assert.ok(
    typeof provider_layer.crate === 'string' && PROVIDER_CRATE_PATTERN.test(provider_layer.crate),
    'the manifest names the provider-layer crate',
  );
  assert.ok(
    typeof provider_layer.reviewedAt === 'string'
      && ISO_DATE_PATTERN.test(provider_layer.reviewedAt),
    'the manifest records the provider-layer review date',
  );

  const failures = [];

  const requirements = provider_layer_requirements(
    readFileSync(WORKSPACE_MANIFEST_PATH, 'utf8'),
  );
  const resolved_versions = provider_layer_versions(
    readFileSync(WORKSPACE_LOCKFILE_PATH, 'utf8'),
  );
  assert.ok(requirements.size > 0, 'the workspace manifest pins the everymap provider crates');
  assert.ok(resolved_versions.size > 0, 'the workspace lockfile resolves the everymap provider crates');

  const distinct_requirements = new Set(requirements.values());
  const distinct_versions = new Set(resolved_versions.values());
  assert.equal(
    distinct_requirements.size,
    1,
    `every everymap dependency must carry one requirement, found ${[...distinct_requirements].join(', ')}`,
  );
  assert.equal(
    distinct_versions.size,
    1,
    `the lockfile must resolve one everymap version, found ${[...distinct_versions].join(', ')}`,
  );
  const workspace_requirement = [...distinct_requirements][0];
  const workspace_version = [...distinct_versions][0];

  if (!resolved_versions.has(provider_layer.crate)) {
    failures.push(
      `the workspace lockfile does not resolve ${provider_layer.crate}, `
      + 'which the manifest records as the provider layer',
    );
  }
  if (provider_layer.requirement !== workspace_requirement) {
    failures.push(
      `the provider-layer requirement changed: manifest records `
      + `${JSON.stringify(provider_layer.requirement)}, Cargo.toml pins `
      + `${JSON.stringify(workspace_requirement)}; review the provider fixtures and update the manifest`,
    );
  }
  if (provider_layer.resolvedVersion !== workspace_version) {
    failures.push(
      `the provider-layer version changed: manifest records `
      + `${JSON.stringify(provider_layer.resolvedVersion)}, Cargo.lock resolves `
      + `${JSON.stringify(workspace_version)}; review the provider fixtures and update the manifest`,
    );
  }

  const recorded_paths = new Set();
  for (const [group_index, group] of manifest.groups.entries()) {
    const group_context = `group ${group_index} (${JSON.stringify(group.directory)})`;
    if (typeof group.directory !== 'string' || group.directory.trim() === '') {
      failures.push(`${group_context}: directory must be a non-empty string`);
      continue;
    }
    const normalized_directory = group.directory.split(path.sep).join('/');
    if (normalized_directory.startsWith('/') || normalized_directory.split('/').includes('..')) {
      failures.push(`${group_context}: directory must be repository-relative`);
    }
    const group_date = group.recorded;
    if (typeof group_date !== 'string' || !ISO_DATE_PATTERN.test(group_date)) {
      failures.push(`${group_context}: recorded must be an ISO-8601 date`);
    }
    if (group.source !== undefined) {
      validate_source(group.source, group_context, workspace_version, failures);
    }
    if (!Array.isArray(group.files) || group.files.length === 0) {
      failures.push(`${group_context}: files must be a non-empty array`);
      continue;
    }
    for (const [file_index, file] of group.files.entries()) {
      const file_context = `${group_context} file ${file_index} (${JSON.stringify(file.name)})`;
      if (typeof file.name !== 'string' || file.name.trim() === '') {
        failures.push(`${file_context}: name must be a non-empty string`);
        continue;
      }
      if (file.name.split('/').includes('..') || file.name.startsWith('/')) {
        failures.push(`${file_context}: name must be a plain file name`);
        continue;
      }
      const relative_path = `${normalized_directory}/${file.name}`;
      if (recorded_paths.has(relative_path)) {
        failures.push(`${file_context}: ${relative_path} is recorded twice`);
        continue;
      }
      recorded_paths.add(relative_path);

      const file_source = file.source ?? group.source;
      const file_date = file.recorded ?? group.recorded;
      if (file_source === undefined) {
        failures.push(`${file_context}: no source at the file or group level`);
      } else {
        validate_source(file_source, file_context, workspace_version, failures);
      }
      if (typeof file_date !== 'string' || !ISO_DATE_PATTERN.test(file_date)) {
        failures.push(`${file_context}: recorded must be an ISO-8601 date`);
      }
      if (typeof file.sha256 !== 'string' || !/^[0-9a-f]{64}$/.test(file.sha256)) {
        failures.push(`${file_context}: sha256 must be 64 lowercase hexadecimal characters`);
      }

      const absolute_path = path.join(REPOSITORY_ROOT, relative_path);
      let bytes;
      try {
        bytes = readFileSync(absolute_path);
      } catch {
        failures.push(
          `${file_context}: the manifest names ${relative_path}, which no longer exists`,
        );
        continue;
      }
      if (typeof file.bytes === 'number' && file.bytes !== bytes.length) {
        failures.push(
          `${file_context}: ${relative_path} is ${bytes.length} bytes, `
          + `but the manifest records ${file.bytes}`,
        );
      }
      const actual_hash = sha256_hexadecimal(bytes);
      if (typeof file.sha256 === 'string' && file.sha256 !== actual_hash) {
        failures.push(
          `${file_context}: ${relative_path} hash drifted (manifest ${file.sha256}, `
          + `actual ${actual_hash}); review the change and update the manifest`,
        );
      }
    }
  }

  for (const absolute_directory of fixture_directories_under(REPOSITORY_ROOT)) {
    const relative_directory = repository_relative_path(absolute_directory);
    const is_scanned = FIXTURE_ROOTS.some(
      (root) => relative_directory === root || relative_directory.startsWith(`${root}/`),
    );
    if (!is_scanned) {
      failures.push(
        `${relative_directory}: a fixture directory is outside the scanned roots; `
        + 'add it to FIXTURE_ROOTS in platform/web/tests/fixture_provenance.mjs',
      );
    }
  }

  for (const discovered_path of discovered_payload_paths()) {
    if (!recorded_paths.has(discovered_path)) {
      failures.push(
        `${discovered_path}: a fixture exists without a provenance entry; `
        + 'add it to fixtures/provenance.json with its source and recording date',
      );
    }
  }

  assert.deepEqual(failures, [], `fixture provenance drift:\n- ${failures.join('\n- ')}`);
}

/** The `node --test` suite name of the provenance assertions. */
const FIXTURE_PROVENANCE_TEST_NAME =
  'every committed fixture carries reviewed provenance and unmodified bytes';

/** The `node --test` suite name of the skipped-directory assertion. */
const SKIPPED_DIRECTORY_TEST_NAME =
  'the scan ignores gitignored scratch such as local worktrees';

// Run the suite only when this file is the entry point; the Playwright
// `globalSetup` imports the exported runner instead.
const RUNNING_AS_ENTRY = process.argv[1] !== undefined
  && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url;

if (RUNNING_AS_ENTRY) {
  test(FIXTURE_PROVENANCE_TEST_NAME, () => {
    run_fixture_provenance_checks();
  });
  test(SKIPPED_DIRECTORY_TEST_NAME, () => {
    assert.equal(is_skipped_directory_name('.claude'), true);
    assert.equal(is_skipped_directory_name('.git'), true);
    assert.equal(is_skipped_directory_name('target'), true);
    assert.equal(is_skipped_directory_name('node_modules'), true);
    assert.equal(is_skipped_directory_name('crates'), false);
    assert.equal(is_skipped_directory_name('fixtures'), false);
  });
}
