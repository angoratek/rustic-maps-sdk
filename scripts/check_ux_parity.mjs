// UX-parity gate: `platform/reference-tokens.json`
// is the single source of the reference applications' design language
// (docs/ReferenceAppUX.md §2) and of the unified SDK error-detail strings.
// Every platform mirror — the Compose theme, the SwiftUI theme, the React
// Native theme and the web playground — must reproduce the artifact's token
// names and values; this test reads both sides and fails on any drift.
//
// M28 slice 1 adds the §2.5 consumer-copy check: the artifact's `ui_labels`
// section is compared with each platform's string source (Android/iOS
// constants, React Native's `theme.ts`, its navigation drawer module and both
// native hosts, and the web page). Interpolation is normalized (`%s`, `%@`,
// `${…}`, `\(…)`, concatenated literals) before comparison. Platform
// divergences and the §2.5 document table are both BLOCKING (M28 slice 6): a
// label that drifts from the artifact fails the run exactly as a token or
// error-sentence drift does.
// Omissions stay printed, never silently passed — they name roles a surface
// genuinely does not have.
//
// The comparison is deliberately addition-tolerant, in the spirit of
// `platform/web/tests/contract_sync.mjs` (which it does not import): a mirror
// may carry platform-derived values the §2 table does not name (an `accentSoft`
// tint, a `scrim`, a screen backdrop); those are ignored. What is not allowed
// is a mirror dropping or changing a value the artifact names, or inventing an
// error sentence that differs from the artifact's.
//
// Honesty rule: a platform that genuinely has no mirror for a token (the web
// playground keeps the v1 panels and therefore has no home-sheet rows; React
// Native does not re-word the SDK error details) is omitted from that
// assertion and reported in the summary. An omission is never a silent pass:
// it is printed with the token name so the coverage is visible.
//
// The web consumer-copy adapter reads what the page renders, not only the
// canonical module (M28 slice 5 fixed that blind spot): every
// `data-ui-label` / `data-ui-label-aria` element in index.html contributes its
// rendered text or named attribute, and every `ui_label_text('<id>')` /
// `ui_label_text_with_values('<id>', …)` call in the playground controller's
// entry and concern modules contributes its dynamic label. What is still not
// covered: the gate does not execute the
// page, so placeholder substitution (a provider name, a readiness detail) and
// the wording of strings that are neither marked nor read from the module
// (the v1 developer-panel vocabulary below) are not evaluated here. The
// Playwright suite asserts the rendered dynamic copy.
//
// Mirror adapters read the value shapes each platform actually stores:
//   - Compose stores CSS colors as `Color(0xAARRGGBB)` / `Color(red = …)` and
//     sizes as `N.sp` / `FontWeight(N)`;
//   - SwiftUI stores colors as `rgba(…)` / `.opaque(0x…)` and sizes as
//     `CGFloat` constants referenced from the `ReferenceAppUXFont` functions;
//   - React Native stores the CSS strings verbatim (`'rgba(…)'`, `'#rrggbb'`);
//   - the web playground stores the CSS custom properties verbatim in
//     `platform/web/styles/playground.css`, and the §2.2 canvas mirror
//     constants as literal JavaScript in the playground controller's entry
//     and concern modules under `platform/web/`.
//
// Run directly:
//   node scripts/check_ux_parity.mjs
// Self-test (mutates a temporary copy and asserts the gate fails for each
// drift class, so it cannot pass vacuously):
//   node scripts/check_ux_parity.mjs --self-test

import {
  mkdtempSync, mkdirSync, copyFileSync, readFileSync, readdirSync, rmSync, writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { scrubPublicCommentSources } from './check-public-comment-scrub.mjs';

// --- File locations ---------------------------------------------------------

const ARTIFACT_PATH = 'platform/reference-tokens.json';
const SOURCE_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

/**
 * The browser playground controller's entry and concern modules. The adapters
 * read them as one text so a constant or a dynamic label stays covered after
 * the controller is split by concern.
 */
const WEB_CONTROLLER_FILES = [
  'platform/web/playground.mjs',
  ...readdirSync(path.join(SOURCE_ROOT, 'platform', 'web'))
    .filter((file_name) => /^playground_.*\.mjs$/.test(file_name))
    .sort()
    .map((file_name) => `platform/web/${file_name}`),
];

/** Every file the mirror adapters read, relative to the repository root. */
const MIRROR_FILES = {
  androidTheme:
    'platform/android/reference-application/src/main/kotlin/com/rusticmaps/sdk/ux/ReferenceAppUXTheme.kt',
  androidDimensions:
    'platform/android/reference-application/src/main/kotlin/com/rusticmaps/sdk/ux/ReferenceAppUXDimensions.kt',
  androidState:
    'platform/android/reference-application/src/main/kotlin/com/rusticmaps/sdk/referenceapplication/ReadinessReportState.kt',
  iosTheme: 'platform/ios/reference-application/Sources/ReferenceAppUXTheme.swift',
  iosLayout: 'platform/ios/reference-application/Sources/AppLayout.swift',
  iosConfiguration:
    'platform/ios/reference-application/Sources/ReferenceApplicationConfiguration.swift',
  iosModel: 'platform/ios/reference-application/Sources/ReferenceApplicationModel.swift',
  rnTheme: 'platform/rn/app-shell/src/theme.ts',
  rnMapControls: 'platform/rn/app-shell/src/mapControls.ts',
  rnSheetLayout: 'platform/rn/app-shell/src/sheetLayout.ts',
  webPage: 'platform/web/styles/playground.css',
  webMapControls: 'platform/web/map_controls.mjs',
  webCanvas: WEB_CONTROLLER_FILES,
};

/**
 * Every file the consumer-copy adapters read (docs/ReferenceAppUX.md §2.5,
 * M28). The web surface is read where it actually renders its copy: the
 * markup that carries `data-ui-label` elements and the controller that reads
 * the module for dynamic labels. The two React Native native hosts hold the
 * "hidden fifth copy" the copy audit calls out, so both are read and must
 * agree.
 */
const STRING_MIRROR_FILES = {
  androidConstants:
    'platform/android/reference-application/src/main/kotlin/com/rusticmaps/sdk/referenceapplication/ReferenceApplicationConstants.kt',
  iosConfiguration:
    'platform/ios/reference-application/Sources/ReferenceApplicationConfiguration.swift',
  reactNativeTheme: 'platform/rn/app-shell/src/theme.ts',
  reactNativeNavigationDrawer:
    'platform/rn/app-shell/src/components/NavigationDrawer.tsx',
  reactNativeAndroidHostConstants:
    'platform/rn/app-shell/android/app/src/main/java/com/rusticmaps/sdk/refapprn/RefappConstants.kt',
  reactNativeIosHostConstants:
    'platform/rn/app-shell/ios/MapsSdkRefappRn/RefappConstants.swift',
  webStrings: 'platform/web/ui_strings.mjs',
  webPageMarkup: 'platform/web/index.html',
  webPageController: WEB_CONTROLLER_FILES,
};

/** The document whose §2.5 table must match the artifact. */
const CONSUMER_COPY_DOCUMENT = 'docs/ReferenceAppUX.md';

/** The artifact section that owns consumer copy, and the document heading. */
const UI_LABELS_SECTION = 'ui_labels';
const CONSUMER_COPY_DOCUMENT_HEADING = '### 2.5 Consumer copy';

/** The platforms every consumer-copy row may override. */
const SUPPORTED_PLATFORMS = ['android', 'ios', 'rn', 'web'];

/**
 * The single placeholder marker both sides normalize to. The artifact spells
 * placeholders `{provider}`; a platform mirror may spell the same slot `%s`,
 * `%@`, `${…}` or `\(…)`, so all forms collapse to this marker before the
 * comparison.
 */
const PLACEHOLDER_MARKER = '{*}';

/** The artifact's own placeholder syntax. */
const CANONICAL_PLACEHOLDER_PATTERN = /\{[A-Za-z0-9_]+\}/g;

/** Each platform's interpolation forms, normalized to the marker above. */
const PLATFORM_INTERPOLATION_PATTERNS = [
  /\$\{[^}]*\}/g, // JavaScript template literal
  /\\\([^)]*\)/g, // Swift string interpolation
  /%[@a-zA-Z]+/g, // printf-style %s, %d, %@, %lu
];

/** Numeric comparison tolerance for colors and layout values. */
const VALUE_TOLERANCE = 1e-6;

/** The artifact's declaration block must carry this name. */
const ARTIFACT_NAME = 'reference-tokens';

// --- Small text helpers -----------------------------------------------------

/**
 * Returns the text between a balanced pair of delimiters, starting at the
 * given opening character.
 * @param {string} text
 * @param {number} openIndex Index of the opening character.
 * @param {string} openChar
 * @param {string} closeChar
 * @returns {string}
 */
function extractDelimited(text, openIndex, openChar, closeChar) {
  if (openIndex < 0 || text[openIndex] !== openChar) {
    throw new Error(`expected "${openChar}" at index ${openIndex}`);
  }
  let depth = 0;
  for (let index = openIndex; index < text.length; index += 1) {
    if (text[index] === openChar) {
      depth += 1;
    } else if (text[index] === closeChar) {
      depth -= 1;
      if (depth === 0) {
        return text.slice(openIndex + 1, index);
      }
    }
  }
  throw new Error(`unbalanced "${openChar}" starting at index ${openIndex}`);
}

/**
 * Extracts the brace-delimited block that follows a marker (the marker may
 * itself end with the opening brace).
 * @param {string} text
 * @param {string} marker
 * @returns {string}
 */
function extractBracedBlockAfter(text, marker) {
  const markerIndex = text.indexOf(marker);
  if (markerIndex < 0) {
    throw new Error(`marker not found: ${marker}`);
  }
  const openIndex = text.indexOf('{', markerIndex);
  return extractDelimited(text, openIndex, '{', '}');
}

/**
 * Extracts the parenthesis-delimited block that follows a marker.
 * @param {string} text
 * @param {string} marker
 * @returns {string}
 */
function extractParenBlockAfter(text, marker) {
  const markerIndex = text.indexOf(marker);
  if (markerIndex < 0) {
    throw new Error(`marker not found: ${marker}`);
  }
  // Search from the marker's last character so a marker that itself ends with
  // `(` selects that parenthesis rather than an earlier one inside the marker.
  const openIndex = text.indexOf('(', markerIndex + marker.length - 1);
  return extractDelimited(text, openIndex, '(', ')');
}

/** Converts an upper/lower camel-case identifier to its kebab-case token name. */
function camelToKebab(name) {
  return name.replace(/[A-Z]/g, (character) => `-${character.toLowerCase()}`);
}

/** Parses a numeric literal, tolerating a trailing `f` or `px`. */
function parseNumber(text) {
  const value = Number.parseFloat(String(text).trim());
  if (!Number.isFinite(value)) {
    throw new Error(`not a number: ${text}`);
  }
  return value;
}

// --- Color and value parsing ------------------------------------------------

/**
 * @typedef {object} RgbaColor
 * @property {number} red 0–255.
 * @property {number} green 0–255.
 * @property {number} blue 0–255.
 * @property {number} alpha 0–1.
 */

/**
 * Parses a CSS `#rrggbb` or `rgba(r, g, b, a)` literal.
 * @param {string} source
 * @returns {RgbaColor}
 */
function parseCssColor(source) {
  const trimmed = source.trim();
  const hex = /^#([0-9a-fA-F]{6})$/.exec(trimmed);
  if (hex !== null) {
    const digits = hex[1];
    return {
      red: Number.parseInt(digits.slice(0, 2), 16),
      green: Number.parseInt(digits.slice(2, 4), 16),
      blue: Number.parseInt(digits.slice(4, 6), 16),
      alpha: 1,
    };
  }
  const rgba = /^rgba?\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)\s*(?:,\s*([\d.]+)\s*)?\)$/
    .exec(trimmed);
  if (rgba !== null) {
    return {
      red: parseNumber(rgba[1]),
      green: parseNumber(rgba[2]),
      blue: parseNumber(rgba[3]),
      alpha: rgba[4] === undefined ? 1 : parseNumber(rgba[4]),
    };
  }
  throw new Error(`unsupported color literal: ${source}`);
}

/**
 * Parses the Kotlin color expressions the Compose theme uses:
 * `Color(0xAARRGGBB)` and `Color(red = 20f / 255f, …, alpha = 0.86f)`.
 * @param {string} source
 * @returns {RgbaColor}
 */
function parseKotlinColor(source) {
  const hex = /Color\(\s*0x([0-9a-fA-F]{8})\s*\)/.exec(source);
  if (hex !== null) {
    const digits = hex[1];
    return {
      red: Number.parseInt(digits.slice(2, 4), 16),
      green: Number.parseInt(digits.slice(4, 6), 16),
      blue: Number.parseInt(digits.slice(6, 8), 16),
      alpha: Number.parseInt(digits.slice(0, 2), 16) / 255,
    };
  }
  const components =
    /Color\(\s*red\s*=\s*([^,]+),\s*green\s*=\s*([^,]+),\s*blue\s*=\s*([^,]+),\s*alpha\s*=\s*([^)]+)\)/
      .exec(source);
  if (components === null) {
    throw new Error(`unsupported Kotlin color expression: ${source}`);
  }
  return {
    red: parseKotlinChannel(components[1]),
    green: parseKotlinChannel(components[2]),
    blue: parseKotlinChannel(components[3]),
    alpha: parseNumber(components[4]),
  };
}

/**
 * Resolves a Kotlin channel expression to 0–255: `20f / 255f` carries its
 * numerator already on the 0–255 scale; a bare `1f` is the unit interval.
 * @param {string} expression
 * @returns {number}
 */
function parseKotlinChannel(expression) {
  const trimmed = expression.trim().replace(/f$/, '');
  if (trimmed.includes('/')) {
    return parseNumber(trimmed.split('/')[0]);
  }
  return parseNumber(trimmed) * 255;
}

/**
 * Parses the SwiftUI color expressions the theme uses:
 * `rgba(20, 24, 30, alpha: 0.86)` and
 * `.opaque(red: 0xf2, green: 0xf4, blue: 0xf8)`.
 * @param {string} source
 * @returns {RgbaColor}
 */
function parseIosColor(source) {
  const rgba = /rgba\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*alpha:\s*([\d.]+)\s*\)/
    .exec(source);
  if (rgba !== null) {
    return {
      red: parseNumber(rgba[1]),
      green: parseNumber(rgba[2]),
      blue: parseNumber(rgba[3]),
      alpha: parseNumber(rgba[4]),
    };
  }
  const opaque = /\.opaque\(red:\s*0x([0-9a-fA-F]+),\s*green:\s*0x([0-9a-fA-F]+),\s*blue:\s*0x([0-9a-fA-F]+)\)/
    .exec(source);
  if (opaque !== null) {
    return {
      red: Number.parseInt(opaque[1], 16),
      green: Number.parseInt(opaque[2], 16),
      blue: Number.parseInt(opaque[3], 16),
      alpha: 1,
    };
  }
  throw new Error(`unsupported SwiftUI color expression: ${source}`);
}

/**
 * Parses a CSS box-shadow string into its offset, blur and color.
 * @param {string} source
 * @returns {{offsetY: number, blurRadius: number, color: RgbaColor}}
 */
function parseShadow(source) {
  const match = /^\s*[-\d.]+(?:px)?\s+([-\d.]+)(?:px)?\s+([-\d.]+)(?:px)?\s+(.+)$/
    .exec(source.trim());
  if (match === null) {
    throw new Error(`unsupported shadow literal: ${source}`);
  }
  return {
    offsetY: parseNumber(match[1]),
    blurRadius: parseNumber(match[2]),
    color: parseCssColor(match[3]),
  };
}

