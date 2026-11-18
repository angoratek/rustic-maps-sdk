# Claude Code Guide: Rustic Maps SDK

This guide provides essential context for working on the Rustic Maps SDK project.

## North star

**The product.** A native map SDK written in Rust. It draws its own maps on the
GPU, plans routes, searches places, follows a turn-by-turn trip from simulated
location updates, and routes offline from a graph compiled out of
OpenStreetMap. It reaches map platforms through vendor-neutral provider
contracts: MapQuest implements the full platform API set, and the URL-template
provider crates serve any `{z}/{x}/{y}` raster or vector source.

**Four targets, one contract.** Android (Kotlin), iOS (Swift/SwiftUI), React
Native and WebAssembly all sit on the same core behind one **frozen UniFFI UDL
contract** (`crates/rustic-maps-native-core/src/rustic_maps.udl`). Kotlin, Swift and
the React Native Turbo Module are generated from it; the wasm surface is a
hand-maintained TypeScript façade, and `platform/web/tests/contract_sync.mjs`
is what keeps that façade honest. `MapView` is the entry point on all four.

**What "done" means.** A milestone is done when its `> Verify:` line in
`PLAN.md` §1 holds. A round is done when the pre-commit gate below is green on
the exact tree and `PLAN.md` and `CHANGELOG.md` record the round in its final
docs commit. Nothing else counts as finished — "it compiles" does not.

**Rules that are not up for relitigation** (change one only with an explicit
product-owner ruling, recorded in `PRD.md` §6):

- UDL-first UniFFI: one contract for all four targets, wasm through the synced
  façade. The contract is **addition-only by default**; a breaking change is
  allowed before 1.0 only under an explicit owner ruling, and then it ships
  with regenerated bindings, reference apps and docs in the same commit
  (`docs/PublicApiV2.md`; the M17 renames and the `rustic_maps` namespace are
  the standing example).
- Own minimal `wgpu` renderer, raster first, vector styled onto the same
  overlay pipeline — not a Navara or maplibre fork.
- Custom self-describing `RMOG` offline routing graph compiled from
  OpenStreetMap — not Valhalla.
- WebAssembly is a first-class target, not a convenience build.
- Provider genericity at every layer (next section).
- **No provider credential in git or in any distributed build.** Credentials
  are entered at runtime; automation may inject one for the session only. The
  one recorded exception is the Appetize-hosted demo build (owner ruling
  2026-10-30, option b): it may carry a **dedicated, separately issued** key,
  injected on the build machine from a repository secret and read at startup
  through the same registration path as the settings surface. The key is
  never the fixture-recording key and never appears in a GitHub Release, an
  npm/Maven/SwiftPM package, a docs download, the repository, a fixture or a
  screenshot (`docs/ProviderCredentialsAndBrowserProviders.md` §6.1). Every
  other build stays keyless.
- Tests never touch the network: imagery and provider payloads are committed
  fixtures, served same-origin by `platform/web/serve.mjs`.
- The published package identities are fixed
  (`com.angoratek:rustic-maps-sdk`; SwiftPM `RusticMapsSDK` and
  `MapNativeCoreUI`; npm `@angoratek/rustic-maps-sdk` and
  `@angoratek/rustic-maps-sdk-web`; the generated `rustic_maps` namespace), and
  the Appetize build ids never change because uploads update in place.

**The three things that most often go wrong here:**

1. **A provider name or URL shape leaks outside its provider crate and the
   registry.** The guard in `platform/web/tests/provider_name_guard.mjs`
   (Playwright `globalSetup` and `node --test`) scans the web harness, the wasm
   sources and all three native shell roots against the registry's own keys.
   Ask of every change: would this work unchanged for a second provider?
2. **A credential reaches a build, a fixture, a log, a doc or a screenshot.**
   There must be no build field, plist key, `local.properties` line or
   `.xcconfig` entry carrying one; redaction is grep-gated where practical
   (`docs/ProviderCredentialsAndBrowserProviders.md` §6).
