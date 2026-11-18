#!/usr/bin/env node
// Live provider smoke for the demo page.
//
// This is a maintainer tool, not a test: it makes real provider requests and
// therefore never runs in the committed suites. It reads the provider keys
// from ~/.everymap/config.toml on the machine it runs on, never prints or
// writes them, and reports one line per provider and served contract.
//
//   node scripts/smoke-providers.mjs                 # the published demo
//   node scripts/smoke-providers.mjs --local         # http://127.0.0.1:8642
//   node scripts/smoke-providers.mjs --strict        # exit 1 on any failure
//
// A provider with no local key and no public deployment is reported as
// skipped, not as a failure.

import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from '../platform/web/node_modules/playwright/index.mjs';

const REPOSITORY_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const PROVIDERS_REGISTRY_PATH = path.join(REPOSITORY_ROOT, 'platform', 'providers.json');
const CONFIG_PATH = path.join(process.env.HOME ?? '', '.everymap', 'config.toml');

const DEFAULT_REMOTE_URL = 'https://angoratek.github.io/rustic-maps-sdk-docs/demo/';
const LOCAL_URL = 'http://127.0.0.1:8642/';

/** Contract names the wasm `configureProvider` accepts. */
const CONTRACT_NAMES = {
  geocoding: 'Geocoding',
  place_search: 'PlaceSearch',
  routing: 'Routing',
  distance_matrix: 'DistanceMatrix',
  traffic_incidents: 'TrafficIncidents',
  traffic_flow: 'TrafficFlow',
  raster_tiles: 'RasterTiles',
};

/** Reads one provider's credential from the local config, never printing it. */
function providerKey(providerKeyName) {
  let text;
  try {
    text = readFileSync(CONFIG_PATH, 'utf8');
  } catch {
    return '';
  }
  const section = text
    .split(/^\[/m)
    .find((part) => part.startsWith(`providers.${providerKeyName}]`));
  if (section === undefined) {
    return '';
  }
  const match = section.match(/^\s*api_key\s*=\s*"([^"]*)"/m);
  return match ? match[1] : '';
}

