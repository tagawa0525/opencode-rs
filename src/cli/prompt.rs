//! Prompt command - runs a single prompt without TUI.

use crate::config::Config;
use crate::permission::PermissionChecker;
use crate::provider::{self, ChatContent, ChatMessage, ContentPart, StreamEvent, ToolDefinition};
use crate::session::{CreateSessionOptions, ModelRef, Session};
use crate::tool::{self, DoomLoopDetector, PendingToolCall, ToolCallTracker, ToolContext};
use anyhow::Result;
use tokio::sync::mpsc;

/// Context for prompt execution
struct PromptContext {
    provider_id: String,
    model_id: String,
    api_key: String,
    model_api_id: String,
    model_api_url: Option<String>,
    max_tokens: u64,
    tool_defs: Vec<ToolDefinition>,
    tool_ctx: ToolContext,
    permission_checker: PermissionChecker,
    format: String,
}

/// Result of processing a stream
struct StreamResult {
    response_text: String,
    pending_calls: Vec<PendingToolCall>,
    finish_reason: String,
}

/// Execute a single prompt without TUI (with agentic loop)
pub async fn execute(prompt: &str, model: Option<&str>, format: &str) -> Result<()> {
    // Initialize context
    let (ctx, mut session) = initialize_context(model, format).await?;

    // Initialize conversation history
    let mut messages: Vec<ChatMessage> = vec![ChatMessage {
        role: "user".to_string(),
        content: ChatContent::Text(prompt.to_string()),
    }];

    // Create streaming client
    let client = provider::StreamingClient::new();

    // Agentic loop
    let mut step = 0;
    let max_steps = 10;
    let mut doom_detector = DoomLoopDetector::new();

    loop {
        step += 1;
        if step > max_steps {
            eprintln!("\n[Warning: Maximum agentic loop steps reached]");
            break;
        }

        if ctx.format == "text" && step > 1 {
            eprintln!("\n[Agentic step {}]", step);
        }

        // Stream the response
        let rx = create_provider_stream(&client, &ctx, &messages).await?;

        // Process the stream
        let result = process_stream(rx, &ctx.format).await?;

        // Handle the result
        let should_continue =
            handle_stream_result(&ctx, &mut messages, result, &mut doom_detector).await?;

        if !should_continue {
            break;
        }
    }

    // Output and save
    output_result(&messages, step, format);
    save_model_to_session(&mut session, &ctx.provider_id, &ctx.model_id).await;

    Ok(())
}

/// Initialize the prompt context with config, provider, and tools
async fn initialize_context(model: Option<&str>, format: &str) -> Result<(PromptContext, Session)> {
    // Load configuration
    let config = Config::load().await?;

    // Initialize provider registry
    provider::registry().initialize(&config).await?;

    // Create a session
    let session = Session::create(CreateSessionOptions::default()).await?;

    // Resolve model
    let (provider_id, model_id) = resolve_model(model, &session, &config).await?;

    // Get model info
    let model_info = provider::registry()
        .get_model(&provider_id, &model_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("Model not found: {}/{}", provider_id, model_id))?;

    // Get API key
    let provider_info = provider::registry()
        .get(&provider_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("Provider not found: {}", provider_id))?;

    let api_key = provider_info
        .key
        .ok_or_else(|| anyhow::anyhow!("No API key for provider: {}", provider_id))?;

    // Create permission checker
    let permission_checker = PermissionChecker::from_config(&config);

    // Create tool context
    let cwd = std::env::current_dir()?.to_string_lossy().to_string();
    let tool_ctx = ToolContext::new("cli-session", "msg-1", "default")
        .with_cwd(cwd.clone())
        .with_root(cwd);

    // Get tool definitions
    let tools = tool::registry().definitions().await;
    let tool_defs: Vec<ToolDefinition> = tools
        .into_iter()
        .map(|t| ToolDefinition {
            name: t.name,
            description: t.description,
            input_schema: t.parameters,
        })
        .collect();

    Ok((
        PromptContext {
            provider_id,
            model_id,
            api_key,
            model_api_id: model_info.api.id.clone(),
            model_api_url: model_info.api.url.clone(),
            max_tokens: model_info.limit.output,
            tool_defs,
            tool_ctx,
            permission_checker,
            format: format.to_string(),
        },
        session,
    ))
}

