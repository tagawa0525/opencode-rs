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
    self, ChatContent, ChatMessage, ContentPart, Model, StreamEvent, StreamingClient,
    ToolDefinition,
};
use crate::tool::{self, DoomLoopDetector, PendingToolCall, ToolCallTracker, ToolContext};

/// Context for streaming operations
struct StreamContext {
    provider_id: String,
    api_key: String,
    model: Model,
    tool_defs: Vec<ToolDefinition>,
    tool_ctx: Arc<ToolContext>,
    permission_checker: PermissionChecker,
    event_tx: mpsc::Sender<AppEvent>,
}

/// Result of processing a stream
struct StreamResult {
    response_text: String,
    pending_calls: Vec<PendingToolCall>,
    finish_reason: String,
}

/// Stream a response from the LLM with agentic loop
pub async fn stream_response_agentic(
    provider_id: String,
    model_id: String,
    initial_prompt: String,
    event_tx: mpsc::Sender<AppEvent>,
) -> Result<()> {
    // Initialize context
    let ctx = initialize_stream_context(&provider_id, &model_id, event_tx).await?;

    // Initialize conversation with user message
    let mut messages = vec![ChatMessage {
        role: "user".to_string(),
        content: ChatContent::Text(initial_prompt),
    }];

    let client = StreamingClient::new();
    let mut step = 0;
    let max_steps = 10;
    let mut doom_detector = DoomLoopDetector::new();

    loop {
        step += 1;
        if step > max_steps {
            let _ = ctx
                .event_tx
                .send(AppEvent::StreamError(
                    "Maximum agentic loop steps reached".to_string(),
                ))
                .await;
            break;
        }

        // Stream the response
        let rx = create_provider_stream(&client, &ctx, &messages).await?;

        // Process the stream
        let result = process_stream(rx, &ctx.event_tx).await?;

        // Handle the result
        let should_continue =
            handle_stream_result(&ctx, &mut messages, result, &mut doom_detector, step).await?;

        if !should_continue {
            break;
        }
    }

    let _ = ctx.event_tx.send(AppEvent::StreamDone).await;
    Ok(())
}

/// Initialize the streaming context
async fn initialize_stream_context(
    provider_id: &str,
    model_id: &str,
    event_tx: mpsc::Sender<AppEvent>,
) -> Result<StreamContext> {
    let config = Config::load().await?;
    let permission_checker = PermissionChecker::from_config(&config);

    let provider = provider::registry()
        .get(provider_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("Provider not found: {}", provider_id))?;

    let model = provider
        .models
        .get(model_id)
        .ok_or_else(|| anyhow::anyhow!("Model not found: {}", model_id))?
        .clone();

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
    let cwd = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(String::from))
        .unwrap_or_else(|| ".".to_string());

    let tool_ctx = Arc::new(ToolContext {
        session_id: String::new(),
        message_id: String::new(),
        agent: "tui".to_string(),
        abort: None,
        cwd: cwd.clone(),
        root: cwd,
        extra: Default::default(),
    });

    Ok(StreamContext {
        provider_id: provider_id.to_string(),
        api_key,
        model,
        tool_defs,
        tool_ctx,
        permission_checker,
        event_tx,
    })
}

/// Create a provider-specific stream
async fn create_provider_stream(
    client: &StreamingClient,
    ctx: &StreamContext,
    messages: &[ChatMessage],
) -> Result<mpsc::Receiver<StreamEvent>> {
    match ctx.provider_id.as_str() {
        "anthropic" => {
            client
                .stream_anthropic(
                    &ctx.api_key,
                    &ctx.model.api.id,
                    messages.to_vec(),
                    None,
                    ctx.tool_defs.clone(),
                    ctx.model.limit.output,
                )
                .await
        }
        "openai" => {
            let base_url = ctx
                .model
                .api
                .url
                .as_deref()
                .unwrap_or("https://api.openai.com/v1");
            client
                .stream_openai(
                    &ctx.api_key,
                    base_url,
                    &ctx.model.api.id,
                    messages.to_vec(),
                    ctx.tool_defs.clone(),
                    ctx.model.limit.output,
                )
                .await
        }
        "copilot" => {
            client
                .stream_copilot(
                    &ctx.api_key,
                    &ctx.model.api.id,
                    messages.to_vec(),
                    ctx.tool_defs.clone(),
                    ctx.model.limit.output,
                )
                .await
        }
        _ => Err(anyhow::anyhow!("Unsupported provider: {}", ctx.provider_id)),
    }
}

/// Process stream events and collect results
async fn process_stream(
    mut rx: mpsc::Receiver<StreamEvent>,
    event_tx: &mpsc::Sender<AppEvent>,
) -> Result<StreamResult> {
    let mut response_text = String::new();
    let mut tool_tracker = ToolCallTracker::new();
    let mut finish_reason = String::new();

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

    Ok(StreamResult {
        response_text,
        pending_calls: tool_tracker.get_all_calls(),
        finish_reason,
    })
}

