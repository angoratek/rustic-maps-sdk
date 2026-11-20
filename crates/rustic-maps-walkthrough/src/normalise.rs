//! The shared walkthrough aspect ratio and the crop-or-pad transform that
//! brings every platform's stills to it.
//!
//! Android, iOS and React Native record different device frames. The
//! walkthrough plays them in step only when every platform declares the same
//! ratio, so each published still is normalised before it enters the contract:
//! the source width is preserved and the height is padded when the shared frame
//! is taller or cropped when it is shorter. The transform is recorded next to
//! the assets so the manifest states exactly what happened to every frame.
//!
//! The normalisation itself is an `ffmpeg` filter. Nothing is bundled or
//! downloaded; `ffmpeg` is a documented prerequisite.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::WalkthroughError;
use crate::ffmpeg;
use crate::png::read_png_dimensions;

/// The shared height-divided-by-width ratio every platform records at.
///
/// The plan's phone frame is 9 units wide and 19.5 units tall.
pub const DEFAULT_WALKTHROUGH_ASPECT_RATIO: f64 = 19.5 / 9.0;

/// The shared ratio in its published form.
pub const ASPECT_RATIO_DESCRIPTION: &str = "9/19.5";

/// The directory, relative to the repository root, that holds the normalised
/// walkthrough stills.
pub const NORMALISED_ASSET_DIRECTORY: &str = "crates/rustic-maps-walkthrough/assets/normalised";

/// The default colour a padded frame gains.
pub const DEFAULT_PAD_COLOR: &str = "#000000";

/// The number of hexadecimal digits in a `#rrggbb` colour.
pub const PAD_COLOR_HEX_DIGITS: usize = 6;

/// The prefix `ffmpeg`'s `pad` filter uses for a hexadecimal colour.
pub const FFMPEG_PAD_COLOR_PREFIX: &str = "0x";

/// The relative tolerance when checking a recorded transform's arithmetic and
/// the ratio its target dimensions describe.
pub const NORMALISATION_RATIO_TOLERANCE: f64 = 1e-3;

/// How one platform's stills were brought to the shared ratio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum NormalisationMode {
    /// The source already used the shared ratio.
    None,
    /// Bars were added to reach the shared ratio.
    Pad,
    /// Content was removed to reach the shared ratio.
    Crop,
}

/// The crop-or-pad choice requested on the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormalisationStrategy {
    /// Pad when the shared frame is taller than the source and crop when it is
    /// shorter.
    Auto,
    /// Always pad; rejected when the shared frame would be shorter.
    Pad,
    /// Always crop; rejected when the shared frame would be taller.
    Crop,
}

/// The exact crop or pad that produced one platform's walkthrough stills.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct NormalisationTransform {
    /// The operation applied to the source stills.
    pub mode: NormalisationMode,
    /// The published still width.
    pub source_width: u32,
    /// The published still height.
    pub source_height: u32,
    /// The walkthrough still width.
    pub target_width: u32,
    /// The walkthrough still height.
    pub target_height: u32,
    /// The horizontal position of the source inside the target frame.
    pub offset_x: u32,
    /// The vertical position of the source inside the target frame.
    pub offset_y: u32,
    /// The colour of pad bars, when the mode is [`NormalisationMode::Pad`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pad_color: Option<String>,
}

/// A platform's shared-ratio declaration: the ratio its frames use and the
/// transform that produced them.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct PlatformNormalisation {
    /// The height-divided-by-width ratio every still on this platform uses.
    pub target_aspect_ratio: f64,
    /// The crop or pad that produced those stills.
    #[serde(flatten)]
    pub transform: NormalisationTransform,
}

/// The options for [`normalise_image`].
#[derive(Debug, Clone, PartialEq)]
pub struct NormaliseOptions {
    /// The published still to transform.
    pub input: PathBuf,
    /// The normalised PNG to write.
    pub output: PathBuf,
    /// The shared height-divided-by-width ratio.
    pub target_aspect_ratio: f64,
    /// The requested crop-or-pad strategy.
    pub strategy: NormalisationStrategy,
    /// The bar colour for a padded frame, as `#rrggbb`.
    pub pad_color: String,
}

