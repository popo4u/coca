use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::launch::{build_launch_target, default_resume_target, launch_options, LaunchMode};

use super::app::{session_key, Action, App, LaunchDialog};

impl App {
    pub(super) fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        if self.search_mode {
            match key.code {
                KeyCode::Esc => {
                    self.search_mode = false;
                }
                KeyCode::Enter => {
                    self.search_mode = false;
                }
                KeyCode::Backspace => {
                    self.query.pop();
                    self.apply_filter();
                }
                KeyCode::Char(ch) => {
                    if !key.modifiers.contains(KeyModifiers::CONTROL) {
                        self.query.push(ch);
                        self.apply_filter();
                    }
                }
                _ => {}
            }
            return None;
        }

        if self.transcript_session.is_some() {
            match key.code {
                KeyCode::Esc => {
                    self.transcript_session = None;
                    self.transcript_scroll = 0;
                }
                KeyCode::Char('l') | KeyCode::Right | KeyCode::PageDown => {
                    self.transcript_scroll = self.transcript_scroll.saturating_add(10);
                }
                KeyCode::Char('h') | KeyCode::Left | KeyCode::PageUp => {
                    self.transcript_scroll = self.transcript_scroll.saturating_sub(10);
                }
                _ => {}
            }
            return None;
        }

        if self.launch_dialog.is_some() {
            return self.handle_launch_dialog_key(key);
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Some(Action::Quit),
            KeyCode::Char('/') => {
                self.search_mode = true;
                None
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(Action::Quit)
            }
            KeyCode::Tab => {
                self.provider_filter = self.provider_filter.next();
                self.apply_filter();
                None
            }
            KeyCode::Char(' ') => {
                self.toggle_detail();
                None
            }
            KeyCode::Char('t') => {
                self.transcript_session = self.selected_key();
                self.transcript_scroll = 0;
                None
            }
            KeyCode::Char('s') => {
                self.open_launch_dialog(LaunchMode::Resume);
                None
            }
            KeyCode::Char('f') => {
                self.open_launch_dialog(LaunchMode::Fork);
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_selection(1);
                None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_selection(-1);
                None
            }
            KeyCode::PageDown => {
                self.move_selection(10);
                None
            }
            KeyCode::PageUp => {
                self.move_selection(-10);
                None
            }
            KeyCode::Char('g') => {
                if !self.filtered_indices.is_empty() {
                    self.list_state.select(Some(0));
                }
                None
            }
            KeyCode::Char('G') => {
                if !self.filtered_indices.is_empty() {
                    self.list_state
                        .select(Some(self.filtered_indices.len() - 1));
                }
                None
            }
            KeyCode::Enter => self
                .selected_session()
                .map(|session| Action::Resume(default_resume_target(session))),
            _ => None,
        }
    }

    fn handle_launch_dialog_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Esc => {
                self.launch_dialog = None;
                None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(dialog) = &mut self.launch_dialog {
                    dialog.selected_option = dialog.selected_option.saturating_sub(1);
                }
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(dialog) = &mut self.launch_dialog {
                    dialog.selected_option =
                        (dialog.selected_option + 1).min(dialog.options.len().saturating_sub(1));
                }
                None
            }
            KeyCode::Char(' ') => {
                if let Some(dialog) = &mut self.launch_dialog {
                    if let Some(option) = dialog.options.get_mut(dialog.selected_option) {
                        option.enabled = !option.enabled;
                    }
                }
                None
            }
            KeyCode::Enter => {
                let dialog = self.launch_dialog.take()?;
                let session = self.session_by_key(&dialog.session)?;
                Some(Action::Resume(build_launch_target(
                    session,
                    dialog.mode,
                    &self.current_cwd,
                    &dialog.options,
                )))
            }
            _ => None,
        }
    }

    fn open_launch_dialog(&mut self, mode: LaunchMode) {
        let Some(session) = self.selected_session() else {
            return;
        };
        self.launch_dialog = Some(LaunchDialog {
            session: session_key(session),
            mode,
            selected_option: 0,
            options: launch_options(session, &self.current_cwd),
        });
    }

    fn toggle_detail(&mut self) {
        let Some(key) = self.selected_key() else {
            return;
        };
        if self.expanded_session.as_ref() == Some(&key) {
            self.expanded_session = None;
        } else {
            self.expanded_session = Some(key);
        }
    }

    fn move_selection(&mut self, delta: isize) {
        if self.filtered_indices.is_empty() {
            return;
        }
        let len = self.filtered_indices.len() as isize;
        let current = self.list_state.selected().unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, len - 1) as usize;
        self.list_state.select(Some(next));
        if next as isize != current {
            self.expanded_session = None;
        }
    }
}
