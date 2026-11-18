/**
 * The reference application's developer console model (M58): the scrubbed SDK
 * event stream at Error, Warning and Info, one row per event, newest first.
 *
 * The console reads the same `DiagnosticsStreamController` window the
 * diagnostics sheet reads and never issues a provider call. A row built from
 * an event carrying the typed `ProviderRequestTrace` names the provider, the
 * contract, the method, the redacted path, the status class, the duration and
 * a truncated response excerpt; the trace is already redacted at emission, so
 * a credential can never reach a row or the exported text.
 *
 * This module is free of React and native imports: the filter, the detail
 * lines, the cache summary and the export text are pinned by jest with
 * recorded events.
 */

import type { FrameStatisticsEvent } from './RefappNative';
import type { TrafficLayerAvailability } from './mapLayersPanelState';
import type {
  TileCacheStatistics,
  TrafficFlowCacheStatistics,
} from 'maps-sdk-rn/MapsSdk';
import {
  type DiagnosticEvent,
  diagnosticsSeverityOrdinals,
} from './diagnosticsStream';

/**
 * One completed provider request, as the native diagnostics event carries it
 * (`ProviderRequestTrace` in the contract). Every field is already redacted.
 */
export interface ProviderRequestTrace {
  /** The registry key of the provider that served the call. */
  providerKey: string;
  /** The contract the call served, for example `RoutingProvider`. */
  providerContract: string;
  /** The HTTP method as an uppercase word (`GET`, `POST`). */
  httpMethod: string;
  /** The redacted request path, with query values removed. */
  requestPath: string;
  /** `2xx`, `3xx`, `4xx`, `5xx` or the transport-failure marker. */
  statusClass: string;
  /** The derived outcome word (`success`, `client-error`, …). */
  outcome: string;
  /** Wall-clock duration of the attempt in milliseconds. */
  durationMilliseconds: number;
  /** Response payload size in bytes; zero when no response arrived. */
  responseBytes: number;
  /** The redacted response prefix, capped at 512 bytes by the contract. */
  responseExcerpt: string | null;
}

/**
 * The console's level filter. `Info` is the minimum level and shows all three
 * documented levels (Error, Warning and Info); `Warning` shows Warning and
 * Error; `Error` shows Error only. Debug and Trace never render here.
 */
export type ProviderConsoleLevel = 'error' | 'warn' | 'info';

/** The filter's visible labels, in the design's order. */
export const providerConsoleLevelLabels: Record<ProviderConsoleLevel, string> = {
  error: 'Error',
  warn: 'Warning',
  info: 'Info',
};

/** The filter's order on the sheet (Error, Warning, Info). */
export const providerConsoleLevelOrder: readonly ProviderConsoleLevel[] = [
  'error',
  'warn',
  'info',
];

/** The console's honest empty state when the window holds no level event. */
export const providerConsoleEmptyText = 'No events recorded at this level yet.';

/** One line of the console list. */
export interface ProviderConsoleRow {
  /** A stable identity for selection across coalesced snapshots. */
  rowKey: string;
  /** The source event, already scrubbed. */
  event: DiagnosticEvent;
  /** The typed provider trace, or null for a non-provider event. */
  trace: ProviderRequestTrace | null;
  /** The row's level word as the filter names it (`Error`, `Warning`, `Info`). */
  levelLabel: string;
  /** The registry display name, or the provider key when the registry lacks it. */
  providerDisplayName: string;
  /** The response excerpt truncated for the list row, or null. */
  truncatedResponseExcerpt: string | null;
}

/** The longest excerpt a list row shows before the detail sheet holds the rest. */
export const providerConsoleRowExcerptCharacters = 160;

/** The response excerpt's truncation mark, so a cut never reads as complete. */
export const providerConsoleExcerptEllipsis = '…';

/**
 * The export's plain-text byte ceiling (256 KB): the share sheet receives the
 * filtered list capped here, with one note line saying so when it truncates.
 */
