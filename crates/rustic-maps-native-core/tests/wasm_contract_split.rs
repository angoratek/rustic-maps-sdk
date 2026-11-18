//! The WebAssembly scaffolding is generated from the frozen contract minus the
//! native surface interface, so the browser module carries the generated
//! contract surface without a dead `MapView` implementation.

/// The frozen contract the native and React Native bindings compile whole.
const SOURCE_CONTRACT: &str = include_str!("../src/rustic_maps.udl");

#[test]
fn the_wasm_contract_keeps_the_contract_and_drops_only_the_surface_interface() {
    let wasm_contract = std::fs::read_to_string(concat!(env!("OUT_DIR"), "/wasm/rustic_maps.udl"))
        .expect("the build script writes the WebAssembly contract copy");

    assert_eq!(
        SOURCE_CONTRACT.matches("interface MapView").count(),
        1,
        "the frozen contract carries exactly one native surface interface"
    );
    assert!(
        !wasm_contract.contains("interface MapView"),
        "the WebAssembly contract must not carry the native surface interface"
    );

    for interface in [
        "interface MapSdk",
        "interface MapCamera",
        "interface Navigator",
        "interface RouteSimulator",
        "interface GeocodingProvider",
        "interface RasterTileProvider",
    ] {
        assert!(
            SOURCE_CONTRACT.contains(interface),
            "the frozen contract must carry {interface}"
        );
        assert!(
            wasm_contract.contains(interface),
            "the WebAssembly contract must keep {interface}"
        );
    }

    for dictionary in [
        "dictionary FrameStatistics",
        "dictionary MapOverlay",
        "dictionary SurfaceHandle",
    ] {
        assert!(
            wasm_contract.contains(dictionary),
            "the WebAssembly contract must keep {dictionary}"
        );
    }

    assert!(
        wasm_contract.len() < SOURCE_CONTRACT.len(),
        "the WebAssembly contract is the frozen contract minus its surface interface"
    );
}
