mod cli;
mod launch;
mod model;
mod process;
mod providers;
mod remote;
mod tui;

use anyhow::Result;

use crate::cli::{Cli, ClientCommand, Command};
use crate::process::exec_resume;
use crate::providers::{load_sessions, sort_sessions};
use crate::remote::{load_remote_config_for_cli, load_remote_sessions, serve, ServeOptions};
use crate::tui::run_tui;

fn main() -> Result<()> {
    let cli = Cli::parse_args();
    if let Some(command) = cli.command() {
        return match command {
            Command::Client(client) => match client.command() {
                ClientCommand::Serve(args) => serve(ServeOptions {
                    bind: args.bind(),
                    token: args.token(),
                    codex_home: args.codex_home(),
                    claude_home: args.claude_home(),
                    provider_filter: args.provider_filter(),
                }),
            },
        };
    }

    let provider_filter = cli.provider_filter();
    let codex_home = cli.codex_home();
    let claude_home = cli.claude_home();

    let mut sessions = load_sessions(
        codex_home.as_deref(),
        claude_home.as_deref(),
        provider_filter,
    )?;
    let remote_config = load_remote_config_for_cli(cli.remote_config().as_deref())?;
    let (mut remote_sessions, warnings) = load_remote_sessions(&remote_config);
    sessions.append(&mut remote_sessions);
    sort_sessions(&mut sessions);

    if let Some(target) = run_tui(sessions, provider_filter, warnings)? {
        exec_resume(target)?;
    }

    Ok(())
}
