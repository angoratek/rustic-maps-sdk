//! Records the walkthrough on Android, iOS and the browser.
//!
//! Capture is two steps. This module builds and runs the recording step: one
//! command sequence per platform, written to a directory of raw recordings.
//! [`crate::slice`] then cuts stills and frame-accurate transition clips from
//! those recordings.
//!
//! Every recording is keyless. The same three credential-free providers serve
//! all three platforms, and the capture refuses to start when a known
//! credential variable is present in the environment, so a key can never slip
//! into a recording or a published frame.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::WalkthroughError;
use crate::ffmpeg;
use crate::normalise::DEFAULT_WALKTHROUGH_ASPECT_RATIO;

/// The default directory raw recordings are written to.
pub const DEFAULT_RECORDING_DIRECTORY: &str = "tmp/walkthrough-recordings";

/// The default address of the existing browser harness, from
/// `platform/web/serve.mjs`.
pub const DEFAULT_HARNESS_URL: &str = "http://127.0.0.1:8642/";

/// The directory the browser harness's Playwright dependency resolves from.
pub const BROWSER_HARNESS_DIRECTORY: &str = "platform/web";

/// The providers every recorded platform uses without a credential.
pub const KEYLESS_PROVIDER_SUMMARY: &str = "OpenFreeMap for the vector base, Photon for geocoding and place search, and OSRM for routing (Valhalla is the documented fallback)";

/// The credential environment variables a capture refuses to run with.
pub const CREDENTIAL_ENVIRONMENT_VARIABLES: [&str; 3] = [
    "PROVIDER_API_KEY",
    "TEST_RUNNER_PROVIDER_API_KEY",
    "MAPQUEST_CONSUMER_KEY",
];

/// The clean status-bar time the iOS recording shows.
pub const IOS_STATUS_BAR_TIME: &str = "9:41";

/// The clean status-bar battery state the iOS recording shows.
pub const IOS_STATUS_BAR_BATTERY_STATE: &str = "charged";

/// The clean status-bar battery level the iOS recording shows.
pub const IOS_STATUS_BAR_BATTERY_LEVEL: &str = "100";

/// The clean status-bar Wi-Fi strength the iOS recording shows.
pub const IOS_STATUS_BAR_WIFI_BARS: &str = "3";

/// The clean status-bar cellular strength the iOS recording shows.
pub const IOS_STATUS_BAR_CELLULAR_BARS: &str = "4";

/// The H.264 codec the iOS simulator records.
pub const IOS_RECORDING_CODEC: &str = "h264";

/// The Android screen-record bit rate.
pub const ANDROID_SCREEN_RECORD_BIT_RATE: &str = "16000000";

/// The recording path the Android emulator writes before the pull.
pub const ANDROID_DEVICE_RECORDING_PATH: &str = "/sdcard/rustic-maps-walkthrough.mp4";

/// The viewport width of the browser recording. The height follows the shared
/// aspect ratio.
pub const BROWSER_RECORDING_WIDTH: u32 = 540;

/// How long the browser recording runs, in milliseconds.
pub const BROWSER_RECORDING_DURATION_MILLISECONDS: u64 = 30_000;

/// The one platform selector on the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapturePlatform {
    /// The Android emulator.
    Android,
    /// The iOS simulator.
    Ios,
    /// The React Native browser harness.
    ReactNative,
}

impl CapturePlatform {
    /// The platform key used by the contract and the recording file name.
    pub fn key(self) -> &'static str {
        match self {
            Self::Android => "android",
            Self::Ios => "ios",
            Self::ReactNative => "react-native",
        }
    }

    /// Every platform in published order.
    pub fn all() -> [Self; 3] {
        [Self::Android, Self::Ios, Self::ReactNative]
    }

    /// The recording file name the platform writes.
    pub fn recording_file_name(self) -> &'static str {
        match self {
            Self::Android => "android.mp4",
            Self::Ios => "ios.mp4",
            Self::ReactNative => "react-native.webm",
        }
    }
}

/// Which platform or platforms a capture run covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureSelection {
    /// One platform.
    One(CapturePlatform),
    /// All three platforms.
    All,
}

