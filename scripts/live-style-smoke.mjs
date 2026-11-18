#!/usr/bin/env node
// Live provider-style smoke.
//
// This is a maintainer tool, not a test. It makes real provider requests, so
// it never runs in the committed suites and refuses to run without
// `--enable-network`. It reads one credential from the environment at run
// time, uses it only in request URLs, and never prints, stores or logs it.
//
// It fetches a provider's published style document, its sprite pair and one
// glyph range, plus one vector tile, then hands the fetched files to the
// SDK's opt-in `style-smoke` binary. That binary compiles the document,
// decodes the tile, styles the tile under the document, installs the compiled
// style in a vector-base scheduler and prints the layer, feature, painted
// overlay, label, icon and diagnostic counts.
//
//   PROVIDER_API_KEY=... node scripts/live-style-smoke.mjs \
//       --enable-network --provider mapbox --tile 14/2661/6286
//   PROVIDER_API_KEY=... node scripts/live-style-smoke.mjs \
//       --enable-network --provider here --tile 6/18/27
//
// The credential's query parameter mirrors the provider crate: Mapbox's
// client appends `access_token` and HERE's appends `apiKey`. The registry
// descriptor carries no credential-parameter field, so the table below is the
// one place this tool knows it.

import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPOSITORY_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const PROVIDERS_REGISTRY_PATH = path.join(REPOSITORY_ROOT, 'platform', 'providers.json');

/** The query parameter each catalog provider's client appends its credential as. */
const CREDENTIAL_PARAMETERS = {
  mapbox: 'access_token',
  here: 'apiKey',
};

/** The default environment variable the credential is read from. */
const DEFAULT_CREDENTIAL_ENVIRONMENT_VARIABLE = 'PROVIDER_API_KEY';

const MAPBOX_SPRITES_URI_PREFIX = 'mapbox://sprites/';
const MAPBOX_FONTS_URI_PREFIX = 'mapbox://fonts/';
const MAPBOX_TILESET_URI_PREFIX = 'mapbox://';
const MAPBOX_SPRITES_API_BASE_URL = 'https://api.mapbox.com/styles/v1';
const MAPBOX_FONTS_API_BASE_URL = 'https://api.mapbox.com/fonts/v1';
const MAPBOX_TILES_API_BASE_URL = 'https://api.mapbox.com/v4';
const GLYPH_RANGE = '0-255';

function usage() {
  console.log(
    [
      'usage: node scripts/live-style-smoke.mjs --enable-network [options]',
      '',
      '  --provider mapbox|here       catalog provider (default: mapbox)',
      '  --style-url URL              style document address (default: registry document_url)',
      '  --tile z/x/y                one vector tile to fetch (required)',
      '  --tile-url TEMPLATE          tile address with {z}/{x}/{y} (default: from the style source)',
      '  --credential-env NAME        environment variable (default: PROVIDER_API_KEY)',
      '',
      'Makes real provider requests and reads a credential at run time. It is never part of the test suites.',
    ].join('\n'),
  );
}

function argumentValue(argumentsList, name) {
  const index = argumentsList.indexOf(name);
  return index >= 0 && index + 1 < argumentsList.length ? argumentsList[index + 1] : undefined;
}

/** Substitutes the credential without letting it enter a message or a file. */
function withCredential(url, credential, parameter) {
  const parsed = new URL(url);
  parsed.searchParams.set(parameter, credential);
  return parsed.toString();
}

/**
 * Fetches one address and returns its bytes. Failures name the resource and
 * the HTTP status only: the request URL carries the credential and must never
 * reach an error message.
 */
async function fetchResource(label, address, credential, parameter) {
  let response;
  try {
    response = await fetch(withCredential(address, credential, parameter));
  } catch {
    throw new Error(`the ${label} request failed before a response arrived`);
  }
  if (!response.ok) {
    throw new Error(`the ${label} request answered HTTP ${response.status}`);
  }
  return new Uint8Array(await response.arrayBuffer());
}

/**
 * Resolves a style resource reference the way the provider does: vendor URI
 * schemes map to the provider's API, absolute web addresses stay as written,
 * and anything else resolves against the style document's own address.
 */
