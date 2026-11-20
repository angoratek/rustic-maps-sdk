// Style-layer coverage audit: which source layers in the committed vector-tile
// fixtures carry a `name` or `ref` (so a style ought to paint or label them),
// and which of those no committed style artifact paints?
//
// The audit reads committed data only; no provider, network or browser is
// involved. Two inputs:
//
//   1. Every vector-tile fixture under `fixtures/` and
//      `crates/rustic-maps-rendering/fixtures/` (`.pbf` and `.mvt`, excluding
//      the `glyphs/` ranges, which are font payloads rather than tiles). A
//      minimal Mapbox Vector Tile wire reader walks each layer's features and
//      marks the layer name-bearing when a feature carries `name`, `name:*`
//      or `name_*`, and ref-bearing when it carries `ref`, `ref:*` or
//      `ref_*`.
//
//   2. Every committed style artifact: the data-only vector style artifacts
//      under `crates/rustic-maps-rendering/fixtures/render/`, their generated
//      version-8 documents under `fixtures/render/styles/`, and the style
//      fixtures under `crates/rustic-maps-style/fixtures/styles/`. A layer's
//      `source_layer` (data-only artifacts) or `source-layer` (version-8
//      documents) is painted unless the layer's visibility is `none`.
//
// A name/ref-bearing source layer that no style artifact paints is a gap:
// either the style set should paint it, or the decision not to is recorded in
// `INTENTIONALLY_UNPAINTED_SOURCE_LAYERS` with a reason. The allow-list is
// stable: an entry that is no longer a name/ref-bearing gap fails the gate, so
// the list cannot rot into stale exemptions.
//
// Run directly (no server or browser involved):
//   node --test scripts/check-style-layer-coverage.mjs
// Print the current report instead:
//   node scripts/check-style-layer-coverage.mjs --report

