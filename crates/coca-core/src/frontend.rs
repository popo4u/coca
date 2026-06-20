use std::path::{Path, PathBuf};

use crate::launch::{
    build_launch_target, default_resume_target, launch_options, LaunchMode, LaunchOption,
    ResumeTarget,
};
use crate::model::Session;
use crate::settings::{save_settings, Settings};
use crate::share::build_share_url;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrontendError {
    RemoteBrowseOnly { origin: String },
    RemoteShareUnsupported { origin: String },
    ShareSettingsMissing,
    SettingsSaveFailed { message: String },
}

impl FrontendError {
    pub fn message(&self) -> String {
        match self {
            Self::RemoteBrowseOnly { origin } => {
                format!("Remote sessions are browse-only in this version: {origin}")
            }
            Self::RemoteShareUnsupported { origin } => {
                format!("Remote sessions cannot be shared from this machine in v0: {origin}")
            }
            Self::ShareSettingsMissing => {
                "Configure share.base_url and share.token in settings.json to generate share URLs."
                    .to_string()
            }
            Self::SettingsSaveFailed { message } => format!("Failed to save settings: {message}"),
        }
    }
}

pub fn default_resume_for_session(session: &Session) -> Result<ResumeTarget, FrontendError> {
    ensure_local_launch(session)?;
    Ok(default_resume_target(session))
}

pub fn launch_options_with_defaults(
    settings: &Settings,
    session: &Session,
    mode: LaunchMode,
    current_cwd: &Path,
) -> Result<Vec<LaunchOption>, FrontendError> {
    ensure_local_launch(session)?;
    let mut options = launch_options(session, current_cwd);
    for option in &mut options {
        option.enabled = settings.launch_default(mode, option.kind);
    }
    Ok(options)
}

pub fn prepare_launch(
    session: &Session,
    mode: LaunchMode,
    current_cwd: &Path,
    options: &[LaunchOption],
) -> Result<ResumeTarget, FrontendError> {
    ensure_local_launch(session)?;
    Ok(build_launch_target(session, mode, current_cwd, options))
}

pub fn share_url_for_session(
    settings: &Settings,
    session: &Session,
) -> Result<String, FrontendError> {
    if !session.is_local() {
        return Err(FrontendError::RemoteShareUnsupported {
            origin: session.origin.to_string(),
        });
    }

    let base_url = settings.share.base_url.trim();
    let token = settings.share.token.trim();
    if base_url.is_empty() || token.is_empty() {
        return Err(FrontendError::ShareSettingsMissing);
    }

    Ok(build_share_url(base_url, token, session))
}

pub fn save_settings_change(
    path: Option<&Path>,
    settings: &Settings,
    success_message: Option<String>,
) -> Result<String, FrontendError> {
    let Some(path) = path else {
        return Ok(
            success_message.unwrap_or_else(|| "Settings updated for this run only.".to_string())
        );
    };

    save_settings(path, settings).map_err(|err| FrontendError::SettingsSaveFailed {
        message: format!("{err:#}"),
    })?;
    Ok(success_message.unwrap_or_else(|| format!("Settings saved to {}", path.to_string_lossy())))
}

pub fn save_settings_change_owned(
    path: Option<PathBuf>,
    settings: &Settings,
    success_message: Option<String>,
) -> Result<String, FrontendError> {
    save_settings_change(path.as_deref(), settings, success_message)
}

fn ensure_local_launch(session: &Session) -> Result<(), FrontendError> {
    if session.is_local() {
        Ok(())
    } else {
        Err(FrontendError::RemoteBrowseOnly {
            origin: session.origin.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ProviderKind, SessionOrigin};

    #[test]
    fn blocks_remote_launch_with_user_message() {
        let mut session = session();
        session.origin = SessionOrigin::Remote("work".to_string());

        let error = default_resume_for_session(&session).unwrap_err();

        assert_eq!(
            error.message(),
            "Remote sessions are browse-only in this version: work"
        );
    }

    #[test]
    fn builds_share_url_from_settings() {
        let mut settings = Settings::default();
        settings.share.base_url = "http://host:8787".to_string();
        settings.share.token = "secret".to_string();

        let url = share_url_for_session(&settings, &session()).unwrap();

        assert_eq!(url, "http://host:8787/s/codex/sid?token=secret");
    }

    fn session() -> Session {
        Session {
            origin: SessionOrigin::Local,
            provider: ProviderKind::Codex,
            id: "sid".to_string(),
            title: "title".to_string(),
            cwd: "/tmp".to_string(),
            created_at_ms: None,
            updated_at_ms: None,
            model: None,
            source_path: "/tmp/session".into(),
            first_user_message: None,
            transcript: Vec::new(),
            resume_program: "codex".to_string(),
            resume_args: vec!["resume".to_string(), "sid".to_string()],
        }
    }
}
