//! Regenerates the Rust scaffolding from the frozen UDL contract.
//!
//! The contract file is the single source of truth for every target. The
//! native and React Native bindings compile it whole; the WebAssembly module
//! compiles the same contract without the native surface `MapView`
//! interface, because the browser owns the surface in the hand-written canvas
//! class and has no implementation to offer the generated interface. The
//! stripped copy is written beside the generated scaffolding so a test can
//! assert the split stays exact.

use std::fs;
use std::path::Path;

/// The frozen contract every binding is generated from.
const CONTRACT_PATH: &str = "src/rustic_maps.udl";

/// The native surface interface the WebAssembly module does not compile.
const SURFACE_INTERFACE_HEADER: &str = "interface MapView {";

/// The subdirectory of `OUT_DIR` holding the WebAssembly contract copy. The
/// generator names its scaffolding output after the UDL file's stem, so the
/// copy keeps the contract's own name and lives one directory down.
const WASM_CONTRACT_DIRECTORY_NAME: &str = "wasm";

/// The contract's file name, kept so the generated module stays `rustic_maps`.
const CONTRACT_FILE_NAME: &str = "rustic_maps.udl";

fn main() {
    println!("cargo:rerun-if-changed={CONTRACT_PATH}");
    let contract = fs::read_to_string(CONTRACT_PATH).unwrap_or_else(|error| {
        panic!("the UDL contract {CONTRACT_PATH} must be readable: {error}")
    });
    let wasm_contract = without_surface_interface(&contract);

    // The stripped copy is written on every build (its contents are
    // deterministic) so the split test can read it from `OUT_DIR`.
    let output_directory =
        std::env::var("OUT_DIR").expect("the build script always runs with an OUT_DIR");
    let wasm_contract_directory = Path::new(&output_directory).join(WASM_CONTRACT_DIRECTORY_NAME);
    fs::create_dir_all(&wasm_contract_directory).unwrap_or_else(|error| {
        panic!("the WebAssembly contract directory must be creatable: {error}")
    });
    let wasm_contract_path = wasm_contract_directory.join(CONTRACT_FILE_NAME);
    fs::write(&wasm_contract_path, &wasm_contract)
        .unwrap_or_else(|error| panic!("the WebAssembly contract copy must be writable: {error}"));

    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32") {
        // The stripped copy sits in the build output, outside the crate's
        // `src/`, so the crate name is supplied instead of guessed.
        let crate_name = std::env::var("CARGO_PKG_NAME")
            .expect("the build script always runs with a CARGO_PKG_NAME")
            .replace('-', "_");
        let wasm_contract_source = wasm_contract_path
            .to_str()
            .expect("the build output directory path is valid UTF-8");
        uniffi::generate_scaffolding_for_crate(wasm_contract_source, &crate_name).unwrap();
    } else {
        uniffi::generate_scaffolding(CONTRACT_PATH).unwrap();
    }
}

/// Returns the contract with the native surface interface, and the doc block
/// that introduces it, removed.
///
/// The interface's documentation is contiguous `///` lines directly above its
/// header; the body closes at the first `};` whose braces bring the interface
/// depth back to zero. The function refuses a contract that does not carry
/// exactly one surface interface, so a rename or a second copy cannot silently
/// leave the WebAssembly module without the expected split.
fn without_surface_interface(contract: &str) -> String {
    let lines: Vec<&str> = contract.lines().collect();
    let header_index = lines
        .iter()
        .position(|line| line.trim() == SURFACE_INTERFACE_HEADER)
        .unwrap_or_else(|| {
            panic!("the UDL contract must carry exactly one `{SURFACE_INTERFACE_HEADER}` interface")
        });
    assert!(
        lines
            .iter()
            .filter(|line| line.trim() == SURFACE_INTERFACE_HEADER)
            .count()
            == 1,
        "the UDL contract must carry exactly one `{SURFACE_INTERFACE_HEADER}` interface"
    );

    let mut start_index = header_index;
    while start_index > 0 && lines[start_index - 1].trim_start().starts_with("///") {
        start_index -= 1;
    }

    let mut depth = 0_i32;
    let mut end_index = None;
    for (line_index, line) in lines.iter().enumerate().skip(header_index) {
        depth += line.matches('{').count() as i32;
        depth -= line.matches('}').count() as i32;
        if depth == 0 {
            end_index = Some(line_index);
            break;
        }
    }
    let end_index = end_index.unwrap_or_else(|| {
        panic!("the UDL contract's `{SURFACE_INTERFACE_HEADER}` interface must close")
    });

    let mut stripped = String::with_capacity(contract.len());
    for (line_index, line) in lines.iter().enumerate() {
        if (start_index..=end_index).contains(&line_index) {
            continue;
        }
        stripped.push_str(line);
        stripped.push('\n');
    }
    stripped
}
