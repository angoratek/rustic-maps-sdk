/**
 * The reference application's opt-in diagnostics stream
 * (docs/ReferenceAppUX.md §4.2).
 *
 * The SDK's sink is process-wide and always recording into its bounded ring;
 * the reference application adds one rule above it: no subscription and no
 * console output before the visitor opens the diagnostics pane. Opening the
 * pane subscribes at `Info` and flushes the bounded backlog. One explicit
 * switch lowers the subscription to `Debug`; the SDK gate never raises back,
 * so debug detail stays on for the rest of the session. Closing the pane
 * stops the refresh cadence but not the lowered gate.
 *
 * The display window and cadence are the shared behavior tokens, mirrored
 * here so the four applications cannot drift:
 *  - `diagnostics-event-window` = 100 events;
 *  - `diagnostics-backlog-flush-limit` = 100 events;
 *  - `diagnostics-refresh-cadence-milliseconds` = 500 ms;
 *  - `diagnostics-minimum-severity` = Info;
 *  - `diagnostics-debug-severity` = Debug.
 *
 * The module is free of React and native imports: every subscription decision
 * is pinned by jest with a fake gateway and jest timers.
 */

import { themeDiagnosticsTokens } from './theme';
import type { ProviderRequestTrace } from './providerConsole';

/** The shared display window: only the most recent events are kept. */
export const diagnosticsEventWindow =
  themeDiagnosticsTokens.diagnosticsEventWindow;

/** The shared backlog flush limit: the most recent ring events delivered on open. */
export const diagnosticsBacklogFlushLimit =
  themeDiagnosticsTokens.diagnosticsBacklogFlushLimit;

/** The shared refresh cadence: events are coalesced onto this interval. */
export const diagnosticsRefreshCadenceMilliseconds =
  themeDiagnosticsTokens.diagnosticsRefreshCadenceMilliseconds;

/** The SDK's per-subsystem in-memory ring capacity (the pane reads this bound). */
export const diagnosticsRingCapacity =
  themeDiagnosticsTokens.diagnosticsRingCapacity;

/** The pane's default subscription severity: coarse-grained progress. */
export const diagnosticsMinimumSeverityName =
  themeDiagnosticsTokens.diagnosticsMinimumSeverity;

/** The explicit debug switch's subscription severity. */
export const diagnosticsDebugSeverityName =
  themeDiagnosticsTokens.diagnosticsDebugSeverity;

/** The generated contract's severity ordinals (`SdkLogSeverity`). */
export const diagnosticsSeverityOrdinals = {
  trace: 0,
  debug: 1,
  info: 2,
  warn: 3,
  error: 4,
} as const;

/** The pane's default subscription severity: coarse-grained progress. */
export const diagnosticsMinimumSeverityOrdinal = diagnosticsSeverityOrdinals.info;

/** The explicit debug switch's subscription severity. */
export const diagnosticsDebugSeverityOrdinal = diagnosticsSeverityOrdinals.debug;

/** One SDK diagnostic event, as the bridge delivers it. */
export interface DiagnosticEvent {
  /** Wall-clock milliseconds since the Unix epoch, as the SDK recorded it. */
  timestampMillisecondsSinceEpoch: number;
  /** The severity's lower-case name (`info`, `debug`, `warn`, …). */
  severity: string;
  /** The named per-subsystem target that logged the event. */
  subsystemTarget: string;
  /** The one-line already-scrubbed event text. */
  messageLine: string;
  /**
   * The typed provider request facts when the event reports a completed
   * provider call, or null/absent for every other event. The record is
   * redacted at emission; the console never parses the message line.
   */
  providerRequestTrace?: ProviderRequestTrace | null;
}

/**
 * The native seam the controller drives: one listener registration at a
 * minimum severity, returning a teardown, plus the bounded backlog flush.
 * A platform without the seam supplies no-op functions, and the pane still
 * renders its honest empty state.
 */
export interface DiagnosticsSubscriptionGateway {
  /**
   * Registers one listener at the minimum severity. Answers the teardown that
   * removes this listener only.
   */
  subscribe(
    minimumSeverityOrdinal: number,
    onEvent: (event: DiagnosticEvent) => void,
  ): () => void;
  /** Delivers the bounded backlog to the listener at the minimum severity. */
  flushBacklog(
    minimumSeverityOrdinal: number,
    maximumCount: number,
    onEvent: (event: DiagnosticEvent) => void,
  ): void;
}

/** What the controller's owner renders: the coalesced window and the detail switch. */
export interface DiagnosticsStreamSnapshot {
  events: DiagnosticEvent[];
  isDebugEventsEnabled: boolean;
  isOpen: boolean;
}

/** The controller's owner callback. */
export type DiagnosticsStreamListener = (snapshot: DiagnosticsStreamSnapshot) => void;