/// Computes the crop or pad that brings a source frame to the shared ratio.
///
/// The source width is preserved; the height is padded or cropped. A source
/// whose target height equals its own height yields
/// [`NormalisationMode::None`].
///
/// # Errors
/// Returns [`WalkthroughError::InvalidOption`] for a non-positive ratio, a
/// frame that cannot be expressed as pixels, or a strategy that contradicts
/// the source and target heights.
pub fn plan_normalisation(
    source_width: u32,
    source_height: u32,
    target_aspect_ratio: f64,
    strategy: NormalisationStrategy,
) -> Result<NormalisationTransform, WalkthroughError> {
    if source_width == 0 || source_height == 0 {
        return Err(invalid_option(
            "--input",
            "the source frame has a zero width or height",
        ));
    }
    if !target_aspect_ratio.is_finite() || target_aspect_ratio <= 0.0 {
        return Err(invalid_option(
            "--aspect-ratio",
            "the ratio must be a positive finite number",
        ));
    }
    let target_height = f64::from(source_width) * target_aspect_ratio;
    if !target_height.is_finite() || target_height > f64::from(u32::MAX) {
        return Err(invalid_option(
            "--aspect-ratio",
            "the ratio produces a frame taller than 4294967295 pixels",
        ));
    }
    let target_height = target_height.round() as u32;
    if target_height == 0 {
        return Err(invalid_option(
            "--aspect-ratio",
            "the ratio produces a zero-height frame",
        ));
    }

    let mode = match strategy {
        NormalisationStrategy::Auto => match target_height.cmp(&source_height) {
            std::cmp::Ordering::Greater => NormalisationMode::Pad,
            std::cmp::Ordering::Less => NormalisationMode::Crop,
            std::cmp::Ordering::Equal => NormalisationMode::None,
        },
        NormalisationStrategy::Pad => NormalisationMode::Pad,
        NormalisationStrategy::Crop => NormalisationMode::Crop,
    };

    let (offset_x, offset_y, pad_color) = match mode {
        NormalisationMode::None => {
            if target_height != source_height {
                return Err(invalid_option(
                    "--mode auto",
                    "the frame needs padding or cropping; choose `pad` or `crop`",
                ));
            }
            (0, 0, None)
        }
        NormalisationMode::Pad => {
            if target_height < source_height {
                return Err(invalid_option(
                    "--normalisation-mode",
                    "padding cannot make a taller frame shorter; choose `crop` or `auto`",
                ));
            }
            (
                0,
                (target_height - source_height) / 2,
                Some(DEFAULT_PAD_COLOR.to_owned()),
            )
        }
        NormalisationMode::Crop => {
            if target_height > source_height {
                return Err(invalid_option(
                    "--normalisation-mode",
                    "cropping cannot make a shorter frame taller; choose `pad` or `auto`",
                ));
            }
            (0, (source_height - target_height) / 2, None)
        }
    };

    Ok(NormalisationTransform {
        mode,
        source_width,
        source_height,
        target_width: source_width,
        target_height,
        offset_x,
        offset_y,
        pad_color,
    })
}

impl NormalisationTransform {
    /// Renders the `ffmpeg` filter this transform applies, when it applies one.
    ///
    /// # Errors
    /// Returns [`WalkthroughError::InvalidOption`] when a pad colour is not a
    /// `#rrggbb` value.
    pub fn filter_description(&self) -> Result<Option<String>, WalkthroughError> {
        match self.mode {
            NormalisationMode::None => Ok(None),
            NormalisationMode::Pad => {
                let color = self.pad_color.as_deref().ok_or_else(|| {
                    invalid_option("--pad-color", "a padded frame needs a bar colour")
                })?;
                let color = ffmpeg_pad_color(color)
                    .map_err(|reason| invalid_option("--pad-color", &reason))?;
                Ok(Some(format!(
                    "pad={}:{}:{}:{}:color={}",
                    self.target_width, self.target_height, self.offset_x, self.offset_y, color
                )))
            }
            NormalisationMode::Crop => Ok(Some(format!(
                "crop={}:{}:{}:{}",
                self.target_width, self.target_height, self.offset_x, self.offset_y
            ))),
        }
    }

