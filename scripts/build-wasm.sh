#!/usr/bin/env bash
# Builds the WebAssembly package the browser harness and npm artifact use:
# a release wasm32 build of `rustic-maps-web-bindings`, the wasm-bindgen `web` emit,
# and — when binaryen's `wasm-opt` is installed — a size/speed optimization
# pass over the module.
#
# The CLI version must match the `wasm-bindgen` crate version (0.2.129);
# wasm-bindgen refuses a mismatch.
#
# Usage:
#   scripts/build-wasm.sh                  # optimize when wasm-opt exists
#   scripts/build-wasm.sh --no-optimize    # skip wasm-opt
#   scripts/build-wasm.sh --require-optimize  # fail when wasm-opt is absent
set -euo pipefail

readonly REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly PACKAGE_DIRECTORY="${REPO_ROOT}/platform/web/pkg"
readonly WASM_PATH="${REPO_ROOT}/target/wasm32-unknown-unknown/release/rustic_maps_web_bindings.wasm"
# `-Oz` favors size; the flag set enables the features Rust's wasm targets
# emit (bulk memory, non-trapping float conversions, sign extension) and
# mutable globals/reference types used by the JS glue.
readonly WASM_OPT_FLAGS=(-Oz --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext --enable-mutable-globals --enable-reference-types)

no_optimize_requested=false
require_optimize=false
for argument in "$@"; do
    case "$argument" in
        --no-optimize) no_optimize_requested=true ;;
        --require-optimize) require_optimize=true ;;
        *) printf 'build-wasm: unknown argument %s\n' "$argument" >&2; exit 1 ;;
    esac
done
if [ "$no_optimize_requested" = true ] && [ "$require_optimize" = true ]; then
    printf 'build-wasm: --require-optimize contradicts --no-optimize\n' >&2
    exit 1
fi
optimize=true
if [ "$no_optimize_requested" = true ]; then
    optimize=false
fi

(
    cd "$REPO_ROOT"
    # A release module embeds the source paths its panic locations name: the
    # workspace, the dependency registry and the toolchain. The build machine's
    # home directory belongs in neither a published module nor a diagnostics
    # readout, so those roots are remapped to relative names at compile time.
    readable_source_paths="--remap-path-prefix=${REPO_ROOT}=source"
    if [ -n "${HOME:-}" ]; then
        readable_source_paths="${readable_source_paths} --remap-path-prefix=${HOME}=home"
    fi
    RUSTFLAGS="${RUSTFLAGS:-} ${readable_source_paths}" \
        cargo build -p rustic-maps-web-bindings --release --target wasm32-unknown-unknown
    # The package directory holds only generated output: clear it so a
    # renamed module (or a failed earlier emit) cannot leave a stale file
    # beside the current one.
    rm -rf "$PACKAGE_DIRECTORY"
    wasm-bindgen --target web --out-dir "$PACKAGE_DIRECTORY" "$WASM_PATH"
    # Record the sources this emit came from: the harness and the release job
    # both run the freshness gate before believing the package.
    node "${REPO_ROOT}/scripts/check-wasm-package-freshness.mjs" --write
)

readonly MODULE_PATH="${PACKAGE_DIRECTORY}/rustic_maps_web_bindings_bg.wasm"
if [ "$optimize" = true ] && command -v wasm-opt >/dev/null 2>&1; then
    local_before_bytes="$(wc -c < "$MODULE_PATH" | tr -d ' ')"
    wasm-opt "${WASM_OPT_FLAGS[@]}" "$MODULE_PATH" -o "${MODULE_PATH}.optimized"
    mv "${MODULE_PATH}.optimized" "$MODULE_PATH"
    local_after_bytes="$(wc -c < "$MODULE_PATH" | tr -d ' ')"
    printf 'wasm-opt: %s -> %s bytes (%s%% of the unoptimized module)\n' \
        "$local_before_bytes" "$local_after_bytes" \
        "$(awk -v before="$local_before_bytes" -v after="$local_after_bytes" 'BEGIN { printf "%.1f", (after / before) * 100 }')"
elif [ "$require_optimize" = true ]; then
    printf 'build-wasm: wasm-opt is required but is not installed\n' >&2
    exit 1
else
    printf 'wasm-opt: skipped (not installed or --no-optimize)\n'
fi
printf 'wasm package ready: %s\n' "$PACKAGE_DIRECTORY"
