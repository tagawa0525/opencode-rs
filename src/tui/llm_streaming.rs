//! LLM streaming handlers for the TUI.
//!
//! This module contains functions for streaming responses from LLM providers
//! with support for tool calling and agentic loops.

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::mpsc;

use super::types::AppEvent;
use crate::provider::{
    self, ChatContent, ChatMessage, ContentPart, Model, OpenAIRequest, StreamEvent,
    StreamingClient, ToolDefinition,
};
use crate::tool::{self, DoomLoopDetector, PendingToolCall, ToolCallTracker, ToolContext};

const MAX_AGENTIC_STEPS: i32 = 10;
const DEFAULT_OPENAI_URL: &str = "https://api.openai.com/v1";

/// Send permission response to waiting tool
pub async fn send_permission_response(id: String, allow: bool, scope: tool::PermissionScope) {
    crate::permission_state::send_permission_response(id, allow, scope).await;
}

/// Send question response to waiting tool
pub async fn send_question_response(id: String, answers: Vec<Vec<String>>) {
    crate::question_state::send_question_response(id, answers).await;
}

/// Context for streaming operations
struct StreamContext {
    provider_id: String,
    api_key: String,
    model: Model,
    tool_defs: Vec<ToolDefinition>,
    tool_ctx: Arc<ToolContext>,
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
    let ctx = StreamContext::new(&provider_id, &model_id, event_tx).await?;
    let mut messages = vec![ChatMessage {
        role: "user".to_string(),
        content: ChatContent::Text(initial_prompt),
    }];
    let client = StreamingClient::new();
    let mut doom_detector = DoomLoopDetector::new();

    for step in 1..=MAX_AGENTIC_STEPS {
        let rx = ctx.create_stream(&client, &messages).await?;
        let result = process_stream(rx, &ctx.event_tx).await?;

        if !handle_stream_result(&ctx, &mut messages, result, &mut doom_detector, step).await? {
            break;
        }
    }

    if messages.len() > MAX_AGENTIC_STEPS as usize * 2 {
        let _ = ctx
            .event_tx
            .send(AppEvent::StreamError(
                "Maximum agentic loop steps reached".to_string(),
            ))
            .await;
    }

    let _ = ctx.event_tx.send(AppEvent::StreamDone).await;
    Ok(())
}

impl StreamContext {
    async fn new(
        provider_id: &str,
        model_id: &str,
        event_tx: mpsc::Sender<AppEvent>,
    ) -> Result<Self> {
        let (api_key, model) = get_provider_credentials(provider_id, model_id).await?;
        let tool_defs = get_tool_definitions().await;
        let cwd = get_current_dir();
        let system_prompt = crate::session::system::generate(&cwd, provider_id, model_id);

        let permission_handler =
            crate::permission_state::create_tui_permission_handler(event_tx.clone());
        let question_handler = crate::question_state::create_tui_question_handler(event_tx.clone());

        let tool_ctx = Arc::new(
            ToolContext::new("", "")
                .with_cwd(cwd.clone())
                .with_root(cwd)
                .with_permission_handler(permission_handler)
                .with_question_handler(question_handler),
        );

        Ok(Self {
            provider_id: provider_id.to_string(),
            api_key,
            model,
            tool_defs,
            tool_ctx,
            event_tx,
            system_prompt,
        })
    }

