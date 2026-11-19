#!/bin/sh
# Runs the full verification gate once, outside a push, and records a stamp
# keyed on the tree it checked. The pre-push hook accepts that stamp and
# returns immediately, so a push never holds the transport open for the
# gate's duration while the remote waits for the pack.
#
# The gate is the same one CI runs: fmt, workspace clippy with -D warnings
# and the workspace tests. A stamp is written only when every step passed
# and the working tree matches the commit, so a stamp always names a tree
# that was actually checked.
#
#     scripts/run-gate.sh          # gate this tree, write its stamp
#     git push                     # hook finds the stamp, returns at once
#
# Bypass a single push with MAPS_SDK_SKIP_HOOKS=1, as the hook documents.
set -eu

repository_root=$(git rev-parse --show-toplevel)
cd "$repository_root"

gate() {
    gate_name="$1"
    shift
    printf '\n==> maps-sdk gate: %s\n' "$gate_name"
    if ! "$@"; then
        printf '\nmaps-sdk gate: the %s step failed (output above). Fix it and run the gate again.\n' "$gate_name"
        exit 1
    fi
}

gate 'cargo fmt' cargo fmt --all --check
gate 'cargo clippy' cargo clippy --workspace --all-targets -- -D warnings
gate 'cargo test' cargo test --workspace

printf '\nmaps-sdk gate: all three gates green.\n'

if [ -n "$(git status --porcelain)" ]; then
    printf '%s\n' 'maps-sdk gate: the working tree has uncommitted changes, so no stamp was written.'
    exit 0
fi

head_tree=$(git rev-parse --verify HEAD^{tree})
stamp_directory=$(git rev-parse --git-path maps-sdk-gate-stamps)
mkdir -p "$stamp_directory"
date -u +%Y-%m-%dT%H:%M:%SZ > "$stamp_directory/$head_tree"
printf 'maps-sdk gate: stamp written for tree %s.\n' "$head_tree"
