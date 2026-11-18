# Contributing

Contribute a defect fix, a provider integration, a platform improvement, or a
documentation correction. This page gives you the shortest path from a clone
to a merged pull request without any provider credential.

## What you need

- **Rust 1.89 or later.** The workspace declares this in `Cargo.toml`.
- **Node.js 22 or later** for the browser suite and the React Native shell.
- **Git** with a GitHub account.
- **Platform toolchains only for the platform you change.** The Android and iOS
  build scripts print their own prerequisites when one is missing.
- **No provider credential.** The test suites are keyless. Credentials are
  entered at runtime in a reference application or supplied through `.env` for
  the optional live recording tools; nothing in the build requires one.

Tests never touch the network. Provider responses and map imagery are committed
recordings served same-origin by `platform/web/serve.mjs`.

## Build and test

The full build and test reference is the [Build & test section of the
README](README.md#build--test). It covers the workspace build, the platform
package scripts, the WebAssembly build, and the browser suite.

Install the repository's git hooks once per clone:

```bash
scripts/install-git-hooks
```

The pre-commit hook checks formatting. The pre-push hook runs formatting,
clippy, and the workspace tests. Bypass either for one command with
`MAPS_SDK_SKIP_HOOKS=1`.

## Pre-commit gate

Run these on the exact tree before you open a pull request:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
node scripts/check-docs-references.mjs
node scripts/check-doc-snippets.mjs
node scripts/check_ux_parity.mjs
node --test platform/web/tests/contract_sync.mjs
```

When your change touches the browser test suite, build the wasm package and
run the Playwright suite as described in the
[WebAssembly and browser end-to-end tests section of the
README](README.md#webassembly-and-browser-end-to-end-tests):

```bash
cd platform/web
npx playwright test
```

The suite's global setup also runs the contract sync check, the provider-name
guard, and the platform-parity floor gate.

## Branches and commits

- Branch from `main` and name the branch `feat/<slug>`, `fix/<slug>`,
  `docs/<slug>`, or `ci/<slug>`.
- Every commit compiles on its own.
- Write one-line commit messages in the form `<type>: <description>`, with an
  imperative description and no body:

  ```text
  feat: add token-bucket rate limiter to the provider client
  fix: map provider error code 2534022 to WindowExpired
  ```

- Use one of `feat`, `fix`, `refactor`, `test`, `docs`, `ci`, or `chore` as the
  type.
- Do not add `Co-Authored-By`, `Signed-off-by` from a tool, or any other
  trailer. The only trailer is your own DCO sign-off, described next.

## Developer Certificate of Origin

Every commit must be signed off under the [Developer Certificate of
Origin](https://developercertificate.org/). There is no contributor license
agreement.

Create the sign-off automatically:

```bash
git commit -s -m "fix: correct the tile cache eviction order"
```

The commit then ends with a line in this form:

```text
Signed-off-by: Your Name <you@example.com>
```

That line certifies that you wrote the contribution or have the right to submit
it under the project's license. Use your real name and an address you control.

To sign off a commit after the fact, run `git commit --amend -s --no-edit`. To
sign off a series of commits, run `git rebase --signoff main`.

## Pull requests

A pull request is ready for review when it contains:

1. **A linked issue**, when one exists. Use `Closes #123` in the description.
2. **Tests** that fail before the change and pass after it.
3. **A `CHANGELOG.md` entry** for any user-visible change.
4. **Documentation updates** for any public API, guide, or reference page the
   change affects.
5. **The green pre-commit gate** above, quoted with the exact commands you ran.
6. **No credentials, tokens, personal file paths, or recorded personal data.**
7. **A DCO sign-off on every commit.**
8. **The pull request template filled in**, including the AI-assistance
   disclosure described in [`AI_POLICY.md`](AI_POLICY.md).

Keep the diff focused. Do not reformat or refactor code unrelated to the change.

## AI assistance

AI tools are allowed. The person submitting the pull request remains
responsible for every line: its correctness, its tests, its license
compatibility, and its behavior. Disclose material AI assistance in the pull
request description. [`AI_POLICY.md`](AI_POLICY.md) states the full policy.

## Larger changes

Open an issue before you write a large change: a new subsystem, a public API
addition, a new platform seam, or a provider integration. Describe:

- the problem and who it affects
- the options you considered
- the approach you recommend and the tradeoffs
- the parts of the repository the change would touch

Wait for a maintainer response before investing in a large implementation. Small
fixes and documentation corrections can go straight to a pull request.

## License of contributions

Inbound contributions are dual-licensed under MIT OR Apache-2.0, the same terms
as the project. By signing off a commit you confirm that you have the right to
submit the contribution under those terms. See [`LICENSE`](LICENSE) for the
license statement.
