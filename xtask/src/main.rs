use std::env;
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

const APP_NAME: &str = "coca";
const DOCKER_RUST_IMAGE: &str = "rust:1-bookworm";
const LINUX_CONTAINER_WORKDIR: &str = "/work";
const DEV_STATE_DIR: &str = ".ai/run/xtask-dev";
const DEV_HTTP_BIND_HOST: &str = "0.0.0.0";
const DEV_LOCAL_URL_HOST: &str = "127.0.0.1";
const DEFAULT_GATEWAY_PORT: u16 = 8787;
const DEFAULT_VITE_PORT: u16 = 5173;
const GATEWAY_PORT_END: u16 = 8877;
const VITE_PORT_END: u16 = 5273;
const LOG_TAIL_LINES: usize = 120;

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
        "fmt" => cargo(["fmt", "--all"])?,
        "check" => cargo(["check", "--workspace"])?,
        "test" => cargo(["test", "--workspace"])?,
        "clippy" => cargo([
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ])?,
        "verify" => {
            cargo(["fmt", "--all", "--check"])?;
            cargo(["test", "--workspace"])?;
            cargo([
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ])?;
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
        "dev" => {
            let Some(command) = DevCommand::parse(args.collect())? else {
                return Ok(());
            };
            run_dev(command)?;
        }
        "run" => {
            let Some(options) = RunOptions::parse(args.collect())? else {
                return Ok(());
            };
            run_stack(options)?;
        }
        "targets" => print_targets(),
        other => return Err(format!("unknown xtask command: {other}")),
    }

    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RunOptions {
    smart_port: bool,
    skip_install: bool,
}

