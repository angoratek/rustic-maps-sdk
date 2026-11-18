// Tests for the hosted demo's pure claim arithmetic. Run with
// `node --test platform/demo-claim-endpoint/api/claim_limits.test.mjs`.
import assert from 'node:assert/strict';
import test from 'node:test';
import {
  DAILY_GRANT_LIMIT,
  MONTHLY_BUDGET_SECONDS,
  PER_ADDRESS_DAILY_LIMIT,
  PER_VISITOR_DAILY_LIMIT,
  SESSION_SECONDS,
  decideClaim,
} from './claim_limits.js';

/** Every count at zero, so a case names only the fact it tests. */
function freshClaim(overrides = {}) {
  return decideClaim({
    usedSeconds: 0,
    grantsToday: 0,
    visitorGrantsToday: 0,
    addressGrantsToday: 0,
    ...overrides,
  });
}

test('a fresh claim spends exactly one session from the month', () => {
  const decision = freshClaim();
  assert.equal(decision.state, 'ok');
  assert.equal(decision.sessionSeconds, SESSION_SECONDS);
  assert.equal(decision.remainingSeconds, MONTHLY_BUDGET_SECONDS - SESSION_SECONDS);
});

test('a month with less than one session left rests instead of granting a partial one', () => {
  const decision = freshClaim({ usedSeconds: MONTHLY_BUDGET_SECONDS - SESSION_SECONDS + 1 });
  assert.equal(decision.state, 'resting');
  assert.equal(decision.reason, 'budget');
  assert.equal(decision.remainingSeconds, SESSION_SECONDS - 1);
});

test('the day\'s shared allowance is checked before the visitor\'s own', () => {
  const decision = freshClaim({
    grantsToday: DAILY_GRANT_LIMIT,
    visitorGrantsToday: PER_VISITOR_DAILY_LIMIT,
  });
  assert.equal(decision.state, 'resting');
  assert.equal(decision.reason, 'daily');
});

test('a visitor who used their two sessions rests while the day still has room', () => {
  const decision = freshClaim({ visitorGrantsToday: PER_VISITOR_DAILY_LIMIT });
  assert.equal(decision.state, 'resting');
  assert.equal(decision.reason, 'visitor');
});

test('a shared address is stopped at its backstop, not at a single visitor', () => {
  const allowedForSecondVisitor = freshClaim({
    addressGrantsToday: PER_VISITOR_DAILY_LIMIT,
  });
  assert.equal(allowedForSecondVisitor.state, 'ok');

  const stoppedAtBackstop = freshClaim({
    addressGrantsToday: PER_ADDRESS_DAILY_LIMIT,
  });
  assert.equal(stoppedAtBackstop.state, 'resting');
  assert.equal(stoppedAtBackstop.reason, 'address');
});

test('an operator claim skips the daily gates but never the month\'s budget', () => {
  const operatorGrant = freshClaim({
    grantsToday: DAILY_GRANT_LIMIT,
    visitorGrantsToday: PER_VISITOR_DAILY_LIMIT,
    addressGrantsToday: PER_ADDRESS_DAILY_LIMIT,
    isOperator: true,
  });
  assert.equal(operatorGrant.state, 'ok');
  assert.equal(
    operatorGrant.remainingSeconds,
    MONTHLY_BUDGET_SECONDS - SESSION_SECONDS,
  );

  const exhaustedMonth = freshClaim({
    usedSeconds: MONTHLY_BUDGET_SECONDS,
    isOperator: true,
  });
  assert.equal(exhaustedMonth.state, 'resting');
  assert.equal(exhaustedMonth.reason, 'budget');
});

test('resting answers always carry the minutes left', () => {
  const decision = freshClaim({ usedSeconds: 4 * SESSION_SECONDS, grantsToday: DAILY_GRANT_LIMIT });
  assert.equal(decision.state, 'resting');
  assert.equal(decision.remainingSeconds, MONTHLY_BUDGET_SECONDS - 4 * SESSION_SECONDS);
});
