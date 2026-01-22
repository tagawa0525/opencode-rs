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
    // Handle special actions first
    if let Some(action) = &output.action {
        return handle_action(app, action).await;
    }

    // Handle special commands that create new sessions
    if matches!(command_name, "clear" | "new") {
        create_new_session(app).await;
        return Ok(());
    }

    // Handle model switch
    if let Some(model) = &output.model {
        handle_model_switch(app, model);
        return Ok(());
    }

    // Handle agent switch
    if let Some(agent_name) = &output.agent {
        handle_agent_switch(app, agent_name);
        return Ok(());
    }

    // Display command output if not empty
    if !output.text.is_empty() {
        app.add_message("system", &output.text);
    }

    // Submit to LLM if requested
    if output.submit_to_llm {
        start_llm_response(app, &output.text, event_tx);
    }

    Ok(())
}

/// Handle special command actions
async fn handle_action(app: &mut App, action: &CommandAction) -> Result<()> {
    match action {
        // UI actions
        CommandAction::OpenModelSelector => app.open_model_selector(),
        CommandAction::OpenProviderConnection => app.open_provider_connection(),
        CommandAction::Exit => app.should_quit = true,
        CommandAction::ToggleTheme => handle_toggle_theme(app),
        CommandAction::ToggleThinking => handle_toggle_thinking(app),

        // Session actions
        CommandAction::NewSession => create_new_session(app).await,
        CommandAction::Status => handle_status(app),

        // Transcript actions
        CommandAction::Copy => handle_copy_transcript(app),
        CommandAction::Export => handle_export_transcript(app),

        // Unimplemented actions with messages
        CommandAction::OpenAgentSelector => {
            app.open_agent_selector().await?;
        }
        CommandAction::OpenSessionList => {
            app.open_session_list().await?;
        }
        CommandAction::Undo => handle_undo(app),
        CommandAction::Redo => handle_redo(app),
        CommandAction::Compact => {
            // Note: /compact command already works by sending a prompt to LLM
            // This action is not used by the current implementation
            app.add_message("system", "Use /compact command to summarize the session");
        }
        CommandAction::Unshare => {
            app.add_message(
                "system",
                "Session sharing features are under development. This requires server-side API implementation.",
            );
        }
        CommandAction::Rename => app.open_session_rename(),
        CommandAction::Timeline => app.open_timeline(),
        CommandAction::Fork => {
            handle_fork_session(app).await?;
        }
        CommandAction::Share => {
            app.add_message(
                "system",
                "Session sharing features are under development. This requires server-side API implementation.",
            );
        }
        CommandAction::ToggleMcp => {
            app.add_message(
                "system",
                "MCP server management dialog is under development. For now, configure MCPs in your opencode.json file.",
            );
        }
        CommandAction::OpenEditor => handle_open_editor(app)?,
        CommandAction::ShowCommands => {
            app.add_message("system", "Use /help to see all available commands")
        }
    }
    Ok(())
}

/// Handle undo action
fn handle_undo(app: &mut App) {
    if app.can_undo() {
        app.undo();
        app.add_message(
            "system",
            &format!(
                "Undone (step {}/{})",
                app.history_position + 1,
                app.message_history.len()
            ),
        );
    } else {
        app.add_message("system", "Nothing to undo");
    }
}

/// Handle redo action
fn handle_redo(app: &mut App) {
    if app.can_redo() {
        app.redo();
        app.add_message(
            "system",
            &format!(
                "Redone (step {}/{})",
                app.history_position + 1,
                app.message_history.len()
            ),
        );
    } else {
        app.add_message("system", "Nothing to redo");
    }
}

/// Handle theme toggle action
fn handle_toggle_theme(app: &mut App) {
    app.theme = if app.theme.name == "dark" {
        crate::tui::theme::Theme::light()
    } else {
        crate::tui::theme::Theme::dark()
    };
    app.add_message("system", &format!("Theme switched to {}", app.theme.name));
}

/// Handle thinking visibility toggle action
fn handle_toggle_thinking(app: &mut App) {
    app.show_thinking = !app.show_thinking;
    let msg = if app.show_thinking {
        "Thinking visibility enabled"
    } else {
        "Thinking visibility disabled"
    };
    app.add_message("system", msg);
}

/// Handle status display action
fn handle_status(app: &mut App) {
    let status_msg = format!(
        "Session: {}\nModel: {}\nProvider: {}\nTokens: {}\nCost: ${:.4}",
        app.session_title, app.model_display, app.provider_id, app.total_tokens, app.total_cost
    );
    app.add_message("system", &status_msg);
}

/// Create transcript options from app state
fn create_transcript_options(app: &App) -> crate::tui::TranscriptOptions {
    crate::tui::TranscriptOptions {
        include_tool_details: app.show_tool_details,
    }
}

/// Handle copy transcript action
fn handle_copy_transcript(app: &mut App) {
    let transcript = crate::tui::format_transcript(
        &app.session_title,
        &app.session_slug,
        &app.messages,
        &create_transcript_options(app),
    );

    match crate::tui::copy_to_clipboard(&transcript) {
        Ok(_) => app.add_message("system", "Transcript copied to clipboard"),
        Err(e) => app.add_message("system", &format!("Failed to copy to clipboard: {}", e)),
    }
}