export const providerConsoleExportMaximumBytes = 256 * 1024;

/** The note appended when the export text hits its byte ceiling. */
export const providerConsoleExportTruncationNote =
  '[export truncated at 256 KB]';

/** The filter's minimum severity ordinal. */
const providerConsoleLevelMinimumOrdinal: Record<ProviderConsoleLevel, number> = {
  error: diagnosticsSeverityOrdinals.error,
  warn: diagnosticsSeverityOrdinals.warn,
  info: diagnosticsSeverityOrdinals.info,
};

/** One level's name from the contract's lower-case severity word. */
export function providerConsoleLevelFromSeverity(
  severity: string,
): ProviderConsoleLevel | null {
  if (severity === 'error') {
    return 'error';
  }
  if (severity === 'warn' || severity === 'warning') {
    return 'warn';
  }
  if (severity === 'info') {
    return 'info';
  }
  return null;
}

/** The epoch timestamp as the console's local `hh:mm:ss` line. */
export function providerConsoleTimeText(
  timestampMillisecondsSinceEpoch: number,
): string {
  const date = new Date(timestampMillisecondsSinceEpoch);
  const hours = String(date.getHours()).padStart(2, '0');
  const minutes = String(date.getMinutes()).padStart(2, '0');
  const seconds = String(date.getSeconds()).padStart(2, '0');
  return `${hours}:${minutes}:${seconds}`;
}

/** Truncates a response excerpt on a character boundary for the list row. */
export function truncateProviderConsoleExcerpt(
  responseExcerpt: string | null,
): string | null {
  if (responseExcerpt === null) {
    return null;
  }
  const characters = Array.from(responseExcerpt);
  if (characters.length <= providerConsoleRowExcerptCharacters) {
    return responseExcerpt;
  }
  return (
    characters.slice(0, providerConsoleRowExcerptCharacters).join('') +
    providerConsoleExcerptEllipsis
  );
}

/**
 * Builds the filtered list newest first. Only Error, Warning and Info events
 * qualify; the filter includes events at or above its own level.
 *
 * # Parameters
 * - `events` — the diagnostics window (oldest first).
 * - `level` — the selected minimum level.
 * - `displayNameForProvider` — the registry display name for a provider key.
 */
export function buildProviderConsoleRows(
  events: readonly DiagnosticEvent[],
  level: ProviderConsoleLevel,
  displayNameForProvider: (providerKey: string) => string,
): ProviderConsoleRow[] {
  const minimumOrdinal = providerConsoleLevelMinimumOrdinal[level];
  const rows: ProviderConsoleRow[] = [];
  for (const event of events) {
    const levelFromSeverity = providerConsoleLevelFromSeverity(event.severity);
    if (levelFromSeverity === null) {
      continue;
    }
    const ordinal =
      diagnosticsSeverityOrdinals[
        event.severity as keyof typeof diagnosticsSeverityOrdinals
      ];
    if (ordinal === undefined || ordinal < minimumOrdinal) {
      continue;
    }
    const trace = event.providerRequestTrace ?? null;
    rows.push({
      rowKey: [
        event.timestampMillisecondsSinceEpoch,
        event.severity,
        event.subsystemTarget,
        event.messageLine,
      ].join('|'),
      event,
      trace,
      levelLabel: providerConsoleLevelLabels[levelFromSeverity],
      providerDisplayName:
        trace === null
          ? ''
          : displayNameForProvider(trace.providerKey),
      truncatedResponseExcerpt: truncateProviderConsoleExcerpt(
        trace?.responseExcerpt ?? null,
      ),
    });
  }
  // The window arrives oldest first; the console reads newest first.
  return rows.reverse();
}

