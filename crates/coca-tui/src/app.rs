use std::io::{self, Stdout};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::widgets::ListState;
use ratatui::Terminal;

use coca_core::launch::{LaunchMode, LaunchOption, LaunchOptionKind, ResumeTarget};
use coca_core::model::{ProviderFilter, ProviderKind, Session, SessionOrigin};
use coca_core::settings::Settings;

use crate::core_client::CoreClient;
#[cfg(test)]
use crate::core_client::SettingsUpdate;
#[cfg(test)]
use anyhow::anyhow;
#[cfg(test)]
use coca_core::catalog::SessionCatalog;
#[cfg(test)]
use coca_core::frontend::{
    default_resume_for_session, launch_options_with_defaults, prepare_launch, share_url_for_session,
};

pub fn run_tui(
    mut core_client: Box<dyn CoreClient>,
    initial_filter: ProviderFilter,
) -> Result<Option<ResumeTarget>> {
    let settings = core_client.settings()?;
    let catalog = core_client.session_catalog()?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut guard = TerminalGuard { restored: false };
    let result = run_loop(
        &mut terminal,
        catalog.sessions,
        initial_filter,
        catalog.warnings,
        settings,
        core_client,
    );
    guard.restore()?;
    result
}

struct TerminalGuard {
    restored: bool,
}

impl TerminalGuard {
    fn restore(&mut self) -> Result<()> {
        if self.restored {
            return Ok(());
        }
        disable_raw_mode()?;
        execute!(io::stdout(), LeaveAlternateScreen)?;
        self.restored = true;
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if !self.restored {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
            self.restored = true;
        }
    }
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    sessions: Vec<Session>,
    initial_filter: ProviderFilter,
    warnings: Vec<String>,
    settings: Settings,
    core_client: Box<dyn CoreClient>,
) -> Result<Option<ResumeTarget>> {
    let mut app = App::new_with_settings(sessions, initial_filter, warnings, settings, core_client);
    loop {
        terminal.draw(|frame| app.render(frame))?;
        if !event::poll(Duration::from_millis(150))? {
            continue;
        }

        let Event::Key(key) = event::read()? else {
            continue;
        };
        if let Some(action) = app.handle_key(key) {
            match action {
                Action::Quit => return Ok(None),
                Action::Resume(target) => return Ok(Some(target)),
            }
        }
    }
}

pub(super) enum Action {
    Quit,
    Resume(ResumeTarget),
}

