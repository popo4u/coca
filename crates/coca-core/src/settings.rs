use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};

use crate::launch::{LaunchMode, LaunchOptionKind};
use crate::model::SessionOrigin;
use crate::remote::{load_remote_config, RemoteConfig, RemoteEndpoint};

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Settings {
    #[serde(default)]
    pub daemon: DaemonSettings,
    #[serde(default)]
    pub gateway: GatewaySettings,
    #[serde(default)]
    pub ai: AiSettings,
    #[serde(default)]
    pub remotes: Vec<ConfiguredRemote>,
    #[serde(default)]
    pub origin_visibility: OriginVisibility,
    #[serde(default)]
    pub launch_defaults: LaunchDefaults,
    #[serde(default)]
    pub share: ShareSettings,
    #[serde(default)]
    pub terminal: TerminalSettings,
}

impl Settings {
    pub fn validate(&self) -> Result<()> {
        let mut names = HashSet::new();
        for remote in &self.remotes {
            let name = remote.name.trim();
            if name.is_empty() {
                anyhow::bail!("remote name must not be empty");
            }
            if !names.insert(name.to_string()) {
                anyhow::bail!("duplicate remote name: {name}");
            }
            if remote.base_url.trim().is_empty() {
                anyhow::bail!("remote {name} base_url must not be empty");
            }
            if remote.token.trim().is_empty() {
                anyhow::bail!("remote {name} token must not be empty");
            }
            if let Some(token) = &remote.terminal_token {
                if token.trim().is_empty() {
                    anyhow::bail!("remote {name} terminal_token must not be empty when configured");
                }
                if token.chars().any(|ch| matches!(ch, '\r' | '\n')) {
                    anyhow::bail!("remote {name} terminal_token must not contain newlines");
                }
            }
        }
        self.daemon.validate()?;
        self.gateway.validate()?;
        self.ai.validate()?;
        if self.share.base_url.trim().is_empty() {
            anyhow::bail!("share base_url must not be empty");
        }
        if self.share.token.trim().is_empty() {
            anyhow::bail!("share token must not be empty");
        }
        self.terminal.validate()?;
        Ok(())
    }

    pub fn ensure_defaults(&mut self) -> bool {
        let mut changed = false;
        if self.daemon.ensure_defaults() {
            changed = true;
        }
        if self.gateway.bind.trim().is_empty() {
            self.gateway.bind = default_gateway_bind();
            changed = true;
        }
        if self.ai.ensure_defaults() {
            changed = true;
        }
        if self.share.base_url.trim().is_empty() {
            self.share.base_url = default_share_base_url();
            changed = true;
        }
        if self.share.token.trim().is_empty() {
            self.share.token = generate_token();
            changed = true;
        }
        if self.terminal.token.trim().is_empty() {
            self.terminal.token = generate_token();
            changed = true;
        }
        changed
    }

    pub fn remote_config(&self) -> RemoteConfig {
        RemoteConfig {
            remotes: self
                .remotes
                .iter()
                .filter(|remote| remote.enabled)
                .map(|remote| RemoteEndpoint {
                    name: remote.name.clone(),
                    base_url: remote.base_url.clone(),
                    token: remote.token.clone(),
                })
                .collect(),
        }
    }

    pub fn origin_visible(&self, origin: &SessionOrigin) -> bool {
        match origin {
            SessionOrigin::Local => self.origin_visibility.local,
            SessionOrigin::Remote(name) => {
                self.remote_enabled(name)
                    && self
                        .origin_visibility
                        .remotes
                        .get(name)
                        .copied()
                        .unwrap_or(true)
            }
        }
    }

    pub fn remote_enabled(&self, name: &str) -> bool {
        self.remotes
            .iter()
            .find(|remote| remote.name == name)
            .map(|remote| remote.enabled)
            .unwrap_or_else(|| {
                self.origin_visibility
                    .remotes
                    .get(name)
                    .copied()
                    .unwrap_or(true)
            })
    }

    pub fn set_remote_enabled(&mut self, name: &str, enabled: bool) {
        if let Some(remote) = self.remotes.iter_mut().find(|remote| remote.name == name) {
            remote.enabled = enabled;
        }
        self.origin_visibility
            .remotes
            .insert(name.to_string(), enabled);
    }

    pub fn launch_default(&self, mode: LaunchMode, kind: LaunchOptionKind) -> bool {
        self.launch_defaults.for_mode(mode).enabled(kind)
    }