    async fn create_stream(
        &self,
        client: &StreamingClient,
        messages: &[ChatMessage],
    ) -> Result<mpsc::Receiver<StreamEvent>> {
        dispatch_to_provider(
            client,
            &self.provider_id,
            &self.api_key,
            &self.model,
            messages.to_vec(),
            &self.system_prompt,
            &self.tool_defs,
        )
        .await
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
    _step: i32,
) -> Result<bool> {
    let mut assistant_parts = build_text_parts(&result.response_text);

    if result.pending_calls.is_empty() {
        add_assistant_message(messages, assistant_parts);
        return Ok(!is_terminal_finish_reason(&result.finish_reason));
    }

    // Check for doom loop
    doom_detector.add_calls(&result.pending_calls);
    if let Some((tool_name, args)) = doom_detector.check_doom_loop() {
        let _ = ctx
            .event_tx
            .send(AppEvent::StreamError(format!(
                "Doom loop detected: {} called repeatedly with same arguments: {}",
                tool_name, args
            )))
            .await;
        return Ok(false);
    }

    // Add tool use parts and execute tools
    for call in &result.pending_calls {
        let args: serde_json::Value =
            serde_json::from_str(&call.arguments).unwrap_or_else(|_| serde_json::json!({}));
        assistant_parts.push(ContentPart::ToolUse {
            id: call.id.clone(),
            name: call.name.clone(),
            input: args,
        });
    }

    add_assistant_message(messages, assistant_parts);

    let tool_results = execute_tools(ctx, result.pending_calls).await;
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: ChatContent::Parts(tool_results),
    });

    Ok(true)
}

fn build_text_parts(text: &str) -> Vec<ContentPart> {
    if text.is_empty() {
        Vec::new()
    } else {
        vec![ContentPart::Text {
            text: text.to_string(),
        }]
    }
}

fn add_assistant_message(messages: &mut Vec<ChatMessage>, parts: Vec<ContentPart>) {
    let content = match parts.len() {
        0 => ChatContent::Text(String::new()),
        1 if matches!(&parts[0], ContentPart::Text { .. }) => {
            if let ContentPart::Text { text } = &parts[0] {
                ChatContent::Text(text.clone())
            } else {
                ChatContent::Parts(parts)
            }
        }
        _ => ChatContent::Parts(parts),
    };

    messages.push(ChatMessage {
        role: "assistant".to_string(),
        content,
    });
}

/// Execute approved tools
async fn execute_tools(ctx: &StreamContext, calls: Vec<PendingToolCall>) -> Vec<ContentPart> {
    let tool_results = tool::execute_all_tools_parallel(calls, &ctx.tool_ctx).await;

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
    let cwd = get_current_dir();
    let system_prompt = crate::session::system::generate(&cwd, provider_id, model_id);

    let client = StreamingClient::new();
    dispatch_to_provider(
        &client,
        provider_id,
        &api_key,
        &model,
        messages,
        &system_prompt,
        &tool_defs,
    )
    .await
}

// --- Shared utility functions ---

fn get_current_dir() -> String {
    std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(String::from))
        .unwrap_or_else(|| ".".to_string())
}

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

async fn dispatch_to_provider(
    client: &StreamingClient,
    provider_id: &str,
    api_key: &str,
    model: &Model,
    messages: Vec<ChatMessage>,
    system_prompt: &str,
    tool_defs: &[ToolDefinition],
) -> Result<mpsc::Receiver<StreamEvent>> {
    match provider_id {
        "anthropic" => {
            client
                .stream_anthropic(
                    api_key,
                    &model.api.id,
                    messages,
                    Some(system_prompt.to_string()),
                    tool_defs.to_vec(),
                    model.limit.output,
                )
                .await
        }
        "openai" => {
            let base_url = model.api.url.as_deref().unwrap_or(DEFAULT_OPENAI_URL);
            let request = OpenAIRequest {
                messages,
                system: Some(system_prompt.to_string()),
                tools: tool_defs.to_vec(),
                max_tokens: model.limit.output,
            };
            client
                .stream_openai(api_key, base_url, &model.api.id, request)
                .await
        }
        "copilot" => {
            client
                .stream_copilot(
                    api_key,
                    &model.api.id,
                    messages,
                    Some(system_prompt.to_string()),
                    tool_defs.to_vec(),
                    model.limit.output,
                )
                .await
        }
        _ => Err(anyhow::anyhow!("Unsupported provider: {}", provider_id)),
    }
}
