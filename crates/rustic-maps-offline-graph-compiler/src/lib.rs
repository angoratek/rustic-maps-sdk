//! Offline routing network graph compiler.
//!
//! This crate compiles OpenStreetMap (PBF) extracts into the binary
//! `OfflineRoutingNetworkGraph` archive: nodes,
//! directed edges with encoded speed/road-class/access columns, lane
//! guidance, instruction strings with lookup indices, and turn restrictions.
//! It is a maintainer/CI tool, never shipped inside the SDK, and stays
//! native-only: its `osmpbf` reader is not wasm-viable.
//!
//! # Crate split
//!
//! The archive *format* — reader, writer and the native `mmap` consumption
//! seam — lives in the wasm-viable `rustic-maps-offline-graph-archive` crate (see
//! its crate docs for the container format), because the SDK's
//! `OfflineRoutingProvider` consumes compiled archives as in-memory bytes on
//! every target. This crate therefore depends on it and re-exports its
//! `archive` module for produce-side path stability:
//!
//! - it holds the container format and the archive layout docs;
//! - it carries the native `mmap` reader (`mmap_reader` lives there,
//!   cfg-gated to non-wasm targets);
//! - its validating reader answers both the compiler's own fixture
//!   byte-equality guard and `rustic-maps-core`'s in-memory and mapped-file load
//!   paths.
//!
//! The compiled fixture and its guarded regeneration seam live in
//! `tests/common` here; the format itself is unchanged wherever its
//! modules physically live.

#![cfg_attr(not(test), deny(clippy::expect_used, clippy::unwrap_used))]

/// The archive format home (`rustic-maps-offline-graph-archive`), re-exported under
/// this crate's `archive` path because this compiler produces the format
/// and its docs, tests and fixture guard refer to it there.
pub use rustic_maps_offline_graph_archive::archive;

pub mod compiler;
pub mod profile;

pub use compiler::{CompilerError, compile_osm_extract_bytes, compile_osm_extract_file};
