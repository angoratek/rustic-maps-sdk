/**
 * The reference application's spoken-guidance consumer (docs/ReferenceAppUX.md
 * §4.2's speech toggle, §7; the contract's platform half,
 * RN-shaped). This is the decision half the jest environment tests without a
 * TTS engine; the voice is the app-owned native TextToSpeech engine behind
 * the bridge module's spoken-guidance methods.
 *
 * What is spoken comes **verbatim from the contract's speech channel** — the
 * `RefappSpeechInstruction` topic delivery's speech text, never paraphrased,
 * prefixed or emptied (§6.3's verbatim rule applied to spoken guidance): an
 * empty line is skipped, never voiced with invented words, and the arrival
 * payload is presented, not spoken (the Compose reference application's
 * `noteArrivalWithoutSpeech` honesty).
 *
 * Dedup: the contract's `utterance_id` is
 * strictly monotonic per controller — a re-delivery of the last-seen id is a
 * replay and is dropped; identical text re-emitted is a NEW id and speaks (a
 * repeated off-route recovery is fresh guidance, never a stutter to silence).
 * Keeping only the last-seen id is sufficient — the topic's deliveries arrive
 * in emission order.
 *
 * Flush/supersede: every spoken line replaces whatever the platform engine
 * plays or queues (the native speak call carries the flush; the Compose
 * application's over-talking fix), and the trip's end abandons the engine's
 * audio focus after drain on the native side. The toggle lives in the
 * navigation drawer (§7.2): default on, turning off stops in-flight speech,
 * turning on takes effect at the next delivery.
 */

/** One speech topic delivery as the event channel carries it (snake-free). */
export interface SpeechDelivery {
  /** The verbatim speech text (the contract's `utterance_speech_text`). */
  speechText: string | null;
  /** The contract's `utterance_id` as its decimal string, or null. */
  speechInstructionId: string | null;
  /** What triggered the line (the lowercase enum label, e.g. `arrival`). */
  instructionKind: string;
}

/** The delivery kinds' label spellings (the Kotlin enum names, lowercased). */
export const speechDeliveryKindLabels = {
  maneuverApproach: 'maneuver_approach',
  immediateManeuver: 'immediate_maneuver',
  offRouteRecovery: 'off_route_recovery',
  arrival: 'arrival',
} as const;

/** The consumer's observable state (the drawer's speech facts). */
export interface SpeechConsumerState {
  /** Whether spoken guidance is wanted (default on, per §4.2). */
  isSpeechEnabled: boolean;
  /** The last line voiced (or noted) — the drawer's speech-log affordance. */
  lastSpokenText: string | null;
  /** How many lines the consumer voiced this session (the exercise count). */
  spokenUtteranceCount: number;
}

/** What the consumer decided for one delivery (the honest decision report). */
export type SpeechDeliveryDecision =
  | 'spoken'
  | 'reDeliveryDropped'
  | 'arrivalNoted'
  | 'toggledOffSkipped'
  | 'noSpeechTextSkipped';

/** The platform seam the consumer drives (stubbed in jest; native on device). */
export interface SpeechConsumerDependencies {
  /** Hands one verbatim line to the platform voice (the engine flushes first). */
  speak(speechText: string, engineUtteranceId: string): void;
  /** Stops whatever speaks (the toggle-off act). */
  stopSpeech(): void;
}

/** The fresh consumer state (§4.2: spoken guidance defaults on). */
export const initialSpeechConsumerState: SpeechConsumerState = {
  isSpeechEnabled: true,
  lastSpokenText: null,
  spokenUtteranceCount: 0,
};

/**
 * Builds the spoken-guidance consumer. The decisions are the jest-tested
 * surface: the delivered utterance ids dedup keep-last-seen, arrivals note
 * without speaking, and the engine id falls back to this consumer's own
 * monotonic counter when the delivery carries no contract id (Compose
 * parity: the platform's stand-in identity on the id-less fallback).
 */
export function createSpeechConsumer(dependencies: SpeechConsumerDependencies): {
  consumeDelivery: (delivery: SpeechDelivery) => SpeechDeliveryDecision;
  setSpeechEnabled: (isSpeechEnabled: boolean) => void;
  getState: () => SpeechConsumerState;
  onStateChange: (listener: (state: SpeechConsumerState) => void) => () => void;
} {
  let state: SpeechConsumerState = initialSpeechConsumerState;
  /** The last-seen (equivalently last-spoken) contract utterance id. */
  let lastSeenUtteranceId: string | null = null;
  /** The platform stand-in counter behind the id-less deliveries' engine ids. */
  let platformUtteranceCounterValue = 0;
  const stateChangeListeners: Array<(state: SpeechConsumerState) => void> = [];

  function commitStateChange(): void {
    for (const listener of stateChangeListeners) {
      listener(state);
    }
  }

  function platformUtteranceIdentifier(): string {
    platformUtteranceCounterValue += 1;
    return String(platformUtteranceCounterValue);
  }

  function consumeDelivery(delivery: SpeechDelivery): SpeechDeliveryDecision {
    if (delivery.instructionKind === speechDeliveryKindLabels.arrival) {
      // The arrival payload presents in the drawer, never voices (the
      // Compose reference application's `noteArrivalWithoutSpeech` honesty:
      // the arrival state carries its own positive-stated card, §7.1).
      state = { ...state, lastSpokenText: null };
      commitStateChange();
      return 'arrivalNoted';
    }
    if (!state.isSpeechEnabled) {
      return 'toggledOffSkipped';
    }
    if (delivery.speechText === null || delivery.speechText === '') {
      // §6.3: an empty speech channel is skipped honestly, never invented.
      return 'noSpeechTextSkipped';
    }
    if (delivery.speechInstructionId !== null &&
      delivery.speechInstructionId === lastSeenUtteranceId
    ) {
      // The speech topic is fire-and-forget: the same id delivered twice is a
      // re-emission replay, voiced once — never twice.
      return 'reDeliveryDropped';
    }
    // The engine id: the contract id verbatim, the platform counter on the
    // fallback (the delivery carries no contract id there).
    const engineUtteranceId = delivery.speechInstructionId ?? platformUtteranceIdentifier();
    dependencies.speak(delivery.speechText, engineUtteranceId);
    lastSeenUtteranceId = delivery.speechInstructionId ?? engineUtteranceId;
    state = {
      ...state,
      lastSpokenText: delivery.speechText,
      spokenUtteranceCount: state.spokenUtteranceCount + 1,
    };
    commitStateChange();
    return 'spoken';
  }

  function setSpeechEnabled(nextSpeechEnabled: boolean): void {
    if (nextSpeechEnabled === state.isSpeechEnabled) {
      return;
    }
    state = { ...state, isSpeechEnabled: nextSpeechEnabled };
    // The toggle-off stops in-flight speech immediately (the Compose
    // reference application's drawer toggle); toggling on announces itself on
    // the next instruction.
    if (!nextSpeechEnabled) {
      dependencies.stopSpeech();
    }
    commitStateChange();
  }

  function getState(): SpeechConsumerState {
    return state;
  }

  function onStateChange(listener: (state: SpeechConsumerState) => void): () => void {
    stateChangeListeners.push(listener);
    return () => {
      const listenerIndex = stateChangeListeners.indexOf(listener);
      if (listenerIndex !== -1) {
        stateChangeListeners.splice(listenerIndex, 1);
      }
    };
  }

  return {
    consumeDelivery,
    setSpeechEnabled,
    getState,
    onStateChange,
  };
}