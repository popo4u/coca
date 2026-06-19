use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::model::ProviderFilter;

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
    Client(ClientArgs),
}

#[derive(Debug, Args)]
pub struct ClientArgs {
    #[command(subcommand)]
    command: ClientCommand,
}

impl ClientArgs {
    pub fn command(&self) -> &ClientCommand {
        &self.command
    }
}

#[derive(Debug, Subcommand)]
pub enum ClientCommand {
    Serve(ServeArgs),
}

#[derive(Debug, Args)]
pub struct ServeArgs {
    #[arg(long, value_name = "HOST:PORT")]
    bind: String,

    #[arg(long)]
    token: String,

    #[arg(long, value_name = "DIR")]
    codex_home: Option<PathBuf>,

    #[arg(long, value_name = "DIR")]
    claude_home: Option<PathBuf>,

    #[arg(long, value_enum, default_value_t = ProviderArg::All)]
    provider: ProviderArg,
}

impl ServeArgs {
    pub fn bind(&self) -> String {
        self.bind.clone()
    }

    pub fn token(&self) -> String {
        self.token.clone()
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