/// Resolve which model to use based on priority
async fn resolve_model(
    model: Option<&str>,
    session: &Session,
    config: &Config,
) -> Result<(String, String)> {
    if let Some(m) = model {
        // CLI argument takes highest priority
        return provider::parse_model_string(m)
            .ok_or_else(|| anyhow::anyhow!("Invalid model format. Use 'provider/model'"));
    }

    if let Some(session_model) = session.get_model().await {
        // Session model is second priority
        return Ok((session_model.provider_id, session_model.model_id));
    }

    if let Some(configured_model) = config.model.as_ref() {
        // Workspace/global config is third priority
        return provider::parse_model_string(configured_model)
            .ok_or_else(|| anyhow::anyhow!("Invalid model format in config"));
    }

    // Fall back to last used model from global storage
    match crate::storage::global()
        .read::<String>(&["state", "last_model"])
        .await
    {
        Ok(Some(last_model)) => provider::parse_model_string(&last_model)
            .ok_or_else(|| anyhow::anyhow!("Invalid last used model format")),
        Ok(None) | Err(_) => Err(anyhow::anyhow!(
            "No model configured. Set a default model in config or use --model flag"
        )),
    }
}

/// Create a provider-specific stream
async fn create_provider_stream(
    client: &provider::StreamingClient,
    ctx: &PromptContext,
    messages: &[ChatMessage],
) -> Result<mpsc::Receiver<StreamEvent>> {
    match ctx.provider_id.as_str() {
        "anthropic" => {
            client
                .stream_anthropic(
                    &ctx.api_key,
                    &ctx.model_api_id,
                    messages.to_vec(),
                    None,
                    ctx.tool_defs.clone(),
                    ctx.max_tokens,
                )
                .await
        }
        "openai" => {
            let base_url = ctx
                .model_api_url
                .as_deref()
                .unwrap_or("https://api.openai.com/v1");
            client
                .stream_openai(
                    &ctx.api_key,
                    base_url,
                    &ctx.model_api_id,
                    messages.to_vec(),
                    ctx.tool_defs.clone(),
                    ctx.max_tokens,
                )
                .await
        }
        "copilot" => {
            client
                .stream_copilot(
                    &ctx.api_key,
                    &ctx.model_api_id,
                    messages.to_vec(),
                    ctx.tool_defs.clone(),
                    ctx.max_tokens,
                )
                .await
        }
        _ => Err(anyhow::anyhow!("Unsupported provider: {}", ctx.provider_id)),
    }
}

