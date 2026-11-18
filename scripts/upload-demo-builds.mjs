#!/usr/bin/env node
// Appetize build uploader (PLAN.md, M24).
//
// Builds nothing: it uploads artifacts the platform builds already produced,
// updating each app in place so its public build id never changes, and writes
// the public manifest the /demo/mobile page reads.
//
// The Appetize API token comes from the environment (`.env` for a local run,
// a repository secret in CI) and is never written to a file, never logged and
// never reaches the page. Provider credentials are not involved at all: the
// public demo runs its own mock provider.
//
// USAGE
//   node scripts/upload-demo-builds.mjs --android <app-debug.apk> \
//        [--ios <RusticMapsDemo.zip>] [--rn-android <app-debug.apk>] \
//        [--rn-ios <RusticMapsRn.zip>] [--dry-run]
//   node scripts/upload-demo-builds.mjs --reapply-settings [--dry-run]
//
// `--reapply-settings` re-applies the session settings (timeout, time limit,
// concurrency, referrer allow-list, button text) to every build already in
// the manifest, without re-uploading an artifact. Use it after the settings
// policy changes or when an account-level change dropped them.
//
// OPTIONS
//   --android <path>      Android reference-application APK.
//   --ios <path>          iOS simulator `.app` zipped (`ditto -c -k --keepParent`).
//   --rn-android <path>   React Native reference app APK (x86_64).
//   --rn-ios <path>       React Native iOS simulator `.app` zipped.
//   --manifest <path>     Manifest to update (default `platform/demo-builds.json`).
//   --dry-run             Print what would be uploaded; touch nothing.
//
// Each upload sends the documented Appetize fields: a 180-second inactivity
// timeout, a 180-second session time limit (the same three minutes the claim
// endpoint budgets), one concurrent session and the docs-site referrer
// restriction, so a hotlinked embed cannot start.

import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const SCRIPT_DIRECTORY = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(SCRIPT_DIRECTORY, '..');

// The hostnames an Appetize session may start from. The documentation site is
// served from the GitHub Pages project origin, which is where the hosted-demo
// page runs. `appetize.io` is required too: the public app page's own session
// request carries an `appetize.io` referer, and without the account's own host
// in the list Appetize refuses even a visitor who presses Start demo on its
// page with "This app may not be embedded on this website" (verified
// 2026-10-31 over the player's websocket, which reports the refusal as a
// `userError`).
const DEMO_REFERRER_HOSTNAMES = [
  'angoratek.github.io',
  'appetize.io',
];

/** The session budget the claim endpoint enforces, in seconds. */
const DEMO_SESSION_SECONDS = 180;

/** The inactivity timeout before Appetize stops a session, in seconds. */
const DEMO_INACTIVITY_TIMEOUT_SECONDS = 180;

/** The concurrent sessions one app may run (the Free plan allows two). */
const DEMO_MAXIMUM_CONCURRENT_SESSIONS = 1;

/** The Appetize REST endpoint for creating and updating apps. */
const APPETIZE_APPS_ENDPOINT = 'https://api.appetize.io/v1/apps';

/** The manifest entry each upload target updates. */
const MANIFEST_KEY_BY_TARGET = new Map([
  ['android', 'android'],
  ['ios', 'ios'],
  ['rn-android', 'reactNativeAndroid'],
  ['rn-ios', 'reactNativeIos'],
]);

/** The Appetize `platform` field each target uploads as. */
const APPETIZE_PLATFORM_BY_TARGET = new Map([
  ['android', 'android'],
  ['ios', 'ios'],
  ['rn-android', 'android'],
  ['rn-ios', 'ios'],
]);

function fail(message) {
  process.stderr.write(`error: ${message}\n`);
  process.exit(2);
}