    /// Describes the transform in one sentence for command output.
    pub fn summary(&self) -> String {
        match self.mode {
            NormalisationMode::None => format!(
                "left {}x{} unchanged because it already uses the shared ratio",
                self.source_width, self.source_height
            ),
            NormalisationMode::Pad => format!(
                "padded {}x{} to {}x{} at offset ({}, {}) with {}",
                self.source_width,
                self.source_height,
                self.target_width,
                self.target_height,
                self.offset_x,
                self.offset_y,
                self.pad_color.as_deref().unwrap_or(DEFAULT_PAD_COLOR)
            ),
            NormalisationMode::Crop => format!(
                "cropped {}x{} to {}x{} at offset ({}, {})",
                self.source_width,
                self.source_height,
                self.target_width,
                self.target_height,
                self.offset_x,
                self.offset_y
            ),
        }
    }

    /// Checks that the recorded source, target and offsets agree.
    ///
    /// # Errors
    /// Returns a human-readable reason when the arithmetic does not describe a
    /// crop or pad to these target dimensions.
    pub fn validate_geometry(&self) -> Result<(), String> {
        if self.source_width == 0 || self.source_height == 0 {
            return Err("the source frame has a zero width or height".to_owned());
        }
        if self.target_width != self.source_width {
            return Err(format!(
                "the target width {} is not the source width {}; normalisation keeps the width",
                self.target_width, self.source_width
            ));
        }
        match self.mode {
            NormalisationMode::None => {
                let unchanged = self.target_height == self.source_height
                    && self.offset_x == 0
                    && self.offset_y == 0
                    && self.pad_color.is_none();
                if !unchanged {
                    return Err(
                        "a `none` transform must keep the source dimensions and offset".to_owned(),
                    );
                }
            }
            NormalisationMode::Pad => {
                if self.target_height < self.source_height {
                    return Err("a padded frame cannot be shorter than its source".to_owned());
                }
                if self.offset_x != 0
                    || self.offset_y != (self.target_height - self.source_height) / 2
                {
                    return Err(format!(
                        "padding {}x{} to {}x{} must sit at offset (0, {})",
                        self.source_width,
                        self.source_height,
                        self.target_width,
                        self.target_height,
                        (self.target_height - self.source_height) / 2
                    ));
                }
                let Some(color) = self.pad_color.as_deref() else {
                    return Err("a padded frame must record its bar colour".to_owned());
                };
                ffmpeg_pad_color(color)?;
            }
            NormalisationMode::Crop => {
                if self.target_height > self.source_height {
                    return Err("a cropped frame cannot be taller than its source".to_owned());
                }
                if self.offset_x != 0
                    || self.offset_y != (self.source_height - self.target_height) / 2
                {
                    return Err(format!(
                        "cropping {}x{} to {}x{} must sit at offset (0, {})",
                        self.source_width,
                        self.source_height,
                        self.target_width,
                        self.target_height,
                        (self.source_height - self.target_height) / 2
                    ));
                }
                if self.pad_color.is_some() {
                    return Err("a cropped frame must not record a bar colour".to_owned());
                }
            }
        }
        Ok(())
    }
}

impl PlatformNormalisation {
    /// Checks a platform's normalisation record against one of its images and
    /// the platform's declared ratio.
    ///
    /// # Errors
    /// Returns [`WalkthroughError::NormalisationMismatch`] naming the platform
    /// and image and stating which check failed.
    pub fn validate(
        &self,
        platform: &str,
        path: &str,
        width: u32,
        height: u32,
        declared_aspect_ratio: f64,
    ) -> Result<(), WalkthroughError> {
        let mismatch = |reason: String| WalkthroughError::NormalisationMismatch {
            platform: platform.to_owned(),
            path: path.to_owned(),
            reason,
        };
        if !self.target_aspect_ratio.is_finite() || self.target_aspect_ratio <= 0.0 {
            return Err(mismatch(format!(
                "the target aspect ratio {} is not a positive finite number",
                self.target_aspect_ratio
            )));
        }
        if relative_difference(self.target_aspect_ratio, declared_aspect_ratio)
            > NORMALISATION_RATIO_TOLERANCE
        {
            return Err(mismatch(format!(
                "the recorded target ratio {:.6} differs from the platform's declared ratio {:.6}",
                self.target_aspect_ratio, declared_aspect_ratio
            )));
        }
        if self.transform.target_width != width || self.transform.target_height != height {
            return Err(mismatch(format!(
                "the image measures {width}x{height}, but the transform records {}x{}",
                self.transform.target_width, self.transform.target_height
            )));
        }
        let measured_aspect_ratio = f64::from(height) / f64::from(width);
        if relative_difference(measured_aspect_ratio, self.target_aspect_ratio)
            > NORMALISATION_RATIO_TOLERANCE
        {
            return Err(mismatch(format!(
                "the image measures ratio {measured_aspect_ratio:.6} rather than the recorded {:.6}",
                self.target_aspect_ratio
            )));
        }
        self.transform.validate_geometry().map_err(mismatch)
    }
}

