// Playwright global setup: runs the pure file-parsing gates once before the
// browser suite starts — the contract-sync test (the frozen UniFFI contract's
// TypeScript/wasm mirror, `tests/contract_sync.mjs`), the provider-name guard
// (`tests/provider_name_guard.mjs`, the M15 allowlist), the platform-parity
// floor (`tests/platform_parity_gate.mjs`, M31), the iOS pod/SwiftPM source
// mirror (`scripts/sync-ios-pod-view.mjs`), the WebAssembly package freshness
// gate (a package emitted from different sources can never be tested), the
// npm packaging assertion (`tests/npm_package_manifest.mjs`: the packed
// tarball carries every declaration the manifest publishes) and the style
// layer coverage audit (`scripts/check-style-layer-coverage.mjs`: every
// name/ref-bearing source layer is painted or documented). It then asserts a
// WebGPU adapter in CI, so a GPU-dependent test cannot pass by skipping on
// a runner that renders nothing.
import { verifyPodViewMirror } from '../../scripts/sync-ios-pod-view.mjs';
import { verifyWasmPackageFreshness } from '../../scripts/check-wasm-package-freshness.mjs';
import { run_reference_flow_checks } from '../../scripts/check-reference-flows.mjs';
import { run_style_layer_coverage_checks } from '../../scripts/check-style-layer-coverage.mjs';
import { run_contract_sync_checks } from './tests/contract_sync.mjs';
import { assertGpuAdapterAvailable } from './tests/gpu_preflight.mjs';
import { run_npm_package_manifest_checks } from './tests/npm_package_manifest.mjs';
import { run_platform_parity_checks } from './tests/platform_parity_gate.mjs';
import { run_provider_name_guard_checks } from './tests/provider_name_guard.mjs';

export default async function run_file_parsing_gates() {
  run_contract_sync_checks();
  run_provider_name_guard_checks();
  run_platform_parity_checks();
  run_style_layer_coverage_checks();
  run_reference_flow_checks();
  verifyPodViewMirror();
  verifyWasmPackageFreshness();
  run_npm_package_manifest_checks();
  await assertGpuAdapterAvailable();
}
