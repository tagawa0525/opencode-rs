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

/// Send permission response to waiting tool
pub async fn send_permission_response(id: String, allow: bool, scope: tool::PermissionScope) {
    crate::permission_state::send_permission_response(id, allow, scope).await;
}

/// Context for streaming operations
struct StreamContext {
    provider_id: String,
    api_key: String,
    model: Model,
    tool_defs: Vec<ToolDefinition>,
    tool_ctx: Arc<ToolContext>,
    permission_checker: PermissionChecker,
    event_tx: mpsc::Sender<AppEvent>,
    system_prompt: String,
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

    // Create TUI permission handler using shared implementation
    let permission_handler =
        crate::permission_state::create_tui_permission_handler(event_tx.clone());

    let tool_ctx = Arc::new(
        ToolContext::new("", "", "tui")
            .with_cwd(cwd.clone())
            .with_root(cwd.clone())
            .with_permission_handler(permission_handler),
    );

    // Generate system prompt
    let system_prompt = crate::session::system::generate(&cwd, provider_id, model_id);

    Ok(StreamContext {
        provider_id: provider_id.to_string(),
        api_key,
        model,
        tool_defs,
        tool_ctx,
        permission_checker,
        event_tx,
        system_prompt,
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
                    Some(ctx.system_prompt.clone()),
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
                    Some(ctx.system_prompt.clone()),
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
                    Some(ctx.system_prompt.clone()),
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
    _step: i32,
) -> Result<bool> {
    // Check for doom loop
    doom_detector.add_calls(&result.pending_calls);

    if let Some((tool_name, args)) = doom_detector.check_doom_loop() {
        // TODO: Implement doom loop handling
        let _ = ctx
            .event_tx
            .send(AppEvent::StreamError(format!(
                "Doom loop detected: {} called repeatedly with same arguments: {}",
                tool_name, args
            )))
            .await;
        return Ok(false);
    }

    // Add tool use parts to assistant message
    for call in &result.pending_calls {
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

    // Execute tools - permission checks happen inside each tool's execute method
    let tool_results = execute_tools(ctx, result.pending_calls).await;

    // Add tool results to conversation
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: ChatContent::Parts(tool_results),
    });

    Ok(true)
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
    let content = if assistant_parts.is_empty() {
        ChatContent::Text(String::new())
    } else {
        build_chat_content(assistant_parts)
    };

    messages.push(ChatMessage {
        role: "assistant".to_string(),
        content,
    });

    let should_continue = !is_terminal_finish_reason(&result.finish_reason);
    Ok(should_continue)
}

/// Check if finish reason indicates stream should stop
fn is_terminal_finish_reason(reason: &str) -> bool {
    matches!(reason, "stop" | "end_turn" | "length")
}

/// Stream a response from the LLM (simple, non-agentic)
pub async fn stream_response(
    provider_id: &str,
    model_id: &str,
    prompt: &str,
) -> Result<mpsc::Receiver<StreamEvent>> {
    let (api_key, model) = get_provider_credentials(provider_id, model_id).await?;

    let messages = vec![ChatMessage {
        role: "user".to_string(),
        content: ChatContent::Text(prompt.to_string()),
    }];

    let tool_defs = get_tool_definitions().await;
    let system_prompt = generate_system_prompt(provider_id, model_id);

    let client = StreamingClient::new();
    dispatch_stream(
        &client,
        provider_id,
        &api_key,
        &model,
        messages,
        system_prompt,
        tool_defs,
    )
    .await
}

/// Get provider credentials (API key and model)
async fn get_provider_credentials(provider_id: &str, model_id: &str) -> Result<(String, Model)> {
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

    Ok((api_key, model))
}

/// Get tool definitions from registry
async fn get_tool_definitions() -> Vec<ToolDefinition> {
    tool::registry()
        .definitions()
        .await
        .into_iter()
        .map(|t| ToolDefinition {
            name: t.name,
            description: t.description,
            input_schema: t.parameters,
        })
        .collect()
}

/// Generate system prompt for current working directory
fn generate_system_prompt(provider_id: &str, model_id: &str) -> String {
    let cwd = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(String::from))
        .unwrap_or_else(|| ".".to_string());
    crate::session::system::generate(&cwd, provider_id, model_id)
}

/// Dispatch stream request to appropriate provider
async fn dispatch_stream(
    client: &StreamingClient,
    provider_id: &str,
    api_key: &str,
    model: &Model,
    messages: Vec<ChatMessage>,
    system_prompt: String,
    tool_defs: Vec<ToolDefinition>,
) -> Result<mpsc::Receiver<StreamEvent>> {
    match provider_id {
        "anthropic" => {
            client
                .stream_anthropic(
                    api_key,
                    &model.api.id,
                    messages,
                    Some(system_prompt),
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
                    api_key,
                    base_url,
                    &model.api.id,
                    messages,
                    Some(system_prompt),
                    tool_defs,
                    model.limit.output,
                )
                .await
        }
        "copilot" => {
            client
                .stream_copilot(
                    api_key,
                    &model.api.id,
                    messages,
                    Some(system_prompt),
                    tool_defs,
                    model.limit.output,
                )
                .await
        }
        _ => Err(anyhow::anyhow!("Unsupported provider: {}", provider_id)),
    }
}
