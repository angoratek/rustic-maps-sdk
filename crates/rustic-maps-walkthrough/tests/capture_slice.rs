//! Capture and slice tests.
//!
//! The media operations are exercised against a stub `ffmpeg` placed first on
//! `PATH`. The stub records every argument vector and writes deterministic
//! bytes, so the tests need no encoder and no device. One test that needs a
//! real `ffmpeg` skips with an explicit message when the binary is absent.
#![cfg(unix)]

use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use rustic_maps_walkthrough::{
    CREDENTIAL_ENVIRONMENT_VARIABLES, CaptureOptions, CapturePlatform, CaptureSelection,
    DEFAULT_HARNESS_URL, DEFAULT_PAD_COLOR, DEFAULT_WALKTHROUGH_ASPECT_RATIO,
    HANDOFF_CHANNEL_TOLERANCE, NormalisationMode, NormalisationStrategy, NormaliseOptions,
    StillOptions, TransitionOptions, WalkthroughError, capture, extract_still, extract_transition,
    normalise_image, read_png_dimensions,
};

/// The stub encoder. It appends every call to `FFMPEG_STUB_ARGUMENTS`, then
/// writes `FFMPEG_STUB_BYTES` bytes to the last argument, which is always an
/// output path. With `FFMPEG_STUB_MISMATCH=1` the still decode writes `0xff`
/// instead of `0x00`, which the handoff comparison must reject.
const STUB_SCRIPT: &str = r#"#!/bin/sh
set -eu
recorded="${FFMPEG_STUB_ARGUMENTS:?}"
{
  printf 'CALL\n'
  for argument in "$@"; do
    printf '%s\n' "$argument"
  done
} >> "$recorded"
output=''
for argument in "$@"; do
  output="$argument"
done
if [ -n "$output" ] && [ "$output" != '-' ]; then
  mkdir -p "$(dirname "$output")" 2>/dev/null || true
  count="${FFMPEG_STUB_BYTES:-0}"
  byte='\000'
  if [ "${FFMPEG_STUB_MISMATCH:-0}" = '1' ]; then
    case "$output" in
      *next-still.rgb) byte='\377' ;;
    esac
  fi
  : > "$output"
  index=0
  while [ "$index" -lt "$count" ]; do
    printf '%b' "$byte" >> "$output"
    index=$((index + 1))
  done
fi
exit 0
"#;

static TEMPORARY_DIRECTORY_COUNTER: AtomicU64 = AtomicU64::new(0);
static ENVIRONMENT_LOCK: Mutex<()> = Mutex::new(());

struct TemporaryDirectory {
    path: PathBuf,
}

impl TemporaryDirectory {
    fn new(label: &str) -> Self {
        let unique = TEMPORARY_DIRECTORY_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "rustic-maps-walkthrough-capture-{}-{unique}-{label}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("temporary directory should be creatable");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Saves the named process variables and restores them when dropped.
struct EnvironmentGuard {
    saved: Vec<(String, Option<OsString>)>,
}

impl EnvironmentGuard {
    fn capture(names: &[&str]) -> Self {
        Self {
            saved: names
                .iter()
                .map(|name| ((*name).to_owned(), std::env::var_os(name)))
                .collect(),
        }
    }
}

impl Drop for EnvironmentGuard {
    fn drop(&mut self) {
        for (name, value) in &self.saved {
            // SAFETY: the environment lock is held for the guard's lifetime,
            // so no other test reads or writes the environment concurrently.
            match value {
                Some(value) => unsafe { std::env::set_var(name, value) },
                None => unsafe { std::env::remove_var(name) },
            }
        }
    }
}

fn managed_environment() -> EnvironmentGuard {
    let mut names: Vec<&str> = vec![
        "PATH",
        "FFMPEG_STUB_ARGUMENTS",
        "FFMPEG_STUB_BYTES",
        "FFMPEG_STUB_MISMATCH",
    ];
    names.extend(CREDENTIAL_ENVIRONMENT_VARIABLES.iter().copied());
    EnvironmentGuard::capture(&names)
}

fn set_variable(name: &str, value: &OsStr) {
    // SAFETY: every mutating test holds ENVIRONMENT_LOCK and restores the
    // variable through EnvironmentGuard.
    unsafe { std::env::set_var(name, value) };
}

fn remove_variable(name: &str) {
    // SAFETY: every mutating test holds ENVIRONMENT_LOCK and restores the
    // variable through EnvironmentGuard.
    unsafe { std::env::remove_var(name) };
}

fn lock_environment() -> std::sync::MutexGuard<'static, ()> {
    ENVIRONMENT_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn install_stub_ffmpeg(directory: &Path) -> PathBuf {
    let binary_directory = directory.join("bin");
    fs::create_dir_all(&binary_directory).expect("the stub directory should be creatable");
    let stub = binary_directory.join("ffmpeg");
    fs::write(&stub, STUB_SCRIPT).expect("the stub should be writable");
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755))
        .expect("the stub should be executable");
    binary_directory
}