/** True when two numbers are close enough to be the same token value. */
function numbersMatch(left, right) {
  return Math.abs(left - right) <= VALUE_TOLERANCE;
}

/** True when two RGBA colors are the same. */
function colorsMatch(actual, expected) {
  return actual !== undefined
    && numbersMatch(actual.red, expected.red)
    && numbersMatch(actual.green, expected.green)
    && numbersMatch(actual.blue, expected.blue)
    && numbersMatch(actual.alpha, expected.alpha);
}

// --- Artifact reading and validation ---------------------------------------

/**
 * @typedef {object} ArtifactExpectation
 * @property {'color' | 'number' | 'shadow' | 'range' | 'string'} kind
 * @property {any} value
 */

/**
 * Reads and validates the artifact, then flattens it into one expectation
 * per mirrored value. A malformed artifact is a failure in its own right.
 * @param {unknown} artifact
 * @returns {Map<string, ArtifactExpectation>}
 */
function buildArtifactExpectations(artifact) {
  if (artifact === null || typeof artifact !== 'object') {
    throw new Error('platform/reference-tokens.json must be a JSON object');
  }
  const declaration = artifact.artifact;
  if (declaration === undefined || declaration.name !== ARTIFACT_NAME) {
    throw new Error(`the artifact declaration must name "${ARTIFACT_NAME}"`);
  }
  const expectations = new Map();
  const addUnique = (key, expectation) => {
    if (expectations.has(key)) {
      throw new Error(`the artifact declares "${key}" twice`);
    }
    expectations.set(key, expectation);
  };

  for (const row of artifact.color_roles ?? []) {
    addUnique(`${row.token}/dark`, { kind: 'color', value: parseCssColor(row.dark) });
    addUnique(`${row.token}/light`, { kind: 'color', value: parseCssColor(row.light) });
  }
  for (const row of artifact.typography_scale ?? []) {
    addUnique(`${row.token}/size`, { kind: 'number', value: row.size_pixels });
    addUnique(`${row.token}/weight`, { kind: 'number', value: row.font_weight });
  }
  for (const row of artifact.spacing_tokens ?? []) {
    addUnique(`spacing/${row.token}`, { kind: 'number', value: row.pixels });
  }
  for (const row of artifact.radius_tokens ?? []) {
    addUnique(`radius/${row.token}`, { kind: 'number', value: row.pixels });
  }
  for (const row of artifact.behavior_tokens ?? []) {
    if (row.token === 'elevation-surface') {
      addUnique(`behavior/${row.token}`, { kind: 'shadow', value: parseShadow(row.value) });
    } else if (row.token === 'scale-bar-width-range') {
      addUnique(`behavior/${row.token}`, {
        kind: 'range',
        value: { minimum: row.minimum_pixels, maximum: row.maximum_pixels },
      });
    } else {
      addUnique(`behavior/${row.token}`, {
        kind: typeof row.value === 'string' ? 'string' : 'number',
        value: row.value,
      });
    }
  }
  const strings = artifact.strings ?? {};
  for (const name of ['rate_limit_exhausted_detail', 'provider_not_configured_detail_template']) {
    if (typeof strings[name] !== 'string') {
      throw new Error(`the artifact is missing strings.${name}`);
    }
    addUnique(`string/${name}`, { kind: 'string', value: strings[name] });
  }

  const requiredSections = ['color_roles', 'typography_scale', 'spacing_tokens', 'radius_tokens', 'behavior_tokens'];
  for (const section of requiredSections) {
    if (!Array.isArray(artifact[section]) || artifact[section].length === 0) {
      throw new Error(`the artifact is missing the "${section}" section`);
    }
  }
  return expectations;
}

/**
 * Compares one mirrored value with its artifact expectation.
 * @param {ArtifactExpectation} expectation
 * @param {any} actual
 * @returns {boolean}
 */
function mirroredValueMatches(expectation, actual) {
  switch (expectation.kind) {
    case 'color':
      return colorsMatch(actual, expectation.value);
    case 'number':
      return typeof actual === 'number' && numbersMatch(actual, expectation.value);
    case 'shadow':
      return actual !== undefined
        && numbersMatch(actual.offsetY, expectation.value.offsetY)
        && numbersMatch(actual.blurRadius, expectation.value.blurRadius)
        && colorsMatch(actual.color, expectation.value.color);
    case 'range':
      return actual !== undefined
        && numbersMatch(actual.minimum, expectation.value.minimum)
        && numbersMatch(actual.maximum, expectation.value.maximum);
    case 'string':
      return actual === expectation.value;
    default:
      throw new Error(`unknown expectation kind: ${expectation.kind}`);
  }
}

// --- Consumer copy (docs/ReferenceAppUX.md §2.5, M28) ----------------------

/**
 * @typedef {object} UiLabelExpectation
 * @property {string} text The canonical value.
 * @property {Record<string, string>} overrides Explicit allow-listed values.
 */

/**
 * Collapses the artifact's placeholders and every platform's interpolation to
 * one marker so the two sides compare on the surrounding wording alone.
 * @param {string} source
 * @returns {string}
 */
function normalizeInterpolation(source) {
  let normalized = source;
  for (const pattern of PLATFORM_INTERPOLATION_PATTERNS) {
    normalized = normalized.replace(pattern, PLACEHOLDER_MARKER);
  }
  return normalized.replace(CANONICAL_PLACEHOLDER_PATTERN, PLACEHOLDER_MARKER).trim();
}

/**
 * Reads and validates the artifact's `ui_labels` section. Every override must
 * name a known platform and carry a reason; a malformed row fails the gate.
 * @param {unknown} artifact
 * @returns {Map<string, UiLabelExpectation>}
 */
function buildUiLabelExpectations(artifact) {
  const rows = artifact[UI_LABELS_SECTION];
  if (!Array.isArray(rows) || rows.length === 0) {
    throw new Error(`the artifact is missing the "${UI_LABELS_SECTION}" section`);
  }
  const expectations = new Map();
  for (const row of rows) {
    if (typeof row.id !== 'string' || row.id.length === 0) {
      throw new Error(`an "${UI_LABELS_SECTION}" entry has no id`);
    }
    if (typeof row.text !== 'string' || row.text.length === 0) {
      throw new Error(`"${row.id}" has no canonical text`);
    }
    if (expectations.has(row.id)) {
      throw new Error(`the artifact declares "${row.id}" twice in ${UI_LABELS_SECTION}`);
    }
    const overrides = row.platform_overrides ?? {};
    const overrideReasons = row.override_reasons ?? {};
    for (const [platform, text] of Object.entries(overrides)) {
      if (!SUPPORTED_PLATFORMS.includes(platform)) {
        throw new Error(`"${row.id}" overrides unknown platform "${platform}"`);
      }
      if (typeof text !== 'string') {
        throw new Error(`"${row.id}" has a non-string override for "${platform}"`);
      }
      if (typeof overrideReasons[platform] !== 'string' || overrideReasons[platform].length === 0) {
        throw new Error(`"${row.id}" overrides "${platform}" without an override_reasons entry`);
      }
    }
    expectations.set(row.id, { text: row.text, overrides });
  }
  return expectations;
}

/**
 * Resolves the value a platform must render: its allow-listed override when
 * one exists, the canonical text otherwise.
 * @param {UiLabelExpectation} expectation
 * @param {string} platform
 * @returns {string}
 */
function effectiveUiLabelText(expectation, platform) {
  return expectation.overrides[platform] ?? expectation.text;
}

/**
 * Maps parsed language constants onto canonical IDs.
 * @param {Map<string, string>} constants
 * @param {Record<string, string>} mapping Canonical ID → constant name.
 * @param {string} sourceName
 * @returns {Map<string, {value: string, source: string}>}
 */
function mapConstantsToUiLabels(constants, mapping, sourceName) {
  const values = new Map();
  for (const [id, constantName] of Object.entries(mapping)) {
    if (constants.has(constantName)) {
      values.set(id, { value: constants.get(constantName), source: `${sourceName}:${constantName}` });
    }
  }
  return values;
}

/**
 * Parses Kotlin `const val NAME: String = "…"` declarations, joining a value
 * written as adjacent quoted literals with `+`. Values are read exactly as the
 * source spells them; interpolation normalization happens later.
 * @param {string} text
 * @returns {Map<string, string>}
 */
