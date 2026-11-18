/**
 * The opt-in diagnostics stream (docs/design/ReferenceApplicationBehavior.md,
 * §Diagnostics policy): nothing subscribes before the pane opens, opening
 * subscribes at Info and flushes the bounded backlog, the debug switch lowers
 * the gate once per session, and the display window is coalesced and bounded.
 */

import {
  DiagnosticsStreamController,
  diagnosticsBacklogFlushLimit,
  diagnosticsDebugSeverityOrdinal,
  diagnosticsEventWindow,
  diagnosticsMinimumSeverityOrdinal,
  diagnosticsRefreshCadenceMilliseconds,
  type DiagnosticEvent,
  type DiagnosticsSubscriptionGateway,
  type DiagnosticsStreamSnapshot,
} from '../src/diagnosticsStream';

/** One event with a unique timestamp. */
function diagnosticEvent(index: number, severity = 'info'): DiagnosticEvent {
  return {
    timestampMillisecondsSinceEpoch: 1_000 + index,
    severity,
    subsystemTarget: 'tile_fetch',
    messageLine: `event ${index}`,
  };
}

/** A fake gateway that records every registration and its teardown. */
function createFakeGateway(): {
  gateway: DiagnosticsSubscriptionGateway;
  subscriptions: number[];
  flushCalls: Array<{ severity: number; maximumCount: number }>;
  unsubscribedCount: () => number;
  deliver: (event: DiagnosticEvent) => void;
} {
  const subscriptions: number[] = [];
  const flushCalls: Array<{ severity: number; maximumCount: number }> = [];
  let activeListener: ((event: DiagnosticEvent) => void) | null = null;
  let unsubscribeCount = 0;
  return {
    subscriptions,
    flushCalls,
    unsubscribedCount: () => unsubscribeCount,
    deliver: event => activeListener?.(event),
    gateway: {
      subscribe: (minimumSeverityOrdinal, onEvent) => {
        subscriptions.push(minimumSeverityOrdinal);
        activeListener = onEvent;
        return () => {
          unsubscribeCount += 1;
          if (activeListener === onEvent) {
            activeListener = null;
          }
        };
      },
      flushBacklog: (minimumSeverityOrdinal, maximumCount, onEvent) => {
        flushCalls.push({
          severity: minimumSeverityOrdinal,
          maximumCount,
        });
        onEvent(diagnosticEvent(0));
      },
    },
  };
}

beforeEach(() => {
  jest.useFakeTimers();
});

afterEach(() => {
  jest.useRealTimers();
});

test('construction subscribes to nothing and shows no events', () => {
  const fake = createFakeGateway();
  const snapshots: DiagnosticsStreamSnapshot[] = [];
  const controller = new DiagnosticsStreamController(fake.gateway, snapshot =>
    snapshots.push(snapshot),
  );

  expect(fake.subscriptions).toEqual([]);
  expect(fake.flushCalls).toEqual([]);
  expect(controller.isOpen).toBe(false);
  expect(controller.events).toEqual([]);
  expect(snapshots).toEqual([]);
});

test('opening subscribes at Info, flushes the bounded backlog and starts the cadence', () => {
  const fake = createFakeGateway();
  const snapshots: DiagnosticsStreamSnapshot[] = [];
  const controller = new DiagnosticsStreamController(fake.gateway, snapshot =>
    snapshots.push(snapshot),
  );

  controller.open();

  expect(fake.subscriptions).toEqual([diagnosticsMinimumSeverityOrdinal]);
  expect(fake.flushCalls).toEqual([
    {
      severity: diagnosticsMinimumSeverityOrdinal,
      maximumCount: diagnosticsBacklogFlushLimit,
    },
  ]);
  expect(controller.isOpen).toBe(true);
  // Coalescing: the backlog is not published until the 500 ms cadence.
  expect(controller.events).toEqual([]);

  jest.advanceTimersByTime(diagnosticsRefreshCadenceMilliseconds);
  expect(controller.events.map(event => event.messageLine)).toEqual(['event 0']);
});

