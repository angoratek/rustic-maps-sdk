//! Builds a self-contained static directory from a validated contract.
//!
//! The build writes the player document with its data embedded, copies every
//! image and transition clip, and records the source commit, the contract hash
//! and each file's SHA-256 in `manifest.json`. It performs no network access.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::contract::{
    DEFAULT_CONTRACT_PATH, DEFAULT_FLOWS_PATH, Gesture, Hotspot, PercentRect, PlatformWalkthrough,
    SUPPORTED_PLATFORMS, WalkthroughContract, validate_contract,
};
use crate::error::WalkthroughError;
use crate::flows::ReferenceFlows;
use crate::manifest::{
    BuiltManifest, MANIFEST_FILE_NAME, MANIFEST_SCHEMA_VERSION, ManifestAsset, ManifestFile,
    ManifestSource, current_source_commit,
};
use crate::site::{
    PLAYER_DATA_PLACEHOLDER, PLAYER_HTML, PLAYER_HTML_FILE_NAME, PLAYER_SCRIPT,
    PLAYER_SCRIPT_FILE_NAME, PLAYER_STYLESHEET, PLAYER_STYLESHEET_FILE_NAME, byte_length,
    sha256_hex,
};

/// The default build output directory, relative to the working directory.
pub const DEFAULT_OUTPUT_DIRECTORY: &str = "tmp/walkthrough-site";

/// The directory copied assets are written under.
pub const ASSET_DIRECTORY: &str = "assets";

/// The options for [`build_site`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildOptions {
    /// The contract document to build.
    pub contract_path: PathBuf,
    /// The reference-flow document whose step identifiers the contract cites.
    pub flows_path: PathBuf,
    /// The directory that contract-relative asset paths resolve against.
    pub repository_root: PathBuf,
    /// The directory that receives the static site.
    pub output_directory: PathBuf,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            contract_path: PathBuf::from(DEFAULT_CONTRACT_PATH),
            flows_path: PathBuf::from(DEFAULT_FLOWS_PATH),
            repository_root: PathBuf::from("."),
            output_directory: PathBuf::from(DEFAULT_OUTPUT_DIRECTORY),
        }
    }
}

/// What a successful build wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildReport {
    /// The directory that now holds the site.
    pub output_directory: PathBuf,
    /// The commit the site was built from, when Git was available.
    pub source_commit: Option<String>,
    /// The number of copied image and clip files.
    pub asset_count: usize,
    /// The number of files written, including the manifest.
    pub file_count: usize,
    /// The total number of bytes written.
    pub total_bytes: u64,
}

/// The data the player reads from its embedded JSON document.
#[derive(Debug, Serialize)]
struct PlayerData {
    platform_order: Vec<&'static str>,
    platforms: BTreeMap<String, PlayerPlatform>,
}

#[derive(Debug, Serialize)]
struct PlayerPlatform {
    display_name: String,
    aspect_ratio: f64,
    screens: Vec<PlayerScreen>,
}

#[derive(Debug, Serialize)]
struct PlayerScreen {
    id: String,
    step_id: String,
    caption: String,
    image: String,
    hotspots: Vec<PlayerHotspot>,
}

#[derive(Debug, Serialize)]
struct PlayerHotspot {
    id: String,
    label: String,
    gesture: Gesture,
    rect: PercentRect,
    #[serde(skip_serializing_if = "Option::is_none")]
    transition_video: Option<String>,
    next_screen: String,
}

struct PendingAsset {
    platform: String,
    source: String,
    output: String,
    bytes: Vec<u8>,
}

struct WrittenPlayerFile {
    source: String,
    output: String,
    bytes: Vec<u8>,
}

