#![cfg_attr(not(test), deny(clippy::expect_used, clippy::unwrap_used))]
//! Captures, slices and validates the recorded product walkthrough.
//!
//! The walkthrough is a contract-driven player for the published reference
//! frames. A contract names six ordered screens for each reference platform and
//! cites the flow steps those screens capture; the builder emits one static
//! directory with the player, the copied images and a manifest of SHA-256
//! hashes. The player is dependency-free HTML, CSS and JavaScript and makes no
//! external request.
//!
//! Every recorded platform uses one shared frame ratio, so a published still
//! is cropped or padded to it and the transform is recorded next to the
//! contract and in the generated manifest.
//!
//! # Entry points
//! - [`capture`] records Android, iOS and the browser harness without a
//!   credential.
//! - [`extract_still`] and [`extract_transition`] cut media with `ffmpeg`;
//!   a transition must end on the still the contract declares next.
//! - [`normalise_image`] brings a published still to the shared ratio.
//! - [`validate_walkthrough`] checks the contract, the flow step identifiers
//!   and every asset the contract names.
//! - [`build_site`] validates the contract and writes the static directory.
//! - [`serve`] serves a built directory with the authoring overlay enabled.
//! - [`validate_manifest`] checks that a manifest's source commit exists.

mod build;
mod capture;
mod contract;
mod error;
mod ffmpeg;
mod flows;
mod json;
mod manifest;
mod normalise;
mod png;
mod serve;
mod site;
mod slice;

pub use build::{ASSET_DIRECTORY, BuildOptions, BuildReport, DEFAULT_OUTPUT_DIRECTORY, build_site};
pub use capture::{
    ANDROID_SCREEN_RECORD_BIT_RATE, BROWSER_RECORDING_DURATION_MILLISECONDS,
    BROWSER_RECORDING_WIDTH, CREDENTIAL_ENVIRONMENT_VARIABLES, CaptureCommand, CaptureOptions,
    CapturePlan, CapturePlatform, CaptureSelection, DEFAULT_HARNESS_URL,
    DEFAULT_RECORDING_DIRECTORY, KEYLESS_PROVIDER_SUMMARY, assert_no_capture_credentials, capture,
    install_instruction, plan_capture,
};
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
pub use normalise::{
    ASPECT_RATIO_DESCRIPTION, DEFAULT_PAD_COLOR, DEFAULT_WALKTHROUGH_ASPECT_RATIO,
    NORMALISATION_RATIO_TOLERANCE, NORMALISED_ASSET_DIRECTORY, NormalisationMode,
    NormalisationStrategy, NormalisationTransform, NormaliseOptions, PlatformNormalisation,
    normalisation_arguments, normalise_image, plan_normalisation, validate_pad_color,
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
pub use slice::{
    HANDOFF_CHANNEL_TOLERANCE, HANDOFF_MAXIMUM_MISMATCH_FRACTION, HandoffReport, StillOptions,
    TRANSITION_VIDEO_CODEC, TRANSITION_VIDEO_CRF, TRANSITION_VIDEO_MOVFLAGS,
    TRANSITION_VIDEO_PIXEL_FORMAT, TRANSITION_VIDEO_PRESET, TransitionOptions, compare_final_frame,
    extract_still, extract_transition, last_frame_arguments, still_arguments, still_raw_arguments,
    transition_arguments, validate_timestamp,
};
