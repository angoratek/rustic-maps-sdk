#!/usr/bin/env node
// Release build plan: which platform packaging jobs must run for a revision,
// and which can reuse an artifact whose inputs are unchanged.
//
// A release re-run is expensive because every platform rebuild is repeated.
// The plan compares a deterministic hash of each platform's inputs with the
// hash recorded beside the artifact a prior run produced. A platform is
// reusable only when both hashes agree and the artifact bytes were recorded;
// everything else runs. The result never authorizes a missing artifact:
// `.github/workflows/release.yml` passes the plan to the platform jobs, and
// the assemble job still asserts the exact published asset set.
//
// WHAT THE INPUT HASH COVERS
//   Each platform group lists the tracked paths that can change its artifact:
//   the platform sources, the Rust workspace closure it compiles, the build
//   and packaging scripts, the version-bearing manifests, and the license and
//   notice files the package ships. The React Native package also mixes in
//   the Android and iOS hashes because it carries their compiled payload.
//
// HOW THE HASH IS COMPUTED
//   `git ls-tree` enumerates every tracked file at the revision, and the
//   hash is SHA-256 over the sorted `<path>\0<git blob object id>` lines.
//   Using the blob id avoids reading file bodies and makes a comparison
//   between two revisions exact and fast.
//
// USAGE
//   Plan mode (the default), for a pre-release check:
//     node scripts/release-plan.mjs --base "$(git describe --tags --abbrev=0)"
//     node scripts/release-plan.mjs --base v0.3.0 --head HEAD --json
//
//   Plan mode against a prior run's records (what the release workflow does):
//     node scripts/release-plan.mjs \
//       --prior-directory prior --require-artifact-proof --output plan.json
//
//   Record an artifact and its input hash for a later re-run:
//     node scripts/release-plan.mjs --record-artifact android \
//       --version 0.4.0 --commit "$(git rev-parse HEAD)" \
//       --input-hash sha256:... --artifact dist/rustic-maps-sdk-android.aar \
//       --output dist/release-artifact-android.json
//
// The script reads local git state only; it never touches the network.

import { createHash } from 'node:crypto';
import { createReadStream, existsSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

export const REPOSITORY_ROOT = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);

/** The plan and per-platform record schema version. */
export const RELEASE_PLAN_SCHEMA_VERSION = 1;

/** Upper bound for git's stdout (the repository's full tree listing). */
const GIT_OUTPUT_BUFFER_BYTES = 1024 * 1024 * 1024;

/** Platform job names, in dependency order (React Native last). */
export const PLATFORM_NAMES = Object.freeze(['android', 'ios', 'wasm', 'rn']);

/**
 * The workflow artifacts a platform job uploads. A reusable platform must
 * still hold every one of these in the prior run; the React Native job
 * consumes the platform payloads, so a missing companion means a rebuild.
 */
export const PLATFORM_ARTIFACT_NAMES = Object.freeze({
  android: Object.freeze(['release-android', 'release-android-static']),
  ios: Object.freeze([
    'release-ios',
    'release-ios-framework',
    'release-ios-sources',
  ]),
  wasm: Object.freeze(['release-wasm']),
  rn: Object.freeze(['release-rn']),
});

/** Version-bearing manifests and the Rust closure every package compiles. */
const SHARED_RUST_CLOSURE_PATHS = Object.freeze([
  'Cargo.toml',
  'Cargo.lock',
  'rust-toolchain.toml',
  'crates',
]);

/** Files bundled verbatim into every package (docs/Release.md (The artifacts)). */
const SHARED_PACKAGE_PATHS = Object.freeze([
  'LICENSE-MIT',
  'LICENSE-APACHE',
  'docs/THIRD_PARTY_NOTICES.md',
]);

/**
 * The workflow pins the build environment (Java, Node, Gradle, NDK, toolchain
 * actions) that can change artifact bytes, so it belongs to every group.
 */
const SHARED_BUILD_DEFINITION_PATHS = Object.freeze([
  '.github/workflows/release.yml',
]);

/**
 * The tracked inputs that can change each platform artifact.
 *
 * Paths are conservative on purpose: an unnecessary rebuild is harmless,
 * while a skipped rebuild of a changed artifact would ship the wrong bytes.
 */
