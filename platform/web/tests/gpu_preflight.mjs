// The CI GPU preflight: a GPU-dependent test must fail the job when the
// runner offers no WebGPU adapter instead of skipping, which would let a
// green suite prove nothing about the rendered path. Local runs and explicit
// `ALLOW_MISSING_GPU=1` invocations keep the documented degradation tests
// runnable on CPU-only machines.
import { chromium } from '@playwright/test';
import { BROWSER_LAUNCH_OPTIONS } from '../browser_launch_options.mjs';

/** Environment value that downgrades the preflight to a warning. */
const ALLOW_MISSING_GPU_ENVIRONMENT_NAME = 'ALLOW_MISSING_GPU';

// The first adapter request after a cold launch can resolve to null while the
// GPU process brings the software device up, so a single null is not yet a
// verdict. Twenty half-second attempts cover the observed startup window.
const ADAPTER_REQUEST_ATTEMPT_LIMIT = 20;
const ADAPTER_REQUEST_RETRY_DELAY_MILLISECONDS = 500;

/**
 * Asserts a WebGPU adapter is available in CI. No-op outside CI.
 *
 * @throws {Error} in CI when the browser reports no adapter and the caller
 *   did not set `ALLOW_MISSING_GPU=1`.
 */
export async function assertGpuAdapterAvailable() {
  if (process.env.CI !== 'true' && process.env.CI !== '1') {
    return;
  }
  const browser = await chromium.launch({
    channel: 'chromium',
    ...BROWSER_LAUNCH_OPTIONS,
  });
  let adapter_available = false;
  try {
    const page = await browser.newPage();
    for (
      let attempt = 1;
      attempt <= ADAPTER_REQUEST_ATTEMPT_LIMIT && !adapter_available;
      attempt += 1
    ) {
      adapter_available = await page.evaluate(async () => {
        if (navigator.gpu === undefined) {
          return false;
        }
        try {
          return (await navigator.gpu.requestAdapter()) !== null;
        } catch {
          return false;
        }
      });
      if (!adapter_available && attempt < ADAPTER_REQUEST_ATTEMPT_LIMIT) {
        await new Promise((resolve) =>
          setTimeout(resolve, ADAPTER_REQUEST_RETRY_DELAY_MILLISECONDS),
        );
      }
    }
  } finally {
    await browser.close();
  }
  if (adapter_available) {
    return;
  }
  if (process.env[ALLOW_MISSING_GPU_ENVIRONMENT_NAME] === '1') {
    console.warn(
      'GPU preflight: no WebGPU adapter, continuing because ' +
        `${ALLOW_MISSING_GPU_ENVIRONMENT_NAME}=1`,
    );
    return;
  }
  throw new Error(
    'GPU preflight: the CI runner exposes no WebGPU adapter; the WebGPU ' +
      'tests would skip and prove nothing. Fix the runner or set ' +
      `${ALLOW_MISSING_GPU_ENVIRONMENT_NAME}=1 to assert only the degradation path.`,
  );
}
