#!/usr/bin/env bash
# Generates the browser contract bindings for the wasm2 player from the
# committed WebAssembly contract.
#
# The crate is built for `wasm32-unknown-unknown`; `ubrn` then reads the
# UniFFI metadata out of that module and generates the TypeScript against
# `crates/rustic-maps-native-core/src/rustic_maps_wasm.udl`, the frozen
# contract minus the native surface `MapView` interface. The build script
# generates the wasm scaffolding from the same committed file, so the
# signature table and checksums the generated bindings call are exactly the
# ones the module exports.
#
# The generation input is committed so a clean checkout generates the same
# bindings; the output itself is a build artifact, not package source.
#
# Usage:
#   scripts/generate-wasm-bindings.sh                    # release build
#   scripts/generate-wasm-bindings.sh --debug            # debug build
#   scripts/generate-wasm-bindings.sh --profile <name>   # cargo profile
#   scripts/generate-wasm-bindings.sh --output-dir <dir> # default: target/wasm-bindings
#   scripts/generate-wasm-bindings.sh --check            # fail on drift
#
# `--check` regenerates into a temporary directory with the same profile and
# compares it byte-for-byte with the output directory. Run the generation
# once first; the check compares against that generated baseline.
#
# Prerequisites: `npm ci` in `platform/rn` (the `ubrn` binary), a Rust
# toolchain with the `wasm32-unknown-unknown` target, and `wasm-bindgen` at
# the version the crate pins. `ubrn` resolves the crate through `cargo
# metadata`, so it must run from `platform/rn`.
set -euo pipefail

readonly REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly RN_PROJECT_ROOT="${REPO_ROOT}/platform/rn"
readonly BINDING_CRATE_DIR="${REPO_ROOT}/crates/rustic-maps-native-core"
readonly COMMITTED_WASM_CONTRACT="${BINDING_CRATE_DIR}/src/rustic_maps_wasm.udl"
readonly UBRN_COMMAND="${RN_PROJECT_ROOT}/node_modules/.bin/ubrn"
readonly DEFAULT_OUTPUT_DIRECTORY="${REPO_ROOT}/target/wasm-bindings"

# The generated signature table names every contract member the module must
# export. `MapView` is stripped from the browser contract, so its presence in
# the signature table means the generation read the frozen contract instead of
# the committed WebAssembly one.
readonly STRIPPED_INTERFACE_MARKER="MapView"

run_check=false
# `ubrn build wasm2` builds a debug profile unless told otherwise; `-r` is the
# release flag. `--profile` overrides either.
profile_flags=(-r)
profile_name="release"
output_directory="${DEFAULT_OUTPUT_DIRECTORY}"

while [ "$#" -gt 0 ]; do
    case "$1" in
        --release)
            profile_flags=(-r)
            profile_name="release"
            ;;
        --debug)
            profile_flags=()
            profile_name="debug"
            ;;
        --profile)
            if [ "$#" -lt 2 ]; then
                printf 'generate-wasm-bindings: --profile needs a cargo profile name\n' >&2
                exit 1
            fi
            profile_flags=(--profile "$2")
            profile_name="$2"
            shift
            ;;
        --output-dir)
            if [ "$#" -lt 2 ]; then
                printf 'generate-wasm-bindings: --output-dir needs a path\n' >&2
                exit 1
            fi
            output_directory="$2"
            shift
            ;;
        --check)
            run_check=true
            ;;
        *)
            printf 'generate-wasm-bindings: unknown argument %s\n' "$1" >&2
            exit 1
            ;;
    esac
    shift
done

