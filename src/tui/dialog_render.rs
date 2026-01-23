//! Dialog rendering functions for the TUI.
//!
//! This module contains the rendering logic for various dialog types,
//! extracted from ui.rs for better organization and maintainability.

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};

use super::theme::Theme;
use super::types::{DialogState, DialogType};

// ============================================================================
// Helper Functions
// ============================================================================

/// Calculate centered dialog area
fn calculate_dialog_area(area: Rect) -> Rect {
    let width = area.width.clamp(40, 60);
    let height = area.height.clamp(10, 20);
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}

/// Render a help text at the bottom of a dialog
fn render_help_text(frame: &mut Frame, theme: &Theme, area: Rect, text: &str) {
    let help = Paragraph::new(text)
        .style(theme.text_dim())
        .alignment(Alignment::Center);
    frame.render_widget(help, area);
}

/// Render an optional message
fn render_message(frame: &mut Frame, theme: &Theme, area: Rect, message: Option<&str>, wrap: bool) {
    if let Some(msg) = message {
        let mut paragraph = Paragraph::new(msg).style(theme.text());
        if wrap {
            paragraph = paragraph.wrap(Wrap { trim: true });
        }
        frame.render_widget(paragraph, area);
    }
}

/// Create a bordered input block with title
fn create_input_block(theme: &Theme, title: &str) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .title(format!(" {} ", title))
}

/// Create selection style based on whether item is selected
fn selection_style(theme: &Theme, is_selected: bool) -> Style {
    if is_selected {
        Style::default()
            .fg(theme.background)
            .bg(theme.accent)
            .add_modifier(Modifier::BOLD)
    } else {
        theme.text()
    }
}

/// Standard input dialog layout constraints
fn input_dialog_constraints() -> [Constraint; 5] {
    [
        Constraint::Length(2), // Message
        Constraint::Length(1), // Spacer
        Constraint::Length(3), // Input
        Constraint::Min(1),    // Spacer
        Constraint::Length(1), // Help
    ]
}

// ============================================================================
// Main Dialog Renderer
// ============================================================================

/// Render a dialog overlay
pub fn render_dialog(frame: &mut Frame, dialog: &DialogState, theme: &Theme, area: Rect) {
    let dialog_area = calculate_dialog_area(area);

    frame.render_widget(Clear, dialog_area);

    let block = Block::default()
        .title(format!(" {} ", dialog.title))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .style(Style::default().bg(theme.background));

    let inner = block.inner(dialog_area);
    frame.render_widget(block, dialog_area);

    match dialog.dialog_type {
        DialogType::ModelSelector
        | DialogType::ProviderSelector
        | DialogType::AuthMethodSelector
        | DialogType::SessionList
        | DialogType::Timeline
        | DialogType::AgentSelector => render_select_dialog(frame, dialog, theme, inner),
        DialogType::ApiKeyInput => render_input_dialog(frame, dialog, theme, inner, true),
        DialogType::SessionRename => render_input_dialog(frame, dialog, theme, inner, false),
        DialogType::OAuthDeviceCode => render_device_code_dialog(frame, dialog, theme, inner),
        DialogType::OAuthWaiting => render_waiting_dialog(frame, dialog, theme, inner),
        DialogType::PermissionRequest => render_permission_dialog(frame, dialog, theme, inner),
        DialogType::Question => render_question_dialog(frame, dialog, theme, inner),
    }
}

// ============================================================================
// Dialog Type Renderers
// ============================================================================