function parseKotlinStringConstants(text) {
  const constants = new Map();
  const declarationPattern = /const val\s+(\w+):\s*String\s*=\s*((?:"[^"]*"|\s|\+)+)/g;
  for (const match of text.matchAll(declarationPattern)) {
    constants.set(match[1], joinQuotedLiterals(match[2]));
  }
  return constants;
}

/**
 * Parses Swift `static let name = "…"` declarations (one line) and
 * `static let name = """…"""` blocks, joining `+`-continued literals.
 * @param {string} text
 * @returns {Map<string, string>}
 */
function parseSwiftStringConstants(text) {
  const constants = new Map();
  const multilinePattern = /static let\s+(\w+)(?::\s*[\w.]+)?\s*=\s*"""\n([\s\S]*?)"""/g;
  for (const match of text.matchAll(multilinePattern)) {
    constants.set(match[1], cleanSwiftMultilineString(match[2]));
  }
  const singleLinePattern =
    /static let\s+(\w+)(?::\s*[\w.]+)?\s*=\s*((?:"(?:[^"\\]|\\.)*"|\s|\+)+)/g;
  for (const match of text.matchAll(singleLinePattern)) {
    if (!constants.has(match[1])) {
      constants.set(match[1], joinQuotedLiterals(match[2]));
    }
  }
  return constants;
}

/**
 * Joins every double-quoted literal in an expression; the Swift/Kotlin `+`
 * concatenations the reference apps use are plain literal concatenation.
 * @param {string} expression
 * @returns {string}
 */
function joinQuotedLiterals(expression) {
  return [...expression.matchAll(/"((?:[^"\\]|\\.)*)"/g)]
    .map((match) => match[1].replace(/\\"/g, '"'))
    .join('');
}

/**
 * Cleans a Swift multiline literal: `\` at end of line suppresses that newline
 * (the indentation that follows is not part of the value), and remaining line
 * breaks read as spaces.
 * @param {string} source
 * @returns {string}
 */
function cleanSwiftMultilineString(source) {
  return source
    .replace(/\\\n\s*/g, '')
    .replace(/\n\s*/g, ' ')
    .trim();
}

/** Android constant names mapped to canonical consumer-copy IDs. */
const ANDROID_UI_LABEL_CONSTANTS = {
  'plan.origin_label': 'ORIGIN_FIELD_LABEL_TEXT',
  'plan.destination_label': 'DESTINATION_FIELD_LABEL_TEXT',
  'plan.search_action': 'SEARCH_SUBMIT_LABEL_TEXT',
  'plan.my_location_action': 'PLAN_MY_LOCATION_ACTION_TEXT',
  'plan.swap_action': 'PLAN_SWAP_ACTION_TEXT',
  'plan.missing_origin': 'PLAN_ROUTE_NEEDS_ORIGIN_TEXT',
  'plan.missing_destination': 'PLAN_ROUTE_NEEDS_DESTINATION_TEXT',
  'plan.missing_both': 'PLAN_ROUTE_NEEDS_BOTH_FIELDS_TEXT',
  'plan.my_location_unavailable': 'PLAN_MY_LOCATION_UNAVAILABLE_TEMPLATE',
  'plan.trip_source_demo_plan': 'PLAN_TRIP_SOURCE_DEMO_PLAN_TEMPLATE',
  'settings.provider_row': 'SETTINGS_PROVIDER_ROW_TEMPLATE',
  'settings.provider_state_ready': 'SETTINGS_PROVIDER_STATE_READY_TEXT',
  'settings.provider_state_needs_credential': 'SETTINGS_PROVIDER_STATE_NEEDS_CREDENTIAL_TEMPLATE',
  'settings.provider_state_not_configured': 'SETTINGS_PROVIDER_STATE_NOT_CONFIGURED_TEXT',
  'settings.routing_demo_endpoint': 'SETTINGS_ROUTING_DEMO_ENDPOINT_TEMPLATE',
  'settings.routing_unreachable_next_step': 'SETTINGS_ROUTING_UNREACHABLE_NEXT_STEP_TEXT',
  'diagnostics.show_debug_events': 'DIAGNOSTICS_SHOW_DEBUG_EVENTS_TEXT',
  'diagnostics.hide_debug_events': 'DIAGNOSTICS_HIDE_DEBUG_EVENTS_TEXT',
  'diagnostics.clear_action': 'DIAGNOSTICS_CLEAR_ACTION_TEXT',
  'diagnostics.no_events': 'DIAGNOSTICS_RECENT_EVENTS_NONE_TEXT',
  'diagnostics.stream_scope_note': 'DIAGNOSTICS_STREAM_SCOPE_NOTE_TEXT',
  'plan.origin_hint': 'ORIGIN_FIELD_HINT_TEXT',
  'plan.destination_hint': 'DESTINATION_FIELD_HINT_TEXT',
  'plan.plan_route_action': 'PLAN_ROUTE_BUTTON_TEXT',
  'plan.planning_in_flight': 'ROUTE_PLAN_IN_FLIGHT_TEXT',
  'plan.searching_in_flight': 'SEARCH_IN_FLIGHT_TEXT',
  'peek.primary_action': 'ROUTE_SUMMARY_START_BUTTON_TEXT',
  'route.start_action': 'ROUTE_SUMMARY_START_BUTTON_TEXT',
  'route.edit_action': 'ROUTE_SUMMARY_EDIT_BUTTON_TEXT',
  'plan.destination_placeholder': 'DESTINATION_FIELD_PLACEHOLDER_TEXT',
  'plan.empty_results_hint': 'EMPTY_RESULTS_HINT_TEXT',
  'plan.no_matches': 'AUTOCOMPLETE_NO_MATCH_TEXT',
  'plan.demo_places_heading': 'DEMO_PLACES_HEADING_TEXT',
  'plan.demo_no_match_hint': 'DEMO_NO_MATCH_HINT_TEMPLATE',
  'plan.recorded_route_note': 'RECORDED_ROUTE_NOTE_TEXT',
  'plan.no_recorded_route_hint': 'NO_RECORDED_ROUTE_HINT_TEMPLATE',
  'navigation.phase_navigating': 'NAVIGATION_PHASE_LABEL_NAVIGATING',
  'navigation.phase_off_route': 'NAVIGATION_PHASE_LABEL_OFF_ROUTE',
  'navigation.phase_arrived': 'NAVIGATION_PHASE_LABEL_ARRIVED',
  'navigation.eta_prefix': 'NAVIGATION_DRAWER_ETA_PREFIX_TEXT',
  'navigation.eta_distance_label': 'NAVIGATION_DRAWER_ETA_DISTANCE_LABEL_TEXT',
  'navigation.eta_duration_label': 'NAVIGATION_DRAWER_ETA_DURATION_LABEL_TEXT',
  'navigation.deviation_alert': 'DEVIATION_ALERT_TEXT_TEMPLATE',
  'navigation.speed_label': 'NAVIGATION_DRAWER_SPEED_LABEL_TEXT',
  'navigation.pause_action': 'NAVIGATION_DRAWER_PAUSE_BUTTON_TEXT',
  'navigation.resume_action': 'NAVIGATION_DRAWER_RESUME_BUTTON_TEXT',
  'navigation.speed_multiplier_label': 'NAVIGATION_SPEED_MULTIPLIER_LABEL_TEMPLATE',
  'navigation.seek_label': 'NAVIGATION_DRAWER_SEEK_LABEL_TEXT',
  'readiness.chip_ready': 'READINESS_CHIP_READY_TEXT',
  'readiness.chip_loading': 'READINESS_CHIP_LOADING_TEXT',
  'readiness.chip_degraded_prefix': 'READINESS_CHIP_DEGRADED_PREFIX_TEXT',
  'readiness.chip_failed_prefix': 'READINESS_CHIP_FAILED_PREFIX_TEXT',
  'overlays.clear_action': 'OVERLAYS_CLEAR_ACTION_TEXT',
  'navigation.exit_action': 'NAVIGATION_DRAWER_EXIT_BUTTON_TEXT',
  'navigation.phase_not_started': 'NAVIGATION_DRAWER_PHASE_NOT_STARTED_TEXT',
  'navigation.voice_toggle': 'SPEECH_TOGGLE_LABEL_TEXT',
  'settings.open_accessibility': 'SETTINGS_OPEN_ACCESSIBILITY_TEXT',
  'settings.close_action': 'SETTINGS_CLOSE_TEXT',
  'settings.credential_applied': 'PROVIDER_CREDENTIAL_APPLIED_TEMPLATE',
  'settings.credential_not_applied': 'PROVIDER_CREDENTIAL_NOT_APPLIED_TEMPLATE',
  'settings.keyless_status': 'KEYLESS_CREDENTIAL_STATUS_TEXT',
  'scenes.heading': 'DEMO_SCENES_ROW_TITLE_TEXT',
  'plans.heading': 'DEMO_PLANS_ROW_TITLE_TEXT',
  'theme.heading': 'THEME_ROW_TITLE_TEXT',
  'offline.heading': 'OFFLINE_MAPS_CARD_TITLE_TEXT',
  'offline.plan_route_action': 'OFFLINE_ROUTE_ACTION_TEXT',
  'offline.recorded_graph_route_title': 'OFFLINE_ROUTE_TITLE_TEXT',
  'offline.recorded_graph_note': 'OFFLINE_ROUTE_NOTE_TEXT',
  'offline.download_action': 'OFFLINE_DOWNLOAD_ACTION_TEXT',
  'offline.downloading_status': 'OFFLINE_DOWNLOADING_STATUS_TEXT',
  'offline.cancel_action': 'OFFLINE_CANCEL_ACTION_TEXT',
  'offline.completed_status': 'OFFLINE_COMPLETED_STATUS_TEXT',
  'offline.failed_status': 'OFFLINE_FAILED_STATUS_TEXT',
  'offline.stored_tiles_label': 'OFFLINE_STORED_TILES_LABEL_TEXT',
  'offline.stored_bytes_label': 'OFFLINE_STORED_BYTES_LABEL_TEXT',
  'offline.delete_action': 'OFFLINE_DELETE_ACTION_TEXT',
  'offline.deleted_status': 'OFFLINE_DELETED_STATUS_TEXT',
  'offline.browser_unavailable_status': 'OFFLINE_BROWSER_UNAVAILABLE_STATUS_TEXT',
  'diagnostics.title': 'DIAGNOSTICS_TITLE_TEXT',
};

/** Swift constant names mapped to canonical consumer-copy IDs. */
const IOS_UI_LABEL_CONSTANTS = {
  'peek.primary_action': 'startSimulatedNavigationLabel',
  'plan.origin_hint': 'originFieldHintText',
  'plan.destination_hint': 'destinationSearchPlaceholder',
  'plan.destination_placeholder': 'destinationFieldPlaceholderText',
  'plan.search_heading': 'searchViewHeadingText',
  'plan.plan_route_action': 'planRouteButtonText',
  'plan.planning_in_flight': 'routeSummaryPlanningText',
  'plan.searching_in_flight': 'searchInFlightText',
  'plan.empty_results_hint': 'searchResultsEmptyText',
  'plan.no_matches': 'planAutocompleteNoMatchesText',
  'plan.autocomplete_unavailable': 'planAutocompleteUnavailableText',
  'plan.demo_places_heading': 'demoPlacesHeadingText',
  'plan.demo_no_match_hint': 'demoNoMatchHintFormat',
  'plan.recorded_route_note': 'recordedRouteNoteText',
  'plan.no_recorded_route_hint': 'noRecordedRouteHintFormat',
  'route.start_action': 'startSimulatedNavigationLabel',
  'route.edit_action': 'routeEditLabel',
  'navigation.exit_action': 'drawerExitLabel',
  'navigation.phase_navigating': 'phaseNavigatingText',
  'navigation.phase_off_route': 'phaseOffRouteText',
  'navigation.phase_arrived': 'drawerArrivedText',
  'navigation.phase_not_started': 'phaseNotStartedText',
  'navigation.eta_prefix': 'etaPrefixText',
  'navigation.eta_distance_label': 'etaDistanceLabelText',
  'navigation.eta_duration_label': 'etaDurationLabelText',
  'navigation.deviation_alert': 'deviationAlertFormat',
  'navigation.voice_toggle': 'drawerVoiceToggleLabel',
  'navigation.speed_label': 'navigationSpeedLabelText',
  'navigation.pause_action': 'navigationPauseActionText',
  'navigation.resume_action': 'navigationResumeActionText',
  'navigation.speed_multiplier_label': 'navigationSpeedMultiplierFormat',
  'navigation.seek_label': 'navigationSeekLabelText',
  'readiness.chip_ready': 'readinessChipReadyText',
  'readiness.chip_loading': 'readinessChipLoadingText',
  'readiness.chip_degraded_prefix': 'readinessChipDegradedPrefix',
  'readiness.chip_failed_prefix': 'readinessChipFailedPrefix',
  'settings.open_accessibility': 'settingsControlLabel',
  'settings.close_action': 'settingsCloseLabel',
  'settings.credential_applied': 'credentialAppliedFormat',
  'settings.credential_not_applied': 'credentialNotAppliedFormat',
  'settings.keyless_status': 'keylessReadinessText',
  'overlays.clear_action': 'clearOverlaysActionText',
  'scenes.heading': 'demoScenesGroupLabel',
  'plans.heading': 'demoPlansGroupLabel',
  'camera.presets_heading': 'cameraPresetsGroupLabel',
  'theme.heading': 'themeRowLabel',
  'offline.heading': 'offlineMapsGroupLabel',
  'offline.plan_route_action': 'offlineRouteActionText',
  'offline.recorded_graph_route_title': 'offlineRouteTitleText',
  'offline.recorded_graph_note': 'offlineRouteNoteText',
  'offline.download_action': 'offlineRegionDownloadActionText',
  'offline.downloading_status': 'offlineRegionDownloadingStatusText',
  'offline.cancel_action': 'offlineRegionCancelActionText',
  'offline.completed_status': 'offlineRegionCompletedStatusText',
  'offline.failed_status': 'offlineRegionFailedStatusText',
  'offline.stored_tiles_label': 'offlineRegionStoredTilesLabel',
  'offline.stored_bytes_label': 'offlineRegionStoredBytesLabel',
  'offline.delete_action': 'offlineRegionDeleteActionText',
  'offline.deleted_status': 'offlineRegionDeletedStatusText',
  'offline.browser_unavailable_status': 'offlineRegionBrowserUnavailableStatusText',
  'diagnostics.title': 'diagnosticsTitleText',
  'plan.origin_label': 'originFieldLabelText',
  'plan.destination_label': 'destinationFieldLabelText',
  'plan.search_action': 'searchSubmitLabel',
  'plan.my_location_action': 'myLocationActionText',
  'plan.swap_action': 'swapPlanFieldsActionText',
  'plan.missing_origin': 'planMissingOriginText',
  'plan.missing_destination': 'planMissingDestinationText',
  'plan.missing_both': 'planMissingBothText',
  'plan.my_location_unavailable': 'myLocationUnavailableFormat',
  'plan.trip_source_demo_plan': 'tripSourceDemoPlanFormat',
  'settings.provider_row': 'providerRowFormat',
  'settings.provider_state_ready': 'providerStateReadyText',
  'settings.provider_state_needs_credential': 'providerStateNeedsCredentialFormat',
  'settings.provider_state_not_configured': 'capabilityNotConfiguredText',
  'settings.routing_demo_endpoint': 'routingDemoEndpointFormat',
  'settings.routing_unreachable_next_step': 'routingUnreachableNextStepText',
  'diagnostics.show_debug_events': 'diagnosticsShowDebugEventsText',
  'diagnostics.hide_debug_events': 'diagnosticsHideDebugEventsText',
  'diagnostics.clear_action': 'diagnosticsClearActionText',
  'diagnostics.no_events': 'diagnosticsNoEventsText',
  'diagnostics.stream_scope_note': 'diagnosticsStreamScopeNoteText',
};

/** React Native `appStrings` keys mapped to canonical consumer-copy IDs. */
const REACT_NATIVE_UI_LABEL_KEYS = {
  'plan.origin_label': 'planOriginLabelText',
  'plan.destination_label': 'planDestinationLabelText',
  'plan.search_action': 'planSearchActionText',
  'plan.my_location_action': 'planMyLocationActionText',
  'plan.swap_action': 'planSwapActionText',
  'plan.missing_origin': 'planMissingOriginText',
  'plan.missing_destination': 'planMissingDestinationText',
  'plan.missing_both': 'planMissingBothText',
  'plan.my_location_unavailable': 'planMyLocationUnavailableFormat',
  'plan.trip_source_demo_plan': 'planTripSourceDemoPlanFormat',
  'settings.provider_row': 'settingsProviderRowFormat',
  'settings.provider_state_ready': 'settingsProviderStateReadyText',
  'settings.provider_state_needs_credential': 'settingsProviderStateNeedsCredentialFormat',
  'settings.provider_state_not_configured': 'settingsProviderStateNotConfiguredText',
  'settings.routing_demo_endpoint': 'settingsRoutingDemoEndpointFormat',
  'settings.routing_unreachable_next_step': 'settingsRoutingUnreachableNextStepText',
  'diagnostics.show_debug_events': 'diagnosticsShowDebugEventsText',
  'diagnostics.hide_debug_events': 'diagnosticsHideDebugEventsText',
  'diagnostics.clear_action': 'diagnosticsClearActionText',
  'diagnostics.no_events': 'diagnosticsNoEventsText',
  'diagnostics.stream_scope_note': 'diagnosticsStreamScopeNoteText',
  'peek.primary_action': 'peekPrimaryActionText',
  'plan.origin_hint': 'originFieldHintText',
  'plan.destination_hint': 'whereToText',
  'plan.destination_placeholder': 'destinationFieldHintText',
  'plan.search_heading': 'searchHeadingText',
  'plan.plan_route_action': 'planRouteButtonText',
  'plan.planning_in_flight': 'routeSummaryPlanningText',
  'plan.searching_in_flight': 'searchInFlightText',
  'plan.empty_results_hint': 'searchHintText',
  'plan.no_matches': 'planAutocompleteNoMatchesText',
  'plan.autocomplete_unavailable': 'planAutocompleteFailedText',
  'plan.demo_places_heading': 'demoPlacesHeadingText',
  'plan.demo_no_match_hint': 'demoNoMatchHintFormat',
  'plan.recorded_route_note': 'recordedDemoRouteNoteText',
  'plan.no_recorded_route_hint': 'noRecordedDemoRouteHintFormat',
  'route.start_action': 'routeSummaryStartButtonText',
  'route.edit_action': 'routeSummaryEditButtonText',
  'navigation.exit_action': 'navigationDrawerExitButtonText',
  'navigation.eta_prefix': 'navigationDrawerEtaLabelText',
  'navigation.phase_not_started': 'phaseNotStartedText',
  'navigation.voice_toggle': 'speechToggleLabelText',
  'navigation.speed_label': 'navigationSpeedLabelText',
  'navigation.pause_action': 'navigationPauseActionText',
  'navigation.resume_action': 'navigationResumeActionText',
  'navigation.speed_multiplier_label': 'navigationSpeedMultiplierFormat',
  'navigation.seek_label': 'navigationSeekLabelText',
  'readiness.chip_ready': 'readinessChipReadyText',
  'readiness.chip_loading': 'readinessChipLoadingText',
  'readiness.chip_degraded_prefix': 'readinessChipDegradedPrefix',
  'readiness.chip_failed_prefix': 'readinessChipFailedPrefix',
  'settings.open_accessibility': 'settingsGearAccessibilityText',
  'settings.close_action': 'settingsDoneText',
  'settings.credential_applied': 'providerCredentialAppliedStatusText',
  'settings.credential_not_applied': 'providerCredentialKeylessStatusText',
  'settings.keyless_status': 'providerCredentialKeylessNeutralText',
  'overlays.clear_action': 'demoScenesClearText',
  'scenes.heading': 'demoScenesRowTitleText',
  'plans.heading': 'demoPlansRowTitleText',
  'camera.presets_heading': 'cameraSectionHeader',
  'theme.heading': 'themeRowTitleText',
  'offline.heading': 'offlineMapsCardTitleText',
  'offline.plan_route_action': 'offlineRouteActionText',
  'offline.recorded_graph_route_title': 'offlineRouteTitleText',
  'offline.recorded_graph_note': 'offlineRouteNoteText',
  'offline.download_action': 'offlineDownloadActionText',
  'offline.downloading_status': 'offlineDownloadingStatusText',
  'offline.cancel_action': 'offlineCancelActionText',
  'offline.completed_status': 'offlineCompletedStatusText',
  'offline.failed_status': 'offlineFailedStatusText',
  'offline.stored_tiles_label': 'offlineStoredTilesLabelText',
  'offline.stored_bytes_label': 'offlineStoredBytesLabelText',
  'offline.delete_action': 'offlineDeleteActionText',
  'offline.deleted_status': 'offlineDeletedStatusText',
  'offline.browser_unavailable_status': 'offlineBrowserUnavailableStatusText',
  'diagnostics.title': 'diagnosticsTitle',
};

/**
 * The two React Native native host constants files must carry the same value.
 * Both names are listed so a rename on one host cannot hide the drift.
 */
const REACT_NATIVE_HOST_UI_LABEL_CONSTANTS = {
  'navigation.phase_navigating': {
    kotlin: 'NAVIGATION_PHASE_LABEL_NAVIGATING',
    swift: 'navigationPhaseLabelNavigating',
  },
  'navigation.phase_off_route': {
    kotlin: 'NAVIGATION_PHASE_LABEL_OFF_ROUTE',
    swift: 'navigationPhaseLabelOffRoute',
  },
  'navigation.phase_arrived': {
    kotlin: 'NAVIGATION_PHASE_LABEL_ARRIVED',
    swift: 'navigationPhaseLabelArrived',
  },
  'navigation.deviation_alert': {
    kotlin: 'DEVIATION_TEXT_TEMPLATE',
    swift: 'deviationTextTemplate',
  },
  // The offline route's summary title is emitted by both native hosts, so the
  // hidden fifth copy must agree on it.
  'offline.recorded_graph_route_title': {
    kotlin: 'OFFLINE_ROUTE_TITLE_TEXT',
    swift: 'offlineRouteTitleText',
  },
};

/**
 * Reads the Compose constants file.
 * @param {(relativePath: string) => string} read
 * @returns {Map<string, {value: string, source: string}>}
 */
function readAndroidStringMirror(read) {
  const constants = parseKotlinStringConstants(read(STRING_MIRROR_FILES.androidConstants));
  return mapConstantsToUiLabels(
    constants,
    ANDROID_UI_LABEL_CONSTANTS,
    'ReferenceApplicationConstants.kt',
  );
}

/**
 * Reads the SwiftUI configuration file.
 * @param {(relativePath: string) => string} read
 * @returns {Map<string, {value: string, source: string}>}
 */
function readIosStringMirror(read) {
  const constants = parseSwiftStringConstants(read(STRING_MIRROR_FILES.iosConfiguration));
  return mapConstantsToUiLabels(
    constants,
    IOS_UI_LABEL_CONSTANTS,
    'ReferenceApplicationConfiguration.swift',
  );
}

/**
 * Reads the React Native JS shell (`theme.ts`, with the navigation drawer
 * asserting the ETA composition) and both native host constant files. The
 * hosts must agree on every value they both declare.
 * @param {(relativePath: string) => string} read
 * @returns {Map<string, {value: string, source: string}>}
 */
function readReactNativeStringMirror(read) {
  const values = new Map();
  const themeText = read(STRING_MIRROR_FILES.reactNativeTheme);
  const appStringsBlock = extractBracedBlockAfter(themeText, 'export const appStrings = {');
  const appStrings = new Map();
  for (const match of appStringsBlock.matchAll(/(\w+):\s*'((?:[^'\\]|\\.)*)'/g)) {
    appStrings.set(match[1], match[2].replace(/\\'/g, "'"));
  }
  for (const [id, key] of Object.entries(REACT_NATIVE_UI_LABEL_KEYS)) {
    if (appStrings.has(key)) {
      values.set(id, { value: appStrings.get(key), source: `theme.ts:${key}` });
    }
  }

  // The drawer composes the ETA line from this label; a page that stops using
  // it would leave the mirror value dead, so assert the composition remains.
  const navigationDrawerText = read(
    STRING_MIRROR_FILES.reactNativeNavigationDrawer,
  );
  if (
    !navigationDrawerText.includes('appStrings.navigationDrawerEtaLabelText')
  ) {
    throw new Error(
      'NavigationDrawer.tsx no longer composes the ETA line from appStrings.navigationDrawerEtaLabelText',
    );
  }

  const androidHost = parseKotlinStringConstants(
    read(STRING_MIRROR_FILES.reactNativeAndroidHostConstants),
  );
  const iosHost = parseSwiftStringConstants(
    read(STRING_MIRROR_FILES.reactNativeIosHostConstants),
  );
  for (const [id, names] of Object.entries(REACT_NATIVE_HOST_UI_LABEL_CONSTANTS)) {
    const androidValue = androidHost.get(names.kotlin);
    const iosValue = iosHost.get(names.swift);
    if (androidValue !== undefined && iosValue !== undefined
      && normalizeInterpolation(androidValue) !== normalizeInterpolation(iosValue)) {
      throw new Error(
        `the two native hosts disagree on "${id}" ("${androidValue}" vs "${iosValue}")`,
      );
    }
    const value = androidValue ?? iosValue;
    if (value !== undefined) {
      values.set(id, {
        value,
        source: androidValue !== undefined
          ? `RefappConstants.kt:${names.kotlin}`
          : `RefappConstants.swift:${names.swift}`,
      });
    }
  }
  return values;
}

/** The web markup's consumer-copy attributes (M28 slice 5). */
const WEB_MARKUP_LABEL_ATTRIBUTE = 'data-ui-label';
const WEB_MARKUP_LABEL_TARGET_ATTRIBUTE = 'data-ui-label-attribute';
const WEB_MARKUP_ACCESSIBLE_LABEL_ATTRIBUTE = 'data-ui-label-aria';
const WEB_ACCESSIBLE_NAME_ATTRIBUTE = 'aria-label';

/** The controller calls that mark a dynamic label as wired to the module. */
const WEB_DYNAMIC_LABEL_PATTERN =
  /\bui_label_text(?:_with_values)?\(\s*'([^']+)'/g;

/** The module's own `'id': 'text',` declarations. */
const WEB_MODULE_LABEL_PATTERN = /^\s*'([^']+)':\s*'([^']*)',?$/gm;

/** HTML entities the page's labeled markup can carry. */
const HTML_ENTITY_REPLACEMENTS = Object.freeze({
  amp: '&',
  lt: '<',
  gt: '>',
  quot: '"',
  apos: "'",
  nbsp: ' ',
  harr: '↔',
  larr: '←',
  rarr: '→',
  times: '×',
});

/** Radices of the numeric HTML entities (`&#8212;`, `&#x2014;`). */
const HTML_DECIMAL_RADIX = 10;
const HTML_HEXADECIMAL_RADIX = 16;

/**
 * Reads one quoted attribute from an opening tag's attribute text.
 * @param {string} attribute_text
 * @param {string} attribute_name
 * @returns {string | null}
 */
function readMarkupAttribute(attribute_text, attribute_name) {
  const attribute_pattern =
    new RegExp(`(?:^|\\s)${attribute_name}\\s*=\\s*(?:"([^"]*)"|'([^']*)')`);
  const match = attribute_pattern.exec(attribute_text);
  if (match === null) {
    return null;
  }
  return match[1] ?? match[2];
}

/**
 * Decodes the named and numeric HTML entities a labeled element can contain.
 * @param {string} source
 * @returns {string}
 */
function decodeHtmlEntities(source) {
  return source.replace(
    /&(#x[0-9a-fA-F]+|#[0-9]+|[a-zA-Z][a-zA-Z0-9]*);/g,
    (entity, entity_body) => {
      if (entity_body.startsWith('#x')) {
        return String.fromCodePoint(
          Number.parseInt(entity_body.slice(2), HTML_HEXADECIMAL_RADIX));
      }
      if (entity_body.startsWith('#')) {
        return String.fromCodePoint(
          Number.parseInt(entity_body.slice(1), HTML_DECIMAL_RADIX));
      }
      return HTML_ENTITY_REPLACEMENTS[entity_body] ?? entity;
    },
  );
}

/**
 * Reads an element's rendered text as a browser would collapse it: tags
 * dropped, entities decoded, whitespace collapsed.
 * @param {string} source
 * @returns {string}
 */
function normalizeMarkupText(source) {
  return decodeHtmlEntities(source.replace(/<[^>]*>/g, ' '))
    .replace(/\s+/g, ' ')
    .trim();
}

/**
 * Reads the text between an opening tag and its closing tag.
 * @param {string} markup_text
 * @param {number} body_start_index
 * @param {string} tag_name
 * @returns {string}
 */
function readMarkedElementText(markup_text, body_start_index, tag_name) {
  const closing_index = markup_text.indexOf(`</${tag_name}>`, body_start_index);
  if (closing_index < 0) {
    throw new Error(`unclosed <${tag_name}> carrying a consumer label`);
  }
  return normalizeMarkupText(markup_text.slice(body_start_index, closing_index));
}

/**
 * Reads the web surface where it actually renders its copy (M28 slice 5):
 *   - every `data-ui-label` element in index.html contributes its rendered
 *     text, or the attribute named by `data-ui-label-attribute` (an input's
 *     placeholder, an aria-label); `data-ui-label-aria` contributes the
 *     accessible name;
 *   - every `ui_label_text('<id>')` / `ui_label_text_with_values('<id>', …)`
 *     call in playground.mjs contributes that dynamic label (its module value,
 *     which the artifact comparison then verifies).
 * Reading only `ui_strings.mjs` was the slice-1 blind spot: a page that
 * hardcoded different wording still reported 37/37. A role the page neither
 * marks nor references is now reported as missing unless it is listed in
 * `LABEL_PLATFORM_OMISSIONS.web` with a reason.
 * @param {(relativePath: string) => string} read
 * @returns {Map<string, {value: string, source: string}>}
 */
function readWebStringMirror(read) {
  const values = new Map();
  const module_labels = new Map();
  for (const match of read(STRING_MIRROR_FILES.webStrings)
    .matchAll(WEB_MODULE_LABEL_PATTERN)) {
    module_labels.set(match[1], match[2]);
  }

  const markup_text = read(STRING_MIRROR_FILES.webPageMarkup);
  for (const match of markup_text.matchAll(/<([a-zA-Z][\w-]*)([^>]*)>/g)) {
    const tag_name = match[1];
    const attribute_text = match[2];
    const body_start_index = match.index + match[0].length;
    const label_id = readMarkupAttribute(attribute_text, WEB_MARKUP_LABEL_ATTRIBUTE);
    if (label_id !== null) {
      const target_attribute = readMarkupAttribute(
        attribute_text, WEB_MARKUP_LABEL_TARGET_ATTRIBUTE);
      let label_text;
      if (target_attribute === null) {
        label_text = readMarkedElementText(markup_text, body_start_index, tag_name);
      } else {
        label_text = readMarkupAttribute(attribute_text, target_attribute);
        if (label_text === null) {
          throw new Error(
            `index.html "${label_id}" has no "${target_attribute}" attribute`);
        }
        label_text = normalizeMarkupText(label_text);
      }
      values.set(label_id, { value: label_text, source: `index.html:${label_id}` });
    }
    const accessible_label_id = readMarkupAttribute(
      attribute_text, WEB_MARKUP_ACCESSIBLE_LABEL_ATTRIBUTE);
    if (accessible_label_id !== null) {
      const accessible_name = readMarkupAttribute(
        attribute_text, WEB_ACCESSIBLE_NAME_ATTRIBUTE);
      if (accessible_name === null) {
        throw new Error(
          `index.html "${accessible_label_id}" has no "${WEB_ACCESSIBLE_NAME_ATTRIBUTE}" attribute`);
      }
      values.set(accessible_label_id, {
        value: normalizeMarkupText(accessible_name),
        source: `index.html:${accessible_label_id} (aria-label)`,
      });
    }
  }

  const controller_text = read(STRING_MIRROR_FILES.webPageController);
  for (const match of controller_text.matchAll(WEB_DYNAMIC_LABEL_PATTERN)) {
    const label_id = match[1];
    if (!module_labels.has(label_id)) {
      throw new Error(
        `playground.mjs reads "${label_id}", which ui_strings.mjs does not declare`);
    }
    if (!values.has(label_id)) {
      values.set(label_id, {
        value: module_labels.get(label_id),
        source: `playground.mjs:ui_label_text("${label_id}")`,
      });
    }
  }
  return values;
}

/**
 * Checks the §2.5 consumer-copy table against the artifact so the document and
 * the shared source cannot drift.
 * @param {(relativePath: string) => string} read
 * @param {Map<string, UiLabelExpectation>} expectations
 * @returns {string[]}
 */
function checkConsumerCopyTable(read, expectations) {
  const failures = [];
  let documentText;
  try {
    documentText = read(CONSUMER_COPY_DOCUMENT);
  } catch (error) {
    return [`[document] could not read ${CONSUMER_COPY_DOCUMENT}: ${error.message}`];
  }
  const headingIndex = documentText.indexOf(CONSUMER_COPY_DOCUMENT_HEADING);
  if (headingIndex < 0) {
    return [`[document] ${CONSUMER_COPY_DOCUMENT} has no "${CONSUMER_COPY_DOCUMENT_HEADING}" heading`];
  }
  const sectionEndIndex = documentText.indexOf('\n## ', headingIndex);
  const section = documentText.slice(
    headingIndex,
    sectionEndIndex < 0 ? documentText.length : sectionEndIndex,
  );
  const rowPattern = /^\|\s*`([^`]+)`\s*\|\s*(.+?)\s*\|\s*(.+?)\s*\|\s*$/gm;
  const seen = new Set();
  for (const match of section.matchAll(rowPattern)) {
    const id = match[1];
    const documentedText = match[2].trim();
    const overrideCell = match[3].trim();
    const expectation = expectations.get(id);
    if (expectation === undefined) {
      failures.push(`[document] §2.5 names "${id}", which the artifact does not declare`);
      continue;
    }
    seen.add(id);
    if (documentedText !== expectation.text.trim()) {
      failures.push(`[document] §2.5 "${id}" drifted from the artifact's canonical text`);
    }
    const hasOverrides = Object.keys(expectation.overrides).length > 0;
    if (hasOverrides === (overrideCell === '—')) {
      failures.push(`[document] §2.5 "${id}" override column does not match the artifact`);
    }
  }
  for (const id of expectations.keys()) {
    if (!seen.has(id)) {
      failures.push(`[document] §2.5 is missing "${id}"`);
    }
  }
  return failures;
}

