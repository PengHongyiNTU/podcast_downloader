use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
};

use crate::core::EpisodeStatus;

pub(super) fn render_empty(frame: &mut ratatui::Frame, area: Rect, title: &str, message: &str) {
    let empty = Paragraph::new(message)
        .block(panel_block(title, false))
        .style(muted_style())
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
    frame.render_widget(empty, area);
}

pub(super) fn panel_block(title: &str, active: bool) -> Block<'_> {
    let style = if active {
        Style::default().fg(Color::Rgb(94, 234, 212))
    } else {
        muted_style()
    };
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(title.to_string())
        .border_style(style)
}

pub(super) fn shell_block() -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Rgb(51, 65, 85)))
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
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1]);
    horizontal[1]
}

pub(super) fn inset_horizontal(area: Rect, margin: u16) -> Rect {
    if area.width <= margin.saturating_mul(2) {
        area
    } else {
        Rect {
            x: area.x + margin,
            width: area.width - margin * 2,
            ..area
        }
    }
}

pub(super) fn short_date(value: Option<&str>) -> Option<&str> {
    value.map(|value| value.get(0..10).unwrap_or(value))
}

pub(super) fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let bytes = bytes as f64;
    if bytes >= GB {
        format!("{:.1} GB", bytes / GB)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes / MB)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes / KB)
    } else {
        format!("{bytes:.0} B")
    }
}

pub(super) fn status_label(status: EpisodeStatus) -> &'static str {
    match status {
        EpisodeStatus::Pending => "pending",
        EpisodeStatus::Downloaded => "downloaded",
        EpisodeStatus::SkippedInitial => "skipped",
        EpisodeStatus::Failed => "failed",
        EpisodeStatus::Deleted => "deleted",
    }
}

pub(super) fn status_style(status: EpisodeStatus) -> Style {
    match status {
        EpisodeStatus::Downloaded => Style::default().fg(Color::Rgb(134, 239, 172)),
        EpisodeStatus::Failed => Style::default().fg(Color::Rgb(248, 113, 113)),
        EpisodeStatus::Pending => Style::default().fg(Color::Rgb(125, 211, 252)),
        EpisodeStatus::SkippedInitial | EpisodeStatus::Deleted => muted_style(),
    }
}

pub(super) fn title_style() -> Style {
    Style::default()
        .fg(Color::Rgb(125, 211, 252))
        .add_modifier(Modifier::BOLD)
}

pub(super) fn text_style() -> Style {
    Style::default().fg(Color::White)
}

pub(super) fn input_style() -> Style {
    Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD)
}

pub(super) fn highlight_style() -> Style {
    Style::default()
        .fg(Color::Black)
        .bg(Color::Rgb(94, 234, 212))
        .add_modifier(Modifier::BOLD)
}

pub(super) fn nav_style(active: bool) -> Style {
    if active {
        Style::default()
            .fg(Color::Rgb(45, 212, 191))
            .add_modifier(Modifier::BOLD)
    } else {
        muted_style()
    }
}

pub(super) fn button_key_style() -> Style {
    Style::default()
        .fg(Color::Rgb(15, 23, 42))
        .bg(Color::Rgb(94, 234, 212))
        .add_modifier(Modifier::BOLD)
}

pub(super) fn button_label_style() -> Style {
    Style::default().fg(Color::Rgb(203, 213, 225))
}

pub(super) fn muted_style() -> Style {
    Style::default().fg(Color::Rgb(148, 163, 184))
}

pub(super) fn subdued_style() -> Style {
    Style::default().fg(Color::Rgb(100, 116, 139))
}

pub(super) fn success_style() -> Style {
    Style::default()
        .fg(Color::Rgb(134, 239, 172))
        .add_modifier(Modifier::BOLD)
}

pub(super) fn warning_style() -> Style {
    Style::default()
        .fg(Color::Rgb(251, 191, 36))
        .add_modifier(Modifier::BOLD)
}

pub(super) fn move_index(current: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    let last = len - 1;
    if delta.is_negative() {
        current.saturating_sub(delta.unsigned_abs()).min(last)
    } else {
        current.saturating_add(delta as usize).min(last)
    }
}

pub(super) fn is_quit_key(key: KeyEvent) -> bool {
    key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
}

pub(super) fn is_text_input_key(key: KeyEvent) -> bool {
    !(key.modifiers.contains(KeyModifiers::CONTROL)
        || key.modifiers.contains(KeyModifiers::ALT)
        || key.modifiers.contains(KeyModifiers::SUPER))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_index_stays_in_bounds() {
        assert_eq!(move_index(0, 0, 1), 0);
        assert_eq!(move_index(0, 3, -1), 0);
        assert_eq!(move_index(1, 3, 1), 2);
        assert_eq!(move_index(2, 3, 1), 2);
    }

    #[test]
    fn ctrl_c_is_a_quit_key() {
        let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(is_quit_key(key));
    }
}
