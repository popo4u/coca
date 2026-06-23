use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use coca_core::model::ProviderFilter;

#[derive(Debug, Parser)]
#[command(author, version, about = "Unified TUI for Codex and Claude sessions")]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    #[arg(long, value_name = "DIR")]
    codex_home: Option<PathBuf>,

    #[arg(long, value_name = "DIR")]
    claude_home: Option<PathBuf>,

    #[arg(long, value_enum, default_value_t = ProviderArg::All)]
    provider: ProviderArg,

    #[arg(long, value_name = "FILE")]
    remote_config: Option<PathBuf>,
}

impl Cli {
    pub fn parse_args() -> Self {
        Self::parse()
    }

    pub fn command(&self) -> Option<&Command> {
        self.command.as_ref()
    }

    pub fn provider_filter(&self) -> ProviderFilter {
        self.provider.into()
    }

    pub fn codex_home(&self) -> Option<PathBuf> {
        self.codex_home
            .clone()
            .or_else(|| dirs::home_dir().map(|home| home.join(".codex")))
    }

    pub fn claude_home(&self) -> Option<PathBuf> {
        self.claude_home
            .clone()
            .or_else(|| dirs::home_dir().map(|home| home.join(".claude")))
    }

    pub fn remote_config(&self) -> Option<PathBuf> {
        self.remote_config.clone()
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ProviderArg {
    All,
    Codex,
    Claude,
}

impl From<ProviderArg> for ProviderFilter {
    fn from(value: ProviderArg) -> Self {
        match value {
            ProviderArg::All => ProviderFilter::All,
            ProviderArg::Codex => ProviderFilter::Codex,
            ProviderArg::Claude => ProviderFilter::Claude,
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Daemon(DaemonArgs),
    Gateway(GatewayArgs),
    Tui,
}

#[derive(Debug, Args)]
pub struct DaemonArgs {
    #[arg(long, value_name = "PATH")]
    socket: Option<PathBuf>,

    #[arg(long, value_name = "PATH")]
    terminal_socket: Option<PathBuf>,

    #[arg(long, value_name = "DIR")]
    codex_home: Option<PathBuf>,

    #[arg(long, value_name = "DIR")]
    claude_home: Option<PathBuf>,

    #[arg(long, value_enum, default_value_t = ProviderArg::All)]
    provider: ProviderArg,
}

impl DaemonArgs {
    pub fn socket(&self) -> Option<PathBuf> {
        self.socket.clone()
    }

    pub fn terminal_socket(&self) -> Option<PathBuf> {
        self.terminal_socket.clone()
    }

    pub fn codex_home(&self) -> Option<PathBuf> {
        self.codex_home
            .clone()
            .or_else(|| dirs::home_dir().map(|home| home.join(".codex")))
    }

    pub fn claude_home(&self) -> Option<PathBuf> {
        self.claude_home
            .clone()
            .or_else(|| dirs::home_dir().map(|home| home.join(".claude")))
    }

    pub fn provider_filter(&self) -> ProviderFilter {
        self.provider.into()
    }
}

#[derive(Debug, Args)]
pub struct GatewayArgs {
    #[arg(long, value_name = "HOST:PORT")]
    bind: Option<String>,

    #[arg(long, value_name = "DIR")]
    static_dir: Option<PathBuf>,

    #[arg(long, value_name = "PATH")]
    daemon_socket: Option<PathBuf>,

    #[arg(long, value_name = "PATH")]
    terminal_socket: Option<PathBuf>,
}

impl GatewayArgs {
    pub fn bind(&self) -> Option<String> {
        self.bind.clone()
    }

    pub fn static_dir(&self) -> Option<PathBuf> {
        self.static_dir.clone()
    }

    pub fn daemon_socket(&self) -> Option<PathBuf> {
        self.daemon_socket.clone()
    }

    pub fn terminal_socket(&self) -> Option<PathBuf> {
        self.terminal_socket.clone()
    }
}
