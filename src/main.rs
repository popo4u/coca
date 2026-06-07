mod cli;
mod launch;
mod model;
mod process;
mod providers;
mod tui;

use anyhow::Result;

use crate::cli::Cli;
use crate::process::exec_resume;
use crate::providers::load_sessions;
use crate::tui::run_tui;

fn main() -> Result<()> {
    let cli = Cli::parse_args();
    let provider_filter = cli.provider_filter();
    let codex_home = cli.codex_home();
    let claude_home = cli.claude_home();

    let sessions = load_sessions(
        codex_home.as_deref(),
        claude_home.as_deref(),
        provider_filter,
    )?;
    if let Some(target) = run_tui(sessions, provider_filter)? {
        exec_resume(target)?;
    }

    Ok(())
}
