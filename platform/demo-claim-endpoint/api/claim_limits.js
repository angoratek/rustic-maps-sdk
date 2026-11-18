// The hosted demo's session-budget arithmetic, kept pure so it can be tested
// without Vercel KV (the endpoint in `claim.js` owns the reads, writes and
// HTTP shape; this module owns only the decision).
//
// WHY A VISITOR TOKEN: a client address is not a visitor. Visitors behind a
// carrier NAT or a VPN share an address, and the same visitor's address
// changes when the tunnel reconnects, so an address-keyed daily limit both
// locks out people who never watched a session and lets one person reset
// theirs by toggling the tunnel. The page therefore sends a random token it
// keeps in the browser, and the per-visitor counter is keyed by that token.
// The address counter stays as a courtesy backstop: clearing browser storage
// must not buy unlimited sessions, and the month's minutes remain the hard
// ceiling whatever a client claims.

/** One session's length, which is also what it costs the budget, in seconds. */
export const SESSION_SECONDS = 180;

/** The month's streamed-minute ceiling (the hosting plan's 30 minutes). */
export const MONTHLY_BUDGET_SECONDS = 30 * 60;

/**
 * How many sessions the endpoint grants in one day across all visitors. The
 * month's ten sessions spread over the days without letting one busy day
 * consume them all.
 */
export const DAILY_GRANT_LIMIT = 4;

/** How many sessions one visitor token may claim in one day. */
export const PER_VISITOR_DAILY_LIMIT = 2;

/**
 * How many sessions one client address may claim in one day, whatever tokens
 * it presents. Above the per-visitor limit so a shared address (a household,
 * an office, a VPN exit) can still serve a second genuine visitor.
 */
export const PER_ADDRESS_DAILY_LIMIT = 4;

/** The visitor-token shape the page generates: URL-safe, bounded, opaque. */
export const VISITOR_TOKEN_PATTERN = /^[A-Za-z0-9._-]{8,64}$/;

/** The resting answer a decision may carry. */
function resting(reason, detail, remainingSeconds) {
  return { state: 'resting', reason, detail, remainingSeconds };
}

/** The grant a decision carries when every gate passed. */
function granted(remainingSeconds) {
  return { state: 'ok', sessionSeconds: SESSION_SECONDS, remainingSeconds };
}

/**
 * Decides one claim.
 *
 * The order is deliberate: the month's minutes are the hard ceiling and are
 * checked first, then the day's shared allowance, then the visitor's own
 * allowance, then the address backstop. An operator claim skips the three
 * daily gates — an operator with the secret can always watch while minutes
 * remain — but never the month's budget, which no client can lift.
 *
 * @param {object} facts
 * @param {number} facts.usedSeconds this month's reserved streamed seconds
 * @param {number} facts.grantsToday sessions granted today, all visitors
 * @param {number} facts.visitorGrantsToday sessions this visitor token claimed today
 * @param {number} facts.addressGrantsToday sessions this address claimed today
 * @param {boolean} [facts.isOperator] the request carried the operator secret
 * @returns {{state: 'ok', sessionSeconds: number, remainingSeconds: number}
 *   | {state: 'resting', reason: string, detail: string, remainingSeconds: number}}
 */
export function decideClaim({
  usedSeconds,
  grantsToday,
  visitorGrantsToday,
  addressGrantsToday,
  isOperator = false,
}) {
  const remainingSeconds = MONTHLY_BUDGET_SECONDS - (Number(usedSeconds) || 0);
  if (remainingSeconds < SESSION_SECONDS) {
    return resting(
      'budget',
      'This month’s demo minutes are used up. The demo opens again next month.',
      Math.max(remainingSeconds, 0),
    );
  }
  if (isOperator) {
    return granted(remainingSeconds - SESSION_SECONDS);
  }
  if ((Number(grantsToday) || 0) >= DAILY_GRANT_LIMIT) {
    return resting(
      'daily',
      'Today’s demo sessions are used up. Try again tomorrow.',
      remainingSeconds,
    );
  }
  if ((Number(visitorGrantsToday) || 0) >= PER_VISITOR_DAILY_LIMIT) {
    return resting(
      'visitor',
      'You have used today’s demo sessions. Try again tomorrow.',
      remainingSeconds,
    );
  }
  if ((Number(addressGrantsToday) || 0) >= PER_ADDRESS_DAILY_LIMIT) {
    return resting(
      'address',
      'This network has used today’s demo sessions. Try again tomorrow.',
      remainingSeconds,
    );
  }
  return granted(remainingSeconds - SESSION_SECONDS);
}

/**
 * Reconciles this month's reserved seconds with the provider's usage report.
 *
 * The endpoint reserves a whole session per claim, so a claim that never
 * streamed keeps costing the budget after the fact. The provider's report is
 * what actually streamed, but it can lag a claim that is in flight, so the
 * reconciled figure never drops below one session of headroom above the
 * report, and never rises above what the endpoint already reserved.
 *
 * @param {object} facts
 * @param {number} facts.reservedSeconds the month's reservations so far
 * @param {number | null} facts.streamedSeconds the provider's report, or null
 * @param {number} [facts.sessionSeconds] one session's cost
 * @returns {number} the month's reconciled used seconds
 */
export function reconciledUsedSeconds({
  reservedSeconds,
  streamedSeconds,
  sessionSeconds = SESSION_SECONDS,
}) {
  const reserved = Number(reservedSeconds) || 0;
  if (streamedSeconds === null || streamedSeconds === undefined) {
    return reserved;
  }
  const streamed = Number(streamedSeconds);
  if (!Number.isFinite(streamed) || streamed < 0) {
    return reserved;
  }
  return Math.max(streamed, Math.min(reserved, streamed + sessionSeconds));
}
