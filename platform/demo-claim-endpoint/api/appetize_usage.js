// The Appetize usage report the hosted demo reconciles its budget against.
//
// The claim endpoint reserves a whole session up front, because an abandoned
// session must never overrun the free plan. That reservation is deliberately
// pessimistic: a visitor who claims and then leaves without pressing Appetize's
// own Start button costs nothing to stream but still costs the budget. This
// module reads the provider's monthly usage report, so the endpoint can bring
// its counter back to what was actually streamed while keeping one session of
// headroom for claims that are in flight.
//
// The report is the v1 usage summary:
// `GET https://api.appetize.io/v1/usageSummary?startMonth=YYYY-MM` with the
// account API key in `X-API-KEY`. Rows are per app and month, and `minutes` is
// decimal. The SDK never ships this key; it lives in the claim endpoint's Vercel
// environment as `APPETIZE_API_KEY` and is read at request time.

/** The usage-summary endpoint. */
export const APPETIZE_USAGE_URL = 'https://api.appetize.io/v1/usageSummary';

/** How long one report request may take before the caller falls back. */
export const USAGE_TIMEOUT_MILLISECONDS = 2500;

/** The month key the report is asked for, as `YYYY-MM` in UTC. */
export function usageMonthKey(now) {
  return `${now.getUTCFullYear()}-${String(now.getUTCMonth() + 1).padStart(2, '0')}`;
}

/**
 * Reads the month's streamed seconds from the report body.
 *
 * Rows for another month are ignored, and the total rounds up so a partly
 * streamed minute is never free.
 *
 * @param {unknown} body the parsed JSON answer
 * @param {string} month the `YYYY-MM` month the caller asked for
 * @returns {number} streamed seconds
 */
export function streamedSecondsFromUsage(body, month) {
  if (typeof body !== 'object' || body === null || !Array.isArray(body.data)) {
    throw new TypeError('usage summary carries no data array');
  }
  let minutes = 0;
  for (const row of body.data) {
    if (typeof row !== 'object' || row === null) {
      continue;
    }
    if (row.month !== month) {
      continue;
    }
    const value = Number(row.minutes);
    if (Number.isFinite(value) && value > 0) {
      minutes += value;
    }
  }
  return Math.ceil(minutes * 60);
}

/**
 * Asks Appetize how many seconds streamed this month.
 *
 * Answers `null` when no API key is configured, and throws when the request
 * fails or the body is not a usage report, so the caller can fall back to its
 * own reservation instead of granting unmetered sessions.
 *
 * @param {object} options
 * @param {string | undefined} options.apiKey
 * @param {Date} options.now
 * @param {typeof fetch} [options.fetchImplementation]
 * @returns {Promise<number | null>}
 */
export async function fetchMonthlyStreamedSeconds({
  apiKey,
  now,
  fetchImplementation = fetch,
}) {
  if (typeof apiKey !== 'string' || apiKey.trim() === '') {
    return null;
  }
  const month = usageMonthKey(now);
  const url = `${APPETIZE_USAGE_URL}?startMonth=${encodeURIComponent(month)}`;
  const response = await fetchImplementation(url, {
    headers: { 'x-api-key': apiKey },
    signal: AbortSignal.timeout(USAGE_TIMEOUT_MILLISECONDS),
  });
  if (!response.ok) {
    throw new Error(`usage summary answered ${response.status}`);
  }
  return streamedSecondsFromUsage(await response.json(), month);
}
