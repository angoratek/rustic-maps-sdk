#!/usr/bin/env node
// Documentation snippet-build check (PLAN.md, M18 residual).
//
// The tutorial chapters under `site/book-src/` claim that every snippet
// compiles against the frozen FFI contract. This harness makes that claim
// machine-true: it extracts every fenced code block and compiles it against
// the real generated surface for its platform.
//
// WHAT IS CHECKED
//   TypeScript/JavaScript, WebAssembly tabs
//     `tsc --noEmit` with `strict`, `allowJs` and `checkJs`, against the
//     generated contract declarations
//     `platform/web/generated/rustic_maps.d.ts` (emitted by
//     `scripts/build-wasm2-package.sh`) plus the hand-written canvas
//     declarations `platform/web/types/browser_canvas.d.ts`. When the
//     generated package is not built, callable snippets are reported as
//     unavailable and `--strict` fails.
//   TypeScript/JavaScript, React Native tabs
//     `tsc --noEmit` against the generated ubrn surface
//     `platform/rn/src/generated/rustic_maps.ts`. Absent generation is a
//     strict-mode failure and a reported local skip.
//   Swift
//     `swiftc -typecheck` against the generated `rustic_maps.swift` and
//     its `rustic_mapsFFI` header/modulemap (scripts/build-ios.sh output,
//     or a directory passed with `--swift-dir`). Package-manifest snippets
//     (`import PackageDescription`) are validated with
//     `swift package dump-package` instead, because they are not SDK code.
//   Kotlin
//     The Kotlin compiler is not part of the documented toolchain, so this is
//     deliberately NOT a type-check: the harness asserts the API surface
//     named by each snippet (imports, constructors, enum entries, and method
//     calls on receivers whose type it can infer) against the generated
//     `rustic_maps.kt`. The report labels it a symbol/API assertion.
//
// HOW FRAGMENTS BECOME COMPILABLE UNITS
//   Tutorial snippets are excerpts, not standalone files. The harness wraps
//   each one:
//     - JavaScript/TypeScript snippets get a trailing `export {}` so every
//       file is a module (top-level await and per-file names are legal), and
//       a global ambient shim declares the surrounding application context
//       (`canvas`, `mapSdk`, `route`, ...) with the real binding types where
//       possible. Snippet-local declarations shadow the shim.
//     - Swift snippets are placed inside `func __docsSnippet() async throws`,
//       `import RusticMapsSDK` lines are lifted into the wrapper preamble,
//       and the same ambient context is declared once per file.
//     - The wasm package's generated declarations type record-shaped results
//       as `object` (which TypeScript cannot read properties from), so the
//       harness widens only those return annotations to `any`; call
//       signatures stay exactly the generated ones. This is reported.
//
// SKIP MARKER
//   An illustrative block that genuinely cannot be compiled (for example a
//   build-script fragment owned by another toolchain) can be excluded with an
//   HTML comment on the line immediately before the fence:
//       <!-- docs-snippet: skip — <reason> -->
//   It is still extracted and reported, and `--strict` does not fail on an
//   explicit marker, but the skipped count is printed so markers cannot
//   accumulate unnoticed. Keep them rare.
//
// SELF-TEST
//   `--self-test` runs every enabled checker on a snippet that must pass and
//   the same snippet with an injected type/symbol error that must fail. A
//   checker that cannot distinguish the two (or that extracts nothing) fails
//   the self-test, so the suite cannot pass by doing nothing.
//
// USAGE
//   node scripts/check-doc-snippets.mjs
//   node scripts/check-doc-snippets.mjs --strict
//   node scripts/check-doc-snippets.mjs --self-test
//   node scripts/check-doc-snippets.mjs --langs js,ts --strict
//
// OPTIONS
//   --strict                Fail when a surface or toolchain is unavailable.
//   --self-test             Prove each enabled checker detects a broken snippet.
//   --langs <list>          Comma list of js,ts,swift,kotlin (default all).
//   --docs-dir <dir>        Default `site/book-src`.
//   --tsc <path>            TypeScript compiler (default: platform/rn tsc, then PATH).
//   --wasm-dts <path>       Generated wasm declaration file.
//   --rn-bindings <path>    Generated React Native TypeScript surface.
//   --swift-dir <dir>       Directory holding generated `*.swift`, `*.h` and `*.modulemap`.
//   --kotlin-file <path>    Generated Kotlin bindings file.
//   --verbose               Print every snippet result, not only failures.
//
// Exit codes: 0 pass (or pass with local skips outside --strict), 1 failures.

