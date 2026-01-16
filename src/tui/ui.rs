//! Main UI layout and rendering.

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};

use super::app::{App, DialogState, DialogType};
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

    // Render dialog if open
    if let Some(dialog) = &app.dialog {
        render_dialog(frame, dialog, theme, size);
    }
}

/// Render a dialog overlay
fn render_dialog(frame: &mut Frame, dialog: &DialogState, theme: &super::theme::Theme, area: Rect) {
    // Calculate dialog size
    let width = area.width.min(60).max(40);
    let height = area.height.min(20).max(10);
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;

    let dialog_area = Rect::new(x, y, width, height);

    // Clear the area behind the dialog
    frame.render_widget(Clear, dialog_area);

    // Draw dialog border
    let block = Block::default()
        .title(format!(" {} ", dialog.title))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .style(Style::default().bg(theme.background));

    let inner = block.inner(dialog_area);
    frame.render_widget(block, dialog_area);

    match dialog.dialog_type {
        DialogType::ModelSelector | DialogType::ProviderSelector => {
            render_select_dialog(frame, dialog, theme, inner);
        }
        DialogType::ApiKeyInput => {
            render_input_dialog(frame, dialog, theme, inner);
        }
        DialogType::None => {}
    }
}

/// Render a selection dialog (model or provider selector)
fn render_select_dialog(
    frame: &mut Frame,
    dialog: &DialogState,
    theme: &super::theme::Theme,
    area: Rect,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Message
            Constraint::Length(1), // Search
            Constraint::Length(1), // Divider
            Constraint::Min(3),    // List
            Constraint::Length(1), // Help
        ])
        .split(area);

    // Message
    if let Some(message) = &dialog.message {
        let msg = Paragraph::new(message.as_str())
            .style(Style::default().fg(theme.dim));
        frame.render_widget(msg, chunks[0]);
    }

    // Search input
    let search_text = if dialog.search_query.is_empty() {
        Span::styled("Type to search...", Style::default().fg(theme.dim))
    } else {
        Span::styled(&dialog.search_query, Style::default().fg(theme.foreground))
    };
    let search = Paragraph::new(Line::from(vec![
        Span::styled("> ", Style::default().fg(theme.accent)),
        search_text,
    ]));
    frame.render_widget(search, chunks[1]);

    // List items
    let visible_count = chunks[3].height as usize;
    let start_index = dialog.selected_index.saturating_sub(visible_count / 2);

    let items: Vec<ListItem> = dialog
        .filtered_indices
        .iter()
        .skip(start_index)
        .take(visible_count)
        .enumerate()
        .map(|(i, &item_idx)| {
            let item = &dialog.items[item_idx];
            let is_selected = start_index + i == dialog.selected_index;

            let style = if is_selected {
                Style::default()
                    .fg(theme.background)
                    .bg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.foreground)
            };

            let content = if let Some(desc) = &item.description {
                format!("{} - {}", item.label, desc)
            } else {
                item.label.clone()
            };

            ListItem::new(content).style(style)
        })
        .collect();

    if items.is_empty() {
        let empty = Paragraph::new("No items found")
            .style(Style::default().fg(theme.dim))
            .alignment(Alignment::Center);
        frame.render_widget(empty, chunks[3]);
    } else {
        let list = List::new(items);
        frame.render_widget(list, chunks[3]);
    }

    // Help text
    let help = Paragraph::new("Enter: Select | Esc: Cancel | Type to search")
        .style(Style::default().fg(theme.dim))
        .alignment(Alignment::Center);
    frame.render_widget(help, chunks[4]);
}

/// Render an input dialog (API key input)
fn render_input_dialog(
    frame: &mut Frame,
    dialog: &DialogState,
    theme: &super::theme::Theme,
    area: Rect,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // Message
            Constraint::Length(1), // Spacer
            Constraint::Length(3), // Input
            Constraint::Min(1),    // Spacer
            Constraint::Length(1), // Help
        ])
        .split(area);

    // Message
    if let Some(message) = &dialog.message {
        let msg = Paragraph::new(message.as_str())
            .style(Style::default().fg(theme.foreground))
            .wrap(Wrap { trim: true });
        frame.render_widget(msg, chunks[0]);
    }

    // Input field
    let input_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .title(" API Key ");

    let inner_input = input_block.inner(chunks[2]);
    frame.render_widget(input_block, chunks[2]);

    // Mask the API key with asterisks
    let display_text = if dialog.input_value.is_empty() {
        Span::styled("Enter API key...", Style::default().fg(theme.dim))
    } else {
        let masked = "*".repeat(dialog.input_value.len().min(40));
        Span::styled(masked, Style::default().fg(theme.foreground))
    };
    let input = Paragraph::new(display_text);
    frame.render_widget(input, inner_input);

    // Help text
    let help = Paragraph::new("Enter: Save | Esc: Back")
        .style(Style::default().fg(theme.dim))
        .alignment(Alignment::Center);
    frame.render_widget(help, chunks[4]);
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