impl CaptureSelection {
    /// Resolves the selection to its platforms in published order.
    pub fn platforms(self) -> Vec<CapturePlatform> {
        match self {
            Self::One(platform) => vec![platform],
            Self::All => CapturePlatform::all().to_vec(),
        }
    }
}

/// The options for [`capture`].
#[derive(Debug, Clone, PartialEq)]
pub struct CaptureOptions {
    /// Which platforms to record.
    pub selection: CaptureSelection,
    /// The directory raw recordings are written to.
    pub recording_directory: PathBuf,
    /// Print the commands instead of running them.
    pub dry_run: bool,
    /// The shared height-divided-by-width ratio.
    pub aspect_ratio: f64,
    /// The browser harness address.
    pub harness_url: String,
}

impl Default for CaptureOptions {
    fn default() -> Self {
        Self {
            selection: CaptureSelection::All,
            recording_directory: PathBuf::from(DEFAULT_RECORDING_DIRECTORY),
            dry_run: false,
            aspect_ratio: DEFAULT_WALKTHROUGH_ASPECT_RATIO,
            harness_url: DEFAULT_HARNESS_URL.to_owned(),
        }
    }
}

/// One external command in a capture recipe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureCommand {
    /// What the command does, printed above it.
    pub step: String,
    /// The directory the command runs in, when it differs from the caller's.
    pub working_directory: Option<PathBuf>,
    /// The program to run.
    pub program: String,
    /// The argument vector.
    pub arguments: Vec<String>,
}

/// The command sequence for one platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturePlan {
    /// The platform this plan records.
    pub platform: CapturePlatform,
    /// The commands to run in order.
    pub commands: Vec<CaptureCommand>,
}

impl CapturePlan {
    /// Renders the plan as the lines a dry run prints.
    pub fn render(&self) -> Vec<String> {
        let mut lines = vec![format!("# {}", self.platform.key())];
        for command in &self.commands {
            lines.push(format!("# {}", command.step));
            let rendered = ffmpeg::render_command(&command.program, &command.arguments);
            lines.push(match &command.working_directory {
                Some(directory) => format!("cd {} && {rendered}", directory.display()),
                None => rendered,
            });
        }
        lines
    }
}

/// Builds the capture plans without running anything.
///
/// # Errors
/// Returns [`WalkthroughError::CredentialEnvironmentPresent`] when a known
/// credential variable is set and [`WalkthroughError::InvalidOption`] for a
/// ratio that cannot size the browser recording.
pub fn plan_capture(options: &CaptureOptions) -> Result<Vec<CapturePlan>, WalkthroughError> {
    assert_no_capture_credentials()?;
    browser_recording_height(options.aspect_ratio)?;
    options
        .selection
        .platforms()
        .into_iter()
        .map(|platform| {
            Ok(CapturePlan {
                platform,
                commands: platform_commands(platform, options)?,
            })
        })
        .collect()
}

/// Prints and, unless this is a dry run, runs the capture plans.
///
/// # Errors
/// Returns the planning failures from [`plan_capture`], a write failure when
/// the recording directory cannot be created, and the program or command
/// failures a recording reports.
pub fn capture(options: &CaptureOptions) -> Result<Vec<CapturePlan>, WalkthroughError> {
    let plans = plan_capture(options)?;
    println!("keyless providers: {KEYLESS_PROVIDER_SUMMARY}");
    if options.dry_run {
        for plan in &plans {
            for line in plan.render() {
                println!("{line}");
            }
        }
        return Ok(plans);
    }

    let recording_directory = absolute_directory(&options.recording_directory)?;
    std::fs::create_dir_all(&recording_directory).map_err(|source| WalkthroughError::Write {
        path: recording_directory.clone(),
        source,
    })?;
    for plan in &plans {
        println!(
            "recording {} into `{}`",
            plan.platform.key(),
            recording_directory.display()
        );
        for command in &plan.commands {
            println!("# {}", command.step);
            run_capture_command(command)?;
        }
    }
    Ok(plans)
}