/** One row's one-line text: level, subsystem, message and any provider facts. */
export function providerConsoleRowLine(row: ProviderConsoleRow): string {
  const baseLine =
    `${providerConsoleTimeText(
      row.event.timestampMillisecondsSinceEpoch,
    )} · ${row.levelLabel} · ${row.event.subsystemTarget} · ` +
    row.event.messageLine;
  if (row.trace === null) {
    return baseLine;
  }
  const durationText = Number.isFinite(row.trace.durationMilliseconds)
    ? `${Math.round(row.trace.durationMilliseconds)} ms`
    : 'duration unavailable';
  return (
    `${baseLine} — ${row.providerDisplayName || row.trace.providerKey} · ` +
    `${row.trace.providerContract} · ${row.trace.httpMethod} ` +
    `${row.trace.requestPath} · ${row.trace.statusClass} · ${durationText}`
  );
}

/**
 * The detail sheet's typed lines: the event fields and, when present, every
 * field of the provider request trace.
 */
export function providerConsoleDetailLines(row: ProviderConsoleRow): string[] {
  const lines = [
    `Timestamp: ${providerConsoleTimeText(
      row.event.timestampMillisecondsSinceEpoch,
    )}`,
    `Severity: ${row.levelLabel}`,
    `Subsystem: ${row.event.subsystemTarget}`,
    `Message: ${row.event.messageLine}`,
  ];
  const trace = row.trace;
  if (trace === null) {
    lines.push('This event carries no provider request trace.');
    return lines;
  }
  lines.push(
    `Provider: ${row.providerDisplayName || trace.providerKey}`,
    `Provider key: ${trace.providerKey}`,
    `Contract: ${trace.providerContract}`,
    `Method: ${trace.httpMethod}`,
    `Request path: ${trace.requestPath}`,
    `Status class: ${trace.statusClass}`,
    `Outcome: ${trace.outcome}`,
    `Duration: ${Math.round(trace.durationMilliseconds)} ms`,
    `Response bytes: ${trace.responseBytes}`,
    `Response excerpt: ${trace.responseExcerpt ?? 'none captured'}`,
  );
  return lines;
}

/** What the cache summary reads: the frame, traffic and registry facts. */
export interface ProviderConsoleCacheSummaryInput {
  /** The session's last settled frame, or null before the first frame. */
  tileStatistics: FrameStatisticsEvent | null;
  /** The installed tile payload cache's exact counters, or null when none. */
  tileCacheStatistics: TileCacheStatistics | null;
  /** The traffic layer's registry-derived availability. */
  trafficAvailability: TrafficLayerAvailability;
  /** The serving traffic cache's counters, or null when none are reported. */
  trafficCacheStatistics: TrafficFlowCacheStatistics | null;
  /** The registry display name for a provider key. */
  displayNameForProvider: (providerKey: string) => string;
}

/**
 * The cache summary's rows: the installed tile payload cache's exact
 * counters, the last settled frame's tile pipeline accounting, and the
 * traffic cache's counters beside their budget. An absent readback states the
 * honest absence rather than inventing a count.
 */
