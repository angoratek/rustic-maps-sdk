## What and why

Describe the change and the problem it solves. Link the user-visible behavior
it affects.

## Linked issue

Link the issue when one exists.

Closes #

## How tested

List the exact commands you ran and the result:

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Add the browser, platform, or documentation gates that apply.

## Docs and changelog

- [ ] `CHANGELOG.md` has an entry for the user-visible change.
- [ ] The guides, API reference, or screenshots are updated where the change
      affects them.

## Safety

- [ ] No credential, token, signing key, or personal file path is included.
- [ ] Every commit carries a DCO `Signed-off-by` line.
- [ ] No test reaches the network; any new recorded data is committed.

## AI assistance

Disclose material AI assistance here, as described in `AI_POLICY.md`. State the
tool and the parts of the change it produced. Routine assistance needs one
line.
