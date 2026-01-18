//! Command output handling for the TUI.
//!
//! This module handles the output from slash commands and special actions.

use anyhow::Result;
use tokio::sync::mpsc;

use super::llm_streaming::stream_response;
use super::state::App;
use super::types::AppEvent;
use crate::provider::{self, StreamEvent};
use crate::session::{CreateSessionOptions, Session};
use crate::slash_command::{CommandAction, CommandOutput};

/// Handle command output
pub async fn handle_command_output(
    app: &mut App,
    command_name: &str,
    output: CommandOutput,
    event_tx: mpsc::Sender<AppEvent>,
) -> Result<()> {
    // Handle special actions
    if let Some(action) = &output.action {
        return handle_action(app, action).await;
    }

    // Handle special commands
    if command_name == "clear" || command_name == "new" {
        create_new_session(app).await;
        return Ok(());
    }

    // Handle model switch
    if let Some(model) = &output.model {
        handle_model_switch(app, model);
        return Ok(());
    }

    // Handle agent switch
    if output.agent.is_some() {
        app.status = "Agent switching not yet implemented".to_string();
        return Ok(());
    }

    // Display command output if not empty
    if !output.text.is_empty() {
        app.add_message("system", &output.text);
    }

    // If the command wants to submit to LLM, do it
    if output.submit_to_llm {
        start_llm_response(app, &output.text, event_tx);
    }

    Ok(())
}

/// Handle special command actions
async fn handle_action(app: &mut App, action: &CommandAction) -> Result<()> {
    match action {
        CommandAction::OpenModelSelector => {
            app.open_model_selector();
        }
        CommandAction::OpenAgentSelector => {
            app.add_message("system", "Agent selector not yet implemented");
        }
        CommandAction::OpenSessionList => {
            app.add_message("system", "Session list not yet implemented");
        }
        CommandAction::NewSession => {
            create_new_session(app).await;
        }
        CommandAction::Exit => {
            app.should_quit = true;
        }
        CommandAction::OpenProviderConnection => {
            app.open_provider_connection();
        }
        CommandAction::Undo => {
            // TODO: Implement message history and revert
            app.add_message(
                "system",
                "Undo not yet fully implemented - message history needed",
            );
        }
        CommandAction::Redo => {
            // TODO: Implement message history and unrevert
            app.add_message(
                "system",
                "Redo not yet fully implemented - message history needed",
            );
        }
        CommandAction::Compact => {
            // Compact is handled as an LLM prompt in the command itself
            app.add_message("system", "Session compaction not yet implemented");
        }
        CommandAction::Unshare => {
            app.add_message("system", "Session sharing not yet implemented");
        }
        CommandAction::Rename => {
            // TODO: Open a rename dialog
            app.add_message("system", "Session rename dialog not yet implemented");
        }
        CommandAction::Copy => {
            use crate::tui::{format_transcript, TranscriptOptions};

            let options = TranscriptOptions {
                include_thinking: app.show_thinking,
                include_tool_details: app.show_tool_details,
                include_metadata: app.show_assistant_metadata,
            };

            let transcript = format_transcript(
                &app.session_title,
                &app.session_slug,
                &app.messages,
                &options,
            );

            match crate::tui::copy_to_clipboard(&transcript) {
                Ok(_) => {
                    app.add_message("system", "Transcript copied to clipboard");
                }
                Err(e) => {
                    app.add_message("system", &format!("Failed to copy to clipboard: {}", e));
                }
            }
        }
        CommandAction::Export => {
            use crate::tui::{format_transcript, TranscriptOptions};
            use std::fs;

            let options = TranscriptOptions {
                include_thinking: app.show_thinking,
                include_tool_details: app.show_tool_details,
                include_metadata: app.show_assistant_metadata,
            };

            let transcript = format_transcript(
                &app.session_title,
                &app.session_slug,
                &app.messages,
                &options,
            );

            // Generate filename from session slug
            let filename = if app.session_slug.is_empty() {
                "session-transcript.md".to_string()
            } else {
                format!(
                    "session-{}.md",
                    &app.session_slug[..app.session_slug.len().min(8)]
                )
            };

            match fs::write(&filename, transcript) {
                Ok(_) => {
                    app.add_message("system", &format!("Transcript exported to {}", filename));
                }
                Err(e) => {
                    app.add_message("system", &format!("Failed to export transcript: {}", e));
                }
            }
        }
        CommandAction::Timeline => {
            // TODO: Open timeline dialog showing all user messages
            app.add_message("system", "Timeline dialog not yet implemented");
        }
        CommandAction::Fork => {
            // TODO: Open fork dialog and create new session
            app.add_message("system", "Session forking not yet implemented");
        }
        CommandAction::ToggleThinking => {
            app.show_thinking = !app.show_thinking;
            let msg = if app.show_thinking {
                "Thinking visibility enabled"
            } else {
                "Thinking visibility disabled"
            };
            app.add_message("system", msg);
        }
        CommandAction::Share => {
            // TODO: Generate share URL and copy to clipboard
            app.add_message("system", "Session sharing not yet implemented");
        }
        CommandAction::Status => {
            let status_msg = format!(
                "Session: {}\nModel: {}\nProvider: {}\nTokens: {}\nCost: ${:.4}",
                app.session_title,
                app.model_display,
                app.provider_id,
                app.total_tokens,
                app.total_cost
            );
            app.add_message("system", &status_msg);
        }
        CommandAction::ToggleMcp => {
            // TODO: Open MCP dialog to enable/disable MCP servers
            app.add_message("system", "MCP toggle dialog not yet implemented");
        }
        CommandAction::ToggleTheme => {
            app.theme = if app.theme.name == "dark" {
                crate::tui::theme::Theme::light()
            } else {
                crate::tui::theme::Theme::dark()
            };
            app.add_message("system", &format!("Theme switched to {}", app.theme.name));
        }
        CommandAction::OpenEditor => {
            // TODO: Open external editor with current prompt
            app.add_message("system", "External editor not yet implemented");
        }
        CommandAction::ShowCommands => {
            // Show help instead
            app.add_message("system", "Use /help to see all available commands");
        }
    }
    Ok(())
}

