// Provider-name guard (M15, docs/ProviderGenericity.md §5): the north star is
// that a provider name lives only in its provider crate, its fixture
// directory, the shared registry `platform/providers.json` and a registration
// dispatch arm. This check scans the layers M15 migrated and hardened since —
// the browser harness (`platform/web`), the wasm binding sources
// (`crates/rustic-maps-web-bindings/src`), the shipped Android wrapper module
// (`platform/android/maps-sdk/src`) and the three native refapp shells — and
// fails when a provider name appears anywhere else.
//
// The allowlist is deliberately small and documented:
//   - `crates/rustic-maps-web-bindings/src/live_providers.rs` is the wasm dispatch arm
//     (and carries the addition-only vendor-named aliases);
//   - the vendor-named alias identifiers (`register_<provider>_providers` /
//     `clear_<provider>_providers`) may be spelled wherever the compatibility
//     surface is asserted (the contract-sync whitelist);
//   - each native shell's registration dispatch arm (and the named
//     dispatch-key constant it shares) is allowlisted individually below.
// The registry and the provider fixture directories sit outside the scanned
// roots, and the provider crate is not scanned at all.
//
// The names are read from the registry, so a newly added provider is guarded
// automatically with no edit here.
//
// The native refapp shells join the same guard (M15's native slice): the
// Android Compose sources, the iOS SwiftUI sources and the React Native JS
// plus both app-module native sources. Each shell's registration dispatch arm
// and its named dispatch-key constant are allowlisted file by file below; a
// provider name anywhere else in a shell is a violation.
import { strict as assert } from 'node:assert';
import { readFileSync, readdirSync } from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const TEST_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const REPOSITORY_ROOT = path.resolve(TEST_DIRECTORY, '..', '..', '..');

/** The shared provider registry (the only provider-named data source). */
const PROVIDERS_REGISTRY_FILE = path.join(
  REPOSITORY_ROOT, 'platform', 'providers.json',
);

/**
 * The roots this guard owns: the browser harness and wasm binding sources,
 * the shipped Android wrapper module, plus the three native refapp shells
 * (their descriptor-driven chrome must never name a provider outside the
 * allowlist).
 */
const SCANNED_DIRECTORIES = [
  path.join(REPOSITORY_ROOT, 'platform', 'web'),
  path.join(REPOSITORY_ROOT, 'crates', 'rustic-maps-web-bindings', 'src'),
  path.join(REPOSITORY_ROOT, 'platform', 'android', 'maps-sdk', 'src'),
  path.join(REPOSITORY_ROOT, 'platform', 'android', 'reference-application', 'src'),
  path.join(REPOSITORY_ROOT, 'platform', 'ios', 'reference-application', 'Sources'),
  path.join(REPOSITORY_ROOT, 'platform', 'rn', 'app-shell', 'src'),
  path.join(REPOSITORY_ROOT, 'platform', 'rn', 'app-shell', 'android', 'app', 'src'),
  path.join(REPOSITORY_ROOT, 'platform', 'rn', 'app-shell', 'ios', 'MapsSdkRefappRn'),
];

/** Individual shell source files outside the scanned directories. */
const SCANNED_FILES = [
  path.join(REPOSITORY_ROOT, 'platform', 'rn', 'app-shell', 'App.tsx'),
];

/**
 * Build outputs, test artifacts, dependencies and gitignored scratch that are
 * not source: `tmp/` holds throwaway probe scripts and evidence captures,
 * which name whatever provider they happen to be probing. `generated/`
 * directories are build output too — the browser wasm2 package reproduces the
 * contract's own documentation, and the native refapp bindings are emitted
 * from the same UDL.
 */
const SKIPPED_DIRECTORY_NAMES = new Set([
  'node_modules', 'pkg', 'generated', 'playwright-report', 'test-results', 'tmp',
]);

/** The file extensions this guard reads. */
const SCANNED_FILE_EXTENSIONS = new Set([
  '.css', '.h', '.html', '.js', '.json', '.kt', '.kts', '.m', '.md', '.mjs',
  '.mm', '.rs', '.swift', '.ts', '.tsx',
]);

/**
 * The registration dispatch arms (the only provider-named code, per the
 * north star) and the named dispatch-key constants they share. Every entry
 * is a file the guard deliberately trusts; keep the list as small as the
 * design allows.
 */