pub(super) struct App {
    pub(super) sessions: Vec<Session>,
    pub(super) filtered_indices: Vec<usize>,
    pub(super) list_state: ListState,
    pub(super) provider_filter: ProviderFilter,
    pub(super) query: String,
    pub(super) search_mode: bool,
    pub(super) expanded_session: Option<SessionKey>,
    pub(super) transcript_session: Option<SessionKey>,
    pub(super) transcript_scroll: u16,
    pub(super) share_dialog: Option<ShareDialog>,
    pub(super) launch_dialog: Option<LaunchDialog>,
    pub(super) config_page: Option<ConfigPage>,
    pub(super) config_edit: Option<ConfigEdit>,
    pub(super) help_page: Option<HelpPage>,
    pub(super) current_cwd: PathBuf,
    pub(super) status_message: Option<String>,
    pub(super) settings: Settings,
    pub(super) core_client: Box<dyn CoreClient>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SessionKey {
    pub(super) origin: SessionOrigin,
    pub(super) provider: ProviderKind,
    pub(super) id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct LaunchDialog {
    pub(super) session: SessionKey,
    pub(super) mode: LaunchMode,
    pub(super) selected_option: usize,
    pub(super) options: Vec<LaunchOption>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ShareDialog {
    pub(super) session: SessionKey,
    pub(super) url: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct ConfigPage {
    pub(super) selected_item: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ConfigEdit {
    pub(super) item: ConfigItem,
    pub(super) input: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct HelpPage;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum ConfigItem {
    OriginLocal,
    OriginRemote(String),
    CoreBind,
    LaunchDefault {
        mode: LaunchMode,
        kind: LaunchOptionKind,
    },
    ShareBaseUrl,
    ShareToken,
}

impl App {
    #[cfg(test)]
    pub(super) fn new_with_warnings(
        sessions: Vec<Session>,
        provider_filter: ProviderFilter,
        warnings: Vec<String>,
    ) -> Self {
        Self::new_with_test_settings(sessions, provider_filter, warnings, Settings::default())
    }

    #[cfg(test)]
    pub(super) fn new_with_test_settings(
        sessions: Vec<Session>,
        provider_filter: ProviderFilter,
        warnings: Vec<String>,
        settings: Settings,
    ) -> Self {
        let mut client_settings = settings.clone();
        client_settings.ensure_defaults();
        Self::new_with_settings_and_client(
            sessions.clone(),
            provider_filter,
            warnings,
            settings.clone(),
            Box::new(TestCoreClient::new(sessions, client_settings, false)),
        )
    }

    pub(super) fn new_with_settings(
        sessions: Vec<Session>,
        provider_filter: ProviderFilter,
        warnings: Vec<String>,
        settings: Settings,
        core_client: Box<dyn CoreClient>,
    ) -> Self {
        Self::new_with_settings_and_client(
            sessions,
            provider_filter,
            warnings,
            settings,
            core_client,
        )
    }

    fn new_with_settings_and_client(
        sessions: Vec<Session>,
        provider_filter: ProviderFilter,
        warnings: Vec<String>,
        mut settings: Settings,
        core_client: Box<dyn CoreClient>,
    ) -> Self {
        settings.ensure_defaults();
        let mut app = Self {
            sessions,
            filtered_indices: Vec::new(),
            list_state: ListState::default(),
            provider_filter,
            query: String::new(),
            search_mode: false,
            expanded_session: None,
            transcript_session: None,
            transcript_scroll: 0,
            share_dialog: None,
            launch_dialog: None,
            config_page: None,
            config_edit: None,
            help_page: None,
            current_cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            status_message: if warnings.is_empty() {
                None
            } else {
                Some(format!("Remote load warnings: {}", warnings.join("; ")))
            },
            settings,
            core_client,
        };
        app.apply_filter();
        app
    }

    pub(super) fn apply_filter(&mut self) {
        let query = self.query.to_lowercase();
        self.filtered_indices = self
            .sessions
            .iter()
            .enumerate()
            .filter(|(_, session)| self.provider_filter.includes(session.provider))
            .filter(|(_, session)| self.settings.origin_visible(&session.origin))
            .filter(|(_, session)| query.is_empty() || session.searchable_text().contains(&query))
            .map(|(idx, _)| idx)
            .collect();

        if self.filtered_indices.is_empty() {
            self.list_state.select(None);
        } else {
            let current = self.list_state.selected().unwrap_or(0);
            self.list_state
                .select(Some(current.min(self.filtered_indices.len() - 1)));
        }
        if let Some(expanded) = &self.expanded_session {
            if !self.session_key_is_visible(expanded) {
                self.expanded_session = None;
            }
        }
        self.transcript_scroll = 0;
    }

    pub(super) fn selected_session(&self) -> Option<&Session> {
        let selected = self.selected_index()?;
        self.filtered_indices
            .get(selected)
            .and_then(|idx| self.sessions.get(*idx))
    }

    pub(super) fn selected_index(&self) -> Option<usize> {
        self.list_state.selected()
    }

    pub(super) fn selected_key(&self) -> Option<SessionKey> {
        self.selected_session().map(session_key)
    }

    pub(super) fn visible_session_count(&self) -> usize {
        self.sessions
            .iter()
            .filter(|session| self.settings.origin_visible(&session.origin))
            .count()
    }

    pub(super) fn session_by_key(&self, key: &SessionKey) -> Option<&Session> {
        self.sessions.iter().find(|session| {
            session.origin == key.origin && session.provider == key.provider && session.id == key.id
        })
    }

    fn session_key_is_visible(&self, key: &SessionKey) -> bool {
        self.filtered_indices.iter().any(|idx| {
            self.sessions
                .get(*idx)
                .map(|session| {
                    session.origin == key.origin
                        && session.provider == key.provider
                        && session.id == key.id
                })
                .unwrap_or(false)
        })
    }

    pub(super) fn config_items(&self) -> Vec<ConfigItem> {
        let mut remote_names = std::collections::BTreeSet::new();
        for remote in &self.settings.remotes {
            remote_names.insert(remote.name.clone());
        }
        for session in &self.sessions {
            if let SessionOrigin::Remote(name) = &session.origin {
                remote_names.insert(name.clone());
            }
        }

        let mut items = vec![ConfigItem::OriginLocal];
        items.extend(remote_names.into_iter().map(ConfigItem::OriginRemote));
        items.extend([
            ConfigItem::CoreBind,
            ConfigItem::ShareBaseUrl,
            ConfigItem::ShareToken,
            ConfigItem::LaunchDefault {
                mode: LaunchMode::Resume,
                kind: LaunchOptionKind::UseCurrentDir,
            },
            ConfigItem::LaunchDefault {
                mode: LaunchMode::Resume,
                kind: LaunchOptionKind::Yolo,
            },
            ConfigItem::LaunchDefault {
                mode: LaunchMode::Fork,
                kind: LaunchOptionKind::UseCurrentDir,
            },
            ConfigItem::LaunchDefault {
                mode: LaunchMode::Fork,
                kind: LaunchOptionKind::Yolo,
            },
        ]);
        items
    }

    pub(super) fn clamp_config_selection(&mut self) {
        let item_count = self.config_items().len();
        if let Some(config_page) = &mut self.config_page {
            config_page.selected_item = config_page.selected_item.min(item_count.saturating_sub(1));
        }
    }
}

pub(super) fn session_key(session: &Session) -> SessionKey {
    SessionKey {
        origin: session.origin.clone(),
        provider: session.provider,
        id: session.id.clone(),
    }
}

#[cfg(test)]
struct TestCoreClient {
    sessions: Vec<Session>,
    settings: Settings,
    persisted: bool,
}

#[cfg(test)]
impl TestCoreClient {
    fn new(sessions: Vec<Session>, settings: Settings, persisted: bool) -> Self {
        Self {
            sessions,
            settings,
            persisted,
        }
    }
}

#[cfg(test)]
impl CoreClient for TestCoreClient {
    fn session_catalog(&mut self) -> Result<SessionCatalog> {
        Ok(SessionCatalog {
            sessions: self.sessions.clone(),
            warnings: Vec::new(),
        })
    }

    fn settings(&mut self) -> Result<Settings> {
        Ok(self.settings.clone())
    }

    fn update_settings(&mut self, settings: &Settings) -> Result<SettingsUpdate> {
        self.settings = settings.clone();
        Ok(SettingsUpdate {
            settings: self.settings.clone(),
            status_message: if self.persisted {
                "Settings saved.".to_string()
            } else {
                "Settings updated for this run only.".to_string()
            },
        })
    }

    fn share_url(&mut self, session: &Session) -> Result<String> {
        share_url_for_session(&self.settings, session).map_err(|err| anyhow!(err.message()))
    }

    fn launch_options(
        &mut self,
        session: &Session,
        mode: LaunchMode,
        current_cwd: &std::path::Path,
    ) -> Result<Vec<LaunchOption>> {
        launch_options_with_defaults(&self.settings, session, mode, current_cwd)
            .map_err(|err| anyhow!(err.message()))
    }

    fn prepare_launch(
        &mut self,
        session: &Session,
        mode: LaunchMode,
        current_cwd: &std::path::Path,
        options: &[LaunchOption],
    ) -> Result<ResumeTarget> {
        if mode == LaunchMode::Resume && options.is_empty() {
            return default_resume_for_session(session).map_err(|err| anyhow!(err.message()));
        }
        prepare_launch(session, mode, current_cwd, options).map_err(|err| anyhow!(err.message()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use coca_core::launch::LaunchOptionKind;
    use coca_core::model::ChatMessage;
    use coca_core::settings::Settings;

    #[test]
    fn filters_sessions_by_provider_and_query() {
        let mut app = App::new_with_warnings(
            vec![
                session(ProviderKind::Codex, "a", "hello codex"),
                session(ProviderKind::Claude, "b", "hello claude"),
            ],
            ProviderFilter::All,
            Vec::new(),
        );
        assert_eq!(app.filtered_indices.len(), 2);

        app.provider_filter = ProviderFilter::Claude;
        app.apply_filter();
        assert_eq!(app.filtered_indices.len(), 1);
        assert_eq!(app.selected_session().unwrap().id, "b");

        app.query = "codex".to_string();
        app.apply_filter();
        assert!(app.filtered_indices.is_empty());
    }

    #[test]
    fn resume_target_uses_session_cwd() {
        let mut app = App::new_with_warnings(
            vec![session(ProviderKind::Claude, "sid", "hello claude")],
            ProviderFilter::All,
            Vec::new(),
        );

        let Some(Action::Resume(target)) = app.handle_key(KeyEvent::from(KeyCode::Enter)) else {
            panic!("expected resume action");
        };

        assert_eq!(target.program, "claude");
        assert_eq!(target.cwd.as_deref(), Some(std::path::Path::new("/tmp")));
    }

    #[test]
    fn detail_is_collapsed_by_default_and_space_toggles_selected_row() {
        let mut app = App::new_with_warnings(
            vec![session(ProviderKind::Claude, "sid", "hello claude")],
            ProviderFilter::All,
            Vec::new(),
        );

        assert!(app.expanded_session.is_none());

        app.handle_key(KeyEvent::from(KeyCode::Char(' ')));
        assert_eq!(
            app.expanded_session,
            Some(SessionKey {
                origin: SessionOrigin::Local,
                provider: ProviderKind::Claude,
                id: "sid".to_string()
            })
        );

        app.handle_key(KeyEvent::from(KeyCode::Char(' ')));
        assert!(app.expanded_session.is_none());
    }

    #[test]
    fn moving_selection_collapses_inline_detail() {
        let mut app = App::new_with_warnings(
            vec![
                session(ProviderKind::Codex, "a", "hello codex"),
                session(ProviderKind::Claude, "b", "hello claude"),
            ],
            ProviderFilter::All,
            Vec::new(),
        );

        app.handle_key(KeyEvent::from(KeyCode::Char(' ')));
        assert!(app.expanded_session.is_some());

        app.handle_key(KeyEvent::from(KeyCode::Down));
        assert!(app.expanded_session.is_none());
        assert_eq!(app.selected_session().unwrap().id, "b");
    }

    #[test]
    fn transcript_modal_uses_h_l_paging_and_escape_closes() {
        let mut app = App::new_with_warnings(
            vec![session(ProviderKind::Claude, "sid", "hello claude")],
            ProviderFilter::All,
            Vec::new(),
        );

        app.handle_key(KeyEvent::from(KeyCode::Char('t')));
        assert_eq!(
            app.transcript_session,
            Some(SessionKey {
                origin: SessionOrigin::Local,
                provider: ProviderKind::Claude,
                id: "sid".to_string()
            })
        );

        app.handle_key(KeyEvent::from(KeyCode::Char('l')));
        assert_eq!(app.transcript_scroll, 10);

        app.handle_key(KeyEvent::from(KeyCode::Char('h')));
        assert_eq!(app.transcript_scroll, 0);

        assert!(app.handle_key(KeyEvent::from(KeyCode::Esc)).is_none());
        assert!(app.transcript_session.is_none());
    }

    #[test]
    fn u_opens_share_dialog_for_local_session() {
        let mut settings = Settings::default();
        settings.share.base_url = "http://host:8787".to_string();
        settings.share.token = "secret".to_string();
        let mut app = App::new_with_test_settings(
            vec![session(ProviderKind::Codex, "sid", "hello codex")],
            ProviderFilter::All,
            Vec::new(),
            settings,
        );

        app.handle_key(KeyEvent::from(KeyCode::Char('u')));

        assert_eq!(
            app.share_dialog.as_ref().map(|dialog| dialog.url.as_str()),
            Some("http://host:8787/s/codex/sid?token=secret")
        );
    }

    #[test]
    fn u_uses_default_share_settings() {
        let mut app = App::new_with_warnings(
            vec![session(ProviderKind::Codex, "sid", "hello codex")],
            ProviderFilter::All,
            Vec::new(),
        );

        app.handle_key(KeyEvent::from(KeyCode::Char('u')));

        let url = app.share_dialog.as_ref().map(|dialog| dialog.url.as_str());
        assert!(url
            .unwrap()
            .starts_with("http://127.0.0.1:8787/s/codex/sid?token="));
    }

    #[test]
    fn u_blocks_remote_share_url() {
        let mut remote = session(ProviderKind::Claude, "sid", "hello claude");
        remote.origin = SessionOrigin::Remote("work-mac".to_string());
        let mut settings = Settings::default();
        settings.share.base_url = "http://host:8787".to_string();
        settings.share.token = "secret".to_string();
        let mut app =
            App::new_with_test_settings(vec![remote], ProviderFilter::All, Vec::new(), settings);

        app.handle_key(KeyEvent::from(KeyCode::Char('u')));

        assert!(app.share_dialog.is_none());
        assert_eq!(
            app.status_message.as_deref(),
            Some("Remote sessions cannot be shared from this machine in v0: work-mac")
        );
    }

    #[test]
    fn s_opens_launch_dialog_and_enter_builds_codex_resume_with_options() {
        let mut app = App::new_with_warnings(
            vec![session(ProviderKind::Codex, "sid", "hello codex")],
            ProviderFilter::All,
            Vec::new(),
        );
        app.current_cwd = PathBuf::from("/current");

        app.handle_key(KeyEvent::from(KeyCode::Char('s')));
        assert_eq!(
            app.launch_dialog.as_ref().map(|dialog| dialog.mode),
            Some(LaunchMode::Resume)
        );

        app.handle_key(KeyEvent::from(KeyCode::Char(' ')));
        app.handle_key(KeyEvent::from(KeyCode::Down));
        app.handle_key(KeyEvent::from(KeyCode::Char(' ')));

        let Some(Action::Resume(target)) = app.handle_key(KeyEvent::from(KeyCode::Enter)) else {
            panic!("expected launch action");
        };

        assert_eq!(target.program, "codex");
        assert_eq!(
            target.args,
            vec![
                "resume",
                "-C",
                "/current",
                "--dangerously-bypass-approvals-and-sandbox",
                "sid"
            ]
        );
        assert_eq!(
            target.cwd.as_deref(),
            Some(std::path::Path::new("/current"))
        );
    }

    #[test]
    fn f_builds_claude_fork_with_skip_permissions() {
        let mut app = App::new_with_warnings(
            vec![session(ProviderKind::Claude, "sid", "hello claude")],
            ProviderFilter::All,
            Vec::new(),
        );

        app.handle_key(KeyEvent::from(KeyCode::Char('f')));
        assert_eq!(
            app.launch_dialog.as_ref().map(|dialog| dialog.mode),
            Some(LaunchMode::Fork)
        );

        app.handle_key(KeyEvent::from(KeyCode::Down));
        app.handle_key(KeyEvent::from(KeyCode::Char(' ')));

        let Some(Action::Resume(target)) = app.handle_key(KeyEvent::from(KeyCode::Enter)) else {
            panic!("expected launch action");
        };

        assert_eq!(target.program, "claude");
        assert_eq!(
            target.args,
            vec![
                "--resume",
                "sid",
                "--fork-session",
                "--dangerously-skip-permissions"
            ]
        );
        assert_eq!(target.cwd.as_deref(), Some(std::path::Path::new("/tmp")));
    }

    #[test]
    fn filters_sessions_by_origin_query() {
        let mut remote = session(ProviderKind::Codex, "sid", "hello codex");
        remote.origin = SessionOrigin::Remote("work-mac".to_string());
        let mut app = App::new_with_warnings(vec![remote], ProviderFilter::All, Vec::new());

        app.query = "work-mac".to_string();
        app.apply_filter();

        assert_eq!(app.filtered_indices.len(), 1);
    }

    #[test]
    fn session_keys_distinguish_origin() {
        let local = session(ProviderKind::Codex, "sid", "local");
        let mut remote = session(ProviderKind::Codex, "sid", "remote");
        remote.origin = SessionOrigin::Remote("work-mac".to_string());
        let app = App::new_with_warnings(vec![local, remote], ProviderFilter::All, Vec::new());

        let remote_key = SessionKey {
            origin: SessionOrigin::Remote("work-mac".to_string()),
            provider: ProviderKind::Codex,
            id: "sid".to_string(),
        };

        assert_eq!(app.session_by_key(&remote_key).unwrap().title, "remote");
    }

    #[test]
    fn enter_blocks_remote_resume() {
        let mut remote = session(ProviderKind::Claude, "sid", "hello claude");
        remote.origin = SessionOrigin::Remote("work-mac".to_string());
        let mut app = App::new_with_warnings(vec![remote], ProviderFilter::All, Vec::new());

        assert!(app.handle_key(KeyEvent::from(KeyCode::Enter)).is_none());
        assert_eq!(
            app.status_message.as_deref(),
            Some("Remote sessions are browse-only in this version: work-mac")
        );
    }

    #[test]
    fn s_blocks_remote_launch_dialog() {
        let mut remote = session(ProviderKind::Codex, "sid", "hello codex");
        remote.origin = SessionOrigin::Remote("work-mac".to_string());
        let mut app = App::new_with_warnings(vec![remote], ProviderFilter::All, Vec::new());

        app.handle_key(KeyEvent::from(KeyCode::Char('s')));

        assert!(app.launch_dialog.is_none());
        assert_eq!(
            app.status_message.as_deref(),
            Some("Remote sessions are browse-only in this version: work-mac")
        );
    }

    #[test]
    fn settings_hide_remote_origins_from_filter() {
        let mut remote = session(ProviderKind::Codex, "sid", "hello codex");
        remote.origin = SessionOrigin::Remote("work-mac".to_string());
        let mut settings = Settings::default();
        settings.set_remote_enabled("work-mac", false);

        let app =
            App::new_with_test_settings(vec![remote], ProviderFilter::All, Vec::new(), settings);

        assert!(app.filtered_indices.is_empty());
    }

    #[test]
    fn visible_session_count_tracks_origin_visibility() {
        let local = session(ProviderKind::Codex, "local", "hello local");
        let mut remote = session(ProviderKind::Claude, "remote", "hello remote");
        remote.origin = SessionOrigin::Remote("work-mac".to_string());
        let mut settings = Settings::default();
        settings.set_remote_enabled("work-mac", false);

        let app = App::new_with_test_settings(
            vec![local, remote],
            ProviderFilter::All,
            Vec::new(),
            settings,
        );

        assert_eq!(app.filtered_indices.len(), 1);
        assert_eq!(app.visible_session_count(), 1);
    }

    #[test]
    fn comma_opens_config_page_and_toggles_origin_visibility() {
        let mut app = App::new_with_warnings(
            vec![session(ProviderKind::Claude, "sid", "hello claude")],
            ProviderFilter::All,
            Vec::new(),
        );

        app.handle_key(KeyEvent::from(KeyCode::Char(',')));
        assert!(app.config_page.is_some());

        app.handle_key(KeyEvent::from(KeyCode::Char(' ')));

        assert!(!app.settings.origin_visibility.local);
        assert!(app.filtered_indices.is_empty());
        assert_eq!(
            app.status_message.as_deref(),
            Some("Settings updated for this run only.")
        );
    }

    #[test]
    fn config_page_edits_share_base_url() {
        let mut app = App::new_with_warnings(
            vec![session(ProviderKind::Claude, "sid", "hello claude")],
            ProviderFilter::All,
            Vec::new(),
        );

        app.handle_key(KeyEvent::from(KeyCode::Char(',')));
        app.handle_key(KeyEvent::from(KeyCode::Down));
        app.handle_key(KeyEvent::from(KeyCode::Down));
        app.handle_key(KeyEvent::from(KeyCode::Enter));

        assert_eq!(
            app.config_edit,
            Some(ConfigEdit {
                item: ConfigItem::ShareBaseUrl,
                input: "http://127.0.0.1:8787".to_string()
            })
        );

        app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        for ch in "http://192.168.1.20:8787".chars() {
            app.handle_key(KeyEvent::from(KeyCode::Char(ch)));
        }
        app.handle_key(KeyEvent::from(KeyCode::Enter));

        assert!(app.config_edit.is_none());
        assert_eq!(app.settings.share.base_url, "http://192.168.1.20:8787");
        assert_eq!(
            app.status_message.as_deref(),
            Some("Settings saved. Restart coca core for changes to take effect.")
        );
    }

    #[test]
    fn config_page_edits_core_bind() {
        let mut app = App::new_with_warnings(
            vec![session(ProviderKind::Claude, "sid", "hello claude")],
            ProviderFilter::All,
            Vec::new(),
        );

        app.handle_key(KeyEvent::from(KeyCode::Char(',')));
        app.handle_key(KeyEvent::from(KeyCode::Down));
        app.handle_key(KeyEvent::from(KeyCode::Enter));

        assert_eq!(
            app.config_edit,
            Some(ConfigEdit {
                item: ConfigItem::CoreBind,
                input: "0.0.0.0:8787".to_string()
            })
        );

        app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        for ch in "127.0.0.1:9999".chars() {
            app.handle_key(KeyEvent::from(KeyCode::Char(ch)));
        }
        app.handle_key(KeyEvent::from(KeyCode::Enter));

        assert_eq!(app.settings.core.bind, "127.0.0.1:9999");
        assert_eq!(
            app.status_message.as_deref(),
            Some("Settings saved. Restart coca core for changes to take effect.")
        );
    }

    #[test]
    fn config_page_edits_share_token() {
        let mut app = App::new_with_warnings(
            vec![session(ProviderKind::Claude, "sid", "hello claude")],
            ProviderFilter::All,
            Vec::new(),
        );

        app.handle_key(KeyEvent::from(KeyCode::Char(',')));
        app.handle_key(KeyEvent::from(KeyCode::Down));
        app.handle_key(KeyEvent::from(KeyCode::Down));
        app.handle_key(KeyEvent::from(KeyCode::Down));
        app.handle_key(KeyEvent::from(KeyCode::Enter));

        app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        for ch in "secret".chars() {
            app.handle_key(KeyEvent::from(KeyCode::Char(ch)));
        }
        app.handle_key(KeyEvent::from(KeyCode::Enter));

        assert_eq!(app.settings.share.token, "secret");
    }

    #[test]
    fn config_page_toggles_launch_defaults_used_by_s_dialog() {
        let mut app = App::new_with_warnings(
            vec![session(ProviderKind::Codex, "sid", "hello codex")],
            ProviderFilter::All,
            Vec::new(),
        );

        app.handle_key(KeyEvent::from(KeyCode::Char(',')));
        app.handle_key(KeyEvent::from(KeyCode::Down));
        app.handle_key(KeyEvent::from(KeyCode::Down));
        app.handle_key(KeyEvent::from(KeyCode::Down));
        app.handle_key(KeyEvent::from(KeyCode::Down));
        app.handle_key(KeyEvent::from(KeyCode::Down));
        app.handle_key(KeyEvent::from(KeyCode::Char(' ')));
        app.handle_key(KeyEvent::from(KeyCode::Esc));

        assert!(app
            .settings
            .launch_default(LaunchMode::Resume, LaunchOptionKind::Yolo));

        app.handle_key(KeyEvent::from(KeyCode::Char('s')));
        let dialog = app.launch_dialog.as_ref().unwrap();
        assert!(dialog
            .options
            .iter()
            .any(|option| option.kind == LaunchOptionKind::Yolo && option.enabled));
    }

    #[test]
    fn launch_dialog_uses_configured_f_defaults() {
        let mut settings = Settings::default();
        settings.set_launch_default(LaunchMode::Fork, LaunchOptionKind::UseCurrentDir, true);
        let mut app = App::new_with_test_settings(
            vec![session(ProviderKind::Claude, "sid", "hello claude")],
            ProviderFilter::All,
            Vec::new(),
            settings,
        );
        app.current_cwd = PathBuf::from("/current");

        app.handle_key(KeyEvent::from(KeyCode::Char('f')));
        let dialog = app.launch_dialog.as_ref().unwrap();

        assert!(dialog
            .options
            .iter()
            .any(|option| { option.kind == LaunchOptionKind::UseCurrentDir && option.enabled }));
    }

    #[test]
    fn question_mark_opens_help_page_and_escape_closes() {
        let mut app = App::new_with_warnings(
            vec![session(ProviderKind::Codex, "sid", "hello codex")],
            ProviderFilter::All,
            Vec::new(),
        );

        app.handle_key(KeyEvent::from(KeyCode::Char('?')));
        assert_eq!(app.help_page, Some(HelpPage));

        app.handle_key(KeyEvent::from(KeyCode::Esc));
        assert!(app.help_page.is_none());
    }

    fn session(provider: ProviderKind, id: &str, title: &str) -> Session {
        Session {
            origin: SessionOrigin::Local,
            provider,
            id: id.to_string(),
            title: title.to_string(),
            cwd: "/tmp".to_string(),
            created_at_ms: Some(1),
            updated_at_ms: Some(2),
            model: None,
            source_path: "/tmp/session".into(),
            first_user_message: Some(title.to_string()),
            transcript: vec![
                ChatMessage {
                    role: "user".to_string(),
                    text: title.to_string(),
                    timestamp_ms: Some(1),
                },
                ChatMessage {
                    role: "assistant".to_string(),
                    text: "answer".to_string(),
                    timestamp_ms: Some(2),
                },
            ],
            resume_program: provider.to_string(),
            resume_args: vec!["resume".to_string(), id.to_string()],
        }
    }
}
