//! The player files embedded in the binary, and the SHA-256 helper the build
//! uses for the manifest.
//!
//! The player is dependency-free HTML, CSS and JavaScript. The production
//! build writes only these files; the authoring overlay lives in a separate
//! script that the local server injects and is never copied into the built
//! directory.

use sha2::{Digest, Sha256};

/// The player document, with a placeholder for the generated walkthrough data.
pub const PLAYER_HTML: &str = include_str!("../player/index.html");

/// The player stylesheet.
pub const PLAYER_STYLESHEET: &str = include_str!("../player/player.css");

/// The player behavior.
pub const PLAYER_SCRIPT: &str = include_str!("../player/player.js");

/// The development-only hotspot authoring overlay.
pub const AUTHORING_SCRIPT: &str = include_str!("../player/authoring.js");

/// The file name the player document is written under.
pub const PLAYER_HTML_FILE_NAME: &str = "index.html";

/// The file name the stylesheet is written under.
pub const PLAYER_STYLESHEET_FILE_NAME: &str = "player.css";

/// The file name the player behavior is written under.
pub const PLAYER_SCRIPT_FILE_NAME: &str = "player.js";

/// The route the local server serves the authoring overlay from.
pub const AUTHORING_SCRIPT_ROUTE: &str = "/authoring.js";

/// The placeholder the builder replaces with the serialized walkthrough data.
pub const PLAYER_DATA_PLACEHOLDER: &str = "__WALKTHROUGH_DATA__";

/// Returns the SHA-256 of `bytes` as 64 lowercase hexadecimal digits.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        encoded.push_str(&format!("{byte:02x}"));
    }
    encoded
}

/// Returns the byte length of `bytes` as a `u64`.
pub fn byte_length(bytes: &[u8]) -> u64 {
    u64::try_from(bytes.len()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_an_empty_document() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn hashes_a_known_document() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