const ALLOWED_FILES = new Set([
  // The wasm dispatch arm and the addition-only vendor-named aliases.
  path.join(
    REPOSITORY_ROOT, 'crates', 'rustic-maps-web-bindings', 'src', 'live_providers.rs',
  ),
  // iOS SwiftUI: the applier's per-provider switch and its dispatch key.
  path.join(
    REPOSITORY_ROOT, 'platform', 'ios', 'reference-application', 'Sources',
    'ReferenceApplicationModel.swift',
  ),
  path.join(
    REPOSITORY_ROOT, 'platform', 'ios', 'reference-application', 'Sources',
    'ReferenceApplicationConfiguration.swift',
  ),
  // React Native Android: the demo session's dispatch arm and its key.
  path.join(
    REPOSITORY_ROOT, 'platform', 'rn', 'app-shell', 'android', 'app', 'src',
    'main', 'java', 'com', 'rusticmaps', 'sdk', 'refapprn',
    'RefappDemoSession.kt',
  ),
  path.join(
    REPOSITORY_ROOT, 'platform', 'rn', 'app-shell', 'android', 'app', 'src',
    'main', 'java', 'com', 'rusticmaps', 'sdk', 'refapprn',
    'RefappConstants.kt',
  ),
  // React Native iOS: the demo session's dispatch arm and its key.
  path.join(
    REPOSITORY_ROOT, 'platform', 'rn', 'app-shell', 'ios', 'MapsSdkRefappRn',
    'RefappDemoSession.swift',
  ),
  path.join(
    REPOSITORY_ROOT, 'platform', 'rn', 'app-shell', 'ios', 'MapsSdkRefappRn',
    'RefappConstants.swift',
  ),
  // This guard names the allowlist itself.
  fileURLToPath(import.meta.url),
]);

/**
 * The compatibility aliases may be named wherever the surface that carries
 * them is asserted (the contract-sync whitelist); they are stripped from a
 * line before the provider-name search, so nothing else on that line escapes
 * the guard. Both spellings count: the Rust/UDL form and the camelCase name
 * the wasm surface publishes through `js_name`.
 */
const COMPATIBILITY_ALIAS_PATTERNS = [
  /register_[a-z0-9]+_providers/g,
  /clear_[a-z0-9]+_providers/g,
  /register[A-Z][A-Za-z0-9]*Providers/g,
  /clear[A-Z][A-Za-z0-9]*Providers/g,
];

/**
 * The shared reference-flow schema names provider-specific automation hooks,
 * for example `#capability-provider-choice-osrm`. The selector is flow data
 * owned by `platform/reference-flows.json` (outside the scanned roots), and
 * the page renders it from its descriptor-driven picker; it is not a vendor
 * branch. Strip the selector shape before the provider-name search so the two
 * gates agree without exempting the rest of the line.
 */
const FLOW_HOOK_SELECTOR_PATTERNS = [
  /capability-provider-choice-[a-z0-9_-]+/g,
];

/**
 * Provider keys that are also ordinary lowercase words. Their lowercase key
 * is not scanned, because every prose "here" or "radar" would match; the
 * display name still is, matched case-sensitively, so a shell that names the
 * vendor ("HERE", "Google Maps", "Radar") is caught while ordinary words are
 * not.
 */
const ORDINARY_WORD_PROVIDER_KEYS = new Set(['here', 'google', 'radar']);

/**
 * Registered media-type names that embed a vendor name for the tile format
 * itself. They identify a wire format, not a provider, so they are stripped
 * before the name search rather than exempting the whole line.
 */
const NON_VENDOR_IDENTIFIER_PATTERNS = [
  /application\/vnd\.mapbox-vector-tile/g,
  // The MVT encoding is named after the company that designed it; the
  // identifier and its prose name describe a wire format, not a provider.
  /MapboxVectorTile/g,
  /Mapbox Vector/g,
];

