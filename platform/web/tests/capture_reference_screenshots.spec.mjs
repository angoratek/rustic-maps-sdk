// Captures the browser reference-application screenshots the documentation
// publishes: the plan sheet with both endpoints chosen, the provider section
// with the applied provider selected on every row, the navigation
// frame with the turn banner, planned polyline, ETA line and speed controls,
// the vector base with its place labels, and the layers control closed and
// open over the credential's raster base.
//
// The capture is opt-in so the ordinary suite stays read-only:
//
//   CAPTURE_REFERENCE_SCREENSHOTS=1 PROVIDER_API_KEY=… npx playwright test \
//     tests/capture_reference_screenshots.spec.mjs --workers=1
//
// The credential is applied through the page's own settings surface for the
// session only; it is never persisted, logged or written below an input's
// masked value. The harness keeps provider traffic same-origin through
// `?useProviderFixtures`, so a capture run never reaches a provider.
//
// Output lands under `tmp/reference-screenshots/web/` for review; only a
// reviewed set from one revision is copied into `site/book-src/img/`.
import { readFileSync } from 'node:fs';
import { mkdir } from 'node:fs/promises';
import path from 'node:path';
import { test, expect } from '@playwright/test';

import { waitForMapTestReady } from './network_guard.mjs';

const OUTPUT_DIRECTORY = path.resolve(
  import.meta.dirname,
  '../../../tmp/reference-screenshots/web',
);
/** Phone width for every frame; the provider sheet is tall enough to fit. */
const PHONE_VIEWPORT = Object.freeze({ width: 430, height: 932 });
const PROVIDER_VIEWPORT = Object.freeze({ width: 430, height: 1600 });
const ROUTE_SETTLE_TIMEOUT_MILLISECONDS = 15_000;
const VECTOR_SETTLE_TIMEOUT_MILLISECONDS = 30_000;
const RENDERER_SETTLE_TIMEOUT_MILLISECONDS = 30_000;
const CAPTURE_SETTLE_MILLISECONDS = 750;
const VECTOR_LABEL_SETTLE_MILLISECONDS = 2_000;

const PROVIDERS_REGISTRY = JSON.parse(readFileSync(
  new URL('../../../platform/providers.json', import.meta.url),
  'utf8',
));
/** The registry's declared keyless vector base, selected on the camera frame. */
const KEYLESS_VECTOR_PROVIDER_KEY = PROVIDERS_REGISTRY.keyless_base_map.provider_key;
const KEYLESS_VECTOR_STYLE_ID = PROVIDERS_REGISTRY.keyless_base_map.style_id;
/** The recorded trace the reference navigation scenario replays. */
const REFERENCE_SCENES = JSON.parse(readFileSync(
  new URL('../../../platform/reference-scenes.json', import.meta.url),
  'utf8',
));
const REFERENCE_NAVIGATION_SCENARIO = REFERENCE_SCENES.navigation_demo.simulated_trace.name;

const CAPTURE_ENABLED = process.env.CAPTURE_REFERENCE_SCREENSHOTS === '1';
const PROVIDER_CREDENTIAL = process.env.PROVIDER_API_KEY?.trim() ?? '';

/**
 * Applies the session credential through the settings surface and returns the
 * provider key it belongs to. The value only ever lives in the masked input
 * and the page's in-memory registration; the input is cleared afterwards so
 * the published frame cannot show it.
 */
async function applyProviderCredential(page) {
  await page.locator('#settings-control').click();
  await expect(page.locator('#settings-dialog')).toBeVisible();
  const providerRow = page.locator('#provider-credential-rows .provider-credential-row').first();
  await expect(providerRow).toBeVisible();
  const providerKey = await providerRow.getAttribute('data-provider-key');
  const credentialInput = providerRow.locator('[data-provider-credential-input]');
  await expect(credentialInput).toHaveAttribute('type', 'password');
  await credentialInput.fill(PROVIDER_CREDENTIAL);
  await providerRow.locator('[data-provider-credential-apply]').click();
  await expect(page.locator('#provider-credential-status')).toContainText(
    'key applied',
    { timeout: ROUTE_SETTLE_TIMEOUT_MILLISECONDS },
  );
  await credentialInput.fill('');
  return providerKey;
}

/** Selects the applied provider on every capability picker that offers it. */
async function selectProviderForEveryCapability(page, providerKey) {
  const pickers = page.locator('select[id^="capability-provider-select-"]');
  const pickerCount = await pickers.count();
  for (let pickerIndex = 0; pickerIndex < pickerCount; pickerIndex += 1) {
    const picker = pickers.nth(pickerIndex);
    if (await picker.locator(`option[value="${providerKey}"]`).count() === 0) continue;
    await picker.selectOption(providerKey);
    const capabilityKey = (await picker.getAttribute('id'))
      .replace('capability-provider-select-', '');
    await expect(page.locator(`#capability-provider-serving-${capabilityKey}`))
      .toContainText('·', { timeout: ROUTE_SETTLE_TIMEOUT_MILLISECONDS });
  }
}

