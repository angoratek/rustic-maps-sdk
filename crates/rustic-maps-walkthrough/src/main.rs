//! Command-line entry point for validating, building, serving, capturing and
//! slicing the recorded walkthrough.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use rustic_maps_walkthrough::{
    BuildOptions, BuiltManifest, CaptureOptions, CapturePlatform, CaptureSelection,
    DEFAULT_CONTRACT_PATH, DEFAULT_FLOWS_PATH, DEFAULT_HARNESS_URL, DEFAULT_OUTPUT_DIRECTORY,
    DEFAULT_RECORDING_DIRECTORY, DEFAULT_SERVE_ADDRESS, DEFAULT_WALKTHROUGH_ASPECT_RATIO,
    NormalisationStrategy, NormaliseOptions, ServeOptions, StillOptions, TransitionOptions,
    ValidateOptions, WalkthroughError, bind, build_site, capture, extract_still,
    extract_transition, normalise_image, serve_on, validate_manifest, validate_pad_color,
    validate_walkthrough,
};

const USAGE_TEXT: &str = "\
usage: rustic-maps-walkthrough <command> [options]

commands:
  validate    validate the contract, its flow step references and every asset
  build       emit a static walkthrough site into a directory
  serve       serve a built site on localhost with the hotspot authoring overlay
  capture     record the walkthrough on Android, iOS and the browser
  slice       extract a still or a frame-accurate transition clip with ffmpeg
  normalise   crop or pad a published still to the shared walkthrough ratio

options:
  --contract <path>    walkthrough contract [default: platform/reference-walkthrough.json]
  --flows <path>       reference flow document [default: platform/reference-flows.json]
  --repository <path>  directory that contract asset paths resolve against [default: .]
  --output <path>      build output directory, or the slice/normalise output file
  --directory <path>   built directory to serve [default: tmp/walkthrough-site]
  --port <port>        local server port [default: 8080]
  --manifest <path>    manifest whose source commit to check (validate only)
  --no-authoring       serve without the hotspot authoring overlay
  --platform <name>    capture target: android, ios, react-native or all [default: all]
  --dry-run            print the capture commands without running them
  --recording <path>   capture output directory [default: tmp/walkthrough-recordings]
  --harness-url <url>  browser harness address [default: http://127.0.0.1:8642/]
  --input <path>       slice/normalise source recording or still
  --timestamp <time>   still timestamp (slice)
  --start <time>       transition clip start timestamp (slice)
  --end <time>         transition clip end timestamp (slice)
  --next-still <path>  still the clip's last frame must match (slice)
  --aspect-ratio <r>   shared height/width ratio [default: 2.1667]
  --normalisation-mode <auto|pad|crop>  crop-or-pad strategy [default: auto]
  --pad-color <#rrggbb>  bar colour for a padded frame [default: #000000]
  --help               print this text
";

const EXIT_USAGE_FAILURE: u8 = 2;
const EXIT_FAILURE: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Validate,
    Build,
    Serve,
    Capture,
    Slice,
    Normalise,
    Help,
}

#[derive(Debug)]
struct CommandLine {
    command: Command,
    contract_path: PathBuf,
    flows_path: PathBuf,
    repository_root: PathBuf,
    output_directory: PathBuf,
    output_provided: bool,
    directory: PathBuf,
    port: u16,
    manifest_path: Option<PathBuf>,
    authoring_enabled: bool,
    capture_selection: CaptureSelection,
    dry_run: bool,
    recording_directory: PathBuf,
    harness_url: String,
    input_path: Option<PathBuf>,
    next_still_path: Option<PathBuf>,
    timestamp: Option<String>,
    start: Option<String>,
    end: Option<String>,
    aspect_ratio: f64,
    normalisation_strategy: NormalisationStrategy,
    pad_color: String,
}