/** The provider names the registry declares. */
function provider_name_patterns() {
  const registry = JSON.parse(readFileSync(PROVIDERS_REGISTRY_FILE, 'utf8'));
  const provider_names = new Set();
  for (const descriptor of registry.providers) {
    // A display name is a proper noun: an ordinary-word display name is
    // matched case-sensitively, every other name case-insensitively.
    const display_name_pattern = new RegExp(
      descriptor.display_name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'),
      ORDINARY_WORD_PROVIDER_KEYS.has(descriptor.provider_key) ? '' : 'i',
    );
    provider_names.add(display_name_pattern);
    if (!ORDINARY_WORD_PROVIDER_KEYS.has(descriptor.provider_key)) {
      provider_names.add(new RegExp(
        descriptor.provider_key.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'),
        'i',
      ));
    } else {
      // An ordinary-word key is still a provider name where it is written as
      // a literal: a dispatch table spells it `"here"` or `'radar'`.
      provider_names.add(new RegExp(
        `["']${descriptor.provider_key.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}["']`,
      ));
    }
    if (typeof descriptor.wasm_registration === 'string'
      && descriptor.wasm_registration.trim() !== ''
      && !ORDINARY_WORD_PROVIDER_KEYS.has(descriptor.wasm_registration)) {
      provider_names.add(new RegExp(
        descriptor.wasm_registration.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'),
        'i',
      ));
    }
  }
  return [...provider_names];
}

/** Every text file under the scanned roots, build outputs excluded. */
function scanned_text_files(directory_path) {
  const file_paths = [];
  for (const directory_entry of readdirSync(directory_path, {
    withFileTypes: true,
  })) {
    const entry_path = path.join(directory_path, directory_entry.name);
    if (directory_entry.isDirectory()) {
      if (!SKIPPED_DIRECTORY_NAMES.has(directory_entry.name)) {
        file_paths.push(...scanned_text_files(entry_path));
      }
      continue;
    }
    if (SCANNED_FILE_EXTENSIONS.has(path.extname(directory_entry.name))) {
      file_paths.push(entry_path);
    }
  }
  return file_paths;
}

/**
 * Collects every provider-name occurrence outside the documented allowlist.
 * @returns {{ file: string, line: number, text: string }[]}
 */
function collect_provider_name_violations() {
  const name_patterns = provider_name_patterns();
  const violations = [];
  const scanned_file_paths = [...SCANNED_FILES];
  for (const scanned_directory of SCANNED_DIRECTORIES) {
    scanned_file_paths.push(...scanned_text_files(scanned_directory));
  }
  for (const file_path of scanned_file_paths) {
    if (ALLOWED_FILES.has(file_path)) {
      continue;
    }
    const file_lines = readFileSync(file_path, 'utf8').split('\n');
    file_lines.forEach((file_line, line_index) => {
      let searchable_line = file_line;
      for (const alias_pattern of COMPATIBILITY_ALIAS_PATTERNS) {
        searchable_line = searchable_line.replace(alias_pattern, '');
      }
      for (const flow_hook_pattern of FLOW_HOOK_SELECTOR_PATTERNS) {
        searchable_line = searchable_line.replace(flow_hook_pattern, '');
      }
      for (const identifier_pattern of NON_VENDOR_IDENTIFIER_PATTERNS) {
        searchable_line = searchable_line.replace(identifier_pattern, '');
      }
      if (name_patterns.some((name_pattern) => name_pattern.test(searchable_line))) {
        violations.push({
          file: path.relative(REPOSITORY_ROOT, file_path),
          line: line_index + 1,
          text: file_line.trim(),
        });
      }
    });
  }
  return violations;
}

/**
 * Runs the provider-name guard; throws with every offending location. Shared
 * by the `node --test` registration and the Playwright `globalSetup`.
 */
function run_provider_name_guard_checks() {
  const violations = collect_provider_name_violations();
  assert.deepStrictEqual(
    violations,
    [],
    'Provider names may appear only in the provider crate, its fixture '
      + 'directory, `platform/providers.json` and a registration dispatch arm; '
      + `found:\n${violations
        .map((violation) => `${violation.file}:${violation.line}: ${violation.text}`)
        .join('\n')}`,
  );
}

export { collect_provider_name_violations, run_provider_name_guard_checks };

const RUNNING_AS_ENTRY = process.argv[1] !== undefined
  && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);

if (RUNNING_AS_ENTRY) {
  test('provider names stay inside the registry, provider crate and dispatch arms', () => {
    run_provider_name_guard_checks();
  });
}
