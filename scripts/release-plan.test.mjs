// Tests for scripts/release-plan.mjs and the release workflow's gate shape.
//
// Run with: node --test scripts/release-plan.test.mjs
//
// Everything here is local: fixture file lists for the hash and decision
// logic, a throwaway git repository for the revision comparison, and the
// workflow file itself. When `actionlint` is installed it also runs over the
// workflow; the structural assertions run either way.

import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { test } from 'node:test';

import {
  PLATFORM_ARTIFACT_NAMES,
  PLATFORM_NAMES,
  buildArtifactRecord,
  buildPlan,
  computePlatformHashes,
  decidePlatforms,
  hashFileEntries,
  parseWorkspaceVersion,
  readAvailableArtifacts,
} from './release-plan.mjs';
import {
  checkReleaseWorkflow,
  parseWorkflowYaml,
} from './check-release-workflow.mjs';

/** File entries as `listRevisionFiles` returns them, with fixture blob ids. */
const FIXTURE_FILES = {
  android: [
    { path: 'Cargo.lock', blobId: 'lock-1' },
    { path: 'Cargo.toml', blobId: 'manifest-1' },
    { path: 'crates/core/src/lib.rs', blobId: 'core-1' },
    { path: 'platform/android/MapView.kt', blobId: 'android-1' },
  ],
  ios: [
    { path: 'Cargo.lock', blobId: 'lock-1' },
    { path: 'Cargo.toml', blobId: 'manifest-1' },
    { path: 'crates/core/src/lib.rs', blobId: 'core-1' },
    { path: 'platform/ios/MapView.swift', blobId: 'ios-1' },
  ],
  wasm: [
    { path: 'Cargo.lock', blobId: 'lock-1' },
    { path: 'Cargo.toml', blobId: 'manifest-1' },
    { path: 'crates/core/src/lib.rs', blobId: 'core-1' },
    { path: 'platform/web/bindings.mjs', blobId: 'wasm-1' },
  ],
  rn: [
    { path: 'Cargo.lock', blobId: 'lock-1' },
    { path: 'Cargo.toml', blobId: 'manifest-1' },
    { path: 'crates/core/src/lib.rs', blobId: 'core-1' },
    { path: 'platform/rn/index.tsx', blobId: 'rn-1' },
  ],
};

/** Every workflow artifact name a reusable release must still hold. */
const ALL_REQUIRED_ARTIFACTS = PLATFORM_NAMES.flatMap(
  (platform) => PLATFORM_ARTIFACT_NAMES[platform],
);

/** Copies the fixture lists so a test can mutate one entry. */
function cloneFixtureFiles() {
  return Object.fromEntries(
    PLATFORM_NAMES.map((platform) => [
      platform,
      FIXTURE_FILES[platform].map((entry) => ({ ...entry })),
    ]),
  );
}

/** A scratch directory the test runner removes. */
function makeScratchDirectory(testContext) {
  const directory = mkdtempSync(path.join(os.tmpdir(), 'release-plan-'));
  testContext.after(() => {
    rmSync(directory, { recursive: true, force: true });
  });
  return directory;
}

/** Runs a git command in a scratch repository. */
function git(repositoryDirectory, argumentsList) {
  const result = spawnSync('git', argumentsList, {
    cwd: repositoryDirectory,
    encoding: 'utf8',
    env: {
      ...process.env,
      GIT_AUTHOR_NAME: 'Release Plan Test',
      GIT_AUTHOR_EMAIL: 'release-plan@example.invalid',
      GIT_COMMITTER_NAME: 'Release Plan Test',
      GIT_COMMITTER_EMAIL: 'release-plan@example.invalid',
    },
  });
  assert.equal(
    result.status,
    0,
    `git ${argumentsList.join(' ')} failed: ${result.stderr}`,
  );
  return result.stdout.trim();
}