/// Fails when a known credential variable is present in the environment.
///
/// # Errors
/// Returns [`WalkthroughError::CredentialEnvironmentPresent`] naming the first
/// non-empty variable.
pub fn assert_no_capture_credentials() -> Result<(), WalkthroughError> {
    for variable in CREDENTIAL_ENVIRONMENT_VARIABLES {
        if std::env::var_os(variable).is_some_and(|value| !value.is_empty()) {
            return Err(WalkthroughError::CredentialEnvironmentPresent {
                variable: variable.to_owned(),
            });
        }
    }
    Ok(())
}

/// Returns the documented installation instruction for a missing program.
pub fn install_instruction(program: &str) -> String {
    match program {
        "ffmpeg" => ffmpeg::FFMPEG_INSTALL_INSTRUCTION.to_owned(),
        "xcrun" => "install the Xcode command-line tools with `xcode-select --install`".to_owned(),
        "adb" => {
            "install Android platform-tools, for example with `brew install --cask android-platform-tools`"
                .to_owned()
        }
        "node" => "install Node.js and run `npm install` in `platform/web`".to_owned(),
        _ => "install it and add it to PATH".to_owned(),
    }
}

fn platform_commands(
    platform: CapturePlatform,
    options: &CaptureOptions,
) -> Result<Vec<CaptureCommand>, WalkthroughError> {
    let recording_directory = absolute_directory(&options.recording_directory)?;
    let output = recording_directory
        .join(platform.recording_file_name())
        .display()
        .to_string();
    match platform {
        CapturePlatform::Ios => Ok(ios_commands(&output)),
        CapturePlatform::Android => Ok(android_commands(&output)),
        CapturePlatform::ReactNative => browser_commands(options, &output, recording_directory),
    }
}

fn absolute_directory(directory: &Path) -> Result<PathBuf, WalkthroughError> {
    if directory.is_absolute() {
        return Ok(directory.to_path_buf());
    }
    let current = std::env::current_dir().map_err(|source| WalkthroughError::InvalidOption {
        option: "--recording".to_owned(),
        reason: format!("the working directory could not be read: {source}"),
    })?;
    Ok(current.join(directory))
}

fn ios_commands(output: &str) -> Vec<CaptureCommand> {
    vec![
        CaptureCommand {
            step: "clear the simulator status bar".to_owned(),
            working_directory: None,
            program: "xcrun".to_owned(),
            arguments: vec![
                "simctl".to_owned(),
                "status_bar".to_owned(),
                "booted".to_owned(),
                "override".to_owned(),
                "--time".to_owned(),
                IOS_STATUS_BAR_TIME.to_owned(),
                "--batteryState".to_owned(),
                IOS_STATUS_BAR_BATTERY_STATE.to_owned(),
                "--batteryLevel".to_owned(),
                IOS_STATUS_BAR_BATTERY_LEVEL.to_owned(),
                "--wifiBars".to_owned(),
                IOS_STATUS_BAR_WIFI_BARS.to_owned(),
                "--cellularBars".to_owned(),
                IOS_STATUS_BAR_CELLULAR_BARS.to_owned(),
            ],
        },
        CaptureCommand {
            step: "record the iOS simulator until interrupted".to_owned(),
            working_directory: None,
            program: "xcrun".to_owned(),
            arguments: vec![
                "simctl".to_owned(),
                "io".to_owned(),
                "booted".to_owned(),
                "recordVideo".to_owned(),
                format!("--codec={IOS_RECORDING_CODEC}"),
                "--force".to_owned(),
                output.to_owned(),
            ],
        },
    ]
}

fn android_commands(output: &str) -> Vec<CaptureCommand> {
    vec![
        CaptureCommand {
            step: "record the Android emulator until interrupted".to_owned(),
            working_directory: None,
            program: "adb".to_owned(),
            arguments: vec![
                "shell".to_owned(),
                "screenrecord".to_owned(),
                "--bit-rate".to_owned(),
                ANDROID_SCREEN_RECORD_BIT_RATE.to_owned(),
                ANDROID_DEVICE_RECORDING_PATH.to_owned(),
            ],
        },
        CaptureCommand {
            step: "copy the recording off the emulator".to_owned(),
            working_directory: None,
            program: "adb".to_owned(),
            arguments: vec![
                "pull".to_owned(),
                ANDROID_DEVICE_RECORDING_PATH.to_owned(),
                output.to_owned(),
            ],
        },
    ]
}