impl Default for CommandLine {
    fn default() -> Self {
        Self {
            command: Command::Help,
            contract_path: PathBuf::from(DEFAULT_CONTRACT_PATH),
            flows_path: PathBuf::from(DEFAULT_FLOWS_PATH),
            repository_root: PathBuf::from("."),
            output_directory: PathBuf::from(DEFAULT_OUTPUT_DIRECTORY),
            output_provided: false,
            directory: PathBuf::from(DEFAULT_OUTPUT_DIRECTORY),
            port: DEFAULT_SERVE_ADDRESS.1,
            manifest_path: None,
            authoring_enabled: true,
            capture_selection: CaptureSelection::All,
            dry_run: false,
            recording_directory: PathBuf::from(DEFAULT_RECORDING_DIRECTORY),
            harness_url: DEFAULT_HARNESS_URL.to_owned(),
            input_path: None,
            next_still_path: None,
            timestamp: None,
            start: None,
            end: None,
            aspect_ratio: DEFAULT_WALKTHROUGH_ASPECT_RATIO,
            normalisation_strategy: NormalisationStrategy::Auto,
            pad_color: rustic_maps_walkthrough::DEFAULT_PAD_COLOR.to_owned(),
        }
    }
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let command_line = match parse_command_line(&arguments) {
        Ok(command_line) => command_line,
        Err(message) => {
            eprintln!("rustic-maps-walkthrough: {message}\n\n{USAGE_TEXT}");
            return ExitCode::from(EXIT_USAGE_FAILURE);
        }
    };
    run(command_line)
}

fn parse_command_line(arguments: &[String]) -> Result<CommandLine, String> {
    let mut command_line = CommandLine::default();
    let mut command: Option<Command> = None;
    let mut index = 0;
    while index < arguments.len() {
        let argument = &arguments[index];
        index += 1;
        if argument == "--help" || argument == "-h" {
            command_line.command = Command::Help;
            return Ok(command_line);
        }
        if argument == "--no-authoring" {
            command_line.authoring_enabled = false;
            continue;
        }
        if argument == "--dry-run" {
            command_line.dry_run = true;
            continue;
        }
        if let Some(option) = argument.strip_prefix("--") {
            let (name, inline_value) = match option.split_once('=') {
                Some((name, value)) => (name, Some(value.to_owned())),
                None => (option, None),
            };
            let value = match inline_value {
                Some(value) => value,
                None => {
                    let next = arguments
                        .get(index)
                        .ok_or_else(|| format!("option `--{name}` requires a value"))?;
                    index += 1;
                    next.clone()
                }
            };
            match name {
                "contract" => command_line.contract_path = PathBuf::from(value),
                "flows" => command_line.flows_path = PathBuf::from(value),
                "repository" => command_line.repository_root = PathBuf::from(value),
                "output" => {
                    command_line.output_directory = PathBuf::from(value);
                    command_line.output_provided = true;
                }
                "directory" => command_line.directory = PathBuf::from(value),
                "manifest" => command_line.manifest_path = Some(PathBuf::from(value)),
                "port" => {
                    command_line.port = value
                        .parse()
                        .map_err(|_| format!("`{value}` is not a valid port number"))?;
                }
                "platform" => command_line.capture_selection = parse_capture_selection(&value)?,
                "recording" => command_line.recording_directory = PathBuf::from(value),
                "harness-url" => command_line.harness_url = value,
                "input" => command_line.input_path = Some(PathBuf::from(value)),
                "next-still" => command_line.next_still_path = Some(PathBuf::from(value)),
                "timestamp" => command_line.timestamp = Some(value),
                "start" => command_line.start = Some(value),
                "end" => command_line.end = Some(value),
                "aspect-ratio" => command_line.aspect_ratio = parse_aspect_ratio(&value)?,
                "normalisation-mode" => {
                    command_line.normalisation_strategy = parse_normalisation_strategy(&value)?;
                }
                "pad-color" => command_line.pad_color = parse_pad_color(&value)?,
                other => return Err(format!("unknown option `--{other}`")),
            }
            continue;
        }
        if argument.starts_with('-') {
            return Err(format!("unknown option `{argument}`"));
        }
        if command.is_some() {
            return Err(format!("unexpected argument `{argument}`"));
        }
        command = Some(parse_command(argument)?);
    }

    command_line.command = command.ok_or_else(|| "a command is required".to_owned())?;
    Ok(command_line)
}

