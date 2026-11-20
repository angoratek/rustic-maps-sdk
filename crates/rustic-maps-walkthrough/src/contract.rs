//! The walkthrough contract: its JSON model, its defaults and its validation
//! rules.
//!
//! The contract names an ordered screen sequence for each of the three
//! reference platforms. Each screen points at one published image and at one
//! step in `platform/reference-flows.json`, so the walkthrough never restates a
//! flow that the reference applications already define. A hotspot on a screen
//! optionally names a transition clip and the screen it moves to.
//!
//! Validation checks the document and every file it names. It never fetches a
//! network resource: imagery is read from the repository.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::WalkthroughError;
use crate::flows::ReferenceFlows;
use crate::png::read_png_dimensions;

/// The contract schema version this build reads and validates.
pub const CONTRACT_SCHEMA_VERSION: u32 = 1;

/// The platform keys a contract must define, in published order.
pub const SUPPORTED_PLATFORMS: [&str; 3] = ["android", "ios", "react_native"];

/// The default contract path, relative to the repository root.
pub const DEFAULT_CONTRACT_PATH: &str = "platform/reference-walkthrough.json";

/// The default reference-flow path, relative to the repository root.
pub const DEFAULT_FLOWS_PATH: &str = "platform/reference-flows.json";

/// The smallest percentage a rectangle edge may use.
pub const PERCENT_MINIMUM: f64 = 0.0;

/// The largest percentage a rectangle edge may use.
pub const PERCENT_MAXIMUM: f64 = 100.0;

/// The absolute tolerance, in percentage points, when checking that a
/// rectangle's top plus height or left plus width stays inside the frame.
pub const PERCENT_SUM_TOLERANCE: f64 = 1e-6;

/// The relative tolerance when comparing an image's measured aspect ratio with
/// the ratio its platform declares. Published frames are encoded from the same
/// revision on every platform, so a one-percent band allows encoder rounding
/// without accepting a different frame shape.
pub const ASPECT_RATIO_RELATIVE_TOLERANCE: f64 = 0.01;

/// The top-level walkthrough contract.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct WalkthroughContract {
    /// The schema version; see [`CONTRACT_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// A short descriptor for the artifact.
    pub artifact: ArtifactDescriptor,
    /// One entry per supported platform, keyed by platform name.
    ///
    /// A [`BTreeMap`] keeps unknown keys visible to validation and gives the
    /// generated manifest a deterministic order.
    pub platforms: BTreeMap<String, PlatformWalkthrough>,
}

/// The artifact name and purpose printed by the validator.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ArtifactDescriptor {
    /// The artifact name.
    pub name: String,
    /// One sentence describing what the artifact shows.
    pub purpose: String,
}

/// The recorded screens for one platform.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct PlatformWalkthrough {
    /// The platform name shown by the player's platform switcher.
    pub display_name: String,
    /// The published frame shape, expressed as height divided by width.
    ///
    /// The three platforms use different device frames, so the ratio belongs
    /// to the platform rather than to the document.
    pub aspect_ratio: f64,
    /// The screens in playback order.
    pub screens: Vec<Screen>,
}

/// One recorded screen in the playback order.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Screen {
    /// The stable screen identifier used by deep links and hotspot targets.
    pub id: String,
    /// The step this screen captures, from `platform/reference-flows.json`.
    pub step_id: String,
    /// The published image, relative to the repository root.
    pub image: String,
    /// The caption shown under the phone frame.
    pub caption: String,
    /// The interactive zones on this screen. The list may be empty while the
    /// screen is still being authored.
    #[serde(default)]
    pub hotspots: Vec<Hotspot>,
}

/// One interactive zone on a screen.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Hotspot {
    /// The stable hotspot identifier, unique within its screen.
    pub id: String,
    /// The label announced by assistive technology.
    pub label: String,
    /// The pointer gesture that activates the hotspot.
    pub gesture: Gesture,
    /// The activation zone as percentages of the phone frame.
    pub rect: PercentRect,
    /// An optional transition clip played while the next screen loads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition_video: Option<String>,
    /// The screen identifier the hotspot moves to.
    pub next_screen: String,
}

/// The pointer gesture a hotspot responds to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Gesture {
    /// A tap.
    Tap,
    /// An upward swipe.
    SwipeUp,
    /// A downward swipe.
    SwipeDown,
    /// A leftward swipe.
    SwipeLeft,
    /// A rightward swipe.
    SwipeRight,
}

/// A rectangle measured as percentages of the phone frame's top-left corner.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
pub struct PercentRect {
    /// The distance from the top edge, in percent.
    pub top: f64,
    /// The distance from the left edge, in percent.
    pub left: f64,
    /// The zone width, in percent.
    pub width: f64,
    /// The zone height, in percent.
    pub height: f64,
}

