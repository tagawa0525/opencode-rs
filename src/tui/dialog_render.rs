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

/// Calculate centered dialog area
fn calculate_dialog_area(area: Rect) -> Rect {
    let width = area.width.clamp(40, 60);
    let height = area.height.clamp(10, 20);
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}

/// Render a dialog overlay
pub fn render_dialog(frame: &mut Frame, dialog: &DialogState, theme: &Theme, area: Rect) {
    let dialog_area = calculate_dialog_area(area);

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
        DialogType::ModelSelector
        | DialogType::ProviderSelector
        | DialogType::AuthMethodSelector
        | DialogType::SessionList
        | DialogType::Timeline
        | DialogType::AgentSelector => {
            render_select_dialog(frame, dialog, theme, inner);
        }
        DialogType::ApiKeyInput => {
            render_input_dialog(frame, dialog, theme, inner);
        }
        DialogType::SessionRename => {
            render_rename_dialog(frame, dialog, theme, inner);
        }
        DialogType::OAuthDeviceCode => {
            render_device_code_dialog(frame, dialog, theme, inner);
        }
        DialogType::OAuthWaiting => {
            render_waiting_dialog(frame, dialog, theme, inner);
        }
        DialogType::PermissionRequest => {
            render_permission_dialog(frame, dialog, theme, inner);
        }
        DialogType::Question => {
            render_question_dialog(frame, dialog, theme, inner);
        }
        DialogType::None => {}
    }
}

/// Render a selection dialog (model or provider selector)
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

    // Message
    if let Some(message) = &dialog.message {
        let msg = Paragraph::new(message.as_str()).style(Style::default().fg(theme.dim));
        frame.render_widget(msg, chunks[0]);
    }

    // Search input with match count (fzf style)
    let match_count = dialog.filtered_indices.len();
    let total_count = dialog.items.len();
    let count_text = format!(" {}/{}", match_count, total_count);

    let search_text = if dialog.search_query.is_empty() {
        vec![
            Span::styled("> ", Style::default().fg(theme.accent)),
            Span::styled("Type to search...", Style::default().fg(theme.dim)),
        ]
    } else {
        vec![
            Span::styled("> ", Style::default().fg(theme.accent)),
            Span::styled(&dialog.search_query, Style::default().fg(theme.foreground)),
            Span::styled(count_text, Style::default().fg(theme.dim)),
        ]
    };
    let search = Paragraph::new(Line::from(search_text));
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

            // Format: label - description (fzf style)
            let content = if let Some(desc) = &item.description {
                format!("  {} - {}", item.label, desc)
            } else {
                format!("  {}", item.label)
            };

            ListItem::new(content).style(style)
        })
        .collect();

    if items.is_empty() {
        let empty = Paragraph::new("No matches")
            .style(Style::default().fg(theme.dim))
            .alignment(Alignment::Center);
        frame.render_widget(empty, chunks[3]);
    } else {
        let list = List::new(items);
        frame.render_widget(list, chunks[3]);
    }

    // Help text (fzf style)
    let help = Paragraph::new("Up/Down: Navigate | Enter: Select | Esc: Cancel")
        .style(Style::default().fg(theme.dim))
        .alignment(Alignment::Center);
    frame.render_widget(help, chunks[4]);
}

/// Render session rename dialog
fn render_rename_dialog(frame: &mut Frame, dialog: &DialogState, theme: &Theme, area: Rect) {
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
        .title(" Session Name ");

    let inner_input = input_block.inner(chunks[2]);
    frame.render_widget(input_block, chunks[2]);

    // Display the current input value
    let display_text = if dialog.input_value.is_empty() {
        Span::styled("Enter session name...", Style::default().fg(theme.dim))
    } else {
        Span::styled(&dialog.input_value, Style::default().fg(theme.foreground))
    };
    let input = Paragraph::new(display_text);
    frame.render_widget(input, inner_input);

    // Help text
    let help = Paragraph::new("Enter: Save | Esc: Cancel")
        .style(Style::default().fg(theme.dim))
        .alignment(Alignment::Center);
    frame.render_widget(help, chunks[4]);
}