impl RunOptions {
    fn parse(args: Vec<String>) -> Result<Option<Self>, String> {
        if args.iter().any(|arg| arg == "-h" || arg == "--help") {
            print_run_help();
            return Ok(None);
        }

        let mut options = Self {
            smart_port: false,
            skip_install: false,
        };
        for arg in args {
            match arg.as_str() {
                "--smart-port" => options.smart_port = true,
                "--skip-install" => options.skip_install = true,
                other => return Err(format!("unknown run option: {other}")),
            }
        }
        Ok(Some(options))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DevCommand {
    Start(DevOptions),
    Stop,
    Restart(DevOptions),
    Status,
    Logs(Option<DevService>),
}

impl DevCommand {
    fn parse(args: Vec<String>) -> Result<Option<Self>, String> {
        let mut iter = args.into_iter();
        let Some(command) = iter.next() else {
            print_dev_help();
            return Ok(None);
        };
        match command.as_str() {
            "help" | "-h" | "--help" => {
                print_dev_help();
                Ok(None)
            }
            "start" => {
                let args = iter.collect::<Vec<_>>();
                if args.iter().any(|arg| arg == "-h" || arg == "--help") {
                    print_dev_help();
                    return Ok(None);
                }
                Ok(Some(Self::Start(DevOptions::parse(args)?)))
            }
            "restart" => {
                let args = iter.collect::<Vec<_>>();
                if args.iter().any(|arg| arg == "-h" || arg == "--help") {
                    print_dev_help();
                    return Ok(None);
                }
                Ok(Some(Self::Restart(DevOptions::parse(args)?)))
            }
            "stop" => reject_extra_args("dev stop", iter.collect()).map(|()| Some(Self::Stop)),
            "status" => {
                reject_extra_args("dev status", iter.collect()).map(|()| Some(Self::Status))
            }
            "logs" => {
                let args = iter.collect::<Vec<_>>();
                match args.as_slice() {
                    [] => Ok(Some(Self::Logs(None))),
                    [service] => Ok(Some(Self::Logs(Some(DevService::parse(service)?)))),
                    _ => Err("dev logs accepts at most one service name".to_string()),
                }
            }
            other => Err(format!("unknown dev command: {other}")),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DevOptions {
    mode: DevMode,
    skip_install: bool,
    force: bool,
}

impl Default for DevOptions {
    fn default() -> Self {
        Self {
            mode: DevMode::Dev,
            skip_install: false,
            force: false,
        }
    }
}

impl DevOptions {
    fn parse(args: Vec<String>) -> Result<Self, String> {
        let mut options = Self::default();
        let mut iter = args.into_iter();
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--mode" => {
                    let Some(mode) = iter.next() else {
                        return Err("--mode requires dev or release".to_string());
                    };
                    options.mode = DevMode::parse(&mode)?;
                }
                "--skip-install" => options.skip_install = true,
                "--force" => options.force = true,
                other => return Err(format!("unknown dev option: {other}")),
            }
        }
        Ok(options)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DevMode {
    Dev,
    Release,
}

impl DevMode {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "dev" => Ok(Self::Dev),
            "release" => Ok(Self::Release),
            other => Err(format!("unknown dev mode: {other}")),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Dev => "dev",
            Self::Release => "release",
        }
    }

    fn desired_services(self) -> &'static [DevService] {
        match self {
            Self::Dev => &[DevService::Daemon, DevService::Gateway, DevService::Vite],
            Self::Release => &[DevService::Daemon, DevService::Gateway],
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DevRuntime {
    bind_host: String,
    gateway_port: u16,
    vite_port: u16,
}

impl Default for DevRuntime {
    fn default() -> Self {
        Self {
            bind_host: DEV_HTTP_BIND_HOST.to_string(),
            gateway_port: DEFAULT_GATEWAY_PORT,
            vite_port: DEFAULT_VITE_PORT,
        }
    }
}

impl DevRuntime {
    fn gateway_bind(&self) -> String {
        format!("{}:{}", self.bind_host, self.gateway_port)
    }

    fn vite_bind(&self) -> String {
        format!("{}:{}", self.bind_host, self.vite_port)
    }

    fn gateway_local_url(&self) -> String {
        format!("http://{DEV_LOCAL_URL_HOST}:{}", self.gateway_port)
    }

    fn vite_local_url(&self) -> String {
        format!("http://{DEV_LOCAL_URL_HOST}:{}", self.vite_port)
    }

    fn vite_api_proxy_target(&self) -> String {
        self.gateway_local_url()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DevState {
    mode: DevMode,
    runtime: DevRuntime,
}

#[derive(Clone, Debug)]
struct ServicePlan {
    mode: DevMode,
    runtime: DevRuntime,
    source: ServiceSource,
}

struct ServiceCommand {
    program: String,
    args: Vec<String>,
    cwd: Option<&'static str>,
    envs: Vec<(&'static str, String)>,
}

#[derive(Clone, Debug)]
enum ServiceSource {
    CargoRun,
    DebugBinary,
    ReleaseBinary,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DevService {
    Daemon,
    Gateway,
    Vite,
}

impl DevService {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "daemon" => Ok(Self::Daemon),
            "gateway" => Ok(Self::Gateway),
            "vite" | "frontend" | "web" => Ok(Self::Vite),
            other => Err(format!("unknown dev service: {other}")),
        }
    }

    fn all() -> &'static [Self] {
        &[Self::Daemon, Self::Gateway, Self::Vite]
    }

    fn name(self) -> &'static str {
        match self {
            Self::Daemon => "daemon",
            Self::Gateway => "gateway",
            Self::Vite => "vite",
        }
    }

    fn url(self, runtime: &DevRuntime) -> String {
        match self {
            Self::Daemon => "-".to_string(),
            Self::Gateway => runtime.gateway_local_url(),
            Self::Vite => runtime.vite_local_url(),
        }
    }

    fn pid_path(self) -> PathBuf {
        Path::new(DEV_STATE_DIR).join(format!("{}.pid", self.name()))
    }

    fn log_path(self) -> PathBuf {
        Path::new(DEV_STATE_DIR).join(format!("{}.log", self.name()))
    }
}

fn run_dev(command: DevCommand) -> Result<(), String> {
    match command {
        DevCommand::Start(options) => dev_start(options),
        DevCommand::Stop => dev_stop(),
        DevCommand::Restart(options) => {
            dev_stop()?;
            dev_start(options)
        }
        DevCommand::Status => dev_status(),
        DevCommand::Logs(service) => dev_logs(service),
    }
}

fn run_stack(options: RunOptions) -> Result<(), String> {
    fs::create_dir_all(DEV_STATE_DIR).map_err(|err| format!("create {DEV_STATE_DIR}: {err}"))?;

    let build_options = BuildOptions {
        release: false,
        target: None,
    };
    println!("detected host platform: {}", current_platform_label());
    build(&build_options)?;
    ensure_web_dependencies(options.skip_install)?;

    dev_stop()?;
    let runtime = if options.smart_port {
        choose_smart_runtime()?
    } else {
        cleanup_default_http_ports()?;
        DevRuntime::default()
    };
    start_services(ServicePlan {
        mode: DevMode::Dev,
        runtime,
        source: ServiceSource::DebugBinary,
    })
}

fn dev_start(options: DevOptions) -> Result<(), String> {
    fs::create_dir_all(DEV_STATE_DIR).map_err(|err| format!("create {DEV_STATE_DIR}: {err}"))?;

    let runtime = DevRuntime::default();
    if options.force {
        stop_services(options.mode.desired_services())?;
    }
    if matches!(options.mode, DevMode::Release) {
        stop_services(&[DevService::Vite])?;
        cargo(["build", "--release"])?;
        npm(["run", "build"])?;
    } else {
        ensure_web_dependencies(options.skip_install)?;
    }

    let source = match options.mode {
        DevMode::Dev => ServiceSource::CargoRun,
        DevMode::Release => ServiceSource::ReleaseBinary,
    };
    start_services(ServicePlan {
        mode: options.mode,
        runtime,
        source,
    })
}

fn dev_stop() -> Result<(), String> {
    stop_services(&[DevService::Vite, DevService::Gateway, DevService::Daemon])?;
    let _ = fs::remove_file(mode_path());
    let _ = fs::remove_file(runtime_path());
    Ok(())
}

fn dev_status() -> Result<(), String> {
    fs::create_dir_all(DEV_STATE_DIR).map_err(|err| format!("create {DEV_STATE_DIR}: {err}"))?;
    let state = read_dev_state()?;
    println!(
        "{:<8} {:<8} {:<8} {:<10} {:<42} url",
        "service", "desired", "pid", "status", "log"
    );
    for service in DevService::all() {
        let pid = read_pid(*service)?;
        let running = pid.map(process_exists).unwrap_or(false);
        if pid.is_some() && !running {
            let _ = fs::remove_file(service.pid_path());
        }
        let desired = if state
            .as_ref()
            .map(|state| state.mode.desired_services().contains(service))
            .unwrap_or(false)
        {
            "yes"
        } else {
            "no"
        };
        let pid_text = pid
            .filter(|_| running)
            .map(|pid| pid.to_string())
            .unwrap_or_else(|| "-".to_string());
        let status = if running { "running" } else { "stopped" };
        println!(
            "{:<8} {:<8} {:<8} {:<10} {:<42} {}",
            service.name(),
            desired,
            pid_text,
            status,
            service.log_path().to_string_lossy(),
            service.url(
                &state
                    .as_ref()
                    .map(|state| state.runtime.clone())
                    .unwrap_or_default(),
            )
        );
    }
    Ok(())
}

fn start_services(plan: ServicePlan) -> Result<(), String> {
    for service in plan.mode.desired_services() {
        ensure_service_startable(*service, &plan.runtime)?;
    }

    write_dev_state(&DevState {
        mode: plan.mode,
        runtime: plan.runtime.clone(),
    })?;

    let mut started = Vec::new();
    for service in plan.mode.desired_services() {
        match start_service(*service, &plan) {
            Ok(true) => started.push(*service),
            Ok(false) => {}
            Err(err) => {
                let _ = stop_services(&started);
                let _ = fs::remove_file(mode_path());
                let _ = fs::remove_file(runtime_path());
                return Err(err);
            }
        }
    }

    dev_status()
}

fn dev_logs(service: Option<DevService>) -> Result<(), String> {
    let services = service
        .map(|service| vec![service])
        .unwrap_or_else(|| DevService::all().to_vec());
    for service in services {
        let path = service.log_path();
        println!("==> {} <==", path.to_string_lossy());
        match tail_file(&path, LOG_TAIL_LINES)? {
            Some(text) if !text.is_empty() => print!("{text}"),
            Some(_) => println!("(empty log)"),
            None => println!("(missing log)"),
        }
    }
    Ok(())
}

fn start_service(service: DevService, plan: &ServicePlan) -> Result<bool, String> {
    if let Some(pid) = read_pid(service)? {
        if process_exists(pid) {
            println!(
                "{} already running with pid {pid}; use --force or restart to replace it",
                service.name()
            );
            return Ok(false);
        }
        fs::remove_file(service.pid_path()).map_err(|err| {
            format!(
                "remove stale pid file {}: {err}",
                service.pid_path().to_string_lossy()
            )
        })?;
    }
    ensure_service_port_available(service, &plan.runtime)?;

    let service_command = service_command(service, plan);
    let log_path = service.log_path();
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|err| format!("open {}: {err}", log_path.to_string_lossy()))?;
    let log_err = log
        .try_clone()
        .map_err(|err| format!("clone {}: {err}", log_path.to_string_lossy()))?;

    let mut command = Command::new(&service_command.program);
    command
        .args(&service_command.args)
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err));
    for (name, value) in service_command.envs {
        command.env(name, value);
    }
    if let Some(cwd) = service_command.cwd {
        command.current_dir(cwd);
    }
    configure_detached_process(&mut command);

    let child = command
        .spawn()
        .map_err(|err| format!("start {}: {err}", service.name()))?;
    let pid = child.id();
    fs::write(service.pid_path(), format!("{pid}\n")).map_err(|err| {
        format!(
            "write pid file {}: {err}",
            service.pid_path().to_string_lossy()
        )
    })?;
    println!(
        "started {} pid {pid}, log {}",
        service.name(),
        log_path.to_string_lossy()
    );
    Ok(true)
}

fn ensure_service_port_available(service: DevService, runtime: &DevRuntime) -> Result<(), String> {
    let addr = match service {
        DevService::Gateway => Some(runtime.gateway_bind()),
        DevService::Vite => Some(runtime.vite_bind()),
        DevService::Daemon => None,
    };
    let Some(addr) = addr else {
        return Ok(());
    };
    TcpListener::bind(&addr).map(|_| ()).map_err(|err| {
        format!(
            "cannot start {}: {addr} is unavailable: {err}",
            service.name()
        )
    })
}

fn ensure_service_startable(service: DevService, runtime: &DevRuntime) -> Result<(), String> {
    if read_pid(service)?.map(process_exists).unwrap_or(false) {
        return Ok(());
    }
    ensure_service_port_available(service, runtime)
}

fn stop_services(services: &[DevService]) -> Result<(), String> {
    for service in services {
        let Some(pid) = read_pid(*service)? else {
            continue;
        };
        if !process_exists(pid) {
            let _ = fs::remove_file(service.pid_path());
            println!("{} stale pid {pid} cleared", service.name());
            continue;
        }

        println!("stopping {} pid {pid}", service.name());
        terminate_process_tree(pid)?;
        if wait_for_exit(pid, Duration::from_secs(5)) {
            let _ = fs::remove_file(service.pid_path());
            continue;
        }

        kill_process_tree(pid)?;
        if wait_for_exit(pid, Duration::from_secs(3)) {
            let _ = fs::remove_file(service.pid_path());
        } else {
            return Err(format!("failed to stop {} pid {pid}", service.name()));
        }
    }
    Ok(())
}

fn service_command(service: DevService, plan: &ServicePlan) -> ServiceCommand {
    match (plan.mode, service, &plan.source) {
        (DevMode::Dev, DevService::Daemon, ServiceSource::CargoRun) => ServiceCommand {
            program: "cargo".to_string(),
            args: vec!["run".to_string(), "--".to_string(), "daemon".to_string()],
            cwd: None,
            envs: vec![],
        },
        (DevMode::Dev, DevService::Daemon, ServiceSource::DebugBinary) => ServiceCommand {
            program: debug_binary_path().to_string_lossy().into_owned(),
            args: vec!["daemon".to_string()],
            cwd: None,
            envs: vec![],
        },
        (DevMode::Dev, DevService::Gateway, ServiceSource::CargoRun) => ServiceCommand {
            program: "cargo".to_string(),
            args: vec![
                "run".to_string(),
                "--".to_string(),
                "gateway".to_string(),
                "--bind".to_string(),
                plan.runtime.gateway_bind(),
            ],
            cwd: None,
            envs: vec![],
        },
        (DevMode::Dev, DevService::Gateway, ServiceSource::DebugBinary) => ServiceCommand {
            program: debug_binary_path().to_string_lossy().into_owned(),
            args: vec![
                "gateway".to_string(),
                "--bind".to_string(),
                plan.runtime.gateway_bind(),
            ],
            cwd: None,
            envs: vec![],
        },
        (DevMode::Dev, DevService::Vite, _) => ServiceCommand {
            program: "npm".to_string(),
            args: vec![
                "run".to_string(),
                "dev".to_string(),
                "--".to_string(),
                "--host".to_string(),
                plan.runtime.bind_host.clone(),
                "--port".to_string(),
                plan.runtime.vite_port.to_string(),
                "--strictPort".to_string(),
            ],
            cwd: Some("app/web"),
            envs: vec![(
                "VITE_API_PROXY_TARGET",
                plan.runtime.vite_api_proxy_target(),
            )],
        },
        (DevMode::Release, DevService::Daemon, ServiceSource::ReleaseBinary) => ServiceCommand {
            program: release_binary_path().to_string_lossy().into_owned(),
            args: vec!["daemon".to_string()],
            cwd: None,
            envs: vec![],
        },
        (DevMode::Release, DevService::Gateway, ServiceSource::ReleaseBinary) => ServiceCommand {
            program: release_binary_path().to_string_lossy().into_owned(),
            args: vec![
                "gateway".to_string(),
                "--bind".to_string(),
                plan.runtime.gateway_bind(),
                "--static-dir".to_string(),
                "app/web/dist".to_string(),
            ],
            cwd: None,
            envs: vec![],
        },
        (DevMode::Release, DevService::Vite, _) => unreachable!("release mode does not start Vite"),
        (DevMode::Release, _, ServiceSource::CargoRun | ServiceSource::DebugBinary) => {
            unreachable!("release services require release binary source")
        }
        (DevMode::Dev, _, ServiceSource::ReleaseBinary) => {
            unreachable!("dev services cannot use release binary source")
        }
    }
}

fn ensure_web_dependencies(skip_install: bool) -> Result<(), String> {
    if skip_install || Path::new("app/web/node_modules").exists() {
        return Ok(());
    }
    npm(["install"])
}

fn choose_smart_runtime() -> Result<DevRuntime, String> {
    Ok(DevRuntime {
        bind_host: DEV_HTTP_BIND_HOST.to_string(),
        gateway_port: choose_free_port(DEFAULT_GATEWAY_PORT, GATEWAY_PORT_END)?,
        vite_port: choose_free_port(DEFAULT_VITE_PORT, VITE_PORT_END)?,
    })
}

fn choose_free_port(start: u16, end: u16) -> Result<u16, String> {
    for port in start..=end {
        if port_is_available(DEV_HTTP_BIND_HOST, port) {
            return Ok(port);
        }
    }
    Err(format!("no available port in range {start}..={end}"))
}

fn cleanup_default_http_ports() -> Result<(), String> {
    cleanup_http_port(DEFAULT_GATEWAY_PORT, "gateway")?;
    cleanup_http_port(DEFAULT_VITE_PORT, "vite")?;
    Ok(())
}

fn cleanup_http_port(port: u16, service: &str) -> Result<(), String> {
    if port_is_available(DEV_HTTP_BIND_HOST, port) {
        return Ok(());
    }

    let owners = port_owners(port)?;
    if owners.is_empty() {
        return Err(format!(
            "cannot start {service}: {DEV_HTTP_BIND_HOST}:{port} is unavailable and no owner could be identified"
        ));
    }

    let unknown = owners
        .iter()
        .filter(|owner| !owner.is_known_project_process())
        .collect::<Vec<_>>();
    if !unknown.is_empty() {
        let details = unknown
            .iter()
            .map(|owner| owner.display())
            .collect::<Vec<_>>()
            .join("; ");
        return Err(format!(
            "cannot start {service}: {DEV_HTTP_BIND_HOST}:{port} is occupied by non-coca process: {details}; use --smart-port or stop it manually"
        ));
    }

    for owner in owners {
        println!(
            "stopping old {} listener on port {port}: {}",
            service,
            owner.display()
        );
        terminate_process_tree(owner.pid)?;
        if !wait_for_exit(owner.pid, Duration::from_secs(5)) {
            kill_process_tree(owner.pid)?;
            if !wait_for_exit(owner.pid, Duration::from_secs(3)) {
                return Err(format!(
                    "failed to stop old {} listener on port {port}: {}",
                    service,
                    owner.display()
                ));
            }
        }
    }

    if port_is_available(DEV_HTTP_BIND_HOST, port) {
        Ok(())
    } else {
        Err(format!(
            "cannot start {service}: {DEV_HTTP_BIND_HOST}:{port} is still unavailable after cleanup"
        ))
    }
}

fn port_is_available(host: &str, port: u16) -> bool {
    TcpListener::bind((host, port)).is_ok()
}

#[derive(Debug)]
struct PortOwner {
    pid: u32,
    command: String,
    cwd: Option<PathBuf>,
}

impl PortOwner {
    fn is_known_project_process(&self) -> bool {
        let command = self.command.to_lowercase();
        if command.contains("coca gateway")
            || command.contains("coca daemon")
            || command.contains("cargo run -- gateway")
            || command.contains("cargo run -- daemon")
        {
            return true;
        }

        let Ok(repo) = env::current_dir() else {
            return false;
        };
        let Some(cwd) = &self.cwd else {
            return false;
        };
        if !cwd.starts_with(&repo) {
            return false;
        }

        command.contains("npm run dev") || command.contains("vite")
    }

