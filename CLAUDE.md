# Claude Code Guide: Rustic Maps SDK

## North star

**The product.** A native map SDK written in Rust. It draws maps on the GPU, plans routes, searches places, follows a turn-by-turn trip from simulated location updates, and routes offline from a graph compiled out of OpenStreetMap. It reaches map platforms through vendor-neutral provider contracts: MapQuest implements the full platform API set, and the URL-template provider crates serve any `{z}/{x}/{y}` raster or vector source.

**Four targets, one contract.** Android (Kotlin), iOS (Swift/SwiftUI), React Native and WebAssembly sit on one core behind one frozen UniFFI UDL contract (`crates/rustic-maps-native-core/src/rustic_maps.udl`). Kotlin, Swift and the React Native Turbo Module are generated from it; the browser surface is generated from the same contract by the `wasm2` bindgen, with a hand-written canvas adapter as the only maintained piece. `MapView` is the entry point on all four.

**What done means.** A milestone is done when its done-when line in the GitHub milestone description holds. A slice is done when the pre-commit gate below is green on the exact tree and `CHANGELOG.md` records it in its final docs commit. Nothing else counts as finished.

**Rules that are not up for relitigation** (change one only with an explicit product-owner ruling, recorded in `PRD.md` §6):

- UDL-first UniFFI: one contract for all four targets. The contract is **addition-only by default**; a breaking change before 1.0 needs an explicit owner ruling and ships with regenerated bindings, reference apps and docs in the same commit.
- Own minimal `wgpu` renderer, raster first, vector styled onto the same overlay pipeline — not a Navara or maplibre fork.
- Custom self-describing `RMOG` offline routing graph compiled from OpenStreetMap — not Valhalla.
- WebAssembly is a first-class target, not a convenience build.
- Provider genericity at every layer (next section).
- **No provider credential in git or in any distributed build.** Credentials are entered at runtime; automation may inject one for the session only. There is no hosted-build exception: the former exception was retired on 2026-11-20 (`PRD.md` §6 item 79), so no build, hosted or distributed, carries a provider credential, and `CHANGELOG.md` records the change.
- Tests never touch the network: imagery and provider payloads are committed fixtures, served same-origin by `platform/web/serve.mjs`.
- The published package identities are fixed (`com.angoratek:rustic-maps-sdk`; SwiftPM `RusticMapsSDK` and `MapNativeCoreUI`; npm `@angoratek/rustic-maps-sdk` and `@angoratek/rustic-maps-sdk-web`; the generated `rustic_maps` namespace).

**The three things that most often go wrong here:**

1. **A provider name or URL shape leaks outside its provider crate and the registry.** `platform/web/tests/provider_name_guard.mjs` scans the web harness, the wasm sources, the Android wrapper and all three native shell roots against the registry's own keys. Ask of every change: would this work unchanged for a second provider?
2. **A credential reaches a build, a fixture, a log, a doc or a screenshot.** No build field, plist key, `local.properties` line or `.xcconfig` entry may carry one; redaction is grep-gated where practical (`docs/Providers.md`, Credentials).
3. **A test reaches the network.** A new route, image or payload a test needs becomes a committed fixture served under `/providers/{provider_key}/…`. The one harness that measures GPU frames (`platform/web/perf/`) runs outside `npm test` because the suite renders under SwiftShader.

## Documentation and public language

Published documentation and public API comments are product surfaces, held to `docs/DocsStyleGuide.md` (the Stripe/HERE bar):

- **Outcome first.** Every page says what the reader gets, what they need, the numbered steps, the expected result, and the next link. No narrative about the project's process.
- **No insider vocabulary.** Published pages and public documentation comments never mention milestones, rounds, plan numbers, owner/team names, audits, internal paths or repository history. `docs/DocsStyleGuide.md` §8 lists the banned words.
- **No parenthetical asides.** Parentheses carry data or a short clarification, never hosting, path or implementation trivia.
- **One scenario, one order.** Platform tabs, matrices, download tables and screenshots are ordered Rust, Android, iOS, React Native, WebAssembly and show the same task with the same values.
- **Screenshots are a set.** Recapture every platform from one revision in the four frames in `docs/DocsStyleGuide.md` §5, at phone width, and update none alone. Each run injects one provider credential for the session only; the published set and the capture log never carry it. `docs/Release.md` holds the procedures and revision record.
- **Comments describe the item.** Public documentation comments cover the problem, parameters, returns and failure modes; history belongs in `CHANGELOG.md`, never in code comments that generate documentation.
- **Snippets compile.** `scripts/check-doc-snippets.mjs` gates every chapter change; `scripts/build-docs-site.sh` gates every site change.

## Provider genericity

The SDK must be provider-agnostic at **every** layer, not only in the Rust core:

