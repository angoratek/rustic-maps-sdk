#!/usr/bin/env node
// The React Native pod ships its own copy of the SwiftUI-independent map
// widget (`platform/rn/ios/MapsSdkRusticMapView.swift`) because the pod module
// must not import the SwiftPM module `RusticMapsSDK`. The SwiftPM source
// (`platform/ios/Sources/MapNativeCoreUI/MapView.swift`) is authoritative:
// this script applies the declared rename map to produce the pod copy and
// refuses drift, so an API addition can never land in one copy only.
//
// Usage:
//   node scripts/sync-ios-pod-view.mjs           # verify (exits 1 on drift)
//   node scripts/sync-ios-pod-view.mjs --write   # regenerate the pod copy
import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPOSITORY_ROOT = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);
/** The SwiftPM source that owns the widget. */
const SWIFTPM_VIEW_RELATIVE_PATH = 'platform/ios/Sources/MapNativeCoreUI/MapView.swift';
/** The generated pod copy that must match it. */
const POD_VIEW_RELATIVE_PATH = 'platform/rn/ios/MapsSdkRusticMapView.swift';

/**
 * The pod module cannot import `RusticMapsSDK`, so the two public names the
 * widget collides with are qualified in the SwiftPM source and unqualified
 * here, and every wrapper symbol gains the `MapsSdk` prefix the pod's
 * generated API uses.
 */
const POD_COPY_HEADER_LINES = [
  '//! Pod copy of `platform/ios/Sources/MapNativeCoreUI/MapView.swift` (the',
  '//! `MapsSdkRusticMapView` widget), renamed to leave the generated binding\'s',
  '//! `MapView` type unambiguous inside the package module. The two copies are',
  '//! kept in sync by the iOS packaging slice; the SwiftPM package and this pod',
  '//! intentionally expose the same widget.',
];

/** Ordered longest-first so a shorter name never splits a longer one. */
const PUBLIC_SYMBOL_RENAMES = [
  ['MapViewFailurePhase', 'MapsSdkMapViewFailurePhase'],
  ['MapViewFailure', 'MapsSdkMapViewFailure'],
  ['MapViewDelegate', 'MapsSdkMapViewDelegate'],
  ['MapCameraPose', 'MapsSdkCameraPose'],
  ['MapView', 'MapsSdkRusticMapView'],
];

/** The SwiftPM module import that the pod module cannot carry. */
const SWIFTPM_MODULE_IMPORT = 'import RusticMapsSDK';
/** Placeholder for the generated `MapView` the pod refers to unqualified. */
const GENERATED_MAP_VIEW_PLACEHOLDER = '\u0000generated-map-view\u0000';

/**
 * Produces the pod copy from the SwiftPM source. The only structural change
 * is dropping the module import; everything else is the declared rename map
 * applied to whole identifiers.
 */
export function renderPodViewCopy(swiftpm_source) {
  const without_module_import = swiftpm_source
    .split('\n')
    .filter((line) => line.trim() !== SWIFTPM_MODULE_IMPORT)
    .join('\n');
  const with_placeholder = without_module_import
    .split('RusticMapsSDK.MapView')
    .join(GENERATED_MAP_VIEW_PLACEHOLDER);
  let renamed = with_placeholder;
  for (const [spm_name, pod_name] of PUBLIC_SYMBOL_RENAMES) {
    renamed = renamed.replace(new RegExp(`\\b${spm_name}\\b`, 'g'), pod_name);
  }
  const with_generated_type = renamed
    .split(GENERATED_MAP_VIEW_PLACEHOLDER)
    .join('MapView');
  return `${POD_COPY_HEADER_LINES.join('\n')}\n${with_generated_type}`;
}

/**
 * Compares both copies and returns the first divergent line, or undefined
 * when they agree.
 */
export function findPodViewDrift(swiftpm_source, pod_source) {
  const expected_lines = renderPodViewCopy(swiftpm_source).split('\n');
  const actual_lines = pod_source.split('\n');
  const line_count = Math.max(expected_lines.length, actual_lines.length);
  for (let line_index = 0; line_index < line_count; line_index += 1) {
    if (expected_lines[line_index] !== actual_lines[line_index]) {
      return {
        lineNumber: line_index + 1,
        expectedLine: expected_lines[line_index],
        actualLine: actual_lines[line_index],
      };
    }
  }
  return undefined;
}

function readMirrorSources() {
  return {
    swiftpm_source: readFileSync(
      path.join(REPOSITORY_ROOT, SWIFTPM_VIEW_RELATIVE_PATH),
      'utf8',
    ),
    pod_source: readFileSync(
      path.join(REPOSITORY_ROOT, POD_VIEW_RELATIVE_PATH),
      'utf8',
    ),
  };
}

/**
 * Throws when the pod copy has drifted from its SwiftPM source, so a suite
 * gate (the Playwright global setup) fails the same way the script does.
 */
export function verifyPodViewMirror() {
  const { swiftpm_source, pod_source } = readMirrorSources();
  const drift = findPodViewDrift(swiftpm_source, pod_source);
  if (drift === undefined) {
    return;
  }
  throw new Error(
    `${POD_VIEW_RELATIVE_PATH}:${drift.lineNumber} has drifted from ` +
      `${SWIFTPM_VIEW_RELATIVE_PATH}\n` +
      `  expected: ${drift.expectedLine ?? '(end of file)'}\n` +
      `  actual:   ${drift.actualLine ?? '(end of file)'}\n` +
      'run `node scripts/sync-ios-pod-view.mjs --write` to refresh the pod copy',
  );
}

function main() {
  const should_write = process.argv.includes('--write');
  if (should_write) {
    const swiftpm_source = readFileSync(
      path.join(REPOSITORY_ROOT, SWIFTPM_VIEW_RELATIVE_PATH),
      'utf8',
    );
    writeFileSync(
      path.join(REPOSITORY_ROOT, POD_VIEW_RELATIVE_PATH),
      renderPodViewCopy(swiftpm_source),
    );
    console.log(`wrote ${POD_VIEW_RELATIVE_PATH}`);
    return;
  }
  try {
    verifyPodViewMirror();
    console.log(`${POD_VIEW_RELATIVE_PATH} mirrors ${SWIFTPM_VIEW_RELATIVE_PATH}`);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main();
}
