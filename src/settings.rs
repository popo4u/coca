use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::launch::{LaunchMode, LaunchOptionKind};
use crate::model::SessionOrigin;
use crate::remote::{load_remote_config, RemoteConfig, RemoteEndpoint};

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Settings {
    #[serde(default)]
    pub remotes: Vec<ConfiguredRemote>,
    #[serde(default)]
    pub origin_visibility: OriginVisibility,
    #[serde(default)]
    pub launch_defaults: LaunchDefaults,
    #[serde(default)]
    pub share: ShareSettings,
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
            if remote.addr.trim().is_empty() {
                anyhow::bail!("remote {name} addr must not be empty");
            }
            if remote.token.trim().is_empty() {
                anyhow::bail!("remote {name} token must not be empty");
            }
        }
        Ok(())
    }

    pub fn remote_config(&self) -> RemoteConfig {
        RemoteConfig {
            remotes: self
                .remotes
                .iter()
                .filter(|remote| remote.enabled)
                .map(|remote| RemoteEndpoint {
                    name: remote.name.clone(),
                    addr: remote.addr.clone(),
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

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ShareSettings {
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub token: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConfiguredRemote {
    pub name: String,
    pub addr: String,
    pub token: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl From<RemoteEndpoint> for ConfiguredRemote {
    fn from(remote: RemoteEndpoint) -> Self {
        Self {
            name: remote.name,
            addr: remote.addr,
            token: remote.token,
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
    let mut settings = if path.exists() {
        load_settings(&path)?
    } else {
        load_legacy_settings().unwrap_or_default()
    };

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

pub fn load_settings(path: &Path) -> Result<Settings> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read settings {}", path.display()))?;
    let settings: Settings = serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse settings {}", path.display()))?;
    settings.validate()?;
    Ok(settings)
}

pub fn save_settings(path: &Path, settings: &Settings) -> Result<()> {
    settings.validate()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create settings directory {}", parent.display()))?;
    }
    let contents =
        serde_json::to_string_pretty(settings).context("failed to serialize settings")?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_settings_with_defaults() {
        let settings: Settings = serde_json::from_str(
            r#"{
                "remotes": [
                    { "name": "work", "addr": "127.0.0.1:8765", "token": "secret" }
                ],
                "launch_defaults": {
                    "resume": { "yolo": true },
                    "fork": { "use_current_dir": true }
                },
                "share": {
                    "base_url": "http://192.168.1.20:8787",
                    "token": "secret"
                }
            }"#,
        )
        .unwrap();

        assert!(settings.remotes[0].enabled);
        assert!(settings.origin_visible(&SessionOrigin::Local));
        assert!(settings.launch_default(LaunchMode::Resume, LaunchOptionKind::Yolo));
        assert!(settings.launch_default(LaunchMode::Fork, LaunchOptionKind::UseCurrentDir));
        assert_eq!(settings.share.base_url, "http://192.168.1.20:8787");
        assert_eq!(settings.share.token, "secret");
    }

    #[test]
    fn filters_disabled_remotes_from_remote_config() {
        let settings: Settings = serde_json::from_str(
            r#"{
                "remotes": [
                    { "name": "enabled", "addr": "127.0.0.1:1", "token": "secret" },
                    { "name": "disabled", "addr": "127.0.0.1:2", "token": "secret", "enabled": false }
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
    fn saves_and_loads_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut settings = Settings::default();
        settings.remotes.push(ConfiguredRemote {
            name: "work".to_string(),
            addr: "127.0.0.1:8765".to_string(),
            token: "secret".to_string(),
            enabled: false,
        });
        settings.set_launch_default(LaunchMode::Fork, LaunchOptionKind::Yolo, true);

        save_settings(&path, &settings).unwrap();
        let loaded = load_settings(&path).unwrap();

        assert_eq!(loaded, settings);
    }
}