/// Normalises one published PNG still to the shared ratio.
///
/// The output must be a PNG. A frame that already matches the shared ratio is
/// copied byte for byte; every other frame is filtered by `ffmpeg`.
///
/// # Errors
/// Returns [`WalkthroughError::Read`] or [`WalkthroughError::InvalidImage`]
/// when the source is not a readable PNG, [`WalkthroughError::InvalidOption`]
/// for a non-PNG output or a contradictory mode, and the `ffmpeg` failures the
/// filter reports.
pub fn normalise_image(
    options: &NormaliseOptions,
) -> Result<NormalisationTransform, WalkthroughError> {
    let source_bytes = std::fs::read(&options.input).map_err(|source| WalkthroughError::Read {
        path: options.input.clone(),
        source,
    })?;
    let (source_width, source_height) =
        read_png_dimensions(&source_bytes).map_err(|reason| WalkthroughError::InvalidImage {
            path: options.input.display().to_string(),
            reason,
        })?;
    require_png_output(&options.output)?;
    ffmpeg::ensure_parent_directory(&options.output)?;

    let mut transform = plan_normalisation(
        source_width,
        source_height,
        options.target_aspect_ratio,
        options.strategy,
    )?;
    if transform.mode == NormalisationMode::Pad {
        transform.pad_color = Some(validate_pad_color(&options.pad_color)?);
    }
    transform
        .validate_geometry()
        .map_err(|reason| invalid_option("--normalisation-mode", &reason))?;

    match transform.filter_description()? {
        Some(filter) => {
            let arguments = normalisation_arguments(&options.input, &options.output, &filter);
            ffmpeg::run(&arguments)?;
        }
        None => {
            std::fs::copy(&options.input, &options.output).map_err(|source| {
                WalkthroughError::Write {
                    path: options.output.clone(),
                    source,
                }
            })?;
        }
    }
    ffmpeg::ensure_output(&options.output)?;
    Ok(transform)
}

/// Builds the `ffmpeg` argument vector for one normalisation filter.
pub fn normalisation_arguments(input: &Path, output: &Path, filter: &str) -> Vec<String> {
    vec![
        "-y".to_owned(),
        "-i".to_owned(),
        input.display().to_string(),
        "-vf".to_owned(),
        filter.to_owned(),
        "-frames:v".to_owned(),
        "1".to_owned(),
        "-compression_level".to_owned(),
        ffmpeg::PNG_COMPRESSION_LEVEL.to_string(),
        output.display().to_string(),
    ]
}

/// Validates and lowercases a `#rrggbb` bar colour.
///
/// # Errors
/// Returns [`WalkthroughError::InvalidOption`] when the value is not a
/// six-digit hexadecimal colour with a leading `#`.
pub fn validate_pad_color(color: &str) -> Result<String, WalkthroughError> {
    let Some(hex) = color.strip_prefix('#') else {
        return Err(invalid_option(
            "--pad-color",
            "the colour must start with `#`, for example `#000000`",
        ));
    };
    let is_hex =
        hex.len() == PAD_COLOR_HEX_DIGITS && hex.chars().all(|digit| digit.is_ascii_hexdigit());
    if !is_hex {
        return Err(invalid_option(
            "--pad-color",
            "the colour must carry exactly six hexadecimal digits, for example `#000000`",
        ));
    }
    Ok(format!("#{}", hex.to_ascii_lowercase()))
}