/// Builds the static walkthrough site.
///
/// The contract and every asset are validated first. The output directory is
/// replaced, so a removed screen never leaves a stale file behind.
///
/// # Parameters
/// - `options` — the contract, flow document, repository root and output
///   directory.
///
/// # Returns
/// The output path, the source commit when Git is available, and the file and
/// byte counts.
///
/// # Errors
/// Returns the first [`WalkthroughError`] from validation, from reading an
/// asset, or from writing the output directory.
pub fn build_site(options: &BuildOptions) -> Result<BuildReport, WalkthroughError> {
    let contract = WalkthroughContract::from_file(&options.contract_path)?;
    let flows = ReferenceFlows::from_file(&options.flows_path)?;
    validate_contract(&contract, &flows, &options.repository_root)?;

    if !PLAYER_HTML.contains(PLAYER_DATA_PLACEHOLDER) {
        return Err(WalkthroughError::TemplatePlaceholderMissing {
            placeholder: PLAYER_DATA_PLACEHOLDER.to_owned(),
        });
    }

    let output_directory = &options.output_directory;
    if output_directory.exists() {
        std::fs::remove_dir_all(output_directory).map_err(|source| WalkthroughError::Write {
            path: output_directory.clone(),
            source,
        })?;
    }
    std::fs::create_dir_all(output_directory).map_err(|source| WalkthroughError::Write {
        path: output_directory.clone(),
        source,
    })?;

    let mut used_output_paths = BTreeSet::new();
    let mut pending_assets = Vec::new();
    let player_platforms = prepare_player_platforms(
        &contract,
        &options.repository_root,
        &mut used_output_paths,
        &mut pending_assets,
    )?;

    let player_data = PlayerData {
        platform_order: SUPPORTED_PLATFORMS.to_vec(),
        platforms: player_platforms,
    };
    let player_json =
        serde_json::to_string(&player_data).map_err(|source| WalkthroughError::Parse {
            path: PathBuf::from("<player-data>"),
            source,
        })?;
    // A JSON string can legally contain `</script>`; escaping `<` keeps the
    // embedded document from closing its own script element.
    let escaped_player_json = player_json.replace('<', "\\u003c");
    let index_html = PLAYER_HTML.replacen(PLAYER_DATA_PLACEHOLDER, &escaped_player_json, 1);

    let player_files = vec![
        WrittenPlayerFile {
            source: "player/index.html".to_owned(),
            output: PLAYER_HTML_FILE_NAME.to_owned(),
            bytes: index_html.into_bytes(),
        },
        WrittenPlayerFile {
            source: "player/player.css".to_owned(),
            output: PLAYER_STYLESHEET_FILE_NAME.to_owned(),
            bytes: PLAYER_STYLESHEET.as_bytes().to_vec(),
        },
        WrittenPlayerFile {
            source: "player/player.js".to_owned(),
            output: PLAYER_SCRIPT_FILE_NAME.to_owned(),
            bytes: PLAYER_SCRIPT.as_bytes().to_vec(),
        },
    ];

    let mut manifest_player = Vec::with_capacity(player_files.len());
    let mut written_bytes = 0_u64;
    for file in &player_files {
        write_bytes(&output_directory.join(&file.output), &file.bytes)?;
        written_bytes += byte_length(&file.bytes);
        manifest_player.push(ManifestFile {
            source: file.source.clone(),
            output: file.output.clone(),
            sha256: sha256_hex(&file.bytes),
            bytes: byte_length(&file.bytes),
        });
    }

    let mut manifest_assets = Vec::with_capacity(pending_assets.len());
    for asset in &pending_assets {
        write_bytes(&output_directory.join(&asset.output), &asset.bytes)?;
        written_bytes += byte_length(&asset.bytes);
        manifest_assets.push(ManifestAsset {
            platform: asset.platform.clone(),
            source: asset.source.clone(),
            output: asset.output.clone(),
            sha256: sha256_hex(&asset.bytes),
            bytes: byte_length(&asset.bytes),
        });
    }

    let contract_bytes =
        std::fs::read(&options.contract_path).map_err(|source| WalkthroughError::Read {
            path: options.contract_path.clone(),
            source,
        })?;
    let normalisation = contract
        .platforms
        .iter()
        .filter_map(|(platform_key, platform)| {
            platform
                .normalisation
                .clone()
                .map(|record| (platform_key.clone(), record))
        })
        .collect();
    let manifest = BuiltManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        artifact: contract.artifact.name.clone(),
        source_commit: current_source_commit(&options.repository_root)?,
        contract: ManifestSource {
            path: options.contract_path.display().to_string(),
            sha256: sha256_hex(&contract_bytes),
        },
        normalisation,
        player: manifest_player,
        assets: manifest_assets,
    };
    let manifest_json = manifest.to_json()?;
    write_bytes(
        &output_directory.join(MANIFEST_FILE_NAME),
        manifest_json.as_bytes(),
    )?;
    written_bytes += byte_length(manifest_json.as_bytes());

    let file_count = player_files.len() + pending_assets.len() + 1;
    Ok(BuildReport {
        output_directory: output_directory.clone(),
        source_commit: manifest.source_commit,
        asset_count: pending_assets.len(),
        file_count,
        total_bytes: written_bytes,
    })
}