- **Core**: vendor-neutral traits and registry (`geospatial-traits`, `rustic-maps-core`); no provider crate is wired directly into the core.
- **Provider crates**: every vendor URL, credential, status mapping, attribution and terms lives here.
- **One registry**: `platform/providers.json` describes each provider (key, display name, credential label, per-platform availability, attribution, terms, fixture routes, wasm registration, raster/vector templates, style families) and is consumed by the refapps, the test harness and the dev server. Never hardcode a provider name or URL shape in a harness, test, dev server or refapp shell.
- **Adding a provider** is a provider crate + a registry descriptor + a registration dispatch arm; it never requires rewriting the live demo, the fixture server, the UI suites or a refapp.
- **Tests and fixtures** are keyed by provider (`/providers/{provider_key}/…`), not by one vendor's path; attribution and labels come from the descriptors.
- **The standing question**: would this work unchanged for a second provider? If not, move the vendor-specific piece into the provider crate or the descriptor. `platform/web/tests/provider_name_guard.mjs` is the gate.

## Where to look

| Question | File |
|---|---|
| Open work, milestones and their done-when lines | the GitHub issue tracker |
| Why the product is shaped this way; what is locked | `PRD.md` (§6 decisions) |
| What a consumer of the SDK sees | `README.md` |
| The exact FFI surface | `crates/rustic-maps-native-core/src/rustic_maps.udl` |
| The v2 API naming and `MapView` shape | `docs/Architecture.md` |
| How the reference apps must look and behave | `docs/ReferenceAppUX.md` |
| Which providers exist and what they declare | `platform/providers.json` |
| Design tokens and the unified error sentences | `platform/reference-tokens.json` |
| Demo scenes, places, plans and recorded routes | `platform/reference-scenes.json` |
| How credentials work and why | `docs/Providers.md` |
| How a release is produced | `docs/Release.md` |

## Pre-commit gate and slice workflow

Every commit must compile standalone; docs commits come last in a slice. Run on the exact tree before asking the owner to commit:

- `scripts/run-gate.sh` runs the first three items below and stamps the tree it checked; the pre-push hook accepts that stamp, so the gate never runs inside a push.
- `cargo fmt --check`.
- `cargo clippy --workspace --all-targets -- -D warnings`.
- wasm32 lib-only clippy over the closure enforced in `.github/workflows/wasm-e2e.yml` (the wasm-reachable workspace crates plus the everymap provider crates; native-only crates stay out).
- `cargo test --workspace`; the run prints the current count.
- The Playwright suite in `platform/web` (restart `serve.mjs` first; new fixtures and routes need a fresh server).
- `node platform/web/check-facade-types.mjs` after the wasm package build; it typechecks the browser facade against the generated declarations.
- `node --test platform/web/tests/contract_sync.mjs`, `node --test platform/web/tests/canvas_declaration_sync.mjs`, the provider-name guard, `platform/web/tests/platform_parity_gate.mjs` and `node scripts/check_ux_parity.mjs`; the first four run in Playwright's `globalSetup`, as does the facade typecheck.
- `node --test platform/web/tests/fixture_provenance.mjs` when a fixture or the `everymap-*` pin changes: it hashes every committed fixture against `fixtures/provenance.json`, requires an entry for every fixture, and fails on a provider-layer version bump until the provenance is reviewed. It also runs in Playwright's `globalSetup`.
- `node --test scripts/*.test.mjs`; it includes the index that requires every script in `scripts/` to be classified in `scripts/README.md` with its role and who runs it.
- `node scripts/check-doc-snippets.mjs` when a chapter or reference app changed; `scripts/build-docs-site.sh` when a site source changed.
- `scripts/build-{android,ios,rn}.sh` when their seams changed.

Generated bindings are gitignored build artifacts; after any UDL change, regenerate and rebuild each affected platform before trusting a platform build.

## Git and commit rules

- **Never commit without explicit user approval.** Always ask before committing.
- Do not push to remote unless explicitly asked.
- **1-liner only** — precise, concise, no body paragraphs. Format `<type>: <imperative description>`; types `feat:`, `fix:`, `refactor:`, `test:`, `docs:`, `ci:`, `chore:`.
- **A maintainer commit carries no trailer**: never `Co-Authored-By`, `Signed-off-by` or any other. An outside contribution submitted through a pull request carries `Signed-off-by:` for the Developer Certificate of Origin, checked by `scripts/check-dco-signoff.mjs`; commits made directly on `main` by the maintainer stay one-line with no trailers.

## Parallel workstreams