/** Writes a file and creates its parent directories. */
function writeFile(rootDirectory, relativePath, content) {
  const absolutePath = path.join(rootDirectory, relativePath);
  mkdirSync(path.dirname(absolutePath), { recursive: true });
  writeFileSync(absolutePath, content);
}

/** Creates a git repository with one commit containing the platform inputs. */
function makeFixtureRepository(testContext) {
  const directory = makeScratchDirectory(testContext);
  git(directory, ['init', '--quiet']);
  writeFile(
    directory,
    'Cargo.toml',
    '[workspace]\nmembers = []\n\n[workspace.package]\nversion = "0.1.0"\n',
  );
  writeFile(directory, 'Cargo.lock', 'version = 4\n');
  writeFile(directory, 'crates/core/src/lib.rs', 'pub fn core() {}\n');
  writeFile(directory, 'LICENSE-MIT', 'MIT\n');
  writeFile(directory, 'LICENSE-APACHE', 'Apache\n');
  writeFile(directory, 'docs/THIRD_PARTY_NOTICES.md', 'notices\n');
  writeFile(directory, 'platform/android/MapView.kt', 'class MapView\n');
  writeFile(directory, 'platform/ios/MapView.swift', 'struct MapView {}\n');
  writeFile(directory, 'platform/web/bindings.mjs', 'export const web = 1;\n');
  writeFile(directory, 'platform/rn/index.tsx', 'export const rn = 1;\n');
  git(directory, ['add', '--all']);
  git(directory, ['commit', '--quiet', '--message', 'fixture']);
  return directory;
}

test('the input hash is order-independent and content-sensitive', () => {
  const forward = hashFileEntries([
    { path: 'a', blobId: '1' },
    { path: 'b', blobId: '2' },
  ]);
  const reversed = hashFileEntries([
    { path: 'b', blobId: '2' },
    { path: 'a', blobId: '1' },
  ]);
  const changed = hashFileEntries([
    { path: 'a', blobId: '1' },
    { path: 'b', blobId: '3' },
  ]);

  assert.match(forward, /^sha256:[0-9a-f]{64}$/u);
  assert.equal(forward, reversed);
  assert.notEqual(forward, changed);
});

test('a platform change propagates to React Native but not to other platforms', () => {
  const before = computePlatformHashes(cloneFixtureFiles());
  const afterFiles = cloneFixtureFiles();
  afterFiles.android = afterFiles.android.map((entry) =>
    entry.path === 'platform/android/MapView.kt'
      ? { ...entry, blobId: 'android-2' }
      : entry,
  );
  const after = computePlatformHashes(afterFiles);

  assert.notEqual(before.android.inputHash, after.android.inputHash);
  assert.equal(before.ios.inputHash, after.ios.inputHash);
  assert.equal(before.wasm.inputHash, after.wasm.inputHash);
  assert.notEqual(
    before.rn.inputHash,
    after.rn.inputHash,
    'the React Native hash must mix in the Android payload inputs',
  );
});

test('an iOS change propagates to React Native only', () => {
  const before = computePlatformHashes(cloneFixtureFiles());
  const afterFiles = cloneFixtureFiles();
  afterFiles.ios = afterFiles.ios.map((entry) =>
    entry.path === 'platform/ios/MapView.swift'
      ? { ...entry, blobId: 'ios-2' }
      : entry,
  );
  const after = computePlatformHashes(afterFiles);

  assert.notEqual(before.ios.inputHash, after.ios.inputHash);
  assert.equal(before.android.inputHash, after.android.inputHash);
  assert.equal(before.wasm.inputHash, after.wasm.inputHash);
  assert.notEqual(before.rn.inputHash, after.rn.inputHash);
});

test('no reference inputs means every platform runs', () => {
  const decisions = decidePlatforms({
    platformHashes: computePlatformHashes(FIXTURE_FILES),
    version: '0.1.0',
    reference: null,
  });
  for (const platform of PLATFORM_NAMES) {
    assert.equal(decisions[platform].decision, 'run');
    assert.match(decisions[platform].reason, /no reference inputs/u);
  }
});

