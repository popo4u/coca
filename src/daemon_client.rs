use std::path::{Path, PathBuf};

use anyhow::Result;
use coca_core::catalog::SessionCatalog;
use coca_core::launch::{LaunchMode, LaunchOption, LaunchOptionKind, ResumeTarget};
use coca_core::model::{ProviderKind, Session, SessionOrigin};
use coca_core::settings::Settings;
use coca_daemon::{LocalRpcClient, RpcDaemonOptions};
use coca_protocol::{
    LaunchModeWire, LaunchOptionKindWire, LaunchOptionWire, LaunchOptionsParams,
    LaunchPrepareParams, PreparedLaunch, SessionRef,
};
use coca_tui::{DaemonClient, SettingsUpdate};

pub struct RpcDaemonClient {
    client: LocalRpcClient,
    settings_path: Option<PathBuf>,
}

impl RpcDaemonClient {
    pub fn new(options: RpcDaemonOptions) -> Self {
        let settings_path = options.settings_path.clone();
        Self {
            client: LocalRpcClient::new(options),
            settings_path,
        }
    }
}

impl DaemonClient for RpcDaemonClient {
    fn session_catalog(&mut self) -> Result<SessionCatalog> {
        self.client.session_catalog()
    }

    fn settings(&mut self) -> Result<Settings> {
        self.client.settings()
    }

    fn update_settings(&mut self, settings: &Settings) -> Result<SettingsUpdate> {
        let settings = self.client.settings_update(settings.clone())?;
        Ok(SettingsUpdate {
            settings,
            status_message: self
                .settings_path
                .as_ref()
                .map(|path| format!("Settings saved to {}", path.to_string_lossy()))
                .unwrap_or_else(|| "Settings updated for this run only.".to_string()),
        })
    }

    fn share_url(&mut self, session: &Session) -> Result<String> {
        self.client.share_url(session_ref(session))
    }

    fn launch_options(
        &mut self,
        session: &Session,
        mode: LaunchMode,
        current_cwd: &Path,
    ) -> Result<Vec<LaunchOption>> {
        let options = self.client.launch_options(LaunchOptionsParams {
            session: session_ref(session),
            mode: launch_mode_wire(mode),
            current_cwd: current_cwd.to_string_lossy().to_string(),
        })?;
        Ok(options.into_iter().map(launch_option).collect())
    }

    fn prepare_launch(
        &mut self,
        session: &Session,
        mode: LaunchMode,
        current_cwd: &Path,
        options: &[LaunchOption],
    ) -> Result<ResumeTarget> {
        let target = self.client.launch_prepare(LaunchPrepareParams {
            session: session_ref(session),
            mode: launch_mode_wire(mode),
            current_cwd: current_cwd.to_string_lossy().to_string(),
            options: options.iter().cloned().map(launch_option_wire).collect(),
        })?;
        Ok(resume_target(target))
    }
}

fn session_ref(session: &Session) -> SessionRef {
    SessionRef {
        origin: match &session.origin {
            SessionOrigin::Local => "local".to_string(),
            SessionOrigin::Remote(name) => name.clone(),
        },
        provider: match session.provider {
            ProviderKind::Codex => "codex".to_string(),
            ProviderKind::Claude => "claude".to_string(),
        },
        id: session.id.clone(),
    }
}

fn launch_mode_wire(mode: LaunchMode) -> LaunchModeWire {
    match mode {
        LaunchMode::Resume => LaunchModeWire::Resume,
        LaunchMode::Fork => LaunchModeWire::Fork,
    }
}

fn launch_option_kind_wire(kind: LaunchOptionKind) -> LaunchOptionKindWire {
    match kind {
        LaunchOptionKind::UseCurrentDir => LaunchOptionKindWire::UseCurrentDir,
        LaunchOptionKind::Yolo => LaunchOptionKindWire::Yolo,
    }
}

fn launch_option_kind(kind: LaunchOptionKindWire) -> LaunchOptionKind {
    match kind {
        LaunchOptionKindWire::UseCurrentDir => LaunchOptionKind::UseCurrentDir,
        LaunchOptionKindWire::Yolo => LaunchOptionKind::Yolo,
    }
}

fn launch_option_wire(option: LaunchOption) -> LaunchOptionWire {
    LaunchOptionWire {
        kind: launch_option_kind_wire(option.kind),
        label: option.label,
        enabled: option.enabled,
    }
}

fn launch_option(option: LaunchOptionWire) -> LaunchOption {
    LaunchOption {
        kind: launch_option_kind(option.kind),
        label: option.label,
        enabled: option.enabled,
    }
}

fn resume_target(target: PreparedLaunch) -> ResumeTarget {
    ResumeTarget {
        program: target.program,
        args: target.args,
        cwd: target.cwd.map(PathBuf::from),
    }
}