fn parse_command(name: &str) -> Result<Command, String> {
    match name {
        "validate" => Ok(Command::Validate),
        "build" => Ok(Command::Build),
        "serve" => Ok(Command::Serve),
        "capture" => Ok(Command::Capture),
        "slice" => Ok(Command::Slice),
        "normalise" => Ok(Command::Normalise),
        other => Err(format!("unknown command `{other}`")),
    }
}

fn parse_capture_selection(value: &str) -> Result<CaptureSelection, String> {
    match value {
        "all" => Ok(CaptureSelection::All),
        "android" => Ok(CaptureSelection::One(CapturePlatform::Android)),
        "ios" => Ok(CaptureSelection::One(CapturePlatform::Ios)),
        "react-native" | "react_native" => Ok(CaptureSelection::One(CapturePlatform::ReactNative)),
        other => Err(format!(
            "`{other}` is not a platform; choose android, ios, react-native or all"
        )),
    }
}

fn parse_aspect_ratio(value: &str) -> Result<f64, String> {
    let ratio: f64 = value
        .parse()
        .map_err(|_| format!("`{value}` is not a height divided by width ratio"))?;
    if ratio.is_finite() && ratio > 0.0 {
        Ok(ratio)
    } else {
        Err(format!("`{value}` is not a positive aspect ratio"))
    }
}

fn parse_normalisation_strategy(value: &str) -> Result<NormalisationStrategy, String> {
    match value {
        "auto" => Ok(NormalisationStrategy::Auto),
        "pad" => Ok(NormalisationStrategy::Pad),
        "crop" => Ok(NormalisationStrategy::Crop),
        other => Err(format!(
            "`{other}` is not a normalisation mode; choose auto, pad or crop"
        )),
    }
}

fn parse_pad_color(value: &str) -> Result<String, String> {
    validate_pad_color(value).map_err(|error| error.to_string())
}

fn run(command_line: CommandLine) -> ExitCode {
    match command_line.command {
        Command::Help => {
            println!("{USAGE_TEXT}");
            ExitCode::SUCCESS
        }
        Command::Validate => run_validate(&command_line),
        Command::Build => run_build(&command_line),
        Command::Serve => run_serve(&command_line),
        Command::Capture => run_capture(&command_line),
        Command::Slice => run_slice(&command_line),
        Command::Normalise => run_normalise(&command_line),
    }
}

fn run_validate(command_line: &CommandLine) -> ExitCode {
    let options = ValidateOptions {
        contract_path: command_line.contract_path.clone(),
        flows_path: command_line.flows_path.clone(),
        repository_root: command_line.repository_root.clone(),
    };
    let manifest_path = command_line.manifest_path.clone();
    let result = validate_walkthrough(&options).and_then(|report| {
        if let Some(path) = &manifest_path {
            let manifest = BuiltManifest::from_file(path)?;
            let manifest_report = validate_manifest(&manifest, &options.repository_root)?;
            for note in &manifest_report.notes {
                println!("{note}");
            }
        }
        Ok(report)
    });
    match result {
        Ok(report) => {
            println!(
                "validated {} platforms, {} screens, {} hotspots, {} assets",
                report.platform_count,
                report.screen_count,
                report.hotspot_count,
                report.asset_count
            );
            ExitCode::SUCCESS
        }
        Err(error) => fail(&error),
    }
}