/** Awaits the GPU renderer the recorded trip's guidance needs. */
async function waitForGpuRenderer(page) {
  await expect
    .poll(() => page.evaluate(() => window.mapCameraState?.().rendererMode), {
      timeout: RENDERER_SETTLE_TIMEOUT_MILLISECONDS,
    })
    .toBe('gpu');
}

/** Loads the keyed page and applies the credential for this session only. */
async function openKeyedPage(page) {
  await page.goto('/?useProviderFixtures');
  await waitForMapTestReady(page);
  const providerKey = await applyProviderCredential(page);
  await page.keyboard.press('Escape');
  await expect(page.locator('#settings-dialog')).toBeHidden();
  return providerKey;
}

/**
 * Fills one endpoint from the demo-place chips. Typing one character makes
 * that field the chip target; the chip then assigns the artifact's place
 * instead of a provider result, so the frame stays the reference scenario
 * whether or not a credential is applied.
 */
async function assignDemoPlace(page, fieldKey, value) {
  const input = page.locator(`#trip-${fieldKey}-input`);
  await input.click();
  await input.press('End');
  await input.type(value.slice(0, 1));
  const chip = page.locator('#trip-suggestions button', { hasText: value }).first();
  await expect(chip).toBeVisible();
  await chip.click();
  await expect(input).toHaveValue(value);
}

