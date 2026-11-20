//! Extracts stills and frame-accurate transition clips with `ffmpeg`.
//!
//! A still is decoded from an exact timestamp with the seek placed after the
//! input, so the decoder produces a true frame rather than the previous
//! keyframe. A transition clip uses the same output-side seek, which makes the
//! cut frame-accurate instead of keyframe-accurate.
//!
//! Every transition is checked against the still the contract declares next:
//! the clip's last decoded frame is decoded to raw RGB and compared with the
//! declared still channel by channel. A clip whose final frame disagrees stops
//! the build with both paths and the measured difference, so the player can
//! never swap to a still the clip does not reach.

use std::path::{Path, PathBuf};

use crate::error::WalkthroughError;
use crate::ffmpeg;

/// The codec every transition clip is encoded with.
pub const TRANSITION_VIDEO_CODEC: &str = "libx264";

/// The constant-rate factor every transition clip is encoded at. A lower
/// value writes more bits; 20 keeps text and map lines crisp at phone size.
pub const TRANSITION_VIDEO_CRF: u8 = 20;

/// The encoder preset every transition clip uses.
pub const TRANSITION_VIDEO_PRESET: &str = "slow";

/// The pixel format every transition clip writes, so a browser decodes it.
pub const TRANSITION_VIDEO_PIXEL_FORMAT: &str = "yuv420p";

/// The container flag that lets a transition clip start before it is fully
/// downloaded.
pub const TRANSITION_VIDEO_MOVFLAGS: &str = "+faststart";

/// The only container a transition clip may be written as.
pub const TRANSITION_OUTPUT_EXTENSION: &str = "mp4";

/// The tail, measured back from the clip end, that is decoded to reach the
/// clip's final frame.
pub const HANDOFF_TAIL_SECONDS: &str = "1";

/// The per-channel absolute difference a comparison tolerates before counting
/// a channel as different. Re-encoding shifts values slightly.
pub const HANDOFF_CHANNEL_TOLERANCE: u8 = 3;

/// The share of colour channels that may differ inside the tolerance band
/// before the final frame is rejected.
pub const HANDOFF_MAXIMUM_MISMATCH_FRACTION: f64 = 0.005;

/// The raw pixel format used for the handoff comparison.
pub const HANDOFF_PIXEL_FORMAT: &str = "rgb24";

/// The options for [`extract_still`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StillOptions {
    /// The recording to read.
    pub input: PathBuf,
    /// The `.png` or `.webp` still to write.
    pub output: PathBuf,
    /// The timestamp to decode, as seconds or `HH:MM:SS.mmm`.
    pub timestamp: String,
}

/// The options for [`extract_transition`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionOptions {
    /// The recording to read.
    pub input: PathBuf,
    /// The `.mp4` clip to write.
    pub output: PathBuf,
    /// The first timestamp of the clip.
    pub start: String,
    /// The timestamp the clip stops at.
    pub end: String,
    /// The still the contract declares next.
    pub next_still: PathBuf,
}

/// The measured agreement between a clip's final frame and the next still.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandoffReport {
    /// The number of colour channels within the tolerance.
    pub matching_channels: u64,
    /// The number of colour channels compared.
    pub total_channels: u64,
    /// The largest absolute channel difference.
    pub maximum_difference: u8,
}

/// Extracts one still at an exact timestamp.
///
/// # Errors
/// Returns [`WalkthroughError::InvalidOption`] for an unsupported timestamp or
/// output format, [`WalkthroughError::Write`] when the output directory cannot
/// be created, the `ffmpeg` failures the decode reports, and
/// [`WalkthroughError::OutputNotProduced`] when no still was written.
pub fn extract_still(options: &StillOptions) -> Result<(), WalkthroughError> {
    let arguments = still_arguments(&options.input, &options.output, &options.timestamp)?;
    ffmpeg::ensure_parent_directory(&options.output)?;
    ffmpeg::run(&arguments)?;
    ffmpeg::ensure_output(&options.output)
}