import { spawnSync } from 'node:child_process';
import {
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const WORK_DIR = path.join(REPO_ROOT, 'target', 'docs-snippets');

/** Fence languages mapped to the checker that owns them. */
const FENCE_LANGUAGE_ALIASES = new Map([
  ['js', 'js'],
  ['javascript', 'js'],
  ['jsx', 'js'],
  ['ts', 'ts'],
  ['typescript', 'ts'],
  ['tsx', 'ts'],
  ['swift', 'swift'],
  ['kotlin', 'kotlin'],
  ['kt', 'kotlin'],
]);

/** Every language the harness can check, in report order. */
const ALL_LANGUAGES = ['js', 'ts', 'swift', 'kotlin'];

/** Languages sharing one checker: a tutorial may fence all its JS as `js`. */
const LANGUAGE_FAMILIES = new Map([
  ['js', 'typescript'],
  ['ts', 'typescript'],
  ['swift', 'swift'],
  ['kotlin', 'kotlin'],
]);

/** Default fenced-block directory (mdBook source). */
const DEFAULT_DOCS_DIR = 'site/book-src';

/** Generated contract declarations shipped by the browser package. */
const DEFAULT_WASM_DTS = 'platform/web/generated/rustic_maps.d.ts';

/** Hand-written declarations for the browser canvas glue. */
const BROWSER_CANVAS_DTS = 'platform/web/types/browser_canvas.d.ts';

/** Generated ubrn TypeScript surface for the React Native tabs. */
const DEFAULT_RN_BINDINGS = 'platform/rn/src/generated/rustic_maps.ts';

/** Generated Swift bindings installed by scripts/build-ios.sh. */
const DEFAULT_SWIFT_BINDINGS = 'platform/ios/Sources/RusticMapsSDK/rustic_maps.swift';

/** Generated Kotlin bindings installed by scripts/build-android.sh. */
const DEFAULT_KOTLIN_BINDINGS =
  'platform/android/maps-sdk-native/src/main/kotlin/uniffi/rustic_maps/rustic_maps.kt';

/** Directory search roots for the FFI header/modulemap the Swift bindings import. */
const SWIFT_HEADER_SEARCH_ROOTS = ['platform/ios/artifacts'];

/** iOS deployment target used for type-checking (the package pins iOS 17). */
const IOS_DEPLOYMENT_TARGET = '17.0';

/** Wrapper function holding each illustrative Swift snippet. */
const SWIFT_ENTRY_FUNCTION = '__docsSnippet';

/** Placeholder values the Swift ambient context supplies to fragment code. */
const SWIFT_AMBIENT_VIEWPORT_PIXELS = 1;
const SWIFT_AMBIENT_FRAME_INTERVAL_NANOSECONDS = 16_666_667;
const SWIFT_AMBIENT_FOLLOW_ZOOM_LEVEL = 17;
const SWIFT_AMBIENT_FOLLOW_PITCH_DEGREES = 45;

/** Maximum number of compiler lines echoed per failing snippet. */
const MAX_ERROR_LINES_PER_SNIPPET = 8;

/** The skip marker recognised on the line before a fence. */
const SKIP_MARKER_PATTERN = /^\s*<!--\s*docs-snippet:\s*skip\b/i;

/** A fenced block: up to three spaces of indent, three or more backticks. */
const FENCE_PATTERN = /^(\s{0,3})(`{3,})(.*)$/;

/** Marks the panel a snippet belongs to (all tutorial snippets sit in one). */
const PLATFORM_PANEL_PATTERN = /data-platform-panel="([^"]+)"/;

// ---------------------------------------------------------------------------
// Command line
// ---------------------------------------------------------------------------

function parseArguments(argv) {
  const options = {
    docsDir: DEFAULT_DOCS_DIR,
    languages: [...ALL_LANGUAGES],
    strict: false,
    selfTest: false,
    verbose: false,
    tsc: null,
    wasmDts: null,
    rnBindings: null,
    swiftDir: null,
    kotlinFile: null,
  };
  const valueOptions = new Map([
    ['--langs', 'languages'],
    ['--docs-dir', 'docsDir'],
    ['--tsc', 'tsc'],
    ['--wasm-dts', 'wasmDts'],
    ['--rn-bindings', 'rnBindings'],
    ['--swift-dir', 'swiftDir'],
    ['--kotlin-file', 'kotlinFile'],
  ]);
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === '--strict') {
      options.strict = true;
    } else if (argument === '--self-test') {
      options.selfTest = true;
    } else if (argument === '--verbose') {
      options.verbose = true;
    } else if (argument === '--help' || argument === '-h') {
      printUsage();
      process.exit(0);
    } else if (valueOptions.has(argument)) {
      const value = argv[index + 1];
      if (value === undefined) {
        fail(`option ${argument} needs a value`);
      }
      options[valueOptions.get(argument)] = value;
      index += 1;
    } else {
      fail(`unknown option: ${argument}`);
    }
  }
  if (typeof options.languages === 'string') {
    const requested = options.languages
      .split(',')
      .map((entry) => entry.trim())
      .filter(Boolean);
    for (const language of requested) {
      if (!ALL_LANGUAGES.includes(language)) {
        fail(`unknown language "${language}" (expected ${ALL_LANGUAGES.join(', ')})`);
      }
    }
    options.languages = requested;
  }
  return options;
}

function printUsage() {
  process.stdout.write(
    'usage: node scripts/check-doc-snippets.mjs [--strict] [--self-test] ' +
      '[--langs js,ts,swift,kotlin] [--docs-dir DIR] [--tsc PATH] ' +
      '[--wasm-dts PATH] [--rn-bindings PATH] [--swift-dir DIR] ' +
      '[--kotlin-file PATH] [--verbose]\n',
  );
}

function fail(message) {
  process.stderr.write(`error: ${message}\n`);
  process.exit(2);
}

// ---------------------------------------------------------------------------
// Fenced-block extraction
// ---------------------------------------------------------------------------

/**
 * Extracts every fenced code block from the tutorial sources. Throws when a
 * fence is left open, because an unbalanced fence silently re-labels every
 * later block and would let the harness check the wrong code.
 */
function extractSnippets(docsDir) {
  const absoluteDocsDir = path.resolve(REPO_ROOT, docsDir);
  if (!existsSync(absoluteDocsDir)) {
    fail(`docs directory not found: ${absoluteDocsDir}`);
  }
  const snippets = [];
  const files = readdirSync(absoluteDocsDir)
    .filter((entry) => entry.endsWith('.md'))
    .sort();
  for (const file of files) {
    const lines = readFileSync(path.join(absoluteDocsDir, file), 'utf8').split('\n');
    let fenceMarker = null;
    let fenceLanguage = null;
    let fenceStartLine = 0;
    let fenceLines = [];
    let platform = null;
    let skipReason = null;
    for (let index = 0; index < lines.length; index += 1) {
      const line = lines[index];
      const panelMatch = line.match(PLATFORM_PANEL_PATTERN);
      if (panelMatch) {
        platform = panelMatch[1];
      }
      const fenceMatch = line.match(FENCE_PATTERN);
      if (!fenceMatch) {
        if (fenceMarker) {
          fenceLines.push(stripFenceIndent(line));
        }
        continue;
      }
      const [, indent, marker, info] = fenceMatch;
      if (fenceMarker === null) {
        fenceMarker = marker;
        fenceLanguage = normalizeFenceLanguage(info);
        fenceStartLine = index + 2; // 1-based line of the first code line.
        fenceLines = [];
        const previousLine = index > 0 ? lines[index - 1] : '';
        skipReason = SKIP_MARKER_PATTERN.test(previousLine)
          ? previousLine.trim().replace(/^<!--\s*docs-snippet:\s*skip\s*/i, '').replace(/\s*-->$/, '')
          : null;
      } else if (marker.length >= fenceMarker.length && info.trim() === '') {
        snippets.push({
          file: path.join(docsDir, file),
          line: fenceStartLine,
          language: fenceLanguage,
          platform,
          code: trimBlankEdges(fenceLines).join('\n'),
          skipReason,
        });
        fenceMarker = null;
        fenceLanguage = null;
        fenceLines = [];
        skipReason = null;
      } else {
        fenceLines.push(stripFenceIndent(line));
      }
      // `indent` is intentionally unused beyond pattern validation.
      void indent;
    }
    if (fenceMarker !== null) {
      throw new Error(
        `${path.join(docsDir, file)}:${fenceStartLine}: unterminated fenced block ` +
          '(a stray closing fence re-labels every later block)',
      );
    }
  }
  return snippets;
}

function stripFenceIndent(line) {
  return line.replace(/^\s{0,3}/, '');
}

function normalizeFenceLanguage(info) {
  const token = info.trim().split(/\s+/)[0]?.toLowerCase() ?? '';
  return FENCE_LANGUAGE_ALIASES.get(token) ?? null;
}

function trimBlankEdges(lines) {
  let start = 0;
  let end = lines.length;
  while (start < end && lines[start].trim() === '') {
    start += 1;
  }
  while (end > start && lines[end - 1].trim() === '') {
    end -= 1;
  }
  return lines.slice(start, end);
}

// ---------------------------------------------------------------------------
// Surface and toolchain discovery
// ---------------------------------------------------------------------------

function which(command) {
  const result = spawnSync('sh', ['-c', `command -v ${shellQuote(command)}`], {
    encoding: 'utf8',
  });
  if (result.status !== 0) {
    return null;
  }
  return result.stdout.trim() || null;
}

function shellQuote(value) {
  return `'${String(value).replace(/'/g, `'\\''`)}'`;
}

function existingPath(relative) {
  const absolute = path.isAbsolute(relative) ? relative : path.resolve(REPO_ROOT, relative);
  return existsSync(absolute) ? absolute : null;
}

function discoverEnvironment(options) {
  const environment = {
    tsc: options.tsc
      ? (existingPath(options.tsc) ?? which(options.tsc))
      : (existingPath('platform/rn/node_modules/.bin/tsc') ?? which('tsc')),
    wasmDts: options.wasmDts
      ? existingPath(options.wasmDts)
      : existingPath(DEFAULT_WASM_DTS),
    browserCanvasDts: existingPath(BROWSER_CANVAS_DTS),
    rnBindings: options.rnBindings
      ? existingPath(options.rnBindings)
      : existingPath(DEFAULT_RN_BINDINGS),
    swiftc: which('swiftc'),
    swift: which('swift'),
    xcrun: which('xcrun'),
    swiftSurface: discoverSwiftSurface(options.swiftDir),
    kotlinBindings: options.kotlinFile
      ? findKotlinFile(options.kotlinFile)
      : existingPath(DEFAULT_KOTLIN_BINDINGS),
  };
  return environment;
}

function discoverSwiftSurface(swiftDir) {
  if (!swiftDir) {
    const bindings = existingPath(DEFAULT_SWIFT_BINDINGS);
    if (!bindings) {
      return null;
    }
    const headerDirectory = findSwiftHeaderDirectory();
    return headerDirectory ? { bindings, headers: headerDirectory } : null;
  }
  const directory = existingPath(swiftDir);
  if (!directory) {
    return null;
  }
  const swiftFile = findFileWithExtension(directory, '.swift');
  const headerFile = findFileWithExtension(directory, '.h');
  const moduleMap = findFileWithExtension(directory, '.modulemap');
  if (!swiftFile || !headerFile || !moduleMap) {
    return null;
  }
  return { bindings: swiftFile, headers: directory };
}

function findKotlinFile(candidate) {
  const resolved = existingPath(candidate);
  if (!resolved) {
    return null;
  }
  if (resolved.endsWith('.kt')) {
    return resolved;
  }
  return findFileWithExtension(resolved, '.kt');
}

function findFileWithExtension(directory, extension) {
  const entries = readdirSync(directory, { withFileTypes: true });
  for (const entry of entries) {
    const entryPath = path.join(directory, entry.name);
    if (entry.isFile() && entry.name.endsWith(extension)) {
      return entryPath;
    }
    if (entry.isDirectory()) {
      const nested = findFileWithExtension(entryPath, extension);
      if (nested) {
        return nested;
      }
    }
  }
  return null;
}

