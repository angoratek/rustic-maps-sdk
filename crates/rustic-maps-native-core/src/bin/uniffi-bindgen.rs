//! The `uniffi-bindgen` command-line entry point, pinned to the same `uniffi`
//! release as this crate's scaffolding; the platform builds regenerate
//! Kotlin, Swift and React Native bindings from the UDL through it.

fn main() {
    uniffi::uniffi_bindgen_main();
}
