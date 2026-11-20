/**
 * Decoding of the native diagnostics stream's payloads: the event's fields,
 * and the typed provider request trace it optionally carries.
 */

import type { DiagnosticEvent } from '../diagnosticsStream';
import type { ProviderRequestTrace } from '../providerConsole';

/** The diagnostics event's fields, decoded from the native payload. */
export function diagnosticEventFromPayload(
  payload: Record<string, unknown>,
): DiagnosticEvent {
  return {
    timestampMillisecondsSinceEpoch: Number(
      payload.timestampMillisecondsSinceEpoch ?? 0,
    ),
    severity:
      typeof payload.severity === 'string' ? payload.severity : 'info',
    subsystemTarget:
      typeof payload.subsystemTarget === 'string'
        ? payload.subsystemTarget
        : '',
    messageLine:
      typeof payload.messageLine === 'string' ? payload.messageLine : '',
    providerRequestTrace: providerRequestTraceFromPayload(
      payload.providerRequestTrace,
    ),
  };
}

/** The typed provider request trace, or null when the event carries none. */
export function providerRequestTraceFromPayload(
  payload: unknown,
): ProviderRequestTrace | null {
  if (payload === null || typeof payload !== 'object') {
    return null;
  }
  const trace = payload as Record<string, unknown>;
  const responseExcerpt =
    typeof trace.responseExcerpt === 'string' ? trace.responseExcerpt : null;
  return {
    providerKey:
      typeof trace.providerKey === 'string' ? trace.providerKey : '',
    providerContract:
      typeof trace.providerContract === 'string'
        ? trace.providerContract
        : '',
    httpMethod:
      typeof trace.httpMethod === 'string' ? trace.httpMethod : '',
    requestPath:
      typeof trace.requestPath === 'string' ? trace.requestPath : '',
    statusClass:
      typeof trace.statusClass === 'string' ? trace.statusClass : '',
    outcome: typeof trace.outcome === 'string' ? trace.outcome : '',
    durationMilliseconds: Number(trace.durationMilliseconds ?? 0),
    responseBytes: Number(trace.responseBytes ?? 0),
    responseExcerpt,
  };
}
