//! Main UI layout and rendering.

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};

use super::app::{App, AutocompleteState};
use super::components::{Header, InputBox, MessageWidget, Spinner, StatusBar};
use super::dialog_render::render_dialog;

/// Main UI rendering function
pub fn render(frame: &mut Frame, app: &App) {
    let theme = &app.theme;
    let size = frame.area();

    // Calculate input height based on content (min 2, max 10 lines)
    let input_lines = app.input.lines().count().max(1);
    let input_height = (input_lines as u16 + 1).clamp(2, 10);

    // Main layout: Header, Messages, Input, Status
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),           // Header
            Constraint::Min(10),             // Messages
            Constraint::Length(input_height), // Input (dynamic)
            Constraint::Length(1),           // Status bar
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
        is_processing: app.is_processing,
        spinner_frame: app.spinner_frame,
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

    // Render dialog if open
    if let Some(dialog) = &app.dialog {
        render_dialog(frame, dialog, &app.theme, size);
    }

    // Render autocomplete if open
    if let Some(autocomplete) = &app.autocomplete {
        render_autocomplete(frame, autocomplete, theme, chunks[2]);
    }
}

/// Render messages area
fn render_messages(frame: &mut Frame, app: &App, area: Rect) {
    use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

    let inner = area;

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
    let messages_vec: Vec<_> = messages_to_show.iter().rev().collect();

    for msg in messages_vec.iter() {
        // Calculate height for this message (+1 for spacing after message)
        let content_lines = msg.content.lines().count();
        let msg_height = (content_lines + 1).max(2).min(11) as u16;

        if current_y + msg_height > inner.y + inner.height {
            break;
        }

        let msg_area = Rect::new(inner.x, current_y, inner.width, msg_height);

        let widget = MessageWidget {
            role: &msg.role,
            content: &msg.content,
            timestamp: "",
            theme: &app.theme,
            selected: false,
        };
        frame.render_widget(widget, msg_area);

        current_y += msg_height;
    }

    // Show processing indicator after all messages
    if app.is_processing && current_y < inner.y + inner.height {
        let spinner_area = Rect::new(inner.x, current_y, inner.width, 1);
        let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        let spinner_char = frames[app.spinner_frame % frames.len()];
        let spinner_text = format!(" {} Thinking...", spinner_char);
        let spinner_para = Paragraph::new(spinner_text)
            .style(app.theme.text_accent().add_modifier(Modifier::BOLD));
        frame.render_widget(spinner_para, spinner_area);
    }
}

/// Render autocomplete popup
fn render_autocomplete(
    frame: &mut Frame,
    autocomplete: &AutocompleteState,
    theme: &super::theme::Theme,
    input_area: Rect,
) {
    if autocomplete.items.is_empty() {
        return;
    }

    // Calculate autocomplete position (above the input box)
    let max_items = autocomplete.items.len().min(10);
    let height = (max_items as u16 + 3).min(13); // +3 for borders and search info
    let width = input_area.width.min(60);
    let x = input_area.x;
    let y = input_area.y.saturating_sub(height);

    let autocomplete_area = Rect::new(x, y, width, height);

    // Clear the area
    frame.render_widget(Clear, autocomplete_area);

    // Draw border
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .style(Style::default().bg(theme.background));

    let inner = block.inner(autocomplete_area);
    frame.render_widget(block, autocomplete_area);

    // Split for search info and items
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Search info
            Constraint::Min(1),    // Items
        ])
        .split(inner);

    // Search info with match count (fzf style)
    let match_count = autocomplete.items.len();
    let search_info = if autocomplete.filter.is_empty() {
        format!("> {} commands", match_count)
    } else {
        format!("> /{} - {} matches", autocomplete.filter, match_count)
    };
    let search_para = Paragraph::new(search_info).style(Style::default().fg(theme.dim));
    frame.render_widget(search_para, chunks[0]);

    // Render items
    let items: Vec<ListItem> = autocomplete
        .items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let is_selected = i == autocomplete.selected_index;

            let style = if is_selected {
                Style::default()
                    .fg(theme.background)
                    .bg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.foreground)
            };

            // Format: display - description (fzf style)
            let content = format!("  {:<20} {}", item.display, item.description);
            ListItem::new(content).style(style)
        })
        .collect();

    let list = List::new(items);
    frame.render_widget(list, chunks[1]);
}
