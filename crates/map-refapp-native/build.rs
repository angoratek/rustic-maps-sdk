//! Regenerates the Rust scaffolding for the refapp-only contract.
//!
//! `src/map_refapp_core.udl` is the single source of truth for the Kotlin and
//! Swift bindings `scripts/build-refapp-core.sh` generates. The scaffolding is
//! compiled into this crate's library, which the generation reads back from
//! its embedded UniFFI metadata.

fn main() {
    uniffi::generate_scaffolding("src/map_refapp_core.udl")
        .expect("the refapp-only UDL contract must generate its scaffolding");
}