/// Extracts a transition clip and checks its final frame against the next
/// still.
///
/// # Errors
/// Returns [`WalkthroughError::InvalidOption`] for an unsupported timestamp or
/// output format, [`WalkthroughError::Write`] when the output directory cannot
/// be created, the `ffmpeg` failures the encode or the comparison decodes
/// report, [`WalkthroughError::OutputNotProduced`] when no clip was written,
/// and [`WalkthroughError::FrameHandoffMismatch`] when the clip does not end on
/// the declared still.
pub fn extract_transition(options: &TransitionOptions) -> Result<HandoffReport, WalkthroughError> {
    let arguments = transition_arguments(
        &options.input,
        &options.output,
        &options.start,
        &options.end,
    )?;
    ffmpeg::ensure_parent_directory(&options.output)?;
    ffmpeg::run(&arguments)?;
    ffmpeg::ensure_output(&options.output)?;

    let last_frame_path = ffmpeg::scratch_path("handoff-last-frame.rgb");
    let next_still_path = ffmpeg::scratch_path("handoff-next-still.rgb");
    let result = compare_with_next_still(
        &options.output,
        &options.next_still,
        &last_frame_path,
        &next_still_path,
    );
    let _ = std::fs::remove_file(&last_frame_path);
    let _ = std::fs::remove_file(&next_still_path);
    result
}

/// Builds the argument vector for a still extraction.
///
/// `-ss` is placed after `-i` so `ffmpeg` decodes to the requested frame
/// instead of seeking to the preceding keyframe.
///
/// # Errors
/// Returns [`WalkthroughError::InvalidOption`] for a rejected timestamp or an
/// output that is neither PNG nor WebP.
pub fn still_arguments(
    input: &Path,
    output: &Path,
    timestamp: &str,
) -> Result<Vec<String>, WalkthroughError> {
    validate_timestamp("--timestamp", timestamp)?;
    let quality = still_quality_arguments(output)?;
    let mut arguments = vec![
        "-y".to_owned(),
        "-i".to_owned(),
        input.display().to_string(),
        "-ss".to_owned(),
        timestamp.to_owned(),
        "-frames:v".to_owned(),
        "1".to_owned(),
    ];
    arguments.extend(quality);
    arguments.push(output.display().to_string());
    Ok(arguments)
}

/// Builds the argument vector for a transition clip.
///
/// `-ss` follows `-i` so the cut is frame-accurate, `-an` drops the audio, and
/// the H.264 encoder writes a Web-ready progressive stream.
///
/// # Errors
/// Returns [`WalkthroughError::InvalidOption`] for a rejected timestamp or an
/// output that is not `.mp4`.
pub fn transition_arguments(
    input: &Path,
    output: &Path,
    start: &str,
    end: &str,
) -> Result<Vec<String>, WalkthroughError> {
    validate_timestamp("--start", start)?;
    validate_timestamp("--end", end)?;
    require_extension(output, TRANSITION_OUTPUT_EXTENSION, "--output")?;
    Ok(vec![
        "-y".to_owned(),
        "-i".to_owned(),
        input.display().to_string(),
        "-ss".to_owned(),
        start.to_owned(),
        "-to".to_owned(),
        end.to_owned(),
        "-an".to_owned(),
        "-c:v".to_owned(),
        TRANSITION_VIDEO_CODEC.to_owned(),
        "-crf".to_owned(),
        TRANSITION_VIDEO_CRF.to_string(),
        "-preset".to_owned(),
        TRANSITION_VIDEO_PRESET.to_owned(),
        "-pix_fmt".to_owned(),
        TRANSITION_VIDEO_PIXEL_FORMAT.to_owned(),
        "-movflags".to_owned(),
        TRANSITION_VIDEO_MOVFLAGS.to_owned(),
        output.display().to_string(),
    ])
}

/// Builds the argument vector that decodes the clip's tail to raw RGB.
///
/// `-sseof` reads backwards from the end; every frame in the tail is written,
/// so the final frame is the last `width * height * 3` bytes of the output.
pub fn last_frame_arguments(clip: &Path, output: &Path) -> Vec<String> {
    vec![
        "-y".to_owned(),
        "-sseof".to_owned(),
        format!("-{HANDOFF_TAIL_SECONDS}"),
        "-i".to_owned(),
        clip.display().to_string(),
        "-an".to_owned(),
        "-f".to_owned(),
        "rawvideo".to_owned(),
        "-pix_fmt".to_owned(),
        HANDOFF_PIXEL_FORMAT.to_owned(),
        output.display().to_string(),
    ]
}

/// Builds the argument vector that decodes the next still to raw RGB.
pub fn still_raw_arguments(still: &Path, output: &Path) -> Vec<String> {
    vec![
        "-y".to_owned(),
        "-i".to_owned(),
        still.display().to_string(),
        "-frames:v".to_owned(),
        "1".to_owned(),
        "-an".to_owned(),
        "-f".to_owned(),
        "rawvideo".to_owned(),
        "-pix_fmt".to_owned(),
        HANDOFF_PIXEL_FORMAT.to_owned(),
        output.display().to_string(),
    ]
}