export const PLATFORM_INPUT_GROUPS = Object.freeze({
  android: Object.freeze({
    paths: Object.freeze([
      'platform/android',
      'scripts/build-android.sh',
      'scripts/check-workspace-version.sh',
      ...SHARED_RUST_CLOSURE_PATHS,
      ...SHARED_PACKAGE_PATHS,
      ...SHARED_BUILD_DEFINITION_PATHS,
    ]),
  }),
  ios: Object.freeze({
    paths: Object.freeze([
      'platform/ios',
      // build-ios.sh refreshes the Swift package's resource copies from these
      // two shared artifacts, so their bytes reach the package.
      'platform/providers.json',
      'platform/reference-scenes.json',
      'scripts/build-ios.sh',
      'scripts/check-workspace-version.sh',
      ...SHARED_RUST_CLOSURE_PATHS,
      ...SHARED_PACKAGE_PATHS,
      ...SHARED_BUILD_DEFINITION_PATHS,
    ]),
  }),
  wasm: Object.freeze({
    paths: Object.freeze([
      'platform/web',
      'scripts/build-wasm2-package.sh',
      'scripts/generate-wasm-bindings.sh',
      'scripts/add-generated-esm-extensions.mjs',
      'scripts/package-web-npm.sh',
      'scripts/check-wasm2-package-freshness.mjs',
      'scripts/check-workspace-version.sh',
      ...SHARED_RUST_CLOSURE_PATHS,
      ...SHARED_PACKAGE_PATHS,
      ...SHARED_BUILD_DEFINITION_PATHS,
    ]),
  }),
  rn: Object.freeze({
    paths: Object.freeze([
      'platform/rn',
      'scripts/package-rn-npm.sh',
      'scripts/verify-rn-package.mjs',
      'scripts/generate-rn-bindings.sh',
      'scripts/check-workspace-version.sh',
      ...SHARED_RUST_CLOSURE_PATHS,
      ...SHARED_PACKAGE_PATHS,
      ...SHARED_BUILD_DEFINITION_PATHS,
    ]),
    // The React Native tarball carries the Android static archives and the
    // iOS XCFramework, so their build inputs are its inputs too.
    dependsOn: Object.freeze(['android', 'ios']),
  }),
});

/** Runs a git command in the repository and returns trimmed stdout. */
function runGit(repositoryRoot, gitArguments, { allowFailure = false } = {}) {
  const result = spawnSync('git', gitArguments, {
    cwd: repositoryRoot,
    encoding: 'utf8',
    maxBuffer: GIT_OUTPUT_BUFFER_BYTES,
  });
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    if (allowFailure) {
      return null;
    }
    throw new Error(
      `git ${gitArguments.join(' ')} failed: ${(result.stderr || '').trim()}`,
    );
  }
  return result.stdout.trim();
}

/** True when a revision resolves in this repository. */
export function revisionExists(repositoryRoot, revision) {
  return runGit(
    repositoryRoot,
    ['rev-parse', '--verify', '--quiet', `${revision}^{commit}`],
    { allowFailure: true },
  ) !== null;
}

/**
 * Lists the tracked blob entries under `paths` at `revision`.
 *
 * Returns `[{ path, blobId }]`, sorted by path and de-duplicated.
 */
export function listRevisionFiles(repositoryRoot, revision, paths) {
  const result = spawnSync(
    'git',
    ['ls-tree', '-r', '-z', '--full-tree', revision, '--', ...paths],
    { cwd: repositoryRoot, maxBuffer: GIT_OUTPUT_BUFFER_BYTES },
  );
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(
      `git ls-tree ${revision} failed: ${String(result.stderr).trim()}`,
    );
  }
  const entriesByPath = new Map();
  for (const record of String(result.stdout).split('\0')) {
    if (record === '') {
      continue;
    }
    const tabIndex = record.indexOf('\t');
    if (tabIndex < 0) {
      continue;
    }
    const metadata = record.slice(0, tabIndex).split(' ');
    const objectType = metadata[1];
    if (objectType !== 'blob') {
      continue;
    }
    entriesByPath.set(record.slice(tabIndex + 1), {
      path: record.slice(tabIndex + 1),
      blobId: metadata[2],
    });
  }
  return [...entriesByPath.values()].sort((left, right) =>
    left.path < right.path ? -1 : left.path > right.path ? 1 : 0,
  );
}

/**
 * Computes the platform input hash from a map of platform to file entries.
 *
 * Accepts fixture lists in tests and the output of `listRevisionFiles` in the
 * command line path. React Native's hash includes its dependencies' hashes.
 */
