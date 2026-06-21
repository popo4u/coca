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
        Ok(SessionsResponse::from_catalog(catalog))
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
        Ok(Some(SessionsResponse::from_catalog(SessionCatalog {
            sessions,
            warnings: vec![
                "serving sessions from coca storage while refreshing in background".to_string(),
            ],
        })))
    }

    pub fn web_session_detail(&self, reference: &SessionRef) -> Result<Option<SessionDetail>> {
        Ok(self.session(reference)?.map(SessionDetail::from_session))
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
    fn from_catalog(catalog: SessionCatalog) -> Self {
        let counts = CatalogCounts::from_sessions(&catalog.sessions);
        Self {
            sessions: catalog
                .sessions
                .into_iter()
                .map(SessionSummary::from_session)
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
}

impl SessionSummary {
    fn from_session(session: Session) -> Self {
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
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionDetail {
    pub summary: SessionSummary,
    pub transcript: Vec<ChatMessageDto>,
}

impl SessionDetail {
    fn from_session(session: Session) -> Self {
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
            summary: SessionSummary::from_session(session),
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
    pub core_bind: String,
    pub ai: AiSummary,
    pub share: ShareSummary,
    pub remotes: Vec<RemoteSummary>,
    pub launch_defaults: LaunchDefaultsSummary,
    pub counts: CatalogCounts,
    pub warnings: Vec<String>,
}

impl ConfigSummary {
    fn from_parts(settings: &Settings, bind: &str, catalog: &SessionCatalog) -> Self {
        let counts = CatalogCounts::from_sessions(&catalog.sessions);
        Self {
            service: "coca-web".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            bind: bind.to_string(),
            core_bind: settings.core.bind.clone(),
            ai: AiSummary::from_settings(&settings.ai),
            share: ShareSummary {
                base_url: settings.share.base_url.clone(),
                token_configured: !settings.share.token.trim().is_empty(),
            },
            remotes: settings
                .remotes
                .iter()
                .map(|remote| RemoteSummary {
                    name: remote.name.clone(),
                    base_url: remote.base_url.clone(),
                    enabled: remote.enabled,
                    visible: settings.origin_visible(&SessionOrigin::Remote(remote.name.clone())),
                    token_configured: !remote.token.trim().is_empty(),
                    session_count: counts
                        .by_origin
                        .get(&remote.name)
                        .copied()
                        .unwrap_or_default(),
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
pub struct RemoteSummary {
    pub name: String,
    pub base_url: String,
    pub enabled: bool,
    pub visible: bool,
    pub token_configured: bool,
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
                "terminal.input",
                "terminal.resize",
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
        let service = app_service(settings);

        let summary = service.config_summary("127.0.0.1:0").unwrap();
        let body = serde_json::to_string(&summary).unwrap();

        assert_eq!(summary.ai.base_url, "https://api.openai.com/v1");
        assert_eq!(summary.ai.model, "gpt-4o-mini");
        assert!(summary.ai.api_key_configured);
        assert!(!body.contains("sk-secret"));
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