import { readFileSync, readdirSync, statSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { test } from 'node:test';

const SCRIPT_PATH = fileURLToPath(import.meta.url);
const DEFAULT_REPOSITORY_ROOT = path.resolve(path.dirname(SCRIPT_PATH), '..');

/** The fixture roots the audit walks for committed vector tiles. */
const VECTOR_TILE_FIXTURE_ROOTS = [
  'fixtures',
  'crates/rustic-maps-rendering/fixtures',
];

/** The extensions a committed vector-tile fixture may carry. */
const VECTOR_TILE_EXTENSIONS = new Set(['.pbf', '.mvt']);

/** The fixture directories that hold payloads this audit must not read. */
const SKIPPED_FIXTURE_DIRECTORY_NAMES = new Set([
  // Glyph range payloads share the `.pbf` extension but are font data.
  'glyphs',
]);

/** The directories holding committed style artifacts. */
const STYLE_ARTIFACT_DIRECTORIES = [
  'crates/rustic-maps-rendering/fixtures/render/styles',
  'crates/rustic-maps-style/fixtures/styles',
];

/** The data-only style artifacts under this directory. */
const DATA_ONLY_ARTIFACT_DIRECTORY = 'crates/rustic-maps-rendering/fixtures/render';
const DATA_ONLY_ARTIFACT_FILE_PATTERN = /^vector-style.*\.json$/;

/**
 * Provider style documents committed under the shared fixture tree: neutral
 * authored documents carrying each vendor family's source-layer names, so the
 * audit checks the schemas a provider base actually paints.
 */
const PROVIDER_STYLE_DOCUMENT_PATHS = [
  'fixtures/here/vendor_style_document.json',
  'fixtures/mapbox/vendor_style_document.json',
];

/**
 * Source layers whose name/ref-bearing features are intentionally not painted
 * by any committed style artifact. Each entry carries the decision's reason;
 * `run_style_layer_coverage_checks` fails when an entry stops being a gap.
 */
const INTENTIONALLY_UNPAINTED_SOURCE_LAYERS = new Map([]);

// --- Protobuf wire reader ---------------------------------------------------

/**
 * Reads one protobuf varint.
 *
 * @param {Buffer} bytes
 * @param {number} offset
 * @returns {[number, number]} the value and the next offset
 */
function readVarint(bytes, offset) {
  let value = 0;
  let shift = 0;
  let cursor = offset;
  while (cursor < bytes.length) {
    const byte = bytes[cursor];
    cursor += 1;
    value += (byte & 0x7f) * 2 ** shift;
    if ((byte & 0x80) === 0) {
      return [value, cursor];
    }
    shift += 7;
    if (shift > 63) break;
  }
  throw new Error(`malformed varint at byte ${offset}`);
}

/**
 * Yields every top-level protobuf field. Only the wire types a vector tile
 * uses are accepted; a group encoding is malformed for this audit.
 *
 * @param {Buffer} bytes
 */
function* protobufFields(bytes) {
  let offset = 0;
  while (offset < bytes.length) {
    const [key, after_key] = readVarint(bytes, offset);
    offset = after_key;
    const field = Math.floor(key / 8);
    const wire = key % 8;
    if (wire === 0) {
      const [value, after_value] = readVarint(bytes, offset);
      offset = after_value;
      yield { field, wire, value };
    } else if (wire === 1) {
      if (offset + 8 > bytes.length) throw new Error('truncated fixed64 field');
      yield { field, wire, value: bytes.subarray(offset, offset + 8) };
      offset += 8;
    } else if (wire === 2) {
      const [length, after_length] = readVarint(bytes, offset);
      offset = after_length;
      if (offset + length > bytes.length) throw new Error('truncated length-delimited field');
      yield { field, wire, value: bytes.subarray(offset, offset + length) };
      offset += length;
    } else if (wire === 5) {
      if (offset + 4 > bytes.length) throw new Error('truncated fixed32 field');
      yield { field, wire, value: bytes.subarray(offset, offset + 4) };
      offset += 4;
    } else {
      throw new Error(`unsupported protobuf wire type ${wire}`);
    }
  }
}

/**
 * Returns a feature's packed tag indices: key index, value index, repeating.
 *
 * @param {Buffer} feature_bytes
 * @returns {number[]}
 */
function featureTagIndices(feature_bytes) {
  const tags = [];
  for (const field of protobufFields(feature_bytes)) {
    if (field.field !== 2 || field.wire !== 2) continue;
    let offset = 0;
    while (offset < field.value.length) {
      const [tag, next_offset] = readVarint(field.value, offset);
      tags.push(tag);
      offset = next_offset;
    }
  }
  if (tags.length % 2 !== 0) {
    throw new Error('a vector-tile feature has an odd number of tag indices');
  }
  return tags;
}

/** Whether one attribute name is a primary or localized name. */
function isNameAttribute(attribute_name) {
  return attribute_name === 'name'
    || attribute_name.startsWith('name:')
    || attribute_name.startsWith('name_');
}

/** Whether one attribute name is a primary or qualified reference. */
function isRefAttribute(attribute_name) {
  return attribute_name === 'ref'
    || attribute_name.startsWith('ref:')
    || attribute_name.startsWith('ref_');
}

/**
 * Decodes one tile layer's name, its keys and the key indices its features
 * reference. Values and geometry are not needed for the audit.
 *
 * @param {Buffer} layer_bytes
 * @param {string} fixture_path
 */
function decodeTileLayer(layer_bytes, fixture_path) {
  let layer_name = null;
  const keys = [];
  const feature_buffers = [];
  for (const field of protobufFields(layer_bytes)) {
    if (field.field === 1 && field.wire === 2) {
      layer_name = field.value.toString('utf8');
    } else if (field.field === 2 && field.wire === 2) {
      feature_buffers.push(field.value);
    } else if (field.field === 3 && field.wire === 2) {
      keys.push(field.value.toString('utf8'));
    }
  }
  if (layer_name === null) {
    throw new Error(`${fixture_path}: a tile layer carries no name`);
  }
  const referenced_attributes = new Set();
  for (const feature_buffer of feature_buffers) {
    const tags = featureTagIndices(feature_buffer);
    for (let index = 0; index < tags.length; index += 2) {
      const attribute_name = keys[tags[index]];
      if (attribute_name === undefined) {
        throw new Error(
          `${fixture_path}: layer ${layer_name} names key index ${tags[index]} `
            + `but declares only ${keys.length} keys`,
        );
      }
      referenced_attributes.add(attribute_name);
    }
  }
  return { layer_name, referenced_attributes, feature_count: feature_buffers.length };
}

/**
 * Reads every layer of one Mapbox Vector Tile.
 *
 * @param {Buffer} tile_bytes
 * @param {string} fixture_path
 * @returns {{layer_name: string, referenced_attributes: Set<string>, feature_count: number}[]}
 */
function decodeVectorTile(tile_bytes, fixture_path) {
  const layers = [];
  for (const field of protobufFields(tile_bytes)) {
    if (field.field === 3 && field.wire === 2) {
      layers.push(decodeTileLayer(field.value, fixture_path));
    }
  }
  return layers;
}

// --- Repository walks -------------------------------------------------------

/**
 * Walks a directory recursively and returns every matching file, sorted.
 *
 * @param {string} absolute_directory
 * @param {(file_path: string) => boolean} matches
 * @returns {string[]}
 */
function collectFiles(absolute_directory, matches) {
  const collected = [];
  const pending = [absolute_directory];
  while (pending.length > 0) {
    const directory = pending.pop();
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const entry_path = path.join(directory, entry.name);
      if (entry.isDirectory()) {
        if (!SKIPPED_FIXTURE_DIRECTORY_NAMES.has(entry.name)) pending.push(entry_path);
      } else if (entry.isFile() && matches(entry_path)) {
        collected.push(entry_path);
      }
    }
  }
  return collected.sort();
}