/// Render an input dialog (API key input)
fn render_input_dialog(frame: &mut Frame, dialog: &DialogState, theme: &Theme, area: Rect) {
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
            Constraint::Length(2), // Code (large)
            Constraint::Min(1),    // Spacer
            Constraint::Length(1), // Help
        ])
        .split(area);

    // Message
    let msg = Paragraph::new("Open your browser and enter the code:")
        .style(Style::default().fg(theme.foreground))
        .alignment(Alignment::Center);
    frame.render_widget(msg, chunks[0]);

    // URL
    if let Some(uri) = &dialog.verification_uri {
        let url_label = Paragraph::new("Go to:")
            .style(Style::default().fg(theme.dim))
            .alignment(Alignment::Center);
        frame.render_widget(url_label, chunks[2]);

        let url = Paragraph::new(uri.as_str())
            .style(
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center);
        frame.render_widget(url, chunks[3]);
    }

    // User code
    if let Some(code) = &dialog.user_code {
        let code_label = Paragraph::new("Enter code:")
            .style(Style::default().fg(theme.dim))
            .alignment(Alignment::Center);
        frame.render_widget(code_label, chunks[5]);

        let code_display = Paragraph::new(code.as_str())
            .style(
                Style::default()
                    .fg(theme.foreground)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center);
        frame.render_widget(code_display, chunks[6]);
    }

    // Help text
    let help = Paragraph::new("Waiting for authorization... (Esc to cancel)")
        .style(Style::default().fg(theme.dim))
        .alignment(Alignment::Center);
    frame.render_widget(help, chunks[8]);
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

    // Message
    let message = dialog.message.as_deref().unwrap_or("Processing...");
    let msg = Paragraph::new(message)
        .style(Style::default().fg(theme.foreground))
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
    frame.render_widget(msg, chunks[1]);

    // Help text
    let help = Paragraph::new("Esc: Cancel")
        .style(Style::default().fg(theme.dim))
        .alignment(Alignment::Center);
    frame.render_widget(help, chunks[3]);
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
    let title_text = "Permission Required";
    let title = Paragraph::new(title_text)
        .style(
            Style::default()
                .fg(theme.warning)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center);
    frame.render_widget(title, chunks[0]);

    // Tool name
    if let Some(req) = &dialog.permission_request {
        let tool_label = format!("Permission: {}", req.permission);
        let tool = Paragraph::new(tool_label)
            .style(Style::default().fg(theme.accent))
            .alignment(Alignment::Left);
        frame.render_widget(tool, chunks[2]);

        // Patterns and metadata
        let patterns_text = req.patterns.join(", ");
        let metadata_text =
            serde_json::to_string_pretty(&req.metadata).unwrap_or_else(|_| "{}".to_string());

        let details_text = format!(
            "Patterns: {}\n\nMetadata:\n{}",
            patterns_text,
            if metadata_text.len() > 300 {
                format!("{}...", &metadata_text[..300])
            } else {
                metadata_text
            }
        );

        let details = Paragraph::new(details_text)
            .style(Style::default().fg(theme.foreground))
            .wrap(Wrap { trim: true });
        frame.render_widget(details, chunks[4]);

        // Options with selection highlighting
        // 0=Once, 1=Session, 2=Workspace, 3=Global, 4=Reject
        let selected = dialog.selected_permission_option;

        let mut option_spans = vec![];

        // Once option
        option_spans.push(Span::styled(
            "[Y]",
            if selected == 0 {
                Style::default()
                    .fg(theme.background)
                    .bg(theme.success)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(theme.success)
                    .add_modifier(Modifier::BOLD)
            },
        ));
        option_spans.push(Span::styled(
            " Once    ",
            if selected == 0 {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            },
        ));

        // Session option
        option_spans.push(Span::styled(
            "[S]",
            if selected == 1 {
                Style::default()
                    .fg(theme.background)
                    .bg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            },
        ));
        option_spans.push(Span::styled(
            " Session    ",
            if selected == 1 {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            },
        ));

        // Workspace option
        option_spans.push(Span::styled(
            "[W]",
            if selected == 2 {
                Style::default()
                    .fg(theme.background)
                    .bg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            },
        ));
        option_spans.push(Span::styled(
            " Workspace    ",
            if selected == 2 {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            },
        ));

        // Global option
        option_spans.push(Span::styled(
            "[G]",
            if selected == 3 {
                Style::default()
                    .fg(theme.background)
                    .bg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            },
        ));
        option_spans.push(Span::styled(
            " Global    ",
            if selected == 3 {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            },
        ));

        // Reject option
        option_spans.push(Span::styled(
            "[N]",
            if selected == 4 {
                Style::default()
                    .fg(theme.background)
                    .bg(theme.error)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(theme.error)
                    .add_modifier(Modifier::BOLD)
            },
        ));
        option_spans.push(Span::styled(
            " Reject",
            if selected == 4 {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            },
        ));

        let options = vec![Line::from(option_spans)];
        let options_widget = Paragraph::new(options)
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme.foreground));
        frame.render_widget(options_widget, chunks[6]);
    }

    // Help text
    let help = Paragraph::new(
        "Left/Right: Navigate | Enter: Confirm | Y/S/W/G/N: Direct select | Esc: Cancel",
    )
    .style(Style::default().fg(theme.dim))
    .alignment(Alignment::Center);
    frame.render_widget(help, chunks[7]);
}