fn prepend_path(directory: &Path) {
    let original = std::env::var_os("PATH").unwrap_or_default();
    let mut combined = OsString::from(directory.as_os_str());
    combined.push(":");
    combined.push(&original);
    set_variable("PATH", &combined);
}

fn png_header(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
    bytes.extend_from_slice(&13_u32.to_be_bytes());
    bytes.extend_from_slice(b"IHDR");
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(&height.to_be_bytes());
    bytes.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
    bytes
}

fn recorded_calls(path: &Path) -> Vec<Vec<String>> {
    let text = fs::read_to_string(path).expect("the stub should record its arguments");
    let mut calls = Vec::new();
    let mut current: Option<Vec<String>> = None;
    for line in text.lines() {
        if line == "CALL" {
            if let Some(call) = current.take() {
                calls.push(call);
            }
            current = Some(Vec::new());
        } else if let Some(call) = current.as_mut() {
            call.push(line.to_owned());
        }
    }
    if let Some(call) = current.take() {
        calls.push(call);
    }
    calls
}

fn position(arguments: &[String], option: &str) -> usize {
    arguments
        .iter()
        .position(|argument| argument == option)
        .unwrap_or_else(|| panic!("`{option}` should be present in {arguments:?}"))
}

fn argument_after<'a>(arguments: &'a [String], option: &str) -> &'a str {
    let index = position(arguments, option);
    arguments
        .get(index + 1)
        .map(String::as_str)
        .unwrap_or_else(|| panic!("`{option}` should carry a value in {arguments:?}"))
}

fn plan_for(
    plans: &[rustic_maps_walkthrough::CapturePlan],
    platform: CapturePlatform,
) -> &rustic_maps_walkthrough::CapturePlan {
    plans
        .iter()
        .find(|plan| plan.platform == platform)
        .unwrap_or_else(|| panic!("the capture plan should contain {platform:?}"))
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the crate lives two levels below the repository root")
        .to_path_buf()
}

#[test]
fn runs_the_slice_handoff_through_a_stub_ffmpeg() {
    let _lock = lock_environment();
    let _environment = managed_environment();
    let temporary = TemporaryDirectory::new("slice");
    let stub_directory = install_stub_ffmpeg(temporary.path());
    let arguments_path = temporary.path().join("arguments.log");

    let input = temporary.path().join("recording.mp4");
    fs::write(&input, b"stub input").expect("the input should be writable");
    let next_still = temporary.path().join("next.png");
    fs::write(&next_still, png_header(4, 6)).expect("the still should be writable");
    let output = temporary.path().join("clip.mp4");

    prepend_path(&stub_directory);
    set_variable("FFMPEG_STUB_ARGUMENTS", arguments_path.as_os_str());
    set_variable("FFMPEG_STUB_BYTES", OsStr::new("72"));

    let report = extract_transition(&TransitionOptions {
        input: input.clone(),
        output: output.clone(),
        start: "1.0".to_owned(),
        end: "3.0".to_owned(),
        next_still: next_still.clone(),
    })
    .expect("the stub should produce a matching clip");

    assert_eq!(report.total_channels, 72);
    assert_eq!(report.matching_channels, 72);
    assert_eq!(report.maximum_difference, 0);
    assert!(output.is_file());

    let calls = recorded_calls(&arguments_path);
    assert_eq!(
        calls.len(),
        3,
        "clip encode, last-frame decode and still decode"
    );

    let transition = &calls[0];
    let input_position = position(transition, "-i");
    let seek_position = position(transition, "-ss");
    assert!(
        input_position < seek_position,
        "`-ss` must follow `-i`: {transition:?}"
    );
    assert_eq!(transition[input_position + 1], input.display().to_string());
    assert_eq!(transition[seek_position + 1], "1.0");
    assert!(seek_position < position(transition, "-to"));
    assert_eq!(argument_after(transition, "-to"), "3.0");
    assert!(transition.contains(&"-an".to_owned()));
    assert_eq!(argument_after(transition, "-c:v"), "libx264");
    assert_eq!(argument_after(transition, "-crf"), "20");
    assert_eq!(argument_after(transition, "-preset"), "slow");
    assert_eq!(argument_after(transition, "-pix_fmt"), "yuv420p");
    assert_eq!(argument_after(transition, "-movflags"), "+faststart");

    let last_frame = &calls[1];
    assert!(last_frame.contains(&"-sseof".to_owned()));
    assert!(position(last_frame, "-sseof") < position(last_frame, "-i"));
    assert_eq!(argument_after(last_frame, "-sseof"), "-1");

    let still_decode = &calls[2];
    assert_eq!(argument_after(still_decode, "-frames:v"), "1");
}

