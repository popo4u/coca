use std::path::Path;

use anyhow::Result;
use coca_core::catalog::SessionCatalog;
use coca_core::launch::{LaunchMode, LaunchOption, ResumeTarget};
use coca_core::model::Session;
use coca_core::settings::Settings;

pub trait DaemonClient {
    fn session_catalog(&mut self) -> Result<SessionCatalog>;
    fn settings(&mut self) -> Result<Settings>;
    fn update_settings(&mut self, settings: &Settings) -> Result<SettingsUpdate>;
    fn share_url(&mut self, session: &Session) -> Result<String>;
    fn launch_options(
        &mut self,
        session: &Session,
        mode: LaunchMode,
        current_cwd: &Path,
    ) -> Result<Vec<LaunchOption>>;
    fn prepare_launch(
        &mut self,
        session: &Session,
        mode: LaunchMode,
        current_cwd: &Path,
        options: &[LaunchOption],
    ) -> Result<ResumeTarget>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsUpdate {
    pub settings: Settings,
    pub status_message: String,
}