export function computePlatformHashes(platformFiles) {
  const hashes = {};
  for (const platform of PLATFORM_NAMES) {
    const entries = [...(platformFiles[platform] ?? [])];
    for (const dependency of PLATFORM_INPUT_GROUPS[platform].dependsOn ?? []) {
      if (!hashes[dependency]) {
        throw new Error(
          `platform ${platform} depends on ${dependency}, which has no hash yet`,
        );
      }
      entries.push({
        path: `@dependency/${dependency}`,
        blobId: hashes[dependency].inputHash,
      });
    }
    hashes[platform] = {
      inputHash: hashFileEntries(entries),
      fileCount: (platformFiles[platform] ?? []).length,
    };
  }
  return hashes;
}

/** SHA-256 over sorted `<path>\0<blobId>` lines. */
export function hashFileEntries(entries) {
  const digest = createHash('sha256');
  const sorted = [...entries].sort((left, right) =>
    left.path < right.path ? -1 : left.path > right.path ? 1 : 0,
  );
  for (const entry of sorted) {
    digest.update(entry.path);
    digest.update('\0');
    digest.update(entry.blobId);
    digest.update('\n');
  }
  return `sha256:${digest.digest('hex')}`;
}

/** Reads every platform's tracked inputs at `revision`. */
export function readRevisionFileMap(repositoryRoot, revision) {
  const fileMap = {};
  for (const platform of PLATFORM_NAMES) {
    fileMap[platform] = listRevisionFiles(
      repositoryRoot,
      revision,
      PLATFORM_INPUT_GROUPS[platform].paths,
    );
  }
  return fileMap;
}

/** Reads the `[workspace.package] version` line from a Cargo.toml body. */
export function parseWorkspaceVersion(cargoTomlText) {
  let inWorkspacePackage = false;
  for (const line of cargoTomlText.split(/\r?\n/u)) {
    if (/^\[workspace\.package\]\s*$/u.test(line)) {
      inWorkspacePackage = true;
      continue;
    }
    if (inWorkspacePackage && /^\[/u.test(line)) {
      inWorkspacePackage = false;
      continue;
    }
    const match = inWorkspacePackage
      ? /^version\s*=\s*"([^"]+)"/u.exec(line)
      : null;
    if (match) {
      return match[1];
    }
  }
  throw new Error('Cargo.toml has no [workspace.package] version');
}

/** The workspace version at a revision. */
export function versionAtRevision(repositoryRoot, revision) {
  const manifest = runGit(repositoryRoot, ['show', `${revision}:Cargo.toml`], {
    allowFailure: true,
  });
  return manifest === null ? null : parseWorkspaceVersion(manifest);
}

/** The workspace version in the working tree. */
export function readWorkspaceVersion(repositoryRoot) {
  return parseWorkspaceVersion(
    readFileSync(path.join(repositoryRoot, 'Cargo.toml'), 'utf8'),
  );
}

/** Loads a combined prior-input record (`release-plan.json` shape). */
export function readPriorInputs(filePath) {
  const record = JSON.parse(readFileSync(filePath, 'utf8'));
  const platforms = {};
  for (const platform of PLATFORM_NAMES) {
    const entry = record.platforms?.[platform];
    if (!entry) {
      continue;
    }
    platforms[platform] = {
      inputHash: entry.inputHash,
      artifacts: entry.artifacts ?? [],
      version: entry.version ?? record.version ?? null,
    };
  }
  return {
    label: 'the recorded release inputs',
    version: record.version ?? null,
    platforms,
  };
}

/** Reads the prior run's artifact names, one per line (empty when absent). */
export function readAvailableArtifacts(filePath) {
  if (!filePath || !existsSync(filePath)) {
    return [];
  }
  return readFileSync(filePath, 'utf8')
    .split(/\r?\n/u)
    .map((name) => name.trim())
    .filter((name) => name !== '');
}

/**
 * Loads the per-platform records a prior run uploaded next to its artifacts.
 *
 * Returns `null` when no record is present, which makes every platform run.
 */
export function readPriorRecords(directory) {
  const platforms = {};
  for (const platform of PLATFORM_NAMES) {
    const candidates = [
      path.join(directory, platform, `release-artifact-${platform}.json`),
      path.join(directory, `release-artifact-${platform}.json`),
    ];
    const recordPath = candidates.find((candidate) => existsSync(candidate));
    if (!recordPath) {
      continue;
    }
    const record = JSON.parse(readFileSync(recordPath, 'utf8'));
    platforms[platform] = {
      inputHash: record.inputHash,
      artifacts: record.artifacts ?? [],
      version: record.version ?? null,
    };
  }
  if (Object.keys(platforms).length === 0) {
    return null;
  }
  return { label: 'the prior run', version: null, platforms };
}