function findSwiftHeaderDirectory() {
  for (const root of SWIFT_HEADER_SEARCH_ROOTS) {
    const absoluteRoot = existingPath(root);
    if (!absoluteRoot) {
      continue;
    }
    const headers = findDirectoryContaining(absoluteRoot, [
      'rustic_mapsFFI.h',
      'module.modulemap',
    ]);
    if (headers) {
      return headers;
    }
  }
  return null;
}

/** Depth-first search for a directory holding every named file. */
function findDirectoryContaining(directory, fileNames) {
  let entries;
  try {
    entries = readdirSync(directory, { withFileTypes: true });
  } catch {
    return null;
  }
  if (fileNames.every((fileName) => existsSync(path.join(directory, fileName)))) {
    return directory;
  }
  for (const entry of entries) {
    if (entry.isDirectory()) {
      const found = findDirectoryContaining(path.join(directory, entry.name), fileNames);
      if (found) {
        return found;
      }
    }
  }
  return null;
}

// ---------------------------------------------------------------------------
// Result bookkeeping
// ---------------------------------------------------------------------------

function createReport() {
  return { results: [], surfaceNotes: [] };
}

function record(
  report,
  snippet,
  checker,
  status,
  detail,
  { command = null, output = null } = {},
) {
  report.results.push({
    file: snippet.file,
    line: snippet.line,
    language: snippet.language,
    platform: snippet.platform,
    checker,
    status,
    detail,
    command,
    output,
  });
}

function statusIsFailure(status) {
  return status === 'fail';
}

function statusIsUnavailable(status) {
  return status === 'unavailable';
}

// ---------------------------------------------------------------------------
// TypeScript / JavaScript
// ---------------------------------------------------------------------------

/**
 * Wraps snippets into one TypeScript project per surface and type-checks the
 * whole project with `tsc --noEmit`.
 */
function checkTypeScriptSurface(report, snippets, environment, surface) {
  if (snippets.length === 0) {
    return;
  }
  if (!environment.tsc) {
    for (const snippet of snippets) {
      record(report, snippet, `typescript-${surface}`, 'unavailable', 'TypeScript compiler not found');
    }
    return;
  }
  const projectDir = path.join(WORK_DIR, `ts-${surface}`);
  mkdirSync(projectDir, { recursive: true });
  const fileMap = new Map();
  for (const snippet of snippets) {
    const fileName = `${path.basename(snippet.file, '.md')}-${snippet.line}.${snippet.language === 'ts' ? 'ts' : 'js'}`;
    fileMap.set(path.join(projectDir, fileName), snippet);
    writeFileSync(path.join(projectDir, fileName), `${snippet.code}\n\nexport {};\n`);
  }

  const compilerOptions = {
    target: 'ES2022',
    lib: ['ES2022', 'DOM', 'DOM.Iterable'],
    module: 'ESNext',
    moduleResolution: 'Bundler',
    allowJs: true,
    checkJs: true,
    noEmit: true,
    strict: true,
    // Tutorial fragments pass surrounding context values into untyped
    // callback parameters; the surface-level checks (imports, constructors,
    // member names, argument types of declared calls) stay in force.
    noImplicitAny: false,
    skipLibCheck: true,
    types: [],
    forceConsistentCasingInFileNames: true,
  };

  if (surface === 'wasm') {
    mkdirSync(path.join(projectDir, 'web'), { recursive: true });
    const declarations = readFileSync(environment.wasmDts, 'utf8');
    const widened = widenRecordReturnTypes(declarations);
    writeFileSync(path.join(projectDir, 'web', 'rustic_maps.d.ts'), widened);
    writeFileSync(path.join(projectDir, 'web', 'index.d.ts'), wasmIndexShim);
    if (environment.browserCanvasDts) {
      writeFileSync(
        path.join(projectDir, 'web', 'rustic_maps_bg.d.ts'),
        readFileSync(environment.browserCanvasDts, 'utf8'),
      );
    }
    writeFileSync(path.join(projectDir, 'ambient.d.ts'), wasmAmbientShim);
    report.surfaceNotes.push(
      `wasm TypeScript surface: ${path.relative(REPO_ROOT, environment.wasmDts)} ` +
        '(record-shaped `object` returns widened to `any`; call signatures unchanged)',
    );
  } else {
    writeFileSync(path.join(projectDir, 'ambient.d.ts'), reactNativeAmbientShim);
    compilerOptions.baseUrl = '.';
    compilerOptions.paths = {
      '@angoratek/rustic-maps-sdk': [environment.rnBindings],
      // The package's hand-written SDK handle is a subpath export, not part
      // of the generated root surface (`src/index.ts` re-exports only the
      // generated bindings). Check it against its shipped source.
      '@angoratek/rustic-maps-sdk/MapsSdk': [
        path.resolve(REPO_ROOT, 'platform/rn/src/MapsSdk.ts'),
      ],
    };
    report.surfaceNotes.push(
      `react-native TypeScript surface: ${path.relative(REPO_ROOT, environment.rnBindings)}`,
    );
  }

  writeFileSync(
    path.join(projectDir, 'tsconfig.json'),
    `${JSON.stringify(
      {
        compilerOptions,
        include: ['*.js', '*.ts', '*.d.ts'],
      },
      null,
      2,
    )}\n`,
  );

  const command = [environment.tsc, '-p', path.join(projectDir, 'tsconfig.json'), '--pretty', 'false'];
  const result = spawnSync(command[0], command.slice(1), { encoding: 'utf8' });
  const output = `${result.stdout ?? ''}${result.stderr ?? ''}`.trim();
  const errorsByFile = parseTypeScriptErrors(output);

  for (const [filePath, snippet] of fileMap) {
    const errors = errorsByFile.get(path.resolve(filePath)) ?? [];
    if (errors.length === 0) {
      record(report, snippet, `typescript-${surface}`, 'pass', 'type-checked', {
        command: command.join(' '),
      });
    } else {
      record(report, snippet, `typescript-${surface}`, 'fail', errors.join('\n'), {
        command: command.join(' '),
      });
    }
  }

  const sharedErrors = output
    .split('\n')
    .filter((line) => line.includes('error TS') && !TYPESCRIPT_ERROR_PATTERN.test(line));
  if (sharedErrors.length > 0) {
    for (const snippet of snippets) {
      const existing = report.results.find(
        (entry) => entry.file === snippet.file && entry.line === snippet.line,
      );
      if (existing && existing.status !== 'fail') {
        existing.status = 'fail';
        existing.detail = `harness surface failed:\n${sharedErrors.slice(0, MAX_ERROR_LINES_PER_SNIPPET).join('\n')}`;
      }
    }
  }
}

const TYPESCRIPT_ERROR_PATTERN = /^(.+?)\((\d+),(\d+)\): error (TS\d+): (.*)$/;

function parseTypeScriptErrors(output) {
  const errorsByFile = new Map();
  for (const line of output.split('\n')) {
    const match = line.match(TYPESCRIPT_ERROR_PATTERN);
    if (!match) {
      continue;
    }
    const [, file, row, column, code, message] = match;
    // tsc prints paths relative to its working directory (the repo root).
    const resolved = path.resolve(process.cwd(), file);
    const entry = `${path.basename(file)}:${row}:${column}: ${code}: ${message}`;
    const list = errorsByFile.get(resolved) ?? [];
    list.push(entry);
    errorsByFile.set(resolved, list);
  }
  return errorsByFile;
}

/**
 * The generated wasm declarations describe record-shaped results as `object`;
 * TypeScript refuses property reads on `object`, which would make every
 * documented record read a false failure. Only those return annotations are
 * widened; every callable signature remains exactly the generated one.
 */
function widenRecordReturnTypes(declarations) {
  return declarations
    .replace(/\):\s*object;/g, '): any;')
    .replace(/Promise<object>/g, 'Promise<any>');
}

const wasmIndexShim = `
export * from './rustic_maps.js';
export declare function uniffiInitAsync(source: any): Promise<void>;
`;