function redact(value) {
  return String(value)
    .replace(/key=[^&\s"']+/gi, 'key=REDACTED')
    .replace(/apiKey=[^&\s"']+/gi, 'apiKey=REDACTED')
    .replace(/access_token=[^&\s"']+/gi, 'access_token=REDACTED')
    .slice(0, 220);
}

function argumentValue(argumentsList, name, fallback) {
  const index = argumentsList.indexOf(name);
  return index >= 0 && index + 1 < argumentsList.length
    ? argumentsList[index + 1]
    : fallback;
}

async function main() {
  const argumentsList = process.argv.slice(2);
  if (argumentsList.includes('--help')) {
    console.log(
      'usage: node scripts/smoke-providers.mjs [--local] [--base-url URL] [--strict]',
    );
    return;
  }
  const base_url = argumentsList.includes('--local')
    ? LOCAL_URL
    : argumentValue(argumentsList, '--base-url', DEFAULT_REMOTE_URL);
  const strict = argumentsList.includes('--strict');

  const registry = JSON.parse(readFileSync(PROVIDERS_REGISTRY_PATH, 'utf8'));
  const browser = await chromium.launch();
  const page = await browser.newPage();
  const results = [];

  await page.goto(`${base_url}?smoke=${Date.now()}`);
  await page.waitForFunction(() => window.mapTest !== undefined, null, { timeout: 60000 });
  await page.evaluate(() => window.mapTestReady);

  for (const descriptor of registry.providers) {
    const contractKeys = (descriptor.served_contracts ?? []).filter(
      (contractKey) => CONTRACT_NAMES[contractKey] !== undefined,
    );
    if (contractKeys.length === 0) {
      continue;
    }
    const key = providerKey(descriptor.provider_key);
    const deployment = descriptor.demo_base_url ?? '';
    if (descriptor.credential_label !== '' && key === '') {
      results.push([descriptor.display_name, 'all', 'skipped', 'no local key']);
      continue;
    }
    if (descriptor.registration_available?.wasm !== true) {
      // A URL-template source is installed through the map surface, not the
      // provider-key catalog.
      if (typeof descriptor.raster_url_template === 'string') {
        const outcome = await page.evaluate(
          async ({ urlTemplate, sourceName, maximumZoomLevel }) => {
            try {
              window.mapTest.clearProviderRegistrations();
              window.mapTest.registerUrlTemplateRasterTileSource({
                urlTemplate,
                sourceName,
                minimumZoomLevel: 0,
                maximumZoomLevel,
                imageFormat: 'png',
                tileResolution: 'standard',
                userAgent: navigator.userAgent,
              });
              const bytes = await window.mapTest.fetchRasterTile(13, 1330, 3141);
              return bytes.length > 0
                ? [true, `${bytes.length} tile byte(s)`]
                : [false, 'empty tile'];
            } catch (error) {
              return [false, String(error?.message ?? error)];
            }
          },
          {
            urlTemplate: descriptor.raster_url_template,
            sourceName: descriptor.provider_key,
            maximumZoomLevel: descriptor.maximum_zoom_level ?? 19,
          },
        );
        results.push([
          descriptor.display_name,
          'raster_tiles',
          outcome[0] ? 'ok' : 'fail',
          redact(outcome[1]),
        ]);
      } else if (typeof descriptor.url_template === 'string') {
        results.push([
          descriptor.display_name,
          'vector_tiles',
          'skipped',
          'base-map source set through the map surface',
        ]);
      }
      continue;
    }
    for (const contractKey of contractKeys) {
      const contractName = CONTRACT_NAMES[contractKey];
      const credential = descriptor.credential_label === '' ? deployment : key;
      const outcome = await page.evaluate(
        async ({ contractName, providerKeyValue, credentialValue, contractKey }) => {
          try {
            window.mapTest.clearProviderRegistrations();
            window.mapTest.configureProvider(contractName, providerKeyValue, credentialValue);
            if (contractKey === 'geocoding') {
              const places = await window.mapTest.forwardGeocode('Brandenburg Gate, Berlin');
              return places.length > 0 ? [true, `${places.length} place(s)`] : [false, 'no places'];
            }
            if (contractKey === 'place_search') {
              const places = await window.mapTest.searchPlaces('Sacramento');
              const suggestions = await window.mapTest.suggestPlaces('Sacra');
              return places.length > 0 || suggestions.length > 0
                ? [true, `${places.length} search, ${suggestions.length} suggestion(s)`]
                : [false, 'no results'];
            }
            if (contractKey === 'routing') {
              const route = await window.mapTest.calculateRoute(52.5163, 13.3777, 52.52, 13.39);
              const points = Array.isArray(route?.geometry) ? route.geometry.length : 0;
              return points > 0
                ? [true, `${points} geometry point(s), ${Math.round(route.distanceMeters)} m`]
                : [false, 'no route geometry'];
            }
            if (contractKey === 'distance_matrix') {
              const entries = await window.mapTest.calculateDistanceMatrix(
                [52.5163, 13.3777],
                [52.52, 13.39],
                'Automobile',
              );
              return entries.length > 0
                ? [true, `${entries.length} matrix cell(s)`]
                : [false, 'no matrix cells'];
            }
            if (contractKey === 'traffic_incidents') {
              const incidents = await window.mapTest.fetchTrafficIncidents(...[52.5, 13.35, 52.54, 13.43]);
              return [true, `${incidents.length} incident(s)`];
            }
            if (contractKey === 'traffic_flow') {
              const flow = await window.mapTest.fetchTrafficFlow(...[52.5, 13.35, 52.54, 13.43]);
              return [true, `${flow.length} flow segment(s)`];
            }
            if (contractKey === 'raster_tiles') {
              const bytes = await window.mapTest.fetchRasterTile(13, 1330, 3141);
              return bytes.length > 0 ? [true, `${bytes.length} tile byte(s)`] : [false, 'empty tile'];
            }
            return [true, 'configured'];
          } catch (error) {
            return [false, String(error?.message ?? error)];
          }
        },
        {
          contractName,
          providerKeyValue: descriptor.provider_key,
          credentialValue: credential,
          contractKey,
        },
      );
      results.push([descriptor.display_name, contractKey, outcome[0] ? 'ok' : 'fail', redact(outcome[1])]);
    }
  }

  await browser.close();
  const width = Math.max(...results.map((row) => row[0].length));
  for (const [provider, contract, status, detail] of results) {
    console.log(`${provider.padEnd(width)}  ${contract.padEnd(18)} ${status.padEnd(7)} ${detail}`);
  }
  const failures = results.filter((row) => row[2] === 'fail').length;
  console.log(`\n${results.length} check(s), ${failures} failure(s)`);
  if (strict && failures > 0) {
    process.exit(1);
  }
}

main().catch((error) => {
  console.error('smoke failed:', redact(error));
  process.exit(1);
});
