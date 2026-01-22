//! Reusable TUI components.

use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Paragraph, Widget, Wrap},
};

use super::theme::Theme;

/// Spinner animation frames (braille pattern)
pub const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Header component showing session info and model
pub struct Header<'a> {
    pub title: &'a str,
    pub model: &'a str,
    pub status: &'a str,
    pub theme: &'a Theme,
}

impl<'a> Widget for Header<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(30),
                Constraint::Min(20),
                Constraint::Length(20),
            ])
            .split(area);

        // Title
        let title = Paragraph::new(self.title)
            .style(self.theme.text_accent())
            .alignment(Alignment::Left);
        title.render(chunks[0], buf);

        // Model
        let model = Paragraph::new(self.model)
            .style(self.theme.text_dim())
            .alignment(Alignment::Center);
        model.render(chunks[1], buf);

        // Status
        let status_style = match self.status {
            "Ready" => self.theme.text().fg(self.theme.success),
            "Processing" => self.theme.text().fg(self.theme.warning),
            "Error" => self.theme.text().fg(self.theme.error),
            _ => self.theme.text_dim(),
        };
        let status = Paragraph::new(self.status)
            .style(status_style)
            .alignment(Alignment::Right);
        status.render(chunks[2], buf);
    }
}

/// Message component for displaying a single message
pub struct MessageWidget<'a> {
    pub role: &'a str,
    pub content: &'a str,
    pub theme: &'a Theme,
}

impl<'a> Widget for MessageWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let bg_style = if self.role == "user" {
            Style::default().bg(self.theme.user_bg)
        } else {
            Style::default()
        };

        Block::default().style(bg_style).render(area, buf);

        let content_lines: Vec<Line> = self
            .content
            .trim()
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| Line::from(Span::styled(format!(" {} ", line), self.theme.text())))
            .collect();

        Paragraph::new(content_lines)
            .wrap(Wrap { trim: false })
            .style(bg_style)
            .render(area, buf);
    }
}

/// Input box component
pub struct InputBox<'a> {
    pub content: &'a str,
    pub placeholder: &'a str,
    pub theme: &'a Theme,
}

impl<'a> Widget for InputBox<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let bg_style = Style::default().bg(self.theme.user_bg);
        Block::default().style(bg_style).render(area, buf);

        // Always show user input, even when processing
        let display_text = if self.content.is_empty() {
            Span::styled(self.placeholder, self.theme.text_dim())
        } else {
            Span::styled(self.content, self.theme.text())
        };

        Paragraph::new(display_text)
            .wrap(Wrap { trim: false })
            .style(bg_style)
            .render(area, buf);
    }
}

/// Status bar component
pub struct StatusBar<'a> {
    pub left: &'a str,
    pub center: &'a str,
    pub right: &'a str,
    pub theme: &'a Theme,
}

impl<'a> Widget for StatusBar<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(33),
                Constraint::Percentage(34),
                Constraint::Percentage(33),
            ])
            .split(area);

        let left = Paragraph::new(self.left)
            .style(self.theme.text_dim())
            .alignment(Alignment::Left);
        left.render(chunks[0], buf);

        let center = Paragraph::new(self.center)
            .style(self.theme.text_dim())
            .alignment(Alignment::Center);
        center.render(chunks[1], buf);

        let right = Paragraph::new(self.right)
            .style(self.theme.text_dim())
            .alignment(Alignment::Right);
        right.render(chunks[2], buf);
    }
}
