// The hosted mobile demo page's keyboard and screen-reader contract: a named
// tablist whose arrow keys move selection with a roving tabindex, a tabpanel
// that names the selected tab, the session link carrying a new-tab hint and
// receiving focus when it replaces the Start button, and Stop returning focus
// to Start. Everything the page would fetch at runtime is fulfilled locally,
// so the test never touches the network.
import { expect, test } from './network_guard.mjs';

const MANIFEST = {
  sourceCommit: '0123456789abcdef0123456789abcdef01234567',
  builds: {
    android: { buildId: 'android-build-id', updatedAt: '2026-10-30T00:00:00Z' },
    ios: { buildId: 'ios-build-id', updatedAt: '2026-10-30T00:00:00Z' },
    reactNativeAndroid: { buildId: 'rn-build-id', updatedAt: '2026-10-30T00:00:00Z' },
  },
};

/** Fulfils the page's two runtime requests from committed local data. */
async function route_page_requests(page) {
  await page.route('**/demo-builds.json', (route) =>
    route.fulfill({ json: MANIFEST }));
  await page.route('**/api/claim**', (route) => {
    const request_url = new URL(route.request().url());
    if (request_url.searchParams.get('status') === 'true') {
      return route.fulfill({ json: { remainingSeconds: 1_200 } });
    }
    return route.fulfill({ json: { state: 'ok', sessionSeconds: 180 } });
  });
}

test.beforeEach(async ({ page }) => {
  await route_page_requests(page);
  await page.goto('/mobile-demo/');
});

test('the platform tabs follow the ARIA tabs pattern', async ({ page }) => {
  const tablist = page.getByRole('tablist', { name: 'Choose a reference application' });
  await expect(tablist).toBeVisible();

  const tabs = page.getByRole('tab');
  await expect(tabs).toHaveCount(3);
  await expect(tabs.nth(0)).toHaveAttribute('aria-controls', 'mobile-demo-stage');
  await expect(tabs.nth(0)).toHaveAttribute('aria-selected', 'true');
  await expect(tabs.nth(0)).toHaveAttribute('tabindex', '0');
  await expect(tabs.nth(1)).toHaveAttribute('tabindex', '-1');
  await expect(tabs.nth(2)).toHaveAttribute('tabindex', '-1');

  const panel = page.getByRole('tabpanel');
  await expect(panel).toHaveAttribute('aria-labelledby', 'platform-tab-android');
});

test('arrow keys move along the tablist with wraparound', async ({ page }) => {
  const android_tab = page.getByRole('tab', { name: 'Android' });
  const ios_tab = page.getByRole('tab', { name: 'iOS' });
  const react_native_tab = page.getByRole('tab', { name: 'React Native' });

  await android_tab.focus();
  await page.keyboard.press('ArrowRight');
  await expect(ios_tab).toBeFocused();
  await expect(ios_tab).toHaveAttribute('aria-selected', 'true');
  await expect(android_tab).toHaveAttribute('tabindex', '-1');
  await expect(page.getByRole('tabpanel'))
    .toHaveAttribute('aria-labelledby', 'platform-tab-ios');

  await page.keyboard.press('End');
  await expect(react_native_tab).toBeFocused();
  await expect(react_native_tab).toHaveAttribute('aria-selected', 'true');

  await page.keyboard.press('ArrowRight');
  await expect(android_tab).toBeFocused();
  await expect(android_tab).toHaveAttribute('aria-selected', 'true');

  await page.keyboard.press('Home');
  await expect(android_tab).toBeFocused();
});

test('the granted session link receives focus, names the new tab, and Stop returns focus', async ({ page }) => {
  const start_button = page.getByRole('button', { name: 'Start demo' });
  await expect(start_button).toBeEnabled();
  await start_button.click();

  const session_link = page.getByRole('link', { name: /Open the demo/ });
  await expect(session_link).toBeVisible();
  await expect(session_link).toBeFocused();
  await expect(session_link).toHaveAttribute('target', '_blank');
  await expect(session_link).toContainText('opens in a new tab');
  await expect(session_link).toHaveAttribute('href', /android-build-id/);

  await page.getByRole('button', { name: 'Stop' }).click();
  await expect(start_button).toBeVisible();
  await expect(start_button).toBeFocused();
});

test('a resting demo states the budget honestly and offers the browser demo', async ({ page }) => {
  await page.route('**/api/claim**', (route) => {
    const request_url = new URL(route.request().url());
    if (request_url.searchParams.get('status') === 'true') {
      return route.fulfill({ json: { remainingSeconds: 60 } });
    }
    return route.fulfill({
      json: {
        state: 'resting',
        detail: 'The demo is resting right now.',
        remainingSeconds: 60,
      },
    });
  });
  await page.getByRole('button', { name: 'Start demo' }).click();
  const resting = page.locator('[data-role="resting"]');
  await expect(resting).toBeVisible();
  await expect(resting).toContainText('The demo is resting right now.');
  await expect(resting).toContainText('About 1 of this month’s streamed minutes remain.');
  const fallback = page.locator('[data-role="resting-fallback"]');
  await expect(fallback).toBeVisible();
  await expect(fallback.getByRole('link', { name: 'browser demo' })).toHaveAttribute('href', '..');
  // The budget is the only thing exhausted: Start stays available so a
  // transient failure can be retried without reloading the page.
  await expect(page.getByRole('button', { name: 'Start demo' })).toBeEnabled();
});
