#!/usr/bin/env node
// Generates the five platform API reference pages from the SDK's single
// sources: the UniFFI contract, the generated Kotlin/Swift/TypeScript
// bindings, the WebAssembly export sources and the Rust crate sources.
//
// The pages are build artifacts, not authored chapters. `site/book-src/`
// keeps only the API reference landing page; `scripts/build-docs-site.sh`
// runs this generator while composing the mdBook source tree. The generated
// pages carry a header that says so, and `--check` is the drift gate a CI
// job can run against a directory of generated pages.
//
// USAGE
//   node scripts/generate-api-reference.mjs
//   node scripts/generate-api-reference.mjs --output-dir <directory>
//   node scripts/generate-api-reference.mjs --check <directory>
//
// The default output directory is `target/api-reference-staging`.
//
// DETERMINISM
//   Types keep the contract's declaration order. Files, modules and wasm
//   exports that have no contract order use a fixed file order and their
//   source declaration order. The output carries no timestamps, no host
//   paths and no absolute paths.

import {
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { parseKotlinBindings } from './check-doc-snippets.mjs';

const REPOSITORY_ROOT = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);
const DEFAULT_OUTPUT_DIRECTORY = path.join(
  REPOSITORY_ROOT,
  'target',
  'api-reference-staging',
);

const UDL_RELATIVE_PATH = 'crates/rustic-maps-native-core/src/rustic_maps.udl';
const KOTLIN_BINDINGS_RELATIVE_PATH =
  'platform/android/maps-sdk-native/src/main/kotlin/uniffi/rustic_maps/rustic_maps.kt';
const SWIFT_BINDINGS_RELATIVE_PATH =
  'platform/ios/Sources/RusticMapsSDK/rustic_maps.swift';
const REACT_NATIVE_BINDINGS_RELATIVE_PATH =
  'platform/rn/src/generated/rustic_maps.ts';
const WASM_PACKAGE_DECLARATIONS_RELATIVE_PATH =
  'platform/web/generated/rustic_maps.d.ts';
const WASM_CONTRACT_MIRROR_RELATIVE_PATH =
  'platform/web/generated/rustic_maps.d.ts';
const WASM_BINDINGS_SOURCE_DIRECTORY = 'crates/rustic-maps-web-bindings/src';

/** The five pages this generator writes, in platform order. */
export const OUTPUT_FILE_NAMES = [
  'api-reference-rust.md',
  'api-reference-android.md',
  'api-reference-ios.md',
  'api-reference-react-native.md',
  'api-reference-wasm.md',
];

/** The Rust crates whose public items the Rust page indexes. */
const RUST_CRATE_DIRECTORIES = [
  'geospatial-types',
  'geospatial-traits',
  'rustic-maps-core',
  'rustic-maps-rendering',
  'rustic-maps-navigation',
];

/**
 * Every single-file input the generated pages derive from, relative to the
 * repository root. `scripts/check-generated-api-reference.mjs` fingerprints
 * these files so each page records the source surface revision it renders.
 */
export const API_REFERENCE_SOURCE_FILE_PATHS = [
  UDL_RELATIVE_PATH,
  KOTLIN_BINDINGS_RELATIVE_PATH,
  SWIFT_BINDINGS_RELATIVE_PATH,
  REACT_NATIVE_BINDINGS_RELATIVE_PATH,
  WASM_PACKAGE_DECLARATIONS_RELATIVE_PATH,
  WASM_CONTRACT_MIRROR_RELATIVE_PATH,
];

/**
 * Every directory whose Rust sources the generated pages index, relative to
 * the repository root. The fingerprint walks their `.rs` files recursively.
 */
export const API_REFERENCE_SOURCE_DIRECTORY_PATHS = [
  WASM_BINDINGS_SOURCE_DIRECTORY,
  ...RUST_CRATE_DIRECTORIES.map((crateDirectory) => `crates/${crateDirectory}/src`),
];

/** The wasm classes in the order the platform page presents them. */
const WASM_CLASS_ORDER = [
  'MapsSdk',
  'MapView',
  'MapCamera',
  'Navigator',
  'RouteSimulator',
];

/** The generated contract classes, in the order the wasm page presents them. */
const WASM_CONTRACT_CLASS_ORDER = [
  'MapSdk',
  'Navigator',
  'RouteSimulator',
  'PositionController',
];

/** Human section names for the wasm export source files. */
const WASM_FUNCTION_SECTION_NAMES = new Map([
  ['live_providers.rs', 'Provider registration and live calls'],
  ['exports.rs', 'Routes, geometry and version'],
  ['diagnostics.rs', 'Diagnostics'],
  ['vector_layer.rs', 'Vector tile helpers'],
  ['camera_interaction.rs', 'Camera interaction aliases'],
  ['canvas.rs', 'Device and base-map helpers'],
]);

/** The wasm export source file order (deterministic grouping). */
const WASM_SOURCE_FILE_ORDER = [
  'exports.rs',
  'live_providers.rs',
  'canvas.rs',
  'vector_layer.rs',
  'refapp.rs',
  'refapp_math.rs',
  'web_camera.rs',
];

/** Contract interfaces the WebAssembly surface exposes. */
export const WASM_EXPOSED_UDL_INTERFACES = [
  'MapCamera',
  'Navigator',
  'RouteSimulator',
  'PositionController',
];

/** Platforms and their naming and prose configuration. */
const PLATFORM_CONFIGURATIONS = {
  android: {
    title: 'Android API reference',
    language: 'kotlin',
    languageLabel: 'Kotlin',
    bindingsLabel: 'Android',
    errorTypeName: 'SdkException',
    memberCase: 'camelCase',
    enumCase: 'UPPER_SNAKE_CASE',
    recordCase: 'camelCase',
    asyncKeyword: 'suspend',
    intro:
      'The Android artifact exposes the provider-neutral SDK through Kotlin bindings generated from the SDK contract, together with the shipped map view. This page documents every contract type and member the Kotlin surface exposes: what it does, how it is constructed, its parameters, its return value, the failures it raises, and its async and threading rules. The shipped `com.rusticmaps.sdk` map view and its facade are described in the platform guides.',
    nextSteps: [
      ['Your first map', 'first-map.md'],
      ['Search and routing', 'search-and-routing.md'],
      ['Turn-by-turn navigation', 'navigation-simulation.md'],
      ['Offline maps', 'offline-maps.md'],
    ],
  },
  ios: {
    title: 'iOS API reference',
    language: 'swift',
    languageLabel: 'Swift',
    bindingsLabel: 'Swift',
    errorTypeName: 'SdkError',
    memberCase: 'camelCase',
    enumCase: 'lowerCamelCase',
    recordCase: 'camelCase',
    asyncKeyword: 'async',
    intro:
      'The iOS artifact exposes the provider-neutral SDK through Swift bindings generated from the SDK contract, together with the shipped map view. This page documents every contract type and member the Swift surface exposes: what it does, how it is constructed, its parameters, its return value, the failures it raises, and its async and threading rules. The shipped `MapNativeCoreUI` map view is described in the platform guides.',
    nextSteps: [
      ['Your first map', 'first-map.md'],
      ['Search and routing', 'search-and-routing.md'],
      ['Turn-by-turn navigation', 'navigation-simulation.md'],
      ['Offline maps', 'offline-maps.md'],
    ],
  },
  'react-native': {
    title: 'React Native API reference',
    language: 'typescript',
    languageLabel: 'TypeScript',
    bindingsLabel: 'React Native',
    errorTypeName: 'SdkError',
    memberCase: 'camelCase',
    enumCase: 'PascalCase',
    recordCase: 'camelCase',
    asyncKeyword: 'Promise',
    intro:
      'The React Native package exposes the provider-neutral SDK through TypeScript bindings generated from the SDK contract, together with the shipped map component. This page documents every contract type and member the generated TypeScript surface exposes: what it does, how it is constructed, its parameters, its return value, the failures it raises, and its async and threading rules. The package `MapsSdk` handle and `MapView` component are described in the platform guides.',
    nextSteps: [
      ['Your first map', 'first-map.md'],
      ['Search and routing', 'search-and-routing.md'],
      ['Turn-by-turn navigation', 'navigation-simulation.md'],
      ['Offline maps', 'offline-maps.md'],
    ],
  },
};

const PRIMITIVE_TYPE_NAMES = new Map([
  ['string', { rust: 'String', kotlin: 'String', swift: 'String', typescript: 'string' }],
  ['double', { rust: 'f64', kotlin: 'Double', swift: 'Double', typescript: 'number' }],
  ['u64', { rust: 'u64', kotlin: 'ULong', swift: 'UInt64', typescript: 'bigint' }],
  ['u32', { rust: 'u32', kotlin: 'UInt', swift: 'UInt32', typescript: 'number' }],
  ['u8', { rust: 'u8', kotlin: 'UByte', swift: 'UInt8', typescript: 'number' }],
  ['boolean', { rust: 'bool', kotlin: 'Boolean', swift: 'Bool', typescript: 'boolean' }],
]);

// ---------------------------------------------------------------------------
// Small text helpers
// ---------------------------------------------------------------------------

/** Splits a comma-separated list while respecting `<...>` and `(...)`. */
function splitTopLevel(text, separator) {
  const parts = [];
  let depth = 0;
  let current = '';
  for (const character of text) {
    if (character === '<' || character === '(' || character === '[') {
      depth += 1;
    } else if (character === '>' || character === ')' || character === ']') {
      depth -= 1;
    }
    if (character === separator && depth === 0) {
      parts.push(current);
      current = '';
    } else {
      current += character;
    }
  }
  parts.push(current);
  return parts.map((part) => part.trim()).filter(Boolean);
}

/** Converts `snake_case` to `camelCase`. */
function snakeToCamel(name) {
  return name.replace(/_([a-z0-9])/g, (_, character) => character.toUpperCase());
}

/** Converts `PascalCase` to `UPPER_SNAKE_CASE`. */
function pascalToUpperSnake(name) {
  return name
    .replace(/([a-z0-9])([A-Z])/g, '$1_$2')
    .replace(/([A-Z]+)([A-Z][a-z])/g, '$1_$2')
    .toUpperCase();
}

/** Converts `PascalCase` to `lowerCamelCase`. */
function pascalToLowerCamel(name) {
  return name.length === 0 ? name : name[0].toLowerCase() + name.slice(1);
}

/** Collapses whitespace to one line and escapes Markdown table pipes. */
function inlineText(text) {
  return text
    .replace(/\s+/g, ' ')
    .replace(/\|/g, '\\|')
    .trim();
}

/** Renders doc comment lines as Markdown paragraphs. */
function docToMarkdown(docLines) {
  const paragraphs = [];
  let current = [];
  for (const line of docLines) {
    const text = line.replace(/\s+$/u, '');
    if (text === '') {
      if (current.length > 0) {
        paragraphs.push(current.join(' '));
        current = [];
      }
    } else {
      current.push(text.trim());
    }
  }
  if (current.length > 0) {
    paragraphs.push(current.join(' '));
  }
  return paragraphs
    .map((paragraph) => paragraph.replace(/[ \t]+/gu, ' '))
    .join('\n\n');
}

/** Renders doc comment lines as one table-cell-safe line. */
function docToTableCell(docLines) {
  const markdown = docToMarkdown(docLines);
  return markdown === '' ? '—' : inlineText(markdown);
}

/** Renders a Markdown table. */
function renderTable(headers, rows) {
  const headerLine = `| ${headers.join(' | ')} |`;
  const separatorLine = `| ${headers.map(() => '---').join(' | ')} |`;
  const rowLines = rows.map(
    (row) => `| ${row.map((cell) => inlineText(String(cell))).join(' | ')} |`,
  );
  return [headerLine, separatorLine, ...rowLines].join('\n');
}

/** Joins page lines and normalizes blank-line runs outside fenced blocks. */
function joinPageLines(lines) {
  const text = lines.join('\n').replace(/\r\n?/gu, '\n');
  return `${text.replace(/\n{3,}/gu, '\n\n').replace(/\s+$/u, '')}\n`;
}

// ---------------------------------------------------------------------------
// UDL parsing
// ---------------------------------------------------------------------------

