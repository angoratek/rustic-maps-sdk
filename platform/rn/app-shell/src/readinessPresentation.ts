/**
 * The readiness chip's presentation (docs/ReferenceAppUX.md §6.1's four-state
 * ladder and §6.5's keyless rule). The chip renders a state only when the
 * state is known: before the first readiness event — and for any state string
 * this build does not recognise — it stays neutral instead of implying
 * loading, readiness or failure. While the session is keyless, a
 * non-ready chip is the neutral product prompt to set a key, never a provider
 * failure the user has not caused.
 */

import type { ReadinessEvent } from './RefappNative';
import { appStrings } from './theme';

/** The chip's color role; App resolves it against the owned theme's rows. */
export type ReadinessTone = 'neutral' | 'positive' | 'warning' | 'danger';

/** The chip's resolved text and color role. */
export interface ReadinessChipPresentation {
  text: string;
  tone: ReadinessTone;
}

/** The known §6.1 ladder states; anything else is an unknown state. */
const knownReadinessLadderStates: ReadonlySet<string> = new Set([
  'loading',
  'ready',
  'degraded',
  'failed',
]);

/**
 * Resolves the readiness chip's honest presentation: the neutral keyless
 * prompt, a known ladder state's own text and tone, or the neutral unknown
 * row when the state has not been reported (or is not one this build knows).
 *
 * §1 fixes the chip's shape: one short line per state ("Ready · gpu",
 * "Loading…", "Degraded: …", "Failed: …"). The ladder's longer status text
 * stays in the diagnostics sheet; the chip composes its own line from the
 * state and the event's detail.
 */
export function readinessChipPresentation(
  readiness: ReadinessEvent,
  isProviderKeyless: boolean,
): ReadinessChipPresentation {
  if (isProviderKeyless && readiness.state !== 'ready') {
    // §6.5: keyless is a first-class state; the prompt is neutral, not a
    // renderer failure the user has not caused.
    return {
      text: appStrings.providerCredentialKeylessNeutralText,
      tone: 'neutral',
    };
  }
  if (!knownReadinessLadderStates.has(readiness.state)) {
    // D4: no state is implied when none is known. An unrecognised state
    // still carries its honest reported text.
    const reportedText =
      readiness.text !== '' ? readiness.text : appStrings.readinessUnknownText;
    return { text: reportedText, tone: 'neutral' };
  }
  switch (readiness.state) {
    case 'ready':
      return { text: appStrings.readinessChipReadyText, tone: 'positive' };
    case 'degraded':
      return {
        text:
          appStrings.readinessChipDegradedPrefix +
          (readiness.detail && readiness.detail !== ''
            ? readiness.detail
            : appStrings.readinessChipDegradedFallbackText),
        tone: 'warning',
      };
    case 'failed':
      return {
        text:
          appStrings.readinessChipFailedPrefix +
          (readiness.detail && readiness.detail !== ''
            ? readiness.detail
            : appStrings.readinessChipFailedFallbackText),
        tone: 'danger',
      };
    default:
      // The loading row: neutral, never a success or failure color.
      return { text: appStrings.readinessChipLoadingText, tone: 'neutral' };
  }
}