function resolveDeclaredAddress(declaredAddress, styleDocumentUrl) {
  const address = declaredAddress.trim();
  if (address.startsWith(MAPBOX_SPRITES_URI_PREFIX)) {
    const segments = address.slice(MAPBOX_SPRITES_URI_PREFIX.length).split('/');
    if (segments.length < 2 || segments[0] === '' || segments[1] === '') {
      throw new Error('the style names a sprite URI without a user and style segment');
    }
    return `${MAPBOX_SPRITES_API_BASE_URL}/${segments[0]}/${segments[1]}/sprite`;
  }
  if (address.startsWith(MAPBOX_FONTS_URI_PREFIX)) {
    const segments = address.slice(MAPBOX_FONTS_URI_PREFIX.length).split('/');
    if (segments.length !== 3 || segments.some((segment) => segment === '')) {
      throw new Error('the style names a glyph URI without a user, font stack and range');
    }
    const [username, fontstack, range] = segments;
    return `${MAPBOX_FONTS_API_BASE_URL}/${username}/${fontstack}/${range}.pbf`;
  }
  if (/^https?:\/\//i.test(address)) {
    return address;
  }
  return new URL(address, styleDocumentUrl).toString();
}

/** The first symbol layer's declared font stack, as a glyph request segment. */
function firstFontStack(styleDocument) {
  for (const layer of styleDocument.layers ?? []) {
    const declaredFont = layer.layout?.['text-font'];
    if (Array.isArray(declaredFont) && declaredFont.length > 0) {
      return declaredFont.join(',');
    }
  }
  return undefined;
}

/** The style's first vector source tile template, when it declares one. */
function declaredTileTemplate(styleDocument) {
  for (const source of Object.values(styleDocument.sources ?? {})) {
    if (source.type !== 'vector') {
      continue;
    }
    if (Array.isArray(source.tiles) && source.tiles.length > 0) {
      return source.tiles[0];
    }
    if (typeof source.url === 'string' && source.url.startsWith(MAPBOX_TILESET_URI_PREFIX)) {
      const tileset = source.url.slice(MAPBOX_TILESET_URI_PREFIX.length);
      return `${MAPBOX_TILES_API_BASE_URL}/${tileset}/{z}/{x}/{y}.mvt`;
    }
  }
  return undefined;
}

function parsedTileArgument(tileArgument) {
  const match = /^(\d+)\/(\d+)\/(\d+)$/.exec(tileArgument ?? '');
  if (match === null) {
    return undefined;
  }
  return { zoom: Number(match[1]), tileX: Number(match[2]), tileY: Number(match[3]) };
}

