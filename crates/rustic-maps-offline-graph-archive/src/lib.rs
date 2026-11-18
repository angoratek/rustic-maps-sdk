//! The `OfflineRoutingNetworkGraph` archive format: the format's reader and
//! writer home, viable on every SDK target.
//!
//! This crate is pure byte parsing and encoding with no platform-specific
//! dependencies outside [`geospatial_types`], so it builds on native
//! targets and on `wasm32-unknown-unknown` alike: [`archive`] carries the
//! format constants, the packed record types, the deterministic writer
//! and the fully validating in-memory reader
//! ([`archive::OfflineRoutingNetworkGraph::from_bytes`]), and the native, memory-mapped
//! consumption seam ([`mmap_reader`]) is cfg-gated to non-wasm targets
//! because `mmap` does not exist in the browser path.
//!
//! The two load paths share the entire typed corruption model: the
//! mmap-backed reader answers the same bytes the in-memory reader
//! consumes, so a route computed from mapped file bytes and one computed
//! from in-memory bytes of the same archive are identical by construction.
//!
//! # Container format
//!
//! The archive is a minimal flatbuffer container of this crate's own
//! definition: resource names, record layout and validation semantics form a
//! single self-describing little-endian file that this crate both writes
//! deterministically and reads back with typed corruption errors.
//!
//! The format deliberately does not build on the `flatdata` Rust crate
//! (crates.io `flatdata 0.5.8`, last published 2024-08-30): its Rust code
//! generation has no macro path — a `build.rs` must call
//! `flatdata::generate`, which shells out to `python3`/`pip` at build time
//! and installs `flatdata-generator==0.4.6` from PyPI into a virtualenv
//! under `OUT_DIR` before running it. That makes every workspace build
//! non-hermetic and dependent on the network and a third-party Python
//! toolchain, which the build and test isolation this workspace requires
//! does not allow. The format versioning in [`archive`] is the seam for a
//! byte-compatible migration if the `flatdata` ecosystem gains a hermetic
//! Rust code-generation path.
//!
//! # Archive format (version 1)
//!
//! All integers are little-endian. The single-file layout is:
//!
//! ```text
//! offset 0  ARCHIVE_MAGIC ("RMOG")                        4 bytes
//! offset 4  format version (u32)                          4 bytes
//! offset 8  table-of-contents entry count (u32)           4 bytes
//! offset 12 table-of-contents entries, 48 bytes each:
//!           resource name (32 ASCII bytes, NUL-padded) +
//!           resource offset (u64) + resource byte length (u64)
//! resources in canonical order (see the [`archive`] docs), each offset a
//! multiple of 8; unnamed padding gaps of up to 7 bytes may separate them.
//! ```
//!
//! Resources hold packed array-of-structs records — the GraphHopper-style
//! encoded-value columns ([`archive::EDGE_RECORD_BYTES`]'s `encoded_value`:
//! speed cells, road class, direction access) keep the routing hot path
//! branch-free. Corruption anywhere fails typed: [`geospatial_types`]'s
//! `SDKError::OfflineRoutingFailure` with
//! `OfflineGraphInvalidReason::{ArchiveMalformed, VersionUnsupported,
//! IndexOutOfRange}`.
//!
//! # Who produces and consumes this format
//!
//! - The `rustic-maps-offline-graph-compiler` crate (the OpenStreetMap PBF →
//!   archive compiler, a maintainer/CI tool that never ships inside the
//!   SDK and stays native-only: its `osmpbf` dependency is not
//!   wasm-viable) writes archives in this format and re-exports the
//!   [`archive`] module for its own code, fixtures and tests.
//! - The SDK's `rustic-maps-core` `OfflineRoutingProvider` consumes archives
//!   through this crate on every target: in-memory bytes (the
//!   `OfflineGraphSource::Memory` seam, the wasm path) everywhere, and
//!   [`mmap_reader`]'s mapped file path on native targets.

#![cfg_attr(not(test), deny(clippy::expect_used, clippy::unwrap_used))]

pub mod archive;

#[cfg(not(target_arch = "wasm32"))]
pub mod mmap_reader;

#[cfg(not(target_arch = "wasm32"))]
pub use mmap_reader::{MemoryMappedArchive, offline_routing_network_graph_from_mapped_file};
