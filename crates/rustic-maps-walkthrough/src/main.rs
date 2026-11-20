//! Command-line entry point for validating, building and serving the recorded
//! walkthrough.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use rustic_maps_walkthrough::{
    BuildOptions, BuiltManifest, DEFAULT_CONTRACT_PATH, DEFAULT_FLOWS_PATH,
    DEFAULT_OUTPUT_DIRECTORY, DEFAULT_SERVE_ADDRESS, ServeOptions, ValidateOptions,
    WalkthroughError, bind, build_site, serve_on, validate_manifest, validate_walkthrough,
};

const USAGE_TEXT: &str = "\
usage: rustic-maps-walkthrough <command> [options]

commands:
  validate   validate the contract, its flow step references and every asset
  build      emit a static walkthrough site into a directory
  serve      serve a built site on localhost with the hotspot authoring overlay
  capture    record transition clips (unavailable in this build)
  slice      cut a transition clip (unavailable in this build)

options:
  --contract <path>    walkthrough contract [default: platform/reference-walkthrough.json]
  --flows <path>       reference flow document [default: platform/reference-flows.json]
  --repository <path>  directory that contract asset paths resolve against [default: .]
  --output <path>      build output directory [default: tmp/walkthrough-site]
  --directory <path>   built directory to serve [default: tmp/walkthrough-site]
  --port <port>        local server port [default: 8080]
  --manifest <path>    manifest whose source commit to check (validate only)
  --no-authoring       serve without the hotspot authoring overlay
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
    Help,
}

#[derive(Debug)]
struct CommandLine {
    command: Command,
    contract_path: PathBuf,
    flows_path: PathBuf,
    repository_root: PathBuf,
    output_directory: PathBuf,
    directory: PathBuf,
    port: u16,
    manifest_path: Option<PathBuf>,
    authoring_enabled: bool,
}

impl Default for CommandLine {
    fn default() -> Self {
        Self {
            command: Command::Help,
            contract_path: PathBuf::from(DEFAULT_CONTRACT_PATH),
            flows_path: PathBuf::from(DEFAULT_FLOWS_PATH),
            repository_root: PathBuf::from("."),
            output_directory: PathBuf::from(DEFAULT_OUTPUT_DIRECTORY),
            directory: PathBuf::from(DEFAULT_OUTPUT_DIRECTORY),
            port: DEFAULT_SERVE_ADDRESS.1,
            manifest_path: None,
            authoring_enabled: true,
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
                "output" => command_line.output_directory = PathBuf::from(value),
                "directory" => command_line.directory = PathBuf::from(value),
                "manifest" => command_line.manifest_path = Some(PathBuf::from(value)),
                "port" => {
                    command_line.port = value
                        .parse()
                        .map_err(|_| format!("`{value}` is not a valid port number"))?;
                }
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
        other => Err(format!("unknown command `{other}`")),
    }
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
        Command::Capture => unavailable("capture"),
        Command::Slice => unavailable("slice"),
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

fn unavailable(command: &'static str) -> ExitCode {
    fail(&WalkthroughError::CommandNotImplemented { command })
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
    }

    #[test]
    fn rejects_an_unknown_command() {
        assert!(parse_command_line(&arguments(&["unknown"])).is_err());
    }

    #[test]
    fn rejects_an_option_without_a_value() {
        assert!(parse_command_line(&arguments(&["build", "--output"])).is_err());
    }
}