test('a matching record with artifact bytes reuses, and requires the proof when asked', () => {
  const hashes = computePlatformHashes(FIXTURE_FILES);
  const reference = {
    label: 'the prior run',
    version: '0.1.0',
    platforms: Object.fromEntries(
      PLATFORM_NAMES.map((platform) => [
        platform,
        {
          inputHash: hashes[platform].inputHash,
          artifacts: [{ name: `${platform}.bin`, sha256: 'a'.repeat(64) }],
          version: '0.1.0',
        },
      ]),
    ),
  };

  const reused = decidePlatforms({
    platformHashes: hashes,
    version: '0.1.0',
    reference,
    requireArtifactProof: true,
    availableArtifacts: ALL_REQUIRED_ARTIFACTS,
  });
  for (const platform of PLATFORM_NAMES) {
    assert.equal(reused[platform].decision, 'reuse');
    assert.deepEqual(reused[platform].artifacts, [
      { name: `${platform}.bin`, sha256: 'a'.repeat(64) },
    ]);
  }

  const withoutArtifacts = {
    ...reference,
    platforms: Object.fromEntries(
      PLATFORM_NAMES.map((platform) => [
        platform,
        { ...reference.platforms[platform], artifacts: [] },
      ]),
    ),
  };
  const proofRequired = decidePlatforms({
    platformHashes: hashes,
    version: '0.1.0',
    reference: withoutArtifacts,
    requireArtifactProof: true,
    availableArtifacts: ALL_REQUIRED_ARTIFACTS,
  });
  for (const platform of PLATFORM_NAMES) {
    assert.equal(proofRequired[platform].decision, 'run');
    assert.match(proofRequired[platform].reason, /no artifact bytes/u);
  }

  const unproven = decidePlatforms({
    platformHashes: hashes,
    version: '0.1.0',
    reference: withoutArtifacts,
    requireArtifactProof: false,
  });
  for (const platform of PLATFORM_NAMES) {
    assert.equal(unproven[platform].decision, 'reuse');
  }
});

test('a changed hash, a missing platform record, or a version bump runs', () => {
  const hashes = computePlatformHashes(FIXTURE_FILES);
  const reference = {
    label: 'the prior run',
    version: '0.1.0',
    platforms: {
      android: {
        inputHash: hashes.android.inputHash,
        artifacts: [{ name: 'android.aar', sha256: 'b'.repeat(64) }],
        version: '0.1.0',
      },
    },
  };

  const decisions = decidePlatforms({
    platformHashes: hashes,
    version: '0.1.0',
    reference,
    requireArtifactProof: true,
    availableArtifacts: ALL_REQUIRED_ARTIFACTS,
  });
  assert.equal(decisions.android.decision, 'reuse');
  for (const platform of ['ios', 'wasm', 'rn']) {
    assert.equal(decisions[platform].decision, 'run');
    assert.match(decisions[platform].reason, /no recorded input hash/u);
  }

  const versionBump = decidePlatforms({
    platformHashes: hashes,
    version: '0.2.0',
    reference,
    requireArtifactProof: true,
    availableArtifacts: ALL_REQUIRED_ARTIFACTS,
  });
  assert.equal(versionBump.android.decision, 'run');
  assert.match(versionBump.android.reason, /differs from the recorded/u);

  const changed = decidePlatforms({
    platformHashes: computePlatformHashes({
      ...cloneFixtureFiles(),
      android: [
        ...FIXTURE_FILES.android,
        { path: 'platform/android/Extra.kt', blobId: 'extra-1' },
      ],
    }),
    version: '0.1.0',
    reference,
    requireArtifactProof: true,
    availableArtifacts: ALL_REQUIRED_ARTIFACTS,
  });
  assert.equal(changed.android.decision, 'run');
  assert.match(changed.android.reason, /inputs changed/u);
});

