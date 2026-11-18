#!/usr/bin/env node
// Verifies the Developer Certificate of Origin (DCO) on every commit of a
// revision range.
//
// A commit satisfies the DCO when its message carries at least one
// `Signed-off-by: Name <email>` trailer whose name and email match the commit
// author. The check is deliberately local: no third-party application,
// service or token is involved, so a pull request from a fork is checked by
// the same code as one from a branch in this repository.
//
// Usage:
//   node scripts/check-dco-signoff.mjs [--range <rev-range>]
//
// The default range is `HEAD~1..HEAD`, which checks the current commit. The
// pull request workflow passes `<base-sha>..<head-sha>`.
//
// Exit codes: 0 every commit is signed off; 1 at least one commit is not;
// 2 a usage or git error.

import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

/** Record and field separators used to read commits without a YAML parser. */
const RECORD_SEPARATOR = '\u001e';
const FIELD_SEPARATOR = '\u001f';

/** One `Signed-off-by:` trailer, with its name and email. */
const SIGN_OFF_PATTERN = /^Signed-off-by:\s*(.*?)\s*<([^<>]+)>\s*$/iu;

/** The `git log` pretty format this script parses. */
const GIT_LOG_FORMAT = '%H%x1f%an%x1f%ae%x1f%s%x1f%B%x1e';

/**
 * Extracts every `Signed-off-by:` trailer from a commit message.
 *
 * @param {string} message the full commit message.
 * @returns {{name: string, email: string}[]} the trailers in message order.
 */
export function parseSignOffs(message) {
  const signOffs = [];
  for (const line of message.split(/\r?\n/u)) {
    const match = SIGN_OFF_PATTERN.exec(line.trim());
    if (match !== null) {
      signOffs.push({ name: match[1].trim(), email: match[2].trim() });
    }
  }
  return signOffs;
}

/**
 * Parses the output of `git log` in {@link GIT_LOG_FORMAT}.
 *
 * @param {string} output raw `git log` output.
 * @returns {{sha: string, authorName: string, authorEmail: string,
 *   subject: string, message: string}[]} the commits in log order.
 */
export function parseCommitRecords(output) {
  const commits = [];
  for (const record of output.split(RECORD_SEPARATOR)) {
    const trimmedRecord = record.replace(/^\n/u, '');
    if (trimmedRecord.trim() === '') {
      continue;
    }
    const [sha, authorName, authorEmail, subject, message] =
      trimmedRecord.split(FIELD_SEPARATOR);
    commits.push({
      sha: sha.trim(),
      authorName: authorName.trim(),
      authorEmail: authorEmail.trim(),
      subject: subject.trim(),
      message: message ?? '',
    });
  }
  return commits;
}

/**
 * Checks one commit for a sign-off that matches its author.
 *
 * @param {{sha: string, authorName: string, authorEmail: string,
 *   subject: string, message: string}} commit the commit to check.
 * @returns {{commit: object, signedOff: boolean,
 *   signOffs: {name: string, email: string}[]}} the verdict.
 */
export function evaluateCommit(commit) {
  const signOffs = parseSignOffs(commit.message);
  const signedOff = signOffs.some(
    (signOff) =>
      signOff.name === commit.authorName &&
      signOff.email === commit.authorEmail,
  );
  return { commit, signedOff, signOffs };
}

/**
 * Checks every commit in a list.
 *
 * @param {object[]} commits commits in `parseCommitRecords` shape.
 * @returns {{ok: boolean, results: object[], failures: object[]}} the verdict.
 */
export function checkDcoSignoff(commits) {
  const results = commits.map(evaluateCommit);
  const failures = results.filter((result) => !result.signedOff);
  return { ok: failures.length === 0, results, failures };
}

/**
 * Formats one failure as a single line naming the commit.
 *
 * @param {object} result an `evaluateCommit` result that failed.
 * @returns {string} the operator-facing failure line.
 */
export function formatFailure(result) {
  const { commit, signOffs } = result;
  const shortSha = commit.sha.slice(0, 12);
  const found =
    signOffs.length === 0
      ? 'no Signed-off-by trailer'
      : `Signed-off-by: ${signOffs
          .map((signOff) => `${signOff.name} <${signOff.email}>`)
          .join(', ')}`;
  return (
    `commit ${shortSha} (${commit.subject}) has ${found}; ` +
    `expected "${commit.authorName} <${commit.authorEmail}>"`
  );
}

/**
 * Reads commits from the repository with `git log`.
 *
 * @param {string} range the revision range to read.
 * @param {string} repositoryRoot the repository working directory.
 * @returns {object[]} commits in `parseCommitRecords` shape.
 * @throws {Error} when git cannot read the range.
 */
export function readCommits(range, repositoryRoot) {
  const result = spawnSync(
    'git',
    ['-C', repositoryRoot, 'log', `--format=${GIT_LOG_FORMAT}`, range],
    { encoding: 'utf8' },
  );
  if (result.error) {
    throw new Error(`could not run git: ${result.error.message}`);
  }
  if (result.status !== 0) {
    const detail = `${result.stderr ?? ''}`.trim();
    throw new Error(detail === '' ? 'git log failed' : detail);
  }
  return parseCommitRecords(result.stdout);
}

/** Prints the usage line. */
function usage() {
  process.stderr.write(
    'usage: node scripts/check-dco-signoff.mjs [--range <rev-range>]\n',
  );
}

/**
 * Runs the command line.
 *
 * @param {string[]} arguments_ the process arguments after the script name.
 * @returns {number} the process exit code.
 */
export function main(arguments_) {
  let range = 'HEAD~1..HEAD';
  for (let index = 0; index < arguments_.length; index += 1) {
    const argument = arguments_[index];
    if (argument === '--range') {
      if (index + 1 >= arguments_.length) {
        usage();
        return 2;
      }
      range = arguments_[index + 1];
      index += 1;
    } else if (argument === '--help' || argument === '-h') {
      usage();
      return 0;
    } else {
      process.stderr.write(`error: unknown argument: ${argument}\n`);
      usage();
      return 2;
    }
  }

  const repositoryRoot = path.resolve(
    path.dirname(fileURLToPath(import.meta.url)),
    '..',
  );
  let commits;
  try {
    commits = readCommits(range, repositoryRoot);
  } catch (error) {
    process.stderr.write(`error: ${error.message}\n`);
    return 2;
  }

  const verdict = checkDcoSignoff(commits);
  if (!verdict.ok) {
    for (const failure of verdict.failures) {
      process.stderr.write(`error: ${formatFailure(failure)}\n`);
    }
    process.stderr.write(
      `error: ${verdict.failures.length} of ${verdict.results.length} ` +
        'commit(s) lack a matching Signed-off-by trailer\n',
    );
    return 1;
  }

  process.stdout.write(
    `ok: ${verdict.results.length} commit(s) carry a matching ` +
      'Signed-off-by trailer\n',
  );
  return 0;
}

const invokedDirectly =
  process.argv[1] !== undefined &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);

if (invokedDirectly) {
  process.exitCode = main(process.argv.slice(2));
}