/// Render a selection dialog (model, provider, session selector, etc.)
fn render_select_dialog(frame: &mut Frame, dialog: &DialogState, theme: &Theme, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Message
            Constraint::Length(1), // Search with count
            Constraint::Length(1), // Divider
            Constraint::Min(3),    // List
            Constraint::Length(1), // Help
        ])
        .split(area);

    // Message (dimmed for selector dialogs)
    if let Some(message) = &dialog.message {
        let msg = Paragraph::new(message.as_str()).style(theme.text_dim());
        frame.render_widget(msg, chunks[0]);
    }

    // Search input with match count
    let count_text = format!(" {}/{}", dialog.filtered_indices.len(), dialog.items.len());
    let search_spans = if dialog.search_query.is_empty() {
        vec![
            Span::styled("> ", theme.text_accent()),
            Span::styled("Type to search...", theme.text_dim()),
        ]
    } else {
        vec![
            Span::styled("> ", theme.text_accent()),
            Span::styled(&dialog.search_query, theme.text()),
            Span::styled(count_text, theme.text_dim()),
        ]
    };
    frame.render_widget(Paragraph::new(Line::from(search_spans)), chunks[1]);

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

            let content = match &item.description {
                Some(desc) => format!("  {} - {}", item.label, desc),
                None => format!("  {}", item.label),
            };

            ListItem::new(content).style(selection_style(theme, is_selected))
        })
        .collect();

    if items.is_empty() {
        let empty = Paragraph::new("No matches")
            .style(theme.text_dim())
            .alignment(Alignment::Center);
        frame.render_widget(empty, chunks[3]);
    } else {
        frame.render_widget(List::new(items), chunks[3]);
    }

    render_help_text(
        frame,
        theme,
        chunks[4],
        "Up/Down: Navigate | Enter: Select | Esc: Cancel",
    );
}

/// Render an input dialog (API key input or session rename)
fn render_input_dialog(
    frame: &mut Frame,
    dialog: &DialogState,
    theme: &Theme,
    area: Rect,
    mask_input: bool,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(input_dialog_constraints())
        .split(area);

    render_message(frame, theme, chunks[0], dialog.message.as_deref(), true);

    // Input field with appropriate title
    let (title, placeholder, help_text) = if mask_input {
        ("API Key", "Enter API key...", "Enter: Save | Esc: Back")
    } else {
        (
            "Session Name",
            "Enter session name...",
            "Enter: Save | Esc: Cancel",
        )
    };

    let input_block = create_input_block(theme, title);
    let inner_input = input_block.inner(chunks[2]);
    frame.render_widget(input_block, chunks[2]);

    let display_text = if dialog.input_value.is_empty() {
        Span::styled(placeholder, theme.text_dim())
    } else if mask_input {
        Span::styled("*".repeat(dialog.input_value.len().min(40)), theme.text())
    } else {
        Span::styled(&dialog.input_value, theme.text())
    };
    frame.render_widget(Paragraph::new(display_text), inner_input);

    render_help_text(frame, theme, chunks[4], help_text);
}

/// Render OAuth device code dialog
fn render_device_code_dialog(frame: &mut Frame, dialog: &DialogState, theme: &Theme, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // Title/Message
            Constraint::Length(1), // Spacer
            Constraint::Length(1), // URL label
            Constraint::Length(1), // URL
            Constraint::Length(1), // Spacer
            Constraint::Length(1), // Code label
            Constraint::Length(2), // Code
            Constraint::Min(1),    // Spacer
            Constraint::Length(1), // Help
        ])
        .split(area);

    let msg = Paragraph::new("Open your browser and enter the code:")
        .style(theme.text())
        .alignment(Alignment::Center);
    frame.render_widget(msg, chunks[0]);

    if let Some(uri) = &dialog.verification_uri {
        let url_label = Paragraph::new("Go to:")
            .style(theme.text_dim())
            .alignment(Alignment::Center);
        frame.render_widget(url_label, chunks[2]);

        let url = Paragraph::new(uri.as_str())
            .style(theme.text_accent().add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center);
        frame.render_widget(url, chunks[3]);
    }

    if let Some(code) = &dialog.user_code {
        let code_label = Paragraph::new("Enter code:")
            .style(theme.text_dim())
            .alignment(Alignment::Center);
        frame.render_widget(code_label, chunks[5]);

        let code_display = Paragraph::new(code.as_str())
            .style(theme.text().add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center);
        frame.render_widget(code_display, chunks[6]);
    }

    render_help_text(
        frame,
        theme,
        chunks[8],
        "Waiting for authorization... (Esc to cancel)",
    );
}