/** Returns every committed vector-tile fixture path, repository-relative. */
function vectorTileFixturePaths(repository_root) {
  const fixture_paths = [];
  for (const relative_root of VECTOR_TILE_FIXTURE_ROOTS) {
    const absolute_root = path.join(repository_root, relative_root);
    if (!statSync(absolute_root, { throwIfNoEntry: false })?.isDirectory()) continue;
    for (const absolute_path of collectFiles(
      absolute_root,
      (file_path) => VECTOR_TILE_EXTENSIONS.has(path.extname(file_path)),
    )) {
      fixture_paths.push(path.relative(repository_root, absolute_path).split(path.sep).join('/'));
    }
  }
  return [...new Set(fixture_paths)].sort();
}

/** Returns every committed style artifact path, repository-relative. */
function styleArtifactPaths(repository_root) {
  const artifact_paths = [];
  const data_only_directory = path.join(repository_root, DATA_ONLY_ARTIFACT_DIRECTORY);
  for (const entry of readdirSync(data_only_directory, { withFileTypes: true })) {
    if (entry.isFile() && DATA_ONLY_ARTIFACT_FILE_PATTERN.test(entry.name)) {
      artifact_paths.push(`${DATA_ONLY_ARTIFACT_DIRECTORY}/${entry.name}`);
    }
  }
  for (const relative_directory of STYLE_ARTIFACT_DIRECTORIES) {
    const absolute_directory = path.join(repository_root, relative_directory);
    for (const entry of readdirSync(absolute_directory, { withFileTypes: true })) {
      if (entry.isFile() && entry.name.endsWith('.json')) {
        artifact_paths.push(`${relative_directory}/${entry.name}`);
      }
    }
  }
  artifact_paths.push(...PROVIDER_STYLE_DOCUMENT_PATHS);
  return [...new Set(artifact_paths)].sort();
}

// --- The audit --------------------------------------------------------------

/**
 * Collects the audit's data: the name/ref-bearing source layers and the source
 * layers the committed style artifacts paint.
 *
 * @param {string} repository_root
 */
