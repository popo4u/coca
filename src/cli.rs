use std::path::PathBuf;

use clap::{Parser, ValueEnum};

use crate::model::ProviderFilter;

#[derive(Debug, Parser)]
#[command(author, version, about = "Unified TUI for Codex and Claude sessions")]
pub struct Cli {
    #[arg(long, value_name = "DIR")]
    codex_home: Option<PathBuf>,

    #[arg(long, value_name = "DIR")]
    claude_home: Option<PathBuf>,

    #[arg(long, value_enum, default_value_t = ProviderArg::All)]
    provider: ProviderArg,
}

impl Cli {
    pub fn parse_args() -> Self {
        Self::parse()
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
