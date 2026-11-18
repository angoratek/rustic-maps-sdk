//! Binary tool: OpenStreetMap PBF extract → `OfflineRoutingNetworkGraph`
//! archive (nodes, directed edges with encoded speed/class/access columns,
//! turn restrictions, lane guidance, instruction strings). Maintainer/CI
//! tooling; never shipped in the SDK. The container format — this crate's
//! own minimal flatbuffer container instead of the `flatdata` crate — is
//! described in the library docs of this crate.
//!
//! Usage: `rustic-maps-offline-graph-compiler <input.osm.pbf> <output.routing-graph>`

use std::path::Path;
use std::process::ExitCode;

use rustic_maps_offline_graph_compiler::archive::OfflineRoutingNetworkGraph;
use rustic_maps_offline_graph_compiler::{CompilerError, compile_osm_extract_file};

const USAGE_TEXT: &str =
    "usage: rustic-maps-offline-graph-compiler <input.osm.pbf> <output.routing-graph>";

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match arguments.as_slice() {
        [input_path, output_path] => compile_input_to_output(input_path, output_path),
        _ => {
            eprintln!("{USAGE_TEXT}");
            ExitCode::from(2)
        }
    }
}

fn compile_input_to_output(input_path: &str, output_path: &str) -> ExitCode {
    match run_compile_input_to_output(input_path, output_path) {
        Ok(()) => ExitCode::SUCCESS,
        Err(compiler_error) => {
            eprintln!("rustic-maps-offline-graph-compiler: {compiler_error}");
            ExitCode::FAILURE
        }
    }
}

/// Compiles and writes the archive, then re-reads it through the validating
/// reader as a self-check and prints the archive summary.
fn run_compile_input_to_output(input_path: &str, output_path: &str) -> Result<(), CompilerError> {
    let archive_bytes = compile_osm_extract_file(Path::new(input_path))?;
    std::fs::write(Path::new(output_path), &archive_bytes)
        .map_err(CompilerError::InputReadFailure)?;
    let graph = OfflineRoutingNetworkGraph::from_bytes(&archive_bytes).map_err(|sdk_error| {
        CompilerError::OsmParsingFailure(format!(
            "compiled archive failed self-check: {sdk_error:?}"
        ))
    })?;
    println!(
        "compiled {} nodes, {} edges, {} turn restrictions, {} lane guidance records -> {}",
        graph.nodes().len(),
        graph.edges().len(),
        graph.turn_restrictions().len(),
        graph.lane_guidances().len(),
        output_path
    );
    Ok(())
}