function collectCoverage(repository_root) {
  const fixture_paths = vectorTileFixturePaths(repository_root);
  if (fixture_paths.length === 0) {
    throw new Error('the audit found no committed vector-tile fixtures');
  }
  /** @type {Map<string, {fixtures: Set<string>, name_bearing: boolean, ref_bearing: boolean, attributes: Set<string>}>} */
  const purposeful_layers = new Map();
  for (const fixture_path of fixture_paths) {
    const tile_bytes = readFileSync(path.join(repository_root, fixture_path));
    const layers = decodeVectorTile(tile_bytes, fixture_path);
    if (layers.length === 0) {
      throw new Error(`${fixture_path}: decoded no tile layers`);
    }
    for (const layer of layers) {
      const name_bearing = [...layer.referenced_attributes].some(isNameAttribute);
      const ref_bearing = [...layer.referenced_attributes].some(isRefAttribute);
      if (!name_bearing && !ref_bearing) continue;
      const existing = purposeful_layers.get(layer.layer_name) ?? {
        fixtures: new Set(),
        name_bearing: false,
        ref_bearing: false,
        attributes: new Set(),
      };
      existing.fixtures.add(fixture_path);
      existing.name_bearing ||= name_bearing;
      existing.ref_bearing ||= ref_bearing;
      for (const attribute_name of layer.referenced_attributes) {
        if (isNameAttribute(attribute_name) || isRefAttribute(attribute_name)) {
          existing.attributes.add(attribute_name);
        }
      }
      purposeful_layers.set(layer.layer_name, existing);
    }
  }
  if (purposeful_layers.size === 0) {
    throw new Error('the audit found no name/ref-bearing source layers; the check would be vacuous');
  }

  const painted_layers = new Set();
  const artifact_paths = styleArtifactPaths(repository_root);
  for (const artifact_path of artifact_paths) {
    const artifact = JSON.parse(readFileSync(path.join(repository_root, artifact_path), 'utf8'));
    if (!Array.isArray(artifact.layers)) continue;
    for (const layer of artifact.layers) {
      const source_layer = layer['source-layer'] ?? layer.source_layer;
      if (typeof source_layer !== 'string' || source_layer.length === 0) continue;
      if (layer.layout?.visibility === 'none') continue;
      painted_layers.add(source_layer);
    }
  }

  const gaps = [];
  const unexpected_gaps = [];
  for (const [source_layer, purpose] of [...purposeful_layers].sort(([first], [second]) => (
    first.localeCompare(second)
  ))) {
    if (painted_layers.has(source_layer)) continue;
    const gap = {
      source_layer,
      fixtures: [...purpose.fixtures].sort(),
      attributes: [...purpose.attributes].sort(),
      name_bearing: purpose.name_bearing,
      ref_bearing: purpose.ref_bearing,
    };
    gaps.push(gap);
    if (!INTENTIONALLY_UNPAINTED_SOURCE_LAYERS.has(source_layer)) {
      unexpected_gaps.push(gap);
    }
  }

  const stale_allow_list_entries = [];
  for (const [source_layer, entry] of INTENTIONALLY_UNPAINTED_SOURCE_LAYERS) {
    if (!purposeful_layers.has(source_layer)) {
      stale_allow_list_entries.push({
        source_layer,
        reason: 'no committed fixture carries it as a name/ref-bearing layer',
      });
    } else if (painted_layers.has(source_layer)) {
      stale_allow_list_entries.push({
        source_layer,
        reason: 'a committed style artifact now paints it',
      });
    } else if (typeof entry.reason !== 'string' || entry.reason.trim().length === 0) {
      stale_allow_list_entries.push({
        source_layer,
        reason: 'the allow-list entry carries no documented reason',
      });
    }
  }

  return {
    fixture_count: fixture_paths.length,
    artifact_count: artifact_paths.length,
    purposeful_layers,
    painted_layers,
    gaps,
    unexpected_gaps,
    stale_allow_list_entries,
  };
}

/** Formats one source-layer row for the human report. */
function formatCoverageRow(gap) {
  const attributes = gap.attributes.join(', ');
  const fixtures = gap.fixtures.join(', ');
  const status = INTENTIONALLY_UNPAINTED_SOURCE_LAYERS.has(gap.source_layer)
    ? 'intentionally unpainted'
    : 'UNEXPECTED';
  return `  ${gap.source_layer.padEnd(20)} ${status.padEnd(24)} ${attributes}\n`
    + `  ${' '.repeat(20)} ${fixtures}`;
}