fn browser_commands(
    options: &CaptureOptions,
    output: &str,
    output_directory: PathBuf,
) -> Result<Vec<CaptureCommand>, WalkthroughError> {
    let script = browser_recording_script(
        options,
        &output_directory,
        output,
        browser_recording_height(options.aspect_ratio)?,
    );
    Ok(vec![CaptureCommand {
        step: "record the browser walkthrough through the existing Playwright harness".to_owned(),
        working_directory: Some(PathBuf::from(BROWSER_HARNESS_DIRECTORY)),
        program: "node".to_owned(),
        arguments: vec!["--input-type=module".to_owned(), "-e".to_owned(), script],
    }])
}

fn browser_recording_height(aspect_ratio: f64) -> Result<u32, WalkthroughError> {
    if !aspect_ratio.is_finite() || aspect_ratio <= 0.0 {
        return Err(WalkthroughError::InvalidOption {
            option: "--aspect-ratio".to_owned(),
            reason: "the ratio must be a positive finite number".to_owned(),
        });
    }
    let height = (f64::from(BROWSER_RECORDING_WIDTH) * aspect_ratio).round();
    if !height.is_finite() || height < 1.0 || height > f64::from(u32::MAX) {
        return Err(WalkthroughError::InvalidOption {
            option: "--aspect-ratio".to_owned(),
            reason: "the ratio produces an unusable browser recording height".to_owned(),
        });
    }
    Ok(height as u32)
}

fn browser_recording_script(
    options: &CaptureOptions,
    output_directory: &std::path::Path,
    output: &str,
    height: u32,
) -> String {
    let harness_url = json_string(&options.harness_url);
    let directory = json_string(&output_directory.display().to_string());
    let output = json_string(output);
    let width = BROWSER_RECORDING_WIDTH;
    let duration = BROWSER_RECORDING_DURATION_MILLISECONDS;
    format!(
        "import {{ chromium }} from 'playwright';\n\
const browser = await chromium.launch({{ channel: 'chromium' }});\n\
const context = await browser.newContext({{\n\
  viewport: {{ width: {width}, height: {height} }},\n\
  recordVideo: {{ dir: {directory}, size: {{ width: {width}, height: {height} }} }},\n\
}});\n\
const page = await context.newPage();\n\
await page.goto({harness_url});\n\
await page.waitForTimeout({duration});\n\
const video = page.video();\n\
await context.close();\n\
if (video) await video.saveAs({output});\n\
await browser.close();\n"
    )
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

fn run_capture_command(command: &CaptureCommand) -> Result<(), WalkthroughError> {
    let mut process = Command::new(&command.program);
    process.args(&command.arguments);
    if let Some(directory) = &command.working_directory {
        process.current_dir(directory);
    }
    let status = process.status();
    match status {
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            Err(WalkthroughError::ProgramMissing {
                program: command.program.clone(),
                install: install_instruction(&command.program),
            })
        }
        Err(source) => Err(WalkthroughError::CommandFailed {
            program: command.program.clone(),
            arguments: command.arguments.join(" "),
            status: "could not start".to_owned(),
            stderr: source.to_string(),
        }),
        Ok(status) if !status.success() => Err(ffmpeg::failed_command(
            &command.program,
            &command.arguments,
            status,
            b"",
        )),
        Ok(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_the_all_selector_in_published_order() {
        let platforms = CaptureSelection::All.platforms();
        assert_eq!(
            platforms,
            vec![
                CapturePlatform::Android,
                CapturePlatform::Ios,
                CapturePlatform::ReactNative,
            ]
        );
    }

    #[test]
    fn derives_the_browser_height_from_the_shared_ratio() {
        assert_eq!(
            browser_recording_height(DEFAULT_WALKTHROUGH_ASPECT_RATIO)
                .expect("the shared ratio should size the recording"),
            1170
        );
    }

    #[test]
    fn rejects_a_negative_ratio() {
        assert!(browser_recording_height(-1.0).is_err());
    }
}