/** Builds a reference from the input hashes at a base revision. */
export function readBaseReference(repositoryRoot, base) {
  if (!revisionExists(repositoryRoot, base)) {
    return null;
  }
  const hashes = computePlatformHashes(readRevisionFileMap(repositoryRoot, base));
  const version = versionAtRevision(repositoryRoot, base);
  const platforms = {};
  for (const platform of PLATFORM_NAMES) {
    platforms[platform] = {
      inputHash: hashes[platform].inputHash,
      artifacts: [],
      version,
    };
  }
  return {
    label: `release ${base}`,
    version,
    platforms,
  };
}

/** The paths that differ between two revisions, or `null` when unavailable. */
export function listChangedPaths(repositoryRoot, base, head) {
  if (!revisionExists(repositoryRoot, base)) {
    return null;
  }
  // `-z` output is NUL-separated; do not trim it, so a path that begins or
  // ends with a space survives intact.
  const result = spawnSync(
    'git',
    ['diff', '--name-only', '-z', `${base}...${head}`],
    { cwd: repositoryRoot, maxBuffer: GIT_OUTPUT_BUFFER_BYTES },
  );
  if (result.error || result.status !== 0) {
    return null;
  }
  return String(result.stdout)
    .split('\0')
    .filter((entry) => entry !== '');
}

/** True when a repository-relative path belongs to a platform group. */
export function pathBelongsToPlatform(filePath, platform) {
  return PLATFORM_INPUT_GROUPS[platform].paths.some(
    (groupPath) =>
      filePath === groupPath || filePath.startsWith(`${groupPath}/`),
  );
}

/**
 * Decides `run` or `reuse` for every platform.
 *
 * `reuse` requires a reference input hash that equals the current one. When
 * `requireArtifactProof` is set (the release workflow), the reference must
 * also carry the artifact names and byte hashes recorded with it, and the
 * prior run must still hold every companion artifact the platform job
 * uploads. Anything else runs, so an absent, stale or incomplete record can
 * never skip a build.
 */
export function decidePlatforms({
  platformHashes,
  version,
  reference,
  requireArtifactProof = false,
  availableArtifacts = null,
}) {
  const availableArtifactNames = new Set(availableArtifacts ?? []);
  const decisions = {};
  for (const platform of PLATFORM_NAMES) {
    const current = platformHashes[platform];
    let decision = 'run';
    let reason = 'no reference inputs; building';
    let referenceInputHash = null;
    let artifacts = [];

    if (reference) {
      const referenceEntry = reference.platforms[platform];
      if (!referenceEntry) {
        reason = `no recorded input hash for ${platform}; building`;
      } else {
        referenceInputHash = referenceEntry.inputHash;
        artifacts = referenceEntry.artifacts ?? [];
        const referenceVersion =
          referenceEntry.version ?? reference.version ?? null;
        const missingArtifacts = requireArtifactProof
          ? PLATFORM_ARTIFACT_NAMES[platform].filter(
              (name) => !availableArtifactNames.has(name),
            )
          : [];
        if (referenceVersion && referenceVersion !== version) {
          reason = `version ${version} differs from the recorded ${referenceVersion}; building`;
        } else if (referenceInputHash !== current.inputHash) {
          reason = `inputs changed since ${reference.label}; building`;
        } else if (requireArtifactProof && artifacts.length === 0) {
          reason = `inputs match ${reference.label} but no artifact bytes were recorded; building`;
        } else if (missingArtifacts.length > 0) {
          reason = `the prior run is missing ${missingArtifacts.join(', ')}; building`;
        } else {
          decision = 'reuse';
          reason = `inputs identical to ${reference.label}; reusing the recorded artifact`;
        }
      }
    }

    decisions[platform] = {
      inputHash: current.inputHash,
      fileCount: current.fileCount,
      referenceInputHash,
      artifacts,
      decision,
      reason,
    };
  }
  return decisions;
}

/** Counts changed files that touch a platform, including its dependencies. */
function countChangedFilesForPlatform(changedPaths, platform) {
  if (!changedPaths) {
    return null;
  }
  const platforms = [
    platform,
    ...(PLATFORM_INPUT_GROUPS[platform].dependsOn ?? []),
  ];
  return changedPaths.filter((filePath) =>
    platforms.some((candidate) =>
      pathBelongsToPlatform(filePath, candidate),
    ),
  ).length;
}