fn run_build(command_line: &CommandLine) -> ExitCode {
    let options = BuildOptions {
        contract_path: command_line.contract_path.clone(),
        flows_path: command_line.flows_path.clone(),
        repository_root: command_line.repository_root.clone(),
        output_directory: command_line.output_directory.clone(),
    };
    match build_site(&options) {
        Ok(report) => {
            println!(
                "built {} files ({} bytes, {} assets) into `{}`",
                report.file_count,
                report.total_bytes,
                report.asset_count,
                report.output_directory.display()
            );
            match &report.source_commit {
                Some(commit) => println!("source commit: {commit}"),
                None => println!("source commit: not recorded because git is unavailable"),
            }
            ExitCode::SUCCESS
        }
        Err(error) => fail(&error),
    }
}

fn run_serve(command_line: &CommandLine) -> ExitCode {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, command_line.port));
    let listener = match bind(address) {
        Ok(listener) => listener,
        Err(error) => return fail(&error),
    };
    let bound_address = match listener.local_addr() {
        Ok(address) => address,
        Err(error) => return fail(&WalkthroughError::Network { source: error }),
    };
    let options = ServeOptions {
        directory: command_line.directory.clone(),
        address,
        authoring_enabled: command_line.authoring_enabled,
    };
    println!(
        "serving `{}` at http://{bound_address}/ with the authoring overlay {}",
        options.directory.display(),
        if options.authoring_enabled {
            "enabled"
        } else {
            "disabled"
        }
    );
    match serve_on(
        listener,
        options.directory.clone(),
        options.authoring_enabled,
        Arc::new(AtomicBool::new(false)),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail(&error),
    }
}

fn run_capture(command_line: &CommandLine) -> ExitCode {
    let options = CaptureOptions {
        selection: command_line.capture_selection,
        recording_directory: command_line.recording_directory.clone(),
        dry_run: command_line.dry_run,
        aspect_ratio: command_line.aspect_ratio,
        harness_url: command_line.harness_url.clone(),
    };
    match capture(&options) {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => fail(&error),
    }
}

fn run_slice(command_line: &CommandLine) -> ExitCode {
    let Some(input) = command_line.input_path.clone() else {
        return usage_failure("the `slice` command requires `--input <path>`");
    };
    if !command_line.output_provided {
        return usage_failure("the `slice` command requires `--output <path>`");
    }
    let output = command_line.output_directory.clone();

    if let Some(timestamp) = &command_line.timestamp {
        if command_line.start.is_some() || command_line.end.is_some() {
            return usage_failure("`--timestamp` extracts a still; drop `--start` and `--end`");
        }
        let options = StillOptions {
            input,
            output: output.clone(),
            timestamp: timestamp.clone(),
        };
        return match extract_still(&options) {
            Ok(()) => {
                println!("wrote still `{}`", output.display());
                ExitCode::SUCCESS
            }
            Err(error) => fail(&error),
        };
    }

    let (Some(start), Some(end)) = (&command_line.start, &command_line.end) else {
        return usage_failure(
            "the `slice` command requires `--timestamp`, or `--start` and `--end`",
        );
    };
    let Some(next_still) = command_line.next_still_path.clone() else {
        return usage_failure(
            "a transition clip requires `--next-still <path>` so its last frame can be checked",
        );
    };
    let options = TransitionOptions {
        input,
        output: output.clone(),
        start: start.clone(),
        end: end.clone(),
        next_still: next_still.clone(),
    };
    match extract_transition(&options) {
        Ok(report) => {
            println!(
                "wrote transition clip `{}`; its last frame matches `{}` on {} of {} colour channels (maximum channel difference {})",
                output.display(),
                next_still.display(),
                report.matching_channels,
                report.total_channels,
                report.maximum_difference
            );
            ExitCode::SUCCESS
        }
        Err(error) => fail(&error),
    }
}

fn run_normalise(command_line: &CommandLine) -> ExitCode {
    let Some(input) = command_line.input_path.clone() else {
        return usage_failure("the `normalise` command requires `--input <path>`");
    };
    if !command_line.output_provided {
        return usage_failure("the `normalise` command requires `--output <path>`");
    }
    let options = NormaliseOptions {
        input: input.clone(),
        output: command_line.output_directory.clone(),
        target_aspect_ratio: command_line.aspect_ratio,
        strategy: command_line.normalisation_strategy,
        pad_color: command_line.pad_color.clone(),
    };
    match normalise_image(&options) {
        Ok(transform) => {
            println!("normalised `{}`: {}", input.display(), transform.summary());
            ExitCode::SUCCESS
        }
        Err(error) => fail(&error),
    }
}

