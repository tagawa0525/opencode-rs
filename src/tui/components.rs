//! Reusable TUI components.

use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap},
};

use super::theme::Theme;

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
    pub timestamp: &'a str,
    pub theme: &'a Theme,
    pub selected: bool,
}

impl<'a> Widget for MessageWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Set background color based on role
        let bg_style = match self.role {
            "user" => Style::default().bg(self.theme.user_bg),
            _ => Style::default(),
        };

        // Apply background to entire area
        let bg_block = Block::default().style(bg_style);
        bg_block.render(area, buf);

        // Render content
        let content_lines: Vec<Line> = self
            .content
            .lines()
            .map(|line| Line::from(Span::styled(format!(" {} ", line), self.theme.text())))
            .collect();

        let paragraph = Paragraph::new(content_lines).wrap(Wrap { trim: false }).style(bg_style);

        paragraph.render(area, buf);
    }
}

/// Input box component
pub struct InputBox<'a> {
    pub content: &'a str,
    pub cursor_position: usize,
    pub placeholder: &'a str,
    pub focused: bool,
    pub theme: &'a Theme,
}

impl<'a> Widget for InputBox<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Apply user background color to input area
        let bg_block = Block::default()
            .style(Style::default().bg(self.theme.user_bg));
        bg_block.render(area, buf);

        let display_text = if self.content.is_empty() {
            Span::styled(self.placeholder, self.theme.text_dim())
        } else {
            Span::styled(self.content, self.theme.text())
        };

        let paragraph = Paragraph::new(display_text)
            .wrap(Wrap { trim: false })
            .style(Style::default().bg(self.theme.user_bg));
        paragraph.render(area, buf);
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

/// Loading spinner component
pub struct Spinner<'a> {
    pub message: &'a str,
    pub frame: usize,
    pub theme: &'a Theme,
}

impl<'a> Widget for Spinner<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        let frame = frames[self.frame % frames.len()];

        let text = format!("{} {}", frame, self.message);
        let paragraph = Paragraph::new(text)
            .style(self.theme.text_accent())
            .alignment(Alignment::Left);
        paragraph.render(area, buf);
    }
}

/// Dialog component for overlays
pub struct Dialog<'a> {
    pub title: &'a str,
    pub content: &'a str,
    pub theme: &'a Theme,
}

impl<'a> Widget for Dialog<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Calculate centered position
        let width = area.width.min(60);
        let height = area.height.min(20);
        let x = (area.width.saturating_sub(width)) / 2;
        let y = (area.height.saturating_sub(height)) / 2;

        let dialog_area = Rect::new(x, y, width, height);

        // Clear the area
        Clear.render(dialog_area, buf);

        // Draw the dialog
        let block = Block::default()
            .title(format!(" {} ", self.title))
            .borders(Borders::ALL)
            .border_style(self.theme.border(true))
            .style(Style::default().bg(self.theme.background));

        let inner = block.inner(dialog_area);
        block.render(dialog_area, buf);

        let paragraph = Paragraph::new(self.content)
            .style(self.theme.text())
            .wrap(Wrap { trim: true });
        paragraph.render(inner, buf);
    }
}

/// Tool output component
pub struct ToolOutput<'a> {
    pub tool_name: &'a str,
    pub title: &'a str,
    pub output: &'a str,
    pub collapsed: bool,
    pub theme: &'a Theme,
}

impl<'a> Widget for ToolOutput<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let header = Line::from(vec![
            Span::styled(
                if self.collapsed { "▶ " } else { "▼ " },
                self.theme.text_dim(),
            ),
            Span::styled(format!("[{}] ", self.tool_name), self.theme.tool()),
            Span::styled(self.title, self.theme.text()),
        ]);

        if self.collapsed {
            let paragraph = Paragraph::new(header);
            paragraph.render(area, buf);
        } else {
            let output_lines: Vec<Line> = self
                .output
                .lines()
                .take(10) // Limit displayed lines
                .map(|line| Line::from(Span::styled(format!("  {}", line), self.theme.text_dim())))
                .collect();

            let mut lines = vec![header];
            lines.extend(output_lines);

            let paragraph = Paragraph::new(lines);
            paragraph.render(area, buf);
        }
    }
}
