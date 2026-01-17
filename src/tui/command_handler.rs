//! Command output handling for the TUI.
//!
//! This module handles the output from slash commands and special actions.

use anyhow::Result;
use tokio::sync::mpsc;

use super::state::App;
use super::types::AppEvent;
use crate::provider::{self, StreamEvent};
use crate::session::{CreateSessionOptions, Session};
use crate::slash_command::{CommandAction, CommandOutput};

use super::llm_streaming::stream_response;

/// Handle command output
pub async fn handle_command_output(
    app: &mut App,
    command_name: &str,
    output: CommandOutput,
    event_tx: mpsc::Sender<AppEvent>,
) -> Result<()> {
    // Handle special actions
    if let Some(action) = &output.action {
        match action {
            CommandAction::OpenModelSelector => {
                app.open_model_selector();
                return Ok(());
            }
            CommandAction::OpenAgentSelector => {
                // TODO: Implement agent selector
                app.add_message("system", "Agent selector not yet implemented");
                return Ok(());
            }
            CommandAction::OpenSessionList => {
                // TODO: Implement session list
                app.add_message("system", "Session list not yet implemented");
                return Ok(());
            }
            CommandAction::NewSession => {
                // Create new session
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
                return Ok(());
            }
        }
    }

    // Handle special commands
    if command_name == "clear" || command_name == "new" {
        // Create new session
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
        return Ok(());
    }

    // Handle model switch
    if let Some(model) = &output.model {
        if let Some((provider_id, model_id)) = provider::parse_model_string(model) {
            app.provider_id = provider_id.clone();
            app.model_id = model_id.clone();
            app.model_display = format!("{}/{}", provider_id, model_id);
            app.model_configured = true;
            app.status = format!("Switched to model: {}", model);
        }
        return Ok(());
    }

    // Handle agent switch
    if let Some(_agent) = &output.agent {
        // TODO: Implement agent switching
        app.status = "Agent switching not yet implemented".to_string();
        return Ok(());
    }

    // Display command output if not empty
    if !output.text.is_empty() {
        app.add_message("system", &output.text);
    }

    // If the command wants to submit to LLM, do it
    if output.submit_to_llm {
        app.is_processing = true;
        app.status = "Processing".to_string();

        // Add empty assistant message
        app.add_message("assistant", "");

        // Start streaming
        let provider_id = app.provider_id.clone();
        let model_id = app.model_id.clone();
        let prompt = output.text.clone();

        tokio::spawn(async move {
            match stream_response(&provider_id, &model_id, &prompt).await {
                Ok(mut rx) => {
                    while let Some(event) = rx.recv().await {
                        match event {
                            StreamEvent::TextDelta(text) => {
                                let _ = event_tx.send(AppEvent::StreamDelta(text)).await;
                            }
                            StreamEvent::Done { .. } => {
                                let _ = event_tx.send(AppEvent::StreamDone).await;
                            }
                            StreamEvent::Error(err) => {
                                let _ = event_tx.send(AppEvent::StreamError(err)).await;
                            }
                            StreamEvent::ToolCallStart { name, .. } => {
                                let _ =
                                    event_tx.send(AppEvent::ToolCall(name, String::new())).await;
                            }
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    let _ = event_tx.send(AppEvent::StreamError(e.to_string())).await;
                }
            }
        });
    }

    Ok(())
}
