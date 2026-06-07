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
            let options = BuildOptions::parse(args.collect())?;
            build(&options)?;
        }
        "dist" => {
            let options = BuildOptions::parse(args.collect())?.release();
            build(&options)?;
            copy_dist(&options)?;
        }
        "dist-all" => {
            for target in default_release_targets() {
                let options = BuildOptions {
                    release: true,
                    target: Some(target.to_string()),
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
    target: Option<String>,
}

impl BuildOptions {
    fn parse(args: Vec<String>) -> Result<Self, String> {
        let mut options = BuildOptions::default();
        let mut iter = args.into_iter();
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--release" => options.release = true,
                "--target" => {
                    let Some(target) = iter.next() else {
                        return Err("--target requires a target triple".to_string());
                    };
                    options.target = Some(expand_target_alias(&target).to_string());
                }
                "-h" | "--help" => {
                    print_help();
                    return Ok(options);
                }
                other => return Err(format!("unknown build option: {other}")),
            }
        }
        Ok(options)
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

fn build(options: &BuildOptions) -> Result<(), String> {
    let mut args = vec!["build"];
    if options.release {
        args.push("--release");
    }
    if let Some(target) = &options.target {
        args.push("--target");
        args.push(target);
    }
    cargo(args)
}

fn copy_dist(options: &BuildOptions) -> Result<(), String> {
    let target = options
        .target
        .clone()
        .unwrap_or_else(current_platform_label);
    let exe_name = executable_name(&target);
    let source = artifact_path(options, &exe_name);
    if !source.exists() {
        return Err(format!(
            "build artifact not found: {}",
            source.to_string_lossy()
        ));
    }

    let dist_dir = PathBuf::from("dist");
    fs::create_dir_all(&dist_dir).map_err(|err| format!("create dist directory: {err}"))?;

    let output_name = if exe_name.ends_with(".exe") {
        format!("{APP_NAME}-{target}.exe")
    } else {
        format!("{APP_NAME}-{target}")
    };
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
            .join(target)
            .join(options.profile_dir())
            .join(exe_name),
        None => Path::new("target")
            .join(options.profile_dir())
            .join(exe_name),
    }
}

fn executable_name(target: &str) -> String {
    if target.contains("windows") {
        format!("{APP_NAME}.exe")
    } else {
        APP_NAME.to_string()
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

fn expand_target_alias(target: &str) -> &str {
    match target {
        "linux-x64" => "x86_64-unknown-linux-gnu",
        "linux-arm64" => "aarch64-unknown-linux-gnu",
        "macos-x64" => "x86_64-apple-darwin",
        "macos-arm64" => "aarch64-apple-darwin",
        "windows-x64" => "x86_64-pc-windows-msvc",
        _ => target,
    }
}

fn default_release_targets() -> &'static [&'static str] {
    &[
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "x86_64-apple-darwin",
        "aarch64-apple-darwin",
        "x86_64-pc-windows-msvc",
    ]
}

fn current_platform_label() -> String {
    format!("{}-{}", env::consts::ARCH, env::consts::OS)
}

fn print_targets() {
    println!("aliases:");
    println!("  linux-x64    -> x86_64-unknown-linux-gnu");
    println!("  linux-arm64  -> aarch64-unknown-linux-gnu");
    println!("  macos-x64    -> x86_64-apple-darwin");
    println!("  macos-arm64  -> aarch64-apple-darwin");
    println!("  windows-x64  -> x86_64-pc-windows-msvc");
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
  dist [--target TARGET]      Build release artifact and copy it to dist/
  dist-all                    Build dist artifacts for default macOS/Linux/Windows targets
  targets                     Print target aliases

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
