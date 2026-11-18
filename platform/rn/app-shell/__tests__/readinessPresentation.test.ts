/**
 * The readiness chip's pure presentation (D4): a state is rendered only when
 * it is known; the keyless prompt is neutral; an unrecognised state string
 * never borrows a success or failure color.
 */

import type { ReadinessEvent } from '../src/RefappNative';
import { readinessChipPresentation } from '../src/readinessPresentation';
import { appStrings } from '../src/theme';

/** Builds a readiness event for the presentation cases. */
function readinessEvent(state: string, text: string): ReadinessEvent {
  return { state, text };
}

describe('readinessChipPresentation', () => {
  test('an unreported state is neutral and names no ladder state', () => {
    const presentation = readinessChipPresentation(
      readinessEvent('unknown', ''),
      false,
    );
    expect(presentation).toEqual({
      text: appStrings.readinessUnknownText,
      tone: 'neutral',
    });
  });

  test.each([
    ['loading', appStrings.readinessChipLoadingText, 'neutral'],
    ['ready', appStrings.readinessChipReadyText, 'positive'],
    [
      'degraded',
      appStrings.readinessChipDegradedPrefix +
        appStrings.readinessChipDegradedFallbackText,
      'warning',
    ],
    [
      'failed',
      appStrings.readinessChipFailedPrefix +
        appStrings.readinessChipFailedFallbackText,
      'danger',
    ],
  ] as const)('the known %s row carries its short line and tone', (state, text, tone) => {
    const presentation = readinessChipPresentation(
      readinessEvent(state, 'reported status'),
      false,
    );
    expect(presentation).toEqual({ text, tone });
  });

  test('a degraded or failed detail leads the chip line verbatim', () => {
    expect(
      readinessChipPresentation(
        { state: 'degraded', text: 'long line', detail: 'surface lost' },
        false,
      ),
    ).toEqual({
      text: appStrings.readinessChipDegradedPrefix + 'surface lost',
      tone: 'warning',
    });
    expect(
      readinessChipPresentation(
        { state: 'failed', text: 'long line', detail: 'no adapter' },
        false,
      ),
    ).toEqual({
      text: appStrings.readinessChipFailedPrefix + 'no adapter',
      tone: 'danger',
    });
  });

  test('an unrecognised state string stays neutral with its honest text', () => {
    const presentation = readinessChipPresentation(
      readinessEvent('hibernating', 'the map reported a new state'),
      false,
    );
    expect(presentation).toEqual({
      text: 'the map reported a new state',
      tone: 'neutral',
    });
  });

  test('keyless keeps a non-ready chip neutral with the product prompt', () => {
    for (const state of ['unknown', 'loading', 'degraded', 'failed']) {
      const presentation = readinessChipPresentation(
        readinessEvent(state, 'a provider failure the user did not cause'),
        true,
      );
      expect(presentation).toEqual({
        text: appStrings.providerCredentialKeylessNeutralText,
        tone: 'neutral',
      });
    }
  });

  test('keyless still reports a ready renderer honestly', () => {
    const presentation = readinessChipPresentation(
      readinessEvent('ready', 'Maps SDK gpu renderer active (version 0.1.0)'),
      true,
    );
    expect(presentation).toEqual({
      text: appStrings.readinessChipReadyText,
      tone: 'positive',
    });
  });
});
