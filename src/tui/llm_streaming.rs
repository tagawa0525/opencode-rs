//! LLM streaming handlers for the TUI.
//!
//! This module contains functions for streaming responses from LLM providers
//! with support for tool calling and agentic loops.

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::mpsc;

use super::types::{AppEvent, PermissionRequest};
use crate::config::Config;
use crate::permission::PermissionChecker;
use crate::provider::{
    self, ChatContent, ChatMessage, ContentPart, StreamEvent, StreamingClient, ToolDefinition,
};
use crate::tool::{self, DoomLoopDetector, ToolCallTracker};

/// Stream a response from the LLM with agentic loop
pub async fn stream_response_agentic(
    provider_id: String,
    model_id: String,
    initial_prompt: String,
    event_tx: mpsc::Sender<AppEvent>,
) -> Result<()> {
    let config = Config::load().await?;
    let permission_checker = PermissionChecker::from_config(&config);

    let provider = provider::registry()
        .get(&provider_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("Provider not found: {}", provider_id))?;

    let model = provider
        .models
        .get(&model_id)
        .ok_or_else(|| anyhow::anyhow!("Model not found: {}", model_id))?;

    let api_key = provider
        .key
        .ok_or_else(|| anyhow::anyhow!("No API key for provider: {}", provider_id))?;

    // Prepare tool definitions
    let tools = tool::registry().definitions().await;
    let tool_defs: Vec<ToolDefinition> = tools
        .into_iter()
        .map(|t| ToolDefinition {
            name: t.name,
            description: t.description,
            input_schema: t.parameters,
        })
        .collect();

    // Tool execution context
    let tool_ctx = Arc::new(tool::ToolContext {
        session_id: String::new(),
        message_id: String::new(),
        agent: "tui".to_string(),
        abort: None,
        cwd: std::env::current_dir()
            .ok()
            .and_then(|p| p.to_str().map(String::from))
            .unwrap_or_else(|| ".".to_string()),
        root: std::env::current_dir()
            .ok()
            .and_then(|p| p.to_str().map(String::from))
            .unwrap_or_else(|| ".".to_string()),
        extra: Default::default(),
    });

    // Initialize conversation with user message
    let mut messages = vec![ChatMessage {
        role: "user".to_string(),
        content: ChatContent::Text(initial_prompt),
    }];

    let client = StreamingClient::new();
    let mut step = 0;
    let max_steps = 10;
    let mut doom_detector = DoomLoopDetector::new();

    // Create channel for permission responses (for future use)
    let (_perm_tx, mut _perm_rx) = mpsc::channel::<(String, bool, bool)>(10);

    loop {
        step += 1;
        if step > max_steps {
            let _ = event_tx
                .send(AppEvent::StreamError(
                    "Maximum agentic loop steps reached".to_string(),
                ))
                .await;
            break;
        }

        // Stream the response
        let mut rx = match provider_id.as_str() {
            "anthropic" => {
                client
                    .stream_anthropic(
                        &api_key,
                        &model.api.id,
                        messages.clone(),
                        None,
                        tool_defs.clone(),
                        model.limit.output,
                    )
                    .await?
            }
            "openai" => {
                let base_url = model
                    .api
                    .url
                    .as_deref()
                    .unwrap_or("https://api.openai.com/v1");
                client
                    .stream_openai(
                        &api_key,
                        base_url,
                        &model.api.id,
                        messages.clone(),
                        tool_defs.clone(),
                        model.limit.output,
                    )
                    .await?
            }
            "copilot" => {
                client
                    .stream_copilot(
                        &api_key,
                        &model.api.id,
                        messages.clone(),
                        tool_defs.clone(),
                        model.limit.output,
                    )
                    .await?
            }
            _ => {
                return Err(anyhow::anyhow!("Unsupported provider: {}", provider_id));
            }
        };

        // Collect response
        let mut response_text = String::new();
        let mut tool_tracker = ToolCallTracker::new();
        let mut finish_reason = String::new();
        let mut assistant_parts: Vec<ContentPart> = Vec::new();

        while let Some(event) = rx.recv().await {
            match event {
                StreamEvent::TextDelta(text) => {
                    let _ = event_tx.send(AppEvent::StreamDelta(text.clone())).await;
                    response_text.push_str(&text);
                }
                StreamEvent::ToolCallStart { id, name } => {
                    let _ = event_tx
                        .send(AppEvent::ToolCall(name.clone(), id.clone()))
                        .await;
                    tool_tracker.start_call(id, name);
                }
                StreamEvent::ToolCallDelta {
                    id,
                    arguments_delta,
                } => {
                    tool_tracker.add_arguments(&id, &arguments_delta);
                }
                StreamEvent::Done {
                    finish_reason: reason,
                } => {
                    finish_reason = reason;
                }
                StreamEvent::Error(err) => {
                    let _ = event_tx.send(AppEvent::StreamError(err.clone())).await;
                    return Err(anyhow::anyhow!(err));
                }
                _ => {}
            }
        }

        // Add assistant response to conversation
        if !response_text.is_empty() {
            assistant_parts.push(ContentPart::Text {
                text: response_text.clone(),
            });
        }

        // Check if there are tool calls to execute
        let pending_calls = tool_tracker.get_all_calls();

        if !pending_calls.is_empty() {
            // Check for doom loop
            doom_detector.add_calls(&pending_calls);

            if let Some((tool_name, args)) = doom_detector.check_doom_loop() {
                // Request permission for doom loop continuation
                let req_id = format!("doom_loop_{}", step);
                let request = PermissionRequest {
                    id: req_id.clone(),
                    tool_name: "doom_loop".to_string(),
                    arguments: format!("Tool '{}' with args: {}", tool_name, args),
                    description: format!(
                        "Doom loop detected: '{}' called {} times with identical arguments",
                        tool_name,
                        tool::DOOM_LOOP_THRESHOLD
                    ),
                };

                let _ = event_tx.send(AppEvent::PermissionRequested(request)).await;

                // Wait for permission response
                // TODO: Implement async permission waiting
                // For now, break the loop
                break;
            }

            // Request permissions for each tool call
            // TODO: Implement async permission system
            // For now, auto-allow read/glob/grep, ask for others

            let mut approved_calls = Vec::new();
            for call in &pending_calls {
                let action = permission_checker.check_tool(&call.name);

                match action {
                    crate::config::PermissionAction::Allow => {
                        approved_calls.push(call.clone());
                    }
                    crate::config::PermissionAction::Deny => {
                        // Skip this call
                    }
                    crate::config::PermissionAction::Ask => {
                        // Request permission via dialog
                        let req_id = format!("tool_{}_{}", call.id, step);
                        let request = PermissionRequest {
                            id: req_id.clone(),
                            tool_name: call.name.clone(),
                            arguments: call.arguments.clone(),
                            description: format!("Allow tool '{}' to execute?", call.name),
                        };

                        let _ = event_tx.send(AppEvent::PermissionRequested(request)).await;

                        // Wait for permission response
                        // TODO: Implement proper async waiting
                        // For now, just skip
                    }
                }
            }

            if approved_calls.is_empty() {
                break;
            }

            // Add tool use parts to assistant message
            for call in &approved_calls {
                let args: serde_json::Value =
                    serde_json::from_str(&call.arguments).unwrap_or_else(|_| serde_json::json!({}));

                assistant_parts.push(ContentPart::ToolUse {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    input: args,
                });
            }

            // Add assistant message with tool calls
            messages.push(ChatMessage {
                role: "assistant".to_string(),
                content: if assistant_parts.len() == 1 {
                    match &assistant_parts[0] {
                        ContentPart::Text { text } => ChatContent::Text(text.clone()),
                        _ => ChatContent::Parts(assistant_parts.clone()),
                    }
                } else {
                    ChatContent::Parts(assistant_parts)
                },
            });

            // Execute tools in parallel
            let tool_results = tool::execute_all_tools_parallel(approved_calls, &tool_ctx).await;

            // Send tool results as events
            for result in &tool_results {
                if let ContentPart::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } = result
                {
                    let _ = event_tx
                        .send(AppEvent::ToolResult {
                            id: tool_use_id.clone(),
                            output: content.clone(),
                            is_error: is_error.unwrap_or(false),
                        })
                        .await;
                }
            }

            // Add tool results to conversation
            messages.push(ChatMessage {
                role: "user".to_string(),
                content: ChatContent::Parts(tool_results),
            });

            // Continue loop
        } else {
            // No tool calls - add final assistant message and exit loop
            messages.push(ChatMessage {
                role: "assistant".to_string(),
                content: if assistant_parts.len() == 1 {
                    match &assistant_parts[0] {
                        ContentPart::Text { text } => ChatContent::Text(text.clone()),
                        _ => ChatContent::Parts(assistant_parts),
                    }
                } else if !assistant_parts.is_empty() {
                    ChatContent::Parts(assistant_parts)
                } else {
                    ChatContent::Text(String::new())
                },
            });

            // Check finish reason
            if finish_reason == "stop" || finish_reason == "end_turn" || finish_reason == "length" {
                break;
            }
        }
    }

    let _ = event_tx.send(AppEvent::StreamDone).await;
    Ok(())
}

