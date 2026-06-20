use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};

use coca_core::model::{ProviderKind, Session};

use super::app::LaunchDialog;
use crate::formatting::{format_time, short_path};

pub(super) fn launch_dialog_height(dialog: &LaunchDialog) -> u16 {
    (dialog.options.len() as u16 + 9).max(10)
}

pub(super) fn session_lines(
    session: &Session,
    expanded: bool,
    selected: bool,
) -> Vec<Line<'static>> {
    let provider_style = match session.provider {
        ProviderKind::Codex => Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
        ProviderKind::Claude => Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD),
    };
    let updated = format_time(session.updated_at_ms.or(session.created_at_ms));
    let cwd = short_path(&session.cwd);
    let model = session.model.as_deref().unwrap_or("");
    let selected_style = if selected {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let mut lines = vec![
        Line::from(vec![
            if selected {
                Span::styled("› ", selected_style)
            } else {
                Span::raw("  ")
            },
            Span::styled(format!("{:<6}", session.provider), provider_style),
            Span::raw(" "),
            Span::styled(
                format!("{:<10}", session.origin),
                if selected {
                    selected_style
                } else {
                    Style::default().fg(Color::DarkGray)
                },
            ),
            Span::raw(" "),
            Span::styled(
                updated,
                if selected {
                    selected_style
                } else {
                    Style::default().fg(Color::Gray)
                },
            ),
            Span::raw(" "),
            Span::styled(
                cwd,
                if selected {
                    selected_style
                } else {
                    Style::default().fg(Color::Blue)
                },
            ),
        ]),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                session.title.clone(),
                if selected {
                    selected_style
                } else {
                    Style::default().fg(Color::White)
                },
            ),
        ]),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                model.to_string(),
                if selected {
                    selected_style
                } else {
                    Style::default().fg(Color::DarkGray)
                },
            ),
        ]),
    ];

    if expanded {
        lines.push(Line::raw(""));
        lines.extend(detail_lines(session));
    }

    lines
}

pub(super) fn detail_lines(session: &Session) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    if let Some(prompt) = session.first_user_message.as_deref() {
        if !prompt.trim().is_empty() {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    "First prompt",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            for line in prompt.lines() {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled("│ ", Style::default().fg(Color::DarkGray)),
                    Span::styled(line.to_string(), Style::default().fg(Color::Gray)),
                ]));
            }
            lines.push(Line::raw(""));
        }
    }

    lines.extend([
        kv("Origin", &session.origin.to_string()),
        kv("Provider", &session.provider.to_string()),
        kv("ID", &session.id),
        kv("CWD", &session.cwd),
        kv("Updated", &format_time(session.updated_at_ms)),
        kv("Created", &format_time(session.created_at_ms)),
        kv("Model", session.model.as_deref().unwrap_or("-")),
        kv("Source", &session.source_path.to_string_lossy()),
        kv(
            "Resume",
            &format!(
                "{} {}",
                session.resume_program,
                session.resume_args.join(" ")
            ),
        ),
    ]);

    lines
}

pub(super) fn transcript_text(session: &Session) -> Text<'static> {
    let mut lines = vec![
        Line::from(vec![Span::styled(
            session.title.clone(),
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::White),
        )]),
        Line::raw(""),
    ];

    if session.transcript.is_empty() {
        lines.push(Line::raw("-"));
    } else {
        for message in &session.transcript {
            lines.push(Line::raw(""));
            let role_style = match message.role.as_str() {
                "user" => Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
                "assistant" => Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
                "preview" => Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
                _ => Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            };
            lines.push(Line::from(vec![
                Span::styled(format!("{} ", message.role), role_style),
                Span::styled(
                    format_time(message.timestamp_ms),
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                ),
            ]));
            lines.push(Line::raw(message.text.clone()));
        }
    }

    Text::from(lines)
}

fn kv(key: &'static str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(format!("{key:<8} "), Style::default().fg(Color::DarkGray)),
        Span::raw(value.to_string()),
    ])
}

pub(super) fn centered_rect_fixed_height(percent_x: u16, height: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

pub(super) fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}
