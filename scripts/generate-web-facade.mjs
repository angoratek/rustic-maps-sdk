#!/usr/bin/env node
// Generates the WebAssembly façade's contract surface from the frozen UniFFI
// contract, and checks that the committed output has not drifted.
//
// The wasm seam has no UniFFI code generation: the Rust exports are written
// by hand and the TypeScript mirror is a curated view of the contract. This
// generator owns the parts that are mechanical:
//
//   - `platform/web/types/rustic_maps.d.ts` — the contract records, enums,
//     error union and error-code union the WebAssembly surface exchanges,
//     with the contract's own documentation comments. The hand-written
//     projections the UDL cannot express are appended verbatim from
//     `platform/web/types/rustic_maps_surface.d.ts`.
//   - `crates/rustic-maps-web-bindings/src/generated_contract_field_names.rs`
//     — the JavaScript field names of every contract record field the wasm
//     seam reads or writes.
//
// The canvas class and the page-specific helpers stay hand-written: a DOM
// canvas and a browser event bridge are not expressible in UniFFI.
//
// USAGE
//   node scripts/generate-web-facade.mjs
//   node scripts/generate-web-facade.mjs --check
//
// DETERMINISM
//   Declarations keep the order of `FACADE_DECLARATIONS` and fields keep the
//   contract's declaration order. The generated Rust constants are sorted by
//   constant name. The output carries no timestamps, host paths or absolute
//   paths.

import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPOSITORY_ROOT = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);

/** The contract the façade mirrors. */
export const UNIFFI_CONTRACT_RELATIVE_PATH =
  'crates/rustic-maps-native-core/src/rustic_maps.udl';

/** The generated TypeScript contract mirror. */
export const GENERATED_TYPESCRIPT_RELATIVE_PATH =
  'platform/web/types/rustic_maps.d.ts';

/** The hand-written WebAssembly projections the generator appends verbatim. */
export const TYPESCRIPT_SURFACE_RELATIVE_PATH =
  'platform/web/types/rustic_maps_surface.d.ts';

/** The generated Rust field-name constants. */
export const GENERATED_RUST_RELATIVE_PATH =
  'crates/rustic-maps-web-bindings/src/generated_contract_field_names.rs';

// --- Contract parser --------------------------------------------------------