fn usage_failure(message: &str) -> ExitCode {
    eprintln!("rustic-maps-walkthrough: {message}\n\n{USAGE_TEXT}");
    ExitCode::from(EXIT_USAGE_FAILURE)
}

fn fail(error: &WalkthroughError) -> ExitCode {
    eprintln!("rustic-maps-walkthrough: {error}");
    ExitCode::from(EXIT_FAILURE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn parses_the_validate_defaults() {
        let parsed = parse_command_line(&arguments(&["validate"]));
        let Ok(parsed) = parsed else {
            panic!("validate should parse");
        };
        assert_eq!(parsed.command, Command::Validate);
        assert_eq!(parsed.contract_path, PathBuf::from(DEFAULT_CONTRACT_PATH));
        assert_eq!(parsed.flows_path, PathBuf::from(DEFAULT_FLOWS_PATH));
    }

    #[test]
    fn parses_inline_and_separate_option_values() {
        let parsed = parse_command_line(&arguments(&[
            "build",
            "--contract=example/contract.json",
            "--output",
            "out",
            "--port",
            "9000",
        ]));
        let Ok(parsed) = parsed else {
            panic!("build options should parse");
        };
        assert_eq!(parsed.contract_path, PathBuf::from("example/contract.json"));
        assert_eq!(parsed.output_directory, PathBuf::from("out"));
        assert_eq!(parsed.port, 9000);
        assert!(parsed.output_provided);
    }

    #[test]
    fn parses_the_capture_defaults_and_selector() {
        let parsed = parse_command_line(&arguments(&[
            "capture",
            "--platform",
            "react-native",
            "--dry-run",
            "--aspect-ratio",
            "2.0",
        ]));
        let Ok(parsed) = parsed else {
            panic!("capture options should parse");
        };
        assert_eq!(parsed.command, Command::Capture);
        assert_eq!(
            parsed.capture_selection,
            CaptureSelection::One(CapturePlatform::ReactNative)
        );
        assert!(parsed.dry_run);
        assert_eq!(parsed.aspect_ratio, 2.0);
    }

    #[test]
    fn parses_the_slice_and_normalise_values() {
        let parsed = parse_command_line(&arguments(&[
            "slice",
            "--input",
            "recording.mp4",
            "--output",
            "clip.mp4",
            "--start",
            "1",
            "--end",
            "3",
            "--next-still",
            "next.png",
        ]));
        let Ok(parsed) = parsed else {
            panic!("slice options should parse");
        };
        assert_eq!(parsed.input_path, Some(PathBuf::from("recording.mp4")));
        assert_eq!(parsed.next_still_path, Some(PathBuf::from("next.png")));
        assert_eq!(parsed.start.as_deref(), Some("1"));

        let parsed = parse_command_line(&arguments(&[
            "normalise",
            "--input",
            "source.png",
            "--output",
            "normalised.png",
            "--normalisation-mode",
            "crop",
            "--pad-color",
            "#AABBCC",
        ]));
        let Ok(parsed) = parsed else {
            panic!("normalise options should parse");
        };
        assert_eq!(parsed.normalisation_strategy, NormalisationStrategy::Crop);
        assert_eq!(parsed.pad_color, "#aabbcc");
    }

    #[test]
    fn rejects_an_unknown_command() {
        assert!(parse_command_line(&arguments(&["unknown"])).is_err());
    }

    #[test]
    fn rejects_an_option_without_a_value() {
        assert!(parse_command_line(&arguments(&["build", "--output"])).is_err());
    }

    #[test]
    fn rejects_an_unknown_platform() {
        assert!(parse_command_line(&arguments(&["capture", "--platform", "windows"])).is_err());
    }
}