/// Create a new session
async fn create_new_session(app: &mut App) {
    match Session::create(CreateSessionOptions::default()).await {
        Ok(session) => {
            app.session_title = session.title.clone();
            app.session_slug = session.slug.clone();
            app.session = Some(session);
            app.messages.clear();
            app.total_cost = 0.0;
            app.total_tokens = 0;
            app.status = "Session cleared".to_string();
        }
        Err(e) => {
            app.status = format!("Error creating session: {}", e);
        }
    }
}

/// Handle model switch command
fn handle_model_switch(app: &mut App, model: &str) {
    if let Some((provider_id, model_id)) = provider::parse_model_string(model) {
        app.provider_id = provider_id.clone();
        app.model_id = model_id.clone();
        app.model_display = format!("{}/{}", provider_id, model_id);
        app.model_configured = true;
        app.status = format!("Switched to model: {}", model);
    }
}

/// Start streaming LLM response
fn start_llm_response(app: &mut App, prompt: &str, event_tx: mpsc::Sender<AppEvent>) {
    app.is_processing = true;
    app.status = "Processing".to_string();

    // Add empty assistant message
    app.add_message("assistant", "");

    // Start streaming
    let provider_id = app.provider_id.clone();
    let model_id = app.model_id.clone();
    let prompt = prompt.to_string();

    tokio::spawn(async move {
        match stream_response(&provider_id, &model_id, &prompt).await {
            Ok(rx) => process_stream_events(rx, event_tx).await,
            Err(e) => {
                let _ = event_tx.send(AppEvent::StreamError(e.to_string())).await;
            }
        }
    });
}

/// Process stream events from LLM
async fn process_stream_events(
    mut rx: mpsc::Receiver<StreamEvent>,
    event_tx: mpsc::Sender<AppEvent>,
) {
    while let Some(event) = rx.recv().await {
        let app_event = match event {
            StreamEvent::TextDelta(text) => Some(AppEvent::StreamDelta(text)),
            StreamEvent::Done { .. } => Some(AppEvent::StreamDone),
            StreamEvent::Error(err) => Some(AppEvent::StreamError(err)),
            StreamEvent::ToolCallStart { name, .. } => {
                Some(AppEvent::ToolCall(name, String::new()))
            }
            _ => None,
        };

        if let Some(evt) = app_event {
            let _ = event_tx.send(evt).await;
        }
    }
}