/// Stream a response from the LLM (simple, non-agentic)
pub async fn stream_response(
    provider_id: &str,
    model_id: &str,
    prompt: &str,
) -> Result<mpsc::Receiver<StreamEvent>> {
    let provider = provider::registry()
        .get(provider_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("Provider not found: {}", provider_id))?;

    let model = provider
        .models
        .get(model_id)
        .ok_or_else(|| anyhow::anyhow!("Model not found: {}", model_id))?;

    let api_key = provider
        .key
        .ok_or_else(|| anyhow::anyhow!("No API key for provider: {}", provider_id))?;

    let messages = vec![ChatMessage {
        role: "user".to_string(),
        content: ChatContent::Text(prompt.to_string()),
    }];

    let tools = tool::registry().definitions().await;
    let tool_defs: Vec<ToolDefinition> = tools
        .into_iter()
        .map(|t| ToolDefinition {
            name: t.name,
            description: t.description,
            input_schema: t.parameters,
        })
        .collect();

    let client = StreamingClient::new();

    match provider_id {
        "anthropic" => {
            client
                .stream_anthropic(
                    &api_key,
                    &model.api.id,
                    messages,
                    None,
                    tool_defs,
                    model.limit.output,
                )
                .await
        }
        "openai" => {
            let base_url = model
                .api
                .url
                .as_deref()
                .unwrap_or("https://api.openai.com/v1");
            client
                .stream_openai(
                    &api_key,
                    base_url,
                    &model.api.id,
                    messages,
                    tool_defs,
                    model.limit.output,
                )
                .await
        }
        "copilot" => {
            client
                .stream_copilot(
                    &api_key,
                    &model.api.id,
                    messages,
                    tool_defs,
                    model.limit.output,
                )
                .await
        }
        _ => Err(anyhow::anyhow!("Unsupported provider: {}", provider_id)),
    }
}
