// Voice guidance on the web reference app (M31, docs/ReferenceAppUX.md §7.3):
// the platform speech engine speaks the snapshot's own speech channel — the
// contract's text-to-speech variant, never the banner sentence re-used as
// speech — when the canonical Voice guidance control is on. The test stubs
// the browser speech engine (headless Chromium has no audio) and records the
// utterances, so the assertion is about the text the page chose to speak.
import { expect, test, waitForMapTestReady } from './network_guard.mjs';

/** The guidance settle budget (the trip dispatches fixes every 250 ms). */
const GUIDANCE_SETTLE_TIMEOUT_MILLISECONDS = 20_000;

test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => {
    window.__spokenTexts = [];
    window.__cancelCount = 0;
    class RecordedSpeechSynthesisUtterance {
      constructor(text) {
        this.text = text;
      }
    }
    Object.defineProperty(window, 'SpeechSynthesisUtterance', {
      value: RecordedSpeechSynthesisUtterance,
      configurable: true,
      writable: true,
    });
    Object.defineProperty(window, 'speechSynthesis', {
      value: {
        speak: (utterance) => window.__spokenTexts.push(utterance.text),
        cancel: () => {
          window.__cancelCount += 1;
        },
      },
      configurable: true,
    });
  });
  await page.goto('/?advanced');
  await waitForMapTestReady(page);
});

test('the voice control speaks the snapshot speech channel and cancels when off', async ({
  page,
}) => {
  // The guidance panel is a collapsed disclosure until a visitor opens it.
  await page.locator('#navigation-demo-panel').evaluate((element) => {
    element.open = true;
  });
  const voice_control = page.getByRole('button', { name: 'Voice guidance' });
  await expect(voice_control).toBeEnabled();
  await expect(voice_control).toHaveAttribute('aria-pressed', 'false');

  // Plan the recorded Capitol Park → Tower Bridge route and start guidance.
  await page.locator('#trip-origin-input').fill('Capitol Park');
  await page.locator('#trip-destination-input').fill('Tower Bridge');
  await page.locator('#trip-destination-search-control').click();
  await page.locator('#trip-search-results button').first().click();
  await page.locator('#trip-plan-route-control').click();
  await expect(page.locator('#trip-route-result')).toContainText(
    'Recorded demo route',
  );
  await page.locator('#follow-demo-start-control').click();
  await expect
    .poll(() => page.evaluate(() => window.mapNavigationSnapshot()?.phase), {
      timeout: GUIDANCE_SETTLE_TIMEOUT_MILLISECONDS,
    })
    .toBe('navigating');

  // Enabling the control speaks the instruction already on screen. Read the
  // snapshot and click in one synchronous turn so the live trip cannot
  // advance between them.
  const enabled = await page.evaluate(() => {
    const speech_text =
      window.mapNavigationSnapshot().instruction.activeManeuver.speechText;
    const spoken_before = window.__spokenTexts.length;
    document.getElementById('navigation-voice-control').click();
    return { speech_text, spoken_before };
  });
  expect(typeof enabled.speech_text).toBe('string');
  expect(enabled.speech_text.length).toBeGreaterThan(0);
  await expect(voice_control).toHaveAttribute('aria-pressed', 'true');
  await expect
    .poll(() => page.evaluate(() => window.__spokenTexts.length), {
      timeout: GUIDANCE_SETTLE_TIMEOUT_MILLISECONDS,
    })
    .toBeGreaterThan(enabled.spoken_before);
  const spoken_texts = await page.evaluate(() => window.__spokenTexts);
  expect(spoken_texts[enabled.spoken_before]).toBe(enabled.speech_text);
  for (const spoken_text of spoken_texts) {
    expect(spoken_text.length).toBeGreaterThan(0);
  }

  // Turning it off cancels the engine and returns the pressed state.
  const cancel_count_before = await page.evaluate(() => window.__cancelCount);
  await voice_control.click();
  await expect(voice_control).toHaveAttribute('aria-pressed', 'false');
  expect(await page.evaluate(() => window.__cancelCount)).toBeGreaterThan(
    cancel_count_before,
  );
});
