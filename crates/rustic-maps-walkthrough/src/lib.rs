#![cfg_attr(not(test), deny(clippy::expect_used, clippy::unwrap_used))]
//! Validates, builds and serves the recorded product walkthrough.
//!
//! The walkthrough is a contract-driven player for the published reference
//! frames. A contract names six ordered screens for each reference platform and
//! cites the flow steps those screens capture; the builder emits one static
//! directory with the player, the copied images and a manifest of SHA-256
//! hashes. The player is dependency-free HTML, CSS and JavaScript and makes no
//! external request.
//!
//! # Entry points
//! - [`validate_walkthrough`] checks the contract, the flow step identifiers
//!   and every asset the contract names.
//! - [`build_site`] validates the contract and writes the static directory.
//! - [`serve`] serves a built directory with the authoring overlay enabled.
//! - [`validate_manifest`] checks that a manifest's source commit exists.

mod build;
mod contract;
mod error;
mod flows;
mod json;
mod manifest;
mod png;
mod serve;
mod site;

pub use build::{ASSET_DIRECTORY, BuildOptions, BuildReport, DEFAULT_OUTPUT_DIRECTORY, build_site};
pub use contract::{
    ASPECT_RATIO_RELATIVE_TOLERANCE, ArtifactDescriptor, CONTRACT_SCHEMA_VERSION,
    DEFAULT_CONTRACT_PATH, DEFAULT_FLOWS_PATH, Gesture, Hotspot, PERCENT_MAXIMUM, PERCENT_MINIMUM,
    PERCENT_SUM_TOLERANCE, PercentRect, PlatformWalkthrough, SUPPORTED_PLATFORMS, Screen,
    ValidateOptions, ValidationReport, WalkthroughContract, validate_contract,
    validate_walkthrough,
};
pub use error::WalkthroughError;
pub use flows::{Flow, FlowStep, ReferenceFlows};
pub use manifest::{
    BuiltManifest, MANIFEST_FILE_NAME, MANIFEST_SCHEMA_VERSION, ManifestAsset, ManifestFile,
    ManifestReport, ManifestSource, current_source_commit, validate_manifest,
};
pub use png::read_png_dimensions;
pub use serve::{
    DEFAULT_SERVE_ADDRESS, SERVER_POLL_INTERVAL_MILLISECONDS, ServeOptions, bind, serve, serve_on,
    serve_with_shutdown,
};
pub use site::{
    AUTHORING_SCRIPT, AUTHORING_SCRIPT_ROUTE, PLAYER_DATA_PLACEHOLDER, PLAYER_HTML,
    PLAYER_HTML_FILE_NAME, PLAYER_SCRIPT, PLAYER_SCRIPT_FILE_NAME, PLAYER_STYLESHEET,
    PLAYER_STYLESHEET_FILE_NAME, byte_length, sha256_hex,
};
