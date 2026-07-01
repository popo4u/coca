use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use coca_core::launch::LaunchMode;

use super::app::{
    session_key, Action, App, ConfigEdit, ConfigItem, ConfigPage, HelpPage, LaunchDialog,
    ShareDialog,
};

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
                KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.query.push(ch);
                    self.apply_filter();
                }
                _ => {}
            }
            return None;
        }

        if self.help_page.is_some() {
            return self.handle_help_key(key);
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

        if self.share_dialog.is_some() {
            match key.code {
                KeyCode::Esc | KeyCode::Char('u') => {
                    self.share_dialog = None;
                }
                _ => {}
            }
            return None;
        }

        if self.config_edit.is_some() {
            return self.handle_config_edit_key(key);
        }

        if self.config_page.is_some() {
            return self.handle_config_key(key);
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
            KeyCode::Char(',') => {
                self.config_page = Some(ConfigPage::default());
                self.clamp_config_selection();
                None
            }
            KeyCode::Char('?') => {
                self.help_page = Some(HelpPage);
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
            KeyCode::Char('u') => {
                self.open_share_dialog();
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
            KeyCode::Enter => {
                let session = self.selected_session()?.clone();
                match self.daemon_client.prepare_launch(
                    &session,
                    LaunchMode::Resume,
                    &self.current_cwd,
                    &[],
                ) {
                    Ok(target) => Some(Action::Resume(target)),
                    Err(err) => {
                        self.status_message = Some(err.to_string());
                        None
                    }
                }
            }
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
                let session = self.session_by_key(&dialog.session)?.clone();
                match self.daemon_client.prepare_launch(
                    &session,
                    dialog.mode,
                    &self.current_cwd,
                    &dialog.options,
                ) {
                    Ok(target) => Some(Action::Resume(target)),
                    Err(err) => {
                        self.status_message = Some(err.to_string());
                        None
                    }
                }
            }
            _ => None,
        }
    }

    fn handle_config_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Esc | KeyCode::Char(',') => {
                self.config_edit = None;
                self.config_page = None;
                None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(config_page) = &mut self.config_page {
                    config_page.selected_item = config_page.selected_item.saturating_sub(1);
                }
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let item_count = self.config_items().len();
                if let Some(config_page) = &mut self.config_page {
                    config_page.selected_item =
                        (config_page.selected_item + 1).min(item_count.saturating_sub(1));
                }
                None
            }
            KeyCode::Char(' ') | KeyCode::Enter => {
                self.toggle_selected_config_item();
                None
            }
            _ => None,
        }
    }

    fn handle_config_edit_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Esc => {
                self.config_edit = None;
                None
            }
            KeyCode::Enter => {
                self.save_config_edit();
                None
            }
            KeyCode::Backspace => {
                if let Some(edit) = &mut self.config_edit {
                    edit.input.pop();
                }
                None
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(edit) = &mut self.config_edit {
                    edit.input.clear();
                }
                None
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(edit) = &mut self.config_edit {
                    edit.input.push(ch);
                }
                None
            }
            _ => None,
        }
    }

    fn handle_help_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('?') => {
                self.help_page = None;
                None
            }
            _ => None,
        }
    }

    fn open_launch_dialog(&mut self, mode: LaunchMode) {
        let Some(session) = self.selected_session().cloned() else {
            return;
        };
        let options = match self
            .daemon_client
            .launch_options(&session, mode, &self.current_cwd)
        {
            Ok(options) => options,
            Err(err) => {
                self.status_message = Some(err.to_string());
                return;
            }
        };
        self.launch_dialog = Some(LaunchDialog {
            session: session_key(&session),
            mode,
            selected_option: 0,
            options,
        });
    }

    fn open_share_dialog(&mut self) {
        let Some(session) = self.selected_session().cloned() else {
            return;
        };
        let url = match self.daemon_client.share_url(&session) {
            Ok(url) => url,
            Err(err) => {
                self.status_message = Some(err.to_string());
                return;
            }
        };

        self.share_dialog = Some(ShareDialog {
            session: session_key(&session),
            url,
        });
    }

    fn toggle_selected_config_item(&mut self) {
        let Some(selected_item) = self
            .config_page
            .as_ref()
            .map(|config_page| config_page.selected_item)
        else {
            return;
        };
        let Some(item) = self.config_items().get(selected_item).cloned() else {
            return;
        };

        match item {
            ConfigItem::OriginLocal => {
                self.settings.origin_visibility.local = !self.settings.origin_visibility.local;
                self.apply_filter();
            }
            ConfigItem::OriginRemote(name) => {
                let enabled = !self.settings.remote_enabled(&name);
                self.settings.set_remote_enabled(&name, enabled);
                self.apply_filter();
                if enabled && !self.has_remote_sessions(&name) {
                    self.status_message = Some(format!(
                        "Settings saved. Restart coca to load remote {name}."
                    ));
                }
            }
            ConfigItem::LaunchDefault { mode, kind } => {
                let enabled = !self.settings.launch_default(mode, kind);
                self.settings.set_launch_default(mode, kind, enabled);
            }
            ConfigItem::GatewayBind | ConfigItem::ShareBaseUrl => {
                self.open_config_edit(item);
                return;
            }
        }

        self.clamp_config_selection();
        self.save_settings_from_tui(None);
    }

    fn open_config_edit(&mut self, item: ConfigItem) {
        let input = match &item {
            ConfigItem::GatewayBind => self.settings.gateway.bind.clone(),
            ConfigItem::ShareBaseUrl => self.settings.share.base_url.clone(),
            ConfigItem::OriginLocal
            | ConfigItem::OriginRemote(_)
            | ConfigItem::LaunchDefault { .. } => String::new(),
        };
        self.config_edit = Some(ConfigEdit { item, input });
    }

    fn save_config_edit(&mut self) {
        let Some(edit) = self.config_edit.take() else {
            return;
        };

        let restart_gateway = matches!(
            &edit.item,
            ConfigItem::GatewayBind | ConfigItem::ShareBaseUrl
        );
        match edit.item {
            ConfigItem::GatewayBind => {
                self.settings.gateway.bind = edit.input.trim().to_string();
            }
            ConfigItem::ShareBaseUrl => {
                self.settings.share.base_url = edit.input.trim().to_string();
            }
            ConfigItem::OriginLocal
            | ConfigItem::OriginRemote(_)
            | ConfigItem::LaunchDefault { .. } => {}
        }

        self.settings.ensure_defaults();
        let success_message = restart_gateway.then(|| {
            "Settings saved. Restart coca gateway for changes to take effect.".to_string()
        });
        self.save_settings_from_tui(success_message);
    }

    fn has_remote_sessions(&self, name: &str) -> bool {
        self.sessions.iter().any(|session| {
            matches!(
                &session.origin,
                coca_core::model::SessionOrigin::Remote(remote_name) if remote_name == name
            )
        })
    }

    fn save_settings_from_tui(&mut self, success_message: Option<String>) {
        match self.daemon_client.update_settings(&self.settings) {
            Ok(update) => {
                self.settings = update.settings;
                let message = success_message.unwrap_or(update.status_message);
                if self.status_message.is_none()
                    || !self
                        .status_message
                        .as_deref()
                        .unwrap_or_default()
                        .starts_with("Settings saved. Restart")
                {
                    self.status_message = Some(message);
                }
            }
            Err(err) => {
                self.status_message = Some(format!("Failed to save settings: {err:#}"));
            }
        }
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
