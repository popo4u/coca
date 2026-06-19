mod claude;
mod codex;

use std::path::Path;

use anyhow::Result;

use crate::model::{ProviderFilter, ProviderKind, Session};

pub use claude::load_claude_sessions;
pub use codex::load_codex_sessions;

pub trait SessionProvider {
    fn load_sessions(&self) -> Result<Vec<Session>>;
}

pub struct CodexProvider<'a> {
    home: &'a Path,
}

impl<'a> CodexProvider<'a> {
    pub fn new(home: &'a Path) -> Self {
        Self { home }
    }
}

impl SessionProvider for CodexProvider<'_> {
    fn load_sessions(&self) -> Result<Vec<Session>> {
        load_codex_sessions(self.home)
    }
}

pub struct ClaudeProvider<'a> {
    home: &'a Path,
}

impl<'a> ClaudeProvider<'a> {
    pub fn new(home: &'a Path) -> Self {
        Self { home }
    }
}

impl SessionProvider for ClaudeProvider<'_> {
    fn load_sessions(&self) -> Result<Vec<Session>> {
        load_claude_sessions(self.home)
    }
}

pub fn load_sessions(
    codex_home: Option<&Path>,
    claude_home: Option<&Path>,
    filter: ProviderFilter,
) -> Result<Vec<Session>> {
    let mut sessions = Vec::new();

    if filter.includes(ProviderKind::Codex) {
        if let Some(home) = codex_home {
            sessions.extend(CodexProvider::new(home).load_sessions()?);
        }
    }

    if filter.includes(ProviderKind::Claude) {
        if let Some(home) = claude_home {
            sessions.extend(ClaudeProvider::new(home).load_sessions()?);
        }
    }

    sort_sessions(&mut sessions);
    Ok(sessions)
}

pub fn sort_sessions(sessions: &mut [Session]) {
    sessions.sort_by(|a, b| {
        b.updated_at_ms
            .or(b.created_at_ms)
            .cmp(&a.updated_at_ms.or(a.created_at_ms))
            .then_with(|| a.origin.to_string().cmp(&b.origin.to_string()))
            .then_with(|| a.provider.to_string().cmp(&b.provider.to_string()))
            .then_with(|| a.id.cmp(&b.id))
    });
}
