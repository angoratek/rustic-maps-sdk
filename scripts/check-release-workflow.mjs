#!/usr/bin/env node
// Structural check for `.github/workflows/release.yml`.
//
// The release workflow must keep its input-gated rebuild shape: a `plan` job
// publishes per-platform decisions, each platform build job carries a
// job-level `if` that consults its decision, a `restore` job republishes the
// artifacts the plan marked reusable, and the `assemble` job runs even when a
// build job was skipped while still asserting the exact asset set.
//
// A full YAML parser is not a dependency of this repository, so the workflow
// is parsed by the small block-mapping reader below (the file uses only block
// mappings and block/flow sequences). When `actionlint` is installed it also
// runs over the file; a missing actionlint is reported, never failed.
//
// Usage:
//   node scripts/check-release-workflow.mjs
//
// Exit 0 when every structural assertion holds, 1 otherwise.

import { readFileSync } from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const REPOSITORY_ROOT = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);
const WORKFLOW_RELATIVE_PATH = '.github/workflows/release.yml';
const PLATFORM_NAMES = ['android', 'ios', 'wasm', 'rn'];

/** Leading-space count of a line. */
function countIndent(line) {
  return /^ */u.exec(line)[0].length;
}

/** True when a mapping key starts on this line (`key: value` or `key:`). */
function isMappingLine(content) {
  return /^[^:#]+:(?: |$)/u.test(content);
}

/** Splits `key: value` into its two halves. */
function splitMapping(content) {
  const colonIndex = content.indexOf(':');
  return [
    content.slice(0, colonIndex).trim(),
    content.slice(colonIndex + 1).trim(),
  ];
}

/** Parses a scalar, including one-line flow sequences. */
function parseScalar(rawValue) {
  let value = rawValue;
  const commentIndex = value.indexOf(' #');
  if (commentIndex >= 0 && !value.startsWith("'") && !value.startsWith('"')) {
    value = value.slice(0, commentIndex).trimEnd();
  }
  if (
    (value.startsWith("'") && value.endsWith("'")) ||
    (value.startsWith('"') && value.endsWith('"'))
  ) {
    return value.slice(1, -1);
  }
  if (value.startsWith('[') && value.endsWith(']')) {
    return value
      .slice(1, -1)
      .split(',')
      .map((item) => item.trim())
      .filter((item) => item !== '')
      .map((item) => item.replace(/^['"]|['"]$/gu, ''));
  }
  return value;
}

/**
 * Parses the block-mapping and block-sequence subset used by the workflow.
 *
 * Block scalars (`run: |`) are skipped; only mapping keys and scalar values
 * are materialized, which is all the structural assertions need.
 */
export function parseWorkflowYaml(text) {
  const root = {};
  const stack = [{ indent: -1, node: root, holder: null }];
  let blockScalar = null;

  for (const rawLine of text.split(/\r?\n/u)) {
    const trimmed = rawLine.trim();
    if (blockScalar) {
      if (trimmed === '') {
        blockScalar.lines.push('');
        continue;
      }
      if (countIndent(rawLine) > blockScalar.indent) {
        blockScalar.lines.push(rawLine);
        continue;
      }
      const contentLines = blockScalar.lines;
      const nonEmptyLines = contentLines.filter((line) => line.trim() !== '');
      const minimumIndent =
        nonEmptyLines.length === 0
          ? 0
          : Math.min(...nonEmptyLines.map(countIndent));
      blockScalar.container[blockScalar.key] = contentLines
        .map((line) =>
          line.trim() === '' ? '' : line.slice(minimumIndent).trimEnd(),
        )
        .join('\n')
        .trim();
      blockScalar = null;
    }
    if (trimmed === '' || trimmed.startsWith('#')) {
      continue;
    }

    const indent = countIndent(rawLine);
    const content = rawLine.slice(indent).trimEnd();
    while (stack.length > 1 && indent <= stack[stack.length - 1].indent) {
      stack.pop();
    }
    const frame = stack[stack.length - 1];

    if (content === '-' || content.startsWith('- ')) {
      if (!Array.isArray(frame.node)) {
        if (!frame.holder) {
          throw new Error(`sequence without a mapping key at: ${rawLine}`);
        }
        frame.holder.container[frame.holder.key] = [];
        frame.node = frame.holder.container[frame.holder.key];
      }
      const itemContent = content === '-' ? '' : content.slice(2).trim();
      if (itemContent === '') {
        const item = {};
        frame.node.push(item);
        stack.push({ indent: indent + 1, node: item, holder: null });
      } else if (isMappingLine(itemContent)) {
        const [key, value] = splitMapping(itemContent);
        const item = {};
        frame.node.push(item);
        item[key] = value === '' ? {} : parseScalar(value);
        stack.push({ indent: indent + 1, node: item, holder: null });
      } else {
        frame.node.push(parseScalar(itemContent));
      }
      continue;
    }

    if (!isMappingLine(content)) {
      continue;
    }
    const [key, value] = splitMapping(content);
    if (Array.isArray(frame.node)) {
      throw new Error(`mapping inside a sequence at: ${rawLine}`);
    }
    if (value === '' || value === '-' || value.startsWith('# ')) {
      const child = {};
      frame.node[key] = child;
      stack.push({
        indent,
        node: child,
        holder: { container: frame.node, key },
      });
    } else if (/^[|>][+-]?$/u.test(value)) {
      frame.node[key] = '';
      blockScalar = { container: frame.node, key, indent, lines: [] };
    } else {
      frame.node[key] = parseScalar(value);
    }
  }
  return root;
}

/** Normalizes `needs:` to an array of job names. */
function needsOf(job) {
  if (!job) {
    return [];
  }
  if (Array.isArray(job.needs)) {
    return job.needs;
  }
  if (typeof job.needs === 'string' && job.needs !== '') {
    return [job.needs];
  }
  return [];
}

/** Collects every `uses`/`name` string in a job's steps for simple assertions. */
function jobStepText(job) {
  if (!job || !Array.isArray(job.steps)) {
    return '';
  }
  return JSON.stringify(job.steps);
}

/**
 * Runs the workflow checks.
 *
 * Returns `{ errors, actionlint, workflow }`; `errors` is empty on success.
 */
export function checkReleaseWorkflow({
  repositoryRoot = REPOSITORY_ROOT,
  runActionlint = true,
} = {}) {
  const errors = [];
  const workflowPath = path.join(repositoryRoot, WORKFLOW_RELATIVE_PATH);
  let workflow;
  let text;
  try {
    text = readFileSync(workflowPath, 'utf8');
    workflow = parseWorkflowYaml(text);
  } catch (error) {
    return {
      errors: [`cannot parse ${WORKFLOW_RELATIVE_PATH}: ${error.message}`],
      actionlint: 'not run',
      workflow: null,
    };
  }

  const jobs = workflow.jobs ?? {};
  const plan = jobs.plan;
  if (!plan) {
    errors.push('the workflow has no plan job');
  } else {
    const outputs = plan.outputs ?? {};
    for (const platform of PLATFORM_NAMES) {
      if (!Object.hasOwn(outputs, platform)) {
        errors.push(`plan.outputs is missing ${platform}`);
      }
      if (!Object.hasOwn(outputs, `${platform}_hash`)) {
        errors.push(`plan.outputs is missing ${platform}_hash`);
      }
    }
    if (!Object.hasOwn(outputs, 'prior_run_id')) {
      errors.push('plan.outputs is missing prior_run_id');
    }
    if (!jobStepText(plan).includes('scripts/release-plan.mjs')) {
      errors.push('the plan job does not run scripts/release-plan.mjs');
    }
    if (!jobStepText(plan).includes('--require-artifact-proof')) {
      errors.push('the plan job does not require artifact proof');
    }
    if (!jobStepText(plan).includes('--available-artifacts')) {
      errors.push('the plan job does not check the prior run artifact list');
    }
    if (!jobStepText(plan).includes('release-plan')) {
      errors.push('the plan job does not upload its plan artifact');
    }
    if (!needsOf(plan).includes('gates')) {
      errors.push('the plan job does not need the gates job');
    }
  }

  for (const platform of PLATFORM_NAMES) {
    const job = jobs[platform];
    if (!job) {
      errors.push(`the workflow has no ${platform} job`);
      continue;
    }
    if (!needsOf(job).includes('plan')) {
      errors.push(`the ${platform} job does not need the plan job`);
    }
    const condition = typeof job.if === 'string' ? job.if : '';
    if (!condition.includes(`needs.plan.outputs.${platform}`)) {
      errors.push(
        `the ${platform} job has no job-level gate on needs.plan.outputs.${platform}`,
      );
    }
    if (!condition.includes("'run'")) {
      errors.push(`the ${platform} job's gate does not compare to 'run'`);
    }
  }

  const restore = jobs.restore;
  if (!restore) {
    errors.push('the workflow has no restore job');
  } else {
    if (!needsOf(restore).includes('plan')) {
      errors.push('the restore job does not need the plan job');
    }
    const restoreText = jobStepText(restore);
    if (!restoreText.includes('scripts/restore-release-artifact.sh')) {
      errors.push('the restore job does not run the restore script');
    }
    for (const platform of PLATFORM_NAMES) {
      if (!restoreText.includes(`release-${platform}`)) {
        errors.push(`the restore job has no release-${platform} upload`);
      }
    }
  }

  const assemble = jobs.assemble;
  if (!assemble) {
    errors.push('the workflow has no assemble job');
  } else {
    const needs = needsOf(assemble);
    for (const dependency of ['gates', 'plan', 'restore', ...PLATFORM_NAMES]) {
      if (!needs.includes(dependency)) {
        errors.push(`the assemble job does not need ${dependency}`);
      }
    }
    const condition = typeof assemble.if === 'string' ? assemble.if : '';
    if (!condition.includes('!cancelled()')) {
      errors.push('the assemble job does not run after skipped build jobs');
    }
    for (const platform of PLATFORM_NAMES) {
      if (!condition.includes(`needs.${platform}.result`)) {
        errors.push(
          `the assemble job does not check the ${platform} job result`,
        );
      }
      if (!condition.includes("'skipped'")) {
        errors.push(
          `the assemble job does not accept a skipped ${platform} job`,
        );
      }
    }
    if (!text.includes('does not hold exactly the expected release asset set')) {
      errors.push('the assemble job lost its exact asset-set assertion');
    }
    if (!jobStepText(assemble).includes('referenceInputHash')) {
      errors.push('the assemble job does not re-check reused input hashes');
    }
  }

  let actionlint = 'not installed';
  if (runActionlint) {
    const result = spawnSync('actionlint', [workflowPath], {
      cwd: repositoryRoot,
      encoding: 'utf8',
    });
    if (result.error) {
      if (result.error.code !== 'ENOENT') {
        errors.push(`actionlint could not run: ${result.error.message}`);
      }
    } else if (result.status !== 0) {
      actionlint = 'failed';
      const output = `${result.stdout ?? ''}${result.stderr ?? ''}`.trim();
      errors.push(`actionlint reported:\n${output}`);
    } else {
      actionlint = 'passed';
    }
  }

  return { errors, actionlint, workflow };
}

function main() {
  const { errors, actionlint } = checkReleaseWorkflow();
  if (errors.length > 0) {
    for (const error of errors) {
      process.stderr.write(`error: ${error}\n`);
    }
    return 1;
  }
  process.stdout.write(
    `release workflow structure is intact (actionlint: ${actionlint})\n`,
  );
  return 0;
}

const invokedDirectly =
  process.argv[1] !== undefined &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);

if (invokedDirectly) {
  process.exitCode = main();
}