/** SHA-256 of a file, streamed so large release artifacts stay off the heap. */
export async function hashFile(filePath) {
  const digest = createHash('sha256');
  for await (const chunk of createReadStream(filePath)) {
    digest.update(chunk);
  }
  return digest.digest('hex');
}

/** Builds the per-platform record uploaded beside a freshly built artifact. */
export async function buildArtifactRecord({
  platform,
  version,
  commit,
  inputHash,
  artifactPaths,
}) {
  if (!PLATFORM_NAMES.includes(platform)) {
    throw new Error(`unknown platform: ${platform}`);
  }
  if (artifactPaths.length === 0) {
    throw new Error(`the ${platform} record needs at least one artifact`);
  }
  const artifacts = [];
  for (const artifactPath of artifactPaths) {
    if (!existsSync(artifactPath)) {
      throw new Error(`artifact not found: ${artifactPath}`);
    }
    artifacts.push({
      name: path.basename(artifactPath),
      sha256: await hashFile(artifactPath),
    });
  }
  return {
    schemaVersion: RELEASE_PLAN_SCHEMA_VERSION,
    platform,
    version,
    commit,
    inputHash,
    artifacts,
  };
}

function parseArguments(commandLineArguments) {
  const options = {
    mode: 'plan',
    artifacts: [],
    json: false,
    requireArtifactProof: false,
    help: false,
  };
  const readValue = (index, flag) => {
    const value = commandLineArguments[index];
    if (value === undefined || value.startsWith('--')) {
      throw new Error(`${flag} requires a value`);
    }
    return value;
  };
  for (
    let index = 0;
    index < commandLineArguments.length;
    index += 1
  ) {
    const argument = commandLineArguments[index];
    switch (argument) {
      case '--base':
        options.base = readValue((index += 1), argument);
        break;
      case '--head':
        options.head = readValue((index += 1), argument);
        break;
      case '--version':
        options.version = readValue((index += 1), argument);
        break;
      case '--commit':
        options.commit = readValue((index += 1), argument);
        break;
      case '--prior-inputs':
        options.priorInputs = readValue((index += 1), argument);
        break;
      case '--prior-directory':
        options.priorDirectory = readValue((index += 1), argument);
        break;
      case '--available-artifacts':
        options.availableArtifacts = readValue((index += 1), argument);
        break;
      case '--output':
        options.output = readValue((index += 1), argument);
        break;
      case '--input-hash':
        options.inputHash = readValue((index += 1), argument);
        break;
      case '--record-artifact':
        options.mode = 'record';
        options.platform = readValue((index += 1), argument);
        break;
      case '--artifact':
        options.artifacts.push(readValue((index += 1), argument));
        break;
      case '--json':
        options.json = true;
        break;
      case '--require-artifact-proof':
        options.requireArtifactProof = true;
        break;
      case '--help':
        options.help = true;
        break;
      default:
        throw new Error(`unknown argument: ${argument}`);
    }
  }
  return options;
}

function usage() {
  return [
    'usage: release-plan.mjs [plan options | --record-artifact <platform> ...]',
    '',
    'plan options:',
    '  --base <ref>               compare inputs against a base revision',
    '  --head <ref>               revision to plan (default HEAD)',
    '  --version <version>        release version (default: Cargo.toml)',
    '  --commit <sha>             commit recorded in the plan (default HEAD)',
    '  --prior-inputs <file>      combined prior input record',
    '  --prior-directory <dir>    directory of release-artifact-<platform>.json',
    '  --available-artifacts <f>  prior run artifact names, one per line',
    '  --require-artifact-proof   reuse only when artifact bytes were recorded',
    '  --json                     print the plan as JSON',
    '  --output <file>            also write the plan JSON to a file',
    '',
    'record options:',
    '  --record-artifact <name>   platform to record (android, ios, wasm, rn)',
    '  --artifact <file>          built artifact (repeatable)',
    '  --input-hash <hash>        the plan hash for the platform',
    '  --version <version>        release version',
    '  --commit <sha>             commit the artifact was built from',
    '  --output <file>            where to write the record',
  ].join('\n');
}

