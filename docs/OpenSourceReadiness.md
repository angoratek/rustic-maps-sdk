# Open-source readiness

This note is the working plan for opening the project. It records the
decisions, the evidence behind them and the order the work lands in. Nothing
here changes the product surface; it changes what the public can see, how
contributions arrive and how the release pipeline is trusted.

## Decisions

Recorded 2026-11-17:

1. **The public history is a rewritten copy, delivered as a new public
   repository.** The development repository keeps its history; a filter-repo
   mirror is pushed to a newly created public repository, so the public
   surface starts without pre-rewrite objects reachable by SHA, private
   issues, pull requests or Actions logs.
2. **Contributions carry a Developer Certificate of Origin sign-off.** No CLA.
   CI checks the sign-off; the commit style otherwise stays one line with no
   trailers.
3. **The opening covers the repository, the published documentation site and
   the browser and mobile demos.** Platform packages stay build-from-source
   until the first versioned release.
4. **Vulnerability reports arrive through GitHub private vulnerability
   reporting only.** No personal address is published.
5. **Internal planning documents are published in sanitized form.** Process
   vocabulary, billing state, credential operations and absolute local paths
   are removed; decisions and open questions survive in project language.
6. **The browser demo keeps its opt-in remembered key, documented as the one
   browser exception.** The credential rule is amended to name `localStorage`
   behind a visible warning; the show-key toggle is removed so every platform
   masks the field, and everything else follows the secure-store rule.
7. **The Rust crates stay source-only until the first versioned release.**
8. **AI assistance is disclosed, never banned.** `CONTRIBUTING.md` states the
   responsibility rule and the pull-request template carries an explicit
   disclosure checkbox.

## Evidence

Six read-only audits ran on 2026-11-17: repository history and sensitive
data, open-source governance scaffolding, documentation size and validity,
history-rewrite impact, product and supply-chain security, and the public
provider layer repository. Their findings are summarized in `PLAN.md` M82;
the detailed evidence lives in the audit transcripts for this round.

The headline findings that shape the sequence:

- **A live provider key is in history.** Blob `1414be28` carries a real
  MapQuest consumer key in the recorded route fixture, reachable from every
  branch descending from commit `48b4ef54` and still present in `refs/stash`.
  The key is revoked and rotated before anything else, and the rewrite is
  mandatory. The tip tree carries the redacted value.
- The tip tree carried the maintainer's absolute home path in `PLAN.md` and a
  research note; both are now repo-relative.
- The working tree is otherwise clean of credentials; the local `.env` holds
  live values that are rotated before opening and never archived.
- The documentation carries 27.5k Markdown lines, 19 implemented design
  notes, 12 contradictions against the code and a 110-entry commit-SHA pin
  inventory that a rewrite invalidates.
- The community-health surface is bare: no contribution guide, code of
  conduct, security policy, support guide, templates, ownership file, roadmap
  or dependency automation.
- The release pipeline is strong (checksums, keyless Sigstore, draft-only
  publication, no long-lived signing key) and the weak points are bounded:
  mutable action tags, missing explicit permissions, secrets at job scope,
  unbounded response and image sizes, and a browser credential stored in
  `localStorage` contrary to the credential rules.
- The provider layer repository is close to ready but publishes stale version
  claims, underdocumented core surfaces and four yanked crates whose tarballs
  still serve the retired session identifier.

## Phase A — repository health and security

Outcome: the tree is safe to publish and every quality signal a new reader
checks is present.

1. Community health files at the repository root: a dual-licence `LICENSE`
   statement, `CONTRIBUTING.md` with the keyless build path and the DCO rule,
   `CODE_OF_CONDUCT.md` (Contributor Covenant 2.1), `SECURITY.md` with
   supported versions and private reporting, `SUPPORT.md`, `GOVERNANCE.md`,
   `MAINTAINERS.md`, `ROADMAP.md`, and an AI-usage policy.
2. GitHub surface: issue forms, a pull-request template, `CODEOWNERS`,
   Dependabot for every ecosystem, and a labeler.
3. Workflow hardening: every action pinned by commit SHA with a version
   comment, least-privilege `permissions` on every workflow, read-only jobs
   without persisted credentials, secrets scoped to the steps that consume
   them, Gradle wrapper digests, and dependency review, CodeQL, Scorecard and
   actionlint jobs.
4. Security fixes: rotate the local credentials, upgrade the SQLite binding
   off its advisory, resolve the memory-map advisory or record a narrow
   ignore, cap HTTP response and image sizes, stop persisting the browser
   credential beyond the session, remove or justify the show-key affordance,
   scrub the browser console export like the native consoles, and settle the
   demo signing keystore.
5. Repository metadata: description, homepage, topics, documentation URL and
   Discussions.

## Phase B — documentation and process sanitization

Outcome: the public reader sees product documentation, not project process.

1. Retire implemented design notes after moving every decision they record
   into `PRD.md` §6 or the changelog; merge overlapping notes; keep only the
   designs that still guide open work.
2. Fix the contradictions between the documentation and the code, including
   the screenshot rules and the stale release claim on the downloads page.
3. Compress `PLAN.md` and `CHANGELOG.md` to one-liners for completed work
   while preserving every ruling, verdict and open question.
4. Rewrite the public pages and remaining internal notes in project
   vocabulary: no owner, round, milestone, billing or credential-operation
   language, and no absolute paths.
5. Replace every pinned commit reference with a revision-independent record;
   the public provenance is regenerated from the public history.

## Phase C — history rewrite and publication

Outcome: one clean public history that reproduces the tree.

1. Clean the working environment: no worktrees, no stale refs, a decision on
   the stash, and the private origin synchronized.
2. Build a mirror and run `git filter-repo` with a replacement file that
   removes the retired session identifier and any absolute local paths.
3. Verify the mirror on a fresh clone: a secrets scan over the full history
   reports zero hits, `git fsck --full` is clean, and the full project gate
   passes on the exported tree.
4. Push the mirror to the new public repository, re-point the public
   documentation and demo build at the public history, and publish the site
   and demos from it.
5. Re-check the reported demo behaviours on the published origin.

## Phase D — after opening

Rulesets and required checks, private vulnerability reporting, secret
scanning and push protection, Dependabot alerts, Discussions, a seeded
`good first issue` set, the provider layer repository fixes, and the first
versioned release when the product is ready.

## Acceptance

The opening is ready when:

- a fresh clone of the public mirror contains no credential, personal path or
  retired identifier in any reachable object;
- the full project gate passes on that clone;
- every community-health file exists and every link resolves;
- no workflow carries a mutable action pin, an implicit permission or a
  secret at job scope;
- the public documentation builds from the mirror and states the real release
  status;
- the published demo passes the reported-behaviour checks.

## Open questions

1. Which repository name the public mirror takes. A new public repository
   cannot reuse the private repository's name under the same account, so the
   private repository is renamed first or the public one takes a different
   name; `SECURITY.md` and the documentation links follow the ruling.
2. Whether the recorded vendor payloads and imagery may be redistributed.
   The MapQuest and HERE fixtures, the recorded raster tiles and the
   screenshots showing vendor imagery need a terms check, or replacement with
   OpenStreetMap and synthetic fixtures.
3. Whether the provider layer repository's four yanked crates are purged
   through crates.io support or the exposure is recorded as accepted, and
   whether the capture credential is rotated as a consequence.
