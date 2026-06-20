use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::model::{ProviderFilter, Session};
use crate::providers::{load_sessions, sort_sessions};
use crate::remote::{load_remote_sessions, RemoteConfig};

#[derive(Clone, Debug)]
pub struct SessionCatalogOptions {
    pub codex_home: Option<PathBuf>,
    pub claude_home: Option<PathBuf>,
    pub provider_filter: ProviderFilter,
    pub remote_config: RemoteConfig,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionCatalog {
    pub sessions: Vec<Session>,
    pub warnings: Vec<String>,
}

pub fn load_session_catalog(options: SessionCatalogOptions) -> Result<SessionCatalog> {
    let mut sessions = load_sessions(
        options.codex_home.as_deref(),
        options.claude_home.as_deref(),
        options.provider_filter,
    )?;
    let (mut remote_sessions, warnings) = load_remote_sessions(&options.remote_config);
    sessions.append(&mut remote_sessions);
    sort_sessions(&mut sessions);

    Ok(SessionCatalog { sessions, warnings })
}
