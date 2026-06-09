use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const APP_NAME: &str = "coca";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        print_help();
        return Ok(());
    };

    match command.as_str() {
        "help" | "-h" | "--help" => print_help(),
        "fmt" => cargo(["fmt"])?,
        "check" => cargo(["check"])?,
        "test" => cargo(["test"])?,
        "clippy" => cargo(["clippy", "--all-targets", "--", "-D", "warnings"])?,
        "verify" => {
            cargo(["fmt", "--check"])?;
            cargo(["test"])?;
            cargo(["clippy", "--all-targets", "--", "-D", "warnings"])?;
        }
        "build" => {
            let Some(options) = BuildOptions::parse(args.collect())? else {
                return Ok(());
            };
            build(&options)?;
        }
        "dist" => {
            let Some(options) = BuildOptions::parse(args.collect())? else {
                return Ok(());
            };
            let options = options.release();
            build(&options)?;
            copy_dist(&options)?;
        }
        "dist-all" => {
            for target in TARGETS {
                let options = BuildOptions {
                    release: true,
                    target: Some(BuildTarget::from_alias(target)),
                };
                build(&options)?;
                copy_dist(&options)?;
            }
        }
        "targets" => print_targets(),
        other => return Err(format!("unknown xtask command: {other}")),
    }

    Ok(())
}

#[derive(Debug, Default)]
struct BuildOptions {
    release: bool,
    target: Option<BuildTarget>,
}

impl BuildOptions {
    fn parse(args: Vec<String>) -> Result<Option<Self>, String> {
        let mut options = BuildOptions::default();
        let mut iter = args.into_iter();
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--release" => options.release = true,
                "--target" => {
                    let Some(target) = iter.next() else {
                        return Err("--target requires a target triple".to_string());
                    };
                    options.target = Some(BuildTarget::parse(&target));
                }
                "-h" | "--help" => {
                    print_help();
                    return Ok(None);
                }
                other => return Err(format!("unknown build option: {other}")),
            }
        }
        Ok(Some(options))
    }

    fn release(mut self) -> Self {
        self.release = true;
        self
    }

    fn profile_dir(&self) -> &'static str {
        if self.release {
            "release"
        } else {
            "debug"
        }
    }
}

#[derive(Debug)]
struct BuildTarget {
    triple: String,
    dist_name: String,
}

impl BuildTarget {
    fn parse(target: &str) -> Self {
        if let Some(spec) = find_target_alias(target) {
            return Self::from_alias(spec);
        }

        Self {
            triple: target.to_string(),
            dist_name: dist_name_for_target(target, target.contains("windows")),
        }
    }

    fn from_alias(spec: &TargetSpec) -> Self {
        Self {
            triple: spec.triple.to_string(),
            dist_name: dist_name_for_target(spec.alias, spec.windows),
        }
    }
}

struct TargetSpec {
    alias: &'static str,
    triple: &'static str,
    windows: bool,
}

const TARGETS: &[TargetSpec] = &[
    TargetSpec {
        alias: "linux-x64",
        triple: "x86_64-unknown-linux-gnu",
        windows: false,
    },
    TargetSpec {
        alias: "linux-arm64",
        triple: "aarch64-unknown-linux-gnu",
        windows: false,
    },
    TargetSpec {
        alias: "macos-x64",
        triple: "x86_64-apple-darwin",
        windows: false,
    },
    TargetSpec {
        alias: "macos-arm64",
        triple: "aarch64-apple-darwin",
        windows: false,
    },
    TargetSpec {
        alias: "windows-x64",
        triple: "x86_64-pc-windows-msvc",
        windows: true,
    },
];

fn build(options: &BuildOptions) -> Result<(), String> {
    let mut args = vec!["build"];
    if options.release {
        args.push("--release");
    }
    if let Some(target) = &options.target {
        args.push("--target");
        args.push(&target.triple);
    }
    cargo(args)
}

fn copy_dist(options: &BuildOptions) -> Result<(), String> {
    let exe_name = executable_name(options.target.as_ref().map(|target| target.triple.as_str()));
    let source = artifact_path(options, &exe_name);
    if !source.exists() {
        return Err(format!(
            "build artifact not found: {}",
            source.to_string_lossy()
        ));
    }

    let dist_dir = PathBuf::from("dist");
    fs::create_dir_all(&dist_dir).map_err(|err| format!("create dist directory: {err}"))?;

    let output_name = dist_output_name(options);
    let output = dist_dir.join(output_name);
    fs::copy(&source, &output).map_err(|err| {
        format!(
            "copy {} to {}: {err}",
            source.to_string_lossy(),
            output.to_string_lossy()
        )
    })?;

    println!("wrote {}", output.to_string_lossy());
    Ok(())
}

fn artifact_path(options: &BuildOptions, exe_name: &str) -> PathBuf {
    match &options.target {
        Some(target) => Path::new("target")
            .join(&target.triple)
            .join(options.profile_dir())
            .join(exe_name),
        None => Path::new("target")
            .join(options.profile_dir())
            .join(exe_name),
    }
}

fn executable_name(target: Option<&str>) -> String {
    if target.map_or(env::consts::OS == "windows", |target| {
        target.contains("windows")
    }) {
        format!("{APP_NAME}.exe")
    } else {
        APP_NAME.to_string()
    }
}

fn dist_output_name(options: &BuildOptions) -> String {
    if let Some(target) = &options.target {
        return target.dist_name.clone();
    }

    dist_name_for_target(&current_platform_label(), env::consts::OS == "windows")
}

fn dist_name_for_target(label: &str, windows: bool) -> String {
    if windows {
        format!("{APP_NAME}-{label}.exe")
    } else {
        format!("{APP_NAME}-{label}")
    }
}

fn cargo<I, S>(args: I) -> Result<(), String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = args.into_iter().collect::<Vec<_>>();
    let printable = args
        .iter()
        .map(|arg| arg.as_ref().to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    println!("cargo {printable}");

    let status = Command::new("cargo")
        .args(args)
        .status()
        .map_err(|err| format!("failed to run cargo: {err}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("cargo {printable} exited with {status}"))
    }
}

fn find_target_alias(alias: &str) -> Option<&'static TargetSpec> {
    TARGETS.iter().find(|target| target.alias == alias)
}

fn current_platform_label() -> String {
    format!("{}-{}", env::consts::ARCH, env::consts::OS)
}

fn print_targets() {
    println!("aliases:");
    for target in TARGETS {
        let dist_name = dist_name_for_target(target.alias, target.windows);
        println!(
            "  {:13} -> {:27} dist/{}",
            target.alias, target.triple, dist_name
        );
    }
}

fn print_help() {
    println!(
        "\
cargo xtask <command>

Commands:
  fmt                         Run cargo fmt
  check                       Run cargo check
  test                        Run cargo test
  clippy                      Run cargo clippy --all-targets -- -D warnings
  verify                      Run fmt --check, test, and clippy
  build [--release] [--target TARGET]
                              Build the app
  dist [--target TARGET]      Build release binary and copy it to dist/
  dist-all                    Build dist binaries for known macOS/Linux/Windows targets
  targets                     Print target aliases and dist output names

Target aliases:
  linux-x64, linux-arm64, macos-x64, macos-arm64, windows-x64

Examples:
  cargo xtask verify
  cargo xtask dist
  cargo xtask dist --target linux-x64
  cargo xtask dist --target windows-x64
"
    );
}