    fn display(&self) -> String {
        match &self.cwd {
            Some(cwd) => format!(
                "pid {} `{}` cwd {}",
                self.pid,
                self.command,
                cwd.to_string_lossy()
            ),
            None => format!("pid {} `{}`", self.pid, self.command),
        }
    }
}

fn port_owners(port: u16) -> Result<Vec<PortOwner>, String> {
    let pids = listening_pids(port)?;
    let mut owners = Vec::new();
    for pid in pids {
        owners.push(PortOwner {
            pid,
            command: process_command(pid).unwrap_or_else(|| "<unknown>".to_string()),
            cwd: process_cwd(pid),
        });
    }
    Ok(owners)
}

#[cfg(unix)]
fn listening_pids(port: u16) -> Result<Vec<u32>, String> {
    let Some(status) = command_status(
        "lsof",
        ["-nP", "-iTCP", &format!(":{port}"), "-sTCP:LISTEN", "-t"],
    )?
    else {
        return Ok(Vec::new());
    };
    if !status.success {
        return Ok(Vec::new());
    }
    Ok(status
        .output
        .lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
        .collect())
}

#[cfg(windows)]
fn listening_pids(_port: u16) -> Result<Vec<u32>, String> {
    Ok(Vec::new())
}

#[cfg(not(any(unix, windows)))]
fn listening_pids(_port: u16) -> Result<Vec<u32>, String> {
    Ok(Vec::new())
}

#[cfg(unix)]
fn process_command(pid: u32) -> Option<String> {
    command_status("ps", ["-p", &pid.to_string(), "-o", "command="])
        .ok()
        .flatten()
        .filter(|status| status.success)
        .map(|status| status.output)
}

#[cfg(windows)]
fn process_command(pid: u32) -> Option<String> {
    command_status(
        "wmic",
        [
            "process",
            "where",
            &format!("ProcessId={pid}"),
            "get",
            "CommandLine",
            "/value",
        ],
    )
    .ok()
    .flatten()
    .filter(|status| status.success)
    .map(|status| status.output)
}

#[cfg(not(any(unix, windows)))]
fn process_command(_pid: u32) -> Option<String> {
    None
}

#[cfg(unix)]
fn process_cwd(pid: u32) -> Option<PathBuf> {
    let status = command_status("lsof", ["-a", "-p", &pid.to_string(), "-d", "cwd", "-Fn"])
        .ok()
        .flatten()?;
    if !status.success {
        return None;
    }
    status
        .output
        .lines()
        .find_map(|line| line.strip_prefix('n').map(PathBuf::from))
}

#[cfg(not(unix))]
fn process_cwd(_pid: u32) -> Option<PathBuf> {
    None
}

fn npm<I, S>(args: I) -> Result<(), String>
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
    println!("cd app/web && npm {printable}");