#[test]
fn refuses_a_clip_whose_last_frame_does_not_match_the_next_still() {
    let _lock = lock_environment();
    let _environment = managed_environment();
    let temporary = TemporaryDirectory::new("handoff-mismatch");
    let stub_directory = install_stub_ffmpeg(temporary.path());
    let arguments_path = temporary.path().join("arguments.log");

    let input = temporary.path().join("recording.mp4");
    fs::write(&input, b"stub input").expect("the input should be writable");
    let next_still = temporary.path().join("next.png");
    fs::write(&next_still, png_header(4, 6)).expect("the still should be writable");
    let output = temporary.path().join("clip.mp4");

    prepend_path(&stub_directory);
    set_variable("FFMPEG_STUB_ARGUMENTS", arguments_path.as_os_str());
    set_variable("FFMPEG_STUB_BYTES", OsStr::new("72"));
    set_variable("FFMPEG_STUB_MISMATCH", OsStr::new("1"));

    let result = extract_transition(&TransitionOptions {
        input,
        output: output.clone(),
        start: "1.0".to_owned(),
        end: "3.0".to_owned(),
        next_still: next_still.clone(),
    });

    let error = result.expect_err("a mismatched final frame must be rejected");
    assert!(
        matches!(
            &error,
            WalkthroughError::FrameHandoffMismatch {
                mismatched_channels: 72,
                total_channels: 72,
                maximum_difference: 255,
                tolerance,
                ..
            } if *tolerance == HANDOFF_CHANNEL_TOLERANCE
        ),
        "expected a handoff mismatch, got {error:?}"
    );
    let message = error.to_string();
    assert!(message.contains(&output.display().to_string()));
    assert!(message.contains(&next_still.display().to_string()));
    assert!(message.contains("255"));
}

#[test]
fn extracts_png_and_webp_stills_with_the_documented_quality() {
    let _lock = lock_environment();
    let _environment = managed_environment();
    let temporary = TemporaryDirectory::new("stills");
    let stub_directory = install_stub_ffmpeg(temporary.path());
    let arguments_path = temporary.path().join("arguments.log");
    let input = temporary.path().join("recording.mp4");
    fs::write(&input, b"stub input").expect("the input should be writable");

    prepend_path(&stub_directory);
    set_variable("FFMPEG_STUB_ARGUMENTS", arguments_path.as_os_str());
    set_variable("FFMPEG_STUB_BYTES", OsStr::new("8"));

    extract_still(&StillOptions {
        input: input.clone(),
        output: temporary.path().join("still.png"),
        timestamp: "2.5".to_owned(),
    })
    .expect("the stub should write the PNG");
    extract_still(&StillOptions {
        input,
        output: temporary.path().join("still.webp"),
        timestamp: "00:00:02.500".to_owned(),
    })
    .expect("the stub should write the WebP");

    let calls = recorded_calls(&arguments_path);
    assert_eq!(calls.len(), 2);
    for call in &calls {
        assert!(position(call, "-i") < position(call, "-ss"));
        assert_eq!(argument_after(call, "-frames:v"), "1");
    }
    assert_eq!(argument_after(&calls[0], "-compression_level"), "9");
    assert_eq!(argument_after(&calls[1], "-quality"), "90");
}