/// Render waiting dialog
fn render_waiting_dialog(frame: &mut Frame, dialog: &DialogState, theme: &Theme, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),    // Spacer
            Constraint::Length(2), // Message
            Constraint::Min(1),    // Spacer
            Constraint::Length(1), // Help
        ])
        .split(area);

    let message = dialog.message.as_deref().unwrap_or("Processing...");
    let msg = Paragraph::new(message)
        .style(theme.text())
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
    frame.render_widget(msg, chunks[1]);

    render_help_text(frame, theme, chunks[3], "Esc: Cancel");
}

/// Render permission request dialog
fn render_permission_dialog(frame: &mut Frame, dialog: &DialogState, theme: &Theme, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // Title
            Constraint::Length(1), // Spacer
            Constraint::Length(2), // Tool name
            Constraint::Length(1), // Spacer
            Constraint::Min(3),    // Arguments/Description
            Constraint::Length(1), // Spacer
            Constraint::Length(3), // Options
            Constraint::Length(1), // Help
        ])
        .split(area);

    // Title
    let title = Paragraph::new("Permission Required")
        .style(
            Style::default()
                .fg(theme.warning)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center);
    frame.render_widget(title, chunks[0]);

    let Some(req) = &dialog.permission_request else {
        return;
    };

    // Tool name
    let tool = Paragraph::new(format!("Permission: {}", req.permission))
        .style(theme.text_accent())
        .alignment(Alignment::Left);
    frame.render_widget(tool, chunks[2]);

    // Patterns and metadata
    let metadata_text =
        serde_json::to_string_pretty(&req.metadata).unwrap_or_else(|_| "{}".to_string());
    let truncated_metadata = if metadata_text.len() > 300 {
        format!("{}...", &metadata_text[..300])
    } else {
        metadata_text
    };

    let details = Paragraph::new(format!(
        "Patterns: {}\n\nMetadata:\n{}",
        req.patterns.join(", "),
        truncated_metadata
    ))
    .style(theme.text())
    .wrap(Wrap { trim: true });
    frame.render_widget(details, chunks[4]);

    // Permission options
    let selected = dialog.selected_permission_option;
    let options_config: [(usize, &str, &str, ratatui::style::Color); 5] = [
        (0, "[Y]", " Once    ", theme.success),
        (1, "[S]", " Session    ", theme.accent),
        (2, "[W]", " Workspace    ", theme.accent),
        (3, "[G]", " Global    ", theme.accent),
        (4, "[N]", " Reject", theme.error),
    ];

    let option_spans: Vec<Span> = options_config
        .iter()
        .flat_map(|(idx, key, label, color)| {
            let is_selected = selected == *idx;
            let key_style = if is_selected {
                Style::default()
                    .fg(theme.background)
                    .bg(*color)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(*color).add_modifier(Modifier::BOLD)
            };
            let label_style = if is_selected {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            [
                Span::styled(*key, key_style),
                Span::styled(*label, label_style),
            ]
        })
        .collect();

    let options_widget = Paragraph::new(vec![Line::from(option_spans)])
        .alignment(Alignment::Center)
        .style(theme.text());
    frame.render_widget(options_widget, chunks[6]);

    render_help_text(
        frame,
        theme,
        chunks[7],
        "Left/Right: Navigate | Enter: Confirm | Y/S/W/G/N: Direct select | Esc: Cancel",
    );
}

