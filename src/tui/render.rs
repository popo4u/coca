use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap,
};
use ratatui::Frame;

use crate::model::Session;
use crate::tui::formatting::{short_id, short_path};

use super::app::{App, LaunchDialog};
use super::views::{
    centered_rect, centered_rect_fixed_height, launch_dialog_height, session_lines, transcript_text,
};

impl App {
    pub(super) fn render(&mut self, frame: &mut Frame<'_>) {
        let root = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(5),
                Constraint::Length(2),
            ])
            .split(frame.area());
        self.render_header(frame, root[0]);
        self.render_body(frame, root[1]);
        self.render_footer(frame, root[2]);

        if self.search_mode {
            self.render_search_overlay(frame);
        }
        if let Some(key) = &self.transcript_session {
            if let Some(session) = self.session_by_key(key) {
                self.render_transcript_modal(frame, session);
            }
        }
        if let Some(dialog) = &self.launch_dialog {
            if let Some(session) = self.session_by_key(&dialog.session) {
                self.render_launch_dialog(frame, dialog, session);
            }
        }
    }

    fn render_header(&self, frame: &mut Frame<'_>, area: Rect) {
        let title = format!(
            " coca  provider:{}  sessions:{}/{} ",
            self.provider_filter.label(),
            self.filtered_indices.len(),
            self.sessions.len()
        );
        let query = if self.query.is_empty() {
            "search:".to_string()
        } else {
            format!("search: {}", self.query)
        };
        let header = Paragraph::new(Line::from(vec![
            Span::styled(
                title,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(query, Style::default().fg(Color::Gray)),
        ]))
        .block(Block::default().borders(Borders::BOTTOM));
        frame.render_widget(header, area);
    }

    fn render_body(&mut self, frame: &mut Frame<'_>, area: Rect) {
        self.render_list(frame, area);
    }

    fn render_list(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let selected = self.selected_index();
        let mut all_lines = Vec::new();
        let mut selected_line = 0usize;
        let mut cursor = 0usize;

        for (visible_idx, session) in self
            .filtered_indices
            .iter()
            .filter_map(|idx| self.sessions.get(*idx))
            .enumerate()
        {
            let is_selected = selected == Some(visible_idx);
            if is_selected {
                selected_line = cursor;
            }
            let expanded = self
                .expanded_session
                .as_ref()
                .map(|key| {
                    key.origin == session.origin
                        && key.provider == session.provider
                        && key.id == session.id
                })
                .unwrap_or(false);
            let lines = session_lines(session, expanded, is_selected);
            cursor += lines.len();
            all_lines.extend(lines);
            all_lines.push(Line::raw(""));
            cursor += 1;
        }

        let height = area.height.saturating_sub(1) as usize;
        let top_line = selected_line.saturating_sub(height / 3);
        let visible_lines = all_lines
            .into_iter()
            .skip(top_line)
            .take(area.height as usize)
            .collect::<Vec<_>>();
        frame.render_widget(Paragraph::new(Text::from(visible_lines)), area);
    }

    fn render_transcript_modal(&self, frame: &mut Frame<'_>, session: &Session) {
        let area = centered_rect(84, 78, frame.area());
        frame.render_widget(Clear, area);
        let title = format!(
            " Transcript  {} {} {}  {} ",
            session.origin,
            session.provider,
            short_id(&session.id),
            session.title
        );
        let block = Block::default().title(title).borders(Borders::ALL);
        let inner = area.inner(Margin {
            vertical: 1,
            horizontal: 2,
        });
        frame.render_widget(block, area);
        let paragraph = Paragraph::new(transcript_text(session))
            .wrap(Wrap { trim: false })
            .scroll((self.transcript_scroll, 0));
        frame.render_widget(paragraph, inner);

        let mut scrollbar_state =
            ScrollbarState::new(100).position(self.transcript_scroll as usize);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
        frame.render_stateful_widget(scrollbar, inner, &mut scrollbar_state);
    }

    fn render_footer(&self, frame: &mut Frame<'_>, area: Rect) {
        let help = if let Some(message) = &self.status_message {
            message.as_str()
        } else if self.transcript_session.is_some() {
            "h/l page transcript  Esc close"
        } else if self.launch_dialog.is_some() {
            "↑/↓ option  Space toggle  Enter launch  Esc cancel"
        } else if self.search_mode {
            "type search  Enter accept  Esc close"
        } else {
            "↑/↓ move  / search  Tab provider  Space detail  t transcript  s execute  f fork  Enter resume  q quit"
        };
        let footer = Paragraph::new(help)
            .fg(Color::Gray)
            .block(Block::default().borders(Borders::TOP));
        frame.render_widget(footer, area);
    }

    fn render_search_overlay(&self, frame: &mut Frame<'_>) {
        let area = centered_rect_fixed_height(60, 3, frame.area());
        frame.render_widget(Clear, area);
        let input = Paragraph::new(self.query.as_str())
            .block(Block::default().title("Search").borders(Borders::ALL))
            .style(Style::default().fg(Color::Yellow));
        frame.render_widget(input, area);
    }

    fn render_launch_dialog(
        &self,
        frame: &mut Frame<'_>,
        dialog: &LaunchDialog,
        session: &Session,
    ) {
        let area = centered_rect_fixed_height(72, launch_dialog_height(dialog), frame.area());
        frame.render_widget(Clear, area);

        let mut lines = vec![
            Line::from(vec![
                Span::styled(
                    dialog.mode.label(),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled(
                    session.provider.to_string(),
                    Style::default().fg(Color::Gray),
                ),
                Span::raw(" "),
                Span::styled(short_id(&session.id), Style::default().fg(Color::DarkGray)),
            ]),
            Line::raw(""),
            Line::from(vec![
                Span::styled("Session cwd  ", Style::default().fg(Color::DarkGray)),
                Span::raw(short_path(&session.cwd)),
            ]),
            Line::from(vec![
                Span::styled("Current cwd  ", Style::default().fg(Color::DarkGray)),
                Span::raw(short_path(&self.current_cwd.to_string_lossy())),
            ]),
            Line::raw(""),
        ];

        for (idx, option) in dialog.options.iter().enumerate() {
            let selected = idx == dialog.selected_option;
            let marker = if selected { "› " } else { "  " };
            let checkbox = if option.enabled { "[x] " } else { "[ ] " };
            let style = if selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::from(vec![
                Span::styled(marker, style),
                Span::styled(checkbox, style),
                Span::styled(option.label.clone(), style),
            ]));
        }

        lines.push(Line::raw(""));
        lines.push(Line::styled(
            "Space toggles options. Enter launches with selected options.",
            Style::default().fg(Color::DarkGray),
        ));

        let paragraph = Paragraph::new(Text::from(lines))
            .block(
                Block::default()
                    .title(" Launch options ")
                    .borders(Borders::ALL),
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(paragraph, area);
    }
}