/// The options for [`validate_walkthrough`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidateOptions {
    /// The contract document to validate.
    pub contract_path: PathBuf,
    /// The reference-flow document whose step identifiers the contract cites.
    pub flows_path: PathBuf,
    /// The directory that contract-relative asset paths resolve against.
    pub repository_root: PathBuf,
}

impl Default for ValidateOptions {
    fn default() -> Self {
        Self {
            contract_path: PathBuf::from(DEFAULT_CONTRACT_PATH),
            flows_path: PathBuf::from(DEFAULT_FLOWS_PATH),
            repository_root: PathBuf::from("."),
        }
    }
}

/// The counts a successful validation reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ValidationReport {
    /// The number of platforms that passed validation.
    pub platform_count: usize,
    /// The number of screens that passed validation.
    pub screen_count: usize,
    /// The number of hotspots that passed validation.
    pub hotspot_count: usize,
    /// The number of image and clip files that were found.
    pub asset_count: usize,
}

impl WalkthroughContract {
    /// Parses a contract from a JSON string.
    ///
    /// # Errors
    /// Returns [`WalkthroughError::Parse`] with the source text's line and
    /// column when the document does not match the schema.
    pub fn from_json_str(json: &str) -> Result<Self, WalkthroughError> {
        serde_json::from_str(json).map_err(|source| WalkthroughError::Parse {
            path: PathBuf::from("<memory>"),
            source,
        })
    }

    /// Reads and parses a contract from disk.
    ///
    /// # Errors
    /// Returns [`WalkthroughError::Read`] when the file cannot be read and
    /// [`WalkthroughError::Parse`] when its contents are not a valid contract.
    pub fn from_file(path: &Path) -> Result<Self, WalkthroughError> {
        crate::json::read_json_file(path)
    }
}

/// Loads both documents and validates the contract against the referenced
/// flow steps and the files on disk.
///
/// # Errors
/// Returns the first [`WalkthroughError`] that validation finds. A successful
/// call reports the number of platforms, screens, hotspots and files checked.
pub fn validate_walkthrough(
    options: &ValidateOptions,
) -> Result<ValidationReport, WalkthroughError> {
    let contract = WalkthroughContract::from_file(&options.contract_path)?;
    let flows = ReferenceFlows::from_file(&options.flows_path)?;
    validate_contract(&contract, &flows, &options.repository_root)
}

/// Validates an already parsed contract against the reference flows and the
/// repository's files.
///
/// # Errors
/// Returns [`WalkthroughError::UnknownPlatform`] for a platform key outside
/// [`SUPPORTED_PLATFORMS`], [`WalkthroughError::MissingPlatform`] when one is
/// absent, [`WalkthroughError::UnknownStep`] when a screen cites an absent
/// flow step, [`WalkthroughError::UnknownScreen`] when a hotspot targets an
/// absent screen, [`WalkthroughError::RectOutOfBounds`] when a rectangle
/// leaves the frame, [`WalkthroughError::MissingAsset`] when a named file does
/// not exist and [`WalkthroughError::AspectRatioMismatch`] when an image's
/// measured shape contradicts its platform's declared ratio.
pub fn validate_contract(
    contract: &WalkthroughContract,
    flows: &ReferenceFlows,
    repository_root: &Path,
) -> Result<ValidationReport, WalkthroughError> {
    if contract.schema_version != CONTRACT_SCHEMA_VERSION {
        return Err(WalkthroughError::UnsupportedSchemaVersion {
            found: contract.schema_version,
            expected: CONTRACT_SCHEMA_VERSION,
        });
    }

    for platform_key in contract.platforms.keys() {
        if !SUPPORTED_PLATFORMS.contains(&platform_key.as_str()) {
            return Err(WalkthroughError::UnknownPlatform {
                platform: platform_key.clone(),
                supported: SUPPORTED_PLATFORMS.join(", "),
            });
        }
    }

    for platform_key in SUPPORTED_PLATFORMS {
        if !contract.platforms.contains_key(platform_key) {
            return Err(WalkthroughError::MissingPlatform {
                platform: platform_key.to_owned(),
            });
        }
    }

    let step_ids = flows.step_ids();
    let mut report = ValidationReport::default();

    for (platform_key, platform) in &contract.platforms {
        if !platform.aspect_ratio.is_finite() || platform.aspect_ratio <= 0.0 {
            return Err(WalkthroughError::InvalidAspectRatio {
                platform: platform_key.clone(),
                aspect_ratio: platform.aspect_ratio,
            });
        }

        let mut screen_ids = BTreeSet::new();
        for screen in &platform.screens {
            if !screen_ids.insert(screen.id.as_str()) {
                return Err(WalkthroughError::DuplicateScreenId {
                    platform: platform_key.clone(),
                    screen: screen.id.clone(),
                });
            }
        }

        for screen in &platform.screens {
            if !step_ids.contains(screen.step_id.as_str()) {
                return Err(WalkthroughError::UnknownStep {
                    platform: platform_key.clone(),
                    screen: screen.id.clone(),
                    step_id: screen.step_id.clone(),
                });
            }

            let image_path = resolve_asset_path(repository_root, &screen.image)?;
            validate_image(
                platform_key,
                &platform.aspect_ratio,
                &screen.image,
                &image_path,
            )?;
            report.asset_count += 1;
            report.screen_count += 1;

            for hotspot in &screen.hotspots {
                validate_rect(platform_key, hotspot)?;
                if !screen_ids.contains(hotspot.next_screen.as_str()) {
                    return Err(WalkthroughError::UnknownScreen {
                        platform: platform_key.clone(),
                        hotspot: hotspot.id.clone(),
                        next_screen: hotspot.next_screen.clone(),
                    });
                }
                if let Some(transition_video) = &hotspot.transition_video {
                    resolve_asset_path(repository_root, transition_video)?;
                    report.asset_count += 1;
                }
                report.hotspot_count += 1;
            }
        }

        report.platform_count += 1;
    }

    Ok(report)
}

