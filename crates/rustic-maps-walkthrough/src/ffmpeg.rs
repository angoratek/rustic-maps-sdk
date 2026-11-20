//! The `ffmpeg` seam.
//!
//! Every image and video operation invokes the `ffmpeg` binary from `PATH` with
//! an explicit argument vector. The crate never links a media library, never
//! bundles an encoder and never downloads one. When the binary is absent the
//! failure names the prerequisite and the command that installs it.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::WalkthroughError;

/// The media binary this crate invokes from `PATH`.
pub const FFMPEG_PROGRAM: &str = "ffmpeg";

/// The documented installation command for the media binary.
pub const FFMPEG_INSTALL_INSTRUCTION: &str = "install it with `brew install ffmpeg` on macOS or `apt-get install ffmpeg` on Debian or Ubuntu";

/// The PNG compression level used for extracted and normalised stills.
pub const PNG_COMPRESSION_LEVEL: u8 = 9;

/// The WebP quality used for extracted stills.
pub const WEBP_QUALITY: u8 = 90;

/// The number of trailing standard-error characters kept in a failure message.
pub const STANDARD_ERROR_TAIL_CHARACTERS: usize = 2_000;

/// Runs `ffmpeg` with `arguments` and captures its standard error.
///
/// # Errors
/// Returns [`WalkthroughError::ProgramMissing`] when the binary is absent,
/// [`WalkthroughError::CommandFailed`] when it cannot start or exits non-zero.
pub fn run(arguments: &[String]) -> Result<(), WalkthroughError> {
    let output = Command::new(FFMPEG_PROGRAM).args(arguments).output();
    match output {
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            Err(WalkthroughError::ProgramMissing {
                program: FFMPEG_PROGRAM.to_owned(),
                install: FFMPEG_INSTALL_INSTRUCTION.to_owned(),
            })
        }
        Err(source) => Err(WalkthroughError::CommandFailed {
            program: FFMPEG_PROGRAM.to_owned(),
            arguments: render_arguments(arguments),
            status: "could not start".to_owned(),
            stderr: source.to_string(),
        }),
        Ok(output) if !output.status.success() => Err(WalkthroughError::CommandFailed {
            program: FFMPEG_PROGRAM.to_owned(),
            arguments: render_arguments(arguments),
            status: output.status.to_string(),
            stderr: standard_error_tail(&output.stderr),
        }),
        Ok(_) => Ok(()),
    }
}

/// Fails when a command reported success but wrote no output.
///
/// # Errors
/// Returns [`WalkthroughError::OutputNotProduced`] when `path` is not a file.
pub fn ensure_output(path: &Path) -> Result<(), WalkthroughError> {
    if path.is_file() {
        Ok(())
    } else {
        Err(WalkthroughError::OutputNotProduced {
            path: path.to_path_buf(),
        })
    }
}

/// Creates the parent directory of an output path when it is missing.
///
/// # Errors
/// Returns [`WalkthroughError::Write`] when the directory cannot be created.
pub fn ensure_parent_directory(path: &Path) -> Result<(), WalkthroughError> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    if parent.as_os_str().is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(parent).map_err(|source| WalkthroughError::Write {
        path: parent.to_path_buf(),
        source,
    })
}

/// Renders a command for a terminal or a dry-run report.
///
/// Arguments with only shell-safe characters are printed verbatim; every other
/// argument is single-quoted.
pub fn render_command(program: &str, arguments: &[String]) -> String {
    if arguments.is_empty() {
        return program.to_owned();
    }
    format!("{program} {}", render_arguments(arguments))
}

/// Renders a media binary failure for an external command.
///
/// # Errors
/// Returns [`WalkthroughError::CommandFailed`] with the captured standard
/// error.
pub fn failed_command(
    program: &str,
    arguments: &[String],
    status: std::process::ExitStatus,
    standard_error: &[u8],
) -> WalkthroughError {
    WalkthroughError::CommandFailed {
        program: program.to_owned(),
        arguments: render_arguments(arguments),
        status: status.to_string(),
        stderr: standard_error_tail(standard_error),
    }
}

/// Returns a unique scratch path for an intermediate decode.
pub fn scratch_path(label: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "rustic-maps-walkthrough-{}-{unique}-{label}",
        std::process::id()
    ))
}

fn render_arguments(arguments: &[String]) -> String {
    arguments
        .iter()
        .map(|argument| quote_argument(argument))
        .collect::<Vec<_>>()
        .join(" ")
}

fn quote_argument(argument: &str) -> String {
    let is_safe = !argument.is_empty()
        && argument
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_./:=+,@%".contains(character));
    if is_safe {
        argument.to_owned()
    } else {
        format!("'{}'", argument.replace('\'', "'\\''"))
    }
}

fn standard_error_tail(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let character_count = text.chars().count();
    if character_count <= STANDARD_ERROR_TAIL_CHARACTERS {
        return text.into_owned();
    }
    let tail: String = text
        .chars()
        .skip(character_count - STANDARD_ERROR_TAIL_CHARACTERS)
        .collect();
    format!("[truncated] {tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_plain_and_quoted_arguments() {
        let arguments = vec!["-i".to_owned(), "a b.mp4".to_owned()];
        assert_eq!(render_command("ffmpeg", &arguments), "ffmpeg -i 'a b.mp4'");
    }

    #[test]
    fn keeps_a_short_standard_error_intact() {
        assert_eq!(standard_error_tail(b"boom"), "boom");
    }
}
