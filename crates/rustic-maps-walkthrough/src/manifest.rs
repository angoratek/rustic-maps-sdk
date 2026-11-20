//! The manifest a build writes, and the source-commit reachability check.
//!
//! The manifest records the commit the site was built from, the contract's
//! hash, and the hash of every player file and copied asset. Validation
//! resolves the recorded commit with Git; when Git or a repository is absent,
//! for example in an extracted copy of the crate, the check is skipped with a
//! report note instead of failing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::error::WalkthroughError;
use crate::normalise::PlatformNormalisation;

/// The manifest schema version this build writes and reads.
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

/// The manifest file name inside the built directory.
pub const MANIFEST_FILE_NAME: &str = "manifest.json";

/// The manifest a build writes into the output directory.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct BuiltManifest {
    /// The manifest schema version.
    pub schema_version: u32,
    /// The artifact name from the contract.
    pub artifact: String,
    /// The commit the site was built from, when Git was available.
    pub source_commit: Option<String>,
    /// The contract file the site was built from, with its hash.
    pub contract: ManifestSource,
    /// The crop or pad that produced each platform's walkthrough stills.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub normalisation: BTreeMap<String, PlatformNormalisation>,
    /// The player files written by the build.
    pub player: Vec<ManifestFile>,
    /// The copied image and clip files, in build order.
    pub assets: Vec<ManifestAsset>,
}

/// A source document and the hash of its contents.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ManifestSource {
    /// The source path relative to the repository root, when possible.
    pub path: String,
    /// The SHA-256 of the source bytes as lowercase hexadecimal.
    pub sha256: String,
}

/// One generated player file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ManifestFile {
    /// The embedded source name.
    pub source: String,
    /// The path inside the built directory.
    pub output: String,
    /// The SHA-256 of the written bytes as lowercase hexadecimal.
    pub sha256: String,
    /// The written byte count.
    pub bytes: u64,
}

/// One copied asset.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ManifestAsset {
    /// The platform key the asset belongs to.
    pub platform: String,
    /// The contract-relative source path.
    pub source: String,
    /// The path inside the built directory.
    pub output: String,
    /// The SHA-256 of the copied bytes as lowercase hexadecimal.
    pub sha256: String,
    /// The copied byte count.
    pub bytes: u64,
}

/// The outcome of validating a manifest against a repository.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ManifestReport {
    /// Whether the recorded commit was resolved in the repository.
    pub source_commit_checked: bool,
    /// Human-readable notes, including a skipped reachability check.
    pub notes: Vec<String>,
}

impl BuiltManifest {
    /// Reads a manifest from disk.
    ///
    /// # Errors
    /// Returns [`WalkthroughError::Read`] when the file cannot be read and
    /// [`WalkthroughError::Parse`] when it is not valid JSON.
    pub fn from_file(path: &Path) -> Result<Self, WalkthroughError> {
        crate::json::read_json_file(path)
    }

    /// Serializes the manifest as pretty JSON with a trailing newline.
    ///
    /// # Errors
    /// Returns [`WalkthroughError::Parse`] when the value cannot be serialized.
    pub fn to_json(&self) -> Result<String, WalkthroughError> {
        let mut json =
            serde_json::to_string_pretty(self).map_err(|source| WalkthroughError::Parse {
                path: PathBuf::from("<manifest>"),
                source,
            })?;
        json.push('\n');
        Ok(json)
    }
}

/// Checks that the manifest's recorded source commit is reachable.
///
/// # Errors
/// Returns [`WalkthroughError::UnreachableSourceCommit`] when the repository
/// resolves Git but not the commit, and [`WalkthroughError::Git`] when Git
/// fails for an operating-system reason.
pub fn validate_manifest(
    manifest: &BuiltManifest,
    repository_root: &Path,
) -> Result<ManifestReport, WalkthroughError> {
    if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
        return Err(WalkthroughError::UnsupportedSchemaVersion {
            found: manifest.schema_version,
            expected: MANIFEST_SCHEMA_VERSION,
        });
    }

    let mut report = ManifestReport::default();
    let Some(commit) = manifest.source_commit.as_deref() else {
        report.notes.push(
            "the manifest records no source commit, so reachability was not checked".to_owned(),
        );
        return Ok(report);
    };

    if !repository_is_available(repository_root)? {
        report.notes.push(format!(
            "source commit `{commit}` was not checked because `{}` is not a Git repository or git is unavailable",
            repository_root.display()
        ));
        return Ok(report);
    }

    if !commit_is_reachable(repository_root, commit)? {
        return Err(WalkthroughError::UnreachableSourceCommit {
            commit: commit.to_owned(),
            repository: repository_root.to_path_buf(),
        });
    }

    report.source_commit_checked = true;
    report
        .notes
        .push(format!("source commit `{commit}` is reachable"));
    Ok(report)
}

/// Reads `HEAD` from a repository, if one is available.
///
/// # Returns
/// The full commit identifier, or `None` when Git is unavailable or the
/// directory is not a working tree.
///
/// # Errors
/// Returns [`WalkthroughError::Git`] when Git runs but cannot start.
pub fn current_source_commit(repository_root: &Path) -> Result<Option<String>, WalkthroughError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository_root)
        .args(["rev-parse", "--verify", "HEAD"])
        .output();
    match output {
        Ok(output) if output.status.success() => {
            let commit = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            Ok((!commit.is_empty()).then_some(commit))
        }
        Ok(_) => Ok(None),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(WalkthroughError::Git {
            repository: repository_root.to_path_buf(),
            source,
        }),
    }
}

fn repository_is_available(repository_root: &Path) -> Result<bool, WalkthroughError> {
    run_git(repository_root, &["rev-parse", "--git-dir"])
}

fn commit_is_reachable(repository_root: &Path, commit: &str) -> Result<bool, WalkthroughError> {
    let commit_reference = format!("{commit}^{{commit}}");
    run_git(repository_root, &["cat-file", "-e", &commit_reference])
}

fn run_git(repository_root: &Path, arguments: &[&str]) -> Result<bool, WalkthroughError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository_root)
        .args(arguments)
        .output();
    match output {
        Ok(output) => Ok(output.status.success()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(WalkthroughError::Git {
            repository: repository_root.to_path_buf(),
            source,
        }),
    }
}