fn prepare_player_platforms(
    contract: &WalkthroughContract,
    repository_root: &Path,
    used_output_paths: &mut BTreeSet<String>,
    pending_assets: &mut Vec<PendingAsset>,
) -> Result<BTreeMap<String, PlayerPlatform>, WalkthroughError> {
    let mut platforms = BTreeMap::new();
    for platform_key in SUPPORTED_PLATFORMS {
        let platform: &PlatformWalkthrough = &contract.platforms[platform_key];
        let mut screens = Vec::with_capacity(platform.screens.len());
        for screen in &platform.screens {
            let image_output = unique_output_path(used_output_paths, platform_key, &screen.image)?;
            let image_bytes = read_asset(repository_root, &screen.image)?;
            pending_assets.push(PendingAsset {
                platform: platform_key.to_owned(),
                source: screen.image.clone(),
                output: image_output.clone(),
                bytes: image_bytes,
            });

            let mut hotspots = Vec::with_capacity(screen.hotspots.len());
            for hotspot in &screen.hotspots {
                hotspots.push(player_hotspot(
                    hotspot,
                    platform_key,
                    repository_root,
                    used_output_paths,
                    pending_assets,
                )?);
            }

            screens.push(PlayerScreen {
                id: screen.id.clone(),
                step_id: screen.step_id.clone(),
                caption: screen.caption.clone(),
                image: image_output,
                hotspots,
            });
        }
        platforms.insert(
            platform_key.to_owned(),
            PlayerPlatform {
                display_name: platform.display_name.clone(),
                aspect_ratio: platform.aspect_ratio,
                screens,
            },
        );
    }
    Ok(platforms)
}

fn player_hotspot(
    hotspot: &Hotspot,
    platform_key: &str,
    repository_root: &Path,
    used_output_paths: &mut BTreeSet<String>,
    pending_assets: &mut Vec<PendingAsset>,
) -> Result<PlayerHotspot, WalkthroughError> {
    let transition_video = match &hotspot.transition_video {
        Some(source) => {
            let output = unique_output_path(used_output_paths, platform_key, source)?;
            let bytes = read_asset(repository_root, source)?;
            pending_assets.push(PendingAsset {
                platform: platform_key.to_owned(),
                source: source.clone(),
                output: output.clone(),
                bytes,
            });
            Some(output)
        }
        None => None,
    };
    Ok(PlayerHotspot {
        id: hotspot.id.clone(),
        label: hotspot.label.clone(),
        gesture: hotspot.gesture,
        rect: hotspot.rect,
        transition_video,
        next_screen: hotspot.next_screen.clone(),
    })
}

fn unique_output_path(
    used_output_paths: &mut BTreeSet<String>,
    platform_key: &str,
    source_path: &str,
) -> Result<String, WalkthroughError> {
    let file_name = Path::new(source_path)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| WalkthroughError::InvalidAssetPath {
            path: source_path.to_owned(),
        })?;
    let mut attempt = 0_u32;
    loop {
        let candidate = if attempt == 0 {
            format!("{ASSET_DIRECTORY}/{platform_key}/{file_name}")
        } else {
            format!("{ASSET_DIRECTORY}/{platform_key}/{attempt}-{file_name}")
        };
        if used_output_paths.insert(candidate.clone()) {
            return Ok(candidate);
        }
        attempt += 1;
    }
}

fn read_asset(repository_root: &Path, source_path: &str) -> Result<Vec<u8>, WalkthroughError> {
    let resolved = crate::contract::resolve_asset_path(repository_root, source_path)?;
    std::fs::read(&resolved).map_err(|source| WalkthroughError::Read {
        path: resolved,
        source,
    })
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), WalkthroughError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|source| WalkthroughError::Write {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    std::fs::write(path, bytes).map_err(|source| WalkthroughError::Write {
        path: path.to_path_buf(),
        source,
    })
}