test('a missing companion artifact forces a rebuild', () => {
  const hashes = computePlatformHashes(FIXTURE_FILES);
  const reference = {
    label: 'the prior run',
    version: '0.1.0',
    platforms: Object.fromEntries(
      PLATFORM_NAMES.map((platform) => [
        platform,
        {
          inputHash: hashes[platform].inputHash,
          artifacts: [{ name: `${platform}.bin`, sha256: 'c'.repeat(64) }],
          version: '0.1.0',
        },
      ]),
    ),
  };
  const decisions = decidePlatforms({
    platformHashes: hashes,
    version: '0.1.0',
    reference,
    requireArtifactProof: true,
    availableArtifacts: ALL_REQUIRED_ARTIFACTS.filter(
      (name) => name !== 'release-ios-framework',
    ),
  });

  assert.equal(decisions.ios.decision, 'run');
  assert.match(decisions.ios.reason, /release-ios-framework/u);
  assert.equal(decisions.android.decision, 'reuse');
  assert.equal(decisions.wasm.decision, 'reuse');
});

test('the workspace version parser reads only [workspace.package]', () => {
  const manifest = [
    '[workspace]',
    'members = ["crates/core"]',
    '',
    '[workspace.package]',
    'version = "1.2.3"',
    '',
    '[workspace.dependencies]',
    'serde = { version = "1" }',
  ].join('\n');
  assert.equal(parseWorkspaceVersion(manifest), '1.2.3');
  assert.throws(
    () => parseWorkspaceVersion('[package]\nname = "x"\n'),
    /no \[workspace\.package\] version/u,
  );
});

test('the revision plan reuses unchanged platforms and rebuilds changed ones', (testContext) => {
  const repositoryDirectory = makeFixtureRepository(testContext);

  const unchanged = buildPlan({
    repositoryRoot: repositoryDirectory,
    base: 'HEAD',
    version: '0.1.0',
    commit: 'fixture',
  });
  for (const platform of PLATFORM_NAMES) {
    assert.equal(
      unchanged.platforms[platform].decision,
      'reuse',
      `${platform} should be unchanged against itself`,
    );
    assert.equal(unchanged.platforms[platform].changedFileCount, 0);
  }

  writeFile(
    repositoryDirectory,
    'platform/android/MapView.kt',
    'class MapView { fun changed() {} }\n',
  );
  git(repositoryDirectory, ['add', '--all']);
  git(repositoryDirectory, ['commit', '--quiet', '--message', 'android']);

  const changed = buildPlan({
    repositoryRoot: repositoryDirectory,
    base: 'HEAD~1',
    version: '0.1.0',
    commit: 'fixture',
  });
  assert.equal(changed.platforms.android.decision, 'run');
  assert.equal(changed.platforms.ios.decision, 'reuse');
  assert.equal(changed.platforms.wasm.decision, 'reuse');
  assert.equal(changed.platforms.rn.decision, 'run');
  assert.ok(changed.platforms.android.changedFileCount >= 1);
  assert.ok(changed.platforms.rn.changedFileCount >= 1);
});

