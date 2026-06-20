mod cli;
mod process;

use anyhow::{Context, Result};
use coca_core::catalog::{load_session_catalog, SessionCatalogOptions};
use coca_core::settings::load_settings_for_cli;
use coca_daemon::{serve as serve_core, serve_rpc, CoreOptions, RpcDaemonOptions};
use coca_tui::run_tui;

use crate::cli::{Cli, Command};
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
        };
    }

    let provider_filter = cli.provider_filter();
    let codex_home = cli.codex_home();
    let claude_home = cli.claude_home();

    let remote_config = settings.remote_config();
    let catalog = load_session_catalog(SessionCatalogOptions {
        codex_home,
        claude_home,
        provider_filter,
        remote_config,
    })?;

    if let Some(target) = run_tui(
        catalog.sessions,
        provider_filter,
        catalog.warnings,
        settings,
        settings_path,
    )? {
        exec_resume(target)?;
    }

    Ok(())
}