function parseArguments(argv) {
  const options = {
    manifestPath: 'platform/demo-builds.json',
    dryRun: false,
    reapplySettings: false,
    uploads: [],
  };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === '--dry-run') {
      options.dryRun = true;
      continue;
    }
    if (argument === '--reapply-settings') {
      options.reapplySettings = true;
      continue;
    }
    if (argument === '--manifest') {
      index += 1;
      if (index >= argv.length) {
        fail('--manifest needs a path');
      }
      options.manifestPath = argv[index];
      continue;
    }
    const target = argument.replace(/^--/, '');
    if (!MANIFEST_KEY_BY_TARGET.has(target)) {
      fail(`unknown option: ${argument}`);
    }
    index += 1;
    if (index >= argv.length) {
      fail(`${argument} needs a file path`);
    }
    options.uploads.push({ target, artifactPath: argv[index] });
  }
  if (options.uploads.length === 0 && !options.reapplySettings) {
    fail(
      'nothing to upload: pass at least one of --android, --ios, --rn-android, ' +
        '--rn-ios, or --reapply-settings',
    );
  }
  return options;
}

/** Reads the manifest, failing loudly rather than silently starting empty. */
function readManifest(manifestPath) {
  const absolutePath = path.resolve(REPO_ROOT, manifestPath);
  if (!existsSync(absolutePath)) {
    fail(`manifest not found: ${absolutePath}`);
  }
  return { absolutePath, manifest: JSON.parse(readFileSync(absolutePath, 'utf8')) };
}

/** The git commit the artifacts were built from, or null outside a checkout. */
function sourceCommit() {
  try {
    return execFileSync('git', ['rev-parse', 'HEAD'], {
      cwd: REPO_ROOT,
      encoding: 'utf8',
    }).trim();
  } catch {
    return null;
  }
}

/**
 * Uploads one artifact through the documented multipart form.
 *
 * An update (`POST /v1/apps/{publicKey}`) keeps the public build id stable, so
 * the page's manifest entry never changes; a create (`POST /v1/apps`) is used
 * only the first time an entry has no build id yet.
 */
function uploadArtifact({ target, artifactPath, token, existingBuildId }) {
  const absoluteArtifact = path.resolve(REPO_ROOT, artifactPath);
  if (!existsSync(absoluteArtifact)) {
    fail(`artifact not found: ${absoluteArtifact}`);
  }
  const endpoint = existingBuildId
    ? `${APPETIZE_APPS_ENDPOINT}/${existingBuildId}`
    : APPETIZE_APPS_ENDPOINT;
  const formFields = [
    `file=@${absoluteArtifact}`,
    `platform=${APPETIZE_PLATFORM_BY_TARGET.get(target)}`,
    `timeout=${DEMO_INACTIVITY_TIMEOUT_SECONDS}`,
    `timeLimit=${DEMO_SESSION_SECONDS}`,
    `maxConcurrent=${DEMO_MAXIMUM_CONCURRENT_SESSIONS}`,
    `buttonText=Start demo`,
  ];
  const argumentList = [
    '--silent',
    '--show-error',
    '--fail-with-body',
    '-X',
    'POST',
    '-H',
    `X-API-KEY: ${token}`,
    endpoint,
  ];
  for (const formField of formFields) {
    argumentList.push('-F', formField);
  }
  // The token travels in the `X-API-KEY` header, never in a file, a query
  // string or the manifest. Like any command-line secret it is visible to the
  // local process table while the upload runs; this is a maintainer and CI
  // tool, not something a visitor's browser ever calls.
  const output = execFileSync('curl', argumentList, { encoding: 'utf8' });
  let response;
  try {
    response = JSON.parse(output);
  } catch {
    fail(`unexpected Appetize response for ${target}: ${output.slice(0, 400)}`);
  }
  if (typeof response.publicKey !== 'string' || response.publicKey === '') {
    fail(`the Appetize response for ${target} carries no publicKey`);
  }
  if (typeof response.versionCode !== 'number') {
    fail(`the Appetize response for ${target} carries no versionCode`);
  }
  return response;
}

/**
 * Re-applies the session settings to an uploaded app as a JSON body.
 *
 * The multipart upload silently drops `referrerHostnamesRestricted` — both
 * apps came back with an empty list after the first publish, which left the
 * demo embeddable from any site. The same fields sent as a JSON body stick,
 * so the restriction is applied in its own call and verified by reading the
 * app back.
 */