    let status = Command::new("npm")
        .args(args)
        .current_dir("app/web")
        .status()
        .map_err(|err| format!("failed to run npm: {err}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("npm {printable} exited with {status}"))
    }
}

fn read_pid(service: DevService) -> Result<Option<u32>, String> {
    let path = service.pid_path();
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(format!("read {}: {err}", path.to_string_lossy())),
    };
    let pid = text
        .trim()
        .parse::<u32>()
        .map_err(|err| format!("parse {}: {err}", path.to_string_lossy()))?;
    Ok(Some(pid))
}

fn wait_for_exit(pid: u32, timeout: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if !process_exists(pid) {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }
    !process_exists(pid)
}

fn tail_file(path: &Path, lines: usize) -> Result<Option<String>, String> {
    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(format!("open {}: {err}", path.to_string_lossy())),
    };
    let mut text = String::new();
    file.read_to_string(&mut text)
        .map_err(|err| format!("read {}: {err}", path.to_string_lossy()))?;
    let mut tail = text.lines().rev().take(lines).collect::<Vec<_>>();
    tail.reverse();
    let mut output = tail.join("\n");
    if !output.is_empty() {
        output.push('\n');
    }
    Ok(Some(output))
}

fn release_binary_path() -> PathBuf {
    Path::new("target")
        .join("release")
        .join(executable_name(None))
}

