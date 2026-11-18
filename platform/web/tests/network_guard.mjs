// One browser context fixture for the whole Playwright suite. Every test
// imports `test` and `expect` from here instead of `@playwright/test`, which
// installs one same-origin network guard: a request that leaves the harness
// origin aborts unless the test itself registered a route for it. A missing
// fixture is then a loud failure with the offending URL, never a silent
// contact with a tile or provider service (CLAUDE.md: tests never touch the
// network). Tests that exercise a live-style URL intercept it with
// `page.route`, and page routes win over this context route.
import { expect, test as base } from '@playwright/test';

export const test = base.extend({
  context: async ({ context, baseURL }, use) => {
    if (baseURL === undefined) {
      throw new Error('the harness origin is unset: playwright.config.mjs must set use.baseURL');
    }
    const harness_origin = new URL(baseURL).origin;
    await context.route('**/*', (route) => {
      const request_url = route.request().url();
      let parsed_url;
      try {
        parsed_url = new URL(request_url);
      } catch {
        // Not an absolute URL (a data: or blob: subresource): never remote.
        return route.continue();
      }
      if (parsed_url.protocol !== 'http:' && parsed_url.protocol !== 'https:') {
        return route.continue();
      }
      if (parsed_url.origin !== harness_origin) {
        return route.abort('blockedbyclient');
      }
      return route.continue();
    });
    await use(context);
  },
});

export { expect };

/**
 * Awaits the page's documented readiness promise after the module has
 * installed it. Evaluated too early, `window.mapTestReady` is `undefined` and
 * `page.evaluate` resolves at once, so a slow module load under CPU pressure
 * could race a test's first assertion.
 *
 * The SDK promise resolves when the Rust runtime is ready, while the
 * playground module finishes its own setup after its shared-artifact fetches;
 * the state hooks a test reads land in that later step. Wait for the
 * playground's camera hook as well, so readiness means the page is safe to
 * assert on rather than only the SDK being started.
 */
export async function waitForMapTestReady(page) {
  await page.waitForFunction(() => window.mapTestReady !== undefined, null, {
    timeout: 30_000,
  });
  await page.evaluate(() => window.mapTestReady);
  await page.waitForFunction(
    () => typeof window.mapCameraState === 'function',
    null,
    { timeout: 30_000 },
  );
}
