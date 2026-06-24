mod cli;
mod daemon_client;
mod process;

use std::path::PathBuf;

use anyhow::Result;
use coca_core::settings::load_settings_for_cli;
use coca_daemon::{serve_daemon, RpcDaemonOptions};
use coca_tui::run_tui;
use coca_web::{serve as serve_gateway, GatewayOptions};

use crate::cli::{Cli, Command};
use crate::daemon_client::RpcDaemonClient;
use crate::process::exec_resume;

fn main() -> Result<()> {
    let cli = Cli::parse_args();
    let (settings, settings_path) = load_settings_for_cli(cli.remote_config().as_deref())?;
    if let Some(command) = cli.command() {
        return match command {
            Command::Daemon(args) => {
                let socket = args
                    .socket()
                    .unwrap_or_else(|| PathBuf::from(settings.daemon.socket.clone()));
                let terminal_socket = args
                    .terminal_socket()
                    .unwrap_or_else(|| PathBuf::from(settings.daemon.terminal_socket.clone()));
                serve_daemon(
                    &socket,
                    &terminal_socket,
                    RpcDaemonOptions {
                        settings,
                        settings_path: Some(settings_path),
                        codex_home: args.codex_home(),
                        claude_home: args.claude_home(),
                        provider_filter: args.provider_filter(),
                        database_path: None,
                    },
                )
            }
            Command::Gateway(args) => {
                let bind = args.bind().unwrap_or_else(|| settings.gateway.bind.clone());
                let daemon_socket = args
                    .daemon_socket()
                    .unwrap_or_else(|| PathBuf::from(settings.daemon.socket.clone()));
                let terminal_socket = args
                    .terminal_socket()
                    .unwrap_or_else(|| PathBuf::from(settings.daemon.terminal_socket.clone()));
                serve_gateway(GatewayOptions {
                    bind,
                    read_token: settings.share.token.clone(),
                    share_base_url: settings.share.base_url.clone(),
                    terminal_enabled: settings.terminal.enabled,
                    terminal_token: settings.terminal.token.clone(),
                    static_dir: args.static_dir().unwrap_or_else(default_web_static_dir),
                    daemon_socket: Some(daemon_socket),
                    terminal_socket: Some(terminal_socket),
                })
            }
            Command::Tui => run_tui_command(&cli, settings, settings_path),
        };
    }

    run_tui_command(&cli, settings, settings_path)
}

fn run_tui_command(
    cli: &Cli,
    settings: coca_core::settings::Settings,
    settings_path: std::path::PathBuf,
) -> Result<()> {
    let provider_filter = cli.provider_filter();
    let daemon_client = RpcDaemonClient::new(RpcDaemonOptions {
        settings,
        settings_path: Some(settings_path),
        codex_home: cli.codex_home(),
        claude_home: cli.claude_home(),
        provider_filter,
        database_path: None,
    });

    if let Some(target) = run_tui(Box::new(daemon_client), provider_filter)? {
        exec_resume(target)?;
    }

    Ok(())
}

fn default_web_static_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("app")
        .join("web")
        .join("dist")
}
