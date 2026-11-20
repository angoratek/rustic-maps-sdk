//! Typed failures for contract validation, site building, manifest checking
//! and the local server.
//!
//! Every public entry point returns [`WalkthroughError`]; the variants name the
//! platform, screen, hotspot or file that caused the failure so a contract
//! author can fix the document without reading the implementation.

use std::path::PathBuf;

use thiserror::Error;

/// A failure while validating, building, serving or inspecting a walkthrough.
#[derive(Debug, Error)]
pub enum WalkthroughError {
    /// A file could not be read.
    #[error("failed to read `{path}`: {source}")]
    Read {
        /// The path that could not be read.
        path: PathBuf,
        /// The operating-system failure.
        #[source]
        source: std::io::Error,
    },

    /// A file could not be written.
    #[error("failed to write `{path}`: {source}")]
    Write {
        /// The path that could not be written.
        path: PathBuf,
        /// The operating-system failure.
        #[source]
        source: std::io::Error,
    },

    /// A JSON document did not match its schema.
    #[error("failed to parse `{path}` as JSON: {source}")]
    Parse {
        /// The path whose contents were rejected.
        path: PathBuf,
        /// The parser failure with its line and column.
        #[source]
        source: serde_json::Error,
    },

    /// The contract names a schema version this build does not read.
    #[error(
        "the walkthrough contract schema version {found} is not supported; this build reads version {expected}"
    )]
    UnsupportedSchemaVersion {
        /// The version recorded in the document.
        found: u32,
        /// The version this build validates.
        expected: u32,
    },

    /// The contract names a platform outside the supported set.
    #[error("unknown platform `{platform}`; the contract must describe {supported}")]
    UnknownPlatform {
        /// The platform key that is not supported.
        platform: String,
        /// The supported platform keys, separated by commas.
        supported: String,
    },

    /// One supported platform has no entry in the contract.
    #[error("platform `{platform}` is missing from the walkthrough contract")]
    MissingPlatform {
        /// The absent platform key.
        platform: String,
    },

    /// A platform declares an aspect ratio that is not a positive finite number.
    #[error("platform `{platform}` declares the invalid aspect ratio {aspect_ratio}")]
    InvalidAspectRatio {
        /// The platform key.
        platform: String,
        /// The rejected value.
        aspect_ratio: f64,
    },

    /// Two screens on one platform share an identifier.
    #[error("platform `{platform}` uses the screen identifier `{screen}` more than once")]
    DuplicateScreenId {
        /// The platform key.
        platform: String,
        /// The repeated screen identifier.
        screen: String,
    },

    /// A screen references a step that is absent from the reference flows.
    #[error(
        "screen `{screen}` on platform `{platform}` references step `{step_id}`, which is not present in the reference flows"
    )]
    UnknownStep {
        /// The platform key.
        platform: String,
        /// The screen identifier.
        screen: String,
        /// The missing step identifier.
        step_id: String,
    },

    /// A hotspot moves to a screen that the platform does not define.
    #[error("hotspot `{hotspot}` on platform `{platform}` moves to unknown screen `{next_screen}`")]
    UnknownScreen {
        /// The platform key.
        platform: String,
        /// The hotspot identifier.
        hotspot: String,
        /// The screen identifier the hotspot names.
        next_screen: String,
    },

    /// A hotspot rectangle leaves the 0-100 percent frame.
    #[error(
        "hotspot `{hotspot}` on platform `{platform}` has the rectangle top {top}, left {left}, width {width}, height {height}; every value and edge must stay within 0-100 percent"
    )]
    RectOutOfBounds {
        /// The platform key.
        platform: String,
        /// The hotspot identifier.
        hotspot: String,
        /// The declared top percentage.
        top: f64,
        /// The declared left percentage.
        left: f64,
        /// The declared width percentage.
        width: f64,
        /// The declared height percentage.
        height: f64,
    },

    /// An asset named by the contract does not exist.
    #[error("asset `{path}` does not exist relative to the repository root")]
    MissingAsset {
        /// The contract-relative asset path.
        path: String,
    },

    /// An asset path is absolute or climbs out of the repository.
    #[error("asset path `{path}` must be relative and must not contain `..`")]
    InvalidAssetPath {
        /// The rejected contract-relative path.
        path: String,
    },

    /// The image is not a PNG document with a readable header.
    #[error("image `{path}` is not a readable PNG: {reason}")]
    InvalidImage {
        /// The contract-relative image path.
        path: String,
        /// The failed header check.
        reason: String,
    },

    /// An image's measured ratio disagrees with the platform's declared ratio.
    #[error(
        "image `{path}` has aspect ratio {actual:.4}, which contradicts the {declared:.4} declared for platform `{platform}`"
    )]
    AspectRatioMismatch {
        /// The platform key.
        platform: String,
        /// The contract-relative image path.
        path: String,
        /// The measured height divided by width.
        actual: f64,
        /// The declared height divided by width.
        declared: f64,
    },

    /// The embedded player document lost its data placeholder.
    #[error("the embedded player template is missing the `{placeholder}` placeholder")]
    TemplatePlaceholderMissing {
        /// The placeholder text the builder expects.
        placeholder: String,
    },

    /// A manifest records a commit the repository cannot resolve.
    #[error(
        "manifest source commit `{commit}` is not reachable in the repository at `{repository}`"
    )]
    UnreachableSourceCommit {
        /// The commit recorded by the manifest.
        commit: String,
        /// The repository that was searched.
        repository: PathBuf,
    },

    /// The Git executable could not be run.
    #[error("failed to run git in `{repository}`: {source}")]
    Git {
        /// The repository directory.
        repository: PathBuf,
        /// The operating-system failure.
        #[source]
        source: std::io::Error,
    },

    /// Two platforms declare different aspect ratios.
    #[error(
        "platform `{platform}` declares aspect ratio {aspect_ratio:.4}, but platform `{reference_platform}` declares {reference_aspect_ratio:.4}; every platform must share one aspect ratio"
    )]
    InconsistentAspectRatio {
        /// The platform whose declaration differs.
        platform: String,
        /// Its declared height divided by width.
        aspect_ratio: f64,
        /// The first platform in the published order, whose ratio is the
        /// reference.
        reference_platform: String,
        /// The reference platform's declared height divided by width.
        reference_aspect_ratio: f64,
    },

    /// A normalisation record contradicts the image it describes or the
    /// platform that declares it.
    #[error("the normalisation recorded for platform `{platform}` contradicts `{path}`: {reason}")]
    NormalisationMismatch {
        /// The platform key.
        platform: String,
        /// The contract-relative image path.
        path: String,
        /// The failed consistency check.
        reason: String,
    },

    /// A required external program is not on `PATH`.
    #[error("`{program}` is required for this command but was not found on PATH; {install}")]
    ProgramMissing {
        /// The program name.
        program: String,
        /// The documented installation command.
        install: String,
    },

    /// An external command failed.
    #[error("`{program} {arguments}` failed with {status}: {stderr}")]
    CommandFailed {
        /// The program name.
        program: String,
        /// The argument vector, rendered for a shell.
        arguments: String,
        /// The exit status.
        status: String,
        /// The captured standard error, tail-truncated.
        stderr: String,
    },

    /// A command option value was rejected.
    #[error("invalid value for `{option}`: {reason}")]
    InvalidOption {
        /// The option name, including its leading `--`.
        option: String,
        /// Why the value was rejected.
        reason: String,
    },

    /// A command reported success but wrote no output file.
    #[error("the command reported success but did not write `{path}`")]
    OutputNotProduced {
        /// The requested output path.
        path: PathBuf,
    },

    /// A credential environment variable is set while capturing.
    #[error(
        "refusing to capture: the credential environment variable `{variable}` is set, but the recorded walkthrough is keyless; unset it and run the capture again"
    )]
    CredentialEnvironmentPresent {
        /// The offending variable name.
        variable: String,
    },

    /// A transition clip's last decoded frame disagrees with the next still.
    #[error(
        "the transition clip `{clip}` does not end on the frame in `{still}`: {mismatched_channels} of {total_channels} colour channels differ by more than {tolerance}, with a maximum channel difference of {maximum_difference}"
    )]
    FrameHandoffMismatch {
        /// The transition clip.
        clip: PathBuf,
        /// The declared next still.
        still: PathBuf,
        /// The number of channels beyond the tolerance.
        mismatched_channels: u64,
        /// The number of channels compared.
        total_channels: u64,
        /// The largest absolute channel difference.
        maximum_difference: u8,
        /// The per-channel tolerance.
        tolerance: u8,
    },

    /// The local server could not read from or write to a connection.
    #[error("walkthrough server failure: {source}")]
    Network {
        /// The operating-system failure.
        #[source]
        source: std::io::Error,
    },
}