fn ffmpeg_pad_color(color: &str) -> Result<String, String> {
    let validated = validate_pad_color(color).map_err(|error| error.to_string())?;
    let hex = validated.trim_start_matches('#');
    Ok(format!("{FFMPEG_PAD_COLOR_PREFIX}{hex}"))
}

fn relative_difference(left: f64, right: f64) -> f64 {
    if right == 0.0 {
        return f64::INFINITY;
    }
    (left - right).abs() / right.abs()
}

fn require_png_output(output: &Path) -> Result<(), WalkthroughError> {
    let is_png = output
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("png"));
    if is_png {
        Ok(())
    } else {
        Err(invalid_option(
            "--output",
            "normalisation writes PNG documents; choose a `.png` output path",
        ))
    }
}

fn invalid_option(option: &str, reason: &str) -> WalkthroughError {
    WalkthroughError::InvalidOption {
        option: option.to_owned(),
        reason: reason.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANDROID_WIDTH: u32 = 1080;
    const ANDROID_HEIGHT: u32 = 2200;
    const ANDROID_TARGET_HEIGHT: u32 = 2340;
    const IOS_WIDTH: u32 = 1206;
    const IOS_HEIGHT: u32 = 2622;
    const IOS_TARGET_HEIGHT: u32 = 2613;

    #[test]
    fn plans_a_centered_pad_for_a_frame_shorter_than_the_shared_ratio() {
        let transform = plan_normalisation(
            ANDROID_WIDTH,
            ANDROID_HEIGHT,
            DEFAULT_WALKTHROUGH_ASPECT_RATIO,
            NormalisationStrategy::Auto,
        )
        .expect("the android frame should plan");
        assert_eq!(transform.mode, NormalisationMode::Pad);
        assert_eq!(
            (transform.target_width, transform.target_height),
            (ANDROID_WIDTH, ANDROID_TARGET_HEIGHT)
        );
        assert_eq!((transform.offset_x, transform.offset_y), (0, 70));
        assert_eq!(transform.pad_color.as_deref(), Some(DEFAULT_PAD_COLOR));
        assert_eq!(
            transform
                .filter_description()
                .expect("the filter should render"),
            Some("pad=1080:2340:0:70:color=0x000000".to_owned())
        );
    }

    #[test]
    fn plans_a_centered_crop_for_a_frame_taller_than_the_shared_ratio() {
        let transform = plan_normalisation(
            IOS_WIDTH,
            IOS_HEIGHT,
            DEFAULT_WALKTHROUGH_ASPECT_RATIO,
            NormalisationStrategy::Auto,
        )
        .expect("the ios frame should plan");
        assert_eq!(transform.mode, NormalisationMode::Crop);
        assert_eq!(
            (transform.target_width, transform.target_height),
            (IOS_WIDTH, IOS_TARGET_HEIGHT)
        );
        assert_eq!((transform.offset_x, transform.offset_y), (0, 4));
        assert_eq!(
            transform
                .filter_description()
                .expect("the filter should render"),
            Some("crop=1206:2613:0:4".to_owned())
        );
    }

    #[test]
    fn leaves_a_frame_that_already_matches_unchanged() {
        let transform = plan_normalisation(
            ANDROID_WIDTH,
            ANDROID_TARGET_HEIGHT,
            DEFAULT_WALKTHROUGH_ASPECT_RATIO,
            NormalisationStrategy::Auto,
        )
        .expect("the exact frame should plan");
        assert_eq!(transform.mode, NormalisationMode::None);
        assert_eq!(
            transform
                .filter_description()
                .expect("the filter should render"),
            None
        );
    }

    #[test]
    fn rejects_a_pad_that_would_make_the_frame_shorter() {
        let result = plan_normalisation(
            IOS_WIDTH,
            IOS_HEIGHT,
            DEFAULT_WALKTHROUGH_ASPECT_RATIO,
            NormalisationStrategy::Pad,
        );
        assert!(matches!(
            result,
            Err(WalkthroughError::InvalidOption { .. })
        ));
    }

    #[test]
    fn rejects_a_non_hexadecimal_pad_colour() {
        assert!(validate_pad_color("#12345").is_err());
        assert!(validate_pad_color("000000").is_err());
        assert_eq!(
            validate_pad_color("#AABBCC").expect("the colour should validate"),
            "#aabbcc"
        );
    }
}
