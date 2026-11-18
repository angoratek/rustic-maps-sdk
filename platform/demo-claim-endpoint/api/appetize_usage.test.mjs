// Tests for the Appetize usage report the claim endpoint reconciles against.
// Run with `node --test platform/demo-claim-endpoint/api/appetize_usage.test.mjs`.
import assert from 'node:assert/strict';
import test from 'node:test';
import {
  fetchMonthlyStreamedSeconds,
  streamedSecondsFromUsage,
  usageMonthKey,
} from './appetize_usage.js';

const march = new Date(Date.UTC(2026, 2, 15, 12, 0, 0));

test('the month key is the UTC year and month', () => {
  assert.equal(usageMonthKey(march), '2026-03');
  assert.equal(usageMonthKey(new Date(Date.UTC(2026, 11, 1, 0, 0, 0))), '2026-12');
});

test('the report sums this month across every app and ignores other months', () => {
  const seconds = streamedSecondsFromUsage(
    {
      data: [
        { month: '2026-03', publicKey: 'android', minutes: 1.5 },
        { month: '2026-03', publicKey: 'ios', minutes: 2 },
        { month: '2026-02', publicKey: 'android', minutes: 30 },
      ],
    },
    '2026-03',
  );
  assert.equal(seconds, 210);
});

test('a partly streamed minute rounds up rather than becoming free', () => {
  const seconds = streamedSecondsFromUsage(
    { data: [{ month: '2026-03', minutes: 0.01 }] },
    '2026-03',
  );
  assert.equal(seconds, 1);
});

test('a body without a data array is refused', () => {
  assert.throws(() => streamedSecondsFromUsage({}, '2026-03'), TypeError);
  assert.throws(() => streamedSecondsFromUsage(null, '2026-03'), TypeError);
});

test('no key means no report, so the caller keeps its own reservation', async () => {
  assert.equal(
    await fetchMonthlyStreamedSeconds({ apiKey: undefined, now: march }),
    null,
  );
});

test('the request carries the key and asks for the month', async () => {
  let seenUrl = '';
  let seenKey = '';
  const seconds = await fetchMonthlyStreamedSeconds({
    apiKey: 'test-key',
    now: march,
    fetchImplementation: async (url, options) => {
      seenUrl = String(url);
      seenKey = options.headers['x-api-key'];
      return {
        ok: true,
        json: async () => ({ data: [{ month: '2026-03', minutes: 3 }] }),
      };
    },
  });
  assert.equal(seconds, 180);
  assert.match(seenUrl, /startMonth=2026-03/);
  assert.equal(seenKey, 'test-key');
});

test('a refused request throws so the caller can fall back', async () => {
  await assert.rejects(
    fetchMonthlyStreamedSeconds({
      apiKey: 'test-key',
      now: march,
      fetchImplementation: async () => ({ ok: false, status: 401 }),
    }),
    /401/,
  );
});