#[test]
fn reports_the_install_command_when_ffmpeg_is_absent() {
    let _lock = lock_environment();
    let _environment = managed_environment();
    let temporary = TemporaryDirectory::new("no-ffmpeg");
    let empty_directory = temporary.path().join("empty");
    fs::create_dir_all(&empty_directory).expect("the empty directory should be creatable");
    set_variable("PATH", empty_directory.as_os_str());

    let result = extract_still(&StillOptions {
        input: temporary.path().join("recording.mp4"),
        output: temporary.path().join("still.png"),
        timestamp: "1".to_owned(),
    });
    let error = result.expect_err("a missing ffmpeg must fail clearly");
    assert!(
        matches!(&error, WalkthroughError::ProgramMissing { program, .. } if program == "ffmpeg"),
        "expected a missing-program failure, got {error:?}"
    );
    let message = error.to_string();
    assert!(message.contains("ffmpeg"));
    assert!(message.contains("brew install ffmpeg"));
    assert!(message.contains("apt-get install ffmpeg"));
}

#[test]
fn refuses_to_capture_with_a_credential_variable_set() {
    let _lock = lock_environment();
    let _environment = managed_environment();
    for variable in CREDENTIAL_ENVIRONMENT_VARIABLES {
        remove_variable(variable);
    }
    set_variable("PROVIDER_API_KEY", OsStr::new("secret"));

    let temporary = TemporaryDirectory::new("credential");
    let result = capture(&CaptureOptions {
        selection: CaptureSelection::All,
        recording_directory: temporary.path().join("recordings"),
        dry_run: true,
        aspect_ratio: DEFAULT_WALKTHROUGH_ASPECT_RATIO,
        harness_url: DEFAULT_HARNESS_URL.to_owned(),
    });
    let error = result.expect_err("a set credential variable must refuse the capture");
    assert!(
        matches!(
            &error,
            WalkthroughError::CredentialEnvironmentPresent { variable }
                if variable == "PROVIDER_API_KEY"
        ),
        "expected a credential refusal, got {error:?}"
    );
    assert!(error.to_string().contains("PROVIDER_API_KEY"));
}

#[test]
fn plans_the_keyless_capture_commands_for_every_platform() {
    let _lock = lock_environment();
    let _environment = managed_environment();
    for variable in CREDENTIAL_ENVIRONMENT_VARIABLES {
        remove_variable(variable);
    }
    let temporary = TemporaryDirectory::new("capture-plan");
    let recording_directory = temporary.path().join("recordings");
    let plans = capture(&CaptureOptions {
        selection: CaptureSelection::All,
        recording_directory: recording_directory.clone(),
        dry_run: true,
        aspect_ratio: DEFAULT_WALKTHROUGH_ASPECT_RATIO,
        harness_url: DEFAULT_HARNESS_URL.to_owned(),
    })
    .expect("a keyless dry run should plan every platform");
    assert_eq!(plans.len(), 3);

    let android = plan_for(&plans, CapturePlatform::Android);
    assert_eq!(android.commands.len(), 2);
    assert_eq!(android.commands[0].program, "adb");
    assert_eq!(
        &android.commands[0].arguments[..4],
        ["shell", "screenrecord", "--bit-rate", "16000000"]
    );
    assert_eq!(android.commands[1].program, "adb");
    assert_eq!(android.commands[1].arguments[0], "pull");
    assert_eq!(
        android.commands[1].arguments[1],
        "/sdcard/rustic-maps-walkthrough.mp4"
    );
    assert_eq!(
        android.commands[1].arguments[2],
        recording_directory
            .join("android.mp4")
            .display()
            .to_string()
    );

    let ios = plan_for(&plans, CapturePlatform::Ios);
    assert_eq!(ios.commands.len(), 2);
    assert_eq!(ios.commands[0].program, "xcrun");
    assert_eq!(
        &ios.commands[0].arguments[..4],
        ["simctl", "status_bar", "booted", "override"]
    );
    assert_eq!(ios.commands[1].program, "xcrun");
    assert_eq!(ios.commands[1].arguments[3], "recordVideo");
    assert!(
        ios.commands[1]
            .arguments
            .contains(&"--codec=h264".to_owned())
    );
    assert!(ios.commands[1].arguments.contains(&"--force".to_owned()));
    let ios_output = recording_directory.join("ios.mp4").display().to_string();
    assert_eq!(
        ios.commands[1].arguments.last().map(String::as_str),
        Some(ios_output.as_str())
    );

    let browser = plan_for(&plans, CapturePlatform::ReactNative);
    assert_eq!(browser.commands.len(), 1);
    assert_eq!(browser.commands[0].program, "node");
    assert!(
        browser.commands[0]
            .arguments
            .iter()
            .any(|argument| argument.contains("playwright"))
    );
    assert!(
        browser.commands[0]
            .arguments
            .iter()
            .any(|argument| argument.contains("recordVideo"))
    );
}

