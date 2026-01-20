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
use super::types::DisplayMessage;

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

    // First, calculate which messages need separators
    let messages_vec: Vec<_> = app.messages.iter().collect();
    let mut need_separators: Vec<bool> = Vec::new();

    for i in 0..messages_vec.len() {
        // Check if separator is needed after this message (between this and next message chronologically)
        let need_sep = if i + 1 < messages_vec.len() {
            messages_vec[i].role != messages_vec[i + 1].role
        } else {
            false // No separator after last message
        };
        need_separators.push(need_sep);
    }

    // Render messages from newest to oldest, calculating heights
    let mut messages_with_heights: Vec<(String, String, u16, bool)> = Vec::new();
    let mut total_height = 0u16;

    for (idx, msg) in messages_vec.iter().rev().enumerate() {
        let msg_index = messages_vec.len() - 1 - idx;

        let content_lines = msg.content
            .trim()
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count();
        let msg_height = content_lines.max(1) as u16;

        let need_separator = need_separators[msg_index];
        let separator_height = if need_separator { 1 } else { 0 };
        let item_total_height = msg_height + separator_height;

        // Stop if we exceed available height
        if total_height + item_total_height > inner.height {
            break;
        }

        total_height += item_total_height;
        messages_with_heights.push((msg.content.clone(), msg.role.clone(), msg_height, need_separator));
    }

    // Reverse to render from oldest to newest (top to bottom)
    messages_with_heights.reverse();

    let mut current_y = inner.y;
    for (content, role, msg_height, need_separator) in messages_with_heights.iter() {
        let msg_area = Rect::new(inner.x, current_y, inner.width, *msg_height);

        let widget = MessageWidget {
            role: role,
            content: content,
            timestamp: "",
            theme: &app.theme,
            selected: false,
        };
        frame.render_widget(widget, msg_area);

        current_y += msg_height;

        if *need_separator && current_y < inner.y + inner.height {
            let separator_area = Rect::new(inner.x, current_y, inner.width, 1);
            let separator_line = "─".repeat(inner.width as usize);
            let separator_para = Paragraph::new(separator_line)
                .style(app.theme.text_dim());
            frame.render_widget(separator_para, separator_area);
            current_y += 1;
        }
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
