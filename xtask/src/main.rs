use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const APP_NAME: &str = "coca";
const DOCKER_RUST_IMAGE: &str = "rust:1-bookworm";
const LINUX_CONTAINER_WORKDIR: &str = "/work";

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
    label: String,
    triple: String,
    dist_name: String,
    linux_container_platform: Option<&'static str>,
}

impl BuildTarget {
    fn parse(target: &str) -> Self {
        if let Some(spec) = find_target_alias(target) {
            return Self::from_alias(spec);
        }

        Self {
            label: target.to_string(),
            triple: target.to_string(),
            dist_name: dist_name_for_target(target, target.contains("windows")),
            linux_container_platform: linux_container_platform(target),
        }
    }

    fn from_alias(spec: &TargetSpec) -> Self {
        Self {
            label: spec.alias.to_string(),
            triple: spec.triple.to_string(),
            dist_name: dist_name_for_target(spec.alias, spec.windows),
            linux_container_platform: spec.linux_container_platform,
        }
    }
}

struct TargetSpec {
    alias: &'static str,
    triple: &'static str,
    windows: bool,
    linux_container_platform: Option<&'static str>,
}

const TARGETS: &[TargetSpec] = &[
    TargetSpec {
        alias: "linux-x64",
        triple: "x86_64-unknown-linux-gnu",
        windows: false,
        linux_container_platform: Some("linux/amd64"),
    },
    TargetSpec {
        alias: "linux-arm64",
        triple: "aarch64-unknown-linux-gnu",
        windows: false,
        linux_container_platform: Some("linux/arm64"),
    },
    TargetSpec {
        alias: "macos-x64",
        triple: "x86_64-apple-darwin",
        windows: false,
        linux_container_platform: None,
    },
    TargetSpec {
        alias: "macos-arm64",
        triple: "aarch64-apple-darwin",
        windows: false,
        linux_container_platform: None,
    },
    TargetSpec {
        alias: "windows-x64",
        triple: "x86_64-pc-windows-msvc",
        windows: true,
        linux_container_platform: None,
    },
];

fn build(options: &BuildOptions) -> Result<(), String> {
    let mut args = vec![build_subcommand(options)];
    if options.release {
        args.push("--release");
    }
    if let Some(target) = &options.target {
        args.push("--target");
        args.push(&target.triple);
    }
    if should_use_linux_container(options) {
        return cargo_in_linux_container(options, args);
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

fn cargo_in_linux_container<I, S>(options: &BuildOptions, cargo_args: I) -> Result<(), String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let target = options
        .target
        .as_ref()
        .ok_or_else(|| "linux container build requires an explicit target".to_string())?;
    let platform = target
        .linux_container_platform
        .ok_or_else(|| format!("no Linux container platform for {}", target.triple))?;

    let Some(docker_status) =
        command_status("docker", ["version", "--format", "{{.Server.Version}}"])?
    else {
        return Err(linux_toolchain_error(
            target,
            "Docker is not installed or is not on PATH.",
        ));
    };
    if !docker_status.success {
        return Err(linux_toolchain_error(
            target,
            &format!("Docker is not available: {}", docker_status.output),
        ));
    }

    let cwd = env::current_dir().map_err(|err| format!("read current directory: {err}"))?;
    let mut args = docker_build_args(platform, &cwd, cargo_args);
    if let Some(user) = current_user() {
        args.splice(2..2, ["--user".to_string(), user]);
    }

    let printable = args.join(" ");
    println!("docker {printable}");
    let status = Command::new("docker")
        .args(&args)
        .status()
        .map_err(|err| format!("failed to run docker: {err}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("docker {printable} exited with {status}"))
    }
}

fn docker_build_args<I, S>(platform: &str, cwd: &Path, cargo_args: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut args = vec![
        "run".to_string(),
        "--rm".to_string(),
        "--platform".to_string(),
        platform.to_string(),
        "--mount".to_string(),
        format!(
            "type=bind,source={},target={LINUX_CONTAINER_WORKDIR}",
            cwd.to_string_lossy()
        ),
        "--workdir".to_string(),
        LINUX_CONTAINER_WORKDIR.to_string(),
        "--env".to_string(),
        "CARGO_TERM_COLOR=always".to_string(),
        "--env".to_string(),
        "CARGO_HOME=/tmp/cargo-home".to_string(),
        "--env".to_string(),
        "HOME=/tmp/coca-home".to_string(),
        DOCKER_RUST_IMAGE.to_string(),
        "cargo".to_string(),
    ];
    args.extend(
        cargo_args
            .into_iter()
            .map(|arg| arg.as_ref().to_string_lossy().into_owned()),
    );
    args
}

fn linux_toolchain_error(target: &BuildTarget, reason: &str) -> String {
    format!(
        "{reason}\n\
         \n\
         {} maps to a Linux GNU target ({}) and this project builds bundled SQLite C code through rusqlite.\n\
         From a non-Linux host, install cargo-zigbuild plus zig, start Docker and retry, run the build on Linux/CI, or install a Linux GNU C toolchain such as x86_64-linux-gnu-gcc and set CC_x86_64_unknown_linux_gnu.",
        target.label, target.triple
    )
}

struct CommandStatus {
    success: bool,
    output: String,
}

fn command_status<I, S>(program: &str, args: I) -> Result<Option<CommandStatus>, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = match Command::new(program).args(args).output() {
        Ok(output) => output,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(format!("failed to run {program}: {err}")),
    };
    let mut text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&stderr);
    }
    if text.is_empty() {
        text = output.status.to_string();
    }
    Ok(Some(CommandStatus {
        success: output.status.success(),
        output: text,
    }))
}