test('prior-run records gate reuse and need the artifact proof', async (testContext) => {
  const repositoryDirectory = makeFixtureRepository(testContext);
  const priorDirectory = path.join(repositoryDirectory, 'prior');
  const plan = buildPlan({
    repositoryRoot: repositoryDirectory,
    base: 'HEAD',
    version: '0.1.0',
    commit: 'fixture',
  });

  for (const platform of PLATFORM_NAMES) {
    const artifactPath = path.join(repositoryDirectory, `${platform}.bin`);
    writeFileSync(artifactPath, `artifact for ${platform}\n`);
    const record = await buildArtifactRecord({
      platform,
      version: '0.1.0',
      commit: 'fixture',
      inputHash: plan.platforms[platform].inputHash,
      artifactPaths: [artifactPath],
    });
    const recordDirectory = path.join(priorDirectory, platform);
    mkdirSync(recordDirectory, { recursive: true });
    writeFileSync(
      path.join(recordDirectory, `release-artifact-${platform}.json`),
      `${JSON.stringify(record, null, 2)}\n`,
    );
  }

  const reused = buildPlan({
    repositoryRoot: repositoryDirectory,
    priorDirectory,
    availableArtifacts: ALL_REQUIRED_ARTIFACTS,
    requireArtifactProof: true,
    version: '0.1.0',
    commit: 'fixture',
  });
  for (const platform of PLATFORM_NAMES) {
    assert.equal(reused.platforms[platform].decision, 'reuse');
    const expectedSha256 = createHash('sha256')
      .update(`artifact for ${platform}\n`)
      .digest('hex');
    assert.deepEqual(reused.platforms[platform].artifacts, [
      { name: `${platform}.bin`, sha256: expectedSha256 },
    ]);
  }

  rmSync(path.join(priorDirectory, 'ios'), { recursive: true });
  const partial = buildPlan({
    repositoryRoot: repositoryDirectory,
    priorDirectory,
    availableArtifacts: ALL_REQUIRED_ARTIFACTS,
    requireArtifactProof: true,
    version: '0.1.0',
    commit: 'fixture',
  });
  assert.equal(partial.platforms.ios.decision, 'run');
  assert.equal(partial.platforms.android.decision, 'reuse');
});

test('the available-artifact list reader ignores blank lines', (testContext) => {
  const directory = makeScratchDirectory(testContext);
  const listPath = path.join(directory, 'artifacts.txt');
  writeFileSync(listPath, 'release-android\n\nrelease-wasm\n');
  assert.deepEqual(readAvailableArtifacts(listPath), [
    'release-android',
    'release-wasm',
  ]);
  assert.deepEqual(readAvailableArtifacts(path.join(directory, 'absent')), []);
});

test('the artifact record names the file and its SHA-256', async (testContext) => {
  const directory = makeScratchDirectory(testContext);
  const artifactPath = path.join(directory, 'rustic-maps-sdk-android.aar');
  writeFileSync(artifactPath, 'archive bytes\n');

  const record = await buildArtifactRecord({
    platform: 'android',
    version: '0.1.0',
    commit: 'abc123',
    inputHash: 'sha256:' + '0'.repeat(64),
    artifactPaths: [artifactPath],
  });

  assert.equal(record.platform, 'android');
  assert.equal(record.version, '0.1.0');
  assert.equal(record.artifacts.length, 1);
  assert.equal(record.artifacts[0].name, 'rustic-maps-sdk-android.aar');
  assert.match(record.artifacts[0].sha256, /^[0-9a-f]{64}$/u);
});

test('the workflow parser reads block scalars, sequences and conditions', () => {
  const workflow = parseWorkflowYaml(
    [
      'name: fixture',
      'jobs:',
      '  build:',
      '    needs: [gates, plan]',
      "    if: ${{ needs.plan.outputs.build == 'run' }}",
      '    steps:',
      '      - name: Build',
      '        run: |',
      '          echo one',
      '          echo two',
      '      - uses: actions/checkout@v4',
      '',
    ].join('\n'),
  );

  assert.deepEqual(workflow.jobs.build.needs, ['gates', 'plan']);
  assert.match(workflow.jobs.build.if, /needs\.plan\.outputs\.build/u);
  assert.match(workflow.jobs.build.steps[0].run, /echo one\necho two/u);
  assert.equal(workflow.jobs.build.steps[1].uses, 'actions/checkout@v4');
});

test('the release workflow keeps the input-gated rebuild structure', () => {
  const { errors, actionlint } = checkReleaseWorkflow();
  assert.deepEqual(errors, []);
  assert.ok(
    actionlint === 'passed' || actionlint === 'not installed',
    `unexpected actionlint state: ${actionlint}`,
  );
});