const wasmAmbientShim = `
import type * as wasm from './web/rustic_maps.js';
import type * as wasmCanvas from './web/rustic_maps_bg.js';

// The route records the generated declarations describe structurally; the
// tutorial reads exactly these fields through \`route.legs\`, so the ambient
// context supplies the documented shape.
type DocsRouteStep = { shape: any[] };
type DocsRouteLeg = { steps: DocsRouteStep[] };
type DocsRoute = {
  legs: DocsRouteLeg[];
  distanceMeters?: number;
  durationSeconds?: number;
  boundingBox?: any;
};

// Surrounding-page context for illustrative WebAssembly snippets. Names
// declared inside a snippet shadow these global declarations; the contract
// classes come from the generated surface and the canvas classes from the
// wasm-bindgen glue, so snippets that rely on an earlier tutorial step are
// still checked.
declare global {
  const MapCamera: typeof wasmCanvas.MapCamera;
  const BrowserMapView: typeof wasmCanvas.BrowserMapView;
  const TileResolution: typeof wasmCanvas.TileResolution;
  const MapSdk: typeof wasm.MapSdk;
  const Navigator: typeof wasm.Navigator;
  const RouteSimulator: typeof wasm.RouteSimulator;
  const PositionController: typeof wasm.PositionController;
  const ProviderContract: typeof wasm.ProviderContract;
  const UnitSystem: typeof wasm.UnitSystem;
  const defaultStepAdvancePolicy: typeof wasm.defaultStepAdvancePolicy;
  const defaultDeviationPolicy: typeof wasm.defaultDeviationPolicy;
  const devicePixelRatioThreshold: typeof wasmCanvas.devicePixelRatioThreshold;

  const canvas: HTMLCanvasElement;
  const canvasElement: HTMLCanvasElement;
  const camera: wasmCanvas.MapCamera;
  const mapView: wasmCanvas.BrowserMapView;
  const mapSdk: wasm.MapSdk;
  const fieldOfViewDegrees: number;
  const latitudeDegrees: number;
  const longitudeDegrees: number;
  const tileZoom: number;
  const deltaXPixels: number;
  const deltaYPixels: number;
  const providerCredential: string;
  const originLatitude: number;
  const originLongitude: number;
  const startLatitude: number;
  const startLongitude: number;
  const endLatitude: number;
  const endLongitude: number;
  const route: any;
  const fix: { latitude: number; longitude: number };
  const smoothedCourseHeadingDegrees: number;
}
export {};
`;

const reactNativeAmbientShim = `
import type * as rn from '@angoratek/rustic-maps-sdk';

// Surrounding-application context for illustrative React Native snippets.
// Values that cross the bridge keep their generated types so method calls are
// checked; purely local application state stays permissive.
declare global {
  const MapView: typeof rn.MapView;
  const MapCamera: typeof rn.MapCamera;
  const MapOverlayKind: typeof rn.MapOverlayKind;
  const Navigator: typeof rn.Navigator;
  const UnitSystem: typeof rn.UnitSystem;
  const defaultStepAdvancePolicy: typeof rn.defaultStepAdvancePolicy;
  const defaultDeviationPolicy: typeof rn.defaultDeviationPolicy;
  const camera: rn.MapCamera;
  // The package's MapView component handle (platform/rn/src/MapView.tsx),
  // not the generated contract view: it addresses the native view with the
  // five-value camera pose, and the contract view's raw draw is not on it.
  const mapView: {
    setCamera(camera: DocsMapCameraPose): void;
    animateCamera(camera: DocsMapCameraPose, durationMilliseconds?: number): void;
    fitBounds(
      boundingBox: rn.BoundingBox,
      edgeInsets?: rn.EdgeInsets,
      maximumZoomLevel?: number,
    ): void;
    setOverlays(overlays: ReadonlyArray<rn.MapOverlay>): Promise<boolean>;
    setLocationIndicator(
      state: rn.LocationIndicatorState,
      style: rn.LocationIndicatorStyle,
    ): void;
  };
  const navigationController: rn.Navigator;
  const mapSdk: rn.MapSdk;
  const offlineManager: rn.OfflineManager;
  const region: rn.RegionDefinition;
  const MapSdk: typeof rn.MapSdk;
  const TransportationMode: typeof rn.TransportationMode;
  const RouteStrategy: typeof rn.RouteStrategy;
  const listener: any;
  const providerApiKey: string;
  const route: rn.Route;
  const routeOptions: rn.RouteOptions;
  const originSearchQuery: string;
  const destinationSearchQuery: string;
  const originPlace: rn.Place;
  const destinationPlace: rn.Place;
  const plannedRoute: rn.Route;
  const waypoints: rn.GeographicCoordinate[];
  const viewport: rn.ViewportSize;
  const visibleRegionBoundingBox: rn.BoundingBox | undefined;
  const destination: rn.Place;
  const fix: any;
  const tileCacheDatabasePath: string;
  const timeToLiveSeconds: bigint;
  const dragDeltaXPixels: number;
  const dragDeltaYPixels: number;
  const fieldOfViewDegrees: number;
  const widthPixels: number;
  const heightPixels: number;
  const sampleLatitude: number;
  const sampleLongitude: number;
  const originLatitude: number;
  const originLongitude: number;
  const smoothedCourseHeadingDegrees: number;
  const Speech: any;
  function markUtteranceComplete(utteranceId: any): void;
  function setNextManeuver(value: any): void;
  function speak(text: string): void;
  function setDeviation(value: any): void;
  function setPhase(value: any): void;

  // The record types the worked example annotates its locals with.
  type RouteOptions = rn.RouteOptions;
  type MapOverlay = rn.MapOverlay;

  // The camera pose the package MapView handle adopts and reads back.
  type DocsMapCameraPose = {
    latitudeDegrees: number;
    longitudeDegrees: number;
    zoomLevel: number;
    bearingDegrees: number;
    pitchDegrees: number;
  };
}
export {};
`;

// ---------------------------------------------------------------------------
// Swift
// ---------------------------------------------------------------------------

function checkSwiftSnippets(report, snippets, environment) {
  if (snippets.length === 0) {
    return;
  }
  const prepared = prepareSwiftTypeCheck(environment);
  if (!prepared.error) {
    report.surfaceNotes.push(
      `swift surface: ${path.relative(REPO_ROOT, environment.swiftSurface.bindings)} ` +
        `(target ${prepared.target})`,
    );
    prepared.swiftc = environment.swiftc;
  }

  for (const snippet of snippets) {
    if (isSwiftPackageManifest(snippet.code)) {
      // Manifests are validated by SwiftPM and do not need the SDK bindings.
      checkSwiftManifest(report, snippet, environment);
    } else if (prepared.error) {
      record(report, snippet, 'swift', 'unavailable', prepared.error);
    } else {
      checkSwiftSnippet(report, snippet, prepared);
    }
  }
}

function prepareSwiftTypeCheck(environment) {
  if (!environment.swiftc) {
    return { error: 'swiftc not found' };
  }
  if (!environment.xcrun) {
    return { error: 'xcrun not found (needed for the iOS SDK)' };
  }
  if (!environment.swiftSurface) {
    return { error: 'generated Swift bindings not found' };
  }
  const sdk = runCapture(environment.xcrun, ['--sdk', 'iphonesimulator', '--show-sdk-path']);
  if (sdk.status !== 0 || !sdk.stdout.trim()) {
    return { error: 'the iOS Simulator SDK is not available' };
  }
  const architecture = process.arch === 'arm64' ? 'arm64' : 'x86_64';
  const target = `${architecture}-apple-ios${IOS_DEPLOYMENT_TARGET}-simulator`;
  const headers = stageSwiftHeaders(environment.swiftSurface.headers);
  const moduleDir = path.join(WORK_DIR, 'swift-module');
  mkdirSync(moduleDir, { recursive: true });
  const modulePath = path.join(moduleDir, 'RusticMapsSDK.swiftmodule');
  const command = [
    environment.swiftc,
    '-parse-as-library',
    '-emit-module',
    '-module-name',
    'RusticMapsSDK',
    '-sdk',
    sdk.stdout.trim(),
    '-target',
    target,
    '-I',
    headers,
    '-emit-module-path',
    modulePath,
    environment.swiftSurface.bindings,
  ];
  const result = spawnSync(command[0], command.slice(1), { encoding: 'utf8' });
  if (result.status !== 0) {
    return {
      error: `could not build the generated Swift module:\n${(
        result.stderr ?? ''
      ).slice(0, MAX_ERROR_LINES_PER_SNIPPET * 200)}`,
    };
  }
  return { moduleDir, sdk: sdk.stdout.trim(), target, headers };
}

/**
 * Swift resolves a module map named `module.modulemap` in a `-I` directory;
 * uniffi emits `<crate>FFI.modulemap` unless the packaging step renames it.
 * Stage a small header directory so a freshly generated `--swift-dir` works
 * exactly like the packaged XCFramework headers.
 */
