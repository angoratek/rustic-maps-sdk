/**
 * The developer console's model (M58): the level filter, newest-first order,
 * the provider request trace row and detail lines, the cache summary, the
 * credential-free export and the honest empty state.
 */

import {
  buildProviderConsoleRows,
  hasProviderConsoleAlertAfter,
  providerConsoleCacheLines,
  providerConsoleDetailLines,
  providerConsoleEmptyText,
  providerConsoleExportMaximumBytes,
  providerConsoleExportText,
  providerConsoleExportTruncationNote,
  providerConsoleLevelLabels,
  providerConsoleRowExcerptCharacters,
  providerConsoleRowLine,
  truncateProviderConsoleExcerpt,
  type ProviderRequestTrace,
} from '../src/providerConsole';
import type { DiagnosticEvent } from '../src/diagnosticsStream';
import type { FrameStatisticsEvent } from '../src/RefappNative';

/** The registry display name the console composes rows with. */
const displayNameForProvider = (providerKey: string): string =>
  providerKey === 'mapquest' ? 'MapQuest' : providerKey;

/** One provider request trace, already redacted at emission. */
function providerTrace(
  overrides: Partial<ProviderRequestTrace> = {},
): ProviderRequestTrace {
  return {
    providerKey: 'mapquest',
    providerContract: 'RoutingProvider',
    httpMethod: 'GET',
    requestPath: '/directions/v2/route',
    statusClass: '2xx',
    outcome: 'success',
    durationMilliseconds: 123,
    responseBytes: 4096,
    responseExcerpt: '{"route":{"distance":1.2}}',
    ...overrides,
  };
}

/** One diagnostic event at a named severity. */
function diagnosticEvent(
  timestamp: number,
  severity: string,
  messageLine: string,
  trace: ProviderRequestTrace | null = null,
): DiagnosticEvent {
  return {
    timestampMillisecondsSinceEpoch: timestamp,
    severity,
    subsystemTarget: 'provider_transport',
    messageLine,
    providerRequestTrace: trace,
  };
}

/** The fixture window: info, debug, warning and error, oldest first. */
const events: DiagnosticEvent[] = [
  diagnosticEvent(1_700_000_000_000, 'info', 'tile cache hit'),
  diagnosticEvent(
    1_700_000_001_000,
    'info',
    'provider request completed',
    providerTrace(),
  ),
  diagnosticEvent(1_700_000_002_000, 'debug', 'frame path detail'),
  diagnosticEvent(
    1_700_000_003_000,
    'warn',
    'provider request completed with a warning',
    providerTrace({ statusClass: '4xx', outcome: 'client-error' }),
  ),
  diagnosticEvent(
    1_700_000_004_000,
    'error',
    'provider request failed',
    providerTrace({ statusClass: 'transport-failure', responseExcerpt: null }),
  ),
];

/** One text's UTF-8 byte length (the export ceiling is in bytes). */
function utf8ByteLength(text: string): number {
  let totalBytes = 0;
  for (const character of text) {
    const codePoint = character.codePointAt(0) ?? 0;
    if (codePoint <= 0x7f) {
      totalBytes += 1;
    } else if (codePoint <= 0x7ff) {
      totalBytes += 2;
    } else if (codePoint <= 0xffff) {
      totalBytes += 3;
    } else {
      totalBytes += 4;
    }
  }
  return totalBytes;
}

test('info shows Error, Warning and Info newest first, never debug', () => {
  const rows = buildProviderConsoleRows(events, 'info', displayNameForProvider);

  expect(rows.map(row => row.levelLabel)).toEqual([
    'Error',
    'Warning',
    'Info',
    'Info',
  ]);
  expect(rows.map(row => row.event.messageLine)).toEqual([
    'provider request failed',
    'provider request completed with a warning',
    'provider request completed',
    'tile cache hit',
  ]);
  expect(rows.some(row => row.event.severity === 'debug')).toBe(false);
});

test('the filter treats Warning and Error as minimum levels', () => {
  const warningRows = buildProviderConsoleRows(
    events,
    'warn',
    displayNameForProvider,
  );
  expect(warningRows.map(row => row.levelLabel)).toEqual([
    'Error',
    'Warning',
  ]);

  const errorRows = buildProviderConsoleRows(
    events,
    'error',
    displayNameForProvider,
  );
  expect(errorRows.map(row => row.levelLabel)).toEqual(['Error']);
  expect(providerConsoleLevelLabels.error).toBe('Error');
});

test('a trace row names the provider, contract, method, status and duration', () => {
  const rows = buildProviderConsoleRows(events, 'info', displayNameForProvider);
  const traceRow = rows.find(
    row => row.event.messageLine === 'provider request completed',
  );
  expect(traceRow).toBeDefined();
  const line = providerConsoleRowLine(traceRow!);

  expect(line).toContain('Info');
  expect(line).toContain('MapQuest');
  expect(line).toContain('RoutingProvider');
  expect(line).toContain('GET /directions/v2/route');
  expect(line).toContain('2xx');
  expect(line).toContain('123 ms');
  expect(traceRow!.truncatedResponseExcerpt).toBe(
    '{"route":{"distance":1.2}}',
  );
});

test('the response excerpt truncates on a character boundary for the row', () => {
  const longExcerpt = 'é'.repeat(providerConsoleRowExcerptCharacters + 20);
  const truncated = truncateProviderConsoleExcerpt(longExcerpt);
  expect(truncated).not.toBeNull();
  expect(Array.from(truncated!).length).toBe(
    providerConsoleRowExcerptCharacters + 1,
  );
  expect(truncated!.endsWith('…')).toBe(true);
  expect(truncateProviderConsoleExcerpt(null)).toBeNull();
});