test.describe('reference application screenshot capture', () => {
  test.skip(!CAPTURE_ENABLED, 'capture-only; set CAPTURE_REFERENCE_SCREENSHOTS=1');

  // Four frames on one page; the vector tiles' first fetch can outlast the
  // suite's default thirty-second per-test budget.
  test.setTimeout(240_000);

  test.beforeAll(async () => {
    if (!PROVIDER_CREDENTIAL) {
      throw new Error(
        'the keyed capture needs a session credential in PROVIDER_API_KEY',
      );
    }
    await mkdir(OUTPUT_DIRECTORY, { recursive: true });
  });

  test('captures the plan sheet, provider section, navigation frame and vector base', async ({
    page,
  }) => {
    await page.setViewportSize(PHONE_VIEWPORT);
    const providerKey = await openKeyedPage(page);

    // The provider section: the applied credential row, every capability
    // row's composed state, and the selected provider on each picker. The
    // capture viewport is phone width and tall enough for the whole sheet.
    await page.setViewportSize(PROVIDER_VIEWPORT);
    await page.locator('#settings-control').click();
    await expect(page.locator('#settings-dialog')).toBeVisible();
    await selectProviderForEveryCapability(page, providerKey);
    await page.addStyleTag({
      content: '#settings-dialog, .settings-sheet { max-height: none !important; }'
        + ' .settings-body { overflow: visible !important; }',
    });
    await page.locator('#settings-dialog').evaluate((dialog) => {
      dialog.scrollTop = 0;
    });
    await page.waitForTimeout(CAPTURE_SETTLE_MILLISECONDS);
    await page.screenshot({
      path: path.join(OUTPUT_DIRECTORY, 'provider-section.png'),
    });
    await page.keyboard.press('Escape');
    await expect(page.locator('#settings-dialog')).toBeHidden();

    // The plan sheet: both endpoints chosen and the reference route planned,
    // with the applied credential still registered for this session.
    await page.setViewportSize(PHONE_VIEWPORT);
    await waitForGpuRenderer(page);
    await assignDemoPlace(page, 'origin', 'Capitol Park');
    await assignDemoPlace(page, 'destination', 'Tower Bridge');
    await page.locator('#trip-plan-route-control').click();
    await expect(page.locator('#trip-route-result')).toBeVisible({
      timeout: ROUTE_SETTLE_TIMEOUT_MILLISECONDS,
    });
    await page.waitForTimeout(CAPTURE_SETTLE_MILLISECONDS);
    await page.screenshot({
      path: path.join(OUTPUT_DIRECTORY, 'plan-sheet.png'),
    });

    // The navigation frame: a fresh keyed session on the recorded reference
    // trace, so the plan, the polyline and the fixes are the same recorded
    // route. The frame lands on the first instruction with the 1× chip.
    await openKeyedPage(page);
    await waitForGpuRenderer(page);
    await page.evaluate(
      async (scenario) => window.selectDemoPlanScenario(scenario),
      REFERENCE_NAVIGATION_SCENARIO,
    );
    await page.locator('#follow-demo-start-control').click();
    await expect
      .poll(() => page.evaluate(() => window.mapFollowDemoState?.().phase), {
        timeout: ROUTE_SETTLE_TIMEOUT_MILLISECONDS,
      })
      .toBe('engaged');
    const speedChips = page.locator('#follow-demo-speed-controls button');
    await speedChips.first().click();
    await expect(speedChips.first()).toHaveAttribute('aria-pressed', 'true');
    await expect
      .poll(async () => page.locator('#banner-instruction-text').textContent(), {
        timeout: ROUTE_SETTLE_TIMEOUT_MILLISECONDS,
      })
      .not.toBe('');
    await page.locator('#controls').evaluate((panel) => {
      panel.scrollTop = panel.scrollHeight;
    });
    await page.screenshot({
      path: path.join(OUTPUT_DIRECTORY, 'navigation.png'),
    });

    // The vector base: the registry's keyless styled base with its place
    // labels, in a fresh keyed session and on the default scene. The consumer
    // sheet is hidden for this frame so the base fills the phone width.
    await page.goto('/?useProviderFixtures');
    await page.evaluate(() => window.localStorage.clear());
    await page.reload();
    await waitForMapTestReady(page);
    await waitForGpuRenderer(page);
    await applyProviderCredential(page);
    await page.keyboard.press('Escape');
    await expect(page.locator('#settings-dialog')).toBeHidden();
    await page.locator('#map-layers-control').click();
    const vectorStyleRow = page.locator(
      `#map-layer-style-${KEYLESS_VECTOR_STYLE_ID}`);
    await expect(vectorStyleRow).toBeVisible();
    await vectorStyleRow.click();
    // Prove the frame draws the registry's styled base, not the raster
    // source the credential registered underneath it.
    await expect(vectorStyleRow).toHaveAttribute('aria-pressed', 'true');
    await expect(vectorStyleRow).toContainText('Serving');
    await page.locator('#map-layers-control').click();
    await expect
      .poll(() => page.evaluate(() => window.mapTileMode()), {
        timeout: VECTOR_SETTLE_TIMEOUT_MILLISECONDS,
      })
      .toBe('vector');
    await expect
      .poll(() => page.evaluate(() => window.mapVectorProviderKey()), {
        timeout: VECTOR_SETTLE_TIMEOUT_MILLISECONDS,
      })
      .toBe(KEYLESS_VECTOR_PROVIDER_KEY);
    await page.waitForTimeout(VECTOR_LABEL_SETTLE_MILLISECONDS);
    await page.addStyleTag({ content: '#controls { display: none !important; }' });
    await page.waitForTimeout(CAPTURE_SETTLE_MILLISECONDS);
    await page.screenshot({
      path: path.join(OUTPUT_DIRECTORY, 'vector-base.png'),
    });
  });

  // The layers control pair from one session on the credential's raster
  // base: the same camera pose with the panel closed, then open with every
  // section and the clear action in one phone-width frame.
  test('captures the layers panel closed and open', async ({ page }) => {
    await page.setViewportSize(PHONE_VIEWPORT);
    const providerKey = await openKeyedPage(page);
    await waitForGpuRenderer(page);
    await expect
      .poll(() => page.evaluate(() => window.mapTileMode()), {
        timeout: VECTOR_SETTLE_TIMEOUT_MILLISECONDS,
      })
      .toBe('raster');
    // The map is the subject, so the plan sheet steps aside as it does for
    // the vector-base frame and the map controls stay.
    await page.addStyleTag({ content: '#controls { display: none !important; }' });
    await page.waitForTimeout(CAPTURE_SETTLE_MILLISECONDS);
    await expect(page.locator('#map-layers-control')).toBeVisible();
    await page.screenshot({
      path: path.join(OUTPUT_DIRECTORY, 'layers-closed.png'),
    });

    await page.locator('#map-layers-control').click();
    const panel = page.locator('#map-layers-panel');
    await expect(panel).toBeVisible();
    await expect(page.locator(`#map-layer-base-${providerKey}`))
      .toContainText('Serving');
    await expect(panel).toContainText('Base map');
    await expect(panel).toContainText('Traffic');
    await expect(panel).toContainText('Overlays');
    await expect(panel).toContainText('Device location');
    await expect(page.locator('#map-layers-clear-scenes'))
      .toHaveText('Clear overlays');
    await page.waitForTimeout(CAPTURE_SETTLE_MILLISECONDS);
    await page.screenshot({
      path: path.join(OUTPUT_DIRECTORY, 'layers-open.png'),
    });
  });
});