3. **A test reaches the network.** A new route, image or payload a test needs
   becomes a committed fixture served under `/providers/{provider_key}/…`. The
   one harness that measures GPU frames (`platform/web/perf/`) runs outside
   `npm test` on purpose, because the suite renders under SwiftShader.

### Documentation and public language

Published documentation and public API comments are product surfaces, held to
`docs/DocsStyleGuide.md` (the Stripe/HERE bar):

- **Outcome first.** Every page says what the reader gets, what they need, the
  numbered steps, the expected result, and the next link. No narrative about
  the project's process.
- **No insider vocabulary.** Published pages and public documentation comments
  never mention milestones, rounds, plan numbers, owner/team names, audits,
  internal paths or repository history. `docs/DocsStyleGuide.md` §8 lists the
  banned words.
- **No parenthetical asides.** Parentheses carry data or a short
  clarification, never hosting, path or implementation trivia.
- **One scenario, one order.** Platform tabs, matrices, download tables and
  screenshots are ordered Rust, Android, iOS, React Native, WebAssembly (Rust
  once its libraries ship) and show the same task with the same values.
- **Screenshots are a set.** Recapture every platform from one revision in the
  same four frames — the plan sheet with both endpoints, the provider section
  with every row's name and credential state, simulated navigation at the
  trip's first instruction with 1× playback speed, and the styled vector base
  with its place labels — at phone width, and update none of them alone. Each
  run injects one provider credential for the session only; the published set
  and the capture log never carry it. `docs/ScreenshotCapture.md` holds the
  per-platform procedures and the revision record, and
  `docs/DocsStyleGuide.md` §5 is the checklist.
- **Comments describe the item.** Public documentation comments cover the
  problem, parameters, returns and failure modes. The milestone/round history
  belongs in `PLAN.md`/`CHANGELOG.md`, never in code comments that generate
  documentation.
- **Snippets compile.** `scripts/check-doc-snippets.mjs` is a gate for every
  chapter change; the site build (`scripts/build-docs-site.sh`) is a gate for
  every site change.

### Provider genericity

The SDK must be provider-agnostic at **every** layer, not only in the Rust core:

- **Core**: vendor-neutral traits and registry (`geospatial-traits`, `rustic-maps-core`);
  no provider crate is wired directly into the core.
- **Provider crates**: every vendor URL, credential, status mapping, attribution
  and terms lives here.
- **One registry**: `platform/providers.json` describes each provider (key, display
  name, credential label, per-platform availability, attribution, terms, fixture
  routes, wasm registration, raster/vector templates, style families) and is
  consumed by the refapps, the test harness and the dev server. Never hardcode a
  provider name or URL shape in a harness, test, dev server or refapp shell.
- **Adding a provider** (HERE, Google, TomTom, Mapbox, Radar, any URL-template
  source) is a provider crate + a registry descriptor + a registration dispatch
  arm; it must never require rewriting the live demo, the fixture server, the UI
  suites or a refapp.
- **Tests and fixtures** are keyed by provider (`/providers/{provider_key}/…`), not
  by one vendor's path; attribution and labels come from the descriptors.
- **The question to ask on every change**: *would this work unchanged for a second
  provider?* If not, move the vendor-specific piece into the provider crate or the
  descriptor. The audit and migration plan is `docs/ProviderGenericity.md` (M15).

**Where to look.**

| Question | File |
|---|---|
| Status, current round, open owner questions | `PLAN.md` (§1 milestones, §4 progress log, §5 questions) |
| Why the product is shaped this way; what is locked | `PRD.md` (§6 decisions) |
| What a consumer of the SDK sees | `README.md` |
| The exact FFI surface | `crates/rustic-maps-native-core/src/rustic_maps.udl` |
| The v2 API naming and `MapView` shape | `docs/PublicApiV2.md` |
| How the reference apps must look and behave | `docs/ReferenceAppUX.md` |
| Which providers exist and what they declare | `platform/providers.json` |
| Design tokens and the unified error sentences | `platform/reference-tokens.json` |
| Demo scenes, places, plans and recorded routes | `platform/reference-scenes.json` |
| How credentials work and why | `docs/ProviderCredentialsAndBrowserProviders.md` |
| How a release is produced | `docs/PackagingAndRelease.md` |
| The provider-genericity audit and migration | `docs/ProviderGenericity.md` |