/**
 * Canonical IDs a platform cannot yet mirror, because this slice reads only
 * the named constant/string files and the text lives inline in a view. They
 * are reported, never silently passed, exactly like PLATFORM_OMISSIONS.
 */
const LABEL_PLATFORM_OMISSIONS = {
  android: new Set([
    'plan.search_heading',
    'camera.presets_heading',
    // Android's inline provider failure is the contract's own detail text;
    // it invents no second sentence.
    'plan.autocomplete_unavailable',
  ]),
  rn: new Set([
    'navigation.eta_distance_label',
    'navigation.eta_duration_label',
  ]),
  web: new Set([
    // The web trip card has no peek surface; its pre-plan action is the
    // explicit Plan route control (plan.plan_route_action).
    'peek.primary_action',
    // The two endpoint inputs are edited in place; there is no separate
    // return-to-search action.
    'route.edit_action',
    // The demo pane always shows the navigating-phase sample; a pre-trip
    // phase has no surface.
    'navigation.phase_not_started',
    // The demo pane renders the scene artifact's own preview sentence, which
    // is demo content, not consumer chrome.
    'navigation.deviation_alert',
    // A keyless provider row carries no negative chip; the row's own state
    // already says no key is applied.
    'settings.credential_not_applied',
    // The v1 page renders its keyless demo-place search and recorded-route
    // sentences as free-form status strings; the canonical IDs are consumed
    // by the native shells' `ui_labels` mirrors (the page's own copy is the
    // wording those rows derive from).
    'plan.demo_places_heading',
    'plan.demo_no_match_hint',
    'plan.recorded_route_note',
    'plan.no_recorded_route_hint',
    // The web type-ahead hides its list on an empty or failed answer; it
    // renders no no-match row and no inline failure sentence.
    'plan.no_matches',
    'plan.autocomplete_unavailable',
    // The layers panel titles its scene section "Overlays" (the section also
    // carries the traffic switch), and the plan drawer no longer lists demo
    // plans; the canonical headings have no surface on the page.
    'scenes.heading',
    'plans.heading',
    // The web planner's one always-visible destination field is the search
    // view's field, so it carries the search placeholder
    // (plan.destination_placeholder); the peek's shorter hint has no surface.
    'plan.destination_hint',
    // The browser has no disk store behind the region download, so the page
    // renders no offline-region card. The published offline-maps chapter
    // states the limitation; the native shells carry the controls.
    'offline.download_action',
    'offline.downloading_status',
    'offline.cancel_action',
    'offline.completed_status',
    'offline.failed_status',
    'offline.stored_tiles_label',
    'offline.stored_bytes_label',
    'offline.delete_action',
    'offline.deleted_status',
    'offline.browser_unavailable_status',
  ]),
};

/** Why each omission exists, shown in the consumer-copy summary. */
const LABEL_OMISSION_NOTES = {
  android: 'the constants file carries every canonical value; the omissions are roles Android has no surface for',
  ios: 'the configuration file carries every canonical value',
  rn: 'the native hosts compose the contract progress line with the ETA-distance and ETA-duration labels; the JS shell does not duplicate them',
  web: "the page renders the shared copy; the omitted roles have no canonical ui_labels surface on the v1 page (its keyless demo copy is free-form status text)",
};