/**
 * The opt-in diagnostics stream. Constructing it subscribes to nothing; only
 * `open()` crosses the native seam.
 */
export class DiagnosticsStreamController {
  private readonly gateway: DiagnosticsSubscriptionGateway;
  private readonly onSnapshot: DiagnosticsStreamListener;
  private pendingEvents: DiagnosticEvent[] = [];
  private visibleEvents: DiagnosticEvent[] = [];
  private seenEventKeys = new Set<string>();
  private unsubscribe: (() => void) | null = null;
  private refreshTimer: ReturnType<typeof setInterval> | null = null;
  private isOpenState = false;
  private debugEventsEnabled = false;

  constructor(
    gateway: DiagnosticsSubscriptionGateway,
    onSnapshot: DiagnosticsStreamListener,
  ) {
    this.gateway = gateway;
    this.onSnapshot = onSnapshot;
  }

  /** Whether the pane currently holds a subscription. */
  get isOpen(): boolean {
    return this.isOpenState;
  }

  /** Whether the explicit debug switch was turned on this session. */
  get isDebugEventsEnabled(): boolean {
    return this.debugEventsEnabled;
  }

  /** The coalesced display window (most recent last). */
  get events(): DiagnosticEvent[] {
    return this.visibleEvents;
  }

  /**
   * Opens the pane: subscribes at Info, flushes the bounded backlog and
   * starts the 500 ms coalescing cadence. A second call is a no-op.
   */
  open(): void {
    if (this.isOpenState) {
      return;
    }
    this.isOpenState = true;
    this.subscribeAt(
      this.debugEventsEnabled
        ? diagnosticsDebugSeverityOrdinal
        : diagnosticsMinimumSeverityOrdinal,
    );
    this.refreshTimer = setInterval(
      () => this.publishPendingEvents(),
      diagnosticsRefreshCadenceMilliseconds,
    );
    this.publishSnapshot();
  }

  /**
   * Closes the pane: stops the refresh cadence and removes the listener. The
   * already-lowered SDK gate stays lowered for the session.
   */
  close(): void {
    if (!this.isOpenState) {
      return;
    }
    this.publishPendingEvents();
    this.isOpenState = false;
    this.unsubscribe?.();
    this.unsubscribe = null;
    if (this.refreshTimer !== null) {
      clearInterval(this.refreshTimer);
      this.refreshTimer = null;
    }
    this.publishSnapshot();
  }

  /**
   * Turns the explicit debug switch on. The subscription re-registers at
   * Debug and the backlog is flushed again; an off request is ignored because
   * the SDK gate never raises back and the note says so.
   */
  setDebugEvents(enabled: boolean): void {
    if (!enabled || this.debugEventsEnabled) {
      return;
    }
    this.debugEventsEnabled = true;
    if (this.isOpenState) {
      this.unsubscribe?.();
      this.unsubscribe = null;
      this.subscribeAt(diagnosticsDebugSeverityOrdinal);
    }
    this.publishSnapshot();
  }

  /** Empties the pane's display window; it never touches the SDK ring. */
  clear(): void {
    this.pendingEvents = [];
    this.visibleEvents = [];
    this.seenEventKeys.clear();
    this.publishSnapshot();
  }

  /** Registers the listener and flushes the bounded backlog at one severity. */
  private subscribeAt(minimumSeverityOrdinal: number): void {
    const onEvent = (event: DiagnosticEvent) => this.noteEvent(event);
    this.unsubscribe = this.gateway.subscribe(minimumSeverityOrdinal, onEvent);
    this.gateway.flushBacklog(
      minimumSeverityOrdinal,
      diagnosticsBacklogFlushLimit,
      onEvent,
    );
  }

  /** Coalesces one delivered event; the cadence publishes it to the pane. */
  private noteEvent(event: DiagnosticEvent): void {
    const eventKey = [
      event.timestampMillisecondsSinceEpoch,
      event.severity,
      event.subsystemTarget,
      event.messageLine,
    ].join('|');
    if (this.seenEventKeys.has(eventKey)) {
      return;
    }
    this.seenEventKeys.add(eventKey);
    this.pendingEvents.push(event);
  }

  /** Moves the pending events into the bounded window and notifies the pane. */
  private publishPendingEvents(): void {
    if (this.pendingEvents.length === 0) {
      return;
    }
    this.visibleEvents = [...this.visibleEvents, ...this.pendingEvents].slice(
      -diagnosticsEventWindow,
    );
    this.pendingEvents = [];
    this.publishSnapshot();
  }

  /** Delivers the current snapshot to the owner. */
  private publishSnapshot(): void {
    this.onSnapshot({
      events: this.visibleEvents,
      isDebugEventsEnabled: this.debugEventsEnabled,
      isOpen: this.isOpenState,
    });
  }
}
