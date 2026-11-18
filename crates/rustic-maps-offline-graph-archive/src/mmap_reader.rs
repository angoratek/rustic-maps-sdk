//! Native, memory-mapped consumption seam for `OfflineRoutingNetworkGraph`
//! archive files.
//!
//! The mapped file is a zero-copy handoff: it answers the same bytes the
//! fully validating in-memory reader
//! ([`OfflineRoutingNetworkGraph::from_bytes`]) consumes unchanged, so both
//! load paths — the file-mapped one here and the in-memory one in
//! [`crate::archive`] — share the entire typed corruption model. The module
//! is compiled only for native targets (see the workspace manifest): the
//! browser loads the graph from in-memory bytes, with no `mmap` in the wasm
//! path.
//!
//! # Alignment validation
//!
//! Opening the mapping validates the file-level alignment invariants the
//! archive format fixes ([`crate::archive`]):
//!
//! - the mapped region covers at least [`FILE_HEADER_BYTES`];
//! - its byte length is a multiple of [`ARCHIVE_RESOURCE_ALIGNMENT`] — the
//!   deterministic writer always pads the archive's total length to that
//!   boundary, so a region that breaks it is a truncated or corrupt archive;
//! - the mapped base address honors the same alignment, which keeps the
//!   little-endian `u64` table-of-contents field reads naturally aligned.
//!   Page-granular mapping guarantees this; it is validated, not assumed.
//!
//! Deeper structure (table of contents, record widths, cross-references) is
//! validated by the reader, exactly as for in-memory bytes.
//!
//! # Error mapping
//!
//! File-level failures (missing, unreadable, mapping refused) surface as
//! `SDKError::OfflineRoutingFailure(OfflineGraphInvalidReason::ArchiveMalformed)`:
//! the typed corruption vocabulary deliberately has no I/O cause (the
//! [`SDKError`] variants serialize across the frozen FFI contract), and an
//! archive that cannot be mapped is unusable in exactly the way a malformed
//! one is. A recognized-but-unsupported archive still fails as
//! `VersionUnsupported` out of the shared reader.

use std::path::Path;

use geospatial_types::OfflineGraphInvalidReason;
use geospatial_types::SDKError;
use memmap2::Mmap;

use crate::archive::{ARCHIVE_RESOURCE_ALIGNMENT, FILE_HEADER_BYTES, OfflineRoutingNetworkGraph};

/// A memory-mapped archive file.
///
/// The mapped region stays valid for the struct's lifetime and exposes the
/// mapped bytes through [`Self::mapped_bytes`]; the validating reader materializes
/// the graph records out of them without touching the filesystem again.
#[derive(Debug)]
pub struct MemoryMappedArchive {
    mapped_file: Mmap,
}

impl MemoryMappedArchive {
    /// Memory-maps an archive file and validates its alignment invariants.
    ///
    /// Structural validation beyond alignment is deferred to the shared
    /// reader (see the module docs).
    ///
    /// # Errors
    /// `SDKError::OfflineRoutingFailure(ArchiveMalformed)` for every I/O and
    /// alignment failure (see the module docs); reader-side failures are
    /// typed unchanged by [`OfflineRoutingNetworkGraph::from_bytes`].
    pub fn open(archive_path: &Path) -> Result<Self, SDKError> {
        let source_file =
            std::fs::File::open(archive_path).map_err(|_| archive_malformed_error())?;
        // SAFETY: `Mmap::map` has no preconditions beyond a readable file
        // descriptor; the resulting view is read-only over the whole file
        // and never mutated in place.
        let mapped_file =
            unsafe { Mmap::map(&source_file) }.map_err(|_| archive_malformed_error())?;

        let mapped_bytes: &[u8] = &mapped_file;
        if mapped_bytes.len() < FILE_HEADER_BYTES
            || !mapped_bytes
                .len()
                .is_multiple_of(ARCHIVE_RESOURCE_ALIGNMENT)
            || !mapped_bytes
                .as_ptr()
                .addr()
                .is_multiple_of(ARCHIVE_RESOURCE_ALIGNMENT)
        {
            return Err(archive_malformed_error());
        }
        Ok(Self { mapped_file })
    }

    /// The mapped archive bytes, exactly as they sit in the file.
    pub fn mapped_bytes(&self) -> &[u8] {
        &self.mapped_file
    }
}

/// Memory-maps an archive file and fully validates it, returning the
/// [`OfflineRoutingNetworkGraph`] graph itself (the validation materializes
/// the typed records from the mapped bytes).
///
/// # Errors
/// See [`MemoryMappedArchive::open`] and
/// [`OfflineRoutingNetworkGraph::from_bytes`].
pub fn offline_routing_network_graph_from_mapped_file(
    archive_path: &Path,
) -> Result<OfflineRoutingNetworkGraph, SDKError> {
    OfflineRoutingNetworkGraph::from_bytes(MemoryMappedArchive::open(archive_path)?.mapped_bytes())
}

/// The typed failure every unmappable or alignment-violating archive maps to
/// (see the module docs for the mapping rationale).
fn archive_malformed_error() -> SDKError {
    SDKError::OfflineRoutingFailure(OfflineGraphInvalidReason::ArchiveMalformed)
}
