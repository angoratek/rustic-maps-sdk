// Playwright global setup: builds the generated WebAssembly package when the
// tree does not carry a current one, then runs the pure file-parsing gates
// once before the browser suite starts — the contract-sync test (the frozen
// UniFFI contract's stripped WebAssembly split and the generated package
// identity, `tests/contract_sync.mjs`), the canvas declaration mirror
// (`tests/canvas_declaration_sync.mjs`: the published types name exactly what
// the glue exports), the provider-name guard
// (`tests/provider_name_guard.mjs`, the M15 allowlist), the platform-parity
// floor (`tests/platform_parity_gate.mjs`, M31), the iOS pod/SwiftPM source
// mirror (`scripts/sync-ios-pod-view.mjs`), the generated package freshness
// gate (a package emitted from different sources can never be tested), the
// npm packaging assertions (`tests/npm_package_manifest.mjs`), the style layer
// coverage audit (`scripts/check-style-layer-coverage.mjs`: every name/ref-
// bearing source layer is painted or documented) and the facade typecheck
// (`check-facade-types.mjs`: `main.mjs` checked against the generated
// declarations, so a renamed field fails before the browser runs). The
// fixture-provenance gate (`tests/fixture_provenance.mjs`) hashes every
// committed fixture against `fixtures/provenance.json` and fails on a byte
// change, an unrecorded fixture, a stale entry or a provider-layer version
// bump. It then asserts a WebGPU adapter in CI, so a GPU-dependent test
// cannot pass by skipping on a runner that renders nothing.
import { execFileSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { verifyPodViewMirror } from '../../scripts/sync-ios-pod-view.mjs';
import { verifyWasm2PackageFreshness } from '../../scripts/check-wasm2-package-freshness.mjs';
import { run_reference_flow_checks } from '../../scripts/check-reference-flows.mjs';
import { run_style_layer_coverage_checks } from '../../scripts/check-style-layer-coverage.mjs';
import { run_facade_typecheck } from './check-facade-types.mjs';
import { run_canvas_declaration_sync_checks } from './tests/canvas_declaration_sync.mjs';
import { run_contract_sync_checks } from './tests/contract_sync.mjs';
import { run_fixture_provenance_checks } from './tests/fixture_provenance.mjs';
import { assertGpuAdapterAvailable } from './tests/gpu_preflight.mjs';
import { run_npm_package_manifest_checks } from './tests/npm_package_manifest.mjs';
import { run_platform_parity_checks } from './tests/platform_parity_gate.mjs';
import { run_provider_name_guard_checks } from './tests/provider_name_guard.mjs';

const REPOSITORY_ROOT = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
  '..',
);
/** The packaging script that builds `platform/web/generated`. */
const GENERATED_PACKAGE_BUILD_SCRIPT = path.join(
  REPOSITORY_ROOT,
  'scripts',
  'build-wasm2-package.sh',
);
/** The generated bindings the packaging step compiles, reused when present. */
const GENERATED_BINDINGS_DIRECTORY = path.join(
  REPOSITORY_ROOT,
  'target',
  'wasm-bindings',
);

/** Whether the generated package is present and matches its current inputs. */
function generated_package_is_fresh() {
  try {
    verifyWasm2PackageFreshness();
    return true;
  } catch {
    return false;
  }
}

/** Runs the packaging script with the given arguments. */
function build_generated_package(script_arguments) {
  execFileSync('bash', [GENERATED_PACKAGE_BUILD_SCRIPT, ...script_arguments], {
    stdio: 'inherit',
  });
}

/**
 * Ensures `platform/web/generated` exists and was packaged from the current
 * sources. Reuses the generated bindings when they are present (the packaging
 * step alone), and falls back to the full generation only when they are not
 * or the repackage is still stale.
 */
function ensure_generated_package() {
  if (generated_package_is_fresh()) {
    return;
  }
  if (existsSync(path.join(GENERATED_BINDINGS_DIRECTORY, 'index.ts'))) {
    build_generated_package(['--no-generate']);
    if (generated_package_is_fresh()) {
      return;
    }
  }
  build_generated_package([]);
  verifyWasm2PackageFreshness();
}

export default async function run_file_parsing_gates() {
  ensure_generated_package();
  run_facade_typecheck();
  run_contract_sync_checks();
  run_canvas_declaration_sync_checks();
  run_provider_name_guard_checks();
  run_platform_parity_checks();
  run_fixture_provenance_checks();
  run_style_layer_coverage_checks();
  run_reference_flow_checks();
  verifyPodViewMirror();
  // The npm tarball stages the same generated package; assert it can be
  // packed before any browser test trusts the published shape.
  run_npm_package_manifest_checks();
  await assertGpuAdapterAvailable();
}