/// Handle stream result and execute tools if needed
async fn handle_stream_result(
    ctx: &StreamContext,
    messages: &mut Vec<ChatMessage>,
    result: StreamResult,
    doom_detector: &mut DoomLoopDetector,
    step: i32,
) -> Result<bool> {
    let mut assistant_parts: Vec<ContentPart> = Vec::new();

    if !result.response_text.is_empty() {
        assistant_parts.push(ContentPart::Text {
            text: result.response_text.clone(),
        });
    }

    if !result.pending_calls.is_empty() {
        // Handle tool calls
        handle_tool_calls(ctx, messages, result, assistant_parts, doom_detector, step).await
    } else {
        // No tool calls - add final assistant message and exit
        handle_final_response(messages, result, assistant_parts)
    }
}

/// Handle tool calls from the LLM
async fn handle_tool_calls(
    ctx: &StreamContext,
    messages: &mut Vec<ChatMessage>,
    result: StreamResult,
    mut assistant_parts: Vec<ContentPart>,
    doom_detector: &mut DoomLoopDetector,
    step: i32,
) -> Result<bool> {
    // Check for doom loop
    doom_detector.add_calls(&result.pending_calls);

    if let Some((tool_name, args)) = doom_detector.check_doom_loop() {
        return handle_doom_loop(ctx, &tool_name, &args, step).await;
    }

    // Check permissions for each tool call
    let approved_calls = check_tool_permissions(ctx, &result.pending_calls, step).await;

    if approved_calls.is_empty() {
        return Ok(false);
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
        content: build_chat_content(assistant_parts),
    });

    // Execute tools
    let tool_results = execute_tools(ctx, approved_calls).await;

    // Add tool results to conversation
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: ChatContent::Parts(tool_results),
    });

    Ok(true)
}

/// Handle doom loop detection
async fn handle_doom_loop(
    ctx: &StreamContext,
    tool_name: &str,
    args: &str,
    step: i32,
) -> Result<bool> {
    let req_id = format!("doom_loop_{}", step);
    let request = PermissionRequest {
        id: req_id,
        tool_name: "doom_loop".to_string(),
        arguments: format!("Tool '{}' with args: {}", tool_name, args),
        description: format!(
            "Doom loop detected: '{}' called {} times with identical arguments",
            tool_name,
            tool::DOOM_LOOP_THRESHOLD
        ),
    };

    let _ = ctx
        .event_tx
        .send(AppEvent::PermissionRequested(request))
        .await;

    // TODO: Implement async permission waiting
    // For now, break the loop
    Ok(false)
}

/// Check permissions for tool calls
async fn check_tool_permissions(
    ctx: &StreamContext,
    calls: &[PendingToolCall],
    step: i32,
) -> Vec<PendingToolCall> {
    let mut approved_calls = Vec::new();

    for call in calls {
        let action = ctx.permission_checker.check_tool(&call.name);

        match action {
            crate::config::PermissionAction::Allow => {
                approved_calls.push(call.clone());
            }
            crate::config::PermissionAction::Deny => {
                // Skip this call
            }
            crate::config::PermissionAction::Ask => {
                request_tool_permission(ctx, call, step).await;
                // TODO: Implement proper async waiting
            }
        }
    }

    approved_calls
}

/// Request permission for a tool call
async fn request_tool_permission(ctx: &StreamContext, call: &PendingToolCall, step: i32) {
    let req_id = format!("tool_{}_{}", call.id, step);
    let request = PermissionRequest {
        id: req_id,
        tool_name: call.name.clone(),
        arguments: call.arguments.clone(),
        description: format!("Allow tool '{}' to execute?", call.name),
    };

    let _ = ctx
        .event_tx
        .send(AppEvent::PermissionRequested(request))
        .await;
}

/// Execute approved tools
async fn execute_tools(
    ctx: &StreamContext,
    approved_calls: Vec<PendingToolCall>,
) -> Vec<ContentPart> {
    let tool_results = tool::execute_all_tools_parallel(approved_calls, &ctx.tool_ctx).await;

    // Send tool results as events
    for result in &tool_results {
        if let ContentPart::ToolResult {
            tool_use_id,
            content,
            is_error,
        } = result
        {
            let _ = ctx
                .event_tx
                .send(AppEvent::ToolResult {
                    id: tool_use_id.clone(),
                    output: content.clone(),
                    is_error: is_error.unwrap_or(false),
                })
                .await;
        }
    }

    tool_results
}

/// Build ChatContent from parts
fn build_chat_content(parts: Vec<ContentPart>) -> ChatContent {
    if parts.len() == 1 {
        if let ContentPart::Text { text } = &parts[0] {
            return ChatContent::Text(text.clone());
        }
    }
    ChatContent::Parts(parts)
}

/// Handle final response when no tool calls
fn handle_final_response(
    messages: &mut Vec<ChatMessage>,
    result: StreamResult,
    assistant_parts: Vec<ContentPart>,
) -> Result<bool> {
    let content = if assistant_parts.len() == 1 {
        if let ContentPart::Text { text } = &assistant_parts[0] {
            ChatContent::Text(text.clone())
        } else {
            ChatContent::Parts(assistant_parts)
        }
    } else if !assistant_parts.is_empty() {
        ChatContent::Parts(assistant_parts)
    } else {
        ChatContent::Text(String::new())
    };

    messages.push(ChatMessage {
        role: "assistant".to_string(),
        content,
    });

    // Check finish reason
    let should_stop = result.finish_reason == "stop"
        || result.finish_reason == "end_turn"
        || result.finish_reason == "length";

    Ok(!should_stop)
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