/**
 * The web playground's documented v1 developer-panel vocabulary (audit §2.4,
 * docs/ReferenceAppUX.md §1): older by design, with no canonical UI label.
 * Printed beside the web coverage so the exclusion is visible, never silent.
 */
const WEB_V1_DEVELOPER_COPY = Object.freeze({
  labels: Object.freeze([
    'Advanced',
    'Debug',
    'Zoom',
    'Bearing',
    'Pitch',
    'Show renderer readout',
    'tile grid overlay (debug)',
    'Refresh recent SDK events',
  ]),
  reason: 'the web playground keeps its documented v1 developer-panel vocabulary (these controls and the diagnostics readout facts/event lines); these strings have no canonical ID by design',
});

// --- Android (Compose) mirror adapter --------------------------------------

/** Material typography slots mapped to their §2.3 token names (§8). */
const ANDROID_TYPOGRAPHY_SLOTS = {
  labelSmall: 'type-caption',
  bodySmall: 'type-body',
  labelLarge: 'type-body-strong',
  titleSmall: 'type-title',
  headlineSmall: 'type-maneuver',
  headlineMedium: 'type-maneuver-distance',
};

/** Compose constant names mapped to artifact keys. */

/** Android behavior constants mirrored from the artifact's diagnostics rows. */
const ANDROID_BEHAVIOR_NUMBER_KEYS = {
  DIAGNOSTICS_EVENT_WINDOW: 'behavior/diagnostics-event-window',
  DIAGNOSTICS_BACKLOG_FLUSH_EVENT_LIMIT: 'behavior/diagnostics-backlog-flush-limit',
  DIAGNOSTICS_REFRESH_CADENCE_MILLISECONDS: 'behavior/diagnostics-refresh-cadence-milliseconds',
  DIAGNOSTICS_RING_CAPACITY: 'behavior/diagnostics-ring-capacity',
};

/** Android string-valued behavior constants. */
const ANDROID_BEHAVIOR_STRING_KEYS = {
  DIAGNOSTICS_MINIMUM_SEVERITY_NAME: 'behavior/diagnostics-minimum-severity',
  DIAGNOSTICS_DEBUG_SEVERITY_NAME: 'behavior/diagnostics-debug-severity',
  LOCATION_ANIMATION_DEAD_RECKONING: 'behavior/location-animation-dead-reckoning',
};

/** React Native diagnostics-token keys mapped to artifact tokens. */
const RN_DIAGNOSTICS_KEYS = {
  diagnosticsEventWindow: 'behavior/diagnostics-event-window',
  diagnosticsBacklogFlushLimit: 'behavior/diagnostics-backlog-flush-limit',
  diagnosticsRefreshCadenceMilliseconds: 'behavior/diagnostics-refresh-cadence-milliseconds',
  diagnosticsRingCapacity: 'behavior/diagnostics-ring-capacity',
  diagnosticsMinimumSeverity: 'behavior/diagnostics-minimum-severity',
  diagnosticsDebugSeverity: 'behavior/diagnostics-debug-severity',
};

/** Web diagnostics constants mapped to artifact tokens. */
const WEB_DIAGNOSTICS_NUMBER_KEYS = {
  DIAGNOSTICS_EVENT_WINDOW: 'behavior/diagnostics-event-window',
  DIAGNOSTICS_BACKLOG_FLUSH_LIMIT: 'behavior/diagnostics-backlog-flush-limit',
  DIAGNOSTICS_REFRESH_CADENCE_MILLISECONDS: 'behavior/diagnostics-refresh-cadence-milliseconds',
  DIAGNOSTICS_RING_CAPACITY: 'behavior/diagnostics-ring-capacity',
};

/** Web string-valued diagnostics constants. */
const WEB_DIAGNOSTICS_STRING_KEYS = {
  DIAGNOSTICS_MINIMUM_SEVERITY_NAME: 'behavior/diagnostics-minimum-severity',
  DIAGNOSTICS_DEBUG_SEVERITY_NAME: 'behavior/diagnostics-debug-severity',
};

const ANDROID_DIMENSION_KEYS = {
  GutterInline: 'spacing/gutter-inline',
  GutterBlock: 'spacing/gutter-block',
  PanelPadding: 'spacing/panel-padding',
  StackTight: 'spacing/stack-tight',
  StackGroup: 'spacing/stack-group',
  MapControlDiameter: 'spacing/map-control-diameter',
  RadiusSurface: 'radius/radius-surface',
  RadiusControl: 'radius/radius-control',
  RadiusInline: 'radius/radius-inline',
  SHEET_HALF_HEIGHT_FRACTION: 'behavior/sheet-half-height',
  SHEET_HALF_PHONE_HEIGHT_FRACTION: 'behavior/sheet-half-phone-height',
  SHEET_PEEK_CONTENT_ROWS: 'behavior/sheet-peek-content-rows',
  FIELD_OF_VIEW_DEGREES: 'behavior/field-of-view-degrees',
  AUTOCOMPLETE_MINIMUM_QUERY_CHARACTERS: 'behavior/autocomplete-minimum-query-characters',
  AUTOCOMPLETE_DEBOUNCE_MILLISECONDS: 'behavior/autocomplete-debounce-milliseconds',
  AUTOCOMPLETE_MAXIMUM_RESULTS: 'behavior/autocomplete-maximum-results',
  NAVIGATION_FOLLOW_ZOOM: 'behavior/navigation-follow-zoom',
  NAVIGATION_FOLLOW_ZOOM_MAXIMUM: 'behavior/navigation-follow-zoom-maximum',
  NAVIGATION_FOLLOW_PITCH_3D_DEGREES: 'behavior/navigation-follow-pitch-3d',
  MAP_CONTROL_ZOOM_STEP: 'behavior/map-control-zoom-step',
  MAP_CONTROL_ZOOM_REPEAT_INTERVAL_MILLISECONDS: 'behavior/map-control-zoom-repeat-interval',
  COMPASS_VISIBILITY_BEARING_THRESHOLD_DEGREES: 'behavior/compass-visibility-bearing-threshold',
  EASED_CAMERA_MOVE_DURATION_MILLISECONDS: 'behavior/compass-renorth-animation',
  LOCATION_ANIMATION_DURATION_MILLISECONDS: 'behavior/location-animation-duration-milliseconds',
  LOCATION_ANIMATION_MINIMUM_MILLISECONDS: 'behavior/location-animation-minimum-milliseconds',
};

/**
 * Reads the Compose theme (colors and typography), the dimensions object and
 * the error-detail mapping.
 * @param {(relativePath: string) => string} read
 * @returns {Map<string, any>}
 */
function readAndroidMirror(read) {
  const values = new Map();
  const themeText = read(MIRROR_FILES.androidTheme);

  const themeBlocks = [
    ['dark', 'val ReferenceAppUXTokenColorsDark'],
    ['light', 'val ReferenceAppUXTokenColorsLight'],
  ];
  for (const [themeName, marker] of themeBlocks) {
    const block = extractParenBlockAfter(themeText, marker);
    for (const line of block.split('\n')) {
      const assignment = /^\s*(\w+)\s*=\s*(Color\(.*\))\s*,?\s*$/.exec(line);
      if (assignment === null) {
        continue;
      }
      values.set(`${camelToKebab(assignment[1])}/${themeName}`, parseKotlinColor(assignment[2]));
    }
  }

  const typographyBlock = extractParenBlockAfter(
    themeText,
    'fun mapTokenTypographyIntoTypography(): Typography = Typography(',
  );
  const textStylePattern = /(\w+)\s*=\s*TextStyle\(([\s\S]*?)\n\s*\),/g;
  for (const match of typographyBlock.matchAll(textStylePattern)) {
    const token = ANDROID_TYPOGRAPHY_SLOTS[match[1]];
    if (token === undefined) {
      continue;
    }
    const size = /fontSize\s*=\s*([\d.]+)\.sp/.exec(match[2]);
    const weight = /fontWeight\s*=\s*FontWeight\((\d+)\)/.exec(match[2]);
    if (size !== null) {
      values.set(`${token}/size`, parseNumber(size[1]));
    }
    if (weight !== null) {
      values.set(`${token}/weight`, parseNumber(weight[1]));
    }
  }

  const dimensionsText = read(MIRROR_FILES.androidDimensions);
  const dimensionPattern =
    /(?:const val|val) (\w+): (?:Dp|Float|Int|Long|Double) = ([\d.]+)(?:f|\.dp)?/g;
  const dimensions = new Map();
  for (const match of dimensionsText.matchAll(dimensionPattern)) {
    dimensions.set(match[1], parseNumber(match[2]));
  }
  for (const [constant, key] of Object.entries(ANDROID_DIMENSION_KEYS)) {
    if (dimensions.has(constant)) {
      values.set(key, dimensions.get(constant));
    }
  }
  if (dimensions.has('SCALE_BAR_WIDTH_MINIMUM_PIXELS')
    && dimensions.has('SCALE_BAR_WIDTH_MAXIMUM_PIXELS')) {
    values.set('behavior/scale-bar-width-range', {
      minimum: dimensions.get('SCALE_BAR_WIDTH_MINIMUM_PIXELS'),
      maximum: dimensions.get('SCALE_BAR_WIDTH_MAXIMUM_PIXELS'),
    });
  }

  const constantsText = read(STRING_MIRROR_FILES.androidConstants);
  const parsedStringConstants = parseKotlinStringConstants(constantsText);
  for (const [constant, key] of Object.entries(ANDROID_BEHAVIOR_STRING_KEYS)) {
    if (parsedStringConstants.has(constant)) {
      values.set(key, parsedStringConstants.get(constant));
    }
  }
  const numericConstantPattern = /const val (\w+): (?:Int|Long|UInt) = (\d+)u?L?/g;
  for (const match of constantsText.matchAll(numericConstantPattern)) {
    const key = ANDROID_BEHAVIOR_NUMBER_KEYS[match[1]];
    if (key !== undefined) {
      values.set(key, parseNumber(match[2]));
    }
  }

  const stateText = read(MIRROR_FILES.androidState);
  const rateLimit = /is SdkException\.RateLimitExhausted\s*->\s*"([^"]*)"/.exec(stateText);
  const provider = /is SdkException\.ProviderNotConfigured\s*->\s*"([^"]*)"/.exec(stateText);
  if (rateLimit !== null) {
    values.set('string/rate_limit_exhausted_detail', normalizeErrorTemplate(rateLimit[1]));
  }
  if (provider !== null) {
    values.set(
      'string/provider_not_configured_detail_template',
      normalizeErrorTemplate(provider[1]),
    );
  }
  return values;
}

/**
 * Normalizes a Kotlin/Swift interpolated error sentence to the artifact's
 * `{provider}` placeholder form.
 * @param {string} source
 * @returns {string}
 */
function normalizeErrorTemplate(source) {
  return source
    .replace(/\$\{[^}]*\}/g, '{provider}')
    .replace(/\\\([^)]*\)/g, '{provider}');
}

// --- iOS (SwiftUI) mirror adapter ------------------------------------------

/** `ReferenceAppUXFont` functions mapped to their §2.3 token names. */
const IOS_TYPOGRAPHY_FUNCTIONS = {
  caption: 'type-caption',
  body: 'type-body',
  bodyStrong: 'type-body-strong',
  title: 'type-title',
  maneuver: 'type-maneuver',
  maneuverDistance: 'type-maneuver-distance',
};

/** SwiftUI font weight names mapped to their numeric weights. */
const IOS_FONT_WEIGHTS = {
  regular: 400,
  semibold: 600,
};

/** AppLayout constant names mapped to artifact keys. */
const IOS_LAYOUT_KEYS = {
  gutterInline: 'spacing/gutter-inline',
  gutterBlock: 'spacing/gutter-block',
  panelPadding: 'spacing/panel-padding',
  stackTight: 'spacing/stack-tight',
  stackGroup: 'spacing/stack-group',
  mapControlDiameter: 'spacing/map-control-diameter',
  radiusSurface: 'radius/radius-surface',
  radiusControl: 'radius/radius-control',
  radiusInline: 'radius/radius-inline',
  fieldOfViewDegrees: 'behavior/field-of-view-degrees',
  autocompleteMinimumQueryCharacters: 'behavior/autocomplete-minimum-query-characters',
  autocompleteDebounceMilliseconds: 'behavior/autocomplete-debounce-milliseconds',
  autocompleteMaximumResults: 'behavior/autocomplete-maximum-results',
  mapControlZoomStep: 'behavior/map-control-zoom-step',
  compassVisibilityBearingThresholdDegrees: 'behavior/compass-visibility-bearing-threshold',
  locationAnimationDurationMilliseconds: 'behavior/location-animation-duration-milliseconds',
  locationAnimationMinimumMilliseconds: 'behavior/location-animation-minimum-milliseconds',
  sheetHalfHeightFraction: 'behavior/sheet-half-height',
  sheetHalfPhoneHeightFraction: 'behavior/sheet-half-phone-height',
  sheetPeekContentRows: 'behavior/sheet-peek-content-rows',
};

/** `AppLayout` string constants mapped to artifact keys. */
const IOS_LAYOUT_STRING_KEYS = {
  locationAnimationDeadReckoning: 'behavior/location-animation-dead-reckoning',
};

/**
 * Reads the SwiftUI theme (colors and fonts), the `AppLayout` constants, the
 * follow-camera configuration and the error-detail mapping.
 * @param {(relativePath: string) => string} read
 * @returns {Map<string, any>}
 */
