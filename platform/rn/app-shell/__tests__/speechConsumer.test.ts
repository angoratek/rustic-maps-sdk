/**
 * Jest-level unit tests of the reference application's spoken-guidance
 * consumer (the speech contract's decision half): dedup, arrival,
 * toggle and no-text honesty — all without a TTS engine (the platform seam is
 * a recorded stub).
 */

import {
  createSpeechConsumer,
  initialSpeechConsumerState,
  speechDeliveryKindLabels,
  type SpeechConsumerDependencies,
  type SpeechDelivery,
} from '../src/speechConsumer';

/** Records the platform seam's calls (the jest stub of the native speaker). */
function recordingDependencies(): SpeechConsumerDependencies & {
  spokenLines: Array<{ speechText: string; engineUtteranceId: string }>;
  readonly stopCallCount: number;
} {
  const spokenLines: Array<{ speechText: string; engineUtteranceId: string }> = [];
  let stopCallCountValue = 0;
  return {
    spokenLines,
    get stopCallCount() {
      return stopCallCountValue;
    },
    speak(speechText: string, engineUtteranceId: string) {
      spokenLines.push({ speechText, engineUtteranceId });
    },
    stopSpeech() {
      stopCallCountValue += 1;
    },
  };
}

function delivery(overrides: Partial<SpeechDelivery> = {}): SpeechDelivery {
  return {
    speechText: 'In 200 m, turn left onto Test Street',
    speechInstructionId: '7',
    instructionKind: speechDeliveryKindLabels.maneuverApproach,
    ...overrides,
  };
}

test('voices the first topic delivery verbatim with the contract utterance id', () => {
  const seam = recordingDependencies();
  const consumer = createSpeechConsumer(seam);
  const decision = consumer.consumeDelivery(delivery());
  expect(decision).toBe('spoken');
  expect(seam.spokenLines).toEqual([
    { speechText: 'In 200 m, turn left onto Test Street', engineUtteranceId: '7' },
  ]);
  expect(consumer.getState().lastSpokenText).toBe('In 200 m, turn left onto Test Street');
  expect(consumer.getState().spokenUtteranceCount).toBe(1);
});

test('drops a re-delivery of the last-seen utterance id (voiced once, never twice)', () => {
  const seam = recordingDependencies();
  const consumer = createSpeechConsumer(seam);
  consumer.consumeDelivery(delivery({ speechInstructionId: '7' }));
  const replayDecision = consumer.consumeDelivery(delivery({ speechInstructionId: '7' }));
  expect(replayDecision).toBe('reDeliveryDropped');
  expect(seam.spokenLines).toHaveLength(1);
  expect(consumer.getState().spokenUtteranceCount).toBe(1);
});

test('an identical line re-emitted carries a new id and speaks again (fresh guidance)', () => {
  // The contract's own guarantee (speech.rs): identical text re-emitted is a
  // NEW emission — e.g. a second off-route recovery after going off route
  // again — and the consumer must voice it, never mistake it for a replay.
  const seam = recordingDependencies();
  const consumer = createSpeechConsumer(seam);
  consumer.consumeDelivery(
    delivery({
      speechText: 'Guidance resumed',
      speechInstructionId: '3',
      instructionKind: speechDeliveryKindLabels.offRouteRecovery,
    }),
  );
  const decision = consumer.consumeDelivery(
    delivery({
      speechText: 'Guidance resumed',
      speechInstructionId: '4',
      instructionKind: speechDeliveryKindLabels.offRouteRecovery,
    }),
  );
  expect(decision).toBe('spoken');
  expect(seam.spokenLines.map(spokenLine => spokenLine.engineUtteranceId)).toEqual(['3', '4']);
});

test('the arrival kind notes itself and never voices (the drawer presents arrival)', () => {
  const seam = recordingDependencies();
  const consumer = createSpeechConsumer(seam);
  consumer.consumeDelivery(delivery());
  const decision = consumer.consumeDelivery(
    delivery({
      speechText: 'In 200 m, turn left onto Test Street',
      speechInstructionId: '9',
      instructionKind: speechDeliveryKindLabels.arrival,
    }),
  );
  expect(decision).toBe('arrivalNoted');
  expect(seam.spokenLines).toHaveLength(1);
  expect(consumer.getState().lastSpokenText).toBeNull();
});

test('the toggle-off stops in-flight speech and the later deliveries skip; on resumes', () => {
  const seam = recordingDependencies();
  const consumer = createSpeechConsumer(seam);
  consumer.consumeDelivery(delivery({ speechInstructionId: '2' }));
  consumer.setSpeechEnabled(false);
  expect(seam.stopCallCount).toBe(1);
  expect(consumer.consumeDelivery(delivery({ speechInstructionId: '3' }))).toBe('toggledOffSkipped');
  expect(seam.spokenLines).toHaveLength(1);
  consumer.setSpeechEnabled(true);
  expect(seam.stopCallCount).toBe(1);
  expect(consumer.consumeDelivery(delivery({ speechInstructionId: '4' }))).toBe('spoken');
  expect(seam.spokenLines).toHaveLength(2);
  expect(consumer.getState().isSpeechEnabled).toBe(true);
});

test('the same-toggle write is a no-op (no stop without a change)', () => {
  const seam = recordingDependencies();
  const consumer = createSpeechConsumer(seam);
  consumer.setSpeechEnabled(false);
  consumer.setSpeechEnabled(false);
  expect(seam.stopCallCount).toBe(1);
});

test('an empty speech text is skipped honestly, never voiced with invented words', () => {
  const seam = recordingDependencies();
  const consumer = createSpeechConsumer(seam);
  expect(consumer.consumeDelivery(delivery({ speechText: null }))).toBe('noSpeechTextSkipped');
  expect(consumer.consumeDelivery(delivery({ speechText: '' }))).toBe('noSpeechTextSkipped');
  expect(seam.spokenLines).toHaveLength(0);
  expect(consumer.getState().lastSpokenText).toBeNull();
});

test('a delivery without a contract id speaks under the consumer own monotonic counter', () => {
  const seam = recordingDependencies();
  const consumer = createSpeechConsumer(seam);
  consumer.consumeDelivery(delivery({ speechInstructionId: null }));
  consumer.consumeDelivery(delivery({ speechInstructionId: null, speechText: 'Turn left now' }));
  expect(seam.spokenLines.map(spokenLine => spokenLine.engineUtteranceId)).toEqual(['1', '2']);
});

test('the consumer state changes notify their subscribers (the drawer rides it)', () => {
  const seam = recordingDependencies();
  const consumer = createSpeechConsumer(seam);
  const observedStates: Array<typeof initialSpeechConsumerState> = [];
  const unsubscribe = consumer.onStateChange(state => observedStates.push(state));
  consumer.consumeDelivery(delivery({ speechInstructionId: '17' }));
  unsubscribe();
  consumer.consumeDelivery(delivery({ speechInstructionId: '18' }));
  expect(observedStates.map(state => state.spokenUtteranceCount)).toEqual([1]);
  expect(consumer.getState().spokenUtteranceCount).toBe(2);
});