//! Milestone M7 slice 2 verify gates for the memory-mapped consumption seam:
//! the mapped archive answers the same validating reader as in-memory
//! bytes, and every file-level failure (missing file, truncation, padding
//! that breaks the alignment invariant) fails typed — never a panic or an
//! uninitialized view.

use geospatial_types::{OfflineGraphInvalidReason, SDKError};
use rustic_maps_offline_graph_archive::MemoryMappedArchive;
use rustic_maps_offline_graph_archive::archive::OfflineRoutingNetworkGraph;
use rustic_maps_offline_graph_archive::offline_routing_network_graph_from_mapped_file;
use rustic_maps_offline_graph_compiler::compiler::compile_osm_extract_bytes;

use common::fixture_bytes;

// The shared fixture-generator module serves several test binaries; this
// one consumes only the committed fixture bytes, so its builder surface is
// deliberately unused here.
#[allow(dead_code)]
mod common;

fn fixture_archive_bytes() -> Vec<u8> {
    compile_osm_extract_bytes(&fixture_bytes()).expect("the committed fixture compiles")
}

/// Distinct temp-file name; no tempfile dependency at test time.
fn unique_temporary_archive_path() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static PATH_COUNTER: AtomicUsize = AtomicUsize::new(0);
    let unique_counter = PATH_COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "rustic-maps-mmap-test-{}-{unique_counter}.rmog",
        std::process::id()
    ))
}

/// A scratch file that deletes itself when the test ends.
struct TemporaryArchiveFile {
    path: std::path::PathBuf,
}

impl TemporaryArchiveFile {
    fn write_with(archive_bytes: &[u8]) -> Self {
        let path = unique_temporary_archive_path();
        std::fs::write(&path, archive_bytes)
            .expect("the test archive writes to the temp directory");
        Self { path }
    }
}

impl Drop for TemporaryArchiveFile {
    fn drop(&mut self) {
        std::fs::remove_file(&self.path).ok();
    }
}

#[test]
fn mapped_archive_answers_the_validating_reader() {
    let temporary_file = TemporaryArchiveFile::write_with(&fixture_archive_bytes());

    let mapped_archive = MemoryMappedArchive::open(&temporary_file.path)
        .expect("the fixture archive maps and passes the alignment validation");
    let graph = OfflineRoutingNetworkGraph::from_bytes(mapped_archive.mapped_bytes())
        .expect("the mapped bytes validate like in-memory bytes");
    assert_eq!(graph.nodes().len(), 6);
    assert_eq!(graph.edges().len(), 8);
    assert_eq!(
        mapped_archive.mapped_bytes().len(),
        fixture_archive_bytes().len()
    );
}

#[test]
fn mapped_file_convenience_loads_the_whole_graph() {
    let temporary_file = TemporaryArchiveFile::write_with(&fixture_archive_bytes());
    let graph = offline_routing_network_graph_from_mapped_file(&temporary_file.path)
        .expect("the mapped fixture archive validates");
    // Route-relevant round-trip facts re-asserted cheaply here (the
    // exhaustive field checks live in the round-trip suite).
    assert_eq!(graph.turn_restrictions().len(), 1);
}

#[test]
fn missing_file_fails_typed_as_archive_malformed() {
    let never_written_path = unique_temporary_archive_path();
    assert_eq!(
        MemoryMappedArchive::open(&never_written_path).unwrap_err(),
        SDKError::OfflineRoutingFailure(OfflineGraphInvalidReason::ArchiveMalformed),
    );
}

#[test]
fn file_shorter_than_the_header_fails_typed_as_archive_malformed() {
    let truncated_header_bytes = fixture_archive_bytes()[..8].to_vec();
    let temporary_file = TemporaryArchiveFile::write_with(&truncated_header_bytes);
    assert_eq!(
        MemoryMappedArchive::open(&temporary_file.path).unwrap_err(),
        SDKError::OfflineRoutingFailure(OfflineGraphInvalidReason::ArchiveMalformed),
    );
}

#[test]
fn byte_aligned_truncation_still_fails_in_the_validating_reader() {
    let archive_bytes = fixture_archive_bytes();
    // Truncate on the 8-byte alignment boundary: past the mmap seam's own
    // alignment checks, but inside the table of contents.
    let aligned_truncation = archive_bytes.len() - 8;
    let temporary_file = TemporaryArchiveFile::write_with(&archive_bytes[..aligned_truncation]);
    assert_eq!(
        offline_routing_network_graph_from_mapped_file(&temporary_file.path).unwrap_err(),
        SDKError::OfflineRoutingFailure(OfflineGraphInvalidReason::ArchiveMalformed),
    );
}

#[test]
fn padding_that_breaks_the_container_length_invariant_is_rejected() {
    let mut padded_archive_bytes = fixture_archive_bytes();
    padded_archive_bytes.push(0);
    let temporary_file = TemporaryArchiveFile::write_with(&padded_archive_bytes);
    assert_eq!(
        MemoryMappedArchive::open(&temporary_file.path).unwrap_err(),
        SDKError::OfflineRoutingFailure(OfflineGraphInvalidReason::ArchiveMalformed),
    );
}