function readIosMirror(read) {
  const values = new Map();
  const themeText = read(MIRROR_FILES.iosTheme);

  // Each token is three lines: the declaration, the dark value, the light
  // value, then the closing parenthesis; matching line-anchored keeps the
  // rgba calls' own parentheses out of the captures.
  const colorPattern =
    /static let (\w+) = SemanticColorToken\(\n\s*darkThemeValue:\s*(.+),\n\s*lightThemeValue:\s*(.+)\n\s*\)/g;
  for (const match of themeText.matchAll(colorPattern)) {
    const token = camelToKebab(match[1]);
    values.set(`${token}/dark`, parseIosColor(match[2]));
    values.set(`${token}/light`, parseIosColor(match[3]));
  }

  const typeScaleBlock = extractBracedBlockAfter(themeText, 'private enum TypeScale {');
  const sizes = new Map();
  const weights = new Map();
  for (const match of typeScaleBlock.matchAll(/static let (\w+): CGFloat = ([\d.]+)/g)) {
    sizes.set(match[1], parseNumber(match[2]));
  }
  for (const match of typeScaleBlock.matchAll(/static let (\w+): Font\.Weight = \.(\w+)/g)) {
    weights.set(match[1], IOS_FONT_WEIGHTS[match[2]]);
  }
  const fontPattern =
    /public static func (\w+)\(\) -> Font \{\s*\.system\(size: TypeScale\.(\w+), weight: TypeScale\.(\w+)\)\s*\}/g;
  for (const match of themeText.matchAll(fontPattern)) {
    const token = IOS_TYPOGRAPHY_FUNCTIONS[match[1]];
    if (token === undefined) {
      continue;
    }
    values.set(`${token}/size`, sizes.get(match[2]));
    values.set(`${token}/weight`, weights.get(match[3]));
  }

  const layoutText = read(MIRROR_FILES.iosLayout);
  const layoutPattern =
    /(?:public |private )static let (\w+): (?:CGFloat|Double|Int|TimeInterval) = ([\d.]+)/g;
  const layout = new Map();
  for (const match of layoutText.matchAll(layoutPattern)) {
    layout.set(match[1], parseNumber(match[2]));
  }
  for (const [constant, key] of Object.entries(IOS_LAYOUT_KEYS)) {
    if (layout.has(constant)) {
      values.set(key, layout.get(constant));
    }
  }
  const layoutStringPattern =
    /(?:public |private )static let (\w+): String = "([^"]*)"/g;
  const layoutStrings = new Map();
  for (const match of layoutText.matchAll(layoutStringPattern)) {
    layoutStrings.set(match[1], match[2]);
  }
  for (const [constant, key] of Object.entries(IOS_LAYOUT_STRING_KEYS)) {
    if (layoutStrings.has(constant)) {
      values.set(key, layoutStrings.get(constant));
    }
  }
  // The two time tokens are stored in seconds on iOS and in milliseconds in
  // the artifact; the adapter converts to the artifact's unit.
  for (const [constant, key] of [
    ['mapControlZoomRepeatInterval', 'behavior/map-control-zoom-repeat-interval'],
    ['compassRenorthAnimationSeconds', 'behavior/compass-renorth-animation'],
  ]) {
    if (layout.has(constant)) {
      values.set(key, layout.get(constant) * 1000);
    }
  }
  if (layout.has('scaleBarMinimumWidthPixels') && layout.has('scaleBarMaximumWidthPixels')) {
    values.set('behavior/scale-bar-width-range', {
      minimum: layout.get('scaleBarMinimumWidthPixels'),
      maximum: layout.get('scaleBarMaximumWidthPixels'),
    });
  }
  if (layout.has('elevationSurfaceOffsetY')
    && layout.has('elevationSurfaceRadius')
    && layout.has('elevationSurfaceOpacity')) {
    values.set('behavior/elevation-surface', {
      offsetY: layout.get('elevationSurfaceOffsetY'),
      blurRadius: layout.get('elevationSurfaceRadius'),
      color: {
        red: 0,
        green: 0,
        blue: 0,
        alpha: layout.get('elevationSurfaceOpacity'),
      },
    });
  }

  const configurationText = read(MIRROR_FILES.iosConfiguration);
  const configuration = new Map();
  for (const match of configurationText.matchAll(
    /(?:public |private )?static let (\w+): (?:CGFloat|Double|Int|TimeInterval) = ([\d.]+)/g,
  )) {
    configuration.set(match[1], parseNumber(match[2]));
  }
  for (const [constant, key] of [
    ['navigationFollowZoomLevel', 'behavior/navigation-follow-zoom'],
    ['navigationFollowZoomMaximumLevel', 'behavior/navigation-follow-zoom-maximum'],
    ['navigationFollowPitchDegrees3D', 'behavior/navigation-follow-pitch-3d'],
    ['diagnosticsEventWindowCount', 'behavior/diagnostics-event-window'],
    ['diagnosticsBacklogFlushLimit', 'behavior/diagnostics-backlog-flush-limit'],
    ['diagnosticsRefreshCadenceMilliseconds', 'behavior/diagnostics-refresh-cadence-milliseconds'],
    ['diagnosticsRingCapacity', 'behavior/diagnostics-ring-capacity'],
  ]) {
    if (configuration.has(constant)) {
      values.set(key, configuration.get(constant));
    }
  }
  // The two severity tokens are artifact strings; the configuration pins
  // them beside the SDK enum the subscription uses.
  const configurationStrings = parseSwiftStringConstants(configurationText);
  for (const [constant, key] of [
    ['diagnosticsMinimumSeverityName', 'behavior/diagnostics-minimum-severity'],
    ['diagnosticsDebugSeverityName', 'behavior/diagnostics-debug-severity'],
  ]) {
    if (configurationStrings.has(constant)) {
      values.set(key, configurationStrings.get(constant));
    }
  }

  const modelText = read(MIRROR_FILES.iosModel);
  // The variant-name switch earlier in the file also returns strings for the
  // same cases; the detail sentences live inside `sdkFailureDetailText`.
  const detailBlock = extractBracedBlockAfter(
    modelText,
    'func sdkFailureDetailText(_ sdkError: SdkError) -> String {',
  );
  const rateLimit = /case \.RateLimitExhausted:\s*return\s*"([^"]*)"/.exec(detailBlock);
  // The missing-provider sentence may be inline in the switch or delegated
  // to `ReferenceApplicationConfiguration.providerNotConfiguredDetailText`
  // (the configuration is the single copy the offline card and the error
  // report both render); follow the delegation in the latter case.
  const provider =
    /case \.ProviderNotConfigured\([^)]*\):\s*return\s*"([^"]*)"/.exec(detailBlock)
    ?? /static func providerNotConfiguredDetailText\(contractName: String\) -> String \{\s*"([^"]*)"/.exec(configurationText);
  if (rateLimit !== null) {
    values.set('string/rate_limit_exhausted_detail', normalizeErrorTemplate(rateLimit[1]));
  }
  if (provider !== null) {
    values.set(
      'string/provider_not_configured_detail_template',
      normalizeErrorTemplate(provider[1]),
    );
  }
  return values;
}

// --- React Native mirror adapter -------------------------------------------

/** RN theme object keys mapped to artifact keys. */
const RN_THEME_KEYS = {
  gutterInline: 'spacing/gutter-inline',
  gutterBlock: 'spacing/gutter-block',
  panelPadding: 'spacing/panel-padding',
  stackTight: 'spacing/stack-tight',
  stackGroup: 'spacing/stack-group',
  mapControlDiameter: 'spacing/map-control-diameter',
  fieldOfViewDegrees: 'behavior/field-of-view-degrees',
  autocompleteMinimumQueryCharacters: 'behavior/autocomplete-minimum-query-characters',
  autocompleteDebounceMilliseconds: 'behavior/autocomplete-debounce-milliseconds',
  autocompleteMaximumResults: 'behavior/autocomplete-maximum-results',
  navigationFollowZoom: 'behavior/navigation-follow-zoom',
  navigationFollowZoomMaximum: 'behavior/navigation-follow-zoom-maximum',
  navigationFollowPitch3dDegrees: 'behavior/navigation-follow-pitch-3d',
};

/** RN radius object keys mapped to artifact keys. */
const RN_RADIUS_KEYS = {
  surface: 'radius/radius-surface',
  control: 'radius/radius-control',
  inline: 'radius/radius-inline',
};

/** RN control-module exports mapped to artifact keys. */
const RN_CONTROL_KEYS = {
  compassVisibilityBearingThresholdDegrees: 'behavior/compass-visibility-bearing-threshold',
  mapControlZoomRepeatIntervalMilliseconds: 'behavior/map-control-zoom-repeat-interval',
  mapControlZoomStep: 'behavior/map-control-zoom-step',
  compassRenorthAnimationMilliseconds: 'behavior/compass-renorth-animation',
  locationAnimationDurationMilliseconds: 'behavior/location-animation-duration-milliseconds',
  locationAnimationMinimumMilliseconds: 'behavior/location-animation-minimum-milliseconds',
  sheetHalfHeightFraction: 'behavior/sheet-half-height',
  sheetHalfPhoneHeightFraction: 'behavior/sheet-half-phone-height',
  sheetPeekContentRows: 'behavior/sheet-peek-content-rows',
};

/** RN control-module string exports mapped to artifact keys. */
const RN_CONTROL_STRING_KEYS = {
  locationAnimationDeadReckoning: 'behavior/location-animation-dead-reckoning',
};

/**
 * Reads the React Native theme, the control cluster module and the sheet
 * layout module.
 * @param {(relativePath: string) => string} read
 * @returns {Map<string, any>}
 */
function readRnMirror(read) {
  const values = new Map();
  const themeText = read(MIRROR_FILES.rnTheme);

  for (const [themeName, marker] of [
    ['dark', 'export const darkThemeColors = {'],
    ['light', 'export const lightThemeColors = {'],
  ]) {
    const block = extractBracedBlockAfter(themeText, marker);
    for (const match of block.matchAll(/(\w+):\s*'([^']+)'/g)) {
      values.set(`${camelToKebab(match[1])}/${themeName}`, parseCssColor(match[2]));
    }
  }

  const typographyBlock = extractBracedBlockAfter(themeText, 'export const themeTypography = {');
  for (const match of typographyBlock.matchAll(/(\w+):\s*\{([^}]*)\}/g)) {
    const token = camelToKebab(match[1]);
    const size = /fontSize:\s*([\d.]+)/.exec(match[2]);
    const weight = /fontWeight:\s*'(\d+)'/.exec(match[2]);
    if (size !== null) {
      values.set(`${token}/size`, parseNumber(size[1]));
    }
    // An unlabelled fontWeight defaults to the regular 400 weight.
    values.set(`${token}/weight`, weight === null ? 400 : parseNumber(weight[1]));
  }

  const spacingBlock = extractBracedBlockAfter(themeText, 'export const themeSpacing = {');
  for (const match of spacingBlock.matchAll(/(\w+):\s*([\d.]+)/g)) {
    const key = RN_THEME_KEYS[match[1]];
    if (key !== undefined) {
      values.set(key, parseNumber(match[2]));
    }
  }
  const radiiBlock = extractBracedBlockAfter(themeText, 'export const themeRadii = {');
  for (const match of radiiBlock.matchAll(/(\w+):\s*([\d.]+)/g)) {
    const key = RN_RADIUS_KEYS[match[1]];
    if (key !== undefined) {
      values.set(key, parseNumber(match[2]));
    }
  }
  const framingBlock = extractBracedBlockAfter(themeText, 'export const themeFramingTokens = {');
  for (const match of framingBlock.matchAll(/(\w+):\s*([\d.]+)/g)) {
    const key = RN_THEME_KEYS[match[1]];
    if (key !== undefined) {
      values.set(key, parseNumber(match[2]));
    }
  }
  const elevationBlock =
    extractBracedBlockAfter(themeText, 'export const themeElevationSurface = {');
  const elevationColor = /color:\s*'([^']+)'/.exec(elevationBlock);
  const elevationOffset = /offsetY:\s*([\d.]+)/.exec(elevationBlock);
  const elevationBlur = /blurRadius:\s*([\d.]+)/.exec(elevationBlock);
  const elevationOpacity = /opacity:\s*([\d.]+)/.exec(elevationBlock);
  if (elevationColor !== null && elevationOffset !== null
    && elevationBlur !== null && elevationOpacity !== null) {
    const base = parseCssColor(elevationColor[1]);
    values.set('behavior/elevation-surface', {
      offsetY: parseNumber(elevationOffset[1]),
      blurRadius: parseNumber(elevationBlur[1]),
      color: { ...base, alpha: parseNumber(elevationOpacity[1]) },
    });
  }

  const diagnosticsBlock =
    extractBracedBlockAfter(themeText, 'export const themeDiagnosticsTokens = {');
  for (const match of diagnosticsBlock.matchAll(/(\w+):\s*(?:'(?:[^'\\]|\\.)*'|\d+)/g)) {
    const key = RN_DIAGNOSTICS_KEYS[match[1]];
    if (key === undefined) {
      continue;
    }
    const stringMatch = /'(?:[^'\\]|\\.)*'/.exec(match[0]);
    values.set(
      key,
      stringMatch === null
        ? parseNumber(match[0].replace(/^[^:]+:\s*/, ''))
        : match[0].replace(/^[^:]+:\s*'/, '').replace(/'$/, ''),
    );
  }
  const controlText = `${read(MIRROR_FILES.rnMapControls)}\n${read(MIRROR_FILES.rnSheetLayout)}`;
  const controlValues = new Map();
  for (const match of controlText.matchAll(/export const (\w+)\s*=\s*([\d.]+);/g)) {
    controlValues.set(match[1], parseNumber(match[2]));
  }
  for (const [constant, key] of Object.entries(RN_CONTROL_KEYS)) {
    if (controlValues.has(constant)) {
      values.set(key, controlValues.get(constant));
    }
  }
  for (const [constant, key] of Object.entries(RN_CONTROL_STRING_KEYS)) {
    const match = new RegExp(`export const ${constant}\\s*=\\s*'([^']*)';`).exec(controlText);
    if (match !== null) {
      values.set(key, match[1]);
    }
  }
  if (controlValues.has('scaleBarWidthMinimumPixels')
    && controlValues.has('scaleBarWidthMaximumPixels')) {
    values.set('behavior/scale-bar-width-range', {
      minimum: controlValues.get('scaleBarWidthMinimumPixels'),
      maximum: controlValues.get('scaleBarWidthMaximumPixels'),
    });
  }
  return values;
}

// --- Web playground mirror adapter -----------------------------------------

/** CSS custom property names mapped to typography artifact keys. */
const WEB_TYPOGRAPHY_VARIABLES = {
  'type-caption-size': ['type-caption', 'size'],
  'type-body-size': ['type-body', 'size'],
  'type-body-strong-weight': ['type-body-strong', 'weight'],
  'type-title-size': ['type-title', 'size'],
  'type-maneuver-size': ['type-maneuver', 'size'],
};

/** Web control-module exports mapped to artifact keys. */
const WEB_CONTROL_KEYS = {
  MAP_CONTROL_ZOOM_STEP: 'behavior/map-control-zoom-step',
  MAP_CONTROL_ZOOM_REPEAT_INTERVAL_MILLISECONDS: 'behavior/map-control-zoom-repeat-interval',
  COMPASS_VISIBILITY_BEARING_THRESHOLD_DEGREES: 'behavior/compass-visibility-bearing-threshold',
  EASED_CAMERA_MOVE_DURATION_MILLISECONDS: 'behavior/compass-renorth-animation',
  LOCATION_ANIMATION_DURATION_MILLISECONDS: 'behavior/location-animation-duration-milliseconds',
  LOCATION_ANIMATION_MINIMUM_MILLISECONDS: 'behavior/location-animation-minimum-milliseconds',
  NAVIGATION_FOLLOW_ZOOM: 'behavior/navigation-follow-zoom',
  NAVIGATION_FOLLOW_ZOOM_MAXIMUM: 'behavior/navigation-follow-zoom-maximum',
  NAVIGATION_FOLLOW_PITCH_3D_DEGREES: 'behavior/navigation-follow-pitch-3d',
};

/** The §2.2 canvas-mirror constant names the web page must declare. */
const WEB_CANVAS_MIRROR_CONSTANT_NAMES = [
  'SCENE_BACKGROUND_BY_THEME',
  'TILE_FILL_BY_THEME',
  'TILE_STROKE_BY_THEME',
  'BACKGROUND_GRID_BY_THEME',
];

/**
 * Reads the web page's inline token block and its §2.2 canvas constants, plus
 * the shared control-cluster module.
 * @param {(relativePath: string) => string} read
 * @returns {Map<string, any>}
 */