/// Render question dialog
fn render_question_dialog(frame: &mut Frame, dialog: &DialogState, theme: &Theme, area: Rect) {
    let request = match &dialog.question_request {
        Some(req) => req,
        None => return,
    };

    let question_count = request.questions.len();
    let current_idx = dialog.current_question_index;
    let current_question = &request.questions[current_idx];

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // Question tabs (if multi-question)
            Constraint::Length(1), // Spacer
            Constraint::Length(2), // Question text
            Constraint::Length(1), // Spacer
            Constraint::Min(5),    // Options
            Constraint::Length(3), // Custom input (if editing)
            Constraint::Length(1), // Help
        ])
        .split(area);

    // Question tabs (if multiple questions)
    if question_count > 1 {
        let mut tab_spans = vec![];
        for (idx, q) in request.questions.iter().enumerate() {
            let is_current = idx == current_idx;
            let has_answer = !dialog.question_answers[idx].is_empty();

            let indicator = if has_answer { " ✓" } else { "" };
            let tab_text = format!(" {}{} ", q.header, indicator);

            let style = if is_current {
                Style::default()
                    .fg(theme.background)
                    .bg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else if has_answer {
                Style::default().fg(theme.success)
            } else {
                Style::default().fg(theme.dim)
            };

            tab_spans.push(Span::styled(tab_text, style));
            if idx + 1 < question_count {
                tab_spans.push(Span::raw(" "));
            }
        }

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
        .style(
            Style::default()
                .fg(theme.foreground)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: true });
    frame.render_widget(question_widget, chunks[2]);

    // Options list
    let mut option_items: Vec<ListItem> = vec![];
    for (idx, option) in current_question.options.iter().enumerate() {
        let is_selected = dialog.current_option_index == idx;
        let is_answered = dialog.question_answers[current_idx]
            .iter()
            .any(|a| a == &option.label);

        let checkbox = if current_question.multiple {
            if is_answered {
                "[✓] "
            } else {
                "[ ] "
            }
        } else if is_answered {
            "(•) "
        } else {
            "( ) "
        };

        let number = format!("{}. ", idx + 1);
        let content = format!("{}{}{}", number, checkbox, option.label);

        let style = if is_selected {
            Style::default()
                .fg(theme.background)
                .bg(theme.accent)
                .add_modifier(Modifier::BOLD)
        } else if is_answered {
            Style::default().fg(theme.success)
        } else {
            Style::default().fg(theme.foreground)
        };

        let mut lines = vec![Line::from(Span::styled(content, style))];

        // Add description
        if !option.description.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("    {}", option.description),
                Style::default().fg(theme.dim),
            )));
        }

        option_items.push(ListItem::new(lines));
    }

    // Custom answer option
    if current_question.custom {
        let custom_idx = current_question.options.len();
        let is_selected = dialog.current_option_index == custom_idx;
        let content = format!("{}. Type your own answer", custom_idx + 1);

        let style = if is_selected {
            Style::default()
                .fg(theme.background)
                .bg(theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.foreground)
        };

        option_items.push(ListItem::new(Line::from(Span::styled(content, style))));
    }

    let options_list = List::new(option_items);
    frame.render_widget(options_list, chunks[4]);

    // Custom answer input (if editing)
    if dialog.is_editing_custom {
        let input_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent))
            .title(" Custom Answer ");

        let inner_input = input_block.inner(chunks[5]);
        frame.render_widget(input_block, chunks[5]);

        let display_text = if dialog.custom_answer_input.is_empty() {
            Span::styled("Type your answer...", Style::default().fg(theme.dim))
        } else {
            Span::styled(
                &dialog.custom_answer_input,
                Style::default().fg(theme.foreground),
            )
        };

        let input = Paragraph::new(display_text);
        frame.render_widget(input, inner_input);
    }

    // Help text
    let help_text = if dialog.is_editing_custom {
        "Enter: Confirm | Esc: Cancel"
    } else if question_count > 1 {
        "Up/Down: Navigate | Enter/Space: Select | Tab: Next | S: Submit | Esc: Cancel"
    } else {
        "Up/Down: Navigate | Enter/Space: Select | S: Submit | Esc: Cancel"
    };

    let help = Paragraph::new(help_text)
        .style(Style::default().fg(theme.dim))
        .alignment(Alignment::Center);
    frame.render_widget(help, chunks[6]);
}