fn validate_image(
    platform: &str,
    declared_aspect_ratio: &f64,
    contract_path: &str,
    resolved_path: &Path,
) -> Result<(), WalkthroughError> {
    let bytes = std::fs::read(resolved_path).map_err(|source| WalkthroughError::Read {
        path: resolved_path.to_path_buf(),
        source,
    })?;
    let (width, height) =
        read_png_dimensions(&bytes).map_err(|reason| WalkthroughError::InvalidImage {
            path: contract_path.to_owned(),
            reason,
        })?;
    let actual_aspect_ratio = f64::from(height) / f64::from(width);
    let allowed_deviation = declared_aspect_ratio * ASPECT_RATIO_RELATIVE_TOLERANCE;
    if (actual_aspect_ratio - declared_aspect_ratio).abs() > allowed_deviation {
        return Err(WalkthroughError::AspectRatioMismatch {
            platform: platform.to_owned(),
            path: contract_path.to_owned(),
            actual: actual_aspect_ratio,
            declared: *declared_aspect_ratio,
        });
    }
    Ok(())
}

fn validate_rect(platform: &str, hotspot: &Hotspot) -> Result<(), WalkthroughError> {
    let rect = hotspot.rect;
    let values_are_finite = rect.top.is_finite()
        && rect.left.is_finite()
        && rect.width.is_finite()
        && rect.height.is_finite();
    let values_are_in_range = values_are_finite
        && rect.top >= PERCENT_MINIMUM
        && rect.left >= PERCENT_MINIMUM
        && rect.width > PERCENT_MINIMUM
        && rect.height > PERCENT_MINIMUM
        && rect.top <= PERCENT_MAXIMUM
        && rect.left <= PERCENT_MAXIMUM
        && rect.width <= PERCENT_MAXIMUM
        && rect.height <= PERCENT_MAXIMUM;
    let edges_are_in_range = values_are_in_range
        && rect.top + rect.height <= PERCENT_MAXIMUM + PERCENT_SUM_TOLERANCE
        && rect.left + rect.width <= PERCENT_MAXIMUM + PERCENT_SUM_TOLERANCE;
    if !edges_are_in_range {
        return Err(WalkthroughError::RectOutOfBounds {
            platform: platform.to_owned(),
            hotspot: hotspot.id.clone(),
            top: rect.top,
            left: rect.left,
            width: rect.width,
            height: rect.height,
        });
    }
    Ok(())
}

/// Resolves a contract-relative asset path against the repository root.
///
/// # Errors
/// Returns [`WalkthroughError::InvalidAssetPath`] for an absolute path or one
/// containing `..`, and [`WalkthroughError::MissingAsset`] when the resolved
/// path is not a regular file.
pub(crate) fn resolve_asset_path(
    repository_root: &Path,
    relative_path: &str,
) -> Result<PathBuf, WalkthroughError> {
    let candidate = Path::new(relative_path);
    let is_safe_relative_path = !relative_path.trim().is_empty()
        && !candidate.is_absolute()
        && !candidate
            .components()
            .any(|component| matches!(component, Component::ParentDir));
    if !is_safe_relative_path {
        return Err(WalkthroughError::InvalidAssetPath {
            path: relative_path.to_owned(),
        });
    }
    let resolved = repository_root.join(candidate);
    if !resolved.is_file() {
        return Err(WalkthroughError::MissingAsset {
            path: relative_path.to_owned(),
        });
    }
    Ok(resolved)
}
