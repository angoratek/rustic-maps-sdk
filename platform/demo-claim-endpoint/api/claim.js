// The session-budget claim endpoint for the hosted mobile demo (PLAN.md, M24).
//
// The docs site's `/demo/mobile` page never embeds a stream on load. A visitor
// who presses Start asks this endpoint for a session; the endpoint decides
// whether the month's streamed minutes allow one, records the reservation, and
// answers either a grant or an honest resting state. The page then starts the
// Appetize session itself.
//
// WHY A SERVER: Appetize's Free plan allows 30 streamed minutes a month. A
// per-browser limit is not a limit — a crawler, a reload or a second tab would
// bypass it. The counter lives in Vercel KV so it survives cold starts, and
// every grant pays for the whole session up front, so a session abandoned
// early can never overrun the budget (the pessimistic direction).
//
// SECRETS: `KV_REST_API_URL`/`KV_REST_API_TOKEN` are injected by the linked
// Vercel KV store; `DEMO_KILL_SWITCH` is an operator switch; `DEMO_OPERATOR_TOKEN`
// lets whoever holds it watch while minutes remain, without lifting the
// month's budget. The Appetize API token is NOT here — the page embeds a
// public build id and never uploads.

import { timingSafeEqual } from 'node:crypto';
import { kv } from '@vercel/kv';
import {
  MONTHLY_BUDGET_SECONDS,
  SESSION_SECONDS,
  VISITOR_TOKEN_PATTERN,
  decideClaim,
} from './claim_limits.js';

// The public origin the page is served from. The hosted-demo page ships with
// the documentation site, so the allowlist names the Pages origin the site
// publishes from and nothing else.
const ALLOWED_ORIGINS = new Set([
  'https://angoratek.github.io',
]);

/** The answer's `access-control-allow-origin` value for one request. */
function allowedOrigin(requestedOrigin) {
  if (typeof requestedOrigin === 'string' && ALLOWED_ORIGINS.has(requestedOrigin)) {
    return requestedOrigin;
  }
  return ALLOWED_ORIGINS.values().next().value;
}

/** The platforms a page may ask for. */
const KNOWN_PLATFORMS = new Set(['android', 'ios', 'react-native']);

/** Seconds a counter key lives, so stale counters cannot accumulate. */
const MONTH_KEY_TIME_TO_LIVE_SECONDS = 40 * 24 * 60 * 60;
const DAY_KEY_TIME_TO_LIVE_SECONDS = 3 * 24 * 60 * 60;

/** Writes one JSON answer with the page's CORS policy and no caching. */
function send(response, statusCode, body) {
  response.status(statusCode);
  response.setHeader('content-type', 'application/json; charset=utf-8');
  response.setHeader('access-control-allow-methods', 'GET, OPTIONS');
  response.setHeader('cache-control', 'no-store');
  response.send(JSON.stringify(body));
}

/** The UTC month key the budget is accounted under. */
function monthKey(now) {
  return `demo:budget:${now.getUTCFullYear()}-${String(now.getUTCMonth() + 1).padStart(2, '0')}`;
}

/** The UTC day key a daily limit is accounted under. */
function dayKey(now, suffix) {
  const day = [
    now.getUTCFullYear(),
    String(now.getUTCMonth() + 1).padStart(2, '0'),
    String(now.getUTCDate()).padStart(2, '0'),
  ].join('-');
  return `demo:${suffix}:${day}`;
}

/**
 * The client address as the platform reports it.
 *
 * Vercel overwrites `x-forwarded-for`, so its first entry is the visitor; the
 * address counter is a courtesy backstop, not the budget — the month's
 * minutes are the hard ceiling whatever a client claims.
 */
function clientAddress(headers) {
  const forwarded = headers['x-forwarded-for'];
  if (typeof forwarded === 'string' && forwarded.trim() !== '') {
    return forwarded.split(',')[0].trim();
  }
  return typeof headers['x-real-ip'] === 'string' ? headers['x-real-ip'] : 'unknown';
}

/** The browser-held visitor token the page sends, or null when it is absent. */
function visitorToken(request) {
  const token = typeof request.query.visitor === 'string' ? request.query.visitor.trim() : '';
  return VISITOR_TOKEN_PATTERN.test(token) ? token : null;
}

/**
 * Whether the request carries the operator secret. The comparison is
 * constant-time over equal-length buffers, and an unset or short secret can
 * never match (so a misconfigured project cannot be opened by an empty query
 * parameter).
 */