test('events are coalesced onto the 500 ms cadence, not published one by one', () => {
  const fake = createFakeGateway();
  const controller = new DiagnosticsStreamController(fake.gateway, () => undefined);
  controller.open();

  fake.deliver(diagnosticEvent(1));
  fake.deliver(diagnosticEvent(2));
  // The open-flush event is still pending with the two delivered ones.
  expect(controller.events).toEqual([]);

  jest.advanceTimersByTime(diagnosticsRefreshCadenceMilliseconds);
  expect(controller.events.map(event => event.messageLine)).toEqual([
    'event 0',
    'event 1',
    'event 2',
  ]);
});

test('the explicit debug switch re-registers at Debug exactly once per session', () => {
  const fake = createFakeGateway();
  const controller = new DiagnosticsStreamController(fake.gateway, () => undefined);
  controller.open();

  controller.setDebugEvents(true);
  expect(fake.unsubscribedCount()).toBe(1);
  expect(fake.subscriptions).toEqual([
    diagnosticsMinimumSeverityOrdinal,
    diagnosticsDebugSeverityOrdinal,
  ]);

  // The SDK gate never raises back, so a second off/on request does nothing.
  controller.setDebugEvents(false);
  controller.setDebugEvents(true);
  expect(fake.subscriptions).toEqual([
    diagnosticsMinimumSeverityOrdinal,
    diagnosticsDebugSeverityOrdinal,
  ]);
  expect(controller.isDebugEventsEnabled).toBe(true);
});

test('the display window keeps only the most recent 100 events', () => {
  const fake = createFakeGateway();
  const controller = new DiagnosticsStreamController(fake.gateway, () => undefined);
  controller.open();

  for (let index = 1; index <= 120; index += 1) {
    fake.deliver(diagnosticEvent(index));
  }
  jest.advanceTimersByTime(diagnosticsRefreshCadenceMilliseconds);

  expect(controller.events).toHaveLength(diagnosticsEventWindow);
  expect(controller.events[0].messageLine).toBe('event 21');
  expect(controller.events[controller.events.length - 1].messageLine).toBe(
    'event 120',
  );
});

test('clear empties the pane without touching the subscription', () => {
  const fake = createFakeGateway();
  const controller = new DiagnosticsStreamController(fake.gateway, () => undefined);
  controller.open();
  jest.advanceTimersByTime(diagnosticsRefreshCadenceMilliseconds);
  expect(controller.events).toHaveLength(1);

  controller.clear();

  expect(controller.events).toEqual([]);
  expect(fake.unsubscribedCount()).toBe(0);
  expect(fake.subscriptions).toEqual([diagnosticsMinimumSeverityOrdinal]);
});

test('closing removes the listener but keeps the lowered debug gate for the session', () => {
  const fake = createFakeGateway();
  const controller = new DiagnosticsStreamController(fake.gateway, () => undefined);
  controller.open();
  controller.setDebugEvents(true);
  controller.close();

  expect(controller.isOpen).toBe(false);
  expect(fake.unsubscribedCount()).toBe(2);

  controller.open();
  expect(controller.isDebugEventsEnabled).toBe(true);
  expect(fake.subscriptions).toEqual([
    diagnosticsMinimumSeverityOrdinal,
    diagnosticsDebugSeverityOrdinal,
    diagnosticsDebugSeverityOrdinal,
  ]);
});

test('duplicate deliveries are ignored so a re-flushed backlog does not repeat', () => {
  const fake = createFakeGateway();
  const controller = new DiagnosticsStreamController(fake.gateway, () => undefined);
  controller.open();
  controller.setDebugEvents(true);

  jest.advanceTimersByTime(diagnosticsRefreshCadenceMilliseconds);
  // The Debug re-flush delivered the same backlog event again.
  expect(controller.events.map(event => event.messageLine)).toEqual(['event 0']);
});
