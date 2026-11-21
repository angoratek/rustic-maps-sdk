# Scripts

Every executable in this directory serves one of three roles:

- **Gate**: run by a workflow, the pre-push hook or another gate, and fails on drift.
- **Pipeline step**: run by another script or a workflow to build, package, publish or assemble a release.
- **Maintainer tool**: run by hand, because it needs a credential, a remote service or a deliberate regeneration.

There are no one-off scripts. When a tool stops serving its role, the change that retires the role deletes the script; a script that is neither classified here nor deleted has no reason to exist. `scripts/scripts-index.test.mjs` fails when a file is missing from this table or a table row names a file that is gone, so a new script forces a decision about its role.

| Script | Role | Run by |
|---|---|---|
| `add-generated-esm-extensions.mjs` | Pipeline step: rewrites the generated TypeScript's relative imports to the `.js` specifiers the browser needs. | `build-wasm2-package.sh` |
| `assemble-release-artifacts.sh` | Pipeline step: collects the per-platform release payloads, their checksums and the notices into one artifact directory. | `release.yml` |
| `build-android.sh` | Pipeline step: builds the four ABIs, generates the Kotlin bindings, stages jniLibs and assembles the one AAR. | `android-platform-build.yml`, `assemble-release-artifacts.sh` |
| `build-docs-site.sh` | Pipeline step: builds the book, rustdoc, demo and walkthrough, runs the pre-publish scrub, and publishes to the pages repository. | `docs-publication.yml`, maintainer (publish) |
| `build-ios-reference-application.sh` | Pipeline step: builds and tests the iOS reference application against the generated package. | `ios-platform-build.yml` |
| `build-ios.sh` | Pipeline step: builds the XCFramework, generates the Swift bindings and verifies the SwiftPM package. | `ios-platform-build.yml`, `assemble-release-artifacts.sh` |
| `build-refapp-core.sh` | Pipeline step: builds the shared reference-application core that the native app shells embed. | `build-android.sh`, `build-ios.sh`, `build-ios-reference-application.sh` |
| `build-rn.sh` | Pipeline step: generates and builds the React Native package and app shell. | `rn-app-shell.yml`, `assemble-release-artifacts.sh` |
| `build-wasm2-package.sh` | Pipeline step: generates the wasm2 bindings and packages them as the browser ES module package. | `docs-publication.yml`, `wasm-e2e.yml`, `package-web-npm.sh` |
| `check-dco-signoff.mjs` | Gate: every contribution commit carries a matching `Signed-off-by` trailer. | `dco.yml` |
| `check-dco-signoff.test.mjs` | Test: the DCO unit suite. | Script tests |
| `check-doc-snippets.mjs` | Gate: every documented snippet compiles against the published surface. | `docs-snippets.yml` |
| `check-docs-references.mjs` | Gate: documentation links and referenced paths resolve. | `docs-snippets.yml` |
| `check-generated-api-reference.mjs` | Gate: the generated API reference pages are current and complete. | `build-docs-site.sh` |
| `check-provider-docs.mjs` | Gate: provider documentation matches the registry. | `build-docs-site.sh` |
| `check-public-comment-scrub.mjs` | Gate: shipped wrapper comments carry no internal vocabulary. | `check_ux_parity.mjs` |
| `check-reference-flows.mjs` | Gate: the reference-application flow checklist stays consistent across the four targets. | Playwright global setup, `platform_parity_gate.mjs` |
| `check-release-workflow.mjs` | Gate: the release workflow's jobs, assets and assertions match the plan. | `release-plan.test.mjs` |
| `check-rn-pod-swift.sh` | Gate: the React Native pod's Swift bridge type-checks directly, without a full app build. | `build-rn.sh` |
| `check-style-layer-coverage.mjs` | Gate: every name- or ref-bearing fixture layer is painted or documented. | `format-and-lint.yml` |
| `check-wasm2-package-freshness.mjs` | Gate: the packaged browser module matches the generated bindings it claims to ship. | `package-web-npm.sh` |
| `check-workspace-version.sh` | Gate: one version line across every release package manifest. | `format-and-lint.yml`, `release.yml` |
| `check_ux_parity.mjs` | Gate: design tokens, error sentences and capability copy are identical on every target. | `format-and-lint.yml` |
| `convert-vector-style-artifacts.mjs` | Maintainer tool: converts the committed legacy style artifacts into version-8 documents, or checks them with `--check`. | Maintainer |
| `generate-api-reference.mjs` | Pipeline step: renders the API reference pages from the UDL and rustdoc. | `build-docs-site.sh` |
| `generate-api-reference.test.mjs` | Test: the API reference generator's unit suite. | Script tests in `docs-publication.yml` (needs the generated declaration mirror) |
| `generate-doc-snippets.mjs` | Pipeline step: regenerates the documented snippet sources. | `docs-snippets.yml` |
| `generate-openstreetmap-us-fixtures.mjs` | Maintainer tool: regenerates the synthetic OpenStreetMap-US vector-tile fixtures. | Maintainer |
| `generate-release-readme.sh` | Pipeline step: writes the release body from the asset list. | `release.yml` |
| `generate-rn-bindings.sh` | Pipeline step: generates the JSI bindings, the TurboModule glue and the curated ordinal layer, or checks them for drift. | `build-rn.sh`, `rn-app-shell.yml`, `rn-binding-drift` |
| `generate-swift-package-manifest.sh` | Pipeline step: emits the published `Package.swift` for the Swift package branch. | `release.yml` |
| `generate-third-party-notices` | Pipeline step: runs the notices generator in isolated Python mode. | `build-docs-site.sh`, maintainer |
| `generate-third-party-notices.py` | Pipeline step: builds the third-party notice table from the locked dependency graph. | `generate-third-party-notices` |
| `generate-wasm-bindings.sh` | Pipeline step: generates the browser bindings from the committed wasm contract, or checks them for drift. | `build-wasm2-package.sh` |
| `git-hooks/pre-commit` | Gate: formats and checks before a commit. | Git hook |
| `git-hooks/pre-push` | Gate: accepts a green gate stamp for the pushed tree or runs the gate. | Git hook |
| `install-git-hooks` | Setup: installs the repository's hooks into `.git/hooks`. | Maintainer, `CONTRIBUTING.md` |
| `live-style-smoke.mjs` | Maintainer tool: fetches one provider's live style, sprite, glyph and tile, then styles it through the opt-in binary. | Maintainer (network, `--enable-network`) |
| `mirror-docs-site-tree.mjs` | Pipeline step: mirrors a staged site onto the pages checkout and preserves the published downloads unless replaced completely. | `build-docs-site.sh` |
| `mirror-docs-site-tree.test.mjs` | Test: the publish mirror's preserve/replace unit suite. | Script tests, `docs-publication.yml` |
| `package-rn-npm.sh` | Pipeline step: packs and verifies the React Native npm tarball. | `release.yml`, `assemble-release-artifacts.sh` |
| `package-web-npm.sh` | Pipeline step: packs and verifies the WebAssembly npm tarball. | `release.yml`, `assemble-release-artifacts.sh` |
| `release-plan.mjs` | Pipeline step: the release plan's jobs, assets, assertions and branch rules in one place. | `release.yml` |
| `release-plan.test.mjs` | Test: the release plan's unit suite. | `release.yml` |
| `restore-release-artifact.sh` | Pipeline step: re-verifies and republishes one release asset. | `release.yml` |
| `run-gate.sh` | Gate: `cargo fmt`, workspace clippy and workspace tests, then stamps the tree. | Pre-push hook, maintainer |
| `scripts-index.test.mjs` | Test: every script is classified in this table and every row names a script. | Script tests |
| `smoke-providers.mjs` | Maintainer tool: one line per provider and served contract against the published or local demo. | Maintainer (network) |
| `sync-ios-pod-view.mjs` | Pipeline step: regenerates the pod's `MapView.swift` from the SwiftPM source, or fails on drift. | Playwright global setup |
| `verify-rn-package.mjs` | Gate: the packed React Native tarball carries the expected members and podspec. | `release.yml`, `assemble-release-artifacts.sh` |

"Script tests" is the `node --test` step over this directory's unit suites: `format-and-lint.yml` runs the suites that need no build artifact, and `docs-publication.yml` runs the API reference generator's after it builds the wasm package.