function isOperatorRequest(request) {
  const operatorToken = process.env.DEMO_OPERATOR_TOKEN;
  const providedToken = typeof request.query.operatorToken === 'string'
    ? request.query.operatorToken
    : '';
  if (typeof operatorToken !== 'string' || operatorToken.length < 16) {
    return false;
  }
  const expected = Buffer.from(operatorToken);
  const provided = Buffer.from(providedToken);
  return provided.length === expected.length && timingSafeEqual(provided, expected);
}

/** Increments a counter and gives it a lifetime, answering the new value. */
async function incrementWithLifetime(key, amount, lifetimeSeconds) {
  const value = amount === 1 ? await kv.incr(key) : await kv.incrby(key, amount);
  await kv.expire(key, lifetimeSeconds);
  return value;
}

export default async function handler(request, response) {
  response.setHeader('access-control-allow-origin', allowedOrigin(request.headers.origin));
  response.setHeader('vary', 'origin');
  if (request.method === 'OPTIONS') {
    send(response, 204, {});
    return;
  }
  if (request.method !== 'GET') {
    send(response, 405, { state: 'error', detail: 'use GET' });
    return;
  }

  const platform = typeof request.query.platform === 'string' ? request.query.platform : '';
  if (!KNOWN_PLATFORMS.has(platform)) {
    send(response, 400, { state: 'error', detail: 'unknown platform' });
    return;
  }

  if (process.env.DEMO_KILL_SWITCH === 'true') {
    send(response, 200, {
      state: 'resting',
      reason: 'maintenance',
      detail: 'The hosted demo is switched off right now.',
    });
    return;
  }

  const now = new Date();
  const budgetCounterKey = monthKey(now);
  const grantCounterKey = dayKey(now, 'grants');
  const address = clientAddress(request.headers);
  const addressCounterKey = dayKey(now, `address:${address}`);
  const token = visitorToken(request);
  const visitorCounterKey = token === null ? null : dayKey(now, `visitor:${token}`);

  let usedSeconds;
  let grantsToday;
  let visitorGrantsToday;
  let addressGrantsToday;
  try {
    [usedSeconds, grantsToday, visitorGrantsToday, addressGrantsToday] = await Promise.all([
      kv.get(budgetCounterKey),
      kv.get(grantCounterKey),
      visitorCounterKey === null ? Promise.resolve(0) : kv.get(visitorCounterKey),
      kv.get(addressCounterKey),
    ]);
  } catch {
    // The budget store is unreachable or unlinked. Refusing is the only
    // honest answer: granting unmetered sessions is how a free plan's minutes
    // disappear.
    send(response, 503, {
      state: 'error',
      detail: 'The demo’s session service is unavailable right now.',
    });
    return;
  }

  // A read-only status answer for the maintainer (and anyone curious): the
  // same counters the decision reads, without reserving a session. It exists
  // so "the demo says I used today's session" can be checked without spending
  // three of the month's thirty minutes on a claim.
  if (request.query.status === 'true') {
    send(response, 200, {
      state: 'status',
      remainingSeconds: Math.max(
        MONTHLY_BUDGET_SECONDS - (Number(usedSeconds) || 0),
        0,
      ),
      grantsToday: Number(grantsToday) || 0,
      visitorGrantsToday: Number(visitorGrantsToday) || 0,
      addressGrantsToday: Number(addressGrantsToday) || 0,
    });
    return;
  }

  const decision = decideClaim({
    usedSeconds: Number(usedSeconds) || 0,
    grantsToday: Number(grantsToday) || 0,
    visitorGrantsToday: Number(visitorGrantsToday) || 0,
    addressGrantsToday: Number(addressGrantsToday) || 0,
    isOperator: isOperatorRequest(request),
  });
  if (decision.state === 'resting') {
    send(response, 200, { ...decision, platform });
    return;
  }

  // Reserve the whole session before granting it: an abandoned session costs
  // the budget exactly what a completed one does, never less.
  try {
    const reservations = [
      incrementWithLifetime(budgetCounterKey, SESSION_SECONDS, MONTH_KEY_TIME_TO_LIVE_SECONDS),
      incrementWithLifetime(grantCounterKey, 1, DAY_KEY_TIME_TO_LIVE_SECONDS),
      incrementWithLifetime(addressCounterKey, 1, DAY_KEY_TIME_TO_LIVE_SECONDS),
    ];
    if (visitorCounterKey !== null) {
      reservations.push(
        incrementWithLifetime(visitorCounterKey, 1, DAY_KEY_TIME_TO_LIVE_SECONDS),
      );
    }
    await Promise.all(reservations);
  } catch {
    // The reservation did not land, so no session is granted: an unrecorded
    // grant is exactly the hole this endpoint exists to close.
    send(response, 503, {
      state: 'error',
      detail: 'The demo’s session service is unavailable right now.',
    });
    return;
  }

  send(response, 200, { ...decision, platform });
}