/// Render question dialog
fn render_question_dialog(frame: &mut Frame, dialog: &DialogState, theme: &Theme, area: Rect) {
    let Some(request) = &dialog.question_request else {
        return;
    };

    let question_count = request.questions.len();
    let current_idx = dialog.current_question_index;
    let current_question = &request.questions[current_idx];

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // Question tabs
            Constraint::Length(1), // Spacer
            Constraint::Length(2), // Question text
            Constraint::Length(1), // Spacer
            Constraint::Min(5),    // Options
            Constraint::Length(3), // Custom input
            Constraint::Length(1), // Help
        ])
        .split(area);

    // Question tabs (if multiple questions)
    if question_count > 1 {
        let tab_spans: Vec<Span> = request
            .questions
            .iter()
            .enumerate()
            .flat_map(|(idx, q)| {
                let is_current = idx == current_idx;
                let has_answer = !dialog.question_answers[idx].is_empty();
                let indicator = if has_answer { " ✓" } else { "" };
                let tab_text = format!(" {}{} ", q.header, indicator);

                let style = if is_current {
                    selection_style(theme, true)
                } else if has_answer {
                    Style::default().fg(theme.success)
                } else {
                    theme.text_dim()
                };

                let mut spans = vec![Span::styled(tab_text, style)];
                if idx + 1 < question_count {
                    spans.push(Span::raw(" "));
                }
                spans
            })
            .collect();

        let tabs = Paragraph::new(Line::from(tab_spans)).alignment(Alignment::Center);
        frame.render_widget(tabs, chunks[0]);
    }

    // Question text
    let question_text = if current_question.multiple {
        format!("{} (select all that apply)", current_question.question)
    } else {
        current_question.question.clone()
    };

    let question_widget = Paragraph::new(question_text)
        .style(theme.text().add_modifier(Modifier::BOLD))
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: true });
    frame.render_widget(question_widget, chunks[2]);

    // Options list
    let option_items: Vec<ListItem> = current_question
        .options
        .iter()
        .enumerate()
        .map(|(idx, option)| {
            let is_selected = dialog.current_option_index == idx;
            let is_answered = dialog.question_answers[current_idx]
                .iter()
                .any(|a| a == &option.label);

            let checkbox = match (current_question.multiple, is_answered) {
                (true, true) => "[✓] ",
                (true, false) => "[ ] ",
                (false, true) => "(•) ",
                (false, false) => "( ) ",
            };

            let content = format!("{}. {}{}", idx + 1, checkbox, option.label);
            let style = if is_selected {
                selection_style(theme, true)
            } else if is_answered {
                Style::default().fg(theme.success)
            } else {
                theme.text()
            };

            let mut lines = vec![Line::from(Span::styled(content, style))];
            if !option.description.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!("    {}", option.description),
                    theme.text_dim(),
                )));
            }

            ListItem::new(lines)
        })
        .collect();

    // Custom answer option
    let mut all_items = option_items;
    if current_question.custom {
        let custom_idx = current_question.options.len();
        let is_selected = dialog.current_option_index == custom_idx;
        let content = format!("{}. Type your own answer", custom_idx + 1);
        all_items.push(ListItem::new(Line::from(Span::styled(
            content,
            selection_style(theme, is_selected),
        ))));
    }

    frame.render_widget(List::new(all_items), chunks[4]);

    // Custom answer input (if editing)
    if dialog.is_editing_custom {
        let input_block = create_input_block(theme, "Custom Answer");
        let inner_input = input_block.inner(chunks[5]);
        frame.render_widget(input_block, chunks[5]);

        let display_text = if dialog.custom_answer_input.is_empty() {
            Span::styled("Type your answer...", theme.text_dim())
        } else {
            Span::styled(&dialog.custom_answer_input, theme.text())
        };
        frame.render_widget(Paragraph::new(display_text), inner_input);
    }

    // Help text
    let help_text = if dialog.is_editing_custom {
        "Enter: Confirm | Esc: Cancel"
    } else if question_count > 1 {
        "Up/Down: Navigate | Enter/Space: Select | Tab: Next | S: Submit | Esc: Cancel"
    } else {
        "Up/Down: Navigate | Enter/Space: Select | S: Submit | Esc: Cancel"
    };

    render_help_text(frame, theme, chunks[6], help_text);
}