    pub fn set_launch_default(&mut self, mode: LaunchMode, kind: LaunchOptionKind, enabled: bool) {
        self.launch_defaults.for_mode_mut(mode).set(kind, enabled);
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonSettings {
    #[serde(default = "default_daemon_socket")]
    pub socket: String,
    #[serde(default = "default_daemon_terminal_socket")]
    pub terminal_socket: String,
}

impl DaemonSettings {
    fn validate(&self) -> Result<()> {
        if self.socket.trim().is_empty() {
            anyhow::bail!("daemon socket must not be empty");
        }
        if self.terminal_socket.trim().is_empty() {
            anyhow::bail!("daemon terminal_socket must not be empty");
        }
        Ok(())
    }

    fn ensure_defaults(&mut self) -> bool {
        let mut changed = false;
        if self.socket.trim().is_empty() {
            self.socket = default_daemon_socket();
            changed = true;
        }
        if self.terminal_socket.trim().is_empty() {
            self.terminal_socket = default_daemon_terminal_socket();
            changed = true;
        }
        changed
    }
}

impl Default for DaemonSettings {
    fn default() -> Self {
        Self {
            socket: default_daemon_socket(),
            terminal_socket: default_daemon_terminal_socket(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GatewaySettings {
    #[serde(default = "default_gateway_bind")]
    pub bind: String,
}

impl GatewaySettings {
    fn validate(&self) -> Result<()> {
        if self.bind.trim().is_empty() {
            anyhow::bail!("gateway bind must not be empty");
        }
        Ok(())
    }
}

impl Default for GatewaySettings {
    fn default() -> Self {
        Self {
            bind: default_gateway_bind(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AiSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_ai_provider")]
    pub provider: String,
    #[serde(default = "default_ai_base_url")]
    pub base_url: String,
    #[serde(default = "default_ai_model")]
    pub model: String,
    #[serde(default = "default_ai_api_key_env")]
    pub api_key_env: String,
    #[serde(default)]
    pub api_key: String,
}

impl AiSettings {
    fn validate(&self) -> Result<()> {
        if self.provider.trim() != "openai_compatible" {
            anyhow::bail!("ai provider must be openai_compatible");
        }
        let base_url = self.base_url.trim();
        if self.enabled && base_url.is_empty() {
            anyhow::bail!("ai base_url must not be empty");
        }
        if !(base_url.is_empty()
            || base_url.starts_with("http://")
            || base_url.starts_with("https://"))
        {
            anyhow::bail!("ai base_url must start with http:// or https://");
        }
        if base_url.chars().any(char::is_whitespace) {
            anyhow::bail!("ai base_url must not contain whitespace");
        }

        let model = self.model.trim();
        if self.enabled && model.is_empty() {
            anyhow::bail!("ai model must not be empty");
        }
        if model.chars().any(char::is_whitespace) {
            anyhow::bail!("ai model must not contain whitespace");
        }
        if self.api_key.chars().any(|ch| matches!(ch, '\r' | '\n')) {
            anyhow::bail!("ai api_key must not contain newlines");
        }
        if self.api_key_env.chars().any(char::is_whitespace) {
            anyhow::bail!("ai api_key_env must not contain whitespace");
        }
        if self.enabled && !self.key_configured() {
            anyhow::bail!("ai api_key or configured ai api_key_env is required when ai is enabled");
        }
        Ok(())
    }

    fn ensure_defaults(&mut self) -> bool {
        let mut changed = false;
        if self.provider.trim().is_empty() {
            self.provider = default_ai_provider();
            changed = true;
        }
        if self.base_url.trim().is_empty() {
            self.base_url = default_ai_base_url();
            changed = true;
        }
        if self.model.trim().is_empty() {
            self.model = default_ai_model();
            changed = true;
        }
        if self.api_key_env.trim().is_empty() {
            self.api_key_env = default_ai_api_key_env();
            changed = true;
        }
        changed
    }

    pub fn key_configured(&self) -> bool {
        !self.api_key.trim().is_empty()
            || (!self.api_key_env.trim().is_empty()
                && std::env::var(self.api_key_env.trim())
                    .map(|value| !value.trim().is_empty())
                    .unwrap_or(false))
    }

    pub fn key_source(&self) -> &'static str {
        if !self.api_key.trim().is_empty() {
            "stored"
        } else if !self.api_key_env.trim().is_empty()
            && std::env::var(self.api_key_env.trim())
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false)
        {
            "env"
        } else {
            "missing"
        }
    }
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: default_ai_provider(),
            base_url: default_ai_base_url(),
            model: default_ai_model(),
            api_key_env: default_ai_api_key_env(),
            api_key: String::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ShareSettings {
    #[serde(default = "default_share_base_url")]
    pub base_url: String,
    #[serde(default)]
    pub token: String,
}

impl Default for ShareSettings {
    fn default() -> Self {
        Self {
            base_url: default_share_base_url(),
            token: String::new(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct TerminalSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub token: String,
}

impl TerminalSettings {
    fn validate(&self) -> Result<()> {
        if self.enabled && self.token.trim().is_empty() {
            anyhow::bail!("terminal token must not be empty when terminal is enabled");
        }
        if self.token.chars().any(|ch| matches!(ch, '\r' | '\n')) {
            anyhow::bail!("terminal token must not contain newlines");
        }
        Ok(())
    }

    pub fn token_configured(&self) -> bool {
        !self.token.trim().is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ConfiguredRemote {
    pub name: String,
    pub base_url: String,
    pub token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_token: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
struct ConfiguredRemoteWire {
    name: String,
    base_url: Option<String>,
    addr: Option<String>,
    token: String,
    terminal_token: Option<String>,
    #[serde(default = "default_true")]
    enabled: bool,
}

impl<'de> Deserialize<'de> for ConfiguredRemote {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = ConfiguredRemoteWire::deserialize(deserializer)?;
        let base_url = wire
            .base_url
            .or_else(|| wire.addr.map(normalize_legacy_addr))
            .unwrap_or_default();
        Ok(Self {
            name: wire.name,
            base_url,
            token: wire.token,
            terminal_token: wire.terminal_token,
            enabled: wire.enabled,
        })
    }
}

impl From<RemoteEndpoint> for ConfiguredRemote {
    fn from(remote: RemoteEndpoint) -> Self {
        Self {
            name: remote.name,
            base_url: remote.base_url,
            token: remote.token,
            terminal_token: None,
            enabled: true,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OriginVisibility {
    #[serde(default = "default_true")]
    pub local: bool,
    #[serde(default)]
    pub remotes: BTreeMap<String, bool>,
}

impl Default for OriginVisibility {
    fn default() -> Self {
        Self {
            local: true,
            remotes: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct LaunchDefaults {
    #[serde(default)]
    pub resume: LaunchOptionDefaults,
    #[serde(default)]
    pub fork: LaunchOptionDefaults,
}

impl LaunchDefaults {
    fn for_mode(&self, mode: LaunchMode) -> &LaunchOptionDefaults {
        match mode {
            LaunchMode::Resume => &self.resume,
            LaunchMode::Fork => &self.fork,
        }
    }

    fn for_mode_mut(&mut self, mode: LaunchMode) -> &mut LaunchOptionDefaults {
        match mode {
            LaunchMode::Resume => &mut self.resume,
            LaunchMode::Fork => &mut self.fork,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct LaunchOptionDefaults {
    #[serde(default)]
    pub use_current_dir: bool,
    #[serde(default)]
    pub yolo: bool,
}

impl LaunchOptionDefaults {
    fn enabled(&self, kind: LaunchOptionKind) -> bool {
        match kind {
            LaunchOptionKind::UseCurrentDir => self.use_current_dir,
            LaunchOptionKind::Yolo => self.yolo,
        }
    }

    fn set(&mut self, kind: LaunchOptionKind, enabled: bool) {
        match kind {
            LaunchOptionKind::UseCurrentDir => self.use_current_dir = enabled,
            LaunchOptionKind::Yolo => self.yolo = enabled,
        }
    }
}

pub fn default_settings_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".config").join("coca").join("settings.json"))
}

fn legacy_remote_config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".config").join("coca").join("remotes.json"))
}

pub fn load_settings_for_cli(remote_config_path: Option<&Path>) -> Result<(Settings, PathBuf)> {
    let path = default_settings_path()
        .context("failed to resolve default settings path: home directory was not found")?;
    let path_exists = path.exists();
    let mut settings = if path_exists {
        load_settings_raw(&path)?
    } else {
        load_legacy_settings().unwrap_or_default()
    };

    let changed = settings.ensure_defaults() || !path_exists;
    settings.validate()?;
    if changed {
        save_settings(&path, &settings)?;
    }

    if let Some(remote_config_path) = remote_config_path {
        settings.remotes = load_remote_config(remote_config_path)?
            .remotes
            .into_iter()
            .map(ConfiguredRemote::from)
            .collect();
    }

    settings.validate()?;
    Ok((settings, path))
}

#[cfg(test)]
fn load_settings(path: &Path) -> Result<Settings> {
    let mut settings = load_settings_raw(path)?;
    settings.ensure_defaults();
    settings.validate()?;
    Ok(settings)
}

fn load_settings_raw(path: &Path) -> Result<Settings> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read settings {}", path.display()))?;
    let settings: Settings = serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse settings {}", path.display()))?;
    Ok(settings)
}

pub fn save_settings(path: &Path, settings: &Settings) -> Result<()> {
    let mut settings = settings.clone();
    settings.ensure_defaults();
    settings.validate()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create settings directory {}", parent.display()))?;
    }
    let contents =
        serde_json::to_string_pretty(&settings).context("failed to serialize settings")?;
    fs::write(path, format!("{contents}\n"))
        .with_context(|| format!("failed to write settings {}", path.display()))
}

fn load_legacy_settings() -> Option<Settings> {
    let path = legacy_remote_config_path()?;
    if !path.exists() {
        return None;
    }
    let remote_config = load_remote_config(&path).ok()?;
    Some(Settings {
        remotes: remote_config
            .remotes
            .into_iter()
            .map(ConfiguredRemote::from)
            .collect(),
        ..Settings::default()
    })
}

fn default_true() -> bool {
    true
}

fn default_gateway_bind() -> String {
    "0.0.0.0:8787".to_string()
}

fn default_daemon_socket() -> String {
    dirs::home_dir()
        .map(|home| {
            home.join(".config")
                .join("coca")
                .join("daemon.sock")
                .to_string_lossy()
                .to_string()
        })
        .unwrap_or_else(|| "daemon.sock".to_string())
}

fn default_daemon_terminal_socket() -> String {
    dirs::home_dir()
        .map(|home| {
            home.join(".config")
                .join("coca")
                .join("daemon.terminal.sock")
                .to_string_lossy()
                .to_string()
        })
        .unwrap_or_else(|| "daemon.terminal.sock".to_string())
}

fn default_ai_provider() -> String {
    "openai_compatible".to_string()
}

fn default_ai_base_url() -> String {
    "https://api.openai.com/v1".to_string()
}

fn default_ai_model() -> String {
    "gpt-4o-mini".to_string()
}

fn default_ai_api_key_env() -> String {
    "OPENAI_API_KEY".to_string()
}

fn default_share_base_url() -> String {
    "http://127.0.0.1:8787".to_string()
}

fn normalize_legacy_addr(addr: String) -> String {
    if addr.starts_with("http://") || addr.starts_with("https://") {
        addr
    } else {
        format!("http://{addr}")
    }
}

fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let mut token = String::with_capacity(bytes.len() * 2);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        token.push(HEX[(byte >> 4) as usize] as char);
        token.push(HEX[(byte & 0x0f) as usize] as char);
    }
    token
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_settings_with_defaults() {
        let settings: Settings = serde_json::from_str(
            r#"{
                "remotes": [
                    { "name": "work", "base_url": "http://127.0.0.1:8787", "token": "secret" }
                ],
                "launch_defaults": {
                    "resume": { "yolo": true },
                    "fork": { "use_current_dir": true }
                },
                "share": {
                    "base_url": "http://192.168.1.20:8787",
                    "token": "secret"
                },
                "terminal": {
                    "enabled": true,
                    "token": "terminal-secret"
                }
            }"#,
        )
        .unwrap();

        assert!(settings.remotes[0].enabled);
        assert_eq!(settings.gateway.bind, "0.0.0.0:8787");
        assert!(settings.daemon.socket.ends_with("daemon.sock"));
        assert!(settings
            .daemon
            .terminal_socket
            .ends_with("daemon.terminal.sock"));
        assert_eq!(settings.ai.base_url, "https://api.openai.com/v1");
        assert_eq!(settings.ai.model, "gpt-4o-mini");
        assert!(settings.ai.api_key.is_empty());
        assert!(settings.origin_visible(&SessionOrigin::Local));
        assert!(settings.launch_default(LaunchMode::Resume, LaunchOptionKind::Yolo));
        assert!(settings.launch_default(LaunchMode::Fork, LaunchOptionKind::UseCurrentDir));
        assert_eq!(settings.share.base_url, "http://192.168.1.20:8787");
        assert_eq!(settings.share.token, "secret");
        assert!(settings.terminal.enabled);
        assert_eq!(settings.terminal.token, "terminal-secret");
    }

    #[test]
    fn ensure_defaults_generates_share_and_terminal_tokens() {
        let mut settings = Settings::default();

        assert!(settings.share.token.is_empty());
        assert!(settings.terminal.token.is_empty());
        assert!(settings.ensure_defaults());

        assert_eq!(settings.gateway.bind, "0.0.0.0:8787");
        assert!(settings.daemon.socket.ends_with("daemon.sock"));
        assert!(settings
            .daemon
            .terminal_socket
            .ends_with("daemon.terminal.sock"));
        assert_eq!(settings.ai.base_url, "https://api.openai.com/v1");
        assert_eq!(settings.ai.model, "gpt-4o-mini");
        assert!(settings.ai.api_key.is_empty());
        assert_eq!(settings.share.base_url, "http://127.0.0.1:8787");
        assert_eq!(settings.share.token.len(), 64);
        assert!(settings
            .share
            .token
            .chars()
            .all(|ch| ch.is_ascii_hexdigit()));
        assert_eq!(settings.terminal.token.len(), 64);
        assert!(!settings.terminal.enabled);
        assert!(settings
            .terminal
            .token
            .chars()
            .all(|ch| ch.is_ascii_hexdigit()));
    }

    #[test]
    fn validates_ai_settings_and_only_requires_key_when_enabled() {
        let mut settings = Settings::default();
        settings.ensure_defaults();

        assert!(settings.validate().is_ok());

        settings.ai.base_url = "ftp://example.test/v1".to_string();
        assert!(settings.validate().is_err());

        settings.ai.base_url = "https://api.openai.com/v1".to_string();
        settings.ai.model = " ".to_string();
        settings.ai.enabled = true;
        assert!(settings.validate().is_err());

        settings.ai.model = "gpt-4o-mini".to_string();
        settings.ai.api_key = "line\nbreak".to_string();
        assert!(settings.validate().is_err());

        settings.ai.api_key.clear();
        assert!(settings.validate().is_err());

        settings.ai.api_key = "sk-test".to_string();
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn filters_disabled_remotes_from_remote_config() {
        let settings: Settings = serde_json::from_str(
            r#"{
                "remotes": [
                    { "name": "enabled", "base_url": "http://127.0.0.1:1", "token": "secret" },
                    { "name": "disabled", "base_url": "http://127.0.0.1:2", "token": "secret", "enabled": false }
                ]
            }"#,
        )
        .unwrap();

        let remote_config = settings.remote_config();
        assert_eq!(remote_config.remotes.len(), 1);
        assert_eq!(remote_config.remotes[0].name, "enabled");
        assert!(!settings.origin_visible(&SessionOrigin::Remote("disabled".to_string())));
    }

    #[test]
    fn validates_terminal_settings_and_remote_terminal_tokens() {
        let mut settings = Settings::default();
        settings.ensure_defaults();

        assert!(settings.validate().is_ok());

        settings.terminal.token = "line\nbreak".to_string();
        assert!(settings.validate().is_err());

        settings.terminal.token.clear();
        settings.terminal.enabled = true;
        assert!(settings.validate().is_err());

        settings.terminal.token = "terminal-secret".to_string();
        settings.remotes.push(ConfiguredRemote {
            name: "work".to_string(),
            base_url: "http://127.0.0.1:8787".to_string(),
            token: "secret".to_string(),
            terminal_token: Some(" ".to_string()),
            enabled: true,
        });
        assert!(settings.validate().is_err());
    }

    #[test]
    fn saves_and_loads_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut settings = Settings::default();
        settings.remotes.push(ConfiguredRemote {
            name: "work".to_string(),
            base_url: "http://127.0.0.1:8787".to_string(),
            token: "secret".to_string(),
            terminal_token: Some("terminal-secret".to_string()),
            enabled: false,
        });
        settings.set_launch_default(LaunchMode::Fork, LaunchOptionKind::Yolo, true);
        settings.ensure_defaults();

        save_settings(&path, &settings).unwrap();
        let loaded = load_settings(&path).unwrap();

        assert_eq!(loaded, settings);
    }
}