function applySessionSettings({ buildId, token, target }) {
  const settings = {
    timeout: DEMO_INACTIVITY_TIMEOUT_SECONDS,
    timeLimit: DEMO_SESSION_SECONDS,
    maxConcurrent: DEMO_MAXIMUM_CONCURRENT_SESSIONS,
    referrerHostnamesRestricted: DEMO_REFERRER_HOSTNAMES,
    buttonText: 'Start demo',
  };
  execFileSync(
    'curl',
    [
      '--silent',
      '--show-error',
      '--fail-with-body',
      '-X',
      'POST',
      '-H',
      `X-API-KEY: ${token}`,
      '-H',
      'content-type: application/json',
      '-d',
      JSON.stringify(settings),
      `${APPETIZE_APPS_ENDPOINT}/${buildId}`,
    ],
    { encoding: 'utf8' },
  );
  const readBack = JSON.parse(
    execFileSync(
      'curl',
      [
        '--silent',
        '--show-error',
        '--fail-with-body',
        '-H',
        `X-API-KEY: ${token}`,
        `${APPETIZE_APPS_ENDPOINT}/${buildId}`,
      ],
      { encoding: 'utf8' },
    ),
  );
  const applied = readBack.referrerHostnamesRestricted;
  if (
    !Array.isArray(applied) ||
    applied.length !== DEMO_REFERRER_HOSTNAMES.length ||
    !DEMO_REFERRER_HOSTNAMES.every((hostname) => applied.includes(hostname))
  ) {
    fail(
      `the referrer restriction did not stick for ${target}: ` +
        `the app reports ${JSON.stringify(applied ?? null)}`,
    );
  }
}

function main() {
  const options = parseArguments(process.argv.slice(2));
  const { absolutePath, manifest } = readManifest(options.manifestPath);
  const token = process.env.APPETIZE_TOKEN;
  if (!options.dryRun && (typeof token !== 'string' || token.trim() === '')) {
    fail('APPETIZE_TOKEN is not set (put it in .env for a local run, or in CI secrets)');
  }

  if (options.reapplySettings) {
    for (const [manifestKey, entry] of Object.entries(manifest.builds ?? {})) {
      if (typeof entry?.buildId !== 'string' || entry.buildId === '') {
        process.stdout.write(`skipped ${manifestKey}: no published build\n`);
        continue;
      }
      if (options.dryRun) {
        process.stdout.write(
          `would re-apply the session settings to ${manifestKey} (build ${entry.buildId})\n`,
        );
        continue;
      }
      applySessionSettings({ buildId: entry.buildId, token, target: manifestKey });
      process.stdout.write(
        `re-applied the session settings to ${manifestKey} (build ${entry.buildId})\n`,
      );
    }
    return;
  }

  const commit = sourceCommit();
  for (const upload of options.uploads) {
    const manifestKey = MANIFEST_KEY_BY_TARGET.get(upload.target);
    const entry = manifest.builds[manifestKey] ?? {};
    const artifactBytes = statSync(path.resolve(REPO_ROOT, upload.artifactPath)).size;
    if (options.dryRun) {
      process.stdout.write(
        `would upload ${upload.target}: ${upload.artifactPath} (${artifactBytes} bytes) ` +
          `as ${entry.buildId ? `build ${entry.buildId}` : 'a new app'}\n`,
      );
      continue;
    }
    const response = uploadArtifact({ ...upload, token, existingBuildId: entry.buildId });
    applySessionSettings({ buildId: response.publicKey, token, target: upload.target });
    manifest.builds[manifestKey] = {
      buildId: response.publicKey,
      versionCode: response.versionCode,
      updatedAt: response.updated ?? new Date().toISOString(),
    };
    process.stdout.write(
      `uploaded ${upload.target}: build ${response.publicKey} (version ${response.versionCode})\n`,
    );
  }

  if (options.dryRun) {
    return;
  }
  manifest.generatedAt = new Date().toISOString();
  manifest.sourceCommit = commit;
  writeFileSync(absolutePath, `${JSON.stringify(manifest, null, 2)}\n`);
  process.stdout.write(`wrote ${options.manifestPath}\n`);
}

main();
