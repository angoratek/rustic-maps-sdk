#!/usr/bin/env node
// Captures the four published React Native reference-application screenshots
// from one keyed run of the Android shell: the plan sheet with both endpoints
// chosen, the provider section with the applied provider serving every
// capability, the navigation frame at the first instruction with the 1× speed
// selected, and the styled vector base with its place labels.
//
// The runner drives the same public controls a visitor uses through
// `uiautomator dump` and `adb shell input`; it is the shell's capture seam,
// not a second application. The credential reaches the app through the
// documented launch-intent extra (`providerApiKey`), is applied through the
// public registration path, and is never persisted or logged by the runner.
//
// Preconditions: the standing Pixel_9 emulator is booted, the debug app for
// this checkout is installed, and Metro serves the app shell on port 8081.
//
//   PROVIDER_API_KEY=… PROVIDER_KEY=mapquest \
//     node platform/rn/app-shell/scripts/capture_reference_screenshots.mjs
//
// A session credential is read from PROVIDER_API_KEY and passed to adb as an
// argv value, never through a shell string. Output lands under
// `tmp/reference-screenshots/react-native/` for review; only a reviewed set
// from one revision is copied into `site/book-src/img/`.
import { execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPOSITORY_ROOT = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../../../..',
);
const APP_SHELL_ROOT = path.join(REPOSITORY_ROOT, 'platform/rn/app-shell');
const OUTPUT_DIRECTORY = path.join(
  REPOSITORY_ROOT, 'tmp/reference-screenshots/react-native',
);
const PROVIDER_REGISTRY = JSON.parse(
  readFileSync(path.join(REPOSITORY_ROOT, 'platform/providers.json'), 'utf8'),
);
const VECTOR_STYLE_ID = PROVIDER_REGISTRY.keyless_base_map.style_id;
/** The first registry provider that declares a credential on this platform. */
const CREDENTIAL_PROVIDER_KEY = PROVIDER_REGISTRY.providers.find(
  (providerDescriptor) =>
    (providerDescriptor.credential_label ?? '').trim() !== ''
    && providerDescriptor.registration_available?.react_native === true,
)?.provider_key;
if (CREDENTIAL_PROVIDER_KEY === undefined) {
  throw new Error('the registry declares no credentialed provider for React Native');
}

/** The debug app's application id, read from its Gradle module. */
const APPLICATION_ID = /applicationId\s+"([^"]+)"/.exec(
  readFileSync(
    path.join(APP_SHELL_ROOT, 'android/app/build.gradle'),
    'utf8',
  ),
)?.[1];
if (APPLICATION_ID === undefined) {
  throw new Error('the app shell Gradle module declares no applicationId');
}

const PROVIDER_CREDENTIAL = process.env.PROVIDER_API_KEY?.trim() ?? '';
const PROVIDER_KEY = process.env.PROVIDER_KEY?.trim() ?? '';
if (PROVIDER_CREDENTIAL === '') {
  throw new Error('the keyed capture needs PROVIDER_API_KEY in the environment');
}

const READY_TIMEOUT_MILLISECONDS = 120_000;
const UI_TIMEOUT_MILLISECONDS = 30_000;
const VECTOR_SETTLE_MILLISECONDS = 12_000;
const FRAME_SETTLE_MILLISECONDS = 1_500;
const POLL_INTERVAL_MILLISECONDS = 250;
const VECTOR_STYLE_ROW_ID = `map-layer-style-${VECTOR_STYLE_ID}`;
/** The raster base row serving the keyed frames (the applied provider's). */
const RASTER_BASE_ROW_ID =
  `map-layer-base-${PROVIDER_KEY !== '' ? PROVIDER_KEY : CREDENTIAL_PROVIDER_KEY}`;

/** The device's display size, read once. */
const SCREEN = (() => {
  const output = adb('shell', 'wm', 'size').toString('utf8');
  const match = /(\d+)x(\d+)/.exec(output);
  if (match === null) throw new Error('could not read the device display size');
  return { width: Number(match[1]), height: Number(match[2]) };
})();

/** Runs one adb invocation with argv values (no shell interpolation). */
function adb(...argumentsList) {
  return execFileSync('adb', argumentsList, {
    encoding: 'buffer',
    maxBuffer: 64 * 1024 * 1024,
  });
}

/** One parsed accessibility node. */
function parseNodes(xml) {
  const nodes = [];
  for (const match of xml.matchAll(/<node\b[^>]*>/g)) {
    const tag = match[0];
    const attribute = (name) =>
      new RegExp(`${name}="([^"]*)"`).exec(tag)?.[1] ?? '';
    const boundsMatch = /\[(\d+),(\d+)\]\[(\d+),(\d+)\]/.exec(attribute('bounds'));
    if (boundsMatch === null) continue;
    const [, left, top, right, bottom] = boundsMatch.map(Number);
    nodes.push({
      identifier: attribute('resource-id'),
      text: attribute('text'),
      contentDescription: attribute('content-desc'),
      centerX: Math.round((left + right) / 2),
      centerY: Math.round((top + bottom) / 2),
    });
  }
  return nodes;
}