test('the detail lines carry every typed trace field', () => {
  const rows = buildProviderConsoleRows(events, 'info', displayNameForProvider);
  const traceRow = rows.find(
    row => row.event.messageLine === 'provider request completed',
  )!;
  const detail = providerConsoleDetailLines(traceRow).join('\n');

  expect(detail).toContain('Provider: MapQuest');
  expect(detail).toContain('Provider key: mapquest');
  expect(detail).toContain('Contract: RoutingProvider');
  expect(detail).toContain('Method: GET');
  expect(detail).toContain('Request path: /directions/v2/route');
  expect(detail).toContain('Status class: 2xx');
  expect(detail).toContain('Outcome: success');
  expect(detail).toContain('Duration: 123 ms');
  expect(detail).toContain('Response bytes: 4096');
  expect(detail).toContain('{"route":{"distance":1.2}}');

  const nonTraceRow = rows.find(
    row => row.event.messageLine === 'tile cache hit',
  )!;
  expect(providerConsoleDetailLines(nonTraceRow).join('\n')).toContain(
    'This event carries no provider request trace.',
  );
});

test('the cache summary renders the tile and traffic counters', () => {
  const frameStatistics: FrameStatisticsEvent = {
    live: true,
    requestedTileCount: 12,
    presentTileCount: 8,
    fallbackTileCount: 2,
    deferredTileCount: 1,
    unavailableTileCount: 1,
    overlayTriangleCount: 40,
    framesDrawnCount: 5,
    framesDeferredCount: 0,
    framesFailedCount: 0,
  };
  const lines = providerConsoleCacheLines({
    tileStatistics: frameStatistics,
    tileCacheStatistics: {
      hitCount: 5,
      missCount: 2,
      storedByteCount: 4096,
    },
    trafficAvailability: {
      state: 'available',
      providerKey: 'traffic-source',
      attributionText: 'Traffic data by Traffic Source',
    },
    trafficCacheStatistics: {
      hitCount: 7,
      missCount: 2,
      entryCount: 4,
      evictionCount: 1,
      segmentCount: 30,
      gridZoomLevel: 12,
      maximumEntryCount: 64,
      timeToLiveSeconds: 300,
    },
    displayNameForProvider: providerKey =>
      providerKey === 'traffic-source' ? 'Traffic Source' : providerKey,
  });

  expect(lines[0]).toContain('hits 5');
  expect(lines[0]).toContain('misses 2');
  expect(lines[0]).toContain('stored 4096 bytes');
  expect(lines[1]).toContain('requested 12');
  expect(lines[1]).toContain('drawn 8');
  expect(lines[1]).toContain('fallback 2');
  expect(lines[1]).toContain('deferred 1');
  expect(lines[1]).toContain('unavailable 1');
  expect(lines[2]).toContain('Traffic Source');
  expect(lines[2]).toContain('hits 7');
  expect(lines[2]).toContain('misses 2');
  expect(lines[2]).toContain('held cells 4');
  expect(lines[2]).toContain('evictions 1');
  expect(lines[2]).toContain('grid zoom z12');

  // An absent readback states the absence instead of inventing a count.
  const absentLines = providerConsoleCacheLines({
    tileStatistics: null,
    tileCacheStatistics: null,
    trafficAvailability: {
      state: 'notConfigured',
      providerKey: null,
      reasonText: 'Not configured',
    },
    trafficCacheStatistics: null,
    displayNameForProvider,
  });
  expect(absentLines[0]).toBe('Tile cache: no tile cache is installed.');
  expect(absentLines[1]).toBe('Last frame: no frame settled yet.');
  expect(absentLines[2]).toBe('Traffic cache: Not configured');
});

test('the export is the filtered list with no credential and a byte ceiling', () => {
  const rows = buildProviderConsoleRows(events, 'warn', displayNameForProvider);
  const exportText = providerConsoleExportText(rows);

  expect(exportText).toContain('provider request failed');
  expect(exportText).toContain('provider request completed with a warning');
  // Info rows are filtered out of the Warning-level export.
  expect(exportText).not.toContain('tile cache hit');
  // The transport's redacted fields never carry a credential or header.
  expect(exportText).not.toContain('api_key=');
  expect(exportText).not.toContain('Authorization');

  const largeRows = Array.from({ length: 4000 }, (_, index) =>
    buildProviderConsoleRows(
      [
        diagnosticEvent(
          1_700_000_100_000 + index,
          'error',
          `provider failure ${index} `.repeat(20),
          providerTrace({
            responseExcerpt: 'x'.repeat(512),
            statusClass: '5xx',
            outcome: 'server-error',
          }),
        ),
      ],
      'error',
      displayNameForProvider,
    )[0],
  );
  const cappedExport = providerConsoleExportText(largeRows);
  expect(utf8ByteLength(cappedExport)).toBeLessThanOrEqual(
    providerConsoleExportMaximumBytes,
  );
  expect(cappedExport.endsWith(providerConsoleExportTruncationNote)).toBe(
    true,
  );
});

test('the console has an honest empty state and an unread alert predicate', () => {
  expect(buildProviderConsoleRows([], 'info', displayNameForProvider)).toEqual(
    [],
  );
  expect(providerConsoleEmptyText.length).toBeGreaterThan(0);
  expect(hasProviderConsoleAlertAfter(events, 1_700_000_003_500)).toBe(true);
  expect(hasProviderConsoleAlertAfter(events, 1_700_000_004_500)).toBe(false);
  expect(hasProviderConsoleAlertAfter(events, Number.MAX_SAFE_INTEGER)).toBe(
    false,
  );
});