/** Opens a UniFFI `dictionary` block. */
const DICTIONARY_OPEN_PATTERN = /^\s*dictionary\s+([A-Za-z0-9_]+)\s*\{/;
/** Opens a UniFFI `enum` block. */
const ENUM_OPEN_PATTERN = /^\s*enum\s+([A-Za-z0-9_]+)\s*\{/;
/** Opens an `interface` block, plain or carrying attributes (`[Error]`, `[Trait, WithForeign]`). */
const INTERFACE_OPEN_PATTERN =
  /^\s*(?:\[[^\]]*\]\s*)*interface\s+([A-Za-z0-9_]+)\s*\{/;
/** Opens the single UniFFI `namespace` block. */
const NAMESPACE_OPEN_PATTERN = /^\s*namespace\s+rustic_maps\s*\{/;
/** Closes any UniFFI block. */
const BLOCK_CLOSE_PATTERN = /^\s*\}\s*;/;
/** One quoted `enum` variant. */
const ENUM_VARIANT_PATTERN = /^\s*"([A-Za-z0-9_]+)"\s*,?\s*$/;
/**
 * A `sequence<...>` dictionary field, with the optional `?` marker and an
 * optional default value (`= ...`) after the field name.
 */
const SEQUENCE_FIELD_PATTERN =
  /^\s*sequence<([A-Za-z0-9_]+)>(\?)?\s+([a-z][0-9a-z_]*)(?:\s*=\s*[^;]+)?;/;
/** A scalar or record-typed dictionary field, with the optional `?` marker and default. */
const PLAIN_FIELD_PATTERN =
  /^\s*([A-Za-z0-9_]+)(\?)?\s+([a-z][0-9a-z_]*)(?:\s*=\s*[^;]+)?;/;
/**
 * An interface method or `[Error]` variant: an optional leading return type,
 * then the member name before the first `(`.
 */
const MEMBER_PATTERN =
  /^\s*(?:[A-Za-z0-9_]+(?:<[^>]+>)*\??\s+)?([A-Za-z0-9_]+)\s*\(/;
/** A `Type name` parameter inside a member or variant declaration. */
const PARAMETER_PATTERN = /^\s*(.+?)\s+([a-z][0-9a-z_]*)\s*$/;

/** Comment lines (`///`) and attribute lines (`[Throws=...]`) carry no code. */
const DOC_LINE_PATTERN = /^\s*\/\/\//;
const ATTRIBUTE_LINE_PATTERN = /^\s*\[/;
const NON_DOC_COMMENT_PATTERN = /^\s*\/\//;

/** The UniFFI constructor member; not a named method of the surface. */
const UNIFFI_CONSTRUCTOR_MEMBER_NAME = 'constructor';

/**
 * One dictionary field of the contract.
 * @typedef {object} ContractField
 * @property {string} name Snake-case field name from the contract.
 * @property {string} typeName UniFFI type annotation of the field.
 * @property {boolean} isOptional Whether the field carries the `?` marker.
 * @property {string | null} sequenceElementType Element type of a `sequence<>`.
 * @property {string[]} docLines The contract's `///` documentation lines.
 */

/**
 * One interface member of the contract.
 * @typedef {object} ContractMember
 * @property {string} name Member or variant name.
 * @property {'constructor' | 'method' | 'variant'} kind Member kind.
 * @property {{ typeName: string, name: string }[]} parameters Declared parameters.
 * @property {string[]} docLines The contract's `///` documentation lines.
 */

/**
 * @typedef {object} ParsedContract
 * @property {Map<string, {name: string, docLines: string[], fields: ContractField[]}>} dictionaryDefinitions
 * @property {Map<string, {name: string, docLines: string[], variants: {name: string, docLines: string[]}[]}>} enumDefinitions
 * @property {Map<string, {name: string, docLines: string[], isError: boolean, members: ContractMember[]}>} interfaceDefinitions
 * @property {{name: string, docLines: string[], members: ContractMember[]}} namespace
 * @property {Map<string, ContractField[]>} dictionaries Dictionary name to its fields.
 * @property {Map<string, string[]>} enums Enum name to its quoted variant names.
 * @property {Map<string, string[]>} interface_members Interface name to its member names.
 * @property {string[]} namespace_members Free function names of the namespace.
 */

/** Splits a declaration's parameter list on top-level commas. */
function split_parameters(parameter_text) {
  if (parameter_text.trim() === '') {
    return [];
  }
  return parameter_text.split(',').map((parameter) => parameter.trim());
}

/** Parses a member declaration's parameter list into typed parameters. */
function parse_parameters(declaration_line) {
  const open_index = declaration_line.indexOf('(');
  const close_index = declaration_line.lastIndexOf(')');
  if (open_index < 0 || close_index <= open_index) {
    return [];
  }
  const parameters = [];
  for (const parameter_text of split_parameters(
    declaration_line.slice(open_index + 1, close_index),
  )) {
    const match = PARAMETER_PATTERN.exec(parameter_text);
    if (match === null) {
      throw new Error(
        `could not parse the contract parameter "${parameter_text}"`,
      );
    }
    parameters.push({ typeName: match[1], name: match[2] });
  }
  return parameters;
}

/**
 * Parses the frozen UniFFI contract into the contract records the generator
 * reads, keeping each block's, field's, variant's and member's `///`
 * documentation.
 * @param {string} contract_text The raw UDL source.
 * @returns {ParsedContract}
 */
export function parseUniffiContract(contract_text) {
  /** @type {Map<string, {name: string, docLines: string[], fields: ContractField[]}>} */
  const dictionaryDefinitions = new Map();
  /** @type {Map<string, {name: string, docLines: string[], variants: {name: string, docLines: string[]}[]}>} */
  const enumDefinitions = new Map();
  /** @type {Map<string, {name: string, docLines: string[], isError: boolean, members: ContractMember[]}>} */
  const interfaceDefinitions = new Map();
  /** @type {{name: string, docLines: string[], members: ContractMember[]}} */
  const namespace = { name: 'rustic_maps', docLines: [], members: [] };

  /** @type {string[]} */
  let pending_doc_lines = [];
  /** @type {null | {kind: 'dictionary' | 'enum' | 'interface' | 'namespace', name: string, docLines: string[], isError: boolean, fields: ContractField[], variants: {name: string, docLines: string[]}[], members: ContractMember[]}} */
  let block = null;

  const take_doc_lines = () => {
    const doc_lines = pending_doc_lines;
    pending_doc_lines = [];
    return doc_lines;
  };

  for (const line of contract_text.split('\n')) {
    if (block !== null && BLOCK_CLOSE_PATTERN.test(line)) {
      if (block.kind === 'dictionary') {
        dictionaryDefinitions.set(block.name, {
          name: block.name,
          docLines: block.docLines,
          fields: block.fields,
        });
      } else if (block.kind === 'enum') {
        enumDefinitions.set(block.name, {
          name: block.name,
          docLines: block.docLines,
          variants: block.variants,
        });
      } else if (block.kind === 'interface') {
        interfaceDefinitions.set(block.name, {
          name: block.name,
          docLines: block.docLines,
          isError: block.isError,
          members: block.members,
        });
      } else {
        namespace.docLines = block.docLines;
        namespace.members = block.members;
      }
      block = null;
      take_doc_lines();
      continue;
    }

    if (DOC_LINE_PATTERN.test(line)) {
      pending_doc_lines.push(line.trim().replace(/^\/\/\/\s?/, ''));
      continue;
    }
    if (line.trim() === '') {
      pending_doc_lines = [];
      continue;
    }
    if (NON_DOC_COMMENT_PATTERN.test(line)) {
      pending_doc_lines = [];
      continue;
    }

    if (block === null) {
      const dictionary_open = DICTIONARY_OPEN_PATTERN.exec(line);
      if (dictionary_open !== null) {
        block = {
          kind: 'dictionary',
          name: dictionary_open[1],
          docLines: take_doc_lines(),
          isError: false,
          fields: [],
          variants: [],
          members: [],
        };
        continue;
      }
      const enum_open = ENUM_OPEN_PATTERN.exec(line);
      if (enum_open !== null) {
        block = {
          kind: 'enum',
          name: enum_open[1],
          docLines: take_doc_lines(),
          isError: false,
          fields: [],
          variants: [],
          members: [],
        };
        continue;
      }
      const interface_open = INTERFACE_OPEN_PATTERN.exec(line);
      if (interface_open !== null) {
        block = {
          kind: 'interface',
          name: interface_open[1],
          docLines: take_doc_lines(),
          isError: /\[Error\]/.test(line),
          fields: [],
          variants: [],
          members: [],
        };
        continue;
      }
      const namespace_open = NAMESPACE_OPEN_PATTERN.exec(line);
      if (namespace_open !== null) {
        block = {
          kind: 'namespace',
          name: 'rustic_maps',
          docLines: take_doc_lines(),
          isError: false,
          fields: [],
          variants: [],
          members: [],
        };
        continue;
      }
      // A line outside every block carries no member; drop any stale doc.
      pending_doc_lines = [];
      continue;
    }

    // An attribute belongs to the declaration that follows it, so the doc
    // lines read so far stay pending.
    if (ATTRIBUTE_LINE_PATTERN.test(line)) {
      continue;
    }

    if (block.kind === 'dictionary') {
      const sequence_field = SEQUENCE_FIELD_PATTERN.exec(line);
      if (sequence_field !== null) {
        block.fields.push({
          name: sequence_field[3],
          typeName: `sequence<${sequence_field[1]}>`,
          isOptional: sequence_field[2] === '?',
          sequenceElementType: sequence_field[1],
          docLines: take_doc_lines(),
        });
        continue;
      }
      const plain_field = PLAIN_FIELD_PATTERN.exec(line);
      if (plain_field !== null) {
        block.fields.push({
          name: plain_field[3],
          typeName: plain_field[1],
          isOptional: plain_field[2] === '?',
          sequenceElementType: null,
          docLines: take_doc_lines(),
        });
        continue;
      }
      pending_doc_lines = [];
      continue;
    }

    if (block.kind === 'enum') {
      const enum_variant = ENUM_VARIANT_PATTERN.exec(line);
      if (enum_variant !== null) {
        block.variants.push({
          name: enum_variant[1],
          docLines: take_doc_lines(),
        });
        continue;
      }
      pending_doc_lines = [];
      continue;
    }

    const member_match = MEMBER_PATTERN.exec(line);
    if (member_match === null) {
      pending_doc_lines = [];
      continue;
    }
    const member_name = member_match[1];
    block.members.push({
      name: member_name,
      kind: member_name === UNIFFI_CONSTRUCTOR_MEMBER_NAME
        ? 'constructor'
        : block.kind === 'interface' && block.isError
          ? 'variant'
          : 'method',
      parameters: parse_parameters(line),
      docLines: take_doc_lines(),
    });
  }

  /** @type {Map<string, ContractField[]>} */
  const dictionaries = new Map();
  for (const [name, definition] of dictionaryDefinitions) {
    dictionaries.set(name, definition.fields);
  }
  /** @type {Map<string, string[]>} */
  const enums = new Map();
  for (const [name, definition] of enumDefinitions) {
    enums.set(name, definition.variants.map((variant) => variant.name));
  }
  /** @type {Map<string, string[]>} */
  const interface_members = new Map();
  for (const [name, definition] of interfaceDefinitions) {
    interface_members.set(
      name,
      definition.members
        .filter((member) => member.name !== UNIFFI_CONSTRUCTOR_MEMBER_NAME)
        .map((member) => member.name),
    );
  }

  return {
    dictionaryDefinitions,
    enumDefinitions,
    interfaceDefinitions,
    namespace,
    dictionaries,
    enums,
    interface_members,
    namespace_members: namespace.members.map((member) => member.name),
  };
}

// --- Facade model -----------------------------------------------------------

/**
 * The contract records the WebAssembly façade mirrors, in declaration order.
 * `surfaceName` renames a projection; `enumFieldsAsText` reports enum-typed
 * fields as the variant's text name; `additiveFields` are wasm-only fields
 * the contract does not carry.
 */
const MIRRORED_DICTIONARIES = [
  { contractName: 'GeographicCoordinate' },
  { contractName: 'BoundingBox' },
  { contractName: 'SimulatedTripPace' },
  { contractName: 'SimulatedTripFix' },
  { contractName: 'RouteStep' },
  { contractName: 'RouteLeg' },
  { contractName: 'Route' },
  {
    contractName: 'NavigationLocationUpdate',
    surfaceName: 'NavigatorLocationUpdate',
    note: 'The WebAssembly surface names this record `NavigatorLocationUpdate`; fields and nullability match the contract.',
  },
  { contractName: 'ScreenPoint' },
  { contractName: 'NormalizedWorldCoordinate' },
  { contractName: 'TileKey' },
  { contractName: 'ViewportSize' },
  { contractName: 'EdgeInsets' },
  { contractName: 'OverlayColor' },
  { contractName: 'SourceHeader' },
  { contractName: 'VectorTileSourceDescriptor' },
  { contractName: 'VectorTileData' },
  { contractName: 'UrlTemplateVectorTileSource' },
  { contractName: 'ProviderVectorTileSource' },
  { contractName: 'PositionFix' },
  { contractName: 'VectorTileSourceLayerCount' },
  {
    contractName: 'VectorTileSourceStatus',
    additiveFields: [
      {
        name: 'payloadCacheHitCount',
        type: 'number',
        isOptional: true,
        docLines: [
          'Payload-cache lookups that hit, cumulative while the source is configured. Absent from the not-configured status.',
        ],
      },
      {
        name: 'payloadCacheMissCount',
        type: 'number',
        isOptional: true,
        docLines: [
          'Payload-cache lookups that missed, cumulative while the source is configured. Absent from the not-configured status.',
        ],
      },
      {
        name: 'inFlightTileFetchCount',
        type: 'number',
        isOptional: true,
        docLines: ['Tile fetches in flight when the last frame resolved.'],
      },
      {
        name: 'fallbackTileCount',
        type: 'number',
        isOptional: true,
        docLines: [
          'Missing tiles the last frame covered with a cached ancestor.',
        ],
      },
      {
        name: 'timeToFirstDrawnTileMilliseconds',
        type: 'number | null',
        isOptional: true,
        docLines: [
          'Milliseconds from installing the source to the first painted tile, or null until a tile has painted.',
        ],
      },
    ],
  },
  { contractName: 'MapOverlay' },
  { contractName: 'TrafficFlowData' },
  { contractName: 'SurfaceHandle' },
  { contractName: 'DistanceMatrixEntry' },
  {
    contractName: 'TrafficIncident',
    surfaceName: 'TrafficIncidentRow',
    enumFieldsAsText: true,
    note: 'The WebAssembly surface flattens this record to lower-camel field names and reports its enum fields as the contract variant names.',
  },
  {
    contractName: 'TrafficFlowData',
    surfaceName: 'TrafficFlowRow',
    enumFieldsAsText: true,
    note: 'The WebAssembly surface flattens this record to lower-camel field names and reports its congestion level as the contract variant name.',
  },
];

/**
 * The contract enums the WebAssembly façade mirrors, in declaration order.
 * `casing` selects the surface spelling: `identity` keeps the contract
 * variant name, `lowerCamel` spells the wasm plain-object value.
 */
const MIRRORED_ENUMS = [
  {
    contractName: 'VectorTileEncoding',
    casing: 'identity',
  },
  {
    contractName: 'MapOverlayKind',
    casing: 'lowerCamel',
    note: 'The WebAssembly canvas selects these as lower-camel strings.',
  },
  {
    contractName: 'TrafficFlowCongestionLevel',
    casing: 'lowerCamel',
    note: 'The WebAssembly surface reports these as lower-camel strings.',
  },
  {
    contractName: 'PositionSourceError',
    casing: 'identity',
  },
  {
    contractName: 'PositionSourceError',
    surfaceName: 'PositionSourceStatusName',
    casing: 'lowerCamel',
    note: 'The WebAssembly `PositionController` reports these lower-camel source-status names.',
  },
  {
    contractName: 'NavigationPhase',
    casing: 'lowerCamel',
  },
  {
    contractName: 'NavigationPhaseChange',
    casing: 'lowerCamel',
  },
  {
    contractName: 'DeviationStatusChangeKind',
    casing: 'identity',
  },
];

/**
 * Error codes the WebAssembly seam defines for failures with no contract
 * variant. They stay hand-declared because the contract has no counterpart.
 */
const SEAM_ONLY_ERROR_CODES = [
  'invalidArgument',
  'unsupportedCapability',
  'geometryUnavailable',
  'runtimeUnavailable',
  'internal',
];

/** The contract error recorded as the structured union. */
const CONTRACT_ERROR_INTERFACE_NAME = 'SdkError';

/** The declaration order of the generated contract surface. */
const FACADE_DECLARATIONS = [
  ...MIRRORED_DICTIONARIES.map((entry) => ({ kind: 'dictionary', ...entry })),
  { kind: 'error', contractName: CONTRACT_ERROR_INTERFACE_NAME },
  { kind: 'errorCode' },
  ...MIRRORED_ENUMS.map((entry) => ({ kind: 'enum', ...entry })),
];

/**
 * The Rust field-name constants the generator owns, each mapped to the
 * contract record field it spells.
 */
const RUST_FIELD_NAME_CONSTANTS = [
  { constant: 'SCREEN_POINT_FIELD_X', dictionary: 'ScreenPoint', field: 'x' },
  { constant: 'SCREEN_POINT_FIELD_Y', dictionary: 'ScreenPoint', field: 'y' },
  { constant: 'COORDINATE_FIELD_LATITUDE', dictionary: 'GeographicCoordinate', field: 'latitude' },
  { constant: 'COORDINATE_FIELD_LONGITUDE', dictionary: 'GeographicCoordinate', field: 'longitude' },
  { constant: 'NORMALIZED_WORLD_FIELD_X', dictionary: 'NormalizedWorldCoordinate', field: 'x' },
  { constant: 'NORMALIZED_WORLD_FIELD_Y', dictionary: 'NormalizedWorldCoordinate', field: 'y' },
  { constant: 'TILE_FIELD_TILE_X', dictionary: 'TileKey', field: 'tile_x' },
  { constant: 'TILE_FIELD_TILE_Y', dictionary: 'TileKey', field: 'tile_y' },
  { constant: 'TILE_FIELD_ZOOM_LEVEL', dictionary: 'TileKey', field: 'zoom_level' },
  { constant: 'ROUTE_FIELD_DISTANCE_METERS', dictionary: 'Route', field: 'distance_meters' },
  { constant: 'ROUTE_FIELD_DURATION_SECONDS', dictionary: 'Route', field: 'duration_seconds' },
  { constant: 'ROUTE_FIELD_BOUNDING_BOX', dictionary: 'Route', field: 'bounding_box' },
  { constant: 'ROUTE_FIELD_GEOMETRY', dictionary: 'Route', field: 'geometry' },
  { constant: 'ROUTE_FIELD_LEGS', dictionary: 'Route', field: 'legs' },
  { constant: 'ROUTE_FIELD_STEPS', dictionary: 'RouteLeg', field: 'steps' },
  { constant: 'ROUTE_FIELD_MANEUVER_TYPE', dictionary: 'RouteStep', field: 'maneuver_type' },
  { constant: 'ROUTE_FIELD_INSTRUCTION_DISPLAY_TEXT', dictionary: 'RouteStep', field: 'instruction_display_text' },
  { constant: 'ROUTE_FIELD_INSTRUCTION_SPEECH_TEXT', dictionary: 'RouteStep', field: 'instruction_speech_text' },
  { constant: 'ROUTE_FIELD_SHAPE', dictionary: 'RouteStep', field: 'shape' },
  { constant: 'BOUNDING_BOX_FIELD_SOUTH_WEST', dictionary: 'BoundingBox', field: 'south_west' },
  { constant: 'BOUNDING_BOX_FIELD_NORTH_EAST', dictionary: 'BoundingBox', field: 'north_east' },
  { constant: 'SIMULATED_TRIP_PACE_FIELD_SPEED_METERS_PER_SECOND', dictionary: 'SimulatedTripPace', field: 'speed_meters_per_second' },
  { constant: 'SIMULATED_TRIP_PACE_FIELD_SPEED_MULTIPLIER', dictionary: 'SimulatedTripPace', field: 'speed_multiplier' },
  { constant: 'SIMULATED_TRIP_FIX_FIELD_COORDINATE', dictionary: 'SimulatedTripFix', field: 'coordinate' },
  { constant: 'SIMULATED_TRIP_FIX_FIELD_COURSE_HEADING_DEGREES', dictionary: 'SimulatedTripFix', field: 'course_heading_degrees' },
  { constant: 'SIMULATED_TRIP_FIX_FIELD_TRAVELLED_METERS', dictionary: 'SimulatedTripFix', field: 'travelled_meters' },
  { constant: 'SIMULATED_TRIP_FIX_FIELD_REMAINING_METERS', dictionary: 'SimulatedTripFix', field: 'remaining_meters' },
  { constant: 'SIMULATED_TRIP_FIX_FIELD_ELAPSED_SECONDS', dictionary: 'SimulatedTripFix', field: 'elapsed_seconds' },
  { constant: 'SIMULATED_TRIP_FIX_FIELD_IS_ARRIVED', dictionary: 'SimulatedTripFix', field: 'is_arrived' },
  { constant: 'COLOR_FIELD_RED', dictionary: 'OverlayColor', field: 'red' },
  { constant: 'COLOR_FIELD_GREEN', dictionary: 'OverlayColor', field: 'green' },
  { constant: 'COLOR_FIELD_BLUE', dictionary: 'OverlayColor', field: 'blue' },
  { constant: 'COLOR_FIELD_ALPHA', dictionary: 'OverlayColor', field: 'alpha' },
  { constant: 'VECTOR_SOURCE_FIELD_URL_TEMPLATE', dictionary: 'UrlTemplateVectorTileSource', field: 'url_template' },
  { constant: 'VECTOR_SOURCE_FIELD_SOURCE_NAME', dictionary: 'UrlTemplateVectorTileSource', field: 'source_name' },
  { constant: 'VECTOR_SOURCE_FIELD_ATTRIBUTION_TEXT', dictionary: 'UrlTemplateVectorTileSource', field: 'attribution_text' },
  { constant: 'VECTOR_SOURCE_FIELD_STYLE_ID', dictionary: 'UrlTemplateVectorTileSource', field: 'style_id' },
  { constant: 'VECTOR_SOURCE_FIELD_MINIMUM_ZOOM_LEVEL', dictionary: 'UrlTemplateVectorTileSource', field: 'minimum_zoom_level' },
  { constant: 'VECTOR_SOURCE_FIELD_MAXIMUM_ZOOM_LEVEL', dictionary: 'UrlTemplateVectorTileSource', field: 'maximum_zoom_level' },
  { constant: 'VECTOR_SOURCE_FIELD_TOKEN_BUCKET_CAPACITY_TOKENS', dictionary: 'UrlTemplateVectorTileSource', field: 'token_bucket_capacity_tokens' },
  { constant: 'VECTOR_SOURCE_FIELD_REFILL_TOKENS_PER_SECOND', dictionary: 'UrlTemplateVectorTileSource', field: 'refill_tokens_per_second' },
  { constant: 'VECTOR_SOURCE_FIELD_TERMS_TEXT', dictionary: 'UrlTemplateVectorTileSource', field: 'terms_text' },
  { constant: 'VECTOR_SOURCE_FIELD_SUBDOMAINS', dictionary: 'UrlTemplateVectorTileSource', field: 'subdomains' },
  { constant: 'VECTOR_SOURCE_FIELD_HEADERS', dictionary: 'UrlTemplateVectorTileSource', field: 'headers' },
  { constant: 'VECTOR_SOURCE_FIELD_STYLE_HINTS', dictionary: 'UrlTemplateVectorTileSource', field: 'style_hints' },
  { constant: 'SOURCE_HEADER_FIELD_NAME', dictionary: 'SourceHeader', field: 'name' },
  { constant: 'SOURCE_HEADER_FIELD_VALUE', dictionary: 'SourceHeader', field: 'value' },
  { constant: 'PROVIDER_VECTOR_SOURCE_FIELD_PROVIDER_KEY', dictionary: 'ProviderVectorTileSource', field: 'provider_key' },
  { constant: 'PROVIDER_VECTOR_SOURCE_FIELD_STYLE_ID', dictionary: 'ProviderVectorTileSource', field: 'style_id' },
  { constant: 'VECTOR_STATUS_FIELD_IS_CONFIGURED', dictionary: 'VectorTileSourceStatus', field: 'is_configured' },
  { constant: 'VECTOR_STATUS_FIELD_STYLE_ID', dictionary: 'VectorTileSourceStatus', field: 'style_id' },
  { constant: 'VECTOR_STATUS_FIELD_SOURCE_NAME', dictionary: 'VectorTileSourceStatus', field: 'source_name' },
  { constant: 'VECTOR_STATUS_FIELD_ATTRIBUTION_TEXT', dictionary: 'VectorTileSourceStatus', field: 'attribution_text' },
  { constant: 'VECTOR_STATUS_FIELD_MAXIMUM_ZOOM_LEVEL', dictionary: 'VectorTileSourceStatus', field: 'maximum_zoom_level' },
  { constant: 'VECTOR_STATUS_FIELD_DRAWN_TILE_COUNT', dictionary: 'VectorTileSourceStatus', field: 'drawn_tile_count' },
  { constant: 'VECTOR_STATUS_FIELD_FAILED_TILE_COUNT', dictionary: 'VectorTileSourceStatus', field: 'failed_tile_count' },
  { constant: 'VECTOR_STATUS_FIELD_LAST_FAILURE_DETAIL', dictionary: 'VectorTileSourceStatus', field: 'last_failure_detail' },
  { constant: 'VECTOR_STATUS_FIELD_UNSUPPORTED_PROPERTY_COUNT', dictionary: 'VectorTileSourceStatus', field: 'unsupported_property_count' },
  { constant: 'VECTOR_STATUS_FIELD_UNSUPPORTED_PROPERTY_NAMES', dictionary: 'VectorTileSourceStatus', field: 'unsupported_property_names' },
  { constant: 'VECTOR_STATUS_FIELD_DEGRADED_ASSET_NAMES', dictionary: 'VectorTileSourceStatus', field: 'degraded_asset_names' },
  { constant: 'VECTOR_STATUS_FIELD_SOURCE_LAYER_COUNTS', dictionary: 'VectorTileSourceStatus', field: 'source_layer_counts' },
  { constant: 'VECTOR_SOURCE_LAYER_COUNT_FIELD_SOURCE_ID', dictionary: 'VectorTileSourceLayerCount', field: 'source_id' },
  { constant: 'VECTOR_SOURCE_LAYER_COUNT_FIELD_MATCHED_LAYER_COUNT', dictionary: 'VectorTileSourceLayerCount', field: 'matched_layer_count' },
  { constant: 'VECTOR_SOURCE_LAYER_COUNT_FIELD_DRAWN_LAYER_COUNT', dictionary: 'VectorTileSourceLayerCount', field: 'drawn_layer_count' },
  { constant: 'OVERLAY_FIELD_KIND', dictionary: 'MapOverlay', field: 'kind' },
  { constant: 'OVERLAY_FIELD_COORDINATES', dictionary: 'MapOverlay', field: 'coordinates' },
  { constant: 'OVERLAY_FIELD_RING_COORDINATES', dictionary: 'MapOverlay', field: 'ring_coordinates' },
  { constant: 'OVERLAY_FIELD_WIDTH_PIXELS', dictionary: 'MapOverlay', field: 'width_pixels' },
  { constant: 'OVERLAY_FIELD_SHAPE_COORDINATES', dictionary: 'TrafficFlowData', field: 'shape_coordinates' },
  { constant: 'OVERLAY_FIELD_CONGESTION_LEVEL', dictionary: 'TrafficFlowData', field: 'congestion_level' },
  { constant: 'OVERLAY_FIELD_LATITUDE', dictionary: 'GeographicCoordinate', field: 'latitude' },
  { constant: 'OVERLAY_FIELD_LONGITUDE', dictionary: 'GeographicCoordinate', field: 'longitude' },
  { constant: 'INDICATOR_STATE_FIELD_COORDINATE', dictionary: 'LocationIndicatorState', field: 'coordinate' },
  { constant: 'INDICATOR_FIELD_COURSE_HEADING_DEGREES', dictionary: 'LocationIndicatorState', field: 'course_heading_degrees' },
  { constant: 'INDICATOR_FIELD_HORIZONTAL_ACCURACY_METERS', dictionary: 'LocationIndicatorState', field: 'horizontal_accuracy_meters' },
  { constant: 'INDICATOR_FIELD_VISUAL_KIND', dictionary: 'LocationIndicatorStyle', field: 'visual_kind' },
  { constant: 'INDICATOR_FIELD_RADIUS_PIXELS', dictionary: 'LocationIndicatorStyle', field: 'radius_pixels' },
  { constant: 'INDICATOR_FIELD_RGBA_COLOR', dictionary: 'LocationIndicatorStyle', field: 'rgba_color' },
  { constant: 'INDICATOR_FIELD_RING_RGBA_COLOR', dictionary: 'LocationIndicatorStyle', field: 'ring_rgba_color' },
  { constant: 'INDICATOR_FIELD_HALO_RGBA_COLOR', dictionary: 'LocationIndicatorStyle', field: 'halo_rgba_color' },
  { constant: 'INDICATOR_FIELD_SHOW_ACCURACY_RING', dictionary: 'LocationIndicatorStyle', field: 'show_accuracy_ring' },
  { constant: 'LOCATION_UPDATE_FIELD_COORDINATE', dictionary: 'NavigationLocationUpdate', field: 'coordinate' },
  { constant: 'LOCATION_UPDATE_FIELD_HORIZONTAL_ACCURACY_METERS', dictionary: 'NavigationLocationUpdate', field: 'horizontal_accuracy_meters' },
  { constant: 'LOCATION_UPDATE_FIELD_COURSE_HEADING_DEGREES', dictionary: 'NavigationLocationUpdate', field: 'course_heading_degrees' },
  { constant: 'LOCATION_UPDATE_FIELD_TIMESTAMP_SECONDS', dictionary: 'NavigationLocationUpdate', field: 'timestamp_seconds' },
  { constant: 'STATE_FIELD_PHASE', dictionary: 'TripNavigationState', field: 'phase' },
  { constant: 'STATE_FIELD_DEVIATION', dictionary: 'TripNavigationState', field: 'deviation' },
  { constant: 'STATE_FIELD_SNAPPED_COORDINATE', dictionary: 'TripNavigationState', field: 'snapped_coordinate' },
  { constant: 'PROGRESS_FIELD_DISTANCE_TO_NEXT_MANEUVER_METERS', dictionary: 'TripProgress', field: 'distance_to_next_maneuver_meters' },
  { constant: 'PROGRESS_FIELD_DISTANCE_REMAINING_METERS', dictionary: 'TripProgress', field: 'distance_remaining_meters' },
  { constant: 'PROGRESS_FIELD_DURATION_REMAINING_SECONDS', dictionary: 'TripProgress', field: 'duration_remaining_seconds' },
  { constant: 'DEVIATION_FIELD_STATUS_CHANGE_KIND', dictionary: 'DeviationStatusChange', field: 'status_change_kind' },
];

// --- TypeScript rendering ---------------------------------------------------

/** UniFFI numeric primitives all narrow to JavaScript `number`. */
const NUMERIC_UNIFFI_TYPE_NAMES = new Set([
  'double', 'f32', 'f64', 'float',
  'i8', 'i16', 'i32', 'i64', 'u8', 'u16', 'u32', 'u64',
]);

/** The scalar UniFFI types with a direct TypeScript spelling. */
const SCALAR_UNIFFI_TYPE_TO_TYPESCRIPT = new Map([
  ['boolean', 'boolean'],
  ['char', 'string'],
  ['string', 'string'],
]);

/** Converts a contract field name from snake case to lower camel case. */
export function contractFieldNameToTypescript(contract_field_name) {
  return contract_field_name.replace(
    /_+([a-z0-9])/g,
    (_match, character) => character.toUpperCase(),
  );
}

/** Converts a contract variant name to the surface's lower-camel spelling. */
function lower_camel_name(variant_name) {
  return variant_name.charAt(0).toLowerCase() + variant_name.slice(1);
}

/** Renders `///` documentation lines as a JSDoc block at one indentation. */
function render_doc_lines(doc_lines, indentation) {
  if (doc_lines.length === 0) {
    return [];
  }
  return [
    `${indentation}/**`,
    ...doc_lines.map((line) => (
      line === '' ? `${indentation} *` : `${indentation} * ${line}`
    )),
    `${indentation} */`,
  ];
}

/** Appends a note paragraph to a block's contract documentation. */
function documentation_with_note(doc_lines, note) {
  if (note === undefined || note === '') {
    return doc_lines;
  }
  return doc_lines.length === 0
    ? [note]
    : [...doc_lines, '', note];
}

/** The default surface name for every mirrored contract dictionary. */
function dictionary_surface_names() {
  const surface_names = new Map();
  for (const entry of MIRRORED_DICTIONARIES) {
    if (!surface_names.has(entry.contractName)) {
      surface_names.set(
        entry.contractName,
        entry.surfaceName ?? entry.contractName,
      );
    }
  }
  return surface_names;
}

/** The default surface name for every mirrored contract enum. */
function enum_surface_names() {
  const surface_names = new Map();
  for (const entry of MIRRORED_ENUMS) {
    if (!surface_names.has(entry.contractName)) {
      surface_names.set(
        entry.contractName,
        entry.surfaceName ?? entry.contractName,
      );
    }
  }
  return surface_names;
}

/** The TypeScript spelling of one UniFFI type annotation. */
function typescript_type_for_contract_type(
  type_name,
  is_nullable,
  contract,
  enum_fields_are_text,
  surface_names,
) {
  let base_type;
  const scalar_type = SCALAR_UNIFFI_TYPE_TO_TYPESCRIPT.get(type_name);
  if (scalar_type !== undefined) {
    base_type = scalar_type;
  } else if (NUMERIC_UNIFFI_TYPE_NAMES.has(type_name)) {
    base_type = 'number';
  } else if (contract.dictionaryDefinitions.has(type_name)) {
    base_type = surface_names.dictionaries.get(type_name) ?? type_name;
  } else if (contract.enumDefinitions.has(type_name)) {
    base_type = enum_fields_are_text
      ? 'string'
      : surface_names.enums.get(type_name) ?? type_name;
  } else {
    throw new Error(
      `the UniFFI type "${type_name}" has no TypeScript spelling`,
    );
  }
  return is_nullable ? `${base_type} | null` : base_type;
}

/** The TypeScript spelling of one dictionary field. */
function typescript_type_for_contract_field(
  field,
  contract,
  enum_fields_are_text,
  surface_names,
) {
  if (field.sequenceElementType !== null) {
    const element_type = typescript_type_for_contract_type(
      field.sequenceElementType,
      false,
      contract,
      enum_fields_are_text,
      surface_names,
    );
    const sequence_type = `${element_type}[]`;
    return field.isOptional ? `${sequence_type} | null` : sequence_type;
  }
  return typescript_type_for_contract_type(
    field.typeName,
    field.isOptional,
    contract,
    enum_fields_are_text,
    surface_names,
  );
}

/** Renders one interface field. */
function render_interface_field(field, doc_lines) {
  return [
    ...render_doc_lines(doc_lines, '    '),
    `    readonly ${field.name}${field.isOptional ? '?' : ''}: ${field.type};`,
  ].join('\n');
}

/** Renders one mirrored dictionary as an interface. */
function render_dictionary(entry, contract, surface_names) {
  const definition = contract.dictionaryDefinitions.get(entry.contractName);
  if (definition === undefined) {
    throw new Error(
      `the contract does not declare the dictionary "${entry.contractName}"`,
    );
  }
  const surface_name = entry.surfaceName ?? entry.contractName;
  const fields = [];
  for (const field of definition.fields) {
    const field_name = contractFieldNameToTypescript(field.name);
    let doc_lines = field.docLines;
    if (
      entry.enumFieldsAsText === true
      && field.sequenceElementType === null
      && contract.enumDefinitions.has(field.typeName)
    ) {
      doc_lines = documentation_with_note(
        doc_lines,
        'The WebAssembly surface reports this enum as its variant name text.',
      );
    }
    fields.push(render_interface_field({
      name: field_name,
      type: typescript_type_for_contract_field(
        field,
        contract,
        entry.enumFieldsAsText === true,
        surface_names,
      ),
      isOptional: false,
    }, doc_lines));
  }
  for (const additive_field of entry.additiveFields ?? []) {
    fields.push(render_interface_field({
      name: additive_field.name,
      type: additive_field.type,
      isOptional: additive_field.isOptional === true,
    }, additive_field.docLines));
  }
  const doc_lines = documentation_with_note(definition.docLines, entry.note);
  return [
    ...render_doc_lines(doc_lines, ''),
    `export interface ${surface_name} {`,
    ...fields,
    '}',
  ].join('\n');
}

/** Renders one mirrored enum as a string union. */
function render_enum(entry, contract) {
  const definition = contract.enumDefinitions.get(entry.contractName);
  if (definition === undefined) {
    throw new Error(
      `the contract does not declare the enum "${entry.contractName}"`,
    );
  }
  const surface_name = entry.surfaceName ?? entry.contractName;
  const values = definition.variants.map((variant) => (
    entry.casing === 'lowerCamel' ? lower_camel_name(variant.name) : variant.name
  ));
  if (values.length === 0) {
    throw new Error(`the contract enum "${entry.contractName}" has no variants`);
  }
  const doc_lines = documentation_with_note(definition.docLines, entry.note);
  return [
    ...render_doc_lines(doc_lines, ''),
    `export type ${surface_name} =`,
    ...values.map((value, index) => (
      `    | "${value}"${index === values.length - 1 ? ';' : ''}`
    )),
  ].join('\n');
}

/** Renders the contract's structured error union. */
function render_error_union(contract, surface_names) {
  const definition = contract.interfaceDefinitions.get(
    CONTRACT_ERROR_INTERFACE_NAME,
  );
  if (definition === undefined) {
    throw new Error(
      `the contract does not declare the error interface "${CONTRACT_ERROR_INTERFACE_NAME}"`,
    );
  }
  const variants = definition.members.filter(
    (member) => member.kind === 'variant',
  );
  const doc_lines = documentation_with_note(
    definition.docLines,
    'On the WebAssembly surface a failure is thrown as a JavaScript `Error` whose `message` is the verbatim detail and whose `code` is one of `SdkErrorCode`.',
  );
  return [
    ...render_doc_lines(doc_lines, ''),
    'export type SdkError =',
    ...variants.map((variant, index) => {
      const payload = variant.parameters.map((parameter) => (
        `; readonly ${contractFieldNameToTypescript(parameter.name)}: ${typescript_type_for_contract_type(
          parameter.typeName,
          false,
          contract,
          false,
          surface_names,
        )}`
      )).join('');
      return `    | { readonly variant: "${variant.name}"${payload} }${index === variants.length - 1 ? ';' : ''}`;
    }),
  ].join('\n');
}

/** Renders the stable error-code union. */
function render_error_code_union(contract) {
  const definition = contract.interfaceDefinitions.get(
    CONTRACT_ERROR_INTERFACE_NAME,
  );
  if (definition === undefined) {
    throw new Error(
      `the contract does not declare the error interface "${CONTRACT_ERROR_INTERFACE_NAME}"`,
    );
  }
  const contract_codes = definition.members
    .filter((member) => member.kind === 'variant')
    .map((variant) => lower_camel_name(variant.name));
  for (const seam_only_code of SEAM_ONLY_ERROR_CODES) {
    if (contract_codes.includes(seam_only_code)) {
      throw new Error(
        `the seam-only error code "${seam_only_code}" now matches a contract variant; drop it from SEAM_ONLY_ERROR_CODES`,
      );
    }
  }
  const codes = [...contract_codes, ...SEAM_ONLY_ERROR_CODES];
  const doc_lines = [
    'Stable codes carried by a failure thrown from the WebAssembly surface',
    '(`error.code`). The first group are the contract\'s `SdkError` variants in',
    'lower camel case; the remaining entries are seam-level categories the',
    'contract does not model.',
  ];
  return [
    ...render_doc_lines(doc_lines, ''),
    'export type SdkErrorCode =',
    ...codes.map((code, index) => (
      `    | "${code}"${index === codes.length - 1 ? ';' : ''}`
    )),
  ].join('\n');
}

/** Renders one entry of the facade declaration order. */
function render_facade_entry(entry, contract, surface_names) {
  if (entry.kind === 'dictionary') {
    return render_dictionary(entry, contract, surface_names);
  }
  if (entry.kind === 'enum') {
    return render_enum(entry, contract);
  }
  if (entry.kind === 'error') {
    return render_error_union(contract, surface_names);
  }
  if (entry.kind === 'errorCode') {
    return render_error_code_union(contract);
  }
  throw new Error(`unknown facade declaration kind "${entry.kind}"`);
}

/** The generated TypeScript header. */
const TYPESCRIPT_HEADER = [
  '// Generated by scripts/generate-web-facade.mjs from',
  `// ${UNIFFI_CONTRACT_RELATIVE_PATH}.`,
  '//',
  '// Do not edit this file by hand. The contract declarations below mirror the',
  '// frozen UniFFI contract for the WebAssembly surface: snake-case fields become',
  '// lower camel case and the UDL nullable marker becomes `T | null`. The',
  '// hand-written section at the end declares the WebAssembly-only projections',
  `// the contract does not carry (${TYPESCRIPT_SURFACE_RELATIVE_PATH}).`,
  '//',
  '// Regenerate with: node scripts/generate-web-facade.mjs',
].join('\n');

/** Renders the generated TypeScript mirror. */
function render_typescript_facade(contract, surface_text) {
  const surface_names = {
    dictionaries: dictionary_surface_names(),
    enums: enum_surface_names(),
  };
  const declarations = FACADE_DECLARATIONS.map((entry) => (
    render_facade_entry(entry, contract, surface_names)
  ));
  const sections = [
    TYPESCRIPT_HEADER,
    '// --- Contract declarations (generated) ---',
    declarations.join('\n\n'),
    '// --- WebAssembly surface projections (hand-written) ---',
    surface_text.trim(),
  ];
  return `${sections.join('\n\n')}\n`;
}

/** The generated Rust header. */
const RUST_HEADER = [
  '//! Generated by `scripts/generate-web-facade.mjs` from',
  `//! \`${UNIFFI_CONTRACT_RELATIVE_PATH}\`.`,
  '//!',
  '//! JavaScript field names of the contract records the WebAssembly surface',
  '//! exchanges. Do not edit this file by hand; run the generator instead.',
].join('\n');

/** Compares two strings by code point, independent of the host locale. */
function compare_code_points(left, right) {
  if (left < right) {
    return -1;
  }
  if (left > right) {
    return 1;
  }
  return 0;
}

/** Renders the generated Rust field-name constants. */
function render_rust_field_names(contract) {
  const seen_constants = new Set();
  const lines = [
    RUST_HEADER,
    '',
    '// The constants sit in a skipped block so the generator output stays byte',
    '// stable under `cargo fmt`; their values are the formatted contract field',
    '// names, not hand-wrapped text. A host test build compiles this block too',
    '// but reads only the declarations its tested seams use.',
    '#[allow(dead_code)]',
    '#[rustfmt::skip]',
    'mod field_names {',
  ];
  const mappings = [...RUST_FIELD_NAME_CONSTANTS].sort(
    (left, right) => compare_code_points(left.constant, right.constant),
  );
  for (const mapping of mappings) {
    if (seen_constants.has(mapping.constant)) {
      throw new Error(
        `the Rust constant "${mapping.constant}" is mapped twice`,
      );
    }
    seen_constants.add(mapping.constant);
    const definition = contract.dictionaryDefinitions.get(
      mapping.dictionary,
    );
    if (definition === undefined) {
      throw new Error(
        `the contract does not declare the dictionary "${mapping.dictionary}" the Rust constant "${mapping.constant}" reads`,
      );
    }
    const field = definition.fields.find(
      (candidate) => candidate.name === mapping.field,
    );
    if (field === undefined) {
      throw new Error(
        `the contract dictionary "${mapping.dictionary}" does not declare the field "${mapping.field}" the Rust constant "${mapping.constant}" reads`,
      );
    }
    lines.push(
      `    /// JavaScript field name of the contract's \`${mapping.dictionary}.${mapping.field}\`.`,
    );
    lines.push(
      `    pub(crate) const ${mapping.constant}: &str = "${contractFieldNameToTypescript(field.name)}";`,
    );
    lines.push('');
  }
  if (lines[lines.length - 1] === '') {
    lines.pop();
  }
  lines.push('}');
  lines.push('');
  lines.push('pub(crate) use field_names::*;');
  return `${lines.join('\n').replace(/\n+$/, '')}\n`;
}

// --- Generation, check and write -------------------------------------------

/**
 * Generates every façade output from the contract.
 * @param {object} [options]
 * @param {string} [options.repositoryRoot] The repository root to read.
 * @param {string} [options.contractText] A contract override (tests).
 * @param {string} [options.surfaceText] A hand-written surface override (tests).
 * @returns {Map<string, string>} Relative path to generated content.
 */
export function generateWebFacade(options = {}) {
  const repository_root = options.repositoryRoot ?? REPOSITORY_ROOT;
  const contract_text = options.contractText
    ?? readFileSync(path.join(repository_root, UNIFFI_CONTRACT_RELATIVE_PATH), 'utf8');
  const surface_text = options.surfaceText
    ?? readFileSync(path.join(repository_root, TYPESCRIPT_SURFACE_RELATIVE_PATH), 'utf8');
  const contract = parseUniffiContract(contract_text);

  return new Map([
    [
      GENERATED_TYPESCRIPT_RELATIVE_PATH,
      render_typescript_facade(contract, surface_text),
    ],
    [
      GENERATED_RUST_RELATIVE_PATH,
      render_rust_field_names(contract),
    ],
  ]);
}

/** Describes the first line where two texts differ. */
function describe_difference(expected_text, actual_text) {
  const expected_lines = expected_text.split('\n');
  const actual_lines = actual_text.split('\n');
  let first_difference_line = 0;
  while (
    first_difference_line < expected_lines.length
    && first_difference_line < actual_lines.length
    && expected_lines[first_difference_line] === actual_lines[first_difference_line]
  ) {
    first_difference_line += 1;
  }
  return {
    firstDifferenceLine: first_difference_line + 1,
    expectedLine: expected_lines[first_difference_line] ?? '',
    actualLine: actual_lines[first_difference_line] ?? '',
    expectedLineCount: expected_lines.length,
    actualLineCount: actual_lines.length,
  };
}

/**
 * Checks the committed façade against freshly generated content.
 * @param {object} [options]
 * @param {string} [options.rootDirectory] The tree holding the committed files.
 * @param {Map<string, string>} [options.generated] The expected content.
 * @returns {{isStale: boolean, staleFiles: {relativePath: string, reason: string, difference?: object}[]}}
 */
export function checkWebFacade(options = {}) {
  const root_directory = options.rootDirectory ?? REPOSITORY_ROOT;
  const generated = options.generated ?? generateWebFacade();
  const stale_files = [];
  for (const [relative_path, expected_text] of generated) {
    const absolute_path = path.join(root_directory, relative_path);
    if (!existsSync(absolute_path)) {
      stale_files.push({ relativePath: relative_path, reason: 'missing' });
      continue;
    }
    const actual_text = readFileSync(absolute_path, 'utf8');
    if (actual_text !== expected_text) {
      stale_files.push({
        relativePath: relative_path,
        reason: 'drifted',
        difference: describe_difference(expected_text, actual_text),
      });
    }
  }
  return { isStale: stale_files.length > 0, staleFiles: stale_files };
}

/**
 * Writes every generated façade output.
 * @param {object} [options]
 * @param {string} [options.rootDirectory] The tree to write into.
 * @param {Map<string, string>} [options.generated] The content to write.
 */
export function writeWebFacade(options = {}) {
  const root_directory = options.rootDirectory ?? REPOSITORY_ROOT;
  const generated = options.generated ?? generateWebFacade();
  for (const [relative_path, content] of generated) {
    writeFileSync(path.join(root_directory, relative_path), content);
  }
}

/**
 * Runs the generator: writes by default, checks with `--check`.
 * @param {string[]} argv Command-line arguments.
 * @returns {number} The process exit code.
 */
export function main(argv = process.argv.slice(2)) {
  if (argv.includes('--check')) {
    const generated = generateWebFacade();
    const result = checkWebFacade({ generated });
    if (result.isStale) {
      for (const stale_file of result.staleFiles) {
        if (stale_file.reason === 'missing') {
          process.stderr.write(
            `web-facade drift: ${stale_file.relativePath} is missing\n`,
          );
          continue;
        }
        const difference = stale_file.difference;
        process.stderr.write(
          `web-facade drift: ${stale_file.relativePath} (first difference at line ${difference.firstDifferenceLine})\n`,
        );
        process.stderr.write(`  expected: ${difference.expectedLine}\n`);
        process.stderr.write(`  found:    ${difference.actualLine}\n`);
      }
      process.stderr.write(
        `web-facade drift: ${result.staleFiles.length} file(s) are stale; run node scripts/generate-web-facade.mjs\n`,
      );
      return 1;
    }
    process.stdout.write(
      `web-facade check: ${generated.size} file(s) match\n`,
    );
    return 0;
  }
  const generated = generateWebFacade();
  writeWebFacade({ generated });
  process.stdout.write(
    `web-facade generated: ${[...generated.keys()].join(', ')}\n`,
  );
  return 0;
}

if (
  process.argv[1] !== undefined
  && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  process.exitCode = main(process.argv.slice(2));
}
