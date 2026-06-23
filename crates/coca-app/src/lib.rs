use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::{DateTime, Local};
use coca_core::catalog::{load_session_catalog, SessionCatalog, SessionCatalogOptions};
use coca_core::launch::{
    build_launch_target, default_resume_target, launch_options, LaunchMode, LaunchOption,
    ResumeTarget,
};
use coca_core::model::{ProviderFilter, ProviderKind, Session, SessionOrigin};
use coca_core::settings::{save_settings, AiSettings, Settings};
use coca_core::storage::{default_database_path, DerivedStore};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub struct AppOptions {
    pub settings: Settings,
    pub settings_path: Option<PathBuf>,
    pub codex_home: Option<PathBuf>,
    pub claude_home: Option<PathBuf>,
    pub provider_filter: ProviderFilter,
    pub database_path: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct AppService {
    options: AppOptions,
}

impl AppService {
    pub fn new(options: AppOptions) -> Self {
        Self { options }
    }

    pub fn settings(&self) -> Settings {
        self.options.settings.clone()
    }

    pub fn update_settings(&mut self, mut settings: Settings) -> Result<Settings> {
        settings.ensure_defaults();
        settings.validate()?;
        if let Some(path) = &self.options.settings_path {
            save_settings(path, &settings)?;
        }
        self.options.settings = settings.clone();
        Ok(settings)
    }

    pub fn update_ai_settings(&mut self, update: AiSettingsUpdate) -> Result<AiSummary> {
        let mut settings = self.options.settings.clone();
        settings.ensure_defaults();
        if let Some(enabled) = update.enabled {
            settings.ai.enabled = enabled;
        }
        if let Some(provider) = update.provider {
            settings.ai.provider = provider.trim().to_string();
        }
        if let Some(base_url) = update.base_url {
            settings.ai.base_url = base_url.trim().to_string();
        }
        if let Some(model) = update.model {
            settings.ai.model = model.trim().to_string();
        }
        if let Some(api_key_env) = update.api_key_env {
            settings.ai.api_key_env = api_key_env.trim().to_string();
        }
        if update.clear_api_key {
            settings.ai.api_key.clear();
        } else if let Some(api_key) = update.api_key {
            let api_key = api_key.trim();
            if !api_key.is_empty() {
                settings.ai.api_key = api_key.to_string();
            }
        }

        let settings = self.update_settings(settings)?;
        Ok(AiSummary::from_settings(&settings.ai))
    }

    pub fn session_catalog(&self) -> Result<SessionCatalog> {
        let mut catalog = load_session_catalog(SessionCatalogOptions {
            codex_home: self.options.codex_home.clone(),
            claude_home: self.options.claude_home.clone(),
            provider_filter: self.options.provider_filter,
            remote_config: self.options.settings.remote_config(),
        })?;
        if let Err(err) = self.store_catalog(&catalog.sessions) {
            catalog
                .warnings
                .push(format!("failed to update coca storage: {err:#}"));
        }
        Ok(catalog)
    }

    pub fn session(&self, reference: &SessionRef) -> Result<Option<Session>> {
        let provider = parse_provider(&reference.provider)?;
        Ok(self
            .session_catalog()?
            .sessions
            .into_iter()
            .find(|session| {
                session.provider == provider
                    && session.id == reference.id
                    && origin_matches(&session.origin, &reference.origin)
            }))
    }

    pub fn config_summary(&self, bind: &str) -> Result<ConfigSummary> {
        let catalog = self.session_catalog()?;
        Ok(ConfigSummary::from_parts(
            &self.options.settings,
            bind,
            &catalog,
        ))
    }

    pub fn web_sessions(&self) -> Result<SessionsResponse> {
        let catalog = self.session_catalog()?;
        Ok(SessionsResponse::from_catalog_with_settings(
            catalog,
            &self.options.settings,
        ))
    }

    pub fn stored_web_sessions(&self) -> Result<Option<SessionsResponse>> {
        let Some(path) = self.database_path() else {
            return Ok(None);
        };
        if !path.exists() {
            return Ok(None);
        }
        let store = DerivedStore::open(&path)?;
        let sessions = store.sessions()?;
        if sessions.is_empty() {
            return Ok(None);
        }
        Ok(Some(SessionsResponse::from_catalog_with_settings(
            SessionCatalog {
                sessions,
                warnings: vec![
                    "serving sessions from coca storage while refreshing in background".to_string(),
                ],
            },
            &self.options.settings,
        )))
    }

    pub fn web_session_detail(&self, reference: &SessionRef) -> Result<Option<SessionDetail>> {
        Ok(self.session(reference)?.map(|session| {
            SessionDetail::from_session_with_settings(session, &self.options.settings)
        }))
    }

    pub fn share_session(&self, reference: &SessionRef) -> Result<ShareLink> {
        let session = self
            .session(reference)?
            .ok_or_else(|| anyhow::anyhow!("session not found"))?;
        self.share_link_for_session(&session)
    }

    pub fn share_link_for_session(&self, session: &Session) -> Result<ShareLink> {
        if !session.is_local() {
            anyhow::bail!(
                "Remote sessions cannot be shared from this machine in v0: {}",
                session.origin
            );
        }
        let base_url = self.options.settings.share.base_url.trim();
        let token = self.options.settings.share.token.trim();
        if base_url.is_empty() || token.is_empty() {
            anyhow::bail!("share base_url and token must be configured");
        }
        let reference = session_ref_from_session(session);
        Ok(ShareLink {
            url: format!(
                "{}/?token={}#/session/{}/{}/{}",
                base_url.trim_end_matches('/'),
                percent_encode(token),
                percent_encode(&reference.origin),
                percent_encode(&reference.provider),
                percent_encode(&reference.id),
            ),
        })
    }

    pub fn launch_options_with_defaults(
        &self,
        session: &Session,
        mode: LaunchMode,
        current_cwd: &Path,
    ) -> Result<Vec<LaunchOption>> {
        ensure_local(session)?;
        let mut options = launch_options(session, current_cwd);
        for option in &mut options {
            option.enabled = self.options.settings.launch_default(mode, option.kind);
        }
        Ok(options)
    }

    pub fn prepare_launch(
        &self,
        session: &Session,
        mode: LaunchMode,
        current_cwd: &Path,
        options: &[LaunchOption],
    ) -> Result<ResumeTarget> {
        ensure_local(session)?;
        Ok(build_launch_target(session, mode, current_cwd, options))
    }

    pub fn prepare_terminal_launch(
        &self,
        session: &Session,
        mode: LaunchMode,
    ) -> Result<ResumeTarget> {
        let current_cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::new());
        self.prepare_terminal_launch_with_cwd(session, mode, &current_cwd)
    }

    pub fn prepare_terminal_launch_with_cwd(
        &self,
        session: &Session,
        mode: LaunchMode,
        current_cwd: &Path,
    ) -> Result<ResumeTarget> {
        ensure_terminal_enabled(&self.options.settings)?;
        ensure_local(session)?;
        let options = self.launch_options_with_defaults(session, mode, current_cwd)?;
        Ok(build_launch_target(session, mode, current_cwd, &options))
    }

    pub fn default_resume_for_session(&self, session: &Session) -> Result<ResumeTarget> {
        ensure_local(session)?;
        Ok(default_resume_target(session))
    }

    fn store_catalog(&self, sessions: &[Session]) -> Result<()> {
        let Some(path) = self.database_path() else {
            return Ok(());
        };
        let mut store = DerivedStore::open(&path)?;
        store.replace_sessions(sessions)
    }

    fn database_path(&self) -> Option<PathBuf> {
        self.options
            .database_path
            .clone()
            .or_else(default_database_path)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionRef {
    pub origin: String,
    pub provider: String,
    pub id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionsResponse {
    pub sessions: Vec<SessionSummary>,
    pub warnings: Vec<String>,
    pub counts: CatalogCounts,
}

impl SessionsResponse {
    fn from_catalog_with_settings(catalog: SessionCatalog, settings: &Settings) -> Self {
        Self::from_catalog_inner(catalog, Some(settings))
    }

    fn from_catalog_inner(catalog: SessionCatalog, settings: Option<&Settings>) -> Self {
        let counts = CatalogCounts::from_sessions(&catalog.sessions);
        Self {
            sessions: catalog
                .sessions
                .into_iter()
                .map(|session| SessionSummary::from_session(session, settings))
                .collect(),
            warnings: catalog.warnings,
            counts,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct CatalogCounts {
    pub total: usize,
    pub by_provider: BTreeMap<String, usize>,
    pub by_origin: BTreeMap<String, usize>,
}

impl CatalogCounts {
    fn from_sessions(sessions: &[Session]) -> Self {
        let mut counts = CatalogCounts {
            total: sessions.len(),
            ..CatalogCounts::default()
        };
        for session in sessions {
            *counts
                .by_provider
                .entry(session.provider.to_string())
                .or_insert(0) += 1;
            *counts
                .by_origin
                .entry(session.origin.to_string())
                .or_insert(0) += 1;
        }
        counts
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionSummary {
    pub origin: String,
    pub provider: String,
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub updated_at_ms: Option<i64>,
    pub updated_label: String,
    pub model: Option<String>,
    pub message_count: usize,
    pub first_user_message: Option<String>,
    pub terminal: TerminalCapability,
}

impl SessionSummary {
    fn from_session(session: Session, settings: Option<&Settings>) -> Self {
        let terminal = settings
            .map(|settings| TerminalCapability::for_session(&session, settings))
            .unwrap_or_else(|| TerminalCapability::browse_only("terminal status unavailable"));
        Self {
            origin: session.origin.to_string(),
            provider: session.provider.to_string(),
            id: session.id,
            title: session.title,
            cwd: session.cwd,
            updated_at_ms: session.updated_at_ms,
            updated_label: format_time(session.updated_at_ms),
            model: session.model,
            message_count: session.transcript.len(),
            first_user_message: session.first_user_message,
            terminal,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionDetail {
    pub summary: SessionSummary,
    pub transcript: Vec<ChatMessageDto>,
}

impl SessionDetail {
    #[cfg(test)]
    fn from_session(session: Session) -> Self {
        Self::from_session_inner(session, None)
    }

    fn from_session_with_settings(session: Session, settings: &Settings) -> Self {
        Self::from_session_inner(session, Some(settings))
    }

    fn from_session_inner(session: Session, settings: Option<&Settings>) -> Self {
        let transcript = session
            .transcript
            .iter()
            .filter(|message| match session.first_user_message.as_deref() {
                Some(prompt) => !(message.role == "user" && message.text.trim() == prompt.trim()),
                None => true,
            })
            .map(ChatMessageDto::from_message)
            .collect();
        Self {
            summary: SessionSummary::from_session(session, settings),
            transcript,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChatMessageDto {
    pub role: String,
    pub display_role: String,
    pub text: String,
    pub timestamp_ms: Option<i64>,
    pub timestamp_label: String,
}

impl ChatMessageDto {
    fn from_message(message: &coca_core::model::ChatMessage) -> Self {
        Self {
            role: message.role.clone(),
            display_role: display_role(&message.role, &message.text).to_string(),
            text: message.text.clone(),
            timestamp_ms: message.timestamp_ms,
            timestamp_label: format_time(message.timestamp_ms),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConfigSummary {
    pub service: String,
    pub version: String,
    pub bind: String,
    pub gateway_bind: String,
    pub ai: AiSummary,
    pub share: ShareSummary,
    pub terminal: TerminalConfigSummary,
    pub remotes: Vec<RemoteSummary>,
    pub launch_defaults: LaunchDefaultsSummary,
    pub counts: CatalogCounts,
    pub warnings: Vec<String>,
}

impl ConfigSummary {
    fn from_parts(settings: &Settings, bind: &str, catalog: &SessionCatalog) -> Self {
        let counts = CatalogCounts::from_sessions(&catalog.sessions);
        Self {
            service: "coca-gateway".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            bind: bind.to_string(),
            gateway_bind: settings.gateway.bind.clone(),
            ai: AiSummary::from_settings(&settings.ai),
            share: ShareSummary {
                base_url: settings.share.base_url.clone(),
                token_configured: !settings.share.token.trim().is_empty(),
            },
            terminal: TerminalConfigSummary::from_settings(settings),
            remotes: settings
                .remotes
                .iter()
                .map(|remote| {
                    let token_configured = !remote.token.trim().is_empty();
                    let terminal_token_configured = remote
                        .terminal_token
                        .as_deref()
                        .map(|token| !token.trim().is_empty())
                        .unwrap_or(false);
                    let terminal_unavailable = remote_terminal_unavailable(
                        remote.enabled,
                        token_configured,
                        terminal_token_configured,
                    );
                    RemoteSummary {
                        name: remote.name.clone(),
                        base_url: remote.base_url.clone(),
                        enabled: remote.enabled,
                        visible: settings
                            .origin_visible(&SessionOrigin::Remote(remote.name.clone())),
                        token_configured,
                        terminal_token_configured,
                        terminal_ready: terminal_unavailable.is_none(),
                        terminal_unavailable_code: terminal_unavailable
                            .as_ref()
                            .map(|(code, _)| (*code).to_string()),
                        terminal_unavailable_message: terminal_unavailable
                            .as_ref()
                            .map(|(_, message)| (*message).to_string()),
                        session_count: counts
                            .by_origin
                            .get(&remote.name)
                            .copied()
                            .unwrap_or_default(),
                    }
                })
                .collect(),
            launch_defaults: LaunchDefaultsSummary {
                resume: LaunchModeDefaultsSummary {
                    use_current_dir: settings.launch_defaults.resume.use_current_dir,
                    yolo: settings.launch_defaults.resume.yolo,
                },
                fork: LaunchModeDefaultsSummary {
                    use_current_dir: settings.launch_defaults.fork.use_current_dir,
                    yolo: settings.launch_defaults.fork.yolo,
                },
            },
            counts,
            warnings: catalog.warnings.clone(),
        }
    }

    pub fn with_terminal_runtime(
        mut self,
        daemon_available: bool,
        terminal_socket_available: bool,
    ) -> Self {
        self.terminal = self
            .terminal
            .with_runtime(daemon_available, terminal_socket_available);
        self
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct AiSettingsUpdate {
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub enabled: Option<bool>,
    pub provider: Option<String>,
    pub api_key_env: Option<String>,
    pub api_key: Option<String>,
    #[serde(default)]
    pub clear_api_key: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AiSummary {
    pub base_url: String,
    pub model: String,
    pub enabled: bool,
    pub provider: String,
    pub api_key_env: String,
    pub api_key_configured: bool,
    pub key_source: String,
}

impl AiSummary {
    fn from_settings(settings: &AiSettings) -> Self {
        Self {
            base_url: settings.base_url.clone(),
            model: settings.model.clone(),
            enabled: settings.enabled,
            provider: settings.provider.clone(),
            api_key_env: settings.api_key_env.clone(),
            api_key_configured: settings.key_configured(),
            key_source: settings.key_source().to_string(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ShareSummary {
    pub base_url: String,
    pub token_configured: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TerminalConfigSummary {
    pub enabled: bool,
    pub token_configured: bool,
    pub daemon_available: bool,
    pub terminal_socket_available: bool,
    pub unavailable_code: Option<String>,
    pub unavailable_message: Option<String>,
}

impl TerminalConfigSummary {
    fn from_settings(settings: &Settings) -> Self {
        Self {
            enabled: settings.terminal.enabled,
            token_configured: settings.terminal.token_configured(),
            daemon_available: false,
            terminal_socket_available: false,
            unavailable_code: None,
            unavailable_message: None,
        }
        .refresh_unavailable()
    }

    fn with_runtime(mut self, daemon_available: bool, terminal_socket_available: bool) -> Self {
        self.daemon_available = daemon_available;
        self.terminal_socket_available = terminal_socket_available;
        self.refresh_unavailable()
    }

    fn refresh_unavailable(mut self) -> Self {
        let unavailable = if !self.enabled {
            Some((
                "terminal_disabled",
                "Terminal execution is disabled in settings.",
            ))
        } else if !self.token_configured {
            Some((
                "missing_terminal_token",
                "Terminal token is not configured.",
            ))
        } else if !self.daemon_available {
            Some(("daemon_unavailable", "coca daemon is not available."))
        } else if !self.terminal_socket_available {
            Some((
                "terminal_socket_unavailable",
                "coca daemon terminal socket is not available.",
            ))
        } else {
            None
        };
        self.unavailable_code = unavailable.as_ref().map(|(code, _)| (*code).to_string());
        self.unavailable_message = unavailable
            .as_ref()
            .map(|(_, message)| (*message).to_string());
        self
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteSummary {
    pub name: String,
    pub base_url: String,
    pub enabled: bool,
    pub visible: bool,
    pub token_configured: bool,
    pub terminal_token_configured: bool,
    pub terminal_ready: bool,
    pub terminal_unavailable_code: Option<String>,
    pub terminal_unavailable_message: Option<String>,
    pub session_count: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LaunchDefaultsSummary {
    pub resume: LaunchModeDefaultsSummary,
    pub fork: LaunchModeDefaultsSummary,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LaunchModeDefaultsSummary {
    pub use_current_dir: bool,
    pub yolo: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ShareLink {
    pub url: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StreamInfo {
    pub protocol: &'static str,
    pub client_events: Vec<&'static str>,
    pub server_events: Vec<&'static str>,
}

impl Default for StreamInfo {
    fn default() -> Self {
        Self {
            protocol: "coca.app-stream.v1",
            client_events: vec![
                "terminal.open",
                "terminal.attach",
                "terminal.input",
                "terminal.resize",
                "terminal.detach",
                "terminal.close",
            ],
            server_events: vec![
                "terminal.opened",
                "terminal.output",
                "terminal.exit",
                "terminal.error",
            ],
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TerminalCapability {
    pub enabled: bool,
    pub can_resume: bool,
    pub can_fork: bool,
    pub unavailable_code: Option<String>,
    pub unavailable_message: Option<String>,
}

impl TerminalCapability {
    fn for_session(session: &Session, settings: &Settings) -> Self {
        if !settings.terminal.enabled {
            return Self::unavailable(
                "terminal_disabled",
                "Terminal execution is disabled in settings.",
            );
        }
        if !settings.terminal.token_configured() {
            return Self::unavailable(
                "missing_terminal_token",
                "Terminal token is not configured.",
            );
        }
        match &session.origin {
            SessionOrigin::Local => Self::available(),
            SessionOrigin::Remote(name) => {
                let Some(remote) = settings.remotes.iter().find(|remote| &remote.name == name)
                else {
                    return Self::unavailable(
                        "remote_browse_only",
                        "Remote origin is not configured for terminal access.",
                    );
                };
                if !remote.enabled {
                    return Self::unavailable(
                        "remote_browse_only",
                        "Remote origin is disabled in settings.",
                    );
                }
                if remote
                    .terminal_token
                    .as_deref()
                    .map(|token| !token.trim().is_empty())
                    .unwrap_or(false)
                {
                    Self::available()
                } else {
                    Self::unavailable(
                        "remote_browse_only",
                        "Remote terminal token is not configured; this remote is browse-only.",
                    )
                }
            }
        }
    }

    fn available() -> Self {
        Self {
            enabled: true,
            can_resume: true,
            can_fork: true,
            unavailable_code: None,
            unavailable_message: None,
        }
    }

    fn unavailable(code: &str, message: &str) -> Self {
        Self {
            enabled: false,
            can_resume: false,
            can_fork: false,
            unavailable_code: Some(code.to_string()),
            unavailable_message: Some(message.to_string()),
        }
    }

    fn browse_only(message: &str) -> Self {
        Self::unavailable("browse_only", message)
    }
}

pub fn session_ref_from_session(session: &Session) -> SessionRef {
    SessionRef {
        origin: session.origin.to_string(),
        provider: session.provider.to_string(),
        id: session.id.clone(),
    }
}

fn parse_provider(provider: &str) -> Result<ProviderKind> {
    match provider {
        "codex" => Ok(ProviderKind::Codex),
        "claude" => Ok(ProviderKind::Claude),
        _ => anyhow::bail!("unknown provider: {provider}"),
    }
}

fn origin_matches(origin: &SessionOrigin, reference: &str) -> bool {
    match origin {
        SessionOrigin::Local => reference == "local",
        SessionOrigin::Remote(name) => reference == name,
    }
}

fn ensure_local(session: &Session) -> Result<()> {
    if session.is_local() {
        Ok(())
    } else {
        anyhow::bail!(
            "Remote sessions are browse-only in this version: {}",
            session.origin
        )
    }
}

fn ensure_terminal_enabled(settings: &Settings) -> Result<()> {
    if !settings.terminal.enabled {
        anyhow::bail!("terminal execution is disabled in settings");
    }
    if !settings.terminal.token_configured() {
        anyhow::bail!("terminal token is not configured");
    }
    Ok(())
}

fn remote_terminal_unavailable(
    enabled: bool,
    token_configured: bool,
    terminal_token_configured: bool,
) -> Option<(&'static str, &'static str)> {
    if !enabled {
        Some((
            "remote_browse_only",
            "Remote origin is disabled in settings.",
        ))
    } else if !token_configured {
        Some(("remote_auth_failed", "Remote read token is not configured."))
    } else if !terminal_token_configured {
        Some((
            "remote_browse_only",
            "Remote terminal token is not configured; this remote is browse-only.",
        ))
    } else {
        None
    }
}

fn display_role<'a>(role: &'a str, text: &str) -> &'a str {
    if text.contains("<environment_context>") || text.starts_with("# AGENTS.md instructions") {
        "context"
    } else if role == "system" || role == "tool" || role == "developer" {
        "event"
    } else {
        role
    }
}

fn format_time(timestamp_ms: Option<i64>) -> String {
    let Some(timestamp_ms) = timestamp_ms else {
        return "-".to_string();
    };
    let Some(dt) = DateTime::from_timestamp_millis(timestamp_ms) else {
        return "-".to_string();
    };
    dt.with_timezone(&Local)
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use coca_core::model::ChatMessage;

    #[test]
    fn session_detail_hides_duplicate_first_prompt() {
        let detail = SessionDetail::from_session(session());

        assert_eq!(detail.summary.message_count, 2);
        assert_eq!(detail.transcript.len(), 1);
        assert_eq!(detail.transcript[0].display_role, "assistant");
    }

    #[test]
    fn stream_protocol_reserves_terminal_events() {
        let info = StreamInfo::default();

        assert!(info.client_events.contains(&"terminal.open"));
        assert!(info.server_events.contains(&"terminal.output"));
    }

    #[test]
    fn config_summary_redacts_ai_api_key() {
        let mut settings = Settings::default();
        settings.ensure_defaults();
        settings.ai.api_key = "sk-secret".to_string();
        settings.terminal.token = "terminal-secret".to_string();
        let service = app_service(settings);

        let summary = service.config_summary("127.0.0.1:0").unwrap();
        let body = serde_json::to_string(&summary).unwrap();

        assert_eq!(summary.ai.base_url, "https://api.openai.com/v1");
        assert_eq!(summary.ai.model, "gpt-4o-mini");
        assert!(summary.ai.api_key_configured);
        assert!(!body.contains("sk-secret"));
        assert!(summary.terminal.token_configured);
        assert_eq!(
            summary.terminal.unavailable_code.as_deref(),
            Some("terminal_disabled")
        );
        assert!(!body.contains("terminal-secret"));
    }

    #[test]
    fn terminal_capability_reports_actionable_reasons() {
        let mut settings = Settings::default();
        settings.ensure_defaults();
        settings.terminal.enabled = true;

        let local = SessionSummary::from_session(session(), Some(&settings));
        assert!(local.terminal.can_resume);
        assert!(local.terminal.can_fork);

        let mut remote_session = session();
        remote_session.origin = SessionOrigin::Remote("work".to_string());
        let remote = SessionSummary::from_session(remote_session.clone(), Some(&settings));
        assert_eq!(
            remote.terminal.unavailable_code.as_deref(),
            Some("remote_browse_only")
        );

        settings
            .remotes
            .push(coca_core::settings::ConfiguredRemote {
                name: "work".to_string(),
                base_url: "http://127.0.0.1:8787".to_string(),
                token: "read-secret".to_string(),
                terminal_token: None,
                enabled: true,
            });
        let remote = SessionSummary::from_session(remote_session.clone(), Some(&settings));
        assert_eq!(
            remote.terminal.unavailable_code.as_deref(),
            Some("remote_browse_only")
        );

        settings.remotes[0].terminal_token = Some("terminal-secret".to_string());
        let remote = SessionSummary::from_session(remote_session, Some(&settings));
        assert!(remote.terminal.can_resume);
        assert!(remote.terminal.can_fork);
    }

    #[test]
    fn terminal_launch_requires_enabled_terminal_settings() {
        let mut settings = Settings::default();
        settings.ensure_defaults();
        let service = app_service(settings);

        let error = service
            .prepare_terminal_launch_with_cwd(&session(), LaunchMode::Resume, Path::new("/work"))
            .unwrap_err()
            .to_string();

        assert!(error.contains("terminal execution is disabled"));
    }

    #[test]
    fn terminal_launch_uses_saved_launch_defaults() {
        let mut settings = Settings::default();
        settings.ensure_defaults();
        settings.terminal.enabled = true;
        settings.launch_defaults.fork.use_current_dir = true;
        settings.launch_defaults.fork.yolo = true;
        let service = app_service(settings);

        let target = service
            .prepare_terminal_launch_with_cwd(&session(), LaunchMode::Fork, Path::new("/work"))
            .unwrap();

        assert_eq!(target.program, "codex");
        assert_eq!(
            target.args,
            vec![
                "fork".to_string(),
                "-C".to_string(),
                "/work".to_string(),
                "--dangerously-bypass-approvals-and-sandbox".to_string(),
                "sid".to_string(),
            ]
        );
        assert_eq!(target.cwd.as_deref(), Some(Path::new("/work")));
    }

    #[test]
    fn update_ai_settings_keeps_blank_key_and_honors_clear_flag() {
        let mut settings = Settings::default();
        settings.ensure_defaults();
        settings.ai.api_key = "sk-existing".to_string();
        let mut service = app_service(settings);

        let summary = service
            .update_ai_settings(AiSettingsUpdate {
                base_url: Some(" https://example.test/v1 ".to_string()),
                model: Some(" custom-model ".to_string()),
                enabled: Some(true),
                provider: Some(" openai_compatible ".to_string()),
                api_key_env: Some(" OPENAI_API_KEY ".to_string()),
                api_key: Some("   ".to_string()),
                clear_api_key: false,
            })
            .unwrap();

        assert_eq!(summary.base_url, "https://example.test/v1");
        assert_eq!(summary.model, "custom-model");
        assert!(summary.api_key_configured);
        assert_eq!(service.settings().ai.api_key, "sk-existing");

        let summary = service
            .update_ai_settings(AiSettingsUpdate {
                enabled: Some(false),
                clear_api_key: true,
                ..AiSettingsUpdate::default()
            })
            .unwrap();

        assert!(!summary.api_key_configured);
        assert!(service.settings().ai.api_key.is_empty());
    }

    fn app_service(settings: Settings) -> AppService {
        AppService::new(AppOptions {
            settings,
            settings_path: None,
            codex_home: None,
            claude_home: None,
            provider_filter: ProviderFilter::All,
            database_path: None,
        })
    }

    fn session() -> Session {
        Session {
            origin: SessionOrigin::Local,
            provider: ProviderKind::Codex,
            id: "sid".to_string(),
            title: "title".to_string(),
            cwd: "/tmp".to_string(),
            created_at_ms: None,
            updated_at_ms: Some(1),
            model: Some("model".to_string()),
            source_path: "/tmp/session".into(),
            first_user_message: Some("hello".to_string()),
            transcript: vec![
                ChatMessage {
                    role: "user".to_string(),
                    text: "hello".to_string(),
                    timestamp_ms: Some(1),
                },
                ChatMessage {
                    role: "assistant".to_string(),
                    text: "world".to_string(),
                    timestamp_ms: Some(2),
                },
            ],
            resume_program: "codex".to_string(),
            resume_args: vec!["resume".to_string(), "sid".to_string()],
        }
    }
}