- **Prefer parallelism for independent slices.** Give each independent track its own git worktree and branch (`Agent` with `isolation: "worktree"` or `git worktree add`); name branches `feat/<slug>`, `docs/<slug>`, `ci/<slug>`.
- **One tree per worker.** Never run `git stash`, `git checkout` or history rewrites in the shared tree.
- **Agree the seam before fan-out.** State method names, event field names, record shapes and file ownership in every brief; two agents must not edit the same file.
- **Small, justified fans only.** Delegate work that spans many files; do the spine yourself when it is a handful of tool calls.
- **Integrate deliberately.** Review each diff and verify its gates, then merge or cherry-pick onto `main`; the main session owns the final full gate on the merged tree.
- **Clean up as you go.** Remove an integrated worktree (`git worktree remove --force <path>`, then `git worktree prune`) and delete its branch; keep `.claude/worktrees/` empty between slices.
- **Branches stay local by default.** Push a feature branch or open a draft PR only when the owner asks; never merge a PR without owner approval.

## Behavioral guidelines

1. **Think before coding.** State assumptions, surface tradeoffs, and ask when a requirement is unclear or has multiple interpretations.
2. **Simplicity first.** Minimum code that solves the problem; no speculative features, abstractions, configurability or impossible-case handling.
3. **Surgical changes.** Touch only what the request needs, match existing style, remove only imports or variables your change orphaned, and mention unrelated dead code rather than deleting it.
4. **Goal-driven execution.** Turn each task into a verifiable goal, state a brief plan, and loop until it is verified.

## Work tracking and CHANGELOG.md

- The GitHub issue tracker is the work tracker: every open work item is one issue with a milestone and a label.
- Each milestone description in GitHub carries its done-when line.
- `CHANGELOG.md` records meaningful changes; a slice ends with its entry. Keep dates ISO-8601 and released sections newest-first.

## Coding conventions

1. Use the best known and shared practices for each language.
2. Follow SOLID: single responsibility, open-closed, Liskov substitution, interface segregation and dependency inversion.
3. No abbreviations in code; choose concise yet precise names for variables, methods, classes and packages.
4. Generate meaningful code documentation that automated docs can consume.
5. **No magic numbers**: every static value used in logic lives in a named, documented constant (see `rustic-maps-rendering`'s `camera.rs`). Literals are acceptable only for structural math (`0.5` halving, NDC `[0,1]` conversions) and inside tests when local and self-explanatory.

## Handoff facts (verified; do not rediscover)

**UniFFI (pinned `uniffi 0.31.2`).**

- Per-member `[Async, Throws=SdkError, Name=...]` only; interface-level `[Async]` is rejected by the UDL parser.
- `[Trait, WithForeign]` placeholder traits are discarded by the macro path: the binding crate defines the real Rust traits with matching signatures, `Send + Sync` supertraits and `#[doc(hidden)] uniffi_foreign_handle` default; `async-trait` must be a direct binding-crate dependency.
- Orphan rule: FFI records are crate-local mirrors with explicit converters — E0210 forbids implementing FFI traits for foreign-crate types.
- ubrn renders module identifiers verbatim from the namespace (hyphens produce invalid TypeScript; pass `rustic_maps`) and resolves the crate via `cargo metadata` walking up from the working directory.
- Exported `[Async]` methods hand reactor-dependent work (reqwest, `tokio::time`) through `rustic-maps-native-core/src/runtime.rs` (`drive()` handback); foreign event loops panic "there is no reactor running" otherwise. Keep locks short and never across an await.

**Rendering and GPU.**

- wgpu 30 device requests use `Limits::downlevel_defaults()`: iOS simulators report `max_inter_stage_shader_variables = 15`, and the default's 16 fails every request.
- Surface alpha mode is preferred-then-negotiated; Android emulator ANGLE surfaces only report `[Inherit]`.
- The Android emulator renders on SwiftShader/ANGLE, so both Android shells set `WGPU_BACKEND=gl` before starting the SDK.
- Chromium-channel browsers support WebGPU flag-free. `drawImage` on a WebGPU canvas reads only a freshly presented frame, so visual tests force a re-present first.
- Imagery comes from `crates/rustic-maps-rendering/fixtures/render/tiles/`, served same-origin; regeneration seams have always-on byte-equality tests.

**Reference applications and demo data.**

- `docs/ReferenceAppUX.md` is the design-language source of truth. The theme is a three-way Light / Dark / **Auto** choice with **Auto** the default, following the OS preference live and remembering an explicit choice on the device.
- The shared artifacts are consumed by name, never copied as constants: `platform/reference-scenes.json`, `platform/providers.json`, `platform/reference-tokens.json`. Each is mirrored where a build needs it; `scripts/check_ux_parity.mjs` fails on token or error-text drift.
- Credentials are entered at runtime through the settings surface. UI automation may inject one at launch (`providerApiKey` on Android/RN, `PROVIDER_API_KEY` on iOS/RN), never persisted by the injection and never logged. Every build is keyless; the former hosted-build exception was retired on 2026-11-20 (`PRD.md` §6 item 79).
- Standing test devices: Android Pixel_9 emulator and iOS iPhone 17 Pro simulator; evidence screenshots live under `tmp/refapp-*` (gitignored).
- `simctl` has no touch injection, so launch-argument automation drives the same user-path methods.
