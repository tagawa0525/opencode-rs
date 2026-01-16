//! Main UI layout and rendering.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    Frame,
};

use super::app::App;
use super::components::{Header, InputBox, MessageWidget, Spinner, StatusBar};

/// Main UI rendering function
pub fn render(frame: &mut Frame, app: &App) {
    let theme = &app.theme;
    let size = frame.area();

    // Main layout: Header, Messages, Input, Status
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Header
            Constraint::Min(10),   // Messages
            Constraint::Length(5), // Input
            Constraint::Length(1), // Status bar
        ])
        .split(size);

    // Render header
    let header = Header {
        title: &app.session_title,
        model: &app.model_display,
        status: &app.status,
        theme,
    };
    frame.render_widget(header, chunks[0]);

    // Render messages
    render_messages(frame, app, chunks[1]);

    // Render input
    let input = InputBox {
        content: &app.input,
        cursor_position: app.cursor_position,
        placeholder: "Type a message... (Enter to send, Shift+Enter for newline)",
        focused: true,
        theme,
    };
    frame.render_widget(input, chunks[2]);

    // Render status bar
    let left = format!("Session: {}", app.session_slug);
    let center = if app.is_processing {
        "Processing...".to_string()
    } else {
        "Ready".to_string()
    };
    let right = format!(
        "Cost: ${:.4} | Tokens: {}",
        app.total_cost, app.total_tokens
    );

    let status = StatusBar {
        left: &left,
        center: &center,
        right: &right,
        theme,
    };
    frame.render_widget(status, chunks[3]);

    // Show spinner if processing
    if app.is_processing {
        let spinner_area = Rect::new(
            chunks[1].x + 1,
            chunks[1].y + chunks[1].height - 2,
            chunks[1].width - 2,
            1,
        );
        let spinner = Spinner {
            message: "Thinking...",
            frame: app.spinner_frame,
            theme,
        };
        frame.render_widget(spinner, spinner_area);
    }
}

/// Render messages area
fn render_messages(frame: &mut Frame, app: &App, area: Rect) {
    use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(app.theme.border(false))
        .title(" Chat ");

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.messages.is_empty() {
        // Show welcome message
        let welcome = format!(
            "Welcome to opencode!\n\n\
             Model: {}\n\n\
             Tips:\n\
             • Type your message and press Enter to send\n\
             • Use Shift+Enter for multi-line input\n\
             • Press Ctrl+C to quit",
            app.model_display
        );
        let paragraph = Paragraph::new(welcome)
            .style(app.theme.text_dim())
            .wrap(Wrap { trim: false });
        frame.render_widget(paragraph, inner);
        return;
    }

    // Calculate total height needed
    let max_y = inner.height as usize;

    // Render messages from bottom to top (newest first visible)
    let messages_to_show: Vec<_> = app
        .messages
        .iter()
        .rev()
        .take(max_y) // Take at most max_y messages
        .collect();

    let mut current_y = inner.y;
    for msg in messages_to_show.iter().rev() {
        let timestamp = chrono::DateTime::from_timestamp_millis(msg.time_created)
            .map(|t| t.format("%H:%M").to_string())
            .unwrap_or_default();

        // Calculate height for this message
        let content_lines = msg.content.lines().count() + 2; // +2 for header and spacing
        let msg_height = content_lines.min(10) as u16;

        if current_y + msg_height > inner.y + inner.height {
            break;
        }

        let msg_area = Rect::new(inner.x, current_y, inner.width, msg_height);

        let widget = MessageWidget {
            role: &msg.role,
            content: &msg.content,
            timestamp: &timestamp,
            theme: &app.theme,
            selected: false,
        };
        frame.render_widget(widget, msg_area);

        current_y += msg_height;
    }
}