fn debug_binary_path() -> PathBuf {
    Path::new("target")
        .join("debug")
        .join(executable_name(None))
}

fn mode_path() -> PathBuf {
    Path::new(DEV_STATE_DIR).join("mode")
}

fn runtime_path() -> PathBuf {
    Path::new(DEV_STATE_DIR).join("runtime")
}

fn write_dev_state(state: &DevState) -> Result<(), String> {
    fs::write(mode_path(), format!("{}\n", state.mode.as_str()))
        .map_err(|err| format!("write {}: {err}", mode_path().to_string_lossy()))?;
    let text = format!(
        "mode={}\nbind_host={}\ngateway_port={}\nvite_port={}\n",
        state.mode.as_str(),
        state.runtime.bind_host,
        state.runtime.gateway_port,
        state.runtime.vite_port
    );
    fs::write(runtime_path(), text)
        .map_err(|err| format!("write {}: {err}", runtime_path().to_string_lossy()))
}

fn read_dev_state() -> Result<Option<DevState>, String> {
    let Some(mode) = read_mode()? else {
        return Ok(None);
    };
    let text = match fs::read_to_string(runtime_path()) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Some(DevState {
                mode,
                runtime: DevRuntime::default(),
            }));
        }
        Err(err) => return Err(format!("read {}: {err}", runtime_path().to_string_lossy())),
    };

    Ok(Some(DevState {
        mode,
        runtime: parse_runtime(&text)?,
    }))
}