#[test]
fn normalises_a_still_through_a_stub_ffmpeg() {
    let _lock = lock_environment();
    let _environment = managed_environment();
    let temporary = TemporaryDirectory::new("normalise");
    let stub_directory = install_stub_ffmpeg(temporary.path());
    let arguments_path = temporary.path().join("arguments.log");
    let input = temporary.path().join("published.png");
    fs::write(&input, png_header(1080, 2200)).expect("the source should be writable");
    let output = temporary.path().join("normalised.png");

    prepend_path(&stub_directory);
    set_variable("FFMPEG_STUB_ARGUMENTS", arguments_path.as_os_str());
    set_variable("FFMPEG_STUB_BYTES", OsStr::new("8"));

    let transform = normalise_image(&NormaliseOptions {
        input,
        output: output.clone(),
        target_aspect_ratio: DEFAULT_WALKTHROUGH_ASPECT_RATIO,
        strategy: NormalisationStrategy::Auto,
        pad_color: DEFAULT_PAD_COLOR.to_owned(),
    })
    .expect("the stub should normalise the frame");

    assert_eq!(transform.mode, NormalisationMode::Pad);
    assert_eq!(
        (transform.target_width, transform.target_height),
        (1080, 2340)
    );
    assert_eq!(transform.offset_y, 70);
    assert!(output.is_file());

    let calls = recorded_calls(&arguments_path);
    assert_eq!(calls.len(), 1);
    assert_eq!(
        argument_after(&calls[0], "-vf"),
        "pad=1080:2340:0:70:color=0x000000"
    );
    assert_eq!(argument_after(&calls[0], "-compression_level"), "9");
}

#[test]
fn normalises_a_published_still_with_a_real_ffmpeg() {
    let _lock = lock_environment();
    let ffmpeg_is_available = Command::new("ffmpeg").arg("-version").output().is_ok();
    if !ffmpeg_is_available {
        eprintln!(
            "skipping normalises_a_published_still_with_a_real_ffmpeg: ffmpeg is not installed; install it with `brew install ffmpeg` or `apt-get install ffmpeg`"
        );
        return;
    }
    let temporary = TemporaryDirectory::new("real-ffmpeg");
    let output = temporary.path().join("android-plan-sheet.png");
    let transform = normalise_image(&NormaliseOptions {
        input: repository_root().join("site/book-src/img/android-plan-sheet.png"),
        output: output.clone(),
        target_aspect_ratio: DEFAULT_WALKTHROUGH_ASPECT_RATIO,
        strategy: NormalisationStrategy::Auto,
        pad_color: DEFAULT_PAD_COLOR.to_owned(),
    })
    .expect("ffmpeg should normalise the published still");
    assert_eq!(transform.mode, NormalisationMode::Pad);
    let bytes = fs::read(&output).expect("the normalised still should exist");
    assert_eq!(
        read_png_dimensions(&bytes).expect("the output should be a PNG"),
        (1080, 2340)
    );
    // The committed asset must be this recipe's own output: decode both to
    // raw RGB and compare channel by channel, so a regenerated still cannot
    // differ from the one the walkthrough ships.
    let committed = repository_root()
        .join("crates/rustic-maps-walkthrough/assets/normalised/android-plan-sheet.png");
    assert_eq!(
        decode_rgb24(&output),
        decode_rgb24(&committed),
        "the regenerated still matches the committed walkthrough asset pixel for pixel",
    );
}

/// Decodes a PNG to raw RGB24 bytes with the same `ffmpeg` the pipeline uses.
fn decode_rgb24(path: &Path) -> Vec<u8> {
    let output = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
        .output()
        .expect("ffmpeg decodes the still");
    assert!(output.status.success(), "ffmpeg decoded {path:?}");
    output.stdout
}