function stageSwiftHeaders(headersDirectory) {
  if (existsSync(path.join(headersDirectory, 'module.modulemap'))) {
    return headersDirectory;
  }
  const moduleMap = findFileWithExtension(headersDirectory, '.modulemap');
  if (!moduleMap) {
    return headersDirectory;
  }
  const staged = path.join(WORK_DIR, 'swift-headers');
  mkdirSync(staged, { recursive: true });
  for (const entry of readdirSync(headersDirectory)) {
    if (entry.endsWith('.h')) {
      cpSync(path.join(headersDirectory, entry), path.join(staged, entry));
    }
  }
  cpSync(moduleMap, path.join(staged, 'module.modulemap'));
  return staged;
}

function isSwiftPackageManifest(code) {
  return /^\s*import\s+PackageDescription\b/m.test(code);
}

function checkSwiftManifest(report, snippet, environment) {
  if (!environment.swift) {
    record(report, snippet, 'swift-manifest', 'unavailable', 'swift not found');
    return;
  }
  const directory = path.join(WORK_DIR, `manifest-${safeName(snippet)}`);
  mkdirSync(directory, { recursive: true });
  // The first line is a documentation label ("// Package.swift"); SwiftPM
  // requires `// swift-tools-version:` on line one.
  const manifest = snippet.code.replace(/^\s*\/\/\s*Package\.swift\s*\n/, '');
  writeFileSync(path.join(directory, 'Package.swift'), `${manifest}\n`);
  const command = [environment.swift, 'package', 'dump-package', '--package-path', directory];
  const result = spawnSync(command[0], command.slice(1), { encoding: 'utf8' });
  const output = `${result.stdout ?? ''}${result.stderr ?? ''}`.trim();
  if (result.status === 0) {
    record(report, snippet, 'swift-manifest', 'pass', 'validated by swift package dump-package', {
      command: command.join(' '),
    });
  } else {
    record(report, snippet, 'swift-manifest', 'fail', output, {
      command: command.join(' '),
      output,
    });
  }
}

function checkSwiftSnippet(report, snippet, prepared) {
  const directory = path.join(WORK_DIR, `swift-${safeName(snippet)}`);
  mkdirSync(directory, { recursive: true });
  const filePath = path.join(directory, 'snippet.swift');
  const body = snippet.code
    .split('\n')
    .filter((line) => !/^\s*import\s+RusticMapsSDK\s*$/.test(line))
    .join('\n');
  writeFileSync(
    filePath,
    [
      'import Foundation',
      'import AVFoundation',
      'import UIKit',
      'import RusticMapsSDK',
      '',
      swiftAmbientShim,
      '',
      `func ${SWIFT_ENTRY_FUNCTION}() async throws {`,
      body,
      '}',
      '',
    ].join('\n'),
  );
  const command = [
    prepared.swiftc,
    '-typecheck',
    '-sdk',
    prepared.sdk,
    '-target',
    prepared.target,
    '-I',
    prepared.moduleDir,
    '-I',
    prepared.headers,
    filePath,
  ];
  const result = spawnSync(command[0], command.slice(1), { encoding: 'utf8' });
  const output = `${result.stdout ?? ''}${result.stderr ?? ''}`.trim();
  if (result.status === 0) {
    record(report, snippet, 'swift', 'pass', 'type-checked', {
      command: command.join(' '),
    });
  } else {
    record(report, snippet, 'swift', 'fail', firstErrorLines(output), {
      command: command.join(' '),
      output,
    });
  }
}

const swiftAmbientShim = `
// Surrounding-application context for illustrative Swift snippets. Values are
// typed from the generated bindings where the tutorial's surrounding code
// would have produced them, so member calls are checked; snippet-local
// declarations shadow these globals.
func __docsSnippetUnavailable<T>() -> T {
  fatalError("docs-snippet context is supplied by the surrounding application")
}

let providerApiKey: String = __docsSnippetUnavailable()
let mapSdk: MapSdk = __docsSnippetUnavailable()
let mapSurface: MapView = __docsSnippetUnavailable()
var mapView: DocsShippedMapView = DocsShippedMapView()
let originSearchQuery: String = ""
let destinationSearchQuery: String = ""
let originPlace: Place = __docsSnippetUnavailable()
let destinationPlace: Place = __docsSnippetUnavailable()
let plannedRoute: Route = __docsSnippetUnavailable()
let camera: MapCamera = __docsSnippetUnavailable()
let navigationController: Navigator = __docsSnippetUnavailable()
let route: Route = __docsSnippetUnavailable()
let routeOptions: RouteOptions = __docsSnippetUnavailable()
let waypoints: [GeographicCoordinate] = __docsSnippetUnavailable()
let originCoordinate: GeographicCoordinate = __docsSnippetUnavailable()
let destination: Place = __docsSnippetUnavailable()
let destinationCoordinate: GeographicCoordinate = __docsSnippetUnavailable()
let viewport = ViewportSize(
  widthPixels: ${SWIFT_AMBIENT_VIEWPORT_PIXELS},
  heightPixels: ${SWIFT_AMBIENT_VIEWPORT_PIXELS}
)
let regionBox: BoundingBox = __docsSnippetUnavailable()
let region: RegionDefinition = __docsSnippetUnavailable()
let offlineManager: OfflineManager = __docsSnippetUnavailable()
let searchBoundary: BoundingBox? = nil
let timeToLiveSeconds: UInt64 = 0
let fieldOfViewDegrees: Double = 60
let dragDeltaPixels = CGSize(width: 0, height: 0)
let pixelExtent = CGSize(width: 1, height: 1)
let metalBackedView: UIView = __docsSnippetUnavailable()
var pinnedFitBoundsCamera: MapCamera? = nil
let frameIntervalNanoseconds: UInt64 = ${SWIFT_AMBIENT_FRAME_INTERVAL_NANOSECONDS}
let currentFixCoordinate: GeographicCoordinate = __docsSnippetUnavailable()
let originalDestination: GeographicCoordinate = __docsSnippetUnavailable()
let followZoomLevel: Double = ${SWIFT_AMBIENT_FOLLOW_ZOOM_LEVEL}
let smoothedCourseHeadingDegrees: Double = 0
let followPitchDegrees: Double = ${SWIFT_AMBIENT_FOLLOW_PITCH_DEGREES}
let locationIndicatorState: LocationIndicatorState = __docsSnippetUnavailable()
let locationIndicatorStyle: LocationIndicatorStyle = __docsSnippetUnavailable()
let tripState: TripNavigationState = __docsSnippetUnavailable()
let fix: RecordedFix = __docsSnippetUnavailable()
let speechInstruction: SpeechInstruction = __docsSnippetUnavailable()

struct RecordedFix {
  let coordinate: GeographicCoordinate
  let horizontalAccuracyMeters: Double
  let courseHeadingDegrees: Double?
}

/// Mirrors the shipped \`MapNativeCoreUI\` wrapper for the members the guides
/// call. That package is a separate module the type-check does not build, so
/// these declarations carry the same signatures; a change to
/// \`platform/ios/Sources/MapNativeCoreUI/MapView.swift\` must be mirrored here.
struct MapCameraPose {
  let latitudeDegrees: Double
  let longitudeDegrees: Double
  let zoomLevel: Double
  let bearingDegrees: Double
  let pitchDegrees: Double
}

struct DocsShippedMapView {
  var sdk: MapSdk?
  func setCamera(_ cameraPose: MapCameraPose) {}
  func setOverlays(_ overlays: [MapOverlay]) -> Bool { false }
  func setLocationIndicator(state: LocationIndicatorState, style: LocationIndicatorStyle) {}
}

/// The worked example's one refusal (the reference application's own type):
/// a plan field whose search returned no place.
enum WalkthroughScenarioError: Error {
  case planFieldResolvedNoPlace
}

func updateReadouts(_ statistics: FrameStatistics) {}
func updateReadouts(statistics: FrameStatistics) {}
func updateReadouts(_ state: TripNavigationState) {}
func planRoutePreview(_ route: Route) throws {}
`;

function firstErrorLines(output) {
  const lines = output.split('\n').filter((line) => line.includes('error:'));
  if (lines.length === 0) {
    return output.slice(0, 2000);
  }
  return lines.slice(0, MAX_ERROR_LINES_PER_SNIPPET).join('\n');
}

// ---------------------------------------------------------------------------
// Kotlin (symbol/API assertion, not a type-check)
// ---------------------------------------------------------------------------

