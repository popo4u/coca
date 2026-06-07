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

use crate::launch::{LaunchMode, LaunchOption, ResumeTarget};
use crate::model::{ProviderFilter, ProviderKind, Session};

pub fn run_tui(
    sessions: Vec<Session>,
    initial_filter: ProviderFilter,
) -> Result<Option<ResumeTarget>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut guard = TerminalGuard { restored: false };
    let result = run_loop(&mut terminal, sessions, initial_filter);
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
) -> Result<Option<ResumeTarget>> {
    let mut app = App::new(sessions, initial_filter);
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
    pub(super) launch_dialog: Option<LaunchDialog>,
    pub(super) current_cwd: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SessionKey {
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

impl App {
    pub(super) fn new(sessions: Vec<Session>, provider_filter: ProviderFilter) -> Self {
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
            launch_dialog: None,
            current_cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
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

    pub(super) fn session_by_key(&self, key: &SessionKey) -> Option<&Session> {
        self.sessions
            .iter()
            .find(|session| session.provider == key.provider && session.id == key.id)
    }

    fn session_key_is_visible(&self, key: &SessionKey) -> bool {
        self.filtered_indices.iter().any(|idx| {
            self.sessions
                .get(*idx)
                .map(|session| session.provider == key.provider && session.id == key.id)
                .unwrap_or(false)
        })
    }
}

pub(super) fn session_key(session: &Session) -> SessionKey {
    SessionKey {
        provider: session.provider,
        id: session.id.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent};

    use crate::model::ChatMessage;

    #[test]
    fn filters_sessions_by_provider_and_query() {
        let mut app = App::new(
            vec![
                session(ProviderKind::Codex, "a", "hello codex"),
                session(ProviderKind::Claude, "b", "hello claude"),
            ],
            ProviderFilter::All,
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
        let mut app = App::new(
            vec![session(ProviderKind::Claude, "sid", "hello claude")],
            ProviderFilter::All,
        );

        let Some(Action::Resume(target)) = app.handle_key(KeyEvent::from(KeyCode::Enter)) else {
            panic!("expected resume action");
        };

        assert_eq!(target.program, "claude");
        assert_eq!(target.cwd.as_deref(), Some(std::path::Path::new("/tmp")));
    }

    #[test]
    fn detail_is_collapsed_by_default_and_space_toggles_selected_row() {
        let mut app = App::new(
            vec![session(ProviderKind::Claude, "sid", "hello claude")],
            ProviderFilter::All,
        );

        assert!(app.expanded_session.is_none());

        app.handle_key(KeyEvent::from(KeyCode::Char(' ')));
        assert_eq!(
            app.expanded_session,
            Some(SessionKey {
                provider: ProviderKind::Claude,
                id: "sid".to_string()
            })
        );

        app.handle_key(KeyEvent::from(KeyCode::Char(' ')));
        assert!(app.expanded_session.is_none());
    }

    #[test]
    fn moving_selection_collapses_inline_detail() {
        let mut app = App::new(
            vec![
                session(ProviderKind::Codex, "a", "hello codex"),
                session(ProviderKind::Claude, "b", "hello claude"),
            ],
            ProviderFilter::All,
        );

        app.handle_key(KeyEvent::from(KeyCode::Char(' ')));
        assert!(app.expanded_session.is_some());

        app.handle_key(KeyEvent::from(KeyCode::Down));
        assert!(app.expanded_session.is_none());
        assert_eq!(app.selected_session().unwrap().id, "b");
    }

    #[test]
    fn transcript_modal_uses_h_l_paging_and_escape_closes() {
        let mut app = App::new(
            vec![session(ProviderKind::Claude, "sid", "hello claude")],
            ProviderFilter::All,
        );

        app.handle_key(KeyEvent::from(KeyCode::Char('t')));
        assert_eq!(
            app.transcript_session,
            Some(SessionKey {
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
    fn s_opens_launch_dialog_and_enter_builds_codex_resume_with_options() {
        let mut app = App::new(
            vec![session(ProviderKind::Codex, "sid", "hello codex")],
            ProviderFilter::All,
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
        let mut app = App::new(
            vec![session(ProviderKind::Claude, "sid", "hello claude")],
            ProviderFilter::All,
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

    fn session(provider: ProviderKind, id: &str, title: &str) -> Session {
        Session {
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