/** The current accessibility tree. */
function dumpNodes() {
  adb('shell', 'uiautomator', 'dump', '/sdcard/window.xml');
  const xml = adb('exec-out', 'cat', '/sdcard/window.xml').toString('utf8');
  return parseNodes(xml);
}

/** Polls until the predicate holds, failing with the named description. */
function waitFor(description, predicate, timeoutMilliseconds = UI_TIMEOUT_MILLISECONDS) {
  const deadline = Date.now() + timeoutMilliseconds;
  for (;;) {
    if (predicate(dumpNodes())) return;
    if (Date.now() >= deadline) {
      throw new Error(`timed out waiting for ${description}`);
    }
    execFileSync('sleep', ['0.25']);
  }
}

/** Waits for one node by test id. */
function waitForIdentifier(identifier, timeoutMilliseconds = UI_TIMEOUT_MILLISECONDS) {
  waitFor(
    `the "${identifier}" control`,
    (nodes) => nodes.some((node) => node.identifier === identifier),
    timeoutMilliseconds,
  );
}

/** Whether one test id is currently in the accessibility tree. */
function hasIdentifier(identifier) {
  return dumpNodes().some((node) => node.identifier === identifier);
}

/** Scrolls the settings sheet's content one step (up shows later rows). */
function scrollSettingsContent(upward) {
  const startY = Math.round(SCREEN.height * (upward ? 0.72 : 0.38));
  const endY = Math.round(SCREEN.height * (upward ? 0.38 : 0.72));
  adb('shell', 'input', 'swipe',
    String(Math.round(SCREEN.width / 2)), String(startY),
    String(Math.round(SCREEN.width / 2)), String(endY), '250');
  execFileSync('sleep', ['0.6']);
}

/** Scrolls the open layers panel toward its end (the lower sections). */
function scrollLayersPanelTowardItsEnd() {
  const startY = Math.round(SCREEN.height * 0.62);
  const endY = Math.round(SCREEN.height * 0.24);
  adb('shell', 'input', 'swipe',
    String(Math.round(SCREEN.width / 2)), String(startY),
    String(Math.round(SCREEN.width / 2)), String(endY), '800');
  execFileSync('sleep', ['1.2']);
}

/** Scrolls the settings sheet until one control is reachable. */
function scrollSettingsTo(identifier) {
  for (let attempt = 0; attempt < 8; attempt += 1) {
    if (hasIdentifier(identifier)) return;
    scrollSettingsContent(true);
  }
  throw new Error(`the settings sheet never showed "${identifier}"`);
}

/** Taps one node by test id. */
function tapIdentifier(identifier) {
  waitForIdentifier(identifier);
  const node = dumpNodes().find((candidate) => candidate.identifier === identifier);
  adb('shell', 'input', 'tap', String(node.centerX), String(node.centerY));
}

/** Types one text value into the focused field. */
function typeText(value) {
  adb('shell', 'input', 'text', value.replaceAll(' ', '%s'));
}

/** Hides the soft keyboard without leaving the current surface. */
function hideKeyboard() {
  adb('shell', 'input', 'keyevent', '4');
}

/** Writes one full-screen PNG. */
function captureScreenshot(fileName) {
  const png = adb('exec-out', 'screencap', '-p');
  writeFileSync(path.join(OUTPUT_DIRECTORY, fileName), png);
}

/** Launches a clean app session with the session credential on its intent. */
function launchApplication() {
  adb('shell', 'pm', 'clear', APPLICATION_ID);
  adb('reverse', 'tcp:8081', 'tcp:8081');
  const intentArguments = [
    'shell', 'am', 'start', '-W', '-n', `${APPLICATION_ID}/.MainActivity`,
    '-e', 'providerApiKey', PROVIDER_CREDENTIAL,
  ];
  if (PROVIDER_KEY !== '') {
    intentArguments.push('-e', 'providerKey', PROVIDER_KEY);
  }
  adb(...intentArguments);
}

/** Awaits the keyed session's readiness chip. */
function awaitReady() {
  waitFor(
    'the readiness chip to report Ready',
    (nodes) => nodes.some((node) =>
      node.identifier === 'readiness-chip-text' && node.text.includes('Ready')),
    READY_TIMEOUT_MILLISECONDS,
  );
}

/**
 * Applies the session credential through the settings surface. The launch
 * intent also applies it natively; the settings apply is the public path that
 * updates the rendered rows without exposing the value back to JavaScript.
 */
function applyProviderCredential() {
  const credentialInputIdentifier =
    `provider-credential-input-${CREDENTIAL_PROVIDER_KEY}`;
  tapIdentifier('open-settings');
  waitForIdentifier('settings-panel');
  // The capability rows open at the top; the credential form is below them.
  scrollSettingsTo(credentialInputIdentifier);
  tapIdentifier(credentialInputIdentifier);
  typeText(PROVIDER_CREDENTIAL);
  hideKeyboard();
  execFileSync('sleep', ['0.75']);
  tapIdentifier(`provider-credential-apply-${CREDENTIAL_PROVIDER_KEY}`);
  waitFor(
    'the applied credential status',
    (nodes) => nodes.some((node) =>
      node.identifier === `provider-credential-status-${CREDENTIAL_PROVIDER_KEY}`
      && /applied/i.test(node.text)),
    UI_TIMEOUT_MILLISECONDS,
  );
  tapIdentifier('settings-done');
  execFileSync('sleep', ['1']);
}