function readWebMirror(read) {
  const values = new Map();
  const pageText = read(MIRROR_FILES.webPage);

  const rootBlock = extractBracedBlockAfter(pageText, ':root {');
  const lightBlock = extractBracedBlockAfter(pageText, ":root[data-theme='light'] {");
  const rootVariables = parseCssVariables(rootBlock);
  const lightVariables = parseCssVariables(lightBlock);

  for (const [variable, value] of rootVariables) {
    applyWebVariable(variable, value, 'dark', values);
  }
  for (const [variable, value] of lightVariables) {
    applyWebVariable(variable, value, 'light', values);
  }

  // The camera framing constant lives with the canvas controller module.
  // The controller is split by concern, so a constant may be declared at the
  // module scope or on the shared runtime object.
  const canvasText = read(MIRROR_FILES.webCanvas);
  const constantDeclarationPattern = (constant) =>
    `(?:const |runtime\\.shared\\.)${constant}\\s*=`;
  const fieldOfView = new RegExp(`${constantDeclarationPattern('FIELD_OF_VIEW_DEGREES')}\\s*([\\d.]+);`)
    .exec(canvasText);
  if (fieldOfView !== null) {
    values.set('behavior/field-of-view-degrees', parseNumber(fieldOfView[1]));
  }
  const autocompleteBehaviorKeys = {
    AUTOCOMPLETE_MINIMUM_QUERY_CHARACTERS: 'behavior/autocomplete-minimum-query-characters',
    AUTOCOMPLETE_DEBOUNCE_MILLISECONDS: 'behavior/autocomplete-debounce-milliseconds',
    AUTOCOMPLETE_MAXIMUM_RESULTS: 'behavior/autocomplete-maximum-results',
  };
  for (const [constant, token] of Object.entries(autocompleteBehaviorKeys)) {
    const match = new RegExp(`${constantDeclarationPattern(constant)}\\s*([\\d.]+);`)
      .exec(canvasText);
    if (match !== null) {
      values.set(token, parseNumber(match[1]));
    }
  }

  const controlText = read(MIRROR_FILES.webMapControls);
  const controlValues = new Map();
  for (const match of controlText.matchAll(/export const (\w+)\s*=\s*([\d.]+);/g)) {
    controlValues.set(match[1], parseNumber(match[2]));
  }
  for (const [constant, key] of Object.entries(WEB_CONTROL_KEYS)) {
    if (controlValues.has(constant)) {
      values.set(key, controlValues.get(constant));
    }
  }
  for (const [constant, key] of Object.entries(WEB_DIAGNOSTICS_NUMBER_KEYS)) {
    if (controlValues.has(constant)) {
      values.set(key, controlValues.get(constant));
    }
  }
  for (const [constant, key] of Object.entries(WEB_DIAGNOSTICS_STRING_KEYS)) {
    const match = new RegExp(`export const ${constant}\\s*=\\s*'([^']*)';`).exec(controlText);
    if (match !== null) {
      values.set(key, match[1]);
    }
  }
  const locationDeadReckoning =
    /export const LOCATION_ANIMATION_DEAD_RECKONING\s*=\s*'([^']*)';/.exec(controlText);
  if (locationDeadReckoning !== null) {
    values.set('behavior/location-animation-dead-reckoning', locationDeadReckoning[1]);
  }
  if (controlValues.has('SCALE_BAR_WIDTH_MINIMUM_PIXELS')
    && controlValues.has('SCALE_BAR_WIDTH_MAXIMUM_PIXELS')) {
    values.set('behavior/scale-bar-width-range', {
      minimum: controlValues.get('SCALE_BAR_WIDTH_MINIMUM_PIXELS'),
      maximum: controlValues.get('SCALE_BAR_WIDTH_MAXIMUM_PIXELS'),
    });
  }
  return values;
}

/**
 * Parses `--name: value;` declarations of a CSS block.
 * @param {string} block
 * @returns {Map<string, string>}
 */
function parseCssVariables(block) {
  const variables = new Map();
  for (const match of block.matchAll(/--([\w-]+)\s*:\s*([^;]+);/g)) {
    variables.set(match[1], match[2].trim());
  }
  return variables;
}

/**
 * Maps one CSS custom property onto the artifact key it mirrors, when it
 * mirrors one at all (component-local properties are ignored).
 * @param {string} variable
 * @param {string} value
 * @param {'dark' | 'light'} themeName
 * @param {Map<string, any>} values
 */
function applyWebVariable(variable, value, themeName, values) {
  if (variable.endsWith('-color')) {
    values.set(
      `${variable.slice(0, -'-color'.length)}/${themeName}`,
      parseCssColor(value),
    );
    return;
  }
  const typography = WEB_TYPOGRAPHY_VARIABLES[variable];
  if (typography !== undefined) {
    values.set(`${typography[0]}/${typography[1]}`, parseNumber(value));
    return;
  }
  const spacingTokens = ['gutter-inline', 'gutter-block', 'panel-padding', 'stack-tight', 'stack-group', 'map-control-diameter'];
  if (spacingTokens.includes(variable)) {
    values.set(`spacing/${variable}`, parseNumber(value));
    return;
  }
  const radiusTokens = ['radius-surface', 'radius-control', 'radius-inline'];
  if (radiusTokens.includes(variable)) {
    values.set(`radius/${variable}`, parseNumber(value));
    return;
  }
  // §2.4 names one elevation value; the light-theme override in the page is a
  // platform tuning of that one row, so the base `:root` declaration is the
  // value the artifact owns.
  if (variable === 'elevation-surface' && themeName === 'dark') {
    values.set('behavior/elevation-surface', parseShadow(value));
  }
}

/**
 * Asserts the §2.2 canvas-mirror block: the named constants exist, and the
 * tile fill/stroke constants' RGB channels derive from the accent token in
 * both themes (the scene background and center mark are platform-derived
 * paints, so only their presence is asserted).
 * @param {(relativePath: string) => string} read
 * @param {Map<string, ArtifactExpectation>} expectations
 * @returns {string[]}
 */
function checkWebCanvasMirrors(read, expectations) {
  const failures = [];
  const pageText = read(MIRROR_FILES.webCanvas);
  // A mirror constant may be declared at the module scope or on the shared
  // runtime object, depending on the concern module that owns it.
  const constantDeclarationPattern = (constant) =>
    `(?:const |runtime\\.shared\\.)${constant}\\s*=`;
  for (const constantName of WEB_CANVAS_MIRROR_CONSTANT_NAMES) {
    if (!new RegExp(constantDeclarationPattern(constantName)).test(pageText)) {
      failures.push(`[web canvas] the §2.2 mirror constant "${constantName}" is missing`);
    }
  }
  const accentDark = expectations.get('accent/dark')?.value;
  const accentLight = expectations.get('accent/light')?.value;
  if (accentDark === undefined || accentLight === undefined) {
    failures.push('[web canvas] the artifact must declare the accent color in both themes');
    return failures;
  }
  for (const constantName of ['TILE_FILL_BY_THEME', 'TILE_STROKE_BY_THEME']) {
    const blockMatch = new RegExp(`${constantDeclarationPattern(constantName)} \\{([\\s\\S]*?)\\};`)
      .exec(pageText);
    if (blockMatch === null) {
      failures.push(`[web canvas] could not parse the "${constantName}" theme map`);
      continue;
    }
    const entries = [...blockMatch[1].matchAll(/(dark|light):\s*'([^']+)'/g)];
    if (entries.length !== 2) {
      failures.push(`[web canvas] "${constantName}" must carry one value per theme`);
      continue;
    }
    for (const match of entries) {
      const expected = match[1] === 'dark' ? accentDark : accentLight;
      const actual = parseCssColor(match[2]);
      if (Math.abs(actual.red - expected.red) > VALUE_TOLERANCE
        || Math.abs(actual.green - expected.green) > VALUE_TOLERANCE
        || Math.abs(actual.blue - expected.blue) > VALUE_TOLERANCE) {
        failures.push(
          `[web canvas] ${constantName}.${match[1]} must derive its RGB from the accent token`,
        );
      }
    }
  }
  return failures;
}

// --- Platform coverage ------------------------------------------------------

/**
 * Artifact keys a platform genuinely does not mirror. These are reported,
 * never silently passed: web keeps the v1 panel interface (no home-sheet
 * rows) and carries no font weights other than `type-body-strong`; RN does
 * not re-word the SDK error details.
 */
const PLATFORM_OMISSIONS = {
  android: new Set([
    'behavior/elevation-surface',
  ]),
  ios: new Set(),
  rn: new Set([
    'string/rate_limit_exhausted_detail',
    'string/provider_not_configured_detail_template',
  ]),
  web: new Set([
    'type-caption/weight',
    'type-body/weight',
    'type-body-strong/size',
    'type-title/weight',
    'type-maneuver/weight',
    'type-maneuver-distance/size',
    'type-maneuver-distance/weight',
    'behavior/sheet-half-height',
    'behavior/sheet-half-phone-height',
    'behavior/sheet-peek-content-rows',
    'string/rate_limit_exhausted_detail',
    'string/provider_not_configured_detail_template',
  ]),
};

/** Why each omission exists, shown in the summary. */
const OMISSION_NOTES = {
  android: 'the Compose theme has no elevation constant (Material provides it)',
  ios: 'the SwiftUI mirror carries every §2 value',
  rn: 'React Native does not re-word the SDK error details',
  web: 'the v1 playground has no home-sheet rows and no weight tokens beyond type-body-strong',
};

/**
 * Artifact keys a platform now reads from the SDK contract instead of
 * mirroring a literal. Each entry names the wrapper source that calls the
 * generated getter, and the gate fails when that call is missing, so an
 * adoption is verified rather than assumed. A platform may keep a literal
 * beside the adoption; the literal is then compared as before.
 */
const PLATFORM_CONTRACT_ADOPTIONS = {
  android: {
    'behavior/field-of-view-degrees': {
      source: 'platform/android/maps-sdk/src/main/kotlin/com/rusticmaps/sdk/MapView.kt',
      call: 'defaultFieldOfViewDegrees()',
    },
  },
  ios: {
    'behavior/field-of-view-degrees': {
      source: 'platform/ios/Sources/MapNativeCoreUI/MapView.swift',
      call: 'defaultFieldOfViewDegrees()',
    },
  },
  rn: {
    'behavior/field-of-view-degrees': {
      source: 'platform/rn/ios/MapsSdkRusticMapView.swift',
      call: 'defaultFieldOfViewDegrees()',
    },
  },
  web: {
    'behavior/field-of-view-degrees': {
      source: 'platform/web/playground_map_surface.mjs',
      call: 'defaultFieldOfViewDegrees()',
    },
  },
};

// --- The parity run ---------------------------------------------------------

/**
 * @typedef {object} ParityReport
 * @property {string[]} failures Blocking drifts (tokens, error sentences, the §2.5 document).
 * @property {{platform: string, mirrored: number, adopted: number, adoptedKeys: string[], total: number, omitted: string[], note: string}[]} summary
 * @property {string[]} labelDivergences Consumer-copy drifts, report-only in M28 slice 1.
 * @property {{platform: string, matched: number, total: number, omitted: string[], note: string}[]} labelSummary
 */

/**
 * Runs every mirror comparison against a repository root. Returns all
 * failures (never throws on drift) so the summary can be complete.
 *
 * Consumer-copy drifts are collected separately from `failures`: M28 slice 1
 * lands the gate report-only, so they print but do not fail the run. Slice 6
 * folds them into the exit condition once every platform has migrated.
 * @param {string} rootDirectory
 * @returns {ParityReport}
 */
function runChecks(rootDirectory) {
  const read = (relativePath) => {
    const relativePaths = Array.isArray(relativePath) ? relativePath : [relativePath];
    return relativePaths
      .map((one_path) => readFileSync(path.join(rootDirectory, one_path), 'utf8'))
      .join('\n');
  };
  const failures = [];
  const summary = [];
  const labelDivergences = [];
  const labelSummary = [];

  let artifact;
  try {
    artifact = JSON.parse(read(ARTIFACT_PATH));
  } catch (error) {
    return { failures: [`[artifact] ${error.message}`], summary, labelDivergences, labelSummary };
  }

  let expectations;
  try {
    expectations = buildArtifactExpectations(artifact);
  } catch (error) {
    return { failures: [`[artifact] ${error.message}`], summary, labelDivergences, labelSummary };
  }

  let uiLabels;
  try {
    uiLabels = buildUiLabelExpectations(artifact);
  } catch (error) {
    failures.push(`[artifact] ${error.message}`);
    uiLabels = new Map();
  }

  const adapters = [
    ['android', readAndroidMirror],
    ['ios', readIosMirror],
    ['rn', readRnMirror],
    ['web', readWebMirror],
  ];
  for (const [platform, adapter] of adapters) {
    let mirrored;
    try {
      mirrored = adapter(read);
    } catch (error) {
      failures.push(`[${platform}] could not read the mirror: ${error.message}`);
      continue;
    }
    const adoptions = PLATFORM_CONTRACT_ADOPTIONS[platform] ?? {};
    const adoptedKeys = Object.keys(adoptions)
      .filter((key) => expectations.has(key));
    const omitted = [...new Set([
      ...(PLATFORM_OMISSIONS[platform] ?? []),
    ])].filter((key) => expectations.has(key) && !adoptedKeys.includes(key));
    const requiredKeys = [...expectations.keys()]
      .filter((key) => !omitted.includes(key));
    let matchedCount = 0;
    let adoptedCount = 0;
    for (const key of requiredKeys) {
      if (!mirrored.has(key)) {
        const adoption = adoptions[key];
        if (adoption !== undefined) {
          const adoptionSource = read(adoption.source);
          if (adoptionSource.includes(adoption.call)) {
            adoptedCount += 1;
            continue;
          }
          failures.push(
            `[${platform}] "${key}" is contract-adopted but `
            + `${adoption.source} has no \`${adoption.call}\``,
          );
          continue;
        }
        failures.push(`[${platform}] the mirror has no value for "${key}"`);
        continue;
      }
      if (!mirroredValueMatches(expectations.get(key), mirrored.get(key))) {
        failures.push(`[${platform}] "${key}" drifted from the artifact value`);
        continue;
      }
      matchedCount += 1;
    }
    summary.push({
      platform,
      mirrored: matchedCount,
      adopted: adoptedCount,
      adoptedKeys,
      total: expectations.size,
      omitted,
      note: OMISSION_NOTES[platform] ?? '',
    });
  }

  failures.push(...checkWebCanvasMirrors(read, expectations));

  if (uiLabels.size > 0) {
    const stringAdapters = [
      ['android', readAndroidStringMirror],
      ['ios', readIosStringMirror],
      ['rn', readReactNativeStringMirror],
      ['web', readWebStringMirror],
    ];
    for (const [platform, adapter] of stringAdapters) {
      let mirrored;
      try {
        mirrored = adapter(read);
      } catch (error) {
        labelDivergences.push(
          `[${platform}] could not read the consumer-copy mirror: ${error.message}`,
        );
        continue;
      }
      const omitted = [...new Set([
        ...(LABEL_PLATFORM_OMISSIONS[platform] ?? []),
      ])].filter((id) => uiLabels.has(id));
      const requiredIds = [...uiLabels.keys()]
        .filter((id) => !omitted.includes(id));
      let matchedCount = 0;
      for (const id of requiredIds) {
        const expectation = uiLabels.get(id);
        const expectedText = normalizeInterpolation(effectiveUiLabelText(expectation, platform));
        if (!mirrored.has(id)) {
          labelDivergences.push(`[${platform}] the mirror has no consumer string for "${id}"`);
          continue;
        }
        const actualEntry = mirrored.get(id);
        if (normalizeInterpolation(actualEntry.value) !== expectedText) {
          labelDivergences.push(
            `[${platform}] "${id}" reads "${actualEntry.value}" instead of `
            + `"${effectiveUiLabelText(expectation, platform)}" (${actualEntry.source})`,
          );
          continue;
        }
        matchedCount += 1;
      }
      labelSummary.push({
        platform,
        matched: matchedCount,
        total: uiLabels.size,
        omitted,
        note: LABEL_OMISSION_NOTES[platform] ?? '',
      });
    }
    failures.push(...checkConsumerCopyTable(read, uiLabels));
  }

  return { failures, summary, labelDivergences, labelSummary };
}