/// Compares the final frame of a decoded clip tail with a decoded still.
///
/// A channel matches when its absolute difference is at most
/// [`HANDOFF_CHANNEL_TOLERANCE`]. The frames match when no more than
/// [`HANDOFF_MAXIMUM_MISMATCH_FRACTION`] of the channels differ.
pub fn compare_final_frame(clip_raw: &[u8], still_raw: &[u8]) -> FrameComparison {
    if still_raw.is_empty() {
        return FrameComparison {
            mismatched_channels: 0,
            total_channels: 0,
            maximum_difference: 0,
            matches: false,
        };
    }
    let total_channels = still_raw.len() as u64;
    if clip_raw.len() < still_raw.len() {
        return FrameComparison {
            mismatched_channels: total_channels,
            total_channels,
            maximum_difference: u8::MAX,
            matches: false,
        };
    }
    let final_frame = &clip_raw[clip_raw.len() - still_raw.len()..];
    let mut mismatched_channels = 0_u64;
    let mut maximum_difference = 0_u8;
    for (clip_channel, still_channel) in final_frame.iter().zip(still_raw.iter()) {
        let difference = clip_channel.abs_diff(*still_channel);
        maximum_difference = maximum_difference.max(difference);
        if difference > HANDOFF_CHANNEL_TOLERANCE {
            mismatched_channels += 1;
        }
    }
    let mismatched_fraction = mismatched_channels as f64 / total_channels as f64;
    FrameComparison {
        mismatched_channels,
        total_channels,
        maximum_difference,
        matches: mismatched_fraction <= HANDOFF_MAXIMUM_MISMATCH_FRACTION,
    }
}

/// The result of comparing a clip's final frame with a still.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameComparison {
    /// The number of channels beyond the tolerance.
    pub mismatched_channels: u64,
    /// The number of channels compared.
    pub total_channels: u64,
    /// The largest absolute channel difference.
    pub maximum_difference: u8,
    /// Whether the mismatch stayed inside the accepted band.
    pub matches: bool,
}

/// Validates a timestamp passed to `ffmpeg`'s `-ss`, `-to` or `-sseof`.
///
/// # Errors
/// Returns [`WalkthroughError::InvalidOption`] when the value is empty or
/// carries anything other than digits, `.` and `:`.
pub fn validate_timestamp(option: &str, value: &str) -> Result<(), WalkthroughError> {
    let is_valid = !value.trim().is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_digit() || character == '.' || character == ':');
    if is_valid {
        Ok(())
    } else {
        Err(WalkthroughError::InvalidOption {
            option: option.to_owned(),
            reason: "use seconds (`12.5`) or `HH:MM:SS.mmm`; the value may contain only digits, `.` and `:`"
                .to_owned(),
        })
    }
}

fn compare_with_next_still(
    clip: &Path,
    next_still: &Path,
    last_frame_path: &Path,
    next_still_path: &Path,
) -> Result<HandoffReport, WalkthroughError> {
    ffmpeg::run(&last_frame_arguments(clip, last_frame_path))?;
    ffmpeg::run(&still_raw_arguments(next_still, next_still_path))?;
    let clip_raw = std::fs::read(last_frame_path).map_err(|source| WalkthroughError::Read {
        path: last_frame_path.to_path_buf(),
        source,
    })?;
    let still_raw = std::fs::read(next_still_path).map_err(|source| WalkthroughError::Read {
        path: next_still_path.to_path_buf(),
        source,
    })?;
    if still_raw.is_empty() {
        return Err(WalkthroughError::InvalidOption {
            option: "--next-still".to_owned(),
            reason: format!(
                "`{}` produced no pixels; check that it is a readable image",
                next_still.display()
            ),
        });
    }
    let comparison = compare_final_frame(&clip_raw, &still_raw);
    if !comparison.matches {
        return Err(WalkthroughError::FrameHandoffMismatch {
            clip: clip.to_path_buf(),
            still: next_still.to_path_buf(),
            mismatched_channels: comparison.mismatched_channels,
            total_channels: comparison.total_channels,
            maximum_difference: comparison.maximum_difference,
            tolerance: HANDOFF_CHANNEL_TOLERANCE,
        });
    }
    Ok(HandoffReport {
        matching_channels: comparison
            .total_channels
            .saturating_sub(comparison.mismatched_channels),
        total_channels: comparison.total_channels,
        maximum_difference: comparison.maximum_difference,
    })
}

