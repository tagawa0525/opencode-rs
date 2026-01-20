//! Main UI layout and rendering.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};

use super::app::{App, AutocompleteState};
use super::components::{Header, InputBox, MessageWidget, StatusBar, SPINNER_FRAMES};
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
            Constraint::Length(1),            // Header
            Constraint::Min(10),              // Messages
            Constraint::Length(input_height), // Input (dynamic)
            Constraint::Length(1),            // Status bar
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

/// Calculate visible line count for message content
fn calculate_message_height(content: &str) -> u16 {
    content
        .trim()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count()
        .max(1) as u16
}

/// Render messages area
fn render_messages(frame: &mut Frame, app: &App, area: Rect) {
    if app.messages.is_empty() {
        render_welcome_message(frame, app, area);
        return;
    }

    let messages = &app.messages;
    let mut visible_messages: Vec<(&str, &str, u16, bool)> = Vec::new();
    let mut total_height = 0u16;

    // Collect messages from newest to oldest until we fill the area
    for (idx, msg) in messages.iter().enumerate().rev() {
        let msg_height = calculate_message_height(&msg.content);
        let needs_separator = idx + 1 < messages.len() && msg.role != messages[idx + 1].role;
        let separator_height = if needs_separator { 1 } else { 0 };
        let item_height = msg_height + separator_height;

        if total_height + item_height > area.height {
            break;
        }

        total_height += item_height;
        visible_messages.push((&msg.content, &msg.role, msg_height, needs_separator));
    }

    // Render from oldest to newest (top to bottom)
    visible_messages.reverse();

    let mut current_y = area.y;
    for (content, role, msg_height, needs_separator) in visible_messages {
        let msg_area = Rect::new(area.x, current_y, area.width, msg_height);
        frame.render_widget(
            MessageWidget {
                role,
                content,
                timestamp: "",
                theme: &app.theme,
                selected: false,
            },
            msg_area,
        );
        current_y += msg_height;

        if needs_separator && current_y < area.y + area.height {
            let separator_area = Rect::new(area.x, current_y, area.width, 1);
            let separator =
                Paragraph::new("─".repeat(area.width as usize)).style(app.theme.text_dim());
            frame.render_widget(separator, separator_area);
            current_y += 1;
        }
    }

    // Show processing indicator
    if app.is_processing && current_y < area.y + area.height {
        let spinner_area = Rect::new(area.x, current_y, area.width, 1);
        let spinner_char = SPINNER_FRAMES[app.spinner_frame % SPINNER_FRAMES.len()];
        let spinner = Paragraph::new(format!(" {} Thinking...", spinner_char))
            .style(app.theme.text_accent().add_modifier(Modifier::BOLD));
        frame.render_widget(spinner, spinner_area);
    }
}

/// Render welcome message when no messages exist
fn render_welcome_message(frame: &mut Frame, app: &App, area: Rect) {
    let welcome = format!(
        "Welcome to opencode!\n\n\
         Model: {}\n\n\
         Tips:\n\
         • Type your message and press Enter to send\n\
         • Use Shift+Enter for multi-line input\n\
         • Press Ctrl+C to quit",
        app.model_display
    );
    frame.render_widget(
        Paragraph::new(welcome)
            .style(app.theme.text_dim())
            .wrap(Wrap { trim: false }),
        area,
    );
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