fn parse_runtime(text: &str) -> Result<DevRuntime, String> {
    let mut runtime = DevRuntime::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "bind_host" => runtime.bind_host = value.to_string(),
            "gateway_port" => {
                runtime.gateway_port = value
                    .parse::<u16>()
                    .map_err(|err| format!("parse gateway_port: {err}"))?;
            }
            "vite_port" => {
                runtime.vite_port = value
                    .parse::<u16>()
                    .map_err(|err| format!("parse vite_port: {err}"))?;
            }
            "mode" => {}
            _ => {}
        }
    }
    Ok(runtime)
}

fn read_mode() -> Result<Option<DevMode>, String> {
    let text = match fs::read_to_string(mode_path()) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(format!("read {}: {err}", mode_path().to_string_lossy())),
    };
    DevMode::parse(text.trim()).map(Some)
}

fn reject_extra_args(command: &str, args: Vec<String>) -> Result<(), String> {
    if args.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{command} does not accept arguments: {}",
            args.join(" ")
        ))
    }
}

#[cfg(unix)]
fn configure_detached_process(command: &mut Command) {
    command.process_group(0);
}

#[cfg(not(unix))]
fn configure_detached_process(_command: &mut Command) {}

#[cfg(unix)]
fn process_exists(pid: u32) -> bool {
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(windows)]
fn process_exists(pid: u32) -> bool {
    Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}")])
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).contains(&pid.to_string()))
        .unwrap_or(false)
}