/** Parses a `[Attribute, Name=value]` run into a plain object. */
function parseAttributes(attributeText) {
  const inner = attributeText.trim().replace(/^\[/u, '').replace(/\]$/u, '');
  const attributes = {};
  for (const part of splitTopLevel(inner, ',')) {
    const separatorIndex = part.indexOf('=');
    if (separatorIndex === -1) {
      attributes[part.trim()] = true;
    } else {
      attributes[part.slice(0, separatorIndex).trim()] = part
        .slice(separatorIndex + 1)
        .trim();
    }
  }
  return attributes;
}

/** Parses a UDL type expression. */
function parseUdlType(typeText) {
  const text = typeText.trim();
  if (text.endsWith('?')) {
    return { kind: 'optional', inner: parseUdlType(text.slice(0, -1)) };
  }
  if (text.startsWith('sequence<') && text.endsWith('>')) {
    return { kind: 'sequence', inner: parseUdlType(text.slice('sequence<'.length, -1)) };
  }
  if (text === 'bytes') {
    return { kind: 'bytes' };
  }
  if (text === 'void') {
    return { kind: 'void' };
  }
  return { kind: 'named', name: text };
}

/** Parses one `Type name` parameter. */
function parseParameter(parameterText) {
  const match = parameterText.trim().match(/^(.+?)\s+([A-Za-z_]\w*)$/u);
  if (match === null) {
    throw new Error(`could not parse parameter "${parameterText}"`);
  }
  return { name: match[2], typeText: match[1], type: parseUdlType(match[1]) };
}

/** Parses one `ReturnType name(params)` declaration. */
function parseCallable(declarationText) {
  const text = declarationText.trim().replace(/;$/u, '');
  const match = text.match(/^(?:([\s\S]+?)\s+)?([A-Za-z_]\w*)\s*\(([\s\S]*)\)$/u);
  if (match === null) {
    throw new Error(`could not parse declaration "${declarationText}"`);
  }
  const parameters = splitTopLevel(match[3], ',').map(parseParameter);
  return {
    returnTypeText: match[1] ? match[1].trim() : 'void',
    returnType: parseUdlType(match[1] ? match[1].trim() : 'void'),
    name: match[2],
    parameters,
  };
}

/**
 * Collects the body lines of a brace-delimited block. `startIndex` is the
 * line carrying the opening brace. The opening line and the line carrying
 * the closing brace are not part of the body. Returns the body and the next
 * index.
 */
function collectBraceBlock(lines, startIndex) {
  const bodyLines = [];
  let depth = 0;
  let started = false;
  for (let index = startIndex; index < lines.length; index += 1) {
    const line = lines[index];
    const code = stripLineComment(line);
    let opens = 0;
    let closes = 0;
    for (const character of code) {
      if (character === '{') {
        opens += 1;
      } else if (character === '}') {
        closes += 1;
      }
    }
    if (!started) {
      if (opens > 0) {
        started = true;
        depth = opens - closes;
        if (depth === 0) {
          return { bodyLines: [], nextIndex: index + 1 };
        }
      }
      continue;
    }
    depth += opens - closes;
    if (depth === 0) {
      const contentBeforeClosingBrace = line.slice(0, line.indexOf('}')).trim();
      if (contentBeforeClosingBrace !== '') {
        bodyLines.push(contentBeforeClosingBrace);
      }
      return { bodyLines, nextIndex: index + 1 };
    }
    bodyLines.push(line);
  }
  throw new Error('unterminated brace block in the contract');
}

/** Removes a `//` comment from a line without touching quoted strings. */
function stripLineComment(line) {
  let insideQuote = false;
  for (let index = 0; index < line.length; index += 1) {
    const character = line[index];
    if (character === '"') {
      insideQuote = !insideQuote;
    } else if (!insideQuote && character === '/' && line[index + 1] === '/') {
      return line.slice(0, index);
    }
  }
  return line;
}

/** Reads consecutive `///` doc lines from a block body scanner. */
function readDocLine(line, docLines) {
  const trimmed = line.trim();
  if (trimmed.startsWith('///')) {
    docLines.push(trimmed.replace(/^\/\/\/\s?/u, ''));
    return true;
  }
  return false;
}

/** Parses the declarations inside a `namespace` block. */
function parseNamespaceBlock(bodyLines) {
  const functions = [];
  let docLines = [];
  let attributes = {};
  let index = 0;
  while (index < bodyLines.length) {
    const trimmed = bodyLines[index].trim();
    if (trimmed === '' || trimmed.startsWith('//') && !trimmed.startsWith('///')) {
      docLines = [];
      index += 1;
      continue;
    }
    if (readDocLine(bodyLines[index], docLines)) {
      index += 1;
      continue;
    }
    if (trimmed.startsWith('[')) {
      attributes = parseAttributes(trimmed);
      index += 1;
      continue;
    }
    let declaration = trimmed;
    while (!callableDeclarationIsComplete(declaration) && index + 1 < bodyLines.length) {
      index += 1;
      declaration += ` ${bodyLines[index].trim()}`;
    }
    const callable = parseCallable(declaration);
    functions.push({
      name: attributes.Name ?? callable.name,
      returnTypeText: callable.returnTypeText,
      returnType: callable.returnType,
      parameters: callable.parameters,
      async: attributes.Async === true,
      throws: attributes.Throws ?? null,
      docLines,
    });
    docLines = [];
    attributes = {};
    index += 1;
  }
  return functions;
}

/** True when a declaration has reached its terminating semicolon. */
function callableDeclarationIsComplete(declarationText) {
  const text = declarationText.trim();
  if (!text.endsWith(';')) {
    return false;
  }
  let depth = 0;
  for (const character of text) {
    if (character === '<' || character === '(') {
      depth += 1;
    } else if (character === '>' || character === ')') {
      depth -= 1;
    }
  }
  return depth === 0;
}

/** Parses the declarations inside an `interface` block. */
function parseInterfaceBlock(bodyLines, isError) {
  const constructors = [];
  const methods = [];
  const errorVariants = [];
  let docLines = [];
  let attributes = {};
  let index = 0;
  while (index < bodyLines.length) {
    const trimmed = bodyLines[index].trim();
    if (trimmed === '' || trimmed.startsWith('//') && !trimmed.startsWith('///')) {
      docLines = [];
      index += 1;
      continue;
    }
    if (readDocLine(bodyLines[index], docLines)) {
      index += 1;
      continue;
    }
    if (trimmed.startsWith('[')) {
      attributes = parseAttributes(trimmed);
      index += 1;
      continue;
    }
    let declaration = trimmed;
    while (!callableDeclarationIsComplete(declaration) && index + 1 < bodyLines.length) {
      index += 1;
      declaration += ` ${bodyLines[index].trim()}`;
    }
    const callable = parseCallable(declaration);
    const entry = {
      name: attributes.Name ?? callable.name,
      returnTypeText: callable.returnTypeText,
      returnType: callable.returnType,
      parameters: callable.parameters,
      async: attributes.Async === true,
      throws: attributes.Throws ?? null,
      docLines,
    };
    if (callable.name === 'constructor') {
      constructors.push(entry);
    } else if (isError) {
      errorVariants.push(entry);
    } else {
      methods.push(entry);
    }
    docLines = [];
    attributes = {};
    index += 1;
  }
  return { constructors, methods, errorVariants };
}

/** Parses the fields of a `dictionary` block. */
function parseDictionaryBlock(bodyLines) {
  const fields = [];
  let docLines = [];
  let index = 0;
  while (index < bodyLines.length) {
    const trimmed = bodyLines[index].trim();
    if (trimmed === '' || trimmed.startsWith('//') && !trimmed.startsWith('///')) {
      docLines = [];
      index += 1;
      continue;
    }
    if (readDocLine(bodyLines[index], docLines)) {
      index += 1;
      continue;
    }
    let declaration = trimmed;
    while (!declaration.trim().endsWith(';') && index + 1 < bodyLines.length) {
      index += 1;
      declaration += ` ${bodyLines[index].trim()}`;
    }
    const withoutSemicolon = declaration.trim().replace(/;$/u, '');
    const match = withoutSemicolon.match(/^(.+?)\s+([A-Za-z_]\w*)$/u);
    if (match === null) {
      throw new Error(`could not parse dictionary field "${withoutSemicolon}"`);
    }
    fields.push({
      name: match[2],
      typeText: match[1],
      type: parseUdlType(match[1]),
      docLines,
    });
    docLines = [];
    index += 1;
  }
  return fields;
}

/** Parses the variants of an `enum` block. */
function parseEnumBlock(bodyLines) {
  const variants = [];
  let docLines = [];
  for (const line of bodyLines) {
    const trimmed = line.trim();
    if (trimmed === '' || trimmed.startsWith('//') && !trimmed.startsWith('///')) {
      docLines = [];
      continue;
    }
    if (readDocLine(line, docLines)) {
      continue;
    }
    const match = trimmed.match(/^"([^"]+)"\s*,?$/u);
    if (match === null) {
      throw new Error(`could not parse enum variant "${trimmed}"`);
    }
    variants.push({ name: match[1], docLines });
    docLines = [];
  }
  return variants;
}

/**
 * Parses the frozen UniFFI contract into its declaration model, preserving
 * the contract's declaration order.
 */
