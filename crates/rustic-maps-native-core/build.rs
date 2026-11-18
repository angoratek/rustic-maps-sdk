//! Generates the Rust scaffolding from the committed UDL contracts.
//!
//! The frozen contract, `src/rustic_maps.udl`, is the single source of truth
//! for every target. The native and React Native bindings compile it whole.
//! The WebAssembly module compiles `src/rustic_maps_wasm.udl` instead: the
//! same contract without the native surface `MapView` interface, because the
//! browser owns the surface in the hand-written canvas class and has no
//! implementation to offer the generated interface.
//!
//! The WebAssembly contract is committed rather than derived here because
//! `ubrn` reads the UDL from disk when it generates the browser bindings.
//! Its `CrateConfigSupplier` looks the file up under the crate's library
//! target name, so the scaffolding is generated with that same name and
//! records it in the module's `UDL_FILE` metadata; `ubrn` then resolves the
//! committed file through the library target. The split test
//! (`tests/wasm_contract_split.rs`) asserts that the committed file is
//! exactly the frozen contract minus the `MapView` block, so the two cannot
//! drift apart silently.

use std::fs;

/// The frozen contract every binding is generated from.
const FROZEN_CONTRACT_PATH: &str = "src/rustic_maps.udl";

/// The committed WebAssembly contract: the frozen contract minus the native
/// surface interface and the doc block that introduces it.
const WASM_CONTRACT_PATH: &str = "src/rustic_maps_wasm.udl";

/// The library target name (`[lib] name` in `Cargo.toml`). The generated
/// scaffolding records this name in its `UDL_FILE` metadata, and `ubrn`'s
/// cargo-metadata path supplier keys crate roots by target name. Using the
/// package name here (`rustic-maps-native-core`) would make `ubrn` look for a
/// crate that no cargo target declares.
const WASM_CONTRACT_CRATE_NAME: &str = "rustic_maps";

fn main() {
    println!("cargo:rerun-if-changed={FROZEN_CONTRACT_PATH}");
    println!("cargo:rerun-if-changed={WASM_CONTRACT_PATH}");

    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32") {
        // The committed contract lives in the crate's `src/`, so the crate
        // name is supplied explicitly instead of being guessed from a path
        // outside the crate. The generator names its output after the UDL
        // file's stem (`rustic_maps_wasm.uniffi.rs`), which `lib.rs` includes
        // on wasm.
        let wasm_contract = fs::read_to_string(WASM_CONTRACT_PATH).unwrap_or_else(|error| {
            panic!("the WebAssembly UDL contract {WASM_CONTRACT_PATH} must be readable: {error}")
        });
        assert!(
            !wasm_contract.contains("interface MapView {"),
            "the WebAssembly UDL contract must not carry the native surface interface"
        );
        uniffi::generate_scaffolding_for_crate(WASM_CONTRACT_PATH, WASM_CONTRACT_CRATE_NAME)
            .unwrap();
    } else {
        uniffi::generate_scaffolding(FROZEN_CONTRACT_PATH).unwrap();
    }
}