#[cfg(not(any(unix, windows)))]
fn process_exists(_pid: u32) -> bool {
    false
}

#[cfg(unix)]
fn terminate_process_tree(pid: u32) -> Result<(), String> {
    signal_process_group_or_pid("TERM", pid)
}

#[cfg(windows)]
fn terminate_process_tree(pid: u32) -> Result<(), String> {
    taskkill(pid, false)
}

#[cfg(not(any(unix, windows)))]
fn terminate_process_tree(_pid: u32) -> Result<(), String> {
    Err("stopping dev services is not implemented for this platform".to_string())
}

#[cfg(unix)]
fn kill_process_tree(pid: u32) -> Result<(), String> {
    signal_process_group_or_pid("KILL", pid)
}

#[cfg(windows)]
fn kill_process_tree(pid: u32) -> Result<(), String> {
    taskkill(pid, true)
}

#[cfg(not(any(unix, windows)))]
fn kill_process_tree(_pid: u32) -> Result<(), String> {
    Err("force-stopping dev services is not implemented for this platform".to_string())
}

#[cfg(unix)]
fn signal_process_group_or_pid(signal: &str, pid: u32) -> Result<(), String> {
    let group = format!("-{pid}");
    let status = Command::new("kill")
        .arg(format!("-{signal}"))
        .arg(&group)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|err| format!("failed to run kill: {err}"))?;
    if status.success() {
        return Ok(());
    }
    let status = Command::new("kill")
        .arg(format!("-{signal}"))
        .arg(pid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|err| format!("failed to run kill: {err}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("kill -{signal} {pid} exited with {status}"))
    }
}

#[cfg(windows)]
fn taskkill(pid: u32, force: bool) -> Result<(), String> {
    let mut args = vec!["/PID".to_string(), pid.to_string(), "/T".to_string()];
    if force {
        args.push("/F".to_string());
    }
    let status = Command::new("taskkill")
        .args(&args)
        .status()
        .map_err(|err| format!("failed to run taskkill: {err}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("taskkill {} exited with {status}", args.join(" ")))
    }
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
  fmt                         Run cargo fmt --all
  check                       Run cargo check --workspace
  test                        Run cargo test --workspace
  clippy                      Run cargo clippy --workspace --all-targets -- -D warnings
  verify                      Run fmt --check, test, and clippy
  run [--smart-port] [--skip-install]
                              Build and run daemon, gateway, and Vite
  build [--release] [--target TARGET]
                              Build the app
  dev <start|stop|restart|status|logs>
                              Manage local daemon/gateway/Vite services
  dist [--target TARGET]      Build release binary and copy it to dist/
  dist-all                    Build dist binaries for known macOS/Linux/Windows targets
  targets                     Print target aliases and dist output names

Target aliases:
  linux-x64, linux-arm64, macos-x64, macos-arm64, windows-x64

Examples:
  cargo xtask verify
  cargo xtask run
  cargo xtask run --smart-port
  cargo xtask dev start
  cargo xtask dev restart --mode release
  cargo xtask dev status
  cargo xtask dist
  cargo xtask dist --target linux-x64
  cargo xtask dist --target windows-x64
"
    );
}