export function parseUdl(sourceText) {
  // UDL permits an attribute run to share a line with its declaration
  // (`[Error] interface SdkError {`). Normalize that form to one attribute
  // line followed by one declaration line so the block scanner sees a
  // consistent shape.
  const lines = [];
  for (const line of sourceText.replace(/\r\n?/gu, '\n').split('\n')) {
    const inlineAttributeMatch = line.match(
      /^(\[[^\]]*\])\s+((?:namespace|dictionary|enum|interface)\b.*)$/u,
    );
    if (inlineAttributeMatch === null) {
      lines.push(line);
    } else {
      lines.push(inlineAttributeMatch[1], inlineAttributeMatch[2]);
    }
  }
  const model = {
    namespace: null,
    functions: [],
    dictionaries: [],
    enums: [],
    interfaces: [],
  };
  let docLines = [];
  let attributes = {};
  let index = 0;
  while (index < lines.length) {
    const trimmed = lines[index].trim();
    if (trimmed === '' || trimmed.startsWith('//') && !trimmed.startsWith('///')) {
      docLines = [];
      attributes = {};
      index += 1;
      continue;
    }
    if (readDocLine(lines[index], docLines)) {
      index += 1;
      continue;
    }
    if (trimmed.startsWith('[')) {
      attributes = parseAttributes(trimmed);
      index += 1;
      continue;
    }
    const namespaceMatch = trimmed.match(/^namespace\s+(\w+)\s*\{/u);
    if (namespaceMatch !== null) {
      model.namespace = namespaceMatch[1];
      const block = collectBraceBlock(lines, index);
      model.functions = parseNamespaceBlock(block.bodyLines);
      index = block.nextIndex;
      docLines = [];
      attributes = {};
      continue;
    }
    const dictionaryMatch = trimmed.match(/^dictionary\s+(\w+)\s*\{/u);
    if (dictionaryMatch !== null) {
      const block = collectBraceBlock(lines, index);
      model.dictionaries.push({
        name: dictionaryMatch[1],
        docLines,
        fields: parseDictionaryBlock(block.bodyLines),
      });
      index = block.nextIndex;
      docLines = [];
      attributes = {};
      continue;
    }
    const enumMatch = trimmed.match(/^enum\s+(\w+)\s*\{/u);
    if (enumMatch !== null) {
      const block = collectBraceBlock(lines, index);
      model.enums.push({
        name: enumMatch[1],
        docLines,
        variants: parseEnumBlock(block.bodyLines),
      });
      index = block.nextIndex;
      docLines = [];
      attributes = {};
      continue;
    }
    const interfaceMatch = trimmed.match(/^interface\s+(\w+)\s*\{/u);
    if (interfaceMatch !== null) {
      const block = collectBraceBlock(lines, index);
      const parsed = parseInterfaceBlock(block.bodyLines, attributes.Error === true);
      model.interfaces.push({
        name: interfaceMatch[1],
        docLines,
        isError: attributes.Error === true,
        isTrait: attributes.Trait === true,
        isForeign: attributes.WithForeign === true,
        ...parsed,
      });
      index = block.nextIndex;
      docLines = [];
      attributes = {};
      continue;
    }
    // Anything else at top level is a comment or a construct this page does
    // not document; skip it without consuming the next item's doc comment.
    index += 1;
  }
  if (model.interfaces.length === 0 || model.functions.length === 0) {
    throw new Error('the contract did not yield a namespace and interfaces');
  }
  return model;
}

// ---------------------------------------------------------------------------
// Generated binding facts
// ---------------------------------------------------------------------------

/** Reads a file when it exists, else returns null. */
function readOptionalFile(filePath) {
  return existsSync(filePath) ? readFileSync(filePath, 'utf8') : null;
}

/** Collects the generated Kotlin declarations. */
function collectKotlinFacts(filePath) {
  const sourceText = readOptionalFile(filePath);
  if (sourceText === null) {
    return { isPresent: false, typeNames: new Set(), enumEntries: new Map(), errorTypeName: null };
  }
  const model = parseKotlinBindings(sourceText);
  const errorTypeMatch = sourceText.match(/\bsealed\s+class\s+(\w+)\s*:/u)
    ?? sourceText.match(/\bclass\s+(\w+Exception)\s*:/u);
  return {
    isPresent: true,
    typeNames: model.types,
    enumEntries: model.enumEntries,
    errorTypeName: errorTypeMatch === null ? null : errorTypeMatch[1],
  };
}

/** Collects the generated Swift declarations. */
function collectSwiftFacts(filePath) {
  const sourceText = readOptionalFile(filePath);
  if (sourceText === null) {
    return { isPresent: false, typeNames: new Set(), enumEntries: new Map(), errorTypeName: null };
  }
  const typeNames = new Set();
  const enumEntries = new Map();
  const typePattern = /\bpublic\s+(?:final\s+)?(?:class|struct|enum|protocol)\s+(\w+)/gu;
  for (const match of sourceText.matchAll(typePattern)) {
    typeNames.add(match[1]);
    if (match[0].includes('enum')) {
      enumEntries.set(match[1], new Set());
    }
  }
  let currentEnum = null;
  let depth = 0;
  for (const line of sourceText.split('\n')) {
    const enumMatch = line.match(/\bpublic\s+enum\s+(\w+)/u);
    if (enumMatch !== null) {
      currentEnum = enumMatch[1];
      depth = 0;
    }
    if (currentEnum !== null) {
      const caseMatch = line.match(/^\s*case\s+(\w+)/u);
      if (caseMatch !== null) {
        enumEntries.get(currentEnum)?.add(caseMatch[1]);
      }
      depth += (line.match(/\{/gu) ?? []).length;
      depth -= (line.match(/\}/gu) ?? []).length;
      if (depth <= 0 && line.includes('}')) {
        currentEnum = null;
      }
    }
  }
  const errorTypeMatch = sourceText.match(/public\s+enum\s+(\w+)\s*:\s*(?:Swift\.)?Error\b/u);
  return {
    isPresent: true,
    typeNames,
    enumEntries,
    errorTypeName: errorTypeMatch === null ? null : errorTypeMatch[1],
  };
}

/** Collects the generated React Native TypeScript declarations. */
function collectTypeScriptFacts(filePath) {
  const sourceText = readOptionalFile(filePath);
  if (sourceText === null) {
    return { isPresent: false, typeNames: new Set(), enumEntries: new Map(), errorTypeName: null };
  }
  const typeNames = new Set();
  const enumEntries = new Map();
  for (const match of sourceText.matchAll(
    /\bexport\s+(?:class|interface|enum|type|function|const)\s+(\w+)/gu,
  )) {
    typeNames.add(match[1]);
  }
  let currentEnum = null;
  for (const line of sourceText.split('\n')) {
    const enumMatch = line.match(/\bexport\s+enum\s+(\w+)/u);
    if (enumMatch !== null) {
      currentEnum = enumMatch[1];
      enumEntries.set(currentEnum, new Set());
    }
    if (currentEnum !== null) {
      const memberMatch = line.match(/^\s*(\w+)\s*=/u);
      if (memberMatch !== null) {
        enumEntries.get(currentEnum)?.add(memberMatch[1]);
      }
      if (line.includes('}')) {
        currentEnum = null;
      }
    }
  }
  const errorTypeMatch = sourceText.match(/export\s+class\s+(\w+)\s+extends\s+Error\b/u);
  return {
    isPresent: true,
    typeNames,
    enumEntries,
    errorTypeName: errorTypeMatch === null ? null : errorTypeMatch[1],
  };
}

// ---------------------------------------------------------------------------
// Type and naming rendering
// ---------------------------------------------------------------------------

/** Renders a UDL type for one platform language. */
function renderUdlType(type, language) {
  if (type.kind === 'optional') {
    const inner = renderUdlType(type.inner, language);
    if (language === 'rust') {
      return `Option<${inner}>`;
    }
    if (language === 'typescript') {
      return `${inner} | undefined`;
    }
    return `${inner}?`;
  }
  if (type.kind === 'sequence') {
    const inner = renderUdlType(type.inner, language);
    if (language === 'kotlin') {
      return `List<${inner}>`;
    }
    if (language === 'swift') {
      return `[${inner}]`;
    }
    if (language === 'typescript') {
      return `${inner}[]`;
    }
    return `Vec<${inner}>`;
  }
  if (type.kind === 'bytes') {
    return {
      rust: 'Vec<u8>',
      kotlin: 'ByteArray',
      swift: 'Data',
      typescript: 'ArrayBuffer',
    }[language];
  }
  if (type.kind === 'void') {
    return {
      rust: '()',
      kotlin: 'Unit',
      swift: 'Void',
      typescript: 'void',
    }[language];
  }
  const primitive = PRIMITIVE_TYPE_NAMES.get(type.name);
  if (primitive !== undefined) {
    return primitive[language];
  }
  return type.name;
}

/** Renders the member name spelling for one platform language. */
function renderMemberName(name, language) {
  if (language === 'kotlin' || language === 'swift' || language === 'typescript') {
    return snakeToCamel(name);
  }
  return name;
}

/** Renders an enum variant spelling for one platform language. */
function renderEnumVariantName(enumName, variantName, language, facts) {
  const generatedEntries = facts.enumEntries.get(enumName);
  if (generatedEntries !== undefined && generatedEntries.size > 0) {
    const candidates = [variantName, renderEnumVariantNameWithoutFacts(variantName, language)];
    for (const candidate of candidates) {
      if (generatedEntries.has(candidate)) {
        return candidate;
      }
    }
    // A generated enum whose case spellings differ wholesale still yields one
    // spelling per declaration; fall back to the documented convention.
  }
  return renderEnumVariantNameWithoutFacts(variantName, language);
}

/** Renders an enum variant spelling from the language convention alone. */
function renderEnumVariantNameWithoutFacts(variantName, language) {
  if (language === 'kotlin') {
    return pascalToUpperSnake(variantName);
  }
  if (language === 'swift') {
    return pascalToLowerCamel(variantName);
  }
  return variantName;
}

/** Renders a nullable record field spelling. */
function renderRecordFieldName(field, language) {
  const name = renderMemberName(field.name, language);
  if (language === 'typescript' && field.type.kind === 'optional') {
    return `${name}?`;
  }
  return name;
}

/** Renders a parameter list for one platform language. */
function renderParameters(parameters, language) {
  return parameters
    .map(
      (parameter) =>
        `${renderMemberName(parameter.name, language)}: ${renderUdlType(parameter.type, language)}`,
    )
    .join(', ');
}

/** Renders the `async` modifier and return annotation of a call for Kotlin. */
function renderKotlinReturn(returnType) {
  return renderUdlType(returnType, 'kotlin');
}

/** Renders a callable signature for the requested platform language. */
function renderCallableSignature(
  callable,
  language,
  isConstructor,
  isNamedConstructor,
  ownerName = null,
) {
  const parameters = renderParameters(callable.parameters, language);
  if (language === 'kotlin') {
    if (isConstructor) {
      return isNamedConstructor
        ? `fun ${snakeToCamel(callable.name)}(${parameters}): ${ownerName}`
        : `constructor(${parameters})`;
    }
    const asyncPrefix = callable.async ? 'suspend ' : '';
    return `${asyncPrefix}fun ${snakeToCamel(callable.name)}(${parameters}): ${renderKotlinReturn(callable.returnType)}`;
  }
  if (language === 'swift') {
    if (isConstructor) {
      const throwsSuffix = callable.throws === null ? '' : ' throws';
      return isNamedConstructor
        ? `static func ${snakeToCamel(callable.name)}(${parameters})${throwsSuffix} -> ${ownerName}`
        : `init(${parameters})${throwsSuffix}`;
    }
    const asyncSuffix = callable.async ? ' async' : '';
    const throwsSuffix = callable.throws === null ? '' : ' throws';
    const returnSuffix =
      callable.returnType.kind === 'void'
        ? ''
        : ` -> ${renderUdlType(callable.returnType, 'swift')}`;
    return `func ${snakeToCamel(callable.name)}(${parameters})${asyncSuffix}${throwsSuffix}${returnSuffix}`;
  }
  // TypeScript / React Native
  if (isConstructor) {
    return isNamedConstructor
      ? `static ${snakeToCamel(callable.name)}(${parameters}): ${ownerName}`
      : `constructor(${parameters})`;
  }
  const asyncPrefix = callable.async ? 'async ' : '';
  const abortOptions = callable.async ? `${parameters === '' ? '' : ', '}asyncOpts_?: { signal: AbortSignal }` : '';
  const returnType = renderUdlType(callable.returnType, 'typescript');
  const returnAnnotation = callable.async ? `Promise<${returnType}>` : returnType;
  return `${asyncPrefix}${snakeToCamel(callable.name)}(${parameters}${abortOptions}): ${returnAnnotation}`;
}

/** Describes a callable's return and error behavior for the table. */
function describeCallableResult(callable, platformConfiguration) {
  const notes = [];
  if (callable.async) {
    notes.push('Asynchronous.');
  }
  if (callable.returnType.kind !== 'void') {
    notes.push(
      `Returns \`${renderUdlType(callable.returnType, platformConfiguration.language)}\`.`,
    );
  }
  if (callable.throws !== null) {
    notes.push(`Throws \`${platformConfiguration.errorTypeName}\`.`);
  }
  if (notes.length === 0) {
    return '—';
  }
  return notes.join(' ');
}

/** The generated header note every page starts with. */
function generatedPageNote(bindingsLabel) {
  return `> Generated from the SDK contract and the generated ${bindingsLabel} bindings. Do not edit this page by hand; change the contract and regenerate the documentation.`;
}

// ---------------------------------------------------------------------------
// Native platform pages
// ---------------------------------------------------------------------------

/** Renders the naming conventions table for a native platform. */
function renderNamingTable(platformConfiguration) {
  return renderTable(
    ['Concept', 'Spelling'],
    [
      ['Type', 'PascalCase'],
      ['Member', platformConfiguration.memberCase],
      ['Enum variant', platformConfiguration.enumCase],
      ['Record field', platformConfiguration.recordCase],
      ['Error type', `\`${platformConfiguration.errorTypeName}\``],
    ],
  );
}

/** Renders the namespace functions section. */
function renderNamespaceFunctions(model, platformConfiguration) {
  const rows = model.functions.map((callable) => [
    `\`${renderMemberName(callable.name, platformConfiguration.language)}\``,
    `\`${renderCallableSignature(callable, platformConfiguration.language, false, false)}\``,
    describeCallableResult(callable, platformConfiguration),
    docToTableCell(callable.docLines),
  ]);
  return renderTable(['Function', 'Signature', 'Returns / errors', 'Purpose'], rows);
}

/** Renders one interface section. */
function renderInterface(interfaceDefinition, platformConfiguration) {
  const lines = [`### ${interfaceDefinition.name}`, ''];
  const markdown = docToMarkdown(interfaceDefinition.docLines);
  if (markdown !== '') {
    lines.push(markdown, '');
  }
  if (interfaceDefinition.isTrait) {
    lines.push(
      `Implement this interface in ${platformConfiguration.languageLabel}; the SDK invokes it through the matching registration or subscription method.`,
      '',
    );
  }
  if (interfaceDefinition.constructors.length > 0) {
    lines.push(
      '**Construction**',
      '',
      renderTable(
        ['Constructor', 'Signature', 'Purpose'],
        interfaceDefinition.constructors.map((constructor) => {
          const isNamed = constructor.name !== 'constructor';
          return [
            `\`${isNamed ? renderMemberName(constructor.name, platformConfiguration.language) : 'constructor'}\``,
            `\`${renderCallableSignature(
              constructor,
              platformConfiguration.language,
              true,
              isNamed,
              interfaceDefinition.name,
            )}\``,
            docToTableCell(constructor.docLines),
          ];
        }),
      ),
      '',
    );
  }
  if (interfaceDefinition.methods.length > 0) {
    lines.push(
      '**Members**',
      '',
      renderTable(
        ['Member', 'Signature', 'Returns / errors', 'Purpose'],
        interfaceDefinition.methods.map((method) => [
          `\`${renderMemberName(method.name, platformConfiguration.language)}\``,
          `\`${renderCallableSignature(method, platformConfiguration.language, false, false)}\``,
          describeCallableResult(method, platformConfiguration),
          docToTableCell(method.docLines),
        ]),
      ),
      '',
    );
  } else {
    lines.push('This interface declares no members.', '');
  }
  return lines;
}

/** Renders one record (dictionary) section. */
function renderDictionary(dictionary, platformConfiguration) {
  const lines = [`### ${dictionary.name}`, ''];
  const markdown = docToMarkdown(dictionary.docLines);
  if (markdown !== '') {
    lines.push(markdown, '');
  }
  lines.push(
    renderTable(
      ['Field', 'Type', 'Purpose'],
      dictionary.fields.map((field) => [
        `\`${renderRecordFieldName(field, platformConfiguration.language)}\``,
        `\`${renderUdlType(
          field.type.kind === 'optional' && platformConfiguration.language === 'typescript'
            ? field.type.inner
            : field.type,
          platformConfiguration.language,
        )}\``,
        docToTableCell(field.docLines),
      ]),
    ),
    '',
  );
  return lines;
}

/** Renders one enum section. */
function renderEnum(enumDefinition, platformConfiguration, facts) {
  const lines = [`### ${enumDefinition.name}`, ''];
  const markdown = docToMarkdown(enumDefinition.docLines);
  if (markdown !== '') {
    lines.push(markdown, '');
  }
  lines.push(
    renderTable(
      ['Variant', 'Purpose'],
      enumDefinition.variants.map((variant) => [
        `\`${renderEnumVariantName(
          enumDefinition.name,
          variant.name,
          platformConfiguration.language,
          facts,
        )}\``,
        docToTableCell(variant.docLines),
      ]),
    ),
    '',
  );
  return lines;
}

/** Renders an error variant spelling for one platform language. */
function renderErrorVariantName(variantName, language) {
  if (language === 'swift') {
    return pascalToLowerCamel(variantName);
  }
  return variantName;
}

/** Describes how the error variants cross one platform surface. */
function describeErrorSurface(platformConfiguration) {
  if (platformConfiguration.language === 'kotlin') {
    return `Each variant is a nested class of the \`${platformConfiguration.errorTypeName}\` sealed class.`;
  }
  if (platformConfiguration.language === 'swift') {
    return `Each variant is a case of the \`${platformConfiguration.errorTypeName}\` enum.`;
  }
  return `Each variant is a tag of the \`${platformConfiguration.errorTypeName}\` class.`;
}

/** Renders the error interface section. */
function renderErrorInterface(interfaceDefinition, platformConfiguration) {
  const lines = [`### ${interfaceDefinition.name}`, ''];
  const markdown = docToMarkdown(interfaceDefinition.docLines);
  if (markdown !== '') {
    lines.push(markdown, '');
  }
  lines.push(
    describeErrorSurface(platformConfiguration),
    '',
    renderTable(
      ['Variant', 'Payload', 'Purpose'],
      interfaceDefinition.errorVariants.map((variant) => [
        `\`${renderErrorVariantName(variant.name, platformConfiguration.language)}\``,
        variant.parameters.length === 0
          ? 'None'
          : variant.parameters
              .map(
                (parameter) =>
                  `${renderMemberName(parameter.name, platformConfiguration.language)}: ${renderUdlType(parameter.type, platformConfiguration.language)}`,
              )
              .join(', '),
        docToTableCell(variant.docLines),
      ]),
    ),
    '',
  );
  return lines;
}

/** Renders the async and threading rules. */
function renderThreadingSection(model, platformConfiguration) {
  const asynchronousNamespaceCalls = model.functions
    .filter((callable) => callable.async)
    .map((callable) => `\`${renderMemberName(callable.name, platformConfiguration.language)}\``);
  const asynchronousInterfaceCalls = [];
  for (const interfaceDefinition of model.interfaces) {
    for (const method of interfaceDefinition.methods) {
      if (method.async) {
        asynchronousInterfaceCalls.push(
          `\`${interfaceDefinition.name}.${renderMemberName(method.name, platformConfiguration.language)}\``,
        );
      }
    }
  }
  const lines = ['## Async and threading', ''];
  const allAsynchronousCalls = [...asynchronousNamespaceCalls, ...asynchronousInterfaceCalls];
  if (allAsynchronousCalls.length > 0) {
    lines.push(
      `These calls are asynchronous on ${platformConfiguration.languageLabel}: ${allAsynchronousCalls.join(', ')}.`,
      '',
    );
  }
  if (platformConfiguration.language === 'kotlin') {
    lines.push(
      'Asynchronous calls are `suspend` functions and do not block the calling thread. The shipped map view members expect the main thread.',
      '',
    );
  } else if (platformConfiguration.language === 'swift') {
    lines.push(
      'Asynchronous calls are `async throws` functions and do not block the calling thread. The shipped map view members expect the main thread.',
      '',
    );
  } else {
    lines.push(
      'Asynchronous calls return a `Promise` and accept an optional `AbortSignal`; they do not block the JavaScript thread.',
      '',
    );
  }
  lines.push(
    '- `MapView.draw` is frame-driven: call it from the platform display callback. A concurrent draw or resize reports `RenderingSurfaceUnavailable`; retry the next frame.',
    '- Navigation listener callbacks run synchronously on the thread that drives the controller. A listener must not block, and a callback made during an in-flight dispatch has its broadcast suppressed because the state change is already applied.',
    '- A diagnostics listener is called fire-and-forget on the thread that produced the event; it never delays the event stream.',
    '',
  );
  return lines;
}

/** Renders the next-steps links. */
function renderNextSteps(nextSteps) {
  return [
    '## Next steps',
    '',
    ...nextSteps.map(([label, target]) => `- [${label}](${target})`),
  ];
}

/** Renders one native platform API reference page. */
function renderNativePage(platformName, model, facts) {
  const platformConfiguration = PLATFORM_CONFIGURATIONS[platformName];
  const errorTypes = model.interfaces.filter((interfaceDefinition) => interfaceDefinition.isError);
  const regularInterfaces = model.interfaces.filter(
    (interfaceDefinition) => !interfaceDefinition.isError,
  );
  const lines = [
    `# ${platformConfiguration.title}`,
    '',
    generatedPageNote(platformConfiguration.bindingsLabel),
    '',
    platformConfiguration.intro,
    '',
    '## Names and conventions',
    '',
    renderNamingTable(platformConfiguration),
    '',
    '## Namespace functions',
    '',
    renderNamespaceFunctions(model, platformConfiguration),
    '',
    '## Interfaces',
    '',
  ];
  for (const interfaceDefinition of regularInterfaces) {
    lines.push(...renderInterface(interfaceDefinition, platformConfiguration));
  }
  lines.push('## Records', '');
  for (const dictionary of model.dictionaries) {
    lines.push(...renderDictionary(dictionary, platformConfiguration));
  }
  lines.push('## Enums', '');
  for (const enumDefinition of model.enums) {
    lines.push(...renderEnum(enumDefinition, platformConfiguration, facts));
  }
  lines.push('## Errors', '');
  for (const errorType of errorTypes) {
    lines.push(...renderErrorInterface(errorType, platformConfiguration));
  }
  lines.push(...renderThreadingSection(model, platformConfiguration), '');
  lines.push(...renderNextSteps(platformConfiguration.nextSteps), '');
  return joinPageLines(lines);
}

// ---------------------------------------------------------------------------
// WebAssembly surface parsing
// ---------------------------------------------------------------------------

/** Counts braces outside strings on one Rust source line. */
function countRustBraces(line) {
  let opens = 0;
  let closes = 0;
  let insideString = false;
  let insideCharacter = false;
  let escaped = false;
  for (let index = 0; index < line.length; index += 1) {
    const character = line[index];
    const next = line[index + 1];
    if (insideString) {
      if (escaped) {
        escaped = false;
      } else if (character === '\\') {
        escaped = true;
      } else if (character === '"') {
        insideString = false;
      }
      continue;
    }
    if (insideCharacter) {
      if (escaped) {
        escaped = false;
      } else if (character === '\\') {
        escaped = true;
      } else if (character === "'") {
        insideCharacter = false;
      }
      continue;
    }
    if (character === '/' && next === '/') {
      break;
    }
    if (character === '"') {
      insideString = true;
    } else if (character === "'") {
      // A Rust lifetime (`&'static str`) must not open a character literal.
      // Only a quoted one-character (or escaped) literal does.
      const remainder = line.slice(index + 1);
      if (/^\\[^']*'/u.test(remainder) || /^[^\\']'/u.test(remainder)) {
        insideCharacter = true;
      }
    } else if (character === '{') {
      opens += 1;
    } else if (character === '}') {
      closes += 1;
    }
  }
  return { opens, closes };
}

/** Reads `js_name = X` from a `#[wasm_bindgen(...)]` attribute. */
function wasmJsName(attributeText) {
  const match = attributeText.match(/js_name\s*=\s*(\w+)/u);
  return match === null ? null : match[1];
}

/** The wasm-bindgen JavaScript names of Rust enum helper types. */
const WASM_RUST_TYPE_NAMES = new Map([
  ['CanvasTileResolution', 'TileResolution'],
  ['CanvasTrafficFlowCongestionLevel', 'TrafficFlowCongestionLevel'],
  ['CanvasSdkLogSeverity', 'SdkLogSeverity'],
]);

/** Reads a Rust type into a TypeScript-facing description. */
function renderWasmType(rustTypeText) {
  let text = rustTypeText.trim().replace(/^&(?:mut\s+)?/u, '').replace(/^mut\s+/u, '');
  let isOptional = false;
  let throws = false;
  if (text.startsWith('Result<')) {
    throws = true;
    const inner = text.slice('Result<'.length, -1);
    const parts = splitTopLevel(inner, ',');
    text = parts.length > 0 ? parts[0].trim() : 'void';
  }
  if (text.startsWith('Option<') && text.endsWith('>')) {
    isOptional = true;
    text = text.slice('Option<'.length, -1).trim();
  }
  let rendered;
  if (text === '()' || text === 'void') {
    rendered = 'void';
  } else if (text === 'String' || text === 'str') {
    rendered = 'string';
  } else if (['f64', 'f32', 'u8', 'u16', 'u32', 'u64', 'i8', 'i16', 'i32', 'i64', 'usize'].includes(text)) {
    rendered = 'number';
  } else if (text === 'bool') {
    rendered = 'boolean';
  } else if (text === 'JsValue') {
    rendered = 'any';
  } else if (text === 'Object' || text === 'js_sys::Object') {
    rendered = 'object';
  } else if (text === 'Array' || text === 'js_sys::Array') {
    rendered = 'any[]';
  } else if (text === 'Vec<u8>' || text === '[u8]') {
    rendered = 'Uint8Array';
  } else if (text.startsWith('Vec<')) {
    const inner = text.slice('Vec<'.length, -1).trim();
    rendered = `${renderWasmType(inner).type}[]`;
  } else if (text === 'js_sys::Function') {
    rendered = 'Function';
  } else if (text === 'js_sys::Promise') {
    rendered = 'Promise<any>';
  } else if (text === 'HtmlCanvasElement') {
    rendered = 'HTMLCanvasElement';
  } else if (WASM_RUST_TYPE_NAMES.has(text)) {
    rendered = WASM_RUST_TYPE_NAMES.get(text);
  } else if (text.length > 0 && /^[A-Z]\w*$/u.test(text)) {
    rendered = text;
  } else {
    rendered = 'any';
  }
  if (isOptional) {
    rendered = `${rendered} | null`;
  }
  return { type: rendered, throws };
}

/** Parses the exported classes, functions and enums of the wasm sources. */
function collectWasmSurface(repositoryRoot) {
  const sourceDirectory = path.join(repositoryRoot, WASM_BINDINGS_SOURCE_DIRECTORY);
  const sourceFiles = readdirSync(sourceDirectory)
    .filter((entry) => entry.endsWith('.rs'))
    .sort();
  const orderedFiles = [
    ...WASM_SOURCE_FILE_ORDER.filter((file) => sourceFiles.includes(file)),
    ...sourceFiles.filter((file) => !WASM_SOURCE_FILE_ORDER.includes(file)),
  ];
  const classes = new Map();
  const classesByName = new Map();
  const freeFunctions = [];
  const enums = new Map();

  for (const fileName of orderedFiles) {
    const filePath = path.join(sourceDirectory, fileName);
    const lines = readFileSync(filePath, 'utf8').replace(/\r\n?/gu, '\n').split('\n');
    let docLines = [];
    let attributeLines = [];
    let currentClass = null;
    let classDepth = null;
    let depth = 0;
    let enumContext = null;

    for (let index = 0; index < lines.length; index += 1) {
      const line = lines[index];
      const trimmed = line.trim();
      if (trimmed.startsWith('///')) {
        docLines.push(trimmed.replace(/^\/\/\/\s?/u, ''));
        continue;
      }
      if (trimmed.startsWith('#[')) {
        attributeLines.push(trimmed);
        continue;
      }
      if (trimmed === '') {
        docLines = [];
        attributeLines = [];
        continue;
      }
      if (trimmed.startsWith('//') && !trimmed.startsWith('///')) {
        // A plain comment between a doc comment and its item does not detach
        // the documentation; keep both buffers.
        continue;
      }

      const combinedAttributes = attributeLines.join(' ');
      const isWasmAttribute = /#\[wasm_bindgen/u.test(combinedAttributes);
      const implMatch = trimmed.match(/^impl\s+(\w+)\s*\{/u);
      if (implMatch !== null) {
        if (isWasmAttribute) {
          currentClass = implMatch[1];
          classDepth = depth + 1;
        }
        depth += countRustBraces(line).opens - countRustBraces(line).closes;
        attributeLines = [];
        docLines = [];
        continue;
      }

      const structMatch = trimmed.match(/^pub\s+struct\s+(\w+)/u);
      if (structMatch !== null && isWasmAttribute) {
        const className = wasmJsName(combinedAttributes) ?? structMatch[1];
        const classEntry = { name: className, docLines: [...docLines], constructors: [], methods: [], properties: [] };
        classes.set(className, classEntry);
        classesByName.set(structMatch[1], classEntry);
        attributeLines = [];
        docLines = [];
      }

      const enumMatch = trimmed.match(/^pub\s+enum\s+(\w+)/u);
      if (enumMatch !== null && isWasmAttribute) {
        const enumName = wasmJsName(combinedAttributes) ?? enumMatch[1];
        enumContext = { name: enumName, docLines: [...docLines], variants: [] };
        enums.set(enumName, enumContext);
        attributeLines = [];
        docLines = [];
      } else if (enumContext !== null) {
        const variantMatch = trimmed.match(/^(\w+)(?:\s*=\s*(\d+))?\s*,?$/u);
        if (variantMatch !== null && variantMatch[1] !== 'pub') {
          enumContext.variants.push({
            name: variantMatch[1],
            valueText: variantMatch[2] ?? null,
          });
        }
        if (trimmed.includes('}')) {
          enumContext = null;
        }
      }

      // Only `pub` functions are exported from a `#[wasm_bindgen]` impl; a
      // private helper stays private even inside the annotated block.
      const functionMatch = trimmed.match(/^pub\s+(async\s+)?(?:unsafe\s+)?fn\s+(\w+)\s*\(/u);
      const isExportedFunction =
        functionMatch !== null && (isWasmAttribute || currentClass !== null);
      if (isExportedFunction) {
        let signature = trimmed;
        let signatureIndex = index;
        while (
          !wasmSignatureIsComplete(signature)
          && signatureIndex + 1 < lines.length
        ) {
          signatureIndex += 1;
          signature += ` ${lines[signatureIndex].trim()}`;
        }
        const parsed = parseWasmFunctionSignature(signature);
        if (parsed !== null) {
          const jsName = wasmJsName(combinedAttributes) ?? snakeToCamel(functionMatch[2]);
          const isConstructor = combinedAttributes.includes('constructor');
          const isGetter = combinedAttributes.includes('getter');
          const isAsync = functionMatch[1] !== undefined || parsed.isAsync;
          const entry = {
            name: jsName,
            docLines: [...docLines],
            parameters: parsed.parameters,
            returnDescription: parsed.returnDescription,
            isAsync,
            isConstructor,
            isGetter,
            isStatic: currentClass !== null && !parsed.hasSelf && !isConstructor,
          };
          const classEntry = currentClass === null ? null : classesByName.get(currentClass);
          if (classEntry !== null && classEntry !== undefined) {
            if (isConstructor) {
              classEntry.constructors.push(entry);
            } else if (isGetter) {
              classEntry.properties.push(entry);
            } else {
              classEntry.methods.push(entry);
            }
          } else {
            freeFunctions.push({ ...entry, fileName });
          }
        }
        // The signature's opening brace belongs to the function body; count
        // it so the class-depth tracking stays aligned with the real braces.
        const signatureBraces = countRustBraces(signature);
        depth += signatureBraces.opens - signatureBraces.closes;
        index = signatureIndex;
        attributeLines = [];
        docLines = [];
        continue;
      }

      const braces = countRustBraces(line);
      depth += braces.opens - braces.closes;
      if (currentClass !== null && classDepth !== null && depth < classDepth) {
        currentClass = null;
        classDepth = null;
      }
      if (!trimmed.startsWith('#[') && trimmed !== '') {
        attributeLines = [];
        if (!trimmed.startsWith('///')) {
          docLines = [];
        }
      }
    }
  }

  return { classes, freeFunctions, enums };
}

/** True when a wasm function signature has reached its body or terminator. */
function wasmSignatureIsComplete(signatureText) {
  const trimmed = signatureText.trim();
  if (trimmed.endsWith(';') || /\{\s*$/u.test(trimmed)) {
    return true;
  }
  return trimmed.includes('{') && trimmed.includes('}');
}

/** Parses one wasm function signature into parameters and a return. */
function parseWasmFunctionSignature(signatureText) {
  const signature = signatureText.replace(/\{\s*$/u, '').trim();
  const openIndex = signature.indexOf('(');
  if (openIndex === -1) {
    return null;
  }
  let depth = 0;
  let closeIndex = -1;
  for (let index = openIndex; index < signature.length; index += 1) {
    if (signature[index] === '(') {
      depth += 1;
    } else if (signature[index] === ')') {
      depth -= 1;
      if (depth === 0) {
        closeIndex = index;
        break;
      }
    }
  }
  if (closeIndex === -1) {
    return null;
  }
  const parameterText = signature.slice(openIndex + 1, closeIndex);
  let hasSelf = false;
  const parameters = [];
  for (const parameter of splitTopLevel(parameterText, ',')) {
    if (/^&?(?:mut\s+)?self$/u.test(parameter.trim())) {
      hasSelf = true;
      continue;
    }
    const match = parameter.trim().match(/^([\s\S]+?)\s*:\s*([\s\S]+)$/u);
    if (match === null) {
      return null;
    }
    parameters.push({
      name: match[1].trim().replace(/^&(?:mut\s+)?/u, ''),
      typeText: match[2].trim(),
    });
  }
  const afterParameters = signature.slice(closeIndex + 1).trim();
  const returnMatch = afterParameters.match(/->\s*([\s\S]+)$/u);
  const returnText = returnMatch === null ? '()' : returnMatch[1].trim();
  return {
    parameters,
    hasSelf,
    returnDescription: renderWasmType(returnText),
    isAsync: /async\s+fn/u.test(signature),
  };
}

// ---------------------------------------------------------------------------
// WebAssembly declaration mirror parsing
// ---------------------------------------------------------------------------

/** Parses the `export interface` and `export type` declarations of a .d.ts. */
function parseTypeScriptDeclarations(sourceText) {
  const lines = sourceText.replace(/\r\n?/gu, '\n').split('\n');
  const interfaces = [];
  const typeAliases = [];
  const classes = [];
  let docLines = [];
  let index = 0;
  while (index < lines.length) {
    const trimmed = lines[index].trim();
    if (trimmed.startsWith('/**')) {
      const commentLines = [];
      while (index < lines.length && !lines[index].includes('*/')) {
        commentLines.push(lines[index].replace(/^\s*\*\s?/u, '').replace(/^\s*\/\*\*?/u, ''));
        index += 1;
      }
      if (index < lines.length) {
        commentLines.push(lines[index].replace(/^\s*\*\//u, '').replace(/^\s*\*\s?/u, ''));
        index += 1;
      }
      docLines = commentLines;
      continue;
    }
    if (trimmed === '') {
      index += 1;
      continue;
    }
    const interfaceMatch = trimmed.match(/^export\s+interface\s+(\w+)(?:\s+extends\s+[^{]+)?\s*\{/u);
    if (interfaceMatch !== null) {
      const fieldLines = [];
      let depth = 0;
      let started = false;
      for (; index < lines.length; index += 1) {
        const line = lines[index];
        const braces = countRustBraces(line);
        if (started) {
          fieldLines.push(line);
        }
        if (line.includes('{')) {
          started = true;
        }
        depth += braces.opens - braces.closes;
        if (started && depth <= 0) {
          break;
        }
      }
      interfaces.push({
        name: interfaceMatch[1],
        docLines,
        fields: parseTypeScriptInterfaceFields(fieldLines),
      });
      docLines = [];
      index += 1;
      continue;
    }
    const typeMatch = trimmed.match(/^export\s+type\s+(\w+)\s*=/u);
    if (typeMatch !== null) {
      let declaration = trimmed;
      let depth = (trimmed.match(/[<({[]/gu) ?? []).length - (trimmed.match(/[>)}]/gu) ?? []).length;
      while (!declaration.trim().endsWith(';') && index + 1 < lines.length) {
        index += 1;
        declaration += ` ${lines[index].trim()}`;
        depth += (lines[index].match(/[<({[]/gu) ?? []).length - (lines[index].match(/[>)}]/gu) ?? []).length;
        if (depth <= 0 && declaration.trim().endsWith(';')) {
          break;
        }
      }
      typeAliases.push({
        name: typeMatch[1],
        docLines,
        shape: declaration.trim().replace(/^export\s+type\s+\w+\s*=\s*/u, '').replace(/;$/u, ''),
      });
      docLines = [];
      index += 1;
      continue;
    }
    const classMatch = trimmed.match(/^export\s+declare\s+class\s+(\w+)(?:\s+[^{]+)?\s*\{/u);
    if (classMatch !== null) {
      const bodyLines = [];
      let depth = 0;
      let started = false;
      for (; index < lines.length; index += 1) {
        const line = lines[index];
        const braces = countRustBraces(line);
        if (started) {
          bodyLines.push(line);
        }
        if (line.includes('{')) {
          started = true;
        }
        depth += braces.opens - braces.closes;
        if (started && depth <= 0) {
          break;
        }
      }
      classes.push({
        name: classMatch[1],
        docLines,
        members: parseTypeScriptClassMembers(bodyLines, classMatch[1]),
      });
      docLines = [];
      index += 1;
      continue;
    }
    docLines = [];
    index += 1;
  }
  return { interfaces, typeAliases, classes };
}

/** Bracket depth of one TypeScript declaration line. */
function typeScriptBracketDepth(text) {
  const opens = (text.match(/[(\[{]/gu) ?? []).length;
  const closes = (text.match(/[)\]}]/gu) ?? []).length;
  return opens - closes;
}

/** The summary sentence a member or class doc starts with. */
function firstDocParagraph(docLines) {
  const paragraph = [];
  for (const line of docLines) {
    const trimmed = line.trim();
    if (trimmed === '') {
      if (paragraph.length === 0) {
        continue;
      }
      break;
    }
    if (trimmed.startsWith('#')) {
      break;
    }
    paragraph.push(trimmed);
  }
  return paragraph.join(' ').trim();
}

/** One class member as the reference table presents it. */
function parseTypeScriptClassMemberWithClass(declaration, docLines, className) {
  const constructorMatch = declaration
    .replace(/;\s*$/u, '')
    .trim()
    .match(/^(static\s+)?constructor\(([\s\S]*)\)\s*(?::\s*([^;]+))?$/u);
  if (constructorMatch !== null) {
    return {
      label: `constructor(${typeScriptParameterNames(constructorMatch[2]).join(', ')})`,
      signature: constructorMatch[1] === undefined ? `new ${className}()` : `new ${className}()`,
      description: firstDocParagraph(docLines),
    };
  }
  return parseTypeScriptClassMember(declaration, docLines);
}

/**
 * Parses one class member's declaration and doc comment.
 *
 * Symbol-keyed internals are skipped by the caller. Getters, setters,
 * constructors, static members and plain properties all keep their own shape
 * in the rendered row.
 */
function parseTypeScriptClassMember(declaration, docLines) {
  const text = declaration.replace(/;\s*$/u, '').trim();
  const description = firstDocParagraph(docLines);
  const propertyMatch = text.match(/^(static\s+)?readonly\s+(\w+)\s*:\s*(.+)$/u);
  if (propertyMatch !== null) {
    return {
      label: `${propertyMatch[1] === undefined ? '' : 'static '}${propertyMatch[2]}`,
      signature: propertyMatch[3].trim(),
      description,
    };
  }
  const getterMatch = text.match(/^get\s+(\w+)\(\)\s*:\s*(.+)$/u);
  if (getterMatch !== null) {
    return {
      label: `${getterMatch[1]} (getter)`,
      signature: getterMatch[2].trim(),
      description,
    };
  }
  const setterMatch = text.match(/^set\s+(\w+)\(([^)]*)\)\s*:\s*(.+)$/u);
  if (setterMatch !== null) {
    const parameters = typeScriptParameterNames(setterMatch[2]);
    return {
      label: `${setterMatch[1]}(${parameters.join(', ')}) (setter)`,
      signature: setterMatch[3].trim(),
      description,
    };
  }
  const methodMatch = text.match(/^(static\s+)?(\w+)(?:<[^>]*>)?\??\(([\s\S]*)\)\s*:\s*([^;]+)$/u);
  if (methodMatch !== null) {
    const isConstructor = methodMatch[2] === 'constructor';
    const parameters = typeScriptParameterNames(methodMatch[3]);
    return {
      label: `${methodMatch[1] === undefined || isConstructor ? '' : 'static '}${methodMatch[2]}(${parameters.join(', ')})`,
      signature: methodMatch[4].trim(),
      description,
    };
  }
  return null;
}

/** Parameter names of one TypeScript signature, types dropped. */
function typeScriptParameterNames(parametersText) {
  const parts = splitTopLevel(parametersText, ',');
  const names = [];
  for (const part of parts) {
    const trimmed = part.trim();
    if (trimmed === '') {
      continue;
    }
    const match = trimmed.match(/^(?:readonly\s+)?(\.\.\.)?(\w+)\??\s*:/u);
    if (match !== null) {
      names.push(`${match[1] ?? ''}${match[2]}`);
    }
  }
  return names;
}

/** Parses the members of one TypeScript class body. */
function parseTypeScriptClassMembers(bodyLines, className = '') {
  const members = [];
  let docLines = [];
  let index = 0;
  while (index < bodyLines.length) {
    const trimmed = bodyLines[index].trim();
    if (trimmed.startsWith('/**')) {
      const commentLines = [];
      while (index < bodyLines.length && !bodyLines[index].includes('*/')) {
        commentLines.push(bodyLines[index].replace(/^\s*\*\s?/u, '').replace(/^\s*\/\*\*?/u, ''));
        index += 1;
      }
      if (index < bodyLines.length) {
        commentLines.push(bodyLines[index].replace(/^\s*\*\//u, '').replace(/^\s*\*\s?/u, ''));
        index += 1;
      }
      docLines = commentLines;
      continue;
    }
    if (trimmed === '') {
      index += 1;
      continue;
    }
    if (trimmed.startsWith('readonly [') || trimmed.startsWith('[')) {
      docLines = [];
      index += 1;
      continue;
    }
    let declaration = trimmed;
    while (typeScriptBracketDepth(declaration) > 0 && index + 1 < bodyLines.length) {
      index += 1;
      declaration += ` ${bodyLines[index].trim()}`;
    }
    const member = parseTypeScriptClassMemberWithClass(declaration, docLines, className);
    if (member !== null) {
      members.push(member);
    }
    docLines = [];
    index += 1;
  }
  return members;
}

/** Parses top-level interface fields from a .d.ts interface body. */
function parseTypeScriptInterfaceFields(fieldLines) {
  const fields = [];
  let docLines = [];
  for (let index = 0; index < fieldLines.length; index += 1) {
    const trimmed = fieldLines[index].trim();
    if (trimmed.startsWith('/**')) {
      const commentLines = [];
      while (index < fieldLines.length && !fieldLines[index].includes('*/')) {
        commentLines.push(fieldLines[index].replace(/^\s*\*\s?/u, '').replace(/^\s*\/\*\*?/u, ''));
        index += 1;
      }
      if (index < fieldLines.length) {
        commentLines.push(fieldLines[index].replace(/^\s*\*\//u, '').replace(/^\s*\*\s?/u, ''));
      }
      docLines = commentLines;
      continue;
    }
    if (trimmed === '' || trimmed === '{' || trimmed === '}') {
      continue;
    }
    const match = trimmed.match(/^readonly\s+(\w+)(\?)?\s*:\s*([\s\S]+);?$/u);
    if (match === null) {
      continue;
    }
    let typeText = match[3].trim().replace(/;$/u, '');
    let depth = (typeText.match(/[<({[]/gu) ?? []).length - (typeText.match(/[>)}]/gu) ?? []).length;
    while (depth > 0 && index + 1 < fieldLines.length) {
      index += 1;
      typeText += ` ${fieldLines[index].trim()}`;
      depth += (fieldLines[index].match(/[<({[]/gu) ?? []).length - (fieldLines[index].match(/[>)}]/gu) ?? []).length;
    }
    fields.push({
      name: match[1],
      isOptional: match[2] === '?',
      typeText: typeText.replace(/;$/u, '').trim(),
      docLines,
    });
    docLines = [];
  }
  return fields;
}

// ---------------------------------------------------------------------------
// WebAssembly page
// ---------------------------------------------------------------------------

/** Renders the wasm generated header note. */
function renderWasmHeader() {
  return generatedPageNote('WebAssembly');
}

/** Renders one wasm class section. */
function renderWasmClass(classEntry) {
  const lines = [`## ${classEntry.name}`, ''];
  const markdown = docToMarkdown(classEntry.docLines);
  if (markdown !== '') {
    lines.push(markdown, '');
  }
  if (classEntry.constructors.length > 0) {
    lines.push(
      '**Construction**',
      '',
      renderTable(
        ['Constructor', 'Signature', 'Purpose'],
        classEntry.constructors.map((constructor) => [
          '`constructor`',
          `\`${renderWasmCallableSignature(constructor)}\``,
          docToTableCell(constructor.docLines),
        ]),
      ),
      '',
    );
  }
  if (classEntry.properties.length > 0) {
    lines.push(
      '**Properties**',
      '',
      renderTable(
        ['Property', 'Type', 'Purpose'],
        classEntry.properties.map((property) => [
          `\`${property.name}\``,
          `\`${property.returnDescription.type}\``,
          docToTableCell(property.docLines),
        ]),
      ),
      '',
    );
  }
  if (classEntry.methods.length > 0) {
    lines.push(
      '**Members**',
      '',
      renderTable(
        ['Member', 'Signature', 'Returns / errors', 'Purpose'],
        classEntry.methods.map((method) => [
          `\`${method.name}\``,
          `\`${renderWasmCallableSignature(method)}\``,
          describeWasmResult(method),
          docToTableCell(method.docLines),
        ]),
      ),
      '',
    );
  }
  if (
    classEntry.constructors.length === 0
    && classEntry.properties.length === 0
    && classEntry.methods.length === 0
  ) {
    lines.push('This class declares no members.', '');
  }
  return lines;
}

/** Renders a wasm callable signature. */
function renderWasmCallableSignature(callable) {
  const parameters = callable.parameters
    .map((parameter) => `${snakeToCamel(parameter.name)}: ${renderWasmType(parameter.typeText).type}`)
    .join(', ');
  if (callable.isConstructor) {
    return `constructor(${parameters})`;
  }
  const returnType = callable.returnDescription.type;
  const promiseReturn = callable.isAsync
    ? `Promise<${returnType === 'void' ? 'void' : returnType}>`
    : returnType;
  const staticPrefix = callable.isStatic ? 'static ' : '';
  return `${staticPrefix}${callable.isAsync ? 'async ' : ''}${callable.name}(${parameters}): ${promiseReturn}`;
}

/** Describes a wasm callable's return and error behavior. */
function describeWasmResult(callable) {
  const notes = [];
  if (callable.isAsync) {
    notes.push('Asynchronous.');
  }
  if (callable.returnDescription.type !== 'void') {
    notes.push(`Returns \`${callable.returnDescription.type}\`.`);
  }
  if (callable.returnDescription.throws) {
    notes.push('Throws the verbatim failure message as a string.');
  }
  return notes.length === 0 ? '—' : notes.join(' ');
}

/** Renders the free-function groups. */
function renderWasmFreeFunctions(freeFunctions) {
  const lines = [];
  const grouped = new Map();
  for (const functionEntry of freeFunctions) {
    const groupName =
      WASM_FUNCTION_SECTION_NAMES.get(functionEntry.fileName) ?? functionEntry.fileName;
    const group = grouped.get(groupName) ?? [];
    group.push(functionEntry);
    grouped.set(groupName, group);
  }
  for (const [groupName, entries] of grouped) {
    lines.push(`## ${groupName}`, '');
    lines.push(
      renderTable(
        ['Function', 'Signature', 'Returns / errors', 'Purpose'],
        entries.map((entry) => [
          `\`${entry.name}\``,
          `\`${renderWasmCallableSignature(entry)}\``,
          describeWasmResult(entry),
          docToTableCell(entry.docLines),
        ]),
      ),
      '',
    );
  }
  return lines;
}

/** Renders the wasm enums. */
function renderWasmEnums(enums) {
  const lines = ['## Enums', ''];
  for (const enumEntry of enums.values()) {
    lines.push(`### ${enumEntry.name}`, '');
    const markdown = docToMarkdown(enumEntry.docLines);
    if (markdown !== '') {
      lines.push(markdown, '');
    }
    lines.push(
      renderTable(
        ['Variant', 'Value'],
        enumEntry.variants.map((variant, variantIndex) => [
          `\`${variant.name}\``,
          `\`${variant.valueText ?? variantIndex}\``,
        ]),
      ),
      '',
    );
  }
  return lines;
}

/** Renders the wasm record shapes from the generated mirror. */
function renderWasmRecordShapes(declarations) {
  const lines = ['## Record shapes', ''];
  for (const interfaceDefinition of declarations.interfaces) {
    lines.push(`### ${interfaceDefinition.name}`, '');
    const markdown = docToMarkdown(interfaceDefinition.docLines);
    if (markdown !== '') {
      lines.push(markdown, '');
    }
    if (interfaceDefinition.fields.length === 0) {
      lines.push('This shape declares no fields.', '');
      continue;
    }
    lines.push(
      renderTable(
        ['Field', 'Type', 'Purpose'],
        interfaceDefinition.fields.map((field) => [
          `\`${field.name}${field.isOptional ? '?' : ''}\``,
          `\`${inlineText(field.typeText)}\``,
          docToTableCell(field.docLines),
        ]),
      ),
      '',
    );
  }
  return lines;
}

/** Renders the wasm union and alias shapes. */
function renderWasmTypeAliases(declarations) {
  const lines = ['## Union and alias shapes', ''];
  for (const alias of declarations.typeAliases) {
    lines.push(`### ${alias.name}`, '');
    const markdown = docToMarkdown(alias.docLines);
    if (markdown !== '') {
      lines.push(markdown, '');
    }
    lines.push(`\`${inlineText(alias.shape)}\``, '');
  }
  return lines;
}

/** Renders the wasm errors section from the contract's error interface. */
function renderWasmErrors(model) {
  const errorType = model.interfaces.find((interfaceDefinition) => interfaceDefinition.isError);
  const lines = ['## Errors', ''];
  lines.push(
    'The WebAssembly surface throws plain strings, not typed objects. A catch handler compares the message; the message is the verbatim reason, and the native surfaces carry the same reasons as typed error variants.',
    '',
  );
  if (errorType === undefined) {
    return lines;
  }
  lines.push(
    renderTable(
      ['Variant', 'Payload', 'Meaning'],
      errorType.errorVariants.map((variant) => [
        `\`${variant.name}\``,
        variant.parameters.length === 0
          ? 'None'
          : variant.parameters
              .map((parameter) => `${snakeToCamel(parameter.name)}: ${renderUdlType(parameter.type, 'typescript')}`)
              .join(', '),
        docToTableCell(variant.docLines),
      ]),
    ),
    '',
  );
  return lines;
}

/**
 * Renders the generated contract classes and their members.
 *
 * The page used to name the classes without their members; the generated
 * declaration carries every member, its parameter names and its return type,
 * so the reference states them rather than pointing at the file.
 */
function renderWasmContractClasses(classes) {
  const lines = [
    '## Contract classes',
    '',
    'These classes are generated from the frozen contract; the shipped `rustic_maps.d.ts` declares them. Each table names every member with its parameter names, its return type and what it does.',
    '',
  ];
  const byName = new Map(classes.map((entry) => [entry.name, entry]));
  for (const name of WASM_CONTRACT_CLASS_ORDER) {
    const entry = byName.get(name);
    if (entry === undefined) {
      continue;
    }
    const summary = firstDocParagraph(entry.docLines);
    lines.push(`### ${name}`, '');
    if (summary !== '') {
      lines.push(summary, '');
    }
    lines.push(
      renderTable(
        ['Member', 'Signature', 'Description'],
        entry.members.map((member) => [
          `\`${member.label}\``,
          `\`${member.signature}\``,
          member.description === '' ? '—' : member.description,
        ]),
      ),
      '',
    );
  }
  return lines;
}

/** Renders the WebAssembly API reference page. */
function renderWasmPage(model, wasmSurface, declarations) {
  const lines = [
    '# WebAssembly API reference',
    '',
    renderWasmHeader(),
    '',
    'The WebAssembly package gives a browser page a WebGPU map canvas (`BrowserMapView` and the canvas `MapCamera`), generated from the frozen contract alongside the provider, search, routing, navigation, positioning and diagnostics classes. There is no native view: you supply an `HTMLCanvasElement`, and the canvas owns the surface and the frame. Contract calls exchange plain `camelCase` records and reject with a typed `SdkError`.',
    '',
    '## Before you start',
    '',
    renderTable(
      ['Requirement', 'Value'],
      [
        ['Browser', 'WebGPU: Chromium 113 or newer, Safari 26 or newer, Firefox 141 or newer'],
        ['Package', '`@angoratek/rustic-maps-sdk-web`'],
        ['Module', '`index.js` and `rustic_maps.js` with their `.d.ts` declarations and `rustic_maps.wasm`'],
        ['Serving', 'A static server over HTTPS or `localhost`'],
        ['Provider credential', 'Passed at runtime; never bundled'],
      ],
    ),
    '',
  ];
  const orderedClasses = [];
  for (const className of WASM_CLASS_ORDER) {
    if (wasmSurface.classes.has(className)) {
      orderedClasses.push(wasmSurface.classes.get(className));
    }
  }
  for (const [className, classEntry] of wasmSurface.classes) {
    if (!WASM_CLASS_ORDER.includes(className)) {
      orderedClasses.push(classEntry);
    }
  }
  for (const classEntry of orderedClasses) {
    lines.push(...renderWasmClass(classEntry));
  }
  lines.push(...renderWasmFreeFunctions(wasmSurface.freeFunctions));
  lines.push(...renderWasmEnums(wasmSurface.enums));
  lines.push(...renderWasmContractClasses(declarations.classes));
  lines.push(...renderWasmRecordShapes(declarations));
  lines.push(...renderWasmTypeAliases(declarations));
  lines.push(...renderWasmErrors(model));
  lines.push(
    '## Async and threading',
    '',
    '- The wasm runtime is module-global; create one `MapSdk` root and one `BrowserMapView` per canvas.',
    '- Asynchronous calls return a `Promise` and reject with a typed `SdkError`.',
    '- Frames are explicit and serialized. A second overlapping `renderFrame` is a borrow violation; keep a busy flag.',
    '- `renderFrame` consumes its canvas `MapCamera` by value; build a fresh camera for each changed pose.',
    '- The navigation subscriptions and diagnostics delivery are synchronous on the calling thread; do not block.',
    '',
    '## Next steps',
    '',
    '- [Your first map](first-map.md) builds the same scenario step by step.',
    '- [Search and routing](search-and-routing.md) drives the live calls.',
    '- [Turn-by-turn navigation](navigation-simulation.md) drives the controller.',
    '- [Offline maps](offline-maps.md) routes from an in-memory offline graph.',
    '',
  );
  return joinPageLines(lines);
}

// ---------------------------------------------------------------------------
// Rust page
// ---------------------------------------------------------------------------

/** Reads leading `//!` module documentation from a Rust file. */
function readRustModuleDocs(filePath) {
  const lines = readFileSync(filePath, 'utf8').replace(/\r\n?/gu, '\n').split('\n');
  const docLines = [];
  for (const line of lines) {
    const trimmed = line.trim();
    if (trimmed.startsWith('//!')) {
      docLines.push(trimmed.replace(/^\/\/!\s?/u, ''));
    } else if (trimmed === '' && docLines.length > 0) {
      break;
    } else if (!trimmed.startsWith('//!')) {
      break;
    }
  }
  return docLines;
}

/** Recursively lists Rust source files below a directory, sorted. */
function listRustSources(directory) {
  const entries = readdirSync(directory).sort();
  const files = [];
  for (const entry of entries) {
    const entryPath = path.join(directory, entry);
    const stats = statSync(entryPath);
    if (stats.isDirectory()) {
      files.push(...listRustSources(entryPath));
    } else if (entry.endsWith('.rs')) {
      files.push(entryPath);
    }
  }
  return files;
}

/** True for a Rust file that is test-only or benchmark-only scaffolding. */
function isTestOnlyRustFile(filePath) {
  const fileName = path.basename(filePath);
  if (
    fileName === 'test_support.rs'
    || fileName === 'trace_fixture.rs'
    || fileName.endsWith('_tests.rs')
    || fileName === 'offline_routing_benchmark.rs'
  ) {
    return true;
  }
  const sourceText = readFileSync(filePath, 'utf8');
  return /^#!\[cfg\(test\)\]/mu.test(sourceText);
}

/**
 * Parses one Rust source file into its public item index. Only items at the
 * file's outer scope are indexed, so methods inside `impl` blocks and items
 * inside non-public modules do not appear twice.
 */
function parseRustItems(filePath) {
  const lines = readFileSync(filePath, 'utf8').replace(/\r\n?/gu, '\n').split('\n');
  const items = [];
  let docLines = [];
  let hiddenNextItem = false;
  let depth = 0;
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    const trimmed = line.trim();
    if (trimmed.startsWith('///')) {
      if (depth === 0) {
        docLines.push(trimmed.replace(/^\/\/\/\s?/u, ''));
      }
      continue;
    }
    if (trimmed.startsWith('//!')) {
      continue;
    }
    if (trimmed.startsWith('#[')) {
      if (depth === 0 && /doc\s*\(\s*hidden\s*\)/u.test(trimmed)) {
        hiddenNextItem = true;
      }
      continue;
    }
    if (trimmed === '' || trimmed.startsWith('//')) {
      continue;
    }
    if (depth > 0) {
      depth += countRustBraces(line).opens - countRustBraces(line).closes;
      continue;
    }
    const declarationMatch = trimmed.match(
      /^pub\s+(async\s+)?(unsafe\s+)?(fn|trait|struct|enum|type|const|mod)\s+(\w+)/u,
    );
    if (declarationMatch === null) {
      depth += countRustBraces(line).opens - countRustBraces(line).closes;
      docLines = [];
      continue;
    }
    if (hiddenNextItem) {
      hiddenNextItem = false;
      docLines = [];
      depth += countRustBraces(line).opens - countRustBraces(line).closes;
      continue;
    }
    const kind = declarationMatch[3];
    if (kind === 'mod' && /^pub\s+mod\s+\w+\s*;/u.test(trimmed)) {
      items.push({
        kind: 'module',
        name: declarationMatch[4],
        signature: `pub mod ${declarationMatch[4]}`,
        docLines,
      });
      docLines = [];
      continue;
    }
    if (kind === 'mod') {
      docLines = [];
      depth += countRustBraces(line).opens - countRustBraces(line).closes;
      continue;
    }
    let signature = trimmed;
    let signatureIndex = index;
    while (
      !/\{\s*$/u.test(signature)
      && !signature.trim().endsWith(';')
      && signatureIndex + 1 < lines.length
    ) {
      signatureIndex += 1;
      signature += ` ${lines[signatureIndex].trim()}`;
    }
    // Count the braces on the raw declaration: a struct, enum, trait or
    // function body opens here and must be skipped until its closing brace.
    const rawSignature = signature;
    signature = signature
      .replace(/\s*\{\s*$/u, '')
      .replace(/\s*;$/u, '')
      .replace(/\s+where\s+[\s\S]*$/u, '')
      .replace(/\s+/gu, ' ')
      .trim();
    if (kind === 'const') {
      signature = signature.replace(/\s*=\s*[\s\S]*$/u, '');
    }
    items.push({
      kind: kind === 'fn' ? 'function' : kind,
      name: declarationMatch[4],
      signature,
      docLines,
    });
    docLines = [];
    depth += countRustBraces(rawSignature).opens - countRustBraces(rawSignature).closes;
    index = signatureIndex;
  }
  return items;
}

/** Collects the public module and item index of the five Rust libraries. */
function collectRustSurface(repositoryRoot) {
  return RUST_CRATE_DIRECTORIES.map((crateDirectory) => {
    const sourceDirectory = path.join(repositoryRoot, 'crates', crateDirectory, 'src');
    const libraryPath = path.join(sourceDirectory, 'lib.rs');
    const crateDocLines = existsSync(libraryPath) ? readRustModuleDocs(libraryPath) : [];
    const files = listRustSources(sourceDirectory).filter(
      (filePath) => !isTestOnlyRustFile(filePath),
    );
    const modules = [];
    for (const filePath of files) {
      const relativePath = path.relative(sourceDirectory, filePath);
      const moduleName =
        relativePath === 'lib.rs'
          ? 'crate'
          : relativePath
              .replace(/\/mod\.rs$/u, '')
              .replace(/\.rs$/u, '')
              .replace(/[\\/]+/gu, '::');
      const items = parseRustItems(filePath);
      if (items.length === 0) {
        continue;
      }
      modules.push({
        name: moduleName,
        docLines: readRustModuleDocs(filePath),
        items,
      });
    }
    modules.sort((left, right) => {
      if (left.name === 'crate') {
        return -1;
      }
      if (right.name === 'crate') {
        return 1;
      }
      return left.name.localeCompare(right.name);
    });
    return {
      directory: crateDirectory,
      crateName: crateDirectory.replace(/-/gu, '_'),
      docLines: crateDocLines,
      modules,
    };
  });
}

/** Renders the Rust API reference page. */
function renderRustPage(rustSurface) {
  const lines = [
    '# Rust API reference',
    '',
    '> Generated from the SDK contract and the Rust crate sources. Do not edit this page by hand; change the sources and regenerate the documentation.',
    '',
    'The Rust libraries give you the map SDK engine without a platform view: the projection and camera math, the overlay and tile pipeline, provider contracts and their registry, offline routing over compiled archives, and the turn-by-turn navigation core. The same code backs the Android, iOS, React Native and WebAssembly surfaces. This page indexes the public modules and items of the five libraries and carries their documentation comments; the [generated Rust documentation](api/index.html) carries the full signatures and examples.',
    '',
    '## Libraries',
    '',
    renderTable(
      ['Crate', 'Role'],
      rustSurface.map((crate) => [
        `\`${crate.crateName}\``,
        docToTableCell(crate.docLines) === '—'
          ? '—'
          : inlineText(docToMarkdown(crate.docLines).split('\n\n')[0]),
      ]),
    ),
    '',
    '## Naming and conventions',
    '',
    '- Types are `PascalCase`, functions and methods are `snake_case`, and record fields are `snake_case`.',
    '- Records are plain `Debug + Clone + PartialEq` values; none carries a handle or a lock.',
    '- Provider traits are asynchronous and return `Result<_, SDKError>`; a provider never panics across the seam.',
    '- `MapCamera` and the camera transforms are pure values. Shared state lives behind `Arc` and short locks.',
    '- Listener callbacks run on the dispatching thread and must not block; re-entrant dispatch is suppressed by the single-in-flight guard.',
    '- The GPU renderers are native-only; the WebAssembly target uses the canvas renderer, which shares the camera, scheduler and overlay types.',
    '',
  ];
  for (const crate of rustSurface) {
    lines.push(`## ${crate.crateName}`, '');
    const markdown = docToMarkdown(crate.docLines);
    if (markdown !== '') {
      lines.push(markdown, '');
    }
    for (const module of crate.modules) {
      lines.push(`### Module \`${module.name}\``, '');
      const moduleMarkdown = docToMarkdown(module.docLines);
      if (moduleMarkdown !== '') {
        lines.push(moduleMarkdown, '');
      }
      lines.push(
        renderTable(
          ['Item', 'Kind', 'Signature', 'Purpose'],
          module.items.map((item) => [
            `\`${item.name}\``,
            item.kind,
            `\`${inlineText(item.signature)}\``,
            docToTableCell(item.docLines),
          ]),
        ),
        '',
      );
    }
  }
  lines.push(
    '## Next steps',
    '',
    '- [Your first map](first-map.md) builds the same scenario on a platform.',
    '- [How it works](how-it-works.md) explains the rendering and navigation pipelines.',
    '- [Performance and profiling](performance.md) covers the frame budget.',
    '',
  );
  return joinPageLines(lines);
}

// ---------------------------------------------------------------------------
// Generation, writing and checking
// ---------------------------------------------------------------------------

/** Generates every page in memory and answers a filename-to-content map. */
export function generateApiReferencePages(repositoryRoot = REPOSITORY_ROOT) {
  const udlPath = path.join(repositoryRoot, UDL_RELATIVE_PATH);
  if (!existsSync(udlPath)) {
    throw new Error(`the SDK contract is missing: ${UDL_RELATIVE_PATH}`);
  }
  const model = parseUdl(readFileSync(udlPath, 'utf8'));

  const kotlinFacts = collectKotlinFacts(
    path.join(repositoryRoot, KOTLIN_BINDINGS_RELATIVE_PATH),
  );
  if (!kotlinFacts.isPresent) {
    process.stderr.write(
      'note: generated Kotlin bindings are absent; the Android page derives its shapes from the contract.\n',
    );
  }
  const swiftFacts = collectSwiftFacts(
    path.join(repositoryRoot, SWIFT_BINDINGS_RELATIVE_PATH),
  );
  if (!swiftFacts.isPresent) {
    process.stderr.write(
      'note: generated Swift bindings are absent; the iOS page derives its shapes from the contract.\n',
    );
  }
  const reactNativeFacts = collectTypeScriptFacts(
    path.join(repositoryRoot, REACT_NATIVE_BINDINGS_RELATIVE_PATH),
  );
  if (!reactNativeFacts.isPresent) {
    process.stderr.write(
      'note: generated React Native bindings are absent; the React Native page derives its shapes from the contract.\n',
    );
  }

  // Use the generated error type name when the binding supplies one; the
  // derivation matches UniFFI's conventions when it does not.
  const androidConfiguration = PLATFORM_CONFIGURATIONS.android;
  if (kotlinFacts.errorTypeName !== null) {
    androidConfiguration.errorTypeName = kotlinFacts.errorTypeName;
  }
  const iosConfiguration = PLATFORM_CONFIGURATIONS.ios;
  if (swiftFacts.errorTypeName !== null) {
    iosConfiguration.errorTypeName = swiftFacts.errorTypeName;
  }
  const reactNativeConfiguration = PLATFORM_CONFIGURATIONS['react-native'];
  if (reactNativeFacts.errorTypeName !== null) {
    reactNativeConfiguration.errorTypeName = reactNativeFacts.errorTypeName;
  }

  const wasmPackageDeclarationsPath = path.join(
    repositoryRoot,
    WASM_PACKAGE_DECLARATIONS_RELATIVE_PATH,
  );
  if (!existsSync(wasmPackageDeclarationsPath)) {
    process.stderr.write(
      'note: the generated wasm declarations are absent; the WebAssembly page reads the export sources and the generated declaration mirror.\n',
    );
  }
  const wasmMirrorPath = path.join(repositoryRoot, WASM_CONTRACT_MIRROR_RELATIVE_PATH);
  if (!existsSync(wasmMirrorPath)) {
    throw new Error(`the WebAssembly declaration mirror is missing: ${WASM_CONTRACT_MIRROR_RELATIVE_PATH}`);
  }
  const wasmSurface = collectWasmSurface(repositoryRoot);
  const wasmDeclarations = parseTypeScriptDeclarations(readFileSync(wasmMirrorPath, 'utf8'));
  const rustSurface = collectRustSurface(repositoryRoot);

  return new Map([
    ['api-reference-rust.md', renderRustPage(rustSurface)],
    ['api-reference-android.md', renderNativePage('android', model, kotlinFacts)],
    ['api-reference-ios.md', renderNativePage('ios', model, swiftFacts)],
    [
      'api-reference-react-native.md',
      renderNativePage('react-native', model, reactNativeFacts),
    ],
    ['api-reference-wasm.md', renderWasmPage(model, wasmSurface, wasmDeclarations)],
  ]);
}

/** Writes the generated pages into an output directory. */
export function writeApiReferencePages(pages, outputDirectory) {
  mkdirSync(outputDirectory, { recursive: true });
  for (const fileName of OUTPUT_FILE_NAMES) {
    const content = pages.get(fileName);
    if (content === undefined) {
      throw new Error(`the generator did not produce ${fileName}`);
    }
    writeFileSync(path.join(outputDirectory, fileName), content);
  }
}

/** Summarizes the first difference between an expected and an actual file. */
function summarizeDifference(expectedText, actualText) {
  const expectedLines = expectedText.split('\n');
  const actualLines = actualText.split('\n');
  let firstDifference = 0;
  while (
    firstDifference < expectedLines.length
    && firstDifference < actualLines.length
    && expectedLines[firstDifference] === actualLines[firstDifference]
  ) {
    firstDifference += 1;
  }
  let expectedEnd = expectedLines.length - 1;
  let actualEnd = actualLines.length - 1;
  while (
    expectedEnd > firstDifference
    && actualEnd > firstDifference
    && expectedLines[expectedEnd] === actualLines[actualEnd]
  ) {
    expectedEnd -= 1;
    actualEnd -= 1;
  }
  return {
    firstDifferenceLine: firstDifference + 1,
    expectedLineCount: expectedEnd - firstDifference + 1,
    actualLineCount: actualEnd - firstDifference + 1,
    expectedExcerpt: expectedLines.slice(firstDifference, firstDifference + 2),
    actualExcerpt: actualLines.slice(firstDifference, firstDifference + 2),
  };
}

/**
 * Compares the generated pages with a directory. Answers the stale files and
 * whether any file differs or is missing.
 */
export function checkApiReferenceDirectory(pages, directory) {
  const staleFiles = [];
  for (const fileName of OUTPUT_FILE_NAMES) {
    const expectedContent = pages.get(fileName);
    const actualPath = path.join(directory, fileName);
    if (!existsSync(actualPath)) {
      staleFiles.push({ fileName, reason: 'missing', difference: null });
      continue;
    }
    const actualContent = readFileSync(actualPath, 'utf8');
    if (actualContent !== expectedContent) {
      staleFiles.push({
        fileName,
        reason: 'changed',
        difference: summarizeDifference(expectedContent, actualContent),
      });
    }
  }
  return { staleFiles, isStale: staleFiles.length > 0 };
}

/** Prints the CLI usage text. */
function printUsage() {
  process.stdout.write(
    'usage: node scripts/generate-api-reference.mjs [--output-dir DIR | --check DIR]\n',
  );
}

/** Parses the CLI arguments. */
function parseArguments(argv) {
  const options = { outputDirectory: DEFAULT_OUTPUT_DIRECTORY, checkDirectory: null };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === '--output-dir') {
      const value = argv[index + 1];
      if (value === undefined) {
        throw new Error('--output-dir needs a value');
      }
      options.outputDirectory = value;
      index += 1;
    } else if (argument === '--check') {
      const value = argv[index + 1];
      if (value === undefined) {
        throw new Error('--check needs a directory');
      }
      options.checkDirectory = value;
      index += 1;
    } else if (argument === '--help' || argument === '-h') {
      options.help = true;
    } else {
      throw new Error(`unknown option: ${argument}`);
    }
  }
  return options;
}

/** Runs the generator, writes or checks the pages, and answers an exit code. */
export function main(argv) {
  let options;
  try {
    options = parseArguments(argv);
  } catch (error) {
    process.stderr.write(`error: ${error.message}\n`);
    return 2;
  }
  if (options.help === true) {
    printUsage();
    return 0;
  }
  let pages;
  try {
    pages = generateApiReferencePages();
  } catch (error) {
    process.stderr.write(`error: ${error.message}\n`);
    return 1;
  }
  if (options.checkDirectory !== null) {
    const result = checkApiReferenceDirectory(pages, options.checkDirectory);
    if (result.isStale) {
      for (const staleFile of result.staleFiles) {
        if (staleFile.reason === 'missing') {
          process.stderr.write(`api-reference drift: ${staleFile.fileName} is missing\n`);
          continue;
        }
        const difference = staleFile.difference;
        process.stderr.write(
          `api-reference drift: ${staleFile.fileName} (first difference at line ${difference.firstDifferenceLine}; expected ${difference.expectedLineCount} line(s), found ${difference.actualLineCount})\n`,
        );
        if (difference.expectedExcerpt.length > 0) {
          process.stderr.write(`  expected: ${difference.expectedExcerpt[0]}\n`);
        }
        if (difference.actualExcerpt.length > 0) {
          process.stderr.write(`  found:    ${difference.actualExcerpt[0]}\n`);
        }
      }
      process.stderr.write(
        `api-reference drift: ${result.staleFiles.length} of ${OUTPUT_FILE_NAMES.length} page(s) are stale; run without --check to regenerate\n`,
      );
      return 1;
    }
    process.stdout.write(
      `api-reference check: ${OUTPUT_FILE_NAMES.length} page(s) match\n`,
    );
    return 0;
  }
  writeApiReferencePages(pages, options.outputDirectory);
  process.stdout.write(
    `api-reference pages written to ${path.relative(process.cwd(), options.outputDirectory) || '.'}\n`,
  );
  return 0;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main(process.argv.slice(2));
}
