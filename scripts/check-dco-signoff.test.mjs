// Tests for scripts/check-dco-signoff.mjs.
//
// Run with: node --test scripts/check-dco-signoff.test.mjs
//
// The three cases are the ones the pull request check must keep honest: every
// commit signed off, a commit with no trailer, and a trailer whose name or
// email does not match the commit author.

import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  checkDcoSignoff,
  formatFailure,
  parseCommitRecords,
  parseSignOffs,
} from './check-dco-signoff.mjs';

/** The author every fixture commit uses. */
const AUTHOR = {
  authorName: 'Ada Lovelace',
  authorEmail: 'ada@example.com',
};

/**
 * Builds a commit in the shape `readCommits` returns.
 *
 * @param {object} overrides fields to replace.
 * @returns {object} the fixture commit.
 */
function commit(overrides = {}) {
  return {
    sha: 'a'.repeat(40),
    ...AUTHOR,
    subject: 'feat: add the analytical engine interface',
    message: [
      'feat: add the analytical engine interface',
      '',
      'Signed-off-by: Ada Lovelace <ada@example.com>',
    ].join('\n'),
    ...overrides,
  };
}

test('accepts a range whose commits all carry a matching sign-off', () => {
  const verdict = checkDcoSignoff([
    commit(),
    commit({ sha: 'b'.repeat(40) }),
  ]);

  assert.equal(verdict.ok, true);
  assert.equal(verdict.failures.length, 0);
});

test('accepts one matching trailer among several sign-offs', () => {
  const verdict = checkDcoSignoff([
    commit({
      message: [
        'fix: keep the punch cards ordered',
        '',
        'Signed-off-by: Charles Babbage <charles@example.com>',
        'Signed-off-by: Ada Lovelace <ada@example.com>',
      ].join('\n'),
    }),
  ]);

  assert.equal(verdict.ok, true);
});

test('fails a commit with no Signed-off-by trailer and lists it', () => {
  const unsigned = commit({
    sha: 'c'.repeat(40),
    subject: 'chore: tidy the notes',
    message: 'chore: tidy the notes\n',
  });

  const verdict = checkDcoSignoff([commit(), unsigned]);

  assert.equal(verdict.ok, false);
  assert.equal(verdict.failures.length, 1);
  const [failure] = verdict.failures;
  assert.equal(failure.commit.sha, unsigned.sha);
  assert.match(formatFailure(failure), /commit c{12} \(chore: tidy the notes\)/u);
  assert.match(formatFailure(failure), /no Signed-off-by trailer/u);
});

test('fails a trailer whose name or email does not match the author', () => {
  const mismatchedName = commit({
    sha: 'd'.repeat(40),
    message: [
      'docs: explain the difference engine',
      '',
      'Signed-off-by: Ada Byron <ada@example.com>',
    ].join('\n'),
  });
  const mismatchedEmail = commit({
    sha: 'e'.repeat(40),
    message: [
      'docs: explain the difference engine',
      '',
      'Signed-off-by: Ada Lovelace <ada@example.org>',
    ].join('\n'),
  });

  const verdict = checkDcoSignoff([mismatchedName, mismatchedEmail]);

  assert.equal(verdict.ok, false);
  assert.equal(verdict.failures.length, 2);
  assert.match(formatFailure(verdict.failures[0]), /Ada Byron/u);
  assert.match(formatFailure(verdict.failures[1]), /ada@example\.org/u);
});

test('parses the git log record and field separators', () => {
  const output =
    `a${'0'.repeat(39)}\u001fAda Lovelace\u001fada@example.com\u001f` +
    'feat: one\u001ffeat: one\n\nSigned-off-by: Ada Lovelace <ada@example.com>' +
    '\u001e' +
    `b${'0'.repeat(39)}\u001fAda Lovelace\u001fada@example.com\u001f` +
    'feat: two\u001ffeat: two\u001e';

  const commits = parseCommitRecords(output);

  assert.equal(commits.length, 2);
  assert.equal(commits[0].subject, 'feat: one');
  assert.equal(commits[1].sha, `b${'0'.repeat(39)}`);
});

test('finds only well-formed Signed-off-by lines', () => {
  const signOffs = parseSignOffs(
    [
      'Signed-off-by: Ada Lovelace <ada@example.com>',
      'signed-off-by: not a trailer',
      'Signed-off-by: no email',
      'Signed-off-by: Grace Hopper <grace@example.com>',
    ].join('\n'),
  );

  assert.deepEqual(signOffs, [
    { name: 'Ada Lovelace', email: 'ada@example.com' },
    { name: 'Grace Hopper', email: 'grace@example.com' },
  ]);
});