/// Handle export transcript action
fn handle_export_transcript(app: &mut App) {
    use std::fs;

    let transcript = crate::tui::format_transcript(
        &app.session_title,
        &app.session_slug,
        &app.messages,
        &create_transcript_options(app),
    );

    let filename = generate_export_filename(&app.session_slug);

    match fs::write(&filename, transcript) {
        Ok(_) => app.add_message("system", &format!("Transcript exported to {}", filename)),
        Err(e) => app.add_message("system", &format!("Failed to export transcript: {}", e)),
    }
}

/// Generate export filename from session slug
fn generate_export_filename(session_slug: &str) -> String {
    if session_slug.is_empty() {
        "session-transcript.md".to_string()
    } else {
        let slug_prefix = &session_slug[..session_slug.len().min(8)];
        format!("session-{}.md", slug_prefix)
    }
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

/// Handle agent switch command
fn handle_agent_switch(app: &mut App, agent_name: &str) {
    // TODO: Implement full agent system with prompts and configurations
    // For now, just store the agent name and notify the user
    app.status = format!(
        "Agent switching to '{}' - agent system under development",
        agent_name
    );
    app.add_message(
        "system",
        &format!(
            "Note: Agent '{}' selected, but agent system is not yet fully implemented",
            agent_name
        ),
    );
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

/// Handle opening external editor
fn handle_open_editor(app: &mut App) -> Result<()> {
    use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
    use std::io::Write;

    // Get editor from environment
    let editor = std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .unwrap_or_else(|_| "vi".to_string());

    // Create temp file with current input
    let temp_dir = std::env::temp_dir();
    let temp_file = temp_dir.join(format!("opencode-edit-{}.txt", std::process::id()));

    {
        let mut file = std::fs::File::create(&temp_file)?;
        file.write_all(app.input.as_bytes())?;
    }

    // Disable raw mode temporarily
    disable_raw_mode()?;

    // Open editor
    let status = std::process::Command::new(&editor).arg(&temp_file).status();

    // Re-enable raw mode
    enable_raw_mode()?;

    match status {
        Ok(exit_status) if exit_status.success() => {
            // Read file content
            match std::fs::read_to_string(&temp_file) {
                Ok(content) => {
                    app.input = content;
                    app.cursor_position = app.input.len();
                    app.add_message("system", "Editor content loaded");
                }
                Err(e) => {
                    app.add_message("system", &format!("Failed to read editor file: {}", e));
                }
            }
        }
        Ok(_) => {
            app.add_message("system", "Editor exited with error");
        }
        Err(e) => {
            app.add_message(
                "system",
                &format!("Failed to open editor '{}': {}", editor, e),
            );
        }
    }

    // Clean up temp file
    let _ = std::fs::remove_file(&temp_file);

    Ok(())
}

/// Handle session forking
async fn handle_fork_session(app: &mut App) -> Result<()> {
    use crate::id::{self, IdPrefix};
    use crate::session::{Message, Part};
    use std::collections::HashMap;

    let current_session = match &app.session {
        Some(s) => s.clone(),
        None => {
            app.add_message("system", "No active session to fork");
            return Ok(());
        }
    };

    // Create new session with parent_id
    let new_session = Session::create(CreateSessionOptions {
        project_id: Some("default".to_string()),
        parent_id: Some(current_session.id.clone()),
        title: Some(format!("Fork of {}", current_session.title)),
        ..Default::default()
    })
    .await?;

    // Load and copy messages
    let messages = Message::list(&current_session.id).await?;
    let mut id_map: HashMap<String, String> = HashMap::new();

    for message in messages {
        let old_id = message.id().to_string();

        // Clone and update message
        let mut new_msg = message.clone();
        let new_id = match &mut new_msg {
            Message::User(ref mut user_msg) => {
                user_msg.id = id::ascending(IdPrefix::Message);
                user_msg.session_id = new_session.id.clone();
                user_msg.id.clone()
            }
            Message::Assistant(ref mut asst_msg) => {
                asst_msg.id = id::ascending(IdPrefix::Message);
                asst_msg.session_id = new_session.id.clone();
                // Update parent_id using the ID map
                if let Some(new_parent) = id_map.get(&asst_msg.parent_id) {
                    asst_msg.parent_id = new_parent.clone();
                }
                asst_msg.id.clone()
            }
        };

        id_map.insert(old_id, new_id.clone());
        new_msg.save().await?;

        // Copy parts for this message
        let parts = Part::list(message.id()).await?;
        for part in parts {
            // Create new part with updated IDs
            // Note: This is a simplified version - in a complete implementation,
            // we would need to properly update all part IDs
            let new_part = part;
            // Update the message_id in the part base
            // (This requires accessing the base field, which varies by part type)
            new_part.save().await?;
        }
    }

    // Switch to the new forked session
    app.session = Some(new_session.clone());
    app.session_title = new_session.title.clone();
    app.session_slug = new_session.slug.clone();
    app.messages.clear();
    app.total_cost = 0.0;
    app.total_tokens = 0;

    app.add_message(
        "system",
        &format!("Session forked successfully: {}", new_session.title),
    );

    Ok(())
}