fn still_quality_arguments(output: &Path) -> Result<Vec<String>, WalkthroughError> {
    let extension = output
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("png") => Ok(vec![
            "-compression_level".to_owned(),
            ffmpeg::PNG_COMPRESSION_LEVEL.to_string(),
        ]),
        Some("webp") => Ok(vec![
            "-quality".to_owned(),
            ffmpeg::WEBP_QUALITY.to_string(),
        ]),
        _ => Err(WalkthroughError::InvalidOption {
            option: "--output".to_owned(),
            reason: "extract a still as `.png` or `.webp`".to_owned(),
        }),
    }
}

fn require_extension(path: &Path, expected: &str, option: &str) -> Result<(), WalkthroughError> {
    let matches = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(expected));
    if matches {
        Ok(())
    } else {
        Err(WalkthroughError::InvalidOption {
            option: option.to_owned(),
            reason: format!("write the transition clip as `.{expected}`"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn position(arguments: &[String], option: &str) -> usize {
        arguments
            .iter()
            .position(|argument| argument == option)
            .unwrap_or_else(|| panic!("`{option}` should be present in {arguments:?}"))
    }

    #[test]
    fn seeks_the_still_after_the_input() {
        let arguments = still_arguments(Path::new("recording.mp4"), Path::new("still.png"), "2.5")
            .expect("the still arguments should build");
        assert!(position(&arguments, "-i") < position(&arguments, "-ss"));
        assert_eq!(arguments[position(&arguments, "-ss") + 1], "2.5");
        assert_eq!(arguments[position(&arguments, "-frames:v") + 1], "1");
        assert_eq!(
            arguments[position(&arguments, "-compression_level") + 1],
            "9"
        );
    }

    #[test]
    fn seeks_the_transition_after_the_input_and_faststarts_it() {
        let arguments =
            transition_arguments(Path::new("recording.mp4"), Path::new("clip.mp4"), "1", "3")
                .expect("the transition arguments should build");
        assert!(position(&arguments, "-i") < position(&arguments, "-ss"));
        assert!(position(&arguments, "-ss") < position(&arguments, "-to"));
        assert_eq!(arguments[position(&arguments, "-to") + 1], "3");
        assert!(arguments.contains(&"-an".to_owned()));
        assert_eq!(
            arguments[position(&arguments, "-c:v") + 1],
            TRANSITION_VIDEO_CODEC
        );
        assert_eq!(
            arguments[position(&arguments, "-crf") + 1],
            TRANSITION_VIDEO_CRF.to_string()
        );
        assert_eq!(
            arguments[position(&arguments, "-preset") + 1],
            TRANSITION_VIDEO_PRESET
        );
        assert_eq!(
            arguments[position(&arguments, "-pix_fmt") + 1],
            TRANSITION_VIDEO_PIXEL_FORMAT
        );
        assert_eq!(
            arguments[position(&arguments, "-movflags") + 1],
            TRANSITION_VIDEO_MOVFLAGS
        );
    }

    #[test]
    fn decodes_the_last_frame_with_sseof() {
        let arguments = last_frame_arguments(Path::new("clip.mp4"), Path::new("frame.rgb"));
        assert_eq!(arguments[0], "-y");
        assert_eq!(arguments[1], "-sseof");
        assert_eq!(arguments[2], "-1");
        assert!(position(&arguments, "-sseof") < position(&arguments, "-i"));
    }

    #[test]
    fn rejects_a_timestamp_that_could_be_read_as_an_option() {
        assert!(validate_timestamp("--start", "-1").is_err());
        assert!(validate_timestamp("--start", "00:00:01.500").is_ok());
    }

    #[test]
    fn accepts_an_identical_final_frame() {
        let frame = vec![10_u8, 20, 30, 40, 50, 60];
        let comparison = compare_final_frame(&frame, &frame);
        assert!(comparison.matches);
        assert_eq!(comparison.mismatched_channels, 0);
        assert_eq!(comparison.maximum_difference, 0);
    }

    #[test]
    fn rejects_a_different_final_frame() {
        let clip = vec![0_u8; 300];
        let still = vec![255_u8; 300];
        let comparison = compare_final_frame(&clip, &still);
        assert!(!comparison.matches);
        assert_eq!(comparison.mismatched_channels, 300);
        assert_eq!(comparison.maximum_difference, 255);
    }

    #[test]
    fn compares_only_the_last_frame_of_the_tail() {
        let mut clip = vec![255_u8; 300];
        clip.extend_from_slice(&[10_u8, 20, 30]);
        let still = vec![10_u8, 20, 30];
        let comparison = compare_final_frame(&clip, &still);
        assert!(comparison.matches);
        assert_eq!(comparison.total_channels, 3);
    }
}