Prior research lives in `tmp/map-sdk-rs-research/` (superseded drafts; the
handwritten JNI bridge there is deliberately discarded — UniFFI generates it).

## Handoff Essentials (verified facts — do not rediscover)

### Session workflow (how rounds run)
- Rounds implement several parallel slices; every commit must compile standalone (docs commits last in a round; merges of planned slices are fine when a split would break per-commit compilation — say so transparently, as round 8 did).
- **Pre-commit verification gate** (run on the exact tree before asking the owner to commit): `cargo fmt --check`; `cargo clippy --workspace --all-targets -- -D warnings`; wasm32 clippy lib-only over the wasm closure (`rustic-maps-rendering`, `rustic-maps-web-bindings`, `rustic-maps-core`, `geospatial-types`, `geospatial-traits`, `rustic-maps-navigation`, `rustic-maps-positioning`, `rustic-maps-vector-tiles`, `rustic-maps-style`, `rustic-maps-glyphs`, `rustic-maps-offline-graph-archive`, `everymap-core`, `everymap-providers-mapquest`, `everymap-providers-photon`, `everymap-providers-here`, `everymap-providers-google`, `everymap-providers-tomtom`, `everymap-providers-mapbox`, `everymap-providers-radar`, `everymap-providers-osrm`, `everymap-providers-valhalla`, `everymap-providers-url-template-vector`, `everymap-providers-url-template-raster`, `rustic-maps-native-core`) — workspace-wide wasm lib clippy still trips on the native-only crates (rustic-maps-cache's bundled SQLite and the offline-graph compiler's mmap reader), so the closure is the honest target set; `rustic-maps-native-core` builds its contract surface for wasm32 (single-threaded UniFFI runtime path, native surface view, JNI and disk cache gated); `cargo test --workspace` (776 tests at round 40 — the run prints the current count; never copy a count forward from here); the Playwright suite in `platform/web` (restart the `serve.mjs` dev server first — new fixtures/routes need a fresh server; the run prints its current count); the cross-platform checks that always ride along: `node --test platform/web/tests/contract_sync.mjs`, the provider-name guard and the platform-parity floor gate (`platform/web/tests/platform_parity_gate.mjs`; all three also run in Playwright's `globalSetup`) and `node scripts/check_ux_parity.mjs`; `node scripts/check-doc-snippets.mjs` when a chapter or a reference app changed, and `scripts/build-docs-site.sh` when a site source changed; platform builds `scripts/build-{android,ios,rn}.sh` whenever their seams changed.
- Update `PLAN.md` (progress log with dates) and `CHANGELOG.md` in the round's final docs commit. Trim PLAN.md when it grows past ~5 completed milestones (keep the dated summary).

### UniFFI (pinned `uniffi 0.31.2`) — recorded verdicts
- Per-member `[Async, Throws=SdkError, Name=...]` only; interface-level `[Async]` is rejected by the UDL parser.
- `[Trait, WithForeign]`: the UDL placeholder traits are discarded by the macro path — the binding crate must define the real Rust traits itself (matching signatures, `Send + Sync` supertraits, `#[doc(hidden)] uniffi_foreign_handle` default); `async-trait` must be a direct binding-crate dependency.
- Orphan rule: FFI records are crate-local mirrors with explicit converters — E0210 forbids implementing FFI traits for foreign-crate types.
- The contract is **addition-only by default**. A breaking change before 1.0 needs an explicit owner ruling and ships with regenerated bindings, reference apps and docs in the same commit (M17 is the standing example: type and member renames, then the `rustic_maps` namespace).
- **Generated bindings are gitignored build artifacts.** After merging a UDL change from a worktree, run `scripts/build-android.sh` and `scripts/build-ios.sh` (and `scripts/build-rn.sh` when its surface matters) on the merged tree before believing a platform build: the worker's generated Kotlin/Swift stayed in its worktree, and the first merged Android build failed on a member the UDL already carried (round 59). The same applies after pulling any commit that changes the UDL. `platform/web/tests/contract_sync.mjs` asserts the wasm TS mirror against the UDL and is addition-tolerant by design.
- ubrn quirks: it renders module identifiers verbatim from the namespace (hyphens produce invalid TS — pass `rustic_maps`), and it resolves the crate via `cargo metadata` walking up from CWD. On the 0.31.2 alignment the generated TypeScript's 108 checksum items agree with the Rust scaffolding (on 0.32.2, 37 of 108 disagreed, which had forced guarded deep imports); the aligned line retires that workaround.
- Exported `[Async]` methods must hand reactor-dependent work (reqwest, `tokio::time`) through the crate-owned runtime in `rustic-maps-native-core/src/runtime.rs` (`drive()` handback) — foreign event loops panic "there is no reactor running" otherwise. Keep locks short and never across an await (every exported future stays `Send`).

### Rendering / GPU facts (verified on-device and in-browser)
- wgpu 30. Device requests use `Limits::downlevel_defaults()` — iOS simulators report `max_inter_stage_shader_variables = 15` and the default's 16 fails every request.
- Surface alpha mode is preferred-then-negotiated; Android emulator ANGLE surfaces only report `[Inherit]`.
- The Android emulator renders on SwiftShader/ANGLE, so both Android shells set `WGPU_BACKEND=gl` before starting the SDK (round-9 ruling: kept, because it is redundant but harmless on hardware).
- Chromium-channel browsers support WebGPU flag-free; `drawImage` on a WebGPU canvas only reads a freshly presented frame, so visual tests force a re-present first.
- Tests never touch the network: imagery comes from the committed tile fixtures in `crates/rustic-maps-rendering/fixtures/render/tiles/`, which `platform/web` serves same-origin. Fixture regeneration seams are guarded by always-on byte-equality tests.

### Reference applications & demo data
- `docs/ReferenceAppUX.md` is the design-language source of truth (§2 token tables, §4 components, §6 error/readiness, §7 TBT cards, §8 per-platform token consumption). The theme is a three-way Light / Dark / **Auto** choice with **Auto the default**, following the OS preference live and remembering an explicit choice on the device (PRD §6 item 9, which overturns the earlier explicit-owned dark default).
- The shared artifacts are consumed by name, never copied as constants: `platform/reference-scenes.json` (scenes, demo places, demo plans, recorded routes), `platform/providers.json` (the provider registry), `platform/reference-tokens.json` (design tokens and the unified error sentences). Each is mirrored where a build needs it (web modules, SwiftPM `.copy`, Android Gradle `Copy`, Metro) and `scripts/check_ux_parity.mjs` fails on token or error-text drift.
- API keys never enter git or a refapp build: `.env` only for the live-smoke example, the fixture-recording tools and the demo-upload script; refapps take provider credentials at runtime through the settings surface (M13, `docs/ProviderCredentialsAndBrowserProviders.md`), with no `local.properties`/Info.plist/xcconfig key seam. UI automation may inject one at runtime (Android/RN launch-intent extra `providerApiKey`, iOS/RN `PROVIDER_API_KEY` launch environment, optionally paired with `providerKey`/`PROVIDER_KEY`), never persisted by the injection; CI stays keyless unless the UI-test job is given the variable. The Appetize-hosted builds are the one recorded exception (owner ruling 2026-10-30, option b): `scripts/inject-hosted-demo-credential.sh` writes a dedicated `HOSTED_DEMO_PROVIDER_API_KEY` into the build on the build machine, the Android asset and the iOS Info.plist entry are gitignored, and the credential is read at startup through the same registration path as a runtime entry — `docs/ProviderCredentialsAndBrowserProviders.md` §6.1 bounds it.
- Standing test devices: Android Pixel_9 emulator, iOS iPhone 17 Pro simulator; evidence screenshots under `tmp/refapp-*` (gitignored; keep the failure series honestly).
- `simctl` has no touch injection, so launch-argument demo automation drives the same user-path methods — it is also the UI-test seam.

## Git Rules

- **Never commit without explicit user approval.** Always ask before committing.
- Do not push to remote unless explicitly asked.
- When allowed, create 1-liner commit messages, precise yet concise, e.g. "feat: move validate-cli.sh to scripts/ (374 assertions, timing, CI job), fix clippy redundant_field_names, optimize api_key clone."
- Never use the Co-author tag in commit messages.
- **DCO exception for contributors (owner ruling 2026-11-17):** a contribution
  submitted through a pull request carries a `Signed-off-by:` trailer, because
  the project requires the Developer Certificate of Origin. CI checks it with
  `scripts/check-dco-signoff.mjs`. Commits made directly on `main` by the
  maintainer keep the one-line style without trailers; never add any other
  trailer.

### Commit Style

- **1-liner only** — precise, concise, no body paragraphs
- **Never** add `Co-Authored-By`, `Signed-off-by`, or any trailer/footer
- Format: `<type>: <imperative description>`
- Types: `feat:`, `fix:`, `refactor:`, `test:`, `docs:`, `ci:`, `chore:`
- Examples:
  - `feat: add token-bucket rate limiter to ig-ratelimit`
  - `fix: map Graph API error code 2534022 to WindowExpired`
  - `test: add wiremock tests for all API error code mappings`

### Parallel workstreams (worktrees, feature branches, agent fan-out)

- **Prefer parallelism for independent slices.** When a round has genuinely independent tracks (per-platform native code, documentation, audits, packaging), give each its own git worktree and branch instead of editing the shared tree in place. Use `Agent` with `isolation: "worktree"` or `git worktree add`; name branches `feat/<slug>`, `docs/<slug>`, `ci/<slug>`.
- **One tree per worker.** Never run `git stash`, `git checkout`, or history rewrites in the shared tree — the round-15 incident where an agent's stash swept parallel in-flight edits is the cautionary case. A worktree makes that class of collision impossible.
- **Agree the seam before fan-out.** State the exact cross-slice contract in every brief — method names, event field names, record shapes, and which files each worker owns. Two agents must not edit the same file; if tracks would overlap, sequence them instead.
- **Small, justified fans only.** Delegate genuinely independent work that would otherwise be read across many files (docs sweeps, audits, per-platform native parity). Do the spine yourself when it is a handful of tool calls or when the slices share too much state to split.
- **Integrate deliberately.** Review each worktree's diff, verify its slice's own gates, then merge or cherry-pick onto `main`. The main session owns the round's final full pre-commit gate on the merged tree.
- **Clean up as you go.** Once a worktree's slice is reviewed and integrated, remove it (`git worktree remove --force <path>`, then `git worktree prune`) and delete its branch — the commits live on `main`. Keep `.claude/worktrees/` empty between rounds; it is gitignored and never committed.
- **Branches stay local by default.** The standing push rule still applies: push a feature branch or open a (draft) PR only when the owner asks for it. Never merge a PR without owner approval.

## Behavioral Guidelines

Behavioral guidelines to reduce common LLM coding mistakes. Merge with project-specific instructions as needed.

Tradeoff: These guidelines bias toward caution over speed. For trivial tasks, use judgment.

1. Think Before Coding

Don't assume. Don't hide confusion. Surface tradeoffs.

Before implementing:

    State your assumptions explicitly. If uncertain, ask.
    If multiple interpretations exist, present them - don't pick silently.
    If a simpler approach exists, say so. Push back when warranted.
    If something is unclear, stop. Name what's confusing. Ask.

2. Simplicity First

Minimum code that solves the problem. Nothing speculative.

    No features beyond what was asked.
    No abstractions for single-use code.
    No "flexibility" or "configurability" that wasn't requested.
    No error handling for impossible scenarios.
    If you write 200 lines and it could be 50, rewrite it.

Ask yourself: "Would a senior engineer say this is overcomplicated?" If yes, simplify.

3. Surgical Changes

Touch only what you must. Clean up only your own mess.

When editing existing code:

    Don't "improve" adjacent code, comments, or formatting.
    Don't refactor things that aren't broken.
    Match existing style, even if you'd do it differently.
    If you notice unrelated dead code, mention it - don't delete it.

When your changes create orphans:

    Remove imports/variables/functions that YOUR changes made unused.
    Don't remove pre-existing dead code unless asked.

The test: Every changed line should trace directly to the user's request.

4. Goal-Driven Execution

Define success criteria. Loop until verified.

Transform tasks into verifiable goals:

    "Add validation" → "Write tests for invalid inputs, then make them pass"
    "Fix the bug" → "Write a test that reproduces it, then make it pass"
    "Refactor X" → "Ensure tests pass before and after"

For multi-step tasks, state a brief plan:

1. [Step] → verify: [check]
2. [Step] → verify: [check]
3. [Step] → verify: [check]

Document such plans and tasks in PLAN.md and update the progress as relevant. Make sure to keep the file in order and use dates to keep giving a sense of things.

**PLAN.md structure and maintenance (keep it in this shape):**

- §0 **Founding gate** — locked founding decisions/approvals as `[x]` checkboxes. Rarely edited; new owner rulings on fundamentals go to `PRD.md` §6 instead.
- §1 **Milestones** — the execution-order line at the top is authoritative (owner-approved; numbering retained even where order differs). **Completed milestones** are `[x]` bullet one-liners (name, completion date, commit, one-line scope; an unfinished residual moves to §2 as an open checkbox). **Active milestones** each carry a heading line ending in `· *status: …*`, a `> Verify:` line with the success criteria, then checkbox work items in landing order. New work = a new checkbox under its milestone or a new milestone block.
- §2 **Backlog & cross-cutting** — `[ ]` checkboxes; land the residual of a closed milestone here so it stays visible.
- §3 **Risks & mitigations** — a table; add a mitigation item whenever the owner records a ruling (with the ruling's round date).
- §4 **Progress log** — dated entries, chronological (append at end). The current round's entry is detailed (slices, commits, verdicts); compress older rounds to one dated line each. When the log grows past a screenful, prune the oldest detail — the full record lives in `CHANGELOG.md` round sections and git history.
- §5 **Queued owner questions** — every question awaiting an owner ruling, grouped by round, numbered, no unilateral decisions. When one is answered, move the ruling into the relevant checklist item (or PRD §6) and delete it here.
- Prune PLAN.md when it grows past ~5 completed milestones or the log overflows: keep dated one-liner summaries, never lose a recorded verdict or unresolved question.

In addition, regularly populate a CHANGELOG.md to help track meaningful features and changes.

Strong success criteria let you loop independently. Weak criteria ("make it work") require constant clarification.

These guidelines are working if: fewer unnecessary changes in diffs, fewer rewrites due to overcomplication, and clarifying questions come before implementation rather than after mistakes.

## Coding Conventions

1. Use the best known and shared practices for each coding language.
2. Follow the SOLID principles: set of five software design principles (Single Responsibility, Open–Closed, Liskov Substitution, Interface Segregation, and Dependency Inversion) meant to make code easier to understand, flexible, and maintainable. They were introduced by Robert C. Martin and are widely used in object-oriented programming to improve testing, stability, and reuse.
3. Do not use any abbreviation in code and choose concise yet precise names for variables, methods, classes, packages.
4. Generate meaningful code documentation that then be used to create docs in an automated way.
5. **No magic numbers**: every static value used in logic lives in a named, documented constant (see `rustic-maps-rendering`'s `camera.rs` for the pattern: geometric tolerances, clip-plane ratios, boundary epsilons, and scaling factors are all named constants). Literal numbers are acceptable only for structural math (`0.5` halving, NDC `[0,1]` conversions) and inside tests when local and self-explanatory; anything reused, physically meaningful, or tunable gets a full-name constant.