function printPlanTable(plan) {
  const reference = plan.reference ?? 'none';
  process.stdout.write(
    `release plan for ${plan.version} (${plan.head}, reference ${reference})\n`,
  );
  process.stdout.write(
    'platform  decision  inputs          changed  reason\n',
  );
  for (const platform of PLATFORM_NAMES) {
    const entry = plan.platforms[platform];
    const changed =
      entry.changedFileCount === null ? '-' : String(entry.changedFileCount);
    process.stdout.write(
      `${platform.padEnd(9)} ${entry.decision.padEnd(9)} ` +
        `${entry.inputHash.slice(0, 15)}…  ${changed.padEnd(7)} ` +
        `${entry.reason}\n`,
    );
  }
}

/**
 * Builds the plan document for a revision.
 *
 * Reference precedence is prior-run records, then a combined prior record,
 * then the base revision. The release workflow passes prior-run records so a
 * skip always carries recorded artifact bytes; the base revision is the
 * network-free form used by the local pre-release check.
 */
export function buildPlan({
  repositoryRoot = REPOSITORY_ROOT,
  head = 'HEAD',
  base = null,
  version,
  commit,
  priorInputs = null,
  priorDirectory = null,
  requireArtifactProof = false,
  availableArtifacts = null,
}) {
  const resolvedVersion = version ?? readWorkspaceVersion(repositoryRoot);
  const resolvedCommit =
    commit ?? runGit(repositoryRoot, ['rev-parse', head]);
  const platformHashes = computePlatformHashes(
    readRevisionFileMap(repositoryRoot, head),
  );

  let reference = null;
  if (priorInputs) {
    reference = readPriorInputs(priorInputs);
  } else if (priorDirectory) {
    reference = readPriorRecords(priorDirectory);
  } else if (base) {
    reference = readBaseReference(repositoryRoot, base);
  }

  const changedPaths = base
    ? listChangedPaths(repositoryRoot, base, head)
    : null;
  const decisions = decidePlatforms({
    platformHashes,
    version: resolvedVersion,
    reference,
    requireArtifactProof,
    availableArtifacts,
  });
  for (const platform of PLATFORM_NAMES) {
    decisions[platform].changedFileCount = countChangedFilesForPlatform(
      changedPaths,
      platform,
    );
  }

  return {
    schemaVersion: RELEASE_PLAN_SCHEMA_VERSION,
    version: resolvedVersion,
    commit: resolvedCommit,
    head,
    base,
    reference: reference?.label ?? null,
    requireArtifactProof,
    platforms: decisions,
  };
}

/** Runs the command line interface. Exported for the tests. */
export async function main(
  commandLineArguments,
  { repositoryRoot = REPOSITORY_ROOT } = {},
) {
  const options = parseArguments(commandLineArguments);
  if (options.help) {
    process.stdout.write(`${usage()}\n`);
    return 0;
  }

  if (options.mode === 'record') {
    const version = options.version ?? readWorkspaceVersion(repositoryRoot);
    const commit =
      options.commit ?? runGit(repositoryRoot, ['rev-parse', 'HEAD']);
    if (!options.inputHash) {
      throw new Error('--input-hash is required with --record-artifact');
    }
    const record = await buildArtifactRecord({
      platform: options.platform,
      version,
      commit,
      inputHash: options.inputHash,
      artifactPaths: options.artifacts,
    });
    const outputPath =
      options.output ?? `release-artifact-${options.platform}.json`;
    writeFileSync(outputPath, `${JSON.stringify(record, null, 2)}\n`);
    process.stdout.write(`wrote ${outputPath}\n`);
    return 0;
  }

  const plan = buildPlan({
    repositoryRoot,
    head: options.head ?? 'HEAD',
    base: options.base ?? null,
    version: options.version,
    commit: options.commit,
    priorInputs: options.priorInputs,
    priorDirectory: options.priorDirectory,
    requireArtifactProof: options.requireArtifactProof,
    availableArtifacts:
      options.availableArtifacts === undefined
        ? null
        : readAvailableArtifacts(options.availableArtifacts),
  });

  if (options.output) {
    writeFileSync(options.output, `${JSON.stringify(plan, null, 2)}\n`);
  }
  if (options.json) {
    process.stdout.write(`${JSON.stringify(plan, null, 2)}\n`);
  } else {
    printPlanTable(plan);
  }
  return 0;
}

const invokedDirectly =
  process.argv[1] !== undefined &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);

if (invokedDirectly) {
  main(process.argv.slice(2)).then(
    (exitCode) => {
      process.exitCode = exitCode;
    },
    (error) => {
      process.stderr.write(`release-plan: ${error.message}\n`);
      process.exitCode = 1;
    },
  );
}
