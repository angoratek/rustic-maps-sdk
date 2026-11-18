#!/usr/bin/env node
// Provider documentation gate.
//
// The five provider-capability pages are published as product pages, and the
// provider switch leaves stale names behind faster than review catches them.
// This gate fails the documentation site build when:
//
//   - a published page still names a crate, trait or type the tree deleted;
//   - a page omits a provider the shared registry declares, or its key;
//   - a page omits a provider's attribution line, so the legal text cannot
//     drift from the descriptor;
//   - a page offers a provider the registry marks unavailable on that target,
//     or claims a capability the descriptor does not serve.
//
// The registry is the source of truth: a provider row that exists only in a
// page, or a page that forgets a descriptor, is a failure. Every page in the
// checked set is listed in PLATFORM_PAGES, and every descriptor is checked on
// every page, because the same provider set is offered on all targets.

import { readFileSync, readdirSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPOSITORY_ROOT = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);
const REGISTRY_PATH = path.join(REPOSITORY_ROOT, 'platform', 'providers.json');
const BOOK_SOURCE_DIRECTORY = path.join(REPOSITORY_ROOT, 'site', 'book-src');
const DOCUMENTS_DIRECTORY = path.join(REPOSITORY_ROOT, 'docs');

/** The capability pages, one per target. */
const PLATFORM_PAGES = [
  'provider-capabilities-android.md',
  'provider-capabilities-ios.md',
  'provider-capabilities-react-native.md',
  'provider-capabilities-wasm.md',
  'provider-capabilities-rust.md',
];

/**
 * How a capability page's file stem maps to the registry's
 * `registration_available` key. The Rust page offers the provider crates
 * themselves, which every descriptor carries, so it has no availability key.
 */
const PAGE_AVAILABILITY_KEYS = new Map([
  ['provider-capabilities-android.md', 'android'],
  ['provider-capabilities-ios.md', 'ios'],
  ['provider-capabilities-react-native.md', 'react_native'],
  ['provider-capabilities-wasm.md', 'wasm'],
]);

/**
 * The capability phrases a provider row's Serves cell may use, paired with
 * the registry contracts they claim. A phrase outside this table fails the
 * gate, so a new capability wording is mapped deliberately rather than
 * passing unread.
 */
const CAPABILITY_PHRASES = [
  { prefix: 'geocoding', contracts: ['geocoding'] },
  { prefix: 'reverse geocoding', contracts: ['geocoding'] },
  { prefix: 'search and suggestions', contracts: ['place_search'] },
  { prefix: 'routing', contracts: ['routing'] },
  { prefix: 'distance matrix', contracts: ['distance_matrix'] },
  {
    prefix: 'traffic incidents and flow',
    contracts: ['traffic_incidents', 'traffic_flow'],
  },
  { prefix: 'raster tiles', contracts: ['raster_tiles'] },
  { prefix: 'vector tiles', contracts: ['vector_tiles'] },
];

/**
 * The capabilities a provider row's Serves cell claims. A comma inside a
 * phrase that does not begin a new capability — the vector sources' style
 * list, for example — continues the previous claim. A leading phrase outside
 * the table fails the gate, so a new capability wording is mapped
 * deliberately rather than passing unread.
 */
function capability_claims(serves) {
  const claims = [];
  for (const raw_segment of serves.split(',')) {
    const segment = raw_segment.trim();
    const capability = CAPABILITY_PHRASES.find((candidate) =>
      segment.toLowerCase().startsWith(candidate.prefix));
    if (capability !== undefined || claims.length === 0) {
      claims.push({ phrase: segment, capability });
    } else {
      claims[claims.length - 1].phrase += `, ${segment}`;
    }
  }
  return claims;
}

/**
 * Parses one capability page's provider table into rows: the backticked
 * provider key and the Serves cell. Rows in other tables on the page
 * (attribution, calls) do not carry a backticked key in their second cell
 * and are skipped.
 */
function provider_rows(content) {
  const rows = [];
  for (const line of content.split('\n')) {
    const cells = line
      .replace(/^\||\|$/g, '')
      .split('|')
      .map((cell) => cell.trim());
    // A row may carry the attribution and terms cells after the five
    // structural columns; the key and the Serves cell keep their positions.
    if (cells.length < 5) {
      continue;
    }
    const key_match = cells[1].match(/^`([a-z0-9_]+)`$/);
    if (key_match === null) {
      continue;
    }
    rows.push({ key: key_match[1], serves: cells[4] });
  }
  return rows;
}

/**
 * Names the provider layer replaced or deleted. A published page that still
 * carries one teaches a reader an interface that does not exist.
 */
const DELETED_NAMES = [
  'provider-transport',
  'provider_transport',
  'mapquest-provider',
  'photon-provider',
  'osrm-provider',
  'valhalla-provider',
  'url-template-raster-provider',
  'url-template-vector-provider',
  'openstreetmap-us-vector-provider',
  'OpenStreetMapUsVectorTileProvider',
  'MapQuestGeocodingProvider',
  'MapQuestRoutingProvider',
  'MapQuestDistanceMatrixProvider',
  'OsrmRoutingProvider',
  'ValhallaRoutingProvider',
];