export function providerConsoleCacheLines(
  input: ProviderConsoleCacheSummaryInput,
): string[] {
  const tileCacheStatistics = input.tileCacheStatistics;
  const tileCacheLine =
    tileCacheStatistics === null
      ? 'Tile cache: no tile cache is installed.'
      : `Tile cache: hits ${tileCacheStatistics.hitCount} · ` +
        `misses ${tileCacheStatistics.missCount} · ` +
        `stored ${
          tileCacheStatistics.storedByteCount === undefined
            ? 'unavailable'
            : `${tileCacheStatistics.storedByteCount} bytes`
        }`;
  const frameLine =
    input.tileStatistics === null || !input.tileStatistics.live
      ? 'Last frame: no frame settled yet.'
      : `Last frame: requested ${
          input.tileStatistics.requestedTileCount ?? 0
        } · drawn ${input.tileStatistics.presentTileCount ?? 0} · fallback ${
          input.tileStatistics.fallbackTileCount ?? 0
        } · deferred ${input.tileStatistics.deferredTileCount ?? 0} · ` +
        `unavailable ${input.tileStatistics.unavailableTileCount ?? 0}`;
  const trafficStatistics = input.trafficCacheStatistics;
  let trafficLine: string;
  if (trafficStatistics !== null) {
    const providerPrefix =
      input.trafficAvailability.state === 'available'
        ? `${input.displayNameForProvider(
            input.trafficAvailability.providerKey,
          )} · `
        : '';
    trafficLine =
      `Traffic cache: ${providerPrefix}hits ${trafficStatistics.hitCount} · ` +
      `misses ${trafficStatistics.missCount} · held cells ` +
      `${trafficStatistics.entryCount} · evictions ` +
      `${trafficStatistics.evictionCount} · segments ` +
      `${trafficStatistics.segmentCount} · grid zoom z${trafficStatistics.gridZoomLevel} · ` +
      `entry budget ${trafficStatistics.maximumEntryCount} · time to live ` +
      `${trafficStatistics.timeToLiveSeconds} s`;
  } else if (input.trafficAvailability.state === 'available') {
    trafficLine =
      `Traffic cache: ${input.displayNameForProvider(
        input.trafficAvailability.providerKey,
      )} · no cache statistics reported.`;
  } else {
    trafficLine = `Traffic cache: ${input.trafficAvailability.reasonText}`;
  }
  return [tileCacheLine, frameLine, trafficLine];
}

/** One character's UTF-8 byte length (structural byte accounting). */
function utf8ByteLength(character: string): number {
  const codePoint = character.codePointAt(0) ?? 0;
  if (codePoint <= 0x7f) {
    return 1;
  }
  if (codePoint <= 0x7ff) {
    return 2;
  }
  if (codePoint <= 0xffff) {
    return 3;
  }
  return 4;
}

/** One text's UTF-8 byte length. */
function utf8TextByteLength(text: string): number {
  let totalBytes = 0;
  for (const character of text) {
    totalBytes += utf8ByteLength(character);
  }
  return totalBytes;
}

/** Caps text to a UTF-8 byte ceiling on a character boundary. */
function truncateToUtf8Bytes(
  text: string,
  maximumBytes: number,
): { text: string; didTruncate: boolean } {
  let usedBytes = 0;
  let usedCharacters = 0;
  for (const character of text) {
    const characterBytes = utf8ByteLength(character);
    if (usedBytes + characterBytes > maximumBytes) {
      return {
        text: Array.from(text).slice(0, usedCharacters).join(''),
        didTruncate: true,
      };
    }
    usedBytes += characterBytes;
    usedCharacters += 1;
  }
  return { text, didTruncate: false };
}

/**
 * The plain-text export of the filtered list, capped at 256 KB. The trace is
 * redacted at emission, so no credential or authorization header can appear;
 * a truncation appends the note line so the file never reads as complete when
 * it is not.
 */
export function providerConsoleExportText(
  rows: readonly ProviderConsoleRow[],
): string {
  const bodyText = rows
    .map(row => {
      const excerpt =
        row.trace?.responseExcerpt !== null &&
        row.trace?.responseExcerpt !== undefined
          ? `\n${row.trace.responseExcerpt}`
          : '';
      return `${providerConsoleRowLine(row)}${excerpt}`;
    })
    .join('\n');
  const noteText = `\n${providerConsoleExportTruncationNote}`;
  const maximumBodyBytes =
    providerConsoleExportMaximumBytes - utf8TextByteLength(noteText);
  const capped = truncateToUtf8Bytes(bodyText, maximumBodyBytes);
  return capped.didTruncate ? `${capped.text}${noteText}` : capped.text;
}

/** Whether the window holds a Warning or Error newer than the watermark. */
export function hasProviderConsoleAlertAfter(
  events: readonly DiagnosticEvent[],
  watermarkMillisecondsSinceEpoch: number,
): boolean {
  return events.some(event => {
    const level = providerConsoleLevelFromSeverity(event.severity);
    return (
      (level === 'warn' || level === 'error') &&
      event.timestampMillisecondsSinceEpoch > watermarkMillisecondsSinceEpoch
    );
  });
}
