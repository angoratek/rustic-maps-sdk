//! Small JSON file helpers shared by the contract, flow and manifest readers.

use std::path::Path;

use serde::de::DeserializeOwned;

use crate::error::WalkthroughError;

/// Reads and parses one JSON document.
///
/// # Errors
/// Returns [`WalkthroughError::Read`] when the file cannot be read and
/// [`WalkthroughError::Parse`] when its contents do not match `T`.
pub(crate) fn read_json_file<T: DeserializeOwned>(path: &Path) -> Result<T, WalkthroughError> {
    let text = std::fs::read_to_string(path).map_err(|source| WalkthroughError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_str(&text).map_err(|source| WalkthroughError::Parse {
        path: path.to_path_buf(),
        source,
    })
}
