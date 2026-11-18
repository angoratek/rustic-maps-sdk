// Proves the generated `wasm2` browser package loads in Chromium from the
// project's own dev server and serves both contract surfaces from one module
// instance: the generated namespace (`uniffiInitAsync`,
// `getSdkCompiledVersion`, `MapSdk.providerRegistrations`) and a DOM-free
// wasm-bindgen class from the staged `_bg.js` glue.
//
// The package is gitignored build output, so the case skips when it has not
// been built. When it is present, the source fingerprint is verified first:
// a package the current contract did not produce fails, never loads. Build it
// with `scripts/build-wasm2-package.sh` and run this file alone:
//
//   PORT=8753 npx playwright test tests/generated_wasm_package.spec.mjs
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test } from '@playwright/test';
import {
  PACKAGE_DIRECTORY,
  verifyWasm2PackageFreshness,
} from '../../../scripts/check-wasm2-package-freshness.mjs';

const REPOSITORY_ROOT = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../../..',
);
/** The contract's compiled version is the workspace version. */
const EXPECTED_COMPILED_VERSION = readFileSync(
  path.join(REPOSITORY_ROOT, 'Cargo.toml'),
  'utf8',
).match(/^version = "([^"]+)"$/m)[1];
/** One registration entry per provider capability in the contract. */
const EXPECTED_PROVIDER_REGISTRATION_COUNT = 10;
/** The circular median of the three samples the smoke page adds. */
const EXPECTED_MEDIAN_COURSE_DEGREES = 20;

test.describe('generated WebAssembly package', () => {
  test.skip(
    !existsSync(PACKAGE_DIRECTORY),
    'build the package with scripts/build-wasm2-package.sh first',
  );

  test.beforeAll(() => {
    verifyWasm2PackageFreshness();
  });

  test('loads and serves the contract surface and the wasm-bindgen glue', async ({
    page,
  }) => {
    const pageErrors = [];
    const failedRequestUrls = [];
    page.on('pageerror', (error) => pageErrors.push(String(error)));
    page.on('requestfailed', (request) => failedRequestUrls.push(request.url()));

    await page.goto('/generated_wasm_smoke.html');
    const smokeResult = await page.evaluate(() => window.generatedPackageSmoke);

    expect(pageErrors).toEqual([]);
    expect(failedRequestUrls).toEqual([]);
    expect(smokeResult).toEqual({
      compiled_version: EXPECTED_COMPILED_VERSION,
      provider_registration_count: EXPECTED_PROVIDER_REGISTRATION_COUNT,
      median: EXPECTED_MEDIAN_COURSE_DEGREES,
    });
  });
});