fn should_use_linux_container(options: &BuildOptions) -> bool {
    if env::consts::OS == "linux" {
        return false;
    }
    let Some(target) = &options.target else {
        return false;
    };
    target.linux_container_platform.is_some()
        && !has_linux_cross_cc(&target.triple)
        && !has_zigbuild()
}

fn build_subcommand(options: &BuildOptions) -> &'static str {
    if should_use_zigbuild(options) {
        "zigbuild"
    } else {
        "build"
    }
}

fn should_use_zigbuild(options: &BuildOptions) -> bool {
    if env::consts::OS == "linux" {
        return false;
    }
    let Some(target) = &options.target else {
        return false;
    };
    target.linux_container_platform.is_some()
        && !has_linux_cross_cc(&target.triple)
        && has_zigbuild()
}

fn has_zigbuild() -> bool {
    command_exists("cargo-zigbuild") && command_exists("zig")
}

fn has_linux_cross_cc(triple: &str) -> bool {
    let env_name = triple.replace('-', "_");
    let target_cc = format!("CC_{env_name}");
    if env::var_os(target_cc).is_some()
        || env::var_os("TARGET_CC").is_some()
        || env::var_os("CROSS_COMPILE").is_some()
    {
        return true;
    }

    let compiler_names = match triple {
        "x86_64-unknown-linux-gnu" => &["x86_64-linux-gnu-gcc", "x86_64-unknown-linux-gnu-gcc"][..],
        "aarch64-unknown-linux-gnu" => {
            &["aarch64-linux-gnu-gcc", "aarch64-unknown-linux-gnu-gcc"][..]
        }
        _ => &[][..],
    };
    compiler_names
        .iter()
        .any(|compiler| command_exists(compiler))
}

fn command_exists(program: &str) -> bool {
    let Some(path) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&path).any(|dir| {
        let candidate = dir.join(program);
        candidate.is_file()
    })
}

#[cfg(unix)]
fn current_user() -> Option<String> {
    let uid = command_status("id", ["-u"]).ok()??;
    let gid = command_status("id", ["-g"]).ok()??;
    if uid.success && gid.success {
        Some(format!("{}:{}", uid.output, gid.output))
    } else {
        None
    }
}

#[cfg(not(unix))]
fn current_user() -> Option<String> {
    None
}

fn find_target_alias(alias: &str) -> Option<&'static TargetSpec> {
    TARGETS.iter().find(|target| target.alias == alias)
}

fn linux_container_platform(target: &str) -> Option<&'static str> {
    match target {
        "x86_64-unknown-linux-gnu" => Some("linux/amd64"),
        "aarch64-unknown-linux-gnu" => Some("linux/arm64"),
        _ => None,
    }
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