/**
 * Runs the audit and throws with every finding when the committed styles do
 * not cover the name/ref-bearing source layers (or the allow-list is stale).
 *
 * @param {string} [repository_root]
 */
export function run_style_layer_coverage_checks(repository_root = DEFAULT_REPOSITORY_ROOT) {
  const coverage = collectCoverage(repository_root);
  const failures = [];
  for (const gap of coverage.unexpected_gaps) {
    failures.push(
      `source layer ${gap.source_layer} carries ${gap.attributes.join(', ')} in `
        + `${gap.fixtures.join(', ')} but no committed style artifact paints it`,
    );
  }
  for (const stale_entry of coverage.stale_allow_list_entries) {
    failures.push(
      `allow-list entry ${stale_entry.source_layer} is stale: ${stale_entry.reason}`,
    );
  }
  if (failures.length > 0) {
    throw new Error(
      `style layer coverage failed:\n${failures.map((failure) => `  - ${failure}`).join('\n')}\n`
        + `\nCurrent gaps:\n${coverage.gaps.map(formatCoverageRow).join('\n')}`,
    );
  }
  return coverage;
}

/**
 * Prints the current coverage table: every name/ref-bearing source layer, its
 * fixture and attributes, and whether a style paints it.
 *
 * @param {string} [repository_root]
 */
export function printStyleLayerCoverageReport(repository_root = DEFAULT_REPOSITORY_ROOT) {
  const coverage = collectCoverage(repository_root);
  const planned = [...coverage.purposeful_layers.keys()].sort();
  console.log(
    `Style layer coverage: ${coverage.fixture_count} vector-tile fixtures, `
      + `${planned.length} name/ref-bearing source layers, `
      + `${planned.filter((layer) => coverage.painted_layers.has(layer)).length} painted, `
      + `${coverage.gaps.length} unpainted.`,
  );
  console.log(`\n${'source layer'.padEnd(20)} ${'painted'.padEnd(10)} attributes`);
  console.log('-'.repeat(78));
  for (const source_layer of planned) {
    const purpose = coverage.purposeful_layers.get(source_layer);
    const painted = coverage.painted_layers.has(source_layer) ? 'yes' : 'no';
    const allow_listed = INTENTIONALLY_UNPAINTED_SOURCE_LAYERS.has(source_layer)
      ? ' (allow-listed)'
      : '';
    console.log(
      `${source_layer.padEnd(20)} ${(painted + allow_listed).padEnd(10)} `
        + `${[...purpose.attributes].sort().join(', ')}`,
    );
    console.log(`  fixtures: ${[...purpose.fixtures].sort().join(', ')}`);
    if (INTENTIONALLY_UNPAINTED_SOURCE_LAYERS.has(source_layer)) {
      console.log(`  reason: ${INTENTIONALLY_UNPAINTED_SOURCE_LAYERS.get(source_layer).reason}`);
    }
  }
  if (coverage.stale_allow_list_entries.length > 0) {
    console.log('\nStale allow-list entries:');
    for (const stale_entry of coverage.stale_allow_list_entries) {
      console.log(`  ${stale_entry.source_layer}: ${stale_entry.reason}`);
    }
  }
}

/** The `node --test` suite name of the coverage assertions. */
const STYLE_LAYER_COVERAGE_TEST_NAME =
  'every name/ref-bearing source layer is painted or documented as intentionally unpainted';

/** Runs the suite only when this file is the entry point. */
const RUNNING_AS_ENTRY = process.argv[1] !== undefined
  && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url;

if (RUNNING_AS_ENTRY) {
  if (process.argv.includes('--report')) {
    printStyleLayerCoverageReport();
  } else {
    test(STYLE_LAYER_COVERAGE_TEST_NAME, () => {
      run_style_layer_coverage_checks();
    });
  }
}
