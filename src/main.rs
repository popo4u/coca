mod cli;
mod core_client;
mod process;

use anyhow::{Context, Result};
use coca_app::AppOptions;
use coca_core::settings::load_settings_for_cli;
use coca_daemon::{serve as serve_core, serve_rpc, CoreOptions, RpcDaemonOptions};
use coca_tui::run_tui;
use coca_web::{serve as serve_web, WebCache, WebOptions};

use crate::cli::{Cli, Command};
use crate::core_client::RpcCoreClient;
use crate::process::exec_resume;

fn main() -> Result<()> {
    let cli = Cli::parse_args();
    let (settings, settings_path) = load_settings_for_cli(cli.remote_config().as_deref())?;
    if let Some(command) = cli.command() {
        return match command {
            Command::Core(args) => serve_core(CoreOptions {
                bind: args.bind().unwrap_or_else(|| settings.core.bind.clone()),
                token: settings.share.token.clone(),
                codex_home: args.codex_home(),
                claude_home: args.claude_home(),
                provider_filter: args.provider_filter(),
            }),
            Command::Daemon(args) => {
                let socket = args.socket().context(
                    "failed to resolve daemon socket path: home directory was not found",
                )?;
                serve_rpc(
                    &socket,
                    RpcDaemonOptions {
                        settings,
                        settings_path: Some(settings_path),
                        codex_home: args.codex_home(),
                        claude_home: args.claude_home(),
                        provider_filter: args.provider_filter(),
                    },
                )
            }
            Command::Web(args) => serve_web(WebOptions {
                bind: args.bind().unwrap_or_else(|| settings.core.bind.clone()),
                app: AppOptions {
                    settings,
                    settings_path: Some(settings_path),
                    codex_home: args.codex_home(),
                    claude_home: args.claude_home(),
                    provider_filter: args.provider_filter(),
                    database_path: None,
                },
                static_dir: args.static_dir().unwrap_or_else(default_web_static_dir),
                cache: WebCache::default(),
            }),
        };
    }

    let provider_filter = cli.provider_filter();
    let core_client = RpcCoreClient::new(RpcDaemonOptions {
        settings,
        settings_path: Some(settings_path),
        codex_home: cli.codex_home(),
        claude_home: cli.claude_home(),
        provider_filter,
    });

    if let Some(target) = run_tui(Box::new(core_client), provider_filter)? {
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