fn print_run_help() {
    println!(
        "\
cargo xtask run [options]

Options:
  --smart-port       Choose available HTTP ports when defaults are occupied
  --skip-install     Do not run npm install when app/web/node_modules is missing

Defaults:
  gateway bind: 0.0.0.0:{DEFAULT_GATEWAY_PORT}
  Vite bind:    0.0.0.0:{DEFAULT_VITE_PORT}
  local URLs:   http://127.0.0.1:{DEFAULT_GATEWAY_PORT}, http://127.0.0.1:{DEFAULT_VITE_PORT}
"
    );
}

fn print_dev_help() {
    println!(
        "\
cargo xtask dev <command>

Commands:
  start [--mode dev|release] [--skip-install] [--force]
                              Start local coca services
  stop                        Stop services managed by xtask
  restart [--mode dev|release] [--skip-install] [--force]
                              Stop then start local coca services
  status                      Print managed service status
  logs [daemon|gateway|vite]  Print recent service logs

Defaults:
  mode:       dev
  gateway:    http://127.0.0.1:{DEFAULT_GATEWAY_PORT}
  Vite:       http://127.0.0.1:{DEFAULT_VITE_PORT}
  state dir:  {DEV_STATE_DIR}

Modes:
  dev         cargo run -- daemon, cargo run -- gateway, npm run dev
  release     cargo build --release, npm run build, target/release/coca daemon/gateway
"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_defaults_to_fixed_ports() {
        let options = RunOptions::parse(vec![]).unwrap().unwrap();

        assert_eq!(
            options,
            RunOptions {
                smart_port: false,
                skip_install: false,
            }
        );
    }

    #[test]
    fn run_parses_smart_port_and_skip_install() {
        let options = RunOptions::parse(vec![
            "--smart-port".to_string(),
            "--skip-install".to_string(),
        ])
        .unwrap()
        .unwrap();

        assert_eq!(
            options,
            RunOptions {
                smart_port: true,
                skip_install: true,
            }
        );
    }

    #[test]
    fn dev_start_defaults_to_dev_mode() {
        let command = DevCommand::parse(vec!["start".to_string()])
            .unwrap()
            .unwrap();

        assert_eq!(
            command,
            DevCommand::Start(DevOptions {
                mode: DevMode::Dev,
                skip_install: false,
                force: false,
            })
        );
    }

    #[test]
    fn dev_restart_parses_release_options() {
        let command = DevCommand::parse(vec![
            "restart".to_string(),
            "--mode".to_string(),
            "release".to_string(),
            "--skip-install".to_string(),
            "--force".to_string(),
        ])
        .unwrap()
        .unwrap();

        assert_eq!(
            command,
            DevCommand::Restart(DevOptions {
                mode: DevMode::Release,
                skip_install: true,
                force: true,
            })
        );
    }

    #[test]
    fn dev_logs_accepts_frontend_alias() {
        let command = DevCommand::parse(vec!["logs".to_string(), "frontend".to_string()])
            .unwrap()
            .unwrap();

        assert_eq!(command, DevCommand::Logs(Some(DevService::Vite)));
    }

    #[test]
    fn dev_mode_desired_services_match_runtime_shape() {
        assert_eq!(
            DevMode::Dev.desired_services(),
            &[DevService::Daemon, DevService::Gateway, DevService::Vite]
        );
        assert_eq!(
            DevMode::Release.desired_services(),
            &[DevService::Daemon, DevService::Gateway]
        );
    }

    #[test]
    fn runtime_defaults_bind_http_services_publicly() {
        let runtime = DevRuntime::default();

        assert_eq!(runtime.gateway_bind(), "0.0.0.0:8787");
        assert_eq!(runtime.vite_bind(), "0.0.0.0:5173");
        assert_eq!(runtime.gateway_local_url(), "http://127.0.0.1:8787");
        assert_eq!(runtime.vite_api_proxy_target(), "http://127.0.0.1:8787");
    }

    #[test]
    fn parse_runtime_reads_selected_ports() {
        let runtime =
            parse_runtime("mode=dev\nbind_host=0.0.0.0\ngateway_port=8790\nvite_port=5180\n")
                .unwrap();

        assert_eq!(runtime.bind_host, "0.0.0.0");
        assert_eq!(runtime.gateway_port, 8790);
        assert_eq!(runtime.vite_port, 5180);
    }
}