/// Process stream events and collect results
async fn process_stream(mut rx: mpsc::Receiver<StreamEvent>, format: &str) -> Result<StreamResult> {
    let mut response_text = String::new();
    let mut tool_tracker = ToolCallTracker::new();
    let mut finish_reason = String::new();

    while let Some(event) = rx.recv().await {
        match event {
            StreamEvent::TextDelta(text) => {
                handle_text_delta(&text, format, &mut response_text);
            }
            StreamEvent::ReasoningDelta(text) => {
                handle_reasoning_delta(&text, format);
            }
            StreamEvent::ToolCallStart { id, name } => {
                if format == "text" {
                    println!("\n[Calling tool: {}]", name);
                }
                tool_tracker.start_call(id, name);
            }
            StreamEvent::ToolCallDelta {
                id,
                arguments_delta,
            } => {
                tool_tracker.add_arguments(&id, &arguments_delta);
            }
            StreamEvent::ToolCallEnd { id } => {
                if format == "text" {
                    println!("[Tool call {} complete]", id);
                }
            }
            StreamEvent::Usage {
                input_tokens,
                output_tokens,
                ..
            } => {
                if format == "text" {
                    eprintln!("[Tokens: {} in, {} out]", input_tokens, output_tokens);
                }
            }
            StreamEvent::Done {
                finish_reason: reason,
            } => {
                finish_reason = reason;
            }
            StreamEvent::Error(err) => {
                eprintln!("\nError: {}", err);
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

/// Handle text delta event
fn handle_text_delta(text: &str, format: &str, response_text: &mut String) {
    if format == "text" {
        print!("{}", text);
        use std::io::Write;
        let _ = std::io::stdout().flush();
    }
    response_text.push_str(text);
}

/// Handle reasoning delta event
fn handle_reasoning_delta(text: &str, format: &str) {
    if format == "text" {
        print!("\x1b[2m{}\x1b[0m", text); // dim
        use std::io::Write;
        let _ = std::io::stdout().flush();
    }
}

/// Handle stream result and execute tools if needed
async fn handle_stream_result(
    ctx: &PromptContext,
    messages: &mut Vec<ChatMessage>,
    result: StreamResult,
    doom_detector: &mut DoomLoopDetector,
) -> Result<bool> {
    let mut assistant_parts: Vec<ContentPart> = Vec::new();

    if !result.response_text.is_empty() {
        assistant_parts.push(ContentPart::Text {
            text: result.response_text.clone(),
        });
    }

    if !result.pending_calls.is_empty() {
        // Handle tool calls
        handle_tool_calls(ctx, messages, result, assistant_parts, doom_detector).await
    } else {
        // No tool calls - add final assistant message and exit
        handle_final_response(messages, result)
    }
}

/// Handle tool calls from the LLM
async fn handle_tool_calls(
    ctx: &PromptContext,
    messages: &mut Vec<ChatMessage>,
    result: StreamResult,
    mut assistant_parts: Vec<ContentPart>,
    doom_detector: &mut DoomLoopDetector,
) -> Result<bool> {
    // Check for doom loop
    doom_detector.add_calls(&result.pending_calls);

    if let Some((tool_name, args)) = doom_detector.check_doom_loop() {
        if !handle_doom_loop(ctx, &tool_name, &args).await? {
            return Ok(false);
        }
    }

    // Check permissions for each tool call
    let approved_calls = check_tool_permissions(ctx, &result.pending_calls).await?;

    if approved_calls.is_empty() {
        eprintln!("[No tools approved - stopping execution]");
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
    messages.push(tool::build_tool_result_message(tool_results));

    Ok(true)
}

/// Handle doom loop detection
async fn handle_doom_loop(ctx: &PromptContext, tool_name: &str, args: &str) -> Result<bool> {
    eprintln!("\n[WARNING: Doom loop detected!]");
    eprintln!(
        "[The LLM has called '{}' with identical arguments {} times in a row]",
        tool_name,
        tool::DOOM_LOOP_THRESHOLD
    );
    eprintln!("[Arguments: {}]", args);
    eprintln!("[This may indicate the LLM is stuck.]");

    let allowed = ctx
        .permission_checker
        .check_doom_loop_and_ask_cli(tool_name, args)
        .await?;

    if !allowed {
        eprintln!("[User declined doom loop continuation - stopping execution]");
        return Ok(false);
    }

    eprintln!("[User approved - continuing execution]");
    Ok(true)
}

/// Check permissions for tool calls
async fn check_tool_permissions(
    ctx: &PromptContext,
    calls: &[PendingToolCall],
) -> Result<Vec<PendingToolCall>> {
    let mut approved_calls = Vec::new();

    for call in calls {
        let allowed = ctx
            .permission_checker
            .check_and_ask_cli(&call.name, &call.arguments)
            .await?;

        if allowed {
            approved_calls.push(call.clone());
        } else {
            eprintln!("[User denied execution of tool: {}]", call.name);
        }
    }

    Ok(approved_calls)
}

/// Execute approved tools
async fn execute_tools(
    ctx: &PromptContext,
    approved_calls: Vec<PendingToolCall>,
) -> Vec<ContentPart> {
    if ctx.format == "text" {
        println!(
            "\n[Executing {} tool(s) in parallel...]",
            approved_calls.len()
        );
    }

    let tool_results = tool::execute_all_tools_parallel(approved_calls, &ctx.tool_ctx).await;

    // Show tool results in text format
    if ctx.format == "text" {
        print_tool_results(&tool_results);
    }

    tool_results
}

/// Print tool results to console
fn print_tool_results(results: &[ContentPart]) {
    for result in results {
        if let ContentPart::ToolResult {
            tool_use_id,
            content,
            is_error,
        } = result
        {
            let status = if is_error.unwrap_or(false) {
                "ERROR"
            } else {
                "OK"
            };
            println!("[Tool {} result: {}]", tool_use_id, status);

            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(content) {
                if let Some(title) = parsed.get("title") {
                    println!("  {}", title);
                }
                if let Some(output) = parsed.get("output") {
                    if let Some(output_str) = output.as_str() {
                        let preview = if output_str.len() > 200 {
                            format!("{}...", &output_str[..200])
                        } else {
                            output_str.to_string()
                        };
                        println!("  {}", preview);
                    }
                }
            }
        }
    }
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
fn handle_final_response(messages: &mut Vec<ChatMessage>, result: StreamResult) -> Result<bool> {
    if !result.response_text.is_empty() {
        messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: ChatContent::Text(result.response_text),
        });
    }

    if result.finish_reason == "tool_calls" {
        eprintln!("[Warning: LLM indicated tool_calls but no tools found]");
    }

    println!();
    Ok(false)
}

/// Output result in requested format
fn output_result(messages: &[ChatMessage], step: i32, format: &str) {
    if format == "json" {
        let output = serde_json::json!({
            "messages": messages,
            "steps": step,
        });
        if let Ok(json) = serde_json::to_string_pretty(&output) {
            println!("{}", json);
        }
    }
    // For "text" and "markdown", output was already printed during streaming
}

/// Save model to session and global storage
async fn save_model_to_session(session: &mut Session, provider_id: &str, model_id: &str) {
    let model_ref = ModelRef {
        provider_id: provider_id.to_string(),
        model_id: model_id.to_string(),
    };

    if let Err(e) = session
        .set_model(&session.project_id.clone(), model_ref)
        .await
    {
        tracing::warn!("Failed to save model to session: {}", e);
    }

    // Save last used model to global storage
    let model_string = format!("{}/{}", provider_id, model_id);
    if let Err(e) = crate::storage::global()
        .write(&["state", "last_model"], &model_string)
        .await
    {
        tracing::warn!("Failed to save last used model: {}", e);
    }
}
