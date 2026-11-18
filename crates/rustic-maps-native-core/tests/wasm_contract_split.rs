//! The committed WebAssembly contract must be exactly the frozen contract
//! minus the native surface interface, so `ubrn`'s on-disk lookup and the
//! wasm scaffolding generation consume the same file and a contract edit
//! cannot drift away from the browser generation input.

/// The frozen contract the native and React Native bindings compile whole.
const FROZEN_CONTRACT: &str = include_str!("../src/rustic_maps.udl");

/// The committed generation input for the WebAssembly scaffolding and the
/// browser bindings.
const WASM_CONTRACT: &str = include_str!("../src/rustic_maps_wasm.udl");

/// The native surface interface the WebAssembly module does not compile.
const SURFACE_INTERFACE_HEADER: &str = "interface MapView {";

/// Returns the contract with the native surface interface, and the contiguous
/// `///` documentation directly above it, removed.
///
/// This is the split the committed file records: the interface body closes at
/// the first line whose braces bring the nesting depth back to zero. The
/// function panics unless the contract carries exactly one such interface, so
/// a rename or a duplicate cannot produce a different expectation.
fn frozen_contract_without_surface_interface() -> String {
    let lines: Vec<&str> = FROZEN_CONTRACT.lines().collect();
    let header_indexes: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.trim() == SURFACE_INTERFACE_HEADER)
        .map(|(index, _)| index)
        .collect();
    assert_eq!(
        header_indexes.len(),
        1,
        "the frozen contract must carry exactly one `{SURFACE_INTERFACE_HEADER}` interface"
    );
    let header_index = header_indexes[0];

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
    let end_index = end_index
        .unwrap_or_else(|| panic!("the `{SURFACE_INTERFACE_HEADER}` interface must close"));

    let mut stripped = String::with_capacity(FROZEN_CONTRACT.len());
    for (line_index, line) in lines.iter().enumerate() {
        if (start_index..=end_index).contains(&line_index) {
            continue;
        }
        stripped.push_str(line);
        stripped.push('\n');
    }
    stripped
}

#[test]
fn the_committed_wasm_contract_is_the_frozen_contract_without_the_surface_interface() {
    assert_eq!(
        WASM_CONTRACT,
        frozen_contract_without_surface_interface(),
        "src/rustic_maps_wasm.udl must be regenerated from src/rustic_maps.udl: \
         it is the frozen contract with the `MapView` interface and its doc block removed"
    );
}

#[test]
fn the_wasm_contract_keeps_every_other_interface_and_dictionary() {
    assert!(
        !WASM_CONTRACT.contains(SURFACE_INTERFACE_HEADER),
        "the WebAssembly contract must not carry the native surface interface"
    );

    for declaration in [
        "interface MapSdk",
        "interface MapCamera",
        "interface Navigator",
        "interface RouteSimulator",
        "interface GeocodingProvider",
        "interface RasterTileProvider",
        "dictionary FrameStatistics",
        "dictionary MapOverlay",
        "dictionary SurfaceHandle",
    ] {
        assert!(
            FROZEN_CONTRACT.contains(declaration),
            "the frozen contract must carry {declaration}"
        );
        assert!(
            WASM_CONTRACT.contains(declaration),
            "the WebAssembly contract must keep {declaration}"
        );
    }

    assert!(
        WASM_CONTRACT.len() < FROZEN_CONTRACT.len(),
        "the WebAssembly contract is the frozen contract minus its surface interface"
    );
}