case "$output_directory" in
    /*) ;;
    *) output_directory="${REPO_ROOT}/${output_directory}" ;;
esac

if [ ! -x "$UBRN_COMMAND" ]; then
    printf 'generate-wasm-bindings: %s is missing; run npm ci in platform/rn first\n' \
        "$UBRN_COMMAND" >&2
    exit 1
fi
if [ ! -f "$COMMITTED_WASM_CONTRACT" ]; then
    printf 'generate-wasm-bindings: the committed WebAssembly contract is missing: %s\n' \
        "$COMMITTED_WASM_CONTRACT" >&2
    exit 1
fi

# The generated entrypoint imports the wasm-bindgen glue and stages the module
# beside it. `ubrn` writes them to the TypeScript directory it is configured
# with, which is what this temporary config supplies. Its project root is the
# nearest `package.json` above the working directory, so the `rust` paths stay
# relative to `platform/rn`.
generate_into() {
    local destination="$1"
    local config_directory
    config_directory="$(mktemp -d "${TMPDIR:-/tmp}/rustic-maps-wasm2.XXXXXX")"
    local config_file="${config_directory}/ubrn.config.yaml"
    cat > "$config_file" <<YAML
name: maps-sdk-rn
version: 0.1.0
rust:
  src: ../../crates/rustic-maps-native-core
  manifestPath: ../../crates/rustic-maps-native-core/Cargo.toml
wasm2:
  tsBindings: ${destination}
YAML
    rm -rf "$destination"
    mkdir -p "$destination"
    # A published module must not embed the build machine's absolute source or
    # registry paths: the docs-site scrub refuses a home directory in any
    # staged file, and a remapped build is reproducible across checkouts.
    export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=${REPO_ROOT}=. --remap-path-prefix=${HOME}=."
    (
        cd "$RN_PROJECT_ROOT"
        # The empty array must not expand under `set -u` on bash 3.2.
        if [ "${#profile_flags[@]}" -gt 0 ]; then
            "$UBRN_COMMAND" build wasm2 --config "$config_file" "${profile_flags[@]}"
        else
            "$UBRN_COMMAND" build wasm2 --config "$config_file"
        fi
    )
    rm -rf "$config_directory"

    # Fail before anything consumes the output if the generator read the
    # frozen contract: the signature table would then name exports the
    # stripped module does not carry.
    if grep -q "$STRIPPED_INTERFACE_MARKER" "${destination}/rustic_maps-ffi.ts"; then
        printf 'generate-wasm-bindings: the generated signature table names %s; the generation read the frozen contract instead of %s\n' \
            "$STRIPPED_INTERFACE_MARKER" "$COMMITTED_WASM_CONTRACT" >&2
        exit 1
    fi
    if ! grep -q "export function getSdkCompiledVersion" "${destination}/rustic_maps.ts"; then
        printf 'generate-wasm-bindings: the generated API is missing the namespace surface\n' >&2
        exit 1
    fi
}

report_artifacts() {
    local directory="$1"
    printf 'generated browser bindings in %s\n' "$directory"
    while IFS= read -r file; do
        local bytes
        bytes="$(wc -c < "$file" | tr -d ' ')"
        case "$file" in
            *.ts)
                local lines
                lines="$(wc -l < "$file" | tr -d ' ')"
                printf '  %s: %s bytes, %s lines\n' "${file#"$directory"/}" "$bytes" "$lines"
                ;;
            *)
                printf '  %s: %s bytes\n' "${file#"$directory"/}" "$bytes"
                ;;
        esac
    done < <(find "$directory" -maxdepth 1 -type f | sort)
}

if [ "$run_check" = true ]; then
    if [ ! -d "$output_directory" ]; then
        printf 'generate-wasm-bindings: no generated output at %s to check; run the generation first\n' \
            "$output_directory" >&2
        exit 1
    fi
    comparison_directory="$(mktemp -d "${TMPDIR:-/tmp}/rustic-maps-wasm2-check.XXXXXX")"
    trap 'rm -rf "$comparison_directory"' EXIT
    generate_into "$comparison_directory"
    if ! diff -r "$output_directory" "$comparison_directory"; then
        printf 'generate-wasm-bindings: the generated bindings drifted from %s; regenerate and review the diff\n' \
            "$output_directory" >&2
        exit 1
    fi
    printf 'generated browser bindings are current for the %s profile: %s\n' \
        "$profile_name" "$output_directory"
    exit 0
fi

generate_into "$output_directory"
report_artifacts "$output_directory"