/** Captures the provider section with every capability row's state. */
function captureProviderSection() {
  tapIdentifier('open-settings');
  waitForIdentifier('settings-panel');
  execFileSync('sleep', ['1.5']);
  captureScreenshot('react-native-provider-section.png');
  tapIdentifier('settings-done');
  execFileSync('sleep', ['1']);
}

/** Captures the plan sheet with both endpoint fields filled. */
function capturePlanSheet() {
  tapIdentifier('peek-search-field');
  waitForIdentifier('origin-search-field');
  tapIdentifier('origin-search-field');
  typeText('Capitol Park');
  // Hide the keyboard before reaching for the second field; otherwise the
  // tap lands on the IME and the text appends to the focused origin.
  hideKeyboard();
  execFileSync('sleep', ['1']);
  tapIdentifier('destination-search-field');
  typeText('Tower Bridge');
  hideKeyboard();
  execFileSync('sleep', ['1.5']);
  captureScreenshot('react-native-plan-sheet.png');
  // Leave the search view through its own Done action.
  tapIdentifier('search-done');
  execFileSync('sleep', ['1']);
}

/**
 * Captures the layers control pair over the credential's raster base: the
 * same camera with the panel closed, then open with every section and the
 * clear action.
 */
function captureLayersPanelFrames() {
  captureScreenshot('react-native-layers-closed.png');
  tapIdentifier('map-layers-control');
  waitForIdentifier('map-layers-panel');
  // The serving row proves the frames show the credential provider's raster
  // base, not a styled base.
  waitFor(
    'the credential provider serving its raster base',
    (nodes) => nodes.some((node) =>
      node.identifier === RASTER_BASE_ROW_ID
      && node.contentDescription.includes('Serving')),
  );
  // The panel scrolls on a phone: the open frame shows its last base rows
  // with the traffic, overlays, clear and device-location sections below
  // them, so every section and the shared clear action are in one frame.
  scrollLayersPanelTowardItsEnd();
  scrollLayersPanelTowardItsEnd();
  waitForIdentifier('map-layer-clear-overlays');
  waitForIdentifier('device-location-status');
  waitFor(
    'the overlays and device-location section titles',
    (nodes) => ['Traffic', 'Overlays', 'Device location'].every((sectionTitle) =>
      nodes.some((node) => node.text === sectionTitle)),
  );
  // The last base rows remain in the frame above the traffic switch.
  waitFor(
    'a base-map row in the frame',
    (nodes) => nodes.some((node) =>
      node.identifier.startsWith('map-layer-base-')
      || node.identifier.startsWith('map-layer-style-')),
  );
  execFileSync('sleep', [String(FRAME_SETTLE_MILLISECONDS / 1000)]);
  captureScreenshot('react-native-layers-open.png');
  tapIdentifier('map-layers-close');
  execFileSync('sleep', ['1']);
}

/** Captures the styled vector base through the layers panel's own row. */
function captureVectorBase() {
  tapIdentifier('map-layers-control');
  waitForIdentifier(VECTOR_STYLE_ROW_ID);
  tapIdentifier(VECTOR_STYLE_ROW_ID);
  // Prove the frame draws the registry's styled base, not the raster source
  // the credential registered underneath it: the tapped style row carries
  // the panel's Serving mark.
  waitFor(
    'the styled base serving mark',
    (nodes) => nodes.some((node) =>
      (node.identifier === VECTOR_STYLE_ROW_ID
        && node.contentDescription.includes('Serving'))
      || (node.text.includes('Serving') && node.text.includes('Bright'))),
    UI_TIMEOUT_MILLISECONDS,
  );
  tapIdentifier('map-layers-close');
  execFileSync('sleep', [String(VECTOR_SETTLE_MILLISECONDS / 1000)]);
  captureScreenshot('react-native-vector-base.png');
}

/** Captures the navigation frame at the first instruction and 1× speed. */
function captureNavigationFrame() {
  tapIdentifier('start-simulated-navigation');
  // The first instruction is the published marker and the default playback
  // factor is 1×. Capture on the next drawn frames: a tree dump here costs
  // seconds, long enough for the accelerated trace to reach its arrival
  // card and hide the playback controls.
  execFileSync('sleep', ['0.75']);
  captureScreenshot('react-native-navigation.png');
}

mkdirSync(OUTPUT_DIRECTORY, { recursive: true });
launchApplication();
awaitReady();
applyProviderCredential();
captureLayersPanelFrames();
captureVectorBase();
captureProviderSection();
capturePlanSheet();
captureNavigationFrame();
console.log(`captured the React Native set in ${OUTPUT_DIRECTORY}`);