async function main() {
  const argumentsList = process.argv.slice(2);
  if (argumentsList.includes('--help') || argumentsList.includes('-h')) {
    usage();
    return;
  }
  if (!argumentsList.includes('--enable-network')) {
    console.error(
      'live-style-smoke makes real provider requests; pass --enable-network to confirm. It never runs in the test suites.',
    );
    process.exitCode = 2;
    return;
  }
  const providerKey = argumentValue(argumentsList, '--provider') ?? 'mapbox';
  const credentialParameter = CREDENTIAL_PARAMETERS[providerKey];
  if (credentialParameter === undefined) {
    console.error(`live-style-smoke: provider ${providerKey} is not a credentialed vector provider.`);
    process.exitCode = 2;
    return;
  }
  const credentialEnvironmentVariable =
    argumentValue(argumentsList, '--credential-env') ?? DEFAULT_CREDENTIAL_ENVIRONMENT_VARIABLE;
  const credential = process.env[credentialEnvironmentVariable] ?? '';
  if (credential.trim() === '') {
    console.error(
      `live-style-smoke: set the credential in ${credentialEnvironmentVariable}; it is read at run time and never printed.`,
    );
    process.exitCode = 2;
    return;
  }
  const tile = parsedTileArgument(argumentValue(argumentsList, '--tile'));
  if (tile === undefined) {
    console.error('live-style-smoke: --tile z/x/y is required, for example --tile 14/2661/6286.');
    process.exitCode = 2;
    return;
  }

  const registry = JSON.parse(readFileSync(PROVIDERS_REGISTRY_PATH, 'utf8'));
  const descriptor = (registry.providers ?? []).find((entry) => entry.provider_key === providerKey);
  if (descriptor === undefined) {
    console.error(`live-style-smoke: the registry carries no ${providerKey} descriptor.`);
    process.exitCode = 2;
    return;
  }
  const styleDocumentUrl =
    argumentValue(argumentsList, '--style-url') ?? descriptor.vector_styles?.[0]?.document_url;
  if (typeof styleDocumentUrl !== 'string') {
    console.error(`live-style-smoke: ${providerKey} declares no style document address.`);
    process.exitCode = 2;
    return;
  }

  const workDirectory = mkdtempSync(path.join(tmpdir(), 'live-style-smoke-'));
  try {
    const styleBytes = await fetchResource(
      'style document',
      styleDocumentUrl,
      credential,
      credentialParameter,
    );
    const styleText = new TextDecoder().decode(styleBytes);
    const styleDocument = JSON.parse(styleText);
    const styleFile = path.join(workDirectory, 'style.json');
    writeFileSync(styleFile, styleText);

    const spriteIndexFile = path.join(workDirectory, 'sprite.json');
    const spriteSheetFile = path.join(workDirectory, 'sprite.png');
    let spritePairFetched = false;
    if (typeof styleDocument.sprite === 'string') {
      const spriteBase = resolveDeclaredAddress(styleDocument.sprite, styleDocumentUrl);
      const spriteIndexBytes = await fetchResource(
        'sprite index',
        `${spriteBase}.json`,
        credential,
        credentialParameter,
      );
      const spriteSheetBytes = await fetchResource(
        'sprite sheet',
        `${spriteBase}.png`,
        credential,
        credentialParameter,
      );
      writeFileSync(spriteIndexFile, spriteIndexBytes);
      writeFileSync(spriteSheetFile, spriteSheetBytes);
      spritePairFetched = true;
    }

    if (typeof styleDocument.glyphs === 'string') {
      const fontStack = firstFontStack(styleDocument);
      if (fontStack !== undefined) {
        const glyphTemplate = resolveDeclaredAddress(styleDocument.glyphs, styleDocumentUrl);
        const glyphAddress = glyphTemplate
          .replace('{fontstack}', encodeURIComponent(fontStack))
          .replace('{range}', GLYPH_RANGE);
        const glyphBytes = await fetchResource(
          'glyph range',
          glyphAddress,
          credential,
          credentialParameter,
        );
        console.log(`glyph range bytes: ${glyphBytes.length}`);
      }
    }

    const tileTemplate =
      argumentValue(argumentsList, '--tile-url') ?? declaredTileTemplate(styleDocument);
    if (typeof tileTemplate !== 'string') {
      throw new Error(
        'the style declares no vector source tile template; pass --tile-url with {z}/{x}/{y}',
      );
    }
    const tileAddress = tileTemplate
      .replace('{z}', String(tile.zoom))
      .replace('{x}', String(tile.tileX))
      .replace('{y}', String(tile.tileY));
    const tileBytes = await fetchResource('vector tile', tileAddress, credential, credentialParameter);
    const tileFile = path.join(workDirectory, 'tile.mvt');
    writeFileSync(tileFile, tileBytes);
    console.log(`tile bytes: ${tileBytes.length}`);

    const helperArguments = [
      'run',
      '--quiet',
      '-p',
      'rustic-maps-providers',
      '--features',
      'style-smoke',
      '--bin',
      'style-smoke',
      '--',
      '--style-file',
      styleFile,
      '--tile-file',
      tileFile,
      '--zoom',
      String(tile.zoom),
      '--tile-x',
      String(tile.tileX),
      '--tile-y',
      String(tile.tileY),
    ];
    if (spritePairFetched) {
      helperArguments.push('--sprite-index', spriteIndexFile, '--sprite-sheet', spriteSheetFile);
    }
    const helper = spawnSync('cargo', helperArguments, {
      cwd: REPOSITORY_ROOT,
      encoding: 'utf8',
    });
    if (helper.stdout !== null && helper.stdout !== '') {
      process.stdout.write(helper.stdout);
    }
    if (helper.status !== 0) {
      if (helper.stderr !== null && helper.stderr !== '') {
        process.stderr.write(helper.stderr);
      }
      throw new Error(`the style-smoke binary exited with status ${helper.status}`);
    }
  } finally {
    rmSync(workDirectory, { recursive: true, force: true });
  }
}

main().catch((error) => {
  // The message never carries the credential: requests use it only in the URL
  // and every failure is reduced to a resource label and status.
  console.error(`live-style-smoke: ${error.message}`);
  process.exitCode = 1;
});
