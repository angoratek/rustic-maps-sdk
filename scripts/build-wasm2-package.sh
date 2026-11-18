#!/usr/bin/env bash
# Packages the generated wasm2 bindings as a browser-loadable ES module
# package at platform/web/generated (gitignored build output).
#
# Import resolution — no bundler:
#   The generated TypeScript is compiled to plain ES modules with the repo's
#   pinned TypeScript, and a small rewrite adds the `.js` extension its
#   relative specifiers lack. The only bare specifiers left are `@ubjs/core`,
#   `@ubjs/wasm` and `@ubjs/wasm/core`; the smoke page resolves them with an
#   import map, and `platform/web/serve.mjs` serves the installed runtime
#   packages under `/generated-runtime/@ubjs/...` from
#   `platform/web/node_modules`. No bundler is added: the package is four
#   compiled modules and the installed runtime is served as-is, so the browser
#   loads exactly the files a reviewer can read.
#
# Inputs:
#   `scripts/generate-wasm-bindings.sh` produces the TypeScript and stages the
#   wasm-bindgen `_bg.js` glue and the `.wasm` in `target/wasm-bindings`.
#
# Usage:
#   scripts/build-wasm2-package.sh                  # generate, then package
#   scripts/build-wasm2-package.sh --no-generate    # reuse target/wasm-bindings
#   scripts/build-wasm2-package.sh --no-optimize    # skip wasm-opt
#
# Prerequisites: `npm ci` in `platform/web` (TypeScript and the `@ubjs`
# runtime packages) and, for the generation step, the conditions in
# `scripts/generate-wasm-bindings.sh`.
set -euo pipefail

readonly REPOSITORY_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly GENERATED_SOURCE_DIRECTORY="${REPOSITORY_ROOT}/target/wasm-bindings"
readonly PACKAGE_DIRECTORY="${REPOSITORY_ROOT}/platform/web/generated"
readonly TYPESCRIPT_BINARY="${REPOSITORY_ROOT}/platform/web/node_modules/.bin/tsc"
readonly RUNTIME_PACKAGE_DIRECTORY="${REPOSITORY_ROOT}/platform/web/node_modules/@ubjs"
readonly MODULE_PATH="${PACKAGE_DIRECTORY}/rustic_maps.wasm"
# `-Oz` favors size; the flag set enables the features Rust's wasm targets
# emit (bulk memory, non-trapping float conversions, sign extension) and
# mutable globals/reference types used by the JS glue. Kept aligned with
# `scripts/build-wasm.sh`.
readonly WASM_OPT_FLAGS=(-Oz --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext --enable-mutable-globals --enable-reference-types)

generate=true
optimize=true
for argument in "$@"; do
    case "$argument" in
        --no-generate) generate=false ;;
        --no-optimize) optimize=false ;;
        *) printf 'build-wasm2-package: unknown argument %s\n' "$argument" >&2; exit 1 ;;
    esac
done

if [ ! -x "$TYPESCRIPT_BINARY" ]; then
    printf 'build-wasm2-package: %s is missing; run npm ci in platform/web first\n' \
        "$TYPESCRIPT_BINARY" >&2
    exit 1
fi
if [ ! -d "$RUNTIME_PACKAGE_DIRECTORY" ]; then
    printf 'build-wasm2-package: the @ubjs runtime packages are missing; run npm ci in platform/web\n' >&2
    exit 1
fi

if [ "$generate" = true ]; then
    "${REPOSITORY_ROOT}/scripts/generate-wasm-bindings.sh" --release
elif [ ! -f "${GENERATED_SOURCE_DIRECTORY}/index.ts" ]; then
    printf 'build-wasm2-package: no generated bindings at %s to package; run without --no-generate\n' \
        "$GENERATED_SOURCE_DIRECTORY" >&2
    exit 1
fi

# The package directory holds only generated output: clear it so a removed
# module (or a failed earlier compile) cannot leave a stale file beside the
# current one.
rm -rf "$PACKAGE_DIRECTORY"
mkdir -p "$PACKAGE_DIRECTORY"

# `--noCheck` keeps compilation fast and independent of the runtime packages'
# type declarations: the generated sources carry `@ts-nocheck`, and this step
# only needs the emit. `--moduleResolution bundler` matches the generator's
# extensionless relative imports, which the rewrite below turns into browser
# URLs.
"$TYPESCRIPT_BINARY" \
    --noCheck \
    --target ES2022 \
    --module ES2022 \
    --moduleResolution bundler \
    --skipLibCheck \
    --rootDir "$GENERATED_SOURCE_DIRECTORY" \
    --outDir "$PACKAGE_DIRECTORY" \
    "${GENERATED_SOURCE_DIRECTORY}/index.ts"

node "${REPOSITORY_ROOT}/scripts/add-generated-esm-extensions.mjs" "$PACKAGE_DIRECTORY"

# The wasm-bindgen glue is plain ES module JavaScript and is served beside the
# compiled modules; the module itself is the one `uniffiInitAsync` opens.
cp "${GENERATED_SOURCE_DIRECTORY}/rustic_maps_bg.js" "$PACKAGE_DIRECTORY/"
cp "${GENERATED_SOURCE_DIRECTORY}/rustic_maps.wasm" "$PACKAGE_DIRECTORY/"

if [ "$optimize" = true ] && command -v wasm-opt >/dev/null 2>&1; then
    local_before_bytes="$(wc -c < "$MODULE_PATH" | tr -d ' ')"
    wasm-opt "${WASM_OPT_FLAGS[@]}" "$MODULE_PATH" -o "${MODULE_PATH}.optimized"
    mv "${MODULE_PATH}.optimized" "$MODULE_PATH"
    local_after_bytes="$(wc -c < "$MODULE_PATH" | tr -d ' ')"
    printf 'wasm-opt: %s -> %s bytes (%s%% of the unoptimized module)\n' \
        "$local_before_bytes" "$local_after_bytes" \
        "$(awk -v before="$local_before_bytes" -v after="$local_after_bytes" 'BEGIN { printf "%.1f", (after / before) * 100 }')"
else
    printf 'wasm-opt: skipped (not installed or --no-optimize)\n'
fi

# Record the inputs this package came from; the browser smoke and the release
# job both run the freshness check before believing the package.
node "${REPOSITORY_ROOT}/scripts/check-wasm2-package-freshness.mjs" --write

printf 'generated browser package ready: %s\n' "$PACKAGE_DIRECTORY"
while IFS= read -r file; do
    printf '  %s: %s bytes\n' "${file#"$PACKAGE_DIRECTORY"/}" "$(wc -c < "$file" | tr -d ' ')"
done < <(find "$PACKAGE_DIRECTORY" -maxdepth 1 -type f | sort)