function checkKotlinSnippets(report, snippets, environment) {
  if (snippets.length === 0) {
    return;
  }
  if (!environment.kotlinBindings) {
    for (const snippet of snippets) {
      record(report, snippet, 'kotlin-symbols', 'unavailable', 'generated Kotlin bindings not found');
    }
    return;
  }
  const model = parseKotlinBindings(readFileSync(environment.kotlinBindings, 'utf8'));
  report.surfaceNotes.push(
    `kotlin surface: ${path.relative(REPO_ROOT, environment.kotlinBindings)} ` +
      '(symbol/API assertion; no Kotlin compiler in this toolchain)',
  );
  for (const snippet of snippets) {
    const issues = assertKotlinSurface(model, snippet.code);
    if (issues.notApplicable) {
      record(report, snippet, 'kotlin-symbols', 'not-applicable', issues.detail);
    } else if (issues.failures.length === 0) {
      record(report, snippet, 'kotlin-symbols', 'pass', issues.detail);
    } else {
      record(report, snippet, 'kotlin-symbols', 'fail', issues.failures.join('\n'));
    }
  }
}

/** Parses the generated Kotlin declarations into types, members and enums. */
function parseKotlinBindings(source) {
  const model = {
    types: new Set(),
    members: new Map(),
    supertypes: new Map(),
    enumEntries: new Map(),
    functions: new Set(),
  };
  let depth = 0;
  const stack = [];
  let blockComment = false;
  const lines = source.split('\n');
  for (const rawLine of lines) {
    const line = stripKotlinComments(rawLine, blockComment, (next) => {
      blockComment = next;
    });
    if (line.trim() === '') {
      continue;
    }
    const opensType = line.match(
      /^\s*(?:public\s+|internal\s+|open\s+|abstract\s+|sealed\s+|data\s+|value\s+|enum\s+)*(class|interface|object)\s+([A-Za-z_]\w*)/,
    );
    const isEnum = /^\s*(?:public\s+|internal\s+)?enum\s+class\s+/.test(line);
    if (opensType) {
      const [, kind, name] = opensType;
      const isCompanion = /\bcompanion\s+object\b/.test(line);
      if (!isCompanion) {
        model.types.add(name);
        if (kind === 'class' || kind === 'interface' || kind === 'object') {
          const inheritance = line.split(':').slice(1).join(':');
          const supertypes = [];
          if (inheritance) {
            for (const candidate of inheritance.split(/[,\s{]+/)) {
              const cleaned = candidate.trim();
              if (/^[A-Z]\w*$/.test(cleaned)) {
                supertypes.push(cleaned);
              }
            }
          }
          model.supertypes.set(name, supertypes);
        }
        if (isEnum) {
          model.enumEntries.set(name, new Set());
        }
        // Generated declarations put the opening brace on the next line
        // (`open class MapSdk: ...` then `{`), so an entry only becomes
        // poppable once the depth has actually reached its body.
        stack.push({ name, depth: depth + 1, isEnum, entered: false });
        model.members.set(name, new Set());
      }
    } else if (depth === 0) {
      const topFunction = line.match(
        /^\s*(?:@\w+(?:\([^)]*\))?\s*)*(?:public\s+|internal\s+|suspend\s+)*fun\s+(?:<[^>]*>\s*)?`?(\w+)`?\s*\(/,
      );
      if (topFunction) {
        model.functions.add(topFunction[1]);
      }
    }
    const memberFunction = line.match(
      /^\s*(?:@\w+(?:\([^)]*\))?\s*)*(?:public\s+|internal\s+|open\s+|override\s+|suspend\s+|inline\s+)*fun\s+(?:<[^>]*>\s*)?`?(\w+)`?\s*\(/,
    );
    const memberValue = line.match(
      /^\s*(?:public\s+|internal\s+|open\s+|override\s+)*(?:val|var)\s+(\w+)\s*:/,
    );
    const current = stack[stack.length - 1];
    if (current) {
      if (memberFunction) {
        model.members.get(current.name)?.add(memberFunction[1]);
      }
      if (memberValue) {
        model.members.get(current.name)?.add(memberValue[1]);
      }
      if (current.isEnum && !opensType) {
        // Generated enum entries sit on their own lines (`METRIC,`); the
        // declaration line is skipped so the type name is not read as one.
        const entry = line.match(/^\s*([A-Z][A-Z0-9_]*)\s*[,;]?\s*$/);
        if (entry) {
          model.enumEntries.get(current.name)?.add(entry[1]);
        }
      }
    }
    const opens = (line.match(/\{/g) ?? []).length;
    const closes = (line.match(/\}/g) ?? []).length;
    depth += opens - closes;
    if (stack.length > 0 && depth >= stack[stack.length - 1].depth) {
      stack[stack.length - 1].entered = true;
    }
    while (
      stack.length > 0 &&
      stack[stack.length - 1].entered &&
      depth < stack[stack.length - 1].depth
    ) {
      stack.pop();
    }
  }
  return model;
}

function stripKotlinComments(line, blockCommentActive, setBlockComment) {
  let active = blockCommentActive;
  let output = '';
  for (let index = 0; index < line.length; index += 1) {
    const character = line[index];
    const next = line[index + 1];
    if (active) {
      if (character === '*' && next === '/') {
        active = false;
        index += 1;
      }
      continue;
    }
    if (character === '/' && next === '/') {
      break;
    }
    if (character === '/' && next === '*') {
      active = true;
      index += 1;
      continue;
    }
    if (character === '"' || character === "'") {
      const quote = character;
      output += character;
      index += 1;
      while (index < line.length) {
        output += line[index];
        if (line[index] === '\\') {
          index += 1;
          if (index < line.length) {
            output += line[index];
          }
        } else if (line[index] === quote) {
          break;
        }
        index += 1;
      }
      continue;
    }
    output += character;
  }
  setBlockComment(active);
  return output;
}

function assertKotlinSurface(model, code) {
  const stripped = stripAllKotlinComments(code);
  const failures = [];
  let assertions = 0;

  const declaredNames = new Set();
  for (const match of stripped.matchAll(/\b(?:class|interface|object)\s+([A-Za-z_]\w*)/g)) {
    declaredNames.add(match[1]);
  }

  // Imports are the strongest statement the docs make about the contract.
  for (const match of stripped.matchAll(/^\s*import\s+uniffi\.rustic_maps\.(\w+)/gm)) {
    assertions += 1;
    const name = match[1];
    if (!model.types.has(name) && !model.functions.has(name)) {
      failures.push(
        `line ${lineOf(stripped, match.index)}: import uniffi.rustic_maps.${name} is not a generated symbol`,
      );
    }
  }

  // Constructors: capitalised calls that are not local application types.
  for (const match of stripped.matchAll(/\b([A-Z]\w*)\s*\(/g)) {
    const name = match[1];
    if (declaredNames.has(name) || NON_SDK_CONSTRUCTORS.has(name)) {
      continue;
    }
    if (model.types.has(name)) {
      assertions += 1;
      continue;
    }
    // Unknown capitals are most likely application/stdlib types; only assert
    // names that look like they came from the SDK through a nearby import or
    // a known receiver.
    if (importedNames(stripped).has(name)) {
      assertions += 1;
      failures.push(
        `line ${lineOf(stripped, match.index)}: constructor ${name}( is not a generated type`,
      );
    }
  }

  const receiverTypes = inferKotlinReceiverTypes(model, stripped);
  for (const match of stripped.matchAll(/\b([a-z_]\w*)\.(\w+)\s*\(/g)) {
    const [, receiver, method] = match;
    const type = receiverTypes.get(receiver);
    if (!type) {
      continue;
    }
    const available = memberSet(model, type);
    if (!available) {
      continue;
    }
    assertions += 1;
    if (!available.has(method)) {
      failures.push(
        `line ${lineOf(stripped, match.index)}: ${type}.${method}( is not a generated member`,
      );
    }
  }

  // Static calls, e.g. MapCamera.fitBounds(...).
  for (const match of stripped.matchAll(/\b([A-Z]\w*)\.(\w+)\s*\(/g)) {
    const [, type, method] = match;
    if (!model.types.has(type)) {
      continue;
    }
    const available = memberSet(model, type);
    if (!available) {
      continue;
    }
    assertions += 1;
    if (!available.has(method)) {
      failures.push(
        `line ${lineOf(stripped, match.index)}: ${type}.${method}( is not a generated member`,
      );
    }
  }

  // Enum entries, e.g. UnitSystem.METRIC.
  for (const match of stripped.matchAll(/\b([A-Z]\w*)\.([A-Z][A-Z0-9_]+)\b/g)) {
    const [, type, entry] = match;
    const entries = model.enumEntries.get(type);
    if (!entries) {
      continue;
    }
    assertions += 1;
    if (!entries.has(entry)) {
      failures.push(
        `line ${lineOf(stripped, match.index)}: ${type}.${entry} is not a generated enum entry`,
      );
    }
  }

  if (assertions === 0) {
    return {
      failures,
      notApplicable: true,
      detail: 'no SDK symbol to assert (build script or platform fragment)',
    };
  }
  return {
    failures,
    notApplicable: false,
    detail: `${assertions} SDK symbol assertion(s)`,
  };
}

function stripAllKotlinComments(code) {
  let blockComment = false;
  return code
    .split('\n')
    .map((line) =>
      stripKotlinComments(line, blockComment, (next) => {
        blockComment = next;
      }),
    )
    .join('\n');
}

function importedNames(code) {
  const names = new Set();
  for (const match of code.matchAll(/^\s*import\s+uniffi\.rustic_maps\.(\w+)/gm)) {
    names.add(match[1]);
  }
  return names;
}

function memberSet(model, type, seen = new Set()) {
  if (seen.has(type) || !model.members.has(type)) {
    return null;
  }
  seen.add(type);
  const members = new Set(model.members.get(type));
  for (const supertype of model.supertypes.get(type) ?? []) {
    const inherited = memberSet(model, supertype, seen);
    if (inherited) {
      for (const member of inherited) {
        members.add(member);
      }
    }
  }
  return members;
}

function inferKotlinReceiverTypes(model, code) {
  const types = new Map([
    ['mapSdk', 'MapSdk'],
    ['mapSurface', 'MapView'],
    ['camera', 'MapCamera'],
    ['navigationController', 'Navigator'],
  ]);
  for (const match of code.matchAll(
    /\b(?:val|var)\s+(\w+)\s*(?::\s*([A-Z]\w*))?\s*=\s*([A-Z]\w*)\s*\(/g,
  )) {
    const [, name, annotation, constructor] = match;
    types.set(name, annotation ?? constructor);
  }
  for (const match of code.matchAll(/\b(\w+)\s*:\s*([A-Z]\w*)\b/g)) {
    const [, name, annotation] = match;
    if (model.types.has(annotation)) {
      types.set(name, annotation);
    }
  }
  // A snippet may shadow an ambient receiver with a local of another type.
  for (const match of code.matchAll(/\b(?:val|var)\s+(\w+)\s*=\s*([A-Z]\w*)\s*\(/g)) {
    types.set(match[1], match[2]);
  }
  return types;
}

function lineOf(text, index) {
  return text.slice(0, index).split('\n').length;
}

const NON_SDK_CONSTRUCTORS = new Set([
  'File',
  'TextToSpeech',
  'Exception',
  'RuntimeException',
  'IllegalStateException',
  'IllegalArgumentException',
  'StringBuilder',
  'Sequence',
  'List',
  'Map',
  'Set',
  'Pair',
  'Triple',
  'Array',
  'UInt',
  'ULong',
  'Long',
  'Int',
  'Double',
  'Float',
  'String',
  'Boolean',
  'Byte',
  'Short',
]);

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

function runSelfTest(report, environment, options) {
  const cases = [];
  for (const language of options.languages) {
    if (language === 'js' || language === 'ts') {
      const surfaces = [];
      if (environment.wasmDts) {
        surfaces.push({
          name: 'wasm',
          control: `import { MapCamera } from './web/rustic_maps_bg.js';\nconst camera = new MapCamera(1, 2, 3, 4, 5);\n`,
          broken: `const broken: number = 'not a number';\n`,
          platform: 'wasm',
        });
      }
      if (environment.rnBindings) {
        surfaces.push({
          name: 'react-native',
          control: `import { MapSdk } from '@angoratek/rustic-maps-sdk';\nconst sdk = new MapSdk();\n`,
          broken: `const broken: number = 'not a number';\n`,
          platform: 'react-native',
        });
      }
      if (surfaces.length === 0) {
        cases.push({
          language,
          surface: 'typescript',
          passed: false,
          detail: 'no TypeScript surface available to self-test',
        });
      }
      for (const surface of surfaces) {
        cases.push(selfTestTypeScript(language, surface, environment));
      }
    } else if (language === 'swift') {
      cases.push(selfTestSwift(environment));
    } else if (language === 'kotlin') {
      cases.push(selfTestKotlin(environment));
    }
  }
  for (const testCase of cases) {
    report.results.push({ ...testCase, checker: 'self-test' });
  }
}

function selfTestTypeScript(language, surface, environment) {
  const control = {
    file: `<self-test:${surface.name}>`,
    line: 1,
    language,
    platform: surface.platform,
    code: surface.control,
  };
  const broken = {
    ...control,
    code: `${surface.control}\n${surface.broken}`,
  };
  const controlReport = createReport();
  checkTypeScriptSurface(controlReport, [control], environment, surface.name);
  const brokenReport = createReport();
  checkTypeScriptSurface(brokenReport, [broken], environment, surface.name);
  const controlStatus = controlReport.results[0]?.status;
  const brokenStatus = brokenReport.results[0]?.status;
  return {
    file: `<self-test:${surface.name}>`,
    line: 1,
    language,
    platform: surface.platform,
    surface: `typescript-${surface.name}`,
    status: controlStatus === 'pass' && brokenStatus === 'fail' ? 'pass' : 'fail',
    detail:
      controlStatus === 'pass' && brokenStatus === 'fail'
        ? 'control passes, injected type error fails'
        : `control=${controlStatus}, injected=${brokenStatus}`,
  };
}

function selfTestSwift(environment) {
  const control = {
    file: '<self-test:swift>',
    line: 1,
    language: 'swift',
    platform: 'ios',
    code: 'let camera = try MapCamera(\n  center: GeographicCoordinate(latitude: 1, longitude: 2),\n  zoomLevel: 12,\n  bearingDegrees: 0,\n  pitchDegrees: 0\n)\n',
  };
  const broken = {
    ...control,
    code: `${control.code}\nlet broken: Int = "not an int"\n`,
  };
  const controlReport = createReport();
  checkSwiftSnippets(controlReport, [control], environment);
  const brokenReport = createReport();
  checkSwiftSnippets(brokenReport, [broken], environment);
  const controlStatus = controlReport.results[0]?.status;
  const brokenStatus = brokenReport.results[0]?.status;
  return {
    file: control.file,
    line: 1,
    language: 'swift',
    platform: 'ios',
    surface: 'swift',
    status: controlStatus === 'pass' && brokenStatus === 'fail' ? 'pass' : 'fail',
    detail:
      controlStatus === 'pass' && brokenStatus === 'fail'
        ? 'control passes, injected type error fails'
        : `control=${controlStatus}, injected=${brokenStatus}`,
  };
}

function selfTestKotlin(environment) {
  if (!environment.kotlinBindings) {
    return {
      file: '<self-test:kotlin>',
      line: 1,
      language: 'kotlin',
      platform: 'android',
      surface: 'kotlin-symbols',
      status: 'fail',
      detail: 'no generated Kotlin bindings available to self-test',
    };
  }
  const model = parseKotlinBindings(readFileSync(environment.kotlinBindings, 'utf8'));
  const control = assertKotlinSurface(
    model,
    'import uniffi.rustic_maps.MapSdk\nval mapSdk = MapSdk()\n',
  );
  const broken = assertKotlinSurface(
    model,
    'import uniffi.rustic_maps.NotARealGeneratedSymbol\n',
  );
  const passed = control.failures.length === 0 && broken.failures.length > 0;
  return {
    file: '<self-test:kotlin>',
    line: 1,
    language: 'kotlin',
    platform: 'android',
    surface: 'kotlin-symbols',
    status: passed ? 'pass' : 'fail',
    detail: passed
      ? 'control passes, injected unknown symbol fails'
      : `control failures=${control.failures.length}, injected failures=${broken.failures.length}`,
  };
}

// ---------------------------------------------------------------------------
// Reporting
// ---------------------------------------------------------------------------

function printReport(report, options, environment) {
  const selected = report.results.filter((entry) => entry.checker !== 'self-test');
  const byChecker = new Map();
  for (const entry of selected) {
    const list = byChecker.get(entry.checker) ?? [];
    list.push(entry);
    byChecker.set(entry.checker, list);
  }

  process.stdout.write('Documentation snippet-build check\n');
  for (const note of report.surfaceNotes) {
    process.stdout.write(`  surface: ${note}\n`);
  }
  const checksTypeScript = options.languages.some(
    (language) => LANGUAGE_FAMILIES.get(language) === 'typescript',
  );
  if (checksTypeScript) {
    if (!environment.tsc) {
      process.stdout.write('  toolchain: TypeScript compiler not found\n');
    } else {
      const version = runCapture(environment.tsc, ['--version']);
      process.stdout.write(`  toolchain: ${version.stdout.trim() || environment.tsc}\n`);
    }
  }
  process.stdout.write('\n');

  for (const [checker, entries] of byChecker) {
    const counts = countBy(entries, (entry) => entry.status);
    process.stdout.write(
      `${checker}: ${entries.length} snippet(s) — pass ${counts.pass ?? 0}, ` +
        `fail ${counts.fail ?? 0}, not-applicable ${counts['not-applicable'] ?? 0}, ` +
        `unavailable ${counts.unavailable ?? 0}\n`,
    );
    for (const entry of entries) {
      if (
        options.verbose ||
        statusIsFailure(entry.status) ||
        statusIsUnavailable(entry.status)
      ) {
        const location = entry.platform ? `${entry.platform}` : 'no-panel';
        process.stdout.write(
          `  ${entry.status.toUpperCase().padEnd(14)} ${entry.file}:${entry.line} (${location})${entry.detail ? ` — ${entry.detail}` : ''}\n`,
        );
        if (statusIsFailure(entry.status) && entry.output) {
          for (const line of entry.output.split('\n').slice(0, MAX_ERROR_LINES_PER_SNIPPET)) {
            process.stdout.write(`      ${line}\n`);
          }
        }
      }
    }
  }
  if (!environment.rnBindings) {
    process.stdout.write(
      'note: React Native snippets were not checked because the generated surface is absent.\n',
    );
  }
  if (!environment.swiftSurface) {
    process.stdout.write(
      'note: Swift snippets were not checked because the generated bindings are absent.\n',
    );
  }
  process.stdout.write('\n');
}

function countBy(entries, key) {
  const counts = {};
  for (const entry of entries) {
    const value = key(entry);
    counts[value] = (counts[value] ?? 0) + 1;
  }
  return counts;
}

function printSelfTestReport(report) {
  const entries = report.results.filter((entry) => entry.checker === 'self-test');
  process.stdout.write('Self-test\n');
  for (const entry of entries) {
    process.stdout.write(
      `  ${entry.status === 'pass' ? 'PASS' : 'FAIL'} ${entry.surface}${entry.language ? ` (${entry.language})` : ''}: ${entry.detail}\n`,
    );
  }
  process.stdout.write('\n');
}

function safeName(snippet) {
  return `${path.basename(snippet.file, '.md')}-${snippet.line}`
    .replace(/[^A-Za-z0-9_-]/g, '-')
    .toLowerCase();
}

function runCapture(command, args) {
  return spawnSync(command, args, { encoding: 'utf8' });
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

function summarizeRun(report, snippets, options, environment) {
  const selected = report.results.filter((entry) => entry.checker !== 'self-test');
  const failures = selected.filter((entry) => statusIsFailure(entry.status));
  const unavailable = selected.filter((entry) => statusIsUnavailable(entry.status));
  const notApplicable = selected.filter((entry) => entry.status === 'not-applicable');
  const explicitSkips = snippets.filter(
    (snippet) => snippet.skipReason && options.languages.includes(snippet.language),
  );
  const checked = selected.filter(
    (entry) =>
      entry.status === 'pass' || entry.status === 'fail',
  );

  if (checked.length === 0) {
    process.stderr.write('error: no snippets were checked (refusing to pass silently)\n');
    return 1;
  }
  if (failures.length > 0) {
    process.stderr.write(`error: ${failures.length} snippet(s) failed\n`);
    return 1;
  }
  if (explicitSkips.length > 0) {
    process.stdout.write(`explicitly skipped snippets: ${explicitSkips.length}\n`);
    for (const snippet of explicitSkips) {
      process.stdout.write(`  SKIP ${snippet.file}:${snippet.line} — ${snippet.skipReason}\n`);
    }
  }
  if (notApplicable.length > 0) {
    process.stdout.write(
      `not applicable: ${notApplicable.length} (fragments with no SDK symbol to assert)\n`,
    );
  }
  if (unavailable.length > 0) {
    process.stdout.write(`unavailable: ${unavailable.length}\n`);
    if (options.strict) {
      process.stderr.write('error: --strict is set and a surface/toolchain is unavailable\n');
      return 1;
    }
  }
  process.stdout.write(`PASS: ${checked.length} snippet(s) checked\n`);
  return 0;
}

function main() {
  const options = parseArguments(process.argv.slice(2));
  rmSync(WORK_DIR, { recursive: true, force: true });
  mkdirSync(WORK_DIR, { recursive: true });

  let snippets;
  try {
    snippets = extractSnippets(options.docsDir);
  } catch (error) {
    process.stderr.write(`error: ${error.message}\n`);
    return 1;
  }

  const selected = snippets.filter(
    (snippet) =>
      options.languages.includes(snippet.language) && snippet.skipReason === null,
  );
  const requiredFamilies = new Set(
    options.languages.map((language) => LANGUAGE_FAMILIES.get(language)),
  );
  for (const family of requiredFamilies) {
    if (!selected.some((snippet) => LANGUAGE_FAMILIES.get(snippet.language) === family)) {
      process.stderr.write(
        `error: no ${family} snippets found under ${options.docsDir} for --langs ${options.languages.join(',')}\n`,
      );
      return 1;
    }
  }

  const environment = discoverEnvironment(options);
  const report = createReport();

  const javascriptSnippets = selected.filter((snippet) => snippet.language === 'js');
  const wasmSnippets = javascriptSnippets.filter((snippet) => snippet.platform === 'wasm');
  const reactNativeSnippets = javascriptSnippets.filter(
    (snippet) => snippet.platform === 'react-native',
  );
  const typeScriptSnippets = selected.filter((snippet) => snippet.language === 'ts');
  const swiftSnippets = selected.filter((snippet) => snippet.language === 'swift');
  const kotlinSnippets = selected.filter((snippet) => snippet.language === 'kotlin');

  if (wasmSnippets.length + typeScriptSnippets.length > 0) {
    const allWasm = [...wasmSnippets, ...typeScriptSnippets.filter((snippet) => snippet.platform === 'wasm')];
    if (environment.wasmDts) {
      checkTypeScriptSurface(report, allWasm, environment, 'wasm');
    } else {
      for (const snippet of allWasm) {
        record(
          report,
          snippet,
          'typescript-wasm',
          'unavailable',
          environment.browserCanvasDts
            ? `generated contract declarations absent; the canvas declarations (${BROWSER_CANVAS_DTS}) ` +
                'describe the canvas only, so callable contract snippets cannot be checked'
            : 'generated contract declarations absent',
        );
      }
    }
  }
  if (reactNativeSnippets.length > 0) {
    if (environment.rnBindings) {
      checkTypeScriptSurface(report, reactNativeSnippets, environment, 'react-native');
    } else {
      for (const snippet of reactNativeSnippets) {
        record(
          report,
          snippet,
          'typescript-react-native',
          'unavailable',
          'generated React Native surface absent (run scripts/build-rn.sh generation)',
        );
      }
    }
  }
  if (swiftSnippets.length > 0) {
    checkSwiftSnippets(report, swiftSnippets, environment);
  }
  if (kotlinSnippets.length > 0) {
    checkKotlinSnippets(report, kotlinSnippets, environment);
  }

  if (options.selfTest) {
    runSelfTest(report, environment, options);
  }

  printReport(report, options, environment);
  if (options.selfTest) {
    printSelfTestReport(report);
    const selfTests = report.results.filter((entry) => entry.checker === 'self-test');
    const failedTests = selfTests.filter((entry) => entry.status !== 'pass');
    if (failedTests.length > 0) {
      process.stderr.write(`error: ${failedTests.length} self-test(s) failed\n`);
      return 1;
    }
  }
  return summarizeRun(report, snippets, options, environment);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main();
}

export { assertKotlinSurface, extractSnippets, parseKotlinBindings };