/**
 * Prints the human-readable parity report.
 * @param {ParityReport} report
 */
function printReport(report) {
  console.log('UX parity: platform/reference-tokens.json ↔ the platform mirrors\n');
  for (const entry of report.summary) {
    const omitted = entry.omitted.length === 0
      ? 'none'
      : `${entry.omitted.length} (${entry.omitted.join(', ')})`;
    const adopted = entry.adopted === 0
      ? ''
      : ` · adopted: ${entry.adopted} (${entry.adoptedKeys.join(', ')})`;
    console.log(
      `  ${entry.platform.padEnd(8)} ${String(entry.mirrored).padStart(2)}/${entry.total}`
      + ` mirrored${adopted} · omitted: ${omitted}`,
    );
    if (entry.omitted.length > 0) {
      console.log(`           note: ${entry.note}`);
    }
  }
  console.log('');
  if (report.failures.length === 0) {
    console.log('PASS: every mirrored token value and error string matches the artifact.');
  } else {
    console.log(`FAIL: ${report.failures.length} drift(s) from the artifact:`);
    for (const failure of report.failures) {
      console.log(`  - ${failure}`);
    }
  }
}

/**
 * Prints the consumer-copy report (§2.5). Report-only in M28 slice 1: the
 * divergence count is the baseline the migration slices drive to zero.
 * @param {ParityReport} report
 */
function printLabelReport(report) {
  console.log('\nConsumer copy: ui_labels ↔ the platform mirrors (blocking since M28 slice 6)\n');
  for (const entry of report.labelSummary) {
    const omitted = entry.omitted.length === 0
      ? 'none'
      : `${entry.omitted.length} (${entry.omitted.join(', ')})`;
    console.log(
      `  ${entry.platform.padEnd(8)} ${String(entry.matched).padStart(2)}/${entry.total}`
      + ` canonical · omitted: ${omitted}`,
    );
    if (entry.omitted.length > 0) {
      console.log(`           note: ${entry.note}`);
    }
    if (entry.platform === 'web') {
      console.log(
        `           v1 developer copy excluded (${WEB_V1_DEVELOPER_COPY.reason}): `
        + WEB_V1_DEVELOPER_COPY.labels.join(', '),
      );
    }
  }
  console.log('');
  if (report.labelDivergences.length === 0) {
    console.log('PASS: every extracted consumer string matches its canonical value.');
  } else {
    console.log(
      `BASELINE (non-blocking): ${report.labelDivergences.length} consumer-copy divergence(s):`,
    );
    for (const divergence of report.labelDivergences) {
      console.log(`  - ${divergence}`);
    }
  }
}

// --- Self-test --------------------------------------------------------------

/** Flattens a mirror-file value that may be a path or a list of paths. */
function sourcePaths(source) {
  return Array.isArray(source) ? source : [source];
}

/**
 * Applies one textual mutation to the first file in `source` that contains
 * `from`. Returns false when no file contains it.
 * @param {string} rootDirectory
 * @param {string|string[]} source
 * @param {string} from
 * @param {string} to
 * @returns {boolean}
 */
function mutateFirstSource(rootDirectory, source, from, to) {
  for (const relativePath of sourcePaths(source)) {
    const filePath = path.join(rootDirectory, relativePath);
    const text = readFileSync(filePath, 'utf8');
    if (text.includes(from)) {
      writeFileSync(filePath, text.replace(from, to));
      return true;
    }
  }
  return false;
}

/**
 * Copies the artifact, the §2.5 document and every mirror file into a
 * throwaway tree so a mutation can be injected without touching the checkout.
 * @param {string} rootDirectory
 * @returns {string}
 */
function createTemporaryCopy(rootDirectory) {
  const temporaryRoot = mkdtempSync(path.join(tmpdir(), 'ux-parity-'));
  const relativePaths = new Set([
    ARTIFACT_PATH,
    CONSUMER_COPY_DOCUMENT,
    ...Object.values(MIRROR_FILES).flatMap(sourcePaths),
    ...Object.values(STRING_MIRROR_FILES).flatMap(sourcePaths),
    ...Object.values(PLATFORM_CONTRACT_ADOPTIONS)
      .flatMap((adoptions) => Object.values(adoptions))
      .map((adoption) => adoption.source),
  ]);
  for (const relativePath of relativePaths) {
    const destination = path.join(temporaryRoot, relativePath);
    mkdirSync(path.dirname(destination), { recursive: true });
    copyFileSync(path.join(rootDirectory, relativePath), destination);
  }
  return temporaryRoot;
}

/**
 * Runs the self-test: the unmutated copy must pass, then each injected drift
 * class must fail. Throws if the gate ever passes vacuously.
 */
function runSelfTest() {
  const rootDirectory = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
  const baseCopy = createTemporaryCopy(rootDirectory);
  const cases = [];

  try {
    const baseline = runChecks(baseCopy);
    cases.push({
      name: 'unmutated copy',
      expectation: 'pass',
      report: baseline,
      ok: baseline.failures.length === 0,
    });

    // Class 1: the artifact value drifts (the source itself changes).
    const artifactCopy = createTemporaryCopy(rootDirectory);
    try {
      const artifactPath = path.join(artifactCopy, ARTIFACT_PATH);
      const artifact = JSON.parse(readFileSync(artifactPath, 'utf8'));
      const accent = artifact.color_roles.find((row) => row.token === 'accent');
      accent.dark = '#4c8d00';
      writeFileSync(artifactPath, `${JSON.stringify(artifact, null, 2)}\n`);
      const report = runChecks(artifactCopy);
      cases.push({
        name: 'artifact drift (accent.dark)',
        expectation: 'fail',
        report,
        ok: report.failures.some((failure) => failure.includes('accent/dark')),
      });
    } finally {
      rmSync(artifactCopy, { recursive: true, force: true });
    }

    // Class 2: a mirror value drifts (the source stays correct).
    const mirrorCopy = createTemporaryCopy(rootDirectory);
    try {
      const themePath = path.join(mirrorCopy, MIRROR_FILES.rnTheme);
      const themeText = readFileSync(themePath, 'utf8')
        .replace("textPrimary: '#f2f4f8'", "textPrimary: '#000000'");
      writeFileSync(themePath, themeText);
      const report = runChecks(mirrorCopy);
      cases.push({
        name: 'mirror drift (RN textPrimary)',
        expectation: 'fail',
        report,
        ok: report.failures.some((failure) => failure.includes('text-primary/dark')),
      });
    } finally {
      rmSync(mirrorCopy, { recursive: true, force: true });
    }

    // Class 2b: a mirrored error sentence drifts.
    const stringCopy = createTemporaryCopy(rootDirectory);
    try {
      const statePath = path.join(stringCopy, MIRROR_FILES.androidState);
      const stateText = readFileSync(statePath, 'utf8')
        .replace(
          '"rate limit exhausted; retry after the next refill"',
          '"rate limit exhausted; retry after the next refill now"',
        );
      writeFileSync(statePath, stateText);
      const report = runChecks(stringCopy);
      cases.push({
        name: 'mirror drift (Android rate-limit sentence)',
        expectation: 'fail',
        report,
        ok: report.failures.some((failure) => failure.includes('rate_limit_exhausted_detail')),
      });
    } finally {
      rmSync(stringCopy, { recursive: true, force: true });
    }
    // Class 2c: a §2.2 canvas-mirror constant drifts from the accent token.
    const canvasCopy = createTemporaryCopy(rootDirectory);
    try {
      mutateFirstSource(
        canvasCopy, MIRROR_FILES.webCanvas,
        "'rgba(76, 141, 255, 0.70)'", "'rgba(0, 0, 0, 0.70)'",
      );
      const report = runChecks(canvasCopy);
      cases.push({
        name: 'canvas mirror drift (TILE_STROKE_BY_THEME)',
        expectation: 'fail',
        report,
        ok: report.failures.some((failure) => failure.includes('TILE_STROKE_BY_THEME')),
      });
    } finally {
      rmSync(canvasCopy, { recursive: true, force: true });
    }

    // Class 2d: a contract adoption loses its generated-getter call.
    const adoptionCopy = createTemporaryCopy(rootDirectory);
    try {
      mutateFirstSource(
        adoptionCopy,
        PLATFORM_CONTRACT_ADOPTIONS.android['behavior/field-of-view-degrees'].source,
        'defaultFieldOfViewDegrees()',
        'defaultFieldOfViewDegreesRemoved()',
      );
      const report = runChecks(adoptionCopy);
      cases.push({
        name: 'contract adoption drift (Android field of view)',
        expectation: 'fail',
        report,
        ok: report.failures.some((failure) => failure.includes('field-of-view-degrees')),
      });
    } finally {
      rmSync(adoptionCopy, { recursive: true, force: true });
    }

    // Class 3: a canonical consumer-copy value drifts in the artifact.
    const labelArtifactCopy = createTemporaryCopy(rootDirectory);
    try {
      const artifactPath = path.join(labelArtifactCopy, ARTIFACT_PATH);
      const artifact = JSON.parse(readFileSync(artifactPath, 'utf8'));
      const peekAction = artifact.ui_labels.find((row) => row.id === 'peek.primary_action');
      peekAction.text = 'Plan a different route';
      writeFileSync(artifactPath, `${JSON.stringify(artifact, null, 2)}\n`);
      const report = runChecks(labelArtifactCopy);
      cases.push({
        name: 'consumer-copy artifact drift (peek.primary_action)',
        expectation: 'report',
        report,
        ok: report.labelDivergences.some((divergence) => divergence.includes('peek.primary_action')),
      });
    } finally {
      rmSync(labelArtifactCopy, { recursive: true, force: true });
    }

    // Class 4: a platform consumer string drifts from the canonical value.
    const labelMirrorCopy = createTemporaryCopy(rootDirectory);
    try {
      const themePath = path.join(labelMirrorCopy, STRING_MIRROR_FILES.reactNativeTheme);
      const themeText = readFileSync(themePath, 'utf8')
        .replace("whereToText: 'Where to?'", "whereToText: 'Where?'");
      writeFileSync(themePath, themeText);
      const report = runChecks(labelMirrorCopy);
      cases.push({
        name: 'consumer-copy mirror drift (RN whereToText)',
        expectation: 'report',
        report,
        ok: report.labelDivergences.some((divergence) => divergence.includes('plan.destination_hint')),
      });
    } finally {
      rmSync(labelMirrorCopy, { recursive: true, force: true });
    }

    // Class 5: React Native renders the canonical off-route word (the
    // recorded override retired with the 2026-10-31 wording-parity ruling).
    cases.push({
      name: 'canonical off-route word on React Native',
      expectation: 'pass',
      report: baseline,
      ok: !baseline.labelDivergences.some(
        (divergence) => divergence.includes('[rn]') && divergence.includes('navigation.phase_off_route'),
      ),
    });

    // Class 6: the §2.5 consumer-copy table drifts from the artifact.
    const documentCopy = createTemporaryCopy(rootDirectory);
    try {
      const documentPath = path.join(documentCopy, CONSUMER_COPY_DOCUMENT);
      const documentText = readFileSync(documentPath, 'utf8')
        .replace('| `route.edit_action` | Edit | — |', '| `route.edit_action` | Change | — |');
      writeFileSync(documentPath, documentText);
      const report = runChecks(documentCopy);
      cases.push({
        name: 'consumer-copy document drift (route.edit_action)',
        expectation: 'fail',
        report,
        ok: report.failures.some((failure) => failure.includes('route.edit_action')),
      });
    } finally {
      rmSync(documentCopy, { recursive: true, force: true });
    }

    // Class 7: the page markup drifts from the canonical value. Reading only
    // ui_strings.mjs was the slice-1 blind spot; the page is what renders.
    const pageMarkupCopy = createTemporaryCopy(rootDirectory);
    try {
      const pagePath = path.join(pageMarkupCopy, STRING_MIRROR_FILES.webPageMarkup);
      // The element carries the same label twice (placeholder and
      // accessible name); the accessible name is the value the adapter keeps,
      // so it is the one the drift class mutates.
      const pageText = readFileSync(pagePath, 'utf8')
        .replace('aria-label="Search a destination"', 'aria-label="Destination"');
      writeFileSync(pagePath, pageText);
      const report = runChecks(pageMarkupCopy);
      cases.push({
        name: 'web page markup drift (destination placeholder)',
        expectation: 'report',
        report,
        ok: report.labelDivergences.some(
          (divergence) => divergence.includes('[web]')
            && divergence.includes('plan.destination_placeholder'),
        ),
      });
    } finally {
      rmSync(pageMarkupCopy, { recursive: true, force: true });
    }

    // Class 8: a dynamic label stops reading the shared module.
    const pageControllerCopy = createTemporaryCopy(rootDirectory);
    try {
      mutateFirstSource(
        pageControllerCopy, STRING_MIRROR_FILES.webPageController,
        "ui_label_text('settings.keyless_status')",
        "'Set a provider key to load live map data'",
      );
      const report = runChecks(pageControllerCopy);
      cases.push({
        name: 'web dynamic label unwired (keyless status)',
        expectation: 'report',
        report,
        ok: report.labelDivergences.some(
          (divergence) => divergence.includes('[web]')
            && divergence.includes('settings.keyless_status'),
        ),
      });
    } finally {
      rmSync(pageControllerCopy, { recursive: true, force: true });
    }
  } finally {
    rmSync(baseCopy, { recursive: true, force: true });
  }

  console.log('UX parity self-test: mutates a temporary copy, never the checkout.\n');
  for (const testCase of cases) {
    console.log(`  ${testCase.ok ? 'ok  ' : 'FAIL'} ${testCase.name} → expected ${testCase.expectation}`);
    if (!testCase.ok) {
      const details = [...testCase.report.failures, ...testCase.report.labelDivergences];
      console.log(`       details: ${details.join(' | ')}`);
    }
  }
  console.log('');
  const failed = cases.filter((testCase) => !testCase.ok);
  if (failed.length > 0) {
    console.log(`FAIL: ${failed.length} self-test case(s) did not behave as required.`);
    process.exitCode = 1;
    return;
  }
  console.log(`PASS: ${cases.length}/${cases.length} self-test cases behaved as required.`);
}

// --- Entry point ------------------------------------------------------------

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
if (process.argv.includes('--self-test')) {
  runSelfTest();
} else {
  const report = runChecks(repositoryRoot);
  printReport(report);
  printLabelReport(report);
  // The published wrapper sources' documentation comments are a product
  // surface too: a leaked milestone, round, internal review term or repository
  // path is a blocking failure of the same standing gate.
  const publicCommentFindings = scrubPublicCommentSources(repositoryRoot);
  if (publicCommentFindings.length === 0) {
    console.log('Public-comment scrub: PASS\n');
  } else {
    console.log('Public-comment scrub:');
    for (const finding of publicCommentFindings) {
      console.log(
        `  FAIL ${finding.filePath}:${finding.lineNumber} (${finding.description}): ${finding.text}`,
      );
    }
    console.log('');
  }
  // M28 slice 6: the consumer-copy gate is blocking. Every platform has
  // migrated, so a label that drifts from `ui_labels` fails the run exactly
  // like a token or error-sentence drift. Omissions stay printed, never
  // silently passed — they are roles a surface genuinely does not have.
  if (
    report.failures.length > 0 ||
    report.labelDivergences.length > 0 ||
    publicCommentFindings.length > 0
  ) {
    process.exitCode = 1;
  }
}