/** Every published chapter under site/book-src. */
function published_chapters() {
  return readdirSync(BOOK_SOURCE_DIRECTORY)
    .filter((file_name) => file_name.endsWith('.md'))
    .map((file_name) => path.join(BOOK_SOURCE_DIRECTORY, file_name));
}

/** Every workspace document that describes the product. */
function workspace_documents() {
  return readdirSync(DOCUMENTS_DIRECTORY)
    .filter((file_name) => file_name.endsWith('.md'))
    .filter((file_name) => file_name !== 'THIRD_PARTY_NOTICES.md')
    .map((file_name) => path.join(DOCUMENTS_DIRECTORY, file_name));
}

function line_number_of(content, index) {
  return content.slice(0, index).split('\n').length;
}

function findings() {
  const failures = [];
  const registry = JSON.parse(readFileSync(REGISTRY_PATH, 'utf8'));
  const descriptors = registry.providers;

  // 1. Deleted names anywhere in a published chapter or workspace document.
  for (const file_path of [...published_chapters(), ...workspace_documents()]) {
    const content = readFileSync(file_path, 'utf8');
    for (const deleted_name of DELETED_NAMES) {
      const index = content.indexOf(deleted_name);
      if (index >= 0) {
        failures.push(
          `${path.relative(REPOSITORY_ROOT, file_path)}:${line_number_of(content, index)} names deleted \`${deleted_name}\``,
        );
      }
    }
  }

  // 2. Every capability page names every provider key and its attribution.
  for (const page_name of PLATFORM_PAGES) {
    const file_path = path.join(BOOK_SOURCE_DIRECTORY, page_name);
    const content = readFileSync(file_path, 'utf8');
    for (const descriptor of descriptors) {
      const key_marker = `\`${descriptor.provider_key}\``;
      if (!content.includes(key_marker)) {
        failures.push(
          `${page_name} omits the \`${descriptor.provider_key}\` provider row`,
        );
      }
      if (!content.includes(descriptor.attribution_text)) {
        failures.push(
          `${page_name} omits the attribution line for \`${descriptor.provider_key}\``,
        );
      }
    }
  }

  // 3. Every provider row claims only what the registry carries: availability
  // on that target and capabilities the descriptor serves. A page that
  // offers more than the catalog does teaches a reader a provider switch
  // that refuses.
  const descriptors_by_key = new Map(
    descriptors.map((descriptor) => [descriptor.provider_key, descriptor]),
  );
  for (const page_name of PLATFORM_PAGES) {
    const file_path = path.join(BOOK_SOURCE_DIRECTORY, page_name);
    const content = readFileSync(file_path, 'utf8');
    const availability_key = PAGE_AVAILABILITY_KEYS.get(page_name);
    const rows = provider_rows(content);
    if (rows.length === 0) {
      failures.push(
        `${page_name} carries no provider table row the gate can read`,
      );
      continue;
    }
    for (const row of rows) {
      const descriptor = descriptors_by_key.get(row.key);
      if (descriptor === undefined) {
        failures.push(
          `${page_name} offers \`${row.key}\`, which the registry does not declare`,
        );
        continue;
      }
      // A descriptor with a registration alias is offered through the
      // capability picker, so the page must agree with its per-target
      // availability. A tile source with none is offered as a base map the
      // layers control installs, outside the picker.
      const offered_through_picker =
        typeof descriptor.wasm_registration === 'string';
      if (
        offered_through_picker
        && availability_key !== undefined
        && descriptor.registration_available?.[availability_key] !== true
      ) {
        failures.push(
          `${page_name} offers \`${row.key}\`, which the registry does not mark `
            + `available on ${availability_key}`,
        );
      }
      for (const claim of capability_claims(row.serves)) {
        if (claim.capability === undefined) {
          failures.push(
            `${page_name} offers \`${row.key}\` with the unmapped capability `
              + `phrase "${claim.phrase}"; map it in CAPABILITY_PHRASES`,
          );
          continue;
        }
        for (const contract of claim.capability.contracts) {
          if (!descriptor.served_contracts.includes(contract)) {
            failures.push(
              `${page_name} claims \`${row.key}\` serves ${contract}, which its `
                + 'descriptor does not',
            );
          }
        }
      }
    }
  }

  return failures;
}

const problems = findings();
if (problems.length > 0) {
  console.error('provider documentation gate: refused');
  for (const problem of problems) {
    console.error(`  - ${problem}`);
  }
  process.exit(1);
}
process.stdout.write(
  `provider documentation gate: ${PLATFORM_PAGES.length} pages agree with ${JSON.parse(readFileSync(REGISTRY_PATH, 'utf8')).providers.length} descriptors\n`,
);
