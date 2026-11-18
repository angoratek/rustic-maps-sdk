// Playwright configuration for the browser harness: a dependency-free static
// server from `serve.mjs` and the system Chromium channel at device scale one.
// The launch flags enable WebGPU in the headless Chromium build (the smoke
// test degrades cleanly to the documented overlay path when they cannot).
import { defineConfig } from '@playwright/test';
import { HOST, PORT } from './serve.mjs';

const TEST_DIRECTORY = './tests';
// The browser launch flags live in `browser_launch_options.mjs` so the CI GPU
// preflight launches the same browser the suite does.
import { BROWSER_LAUNCH_OPTIONS } from './browser_launch_options.mjs';

export default defineConfig({
  testDir: TEST_DIRECTORY,
  // One retry on the CI runner: several cases drive an asynchronous
  // tile/label settle, and a software-rendered share-of-a-core runner can
  // miss a poll window that never reproduces standalone. Local runs stay
  // strict so a real regression fails on the first attempt.
  retries: process.env.CI ? 1 : 0,
  // One worker: every page owns a WebGPU device, and parallel browser
  // processes sharing one GPU contend for adapter and device creation until a
  // readiness wait expires (verified: the same file passes 15/15 serially and
  // fails under three concurrent workers). The suite is dominated by the
  // deliberate 30-second fetch-deadline case; a reliable gate is worth the
  // extra wall-clock time.
  workers: 1,
  // Asserts the frozen UniFFI contract's wasm/TypeScript mirror
  // (`tests/contract_sync.mjs`) before any browser test runs; pure file
  // parsing, no server involved.
  globalSetup: './global_setup.mjs',
  use: {
    channel: 'chromium',
    baseURL: `http://${HOST}:${PORT}`,
    viewport: { width: 640, height: 480 },
    contextOptions: { deviceScaleFactor: 1 },
    launchOptions: BROWSER_LAUNCH_OPTIONS,
  },
  webServer: {
    command: 'node serve.mjs',
    url: `http://${HOST}:${PORT}/`,
    reuseExistingServer: false,
  },
});